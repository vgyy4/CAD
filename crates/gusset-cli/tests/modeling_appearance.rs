//! Appearances (ADR 0045) through STEP: a coloured body exported and imported again keeps its
//! name and colour; an assembly's solids keep their own part names.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_commands::{CmdCtx, Session, execute};
use serde_json::{Value, json};
use std::sync::Arc;

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn session(dir: &std::path::Path) -> Session {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let worker = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gusset-worker");
    let k: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    s.project_dir = Some(dir.to_path_buf());
    s.importer = Some(Arc::new(gusset_worker::WorkerImporter::new(worker)));
    s
}

#[test]
fn a_coloured_step_round_trip_keeps_names_and_colours() {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-appearance-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = session(&dir);
    for (i, x) in [0, 30].into_iter().enumerate() {
        let sk = format!("Sketch{}", i + 1);
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.add_rectangle", json!({"sketch": sk, "x": x, "y": 0, "width": 20, "height": 10}));
        run(&mut s, "feature.extrude", json!({"sketch": sk, "distance": "5 mm", "op": "new"}));
    }
    run(&mut s, "body.appearance", json!({"body": "Extrude1", "color": "#c03020"}));
    run(&mut s, "body.appearance", json!({"body": "Extrude2", "color": "#2040c0", "finish": "metal"}));
    run(&mut s, "export.step", json!({"path": "two.step"}));
    // A fresh project imports both, each with its name as label and its colour.
    let mut t = session(&dir);
    let r = run(&mut t, "import.file", json!({"path": "two.step"}));
    assert_eq!(r["features"].as_array().unwrap().len(), 2, "{r}");
    let text = run(&mut t, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(text.contains("\"#c03020\"") && text.contains("\"#2040c0\""), "{text}");
    assert!(text.contains("label: \"Extrude1\"") && text.contains("label: \"Extrude2\""), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn face_colours_survive_import_and_export() {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-face-colours-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A STEP file whose plate has two faces in their own colours.
    let mut s = session(&dir);
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 20, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm", "op": "new"}));
    let ev = s.evaluate(&gusset_history::Actor::User).unwrap();
    let step = s.kernel().export_step(&[(&ev.bodies[0].shape, "Plate\u{1f}#808080\u{1f}0=d02020,3=2020d0")]).unwrap();
    std::fs::write(dir.join("faces.step"), step).unwrap();
    // Imported: the body colour and both face colours, by the import's face names.
    let mut t = session(&dir);
    run(&mut t, "import.file", json!({"path": "faces.step"}));
    let text = run(&mut t, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(text.contains("color: \"#808080\""), "{text}");
    assert!(text.contains("\"#d02020\": [Import1/face(") && text.contains("\"#2020d0\": [Import1/face("), "{text}");
    // Exported again and imported into a third project: still there.
    run(&mut t, "export.step", json!({"path": "again.step"}));
    let mut u = session(&dir);
    run(&mut u, "import.file", json!({"path": "again.step"}));
    let text = run(&mut u, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(text.contains("\"#d02020\": [Import1/face(") && text.contains("\"#2020d0\": [Import1/face("), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}
