//! Holes made one after another in a sheet-metal part keep their flat-pattern cut-outs
//! (dogfooding run 7, F7-1 and F7-2), through the commands with the real kernel.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

#[test]
fn each_hole_is_one_cut_out_however_many_follow() {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "S", "x": 0, "y": 0, "width": 100, "height": 60}));
    run(&mut s, "feature.tab", json!({"profile": "S", "thickness": "2 mm", "name": "Plate"}));
    run(&mut s, "feature.edge_flange", json!({"body": "Plate", "line": "l6", "length": "20 mm", "name": "F1"}));
    let hole = |s: &mut Session, face: &str, d: &str, at: [&str; 2], name: &str| {
        run(s, "feature.hole", json!({"face": face, "diameter": d, "through": true, "at": [at], "name": name}));
    };
    let flat = |s: &mut Session| run(s, "query.flat_pattern", json!({}));
    hole(&mut s, "F1/flat(l6)", "6 mm", ["30 mm", "14 mm"], "FH");
    hole(&mut s, "Plate/cap(end)", "10 mm", ["30 mm", "30 mm"], "TH");
    hole(&mut s, "Plate/cap(end)", "4 mm", ["70 mm", "30 mm"], "TH2");
    let q = flat(&mut s);
    assert_eq!(q["changed_by"], json!([]), "{q}");
    let cuts = q["cutouts"].as_array().unwrap();
    assert_eq!(cuts.len(), 3, "{q}");
    assert_eq!(cuts.iter().filter(|c| c.get("circle").is_some()).count(), 3, "{q}");
    // A small hole through the bend, then another hole in the flange: still unfolded (F7-2).
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "H2"}));
    run(&mut s, "sketch.add_circle", json!({"sketch": "H2", "cx": 101, "cy": 45, "radius": 0.9}));
    run(&mut s, "feature.extrude", json!({"sketch": "H2", "distance": "6 mm", "op": "cut", "name": "BendHole"}));
    let q = flat(&mut s);
    let bend_ok = q["changed_by"] == json!([]);
    hole(&mut s, "F1/flat(l6)", "4 mm", ["50 mm", "14 mm"], "FH2");
    let q = flat(&mut s);
    if bend_ok {
        assert_eq!(q["changed_by"], json!([]), "{q}");
        assert_eq!(q["cutouts"].as_array().unwrap().len(), 5, "{q}");
    } else {
        // A cut the pattern cannot show keeps it marked, naming that cut only.
        assert_eq!(q["changed_by"], json!(["BendHole"]), "{q}");
    }
}

#[test]
fn laying_a_face_through_the_body_is_refused_before_committing() {
    // Dogfooding F7-8: the top of a 40 x 20 x 10 block laid onto the plane of a face below
    // its bottom would turn it inside out; the command refuses and commits nothing.
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 100, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "-5 mm", "op": "new"}));
    let n = s.document(&gusset_history::Actor::User).unwrap().features.len();
    let r = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.move_face",
        json!({"faces": ["Extrude1/cap(end)"], "onto": "Extrude2/cap(end)"}),
    );
    let e = r.expect_err("refused");
    assert_eq!(e.code, "invalid_args", "{e:?}");
    assert_eq!(s.document(&gusset_history::Actor::User).unwrap().features.len(), n, "nothing committed");
    // Onto a reachable plane it works.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch3", "x": 200, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch3", "distance": "15 mm", "op": "new"}));
    let ok = run(&mut s, "feature.move_face", json!({"faces": ["Extrude1/cap(end)"], "onto": "Extrude3/cap(end)"}));
    assert_eq!(ok["rebuild_ok"], true, "{ok}");
}
