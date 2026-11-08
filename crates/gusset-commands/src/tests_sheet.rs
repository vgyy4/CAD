//! Sheet-metal commands (ADR 0037) through the registry (mock kernel: straight edges only,
//! so the flange here has one line; bends are tested with OCCT in `modeling_sheet.rs`).

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    // Parallel tests can read the same clock value (Windows): a sequence number keeps their
    // folders apart.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = format!("{n}-{}", SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let dir = std::env::temp_dir().join(format!("gusset-sheet-{}-{n}", std::process::id()));
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

#[test]
fn a_contour_flange_is_made_queried_and_exported_flat() {
    let mut s = session();
    assert_eq!(err(&mut s, "query.flat_pattern", json!({})).code, "not_found");
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_polyline", json!({"sketch": "Sketch1", "points": [[0, 0], [80, 0]]}));
    let r = run(
        &mut s,
        "feature.contour_flange",
        json!({"curves": "Sketch1.curves(l3)", "width": "30 mm", "thickness": "1.5 mm"}),
    );
    assert_eq!(r["feature"], "Flange1", "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(
        text.contains("contour_flange Flange1 (curves: Sketch1.curves(l3), width: 30 mm, thickness: 1.5 mm, bend_radius: 1.5 mm, k_factor: 0.44)"),
        "{text}"
    );
    let q = run(&mut s, "query.flat_pattern", json!({}));
    assert_eq!(q["size_mm"], json!([80.0, 30.0]), "{q}");
    assert_eq!(q["bends"], json!([]), "{q}");
    for (path, magic) in [("flat.dxf", "AC1015"), ("flat.svg", "<svg")] {
        let w = run(&mut s, "sheet.export_flat", json!({"path": path}));
        assert_eq!(w["written"], path, "{w}");
        let body =
            String::from_utf8_lossy(&std::fs::read(s.project_dir.clone().unwrap().join(path)).unwrap()).into_owned();
        assert!(body.contains(magic), "{path}");
    }
    assert_eq!(err(&mut s, "sheet.export_flat", json!({"path": "flat.svg"})).code, "confirmation_required");
    // A tab, and a body that is not sheet metal.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 0, "y": 40, "width": 20, "height": 10}));
    run(&mut s, "feature.tab", json!({"profile": "Sketch2", "thickness": "1.5 mm"}));
    let q = run(&mut s, "query.flat_pattern", json!({"body": "Tab1"}));
    assert_eq!(q["size_mm"], json!([20.0, 10.0]), "{q}");
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch3", "x": 0, "y": 60, "width": 5, "height": 5}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch3", "distance": "5 mm", "body_name": "Block"}));
    assert_eq!(err(&mut s, "query.flat_pattern", json!({"body": "Block"})).code, "invalid_args");
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}

#[test]
fn a_tab_changed_by_a_move_has_a_marked_flat_pattern() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.tab", json!({"profile": "Sketch1", "thickness": "2 mm"}));
    run(&mut s, "feature.move_body", json!({"body": "Tab1", "translate": ["10 mm", "0 mm", "0 mm"]}));
    let q = run(&mut s, "query.flat_pattern", json!({}));
    assert_eq!(q["changed_by"], json!(["Move1"]), "{q}");
    assert!(q["warning"].as_str().is_some_and(|w| w.contains("Move1")), "{q}");
    assert_eq!(err(&mut s, "sheet.export_flat", json!({"path": "flat.dxf"})).code, "invalid_args");
    run(&mut s, "sheet.export_flat", json!({"path": "flat.dxf", "allow_changed": true}));
    // An edge flange after the cut is refused, and so is a line that is not a name.
    let ev = s.evaluate(&gusset_history::Actor::User).unwrap();
    assert!(ev.errors().is_empty());
    let r = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.edge_flange",
        json!({"body": "Tab1", "line": "l3", "length": "10 mm"}),
    );
    let ev = s.evaluate(&gusset_history::Actor::User).unwrap();
    assert!(r.is_err() || ev.errors().iter().any(|e| e.message.contains("changed by Move1")), "{:?}", ev.errors());
    assert_eq!(
        err(&mut s, "feature.edge_flange", json!({"body": "Tab1", "line": "l3) x", "length": "10 mm"})).code,
        "invalid_args"
    );
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}
