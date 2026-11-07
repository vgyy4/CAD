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
