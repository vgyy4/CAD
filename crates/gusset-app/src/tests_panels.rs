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
