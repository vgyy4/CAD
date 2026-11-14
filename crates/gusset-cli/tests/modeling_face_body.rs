//! Revolve and sweep of a face profile with `op: add` and no `body` work on the face's own
//! body, not the latest one (dogfooding F18-10), through the commands with the real kernel.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn two_bodies() -> Session {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 20, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm", "body_name": "Base"}));
    // A later body: "the latest body" is not the one the face belongs to.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 60, "y": 0, "width": 5, "height": 5}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "5 mm", "body_name": "Other"}));
    s
}

fn changed(r: &Value) -> Vec<String> {
    r["body_changes"].as_array().unwrap().iter().map(|c| c["body"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_revolved_face_adds_to_the_faces_own_body() {
    let mut s = two_bodies();
    // The cap's top edge (y = 10) is the axis; the face turns 90 deg about it, up and over.
    let r = run(
        &mut s,
        "feature.revolve",
        json!({"face": "Extrude1/cap(end)", "axis": "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l7))", "angle": "90 deg",
               "op": "add"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert_eq!(changed(&r), ["Base"], "{r}");
}

#[test]
fn a_swept_face_adds_to_the_faces_own_body() {
    let mut s = two_bodies();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.add_polyline", json!({"sketch": "Sketch3", "points": [[10, 5], [10, 25]]}));
    let r = run(&mut s, "feature.sweep", json!({"face": "Extrude1/cap(end)", "path": "Sketch3", "op": "add"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert_eq!(changed(&r), ["Base"], "{r}");
}
