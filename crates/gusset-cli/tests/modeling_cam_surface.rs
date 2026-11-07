//! 3D surface finishing (ADR 0040 amendment) over a model built with the real kernel.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

#[test]
fn a_flat_end_mill_finishes_a_block_top_at_its_height() {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    let job = json!({"sketch": "Sketch1", "operation": "surface_flat", "tool_diameter": 6.0, "top": 10.0,
        "depth": 10.0, "safe": 15.0, "stepover": 0.5, "path": true});
    let r = run(&mut s, "cam.toolpath", job);
    let pts: Vec<[f64; 3]> = r["path"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|l| l.as_array().unwrap().iter().map(|p| [0, 1, 2].map(|i| p[i].as_f64().unwrap())))
        .collect();
    assert!(!pts.is_empty(), "{r}");
    // Never below the top; the cutting passes run on it (the approach comes down from 1 mm above).
    assert!(pts.iter().all(|p| p[2] >= 10.0 - 1e-3), "{r}");
    let on = pts.iter().filter(|p| (p[2] - 10.0).abs() < 1e-3).count();
    assert!(on * 2 > pts.len(), "{on} of {}", pts.len());
    // Lines 3 mm apart cover the 20 mm width: x runs the length, y steps across.
    let ys: std::collections::BTreeSet<i64> = pts.iter().map(|p| (p[1] * 1000.0).round() as i64).collect();
    assert!(ys.len() >= 7, "{ys:?}");
}
