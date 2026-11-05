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

#[test]
fn flanges_on_adjacent_lines_leave_an_open_corner() {
    let src = format!(
        "{TAB}\nedge_flange F1 (body: Plate, line: l5, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)\n\n\
         edge_flange F2 (body: Plate, line: l6, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)\n"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Plate").unwrap();
    let t = 2.0;
    let flange = |len: f64, rad: f64, l: f64| l * (FRAC_PI_2 * t * (rad + t / 2.0) + len * t);
    // No overlap: the two flanges' volumes add up exactly.
    let volume = 60.0 * 40.0 * t + flange(10.0, 2.0, 60.0) + flange(10.0, 2.0, 40.0);
    let m = ev.kernel().mass_properties(&b.shape).unwrap();
    assert!(near(m.volume, volume), "{} vs {volume}", m.volume);
    assert_eq!(b.sheet.as_ref().unwrap().bends.len(), 2);
}

#[test]
fn flanges_meeting_at_an_inside_corner_are_refused() {
    // An L: the inside corner is at (20, 20), between l11 (along x) and l12 (along y).
    let src = "gusset 1
units mm

sketch L (plane: XY, next_id: 13) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(20, 20)
  p5 = point(20, 40)
  p6 = point(0, 40)
  l7 = line(p1, p2)
  l8 = line(p2, p3)
  l9 = line(p3, p4)
  l10 = line(p4, p5)
  l11 = line(p5, p6)
  l12 = line(p6, p1)
}

tab Plate (profile: L, thickness: 2 mm)

edge_flange F1 (body: Plate, line: l9, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)

edge_flange F2 (body: Plate, line: l10, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)
";
    let doc = gusset_doc::Document::from_text(src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    let errs: Vec<String> = r.errors().iter().map(|e| format!("{}: {}", e.feature, e.message)).collect();
    assert!(
        errs.iter().any(|e| e.starts_with("F2") && (e.contains("run into") || e.contains("covers it"))),
        "{errs:?}"
    );
    assert!(!errs.iter().any(|e| e.starts_with("F1")), "{errs:?}");
}

#[test]
fn a_hem_folds_back_over_the_tab() {
    let src = format!(
        "{TAB}\nedge_flange Hem (body: Plate, line: l6, length: 8 mm, angle: 180 deg, bend_radius: 1 mm, k_factor: 0.5)\n"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Plate").unwrap();
    let t = 2.0;
    let volume = 60.0 * 40.0 * t + 40.0 * (std::f64::consts::PI * t * (1.0 + t / 2.0) + 8.0 * t);
    let m = ev.kernel().mass_properties(&b.shape).unwrap();
    assert!(near(m.volume, volume), "{} vs {volume}", m.volume);
    let bb = ev.kernel().bbox(&b.shape).unwrap();
    assert!(near(bb.max[2], 2.0 * t + 2.0), "the hem lies one bend gap above the tab: {:?}", bb.max);
    let flat = b.sheet.as_ref().unwrap();
    assert!(near(flat.size()[0], 60.0 + std::f64::consts::PI * 2.0 + 8.0));
}

#[test]
fn edge_flanges_stand_off_a_contour_flanges_sides() {
    let area = |o: &Vec<[f64; 2]>| {
        let n = o.len();
        (0..n).map(|i| o[i][0] * o[(i + 1) % n][1] - o[(i + 1) % n][0] * o[i][1]).sum::<f64>().abs() / 2.0
    };
    for flip in [false, true] {
        let f = if flip { ", flip: true" } else { "" };
        let (ev, r) = eval(&format!(
            "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)\n\n\
             edge_flange F1 (body: U, line: l6_start, length: 12 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5{f})\n\n\
             edge_flange F2 (body: U, line: l5_end, length: 8 mm, angle: 60 deg, bend_radius: 1 mm, k_factor: 0.5{f})"
        ));
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        assert_eq!(r.bodies.len(), 1, "the flanges join the channel");
        let b = r.body("U").unwrap();
        let (t, rad, w) = (2.0, 3.0, 40.0);
        let ra = rad + t;
        let (s5, s6, s7) = (30.0 - ra, 60.0 - 2.0 * ra, 30.0 - ra);
        let channel = w * (t * (s5 + s6 + s7) + 2.0 * FRAC_PI_2 * t * (rad + t / 2.0));
        let flange = |len: f64, th: f64, r: f64, l: f64| l * (th * t * (r + t / 2.0) + len * t);
        let volume = channel + flange(12.0, FRAC_PI_2, 2.0, s6) + flange(8.0, FRAC_PI_2 * 2.0 / 3.0, 1.0, s5);
        let m = ev.kernel().mass_properties(&b.shape).unwrap();
        assert!(near(m.volume, volume), "flip {flip}: {} vs {volume}", m.volume);
        let flat = b.sheet.as_ref().unwrap();
        assert_eq!(flat.bends.len(), 4);
        assert!(near(area(&flat.outlines[0]) * t, volume), "at k = 0.5 the flat area times t is the volume");
        let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
        for want in ["F1/flat(l6_start)", "F1/bend_in(l6_start)", "F2/end(l5_end)"] {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
    }
    // Neighbouring sides would meet in the bend; an unknown side is named.
    let (_, r) = eval(
        "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)\n\n\
         edge_flange F1 (body: U, line: l6_start, length: 12 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)\n\n\
         edge_flange F2 (body: U, line: l5_start, length: 12 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)\n\n\
         edge_flange F3 (body: U, line: l6, length: 12 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)",
    );
    let errs: Vec<String> = r.errors().iter().map(|e| e.to_string()).collect();
    assert!(errs.iter().any(|e| e.contains("collide")) && errs.iter().any(|e| e.contains("names no side")), "{errs:?}");
}

#[test]
fn a_flange_bends_off_another_flanges_far_edge() {
    let area = |o: &Vec<[f64; 2]>| {
        let n = o.len();
        (0..n).map(|i| o[i][0] * o[(i + 1) % n][1] - o[(i + 1) % n][0] * o[i][1]).sum::<f64>().abs() / 2.0
    };
    for (down, down2) in [(false, false), (false, true), (true, false)] {
        let f1 = if down { ", flip: true" } else { "" };
        let f2 = if down2 { ", flip: true" } else { "" };
        let src = format!(
            "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5{f1})\n\n\
             edge_flange F2 (body: Plate, line: F1_end, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5{f2})\n"
        );
        let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
        let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
        let mut ev = Evaluator::new(k);
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        assert_eq!(r.bodies.len(), 1);
        let b = r.body("Plate").unwrap();
        let t = 2.0;
        let flange = |len: f64, th: f64, rad: f64, l: f64| l * (th * t * (rad + t / 2.0) + len * t);
        let volume = 60.0 * 40.0 * t + flange(15.0, FRAC_PI_2, 3.0, 40.0) + flange(10.0, FRAC_PI_2, 2.0, 40.0);
        let m = ev.kernel().mass_properties(&b.shape).unwrap();
        assert!(near(m.volume, volume), "{down} {down2}: {} vs {volume}", m.volume);
        let flat = b.sheet.as_ref().unwrap();
        assert_eq!(flat.bends.len(), 2);
        assert!(near(area(&flat.outlines[0]) * t, volume), "at k = 0.5 the flat area times t is the volume");
        let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
        assert!(names.iter().any(|n| n == "F2/flat(F1_end)"), "{names:?}");
        let bb = ev.kernel().bbox(&b.shape).unwrap();
        // Up then up folds back over the tab; flipping the second sends it outward; a
        // downward first flange mirrors the whole below the tab.
        let want = match (down, down2) {
            (false, false) => ([65.0, 24.0], 0.0),
            (false, true) => ([77.0, 24.0], 0.0),
            _ => ([65.0, 2.0], -22.0),
        };
        assert!(
            near(bb.max[0], want.0[0]) && near(bb.max[2], want.0[1]) && near(bb.min[2], want.1),
            "{down} {down2}: {bb:?}"
        );
    }
    // Twice off the same end is refused.
    let src = format!(
        "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5)\n\n\
         edge_flange F2 (body: Plate, line: F1_end, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)\n\n\
         edge_flange F3 (body: Plate, line: F1_end, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.5)\n"
    );
    let (_, r) = {
        let doc = gusset_doc::Document::from_text(&src).unwrap();
        let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
        let mut ev = Evaluator::new(k);
        let r = ev.evaluate(&doc, None, &());
        (ev, r)
    };
    assert!(r.errors().iter().any(|e| e.to_string().contains("already has a flange")), "{:?}", r.errors());
}

#[test]
fn a_hole_cut_through_a_flat_is_unfolded_into_the_pattern() {
    let src = format!(
        "{TAB}
sketch C (plane: XY, next_id: 3) {{
  p1 = point(30, 20)
  c2 = circle(p1, 5)
}}

extrude Cut (profile: C, distance: 20 mm, symmetric: true, op: cut, body: Plate)
"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("Plate").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2, "the plate and the hole");
    let hole = &flat.outlines[1];
    let r_mean =
        hole.iter().map(|p| ((p[0] - 30.0).powi(2) + (p[1] - 20.0).powi(2)).sqrt()).sum::<f64>() / hole.len() as f64;
    assert!((r_mean - 5.0).abs() < 1e-3 && hole.len() >= 8, "{r_mean} {}", hole.len());
    // A cut across the plate's edge (a notch) changes the outline: still marked.
    let notch = src.replace("point(30, 20)", "point(60, 20)");
    let doc = gusset_doc::Document::from_text(&notch).unwrap();
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.body("Plate").unwrap().sheet.as_ref().unwrap().changed_by, ["Cut"]);
}

#[test]
fn a_hole_through_a_contour_flanges_straight_lands_where_it_develops() {
    let (_, r) = eval(
        "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)

sketch C (plane: XY, next_id: 3) {
  p1 = point(30, -20)
  c2 = circle(p1, 4)
}

extrude Cut (profile: C, distance: 20 mm, symmetric: true, op: cut, body: U)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("U").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    // l5's straight (30 - 5) and its bend (π/2 · (3 + 1)), then 25 mm along l6.
    let cx = 25.0 + FRAC_PI_2 * 4.0 + 25.0;
    let hole = &flat.outlines[1];
    let (mx, my) = (
        hole.iter().map(|p| p[0]).sum::<f64>() / hole.len() as f64,
        hole.iter().map(|p| p[1]).sum::<f64>() / hole.len() as f64,
    );
    assert!((mx - cx).abs() < 0.05 && (my - 20.0).abs() < 0.05, "{mx} {my} vs {cx} 20");
}
