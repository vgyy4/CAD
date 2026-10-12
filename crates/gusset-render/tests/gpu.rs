//! GPU screenshot tests (golden PNGs in `tests/golden/`) and adapter fallback checks.
//!
//! * No adapter at all: the GPU tests print `SKIP: ...` with the reason and pass,
//!   unless `GUSSET_REQUIRE_GPU=1` (set in CI, where lavapipe/WARP must exist), which
//!   turns the skip into a failure.
//! * `GUSSET_UPDATE_GOLDEN=1` rewrites the golden files from the current output.
//! * On a mismatch, the actual image and a diff visualization are written to
//!   `target/golden-failures/` for inspection.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

use std::path::PathBuf;

use gusset_render::image::diff;
use gusset_render::math::translation;
use gusset_render::{
    AdapterPreference, Camera, DiffTolerance, HeadlessRenderer, Highlight, Material, MeshId, NamedView, Projection,
    RgbaImage, Scene, SoftwareRenderer, Srgb, Theme, ViewOptions, demo,
};

const SIZE: [u32; 2] = [192, 144];

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

fn failure_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("golden-failures")
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

/// A box and a cylinder side by side.
fn scene() -> Scene {
    let mut s = Scene::new();
    s.insert_mesh(MeshId(1), &demo::box_mesh([20.0, 20.0, 20.0])).expect("box");
    s.set_transform(MeshId(1), translation([-14.0, 0.0, 10.0]));
    s.insert_mesh(MeshId(2), &demo::cylinder_mesh(9.0, 24.0, 48)).expect("cylinder");
    s.set_transform(MeshId(2), translation([14.0, 0.0, 0.0]));
    s.set_material(MeshId(2), Material { color: Srgb(0xC9B79C) });
    s
}

fn highlighted_scene() -> Scene {
    let mut s = scene();
    s.set_face_highlight(MeshId(1), 1, Highlight::Select); // box top
    s.set_face_highlight(MeshId(2), 0, Highlight::Preselect); // cylinder side
    s.set_edge_highlight(MeshId(2), 1, Highlight::Select); // cylinder top circle
    s
}

fn camera(s: &Scene) -> Camera {
    let mut c = Camera::default();
    c.set_view(NamedView::Iso);
    c.fit(&s.bounds(), f64::from(SIZE[0]) / f64::from(SIZE[1]));
    c
}

/// One GPU test at a time: several devices created concurrently crash the WARP
/// software adapter on Windows ARM64 runners (access violation inside the driver).
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn gpu(pref: AdapterPreference) -> Option<HeadlessRenderer> {
    match HeadlessRenderer::with_preference(pref) {
        Ok(r) => {
            eprintln!("adapter ({pref:?}): {}", r.adapter());
            Some(r)
        }
        Err(e) => {
            assert!(!env_flag("GUSSET_REQUIRE_GPU"), "GUSSET_REQUIRE_GPU=1 but no adapter ({pref:?}): {e}");
            eprintln!("SKIP: no wgpu adapter for {pref:?}: {e}");
            None
        }
    }
}

fn check_golden(name: &str, actual: &RgbaImage, tol: DiffTolerance) {
    let path = golden_dir().join(format!("{name}.png"));
    if env_flag("GUSSET_UPDATE_GOLDEN") {
        std::fs::create_dir_all(golden_dir()).expect("mkdir");
        std::fs::write(&path, actual.to_png().expect("png")).expect("write golden");
        eprintln!("updated {}", path.display());
        return;
    }
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("missing golden {} ({e}); run with GUSSET_UPDATE_GOLDEN=1", path.display()));
    let expected = RgbaImage::from_png(&bytes).expect("decode golden");
    let report = diff(&expected, actual, tol);
    eprintln!("{name}: {report}");
    if !report.passed {
        let dir = failure_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{name}.actual.png")), actual.to_png().expect("png"));
        if let Some(d) = &report.diff_image {
            let _ = std::fs::write(dir.join(format!("{name}.diff.png")), d.to_png().expect("png"));
        }
        panic!("{name} differs from golden: {report} (see {})", dir.display());
    }
}

fn render(r: &mut HeadlessRenderer, s: &Scene, theme: Theme) -> RgbaImage {
    r.renderer_mut().set_options(ViewOptions { theme, ..ViewOptions::default() });
    let (img, stats) = r.render_rgba(s, &camera(s), SIZE).expect("render");
    assert_eq!(stats.meshes, 2);
    assert!(stats.triangles > 0 && stats.edge_segments > 0);
    img
}

#[test]
fn golden_dark_light_and_highlight() {
    let _g = serial();
    let Some(mut r) = gpu(AdapterPreference::Auto) else { return };
    let s = scene();
    check_golden("box_cylinder_dark", &render(&mut r, &s, Theme::Dark), DiffTolerance::SCREENSHOT);
    check_golden("box_cylinder_light", &render(&mut r, &s, Theme::Light), DiffTolerance::SCREENSHOT);
    let h = highlighted_scene();
    check_golden("box_cylinder_highlight_dark", &render(&mut r, &h, Theme::Dark), DiffTolerance::SCREENSHOT);
}

/// The CPU fallback has its own golden (it has no anti-aliasing and rasterizes
/// slightly differently), and must also resemble the GPU image.
#[test]
fn software_golden_and_resemblance() {
    let _g = serial();
    let s = highlighted_scene();
    let sw = SoftwareRenderer::new().render_rgba(&s, &camera(&s), SIZE).expect("software render");
    check_golden("software_highlight_dark", &sw, DiffTolerance::SCREENSHOT);
    let Some(mut r) = gpu(AdapterPreference::Auto) else { return };
    let hw = render(&mut r, &s, Theme::Dark);
    let report = diff(&hw, &sw, DiffTolerance { per_channel: 40, max_differing_ratio: 0.06 });
    eprintln!("gpu vs software: {report}");
    assert!(report.passed, "software fallback looks too different from the GPU path: {report}");
}

/// Every adapter class available here renders the same image within tolerance.
#[test]
fn each_available_adapter_matches_golden() {
    let _g = serial();
    let s = scene();
    let mut any = false;
    for pref in [AdapterPreference::Primary, AdapterPreference::SoftwareFallback, AdapterPreference::Gl] {
        let r = match HeadlessRenderer::with_preference(pref) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("SKIP: {pref:?} unavailable: {e}");
                continue;
            }
        };
        any = true;
        let mut r = r;
        eprintln!("{pref:?}: {} tier={:?}", r.adapter(), r.renderer().quality().tier());
        let img = render(&mut r, &s, Theme::Dark);
        check_golden("box_cylinder_dark", &img, DiffTolerance::SCREENSHOT);
    }
    if !any {
        assert!(!env_flag("GUSSET_REQUIRE_GPU"), "GUSSET_REQUIRE_GPU=1 but no adapter class worked");
        eprintln!("SKIP: no adapter of any class");
    }
}

#[test]
fn wide_edges_msaa_scale_and_ortho_render_without_errors() {
    let _g = serial();
    let Some(mut r) = gpu(AdapterPreference::Auto) else { return };
    let s = highlighted_scene();
    let cam = camera(&s);
    let dark_px = |img: &RgbaImage| img.data.chunks_exact(4).filter(|p| p.iter().take(3).all(|c| *c < 30)).count();

    // HiDPI: 2 physical px edges use the quad path and cover more pixels.
    r.renderer_mut().set_options(ViewOptions::default());
    let (thin, st) = r.render_rgba(&s, &cam, SIZE).expect("thin");
    assert!(!st.quad_edges);
    r.renderer_mut().set_options(ViewOptions { pixels_per_point: 2.0, ..ViewOptions::default() });
    let (wide, st) = r.render_rgba(&s, &cam, SIZE).expect("wide");
    assert!(st.quad_edges);
    assert!(dark_px(&wide) > dark_px(&thin) * 3 / 2, "{} vs {}", dark_px(&wide), dark_px(&thin));

    // Medium tier: 4x MSAA.
    r.renderer_mut().set_options(ViewOptions::default());
    r.renderer_mut().set_quality_tier(gusset_hw::QualityTier::Medium);
    let (aa, st) = r.render_rgba(&s, &cam, SIZE).expect("msaa");
    assert_eq!(st.samples, 4);
    let d = diff(&thin, &aa, DiffTolerance { per_channel: 40, max_differing_ratio: 0.05 });
    assert!(d.passed, "MSAA should only change edges: {d}");

    // Interacting: 1x MSAA, reduced render scale, upscaled into the full-size target.
    r.renderer_mut().set_quality_tier(gusset_hw::QualityTier::Low);
    r.renderer_mut().set_interacting(true);
    let (moving, st) = r.render_rgba(&s, &cam, SIZE).expect("interacting");
    assert_eq!(st.samples, 1);
    assert!(st.render_size[0] < SIZE[0]);
    assert_eq!((moving.width, moving.height), (SIZE[0], SIZE[1]));
    let d = diff(&thin, &moving, DiffTolerance { per_channel: 60, max_differing_ratio: 0.12 });
    assert!(d.passed, "reduced-scale frame should resemble the still frame: {d}");
    r.renderer_mut().set_interacting(false);

    // Orthographic projection renders the part (center pixel is not background).
    let mut ortho = cam;
    ortho.projection = Projection::Orthographic;
    let (img, _) = r.render_rgba(&s, &ortho, SIZE).expect("ortho");
    let bg = Theme::Dark.colors();
    let c = img.pixel(SIZE[0] / 4, SIZE[1] / 2).expect("px");
    let top = bg.bg_top.bytes();
    let bottom = bg.bg_bottom.bytes();
    assert!((0..3).any(|k| c[k].abs_diff(top[k]) > 20 && c[k].abs_diff(bottom[k]) > 20), "{c:?}");

    // Empty scene: just the gradient (top lighter than bottom in the dark theme).
    let (img, st) = r.render_rgba(&Scene::new(), &cam, [16, 64]).expect("empty");
    assert_eq!(st.meshes, 0);
    let t = img.pixel(8, 0).expect("px");
    let b = img.pixel(8, 63).expect("px");
    assert!(t[0] > b[0] && t[2] > b[2], "{t:?} {b:?}");
    assert!(r.render_rgba(&s, &cam, [0, 5]).is_err());
}

#[test]
fn diagnostics_report_tier() {
    let _g = serial();
    let Some(r) = gpu(AdapterPreference::Auto) else { return };
    let mut d = gusset_hw::Diagnostics::collect();
    d.set_gpu(r.gpu_diagnostics(None));
    let text = d.to_text();
    eprintln!("{text}");
    assert!(text.contains("gpu.backend: "));
    assert!(text.contains("gpu.tier: "));
    let o = r.gpu_diagnostics(Some(gusset_hw::QualityTier::High));
    assert!(o.tier_overridden && o.tier == gusset_hw::QualityTier::High);
}
