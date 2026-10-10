//! The `render_view` hook. The real renderer lives in the renderer crate (GPU, offscreen)
//! and plugs in through [`ViewRenderer`]; headless sessions have none and the tool then
//! returns a clear structured error.

use gusset_kernel::Mesh;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Largest image edge `render_view` accepts.
pub const MAX_RENDER_SIZE: u32 = 2048;
/// Largest PNG we pass back to an agent.
pub const MAX_PNG_BYTES: usize = 16 << 20;

/// A camera given explicitly (model coordinates, mm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    /// Eye position.
    pub eye: [f64; 3],
    /// Point looked at.
    pub target: [f64; 3],
    /// Up direction (default +Z).
    #[serde(default = "default_up")]
    pub up: [f64; 3],
    /// Orthographic projection instead of perspective.
    #[serde(default)]
    pub orthographic: bool,
}

fn default_up() -> [f64; 3] {
    [0.0, 0.0, 1.0]
}

/// Display style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RenderStyle {
    /// Shaded with edges.
    #[default]
    Shaded,
    /// Edges only.
    Wireframe,
}

/// Named views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NamedView {
    /// Isometric (default).
    #[default]
    Iso,
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
}

/// Arguments of the `render_view` tool (also what the renderer receives).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderRequest {
    /// Named view (ignored when `camera` is given).
    #[serde(default)]
    pub view: NamedView,
    /// Explicit camera.
    #[serde(default)]
    pub camera: Option<Camera>,
    /// Image width in pixels (1..=2048, default 800).
    #[serde(default = "default_size")]
    pub width: u32,
    /// Image height in pixels (1..=2048, default 600).
    #[serde(default = "default_height")]
    pub height: u32,
    /// Shaded or wireframe.
    #[serde(default)]
    pub style: RenderStyle,
    /// Face or edge names to highlight (from query_body with names: true).
    #[serde(default)]
    pub highlight: Vec<String>,
}

fn default_size() -> u32 {
    800
}

fn default_height() -> u32 {
    600
}

impl RenderRequest {
    /// Checks sizes and the highlight list.
    ///
    /// # Errors
    /// A message for the agent.
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_RENDER_SIZE).contains(&self.width) || !(1..=MAX_RENDER_SIZE).contains(&self.height) {
            return Err(format!("width and height must be between 1 and {MAX_RENDER_SIZE} pixels"));
        }
        if self.highlight.len() > 1000 || self.highlight.iter().any(|h| h.len() > 512) {
            return Err("too many or too long highlight references".into());
        }
        if let Some(c) = &self.camera
            && c.eye.iter().chain(&c.target).chain(&c.up).any(|v| !v.is_finite())
        {
            return Err("camera coordinates must be finite numbers".into());
        }
        Ok(())
    }
}

/// One body, tessellated, with the persistent names of its faces and edges (in the
/// order of `mesh.face_ranges` and `mesh.edges`).
#[derive(Debug, Clone)]
pub struct RenderBody {
    /// Body name.
    pub name: String,
    /// Triangle mesh and edge polylines.
    pub mesh: Mesh,
    /// Persistent face names.
    pub face_names: Vec<String>,
    /// Persistent edge names.
    pub edge_names: Vec<String>,
}

/// What to draw: the evaluated bodies of the agent's current branch.
#[derive(Debug, Clone, Default)]
pub struct RenderScene {
    /// Bodies.
    pub bodies: Vec<RenderBody>,
}

/// Renders a view of the model to PNG. Implemented by the renderer crate; called on a
/// blocking worker thread, never on the model thread.
pub trait ViewRenderer: Send + Sync + 'static {
    /// Renders `scene` as described by `request` and returns PNG bytes.
    ///
    /// # Errors
    /// A message for the agent (e.g. "no GPU adapter available").
    fn render_png(&self, request: &RenderRequest, scene: &RenderScene) -> Result<Vec<u8>, String>;
}

/// A test double: a valid 1×1 PNG whatever the request (records nothing).
#[derive(Debug, Default, Clone, Copy)]
pub struct StubRenderer;

/// A 1×1 transparent PNG.
pub const STUB_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

impl ViewRenderer for StubRenderer {
    fn render_png(&self, request: &RenderRequest, _scene: &RenderScene) -> Result<Vec<u8>, String> {
        request.validate()?;
        Ok(STUB_PNG.to_vec())
    }
}

/// True if `bytes` start with the PNG signature.
#[must_use]
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}
