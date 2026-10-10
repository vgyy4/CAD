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
