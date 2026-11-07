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

#[test]
fn a_hole_through_a_bend_is_developed_with_it() {
    let base = "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)";
    // A short vertical hole at the corner between l6 and l7: in through the bend's outside,
    // out through its inside, clear of the walls.
    let (_, r) = eval(&format!(
        "{base}

sketch C (plane: XY, next_id: 3) {{
  p1 = point(56.5, -20)
  c2 = circle(p1, 1)
}}

extrude Cut (profile: C, distance: 8 mm, symmetric: true, op: cut, body: U)"
    ));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("U").unwrap();
    let flat = b.sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    // Developed from its outline on the bend's sketch side (the outside, radius 5, centre
    // (55, 5)): x = 55.5 and 57.5 lie at asin(0.1) and asin(0.5) round from the bend's start,
    // each angle a share of the quarter turn along its 2π mm zone (π/2 · (3 + 1)); the zone
    // starts after l5's straight (25), its bend (2π) and l6's straight (50).
    let x0 = 25.0 + FRAC_PI_2 * 4.0 + 50.0;
    let (lo, hi) = (x0 + libm::asin(0.5 / 4.0) * 4.0, x0 + libm::asin(2.5 / 4.0) * 4.0);
    let h = &flat.outlines[1];
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in h {
        (xmin, xmax, ymin, ymax) = (xmin.min(p[0]), xmax.max(p[0]), ymin.min(p[1]), ymax.max(p[1]));
    }
    assert!((xmin - lo).abs() < 0.05 && (xmax - hi).abs() < 0.05, "{xmin}..{xmax} vs {lo}..{hi}");
    assert!((ymin - 19.0).abs() < 0.05 && (ymax - 21.0).abs() < 0.05, "{ymin}..{ymax}");
    let z = &flat.bends[1];
    let (x0, x1) = (z.start[0][0], z.end[0][0]);
    assert!(flat.outlines[1].iter().all(|p| p[0] > x0 - 2.0 && p[0] < x1 + 2.0), "{x0} {x1} {:?}", flat.outlines[1]);
}

#[test]
fn a_hole_through_an_edge_flanges_bend_is_developed_with_it() {
    let src = format!(
        "{TAB}
edge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5)

sketch C (plane: XY, next_id: 3) {{
  p1 = point(61.5, 20)
  c2 = circle(p1, 1)
}}

extrude Cut (profile: C, distance: 8 mm, symmetric: true, op: cut, body: Plate)
"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("Plate").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    // The bend's outer face: radius 5 about (60, z 5); x = 60.5 and 62.5 lie at asin(0.1)
    // and asin(0.5) round it, along its 2π mm zone beyond the tab's edge at x = 60.
    let (lo, hi) = (60.0 + libm::asin(0.5 / 4.0) * 4.0, 60.0 + libm::asin(2.5 / 4.0) * 4.0);
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &flat.outlines[1] {
        (xmin, xmax, ymin, ymax) = (xmin.min(p[0]), xmax.max(p[0]), ymin.min(p[1]), ymax.max(p[1]));
    }
    assert!((xmin - lo).abs() < 0.05 && (xmax - hi).abs() < 0.05, "{xmin}..{xmax} vs {lo}..{hi}");
    assert!((ymin - 19.0).abs() < 0.05 && (ymax - 21.0).abs() < 0.05, "{ymin}..{ymax}");
}

#[test]
fn a_closed_corner_butts_the_walls_and_keeps_a_relief() {
    let area = |o: &Vec<[f64; 2]>| {
        let n = o.len();
        (0..n).map(|i| o[i][0] * o[(i + 1) % n][1] - o[(i + 1) % n][0] * o[i][1]).sum::<f64>().abs() / 2.0
    };
    for f in ["", ", flip: true"] {
        let src = format!(
            "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5{f})\n\n\
             edge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5, close: true{f})\n"
        );
        let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.to_text(), src);
        let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
        let mut ev = Evaluator::new(k);
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        let b = r.body("Plate").unwrap();
        let t = 2.0;
        let flange = |len: f64, l: f64| l * (FRAC_PI_2 * t * (3.0 + t / 2.0) + len * t);
        // F2's wall runs on past the corner by F1's radius and thickness (5), F1's by F2's
        // radius (3), up to F2's inside.
        let volume = 60.0 * 40.0 * t + flange(15.0, 40.0) + flange(15.0, 60.0) + 15.0 * t * 5.0 + 15.0 * t * 3.0;
        let m = ev.kernel().mass_properties(&b.shape).unwrap();
        assert!(near(m.volume, volume), "{f}: {} vs {volume}", m.volume);
        let flat = b.sheet.as_ref().unwrap();
        assert_eq!(flat.outlines.len(), 1);
        assert!(near(area(&flat.outlines[0]) * t, volume), "{f}: flat {} vs {volume}", area(&flat.outlines[0]) * t);
    }
    // Mitred: the walls meet on the corner's diagonal, 0.1 mm apart. Each wedge runs (x from
    // 3 to 5) past the end up to s = x - c, c = 0.1 / sqrt 2: 15 (8 - 2c); diagonal faces show
    // the mitre, and the flat flaps are as wide as the wedges at the neutral layer.
    for f in ["", ", flip: true"] {
        let src = format!(
            "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5{f})\n\n\
             edge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5, close: mitre{f})\n"
        );
        let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.to_text(), src);
        let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
        let mut ev = Evaluator::new(k);
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        let b = r.body("Plate").unwrap();
        let t = 2.0;
        let flange = |len: f64, l: f64| l * (FRAC_PI_2 * t * (3.0 + t / 2.0) + len * t);
        let c = 0.1 / std::f64::consts::SQRT_2;
        let volume = 60.0 * 40.0 * t + flange(15.0, 40.0) + flange(15.0, 60.0) + 2.0 * 15.0 * (8.0 - 2.0 * c);
        let m = ev.kernel().mass_properties(&b.shape).unwrap();
        assert!(near(m.volume, volume), "{f}: {} vs {volume}", m.volume);
        let diag = b.topology.faces.iter().filter(|fc| {
            let n = fc.normal;
            (n[0].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6
                && (n[1].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6
        });
        assert_eq!(diag.count(), 2, "{f}: the mitre's two faces");
        let flat = b.sheet.as_ref().unwrap();
        assert_eq!(flat.outlines.len(), 1);
        assert!(near(area(&flat.outlines[0]) * t, volume), "{f}: flat {} vs {volume}", area(&flat.outlines[0]) * t);
    }
    // Nothing to close with: refused.
    let src = format!(
        "{TAB}\nedge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5, close: true)\n"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let r = Evaluator::new(k).evaluate(&doc, None, &());
    assert!(r.errors().iter().any(|e| e.to_string().contains("outside corner")), "{:?}", r.errors());
}

#[test]
fn a_corner_between_60_degree_flanges_closes_on_the_mirror_plane() {
    // Two 60 degree flanges (r 3, t 2, length 15, k 0.5) at the tab's corner: each wall runs
    // on to the plane that mirrors it into the other. A wedge holds the wall's section
    // (t x len) swept up to the section point's distance from the line:
    // t len ((r + t/2) sin 60 + len/2 cos 60); a mitre takes c t len more (c = 0.1 / sqrt 2).
    let area = |o: &Vec<[f64; 2]>| {
        let n = o.len();
        (0..n).map(|i| o[i][0] * o[(i + 1) % n][1] - o[(i + 1) % n][0] * o[i][1]).sum::<f64>().abs() / 2.0
    };
    let (t, r, len) = (2.0, 3.0, 15.0);
    let th = 60f64.to_radians();
    let flange = |l: f64| l * (th * t * (r + t / 2.0) + len * t);
    let wedge = t * len * ((r + t / 2.0) * libm::sin(th) + len / 2.0 * libm::cos(th));
    for (close, cut) in [("true", 0.0), ("mitre", 0.1 / std::f64::consts::SQRT_2)] {
        for f in ["", ", flip: true"] {
            let src = format!(
                "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 60 deg, bend_radius: 3 mm, k_factor: 0.5{f})\n\n\
                 edge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 60 deg, bend_radius: 3 mm, k_factor: 0.5, close: {close}{f})\n"
            );
            let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
            let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
            let mut ev = Evaluator::new(k);
            let r0 = ev.evaluate(&doc, None, &());
            assert!(r0.errors().is_empty(), "{close}{f}: {:?}", r0.errors());
            let b = r0.body("Plate").unwrap();
            let volume = 60.0 * 40.0 * t + flange(40.0) + flange(60.0) + 2.0 * (wedge - cut * t * len);
            let m = ev.kernel().mass_properties(&b.shape).unwrap();
            assert!(near(m.volume, volume), "{close}{f}: {} vs {volume}", m.volume);
            let flat = b.sheet.as_ref().unwrap();
            assert_eq!(flat.outlines.len(), 1);
            assert!(
                near(area(&flat.outlines[0]) * t, volume),
                "{close}{f}: flat {} vs {volume}",
                area(&flat.outlines[0]) * t
            );
        }
    }
}

#[test]
fn a_corner_between_120_degree_flanges_is_trimmed_on_the_mirror_plane() {
    // Flanges leaning back over the tab (r 3, t 2, length 15, k 0.5): the walls cross above the
    // corner, so each is cut off at the mirror plane. The solid holds the tab, both flanges
    // and, per wall, the signed wedge t len ((r + t/2) sin 120 + len/2 cos 120) (negative: the
    // wall is trimmed), a mitre taking c t len off (c = 0.1 / sqrt 2).
    let area = |o: &Vec<[f64; 2]>| {
        let n = o.len();
        (0..n).map(|i| o[i][0] * o[(i + 1) % n][1] - o[(i + 1) % n][0] * o[i][1]).sum::<f64>().abs() / 2.0
    };
    let (t, r, len) = (2.0, 3.0, 15.0);
    let th = 120f64.to_radians();
    let flange = |l: f64| l * (th * t * (r + t / 2.0) + len * t);
    let wedge = t * len * ((r + t / 2.0) * libm::sin(th) + len / 2.0 * libm::cos(th));
    for (close, cut) in [("true", 0.0), ("mitre", 0.1 / std::f64::consts::SQRT_2)] {
        for f in ["", ", flip: true"] {
            let src = format!(
                "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 120 deg, bend_radius: 3 mm, k_factor: 0.5{f})\n\n\
                 edge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 120 deg, bend_radius: 3 mm, k_factor: 0.5, close: {close}{f})\n"
            );
            let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
            let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
            let mut ev = Evaluator::new(k);
            let r0 = ev.evaluate(&doc, None, &());
            assert!(r0.errors().is_empty(), "{close}{f}: {:?}", r0.errors());
            let b = r0.body("Plate").unwrap();
            let volume = 60.0 * 40.0 * t + flange(40.0) + flange(60.0) + 2.0 * (wedge - cut * t * len);
            let m = ev.kernel().mass_properties(&b.shape).unwrap();
            assert!(near(m.volume, volume), "{close}{f}: {} vs {volume}", m.volume);
            let flat = b.sheet.as_ref().unwrap();
            assert_eq!(flat.outlines.len(), 1);
            assert!(
                near(area(&flat.outlines[0]) * t, volume),
                "{close}{f}: flat {} vs {volume}",
                area(&flat.outlines[0]) * t
            );
        }
    }
}

#[test]
fn corners_that_are_not_square_close_too() {
    // Dogfooding F8-3: tab corners of 120 and 60 degrees. With k 0.5 the flat pattern holds
    // exactly the solid's volume (the flaps are the wedges at mid-thickness).
    let area = |o: &Vec<[f64; 2]>| {
        let n = o.len();
        (0..n).map(|i| o[i][0] * o[(i + 1) % n][1] - o[(i + 1) % n][0] * o[i][1]).sum::<f64>().abs() / 2.0
    };
    for (p3x, label) in [(70.0, "120"), (50.0, "60")] {
        for close in ["true", "mitre"] {
            let src = format!(
                "gusset 1
units mm

sketch T (plane: XY, next_id: 9) {{
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point({p3x}, 17.320508075688775)
  p4 = point(0, 17.320508075688775)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

tab Plate (profile: T, thickness: 2 mm)

edge_flange F1 (body: Plate, line: l5, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5)

edge_flange F2 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5, close: {close})
"
            );
            let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
            let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
            let mut ev = Evaluator::new(k);
            let r = ev.evaluate(&doc, None, &());
            assert!(r.errors().is_empty(), "{label} {close}: {:?}", r.errors());
            let b = r.body("Plate").unwrap();
            let m = ev.kernel().mass_properties(&b.shape).unwrap();
            let flat = b.sheet.as_ref().unwrap();
            assert_eq!(flat.outlines.len(), 1, "{label} {close}");
            let f = area(&flat.outlines[0]) * 2.0;
            assert!((f - m.volume).abs() < 1e-6 * m.volume, "{label} {close}: flat {f} vs {}", m.volume);
        }
    }
}

#[test]
fn flanges_past_a_right_angle_meeting_at_a_corner_must_be_closed() {
    // Dogfooding F9-5: left open, two 120 degree walls run into each other over the corner.
    let src = |close: &str| {
        format!(
            "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 120 deg, bend_radius: 3 mm, k_factor: 0.5)\n\n\
             edge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 120 deg, bend_radius: 3 mm, k_factor: 0.5{close})\n"
        )
    };
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let open = gusset_doc::Document::from_text(&src("")).unwrap();
    let r = Evaluator::new(k.clone()).evaluate(&open, None, &());
    assert!(r.errors().iter().any(|e| e.to_string().contains("run into each other")), "{:?}", r.errors());
    let closed = gusset_doc::Document::from_text(&src(", close: true")).unwrap();
    let r = Evaluator::new(k).evaluate(&closed, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
}

#[test]
fn cuts_unfold_with_several_flanges_and_are_listed_once() {
    // Two flanges with a closed corner (their end caps are sheet faces, not the cut's), then
    // two holes in two features: three outlines, the plate and each hole once.
    let src = format!(
        "{TAB}
edge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5)

edge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5, close: true)

sketch C (plane: XY, next_id: 3) {{
  p1 = point(20, 20)
  c2 = circle(p1, 4)
}}

extrude Cut (profile: C, distance: 20 mm, symmetric: true, op: cut, body: Plate)

sketch D (plane: XY, next_id: 3) {{
  p1 = point(40, 25)
  c2 = circle(p1, 3)
}}

extrude Cut2 (profile: D, distance: 20 mm, symmetric: true, op: cut, body: Plate)
"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let r = Evaluator::new(k).evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("Plate").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 3, "{:?}", flat.outlines.iter().map(Vec::len).collect::<Vec<_>>());
    // Each hole is one circle (F7-1: earlier holes were listed again by later cuts).
    assert_eq!(flat.circles.len(), 2, "{:?}", flat.circles);
}

#[test]
fn a_hole_after_a_cut_across_a_bend_still_unfolds() {
    // Dogfooding F7-2: the cut across the bend's walls were taken for the hole's.
    let base = "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)";
    let (_, r) = eval(&format!(
        "{base}

sketch C (plane: XY, next_id: 9) {{
  p1 = point(55.5, -22)
  p2 = point(57.5, -22)
  p3 = point(57.5, -18)
  p4 = point(55.5, -18)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

extrude Cut (profile: C, distance: 8 mm, symmetric: true, op: cut, body: U)

sketch D (plane: XY, next_id: 3) {{
  p1 = point(30, -20)
  c2 = circle(p1, 3)
}}

extrude Hole (profile: D, distance: 8 mm, symmetric: true, op: cut, body: U)"
    ));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("U").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 3);
    assert_eq!(flat.circles.len(), 1, "{:?}", flat.circles);
    assert!((flat.circles[0].radius - 3.0).abs() < 1e-6);
}

#[test]
fn a_cut_across_a_bend_is_as_wide_as_the_neutral_arc() {
    let base = "contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)";
    // A rectangular hole over the l6/l7 bend (centre (55, 5), outside radius 5, inside 3, neutral
    // 4), x from 55.5 to 57.5, cut straight down: its walls cross the neutral surface at
    // asin(0.5 / 4) and asin(2.5 / 4) from the bend's start.
    let (_, r) = eval(&format!(
        "{base}

sketch C (plane: XY, next_id: 9) {{
  p1 = point(55.5, -22)
  p2 = point(57.5, -22)
  p3 = point(57.5, -18)
  p4 = point(55.5, -18)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

extrude Cut (profile: C, distance: 8 mm, symmetric: true, op: cut, body: U)"
    ));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("U").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    let h = &flat.outlines[1];
    let (xmin, xmax) = h.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let x0 = 25.0 + FRAC_PI_2 * 4.0 + 50.0;
    let (lo, hi) = (x0 + libm::asin(0.5 / 4.0) * 4.0, x0 + libm::asin(2.5 / 4.0) * 4.0);
    // (Mesh vertices are single precision: 1e-4 is as close as they allow.)
    assert!((xmin - lo).abs() < 1e-4 && (xmax - hi).abs() < 1e-4, "{xmin}..{xmax} vs {lo}..{hi}");
    assert!(((xmax - xmin) - 4.0 * (libm::asin(0.625) - libm::asin(0.125))).abs() < 1e-4);
    assert!(flat.circles.is_empty());
}

/// Area of the polygon `o` (shoelace).
fn poly_area(o: &[[f64; 2]]) -> f64 {
    let n = o.len();
    (0..n).map(|i| o[i][0] * o[(i + 1) % n][1] - o[(i + 1) % n][0] * o[i][1]).sum::<f64>().abs() / 2.0
}

/// The `flat_pattern` query and the DXF export of `src`'s sheet.
fn dxf_of(src: &str) -> (String, serde_json::Value) {
    use gusset_commands::{CmdCtx, Session, execute};
    let doc = gusset_doc::Document::from_text(src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let dir = std::env::temp_dir().join(format!("gusset-sheet-straddle-{}-{}", std::process::id(), src.len()));
    std::fs::create_dir_all(&dir).unwrap();
    s.project_dir = Some(dir.clone());
    let u = CmdCtx::user(1);
    let q = execute(&mut s, &u, "query.flat_pattern", serde_json::json!({})).unwrap();
    execute(&mut s, &u, "sheet.export_flat", serde_json::json!({"path": "flat.dxf"})).unwrap();
    let dxf = std::fs::read_to_string(dir.join("flat.dxf")).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    (dxf, q)
}

#[test]
fn a_cut_over_a_straight_and_its_bend_is_unfolded_as_one_outline() {
    // F8-7: x from 53 to 57 over the l6 straight (to x = 55) and the l6/l7 bend (centre
    // (55, 5), neutral radius 4), cut straight down. Flat: 2 mm of straight plus the neutral
    // arc to asin(2 / 4).
    let src = format!(
        "{SKETCH}
contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)

sketch C (plane: XY, next_id: 9) {{
  p1 = point(53, -22)
  p2 = point(57, -22)
  p3 = point(57, -18)
  p4 = point(53, -18)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

extrude Cut (profile: C, distance: 8 mm, symmetric: true, op: cut, body: U)
"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("U").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    let h = &flat.outlines[1];
    let (xmin, xmax) = h.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let x0 = 25.0 + FRAC_PI_2 * 4.0 + 50.0;
    let arc = 4.0 * libm::asin(0.5);
    assert!((xmin - (x0 - 2.0)).abs() < 1e-4 && (xmax - (x0 + arc)).abs() < 1e-4, "{xmin}..{xmax} vs {x0}");
    assert!((poly_area(h) - 4.0 * (2.0 + arc)).abs() < 1e-3, "{}", poly_area(h));
    let (dxf, q) = dxf_of(&src);
    assert_eq!(q["cutouts"].as_array().unwrap().len(), 1, "{q}");
    assert!(dxf.contains("LINE"), "{dxf}");
}

#[test]
fn a_cut_straight_down_over_a_bend_and_its_wall_is_unfolded_as_one_outline() {
    // z from 3 to 8, x from 57 to 61, y 4 wide: it takes the l7 wall from its bend (z = 5) up
    // to z = 8 and the bend from z = 3 round to its end. On the neutral arc (radius 4, centre
    // (55, 5)) z = 3 lies at acos(2 / 4) = 60 deg: the outline is the rest of the quarter arc
    // plus 3 mm of straight.
    let src = format!(
        "{SKETCH}
contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 3 mm, k_factor: 0.5)

plane P (base: XY, offset: 3 mm)

sketch C (plane: P, next_id: 9) {{
  p1 = point(57, -22)
  p2 = point(61, -22)
  p3 = point(61, -18)
  p4 = point(57, -18)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

extrude Cut (profile: C, distance: 5 mm, op: cut, body: U)
"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("U").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    let h = &flat.outlines[1];
    let (xmin, xmax) = h.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let x0 = 25.0 + FRAC_PI_2 * 4.0 + 50.0;
    // The floor at z = 3 is developed along the cut (z) onto the neutral surface, so its
    // flat edge is not at the neutral arc's 60 deg: only the wall end is exact.
    let hi = x0 + FRAC_PI_2 * 4.0 + 3.0;
    assert!((xmax - hi).abs() < 1e-4 && xmin > x0 + 4.0 && xmin < x0 + 5.0, "{xmin}..{xmax}");
    let (dxf, q) = dxf_of(&src);
    assert_eq!(q["cutouts"].as_array().unwrap().len(), 1, "{q}");
    assert!(dxf.contains("LINE"), "{dxf}");
}

#[test]
fn a_hole_through_a_bend_and_its_wall_is_unfolded_as_one_outline() {
    // F8-7: d5 along x, centred on the bend/wall line z = 5 (z 2.5 .. 7.5); the wall is at
    // x 58 .. 60, the bend (centre (55, 5)) turns into it.
    let src = format!(
        "{SKETCH}
contour_flange U (curves: S.curves(l5, l6, l7), width: 40 mm, thickness: 2 mm, bend_radius: 2 mm, k_factor: 0.44)

plane P (base: YZ, offset: 56 mm)

sketch C (plane: P, next_id: 3) {{
  p1 = point(-20, 4)
  c2 = circle(p1, 2.5)
}}

extrude Cut (profile: C, distance: 4 mm, op: cut, body: U)
"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("U").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    let h = &flat.outlines[1];
    let (xmin, xmax) = h.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    // Bend and wall meet at 26 + BA + 52 + BA (BA = pi/2 (2 + 0.44 * 2)); the wall half is 2.5
    // long; the bend half is the neutral arc (radius 2.88) from z = 1.5 (where the circle
    // starts) to the bend's end at z = 4.
    let (rn, ba) = (2.88, FRAC_PI_2 * 2.88);
    let wall = 26.0 + ba + 52.0 + ba;
    let arc = rn * (FRAC_PI_2 - libm::acos(2.5 / rn));
    assert!((xmax - (wall + 2.5)).abs() < 1e-4 && (xmin - (wall - arc)).abs() < 2e-3, "{xmin}..{xmax} vs {wall} {arc}");
    // Area: the wall's half disc plus the bend half, a chord 2 sqrt(6.25 - w^2) over the arc
    // length dw / sqrt(1 - w^2 / rn^2) (midpoint rule).
    let n = 20000;
    let bend: f64 = (0..n)
        .map(|i| {
            let w = (f64::from(i) + 0.5) * 2.5 / f64::from(n);
            2.0 * libm::sqrt(6.25 - w * w) / libm::sqrt(1.0 - w * w / (rn * rn)) * 2.5 / f64::from(n)
        })
        .sum();
    let want = std::f64::consts::PI * 6.25 / 2.0 + bend;
    assert!((poly_area(h) - want).abs() < 0.01 * want, "{} vs {want}", poly_area(h));
    assert!(flat.circles.is_empty());
    let (dxf, q) = dxf_of(&src);
    assert_eq!(q["cutouts"].as_array().unwrap().len(), 1, "{q}");
    assert!(dxf.contains("LINE"), "{dxf}");
}

#[test]
fn a_round_hole_over_a_tab_and_a_flange_bend_is_unfolded() {
    // Centred on the tab's edge x = 60 where the flange's bend starts (outside radius 5,
    // neutral 4): half a disc on the tab, and on the bend the disc stretched to the neutral arc.
    let src = format!(
        "{TAB}
edge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: 3 mm, k_factor: 0.5)

sketch C (plane: XY, next_id: 3) {{
  p1 = point(60, 20)
  c2 = circle(p1, 2)
}}

extrude Cut (profile: C, distance: 8 mm, symmetric: true, op: cut, body: Plate)
"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let flat = r.body("Plate").unwrap().sheet.clone().unwrap();
    assert!(flat.changed_by.is_empty(), "{:?}", flat.changed_by);
    assert_eq!(flat.outlines.len(), 2);
    let h = &flat.outlines[1];
    let (xmin, xmax) = h.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let arc = 4.0 * libm::asin(0.5);
    assert!((xmin - 58.0).abs() < 1e-3 && (xmax - (60.0 + arc)).abs() < 1e-3, "{xmin}..{xmax}");
    // Area: the tab half disc plus the integral of the chord 2 sqrt(4 - u^2) over the arc
    // length ds = du / sqrt(1 - u^2 / 16) (midpoint rule).
    let n = 20000;
    let bend: f64 = (0..n)
        .map(|i| {
            let u = (f64::from(i) + 0.5) * 2.0 / f64::from(n);
            2.0 * libm::sqrt(4.0 - u * u) / libm::sqrt(1.0 - u * u / 16.0) * 2.0 / f64::from(n)
        })
        .sum();
    let want = std::f64::consts::PI * 2.0 + bend;
    assert!((poly_area(h) - want).abs() < 0.02 * want, "{} vs {want}", poly_area(h));
    let (dxf, q) = dxf_of(&src);
    assert_eq!(q["cutouts"].as_array().unwrap().len(), 1, "{q}");
    assert!(dxf.contains("LINE"), "{dxf}");
}

#[test]
fn a_round_hole_stays_a_circle_in_the_flat_pattern_query_and_dxf() {
    use gusset_commands::{CmdCtx, Session, execute};
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
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let dir = std::env::temp_dir().join(format!("gusset-sheet-circle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.project_dir = Some(dir.clone());
    let u = CmdCtx::user(1);
    let q = execute(&mut s, &u, "query.flat_pattern", serde_json::json!({})).unwrap();
    let cut = &q["cutouts"][0];
    assert_eq!(q["cutouts"].as_array().unwrap().len(), 1, "{q}");
    assert_eq!(cut["circle"]["radius_mm"], 5.0, "{q}");
    assert_eq!(cut["circle"]["centre_mm"], serde_json::json!([30.0, 20.0]), "{q}");
    execute(&mut s, &u, "sheet.export_flat", serde_json::json!({"path": "flat.dxf"})).unwrap();
    let dxf = std::fs::read_to_string(dir.join("flat.dxf")).unwrap();
    assert_eq!(dxf.matches("CIRCLE").count(), 1, "{dxf}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_mitre_on_a_bend_radius_below_the_half_gap_keeps_its_diagonal() {
    // Review R7: with r = 0.05 the wedge under the diagonal y = x - c starts where it
    // meets y = 0 (x = c), a triangle of legs r + t - c; the walls still stay 0.1 apart.
    let r0 = 0.05;
    let src = format!(
        "{TAB}\nedge_flange F1 (body: Plate, line: l6, length: 15 mm, angle: 90 deg, bend_radius: {r0} mm, k_factor: 0.5)\n\n\
         edge_flange F2 (body: Plate, line: l5, length: 15 mm, angle: 90 deg, bend_radius: {r0} mm, k_factor: 0.5, close: mitre)\n"
    );
    let doc = gusset_doc::Document::from_text(&src).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let b = r.body("Plate").unwrap();
    let t = 2.0;
    let flange = |len: f64, l: f64| l * (FRAC_PI_2 * t * (r0 + t / 2.0) + len * t);
    let c = 0.1 / std::f64::consts::SQRT_2;
    let leg = r0 + t - c;
    let volume = 60.0 * 40.0 * t + flange(15.0, 40.0) + flange(15.0, 60.0) + 2.0 * 15.0 * leg * leg / 2.0;
    let m = ev.kernel().mass_properties(&b.shape).unwrap();
    assert!(near(m.volume, volume), "{} vs {volume}", m.volume);
    let diag = b.topology.faces.iter().filter(|fc| {
        let n = fc.normal;
        (n[0].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6
            && (n[1].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6
    });
    assert_eq!(diag.count(), 2, "the mitre's two faces");
}
