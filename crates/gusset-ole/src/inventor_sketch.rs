//! Solving an Inventor sketch again for an iPart member's row (ADR 0046 amendment 17, note AP of
//! `docs/formats/inventor-feature-tree.md`).
//!
//! An iPart factory stores its sketches solved for its saved row only. A member whose row
//! changes a sketch dimension needs the sketch solved with the member's values: the sketch's
//! constraints and dimensions are read from the records the sketch lists (each refers to the
//! sketch's constraint set `90874d62`), turned into `gusset-sketch` constraints on the saved
//! geometry, and solved from there (moving the geometry as little as possible). How a
//! dimension is measured (which side of a line, which of the angles two lines make, the sign
//! of a horizontal distance) is read off the saved geometry, where the saved value holds.
//!
//! Classes (first 32 bits of the GUID), confirmed on the saved geometry of the AN/MS fitting
//! factories: `90874d94` coincident (point on a curve, or two points), `90874d95` parallel,
//! `90874d96` perpendicular, `90874d97` tangent, `90874d98` horizontal, `90874d99` vertical,
//! `442c7dd0` equal radius, `64da5250` two points aligned, `21e870bf` midpoint, `0ddd7c10`
//! symmetric about a line, `e1108c00` a circle's centre, `fbdb891f` a regular polygon's equal
//! sides and corners, `52534838` the polygon's centre (no equation); dimensions (with a
//! parameter): `11058558` distance, `3683ff40` vertical distance, `00acc000` horizontal
//! distance, `590d0a10` angle, `671bb700` radius, `74df96e0` diameter.

use crate::inventor_dc::{Dc, Role};
use crate::inventor_features::{Curve, Sketch};
use crate::inventor_profile::P2;
use gusset_sketch::{Constraint, EntityId, Sketch as Gs, SolveOptions, SolveStatus};
use std::collections::HashMap;

/// Centimetres to millimetres.
const CM: f64 = 10.0;
/// The class of a sketch's constraint set: every constraint and dimension refers to it.
const G_CONSTRAINT_SET: u32 = 0x9087_4d62;
/// The document record: it refers to every record, the constraint sets among them.
const G_DOC: u32 = 0x9087_4d63;
/// Most constraints read from one sketch.
const MAX_CONSTRAINTS: usize = 5000;
/// How far (mm, or radians) the saved geometry may be from a saved dimension or constraint.
const SAVED_TOL: f64 = 1e-5;
/// A solved line shorter than this (mm) has shrunk to nothing.
const NOTHING: f64 = 1e-6;
/// The last solve's tolerance (mm): far below what the rebuild check sees.
const LOOSE: f64 = 1e-7;

/// What a constraint record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Coincident,
    Parallel,
    Perpendicular,
    Tangent,
    Horizontal,
    Vertical,
    EqualRadius,
    Aligned,
    Midpoint,
    Symmetric,
    Centre,
    Polygon,
    PolygonCentre,
    Distance,
    VDistance,
    HDistance,
    Angle,
    Radius,
    Diameter,
}

fn kind(class: u32) -> Option<Kind> {
    Some(match class {
        0x9087_4d94 => Kind::Coincident,
        0x9087_4d95 => Kind::Parallel,
        0x9087_4d96 => Kind::Perpendicular,
        0x9087_4d97 => Kind::Tangent,
        0x9087_4d98 => Kind::Horizontal,
        0x9087_4d99 => Kind::Vertical,
        0x442c_7dd0 => Kind::EqualRadius,
        0x64da_5250 => Kind::Aligned,
        0x21e8_70bf => Kind::Midpoint,
        0x0ddd_7c10 => Kind::Symmetric,
        0xe110_8c00 => Kind::Centre,
        0xfbdb_891f => Kind::Polygon,
        0x5253_4838 => Kind::PolygonCentre,
        0x1105_8558 => Kind::Distance,
        0x3683_ff40 => Kind::VDistance,
        0x00ac_c000 => Kind::HDistance,
        0x590d_0a10 => Kind::Angle,
        0x671b_b700 => Kind::Radius,
        0x74df_96e0 => Kind::Diameter,
        _ => return None,
    })
}

/// One constraint record: its kind, the sketch entities it names (in record order) and its
/// parameter (dimensions).
struct Con {
    kind: Kind,
    ents: Vec<usize>,
    param: Option<usize>,
    /// A reference dimension: it shows the geometry and drives nothing.
    driven: bool,
}

/// The constraints a sketch lists: the records that refer to its constraint set. Entities and
/// the parameter follow the record header (offset 30 on).
fn constraints(dc: &Dc, s: usize) -> Result<Vec<Con>, String> {
    let mut out = Vec::new();
    for (_, x) in dc.refs(s) {
        if dc.role(x) != Role::Other || dc.class(x) == G_DOC {
            continue;
        }
        let refs = dc.refs(x);
        if !refs.iter().any(|&(_, y)| dc.class(y) == G_CONSTRAINT_SET) {
            continue;
        }
        let k = kind(dc.class(x)).ok_or_else(|| format!("its sketch constraint {:08x} is not read", dc.class(x)))?;
        let param = refs.iter().find(|&&(o, y)| o >= 30 && dc.role(y) == Role::Param).map(|&(_, y)| y);
        // A reference dimension shows the geometry; it does not drive it.
        let driven = param.is_some_and(|p| dc.driven(p));
        if out.len() >= MAX_CONSTRAINTS {
            return Err("its sketch has too many constraints".into());
        }
        let after = |o: usize| o >= 30;
        let ents = refs
            .iter()
            .filter(|&&(o, y)| after(o) && matches!(dc.role(y), Role::SkPoint | Role::SkLine | Role::SkCircle))
            .map(|&(_, y)| y)
            .collect();
        out.push(Con { kind: k, ents, param, driven });
    }
    Ok(out)
}

/// Whether the row changes a dimension of sketch `s`.
pub(crate) fn row_changes(dc: &Dc, s: usize) -> bool {
    dc.has_row()
        && constraints(dc, s).is_ok_and(|cs| cs.iter().any(|c| !c.driven && c.param.is_some_and(|p| dc.in_row(p))))
}

/// A `gusset-sketch` copy of a sketch (mm): every point, line, circle, arc and ellipse, by
/// record. Construction curves stay construction.
fn to_gs(sk: &Sketch) -> Result<(Gs, HashMap<usize, EntityId>), String> {
    let e = |x: gusset_sketch::SketchError| format!("its sketch is not usable: {x}");
    let mut g = Gs::new();
    let mut ids: HashMap<usize, EntityId> = HashMap::new();
    for &(i, u, v, _) in &sk.points {
        ids.insert(i, g.add_point([u, v]).map_err(e)?);
    }
    let id = |ids: &HashMap<usize, EntityId>, i: usize| ids.get(&i).copied().ok_or("a sketch curve's point is missing");
    for c in &sk.curves {
        let (rec, made, con) = match *c {
            Curve::Line { id: rec, a, b, con } => {
                (rec, g.add_line_between(id(&ids, a)?, id(&ids, b)?).map_err(e)?, con)
            }
            Curve::Circle { id: rec, c, r, con } => (rec, g.add_circle_at(id(&ids, c)?, r).map_err(e)?, con),
            Curve::Arc { id: rec, c, s, e: end, con } => {
                (rec, g.add_arc_between(id(&ids, c)?, id(&ids, s)?, id(&ids, end)?).map_err(e)?, con)
            }
            Curve::Ellipse { id: rec, c, major, a, b, con } => {
                let angle = libm::atan2(major[1], major[0]);
                (rec, g.add_ellipse_at(id(&ids, c)?, angle, a, b).map_err(e)?, con)
            }
        };
        if con {
            g.set_construction(made, true).map_err(e)?;
        }
        ids.insert(rec, made);
    }
    Ok((g, ids))
}

/// Line ends, circle and arc centres, by record.
struct Topo {
    line: HashMap<usize, (usize, usize)>,
    circle: HashMap<usize, (usize, Option<(usize, usize)>)>,
}

fn topo(sk: &Sketch) -> Topo {
    let mut t = Topo { line: HashMap::new(), circle: HashMap::new() };
    for c in &sk.curves {
        match *c {
            Curve::Line { id, a, b, .. } => {
                t.line.insert(id, (a, b));
            }
            Curve::Circle { id, c, .. } => {
                t.circle.insert(id, (c, None));
            }
            Curve::Arc { id, c, s, e, .. } => {
                t.circle.insert(id, (c, Some((s, e))));
            }
            Curve::Ellipse { .. } => {}
        }
    }
    t
}

/// The saved geometry, for reading how a dimension is measured.
struct Saved<'a> {
    pts: HashMap<usize, P2>,
    sk: &'a Sketch,
    t: &'a Topo,
}

impl Saved<'_> {
    fn p(&self, i: usize) -> Result<P2, String> {
        self.pts.get(&i).copied().ok_or_else(|| "a constraint names a point the sketch does not hold".into())
    }
    fn line(&self, l: usize) -> Result<(P2, P2), String> {
        let &(a, b) = self.t.line.get(&l).ok_or("a constraint names a line the sketch does not hold")?;
        Ok((self.p(a)?, self.p(b)?))
    }
    /// Signed distance of `p` from line `l` (left of its direction is positive).
    fn side(&self, p: P2, l: usize) -> Result<f64, String> {
        let (a, b) = self.line(l)?;
        let len = libm::hypot(b.0 - a.0, b.1 - a.1);
        if len < 1e-12 {
            return Err("a dimensioned line has no length".into());
        }
        Ok(((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) / len)
    }
    fn radius(&self, c: usize) -> Result<f64, String> {
        self.sk
            .curves
            .iter()
            .find_map(|k| match *k {
                Curve::Circle { id, r, .. } if id == c => Some(Ok(r)),
                Curve::Arc { id, c: cc, s, .. } if id == c => {
                    Some(self.p(cc).and_then(|o| self.p(s).map(|q| libm::hypot(q.0 - o.0, q.1 - o.1))))
                }
                _ => None,
            })
            .unwrap_or_else(|| Err("a constraint names a circle the sketch does not hold".into()))
    }
    fn angle(&self, l: usize) -> Result<f64, String> {
        let (a, b) = self.line(l)?;
        Ok(libm::atan2(b.1 - a.1, b.0 - a.0))
    }
}

/// The angle in (-pi, pi].
fn wrap(a: f64) -> f64 {
    let tau = 2.0 * std::f64::consts::PI;
    let mut x = a % tau;
    if x <= -std::f64::consts::PI {
        x += tau;
    }
    if x > std::f64::consts::PI {
        x -= tau;
    }
    x
}

/// The sketch solved again with the row's values, or `None` when the row changes none of its
/// dimensions (or projected points it moves: `moved`, by record, mm). `projected(i)`: entity `i` is projected (its place comes from elsewhere and is
/// kept). Every dimension of the sketch is read through `dc.param` (so the row's values count
/// as read).
pub(crate) fn resolve(
    dc: &Dc,
    s: usize,
    sk: &Sketch,
    moved: &HashMap<usize, P2>,
    projected: &dyn Fn(usize) -> bool,
) -> Result<Option<Sketch>, String> {
    if !dc.has_row() {
        return Ok(None);
    }
    let cons = constraints(dc, s)?;
    if moved.is_empty() && !cons.iter().any(|c| !c.driven && c.param.is_some_and(|p| dc.in_row(p))) {
        return Ok(None);
    }
    // First as the file has it; then with the angles of lines that end tangent on an arc
    // taken by the arc's radius line instead (the same angle when the line has a length, and
    // still one when the member's row shrinks the line to nothing); last to a looser tolerance
    // (a line shrunk to nothing leaves the solve singular there, and it closes in slowly).
    let mut first = None;
    // Point-to-point distances: the points keep their order, as the lines keep their way.
    let pairs: Vec<(usize, usize)> = cons
        .iter()
        .filter(|c| c.kind == Kind::Distance && !c.driven)
        .filter_map(|c| match c.ents.as_slice() {
            [a, b] if dc.role(*a) == Role::SkPoint && dc.role(*b) == Role::SkPoint => Some((*a, *b)),
            _ => None,
        })
        .collect();
    // Each first holding the lines that run along the sketch's axes as they do (a sketch the
    // file leaves free to turn keeps its way round, as Inventor's solve keeps it), then not.
    // Last, each line that ends tangent on an arc shrunk to nothing in turn (an iPart's formula
    // can make the arc reach the line's far end: the solve is then singular at the shape).
    let t = topo(sk);
    let shrink: Vec<Option<usize>> = std::iter::once(None)
        .chain(sk.curves.iter().filter_map(|c| match *c {
            Curve::Line { id, .. } if tangent_end(&t, &cons, id).is_some() => Some(Some(id)),
            _ => None,
        }))
        .collect();
    let tries = shrink.iter().flat_map(|&collapse| {
        let modes: &[(bool, f64)] = if collapse.is_none() {
            &[(false, 1e-10), (true, 1e-10), (true, LOOSE)]
        } else {
            &[(true, 1e-10), (true, LOOSE)]
        };
        modes.iter().flat_map(move |&m| [(m, true, collapse), (m, false, collapse)])
    });
    for ((by_radius, tolerance), hold, collapse) in tries {
        let max_iterations = if tolerance >= LOOSE { 1000 } else { 100 };
        let (g, ids) = system(dc, sk, &cons, moved, projected, Mode { by_radius, hold, collapse })?;
        let r =
            gusset_sketch::solve_continued(&g, &SolveOptions { tolerance, max_iterations, ..SolveOptions::default() });
        if r.status != SolveStatus::Failed && r.conflicting.is_empty() {
            let out = solved(sk, &ids, &r.sketch)?;
            // A solve that turns a line round found another shape than Inventor's.
            if !turned(sk, &out, &pairs) {
                return Ok(Some(out));
            }
            first.get_or_insert_with(|| "it turns a line round".to_string());
            continue;
        }
        if first.is_none() {
            let conflict = r.conflicting.first().and_then(|&k| g.constraint(k)).map(|k| {
                let d = format!("{k:?}");
                d.split(['(', ' ', '{']).next().unwrap_or_default().to_lowercase()
            });
            first = Some(r.error.map_or_else(
                || format!("its constraints conflict (first: {})", conflict.unwrap_or_default()),
                |x| x.to_string(),
            ));
        }
    }
    Err(format!("its sketch does not solve for the member's row: {}", first.unwrap_or_default()))
}

/// Whether a line of the solved sketch, or one of the point pairs `pairs`, runs the other way
/// than in the saved one.
fn turned(before: &Sketch, after: &Sketch, pairs: &[(usize, usize)]) -> bool {
    let at = |sk: &Sketch| -> HashMap<usize, P2> { sk.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect() };
    let (p0, p1) = (at(before), at(after));
    let lines = after.curves.iter().filter_map(|c| match *c {
        Curve::Line { a, b, .. } => Some((a, b)),
        _ => None,
    });
    lines.chain(pairs.iter().copied()).any(|(a, b)| match (p0.get(&a), p0.get(&b), p1.get(&a), p1.get(&b)) {
        (Some(a0), Some(b0), Some(a1), Some(b1)) => {
            let (u, w) = ((b0.0 - a0.0, b0.1 - a0.1), (b1.0 - a1.0, b1.1 - a1.1));
            libm::hypot(u.0, u.1) > NOTHING && libm::hypot(w.0, w.1) > NOTHING && u.0 * w.0 + u.1 * w.1 < 0.0
        }
        _ => false,
    })
}

/// The saved sketch with the solved places: points, circle radii; lines the row shrank to
/// nothing are left out.
fn solved(sk: &Sketch, ids: &HashMap<usize, EntityId>, g: &Gs) -> Result<Sketch, String> {
    let id = |i: usize| ids.get(&i).copied();
    let mut out = sk.clone();
    for p in &mut out.points {
        let q = id(p.0).and_then(|x| g.point(x)).ok_or("a solved point is missing")?;
        p.1 = crate::inventor_features::rnd(q[0]);
        p.2 = crate::inventor_features::rnd(q[1]);
    }
    for cv in &mut out.curves {
        if let Curve::Circle { id: rec, r: rad, .. } = cv {
            let v = id(*rec).and_then(|x| g.radius(x)).ok_or("a solved circle is missing")?;
            *rad = crate::inventor_features::rnd(v);
        }
    }
    let at: HashMap<usize, P2> = out.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect();
    out.curves.retain(|c| match *c {
        Curve::Line { a, b, .. } => match (at.get(&a), at.get(&b)) {
            (Some(p), Some(q)) => libm::hypot(q.0 - p.0, q.1 - p.1) > NOTHING,
            _ => true,
        },
        _ => true,
    });
    Ok(out)
}

/// The `gusset-sketch` system of a sketch: its geometry, projected entities fixed, and its
/// constraints and dimensions with the row's values.
fn system(
    dc: &Dc,
    sk: &Sketch,
    cons: &[Con],
    moved: &HashMap<usize, P2>,
    projected: &dyn Fn(usize) -> bool,
    mode: Mode,
) -> Result<(Gs, HashMap<usize, EntityId>), String> {
    let Mode { by_radius, hold, collapse } = mode;
    let (mut g, ids) = to_gs(sk)?;
    // Projected points the row moves start (and are fixed) where they move to.
    let mut order: Vec<(&usize, &P2)> = moved.iter().collect();
    order.sort_by_key(|x| *x.0);
    for (&p, &(u, v)) in order {
        let e = ids.get(&p).copied().ok_or("a projected point is missing")?;
        g.set_point(e, [u, v]).map_err(|x| format!("its sketch is not usable: {x}"))?;
    }
    let t = topo(sk);
    let saved = Saved { pts: sk.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect(), sk, t: &t };
    let e = |x: gusset_sketch::SketchError| format!("its sketch constraint does not fit: {x}");
    let id = |i: usize| {
        ids.get(&i).copied().ok_or_else(|| "a constraint names an entity the sketch does not hold".to_string())
    };
    for &(i, ..) in &sk.points {
        if projected(i) {
            g.add_constraint(Constraint::Fixed(id(i)?)).map_err(e)?;
        }
    }
    for c in &sk.curves {
        if let Curve::Circle { id: rec, .. } = *c
            && projected(rec)
        {
            g.add_constraint(Constraint::Fixed(id(rec)?)).map_err(e)?;
        }
    }
    let role = |i: usize| dc.role(i);
    let mut helpers: HashMap<(usize, usize), EntityId> = HashMap::new();
    // Lines that end tangent on an arc -> the arc's radius line there and its turn from the line.
    let mut radial: HashMap<usize, (EntityId, f64)> = HashMap::new();
    if by_radius {
        for &l in t.line.keys().collect::<std::collections::BTreeSet<_>>() {
            if let Some(h) = radius_line(&mut g, &saved, cons, &id, l)? {
                radial.insert(l, h);
            }
        }
    }
    if let Some(l) = collapse {
        let &(a, b) = t.line.get(&l).ok_or("a shrunk line is missing")?;
        push(&mut g, Constraint::Coincident(id(a)?, id(b)?))?;
    }
    for c in cons {
        // A line shrunk to nothing has no way to run: what holds its way goes.
        let ways =
            matches!(c.kind, Kind::Parallel | Kind::Perpendicular | Kind::Horizontal | Kind::Vertical | Kind::Tangent);
        if ways && collapse.is_some_and(|l| c.ents.contains(&l)) {
            continue;
        }
        if c.driven {
            // Read all the same: the member's row holds its measured value.
            let _ = c.param.and_then(|p| dc.param(p));
            continue;
        }
        let ents = c.ents.as_slice();
        // A dimension's value (mm or radians) and the value the file saved.
        let value = || -> Result<(f64, f64), String> {
            let p = c.param.ok_or("a sketch dimension has no parameter")?;
            let (_, now) = dc.param(p).ok_or("a sketch dimension's parameter is not readable")?;
            let (_, was) = dc.param_saved(p).ok_or("a sketch dimension's parameter is not readable")?;
            let unit = if c.kind == Kind::Angle { 1.0 } else { CM };
            Ok((now * unit, was * unit))
        };
        match c.kind {
            Kind::PolygonCentre => {}
            Kind::Coincident => {
                let [.., a, b] = ents else {
                    return Err("a coincident constraint names fewer than two entities".into());
                };
                let (p, q) = (*a, *b);
                match (role(p), role(q)) {
                    (Role::SkPoint, Role::SkPoint) => {
                        if p != q {
                            push(&mut g, Constraint::Coincident(id(p)?, id(q)?))?;
                        }
                    }
                    (Role::SkPoint, _) => on_curve(&mut g, &t, &id, q, p)?,
                    (_, Role::SkPoint) => on_curve(&mut g, &t, &id, p, q)?,
                    _ => return Err("a coincident constraint between two curves is not read".into()),
                }
            }
            Kind::Parallel | Kind::Perpendicular => {
                let [a, b] = ents else { return Err("a parallel constraint does not name two lines".into()) };
                let (a, b) = (id(*a)?, id(*b)?);
                push(
                    &mut g,
                    if c.kind == Kind::Parallel { Constraint::Parallel(a, b) } else { Constraint::Perpendicular(a, b) },
                )?;
            }
            Kind::Horizontal | Kind::Vertical => {
                let [l] = ents else { return Err("a horizontal constraint does not name one line".into()) };
                let l = id(*l)?;
                push(
                    &mut g,
                    if c.kind == Kind::Horizontal { Constraint::Horizontal(l) } else { Constraint::Vertical(l) },
                )?;
            }
            Kind::Tangent => {
                let mut u: Vec<usize> = Vec::new();
                for &x in ents {
                    if !u.contains(&x) {
                        u.push(x);
                    }
                }
                let [a, b] = u.as_slice() else { return Err("a tangent constraint does not name two curves".into()) };
                // A line with a radius line stands square to it, whichever way it runs (the row
                // may shrink it through nothing on the way).
                if let Some(&(h, _)) = radial.get(a).or_else(|| radial.get(b)) {
                    let l = if radial.contains_key(a) { *a } else { *b };
                    push(&mut g, Constraint::Perpendicular(id(l)?, h))?;
                } else {
                    push(&mut g, Constraint::Tangent(id(*a)?, id(*b)?))?;
                }
            }
            Kind::EqualRadius => {
                let [a, b] = ents else { return Err("an equal constraint does not name two curves".into()) };
                push(&mut g, Constraint::Equal(id(*a)?, id(*b)?))?;
            }
            Kind::Aligned => {
                let [a, b] = ents else { return Err("an alignment does not name two points".into()) };
                let (pa, pb) = (saved.p(*a)?, saved.p(*b)?);
                let (from, to) = (Some(id(*a)?), id(*b)?);
                if (pa.0 - pb.0).abs() < SAVED_TOL {
                    push(&mut g, Constraint::HDistance { from, to, value: 0.0 })?;
                } else if (pa.1 - pb.1).abs() < SAVED_TOL {
                    push(&mut g, Constraint::VDistance { from, to, value: 0.0 })?;
                } else {
                    return Err("two aligned points are not aligned in the saved sketch".into());
                }
            }
            Kind::Midpoint => {
                let [l, p] = ents else { return Err("a midpoint constraint does not name a line and a point".into()) };
                push(&mut g, Constraint::Midpoint(id(*p)?, id(*l)?))?;
            }
            Kind::Symmetric => {
                let [a, b, l] = ents else { return Err("a symmetry does not name two points and a line".into()) };
                push(&mut g, Constraint::Symmetric { a: id(*a)?, b: id(*b)?, axis: id(*l)? })?;
            }
            Kind::Centre => {
                let [cv, p] = ents else { return Err("a centre constraint does not name a circle and a point".into()) };
                let &(cen, _) = t.circle.get(cv).ok_or("a centre constraint names no circle")?;
                if cen != *p {
                    push(&mut g, Constraint::Coincident(id(cen)?, id(*p)?))?;
                }
            }
            Kind::Polygon => {
                let [a, b, centre] = ents else {
                    return Err("a polygon constraint does not name three entities".into());
                };
                match (role(*a), role(*b)) {
                    (Role::SkLine, Role::SkLine) => push(&mut g, Constraint::Equal(id(*a)?, id(*b)?))?,
                    (Role::SkPoint, Role::SkPoint) => {
                        // Corners equally far from the centre: equal construction spokes.
                        let mut spoke = |q: usize| -> Result<EntityId, String> {
                            if let Some(&h) = helpers.get(&(*centre, q)) {
                                return Ok(h);
                            }
                            let h = g.add_line_between(id(*centre)?, id(q)?).map_err(e)?;
                            g.set_construction(h, true).map_err(e)?;
                            helpers.insert((*centre, q), h);
                            Ok(h)
                        };
                        let (ha, hb) = (spoke(*a)?, spoke(*b)?);
                        g.add_constraint(Constraint::Equal(ha, hb)).map_err(e)?;
                    }
                    _ => return Err("a polygon constraint names other entities".into()),
                }
            }
            Kind::Distance => {
                let [a, b] = ents else { return Err("a distance does not name two entities".into()) };
                let (now, was) = value()?;
                distance(&mut g, &saved, &id, (*a, *b), (role(*a), role(*b)), now, was)?;
            }
            Kind::HDistance | Kind::VDistance => {
                let [a, b] = ents else { return Err("a distance does not name two points".into()) };
                let (now, was) = value()?;
                let (pa, pb) = (saved.p(*a)?, saved.p(*b)?);
                let d = if c.kind == Kind::HDistance { pb.0 - pa.0 } else { pb.1 - pa.1 };
                if (d.abs() - was).abs() > SAVED_TOL {
                    return Err("a sketch dimension does not hold in the saved sketch".into());
                }
                let value = if d < 0.0 { -now } else { now };
                let (from, to) = (Some(id(*a)?), id(*b)?);
                push(
                    &mut g,
                    if c.kind == Kind::HDistance {
                        Constraint::HDistance { from, to, value }
                    } else {
                        Constraint::VDistance { from, to, value }
                    },
                )?;
            }
            Kind::Angle => {
                let [a, b] = ents else { return Err("an angle does not name two lines".into()) };
                let (now, was) = value()?;
                let theta = wrap(saved.angle(*b)? - saved.angle(*a)?);
                // The angle the dimension shows is one of the four the lines make.
                let pi = std::f64::consts::PI;
                let forms: [(f64, f64); 4] = [(1.0, 0.0), (-1.0, 0.0), (-1.0, pi), (1.0, -pi)];
                let (sgn, off) = forms
                    .iter()
                    .copied()
                    .min_by(|x, y| {
                        let dx = wrap(x.0 * was + x.1 - theta).abs();
                        let dy = wrap(y.0 * was + y.1 - theta).abs();
                        dx.total_cmp(&dy)
                    })
                    .unwrap_or((1.0, 0.0));
                if wrap(sgn * was + off - theta).abs() > SAVED_TOL {
                    return Err("a sketch angle does not hold in the saved sketch".into());
                }
                let mut value = wrap(sgn * now + off);
                let (mut la, mut lb) = (id(*a)?, id(*b)?);
                // A line that ends tangent on an arc turns with the arc's radius line there: the
                // angle moves by the radius line's turn from the line (saved geometry).
                if let Some(&(h, turn)) = radial.get(a) {
                    la = h;
                    value = wrap(value - turn);
                }
                if let Some(&(h, turn)) = radial.get(b) {
                    lb = h;
                    value = wrap(value + turn);
                }
                push(&mut g, Constraint::Angle { a: la, b: lb, value })?;
            }
            Kind::Radius | Kind::Diameter => {
                let [cv] = ents else { return Err("a radius does not name one circle".into()) };
                let (now, _) = value()?;
                let cv = id(*cv)?;
                push(
                    &mut g,
                    if c.kind == Kind::Radius {
                        Constraint::Radius { c: cv, value: now }
                    } else {
                        Constraint::Diameter { c: cv, value: now }
                    },
                )?;
            }
        }
    }
    if hold {
        for c in &sk.curves {
            let Curve::Line { id: l, a, b, .. } = *c else { continue };
            if (projected(a) && projected(b)) || collapse == Some(l) {
                continue;
            }
            let (p, q) = (saved.p(a)?, saved.p(b)?);
            let len = libm::hypot(q.0 - p.0, q.1 - p.1);
            if len < NOTHING {
                continue;
            }
            if (q.1 - p.1).abs() < 1e-12 * len {
                push(&mut g, Constraint::Horizontal(id(l)?))?;
            } else if (q.0 - p.0).abs() < 1e-12 * len {
                push(&mut g, Constraint::Vertical(id(l)?))?;
            }
        }
    }
    Ok((g, ids))
}

/// How [`system`] states the sketch: angles of lines that end tangent on an arc by the arc's
/// radius line; lines along the sketch's axes held so; one line shrunk to nothing.
#[derive(Clone, Copy)]
struct Mode {
    by_radius: bool,
    hold: bool,
    collapse: Option<usize>,
}

/// The arc a line ends tangent on (a tangent constraint names both and the line's end is one
/// of the arc's): the arc and that end.
fn tangent_end(t: &Topo, cons: &[Con], l: usize) -> Option<(usize, usize)> {
    let &(la, lb) = t.line.get(&l)?;
    cons.iter().filter(|c| c.kind == Kind::Tangent && c.ents.contains(&l)).find_map(|c| {
        c.ents.iter().find_map(|arc| {
            let (s, e) = t.circle.get(arc)?.1?;
            [la, lb].into_iter().find(|&p| p == s || p == e).map(|end| (*arc, end))
        })
    })
}

/// The radius line of the arc a line ends tangent on (a tangent constraint names both and the
/// line's end is one of the arc's), drawn from the arc's centre to that end as construction,
/// with its turn from the line in the saved sketch.
fn radius_line(
    g: &mut Gs,
    saved: &Saved<'_>,
    cons: &[Con],
    id: &dyn Fn(usize) -> Result<EntityId, String>,
    l: usize,
) -> Result<Option<(EntityId, f64)>, String> {
    let Some((arc, end)) = tangent_end(saved.t, cons, l) else { return Ok(None) };
    let &(cen, _) = saved.t.circle.get(&arc).ok_or("a tangent arc is missing")?;
    let (o, q) = (saved.p(cen)?, saved.p(end)?);
    let h = g.add_line_between(id(cen)?, id(end)?).map_err(|x| format!("its sketch is not usable: {x}"))?;
    g.set_construction(h, true).map_err(|x| format!("its sketch is not usable: {x}"))?;
    let turn = wrap(libm::atan2(q.1 - o.1, q.0 - o.0) - saved.angle(l)?);
    Ok(Some((h, turn)))
}

/// Adds a constraint to the copy.
fn push(g: &mut Gs, k: Constraint) -> Result<(), String> {
    g.add_constraint(k).map(|_| ()).map_err(|x| format!("its sketch constraint does not fit: {x}"))
}

/// A point on a curve: nothing when it is one of the curve's own ends or its centre.
fn on_curve(
    g: &mut Gs,
    t: &Topo,
    id: &dyn Fn(usize) -> Result<EntityId, String>,
    curve: usize,
    p: usize,
) -> Result<(), String> {
    if let Some(&(a, b)) = t.line.get(&curve) {
        if p != a && p != b {
            push(g, Constraint::PointOnLine(id(p)?, id(curve)?))?;
        }
        return Ok(());
    }
    if let Some(&(c, ends)) = t.circle.get(&curve) {
        if p != c && ends.is_none_or(|(s, e)| p != s && p != e) {
            push(g, Constraint::PointOnCircle(id(p)?, id(curve)?))?;
        }
        return Ok(());
    }
    Err("a coincident constraint names a curve that is not read".into())
}

/// A distance dimension between two entities: points, a point and a line, two (parallel)
/// lines, or a circle and a line (to the circle's near side, through a construction line
/// tangent to the circle).
fn distance(
    g: &mut Gs,
    saved: &Saved<'_>,
    id: &dyn Fn(usize) -> Result<EntityId, String>,
    (a, b): (usize, usize),
    roles: (Role, Role),
    now: f64,
    was: f64,
) -> Result<(), String> {
    let e = |x: gusset_sketch::SketchError| format!("its sketch dimension does not fit: {x}");
    let check = |d: f64| {
        if (d - was).abs() > SAVED_TOL {
            Err("a sketch dimension does not hold in the saved sketch".to_string())
        } else {
            Ok(())
        }
    };
    match roles {
        (Role::SkPoint, Role::SkPoint) => {
            let (pa, pb) = (saved.p(a)?, saved.p(b)?);
            check(libm::hypot(pb.0 - pa.0, pb.1 - pa.1))?;
            g.add_constraint(Constraint::Distance { a: id(a)?, b: id(b)?, value: now }).map_err(e)?;
        }
        (Role::SkPoint, Role::SkLine) | (Role::SkLine, Role::SkPoint) => {
            let (p, l) = if roles.0 == Role::SkPoint { (a, b) } else { (b, a) };
            check(saved.side(saved.p(p)?, l)?.abs())?;
            g.add_constraint(Constraint::Distance { a: id(p)?, b: id(l)?, value: now }).map_err(e)?;
        }
        (Role::SkLine, Role::SkLine) => {
            let &(q, _) = saved.t.line.get(&b).ok_or("a dimensioned line is missing")?;
            check(saved.side(saved.p(q)?, a)?.abs())?;
            g.add_constraint(Constraint::Distance { a: id(q)?, b: id(a)?, value: now }).map_err(e)?;
        }
        (Role::SkCircle, Role::SkLine) | (Role::SkLine, Role::SkCircle) => {
            let (c, l) = if roles.0 == Role::SkCircle { (a, b) } else { (b, a) };
            let &(cen, _) = saved.t.circle.get(&c).ok_or("a dimensioned circle is missing")?;
            let (o, r) = (saved.p(cen)?, saved.radius(c)?);
            let d = saved.side(o, l)?;
            // The tangent point on the side the dimension measures to.
            let (la, lb) = saved.line(l)?;
            let len = libm::hypot(lb.0 - la.0, lb.1 - la.1);
            let n = (-(lb.1 - la.1) / len, (lb.0 - la.0) / len);
            let s = if d >= 0.0 { 1.0 } else { -1.0 };
            let cands = [r, -r].map(|k| (o.0 - s * k * n.0, o.1 - s * k * n.1));
            let pick = cands
                .iter()
                .copied()
                .find(|&tp| saved.side(tp, l).is_ok_and(|x| (x.abs() - was).abs() <= SAVED_TOL))
                .ok_or("a sketch dimension does not hold in the saved sketch")?;
            let dir = ((lb.0 - la.0) / len, (lb.1 - la.1) / len);
            let h = g.add_line([pick.0 - dir.0, pick.1 - dir.1], [pick.0 + dir.0, pick.1 + dir.1]).map_err(e)?;
            g.set_construction(h, true).map_err(e)?;
            let (ha, _) = g.line_points(h).ok_or("a construction line is missing")?;
            g.add_constraint(Constraint::Parallel(h, id(l)?)).map_err(e)?;
            g.add_constraint(Constraint::Tangent(h, id(c)?)).map_err(e)?;
            g.add_constraint(Constraint::Distance { a: ha, b: id(l)?, value: now }).map_err(e)?;
        }
        _ => return Err("a distance between these entities is not read".into()),
    }
    Ok(())
}

/// Where a profile pick made on the saved sketch lands on the solved one: the region holding
/// it in the saved sketch, found again in the solved one by the pieces of curves around it.
pub(crate) fn map_pick(before: &Sketch, after: &Sketch, p: P2) -> Option<P2> {
    const TOL: f64 = 1e-6;
    let (g0, _) = to_gs(before).ok()?;
    let (g1, _) = to_gs(after).ok()?;
    let a0 = gusset_sketch::arrange(&g0, TOL).ok()?;
    let a1 = gusset_sketch::arrange(&g1, TOL).ok()?;
    let q = [p.0, p.1];
    let inside = |pr: &gusset_sketch::Profile| pr.outer.contains(q) && !pr.holes.iter().any(|h| h.contains(q));
    let region = a0.profiles.iter().filter(|pr| inside(pr)).min_by(|x, y| x.area.total_cmp(&y.area))?;
    let found = a1.profiles.iter().find(|pr| pr.pieces == region.pieces)?;
    Some((found.label[0], found.label[1]))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
    use super::*;

    /// A rectangle `w` x `h` (mm) split by a vertical line at `x`: points 1..=6, lines 11..=17.
    fn split_box(w: f64, h: f64, x: f64) -> Sketch {
        let points = vec![
            (1, 0.0, 0.0, false),
            (2, w, 0.0, false),
            (3, w, h, false),
            (4, 0.0, h, false),
            (5, x, 0.0, false),
            (6, x, h, false),
        ];
        let line = |id, a, b| Curve::Line { id, a, b, con: false };
        let curves = vec![
            line(11, 1, 5),
            line(12, 5, 2),
            line(13, 2, 3),
            line(14, 3, 6),
            line(15, 6, 4),
            line(16, 4, 1),
            line(17, 5, 6),
        ];
        Sketch { name: "S".into(), m: [0.0; 16], points, curves, before: None }
    }

    #[test]
    fn angles_wrap_into_a_half_turn_either_way() {
        let pi = std::f64::consts::PI;
        assert!((wrap(3.0 * pi / 2.0) + pi / 2.0).abs() < 1e-12);
        assert!((wrap(-pi) - pi).abs() < 1e-12);
        assert_eq!(wrap(0.25), 0.25);
    }

    /// A pick on the saved sketch lands in the same region of the solved one, found by the
    /// curves around it, wherever the region moved.
    #[test]
    fn a_pick_follows_its_region() {
        let (before, after) = (split_box(20.0, 10.0, 5.0), split_box(40.0, 10.0, 30.0));
        let right = map_pick(&before, &after, (15.0, 5.0)).unwrap();
        assert!(right.0 > 30.0 && right.0 < 40.0, "{right:?}");
        let left = map_pick(&before, &after, (2.0, 5.0)).unwrap();
        assert!(left.0 > 0.0 && left.0 < 30.0, "{left:?}");
        assert_eq!(map_pick(&before, &after, (25.0, 5.0)), None);
    }

    /// A solve that runs a line, or a dimensioned pair of points, the other way is not
    /// Inventor's shape.
    #[test]
    fn a_line_or_a_pair_that_turns_round_is_noticed() {
        let before = split_box(20.0, 10.0, 5.0);
        assert!(!turned(&before, &split_box(40.0, 10.0, 30.0), &[(1, 5)]));
        // The split line moves past the right side: line 12 (5 -> 2) runs back.
        assert!(turned(&before, &split_box(20.0, 10.0, 25.0), &[]));
        let mut flipped = before.clone();
        flipped.curves.clear();
        flipped.points[4].1 = -5.0;
        assert!(turned(&before, &flipped, &[(1, 5)]));
        assert!(!turned(&before, &flipped, &[]));
    }
}
