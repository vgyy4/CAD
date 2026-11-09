//! Feature-tree import (ADR 0042) with the real OCCT kernel: each exporter plugin's sample
//! file imports as features whose volume matches the volume the sample states.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    // Parallel tests can read the same clock value (Windows): a sequence number keeps their
    // folders apart.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = format!("{n}-{}", SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let dir = std::env::temp_dir().join(format!("gusset-tree-occt-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.project_dir = Some(dir);
    s
}

#[test]
fn the_plugin_samples_import_with_their_stated_volumes() {
    for sample in ["fusion", "onshape", "solidworks"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../plugins/{sample}/sample/expected.gtree.json"));
        let bytes = std::fs::read(&path).unwrap();
        let mut s = session();
        std::fs::write(s.project_dir.clone().unwrap().join("s.gtree.json"), &bytes).unwrap();
        let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "s.gtree.json"}))
            .unwrap_or_else(|e| panic!("{sample}: {e:?}"));
        eprintln!("{sample}: {r}");
        let t: Value = serde_json::from_slice(&bytes).unwrap();
        let unsupported = t["features"].as_array().unwrap().iter().filter(|f| f["type"] == "unsupported").count();
        assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), unsupported, "{sample}: {r}");
        let v = &r["volume_mm3"];
        let (src, ours) = (v["source"].as_f64().unwrap(), v["imported"].as_f64().unwrap());
        assert!((src - ours).abs() < 1e-3 * src, "{sample}: {r}");
    }
}

#[test]
fn a_revolve_about_a_world_axis_lands_on_the_sketch_axis() {
    // Dogfooding F8-2: on an XZ sketch, world Z is the sketch's y axis. A 5..10 x 0..10
    // rectangle turned about it is a ring of pi (10^2 - 5^2) 10.
    let tree = json!({
        "format": "gusset-feature-tree", "version": 1, "source": {"app": "test"},
        "features": [
            {"type": "sketch", "name": "S", "plane": {"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 0, 1]},
             "points": [{"id": "a", "x": 5, "y": 0}, {"id": "b", "x": 10, "y": 0}, {"id": "c", "x": 10, "y": 10}, {"id": "d", "x": 5, "y": 10}],
             "curves": [{"id": "l1", "type": "line", "start": "a", "end": "b"}, {"id": "l2", "type": "line", "start": "b", "end": "c"},
                        {"id": "l3", "type": "line", "start": "c", "end": "d"}, {"id": "l4", "type": "line", "start": "d", "end": "a"}]},
            {"type": "revolve", "name": "R", "sketch": "S", "profiles": "all",
             "axis": {"origin": [0, 0, 0], "direction": [0, 0, -1]}, "angle": 360, "operation": "new"}
        ],
        "bodies": [{"name": "R", "volume_mm3": 2356.194490}]
    });
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("r.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "r.gtree.json"})).unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let v = &r["volume_mm3"];
    assert!((v["imported"].as_f64().unwrap() - 2356.194490).abs() < 1e-3, "{r}");
}

// ---- Version 2 (ADR 0042 amendment): holes, patterns, mirrors, shells ----

fn rect(name: &str, plane: Value, x0: f64, y0: f64, w: f64, h: f64) -> Value {
    json!({
        "type": "sketch", "name": name, "plane": plane,
        "points": [
            {"id": "p1", "x": x0, "y": y0}, {"id": "p2", "x": x0 + w, "y": y0},
            {"id": "p3", "x": x0 + w, "y": y0 + h}, {"id": "p4", "x": x0, "y": y0 + h}
        ],
        "curves": [
            {"id": "l1", "type": "line", "start": "p1", "end": "p2"},
            {"id": "l2", "type": "line", "start": "p2", "end": "p3"},
            {"id": "l3", "type": "line", "start": "p3", "end": "p4"},
            {"id": "l4", "type": "line", "start": "p4", "end": "p1"}
        ]
    })
}

fn disc(name: &str, plane: Value, x: f64, y: f64, r: f64) -> Value {
    json!({
        "type": "sketch", "name": name, "plane": plane,
        "points": [{"id": "c", "x": x, "y": y}],
        "curves": [{"id": "k", "type": "circle", "center": "c", "radius": r}]
    })
}

fn plane_at(z: f64) -> Value {
    json!({"origin": [0, 0, z], "x_axis": [1, 0, 0], "y_axis": [0, 1, 0]})
}

fn extrude(name: &str, sketch: &str, d: f64, op: &str) -> Value {
    json!({"type": "extrude", "name": name, "sketch": sketch, "profiles": "all", "distance": d, "operation": op})
}

/// Imports a version 2 tree and returns the report and the one body's volume.
fn import_v2(features: Vec<Value>) -> (Value, f64) {
    let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"}, "features": features});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
    (r, b["volume_mm3"].as_f64().unwrap())
}

fn assert_vol(got: f64, want: f64) {
    assert!((got - want).abs() < 1e-2, "volume {got}, expected {want}");
}

#[test]
fn a_plate_with_a_through_hole_and_a_counterbore() {
    let pi = std::f64::consts::PI;
    let (_, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 40.0, 20.0),
        extrude("Plate", "Sketch1", 10.0, "new"),
        json!({"type": "hole", "name": "Hole1", "face": {"point": [20, 10, 10], "normal": [0, 0, 1]},
               "positions": [[10, 10, 10]], "diameter": 6, "through": true}),
        json!({"type": "hole", "name": "Hole2", "face": {"point": [20, 10, 10]}, "variant": "counterbore",
               "positions": [[30, 10, 10]], "diameter": 4, "cbore_diameter": 8, "cbore_depth": 3, "through": true}),
    ]);
    // 6 mm through (9 pi 10), 4 mm through (4 pi 10) and the 8 mm step 3 deep ((16 - 4) pi 3).
    assert_vol(v, 8000.0 - pi * (90.0 + 40.0 + 36.0));
}

#[test]
fn a_blind_hole_in_a_side_face() {
    // A hole on the +X face of a block: the face frame is not the XY one.
    let (r, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 20.0, 20.0),
        extrude("Block", "Sketch1", 20.0, "new"),
        json!({"type": "hole", "name": "Side", "face": {"point": [20, 10, 10], "normal": [1, 0, 0]},
               "positions": [[20, 5, 15]], "diameter": 4, "depth": 10, "point_angle": 180}),
    ]);
    assert_vol(v, 8000.0 - std::f64::consts::PI * 4.0 * 10.0);
    assert!(r["features"].as_array().unwrap().iter().any(|f| f == "Side"), "{r}");
}

#[test]
fn a_linear_pattern_of_three_bosses() {
    let (_, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 60.0, 20.0),
        extrude("Plate", "Sketch1", 5.0, "new"),
        disc("Sketch2", plane_at(5.0), 10.0, 10.0, 2.0),
        extrude("Boss", "Sketch2", 4.0, "join"),
        json!({"type": "pattern_linear", "name": "Row", "features": ["Boss"], "direction": [1, 0, 0],
               "count": 3, "spacing": 20}),
    ]);
    assert_vol(v, 6000.0 + 3.0 * std::f64::consts::PI * 4.0 * 4.0);
}

#[test]
fn a_circular_pattern_of_holes() {
    let (_, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), -20.0, -20.0, 40.0, 40.0),
        extrude("Plate", "Sketch1", 5.0, "new"),
        json!({"type": "hole", "name": "H", "face": {"point": [0, 0, 5]}, "positions": [[10, 0, 5]],
               "diameter": 2, "through": true}),
        json!({"type": "pattern_circular", "name": "Ring", "features": ["H"], "count": 4,
               "axis": {"origin": [0, 0, 0], "direction": [0, 0, 1]}, "angle": 360}),
    ]);
    assert_vol(v, 8000.0 - 4.0 * std::f64::consts::PI * 1.0 * 5.0);
}

#[test]
fn a_mirrored_boss() {
    // A boss at x = 10 on a plate spanning x = -20..20, mirrored across the YZ plane.
    let (r, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), -20.0, 0.0, 40.0, 20.0),
        extrude("Plate", "Sketch1", 5.0, "new"),
        disc("Sketch2", plane_at(5.0), 10.0, 10.0, 3.0),
        extrude("Boss", "Sketch2", 4.0, "join"),
        json!({"type": "mirror", "name": "Mirror1", "features": ["Boss"],
               "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}}),
    ]);
    assert_vol(v, 4000.0 + 2.0 * std::f64::consts::PI * 9.0 * 4.0);
    assert!(r["features"].as_array().unwrap().iter().any(|f| f == "Boss_Mirror1"), "{r}");
}

#[test]
fn a_mirrored_body_merges_across_an_offset_plane() {
    let (_, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
        extrude("Half", "Sketch1", 5.0, "new"),
        json!({"type": "mirror", "name": "Both", "target": "Half", "merge": true,
               "plane": {"origin": [10, 0, 0], "normal": [1, 0, 0]}}),
    ]);
    assert_vol(v, 1000.0);
}

#[test]
fn a_shelled_box_with_the_top_face_removed() {
    let (_, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 30.0, 20.0),
        extrude("Box", "Sketch1", 10.0, "new"),
        json!({"type": "shell", "name": "Shell1", "faces": [{"point": [15, 10, 10], "normal": [0, 0, 1]}],
               "thickness": 2, "side": "inside"}),
    ]);
    assert_vol(v, 30.0 * 20.0 * 10.0 - 26.0 * 16.0 * 8.0);
}

/// Like `import_v2` but keeps the session, for tests that look into the model.
fn import_v2_session(features: Vec<Value>) -> (Session, Value) {
    let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"}, "features": features});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    (s, r)
}

#[test]
fn a_hole_picked_on_one_of_two_coplanar_tops_lands_on_that_one() {
    // Two bars on a plate with tops in the plane z = 9. A point on the second (short) bar is
    // nearer the first bar's centre than its own: the point-in-face test picks the second.
    let pi = std::f64::consts::PI;
    let (mut s, r) = import_v2_session(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 100.0, 20.0),
        extrude("Plate", "Sketch1", 5.0, "new"),
        rect("Sketch2", plane_at(5.0), 0.0, 0.0, 100.0, 10.0),
        extrude("BarA", "Sketch2", 4.0, "join"),
        rect("Sketch3", plane_at(5.0), 0.0, 14.0, 60.0, 4.0),
        extrude("BarB", "Sketch3", 4.0, "join"),
        json!({"type": "hole", "name": "Hole1", "face": {"point": [58, 16, 9], "normal": [0, 0, 1]},
               "positions": [[58, 16, 9]], "diameter": 4, "through": true}),
    ]);
    assert!(r["warnings_user_text"].as_array().unwrap().is_empty(), "{r}");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 10000.0 + 4000.0 + 960.0 - 36.0 * pi);
    let text = execute(&mut s, &CmdCtx::user(1), "document.get_text", json!({})).unwrap();
    let text = text["text_user_text"].as_str().unwrap().to_string();
    let face = text.split("hole Hole1 (face: ").nth(1).and_then(|t| t.split(',').next()).unwrap().to_string();
    // The face is named on the model before the hole.
    let doc = gusset_doc::Document::from_text(text.split("\nhole Hole1").next().unwrap()).unwrap();
    let ev = gusset_eval::Evaluator::new(s.kernel().clone()).evaluate(&doc, None, &());
    let body = ev.bodies.last().unwrap();
    // The hole's face is a bar top, the one centred at (30, 16).
    let i = body.names.faces.iter().position(|n| n.to_string() == face).unwrap();
    let c = body.topology.faces[i].centroid;
    assert!((c[0] - 30.0).abs() < 1e-3 && (c[1] - 16.0).abs() < 1e-3 && (c[2] - 9.0).abs() < 1e-3, "{face} {c:?}");
}

#[test]
fn a_mirrored_cut_cuts_the_mirrored_copy() {
    // [Base, a cut of it] mirrored across x = 0: the copy's cut targets the copy.
    let pi = std::f64::consts::PI;
    let (mut s, _) = import_v2_session(vec![
        rect("Sketch1", plane_at(0.0), 10.0, 0.0, 20.0, 10.0),
        extrude("Base", "Sketch1", 5.0, "new"),
        disc("Sketch2", plane_at(5.0), 20.0, 5.0, 2.0),
        json!({"type": "extrude", "name": "Cut1", "sketch": "Sketch2", "profiles": "all", "distance": -5,
               "operation": "cut", "target": "Base"}),
        json!({"type": "mirror", "name": "Mirror1", "features": ["Base", "Cut1"],
               "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}}),
    ]);
    let want = 1000.0 - pi * 4.0 * 5.0;
    let ev = s.evaluate(&gusset_history::Actor::User).unwrap();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    assert_eq!(ev.bodies.len(), 2);
    let k = s.kernel().clone();
    for b in &ev.bodies {
        assert_vol(k.mass_properties(&b.shape).unwrap().volume, want);
    }
}

#[test]
fn a_sketch_on_a_plane_tilted_two_ways_lands_where_the_source_has_it() {
    // x = (0.8, 0.6, 0), y = (-0.36, 0.48, 0.8): normal (0.48, -0.64, 0.6), tilted about two axes.
    let (o, x, y) = ([5.0, -3.0, 7.0], [0.8, 0.6, 0.0], [-0.36, 0.48, 0.8]);
    let n = [0.48, -0.64, 0.6];
    let plane = json!({"origin": o, "x_axis": x, "y_axis": y});
    let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"},
        "features": [rect("S", plane, 2.0, 3.0, 10.0, 20.0), extrude("E", "S", 5.0, "new")]});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0);
    // The box of the eight corners origin + u x + v y + w n.
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for (u, v, w) in
        [2.0, 12.0].iter().flat_map(|u| [3.0, 23.0].map(|v| (*u, v))).flat_map(|(u, v)| [0.0, 5.0].map(|w| (u, v, w)))
    {
        for i in 0..3 {
            let p = o[i] + u * x[i] + v * y[i] + w * n[i];
            lo[i] = lo[i].min(p);
            hi[i] = hi[i].max(p);
        }
    }
    for i in 0..3 {
        let (l, h) = (b["bbox_min_mm"][i].as_f64().unwrap(), b["bbox_max_mm"][i].as_f64().unwrap());
        assert!((l - lo[i]).abs() < 1e-3 && (h - hi[i]).abs() < 1e-3, "{i}: {l}..{h} vs {}..{} ({b})", lo[i], hi[i]);
    }
}

#[test]
fn features_mirrored_across_a_tilted_plane_land_on_their_reflection() {
    // The block [0, 10] x [0, 10] x [0, 5] reflected across the plane through (30, 0, 0)
    // with normal (1, 0, 2) / sqrt 5, wholly on one side of it.
    let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"},
        "features": [rect("S", plane_at(0.0), 0.0, 0.0, 10.0, 10.0), extrude("E", "S", 5.0, "new"),
            {"type": "mirror", "name": "T", "features": ["E"], "plane": {"origin": [30, 0, 0], "normal": [1, 0, 2]}}]});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let m = execute(&mut s, &CmdCtx::user(1), "query.model", json!({})).unwrap();
    eprintln!("{m}");
    let names: Vec<String> =
        m["bodies"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap().to_string()).collect();
    let copy = names.iter().find(|n| n.contains("_T")).unwrap_or_else(|| panic!("{m}"));
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": copy})).unwrap();
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 500.0);
    let k = 1.0 / 5.0_f64.sqrt();
    let n = [k, 0.0, 2.0 * k];
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in
        [0.0, 10.0].iter().flat_map(|x| [0.0, 10.0].map(|y| (*x, y))).flat_map(|(x, y)| [0.0, 5.0].map(|z| [x, y, z]))
    {
        let d = (p[0] - 30.0) * n[0] + p[2] * n[2];
        for i in 0..3 {
            let q = p[i] - 2.0 * d * n[i];
            lo[i] = lo[i].min(q);
            hi[i] = hi[i].max(q);
        }
    }
    for i in 0..3 {
        let (l, h) = (b["bbox_min_mm"][i].as_f64().unwrap(), b["bbox_max_mm"][i].as_f64().unwrap());
        assert!((l - lo[i]).abs() < 1e-3 && (h - hi[i]).abs() < 1e-3, "{i}: {l}..{h} vs {}..{} ({b})", lo[i], hi[i]);
    }
}

#[test]
fn a_body_mirrored_across_a_tilted_plane_lands_on_its_reflection() {
    // The block [0, 10] x [0, 10] x [0, 5] reflected across the plane through (30, 0, 0)
    // with normal (1, 0, 2) / sqrt 5, wholly on one side of it.
    let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"},
        "features": [rect("S", plane_at(0.0), 0.0, 0.0, 10.0, 10.0), extrude("E", "S", 5.0, "new"),
            {"type": "mirror", "name": "T", "target": "E", "merge": false, "plane": {"origin": [30, 0, 0], "normal": [1, 0, 2]}}]});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let m = execute(&mut s, &CmdCtx::user(1), "query.model", json!({})).unwrap();
    eprintln!("{m}");
    let names: Vec<String> =
        m["bodies"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap().to_string()).collect();
    let copy = names.iter().find(|n| *n != "E").unwrap_or_else(|| panic!("{m}"));
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": copy})).unwrap();
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 500.0);
    let k = 1.0 / 5.0_f64.sqrt();
    let n = [k, 0.0, 2.0 * k];
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in
        [0.0, 10.0].iter().flat_map(|x| [0.0, 10.0].map(|y| (*x, y))).flat_map(|(x, y)| [0.0, 5.0].map(|z| [x, y, z]))
    {
        let d = (p[0] - 30.0) * n[0] + p[2] * n[2];
        for i in 0..3 {
            let q = p[i] - 2.0 * d * n[i];
            lo[i] = lo[i].min(q);
            hi[i] = hi[i].max(q);
        }
    }
    for i in 0..3 {
        let (l, h) = (b["bbox_min_mm"][i].as_f64().unwrap(), b["bbox_max_mm"][i].as_f64().unwrap());
        assert!((l - lo[i]).abs() < 1e-3 && (h - hi[i]).abs() < 1e-3, "{i}: {l}..{h} vs {}..{} ({b})", lo[i], hi[i]);
    }
}

#[test]
fn a_disc_swept_along_a_path_sketch_line() {
    // A radius 2 disc swept 20 mm up a line of an XZ sketch: pi 4 20.
    let path = json!({
        "type": "sketch", "name": "Path", "plane": {"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 0, 1]},
        "points": [{"id": "a", "x": 0, "y": 0}, {"id": "b", "x": 0, "y": 20}],
        "curves": [{"id": "p", "type": "line", "start": "a", "end": "b"}]
    });
    let sweep = json!({"type": "sweep", "name": "Pipe", "sketch": "Disc", "path": "Path", "path_curves": ["p"],
        "orientation": "frenet", "operation": "new"});
    let (r, v) = import_v2(vec![disc("Disc", plane_at(0.0), 0.0, 0.0, 2.0), path, sweep]);
    assert_vol(v, std::f64::consts::PI * 4.0 * 20.0);
    assert!(r["features"].to_string().contains("Pipe"), "{r}");
}

#[test]
fn a_loft_between_two_squares() {
    // Two 10 mm squares 8 mm apart: a 10 x 10 x 8 block.
    let loft = json!({"type": "loft", "name": "Block", "sketches": ["Low", "High"], "operation": "new"});
    let (_, v) = import_v2(vec![
        rect("Low", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
        rect("High", plane_at(8.0), 0.0, 0.0, 10.0, 10.0),
        loft,
    ]);
    assert_vol(v, 800.0);
}

#[test]
fn a_countersunk_hole_on_a_pocket_floor() {
    // A 20 x 20 x 10 block, a 6 mm, 0.15 mm deep pocket on top (a head clearance), then a
    // through 3 mm hole with a 6 mm 90 deg countersink starting on the pocket floor.
    let hole = json!({"type": "hole", "name": "H", "face": {"point": [10, 10, 9.85], "normal": [0, 0, 1]},
        "positions": [[10, 10, 9.85]], "diameter": 3, "through": true,
        "variant": "countersink", "csink_diameter": 6, "csink_angle": 90});
    let (r, v) = import_v2(vec![
        rect("Base", plane_at(0.0), 0.0, 0.0, 20.0, 20.0),
        extrude("Block", "Base", 10.0, "new"),
        disc("Top", plane_at(10.0), 10.0, 10.0, 3.0),
        json!({"type": "extrude", "name": "Clear", "sketch": "Top", "profiles": "all", "distance": -0.15,
            "operation": "cut", "target": "Block"}),
        hole,
    ]);
    let pi = std::f64::consts::PI;
    let frustum = pi * 1.5 / 3.0 * (9.0 + 4.5 + 2.25);
    let removed = pi * 9.0 * 0.15 + frustum + pi * 2.25 * (9.85 - 1.5);
    assert_vol(v, 4000.0 - removed);
    assert!(r["features"].to_string().contains("\"H\""), "{r}");
}

#[test]
fn one_tree_hole_on_two_separate_pocket_floors() {
    // Two 6 mm, 0.15 mm deep pockets, then one countersunk through hole at both floors: the
    // floors are separate faces of one plane.
    let hole = json!({"type": "hole", "name": "H", "face": {"point": [5, 10, 9.85], "normal": [0, 0, 1]},
        "positions": [[5, 10, 9.85], [15, 10, 9.85]], "diameter": 3, "through": true,
        "variant": "countersink", "csink_diameter": 6, "csink_angle": 90});
    let two = json!({"type": "sketch", "name": "Top", "plane": {"origin": [0, 0, 10], "x_axis": [1, 0, 0], "y_axis": [0, 1, 0]},
        "points": [{"id": "a", "x": 5, "y": 10}, {"id": "b", "x": 15, "y": 10}],
        "curves": [{"id": "k1", "type": "circle", "center": "a", "radius": 3}, {"id": "k2", "type": "circle", "center": "b", "radius": 3}]});
    let (_, v) = import_v2(vec![
        rect("Base", plane_at(0.0), 0.0, 0.0, 20.0, 20.0),
        extrude("Block", "Base", 10.0, "new"),
        two,
        json!({"type": "extrude", "name": "Clear", "sketch": "Top", "profiles": "all", "distance": -0.15,
            "operation": "cut", "target": "Block"}),
        hole,
    ]);
    let pi = std::f64::consts::PI;
    let frustum = pi * 1.5 / 3.0 * (9.0 + 4.5 + 2.25);
    let one = pi * 9.0 * 0.15 + frustum + pi * 2.25 * (9.85 - 1.5);
    assert_vol(v, 4000.0 - 2.0 * one);
}

#[test]
fn a_tree_combine_cuts_one_body_from_another() {
    // A 10 mm cube less a radius 2 mm through cylinder.
    let (_, v) = import_v2(vec![
        rect("Base", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
        extrude("Box", "Base", 10.0, "new"),
        disc("Round", plane_at(0.0), 5.0, 5.0, 2.0),
        extrude("Pin", "Round", 10.0, "new"),
        json!({"type": "combine", "name": "Cut", "target": "Box", "tools": ["Pin"], "operation": "cut"}),
    ]);
    assert_vol(v, 1000.0 - std::f64::consts::PI * 4.0 * 10.0);
}

#[test]
fn a_cut_through_two_regions_that_share_an_edge() {
    // A 10 x 176.25 strip and a 3 x 32 tab whose corner lies on the strip's side: two
    // regions with a common edge. Their prisms touch face to face; the cut must still take
    // both (it once failed as an invalid solid).
    let profile = json!({
        "type": "sketch", "name": "S1", "plane": plane_at(0.0),
        "points": [{"id": "p0", "x": -90, "y": -80}, {"id": "p1", "x": -80, "y": -80}, {"id": "p2", "x": -80, "y": 96.25},
                   {"id": "p3", "x": -90, "y": 96.25}, {"id": "p4", "x": -80, "y": -48}, {"id": "p5", "x": -77, "y": -80},
                   {"id": "p6", "x": -77, "y": -48}],
        "curves": [{"id": "c0", "type": "line", "start": "p0", "end": "p1"}, {"id": "c1", "type": "line", "start": "p1", "end": "p2"},
                   {"id": "c2", "type": "line", "start": "p2", "end": "p3"}, {"id": "c3", "type": "line", "start": "p3", "end": "p0"},
                   {"id": "c4", "type": "line", "start": "p1", "end": "p5"}, {"id": "c5", "type": "line", "start": "p5", "end": "p6"},
                   {"id": "c6", "type": "line", "start": "p6", "end": "p4"}, {"id": "c7", "type": "line", "start": "p4", "end": "p1"}]
    });
    let (_, v) = import_v2(vec![
        rect("Base", plane_at(0.0), -100.0, -100.0, 200.0, 200.0),
        extrude("Plate", "Base", 1.5, "new"),
        profile,
        json!({"type": "extrude", "name": "Cut", "sketch": "S1", "profiles": "all", "distance": 5.0, "operation": "cut", "target": "Plate"}),
    ]);
    assert_vol(v, 60000.0 - 1.5 * (10.0 * 176.25 + 3.0 * 32.0));
}

/// Imports a version 2 tree into a fresh session; returns the session and the report.
fn import_into_session(features: Vec<Value>) -> (Session, Value) {
    let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"}, "features": features});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    (s, r)
}

fn unequal_chamfer_tree(chamfer: Value) -> Vec<Value> {
    vec![rect("Base", plane_at(0.0), 0.0, 0.0, 10.0, 10.0), extrude("Box", "Base", 10.0, "new"), chamfer]
}

#[test]
fn an_unequal_chamfer_runs_its_first_distance_along_the_picked_face() {
    // The 10 mm edge along x at y = 0, z = 10 borders the top (z = 10) and the front (y = 0).
    let edge = json!({"midpoint": [5, 0, 10]});
    let top = json!({"point": [5, 5, 10], "normal": [0, 0, 1]});
    let front = json!({"point": [5, 0, 5], "normal": [0, -1, 0]});
    for (face, along_top) in [(&top, true), (&front, false)] {
        let (mut s, r) = import_into_session(unequal_chamfer_tree(json!({
            "type": "chamfer", "name": "C", "edges": [edge], "distance": 2, "distance2": 3, "face": face
        })));
        assert_eq!(r["skipped_user_text"], json!([]), "{r}");
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
        assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0 - 0.5 * 2.0 * 3.0 * 10.0);
        let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
        let line = text.lines().find(|l| l.starts_with("chamfer ")).unwrap_or_else(|| panic!("{text}"));
        assert!(line.contains("distance: 2 mm, distance2: 3 mm, faces: ["), "{line}");
        assert_eq!(line.contains("faces: [Extrude1/cap(end)]") || line.contains("/cap(end)]"), along_top, "{line}");
    }
}

#[test]
fn a_distance_and_angle_chamfer_without_a_face_uses_the_default_face() {
    // 3 mm from the default reference face at 30 degrees: legs 3 and 3 tan 30; edge 10 mm.
    let (_, v) = import_v2(unequal_chamfer_tree(json!({
        "type": "chamfer", "name": "C", "edges": [{"midpoint": [5, 0, 10]}], "distance": 3, "angle": 30
    })));
    assert_vol(v, 1000.0 - 0.5 * 3.0 * (3.0 / 3.0_f64.sqrt()) * 10.0);
}

#[test]
fn bad_unequal_chamfers_are_skipped_with_a_reason() {
    for extra in
        [json!({"distance2": 2, "angle": 30}), json!({"distance2": -1}), json!({"angle": 90}), json!({"angle": 0})]
    {
        let mut c = json!({"type": "chamfer", "name": "C", "edges": [{"midpoint": [5, 0, 10]}], "distance": 2});
        c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let (mut s, r) = import_into_session(unequal_chamfer_tree(c));
        let skipped = r["skipped_user_text"].as_array().unwrap();
        assert_eq!(skipped.len(), 1, "{extra}: {r}");
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
        assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0);
    }
}
