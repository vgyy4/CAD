//! `feature.add` (a feature from its code-view statement) with the real kernel.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

#[test]
fn a_cut_added_from_text_warns_when_it_misses_the_body() {
    // Dogfooding F11-9: a plate z 0..10 and a cut from its bottom sketch further down.
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 5, "y": 5, "width": 10, "height": 10}));
    let r =
        run(&mut s, "feature.add", json!({"statement": "extrude Miss (profile: Sketch2, distance: -6 mm, op: cut)"}));
    assert!(r["warnings"].to_string().contains("did not change body"), "{r}");
    let r = run(&mut s, "feature.add", json!({"statement": "extrude Hit (profile: Sketch2, distance: 6 mm, op: cut)"}));
    assert!(r.get("warnings").is_none(), "{r}");
    assert!(r["body_changes"].is_array(), "{r}");
}

#[test]
fn a_baked_part_keeps_its_volume_and_takes_direct_edits() {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    let before = run(&mut s, "query.body", json!({}))["volume_mm3"].as_f64().unwrap();
    let r = run(&mut s, "feature.bake", json!({"confirm": true}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run(&mut s, "query.body", json!({"names": true}));
    assert!((b["volume_mm3"].as_f64().unwrap() - before).abs() < 1e-6, "{b}");
    // The top face (z = 10) of the baked body, moved up 5 mm: 40 x 20 x 15.
    let faces = b["face_names"].as_array().unwrap();
    let top = faces
        .iter()
        .find(|f| f["normal"].as_array().is_some_and(|n| n[2].as_f64().unwrap() > 0.99))
        .unwrap_or_else(|| panic!("{b}"));
    let name = top["name"].as_str().unwrap();
    let r = run(&mut s, "feature.offset_face", json!({"faces": [name], "distance": "5 mm"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let v = run(&mut s, "query.body", json!({}))["volume_mm3"].as_f64().unwrap();
    assert!((v - 40.0 * 20.0 * 15.0).abs() < 1e-6, "{v}");
}
