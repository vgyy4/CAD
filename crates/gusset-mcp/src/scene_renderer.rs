//! The real `render_view` renderer (cargo feature `render`): adapts
//! [`gusset_render::ViewRenderer`] (GPU headless with the wgpu fallback chain, CPU
//! rasterizer when no adapter exists) to this crate's [`ViewRenderer`] hook, so
//! `render_view` always returns a PNG.
//!
//! The bodies arrive already tessellated by the session's kernel (see
//! `server::render_view`); this module only builds a [`gusset_render::Scene`], the camera
//! and the highlights, and renders on the calling (blocking worker) thread.
//!
//! The GPU device is created lazily on the first `render_view` call: a headless MCP
//! session that never renders never touches the GPU stack.

use crate::render::{Camera, NamedView, RenderRequest, RenderScene, RenderStyle, ViewRenderer};
use gusset_render::{
    AdapterPreference, Highlight, MeshId, Projection, Scene, SoftwareRenderer, ViewRenderer as RenderPath, math,
};
use std::sync::{Mutex, PoisonError};

/// Which rendering path [`SceneRenderer`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderBackend {
    /// GPU (hardware → WARP/lavapipe → GL), CPU rasterizer if no adapter works.
    #[default]
    Auto,
    /// Always the CPU rasterizer (deterministic; tests and `--software` setups).
    Software,
}

/// A [`ViewRenderer`] backed by `gusset-render`. Cheap to create; the GPU is initialized
/// on first use.
#[derive(Debug, Default)]
pub struct SceneRenderer {
    backend: RenderBackend,
    path: Mutex<Option<RenderPath>>,
}

impl SceneRenderer {
    /// GPU when available, CPU fallback otherwise.
    #[must_use]
    pub fn new() -> SceneRenderer {
        Self::with_backend(RenderBackend::Auto)
    }

    /// A renderer restricted to `backend`.
    #[must_use]
    pub fn with_backend(backend: RenderBackend) -> SceneRenderer {
        SceneRenderer { backend, path: Mutex::new(None) }
    }

    /// Describes the path in use (initializing it if needed), for diagnostics.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut guard = self.path.lock().unwrap_or_else(PoisonError::into_inner);
        self.init(&mut guard).describe()
    }

    fn init<'a>(&self, slot: &'a mut Option<RenderPath>) -> &'a mut RenderPath {
        slot.get_or_insert_with(|| match self.backend {
            RenderBackend::Auto => RenderPath::with_preference(AdapterPreference::Auto),
            RenderBackend::Software => {
                RenderPath::Software(SoftwareRenderer::new(), "software rendering requested".into())
            }
        })
    }
}

/// Builds the render scene: one mesh per body; wireframe drops the triangles and keeps
/// the edge polylines; highlight names select faces and edges.
///
/// # Errors
/// A message for the agent when a mesh is invalid or a highlight name matches nothing.
pub fn build_scene(request: &RenderRequest, scene: &RenderScene) -> Result<Scene, String> {
    let mut out = Scene::new();
    let mut matched = vec![false; request.highlight.len()];
    for (i, body) in scene.bodies.iter().enumerate() {
        let id = MeshId(i as u64);
        let inserted = match request.style {
            RenderStyle::Shaded => out.insert_mesh(id, &body.mesh),
            RenderStyle::Wireframe => {
                let mut m = body.mesh.clone();
                m.indices.clear();
                m.face_ranges.clear();
                out.insert_mesh(id, &m)
            }
        };
        // Body names are user text: refer to the body by position only.
        inserted.map_err(|e| format!("body #{} could not be prepared for rendering: {e}", i + 1))?;
        for (h, hit) in request.highlight.iter().zip(matched.iter_mut()) {
            if let Some(f) = body.face_names.iter().position(|n| n == h) {
                *hit = true;
                if request.style == RenderStyle::Shaded {
                    out.set_face_highlight(id, u32::try_from(f).unwrap_or(u32::MAX), Highlight::Select);
                }
            }
            if let Some(e) = body.edge_names.iter().position(|n| n == h) {
                *hit = true;
                out.set_edge_highlight(id, u32::try_from(e).unwrap_or(u32::MAX), Highlight::Select);
            }
        }
    }
    let missing = matched.iter().filter(|m| !**m).count();
    if missing > 0 {
        return Err(format!(
            "{missing} highlight reference(s) matched no face or edge; use names from query_body with names: true"
        ));
    }
    Ok(out)
}

/// The camera for `request`: the explicit one, or the named view fitted to the scene
/// (axis views orthographic, iso perspective).
///
/// # Errors
/// A degenerate explicit camera (eye at the target).
pub fn build_camera(request: &RenderRequest, scene: &Scene) -> Result<gusset_render::Camera, String> {
    let mut cam = gusset_render::Camera::default();
    match &request.camera {
        Some(Camera { eye, target, up, orthographic }) => {
            let dir = math::sub(*target, *eye);
            let dist = math::length(dir);
            if !(dist.is_finite() && dist > 1e-9) {
                return Err("camera eye and target must be different points".into());
            }
            cam.target = *target;
            cam.distance = dist;
            cam.forward = math::scale(dir, 1.0 / dist);
            cam.up = *up;
            cam.orthonormalize();
            cam.projection = if *orthographic { Projection::Orthographic } else { Projection::Perspective };
        }
        None => {
            let (view, ortho) = match request.view {
                NamedView::Iso => (gusset_render::NamedView::Iso, false),
                NamedView::Front => (gusset_render::NamedView::Front, true),
                NamedView::Back => (gusset_render::NamedView::Back, true),
                NamedView::Top => (gusset_render::NamedView::Top, true),
                NamedView::Bottom => (gusset_render::NamedView::Bottom, true),
                NamedView::Left => (gusset_render::NamedView::Left, true),
                NamedView::Right => (gusset_render::NamedView::Right, true),
            };
            cam.set_view(view);
            cam.projection = if ortho { Projection::Orthographic } else { Projection::Perspective };
            cam.fit(&scene.bounds(), f64::from(request.width) / f64::from(request.height.max(1)));
        }
    }
    Ok(cam)
}

impl ViewRenderer for SceneRenderer {
    fn render_png(&self, request: &RenderRequest, scene: &RenderScene) -> Result<Vec<u8>, String> {
        request.validate()?;
        let s = build_scene(request, scene)?;
        let camera = build_camera(request, &s)?;
        let mut guard = self.path.lock().unwrap_or_else(PoisonError::into_inner);
        let path = self.init(&mut guard);
        path.render_png(&s, &camera, [request.width, request.height]).map_err(|e| format!("rendering failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{RenderBody, is_png};
    use gusset_eval::Evaluator;
    use gusset_kernel::Kernel;
    use gusset_kernel::mock::MockKernel;
    use gusset_render::RgbaImage;
    use std::sync::Arc;

    const PLATE: &str = "gusset 1
units mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)
";

    /// Bodies evaluated and tessellated by the MockKernel, as `render_view` does.
    fn plate_scene() -> RenderScene {
        let k: Arc<dyn Kernel> = Arc::new(MockKernel);
        let doc = gusset_doc::Document::from_text(PLATE).unwrap();
        let r = Evaluator::new(k.clone()).evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        let bodies = r
            .bodies
            .iter()
            .map(|b| RenderBody {
                name: b.name.clone(),
                mesh: k.tessellate(&b.shape, &gusset_kernel::TessellationParams::default()).unwrap(),
                face_names: b.names.faces.iter().map(ToString::to_string).collect(),
                edge_names: b.names.edges.iter().map(ToString::to_string).collect(),
            })
            .collect::<Vec<_>>();
        assert_eq!(bodies.len(), 1);
        RenderScene { bodies }
    }

    fn request(w: u32, h: u32) -> RenderRequest {
        serde_json::from_value(serde_json::json!({"width": w, "height": h})).unwrap()
    }

    fn check(renderer: &SceneRenderer) {
        let scene = plate_scene();
        let req = request(160, 120);
        let png = renderer.render_png(&req, &scene).unwrap();
        assert!(is_png(&png));
        let img = RgbaImage::from_png(&png).unwrap();
        assert_eq!((img.width, img.height), (160, 120));
        let empty = RgbaImage::from_png(&renderer.render_png(&req, &RenderScene::default()).unwrap()).unwrap();
        // The fitted body covers the image center; the empty scene shows background there.
        let (cx, cy) = (80, 60);
        assert_ne!(img.pixel(cx, cy), empty.pixel(cx, cy), "{}", renderer.describe());
        // The corner stays background.
        assert_eq!(img.pixel(1, 1), empty.pixel(1, 1));
        // Wireframe and an explicit camera work too, and highlights resolve.
        let face = scene.bodies[0].face_names[0].clone();
        let req: RenderRequest = serde_json::from_value(serde_json::json!({
            "width": 64, "height": 48, "style": "wireframe", "highlight": [face],
            "camera": {"eye": [100.0, -100.0, 80.0], "target": [20.0, 10.0, 5.0], "orthographic": true}
        }))
        .unwrap();
        let img = RgbaImage::from_png(&renderer.render_png(&req, &scene).unwrap()).unwrap();
        assert_eq!((img.width, img.height), (64, 48));
        let bad: RenderRequest =
            serde_json::from_value(serde_json::json!({"width": 8, "height": 8, "highlight": ["nope"]})).unwrap();
        assert!(renderer.render_png(&bad, &scene).unwrap_err().contains("matched no face"));
    }

    #[test]
    fn software_renders_mock_bodies() {
        check(&SceneRenderer::with_backend(RenderBackend::Software));
    }

    #[test]
    fn auto_renders_mock_bodies_on_gpu_or_cpu() {
        let r = SceneRenderer::new();
        eprintln!("render path: {}", r.describe());
        check(&r);
    }

    #[test]
    fn degenerate_camera_is_an_error() {
        let req: RenderRequest = serde_json::from_value(serde_json::json!({
            "camera": {"eye": [1.0, 1.0, 1.0], "target": [1.0, 1.0, 1.0]}
        }))
        .unwrap();
        let r = SceneRenderer::with_backend(RenderBackend::Software);
        assert!(r.render_png(&req, &plate_scene()).unwrap_err().contains("different points"));
    }
}
