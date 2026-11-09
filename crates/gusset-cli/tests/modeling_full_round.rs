//! Full-round fillets (ADR 0004 amendment) with the real OCCT kernel: exact volume, the
//! persistent name of the rounded face, and regeneration after a parameter change.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

/// A 20 x 4 x `h` slab; its end face on `l6` is 4 wide and `h` tall.
const SLAB: &str = "gusset 1
units mm

param h = 10 mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(20, 0)
  p3 = point(20, 4)
  p4 = point(0, 4)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: h, op: new)
";

const END: &str = "Extrude1/side(Sketch1/l6)";

fn eval(text: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
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

fn expected(t: f64, h: f64) -> f64 {
    20.0 * t * h - (t * (t / 2.0) - std::f64::consts::PI * t * t / 8.0) * h
}

#[test]
fn a_full_round_has_the_exact_volume_and_a_persistent_name() {
    let (ev, r) = eval(&format!("{SLAB}\nfillet F1 (full_round: [{END}])\n"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r), expected(4.0, 10.0)), "{}", volume(&ev, &r));
    let b = r.body("Extrude1").unwrap();
    let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
    // The end face is gone, the rounded face is a blend of one of its edges.
    assert!(!names.iter().any(|n| n == END), "{names:?}");
    let blends: Vec<usize> = (0..names.len()).filter(|&i| names[i].starts_with("F1/blend(")).collect();
    assert_eq!(blends.len(), 1, "{names:?}");
    let f = &b.topology.faces[blends[0]];
    assert!(near(f.area, std::f64::consts::PI * 2.0 * 10.0), "{}", f.area);
}

#[test]
fn the_round_follows_the_wall_thickness_when_parameters_change() {
    for (t, h) in [(4.0, 10.0), (6.0, 10.0), (3.0, 25.0)] {
        let text = format!("{SLAB}\nfillet F1 (full_round: [{END}])\n")
            .replace("point(20, 4)", &format!("point(20, {t})"))
            .replace("point(0, 4)", &format!("point(0, {t})"))
            .replace("param h = 10 mm", &format!("param h = {h} mm"));
        let (ev, r) = eval(&text);
        assert!(r.errors().is_empty(), "{t}: {:?}", r.errors());
        assert!(near(volume(&ev, &r), expected(t, h)), "{t} x {h}: {}", volume(&ev, &r));
    }
}

#[test]
fn a_later_feature_can_use_the_rounded_face() {
    // The rounded face is named after the edge it grew from (the smaller-named one).
    let round = "F1/blend(edge(Extrude1/side(Sketch1/l5), Extrude1/side(Sketch1/l6)))";
    let text = format!(
        "{SLAB}\nfillet F1 (full_round: [{END}])\n\nchamfer C1 (edges: [edge(Extrude1/cap(end), {round})], distance: 1 mm)\n"
    );
    let (ev, r) = eval(&text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(volume(&ev, &r) < expected(4.0, 10.0) - 1.0, "{}", volume(&ev, &r));
}

#[test]
fn bad_input_is_a_structured_error() {
    let (_, r) = eval(&format!("{SLAB}\nfillet F1 (full_round: [Extrude1/side(Sketch1/l99)])\n"));
    assert!(r.errors()[0].message.contains("no longer exists"), "{:?}", r.errors());
}
