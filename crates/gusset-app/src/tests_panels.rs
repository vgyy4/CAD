//! The Fusion-style panels (DESIGN.md §2): the browser tree, the timeline with playback
//! and the command line, driven through the accessibility tree.

use crate::app::App;
use crate::browser::Category;
use crate::tests::{app, harness, settle, two_sketches};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

/// A harness whose frames are short, so two clicks (four events, one frame each) fall
/// inside egui's 0.3 s double-click delay.
fn quick_harness(app: App) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_max_steps(12)
        .with_step_dt(0.04)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app)
}

fn double_click_at(h: &mut Harness<'static, App>, pos: egui::Pos2) {
    h.hover_at(pos);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    h.run();
}

/// Two sketches (one extruded) and a datum plane.
fn with_plane() -> App {
    let mut a = two_sketches();
    a.run("feature.plane", json!({"base": "XY", "offset": "5 mm", "name": "Up"})).unwrap();
    a.wait_idle();
    a
}

// ------------------------------------------------------------------------- browser

#[test]
fn browser_has_the_fusion_categories_and_sorts_features_into_them() {
    let mut h = harness(with_plane());
    settle(&mut h);
    for c in Category::ALL {
        h.get_by_label(c.title());
    }
    // Sketches, features and datum planes are listed in their own categories.
    h.get_by_label("sketch Base");
    h.get_by_label("sketch Free");
    h.get_by_label("extrude Extrude1");
    h.get_by_label("plane Up");
    // The DOF line of a sketch.
    assert!(
        h.query_all_by_label_contains("DOF free").count() + h.query_all_by_label_contains("constrained").count() > 0
    );
    // Origin and Document settings start collapsed and open on a click.
    assert!(h.query_by_label("XY plane").is_none() && h.query_by_label_contains("Units").is_none());
    h.get_by_label("Origin").click();
    h.get_by_label("Document settings").click();
    h.run();
    for l in ["Origin point", "XY plane", "XZ plane", "YZ plane"] {
        assert!(h.query_by_label(l).is_some(), "{l}");
    }
    h.get_by_label("Units: mm");
    assert!(h.state().browser.is_open(Category::Origin));
    // Collapsing hides the rows again.
    h.get_by_label("Sketches").click();
    h.run();
    assert!(h.query_by_label("sketch Base").is_none());
    assert!(!h.state().browser.is_open(Category::Sketches));
}

#[test]
fn browser_keeps_sketch_double_click_and_the_eye() {
    let mut h = quick_harness(with_plane());
    settle(&mut h);
    assert_eq!(h.state().active_sketch(), None);
    let pos = h.get_by_label("sketch Free").rect().center();
    double_click_at(&mut h, pos);
    assert_eq!(h.state().active_sketch(), Some("Free"));
    // The eye toggles stay (the old tree's behaviour, also covered by the sketch test).
    h.get_by_label("Visibility of Base");
}

#[test]
fn browser_parameters_edit_and_add_through_the_forms() {
    let mut a = app();
    a.run("param.set", json!({"name": "width", "expr": "12 mm"})).unwrap();
    a.wait_idle();
    let mut h = quick_harness(a);
    settle(&mut h);
    h.get_by_label("+ Add parameter").click();
    h.run();
    assert_eq!(h.state().form.as_ref().map(|f| f.command), Some("param.set"));
    h.state_mut().form = None;
    // Let egui's click-count window (a double click is two clicks, not the third) expire.
    h.run_steps(20);
    let pos = h.get_by_label_contains("width = 12 mm").rect().center();
    double_click_at(&mut h, pos);
    let form = h.state().form.as_ref().unwrap();
    assert_eq!(form.command, "param.set");
    let value = |n: &str| form.fields.iter().find(|f| f.name == n).map(|f| f.value.clone());
    assert_eq!((value("name").as_deref(), value("expr").as_deref()), (Some("width"), Some("12 mm")));
}

#[test]
fn a_parameter_with_a_range_gets_a_customizer_slider() {
    let mut a = app();
    a.run("param.set", json!({"name": "width", "expr": "40 mm", "min": "10 mm", "max": "100 mm", "step": "5 mm"}))
        .unwrap();
    a.wait_idle();
    let mut h = quick_harness(a);
    settle(&mut h);
    let slider = || egui::accesskit::Role::Slider;
    h.get_by_role(slider()).focus();
    h.run();
    // One step to the right: 45 mm, set as an ordinary (undoable) parameter edit.
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.state_mut().wait_idle();
    h.run();
    let p = h.state().snapshot.doc.params.iter().find(|p| p.name == "width").map(|p| p.expr.to_string());
    assert_eq!(p.as_deref(), Some("45 mm"));
    // The range survives the edit.
    assert!(h.state().snapshot.doc.to_text().contains("param width = 45 mm, min: 10 mm, max: 100 mm, step: 5 mm"));
}

#[test]
fn a_configuration_row_applies_its_values() {
    let mut a = app();
    a.run("param.set", json!({"name": "width", "expr": "40 mm"})).unwrap();
    a.run("configuration.save", json!({"name": "Wide"})).unwrap();
    a.run("param.set", json!({"name": "width", "expr": "10 mm"})).unwrap();
    a.wait_idle();
    let mut h = quick_harness(a);
    settle(&mut h);
    h.get_by_label_contains("configuration Wide");
    h.get_by_label("Apply").click();
    h.run();
    h.state_mut().wait_idle();
    h.run();
    let p = h.state().snapshot.doc.params.iter().find(|p| p.name == "width").map(|p| p.expr.to_string());
    assert_eq!(p.as_deref(), Some("40 mm"));
}

#[test]
fn browser_branch_rows_switch_branches() {
    let mut a = app();
    a.run("param.set", json!({"name": "w", "expr": "1 mm"})).unwrap();
    // (creating a branch switches to it)
    a.run("branch.create", json!({"name": "side"})).unwrap();
    a.wait_idle();
    let mut h = harness(a);
    settle(&mut h);
    let snap = &h.state().snapshot;
    let before = snap.branch;
    assert_eq!(snap.branches.len(), 2, "{:?}", snap.branches);
    let other = snap.branches.iter().find(|b| Some(b.id) != before).map(|b| b.name.clone()).unwrap();
    h.get_by_label("Branches").click();
    h.run();
    h.get_by_label("side");
    h.get_by_label(&other).click();
    settle(&mut h);
    assert_ne!(h.state().snapshot.branch, before);
}

// ------------------------------------------------------------------------- timeline

#[test]
fn playback_buttons_move_the_rollback_marker() {
    let mut h = harness(two_sketches());
    settle(&mut h);
    let n = h.state().snapshot.doc.features.len();
    assert_eq!(n, 3);
    // At the end: forward and to-end are disabled, back and to-start work.
    assert!(h.get_by_label("Step forward one feature").accesskit_node().is_disabled());
    assert!(!h.get_by_label("Step back one feature").accesskit_node().is_disabled());
    h.get_by_label("Step back one feature").click();
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(2));
    // The chip of the rolled-back feature says so.
    h.get_by_label("Extrude1 (rolled back)");
    h.get_by_label("Roll back to the start").click();
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(0));
    assert!(h.get_by_label("Roll back to the start").accesskit_node().is_disabled());
    h.get_by_label("Step forward one feature").click();
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(1));
    h.get_by_label("Roll forward to the end").click();
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, None);
    h.get_by_label("Extrude1 (ok)");
    // The iteration graph toggle stays.
    h.get_by_label("Show iteration graph").click();
    h.run();
    h.get_by_label("Hide iteration graph");
}

// ------------------------------------------------------------------------- command line

fn type_line(h: &mut Harness<'static, App>, line: &str) {
    let input = || egui::accesskit::Role::TextInput;
    // Hidden by default; Ctrl+Shift+K shows it.
    if !h.state().cmdline.visible {
        h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::K);
        h.run();
        h.run();
    }
    h.get_by_role(input()).focus();
    h.run();
    h.get_by_role(input()).type_text(line);
    h.run();
}

#[test]
fn command_line_runs_registry_commands_with_key_value_args() {
    let mut h = harness(app());
    h.run();
    type_line(&mut h, "sketch.create plane=XY name=Plate");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(h.state().snapshot.doc.features.iter().any(|f| f.name == "Plate"), "{:?}", h.state().notice);
    // The result is in the status bar and the field is empty and remembered.
    assert!(h.state().notice.as_ref().is_some_and(|n| !n.error && n.text.starts_with("sketch.create: ok")));
    assert_eq!(h.state().cmdline.text, "");
    assert_eq!(h.state().cmdline.history, ["sketch.create plane=XY name=Plate"]);
    h.get_by_label_contains("sketch.create: ok");
}

#[test]
fn command_line_goes_through_the_same_path_as_the_ribbon() {
    // The same command typed as JSON and as key=value gives what the registry gives.
    let mut typed = app();
    typed.run_command_line(r#"param.set {"name": "w", "expr": "12 mm"}"#);
    typed.run_command_line("param.set name=v expr=\"3 mm\"");
    typed.wait_idle();
    let mut direct = app();
    direct.run("param.set", json!({"name": "w", "expr": "12 mm"})).unwrap();
    direct.run("param.set", json!({"name": "v", "expr": "3 mm"})).unwrap();
    direct.wait_idle();
    assert_eq!(typed.snapshot.doc.to_text(), direct.snapshot.doc.to_text());
    assert!(typed.notice.as_ref().is_some_and(|n| !n.error), "{:?}", typed.notice);
}

#[test]
fn command_line_reports_parse_and_registry_errors() {
    let mut h = harness(two_sketches());
    settle(&mut h);
    type_line(&mut h, "no.such_command x=1");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(h.state().notice.as_ref().is_some_and(|n| n.error && n.text.contains("unknown command")));
    h.get_by_label_contains("unknown command");
    // An argument the command does not have.
    h.state_mut().run_command_line("feature.extrude bogus=1");
    assert!(h.state().notice.as_ref().is_some_and(|n| n.error && n.text.contains("unknown argument 'bogus'")));
    // A well-formed line the registry rejects (no such sketch) is reported from the command.
    let before = h.state().snapshot.doc.features.len();
    h.state_mut().run_command_line("feature.extrude sketch=Nope distance=\"5 mm\"");
    h.state_mut().wait_idle();
    h.run();
    assert!(h.state().notice.as_ref().is_some_and(|n| n.error), "{:?}", h.state().notice);
    assert_eq!(h.state().snapshot.doc.features.len(), before);
}

#[test]
fn command_line_tab_completes_command_ids() {
    let mut h = harness(app());
    h.run();
    type_line(&mut h, "feature.extr");
    h.key_press(egui::Key::Tab);
    h.run();
    assert_eq!(h.state().cmdline.text, "feature.extrude ");
    // The hint lists the arguments of the command.
    h.get_by_label_contains("distance=");
}

#[test]
fn command_line_is_toggled_from_the_view_menu() {
    let input = egui::accesskit::Role::TextInput;
    let mut h = harness(app());
    h.run();
    // Hidden by default, shown and hidden again from the View menu.
    assert!(h.query_by_role(input).is_none());
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Command line").click();
    h.run();
    h.run();
    assert!(h.state().cmdline.visible);
    assert!(h.query_by_role(input).is_some());
    h.get_by_label("View").click();
    h.run();
    h.get_by_label("Command line").click();
    h.run();
    h.run();
    assert!(!h.state().cmdline.visible);
    assert!(h.query_by_role(input).is_none());
}

#[test]
fn two_picked_faces_fill_the_joint_form() {
    use crate::viewport::Hit;
    let mut a = app();
    let face = |body: &str, name: &str| Hit::Face { body: body.into(), index: 0, name: name.into() };
    a.selection = vec![face("Lid", "Lid/cap(start)"), face("Base", "Base/cap(end)")];
    a.start_command("feature.joint");
    let form = a.form.as_mut().unwrap();
    assert_eq!(form.value("at"), Some("Lid/cap(start)"));
    assert_eq!(form.value("to"), Some("Base/cap(end)"));
    assert_eq!(form.value("body"), Some("Lid"));
    // A later pick replaces the target.
    crate::app::pick_joint_frame(form, &face("Base", "Base/side(S1/l5)"));
    assert_eq!(form.value("to"), Some("Base/side(S1/l5)"));
    assert_eq!(form.value("at"), Some("Lid/cap(start)"));
}
