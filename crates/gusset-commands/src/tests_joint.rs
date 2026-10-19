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
    // The result shows the frames as placed (F4-3/F4-4): the plate's top centre, Z flipped
    // down, and the block's bottom 3 mm below it (the overlap warning needs a kernel with
    // booleans: modeling_joint.rs).
    assert_eq!(r["to_frame"]["origin_mm"], json!([20.0, 10.0, 10.0]), "{r}");
    assert_eq!(r["to_frame"]["z_axis"], json!([0.0, 0.0, -1.0]), "{r}");
    assert_eq!(r["at_frame"]["origin_mm"], json!([20.0, 10.0, 7.0]), "{r}");
    let q = run(&mut s, "query.joints", json!({}));
    let j = &q["joints"][0];
    assert_eq!(j["to_frame"], r["to_frame"], "{q}");
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

#[test]
fn a_second_joint_closes_the_loop_and_the_solver_picks_the_free_value() {
    let mut s = session();
    plate_and_block(&mut s);
    // Slide the block down from the plate's top by a free distance ...
    run(
        &mut s,
        "feature.joint",
        json!({"type": "slider", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(end)",
               "flip": true, "free": ["distance"]}),
    );
    // ... until its bottom meets the plate's bottom: 10 mm.
    let r = run(
        &mut s,
        "feature.joint",
        json!({"type": "rigid", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(start)"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let q = run(&mut s, "query.joints", json!({}));
    let j = &q["joints"][0];
    assert_eq!(j["free"], json!(["distance"]), "{q}");
    assert_eq!(j["built_values"]["distance"], "10 mm", "{q}");
    assert_eq!(q["joints"][1]["status"]["state"], "ok", "{q}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("free: [distance]"), "{text}");
}

#[test]
fn interference_and_a_motion_study() {
    let mut s = session();
    plate_and_block(&mut s);
    // Apart: nothing to report.
    let q = run(&mut s, "query.interference", json!({}));
    assert_eq!(q["checked"], 2, "{q}");
    assert_eq!(q["interference"], json!([]), "{q}");
    assert_eq!(q["truncated"], false, "{q}");
    run(
        &mut s,
        "feature.joint",
        json!({"type": "slider", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(end)",
               "distance": "slide", "distance_limits": ["0 mm", "10 mm"]}),
    );
    let before = s.document(&gusset_history::Actor::User).unwrap().to_text();
    let m = run(
        &mut s,
        "study.motion",
        json!({"parameter": "slide", "from": "0 mm", "to": "1 cm", "steps": 3, "bodies": ["Block"]}),
    );
    let steps = m["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3, "{m}");
    let z: Vec<f64> = steps.iter().map(|f| f["bodies"][0]["centroid_mm"][2].as_f64().unwrap()).collect();
    // Without flip the block's outward bottom normal turns to the plate's top normal, so
    // it hangs into the plate (z 5..10) and slides up 0, 5 and 10 mm.
    assert!(z.iter().zip([7.5, 12.5, 17.5]).all(|(a, b)| (a - b).abs() < 1e-9), "{z:?}");
    assert_eq!(steps[1]["value"], "5 mm", "{m}");
    assert!(steps.iter().all(|f| f["errors"] == json!([])), "{m}");
    // The document did not change.
    assert_eq!(s.document(&gusset_history::Actor::User).unwrap().to_text(), before);
    // Past the limits the step reports the joint's error.
    let m = run(&mut s, "study.motion", json!({"parameter": "slide", "from": "10 mm", "to": "20 mm", "steps": 2}));
    assert!(m["steps"][1]["errors"][0].as_str().unwrap().contains("limits"), "{m}");
    // Past the parameter's own max: the step says so, and value stays a string (F4-6).
    run(&mut s, "param.set", json!({"name": "slide", "expr": "0 mm", "max": "15 mm"}));
    let m = run(&mut s, "study.motion", json!({"parameter": "slide", "from": "0 mm", "to": "20 mm", "steps": 2}));
    let last = &m["steps"][1];
    assert_eq!(last["value"], "20 mm", "{m}");
    assert!(last["value_error"].as_str().unwrap().contains("outside its range"), "{m}");
    assert!(last["errors"].to_string().contains("outside its range"), "{m}");
    for bad in [
        json!({"parameter": "nope", "from": "0", "to": "1"}),
        json!({"parameter": "slide", "from": "0 mm", "to": "1 mm", "steps": 1}),
    ] {
        assert!(execute(&mut s, &CmdCtx::user(1), "study.motion", bad).is_err());
    }
}

#[test]
fn bom_groups_identical_bodies() {
    let mut s = session();
    plate_and_block(&mut s);
    // A second block, elsewhere: the same part.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch3", "x": 200, "y": 50, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch3", "distance": "5 mm", "body_name": "Block2"}));
    let b = run(&mut s, "query.bom", json!({}));
    let rows = b["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{b}");
    assert_eq!(rows[0]["part"], "Plate");
    assert_eq!(rows[0]["quantity"], 1);
    assert_eq!(rows[1]["bodies"], json!(["Block", "Block2"]), "{b}");
    assert_eq!(rows[1]["quantity"], 2);
    let csv = b["csv"].as_str().unwrap();
    assert!(csv.starts_with("item,part,quantity,bodies,volume_mm3,area_mm2\n1,Plate,1,Plate,8000,"), "{csv}");
    assert!(csv.contains("\n2,Block,2,Block Block2,500,"), "{csv}");
}

#[test]
fn with_lists_that_place_a_body_twice_are_refused() {
    // Review M3: a body name and its feature name are one body; a body an earlier joint
    // placed is not carried again; a closing joint carries nothing.
    let mut s = session();
    plate_and_block(&mut s);
    let refused = |s: &mut Session, args: Value| {
        let e = execute(s, &CmdCtx::user(1), "feature.joint", args).unwrap_err();
        format!("{e:?}")
    };
    let base = json!({"type": "rigid", "body": "Plate", "at": "Extrude1/cap(start)", "to": "origin"});
    let with = |w: Value| {
        let mut a = base.clone();
        a["with"] = w;
        a
    };
    let e = refused(&mut s, with(json!(["Block", "Extrude2"])));
    assert!(e.contains("listed twice"), "{e}");
    let e = refused(&mut s, with(json!(["Plate"])));
    assert!(e.contains("both the moving body"), "{e}");
    // Block is placed by the first joint; carrying it again, or moving it by a closing joint
    // that carries something, is refused.
    run(
        &mut s,
        "feature.joint",
        json!({"type": "rigid", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(end)"}),
    );
    let e = refused(&mut s, with(json!(["Block"])));
    assert!(e.contains("already placed"), "{e}");
    let closing =
        json!({"type": "rigid", "body": "Block", "with": ["Plate"], "at": "Extrude2/cap(start)", "to": "origin"});
    let e = refused(&mut s, closing);
    assert!(e.contains("cannot carry"), "{e}");
    // Nothing was added by the refusals.
    let q = run(&mut s, "query.joints", json!({}));
    assert_eq!(q["joints"].as_array().map(Vec::len), Some(1), "{q}");
}
