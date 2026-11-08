//! Screenshot gallery generator (spec: docs/screenshots/ updated at each milestone).
//!
//! Not a check: it runs only when `GUSSET_SCREENSHOTS=<dir>` is set, renders the key
//! screens in both themes with wgpu, and writes PNGs there. The real kernel is used
//! when this build has OCCT.

use crate::app::{App, Dialog};
use crate::theme::ThemeKind;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use gusset_kernel::Kernel;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn kernel() -> Arc<dyn Kernel> {
    match gusset_kernel_occt::OcctKernel::new() {
        Ok(k) => Arc::new(k),
        Err(_) => Arc::new(gusset_kernel::mock::MockKernel),
    }
}

fn app(theme: ThemeKind) -> App {
    let dir = std::env::temp_dir().join(format!("gusset-shots-{}-{:?}", std::process::id(), theme));
    let dirs = gusset_store::paths::DataDirs::at(dir.join("data"), dir.join("cache"));
    let mut a = App::new(kernel(), Some(dirs));
    a.theme = theme;
    a
}

fn bracket(a: &mut App) {
    let text = include_str!("../../../examples/bracket.gus.txt");
    a.run("document.set_text", json!({ "text": text })).ok();
    a.wait_idle();
}

fn shoot(dir: &Path, name: &str, mut a: App, setup: impl FnOnce(&mut Harness<'static, App>)) {
    // Built like the real window (eframe), so the viewport renders on the harness's wgpu
    // device exactly as it does on eframe's.
    let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).wgpu().build_eframe(move |cc| {
        a.set_wgpu(cc.wgpu_render_state.as_ref());
        a
    });
    h.run_steps(4);
    setup(&mut h);
    h.state_mut().wait_idle();
    h.run_steps(6);
    match h.render() {
        Ok(img) => {
            let path = dir.join(format!("{name}.png"));
            if let Err(e) = img.save(&path) {
                eprintln!("{name}: {e}");
            }
        }
        Err(e) => eprintln!("{name}: render failed: {e}"),
    }
}

#[test]
fn generate_gallery() {
    let Some(dir) = std::env::var_os("GUSSET_SCREENSHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).ok();
    for (theme, t) in [(ThemeKind::Dark, "dark"), (ThemeKind::Light, "light")] {
        shoot(&dir, &format!("01-empty-{t}"), app(theme), |_| {});
        let mut a = app(theme);
        bracket(&mut a);
        shoot(&dir, &format!("02-model-{t}"), a, |_| {});
        let mut a = app(theme);
        bracket(&mut a);
        a.toggle_code();
        shoot(&dir, &format!("03-code-view-{t}"), a, |_| {});
        let mut a = app(theme);
        bracket(&mut a);
        a.start_command("feature.extrude");
        shoot(&dir, &format!("04-command-form-{t}"), a, |_| {});
        let mut a = app(theme);
        bracket(&mut a);
        a.palette = Some("fil".into());
        shoot(&dir, &format!("05-command-search-{t}"), a, |_| {});
        let mut a = app(theme);
        bracket(&mut a);
        a.agents.show_panel = true;
        shoot(&dir, &format!("06-agents-panel-{t}"), a, |_| {});
        let mut a = app(theme);
        bracket(&mut a);
        a.dialog = Some(Dialog::Unsaved { pending: crate::app::Pending::Close, save_failed: None });
        shoot(&dir, &format!("07-close-dialog-{t}"), a, |_| {});
        // Sketch editing: the SKETCH tab's ribbon with "Finish sketch".
        let mut a = app(theme);
        bracket(&mut a);
        a.edit_sketch("Profile");
        shoot(&dir, &format!("08-sketch-mode-{t}"), a, |_| {});
        // A ribbon dropdown listing every command of its group.
        let mut a = app(theme);
        bracket(&mut a);
        shoot(&dir, &format!("09-ribbon-dropdown-{t}"), a, |h| {
            h.get_by_label("CREATE").click();
            h.run_steps(3);
        });
        // The Drawing tab: a sheet with front, top, right and iso views.
        let mut a = app(theme);
        bracket(&mut a);
        a.run("drawing.create", json!({"name": "Bracket_dwg", "title": "Bracket", "standard_views": true})).ok();
        a.wait_idle();
        a.open_drawing("Bracket_dwg");
        shoot(&dir, &format!("10-drawing-{t}"), a, |h| {
            for _ in 0..3 {
                h.state_mut().wait_idle();
                h.run_steps(2);
            }
        });
        // The section view: the bracket cut open across Y, with its controls.
        let mut a = app(theme);
        bracket(&mut a);
        a.viewport.section = Some(crate::viewport::SectionView::default());
        shoot(&dir, &format!("11-section-{t}"), a, |_| {});
        // Construction planes: the XY origin plane shown from the browser's Origin.
        let mut a = app(theme);
        bracket(&mut a);
        a.viewport.set_plane_visible("XY", true);
        a.browser.set_open(crate::browser::Category::Origin, true);
        shoot(&dir, &format!("12-planes-{t}"), a, |_| {});
        // Two picked lines of the edited sketch: the constraints that fit them.
        let mut a = app(theme);
        bracket(&mut a);
        a.edit_sketch("Profile");
        a.viewport.camera.set_view(gusset_render::NamedView::Front);
        a.viewport.fit_pending = true;
        let lines: Vec<String> = match a.snapshot.doc.features.iter().find(|f| f.name == "Profile").map(|f| &f.kind) {
            Some(gusset_doc::FeatureKind::Sketch(s)) => s
                .entities
                .iter()
                .filter(|e| matches!(e.geom, gusset_doc::SkGeom::Line { .. }))
                .map(|e| e.id.clone())
                .collect(),
            _ => Vec::new(),
        };
        if let [l0, l1, ..] = lines.as_slice() {
            a.run("sketch.add_constraint", json!({"sketch": "Profile", "kind": "horizontal", "entities": [l0]})).ok();
            a.run("sketch.add_constraint", json!({"sketch": "Profile", "kind": "vertical", "entities": [l1]})).ok();
            a.run(
                "sketch.add_constraint",
                json!({"sketch": "Profile", "kind": "distance", "entities": [l0, l0], "value": "leg"}),
            )
            .ok();
            a.wait_idle();
        }
        a.selection = lines
            .iter()
            .take(2)
            .map(|id| crate::viewport::Hit::Sketch { sketch: "Profile".into(), id: id.clone() })
            .collect();
        shoot(&dir, &format!("14-constrain-{t}"), a, |_| {});
        // A live preview while a feature is edited: the body as it would be.
        let mut a = app(theme);
        bracket(&mut a);
        a.edit_feature("Body");
        a.form.as_mut().map(|f| f.set("distance", "width * 2"));
        shoot(&dir, &format!("15-preview-{t}"), a, |h| {
            for _ in 0..4 {
                h.run_steps(5);
                h.state_mut().wait_idle();
            }
        });
        // Draft analysis: faces red (negative), yellow (would drag), green (release upwards).
        let mut a = app(theme);
        bracket(&mut a);
        shoot(&dir, &format!("17-draft-analysis-{t}"), a, |h| {
            for _ in 0..4 {
                h.run_steps(5);
                h.state_mut().wait_idle();
            }
            h.state_mut().submit_command(
                "query.draft_analysis",
                serde_json::json!({"direction": [0, 0, 1], "field": true}),
                crate::app::Then::Show,
            );
            for _ in 0..4 {
                h.run_steps(5);
                h.state_mut().wait_idle();
            }
            h.run_steps(3);
        });
        // A body's appearance: copper metal, its swatch in the browser (ADR 0045).
        let mut a = app(theme);
        bracket(&mut a);
        a.run("body.appearance", json!({"body": "Body", "color": "#b87333", "finish": "metal"})).ok();
        shoot(&dir, &format!("18-appearance-{t}"), a, |h| {
            for _ in 0..4 {
                h.run_steps(5);
                h.state_mut().wait_idle();
            }
        });
        // The marking menu (a right click in the viewport).
        let mut a = app(theme);
        bracket(&mut a);
        a.marking = Some(egui::pos2(760.0, 470.0));
        shoot(&dir, &format!("16-marking-menu-{t}"), a, |_| {});
        // Sketch geometry placed with clicks: a line from a clicked point to the cursor.
        let mut a = app(theme);
        bracket(&mut a);
        a.edit_sketch("Profile");
        a.viewport.camera.set_view(gusset_render::NamedView::Front);
        a.viewport.fit_pending = true;
        a.start_command("sketch.add_line");
        shoot(&dir, &format!("13-sketch-line-{t}"), a, |h| {
            let click = egui::pos2(700.0, 420.0);
            h.hover_at(click);
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            h.run_steps(2);
            h.hover_at(egui::pos2(860.0, 330.0));
            h.run_steps(3);
        });
    }
}
