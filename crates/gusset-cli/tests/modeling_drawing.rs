//! Drawings with the real OCCT kernel (ADR 0032): exact hidden-line views placed by the
//! projection, hidden edges of a through hole, measured dimensions, centre marks and
//! centrelines, and the exports. Mock-kernel coverage lives in gusset-drawing and
//! gusset-commands.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp, clippy::disallowed_methods)]

use gusset_doc::Document;
use gusset_drawing::{Context, HlrCache, Prim, Scene, Style};
use gusset_eval::Evaluator;
use gusset_kernel::CurveKind;
use std::sync::Arc;

/// A 60 x 40 x 20 plate with a 10 mm hole through it.
const PART: &str = "gusset 1
units mm

sketch S1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point(60, 40)
  p4 = point(0, 40)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude E1 (profile: S1, distance: 20 mm, op: new)

sketch S2 (plane: XY, next_id: 3) {
  p1 = point(30, 20)
  c2 = circle(p1, 5)
}

extrude H (profile: S2, distance: 20 mm, op: cut, body: E1)
";

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn count(s: &Scene, style: Style) -> usize {
    s.items.iter().filter(|i| i.style == style).count()
}

#[test]
fn a_dimensioned_sheet_of_a_plate_with_a_hole() {
    let k = kernel();
    // The name of the hole's top circle, from the evaluated body.
    let doc = Document::from_text(PART).unwrap_or_else(|e| panic!("{e}"));
    let ev = Evaluator::new(k.clone()).evaluate(&doc, None, &());
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    let b = ev.body("E1").unwrap();
    let hole = (0..b.topology.edges.len())
        .find(|&i| {
            let e = &b.topology.edges[i];
            e.kind == CurveKind::Circle && e.circle.is_some_and(|c| (c.center[2] - 20.0).abs() < 1e-9)
        })
        .map(|i| b.names.edges[i].to_string())
        .expect("the hole's top circle");
    let front_edge = "edge(E1/cap(end), E1/side(S1/l5))";
    let drawing = format!(
        "drawing D (size: A3, projection: first, scale: 2, title: \"Plate\") {{
  view Front (bodies: [E1], from: front, at: [200, 200])
  view Top (of: Front, side: top)
  view Right (of: Front, side: right)
  view Iso (from: iso, at: [330, 210], scale: 1)
  dim W (view: Front, kind: length, a: {front_edge})
  dim T (view: Right, kind: distance, a: edge(E1/cap(start), E1/side(S1/l6)), b: edge(E1/cap(end), E1/side(S1/l6)))
  dim Dia (view: Top, kind: diameter, a: {hole})
}}
"
    );
    let doc = Document::from_text(&format!("{PART}\n{drawing}")).unwrap_or_else(|e| panic!("{e}"));
    let ev = Evaluator::new(k.clone()).evaluate(&doc, None, &());
    let cx = Context { ev: &ev, kernel: k.as_ref(), units: doc.units, cancel: None };
    let mut cache = HlrCache::default();
    let t0 = std::time::Instant::now();
    let s = gusset_drawing::build(&doc.drawings[0], &cx, &mut cache);
    let first = t0.elapsed();
    assert!(
        s.issues.is_empty(),
        "{:?} {:?}",
        s.issues,
        s.views.iter().map(|v| (&v.name, v.min, v.max)).collect::<Vec<_>>()
    );
    let view = |n: &str| s.views.iter().find(|v| v.name == n).unwrap();
    let size = |n: &str| {
        let v = view(n);
        [v.max[0] - v.min[0], v.max[1] - v.min[1]]
    };
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    // 2:1: the front view is 120 x 40, the top 120 x 80 below it, the right 80 x 40 left of it.
    assert!(near(size("Front"), [120.0, 40.0]), "{:?}", view("Front"));
    assert!(near(size("Top"), [120.0, 80.0]), "{:?}", view("Top"));
    assert!(near(size("Right"), [80.0, 40.0]), "{:?}", view("Right"));
    assert!((view("Top").max[1] - (view("Front").min[1] - 20.0)).abs() < 1e-6);
    assert!((view("Right").max[0] - (view("Front").min[0] - 20.0)).abs() < 1e-6);
    // The hole is hidden in the front and right views.
    assert!(count(&s, Style::Hidden) >= 4, "{} hidden", count(&s, Style::Hidden));
    // Dimensions measured from the model.
    let dim = |n: &str| s.dims.iter().find(|d| d.name == n).unwrap();
    assert_eq!((dim("W").value, dim("W").text.as_str()), (Some(60.0), "60"));
    assert_eq!(dim("T").value, Some(20.0), "{:?}", dim("T"));
    assert_eq!((dim("Dia").value, dim("Dia").text.as_str()), (Some(10.0), "Ø10"));
    // A centre mark in the top view (2 lines) and the iso view sees the hole at an angle;
    // centrelines of the hole in the front and right views.
    assert!(count(&s, Style::Center) >= 4, "{} centre lines", count(&s, Style::Center));
    // Exports: SVG and both DXF profiles hold every line.
    let lines = s.items.iter().filter(|i| matches!(i.prim, Prim::Line { .. })).count();
    for p in [gusset_drawing::dxf::Profile::R2000, gusset_drawing::dxf::Profile::R12] {
        let d = gusset_drawing::dxf::to_dxf(&s, p);
        assert_eq!(d.matches("\n  0\nLINE\n").count(), lines);
    }
    let svg = gusset_drawing::svg::to_svg(&s);
    if let Ok(dir) = std::env::var("GUSSET_DRAWING_OUT") {
        std::fs::write(format!("{dir}/occt_plate.svg"), &svg).unwrap();
        std::fs::write(format!("{dir}/occt_plate.dxf"), gusset_drawing::dxf::to_dxf(&s, Default::default())).unwrap();
    }
    // A second build reuses the projections (counted, not timed: review N6).
    let misses = cache.misses();
    assert!(misses > 0);
    let t1 = std::time::Instant::now();
    let again = gusset_drawing::build(&doc.drawings[0], &cx, &mut cache);
    let second = t1.elapsed();
    assert_eq!(again, s);
    eprintln!("sheet built in {first:?}, again in {second:?}");
    assert_eq!(cache.misses(), misses, "the cache is used");
}

/// Hatched area of a section view: hatch length times spacing (sheet mm², at 1:1).
fn hatched(s: &Scene) -> f64 {
    s.items
        .iter()
        .filter(|i| i.style == Style::Thin)
        .filter_map(|i| match i.prim {
            Prim::Line { a, b } => Some(libm::hypot(b[0] - a[0], b[1] - a[1])),
            _ => None,
        })
        .sum::<f64>()
        * gusset_drawing::section::HATCH_SPACING
}

#[test]
fn section_views_cut_the_part_and_hatch_the_cut() {
    let k = kernel();
    let build = |views: &str| {
        let text = format!(
            "{PART}\ndrawing D (size: A3, projection: first) {{\n  view Front (bodies: [E1], from: front, at: [150, 220])\n{views}}}\n"
        );
        let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}"));
        let ev = Evaluator::new(k.clone()).evaluate(&doc, None, &());
        let cx = Context { ev: &ev, kernel: k.as_ref(), units: doc.units, cancel: None };
        gusset_drawing::build(&doc.drawings[0], &cx, &mut HlrCache::default())
    };
    let base = build("");
    // Thin lines of the sheet itself (title block) are not hatching.
    let hatched = |s: &Scene| hatched(s) - hatched(&base);
    // Through the middle of the plate's thickness (z = 10): the plate with its hole.
    let s = build("  view A (of: Front, side: top, section: A)\n");
    if let Ok(dir) = std::env::var("GUSSET_DRAWING_OUT") {
        std::fs::write(format!("{dir}/section.png"), gusset_drawing::raster::render(&s, 1400).to_png().unwrap())
            .unwrap();
    }
    assert!(s.issues.is_empty(), "{:?}", s.issues);
    let a = 60.0 * 40.0 - std::f64::consts::PI * 25.0;
    assert!((hatched(&s) - a).abs() < 0.03 * a, "hatched {} of {a}", hatched(&s));
    // The label under the view and the cutting plane (letters at both ends) in the parent.
    let texts: Vec<&str> = s
        .items
        .iter()
        .filter_map(|i| if let Prim::Text { text, .. } = &i.prim { Some(text.as_str()) } else { None })
        .collect();
    assert!(texts.contains(&"A–A") && texts.iter().filter(|t| **t == "A").count() == 2, "{texts:?}");
    assert!(count(&s, Style::Center) > count(&base, Style::Center));
    // A vertical cut through the hole's axis (x = 30): two 15 x 20 strips beside the hole.
    let s = build("  view B (of: Front, side: right, section: B, cut_at: 30 mm)\n");
    assert!(s.issues.is_empty(), "{:?}", s.issues);
    let a = 2.0 * 15.0 * 20.0;
    assert!((hatched(&s) - a).abs() < 0.03 * a, "hatched {} of {a}", hatched(&s));
    // A detail of the section keeps its hatching (review D8): a circle around the whole
    // cut, at 1:1, doubles the hatched area.
    let two = build(
        "  view B (of: Front, side: right, section: B, cut_at: 30 mm)\n  view DB (detail_of: B, center: [0, 0], radius: 80, at: [320, 190], scale: 1)\n",
    );
    assert!(two.issues.is_empty(), "{:?}", two.issues);
    assert!((hatched(&two) - 2.0 * a).abs() < 0.06 * a, "hatched {} of {}", hatched(&two), 2.0 * a);
    // A plane beyond the part leaves nothing: reported, not a crash.
    let s = build("  view C (of: Front, side: top, section: C, cut_at: -5 mm)\n");
    assert!(s.issues.iter().any(|i| i.contains("nothing")), "{:?}", s.issues);
}
