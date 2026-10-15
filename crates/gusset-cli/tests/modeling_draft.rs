//! Draft (ADR 0028) with the real OCCT kernel: exact volumes, face names and the registry
//! command.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{ErrorCode, Evaluation, Evaluator};
use std::sync::Arc;

const BOX: &str = "gusset 1
units mm

param n_bolts = 6

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

const PEG: &str = "gusset 1
units mm

param n_bolts = 6

sketch Sketch1 (plane: XY, next_id: 3) {
  p1 = point(30, 0)
  c2 = circle(p1, 5)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)
";

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn eval(base: &str, extra: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(&format!("{base}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

fn names(r: &Evaluation, body: &str) -> Vec<String> {
    r.body(body).unwrap().names.faces.iter().map(ToString::to_string).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

const SIDES: &str =
    "Extrude1/side(Sketch1/l5), Extrude1/side(Sketch1/l6), Extrude1/side(Sketch1/l7), Extrude1/side(Sketch1/l8)";

/// The 40x20x10 box with its sides drafted by `deg` about its bottom or top: a prismatoid,
/// h/6 (A_0 + A_1 + 4 A_mid), the sides moving by 10 tan(a) between bottom and top.
fn frustum(deg: f64, narrow_up: bool) -> f64 {
    let off = 10.0 * libm::tan(deg.to_radians());
    let rect = |s: f64| (40.0 - 2.0 * s) * (20.0 - 2.0 * s);
    let (a0, a1) = if narrow_up { (0.0, off) } else { (off, 0.0) };
    10.0 / 6.0 * (rect(a0) + rect(a1) + 4.0 * rect((a0 + a1) / 2.0))
}

#[test]
fn draft_box_sides_about_base_plane_and_faces() {
    let (_, plain) = eval(BOX, "");
    let (ev, r) = eval(BOX, &format!("draft Draft1 (faces: [{SIDES}], plane: XY, angle: 5 deg)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), frustum(5.0, true)), "{}", volume(&ev, &r, "Extrude1"));
    // A modification: every face keeps its name.
    let (mut a, mut b) = (names(&plain, "Extrude1"), names(&r, "Extrude1"));
    a.sort();
    b.sort();
    assert_eq!(a, b);
    // About the top face, pulling into the body: the part narrows downward.
    let (ev, r) = eval(BOX, &format!("draft Draft1 (faces: [{SIDES}], plane: Extrude1/cap(end), angle: 5 deg)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), frustum(5.0, false)));
    // flip and a negative angle both widen it away from the plane.
    for extra in [", flip: true", ""] {
        let angle = if extra.is_empty() { "-5 deg" } else { "5 deg" };
        let (ev, r) = eval(BOX, &format!("draft Draft1 (faces: [{SIDES}], plane: XY, angle: {angle}{extra})"));
        assert!(r.errors().is_empty(), "{extra}: {:?}", r.errors());
        assert!(near(volume(&ev, &r, "Extrude1"), frustum(-5.0, true)), "{extra}");
    }
    // A shell of the drafted part still finds its faces.
    let (_, r) = eval(
        BOX,
        &format!(
            "draft Draft1 (faces: [{SIDES}], plane: XY, angle: 3 deg)\nshell Shell1 (faces: [Extrude1/cap(end)], thickness: 1 mm)"
        ),
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
}

#[test]
fn draft_a_cylinder_into_a_cone() {
    let (ev, r) = eval(PEG, "draft Draft1 (faces: [Extrude1/side(Sketch1/c2)], plane: XY, angle: 10 deg)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let (r0, r1) = (5.0, 5.0 - 10.0 * libm::tan(10f64.to_radians()));
    let want = std::f64::consts::PI * 10.0 / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
    assert!(near(volume(&ev, &r, "Extrude1"), want), "{} vs {want}", volume(&ev, &r, "Extrude1"));
}

#[test]
fn draft_errors_explain() {
    // The top is parallel to the neutral plane: nothing to tilt about.
    let (_, r) = eval(BOX, "draft Draft1 (faces: [Extrude1/cap(end)], plane: XY, angle: 5 deg)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure, "{e}");
    assert!(e.hint.is_some(), "{e}");
    let (_, r) = eval(BOX, &format!("draft Draft1 (faces: [{SIDES}], plane: XY, angle: 90 deg)"));
    assert!(r.errors()[0].message.contains("between 0 and 90"), "{:?}", r.errors());
}

#[test]
fn draft_command_drives_the_real_kernel() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text(BOX).unwrap();
    let mut s = Session::new(&doc, kernel(), &CmdCtx::user(0));
    let u = CmdCtx::user(1);
    let faces: Vec<&str> = SIDES.split(", ").collect();
    let r = execute(&mut s, &u, "feature.draft", json!({"faces": faces, "plane": "XY", "angle": "5 deg"})).unwrap();
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = execute(&mut s, &u, "query.body", json!({})).unwrap();
    assert!(near(b["volume_mm3"].as_f64().unwrap(), frustum(5.0, true)), "{b}");
}
