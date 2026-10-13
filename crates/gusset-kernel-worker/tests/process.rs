//! The kernel in a real worker process (ADR 0022). With the OCCT kernel only.
//! `cargo test -p gusset-kernel-worker --features fault-injection` adds the crash tests.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use gusset_kernel::{
    ExtrudeSpec, Frame, Kernel, KernelErrorKind, Loop2, Profile, ProfileEdge, Region2, Seg2, TessellationParams,
};
use gusset_kernel_remote::RemoteKernel;
use std::path::PathBuf;

fn worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gusset-kernel-worker"))
}

fn remote(memory_mb: u64) -> RemoteKernel {
    RemoteKernel::new("occt", gusset_kernel_worker::connector(worker(), memory_mb << 20))
}

fn square(size: f64) -> Profile {
    let pts = [[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]];
    let edges = (0..4)
        .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: 10 + i as u64 })
        .collect();
    Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] }
}

#[test]
fn the_worker_gives_the_same_results_as_the_kernel_in_process() {
    let k = remote(1024);
    k.ping().unwrap();
    let local = gusset_kernel_occt::OcctKernel::new().unwrap();
    let spec = ExtrudeSpec { distance: 10.0, symmetric: false };
    let (a, b) = (k.extrude(&square(10.0), &spec).unwrap(), local.extrude(&square(10.0), &spec).unwrap());
    assert_eq!(a.face_origins, b.face_origins);
    let (ma, mb) = (k.mass_properties(&a.shape).unwrap(), local.mass_properties(&b.shape).unwrap());
    assert_eq!(ma.volume.to_bits(), mb.volume.to_bits());
    let (ta, tb) = (k.topology(&a.shape).unwrap(), local.topology(&b.shape).unwrap());
    assert_eq!(ta.faces.len(), tb.faces.len());
    assert_eq!(ta.edges.len(), tb.edges.len());
    let p = TessellationParams::default();
    assert_eq!(k.tessellate(&a.shape, &p).unwrap().positions, local.tessellate(&b.shape, &p).unwrap().positions);
    // Fillet the remote box: shapes made in the worker go back to it as inputs.
    let edges: Vec<u32> = (0..4).collect();
    let f = k.fillet(&a.shape, &edges, 1.0).unwrap();
    assert!(k.mass_properties(&f.shape).unwrap().volume < ma.volume);
    // A STEP export of a remote shape is a real STEP file.
    let step = k.export_step(&[(&f.shape, "Box")]).unwrap();
    assert!(step.starts_with(b"ISO-10303-21"));
    assert_eq!(k.generation(), 1);
}

#[test]
fn a_missing_worker_binary_is_a_clear_error() {
    let k = RemoteKernel::new("occt", gusset_kernel_worker::connector(PathBuf::from("/nonexistent/worker"), 1 << 30));
    let e = k.ping().unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::Crashed);
    assert!(e.message.contains("could not start"), "{e}");
}

#[cfg(feature = "fault-injection")]
mod faults {
    use super::*;
    use gusset_kernel_remote::Fault;

    /// Each fault ends the worker; the call reports how, and the next call works on a new one.
    fn crash_and_recover(fault: Fault, memory_mb: u64, expect: &[&str]) {
        let k = remote(memory_mb);
        let spec = ExtrudeSpec { distance: 10.0, symmetric: false };
        let before = k.extrude(&square(10.0), &spec).unwrap();
        let e = k.inject_fault(fault).unwrap_err();
        assert_eq!(e.kind, KernelErrorKind::Crashed, "{fault:?}: {e:?}");
        assert!(expect.iter().any(|w| e.message.contains(w)), "{fault:?}: {e}");
        // The old shape died with its worker and is refused, not resent.
        let stale = k.topology(&before.shape).unwrap_err();
        assert!(stale.message.contains("restarted"), "{stale}");
        // New work runs on a new worker.
        let after = k.extrude(&square(10.0), &spec).unwrap();
        assert!((k.mass_properties(&after.shape).unwrap().volume - 1000.0).abs() < 1e-6);
        assert_eq!(k.generation(), 2);
    }

    #[test]
    fn abort_is_contained() {
        crash_and_recover(Fault::Abort, 1024, &["aborted", "status"]);
    }

    #[test]
    fn stack_overflow_is_contained() {
        crash_and_recover(Fault::StackOverflow, 1024, &["segmentation", "aborted", "stack overflow", "signal"]);
    }

    #[test]
    fn running_out_of_memory_is_contained() {
        crash_and_recover(Fault::ExhaustMemory, 256, &["aborted", "memory", "killed", "signal"]);
    }

    #[test]
    fn an_exit_is_contained() {
        crash_and_recover(Fault::Exit, 1024, &["status 70"]);
    }

    #[test]
    fn a_slow_call_still_answers() {
        let k = remote(1024);
        k.inject_fault(Fault::Sleep(50)).unwrap();
        assert_eq!(k.generation(), 1);
    }
}
