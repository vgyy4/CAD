//! Text (ADR 0029) with the real OCCT kernel: embossed, engraved, names and the command.

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

#[test]
fn text_as_a_new_body_and_engraved_into_a_face() {
    let (ev, r) =
        eval(BOX, "text Text1 (text: \"Gusset 42\", plane: XY, at: [0 mm, 30 mm], height: 5 mm, distance: 1 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let v = volume(&ev, &r, "Text1");
    assert!(v > 10.0 && v < 200.0, "{v}");
    let n = names(&r, "Text1");
    assert!(n.contains(&"Text1/cap(end)".to_string()), "{n:?}");
    assert!(n.iter().any(|f| f.starts_with("Text1/side(Text1/g0_1_g")), "{n:?}");
    let b = ev.kernel().bbox(&r.body("Text1").unwrap().shape).unwrap();
    assert!((b.max[2] - b.min[2] - 1.0).abs() < 1e-6, "{b:?}");
    // Engraved 0.5 mm into the top of the box: the letters' volume is removed.
    let (ev, r) = eval(
        BOX,
        "text Text1 (text: \"AB\", plane: Extrude1/cap(end), at: [20 mm, 10 mm], height: 6 mm, distance: -0.5 mm, align: center, op: cut)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let (ev2, r2) = eval(BOX, "text Text1 (text: \"AB\", plane: XY, at: [0 mm, 0 mm], height: 6 mm, distance: 0.5 mm)");
    let letters = volume(&ev2, &r2, "Text1");
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0 - letters), "{} {letters}", volume(&ev, &r, "Extrude1"));
    // Turned 90 degrees, the same letters stand along Y.
    let (ev, r) = eval(
        BOX,
        "text Text1 (text: \"AB\", plane: XY, at: [0 mm, 0 mm], height: 6 mm, distance: 0.5 mm, angle: 90 deg)",
    );
    let b = ev.kernel().bbox(&r.body("Text1").unwrap().shape).unwrap();
    assert!(b.max[1] - b.min[1] > b.max[0] - b.min[0], "{b:?}");
}

#[test]
fn text_errors_explain() {
    let (_, r) =
        eval(BOX, "text Text1 (text: \"\u{4e2d}\", plane: XY, at: [0 mm, 0 mm], height: 5 mm, distance: 1 mm)");
    assert!(r.errors()[0].message.contains("no glyph"), "{:?}", r.errors());
    let (_, r) = eval(BOX, "text Text1 (text: \"   \", plane: XY, at: [0 mm, 0 mm], height: 5 mm, distance: 1 mm)");
    assert!(r.errors()[0].message.contains("no visible"), "{:?}", r.errors());
}

#[test]
fn text_command_drives_the_real_kernel() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text(BOX).unwrap();
    let mut s = Session::new(&doc, kernel(), &CmdCtx::user(0));
    let u = CmdCtx::user(1);
    let r = execute(
        &mut s,
        &u,
        "feature.text",
        json!({"text": "V1", "plane": "Extrude1/cap(end)", "at": ["5 mm", "5 mm"], "height": "4 mm", "distance": "0.8 mm", "op": "add"}),
    )
    .unwrap();
    assert_eq!(r["rebuild_ok"], true, "{r}");
    // The top face's frame, in which `at` is given (F4-10).
    assert_eq!(r["frame"]["normal"], json!([0.0, 0.0, 1.0]), "{r}");
    let b = execute(&mut s, &u, "query.body", json!({})).unwrap();
    assert!(b["volume_mm3"].as_f64().unwrap() > 8000.0, "{b}");
}
