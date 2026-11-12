//! Surfaces (ADR 0036) with the real OCCT kernel: a box made of an extruded wall sheet and
//! two patches, stitched into a solid; a sheet thickened; names on the results.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

const SKETCHES: &str = "gusset 1
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

plane Top (base: XY, offset: 10 mm)

sketch Sketch2 (plane: Top, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}
";

fn eval(extra: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(&format!("{SKETCHES}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn a_wall_and_two_patches_stitch_into_a_box() {
    let (ev, r) = eval(
        "surface_extrude Wall (curves: Sketch1, distance: 10 mm)

patch Bottom (profile: Sketch1)

patch Lid (profile: Sketch2)

stitch Box (bodies: [Wall, Bottom, Lid], tolerance: 0.01 mm)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Box").unwrap();
    let m = ev.kernel().mass_properties(&b.shape).unwrap();
    assert!(near(m.volume.abs(), 8000.0), "{}", m.volume);
    assert_eq!(b.topology.solids, 1);
    assert!(r.body("Wall").is_none(), "the inputs are consumed");
    let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
    assert!(names.iter().any(|n| n == "Wall/side(Sketch1/l5)"), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("Lid/cap(start")), "{names:?}");
}

#[test]
fn a_sheet_thickens_into_a_solid_and_open_curves_make_sheets() {
    let (ev, r) =
        eval("patch Plate (profile: Sketch1)\n\nthicken Thick (body: Plate, thickness: 3 mm, both_sides: true)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Plate").unwrap();
    assert!(near(ev.kernel().mass_properties(&b.shape).unwrap().volume.abs(), 2400.0));
    let (ev, r) = eval("surface_extrude Fence (curves: Sketch1.curves(l5, l6), distance: 5 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let f = r.body("Fence").unwrap();
    let m = ev.kernel().mass_properties(&f.shape).unwrap();
    assert!(near(m.area, (40.0 + 20.0) * 5.0), "{}", m.area);
    assert_eq!(f.topology.solids, 0);
}

#[test]
fn a_thickened_sheet_is_never_inside_out() {
    // Each side and both sides: the solid's volume is positive (OCCT's thick solid can come out
    // with its faces turned inward), so it unites with other bodies.
    for (t, both) in [("2 mm", false), ("-2 mm", false), ("2 mm", true), ("-2 mm", true)] {
        let extra = format!(
            "surface_extrude Fence (curves: Sketch1.curves(l5), distance: 5 mm)\n\nthicken Thick (body: Fence, thickness: {t}, both_sides: {both})\n\nextrude Post (profile: Sketch1, distance: 3 mm, op: new)\n\ncombine Join (body: Post, tools: [Fence], op: add)"
        );
        let (ev, r) = eval(&extra);
        assert!(r.errors().is_empty(), "{t} {both}: {:?}", r.errors());
        let post = r.body("Post").unwrap();
        let v = ev.kernel().mass_properties(&post.shape).unwrap().volume;
        // The post (2400) and the wall along its side y = 0 (40 x 5 mm, 2 mm thick): inside the
        // post up to its top (2 mm of the wall's 5 mm height above it), outside it all of it, or
        // half and half.
        let wall = match (t, both) {
            (_, true) => 40.0 * 1.0 * 5.0 + 40.0 * 1.0 * 2.0,
            ("2 mm", false) => 40.0 * 2.0 * 2.0,
            _ => 40.0 * 2.0 * 5.0,
        };
        assert!(near(v, 2400.0 + wall), "{t} {both}: {v}");
    }
}
