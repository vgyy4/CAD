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
    for (path, profile, magic) in [
        ("plate.svg", None, "<svg"),
        ("plate.dxf", None, "AC1015"),
        ("plate12.dxf", Some("r12"), "AC1009"),
        ("plate.pdf", None, "%PDF-1.4"),
    ] {
        let r = run(&mut s, "drawing.export", json!({"drawing": "Drawing1", "path": path, "profile": profile}));
        assert_eq!(r["written"], path, "{r}");
        let body =
            String::from_utf8_lossy(&std::fs::read(s.project_dir.clone().unwrap().join(path)).unwrap()).into_owned();
        assert!(body.contains(magic), "{path}");
    }
    assert_eq!(
        err(&mut s, "drawing.export", json!({"drawing": "Drawing1", "path": "plate.svg"})).code,
        "confirmation_required"
    );
    assert_eq!(err(&mut s, "drawing.export", json!({"drawing": "Drawing1", "path": "plate.png"})).code, "invalid_args");
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
                     "a": "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))"});
    let r = run(&mut s, "drawing.add_dimension", dim.clone());
    assert_eq!(r["dimension"], "D1", "{r}");
    run(
        &mut s,
        "drawing.edit_dimension",
        json!({"drawing": "D", "dimension": "D1", "tolerance": "±0.05", "text_at": ["100", "170"]}),
    );
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("text_at: [100, 170], tolerance: \"±0.05\")"), "{text}");
    run(&mut s, "drawing.edit_dimension", json!({"drawing": "D", "dimension": "D1", "tolerance": "", "text_at": []}));
    // Moving on the sheet: a base view, a dimension's text; a projected view refuses.
    run(&mut s, "drawing.move_item", json!({"drawing": "D", "item": "View1", "at": ["130", "150"]}));
    run(&mut s, "drawing.move_item", json!({"drawing": "D", "item": "D1", "at": ["130", "120"]}));
    let e = err(&mut s, "drawing.move_item", json!({"drawing": "D", "item": "View2", "at": ["0", "0"]}));
    assert!(e.message.contains("follows its parent"), "{e:?}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(
        text.contains("view View1 (from: front, at: [130, 150])") && text.contains("text_at: [130, 120]"),
        "{text}"
    );
    let q = run(&mut s, "query.drawing", json!({"drawing": "D"}));
    assert_eq!(q["dimensions"][0]["value"], 40.0, "{q}");
    run(&mut s, "drawing.edit_dimension", json!({"drawing": "D", "dimension": "D1", "text_at": []}));
    // A detail view, moved like a base view.
    let r = run(
        &mut s,
        "drawing.add_view",
        json!({"drawing": "D", "name": "Det", "detail_of": "View1", "center": ["0", "0"], "radius": "4 mm", "at": ["300", "100"]}),
    );
    assert_eq!(r["view"], "Det", "{r}");
    run(&mut s, "drawing.move_item", json!({"drawing": "D", "item": "Det", "at": ["310", "100"]}));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("view Det (detail_of: View1, center: [0, 0], radius: 4 mm, at: [310, 100])"), "{text}");
    let e = err(&mut s, "drawing.add_view", json!({"drawing": "D", "detail_of": "View1", "at": ["1", "1"]}));
    assert!(e.message.contains("center"), "{e:?}");
    run(&mut s, "drawing.delete", json!({"drawing": "D", "item": "Det"}));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(!text.contains("text_at") && !text.contains("tolerance"), "{text}");
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
    // A view of a body that does not exist is refused (by the sheet check, or by validation).
    run(&mut s, "drawing.create", json!({"name": "E"}));
    let e =
        err(&mut s, "drawing.add_view", json!({"drawing": "E", "from": "top", "at": ["0", "0"], "bodies": ["Nope"]}));
    assert!(e.message.contains("not a body") || e.message.contains("no body"), "{e:?}");
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}

#[test]
fn a_dxf_file_becomes_sketch_geometry() {
    let mut s = session();
    plate(&mut s);
    let dir = s.project_dir.clone().unwrap();
    // A 1 x 0.5 inch closed polyline and a 0.1 inch circle.
    let dxf = "  0\nSECTION\n  2\nHEADER\n  9\n$INSUNITS\n 70\n1\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n\
  0\nLWPOLYLINE\n 90\n4\n 70\n1\n 10\n0\n 20\n0\n 10\n1\n 20\n0\n 10\n1\n 20\n0.5\n 10\n0\n 20\n0.5\n\
  0\nCIRCLE\n 10\n0.5\n 20\n0.25\n 40\n0.1\n\
  0\nARC\n 10\n0.2\n 20\n0.2\n 40\n0.05\n 50\n0\n 51\n360\n\
  0\nARC\n 10\n0.8\n 20\n0.2\n 40\n0.05\n 50\n0\n 51\n0.0000001\n  0\nENDSEC\n  0\nEOF\n";
    std::fs::write(dir.join("plate.dxf"), dxf).unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
    let r = run(&mut s, "sketch.import_dxf", json!({"sketch": "Cut", "path": "plate.dxf"}));
    assert_eq!((r["lines"].as_u64(), r["circles"].as_u64(), r["points"].as_u64()), (Some(4), Some(2), Some(6)), "{r}");
    // A full-sweep ARC is a circle; an arc whose ends meet after snapping is dropped (D11).
    assert_eq!(r["arcs"].as_u64(), Some(0), "{r}");
    assert_eq!(r["unit_mm"], 25.4);
    // The square is a closed profile: it extrudes (25.4 x 12.7 mm, minus the hole).
    let r = run(&mut s, "feature.extrude", json!({"sketch": "Cut", "distance": "2 mm"}));
    assert!(r.get("feature").is_some(), "{r}");
    // Our own DXF exports read back.
    run(&mut s, "drawing.create", json!({"standard_views": true}));
    run(&mut s, "drawing.export", json!({"drawing": "Drawing1", "path": "sheet.dxf"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Sheet"}));
    let r = run(&mut s, "sketch.import_dxf", json!({"sketch": "Sheet", "path": "sheet.dxf"}));
    assert!(r["lines"].as_u64().unwrap() > 20, "{r}");
    assert!(r["skipped_user_text"].as_array().is_some_and(|a| !a.is_empty()), "texts and fills are skipped: {r}");
    // Not a DXF, and a file that is not there.
    std::fs::write(dir.join("junk.dxf"), "hello").unwrap();
    assert_eq!(err(&mut s, "sketch.import_dxf", json!({"sketch": "Cut", "path": "junk.dxf"})).code, "io");
    assert_eq!(err(&mut s, "sketch.import_dxf", json!({"sketch": "Cut", "path": "plate.svg"})).code, "invalid_args");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn renaming_or_deleting_a_measured_feature_keeps_the_drawing() {
    // Review D4.
    let mut s = session();
    plate(&mut s);
    run(&mut s, "drawing.create", json!({"name": "D"}));
    run(
        &mut s,
        "drawing.add_view",
        json!({"drawing": "D", "name": "F", "bodies": ["Extrude1"], "from": "front", "at": ["100", "100"]}),
    );
    let e = "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))";
    run(&mut s, "drawing.add_dimension", json!({"drawing": "D", "view": "F", "kind": "length", "a": e}));
    run(&mut s, "feature.rename", json!({"from": "Extrude1", "to": "Base"}));
    run(&mut s, "feature.rename", json!({"from": "Sketch1", "to": "Prof"}));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("bodies: [Base]") && text.contains("edge(Base/cap(end), Base/side(Prof/l5))"), "{text}");
    let q = run(&mut s, "query.drawing", json!({"drawing": "D"}));
    assert_eq!(q["dimensions"][0]["value"], 40.0, "{q}");
}

#[test]
fn drawing_items_the_sheet_cannot_draw_are_refused() {
    // Dogfooding F5-5: these were committed with rebuild_ok and failed only on the sheet.
    let mut s = session();
    plate(&mut s);
    let d = |s: &mut Session, args: Value| err(s, "drawing.create", args);
    for bad in ["abc", "-1", "0"] {
        assert_eq!(d(&mut s, json!({"name": "X", "scale": bad})).code, "invalid_args", "{bad}");
    }
    run(&mut s, "drawing.create", json!({"name": "D"}));
    let nodes = s.history().nodes().count();
    let view = |s: &mut Session, extra: Value| {
        let mut a = json!({"drawing": "D", "name": "F", "bodies": ["Extrude1"], "from": "front", "at": ["100", "100"]});
        if let (Value::Object(m), Value::Object(e)) = (&mut a, extra) {
            m.extend(e);
        }
        execute(s, &CmdCtx::user(1), "drawing.add_view", a)
    };
    assert_eq!(view(&mut s, json!({"scale": "1e9"})).unwrap_err().code, "invalid_args");
    assert_eq!(s.history().nodes().count(), nodes, "nothing was committed");
    view(&mut s, json!({})).unwrap();
    let e = "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))";
    let dim = |s: &mut Session, kind: &str| {
        execute(
            s,
            &CmdCtx::user(1),
            "drawing.add_dimension",
            json!({"drawing": "D", "view": "F", "kind": kind, "a": e}),
        )
    };
    let r = dim(&mut s, "diameter").unwrap_err();
    assert!(r.code == "invalid_args" && r.message.contains("circular"), "{r:?}");
    dim(&mut s, "length").unwrap();
    run(&mut s, "drawing.edit_dimension", json!({"drawing": "D", "dimension": "D1", "offset": "15"}));
}

#[test]
fn deleting_a_measured_feature_names_the_dangling_dimensions() {
    // Dogfooding F5-8: the delete said nothing.
    let mut s = session();
    plate(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 60, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "5 mm", "body_name": "Block"}));
    run(&mut s, "drawing.create", json!({"name": "D"}));
    run(&mut s, "drawing.add_view", json!({"drawing": "D", "name": "F", "from": "front", "at": ["150", "150"]}));
    let e = "edge(Extrude2/cap(end), Extrude2/side(Sketch2/l5))";
    run(&mut s, "drawing.add_dimension", json!({"drawing": "D", "view": "F", "kind": "length", "a": e}));
    let r = run(&mut s, "feature.delete", json!({"name": "Extrude2"}));
    assert_eq!(r["dangling_dimensions"], json!(["D/D1"]), "{r}");
}

#[test]
fn a_drawing_can_dimension_in_inches_with_its_own_decimals() {
    // Dogfooding F5-14: dimensions were in the document's unit only.
    let mut s = session();
    plate(&mut s);
    run(&mut s, "drawing.create", json!({"name": "D", "units": "in", "decimals": 2}));
    run(&mut s, "drawing.add_view", json!({"drawing": "D", "name": "F", "from": "front", "at": ["150", "150"]}));
    let e = "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))";
    run(&mut s, "drawing.add_dimension", json!({"drawing": "D", "view": "F", "kind": "length", "a": e}));
    let q = run(&mut s, "query.drawing", json!({"drawing": "D"}));
    assert_eq!(q["dimensions"][0]["text_user_text"], "1.57", "{q}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("units: in, decimals: 2"), "{text}");
    assert_eq!(gusset_doc::Document::from_text(&text).unwrap().to_text(), text);
    run(&mut s, "drawing.edit", json!({"drawing": "D", "units": "", "decimals": 1}));
    let q = run(&mut s, "query.drawing", json!({"drawing": "D"}));
    assert_eq!(q["dimensions"][0]["text_user_text"], "40", "{q}");
    assert_eq!(err(&mut s, "drawing.edit", json!({"drawing": "D", "units": "ft"})).code, "invalid_args");
    // Dual dimensions: the other unit in brackets.
    run(&mut s, "drawing.edit", json!({"drawing": "D", "dual": true, "decimals": 2}));
    let q = run(&mut s, "query.drawing", json!({"drawing": "D"}));
    assert_eq!(q["dimensions"][0]["text_user_text"], "40 [1.575]", "{q}");
    assert!(s.document(&gusset_history::Actor::User).unwrap().to_text().contains("dual: true"));
}

#[test]
fn a_parts_list_shows_the_bill_of_materials() {
    let mut s = session();
    plate(&mut s);
    for (i, x) in [60, 80].iter().enumerate() {
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        let sk = format!("Sketch{}", i + 2);
        run(&mut s, "sketch.add_rectangle", json!({"sketch": sk, "x": x, "y": 0, "width": 10, "height": 10}));
        run(
            &mut s,
            "feature.extrude",
            json!({"sketch": sk, "distance": "5 mm", "body_name": format!("Block{}", i + 1)}),
        );
    }
    run(&mut s, "drawing.create", json!({"name": "D"}));
    let r = run(&mut s, "drawing.add_parts_list", json!({"drawing": "D", "at": ["30", "120"]}));
    assert_eq!(r["parts_list"], "Parts1", "{r}");
    let sc = crate::commands::drawing_scene(&mut s, &gusset_history::Actor::User, "D").unwrap();
    let texts: Vec<&str> = sc
        .items
        .iter()
        .filter_map(|i| match &i.prim {
            gusset_drawing::scene::Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for t in ["ITEM", "PART", "QTY", "Plate", "Block1", "2"] {
        assert!(texts.contains(&t), "{t}: {texts:?}");
    }
    let bom = run(&mut s, "query.bom", json!({}));
    assert_eq!(bom["rows"].as_array().unwrap().len(), 2, "{bom}");
    run(&mut s, "drawing.move_item", json!({"drawing": "D", "item": "Parts1", "at": ["40", "130"]}));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("parts_list Parts1 (at: [40, 130])"), "{text}");
    assert_eq!(gusset_doc::Document::from_text(&text).unwrap().to_text(), text);
}

#[test]
fn a_balloon_names_a_part_and_points_at_it() {
    let mut s = session();
    plate(&mut s);
    run(&mut s, "drawing.create", json!({"name": "D"}));
    run(&mut s, "drawing.add_view", json!({"drawing": "D", "name": "F", "from": "top", "at": ["150", "150"]}));
    run(&mut s, "drawing.add_parts_list", json!({"drawing": "D", "at": ["30", "120"]}));
    let r =
        run(&mut s, "drawing.add_balloon", json!({"drawing": "D", "view": "F", "body": "Plate", "at": ["200", "190"]}));
    assert_eq!(r["balloon"], "Balloon1", "{r}");
    let sc = crate::commands::drawing_scene(&mut s, &gusset_history::Actor::User, "D").unwrap();
    assert!(sc.issues.is_empty(), "{:?}", sc.issues);
    // The leader ends at the plate's centre in the top view: (20, 10) of 40 x 20 at 1:1.
    let ends: Vec<[f64; 2]> = sc
        .items
        .iter()
        .filter_map(|i| match i.prim {
            gusset_drawing::scene::Prim::Arc { center, radius, .. } if (radius - 0.6).abs() < 1e-9 => Some(center),
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert!((ends[0][0] - 150.0).abs() < 1e-6 && (ends[0][1] - 150.0).abs() < 1e-6, "{ends:?}");
    // An unknown body is refused.
    let e =
        err(&mut s, "drawing.add_balloon", json!({"drawing": "D", "view": "F", "body": "Nope", "at": ["200", "190"]}));
    assert_eq!(e.code, "invalid_args", "{e:?}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("balloon Balloon1 (view: F, body: Plate, at: [200, 190])"), "{text}");
    assert_eq!(gusset_doc::Document::from_text(&text).unwrap().to_text(), text);
}

#[test]
fn gdt_frames_and_datums_are_drawn_and_checked() {
    let mut s = session();
    plate(&mut s);
    run(&mut s, "drawing.create", json!({"name": "D"}));
    run(&mut s, "drawing.add_view", json!({"drawing": "D", "name": "F", "from": "top", "at": ["150", "150"]}));
    let e5 = "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))";
    let e6 = "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))";
    run(
        &mut s,
        "drawing.add_datum",
        json!({"drawing": "D", "view": "F", "a": e5, "letter": "A", "at": ["150", "120"]}),
    );
    let r = run(
        &mut s,
        "drawing.add_gdt",
        json!({"drawing": "D", "view": "F", "a": e6, "symbol": "perpendicularity", "tolerance": "0.05", "datums": ["A"], "at": ["190", "160"]}),
    );
    assert_eq!(r["gdt"], "Gdt1", "{r}");
    let sc = crate::commands::drawing_scene(&mut s, &gusset_history::Actor::User, "D").unwrap();
    assert!(sc.issues.is_empty(), "{:?}", sc.issues);
    let texts: Vec<&str> = sc
        .items
        .iter()
        .filter_map(|i| match &i.prim {
            gusset_drawing::scene::Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(texts.contains(&"0.05") && texts.iter().filter(|t| **t == "A").count() == 2, "{texts:?}");
    // A form tolerance takes no datum; a frame names only datums on the drawing.
    let bad = |s: &mut Session, symbol: &str, datum: &str| {
        err(
            s,
            "drawing.add_gdt",
            json!({"drawing": "D", "view": "F", "a": e6, "symbol": symbol, "tolerance": "0.1", "datums": [datum], "at": ["190", "100"]}),
        )
    };
    assert_eq!(bad(&mut s, "flatness", "A").code, "invalid_document");
    assert_eq!(bad(&mut s, "position", "B").code, "invalid_document");
    assert_eq!(bad(&mut s, "roundness", "A").code, "invalid_args");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("symbol: perpendicularity, tolerance: \"0.05\", datums: [A]"), "{text}");
    assert_eq!(gusset_doc::Document::from_text(&text).unwrap().to_text(), text);
}
