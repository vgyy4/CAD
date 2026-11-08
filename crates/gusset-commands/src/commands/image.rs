//! `export.image`: a path-traced picture of the model as PNG in the project folder (the
//! Render workspace's "Render image"). Hosts register the renderer once per process
//! ([`set_photo_renderer`]); the commands crate does not depend on the GPU renderer.

use super::*;
use std::sync::{Arc, OnceLock};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<ExportImage>()]
}

/// One body or reference mesh to draw, with its look.
#[derive(Debug, Clone)]
pub struct PhotoBody {
    pub name: String,
    pub mesh: Arc<gusset_kernel::Mesh>,
    /// Persistent face names (empty for reference meshes).
    pub face_names: Vec<String>,
    pub look: Option<gusset_doc::Appearance>,
    /// Faces in colours of their own from an imported mesh file (face index, `0xRRGGBB`).
    pub face_colors: Vec<(u32, u32)>,
}

/// What [`PhotoRenderer::photo`] draws and how.
#[derive(Debug, Clone)]
pub struct PhotoRequest {
    pub bodies: Vec<PhotoBody>,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    /// A named view: iso, iso_back, front, back, top, bottom, left, right.
    pub view: String,
}

/// Path-traces a [`PhotoRequest`] to PNG bytes. Implemented by the hosts (gusset-mcp's
/// scene renderer).
pub trait PhotoRenderer: Send + Sync {
    /// Renders the picture; should stop soon after `stop` is set (Stop, an agent's deadline)
    /// and return an error then.
    ///
    /// # Errors
    /// A message for the user.
    fn photo(&self, request: &PhotoRequest, stop: &std::sync::atomic::AtomicBool) -> Result<Vec<u8>, String>;
}

static RENDERER: OnceLock<Arc<dyn PhotoRenderer>> = OnceLock::new();

/// Registers the renderer `export.image` uses (the first registration wins).
pub fn set_photo_renderer(r: Arc<dyn PhotoRenderer>) {
    let _ = RENDERER.set(r);
}

/// The largest picture side and the most samples a pixel.
const MAX_SIDE: u32 = 4096;
const MAX_SAMPLES: u32 = 1024;
/// Most paths one picture may trace (width x height x samples): about a minute on four
/// cores in a release build (dogfooding F16-6). 1280 x 800 at 64 samples fits.
const MAX_PATHS: u64 = 1 << 26;
const VIEWS: [&str; 8] = ["iso", "iso_back", "front", "back", "top", "bottom", "left", "right"];

/// Path-traces the model to a PNG in the project folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ExportImage {
    /// File name ending in .png.
    pub path: String,
    /// Pixels (default 1280 x 800).
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// Paths a pixel (default 32): more is smoother and slower.
    #[serde(default)]
    pub samples: Option<u32>,
    /// iso (default), front, top, right, left, back, bottom, iso_back.
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for ExportImage {
    const ID: &'static str = "export.image";
    const TITLE: &'static str = "Render image";
    const DESCRIPTION: &'static str =
        "Path-trace the model with its appearances to a PNG in the project folder (slow at many samples).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (width, height) = (self.width.unwrap_or(1280), self.height.unwrap_or(800));
        let samples = self.samples.unwrap_or(32);
        if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
            return Err(CommandError::args(format!("width and height are 1 to {MAX_SIDE} pixels")));
        }
        if !(1..=MAX_SAMPLES).contains(&samples) {
            return Err(CommandError::args(format!("samples is 1 to {MAX_SAMPLES}")));
        }
        if u64::from(width) * u64::from(height) * u64::from(samples) > MAX_PATHS {
            return Err(CommandError::args(format!(
                "{width} x {height} at {samples} samples is too much work (at most {MAX_PATHS} paths)"
            ))
            .hint("use fewer samples or a smaller picture"));
        }
        let view = self.view.as_deref().unwrap_or("iso").trim().to_ascii_lowercase();
        if !VIEWS.contains(&view.as_str()) {
            return Err(CommandError::args(format!("unknown view; use {}", VIEWS.join(", "))));
        }
        let png_name = std::path::Path::new(&self.path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        if !png_name {
            return Err(CommandError::args("the picture is a PNG: give a file name ending in .png"));
        }
        let renderer = RENDERER
            .get()
            .cloned()
            .ok_or_else(|| CommandError::new("not_allowed", "rendering is not available in this build"))?;
        let path = crate::paths::export_target(s.project_dir.as_deref(), &self.path, &ctx.actor, &["png"])?;
        if path.exists() {
            if !self.overwrite {
                return Err(CommandError::new("confirmation_required", format!("'{}' already exists", self.path))
                    .hint("pass overwrite: true to replace it"));
            }
            if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
                return Err(CommandError::approval("overwriting an exported file"));
            }
        }
        let ev = s.evaluate(&ctx.actor)?;
        if ev.bodies.is_empty() && ev.meshes.is_empty() {
            return Err(CommandError::args("there is nothing to render").hint("create a body first"));
        }
        let looks = s.document(&ctx.actor)?.appearances;
        let look = |n: &str| looks.iter().find(|a| a.body == n).cloned();
        let tp = gusset_kernel::TessellationParams::default();
        let mut bodies = Vec::with_capacity(ev.bodies.len() + ev.meshes.len());
        for b in &ev.bodies {
            let mesh =
                s.tessellate_cached(&b.shape, &tp).map_err(|e| CommandError::new("evaluation", e.to_string()))?;
            bodies.push(PhotoBody {
                name: b.name.clone(),
                mesh,
                face_names: b.names.faces.iter().map(ToString::to_string).collect(),
                look: look(&b.name),
                face_colors: Vec::new(),
            });
        }
        for m in &ev.meshes {
            bodies.push(PhotoBody {
                name: m.name.clone(),
                mesh: Arc::new(m.mesh.clone()),
                face_names: Vec::new(),
                look: look(&m.name),
                face_colors: m.face_colors.clone(),
            });
        }
        let request = PhotoRequest { bodies, width, height, samples, view };
        let t0 = std::time::Instant::now();
        // Long renders stop with the Stop button and agents' deadlines (review R3).
        let stop = s.rebuild_cancel_flag();
        let png = renderer.photo(&request, &stop).map_err(|e| {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                CommandError::new("cancelled", "the render was stopped")
            } else {
                CommandError::new("io", e)
            }
        })?;
        crate::paths::write_file(&path, &self.path, &png)?;
        Ok(merge(
            json!({ "written": self.path, "bytes": png.len(), "width": width, "height": height, "samples": samples, "seconds": (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0 }),
            rebuild_report(&ev),
        ))
    }
}
