//! The adapter proper (feature `solidcore`).

use std::any::Any;
use std::f64::consts::{PI, TAU};
use std::sync::OnceLock;

use gusset_kernel::{
    Axis3, BBox, BoolOp, CapEnd, Circle3, CurveKind, EdgeInfo, ExtrudeSpec, FaceInfo, FaceOrigin, Frame, KResult,
    Kernel, KernelError, KernelErrorKind, MassProps, Mesh, OpResult, P2, Profile, RevolveSpec, Seg2, Shape, ShapeData,
    SurfaceKind, TessellationParams, Topology, Transform, V3, VertexInfo,
};
use solidcore_ops as ops;
use solidcore_record as rec;
use solidcore_record::brep::{Curve as RCurve, Surface as RSurface};

use crate::SolidcoreKernel;

const NAME: &str = "solidcore";

/// Relative accuracy asked of SolidCore's mass-property integrator.
const MASS_ACCURACY: f64 = 1e-10;

/// Turns every SolidCore upstream fix on, once per process (the session is process-global).
pub(crate) fn init() -> KResult<()> {
    static DONE: OnceLock<Result<(), String>> = OnceLock::new();
    DONE.get_or_init(|| {
        ops::session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied())
            .map_err(|e| format!("SolidCore refused its fixes: {e:?}"))
    })
    .clone()
    .map_err(|m| KernelError::new(KernelErrorKind::Internal, m))
}

fn err(kind: KernelErrorKind, m: impl Into<String>) -> KernelError {
    KernelError::new(kind, m)
}

fn invalid(m: impl Into<String>) -> KernelError {
    err(KernelErrorKind::InvalidInput, m)
}

fn internal(m: impl Into<String>) -> KernelError {
    err(KernelErrorKind::Internal, m)
}

/// SolidCore's error as ours.
fn op_err(what: &str, e: &ops::Error) -> KernelError {
    let kind = match e {
        ops::Error::Argument => KernelErrorKind::InvalidInput,
        ops::Error::NotSolid => KernelErrorKind::InvalidResult,
        ops::Error::Internal(_) => KernelErrorKind::Internal,
        _ => KernelErrorKind::OperationFailed,
    };
    err(kind, format!("{what} failed in SolidCore: {e:?}"))
}

// ---------------------------------------------------------------------------------------------
// Small vector helpers (deterministic: no platform transcendental functions, see clippy.toml).

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let n = norm(a);
    if n > 0.0 { scale(a, 1.0 / n) } else { a }
}
fn dist(a: V3, b: V3) -> f64 {
    norm(sub(a, b))
}

/// Distance from `p` to the triangle `a b c`.
fn point_triangle(p: V3, a: V3, b: V3, c: V3) -> f64 {
    // Ericson, Real-Time Collision Detection 5.1.5.
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return dist(p, a);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return dist(p, b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return dist(p, add(a, scale(ab, v)));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return dist(p, c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return dist(p, add(a, scale(ac, w)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return dist(p, add(b, scale(sub(c, b), w)));
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    dist(p, add(a, add(scale(ab, v), scale(ac, w))))
}

/// Distance from `p` to the segment `a b`.
fn point_segment(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 > 0.0 { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    dist(p, add(a, scale(ab, t)))
}

// ---------------------------------------------------------------------------------------------
// Shapes.

/// A fine mesh of one face, for measures and geometric history.
#[derive(Debug, Default)]
struct FaceMesh {
    tris: Vec<[V3; 3]>,
    area: f64,
    centroid: V3,
    normal: V3,
    /// A point well inside the face: the centroid of its largest triangle.
    inner: V3,
}

#[derive(Debug)]
struct Faceted {
    faces: Vec<FaceMesh>,
    edges: Vec<Vec<V3>>,
    /// The chord the mesh was made at.
    chord: f64,
}

/// A SolidCore body with its record (whose face, edge and vertex order is the topology's).
#[derive(Debug)]
pub(crate) struct ScShape {
    body: ops::Body,
    record: rec::brep::Brep,
    order: ops::Order,
    size: f64,
    faceted: OnceLock<Result<Faceted, String>>,
}

impl ShapeData for ScShape {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn kernel(&self) -> &'static str {
        NAME
    }
}

impl ScShape {
    fn new(body: ops::Body) -> KResult<ScShape> {
        let (record, order) = body.to_record().map_err(|e| op_err("reading the result", &e))?;
        let (lo, hi) = body.bounds().map_err(|e| op_err("bounds", &e))?;
        let size = dist(lo, hi).max(1e-3);
        Ok(ScShape { body, record, order, size, faceted: OnceLock::new() })
    }

    /// A copy of the body, with the order that maps this record's indices onto it.
    fn copy(&self) -> KResult<(ops::Body, ops::Order)> {
        self.body.copy_with_order(&self.order).map_err(|e| op_err("copy", &e))
    }

    fn faceted(&self) -> KResult<&Faceted> {
        self.faceted
            .get_or_init(|| facet(&self.record, (self.size * 2e-4).max(1e-4)))
            .as_ref()
            .map_err(|m| err(KernelErrorKind::OperationFailed, m.clone()))
    }

    fn surface_kind(&self, f: usize) -> SurfaceKind {
        self.record.faces.get(f).map_or(SurfaceKind::Other, |face| surface_kind(&face.surface))
    }
}

fn facet(record: &rec::brep::Brep, chord: f64) -> Result<Faceted, String> {
    let (plan, _) = solidcore_tess::plan(record, &solidcore_tess::MeshTolerance::chord(chord), 0.0)
        .map_err(|e| format!("SolidCore could not mesh the body: {e}"))?;
    let (mesh, refused) = plan.evaluate();
    if let Some(r) = refused.first() {
        return Err(format!("SolidCore could not mesh {} face(s), first: {r:?}", refused.len()));
    }
    let p = |i: u32| mesh.positions.get(i as usize).map(|v| [v.x, v.y, v.z]);
    let mut faces: Vec<FaceMesh> = (0..record.faces.len()).map(|_| FaceMesh::default()).collect();
    let mut sums: Vec<(f64, V3, V3, f64)> = vec![(0.0, [0.0; 3], [0.0; 3], -1.0); record.faces.len()];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) else {
            return Err("SolidCore's mesh indexes a vertex it does not have".into());
        };
        let (Some(fm), Some(s)) = (faces.get_mut(*f as usize), sums.get_mut(*f as usize)) else {
            return Err("SolidCore's mesh names a face the body does not have".into());
        };
        let n = cross(sub(b, a), sub(c, a));
        let area = norm(n) / 2.0;
        let g = scale(add(a, add(b, c)), 1.0 / 3.0);
        s.0 += area;
        s.1 = add(s.1, scale(g, area));
        s.2 = add(s.2, n);
        if area > s.3 {
            s.3 = area;
            fm.inner = g;
        }
        fm.tris.push([a, b, c]);
    }
    for (fm, s) in faces.iter_mut().zip(&sums) {
        fm.area = s.0;
        fm.centroid = if s.0 > 0.0 { scale(s.1, 1.0 / s.0) } else { fm.inner };
        fm.normal = unit(s.2);
    }
    let edges = mesh.edges.iter().map(|pl| pl.iter().map(|v| [v.x, v.y, v.z]).collect()).collect();
    Ok(Faceted { faces, edges, chord })
}

fn sc(shape: &Shape) -> KResult<&ScShape> {
    shape.downcast::<ScShape>()
}

fn surface_kind(s: &RSurface) -> SurfaceKind {
    match s {
        RSurface::Plane { .. } | RSurface::PlaneThrough { .. } => SurfaceKind::Plane,
        RSurface::Cylinder { .. } => SurfaceKind::Cylinder,
        RSurface::Cone { .. } => SurfaceKind::Cone,
        RSurface::Sphere { .. } | RSurface::BallPatch { .. } => SurfaceKind::Sphere,
        RSurface::Torus { .. } => SurfaceKind::Torus,
        RSurface::Spline { .. } | RSurface::RationalSpline { .. } => SurfaceKind::BSpline,
        RSurface::PlacedRevolution { .. } => SurfaceKind::Other,
    }
}

fn frame_axis(f: &rec::brep::Frame) -> Axis3 {
    Axis3 { origin: f.origin, dir: unit(cross(f.x_axis, f.y_axis)) }
}

fn surface_axis(s: &RSurface) -> Option<Axis3> {
    match s {
        RSurface::Cylinder { frame, .. }
        | RSurface::Cone { frame, .. }
        | RSurface::Sphere { frame, .. }
        | RSurface::Torus { frame, .. } => Some(frame_axis(frame)),
        RSurface::BallPatch { center, .. } => Some(Axis3 { origin: *center, dir: [0.0, 0.0, 1.0] }),
        RSurface::PlacedRevolution { origin, axis, .. } => Some(Axis3 { origin: *origin, dir: unit(*axis) }),
        _ => None,
    }
}

fn curve_kind(c: &RCurve) -> CurveKind {
    match c {
        RCurve::Line { .. } => CurveKind::Line,
        RCurve::Circle { .. } => CurveKind::Circle,
        RCurve::Ellipse { .. } => CurveKind::Ellipse,
        RCurve::Spline { .. } | RCurve::RationalSpline { .. } => CurveKind::BSpline,
        RCurve::Polyline { .. } | RCurve::SurfaceIntersection { .. } => CurveKind::Other,
    }
}

/// Length and arc-length midpoint of a polyline.
fn polyline_measure(pl: &[V3]) -> (f64, V3) {
    let total: f64 = pl.windows(2).map(|w| if let [a, b] = w { dist(*a, *b) } else { 0.0 }).sum();
    let mut run = 0.0;
    for w in pl.windows(2) {
        if let [a, b] = w {
            let l = dist(*a, *b);
            if run + l >= total / 2.0 && l > 0.0 {
                return (total, add(*a, scale(sub(*b, *a), (total / 2.0 - run) / l)));
            }
            run += l;
        }
    }
    (total, pl.first().copied().unwrap_or([0.0; 3]))
}

fn topology_of(s: &ScShape) -> KResult<Topology> {
    let fc = s.faceted()?;
    let r = &s.record;
    let faces = r
        .faces
        .iter()
        .zip(&fc.faces)
        .map(|(f, m)| {
            let kind = surface_kind(&f.surface);
            // The normal at the centroid: exact for planes; for curved faces, the normal of the
            // facet nearest the centroid.
            let normal = if kind == SurfaceKind::Plane {
                m.normal
            } else {
                m.tris
                    .iter()
                    .map(|[a, b, c]| {
                        (dist(scale(add(*a, add(*b, *c)), 1.0 / 3.0), m.centroid), cross(sub(*b, *a), sub(*c, *a)))
                    })
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map_or(m.normal, |(_, n)| unit(n))
            };
            FaceInfo { kind, area: m.area, centroid: m.centroid, normal, axis: surface_axis(&f.surface) }
        })
        .collect();
    let point = |v: u32| r.vertices.get(v as usize).map(|v| v.point);
    let mut edges = Vec::with_capacity(r.edges.len());
    for (i, e) in r.edges.iter().enumerate() {
        let [v0, v1] = e.vertices;
        let polyline = fc.edges.get(i).map(Vec::as_slice).unwrap_or(&[]);
        let (length, midpoint, circle) = match &e.curve {
            RCurve::Line { .. } => match (point(v0), point(v1)) {
                (Some(a), Some(b)) => (dist(a, b), scale(add(a, b), 0.5), None),
                _ => return Err(internal("an edge names a vertex the body does not have")),
            },
            RCurve::Circle { frame, radius } => {
                // The arc between the end vertices that passes the mesh polyline's middle (the
                // record's parameter interval is not an angle).
                let angle = |p: V3| {
                    let d = sub(p, frame.origin);
                    libm::atan2(dot(d, frame.y_axis), dot(d, frame.x_axis))
                };
                let (Some(a), Some(b)) = (point(v0), point(v1)) else {
                    return Err(internal("an edge names a vertex the body does not have"));
                };
                let a0 = angle(a);
                let (sweep, mid_angle) = if v0 == v1 {
                    (TAU, a0 + PI)
                } else {
                    let ccw = (angle(b) - a0).rem_euclid(TAU);
                    let through = polyline_measure(polyline).1;
                    if (angle(through) - a0).rem_euclid(TAU) < ccw {
                        (ccw, a0 + ccw / 2.0)
                    } else {
                        (TAU - ccw, a0 - (TAU - ccw) / 2.0)
                    }
                };
                let mid = add(
                    frame.origin,
                    add(
                        scale(frame.x_axis, radius * libm::cos(mid_angle)),
                        scale(frame.y_axis, radius * libm::sin(mid_angle)),
                    ),
                );
                let c =
                    Circle3 { center: frame.origin, normal: unit(cross(frame.x_axis, frame.y_axis)), radius: *radius };
                (radius * sweep, mid, Some(c))
            }
            _ => {
                let (l, m) = polyline_measure(polyline);
                (l, m, None)
            }
        };
        let mut faces: Vec<u32> = e.uses.iter().map(|u| u.face).collect();
        faces.sort_unstable();
        faces.dedup();
        let vertices = if v0 == v1 { vec![v0] } else { vec![v0, v1] };
        edges.push(EdgeInfo { kind: curve_kind(&e.curve), length, midpoint, faces, vertices, circle });
    }
    let vertices = r.vertices.iter().map(|v| VertexInfo { point: v.point }).collect();
    let solids = s.body.solid_count().map_err(|e| op_err("solid count", &e))?;
    Ok(Topology {
        faces,
        edges,
        vertices,
        solids: u32::try_from(solids).map_err(|_| internal("too many solids"))?,
        shells: u32::try_from(r.shells.len()).map_err(|_| internal("too many shells"))?,
    })
}

/// A finished result: checked by SolidCore's own validity tests, and measured once.
fn finish(body: ops::Body, origins: impl FnOnce(&ScShape) -> KResult<Vec<Vec<FaceOrigin>>>) -> KResult<OpResult> {
    let report = body.check(ops::CheckLevel::Geometry).map_err(|e| op_err("validity check", &e))?;
    if !report.is_valid() {
        return Err(err(
            KernelErrorKind::InvalidResult,
            format!("SolidCore's result fails its validity check: {}", report.text.lines().next().unwrap_or("")),
        ));
    }
    let shape = ScShape::new(body)?;
    let face_origins = origins(&shape)?;
    Ok(OpResult { shape: Shape::new(shape), face_origins })
}

// ---------------------------------------------------------------------------------------------
// Geometric face history.

/// The input faces each face of `result` lies on (same surface kind, its inner point within
/// the meshes' chords of the input face).
fn input_origins(result: &ScShape, inputs: &[&ScShape]) -> KResult<Vec<Vec<FaceOrigin>>> {
    let rf = result.faceted()?;
    let mut fins = Vec::with_capacity(inputs.len());
    for s in inputs {
        fins.push(s.faceted()?);
    }
    let mut out = Vec::with_capacity(rf.faces.len());
    for (f, fm) in rf.faces.iter().enumerate() {
        let kind = result.surface_kind(f);
        let mut origins = Vec::new();
        for (i, (s, fin)) in inputs.iter().zip(&fins).enumerate() {
            let tol = rf.chord + fin.chord + 1e-7 * result.size.max(s.size);
            for (g, gm) in fin.faces.iter().enumerate() {
                if s.surface_kind(g) != kind {
                    continue;
                }
                if gm.tris.iter().any(|[a, b, c]| point_triangle(fm.inner, *a, *b, *c) <= tol) {
                    let input = u32::try_from(i).map_err(|_| internal("too many inputs"))?;
                    let face = u32::try_from(g).map_err(|_| internal("too many faces"))?;
                    origins.push(FaceOrigin::Input { input, face });
                }
            }
        }
        origins.sort_unstable();
        origins.dedup();
        out.push(origins);
    }
    Ok(out)
}

/// Faces of a blend that lie on no input face come from the nearest blended edge.
fn blend_origins(result: &ScShape, input: &ScShape, edges: &[u32]) -> KResult<Vec<Vec<FaceOrigin>>> {
    let mut out = input_origins(result, &[input])?;
    let rf = result.faceted()?;
    let fin = input.faceted()?;
    for (os, fm) in out.iter_mut().zip(&rf.faces) {
        if !os.is_empty() {
            continue;
        }
        let nearest = edges
            .iter()
            .filter_map(|&e| {
                let pl = fin.edges.get(e as usize)?;
                let d = pl
                    .windows(2)
                    .filter_map(|w| if let [a, b] = w { Some(point_segment(fm.inner, *a, *b)) } else { None })
                    .fold(f64::INFINITY, f64::min);
                Some((d, e))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, edge)) = nearest {
            os.push(FaceOrigin::FromEdge { input: 0, edge });
        }
    }
    Ok(out)
}

/// Faces of a shell that lie on no input face are the inside offsets of the nearest input
/// face of the same kind.
fn shell_origins(result: &ScShape, input: &ScShape) -> KResult<Vec<Vec<FaceOrigin>>> {
    let mut out = input_origins(result, &[input])?;
    let rf = result.faceted()?;
    let fin = input.faceted()?;
    for (f, (os, fm)) in out.iter_mut().zip(&rf.faces).enumerate() {
        if !os.is_empty() {
            continue;
        }
        let kind = result.surface_kind(f);
        let nearest = fin
            .faces
            .iter()
            .enumerate()
            .filter(|(g, _)| input.surface_kind(*g) == kind)
            .map(|(g, gm)| {
                let d =
                    gm.tris.iter().map(|[a, b, c]| point_triangle(fm.inner, *a, *b, *c)).fold(f64::INFINITY, f64::min);
                (d, g)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, g)) = nearest {
            let face = u32::try_from(g).map_err(|_| internal("too many faces"))?;
            os.push(FaceOrigin::Offset { input: 0, face });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Profiles.

fn v2(p: P2) -> rec::Vec2 {
    rec::Vec2 { x: p[0], y: p[1] }
}

fn v3(p: V3) -> rec::Vec3 {
    rec::Vec3 { x: p[0], y: p[1], z: p[2] }
}

fn plane(f: &Frame) -> rec::Plane {
    rec::Plane { origin: v3(f.origin), x: v3(f.x), y: v3(f.y) }
}

fn seg(s: &Seg2) -> KResult<rec::Seg2> {
    Ok(match *s {
        Seg2::Line { a, b } => rec::Seg2::Line { a: v2(a), b: v2(b) },
        Seg2::Circle { center, radius } => rec::Seg2::Arc { center: v2(center), radius, start: 0.0, sweep: TAU },
        Seg2::Arc { center, start, end, ccw } => {
            let r = libm::hypot(start[0] - center[0], start[1] - center[1]);
            let a0 = libm::atan2(start[1] - center[1], start[0] - center[0]);
            let a1 = libm::atan2(end[1] - center[1], end[0] - center[0]);
            let ccw_sweep = (a1 - a0).rem_euclid(TAU);
            let sweep = if ccw {
                if ccw_sweep <= 1e-12 { TAU } else { ccw_sweep }
            } else {
                let cw = (a0 - a1).rem_euclid(TAU);
                -(if cw <= 1e-12 { TAU } else { cw })
            };
            if !(r.is_finite() && r > 0.0) {
                return Err(invalid("an arc of the profile has no radius"));
            }
            rec::Seg2::Arc { center: v2(center), radius: r, start: a0, sweep }
        }
        Seg2::Bezier { p } => {
            let [p0, p1, p2, p3] = p;
            rec::Seg2::Cubic { p0: v2(p0), p1: v2(p1), p2: v2(p2), p3: v2(p3) }
        }
    })
}

fn loop2(l: &gusset_kernel::Loop2) -> KResult<rec::Loop2> {
    Ok(rec::Loop2 { segs: l.edges.iter().map(|e| seg(&e.seg)).collect::<KResult<_>>()? })
}

fn regions(p: &Profile) -> KResult<Vec<rec::Region2>> {
    if !p.frame.is_valid() {
        return Err(invalid("the profile's frame is not orthonormal"));
    }
    if p.regions.is_empty() {
        return Err(invalid("the profile has no region"));
    }
    p.regions
        .iter()
        .map(|r| {
            Ok(rec::Region2 { outer: loop2(&r.outer)?, holes: r.holes.iter().map(loop2).collect::<KResult<_>>()? })
        })
        .collect()
}

/// Points along a profile segment (in frame coordinates), dense enough to measure distances.
fn seg_points(s: &Seg2) -> Vec<P2> {
    const N: usize = 64;
    let lerp = |t: f64, f: &dyn Fn(f64) -> P2| (0..=N).map(|i| f(t * i as f64 / N as f64)).collect::<Vec<_>>();
    match *s {
        Seg2::Line { a, b } => vec![a, b],
        Seg2::Circle { center, radius } => {
            lerp(TAU, &|t| [center[0] + radius * libm::cos(t), center[1] + radius * libm::sin(t)])
        }
        Seg2::Arc { .. } => match seg(s) {
            Ok(rec::Seg2::Arc { center, radius, start, sweep }) => lerp(1.0, &|u| {
                let t = start + sweep * u;
                [center.x + radius * libm::cos(t), center.y + radius * libm::sin(t)]
            }),
            _ => Vec::new(),
        },
        Seg2::Bezier { p: [p0, p1, p2, p3] } => lerp(1.0, &|t| {
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            [
                w[0] * p0[0] + w[1] * p1[0] + w[2] * p2[0] + w[3] * p3[0],
                w[0] * p0[1] + w[1] * p1[1] + w[2] * p2[1] + w[3] * p3[1],
            ]
        }),
    }
}

fn dist2(p: P2, s: &[P2]) -> f64 {
    s.windows(2)
        .filter_map(|w| {
            if let [a, b] = w {
                Some(point_segment([p[0], p[1], 0.0], [a[0], a[1], 0.0], [b[0], b[1], 0.0]))
            } else {
                None
            }
        })
        .fold(f64::INFINITY, f64::min)
}

/// The tag of the profile edge nearest to `p` (frame coordinates).
fn nearest_tag(profile: &Profile, p: P2) -> Option<u64> {
    profile
        .regions
        .iter()
        .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
        .flat_map(|l| &l.edges)
        .map(|e| (dist2(p, &seg_points(&e.seg)), e.tag))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

/// Whether `p` (frame coordinates) is inside a closed loop (even-odd on its sampled boundary).
fn inside(l: &gusset_kernel::Loop2, p: P2) -> bool {
    let pts: Vec<P2> = l.edges.iter().flat_map(|e| seg_points(&e.seg)).collect();
    let mut c = false;
    for w in pts.windows(2) {
        if let [a, b] = w
            && (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

fn region_of(profile: &Profile, p: P2) -> u32 {
    profile
        .regions
        .iter()
        .position(|r| inside(&r.outer, p) && !r.holes.iter().any(|h| inside(h, p)))
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

fn to_frame(f: &Frame, p: V3) -> P2 {
    let d = sub(p, f.origin);
    [dot(d, f.x), dot(d, f.y)]
}

// ---------------------------------------------------------------------------------------------
// Transforms.

/// `t` as SolidCore's affine matrix: rows 0-2 the images of x, y and z, row 3 the translation.
fn matrix(t: &Transform) -> KResult<[[f64; 4]; 4]> {
    let (a, tr): ([V3; 3], V3) = match *t {
        Transform::Translate(v) => ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], v),
        Transform::Rotate { origin, axis, angle } => {
            let k = unit(axis);
            if norm(axis) < 1e-12 {
                return Err(invalid("the rotation axis has no direction"));
            }
            let (s, c) = (libm::sin(angle), libm::cos(angle));
            // Rodrigues: the image of each basis vector.
            let img = |v: V3| add(add(scale(v, c), scale(cross(k, v), s)), scale(k, dot(k, v) * (1.0 - c)));
            let a = [img([1.0, 0.0, 0.0]), img([0.0, 1.0, 0.0]), img([0.0, 0.0, 1.0])];
            // p' = R (p - o) + o
            let ro = add(add(scale(a[0], origin[0]), scale(a[1], origin[1])), scale(a[2], origin[2]));
            (a, sub(origin, ro))
        }
        Transform::Mirror { origin, normal } => {
            let n = unit(normal);
            if norm(normal) < 1e-12 {
                return Err(invalid("the mirror plane has no normal"));
            }
            let img = |v: V3| sub(v, scale(n, 2.0 * dot(n, v)));
            let a = [img([1.0, 0.0, 0.0]), img([0.0, 1.0, 0.0]), img([0.0, 0.0, 1.0])];
            (a, scale(n, 2.0 * dot(n, origin)))
        }
    };
    let [x, y, z] = a;
    Ok([[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [tr[0], tr[1], tr[2], 1.0]])
}

// ---------------------------------------------------------------------------------------------

fn boolean_op(op: BoolOp) -> ops::BooleanOp {
    match op {
        BoolOp::Union => ops::BooleanOp::Union,
        BoolOp::Cut => ops::BooleanOp::Subtract,
        BoolOp::Intersect => ops::BooleanOp::Intersect,
    }
}

fn indices(ids: &[u32], len: usize, what: &str) -> KResult<Vec<usize>> {
    ids.iter()
        .map(|&i| {
            let i = i as usize;
            if i < len { Ok(i) } else { Err(invalid(format!("{what} {i} does not exist (the shape has {len})"))) }
        })
        .collect()
}

impl SolidcoreKernel {
    fn blend(&self, shape: &Shape, edges: &[u32], size: f64, kind: ops::Blend, what: &str) -> KResult<OpResult> {
        let s = sc(shape)?;
        if edges.is_empty() {
            return Err(invalid(format!("a {what} needs at least one edge")));
        }
        if !(size.is_finite() && size > 0.0) {
            return Err(invalid(format!("the {what} size must be a positive length")));
        }
        let es = indices(edges, s.record.edges.len(), "edge")?;
        let (body, order) = s.copy()?;
        let sizes = vec![size; es.len()];
        ops::blend_ordered(&body, &order, kind, &es, &sizes, &[], ops::Corners::Filled)
            .map_err(|e| op_err(what, &e))?;
        finish(body, |r| blend_origins(r, s, edges))
    }
}

impl Kernel for SolidcoreKernel {
    fn name(&self) -> &'static str {
        NAME
    }

    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> KResult<OpResult> {
        let d = spec.distance;
        if !(d.is_finite() && d.abs() > 1e-9) {
            return Err(invalid("the extrusion distance must be a non-zero length"));
        }
        let (start, end) = if spec.symmetric { (-d.abs() / 2.0, d.abs() / 2.0) } else { (0.0, d) };
        let (lo, hi) = (start.min(end), start.max(end));
        let body = ops::prism::extrude_planar_regions(&plane(&profile.frame), &regions(profile)?, lo, hi)
            .map_err(|e| op_err("extrude", &e))?;
        let n = profile.frame.normal();
        finish(body, |r| {
            let fc = r.faceted()?;
            let tol = 1e-6 * r.size.max(1.0);
            let mut out = Vec::with_capacity(fc.faces.len());
            for (f, fm) in fc.faces.iter().enumerate() {
                let h = dot(sub(fm.inner, profile.frame.origin), n);
                let p = to_frame(&profile.frame, fm.inner);
                let cap = r.surface_kind(f) == SurfaceKind::Plane && dot(fm.normal, n).abs() > 1.0 - 1e-9;
                let o = if cap && (h - start).abs() <= tol {
                    FaceOrigin::Cap { end: CapEnd::Start, region: region_of(profile, p) }
                } else if cap && (h - end).abs() <= tol {
                    FaceOrigin::Cap { end: CapEnd::End, region: region_of(profile, p) }
                } else {
                    let tag = nearest_tag(profile, p).ok_or_else(|| internal("the profile has no edges"))?;
                    FaceOrigin::Lateral { tag }
                };
                out.push(vec![o]);
            }
            Ok(out)
        })
    }

    fn revolve(&self, profile: &Profile, spec: &RevolveSpec) -> KResult<OpResult> {
        let angle = spec.angle;
        if !(angle.is_finite() && angle > 1e-9 && angle <= TAU * (1.0 + 1e-12)) {
            return Err(invalid("the revolve angle must be in (0, 2π]"));
        }
        let pl = plane(&profile.frame);
        let mut curves = Vec::new();
        for r in regions(profile)? {
            for l in r.loops() {
                for s in &l.segs {
                    curves.push(
                        ops::sweep::Curve::from_seg2(s, &pl)
                            .ok_or_else(|| invalid("a profile segment cannot be revolved"))?,
                    );
                }
            }
        }
        let f = &profile.frame;
        let origin = f.to_world(spec.axis_origin);
        let axis = unit(add(scale(f.x, spec.axis_dir[0]), scale(f.y, spec.axis_dir[1])));
        if norm(axis) < 0.5 {
            return Err(invalid("the revolve axis has no direction"));
        }
        let body = ops::sweep::revolve(&curves, origin, axis, angle, true).map_err(|e| op_err("revolve", &e))?;
        // The in-plane direction from the axis towards the profile: angle 0 of the revolve.
        let mut toward = cross(axis, f.normal());
        let probe = profile
            .regions
            .first()
            .and_then(|r| r.outer.edges.first())
            .map(|e| seg_points(&e.seg))
            .and_then(|pts| pts.get(pts.len() / 2).copied());
        if let Some(p) = probe
            && dot(sub(f.to_world(p), origin), toward) < 0.0
        {
            toward = scale(toward, -1.0);
        }
        let side = cross(axis, toward);
        let full = angle >= TAU * (1.0 - 1e-12);
        finish(body, |r| {
            let fc = r.faceted()?;
            let mut out = Vec::with_capacity(fc.faces.len());
            for (fi, fm) in fc.faces.iter().enumerate() {
                let w = sub(fm.inner, origin);
                let h = dot(w, axis);
                let radial = sub(w, scale(axis, h));
                let rho = norm(radial);
                let phi = libm::atan2(dot(radial, side), dot(radial, toward)).rem_euclid(TAU);
                let planar_through_axis = r.surface_kind(fi) == SurfaceKind::Plane && dot(fm.normal, axis).abs() < 1e-9;
                let o = if !full && planar_through_axis && !(1e-6..=TAU - 1e-6).contains(&phi) {
                    FaceOrigin::Cap { end: CapEnd::Start, region: 0 }
                } else if !full && planar_through_axis && (phi - angle).abs() < 1e-6 {
                    FaceOrigin::Cap { end: CapEnd::End, region: 0 }
                } else {
                    // Turn the point back into the profile plane.
                    let back = add(origin, add(scale(axis, h), scale(toward, rho)));
                    let tag =
                        nearest_tag(profile, to_frame(f, back)).ok_or_else(|| internal("the profile has no edges"))?;
                    FaceOrigin::Lateral { tag }
                };
                out.push(vec![o]);
            }
            Ok(out)
        })
    }

    fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> KResult<OpResult> {
        let (a, b) = (sc(target)?, sc(tool)?);
        let (ba, _) = a.copy()?;
        let (bb, _) = b.copy()?;
        let body = ops::boolean::boolean(ba, bb, boolean_op(op))
            .map_err(|e| op_err("boolean", &e))?
            .ok_or_else(|| err(KernelErrorKind::OperationFailed, "the boolean leaves nothing"))?;
        finish(body, |r| input_origins(r, &[a, b]))
    }

    fn boolean_many(&self, op: BoolOp, target: &Shape, tools: &[Shape]) -> KResult<OpResult> {
        if tools.is_empty() || op == BoolOp::Intersect {
            return gusset_kernel::sequential_boolean(self, op, target, tools);
        }
        let a = sc(target)?;
        let ts: Vec<&ScShape> = tools.iter().map(sc).collect::<KResult<_>>()?;
        let mut bodies = Vec::with_capacity(ts.len() + 1);
        if op == BoolOp::Union {
            bodies.push(a.copy()?.0);
        }
        for t in &ts {
            bodies.push(t.copy()?.0);
        }
        // Cut: the tools united first, then one subtraction (2x faster than one cut per tool on
        // the hole grid of docs/research/solidcore-2026-11.md).
        let united = ops::boolean::union_all(bodies).map_err(|e| op_err("union of the tools", &e))?;
        let body = if op == BoolOp::Cut {
            ops::boolean::boolean(a.copy()?.0, united, ops::BooleanOp::Subtract)
                .map_err(|e| op_err("boolean", &e))?
                .ok_or_else(|| err(KernelErrorKind::OperationFailed, "the boolean leaves nothing"))?
        } else {
            united
        };
        let mut inputs = vec![a];
        inputs.extend(ts);
        finish(body, |r| input_origins(r, &inputs))
    }

    fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> KResult<OpResult> {
        self.blend(shape, edges, radius, ops::Blend::Round, "fillet")
    }

    fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> KResult<OpResult> {
        self.blend(shape, edges, distance, ops::Blend::Chamfer, "chamfer")
    }

    fn shell(&self, shape: &Shape, remove: &[u32], thickness: f64) -> KResult<OpResult> {
        let s = sc(shape)?;
        if !(thickness.is_finite() && thickness.abs() > 1e-9) {
            return Err(invalid("the shell thickness must be a non-zero length"));
        }
        let open = indices(remove, s.record.faces.len(), "face")?;
        let (body, order) = s.copy()?;
        // Gusset: > 0 grows inward; SolidCore: negative is inward.
        ops::shell(&body, &order, &open, -thickness).map_err(|e| op_err("shell", &e))?;
        finish(body, |r| shell_origins(r, s))
    }

    fn transform(&self, shape: &Shape, t: &Transform) -> KResult<OpResult> {
        let s = sc(shape)?;
        let m = matrix(t)?;
        let (body, _) = s.copy()?;
        body.transform(&m).map_err(|e| op_err("transform", &e))?;
        let faces = s.record.faces.len();
        finish(body, |r| {
            if r.record.faces.len() != faces {
                return Err(internal("the transform changed the face count"));
            }
            (0..faces)
                .map(|f| u32::try_from(f).map(|face| vec![FaceOrigin::Input { input: 0, face }]))
                .collect::<Result<_, _>>()
                .map_err(|_| internal("too many faces"))
        })
    }

    fn compound(&self, _shapes: &[Shape]) -> KResult<OpResult> {
        Err(err(KernelErrorKind::Unsupported, "compound is not supported by the solidcore kernel"))
    }

    fn topology(&self, shape: &Shape) -> KResult<Topology> {
        topology_of(sc(shape)?)
    }

    fn mass_properties(&self, shape: &Shape) -> KResult<MassProps> {
        let m = sc(shape)?.body.mass_properties(MASS_ACCURACY).map_err(|e| op_err("mass properties", &e))?;
        Ok(MassProps { volume: m.volume, area: m.area, centroid: m.centroid })
    }

    fn bbox(&self, shape: &Shape) -> KResult<BBox> {
        let (min, max) = sc(shape)?.body.bounds().map_err(|e| op_err("bounding box", &e))?;
        Ok(BBox { min, max })
    }

    fn tessellate(&self, shape: &Shape, params: &TessellationParams) -> KResult<Mesh> {
        let s = sc(shape)?;
        let tol = solidcore_tess::MeshTolerance {
            chord: params.linear_deflection,
            angle: (params.angular_deflection.is_finite() && params.angular_deflection > 0.0)
                .then_some(params.angular_deflection.clamp(PI / 180.0, PI / 2.0)),
        };
        let (plan, _) = solidcore_tess::plan(&s.record, &tol, s.size)
            .map_err(|e| err(KernelErrorKind::OperationFailed, format!("SolidCore could not mesh the body: {e}")))?;
        let (m, refused) = plan.evaluate();
        if !refused.is_empty() {
            return Err(err(
                KernelErrorKind::OperationFailed,
                format!("SolidCore could not mesh {} face(s)", refused.len()),
            ));
        }
        let f32s = |v: &rec::Vec3| [v.x as f32, v.y as f32, v.z as f32];
        let mut by_face: Vec<Vec<[u32; 3]>> = vec![Vec::new(); s.record.faces.len()];
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            by_face
                .get_mut(*f as usize)
                .ok_or_else(|| internal("the mesh names a face the body does not have"))?
                .push(*t);
        }
        let mut indices = Vec::with_capacity(m.triangles.len() * 3);
        let mut face_ranges = Vec::with_capacity(by_face.len());
        for tris in &by_face {
            let first = u32::try_from(indices.len() / 3).map_err(|_| internal("mesh too large"))?;
            for t in tris {
                indices.extend_from_slice(t);
            }
            let last = u32::try_from(indices.len() / 3).map_err(|_| internal("mesh too large"))?;
            face_ranges.push(first..last);
        }
        Ok(Mesh {
            positions: m.positions.iter().map(f32s).collect(),
            normals: m.normals.iter().map(f32s).collect(),
            indices,
            face_ranges,
            edges: m.edges.iter().map(|pl| pl.iter().map(f32s).collect()).collect(),
        })
    }
}
