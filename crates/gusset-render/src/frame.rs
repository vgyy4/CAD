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
    /// Draw shaded faces; off is the wireframe style (edges only, so hidden ones show too).
    pub faces: bool,
    /// Edge width in logical pixels (DESIGN.md §5: 1 px).
    pub edge_width: f32,
    /// The lighting rig.
    pub lighting: Lighting,
    /// Section view: the part of the model on the normal's side of this plane is hidden
    /// and the cut shows as a flat face (DESIGN.md §2 Inspect: section).
    pub section: Option<Section>,
}

/// A world-space section plane. Geometry on the side the normal points to is removed, so a
/// camera on that side looks at the cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    /// A point on the plane.
    pub origin: Vec3,
    /// The plane normal, towards the removed side (need not be unit length).
    pub normal: Vec3,
}

impl Section {
    /// Whether a world point is kept (on or behind the plane).
    #[must_use]
    pub fn keeps(&self, p: Vec3) -> bool {
        math::dot(math::sub(p, self.origin), self.normal) <= 0.0
    }
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            theme: Theme::Dark,
            pixels_per_point: 1.0,
            edges: true,
            faces: true,
            edge_width: 1.0,
            lighting: Lighting::DEFAULT,
            section: None,
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
    /// The mesh is watertight: its back faces seen through a section are the cut face.
    pub closed: bool,
    /// Finish code for shading (0 plastic, 1 metal, 2 glass, 3 rubber).
    pub finish: u32,
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
    /// View-space section plane `[nx, ny, nz, w]`: points with `n.p + w >= 0` are kept;
    /// back faces seen through the cut are drawn flat with normal `-n` (the cut face).
    pub clip: Option<[f32; 4]>,
    /// Tint of the cut face (the theme's pre-selection blue, linear), mixed by `CAP_MIX`.
    pub cap: [f32; 3],
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
            // Level of detail by screen size (spec: large assemblies): a body smaller than
            // a pixel or two is not drawn, and one under a few pixels without its edges.
            let px = screen_px(&viewproj, m, size);
            if px < TINY_PX {
                continue;
            }
            let edges_too = px >= EDGES_PX;
            let data = m.data();
            let model_view = math::mul(&cm.view, m.transform());
            let base = m.material().color.linear();
            let tri_runs = if opts.faces {
                SceneMesh::runs(&data.face_ranges, &m.face_highlights, data.triangle_count() as u32)
                    .into_iter()
                    .map(|(r, h)| (r, hl(h, base)))
                    .collect()
            } else {
                Vec::new()
            };
            let edge_runs = if opts.edges && edges_too {
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
                closed: data.closed,
                finish: match m.material().finish {
                    crate::Finish::Plastic => 0,
                    crate::Finish::Metal => 1,
                    crate::Finish::Glass => 2,
                    crate::Finish::Rubber => 3,
                },
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
            cap: colors.preselect.linear(),
            clip: opts.section.and_then(|sec| view_plane(&cm.view, &sec)),
            draws,
        }
    }
}

/// The section plane in view space, kept side positive; `None` for a degenerate normal.
fn view_plane(view: &Mat4, sec: &Section) -> Option<[f32; 4]> {
    let len = math::length(sec.normal);
    if !(len.is_finite() && len > 1e-12) || !sec.origin.iter().all(|c| c.is_finite()) {
        return None;
    }
    let n = math::transform_dir(view, math::scale(sec.normal, -1.0 / len));
    let o = math::transform_point(view, sec.origin);
    Some([n[0] as f32, n[1] as f32, n[2] as f32, -math::dot(n, o) as f32])
}

/// Whether a mesh's world box is entirely outside one clip plane.
/// Below this many pixels across a body is not drawn; below `EDGES_PX` its edges are not.
const TINY_PX: f64 = 1.5;
const EDGES_PX: f64 = 6.0;

/// The larger side, in pixels, of the screen box of `m`'s bounds; infinite when the box
/// reaches behind the eye.
fn screen_px(viewproj: &Mat4, m: &SceneMesh, size: [u32; 2]) -> f64 {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for c in m.world.corners() {
        let p = math::transform4(viewproj, c);
        if p[3] <= 1e-9 {
            return f64::INFINITY;
        }
        for k in 0..2 {
            let v = p.get(k).copied().unwrap_or(0.0) / p[3];
            if let (Some(l), Some(h)) = (lo.get_mut(k), hi.get_mut(k)) {
                *l = l.min(v);
                *h = h.max(v);
            }
        }
    }
    let w = (hi[0] - lo[0]) * 0.5 * f64::from(size[0]);
    let h = (hi[1] - lo[1]) * 0.5 * f64::from(size[1]);
    w.max(h)
}

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
    fn bodies_of_a_pixel_are_skipped_and_small_ones_lose_their_edges() {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(1), &demo::box_mesh([100.0; 3])).expect("ok");
        s.insert_mesh(MeshId(2), &demo::box_mesh([0.05; 3])).expect("ok"); // well under a pixel
        s.insert_mesh(MeshId(3), &demo::box_mesh([0.8; 3])).expect("ok"); // a few pixels
        let mut cam = Camera::default();
        cam.fit(&s.bounds(), 1.0);
        let plan = FramePlan::build(&s, &cam, [400, 400], &ViewOptions::default());
        let ids: Vec<u64> = plan.draws.iter().map(|d| d.id.0).collect();
        assert_eq!(ids, [1, 3], "{ids:?}");
        assert!(plan.draws[1].edge_runs.is_empty() && !plan.draws[0].edge_runs.is_empty());
    }

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
