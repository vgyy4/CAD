//! UI automation (egui_kittest drives the real UI through the accessibility tree).

use crate::app::{App, search};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use gusset_kernel::mock::MockKernel;
use gusset_mcp::{ApprovalDecision, ApprovalProvider, ApprovalRequest};
use serde_json::json;
use std::sync::Arc;

fn app() -> App {
    let dir = std::env::temp_dir().join(format!("gusset-app-test-{}-{}", std::process::id(), rand_suffix()));
    let dirs = gusset_store::paths::DataDirs::at(dir.join("data"), dir.join("cache"));
    App::new(Arc::new(MockKernel), Some(dirs))
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

fn harness(app: App) -> Harness<'static, App> {
    // Host change events wake the UI from another thread and can add a frame or two.
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_max_steps(12)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app)
}

#[test]
fn empty_state_and_modeling_through_forms() {
    let mut h = harness(app());
    h.run();
    h.get_by_label("Start your first part");
    // Create a sketch through the generated form.
    h.get_by_label("New sketch on XY").click();
    h.run();
    assert!(h.state().form.as_ref().is_some_and(|f| f.command == "sketch.create"));
    h.state_mut().apply_form();
    h.state_mut().wait_idle();
    h.run();
    assert!(h.state().form.is_none(), "{:?}", h.state().form.as_ref().and_then(|f| f.error.clone()));
    // Rectangle + extrude via commands (same registry the forms use).
    h.state_mut()
        .run("sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0.0, "y": 0.0, "width": 40.0, "height": 20.0}))
        .unwrap();
    h.state_mut().wait_idle();
    h.state_mut().start_command("feature.extrude");
    h.run();
    {
        let form = h.state_mut().form.as_mut().unwrap();
        assert_eq!(form.fields.iter().find(|f| f.name == "sketch").map(|f| f.value.as_str()), Some("Sketch1"));
        form.set("distance", "10 mm");
    }
    h.state_mut().apply_form();
    h.state_mut().wait_idle();
    h.run();
    let ev = h.state().evaluation().unwrap();
    assert!(h.state().is_dirty());
    // The status bar reflects unsaved work and the browser lists the features.
    h.get_by_label_contains("Unsaved changes");
    h.get_by_label_contains("sketch Sketch1");
    h.get_by_label_contains("extrude Extrude1");
    assert!(h.state().form.is_none());
    assert_eq!(ev.bodies.len(), 1, "{:?}", ev.errors());
}

#[test]
fn save_as_then_reopen_roundtrips() {
    let mut a = app();
    a.run("param.set", json!({"name": "w", "expr": "12 mm"})).unwrap();
    let dir = std::env::temp_dir().join(format!("gusset-app-save-{}", rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.gus");
    a.save_as(path.clone());
    a.wait_idle();
    assert!(!a.is_dirty());
    assert!(a.dialog.is_none());
    let mut b = app();
    b.open(path).unwrap();
    let doc = b.host().with_session(|s| s.document(&gusset_history::Actor::User)).unwrap().unwrap();
    assert_eq!(doc.params.first().map(|p| p.name.as_str()), Some("w"));
}

#[test]
fn palette_search_ranks_titles() {
    let r = search("extrude");
    assert_eq!(r.first().map(|s| s.id), Some("feature.extrude"));
    assert!(search("zzzz-nothing").is_empty());
    let mut h = harness(app());
    h.state_mut().palette = Some("extrude".into());
    // The focused search field's blinking cursor keeps scheduling repaints, so step a
    // fixed number of frames instead of waiting for the UI to go idle.
    h.run_steps(3);
    h.get_by_label_contains("feature.extrude");
}

#[test]
fn light_theme_and_code_view_render() {
    let mut a = app();
    a.theme = crate::theme::ThemeKind::Light;
    a.run("param.set", json!({"name": "w", "expr": "10 mm"})).unwrap();
    a.wait_idle();
    a.toggle_code();
    let mut h = harness(a);
    h.run();
    h.get_by_label("Apply code");
    // Broken code is reported with its location instead of applied.
    h.state_mut().code = Some(("gusset 1\nparam = = \n".into(), None));
    h.get_by_label("Apply code").click();
    h.run();
    h.state_mut().wait_idle();
    h.run();
    let err = h.state().code.as_ref().and_then(|c| c.1.clone());
    assert!(err.is_some_and(|e| e.contains("line")));
}

#[test]
fn agent_command_through_the_host_updates_the_gui() {
    let mut h = harness(app());
    h.run();
    assert!(h.query_by_label_contains("agent_w").is_none());
    // An agent connection (another thread, like an MCP session) edits the user's branch
    // with an approval the human gave.
    let host = h.state().host().clone();
    std::thread::spawn(move || {
        let mut a = gusset_commands::CmdCtx::agent("bot-1", 1);
        host.call(a.clone(), "branch.switch", json!({"branch": "main"})).unwrap();
        a.approved = true;
        host.call(a, "param.set", json!({"name": "agent_w", "expr": "7 mm"})).unwrap();
    })
    .join()
    .unwrap();
    // No GUI call involved: the host's change broadcast alone refreshes the window.
    h.run();
    h.get_by_label_contains("agent_w");
    assert!(h.state().agent_changes >= 1);
    h.get_by_label_contains("Agent activity");
}

/// Steps the harness (real time) until `f` holds, for work done on other threads.
fn run_until(h: &mut Harness<'static, App>, what: &str, f: impl Fn(&Harness<'static, App>) -> bool) {
    for _ in 0..200 {
        h.step();
        if f(h) {
            h.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn approval_request(args: &str) -> ApprovalRequest {
    ApprovalRequest {
        agent: "claude-code-1".into(),
        tool: "branch_set_main".into(),
        command: "branch.set_main".into(),
        title: "Set as main".into(),
        reason: "Agents need human approval to change the main branch".into(),
        arguments: args.into(),
        context: "branch: b2 = \"agent/x\", created by agent x, not main".into(),
    }
}

#[test]
fn approval_modal_approve_and_deny() {
    let mut h = harness(app());
    h.run();
    let provider = h.state().agents.approvals.clone();
    // Approve: the agent call waits on its own thread while the UI keeps running.
    let p = provider.clone();
    let t = std::thread::spawn(move || p.request_approval(&approval_request(r#"{"branch": "agent/claude-code-1"}"#)));
    run_until(&mut h, "the approval modal", |h| h.query_by_label("Approve").is_some());
    h.get_by_label_contains("An AI agent asks for approval");
    h.get_by_label_contains("Set as main (branch.set_main)");
    h.get_by_label_contains(r#"{"branch": "agent/claude-code-1"}"#);
    h.get_by_label("Approve").click();
    h.run();
    assert_eq!(t.join().unwrap(), ApprovalDecision::Approved);
    assert!(h.query_by_label("Approve").is_none(), "modal closed");

    // Deny. Agent text with markup, a link and a bidi override is shown as plain text.
    let p = provider.clone();
    let t =
        std::thread::spawn(move || p.request_approval(&approval_request("[click](https://evil.example) \u{202E}x")));
    run_until(&mut h, "the approval modal", |h| h.query_by_label("Deny").is_some());
    h.get_by_label_contains("[click](https://evil.example) x");
    h.get_by_label("Deny").click();
    h.run();
    assert!(matches!(t.join().unwrap(), ApprovalDecision::Denied(_)));

    // Nobody answers: the request is denied after the timeout and leaves the queue.
    provider.set_timeout(std::time::Duration::from_millis(50));
    match provider.request_approval(&approval_request("{}")) {
        ApprovalDecision::Denied(why) => assert!(why.contains("did not answer"), "{why}"),
        ApprovalDecision::Approved => panic!("timed-out request approved"),
    }
    assert_eq!(provider.pending(), 0);
}

#[test]
fn agents_panel_shows_connection_audit_and_branches() {
    let mut a = app();
    let conn = std::env::temp_dir().join(format!("gusset-app-mcp-{}", rand_suffix()));
    a.start_agents(gusset_mcp::HttpOptions {
        connection_dir: Some(conn.clone()),
        ipv6: false,
        ..gusset_mcp::HttpOptions::default()
    });
    assert!(a.agents.info.is_some(), "{:?}", a.agents.error);
    a.agents.show_panel = true;
    let mut h = harness(a);
    h.run();
    h.get_by_label_contains("Agents can connect at http://127.0.0.1:");
    h.get_by_label_contains("Bearer <token>");
    h.get_by_label("No agent actions yet.");

    // An agent edits its own branch through the host (as an MCP connection would).
    let host = h.state().host().clone();
    host.call(gusset_commands::CmdCtx::agent("bot-2", 3_661), "param.set", json!({"name": "t", "expr": "3 mm"}))
        .unwrap();
    h.run();
    h.get_by_label_contains("param.set on agent/bot-2: ok");
    h.get_by_label_contains("01:01:01");
    h.get_by_label("agent/bot-2");

    // Compare, then apply the agent branch onto the user's branch.
    h.get_by_label("Compare").click();
    h.run();
    h.state_mut().wait_idle();
    h.run();
    h.get_by_label_contains("Parameter t: added");
    h.get_by_label("Apply").click();
    h.run();
    h.state_mut().wait_idle();
    h.run();
    assert_eq!(h.state().snapshot.doc.params.first().map(|p| p.name.as_str()), Some("t"));

    // Read-only agents: the next agent edit is refused.
    h.get_by_label("Read-only agents").click();
    h.run();
    h.state_mut().wait_idle();
    assert!(h.state().snapshot.read_only_agents);
    let e =
        host.call(gusset_commands::CmdCtx::agent("bot-2", 3_700), "param.set", json!({"name": "u", "expr": "1 mm"}));
    assert_eq!(e.unwrap_err().code, "read_only");

    // Kill switch: the token changes at once.
    let before = h.state().agents.server().and_then(gusset_mcp::McpServer::bearer_token);
    h.get_by_label("Revoke token").click();
    h.run();
    let after = h.state().agents.server().and_then(gusset_mcp::McpServer::bearer_token);
    assert!(before.is_some() && after.is_some() && before != after);
    h.get_by_label_contains("Token revoked");
    drop(h);
    let _ = std::fs::remove_dir_all(conn);
}

/// A part with one sketch used by an extrude and one free sketch.
fn two_sketches() -> App {
    let mut a = app();
    for (name, x) in [("Base", 0.0), ("Free", 60.0)] {
        a.run("sketch.create", json!({"plane": "XY", "name": name})).unwrap();
        a.run("sketch.add_rectangle", json!({"sketch": name, "x": x, "y": 0.0, "width": 40.0, "height": 20.0}))
            .unwrap();
    }
    a.run("feature.extrude", json!({"sketch": "Base", "distance": "10 mm"})).unwrap();
    a.wait_idle();
    a
}

#[test]
fn consumed_sketches_are_hidden_until_toggled_or_edited() {
    let a = two_sketches();
    assert_eq!(crate::app::consumed_sketches(&a.snapshot.doc).into_iter().collect::<Vec<_>>(), vec!["Base"]);
    assert!(!a.sketch_visible("Base"));
    assert!(a.sketch_visible("Free"));
    let mut h = harness(a);
    h.run();
    // The browser's eye toggles reflect and change the visibility.
    assert_eq!(h.get_by_label("Visibility of Base").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    h.get_by_label("Visibility of Base").click();
    h.run();
    assert!(h.state().sketch_visible("Base"));
    h.get_by_label("Visibility of Free").click();
    h.run();
    assert!(!h.state().sketch_visible("Free"));
    // The sketch being edited is always shown.
    h.state_mut().sketch_visibility.clear();
    h.state_mut().start_command("sketch.add_line");
    h.state_mut().form.as_mut().unwrap().set("sketch", "Base");
    assert!(h.state().sketch_visible("Base"));
    h.state_mut().form = None;
    assert!(!h.state().sketch_visible("Base"));
}

fn settle(h: &mut Harness<'static, App>) {
    // Host change events wake the UI from another thread, so step a fixed number of
    // frames rather than waiting for the UI to go idle.
    h.run_steps(4);
    h.state_mut().wait_idle();
    h.run_steps(4);
}

#[test]
fn timeline_marker_moves_with_keys_and_drag() {
    let mut h = harness(two_sketches());
    settle(&mut h);
    let n = h.state().snapshot.doc.features.len();
    assert_eq!(n, 3);
    assert_eq!(h.get_by_label("Rollback marker").accesskit_node().numeric_value(), Some(n as f64));
    // Keyboard: focus the marker, then Left / Home / End.
    h.get_by_label("Rollback marker").focus();
    h.run();
    h.key_press(egui::Key::ArrowLeft);
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(n - 1));
    let ev = h.state().evaluation().unwrap();
    assert_eq!(ev.status("Extrude1"), Some(&gusset_eval::FeatureStatus::RolledBack));
    h.key_press(egui::Key::Home);
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(0));
    h.key_press(egui::Key::End);
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, None);
    // Drag the marker to the far left: everything rolled back.
    let from = h.get_by_label("Rollback marker").rect().center();
    let to = egui::pos2(0.0, from.y);
    h.event(egui::Event::PointerMoved(from));
    h.run();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(button(from, true));
    h.run();
    for k in 1..=10 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 10.0)));
        h.run();
    }
    h.event(button(to, false));
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(0));
    assert!(h.state().notice.as_ref().is_none_or(|n| !n.error), "{:?}", h.state().notice);
}

#[test]
fn viewport_renders_only_when_something_changed() {
    let mut h = harness(two_sketches());
    h.run();
    assert!(!h.state().viewport.uses_gpu(), "the test harness has no wgpu device: CPU fallback");
    assert!(!h.state().viewport.scene().is_empty());
    let n = h.state().viewport.software_renders;
    assert!(n >= 1);
    // Idle frames reuse the last image.
    h.run_steps(5);
    assert_eq!(h.state().viewport.software_renders, n);
    // A camera change renders again.
    h.state_mut().viewport.camera.orbit(0.3, 0.0);
    h.run_steps(1);
    assert_eq!(h.state().viewport.software_renders, n + 1);
}
