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
