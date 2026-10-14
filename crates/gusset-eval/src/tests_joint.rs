//! Joints (ADR 0026) on the mock kernel: planar-face frames, flip, offset, motion, limits,
//! carried bodies.

use super::*;
use gusset_kernel::mock::MockKernel;

fn rect(name: &str, w: f64, h: f64) -> String {
    format!(
        "sketch {name} (plane: XY, next_id: 9) {{
  p1 = point(0, 0)
  p2 = point({w}, 0)
  p3 = point({w}, {h})
  p4 = point(0, {h})
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}
"
    )
}

fn eval(joint: &str) -> (Evaluator, Evaluation) {
    let text = format!(
        "gusset 1\nunits mm\nparam open = 90 deg\n{}extrude Base (profile: S1, distance: 5 mm, op: new)\n{}\
         extrude Lid (profile: S2, distance: 2 mm, op: new)\nextrude Tag (profile: S2, distance: 1 mm, op: new)\n{joint}\n",
        rect("S1", 10.0, 10.0),
        rect("S2", 10.0, 4.0),
    );
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn bbox(ev: &Evaluator, r: &Evaluation, body: &str) -> ([f64; 3], [f64; 3]) {
    let b = ev.kernel().bbox(&r.body(body).unwrap_or_else(|| panic!("no body {body}")).shape).unwrap();
    (b.min, b.max)
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn a_rigid_joint_puts_the_lid_on_the_base() {
    let (ev, r) = eval("joint J1 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The lid's bottom face center (5, 2, 0) lands on the base's top center (5, 5, 5).
    let (lo, hi) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 3.0, 5.0]) && close(hi, [10.0, 7.0, 7.0]), "{lo:?} {hi:?}");
    // Names survive the placement.
    assert!(r.body("Lid").unwrap().names.faces.iter().any(|f| f.to_string() == "Lid/cap(end)"));
}

#[test]
fn a_revolute_joint_turns_and_carries_bodies() {
    let (ev, r) = eval(
        "joint J1 (type: revolute, body: Lid, with: [Tag], at: Lid/cap(start), to: Base/cap(end), flip: true, \
         angle: open, offset: 1 mm)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // A quarter turn about the base's vertical center line, one millimetre above it.
    let (lo, hi) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [3.0, 0.0, 6.0]) && close(hi, [7.0, 10.0, 8.0]), "{lo:?} {hi:?}");
    // The tag moved with the lid (same transform).
    let (lo, hi) = bbox(&ev, &r, "Tag");
    assert!(close(lo, [3.0, 0.0, 6.0]) && close(hi, [7.0, 10.0, 7.0]), "{lo:?} {hi:?}");
}

#[test]
fn a_slider_moves_along_the_frame_normal_and_respects_limits() {
    let (ev, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, distance: -3 mm, \
         distance_limits: [-4 mm, 0 mm])",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The flipped fixed frame's Z points down: -3 mm moves the lid up.
    let (lo, _) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 3.0, 8.0]), "{lo:?}");
    let (_, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), distance: 9 mm, \
         distance_limits: [-4 mm, 0 mm])",
    );
    let e = r.errors();
    assert!(e.iter().any(|e| e.feature == "J1" && e.message.contains("outside the joint's limits")), "{e:?}");
}

#[test]
fn a_relation_is_an_expression_of_another_joint_value() {
    // A rack: 2 mm of travel per quarter turn of `open` (90 deg).
    let (ev, r) = eval(
        "joint J1 (type: slider, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true, \
         distance: open / 360 deg * 8 mm)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let (lo, _) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [0.0, 3.0, 3.0]), "{lo:?}");
}

#[test]
fn a_joint_to_the_origin_and_a_bad_frame() {
    let (ev, r) = eval("joint J1 (type: rigid, body: Lid, at: Lid/cap(end), to: origin)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The lid's top face center goes to the world origin.
    let (lo, hi) = bbox(&ev, &r, "Lid");
    assert!(close(lo, [-5.0, -2.0, -2.0]) && close(hi, [5.0, 2.0, 0.0]), "{lo:?} {hi:?}");
    // An unknown body is caught when the document is read.
    let bad = format!(
        "gusset 1\n{}extrude Base (profile: S1, distance: 5 mm, op: new)\njoint J1 (type: rigid, body: Nope, at: origin, to: origin)\n",
        rect("S1", 1.0, 1.0)
    );
    assert!(Document::from_text(&bad).unwrap_err().to_string().contains("Nope"));
}
