//! A body face as the profile of an extrude, sweep or loft, with the real OCCT kernel: the
//! face's outline (and holes) sweeps like a sketch region, and the lateral faces are named
//! after the body edges they come from (`along(<edge>)`).

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

/// A 40 x 20 plate 10 mm thick with a hole of r 5 about (10, 10).
const PLATE: &str = "gusset 1
units mm

sketch Sketch1 (plane: XY, next_id: 11) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
  p9 = point(10, 10)
  c10 = circle(p9, 5)
}

extrude Plate (profile: Sketch1, distance: 10 mm, op: new)

sketch Path (plane: XZ, next_id: 4) {
  p1 = point(30, 10)
  p2 = point(30, 30)
  l3 = line(p1, p2)
}
";

fn eval(extra: &str) -> (Evaluator, Evaluation) {
    let text = format!("{PLATE}\n{extra}\n");
    let doc = gusset_doc::Document::from_text(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc.to_text().trim_end(), text.trim_end(), "the text round-trips");
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

const PLATE_VOLUME: f64 = 10.0 * (800.0 - 25.0 * std::f64::consts::PI);

#[test]
fn an_extrude_of_a_cap_adds_the_plate_again() {
    let (ev, r) = eval("extrude Raise (profile: Plate/cap(end), distance: 5 mm, op: add, body: Plate)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Plate").unwrap();
    let m = ev.kernel().mass_properties(&b.shape).unwrap();
    assert!(near(m.volume, PLATE_VOLUME * 1.5), "{}", m.volume);
    let bb = ev.kernel().bbox(&b.shape).unwrap();
    assert!(near(bb.max[2], 15.0), "{:?}", bb);

    // A new body: the outward normal of the cap is +z; its walls are named after the plate's
    // top edges, the hole's wall included.
    let (ev, r) = eval("extrude Lid (profile: Plate/cap(end), distance: 2 mm, op: new)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let lid = r.body("Lid").unwrap();
    let m = ev.kernel().mass_properties(&lid.shape).unwrap();
    assert!(near(m.volume, PLATE_VOLUME * 0.2), "{}", m.volume);
    let bb = ev.kernel().bbox(&lid.shape).unwrap();
    assert!(near(bb.min[2], 10.0) && near(bb.max[2], 12.0), "{:?}", bb);
    let names: Vec<String> = lid.names.faces.iter().map(ToString::to_string).collect();
    assert!(names.iter().any(|n| n == "Lid/along(edge(Plate/cap(end), Plate/side(Sketch1/c10)))"), "{names:?}");
    assert!(names.iter().any(|n| n == "Lid/along(edge(Plate/cap(end), Plate/side(Sketch1/l5)))"), "{names:?}");
    assert!(names.iter().any(|n| n == "Lid/cap(end)"), "{names:?}");
    assert_eq!(names.len(), 7, "{names:?}");
}

#[test]
fn a_cut_of_a_side_face_and_a_bottom_face() {
    // The side y = 0 (outward -y), cut 5 mm into the plate: a quarter of its depth.
    let (ev, r) = eval("extrude Pocket (profile: Plate/side(Sketch1/l5), distance: -5 mm, op: cut, body: Plate)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Plate").unwrap();
    let m = ev.kernel().mass_properties(&b.shape).unwrap();
    // The cut removes y 0..5, which the hole (y 5..15) does not reach.
    assert!(near(m.volume, PLATE_VOLUME - 40.0 * 10.0 * 5.0), "{}", m.volume);
    // The bottom faces down: a positive distance goes below z 0.
    let (ev, r) = eval("extrude Foot (profile: Plate/cap(start), distance: 1 mm, op: new)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let bb = ev.kernel().bbox(&r.body("Foot").unwrap().shape).unwrap();
    assert!(near(bb.min[2], -1.0) && near(bb.max[2], 0.0), "{:?}", bb);
}

#[test]
fn a_face_sweeps_along_a_path_and_lofts_to_another_face() {
    // The side x = 40 swept 20 mm along +z... the path runs up from the side's height.
    let (ev, r) = eval(
        "extrude Post (profile: Sketch1.regions(c10), distance: 30 mm, op: new)

loft Bridge (profiles: [Post/cap(end), Plate/side(Sketch1/l6)], op: new)",
    );
    // A circle to a rectangle: the loft builds a solid between the two faces.
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Bridge").unwrap();
    assert_eq!(b.topology.solids, 1);
    assert!(ev.kernel().mass_properties(&b.shape).unwrap().volume > 100.0);
    let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
    assert!(names.iter().any(|n| n.starts_with("Bridge/along(edge(")), "{names:?}");

    let (ev, r) = eval("sweep Rail (profile: Plate/cap(end), path: Path, orientation: fixed, op: new)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let rail = r.body("Rail").unwrap();
    let m = ev.kernel().mass_properties(&rail.shape).unwrap();
    assert!(near(m.volume.abs(), PLATE_VOLUME * 2.0), "{}", m.volume);
}

#[test]
fn a_revolve_or_a_curved_face_is_refused() {
    let (_, r) = eval(
        "extrude Post (profile: Sketch1.regions(c10), distance: 30 mm, op: new)

extrude Bad (profile: Post/side(Sketch1/c10), distance: 1 mm, op: new)",
    );
    let e = r.errors();
    assert!(e.iter().any(|e| e.feature == "Bad" && e.message.contains("planar")), "{e:?}");
    let (_, r) = eval("revolve Turn (profile: Plate/cap(end), axis: X, angle: 90 deg, op: new)");
    let e = r.errors();
    assert!(e.iter().any(|e| e.feature == "Turn" && e.message.contains("sketch profile")), "{e:?}");
}
