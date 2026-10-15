//! Offset face (ADR 0030) with the real OCCT kernel: exact volumes, names and the command.

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

#[test]
fn offset_faces_of_a_box_and_a_hole() {
    let (_, plain) = eval(BOX, "");
    let (ev, r) = eval(BOX, "param lift = 2 mm\noffset_face OffsetFace1 (faces: [Extrude1/cap(end)], distance: lift)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 40.0 * 20.0 * 12.0));
    let (mut a, mut b) = (names(&plain, "Extrude1"), names(&r, "Extrude1"));
    a.sort();
    b.sort();
    assert_eq!(a, b, "faces keep their names");
    // Two faces in at once.
    let (ev, r) =
        eval(BOX, "offset_face OffsetFace1 (faces: [Extrude1/cap(end), Extrude1/side(Sketch1/l6)], distance: -2 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 38.0 * 20.0 * 8.0));
    // The hole's wall moved 1 mm out of the material: radius 3 -> 2.
    let (ev, r) =
        eval(BOX, &format!("{HOLE}\noffset_face OffsetFace1 (faces: [Extrude2/side(S2/c2)], distance: 1 mm)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let want = 8000.0 - std::f64::consts::PI * 4.0 * 10.0;
    assert!(near(volume(&ev, &r, "Extrude1"), want), "{} vs {want}", volume(&ev, &r, "Extrude1"));
}

#[test]
fn offset_face_command_drives_the_real_kernel() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text(BOX).unwrap();
    let mut s = Session::new(&doc, kernel(), &CmdCtx::user(0));
    let u = CmdCtx::user(1);
    let r = execute(&mut s, &u, "feature.offset_face", json!({"faces": ["Extrude1/cap(end)"], "distance": "-4 mm"}))
        .unwrap();
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = execute(&mut s, &u, "query.body", json!({})).unwrap();
    assert!(near(b["volume_mm3"].as_f64().unwrap(), 4800.0), "{b}");
}
