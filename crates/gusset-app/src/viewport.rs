//! Interim CPU viewport (ADR 0009 fallback path): tessellated bodies are projected and
//! drawn through egui's painter, depth-sorted per triangle. Works with no GPU at all;
//! the wgpu renderer (gusset-render) replaces it where a GPU is available.
//!
//! Orbit: middle/right drag. Pan: Shift + drag. Zoom: scroll (to cursor). Pick: click.

use crate::theme::Palette;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2, epaint::Vertex};
use gusset_kernel::Mesh;

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 1e-12 { scale(a, 1.0 / l) } else { [0.0, 0.0, 1.0] }
}

/// Orthographic orbit camera (Z up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: V3,
    /// Radians around Z.
    pub yaw: f32,
    /// Radians above the XY plane.
    pub pitch: f32,
    /// Visible height of the view in mm.
    pub height: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { target: [0.0; 3], yaw: -0.6, pitch: 0.55, height: 120.0 }
    }
}

impl Camera {
    /// (right, up, toward-viewer) basis.
    pub fn basis(&self) -> (V3, V3, V3) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let back = [cp * cy, cp * sy, sp];
        let right = norm(cross([0.0, 0.0, 1.0], back));
        let right = if dot(right, right) < 0.5 { [1.0, 0.0, 0.0] } else { right };
        let up = cross(back, right);
        (right, up, back)
    }

    /// World → screen position and depth (larger = closer to the viewer).
    pub fn project(&self, rect: Rect, p: V3) -> (Pos2, f32) {
        let (r, u, b) = self.basis();
        let d = sub(p, self.target);
        let s = rect.height().max(1.0) / self.height;
        (rect.center() + Vec2::new(dot(d, r) * s, -dot(d, u) * s), dot(d, b))
    }

    /// Screen → world point on the view plane through the target.
    pub fn unproject(&self, rect: Rect, p: Pos2) -> V3 {
        let (r, u, _) = self.basis();
        let s = self.height / rect.height().max(1.0);
        let d = p - rect.center();
        add(self.target, add(scale(r, d.x * s), scale(u, -d.y * s)))
    }

    /// Fits the camera to a bounding box.
    pub fn fit(&mut self, min: V3, max: V3) {
        self.target = scale(add(min, max), 0.5);
        let diag = dot(sub(max, min), sub(max, min)).sqrt();
        self.height = (diag * 1.25).max(10.0);
    }
}

/// One drawable body.
#[derive(Debug, Clone)]
pub struct DrawBody {
    pub name: String,
    pub mesh: Mesh,
    pub face_names: Vec<String>,
    pub edge_names: Vec<String>,
}

/// A sketch drawn as polylines in 3D (already on its plane).
#[derive(Debug, Clone, Default)]
pub struct DrawSketch {
    pub name: String,
    pub polylines: Vec<Vec<V3>>,
    pub points: Vec<V3>,
}

/// What the cursor is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Face { body: String, index: u32, name: String },
    Edge { body: String, index: u32, name: String },
}

impl Hit {
    pub fn label(&self) -> String {
        match self {
            Hit::Face { body, name, .. } | Hit::Edge { body, name, .. } => format!("{body}: {name}"),
        }
    }
}

/// Result of one viewport frame.
#[derive(Debug, Default)]
pub struct Output {
    pub hover: Option<Hit>,
    pub clicked: Option<Option<Hit>>,
    /// Cursor position on the view plane (for the status bar).
    pub cursor_world: Option<V3>,
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Distance from `p` to segment `a`–`b` in screen space.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

/// Barycentric point-in-triangle test; returns the interpolated depth.
fn tri_hit(p: Pos2, a: (Pos2, f32), b: (Pos2, f32), c: (Pos2, f32)) -> Option<f32> {
    let v0 = b.0 - a.0;
    let v1 = c.0 - a.0;
    let v2 = p - a.0;
    let den = v0.x * v1.y - v1.x * v0.y;
    if den.abs() < 1e-9 {
        return None;
    }
    let v = (v2.x * v1.y - v1.x * v2.y) / den;
    let w = (v0.x * v2.y - v2.x * v0.y) / den;
    let u = 1.0 - v - w;
    (u >= 0.0 && v >= 0.0 && w >= 0.0).then_some(u * a.1 + v * b.1 + w * c.1)
}

/// Finds the face or edge under `p`. Edges win within 5 px if not hidden by a face.
pub fn pick(cam: &Camera, rect: Rect, bodies: &[DrawBody], p: Pos2) -> Option<Hit> {
    let mut best_face: Option<(f32, Hit)> = None;
    for b in bodies {
        let pr: Vec<(Pos2, f32)> = b.mesh.positions.iter().map(|&v| cam.project(rect, v)).collect();
        for (fi, range) in b.mesh.face_ranges.iter().enumerate() {
            let idx = b.mesh.indices.get(range.start as usize..range.end as usize).unwrap_or_default();
            for &[t0, t1, t2] in idx.as_chunks::<3>().0 {
                let (Some(&a), Some(&bb), Some(&c)) = (pr.get(t0 as usize), pr.get(t1 as usize), pr.get(t2 as usize))
                else {
                    continue;
                };
                if let Some(d) = tri_hit(p, a, bb, c)
                    && best_face.as_ref().is_none_or(|(bd, _)| d > *bd)
                {
                    let index = u32::try_from(fi).unwrap_or(u32::MAX);
                    let name = b.face_names.get(fi).cloned().unwrap_or_else(|| format!("face {fi}"));
                    best_face = Some((d, Hit::Face { body: b.name.clone(), index, name }));
                }
            }
        }
    }
    let front = best_face.as_ref().map_or(f32::NEG_INFINITY, |(d, _)| *d);
    let mut best_edge: Option<(f32, Hit)> = None;
    for b in bodies {
        for (ei, pl) in b.mesh.edges.iter().enumerate() {
            for w in pl.windows(2) {
                let [a, c] = w else { continue };
                let (pa, da) = cam.project(rect, *a);
                let (pc, dc) = cam.project(rect, *c);
                let dist = seg_dist(p, pa, pc);
                // An edge on the front face has the same depth; allow a small tolerance.
                let tol = cam.height * 0.01;
                if dist < 5.0 && da.max(dc) + tol >= front && best_edge.as_ref().is_none_or(|(bd, _)| dist < *bd) {
                    let index = u32::try_from(ei).unwrap_or(u32::MAX);
                    let name = b.edge_names.get(ei).cloned().unwrap_or_else(|| format!("edge {ei}"));
                    best_edge = Some((dist, Hit::Edge { body: b.name.clone(), index, name }));
                }
            }
        }
    }
    best_edge.map(|(_, h)| h).or(best_face.map(|(_, h)| h))
}

/// Paints the scene and handles camera input.
#[allow(clippy::too_many_lines)]
pub fn show(
    ui: &mut egui::Ui,
    cam: &mut Camera,
    pal: &Palette,
    bodies: &[DrawBody],
    sketches: &[DrawSketch],
    selection: &[Hit],
) -> Output {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let mut out = Output::default();

    // Camera input.
    let shift = ui.input(|i| i.modifiers.shift);
    if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
        let d = resp.drag_delta();
        if shift {
            let s = cam.height / rect.height().max(1.0);
            let (r, u, _) = cam.basis();
            cam.target = add(cam.target, add(scale(r, -d.x * s), scale(u, d.y * s)));
        } else {
            cam.yaw -= d.x * 0.008;
            cam.pitch = (cam.pitch + d.y * 0.008).clamp(-1.55, 1.55);
        }
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let before = resp.hover_pos().map(|p| cam.unproject(rect, p));
            cam.height = (cam.height * (-scroll * 0.002).exp()).clamp(0.01, 1e6);
            if let (Some(b), Some(p)) = (before, resp.hover_pos()) {
                let after = cam.unproject(rect, p);
                cam.target = add(cam.target, sub(b, after));
            }
        }
    }

    // Background gradient.
    let mut bg = egui::Mesh::default();
    bg.colored_vertex(rect.left_top(), pal.vp_bg_top);
    bg.colored_vertex(rect.right_top(), pal.vp_bg_top);
    bg.colored_vertex(rect.right_bottom(), pal.vp_bg_bottom);
    bg.colored_vertex(rect.left_bottom(), pal.vp_bg_bottom);
    bg.add_triangle(0, 1, 2);
    bg.add_triangle(0, 2, 3);
    painter.add(bg);

    // Ground grid on XY, 10 mm spacing adapted to zoom.
    let step = 10f32.powf((cam.height / 10.0).log10().floor()).max(0.1);
    let n = 20i32;
    let grid = Color32::from_rgba_unmultiplied(pal.text_muted.r(), pal.text_muted.g(), pal.text_muted.b(), 28);
    let c = [(cam.target[0] / step).round() * step, (cam.target[1] / step).round() * step];
    for i in -n..=n {
        let o = i as f32 * step;
        let ext = n as f32 * step;
        for (a, b) in [
            ([c[0] + o, c[1] - ext, 0.0], [c[0] + o, c[1] + ext, 0.0]),
            ([c[0] - ext, c[1] + o, 0.0], [c[0] + ext, c[1] + o, 0.0]),
        ] {
            painter.line_segment([cam.project(rect, a).0, cam.project(rect, b).0], Stroke::new(1.0, grid));
        }
    }

    // Faces: collect all triangles, sort back to front, shade by normal.
    let hover = resp.hover_pos().and_then(|p| pick(cam, rect, bodies, p));
    let (_, _, view) = cam.basis();
    let light = norm(add(view, [0.3, 0.2, 0.6]));
    let base = lerp(pal.bg_raised, pal.text_muted, 0.55);
    let mut tris: Vec<(f32, [Pos2; 3], Color32)> = Vec::new();
    for b in bodies {
        let pr: Vec<(Pos2, f32)> = b.mesh.positions.iter().map(|&v| cam.project(rect, v)).collect();
        for (fi, range) in b.mesh.face_ranges.iter().enumerate() {
            let fi32 = u32::try_from(fi).unwrap_or(u32::MAX);
            let is = |h: &Hit| matches!(h, Hit::Face { body, index, .. } if *body == b.name && *index == fi32);
            let tint = if selection.iter().any(is) {
                Some(pal.vp_select)
            } else if hover.as_ref().is_some_and(is) {
                Some(pal.vp_preselect)
            } else {
                None
            };
            let idx = b.mesh.indices.get(range.start as usize..range.end as usize).unwrap_or_default();
            for &[t0, t1, t2] in idx.as_chunks::<3>().0 {
                let get = |i: u32| b.mesh.positions.get(i as usize).copied();
                let (Some(a), Some(bb), Some(cc)) = (get(t0), get(t1), get(t2)) else { continue };
                let (Some(pa), Some(pb), Some(pc)) = (pr.get(t0 as usize), pr.get(t1 as usize), pr.get(t2 as usize))
                else {
                    continue;
                };
                let nrm = norm(cross(sub(bb, a), sub(cc, a)));
                let lit = 0.35 + 0.65 * dot(nrm, light).abs();
                let col = lerp(Color32::BLACK, tint.map_or(base, |t| lerp(base, t, 0.6)), lit);
                tris.push(((pa.1 + pb.1 + pc.1) / 3.0, [pa.0, pb.0, pc.0], col));
            }
        }
    }
    tris.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut m = egui::Mesh::default();
    for (_, p, col) in &tris {
        let i = u32::try_from(m.vertices.len()).unwrap_or(u32::MAX);
        for q in p {
            m.vertices.push(Vertex { pos: *q, uv: egui::epaint::WHITE_UV, color: *col });
        }
        m.add_triangle(i, i + 1, i + 2);
        if m.vertices.len() > 60_000 {
            painter.add(std::mem::take(&mut m));
        }
    }
    painter.add(m);

    // Edges on top (silhouette quality is good enough for the interim renderer).
    for b in bodies {
        for (ei, pl) in b.mesh.edges.iter().enumerate() {
            let ei32 = u32::try_from(ei).unwrap_or(u32::MAX);
            let is = |h: &Hit| matches!(h, Hit::Edge { body, index, .. } if *body == b.name && *index == ei32);
            let stroke = if selection.iter().any(is) {
                Stroke::new(3.0, pal.vp_select)
            } else if hover.as_ref().is_some_and(is) {
                Stroke::new(3.0, pal.vp_preselect)
            } else {
                Stroke::new(1.2, pal.vp_edge)
            };
            let pts: Vec<Pos2> = pl.iter().map(|&v| cam.project(rect, v).0).collect();
            painter.add(egui::Shape::line(pts, stroke));
        }
    }

    // Sketches.
    for s in sketches {
        for pl in &s.polylines {
            let pts: Vec<Pos2> = pl.iter().map(|&v| cam.project(rect, v).0).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(1.6, pal.accent)));
        }
        for &p in &s.points {
            painter.circle_filled(cam.project(rect, p).0, 2.5, pal.accent);
        }
    }

    // Axis triad (bottom-left), colored by convention with labels for color-blind users.
    let origin = rect.left_bottom() + Vec2::new(36.0, -36.0);
    let (r, u, _) = cam.basis();
    for (axis, col, name) in
        [([1.0, 0.0, 0.0], pal.danger, "X"), ([0.0, 1.0, 0.0], pal.success, "Y"), ([0.0, 0.0, 1.0], pal.accent, "Z")]
    {
        let tip = origin + Vec2::new(dot(axis, r), -dot(axis, u)) * 24.0;
        painter.line_segment([origin, tip], Stroke::new(2.0, col));
        painter.text(tip, egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(11.0), pal.text);
    }

    if let Some(h) = &hover {
        resp.clone().on_hover_text_at_pointer(h.label());
    }
    out.cursor_world = resp.hover_pos().map(|p| cam.unproject(rect, p));
    if resp.clicked() {
        out.clicked = Some(hover.clone());
    }
    out.hover = hover;
    out
}

/// Bounding box of all bodies and sketches.
pub fn bounds(bodies: &[DrawBody], sketches: &[DrawSketch]) -> Option<(V3, V3)> {
    let pts = bodies
        .iter()
        .flat_map(|b| b.mesh.positions.iter().copied())
        .chain(sketches.iter().flat_map(|s| s.polylines.iter().flatten().copied().chain(s.points.iter().copied())));
    let mut it = pts.peekable();
    let first = *it.peek()?;
    Some(it.fold((first, first), |(lo, hi), p| {
        ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> DrawBody {
        let mesh = Mesh {
            positions: vec![[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [-10.0, 10.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            face_ranges: std::iter::once(0..6).collect(),
            edges: vec![vec![[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0]]],
        };
        DrawBody { name: "B".into(), mesh, face_names: vec!["top".into()], edge_names: vec!["e0".into()] }
    }

    #[test]
    fn projection_roundtrips_and_picks() {
        let cam = Camera { target: [0.0; 3], yaw: 0.0, pitch: 1.5, height: 100.0 };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 400.0));
        let (c, _) = cam.project(rect, [0.0; 3]);
        assert!((c - rect.center()).length() < 1e-3);
        let w = cam.unproject(rect, Pos2::new(250.0, 180.0));
        let (back, _) = cam.project(rect, w);
        assert!((back - Pos2::new(250.0, 180.0)).length() < 1e-2);
        let bodies = [quad()];
        assert!(matches!(pick(&cam, rect, &bodies, rect.center()), Some(Hit::Face { ref name, .. }) if name == "top"));
        assert_eq!(pick(&cam, rect, &bodies, Pos2::new(5.0, 5.0)), None);
        let (edge_mid, _) = cam.project(rect, [0.0, -10.0, 0.0]);
        assert!(matches!(pick(&cam, rect, &bodies, edge_mid), Some(Hit::Edge { .. })));
        let (lo, hi) = bounds(&bodies, &[]).unwrap();
        assert_eq!((lo[0], hi[1]), (-10.0, 10.0));
    }
}
