//! `durable_write`, `save_project` and the recovery file under real I/O and under
//! simulated power loss (`FaultFs`).

// Test-only crate: clippy.toml already allows these inside #[test] functions; the
// same allowance covers the helpers here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod common;

use std::io;
use std::path::Path;

use common::{TempDir, project_bytes};
use gusset_store::container;
use gusset_store::durable::{self, DurableError, Step, durable_write, save_project};
use gusset_store::fs::{FaultFs, Fs, FsFile, RealFs};
use gusset_store::recovery;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn success_leaves_exactly_one_file_and_no_temp() {
    let d = TempDir::new("durable-ok");
    let p = d.path().join("part.gus");
    for seq in 1..=3 {
        save_project(&RealFs::default(), &p, &RealFs::private(), None, &project_bytes(seq)).unwrap();
        assert_eq!(names(d.path()), vec!["part.gus"]);
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(container::open(&bytes).unwrap().header().save_seq, seq);
    }
}

#[test]
fn validator_failure_keeps_old_file_and_removes_temp() {
    let d = TempDir::new("durable-invalid");
    let p = d.path().join("settings.toml");
    std::fs::write(&p, b"old").unwrap();
    let r = durable_write(&RealFs::default(), &p, b"new", &|_| Err::<(), _>("nope"));
    assert!(matches!(r, Err(DurableError::Invalid("nope"))));
    assert_eq!(std::fs::read(&p).unwrap(), b"old");
    assert_eq!(names(d.path()), vec!["settings.toml"]);
    // Invalid project bytes are refused before anything is written.
    let g = d.path().join("p.gus");
    let r = save_project(&RealFs::default(), &g, &RealFs::private(), None, b"not a container");
    assert!(matches!(r, Err(DurableError::Invalid(_))));
    assert_eq!(names(d.path()), vec!["settings.toml"]);
}

#[test]
fn missing_directory_is_a_create_temp_error() {
    let d = TempDir::new("durable-nodir");
    let p = d.path().join("no/such/dir/x.gus");
    let r = durable::durable_write_unchecked(&RealFs::default(), &p, b"x");
    assert!(matches!(r, Err(DurableError::Io { step: Step::CreateTemp, .. })));
}

#[test]
fn lkg_holds_the_previous_verified_version() {
    let d = TempDir::new("durable-lkg");
    let p = d.path().join("part.gus");
    let lkg = d.path().join("lkg.gus");
    save_project(&RealFs::default(), &p, &RealFs::private(), Some(&lkg), &project_bytes(1)).unwrap();
    assert!(!lkg.exists());
    save_project(&RealFs::default(), &p, &RealFs::private(), Some(&lkg), &project_bytes(2)).unwrap();
    assert_eq!(container::open(&std::fs::read(&lkg).unwrap()).unwrap().header().save_seq, 1);
    // A damaged project file is never copied over a good LKG.
    std::fs::write(&p, b"garbage").unwrap();
    save_project(&RealFs::default(), &p, &RealFs::private(), Some(&lkg), &project_bytes(3)).unwrap();
    assert_eq!(container::open(&std::fs::read(&lkg).unwrap()).unwrap().header().save_seq, 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&lkg).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[cfg(unix)]
#[test]
fn planted_symlink_at_temp_name_is_not_followed() {
    // create_new semantics: an existing name (even a symlink) makes creation fail, so
    // durable_write retries with a new random name instead of writing through it.
    let d = TempDir::new("durable-symlink");
    let victim = d.path().join("victim");
    std::fs::write(&victim, b"precious").unwrap();
    let link = d.path().join("~$x.0000000000000000.gtmp");
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    assert_eq!(RealFs::default().create_new(&link).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    durable::durable_write_unchecked(&RealFs::default(), &d.path().join("x.gus"), b"data").unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
}

// ---- power-loss simulation ---------------------------------------------------------

const SAVES: u64 = 4;
const PROJECT: &str = "/proj/part.gus";
const LKG: &str = "/data/lkg/0123.gus";

fn setup(seed: u64) -> FaultFs {
    let fs = FaultFs::new(seed);
    fs.create_dir_all(Path::new("/proj")).unwrap();
    fs.create_dir_all(Path::new("/data/lkg")).unwrap();
    fs.create_dir_all(Path::new("/data/recovery")).unwrap();
    fs
}

/// Runs up to `SAVES` saves; returns the last acknowledged save sequence.
fn save_loop(fs: &FaultFs, images: &[Vec<u8>]) -> u64 {
    let mut acked = 0;
    for (i, img) in images.iter().enumerate() {
        match save_project(fs, Path::new(PROJECT), fs, Some(Path::new(LKG)), img) {
            Ok(()) => acked = i as u64 + 1,
            Err(_) => break,
        }
    }
    acked
}

fn seq_of(fs: &FaultFs, p: &str) -> Option<Result<u64, container::OpenError>> {
    match fs.read(Path::new(p)) {
        Ok(b) => Some(container::open(&b).map(|c| c.header().save_seq)),
        Err(e) => {
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
            None
        }
    }
}

#[test]
fn project_saves_survive_power_loss_at_every_point() {
    let images: Vec<Vec<u8>> = (1..=SAVES).map(project_bytes).collect();
    // Count the operations of a full run to spread crash points over all of them.
    let dry = setup(0);
    let base = dry.op_count();
    assert_eq!(save_loop(&dry, &images), SAVES);
    let total = dry.op_count() - base;

    let iterations = 4000;
    let mut outcomes = std::collections::BTreeMap::new();
    for iter in 0..iterations {
        let fs = setup(iter);
        fs.set_crash_after(Some(base + iter % (total + 1)));
        let acked = save_loop(&fs, &images);
        let after = fs.crash();

        // The project file is the newest acknowledged save or the one in flight, and
        // always opens cleanly.
        match seq_of(&after, PROJECT) {
            None => assert_eq!(acked, 0, "iter {iter}: acknowledged save lost"),
            Some(Ok(seq)) => {
                assert!(seq == acked || seq == acked + 1, "iter {iter}: seq {seq}, acked {acked}");
                *outcomes.entry(seq == acked).or_insert(0) += 1;
            }
            Some(Err(e)) => unreachable!("iter {iter}: project damaged after power loss: {e}"),
        }
        // The LKG is never torn and is at most one generation behind.
        if let Some(r) = seq_of(&after, LKG) {
            let seq = r.unwrap_or_else(|e| unreachable!("iter {iter}: LKG damaged: {e}"));
            assert!(seq + 1 >= acked && seq <= acked, "iter {iter}: LKG seq {seq}, acked {acked}");
        } else {
            assert!(acked <= 1, "iter {iter}: LKG missing after {acked} saves");
        }
        // At most one orphaned temp per directory, and only ones housekeeping recognises.
        for f in after.files() {
            let name = f.file_name().unwrap();
            if f != Path::new(PROJECT) && f != Path::new(LKG) {
                assert!(durable::is_temp_name(name), "iter {iter}: unexpected file {}", f.display());
            }
        }
        assert!(after.files().len() <= 4);
    }
    // Both outcomes (in-flight save lost / landed) actually happened.
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
}

/// A write path that skips the file fsync must be caught by the simulator.
#[test]
fn simulator_catches_missing_file_fsync() {
    let mut broken = 0;
    for seed in 0..300 {
        let fs = setup(seed);
        let tmp = Path::new("/proj/~$part.0000000000000001.gtmp");
        let mut f = fs.create_new(tmp).unwrap();
        f.write_all(&project_bytes(1)).unwrap();
        drop(f); // no sync_all
        fs.replace(tmp, Path::new(PROJECT)).unwrap();
        fs.sync_dir(Path::new("/proj")).unwrap(); // "acknowledged"
        if !matches!(seq_of(&fs.crash(), PROJECT), Some(Ok(1))) {
            broken += 1;
        }
    }
    assert!(broken > 0, "FaultFs failed to expose a missing fsync");
}

/// A write path that skips the directory fsync must be caught by the simulator.
#[test]
fn simulator_catches_missing_dir_fsync() {
    let mut lost = 0;
    for seed in 0..300 {
        let fs = setup(seed);
        durable::durable_write_unchecked(&fs, Path::new(PROJECT), &project_bytes(1)).unwrap();
        let tmp = Path::new("/proj/~$part.0000000000000002.gtmp");
        let mut f = fs.create_new(tmp).unwrap();
        f.write_all(&project_bytes(2)).unwrap();
        f.sync_all().unwrap();
        drop(f);
        fs.replace(tmp, Path::new(PROJECT)).unwrap(); // no sync_dir, "acknowledged"
        if matches!(seq_of(&fs.crash(), PROJECT), Some(Ok(1))) {
            lost += 1;
        }
    }
    assert!(lost > 0, "FaultFs failed to expose a missing directory fsync");
}

#[test]
fn recovery_file_two_slot_guarantee_under_power_loss() {
    const SNAPS: u64 = 5;
    let path = Path::new("/data/recovery/0123.grec");
    let images: Vec<Vec<u8>> = (1..=SNAPS).map(project_bytes).collect();
    let run = |fs: &FaultFs| {
        let mut acked = 0;
        for img in &images {
            match recovery::write_snapshot(fs, path, img, &container::verify) {
                Ok(seq) => acked = seq,
                Err(_) => break,
            }
        }
        acked
    };
    let dry = setup(0);
    let base = dry.op_count();
    assert_eq!(run(&dry), SNAPS);
    let total = dry.op_count() - base;

    for iter in 0..3000 {
        let fs = setup(iter);
        fs.set_crash_after(Some(base + iter % (total + 1)));
        let acked = run(&fs);
        let after = fs.crash();
        match recovery::read_newest(&after, path, &container::verify) {
            Ok(None) => assert_eq!(acked, 0, "iter {iter}: acknowledged snapshot lost"),
            Ok(Some(snap)) => {
                assert!(snap.seq == acked || snap.seq == acked + 1, "iter {iter}: seq {} acked {acked}", snap.seq);
                // The payload is the project image written with that sequence number.
                assert_eq!(container::open(&snap.payload).unwrap().header().save_seq, snap.seq);
                // The other slot is the previous snapshot.
                let slots = recovery::decode_slots(&after.read(path).unwrap());
                if snap.seq > 1 {
                    assert_eq!(slots.len(), 2, "iter {iter}");
                    assert_eq!(slots[0].seq + 1, slots[1].seq);
                }
            }
            Err(e) => unreachable!("iter {iter}: recovery file damaged: {e}"),
        }
        // Exactly one recovery file per project (plus at most one orphaned temp).
        let in_dir: Vec<_> = after.files().into_iter().filter(|f| f.starts_with("/data/recovery")).collect();
        let real: Vec<_> = in_dir.iter().filter(|f| !durable::is_temp_name(f.file_name().unwrap())).collect();
        assert!(real.len() <= 1 && in_dir.len() <= 2, "iter {iter}: {in_dir:?}");
    }
}
