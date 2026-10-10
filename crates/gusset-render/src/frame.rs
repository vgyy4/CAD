//! Per-frame plan shared by the GPU renderer and the CPU fallback: camera matrices,
//! colors and the list of draws (with highlight runs), after frustum culling.

use std::ops::Range;

use crate::camera::{Camera, Projection};
use crate::math::{self, Mat4, Vec3};
use crate::scene::{Highlight, MeshId, Scene, SceneMesh};
use crate::theme::{Lighting, Theme};

/// How a view is drawn (independent of the GPU/CPU path).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOptions {
    /// Color theme.
    pub theme: Theme,
    /// Physical pixels per logical pixel (DPI scale).
    pub pixels_per_point: f32,
    /// Draw feature edges.
    pub edges: bool,
    /// Edge width in logical pixels (DESIGN.md §5: 1 px).
    pub edge_width: f32,
    /// The lighting rig.
    pub lighting: Lighting,
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            theme: Theme::Dark,
            pixels_per_point: 1.0,
            edges: true,
            edge_width: 1.0,
            lighting: Lighting::DEFAULT,
        }
    }
}

impl ViewOptions {
    /// Edge width in physical pixels.
    #[must_use]
    pub fn edge_width_px(&self) -> f32 {
        let ppp =
            if self.pixels_per_point.is_finite() && self.pixels_per_point > 0.0 { self.pixels_per_point } else { 1.0 };
        (self.edge_width * ppp).clamp(0.5, 16.0)
    }
}

/// One mesh to draw.
#[derive(Debug, Clone)]
pub(crate) struct MeshDraw {
    pub id: MeshId,
    pub geometry_rev: u64,
    /// Object → view.
    pub model_view: Mat4,
    /// Object → view for normals (inverse-transpose), as columns.
    pub normal: [Vec3; 3],
    /// Triangle runs with a linear base color.
    pub tri_runs: Vec<(Range<u32>, [f32; 3])>,
    /// Edge segment runs with a linear color.
    pub edge_runs: Vec<(Range<u32>, [f32; 3])>,
}

/// Everything needed to draw one frame.
#[derive(Debug, Clone)]
pub(crate) struct FramePlan {
    pub proj: Mat4,
    pub ortho: bool,
    pub bg_top: [f32; 3],
    pub bg_bottom: [f32; 3],
    /// View-space pull towards the eye for edges: fraction of distance (perspective).
    pub edge_bias_persp: f32,
    /// View-space pull towards the eye for edges: absolute (orthographic).
    pub edge_bias_ortho: f32,
    pub edge_width_px: f32,
    pub lighting: Lighting,
    pub draws: Vec<MeshDraw>,
}

impl FramePlan {
    pub(crate) fn build(scene: &Scene, camera: &Camera, size: [u32; 2], opts: &ViewOptions) -> FramePlan {
        let colors = opts.theme.colors();
        let bounds = scene.bounds();
        let aspect = f64::from(size[0].max(1)) / f64::from(size[1].max(1));
        let cm = camera.matrices(aspect, &bounds);
        let viewproj = math::mul(&cm.proj, &cm.view);
        let edge = colors.edge.linear();
        let pre = colors.preselect.linear();
        let sel = colors.select.linear();
        let hl = |h: Highlight, base: [f32; 3]| match h {
            Highlight::None => base,
            Highlight::Preselect => pre,
            Highlight::Select => sel,
        };
        let mut draws = Vec::new();
        for (id, m) in scene.meshes() {
            if !m.visible() || culled(&viewproj, m) {
                continue;
            }
            let data = m.data();
            let model_view = math::mul(&cm.view, m.transform());
            let base = m.material().color.linear();
            let tri_runs = SceneMesh::runs(&data.face_ranges, &m.face_highlights, data.triangle_count() as u32)
                .into_iter()
                .map(|(r, h)| (r, hl(h, base)))
                .collect();
            let edge_runs = if opts.edges {
                SceneMesh::runs(&data.edge_ranges, &m.edge_highlights, (data.edge_points.len() / 2) as u32)
                    .into_iter()
                    .map(|(r, h)| (r, hl(h, edge)))
                    .collect()
            } else {
                Vec::new()
            };
            draws.push(MeshDraw {
                id,
                geometry_rev: m.geometry_rev,
                normal: math::normal_matrix(&model_view),
                model_view,
                tri_runs,
                edge_runs,
            });
        }
        FramePlan {
            proj: cm.proj,
            ortho: camera.projection == Projection::Orthographic,
            bg_top: colors.bg_top.linear(),
            bg_bottom: colors.bg_bottom.linear(),
            edge_bias_persp: 1.5e-3,
            edge_bias_ortho: ((cm.far - cm.near) * 1.5e-3) as f32,
            edge_width_px: opts.edge_width_px(),
            lighting: opts.lighting,
            draws,
        }
    }
}

/// Whether a mesh's world box is entirely outside one clip plane.
fn culled(viewproj: &Mat4, m: &SceneMesh) -> bool {
    let b = m.world_bounds();
    if b.is_empty() {
        return true;
    }
    let clip: Vec<[f64; 4]> = b.corners().iter().map(|&c| math::transform4(viewproj, c)).collect();
    let outside = |f: &dyn Fn(&[f64; 4]) -> bool| clip.iter().all(f);
    outside(&|c| c[0] < -c[3])
        || outside(&|c| c[0] > c[3])
        || outside(&|c| c[1] < -c[3])
        || outside(&|c| c[1] > c[3])
        || outside(&|c| c[2] < 0.0)
        || outside(&|c| c[2] > c[3])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn culls_offscreen_meshes_and_splits_highlights() {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(1), &demo::box_mesh([1.0; 3])).expect("ok");
        s.insert_mesh(MeshId(2), &demo::box_mesh([1.0; 3])).expect("ok");
        s.set_face_highlight(MeshId(1), 2, Highlight::Select);
        s.set_edge_highlight(MeshId(1), 0, Highlight::Preselect);
        let mut cam = Camera::default();
        cam.fit(&s.mesh(MeshId(1)).expect("m").world_bounds(), 1.0);
        // Move mesh 2 far to the side after fitting: it is culled.
        s.set_transform(MeshId(2), math::translation([0.0, 0.0, 1e4]));
        let plan = FramePlan::build(&s, &cam, [100, 100], &ViewOptions::default());
        // Bounds now include mesh 2, but the camera was fitted to mesh 1 only.
        assert_eq!(plan.draws.len(), 1);
        let d = &plan.draws[0];
        assert_eq!(d.tri_runs.len(), 3);
        assert_eq!(d.tri_runs[1].0, 4..6);
        assert_eq!(d.edge_runs.len(), 2);
        let no_edges = FramePlan::build(&s, &cam, [100, 100], &ViewOptions { edges: false, ..ViewOptions::default() });
        assert!(no_edges.draws[0].edge_runs.is_empty());
    }

    #[test]
    fn edge_width_is_dpi_aware() {
        let o = ViewOptions { pixels_per_point: 2.0, ..ViewOptions::default() };
        assert!((o.edge_width_px() - 2.0).abs() < 1e-6);
        let o = ViewOptions { pixels_per_point: f32::NAN, ..ViewOptions::default() };
        assert!((o.edge_width_px() - 1.0).abs() < 1e-6);
    }
}
