//! Locks, cache, housekeeping, project deletion and the multi-session hygiene test.

// Test-only crate: clippy.toml already allows these inside #[test] functions; the
// same allowance covers the helpers here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod common;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use common::{Rng, TempDir, all_files, project_bytes};
use gusset_store::Hash;
use gusset_store::cache::{Cache, Scope};
use gusset_store::container;
use gusset_store::durable::{self, save_project};
use gusset_store::fs::RealFs;
use gusset_store::housekeeping::{self, Options, Registry};
use gusset_store::lock::{Acquired, LockError, ProjectLock};
use gusset_store::paths::{DataDirs, ProjectId};
use gusset_store::recovery;

fn dirs(t: &TempDir) -> DataDirs {
    let d = DataDirs::at(t.path().join("data"), t.path().join("cache"));
    d.ensure().unwrap();
    d
}

fn now_opts() -> Options {
    Options { temp_min_age: Duration::ZERO, extra_dirs: Vec::new() }
}

fn set_mtime(p: &Path, secs: u64) {
    File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

fn plant_temp(dir: &Path, stem: &str, rng: &mut Rng) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(format!("~${stem}.{:016x}.gtmp", rng.next()));
    std::fs::write(&p, b"partial").unwrap();
    p
}

// ---- locks ---------------------------------------------------------------------

#[test]
fn lock_takeover_after_crash() {
    let t = TempDir::new("lock");
    let d = dirs(&t);
    let id = ProjectId::new_random().unwrap();
    let (first, how) = ProjectLock::acquire(&d.lock_file(&id)).unwrap();
    assert_eq!(how, Acquired::Fresh);
    assert!(matches!(ProjectLock::acquire(&d.lock_file(&id)), Err(LockError::Held(_))));
    let info = first.info().clone();
    drop(first); // crash: OS lock released, file left behind
    let (second, how) = ProjectLock::acquire(&d.lock_file(&id)).unwrap();
    assert_eq!(how, Acquired::TookOverStale(Some(info)));
    second.release().unwrap();
    assert!(!d.lock_file(&id).exists());
}

// ---- cache ---------------------------------------------------------------------

#[test]
fn cache_evicts_least_recently_used() {
    let t = TempDir::new("cache-lru");
    let entry = 1000u64;
    let cache = Cache::open(t.path().join("c"), 4 * (entry + 44)).unwrap();
    let keys: Vec<Hash> = (0..5u8).map(|i| Hash::of(&[i])).collect();
    for (i, k) in keys.iter().take(4).enumerate() {
        cache.put(Scope::Shared, k, &vec![i as u8; entry as usize]).unwrap();
    }
    for (i, k) in keys.iter().take(4).enumerate() {
        set_mtime(&cache.dir().join(format!("shared/{}.bin", k.to_hex())), 1000 + i as u64);
    }
    // Touch the oldest; the second-oldest becomes the LRU victim.
    assert!(cache.get(Scope::Shared, &keys[0]).unwrap().is_some());
    cache.put(Scope::Shared, &keys[4], &[4u8; 1000]).unwrap();
    assert!(cache.usage().unwrap() <= cache.cap());
    assert!(cache.get(Scope::Shared, &keys[1]).unwrap().is_none());
    for k in [&keys[0], &keys[2], &keys[3], &keys[4]] {
        assert!(cache.get(Scope::Shared, k).unwrap().is_some());
    }
    // Oversized entries are not stored.
    cache.put(Scope::Shared, &Hash::of(b"big"), &vec![0; 10_000]).unwrap();
    assert!(cache.get(Scope::Shared, &Hash::of(b"big")).unwrap().is_none());
    assert!(all_files(cache.dir()).iter().all(|f| f.extension().is_some_and(|e| e == "bin")));
}

#[test]
fn cache_damaged_entry_is_a_miss_and_removed() {
    let t = TempDir::new("cache-damage");
    let cache = Cache::open(t.path().join("c"), 1 << 20).unwrap();
    let id = ProjectId([9; 16]);
    let k = Hash::of(b"thumb");
    cache.put(Scope::Project(id), &k, b"png bytes").unwrap();
    let p = cache.dir().join(format!("{id}/{}.bin", k.to_hex()));
    let mut b = std::fs::read(&p).unwrap();
    let n = b.len();
    b[n - 1] ^= 1;
    std::fs::write(&p, b).unwrap();
    assert_eq!(cache.get(Scope::Project(id), &k).unwrap(), None);
    assert!(!p.exists());
    cache.put(Scope::Project(id), &k, b"png bytes").unwrap();
    assert_eq!(cache.project_scopes().unwrap(), vec![id]);
    cache.remove_project(&id).unwrap();
    assert!(cache.project_scopes().unwrap().is_empty());
}

#[test]
fn cache_is_safe_with_concurrent_instances() {
    let t = TempDir::new("cache-conc");
    let dir = t.path().join("c");
    let keys: Vec<Hash> = (0..8u8).map(|i| Hash::of(&[i])).collect();
    let valid = |v: &[u8]| v.len() == 3000 && v.iter().all(|&b| b == v[0]) && v[0] < 4;
    std::thread::scope(|s| {
        for w in 0..4u8 {
            let dir = dir.clone();
            let keys = keys.clone();
            s.spawn(move || {
                // A small cap forces constant eviction races.
                let cache = Cache::open(&dir, 5 * 3044).unwrap();
                let mut rng = Rng(u64::from(w) + 1);
                for _ in 0..300 {
                    let k = &keys[rng.below(keys.len() as u64) as usize];
                    if rng.below(2) == 0 {
                        cache.put(Scope::Shared, k, &[w; 3000]).unwrap();
                    } else if let Some(v) = cache.get(Scope::Shared, k).unwrap() {
                        assert!(valid(&v), "corrupt entry read");
                    }
                }
            });
        }
    });
    let cache = Cache::open(&dir, 5 * 3044).unwrap();
    cache.evict().unwrap();
    assert!(cache.usage().unwrap() <= cache.cap());
    for f in all_files(&dir) {
        assert!(f.extension().is_some_and(|e| e == "bin"), "leftover {}", f.display());
    }
}

// ---- housekeeping ----------------------------------------------------------------

struct Project {
    id: ProjectId,
    path: PathBuf,
}

fn new_project(d: &DataDirs, dir: &Path, name: &str, seq: u64) -> Project {
    let id = ProjectId::new_random().unwrap();
    let path = dir.join(format!("{name}.gus"));
    save_project(&RealFs::default(), &path, &RealFs::private(), Some(&d.lkg_file(&id)), &project_bytes(seq)).unwrap();
    housekeeping::register(d, id, &path).unwrap();
    Project { id, path }
}

fn snapshot(d: &DataDirs, p: &Project, seq: u64) {
    recovery::write_snapshot(&RealFs::private(), &d.recovery_file(&p.id), &project_bytes(seq), &container::verify)
        .unwrap();
}

#[test]
fn housekeeping_removes_orphans_and_keeps_live_data() {
    let t = TempDir::new("hk");
    let d = dirs(&t);
    let projects = t.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut rng = Rng(5);
    let cache = Cache::open(d.cache_dir(), 1 << 20).unwrap();

    // Alive, crashed last session: stale lock, recovery file, temp files.
    let alive = new_project(&d, &projects, "alive", 1);
    save_project(&RealFs::default(), &alive.path, &RealFs::private(), Some(&d.lkg_file(&alive.id)), &project_bytes(2))
        .unwrap();
    snapshot(&d, &alive, 3);
    drop(ProjectLock::acquire(&d.lock_file(&alive.id)).unwrap());
    cache.put(Scope::Project(alive.id), &Hash::of(b"t"), b"thumb").unwrap();
    let t1 = plant_temp(&projects, "alive", &mut rng);
    let t2 = plant_temp(&d.recovery_dir(), &alive.id.to_hex(), &mut rng);
    let t3 = plant_temp(d.data_dir(), "projects", &mut rng);
    let t4 = plant_temp(&d.cache_dir().join("shared"), &Hash::of(b"x").to_hex(), &mut rng);
    // Deleted outside the app.
    let gone = new_project(&d, &projects, "gone", 1);
    save_project(&RealFs::default(), &gone.path, &RealFs::private(), Some(&d.lkg_file(&gone.id)), &project_bytes(2))
        .unwrap();
    snapshot(&d, &gone, 2);
    cache.put(Scope::Project(gone.id), &Hash::of(b"t"), b"thumb").unwrap();
    std::fs::remove_file(&gone.path).unwrap();
    // Recovery file of a project that was never registered.
    let stray = ProjectId::new_random().unwrap();
    recovery::write_snapshot(&RealFs::private(), &d.recovery_file(&stray), &project_bytes(1), &container::verify)
        .unwrap();
    // On a drive that is not mounted right now: keep.
    let unmounted = ProjectId::new_random().unwrap();
    housekeeping::register(&d, unmounted, &t.path().join("no-such-drive/x.gus")).unwrap();
    recovery::write_snapshot(&RealFs::private(), &d.recovery_file(&unmounted), &project_bytes(1), &container::verify)
        .unwrap();

    // Young temp files are left alone by default.
    let first = housekeeping::run(&d, &Options::default()).unwrap();
    assert_eq!(first.temp_files, 0);
    assert!(t1.exists());

    let r = housekeeping::run(&d, &now_opts()).unwrap();
    assert_eq!(r.temp_files, 4, "{r:?}");
    for t in [&t1, &t2, &t3, &t4] {
        assert!(!t.exists());
    }
    assert!(d.recovery_file(&alive.id).exists(), "crash recovery data must be kept");
    assert!(d.lkg_file(&alive.id).exists());
    assert!(cache.project_scopes().unwrap().contains(&alive.id));
    assert!(!d.lock_file(&alive.id).exists(), "stale lock not removed");
    for f in [d.recovery_file(&gone.id), d.lkg_file(&gone.id), d.recovery_file(&stray)] {
        assert!(!f.exists(), "{} not removed", f.display());
    }
    assert!(!cache.project_scopes().unwrap().contains(&gone.id));
    assert!(d.recovery_file(&unmounted).exists());
    let reg = Registry::load(&d.registry_file()).unwrap();
    assert!(reg.get(&gone.id).is_none() && reg.get(&alive.id).is_some() && reg.get(&unmounted).is_some());
    assert_eq!(first.stale_locks, 1);
    assert_eq!(first.unregistered, 1);
    assert_eq!(first.recovery_files, 2);
    assert_eq!(first.lkg_files, 1);
    assert_eq!(first.cache_scopes, 1);

    // Idempotent.
    let r = housekeeping::run(&d, &now_opts()).unwrap();
    assert_eq!(r, housekeeping::Report::default());
}

#[test]
fn housekeeping_never_touches_locked_projects() {
    let t = TempDir::new("hk-locked");
    let d = dirs(&t);
    let projects = t.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut rng = Rng(6);
    // Open in a running instance; even "gone" from disk (e.g. being renamed), its data
    // must not be touched while the lock is held.
    let p = new_project(&d, &projects, "open", 1);
    snapshot(&d, &p, 2);
    let (lock, _) = ProjectLock::acquire(&d.lock_file(&p.id)).unwrap();
    std::fs::remove_file(&p.path).unwrap();
    let t1 = plant_temp(&projects, "open", &mut rng);
    let t2 = plant_temp(&d.recovery_dir(), &p.id.to_hex(), &mut rng);
    let before = all_files(t.path());

    let r = housekeeping::run(&d, &now_opts()).unwrap();
    assert_eq!(r.skipped_locked, 1);
    assert_eq!(all_files(t.path()), before, "housekeeping touched a locked project's files");
    assert!(t1.exists() && t2.exists());
    assert!(Registry::load(&d.registry_file()).unwrap().get(&p.id).is_some());

    // Once released, the same run cleans everything.
    lock.release().unwrap();
    housekeeping::run(&d, &now_opts()).unwrap();
    assert!(!t1.exists() && !t2.exists() && !d.recovery_file(&p.id).exists() && !d.lkg_file(&p.id).exists());
}

#[test]
fn delete_project_removes_everything() {
    let t = TempDir::new("hk-delete");
    let d = dirs(&t);
    let projects = t.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let cache = Cache::open(d.cache_dir(), 1 << 20).unwrap();
    let p = new_project(&d, &projects, "doomed", 1);
    save_project(&RealFs::default(), &p.path, &RealFs::private(), Some(&d.lkg_file(&p.id)), &project_bytes(2)).unwrap();
    snapshot(&d, &p, 3);
    cache.put(Scope::Project(p.id), &Hash::of(b"t"), b"thumb").unwrap();
    let mut rng = Rng(8);
    plant_temp(&projects, "doomed", &mut rng);
    let keep = new_project(&d, &projects, "keep", 1);

    // Another instance has it open: refused, nothing deleted.
    let (other, _) = ProjectLock::acquire(&d.lock_file(&p.id)).unwrap();
    let r = housekeeping::delete_project(&d, &p.path, None, None, true);
    assert!(matches!(r, Err(housekeeping::DeleteError::InUse(_))));
    assert!(p.path.exists() && d.recovery_file(&p.id).exists());
    // We have it open ourselves.
    housekeeping::delete_project(&d, &p.path, None, Some(other), true).unwrap();

    let left: Vec<PathBuf> = all_files(t.path());
    let mentions = |f: &PathBuf| {
        let s = f.to_string_lossy();
        s.contains(&p.id.to_hex()) || s.contains("doomed")
    };
    assert!(!left.iter().any(mentions), "leftovers: {left:?}");
    assert!(keep.path.exists() && !d.lkg_file(&keep.id).exists());
    let reg = Registry::load(&d.registry_file()).unwrap();
    assert!(reg.get(&p.id).is_none() && reg.get(&keep.id).is_some());
}

/// N sessions of edits, saves, snapshots, crashes, clean closes and deletions (inside
/// and outside the app), with housekeeping at every start: no orphans accumulate and
/// there is never more than one recovery file per project.
#[test]
fn hygiene_over_many_sessions() {
    let t = TempDir::new("hygiene");
    let d = dirs(&t);
    let projects = t.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let cache = Cache::open(d.cache_dir(), 64 << 10).unwrap();
    let mut rng = Rng(2024);
    let mut live: Vec<(Project, u64)> = Vec::new();
    let mut created = 0;

    for session in 0..40 {
        housekeeping::run(&d, &now_opts()).unwrap();
        // Invariant at every session start.
        let recovery_files = all_files(&d.recovery_dir());
        for f in &recovery_files {
            assert!(!durable::is_temp_name(f.file_name().unwrap()), "session {session}: orphan {}", f.display());
        }
        assert!(recovery_files.len() <= live.len(), "session {session}: more recovery files than projects");

        if live.len() < 4 || rng.below(3) == 0 {
            live.push((new_project(&d, &projects, &format!("p{created}"), 1), 1));
            created += 1;
        }
        let mut locks = Vec::new();
        for (p, seq) in live.iter_mut() {
            if rng.below(2) == 0 {
                continue;
            }
            let (lock, _) = ProjectLock::acquire(&d.lock_file(&p.id)).unwrap();
            for _ in 0..rng.below(4) {
                *seq += 1;
                if rng.below(2) == 0 {
                    snapshot(&d, p, *seq);
                } else {
                    let lkg = d.lkg_file(&p.id);
                    save_project(&RealFs::default(), &p.path, &RealFs::private(), Some(&lkg), &project_bytes(*seq))
                        .unwrap();
                }
                cache.put(Scope::Project(p.id), &Hash::of(&seq.to_le_bytes()), &[1u8; 2000]).unwrap();
                cache.put(Scope::Shared, &Hash::of(&[seq.to_le_bytes(), [9; 8]].concat()), &[2u8; 3000]).unwrap();
            }
            locks.push((p.id, lock));
        }
        let crash = rng.below(3) == 0;
        for (id, lock) in locks {
            if crash {
                // Power loss mid-write leaves temp files next to the project and in our dirs.
                let p = &live.iter().find(|(p, _)| p.id == id).unwrap().0;
                plant_temp(&projects, p.path.file_stem().unwrap().to_str().unwrap(), &mut rng);
                plant_temp(&d.recovery_dir(), &id.to_hex(), &mut rng);
                plant_temp(&d.cache_dir().join(id.to_hex()), &Hash::of(b"c").to_hex(), &mut rng);
                drop(lock);
            } else {
                recovery::remove(&RealFs::private(), &d.recovery_file(&id)).unwrap();
                lock.release().unwrap();
            }
        }
        // Deletions between sessions.
        if live.len() > 2 && rng.below(3) == 0 {
            let (p, _) = live.remove(rng.below(live.len() as u64) as usize);
            if rng.below(2) == 0 {
                std::fs::remove_file(&p.path).unwrap(); // outside the app
            } else {
                housekeeping::delete_project(&d, &p.path, Some(p.id), None, true).unwrap();
            }
        }
    }
    housekeeping::run(&d, &now_opts()).unwrap();

    let live_ids: Vec<String> = live.iter().map(|(p, _)| p.id.to_hex()).collect();
    let everything = all_files(t.path());
    for f in &everything {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!durable::is_temp_name(f.file_name().unwrap()), "orphan temp {}", f.display());
        if f.starts_with(d.recovery_dir()) || f.starts_with(d.lkg_dir()) {
            assert!(live_ids.iter().any(|id| name.starts_with(id)), "orphan {}", f.display());
        }
        if f.starts_with(d.cache_dir()) && !f.starts_with(d.cache_dir().join("shared")) {
            assert!(live_ids.iter().any(|id| f.to_string_lossy().contains(id)), "orphan cache {}", f.display());
        }
    }
    assert!(all_files(&d.locks_dir()).is_empty(), "stale locks left");
    for id in &live_ids {
        let n = all_files(&d.recovery_dir()).iter().filter(|f| f.to_string_lossy().contains(id)).count();
        assert!(n <= 1);
    }
    let reg = Registry::load(&d.registry_file()).unwrap();
    assert_eq!(reg.len(), live.len());
    assert!(cache.usage().unwrap() <= cache.cap());
    let project_files = all_files(&projects);
    assert_eq!(project_files.len(), live.len(), "{project_files:?}");
    assert!(created > live.len(), "the test should have deleted some projects");
}
