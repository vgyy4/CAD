use super::*;
use gusset_doc::Document;
use gusset_history::{Ctx, NodeState};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let p = std::env::temp_dir().join(format!("gusset-project-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn dirs(&self) -> DataDirs {
        DataDirs::at(self.0.join("data"), self.0.join("cache"))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn doc(a: &str) -> Document {
    Document::from_text(&format!("gusset 1\nunits mm\n\nparam a = {a}\n")).unwrap()
}

fn sample_history() -> History {
    let u = Ctx::user(10);
    let mut h = History::new(&doc("1 mm"), &u);
    let main = h.main();
    h.commit(&u, main, &doc("2 mm"), "a=2", NodeState::Ok).unwrap();
    let root = h.lineage(h.head(main).unwrap()).last().copied().unwrap();
    let b = h.create_branch(&u, root, Some("Lighter")).unwrap();
    h.commit(&u, b, &doc("0.5 mm"), "light", NodeState::Ok).unwrap();
    h.put_blob(b"imported bytes");
    h
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            v.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    v.sort();
    v
}

#[test]
fn encode_decode_roundtrip() {
    let h = sample_history();
    let meta = ProjectMeta { id: ProjectId::new_random().unwrap(), save_seq: 3, base_seq: 3, created: 5 };
    let bytes = encode(&h, &meta).unwrap();
    let (h2, m2) = decode(&bytes).unwrap();
    assert_eq!(m2, meta);
    assert_eq!(h2.to_records().nodes, h.to_records().nodes);
    assert_eq!(h2.to_records().branches, h.to_records().branches);
    assert_eq!(h2.head_document(h2.main()).unwrap(), doc("2 mm"));
    // Deterministic: same history and meta → identical bytes.
    assert_eq!(encode(&h2, &m2).unwrap(), bytes);
}

#[test]
fn save_open_close_lifecycle_and_hygiene() {
    let t = Tmp::new("life");
    let file = t.0.join("part.gus");
    let h = sample_history();
    let mut p = Project::create(t.dirs()).unwrap();
    assert!(p.save(&h).is_err(), "never-saved project needs Save As");
    p.save_as(&file, &h).unwrap();
    p.save(&h).unwrap();
    assert_eq!(p.meta.save_seq, 2);
    p.close();
    // Only the project file next to the project: no temp or sidecar files.
    assert_eq!(files_in(&t.0), vec!["cache".to_string(), "data".to_string(), "part.gus".to_string()]);
    let (p, h2, rep) = Project::open(&file, t.dirs()).unwrap();
    assert!(matches!(rep, OpenReport::Clean));
    assert_eq!(h2.head_document(h2.main()).unwrap(), doc("2 mm"));
    let id = p.meta.id;
    p.close();
    // Exactly zero recovery files after a clean close; one LKG copy.
    assert!(files_in(&t.dirs().recovery_dir()).is_empty());
    assert_eq!(files_in(&t.dirs().lkg_dir()).len(), 1);
    assert!(t.dirs().lkg_file(&id).exists());
}

#[test]
fn crash_leaves_recovery_that_is_offered() {
    let t = Tmp::new("crash");
    let file = t.0.join("part.gus");
    let mut h = sample_history();
    let mut p = Project::create(t.dirs()).unwrap();
    p.save_as(&file, &h).unwrap();
    // Unsaved work, snapshotted twice (two slots, one file).
    let u = Ctx::user(20);
    let main = h.main();
    h.commit(&u, main, &doc("9 mm"), "unsaved", NodeState::Ok).unwrap();
    p.snapshot(&h).unwrap();
    h.commit(&u, main, &doc("10 mm"), "unsaved2", NodeState::Ok).unwrap();
    let seq = p.snapshot(&h).unwrap();
    assert_eq!(files_in(&t.dirs().recovery_dir()).len(), 1);
    drop(p); // simulated crash: no clean close
    let (p, saved, rep) = Project::open(&file, t.dirs()).unwrap();
    assert_eq!(saved.head_document(saved.main()).unwrap(), doc("2 mm"));
    match rep {
        OpenReport::RecoveryAvailable { recovered, recovery_seq } => {
            assert_eq!(recovery_seq, seq);
            assert_eq!(recovered.head_document(recovered.main()).unwrap(), doc("10 mm"));
        }
        other => panic!("{other:?}"),
    }
    p.close();
    assert!(files_in(&t.dirs().recovery_dir()).is_empty());
}

#[test]
fn damaged_file_falls_back_to_lkg_or_salvage() {
    let t = Tmp::new("damage");
    let file = t.0.join("part.gus");
    let h = sample_history();
    let mut p = Project::create(t.dirs()).unwrap();
    p.save_as(&file, &h).unwrap();
    p.save(&h).unwrap(); // second save creates the LKG copy
    p.close();
    let mut bytes = std::fs::read(&file).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&file, &bytes).unwrap();
    let (p, h2, rep) = Project::open(&file, t.dirs()).unwrap();
    match rep {
        OpenReport::Damaged { source, reason } => {
            assert!(matches!(source, DamageSource::LastKnownGood | DamageSource::Salvaged), "{source:?}");
            assert!(!reason.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert!(h2.head_document(h2.main()).is_ok());
    p.close();
    // Pure salvage (no LKG, no recovery) still recovers a document.
    let s = salvage(&bytes, 1).unwrap();
    assert!(s.head_document(s.main()).is_ok());
    // Total garbage: a clear error, no panic.
    std::fs::write(&file, b"not a gusset file").unwrap();
    let e = Project::open(&file, Tmp::new("garbage").dirs()).unwrap_err();
    assert!(e.to_string().contains("damaged"), "{e}");
}

#[test]
fn second_open_is_refused_while_locked() {
    let t = Tmp::new("lock");
    let file = t.0.join("part.gus");
    let mut p = Project::create(t.dirs()).unwrap();
    p.save_as(&file, &sample_history()).unwrap();
    let e = Project::open(&file, t.dirs()).unwrap_err();
    assert!(matches!(e, ProjectError::Locked { .. }), "{e}");
    p.close();
    let (p2, _, _) = Project::open(&file, t.dirs()).unwrap();
    p2.close();
}

#[test]
fn untitled_crash_recovery_is_offered_and_survives_housekeeping() {
    // Review H1.
    let t = Tmp::new("untitled");
    let mut h = sample_history();
    let mut p = Project::create(t.dirs()).unwrap();
    let id = p.meta.id;
    let u = Ctx::user(30);
    let main = h.main();
    h.commit(&u, main, &doc("7 mm"), "never saved", NodeState::Ok).unwrap();
    p.snapshot(&h).unwrap();
    drop(p); // crash before the first save
    let opts = housekeeping::Options { temp_min_age: std::time::Duration::ZERO, ..Default::default() };
    housekeeping::run(&t.dirs(), &opts).unwrap();
    assert!(t.dirs().recovery_file(&id).exists(), "housekeeping deleted an untitled project's recovery");
    let pending = pending_recoveries(&t.dirs());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id);
    assert_eq!(pending[0].path, None);
    let (mut p, rec) = Project::recover_unsaved(t.dirs(), id).unwrap();
    assert_eq!(rec.head_document(rec.main()).unwrap(), doc("7 mm"));
    assert!(p.path().is_none());
    // While it is open again, it is not offered a second time.
    assert!(pending_recoveries(&t.dirs()).is_empty());
    let file = t.0.join("rescued.gus");
    p.save_as(&file, &rec).unwrap();
    p.close();
    assert!(pending_recoveries(&t.dirs()).is_empty());
    assert!(files_in(&t.dirs().recovery_dir()).is_empty());
    // Discarding instead also works.
    let mut p = Project::create(t.dirs()).unwrap();
    p.snapshot(&h).unwrap();
    let id = p.meta.id;
    drop(p);
    assert_eq!(pending_recoveries(&t.dirs()).len(), 1);
    discard_pending_recovery(&t.dirs(), id).unwrap();
    assert!(pending_recoveries(&t.dirs()).is_empty());
}

#[test]
fn save_as_gives_the_new_file_its_own_identity() {
    // Review H2, scenario 1.
    let t = Tmp::new("saveas");
    let (a, b) = (t.0.join("a.gus"), t.0.join("b.gus"));
    let h = sample_history();
    let mut p = Project::create(t.dirs()).unwrap();
    p.save_as(&a, &h).unwrap();
    p.save(&h).unwrap();
    let id_a = p.meta.id;
    p.save_as(&b, &h).unwrap();
    assert_ne!(p.meta.id, id_a, "Save As must give the new file a new project ID");
    assert_eq!(p.path(), Some(b.as_path()));
    // a.gus opens next to b.gus (different locks), and keeps its ID.
    let (pa, _, rep) = Project::open(&a, t.dirs()).unwrap();
    assert!(matches!(rep, OpenReport::Clean));
    assert_eq!(pa.meta.id, id_a);
    assert!(pa.reidentified_from.is_none());
    assert_ne!(t.dirs().lkg_file(&pa.meta.id), t.dirs().lkg_file(&p.meta.id));
    pa.close();
    p.close();
}

#[test]
fn file_manager_copy_is_reidentified_on_open() {
    // Review H2, scenario 2: a copy opened beside its original.
    let t = Tmp::new("copy");
    let (a, b) = (t.0.join("a.gus"), t.0.join("b.gus"));
    let h = sample_history();
    let mut p = Project::create(t.dirs()).unwrap();
    p.save_as(&a, &h).unwrap();
    let id_a = p.meta.id;
    std::fs::copy(&a, &b).unwrap();
    // The original is open: the copy still opens, with its own identity.
    let (pb, hb, rep) = Project::open(&b, t.dirs()).unwrap();
    assert!(matches!(rep, OpenReport::Clean));
    assert_eq!(pb.reidentified_from, Some(id_a));
    let id_b = pb.meta.id;
    assert_ne!(id_b, id_a);
    assert_eq!(hb.head_document(hb.main()).unwrap(), doc("2 mm"));
    pb.close();
    p.close();
    // The new identity was written into the copy: it is stable from now on.
    let (pb, _, _) = Project::open(&b, t.dirs()).unwrap();
    assert_eq!(pb.meta.id, id_b);
    assert!(pb.reidentified_from.is_none());
    pb.close();
    let (pa, _, _) = Project::open(&a, t.dirs()).unwrap();
    assert_eq!(pa.meta.id, id_a);
    pa.close();
}

#[test]
fn one_copys_recovery_is_never_offered_for_another() {
    // Review H2, scenario 2 of the report: two files share an ID (made by an older
    // build's Save As); b crashed with unsaved work; opening a must neither offer nor
    // delete b's recovery.
    let t = Tmp::new("shared-id");
    let (a, b) = (t.0.join("a.gus"), t.0.join("b.gus"));
    let mut h = sample_history();
    let mut p = Project::create(t.dirs()).unwrap();
    p.save_as(&a, &h).unwrap();
    let id = p.meta.id;
    p.close();
    std::fs::copy(&a, &b).unwrap();
    let (mut pb, _, _) = {
        // Simulate the old build: b registered under the same ID.
        housekeeping::register(&t.dirs(), id, &b).unwrap();
        Project::open(&b, t.dirs()).unwrap()
    };
    assert_eq!(pb.meta.id, id, "b is the registered owner of the ID");
    let main = h.main();
    h.commit(&Ctx::user(40), main, &doc("11 mm"), "b's unsaved work", NodeState::Ok).unwrap();
    pb.snapshot(&h).unwrap();
    drop(pb); // crash
    let (pa, _, rep) = Project::open(&a, t.dirs()).unwrap();
    assert!(matches!(rep, OpenReport::Clean), "b's recovery offered for a: {rep:?}");
    assert_ne!(pa.meta.id, id);
    pa.close();
    assert!(t.dirs().recovery_file(&id).exists(), "b's only recovery copy was deleted");
    let (pb, _, rep) = Project::open(&b, t.dirs()).unwrap();
    match rep {
        OpenReport::RecoveryAvailable { recovered, .. } => {
            assert_eq!(recovered.head_document(recovered.main()).unwrap(), doc("11 mm"));
        }
        other => panic!("{other:?}"),
    }
    pb.close();
}
