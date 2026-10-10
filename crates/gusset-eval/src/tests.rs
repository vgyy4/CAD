use super::*;
use gusset_kernel::mock::MockKernel;

fn eval(text: &str) -> (Evaluator, Evaluation) {
    let doc = Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

const PLATE: &str = "gusset 1
units mm

param width = 40 mm
param depth = 10 mm

sketch Sketch1 (plane: XY, next_id: 13) {
  p1 = point(0, 0)
  p2 = point(30, 0)
  p3 = point(30, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
  k9 = fixed(p1)
  k10 = distance(l5, l5, width)
  k11 = horizontal(l5)
  k12 = vertical(l6)
}

extrude Extrude1 (profile: Sketch1, distance: depth, op: new)
";

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

#[test]
fn plate_evaluates_with_parameters() {
    let (ev, r) = eval(PLATE);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let sk = &r.sketches["Sketch1"];
    // Width is driven to 40 mm by the parameter; the sketch has free DOF (height, p4...).
    assert!((sk.points["p2"][0] - 40.0).abs() < 1e-6);
    assert_eq!(sk.status, SketchStatus::UnderConstrained);
    let b = r.body("Extrude1").unwrap();
    assert_eq!(b.names.faces.len(), 6);
    assert!(b.names.faces.iter().any(|f| f.to_string() == "Extrude1/side(Sketch1/l6)"));
    // The p3/p4 points moved with the horizontal/vertical constraints; volume = 40·20·10.
    assert!((volume(&ev, &r, "Extrude1") - 8000.0).abs() < 1e-6);
}

#[test]
fn incremental_rebuild_reuses_cache() {
    let doc = Document::from_text(PLATE).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r1 = ev.evaluate(&doc, None, &());
    assert_eq!(r1.recomputed, 2);
    let r2 = ev.evaluate(&doc, None, &());
    assert_eq!(r2.recomputed, 0);
    // Changing `depth` only affects Extrude1.
    let mut d2 = doc.clone();
    d2.params[1].expr = gusset_units::expr::parse("12 mm").unwrap();
    let r3 = ev.evaluate(&d2, None, &());
    assert_eq!(r3.recomputed, 1);
    assert!((volume(&ev, &r3, "Extrude1") - 9600.0).abs() < 1e-6);
    // Changing `width` affects both.
    let mut d3 = doc;
    d3.params[0].expr = gusset_units::expr::parse("1 in").unwrap();
    let r4 = ev.evaluate(&d3, None, &());
    assert_eq!(r4.recomputed, 2);
    assert!((volume(&ev, &r4, "Extrude1") - 25.4 * 20.0 * 10.0).abs() < 1e-6);
}

#[test]
fn rollback_marker_stops_evaluation() {
    let doc = Document::from_text(PLATE).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, Some(1), &());
    assert_eq!(r.status("Extrude1"), Some(&FeatureStatus::RolledBack));
    assert!(r.bodies.is_empty());
}

#[test]
fn conflict_fails_sketch_and_blocks_downstream() {
    let text = PLATE
        .replace("  k12 = vertical(l6)\n", "  k12 = vertical(l6)\n  k13 = distance(p1, p2, 5 mm)\n")
        .replace("next_id: 13", "next_id: 14");
    let (_, r) = eval(&text);
    match r.status("Sketch1") {
        Some(FeatureStatus::Failed(e)) => {
            assert_eq!(e.code, ErrorCode::SketchConflict);
            assert!(e.message.contains("k"), "{e}");
            assert!(e.hint.is_some());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.status("Extrude1"), Some(&FeatureStatus::Blocked { by: "Sketch1".into() }));
}

#[test]
fn expression_errors_are_actionable() {
    let text = PLATE.replace("param depth = 10 mm", "param depth = 10");
    // Bare numbers in a length context use document units: fine.
    let (_, r) = eval(&text);
    assert!(r.errors().is_empty());
    let text = PLATE.replace("param depth = 10 mm", "param depth = 10 deg");
    let (_, r) = eval(&text);
    let e = r.errors()[0].clone();
    assert_eq!((e.feature.as_str(), e.code), ("Extrude1", ErrorCode::Expression));
    assert!(e.message.contains("expected a length"), "{e}");
}

#[test]
fn open_profile_is_reported() {
    let text = PLATE.replace("  l8 = line(p4, p1)\n", "");
    let (_, r) = eval(&text);
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::OpenProfile);
    assert!(e.message.contains("not closed"), "{e}");
}

#[test]
fn sketch_on_face_and_second_body() {
    let text = format!(
        "{PLATE}
sketch Sketch2 (plane: Extrude1/cap(end), next_id: 9) {{
  p1 = point(5, 5)
  p2 = point(15, 5)
  p3 = point(15, 15)
  p4 = point(5, 15)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

extrude Boss (profile: Sketch2, distance: 5 mm, op: new)
"
    );
    let (ev, r) = eval(&text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let bb = ev.kernel().bbox(&r.body("Boss").unwrap().shape).unwrap();
    assert!((bb.min[2] - 10.0).abs() < 1e-9 && (bb.max[2] - 15.0).abs() < 1e-9);
}

#[test]
fn missing_edge_reference_suggests_a_fix() {
    let text = format!(
        "{PLATE}
fillet Fillet1 (edges: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l99))], radius: 1 mm)
"
    );
    // l99 doesn't exist in Sketch1: document validation doesn't check generated names,
    // so this is caught at evaluation with a suggestion.
    let (_, r) = eval(&text);
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::MissingReference);
    assert!(e.message.contains("no longer exists"));
    assert!(e.suggestion.is_some());
}

#[test]
fn kernel_limits_surface_as_structured_errors() {
    let text = format!(
        "{PLATE}
fillet Fillet1 (edges: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))], radius: 1 mm)
"
    );
    let (_, r) = eval(&text);
    let e = r.errors()[0].clone();
    assert_eq!(e.feature, "Fillet1");
    assert!(matches!(e.code, ErrorCode::KernelFailure | ErrorCode::FilletTooLarge));
}

#[test]
fn suppressed_features_are_skipped() {
    let text = PLATE.replace("op: new)", "op: new, suppressed: true)");
    let (_, r) = eval(&text);
    assert_eq!(r.status("Extrude1"), Some(&FeatureStatus::Suppressed));
    assert!(r.bodies.is_empty());
}

#[test]
fn imports_need_their_blob() {
    let text = "gusset 1\nunits mm\n\nimport Import1 (blob: \"0000000000000000000000000000000000000000000000000000000000000000\", format: brep, file: \"x.brep\")\n";
    let (_, r) = eval(text);
    assert_eq!(r.errors()[0].code, ErrorCode::MissingBlob);
}
