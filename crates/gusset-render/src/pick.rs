//! CPU picking: ray casts against per-mesh triangle BVHs, and edge picking by
//! screen-space distance to the edge polylines. Display-only (ADR 0011 §3).

use crate::camera::{Camera, Ray};
use crate::frame::Section;
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
        self.pick_section(ray, None)
    }

    /// Closest visible face along `ray` in a section view: faces in the removed half are
    /// skipped, and a ray that meets the cut face first (a back face seen through the
    /// section) picks nothing.
    #[must_use]
    pub fn pick_section(&self, ray: &Ray, section: Option<&Section>) -> Option<PickHit> {
        let (hit, back) = self.pick_kept(ray, section)?;
        (!back || section.is_none()).then_some(hit)
    }

    /// Every face along `ray` (nearest first, at most `max`), for "select other": each hit
    /// is found by casting again just past the previous one.
    #[must_use]
    pub fn pick_all(&self, ray: &Ray, section: Option<&Section>, max: usize) -> Vec<PickHit> {
        let mut out: Vec<PickHit> = Vec::new();
        let mut skip = 0.0;
        let step = self.bounds().radius().max(1e-6) * 1e-6;
        while out.len() < max {
            let start = Ray { origin: add(ray.origin, scale(ray.dir, skip)), dir: ray.dir };
            let Some((mut h, _)) = self.pick_kept(&start, section) else { break };
            h.distance += skip;
            h.point = add(ray.origin, scale(ray.dir, h.distance));
            skip = h.distance + step;
            if !out.iter().any(|o| o.mesh == h.mesh && o.face == h.face) {
                out.push(h);
            }
        }
        out
    }

    /// Closest face in the kept part of the ray, and whether it is seen from behind.
    fn pick_kept(&self, ray: &Ray, section: Option<&Section>) -> Option<(PickHit, bool)> {
        let (t0, t1) = match section {
            Some(sec) => kept_interval(ray, sec)?,
            None => (0.0, f64::INFINITY),
        };
        let start = add(ray.origin, scale(ray.dir, t0));
        let mut best: Option<(PickHit, bool)> = None;
        // The meshes whose world boxes the ray meets, through the scene's top-level BVH;
        // each returns the distance (from `start`) still worth looking within.
        let far = if t1.is_finite() { t1 - t0 } else { f64::MAX };
        self.top().visit(start, ray.dir, far, |id| {
            self.pick_mesh(id, ray, start, (t0, t1), &mut best);
            best.map_or(far, |(b, _)| (b.distance - t0) * 1.0001)
        });
        best
    }

    /// Casts the ray against one mesh, keeping the closest hit in `best` (ties go to the
    /// lower mesh id, as when meshes were tested in id order).
    fn pick_mesh(&self, id: MeshId, ray: &Ray, start: Vec3, (t0, t1): (f64, f64), best: &mut Option<(PickHit, bool)>) {
        {
            let Some(m) = self.mesh(id) else { return };
            if !m.visible() {
                return;
            }
            let Some(inv) = m.inverse.as_ref() else { return };
            let inv = *inv;
            let o = math::transform_point(&inv, start).map(|c| c as f32);
            let d = math::transform_dir(&inv, ray.dir).map(|c| c as f32);
            let limit = best.map_or((t1 - t0) as f32, |(b, _)| (b.distance - t0) as f32 * 1.0001);
            let data = m.data();
            let Some(hit) = data.bvh.raycast(data, o, d, limit) else { return };
            // Affine maps preserve the ray parameter; `ray.dir` is unit length.
            let t = t0 + f64::from(hit.t);
            if t <= t1 && best.is_none_or(|(b, _)| t.total_cmp(&b.distance).then(id.cmp(&b.mesh)).is_lt()) {
                let face = data.face_of_triangle(hit.triangle).unwrap_or(0);
                let hit = PickHit {
                    mesh: id,
                    face,
                    triangle: hit.triangle,
                    distance: t,
                    point: add(ray.origin, scale(ray.dir, t)),
                };
                // Only a closed body has an inside: an open mesh's back faces are ordinary.
                *best = Some((hit, data.closed && back_facing(data, hit.triangle, d)));
            }
        }
    }

    /// The edge closest to `pixel` (top-left origin, same units as `viewport`) within
    /// `max_px`, ignoring edges hidden behind faces.
    #[must_use]
    pub fn pick_edge(&self, camera: &Camera, viewport: [f64; 2], pixel: [f64; 2], max_px: f64) -> Option<EdgeHit> {
        self.pick_edge_section(camera, viewport, pixel, max_px, None)
    }

    /// [`Scene::pick_edge`] in a section view: edge points in the removed half are not
    /// picked, and the cut face hides what lies behind it.
    #[must_use]
    pub fn pick_edge_section(
        &self,
        camera: &Camera,
        viewport: [f64; 2],
        pixel: [f64; 2],
        max_px: f64,
        section: Option<&Section>,
    ) -> Option<EdgeHit> {
        let bounds = self.bounds();
        let cm = camera.matrices(viewport[0] / viewport[1].max(1.0), &bounds);
        let ray = camera.ray(pixel, viewport);
        // A cut face lies on the section plane: it hides the edges behind it (review R7).
        let face_t = self.pick_kept(&ray, section).map(|(h, back)| match (back, section) {
            (true, Some(sec)) => kept_interval(&ray, sec).map_or(h.distance, |(t0, _)| t0),
            _ => h.distance,
        });
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
                    if section.is_some_and(|sec| !sec.keeps(point)) {
                        continue;
                    }
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

/// The ray parameters `[t0, t1]` inside the section's kept half-space, if any.
fn kept_interval(ray: &Ray, sec: &Section) -> Option<(f64, f64)> {
    let s0 = dot(sub(ray.origin, sec.origin), sec.normal);
    let sd = dot(ray.dir, sec.normal);
    if sd.abs() < 1e-300 {
        return (s0 <= 0.0).then_some((0.0, f64::INFINITY));
    }
    let t = -s0 / sd;
    if sd > 0.0 { (t >= 0.0).then_some((0.0, t)) } else { Some((t.max(0.0), f64::INFINITY)) }
}

/// Whether a local-space ray direction meets the triangle from behind.
fn back_facing(data: &crate::mesh::MeshData, triangle: u32, dir: [f32; 3]) -> bool {
    let i = triangle as usize * 3;
    let Some(&[a, b, c]) = data.indices.get(i..i + 3) else { return false };
    let p = |k: u32| data.positions.get(k as usize).map(|v| v.map(f64::from));
    let (Some(a), Some(b), Some(c)) = (p(a), p(b), p(c)) else { return false };
    dot(math::cross(sub(b, a), sub(c, a)), dir.map(f64::from)) > 0.0
}

fn point_segment_2d(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let s = if l2 > 1e-18 { ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + ab[0] * s - p[0], a[1] + ab[1] * s - p[1]];
    ((q[0] * q[0] + q[1] * q[1]).sqrt(), s)
}

/// Whether the ray from `o` along `d` meets `b` within `t_max` (slab test, padded a little).
pub(crate) fn ray_meets_box(o: Vec3, d: Vec3, b: &math::Aabb, t_max: f64) -> bool {
    if b.min[0] > b.max[0] {
        return false;
    }
    let pad = 1e-6 * (1.0 + b.max.iter().chain(&b.min).fold(0.0f64, |m, v| m.max(v.abs())));
    let (mut lo, mut hi) = (0.0f64, t_max);
    for (((oi, di), bmin), bmax) in o.into_iter().zip(d).zip(b.min).zip(b.max) {
        let (bmin, bmax) = (bmin - pad, bmax + pad);
        if di.abs() < 1e-300 {
            if oi < bmin || oi > bmax {
                return false;
            }
            continue;
        }
        let (a, c) = ((bmin - oi) / di, (bmax - oi) / di);
        let (near, far) = if a < c { (a, c) } else { (c, a) };
        lo = lo.max(near);
        hi = hi.min(far);
        if lo > hi {
            return false;
        }
    }
    true
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
    fn section_skips_the_removed_half_and_the_cut_face() {
        let s = scene();
        let ray = Ray { origin: [1.0, -100.0, 2.0], dir: [0.0, 1.0, 0.0] };
        // Remove y < 20 (normal towards the eye): the first box is gone, the second's
        // front face (y = 25) is hit.
        let sec = Section { origin: [0.0, 20.0, 0.0], normal: [0.0, -1.0, 0.0] };
        let h = s.pick_section(&ray, Some(&sec)).expect("hit");
        assert_eq!((h.mesh, h.face), (MeshId(2), 2));
        // Cut through the first box: the ray meets its inside (the cut face): no pick.
        let sec = Section { origin: [0.0, 0.0, 0.0], normal: [0.0, -1.0, 0.0] };
        assert_eq!(s.pick_section(&ray, Some(&sec)), None);
        // Normal away from the eye keeps the near half: the first box's front face.
        let sec = Section { origin: [0.0, 0.0, 0.0], normal: [0.0, 1.0, 0.0] };
        assert_eq!(s.pick_section(&ray, Some(&sec)).map(|h| (h.mesh, h.face)), Some((MeshId(1), 2)));
        // Everything removed.
        let sec = Section { origin: [0.0, 1e3, 0.0], normal: [0.0, -1.0, 0.0] };
        assert_eq!(s.pick_section(&ray, Some(&sec)), None);
    }

    #[test]
    fn pick_all_lists_every_face_along_the_ray() {
        let s = scene();
        // Looking along +Y through both boxes: front and back faces of each, nearest first.
        let ray = Ray { origin: [1.0, -100.0, 2.0], dir: [0.0, 1.0, 0.0] };
        let hits = s.pick_all(&ray, None, 16);
        let order: Vec<(u64, u32)> = hits.iter().map(|h| (h.mesh.0, h.face)).collect();
        assert_eq!(order, [(1, 2), (1, 3), (2, 2), (2, 3)], "{hits:?}");
        assert!(hits.windows(2).all(|w| w[0].distance < w[1].distance));
        assert_eq!(s.pick_all(&ray, None, 2).len(), 2);
    }

    #[test]
    fn edges_behind_the_cut_face_are_not_picked() {
        let mut s = Scene::new();
        s.insert_mesh(MeshId(1), &demo::box_mesh([10.0; 3])).expect("ok");
        let mut cam = Camera::default();
        cam.set_view(NamedView::Front);
        cam.fit(&s.bounds(), 1.0);
        let vp = [400.0, 400.0];
        let cm = cam.matrices(1.0, &s.bounds());
        let (p, _) = cam.project(&cm, [0.0, -5.0, 5.0], vp).expect("visible");
        let near_top = [p[0], p[1] + 2.0];
        assert!(s.pick_edge(&cam, vp, near_top, 5.0).is_some(), "the front top edge");
        // Cut at y = 0 towards the eye: the front edges are gone, and the back ones lie
        // behind the cut face.
        let sec = Section { origin: [0.0; 3], normal: cam.forward.map(|c| -c) };
        assert_eq!(s.pick_edge_section(&cam, vp, near_top, 5.0, Some(&sec)), None);
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

#[cfg(test)]
mod top_tests {
    use super::*;
    use crate::demo;

    #[test]
    fn the_cached_hierarchy_follows_moves_hides_and_removals() {
        // 50 boxes in a row along X; a ray down -Z over each must hit that box.
        let mut s = Scene::new();
        for i in 0..50u64 {
            s.insert_mesh(MeshId(i), &demo::box_mesh([1.0; 3])).expect("box");
            s.set_transform(MeshId(i), math::translation([i as f64 * 2.0, 0.0, 0.0]));
        }
        let down = |x: f64| Ray { origin: [x, 0.0, 10.0], dir: [0.0, 0.0, -1.0] };
        for i in 0..50u64 {
            assert_eq!(s.pick(&down(i as f64 * 2.0)).map(|h| h.mesh), Some(MeshId(i)));
        }
        // Moved far away: the old place is empty, the new one hits.
        s.set_transform(MeshId(7), math::translation([500.0, 0.0, 0.0]));
        assert_eq!(s.pick(&down(14.0)).map(|h| h.mesh), None);
        assert_eq!(s.pick(&down(500.0)).map(|h| h.mesh), Some(MeshId(7)));
        // Hidden and removed meshes are not hit.
        s.set_visible(MeshId(3), false);
        assert_eq!(s.pick(&down(6.0)).map(|h| h.mesh), None);
        s.remove_mesh(MeshId(4));
        assert_eq!(s.pick(&down(8.0)).map(|h| h.mesh), None);
        // Two boxes at one place: the lower id wins, as before the hierarchy.
        s.insert_mesh(MeshId(99), &demo::box_mesh([1.0; 3])).expect("box");
        s.set_transform(MeshId(99), math::translation([10.0, 0.0, 0.0]));
        assert_eq!(s.pick(&down(10.0)).map(|h| h.mesh), Some(MeshId(5)));
    }
}
