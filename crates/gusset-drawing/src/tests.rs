use super::*;
use gusset_doc::Document;
use gusset_eval::Evaluator;
use gusset_kernel::mock::MockKernel;

const PLATE: &str = "gusset 1
units mm

sketch S (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Plate (profile: S, distance: 10 mm, op: new)
";

fn scene(drawing: &str) -> Scene {
    let doc = Document::from_text(&format!("{PLATE}\n{drawing}")).unwrap_or_else(|e| panic!("{e}"));
    let k = Arc::new(MockKernel);
    let ev = Evaluator::new(k.clone()).evaluate(&doc, None, &());
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    let cx = Context { ev: &ev, kernel: k.as_ref(), units: doc.units };
    build(&doc.drawings[0], &cx, &mut HlrCache::default())
}

fn view<'a>(s: &'a Scene, name: &str) -> &'a PlacedView {
    s.views.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("no view {name}: {:?}", s.issues))
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

const SHEET: &str = "drawing D (size: A3, projection: first, title: \"Plate\") {
  view Front (bodies: [Plate], from: front, at: [130, 160])
  view Top (of: Front, side: top)
  view Right (of: Front, side: right, gap: 15 mm)
  view Iso (from: iso, at: [330, 210], scale: 0.5)
  note N (text: \"Break sharp edges\", at: [30, 40])
}
";

#[test]
fn views_are_placed_by_the_projection() {
    let s = scene(SHEET);
    assert!(s.issues.is_empty(), "{:?}", s.issues);
    assert!(close(s.size, [420.0, 297.0]));
    // The front view: 40 x 10 centred at (130, 160).
    let f = view(&s, "Front");
    assert!(close(f.min, [110.0, 155.0]) && close(f.max, [150.0, 165.0]), "{f:?}");
    // First angle: the view of the top goes below, aligned in X, 20 mm apart.
    let t = view(&s, "Top");
    assert!(close(t.min, [110.0, 115.0]) && close(t.max, [150.0, 135.0]), "{t:?}");
    // The view of the right side goes left, aligned in Y, 15 mm apart.
    let r = view(&s, "Right");
    assert!(close(r.min, [75.0, 155.0]) && close(r.max, [95.0, 165.0]), "{r:?}");
    // The top view sees the plate's +Y edge at its top: Y up on the sheet.
    assert!(close(t.to_sheet([0.0, 20.0, 10.0]), [110.0, 135.0]), "{t:?}");
    // Third angle mirrors the placement.
    let s3 = scene(&SHEET.replace("first", "third"));
    let t3 = view(&s3, "Top");
    assert!(close(t3.min, [110.0, 185.0]), "{t3:?}");
    assert!(view(&s3, "Right").min[0] > 150.0);
    // Edges are drawn: the mock projects every edge as visible.
    assert!(s.items.iter().filter(|i| i.style == Style::Visible).count() > 12);
    assert!(s.items.iter().any(|i| matches!(&i.prim, Prim::Text { text, .. } if text == "Break sharp edges")));
    assert!(s.items.iter().any(|i| matches!(&i.prim, Prim::Text { text, .. } if text == "1:1")));
}

#[test]
fn scale_and_gap_follow_parameters_and_bad_values_are_reported() {
    let s = scene(&SHEET.replace("title: \"Plate\"", "scale: 2"));
    let f = view(&s, "Front");
    assert!(close(f.min, [90.0, 150.0]) && close(f.max, [170.0, 170.0]), "{f:?}");
    assert!(s.items.iter().any(|i| matches!(&i.prim, Prim::Text { text, .. } if text == "2:1")));
    let s = scene(&SHEET.replace("gap: 15 mm", "gap: 5 deg"));
    assert!(s.issues.iter().any(|e| e.contains("must be a number or a length")), "{:?}", s.issues);
    assert!(view(&s, "Right").error.is_some());
    assert_eq!(scale_text(0.5), "1:2");
    assert_eq!(scale_text(2.5), "2.5:1");
}

#[test]
fn svg_and_dxf_hold_the_scene() {
    let s = scene(SHEET);
    let svg = svg::to_svg(&s);
    assert!(svg.starts_with("<?xml") && svg.ends_with("</svg>\n"));
    assert!(svg.contains(r#"width="420mm" height="297mm" viewBox="0 0 420 297""#), "{}", &svg[..300]);
    assert!(svg.contains(r#"class="VISIBLE""#) && svg.contains("Break sharp edges"));
    // Y is flipped: the front view's top edge (sheet y 165) is at 297 - 165 = 132.
    assert!(svg.contains(r#"y1="132""#), "no flipped line");
    let dxf = dxf::to_dxf(&s, dxf::Profile::R12);
    // For checking with other readers by hand.
    if let Ok(dir) = std::env::var("GUSSET_DRAWING_OUT") {
        let _ = std::fs::write(format!("{dir}/plate.svg"), &svg);
        let _ = std::fs::write(format!("{dir}/plate_r12.dxf"), &dxf);
        let _ = std::fs::write(format!("{dir}/plate_r2000.dxf"), dxf::to_dxf(&s, dxf::Profile::R2000));
    }
    assert!(dxf.contains("AC1009") && dxf.trim_end().ends_with("EOF"));
    let lines = dxf.matches("\n  0\nLINE\n").count();
    let sheet_lines = s.items.iter().filter(|i| matches!(i.prim, Prim::Line { .. })).count();
    assert_eq!(lines, sheet_lines);
    assert!(dxf.contains("\n  8\nVISIBLE\n") && dxf.contains("\n  2\nHIDDEN\n"));
    // Group codes and values alternate: an even number of lines.
    assert_eq!(dxf.lines().count() % 2, 0);
    // R2000: units, handles, line weights; the same entities.
    let d2 = dxf::to_dxf(&s, dxf::Profile::default());
    assert!(d2.contains("AC1015") && d2.contains("$INSUNITS\n 70\n4\n") && d2.trim_end().ends_with("EOF"));
    assert_eq!(d2.matches("\n  0\nLINE\n").count(), sheet_lines);
    assert_eq!(d2.lines().count() % 2, 0);
    assert!(d2.contains("\n370\n50\n"), "visible lines are 0.5 mm");
    // Non-ASCII text is escaped.
    let s = scene(&SHEET.replace("Break sharp edges", "Ø 5 ±0.1"));
    assert!(dxf::to_dxf(&s, dxf::Profile::R12).contains("\\U+00D8 5 \\U+00B10.1"));
}

#[test]
fn dimensions_are_measured_from_the_model() {
    let e = |l: &str| format!("edge(Plate/cap(end), Plate/side(S/{l}))");
    let sheet = format!(
        "drawing D (size: A3, projection: first) {{
  view Front (bodies: [Plate], from: front, at: [130, 160])
  view Top (of: Front, side: top)
  dim D1 (view: Front, kind: length, a: {l5})
  dim D2 (view: Top, kind: distance, a: {l5}, b: {l7}, tolerance: \"±0.1\")
  dim D3 (view: Top, kind: angle, a: {l5}, b: {l6})
  dim D4 (view: Top, kind: length, a: {l6}, direction: horizontal)
  dim D5 (view: Top, kind: diameter, a: {l6})
}}
",
        l5 = e("l5"),
        l6 = e("l6"),
        l7 = e("l7"),
    );
    let s = scene(&sheet);
    if let Ok(dir) = std::env::var("GUSSET_DRAWING_OUT") {
        let _ = std::fs::write(format!("{dir}/dims.svg"), svg::to_svg(&s));
        let _ = std::fs::write(format!("{dir}/dims_r12.dxf"), dxf::to_dxf(&s, dxf::Profile::R12));
        let _ = std::fs::write(format!("{dir}/dims_r2000.dxf"), dxf::to_dxf(&s, dxf::Profile::R2000));
    }
    let d = |n: &str| s.dims.iter().find(|d| d.name == n).unwrap_or_else(|| panic!("{n}: {:?}", s.dims));
    let near = |n: &str, v: f64| {
        let got = d(n).value.unwrap_or_else(|| panic!("{n}: {:?}", d(n).error));
        assert!((got - v).abs() < 1e-9, "{n}: {got}");
    };
    near("D1", 40.0);
    near("D2", 20.0);
    assert_eq!(d("D2").text, "20 ±0.1");
    near("D3", 90.0);
    assert_eq!(d("D3").text, "90°");
    // A vertical edge measured horizontally is zero long.
    near("D4", 0.0);
    assert!(d("D5").error.as_deref().is_some_and(|e| e.contains("circular")), "{:?}", d("D5"));
    assert_eq!(s.issues.len(), 1, "{:?}", s.issues);
    // Arrows and dimension lines are drawn.
    assert!(s.items.iter().filter(|i| matches!(i.prim, Prim::Fill { .. })).count() >= 6);
    assert!(s.items.iter().any(|i| matches!(&i.prim, Prim::Text { text, .. } if text == "40")));
    // A reference to an edge the model no longer has dangles; the sheet still builds.
    let s = scene(&sheet.replace("S/l5)))\n  dim D2", "S/l9)))\n  dim D2"));
    assert!(s.dims.first().is_some_and(|d| d.error.is_some()), "{:?}", s.dims);
}

#[test]
fn centre_marks_and_centrelines() {
    use gusset_kernel::{Axis3, Circle3, CurveKind, EdgeInfo, FaceInfo, SurfaceKind, Topology};
    // A cylinder of radius 5 along Z from 0 to 10, its end circles, and a quarter-turn
    // fillet arc that gets no mark.
    let doc = Document::from_text(PLATE).unwrap();
    let k = Arc::new(MockKernel);
    let ev = Evaluator::new(k.clone()).evaluate(&doc, None, &());
    let plate = ev.bodies.first().unwrap();
    let circle = |z: f64, r: f64, turn: f64| EdgeInfo {
        kind: CurveKind::Circle,
        length: turn * std::f64::consts::TAU * r,
        midpoint: [r, 0.0, z],
        faces: vec![0],
        vertices: vec![],
        circle: Some(Circle3 { center: [0.0, 0.0, z], normal: [0.0, 0.0, 1.0], radius: r }),
    };
    let face = |kind, axis| FaceInfo { kind, area: 1.0, centroid: [0.0; 3], normal: [1.0, 0.0, 0.0], axis };
    let topology = Topology {
        faces: vec![face(SurfaceKind::Cylinder, Some(Axis3 { origin: [0.0; 3], dir: [0.0, 0.0, 1.0] }))],
        edges: vec![circle(0.0, 5.0, 1.0), circle(10.0, 5.0, 1.0), circle(10.0, 2.0, 0.25)],
        vertices: vec![],
        solids: 1,
        shells: 1,
    };
    let body = Arc::new(gusset_eval::Body {
        name: "Pin".into(),
        feature: "Pin".into(),
        shape: plate.shape.clone(),
        names: Default::default(),
        topology,
        holes: vec![],
    });
    let view = |toward: [f64; 3], x: [f64; 3], y: [f64; 3]| PlacedView {
        name: "V".into(),
        min: [0.0; 2],
        max: [0.0; 2],
        scale: 2.0,
        origin: [100.0, 100.0],
        toward,
        x,
        y,
        error: None,
    };
    let lines = |s: &Scene| -> Vec<([f64; 2], [f64; 2])> {
        s.items
            .iter()
            .filter(|i| i.style == Style::Center)
            .filter_map(|i| match i.prim {
                Prim::Line { a, b } => Some((a, b)),
                _ => None,
            })
            .collect()
    };
    // Seen from the top: one cross for both end circles, 2 x 5 + 2.5 mm each side.
    let mut s = Scene::default();
    crate::centers::draw(&mut s, std::slice::from_ref(&body), &view([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    let l = lines(&s);
    assert_eq!(l.len(), 2, "{l:?}");
    assert!(close(l[0].0, [87.5, 100.0]) && close(l[0].1, [112.5, 100.0]), "{l:?}");
    // From the front: a centreline along the axis, 2.5 mm past each end.
    let mut s = Scene::default();
    crate::centers::draw(&mut s, &[body], &view([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    let l = lines(&s);
    assert_eq!(l.len(), 1, "{l:?}");
    assert!(close(l[0].0, [100.0, 97.5]) && close(l[0].1, [100.0, 122.5]), "{l:?}");
}

#[test]
fn a_raster_of_the_sheet() {
    let s = scene(SHEET);
    let r = raster::render(&s, 840);
    assert_eq!((r.width, r.height), (840, 594));
    assert_eq!(r.rgba.len(), 840 * 594 * 4);
    // The darkest red channel around a sheet point (a line can straddle two pixels).
    let px = |x: f64, y: f64| {
        let (i, j) = ((x * 2.0) as usize, ((297.0 - y) * 2.0) as usize);
        (i - 1..=i + 1)
            .flat_map(|i| (j - 1..=j + 1).map(move |j| (i, j)))
            .map(|(i, j)| r.rgba[(j * 840 + i) * 4])
            .min()
            .unwrap()
    };
    // The frame's left edge at x = 20 mm and the front view's bottom edge at y = 155 mm
    // are ink; the paper between them is white.
    assert!(px(20.0, 150.0) < 100, "frame: {}", px(20.0, 150.0));
    assert!(px(130.0, 155.0) <= 128, "view edge: {}", px(130.0, 155.0));
    assert_eq!(px(60.0, 250.0), 255);
    // Text is drawn: the title block's title area has ink.
    let inked = (0..40)
        .flat_map(|i| (0..10).map(move |j| (i, j)))
        .filter(|&(i, j)| px(235.0 + f64::from(i), 38.0 + f64::from(j)) < 128)
        .count();
    assert!(inked > 10, "{inked} inked pixels in the title");
    if let Ok(dir) = std::env::var("GUSSET_DRAWING_OUT") {
        let _ = std::fs::write(format!("{dir}/plate.png"), r.to_png().unwrap());
    }
}
