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
    for id in [
        "feature.shell",
        "feature.draft",
        "feature.offset_face",
        "feature.mirror",
        "feature.pattern_linear",
        "feature.pattern_circular",
    ] {
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
fn draft_command_writes_the_statement_and_follows_renames() {
    let mut s = session();
    let u = CmdCtx::user(1);
    plate(&mut s, &u);
    let r = run(
        &mut s,
        &u,
        "feature.draft",
        json!({"faces": ["Extrude1/side(Sketch1/l5)"], "plane": "Extrude1/cap(start)", "angle": "2 deg", "flip": true}),
    );
    assert_eq!(r["feature"], "Draft1", "{r}");
    // The mock kernel has no draft: the edit is kept and reported as not rebuilding.
    assert_eq!(r["rebuild_ok"], false, "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(
        text.contains("draft Draft1 (body: Extrude1, faces: [Extrude1/side(Sketch1/l5)], plane: Extrude1/cap(start), angle: 2 deg, flip: true)"),
        "{text}"
    );
    run(&mut s, &u, "feature.rename", json!({"from": "Extrude1", "to": "Base"}));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("faces: [Base/side(Sketch1/l5)], plane: Base/cap(start)"), "{text}");
}

#[test]
fn offset_face_command_writes_the_statement() {
    let mut s = session();
    let u = CmdCtx::user(1);
    plate(&mut s, &u);
    let r = run(&mut s, &u, "feature.offset_face", json!({"faces": ["Extrude1/cap(end)"], "distance": "-1.5 mm"}));
    assert_eq!(r["feature"], "OffsetFace1", "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(
        text.contains("offset_face OffsetFace1 (body: Extrude1, faces: [Extrude1/cap(end)], distance: -1.5 mm)"),
        "{text}"
    );
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
        ("feature.draft", json!({"faces": [], "plane": "XY", "angle": "2 deg"})),
        ("feature.offset_face", json!({"faces": [], "distance": "2 mm"})),
        ("feature.offset_face", json!({"faces": ["Extrude1/cap(end)"], "distance": "2 +"})),
        ("feature.draft", json!({"faces": ["Extrude1/side(Sketch1/l5)"], "plane": "ZZ", "angle": "2 deg"})),
        ("feature.draft", json!({"faces": ["Extrude1/side(Sketch1/l5)"], "plane": "XY", "angle": "2 +"})),
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

#[test]
fn ops_target_bodies_by_body_name() {
    // F2-11 meets v0.2: a body named with body_name can be mirrored and patterned by
    // that name; the document stores the creating feature's name.
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, &u, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    let r = run(&mut s, &u, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm", "body_name": "Plate"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run(
        &mut s,
        &u,
        "feature.pattern_linear",
        json!({"direction": "[0, 1, 0]", "count": "3", "spacing": "60 mm", "body": "Plate"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run(&mut s, &u, "feature.mirror", json!({"plane": "YZ", "body": "Plate"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("pattern_linear Pattern1 (body: Extrude1,"), "{text}");
    assert!(text.contains("mirror Mirror1 (body: Extrude1,"), "{text}");
    let b = run(&mut s, &u, "query.body", json!({"body": "Plate"}));
    assert!((b["volume_mm3"].as_f64().unwrap() - 3.0 * 8000.0).abs() < 1e-6, "{b}");
    // The mirror is a new body named after its feature; it can be targeted by that name.
    let r = run(
        &mut s,
        &u,
        "feature.pattern_linear",
        json!({"direction": "[0, 0, 1]", "count": "2", "spacing": "50 mm", "body": "Mirror1"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run(&mut s, &u, "query.body", json!({"body": "Mirror1"}));
    assert!((b["volume_mm3"].as_f64().unwrap() - 2.0 * 3.0 * 8000.0).abs() < 1e-6, "{b}");
}
