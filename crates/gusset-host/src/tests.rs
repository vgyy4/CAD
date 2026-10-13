use super::*;
use gusset_commands::{CmdCtx, Session};
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::json;
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0))
}

#[test]
fn runs_commands_serially_and_broadcasts_changes() {
    let host = Host::spawn(session(), None);
    let mut rx = host.subscribe();
    let r = host.call(CmdCtx::user(1), "param.set", json!({"name": "w", "expr": "10 mm"})).unwrap();
    assert_eq!(r["value"], "10 mm");
    match rx.try_recv().unwrap() {
        HostEvent::Changed { actor, label, .. } => {
            assert_eq!(actor, Actor::User);
            assert_eq!(label, "Set w");
        }
        other => panic!("{other:?}"),
    }
    assert!(host.is_quiescent());
    // Read-only snapshot from another thread.
    let h2 = host.clone();
    let n = std::thread::spawn(move || h2.with_session(|s| s.document(&Actor::User).unwrap().params.len()).unwrap())
        .join()
        .unwrap();
    assert_eq!(n, 1);
    // Errors come back structured.
    let e = host.call(CmdCtx::user(2), "nope.nope", json!({})).unwrap_err();
    assert_eq!(e.code, "not_found");
    let (s, p) = host.shutdown().unwrap();
    assert!(p.is_none());
    assert!(s.dirty);
    assert_eq!(host.call(CmdCtx::user(3), "query.model", json!({})).unwrap_err().code, "shutdown");
    assert!(host.shutdown().is_none());
}

#[test]
fn many_threads_get_a_total_order() {
    let host = Host::spawn(session(), None);
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let h = host.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    h.call(CmdCtx::user(1), "param.set", json!({"name": format!("p{i}_{j}"), "expr": "1 mm"})).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let n = host.with_session(|s| s.document(&Actor::User).unwrap().params.len()).unwrap();
    assert_eq!(n, 80);
}

#[test]
fn reentrancy_is_an_error_not_a_deadlock() {
    let host = Host::spawn(session(), None);
    let h2 = host.clone();
    let inner = host.with_session(move |_| h2.call(CmdCtx::user(1), "query.model", json!({}))).unwrap();
    assert_eq!(inner.unwrap_err().code, "reentrant");
    let h3 = host.clone();
    let nested = host.with_session(move |_| h3.with_session(|_| ())).unwrap();
    assert_eq!(nested.unwrap_err().code, "reentrant");
    let h4 = host.clone();
    let from_job = host.exec(move |_, _| h4.call(CmdCtx::user(1), "query.model", json!({}))).unwrap();
    assert_eq!(from_job.unwrap_err().code, "reentrant");
}

#[test]
fn a_panicking_job_does_not_kill_the_host() {
    let host = Host::spawn(session(), None);
    #[allow(clippy::panic)]
    let r: Result<(), _> = host.exec(|_, _| panic!("boom"));
    assert_eq!(r.unwrap_err().code, "internal");
    assert!(host.call(CmdCtx::user(1), "query.model", json!({})).is_ok());
    assert!(host.is_quiescent());
}

#[tokio::test(flavor = "current_thread")]
async fn async_calls_do_not_block_the_runtime() {
    let host = Host::spawn(session(), None);
    let a = CmdCtx::agent("bot-1", 5);
    host.call_async(a.clone(), "param.set", json!({"name": "d", "expr": "2 mm"})).await.unwrap();
    let list = host.call_async(a, "branch.list", json!({})).await.unwrap();
    let names: Vec<String> = list["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name_user_text"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"agent/bot-1".to_string()), "{names:?}");
}

#[test]
fn save_and_snapshot_with_a_project() {
    let base = std::env::temp_dir().join(format!("gusset-host-test-{}-{}", std::process::id(), line!()));
    let _ = std::fs::remove_dir_all(&base);
    let dirs = gusset_store::paths::DataDirs::at(base.join("data"), base.join("cache"));
    let project = gusset_project::Project::create(dirs).unwrap();
    let host = Host::spawn(session(), Some(project));
    // No file yet: save is refused clearly.
    assert_eq!(host.save().unwrap_err().code, "not_allowed");
    // Nothing dirty: no snapshot.
    assert_eq!(host.snapshot().unwrap(), None);
    host.call(CmdCtx::user(1), "param.set", json!({"name": "w", "expr": "1 mm"})).unwrap();
    assert!(host.snapshot().unwrap().is_some());
    let path = base.join("p.gus");
    host.exec(move |s, p| p.unwrap().save_as(&path, s.history())).unwrap().unwrap();
    let mut rx = host.subscribe();
    host.save().unwrap();
    assert_eq!(rx.try_recv().unwrap(), HostEvent::Saved);
    assert!(!host.with_session(|s| s.dirty).unwrap());
    let (_, p) = host.shutdown().unwrap();
    p.unwrap().close();
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn detached_jobs_run_in_order_and_always_report_back() {
    let host = Host::spawn(session(), None);
    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..5 {
        let tx = tx.clone();
        host.exec_detached(
            move |s, _| {
                gusset_commands::execute(
                    s,
                    &CmdCtx::user(1),
                    "param.set",
                    json!({"name": format!("p{i}"), "expr": "1 mm"}),
                )
                .is_ok()
            },
            move |r| {
                let _ = tx.send((i, r));
            },
        )
        .unwrap();
    }
    let got: Vec<_> = (0..5).map(|_| rx.recv().unwrap()).collect();
    assert_eq!(got, (0..5).map(|i| (i, Ok(true))).collect::<Vec<_>>());
    // A panicking detached job reports `internal` instead of going silent.
    let tx2 = tx.clone();
    #[allow(clippy::panic)]
    host.exec_detached(
        |_, _| -> bool { panic!("boom") },
        move |r| {
            let _ = tx2.send((99, r));
        },
    )
    .unwrap();
    let (i, r) = rx.recv().unwrap();
    assert_eq!(i, 99);
    assert_eq!(r.unwrap_err().code, "internal");
    assert!(host.is_quiescent());
    // After shutdown the callback still runs, with the error.
    let _ = host.shutdown();
    let tx3 = tx.clone();
    let e = host
        .exec_detached(
            |_, _| true,
            move |r| {
                let _ = tx3.send((7, r));
            },
        )
        .unwrap_err();
    assert_eq!(e.code, "shutdown");
    assert_eq!(rx.recv().unwrap().1.unwrap_err().code, "shutdown");
}

#[test]
fn try_with_session_never_waits_for_a_running_job() {
    let host = Host::spawn(session(), None);
    assert_eq!(host.try_with_session(|s| s.dirty).unwrap(), Some(false));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    host.exec_detached(
        move |_, _| {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        },
        |_| {},
    )
    .unwrap();
    started_rx.recv().unwrap();
    assert_eq!(host.try_with_session(|_| 1).unwrap(), None, "busy: must not block");
    release_tx.send(()).unwrap();
    host.exec(|_, _| ()).unwrap();
    assert_eq!(host.try_with_session(|_| 2).unwrap(), Some(2));
    let _ = host.shutdown();
    assert_eq!(host.try_with_session(|_| 3).unwrap_err().code, "shutdown");
}

/// Code review M6: while an import parses, the model thread keeps serving commands, and
/// the import can be cancelled.
#[test]
fn import_parses_off_the_model_thread_and_cancels() {
    use gusset_commands::import::CancelFlag;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    struct Slow(Arc<AtomicBool>);
    impl gusset_commands::session::Importer for Slow {
        fn import(&self, _: &str, _: &[u8], cancel: &AtomicBool) -> Result<Vec<(String, Vec<u8>)>, String> {
            self.0.store(true, Ordering::SeqCst);
            while !cancel.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err("stopped".into())
        }
    }
    let dir = std::env::temp_dir().join(format!("gusset-host-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("part.step"), b"ISO-10303-21;").unwrap();
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(gusset_kernel::mock::MockKernel), &CmdCtx::user(0));
    let started = Arc::new(AtomicBool::new(false));
    s.importer = Some(Arc::new(Slow(started.clone())));
    s.project_dir = Some(dir.clone());
    let h = Host::spawn(s, None);
    let cancel = CancelFlag::default();
    let (tx, rx) = mpsc::channel();
    h.import_detached(CmdCtx::user(1), serde_json::json!({"path": "part.step"}), cancel.clone(), move |r| {
        let _ = tx.send(r);
    })
    .unwrap();
    while !started.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // The model thread is free while the import runs.
    h.call(CmdCtx::user(2), "param.set", serde_json::json!({"name": "w", "expr": "1 mm"})).unwrap();
    cancel.store(true, Ordering::SeqCst);
    let e = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap().unwrap_err();
    assert_eq!(e.code, "cancelled");
    let _ = h.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Security review V1: a running rebuild can be stopped, explicitly or by a newer edit, and
/// the cancel never leaks into the next job.
#[test]
fn rebuilds_are_cancelled_by_request_and_by_newer_edits() {
    let text = "gusset 1\nunits mm\n\nsketch S1 (plane: XY, next_id: 9) {\n  p1 = point(0, 0)\n  \
                p2 = point(10, 0)\n  p3 = point(10, 10)\n  p4 = point(0, 10)\n  l5 = line(p1, p2)\n  \
                l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\n\
                extrude E1 (profile: S1, distance: 10 mm, op: new)\n";
    let doc = Document::from_text(text).unwrap();
    let host = Host::spawn(session(), None);
    // A job that waits for the test, then evaluates.
    let run_blocked = |doc: Document| {
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        host.exec_detached(
            move |s, _| {
                let _ = go_rx.recv();
                s.evaluate_doc(&doc, None)
            },
            move |r| {
                let _ = done_tx.send(r.unwrap());
            },
        )
        .unwrap();
        while host.is_quiescent() {
            std::thread::yield_now();
        }
        (go_tx, done_rx)
    };

    let (go, done) = run_blocked(doc.clone());
    host.cancel_rebuild();
    go.send(()).unwrap();
    let ev = done.recv().unwrap();
    assert!(ev.cancelled);
    assert!(ev.bodies.is_empty());

    // A read-only command does not supersede; an edit does.
    let (go, done) = run_blocked(doc.clone());
    host.supersede_rebuild("document.get_text");
    go.send(()).unwrap();
    let ev = done.recv().unwrap();
    assert!(!ev.cancelled && ev.errors().is_empty(), "{:?}", ev.errors());
    // (A document not evaluated yet: cached features cost nothing and are not cancelled.)
    let other = Document::from_text(&text.replace("10 mm, op", "12 mm, op")).unwrap();
    let (go, done) = run_blocked(other);
    host.supersede_rebuild("param.set");
    go.send(()).unwrap();
    assert!(done.recv().unwrap().cancelled);

    // The next job starts with the flag cleared, and an idle host ignores a cancel.
    host.cancel_rebuild();
    let ev = host.exec(move |s, _| s.evaluate_doc(&doc, None)).unwrap();
    assert!(!ev.cancelled && ev.bodies.len() == 1);
}
