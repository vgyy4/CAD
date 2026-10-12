//! Closed-region ("profile") extraction from a solved sketch: the planar arrangement
//! of its curves.
//!
//! Non-construction lines, arcs and circles are merged at shared endpoints (within
//! `tol`), cut at every crossing, touch or T-junction ([`crate::arrange`]) and turned
//! into a planar graph whose edges are *pieces* of the entities. Edges that do not bound
//! two different faces (dangling stubs, bridges between loops) are dropped. Faces of the
//! rest are traced with the usual "turn most clockwise" rule; every bounded face becomes
//! a profile whose holes are the outer boundaries of the components nested directly
//! inside it. Results are ordered deterministically.

use std::collections::BTreeMap;
use std::fmt;

use crate::arrange::{self, Verts};
use crate::geom2::{V2, angle, cross, dist, dot, not_gt, sub, wrap_2pi};
use crate::sketch::{EntityId, Geom, Sketch, SketchError};
use crate::spline::{self, Bez};

/// Most non-construction curves [`profiles`] accepts.
pub const MAX_PROFILE_CURVES: usize = 10_000;
/// Smallest allowed `tol` for [`profiles`] (mm).
pub const MIN_PROFILE_TOL: f64 = 1e-9;
/// Largest allowed `tol` for [`profiles`] (mm).
pub const MAX_PROFILE_TOL: f64 = 1.0;

const TAU: f64 = core::f64::consts::TAU;
const PI: f64 = core::f64::consts::PI;

/// Oriented geometry of one profile segment, in the loop's direction of travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    Line {
        start: [f64; 2],
        end: [f64; 2],
    },
    /// `ccw == false` means the arc is travelled clockwise from `start` to `end`.
    Arc {
        center: [f64; 2],
        radius: f64,
        start: [f64; 2],
        end: [f64; 2],
        ccw: bool,
    },
    Circle {
        center: [f64; 2],
        radius: f64,
        ccw: bool,
    },
    /// One cubic Bézier segment of a spline, with its four control points. A spline piece
    /// is several of these in a row (see [`Segment::sub`]).
    Bezier {
        p: [[f64; 2]; 4],
    },
}

/// One segment of a loop, referring to the sketch entity it comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub entity: EntityId,
    /// Which piece of the entity this is: the entity is cut at its crossings and the
    /// pieces are numbered from 0 along it (lines from `a`, arcs from `start`, circles
    /// from their canonical first cut). An uncut entity has piece 0.
    pub piece: u32,
    /// For a spline: which Bézier segment of the spline this is (segment `k` runs from fit
    /// point `k` to the next one); 0 for every other entity.
    pub sub: u32,
    /// True when the loop runs against the entity's own direction (line `b → a`,
    /// arc clockwise, circle clockwise).
    pub reversed: bool,
    pub curve: Curve,
}

/// A closed chain of segments; each segment ends where the next one starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub segments: Vec<Segment>,
}

impl Loop {
    /// Signed area (positive for counter-clockwise loops).
    pub fn signed_area(&self) -> f64 {
        self.segments.iter().map(seg_area).sum()
    }

    /// Entity ids of the segments, in loop order.
    pub fn entities(&self) -> Vec<EntityId> {
        self.segments.iter().map(|s| s.entity).collect()
    }
}

/// A closed region: a counter-clockwise outer loop and clockwise hole loops.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub outer: Loop,
    pub holes: Vec<Loop>,
    /// Enclosed area (outer minus holes), mm².
    pub area: f64,
    /// `(entity, piece)` of every segment of the outer loop, sorted. This is what
    /// identifies the region across parameter edits (see [`Arrangement::crossings`]).
    pub pieces: Vec<(EntityId, u32)>,
    /// A point strictly inside the region (and outside its holes).
    pub label: [f64; 2],
}

/// The arrangement of a sketch's curves.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrangement {
    /// Every bounded face, ordered by the `(entity, piece, reversed)` key of its outer loop.
    pub profiles: Vec<Profile>,
    /// Entities with a free end (an endpoint where nothing else attaches). Their dangling
    /// pieces bound no region and are ignored.
    pub open_ends: Vec<EntityId>,
    /// Some curve is cut in its interior by another one. When false, every region is
    /// bounded by whole entities (the classic "closed outline" case).
    pub crossings: bool,
}

/// Why profiles could not be extracted.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileError {
    Invalid(SketchError),
    /// `tol` is not within [`MIN_PROFILE_TOL`]..=[`MAX_PROFILE_TOL`].
    BadTolerance,
    TooManyCurves {
        count: usize,
        limit: usize,
    },
    /// A zero-length line, zero-radius circle/arc, an arc whose end is not on its
    /// circle, or an arc whose start and end coincide.
    Degenerate {
        entity: EntityId,
    },
    /// No closed region, and geometry with free ends (the entities touching a dangling
    /// endpoint).
    Open {
        entities: Vec<EntityId>,
    },
    /// Two curves overlap along a stretch (instead of crossing at points).
    SelfIntersecting {
        a: EntityId,
        b: EntityId,
        at: [f64; 2],
    },
    /// More vertices than the arrangement may hold.
    TooComplex {
        limit: usize,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::Invalid(e) => write!(f, "invalid sketch: {e}"),
            ProfileError::BadTolerance => {
                write!(f, "tolerance must be between {MIN_PROFILE_TOL} and {MAX_PROFILE_TOL} mm")
            }
            ProfileError::TooManyCurves { count, limit } => write!(f, "{count} curves (limit {limit})"),
            ProfileError::Degenerate { entity } => write!(f, "entity {entity} is degenerate"),
            ProfileError::Open { entities } => {
                write!(f, "sketch is not closed; open ends at")?;
                for e in entities {
                    write!(f, " {e}")?;
                }
                Ok(())
            }
            ProfileError::SelfIntersecting { a, b, at } => {
                write!(f, "{a} and {b} overlap along a stretch near ({}, {})", at[0], at[1])
            }
            ProfileError::TooComplex { limit } => write!(f, "the sketch has more than {limit} crossings"),
        }
    }
}

impl std::error::Error for ProfileError {}

#[derive(Debug, Clone)]
pub(crate) enum Shape {
    Line,
    /// `a0`: start angle, `span`: CCW sweep in `(0, 2π)`.
    Arc {
        c: V2,
        r: f64,
        a0: f64,
        span: f64,
    },
    Circle {
        c: V2,
        r: f64,
    },
    /// A chain of cubic Béziers (a whole spline, or a piece of one with `closed: false`).
    /// `chain[j]` is segment `(first + j) % total` of the spline.
    Spline {
        chain: Vec<Bez>,
        first: u32,
        total: u32,
        closed: bool,
    },
}

impl Shape {
    /// A curve without end points: a circle or a closed spline.
    pub(crate) fn is_ring(&self) -> bool {
        matches!(self, Shape::Circle { .. } | Shape::Spline { closed: true, .. })
    }
}

/// A curve of the sketch, or (after cutting) one piece of it.
#[derive(Debug, Clone)]
pub(crate) struct Cv {
    pub(crate) id: EntityId,
    pub(crate) shape: Shape,
    /// Endpoint positions (unused for circles).
    pub(crate) p: [V2; 2],
    /// Merged vertex index of each endpoint (`usize::MAX` for circles).
    pub(crate) v: [usize; 2],
    pub(crate) bbox: [f64; 4],
    /// Number of this piece along its entity.
    pub(crate) piece: u32,
}

pub(crate) fn bbox_of(shape: &Shape, p: [V2; 2]) -> [f64; 4] {
    match shape {
        Shape::Line => [p[0][0].min(p[1][0]), p[0][1].min(p[1][1]), p[0][0].max(p[1][0]), p[0][1].max(p[1][1])],
        Shape::Arc { c, r, .. } | Shape::Circle { c, r } => [c[0] - r, c[1] - r, c[0] + r, c[1] + r],
        Shape::Spline { chain, .. } => {
            let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for q in chain.iter().flatten() {
                b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
            }
            b
        }
    }
}

fn pt_of(s: &Sketch, id: EntityId) -> Result<V2, ProfileError> {
    s.point(id).ok_or(ProfileError::Invalid(SketchError::UnknownEntity(id)))
}

pub(crate) fn collect(s: &Sketch, tol: f64) -> Result<Vec<Cv>, ProfileError> {
    let mut out = Vec::new();
    for e in s.entities() {
        if e.construction {
            continue;
        }
        let deg = ProfileError::Degenerate { entity: e.id };
        let (shape, p) = match e.geom {
            Geom::Point { .. } => continue,
            Geom::Line { a, b } => {
                let (pa, pb) = (pt_of(s, a)?, pt_of(s, b)?);
                if not_gt(dist(pa, pb), tol) {
                    return Err(deg);
                }
                (Shape::Line, [pa, pb])
            }
            Geom::Circle { center, radius } => {
                let c = pt_of(s, center)?;
                if not_gt(radius, tol) {
                    return Err(deg);
                }
                (Shape::Circle { c, r: radius }, [c, c])
            }
            Geom::Arc { center, start, end } => {
                let (c, ps, pe) = (pt_of(s, center)?, pt_of(s, start)?, pt_of(s, end)?);
                let r = dist(ps, c);
                if not_gt(r, tol) || (dist(pe, c) - r).abs() > tol || not_gt(dist(ps, pe), tol) {
                    return Err(deg);
                }
                let a0 = angle(sub(ps, c));
                let span = wrap_2pi(angle(sub(pe, c)) - a0);
                if not_gt(span, 0.0) {
                    return Err(deg);
                }
                (Shape::Arc { c, r, a0, span }, [ps, pe])
            }
            Geom::Spline { closed, .. } => {
                let fit: Vec<V2> =
                    s.spline_points(e.id).unwrap_or(&[]).iter().map(|&q| pt_of(s, q)).collect::<Result<Vec<_>, _>>()?;
                let n = fit.len();
                let gap = |i: usize| match (fit.get(i), fit.get((i + 1) % n.max(1))) {
                    (Some(&a), Some(&b)) => dist(a, b),
                    _ => 0.0,
                };
                let last = if closed { n } else { n.saturating_sub(1) };
                if n < 3 || (0..last).any(|i| not_gt(gap(i), tol)) {
                    return Err(deg);
                }
                let chain = spline::chain(&fit, closed);
                let ends = match (chain.first(), chain.last()) {
                    (Some(f), Some(l)) => [f[0], l[3]],
                    _ => return Err(deg),
                };
                let total = u32::try_from(chain.len()).unwrap_or(u32::MAX);
                (Shape::Spline { chain, first: 0, total, closed }, ends)
            }
        };
        if out.len() >= MAX_PROFILE_CURVES {
            let count =
                s.entities().iter().filter(|e| !e.construction && !matches!(e.geom, Geom::Point { .. })).count();
            return Err(ProfileError::TooManyCurves { count, limit: MAX_PROFILE_CURVES });
        }
        let bbox = bbox_of(&shape, p);
        out.push(Cv { id: e.id, shape, p, v: [usize::MAX; 2], bbox, piece: 0 });
    }
    Ok(out)
}

/// A curve used in a loop, in a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Use {
    cv: usize,
    rev: bool,
}

/// The loop segments for one use of a curve: one, except for a spline piece (one per Bézier).
fn make_segments(cvs: &[Cv], verts: &[V2], u: Use) -> Option<Vec<Segment>> {
    let cv = cvs.get(u.cv)?;
    if let Shape::Spline { chain, first, total, .. } = &cv.shape {
        let n = chain.len();
        let make = |j: usize, p: [V2; 4]| Segment {
            entity: cv.id,
            piece: cv.piece,
            sub: ((*first as usize + j) % (*total as usize).max(1)) as u32,
            reversed: u.rev,
            curve: Curve::Bezier { p },
        };
        return Some(if u.rev {
            chain.iter().enumerate().rev().map(|(j, b)| make(j, [b[3], b[2], b[1], b[0]])).collect()
        } else {
            let _ = n;
            chain.iter().enumerate().map(|(j, b)| make(j, *b)).collect()
        });
    }
    make_segment(cvs, verts, u).map(|s| vec![s])
}

fn make_segment(cvs: &[Cv], verts: &[V2], u: Use) -> Option<Segment> {
    let cv = cvs.get(u.cv)?;
    let vp = |k: usize| cv.v.get(k).and_then(|&v| verts.get(v)).copied();
    let curve = match &cv.shape {
        Shape::Line => {
            let (s, e) = (vp(0)?, vp(1)?);
            if u.rev { Curve::Line { start: e, end: s } } else { Curve::Line { start: s, end: e } }
        }
        Shape::Arc { c, r, .. } => {
            let (s, e) = (vp(0)?, vp(1)?);
            if u.rev {
                Curve::Arc { center: *c, radius: *r, start: e, end: s, ccw: false }
            } else {
                Curve::Arc { center: *c, radius: *r, start: s, end: e, ccw: true }
            }
        }
        Shape::Circle { c, r } => Curve::Circle { center: *c, radius: *r, ccw: !u.rev },
        // Splines are handled by `make_segments`.
        Shape::Spline { .. } => return None,
    };
    Some(Segment { entity: cv.id, piece: cv.piece, sub: 0, reversed: u.rev, curve })
}

fn seg_area(s: &Segment) -> f64 {
    match &s.curve {
        Curve::Line { start, end } => 0.5 * cross(*start, *end),
        Curve::Arc { center, radius, start, end, ccw } => {
            // Area term of the underlying counter-clockwise arc (s0 → e0).
            let (s0, e0) = if *ccw { (*start, *end) } else { (*end, *start) };
            let span = wrap_2pi(angle(sub(e0, *center)) - angle(sub(s0, *center)));
            let t = 0.5 * (radius * radius * span + center[0] * (e0[1] - s0[1]) - center[1] * (e0[0] - s0[0]));
            if *ccw { t } else { -t }
        }
        Curve::Circle { radius, ccw, .. } => {
            let a = PI * radius * radius;
            if *ccw { a } else { -a }
        }
        Curve::Bezier { p } => spline::area_term(p),
    }
}

fn line_angle(p: V2, a: V2, b: V2) -> f64 {
    let (u, v) = (sub(a, p), sub(b, p));
    libm::atan2(cross(u, v), dot(u, v))
}

/// Angle swept around `p` by one segment (exact for arcs and circles).
fn seg_winding(s: &Segment, p: V2) -> f64 {
    match &s.curve {
        Curve::Line { start, end } => line_angle(p, *start, *end),
        Curve::Bezier { p: bez } => {
            let pts = spline::flatten(std::slice::from_ref(bez), 32);
            pts.windows(2).map(|w| if let &[a, b] = w { line_angle(p, a, b) } else { 0.0 }).sum()
        }
        Curve::Arc { center, radius, start, end, ccw } => {
            let (ccw, center, radius) = (*ccw, *center, *radius);
            let (s0, e0) = if ccw { (*start, *end) } else { (*end, *start) };
            let base = line_angle(p, s0, e0);
            let inside = dist(p, center) < radius;
            let side = cross(sub(e0, s0), sub(p, s0));
            let fwd = if inside && side < 0.0 {
                base + TAU
            } else if inside && side == 0.0 && dot(sub(s0, p), sub(e0, p)) < 0.0 {
                PI
            } else {
                base
            };
            if ccw { fwd } else { -fwd }
        }
        Curve::Circle { center, radius, ccw } => {
            let w = if dist(p, *center) < *radius { TAU } else { 0.0 };
            if *ccw { w } else { -w }
        }
    }
}

fn winding(l: &Loop, p: V2) -> i64 {
    let total: f64 = l.segments.iter().map(|s| seg_winding(s, p)).sum();
    libm::round(total / TAU) as i64
}

fn loop_bbox(l: &Loop) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut grow = |p: V2, r: f64| {
        b[0] = b[0].min(p[0] - r);
        b[1] = b[1].min(p[1] - r);
        b[2] = b[2].max(p[0] + r);
        b[3] = b[3].max(p[1] + r);
    };
    for s in &l.segments {
        match &s.curve {
            Curve::Line { start, end } => {
                grow(*start, 0.0);
                grow(*end, 0.0);
            }
            Curve::Arc { center, radius, .. } | Curve::Circle { center, radius, .. } => grow(*center, *radius),
            Curve::Bezier { p } => p.iter().for_each(|q| grow(*q, 0.0)),
        }
    }
    b
}

type SegKey = (EntityId, u32, u32, bool);

fn seg_key(s: &Segment) -> SegKey {
    (s.entity, s.piece, s.sub, s.reversed)
}

/// Rotates a loop to start at its smallest `(entity, piece, reversed)` segment.
fn canonical(mut l: Loop) -> Loop {
    let start = l.segments.iter().enumerate().min_by_key(|(_, s)| seg_key(s)).map_or(0, |(i, _)| i);
    l.segments.rotate_left(start);
    l
}

fn key(l: &Loop) -> SegKey {
    l.segments.first().map_or((EntityId(u32::MAX), u32::MAX, u32::MAX, true), seg_key)
}

/// Half-edge `h` of edge `k`: `2k` runs from vertex `v[0]` to `v[1]`, `2k+1` back.
struct Graph {
    /// Index into the edge list of each graph edge.
    edge: Vec<usize>,
    /// Outgoing half-edges of each vertex, sorted counter-clockwise.
    out: Vec<Vec<usize>>,
    /// Position of each half-edge in its origin's `out` list.
    pos: Vec<usize>,
}

impl Graph {
    fn origin(&self, edges: &[Cv], h: usize) -> Option<usize> {
        edges.get(*self.edge.get(h / 2)?)?.v.get(h % 2).copied()
    }

    fn next(&self, edges: &[Cv], h: usize) -> Option<usize> {
        let t = h ^ 1;
        let list = self.out.get(self.origin(edges, t)?)?;
        let i = *self.pos.get(t)?;
        list.get((i + list.len() - 1) % list.len()).copied()
    }
}

/// Outgoing tangent direction and signed curvature of half-edge `h`.
fn tangent(cv: &Cv, verts: &[V2], rev: bool) -> (f64, f64) {
    let vp = |k: usize| cv.v.get(k).and_then(|&v| verts.get(v)).copied().unwrap_or([0.0, 0.0]);
    match &cv.shape {
        Shape::Line => {
            let d = if rev { sub(vp(0), vp(1)) } else { sub(vp(1), vp(0)) };
            (angle(d), 0.0)
        }
        Shape::Arc { c, r, .. } => {
            if rev {
                let q = sub(vp(1), *c);
                (angle([q[1], -q[0]]), -1.0 / *r)
            } else {
                let q = sub(vp(0), *c);
                (angle([-q[1], q[0]]), 1.0 / *r)
            }
        }
        Shape::Circle { .. } => (0.0, 0.0),
        Shape::Spline { chain, .. } => match (chain.first(), chain.last()) {
            (Some(first), Some(_)) if !rev => {
                (angle(spline::tangent_dir(first, false)), spline::curvature(first, false))
            }
            (Some(_), Some(last)) => {
                let d = spline::tangent_dir(last, true);
                (angle([-d[0], -d[1]]), -spline::curvature(last, true))
            }
            _ => (0.0, 0.0),
        },
    }
}

fn build_graph(edges: &[Cv], active: &[usize], verts: &[V2]) -> Graph {
    let mut out: Vec<Vec<(i64, f64, usize)>> = vec![Vec::new(); verts.len()];
    for (k, &ci) in active.iter().enumerate() {
        let Some(cv) = edges.get(ci) else { continue };
        for side in 0..2usize {
            let (ang, curv) = tangent(cv, verts, side == 1);
            let mut q = libm::round(ang * 1e9) as i64;
            if q == libm::round(-PI * 1e9) as i64 {
                q = libm::round(PI * 1e9) as i64;
            }
            if let Some(list) = cv.v.get(side).and_then(|&v| out.get_mut(v)) {
                list.push((q, curv, 2 * k + side));
            }
        }
    }
    let mut pos = vec![0usize; 2 * active.len()];
    let mut sorted = Vec::with_capacity(out.len());
    for mut list in out {
        list.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        for (i, &(_, _, h)) in list.iter().enumerate() {
            if let Some(p) = pos.get_mut(h) {
                *p = i;
            }
        }
        sorted.push(list.into_iter().map(|e| e.2).collect());
    }
    Graph { edge: active.to_vec(), out: sorted, pos }
}

/// Traces every face. Returns the half-edges of each face, and the face of each half-edge.
fn trace_faces(g: &Graph, edges: &[Cv]) -> (Vec<Vec<usize>>, Vec<usize>) {
    let nh = 2 * g.edge.len();
    let mut face_of = vec![usize::MAX; nh];
    let mut faces: Vec<Vec<usize>> = Vec::new();
    for h0 in 0..nh {
        if face_of.get(h0).copied() != Some(usize::MAX) {
            continue;
        }
        let mut face = Vec::new();
        let mut h = h0;
        for _ in 0..=nh {
            if let Some(v) = face_of.get_mut(h) {
                *v = faces.len();
            }
            face.push(h);
            match g.next(edges, h) {
                Some(n) if n != h0 => h = n,
                _ => break,
            }
        }
        faces.push(face);
    }
    (faces, face_of)
}

/// Samples a loop as a polygon (arcs and circles approximated).
fn polygon(l: &Loop) -> Vec<V2> {
    let mut pts = Vec::new();
    for s in &l.segments {
        match &s.curve {
            Curve::Line { start, .. } => pts.push(*start),
            Curve::Bezier { p } => {
                let mut f = spline::flatten(std::slice::from_ref(p), 12);
                f.pop();
                pts.extend(f);
            }
            Curve::Arc { center, radius, start, end, ccw } => {
                let (center, radius, start, end, ccw) = (*center, *radius, *start, *end, *ccw);
                let a0 = angle(sub(start, center));
                let sweep =
                    if ccw { wrap_2pi(angle(sub(end, center)) - a0) } else { -wrap_2pi(a0 - angle(sub(end, center))) };
                const N: usize = 24;
                for k in 0..N {
                    let t = a0 + sweep * k as f64 / N as f64;
                    pts.push([center[0] + radius * libm::cos(t), center[1] + radius * libm::sin(t)]);
                }
            }
            Curve::Circle { center, radius, .. } => {
                let (center, radius) = (*center, *radius);
                const N: usize = 64;
                for k in 0..N {
                    let t = TAU * k as f64 / N as f64;
                    pts.push([center[0] + radius * libm::cos(t), center[1] + radius * libm::sin(t)]);
                }
            }
        }
    }
    pts
}

/// A point inside the region: the middle of the widest stretch of a few horizontal scan
/// lines that lies inside the outer loop and outside the holes.
fn interior_point(outer: &Loop, holes: &[Loop]) -> V2 {
    let polys: Vec<Vec<V2>> = std::iter::once(outer).chain(holes).map(polygon).collect();
    let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
    for q in polys.first().into_iter().flatten() {
        y0 = y0.min(q[1]);
        y1 = y1.max(q[1]);
    }
    let mut best: Option<(f64, V2)> = None;
    for frac in [0.5, 0.37, 0.63, 0.23, 0.77, 0.11, 0.89] {
        let y = y0 + frac * (y1 - y0);
        let mut xs: Vec<f64> = Vec::new();
        for poly in &polys {
            for (i, &p) in poly.iter().enumerate() {
                let q = poly.get((i + 1) % poly.len()).copied().unwrap_or(p);
                if (p[1] <= y) != (q[1] <= y) {
                    xs.push(p[0] + (y - p[1]) * (q[0] - p[0]) / (q[1] - p[1]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for w in xs.chunks_exact(2) {
            if let &[x0, x1] = w
                && best.is_none_or(|(width, _)| x1 - x0 > width)
            {
                best = Some((x1 - x0, [0.5 * (x0 + x1), y]));
            }
        }
    }
    best.map_or_else(
        || {
            outer.segments.first().map_or([0.0, 0.0], |s| match &s.curve {
                Curve::Line { start, .. } | Curve::Arc { start, .. } => *start,
                Curve::Circle { center, radius, .. } => [center[0] + radius, center[1]],
                Curve::Bezier { p } => p[0],
            })
        },
        |(_, p)| p,
    )
}

/// Closed regions of a (solved) sketch, ignoring construction geometry.
///
/// `tol` (mm) is the distance below which endpoints are considered joined; it
/// must be in [`MIN_PROFILE_TOL`]..=[`MAX_PROFILE_TOL`]. See [`arrange`]; this is its
/// regions, with "no region but free ends" reported as [`ProfileError::Open`].
pub fn profiles(sketch: &Sketch, tol: f64) -> Result<Vec<Profile>, ProfileError> {
    let a = arrange(sketch, tol)?;
    if a.profiles.is_empty() && !a.open_ends.is_empty() {
        return Err(ProfileError::Open { entities: a.open_ends });
    }
    Ok(a.profiles)
}

/// The planar arrangement of the non-construction curves of a (solved) sketch.
///
/// Curves are cut wherever they cross, touch or meet at a T-junction; every bounded
/// face is a region, whatever combination of pieces bounds it. Dangling stubs and
/// bridges bound nothing and are ignored (the entities with free ends are reported in
/// [`Arrangement::open_ends`]). Profiles are ordered by the `(entity, piece, reversed)`
/// key of their outer loop's smallest segment; every loop starts at its smallest such key.
/// Points are ignored. Two curves must not overlap along a stretch.
pub fn arrange(sketch: &Sketch, tol: f64) -> Result<Arrangement, ProfileError> {
    if !(MIN_PROFILE_TOL..=MAX_PROFILE_TOL).contains(&tol) {
        return Err(ProfileError::BadTolerance);
    }
    sketch.validate().map_err(ProfileError::Invalid)?;
    let mut cvs = collect(sketch, tol)?;
    let mut verts = Verts::new(tol);
    for cv in &mut cvs {
        if cv.shape.is_ring() {
            continue;
        }
        for k in 0..2 {
            if let (Some(&p), Some(slot)) = (cv.p.get(k), cv.v.get_mut(k)) {
                *slot = verts.intern(p)?;
            }
        }
        if let Some(at) = crate::curves::self_crossing(cv) {
            return Err(ProfileError::SelfIntersecting { a: cv.id, b: cv.id, at });
        }
    }
    for cv in cvs.iter().filter(|c| c.shape.is_ring()) {
        if let Some(at) = crate::curves::self_crossing(cv) {
            return Err(ProfileError::SelfIntersecting { a: cv.id, b: cv.id, at });
        }
    }
    let splits = arrange::find_splits(&cvs, &mut verts, tol)?;
    let crossings = splits.crossings;
    let mut edges: Vec<Cv> = Vec::new();
    let mut circles: Vec<Cv> = Vec::new();
    for (cv, sp) in cvs.iter().zip(splits.per_curve) {
        if let Some(c) = arrange::make_pieces(cv, sp, &mut verts, &mut edges)? {
            circles.push(c);
        }
    }
    arrange::check_pieces(&edges)?;
    let verts = verts.pts;

    // Drop edges that have the same face on both sides (dangling stubs and bridges); they
    // bound nothing. Removing them does not change the other faces.
    let mut active: Vec<usize> = (0..edges.len()).collect();
    let mut open_ends: Vec<EntityId> = Vec::new();
    let mut first = true;
    let (g, faces) = loop {
        let g = build_graph(&edges, &active, &verts);
        if first {
            first = false;
            for list in &g.out {
                if let [h] = list.as_slice()
                    && let Some(cv) = g.edge.get(h / 2).and_then(|&ci| edges.get(ci))
                {
                    open_ends.push(cv.id);
                }
            }
            open_ends.sort_unstable();
            open_ends.dedup();
        }
        let (faces, face_of) = trace_faces(&g, &edges);
        let keep: Vec<usize> = g
            .edge
            .iter()
            .enumerate()
            .filter(|(k, _)| face_of.get(2 * k) != face_of.get(2 * k + 1))
            .map(|(_, &ci)| ci)
            .collect();
        if keep.len() == active.len() {
            break (g, faces);
        }
        active = keep;
    };

    // Connected components of the edge graph (union-find over vertices).
    let mut parent: Vec<usize> = (0..verts.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        for _ in 0..p.len() {
            let q = p.get(i).copied().unwrap_or(i);
            if q == i {
                break;
            }
            let gq = p.get(q).copied().unwrap_or(q);
            if let Some(s) = p.get_mut(i) {
                *s = gq;
            }
            i = gq;
        }
        i
    }
    for &ci in &g.edge {
        if let Some(cv) = edges.get(ci) {
            let (a, b) = (find(&mut parent, cv.v[0]), find(&mut parent, cv.v[1]));
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            if let Some(s) = parent.get_mut(hi) {
                *s = lo;
            }
        }
    }

    struct Face {
        lp: Loop,
        area: f64,
        bbox: [f64; 4],
        comp: usize,
    }
    struct Comp {
        sample: V2,
        outer: Option<Loop>,
    }
    let mut comp_index: BTreeMap<usize, usize> = BTreeMap::new();
    let mut comps: Vec<Comp> = Vec::new();
    let mut bounded: Vec<Face> = Vec::new();
    let mut outer_face: BTreeMap<usize, (f64, Loop)> = BTreeMap::new();
    for face in &faces {
        let segs: Option<Vec<Vec<Segment>>> = face
            .iter()
            .map(|&h| make_segments(&edges, &verts, Use { cv: *g.edge.get(h / 2)?, rev: h % 2 == 1 }))
            .collect();
        let Some(segs) = segs.map(|v| v.concat()) else { continue };
        let Some(v0) = face.first().and_then(|&h| g.origin(&edges, h)) else { continue };
        let root = find(&mut parent, v0);
        let comp = *comp_index.entry(root).or_insert_with(|| {
            comps.push(Comp { sample: verts.get(root).copied().unwrap_or([0.0, 0.0]), outer: None });
            comps.len() - 1
        });
        let lp = Loop { segments: segs };
        let area = lp.signed_area();
        // The most negative face of each component is its outer boundary.
        match outer_face.get(&comp) {
            Some((a, _)) if *a <= area => {
                if area > 0.0 {
                    bounded.push(Face { bbox: loop_bbox(&lp), lp, area, comp });
                }
            }
            _ => {
                if let Some((a, prev)) = outer_face.insert(comp, (area, lp))
                    && a > 0.0
                {
                    bounded.push(Face { bbox: loop_bbox(&prev), lp: prev, area: a, comp });
                }
            }
        }
    }
    for (comp, (_, lp)) in outer_face {
        if let Some(c) = comps.get_mut(comp) {
            c.outer = Some(lp);
        }
    }
    // Circles and closed splines that nothing touches are components of their own.
    for (i, cv) in circles.iter().enumerate() {
        let (sample, ccw) = match &cv.shape {
            Shape::Circle { c, r } => ([c[0] + r, c[1]], true),
            Shape::Spline { chain, .. } => {
                (chain.first().map_or([0.0, 0.0], |b| b[0]), chain.iter().map(spline::area_term).sum::<f64>() > 0.0)
            }
            _ => continue,
        };
        let comp = comps.len();
        let mk = |rev| make_segments(&circles, &verts, Use { cv: i, rev });
        let (Some(fwd), Some(bwd)) = (mk(false), mk(true)) else { continue };
        let (face_seg, boundary_seg) = if ccw { (fwd, bwd) } else { (bwd, fwd) };
        comps.push(Comp { sample, outer: Some(Loop { segments: boundary_seg }) });
        let lp = Loop { segments: face_seg };
        bounded.push(Face { bbox: loop_bbox(&lp), area: lp.signed_area(), lp, comp });
    }

    // Nesting: each component's outer boundary is a hole of the smallest bounded
    // face of another component that contains it.
    let mut holes: Vec<Vec<Loop>> = (0..bounded.len()).map(|_| Vec::new()).collect();
    for (ci, comp) in comps.iter().enumerate() {
        let p = comp.sample;
        let mut best: Option<(f64, usize)> = None;
        for (fi, f) in bounded.iter().enumerate() {
            if f.comp == ci || p[0] < f.bbox[0] || p[0] > f.bbox[2] || p[1] < f.bbox[1] || p[1] > f.bbox[3] {
                continue;
            }
            if winding(&f.lp, p) != 0 && best.is_none_or(|(a, _)| f.area < a) {
                best = Some((f.area, fi));
            }
        }
        if let (Some((_, fi)), Some(outer)) = (best, &comp.outer)
            && let Some(h) = holes.get_mut(fi)
        {
            h.push(canonical(outer.clone()));
        }
    }

    let mut result: Vec<Profile> = bounded
        .into_iter()
        .zip(holes)
        .map(|(f, mut hs)| {
            hs.sort_by_key(key);
            let area = f.area + hs.iter().map(Loop::signed_area).sum::<f64>();
            let outer = canonical(f.lp);
            let mut pieces: Vec<(EntityId, u32)> = outer.segments.iter().map(|s| (s.entity, s.piece)).collect();
            pieces.sort_unstable();
            pieces.dedup();
            let label = interior_point(&outer, &hs);
            Profile { outer, holes: hs, area, pieces, label }
        })
        .collect();
    result.sort_by_key(|p| key(&p.outer));
    Ok(Arrangement { profiles: result, open_ends, crossings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_overlap_measures_shared_sweep() {
        assert!((arrange::arc_overlap(0.0, 1.0, 0.5, 1.0) - 0.5).abs() < 1e-12);
        assert!(arrange::arc_overlap(0.0, 1.0, 1.0, 1.0).abs() < 1e-12);
        assert!((arrange::arc_overlap(6.0, 1.0, 0.0, 0.5) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn arc_winding_is_exact() {
        // Upper half circle CCW from (1,0) to (-1,0), closed by the diameter.
        let arc = Segment {
            entity: EntityId(0),
            piece: 0,
            sub: 0,
            reversed: false,
            curve: Curve::Arc { center: [0.0, 0.0], radius: 1.0, start: [1.0, 0.0], end: [-1.0, 0.0], ccw: true },
        };
        let line = Segment {
            entity: EntityId(1),
            piece: 0,
            sub: 0,
            reversed: false,
            curve: Curve::Line { start: [-1.0, 0.0], end: [1.0, 0.0] },
        };
        let l = Loop { segments: vec![arc, line] };
        assert_eq!(winding(&l, [0.0, 0.5]), 1);
        assert_eq!(winding(&l, [0.0, 0.99]), 1);
        assert_eq!(winding(&l, [0.0, -0.5]), 0);
        assert_eq!(winding(&l, [0.0, 1.5]), 0);
        assert!((l.signed_area() - PI / 2.0).abs() < 1e-12);
    }
}
