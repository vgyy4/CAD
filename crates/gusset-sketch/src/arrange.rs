//! Planar arrangement of the sketch's curves: intersections, vertices and edge pieces.
//!
//! Every curve is split at the points where it meets another curve (a crossing, a touch
//! or an endpoint lying on its interior), so what comes out is a planar graph whose
//! edges are *pieces* of the original entities. [`crate::profile`] traces the faces.

use std::collections::BTreeMap;

use crate::geom2::{V2, add, angle, cross, dist, dot, norm, not_gt, scale, sub, wrap_2pi};
use crate::profile::{Cv, ProfileError, Shape};
use crate::sketch::EntityId;

const TAU: f64 = core::f64::consts::TAU;

/// Most vertices an arrangement may have (bounds memory on hostile input).
pub(crate) const MAX_VERTICES: usize = 200_000;

/// Merged points: positions closer than `tol` share one vertex (first come, first served).
pub(crate) struct Verts {
    pub(crate) pts: Vec<V2>,
    grid: BTreeMap<(i64, i64), Vec<usize>>,
    tol: f64,
}

impl Verts {
    pub(crate) fn new(tol: f64) -> Self {
        Verts { pts: Vec::new(), grid: BTreeMap::new(), tol }
    }

    fn cell(&self, x: f64) -> i64 {
        libm::floor(x / self.tol) as i64
    }

    /// The vertex within `tol` of `p` (the lowest index when several), or a new one.
    pub(crate) fn intern(&mut self, p: V2) -> Result<usize, ProfileError> {
        let (gx, gy) = (self.cell(p[0]), self.cell(p[1]));
        let mut found: Option<usize> = None;
        for dx in -1..=1i64 {
            for dy in -1..=1i64 {
                if let Some(list) = self.grid.get(&(gx.saturating_add(dx), gy.saturating_add(dy))) {
                    for &vi in list {
                        if self.pts.get(vi).is_some_and(|&q| dist(p, q) <= self.tol) && found.is_none_or(|f| vi < f) {
                            found = Some(vi);
                        }
                    }
                }
            }
        }
        if let Some(v) = found {
            return Ok(v);
        }
        if self.pts.len() >= MAX_VERTICES {
            return Err(ProfileError::TooComplex { limit: MAX_VERTICES });
        }
        self.pts.push(p);
        self.grid.entry((gx, gy)).or_default().push(self.pts.len() - 1);
        Ok(self.pts.len() - 1)
    }

    pub(crate) fn at(&self, v: usize) -> V2 {
        self.pts.get(v).copied().unwrap_or([0.0, 0.0])
    }
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
pub(crate) fn arc_overlap(a0: f64, sa: f64, b0: f64, sb: f64) -> f64 {
    let rb = wrap_2pi(b0 - a0);
    let ov = |lo: f64, hi: f64| (hi.min(sa) - lo.max(0.0)).max(0.0);
    ov(rb, rb + sb) + ov(rb - TAU, rb + sb - TAU)
}

fn curve_on(cv: &Cv, p: V2, tol: f64) -> bool {
    match cv.shape {
        Shape::Arc { c, r, a0, span } => on_arc(p, c, a0, span, tol / r),
        Shape::Line | Shape::Circle { .. } | Shape::Spline { .. } => true,
    }
}

/// Where `p` lies along the curve: a fraction in `[0, 1]` for lines, a counter-clockwise
/// angle from the start (arcs) or from the +x axis (circles) otherwise.
fn param_on(cv: &Cv, p: V2) -> f64 {
    match &cv.shape {
        Shape::Line => {
            let d = sub(cv.p[1], cv.p[0]);
            dot(sub(p, cv.p[0]), d) / dot(d, d)
        }
        Shape::Arc { c, a0, .. } => wrap_2pi(angle(sub(p, *c)) - a0),
        Shape::Circle { c, .. } => wrap_2pi(angle(sub(p, *c))),
        Shape::Spline { .. } => crate::curves::nearest_param(cv, p),
    }
}

/// Direction of travel (increasing parameter) at `p`.
fn direction_at(cv: &Cv, p: V2) -> V2 {
    match &cv.shape {
        Shape::Line => sub(cv.p[1], cv.p[0]),
        Shape::Arc { c, .. } | Shape::Circle { c, .. } => {
            let q = sub(p, *c);
            [-q[1], q[0]]
        }
        Shape::Spline { .. } => crate::curves::eval(cv, crate::curves::nearest_param(cv, p)).1,
    }
}

/// `+1`/`-1` when `a` crosses `b` from right to left/left to right at `p`, `0` when tangent.
fn crossing_sign(a: &Cv, b: &Cv, p: V2) -> i8 {
    let (ta, tb) = (direction_at(a, p), direction_at(b, p));
    let x = cross(ta, tb) / (norm(ta) * norm(tb)).max(f64::MIN_POSITIVE);
    if x > 1e-9 {
        1
    } else if x < -1e-9 {
        -1
    } else {
        0
    }
}

/// Points where two curves meet. `Err(at)` when they overlap along a stretch.
/// What stops [`raw_points`]: two curves overlap along a stretch (at this point), or the
/// intersection work limit was reached.
enum Stop {
    Overlap(V2),
    TooMuchWork,
}

fn raw_points(a: &Cv, b: &Cv, tol: f64, work: &mut crate::curves::Work) -> Result<Vec<V2>, Stop> {
    if matches!(a.shape, Shape::Spline { .. }) || matches!(b.shape, Shape::Spline { .. }) {
        return crate::curves::numeric_hits(a, b, tol, work).map_err(|_| Stop::TooMuchWork);
    }
    raw_points_analytic(a, b, tol).map_err(Stop::Overlap)
}

fn raw_points_analytic(a: &Cv, b: &Cv, tol: f64) -> Result<Vec<V2>, V2> {
    let circ = |s: &Shape| match *s {
        Shape::Arc { c, r, .. } | Shape::Circle { c, r } => Some((c, r)),
        Shape::Line | Shape::Spline { .. } => None,
    };
    let (ca, cb) = (circ(&a.shape), circ(&b.shape));
    let mut pts = match (ca, cb) {
        (None, None) => {
            let h = seg_seg(a.p[0], a.p[1], b.p[0], b.p[1], tol);
            if let Some(o) = h.overlap {
                return Err(o);
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
                return match (&a.shape, &b.shape) {
                    (&Shape::Arc { a0, span, .. }, &Shape::Arc { a0: b0, span: sb, .. }) => {
                        if arc_overlap(a0, span, b0, sb) > tol / r1 {
                            let mid = a0 + 0.5 * span;
                            Err(add(c1, [r1 * libm::cos(mid), r1 * libm::sin(mid)]))
                        } else {
                            // Arcs of one circle that only touch end to end share a vertex.
                            Ok(Vec::new())
                        }
                    }
                    _ => Err(at),
                };
            }
        },
    };
    pts.retain(|&p| curve_on(a, p, tol) && curve_on(b, p, tol));
    Ok(pts)
}

/// One place where a curve is cut.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Split {
    pub(crate) param: f64,
    pub(crate) vertex: usize,
    /// The other curve involved, and how this curve crosses it (see [`crossing_sign`]).
    pub(crate) partner: EntityId,
    pub(crate) sign: i8,
}

/// Everything found by [`find_splits`].
pub(crate) struct Splits {
    /// Per curve, unordered.
    pub(crate) per_curve: Vec<Vec<Split>>,
    /// Some curve is cut in its interior (a real crossing or T-junction).
    pub(crate) crossings: bool,
}

/// Finds all the places where curves meet, interning the meeting points as vertices.
/// Endpoint vertices must already be in `verts` and in `cvs[..].v`.
pub(crate) fn find_splits(cvs: &[Cv], verts: &mut Verts, tol: f64) -> Result<Splits, ProfileError> {
    let mut per_curve: Vec<Vec<Split>> = vec![Vec::new(); cvs.len()];
    let mut crossings = false;
    let mut order: Vec<usize> = (0..cvs.len()).collect();
    let minx = |i: usize| cvs.get(i).map_or(0.0, |c| c.bbox[0]);
    order.sort_by(|&i, &j| minx(i).total_cmp(&minx(j)).then(i.cmp(&j)));
    let mut count = 0usize;
    let mut work = crate::curves::Work::default();
    let too_much = || ProfileError::TooMuchWork { limit: crate::curves::MAX_INTERSECT_WORK };
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
            let pts = raw_points(a, b, tol, &mut work).map_err(|stop| match stop {
                Stop::Overlap(at) => {
                    let (x, y) = if a.id < b.id { (a.id, b.id) } else { (b.id, a.id) };
                    ProfileError::SelfIntersecting { a: x, b: y, at }
                }
                Stop::TooMuchWork => too_much(),
            })?;
            if pts.is_empty() {
                continue;
            }
            // Hits near a joint the two curves share are the joint itself. Near tangency the
            // computed points scatter by about sqrt(2·r·tol), so allow that much.
            let rmax = [&a.shape, &b.shape]
                .iter()
                .map(|s| match **s {
                    Shape::Arc { r, .. } | Shape::Circle { r, .. } => r,
                    Shape::Line | Shape::Spline { .. } => 0.0,
                })
                .fold(0.0, f64::max);
            let near = if rmax > 0.0 { tol + (2.0 * tol * rmax).sqrt() } else { tol };
            let shared: Vec<usize> = a.v.iter().copied().filter(|v| *v != usize::MAX && b.v.contains(v)).collect();
            for p in pts {
                count += 1;
                if count > 4 * MAX_VERTICES {
                    return Err(ProfileError::TooComplex { limit: MAX_VERTICES });
                }
                let snapped = shared.iter().copied().find(|&v| dist(p, verts.at(v)) <= near);
                let vertex = match snapped {
                    Some(v) => v,
                    None => verts.intern(p)?,
                };
                let at = verts.at(vertex);
                let sign = crossing_sign(a, b, at);
                for (idx, cv, other, s) in [(i, a, b, sign), (j, b, a, -sign)] {
                    if cv.v.contains(&vertex) {
                        continue;
                    }
                    crossings = true;
                    if let Some(list) = per_curve.get_mut(idx) {
                        list.push(Split { param: param_on(cv, at), vertex, partner: other.id, sign: s });
                    }
                }
            }
        }
    }

    Ok(Splits { per_curve, crossings })
}

/// Cuts a curve at its splits. Pieces are appended to `out`; a circle or closed spline
/// without a cut is returned unchanged (`Some`).
pub(crate) fn make_pieces(
    cv: &Cv,
    mut splits: Vec<Split>,
    verts: &mut Verts,
    out: &mut Vec<Cv>,
) -> Result<Option<Cv>, ProfileError> {
    splits.sort_by(|x, y| x.param.total_cmp(&y.param).then(x.vertex.cmp(&y.vertex)));
    let (d0, d1) = crate::curves::domain(cv);
    if !cv.shape.is_ring() {
        let mut chain: Vec<(usize, f64)> = vec![(cv.v[0], d0)];
        for s in &splits {
            if chain.last().map(|l| l.0) != Some(s.vertex) {
                chain.push((s.vertex, s.param));
            }
        }
        if chain.last().map(|l| l.0) != Some(cv.v[1]) {
            chain.push((cv.v[1], d1));
        }
        for (ord, w) in chain.windows(2).enumerate() {
            if let &[(v0, u0), (v1, u1)] = w {
                out.push(piece_of(cv, [v0, v1], [u0, u1], verts, u32::try_from(ord).unwrap_or(u32::MAX)));
            }
        }
        return Ok(None);
    }
    // A circle or closed spline: pieces run between consecutive cuts round the ring.
    let len = d1 - d0;
    // One entry per vertex: the best (lowest partner, then entering) split.
    let mut best: BTreeMap<usize, Split> = BTreeMap::new();
    for s in &splits {
        match best.get(&s.vertex) {
            Some(old) if cmp_split(s, old).is_ge() => {}
            _ => {
                best.insert(s.vertex, *s);
            }
        }
    }
    let mut ring: Vec<Split> = best.into_values().collect();
    if ring.is_empty() {
        return Ok(Some(cv.clone()));
    }
    if let [only] = ring.as_slice() {
        // A single touch point: add the opposite point so every piece is a proper curve.
        let u = d0 + (only.param - d0 + 0.5 * len) % len;
        let anti = match cv.shape {
            Shape::Circle { c, .. } => verts.intern(sub(c, sub(verts.at(only.vertex), c)))?,
            _ => verts.intern(crate::curves::eval(cv, u).0)?,
        };
        ring.push(Split { param: u, vertex: anti, partner: only.partner, sign: 0 });
    }
    ring.sort_by(|x, y| x.param.total_cmp(&y.param).then(x.vertex.cmp(&y.vertex)));
    // The piece numbered 0 starts at the most "canonical" cut, so numbers survive moving
    // or rotating the sketch.
    let origin = ring.iter().enumerate().min_by(|(_, x), (_, y)| cmp_split(x, y)).map_or(0, |(i, _)| i);
    let n = ring.len();
    for j in 0..n {
        let (Some(s0), Some(s1)) = (ring.get((origin + j) % n), ring.get((origin + j + 1) % n)) else {
            continue;
        };
        let u1 = if s1.param > s0.param { s1.param } else { s1.param + len };
        out.push(piece_of(cv, [s0.vertex, s1.vertex], [s0.param, u1], verts, u32::try_from(j).unwrap_or(u32::MAX)));
    }
    Ok(None)
}

/// Canonical order of cuts of one circle: lowest partner first, entering before leaving,
/// then by angle.
fn cmp_split(x: &Split, y: &Split) -> core::cmp::Ordering {
    let key = |s: &Split| (s.partner, s.sign <= 0);
    key(x).cmp(&key(y)).then(x.param.total_cmp(&y.param))
}

/// The piece of `cv` running from vertex `v[0]` (parameter `u[0]`) to vertex `v[1]`
/// (parameter `u[1]`), counter-clockwise for arcs and circles.
fn piece_of(cv: &Cv, v: [usize; 2], u: [f64; 2], verts: &Verts, piece: u32) -> Cv {
    let p = [verts.at(v[0]), verts.at(v[1])];
    let shape = match &cv.shape {
        Shape::Line => Shape::Line,
        Shape::Arc { c, r, .. } | Shape::Circle { c, r } => {
            let a0 = angle(sub(p[0], *c));
            let span = wrap_2pi(angle(sub(p[1], *c)) - a0);
            Shape::Arc { c: *c, r: *r, a0, span }
        }
        Shape::Spline { chain, total, closed, first } => {
            // Chain parameters are relative to this shape's first segment.
            let mut sub_chain = crate::spline::sub_chain(chain, u[0], u[1], *closed);
            let seg0 = (*first as usize + libm::floor(u[0]).max(0.0) as usize) % (*total as usize).max(1);
            // Make the piece end exactly on the vertices it joins.
            if let Some(first) = sub_chain.first_mut() {
                first[0] = p[0];
            }
            if let Some(last) = sub_chain.last_mut() {
                last[3] = p[1];
            }
            Shape::Spline { chain: sub_chain, first: u32::try_from(seg0).unwrap_or(0), total: *total, closed: false }
        }
    };
    let bbox = crate::profile::bbox_of(&shape, p);
    Cv { id: cv.id, shape, p, v, bbox, piece }
}

/// Rejects zero-length pieces the tolerance cannot represent.
pub(crate) fn check_pieces(pieces: &[Cv]) -> Result<(), ProfileError> {
    for p in pieces {
        let empty = matches!(&p.shape, Shape::Spline { chain, .. } if chain.is_empty());
        if empty || (p.v[0] == p.v[1] && !matches!(p.shape, Shape::Spline { .. })) || not_gt(dist(p.p[0], p.p[1]), 0.0)
        {
            return Err(ProfileError::Degenerate { entity: p.id });
        }
    }
    Ok(())
}

/// One place where an entity is cut by another curve (see [`cuts`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    /// Position along the entity: the fraction `0..1` from `a` to `b` for a line, the
    /// counter-clockwise angle from `start` (arc) or from the +x axis (circle), in radians.
    pub param: f64,
    pub pos: [f64; 2],
    /// The other curve that meets the entity here.
    pub partner: EntityId,
    /// An existing point entity at the cut (e.g. the endpoint of a curve that touches the
    /// entity), which can serve as the new end of a trimmed piece.
    pub at_point: Option<EntityId>,
}

/// The cuts of one entity and where a pick lies among them.
#[derive(Debug, Clone, PartialEq)]
pub struct Cuts {
    /// Ordered by `param`, one per place.
    pub cuts: Vec<Cut>,
    /// `param` of the entity point nearest to the pick (clamped to an arc's extent).
    pub pick: f64,
    /// Sweep of an arc in radians (0 for other entities).
    pub span: f64,
}

/// Where `entity` is cut by the other non-construction curves of the sketch, and where
/// the point `pick` (near the entity) falls along it. This is what `sketch.trim` works on.
pub fn cuts(sketch: &crate::sketch::Sketch, entity: EntityId, pick: [f64; 2], tol: f64) -> Result<Cuts, ProfileError> {
    use crate::sketch::{Geom, SketchError};
    if !(crate::profile::MIN_PROFILE_TOL..=crate::profile::MAX_PROFILE_TOL).contains(&tol) {
        return Err(ProfileError::BadTolerance);
    }
    sketch.validate().map_err(ProfileError::Invalid)?;
    let mut cvs = crate::profile::collect(sketch, tol)?;
    if matches!(sketch.entity(entity).map(|e| e.geom), Some(Geom::Spline { .. })) {
        return Err(ProfileError::Invalid(SketchError::WrongKind {
            entity,
            expected: "a line, arc or circle (splines cannot be trimmed)",
        }));
    }
    let Some(idx) = cvs.iter().position(|c| c.id == entity) else {
        return Err(ProfileError::Invalid(SketchError::WrongKind {
            entity,
            expected: "a line, arc or circle that is not construction geometry",
        }));
    };
    let mut verts = Verts::new(tol);
    for cv in &mut cvs {
        if matches!(cv.shape, Shape::Circle { .. }) {
            continue;
        }
        for k in 0..2 {
            if let (Some(&p), Some(slot)) = (cv.p.get(k), cv.v.get_mut(k)) {
                *slot = verts.intern(p)?;
            }
        }
    }
    let splits = find_splits(&cvs, &mut verts, tol)?;
    let Some(target) = cvs.get(idx) else { return Ok(Cuts { cuts: Vec::new(), pick: 0.0, span: 0.0 }) };
    let mut list = splits.per_curve.get(idx).cloned().unwrap_or_default();
    list.sort_by(|x, y| x.param.total_cmp(&y.param).then(x.vertex.cmp(&y.vertex)));
    let own: Vec<EntityId> = match sketch.entity(entity).map(|e| e.geom) {
        Some(Geom::Line { a, b }) => vec![a, b],
        Some(Geom::Circle { center, .. }) => vec![center],
        Some(Geom::Arc { center, start, end }) => vec![center, start, end],
        _ => Vec::new(),
    };
    let mut out: Vec<Cut> = Vec::new();
    let mut last_vertex = usize::MAX;
    for s in list {
        if s.vertex == last_vertex {
            continue;
        }
        last_vertex = s.vertex;
        let pos = verts.at(s.vertex);
        let at_point = sketch
            .entities()
            .iter()
            .filter(|e| !own.contains(&e.id))
            .find(|e| matches!(e.geom, Geom::Point { p } if dist(p, pos) <= tol))
            .map(|e| e.id);
        out.push(Cut { param: s.param, pos, partner: s.partner, at_point });
    }
    let span = match target.shape {
        Shape::Arc { span, .. } => span,
        _ => 0.0,
    };
    let mut pick_param = param_on(target, pick);
    match target.shape {
        Shape::Line => pick_param = pick_param.clamp(0.0, 1.0),
        Shape::Arc { span, .. } => {
            if pick_param > span {
                // Outside the arc: nearer to the end or to the start?
                pick_param = if pick_param - span < TAU - pick_param { span } else { 0.0 };
            }
        }
        Shape::Circle { .. } | Shape::Spline { .. } => {}
    }
    Ok(Cuts { cuts: out, pick: pick_param, span })
}
