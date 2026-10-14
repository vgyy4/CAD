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

/// A four-bar linkage in the XY plane: links are boxes along +X, pivots their vertical
/// corner edges at (0, 0) and (length, 0). Ground 40, crank 10, coupler 35, rocker 25.
fn four_bar(crank: &str, closing: &str) -> (Evaluator, Evaluation) {
    let pivot = |b: &str, s: &str, end: bool| {
        if end {
            format!("edge({b}/side({s}/l5), {b}/side({s}/l6))")
        } else {
            format!("edge({b}/side({s}/l8), {b}/side({s}/l5))")
        }
    };
    let text = format!(
        "gusset 1\nunits mm\nparam crank = {crank}\n{}{}{}{}\
         extrude Ground (profile: G, distance: 1 mm, op: new)\nextrude Crank (profile: C, distance: 1 mm, op: new)\n\
         extrude Coupler (profile: K, distance: 1 mm, op: new)\nextrude Rocker (profile: R, distance: 1 mm, op: new)\n\
         joint J1 (type: revolute, body: Crank, at: {}, to: {}, angle: crank)\n\
         joint J2 (type: revolute, body: Coupler, at: {}, to: {}, free: [angle])\n\
         joint J3 (type: revolute, body: Rocker, at: {}, to: {}, angle: 90 deg, free: [angle])\n\
         joint J4 (type: revolute, body: Coupler, at: {}, to: {}{closing})\n",
        rect("G", 40.0, 2.0),
        rect("C", 10.0, 2.0),
        rect("K", 35.0, 2.0),
        rect("R", 25.0, 2.0),
        pivot("Crank", "C", false),
        pivot("Ground", "G", false),
        pivot("Coupler", "K", false),
        pivot("Crank", "C", true),
        pivot("Rocker", "R", false),
        pivot("Ground", "G", true),
        pivot("Coupler", "K", true),
        pivot("Rocker", "R", true),
    );
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

/// The rocker's tip for rocker angle `a` (either rotation sense), and the crank's tip.
fn closes(crank_deg: f64, r: &Evaluation) -> bool {
    let a = r.joint_values.get("J3").map_or(f64::NAN, |v| v.angle);
    let c = crank_deg.to_radians();
    let tip = (10.0 * libm::cos(c), 10.0 * libm::sin(c));
    [a, -a].iter().any(|a| {
        let d = (40.0 + 25.0 * libm::cos(*a), 25.0 * libm::sin(*a));
        (libm::hypot(d.0 - tip.0, d.1 - tip.1) - 35.0).abs() < 1e-6
    })
}

#[test]
fn a_closing_joint_solves_the_free_values_of_a_four_bar() {
    let (_, r) = four_bar("60 deg", ", free: [angle]");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(closes(60.0, &r), "{:?}", r.joint_values);
    // A driven value stays as written.
    assert!((r.joint_values["J1"].angle - 60f64.to_radians()).abs() < 1e-12);
    // Moving the crank re-solves the loop.
    let (_, r) = four_bar("30 deg", ", free: [angle]");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(closes(30.0, &r), "{:?}", r.joint_values);
}

#[test]
fn a_loop_that_cannot_close_says_why() {
    let first_error =
        |r: &Evaluation| r.errors().first().map(|e| format!("{} | {:?}", e.message, e.hint)).unwrap_or_default();
    // With the crank driven, all three other pivots must turn: fixing the closing joint's
    // angle over-constrains the loop.
    let (_, r) = four_bar("60 deg", "");
    let e = first_error(&r);
    assert!(e.contains("cannot close") && r.errors().first().is_some_and(|e| e.feature == "J4"), "{e}");
    // Nothing free: a second joint on the lid cannot also put it on the base's bottom.
    let rigid = "joint J1 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(end), flip: true)
";
    let (_, r) = eval(&format!("{rigid}joint J2 (type: rigid, body: Lid, at: Lid/cap(start), to: Base/cap(start))"));
    let e = first_error(&r);
    assert!(e.contains("cannot close") && e.contains("already placed by J1") && e.contains("free"), "{e}");
    // Free, but a turn cannot make up for a 5 mm gap.
    let (_, r) = eval(&format!(
        "{rigid}joint J2 (type: revolute, body: Lid, at: Lid/cap(start), to: Base/cap(start), free: [angle])"
    ));
    let e = first_error(&r);
    assert!(e.contains("cannot close") && e.contains("mm") && e.contains("cannot be assembled"), "{e}");
}
