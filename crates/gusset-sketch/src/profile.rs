//! Closed-region ("profile") extraction from a solved sketch.
//!
//! Non-construction lines, arcs and circles are merged at shared endpoints (within
//! `tol`), checked for crossings, and turned into a planar graph. Faces of that
//! graph are traced with the usual "turn most clockwise" rule; every bounded face
//! becomes a profile whose holes are the outer boundaries of the components nested
//! directly inside it. Results are ordered deterministically.

use std::collections::BTreeMap;
use std::fmt;

use crate::geom2::{V2, add, angle, cross, dist, dot, norm, not_gt, scale, sub, wrap_2pi};
use crate::sketch::{EntityId, Geom, Sketch, SketchError};

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
}

/// One segment of a loop, referring to the sketch entity it comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub entity: EntityId,
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
    /// Geometry with free ends (the entities touching a dangling endpoint).
    Open {
        entities: Vec<EntityId>,
    },
    /// Two curves cross, touch or overlap away from a shared endpoint.
    SelfIntersecting {
        a: EntityId,
        b: EntityId,
        at: [f64; 2],
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
                write!(f, "{a} and {b} intersect at ({}, {})", at[0], at[1])
            }
        }
    }
}

impl std::error::Error for ProfileError {}

#[derive(Debug, Clone, Copy)]
enum Shape {
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
}

#[derive(Debug, Clone, Copy)]
struct Cv {
    id: EntityId,
    shape: Shape,
    /// Original endpoint positions (unused for circles).
    p: [V2; 2],
    /// Merged vertex index of each endpoint (`usize::MAX` for circles).
    v: [usize; 2],
    bbox: [f64; 4],
}

fn bbox_of(shape: &Shape, p: [V2; 2]) -> [f64; 4] {
    match *shape {
        Shape::Line => [p[0][0].min(p[1][0]), p[0][1].min(p[1][1]), p[0][0].max(p[1][0]), p[0][1].max(p[1][1])],
        Shape::Arc { c, r, .. } | Shape::Circle { c, r } => [c[0] - r, c[1] - r, c[0] + r, c[1] + r],
    }
}

fn pt_of(s: &Sketch, id: EntityId) -> Result<V2, ProfileError> {
    s.point(id).ok_or(ProfileError::Invalid(SketchError::UnknownEntity(id)))
}

fn collect(s: &Sketch, tol: f64) -> Result<Vec<Cv>, ProfileError> {
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
        };
        if out.len() >= MAX_PROFILE_CURVES {
            let count =
                s.entities().iter().filter(|e| !e.construction && !matches!(e.geom, Geom::Point { .. })).count();
            return Err(ProfileError::TooManyCurves { count, limit: MAX_PROFILE_CURVES });
        }
        out.push(Cv { id: e.id, shape, p, v: [usize::MAX; 2], bbox: bbox_of(&shape, p) });
    }
    Ok(out)
}

/// Merges endpoints closer than `tol` into shared vertices (first come, first
/// served, in entity order). Returns vertex positions.
fn merge_vertices(cvs: &mut [Cv], tol: f64) -> Vec<V2> {
    let mut verts: Vec<V2> = Vec::new();
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    let cell = |x: f64| libm::floor(x / tol) as i64;
    for cv in cvs.iter_mut() {
        if matches!(cv.shape, Shape::Circle { .. }) {
            continue;
        }
        for k in 0..2 {
            let Some(&p) = cv.p.get(k) else { continue };
            let (gx, gy) = (cell(p[0]), cell(p[1]));
            let mut found = None;
            'search: for dx in -1..=1i64 {
                for dy in -1..=1i64 {
                    if let Some(list) = grid.get(&(gx.saturating_add(dx), gy.saturating_add(dy))) {
                        for &vi in list {
                            if verts.get(vi).is_some_and(|&q| dist(p, q) <= tol) && found.is_none_or(|f| vi < f) {
                                found = Some(vi);
                            }
                        }
                    }
                    if found.is_some() && dx == 1 && dy == 1 {
                        break 'search;
                    }
                }
            }
            let vi = found.unwrap_or_else(|| {
                verts.push(p);
                grid.entry((gx, gy)).or_default().push(verts.len() - 1);
                verts.len() - 1
            });
            if let Some(slot) = cv.v.get_mut(k) {
                *slot = vi;
            }
        }
    }
    verts
}

fn on_arc(p: V2, c: V2, a0: f64, span: f64, slack: f64) -> bool {
    let rel = wrap_2pi(angle(sub(p, c)) - a0);
    rel <= span + slack || rel >= TAU - slack
}

/// Candidate intersection points of two curves, or an overlap point.
struct Hits {
    points: Vec<V2>,
    overlap: Option<V2>,
}

fn seg_seg(a0: V2, a1: V2, b0: V2, b1: V2, tol: f64) -> Hits {
    let mut h = Hits { points: Vec::new(), overlap: None };
    let (d1, d2) = (sub(a1, a0), sub(b1, b0));
    let (l1, l2) = (norm(d1), norm(d2));
    let den = cross(d1, d2);
    let w = sub(b0, a0);
    if den.abs() <= 1e-12 * l1 * l2 {
        if (cross(d1, w) / l1).abs() > tol {
            return h;
        }
        let t0 = dot(w, d1) / l1;
        let t1 = dot(sub(b1, a0), d1) / l1;
        let lo = t0.min(t1).max(0.0);
        let hi = t0.max(t1).min(l1);
        let at = add(a0, scale(d1, (lo + hi) * 0.5 / l1));
        if hi - lo > tol {
            h.overlap = Some(at);
        } else if hi - lo >= -tol {
            h.points.push(at);
        }
        return h;
    }
    let t = cross(w, d2) / den;
    let u = cross(w, d1) / den;
    let (et, eu) = (tol / l1, tol / l2);
    if (-et..=1.0 + et).contains(&t) && (-eu..=1.0 + eu).contains(&u) {
        h.points.push(add(a0, scale(d1, t)));
    }
    h
}

fn seg_circle(a0: V2, a1: V2, c: V2, r: f64, tol: f64) -> Vec<V2> {
    let d = sub(a1, a0);
    let l = norm(d);
    let u = scale(d, 1.0 / l);
    let tf = dot(sub(c, a0), u);
    let hgt = cross(u, sub(c, a0)).abs();
    if hgt > r + tol {
        return Vec::new();
    }
    let ts = if hgt >= r - tol {
        vec![tf]
    } else {
        let half = (r * r - hgt * hgt).sqrt();
        vec![tf - half, tf + half]
    };
    ts.into_iter().filter(|t| (-tol..=l + tol).contains(t)).map(|t| add(a0, scale(u, t))).collect()
}

/// Intersections of two circles; `None` when they coincide.
fn circle_circle(c1: V2, r1: f64, c2: V2, r2: f64, tol: f64) -> Option<Vec<V2>> {
    let d = dist(c1, c2);
    if d <= tol {
        return if (r1 - r2).abs() <= tol { None } else { Some(Vec::new()) };
    }
    if d > r1 + r2 + tol || d < (r1 - r2).abs() - tol {
        return Some(Vec::new());
    }
    let a = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let u = scale(sub(c2, c1), 1.0 / d);
    let m = add(c1, scale(u, a));
    let h2 = r1 * r1 - a * a;
    if h2 <= tol * tol {
        return Some(vec![m]);
    }
    let h = h2.sqrt();
    let perp = [-u[1], u[0]];
    Some(vec![add(m, scale(perp, h)), sub(m, scale(perp, h))])
}

/// Angular length of the overlap of two CCW arcs `[a0, a0+sa]` and `[b0, b0+sb]`.
fn arc_overlap(a0: f64, sa: f64, b0: f64, sb: f64) -> f64 {
    let rb = wrap_2pi(b0 - a0);
    let ov = |lo: f64, hi: f64| (hi.min(sa) - lo.max(0.0)).max(0.0);
    ov(rb, rb + sb) + ov(rb - TAU, rb + sb - TAU)
}

fn curve_on(cv: &Cv, p: V2, tol: f64) -> bool {
    match cv.shape {
        Shape::Arc { c, r, a0, span } => on_arc(p, c, a0, span, tol / r),
        Shape::Line | Shape::Circle { .. } => true,
    }
}

/// Finds a crossing/touching/overlap of two curves that is not at a shared vertex.
fn intersection(a: &Cv, b: &Cv, verts: &[V2], tol: f64) -> Option<V2> {
    let circ = |s: &Shape| match *s {
        Shape::Arc { c, r, .. } | Shape::Circle { c, r } => Some((c, r)),
        Shape::Line => None,
    };
    let (ca, cb) = (circ(&a.shape), circ(&b.shape));
    let rmax = ca.map_or(0.0, |x| x.1).max(cb.map_or(0.0, |x| x.1));
    let mut pts = match (ca, cb) {
        (None, None) => {
            let h = seg_seg(a.p[0], a.p[1], b.p[0], b.p[1], tol);
            if let Some(o) = h.overlap {
                return Some(o);
            }
            h.points
        }
        (None, Some((c, r))) => seg_circle(a.p[0], a.p[1], c, r, tol),
        (Some((c, r)), None) => seg_circle(b.p[0], b.p[1], c, r, tol),
        (Some((c1, r1)), Some((c2, r2))) => match circle_circle(c1, r1, c2, r2, tol) {
            Some(p) => p,
            None => {
                // Same circle: overlapping unless two arcs only share endpoints.
                let at = add(c1, [r1, 0.0]);
                return match (a.shape, b.shape) {
                    (Shape::Arc { a0, span, .. }, Shape::Arc { a0: b0, span: sb, .. }) => {
                        (arc_overlap(a0, span, b0, sb) > tol / r1).then(|| {
                            let mid = a0 + 0.5 * span;
                            add(c1, [r1 * libm::cos(mid), r1 * libm::sin(mid)])
                        })
                    }
                    _ => Some(at),
                };
            }
        },
    };
    pts.retain(|&p| curve_on(a, p, tol) && curve_on(b, p, tol));
    // Intersections near a shared endpoint are the joint itself. Near tangency the
    // computed points scatter by about sqrt(2·r·tol), so allow that much.
    let near = if rmax > 0.0 { tol + (2.0 * tol * rmax).sqrt() } else { tol };
    let shared: Vec<V2> =
        a.v.iter().filter(|v| **v != usize::MAX && b.v.contains(v)).filter_map(|&v| verts.get(v).copied()).collect();
    pts.into_iter().find(|&p| !shared.iter().any(|&v| dist(p, v) <= near))
}

fn check_intersections(cvs: &[Cv], verts: &[V2], tol: f64) -> Result<(), ProfileError> {
    let mut order: Vec<usize> = (0..cvs.len()).collect();
    let minx = |i: usize| cvs.get(i).map_or(0.0, |c| c.bbox[0]);
    order.sort_by(|&i, &j| minx(i).total_cmp(&minx(j)).then(i.cmp(&j)));
    for (k, &i) in order.iter().enumerate() {
        let Some(a) = cvs.get(i) else { continue };
        for &j in order.iter().skip(k + 1) {
            let Some(b) = cvs.get(j) else { continue };
            if b.bbox[0] > a.bbox[2] + tol {
                break;
            }
            if b.bbox[1] > a.bbox[3] + tol || b.bbox[3] < a.bbox[1] - tol {
                continue;
            }
            if let Some(at) = intersection(a, b, verts, tol) {
                let (x, y) = if a.id < b.id { (a.id, b.id) } else { (b.id, a.id) };
                return Err(ProfileError::SelfIntersecting { a: x, b: y, at });
            }
        }
    }
    Ok(())
}

/// A curve used in a loop, in a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Use {
    cv: usize,
    rev: bool,
}

fn make_segment(cvs: &[Cv], verts: &[V2], u: Use) -> Option<Segment> {
    let cv = cvs.get(u.cv)?;
    let vp = |k: usize| cv.v.get(k).and_then(|&v| verts.get(v)).copied();
    let curve = match cv.shape {
        Shape::Line => {
            let (s, e) = (vp(0)?, vp(1)?);
            if u.rev { Curve::Line { start: e, end: s } } else { Curve::Line { start: s, end: e } }
        }
        Shape::Arc { c, r, .. } => {
            let (s, e) = (vp(0)?, vp(1)?);
            if u.rev {
                Curve::Arc { center: c, radius: r, start: e, end: s, ccw: false }
            } else {
                Curve::Arc { center: c, radius: r, start: s, end: e, ccw: true }
            }
        }
        Shape::Circle { c, r } => Curve::Circle { center: c, radius: r, ccw: !u.rev },
    };
    Some(Segment { entity: cv.id, reversed: u.rev, curve })
}

fn seg_area(s: &Segment) -> f64 {
    match s.curve {
        Curve::Line { start, end } => 0.5 * cross(start, end),
        Curve::Arc { center, radius, start, end, ccw } => {
            // Area term of the underlying counter-clockwise arc (s0 → e0).
            let (s0, e0) = if ccw { (start, end) } else { (end, start) };
            let span = wrap_2pi(angle(sub(e0, center)) - angle(sub(s0, center)));
            let t = 0.5 * (radius * radius * span + center[0] * (e0[1] - s0[1]) - center[1] * (e0[0] - s0[0]));
            if ccw { t } else { -t }
        }
        Curve::Circle { radius, ccw, .. } => {
            let a = PI * radius * radius;
            if ccw { a } else { -a }
        }
    }
}

fn line_angle(p: V2, a: V2, b: V2) -> f64 {
    let (u, v) = (sub(a, p), sub(b, p));
    libm::atan2(cross(u, v), dot(u, v))
}

/// Angle swept around `p` by one segment (exact for arcs and circles).
fn seg_winding(s: &Segment, p: V2) -> f64 {
    match s.curve {
        Curve::Line { start, end } => line_angle(p, start, end),
        Curve::Arc { center, radius, start, end, ccw } => {
            let (s0, e0) = if ccw { (start, end) } else { (end, start) };
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
            let w = if dist(p, center) < radius { TAU } else { 0.0 };
            if ccw { w } else { -w }
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
        match s.curve {
            Curve::Line { start, end } => {
                grow(start, 0.0);
                grow(end, 0.0);
            }
            Curve::Arc { center, radius, .. } | Curve::Circle { center, radius, .. } => grow(center, radius),
        }
    }
    b
}

/// Rotates a loop to start at its smallest `(entity, reversed)` segment.
fn canonical(mut l: Loop) -> Loop {
    let start = l.segments.iter().enumerate().min_by_key(|(_, s)| (s.entity, s.reversed)).map_or(0, |(i, _)| i);
    l.segments.rotate_left(start);
    l
}

fn key(l: &Loop) -> (EntityId, bool) {
    l.segments.first().map_or((EntityId(u32::MAX), true), |s| (s.entity, s.reversed))
}

/// Half-edge `h` of edge `k`: `2k` runs from vertex `v[0]` to `v[1]`, `2k+1` back.
struct Graph {
    /// Curve index of each edge.
    edge_cv: Vec<usize>,
    /// Outgoing half-edges of each vertex, sorted counter-clockwise.
    out: Vec<Vec<usize>>,
    /// Position of each half-edge in its origin's `out` list.
    pos: Vec<usize>,
}

impl Graph {
    fn origin(&self, cvs: &[Cv], h: usize) -> Option<usize> {
        cvs.get(*self.edge_cv.get(h / 2)?)?.v.get(h % 2).copied()
    }

    fn next(&self, cvs: &[Cv], h: usize) -> Option<usize> {
        let t = h ^ 1;
        let list = self.out.get(self.origin(cvs, t)?)?;
        let i = *self.pos.get(t)?;
        list.get((i + list.len() - 1) % list.len()).copied()
    }
}

/// Outgoing tangent direction and signed curvature of half-edge `h`.
fn tangent(cv: &Cv, verts: &[V2], rev: bool) -> (f64, f64) {
    let vp = |k: usize| cv.v.get(k).and_then(|&v| verts.get(v)).copied().unwrap_or([0.0, 0.0]);
    match cv.shape {
        Shape::Line => {
            let d = if rev { sub(vp(0), vp(1)) } else { sub(vp(1), vp(0)) };
            (angle(d), 0.0)
        }
        Shape::Arc { c, r, .. } => {
            if rev {
                let q = sub(vp(1), c);
                (angle([q[1], -q[0]]), -1.0 / r)
            } else {
                let q = sub(vp(0), c);
                (angle([-q[1], q[0]]), 1.0 / r)
            }
        }
        Shape::Circle { .. } => (0.0, 0.0),
    }
}

fn build_graph(cvs: &[Cv], verts: &[V2]) -> Result<Graph, ProfileError> {
    let edge_cv: Vec<usize> =
        cvs.iter().enumerate().filter(|(_, c)| !matches!(c.shape, Shape::Circle { .. })).map(|(i, _)| i).collect();
    let mut out: Vec<Vec<(i64, f64, usize)>> = vec![Vec::new(); verts.len()];
    for (k, &ci) in edge_cv.iter().enumerate() {
        let Some(cv) = cvs.get(ci) else { continue };
        if cv.v[0] == cv.v[1] {
            return Err(ProfileError::Degenerate { entity: cv.id });
        }
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
    let mut dangling: Vec<EntityId> = Vec::new();
    for list in &out {
        if list.len() == 1 {
            for &(_, _, h) in list {
                if let Some(cv) = edge_cv.get(h / 2).and_then(|&ci| cvs.get(ci)) {
                    dangling.push(cv.id);
                }
            }
        }
    }
    if !dangling.is_empty() {
        dangling.sort_unstable();
        dangling.dedup();
        return Err(ProfileError::Open { entities: dangling });
    }
    let mut pos = vec![0usize; 2 * edge_cv.len()];
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
    Ok(Graph { edge_cv, out: sorted, pos })
}

/// Closed regions of a (solved) sketch, ignoring construction geometry.
///
/// `tol` (mm) is the distance below which endpoints are considered joined; it
/// must be in [`MIN_PROFILE_TOL`]..=[`MAX_PROFILE_TOL`]. Profiles are ordered by
/// the `(entity, reversed)` key of their outer loop's first segment; every loop
/// starts at its smallest such key. Points are ignored. Circles and arcs must not
/// touch or cross other curves except at shared arc/line endpoints.
pub fn profiles(sketch: &Sketch, tol: f64) -> Result<Vec<Profile>, ProfileError> {
    if !(MIN_PROFILE_TOL..=MAX_PROFILE_TOL).contains(&tol) {
        return Err(ProfileError::BadTolerance);
    }
    sketch.validate().map_err(ProfileError::Invalid)?;
    let mut cvs = collect(sketch, tol)?;
    let verts = merge_vertices(&mut cvs, tol);
    // Open ends first: a small gap is reported as a gap, not as a touch.
    let g = build_graph(&cvs, &verts)?;
    check_intersections(&cvs, &verts, tol)?;

    // Trace faces.
    let nh = 2 * g.edge_cv.len();
    let mut visited = vec![false; nh];
    let mut faces: Vec<Vec<usize>> = Vec::new();
    for h0 in 0..nh {
        if visited.get(h0).copied().unwrap_or(true) {
            continue;
        }
        let mut face = Vec::new();
        let mut h = h0;
        for _ in 0..=nh {
            if let Some(v) = visited.get_mut(h) {
                *v = true;
            }
            face.push(h);
            match g.next(&cvs, h) {
                Some(n) if n != h0 => h = n,
                _ => break,
            }
        }
        faces.push(face);
    }

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
    for &ci in &g.edge_cv {
        if let Some(cv) = cvs.get(ci) {
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
        let segs: Option<Vec<Segment>> = face
            .iter()
            .map(|&h| make_segment(&cvs, &verts, Use { cv: *g.edge_cv.get(h / 2)?, rev: h % 2 == 1 }))
            .collect();
        let Some(segs) = segs else { continue };
        let Some(v0) = face.first().and_then(|&h| g.origin(&cvs, h)) else { continue };
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
    // Circles are components of their own.
    for (i, cv) in cvs.iter().enumerate() {
        if let Shape::Circle { c, r } = cv.shape {
            let comp = comps.len();
            let mk = |rev| make_segment(&cvs, &verts, Use { cv: i, rev });
            let (Some(fwd), Some(bwd)) = (mk(false), mk(true)) else { continue };
            comps.push(Comp { sample: [c[0] + r, c[1]], outer: Some(Loop { segments: vec![bwd] }) });
            let lp = Loop { segments: vec![fwd] };
            bounded.push(Face { bbox: loop_bbox(&lp), area: lp.signed_area(), lp, comp });
        }
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
            Profile { outer: canonical(f.lp), holes: hs, area }
        })
        .collect();
    result.sort_by_key(|p| key(&p.outer));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_overlap_measures_shared_sweep() {
        assert!((arc_overlap(0.0, 1.0, 0.5, 1.0) - 0.5).abs() < 1e-12);
        assert!(arc_overlap(0.0, 1.0, 1.0, 1.0).abs() < 1e-12);
        assert!((arc_overlap(6.0, 1.0, 0.0, 0.5) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn arc_winding_is_exact() {
        // Upper half circle CCW from (1,0) to (-1,0), closed by the diameter.
        let arc = Segment {
            entity: EntityId(0),
            reversed: false,
            curve: Curve::Arc { center: [0.0, 0.0], radius: 1.0, start: [1.0, 0.0], end: [-1.0, 0.0], ccw: true },
        };
        let line = Segment {
            entity: EntityId(1),
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
