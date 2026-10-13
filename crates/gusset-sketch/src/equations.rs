//! Turns a [`Sketch`] into a vector of unknowns and scalar residual equations.
//!
//! Unknowns: `x, y` of every point entity and the radius of every circle, in entity
//! order. Arc radii are derived from their centre and start point; each arc adds
//! one internal equation `|start − center| − |end − center| = 0`.
//!
//! Residual units: lengths in mm, angles in radians, and `sin`/`cos` of angles for
//! parallel/perpendicular (dimensionless, ≈ radians near the solution).

use std::collections::BTreeSet;

use crate::dual::{Dual, N};
use crate::sketch::{Constraint, EntityId, Geom, Sketch};

/// Who an equation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    /// Arc consistency of the arc entity at this index into `Sketch::entities()`.
    Internal(usize),
    /// The constraint at this index into `Sketch::constraints()`.
    Constraint(usize),
}

/// A radius: a circle's own unknown, or `|start − center|` of an arc. Offsets are
/// into the equation's local variables (a point occupies two consecutive slots).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum R {
    Param(u8),
    Arc { c: u8, s: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    /// `x[a] − x[b]`
    Diff { a: u8, b: u8 },
    /// `x[a] − v`
    Value { a: u8, v: f64 },
    /// `x[p] − (x[a] + x[b]) / 2`
    Mid { p: u8, a: u8, b: u8 },
    /// `|p − q| − v`
    PtPt { p: u8, q: u8, v: f64 },
    /// `sign · sd(p; a→b) − v` with `sd` the signed distance to the line.
    PtLine { p: u8, a: u8, b: u8, sign: f64, v: f64 },
    /// `|p − c| − R`
    PtCircle { p: u8, c: u8, r: R },
    /// `R − v`
    Radius { r: R, v: f64 },
    /// `|b1 − a1| − |b2 − a2|`
    EqualLen { a1: u8, b1: u8, a2: u8, b2: u8 },
    /// `R1 − R2`
    EqualRad { r1: R, r2: R },
    /// `cross(d1, d2) / (|d1| |d2|)`
    Parallel { a1: u8, b1: u8, a2: u8, b2: u8 },
    /// `dot(d1, d2) / (|d1| |d2|)`
    Perp { a1: u8, b1: u8, a2: u8, b2: u8 },
    /// `wrap(atan2(cross, dot) − v)` into `(−π, π]`
    Angle { a1: u8, b1: u8, a2: u8, b2: u8, v: f64 },
    /// `sign · sd(c; a→b) − R`
    LineTangent { c: u8, r: R, a: u8, b: u8, sign: f64 },
    /// External (`mode == 0`): `|c1 − c2| − (R1 + R2)`;
    /// internal (`mode = ±1`): `|c1 − c2| − mode · (R1 − R2)`.
    CircTangent { c1: u8, r1: R, c2: u8, r2: R, mode: i8 },
    /// Tangency at a shared endpoint `p` of a line and an arc with centre `c`:
    /// `dot(b − a, p − c) / |b − a|` (the radius is perpendicular to the line).
    EndTangentLine { p: u8, c: u8, a: u8, b: u8 },
    /// Tangency of two arcs at a shared endpoint (`p1`, `p2` coincide):
    /// `cross(p1 − c1, p2 − c2) / (|p1 − c1| |p2 − c2|)` (centres and contact
    /// point collinear).
    EndTangentCurves { p1: u8, c1: u8, p2: u8, c2: u8 },
    /// `sd((p + q) / 2; a→b)`
    SymMid { p: u8, q: u8, a: u8, b: u8 },
    /// `dot(q − p, b − a) / |b − a|`
    SymPerp { p: u8, q: u8, a: u8, b: u8 },
    /// `|s − c| − |e − c|`
    ArcRadius { c: u8, s: u8, e: u8 },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Equation {
    pub(crate) owner: Owner,
    /// Global unknown index of each local variable.
    pub(crate) locals: [usize; N],
    pub(crate) nloc: usize,
    pub(crate) kind: Kind,
}

/// Mapping between sketch entities and the unknown vector.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    /// Initial values of all unknowns.
    pub(crate) x0: Vec<f64>,
    /// For each entity index: offset of its first unknown (points: x, circles: r).
    pub(crate) first: Vec<Option<usize>>,
}

impl Layout {
    pub(crate) fn new(s: &Sketch) -> Layout {
        let mut x0 = Vec::new();
        let mut first = Vec::with_capacity(s.entities().len());
        for e in s.entities() {
            match e.geom {
                Geom::Point { p } => {
                    first.push(Some(x0.len()));
                    x0.extend_from_slice(&p);
                }
                Geom::Circle { radius, .. } => {
                    first.push(Some(x0.len()));
                    x0.push(radius);
                }
                Geom::Line { .. } | Geom::Arc { .. } | Geom::Spline { .. } => first.push(None),
            }
        }
        Layout { x0, first }
    }

    fn param(&self, s: &Sketch, id: EntityId) -> Option<usize> {
        self.first.get(s.entity_index(id)?).copied().flatten()
    }
}

fn get(x: &[f64], i: usize) -> f64 {
    x.get(i).copied().unwrap_or(0.0)
}

/// Collects the local variables of one equation, de-duplicating points.
struct Builder<'a> {
    s: &'a Sketch,
    lay: &'a Layout,
    locals: [usize; N],
    n: usize,
    points: [(EntityId, u8); N],
    np: usize,
}

impl<'a> Builder<'a> {
    fn new(s: &'a Sketch, lay: &'a Layout) -> Self {
        Builder { s, lay, locals: [0; N], n: 0, points: [(EntityId(0), 0); N], np: 0 }
    }

    fn push(&mut self, global: usize) -> Option<u8> {
        let slot = self.locals.get_mut(self.n)?;
        *slot = global;
        let off = u8::try_from(self.n).ok()?;
        self.n += 1;
        Some(off)
    }

    fn pt(&mut self, id: EntityId) -> Option<u8> {
        if let Some(&(_, off)) = self.points.get(..self.np)?.iter().find(|(e, _)| *e == id) {
            return Some(off);
        }
        let g = self.lay.param(self.s, id)?;
        let off = self.push(g)?;
        self.push(g + 1)?;
        *self.points.get_mut(self.np)? = (id, off);
        self.np += 1;
        Some(off)
    }

    fn line(&mut self, id: EntityId) -> Option<(u8, u8)> {
        let (a, b) = self.s.line_points(id)?;
        Some((self.pt(a)?, self.pt(b)?))
    }

    fn center(&mut self, id: EntityId) -> Option<u8> {
        match self.s.entity(id)?.geom {
            Geom::Circle { center, .. } | Geom::Arc { center, .. } => self.pt(center),
            _ => None,
        }
    }

    fn rad(&mut self, id: EntityId) -> Option<R> {
        match self.s.entity(id)?.geom {
            Geom::Circle { .. } => {
                let g = self.lay.param(self.s, id)?;
                Some(R::Param(self.push(g)?))
            }
            Geom::Arc { center, start, .. } => Some(R::Arc { c: self.pt(center)?, s: self.pt(start)? }),
            _ => None,
        }
    }

    fn finish(&self, owner: Owner, kind: Kind) -> Equation {
        Equation { owner, locals: self.locals, nloc: self.n, kind }
    }
}

fn is_line(s: &Sketch, id: EntityId) -> bool {
    matches!(s.entity(id).map(|e| e.geom), Some(Geom::Line { .. }))
}

fn is_point(s: &Sketch, id: EntityId) -> bool {
    matches!(s.entity(id).map(|e| e.geom), Some(Geom::Point { .. }))
}

fn arc_ends(s: &Sketch, id: EntityId) -> Option<[EntityId; 2]> {
    match s.entity(id)?.geom {
        Geom::Arc { start, end, .. } => Some([start, end]),
        _ => None,
    }
}

/// Pairs of points joined by a direct `Coincident` constraint (both orders).
type Joined = BTreeSet<(EntityId, EntityId)>;

/// The same point entity, or two points joined by a direct `Coincident` constraint.
fn same_point(j: &Joined, a: EntityId, b: EntityId) -> bool {
    a == b || j.contains(&(a, b))
}

/// The first endpoint of arc `curve` that coincides with one of `pts`.
fn shared_end(s: &Sketch, j: &Joined, pts: &[EntityId], curve: EntityId) -> Option<EntityId> {
    let ends = arc_ends(s, curve)?;
    pts.iter().find_map(|&p| ends.into_iter().find(|&q| same_point(j, p, q)))
}

/// Builds every equation: arc-consistency equations first (entity order), then the
/// constraints in id order. Sign/branch choices are taken from `x0`. Returns `None`
/// only for a sketch that failed validation.
pub(crate) fn build(s: &Sketch, lay: &Layout) -> Option<Vec<Equation>> {
    let mut eqs = Vec::new();
    for (i, e) in s.entities().iter().enumerate() {
        if let Geom::Arc { center, start, end } = e.geom {
            let mut b = Builder::new(s, lay);
            let kind = Kind::ArcRadius { c: b.pt(center)?, s: b.pt(start)?, e: b.pt(end)? };
            eqs.push(b.finish(Owner::Internal(i), kind));
        }
    }
    let mut joined = Joined::new();
    for (_, c) in s.constraints() {
        if let Constraint::Coincident(a, b) = *c {
            joined.insert((a, b));
            joined.insert((b, a));
        }
    }
    for (ci, (_, c)) in s.constraints().iter().enumerate() {
        constraint_eqs(s, lay, &joined, Owner::Constraint(ci), c, &mut eqs)?;
    }
    Some(eqs)
}

fn constraint_eqs(
    s: &Sketch,
    lay: &Layout,
    joined: &Joined,
    owner: Owner,
    c: &Constraint,
    out: &mut Vec<Equation>,
) -> Option<()> {
    let x0 = &lay.x0;
    let mut one = |f: &dyn Fn(&mut Builder<'_>) -> Option<Kind>| -> Option<()> {
        let mut b = Builder::new(s, lay);
        let k = f(&mut b)?;
        out.push(b.finish(owner, k));
        Some(())
    };
    match *c {
        Constraint::Coincident(p, q) => {
            for ax in 0..2u8 {
                one(&|b| Some(Kind::Diff { a: b.pt(p)? + ax, b: b.pt(q)? + ax }))?;
            }
        }
        Constraint::PointOnLine(p, l) => {
            one(&|b| {
                let pp = b.pt(p)?;
                let (a, bb) = b.line(l)?;
                Some(Kind::PtLine { p: pp, a, b: bb, sign: 1.0, v: 0.0 })
            })?;
        }
        Constraint::PointOnCircle(p, c) => {
            one(&|b| Some(Kind::PtCircle { p: b.pt(p)?, c: b.center(c)?, r: b.rad(c)? }))?;
        }
        Constraint::Midpoint(p, l) => {
            for ax in 0..2u8 {
                one(&|b| {
                    let pp = b.pt(p)?;
                    let (a, bb) = b.line(l)?;
                    Some(Kind::Mid { p: pp + ax, a: a + ax, b: bb + ax })
                })?;
            }
        }
        Constraint::Horizontal(l) | Constraint::Vertical(l) => {
            let ax = u8::from(matches!(c, Constraint::Horizontal(_)));
            one(&|b| {
                let (a, bb) = b.line(l)?;
                Some(Kind::Diff { a: a + ax, b: bb + ax })
            })?;
        }
        Constraint::Parallel(l1, l2) | Constraint::Perpendicular(l1, l2) => {
            let par = matches!(c, Constraint::Parallel(..));
            one(&|b| {
                let (a1, b1) = b.line(l1)?;
                let (a2, b2) = b.line(l2)?;
                Some(if par { Kind::Parallel { a1, b1, a2, b2 } } else { Kind::Perp { a1, b1, a2, b2 } })
            })?;
        }
        Constraint::Tangent(e1, e2) => {
            let (line, curve) = if is_line(s, e1) {
                (Some(e1), e2)
            } else if is_line(s, e2) {
                (Some(e2), e1)
            } else {
                (None, e1)
            };
            // Curves joined at an endpoint: tangency there is first order (a direction
            // condition), whereas the distance form is degenerate (second order) once
            // the contact point is also pinned by the shared endpoint.
            if let Some(l) = line
                && let Some((la, lb)) = s.line_points(l)
                && let Some(p) = shared_end(s, joined, &[la, lb], curve)
            {
                one(&|b| {
                    let (a, bb) = b.line(l)?;
                    Some(Kind::EndTangentLine { p: b.pt(p)?, c: b.center(curve)?, a, b: bb })
                })?;
            } else if line.is_none()
                && let Some(ends) = arc_ends(s, e1)
                && let Some(p2) = shared_end(s, joined, &ends, e2)
                && let Some(p1) = ends.into_iter().find(|&q| same_point(joined, q, p2))
            {
                one(&|b| {
                    Some(Kind::EndTangentCurves { p1: b.pt(p1)?, c1: b.center(e1)?, p2: b.pt(p2)?, c2: b.center(e2)? })
                })?;
            } else if let Some(l) = line {
                let mut b = Builder::new(s, lay);
                let (a, bb) = b.line(l)?;
                let cc = b.center(curve)?;
                let r = b.rad(curve)?;
                let mut eq = b.finish(owner, Kind::LineTangent { c: cc, r, a, b: bb, sign: 1.0 });
                let sd =
                    eval(&Equation { kind: Kind::PtLine { p: cc, a, b: bb, sign: 1.0, v: 0.0 }, ..eq.clone() }, x0);
                if sd.v < 0.0 {
                    eq.kind = Kind::LineTangent { c: cc, r, a, b: bb, sign: -1.0 };
                }
                out.push(eq);
            } else {
                let mut b = Builder::new(s, lay);
                let c1 = b.center(e1)?;
                let r1 = b.rad(e1)?;
                let c2 = b.center(e2)?;
                let r2 = b.rad(e2)?;
                let probe = |kind| eval(&b.finish(owner, kind), x0).v;
                let d = probe(Kind::PtPt { p: c1, q: c2, v: 0.0 });
                let rv1 = probe(Kind::Radius { r: r1, v: 0.0 });
                let rv2 = probe(Kind::Radius { r: r2, v: 0.0 });
                let mode: i8 = if d < rv1.max(rv2) { if rv1 >= rv2 { 1 } else { -1 } } else { 0 };
                out.push(b.finish(owner, Kind::CircTangent { c1, r1, c2, r2, mode }));
            }
        }
        Constraint::Equal(e1, e2) => {
            if is_line(s, e1) {
                one(&|b| {
                    let (a1, b1) = b.line(e1)?;
                    let (a2, b2) = b.line(e2)?;
                    Some(Kind::EqualLen { a1, b1, a2, b2 })
                })?;
            } else {
                one(&|b| Some(Kind::EqualRad { r1: b.rad(e1)?, r2: b.rad(e2)? }))?;
            }
        }
        Constraint::Distance { a, b: e2, value } => {
            if a == e2 {
                one(&|b| {
                    let (pa, pb) = b.line(a)?;
                    Some(Kind::PtPt { p: pa, q: pb, v: value })
                })?;
            } else if is_point(s, a) && is_point(s, e2) {
                one(&|b| Some(Kind::PtPt { p: b.pt(a)?, q: b.pt(e2)?, v: value }))?;
            } else {
                let (p, l) = if is_point(s, a) { (a, e2) } else { (e2, a) };
                let mut b = Builder::new(s, lay);
                let pp = b.pt(p)?;
                let (la, lb) = b.line(l)?;
                let mut eq = b.finish(owner, Kind::PtLine { p: pp, a: la, b: lb, sign: 1.0, v: value });
                if eval(&eq, x0).v + value < 0.0 {
                    eq.kind = Kind::PtLine { p: pp, a: la, b: lb, sign: -1.0, v: value };
                }
                out.push(eq);
            }
        }
        Constraint::Angle { a, b: l2, value } => {
            one(&|b| {
                let (a1, b1) = b.line(a)?;
                let (a2, b2) = b.line(l2)?;
                Some(Kind::Angle { a1, b1, a2, b2, v: value })
            })?;
        }
        Constraint::Radius { c, value } => one(&|b| Some(Kind::Radius { r: b.rad(c)?, v: value }))?,
        Constraint::Diameter { c, value } => one(&|b| Some(Kind::Radius { r: b.rad(c)?, v: value * 0.5 }))?,
        Constraint::Fixed(e) => {
            let ent = s.entity(e)?;
            let mut params: Vec<usize> = Vec::new();
            let mut push_point = |id: EntityId| -> Option<()> {
                let g = lay.param(s, id)?;
                params.extend_from_slice(&[g, g + 1]);
                Some(())
            };
            match ent.geom {
                Geom::Point { .. } => push_point(e)?,
                Geom::Line { a, b } => {
                    push_point(a)?;
                    push_point(b)?;
                }
                Geom::Circle { center, .. } => {
                    push_point(center)?;
                    params.push(lay.param(s, e)?);
                }
                Geom::Arc { center, start, end } => {
                    push_point(center)?;
                    push_point(start)?;
                    push_point(end)?;
                }
                Geom::Spline { .. } => {
                    for p in s.spline_points(e)? {
                        push_point(*p)?;
                    }
                }
            }
            for g in params {
                let mut b = Builder::new(s, lay);
                let a = b.push(g)?;
                out.push(b.finish(owner, Kind::Value { a, v: get(x0, g) }));
            }
        }
        Constraint::Symmetric { a, b: q, axis } => {
            one(&|b| {
                let (p, qq) = (b.pt(a)?, b.pt(q)?);
                let (la, lb) = b.line(axis)?;
                Some(Kind::SymMid { p, q: qq, a: la, b: lb })
            })?;
            one(&|b| {
                let (p, qq) = (b.pt(a)?, b.pt(q)?);
                let (la, lb) = b.line(axis)?;
                Some(Kind::SymPerp { p, q: qq, a: la, b: lb })
            })?;
        }
    }
    Some(())
}

fn at(v: &[Dual; N], o: u8) -> Dual {
    v.get(usize::from(o)).copied().unwrap_or(Dual::ZERO)
}

fn pt(v: &[Dual; N], o: u8) -> (Dual, Dual) {
    (at(v, o), at(v, o.saturating_add(1)))
}

fn radius(v: &[Dual; N], r: R) -> Dual {
    match r {
        R::Param(o) => at(v, o),
        R::Arc { c, s } => {
            let (cx, cy) = pt(v, c);
            let (sx, sy) = pt(v, s);
            Dual::hypot(sx - cx, sy - cy)
        }
    }
}

fn dir(v: &[Dual; N], a: u8, b: u8) -> (Dual, Dual) {
    let (ax, ay) = pt(v, a);
    let (bx, by) = pt(v, b);
    (bx - ax, by - ay)
}

/// Signed distance from `p` to the line through `a → b` (positive on the left).
fn signed_dist(v: &[Dual; N], p: (Dual, Dual), a: u8, b: u8) -> Dual {
    let (ax, ay) = pt(v, a);
    let (dx, dy) = dir(v, a, b);
    let (wx, wy) = (p.0 - ax, p.1 - ay);
    (dx * wy - dy * wx) / Dual::hypot(dx, dy)
}

/// Evaluates one equation: residual value and its exact partial derivatives with
/// respect to the equation's local variables.
pub(crate) fn eval(eq: &Equation, x: &[f64]) -> Dual {
    let mut v = [Dual::ZERO; N];
    for (k, (slot, &g)) in v.iter_mut().zip(eq.locals.iter()).enumerate().take(eq.nloc) {
        *slot = Dual::var(get(x, g), k);
    }
    match eq.kind {
        Kind::Diff { a, b } => at(&v, a) - at(&v, b),
        Kind::Value { a, v: val } => at(&v, a) - Dual::cst(val),
        Kind::Mid { p, a, b } => at(&v, p) - (at(&v, a) + at(&v, b)).scale(0.5),
        Kind::PtPt { p, q, v: val } => {
            let (dx, dy) = dir(&v, q, p);
            Dual::hypot(dx, dy) - Dual::cst(val)
        }
        Kind::PtLine { p, a, b, sign, v: val } => signed_dist(&v, pt(&v, p), a, b).scale(sign) - Dual::cst(val),
        Kind::PtCircle { p, c, r } => {
            let (dx, dy) = dir(&v, c, p);
            Dual::hypot(dx, dy) - radius(&v, r)
        }
        Kind::Radius { r, v: val } => radius(&v, r) - Dual::cst(val),
        Kind::EqualLen { a1, b1, a2, b2 } => {
            let (x1, y1) = dir(&v, a1, b1);
            let (x2, y2) = dir(&v, a2, b2);
            Dual::hypot(x1, y1) - Dual::hypot(x2, y2)
        }
        Kind::EqualRad { r1, r2 } => radius(&v, r1) - radius(&v, r2),
        Kind::Parallel { a1, b1, a2, b2 } | Kind::Perp { a1, b1, a2, b2 } => {
            let (x1, y1) = dir(&v, a1, b1);
            let (x2, y2) = dir(&v, a2, b2);
            let num = if matches!(eq.kind, Kind::Parallel { .. }) { x1 * y2 - y1 * x2 } else { x1 * x2 + y1 * y2 };
            num / (Dual::hypot(x1, y1) * Dual::hypot(x2, y2))
        }
        Kind::Angle { a1, b1, a2, b2, v: val } => {
            let (x1, y1) = dir(&v, a1, b1);
            let (x2, y2) = dir(&v, a2, b2);
            let th = Dual::atan2(x1 * y2 - y1 * x2, x1 * x2 + y1 * y2);
            let tau = core::f64::consts::TAU;
            let diff = th.v - val;
            // Wrap into (−π, π]; the shift is constant so derivatives are unchanged.
            let k = libm::round(diff / tau);
            let mut r = th - Dual::cst(val + k * tau);
            if r.v <= -core::f64::consts::PI {
                r.v += tau;
            }
            r
        }
        Kind::LineTangent { c, r, a, b, sign } => signed_dist(&v, pt(&v, c), a, b).scale(sign) - radius(&v, r),
        Kind::CircTangent { c1, r1, c2, r2, mode } => {
            let (dx, dy) = dir(&v, c1, c2);
            let d = Dual::hypot(dx, dy);
            if mode == 0 {
                d - (radius(&v, r1) + radius(&v, r2))
            } else {
                d - (radius(&v, r1) - radius(&v, r2)).scale(f64::from(mode))
            }
        }
        Kind::EndTangentLine { p, c, a, b } => {
            let (rx, ry) = dir(&v, c, p);
            let (dx, dy) = dir(&v, a, b);
            (rx * dx + ry * dy) / Dual::hypot(dx, dy)
        }
        Kind::EndTangentCurves { p1, c1, p2, c2 } => {
            let (ux, uy) = dir(&v, c1, p1);
            let (wx, wy) = dir(&v, c2, p2);
            (ux * wy - uy * wx) / (Dual::hypot(ux, uy) * Dual::hypot(wx, wy))
        }
        Kind::SymMid { p, q, a, b } => {
            let (px, py) = pt(&v, p);
            let (qx, qy) = pt(&v, q);
            signed_dist(&v, ((px + qx).scale(0.5), (py + qy).scale(0.5)), a, b)
        }
        Kind::SymPerp { p, q, a, b } => {
            let (ux, uy) = dir(&v, p, q);
            let (dx, dy) = dir(&v, a, b);
            (ux * dx + uy * dy) / Dual::hypot(dx, dy)
        }
        Kind::ArcRadius { c, s, e } => {
            let (sx, sy) = dir(&v, c, s);
            let (ex, ey) = dir(&v, c, e);
            Dual::hypot(sx, sy) - Dual::hypot(ex, ey)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::Constraint as C;

    /// Builds a sketch with one of every entity kind in general position.
    fn base() -> (Sketch, Vec<EntityId>) {
        let mut s = Sketch::new();
        let l1 = s.add_line([0.3, -0.2], [4.1, 1.3]).unwrap();
        let l2 = s.add_line([-1.0, 2.0], [0.7, 5.5]).unwrap();
        let p = s.add_point([2.2, 3.1]).unwrap();
        let q = s.add_point([-0.4, 0.9]).unwrap();
        let circ = s.add_circle([6.0, 2.0], 1.7).unwrap();
        let arc = s.add_arc([-3.0, -2.0], [-1.0, -2.5], [-3.5, 0.0]).unwrap();
        let circ2 = s.add_circle([6.5, 2.2], 3.1).unwrap();
        (s, vec![l1, l2, p, q, circ, arc, circ2])
    }

    fn all_constraints(ids: &[EntityId]) -> Vec<C> {
        let &[l1, l2, p, q, circ, arc, circ2] = ids else { panic!() };
        vec![
            C::Coincident(p, q),
            C::PointOnLine(p, l1),
            C::PointOnCircle(p, circ),
            C::PointOnCircle(q, arc),
            C::Midpoint(p, l2),
            C::Horizontal(l1),
            C::Vertical(l2),
            C::Parallel(l1, l2),
            C::Perpendicular(l1, l2),
            C::Tangent(l1, circ),
            C::Tangent(arc, l2),
            C::Tangent(circ, arc),
            C::Tangent(circ, circ2),
            C::Equal(l1, l2),
            C::Equal(circ, arc),
            C::Distance { a: p, b: q, value: 2.0 },
            C::Distance { a: p, b: l1, value: 1.5 },
            C::Distance { a: l2, b: q, value: 0.5 },
            C::Distance { a: l1, b: l1, value: 3.0 },
            C::Angle { a: l1, b: l2, value: 0.7 },
            C::Angle { a: l2, b: l1, value: -2.9 },
            C::Radius { c: circ, value: 2.0 },
            C::Diameter { c: arc, value: 3.0 },
            C::Fixed(arc),
            C::Fixed(circ),
            C::Symmetric { a: p, b: q, axis: l1 },
        ]
    }

    /// Every equation's analytic gradient matches central finite differences.
    #[test]
    fn jacobians_match_finite_differences() {
        let (mut s, ids) = base();
        for c in all_constraints(&ids) {
            s.add_constraint(c).unwrap();
        }
        let lay = Layout::new(&s);
        let eqs = build(&s, &lay).unwrap();
        assert!(eqs.len() > 30);
        // Evaluate away from x0 so that Fixed etc. have non-trivial residuals.
        let x: Vec<f64> = lay.x0.iter().enumerate().map(|(i, v)| v + 0.1 * libm::sin((i as f64) * 1.7)).collect();
        for eq in &eqs {
            let r = eval(eq, &x);
            assert!(r.v.is_finite());
            for k in 0..eq.nloc {
                let g = eq.locals[k];
                let h = 1e-6;
                let mut xp = x.clone();
                xp[g] += h;
                let mut xm = x.clone();
                xm[g] -= h;
                // Several locals may alias the same unknown; sum their derivatives.
                let analytic: f64 = (0..eq.nloc).filter(|&j| eq.locals[j] == g).map(|j| r.d[j]).sum();
                let fd = (eval(eq, &xp).v - eval(eq, &xm).v) / (2.0 * h);
                assert!(
                    (analytic - fd).abs() < 1e-6 * (1.0 + fd.abs()),
                    "{:?}: d/dx{g} analytic {analytic} vs fd {fd}",
                    eq.kind
                );
            }
        }
    }

    #[test]
    fn residuals_have_expected_values() {
        let mut s = Sketch::new();
        let l = s.add_line([0.0, 0.0], [3.0, 4.0]).unwrap();
        let p = s.add_point([0.0, 5.0]).unwrap();
        let c = s.add_circle([10.0, 0.0], 2.0).unwrap();
        s.add_constraint(C::Distance { a: l, b: l, value: 4.0 }).unwrap();
        s.add_constraint(C::Distance { a: p, b: l, value: 1.0 }).unwrap();
        s.add_constraint(C::Diameter { c, value: 3.0 }).unwrap();
        s.add_constraint(C::Horizontal(l)).unwrap();
        let lay = Layout::new(&s);
        let eqs = build(&s, &lay).unwrap();
        let r: Vec<f64> = eqs.iter().map(|e| eval(e, &lay.x0).v).collect();
        assert!((r[0] - 1.0).abs() < 1e-12); // length 5 vs 4
        assert!((r[1] - 2.0).abs() < 1e-12); // |sd| = 3 vs 1
        assert!((r[2] - 0.5).abs() < 1e-12); // r 2 vs 1.5
        assert!((r[3] + 4.0).abs() < 1e-12); // ay - by
    }

    #[test]
    fn angle_residual_wraps() {
        let mut s = Sketch::new();
        let l1 = s.add_line([0.0, 0.0], [1.0, 0.0]).unwrap();
        let l2 = s.add_line([0.0, 0.0], [-1.0, -1e-9]).unwrap();
        s.add_constraint(C::Angle { a: l1, b: l2, value: core::f64::consts::PI }).unwrap();
        let lay = Layout::new(&s);
        let eqs = build(&s, &lay).unwrap();
        let r = eval(&eqs[0], &lay.x0).v;
        assert!(r.abs() < 1e-8, "{r}");
    }
}
