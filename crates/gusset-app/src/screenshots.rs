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
    std::fs::create_dir_all(dir.join("data")).ok();
    std::fs::create_dir_all(dir.join("cache")).ok();
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
        // Glass in the viewport: the bracket seen through, a block behind it.
        let mut a = app(theme);
        bracket(&mut a);
        a.run("sketch.create", json!({"plane": "XY"})).ok();
        a.run("sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 10, "y": 35, "width": 20, "height": 20})).ok();
        a.run("feature.extrude", json!({"sketch": "Sketch1", "distance": "30 mm", "op": "new"})).ok();
        a.run("body.appearance", json!({"body": "Body", "color": "#9fd4ff", "finish": "glass"})).ok();
        a.run("body.appearance", json!({"body": "Extrude1", "color": "#d04020"})).ok();
        shoot(&dir, &format!("20-glass-{t}"), a, |h| {
            for _ in 0..4 {
                h.run_steps(5);
                h.state_mut().wait_idle();
            }
        });
        // Face colours (ADR 0045 amendment): a grey bracket with three faces in their own colours.
        let mut a = app(theme);
        bracket(&mut a);
        a.run("body.appearance", json!({"body": "Body", "color": "#9aa3ad"})).ok();
        // Faces turned towards the iso view (+X, -Y, +Z), largest first.
        let mut faces: Vec<(f64, String)> = a
            .run("query.body", json!({"body": "Body", "names": true}))
            .ok()
            .and_then(|v| v["face_names"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|f| {
                let n: Vec<f64> = f["normal"].as_array()?.iter().filter_map(serde_json::Value::as_f64).collect();
                let [x, y, z] = n.as_slice() else { return None };
                (x - y + z > 0.5)
                    .then(|| (f["area_mm2"].as_f64().unwrap_or(0.0), f["name"].as_str().unwrap_or("").to_string()))
            })
            .collect();
        faces.sort_by(|p, q| q.0.total_cmp(&p.0));
        let faces: Vec<String> = faces.into_iter().map(|(_, n)| n).collect();
        for (f, c) in faces.iter().zip(["#d04020", "#2f7fd0", "#e0b020"]) {
            a.run("body.appearance", json!({"body": "Body", "faces": [f], "color": c})).ok();
        }
        shoot(&dir, &format!("21-face-colours-{t}"), a, |h| {
            for _ in 0..4 {
                h.run_steps(5);
                h.state_mut().wait_idle();
            }
        });
        // The Render tab with the photo preview of a copper bracket.
        let mut a = app(theme);
        bracket(&mut a);
        a.run("body.appearance", json!({"body": "Body", "color": "#b87333", "finish": "metal"})).ok();
        a.render_tab = true;
        a.photo.set(true);
        shoot(&dir, &format!("22-render-preview-{t}"), a, |h| {
            for _ in 0..4 {
                h.run_steps(5);
                h.state_mut().wait_idle();
            }
            let t0 = std::time::Instant::now();
            while h.state().photo.samples < 16 && t0.elapsed() < std::time::Duration::from_secs(120) {
                std::thread::sleep(std::time::Duration::from_millis(100));
                h.run_steps(1);
            }
            h.run_steps(2);
        });
        // The appearance form: colour text with its picker, the finishes offered.
        let mut a = app(theme);
        bracket(&mut a);
        a.feature_choice("Body", crate::browser::FeatureChoice::Appearance);
        shoot(&dir, &format!("19-appearance-form-{t}"), a, |_| {});
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

const SYM_LIB: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "10k" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type background))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "GND" (power) (in_bom yes) (on_board yes)
    (property "Reference" "#PWR" (at 0 -6.35 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Value" "GND" (at 0 -3.81 0) (effects (font (size 1.27 1.27))))
    (symbol "GND_1_1" (pin power_in line (at 0 0 270) (length 0) (hide yes) (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))))
"##;

const FOOTPRINT: &str = r#"(footprint "R_0603" (version 20240108) (generator "hand") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -1.5) (layer "F.SilkS"))
  (property "Value" "R_0603" (at 0 1.5) (layer "F.Fab"))
  (fp_rect (start -1.4 -0.7) (end 1.4 0.7) (stroke (width 0.1) (type default)) (fill none) (layer "F.CrtYd"))
  (pad "1" smd rect (at -0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Paste" "F.Mask"))
  (pad "2" smd rect (at 0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Paste" "F.Mask")))"#;

/// An app whose project folder holds the demo symbol and footprint libraries.
fn demo_app(theme: ThemeKind) -> App {
    let mut a = app(theme);
    let dir = std::env::temp_dir().join(format!("gusset-shots-lib-{}-{:?}", std::process::id(), theme));
    std::fs::create_dir_all(dir.join("fp/Pads.pretty")).ok();
    std::fs::write(dir.join("demo.kicad_sym"), SYM_LIB).ok();
    std::fs::write(dir.join("fp/Pads.pretty/R_0603.kicad_mod"), FOOTPRINT).ok();
    a.set_project_dir(dir);
    a.wait_idle();
    a
}

#[test]
fn generate_gallery_pcb_sch() {
    let Some(dir) = std::env::var_os("GUSSET_SCREENSHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).ok();
    for (theme, t) in [(ThemeKind::Dark, "dark"), (ThemeKind::Light, "light")] {
        // The PCB tab with a demo board: two resistors, a track, a via.
        let mut a = demo_app(theme);
        a.run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).ok();
        for at in [[12, 10], [28, 20]] {
            a.run("pcb.add_footprint", json!({"library": "fp", "footprint": "Pads:R_0603", "at": at})).ok();
        }
        // The route command in 45-degree legs from pad to pad (the router's own search treats
        // these net-less demo pads as obstacles, so the corner is given, not searched).
        let pts = json!([[12.8, 10], [22.8, 20], [27.2, 20]]);
        let r = a.run("pcb.route", json!({"points": pts, "width": 0.25, "around": false}));
        assert!(r.is_ok(), "{r:?}");
        a.run("pcb.add_via", json!({"at": [17.8, 15]})).ok();
        a.wait_idle();
        a.ribbon_action(crate::ribbon::Action::PcbTab);
        shoot(&dir, &format!("24-pcb-{t}"), a, |h| h.run_steps(4));
        // The Schematic tab with two resistors, ground and a label.
        let mut a = demo_app(theme);
        a.run("sch.new", json!({"name": "demo"})).ok();
        for (sym, at) in [("R", [100, 50]), ("R", [110, 50]), ("GND", [100, 60])] {
            a.run("sch.add_symbol", json!({"library": "demo.kicad_sym", "symbol": sym, "at": at})).ok();
        }
        // Footprints assigned, so the ERC has nothing to say about them.
        for r in ["R1", "R2"] {
            a.run("sch.set_field", json!({"symbol": r, "field": "Footprint", "value": "Pads:R_0603"})).ok();
        }
        a.run("sch.add_wire", json!({"points": [[100, 46.19], [110, 46.19]]})).ok();
        a.run("sch.add_wire", json!({"points": [[100, 53.81], [100, 60]]})).ok();
        a.run("sch.add_label", json!({"text": "OUT", "at": [110, 53.81]})).ok();
        a.wait_idle();
        a.ribbon_action(crate::ribbon::Action::SchTab);
        shoot(&dir, &format!("25-schematic-{t}"), a, |h| h.run_steps(4));
        // Empty states: the tabs with nothing open.
        let mut a = app(theme);
        a.ribbon_action(crate::ribbon::Action::PcbTab);
        shoot(&dir, &format!("26-pcb-empty-{t}"), a, |_| {});
        let mut a = app(theme);
        a.ribbon_action(crate::ribbon::Action::SchTab);
        shoot(&dir, &format!("27-schematic-empty-{t}"), a, |_| {});
    }
}
