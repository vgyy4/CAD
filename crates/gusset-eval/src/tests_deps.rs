//! Dependency-keyed reuse (ADR 0024): features that do not depend on an edit are not
//! built again, and a warm evaluator always gives what a fresh one gives.

use super::*;
use gusset_kernel::mock::MockKernel;

fn square(name: &str, x: f64) -> String {
    format!(
        "sketch {name} (plane: XY, next_id: 9) {{
  p1 = point({x}, 0)
  p2 = point({}, 0)
  p3 = point({}, 10)
  p4 = point({x}, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}
",
        x + 10.0,
        x + 10.0
    )
}

fn doc(params: [&str; 3], tail: &str) -> Document {
    let [a, b, c] = params;
    let text = format!(
        "gusset 1\nunits mm\nparam a = {a}\nparam b = {b}\nparam c = {c}\n{}extrude E1 (profile: S1, distance: a, op: new)\n{}\
         extrude E2 (profile: S2, distance: b, op: new)\n{}extrude E3 (profile: S3, distance: c, op: add, body: E2)\n{tail}",
        square("S1", 0.0),
        square("S2", 20.0),
        square("S3", 60.0),
    );
    Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

/// Statuses, bodies (names, face names, volumes) and sketches, comparable across evaluators.
fn summary(ev: &Evaluator, r: &Evaluation) -> String {
    let mut s = format!("{:?}\n", r.statuses);
    for b in &r.bodies {
        let v = ev.kernel().mass_properties(&b.shape).map(|m| m.volume).unwrap_or(f64::NAN);
        let faces: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
        s.push_str(&format!("{} {} {v:.9} {faces:?}\n", b.name, b.feature));
    }
    s.push_str(&format!("{:?}", r.sketches.keys().collect::<Vec<_>>()));
    s
}

#[test]
fn an_edit_rebuilds_only_what_depends_on_it() {
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc(["5 mm", "7 mm", "3 mm"], ""), None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.recomputed, 6);
    // `a` drives E1 only: S2, E2, S3 and E3 are replayed, not built.
    let edited = doc(["6 mm", "7 mm", "3 mm"], "");
    let r = ev.evaluate(&edited, None, &());
    assert_eq!(r.recomputed, 1, "{:?}", r.statuses);
    let mut fresh = Evaluator::new(Arc::new(MockKernel));
    let f = fresh.evaluate(&edited, None, &());
    assert_eq!(summary(&ev, &r), summary(&fresh, &f));
    // `b` drives E2, and E3 changes E2: both are built, E1 is not.
    let r = ev.evaluate(&doc(["6 mm", "8 mm", "3 mm"], ""), None, &());
    assert_eq!(r.recomputed, 2, "{:?}", r.statuses);
}

#[test]
fn a_warm_evaluator_matches_a_fresh_one_across_edits() {
    // Barriers (no `body:`, a face reference), combine, a copy and a failing feature.
    let tail = format!(
        "plane F1 (base: E1/cap(end), offset: c)\n{}extrude E4 (profile: S4, distance: c, op: add)\n\
         move M1 (body: E2, translate: [0 mm, 30 mm, 0 mm], copy: true)\n\
         extrude E5 (profile: S2, distance: 0 mm, op: new)\ncombine C1 (body: E2, tools: [M1])\n",
        square("S4", 100.0)
    );
    let values = [
        ["5 mm", "7 mm", "3 mm"],
        ["6 mm", "7 mm", "3 mm"],
        ["6 mm", "9 mm", "3 mm"],
        ["6 mm", "9 mm", "2 mm"],
        ["5 mm", "7 mm", "3 mm"],
        ["5 mm", "7 mm", "30 mm"],
        ["4 mm", "7 mm", "3 mm"],
    ];
    let mut warm = Evaluator::new(Arc::new(MockKernel));
    let r = warm.evaluate(&doc(values[0], &tail), None, &());
    for ok in ["F1", "E4", "M1", "C1"] {
        assert!(matches!(r.status(ok), Some(FeatureStatus::Ok)), "{ok}: {:?}", r.status(ok));
    }
    assert!(matches!(r.status("E5"), Some(FeatureStatus::Failed(_))));
    for v in values {
        for rollback in [None, Some(8), Some(10)] {
            let d = doc(v, &tail);
            let r = warm.evaluate(&d, rollback, &());
            let mut fresh = Evaluator::new(Arc::new(MockKernel));
            let f = fresh.evaluate(&d, rollback, &());
            assert_eq!(summary(&warm, &r), summary(&fresh, &f), "{v:?} rollback {rollback:?}");
        }
    }
}

#[test]
fn building_ahead_in_parallel_gives_the_sequential_result() {
    let tail = format!(
        "plane F1 (base: E1/cap(end), offset: c)\n{}extrude E4 (profile: S4, distance: c, op: add)\n\
         move M1 (body: E2, translate: [0 mm, 30 mm, 0 mm], copy: true)\n\
         extrude E5 (profile: S2, distance: 0 mm, op: new)\ncombine C1 (body: E2, tools: [M1])\n",
        square("S4", 100.0)
    );
    let values =
        [["5 mm", "7 mm", "3 mm"], ["6 mm", "7 mm", "3 mm"], ["6 mm", "9 mm", "2 mm"], ["4 mm", "7 mm", "30 mm"]];
    let mut par = Evaluator::new(Arc::new(MockKernel));
    par.set_parallel(4);
    for v in values {
        for rollback in [None, Some(8)] {
            let d = doc(v, &tail);
            let r = par.evaluate(&d, rollback, &());
            let mut seq = Evaluator::new(Arc::new(MockKernel));
            let f = seq.evaluate(&d, rollback, &());
            assert_eq!(summary(&par, &r), summary(&seq, &f), "{v:?} rollback {rollback:?}");
        }
    }
    assert!(par.speculated() > 0, "independent extrudes were built ahead");
}
