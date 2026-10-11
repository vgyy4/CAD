//! Worker process handling without OCCT: stand-in "workers" are shell scripts that
//! misbehave the way a compromised worker could (security review #6).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gusset_worker::{Format, ImportError, import_out_of_process};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn script(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gusset-worker-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("worker.sh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
}

fn run(p: &Path, timeout: Duration) -> (Result<gusset_worker::Imported, ImportError>, Duration) {
    let t = Instant::now();
    let r = import_out_of_process(p, Format::Step, b"x", &gusset_sandbox::Limits::default(), timeout);
    (r, t.elapsed())
}

#[test]
fn grandchild_holding_stdout_does_not_hang_the_import() {
    // The worker exits at once but leaves a grandchild that keeps stdout open.
    let p = script("exit", "/bin/sleep 30 &\nexit 0");
    let (r, took) = run(&p, Duration::from_secs(20));
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
    assert!(matches!(r, Err(ImportError::Protocol(_))), "{r:?}");
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[test]
fn timeout_kills_the_whole_process_group() {
    let p = script("hang", "/bin/sleep 30 &\n/bin/sleep 30");
    let (r, took) = run(&p, Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
    assert_eq!(r.unwrap_err(), ImportError::TimedOut);
    assert!(took < Duration::from_secs(10), "took {took:?}");
}
