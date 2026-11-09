//! `refine: false` (ADR 0004 amendment) with the real OCCT kernel: a join that keeps its
//! split faces, stable names for them, and edges of the seam usable by a later fillet.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

/// Two 10 mm squares side by side.
const BASE: &str = "gusset 1
units mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)

sketch Sketch2 (plane: XY, next_id: 9) {
  p1 = point(10, 0)
  p2 = point(20, 0)
  p3 = point(20, 10)
  p4 = point(10, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}
";

fn eval(text: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn join(extra: &str) -> String {
    format!("{BASE}\nextrude Extrude2 (profile: Sketch2, distance: 10 mm, op: add, body: Extrude1{extra})\n")
}

fn names(r: &Evaluation) -> Vec<String> {
    r.body("Extrude1").unwrap().names.faces.iter().map(ToString::to_string).collect()
}

fn volume(ev: &Evaluator, r: &Evaluation) -> f64 {
    ev.kernel().mass_properties(&r.body("Extrude1").unwrap().shape).unwrap().volume
}

#[test]
fn refine_false_keeps_the_seam_and_gives_every_face_its_own_name() {
    let (ev, refined) = eval(&join(""));
    assert!(refined.errors().is_empty(), "{:?}", refined.errors());
    assert_eq!(refined.body("Extrude1").unwrap().topology.faces.len(), 6);
    let (ev2, raw) = eval(&join(", refine: false"));
    assert!(raw.errors().is_empty(), "{:?}", raw.errors());
    assert_eq!(raw.body("Extrude1").unwrap().topology.faces.len(), 10);
    assert!((volume(&ev, &refined) - 2000.0).abs() < 1e-6 && (volume(&ev2, &raw) - 2000.0).abs() < 1e-6);
    let n = names(&raw);
    let mut unique = n.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), n.len(), "every face has its own name: {n:?}");
    // Same names after the second box grows: the split faces are found again.
    let (_, wide) = eval(&join(", refine: false").replace("point(20, ", "point(24, "));
    assert!(wide.errors().is_empty(), "{:?}", wide.errors());
    let mut a = n.clone();
    let mut b = names(&wide);
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[test]
fn a_fillet_can_round_an_edge_of_a_split_face() {
    let (_, raw) = eval(&join(", refine: false"));
    let n = names(&raw);
    // The far end of the second box is a face of its own; its top edge is a plain edge.
    let tops: Vec<&String> = n.iter().filter(|s| s.contains("cap(end)")).collect();
    assert_eq!(tops.len(), 2, "{n:?}");
    let sides: Vec<&String> = n.iter().filter(|s| s.contains("Sketch2/l6")).collect();
    assert_eq!(sides.len(), 1, "{n:?}");
    let text =
        join(", refine: false") + &format!("\nfillet F1 (edges: [edge({}, {})], radius: 1 mm)\n", tops[1], sides[0]);
    let (ev, r) = eval(&text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(volume(&ev, &r) < 2000.0 - 0.1, "{}", volume(&ev, &r));
}
