//! Sweep and loft commands through the registry (mock kernel: the features are written
//! to the document; the geometry itself is tested with OCCT in gusset-cli).

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0))
}

fn run(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> Value {
    execute(s, ctx, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn fail(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> String {
    match execute(s, ctx, id, args) {
        Ok(v) => panic!("{id} should fail: {v}"),
        Err(e) => format!("{e:?}"),
    }
}

fn text(s: &Session) -> String {
    s.document(&gusset_history::Actor::User).unwrap().to_text()
}

/// A square profile on XY and a bent path on XZ.
fn sketches(s: &mut Session, ctx: &CmdCtx) {
    run(s, ctx, "sketch.create", json!({"plane": "XY"}));
    run(
        s,
        ctx,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": -3, "y": -3, "width": 6, "height": 6, "width_expr": "6 mm", "height_expr": "6 mm"}),
    );
    run(s, ctx, "sketch.create", json!({"plane": "XZ"}));
    run(s, ctx, "sketch.add_polyline", json!({"sketch": "Sketch2", "points": [[0, 0], [0, 20], [15, 20]]}));
}

#[test]
fn commands_are_registered_with_short_schemas() {
    for id in ["feature.sweep", "feature.loft"] {
        let spec = find(id).unwrap_or_else(|| panic!("{id} missing"));
        assert!(spec.description.len() > 40 && spec.description.len() < 200, "{id}");
        let schema = serde_json::to_string(&(spec.schema)()).unwrap();
        assert!(schema.contains("properties"), "{id}: {schema}");
    }
}

#[test]
fn sweep_command_writes_the_feature() {
    let mut s = session();
    let u = CmdCtx::user(1);
    sketches(&mut s, &u);
    run(&mut s, &u, "param.set", json!({"name": "turn", "expr": "45 deg"}));
    let r = run(
        &mut s,
        &u,
        "feature.sweep",
        json!({"sketch": "Sketch1", "path": "Sketch2", "twist": "turn", "body_name": "Rail"}),
    );
    assert_eq!(r["feature"], "Sweep1");
    assert_eq!(r["body"], "Rail");
    // The mock kernel cannot sweep: the feature is there, the rebuild reports why.
    assert_eq!(r["rebuild_ok"], false, "{r}");
    assert!(r["new_errors"].to_string().contains("sweep is not supported"), "{r}");
    let t = text(&s);
    assert!(
        t.contains(
            "sweep Sweep1 (profile: Sketch1, path: Sketch2, orientation: frenet, twist: turn, op: new, body_name: Rail)"
        ),
        "{t}"
    );
    // Other path forms: some curves, edges; a fixed orientation; a cut into a body.
    run(&mut s, &u, "feature.delete", json!({"name": "Sweep1"}));
    run(
        &mut s,
        &u,
        "feature.sweep",
        json!({"sketch": "Sketch1", "regions": ["l5"], "path": "Sketch2.curves(l4, l5)", "orientation": "fixed", "name": "Pipe"}),
    );
    assert!(text(&s).contains(
        "sweep Pipe (profile: Sketch1.regions(l5), path: Sketch2.curves(l4, l5), orientation: fixed, op: new)"
    ));
    let r = run(
        &mut s,
        &u,
        "feature.sweep",
        json!({"sketch": "Sketch1", "path": "[edge(Pipe/cap(end), Pipe/side(Sketch1/l5))]", "name": "Edged"}),
    );
    assert_eq!(r["feature"], "Edged");
    assert!(text(&s).contains("path: [edge(Pipe/cap(end), Pipe/side(Sketch1/l5))]"));
    // Renaming the path sketch and the profile updates the sweeps.
    run(&mut s, &u, "feature.rename", json!({"from": "Sketch2", "to": "Rail2"}));
    run(&mut s, &u, "feature.rename", json!({"from": "Sketch1", "to": "Section"}));
    let t = text(&s);
    assert!(t.contains("sweep Pipe (profile: Section.regions(l5), path: Rail2.curves(l4, l5)"), "{t}");
    assert!(t.contains("path: [edge(Pipe/cap(end), Pipe/side(Section/l5))]"), "{t}");
    // Renaming the sweep updates later edge references.
    run(&mut s, &u, "feature.rename", json!({"from": "Pipe", "to": "Tube"}));
    assert!(text(&s).contains("path: [edge(Tube/cap(end), Tube/side(Section/l5))]"));
}

#[test]
fn sweep_arguments_are_checked() {
    let mut s = session();
    let u = CmdCtx::user(1);
    sketches(&mut s, &u);
    let bad = |s: &mut Session, args: Value, why: &str| {
        let e = fail(s, &u, "feature.sweep", args);
        assert!(e.contains(why), "{why}: {e}");
    };
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch2", "orientation": "sideways"}), "orientation");
    bad(
        &mut s,
        json!({"sketch": "Sketch1", "path": "Sketch2", "twist": "90 deg", "orientation": "fixed"}),
        "twist needs",
    );
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch2", "twist": "banana"}), "banana");
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch2.wires(l4)"}), "path");
    bad(&mut s, json!({"sketch": "Sketch1", "path": "[]"}), "path");
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch2", "op": "weld"}), "unknown op");
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch2", "op": "cut"}), "no body");
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch2", "body_name": "B", "op": "add"}), "no body");
    bad(&mut s, json!({"sketch": "Sketch1"}), "path");
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch2", "bogus": 1}), "bogus");
    // The same sketch as profile and path is caught when the document is checked.
    bad(&mut s, json!({"sketch": "Sketch1", "path": "Sketch1"}), "same sketch");
    // Nothing was written by the failed calls.
    assert!(!text(&s).contains("sweep "));
}

#[test]
fn loft_command_writes_the_feature() {
    let mut s = session();
    let u = CmdCtx::user(1);
    sketches(&mut s, &u);
    run(&mut s, &u, "feature.plane", json!({"base": "XY", "offset": "12 mm", "name": "Up"}));
    run(&mut s, &u, "sketch.create", json!({"plane": "Up"}));
    run(
        &mut s,
        &u,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch3", "x": -1, "y": -1, "width": 2, "height": 2, "width_expr": "2 mm", "height_expr": "2 mm"}),
    );
    let r = run(&mut s, &u, "feature.loft", json!({"profiles": ["Sketch1", "Sketch3.regions(l5)"], "ruled": true}));
    assert_eq!(r["feature"], "Loft1");
    assert!(r["new_errors"].to_string().contains("loft is not supported"), "{r}");
    assert!(
        text(&s).contains("loft Loft1 (profiles: [Sketch1, Sketch3.regions(l5)], ruled: true, op: new)"),
        "{}",
        text(&s)
    );
    for (args, why) in [
        (json!({"profiles": ["Sketch1"]}), "at least two"),
        (json!({"profiles": []}), "at least two"),
        (json!({"profiles": ["Sketch1", "Sketch3.regions("]}), "profile"),
        (json!({"profiles": ["Sketch1", "Nope"]}), "Nope"),
        (json!({"profiles": ["Sketch1", "Sketch3"], "op": "cut", "body_name": "B"}), "body_name"),
        (json!({"profile": ["Sketch1", "Sketch3"]}), "profile"),
    ] {
        let e = fail(&mut s, &u, "feature.loft", args);
        assert!(e.contains(why), "{why}: {e}");
    }
    run(&mut s, &u, "feature.rename", json!({"from": "Sketch3", "to": "Cap"}));
    assert!(text(&s).contains("profiles: [Sketch1, Cap.regions(l5)]"));
}
