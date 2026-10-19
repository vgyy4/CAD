//! The Fusion-style shell (DESIGN.md §2): workspace tabs, the ribbon, the view cube and
//! the navigation bar, driven through the accessibility tree and pointer events.

use crate::app::App;
use crate::tests::{app, harness};
use crate::viewport_nav::{self, NavMode};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use gusset_render::{NamedView, Projection};
use serde_json::json;

fn title(id: &str) -> &'static str {
    gusset_commands::find(id).map_or("?", |s| s.title)
}

fn form_command(h: &Harness<'static, App>) -> Option<&'static str> {
    h.state().form.as_ref().map(|f| f.command)
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

fn same(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9)
}

fn looks_like(h: &Harness<'static, App>, v: NamedView) -> bool {
    let (f, u) = v.orientation();
    let c = &h.state().viewport.camera;
    same(c.forward, f) && same(c.up, u)
}

/// A model with one body, so the viewport has something to frame and measure.
fn with_body() -> App {
    let mut a = app();
    a.run("sketch.create", json!({"plane": "XY"})).unwrap();
    a.run("sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0.0, "y": 0.0, "width": 40.0, "height": 20.0}))
        .unwrap();
    a.run("feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"})).unwrap();
    a.wait_idle();
    a
}

#[test]
fn ribbon_large_tools_and_dropdowns_open_their_commands() {
    let mut h = harness(app());
    h.run();
    // A large icon tool.
    h.get_by_label(title("feature.extrude")).click();
    h.run();
    assert_eq!(form_command(&h), Some("feature.extrude"));
    h.state_mut().form = None;
    h.run();
    // A command only the dropdown lists: CREATE ▾ → Sweep.
    assert!(h.query_by_label(title("feature.sweep")).is_none(), "Sweep is not a large tool");
    h.get_by_label("CREATE").click();
    h.run();
    h.get_by_label(title("feature.sweep")).click();
    h.run();
    assert_eq!(form_command(&h), Some("feature.sweep"));
    h.state_mut().form = None;
    h.run();
    // MODIFY ▾ lists the less used modify tools.
    h.get_by_label("MODIFY").click();
    h.run();
    h.get_by_label(title("feature.mirror")).click();
    h.run();
    assert_eq!(form_command(&h), Some("feature.mirror"));
    h.state_mut().form = None;
    h.run();
    // ASSEMBLE has the joint as its large tool.
    h.get_by_label("ASSEMBLE");
    h.get_by_label(title("feature.joint")).click();
    h.run();
    assert_eq!(form_command(&h), Some("feature.joint"));
}

#[test]
fn every_ribbon_command_starts_through_the_registry() {
    let mut h = harness(app());
    h.run();
    for sketch in [false, true] {
        for id in crate::ribbon::command_ids(sketch) {
            h.state_mut().dialog = None;
            h.state_mut().start_command(id);
            let opened = form_command(&h) == Some(id);
            // A command that takes no input runs at once instead.
            assert!(opened || h.state().busy() || h.state().dialog.is_some(), "{id}: nothing happened");
            h.state_mut().wait_idle();
            h.state_mut().form = None;
        }
    }
}

#[test]
fn measure_body_shows_its_result() {
    let mut h = harness(with_body());
    h.run();
    h.get_by_label(title("query.body")).click();
    h.run();
    assert_eq!(form_command(&h), Some("query.body"));
    h.state_mut().apply_form();
    h.state_mut().wait_idle();
    h.run();
    assert!(h.state().form.is_none());
    h.get_by_label_contains("volume_mm3");
}

#[test]
fn sketch_tab_takes_over_the_ribbon_until_finish_sketch() {
    let mut h = harness(app());
    h.run();
    assert!(h.query_by_label("Finish sketch").is_none());
    // The SKETCH tab with no sketch yet starts one.
    h.get_by_label("SKETCH").click();
    h.run();
    assert_eq!(form_command(&h), Some("sketch.create"));
    h.state_mut().apply_form();
    h.state_mut().wait_idle();
    h.run();
    assert_eq!(h.state().active_sketch(), Some("Sketch1"));
    // The sketch tools replace the solid tools, with a prominent "Finish sketch".
    h.get_by_label("Finish sketch");
    assert!(h.query_by_label(title("feature.extrude")).is_none());
    h.get_by_label(title("sketch.add_rectangle")).click();
    h.run();
    let form = h.state().form.as_ref().unwrap();
    assert_eq!(form.command, "sketch.add_rectangle");
    assert_eq!(form.fields.iter().find(|f| f.name == "sketch").map(|f| f.value.as_str()), Some("Sketch1"));
    h.state_mut().form = None;
    h.run();
    h.get_by_label("Finish sketch").click();
    h.run();
    assert_eq!(h.state().active_sketch(), None);
    h.get_by_label(title("feature.extrude"));
    assert!(h.query_by_label("Finish sketch").is_none());
    // A sketch from the browser (a double-click there) is edited again; the SOLID tab ends that.
    h.state_mut().edit_sketch("Sketch1");
    h.run();
    h.get_by_label("Finish sketch");
    h.get_by_label("SOLID").click();
    h.run();
    assert_eq!(h.state().active_sketch(), None);
}

#[test]
fn undoing_the_sketch_leaves_sketch_editing() {
    let mut h = harness(app());
    h.run();
    h.state_mut().start_command("sketch.create");
    h.state_mut().apply_form();
    h.state_mut().wait_idle();
    h.run();
    assert_eq!(h.state().active_sketch(), Some("Sketch1"));
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    h.run();
    h.run();
    assert_eq!(h.state().active_sketch(), None);
}

#[test]
fn view_cube_faces_edges_and_corners_set_the_view() {
    for animate in [false, true] {
        let mut a = with_body();
        a.reduce_motion = !animate;
        let mut h = harness(a);
        h.run();
        let cube = h.get_by_label("View cube").rect();
        // The default camera is isometric: Top, Front and Right face the viewer.
        for (normal, view) in [
            ([0.0, 0.0, 1.0], NamedView::Top),
            ([0.0, -1.0, 0.0], NamedView::Front),
            ([1.0, 0.0, 0.0], NamedView::Right),
        ] {
            h.state_mut().viewport.camera.set_view(NamedView::Iso);
            h.run();
            let at = viewport_nav::face_centre(&h.state().viewport.camera, cube.center(), normal);
            click_at(&mut h, at);
            h.run();
            assert!(looks_like(&h, view), "animate={animate}: clicking the {view:?} face");
        }
        // From behind: Back and Left.
        for (normal, view) in [([0.0, 1.0, 0.0], NamedView::Back), ([-1.0, 0.0, 0.0], NamedView::Left)] {
            h.state_mut().viewport.camera.set_view(NamedView::IsoBack);
            h.run();
            let at = viewport_nav::face_centre(&h.state().viewport.camera, cube.center(), normal);
            click_at(&mut h, at);
            h.run();
            assert!(looks_like(&h, view), "animate={animate}: clicking the {view:?} face");
        }
    }
}

#[test]
fn view_cube_edge_looks_from_the_edge() {
    let mut a = with_body();
    a.reduce_motion = true;
    let mut h = harness(a);
    h.run();
    let cube = h.get_by_label("View cube").rect();
    // The Top/Front edge strip: halfway between the Top and Front face centers, nearer the edge.
    let cam = h.state().viewport.camera;
    let top = viewport_nav::face_centre(&cam, cube.center(), [0.0, 0.0, 1.0]);
    let front = viewport_nav::face_centre(&cam, cube.center(), [0.0, -1.0, 0.0]);
    click_at(&mut h, top + (front - top) * 0.5);
    h.run();
    let f = h.state().viewport.camera.forward;
    // Looking from the top-front edge: down and towards +y, no x component.
    assert!(f[0].abs() < 1e-9 && f[1] > 0.5 && f[2] < -0.5, "{f:?}");
}

#[test]
fn navigation_bar_buttons_work() {
    let mut a = with_body();
    a.reduce_motion = true;
    let mut h = harness(a);
    h.run();
    // Tool modes toggle, and a plain drag then turns the view.
    h.get_by_label("Orbit").click();
    h.run();
    assert_eq!(h.state().viewport.nav_mode, NavMode::Orbit);
    let before = h.state().viewport.camera;
    let from = egui::pos2(500.0, 450.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(from));
    h.run();
    h.event(button(from, true));
    h.run();
    for k in 1..=5 {
        h.event(egui::Event::PointerMoved(from + egui::vec2(20.0 * k as f32, 0.0)));
        // A moving view schedules a refinement repaint: step instead of waiting for idle.
        h.step();
    }
    h.event(button(from + egui::vec2(100.0, 0.0), false));
    h.run();
    assert!(!same(h.state().viewport.camera.forward, before.forward), "dragging in Orbit mode turns the view");
    h.get_by_label("Orbit").click();
    h.run();
    assert_eq!(h.state().viewport.nav_mode, NavMode::Select);
    h.get_by_label("Pan").click();
    h.run();
    assert_eq!(h.state().viewport.nav_mode, NavMode::Pan);
    h.get_by_label("Zoom").click();
    h.run();
    assert_eq!(h.state().viewport.nav_mode, NavMode::Zoom);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().viewport.nav_mode, NavMode::Select);

    // Fit all re-frames after a zoom; Home returns to the isometric view, framed.
    let framed = h.state().viewport.camera.distance;
    h.state_mut().viewport.camera.distance *= 3.0;
    h.get_by_label("Fit all").click();
    h.run();
    assert!((h.state().viewport.camera.distance - framed).abs() < 1e-6 * framed);
    h.state_mut().viewport.camera.set_view(NamedView::Top);
    h.get_by_label("Home view").click();
    h.run();
    assert!(looks_like(&h, NamedView::Iso));
    // Projection and edges toggle.
    h.get_by_label("Orthographic view").click();
    h.run();
    assert_eq!(h.state().viewport.camera.projection, Projection::Orthographic);
    h.get_by_label("Orthographic view").click();
    h.run();
    assert_eq!(h.state().viewport.camera.projection, Projection::Perspective);
    assert!(h.state().viewport.show_edges);
    h.get_by_label("Show edges").click();
    h.run();
    assert!(!h.state().viewport.show_edges);
}

#[test]
fn camera_blend_ends_at_the_target_view() {
    // The animation helper turns along the shortest arc and ends exactly at the target,
    // also for opposite views (Front -> Back).
    let mut a = gusset_render::Camera::default();
    let mut b = a;
    a.set_view(NamedView::Front);
    b.set_view(NamedView::Back);
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let c = crate::viewport::blend(&a, &b, t);
        let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len(c.forward) - 1.0).abs() < 1e-9 && (len(c.up) - 1.0).abs() < 1e-9, "t={t}");
        let dot = c.forward[0] * c.up[0] + c.forward[1] * c.up[1] + c.forward[2] * c.up[2];
        assert!(dot.abs() < 1e-9, "forward and up stay orthogonal at t={t}");
    }
    let end = crate::viewport::blend(&a, &b, 1.0);
    assert!(same(end.forward, b.forward) && same(end.up, b.up));
}

fn drag(h: &mut Harness<'static, App>, from: egui::Pos2, by: egui::Vec2) {
    h.hover_at(from);
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    for i in 1..=5 {
        h.hover_at(from + by * (i as f32 / 5.0));
        h.run();
    }
    let to = from + by;
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

#[test]
fn drawing_tab_shows_the_sheet_and_dragging_moves_a_view() {
    let mut h = harness(with_body());
    h.run();
    // No drawing yet: the tab starts one.
    h.get_by_label("DRAWING").click();
    h.run();
    assert_eq!(form_command(&h), Some("drawing.create"));
    h.state_mut().form = None;
    h.state_mut().run("drawing.create", json!({"standard_views": true})).unwrap();
    h.state_mut().wait_idle();
    // Now the tab opens it, with the drawing tools in the ribbon.
    h.get_by_label("DRAWING").click();
    h.run();
    assert_eq!(h.state().active_drawing(), Some("Drawing1"));
    for _ in 0..4 {
        h.state_mut().wait_idle();
        h.run();
    }
    let _ = h.get_by_label(title("drawing.add_view"));
    let views = h.state().sheet.as_ref().map(|(_, s)| s.views.clone()).expect("the sheet was built");
    let front = views.iter().find(|v| v.name == "Front").unwrap().clone();
    let top_before = views.iter().find(|v| v.name == "Top").unwrap().min[0];
    let center = [(front.min[0] + front.max[0]) / 2.0, (front.min[1] + front.max[1]) / 2.0];
    let at = h.state().sheet_state.screen(center).unwrap();
    let zoom =
        h.state().sheet_state.screen([1.0, 0.0]).unwrap().x - h.state().sheet_state.screen([0.0, 0.0]).unwrap().x;
    drag(&mut h, at, egui::Vec2::new(60.0, 0.0));
    for _ in 0..4 {
        h.state_mut().wait_idle();
        h.run();
    }
    let text = h.state().snapshot.doc.to_text();
    let moved = center[0] + f64::from(60.0 / zoom);
    let want = format!("view Front (from: front, at: [{}, ", (moved * 10.0).round() / 10.0);
    assert!(text.contains(&want), "{want} in\n{text}");
    // The projected view followed its parent.
    let views = &h.state().sheet.as_ref().unwrap().1.views;
    let top_after = views.iter().find(|v| v.name == "Top").unwrap().min[0];
    assert!((top_after - top_before - f64::from(60.0 / zoom)).abs() < 0.2, "{top_before} -> {top_after}");
    // A note is dragged by its text.
    h.state_mut()
        .run("drawing.add_note", json!({"drawing": "Drawing1", "text": "Deburr", "at": ["40", "60"]}))
        .unwrap();
    for _ in 0..4 {
        h.state_mut().wait_idle();
        h.run();
    }
    let note = h.state().sheet.as_ref().unwrap().1.handles.iter().find(|x| x.item == "Note1").cloned().unwrap();
    let grip = [(note.min[0] + note.max[0]) / 2.0, (note.min[1] + note.max[1]) / 2.0];
    let at = h.state().sheet_state.screen(grip).unwrap();
    drag(&mut h, at, egui::Vec2::new(0.0, -40.0));
    for _ in 0..4 {
        h.state_mut().wait_idle();
        h.run();
    }
    let text = h.state().snapshot.doc.to_text();
    let y = ((60.0 + f64::from(40.0 / zoom)) * 10.0).round() / 10.0;
    assert!(text.contains(&format!("note Note1 (text: \"Deburr\", at: [40, {y}])")), "{y} in\n{text}");
    // Back to the model.
    h.get_by_label("SOLID").click();
    h.run();
    assert_eq!(h.state().active_drawing(), None);
}

#[test]
fn every_drawing_ribbon_command_is_registered() {
    for id in crate::ribbon::drawing_command_ids() {
        assert!(gusset_commands::find(id).is_some(), "{id}");
    }
    assert_eq!(crate::ribbon::drawing_command_ids().len(), 10);
}
