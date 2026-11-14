//! A bend made with `feature.bend` is a bend of the flat pattern (dogfooding F18-12), through
//! the commands with the real kernel.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

/// A tab 80 x 40 x 2 with a bend line at x = 40 sketched in `plane`; the flat pattern query.
fn bent_in(plane: &str) -> Value {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 80, "height": 40}));
    run(&mut s, "feature.tab", json!({"profile": "Sketch1", "thickness": "2 mm"}));
    run(&mut s, "sketch.create", json!({"plane": plane}));
    run(&mut s, "sketch.add_polyline", json!({"sketch": "Sketch2", "points": [[40, -5], [40, 45]]}));
    let r = run(&mut s, "feature.bend", json!({"body": "Tab1", "line": "Sketch2.l3", "angle": "90 deg"}));
    assert_eq!(r["rebuild_ok"], true, "{plane}: {r}");
    run(&mut s, "query.flat_pattern", json!({}))
}

#[test]
fn a_bend_command_shows_in_the_flat_pattern() {
    // The line sketched in the tab's plane and on the tab's top face (a parallel plane).
    for plane in ["XY", "Tab1/cap(end)"] {
        let q = bent_in(plane);
        assert_eq!(q["bends"].as_array().map(Vec::len), Some(1), "{plane}: {q}");
        assert!(!q.to_string().contains("changed after"), "{plane}: {q}");
        assert_eq!(q["size_mm"], json!([80.0, 40.0]), "{plane}: {q}");
        // The zone is centred on the line x = 40.
        let b = &q["bends"][0];
        let mid = (b["zone_start_mm"][0][0].as_f64().unwrap() + b["zone_end_mm"][0][0].as_f64().unwrap()) / 2.0;
        assert!((mid - 40.0).abs() < 1e-6, "{plane}: the bend lies at x = 40: {q}");
    }
}
