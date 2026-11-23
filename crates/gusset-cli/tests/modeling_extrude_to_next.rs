//! Extrudes up to the next face (`to: next`, ADR 0061 amendment) with the real OCCT kernel: a
//! closed tank, a cut through its bottom wall only, a boss grown up to it, and the command.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

fn rect(name: &str, plane: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> String {
    format!(
        "sketch {name} (plane: {plane}, next_id: 9) {{
  p1 = point({x0}, {y0})
  p2 = point({x1}, {y0})
  p3 = point({x1}, {y1})
  p4 = point({x0}, {y1})
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

"
    )
}

/// A closed tank 40 x 20 x 30 (z 0..30) with 5 mm walls, and a 10 x 10 square 10 mm below it.
fn tank() -> String {
    format!(
        "gusset 1\nunits mm\n\n{}extrude Tank (profile: Sk1, distance: 30 mm, op: new)\n\nplane Mid (base: XY, offset: 5 mm)\n\n{}extrude Cavity (profile: Sk2, distance: 20 mm, op: cut, body: Tank)\n\nplane Low (base: XY, offset: -10 mm)\n\n{}",
        rect("Sk1", "XY", 0.0, 0.0, 40.0, 20.0),
        rect("Sk2", "Mid", 5.0, 5.0, 35.0, 15.0),
        rect("Sk3", "Low", 12.0, 5.0, 22.0, 15.0),
    )
}

/// The tank's volume: 40 x 20 x 30 less the 30 x 10 x 20 cavity.
const TANK: f64 = 24000.0 - 6000.0;

fn eval(text: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

#[test]
fn a_cut_to_the_next_face_opens_one_wall_and_a_boss_grows_to_it() {
    let (ev, r) = eval(&tank());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(volume(&ev, &r, "Tank"), TANK));
    // The cut goes through the bottom wall (z 0..5), not the top wall behind the cavity.
    let (ev, r) = eval(&format!("{}extrude Hole (profile: Sk3, to: next, op: cut, body: Tank)\n", tank()));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(volume(&ev, &r, "Tank"), TANK - 500.0), "{}", volume(&ev, &r, "Tank"));
    // The boss grows from z -10 up to the tank's bottom face; reversed, it meets nothing.
    let (ev, r) = eval(&format!("{}extrude Boss (profile: Sk3, to: next, op: add, body: Tank)\n", tank()));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(volume(&ev, &r, "Tank"), TANK + 1000.0), "{}", volume(&ev, &r, "Tank"));
    let (_, r) =
        eval(&format!("{}extrude Boss (profile: Sk3, to: next, reverse: true, op: add, body: Tank)\n", tank()));
    let e = r.errors().into_iter().find(|e| e.feature == "Boss").unwrap();
    assert!(e.message.contains("meets no face"), "{}", e.message);
}

#[test]
fn a_new_body_up_to_the_next_face_leaves_the_body_it_stops_at() {
    // The boss as a body of its own: grown from z -10 up to the tank, which keeps its volume.
    let (ev, r) = eval(&format!("{}extrude Boss (profile: Sk3, to: next, op: new)\n", tank()));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(volume(&ev, &r, "Tank"), TANK), "{}", volume(&ev, &r, "Tank"));
    assert!(close(volume(&ev, &r, "Boss"), 1000.0), "{}", volume(&ev, &r, "Boss"));
}

#[test]
fn the_command_cuts_up_to_the_next_face() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text(&tank()).unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    execute(&mut s, &CmdCtx::user(1), "feature.up_to_next", json!({"sketch": "Sk3", "op": "cut", "name": "Hole"}))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Tank"})).unwrap();
    assert!((b["volume_mm3"].as_f64().unwrap() - (TANK - 500.0)).abs() < 1e-3, "{b}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("extrude Hole (profile: Sk3, to: next, op: cut, body: Tank)"), "{text}");
    // Only add or cut.
    let e = execute(&mut s, &CmdCtx::user(1), "feature.up_to_next", json!({"sketch": "Sk3", "op": "new"}));
    assert!(e.is_err());
}
