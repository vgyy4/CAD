//! Shell, mirror and pattern features against the mock kernel (which can transform boxes
//! and unite bodies that do not touch; shell needs OCCT).

use super::*;
use gusset_kernel::mock::MockKernel;

const BASE: &str = "gusset 1
units mm

param n = 3
param pitch = 60 mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)
";

fn run(extra: &str) -> (Evaluator, Evaluation) {
    let doc = Document::from_text(&format!("{BASE}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

fn face_names(r: &Evaluation, body: &str) -> Vec<String> {
    r.body(body).unwrap().names.faces.iter().map(ToString::to_string).collect()
}

#[test]
fn mirror_makes_a_named_copy() {
    let (ev, r) = run("mirror Mirror1 (body: Extrude1, plane: YZ)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 2);
    assert!((volume(&ev, &r, "Mirror1") - 8000.0).abs() < 1e-9);
    let bb = ev.kernel().bbox(&r.body("Mirror1").unwrap().shape).unwrap();
    assert!((bb.min[0] + 40.0).abs() < 1e-9 && bb.max[0].abs() < 1e-9);
    let names = face_names(&r, "Mirror1");
    assert_eq!(names.len(), 6);
    assert!(names.contains(&"Mirror1/mirror(Extrude1/side(Sketch1/l6))".to_string()), "{names:?}");
    assert!(names.contains(&"Mirror1/mirror(Extrude1/cap(end))".to_string()));
    // The mirrored l6 side (originally at x = 40) now sits at x = -40 and faces -X.
    let b = r.body("Mirror1").unwrap();
    let fi = b.names.faces.iter().position(|f| f.to_string().ends_with("mirror(Extrude1/side(Sketch1/l6))")).unwrap();
    assert!((b.topology.faces[fi].centroid[0] + 40.0).abs() < 1e-9);
    assert!((b.topology.faces[fi].normal[0] + 1.0).abs() < 1e-9);
    // The source is untouched.
    assert!((volume(&ev, &r, "Extrude1") - 8000.0).abs() < 1e-9);
}

#[test]
fn mirror_across_a_face_reference() {
    // Mirror plane = the x = 40 side face of the plate; the copy lands at x 40..80.
    let (ev, r) = run("mirror Mirror1 (plane: Extrude1/side(Sketch1/l6))");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let bb = ev.kernel().bbox(&r.body("Mirror1").unwrap().shape).unwrap();
    assert!((bb.min[0] - 40.0).abs() < 1e-9 && (bb.max[0] - 80.0).abs() < 1e-9, "{bb:?}");
    // Faces can be referenced after the mirror: a sketch on the mirrored top.
    let sk = "sketch Sketch2 (plane: Mirror1/mirror(Extrude1/cap(end)), next_id: 5) {
  p1 = point(0, 0)
  p2 = point(5, 0)
  p3 = point(5, 5)
  l4 = line(p1, p2)
}";
    let (_, r) = run(&format!("mirror Mirror1 (plane: Extrude1/side(Sketch1/l6))\n{sk}"));
    assert_eq!(r.status("Sketch2"), Some(&FeatureStatus::Ok), "{:?}", r.statuses);
    // A missing or non-body plane is reported.
    let (_, r) = run("mirror Mirror1 (plane: Extrude1/side(Sketch1/l99))");
    assert_eq!(r.errors()[0].code, ErrorCode::MissingReference);
}

#[test]
fn merging_a_touching_mirror_needs_the_real_kernel() {
    let (_, r) = run("mirror Mirror1 (body: Extrude1, plane: YZ, merge: true)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure);
    assert!(e.message.contains("union"), "{e}");
    assert_eq!(e.feature, "Mirror1");
}

#[test]
fn linear_pattern_unites_instances_and_names_their_faces() {
    let (ev, r) = run("pattern_linear Pattern1 (body: Extrude1, direction: X, count: n, spacing: pitch)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1);
    assert!((volume(&ev, &r, "Extrude1") - 24000.0).abs() < 1e-6);
    let b = r.body("Extrude1").unwrap();
    assert_eq!(b.names.faces.len(), 18);
    let names = face_names(&r, "Extrude1");
    // The source faces keep their names; instances 1 and 2 are named after them.
    assert!(names.contains(&"Extrude1/cap(end)".to_string()));
    assert!(names.contains(&"Pattern1/instance(1, Extrude1/cap(end))".to_string()), "{names:?}");
    assert!(names.contains(&"Pattern1/instance(2, Extrude1/side(Sketch1/l5))".to_string()));
    let mut uniq = names.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), 18, "face names are unique");
    // Instance 2 is 120 mm away.
    let fi = b.names.faces.iter().position(|f| f.to_string() == "Pattern1/instance(2, Extrude1/side(Sketch1/l6))");
    assert!((b.topology.faces[fi.unwrap()].centroid[0] - 160.0).abs() < 1e-9);
    // Edges are named from the faces, so a later feature can refer to them.
    assert_eq!(b.names.edges.len(), 36);
}

#[test]
fn pattern_counts_are_parameters_and_rebuild_incrementally() {
    let text = format!("{BASE}\npattern_linear Pattern1 (direction: Y, count: n, spacing: 30 mm)\n");
    let doc = Document::from_text(&text).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!((volume(&ev, &r, "Extrude1") - 24000.0).abs() < 1e-6);
    let mut doc2 = doc.clone();
    doc2.params[0].expr = gusset_units::expr::parse("5").unwrap();
    let r2 = ev.evaluate(&doc2, None, &());
    assert!(r2.errors().is_empty(), "{:?}", r2.errors());
    assert_eq!(r2.recomputed, 1, "only the pattern is recomputed");
    assert!((volume(&ev, &r2, "Extrude1") - 40000.0).abs() < 1e-6);
    assert_eq!(r2.body("Extrude1").unwrap().names.faces.len(), 30);
}

#[test]
fn circular_pattern_spans_the_angle() {
    // Axis Z through (100, 0): instances at 0, 90 and 180 degrees; none touch.
    let (ev, r) =
        run("pattern_circular Ring (body: Extrude1, axis: Z, through: [100 mm, 0 mm, 0 mm], count: 3, angle: 180 deg)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!((volume(&ev, &r, "Extrude1") - 24000.0).abs() < 1e-6);
    let bb = ev.kernel().bbox(&r.body("Extrude1").unwrap().shape).unwrap();
    assert!((bb.min[1] + 100.0).abs() < 1e-6 && (bb.max[0] - 200.0).abs() < 1e-6, "{bb:?}");
    // A full turn divides the circle evenly: 4 instances 90 degrees apart (quarter turns
    // around (100, 100) never overlap the 40x20 plate).
    let (ev, r) = run("pattern_circular Ring (axis: Z, through: [100 mm, 100 mm, 0 mm], count: 4, angle: 360 deg)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!((volume(&ev, &r, "Extrude1") - 32000.0).abs() < 1e-6);
    let names = face_names(&r, "Extrude1");
    assert!(names.contains(&"Ring/instance(3, Extrude1/cap(start))".to_string()), "{names:?}");
}

#[test]
fn pattern_errors_are_actionable() {
    let cases = [
        ("pattern_linear P (direction: X, count: 2.5, spacing: 60 mm)", "whole number"),
        ("pattern_linear P (direction: X, count: 1, spacing: 60 mm)", "between 2 and"),
        ("pattern_linear P (direction: X, count: 9999, spacing: 60 mm)", "between 2 and"),
        ("pattern_linear P (direction: X, count: 3, spacing: 0 mm)", "spacing is zero"),
        ("pattern_linear P (direction: [0, 0, 0], count: 3, spacing: 5 mm)", "no length"),
        ("pattern_linear P (direction: X, count: 3, spacing: 5 deg)", "expected"),
        ("pattern_circular P (axis: Z, count: 3, angle: 0 deg)", "angle is zero"),
        ("pattern_linear P (body: Nope, direction: X, count: 3, spacing: 60 mm)", "does not exist"),
    ];
    for (src, want) in cases {
        // 'Nope' is rejected by validation, not the evaluator.
        let text = format!("{BASE}\n{src}\n");
        let Ok(doc) = Document::from_text(&text) else {
            assert!(want == "does not exist", "{src}");
            continue;
        };
        let mut ev = Evaluator::new(Arc::new(MockKernel));
        let r = ev.evaluate(&doc, None, &());
        let e = r.errors().into_iter().next().unwrap_or_else(|| panic!("{src} did not fail"));
        assert!(e.message.contains(want), "{src}: {e}");
        assert_eq!(e.feature, "P");
    }
    // Overlapping instances cannot be united by the mock; the error says what to check.
    let (_, r) = run("pattern_linear P (direction: X, count: 3, spacing: 10 mm)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure);
    assert!(e.message.contains("instance 1"), "{e}");
    assert!(e.hint.is_some());
}

#[test]
fn shell_reports_missing_faces_and_kernel_limits() {
    let (_, r) = run("shell Shell1 (faces: [Extrude1/cap(end)], thickness: 2 mm)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure);
    assert!(e.message.contains("shell"), "{e}");
    let (_, r) = run("shell Shell1 (faces: [Extrude1/side(Sketch1/l99)], thickness: 2 mm)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::MissingReference);
    assert_eq!(e.reference.as_deref(), Some("Extrude1/side(Sketch1/l99)"));
    assert!(e.suggestion.as_deref().is_some_and(|s| s.starts_with("Extrude1/")), "{e}");
    let (_, r) = run("shell Shell1 (faces: [Extrude1/cap(end)], thickness: 0 mm)");
    assert_eq!(r.errors()[0].code, ErrorCode::Expression);
    let all = (0..4).map(|i| format!("Extrude1/side(Sketch1/l{})", 5 + i)).collect::<Vec<_>>().join(", ");
    let (_, r) =
        run(&format!("shell Shell1 (faces: [Extrude1/cap(start), Extrude1/cap(end), {all}], thickness: 1 mm)"));
    assert_eq!(r.errors()[0].code, ErrorCode::InvalidReference);
}

#[test]
fn failures_block_dependents() {
    let text = BASE.replace("distance: 10 mm", "distance: 0 mm");
    let text = format!("{text}\npattern_linear P (direction: X, count: 3, spacing: 60 mm)\n");
    let doc = Document::from_text(&text).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    assert!(matches!(r.status("Extrude1"), Some(FeatureStatus::Failed(_))));
    assert!(matches!(r.status("P"), Some(FeatureStatus::Failed(_) | FeatureStatus::Blocked { .. })));
}
