//! Move face (ADR 0031) with the real OCCT kernel: holes and bosses slide and turn, names stay.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{Evaluation, Evaluator};
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

const HOLE: &str = "
sketch S2 (plane: Extrude1/cap(end), next_id: 3) {
  p1 = point(20, 10)
  c2 = circle(p1, 3)
}

extrude Extrude2 (profile: S2, distance: -10 mm, op: cut, body: Extrude1)
";

const BOSS: &str = "
sketch S3 (plane: Extrude1/cap(end), next_id: 3) {
  p1 = point(10, 10)
  c2 = circle(p1, 4)
}

extrude Extrude3 (profile: S3, distance: 5 mm, op: add, body: Extrude1)
";

/// The axis origin (x, y) of the named cylindrical face.
fn axis_xy(r: &Evaluation, body: &str, face: &str) -> [f64; 2] {
    let b = r.body(body).unwrap();
    let i = b.names.faces.iter().position(|n| n.to_string() == face).unwrap_or_else(|| panic!("{face}"));
    let a = b.topology.faces[i].axis.unwrap();
    [a.origin[0], a.origin[1]]
}

#[test]
fn holes_and_bosses_slide_and_turn() {
    let (_, plain) = eval(BOX, HOLE);
    let hole_volume = 8000.0 - std::f64::consts::PI * 9.0 * 10.0;
    let (ev, r) = eval(
        BOX,
        &format!(
            "{HOLE}\nparam shift = 8 mm\nmove_face MoveFace1 (faces: [Extrude2/side(S2/c2)], translate: [shift, 2 mm, 0 mm])"
        ),
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), hole_volume));
    let xy = axis_xy(&r, "Extrude1", "Extrude2/side(S2/c2)");
    assert!(near(xy[0], 28.0) && near(xy[1], 12.0), "{xy:?}");
    let (mut a, mut b) = (names(&plain, "Extrude1"), names(&r, "Extrude1"));
    a.sort();
    b.sort();
    assert_eq!(a, b, "faces keep their names");
    // A quarter turn about the vertical axis through (20, 5): (20, 10) -> (15, 5).
    let (_, r) = eval(
        BOX,
        &format!(
            "{HOLE}\nmove_face MoveFace1 (faces: [Extrude2/side(S2/c2)], axis: Z, angle: 90 deg, through: [20 mm, 5 mm, 0 mm])"
        ),
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let xy = axis_xy(&r, "Extrude1", "Extrude2/side(S2/c2)");
    assert!(near(xy[0], 15.0) && near(xy[1], 5.0), "{xy:?}");
    // A boss with its top: both faces move together.
    let (ev, r) = eval(
        BOX,
        &format!(
            "{BOSS}\nmove_face MoveFace1 (faces: [Extrude3/side(S3/c2), Extrude3/cap(end)], translate: [15 mm, 0 mm, 0 mm])"
        ),
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0 + std::f64::consts::PI * 16.0 * 5.0));
    let xy = axis_xy(&r, "Extrude1", "Extrude3/side(S3/c2)");
    assert!(near(xy[0], 25.0) && near(xy[1], 10.0), "{xy:?}");
    // A side face along its normal; a zero move leaves the body alone.
    let (ev, r) = eval(BOX, "move_face MoveFace1 (faces: [Extrude1/side(Sketch1/l6)], translate: [5 mm, 0 mm, 0 mm])");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 45.0 * 20.0 * 10.0));
    let (ev, r) = eval(
        BOX,
        "param slide = 0 mm\nmove_face MoveFace1 (faces: [Extrude1/side(Sketch1/l6)], translate: [slide, 0 mm, 0 mm])",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0));
}

#[test]
fn moves_the_faces_around_cannot_follow_are_refused() {
    // The top sliding sideways would need the side faces to tilt.
    let (_, r) = eval(BOX, "move_face MoveFace1 (faces: [Extrude1/cap(end)], translate: [3 mm, 0 mm, 0 mm])");
    let e = r.errors();
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].message.contains("change shape"), "{e:?}");
    // The hole pushed through the box's side.
    let (_, r) = eval(
        BOX,
        &format!("{HOLE}\nmove_face MoveFace1 (faces: [Extrude2/side(S2/c2)], translate: [19 mm, 0 mm, 0 mm])"),
    );
    assert_eq!(r.errors().len(), 1, "{:?}", r.errors());
}

#[test]
fn move_face_command_drives_the_real_kernel() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text(&format!("{BOX}\n{HOLE}\n")).unwrap();
    let mut s = Session::new(&doc, kernel(), &CmdCtx::user(0));
    let u = CmdCtx::user(1);
    let r = execute(
        &mut s,
        &u,
        "feature.move_face",
        json!({"faces": ["Extrude2/side(S2/c2)"], "translate": ["-10 mm", "0 mm", "0 mm"]}),
    )
    .unwrap();
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = execute(&mut s, &u, "query.body", json!({})).unwrap();
    assert!(near(b["volume_mm3"].as_f64().unwrap(), 8000.0 - std::f64::consts::PI * 90.0), "{b}");
}
