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
    let q = run(&mut s, &u, "query.components", json!({}));
    assert_eq!(q["components"][0]["update_available"], true, "{q}");
    let r = run(&mut s, &u, "assembly.update_component", json!({"component": "Pin"}));
    assert_eq!(r["changed"], true, "{r}");
    let q = run(&mut s, &u, "query.components", json!({}));
    assert_eq!(q["components"][0]["update_available"], false, "{q}");
    assert_eq!(q["components"][0]["version"], "n2", "{q}");
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

#[test]
#[allow(clippy::float_cmp)] // the mock kernel's box volumes are exact
fn components_take_parameters_from_this_document() {
    let d = dir();
    write_part(&d.join("pin.gus"), &[PART]);
    let mut s = session(&d);
    let u = CmdCtx::user(1);
    let vol = |s: &mut Session, b: &str| {
        execute(s, &CmdCtx::user(1), "query.body", json!({"body": b})).unwrap()["volume_mm3"].as_f64().unwrap()
    };
    run(&mut s, &u, "assembly.insert_component", json!({"path": "pin.gus", "set": {"h": "3 mm"}}));
    assert_eq!(vol(&mut s, "Pin"), 120.0);
    // Two sizes of one part, the second driven by a parameter here.
    run(&mut s, &u, "param.set", json!({"name": "depth", "expr": "5 mm"}));
    run(&mut s, &u, "assembly.insert_component", json!({"path": "pin.gus", "name": "Long"}));
    let r = run(&mut s, &u, "assembly.configure_component", json!({"component": "Long", "set": {"h": "depth"}}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert_eq!((vol(&mut s, "Pin"), vol(&mut s, "Long")), (120.0, 200.0));
    run(&mut s, &u, "param.set", json!({"name": "depth", "expr": "1 cm"}));
    assert_eq!(vol(&mut s, "Long"), 400.0);
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains(", set: [h = depth])"), "{text}");
    // null restores the part's own value; unknown parameters are refused.
    run(&mut s, &u, "assembly.configure_component", json!({"component": "Long", "set": {"h": null}}));
    assert_eq!(vol(&mut s, "Long"), 80.0);
    let e = execute(&mut s, &u, "assembly.configure_component", json!({"component": "Long", "set": {"nope": "1"}}));
    assert_eq!(e.unwrap_err().code, "not_found");
    let e = execute(&mut s, &u, "assembly.insert_component", json!({"path": "pin.gus", "set": {"h": "2 deg"}}));
    // A wrong unit is the component's rebuild error.
    assert!(e.is_ok_and(|r| r["rebuild_ok"] == false));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_component_can_use_a_configuration_of_its_document() {
    let d = dir();
    let part = PART.replace("param h = 2 mm\n", "param h = 2 mm\nconfiguration Long (set: [h = 10 mm])\n");
    write_part(&d.join("pin.gus"), &[&part]);
    let mut s = session(&d);
    let u = CmdCtx::user(1);
    run(&mut s, &u, "assembly.insert_component", json!({"path": "pin.gus"}));
    let vol = |s: &mut Session| run(s, &CmdCtx::user(1), "query.body", json!({"body": "Pin"}))["volume_mm3"].clone();
    assert_eq!(vol(&mut s), 80.0);
    run(&mut s, &u, "assembly.configure_component", json!({"component": "Pin", "configuration": "Long"}));
    assert_eq!(vol(&mut s), 400.0);
    let e = execute(&mut s, &u, "assembly.configure_component", json!({"component": "Pin", "configuration": "Nope"}))
        .unwrap_err();
    assert!(e.hint.unwrap_or_default().contains("Long"));
    run(&mut s, &u, "assembly.configure_component", json!({"component": "Pin", "configuration": ""}));
    assert_eq!(vol(&mut s), 80.0);
}

#[test]
fn the_check_after_opening_reads_only_sources_inside_the_project() {
    // Review v0.3 H1: an opened file must not make the app read other files.
    let (d, other) = (dir(), dir());
    let outside = other.join("pin.gus");
    write_part(&outside, &[PART]);
    write_part(&d.join("pin.gus"), &[PART]);
    let mut s = session(&d);
    let u = CmdCtx::user(1);
    run(&mut s, &u, "assembly.insert_component", json!({"path": outside.to_str().unwrap(), "name": "Far"}));
    run(&mut s, &u, "assembly.insert_component", json!({"path": "pin.gus", "name": "Near"}));
    run(&mut s, &u, "assembly.insert_component", json!({"path": "pin.gus", "name": "Near2"}));
    let q = run(&mut s, &u, "query.components", json!({"local_only": true}));
    let c = q["components"].as_array().unwrap();
    assert!(c[0]["update_available"].is_null(), "{q}");
    assert!(c[0]["source_error"].as_str().unwrap().contains("outside the project folder"), "{q}");
    assert_eq!(c[1]["update_available"], false, "{q}");
    assert_eq!(c[2]["update_available"], false, "{q}");
    // Asked for explicitly, the user's own source is read.
    let q = run(&mut s, &u, "query.components", json!({}));
    assert_eq!(q["components"][0]["update_available"], false, "{q}");
    // Network paths are refused for everyone.
    for p in ["//host/share/pin.gus", "\\\\host\\share\\pin.gus"] {
        let e = execute(&mut s, &u, "assembly.insert_component", json!({"path": p})).unwrap_err();
        assert_eq!(e.code, "not_allowed", "{p}: {e:?}");
    }
}

#[test]
fn identical_nested_components_are_copied_and_built_once() {
    // Review v0.3 H3: 4 components per level over 6 levels was 4^6 copies and builds.
    let d = dir();
    write_part(&d.join("l0.gus"), &[PART]);
    let u = CmdCtx::user(1);
    let meta = gusset_project::ProjectMeta {
        id: gusset_store::paths::ProjectId::new_random().unwrap(),
        save_seq: 1,
        base_seq: 1,
        created: 1,
    };
    let start = std::time::Instant::now();
    for level in 1..=6 {
        let mut s = session(&d);
        for i in 0..4 {
            let r = run(
                &mut s,
                &u,
                "assembly.insert_component",
                json!({"path": format!("l{}.gus", level - 1), "name": format!("C{i}")}),
            );
            assert_eq!(r["rebuild_ok"], true, "level {level}: {r}");
        }
        std::fs::write(d.join(format!("l{level}.gus")), gusset_project::encode(s.history(), &meta).unwrap()).unwrap();
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(60), "took {:?}", start.elapsed());
}
