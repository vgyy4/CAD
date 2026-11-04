//! Delete face (ADR 0038) with the real OCCT kernel: a hole and a fillet removed from a
//! block heal back to the exact block; faces keep their names.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

const BLOCK: &str = "gusset 1
units mm

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

sketch S2 (plane: Extrude1/cap(end), next_id: 3) {
  p1 = point(20, 10)
  c2 = circle(p1, 3)
}

extrude Extrude2 (profile: S2, distance: -10 mm, op: cut, body: Extrude1)
";

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

#[test]
fn deleting_a_hole_and_a_fillet_heals_the_block() {
    let (ev, r) = eval(
        "fillet Fillet1 (edges: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))], radius: 2 mm)\n\n\
         delete_face DeleteFace1 (body: Extrude1, faces: [Extrude2/side(S2/c2), Fillet1/blend(edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6)))])",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Extrude1").unwrap();
    let v = ev.kernel().mass_properties(&b.shape).unwrap().volume;
    assert!(near(v, 40.0 * 20.0 * 10.0), "{v}");
    assert_eq!(b.topology.faces.len(), 6, "a plain block again");
    let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
    for want in ["Extrude1/cap(end)", "Extrude1/side(Sketch1/l6)", "Extrude1/cap(start)"] {
        assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
    }
}

#[test]
fn deleting_a_face_that_cannot_heal_fails_cleanly() {
    // The block's top: nothing around it can close a gap that size.
    let (_, r) = eval("delete_face DeleteFace1 (body: Extrude1, faces: [Extrude1/cap(end)])");
    let errs: Vec<String> = r.errors().iter().map(|e| e.message.clone()).collect();
    assert!(errs.iter().any(|e| e.contains("cannot be removed")), "{errs:?}");
}
