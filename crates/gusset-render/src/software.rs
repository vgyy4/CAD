//! Last-resort CPU rasterizer: used only when no wgpu adapter at all can be created, so
//! `render_view` always returns an image. Same frame plan, colors and lighting as the
//! GPU path (per-pixel shading, z-buffer, near-plane clipping, edges as lines), without
//! anti-aliasing. Single-threaded and simple by design.

use crate::camera::Camera;
use crate::frame::{FramePlan, ViewOptions};
use crate::headless::RenderError;
use crate::image::RgbaImage;
use crate::math::{self, Mat4, zip_with};
use crate::mesh::unpack_normal;
use crate::scene::Scene;
use crate::theme::linear_to_srgb;

/// Largest image side the CPU renderer accepts.
pub const SOFTWARE_MAX_SIDE: u32 = 8192;

/// The CPU fallback renderer.
#[derive(Debug, Clone, Default)]
pub struct SoftwareRenderer {
    options: ViewOptions,
}

/// A clip-space vertex with the attributes we interpolate.
#[derive(Debug, Clone, Copy)]
struct ClipVert {
    clip: [f32; 4],
    view: [f32; 3],
    normal: [f32; 3],
}

impl ClipVert {
    fn lerp(a: &ClipVert, b: &ClipVert, t: f32) -> ClipVert {
        let l = |x: f32, y: f32| x + (y - x) * t;
        ClipVert {
            clip: zip_with(a.clip, b.clip, l),
            view: zip_with(a.view, b.view, l),
            normal: zip_with(a.normal, b.normal, l),
        }
    }
}

struct Target {
    w: usize,
    h: usize,
    color: Vec<[f32; 3]>,
    depth: Vec<f32>,
}

impl Target {
    fn idx(&self, x: i64, y: i64) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h).then(|| y as usize * self.w + x as usize)
    }

    /// Depth-tested write; `less_equal` for edges.
    fn plot(&mut self, i: usize, z: f32, c: [f32; 3], less_equal: bool) {
        if let (Some(d), Some(px)) = (self.depth.get_mut(i), self.color.get_mut(i))
            && (0.0..=1.0).contains(&z)
            && (z < *d || (less_equal && z <= *d))
        {
            *d = z;
            *px = c;
        }
    }
}

impl SoftwareRenderer {
    /// A renderer with default options.
    #[must_use]
    pub fn new() -> SoftwareRenderer {
        SoftwareRenderer::default()
    }

    /// View options.
    #[must_use]
    pub fn options(&self) -> &ViewOptions {
        &self.options
    }

    /// Replace the view options.
    pub fn set_options(&mut self, options: ViewOptions) {
        self.options = options;
    }

    /// Render to an RGBA8 sRGB image.
    ///
    /// # Errors
    /// [`RenderError::InvalidSize`] for a zero or oversized image.
    pub fn render_rgba(&self, scene: &Scene, camera: &Camera, size: [u32; 2]) -> Result<RgbaImage, RenderError> {
        if size[0] == 0 || size[1] == 0 || size[0] > SOFTWARE_MAX_SIDE || size[1] > SOFTWARE_MAX_SIDE {
            return Err(RenderError::InvalidSize { size, max: SOFTWARE_MAX_SIDE });
        }
        let plan = FramePlan::build(scene, camera, size, &self.options);
        let (w, h) = (size[0] as usize, size[1] as usize);
        let mut t = Target { w, h, color: Vec::with_capacity(w * h), depth: vec![1.0; w * h] };
        for y in 0..h {
            // Match the GPU: the gradient parameter at the pixel center.
            let s = (y as f32 + 0.5) / h as f32;
            let c = zip_with(plan.bg_top, plan.bg_bottom, |t, b| t + (b - t) * s);
            t.color.extend(std::iter::repeat_n(c, w));
        }
        let proj = &plan.proj;
        for d in &plan.draws {
            let Some(m) = scene.mesh(d.id) else { continue };
            let data = m.data();
            let verts: Vec<ClipVert> = data
                .positions
                .iter()
                .zip(&data.normals)
                .map(|(p, n)| {
                    let vp = math::transform_point(&d.model_view, p.map(f64::from));
                    let un = unpack_normal(*n).map(f64::from);
                    let vn = math::add(
                        math::add(math::scale(d.normal[0], un[0]), math::scale(d.normal[1], un[1])),
                        math::scale(d.normal[2], un[2]),
                    );
                    ClipVert {
                        clip: math::transform4(proj, vp).map(|c| c as f32),
                        view: vp.map(|c| c as f32),
                        normal: vn.map(|c| c as f32),
                    }
                })
                .collect();
            for (run, base) in &d.tri_runs {
                for tri in run.clone() {
                    let i = tri as usize * 3;
                    let Some(idx) = data.indices.get(i..i + 3) else { continue };
                    let vs: Vec<ClipVert> = idx.iter().filter_map(|&k| verts.get(k as usize).copied()).collect();
                    if let [a, b, c] = vs.as_slice() {
                        let poly = clip_near(&[*a, *b, *c]);
                        for k in 1..poly.len().saturating_sub(1) {
                            if let (Some(p0), Some(p1), Some(p2)) = (poly.first(), poly.get(k), poly.get(k + 1)) {
                                raster_triangle(&mut t, [p0, p1, p2], *base, &plan);
                            }
                        }
                    }
                }
            }
            for (run, color) in &d.edge_runs {
                for seg in run.clone() {
                    let i = seg as usize * 2;
                    let (Some(a), Some(b)) = (data.edge_points.get(i), data.edge_points.get(i + 1)) else { continue };
                    let pa = pulled(&d.model_view, *a, &plan);
                    let pb = pulled(&d.model_view, *b, &plan);
                    raster_line(
                        &mut t,
                        math::transform4(proj, pa),
                        math::transform4(proj, pb),
                        *color,
                        plan.edge_width_px,
                    );
                }
            }
        }
        let mut data = Vec::with_capacity(w * h * 4);
        for c in &t.color {
            for k in c {
                data.push((linear_to_srgb(*k) * 255.0).round() as u8);
            }
            data.push(255);
        }
        Ok(RgbaImage { width: size[0], height: size[1], data })
    }

    /// Render to PNG bytes.
    ///
    /// # Errors
    /// See [`SoftwareRenderer::render_rgba`].
    pub fn render_png(&self, scene: &Scene, camera: &Camera, size: [u32; 2]) -> Result<Vec<u8>, RenderError> {
        Ok(self.render_rgba(scene, camera, size)?.to_png()?)
    }
}

/// Edge vertex in view space, pulled towards the eye like `pull()` in the shader.
fn pulled(model_view: &Mat4, p: [f32; 3], plan: &FramePlan) -> math::Vec3 {
    let v = math::transform_point(model_view, p.map(f64::from));
    if plan.ortho {
        [v[0], v[1], v[2] + f64::from(plan.edge_bias_ortho)]
    } else {
        math::scale(v, 1.0 - f64::from(plan.edge_bias_persp))
    }
}

/// Clip a polygon against the near plane (`z >= 0` in wgpu clip space).
fn clip_near(poly: &[ClipVert]) -> Vec<ClipVert> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    for (i, a) in poly.iter().enumerate() {
        let Some(b) = poly.get((i + 1) % poly.len()) else { continue };
        let (da, db) = (a.clip[2], b.clip[2]);
        if da >= 0.0 {
            out.push(*a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            out.push(ClipVert::lerp(a, b, da / (da - db)));
        }
    }
    out
}

fn to_screen(c: [f32; 4], w: usize, h: usize) -> Option<[f32; 4]> {
    if c[3] <= 1e-12 {
        return None;
    }
    let iw = 1.0 / c[3];
    Some([(c[0] * iw * 0.5 + 0.5) * w as f32, (0.5 - c[1] * iw * 0.5) * h as f32, c[2] * iw, iw])
}

fn raster_triangle(t: &mut Target, v: [&ClipVert; 3], base: [f32; 3], plan: &FramePlan) {
    let (Some(s0), Some(s1), Some(s2)) =
        (to_screen(v[0].clip, t.w, t.h), to_screen(v[1].clip, t.w, t.h), to_screen(v[2].clip, t.w, t.h))
    else {
        return;
    };
    let s = [s0, s1, s2];
    // Signed area in screen space (y down); CCW in NDC (front) is negative here.
    let area = (s1[0] - s0[0]) * (s2[1] - s0[1]) - (s2[0] - s0[0]) * (s1[1] - s0[1]);
    if area.abs() < 1e-12 || !area.is_finite() {
        return;
    }
    let front = area < 0.0;
    let minx = s.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64;
    let maxx = s.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(t.w as f32) as i64;
    let miny = s.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64;
    let maxy = s.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(t.h as f32) as i64;
    let edge = |a: [f32; 4], b: [f32; 4], x: f32, y: f32| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    let light = &plan.lighting;
    for y in miny..maxy {
        for x in minx..maxx {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = edge(s1, s2, px, py) / area;
            let w1 = edge(s2, s0, px, py) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * s0[2] + w1 * s1[2] + w2 * s2[2];
            let Some(i) = t.idx(x, y) else { continue };
            if t.depth.get(i).is_none_or(|d| z >= *d) {
                continue;
            }
            // Perspective-correct attributes.
            let q = [w0 * s0[3], w1 * s1[3], w2 * s2[3]];
            let qs = q[0] + q[1] + q[2];
            if qs.abs() < 1e-30 {
                continue;
            }
            let attr = |f: &dyn Fn(&ClipVert) -> [f32; 3]| {
                let (a, b, c) = (f(v[0]), f(v[1]), f(v[2]));
                zip_with(zip_with(a, b, |x, y| x * q[0] + y * q[1]), c, |xy, z| (xy + z * q[2]) / qs)
            };
            let n = attr(&|cv| cv.normal);
            let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let sign = if front { 1.0 } else { -1.0 };
            let n = n.map(|c| c / nl * sign);
            let vdir = if plan.ortho {
                [0.0, 0.0, 1.0]
            } else {
                let p = attr(&|cv| cv.view);
                let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1e-12);
                p.map(|c| -c / l)
            };
            t.plot(i, z, light.shade(base, n, vdir), false);
        }
    }
}

fn raster_line(t: &mut Target, a: [f64; 4], b: [f64; 4], color: [f32; 3], width_px: f32) {
    let (mut a, mut b) = (a.map(|c| c as f32), b.map(|c| c as f32));
    // Near-plane clip.
    if a[2] < 0.0 && b[2] < 0.0 {
        return;
    }
    if a[2] < 0.0 || b[2] < 0.0 {
        let s = a[2] / (a[2] - b[2]);
        let p = zip_with(a, b, |x, y| x + (y - x) * s);
        if a[2] < 0.0 {
            a = p;
        } else {
            b = p;
        }
    }
    let (Some(sa), Some(sb)) = (to_screen(a, t.w, t.h), to_screen(b, t.w, t.h)) else { return };
    let (dx, dy) = (sb[0] - sa[0], sb[1] - sa[1]);
    let steps = dx.abs().max(dy.abs()).ceil().clamp(1.0, 1e5) as i64;
    let r = ((width_px - 1.0) * 0.5).round().max(0.0) as i64;
    for k in 0..=steps {
        let f = k as f32 / steps as f32;
        let x = (sa[0] + dx * f).floor() as i64;
        let y = (sa[1] + dy * f).floor() as i64;
        let z = sa[2] + (sb[2] - sa[2]) * f;
        for oy in -r..=r {
            for ox in -r..=r {
                if let Some(i) = t.idx(x + ox, y + oy) {
                    t.plot(i, z, color, true);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use crate::scene::{Highlight, MeshId};
    use crate::theme::Theme;

    #[test]
    fn renders_background_part_and_highlight() {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(1), &demo::box_mesh([10.0; 3])).expect("ok");
        let mut cam = Camera::default();
        cam.set_view(crate::NamedView::Front);
        cam.fit(&s.bounds(), 1.0);
        let r = SoftwareRenderer::new();
        let img = r.render_rgba(&s, &cam, [64, 64]).expect("render");
        let bg = Theme::Dark.colors().bg_top.bytes();
        let corner = img.pixel(0, 0).expect("px");
        assert!((0..3).all(|k| corner[k].abs_diff(bg[k]) < 4), "{corner:?}");
        let center = img.pixel(32, 32).expect("px");
        assert!(center[0] > 60, "part should be lit: {center:?}");
        // Highlight the front face (2): the center turns amber (R > B).
        s.set_face_highlight(MeshId(1), 2, Highlight::Select);
        let img = r.render_rgba(&s, &cam, [64, 64]).expect("render");
        let c = img.pixel(32, 32).expect("px");
        assert!(c[0] > c[2] + 60, "{c:?}");
        assert!(r.render_rgba(&s, &cam, [0, 10]).is_err());
    }

    #[test]
    fn near_clipping_keeps_inside_part() {
        let poly = [
            ClipVert { clip: [0.0, 0.0, -1.0, 1.0], view: [0.0; 3], normal: [0.0; 3] },
            ClipVert { clip: [1.0, 0.0, 1.0, 1.0], view: [0.0; 3], normal: [0.0; 3] },
            ClipVert { clip: [0.0, 1.0, 1.0, 1.0], view: [0.0; 3], normal: [0.0; 3] },
        ];
        let out = clip_near(&poly);
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|v| v.clip[2] >= -1e-6));
    }
}
