//! Feature-tree import (`.gtree.json`, ADR 0042) through `import.file`.

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-tree-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.project_dir = Some(dir);
    s
}

fn import(s: &mut Session, name: &str, tree: &Value) -> Result<Value, CommandError> {
    let dir = s.project_dir.clone().unwrap();
    std::fs::write(dir.join(name), serde_json::to_vec_pretty(tree).unwrap()).unwrap();
    execute(s, &CmdCtx::user(1), "import.file", json!({ "path": name }))
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn rect(name: &str, plane: Value, w: f64, h: f64) -> Value {
    json!({
        "type": "sketch", "name": name, "plane": plane,
        "points": [
            {"id": "p1", "x": 0, "y": 0}, {"id": "p2", "x": w, "y": 0},
            {"id": "p3", "x": w, "y": h}, {"id": "p4", "x": 0, "y": h}
        ],
        "curves": [
            {"id": "l1", "type": "line", "start": "p1", "end": "p2"},
            {"id": "l2", "type": "line", "start": "p2", "end": "p3"},
            {"id": "l3", "type": "line", "start": "p3", "end": "p4"},
            {"id": "l4", "type": "line", "start": "p4", "end": "p1"}
        ],
        "constraints": []
    })
}

fn xy() -> Value {
    json!({"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 1, 0]})
}

fn tree(parameters: Value, features: Value, bodies: Value) -> Value {
    json!({
        "format": "gusset-feature-tree", "version": 1,
        "source": {"app": "fusion", "app_version": "test", "exporter": "hand-written", "document": "Plate"},
        "parameters": parameters, "features": features, "bodies": bodies
    })
}

fn volume(s: &mut Session) -> f64 {
    run(s, "query.body", json!({}))["volume_mm3"].as_f64().unwrap()
}

#[test]
fn a_tree_imports_as_parametric_features_and_reports_what_it_skipped() {
    let mut s = session();
    let t = tree(
        json!([
            {"name": "width", "expression": "40 mm", "value": 40.0, "kind": "length"},
            {"name": "depth", "expression": "width / 4", "value": 10.0, "kind": "length"},
            {"name": "odd", "expression": "Fusion::thing(1)", "value": 3.0, "kind": "number"}
        ]),
        json!([
            rect("Sketch1", xy(), 40.0, 20.0),
            {"type": "extrude", "name": "Extrude1", "sketch": "Sketch1", "profiles": "all",
             "distance": 10, "distance_parameter": "depth", "operation": "new"},
            {"type": "unsupported", "name": "Loft3", "original_type": "LoftFeature"},
            {"type": "shell", "name": "Shell1"}
        ]),
        json!([{"name": "Body1", "volume_mm3": 8000.0, "bbox_mm": [[0, 0, 0], [40, 20, 10]]}]),
    );
    let r = import(&mut s, "plate.gtree.json", &t).unwrap();
    assert_eq!(r["features"], json!(["Sketch1", "Extrude1"]), "{r}");
    let skipped: Vec<&str> =
        r["skipped_user_text"].as_array().unwrap().iter().map(|v| v["feature"].as_str().unwrap()).collect();
    assert_eq!(skipped, ["Loft3", "Shell1"], "{r}");
    assert!((volume(&mut s) - 8000.0).abs() < 1e-6);
    assert!((r["volume_mm3"]["source"].as_f64().unwrap() - 8000.0).abs() < 1e-9, "{r}");
    // The depth stays an expression of width: changing width changes nothing of the
    // fixed sketch, but depth follows it.
    let text = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(text.contains("param depth = width / 4"), "{text}");
    assert!(text.contains("param odd = 3"), "{text}");
    run(&mut s, "param.set", json!({"name": "width", "expr": "80 mm"}));
    assert!((volume(&mut s) - 40.0 * 20.0 * 20.0).abs() < 1e-6);
    // One undo removes the whole import.
    run(&mut s, "history.undo", json!({}));
    run(&mut s, "history.undo", json!({}));
    assert!(s.document(&gusset_history::Actor::User).unwrap().features.is_empty());
}

#[test]
fn a_sketch_on_an_offset_downward_plane_becomes_a_datum_plane() {
    let mut s = session();
    // Normal -Z at z = 5: extruding 10 along it fills z in [-5, 5].
    let plane = json!({"origin": [0, 0, 5], "x_axis": [1, 0, 0], "y_axis": [0, -1, 0]});
    let t = tree(
        json!([]),
        json!([
            rect("Sk", plane, 10.0, 10.0),
            {"type": "extrude", "name": "Ex", "sketch": "Sk", "profiles": [{"point": [5, 5]}],
             "distance": 10, "operation": "new"}
        ]),
        json!([]),
    );
    let r = import(&mut s, "down.gtree.json", &t).unwrap();
    assert_eq!(r["features"], json!(["Sk_plane", "Sk", "Ex"]), "{r}");
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    let b = run(&mut s, "query.body", json!({}));
    assert_eq!(b["bbox_min_mm"], json!([0.0, -10.0, -5.0]), "{b}");
    assert_eq!(b["bbox_max_mm"], json!([10.0, 0.0, 5.0]), "{b}");
}

#[test]
fn a_tree_that_is_not_one_is_refused() {
    let mut s = session();
    let e = import(&mut s, "x.json", &json!({"format": "something-else", "version": 1})).unwrap_err();
    assert_eq!(e.code, "io", "{e:?}");
    let e = import(&mut s, "y.json", &json!({"format": "gusset-feature-tree", "version": 9})).unwrap_err();
    assert_eq!(e.code, "io", "{e:?}");
}

#[test]
fn the_plugin_samples_import() {
    for sample in ["fusion", "onshape", "solidworks"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../plugins/{sample}/sample/expected.gtree.json"));
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let mut s = session();
        let t: Value = serde_json::from_slice(&bytes).unwrap();
        let r = import(&mut s, "sample.gtree.json", &t).unwrap_or_else(|e| panic!("{sample}: {e:?}"));
        eprintln!("{sample}: {r}");
        assert!(!r["features"].as_array().unwrap().is_empty(), "{sample}: {r}");
    }
}

#[test]
fn a_bad_sketch_or_parameter_is_skipped_without_spoiling_the_rest() {
    let mut s = session();
    let mut bad = rect("Bad", xy(), 5.0, 5.0);
    // A one-entity coincident, a value on a horizontal and a line ending on a curve id.
    bad["constraints"] = json!([{"type": "coincident", "entities": ["p1"]}]);
    let mut worse = rect("Worse", xy(), 5.0, 5.0);
    worse["curves"][0]["end"] = json!("l2");
    let t = tree(
        json!([
            {"name": "w", "expression": "40 mm", "value": 40.0, "kind": "length"},
            {"name": "W", "expression": "2 * w", "value": 80.0, "kind": "length"},
            {"name": "mm", "expression": "w +\n1 mm", "value": 41.0, "kind": "length"},
            {"name": "huge", "value": 1e300, "kind": "length"}
        ]),
        json!([
            bad,
            worse,
            rect("Good", xy(), 10.0, 10.0),
            {"type": "extrude", "name": "E", "sketch": "Good", "profiles": "all", "distance": 2}
        ]),
        json!([]),
    );
    let r = import(&mut s, "bad.gtree.json", &t).unwrap();
    assert_eq!(r["features"], json!(["Good", "E"]), "{r}");
    let skipped = r["skipped_user_text"].to_string();
    assert!(skipped.contains("Bad") && skipped.contains("Worse"), "{r}");
    assert!((volume(&mut s) - 200.0).abs() < 1e-6);
    let text = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    // W collides with w (names are case-insensitive) and still refers to it; "mm" is a unit;
    // the expression is printed on one line.
    assert!(text.contains("param w = 40 mm") && text.contains("param W_2 = 2 * w"), "{text}");
    assert!(text.contains("param p_mm = w + 1 mm"), "{text}");
    assert!(!text.contains("huge"), "{text}");
}

#[test]
fn bad_values_are_reported_not_imported() {
    // Dogfooding run 7: F7-3 (duplicates, unknown constraints), F7-4 (1/0), F7-5 (a huge
    // distance), F7-6 (an empty tree).
    let mut s = session();
    let mut sk = rect("S", xy(), 10.0, 10.0);
    sk["constraints"] = json!([{"type": "frobnicate", "entities": ["l1"]}]);
    let t = tree(
        json!([
            {"name": "a", "expression": "1 mm / 0", "value": 3.0, "kind": "length"},
            {"name": "a", "expression": "2 mm", "value": 2.0, "kind": "length"}
        ]),
        json!([
            sk,
            {"type": "extrude", "name": "Far", "sketch": "S", "profiles": "all", "distance": 1e13},
            {"type": "extrude", "name": "E", "sketch": "S", "profiles": "all", "distance": 1}
        ]),
        json!([{"name": "B", "volume_mm3": 100.0}]),
    );
    let r = import(&mut s, "v.gtree.json", &t).unwrap();
    let w = r["warnings_user_text"].to_string();
    assert!(w.contains("appears twice") && w.contains("unknown types"), "{r}");
    assert!(w.contains("its expression was replaced by its value"), "{r}");
    assert!(r["skipped_user_text"].to_string().contains("Far"), "{r}");
    assert_eq!(r["volume_mm3"]["imported"], json!(100.0), "{r}");
    assert!(!w.contains("differs"), "{r}");
    let text = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(text.contains("param a = 3 mm") && text.contains("param a_2 = 2 mm"), "{text}");
    let e = import(&mut s, "empty.gtree.json", &tree(json!([]), json!([]), json!([]))).unwrap_err();
    assert!(e.message.contains("empty"), "{e:?}");
}

fn tree2(features: Value) -> Value {
    let mut t = tree(json!([]), features, json!([]));
    t["version"] = json!(2);
    t
}

fn box_features(w: f64, h: f64, d: f64) -> Vec<Value> {
    vec![
        rect("Base", xy(), w, h),
        json!({"type": "extrude", "name": "Block", "sketch": "Base", "profiles": "all", "distance": d, "operation": "new"}),
    ]
}

fn skipped(r: &Value) -> Vec<(String, String)> {
    r["skipped_user_text"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["feature"].as_str().unwrap().to_string(), v["reason"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn version_2_types_need_a_version_2_file_and_version_1_files_still_import() {
    let mut s = session();
    let mut t = tree2(json!([]));
    t["version"] = json!(1);
    let mut fs = box_features(10.0, 10.0, 5.0);
    fs.push(json!({"type": "shell", "name": "Sh", "faces": [{"point": [5, 5, 5]}], "thickness": 1}));
    t["features"] = json!(fs);
    let r = import(&mut s, "v1.gtree.json", &t).unwrap();
    assert_eq!(r["features"], json!(["Base", "Block"]), "{r}");
    let sk = skipped(&r);
    assert_eq!(sk.len(), 1, "{r}");
    assert!(sk[0].1.contains("version 2"), "{r}");
    let mut t3 = tree2(json!([]));
    t3["version"] = json!(3);
    let e = import(&mut s, "v3.gtree.json", &t3).unwrap_err();
    assert_eq!(e.code, "io", "{e:?}");
}

#[test]
fn bad_version_2_features_are_skipped_with_reasons_and_rolled_back() {
    let mut s = session();
    let mut fs = box_features(10.0, 10.0, 5.0);
    fs.extend([
        // A face nothing lies on.
        json!({"type": "hole", "name": "H1", "face": {"point": [5, 5, 99]}, "positions": [[5, 5, 99]], "diameter": 2, "through": true}),
        // Nonsense numbers and variants.
        json!({"type": "hole", "name": "H2", "face": {"point": [5, 5, 5]}, "positions": [[5, 5, 5]], "diameter": -2, "through": true}),
        json!({"type": "hole", "name": "H3", "face": {"point": [5, 5, 5]}, "positions": [[5, 5, 5]], "diameter": 2, "variant": "tapped", "through": true}),
        json!({"type": "hole", "name": "H4", "face": {"point": [5, 5, 5]}, "positions": [[5, 5, 5]], "diameter": 4, "variant": "counterbore", "cbore_diameter": 3, "cbore_depth": 1, "through": true}),
        json!({"type": "hole", "name": "H5", "face": {"point": [5, 5, 5]}, "positions": [[5, 5, 1e300]], "diameter": 2, "through": true}),
        json!({"type": "pattern_linear", "name": "P1", "features": ["Nope"], "direction": [1, 0, 0], "count": 3, "spacing": 5}),
        json!({"type": "pattern_linear", "name": "P2", "features": ["Block"], "direction": [1, 0, 0], "count": 1, "spacing": 5}),
        json!({"type": "pattern_linear", "name": "P3", "features": ["Block"], "direction": [0, 0, 0], "count": 3, "spacing": 5}),
        json!({"type": "pattern_circular", "name": "P4", "features": ["Block"], "count": 3}),
        json!({"type": "mirror", "name": "M1", "features": ["Nope"], "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}}),
        json!({"type": "mirror", "name": "M2", "plane": {"origin": [0, 0, 0], "normal": [1, 1, 0]}}),
        json!({"type": "mirror", "name": "M3", "plane": {"origin": [0, 0, 0], "normal": [0, 0, 0]}}),
        json!({"type": "shell", "name": "S1", "faces": [], "thickness": 1}),
        json!({"type": "shell", "name": "S2", "faces": [{"point": [5, 5, 99]}], "thickness": 1}),
        json!({"type": "shell", "name": "S3", "faces": [{"point": [5, 5, 5]}], "thickness": 1, "side": "sideways"}),
        json!({"type": "shell", "name": "S4", "faces": [{"point": [5, 5, 5]}], "thickness": 0}),
        json!({"type": "hole", "name": "H6"}),
        rect("After", xy(), 3.0, 3.0),
    ]);
    let r = import(&mut s, "bad2.gtree.json", &tree2(json!(fs))).unwrap();
    let sk = skipped(&r);
    for n in ["H1", "H2", "H3", "H4", "H5", "P1", "P2", "P3", "P4", "M1", "M2", "M3", "S1", "S2", "S3", "S4", "H6"] {
        assert!(sk.iter().any(|(f, _)| f == n), "{n} should be skipped: {r}");
    }
    assert!(sk.iter().any(|(f, why)| f == "H3" && why.contains("tapped")), "{r}");
    assert!(sk.iter().any(|(f, why)| f == "M2" && why.contains("parallel")), "{r}");
    // Everything good before and after survived, and none of the bad text stayed.
    assert_eq!(r["features"], json!(["Base", "Block", "After"]), "{r}");
    let text = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    for kw in ["hole ", "pattern_", "mirror ", "shell "] {
        assert!(!text.contains(kw), "{kw} left in the text: {text}");
    }
}

#[test]
fn a_mirrored_body_and_mirrored_features_import() {
    let mut s = session();
    let mut fs = box_features(10.0, 10.0, 5.0);
    // The whole body across x = 20 (an offset datum plane), kept as its own body.
    fs.push(json!({"type": "mirror", "name": "M", "target": "Block", "merge": false,
                   "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    // The extrude mirrored across x = 0 (its sketch is mirrored with it).
    fs.push(json!({"type": "mirror", "name": "MF", "features": ["Block"],
                   "plane": {"origin": [0, 0, 0], "normal": [-1, 0, 0]}}));
    let r = import(&mut s, "mir.gtree.json", &tree2(json!(fs))).unwrap();
    assert_eq!(skipped(&r), vec![], "{r}");
    let features = r["features"].as_array().unwrap();
    assert!(features.contains(&json!("M_plane")) && features.contains(&json!("M")), "{r}");
    assert!(features.contains(&json!("Base_MF")) && features.contains(&json!("Block_MF")), "{r}");
    let text = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(text.contains("plane M_plane (base: YZ, offset: 20 mm)"), "{text}");
    assert!(text.contains("mirror M (body: Block, plane: M_plane)"), "{text}");
    // The reflected sketch lies on x in [-10, 0] and extrudes the same way (+z): its frame
    // is mirrored, so the distance flips.
    let b = run(&mut s, "query.body", json!({"body": "Block_MF"}));
    assert_eq!(b["bbox_min_mm"], json!([-10.0, 0.0, 0.0]), "{b}");
    assert_eq!(b["bbox_max_mm"], json!([0.0, 10.0, 5.0]), "{b}");
}

#[test]
fn a_tilted_mirror_of_features_sketches_on_tilted_datum_planes() {
    // Its reflected sketch plane is tilted: XY turned about Y, then offset (OCCT checks the
    // geometry in modeling_tree_import.rs).
    let mut s = session();
    let mut fs = box_features(10.0, 10.0, 5.0);
    fs.push(json!({"type": "mirror", "name": "T", "features": ["Block"],
                   "plane": {"origin": [0, 0, 0], "normal": [1, 0, 2]}}));
    let r = import(&mut s, "tilt.gtree.json", &tree2(json!(fs))).unwrap();
    assert_eq!(skipped(&r), Vec::<(String, String)>::new(), "{r}");
    assert_eq!(r["features"], json!(["Base", "Block", "Base_T_tilt2", "Base_T", "Block_T"]), "{r}");
    let text = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(text.contains("plane Base_T_tilt2 (base: XY, axis: Y, angle: "), "{text}");
}

#[test]
fn two_mirrors_named_alike_each_reflect_their_own_copy() {
    // Review of v2: copies were named after the mirror's name, so a second mirror called
    // "M" reused the first one's sketch, reflected across the wrong plane.
    let mut s = session();
    let t = tree2(json!([
        rect("S", json!({"origin": [10, 10, 0], "x_axis": [1, 0, 0], "y_axis": [0, 1, 0]}), 10.0, 10.0),
        {"type": "extrude", "name": "E", "sketch": "S", "profiles": "all", "distance": 5},
        {"type": "mirror", "name": "M", "features": ["E"], "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}},
        {"type": "mirror", "name": "M", "features": ["E"], "plane": {"origin": [0, 0, 0], "normal": [0, 1, 0]}}
    ]));
    let r = import(&mut s, "m.gtree.json", &t).unwrap();
    assert_eq!(r["skipped_user_text"], json!([]), "{r}");
    // Three blocks: the original at x, y 10..20, one at x -20..-10 and one at y -20..-10.
    let ev = s.evaluate(&gusset_history::Actor::User).unwrap();
    let k = s.kernel().clone();
    let mins: Vec<[f64; 2]> = ev
        .bodies
        .iter()
        .map(|b| {
            let bb = k.bbox(&b.shape).unwrap();
            [(bb.min[0] * 1e6).round() / 1e6, (bb.min[1] * 1e6).round() / 1e6]
        })
        .collect();
    assert_eq!(mins.len(), 3, "{mins:?}");
    assert!(mins.contains(&[10.0, 10.0]) && mins.contains(&[-20.0, 10.0]) && mins.contains(&[10.0, -20.0]), "{mins:?}");
}

#[test]
fn a_body_count_unlike_the_sources_is_warned() {
    let mut s = session();
    let t = tree(
        json!([]),
        json!([
            rect("Sketch1", xy(), 40.0, 20.0),
            {"type": "extrude", "name": "Extrude1", "sketch": "Sketch1", "profiles": "all",
             "distance": 10, "operation": "new"}
        ]),
        json!([{"name": "Body1", "volume_mm3": 4000.0}, {"name": "Body2", "volume_mm3": 4000.0}]),
    );
    let r = import(&mut s, "two.gtree.json", &t).unwrap();
    assert!(r["warnings_user_text"].to_string().contains("made 1 bodies; the source lists 2"), "{r}");
    // Volumes agree, so that is the only warning.
    assert_eq!(r["warnings_user_text"].as_array().unwrap().len(), 1, "{r}");
}
