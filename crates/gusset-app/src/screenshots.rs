//! Screenshot gallery generator (spec: docs/screenshots/ updated at each milestone).
//!
//! Not a check: it runs only when `GUSSET_SCREENSHOTS=<dir>` is set, renders the key
//! screens in both themes with wgpu, and writes PNGs there. The real kernel is used
//! when this build has OCCT.

use crate::app::{App, Dialog};
use crate::theme::ThemeKind;
use egui_kittest::Harness;
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
        a.dialog = Some(Dialog::Close { save_failed: None });
        shoot(&dir, &format!("07-close-dialog-{t}"), a, |_| {});
    }
}
