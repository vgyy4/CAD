//! Unequal chamfers (chamfer ADR amendment) with the real OCCT kernel: exact volumes, the
//! reference face rule, and names that survive regeneration.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

const BLOCK: &str = "gusset 1
units mm

param h = 10 mm

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

extrude Extrude1 (profile: Sketch1, distance: h, op: new)
";

/// The cap/side edge along the 20 mm side `l6`.
const EDGE: &str = "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))";

fn eval(extra: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(&format!("{BLOCK}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

fn volume(ev: &Evaluator, r: &Evaluation) -> f64 {
    ev.kernel().mass_properties(&r.body("Extrude1").unwrap().shape).unwrap().volume
}

/// Area of the named face of the body.
fn area(r: &Evaluation, face: &str) -> f64 {
    let b = r.body("Extrude1").unwrap();
    let i = b.names.faces.iter().position(|f| f.to_string() == face).unwrap_or_else(|| {
        panic!("{face} not in {:?}", b.names.faces.iter().map(ToString::to_string).collect::<Vec<_>>())
    });
    b.topology.faces[i].area
}

#[test]
fn two_distances_cut_the_exact_wedge() {
    // 2 mm along the cap, 3 mm down the side; the edge is 20 mm long.
    let (ev, r) =
        eval(&format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, distance2: 3 mm, faces: [Extrude1/cap(end)])"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r), 8000.0 - 0.5 * 2.0 * 3.0 * 20.0), "{}", volume(&ev, &r));
    assert!(near(area(&r, "Extrude1/cap(end)"), 800.0 - 2.0 * 20.0));
    assert!(near(area(&r, "Extrude1/side(Sketch1/l6)"), 200.0 - 3.0 * 20.0));
    // The chamfer face keeps its persistent name.
    let b = r.body("Extrude1").unwrap();
    assert!(b.names.faces.iter().any(|f| f.to_string().starts_with("C1/blend(")));
}

#[test]
fn the_first_distance_follows_the_reference_face() {
    let swapped =
        format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, distance2: 3 mm, faces: [Extrude1/side(Sketch1/l6)])");
    let (ev, r) = eval(&swapped);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r), 8000.0 - 0.5 * 2.0 * 3.0 * 20.0));
    assert!(near(area(&r, "Extrude1/side(Sketch1/l6)"), 200.0 - 2.0 * 20.0));
    assert!(near(area(&r, "Extrude1/cap(end)"), 800.0 - 3.0 * 20.0));
}

#[test]
fn default_reference_face_is_the_one_whose_name_sorts_first() {
    // No `faces`: `side(...)` sorts before `cap(...)`, so the 2 mm leg is on the side face.
    let (ev, r) = eval(&format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, distance2: 3 mm)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r), 8000.0 - 0.5 * 2.0 * 3.0 * 20.0));
    assert!(near(area(&r, "Extrude1/side(Sketch1/l6)"), 200.0 - 2.0 * 20.0));
    assert!(near(area(&r, "Extrude1/cap(end)"), 800.0 - 3.0 * 20.0));
}

#[test]
fn distance_and_angle() {
    // 2 mm along the cap, 45 deg: both legs 2 mm.
    let (ev, r) =
        eval(&format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, angle: 45 deg, faces: [Extrude1/cap(end)])"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r), 8000.0 - 0.5 * 2.0 * 2.0 * 20.0));
    // 30 deg from the side face: the cap leg is 4 * tan(30 deg).
    let leg = 4.0 * 30.0_f64.to_radians().tan();
    let (ev, r) = eval(&format!(
        "chamfer C1 (edges: [{EDGE}], distance: 4 mm, angle: 30 deg, faces: [Extrude1/side(Sketch1/l6)])"
    ));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r), 8000.0 - 0.5 * 4.0 * leg * 20.0), "{}", volume(&ev, &r));
    assert!(near(area(&r, "Extrude1/side(Sketch1/l6)"), 200.0 - 4.0 * 20.0));
    assert!(near(area(&r, "Extrude1/cap(end)"), 800.0 - leg * 20.0));
}

#[test]
fn names_survive_a_parameter_change_and_equal_chamfers_are_unchanged() {
    let text = format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, distance2: 3 mm, faces: [Extrude1/cap(end)])");
    for h in ["10 mm", "25 mm"] {
        let doc = format!("{BLOCK}\n{text}\n").replace("param h = 10 mm", &format!("param h = {h}"));
        let doc = gusset_doc::Document::from_text(&doc).unwrap();
        let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
        let mut ev = Evaluator::new(k);
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{h}: {:?}", r.errors());
        let hv: f64 = h.trim_end_matches(" mm").parse().unwrap();
        // The edge on `l6` is as long as the block is wide on that side (20 mm).
        assert!(near(volume(&ev, &r), 800.0 * hv - 0.5 * 2.0 * 3.0 * 20.0), "{h}: {}", volume(&ev, &r));
    }
    let (ev, r) = eval(&format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm)"));
    assert!(near(volume(&ev, &r), 8000.0 - 0.5 * 2.0 * 2.0 * 20.0));
}

#[test]
fn bad_input_is_a_structured_error() {
    let (_, r) = eval(&format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, angle: 95 deg)"));
    assert!(r.errors()[0].message.contains("chamfer angle"), "{:?}", r.errors());
    let (_, r) = eval(&format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, distance2: 0 mm)"));
    assert!(r.errors()[0].message.contains("second distance"), "{:?}", r.errors());
    // A reference face that is not a neighbour of the edge.
    let (_, r) = eval(&format!(
        "chamfer C1 (edges: [{EDGE}], distance: 2 mm, distance2: 3 mm, faces: [Extrude1/side(Sketch1/l8)])"
    ));
    assert!(r.errors()[0].message.contains("does not border"), "{:?}", r.errors());
    // Too large for the block.
    let (_, r) = eval(&format!("chamfer C1 (edges: [{EDGE}], distance: 2 mm, distance2: 500 mm)"));
    assert!(!r.errors().is_empty());
}
