//! Drawing commands (ADR 0032) through the registry (mock kernel).

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-drawing-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.project_dir = Some(dir);
    s
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn err(s: &mut Session, id: &str, args: Value) -> CommandError {
    execute(s, &CmdCtx::user(1), id, args).map(|v| panic!("{id} should fail: {v}")).unwrap_err()
}

fn plate(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm", "body_name": "Plate"}));
}

#[test]
fn a_drawing_with_standard_views_is_built_queried_and_exported() {
    let mut s = session();
    plate(&mut s);
    let r = run(&mut s, "drawing.create", json!({"title": "Plate", "standard_views": true}));
    assert_eq!(r["drawing"], "Drawing1", "{r}");
    let q = run(&mut s, "query.drawing", json!({"drawing": "Drawing1"}));
    assert_eq!(q["issues"], json!([]), "{q}");
    let views = q["views"].as_array().unwrap();
    assert_eq!(views.len(), 4, "{q}");
    let bx = |n: &str| views.iter().find(|v| v["name"] == n).unwrap()["box_mm"].clone();
    // A3 landscape: the front view (40 x 10) centred at (176.4, 184.1), rounded to 0.1 mm.
    assert_eq!(bx("Front"), json!([156.4, 179.1, 196.4, 189.1]), "{q}");
    assert_eq!(bx("Top"), json!([156.4, 139.1, 196.4, 159.1]), "first angle: below, 20 mm apart");
    assert!(views.iter().all(|v| v["off_sheet"] == false), "{q}");
    // A note; listing; the text form.
    run(&mut s, "drawing.add_note", json!({"drawing": "Drawing1", "text": "Break edges", "at": ["30", "40"]}));
    let list = run(&mut s, "query.drawing", json!({}));
    assert_eq!(list["drawings"][0]["items"], json!(["Front", "Top", "Right", "Iso", "Note1"]), "{list}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("drawing Drawing1 (size: A3, projection: first, title: \"Plate\") {"), "{text}");
    // Export both formats.
    for (path, magic) in [("plate.svg", "<svg"), ("plate.dxf", "AC1009")] {
        let r = run(&mut s, "drawing.export", json!({"drawing": "Drawing1", "path": path}));
        assert_eq!(r["written"], path, "{r}");
        let body = std::fs::read_to_string(s.project_dir.clone().unwrap().join(path)).unwrap();
        assert!(body.contains(magic), "{path}");
    }
    assert_eq!(
        err(&mut s, "drawing.export", json!({"drawing": "Drawing1", "path": "plate.svg"})).code,
        "confirmation_required"
    );
    assert_eq!(err(&mut s, "drawing.export", json!({"drawing": "Drawing1", "path": "plate.pdf"})).code, "invalid_args");
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}

#[test]
fn views_and_items_are_edited_and_deleted_in_order() {
    let mut s = session();
    plate(&mut s);
    run(&mut s, "drawing.create", json!({"name": "D", "projection": "third", "scale": "2"}));
    let r = run(&mut s, "drawing.add_view", json!({"drawing": "D", "from": "front", "at": ["100", "150"]}));
    assert_eq!(r["view"], "View1");
    assert_eq!(r["placed"]["box_mm"], json!([60.0, 140.0, 140.0, 160.0]), "{r}");
    let r = run(&mut s, "drawing.add_view", json!({"drawing": "D", "of": "View1", "side": "top"}));
    // Third angle: above.
    assert_eq!(r["placed"]["box_mm"], json!([60.0, 180.0, 140.0, 220.0]), "{r}");
    run(&mut s, "drawing.edit_view", json!({"drawing": "D", "view": "View1", "at": ["120", "150"]}));
    let q = run(&mut s, "query.drawing", json!({"drawing": "D"}));
    assert_eq!(q["views"][1]["box_mm"], json!([80.0, 180.0, 160.0, 220.0]), "the projected view follows: {q}");
    assert_eq!(
        err(&mut s, "drawing.edit_view", json!({"drawing": "D", "view": "View2", "at": ["0", "0"]})).code,
        "invalid_args"
    );
    let dim = json!({"drawing": "D", "view": "View1", "kind": "length",
                     "a": "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l1))"});
    let r = run(&mut s, "drawing.add_dimension", dim.clone());
    assert_eq!(r["dimension"], "D1", "{r}");
    let mut two = dim.clone();
    two["kind"] = json!("distance");
    assert_eq!(err(&mut s, "drawing.add_dimension", two).code, "invalid_args");
    // View1 is the parent of View2 and holds D1: it goes last.
    let e = err(&mut s, "drawing.delete", json!({"drawing": "D", "item": "View1"}));
    assert!(e.message.contains("View2, D1"), "{e:?}");
    run(&mut s, "drawing.delete", json!({"drawing": "D", "item": "D1"}));
    run(&mut s, "drawing.delete", json!({"drawing": "D", "item": "View2"}));
    run(&mut s, "drawing.delete", json!({"drawing": "D", "item": "View1"}));
    run(&mut s, "drawing.edit", json!({"drawing": "D", "title": "Bracket", "size": "A4", "portrait": true}));
    let q = run(&mut s, "query.drawing", json!({"drawing": "D"}));
    assert_eq!(q["sheet_mm"], json!([210.0, 297.0]), "{q}");
    run(&mut s, "drawing.delete", json!({"drawing": "D"}));
    assert_eq!(run(&mut s, "query.drawing", json!({}))["drawings"], json!([]));
    // A view of a body that does not exist is refused by validation.
    run(&mut s, "drawing.create", json!({"name": "E"}));
    let e =
        err(&mut s, "drawing.add_view", json!({"drawing": "E", "from": "top", "at": ["0", "0"], "bodies": ["Nope"]}));
    assert!(e.message.contains("not a body") || e.to_string().contains("not a body"), "{e:?}");
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}
