//! Datum plane, body move/copy and combine commands through the registry (mock kernel).

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0))
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn fail(s: &mut Session, id: &str, args: Value) -> CommandError {
    match execute(s, &CmdCtx::user(1), id, args.clone()) {
        Ok(v) => panic!("{id} {args} should fail, gave {v}"),
        Err(e) => e,
    }
}

fn text(s: &Session) -> String {
    s.document(&gusset_history::Actor::User).unwrap().to_text()
}

/// A 40 x 20 x 10 plate (Extrude1) and a 10 x 10 x 5 block (Extrude2) 100 mm to the right.
fn plate_and_block(s: &mut Session) {
    run(s, "param.set", json!({"name": "lift", "expr": "12 mm"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    let r = run(s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 100, "y": 0, "width": 10, "height": 10}));
    let r = run(s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "5 mm", "body_name": "Block"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
}

fn near(a: &Value, b: [f64; 3]) -> bool {
    a.as_array().is_some_and(|v| v.iter().zip(b).all(|(x, y)| (x.as_f64().unwrap_or(f64::NAN) - y).abs() < 1e-9))
}

#[test]
fn commands_are_registered_with_compact_schemas() {
    for id in ["feature.plane", "feature.move_body", "feature.combine"] {
        let spec = find(id).unwrap_or_else(|| panic!("{id} missing"));
        assert!(spec.description.len() > 60 && spec.description.len() < 400, "{id}: {}", spec.description.len());
        let schema = serde_json::to_string(&(spec.schema)()).unwrap();
        assert!(schema.contains("properties"), "{id}");
        assert!(schema.len() < 3500, "{id} schema is {} bytes", schema.len());
    }
    // The timeline command keeps its name; the body move has its own.
    assert!(find("feature.move").is_some() && find("feature_move_body").is_some());
}

#[test]
fn datum_planes_through_commands() {
    let mut s = session();
    plate_and_block(&mut s);
    let r = run(&mut s, "feature.plane", json!({"base": "XY", "offset": "lift"}));
    assert_eq!(r["feature"], "Plane1");
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert!(near(&r["frame"]["origin_mm"], [0.0, 0.0, 12.0]), "{r}");
    assert!(near(&r["frame"]["normal"], [0.0, 0.0, 1.0]), "{r}");
    // Sketch on it, extrude from it, mirror across a second plane.
    let r = run(&mut s, "sketch.create", json!({"plane": "Plane1"}));
    assert!(near(&r["frame"]["origin_mm"], [0.0, 0.0, 12.0]), "{r}");
    let sk = r["sketch"].as_str().unwrap().to_string();
    run(&mut s, "sketch.add_rectangle", json!({"sketch": sk, "x": 0, "y": 0, "width": 5, "height": 5}));
    let r = run(&mut s, "feature.extrude", json!({"sketch": sk, "distance": "2 mm", "body_name": "Cap"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run(&mut s, "query.body", json!({"body": "Cap"}));
    assert!(near(&b["bbox_min_mm"], [0.0, 0.0, 12.0]) && near(&b["bbox_max_mm"], [5.0, 5.0, 14.0]), "{b}");
    let r = run(&mut s, "feature.plane", json!({"base": "YZ", "offset": "60 mm", "name": "Mid"}));
    assert!(near(&r["frame"]["origin_mm"], [60.0, 0.0, 0.0]), "{r}");
    let r = run(&mut s, "feature.mirror", json!({"plane": "Mid", "body": "Extrude1"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run(&mut s, "query.body", json!({"body": "Mirror1"}));
    assert!(near(&b["bbox_min_mm"], [80.0, 0.0, 0.0]), "{b}");
    // Angled: base plane Plane1 turned 30 degrees about X; and about a straight edge of the
    // plate (the top face turned 90 degrees about its front edge stands up through it).
    let r = run(&mut s, "feature.plane", json!({"base": "Plane1", "axis": "X", "angle": "30 deg"}));
    assert_eq!(r["feature"], "Plane2");
    assert!(near(&r["frame"]["normal"], [0.0, -0.5, 0.75_f64.sqrt()]), "{r}");
    let r = run(
        &mut s,
        "feature.plane",
        json!({"base": "Extrude1/cap(end)", "axis": "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))", "angle": "90 deg"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert!(near(&r["frame"]["origin_mm"], [0.0, 0.0, 10.0]) && near(&r["frame"]["normal"], [0.0, -1.0, 0.0]), "{r}");
    // Query results list the planes with origin and normal.
    let m = run(&mut s, "query.model", json!({}));
    let planes = m["datum_planes"].as_array().unwrap();
    assert_eq!(planes.len(), 4, "{m}");
    assert_eq!(planes[0]["name"], "Plane1");
    assert!(near(&planes[0]["origin_mm"], [0.0, 0.0, 12.0]) && near(&planes[0]["normal"], [0.0, 0.0, 1.0]), "{m}");
    assert_eq!(m["bodies"].as_array().unwrap().len(), 4, "planes create no body: {m}");
    // They survive parameter edits.
    run(&mut s, "param.set", json!({"name": "lift", "expr": "20 mm"}));
    let m = run(&mut s, "query.model", json!({}));
    assert!(near(&m["datum_planes"][0]["origin_mm"], [0.0, 0.0, 20.0]), "{m}");
    // Renaming updates the references; deleting a used plane is refused.
    run(&mut s, "feature.rename", json!({"from": "Mid", "to": "Middle"}));
    let t = text(&s);
    assert!(t.contains("mirror Mirror1 (body: Extrude1, plane: Middle)"), "{t}");
    assert!(t.contains("plane Middle (base: YZ, offset: 60 mm)"), "{t}");
    assert!(t.contains("(plane: Plane1,"), "{t}");
    let e = fail(&mut s, "feature.delete", json!({"name": "Plane1"}));
    assert!(e.message.contains("used by"), "{e:?}");
    // Text round trip through the document.
    let t = text(&s);
    run(&mut s, "document.set_text", json!({"text": t}));
}

#[test]
fn datum_plane_argument_errors() {
    let mut s = session();
    plate_and_block(&mut s);
    let before = text(&s);
    let cases = [
        json!({}),
        json!({"offset": "1 mm", "axis": "X", "angle": "5 deg"}),
        json!({"axis": "X"}),
        json!({"angle": "5 deg"}),
        json!({"base": "Nope", "offset": "1 mm"}),
        json!({"base": "Extrude1/side(Sketch1/l99)", "offset": "1 mm"}),
        json!({"offset": "1 mm +"}),
        json!({"axis": "W", "angle": "5 deg"}),
        json!({"axis": "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l99))", "angle": "5 deg"}),
        json!({"offset": "1 mm", "name": "bad name"}),
        json!({"offset": "1 mm", "color": "red"}),
    ];
    for args in cases {
        let e = execute(&mut s, &CmdCtx::user(1), "feature.plane", args.clone()).unwrap_err();
        assert!(matches!(e.code.as_str(), "invalid_args" | "not_found"), "{args}: {e:?}");
    }
    let e = fail(&mut s, "feature.plane", json!({"base": "Nope", "offset": "1 mm"}));
    assert!(e.hint.as_deref().is_some_and(|h| h.contains("feature_plane")), "{e:?}");
    let e = fail(&mut s, "sketch.create", json!({"plane": "Nope"}));
    assert_eq!(e.code, "not_found");
    let e = fail(&mut s, "feature.mirror", json!({"plane": "Nope"}));
    assert_eq!(e.code, "not_found");
    assert_eq!(text(&s), before, "failed commands change nothing");
    // A wrong unit is a failing feature with a message, not a panic.
    let r = run(&mut s, "feature.plane", json!({"offset": "5 deg"}));
    assert_eq!(r["rebuild_ok"], false, "{r}");
    assert!(r["new_errors"].to_string().contains("Plane1"), "{r}");
}

#[test]
fn move_body_translates_rotates_and_copies() {
    let mut s = session();
    plate_and_block(&mut s);
    let r = run(&mut s, "feature.move_body", json!({"body": "Extrude1", "translate": ["10 mm", "0 mm", "lift"]}));
    assert_eq!(r["feature"], "Move1");
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run(&mut s, "query.body", json!({"body": "Extrude1"}));
    assert!(near(&b["bbox_min_mm"], [10.0, 0.0, 12.0]), "{b}");
    assert!(text(&s).contains("move Move1 (body: Extrude1, translate: [10 mm, 0 mm, lift])"), "{}", text(&s));
    // The block is addressed by its body name and rotated about a vertical axis through its
    // centre; as a copy it becomes a new body named after the feature.
    let r = run(
        &mut s,
        "feature.move_body",
        json!({"body": "Block", "axis": "Z", "angle": "90 deg", "through": ["105 mm", "5 mm", "0 mm"], "copy": true}),
    );
    assert_eq!(r["feature"], "Copy1", "{r}");
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert!(r["body_changes"].to_string().contains("Copy1"), "{r}");
    let t = text(&s);
    assert!(
        t.contains("move Copy1 (body: Extrude2, axis: Z, angle: 90 deg, through: [105 mm, 5 mm, 0 mm], copy: true)"),
        "{t}"
    );
    let b = run(&mut s, "query.body", json!({"body": "Copy1", "names": true}));
    assert!(b.to_string().contains("Copy1/copy(Extrude2/cap(end))"), "{b}");
    assert!(near(&b["size_mm"], [10.0, 10.0, 5.0]), "{b}");
    let m = run(&mut s, "query.model", json!({}));
    assert_eq!(m["bodies"].as_array().unwrap().len(), 3, "{m}");
    // Faces of the copy are references like any other.
    let r = run(&mut s, "sketch.create", json!({"plane": "Copy1/copy(Extrude2/cap(end))"}));
    assert!(near(&r["frame"]["origin_mm"], [0.0, 0.0, 5.0]), "{r}");
    // The default target is the latest body; a parameter drives the move.
    run(&mut s, "param.set", json!({"name": "up", "expr": "7 mm"}));
    let r = run(&mut s, "feature.move_body", json!({"translate": ["0 mm", "0 mm", "up"]}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert!(text(&s).contains("move Move2 (body: Copy1, translate"), "{}", text(&s));
    // Undo takes the move back.
    run(&mut s, "history.undo", json!({}));
    let m = run(&mut s, "query.model", json!({}));
    assert_eq!(m["features"].as_array().unwrap().len(), 7, "{m}");
}

#[test]
fn move_body_argument_errors() {
    let mut s = session();
    plate_and_block(&mut s);
    let before = text(&s);
    let cases = [
        json!({}),
        json!({"axis": "Z"}),
        json!({"angle": "5 deg"}),
        json!({"through": ["0 mm", "0 mm", "0 mm"], "translate": ["1 mm", "0 mm", "0 mm"]}),
        json!({"axis": "W", "angle": "5 deg"}),
        json!({"translate": ["1 mm", "0 mm", "+"]}),
        json!({"translate": ["1 mm", "0 mm"]}),
        json!({"translate": ["1 mm", "0 mm", "0 mm"], "body": "Nope"}),
        json!({"translate": ["1 mm", "0 mm", "0 mm"], "copy": true, "name": "Block"}),
    ];
    for args in cases {
        let e = execute(&mut s, &CmdCtx::user(1), "feature.move_body", args.clone()).unwrap_err();
        assert!(matches!(e.code.as_str(), "invalid_args" | "not_found" | "invalid_document"), "{args}: {e:?}");
    }
    assert_eq!(text(&s), before);
    // Units are checked when the feature is rebuilt: an error in the result, no panic.
    let r = run(&mut s, "feature.move_body", json!({"translate": ["1 deg", "0 mm", "0 mm"]}));
    assert_eq!(r["rebuild_ok"], false, "{r}");
    let r = run(&mut s, "feature.move_body", json!({"axis": "[0, 0, 0]", "angle": "5 deg"}));
    assert_eq!(r["rebuild_ok"], false, "{r}");
    assert!(r["new_errors"].to_string().contains("no length"), "{r}");
}

#[test]
fn combine_merges_and_consumes_bodies() {
    let mut s = session();
    plate_and_block(&mut s);
    // Tools can be named by body name; the target defaults to the latest other body.
    let r = run(&mut s, "feature.combine", json!({"tools": ["Block"]}));
    assert_eq!(r["feature"], "Combine1");
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert_eq!(r["target"], "Extrude1");
    assert!(r["body_changes"].to_string().contains("removed"), "{r}");
    assert!(text(&s).contains("combine Combine1 (body: Extrude1, tools: [Extrude2], op: add)"), "{}", text(&s));
    let m = run(&mut s, "query.model", json!({}));
    assert_eq!(m["bodies"].as_array().unwrap().len(), 1, "{m}");
    let b = run(&mut s, "query.body", json!({"body": "Extrude1", "names": true}));
    assert!((b["volume_mm3"].as_f64().unwrap() - 8500.0).abs() < 1e-6, "{b}");
    assert!(b.to_string().contains("Extrude2/cap(end)"), "faces of the tool keep their names: {b}");
    // The tool is gone: it can neither be queried nor targeted again.
    assert_eq!(fail(&mut s, "query.body", json!({"body": "Block"})).code, "not_found");
    let e = fail(&mut s, "feature.combine", json!({"tools": ["Block"]}));
    assert_eq!(e.code, "not_found");
    assert!(e.hint.as_deref().is_some_and(|h| h.contains("Extrude1")), "{e:?}");
    let e = fail(
        &mut s,
        "feature.pattern_linear",
        json!({"direction": "X", "count": "2", "spacing": "1 mm", "body": "Extrude2"}),
    );
    assert!(matches!(e.code.as_str(), "invalid_args" | "not_found" | "invalid_document"), "{e:?}");
    // Deleting the tool's feature is refused while the combine uses it, and undo brings the
    // tool back.
    assert!(execute(&mut s, &CmdCtx::user(1), "feature.delete", json!({"name": "Extrude2"})).is_err());
    run(&mut s, "history.undo", json!({}));
    let m = run(&mut s, "query.model", json!({}));
    assert_eq!(m["bodies"].as_array().unwrap().len(), 2, "{m}");
}

#[test]
fn combine_self_target_error_uses_the_caller_body_name() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm", "body_name": "Bracket"}));
    let error = fail(&mut s, "feature.combine", json!({"body": "Bracket", "tools": ["Bracket"]}));
    assert!(error.message.contains("'Bracket' is both the target and a tool"), "{error:?}");
}

#[test]
fn a_patterned_feature_combines_into_the_base_by_commands() {
    let mut s = session();
    plate_and_block(&mut s);
    let r = run(
        &mut s,
        "feature.pattern_linear",
        json!({"direction": "Y", "count": "3", "spacing": "30 mm", "body": "Block"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r =
        run(&mut s, "feature.move_body", json!({"body": "Block", "translate": ["0 mm", "0 mm", "0 mm"], "copy": true}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run(&mut s, "feature.combine", json!({"tools": ["Block", "Copy1"], "body": "Extrude1"}));
    assert_eq!(r["rebuild_ok"], false, "the copy overlaps the pattern, which the mock kernel cannot unite: {r}");
    assert!(r["new_errors"].to_string().contains("Combine1"), "{r}");
    run(&mut s, "history.undo", json!({}));
    let r = run(&mut s, "feature.combine", json!({"tools": ["Block"], "body": "Extrude1"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run(&mut s, "query.body", json!({"body": "Extrude1", "names": true}));
    assert!((b["volume_mm3"].as_f64().unwrap() - (8000.0 + 3.0 * 500.0)).abs() < 1e-6, "{b}");
    assert!(b.to_string().contains("Pattern1/instance(2, Extrude2/cap(end))"), "{b}");
}

#[test]
fn combine_argument_errors() {
    let mut s = session();
    plate_and_block(&mut s);
    let before = text(&s);
    let cases = [
        json!({"tools": []}),
        json!({"tools": ["Nope"]}),
        json!({"tools": ["Block"], "body": "Nope"}),
        json!({"tools": ["Block"], "body": "Block"}),
        json!({"tools": ["Block"], "op": "new"}),
        json!({"tools": ["Block"], "op": "weld"}),
        json!({"tools": ["Block", "Extrude1"]}),
        json!({"tools": "Block"}),
        json!({}),
    ];
    for args in cases {
        let e = execute(&mut s, &CmdCtx::user(1), "feature.combine", args.clone()).unwrap_err();
        assert!(matches!(e.code.as_str(), "invalid_args" | "not_found"), "{args}: {e:?}");
    }
    assert_eq!(text(&s), before);
    // The mock kernel cannot subtract: a failing feature with a hint, nothing consumed.
    let r = run(&mut s, "feature.combine", json!({"tools": ["Block"], "op": "cut"}));
    assert_eq!(r["rebuild_ok"], false, "{r}");
    assert!(r["new_errors"].to_string().contains("cut"), "{r}");
    let m = run(&mut s, "query.model", json!({}));
    assert_eq!(m["bodies"].as_array().unwrap().len(), 2, "{m}");
    // Without any body to merge into.
    let mut empty = session();
    assert!(execute(&mut empty, &CmdCtx::user(1), "feature.combine", json!({"tools": ["A"]})).is_err());
}

#[test]
fn suppressing_a_combine_gives_the_tool_back() {
    let mut s = session();
    plate_and_block(&mut s);
    run(&mut s, "feature.combine", json!({"tools": ["Block"]}));
    run(&mut s, "feature.suppress", json!({"name": "Combine1", "suppressed": true}));
    let m = run(&mut s, "query.model", json!({}));
    assert_eq!(m["bodies"].as_array().unwrap().len(), 2, "{m}");
    // With the combine suppressed the tool can be targeted again; the latest body is the tool.
    let r = run(&mut s, "feature.move_body", json!({"translate": ["0 mm", "0 mm", "1 mm"]}));
    assert!(text(&s).contains("move Move1 (body: Extrude2,"), "{}", text(&s));
    assert_eq!(r["rebuild_ok"], true, "{r}");
}

#[test]
fn agent_policy_applies_to_the_new_commands() {
    let mut s = session();
    plate_and_block(&mut s);
    let a = CmdCtx::agent("bot", 3);
    s.config.read_only_agents = true;
    for (id, args) in [
        ("feature.plane", json!({"offset": "1 mm"})),
        ("feature.move_body", json!({"translate": ["1 mm", "0 mm", "0 mm"]})),
        ("feature.combine", json!({"tools": ["Block"]})),
    ] {
        let e = execute(&mut s, &a, id, args).unwrap_err();
        assert_eq!(e.code, "read_only", "{id}");
    }
}
