//! CPU picking: ray casts against per-mesh triangle BVHs, and edge picking by
//! screen-space distance to the edge polylines. Display-only (ADR 0011 §3).

use crate::camera::{Camera, Ray};
use crate::math::{self, Vec3, add, dot, scale, sub};
use crate::scene::{MeshId, Scene};

/// A face hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickHit {
    /// Mesh hit.
    pub mesh: MeshId,
    /// B-rep face index (into the mesh's `face_ranges`).
    pub face: u32,
    /// Triangle index.
    pub triangle: u32,
    /// World distance from the ray origin.
    pub distance: f64,
    /// World hit point.
    pub point: Vec3,
}

/// An edge hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeHit {
    /// Mesh hit.
    pub mesh: MeshId,
    /// Topological edge index (into the mesh's `edges`).
    pub edge: u32,
    /// Segment index within the mesh's flattened edge segments.
    pub segment: u32,
    /// Screen distance from the cursor in pixels.
    pub distance_px: f64,
    /// Closest world point on the edge (approximate under perspective).
    pub point: Vec3,
}

impl Scene {
    /// Closest visible face along `ray`.
    #[must_use]
    pub fn pick(&self, ray: &Ray) -> Option<PickHit> {
        let mut best: Option<PickHit> = None;
        for (id, m) in self.meshes() {
            if !m.visible() {
                continue;
            }
            let Some(inv) = math::invert(m.transform()) else { continue };
            let o = math::transform_point(&inv, ray.origin).map(|c| c as f32);
            let d = math::transform_dir(&inv, ray.dir).map(|c| c as f32);
            let limit = best.map_or(f32::INFINITY, |b| b.distance as f32 * 1.0001);
            let data = m.data();
            let Some(hit) = data.bvh.raycast(data, o, d, limit) else { continue };
            // Affine maps preserve the ray parameter; `ray.dir` is unit length.
            let t = f64::from(hit.t);
            if best.is_none_or(|b| t < b.distance) {
                let face = data.face_of_triangle(hit.triangle).unwrap_or(0);
                best = Some(PickHit {
                    mesh: id,
                    face,
                    triangle: hit.triangle,
                    distance: t,
                    point: add(ray.origin, scale(ray.dir, t)),
                });
            }
        }
        best
    }

    /// The edge closest to `pixel` (top-left origin, same units as `viewport`) within
    /// `max_px`, ignoring edges hidden behind faces.
    #[must_use]
    pub fn pick_edge(&self, camera: &Camera, viewport: [f64; 2], pixel: [f64; 2], max_px: f64) -> Option<EdgeHit> {
        let bounds = self.bounds();
        let cm = camera.matrices(viewport[0] / viewport[1].max(1.0), &bounds);
        let ray = camera.ray(pixel, viewport);
        let face_t = self.pick(&ray).map(|h| h.distance);
        let tol = face_t.map_or(0.0, |t| t * 2e-3) + camera.pixel_size_at_target(viewport[1]) * 2.0;
        let mut best: Option<(EdgeHit, f64)> = None;
        for (id, m) in self.meshes() {
            if !m.visible() {
                continue;
            }
            let data = m.data();
            for (edge, range) in data.edge_ranges.iter().enumerate() {
                for seg in range.clone() {
                    let i = seg as usize * 2;
                    let (Some(a), Some(b)) = (data.edge_points.get(i), data.edge_points.get(i + 1)) else {
                        continue;
                    };
                    let wa = math::transform_point(m.transform(), a.map(f64::from));
                    let wb = math::transform_point(m.transform(), b.map(f64::from));
                    let (Some((pa, _)), Some((pb, _))) =
                        (camera.project(&cm, wa, viewport), camera.project(&cm, wb, viewport))
                    else {
                        continue;
                    };
                    let (dist, s) = point_segment_2d(pixel, pa, pb);
                    if dist > max_px {
                        continue;
                    }
                    let point = add(wa, scale(sub(wb, wa), s));
                    let along = dot(sub(point, ray.origin), ray.dir);
                    if face_t.is_some_and(|t| along > t + tol) {
                        continue;
                    }
                    let better =
                        best.is_none_or(|(h, d)| dist < h.distance_px - 1e-9 || (dist <= h.distance_px && along < d));
                    if better {
                        let hit = EdgeHit { mesh: id, edge: edge as u32, segment: seg, distance_px: dist, point };
                        best = Some((hit, along));
                    }
                }
            }
        }
        best.map(|(h, _)| h)
    }
}

fn point_segment_2d(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let s = if l2 > 1e-18 { ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + ab[0] * s - p[0], a[1] + ab[1] * s - p[1]];
    ((q[0] * q[0] + q[1] * q[1]).sqrt(), s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::NamedView;
    use crate::demo;

    fn scene() -> Scene {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(1), &demo::box_mesh([10.0; 3])).expect("ok");
        s.insert_mesh(MeshId(2), &demo::box_mesh([10.0; 3])).expect("ok");
        s.set_transform(MeshId(2), math::translation([0.0, 30.0, 0.0]));
        s
    }

    #[test]
    fn picks_front_face_of_nearest_box() {
        let s = scene();
        // Looking along +Y from y = -100: first box's front face (-Y, face 2) at y = -5.
        let ray = Ray { origin: [1.0, -100.0, 2.0], dir: [0.0, 1.0, 0.0] };
        let h = s.pick(&ray).expect("hit");
        assert_eq!(h.mesh, MeshId(1));
        assert_eq!(h.face, 2);
        assert!((h.distance - 95.0).abs() < 1e-4);
        assert!((h.point[1] + 5.0).abs() < 1e-4);
        // From behind (+Y side) the translated box is hit first, on its back face.
        let ray = Ray { origin: [1.0, 100.0, 2.0], dir: [0.0, -1.0, 0.0] };
        let h = s.pick(&ray).expect("hit");
        assert_eq!((h.mesh, h.face), (MeshId(2), 3));
        // Hidden meshes are ignored.
        let mut s2 = s.clone();
        s2.set_visible(MeshId(2), false);
        assert_eq!(s2.pick(&ray).map(|h| h.mesh), Some(MeshId(1)));
        assert!(s.pick(&Ray { origin: [50.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0] }).is_none());
    }

    #[test]
    fn camera_ray_pick_and_edge_pick() {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(1), &demo::box_mesh([10.0; 3])).expect("ok");
        let mut cam = Camera::default();
        cam.set_view(NamedView::Front);
        cam.fit(&s.bounds(), 1.0);
        let vp = [400.0, 400.0];
        let h = s.pick(&cam.ray([200.0, 200.0], vp)).expect("hit");
        assert_eq!(h.face, 2);
        // Project the top-front edge (z = 5, y = -5) and click 2 px below it.
        let cm = cam.matrices(1.0, &s.bounds());
        let (p, _) = cam.project(&cm, [0.0, -5.0, 5.0], vp).expect("visible");
        let e = s.pick_edge(&cam, vp, [p[0], p[1] + 2.0], 5.0).expect("edge");
        assert!((e.distance_px - 2.0).abs() < 1e-6, "{e:?}");
        assert!((e.point[2] - 5.0).abs() < 1e-6 && (e.point[1] + 5.0).abs() < 1e-6, "{e:?}");
        assert!(s.pick_edge(&cam, vp, [200.0, 200.0], 5.0).is_none());
        // The back-top edge (y = +5) projects onto the same pixels from the front but is
        // hidden by the box, so the front edge wins.
        let mid = s.mesh(MeshId(1)).expect("mesh").data();
        let seg = e.segment as usize * 2;
        assert!((mid.edge_points[seg][1] + 5.0).abs() < 1e-6);
    }
}
