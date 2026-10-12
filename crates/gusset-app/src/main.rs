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

/// Vulkan/Metal/DX12 first. On Windows, DX12 (with WARP when there is no GPU) always
/// exists, so the GL backend is left out there (ADR 0009); elsewhere it is the last
/// resort before the CPU viewport.
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    if cfg!(windows) {
        setup.instance_descriptor.backends =
            eframe::wgpu::Backends::from_env().unwrap_or(eframe::wgpu::Backends::PRIMARY);
    }
    eframe::egui_wgpu::WgpuConfiguration {
        wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    // Code-loading hardening before anything else (Windows DLL search path; ADR 0008 §6).
    if let Some(e) = gusset_hw::codeload::harden() {
        eprintln!("warning: {e}");
    }
    let (k, note) = kernel();
    // IGES input copies of an import that was cut short by a crash of this app.
    gusset_worker::sweep_stale_import_dirs(std::time::Duration::from_secs(24 * 3600));
    let dirs = gusset_store::paths::DataDirs::from_os();
    // B-rep blobs written by OCCT's writer (ours, or the worker's canonical output) are
    // trusted for this installation (ADR 0008 §4). "v2": the v1 list also held hashes
    // of untrusted originals, which must not be trusted (security re-review 2).
    if let Some(d) = &dirs
        && let Err(e) = gusset_worker::trust::persist_to(&d.data_dir().join("trusted-blobs-v2"))
    {
        eprintln!("warning: trusted-blob list unavailable: {e}");
    }
    let grant_file = dirs.as_ref().map(|d| d.data_dir().join("agent-grants.json"));
    let mut app = App::new(k, dirs);
    // Remembered "always allow" agent grants (ADR 0012 "Standing approvals").
    if let Some(f) = &grant_file
        && let Err(e) = app.agents.load_grants(f)
    {
        app.notice = Some(gusset_app::app::Notice { text: e, hint: None, error: true });
    }
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
        wgpu_options: wgpu_options(),
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
