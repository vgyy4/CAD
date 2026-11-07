//! 3D surface finishing (ADR 0040 amendment) over a model built with the real kernel.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

#[test]
fn a_flat_end_mill_finishes_a_block_top_at_its_height() {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    let job = json!({"sketch": "Sketch1", "operation": "surface_flat", "tool_diameter": 6.0, "top": 10.0,
        "depth": 10.0, "safe": 15.0, "stepover": 0.5, "path": true});
    let r = run(&mut s, "cam.toolpath", job);
    let pts: Vec<[f64; 3]> = r["path"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|l| l.as_array().unwrap().iter().map(|p| [0, 1, 2].map(|i| p[i].as_f64().unwrap())))
        .collect();
    assert!(!pts.is_empty(), "{r}");
    // Never below the top; the cutting passes run on it (the approach comes down from 1 mm above).
    assert!(pts.iter().all(|p| p[2] >= 10.0 - 1e-3), "{r}");
    let on = pts.iter().filter(|p| (p[2] - 10.0).abs() < 1e-3).count();
    assert!(on * 2 > pts.len(), "{on} of {}", pts.len());
    // Lines 3 mm apart cover the 20 mm width: x runs the length, y steps across.
    let ys: std::collections::BTreeSet<i64> = pts.iter().map(|p| (p[1] * 1000.0).round() as i64).collect();
    assert!(ys.len() >= 7, "{ys:?}");
}

#[test]
fn waterlines_ring_a_boss_at_its_radius_plus_the_tool_radius() {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 60, "height": 60}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_circle", json!({"sketch": "Sketch2", "cx": 30, "cy": 30, "radius": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "15 mm", "op": "add"}));
    let job = json!({"sketch": "Sketch1", "operation": "waterline_flat", "tool_diameter": 6.0, "top": 15.0,
        "depth": 10.0, "stepdown": 2.5, "safe": 20.0, "path": true});
    let r = run(&mut s, "cam.toolpath", job);
    let pts: Vec<[f64; 3]> = r["path"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|l| l.as_array().unwrap().iter().map(|p| [0, 1, 2].map(|i| p[i].as_f64().unwrap())))
        .collect();
    // Above the plate the only wall is the boss: a flat cutter rides 13 mm from its axis.
    let ring: Vec<f64> = pts
        .iter()
        .filter(|p| p[2] > 5.5 && p[2] < 14.9)
        .map(|p| ((p[0] - 30.0).powi(2) + (p[1] - 30.0).powi(2)).sqrt())
        .collect();
    assert!(!ring.is_empty(), "{r}");
    assert!(
        ring.iter().all(|d| (d - 13.0).abs() < 0.05),
        "{:?}",
        ring.iter().fold((f64::MAX, 0.0f64), |a, d| (a.0.min(*d), a.1.max(*d)))
    );
}

#[test]
fn a_short_stickout_lifts_the_tool_away_from_a_tall_boss() {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 60, "height": 60}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_circle", json!({"sketch": "Sketch2", "cx": 30, "cy": 30, "radius": 5}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "30 mm", "op": "add"}));
    let job = |holder: Value| {
        json!({"sketch": "Sketch1", "operation": "surface_flat", "tool_diameter": 6.0, "top": 30.0,
            "depth": 25.0, "safe": 35.0, "stepover": 0.5, "holder": holder})
    };
    // A 40 mm holder 10 mm above the tip hits the 25 mm tall boss around it: lifted, with a warning.
    let r = run(&mut s, "cam.toolpath", job(json!([6.0, 8.0, 40.0, 10.0])));
    let w = r["warnings"].to_string();
    assert!(w.contains("holder:") && w.contains("leaving material"), "{r}");
    // With 40 mm stickout nothing touches.
    let r = run(&mut s, "cam.toolpath", job(json!([6.0, 35.0, 40.0, 40.0])));
    assert!(!r["warnings"].to_string().contains("holder:"), "{r}");
    // A 2D operation says the holder is not checked.
    let r = run(
        &mut s,
        "cam.toolpath",
        json!({"sketch": "Sketch1", "operation": "pocket", "tool_diameter": 6.0, "depth": 1.0,
            "holder": [6.0, 8.0, 40.0, 10.0]}),
    );
    assert!(r["warnings"].to_string().contains("only for 3D"), "{r}");
}
