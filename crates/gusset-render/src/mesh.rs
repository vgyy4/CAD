//! Compact display meshes and the CPU-side triangle BVH used for picking.
//!
//! A [`MeshData`] is the only CPU copy the renderer keeps of a mesh: positions and
//! indices (needed for picking anyway), normals packed to `snorm16x4` (8 bytes instead
//! of 12), per-B-rep-face triangle ranges, edge segments and the BVH. The GPU buffers
//! are built from it; the caller can drop its `gusset_kernel::Mesh` after
//! [`crate::Scene::insert_mesh`].

use std::fmt;
use std::ops::Range;

use crate::math::{Aabb, zip_with};

/// Why a mesh was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshError {
    /// `indices.len()` is not a multiple of 3.
    IndicesNotTriangles,
    /// An index points past the end of `positions`.
    IndexOutOfRange {
        /// Offending triangle.
        triangle: usize,
    },
    /// A position is NaN or infinite.
    NonFinitePosition {
        /// Offending vertex.
        vertex: usize,
    },
    /// More vertices or triangles than a `u32` index can address.
    TooLarge,
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndicesNotTriangles => write!(f, "mesh index count is not a multiple of 3"),
            Self::IndexOutOfRange { triangle } => write!(f, "triangle {triangle} references a missing vertex"),
            Self::NonFinitePosition { vertex } => write!(f, "vertex {vertex} has a non-finite coordinate"),
            Self::TooLarge => write!(f, "mesh is too large for 32-bit indices"),
        }
    }
}

impl std::error::Error for MeshError {}

/// Pack a unit normal into `snorm16x4` (w = 0).
#[must_use]
pub fn pack_normal(n: [f32; 3]) -> [i16; 4] {
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let n = if l > 1e-20 && l.is_finite() { n.map(|c| c / l) } else { [0.0, 0.0, 1.0] };
    let q = |c: f32| (c.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    [q(n[0]), q(n[1]), q(n[2]), 0]
}

/// Unpack a `snorm16x4` normal (as the GPU does).
#[must_use]
pub fn unpack_normal(p: [i16; 4]) -> [f32; 3] {
    let u = |c: i16| (f32::from(c) / 32767.0).max(-1.0);
    [u(p[0]), u(p[1]), u(p[2])]
}

/// A validated, compact display mesh.
#[derive(Debug, Clone)]
pub struct MeshData {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) normals: Vec<[i16; 4]>,
    pub(crate) indices: Vec<u32>,
    /// Triangle range per B-rep face (sorted, disjoint, covering every triangle).
    pub(crate) face_ranges: Vec<Range<u32>>,
    /// Segment endpoints, two per segment.
    pub(crate) edge_points: Vec<[f32; 3]>,
    /// Segment range per topological edge.
    pub(crate) edge_ranges: Vec<Range<u32>>,
    pub(crate) bounds: Aabb,
    pub(crate) bvh: Bvh,
}

impl MeshData {
    /// Validate and compact a kernel mesh. Missing or mismatched normals are
    /// recomputed (area-weighted); missing or inconsistent `face_ranges` become one face
    /// covering every triangle.
    ///
    /// # Errors
    /// See [`MeshError`].
    pub fn from_mesh(mesh: &gusset_kernel::Mesh) -> Result<MeshData, MeshError> {
        if !mesh.indices.len().is_multiple_of(3) {
            return Err(MeshError::IndicesNotTriangles);
        }
        let nv = mesh.positions.len();
        let ntri = mesh.indices.len() / 3;
        if u32::try_from(nv).is_err() || u32::try_from(ntri).is_err() {
            return Err(MeshError::TooLarge);
        }
        let mut bounds = Aabb::EMPTY;
        for (i, p) in mesh.positions.iter().enumerate() {
            if !p.iter().all(|c| c.is_finite()) {
                return Err(MeshError::NonFinitePosition { vertex: i });
            }
            bounds.include(p.map(f64::from));
        }
        for (t, tri) in mesh.indices.chunks_exact(3).enumerate() {
            if tri.iter().any(|&i| i as usize >= nv) {
                return Err(MeshError::IndexOutOfRange { triangle: t });
            }
        }
        let normals = if mesh.normals.len() == nv && mesh.normals.iter().all(|n| n.iter().all(|c| c.is_finite())) {
            mesh.normals.iter().map(|&n| pack_normal(n)).collect()
        } else {
            compute_normals(&mesh.positions, &mesh.indices).into_iter().map(pack_normal).collect()
        };
        let ntri32 = ntri as u32;
        let face_ranges = if ranges_are_valid(&mesh.face_ranges, ntri32) {
            mesh.face_ranges.clone()
        } else {
            std::iter::once(0..ntri32).collect()
        };
        let mut edge_points = Vec::new();
        let mut edge_ranges = Vec::with_capacity(mesh.edges.len());
        for poly in &mesh.edges {
            let start = (edge_points.len() / 2) as u32;
            for w in poly.windows(2) {
                if let [a, b] = w
                    && a.iter().chain(b.iter()).all(|c| c.is_finite())
                {
                    edge_points.push(*a);
                    edge_points.push(*b);
                    bounds.include(a.map(f64::from));
                    bounds.include(b.map(f64::from));
                }
            }
            edge_ranges.push(start..(edge_points.len() / 2) as u32);
        }
        if u32::try_from(edge_points.len()).is_err() {
            return Err(MeshError::TooLarge);
        }
        let bvh = Bvh::build(&mesh.positions, &mesh.indices);
        Ok(MeshData {
            positions: mesh.positions.clone(),
            normals,
            indices: mesh.indices.clone(),
            face_ranges,
            edge_points,
            edge_ranges,
            bounds,
            bvh,
        })
    }

    /// Number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Number of B-rep faces.
    #[must_use]
    pub fn face_count(&self) -> usize {
        self.face_ranges.len()
    }

    /// Number of topological edges.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edge_ranges.len()
    }

    /// Bounds in mesh (local) coordinates.
    #[must_use]
    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    /// The face that owns triangle `tri`.
    #[must_use]
    pub fn face_of_triangle(&self, tri: u32) -> Option<u32> {
        let i = self.face_ranges.partition_point(|r| r.end <= tri);
        self.face_ranges.get(i).filter(|r| r.contains(&tri)).map(|_| i as u32)
    }

    /// Approximate CPU memory held by this mesh, in bytes.
    #[must_use]
    pub fn cpu_bytes(&self) -> usize {
        self.positions.len() * 12
            + self.normals.len() * 8
            + self.indices.len() * 4
            + self.edge_points.len() * 12
            + (self.face_ranges.len() + self.edge_ranges.len()) * 8
            + self.bvh.bytes()
    }

    pub(crate) fn triangle(&self, t: u32) -> Option<[[f32; 3]; 3]> {
        let base = t as usize * 3;
        let idx = self.indices.get(base..base + 3)?;
        let p = |i: u32| self.positions.get(i as usize).copied();
        Some([p(*idx.first()?)?, p(*idx.get(1)?)?, p(*idx.get(2)?)?])
    }
}

fn ranges_are_valid(ranges: &[Range<u32>], ntri: u32) -> bool {
    if ranges.is_empty() {
        return false;
    }
    let mut next = 0;
    for r in ranges {
        if r.start != next || r.end < r.start {
            return false;
        }
        next = r.end;
    }
    next == ntri
}

fn compute_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0_f32; 3]; positions.len()];
    for tri in indices.chunks_exact(3) {
        let (Some(&a), Some(&b), Some(&c)) = (
            tri.first().and_then(|&i| positions.get(i as usize)),
            tri.get(1).and_then(|&i| positions.get(i as usize)),
            tri.get(2).and_then(|&i| positions.get(i as usize)),
        ) else {
            continue;
        };
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let f = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        for &i in tri {
            if let Some(v) = n.get_mut(i as usize) {
                *v = crate::math::zip_with(*v, f, |a, b| a + b);
            }
        }
    }
    n
}

/// One BVH node: a box plus either two children (`count == 0`, children at `first` and
/// `first + 1`) or a leaf of `count` triangles starting at `first` in the order array.
#[derive(Debug, Clone, Copy)]
struct BvhNode {
    min: [f32; 3],
    max: [f32; 3],
    first: u32,
    count: u32,
}

/// A bounding volume hierarchy over a mesh's triangles (median split on the longest
/// centroid axis, up to 4 triangles per leaf). Display-only: picking tolerances do not
/// affect the model.
#[derive(Debug, Clone, Default)]
pub struct Bvh {
    nodes: Vec<BvhNode>,
    order: Vec<u32>,
}

const LEAF_SIZE: usize = 4;

/// A ray hit in mesh-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalHit {
    /// Triangle index.
    pub triangle: u32,
    /// Ray parameter (local units of the ray direction).
    pub t: f32,
}

impl Bvh {
    /// Build over `indices` (triangles) into `positions`.
    #[must_use]
    pub fn build(positions: &[[f32; 3]], indices: &[u32]) -> Bvh {
        let tris: Vec<([f32; 3], [f32; 3], [f32; 3])> = indices
            .chunks_exact(3)
            .map(|t| {
                let mut lo = [f32::INFINITY; 3];
                let mut hi = [f32::NEG_INFINITY; 3];
                for &i in t {
                    if let Some(&p) = positions.get(i as usize) {
                        lo = zip_with(lo, p, f32::min);
                        hi = zip_with(hi, p, f32::max);
                    }
                }
                let c = zip_with(lo, hi, |l, h| 0.5 * (l + h));
                (lo, hi, c)
            })
            .collect();
        let mut bvh = Bvh { nodes: Vec::with_capacity(tris.len() / 2 + 1), order: (0..tris.len() as u32).collect() };
        if tris.is_empty() {
            return bvh;
        }
        bvh.nodes.push(BvhNode { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
        // Explicit stack: (node index, range in `order`).
        let mut stack = vec![(0_usize, 0_usize, tris.len())];
        while let Some((node, start, end)) = stack.pop() {
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            let mut clo = [f32::INFINITY; 3];
            let mut chi = [f32::NEG_INFINITY; 3];
            for &t in bvh.order.get(start..end).unwrap_or(&[]) {
                if let Some(&(a, b, c)) = tris.get(t as usize) {
                    lo = zip_with(lo, a, f32::min);
                    hi = zip_with(hi, b, f32::max);
                    clo = zip_with(clo, c, f32::min);
                    chi = zip_with(chi, c, f32::max);
                }
            }
            let ext = zip_with(chi, clo, |h, l| h - l);
            let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] {
                0
            } else if ext[1] >= ext[2] {
                1
            } else {
                2
            };
            let n = end - start;
            let leaf = n <= LEAF_SIZE || ext.get(axis).is_none_or(|e| *e <= 0.0);
            if leaf {
                if let Some(nd) = bvh.nodes.get_mut(node) {
                    *nd = BvhNode { min: lo, max: hi, first: start as u32, count: n as u32 };
                }
                continue;
            }
            let mid = start + n / 2;
            if let Some(slice) = bvh.order.get_mut(start..end) {
                let key = |t: &u32| tris.get(*t as usize).map_or(0.0, |x| x.2.get(axis).copied().unwrap_or(0.0));
                slice.select_nth_unstable_by(n / 2, |a, b| key(a).total_cmp(&key(b)));
            }
            let left = bvh.nodes.len();
            bvh.nodes.push(BvhNode { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
            bvh.nodes.push(BvhNode { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
            if let Some(nd) = bvh.nodes.get_mut(node) {
                *nd = BvhNode { min: lo, max: hi, first: left as u32, count: 0 };
            }
            stack.push((left, start, mid));
            stack.push((left + 1, mid, end));
        }
        bvh
    }

    fn bytes(&self) -> usize {
        self.nodes.len() * std::mem::size_of::<BvhNode>() + self.order.len() * 4
    }

    /// Closest hit along `origin + t * dir` with `t` in `(0, t_max)`. Triangles are
    /// hit from both sides.
    #[must_use]
    pub fn raycast(&self, mesh: &MeshData, origin: [f32; 3], dir: [f32; 3], t_max: f32) -> Option<LocalHit> {
        let inv = dir.map(|d| if d.abs() > 1e-30 { 1.0 / d } else { 1e30_f32.copysign(d) });
        let mut best: Option<LocalHit> = None;
        let mut limit = t_max;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        if !self.nodes.is_empty() {
            stack.push(0);
        }
        while let Some(i) = stack.pop() {
            let Some(node) = self.nodes.get(i as usize) else { continue };
            if !slab(node, origin, inv, limit) {
                continue;
            }
            if node.count == 0 {
                stack.push(node.first);
                stack.push(node.first + 1);
                continue;
            }
            let range = node.first as usize..(node.first + node.count) as usize;
            for &tri in self.order.get(range).unwrap_or(&[]) {
                if let Some(v) = mesh.triangle(tri)
                    && let Some(t) = intersect_triangle(origin, dir, v)
                    && t < limit
                {
                    limit = t;
                    best = Some(LocalHit { triangle: tri, t });
                }
            }
        }
        best
    }
}

fn slab(n: &BvhNode, o: [f32; 3], inv: [f32; 3], t_max: f32) -> bool {
    let mut t0 = 0.0_f32;
    let mut t1 = t_max;
    for (((mn, mx), ok), ik) in n.min.iter().zip(&n.max).zip(&o).zip(&inv) {
        let a = (mn - ok) * ik;
        let b = (mx - ok) * ik;
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    t0 <= t1
}

/// Möller–Trumbore, two-sided. Returns `t > 0`.
fn intersect_triangle(o: [f32; 3], d: [f32; 3], v: [[f32; 3]; 3]) -> Option<f32> {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross =
        |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let e1 = sub(v[1], v[0]);
    let e2 = sub(v[2], v[0]);
    let p = cross(d, e2);
    let det = dot(e1, p);
    let scale = dot(e1, e1).max(dot(e2, e2)) * dot(d, d).sqrt();
    if det.abs() <= 1e-12 * scale {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, v[0]);
    let u = dot(s, p) * inv;
    if !(-1e-6..=1.0 + 1e-6).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let w = dot(d, q) * inv;
    if w < -1e-6 || u + w > 1.0 + 1e-6 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t > 0.0 && t.is_finite()).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    #[test]
    fn rejects_bad_meshes() {
        let mut m = demo::box_mesh([1.0; 3]);
        m.indices.push(0);
        assert_eq!(MeshData::from_mesh(&m).err(), Some(MeshError::IndicesNotTriangles));
        let mut m = demo::box_mesh([1.0; 3]);
        m.indices[5] = 999;
        assert_eq!(MeshData::from_mesh(&m).err(), Some(MeshError::IndexOutOfRange { triangle: 1 }));
        let mut m = demo::box_mesh([1.0; 3]);
        m.positions[2][1] = f32::NAN;
        assert_eq!(MeshData::from_mesh(&m).err(), Some(MeshError::NonFinitePosition { vertex: 2 }));
    }

    #[test]
    fn repairs_normals_and_face_ranges() {
        let mut m = demo::box_mesh([2.0; 3]);
        m.normals.clear();
        m.face_ranges = std::iter::once(0..3).collect();
        let d = MeshData::from_mesh(&m).expect("valid");
        assert_eq!(d.face_count(), 1);
        assert_eq!(d.normals.len(), d.positions.len());
        assert_eq!(d.face_of_triangle(11), Some(0));
        let d = MeshData::from_mesh(&demo::box_mesh([2.0; 3])).expect("valid");
        assert_eq!(d.face_count(), 6);
        assert_eq!(d.face_of_triangle(0), Some(0));
        assert_eq!(d.face_of_triangle(3), Some(1));
        assert_eq!(d.face_of_triangle(11), Some(5));
        assert_eq!(d.face_of_triangle(12), None);
        assert_eq!(d.edge_count(), 12);
    }

    #[test]
    fn normal_packing_round_trips() {
        for n in [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.577_35, 0.577_35, -0.577_35]] {
            let u = unpack_normal(pack_normal(n));
            for k in 0..3 {
                assert!((u[k] - n[k]).abs() < 1e-4);
            }
        }
        assert_eq!(pack_normal([0.0, 0.0, 0.0]), [0, 0, 32767, 0]);
    }

    #[test]
    fn bvh_matches_brute_force() {
        let m = demo::cylinder_mesh(3.0, 5.0, 48);
        let d = MeshData::from_mesh(&m).expect("valid");
        let mut seed = 12345_u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32
        };
        let mut hits = 0;
        for _ in 0..500 {
            let o = [rnd() * 20.0 - 10.0, rnd() * 20.0 - 10.0, rnd() * 20.0 - 10.0];
            let target = [rnd() * 6.0 - 3.0, rnd() * 6.0 - 3.0, rnd() * 5.0];
            let dir = [target[0] - o[0], target[1] - o[1], target[2] - o[2]];
            let fast = d.bvh.raycast(&d, o, dir, f32::INFINITY);
            let mut slow: Option<LocalHit> = None;
            for t in 0..d.triangle_count() as u32 {
                if let Some(tt) = intersect_triangle(o, dir, d.triangle(t).expect("tri"))
                    && slow.is_none_or(|s| tt < s.t)
                {
                    slow = Some(LocalHit { triangle: t, t: tt });
                }
            }
            match (fast, slow) {
                (Some(a), Some(b)) => {
                    hits += 1;
                    assert!((a.t - b.t).abs() < 1e-4, "{a:?} {b:?}");
                }
                (None, None) => {}
                other => panic!("mismatch {other:?}"),
            }
        }
        assert!(hits > 100);
    }
}
