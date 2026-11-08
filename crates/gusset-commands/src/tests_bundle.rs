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

#[test]
fn a_branch_travels_between_sessions_and_merges() {
    let d = std::env::temp_dir().join(format!("gusset-bundle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let u = CmdCtx::user(1);
    // Alice builds a plate; Bob starts from the same empty project and adds a parameter.
    let mut alice = session();
    run(&mut alice, &u, "param.set", json!({"name": "width", "expr": "40 mm"}));
    run(&mut alice, &u, "sketch.create", json!({"plane": "XY"}));
    run(
        &mut alice,
        &u,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 30, "height": 20}),
    );
    run(&mut alice, &u, "feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm"}));
    alice.project_dir = Some(d.clone());
    let r = run(&mut alice, &u, "history.export_bundle", json!({"path": "alice.gusb"}));
    assert!(r["bytes"].as_u64().unwrap() > 0, "{r}");
    assert_eq!(
        execute(&mut alice, &u, "history.export_bundle", json!({"path": "alice.gusb"})).unwrap_err().code,
        "confirmation_required"
    );
    run(&mut alice, &u, "history.export_bundle", json!({"path": "alice.gusb", "overwrite": true}));
    let a = CmdCtx::agent("peer", 5);
    let wrong = execute(&mut alice, &a, "history.export_bundle", json!({"path": "alice.txt"})).unwrap_err();
    assert_eq!(wrong.code, "not_allowed");

    let mut bob = session();
    bob.project_dir = Some(d.clone());
    run(&mut bob, &u, "param.set", json!({"name": "depth", "expr": "12 mm"}));
    let r = run(&mut bob, &u, "history.import_bundle", json!({"path": "alice.gusb"}));
    assert_eq!(r["created"], true, "{r}");
    assert_eq!(r["name_user_text"], "from/Main");
    // The empty first iteration is shared; the other four are new.
    assert_eq!(r["iterations_reused"], 1, "{r}");
    assert_eq!(r["iterations_added"], 4, "{r}");
    // Importing again adds nothing.
    let again = run(&mut bob, &u, "history.import_bundle", json!({"path": "alice.gusb"}));
    assert_eq!(
        (again["iterations_added"].as_u64(), again["objects_added"].as_u64(), again["created"].clone()),
        (Some(0), Some(0), json!(false))
    );
    // Compare, then merge into Bob's main: both sides' work is there.
    let c = run(&mut bob, &u, "branch.compare", json!({"a": "main", "b": "from/Main", "base": true}));
    assert!(c.to_string().contains("width"), "{c}");
    let m = run(&mut bob, &u, "branch.merge", json!({"from": "from/Main"}));
    assert!(m.get("conflicts").is_none_or(|c| c.as_array().is_none_or(Vec::is_empty)), "{m}");
    let text = run(&mut bob, &u, "document.get_text", json!({}));
    let text = text.to_string();
    assert!(text.contains("width") && text.contains("depth") && text.contains("Extrude"), "{text}");
    // Refusals: junk file, missing file, agent outside the folder.
    std::fs::write(d.join("bad.gusb"), b"GUSBxxxx").unwrap();
    assert_eq!(
        execute(&mut bob, &u, "history.import_bundle", json!({"path": "bad.gusb"})).unwrap_err().code,
        "history"
    );
    assert_eq!(
        execute(&mut bob, &u, "history.import_bundle", json!({"path": "nope.gusb"})).unwrap_err().code,
        "not_found"
    );
    assert!(execute(&mut bob, &a, "history.import_bundle", json!({"path": "../alice.gusb"})).is_err());
    let _ = std::fs::remove_dir_all(&d);
}
