//! Gusset's single history model (ADR 0006): undo/redo, the feature timeline and the
//! iteration tree are one system.
//!
//! - Immutable, content-addressed objects ([`objects`]) give structural sharing: a new
//!   iteration costs only its changed features.
//! - Nodes form a tree; branches are named heads. Undo moves a head back without deleting
//!   anything, so the "redo future" is never lost.
//! - Every change is an [`Event`] in one log, used for undo, the audit log and the UI.

#![cfg_attr(test, allow(clippy::float_cmp))]

pub mod diff;
pub mod history;
pub mod objects;
pub mod persist;

pub use diff::{Change, DocDiff, ItemChange};
pub use history::*;
pub use objects::{Hash, ObjectKind, ObjectStore, Snapshot};
pub use persist::HistoryRecords;

#[cfg(test)]
mod tests {
    use super::*;
    use gusset_doc::Document;

    fn doc(params: &[(&str, &str)]) -> Document {
        let mut t = String::from("gusset 1\nunits mm\n\n");
        for (n, v) in params {
            t.push_str(&format!("param {n} = {v}\n"));
        }
        Document::from_text(&t).unwrap()
    }

    fn with_feature(mut d: Document, sketch: &str) -> Document {
        let f = Document::from_text(&format!("gusset 1\nunits mm\n\nsketch {sketch} (plane: XY, next_id: 1) {{\n}}\n"))
            .unwrap()
            .features
            .remove(0);
        d.features.push(f);
        d
    }

    #[test]
    fn undo_is_a_tree() {
        let u = Ctx::user(100);
        let mut h = History::new(&doc(&[("a", "1 mm")]), &u);
        let main = h.main();
        let n1 = h.commit(&u, main, &doc(&[("a", "2 mm")]), "a=2", NodeState::Ok).unwrap();
        let n2 = h.commit(&u, main, &doc(&[("a", "3 mm")]), "a=3", NodeState::Ok).unwrap();
        h.undo(&u, main).unwrap();
        assert_eq!(h.head(main).unwrap(), n1);
        // A new edit after undo creates a sibling; n2 still exists.
        let n3 = h.commit(&u, main, &doc(&[("a", "4 mm")]), "a=4", NodeState::Ok).unwrap();
        assert_eq!(h.node(n3).unwrap().parent, Some(n1));
        assert_eq!(h.children(n1), vec![n2, n3]);
        assert!(h.node(n2).is_ok());
        // Redo stack was cleared by the new edit.
        assert!(h.redo(&u, main).is_err());
        h.undo(&u, main).unwrap();
        h.redo(&u, main).unwrap();
        assert_eq!(h.head(main).unwrap(), n3);
        // Committing an identical document records nothing.
        assert_eq!(h.commit(&u, main, &doc(&[("a", "4 mm")]), "same", NodeState::Ok).unwrap(), n3);
    }

    #[test]
    fn undo_message_names_the_undone_node_and_new_head() {
        let ctx = Ctx::user(1);
        let mut history = History::new(&doc(&[]), &ctx);
        let branch = history.main();
        let old_head = history.head(branch).unwrap();
        let new_head = history.commit(&ctx, branch, &doc(&[("a", "1 mm")]), "edit", NodeState::Ok).unwrap();
        let message = history.undo(&ctx, branch).unwrap();
        assert_eq!(message, format!("Undid {new_head} (edited {branch}); now at {old_head}"));
    }

    #[test]
    fn per_actor_and_per_branch_undo() {
        let u = Ctx::user(1);
        let a = Ctx::agent("claude-1", 2);
        let mut h = History::new(&doc(&[]), &u);
        let main = h.main();
        let root = h.head(main).unwrap();
        let ab = h.create_branch(&a, root, Some("agent/claude-1")).unwrap();
        assert_eq!(h.branch(ab).unwrap().color.as_deref(), Some("agent"));
        h.commit(&a, ab, &doc(&[("x", "1 mm")]), "agent edit", NodeState::Ok).unwrap();
        let un = h.commit(&u, main, &doc(&[("y", "1 mm")]), "user edit", NodeState::Ok).unwrap();
        // The agent's undo does not touch the user's branch.
        h.undo(&a, ab).unwrap();
        assert_eq!(h.head(ab).unwrap(), root);
        assert_eq!(h.head(main).unwrap(), un);
        // The user has nothing to undo on the agent branch, but can undo on main.
        assert!(!h.can_undo(&Actor::User, ab));
        assert!(h.can_undo(&Actor::User, main));
    }

    #[test]
    fn undo_conflict_is_reported() {
        let u = Ctx::user(1);
        let a = Ctx::agent("bot", 1);
        let mut h = History::new(&doc(&[]), &u);
        let main = h.main();
        h.commit(&u, main, &doc(&[("a", "1 mm")]), "u", NodeState::Ok).unwrap();
        h.commit(&a, main, &doc(&[("a", "2 mm")]), "a", NodeState::Ok).unwrap();
        // The user's edit is no longer the head; undoing it would discard the agent's edit.
        assert!(matches!(h.undo(&u, main), Err(HistoryError::Conflict(_))));
    }

    #[test]
    fn branch_lifecycle() {
        let u = Ctx::user(10);
        let mut h = History::new(&doc(&[]), &u);
        let main = h.main();
        let root = h.head(main).unwrap();
        let b = h.create_branch(&u, root, None).unwrap();
        assert_eq!(h.branch(b).unwrap().name, "Iteration 2");
        assert!(matches!(h.create_branch(&u, root, Some("Iteration 2")), Err(HistoryError::NameTaken(_))));
        h.rename_branch(&u, b, "Lighter").unwrap();
        // Main can't be deleted; others need confirmation.
        assert_eq!(h.delete_branch(&u, main, true), Err(HistoryError::MainCannotBeDeleted));
        let n = h.commit(&u, b, &doc(&[("w", "1 mm")]), "w", NodeState::Ok).unwrap();
        h.set_starred(&u, n, true).unwrap();
        match h.delete_branch(&u, b, false) {
            Err(HistoryError::ConfirmationRequired { reason }) => assert!(reason.contains("1 starred")),
            other => panic!("{other:?}"),
        }
        h.delete_branch(&u, b, true).unwrap();
        assert_eq!(h.branches().len(), 1);
        assert_eq!(h.deleted_branches().len(), 1);
        // Deletion is undoable.
        h.undo(&u, main).unwrap();
        assert_eq!(h.branches().len(), 2);
        // Set main is logged and undoable.
        h.set_main(&u, b).unwrap();
        assert_eq!(h.main(), b);
        assert!(h.delete_branch(&u, b, true).is_err());
        h.undo(&u, main).unwrap();
        assert_eq!(h.main(), main);
    }

    #[test]
    fn undo_of_branch_delete_or_create_never_duplicates_names() {
        let u = Ctx::user(1);
        let mut h = History::new(&doc(&[]), &u);
        let main = h.main();
        let root = h.head(main).unwrap();
        // Redo of a branch creation after another actor took the name.
        let b = h.create_branch(&u, root, Some("Wing")).unwrap();
        h.undo(&u, main).unwrap();
        assert!(h.branch(b).unwrap().deleted_at.is_some());
        let other = h.create_branch(&Ctx::agent("bot", 2), root, Some("WING")).unwrap();
        h.redo(&u, main).unwrap();
        assert!(h.branch(other).unwrap().deleted_at.is_none());
        assert!(h.branch(b).unwrap().deleted_at.is_none());
        assert_eq!(h.branch(b).unwrap().name, "Wing (restored)");
        assert_eq!(h.branch_by_name_folded("wing").unwrap().id, other);

        // The review's scenario: delete, recreate the same default name, undo the delete.
        let mut h = History::new(&doc(&[]), &u);
        let main = h.main();
        let root = h.head(main).unwrap();
        let b = h.create_branch(&u, root, None).unwrap();
        h.delete_branch(&u, b, true).unwrap();
        let c = h.create_branch(&Ctx::agent("bot", 2), root, None).unwrap();
        assert_eq!(h.branch(c).unwrap().name, h.branch(b).unwrap().name);
        h.undo(&u, main).unwrap();
        let names: Vec<&str> = h.branches().iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["Main", "Iteration 2 (restored)", "Iteration 2"]);
        assert_eq!(h.branch_by_name("Iteration 2").unwrap().id, c);
    }

    fn sizes_match(h: &History) {
        let r = h.to_records();
        let s = h.record_sizes();
        assert_eq!((s.nodes, s.branches, s.events), (r.nodes.len(), r.branches.len(), r.events.len()));
    }

    fn small_limits(bytes: usize) -> HistoryLimits {
        HistoryLimits { max_record_bytes: bytes, max_branches: 50, ..HistoryLimits::default() }
    }

    #[test]
    fn write_limits_keep_every_history_savable() {
        // Security review #5: ~150k edits made the nodes record exceed the container's
        // 16 MiB record limit, so nothing could be saved. Scaled down: 32 KiB records.
        let u = Ctx::user(0);
        let a = Ctx::agent("bot", 0);
        let mut h = History::new(&doc(&[]), &u);
        h.set_limits(small_limits(32 << 10));
        let main = h.main();
        let root = h.head(main).unwrap();
        let star = h.commit(&u, main, &doc(&[("st", "1 mm")]), "starred", NodeState::Ok).unwrap();
        h.set_starred(&u, star, true).unwrap();
        for i in 0..3000u64 {
            let c = Ctx::user(i);
            h.commit(&c, main, &doc(&[("a", &format!("{i} mm"))]), "edit with a fairly long label", NodeState::Ok)
                .unwrap();
            h.record_agent_action(&Ctx::agent("bot", i), "feature_add", Some(main), "ok", &"x".repeat(400));
            let s = h.record_sizes();
            assert!(s.nodes <= 32 << 10 && s.events <= 32 << 10 && s.branches <= 32 << 10, "{s:?}");
        }
        sizes_match(&h);
        // Starred and head iterations survived compaction; undo of the last edit works.
        assert!(h.node(star).unwrap().starred);
        assert_eq!(h.head_document(main).unwrap(), doc(&[("a", "2999 mm")]));
        h.undo(&u, main).unwrap();
        assert_eq!(h.head_document(main).unwrap(), doc(&[("a", "2998 mm")]));
        sizes_match(&h);
        let back = History::from_records(&h.to_records()).unwrap();
        assert_eq!(back.head_document(main).unwrap(), h.head_document(main).unwrap());

        // Branches: a clean error at the limit (live branches are never purged).
        let mut made = 0;
        let err = loop {
            match h.create_branch(&a, root, None) {
                Ok(_) => made += 1,
                Err(e) => break e,
            }
            assert!(made <= 50);
        };
        assert!(matches!(err, HistoryError::LimitReached(_)), "{err:?}");
        sizes_match(&h);
        // Deleted branches make room again (oldest purged first, only when needed).
        let victim = h.branches().last().unwrap().id;
        h.delete_branch(&u, victim, true).unwrap();
        assert!(h.create_branch(&u, root, Some("after")).is_ok());
        sizes_match(&h);
        let back = History::from_records(&h.to_records()).unwrap();
        assert_eq!(back.branches().len(), h.branches().len());
    }

    #[test]
    fn oversized_objects_and_notes_are_refused() {
        let u = Ctx::user(0);
        let mut h = History::new(&doc(&[]), &u);
        h.set_limits(HistoryLimits { max_object_bytes: 200, ..small_limits(4 << 10) });
        let main = h.main();
        let many: Vec<(String, String)> = (0..20).map(|i| (format!("p{i}"), "1 mm".to_string())).collect();
        let refs: Vec<(&str, &str)> = many.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let objects = h.objects().len();
        let e = h.commit(&u, main, &doc(&refs), "big", NodeState::Ok).unwrap_err();
        assert!(matches!(e, HistoryError::LimitReached(_)), "{e:?}");
        assert_eq!(h.objects().len(), objects, "nothing was stored");
        let e = h.set_branch_meta(main, None, Some("n".repeat(5000)), None).unwrap_err();
        assert!(matches!(e, HistoryError::LimitReached(_)), "{e:?}");
        sizes_match(&h);
    }

    #[test]
    fn default_write_limits_are_within_load_limits() {
        let l = HistoryLimits::default();
        assert!(l.max_record_bytes < RECORD_LIMIT);
        assert!(l.max_object_bytes <= RECORD_LIMIT);
        assert!(l.max_branches <= persist::MAX_BRANCHES);
        assert!(l.max_nodes <= persist::MAX_NODES);
        assert!(l.max_events <= persist::MAX_EVENTS);
        // A history cannot be given looser limits than these.
        let mut h = History::new(&doc(&[]), &Ctx::user(0));
        h.set_limits(HistoryLimits { max_branches: usize::MAX, max_record_bytes: usize::MAX, ..l.clone() });
        assert_eq!(h.limits(), &l);
    }

    #[test]
    fn record_sizes_track_every_mutation() {
        let u = Ctx::user(5);
        let mut h = History::new(&doc(&[]), &u);
        let main = h.main();
        let root = h.head(main).unwrap();
        let n = h.commit(&u, main, &doc(&[("a", "1 mm")]), "a", NodeState::Broken { message: "x\ty".into() }).unwrap();
        let b = h.create_branch(&u, root, Some("Wide %")).unwrap();
        h.rename_branch(&u, b, "Wider\u{e9}").unwrap();
        h.set_branch_meta(b, Some(Some("#ff0000".into())), Some("notes\nmore".into()), Some(true)).unwrap();
        h.name_node(&u, n, Some("Baseline")).unwrap();
        h.set_starred(&u, n, true).unwrap();
        h.set_rollback(&u, main, Some(0)).unwrap();
        h.move_head(&u, b, n).unwrap();
        h.set_main(&u, b).unwrap();
        h.undo(&u, main).unwrap();
        h.delete_branch(&u, b, true).unwrap();
        h.undo(&u, main).unwrap();
        h.redo(&u, main).unwrap();
        h.restore_branch(&u, b).unwrap();
        h.record_agent_action(&Ctx::agent("a\tb", 9), "x", None, "ok", "d");
        sizes_match(&h);
        h.gc(1_000_000_000, &GcPolicy::default());
        sizes_match(&h);
    }

    #[test]
    fn structural_sharing_and_gc() {
        let u = Ctx::user(0);
        let base = with_feature(with_feature(doc(&[("a", "1 mm")]), "S1"), "S2");
        let mut h = History::new(&base, &u);
        let main = h.main();
        let objects_before = h.objects().len();
        let mut d = base.clone();
        d.params[0].expr = gusset_units::expr::parse("2 mm").unwrap();
        h.commit(&u, main, &d, "a=2", NodeState::Ok).unwrap();
        // Only a new params object and a new snapshot were added; features are shared.
        assert_eq!(h.objects().len(), objects_before + 2);

        // An abandoned branch is purged after retention, its objects collected.
        let root = h.lineage(h.head(main).unwrap()).last().copied().unwrap();
        let b = h.create_branch(&u, root, Some("tmp")).unwrap();
        h.commit(&u, b, &with_feature(base.clone(), "S9"), "s9", NodeState::Ok).unwrap();
        h.delete_branch(&u, b, true).unwrap();
        let rep = h.gc(10, &GcPolicy::default());
        assert_eq!(rep.branches_purged, 0, "still within retention");
        let rep = h.gc(10 + 8 * 24 * 3600, &GcPolicy { squash_after: None, ..GcPolicy::default() });
        assert_eq!(rep.branches_purged, 1);
        assert_eq!(rep.nodes_removed, 1);
        assert!(rep.objects_removed >= 2);
        assert!(h.head_document(main).is_ok());
    }

    #[test]
    fn gc_keeps_everything_when_a_snapshot_is_damaged() {
        // Review N6: a snapshot whose list chunk is missing cannot name its features;
        // GC must not delete them (or anything else) on that basis.
        let u = Ctx::user(0);
        let base = with_feature(with_feature(doc(&[("a", "1 mm")]), "S1"), "S2");
        let mut h = History::new(&base, &u);
        let main = h.main();
        let head = h.head(main).unwrap();
        let snap = h.node(head).unwrap().snapshot;
        let chunk = *h.objects().snapshot(snap).unwrap().chunks.last().unwrap();
        let keep: std::collections::BTreeSet<_> =
            h.objects().iter().map(|(k, _, _)| k).filter(|k| *k != chunk).collect();
        h.objects_mut().retain(&keep);
        let remaining = h.objects().len();
        assert!(h.objects().snapshot(snap).is_err());
        let rep = h.gc(1_000_000_000, &GcPolicy::default());
        assert_eq!(rep.objects_removed, 0);
        assert_eq!(h.objects().len(), remaining);
    }

    #[test]
    fn squash_keeps_heads_and_stars() {
        let u = Ctx::user(0);
        let mut h = History::new(&doc(&[]), &u);
        let main = h.main();
        let mut ids = Vec::new();
        for i in 1..=5 {
            ids.push(h.commit(&u, main, &doc(&[("a", &format!("{i} mm"))]), "e", NodeState::Ok).unwrap());
        }
        h.set_starred(&u, ids[1], true).unwrap();
        let rep = h.gc(1_000_000_000, &GcPolicy::default());
        // ids[0], ids[2], ids[3] squashed; ids[1] starred, ids[4] head.
        assert_eq!(rep.nodes_squashed, 3);
        assert!(h.node(ids[1]).is_ok() && h.node(ids[4]).is_ok());
        assert_eq!(h.node(ids[4]).unwrap().parent, Some(ids[1]));
        assert_eq!(h.head_document(main).unwrap(), doc(&[("a", "5 mm")]));
    }

    #[test]
    fn compare_and_cherry_pick() {
        let u = Ctx::user(0);
        let base = with_feature(doc(&[("a", "1 mm")]), "S1");
        let mut h = History::new(&base, &u);
        let main = h.main();
        let root = h.head(main).unwrap();
        let b = h.create_branch(&u, root, Some("exp")).unwrap();
        let mut d = with_feature(base.clone(), "S2");
        d.params.push(gusset_units::params::Param {
            name: "w".into(),
            expr: gusset_units::expr::parse("5 mm").unwrap(),
            comment: String::new(),
        });
        let nb = h.commit(&u, b, &d, "exp", NodeState::Ok).unwrap();
        let diff = h.compare(root, nb).unwrap();
        assert_eq!(diff.features, vec![ItemChange { name: "S2".into(), change: Change::Added }]);
        assert_eq!(diff.params, vec![ItemChange { name: "w".into(), change: Change::Added }]);
        let picked = h.cherry_pick(nb, main, &["S2".into()], &["w".into()]).unwrap();
        assert!(picked.feature("S2").is_some());
        assert_eq!(picked.params.len(), 2);
        assert!(h.cherry_pick(nb, main, &["Nope".into()], &[]).is_err());
    }

    #[test]
    fn persistence_roundtrip() {
        let u = Ctx::user(5);
        let a = Ctx::agent("tab\tname", 6);
        let mut h = History::new(&with_feature(doc(&[("a", "1 mm")]), "S1"), &u);
        let main = h.main();
        let root = h.head(main).unwrap();
        let b = h.create_branch(&a, root, Some("agent/x")).unwrap();
        h.commit(&a, b, &doc(&[]), "agent\nedit", NodeState::Broken { message: "Fillet1: radius too big".into() })
            .unwrap();
        h.set_rollback(&u, main, Some(0)).unwrap();
        h.name_node(&u, root, Some("Baseline")).unwrap();
        h.delete_branch(&u, b, true).unwrap();
        let rec = h.to_records();
        let h2 = History::from_records(&rec).unwrap();
        let rec2 = h2.to_records();
        assert_eq!(rec.nodes, rec2.nodes);
        assert_eq!(rec.branches, rec2.branches);
        assert_eq!(rec.events, rec2.events);
        assert_eq!(h2.head_document(main).unwrap(), h.head_document(main).unwrap());
        assert_eq!(h2.events().len(), h.events().len());
        // Tampered graph is rejected.
        let mut bad = rec.clone();
        bad.nodes = bad.nodes.replace("\tn1\t", "\tn99\t");
        assert!(History::from_records(&bad).is_err());
        let mut bad = rec;
        if let Some(o) = bad.objects.first_mut() {
            o.2 = std::sync::Arc::from(&b"tampered"[..]);
        }
        assert!(History::from_records(&bad).is_err());
    }
}
