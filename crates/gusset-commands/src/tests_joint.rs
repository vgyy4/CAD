//! Joint commands through the registry (mock kernel, ADR 0026).

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0))
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

/// A 40 x 20 x 10 plate and a 10 x 10 x 5 block 100 mm to the right.
fn plate_and_block(s: &mut Session) {
    run(s, "param.set", json!({"name": "slide", "expr": "3 mm"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm", "body_name": "Plate"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 100, "y": 0, "width": 10, "height": 10}));
    run(s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "5 mm", "body_name": "Block"}));
}

#[test]
fn a_slider_joint_places_the_block_and_query_joints_reports_it() {
    let mut s = session();
    plate_and_block(&mut s);
    let r = run(
        &mut s,
        "feature.joint",
        json!({"type": "slider", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(end)",
               "flip": true, "distance": "slide", "distance_limits": ["0 mm", "10 mm"]}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let q = run(&mut s, "query.joints", json!({}));
    let j = &q["joints"][0];
    assert_eq!(j["name"], "Joint1", "{q}");
    assert_eq!(j["type"], "slider");
    assert_eq!(j["dof"], 1);
    assert_eq!(j["at"], "Extrude2/cap(start)");
    assert_eq!(j["values"]["distance"], "3 mm", "{q}");
    assert_eq!(j["distance_limits"], json!(["0 mm", "10 mm"]));
    assert_eq!(j["status"]["state"], "ok", "{q}");
    // The block's bottom center goes onto the plate's top center (20, 10, 10), then the
    // slide moves it 3 mm along the flipped Z: down into the plate.
    let c = j["body_centroid_mm"].as_array().unwrap();
    let want = [20.0, 10.0, 10.0 - 3.0 + 2.5];
    assert!(c.iter().zip(want).all(|(a, b)| (a.as_f64().unwrap() - b).abs() < 1e-9), "{c:?}");
    assert_eq!(q["unplaced_bodies"], json!(["Plate"]));
    // Out of the limits: the joint fails, the query says so.
    run(&mut s, "param.set", json!({"name": "slide", "expr": "12 mm"}));
    let q = run(&mut s, "query.joints", json!({}));
    assert_ne!(q["joints"][0]["status"]["state"], "ok", "{q}");
}
