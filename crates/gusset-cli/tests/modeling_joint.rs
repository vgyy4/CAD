//! Joints with the real kernel (ADR 0026): frames on cylinder axes, a shaft placed in a
//! bore with a cylindrical joint, and motion driven by parameters.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp, clippy::disallowed_methods)]

use gusset_doc::Document;
use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

const PLATE_AND_SHAFT: &str = "gusset 1
units mm

param slide = 0 mm
param turn = 0 deg

sketch Plate_s (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 30)
  p4 = point(0, 30)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Plate (profile: Plate_s, distance: 6 mm, op: new)

sketch Bore_s (plane: XY, next_id: 3) {
  p1 = point(25, 12)
  c2 = circle(p1, 4)
}

extrude Bore (profile: Bore_s, distance: 6 mm, op: cut, body: Plate)

sketch Shaft_s (plane: XZ, next_id: 3) {
  p1 = point(-50, 7)
  c2 = circle(p1, 4)
}

extrude Shaft (profile: Shaft_s, distance: 20 mm, op: new)

joint J1 (type: cylindrical, body: Shaft, at: Shaft/side(Shaft_s/c2), to: Bore/side(Bore_s/c2), distance: slide, angle: turn)
";

fn eval(slide: &str) -> (Evaluator, Evaluation) {
    let text = PLATE_AND_SHAFT.replace("param slide = 0 mm", &format!("param slide = {slide}"));
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn shaft_axis(ev: &Evaluator, r: &Evaluation) -> gusset_kernel::Axis3 {
    let t = ev.kernel().topology(&r.body("Shaft").unwrap().shape).unwrap();
    t.faces.iter().find_map(|f| f.axis).unwrap()
}

#[test]
fn a_cylindrical_joint_puts_the_shaft_on_the_bore_axis_and_slides_it() {
    let (ev, r) = eval("0 mm");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The shaft's axis is the bore's: vertical through (25, 12).
    let a = shaft_axis(&ev, &r);
    assert!((a.dir[2].abs() - 1.0).abs() < 1e-9, "{a:?}");
    assert!((a.origin[0] - 25.0).abs() < 1e-9 && (a.origin[1] - 12.0).abs() < 1e-9, "{a:?}");
    // The shaft's middle sits at the bore's middle (z = 3).
    let bb = ev.kernel().bbox(&r.body("Shaft").unwrap().shape).unwrap();
    assert!(((bb.min[2] + bb.max[2]) / 2.0 - 3.0).abs() < 1e-6, "{bb:?}");
    // Sliding moves it along the axis; the volume does not change.
    let (ev2, r2) = eval("5 mm");
    assert!(r2.errors().is_empty(), "{:?}", r2.errors());
    let bb2 = ev2.kernel().bbox(&r2.body("Shaft").unwrap().shape).unwrap();
    assert!(((bb2.min[2] - bb.min[2]).abs() - 5.0).abs() < 1e-6, "{bb:?} {bb2:?}");
    let vol =
        |ev: &Evaluator, r: &Evaluation| ev.kernel().mass_properties(&r.body("Shaft").unwrap().shape).unwrap().volume;
    assert!((vol(&ev, &r) - vol(&ev2, &r2)).abs() < 1e-6);
    assert!((vol(&ev, &r) - std::f64::consts::PI * 16.0 * 20.0).abs() < 1e-3);
}
