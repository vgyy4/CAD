//! Region arrangement (crossing curves) and trim with the real OCCT kernel: exact volumes,
//! persistent names across parameter edits. Mock-kernel coverage lives in gusset-commands.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::f64::consts::PI;
use std::sync::Arc;

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

struct Rig {
    s: Session,
    u: CmdCtx,
}

impl Rig {
    fn new() -> Rig {
        let d = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
        Rig { s: Session::new(&d, kernel(), &CmdCtx::user(0)), u: CmdCtx::user(1) }
    }

    fn run(&mut self, id: &str, args: Value) -> Value {
        execute(&mut self.s, &self.u, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
    }

    /// Region ids with their areas, smallest first.
    fn regions(&mut self, sketch: &str) -> Vec<(String, f64)> {
        let q = self.run("query.sketch", json!({"sketch": sketch}));
        let mut v: Vec<(String, f64)> = q["regions"]
            .as_array()
            .unwrap_or_else(|| panic!("{q}"))
            .iter()
            .map(|r| (r["id"].as_str().unwrap().to_string(), r["area_mm2"].as_f64().unwrap()))
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    fn volume(&mut self, body: &str) -> f64 {
        self.run("query.body", json!({"body": body}))["volume_mm3"].as_f64().unwrap()
    }

    fn face_names(&mut self, body: &str) -> Vec<String> {
        let b = self.run("query.body", json!({"body": body, "names": true}));
        let mut v: Vec<String> =
            b["face_names"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap().to_string()).collect();
        v.sort();
        v
    }
}

#[test]
fn overlapping_rectangles_extrude_region_by_region() {
    let mut r = Rig::new();
    r.run("param.set", json!({"name": "h", "expr": "6 mm"}));
    r.run("sketch.create", json!({"plane": "XY"}));
    r.run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 20, "height": 10, "width_expr": "20 mm", "height_expr": "10 mm"}),
    );
    r.run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 10, "y": 5, "width": 20, "height": 10, "width_expr": "20 mm", "height_expr": "10 mm"}),
    );
    let regs = r.regions("Sketch1");
    assert_eq!(regs.len(), 3, "{regs:?}");
    for ((id, area), want) in regs.iter().zip([50.0, 150.0, 150.0]) {
        assert!(near(*area, want), "{regs:?}");
        let name = format!("Part_{id}");
        let e =
            r.run("feature.extrude", json!({"sketch": "Sketch1", "regions": [id], "distance": "h", "body_name": name}));
        assert_eq!(e["rebuild_ok"], true, "{e}");
        assert!(near(r.volume(&name), want * 6.0), "{id}: {}", r.volume(&name));
    }
    // All regions at once: one body per sketch extrusion covering the union (350 mm2).
    let e = r.run("feature.extrude", json!({"sketch": "Sketch1", "distance": "h", "body_name": "All"}));
    assert_eq!(e["rebuild_ok"], true, "{e}");
    assert!(near(r.volume("All"), 350.0 * 6.0), "{}", r.volume("All"));
}

#[test]
fn a_line_through_a_circle_gives_two_half_discs_with_stable_names() {
    let mut r = Rig::new();
    r.run("param.set", json!({"name": "rr", "expr": "10 mm"}));
    r.run("sketch.create", json!({"plane": "XY"}));
    r.run("sketch.add_circle", json!({"sketch": "Sketch1", "cx": 0, "cy": 0, "radius": 10, "radius_expr": "rr"}));
    r.run("sketch.add_line", json!({"sketch": "Sketch1", "x1": -20, "y1": 0, "x2": 20, "y2": 0}));
    let regs = r.regions("Sketch1");
    assert_eq!(regs.len(), 2, "{regs:?}");
    assert!(regs.iter().all(|(_, a)| near(*a, 50.0 * PI)), "{regs:?}");
    let q = r.run("query.sketch", json!({"sketch": "Sketch1"}));
    assert!(q["open_ends"].to_string().contains("l"), "the line's outer stubs are reported: {q}");
    let (first, second) = (regs[0].0.clone(), regs[1].0.clone());
    assert_ne!(first, second);
    // Extrude the half named `first` and keep the faces' names across a radius change.
    let e = r.run(
        "feature.extrude",
        json!({"sketch": "Sketch1", "regions": [first.clone()], "distance": "4 mm", "body_name": "Half"}),
    );
    assert_eq!(e["rebuild_ok"], true, "{e}");
    assert!(near(r.volume("Half"), 50.0 * PI * 4.0), "{}", r.volume("Half"));
    let names = r.face_names("Half");
    assert_eq!(names.len(), 4, "{names:?}");
    let w = r.run("param.set", json!({"name": "rr", "expr": "12 mm"}));
    assert_eq!(w["rebuild_ok"], true, "{w}");
    assert!(near(r.volume("Half"), 72.0 * PI * 4.0), "{}", r.volume("Half"));
    assert_eq!(r.face_names("Half"), names, "face names are stable");
    // The same id still names the same half.
    let after = r.regions("Sketch1");
    assert!(after.iter().any(|(id, a)| *id == first && near(*a, 72.0 * PI)), "{after:?}");
}

#[test]
fn trim_builds_a_rectangle_with_a_round_bump() {
    let mut r = Rig::new();
    r.run("sketch.create", json!({"plane": "XY"}));
    r.run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20, "width_expr": "40 mm", "height_expr": "20 mm"}),
    );
    // A circle centred on the right edge (l6 = p2 -> p3): half inside, half outside.
    let c = r.run("sketch.add_circle", json!({"sketch": "Sketch1", "cx": 40, "cy": 10, "radius": 8}));
    let circle = c["circle"].as_str().unwrap().to_string();
    assert_eq!(r.regions("Sketch1").len(), 3);
    // Trim away the arc inside the rectangle and the edge piece inside the circle.
    let t = r.run("sketch.trim", json!({"sketch": "Sketch1", "entity": circle, "x": 32, "y": 10}));
    assert_eq!(t["deleted"].as_array().unwrap().len(), 1, "{t}");
    let t = r.run("sketch.trim", json!({"sketch": "Sketch1", "entity": "l6", "x": 40, "y": 10}));
    assert_eq!(t["kept"].as_array().unwrap().len(), 2, "{t}");
    let regs = r.regions("Sketch1");
    assert_eq!(regs.len(), 1, "{regs:?}");
    assert!(near(regs[0].1, 800.0 + 32.0 * PI), "{regs:?}");
    let e = r.run("feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm"}));
    assert_eq!(e["rebuild_ok"], true, "{e}");
    assert!(near(r.volume("Extrude1"), (800.0 + 32.0 * PI) * 5.0), "{}", r.volume("Extrude1"));
    // Undo the extrude and the second trim: the rectangle and the bump are separate regions again.
    r.run("history.undo", json!({}));
    r.run("history.undo", json!({}));
    assert_eq!(r.regions("Sketch1").len(), 2);
}

/// Area of the polygon through many samples of the exact spline (the "high-resolution
/// estimate" the solid is compared with).
fn spline_area(pts: &[[f64; 2]], closed: bool) -> f64 {
    let poly = gusset_sketch::spline_polyline(pts, closed, 4000);
    let n = poly.len();
    0.5 * (0..n).map(|i| poly[i][0] * poly[(i + 1) % n][1] - poly[(i + 1) % n][0] * poly[i][1]).sum::<f64>()
}

fn ring(n: u32, r: f64, c: [f64; 2]) -> Vec<[f64; 2]> {
    (0..n)
        .map(|k| {
            let t = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect()
}

#[test]
fn a_closed_spline_profile_extrudes_to_the_volume_of_its_curve() {
    let mut r = Rig::new();
    r.run("sketch.create", json!({"plane": "XY"}));
    let pts = ring(12, 10.0, [0.0, 0.0]);
    let sp = r.run("sketch.add_polyline", json!({"sketch": "Sketch1", "points": pts, "spline": true, "closed": true}));
    let spline = sp["spline"].as_str().unwrap().to_string();
    let e = r.run("feature.extrude", json!({"sketch": "Sketch1", "distance": "7 mm"}));
    assert_eq!(e["rebuild_ok"], true, "{e}");
    let want = spline_area(&pts, true) * 7.0;
    let got = r.volume("Extrude1");
    eprintln!(
        "spline golden: kernel volume {got:.6} mm3, polygon estimate {want:.6} mm3, circle {:.6}",
        PI * 100.0 * 7.0
    );
    assert!((got - want).abs() < 1e-5 * want, "{got} vs {want}");
    // Close to the circle through the same points (the spline bulges slightly).
    assert!((got - PI * 100.0 * 7.0).abs() < 0.003 * PI * 100.0 * 7.0, "{got}");
    // One lateral face per Bézier segment (segment k runs from fit point k+1), plus the caps.
    let names = r.face_names("Extrude1");
    assert_eq!(names.len(), 12 + 2, "{names:?}");
    for k in 0..12 {
        assert!(names.contains(&format!("Extrude1/side(Sketch1/{spline}_g{k})")), "{k}: {names:?}");
    }
    // Moving a fit point reshapes the solid and keeps the names.
    r.run("sketch.move_point", json!({"sketch": "Sketch1", "point": "p4", "x": 0.0, "y": 13.0}));
    let got2 = r.volume("Extrude1");
    assert!(got2 > got, "{got2} > {got}");
    // Recompute the estimate from the document's points.
    let q = r.run("query.sketch", json!({"sketch": "Sketch1"}));
    let fit: Vec<[f64; 2]> = (1..=12)
        .map(|k| {
            let p = &q["points"][format!("p{k}")];
            [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]
        })
        .collect();
    let want2 = spline_area(&fit, true) * 7.0;
    assert!((got2 - want2).abs() < 1e-5 * want2, "{got2} vs {want2}");
    assert_eq!(r.face_names("Extrude1"), names);
}

#[test]
fn a_spline_hole_in_a_plate_and_an_open_spline_across_a_rectangle() {
    let mut r = Rig::new();
    r.run("sketch.create", json!({"plane": "XY"}));
    r.run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": -20, "y": -15, "width": 40, "height": 30, "width_expr": "40 mm", "height_expr": "30 mm"}),
    );
    let pts = ring(8, 6.0, [0.0, 0.0]);
    r.run("sketch.add_polyline", json!({"sketch": "Sketch1", "points": pts, "spline": true, "closed": true}));
    let regs = r.regions("Sketch1");
    assert_eq!(regs.len(), 2, "{regs:?}");
    let hole = spline_area(&pts, true);
    let plate = regs.iter().find(|(_, a)| *a > 1000.0).unwrap().clone();
    assert!((plate.1 - (1200.0 - hole)).abs() < 1e-5 * 1200.0, "{regs:?}");
    let e = r.run(
        "feature.extrude",
        json!({"sketch": "Sketch1", "regions": [plate.0], "distance": "3 mm", "body_name": "Plate"}),
    );
    assert_eq!(e["rebuild_ok"], true, "{e}");
    let got = r.volume("Plate");
    assert!((got - (1200.0 - hole) * 3.0).abs() < 1e-5 * 3600.0, "{got} vs {}", (1200.0 - hole) * 3.0);
    // An open spline across a second sketch's rectangle splits it into two solids' worth of regions.
    r.run("sketch.create", json!({"plane": "XY"}));
    r.run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch2", "x": 0, "y": 0, "width": 40, "height": 20, "width_expr": "40 mm", "height_expr": "20 mm"}),
    );
    r.run(
        "sketch.add_polyline",
        json!({"sketch": "Sketch2", "points": [[-10, 10], [10, 5], [25, 15], [50, 10]], "spline": true}),
    );
    let regs2 = r.regions("Sketch2");
    assert_eq!(regs2.len(), 2, "{regs2:?}");
    assert!((regs2.iter().map(|x| x.1).sum::<f64>() - 800.0).abs() < 1e-6, "{regs2:?}");
    let mut total = 0.0;
    for (i, (id, area)) in regs2.iter().enumerate() {
        let name = format!("Half{i}");
        let e = r.run(
            "feature.extrude",
            json!({"sketch": "Sketch2", "regions": [id], "distance": "2 mm", "body_name": name}),
        );
        assert_eq!(e["rebuild_ok"], true, "{e}");
        let v = r.volume(&name);
        assert!((v - area * 2.0).abs() < 1e-5 * area * 2.0, "{v} vs {}", area * 2.0);
        total += v;
    }
    assert!((total - 1600.0).abs() < 1e-3, "{total}");
}

#[test]
fn sketch_patterns_drill_a_plate_and_follow_parameters() {
    let mut r = Rig::new();
    r.run("param.set", json!({"name": "holes", "expr": "4"}));
    r.run("param.set", json!({"name": "pitch", "expr": "11 mm"}));
    r.run("sketch.create", json!({"plane": "XY"}));
    r.run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 80, "height": 60, "width_expr": "80 mm", "height_expr": "60 mm"}),
    );
    let c = r.run("sketch.add_circle", json!({"sketch": "Sketch1", "cx": 12, "cy": 12, "radius": 4}));
    let circle = c["circle"].as_str().unwrap().to_string();
    let p = r.run(
        "sketch.pattern",
        json!({"sketch": "Sketch1", "kind": "linear", "entities": [circle], "count": "holes", "dx": "pitch"}),
    );
    assert_eq!(p["sketch_status"]["regions"].as_array().unwrap().len(), 5, "{p}");
    let plate_id = |r: &mut Rig| {
        let regs = r.regions("Sketch1");
        regs.iter().find(|(_, a)| *a > 1000.0).unwrap().0.clone()
    };
    let id = plate_id(&mut r);
    let e = r.run(
        "feature.extrude",
        json!({"sketch": "Sketch1", "regions": [id], "distance": "5 mm", "body_name": "Plate"}),
    );
    assert_eq!(e["rebuild_ok"], true, "{e}");
    assert!(near(r.volume("Plate"), (4800.0 - 4.0 * PI * 16.0) * 5.0), "{}", r.volume("Plate"));
    let names = r.face_names("Plate");
    for k in 1..4 {
        assert!(names.contains(&format!("Extrude1/side(Sketch1/{circle}_{k})")), "{k}: {names:?}");
    }
    // More holes, wider pitch: the solid follows and the copies keep their names.
    r.run("param.set", json!({"name": "holes", "expr": "6"}));
    r.run("param.set", json!({"name": "pitch", "expr": "12 mm"}));
    assert!(near(r.volume("Plate"), (4800.0 - 6.0 * PI * 16.0) * 5.0), "{}", r.volume("Plate"));
    assert!(r.face_names("Plate").contains(&format!("Extrude1/side(Sketch1/{circle}_5)")));
    // A bolt circle: 5 holes round the plate's centre (a construction point), 270 degrees.
    r.run("sketch.create", json!({"plane": "XY"}));
    r.run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch2", "x": -30, "y": -30, "width": 60, "height": 60, "width_expr": "60 mm", "height_expr": "60 mm"}),
    );
    let hub = r.run("sketch.add_circle", json!({"sketch": "Sketch2", "cx": 20, "cy": 0, "radius": 3}));
    let hole = hub["circle"].as_str().unwrap().to_string();
    let centre = r
        .run("sketch.add_line", json!({"sketch": "Sketch2", "x1": 0, "y1": 0, "x2": 1, "y2": 0, "construction": true}));
    let origin = centre["points"][0].as_str().unwrap().to_string();
    r.run(
        "sketch.pattern",
        json!({"sketch": "Sketch2", "kind": "circular", "entities": [hole], "count": "5", "center": origin, "angle": "270 deg"}),
    );
    let regs = r.regions("Sketch2");
    assert_eq!(regs.len(), 6, "{regs:?}");
    let id2 = regs.iter().find(|(_, a)| *a > 1000.0).unwrap().0.clone();
    let e = r.run(
        "feature.extrude",
        json!({"sketch": "Sketch2", "regions": [id2], "distance": "2 mm", "body_name": "Flange"}),
    );
    assert_eq!(e["rebuild_ok"], true, "{e}");
    assert!(near(r.volume("Flange"), (3600.0 - 5.0 * PI * 9.0) * 2.0), "{}", r.volume("Flange"));
    // The last hole sits 270 deg round: at (0, -20) (a partial angle spans first to last copy).
    let q = r.run("query.sketch", json!({"sketch": "Sketch2"}));
    let hub_centre = hub["center"].as_str().unwrap().to_string();
    let last = &q["points"][format!("{hub_centre}_4")];
    assert!(last[0].as_f64().unwrap().abs() < 1e-9 && (last[1].as_f64().unwrap() + 20.0).abs() < 1e-9, "{q}");
}
