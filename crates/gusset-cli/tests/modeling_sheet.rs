//! Sheet metal (ADR 0037) with the real OCCT kernel: a U channel bent from three lines,
//! its exact volume, its face names and its flat pattern (at k = 0.5 the flat pattern's
//! area times the thickness is the volume).

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{Evaluation, Evaluator};
use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

const SKETCH: &str = "gusset 1
units mm

sketch S (plane: XZ, next_id: 10) {
  p1 = point(0, 30)
  p2 = point(0, 0)
  p3 = point(60, 0)
  p4 = point(60, 30)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
}
";

fn eval(extra: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(&format!("{SKETCH}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn a_u_channel_has_its_exact_volume_names_and_flat_pattern() {
    for flip in [false, true] {
        let (ev, r) = eval(&format!(
            "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5{})",
            if flip { ", flip: true" } else { "" }
        ));
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        let b = r.body("U").unwrap();
        let m = ev.kernel().mass_properties(&b.shape).unwrap();
        // Two right-angle bends; the sketch side is the inside (r) or the outside (r + t).
        let (t, rad, w) = (2.0, 3.0, 40.0);
        let ra = if flip { rad } else { rad + t };
        // Travelling down l5 then along l6 turns left; the material (left) is inside.
        let straights = (30.0 - ra) + (60.0 - 2.0 * ra) + (30.0 - ra);
        let bend_area = 2.0 * FRAC_PI_2 * t * (rad + t / 2.0);
        let volume = w * (t * straights + bend_area);
        assert!(near(m.volume, volume), "flip {flip}: {} vs {volume}", m.volume);
        let flat = b.sheet.as_ref().unwrap();
        assert_eq!(flat.bends.len(), 2);
        let size = flat.size();
        assert!(near(size[0] * size[1] * t, volume), "flat {size:?}");
        let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
        for want in ["U/flat(l6)", "U/flat_back(l6)", "U/bend_in(l5)", "U/bend_out(l6)", "U/start(l5)", "U/end(l7)"] {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
        assert!(names.iter().all(|n| !n.contains('#')), "no ambiguous names: {names:?}");
    }
}

#[test]
fn bends_too_large_for_their_lines_are_refused() {
    let (_, r) = eval(
        "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 30 mm, k_factor: 0.44)",
    );
    let errs: Vec<String> = r.errors().iter().map(|e| e.message.clone()).collect();
    assert!(errs.iter().any(|e| e.contains("too short")), "{errs:?}");
}

const TAB: &str = "gusset 1
units mm

sketch T (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point(60, 40)
  p4 = point(0, 40)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

tab Plate (profile: T, thickness: 2 mm)
";

#[test]
fn edge_flanges_on_a_tab_have_exact_volumes_and_grow_the_flat_pattern() {
    for down in [false, true] {
        let f = if down { ", flip: true" } else { "" };
        let src = format!(
            "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5{f})\n\n\
             edge_flange F2 (body: Plate, line: l8, length: 10 mm, angle: 45 deg, bend_radius: 2 mm, k_factor: 0.5{f})\n"
        );
        let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.to_text(), src);
        let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
        let mut ev = Evaluator::new(k);
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        assert_eq!(r.bodies.len(), 1, "the flanges join the tab");
        let b = r.body("Plate").unwrap();
        let t = 2.0;
        let flange = |len: f64, th: f64, rad: f64, l: f64| l * (th * t * (rad + t / 2.0) + len * t);
        let volume = 60.0 * 40.0 * t + flange(15.0, FRAC_PI_2, 3.0, 40.0) + flange(10.0, FRAC_PI_2 / 2.0, 2.0, 40.0);
        let m = ev.kernel().mass_properties(&b.shape).unwrap();
        assert!(near(m.volume, volume), "down {down}: {} vs {volume}", m.volume);
        let flat = b.sheet.as_ref().unwrap();
        assert_eq!(flat.bends.len(), 2);
        let size = flat.size();
        let width = 60.0 + FRAC_PI_2 * (3.0 + 1.0) + 15.0 + FRAC_PI_2 / 2.0 * (2.0 + 1.0) + 10.0;
        assert!(near(size[0], width) && near(size[1], 40.0), "{size:?}");
        assert!(near(size[0] * size[1] * t, volume), "at k = 0.5 the flat area times t is the volume");
        let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
        for want in
            ["F1/bend_in(l6)", "F1/bend_out(l6)", "F1/flat(l6)", "F1/end(l6)", "F2/flat_back(l8)", "Plate/cap(end)"]
        {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
    }
    // A line inside the outline, or one already flanged, is refused.
    let src = format!(
        "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5)\n\n\
         edge_flange F2 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5)\n"
    );
    let (_, r) = {
        let doc = gusset_doc::Document::from_text(&src).unwrap();
        let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
        let mut ev = Evaluator::new(k);
        let r = ev.evaluate(&doc, None, &());
        (ev, r)
    };
    assert!(r.errors().iter().any(|e| e.feature == "F2" && e.message.contains("outline")), "{:?}", r.errors());
}
