//! A tiny exact kernel for tests: straight-edged prisms only.
//!
//! Supports `extrude` of polygonal regions without holes (each region becomes its own
//! solid), `compound`, topology, mass properties, bounding boxes and tessellation. It
//! exists so naming, evaluation and document code can be tested deterministically
//! without OCCT.

// Test-support geometry code: every index is taken modulo the polygon length or comes
// from a range over the same vector, so indexing cannot panic.
#![allow(clippy::indexing_slicing, reason = "indices are bounded by construction")]

use crate::types::*;
use crate::{KResult, Kernel, Shape, ShapeData};
use std::any::Any;

#[derive(Debug, Clone, PartialEq)]
struct Prism {
    frame: Frame,
    /// Counter-clockwise polygon in frame coordinates.
    poly: Vec<P2>,
    /// Profile tag per polygon edge (edge i goes from poly[i] to poly[i+1]).
    tags: Vec<u64>,
    /// Extent along the frame normal, `lo < hi`.
    lo: f64,
    hi: f64,
    /// Which cap of the sweep is at `lo` (Start for positive extrusions).
    lo_end: CapEnd,
    region: u32,
}

#[derive(Debug, Clone, PartialEq)]
struct MockShape {
    prisms: Vec<Prism>,
}

impl ShapeData for MockShape {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn kernel(&self) -> &'static str {
        "mock"
    }
}

/// See the module documentation.
#[derive(Debug, Default, Clone, Copy)]
pub struct MockKernel;

const MAX_POLY: usize = 100_000;

fn signed_area(poly: &[P2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        * 0.5
}

fn invalid(msg: impl Into<String>) -> crate::KernelError {
    crate::KernelError::new(KernelErrorKind::InvalidInput, msg)
}

impl Prism {
    fn n(&self) -> usize {
        self.poly.len()
    }
    fn height(&self) -> f64 {
        self.hi - self.lo
    }
    fn at(&self, p: P2, h: f64) -> V3 {
        add(self.frame.to_world(p), scale(self.frame.normal(), h))
    }
    fn p(&self, i: usize) -> P2 {
        self.poly[i % self.n()]
    }
}

impl Kernel for MockKernel {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> KResult<OpResult> {
        if !profile.frame.is_valid() {
            return Err(invalid("profile frame is not orthonormal"));
        }
        if !spec.distance.is_finite() || spec.distance.abs() < 1e-9 {
            return Err(invalid("extrude distance must be non-zero").with_hint("enter a distance such as 10 mm"));
        }
        let (lo, hi) = if spec.symmetric {
            (-spec.distance.abs() / 2.0, spec.distance.abs() / 2.0)
        } else if spec.distance > 0.0 {
            (0.0, spec.distance)
        } else {
            (spec.distance, 0.0)
        };
        let lo_end = if spec.distance > 0.0 || spec.symmetric { CapEnd::Start } else { CapEnd::End };
        let mut prisms = Vec::new();
        for (ri, region) in profile.regions.iter().enumerate() {
            if !region.holes.is_empty() {
                return Err(crate::KernelError::new(
                    KernelErrorKind::Unsupported,
                    "the mock kernel cannot extrude regions with holes",
                ));
            }
            let mut poly = Vec::new();
            let mut tags = Vec::new();
            for e in &region.outer.edges {
                match e.seg {
                    Seg2::Line { a, .. } => {
                        poly.push(a);
                        tags.push(e.tag);
                    }
                    _ => {
                        return Err(crate::KernelError::new(
                            KernelErrorKind::Unsupported,
                            "the mock kernel only supports straight edges",
                        ));
                    }
                }
            }
            if poly.len() < 3 || poly.len() > MAX_POLY {
                return Err(invalid("a profile region needs at least 3 edges"));
            }
            if poly.iter().flatten().any(|c| !c.is_finite()) {
                return Err(invalid("profile contains non-finite coordinates"));
            }
            let area = signed_area(&poly);
            if area.abs() < 1e-12 {
                return Err(invalid("profile region has zero area"));
            }
            if area < 0.0 {
                // Reverse to counter-clockwise; edge i (a_i→a_{i+1}) becomes the reversed
                // edge, so rotate tags to stay attached to the same segment.
                poly.reverse();
                tags.reverse();
                tags.rotate_left(1);
            }
            let region = u32::try_from(ri).map_err(|_| invalid("too many regions"))?;
            prisms.push(Prism { frame: profile.frame, poly, tags, lo, hi, lo_end, region });
        }
        if prisms.is_empty() {
            return Err(invalid("profile has no regions").with_hint("close the sketch profile"));
        }
        let shape = MockShape { prisms };
        let face_origins = face_origins(&shape);
        Ok(OpResult { shape: Shape::new(shape), face_origins })
    }

    fn compound(&self, shapes: &[Shape]) -> KResult<OpResult> {
        let mut prisms = Vec::new();
        let mut origins = Vec::new();
        for (i, s) in shapes.iter().enumerate() {
            let m = s.downcast::<MockShape>()?;
            let input = u32::try_from(i).map_err(|_| invalid("too many shapes"))?;
            let nf = topology(m).faces.len();
            for f in 0..nf {
                let face = u32::try_from(f).map_err(|_| invalid("too many faces"))?;
                origins.push(vec![FaceOrigin::Input { input, face }]);
            }
            prisms.extend(m.prisms.iter().cloned());
        }
        Ok(OpResult { shape: Shape::new(MockShape { prisms }), face_origins: origins })
    }

    fn topology(&self, shape: &Shape) -> KResult<Topology> {
        Ok(topology(shape.downcast::<MockShape>()?))
    }

    fn mass_properties(&self, shape: &Shape) -> KResult<MassProps> {
        let m = shape.downcast::<MockShape>()?;
        let mut volume = 0.0;
        let mut area = 0.0;
        let mut moment = [0.0; 3];
        for p in &m.prisms {
            let a = signed_area(&p.poly);
            let v = a * p.height();
            // Polygon centroid.
            let n = p.n();
            let (mut cx, mut cy) = (0.0, 0.0);
            for i in 0..n {
                let (u, w) = (p.p(i), p.p(i + 1));
                let c = u[0] * w[1] - w[0] * u[1];
                cx += (u[0] + w[0]) * c;
                cy += (u[1] + w[1]) * c;
            }
            let c2 = [cx / (6.0 * a), cy / (6.0 * a)];
            let c3 = p.at(c2, (p.lo + p.hi) / 2.0);
            volume += v;
            moment = add(moment, scale(c3, v));
            let perimeter: f64 = (0..n).map(|i| dist2(p.p(i), p.p(i + 1))).sum();
            area += 2.0 * a + perimeter * p.height();
        }
        let centroid = if volume > 0.0 { scale(moment, 1.0 / volume) } else { [0.0; 3] };
        Ok(MassProps { volume, area, centroid })
    }

    fn bbox(&self, shape: &Shape) -> KResult<BBox> {
        let m = shape.downcast::<MockShape>()?;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for p in &m.prisms {
            for &q in &p.poly {
                for h in [p.lo, p.hi] {
                    let w = p.at(q, h);
                    for k in 0..3 {
                        min[k] = min[k].min(w[k]);
                        max[k] = max[k].max(w[k]);
                    }
                }
            }
        }
        Ok(BBox { min, max })
    }

    fn tessellate(&self, shape: &Shape, _params: &TessellationParams) -> KResult<Mesh> {
        let m = shape.downcast::<MockShape>()?;
        let mut mesh = Mesh::default();
        let f32v = |v: V3| [v[0] as f32, v[1] as f32, v[2] as f32];
        for p in &m.prisms {
            let nrm = p.frame.normal();
            let tris = ear_clip(&p.poly);
            // Caps: low (normal -n), high (+n). Order matches topology: start cap, end cap.
            let caps =
                if p.lo_end == CapEnd::Start { [(p.lo, -1.0), (p.hi, 1.0)] } else { [(p.hi, 1.0), (p.lo, -1.0)] };
            for (h, sgn) in caps {
                let start = (mesh.indices.len() / 3) as u32;
                let base = mesh.positions.len() as u32;
                for &q in &p.poly {
                    mesh.positions.push(f32v(p.at(q, h)));
                    mesh.normals.push(f32v(scale(nrm, sgn)));
                }
                for t in &tris {
                    if sgn > 0.0 {
                        mesh.indices.extend([base + t[0], base + t[1], base + t[2]]);
                    } else {
                        mesh.indices.extend([base + t[0], base + t[2], base + t[1]]);
                    }
                }
                mesh.face_ranges.push(start..(mesh.indices.len() / 3) as u32);
            }
            for i in 0..p.n() {
                let (a, b) = (p.p(i), p.p(i + 1));
                let d = [b[0] - a[0], b[1] - a[1]];
                let out = normalize(add(scale(p.frame.x, d[1]), scale(p.frame.y, -d[0]))).unwrap_or(p.frame.x);
                let start = (mesh.indices.len() / 3) as u32;
                let base = mesh.positions.len() as u32;
                for w in [p.at(a, p.lo), p.at(b, p.lo), p.at(b, p.hi), p.at(a, p.hi)] {
                    mesh.positions.push(f32v(w));
                    mesh.normals.push(f32v(out));
                }
                mesh.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
                mesh.face_ranges.push(start..(mesh.indices.len() / 3) as u32);
            }
            for e in prism_edges(p) {
                mesh.edges.push(vec![f32v(e.0), f32v(e.1)]);
            }
        }
        Ok(mesh)
    }
}

fn dist2(a: P2, b: P2) -> f64 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
}

/// Face order per prism: [start cap, end cap, lateral 0..n].
fn face_origins(m: &MockShape) -> Vec<Vec<FaceOrigin>> {
    let mut out = Vec::new();
    for p in &m.prisms {
        out.push(vec![FaceOrigin::Cap { end: CapEnd::Start, region: p.region }]);
        out.push(vec![FaceOrigin::Cap { end: CapEnd::End, region: p.region }]);
        for &tag in &p.tags {
            out.push(vec![FaceOrigin::Lateral { tag }]);
        }
    }
    out
}

/// Edge order per prism: bottom (lo) edges 0..n, top (hi) edges 0..n, vertical edges 0..n.
fn prism_edges(p: &Prism) -> Vec<(V3, V3)> {
    let n = p.n();
    let mut v = Vec::with_capacity(3 * n);
    for h in [p.lo, p.hi] {
        for i in 0..n {
            v.push((p.at(p.p(i), h), p.at(p.p(i + 1), h)));
        }
    }
    for i in 0..n {
        v.push((p.at(p.p(i), p.lo), p.at(p.p(i), p.hi)));
    }
    v
}

fn topology(m: &MockShape) -> Topology {
    let mut t = Topology::default();
    for p in &m.prisms {
        let n = p.n();
        let f0 = t.faces.len() as u32;
        let v0 = t.vertices.len() as u32;
        let nrm = p.frame.normal();
        let a = signed_area(&p.poly);
        let mut c2 = [0.0, 0.0];
        for q in &p.poly {
            c2[0] += q[0] / n as f64;
            c2[1] += q[1] / n as f64;
        }
        let (lo_face, hi_face) = if p.lo_end == CapEnd::Start { (f0, f0 + 1) } else { (f0 + 1, f0) };
        let caps = if p.lo_end == CapEnd::Start { [(p.lo, -1.0), (p.hi, 1.0)] } else { [(p.hi, 1.0), (p.lo, -1.0)] };
        for (h, sgn) in caps {
            t.faces.push(FaceInfo {
                kind: SurfaceKind::Plane,
                area: a,
                centroid: p.at(c2, h),
                normal: scale(nrm, sgn),
            });
        }
        for i in 0..n {
            let (u, w) = (p.p(i), p.p(i + 1));
            let d = [w[0] - u[0], w[1] - u[1]];
            let out = normalize(add(scale(p.frame.x, d[1]), scale(p.frame.y, -d[0]))).unwrap_or(p.frame.x);
            let mid = [(u[0] + w[0]) / 2.0, (u[1] + w[1]) / 2.0];
            t.faces.push(FaceInfo {
                kind: SurfaceKind::Plane,
                area: dist2(u, w) * p.height(),
                centroid: p.at(mid, (p.lo + p.hi) / 2.0),
                normal: out,
            });
        }
        for h in [p.lo, p.hi] {
            for i in 0..n {
                let _ = i;
                t.vertices.push(VertexInfo { point: p.at(p.p(i), h) });
            }
        }
        let lat = |i: usize| f0 + 2 + (i % n) as u32;
        let vlo = |i: usize| v0 + (i % n) as u32;
        let vhi = |i: usize| v0 + n as u32 + (i % n) as u32;
        let edges = prism_edges(p);
        for (k, (a3, b3)) in edges.into_iter().enumerate() {
            let i = k % n;
            let (mut faces, verts) = match k / n {
                0 => (vec![lo_face, lat(i)], vec![vlo(i), vlo(i + 1)]),
                1 => (vec![hi_face, lat(i)], vec![vhi(i), vhi(i + 1)]),
                _ => (vec![lat(i + n - 1), lat(i)], vec![vlo(i), vhi(i)]),
            };
            faces.sort_unstable();
            t.edges.push(EdgeInfo {
                kind: CurveKind::Line,
                length: norm(sub(b3, a3)),
                midpoint: scale(add(a3, b3), 0.5),
                faces,
                vertices: verts,
            });
        }
        t.solids += 1;
        t.shells += 1;
    }
    t
}

/// Ear-clipping triangulation of a simple counter-clockwise polygon. O(n²); fine for tests.
fn ear_clip(poly: &[P2]) -> Vec<[u32; 3]> {
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    let mut out = Vec::new();
    let cross2 = |o: P2, a: P2, b: P2| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut guard = 0;
    while idx.len() > 3 && guard < poly.len() * poly.len() + 10 {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross2(a, b, c) <= 0.0 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                j != ia
                    && j != ib
                    && j != ic
                    && cross2(a, b, poly[j]) >= 0.0
                    && cross2(b, c, poly[j]) >= 0.0
                    && cross2(c, a, poly[j]) >= 0.0
            });
            if !inside {
                out.push([ia as u32, ib as u32, ic as u32]);
                idx.remove(k);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        out.push([idx[0] as u32, idx[1] as u32, idx[2] as u32]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Profile {
        let pts = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
        let edges = (0..4)
            .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: 10 + i as u64 })
            .collect();
        Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] }
    }

    #[test]
    fn box_topology_and_mass() {
        let k = MockKernel;
        let r = k.extrude(&rect(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
        let t = k.topology(&r.shape).unwrap();
        assert_eq!((t.faces.len(), t.edges.len(), t.vertices.len()), (6, 12, 8));
        assert_eq!(r.face_origins.len(), 6);
        assert_eq!(r.face_origins[2], vec![FaceOrigin::Lateral { tag: 10 }]);
        // Euler: V - E + F = 2.
        assert_eq!(t.vertices.len() as i64 - t.edges.len() as i64 + t.faces.len() as i64, 2);
        // Every edge has exactly two faces.
        assert!(t.edges.iter().all(|e| e.faces.len() == 2));
        let mp = k.mass_properties(&r.shape).unwrap();
        assert!((mp.volume - 8000.0).abs() < 1e-9);
        assert!((mp.area - (2.0 * 800.0 + 120.0 * 10.0)).abs() < 1e-9);
        assert!((mp.centroid[2] - 5.0).abs() < 1e-12);
        // Start cap faces -Z, end cap +Z.
        assert_eq!(t.faces[0].normal, [0.0, 0.0, -1.0]);
        assert_eq!(t.faces[1].normal, [0.0, 0.0, 1.0]);
        // Lateral face 0 (from (0,0) to (40,0)) points -Y.
        assert!((t.faces[2].normal[1] + 1.0).abs() < 1e-12);
        let bb = k.bbox(&r.shape).unwrap();
        assert_eq!(bb.size(), [40.0, 20.0, 10.0]);
    }

    #[test]
    fn negative_and_symmetric() {
        let k = MockKernel;
        let r = k.extrude(&rect(1.0, 1.0), &ExtrudeSpec { distance: -4.0, symmetric: false }).unwrap();
        let bb = k.bbox(&r.shape).unwrap();
        assert_eq!((bb.min[2], bb.max[2]), (-4.0, 0.0));
        let t = k.topology(&r.shape).unwrap();
        // Start cap stays the sketch-plane face (z=0) and faces +Z for a negative extrusion.
        assert_eq!(t.faces[0].centroid[2], 0.0);
        let r = k.extrude(&rect(1.0, 1.0), &ExtrudeSpec { distance: 4.0, symmetric: true }).unwrap();
        let bb = k.bbox(&r.shape).unwrap();
        assert_eq!((bb.min[2], bb.max[2]), (-2.0, 2.0));
    }

    #[test]
    fn clockwise_profile_keeps_tags_on_segments() {
        let k = MockKernel;
        let pts = [[0.0, 0.0], [0.0, 2.0], [3.0, 2.0], [3.0, 0.0]]; // clockwise
        let edges =
            (0..4).map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: i as u64 }).collect();
        let prof = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
        let r = k.extrude(&prof, &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap();
        let t = k.topology(&r.shape).unwrap();
        // Tag 1 is the segment (0,2)→(3,2): its lateral face centroid has y = 2.
        let f = r.face_origins.iter().position(|o| o == &vec![FaceOrigin::Lateral { tag: 1 }]).unwrap();
        assert!((t.faces[f].centroid[1] - 2.0).abs() < 1e-12);
        assert!((t.faces[f].normal[1] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn tessellation_counts() {
        let k = MockKernel;
        let r = k.extrude(&rect(2.0, 1.0), &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap();
        let m = k.tessellate(&r.shape, &TessellationParams::default()).unwrap();
        assert_eq!(m.face_ranges.len(), 6);
        assert_eq!(m.triangle_count(), 2 * 2 + 4 * 2);
        assert_eq!(m.edges.len(), 12);
    }

    #[test]
    fn rejects_bad_input() {
        let k = MockKernel;
        assert!(k.extrude(&rect(1.0, 1.0), &ExtrudeSpec { distance: 0.0, symmetric: false }).is_err());
        assert!(k.extrude(&rect(0.0, 1.0), &ExtrudeSpec { distance: 1.0, symmetric: false }).is_err());
        let mut p = rect(1.0, 1.0);
        p.frame.x = [2.0, 0.0, 0.0];
        assert!(k.extrude(&p, &ExtrudeSpec { distance: 1.0, symmetric: false }).is_err());
        assert_eq!(
            k.fillet(
                &k.extrude(&rect(1.0, 1.0), &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap().shape,
                &[0],
                0.1
            )
            .unwrap_err()
            .kind,
            KernelErrorKind::Unsupported
        );
    }

    #[test]
    fn ear_clip_concave() {
        // L shape.
        let poly = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]];
        let tris = ear_clip(&poly);
        assert_eq!(tris.len(), 4);
        let area: f64 = tris
            .iter()
            .map(|t| {
                let (a, b, c) = (poly[t[0] as usize], poly[t[1] as usize], poly[t[2] as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2.0
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-12);
    }
}
