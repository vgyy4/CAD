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
fn a_derived_parameter_gets_no_slider() {
    // Review v0.3 L4: the slider would replace the relation with a number.
    let mut a = app();
    a.run("param.set", json!({"name": "height", "expr": "20 mm"})).unwrap();
    a.run("param.set", json!({"name": "width", "expr": "height * 2", "min": "10 mm", "max": "100 mm"})).unwrap();
    a.wait_idle();
    let mut h = quick_harness(a);
    settle(&mut h);
    assert!(h.query_by_role(egui::accesskit::Role::Slider).is_none());
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

#[test]
fn browser_body_eye_hides_the_body_and_keeps_it_hidden_across_rebuilds() {
    let mut h = quick_harness(with_plane());
    settle(&mut h);
    assert!(h.state().viewport.body_visible("Extrude1"));
    h.get_by_label("Visibility of Extrude1").click();
    h.run();
    assert!(!h.state().viewport.body_visible("Extrude1"));
    let id = h.state().viewport.bodies().iter().position(|b| b.name == "Extrude1").expect("body");
    let mesh = |h: &Harness<'static, App>| {
        h.state().viewport.scene().mesh(gusset_render::MeshId(id as u64)).map(|m| m.visible())
    };
    assert_eq!(mesh(&h), Some(false));
    // A rebuild of the model (a new datum plane) keeps the body hidden.
    h.state_mut().run("feature.plane", json!({"base": "XY", "offset": "9 mm", "name": "Up2"})).unwrap();
    h.state_mut().wait_idle();
    settle(&mut h);
    assert_eq!(mesh(&h), Some(false));
    h.get_by_label("Visibility of Extrude1").click();
    h.run();
    assert_eq!(mesh(&h), Some(true));
}

#[test]
fn plane_eyes_show_origin_and_datum_planes_and_a_picked_plane_fills_the_form() {
    let mut h = quick_harness(with_plane());
    settle(&mut h);
    // Origin planes start hidden, datum planes shown; the app hands all to the viewport.
    let names: Vec<String> = h.state().viewport.planes.iter().map(|p| p.name.clone()).collect();
    assert_eq!(names, ["XY", "XZ", "YZ", "Up"]);
    assert!(!h.state().viewport.plane_visible("XY") && h.state().viewport.plane_visible("Up"));
    h.get_by_label("Origin").click();
    h.run();
    h.get_by_label("Visibility of plane XY").click();
    h.run();
    assert!(h.state().viewport.plane_visible("XY"));
    h.get_by_label("Visibility of plane Up").click();
    h.run();
    assert!(!h.state().viewport.plane_visible("Up"));
    // A picked plane is what a new sketch is placed on.
    h.state_mut().selection = vec![crate::viewport::Hit::Plane { name: "Up".into() }];
    h.state_mut().start_command("sketch.create");
    h.run();
    assert_eq!(h.state().form.as_ref().and_then(|f| f.value("plane")), Some("Up"));
}

#[test]
fn a_command_form_opens_in_the_properties_panel_and_closes() {
    let mut h = quick_harness(with_plane());
    settle(&mut h);
    h.state_mut().start_command("feature.plane");
    h.run();
    assert!(h.state().form.is_some());
    // The panel shows the command's title and its fields, with a Close button.
    let title = h.state().form.as_ref().map(|f| f.title).unwrap_or_default();
    assert!(h.query_all_by_label(title).count() >= 1);
    h.get_by_label("Close").click();
    h.run();
    assert!(h.state().form.is_none());
}

fn click_at(h: &mut Harness<'static, App>, pos: egui::Pos2) {
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
}

/// The first viewport pixel (on a coarse grid) whose pick matches `want`, in screen
/// coordinates.
fn find_pick(h: &Harness<'static, App>, want: fn(&crate::viewport::Hit) -> bool) -> Option<egui::Pos2> {
    let v = &h.state().viewport;
    let r = v.rect();
    (0..(r.height() as i32 / 3))
        .flat_map(|y| (0..(r.width() as i32 / 3)).map(move |x| egui::pos2(x as f32 * 3.0, y as f32 * 3.0)))
        .find(|p| v.pick(r.size(), *p).is_some_and(|(hit, _)| want(&hit)))
        .map(|p| r.min + p.to_vec2())
}

#[test]
fn picks_made_with_a_form_open_fill_its_fields() {
    use crate::viewport::Hit;
    let mut h = quick_harness(with_plane());
    settle(&mut h);
    h.state_mut().start_command("feature.fillet");
    h.run();
    let at = find_pick(&h, |hit| matches!(hit, Hit::Edge { .. })).expect("an edge on screen");
    click_at(&mut h, at);
    let Some(Hit::Edge { name, .. }) = h.state().selection.last().cloned() else { panic!("no edge picked") };
    assert_eq!(h.state().form.as_ref().and_then(|f| f.value("edges")), Some(name.as_str()));
    // A face pick fills `faces` (offset face) without touching other fields.
    h.state_mut().selection.clear();
    h.state_mut().start_command("feature.offset_face");
    h.run();
    let at = find_pick(&h, |hit| matches!(hit, Hit::Face { .. })).expect("a face on screen");
    click_at(&mut h, at);
    let Some(Hit::Face { name, .. }) = h.state().selection.last().cloned() else { panic!("no face picked") };
    assert_eq!(h.state().form.as_ref().and_then(|f| f.value("faces")), Some(name.as_str()));
}

#[test]
fn double_clicking_a_feature_edits_it_in_place() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let statement = |h: &Harness<'static, App>| {
        h.state().snapshot.doc.feature("Extrude1").map(gusset_doc::text::print_feature_object).unwrap_or_default()
    };
    assert!(statement(&h).contains("distance: 10 mm"), "{}", statement(&h));
    let at = h.get_by_label("extrude Extrude1").rect().center();
    double_click_at(&mut h, at);
    let form = h.state().form.as_ref().expect("edit form");
    assert_eq!(
        (form.command, form.value("name"), form.value("distance")),
        ("feature.edit", Some("Extrude1"), Some("10 mm"))
    );
    h.state_mut().form.as_mut().map(|f| f.set("distance", "25 mm"));
    h.get_by_label("OK").click();
    settle(&mut h);
    assert!(h.state().form.is_none(), "{:?}", h.state().form.as_ref().and_then(|f| f.error.clone()));
    assert!(statement(&h).contains("distance: 25 mm"), "{}", statement(&h));
    assert!(h.state().evaluation().is_some_and(|e| e.errors().is_empty()));
    assert_eq!(h.state().snapshot.doc.feature_index("Extrude1"), Some(2));
    // The timeline chip opens the same editor; a sketch's chip edits the sketch. (Time
    // passes first, or egui counts the clicks as a triple click with the earlier ones.)
    h.run_steps(20);
    let at = h.get_by_label("Extrude1 (ok)").rect().center();
    double_click_at(&mut h, at);
    assert_eq!(h.state().form.as_ref().map(|f| (f.command, f.value("name"))), Some(("feature.edit", Some("Extrude1"))));
    h.state_mut().form = None;
    h.run_steps(20);
    let at = h.get_by_label("Base (ok)").rect().center();
    double_click_at(&mut h, at);
    assert_eq!(h.state().active_sketch(), Some("Base"));
}

#[test]
fn feature_forms_show_a_live_preview_until_they_close() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let height = |h: &Harness<'static, App>| {
        let b = h.state().viewport.scene().bounds();
        b.max[2] - b.min[2]
    };
    assert!((height(&h) - 10.0).abs() < 1e-6);
    h.state_mut().edit_feature("Extrude1");
    h.state_mut().form.as_mut().map(|f| f.set("distance", "30 mm"));
    // The preview waits for typing to pause, then builds on the model thread.
    for _ in 0..4 {
        h.run_steps(5);
        h.state_mut().wait_idle();
    }
    h.run_steps(3);
    assert!(h.state().viewport.previewing());
    assert!((height(&h) - 30.0).abs() < 1e-6, "{}", height(&h));
    // The model as it was shows over the preview as edges (the 10 mm box: 12 edges).
    assert!(h.state().viewport.before_edges() >= 12, "{}", h.state().viewport.before_edges());
    // Nothing was committed; closing the form puts the model back.
    assert!(
        h.state()
            .snapshot
            .doc
            .feature("Extrude1")
            .is_some_and(|f| { gusset_doc::text::print_feature_object(f).contains("distance: 10 mm") })
    );
    h.state_mut().form = None;
    h.run_steps(3);
    assert!(!h.state().viewport.previewing());
    assert_eq!(h.state().viewport.before_edges(), 0);
    assert!((height(&h) - 10.0).abs() < 1e-6, "{}", height(&h));
}

fn right_click_at(h: &mut Harness<'static, App>, pos: egui::Pos2) {
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run();
}

#[test]
fn feature_context_menus_suppress_and_roll_back() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let suppressed =
        |h: &Harness<'static, App>| h.state().snapshot.doc.feature("Extrude1").is_some_and(|f| f.suppressed);
    assert!(!suppressed(&h));
    let at = h.get_by_label("extrude Extrude1").rect().center();
    right_click_at(&mut h, at);
    h.get_by_label("Suppress").click();
    settle(&mut h);
    assert!(suppressed(&h));
    // The timeline chip's menu: bring it back, then roll back to the first sketch.
    h.run_steps(20);
    let at = h.get_by_label_contains("Extrude1 (").rect().center();
    right_click_at(&mut h, at);
    h.get_by_label("Unsuppress").click();
    settle(&mut h);
    assert!(!suppressed(&h));
    let at = h.get_by_label("Base (ok)").rect().center();
    right_click_at(&mut h, at);
    h.get_by_label("Roll back to here").click();
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(1));
    // Outside sketch mode the Delete key is feature.delete's shortcut again.
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(h.state().form.as_ref().map(|f| f.command), Some("feature.delete"));
}

#[test]
fn f11_gives_a_clean_viewport_and_ctrl_enter_repeats_a_command() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    h.get_by_label("Origin");
    h.key_press(egui::Key::F11);
    h.run();
    assert!(h.state().full_screen);
    assert!(h.query_by_label("Origin").is_none(), "the browser is hidden");
    h.key_press(egui::Key::F11);
    h.run();
    h.get_by_label("Origin");
    // A form confirmed with Ctrl+Enter opens again after it succeeds, with its values.
    h.state_mut().start_command("param.set");
    let form = h.state_mut().form.as_mut().expect("form");
    form.set("name", "gap");
    form.set("expr", "2 mm");
    form.repeat = true;
    h.state_mut().apply_form();
    settle(&mut h);
    assert!(h.state().snapshot.doc.params.iter().any(|p| p.name == "gap"));
    let again = h.state().form.as_ref().expect("the command again");
    assert_eq!((again.command, again.value("name"), again.repeat), ("param.set", Some("gap"), false));
}

#[test]
fn marking_menu_and_quick_bar_start_context_tools() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let c = h.state().viewport.rect().center();
    right_click_at(&mut h, c);
    assert!(h.state().marking.is_some(), "a right click opens the marking menu");
    assert!(h.state().form.is_none(), "and runs nothing by itself");
    // (The ribbon has an Extrude button too: the menu's is drawn last.)
    h.get_all_by_label("Extrude").last().expect("menu item").click();
    h.run();
    assert_eq!(h.state().form.as_ref().map(|f| f.command), Some("feature.extrude"));
    assert!(h.state().marking.is_none());
    // The S quick bar leads with the last command; Esc closes it.
    h.state_mut().form = None;
    h.hover_at(c);
    h.key_press(egui::Key::S);
    h.run();
    assert!(h.state().quick.is_some());
    assert!(h.get_all_by_label("Extrude").count() >= 2, "the quick bar lists the last command");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().quick.is_none());
}

#[test]
fn saved_preferences_come_back_in_a_new_window() {
    let dir = std::env::temp_dir().join(format!("gusset-app-prefs-{}", std::process::id()));
    let dirs = gusset_store::paths::DataDirs::at(dir.join("data"), dir.join("cache"));
    std::fs::create_dir_all(dirs.data_dir()).unwrap();
    let want = crate::prefs::Prefs {
        theme: crate::theme::ThemeKind::Light,
        reduce_motion: true,
        nav: crate::viewport::NavPreset::Fusion,
        recent: vec!["/tmp/a.gus".into()],
        browser_width: Some(310.0),
        properties_width: None,
        ui_scale: 1.25,
        quick_solid: vec!["feature.shell".into()],
        quick_sketch: Vec::new(),
        browser_collapsed: true,
        timeline_collapsed: false,
        docks_swapped: true,
    };
    crate::prefs::save(&crate::prefs::path(dirs.data_dir()), &want).unwrap();
    let a = App::new(std::sync::Arc::new(gusset_kernel::mock::MockKernel), Some(dirs));
    assert_eq!((a.theme, a.reduce_motion, a.viewport.nav_preset), (want.theme, want.reduce_motion, want.nav));
    assert_eq!(a.panel_widths, (Some(310.0), None));
    assert!((a.ui_scale - 1.25).abs() < 1e-6);
    assert_eq!(a.quick_tools, [vec!["feature.shell"], vec![]]);
    assert!(a.browser_collapsed && !a.timeline_collapsed && a.docks_swapped);
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn alt_click_lists_everything_under_the_pointer() {
    use crate::viewport::Hit;
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let at = find_pick(&h, |hit| matches!(hit, Hit::Face { .. })).expect("a face on screen");
    h.hover_at(at);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::ALT,
        });
    }
    h.run();
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
    let (_, hits) = h.state().picker.clone().expect("the list opens");
    let faces = hits.iter().filter(|x| matches!(x, Hit::Face { .. })).count();
    assert!(faces >= 2, "the near face and the one behind it: {hits:?}");
    // Picking the second entry selects it.
    let second = hits[1].clone();
    h.get_by_label(&second.label()).click();
    h.run();
    assert_eq!(h.state().selection, vec![second]);
    assert!(h.state().picker.is_none());
}

fn drag(h: &mut Harness<'static, App>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for k in 1..=8 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0)));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

#[test]
fn timeline_chips_reorder_by_drag_within_dependencies() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let order = |h: &Harness<'static, App>| -> Vec<String> {
        h.state().snapshot.doc.features.iter().map(|f| f.name.clone()).collect()
    };
    assert_eq!(order(&h), ["Base", "Free", "Extrude1"]);
    // The unused sketch moves to the end.
    let from = h.get_by_label("Free (ok)").rect().center();
    let to = h.get_by_label("Extrude1 (ok)").rect().right_center() + egui::vec2(6.0, 0.0);
    drag(&mut h, from, to);
    settle(&mut h);
    assert_eq!(order(&h), ["Base", "Extrude1", "Free"]);
    // The extrude cannot come before the sketch it uses: refused, order kept.
    let from = h.get_by_label("Extrude1 (ok)").rect().center();
    let to = h.get_by_label("Base (ok)").rect().left_center() - egui::vec2(6.0, 0.0);
    drag(&mut h, from, to);
    settle(&mut h);
    assert_eq!(order(&h), ["Base", "Extrude1", "Free"]);
    assert!(h.state().notice.as_ref().is_some_and(|n| n.error), "{:?}", h.state().notice);
}

#[test]
fn editing_an_earlier_sketch_rolls_back_and_ghosts_the_rest() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, None);
    h.state_mut().edit_sketch("Base");
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(1), "rolled back to just after the sketch");
    assert!(!h.state().viewport.ghost.is_empty(), "the extrude is ghosted");
    assert!(h.state().evaluation().is_some_and(|e| e.bodies.is_empty()));
    h.get_by_label("Finish sketch").click();
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, None, "the marker is back");
    assert!(h.state().viewport.ghost.is_empty());
    // Free also comes before Extrude1, so editing it rolls back to just after it too.
    h.state_mut().edit_sketch("Free");
    settle(&mut h);
    assert_eq!(h.state().snapshot.rollback, Some(2));
}

#[test]
fn command_search_finds_features_parameters_and_settings() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    h.state_mut().palette = Some("extrude1".into());
    h.run();
    h.get_by_label_contains("Edit Extrude1").click();
    h.run();
    assert_eq!(h.state().form.as_ref().map(|f| (f.command, f.value("name"))), Some(("feature.edit", Some("Extrude1"))));
    h.state_mut().form = None;
    h.state_mut().palette = Some("light theme".into());
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(h.state().theme, crate::theme::ThemeKind::Light);
    assert!(h.state().palette.is_none());
}

#[test]
fn the_parameters_table_applies_changes_in_one_step() {
    let mut a = crate::tests::app();
    a.run("param.set", json!({"name": "width", "expr": "40 mm"})).unwrap();
    a.run("param.set", json!({"name": "depth", "expr": "width / 4"})).unwrap();
    a.wait_idle();
    let mut h = quick_harness(a);
    settle(&mut h);
    h.state_mut().open_param_table();
    h.run();
    let rows = h.state_mut().param_table.as_mut().expect("open");
    assert_eq!(rows.len(), 3, "two parameters and the new-parameter row");
    rows[0].1 = "60 mm".into();
    rows[2].0 = "gap".into();
    rows[2].1 = "2 mm".into();
    h.get_by_label("Apply").click();
    settle(&mut h);
    assert!(h.state().param_table.is_none());
    let expr = |h: &Harness<'static, App>, n: &str| {
        h.state().snapshot.doc.params.iter().find(|p| p.name == n).map(|p| p.expr.to_string())
    };
    assert_eq!(expr(&h, "width"), Some("60 mm".into()));
    assert_eq!(expr(&h, "gap"), Some("2 mm".into()));
    assert_eq!(expr(&h, "depth"), Some("width / 4".into()), "unchanged rows are left alone");
}

#[test]
fn holding_x_sees_through_the_model() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    h.key_down(egui::Key::X);
    h.run();
    assert!(h.state().viewport.see_through);
    h.key_up(egui::Key::X);
    h.run();
    assert!(!h.state().viewport.see_through);
}

#[test]
fn the_ui_scale_is_applied_and_keyboard_zoom_becomes_the_scale() {
    let mut h = quick_harness(two_sketches());
    h.state_mut().ui_scale = 1.5;
    h.run();
    assert!((h.ctx.zoom_factor() - 1.5).abs() < 1e-6);
    // A zoom from elsewhere (Ctrl + plus) is taken over, within 75..=250 %.
    h.ctx.set_zoom_factor(4.0);
    h.run();
    assert!((h.state().ui_scale - 2.5).abs() < 1e-6, "{}", h.state().ui_scale);
    assert!((h.ctx.zoom_factor() - 2.5).abs() < 1e-6);
}

#[test]
fn a_right_drag_flick_runs_the_marking_menu_item_in_its_direction() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    h.state_mut().viewport.nav_preset = crate::viewport::NavPreset::Fusion;
    let c = h.state().viewport.rect().center();
    h.hover_at(c);
    let press = |h: &mut Harness<'static, App>, pos: egui::Pos2, pressed: bool| {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    };
    press(&mut h, c, true);
    // Straight up: the first item (Sketch).
    for k in 1..=6 {
        h.hover_at(c - egui::vec2(0.0, 12.0 * k as f32));
    }
    press(&mut h, c - egui::vec2(0.0, 72.0), false);
    h.run();
    assert!(h.state().marking.is_none(), "a flick opens no menu");
    assert_eq!(h.state().last_command, Some("sketch.create"));
    assert_eq!(h.state().form.as_ref().map(|f| f.command), Some("sketch.create"));
}

#[test]
fn ribbon_tools_join_and_leave_the_quick_bar() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    // Right click the Revolve tool: it is in the quick bar already, so it can be removed.
    let at = h.get_all_by_label("Revolve").next().expect("the Revolve tool").rect().center();
    right_click_at(&mut h, at);
    h.get_by_label("Remove from quick bar (S)").click();
    h.run();
    assert!(!h.state().quick_tools[0].contains(&"feature.revolve"), "{:?}", h.state().quick_tools);
    // Adding keeps at most seven, dropping the oldest.
    h.state_mut().set_quick_tool("feature.draft", true);
    h.state_mut().set_quick_tool("feature.mirror", true);
    let solid = h.state().quick_tools[0].clone();
    assert_eq!(solid.last(), Some(&"feature.mirror"));
    assert_eq!(solid.len(), crate::quick_menus::MAX_TOOLS);
}

#[test]
fn timeline_groups_frame_and_fold_their_features() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let names: Vec<String> = h.state().snapshot.doc.features.iter().take(2).map(|f| f.name.clone()).collect();
    h.state_mut().submit_command(
        "feature.group",
        json!({ "features": names, "group": "Base" }),
        crate::app::Then::Notice,
    );
    settle(&mut h);
    assert!(h.state().snapshot.doc.features.iter().take(2).all(|f| f.group.as_deref() == Some("Base")));
    // The group's name tag folds the run and unfolds it again.
    h.get_by_label("Group Base").click();
    h.run();
    let first = h.get_by_label_contains(&format!("{} (", names[0])).rect().width();
    assert!(first < 10.0, "folded chips are slivers: {first}");
    h.get_by_label("Group Base").click();
    h.run();
    assert!(h.get_by_label_contains(&format!("{} (", names[0])).rect().width() > 20.0);
    // Ungroup from the chip's context menu.
    h.state_mut().feature_choice(&names[0], crate::browser::FeatureChoice::Ungroup);
    settle(&mut h);
    assert!(h.state().snapshot.doc.features.first().is_some_and(|f| f.group.is_none()));
}

#[test]
fn panels_collapse_and_swap_sides() {
    let mut h = quick_harness(two_sketches());
    settle(&mut h);
    let browser_x = |h: &Harness<'static, App>| h.get_by_label_contains("Bodies").rect().center().x;
    let left = browser_x(&h);
    // Docked at the right: the browser moves past the viewport's centre.
    h.state_mut().docks_swapped = true;
    h.run();
    let vp = h.state().viewport.rect().center().x;
    assert!(left < vp && browser_x(&h) > vp, "{left} {} {vp}", browser_x(&h));
    // Collapsed: only the rail's button is left; it brings the browser back.
    h.state_mut().browser_collapsed = true;
    h.run();
    assert!(h.query_by_label_contains("Bodies").is_none());
    h.get_by_label("‹").click();
    h.run();
    assert!(!h.state().browser_collapsed);
    // The timeline folds to a one-line rail.
    h.state_mut().timeline_collapsed = true;
    h.run();
    h.get_by_label("+ Timeline").click();
    h.run();
    assert!(!h.state().timeline_collapsed);
}

#[test]
fn the_view_is_rendered_to_a_png_off_the_ui_thread() {
    let mut h = harness(two_sketches());
    settle(&mut h);
    let dir = std::env::temp_dir().join(format!("gusset-app-render-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("view.png");
    h.state_mut().render_quality = ([40, 30], 1);
    h.state_mut().export(crate::app::PathPurpose::Render, &path, false);
    for _ in 0..200 {
        h.run_steps(1);
        if h.state().notice.as_ref().is_some_and(|n| n.text.contains("written")) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let n = h.state().notice.clone().unwrap();
    assert!(n.text.contains("view.png") && !n.error, "{n:?}");
    assert!(std::fs::read(&path).unwrap().starts_with(b"\x89PNG"));
    let _ = std::fs::remove_dir_all(dir);
}
