//! UI automation (egui_kittest drives the real UI through the accessibility tree).

use crate::app::{App, Dialog, search};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use gusset_kernel::mock::MockKernel;
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
    Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app)
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
fn close_with_unsaved_changes_asks_first() {
    let mut a = app();
    a.run("param.set", json!({"name": "w", "expr": "10 mm"})).unwrap();
    let mut h = harness(a);
    h.run();
    h.event(egui::Event::WindowFocused(true));
    // Simulate the OS close button.
    h.state_mut().dialog = Some(Dialog::Close { save_failed: None });
    h.run();
    h.get_by_label("Save changes before closing?");
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().dialog.is_none());
    assert!(!h.state().allow_close);
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
    h.run();
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
