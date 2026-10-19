#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;
use gusset_kernel::mock::MockKernel;
use gusset_kernel::{
    BoolOp, ExtrudeSpec, Frame, Kernel, KernelErrorKind, Loop2, Profile, ProfileEdge, Region2, Seg2, TessellationParams,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

type Servers = Arc<Mutex<Vec<Arc<Mutex<Server<MockKernel>>>>>>;

/// A worker in this process: every request goes through the codec and the server.
#[derive(Debug)]
struct Local {
    server: Arc<Mutex<Server<MockKernel>>>,
    /// Requests left before this "worker" dies (simulated crash).
    lives: Arc<AtomicU32>,
}

impl Transport for Local {
    fn call(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError> {
        if self.lives.fetch_sub(1, Ordering::SeqCst) == 0 {
            self.lives.store(0, Ordering::SeqCst);
            return Err(TransportError::Died { reason: "killed by signal 11".into() });
        }
        Ok(self.server.lock().unwrap().handle(request))
    }
}

/// A remote kernel whose workers are in-process servers; returns the kernel, the list of
/// servers it started, and the "lives" counter of the current one.
fn remote(lives: u32) -> (RemoteKernel, Servers, Arc<AtomicU32>) {
    let servers: Servers = Arc::default();
    let counter = Arc::new(AtomicU32::new(lives));
    let (s2, c2) = (servers.clone(), counter.clone());
    let k = RemoteKernel::new(
        "mock",
        Box::new(move || {
            let server = Arc::new(Mutex::new(Server::new(MockKernel)));
            s2.lock().unwrap().push(server.clone());
            Ok(Box::new(Local { server, lives: c2.clone() }) as Box<dyn Transport>)
        }),
        Box::new(crate::UnvettedNatives),
    );
    (k, servers, counter)
}

fn square() -> Profile {
    let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let edges = (0..4)
        .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: 10 + i as u64 })
        .collect();
    Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] }
}

#[test]
fn operations_match_the_kernel_they_wrap() {
    let (k, servers, _) = remote(u32::MAX);
    k.ping().unwrap();
    let local = MockKernel;
    let spec = ExtrudeSpec { distance: 5.0, symmetric: false };
    let a = k.extrude(&square(), &spec).unwrap();
    let b = local.extrude(&square(), &spec).unwrap();
    assert_eq!(a.face_origins, b.face_origins);
    let (ta, tb) = (k.topology(&a.shape).unwrap(), local.topology(&b.shape).unwrap());
    assert_eq!(ta.faces.len(), tb.faces.len());
    assert_eq!(k.mass_properties(&a.shape).unwrap().volume, local.mass_properties(&b.shape).unwrap().volume);
    assert_eq!(k.bbox(&a.shape).unwrap().max, local.bbox(&b.shape).unwrap().max);
    let params = TessellationParams::default();
    assert_eq!(
        k.tessellate(&a.shape, &params).unwrap().positions,
        local.tessellate(&b.shape, &params).unwrap().positions
    );
    // Hidden lines cross the wire unchanged (ADR 0032).
    let view = gusset_kernel::HlrView { toward: [0.0, -1.0, 0.0], x: [1.0, 0.0, 0.0], exact: true, deflection: 0.1 };
    let hl = k.hidden_lines(&a.shape, &view).unwrap();
    assert!(!hl.is_empty());
    assert_eq!(hl, local.hidden_lines(&b.shape, &view).unwrap());
    // Shapes made remotely go back as inputs.
    let moved = k.transform(&a.shape, &gusset_kernel::Transform::Translate([20.0, 0.0, 0.0])).unwrap();
    let both = k.boolean(BoolOp::Union, &a.shape, &moved.shape).unwrap();
    assert!(k.mass_properties(&both.shape).unwrap().volume > 0.0);
    // Errors keep their kind and text.
    let e = k.extrude(&square(), &ExtrudeSpec { distance: 0.0, symmetric: false }).unwrap_err();
    let le = local.extrude(&square(), &ExtrudeSpec { distance: 0.0, symmetric: false }).unwrap_err();
    assert_eq!((e.kind, e.message), (le.kind, le.message));
    assert_eq!(servers.lock().unwrap().len(), 1);
}

#[test]
fn dropped_shapes_are_freed_with_the_next_call() {
    let (k, servers, _) = remote(u32::MAX);
    let spec = ExtrudeSpec { distance: 5.0, symmetric: false };
    let a = k.extrude(&square(), &spec).unwrap();
    let b = k.extrude(&square(), &spec).unwrap();
    let server = servers.lock().unwrap()[0].clone();
    assert_eq!(server.lock().unwrap().held(), 2);
    drop(a);
    k.ping().unwrap();
    assert_eq!(server.lock().unwrap().held(), 1);
    drop(b);
    k.ping().unwrap();
    assert_eq!(server.lock().unwrap().held(), 0);
}

#[test]
fn a_dead_worker_is_an_error_and_the_next_call_restarts_it() {
    // The worker dies on its third request.
    let (k, servers, lives) = remote(2);
    let spec = ExtrudeSpec { distance: 5.0, symmetric: false };
    let a = k.extrude(&square(), &spec).unwrap();
    k.topology(&a.shape).unwrap();
    let e = k.mass_properties(&a.shape).unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::Crashed, "{e:?}");
    assert!(e.message.contains("signal 11"), "{e}");
    assert_eq!(k.generation(), 2);
    // Shapes of the dead worker are refused with a clear error, not sent to the new one.
    lives.store(u32::MAX, Ordering::SeqCst);
    let e = k.topology(&a.shape).unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::Crashed);
    assert!(e.message.contains("restarted"), "{e}");
    // New work goes to a new worker.
    let b = k.extrude(&square(), &spec).unwrap();
    k.topology(&b.shape).unwrap();
    assert_eq!(servers.lock().unwrap().len(), 2);
    // Dropping a stale shape does not free anything in the new worker.
    drop(a);
    k.ping().unwrap();
    assert_eq!(servers.lock().unwrap()[1].lock().unwrap().held(), 1);
}

#[test]
fn a_worker_that_cannot_start_is_an_error() {
    let k =
        RemoteKernel::new("mock", Box::new(|| Err("gusset-worker not found".into())), Box::new(crate::UnvettedNatives));
    let e = k.ping().unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::Crashed);
    assert!(e.message.contains("could not start") && e.message.contains("not found"), "{e}");
}

#[test]
fn faults_are_refused_without_the_feature() {
    let (k, _, _) = remote(u32::MAX);
    let r = k.inject_fault(Fault::Abort);
    if cfg!(feature = "fault-injection") {
        // Not exercised in-process: it would abort the test binary.
        drop(r);
    } else {
        assert_eq!(r.unwrap_err().kind, KernelErrorKind::Unsupported);
    }
}

#[test]
fn stream_transport_round_trips_through_frames() {
    // The server end runs on a thread over two OS pipes' worth of in-memory buffers.
    use std::io::Cursor;
    let server = Server::new(MockKernel);
    let mut requests = Vec::new();
    let ping = proto::Request::Ping.encode(&gusset_kernel::wire::NoShapes).unwrap();
    frame::write_frame(&mut requests, frame::REQUEST_MAGIC, &ping).unwrap();
    let mut replies = Vec::new();
    server.serve(&mut Cursor::new(requests), &mut replies).unwrap();
    let mut t = StreamTransport::new(Vec::new(), Cursor::new(replies));
    let reply = t.call(&ping).unwrap();
    let r = proto::Reply::decode(&reply, &gusset_kernel::wire::NoShapes).unwrap().unwrap();
    assert!(matches!(r, proto::Reply::Version(VERSION)));
    // A closed stream is a dead worker.
    assert!(matches!(t.call(&ping), Err(TransportError::Died { .. })));
}

#[test]
fn a_pool_runs_on_the_slot_worker_and_moves_shapes_between_workers() {
    let (a, _, _) = remote(u32::MAX);
    let (b, _, counter_b) = remote(u32::MAX);
    let pool = PoolKernel::new("mock", vec![a, b]);
    let spec = ExtrudeSpec { distance: 5.0, symmetric: false };
    let s0 = with_slot(0, || pool.extrude(&square(), &spec)).unwrap();
    let s1 = with_slot(1, || pool.extrude(&square(), &spec)).unwrap();
    let s1 = pool.transform(&s1.shape, &gusset_kernel::Transform::Translate([20.0, 0.0, 0.0])).unwrap();
    // Built on different workers ...
    assert_eq!(pool.generations(), [1, 1]);
    // ... and still combined: the tool moves to the target's worker.
    let u = pool.boolean(BoolOp::Union, &s0.shape, &s1.shape).unwrap();
    let local = MockKernel;
    let l0 = local.extrude(&square(), &spec).unwrap();
    let l1 = local.transform(&l0.shape, &gusset_kernel::Transform::Translate([20.0, 0.0, 0.0])).unwrap();
    let lu = local.boolean(BoolOp::Union, &l0.shape, &l1.shape).unwrap();
    assert_eq!(pool.mass_properties(&u.shape).unwrap().volume, local.mass_properties(&lu.shape).unwrap().volume);
    // Work in parallel on both workers.
    let pool = Arc::new(pool);
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let p = pool.clone();
            std::thread::spawn(move || {
                with_slot(i, || {
                    (0..20).map(|_| p.extrude(&square(), &spec).map(|r| r.shape)).collect::<Result<Vec<_>, _>>()
                })
            })
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap().unwrap().len(), 20);
    }
    // A worker restart changes the pool's epoch.
    let before = pool.epoch();
    counter_b.store(0, Ordering::SeqCst);
    let _ = with_slot(1, || pool.extrude(&square(), &spec));
    assert!(pool.epoch() > before);
}

#[test]
fn the_worker_reports_its_status_and_refuses_raw_imports() {
    let k = RemoteKernel::new(
        "mock",
        Box::new(|| {
            let server = Arc::new(Mutex::new(Server::new(MockKernel).with_status("sandbox: applied seccomp".into())));
            Ok(Box::new(Local { server, lives: Arc::new(AtomicU32::new(u32::MAX)) }) as Box<dyn Transport>)
        }),
        Box::new(crate::UnvettedNatives),
    );
    assert_eq!(k.status().unwrap(), "sandbox: applied seccomp");
    assert_eq!(gusset_kernel::Kernel::diagnostics(&k).as_deref(), Some("sandbox: applied seccomp"));
    // Untrusted files never reach the worker that holds the session's shapes (review L4).
    assert_eq!(
        gusset_kernel::Kernel::import_step(&k, b"ISO-10303-21;").unwrap_err().kind,
        KernelErrorKind::Unsupported
    );
    assert_eq!(gusset_kernel::Kernel::import_iges(&k, b"S").unwrap_err().kind, KernelErrorKind::Unsupported);
}
