//! Region arrangement (crossing curves) and trim with the real OCCT kernel: exact volumes,
//! persistent names across parameter edits. Mock-kernel coverage lives in gusset-commands.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

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
