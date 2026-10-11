//! `gusset-gui`: the desktop application.

use gusset_app::app::App;
use gusset_kernel::Kernel;
use std::sync::Arc;

/// The real kernel, or the mock one with a visible warning when OCCT is not available.
fn kernel() -> (Arc<dyn Kernel>, Option<String>) {
    match gusset_kernel_occt::OcctKernel::new() {
        Ok(k) => (Arc::new(k), None),
        Err(e) => (
            Arc::new(gusset_kernel::mock::MockKernel),
            Some(format!("Geometry kernel unavailable ({e}); using the preview kernel")),
        ),
    }
}

fn main() -> eframe::Result {
    let (k, note) = kernel();
    let dirs = gusset_store::paths::DataDirs::from_os();
    let mut app = App::new(k, dirs);
    app.kernel_note = note;
    if let Some(path) = std::env::args_os().nth(1)
        && let Err(e) = app.open(path.into())
    {
        app.notice = Some(gusset_app::app::Notice { text: e, hint: None, error: true });
    }
    // Agents reach the open project over localhost MCP (ADR 0012). `render_view` uses the
    // GPU when an adapter exists (created lazily) and the CPU rasterizer otherwise.
    app.set_renderer(Some(std::sync::Arc::new(gusset_mcp::SceneRenderer::new())));
    app.start_agents(gusset_mcp::HttpOptions::default());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Gusset")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Gusset",
        options,
        Box::new(|cc| {
            // The viewport renders on eframe's wgpu device; without one it uses the CPU.
            app.set_wgpu(cc.wgpu_render_state.as_ref());
            Ok(Box::new(app))
        }),
    )
}
