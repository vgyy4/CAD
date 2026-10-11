//! Process-kill crash tests (ADR 0007 "Power-loss testing", review M14).
//!
//! The test re-runs its own binary as a helper that edits, snapshots and saves a project
//! in a loop and prints a line after every *acknowledged* write. The parent kills it at
//! random points (mid-save, mid-snapshot, between writes) and then checks that:
//! - the project file always opens cleanly (never damaged), at the newest acknowledged
//!   save or a newer one;
//! - a recovery file, if present, always yields a valid snapshot at least as new as the
//!   newest acknowledged snapshot written after the last save, and is offered on open
//!   when it is newer than the file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use gusset_doc::Document;
use gusset_history::{Ctx, History, NodeState};
use gusset_project::{OpenReport, Project};
use gusset_store::paths::DataDirs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const HELPER_ENV: &str = "GUSSET_KILL_HELPER_DIR";

fn dirs(root: &Path) -> DataDirs {
    DataDirs::at(root.join("data"), root.join("cache"))
}

fn doc(i: u64) -> Document {
    Document::from_text(&format!("gusset 1\nunits mm\n\nparam a = {i} mm\n")).unwrap()
}

/// The iteration number stored in the main branch head's label ("edit <i>").
fn edit_number(h: &History) -> u64 {
    let head = h.head(h.main()).unwrap();
    h.node(head).unwrap().label.strip_prefix("edit ").and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// The helper process: runs only when the parent sets `HELPER_ENV`.
#[test]
fn kill_helper() {
    let Ok(root) = std::env::var(HELPER_ENV) else { return };
    let root = PathBuf::from(root);
    let file = root.join("p.gus");
    let (mut p, mut h) = if file.exists() {
        let (p, h, _) = Project::open(&file, dirs(&root)).unwrap();
        (p, h)
    } else {
        let mut h = History::new(&doc(0), &Ctx::user(1));
        // A blob makes each write long enough to be interrupted in the middle.
        let blob: Vec<u8> = (0..(3u32 << 20)).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        h.put_blob(&blob);
        let mut p = Project::create(dirs(&root)).unwrap();
        p.save_as(&file, &h).unwrap();
        (p, h)
    };
    let mut i = edit_number(&h);
    loop {
        i += 1;
        let main = h.main();
        h.commit(&Ctx::user(i), main, &doc(i), &format!("edit {i}"), NodeState::Ok).unwrap();
        if i % 3 == 0 {
            p.save(&h).unwrap();
            println!("saved {i}");
        } else {
            p.snapshot(&h).unwrap();
            println!("snap {i}");
        }
    }
}

struct Xorshift(u64);

impl Xorshift {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[test]
fn killed_mid_write_always_leaves_an_openable_project() {
    if std::env::var(HELPER_ENV).is_ok() {
        return;
    }
    let root = std::env::temp_dir().join(format!("gusset-kill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("p.gus");
    let mut rng = Xorshift(0x5eed_1234_abcd_0001);
    let (mut last_saved, mut last_snap_after_save) = (0u64, 0u64);
    let iterations = 24;
    for round in 0..iterations {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "kill_helper", "--nocapture", "--test-threads", "1"])
            .env(HELPER_ENV, &root)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        // Let it get going (at least one acknowledged write), then kill at a random point.
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut acks = Vec::new();
        while acks.is_empty() && Instant::now() < deadline {
            // Only acknowledgement lines count (libtest prints its own lines too).
            if let Ok(l) = rx.recv_timeout(Duration::from_millis(50))
                && (l.starts_with("saved ") || l.starts_with("snap "))
            {
                acks.push(l);
            }
        }
        assert!(!acks.is_empty(), "round {round}: the helper never acknowledged a write");
        std::thread::sleep(Duration::from_millis(rng.below(400)));
        child.kill().unwrap();
        child.wait().unwrap();
        reader.join().unwrap();
        acks.extend(rx.try_iter());
        for a in &acks {
            if let Some(n) = a.strip_prefix("saved ").and_then(|s| s.parse::<u64>().ok()) {
                last_saved = n;
                last_snap_after_save = 0;
            } else if let Some(n) = a.strip_prefix("snap ").and_then(|s| s.parse::<u64>().ok()) {
                last_snap_after_save = n;
            }
        }

        let (p, h, report) =
            Project::open(&file, dirs(&root)).unwrap_or_else(|e| panic!("round {round}: project does not open: {e}"));
        let at = edit_number(&h);
        assert!(at >= last_saved, "round {round}: file at edit {at}, but edit {last_saved} was acknowledged");
        match report {
            OpenReport::Clean => {
                assert!(
                    last_snap_after_save == 0 || at >= last_snap_after_save,
                    "round {round}: acknowledged snapshot {last_snap_after_save} was not offered (file at {at})"
                );
            }
            OpenReport::RecoveryAvailable { recovered, .. } => {
                let r = edit_number(&recovered);
                assert!(
                    r >= last_snap_after_save,
                    "round {round}: recovery at {r} < acknowledged {last_snap_after_save}"
                );
            }
            OpenReport::Damaged { reason, source } => {
                panic!("round {round}: damaged after a kill ({reason}); fell back to {source:?}")
            }
        }
        // Release without resolving: the next helper continues from the saved file and
        // its snapshots replace the recovery copy (still one file).
        p.release();
    }
    // No temp files survive outside the transient one per interrupted write, and
    // housekeeping removes them.
    let opts = gusset_store::housekeeping::Options { temp_min_age: Duration::ZERO, ..Default::default() };
    gusset_store::housekeeping::run(&dirs(&root), &opts).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| gusset_store::durable::is_temp_name(&e.file_name()))
        .map(|e| e.file_name())
        .collect();
    assert!(leftovers.is_empty(), "orphan temp files: {leftovers:?}");
    let recovery_files = std::fs::read_dir(dirs(&root).recovery_dir()).unwrap().count();
    assert!(recovery_files <= 1, "more than one recovery file per project");
    let _ = std::fs::remove_dir_all(&root);
}
