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

/// An explicit eye/target camera or an orbit camera in degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    /// Eye position in model coordinates; give with `target` instead of orbit angles.
    #[serde(default)]
    pub eye: Option<[f64; 3]>,
    /// Point looked at in model coordinates; give with `eye` instead of orbit angles.
    #[serde(default)]
    pub target: Option<[f64; 3]>,
    /// Up direction (default +Z).
    #[serde(default = "default_up")]
    pub up: [f64; 3],
    /// Orthographic projection instead of perspective.
    #[serde(default)]
    pub orthographic: bool,
    /// Orbit azimuth in degrees, counter-clockwise from +X in the XY plane.
    #[serde(default)]
    pub azimuth: Option<f64>,
    /// Orbit elevation in degrees above the XY plane.
    #[serde(default)]
    pub elevation: Option<f64>,
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
    /// Isometric view from the opposite azimuth to `iso`.
    IsoBack,
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
    /// A drawing to show.
    #[serde(default)]
    pub drawing: Option<String>,
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
        if let Some(c) = &self.camera {
            let explicit = c.eye.is_some() || c.target.is_some();
            let orbit = c.azimuth.is_some() || c.elevation.is_some();
            if explicit && orbit {
                return Err("camera must use eye/target or azimuth/elevation, not both".into());
            }
            match (c.eye, c.target, c.azimuth, c.elevation) {
                (Some(eye), Some(target), None, None) => {
                    if eye.iter().chain(&target).chain(&c.up).any(|v| !v.is_finite()) {
                        return Err("camera coordinates must be finite numbers".into());
                    }
                }
                (None, None, Some(azimuth), Some(elevation)) => {
                    if !azimuth.is_finite() || !elevation.is_finite() {
                        return Err("camera angles must be finite numbers".into());
                    }
                    if !c.up.iter().all(|v| v.is_finite()) {
                        return Err("camera coordinates must be finite numbers".into());
                    }
                }
                _ => return Err("camera needs eye and target, or azimuth and elevation".into()),
            }
        }
        Ok(())
    }
}

impl Camera {
    /// Unit vector from the orbit target toward the eye.
    #[must_use]
    pub fn orbit_eye_direction(azimuth: f64, elevation: f64) -> [f64; 3] {
        let (a, e) = (azimuth.to_radians(), elevation.to_radians());
        [libm::cos(e) * libm::cos(a), libm::cos(e) * libm::sin(a), libm::sin(e)]
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

    /// Prepares for the first render (device, pipelines) so it does not pay for that.
    /// Blocking: call it on a thread of its own.
    fn warm(&self) {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_angles_give_expected_eye_direction() {
        let direction = Camera::orbit_eye_direction(90.0, 30.0);
        assert!(direction[0].abs() < 1e-12);
        assert!((direction[1] - libm::cos(30.0_f64.to_radians())).abs() < 1e-12);
        assert!((direction[2] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn camera_forms_are_mutually_exclusive() {
        let request: RenderRequest = serde_json::from_value(serde_json::json!({
            "camera": {
                "eye": [1.0, 2.0, 3.0], "target": [0.0, 0.0, 0.0],
                "azimuth": 30.0, "elevation": 20.0
            }
        }))
        .unwrap();
        assert!(request.validate().unwrap_err().contains("not both"));
    }

    #[test]
    fn non_finite_orbit_angles_are_refused() {
        let request = RenderRequest {
            view: NamedView::default(),
            camera: Some(Camera {
                eye: None,
                target: None,
                up: default_up(),
                orthographic: false,
                azimuth: Some(30.0),
                elevation: Some(f64::NAN),
            }),
            width: default_size(),
            height: default_height(),
            style: RenderStyle::default(),
            highlight: Vec::new(),
            drawing: None,
        };
        assert!(request.validate().unwrap_err().contains("finite"));
    }
}
