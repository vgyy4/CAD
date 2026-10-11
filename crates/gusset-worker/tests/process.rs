//! Worker process handling without OCCT: stand-in "workers" are shell scripts that
//! misbehave the way a compromised worker could (security review #6).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
    // A test running in parallel may fork while this script's write fd is still open in
    // this process; until that child execs, exec of the script fails with ETXTBSY.
    for _ in 0..50 {
        let t = Instant::now();
        let r = import_out_of_process(p, Format::Step, b"x", &gusset_sandbox::Limits::default(), timeout);
        match r {
            Err(ImportError::Spawn(ref e)) if e.contains("busy") => std::thread::sleep(Duration::from_millis(20)),
            r => return (r, t.elapsed()),
        }
    }
    panic!("the test script stayed busy");
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

#[test]
fn oversized_response_is_refused_without_buffering_it() {
    // 64 MiB of output for a 1-byte input: far over `output_cap(1)`.
    let p = script("flood", "/bin/dd if=/dev/zero bs=1048576 count=64 2>/dev/null");
    let (r, took) = run(&p, Duration::from_secs(20));
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
    assert_eq!(r.unwrap_err(), ImportError::OutputTooLarge);
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

/// Runs an IGES import against a stand-in worker; returns the result and what the script
/// recorded in `rec` (argv and the state of the private input file).
fn run_iges(
    tag: &str,
    body: &str,
    timeout: Duration,
    cancel: &std::sync::atomic::AtomicBool,
) -> (Result<gusset_worker::Imported, ImportError>, String) {
    let p = script(tag, body);
    let rec = p.parent().unwrap().join("rec");
    for _ in 0..50 {
        let r = gusset_worker::import_out_of_process_cancellable(
            &p,
            Format::Iges,
            b"IGES-BYTES",
            &gusset_sandbox::Limits::default(),
            timeout,
            cancel,
        );
        match r {
            Err(ImportError::Spawn(ref e)) if e.contains("busy") => std::thread::sleep(Duration::from_millis(20)),
            r => {
                let text = std::fs::read_to_string(&rec).unwrap_or_default();
                let _ = std::fs::remove_dir_all(p.parent().unwrap());
                return (r, text);
            }
        }
    }
    panic!("the test script stayed busy");
}

/// Script prologue: $1 is the flag, $2 the file. Records flag, file, modes and contents.
fn recording_body(tail: &str) -> String {
    format!(
        "rec=\"$(dirname \"$0\")/rec\"\n\
         echo \"$1\" > \"$rec\"\n\
         echo \"$2\" >> \"$rec\"\n\
         stat -c '%a' \"$(dirname \"$2\")\" >> \"$rec\"\n\
         stat -c '%a' \"$2\" >> \"$rec\"\n\
         cat \"$2\" >> \"$rec\"\n\
         {tail}"
    )
}

#[test]
fn iges_input_goes_to_a_private_file_that_is_deleted_afterwards() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let (r, rec) = run_iges("iges-ok", &recording_body("exit 0"), Duration::from_secs(20), &cancel);
    // The stand-in sends no response frame; what matters is what it saw.
    assert!(matches!(r, Err(ImportError::Protocol(_))), "{r:?}");
    let lines: Vec<&str> = rec.lines().collect();
    assert_eq!(lines.first(), Some(&"--input-file"), "{rec}");
    let file = Path::new(lines.get(1).unwrap());
    assert!(file.is_absolute());
    assert_eq!(lines.get(2), Some(&"700"), "directory mode: {rec}");
    assert_eq!(lines.get(3), Some(&"600"), "file mode: {rec}");
    assert_eq!(lines.get(4), Some(&"IGES-BYTES"), "contents: {rec}");
    assert!(!file.exists() && !file.parent().unwrap().exists(), "temp dir left behind: {}", file.display());
}

#[test]
fn iges_temp_dir_is_deleted_after_a_crash_and_a_timeout() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let (r, rec) = run_iges("iges-crash", &recording_body("kill -9 $$"), Duration::from_secs(20), &cancel);
    assert!(matches!(r, Err(ImportError::Crashed(_))), "{r:?}");
    let file = rec.lines().nth(1).map(PathBuf::from).unwrap();
    assert!(!file.parent().unwrap().exists(), "left behind after crash");

    let body = recording_body("exec /bin/sleep 30");
    let (r, rec) = run_iges("iges-timeout", &body, Duration::from_millis(500), &cancel);
    assert_eq!(r.unwrap_err(), ImportError::TimedOut);
    let file = rec.lines().nth(1).map(PathBuf::from).unwrap();
    assert!(!file.parent().unwrap().exists(), "left behind after timeout");
}

#[test]
fn iges_temp_dir_is_deleted_after_a_cancel() {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let c2 = cancel.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        c2.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let body = recording_body("exec /bin/sleep 30");
    let (r, rec) = run_iges("iges-cancel", &body, Duration::from_secs(60), &cancel);
    t.join().unwrap();
    assert_eq!(r.unwrap_err(), ImportError::Cancelled);
    let file = rec.lines().nth(1).map(PathBuf::from).unwrap();
    assert!(!file.parent().unwrap().exists(), "left behind after cancel");
}

#[test]
fn step_stays_on_stdin_without_a_file_argument() {
    let p = script("step-args", "rec=\"$(dirname \"$0\")/rec\"\necho \"argc=$#\" > \"$rec\"\nexit 0");
    let (r, _) = run(&p, Duration::from_secs(20));
    let rec = std::fs::read_to_string(p.parent().unwrap().join("rec")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
    assert!(matches!(r, Err(ImportError::Protocol(_))), "{r:?}");
    assert_eq!(rec.trim(), "argc=0");
}
