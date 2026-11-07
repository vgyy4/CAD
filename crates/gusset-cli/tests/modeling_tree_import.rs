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
