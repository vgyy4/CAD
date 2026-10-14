//! Components from other documents (ADR 0026 §7) through the registry (mock kernel).

use crate::*;
use gusset_doc::Document;
use gusset_history::{Ctx, History, NodeState};
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PART: &str = "gusset 1
units mm

param h = 2 mm

sketch S (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 4)
  p4 = point(0, 4)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Pin (profile: S, distance: h, op: new)
";

fn dir() -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let d = std::env::temp_dir().join(format!("gusset-component-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Writes `docs` as successive versions of a project file at `path`.
fn write_part(path: &Path, docs: &[&str]) {
    let u = Ctx::user(1);
    let mut h = History::new(&Document::from_text(docs.first().unwrap()).unwrap(), &u);
    for d in docs.iter().skip(1) {
        let main = h.main();
        h.commit(&u, main, &Document::from_text(d).unwrap(), "edit", NodeState::Ok).unwrap();
    }
    let meta = gusset_project::ProjectMeta {
        id: gusset_store::paths::ProjectId::new_random().unwrap(),
        save_seq: 1,
        base_seq: 1,
        created: 1,
    };
    std::fs::write(path, gusset_project::encode(&h, &meta).unwrap()).unwrap();
}

fn session(dir: &Path) -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    s.project_dir = Some(dir.to_path_buf());
    s
}

fn run(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> Value {
    execute(s, ctx, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

#[test]
fn a_component_brings_in_a_pinned_body_that_joints_can_place() {
    let d = dir();
    write_part(&d.join("pin.gus"), &[PART]);
    let mut s = session(&d);
    let u = CmdCtx::user(1);
    let r = run(&mut s, &u, "assembly.insert_component", json!({"path": "pin.gus"}));
    assert_eq!(r["feature"], "Pin", "{r}");
    assert_eq!(r["rebuild_ok"], true, "{r}");
    // Its faces are the part's, wrapped.
    let b = run(&mut s, &u, "query.body", json!({"body": "Pin", "names": true}));
    let faces = b["face_names"].to_string();
    assert!(faces.contains("Pin/part(Pin/cap(end))"), "{faces}");
    assert_eq!(b["volume_mm3"], 80.0, "{b}");
    // A joint places it by those names.
    let r = run(
        &mut s,
        &u,
        "feature.joint",
        json!({"type": "rigid", "body": "Pin", "at": "Pin/part(Pin/cap(start))", "to": "origin"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    // The source changes; the component does not until it is updated.
    write_part(&d.join("pin.gus"), &[PART, &PART.replace("param h = 2 mm", "param h = 4 mm")]);
    let b = run(&mut s, &u, "query.body", json!({"body": "Pin"}));
    assert_eq!(b["volume_mm3"], 80.0, "{b}");
    let r = run(&mut s, &u, "assembly.update_component", json!({"component": "Pin"}));
    assert_eq!(r["changed"], true, "{r}");
    let b = run(&mut s, &u, "query.body", json!({"body": "Pin"}));
    assert_eq!(b["volume_mm3"], 160.0, "{b}");
    let r = run(&mut s, &u, "assembly.update_component", json!({"component": "Pin"}));
    assert_eq!(r["changed"], false, "{r}");
    // The text form keeps the pin.
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("component Pin (source: \"pin.gus\", version: \"n2\", document: \""), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn component_errors() {
    let d = dir();
    write_part(&d.join("pin.gus"), &[PART]);
    std::fs::write(d.join("junk.gus"), b"not a project").unwrap();
    let mut s = session(&d);
    let u = CmdCtx::user(1);
    let code = |s: &mut Session, args: Value| execute(s, &u, "assembly.insert_component", args).unwrap_err().code;
    assert_eq!(code(&mut s, json!({"path": "missing.gus"})), "not_found");
    assert_eq!(code(&mut s, json!({"path": "junk.gus"})), "io");
    assert_eq!(code(&mut s, json!({"path": "pin.step"})), "invalid_args");
    assert_eq!(code(&mut s, json!({"path": "pin.gus", "body": "Nope"})), "not_found");
    assert_eq!(code(&mut s, json!({"path": "pin.gus", "branch": "Nope"})), "not_found");
    // Agents stay inside the project folder.
    let a = CmdCtx::agent("bot", 1);
    let e = execute(&mut s, &a, "assembly.insert_component", json!({"path": "../pin.gus"})).unwrap_err();
    assert_eq!(e.code, "not_allowed", "{e:?}");
    // A component whose stored document is missing fails with a clear error.
    let hash = "0".repeat(64);
    let doc = Document::from_text(&format!(
        "gusset 1\nunits mm\ncomponent C (source: \"x.gus\", version: \"n1\", document: \"{hash}\")\n"
    ))
    .unwrap();
    let mut s2 = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    let ev = s2.evaluate(&gusset_history::Actor::User).unwrap();
    assert!(ev.errors().iter().any(|e| e.message.contains("missing")), "{:?}", ev.errors());
    let _ = std::fs::remove_dir_all(&d);
}
