//! Datum planes, body move/copy and combine against the mock kernel (which can transform
//! boxes and unite bodies that do not touch; cut and intersect need OCCT).

use super::*;
use gusset_kernel::mock::MockKernel;

const BASE: &str = "gusset 1
units mm

param lift = 12 mm
param tilt = 30 deg
param dx = 100 mm

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

/// A small plate far away from the base, as its own body.
const TOOL: &str = "
sketch Sketch2 (plane: XY, next_id: 9) {
  p1 = point(100, 0)
  p2 = point(110, 0)
  p3 = point(110, 10)
  p4 = point(100, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude2 (profile: Sketch2, distance: 5 mm, op: new)
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

fn bbox(ev: &Evaluator, r: &Evaluation, body: &str) -> gusset_kernel::BBox {
    ev.kernel().bbox(&r.body(body).unwrap().shape).unwrap()
}

fn face_names(r: &Evaluation, body: &str) -> Vec<String> {
    r.body(body).unwrap().names.faces.iter().map(ToString::to_string).collect()
}

fn frame(r: &Evaluation, plane: &str) -> Frame {
    r.planes.iter().find(|(n, _)| n == plane).map(|(_, f)| *f).unwrap_or_else(|| panic!("no plane {plane}"))
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn offset_planes_from_base_planes_faces_and_datums() {
    let (_, r) = run("plane Plane1 (base: XY, offset: lift)
plane Plane2 (base: XZ, offset: 5 mm)
plane Plane3 (base: YZ, offset: -3 mm)
plane Plane4 (base: Extrude1/cap(end), offset: 5 mm)
plane Plane5 (base: Plane1, offset: lift)
plane Plane6 (base: Extrude1/side(Sketch1/l6), offset: 1 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1, "datum planes create no body");
    assert_eq!(r.planes.len(), 6);
    assert!(close(frame(&r, "Plane1").origin, [0.0, 0.0, 12.0]));
    // The offset runs along the plane's normal: XZ's normal is -Y, YZ's is +X.
    assert!(close(frame(&r, "Plane2").origin, [0.0, -5.0, 0.0]));
    assert!(close(frame(&r, "Plane3").origin, [-3.0, 0.0, 0.0]));
    assert!(close(frame(&r, "Plane4").origin, [0.0, 0.0, 15.0]));
    assert!(close(frame(&r, "Plane4").normal(), [0.0, 0.0, 1.0]));
    assert!(close(frame(&r, "Plane5").origin, [0.0, 0.0, 24.0]));
    // The side face l6 sits at x = 40 and faces +X.
    assert!(close(frame(&r, "Plane6").origin, [41.0, 0.0, 0.0]));
    assert!(close(frame(&r, "Plane6").normal(), [1.0, 0.0, 0.0]));
}

#[test]
fn angled_planes_turn_about_axes_and_edges() {
    let (_, r) = run("plane Plane1 (base: XY, offset: lift)
plane Plane2 (base: Plane1, axis: X, angle: tilt)
plane Plane3 (base: Extrude1/cap(end), axis: edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5)), angle: 90 deg)
plane Plane4 (base: XY, axis: Z, angle: 90 deg)
plane Plane5 (base: XZ, axis: Y, angle: -90 deg)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // (0, 0, 12) turned 30 degrees about X; z becomes (0, -sin, cos).
    let (s, c) = (0.5, 0.75_f64.sqrt());
    assert!(close(frame(&r, "Plane2").origin, [0.0, -12.0 * s, 12.0 * c]), "{:?}", frame(&r, "Plane2"));
    assert!(close(frame(&r, "Plane2").normal(), [0.0, -s, c]));
    // The top face turned 90 degrees about its own edge (x axis through z = 10): a wall
    // through that edge facing -Y.
    assert!(close(frame(&r, "Plane3").origin, [0.0, 0.0, 10.0]));
    assert!(close(frame(&r, "Plane3").normal(), [0.0, -1.0, 0.0]));
    // Turning XY about its own normal changes the axes, not the plane.
    assert!(close(frame(&r, "Plane4").normal(), [0.0, 0.0, 1.0]));
    assert!(close(frame(&r, "Plane4").x, [0.0, 1.0, 0.0]));
    // XZ (normal -Y) turned about Y keeps its normal; its x axis goes from +X to +Z
    // (-90 degrees, right-hand rule about +Y).
    assert!(close(frame(&r, "Plane5").normal(), [0.0, -1.0, 0.0]));
    assert!(close(frame(&r, "Plane5").x, [0.0, 0.0, 1.0]), "{:?}", frame(&r, "Plane5"));
    for (_, f) in &r.planes {
        assert!(f.is_valid());
    }
}

#[test]
fn sketches_and_mirrors_use_datum_planes() {
    let (ev, r) = run("plane Plane1 (base: XY, offset: lift)
plane Plane2 (base: YZ, offset: 60 mm)
plane Plane3 (base: Plane1, axis: X, angle: tilt)
sketch Sketch2 (plane: Plane1, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}
extrude Extrude2 (profile: Sketch2, distance: 3 mm, op: new)
sketch Sketch3 (plane: Plane3, next_id: 2) {
  p1 = point(1, 1)
}
mirror Mirror1 (body: Extrude1, plane: Plane2)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(r.sketches["Sketch2"].frame.origin, [0.0, 0.0, 12.0]));
    let bb = bbox(&ev, &r, "Extrude2");
    assert!((bb.min[2] - 12.0).abs() < 1e-9 && (bb.max[2] - 15.0).abs() < 1e-9, "{bb:?}");
    assert!(close(r.sketches["Sketch3"].frame.normal(), frame(&r, "Plane3").normal()));
    // Mirroring 0..40 across x = 60 gives 80..120.
    let bb = bbox(&ev, &r, "Mirror1");
    assert!((bb.min[0] - 80.0).abs() < 1e-9 && (bb.max[0] - 120.0).abs() < 1e-9, "{bb:?}");
}

#[test]
fn datum_planes_follow_parameter_edits_incrementally() {
    let src = format!(
        "{BASE}\nplane Plane1 (base: Extrude1/cap(end), offset: lift)
sketch Sketch2 (plane: Plane1, next_id: 2) {{
  p1 = point(1, 1)
}}\n"
    );
    let doc = Document::from_text(&src).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r1 = ev.evaluate(&doc, None, &());
    assert!(r1.errors().is_empty(), "{:?}", r1.errors());
    assert!(close(frame(&r1, "Plane1").origin, [0.0, 0.0, 22.0]));
    let mut d2 = doc.clone();
    d2.params[0].expr = gusset_units::expr::parse("20 mm").unwrap();
    let r2 = ev.evaluate(&d2, None, &());
    assert!(r2.errors().is_empty(), "{:?}", r2.errors());
    assert!(close(frame(&r2, "Plane1").origin, [0.0, 0.0, 30.0]));
    assert!(close(r2.sketches["Sketch2"].frame.origin, [0.0, 0.0, 30.0]));
    assert_eq!(r2.recomputed, 2, "only the plane and the sketch on it are recomputed");
    // A change to the extrusion height moves the face-based plane too.
    let mut d3 = d2.clone();
    if let FeatureKind::Extrude(e) = &mut d3.features[1].kind {
        e.distance = gusset_units::expr::parse("15 mm").unwrap();
    }
    let r3 = ev.evaluate(&d3, None, &());
    assert!(close(frame(&r3, "Plane1").origin, [0.0, 0.0, 35.0]));
}

#[test]
fn datum_plane_errors_are_actionable() {
    // A face that does not exist.
    let (_, r) = run("plane Plane1 (base: Extrude1/side(Sketch1/l99), offset: 1 mm)
sketch Sketch2 (plane: Plane1, next_id: 1) {
}");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::MissingReference);
    assert_eq!(e.reference.as_deref(), Some("Extrude1/side(Sketch1/l99)"));
    assert!(matches!(r.status("Sketch2"), Some(FeatureStatus::Blocked { by }) if by == "Plane1"), "{:?}", r.statuses);
    // An axis edge that does not exist.
    let (_, r) =
        run("plane Plane1 (base: XY, axis: edge(Extrude1/cap(end), Extrude1/side(Sketch1/l99)), angle: 10 deg)");
    assert_eq!(r.errors()[0].code, ErrorCode::MissingReference);
    assert!(r.errors()[0].hint.is_some());
    // Wrong dimensions.
    let (_, r) = run("plane Plane1 (base: XY, offset: 5 deg)");
    assert_eq!(r.errors()[0].code, ErrorCode::Expression);
    let (_, r) = run("plane Plane1 (base: XY, axis: Z, angle: 5 mm)");
    assert_eq!(r.errors()[0].code, ErrorCode::Expression);
    // A suppressed plane is skipped, and what needs it is blocked or missing.
    let (_, r) = run("plane Plane1 (base: XY, offset: 1 mm, suppressed: true)");
    assert_eq!(r.status("Plane1"), Some(&FeatureStatus::Suppressed));
    assert!(r.planes.is_empty());
}

#[test]
fn move_translates_and_rotates_keeping_names() {
    let (ev, r) = run("move Move1 (translate: [10 mm, -5 mm, 3 mm])");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1);
    let bb = bbox(&ev, &r, "Extrude1");
    assert!(close(bb.min, [10.0, -5.0, 3.0]) && close(bb.max, [50.0, 15.0, 13.0]), "{bb:?}");
    // A rigid move keeps every face name and the volume.
    let (_, base) = run("");
    assert_eq!(face_names(&r, "Extrude1"), face_names(&base, "Extrude1"));
    assert!((volume(&ev, &r, "Extrude1") - 8000.0).abs() < 1e-9);
    // Quarter turn about Z through the origin, then a shift: x 0..40 -> y 0..40.
    let (ev, r) = run("move Move1 (axis: Z, angle: 90 deg, translate: [100 mm, 0 mm, 0 mm])");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let bb = bbox(&ev, &r, "Extrude1");
    assert!(close(bb.min, [80.0, 0.0, 0.0]) && close(bb.max, [100.0, 40.0, 10.0]), "{bb:?}");
    // The axis through another point and given as a vector.
    let (ev, r) = run("move Move1 (axis: [0, 0, 2], angle: 180 deg, through: [20 mm, 10 mm, 0 mm])");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let bb = bbox(&ev, &r, "Extrude1");
    assert!(
        close(bb.min, [0.0, 0.0, 0.0]) && close(bb.max, [40.0, 20.0, 10.0]),
        "turning the plate about its centre: {bb:?}"
    );
    // Faces of a moved body are still addressable and sketches on them follow the move.
    let (_, r) = run("move Move1 (translate: [0 mm, 0 mm, lift])
sketch Sketch2 (plane: Extrude1/cap(end), next_id: 2) {
  p1 = point(1, 1)
}");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(r.sketches["Sketch2"].frame.origin, [0.0, 0.0, 22.0]));
}

#[test]
fn move_copy_is_a_new_named_body() {
    let (ev, r) = run("move Move1 (body: Extrude1, translate: [dx, 0 mm, 0 mm], copy: true)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 2);
    let moved = r.body("Move1").unwrap();
    assert_eq!(moved.feature, "Move1");
    assert!((volume(&ev, &r, "Move1") - 8000.0).abs() < 1e-9);
    let bb = bbox(&ev, &r, "Move1");
    assert!(close(bb.min, [100.0, 0.0, 0.0]), "{bb:?}");
    let names = face_names(&r, "Move1");
    assert!(names.contains(&"Move1/copy(Extrude1/cap(end))".to_string()), "{names:?}");
    assert!(names.contains(&"Move1/copy(Extrude1/side(Sketch1/l6))".to_string()));
    // The original stays put with its own names.
    let bb = bbox(&ev, &r, "Extrude1");
    assert!(close(bb.min, [0.0; 3]));
    assert!(face_names(&r, "Extrude1").contains(&"Extrude1/cap(end)".to_string()));
    // The copy is a body like any other: a sketch on its top, then a rotated copy of it.
    let (_, r) = run("move Move1 (translate: [dx, 0 mm, 0 mm], copy: true)
sketch Sketch2 (plane: Move1/copy(Extrude1/cap(end)), next_id: 2) {
  p1 = point(1, 1)
}
move Move2 (body: Move1, axis: Z, angle: 90 deg, copy: true)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 3);
    assert!(close(r.sketches["Sketch2"].frame.origin, [0.0, 0.0, 10.0]));
    assert!(face_names(&r, "Move2").contains(&"Move2/copy(Move1/copy(Extrude1/cap(end)))".to_string()));
}

#[test]
fn move_parameters_rebuild_incrementally() {
    let src = format!("{BASE}\nmove Move1 (body: Extrude1, translate: [dx, 0 mm, 0 mm], copy: true)\n");
    let doc = Document::from_text(&src).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r1 = ev.evaluate(&doc, None, &());
    assert!(r1.errors().is_empty(), "{:?}", r1.errors());
    let mut d2 = doc.clone();
    d2.params[2].expr = gusset_units::expr::parse("250 mm").unwrap();
    let r2 = ev.evaluate(&d2, None, &());
    assert_eq!(r2.recomputed, 1);
    assert!(close(bbox(&ev, &r2, "Move1").min, [250.0, 0.0, 0.0]));
}

#[test]
fn move_errors_are_actionable() {
    let (_, r) = run("move Move1 (axis: [0, 0, 0], angle: 10 deg)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::Expression);
    assert!(e.message.contains("no length"), "{e}");
    let (_, r) = run("move Move1 (translate: [1 deg, 0 mm, 0 mm])");
    assert_eq!(r.errors()[0].code, ErrorCode::Expression);
    let (_, r) = run("move Move1 (axis: Z, angle: 1 mm)");
    assert_eq!(r.errors()[0].code, ErrorCode::Expression);
    // A target whose feature failed.
    let text = BASE.replace("distance: 10 mm", "distance: 0 mm");
    let doc = Document::from_text(&format!("{text}\nmove Move1 (translate: [1 mm, 0 mm, 0 mm])\n")).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    assert!(matches!(r.status("Move1"), Some(FeatureStatus::Failed(_) | FeatureStatus::Blocked { .. })));
}

#[test]
fn combine_unites_tool_bodies_and_consumes_them() {
    let (ev, r) = run(&format!("{TOOL}\ncombine Combine1 (body: Extrude1, tools: [Extrude2])"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1, "the tool is consumed");
    let b = r.body("Extrude1").unwrap();
    assert!((volume(&ev, &r, "Extrude1") - (8000.0 + 500.0)).abs() < 1e-9);
    assert_eq!(b.names.faces.len(), 12);
    // Faces of both inputs keep their names.
    let names = face_names(&r, "Extrude1");
    assert!(names.contains(&"Extrude1/cap(end)".to_string()));
    assert!(names.contains(&"Extrude2/cap(end)".to_string()), "{names:?}");
    assert!(r.body("Extrude2").is_none());
    // Without a target the latest body that is not a tool is used.
    let (_, r2) = run(&format!("{TOOL}\ncombine Combine1 (tools: [Extrude2])"));
    assert_eq!(face_names(&r2, "Extrude1"), names);
}

#[test]
fn a_patterned_feature_is_combined_into_the_base() {
    // The flow for patterning a single feature: make it a body of its own, pattern that
    // body, combine the result into the base.
    let (ev, r) = run(&format!(
        "{TOOL}\npattern_linear Pattern1 (body: Extrude2, direction: Y, count: 3, spacing: 30 mm)
combine Combine1 (body: Extrude1, tools: [Extrude2])"
    ));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1);
    assert!((volume(&ev, &r, "Extrude1") - (8000.0 + 3.0 * 500.0)).abs() < 1e-6);
    let names = face_names(&r, "Extrude1");
    assert!(names.contains(&"Pattern1/instance(2, Extrude2/cap(end))".to_string()), "{names:?}");
    // A moved copy as the tool.
    let (ev, r) = run("move Move1 (translate: [100 mm, 0 mm, 0 mm], copy: true)
combine Combine1 (body: Extrude1, tools: [Move1])");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!((volume(&ev, &r, "Extrude1") - 16000.0).abs() < 1e-6);
    assert!(face_names(&r, "Extrude1").contains(&"Move1/copy(Extrude1/cap(end))".to_string()));
}

#[test]
fn several_tools_and_edits_to_their_parameters() {
    let src = format!(
        "{BASE}{TOOL}
move Move1 (body: Extrude2, translate: [0 mm, dx, 0 mm], copy: true)
combine Combine1 (body: Extrude1, tools: [Extrude2, Move1])
"
    );
    let doc = Document::from_text(&src).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1);
    assert!((volume(&ev, &r, "Extrude1") - (8000.0 + 2.0 * 500.0)).abs() < 1e-9);
    // Names stay the same while a parameter moves the copy.
    let before = face_names(&r, "Extrude1");
    let mut d2 = doc.clone();
    d2.params[2].expr = gusset_units::expr::parse("300 mm").unwrap();
    let r2 = ev.evaluate(&d2, None, &());
    assert!(r2.errors().is_empty(), "{:?}", r2.errors());
    assert_eq!(face_names(&r2, "Extrude1"), before);
    let b = r2.body("Extrude1").unwrap();
    let fi = b.names.faces.iter().position(|f| f.to_string() == "Move1/copy(Extrude2/cap(end))").unwrap();
    assert!((b.topology.faces[fi].centroid[1] - 305.0).abs() < 1e-9);
}

#[test]
fn combine_errors_are_actionable() {
    // The mock kernel cannot subtract: a clean kernel failure that names the tool.
    let (_, r) = run(&format!("{TOOL}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: cut)"));
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure);
    assert_eq!(e.feature, "Combine1");
    assert!(e.message.contains("cut") && e.message.contains("Extrude2"), "{e}");
    assert!(e.hint.is_some());
    // A failed tool blocks the combine.
    let text = TOOL.replace("distance: 5 mm", "distance: 0 mm");
    let (_, r) = run(&format!("{text}\ncombine Combine1 (body: Extrude1, tools: [Extrude2])"));
    assert!(matches!(r.status("Extrude2"), Some(FeatureStatus::Failed(_))));
    assert!(
        matches!(r.status("Combine1"), Some(FeatureStatus::Blocked { by }) if by == "Extrude2"),
        "{:?}",
        r.statuses
    );
    // The base is still there, untouched.
    assert!(r.body("Extrude1").is_some());
    // Overlapping bodies cannot be united by the mock; the base body is left intact.
    let (ev, r) = run("move Move1 (translate: [10 mm, 0 mm, 0 mm], copy: true)
combine Combine1 (body: Extrude1, tools: [Move1])");
    assert_eq!(r.errors()[0].code, ErrorCode::KernelFailure);
    assert_eq!(r.bodies.len(), 2, "a failed combine consumes nothing");
    assert!((volume(&ev, &r, "Extrude1") - 8000.0).abs() < 1e-9);
}

#[test]
fn combine_survives_rollback_and_suppression() {
    let src = format!("{BASE}{TOOL}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], suppressed: true)\n");
    let doc = Document::from_text(&src).unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    assert_eq!(r.status("Combine1"), Some(&FeatureStatus::Suppressed));
    assert_eq!(r.bodies.len(), 2, "a suppressed combine leaves the tool alone");
    let doc2 = Document::from_text(&src.replace(", suppressed: true", "")).unwrap();
    let r = ev.evaluate(&doc2, Some(doc2.features.len() - 1), &());
    assert_eq!(r.bodies.len(), 2, "rolled back before the combine");
}
