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

#[test]
fn faces_picked_by_their_boundary_points_and_surface() {
    // The box's top by its corners on the plane z = 10 (ADR 0042 amendment 2026-11-11).
    let corners = json!([[0, 0, 10], [30, 0, 10], [30, 20, 10], [0, 20, 10]]);
    let (_, v) = import_v2(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 30.0, 20.0),
        extrude("Box", "Sketch1", 10.0, "new"),
        json!({"type": "shell", "name": "Shell1", "thickness": 2, "side": "inside",
               "faces": [{"point": [0, 0, 10], "points": corners,
                          "surface": {"type": "plane", "origin": [0, 0, 10], "normal": [0, 0, 1]}}]}),
    ]);
    assert_vol(v, 30.0 * 20.0 * 10.0 - 26.0 * 16.0 * 8.0);
    // A disc's side by points of its two circles on the cylinder r = 5, pushed out by 1 mm.
    let ring: Vec<Value> =
        [0.0, 10.0].iter().flat_map(|z| [json!([5, 0, z]), json!([0, 5, z]), json!([-5, 0, z])]).collect();
    let (_, v) = import_v2(vec![
        disc("Sketch1", plane_at(0.0), 0.0, 0.0, 5.0),
        extrude("Rod", "Sketch1", 10.0, "new"),
        json!({"type": "offset_face", "name": "Fatter", "distance": 1,
               "faces": [{"point": [5, 0, 0], "points": ring,
                          "surface": {"type": "cylinder", "origin": [0, 0, 0], "axis": [0, 0, 1], "radius": 5}}]}),
    ]);
    assert_vol(v, std::f64::consts::PI * 36.0 * 10.0);
}

#[test]
fn a_face_pick_two_faces_hold_alike_is_refused() {
    // The ends of the top front edge lie on the top and on the front: no surface to tell them
    // apart, so the shell is skipped.
    let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"}, "features": [
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 30.0, 20.0),
        extrude("Box", "Sketch1", 10.0, "new"),
        {"type": "shell", "name": "Shell1", "thickness": 2,
         "faces": [{"point": [0, 0, 10], "points": [[0, 0, 10], [30, 0, 10]], "surface": {"type": "other"}}]}]});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    let sk = r["skipped_user_text"].as_array().unwrap();
    assert!(sk.iter().any(|x| x["feature"] == "Shell1" && x["reason"].as_str().unwrap().contains("ambiguous")), "{r}");
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
fn an_offset_on_a_body_whose_face_is_in_pieces_offsets_the_pieces() {
    // A 10 mm block, and beside it a plug made of two regions whose side on x = 10 stays in two
    // pieces. The offset's points are the plug side's corners and its body is the plug: the
    // block's side holds them all in one face, the plug's two pieces hold them between them.
    let two = json!({
        "type": "sketch", "name": "Two", "plane": {"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 1, 0]},
        "points": [
            {"id": "a", "x": 10, "y": 0}, {"id": "b", "x": 15, "y": 0}, {"id": "c", "x": 15, "y": 5},
            {"id": "d", "x": 15, "y": 10}, {"id": "e", "x": 10, "y": 10}, {"id": "f", "x": 10, "y": 5}
        ],
        "curves": [
            {"id": "l1", "type": "line", "start": "a", "end": "b"}, {"id": "l2", "type": "line", "start": "b", "end": "c"},
            {"id": "l3", "type": "line", "start": "c", "end": "d"}, {"id": "l4", "type": "line", "start": "d", "end": "e"},
            {"id": "l5", "type": "line", "start": "e", "end": "f"}, {"id": "l6", "type": "line", "start": "f", "end": "a"},
            {"id": "l7", "type": "line", "start": "f", "end": "c"}
        ]
    });
    let corners = json!([[10, 0, 0], [10, 10, 0], [10, 10, 5], [10, 0, 5], [10, 5, 0], [10, 5, 5]]);
    let (mut s, _) = import_v2_session(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
        extrude("Block", "Sketch1", 10.0, "new"),
        two,
        extrude("Plug", "Two", 5.0, "new"),
        json!({"type": "offset_face", "name": "Thinner", "distance": -1, "on_body": "Plug",
               "faces": [{"point": [10, 5, 2.5], "points": corners,
                          "surface": {"type": "plane", "origin": [10, 0, 0], "normal": [-1, 0, 0]}}]}),
    ]);
    // The plug (the body made last) loses 1 x 10 x 5; on the block it would keep its 250.
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 200.0);
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

/// A tree helix about world Z.
fn helix(extra: Value) -> Value {
    let mut h = json!({"origin": [0, 0, 0], "axis": [0, 0, 1], "radius": 10, "pitch": 5, "height": 20});
    h.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    h
}

fn helix_sweep(profile: Value, helix: Value, extra: Value) -> Vec<Value> {
    let mut sweep = json!({"type": "sweep", "name": "Coil", "sketch": "Wire", "helix": helix, "operation": "new"});
    sweep.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    vec![profile, sweep]
}

/// A radius 1 disc in the XZ plane (through the Z axis), centred 10 mm from it.
fn wire_through_the_axis() -> Value {
    disc("Wire", json!({"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 0, 1]}), 10.0, 0.0, 1.0)
}

#[test]
fn a_disc_swept_along_a_helix_gives_pappus_volume() {
    // Per turn the disc (a pi, centroid 10 mm from the axis) sweeps pi x 2 pi 10: four turns
    // (pitch 5, height 20), right- or left-handed, by height or by turns.
    let want = 4.0 * std::f64::consts::PI * 2.0 * std::f64::consts::PI * 10.0;
    for h in [json!({}), json!({"left_handed": true}), json!({"height": null, "turns": 4})] {
        let (r, v) = import_v2(helix_sweep(wire_through_the_axis(), helix(h.clone()), json!({})));
        assert_vol(v, want);
        assert!(r["features"].to_string().contains("Coil"), "{h}: {r}");
    }
}

#[test]
fn a_disc_across_the_axis_swept_along_a_helix_gives_area_times_height() {
    // A disc in a plane across the axis moves 20 mm along it: pi 1^2 x 20.
    let (_, v) =
        import_v2(helix_sweep(disc("Wire", plane_at(0.0), 10.0, 0.0, 1.0), helix(json!({"x": [1, 0, 0]})), json!({})));
    assert_vol(v, std::f64::consts::PI * 20.0);
}

#[test]
fn a_helical_cut_removes_a_groove() {
    // The same coil cut from a 24 x 24 x 24 block from z = -2 (the coil is wholly inside it).
    let mut f = vec![rect("Base", plane_at(-2.0), -12.0, -12.0, 24.0, 24.0), extrude("Block", "Base", 24.0, "new")];
    f.extend(helix_sweep(wire_through_the_axis(), helix(json!({})), json!({"operation": "cut", "target": "Block"})));
    let (_, v) = import_v2(f);
    assert_vol(v, 24.0 * 24.0 * 24.0 - 4.0 * std::f64::consts::PI * 2.0 * std::f64::consts::PI * 10.0);
}

#[test]
fn bad_helical_sweeps_are_skipped_with_a_reason() {
    let path = json!({
        "type": "sketch", "name": "Path", "plane": {"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 0, 1]},
        "points": [{"id": "a", "x": 0, "y": 0}, {"id": "b", "x": 0, "y": 20}],
        "curves": [{"id": "p", "type": "line", "start": "a", "end": "b"}]
    });
    let cases: Vec<(Value, Value, &str)> = vec![
        (helix(json!({"taper_angle": 5})), json!({}), "tapered"),
        (helix(json!({"axis": [0, 0, -1]})), json!({}), "world axis"),
        (helix(json!({"axis": [1, 1, 0]})), json!({}), "world axis"),
        (helix(json!({"origin": [3, 0, 0]})), json!({}), "through the origin"),
        (helix(json!({"x": [0, 0, 1]})), json!({}), "across the axis"),
        (helix(json!({"turns": 4})), json!({}), "not both"),
        (helix(json!({"height": null})), json!({}), "height or turns"),
        (helix(json!({"pitch": 0})), json!({}), "pitch"),
        (helix(json!({"pitch": -5})), json!({}), "pitch"),
        (helix(json!({"radius": 0})), json!({}), "radius"),
        (helix(json!({"height": -1})), json!({}), "height"),
        (helix(json!({"height": 20000})), json!({}), "2000 turns"),
        (helix(json!({"spin": 1})), json!({}), "spin"),
        (helix(json!({})), json!({"orientation": "fixed"}), "frenet"),
        (helix(json!({})), json!({"path": "Path"}), "not both"),
        (helix(json!({})), json!({"path_curves": ["p"]}), "path_curves"),
    ];
    for (h, extra, why) in cases {
        let mut f = helix_sweep(wire_through_the_axis(), h.clone(), extra.clone());
        f.push(path.clone());
        let (_, r) = import_into_session(f);
        let skipped = r["skipped_user_text"].to_string();
        assert!(skipped.contains(why), "{h} {extra}: expected '{why}' in {r}");
    }
    // Neither a path nor a helix.
    let mut f = helix_sweep(wire_through_the_axis(), json!({}), json!({}));
    f[1].as_object_mut().unwrap().remove("helix");
    let (_, r) = import_into_session(f);
    assert!(r["skipped_user_text"].to_string().contains("path sketch or a helix"), "{r}");
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
fn a_split_makes_its_new_body_on_the_side_its_plane_faces() {
    // A 10 mm cube split at z = 3 mm: the new body is the part the plane's normal points to.
    for (normal, want) in [([0, 0, -1], 300.0), ([0, 0, 1], 700.0)] {
        let (mut s, _) = import_v2_session(vec![
            rect("Base", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
            extrude("Box", "Base", 10.0, "new"),
            json!({"type": "split", "name": "Split1", "target": "Box", "creates_bodies": 1,
                   "plane": {"origin": [5, 5, 3], "normal": normal}}),
        ]);
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Split1"})).unwrap();
        assert_vol(b["volume_mm3"].as_f64().unwrap(), want);
    }
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

#[test]
fn a_chamfer_of_edges_on_two_bodies_becomes_one_per_body() {
    // Two 10 mm cubes apart, one top edge of each chamfered by 2 mm.
    let features = vec![
        rect("A", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
        extrude("BoxA", "A", 10.0, "new"),
        rect("B", plane_at(0.0), 20.0, 0.0, 10.0, 10.0),
        extrude("BoxB", "B", 10.0, "new"),
        json!({"type": "chamfer", "name": "C", "distance": 2,
               "edges": [{"midpoint": [5, 0, 10]}, {"midpoint": [25, 0, 10]}]}),
    ];
    let (mut s, r) = import_into_session(features);
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("chamfer C (") && text.contains("chamfer C_2 ("), "{text}");
    for body in ["BoxA", "BoxB"] {
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": body})).unwrap();
        assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0 - 0.5 * 2.0 * 2.0 * 10.0);
    }
}

// ---- Full-round fillets (ADR 0042 amendment) ----

fn slab_tree(round: Value) -> Vec<Value> {
    vec![rect("Base", plane_at(0.0), 0.0, 0.0, 20.0, 4.0), extrude("Slab", "Base", 10.0, "new"), round]
}

#[test]
fn a_full_round_fillet_is_picked_by_a_point_on_its_face() {
    // The 4 x 10 end face at x = 20 becomes a half cylinder of radius 2.
    let round = json!({"type": "fillet", "name": "R", "full_round": {"point": [20, 2, 5], "normal": [1, 0, 0]}});
    let (mut s, r) = import_into_session(slab_tree(round));
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
    let pi = std::f64::consts::PI;
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 800.0 - (4.0 * 2.0 * 10.0 - pi * 2.0 * 2.0 / 2.0 * 10.0));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("fillet R (full_round: [Slab/side(Base/l6)])"), "{text}");
}

#[test]
fn bad_full_rounds_are_skipped_with_a_reason() {
    let pick = json!({"point": [20, 2, 5], "normal": [1, 0, 0]});
    for round in [
        json!({"type": "fillet", "name": "R", "full_round": pick, "radius": 1, "edges": [{"midpoint": [5, 0, 10]}]}),
        json!({"type": "chamfer", "name": "R", "full_round": pick, "distance": 1}),
        json!({"type": "fillet", "name": "R", "full_round": {"point": [20, "x", 5]}}),
        // A point far from every face.
        json!({"type": "fillet", "name": "R", "full_round": {"point": [500, 500, 500], "normal": [1, 0, 0]}}),
    ] {
        let (mut s, r) = import_into_session(slab_tree(round.clone()));
        let skipped = r["skipped_user_text"].as_array().unwrap();
        assert_eq!(skipped.len(), 1, "{round}: {r}");
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
        assert_vol(b["volume_mm3"].as_f64().unwrap(), 800.0);
    }
}

// ---- Ellipses and elliptical arcs (ADR 0042 amendment) ----

fn ellipse_sketch(name: &str, plane: Value, x: f64, y: f64, major: [f64; 2], a: f64, b: f64) -> Value {
    json!({
        "type": "sketch", "name": name, "plane": plane,
        "points": [{"id": "c", "x": x, "y": y}],
        "curves": [{"id": "e", "type": "ellipse", "center": "c", "major": major, "a": a, "b": b}]
    })
}

#[test]
fn a_tilted_ellipse_extrudes_to_its_area_times_the_length() {
    let (_, v) = import_v2(vec![
        ellipse_sketch("E", plane_at(0.0), 1.0, 2.0, [0.6, 0.8], 5.0, 3.0),
        extrude("X", "E", 10.0, "new"),
    ]);
    assert_vol(v, std::f64::consts::PI * 5.0 * 3.0 * 10.0);
}

#[test]
fn an_o_ring_section_revolves_to_a_torus_like_ring() {
    // Inventor's 10x5 O-ring: centre x = 7.85, a = 3.24 along the sketch's y axis, b = 2.15,
    // on an XZ sketch, turned about world Z (the sketch's y axis): V = 2 pi R * pi a b.
    let xz = json!({"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 0, 1]});
    let (r, a, b) = (7.85, 3.24, 2.15);
    let (_, v) = import_v2(vec![
        ellipse_sketch("S", xz, r, 0.0, [0.0, 1.0], a, b),
        json!({"type": "revolve", "name": "R", "sketch": "S", "profiles": "all",
               "axis": {"origin": [0, 0, 0], "direction": [0, 0, -1]}, "angle": 360, "operation": "new"}),
    ]);
    let pi = std::f64::consts::PI;
    assert_vol(v, 2.0 * pi * r * pi * a * b);
}

#[test]
fn an_elliptical_arc_joins_a_line_on_either_side_of_the_plane() {
    // The upper half of x^2/16 + y^2/4 = 1 (from (4,0) to (-4,0), counter-clockwise about
    // the normal) closed by the major axis. On the flipped plane (y axis reversed) the same
    // source arc runs the other way round in Gusset's frame.
    for y_axis in [[0, 1, 0], [0, -1, 0]] {
        let plane = json!({"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": y_axis});
        let sk = json!({
            "type": "sketch", "name": "H", "plane": plane,
            "points": [{"id": "c", "x": 0, "y": 0}, {"id": "s", "x": 4, "y": 0}, {"id": "e", "x": -4, "y": 0}],
            "curves": [
                {"id": "q", "type": "ellipse_arc", "center": "c", "major": [1, 0], "a": 4, "b": 2, "start": "s", "end": "e"},
                {"id": "l", "type": "line", "start": "e", "end": "s"}
            ]
        });
        let (_, v) = import_v2(vec![sk, extrude("X", "H", 3.0, "new")]);
        assert_vol(v, std::f64::consts::PI * 4.0 * 3.0);
    }
}

#[test]
fn a_bad_ellipse_is_skipped_with_a_reason() {
    let mut e = ellipse_sketch("E", plane_at(0.0), 0.0, 0.0, [1.0, 0.0], 5.0, -3.0);
    e["curves"][0]["b"] = json!(-3.0);
    let (_, r) = import_into_session(vec![e]);
    assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 1, "{r}");
}

/// A 40 x 20 x 10 box as the kernel's native B-rep bytes.
fn native_box() -> Vec<u8> {
    use gusset_kernel::{ExtrudeSpec, Frame, Kernel, Loop2, Profile, ProfileEdge, Region2, Seg2};
    let pts = [[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]];
    let edges = (0..4)
        .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: 10 + i as u64 })
        .collect();
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let k = gusset_kernel_occt::OcctKernel::new().unwrap();
    let r = k.extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    k.write_native(&r.shape).unwrap()
}

#[test]
fn a_stored_body_is_imported_and_later_features_build_on_it() {
    // A frozen base body (a Fusion base feature, a SolidWorks stock body), a boss joined on
    // its top, then the whole body mirrored across x = 0 and merged.
    let tree = json!({
        "format": "gusset-feature-tree", "version": 2, "source": {"app": "test"},
        "features": [
            {"type": "stored_body", "name": "Base", "body": 0},
            disc("Top", plane_at(10.0), 20.0, 10.0, 5.0),
            {"type": "extrude", "name": "Boss", "sketch": "Top", "profiles": "all", "distance": 4.0,
             "operation": "join", "target": "Base"},
            {"type": "mirror", "name": "Both", "target": "Base", "merge": true, "features": [],
             "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}}
        ]
    });
    let mut t = gusset_commands::tree_import::parse(&serde_json::to_vec(&tree).unwrap()).unwrap();
    t.stored_bodies.push(native_box());
    let mut s = session();
    let r = gusset_commands::tree_import::apply(&mut s, &CmdCtx::user(1), &t, "t.gtree.json").unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
    let one = 8000.0 + std::f64::consts::PI * 25.0 * 4.0;
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 2.0 * one);
}

#[test]
fn a_tree_file_alone_cannot_bring_in_a_stored_body() {
    let (r, _) = {
        let tree = json!({"format": "gusset-feature-tree", "version": 2, "source": {"app": "test"},
            "features": [{"type": "stored_body", "name": "Base", "body": 0}, rect("S", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
                         extrude("E", "S", 5.0, "new")]});
        let mut s = session();
        std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap())
            .unwrap();
        (execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap(), ())
    };
    let skipped = r["skipped_user_text"].to_string();
    assert!(skipped.contains("native importer"), "{r}");
}

#[test]
fn refine_false_keeps_the_split_faces_of_a_join() {
    // Two touching 10 mm cubes: refined they merge into one box (6 faces); `refine: false`
    // keeps the four faces around the seam split (10 faces).
    let tree = |refine: Option<bool>| {
        let mut join = extrude("Box2", "Side", 10.0, "join");
        join["target"] = json!("Box");
        if let Some(r) = refine {
            join["refine"] = json!(r);
        }
        vec![
            rect("Base", plane_at(0.0), 0.0, 0.0, 10.0, 10.0),
            extrude("Box", "Base", 10.0, "new"),
            rect("Side", plane_at(0.0), 10.0, 0.0, 10.0, 10.0),
            join,
        ]
    };
    for (refine, faces, line_has) in [(None, 6u64, false), (Some(true), 6, false), (Some(false), 10, true)] {
        let (mut s, r) = import_v2_session(tree(refine));
        assert_eq!(r["skipped_user_text"], json!([]), "{r}");
        let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
        assert_eq!(text.contains("refine: false"), line_has, "{text}");
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
        assert_eq!(b["faces"].as_u64(), Some(faces), "{b}");
    }
}

#[test]
fn refine_false_on_a_pattern_keeps_the_seams_between_copies() {
    // Three touching 10 mm cubes from a linear pattern: 6 faces refined, 14 unrefined.
    let tree = |refine: bool| {
        let mut p = json!({"type": "pattern_linear", "name": "Row", "target": "Box", "direction": [1, 0, 0],
            "spacing": 10, "count": 3});
        if !refine {
            p["refine"] = json!(false);
        }
        vec![rect("Base", plane_at(0.0), 0.0, 0.0, 10.0, 10.0), extrude("Box", "Base", 10.0, "new"), p]
    };
    for (refine, faces) in [(true, 6u64), (false, 14)] {
        let (mut s, _) = import_v2_session(tree(refine));
        let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
        assert_eq!(text.contains("refine: false"), !refine, "{text}");
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
        assert_eq!(b["faces"].as_u64(), Some(faces), "{b}");
    }
}

// ---- Extrudes up to a face (ADR 0042 amendment) ----

fn to_face_tree(post: Value) -> Vec<Value> {
    vec![
        rect("PlateSketch", plane_at(0.0), 0.0, 0.0, 40.0, 20.0),
        extrude("Plate", "PlateSketch", 10.0, "new"),
        rect("RoofSketch", plane_at(30.0), 0.0, 0.0, 40.0, 20.0),
        extrude("Roof", "RoofSketch", 10.0, "new"),
        rect("PostSketch", plane_at(0.0), 5.0, 5.0, 10.0, 10.0),
        post,
    ]
}

fn to_face_post(extra: Value) -> Value {
    let mut p = json!({
        "type": "extrude", "name": "Post", "sketch": "PostSketch", "profiles": "all", "operation": "new",
        "end_condition": "to_face", "face": {"point": [20, 10, 30], "normal": [0, 0, -1]}
    });
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    p
}

#[test]
fn an_extrude_to_face_ends_on_the_picked_face_with_its_offset() {
    // The roof's underside is the plane z = 30; the post starts at z = 0.
    for (extra, volume, args) in [
        (json!({}), 3000.0, "to: Roof/cap(start)"),
        (json!({"offset": 2}), 3200.0, "to: Roof/cap(start), offset: 2 mm"),
        (json!({"offset": -5}), 2500.0, "to: Roof/cap(start), offset: -5 mm"),
    ] {
        let (mut s, r) = import_into_session(to_face_tree(to_face_post(extra.clone())));
        assert_eq!(r["skipped_user_text"], json!([]), "{extra}: {r}");
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Post"})).unwrap();
        assert_vol(b["volume_mm3"].as_f64().unwrap(), volume);
        let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
        let line = text.lines().find(|l| l.starts_with("extrude Post")).unwrap_or_else(|| panic!("{text}"));
        assert!(line.contains(args) && !line.contains("distance"), "{line}");
    }
}

#[test]
fn an_extrude_to_face_can_cut_a_pocket_and_reverse() {
    // A pocket from the plate's top down to its underside, 3 mm short: the sketch is on the
    // top (z = 10), the sweep runs against the sketch normal.
    let pocket = json!({
        "type": "extrude", "name": "Pocket", "sketch": "PocketSketch", "profiles": "all", "operation": "cut",
        "target": "Plate", "end_condition": "to_face", "face": {"point": [20, 10, 0], "normal": [0, 0, -1]},
        "offset": -3, "reverse": true
    });
    let (mut s, r) = import_into_session(vec![
        rect("PlateSketch", plane_at(0.0), 0.0, 0.0, 40.0, 20.0),
        extrude("Plate", "PlateSketch", 10.0, "new"),
        rect("PocketSketch", plane_at(10.0), 5.0, 5.0, 10.0, 10.0),
        pocket,
    ]);
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({})).unwrap();
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 8000.0 - 700.0);
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("to: Plate/cap(start), offset: -3 mm, reverse: true, op: cut"), "{text}");
}

#[test]
fn bad_extrudes_to_a_face_are_skipped_with_a_reason() {
    for extra in [
        json!({"face": null}),
        json!({"symmetric": true}),
        json!({"end_condition": "up_to_next"}),
        // Behind the sketch: the roof's top is not ahead of a sweep that ends on... the plate's underside.
        json!({"face": {"point": [20, 10, 0], "normal": [0, 0, -1]}}),
        json!({"face": {"point": [500, 500, 500], "normal": [0, 0, -1]}}),
    ] {
        let (mut s, r) = import_into_session(to_face_tree(to_face_post(extra.clone())));
        let skipped = r["skipped_user_text"].as_array().unwrap();
        assert_eq!(skipped.len(), 1, "{extra}: {r}");
        let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Roof"})).unwrap();
        assert_vol(b["volume_mm3"].as_f64().unwrap(), 40.0 * 20.0 * 10.0);
    }
}

// ---- Whole-body edits: move_body, scale_body, remove_body, draft (ADR 0042 amendment) ----

fn box_tree(w: f64, d: f64, h: f64) -> Vec<Value> {
    vec![rect("Base", plane_at(0.0), 0.0, 0.0, w, d), extrude("Box", "Base", h, "new")]
}

fn body_of(s: &mut Session, name: Option<&str>) -> Value {
    execute(s, &CmdCtx::user(1), "query.body", json!({"body": name})).unwrap()
}

fn close3(got: &Value, want: [f64; 3]) {
    for (i, w) in want.iter().enumerate() {
        let g = got[i].as_f64().unwrap();
        assert!((g - w).abs() < 1e-6, "centroid {got}, expected {want:?}");
    }
}

#[test]
fn a_moved_body_keeps_its_tree_name_for_later_features() {
    // A 10 mm cube lifted 5 mm; a boss joined on its new top targets the old name.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "move_body", "name": "Lift", "target": "Box", "translate": [0, 0, 5]}));
    f.push(disc("Top", plane_at(15.0), 5.0, 5.0, 2.0));
    f.push(json!({"type": "extrude", "name": "Boss", "sketch": "Top", "profiles": "all", "distance": 3.0,
                  "operation": "join", "target": "Box"}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0 + std::f64::consts::PI * 4.0 * 3.0);
    assert!((b["bbox_min_mm"][2].as_f64().unwrap() - 5.0).abs() < 1e-6, "{b}");
}

#[test]
fn a_body_turned_about_an_axis_then_translated() {
    // A 20 x 10 x 10 block turned 90 degrees about the vertical axis through (10, 5, 0)
    // (it now spans x 5..15, y -5..15), then moved 100 mm along x.
    let mut f = box_tree(20.0, 10.0, 10.0);
    f.push(json!({"type": "move_body", "name": "Turn", "target": "Box", "translate": [100, 0, 0],
                  "rotate": {"axis": [0, 0, 1], "origin": [10, 5, 0], "angle": 90}}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 2000.0);
    close3(&b["centroid_mm"], [110.0, 5.0, 5.0]);
}

#[test]
fn a_copied_body_leaves_the_original_in_place() {
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "move_body", "name": "Twin", "target": "Box", "translate": [30, 0, 0], "copy": true}));
    let (mut s, _) = import_v2_session(f);
    let orig = body_of(&mut s, Some("Box"));
    close3(&orig["centroid_mm"], [5.0, 5.0, 5.0]);
    let twin = body_of(&mut s, Some("Twin"));
    assert_vol(twin["volume_mm3"].as_f64().unwrap(), 1000.0);
    close3(&twin["centroid_mm"], [35.0, 5.0, 5.0]);
}

#[test]
fn a_move_without_motion_is_skipped() {
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "move_body", "name": "Nothing", "target": "Box"}));
    f.push(json!({"type": "move_body", "name": "Lost", "target": "Nope", "translate": [1, 0, 0]}));
    let (_, r) = import_into_session(f);
    let skipped = r["skipped_user_text"].to_string();
    assert!(skipped.contains("neither a translation nor a rotation") && skipped.contains("was not imported"), "{r}");
}

#[test]
fn a_body_scaled_uniformly_about_a_point() {
    // 10 x 20 x 30 doubled about its own centre: 8 times the volume, centre unchanged.
    let mut f = box_tree(10.0, 20.0, 30.0);
    f.push(json!({"type": "scale_body", "name": "Big", "target": "Box", "factor": 2.0, "center": [5, 10, 15]}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 6000.0 * 8.0);
    close3(&b["centroid_mm"], [5.0, 10.0, 15.0]);
}

#[test]
fn a_scaled_body_about_the_origin_moves_away_from_it() {
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "scale_body", "name": "Half", "target": "Box", "factor": 0.5}));
    // A boss on the new top (z = 5) joins the scaled body by its old name.
    f.push(disc("Top", plane_at(5.0), 2.5, 2.5, 1.0));
    f.push(json!({"type": "extrude", "name": "Boss", "sketch": "Top", "profiles": "all", "distance": 2.0,
                  "operation": "join", "target": "Box"}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 125.0 + std::f64::consts::PI * 2.0);
}

#[test]
fn a_body_scaled_by_a_factor_per_axis() {
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "scale_body", "name": "Stretch", "target": "Box", "factors": [2.0, 1.0, 0.5]}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0);
    close3(&b["centroid_mm"], [10.0, 5.0, 2.5]);
}

#[test]
fn bad_scales_are_skipped_with_a_reason() {
    for bad in [
        json!({"factor": 0.0}),
        json!({"factor": -2.0}),
        json!({"factors": [1.0, 0.0, 1.0]}),
        json!({}),
        json!({"factor": 2.0, "factors": [1.0, 1.0, 1.0]}),
        json!({"factor": 1e300, "center": [0, 0, 0]}),
    ] {
        let mut f = box_tree(10.0, 10.0, 10.0);
        let mut s = json!({"type": "scale_body", "name": "Bad", "target": "Box"});
        s.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        f.push(s);
        let (_, r) = import_into_session(f);
        assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 1, "{bad}: {r}");
    }
}

#[test]
fn a_removed_body_is_gone_and_cannot_be_named_again() {
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(disc("Round", plane_at(0.0), 20.0, 5.0, 2.0));
    f.push(extrude("Pin", "Round", 10.0, "new"));
    f.push(json!({"type": "remove_body", "name": "Drop", "target": "Pin"}));
    f.push(json!({"type": "move_body", "name": "Again", "target": "Pin", "translate": [1, 0, 0]}));
    let (mut s, r) = import_into_session(f);
    let skipped = r["skipped_user_text"].to_string();
    assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 1, "{r}");
    assert!(skipped.contains("removed by"), "{r}");
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0);
    assert!(execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Pin"})).is_err());
}

#[test]
fn removing_a_modified_body_removes_it_under_every_name() {
    // The cube was joined with a boss (still "Box"), then dropped; the pin stays.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(disc("Round", plane_at(0.0), 20.0, 5.0, 2.0));
    f.push(extrude("Pin", "Round", 10.0, "new"));
    f.push(json!({"type": "scale_body", "name": "Grow", "target": "Box", "factor": 2.0}));
    f.push(json!({"type": "remove_body", "name": "Drop", "target": "Grow"}));
    f.push(json!({"type": "scale_body", "name": "Late", "target": "Box", "factor": 2.0}));
    let (mut s, r) = import_into_session(f);
    assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 1, "{r}");
    assert!(r["skipped_user_text"].to_string().contains("removed by"), "{r}");
    assert_vol(body_of(&mut s, None)["volume_mm3"].as_f64().unwrap(), std::f64::consts::PI * 4.0 * 10.0);
}

#[test]
fn a_drafted_side_face_leans_inward() {
    // The +x wall of a 10 mm cube turns 5 degrees about its foot on z = 0, narrowing the
    // cube upwards: a wedge of 10 * (10 tan 5) / 2 over the 10 mm depth is gone.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "draft", "name": "Taper", "faces": [{"point": [10, 5, 5], "normal": [1, 0, 0]}],
                  "plane": {"origin": [0, 0, 0], "normal": [0, 0, 1]}, "angle": 5}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    let want = 1000.0 - 500.0 * libm::tan(5.0_f64.to_radians());
    assert_vol(b["volume_mm3"].as_f64().unwrap(), want);
}

#[test]
fn a_draft_pulls_along_its_plane_normal_whichever_way_the_base_plane_faces() {
    // Neutral plane y = 0 pulled along +y (the XZ base plane's normal is -y): the +x wall
    // narrows towards +y, losing the same wedge as an upward pull from z = 0.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "draft", "name": "Taper", "faces": [{"point": [10, 5, 5], "normal": [1, 0, 0]}],
                  "plane": {"origin": [0, 0, 0], "normal": [0, 1, 0]}, "angle": 5}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0 - 500.0 * libm::tan(5.0_f64.to_radians()));
    // A negative angle leans the wall outward along the pull: the wedge is added.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "draft", "name": "Taper", "faces": [{"point": [10, 5, 5], "normal": [1, 0, 0]}],
                  "plane": {"origin": [0, 0, 0], "normal": [0, 1, 0]}, "angle": -5}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0 + 500.0 * libm::tan(5.0_f64.to_radians()));
    // An offset datum plane (y = 2) pulled along +y: 0.5 * 8 * 8 tan 5 * 10 lost above it,
    // 0.5 * 2 * 2 tan 5 * 10 gained below.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "draft", "name": "Taper", "faces": [{"point": [10, 5, 5], "normal": [1, 0, 0]}],
                  "plane": {"origin": [0, 2, 0], "normal": [0, 1, 0]}, "angle": 5}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), 1000.0 - 300.0 * libm::tan(5.0_f64.to_radians()));
}

#[test]
fn a_flipped_draft_on_an_offset_plane_leans_the_other_way() {
    // Neutral plane at z = 10 (the top), pulled the other way (down): the wall narrows
    // towards the foot, the mirror image of the first case, so the same volume is lost.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "draft", "name": "Taper", "flip": true,
                  "faces": [{"point": [10, 5, 5], "normal": [1, 0, 0]}],
                  "plane": {"origin": [0, 0, 10], "normal": [0, 0, 1]}, "angle": 5}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    let want = 1000.0 - 500.0 * libm::tan(5.0_f64.to_radians());
    assert_vol(b["volume_mm3"].as_f64().unwrap(), want);
}

#[test]
fn bad_drafts_are_skipped_with_a_reason() {
    for bad in [
        json!({"faces": []}),
        json!({"faces": [{"point": [10, 5, 5]}], "plane": {"origin": [0, 0, 0], "normal": [0, 0, 0]}}),
        json!({"faces": [{"point": [10, 5, 5]}], "angle": 95}),
    ] {
        let mut f = box_tree(10.0, 10.0, 10.0);
        let mut d = json!({"type": "draft", "name": "Bad", "faces": [{"point": [10, 5, 5]}],
                           "plane": {"origin": [0, 0, 0], "normal": [0, 0, 1]}, "angle": 5});
        d.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        f.push(d);
        let (_, r) = import_into_session(f);
        assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 1, "{bad}: {r}");
    }
}

#[test]
fn an_offset_face_pushes_and_pulls_along_the_normal() {
    // The top of a 10 mm cube lifted by 2 mm, then its +x wall pulled in by 1 mm; the body
    // keeps its name for the second edit.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "offset_face", "name": "Lift", "faces": [{"point": [5, 5, 10], "normal": [0, 0, 1]}],
                  "distance": 2}));
    f.push(json!({"type": "offset_face", "name": "Trim", "faces": [{"point": [10, 5, 5], "normal": [1, 0, 0]}],
                  "distance": -1}));
    let (mut s, _) = import_v2_session(f);
    assert_vol(body_of(&mut s, None)["volume_mm3"].as_f64().unwrap(), 9.0 * 10.0 * 12.0);
}

#[test]
fn bad_offset_faces_are_skipped_with_a_reason() {
    for bad in [json!({"faces": []}), json!({"distance": 0}), json!({"faces": [{"point": [50, 50, 50]}]})] {
        let mut f = box_tree(10.0, 10.0, 10.0);
        let mut d = json!({"type": "offset_face", "name": "Bad", "faces": [{"point": [5, 5, 10]}], "distance": 1});
        d.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        f.push(d);
        let (_, r) = import_into_session(f);
        assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 1, "{bad}: {r}");
    }
}

#[test]
fn the_whole_body_edits_need_a_version_2_file() {
    let tree = json!({"format": "gusset-feature-tree", "version": 1, "source": {"app": "test"},
        "features": [{"type": "remove_body", "name": "R", "target": "X"}]});
    let mut s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("t.gtree.json"), serde_json::to_vec(&tree).unwrap()).unwrap();
    let r = execute(&mut s, &CmdCtx::user(1), "import.file", json!({"path": "t.gtree.json"})).unwrap();
    assert!(r["skipped_user_text"].to_string().contains("version 2"), "{r}");
}

// ---- Exact B-spline sketch curves (ADR 0042 amendment) ----

/// The circle of radius `r` around (x, y) as the tree's clamped rational quadratic: nine
/// poles (the last is the first point again), knots 0 0 0 1 1 2 2 3 3 4 4 4, weights 1, √2/2.
fn nurbs_circle_sketch(name: &str, plane: Value, x: f64, y: f64, r: f64) -> Value {
    let q = [[r, 0.0], [r, r], [0.0, r], [-r, r], [-r, 0.0], [-r, -r], [0.0, -r], [r, -r]];
    let points: Vec<Value> =
        q.iter().enumerate().map(|(i, p)| json!({"id": format!("q{i}"), "x": x + p[0], "y": y + p[1]})).collect();
    let h = std::f64::consts::FRAC_1_SQRT_2;
    json!({
        "type": "sketch", "name": name, "plane": plane, "points": points,
        "curves": [{"id": "n", "type": "bspline", "degree": 2,
                    "knots": [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 4],
                    "poles": ["q0", "q1", "q2", "q3", "q4", "q5", "q6", "q7", "q0"],
                    "weights": [1, h, 1, h, 1, h, 1, h, 1], "periodic": false}]
    })
}

#[test]
fn a_nurbs_circle_extrudes_to_a_cylinder() {
    let (r, v) =
        import_v2(vec![nurbs_circle_sketch("C", plane_at(0.0), 3.0, -2.0, 5.0), extrude("X", "C", 8.0, "new")]);
    assert_vol(v, std::f64::consts::PI * 25.0 * 8.0);
    let _ = r;
}

#[test]
fn a_periodic_quadratic_bspline_on_a_flipped_plane_has_its_analytic_area() {
    // Uniform periodic quadratic over the square of corners (±6, ±6): area 10 a² / 3. On a
    // plane whose y axis is reversed the poles are mirrored; the region is the same.
    let a = 6.0;
    for y_axis in [[0, 1, 0], [0, -1, 0]] {
        let plane = json!({"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": y_axis});
        let sk = json!({
            "type": "sketch", "name": "P", "plane": plane,
            "points": [{"id": "a", "x": a, "y": a}, {"id": "b", "x": -a, "y": a},
                       {"id": "c", "x": -a, "y": -a}, {"id": "d", "x": a, "y": -a}],
            "curves": [{"id": "s", "type": "bspline", "degree": 2, "knots": [0, 1, 2, 3, 4],
                        "poles": ["a", "b", "c", "d"], "periodic": true}]
        });
        let (_, v) = import_v2(vec![sk, extrude("X", "P", 3.0, "new")]);
        assert_vol(v, 10.0 * a * a / 3.0 * 3.0);
    }
}

#[test]
fn a_clamped_cubic_closed_by_a_line_extrudes_to_its_area() {
    // Fusion's typical record: degree 3, four poles, knots 0 0 0 0 1 1 1 1, unit weights.
    let p = [[0.0, 0.0], [2.0, 6.0], [8.0, 6.0], [10.0, 0.0]];
    let sk = json!({
        "type": "sketch", "name": "D", "plane": plane_at(0.0),
        "points": [{"id": "a", "x": p[0][0], "y": p[0][1]}, {"id": "b", "x": p[1][0], "y": p[1][1]},
                   {"id": "c", "x": p[2][0], "y": p[2][1]}, {"id": "d", "x": p[3][0], "y": p[3][1]}],
        "curves": [{"id": "s", "type": "bspline", "degree": 3, "knots": [0, 0, 0, 0, 1, 1, 1, 1],
                    "poles": ["a", "b", "c", "d"], "weights": [1, 1, 1, 1]},
                   {"id": "l", "type": "line", "start": "d", "end": "a"}]
    });
    // Area under the Bézier y(t) dx(t) from x = 0 to 10: by Simpson on a fine grid.
    let n = 20000;
    let bez = |t: f64| {
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        let dw = [-3.0 * u * u, 3.0 * u * u - 6.0 * u * t, 6.0 * u * t - 3.0 * t * t, 3.0 * t * t];
        let y: f64 = (0..4).map(|i| w[i] * p[i][1]).sum();
        let dx: f64 = (0..4).map(|i| dw[i] * p[i][0]).sum();
        y * dx
    };
    let h = 1.0 / f64::from(n);
    let area: f64 = (0..=n)
        .map(|k| {
            let c = if k == 0 || k == n {
                1.0
            } else if k % 2 == 1 {
                4.0
            } else {
                2.0
            };
            c * bez(f64::from(k) * h)
        })
        .sum::<f64>()
        * h
        / 3.0;
    let (_, v) = import_v2(vec![sk, extrude("X", "D", 2.0, "new")]);
    assert_vol(v, area * 2.0);
}

#[test]
fn a_shelled_nurbs_cylinder_names_its_span_faces() {
    // Open at the top: pi r^2 h - pi (r - t)^2 (h - t).
    let (r, h, t) = (5.0, 8.0, 1.0);
    let f = vec![
        nurbs_circle_sketch("C", plane_at(0.0), 0.0, 0.0, r),
        extrude("X", "C", h, "new"),
        json!({"type": "shell", "name": "Cup", "faces": [{"point": [0, 0, h], "normal": [0, 0, 1]}],
               "thickness": t, "side": "inside"}),
    ];
    let (mut s, _) = import_v2_session(f);
    let pi = std::f64::consts::PI;
    let b = body_of(&mut s, None);
    assert_vol(b["volume_mm3"].as_f64().unwrap(), pi * r * r * h - pi * (r - t) * (r - t) * (h - t));
    // One lateral face per knot span, named by the B-spline and the span (`b9_g0` .. `b9_g3`);
    // the shell's inner faces are named after them.
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("bspline(degree: 2, poles: [p1, p2, p3, p4, p5, p6, p7, p8, p1]"), "{text}");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"names": true})).unwrap();
    let names = b["face_names"].to_string();
    for g in 0..4 {
        assert!(names.contains(&format!("/side(C/b9_g{g})\"")), "{names}");
        assert!(names.contains(&format!("Cup/inner(X_2/side(C/b9_g{g}))")), "{names}");
    }
}

#[test]
fn bad_bsplines_are_skipped_with_a_reason() {
    for (key, bad) in [
        ("degree", json!(0)),
        ("knots", json!([0, 0, 0, 1, 1])),
        ("weights", json!([1, -1, 1, 1, 1, 1, 1, 1, 1])),
        ("poles", json!(["q0", "q1", "zz"])),
        ("knots", json!([0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, "x"])),
    ] {
        let mut sk = nurbs_circle_sketch("C", plane_at(0.0), 0.0, 0.0, 5.0);
        sk["curves"][0][key] = bad.clone();
        let (_, r) = import_into_session(vec![sk, extrude("X", "C", 1.0, "new")]);
        assert!(!r["skipped_user_text"].as_array().unwrap().is_empty(), "{key} {bad}: {r}");
    }
}

// ---- Body patterns that keep their copies apart (ADR 0042 amendment) ----

#[test]
fn an_unmerged_pattern_leaves_copies_that_later_features_target() {
    // Three 10 mm cubes 20 mm apart, kept as three bodies: copy 2 is lifted 5 mm, a boss
    // joined on its new top, copy 1 removed; the original stays where it was.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "pattern_linear", "name": "Row", "target": "Box", "direction": [1, 0, 0],
                  "spacing": 20, "count": 3, "merge": false}));
    f.push(json!({"type": "move_body", "name": "Lift", "target": "Row[2]", "translate": [0, 0, 5]}));
    f.push(disc("Top", plane_at(15.0), 45.0, 5.0, 2.0));
    f.push(json!({"type": "extrude", "name": "Boss", "sketch": "Top", "profiles": "all", "distance": 3.0,
                  "operation": "join", "target": "Row[2]"}));
    f.push(json!({"type": "remove_body", "name": "Drop", "target": "Row[1]"}));
    let (mut s, _) = import_v2_session(f);
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("count: 3, spacing: 20 mm, merge: false)"), "{text}");
    assert!(text.contains("move Lift (body: Row_copy2,") && text.contains("remove Drop (body: Row_copy1)"), "{text}");
    let orig = body_of(&mut s, Some("Box"));
    assert_vol(orig["volume_mm3"].as_f64().unwrap(), 1000.0);
    close3(&orig["centroid_mm"], [5.0, 5.0, 5.0]);
    let copy = body_of(&mut s, Some("Row_copy2"));
    assert_vol(copy["volume_mm3"].as_f64().unwrap(), 1000.0 + std::f64::consts::PI * 4.0 * 3.0);
    assert!((copy["bbox_min_mm"][0].as_f64().unwrap() - 40.0).abs() < 1e-6, "{copy}");
    assert!((copy["bbox_min_mm"][2].as_f64().unwrap() - 5.0).abs() < 1e-6, "{copy}");
    let gone = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Row_copy1"}));
    assert!(gone.is_err(), "{gone:?}");
    // A circular one too, and an unmerged pattern of features is refused.
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "pattern_circular", "name": "Ring", "target": "Box",
                  "axis": {"origin": [-20, 5, 0], "direction": [0, 0, 1]}, "angle": 360, "count": 4, "merge": false}));
    f.push(json!({"type": "remove_body", "name": "Drop", "target": "Ring[3]"}));
    let (mut s, _) = import_v2_session(f);
    for b in ["Box", "Ring_copy1", "Ring_copy2"] {
        assert_vol(body_of(&mut s, Some(b))["volume_mm3"].as_f64().unwrap(), 1000.0);
    }
    let mut f = box_tree(10.0, 10.0, 10.0);
    f.push(json!({"type": "pattern_linear", "name": "Row", "features": ["Box"], "direction": [1, 0, 0],
                  "spacing": 20, "count": 2, "merge": false}));
    let (_, r) = import_into_session(f);
    assert!(r["skipped_user_text"].to_string().contains("merge: false"), "{r}");
}

#[test]
fn a_tree_thread_cuts_its_groove_or_records_a_cosmetic_one() {
    // A 16.4 mm rod 13 mm up Z with 1145's M16x1 thread, 9 mm from 4 mm: the rod cut down to
    // the major diameter, the groove's section turned about the axis per pitch over 9 mm.
    let rod = || vec![disc("S", plane_at(0.0), 0.0, 0.0, 8.2), extrude("Rod", "S", 13.0, "new")];
    let thread = |modeled: bool| {
        json!({"type": "thread", "name": "Thread1", "face": {"point": [8.2, 0, 6.5], "normal": [1, 0, 0]},
            "pitch": 1, "major": 15.884, "minor": 14.741, "angle": 60, "length": 9, "offset": 4,
            "along": [0, 0, 1], "modeled": modeled, "designation": "M16x1 6g"})
    };
    let (rmaj, rmin, p) = (7.942_f64, 7.3705_f64, 1.0_f64);
    let w = (rmaj - rmin) * libm::tan(30f64.to_radians());
    // The groove's width at r is linear (7P/8 at the major radius): Simpson's rule is exact.
    let width = |r: f64| 7.0 * p / 8.0 - 2.0 * w * (rmaj - r) / (rmaj - rmin);
    let m = |r: f64| r * width(r);
    let moment = (rmaj - rmin) / 6.0 * (m(rmin) + 4.0 * m((rmaj + rmin) / 2.0) + m(rmaj));
    let want = std::f64::consts::PI * rmaj * rmaj * 13.0 - 9.0 * moment * std::f64::consts::TAU / p;
    let mut f = rod();
    f.push(thread(true));
    let (r, v) = import_v2(f);
    assert!((v - want).abs() < 2e-5 * want, "volume {v}, expected {want}");
    assert!(r["features"].to_string().contains("Thread1"), "{r}");
    let mut f = rod();
    f.push(thread(false));
    let (_, v) = import_v2(f);
    assert_vol(v, std::f64::consts::PI * 8.2 * 8.2 * 13.0);
}

#[test]
fn a_rounded_edge_recorded_before_an_offset_moves_with_its_face() {
    // 1054: a plug of r 1.5 mm made 1.4 mm by an offset, then its rim rounded by an edge
    // recorded on the 1.5 mm cylinder.
    let circle: Vec<Value> = (0..16)
        .map(|i| {
            let t = f64::from(i) * std::f64::consts::TAU / 16.0;
            json!([1.5 * libm::cos(t), 1.5 * libm::sin(t), 1.0])
        })
        .collect();
    let f = vec![
        disc("S", plane_at(0.0), 0.0, 0.0, 1.5),
        extrude("Plug", "S", 1.0, "new"),
        json!({"type": "offset_face", "name": "Offset1", "distance": -0.1, "on_body": "Plug",
            "faces": [{"point": [1.5, 0, 0.5],
                "surface": {"type": "cylinder", "origin": [0, 0, 0], "axis": [0, 0, 1], "radius": 1.5}}]}),
        json!({"type": "fillet", "name": "Fillet1", "radius": 0.2, "on_body": "Plug",
            "edges": [{"midpoint": [-1.5, 0, 1.0], "path": circle}]}),
    ];
    let (r, v) = import_v2(f);
    assert!(r["features"].to_string().contains("Fillet1"), "{r}");
    // The rim round takes (1 - pi / 4) r^2 of the section, turned about the axis at a radius
    // under 1.4 mm.
    let plain = std::f64::consts::PI * 1.4 * 1.4;
    let most = (1.0 - std::f64::consts::FRAC_PI_4) * 0.04 * std::f64::consts::TAU * 1.4;
    assert!(v < plain && v > plain - most, "{v}");

// ---- Face profiles, surface extrudes, Thicken, edge paths (section 49 of the Fusion notes) ----

fn volume_of(s: &mut Session, body: &str) -> f64 {
    let b = execute(s, &CmdCtx::user(1), "query.body", json!({"body": body})).unwrap();
    b["volume_mm3"].as_f64().unwrap()
}

#[test]
fn extrudes_of_body_faces() {
    // The plate's top raised 5 mm, then 2 mm cut into its front (y = 0) face.
    let (mut s, _) = import_v2_session(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 40.0, 20.0),
        extrude("Plate", "Sketch1", 10.0, "new"),
        json!({"type": "extrude", "name": "Raise", "profile_face": {"point": [20, 10, 10], "normal": [0, 0, 1]},
               "distance": 5, "operation": "join", "target": "Plate"}),
        json!({"type": "extrude", "name": "Shave", "profile_face": {"point": [20, 0, 7]},
               "distance": -2, "operation": "cut", "target": "Plate"}),
    ]);
    assert_vol(volume_of(&mut s, "Plate"), 40.0 * 18.0 * 15.0);
}

#[test]
fn a_sheet_thickened_into_a_body_and_repeated() {
    // A wall 30 mm long and 5 high swept up from a line on the plate's top, thickened 2 mm (1 to
    // each side) into the plate; the pattern turns the pair a quarter about the plate's middle:
    // a cross of two walls, 2 x 2 x 5 shared.
    let line = json!({"type": "sketch", "name": "Line", "plane": plane_at(10.0),
        "points": [{"id": "a", "x": 5, "y": 10}, {"id": "b", "x": 35, "y": 10}],
        "curves": [{"id": "l1", "type": "line", "start": "a", "end": "b"}]});
    let (mut s, _) = import_v2_session(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 40.0, 20.0),
        extrude("Plate", "Sketch1", 10.0, "new"),
        line,
        json!({"type": "extrude", "name": "Wall", "sketch": "Line", "curves": ["l1"], "distance": 5,
               "operation": "new"}),
        json!({"type": "thicken", "name": "Thicken1", "sheets": ["Wall"], "thickness": 2, "symmetric": true,
               "operation": "join", "target": "Plate"}),
        json!({"type": "pattern_circular", "name": "Turn", "features": ["Wall", "Thicken1"], "count": 2,
               "angle": 90, "axis": {"origin": [20, 10, 0], "direction": [0, 0, 1]}}),
    ]);
    assert_vol(volume_of(&mut s, "Plate"), 8000.0 + 2.0 * 300.0 - 20.0);
}

#[test]
fn a_loft_from_a_face_and_a_sweep_along_an_edge() {
    let (mut s, _) = import_v2_session(vec![
        rect("Sketch1", plane_at(0.0), 0.0, 0.0, 40.0, 20.0),
        extrude("Plate", "Sketch1", 10.0, "new"),
        disc("Top", plane_at(20.0), 20.0, 10.0, 5.0),
        json!({"type": "loft", "name": "Spire", "sketches": [{"face": {"point": [20, 10, 10]}}, "Top"],
               "operation": "new"}),
        disc("Ring", json!({"origin": [0, 0, 0], "x_axis": [0, 1, 0], "y_axis": [0, 0, 1]}), 0.0, 10.0, 1.0),
        json!({"type": "sweep", "name": "Rail", "sketch": "Ring", "path_edges": [{"midpoint": [20, 0, 10]}],
               "orientation": "fixed", "operation": "new"}),
    ]);
    // The loft lies between the plate's top (800 mm2) and the disc (25 pi) 10 mm above.
    let v = volume_of(&mut s, "Spire");
    assert!(v > 25.0 * std::f64::consts::PI * 10.0 && v < 8000.0, "{v}");
    assert_vol(volume_of(&mut s, "Rail"), std::f64::consts::PI * 40.0);

/// A sketch with no curves (one that only holds text).
fn bare_sketch(name: &str, plane: Value) -> Value {
    json!({"type": "sketch", "name": name, "plane": plane, "points": [], "curves": []})
}

#[test]
fn a_sketch_text_is_cut_in_a_font_gusset_has_and_the_missing_font_is_named() {
    // "GUSSET" cut 1 mm into the top of a 60 x 20 x 10 block in "Arial", which the file does not
    // hold: the text comes in as a `text` feature in sans, with a warning that names both.
    let mut f = box_tree(60.0, 20.0, 10.0);
    f.push(bare_sketch("Labels", plane_at(10.0)));
    f.push(json!({"type": "text", "name": "Label", "sketch": "Labels", "text": "GUSSET", "font": "sans",
                  "source_font": "Arial", "height": 5, "at": [5, 7], "direction": [1, 0], "align": "left",
                  "distance": -1, "operation": "cut", "target": "Box"}));
    let (mut s, r) = import_v2_session(f);
    let w = r["warnings_user_text"].to_string();
    assert!(w.contains("'Arial' is not in the file") && w.contains("sans"), "{r}");
    let v = body_of(&mut s, None)["volume_mm3"].as_f64().unwrap();
    assert!(v < 12_000.0 - 10.0 && v > 12_000.0 - 150.0, "{v}");
    let text = s.document(&CmdCtx::user(1).actor).unwrap().to_text();
    // Sans is the default font: the text does not spell it out.
    assert!(text.contains("text Label (text: \"GUSSET\", plane: Label_plane, at: [5, 7]"), "{text}");
}

#[test]
fn a_text_on_a_sketch_facing_down_reads_the_right_way_round() {
    // The sketch on the block's bottom faces down (its y runs along -y). The text's up is the
    // sketch's y: the letters lie on the -y side of the baseline at y = 12, so the material goes
    // from below the block's middle (y = 10) and the centroid moves up. Drawn on a plane facing
    // up the text would read mirrored and lie above the baseline.
    let mut f = box_tree(60.0, 20.0, 10.0);
    f.push(bare_sketch("Under", json!({"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, -1, 0]})));
    f.push(json!({"type": "text", "name": "Mark", "sketch": "Under", "text": "EEE", "font": "mono",
                  "source_font": "Courier New", "height": 5, "at": [5, -12], "direction": [1, 0],
                  "distance": -2, "operation": "cut", "target": "Box"}));
    let (mut s, _) = import_v2_session(f);
    let b = body_of(&mut s, None);
    assert!(b["volume_mm3"].as_f64().unwrap() < 12_000.0 - 10.0, "{b}");
    assert!(b["centroid_mm"][1].as_f64().unwrap() > 10.0 + 1e-3, "{b}");
    assert!(b["centroid_mm"][0].as_f64().unwrap() > 30.0, "{b}");
}

#[test]
fn a_suppressed_text_is_kept_to_edit_and_changes_nothing() {
    let mut f = box_tree(60.0, 20.0, 10.0);
    f.push(bare_sketch("Labels", plane_at(10.0)));
    f.push(json!({"type": "text", "name": "Label text 1", "sketch": "Labels", "text": "+", "font": "sans",
                  "source_font": "Arial", "height": 3, "at": [5, 7], "direction": [1, 0], "align": "center",
                  "distance": -0.4, "operation": "cut", "target": "Box", "suppressed": true}));
    let (mut s, r) = import_v2_session(f);
    assert!(r["warnings_user_text"].to_string().contains("kept suppressed"), "{r}");
    assert_vol(body_of(&mut s, None)["volume_mm3"].as_f64().unwrap(), 12_000.0);
    let text = s.document(&CmdCtx::user(1).actor).unwrap().to_text();
    assert!(text.contains("suppressed: true"), "{text}");
}

#[test]
fn bad_texts_are_skipped_with_a_reason() {
    for bad in [
        json!({"text": ""}),
        json!({"text": "a\"b"}),
        json!({"font": "serif"}),
        json!({"height": 0}),
        json!({"sketch": "Nowhere"}),
        json!({"target": null}),
    ] {
        let mut f = box_tree(60.0, 20.0, 10.0);
        f.push(bare_sketch("Labels", plane_at(10.0)));
        let mut d = json!({"type": "text", "name": "Bad", "sketch": "Labels", "text": "A", "font": "sans",
                           "height": 5, "at": [5, 7], "direction": [1, 0], "distance": -1, "operation": "cut",
                           "target": "Box"});
        d.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        f.push(d);
        let (_, r) = import_into_session(f);
        assert_eq!(r["skipped_user_text"].as_array().unwrap().len(), 1, "{bad}: {r}");
    }
}
