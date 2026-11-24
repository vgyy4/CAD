//! Ribs (ADR 0062) with the real OCCT kernel: a gusset in an L bracket from a line drawn short
//! of both walls, to one side, a rib along the normal in a tray, the refusal when the rib grows
//! away from the part, and the command.

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

fn line(name: &str, plane: &str, a: [f64; 2], b: [f64; 2]) -> String {
    format!(
        "sketch {name} (plane: {plane}, next_id: 4) {{
  p1 = point({}, {})
  p2 = point({}, {})
  l3 = line(p1, p2)
}}

",
        a[0], a[1], b[0], b[1]
    )
}

/// An L bracket 40 long, 20 wide (y -10..10): a 5 mm plate (z 0..5) and a 5 mm wall (x 0..5)
/// 40 high, with the rib's line on XZ (y = 0) on x + z = 25, drawn short of both.
fn bracket() -> String {
    format!(
        "gusset 1\nunits mm\n\n{}extrude Plate (profile: Sk1, distance: 5 mm, op: new)\n\n{}extrude Wall (profile: Sk2, distance: 40 mm, op: add, body: Plate)\n\n{}",
        rect("Sk1", "XY", 0.0, -10.0, 40.0, 10.0),
        rect("Sk2", "XY", 0.0, -10.0, 5.0, 10.0),
        line("Gusset", "XZ", [17.0, 8.0], [8.0, 17.0]),
    )
}

/// The bracket: plate 40 x 20 x 5 and wall 5 x 20 x 35 above it.
const BRACKET: f64 = 4000.0 + 3500.0;

fn eval(text: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn mass(ev: &Evaluator, r: &Evaluation, body: &str) -> gusset_kernel::MassProps {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

#[test]
fn a_gusset_reaches_both_walls_from_a_short_line() {
    let (ev, r) = eval(&bracket());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(mass(&ev, &r, "Plate").volume, BRACKET));
    // The triangle between the walls (legs 15 mm) times 4 mm, centred on y = 0.
    let (ev, r) = eval(&format!("{}rib Rib1 (path: Gusset, thickness: 4 mm, toward: [-1, -1])\n", bracket()));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let m = mass(&ev, &r, "Plate");
    assert!(close(m.volume, BRACKET + 450.0), "{}", m.volume);
    assert!(m.centroid[1].abs() < 1e-9, "{:?}", m.centroid);
    // All to the front (along XZ's normal, -y): the same volume, its centroid at y = -2.
    let (ev, r) =
        eval(&format!("{}rib Rib1 (path: Gusset, thickness: 4 mm, toward: [-1, -1], side: front)\n", bracket()));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let m = mass(&ev, &r, "Plate");
    assert!(close(m.volume, BRACKET + 450.0), "{}", m.volume);
    assert!(close(m.centroid[1] * m.volume, -2.0 * 450.0), "{:?}", m.centroid);
}

#[test]
fn a_rib_growing_away_from_the_part_is_refused() {
    let (_, r) = eval(&format!("{}rib Rib1 (path: Gusset, thickness: 4 mm, toward: [1, 1])\n", bracket()));
    let e = r.errors().into_iter().find(|e| e.feature == "Rib1").unwrap();
    assert!(e.message.contains("meets no face"), "{}", e.message);
}

#[test]
fn a_rib_along_the_normal_reaches_down_to_the_floor_and_the_walls() {
    // A tray: a floor (z 0..5) and two end walls (x 0..5 and 35..40) 30 high; the rib's line at
    // z = 25 along x, short of both walls, grows down to the floor: 30 x 4 x 20.
    let text = format!(
        "gusset 1\nunits mm\n\n{}extrude Tray (profile: Sk1, distance: 5 mm, op: new)\n\n{}extrude Ends (profile: Sk2, distance: 30 mm, op: add, body: Tray)\n\nplane Top (base: XY, offset: 25 mm)\n\n{}rib Rib1 (path: Web, thickness: 4 mm, normal: true, reverse: true)\n",
        rect("Sk1", "XY", 0.0, -10.0, 40.0, 10.0),
        "sketch Sk2 (plane: XY, next_id: 17) {\n  p1 = point(0, -10)\n  p2 = point(5, -10)\n  p3 = point(5, 10)\n  p4 = point(0, 10)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n  p9 = point(35, -10)\n  p10 = point(40, -10)\n  p11 = point(40, 10)\n  p12 = point(35, 10)\n  l13 = line(p9, p10)\n  l14 = line(p10, p11)\n  l15 = line(p11, p12)\n  l16 = line(p12, p9)\n}\n\n",
        line("Web", "Top", [12.0, 0.0], [28.0, 0.0]),
    );
    let (ev, r) = eval(&text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let m = mass(&ev, &r, "Tray");
    assert!(close(m.volume, 4000.0 + 2.0 * 2500.0 + 2400.0), "{}", m.volume);
}

#[test]
fn the_command_adds_a_rib() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text(&bracket()).unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.rib",
        json!({"sketch": "Gusset", "thickness": "4 mm", "toward": [-1, -1], "name": "Rib1"}),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Plate"})).unwrap();
    assert!((b["volume_mm3"].as_f64().unwrap() - (BRACKET + 450.0)).abs() < 1e-3, "{b}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("rib Rib1 (path: Gusset, thickness: 4 mm, toward: [-1, -1], body: Plate)"), "{text}");
    // Either in the plane or along the normal.
    let e = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.rib",
        json!({"sketch": "Gusset", "thickness": "4 mm", "toward": [-1, -1], "normal": true}),
    );
    assert!(e.is_err());
}
