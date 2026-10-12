//! Shell, mirror and pattern commands through the registry (mock kernel).

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

fn plate(s: &mut Session, ctx: &CmdCtx) {
    run(s, ctx, "sketch.create", json!({"plane": "XY"}));
    run(
        s,
        ctx,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20, "width_expr": "40 mm", "height_expr": "20 mm"}),
    );
    let r = run(s, ctx, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
}

#[test]
fn commands_are_registered_with_schemas() {
    for id in ["feature.shell", "feature.mirror", "feature.pattern_linear", "feature.pattern_circular"] {
        let spec = find(id).unwrap_or_else(|| panic!("{id} missing"));
        assert!(spec.description.len() > 40, "{id}");
        let schema = serde_json::to_string(&(spec.schema)()).unwrap();
        assert!(schema.contains("properties"), "{id}: {schema}");
    }
}

#[test]
fn mirror_and_pattern_commands_edit_the_document() {
    let mut s = session();
    let u = CmdCtx::user(1);
    plate(&mut s, &u);
    run(&mut s, &u, "param.set", json!({"name": "n_bolts", "expr": "3"}));
    let r = run(&mut s, &u, "feature.mirror", json!({"plane": "YZ"}));
    assert_eq!(r["feature"], "Mirror1");
    assert_eq!(r["rebuild_ok"], true, "{r}");
    // Direction vector and count expression; the target body is stored explicitly.
    let r = run(
        &mut s,
        &u,
        "feature.pattern_linear",
        json!({"direction": "[0, 1, 0]", "count": "n_bolts", "spacing": "60 mm", "body": "Extrude1"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run(
        &mut s,
        &u,
        "feature.pattern_circular",
        json!({"axis": "Z", "through": ["0 mm", "300 mm", "0 mm"], "count": "n_bolts", "angle": "180 deg", "body": "Mirror1"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("mirror Mirror1 (body: Extrude1, plane: YZ)"), "{text}");
    assert!(
        text.contains("pattern_linear Pattern1 (body: Extrude1, direction: [0, 1, 0], count: n_bolts, spacing: 60 mm)")
    );
    assert!(text.contains("pattern_circular Pattern2 (body: Mirror1, axis: Z, through: [0 mm, 300 mm, 0 mm], count: n_bolts, angle: 180 deg)"), "{text}");
    // Body queries show the instance names.
    let b = run(&mut s, &u, "query.body", json!({"names": true, "body": "Extrude1"}));
    assert!(b.to_string().contains("Pattern1/instance(2, Extrude1/cap(end))"), "{b}");
    // Parameter edits flow through the count.
    run(&mut s, &u, "param.set", json!({"name": "n_bolts", "expr": "4"}));
    let b = run(&mut s, &u, "query.body", json!({"body": "Extrude1"}));
    assert!((b["volume_mm3"].as_f64().unwrap() - 4.0 * 8000.0).abs() < 1e-6, "{b}");
    // Renaming the source updates stored references and names stay valid.
    run(&mut s, &u, "feature.rename", json!({"from": "Extrude1", "to": "Base"}));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("mirror Mirror1 (body: Base,"), "{text}");
    // Deleting the source is refused while dependants exist.
    assert!(execute(&mut s, &u, "feature.delete", json!({"name": "Base"})).is_err());
}

#[test]
fn bad_arguments_are_rejected_with_messages() {
    let mut s = session();
    let u = CmdCtx::user(1);
    plate(&mut s, &u);
    let cases = [
        ("feature.shell", json!({"faces": ["Extrude1/cap(end)"]})),
        ("feature.shell", json!({"faces": ["nonsense"], "thickness": "2 mm"})),
        ("feature.shell", json!({"faces": ["Extrude1/side(Sketch1/l99)"], "thickness": "2 mm"})),
        ("feature.mirror", json!({"plane": "ZZ"})),
        ("feature.mirror", json!({"plane": "XY", "color": "red"})),
        ("feature.pattern_linear", json!({"direction": "W", "count": "3", "spacing": "1 mm"})),
        ("feature.pattern_linear", json!({"direction": "X", "count": "3 +", "spacing": "1 mm"})),
        ("feature.pattern_circular", json!({"axis": "Z", "count": "3", "through": ["1", "2", "3"], "body": "Nope"})),
    ];
    for (id, args) in cases {
        let e = execute(&mut s, &u, id, args.clone()).unwrap_err();
        assert!(matches!(e.code.as_str(), "invalid_args" | "not_found" | "invalid_document"), "{id} {args}: {e:?}");
    }
    // Nothing was added.
    assert_eq!(s.document(&gusset_history::Actor::User).unwrap().features.len(), 2);
    // Without any body the patterns refuse early.
    let mut empty = session();
    let e =
        execute(&mut empty, &u, "feature.pattern_linear", json!({"direction": "X", "count": "3", "spacing": "1 mm"}))
            .unwrap_err();
    assert!(e.message.contains("no body"), "{e:?}");
}
