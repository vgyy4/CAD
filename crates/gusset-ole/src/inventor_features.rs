//! An Inventor part's feature history as a Gusset feature tree (`gusset-feature-tree` v2),
//! read from the `PmDCSegment` records of the `.ipt` and, for fillets and chamfers, from the
//! ShapeManager result body. Clean room, from sample bytes (`docs/formats/inventor-feature-tree.md`).
//!
//! What is read: sketches (a mask-compressed 4x4 placement in cm, points, lines, circles and
//! arcs), extrudes, revolves, holes, fillets and chamfers, in history order. Fillet and chamfer
//! edges are not stored as geometry: the result body's blend faces carry the feature's id and
//! the original edge is rebuilt from the faces beside them (see `acis`).
//!
//! The reader does not judge the tree. It is only a candidate: the caller rebuilds it and
//! keeps it when it gives the part's own body. A hole carries `_fallback`, the same hole as
//! plain cuts, for the caller to try when the hole feature itself is refused (keys that start
//! with `_` are for the caller and must be removed before the tree is parsed).
//!
//! Input is untrusted: every count, list, chain and walk is bounded, and a record that is not
//! shaped as expected gives an `unsupported` feature or an error, never a panic.

use crate::acis::{Acis, BridgedRound, EdgeInfo, EndSurface, FullRound, RevSeg, Surf, V3, cross, dot, local_path};
use crate::inventor_dc::{Dc, FeatType, Feature, G_BODY_LIST, G_SOLID_BODY, Role, f64_at, u16_at, u32_at};
use crate::inventor_profile::{P2, Poly, picks, sample};
use crate::{Limits, inventor};
use serde_json::{Value, json};
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

/// Centimetres to millimetres.
const CM: f64 = 10.0;
/// Most features read from one part.
const MAX_FEATURES: usize = 2000;
/// Most curves of one sketch.
const MAX_CURVES: usize = 20_000;
/// How far either side of a blend edge's midpoint its pick's path reaches, in file units (cm): 0.1 mm.
const PATH_EPS: f64 = 0.01;
/// A through-all extent, in millimetres.
const THROUGH: f64 = 1e4;

pub(crate) fn rnd(v: f64) -> f64 {
    rnd_to(v, 1e9)
}

fn rnd_to(v: f64, scale: f64) -> f64 {
    libm::round(v * scale) / scale + 0.0
}

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a` turned by `t` radians about the unit vector `k` (right-handed).
fn rotate(a: V3, k: V3, t: f64) -> V3 {
    let (c, s) = (libm::cos(t), libm::sin(t));
    let kxa = cross(k, a);
    let ka = dot(k, a) * (1.0 - c);
    [a[0] * c + kxa[0] * s + k[0] * ka, a[1] * c + kxa[1] * s + k[1] * ka, a[2] * c + kxa[2] * s + k[2] * ka]
}

fn unit3(a: V3) -> V3 {
    let l = libm::sqrt(dot(a, a));
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Sets a key of a JSON object.
fn put(o: &mut Value, key: &str, v: Value) {
    if let Some(m) = o.as_object_mut() {
        m.insert(key.to_string(), v);
    }
}

fn arr(v: V3) -> Value {
    json!([rnd(v[0]), rnd(v[1]), rnd(v[2])])
}

/// Origin (mm) and axes of a placement matrix.
fn frame(m: &[f64; 16]) -> (V3, V3, V3) {
    let g = |i: usize| m.get(i).copied().unwrap_or(0.0);
    ([g(3) * CM, g(7) * CM, g(11) * CM], [g(0), g(4), g(8)], [g(1), g(5), g(9)])
}

/// A feature's slots with one record's bytes at hand.
struct Slots(Vec<Option<usize>>);

impl Slots {
    fn get(&self, j: usize) -> Option<usize> {
        self.0.get(j).copied().flatten()
    }
    fn need(&self, j: usize) -> Result<usize, String> {
        self.get(j).ok_or_else(|| format!("slot {j} is empty"))
    }
}

/// u16 at the end of a record (operation, extent).
fn tail_u16(dc: &Dc, i: usize) -> Option<u16> {
    let r = dc.r(i);
    u16_at(r, r.len().checked_sub(2)?)
}

#[derive(Clone)]
pub(crate) struct Sketch {
    pub(crate) name: String,
    pub(crate) m: [f64; 16],
    /// (record, x, y, projected), mm.
    pub(crate) points: Vec<(usize, f64, f64, bool)>,
    pub(crate) curves: Vec<Curve>,
    /// The sketch as saved, when it was solved again for an iPart member's row (the profile
    /// loops of the history body show the saved sketch).
    pub(crate) before: Option<Box<Sketch>>,
}

#[derive(Clone)]
pub(crate) enum Curve {
    Line {
        id: usize,
        a: usize,
        b: usize,
        con: bool,
    },
    Arc {
        id: usize,
        c: usize,
        s: usize,
        e: usize,
        con: bool,
    },
    Circle {
        id: usize,
        c: usize,
        r: f64,
        con: bool,
    },
    /// Centre point, unit major direction, semi-axes (mm).
    Ellipse {
        id: usize,
        c: usize,
        major: [f64; 2],
        a: f64,
        b: f64,
        con: bool,
    },
    /// A clamped B-spline from point `s` to point `e`: its first and last poles are those
    /// points, the others are given (mm); weights empty for a polynomial curve (note AR).
    Spline {
        id: usize,
        s: usize,
        e: usize,
        degree: u32,
        knots: Vec<f64>,
        poles: Vec<P2>,
        weights: Vec<f64>,
        con: bool,
    },
}

impl Sketch {
    fn json(&self) -> Value {
        let (o, x, y) = frame(&self.m);
        let points: Vec<Value> = self
            .points
            .iter()
            .map(|&(i, u, v, con)| {
                let mut p = json!({"id": format!("p{i}"), "x": u, "y": v});
                if con {
                    put(&mut p, "construction", json!(true));
                }
                p
            })
            .collect();
        let mut points = points;
        for c in &self.curves {
            if let Curve::Spline { id, ref poles, .. } = *c {
                for (k, q) in poles.iter().enumerate().skip(1).take(poles.len().saturating_sub(2)) {
                    points.push(json!({"id": format!("p{id}_{k}"), "x": q.0, "y": q.1, "construction": true}));
                }
            }
        }
        // A line shorter than the profile tolerance (5e-8 mm between two points the file keeps
        // apart, note AR) closes no gap: left out, its ends are one point to the regions. So is
        // an arc of no radius (a sketch fillet of radius 0: centre and ends at one place).
        let at: HashMap<usize, (f64, f64)> = self.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect();
        let near = |a: usize, b: usize| matches!((at.get(&a), at.get(&b)), (Some(p), Some(q)) if libm::hypot(p.0 - q.0, p.1 - q.1) < 1e-6);
        let empty = |c: &Curve| match *c {
            Curve::Line { a, b, .. } => near(a, b),
            Curve::Arc { c, s, e, .. } => near(c, s) && near(c, e),
            _ => false,
        };
        let curves: Vec<Value> = self
            .curves
            .iter()
            .filter(|c| !empty(c))
            .map(|c| {
                let (mut j, con) = match *c {
                    Curve::Line { id, a, b, con } => {
                        (json!({"id": format!("l{id}"), "type": "line", "start": format!("p{a}"), "end": format!("p{b}")}), con)
                    }
                    // An arc whose ends meet runs the whole turn (as the profile picks read it).
                    Curve::Arc { id, c, s, e, con } if near(s, e) => {
                        let r = match (at.get(&c), at.get(&s)) {
                            (Some(p), Some(q)) => rnd(libm::hypot(p.0 - q.0, p.1 - q.1)),
                            _ => 0.0,
                        };
                        (json!({"id": format!("a{id}"), "type": "circle", "center": format!("p{c}"), "radius": r}), con)
                    }
                    Curve::Arc { id, c, s, e, con } => (
                        json!({"id": format!("a{id}"), "type": "arc", "center": format!("p{c}"), "start": format!("p{s}"), "end": format!("p{e}")}),
                        con,
                    ),
                    Curve::Circle { id, c, r, con } => {
                        (json!({"id": format!("c{id}"), "type": "circle", "center": format!("p{c}"), "radius": r}), con)
                    }
                    Curve::Ellipse { id, c, major, a, b, con } => (
                        json!({"id": format!("e{id}"), "type": "ellipse", "center": format!("p{c}"), "major": major, "a": a, "b": b}),
                        con,
                    ),
                    Curve::Spline { id, s, e, degree, ref knots, ref poles, ref weights, con } => {
                        let n = poles.len();
                        let ids: Vec<String> = (0..n)
                            .map(|k| match k {
                                0 => format!("p{s}"),
                                k if k + 1 == n => format!("p{e}"),
                                k => format!("p{id}_{k}"),
                            })
                            .collect();
                        let mut j = json!({"id": format!("b{id}"), "type": "bspline", "degree": degree,
                                           "knots": knots, "poles": ids});
                        if !weights.is_empty() {
                            put(&mut j, "weights", json!(weights));
                        }
                        (j, con)
                    }
                };
                if con {
                    put(&mut j, "construction", json!(true));
                }
                j
            })
            .collect();
        json!({"type": "sketch", "name": self.name,
               "plane": {"origin": arr(o), "x_axis": arr(x), "y_axis": arr(y)},
               "points": points, "curves": curves})
    }

    /// The non-construction curves in the sketch plane (mm), for the profile picks.
    fn polys(&self) -> Vec<Poly> {
        let p: HashMap<usize, P2> = self.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect();
        let mut out = Vec::new();
        for c in &self.curves {
            match *c {
                Curve::Line { a, b, con: false, .. } => {
                    if let (Some(&a), Some(&b)) = (p.get(&a), p.get(&b)) {
                        out.push(Poly::Line(a, b));
                    }
                }
                Curve::Circle { c, r, con: false, .. } => {
                    if let Some(&c) = p.get(&c) {
                        out.push(Poly::Circle(c, r));
                    }
                }
                Curve::Arc { c, s, e, con: false, .. } => {
                    if let (Some(&c), Some(&s), Some(&e)) = (p.get(&c), p.get(&s), p.get(&e)) {
                        out.push(Poly::Arc { c, s, e });
                    }
                }
                Curve::Ellipse { c, major, a, b, con: false, .. } => {
                    if let Some(&c) = p.get(&c) {
                        out.push(Poly::Ellipse { c, major: (major[0], major[1]), a, b });
                    }
                }
                Curve::Spline { degree, ref knots, ref poles, ref weights, con: false, .. } => {
                    if let Some(pts) = crate::inventor_profile::bspline_points(degree, knots, poles, weights) {
                        out.push(Poly::Path(pts));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

struct Reader<'a> {
    dc: Dc,
    /// The part whose stored body is the result the blends are read from: the part itself, or
    /// an iPart member read through its factory's history.
    result_bytes: &'a [u8],
    limits: &'a Limits,
    /// Records that refer to each record.
    back: OnceCell<Vec<Vec<usize>>>,
    /// Sketch contents by sketch record.
    owned: OnceCell<HashMap<usize, Vec<usize>>>,
    history: OnceCell<Option<Acis>>,
    result: OnceCell<Option<Acis>>,
    /// Sketches solved again for an iPart member's row, by sketch record.
    solved: RefCell<HashMap<usize, Sketch>>,
    /// The sketches whose dimensions the row changes.
    changed: OnceCell<Vec<usize>>,
}

impl Reader<'_> {
    fn back(&self) -> &Vec<Vec<usize>> {
        self.back.get_or_init(|| {
            let mut br = vec![Vec::new(); self.dc.n()];
            for i in 0..self.dc.n() {
                for (_, x) in self.dc.refs(i) {
                    if let Some(l) = br.get_mut(x) {
                        l.push(i);
                    }
                }
            }
            br
        })
    }

    fn owner(&self, i: usize) -> Option<usize> {
        super::inventor_dc::reference(u32_at(self.dc.r(i), self.dc.body(i) + 4)?, self.dc.n())
    }

    fn owned(&self, s: usize) -> &[usize] {
        let m = self.owned.get_or_init(|| {
            let mut m: HashMap<usize, Vec<usize>> = HashMap::new();
            for i in 0..self.dc.n() {
                if matches!(
                    self.dc.role(i),
                    Role::SkPoint | Role::SkLine | Role::SkCircle | Role::SkEllipse | Role::SkSpline
                ) && let Some(o) = self.owner(i)
                {
                    m.entry(o).or_default().push(i);
                }
            }
            m
        });
        m.get(&s).map_or(&[], Vec::as_slice)
    }

    fn flags(&self, i: usize) -> u32 {
        u32_at(self.dc.r(i), self.dc.body(i)).unwrap_or(0)
    }

    fn history(&self) -> Option<&Acis> {
        self.history
            .get_or_init(|| {
                let hb = (0..self.dc.n()).find(|&i| self.dc.role(i) == Role::HistoryBlob)?;
                let r = self.dc.r(hb);
                let k = r.windows(14).position(|w| w == b"ASM BinaryFile")?;
                Acis::parse(r.get(k..)?).ok()
            })
            .as_ref()
    }

    fn result(&self) -> Option<&Acis> {
        self.result
            .get_or_init(|| {
                let e = inventor::extract_sab(self.result_bytes, self.limits).ok()?;
                Acis::parse(&e.sab).ok()
            })
            .as_ref()
    }

    /// A sketch as the features see it: as saved, or solved again with the iPart member's row
    /// when the row changes one of its dimensions (note AP). A sketch that projects edges of
    /// the model is refused when the row changes another sketch: the projected geometry would
    /// move with it.
    fn sketch(&self, s: usize) -> Result<Sketch, String> {
        if !self.dc.has_row() {
            return self.saved_sketch(s);
        }
        if let Some(done) = self.solved.borrow().get(&s) {
            return Ok(done.clone());
        }
        let changed = self.changed.get_or_init(|| {
            (0..self.dc.n())
                .filter(|&i| self.dc.role(i) == Role::Sketch && crate::inventor_sketch::row_changes(&self.dc, i))
                .collect()
        });
        let sk = self.saved_sketch(s)?;
        let others = changed.iter().any(|&c| c != s);
        let placed = if others { self.moved_placement(s, &sk)? } else { None };
        let m_new = placed.unwrap_or(sk.m);
        let mut moved =
            if others && self.projects_model(&sk) { self.reproject(s, &sk, &m_new)? } else { HashMap::new() };
        if placed.is_some() {
            // The origin's point and axes stay where they are in the model: seen from the moved
            // plane they move the other way.
            let (o0, x0, y0) = frame(&sk.m);
            let (o1, x1, y1) = frame(&m_new);
            for &(p, u, v, _) in &sk.points {
                if self.flags(p) & 0x40 != 0 && !self.model_projected(p) {
                    let w =
                        [o0[0] + u * x0[0] + v * y0[0], o0[1] + u * x0[1] + v * y0[1], o0[2] + u * x0[2] + v * y0[2]];
                    let q = sub3(w, o1);
                    moved.insert(p, (rnd(dot(q, x1)), rnd(dot(q, y1))));
                }
            }
        }
        let projected = |i: usize| self.flags(i) & 0x40 != 0;
        let Some(mut new) = crate::inventor_sketch::resolve(&self.dc, s, &sk, &moved, &projected)? else {
            if placed.is_none() {
                return Ok(sk);
            }
            // Moved with its face only: the same sketch in the new place.
            let mut new = sk;
            new.m = m_new;
            self.solved.borrow_mut().insert(s, new.clone());
            return Ok(new);
        };
        new.m = m_new;
        new.before = Some(Box::new(sk));
        self.solved.borrow_mut().insert(s, new.clone());
        Ok(new)
    }

    /// Points of the earlier sketches in model space (mm), as saved and as the row moves them:
    /// every point of a sketch an extrude sweeps (blind or symmetric) at the start, end and
    /// middle of the sweep, and every point of a sketch a revolve turns (with the revolve's
    /// axis, for its outline seen from a plane through the axis). The sweep's distance follows
    /// the row; the sketches are taken as solved for it.
    fn sources(&self, s: usize) -> Result<Vec<Source>, String> {
        let mut out = Vec::new();
        for f in self.dc.ordered_features() {
            let (Some(typ @ (FeatType::Extrude | FeatType::Revolve)), Ok(slots)) = (f.typ, self.slots(&f)) else {
                continue;
            };
            let Some(s2) = slots.get(1).and_then(|l| self.profile_sketch(l)) else { continue };
            if s2 >= s {
                continue;
            }
            let (old2, new2) = (self.saved_sketch(s2)?, self.sketch(s2)?);
            let at3 = |m: &[f64; 16], a: f64, b: f64| {
                let (o, x, y) = frame(m);
                [o[0] + a * x[0] + b * y[0], o[1] + a * x[1] + b * y[1], o[2] + a * x[2] + b * y[2]]
            };
            let (_, x2, y2) = frame(&old2.m);
            let n2 = unit3(cross(x2, y2));
            let mut sweeps: Vec<(f64, f64)> = vec![(0.0, 0.0)];
            let mut axis = None;
            if typ == FeatType::Extrude {
                let ext = slots.get(6).and_then(|i| tail_u16(&self.dc, i));
                if let Some(dp) = slots.get(4)
                    && let (Some((_, was)), Some((_, now))) = (self.dc.param_saved(dp), self.dc.param(dp))
                {
                    let ks: &[f64] = match ext {
                        Some(2) => &[0.5, -0.5],
                        Some(1) => &[1.0, -1.0],
                        _ => &[],
                    };
                    sweeps.extend(ks.iter().map(|k| (k * was * CM, k * now * CM)));
                }
            } else if let Some(ax) = slots.get(2)
                && let Ok((p0, v)) = work_axis(&self.dc, ax)
            {
                axis = Some(([p0[0] * CM, p0[1] * CM, p0[2] * CM], v));
            }
            for &(q, qa, qb, _) in &old2.points {
                let Some(&(_, na, nb, _)) = new2.points.iter().find(|w| w.0 == q) else { continue };
                let (b0, b1) = (at3(&old2.m, qa, qb), at3(&new2.m, na, nb));
                for &(t0, t1) in &sweeps {
                    let shift = |b: V3, t: f64| [b[0] + t * n2[0], b[1] + t * n2[1], b[2] + t * n2[2]];
                    out.push(Source { old: shift(b0, t0), new: shift(b1, t1), axis });
                }
            }
        }
        Ok(out)
    }

    /// Where the row moves the points a sketch projects from the model (mm, by record), seen
    /// from its plane `m_new` (note AP): each must be one of the [`Reader::sources`] seen from
    /// the plane (or, for a turned point and a plane through the revolve's axis, the outline of
    /// its circle). Points that several sources explain must agree.
    fn reproject(&self, s: usize, sk: &Sketch, m_new: &[f64; 16]) -> Result<HashMap<usize, P2>, String> {
        const TOL: f64 = 1e-5;
        let refuse = || format!("sketch '{}' projects model edges that are not read for a member's row", sk.name);
        if sk
            .curves
            .iter()
            .any(|c| matches!(*c, Curve::Circle { id, .. } | Curve::Ellipse { id, .. } if self.model_projected(id)))
        {
            return Err(refuse());
        }
        let sources = self.sources(s)?;
        let view = |m: &[f64; 16]| {
            let (o, x, y) = frame(m);
            move |p: V3| {
                let q = sub3(p, o);
                (dot(q, x), dot(q, y))
            }
        };
        let (to2, to2_new) = (view(&sk.m), view(m_new));
        let (_, x, y) = frame(&sk.m);
        let n = unit3(cross(x, y));
        // A turned point's outline: on the axis, its radius out across it in the plane.
        let outline = |p: V3, p0: V3, v: V3, sign: f64| {
            let a = dot(sub3(p, p0), v);
            let foot = [p0[0] + a * v[0], p0[1] + a * v[1], p0[2] + a * v[2]];
            let r = libm::sqrt(dot(sub3(p, foot), sub3(p, foot)));
            let w = unit3(cross(n, v));
            [foot[0] + sign * r * w[0], foot[1] + sign * r * w[1], foot[2] + sign * r * w[2]]
        };
        let mut out = HashMap::new();
        for &(p, u, v, _) in &sk.points {
            if !self.model_projected(p) {
                continue;
            }
            let near = |q: P2| libm::hypot(q.0 - u, q.1 - v) < TOL;
            let mut found: Vec<P2> = Vec::new();
            for src in &sources {
                if near(to2(src.old)) {
                    found.push(to2_new(src.new));
                }
                if let Some((p0, ax)) = src.axis
                    && dot(ax, n).abs() < 1e-9
                {
                    for sign in [1.0, -1.0] {
                        if near(to2(outline(src.old, p0, ax, sign))) {
                            found.push(to2_new(outline(src.new, p0, ax, sign)));
                        }
                    }
                }
            }
            let Some(&first) = found.first() else { return Err(refuse()) };
            if found.iter().any(|q| libm::hypot(q.0 - first.0, q.1 - first.1) > TOL) {
                return Err(format!(
                    "sketch '{}' projects a model edge that several earlier sketches explain",
                    sk.name
                ));
            }
            out.insert(p, (rnd(first.0), rnd(first.1)));
        }
        Ok(out)
    }

    /// The placement of a sketch on a face of the model (a `90874d23` record ties its placement
    /// to the face) when the row moves the face: its origin must be one of the
    /// [`Reader::sources`], and moves with it (the face is taken not to turn). `None`: the
    /// sketch is not on a face, or its origin is no such point.
    fn moved_placement(&self, s: usize, sk: &Sketch) -> Result<Option<[f64; 16]>, String> {
        const TOL: f64 = 1e-5;
        let on_face = self.dc.refs(s).into_iter().any(|(_, pl)| {
            self.dc.role(pl) == Role::SkPlacement
                && self.back().get(pl).is_some_and(|b| b.iter().any(|&j| self.dc.class(j) == 0x9087_4d23))
        });
        if !on_face {
            return Ok(None);
        }
        let (o, x, y) = frame(&sk.m);
        let sources = self.sources(s)?;
        let found: Vec<V3> = sources
            .iter()
            .filter(|src| libm::sqrt(dot(sub3(src.old, o), sub3(src.old, o))) < TOL)
            .map(|src| src.new)
            .collect();
        let Some(&first) = found.first() else {
            // No earlier point at its origin: the face moves as the points of earlier sketches
            // that lie in its plane move across it (they must all agree); the origin keeps its
            // place in the plane.
            let n = unit3(cross(x, y));
            let d = match self.plane_shift(s, o, n) {
                Ok(Some(d)) => d,
                Ok(None) => {
                    return Err(format!(
                        "sketch '{}' lies on a face the member's row may move (not read yet)",
                        sk.name
                    ));
                }
                Err(_) => {
                    return Err(format!("sketch '{}' lies on a face that several earlier sketches explain", sk.name));
                }
            };
            let mut m = sk.m;
            for (k, c) in [3usize, 7, 11].into_iter().zip([o[0] + d * n[0], o[1] + d * n[1], o[2] + d * n[2]]) {
                if let Some(e) = m.get_mut(k) {
                    *e = c / CM;
                }
            }
            return Ok(Some(m));
        };
        if found.iter().any(|q| libm::sqrt(dot(sub3(*q, first), sub3(*q, first))) > TOL) {
            return Err(format!("sketch '{}' lies on a face that several earlier sketches explain", sk.name));
        }
        let mut m = sk.m;
        for (k, c) in [3usize, 7, 11].into_iter().zip(first) {
            if let Some(e) = m.get_mut(k) {
                *e = c / CM;
            }
        }
        Ok(Some(m))
    }

    /// How far (mm, along unit `n`) the member's row moves a plane through `o`: as the points of
    /// the sketches before sketch `s` that lie in it move across it, which must all agree.
    /// `None` when no such point lies in the plane.
    fn plane_shift(&self, s: usize, o: V3, n: V3) -> Result<Option<f64>, String> {
        const TOL: f64 = 1e-5;
        let shifts: Vec<f64> = self
            .sources(s)?
            .iter()
            .filter(|src| dot(sub3(src.old, o), n).abs() < TOL)
            .map(|src| dot(sub3(src.new, src.old), n))
            .collect();
        let Some(&d) = shifts.first() else { return Ok(None) };
        if shifts.iter().any(|e| (e - d).abs() > TOL) {
            return Err("a plane that several earlier sketches explain".into());
        }
        Ok(Some(d))
    }

    /// A sketch entity projected from the model's edges (not the origin's point or axes: those
    /// carry `feb0d977` / `18951917` links, or the axis flags).
    fn model_projected(&self, i: usize) -> bool {
        let f = self.flags(i);
        f & 0x40 != 0
            && f & 0x30_0000 == 0
            && !self
                .back()
                .get(i)
                .is_some_and(|b| b.iter().any(|&j| matches!(self.dc.class(j), 0xfeb0_d977 | 0x1895_1917)))
    }

    /// Whether a sketch holds geometry projected from the model's edges (not the origin's point
    /// or axes: those carry `feb0d977` / `18951917` links, or the axis flags).
    fn projects_model(&self, sk: &Sketch) -> bool {
        let curve_ids = sk.curves.iter().map(|c| match *c {
            Curve::Line { id, .. }
            | Curve::Arc { id, .. }
            | Curve::Circle { id, .. }
            | Curve::Ellipse { id, .. }
            | Curve::Spline { id, .. } => id,
        });
        sk.points.iter().map(|p| p.0).chain(curve_ids).any(|i| self.model_projected(i))
    }

    fn saved_sketch(&self, s: usize) -> Result<Sketch, String> {
        let r = self.dc.refs(s);
        let placement = r.iter().map(|&(_, x)| x).find(|&x| self.dc.role(x) == Role::SkPlacement);
        let m = match placement {
            Some(p) => self.dc.placement(p).ok_or("a sketch placement is not readable")?,
            None => [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let mut sk = Sketch {
            name: self.dc.name(s).unwrap_or_else(|| format!("Sketch{s}")),
            m,
            points: Vec::new(),
            curves: Vec::new(),
            before: None,
        };
        let mut ids: HashMap<usize, ()> = HashMap::new();
        let items = self.owned(s);
        for &i in items {
            if self.dc.role(i) != Role::SkPoint {
                continue;
            }
            let rec = self.dc.r(i);
            let b = self.dc.body(i);
            let (Some(u), Some(v)) = (f64_at(rec, b + 8), f64_at(rec, b + 16)) else {
                return Err("a sketch point is cut short".into());
            };
            ids.insert(i, ());
            sk.points.push((i, rnd(u * CM), rnd(v * CM), self.flags(i) & 0x40 != 0));
        }
        for &i in items {
            let role = self.dc.role(i);
            if !matches!(role, Role::SkLine | Role::SkCircle | Role::SkEllipse | Role::SkSpline) {
                continue;
            }
            if sk.curves.len() >= MAX_CURVES {
                return Err("the sketch has too many curves".into());
            }
            let con = self.flags(i) & 0x80000 != 0;
            let list = self.dc.first_list(i, None).map(|x| x.0).unwrap_or_default();
            let known = |t: &Option<usize>| t.is_some_and(|t| ids.contains_key(&t));
            if role == Role::SkLine {
                if let (Some(Some(a)), Some(Some(b)), true) =
                    (list.first(), list.get(1), list.iter().take(2).all(known))
                {
                    sk.curves.push(Curve::Line { id: i, a: *a, b: *b, con });
                }
                continue;
            }
            if role == Role::SkEllipse {
                if let Some(c) = self.ellipse(i, &sk.points, con) {
                    sk.curves.push(c);
                }
                continue;
            }
            if role == Role::SkSpline {
                if let Some(c) = self.spline(i, &list, &sk.points, con) {
                    sk.curves.push(c);
                }
                continue;
            }
            let rec = self.dc.r(i);
            let (Some(rad), Some(cen)) = (
                rec.len().checked_sub(9).and_then(|k| f64_at(rec, k)),
                rec.len()
                    .checked_sub(13)
                    .and_then(|k| u32_at(rec, k))
                    .and_then(|v| super::inventor_dc::reference(v, self.dc.n())),
            ) else {
                continue;
            };
            if !ids.contains_key(&cen) {
                continue;
            }
            if let (2, Some(Some(s0)), Some(Some(e0)), true) =
                (list.len(), list.first(), list.get(1), list.iter().all(known))
            {
                sk.curves.push(Curve::Arc { id: i, c: cen, s: *s0, e: *e0, con });
            } else {
                sk.curves.push(Curve::Circle { id: i, c: cen, r: rnd(rad * CM), con });
            }
        }
        Ok(sk)
    }

    /// A B-spline record (`317b7346` or `f9372fd4`, note AR): its first list names the curve's
    /// points (the ends, at the first and last poles, and points on it); further on, `u32 degree, f64 tolerance`, then three arrays, each
    /// `u32 n, u32 n, u32 8` and its doubles: the knots, the weights (none for a polynomial
    /// curve) and, as `u32 n` and two more words, the poles (x, y in cm). The first and last
    /// poles must be the end points, the knots non-decreasing and `poles + degree + 1` long.
    /// Anything else (a periodic or unclamped curve, another layout) is not read.
    fn spline(&self, i: usize, list: &[Option<usize>], points: &[(usize, f64, f64, bool)], con: bool) -> Option<Curve> {
        const MAX_KNOTS: usize = 4096;
        let rec = self.dc.r(i);
        let at = |p: usize| points.iter().find(|q| q.0 == p).map(|q| (q.1, q.2));
        let dbls = |o: usize, n: usize| -> Option<Vec<f64>> {
            (0..n).map(|k| f64_at(rec, o + 8 * k).filter(|v| v.is_finite() && v.abs() < 1e7)).collect()
        };
        // The first offset where the whole layout reads (an earlier match of the degree and
        // tolerance words can be part of something else).
        (self.dc.body(i)..rec.len().saturating_sub(24)).find_map(|j| -> Option<Curve> {
            let degree = u32_at(rec, j).filter(|d| (1..=9).contains(d))?;
            if !f64_at(rec, j + 4).is_some_and(|t| t > 0.0 && t < 1e-3) {
                return None;
            }
            let (Some(nk), Some(nk2), Some(8)) = (u32_at(rec, j + 12), u32_at(rec, j + 16), u32_at(rec, j + 20)) else {
                return None;
            };
            let nk = nk as usize;
            // The second word is the array's capacity (32 for 22 knots on a closed curve).
            if nk > nk2 as usize || nk > MAX_KNOTS {
                return None;
            }
            let knots = dbls(j + 24, nk)?;
            let o = j + 24 + 8 * nk;
            let (nw, nw2, es) = (u32_at(rec, o)? as usize, u32_at(rec, o + 4)? as usize, u32_at(rec, o + 8)?);
            if nw > nw2 || es != 8 || nw > MAX_KNOTS {
                return None;
            }
            let weights = dbls(o + 12, nw)?;
            let o = o + 12 + 8 * nw;
            let np = u32_at(rec, o)? as usize;
            if np < 2 || nk != np + degree as usize + 1 || !(nw == 0 || nw == np) {
                return None;
            }
            let flat = dbls(o + 12, 2 * np)?;
            let poles: Vec<P2> = flat
                .chunks_exact(2)
                .filter_map(|c| match *c {
                    [x, y] => Some((rnd(x * CM), rnd(y * CM))),
                    _ => None,
                })
                .collect();
            let near = |a: P2, b: P2| libm::hypot(a.0 - b.0, a.1 - b.1) < 1e-4;
            // The ends are the listed points at the first and last poles: the list can name
            // points on the curve too, where other curves end (`f9372fd4`, the coin outlines).
            let end = |q: Option<&P2>| -> Option<usize> {
                let q = *q?;
                let on = |p: &&Option<usize>| p.and_then(at).is_some_and(|a| near(a, q));
                [list.first(), list.last()]
                    .into_iter()
                    .flatten()
                    .find(on)
                    .or_else(|| list.iter().find(on))
                    .copied()
                    .flatten()
            };
            let (s, e) = (end(poles.first())?, end(poles.last())?);
            // A closed curve lists its one end point (its first and last poles meet there).
            let ok = knots.windows(2).all(|w| matches!(*w, [a, b] if a <= b))
                && weights.iter().all(|&w| w > 0.0)
                && (s != e || (np > 2 && list.len() == 1));
            ok.then_some(Curve::Spline { id: i, s, e, degree, knots, poles, weights, con })
        })
    }

    /// An ellipse record: the tail holds `f64 s, a, b, u8` (cm; `s` = y component of the major
    /// axis direction, `a` >= `b`), and it refers to its centre and axis-end points. The centre
    /// is the referred point from which the others lie `a` / `b` away; the x sign of the
    /// direction comes from the axis-end point when there is one, else it is taken as positive.
    fn ellipse(&self, i: usize, points: &[(usize, f64, f64, bool)], con: bool) -> Option<Curve> {
        let rec = self.dc.r(i);
        let k = rec.len().checked_sub(25)?;
        let (s, a, b) = (f64_at(rec, k)?, f64_at(rec, k + 8)? * CM, f64_at(rec, k + 16)? * CM);
        if !(a > 0.0 && b > 0.0 && a >= b && s.abs() <= 1.0 + 1e-9) {
            return None;
        }
        let at = |id: usize| points.iter().find(|p| p.0 == id).map(|p| (p.1, p.2));
        let refd: Vec<(usize, (f64, f64))> = self
            .dc
            .refs(i)
            .into_iter()
            .filter(|&(_, x)| self.dc.role(x) == Role::SkPoint)
            .filter_map(|(_, x)| Some((x, at(x)?)))
            .collect();
        let near = |p: (f64, f64), q: (f64, f64), d: f64| (libm::hypot(p.0 - q.0, p.1 - q.1) - d).abs() < 1e-4;
        let centre = if let [only] = refd.as_slice() {
            *only
        } else {
            *refd
                .iter()
                .find(|&&(_, c)| refd.iter().all(|&(_, q)| near(q, c, 0.0) || near(q, c, a) || near(q, c, b)))?
        };
        let sy = s.clamp(-1.0, 1.0);
        let cx = libm::sqrt((1.0 - sy * sy).max(0.0));
        // The major-axis end, when it is among the points, fixes the x sign.
        let sx = refd
            .iter()
            .find(|&&(_, q)| near(q, centre.1, a) && ((q.1 - centre.1.1) / a - sy).abs() < 1e-4)
            .map_or(cx, |&(_, q)| if cx > 1e-9 { cx * ((q.0 - centre.1.0).signum()) } else { cx });
        Some(Curve::Ellipse { id: i, c: centre.0, major: [rnd(sx), rnd(sy)], a: rnd(a), b: rnd(b), con })
    }

    /// The sketch whose profile a loops list belongs to.
    fn profile_sketch(&self, lst: usize) -> Option<usize> {
        for &j in self.back().get(lst)? {
            if self.dc.class(j) == super::inventor_dc::G_PROFILE {
                for (_, x) in self.dc.refs(j) {
                    if self.dc.role(x) == Role::Sketch {
                        return Some(x);
                    }
                }
            }
        }
        None
    }

    /// The history-body tags of the loops in a loops list.
    fn loop_tags(&self, lst: usize) -> Option<Vec<i64>> {
        let (items, _) = self.dc.first_list(lst, None)?;
        let mut out = Vec::new();
        for it in items {
            out.push(self.loop_tag(it?)?);
        }
        Some(out)
    }

    /// The history-body tag of one loop record (`a477243b`).
    fn loop_tag(&self, x: usize) -> Option<i64> {
        (self.dc.class(x) == super::inventor_dc::G_LOOP).then_some(())?;
        let refs = self.dc.refs(x);
        let c = refs.iter().map(|&(_, y)| y).rfind(|&y| self.dc.class(y) == super::inventor_dc::G_LOOP_BODY)?;
        let hb = self.dc.refs(c).into_iter().find(|&(_, y)| self.dc.role(y) == Role::HistoryBlob)?.0;
        Some(i64::from(u32_at(self.dc.r(c), hb + 4)?))
    }

    /// The sketch of a sweep's path loop: the one its `424eb7d7` record names.
    fn path_sketch(&self, lp: usize) -> Option<usize> {
        for &j in self.back().get(lp)? {
            if self.dc.class(j) == super::inventor_dc::G_PATH {
                for (_, x) in self.dc.refs(j) {
                    if self.dc.role(x) == Role::Sketch {
                        return Some(x);
                    }
                }
            }
        }
        None
    }

    /// `"all"`, or one point per region the feature's loops surround.
    fn profiles(&self, sk: &Sketch, lst: usize) -> Value {
        let Some(before) = &sk.before else { return self.saved_profiles(sk, lst) };
        // Picked on the saved sketch (the loops show it), then found again on the solved one.
        match self.saved_profiles(before, lst) {
            Value::Array(picks) => {
                let mut out = Vec::new();
                for p in picks {
                    let at = p
                        .get("point")
                        .and_then(Value::as_array)
                        .and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
                    let Some(q) = at.and_then(|q| crate::inventor_sketch::map_pick(before, sk, q)) else {
                        return json!("all");
                    };
                    out.push(json!({"point": [rnd(q.0), rnd(q.1)]}));
                }
                Value::Array(out)
            }
            other => other,
        }
    }

    fn saved_profiles(&self, sk: &Sketch, lst: usize) -> Value {
        let (Some(ac), Some(tags)) = (self.history(), self.loop_tags(lst)) else { return json!("all") };
        if tags.is_empty() {
            return json!("all");
        }
        let (o, x, y) = ([sk.m[3], sk.m[7], sk.m[11]], [sk.m[0], sk.m[4], sk.m[8]], [sk.m[1], sk.m[5], sk.m[9]]);
        // The loops of each region body (one body per region the feature took).
        let mut l2: Vec<Vec<Vec<P2>>> = Vec::new();
        for t in tags {
            let Some(ls) = ac.loops_of_body(t) else { return json!("all") };
            l2.push(
                ls.into_iter()
                    .map(|l| {
                        l.into_iter()
                            .map(|p| {
                                let q = sub3(p, o);
                                (dot(q, x) * CM, dot(q, y) * CM)
                            })
                            .collect()
                    })
                    .collect(),
            );
        }
        if l2.iter().all(Vec::is_empty) {
            return json!("all");
        }

        match picks(&sk.polys(), &l2) {
            Some(pk) if !pk.is_empty() => Value::Array(pk.into_iter().map(|(a, b)| json!({"point": [a, b]})).collect()),
            _ => json!("all"),
        }
    }

    /// How far an extrude that ends on a face (extent 4, to the next face, or 7, to a chosen
    /// face) reached, in mm along its sketch's normal: the face is named through the naming
    /// records, so the reach is measured instead on the faces the feature made in the result
    /// body (side faces and caps carry its id). All their corners must lie on one side of the
    /// sketch plane; the farthest is the reach. A later feature that trimmed every face back,
    /// or an end face that is not parallel to the sketch, gives a reach the rebuild check
    /// refuses.
    fn swept_distance(&self, sk: &Sketch, fid: i64) -> Result<f64, String> {
        let b = self.result().ok_or("the result body is not readable")?;
        let faces = b.sweep_faces(fid);
        if faces.is_empty() {
            return Err("no face of the feature is in the result body".into());
        }
        let (o, x, y) = frame(&sk.m);
        let n = unit3(cross(x, y));
        let (mut lo, mut hi) = (0.0f64, 0.0f64);
        for f in faces {
            for v in b.verts(f) {
                let t = dot(sub3([v[0] * CM, v[1] * CM, v[2] * CM], o), n);
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
        match (lo < -1e-6, hi > 1e-6) {
            (true, true) => Err("the feature's faces lie on both sides of its sketch".into()),
            (false, false) => Err("the feature's faces lie in its sketch plane".into()),
            (true, false) => Ok(lo),
            (false, true) => Ok(hi),
        }
    }

    /// The face an extrude that ends on a face ran up to, as a face pick (mm): points where its
    /// faces meet that face in the result body, and the face's surface. `None` when the result
    /// does not show one such face of a plane, cylinder or cone.
    fn sweep_end_pick(&self, sk: &Sketch, fid: i64) -> Option<Value> {
        const MAX_POINTS: usize = 64;
        let b = self.result()?;
        let (o, x, y) = frame(&sk.m);
        let n = unit3(cross(x, y));
        let end = b.sweep_end(fid, [o[0] / CM, o[1] / CM, o[2] / CM], n)?;
        let mm = |v: V3| json!([rnd(v[0] * CM), rnd(v[1] * CM), rnd(v[2] * CM)]);
        let dir = |v: V3| json!([rnd(v[0]), rnd(v[1]), rnd(v[2])]);
        let surface = match end.surface {
            EndSurface::Plane { p, n } => json!({"type": "plane", "origin": mm(p), "normal": dir(n)}),
            EndSurface::Cyl { c, ax, r } => {
                json!({"type": "cylinder", "origin": mm(c), "axis": dir(ax), "radius": rnd(r * CM)})
            }
            EndSurface::Cone { c, ax, r, sin, cos } => {
                json!({"type": "cone", "origin": mm(c), "axis": dir(ax), "radius": rnd(r * CM), "sin": rnd(sin), "cos": rnd(cos)})
            }
        };
        let step = end.points.len().div_ceil(MAX_POINTS).max(1);
        let points: Vec<Value> = end.points.iter().step_by(step).map(|&p| mm(p)).collect();
        Some(json!({"point": points.first()?.clone(), "points": points, "surface": surface}))
    }

    fn slots(&self, f: &Feature) -> Result<Slots, String> {
        self.dc.slots(f.rec).map(Slots).ok_or_else(|| "the feature has no slot list".into())
    }

    fn param(&self, s: &Slots, j: usize) -> Result<f64, String> {
        let i = s.need(j)?;
        self.dc.param(i).map(|p| p.1).ok_or_else(|| format!("slot {j} is not a parameter"))
    }

    fn pval(&self, s: &Slots, j: usize) -> Result<f64, String> {
        self.dc.pval(&s.0, j).ok_or_else(|| format!("slot {j} is not a parameter"))
    }
}

/// Reads (for the row) the offsets of the offset work planes (`0d28d8c0`: the plane, its base
/// plane, the offset) that nothing but their names (and the document record) refer to: they
/// place nothing (note AP).
fn unused_planes(r: &Reader) {
    for i in 0..r.dc.n() {
        if r.dc.class(i) != 0x0d28_d8c0 {
            continue;
        }
        // The plane is the first work plane it names (the base plane follows).
        let refs: Vec<usize> = r.dc.refs(i).into_iter().map(|x| x.1).collect();
        let (Some(&plane), Some(&p)) = (
            refs.iter().find(|&&x| matches!(r.dc.class(x), 0xce52_df42)),
            refs.iter().find(|&&x| r.dc.role(x) == Role::Param),
        ) else {
            continue;
        };
        let idle = r
            .back()
            .get(plane)
            .is_some_and(|b| b.iter().all(|&j| j == i || r.dc.role(j) == Role::Name || r.dc.class(j) == 0x9087_4d63));
        if idle {
            let _ = r.dc.param(p);
        }
    }
}

/// A point of an earlier sketch in model space (mm), as saved and as the member's row moves it,
/// with the axis (point, unit direction) of the revolve that turns it.
struct Source {
    old: V3,
    new: V3,
    axis: Option<(V3, V3)>,
}

/// What a hole's plain-cut form is made from (kept to remake it when a pattern moves the hole).
struct HoleInfo {
    nd: V3,
    diameter: f64,
    through: bool,
    depth: f64,
    point_angle: Option<f64>,
    cbore: Option<(f64, f64)>,
    /// Countersink diameter (mm) and angle (degrees).
    csink: Option<(f64, f64)>,
}

/// Everything decoded so far.
struct State {
    out: Vec<Value>,
    done: HashMap<usize, ()>,
    first_body: Option<String>,
    omitted: Vec<Value>,
    /// Feature record -> its name.
    names: HashMap<usize, String>,
    holes: HashMap<String, HoleInfo>,
    /// Feature name -> the names of the tree features it became (none when it was left out;
    /// a fillet with several radii is several), for the patterns and mirrors that copy it.
    emitted: HashMap<String, Vec<String>>,
    /// Solid body record -> the tree body made for it.
    bodies: HashMap<usize, String>,
    /// The flat plates of the sheet-metal faces: a point and unit normal of the sketch plane
    /// and the span of the material along the normal (mm), for the folds.
    sheets: Vec<(V3, V3, f64, f64)>,
}

fn solid(r: &Reader, st: &mut State, f: &Feature, name: &str, typ: FeatType) -> Result<(), String> {
    let s = r.slots(f)?;
    let lst = s.need(1)?;
    let sk = r.profile_sketch(lst).ok_or("no sketch found")?;
    let sketch = r.sketch(sk)?;
    let mut op = match s.get(0).and_then(|i| tail_u16(&r.dc, i)) {
        Some(2) => "cut",
        Some(1) => "new",
        Some(4) => "intersect",
        _ => "join",
    };
    // A new solid makes a body of its own when the feature names a solid body no feature made
    // yet (notes AK, AR); the bodies a cut or join lists are those it changes.
    let own = body_record(r, f);
    let new_body = op == "new" && st.first_body.is_some() && own.is_some_and(|b| !st.bodies.contains_key(&b));
    if st.first_body.is_some() && op == "new" && !new_body {
        op = "join";
    }
    if st.first_body.is_none() && op == "join" {
        op = "new";
    }
    let mut j = json!({"type": if typ == FeatType::Extrude {"extrude"} else {"revolve"}, "name": name,
                       "sketch": sketch.name, "profiles": r.profiles(&sketch, lst), "operation": op});
    if op != "new" {
        put(&mut j, "target", changed_bodies(r, st, f, op == "cut")?);
    } else if let Some(b) = own {
        st.bodies.insert(b, name.to_string());
    }
    let ext = |i: Option<usize>| i.and_then(|i| tail_u16(&r.dc, i));
    let mut axis_line = None;
    // A copy of the sketch the extrude starts from instead (extent 12).
    let mut start = None;
    // A revolve whose sketch is turned about its axis (point, unit direction, radians) first.
    let mut turned: Option<(V3, V3, f64)> = None;
    if typ == FeatType::Extrude {
        // Slot 2 is the sweep direction: the sketch normal, or (three extrudes of one part, note
        // AR) the other way; slot 3 turns it round.
        let (_, sx, sy) = frame(&sketch.m);
        let away = s.get(2).and_then(|i| r.dc.skdir(i)).is_some_and(|d| dot([d[2], d[3], d[4]], cross(sx, sy)) < -0.5);
        let rev = (s.get(3).and_then(|i| r.dc.r(i).last().copied()) == Some(1)) != away;
        let both = s.get(7).and_then(|i| r.dc.r(i).last().copied()) == Some(1);
        let dist = r.param(&s, 4)? * CM;
        // Slot 5: the taper (radians; positive widens the solid away from its sketch, note AS).
        let taper = s.get(5).filter(|&i| r.dc.role(i) == Role::Param).and_then(|i| r.dc.param(i)).map_or(0.0, |p| p.1);
        let taper = if taper.abs() > 1e-12 { taper.to_degrees() } else { 0.0 };
        if taper != 0.0 {
            put(&mut j, "taper_deg", json!(rnd(taper)));
        }
        match ext(s.get(6)) {
            Some(2) => {
                put(&mut j, "distance", json!(rnd(dist)));
                put(&mut j, "symmetric", json!(true));
            }
            Some(1) => {
                let d1 = if rev { -dist } else { dist };
                put(&mut j, "distance", json!(rnd(d1)));
                put(&mut j, "symmetric", json!(false));
                // Slots 27 to 29: a second direction (`ex_two_dir`): its distance, taper
                // (radians) and extent (1: a distance), on the other side of the sketch.
                let second = |k: usize| s.get(k).filter(|&i| r.dc.role(i) == Role::Param).and_then(|i| r.dc.param(i));
                if ext(s.get(29)) == Some(1)
                    && let Some((_, d2)) = second(27)
                    && d2 > 0.0
                {
                    put(&mut j, "distance2", json!(rnd(-d1.signum() * d2 * CM)));
                    let t2 = second(28).map_or(0.0, |p| p.1);
                    if t2.abs() > 1e-12 {
                        put(&mut j, "taper2_deg", json!(rnd(t2.to_degrees())));
                    }
                }
            }
            // Through all; with slot 7 set, through all both ways (notes AA).
            Some(5) if both => {
                put(&mut j, "distance", json!(2.0 * THROUGH));
                put(&mut j, "symmetric", json!(true));
                put(&mut j, "through_all", json!(true));
            }
            Some(5) => {
                put(&mut j, "distance", json!(rnd(if rev { -THROUGH } else { THROUGH })));
                put(&mut j, "symmetric", json!(false));
                put(&mut j, "through_all", json!(true));
            }
            Some(e @ (4 | 7)) => {
                let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
                let measured = match r.swept_distance(&sketch, fid) {
                    Ok(d) => Some(d),
                    // No face of it is left in the result: the face it ran to is kept as a
                    // plane (slot 18, note AS); parallel to the sketch, the reach is its distance.
                    // To the next face (extent 4): the next face as the model is built here.
                    Err(_) if e == 4 && taper == 0.0 && matches!(op, "join" | "cut") => {
                        put(&mut j, "end_condition", json!("to_next"));
                        put(&mut j, "reverse", json!(rev));
                        None
                    }
                    Err(w) => {
                        let d = (e == 7)
                            .then(|| end_plane_reach(r, &s, 18, &sketch))
                            .flatten()
                            .ok_or_else(|| format!("extrude extent {e}: {w}"))?;
                        put(&mut j, "distance", json!(rnd(d)));
                        None
                    }
                };
                put(&mut j, "symmetric", json!(false));
                // Up to the face it ran to, as the result shows it (notes AB); the measured
                // reach is the blind form the caller falls back to.
                match measured.map(|d| (d, (taper == 0.0).then(|| r.sweep_end_pick(&sketch, fid)).flatten())) {
                    Some((d, Some(face))) => {
                        put(&mut j, "end_condition", json!("to_face"));
                        put(&mut j, "face", face);
                        put(&mut j, "reverse", json!(d < 0.0));
                        // The face's surface extended (ADR 0061): where the face does not cover
                        // the profile, Inventor's result ran to its extension.
                        put(&mut j, "extend", json!(true));
                        put(&mut j, "_blind", json!(rnd(d)));
                    }
                    // A tapered extrude is blind (Gusset tapers no extrude that ends on a face).
                    // Its faces' corners may stop short of where its walls meet a curved end
                    // face (a cross recess filled up to its cone): a join runs on into the
                    // material behind that face.
                    Some((d, None)) if taper != 0.0 && op == "join" => put(&mut j, "distance", json!(rnd(d * 1.1))),
                    Some((d, None)) => put(&mut j, "distance", json!(rnd(d))),
                    None => {}
                }
            }
            // From a face (extent 12): the distance runs from the face slot 17 keeps, parallel to
            // the sketch: the sketch moved onto that face (note AS).
            Some(12) => {
                let off = end_plane_reach(r, &s, 17, &sketch).ok_or("extrude extent 12: the start face is not read")?;
                let (o, x, y) = frame(&sketch.m);
                let n = unit3(cross(x, y));
                let mut moved = sketch.json();
                let name2 = format!("{}_from_{name}", sketch.name);
                put(&mut moved, "name", json!(name2));
                if let Some(pl) = moved.get_mut("plane") {
                    put(pl, "origin", json!([rnd(o[0] + off * n[0]), rnd(o[1] + off * n[1]), rnd(o[2] + off * n[2])]));
                }
                start = Some(moved);
                put(&mut j, "sketch", json!(name2));
                put(&mut j, "distance", json!(rnd(if rev { -dist } else { dist })));
                put(&mut j, "symmetric", json!(false));
            }
            e => return Err(format!("extrude extent {e:?}")),
        }
    } else {
        let (p0, v) =
            work_axis(&r.dc, s.need(2)?).map_err(|w| format!("the revolve {}", w.trim_start_matches("the ")))?;
        put(
            &mut j,
            "axis",
            json!({"origin": [rnd(p0[0] * CM), rnd(p0[1] * CM), rnd(p0[2] * CM)],
                           "direction": [rnd(v[0]), rnd(v[1]), rnd(v[2])]}),
        );
        axis_line = sketch_axis_line(&sketch, [p0[0] * CM, p0[1] * CM, p0[2] * CM], v, f.rec);
        let angle = match ext(s.get(3)) {
            Some(3) => 360.0,
            _ => r.param(&s, 4)?.to_degrees(),
        };
        put(&mut j, "angle", json!(rnd(angle)));
        // Slot 5 (`c7a06ac2`) 2: the angle runs half each way from the sketch (note AS). The
        // revolve turns a copy of its sketch turned back by half the angle about the axis.
        let both = s.get(5).filter(|&i| r.dc.class(i) == 0xc7a0_6ac2).and_then(|i| tail_u16(&r.dc, i)) == Some(2);
        if both && angle < 360.0 {
            turned = Some(([p0[0] * CM, p0[1] * CM, p0[2] * CM], unit3(v), -angle.to_radians() / 2.0));
        }
    }
    if !st.done.contains_key(&sk) {
        st.out.push(sketch.json());
        st.done.insert(sk, ());
    }
    if let Some(moved) = start {
        st.out.push(moved);
    }
    // An axis in the sketch plane off its origin or axes: a construction line of the sketch
    // along it (from the axis point, in its direction) is the axis.
    if let Some((pts, line, id)) = axis_line {
        let entry = st.out.iter_mut().rev().find(|o| {
            o.get("type").and_then(Value::as_str) == Some("sketch")
                && o.get("name").and_then(Value::as_str) == Some(&sketch.name)
        });
        if let Some(Value::Object(o)) = entry {
            if let Some(Value::Array(p)) = o.get_mut("points") {
                p.extend(pts);
            }
            if let Some(Value::Array(c)) = o.get_mut("curves") {
                c.push(line);
            }
            put(&mut j, "axis", json!({"line": id}));
        }
    }
    if let Some((p0, v, t)) = turned {
        let entry = st.out.iter().rev().find(|o| {
            o.get("type").and_then(Value::as_str) == Some("sketch")
                && o.get("name").and_then(Value::as_str) == Some(&sketch.name)
        });
        let mut copy = entry.cloned().ok_or("the revolve's sketch is not written")?;
        let name2 = format!("{}_turned_{name}", sketch.name);
        put(&mut copy, "name", json!(name2));
        let (o, x, y) = frame(&sketch.m);
        let o2 = add3(p0, rotate(sub3(o, p0), v, t));
        if let Some(pl) = copy.get_mut("plane") {
            put(pl, "origin", json!([rnd(o2[0]), rnd(o2[1]), rnd(o2[2])]));
            let (x2, y2) = (rotate(x, v, t), rotate(y, v, t));
            put(pl, "x_axis", json!([rnd(x2[0]), rnd(x2[1]), rnd(x2[2])]));
            put(pl, "y_axis", json!([rnd(y2[0]), rnd(y2[1]), rnd(y2[2])]));
        }
        st.out.push(copy);
        put(&mut j, "sketch", json!(name2));
    }
    st.out.push(j);
    if st.first_body.is_none() {
        st.first_body = Some(name.to_string());
    }
    Ok(())
}

fn hole(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let items = match s.get(7) {
        Some(c) => r.dc.first_list(c, None).map(|x| x.0).ok_or("the hole has no centre list")?,
        // Placed on something else than sketch points (concentric to an edge, on a point):
        // the centres come from the hole's faces in the result body (notes AF).
        None => Vec::new(),
    };
    let mut m = r.dc.placement(s.need(8)?).ok_or("the hole placement is not readable")?;
    // For an iPart member's row: the centres' sketch as the row solves and places it; when it
    // is not solved again (its centres are projected from the model), as its face moves.
    let mut solved: HashMap<usize, (f64, f64)> = HashMap::new();
    let owner = items
        .iter()
        .flatten()
        .find(|&&it| r.dc.role(it) == Role::SkPoint)
        .and_then(|&it| r.back().get(it)?.iter().copied().find(|&j| r.dc.role(j) == Role::Sketch));
    if r.dc.has_row()
        && let Some(owner) = owner
    {
        match r.sketch(owner) {
            Ok(sk) => {
                m = sk.m;
                solved = sk.points.iter().map(|&(i, u, v, _)| (i, (u / CM, v / CM))).collect();
            }
            Err(_) => {
                if let Ok(Some(moved)) = r.saved_sketch(owner).and_then(|sk| r.moved_placement(owner, &sk)) {
                    m = moved;
                }
            }
        }
    }
    let (o, x, y) = frame(&m);
    let mut pos: Vec<V3> = Vec::new();
    for it in items.into_iter().flatten() {
        if r.dc.role(it) != Role::SkPoint {
            continue;
        }
        let rec = r.dc.r(it);
        let b = r.dc.body(it);
        let (Some(u), Some(v)) =
            solved.get(&it).map_or_else(|| (f64_at(rec, b + 8), f64_at(rec, b + 16)), |&(u, v)| (Some(u), Some(v)))
        else {
            return Err("a hole centre is cut short".into());
        };
        // Some projected centres (flags 0x40) carry one more byte before their coordinates,
        // not decoded: what is read there is no position.
        if !(u.abs() < 1e5 && v.abs() < 1e5) {
            return Err("a hole centre is not a usable number".into());
        }
        let at = |o: f64, x: f64, y: f64| rnd(o + (u * x + v * y) * CM);
        pos.push([at(o[0], x[0], y[0]), at(o[1], x[1], y[1]), at(o[2], x[2], y[2])]);
    }
    let n = cross(x, y);
    let nd: V3 = match s.get(16) {
        Some(d) => {
            let a = r.dc.skdir(d).ok_or("the hole direction is not readable")?;
            [a[2], a[3], a[4]]
        }
        None => n,
    };
    // Slot 17 set: the hole goes the other way (into the face whose outward normal is the
    // opposite; notes AF).
    let nd = if s.get(17).is_some_and(|i| r.dc.r(i).last() == Some(&1)) { [-nd[0], -nd[1], -nd[2]] } else { nd };
    if s.get(7).is_none() {
        let b = r.result().ok_or("slot 7 is empty and the result body is not readable")?;
        let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
        let u = unit3(nd);
        pos = b.hole_centres(fid, u).into_iter().map(|p| [rnd(p[0] * CM), rnd(p[1] * CM), rnd(p[2] * CM)]).collect();
        if pos.is_empty() {
            return Err("slot 7 is empty and no face of the hole is in the result body".into());
        }
    }
    let t0 = s.need(0)?;
    let typ = u32_at(r.dc.r(t0), r.dc.body(t0)).ok_or("the hole type is not readable")?;
    let diameter = rnd(r.pval(&s, 1)? * CM);
    let mut j = json!({"type": "hole", "name": name,
        "face": {"point": arr(pos.first().copied().unwrap_or(o)), "normal": arr(nd)},
        "positions": pos.iter().map(|p| arr(*p)).collect::<Vec<_>>(), "diameter": diameter});
    let mut cb = None;
    let mut cs = None;
    if typ >> 16 == 1 {
        // Countersink: slot 3 its diameter, slot 5 its angle.
        let (d, a) = (rnd(r.pval(&s, 3)? * CM), rnd_to(r.pval(&s, 5)?.to_degrees(), 1e6));
        if !(d > diameter && a > 0.0 && a < 180.0) {
            return Err("the countersink is not wider than the hole, or its angle is not in 0..180".into());
        }
        put(&mut j, "variant", json!("countersink"));
        put(&mut j, "csink_diameter", json!(d));
        put(&mut j, "csink_angle", json!(a));
        cs = Some((d, a));
    } else if typ & 0x20000 != 0 {
        let (d, h) = (rnd(r.pval(&s, 3)? * CM), rnd(r.pval(&s, 4)? * CM));
        put(&mut j, "variant", json!("counterbore"));
        put(&mut j, "cbore_diameter", json!(d));
        put(&mut j, "cbore_depth", json!(h));
        cb = Some((d, h));
    }
    let extent = s.get(9).and_then(|i| tail_u16(&r.dc, i));
    let through = extent == Some(5);
    let mut depth = 0.0;
    let mut point_angle = None;
    if through {
        put(&mut j, "through", json!(true));
    } else {
        depth = if extent == Some(7) { to_depth(r, &s, f, &pos, nd, owner)? } else { rnd(r.pval(&s, 2)? * CM) };
        put(&mut j, "depth", json!(depth));
        if let Ok(pa) = r.pval(&s, 6)
            && pa != 0.0
        {
            let a = rnd_to(pa.to_degrees(), 1e6);
            put(&mut j, "point_angle", json!(a));
            point_angle = Some(a);
        }
    }
    let info = HoleInfo { nd, diameter, through, depth, point_angle, cbore: cb, csink: cs };
    if let Some(target) = &st.first_body
        && let Some(fb) = hole_fallback(name, &pos, &info, target)
    {
        put(&mut j, "_fallback", Value::Array(fb));
    }
    st.holes.insert(name.to_string(), info);
    st.out.push(j);
    Ok(())
}

/// The same hole as plain cuts (exact geometry, less editable): a through hole as a circle
/// sketch cut through everything, a blind one as one revolve cut per position (drill point
/// and counterbore included).
/// The depth (mm) of a hole that ends at a plane or face (extent 7, note AR): slot 13 a work
/// plane (the depth to it along the hole, the same from every centre), else the face named
/// through the naming records, measured on the hole's faces in the result body.
fn to_depth(r: &Reader, s: &Slots, f: &Feature, pos: &[V3], nd: V3, owner: Option<usize>) -> Result<f64, String> {
    const G_PLANE: u32 = 0xce52_df42;
    let u = unit3(nd);
    let first = *pos.first().ok_or("the hole has no centre")?;
    if let Some(pl) = s.get(13).filter(|&pl| r.dc.class(pl) == G_PLANE) {
        let (o, n) = work_plane(&r.dc, pl).map_err(|w| format!("the hole's end {w}"))?;
        let mut o = [o[0] * CM, o[1] * CM, o[2] * CM];
        // An iPart member's row moves the plane with the faces of earlier sketches in it.
        if r.dc.has_row()
            && let Some(owner) = owner
            && let Some(d) = r.plane_shift(owner, o, unit3(n))?
        {
            let n = unit3(n);
            o = [o[0] + d * n[0], o[1] + d * n[1], o[2] + d * n[2]];
        }
        let along = dot(u, n);
        if along.abs() < 1e-9 {
            return Err("the hole's end plane lies along the hole".into());
        }
        let depth = |p: V3| dot(sub3(p, o), n) / along;
        let d = depth(first);
        if d.is_nan() || d <= 0.0 || pos.iter().any(|&p| (depth(p) - d).abs() > 1e-6) {
            return Err("the hole's end plane is not at one depth below its face".into());
        }
        return Ok(rnd(d));
    }
    let b = r.result().ok_or("the hole ends at a face and the result body is not readable")?;
    let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
    let d = b
        .hole_reach(fid, [first[0] / CM, first[1] / CM, first[2] / CM], u)
        .ok_or("the hole ends at a face and none of its faces is in the result body")?;
    if d.is_nan() || d <= 0.0 {
        return Err("the hole's faces do not reach below its face".into());
    }
    Ok(rnd(d * CM))
}

fn hole_fallback(name: &str, pos: &[V3], info: &HoleInfo, target: &str) -> Option<Vec<Value>> {
    let HoleInfo { nd, diameter, through, depth, point_angle, cbore, csink } = *info;
    let ln = libm::sqrt(dot(nd, nd));
    if ln == 0.0 || !ln.is_finite() || pos.is_empty() {
        return None;
    }
    let n = unit3(nd);
    let a: V3 = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let x = unit3(cross(a, n));
    let y = cross(n, x);
    let r = diameter / 2.0;
    let mut out = Vec::new();
    if through && cbore.is_none() && csink.is_none() {
        let o = *pos.first()?;
        let mut pts = Vec::new();
        let mut cur = Vec::new();
        for (k, p) in pos.iter().enumerate() {
            let q = sub3(*p, o);
            pts.push(json!({"id": format!("h{k}"), "x": rnd(dot(q, x)), "y": rnd(dot(q, y))}));
            cur.push(json!({"id": format!("hc{k}"), "type": "circle", "center": format!("h{k}"), "radius": r}));
        }
        let sk = format!("{name}_sketch");
        out.push(json!({"type": "sketch", "name": sk, "plane": {"origin": o, "x_axis": x, "y_axis": y}, "points": pts, "curves": cur}));
        out.push(json!({"type": "extrude", "name": name, "sketch": sk, "profiles": "all", "operation": "cut", "target": target,
                        "distance": -THROUGH, "symmetric": false, "from_hole": true}));
        return Some(out);
    }
    // A through hole with a counterbore or countersink: as deep as a through-all cut.
    let (depth, point_angle) = if through { (THROUGH, None) } else { (depth, point_angle) };
    let tip = point_angle.map_or(0.0, |pa| r / libm::tan(pa.to_radians() / 2.0));
    let mut prof: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    match (cbore, csink) {
        (Some((d, cd)), _) => prof.extend([(d / 2.0, 0.0), (d / 2.0, -cd), (r, -cd)]),
        (None, Some((d, a))) => prof.extend([(d / 2.0, 0.0), (r, -(d / 2.0 - r) / libm::tan(a.to_radians() / 2.0))]),
        (None, None) => prof.push((r, 0.0)),
    }
    prof.extend([(r, -depth), (0.0, -depth - tip)]);
    for (k, p) in pos.iter().enumerate() {
        let sk = format!("{name}_{k}_sketch");
        let pts: Vec<Value> = prof
            .iter()
            .enumerate()
            .map(|(j, &(u, v))| json!({"id": format!("q{j}"), "x": rnd(u), "y": rnd(v)}))
            .collect();
        let cur: Vec<Value> = (0..prof.len())
            .map(|j| json!({"id": format!("e{j}"), "type": "line", "start": format!("q{j}"), "end": format!("q{}", (j + 1) % prof.len())}))
            .collect();
        out.push(json!({"type": "sketch", "name": sk, "plane": {"origin": p, "x_axis": x, "y_axis": n}, "points": pts, "curves": cur}));
        out.push(json!({"type": "revolve", "name": format!("{name}_{k}"), "sketch": sk, "profiles": "all", "operation": "cut",
                        "target": target, "axis": {"line": format!("e{}", prof.len() - 1)}, "angle": 360.0, "from_hole": name}));
    }
    Some(out)
}

/// A rectangular pattern. The record's references, in order: the patterned features (part or
/// pattern features; in older files a second direction is a pattern of its own, of the
/// originals and the first pattern), then per direction a `bb1dd5df` count (its last u32) and
/// a spacing parameter, the directions, and `90874d28` flags. A direction is an `a244457b`
/// edge (see [`pattern_direction`]) or, in version 3104, a `ce52df40` unit vector (notes AC). Of the flags after the
/// directions, the first per direction turns it round and the last per direction is midplane.
/// Two directions are two patterns: the second copies the features and the first pattern.
fn pattern(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    const G_DIR: u32 = 0xa244_457b;
    const G_UNIT_DIR: u32 = 0xce52_df40;
    const G_COUNT: u32 = 0xbb1d_d5df;
    const G_FLAGS: u32 = 0x9087_4d28;
    let refs = r.dc.refs(f.rec);
    // A feature that was left out (a cosmetic thread, note AQ) is not copied.
    let tgt: Vec<String> = refs
        .iter()
        .filter(|&&(_, x)| x != f.rec)
        .filter_map(|&(_, x)| st.names.get(&x).cloned())
        .filter(|n| st.emitted.get(n).is_none_or(|m| !m.is_empty()))
        .collect();
    let count_of = |x: usize| {
        let b = r.dc.r(x);
        u32_at(b, b.len().checked_sub(4)?)
    };
    let body: Vec<(usize, usize)> = refs.iter().copied().filter(|&(k, _)| k > 14).collect();
    let dirs: Vec<usize> =
        body.iter().map(|x| x.1).filter(|&x| matches!(r.dc.class(x), G_DIR | G_UNIT_DIR)).take(3).collect();
    if dirs.len() == 2 {
        return two_way_pattern(r, st, name, &body, &dirs, &tgt);
    }
    let dir = dirs.first().copied();
    // The count: newer records also hold an internal variable's count (`RDxVar..`, 0 or a
    // stale 2) before the list of bodies (note AR); the pattern's own comes after it.
    let counts: Vec<(usize, usize)> = refs
        .iter()
        .copied()
        .filter(|&(_, x)| r.dc.class(x) == G_COUNT && count_of(x).is_some_and(|c| 1 < c && c < 1000))
        .collect();
    let list_at = refs.iter().find(|&&(_, x)| r.dc.class(x) == G_BODY_LIST).map(|x| x.0);
    let cnt = counts.iter().find(|&&(k, _)| list_at.is_some_and(|l| k > l)).or_else(|| counts.first()).map(|&(_, x)| x);
    let par = refs.iter().map(|x| x.1).find(|&x| r.dc.role(x) == Role::Param);
    let (true, Some(dir), Some(cnt), Some(par)) = (!tgt.is_empty(), dir, cnt, par) else {
        return Err("direction, count or spacing not found".into());
    };
    let vec = pattern_direction(r, dir)?;
    let count = count_of(cnt).ok_or("no count")?;
    let flags: Vec<bool> = flags_after(r, &body, dir, G_FLAGS);
    let midplane = flags.last().copied().unwrap_or(false);
    // The first flag after the direction: the parameter is the distance from the first
    // occurrence to the last (spacing "fit"), not the spacing (note AR). The direction is
    // stored as the pattern runs.
    let fit = flags.len() > 1 && flags.first().copied().unwrap_or(false);
    let total = r.dc.param(par).ok_or("the spacing is not readable")?.1 * CM;
    let spacing = rnd(if fit { total / f64::from(count - 1) } else { total });
    let u = [rnd(vec[0]), rnd(vec[1]), rnd(vec[2])];
    if midplane {
        // Gusset has no midplane pattern: move the original holes back by half the run.
        let sh: V3 = u.map(|c| -c * spacing * f64::from(count - 1) / 2.0);
        let holes_only = tgt.iter().all(|t| st.holes.contains_key(t));
        if !holes_only {
            midplane_halves(st, name, &tgt, u, spacing, count)?;
            return Ok(());
        }
        for t in &tgt {
            let Some(info) = st.holes.get(t) else { continue };
            let Some(o) = st.out.iter_mut().find(|o| o.get("name").and_then(Value::as_str) == Some(t.as_str())) else {
                continue;
            };
            let moved: Vec<V3> = o
                .get("positions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?]))
                .map(|p| [rnd(p[0] + sh[0]), rnd(p[1] + sh[1]), rnd(p[2] + sh[2])])
                .collect();
            let fp = o
                .pointer("/face/point")
                .and_then(Value::as_array)
                .and_then(|p| Some([p.first()?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?]));
            put(o, "positions", Value::Array(moved.iter().map(|p| arr(*p)).collect()));
            if let (Some(fp), Some(face)) = (fp, o.get_mut("face")) {
                put(face, "point", arr([rnd(fp[0] + sh[0]), rnd(fp[1] + sh[1]), rnd(fp[2] + sh[2])]));
            }
            if o.get("_fallback").is_some() {
                let fb = st.first_body.as_deref().and_then(|tb| hole_fallback(t, &moved, info, tb));
                match fb {
                    Some(fb) => put(o, "_fallback", Value::Array(fb)),
                    None => {
                        if let Some(m) = o.as_object_mut() {
                            m.remove("_fallback");
                        }
                    }
                }
            }
        }
    }
    st.out.push(json!({"type": "pattern_linear", "name": name, "features": tgt, "direction": u, "spacing": spacing, "count": count}));
    Ok(())
}

/// The unit direction a rectangular pattern runs along. An `a244457b` edge holds three triples
/// at its end (cm): its start, its vector and the end the pattern starts from; the pattern runs
/// from that end along the edge (against the vector when it is the edge's end point, along it
/// when it is the start). A `ce52df40` holds five doubles, the last three the unit vector.
fn pattern_direction(r: &Reader, dir: usize) -> Result<V3, String> {
    let rec = r.dc.r(dir);
    let n = rec.len();
    let at = |o: Option<usize>| -> Result<V3, String> {
        let o = o.ok_or("the direction is cut short")?;
        match [f64_at(rec, o), f64_at(rec, o + 8), f64_at(rec, o + 16)] {
            [Some(x), Some(y), Some(z)] if x.is_finite() && y.is_finite() && z.is_finite() => Ok([x, y, z]),
            _ => Err("the direction is cut short".into()),
        }
    };
    let (vec, sign) = if r.dc.class(dir) == 0xa244_457b {
        // Start point, vector, the end it starts from, then `u8 1` and two references.
        let (start, vec, from) = (at(n.checked_sub(81))?, at(n.checked_sub(57))?, at(n.checked_sub(33))?);
        let end = [start[0] + vec[0], start[1] + vec[1], start[2] + vec[2]];
        let d = |a: V3, b: V3| dot(sub3(a, b), sub3(a, b));
        (vec, if d(from, start) < d(from, end) { 1.0 } else { -1.0 })
    } else {
        (at(n.checked_sub(24))?, 1.0)
    };
    let ln = libm::sqrt(dot(vec, vec));
    if ln == 0.0 || !ln.is_finite() {
        return Err("the direction has no length".into());
    }
    Ok([sign * vec[0] / ln, sign * vec[1] / ln, sign * vec[2] / ln])
}

/// The flag records (`90874d28`, set when the last byte is not 0) after a pattern's last
/// direction.
fn flags_after(r: &Reader, body: &[(usize, usize)], last_dir: usize, g_flags: u32) -> Vec<bool> {
    body.iter()
        .skip_while(|&&(_, x)| x != last_dir)
        .filter(|&&(_, x)| r.dc.class(x) == g_flags)
        .map(|&(_, x)| r.dc.r(x).last().is_some_and(|&b| b != 0))
        .collect()
}

/// A rectangular pattern in two directions (one record, both counts, spacings and
/// directions): a pattern along the first, then one along the second of the features and the
/// first pattern. A midplane direction is refused.
fn two_way_pattern(
    r: &Reader,
    st: &mut State,
    name: &str,
    body: &[(usize, usize)],
    dirs: &[usize],
    tgt: &[String],
) -> Result<(), String> {
    const G_COUNT: u32 = 0xbb1d_d5df;
    const G_FLAGS: u32 = 0x9087_4d28;
    let (&[d1, d2], false) = (dirs, tgt.is_empty()) else { return Err("direction, count or spacing not found".into()) };
    let counts: Vec<u32> = body
        .iter()
        .filter(|&&(_, x)| r.dc.class(x) == G_COUNT)
        .filter_map(|&(_, x)| {
            let b = r.dc.r(x);
            u32_at(b, b.len().checked_sub(4)?)
        })
        .filter(|&c| (1..1000).contains(&c))
        .collect();
    let pars: Vec<f64> = body
        .iter()
        .filter(|&&(_, x)| r.dc.role(x) == Role::Param)
        .filter_map(|&(_, x)| r.dc.param(x))
        .map(|p| p.1)
        .collect();
    let (&[c1, c2], Some(&s1), Some(&s2)) = (counts.as_slice(), pars.first(), pars.get(1)) else {
        return Err("direction, count or spacing not found".into());
    };
    let flags = flags_after(r, body, d2, G_FLAGS);
    let flag = |k: usize| flags.get(k).copied().unwrap_or(false);
    let n = flags.len();
    if n < 4 {
        return Err("direction, count or spacing not found".into());
    }
    let mut made: Vec<String> = Vec::new();
    for (k, (dir, count, spacing)) in [(d1, c1, s1), (d2, c2, s2)].into_iter().enumerate() {
        if count < 2 {
            continue;
        }
        let v = pattern_direction(r, dir)?;
        let v = if flag(k) { [-v[0], -v[1], -v[2]] } else { v };
        let u = [rnd(v[0]), rnd(v[1]), rnd(v[2])];
        let nm = if made.is_empty() { name.to_string() } else { format!("{name}_2") };
        let mut feats: Vec<String> = tgt.to_vec();
        feats.extend(made.iter().cloned());
        // The last flag of each direction: midplane.
        if flag(n - 2 + k) {
            made.extend(midplane_halves(st, &nm, &feats, u, rnd(spacing * CM), count)?);
            continue;
        }
        st.out.push(json!({"type": "pattern_linear", "name": nm, "features": feats,
            "direction": u, "spacing": rnd(spacing * CM), "count": count}));
        made.push(nm);
    }
    if made.is_empty() {
        return Err("a pattern of one occurrence".into());
    }
    Ok(())
}

/// A midplane pattern of features (occurrences on both sides of the originals): Gusset
/// patterns run one way, so it is a pattern each way sharing the originals. An odd count has
/// `(count - 1) / 2` copies each way; an even count one more backward (`count / 2`) than
/// forward (confirmed for 2 on the `pt_*` fixtures: the copy is behind). The names of the
/// patterns made.
fn midplane_halves(
    st: &mut State,
    name: &str,
    feats: &[String],
    u: V3,
    spacing: f64,
    count: u32,
) -> Result<Vec<String>, String> {
    let (fw, bw) =
        if count.is_multiple_of(2) { (count / 2, count / 2 + 1) } else { (count.div_ceil(2), count.div_ceil(2)) };
    let mut made = Vec::new();
    if fw > 1 {
        st.out.push(json!({"type": "pattern_linear", "name": name, "features": feats, "direction": u, "spacing": spacing, "count": fw}));
        made.push(name.to_string());
    }
    let back = if made.is_empty() { name.to_string() } else { format!("{name}_back") };
    let neg = [rnd(-u[0]), rnd(-u[1]), rnd(-u[2])];
    st.out.push(json!({"type": "pattern_linear", "name": back, "features": feats, "direction": neg, "spacing": spacing, "count": bw}));
    made.push(back);
    Ok(made)
}

/// A revolve axis (point `p` in mm, unit direction `v`) that lies in the sketch plane but is
/// not one of the sketch's own axes through its origin, as a construction line of the sketch:
/// its two points, the line and the line's id. `None` for the sketch's own axes (read as
/// such) and for an axis off the plane.
fn sketch_axis_line(sk: &Sketch, p: V3, v: V3, rec: usize) -> Option<(Vec<Value>, Value, String)> {
    let (o, x, y) = frame(&sk.m);
    let q = sub3(p, o);
    let along = |a: V3| (dot(v, a).abs() - 1.0).abs() < 1e-9;
    let through_origin = dot(cross(q, v), cross(q, v)) < 1e-12;
    if through_origin && (along(x) || along(y)) {
        return None;
    }
    sketch_line_along(sk, p, v, rec)
}

/// A construction line of the sketch from `p` (mm) in the direction `v`, when both lie in its
/// plane: its ids.
fn sketch_line_along(sk: &Sketch, p: V3, v: V3, rec: usize) -> Option<(Vec<Value>, Value, String)> {
    let (o, x, y) = frame(&sk.m);
    let n = unit3(cross(x, y));
    let q = sub3(p, o);
    if dot(q, n).abs() > 1e-6 || dot(v, n).abs() > 1e-9 {
        return None;
    }
    let (u0, v0) = (rnd(dot(q, x)), rnd(dot(q, y)));
    let (du, dv) = (dot(v, x), dot(v, y));
    // A line of the sketch's size, so that its ends are not lost in rounding.
    let len = 10.0;
    let (a, b, l) = (format!("ax{rec}a"), format!("ax{rec}b"), format!("ax{rec}"));
    let pts = vec![
        json!({"id": a, "x": u0, "y": v0, "construction": true}),
        json!({"id": b, "x": rnd(u0 + len * du), "y": rnd(v0 + len * dv), "construction": true}),
    ];
    let line = json!({"id": l, "type": "line", "start": a, "end": b, "construction": true});
    Some((pts, line, l))
}

/// A feature whose own records are not read, rebuilt as the solid of revolution its faces in
/// the result body show (notes AE; the Design Accelerator pulley flanges, `9a78add3`): the
/// profile is the chain of their generatrices, closed along the axis when it is open, revolved
/// a full turn and joined to the body. Any face not of revolution about one axis refuses it.
fn revolved_from_faces(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let b = r.result().ok_or("the result body is not readable")?;
    let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
    let (o, ax, segs) = b.revolution_profile(fid)?;
    let target = st.first_body.clone().ok_or("a join before any body")?;
    let sk = format!("{name}_profile");
    let (sketch, origin, ax) = revolution_sketch(&sk, o, ax, segs)?;
    st.out.push(sketch);
    st.out.push(
        json!({"type": "revolve", "name": name, "sketch": sk, "profiles": "all", "operation": "join", "target": target,
        "axis": {"origin": origin, "direction": [rnd(ax[0]), rnd(ax[1]), rnd(ax[2])]}, "angle": 360.0}),
    );
    Ok(())
}

/// The sketch of a profile of revolution (file units: axis point `o`, axis `ax`, pieces in
/// radius and height): the pieces chained from an end only one of them has, closed along the
/// axis when the chain is open, on the plane through the axis (its y axis). Also the plane's
/// origin (mm) and the unit axis.
pub(crate) fn revolution_sketch(sk: &str, o: V3, ax: V3, segs: Vec<RevSeg>) -> Result<(Value, Value, V3), String> {
    const TOL: f64 = 1e-6;
    let ends = |g: &RevSeg| match *g {
        RevSeg::Line(a, b) | RevSeg::Arc { a, b, .. } => (a, b),
    };
    let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < TOL && (p[1] - q[1]).abs() < TOL;
    let segs: Vec<RevSeg> = segs.into_iter().filter(|g| !near(ends(g).0, ends(g).1)).collect();
    if segs.is_empty() || segs.len() > MAX_CURVES {
        return Err("no profile of revolution".into());
    }
    // Chain the pieces from an end that only one of them has (or any, for a closed chain).
    let degree = |p: [f64; 2]| segs.iter().filter(|g| near(ends(g).0, p) || near(ends(g).1, p)).count();
    let start = segs.iter().flat_map(|g| [ends(g).0, ends(g).1]).find(|&p| degree(p) == 1);
    let mut cur = start.unwrap_or(ends(segs.first().ok_or("no profile")?).0);
    let first = cur;
    let mut left: Vec<RevSeg> = segs;
    let mut chain: Vec<(RevSeg, [f64; 2])> = Vec::new();
    while let Some(k) = left.iter().position(|g| near(ends(g).0, cur) || near(ends(g).1, cur)) {
        let g = left.swap_remove(k);
        let (a, b) = ends(&g);
        let next = if near(a, cur) { b } else { a };
        chain.push((g, next));
        cur = next;
    }
    if !left.is_empty() {
        return Err("the faces of revolution do not make one profile".into());
    }
    let mm = |p: [f64; 2]| [rnd(p[0] * CM), rnd(p[1] * CM)];
    let mut pts: Vec<(String, [f64; 2])> = vec![("q0".into(), mm(first))];
    let mut curves: Vec<Value> = Vec::new();
    for (k, (g, next)) in chain.iter().enumerate() {
        let (from, to) = (format!("q{k}"), format!("q{}", k + 1));
        pts.push((to.clone(), mm(*next)));
        let id = format!("e{k}");
        match *g {
            RevSeg::Line(..) => curves.push(json!({"id": id, "type": "line", "start": from, "end": to})),
            RevSeg::Arc { c, .. } => {
                // Counter-clockwise from start to end: the arc of a blend is under half a turn.
                let (cm, p, q) = (mm(c), mm(chain.get(k.wrapping_sub(1)).map_or(first, |x| x.1)), mm(*next));
                let ang = |x: [f64; 2]| libm::atan2(x[1] - cm[1], x[0] - cm[0]);
                let ccw = (ang(q) - ang(p)).rem_euclid(std::f64::consts::TAU) <= std::f64::consts::PI;
                let centre = format!("c{k}");
                pts.push((centre.clone(), cm));
                let (s0, s1) = if ccw { (from, to) } else { (to, from) };
                curves.push(json!({"id": id, "type": "arc", "center": centre, "start": s0, "end": s1}));
            }
        }
    }
    if !near(cur, first) {
        // Open: closed along the axis.
        let (a, z) = (mm(cur), mm(first));
        let mut last = format!("q{}", chain.len());
        for (k, p) in [[0.0, a[1]], [0.0, z[1]]].into_iter().enumerate() {
            let same = |q: [f64; 2]| near(q, p);
            if same(z) || pts.iter().any(|(id, q)| *id == last && same(*q)) {
                continue;
            }
            let id = format!("x{k}");
            pts.push((id.clone(), p));
            curves.push(json!({"id": format!("ex{k}"), "type": "line", "start": last, "end": id}));
            last = id;
        }
        curves.push(json!({"id": "eclose", "type": "line", "start": last, "end": "q0"}));
    }
    let ax = unit3(ax);
    let w = if ax[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let x = unit3(cross(cross(ax, w), ax));
    let points: Vec<Value> = pts.iter().map(|(id, p)| json!({"id": id, "x": p[0], "y": p[1]})).collect();
    let origin = json!([rnd(o[0] * CM), rnd(o[1] * CM), rnd(o[2] * CM)]);
    let sketch = json!({"type": "sketch", "name": sk, "plane": {"origin": origin, "x_axis": [rnd(x[0]), rnd(x[1]), rnd(x[2])],
        "y_axis": [rnd(ax[0]), rnd(ax[1]), rnd(ax[2])]}, "points": points, "curves": curves});
    Ok((sketch, origin, ax))
}

/// A work axis record (`8ef06c89`, as a revolve's axis): start point and unit vector, in cm.
fn work_axis(dc: &Dc, ax: usize) -> Result<(V3, V3), String> {
    let rec = dc.r(ax);
    let k = rec
        .get(dc.body(ax)..)
        .and_then(|t| t.windows(4).position(|w| w == [0x02, 0, 0, 0x30]))
        .map(|k| k + dc.body(ax))
        .ok_or("the axis is not readable")?;
    let g = |o: usize| f64_at(rec, k + o).ok_or("the axis is cut short");
    let p0 = [g(8)?, g(16)?, g(24)?];
    let v = [g(32)?, g(40)?, g(48)?];
    let ln = libm::sqrt(dot(v, v));
    if !(ln > 1e-12 && ln.is_finite() && p0.iter().all(|c| c.is_finite())) {
        return Err("the axis has no direction".into());
    }
    Ok((p0, [v[0] / ln, v[1] / ln, v[2] / ln]))
}

/// What a pattern or mirror copies: the features it refers to before its own settings (the
/// `90874d18` record), as the tree names them in the order they were made, or (empty, with
/// the body's tree name) the
/// whole body when the flag after its `d83ef271` record is set and it lists no feature, or
/// when it lists every feature so far. A
/// feature that was left out (suppressed, a cosmetic thread) is skipped; one that could not
/// be read refuses the copy.
fn copied_features(r: &Reader, st: &State, f: &Feature) -> Result<(Vec<String>, Option<String>), String> {
    let refs = r.dc.refs(f.rec);
    let end = refs.iter().find(|&&(k, x)| k > 14 && r.dc.role(x) == Role::SkPlacement).map_or(usize::MAX, |x| x.0);
    let mut out: Vec<String> = Vec::new();
    for &(k, x) in &refs {
        if k <= 14 || k >= end || x == f.rec {
            continue;
        }
        let Some(n) = st.names.get(&x) else { continue };
        let made = st.emitted.get(n).ok_or_else(|| format!("its feature {n} comes later"))?;
        if made.is_empty() {
            continue;
        }
        let read = st.out.iter().filter(|o| made.iter().any(|m| o.get("name").and_then(Value::as_str) == Some(m)));
        if read.clone().any(|o| o.get("type").and_then(Value::as_str) == Some("unsupported")) {
            return Err(format!("its feature {n} is not read"));
        }
        for m in made {
            if !out.contains(m) {
                out.push(m.clone());
            }
        }
    }
    // In the order they were made (the selection's order is the user's picks): a chamfer
    // copied with the cut that made its edge comes after it (EY4180 A-02).
    out.sort_by_key(|m| st.out.iter().position(|o| o.get("name").and_then(Value::as_str) == Some(m)));
    if out.is_empty() {
        let solids = copy_flags(r, f).first().copied().unwrap_or(false);
        return match (solids, &st.first_body) {
            (true, Some(b)) => Ok((out, Some(b.clone()))),
            _ => Err("no feature to copy".into()),
        };
    }
    // The features listed include the one that made the body: a copy of every feature so far
    // is a copy of the body (built again, the first feature's copy would be a body of its own).
    if let Some(b) = st.first_body.as_ref().filter(|b| out.contains(b)) {
        let so_far = st.out.iter().filter(|o| o.get("type").and_then(Value::as_str) != Some("sketch"));
        if so_far.clone().all(|o| o.get("name").and_then(Value::as_str).is_some_and(|n| out.iter().any(|m| m == n))) {
            return Ok((Vec::new(), Some(b.clone())));
        }
        return Err("it copies the feature that made the body, but not every feature before it".into());
    }
    Ok((out, None))
}

/// A circular pattern (`06977131`, notes AA): the copied features, the count (`bb1dd5df`
/// whose last word is above 1), the angle (the parameter, radians; 2 pi = a full turn, the
/// copies spread evenly) and the axis (`8ef06c89` work axis, cm). The flag after the axis's
/// `8b2be62e` record, when set, turns the other way.
fn circular_pattern(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    const G_COUNT: u32 = 0xbb1d_d5df;
    const G_AXIS: u32 = 0x8ef0_6c89;
    const G_FLAGS: u32 = 0x9087_4d28;
    let (tgt, body) = copied_features(r, st, f)?;
    let refs: Vec<(usize, usize)> = r.dc.refs(f.rec).into_iter().filter(|&(k, _)| k > 14).collect();
    let count_of = |x: usize| {
        let b = r.dc.r(x);
        u32_at(b, b.len().checked_sub(4)?)
    };
    let count = refs
        .iter()
        .filter(|&&(_, x)| r.dc.class(x) == G_COUNT)
        .filter_map(|&(_, x)| count_of(x))
        .find(|&c| 1 < c && c < 1000)
        .ok_or("no count")?;
    let par = refs.iter().map(|x| x.1).find(|&x| r.dc.role(x) == Role::Param).ok_or("no angle")?;
    let angle = r.dc.param(par).ok_or("the angle is not readable")?.1.to_degrees();
    let ka = refs.iter().position(|&(_, x)| r.dc.class(x) == G_AXIS).ok_or("the axis is not a work axis")?;
    let (p0, mut v) = work_axis(&r.dc, refs.get(ka).ok_or("no axis")?.1)?;
    let flip = refs
        .iter()
        .skip(ka + 1)
        .find(|&&(_, x)| r.dc.class(x) == G_FLAGS)
        .is_some_and(|&(_, x)| r.dc.r(x).last().is_some_and(|&b| b != 0));
    if flip {
        v = [-v[0], -v[1], -v[2]];
    }
    if !(angle.is_finite() && angle.abs() > 1e-9 && angle.abs() <= 360.0 + 1e-6) {
        return Err("the angle is out of range".into());
    }
    let angle = if (angle.abs() - 360.0).abs() < 1e-6 { 360.0 } else { rnd(angle) };
    let mut j = json!({"type": "pattern_circular", "name": name, "features": tgt,
        "axis": {"origin": [rnd(p0[0] * CM), rnd(p0[1] * CM), rnd(p0[2] * CM)], "direction": [rnd(v[0]), rnd(v[1]), rnd(v[2])]},
        "angle": angle, "count": count});
    if let Some(b) = body {
        put(&mut j, "target", json!(b));
    }
    st.out.push(j);
    Ok(())
}

/// A mirror (`fad9a9b5`, notes AA): the copied features and the plane, the last work-plane
/// record (`ce52df42`) it refers to, whose last 9 doubles are a point and two unit vectors
/// spanning the plane (cm).
fn mirror(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    const G_PLANE: u32 = 0xce52_df42;
    let (tgt, body) = copied_features(r, st, f)?;
    let pl =
        r.dc.refs(f.rec)
            .into_iter()
            .rfind(|&(k, x)| k > 14 && r.dc.class(x) == G_PLANE)
            .ok_or("the mirror plane is not a work plane")?
            .1;
    let (o, n) = work_plane(&r.dc, pl).map_err(|w| format!("the mirror {w}"))?;
    let mut j = json!({"type": "mirror", "name": name, "features": tgt,
        "plane": {"origin": [rnd(o[0] * CM), rnd(o[1] * CM), rnd(o[2] * CM)], "normal": [rnd(n[0]), rnd(n[1]), rnd(n[2])]}});
    // A mirror of the body whose second flag after its references is set removes the original
    // (note AR): the mirrored copy is the body from then on.
    let remove = body.is_some() && copy_flags(r, f).get(1).copied().unwrap_or(false);
    if let Some(b) = &body {
        put(&mut j, "target", json!(b));
        if remove {
            put(&mut j, "merge", json!(false));
        }
    }
    st.out.push(j);
    if let (Some(b), true) = (body, remove) {
        st.out.push(json!({"type": "remove_body", "name": format!("{name}_original"), "target": b}));
        for v in st.bodies.values_mut().filter(|v| **v == b) {
            *v = name.to_string();
        }
        st.first_body = Some(name.to_string());
    }
    Ok(())
}

/// The flag records (`90874d28`, set when the last byte is 1) after a copy feature's
/// references record (`d83ef271`): the first copies the solid bodies, the second (mirror)
/// removes the original.
fn copy_flags(r: &Reader, f: &Feature) -> Vec<bool> {
    const G_REFS: u32 = 0xd83e_f271;
    const G_FLAGS: u32 = 0x9087_4d28;
    r.dc.refs(f.rec)
        .into_iter()
        .skip_while(|&(k, x)| k <= 14 || r.dc.class(x) != G_REFS)
        .filter(|&(_, x)| r.dc.class(x) == G_FLAGS)
        .map(|(_, x)| r.dc.r(x).last() == Some(&1))
        .collect()
}

/// A thicken of a side face of the base block: the same as a join extrude of that side's
/// outline (the base block's extent across the side), from the cap plane back by the thickness
/// (section F of the notes). The thickness is the parameter under the face reference (slot 0);
/// the cap is the thicken-tagged planar face lying exactly that far outside the first extrusion's
/// bounding box. The rebuild check confirms the reading.
fn thicken(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let mut dist = None;
    for (_, x1) in r.dc.refs(s.need(0)?) {
        for (_, x2) in r.dc.refs(x1) {
            if r.dc.role(x2) == Role::Param {
                dist = r.dc.param(x2).map(|p| p.1 * CM);
            }
        }
    }
    let dist = dist.filter(|d| d.is_finite() && *d > 0.0).ok_or("the thickness is not readable")?;
    let b = r.result().ok_or("the result body is not readable")?;
    let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
    let num = |v: Option<&Value>| v.and_then(Value::as_f64);
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([num(a.first())?, num(a.get(1))?, num(a.get(2))?])
    };
    let first = st
        .out
        .iter()
        .find(|o| {
            o.get("type").and_then(Value::as_str) == Some("extrude")
                && o.get("operation").and_then(Value::as_str) == Some("new")
        })
        .ok_or("no base extrusion")?;
    let sk = st
        .out
        .iter()
        .find(|o| o.get("type").and_then(Value::as_str) == Some("sketch") && o.get("name") == first.get("sketch"))
        .ok_or("the base sketch is not found")?;
    let (o, x, y) = (
        v3(sk.pointer("/plane/origin")).ok_or("sketch plane")?,
        v3(sk.pointer("/plane/x_axis")).ok_or("sketch plane")?,
        v3(sk.pointer("/plane/y_axis")).ok_or("sketch plane")?,
    );
    let n = cross(x, y);
    let len = num(first.get("distance")).ok_or("the base extent")?;
    let symmetric = first.get("symmetric").and_then(Value::as_bool) == Some(true);
    let target = first.get("name").and_then(Value::as_str).ok_or("the base name")?.to_string();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in sk.get("points").and_then(Value::as_array).into_iter().flatten() {
        let (Some(u), Some(v)) = (num(p.get("x")), num(p.get("y"))) else { return Err("a base sketch point".into()) };
        for k in [if symmetric { -len } else { 0.0 }, len] {
            for i in 0..3 {
                let c = o.get(i).copied().unwrap_or(0.0)
                    + u * x.get(i).copied().unwrap_or(0.0)
                    + v * y.get(i).copied().unwrap_or(0.0)
                    + k * n.get(i).copied().unwrap_or(0.0);
                if let (Some(l), Some(h)) = (lo.get_mut(i), hi.get_mut(i)) {
                    *l = l.min(c);
                    *h = h.max(c);
                }
            }
        }
    }
    let mut caps: Vec<(usize, f64, f64)> = Vec::new();
    for (p, nn) in b.feature_planes(fid) {
        for ax in 0..3 {
            let na = nn.get(ax).copied().unwrap_or(0.0);
            if (na.abs() - 1.0).abs() < 1e-6 {
                let sg = if na > 0.0 { 1.0 } else { -1.0 };
                let c = p.get(ax).copied().unwrap_or(0.0) * CM;
                let ext = if sg > 0.0 { hi.get(ax) } else { lo.get(ax) }.copied().unwrap_or(f64::NAN);
                if (sg * (c - ext) - dist).abs() < 1e-4 && !caps.iter().any(|q| q.0 == ax && (q.2 - c).abs() < 1e-6) {
                    caps.push((ax, sg, c));
                }
            }
        }
    }
    let [(ax, sg, c)] = caps.as_slice() else {
        return Err(format!("the cap face is not found ({} candidates)", caps.len()));
    };
    let (ax, sg, c) = (*ax, *sg, *c);
    let (j, k) = match ax {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let unit_of = |i: usize, s: f64| -> V3 {
        let mut v = [0.0; 3];
        if let Some(e) = v.get_mut(i) {
            *e = s;
        }
        v
    };
    let ej = unit_of(j, 1.0);
    let mut ek = unit_of(k, 1.0);
    if dot(cross(ej, ek), unit_of(ax, sg)) < 0.0 {
        ek = unit_of(k, -1.0);
    }
    let mut org = [0.0; 3];
    for (i, e) in org.iter_mut().enumerate() {
        *e = (lo.get(i).copied().unwrap_or(0.0) + hi.get(i).copied().unwrap_or(0.0)) / 2.0;
    }
    if let Some(e) = org.get_mut(ax) {
        *e = c;
    }
    let half = |i: usize| (hi.get(i).copied().unwrap_or(0.0) - lo.get(i).copied().unwrap_or(0.0)) / 2.0;
    let (hj, hk) = (half(j), half(k));
    let ps = [(-hj, -hk), (hj, -hk), (hj, hk), (-hj, hk)];
    let sname = format!("{name}_sketch");
    let points: Vec<Value> =
        ps.iter().enumerate().map(|(i, &(u, v))| json!({"id": format!("t{i}"), "x": rnd(u), "y": rnd(v)})).collect();
    let curves: Vec<Value> = (0..4)
        .map(|i| json!({"id": format!("tl{i}"), "type": "line", "start": format!("t{i}"), "end": format!("t{}", (i + 1) % 4)}))
        .collect();
    st.out.push(json!({"type": "sketch", "name": sname, "plane": {"origin": arr(org), "x_axis": ej, "y_axis": ek},
                       "points": points, "curves": curves}));
    st.out.push(json!({"type": "extrude", "name": name, "sketch": sname, "profiles": "all", "operation": "join",
                       "target": target, "distance": rnd(-dist), "symmetric": false}));
    Ok(())
}

/// A shell: slot 3 the wall thickness, slot 0 ends with the side (0 inside, 1 outside), slot 1
/// lists the names of the faces it opens. The names are not decoded: the open faces are found
/// in the result body instead ([`Acis::shell_openings`]), and their number must be the list's.
fn shell(r: &Reader, st: &mut State, f: &Feature, name: &str, later: &[i64]) -> Result<(), String> {
    let s = r.slots(f)?;
    let t = r.pval(&s, 3)? * CM;
    let side = match s.get(0).and_then(|i| tail_u16(&r.dc, i)) {
        Some(0) => "inside",
        Some(1) => "outside",
        o => return Err(format!("shell side {o:?} is not decoded")),
    };
    let target = st.first_body.clone().ok_or("a shell before any body")?;
    let n = r.dc.first_list(s.need(1)?, None).map(|x| x.0.len()).ok_or("the shell's face list is not readable")?;
    let mut j = json!({"type": "shell", "name": name, "thickness": rnd(t), "side": side});
    if n == 0 {
        put(&mut j, "body", json!(target));
    } else {
        let b = r.result().ok_or("the result body is not readable")?;
        let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
        let open = b.shell_openings(fid, later);
        let mut pts: Vec<V3> = open.iter().map(|(p, _)| [p[0] * CM, p[1] * CM, p[2] * CM]).collect();
        // More or fewer rims than open faces: the faces the shell names give the points.
        if open.len() != n {
            let why = || format!("{} of its {n} open faces found in the result body", open.len());
            pts = named_openings(r, st, s.need(1)?, &why)?;
            if pts.len() != n {
                return Err(why());
            }
        }
        let faces: Vec<Value> =
            pts.iter().map(|p| json!({"point": [rnd_to(p[0], 1e6), rnd_to(p[1], 1e6), rnd_to(p[2], 1e6)]})).collect();
        put(&mut j, "faces", json!(faces));
    }
    st.out.push(j);
    Ok(())
}

/// The surface of a face placed from its name (mm): a plane's unit normal, or a cylinder's
/// axis point and unit direction, radius, and whether the extrude that made it cut (a hole's
/// wall) or added material (a boss).
enum NamedSurf {
    Plane(V3),
    Cyl { c: V3, ax: V3, r: f64, cut: bool },
}

/// A face an extrude made, from its name (see [`face_name`]): a point in its middle and its
/// surface (mm). A start or end cap at the extrude's profile pick point moved to the cap, a side
/// face at the middle of the sketch line it was swept from, or on the circle it was swept from
/// (the name's curve number counts the sketch's entities from 1), half way along the extrude.
fn named_face(r: &Reader, st: &State, (fid, a, b): (i64, i64, i64)) -> Result<(V3, NamedSurf), String> {
    let (&frec, fname) = st
        .names
        .iter()
        .find(|(k, _)| r.dc.seq(**k).map(i64::from) == Some(fid))
        .ok_or("the feature that made it is not found")?;
    let ex = st
        .out
        .iter()
        .find(|o| {
            o.get("type").and_then(Value::as_str) == Some("extrude")
                && o.get("name").and_then(Value::as_str) == Some(fname)
        })
        .ok_or_else(|| format!("{} is not an extrude", fname))?;
    if ex.get("symmetric").and_then(Value::as_bool) == Some(true) || ex.get("distance2").is_some() {
        return Err(format!("{} runs both ways", fname));
    }
    let d = ex.get("distance").and_then(Value::as_f64).ok_or_else(|| format!("{} has no distance", fname))?;
    let ps = r
        .slots(&Feature { rec: frec, name: None, typ: None })?
        .get(1)
        .and_then(|l| r.profile_sketch(l))
        .ok_or("the extrude's sketch is not found")?;
    let sk = r.sketch(ps)?;
    let (o, x, y) = frame(&sk.m);
    let nn = unit3(cross(x, y));
    let w = |q: P2, h: f64| {
        add3(
            add3(o, add3([x[0] * q.0, x[1] * q.0, x[2] * q.0], [y[0] * q.1, y[1] * q.1, y[2] * q.1])),
            [nn[0] * h, nn[1] * h, nn[2] * h],
        )
    };
    match b {
        4 | 12 => {
            let q = ex
                .pointer("/profiles/0/point")
                .and_then(Value::as_array)
                .and_then(|pk| Some((pk.first()?.as_f64()?, pk.get(1)?.as_f64()?)))
                .ok_or("the extrude has no profile point")?;
            Ok((w(q, if b == 12 { d } else { 0.0 }), NamedSurf::Plane(nn)))
        }
        20 => {
            let entity = usize::try_from(a)
                .ok()
                .and_then(|a| r.owned(ps).get(a.checked_sub(1)?).copied())
                .ok_or("its sketch entity is not found")?;
            let pts: HashMap<usize, P2> = sk.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect();
            let curve = sk
                .curves
                .iter()
                .find(|c| matches!(**c, Curve::Line { id, .. } | Curve::Circle { id, .. } if id == entity))
                .ok_or_else(|| format!("its sketch entity is not a line or circle ({:?})", r.dc.role(entity)))?;
            match *curve {
                Curve::Line { a, b, .. } => {
                    let (pa, pb) = (*pts.get(&a).ok_or("a line end")?, *pts.get(&b).ok_or("a line end")?);
                    let dir = sub3(w(pb, 0.0), w(pa, 0.0));
                    Ok((
                        w(((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0), d / 2.0),
                        NamedSurf::Plane(unit3(cross(dir, nn))),
                    ))
                }
                Curve::Circle { c, r: rad, .. } => {
                    let c = *pts.get(&c).ok_or("a circle centre")?;
                    let cut = ex.get("operation").and_then(Value::as_str) == Some("cut");
                    Ok((w((c.0 + rad, c.1), d / 2.0), NamedSurf::Cyl { c: w(c, 0.0), ax: nn, r: rad, cut }))
                }
                _ => Err("its sketch entity is not a line or circle".into()),
            }
        }
        _ => Err(format!("face kind {b} is not read")),
    }
}

/// A face draft (`f4fa0900`): slot 0 the pull direction, 1 the faces (named, see
/// [`face_name`]), 3 the angle (radians). Each face turns about the line where its plane before
/// the draft (from its name) meets its plane in the result body (found by its name's
/// attributes); those lines must lie in one plane across the pull direction, which is the
/// draft's neutral plane, and the planes must be the angle apart. The part narrows along the
/// pull as the result's faces lean.
fn face_draft(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let pull = r.dc.skdir(s.need(0)?).map(|a| unit3([a[2], a[3], a[4]])).ok_or("the pull direction is not readable")?;
    let angle = r.pval(&s, 3)?;
    let (items, _) = r.dc.first_list(s.need(1)?, None).ok_or("the draft's face list is not readable")?;
    let res = r.result().ok_or("the result body is not readable")?;
    let mut picks: Vec<Value> = Vec::new();
    let mut plane: Option<(f64, bool)> = None;
    for it in items {
        let nm = it.and_then(|x| face_name(r, x)).ok_or("a drafted face's name is not read")?;
        let (p0, surf) = named_face(r, st, nm).map_err(|w| format!("a drafted face: {w}"))?;
        let (h, lean) = match surf {
            NamedSurf::Plane(n0) => {
                let (q1, n1) = named_result_plane(res, nm).ok_or("a drafted face is not a plane in the result body")?;
                // The part narrows along the pull where the result's outward normals lean along it.
                (hinge_height((p0, n0), (q1, n1), pull, angle)?, dot(n1, pull) > 0.0)
            }
            NamedSurf::Cyl { c, ax, r: rad, cut } => {
                if (dot(ax, pull).abs() - 1.0).abs() > 1e-9 {
                    return Err("a drafted cylinder does not run along the pull".into());
                }
                let rims = named_result_rims(res, nm, c, pull).ok_or("a drafted cylinder's rims are not found")?;
                let ((h0, r0), (h1, r1)) = rims;
                let slope = (r1 - r0) / (h1 - h0);
                if (libm::atan(slope.abs()) - angle).abs() > 1e-6 {
                    return Err("a drafted cylinder is not turned by the draft angle".into());
                }
                // A boss narrows where its radius falls along the pull, a hole where it grows.
                (h0 + (rad - r0) / slope, (slope < 0.0) != cut)
            }
        };
        match plane {
            None => plane = Some((h, lean)),
            Some((h0, l0)) if (h0 - h).abs() < 1e-6 && l0 == lean => {}
            Some(_) => return Err("the drafted faces turn about lines in several planes".into()),
        }
        picks.push(json!({"point": arr(p0)}));
    }
    let (h, lean) = plane.ok_or("the draft has no faces")?;
    let normal = if lean { pull } else { [-pull[0], -pull[1], -pull[2]] };
    st.out.push(json!({"type": "draft", "name": name, "faces": picks,
                       "plane": {"origin": arr([pull[0] * h, pull[1] * h, pull[2] * h]), "normal": arr(normal)},
                       "angle": rnd(angle.to_degrees())}));
    Ok(())
}

/// How far along the pull (unit) the line lies where a face's plane before a draft (a point and
/// unit normal) meets its plane after: the height of the draft's neutral plane. The line must run
/// across the pull and the planes be `angle` (radians) apart.
pub(crate) fn hinge_height((p0, n0): (V3, V3), (q1, n1): (V3, V3), pull: V3, angle: f64) -> Result<f64, String> {
    let u = cross(n0, n1);
    let uu = dot(u, u);
    if uu < 1e-18 || dot(unit3(u), pull).abs() > 1e-6 {
        return Err("a drafted face does not turn about a line across the pull".into());
    }
    if (libm::acos(dot(n0, n1).abs().min(1.0)) - angle).abs() > 1e-6 {
        return Err("a drafted face is not turned by the draft angle".into());
    }
    // The point of the line nearest the origin.
    let (d0, d1) = (dot(n0, p0), dot(n1, q1));
    let (c1, c0) = (cross(n1, u), cross(u, n0));
    let x = add3([c1[0] * d0, c1[1] * d0, c1[2] * d0], [c0[0] * d1, c0[1] * d1, c0[2] * d1]);
    Ok(dot(x, pull) / uu)
}

/// The two rims of a result-body face that carries the name's attributes, as (height along
/// `pull`, distance from the axis through `c` along `pull`) at its lowest and highest vertices
/// (mm). `None` unless the face has vertices at two heights, each at one distance.
fn named_result_rims(res: &Acis, (fid, a, b): (i64, i64, i64), c: V3, pull: V3) -> Option<((f64, f64), (f64, f64))> {
    let f = res.recs.iter().find_map(|r| {
        (r.ty.starts_with("NMx")
            && r.ints.get(3) == Some(&fid)
            && r.ints.get(5) == Some(&a)
            && r.ints.get(7) == Some(&b))
        .then(|| r.refs.iter().copied().find(|&y| res.is(y, "face")))
        .flatten()
    })?;
    let hr: Vec<(f64, f64)> = res
        .verts(f)
        .into_iter()
        .map(|v| {
            let v = [v[0] * CM, v[1] * CM, v[2] * CM];
            let q = sub3(v, c);
            let along = dot(q, pull);
            (dot(v, pull), libm::sqrt((dot(q, q) - along * along).max(0.0)))
        })
        .collect();
    let lo = hr.iter().map(|x| x.0).fold(f64::INFINITY, f64::min);
    let hi = hr.iter().map(|x| x.0).fold(f64::NEG_INFINITY, f64::max);
    if hi - lo <= 1e-6 || !hi.is_finite() || !lo.is_finite() {
        return None;
    }
    let rim = |h: f64| {
        let rs: Vec<f64> = hr.iter().filter(|x| (x.0 - h).abs() < 1e-6).map(|x| x.1).collect();
        let r0 = *rs.first()?;
        rs.iter().all(|r| (r - r0).abs() < 1e-6).then_some(r0)
    };
    Some(((lo, rim(lo)?), (hi, rim(hi)?)))
}

/// The plane (a point and the outward unit normal, mm) of a result-body face that carries the
/// name's attributes (feature id and its two numbers, `NMx_*` ints 3, 5 and 7).
fn named_result_plane(res: &Acis, (fid, a, b): (i64, i64, i64)) -> Option<(V3, V3)> {
    res.recs.iter().find_map(|r| {
        if !r.ty.starts_with("NMx")
            || r.ints.get(3) != Some(&fid)
            || r.ints.get(5) != Some(&a)
            || r.ints.get(7) != Some(&b)
        {
            return None;
        }
        let f = r.refs.iter().copied().find(|&y| res.is(y, "face"))?;
        let Surf::Plane { p, n } = res.kind(f) else { return None };
        // A reversed face (first flag) faces against its surface.
        let sense = if res.recs.get(usize::try_from(f).ok()?)?.flags.first() == Some(&0x0a) { -1.0 } else { 1.0 };
        let n = unit3(n);
        Some(([p[0] * CM, p[1] * CM, p[2] * CM], [n[0] * sense, n[1] * sense, n[2] * sense]))
    })
}

/// Points (mm) on the faces a shell opens, from the faces' names (see [`named_face`]).
fn named_openings(r: &Reader, st: &State, list: usize, why: &dyn Fn() -> String) -> Result<Vec<V3>, String> {
    let (items, _) = r.dc.first_list(list, None).ok_or_else(why)?;
    let mut out: Vec<V3> = Vec::new();
    for it in items {
        let nm = it.and_then(|x| face_name(r, x)).ok_or_else(why)?;
        let p = named_face(r, st, nm).map_err(|_| why())?.0;
        if !out.contains(&p) {
            out.push(p);
        }
    }
    Ok(out)
}

/// The distance from `p` to the nearest segment of the polylines.
pub(crate) fn polyline_dist(p: P2, lines: &[Vec<P2>]) -> f64 {
    let mut best = f64::INFINITY;
    for l in lines {
        for w in l.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            best = best.min(libm::hypot(p.0 - a.0 - t * dx, p.1 - a.1 - t * dy));
        }
    }
    best
}

/// A sweep (`90bada05`): slot 0 the profile loops (as an extrude's), slot 1 the path loop, slot 2
/// the operation, slot 3 the taper angle (only 0 is read). The path is a wire body of the
/// history blob; the curves of the path sketch that lie on it (ends and middle within 0.001 mm,
/// or the sampling's chord error for arcs) are the path.
fn sweep(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let lst = s.need(0)?;
    let sk = r.profile_sketch(lst).ok_or("no profile sketch found")?;
    let lp = s.need(1)?;
    // A path of a `fc203f47` record names the history body of its wire (`cc0f7521`).
    let tag = r.loop_tag(lp).or_else(|| {
        let c =
            r.dc.refs(lp).into_iter().map(|(_, y)| y).find(|&y| r.dc.class(y) == super::inventor_dc::G_LOOP_BODY)?;
        loop_body_tag(r, c)
    });
    // The path's sketch, or the one sketch whose curves the path wire runs along.
    let pk = match r.path_sketch(lp) {
        Some(pk) => pk,
        None => {
            let tag = tag.ok_or("no path sketch found")?;
            let ac = r.history().ok_or("the history blob is not readable")?;
            let wire = ac.loops_of_body(tag).ok_or("the path body is not readable")?;
            let all: Vec<usize> = (0..r.dc.n()).filter(|&i| r.dc.role(i) == Role::Sketch && i != sk).collect();
            sketch_holding(r, &wire, &all, true).map_err(|w| format!("the path {w}"))?.ok_or("no path sketch found")?.0
        }
    };
    if pk == sk {
        return Err("the path lies in the profile's sketch".into());
    }
    if r.pval(&s, 3).is_ok_and(|t| t != 0.0) {
        return Err("a tapered sweep is not read".into());
    }
    let profile = r.sketch(sk)?;
    let path = r.sketch(pk)?;
    let ac = r.history().ok_or("the history blob is not readable")?;
    let tag = tag.ok_or("the path loop has no body")?;
    let wire = ac.loops_of_body(tag).ok_or("the path body is not readable")?;
    let (o, x, y) =
        ([path.m[3], path.m[7], path.m[11]], [path.m[0], path.m[4], path.m[8]], [path.m[1], path.m[5], path.m[9]]);
    let n = unit3(cross(x, y));
    let mut l2: Vec<Vec<P2>> = Vec::new();
    for l in wire {
        let mut out = Vec::new();
        for p in l {
            let q = sub3(p, o);
            if dot(q, n).abs() * CM > 1e-6 {
                return Err("the path leaves its sketch's plane".into());
            }
            out.push((dot(q, x) * CM, dot(q, y) * CM));
        }
        l2.push(out);
    }
    let pts: HashMap<usize, P2> = path.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect();
    let mut ids = Vec::new();
    for c in &path.curves {
        let (id, samples, tol) = match *c {
            Curve::Line { id, a, b, con: false } => {
                let (Some(&a), Some(&b)) = (pts.get(&a), pts.get(&b)) else { continue };
                (format!("l{id}"), vec![a, b, ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)], 1e-3)
            }
            Curve::Arc { id, c, s, e, con: false } => {
                let (Some(&c), Some(&s), Some(&e)) = (pts.get(&c), pts.get(&s), pts.get(&e)) else { continue };
                let rad = libm::hypot(s.0 - c.0, s.1 - c.1);
                let (a0, mut a1) = (libm::atan2(s.1 - c.1, s.0 - c.0), libm::atan2(e.1 - c.1, e.0 - c.0));
                if a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                let m = (a0 + a1) / 2.0;
                let mid = (c.0 + rad * libm::cos(m), c.1 + rad * libm::sin(m));
                (format!("a{id}"), vec![s, e, mid], 1e-3_f64.max(rad * 3e-3))
            }
            Curve::Circle { id, c, r: rad, con: false } => {
                let Some(&c) = pts.get(&c) else { continue };
                let s = vec![(c.0 + rad, c.1), (c.0 - rad, c.1), (c.0, c.1 + rad), (c.0, c.1 - rad)];
                (format!("c{id}"), s, 1e-3_f64.max(rad * 3e-3))
            }
            _ => continue,
        };
        if samples.iter().all(|&p| polyline_dist(p, &l2) <= tol) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err("no curve of the path sketch lies on the path".into());
    }
    let mut op = match s.get(2).and_then(|i| tail_u16(&r.dc, i)) {
        Some(2) => "cut",
        Some(1) => "new",
        Some(4) => "intersect",
        _ => "join",
    };
    if st.first_body.is_some() && op == "new" {
        op = "join";
    }
    if st.first_body.is_none() && op == "join" {
        op = "new";
    }
    let mut j = json!({"type": "sweep", "name": name, "sketch": profile.name, "path": path.name,
                       "path_curves": ids, "operation": op});
    if op != "new" {
        let fb = st.first_body.clone().ok_or("a cut or join before any body")?;
        put(&mut j, "target", json!(fb));
    }
    for (k, sketch) in [(sk, profile), (pk, path)] {
        if !st.done.contains_key(&k) {
            st.out.push(sketch.json());
            st.done.insert(k, ());
        }
    }
    st.out.push(j);
    if st.first_body.is_none() {
        st.first_body = Some(name.to_string());
    }
    Ok(())
}

/// A coil (`b41914da`; notes AI): the profile screwed along an axis. Slot 0 the operation (as a
/// revolve's), 1 the profile loops, 2 the axis (as a revolve's), 3 flips the axis, 4 the hand
/// (1 right), 5 the kind (byte 2: 0 pitch and turns, 1 turns and height, 2 pitch and height,
/// 3 a spiral), 6 the pitch, 7 the height (cm), 8 the turns, 9 the taper, 10 and 13 the start
/// and end (last byte 0: natural; flat ends are not read). A helical sweep of the profile that
/// starts at the profile and climbs the axis.
fn coil(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let lst = s.need(1)?;
    let sk = r.profile_sketch(lst).ok_or("no sketch found")?;
    let sketch = r.sketch(sk)?;
    let last = |j: usize| s.get(j).and_then(|i| r.dc.r(i).last().copied());
    if [10, 13].iter().any(|&j| last(j) != Some(0)) {
        return Err("a coil with flat ends is not read".into());
    }
    if r.pval(&s, 9)? != 0.0 {
        return Err("a tapered coil is not read".into());
    }
    let kind = s.get(5).and_then(|i| r.dc.r(i).get(r.dc.body(i) + 2).copied());
    let (pitch, turns, height) = (r.pval(&s, 6)? * CM, r.pval(&s, 8)?, r.pval(&s, 7)? * CM);
    let (pitch, height) = match kind {
        Some(0) => (pitch, pitch * turns),
        Some(1) if turns > 0.0 => (height / turns, height),
        Some(2) => (pitch, height),
        k => return Err(format!("coil kind {k:?}")),
    };
    if !(pitch > 0.0 && height > 0.0 && pitch.is_finite() && height.is_finite()) {
        return Err("the coil has no pitch or height".into());
    }
    let (p0, mut v) = work_axis(&r.dc, s.need(2)?).map_err(|w| format!("the coil {}", w.trim_start_matches("the ")))?;
    if last(3) == Some(1) {
        v = [-v[0], -v[1], -v[2]];
    }
    let p = [p0[0] * CM, p0[1] * CM, p0[2] * CM];
    let mut op = match s.get(0).and_then(|i| tail_u16(&r.dc, i)) {
        Some(2) => "cut",
        Some(1) => "new",
        Some(4) => "intersect",
        _ => "join",
    };
    if st.first_body.is_some() && op == "new" {
        op = "join";
    }
    if st.first_body.is_none() && op == "join" {
        op = "new";
    }
    // The radius only describes the helix: the document takes it from the profile.
    let pts: HashMap<usize, P2> = sketch.points.iter().map(|&(i, u, w, _)| (i, (u, w))).collect();
    let (o, x, y) = frame(&sketch.m);
    let mut radius = 0.0f64;
    for (u, w) in pts.values() {
        let q = sub3([o[0] + u * x[0] + w * y[0], o[1] + u * x[1] + w * y[1], o[2] + u * x[2] + w * y[2]], p);
        let c = cross(q, v);
        radius = radius.max(libm::sqrt(dot(c, c)));
    }
    let mut helix = json!({"origin": arr(p), "axis": arr(v), "radius": rnd(radius.max(1e-3)),
                           "pitch": rnd(pitch), "height": rnd(height),
                           "left_handed": last(4) != Some(1)});
    // The document sweeps a section through the axis; a section across it (a twisted
    // extrusion) is not that.
    if dot(v, unit3(cross(x, y))).abs() > 1e-9 {
        return Err("the coil's profile does not lie along its axis".into());
    }
    let world = (0..3).find(|&k| (v.get(k).copied().unwrap_or(0.0) - 1.0).abs() < 1e-9);
    let on_world = world.is_some_and(|k| p.iter().enumerate().all(|(i, c)| i == k || c.abs() < 1e-6));
    let mut axis_line = None;
    if !on_world {
        let l = sketch_line_along(&sketch, p, v, f.rec).ok_or("the coil axis is not in the profile's plane")?;
        put(&mut helix, "line", json!(l.2.clone()));
        axis_line = Some(l);
    }
    let mut j = json!({"type": "sweep", "name": name, "sketch": sketch.name, "helix": helix, "operation": op});
    if op != "new" {
        let fb = st.first_body.clone().ok_or("a cut or join before any body")?;
        put(&mut j, "target", json!(fb));
    }
    if !st.done.contains_key(&sk) {
        st.out.push(sketch.json());
        st.done.insert(sk, ());
    }
    if let Some((lp, line, _)) = axis_line {
        let entry = st.out.iter_mut().rev().find(|o| {
            o.get("type").and_then(Value::as_str) == Some("sketch")
                && o.get("name").and_then(Value::as_str) == Some(&sketch.name)
        });
        let Some(Value::Object(o)) = entry else { return Err("the profile sketch is not out".into()) };
        if let Some(Value::Array(a)) = o.get_mut("points") {
            a.extend(lp);
        }
        if let Some(Value::Array(c)) = o.get_mut("curves") {
            c.push(line);
        }
    }
    st.out.push(j);
    if st.first_body.is_none() {
        st.first_body = Some(name.to_string());
    }
    Ok(())
}

/// An emboss or engrave (`285e398c`; notes AJ): slot 0 the kind (3 emboss from the face, 2
/// engrave from the face), 1 the profile loops, 5 the depth (cm), 6 the taper, 9 the face a
/// wrapped emboss wraps onto. The profile is extruded from its sketch: the reach and its side
/// are measured on the feature's faces in the result body (as for an extrude to a face) and
/// must match the depth.
fn emboss(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let lst = s.need(1)?;
    let sk = r.profile_sketch(lst).ok_or("no sketch found")?;
    let sketch = r.sketch(sk)?;
    let op = match s.get(0).and_then(|i| tail_u16(&r.dc, i)) {
        Some(3) => "join",
        Some(2) => "cut",
        k => return Err(format!("emboss kind {k:?}")),
    };
    if s.get(9).is_some() {
        return Err("a wrapped emboss is not read".into());
    }
    if r.pval(&s, 6)? != 0.0 {
        return Err("a tapered emboss is not read".into());
    }
    let depth = r.pval(&s, 5)? * CM;
    let dir = s.get(2).and_then(|i| {
        let rec = r.dc.r(i);
        let k = rec.len().checked_sub(24)?;
        Some([f64_at(rec, k)?, f64_at(rec, k + 8)?, f64_at(rec, k + 16)?])
    });
    let dir = dir.ok_or("the emboss has no direction")?;
    let (_, x, y) = frame(&sketch.m);
    let along = dot(dir, unit3(cross(x, y)));
    if (along.abs() - 1.0).abs() > 1e-9 {
        return Err("the emboss direction is not across its sketch".into());
    }
    // The direction points into the material: an emboss rises against it, an engrave cuts
    // along it (slot 3 does not change the side of one from the face).
    let d = depth * along.signum() * if op == "join" { -1.0 } else { 1.0 };
    let fb = st.first_body.clone().ok_or("a cut or join before any body")?;
    let j = json!({"type": "extrude", "name": name, "sketch": sketch.name, "profiles": r.profiles(&sketch, lst),
                   "operation": op, "target": fb, "distance": rnd(d), "symmetric": false});
    if !st.done.contains_key(&sk) {
        st.out.push(sketch.json());
        st.done.insert(sk, ());
    }
    st.out.push(j);
    Ok(())
}

/// A whole-body transform (note AQ): Move Bodies (`2b839fe9`), or a direct edit Move
/// (`8fa6a6de`) or Scale (`5bcb6fff`) of bodies. Slot 0 lists the bodies (`0e6870ae`), slot 1
/// (`0800fe29`) points at the matrix (cm) of the move or scale ([`transform_steps`]).
fn body_transform(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let list = s.need(0)?;
    if r.dc.class(list) != G_BODY_LIST {
        return Err("the moved bodies are not a body list (a move of faces is not read)".into());
    }
    let recs: Vec<usize> =
        r.dc.refs(list).into_iter().map(|(_, x)| x).filter(|&x| r.dc.class(x) == G_SOLID_BODY).collect();
    let pl = r.dc.refs(s.need(1)?).into_iter().map(|(_, x)| x).find(|&x| r.dc.role(x) == Role::SkPlacement);
    let m = pl.and_then(|p| r.dc.placement(p)).ok_or("the transform is not readable")?;
    let steps = transform_steps(&m, f.typ == Some(FeatType::Other(0x5bcb_6fff)))?;
    let mut targets = Vec::new();
    for b in &recs {
        let t = match st.bodies.get(b) {
            Some(t) => t.clone(),
            // A body no feature read named: the part's body, when it is the only one.
            None if recs.len() == 1 && st.bodies.values().all(|v| Some(v) == st.first_body.as_ref()) => {
                st.first_body.clone().ok_or("a move before any body")?
            }
            None => return Err("a moved body was not made by a feature read".into()),
        };
        targets.push(t);
    }
    if targets.is_empty() {
        return Err("the transform moves no solid body".into());
    }
    let mut k = 0;
    for t in &targets {
        for step in &steps {
            k += 1;
            let nm = if k == 1 { name.to_string() } else { format!("{name}_{k}") };
            let mut j = step.clone();
            put(&mut j, "name", json!(nm));
            put(&mut j, "target", json!(t));
            st.out.push(j);
        }
    }
    Ok(())
}

/// The tree features (without name and target) that apply a transform's matrix (row-major,
/// entries 3, 7 and 11 in cm) to a body. A move's matrix is a turn and a shift: a move (the turn
/// about the origin, then the shift). A scale's (`scale`) is the factor on the diagonal (one, or
/// one per axis) and the base point the scale keeps in the shift's place: a scale about it
/// (the de_scale sample: the base point is the scaled block's top face centre). A scale that
/// also turns is not read.
pub(crate) fn transform_steps(m: &[f64; 16], scale: bool) -> Result<Vec<Value>, String> {
    let g = |i: usize| m.get(i).copied().unwrap_or(f64::NAN);
    if !(0..16).all(|i| g(i).is_finite()) {
        return Err("the transform is not finite".into());
    }
    if [g(12), g(13), g(14)].iter().any(|v| v.abs() > 1e-9) || (g(15) - 1.0).abs() > 1e-9 {
        return Err("the transform is not affine".into());
    }
    let a = [[g(0), g(1), g(2)], [g(4), g(5), g(6)], [g(8), g(9), g(10)]];
    // Row i, column j of the turn and scale part.
    let e = |i: usize, j: usize| g(4 * i + j);
    let t = [g(3) * CM, g(7) * CM, g(11) * CM];
    if t.iter().any(|v| v.abs() > 1e7) {
        return Err("the transform reaches too far".into());
    }
    if scale {
        let off = (0..3).any(|i| (0..3).any(|j| i != j && e(i, j).abs() > 1e-12));
        if off {
            return Err("a scale that also turns is not read".into());
        }
        let k = [e(0, 0), e(1, 1), e(2, 2)];
        if k.iter().any(|&x| !(x > 1e-9 && x < 1e6)) {
            return Err("the scale factor is not positive".into());
        }
        let near = |x: f64, y: f64| (x - y).abs() <= 1e-12 * x.abs().max(y.abs()).max(1.0);
        let mut o = json!({"type": "scale_body", "center": arr(t)});
        if near(k[0], k[1]) && near(k[1], k[2]) {
            put(&mut o, "factor", json!(rnd(k[0])));
        } else {
            put(&mut o, "factors", json!([rnd(k[0]), rnd(k[1]), rnd(k[2])]));
        }
        return Ok(vec![o]);
    }
    let col = |j: usize| [e(0, j), e(1, j), e(2, j)];
    let ortho = (0..3).all(|i| (0..3).all(|j| (dot(col(i), col(j)) - if i == j { 1.0 } else { 0.0 }).abs() <= 1e-9));
    if !ortho || dot(cross(col(0), col(1)), col(2)) <= 0.0 {
        return Err("the move scales, shears or mirrors the body".into());
    }
    let ident = (0..3).all(|i| (0..3).all(|j| (e(i, j) - if i == j { 1.0 } else { 0.0 }).abs() <= 1e-12));
    let mut mv = json!({"type": "move_body"});
    if !ident {
        let (axis, angle) = axis_angle(&a).ok_or("the turn of the move is not readable")?;
        put(&mut mv, "rotate", json!({"axis": arr(axis), "origin": [0.0, 0.0, 0.0], "angle": rnd(angle)}));
    }
    if t.iter().any(|v| v.abs() > 1e-9) {
        put(&mut mv, "translate", arr(t));
    } else if ident {
        return Err("the move leaves the body where it is".into());
    }
    Ok(vec![mv])
}

/// The unit axis and angle (degrees, 0..=180) of a rotation matrix (rows).
fn axis_angle(m: &[[f64; 3]; 3]) -> Option<(V3, f64)> {
    let e = |i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let tr = e(0, 0) + e(1, 1) + e(2, 2);
    let angle = libm::acos(((tr - 1.0) / 2.0).clamp(-1.0, 1.0));
    let v = [e(2, 1) - e(1, 2), e(0, 2) - e(2, 0), e(1, 0) - e(0, 1)];
    let s = libm::sqrt(dot(v, v));
    let axis = if s > 1e-6 {
        [v[0] / s, v[1] / s, v[2] / s]
    } else {
        // Half a turn: the axis is the longest column of (m + I) / 2.
        let c = |j: usize| {
            [
                e(0, j) + if j == 0 { 1.0 } else { 0.0 },
                e(1, j) + if j == 1 { 1.0 } else { 0.0 },
                e(2, j) + if j == 2 { 1.0 } else { 0.0 },
            ]
        };
        let best = (0..3).map(c).max_by(|a, b| dot(*a, *a).total_cmp(&dot(*b, *b)))?;
        let l = libm::sqrt(dot(best, best));
        (l > 1e-9).then(|| [best[0] / l, best[1] / l, best[2] / l])?
    };
    Some((axis, angle.to_degrees()))
}

/// The solid body record (`90874d47`) a feature made or changed: the first one its
/// `0e6870ae` slot lists.
fn body_record(r: &Reader, f: &Feature) -> Option<usize> {
    body_records(r, f).first().copied()
}

/// The tree bodies a cut or join changes: those its body list names that features made (all
/// of them for a cut through several, else the first), or the first body.
fn changed_bodies(r: &Reader, st: &State, f: &Feature, many: bool) -> Result<Value, String> {
    let mut named: Vec<String> = Vec::new();
    for b in body_records(r, f) {
        if let Some(n) = st.bodies.get(&b).filter(|n| !named.contains(n)) {
            named.push(n.clone());
        }
    }
    match named.as_slice() {
        [] => Ok(json!(st.first_body.clone().ok_or("a cut or join before any body")?)),
        [one, ..] if !many || named.len() == 1 => Ok(json!(one)),
        _ => Ok(json!(named)),
    }
}

/// The solid bodies a feature's body list (`0e6870ae`) names.
fn body_records(r: &Reader, f: &Feature) -> Vec<usize> {
    let Some(s) = r.dc.slots(f.rec) else { return Vec::new() };
    let Some(l) = s.into_iter().flatten().find(|&i| r.dc.class(i) == G_BODY_LIST) else { return Vec::new() };
    r.dc.refs(l).into_iter().map(|(_, x)| x).filter(|&x| r.dc.class(x) == G_SOLID_BODY).collect()
}

/// The solid bodies a combine takes as tools (its slot 1).
fn combine_tools(r: &Reader, f: &Feature) -> Vec<usize> {
    let Some(Some(l)) = r.dc.slots(f.rec).and_then(|s| s.get(1).copied()) else { return Vec::new() };
    r.dc.refs(l).into_iter().map(|(_, x)| x).filter(|&x| r.dc.class(x) == G_SOLID_BODY).collect()
}

/// A combine (`c0236733`; notes AK): slot 0 the base body, 1 the tool bodies, 2 the operation
/// (2 cut, 3 join, 4 intersect), 3 keeps the tools (not read). The bodies are those the
/// features that made them were given.
fn combine(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let op = match s.get(2).and_then(|i| tail_u16(&r.dc, i)) {
        Some(2) => "cut",
        Some(3) => "join",
        Some(4) => "intersect",
        k => return Err(format!("combine operation {k:?}")),
    };
    if s.get(3).and_then(|i| r.dc.r(i).last().copied()) != Some(0) {
        return Err("a combine that keeps its tools is not read".into());
    }
    let named = |b: usize| st.bodies.get(&b).cloned().ok_or("a body of the combine was not made by a feature read");
    let base = r.dc.refs(s.need(0)?).into_iter().map(|(_, x)| x).find(|&x| r.dc.class(x) == G_SOLID_BODY);
    let target = named(base.ok_or("the combine has no base body")?)?;
    let mut tools = Vec::new();
    for b in combine_tools(r, f) {
        tools.push(named(b)?);
    }
    if tools.is_empty() {
        return Err("the combine has no tool body".into());
    }
    st.out.push(json!({"type": "combine", "name": name, "target": target, "tools": tools, "operation": op}));
    Ok(())
}

/// The reach (mm along the sketch normal) of an extrude that ends on a face, from the plane
/// of that face its slot 18 keeps (`151280f0`, the last 72 bytes as a work plane's; note AS);
/// slot 17 keeps the face an extrude starts from in the same way.
/// `None` when the slot holds no plane or the plane is not parallel to the sketch.
fn end_plane_reach(r: &Reader, s: &Slots, slot: usize, sk: &Sketch) -> Option<f64> {
    let i = s.get(slot).filter(|&i| r.dc.class(i) == 0x1512_80f0)?;
    let (p, pn) = work_plane(&r.dc, i).ok()?;
    let (o, x, y) = frame(&sk.m);
    let n = unit3(cross(x, y));
    if dot(pn, n).abs() < 1.0 - 1e-9 {
        return None;
    }
    let d = dot(sub3([p[0] * CM, p[1] * CM, p[2] * CM], o), n);
    (d.abs() > 1e-6).then_some(d)
}

/// A work plane record (`ce52df42`): a point (cm) and two unit vectors, the last 72 bytes;
/// the point and the normal.
fn work_plane(dc: &Dc, pl: usize) -> Result<(V3, V3), String> {
    let rec = dc.r(pl);
    let k = rec.len().checked_sub(72).ok_or("plane is cut short")?;
    let g = |j: usize| f64_at(rec, k + 8 * j).ok_or("plane is cut short");
    let o = [g(0)?, g(1)?, g(2)?];
    let (u, w) = ([g(3)?, g(4)?, g(5)?], [g(6)?, g(7)?, g(8)?]);
    let unit = |a: V3| (dot(a, a) - 1.0).abs() < 1e-6;
    if !(unit(u) && unit(w) && dot(u, w).abs() < 1e-6 && o.iter().all(|c| c.is_finite() && c.abs() < 1e6)) {
        return Err("plane is not readable".into());
    }
    Ok((o, unit3(cross(u, w))))
}

/// A split (`8371a8ba`; notes AL): slot 0 the kind (1 trims the part, 2 splits faces, 3
/// splits the body in two), 5 the work plane it splits at, 6 the side a trim removes. Split
/// faces change no shape and are left out; a trim is a split at the plane and the removal of
/// one side; a split body keeps both.
fn split(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    const G_PLANE: u32 = 0xce52_df42;
    let s = r.slots(f)?;
    let kind = s.get(0).and_then(|i| tail_u16(&r.dc, i));
    if kind == Some(2) {
        st.omitted.push(json!({"name": name, "why": "split faces (no change of shape)"}));
        return Ok(());
    }
    let pl = s.need(5)?;
    if r.dc.class(pl) != G_PLANE {
        return Err("the split tool is not a work plane".into());
    }
    let (o, n) = work_plane(&r.dc, pl).map_err(|w| format!("the split {w}"))?;
    let fb = st.first_body.clone().ok_or("a split before any body")?;
    let plane = json!({"origin": [rnd(o[0] * CM), rnd(o[1] * CM), rnd(o[2] * CM)], "normal": [rnd(n[0]), rnd(n[1]), rnd(n[2])]});
    match kind {
        Some(1) => {
            // The new body is the side the normal points to; slot 6 clear removes it, set
            // keeps it.
            let other = s.get(6).and_then(|i| r.dc.r(i).last().copied()) != Some(1);
            st.out.push(json!({"type": "split", "name": name, "target": fb, "plane": plane}));
            if other {
                st.out.push(json!({"type": "remove_body", "name": format!("{name}_removed"), "target": name}));
            } else {
                st.out.push(json!({"type": "remove_body", "name": format!("{name}_removed"), "target": fb}));
                st.first_body = Some(name.to_string());
            }
            Ok(())
        }
        k => Err(format!("split kind {k:?}")),
    }
}

/// The sketch whose curves a loop of the history blob runs along, given the record that names
/// the loop's history body (`cc0f7521`), and the regions the loop surrounds in it. Several
/// sketches may share a plane: the loop's points must lie on one sketch's curves.
/// A sketch record, the sketch, and loops in its coordinates (mm).
type Held = (usize, Sketch, Vec<Vec<P2>>);

/// The one sketch among `cands` whose plane holds the loops (file units) and whose curves they
/// run along, with the loops in its coordinates (mm); with `first`, the first such sketch (any
/// one gives the same curves). `None` when none does.
fn sketch_holding(r: &Reader, loops: &[Vec<V3>], cands: &[usize], first: bool) -> Result<Option<Held>, String> {
    let mut found: Option<Held> = None;
    for &k in cands {
        // A sketch that does not read holds no loop.
        let Ok(sk) = r.sketch(k) else { continue };
        let (o, x, y) = frame(&sk.m);
        let n = unit3(cross(x, y));
        let polys = sk.polys();
        let mut l2 = Vec::new();
        let mut on = true;
        for l in loops {
            let mut out = Vec::new();
            for p in l {
                let q = sub3([p[0] * CM, p[1] * CM, p[2] * CM], o);
                let uv = (dot(q, x), dot(q, y));
                if dot(q, n).abs() > 1e-6 || curve_dist(uv, &polys) > 0.02 {
                    on = false;
                    break;
                }
                out.push(uv);
            }
            if !on {
                break;
            }
            l2.push(out);
        }
        if on {
            if found.is_some() {
                return Err("runs along several sketches".into());
            }
            found = Some((k, sk, l2));
            if first {
                break;
            }
        }
    }
    Ok(found)
}

/// The history-body tag a loop body record (`cc0f7521`) names.
fn loop_body_tag(r: &Reader, c: usize) -> Option<i64> {
    let hb = r.dc.refs(c).into_iter().find(|&(_, y)| r.dc.role(y) == Role::HistoryBlob)?.0;
    u32_at(r.dc.r(c), hb + 4).map(i64::from)
}

fn loop_sketch(r: &Reader, c: usize) -> Result<(usize, Sketch, Value), String> {
    let ac = r.history().ok_or("the history blob is not readable")?;
    let tag = loop_body_tag(r, c).ok_or("no history body")?;
    let loops = ac.loops_of_body(tag).ok_or("the profile's loop is not readable")?;
    if loops.iter().all(Vec::is_empty) {
        return Err("the profile's loop has no points".into());
    }
    let mut cands: Vec<usize> = Vec::new();
    for j in 0..r.dc.n() {
        if r.dc.class(j) != super::inventor_dc::G_PROFILE {
            continue;
        }
        let Some((_, k)) = r.dc.refs(j).into_iter().find(|&(_, x)| r.dc.role(x) == Role::Sketch) else { continue };
        if !cands.contains(&k) {
            cands.push(k);
        }
    }
    let found = sketch_holding(r, &loops, &cands, false).map_err(|w| format!("the profile's loop {w}"))?;
    let (k, sk, l2) = found.ok_or("no sketch holds the profile's loop")?;
    let profiles = match picks(&sk.polys(), &[l2]) {
        Some(pk) if !pk.is_empty() => Value::Array(pk.into_iter().map(|(a, b)| json!({"point": [a, b]})).collect()),
        _ => return Err("the profile's regions are not found".into()),
    };
    Ok((k, sk, profiles))
}

/// The distance from `p` to the nearest sketch curve: exact for lines, circles and arcs (the
/// profile picks' sampling leaves 0.09 mm between the chords and an arc of 160 mm radius),
/// along the sampled curve otherwise. For matching loops to the curves they run along.
pub(crate) fn curve_dist(p: P2, polys: &[Poly]) -> f64 {
    let tau = 2.0 * std::f64::consts::PI;
    let d = |a: P2, b: P2| libm::hypot(a.0 - b.0, a.1 - b.1);
    polys
        .iter()
        .map(|c| match *c {
            Poly::Circle(c, r) => (d(p, c) - r).abs(),
            Poly::Arc { c, s, e } => {
                let r = d(s, c);
                let a0 = libm::atan2(s.1 - c.1, s.0 - c.0);
                let mut a1 = libm::atan2(e.1 - c.1, e.0 - c.0);
                while a1 <= a0 + 1e-12 {
                    a1 += tau;
                }
                let mut a = libm::atan2(p.1 - c.1, p.0 - c.0);
                while a < a0 {
                    a += tau;
                }
                if a <= a1 { (d(p, c) - r).abs() } else { d(p, s).min(d(p, e)) }
            }
            ref other => polyline_dist(p, &[sample(other)]),
        })
        .fold(f64::INFINITY, f64::min)
}

/// A sheet-metal face (`55585fc6`; notes AM): slot 0 names the profile's loop body, 1 a
/// direction, 3 the thickness (cm), 16 the operation. An extrude of the profile by the
/// thickness, on the side its faces lie in the result body.
fn plate(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let c =
        r.dc.refs(s.need(0)?)
            .into_iter()
            .map(|(_, y)| y)
            .find(|&y| r.dc.class(y) == super::inventor_dc::G_LOOP_BODY)
            .ok_or("the face has no profile loop")?;
    let (k, sketch, profiles) = loop_sketch(r, c)?;
    let t = r.pval(&s, 3)? * CM;
    let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
    // Slot 2 (`5e50b969`) is the side the face grows to: 0 along the sketch normal, 1 against
    // it, 2 both ways (note AS). Older files without it: the side its faces lie on in the result.
    let side = s.get(2).filter(|&i| r.dc.class(i) == 0x5e50_b969).and_then(|i| tail_u16(&r.dc, i));
    let (d, symmetric) = match side {
        Some(0) => (t, false),
        Some(1) => (-t, false),
        Some(2) => (t, true),
        _ => {
            let d = r.swept_distance(&sketch, fid)?;
            if (d.abs() - t).abs() > 1e-6 * t.max(1.0) {
                return Err(format!("the face reaches {} mm, its thickness is {} mm", rnd(d.abs()), rnd(t)));
            }
            (d, false)
        }
    };
    let mut op = match s.get(16).and_then(|i| tail_u16(&r.dc, i)) {
        Some(2) => "cut",
        Some(1) => "new",
        _ => "join",
    };
    if st.first_body.is_some() && op == "new" {
        op = "join";
    }
    if st.first_body.is_none() && op == "join" {
        op = "new";
    }
    let (o, x, y) = frame(&sketch.m);
    let span = if symmetric { (-d.abs() / 2.0, d.abs() / 2.0) } else { (d.min(0.0), d.max(0.0)) };
    st.sheets.push((o, unit3(cross(x, y)), span.0, span.1));
    let mut j = json!({"type": "extrude", "name": name, "sketch": sketch.name, "profiles": profiles,
                       "operation": op, "distance": rnd(d), "symmetric": symmetric});
    if op != "new" {
        let fb = st.first_body.clone().ok_or("a cut or join before any body")?;
        put(&mut j, "target", json!(fb));
    }
    if !st.done.contains_key(&k) {
        st.out.push(sketch.json());
        st.done.insert(k, ());
    }
    st.out.push(j);
    if st.first_body.is_none() {
        st.first_body = Some(name.to_string());
    }
    Ok(())
}

/// A loft (`2f54efe0`; note AS): slot 0 lists its sections (`4b3150e8`), each naming the
/// history body of its loop (`cc0f7521`) as a sheet-metal face does; slot 1 the operation. A
/// section is the sketch whose curves its loop runs along; two sections are joined by
/// straight lines.
fn loft(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    const G_SECTION: u32 = 0x4b31_50e8;
    let s = r.slots(f)?;
    let (items, _) = r.dc.first_list(s.need(0)?, None).ok_or("the loft has no section list")?;
    let mut names = Vec::new();
    let mut used = Vec::new();
    for it in items {
        let sec = it.ok_or("a loft section is empty")?;
        if r.dc.class(sec) != G_SECTION {
            return Err("a loft section is not a profile".into());
        }
        let c =
            r.dc.refs(sec)
                .into_iter()
                .map(|(_, y)| y)
                .find(|&y| r.dc.class(y) == super::inventor_dc::G_LOOP_BODY)
                .ok_or("a loft section has no loop")?;
        let (k, sk, _) = loop_sketch(r, c).map_err(|w| format!("a loft section: {w}"))?;
        names.push(sk.name.clone());
        if let std::collections::hash_map::Entry::Vacant(e) = st.done.entry(k) {
            used.push(sk.json());
            e.insert(());
        }
    }
    if names.len() < 2 {
        return Err("a loft takes two sections or more".into());
    }
    let mut op = match s.get(1).and_then(|i| tail_u16(&r.dc, i)) {
        Some(2) => "cut",
        Some(1) => "new",
        Some(4) => "intersect",
        _ => "join",
    };
    if st.first_body.is_some() && op == "new" {
        op = "join";
    }
    if st.first_body.is_none() && op == "join" {
        op = "new";
    }
    let mut j = json!({"type": "loft", "name": name, "sketches": names, "ruled": names.len() == 2, "operation": op});
    if op != "new" {
        let fb = st.first_body.clone().ok_or("a cut or join before any body")?;
        put(&mut j, "target", json!(fb));
    }
    st.out.extend(used);
    st.out.push(j);
    if st.first_body.is_none() {
        st.first_body = Some(name.to_string());
    }
    Ok(())
}

/// The K-factor of the folds: Inventor's default unfold rule's (every fold of the corpus has its
/// bend zone where 0.44 places it).
const K_FACTOR: f64 = 0.44;

/// A sheet-metal fold (`4c37ffe8`): slot 3 the angle (radians), 4 the bend radius (cm), 5 the
/// bend line (a `833d1b91` record whose `8ef06c89` line a `93c7ee68` record ties to a sketch
/// line), 1 and 2 the side that moves and the direction, slot 0's third body byte the place of
/// the bend zone against the line.
fn fold(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let angle = r.pval(&s, 3)?;
    let radius = r.pval(&s, 4)? * CM;
    let edge =
        r.dc.refs(s.need(5)?)
            .into_iter()
            .map(|(_, y)| y)
            .find(|&y| r.dc.class(y) == 0x8ef0_6c89)
            .ok_or("the fold has no bend line")?;
    let ln = r
        .back()
        .get(edge)
        .into_iter()
        .flatten()
        .copied()
        .filter(|&b| r.dc.class(b) == 0x93c7_ee68)
        .find_map(|b| r.dc.refs(b).into_iter().map(|(_, y)| y).find(|&y| r.dc.role(y) == Role::SkLine))
        .ok_or("the bend line is not a sketch line")?;
    let k = r.owner(ln).ok_or("the bend line has no sketch")?;
    let sk = r.sketch(k)?;
    let pts: HashMap<usize, (f64, f64)> = sk.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect();
    let (a, b) = sk
        .curves
        .iter()
        .find_map(|c| match *c {
            Curve::Line { id, a, b, .. } if id == ln => Some((pts.get(&a).copied()?, pts.get(&b).copied()?)),
            _ => None,
        })
        .ok_or("the bend line is not in its sketch")?;
    let (o, x, y) = frame(&sk.m);
    let n = unit3(cross(x, y));
    let w = |p: (f64, f64)| add3(o, add3([x[0] * p.0, x[1] * p.0, x[2] * p.0], [y[0] * p.1, y[1] * p.1, y[2] * p.1]));
    let (pa, pb) = (w(a), w(b));
    let u = unit3(sub3(pb, pa));
    // The flat stretch of sheet the sketch lies on (a plate, or the far side of an earlier
    // fold), and the side of the sketch plane its material lies on: `m` is 1 along the sketch
    // normal.
    let (slab, m) = st
        .sheets
        .iter()
        .rev()
        .find_map(|&(po, pn, lo, hi)| {
            let c = dot(pn, n);
            if (c.abs() - 1.0).abs() > 1e-9 {
                return None;
            }
            let e = dot(sub3(o, po), pn);
            if (e - lo).abs() < 1e-6 {
                Some(((po, pn, lo, hi), c.signum()))
            } else if (e - hi).abs() < 1e-6 {
                Some(((po, pn, lo, hi), -c.signum()))
            } else {
                None
            }
        })
        .ok_or("the fold's sketch is not on a face of a flat stretch of the sheet")?;
    // Slot 1: 1 folds away from the material (along the face's outward normal), 0 into it.
    // Slot 2: 0 moves the material left of the line seen from outside the face, 1 the right.
    let flag = |j: usize| s.get(j).and_then(|i| r.dc.r(i).last().copied());
    let (Some(away @ 0..=1), Some(right @ 0..=1)) = (flag(1), flag(2)) else {
        return Err("the fold's side or direction is not read".into());
    };
    let out = [-m * n[0], -m * n[1], -m * n[2]];
    let left_out = cross(out, u);
    let moving = if right == 1 { [-left_out[0], -left_out[1], -left_out[2]] } else { left_out };
    let mid = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0, (pa[2] + pb[2]) / 2.0];
    let keep = sub3(mid, moving);
    let d = if away == 1 { out } else { [-out[0], -out[1], -out[2]] };
    let up = dot(d, n);
    // Slot 0's third body byte: 0 the zone centred on the line, 1 starting at it, 2 ending at it.
    let s0 = s.get(0).map(|i| r.dc.r(i)).unwrap_or(&[]);
    let (po, pn, lo, hi) = slab;
    let t = hi - lo;
    let zone = angle * (radius + K_FACTOR * t);
    let (position, fixed_end) = match s0.get(s0.len().saturating_sub(2)) {
        Some(0) => ("center", -zone / 2.0),
        Some(1) => ("start", 0.0),
        Some(2) => ("end", -zone),
        _ => return Err("the fold's position is not read".into()),
    };
    // The folded stretch is the slab turned about the bend's axis (`radius` beyond the inner
    // face, over the end of the material that stays): later folds may be sketched on it.
    let inner = if dot(d, pn) > 0.0 { hi + radius } else { lo - radius };
    let along = inner - dot(sub3(mid, po), pn);
    let axis_at = add3(
        add3(mid, [pn[0] * along, pn[1] * along, pn[2] * along]),
        [moving[0] * fixed_end, moving[1] * fixed_end, moving[2] * fixed_end],
    );
    let k_ax = unit3(cross(moving, d));
    let po2 = add3(axis_at, rotate(sub3(po, axis_at), k_ax, angle));
    st.sheets.push((po2, rotate(pn, k_ax, angle), lo, hi));
    let target = st.first_body.clone().ok_or("a fold before any body")?;
    if !st.done.contains_key(&k) {
        st.out.push(sk.json());
        st.done.insert(k, ());
    }
    st.out.push(json!({"type": "bend", "name": name, "target": target, "sketch": sk.name,
                       "line": format!("l{ln}"), "angle": rnd(angle.to_degrees() * up.signum()),
                       "bend_radius": rnd(radius), "k_factor": K_FACTOR, "position": position,
                       "keep_point": arr(keep)}));
    Ok(())
}

/// The name of a face a feature picked (a `509fb5cc` record): the naming record (`03aa812c`) that
/// refers to it names an entry of a naming table (`cce92042`, by index), whose `0645c2a5`
/// record holds the id of the feature that made the face and the two numbers its result-body
/// attributes carry (`NMx_*` ints 5 and 7: the profile curve, and 4 start cap, 12 end cap, 20
/// side). `None` when the chain is not there.
fn face_name(r: &Reader, x: usize) -> Option<(i64, i64, i64)> {
    const G_NAMING: u32 = 0x03aa_812c;
    const G_TABLE: u32 = 0xcce9_2042;
    const G_FACE_NAME: u32 = 0x0645_c2a5;
    let nm = r.back().get(x)?.iter().copied().find(|&b| r.dc.class(b) == G_NAMING)?;
    let (off, table) = r.dc.refs(nm).into_iter().find(|&(_, y)| r.dc.class(y) == G_TABLE)?;
    let idx = u32_at(r.dc.r(nm), off + 4)?;
    let (_, y) = r.dc.refs(table).into_iter().find(|&(o, _)| o >= 4 && u32_at(r.dc.r(table), o - 4) == Some(idx))?;
    if r.dc.class(y) != G_FACE_NAME {
        return None;
    }
    let rec = r.dc.r(y);
    let m = rec.windows(4).position(|w| w == [0xff, 0xff, 0xff, 0xff])?;
    let fid = u32_at(rec, m.checked_sub(4)?)?;
    let (a, b) = (u32_at(rec, m + 12)?, u32_at(rec, m + 16)?);
    Some((i64::from(fid), i64::from(a), i64::from(b)))
}

fn unsupported(name: &str, what: &str, reason: Option<String>) -> Value {
    let mut v = json!({"type": "unsupported", "name": name, "original_type": what});
    if let Some(r) = reason {
        put(&mut v, "reason", json!(r));
    }
    v
}

/// The rounds of slot ends (concave full rounds: the kernel's full round does the convex ones).
/// The round fills the two corners between the slot's end wall and the half cylinder with
/// material, so each is a join of the two slivers, drawn in a sketch across the cylinder's axis
/// (x towards the crest, y across the slot) and extruded over the cylinder's extent.
fn slot_end_rounds(st: &mut State, name: &str, ends: &[FullRound]) -> Result<(), String> {
    let target = st.first_body.clone().ok_or("a slot end round before any body")?;
    let mut ends = ends.to_vec();
    ends.sort_by(|a, b| a.point.partial_cmp(&b.point).unwrap_or(std::cmp::Ordering::Equal));
    ends.dedup_by(|a, b| crate::acis::dist(a.point, b.point) < 1e-6);
    for (k, e) in ends.iter().enumerate() {
        let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
        let (lo, hi) = e.span;
        let top = crate::acis::add(e.axis, crate::acis::mul(e.dir, hi));
        let org = [top[0] * CM, top[1] * CM, top[2] * CM];
        let ex = unit3(e.out);
        let ey = cross(unit3(e.dir), ex);
        let r = e.radius * CM;
        let pt = |id: &str, x: f64, y: f64| json!({"id": id, "x": rnd(x), "y": rnd(y)});
        let points = vec![
            pt("o", 0.0, 0.0),
            pt("a1", 0.0, r),
            pt("b1", r, r),
            pt("c1", r, 0.0),
            pt("a2", 0.0, -r),
            pt("b2", r, -r),
            pt("c2", r, 0.0),
        ];
        let line = |id: &str, a: &str, b: &str| json!({"id": id, "type": "line", "start": a, "end": b});
        let curves = vec![
            line("l1", "a1", "b1"),
            line("l2", "b1", "c1"),
            json!({"id": "r1", "type": "arc", "center": "o", "start": "c1", "end": "a1"}),
            line("l3", "a2", "b2"),
            line("l4", "b2", "c2"),
            json!({"id": "r2", "type": "arc", "center": "o", "start": "a2", "end": "c2"}),
        ];
        let sname = format!("{nm}_sketch");
        st.out.push(json!({"type": "sketch", "name": sname, "plane": {"origin": org, "x_axis": ex, "y_axis": ey},
                           "points": points, "curves": curves}));
        let picks = [[rnd(0.8 * r), rnd(0.8 * r)], [rnd(0.8 * r), rnd(-0.8 * r)]];
        st.out.push(json!({"type": "extrude", "name": nm, "sketch": sname,
                           "profiles": [{"point": picks[0]}, {"point": picks[1]}], "operation": "join",
                           "target": target, "distance": rnd(-(hi - lo) * CM), "symmetric": false}));
    }
    Ok(())
}

/// A cylinder blend between a plane and a convex cylinder that do not meet (a consumed middle
/// face): the material it adds is the sliver between its arc and the surrounding body. It is
/// joined as a sketch of the arc and a circumscribed polyline (1.2 radii out) whose far side
/// lies in the existing body; only the whole-part rebuild check vouches for that.
fn bridged_rounds(st: &mut State, name: &str, rs: &[BridgedRound]) -> Result<(), String> {
    let target = st.first_body.clone().ok_or("a bridged round before any body")?;
    for (k, b) in rs.iter().enumerate() {
        let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
        let (lo, hi) = b.span;
        let top = crate::acis::add(b.axis, crate::acis::mul(b.dir, hi));
        let org = [top[0] * CM, top[1] * CM, top[2] * CM];
        let ex = unit3(b.to_plane);
        let ey = cross(unit3(b.dir), ex);
        let phi = libm::atan2(crate::acis::dot(b.to_cyl, ey), crate::acis::dot(b.to_cyl, ex));
        let (rad, out) = (b.radius * CM, 1.2 * b.radius * CM);
        let at = |a: f64, q: f64| (q * libm::cos(a), q * libm::sin(a));
        let ang = [0.0, phi / 3.0, 2.0 * phi / 3.0, phi];
        let mut points = vec![
            json!({"id": "o", "x": 0.0, "y": 0.0}),
            json!({"id": "p0", "x": rnd(rad), "y": 0.0}),
            json!({"id": "k", "x": rnd(at(phi, rad).0), "y": rnd(at(phi, rad).1)}),
        ];
        for (i, a) in ang.iter().enumerate() {
            let (x, y) = at(*a, out);
            points.push(json!({"id": format!("w{i}"), "x": rnd(x), "y": rnd(y)}));
        }
        let line = |id: &str, a: &str, b: &str| json!({"id": id, "type": "line", "start": a, "end": b});
        let (s0, s1) = if phi > 0.0 { ("p0", "k") } else { ("k", "p0") };
        let curves = vec![
            json!({"id": "arc", "type": "arc", "center": "o", "start": s0, "end": s1}),
            line("l0", "k", "w3"),
            line("l1", "w3", "w2"),
            line("l2", "w2", "w1"),
            line("l3", "w1", "w0"),
            line("l4", "w0", "p0"),
        ];
        let sname = format!("{nm}_sketch");
        st.out.push(json!({"type": "sketch", "name": sname, "plane": {"origin": org, "x_axis": ex, "y_axis": ey},
                           "points": points, "curves": curves}));
        let mid = at(phi / 2.0, (rad + out) / 2.0);
        st.out.push(json!({"type": "extrude", "name": nm, "sketch": sname,
                           "profiles": [{"point": [rnd(mid.0), rnd(mid.1)]}], "operation": "join",
                           "target": target, "distance": rnd(-(hi - lo) * CM), "symmetric": false}));
    }
    Ok(())
}

fn blend(r: &Reader, st: &mut State, f: &Feature, name: &str, typ: FeatType) -> Result<(), String> {
    let s = r.slots(f)?;
    let label = typ.label();
    let Some(b) = r.result() else {
        st.out.push(unsupported(name, &label, Some("the result body is not readable".into())));
        return Ok(());
    };
    let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
    let faces = b.blend_faces(fid);
    // A pick: the midpoint of the pre-fillet edge (mm) and, unless the edge is a whole circle,
    // two points 0.1 mm either side of it along the edge.
    // With the blend face's radius (mm; cylinders and tori), to tell the edge sets of a fillet
    // with several radii apart.
    let mut mids: Vec<(V3, Option<[V3; 2]>, Option<f64>)> = Vec::new();
    let mut why: Vec<String> = Vec::new();
    let mut radii: Vec<f64> = Vec::new();
    let mut rounds: Vec<(V3, V3)> = Vec::new();
    let mut slot_ends: Vec<FullRound> = Vec::new();
    // The edges the picks lie on: the circles, and how many picks lie on lines.
    let mut circles: Vec<(V3, V3, f64)> = Vec::new();
    let mut lines = 0usize;
    let mut infos: Vec<(V3, EdgeInfo)> = Vec::new();
    let mut bridged: Vec<BridgedRound> = Vec::new();
    let chamfer_d = (typ == FeatType::Chamfer).then(|| r.pval(&s, 2).ok()).flatten();
    let mm = |v: V3| [rnd_to(v[0] * CM, 1e6), rnd_to(v[1] * CM, 1e6), rnd_to(v[2] * CM, 1e6)];
    for &face in &faces {
        let k = b.kind(face);
        // A corner patch; and a planar face with a fillet's tag is a leftover of an edge split
        // later (a fillet face is never planar).
        // A vertex blend (`srf_srf_v_bl_spl_sur`) is the patch where blends meet at a corner.
        let patch = matches!(&k, Surf::Other(t) if t.contains("_v_bl_"));
        if matches!(k, Surf::Sphere) || patch || (typ == FeatType::Fillet && matches!(k, Surf::Plane { .. })) {
            continue;
        }
        if typ == FeatType::Chamfer && b.is_chamfer_corner_patch(face, &faces) {
            continue;
        }
        // A tagged face without a vertex (no loop the reader follows) names no edge; the
        // feature's other faces do.
        if b.verts(face).is_empty() && faces.len() > 1 {
            continue;
        }
        if typ == FeatType::Fillet
            && let Some(fr) = b.full_round(face)
        {
            if fr.concave {
                slot_ends.push(fr);
            } else {
                rounds.push((mm(fr.point), fr.normal));
            }
            continue;
        }
        if typ == FeatType::Fillet
            && let Some(br) = b.bridged_round(face)
        {
            if bridged.iter().all(|x| crate::acis::dist(x.axis, br.axis) > 1e-6) {
                bridged.push(br);
            }
            continue;
        }
        match b.edge_mid(face) {
            Err(w) => why.push(w),
            Ok((m, info)) => {
                // A chamfer face lies within a few distances of the edge it replaced. A planar
                // face with the tag whose "edge" is further away is a leftover (an original
                // wall that kept the tag when a chamfer chain went round it), not a chamfer.
                if typ == FeatType::Chamfer
                    && matches!(k, Surf::Plane { .. })
                    && let (EdgeInfo::Line { dir }, Some(d)) = (&info, chamfer_d)
                    && b.verts(face).iter().any(|&v| {
                        let q = crate::acis::sub(v, m);
                        let off = crate::acis::sub(q, crate::acis::mul(*dir, crate::acis::dot(q, *dir)));
                        libm::sqrt(crate::acis::dot(off, off)) > 3.0 * d
                    })
                {
                    continue;
                }
                let rad = match k {
                    Surf::Cyl { r, .. } => Some(rnd_to(r * CM, 1e6)),
                    Surf::Torus { minor, .. } => Some(rnd_to(minor * CM, 1e6)),
                    _ => None,
                };
                radii.extend(rad);
                mids.push((mm(m), local_path(m, &info, PATH_EPS).map(|[a, b]| [mm(a), mm(b)]), rad));
                infos.push((m, info));
                // Pieces of one circle (the result splits a whole circle in two) are one edge.
                match info {
                    EdgeInfo::Circle { cen, ax, r: rc } => {
                        let same = |c: &(V3, V3, f64)| {
                            crate::acis::dist(c.0, cen) <= 1e-9
                                && (crate::acis::dot(unit3(c.1), unit3(ax)).abs() - 1.0).abs() <= 1e-9
                                && (c.2 - rc).abs() <= 1e-9
                        };
                        if !circles.iter().any(same) {
                            circles.push((cen, ax, rc));
                        }
                    }
                    EdgeInfo::Line { .. } => lines += 1,
                }
            }
        }
    }
    if !bridged.is_empty() {
        if !mids.is_empty() || !why.is_empty() || !rounds.is_empty() || !slot_ends.is_empty() {
            why.push("a bridged round next to other fillet edges".into());
        } else {
            return bridged_rounds(st, name, &bridged);
        }
    }
    if !slot_ends.is_empty() {
        if !mids.is_empty() || !why.is_empty() || !rounds.is_empty() {
            why.push("a full round next to other fillet edges".into());
        } else {
            return slot_end_rounds(st, name, &slot_ends);
        }
    }
    if !rounds.is_empty() {
        if !mids.is_empty() || !why.is_empty() {
            why.push("a full round next to other fillet edges".into());
        } else {
            // One feature, several middle faces: one full-round fillet each (the first keeps
            // the feature's name).
            rounds.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            rounds.dedup_by(|a, b| crate::acis::dist(a.0, b.0) < 1e-6);
            for (k, (p, n)) in rounds.iter().enumerate() {
                let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
                let normal = [rnd(n[0]), rnd(n[1]), rnd(n[2])];
                st.out.push(json!({"type": "fillet", "name": nm, "full_round": {"point": p, "normal": normal}}));
            }
            return Ok(());
        }
    }
    if faces.is_empty() {
        why.push("no blend faces tagged with this feature in the result body".into());
    }
    if !why.is_empty() {
        why.sort();
        why.dedup();
        st.out.push(unsupported(name, &label, Some(why.join("; "))));
        return Ok(());
    }
    let mut uniq: Vec<(V3, Option<[V3; 2]>, Option<f64>)> = Vec::new();
    for (m, p, q) in mids {
        if uniq.iter().all(|u| crate::acis::dist(m, u.0) > 1e-6) {
            uniq.push((m, p, q));
        }
    }
    if uniq.is_empty() {
        st.out.push(unsupported(name, &label, Some("no edge of the blend faces".into())));
        return Ok(());
    }
    let pick = |(m, p, _): &(V3, Option<[V3; 2]>, Option<f64>)| match p {
        Some([a, b]) => json!({"midpoint": m, "path": [a, b]}),
        None => json!({"midpoint": m}),
    };
    let mut edges: Vec<Value> = uniq.iter().map(pick).collect();
    // A chamfer on one edge: its reference lines start on that edge (note AS).
    if typ == FeatType::Chamfer
        && !r.dc.has_row()
        && circles.len() + lines == 1
        && let Some((m, info)) = infos.first()
        && let Some((p0, at)) = chamfer_edge_at_lines(&ref_lines(r, &s), info, *m)
    {
        let path = local_path(p0, &at, PATH_EPS).map(|[a, b]| [mm(a), mm(b)]);
        edges = vec![pick(&(mm(p0), path, None))];
    }
    if typ == FeatType::Fillet {
        let mut rad = None;
        let (sets, _) = r.dc.first_list(s.need(0)?, None).ok_or("the fillet has no edge sets")?;
        'sets: for x in sets.iter().flatten() {
            for (_, y) in r.dc.refs(*x) {
                if r.dc.role(y) == Role::Param {
                    rad = r.dc.param(y).map(|p| p.1 * CM);
                    break;
                }
            }
            if rad.is_some_and(|v| v != 0.0) {
                break 'sets;
            }
        }
        radii.sort_by(f64::total_cmp);
        radii.dedup();
        if rad.is_none() && radii.len() == 1 {
            rad = radii.first().copied();
        }
        let Some(rad) = rad else {
            st.out.push(unsupported(name, "fillet", Some("the radius is not readable".into())));
            return Ok(());
        };
        if sets.len() > 1 || radii.iter().any(|q| (q - rad).abs() > 1e-6) {
            // Several edge sets: one fillet per radius, each edge going to the set whose radius
            // its blend face has (a face without a radius, or a radius no set has, refuses).
            let mut set_radii: Vec<f64> = Vec::new();
            for x in sets.iter().flatten() {
                let q = r.dc.refs(*x).into_iter().find(|&(_, y)| r.dc.role(y) == Role::Param);
                if let Some(v) = q.and_then(|(_, y)| r.dc.param(y)).map(|p| rnd_to(p.1 * CM, 1e6))
                    && v > 0.0
                    && set_radii.iter().all(|w| (w - v).abs() > 1e-6)
                {
                    set_radii.push(v);
                }
            }
            let face_radii: Vec<Option<f64>> = uniq.iter().map(|u| u.2).collect();
            let Some(groups) = radius_groups(&set_radii, &face_radii) else {
                st.out.push(unsupported(name, "fillet with several radii", None));
                return Ok(());
            };
            for (k, (rad, idx)) in groups.into_iter().enumerate() {
                let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
                let edges: Vec<Value> = idx.iter().filter_map(|&i| uniq.get(i)).map(pick).collect();
                st.out.push(json!({"type": "fillet", "name": nm, "edges": edges, "radius": rnd(rad)}));
            }
            return Ok(());
        }
        st.out.push(json!({"type": "fillet", "name": name, "edges": edges, "radius": rnd(rad)}));
    } else {
        let dist = r.pval(&s, 2)?;
        let mut ch = json!({"type": "chamfer", "name": name, "edges": edges, "distance": rnd(dist * CM)});
        match chamfer_kind(r, &s, dist, (circles.len() + lines).min(edges.len())) {
            Ok(None) => {}
            Ok(Some(Second::Angle(angle, face))) => {
                put(&mut ch, "angle", json!(rnd(angle.to_degrees())));
                put(&mut ch, "face", json!({"point": face}));
            }
            Ok(Some(Second::Distance(d2, face))) => {
                put(&mut ch, "distance2", json!(rnd(d2 * CM)));
                put(&mut ch, "face", json!({"point": face}));
            }
            Err(why) => {
                st.out.push(unsupported(name, "chamfer", Some(why)));
                return Ok(());
            }
        }
        st.out.push(ch);
    }
    Ok(())
}

/// The edges of a fillet with several edge sets, by radius: for each set radius (mm, in set
/// order) that some edge has, the indices of the edges whose blend face has it. `None` when an
/// edge has no radius (a spline blend) or one no set has.
pub(crate) fn radius_groups(set_radii: &[f64], face_radii: &[Option<f64>]) -> Option<Vec<(f64, Vec<usize>)>> {
    let mut groups: Vec<(f64, Vec<usize>)> = set_radii.iter().map(|&r| (r, Vec::new())).collect();
    for (i, q) in face_radii.iter().enumerate() {
        let q = (*q)?;
        groups.iter_mut().find(|g| (g.0 - q).abs() <= 1e-6)?.1.push(i);
    }
    groups.retain(|g| !g.1.is_empty());
    Some(groups)
}

/// The chamfer type (slot 4, `7daa0032`: `u16 2, u16 type, u32 0` at the end; 0 equal
/// distances, 1 distance and angle, confirmed on the M8 screw) and, for an unequal one, the
/// angle (slot 10, radians) and the point (mm) that names the face the distance runs along.
/// Slot 8 holds two records of the two offset lines of the first edge: start on the edge, end
/// `distance` away inside a support face; the first one is the face of the first distance.
/// Anything else (another type, one record not a line of that length, several edges: the pick
/// would apply to every edge) is refused.
/// The second leg of an unequal chamfer: an angle (radians) or a distance (cm), with a point on
/// the reference face (mm) that the first distance runs along.
#[derive(Debug, PartialEq)]
pub(crate) enum Second {
    Angle(f64, V3),
    Distance(f64, V3),
}

/// A chamfer's type (slot 4) read: 0 equal distances, 1 distance and angle (slot 10), 2 two
/// distances (slots 2 and 3). `None` for an equal chamfer (type 2 with both distances the same
/// included).
fn chamfer_kind(r: &Reader, s: &Slots, dist: f64, edges: usize) -> Result<Option<Second>, String> {
    let rec = r.dc.r(s.need(4)?);
    let n = rec.len();
    let (Some(two), Some(ty), Some(zero)) = (
        n.checked_sub(8).and_then(|k| u16_at(rec, k)),
        n.checked_sub(6).and_then(|k| u16_at(rec, k)),
        n.checked_sub(4).and_then(|k| u32_at(rec, k)),
    ) else {
        return Err("the chamfer type is not readable".into());
    };
    if two != 2 || zero != 0 {
        return Err("the chamfer type record has an unknown layout".into());
    }
    let leg = match ty {
        0 => return Ok(None),
        1 => r.pval(s, 10)?,
        2 => r.pval(s, 3)?,
        t => return Err(format!("chamfer type {t} is not decoded")),
    };
    chamfer_second(ty, dist, leg, edges, &ref_lines(r, s))
}

/// A chamfer's reference lines (`797737b1` records of slot 8, the first two): kind, start, end
/// (cm). Both start at a point of the (first) chamfered edge and run along its two faces.
fn ref_lines(r: &Reader, s: &Slots) -> Vec<(u32, V3, V3)> {
    let mut lines: Vec<(u32, V3, V3)> = Vec::new();
    if let Some(i) = s.get(8)
        && let Some((items, _)) = r.dc.first_list(i, None)
    {
        for k in items.iter().take(2).flatten() {
            let rf = r.dc.r(*k);
            let f = |j: usize| f64_at(rf, 22 + 8 * j).filter(|v| v.is_finite());
            let (Some(kind), Some(a), Some(b), Some(c), Some(d), Some(e), Some(g)) =
                (u32_at(rf, 10), f(0), f(1), f(2), f(3), f(4), f(5))
            else {
                break;
            };
            lines.push((kind, [a, b, c], [d, e, g]));
        }
    }
    lines
}

/// A chamfer on one edge whose reference lines start off the edge read from its blend face
/// (the face's neighbours meet elsewhere): the pick at the lines' start, on that edge's line
/// or on the circle about its axis through the start. `None` when the start lies on the edge
/// read, or more than 1 cm off it (the lines then belong to another edge: 3 to 25 cm off on six
/// corpus parts where the edge read is the right one). An iPart member's lines are the
/// factory's, so they are not used for it.
fn chamfer_edge_at_lines(lines: &[(u32, V3, V3)], info: &EdgeInfo, m: V3) -> Option<(V3, EdgeInfo)> {
    // How far (cm) the start may lie from the edge read from the blend faces.
    const REF_REACH: f64 = 1.0;
    let &(2, p0, _) = lines.first()? else { return None };
    let off = |q: V3, dir: V3| {
        let d = crate::acis::sub(q, m);
        let along = crate::acis::mul(dir, dot(d, dir));
        crate::acis::dist(crate::acis::sub(d, along), [0.0; 3])
    };
    match *info {
        EdgeInfo::Line { dir } => {
            let dir = unit3(dir);
            let o = off(p0, dir);
            (o > 1e-7 && o <= REF_REACH).then_some((p0, EdgeInfo::Line { dir }))
        }
        EdgeInfo::Circle { cen, ax, r } => {
            let ax = unit3(ax);
            let h = dot(crate::acis::sub(p0, cen), ax);
            let c = crate::acis::add(cen, crate::acis::mul(ax, h));
            let rr = crate::acis::dist(p0, c);
            let on = h.abs() <= 1e-7 && (rr - r).abs() <= 1e-7;
            let near = h.abs() <= REF_REACH && (rr - r).abs() <= REF_REACH;
            (!on && near && rr > 1e-9).then_some((p0, EdgeInfo::Circle { cen: c, ax, r: rr }))
        }
    }
}

/// The second leg of a chamfer of type `ty` (1 or 2) from its first distance `dist` (cm), the
/// leg (the angle in radians for type 1, the second distance in cm for type 2), the number of
/// edges and the reference lines of slot 8. The first line runs from the edge `dist` into the
/// face of the first distance (its end is the reference face's point); for two distances the
/// second runs from the same start `leg` into the other face. Two equal distances are an equal
/// chamfer. Only one edge: the reference face of each further edge is not read.
pub(crate) fn chamfer_second(
    ty: u16,
    dist: f64,
    leg: f64,
    edges: usize,
    lines: &[(u32, V3, V3)],
) -> Result<Option<Second>, String> {
    match ty {
        1 if !(leg > 0.0 && leg < std::f64::consts::FRAC_PI_2) => {
            return Err("the chamfer angle is out of range".into());
        }
        2 if !(leg.is_finite() && leg > 0.0) => return Err("the second chamfer distance is not positive".into()),
        2 if (leg - dist).abs() <= 1e-9 => return Ok(None),
        1 | 2 => {}
        t => return Err(format!("chamfer type {t} is not decoded")),
    }
    if edges != 1 {
        return Err("an unequal chamfer on several edges: the reference face of each edge is not read".into());
    }
    let Some(&(_, p0, p1)) = lines.first().filter(|l| l.0 == 2) else {
        return Err("the reference face of the chamfer is not readable".into());
    };
    if (crate::acis::dist(p0, p1) - dist).abs() > 1e-6 {
        return Err("the first distance does not run along the first reference face".into());
    }
    let face = [rnd_to(p1[0] * CM, 1e6), rnd_to(p1[1] * CM, 1e6), rnd_to(p1[2] * CM, 1e6)];
    if ty == 1 {
        return Ok(Some(Second::Angle(leg, face)));
    }
    let Some(&(_, q0, q1)) = lines.get(1).filter(|l| l.0 == 2) else {
        return Err("the second reference face of the chamfer is not readable".into());
    };
    if (crate::acis::dist(q0, q1) - leg).abs() > 1e-6 || crate::acis::dist(p0, q0) > 1e-6 {
        return Err("the second distance does not run along the second reference face".into());
    }
    Ok(Some(Second::Distance(leg, face)))
}

/// The part's feature history as a tree (see the module notes).
///
/// # Errors
/// A reason: the file is not an Inventor part, has no feature segment, or its records do not
/// have the layout read here.
pub fn feature_tree(bytes: &[u8], file_name: &str, limits: &Limits) -> Result<Value, String> {
    feature_tree_with(bytes, file_name, limits, &[], None, None).map(|t| t.0)
}

/// [`feature_tree`] for a part read as part of another (note AO).
///
/// The parameters named in `row` are read as the given values (cm or radians) instead of the
/// stored ones: an iPart factory read for one of its members. A sketch whose dimensions the row
/// changes is solved again (note AP). Also gives the names of the row's parameters the features
/// and sketches took.
///
/// With `derived_body`, the part's derive feature stands for that body (made by features before
/// this tree): the features after it cut or join it. Without, a derive is not read.
///
/// With `result`, the stored body of that part (an iPart member's, in the factory's place) is
/// the result the fillets and chamfers are read from.
///
/// # Errors
/// As [`feature_tree`].
pub fn feature_tree_with(
    bytes: &[u8],
    file_name: &str,
    limits: &Limits,
    row: &[(String, f64)],
    derived_body: Option<&str>,
    result: Option<&[u8]>,
) -> Result<(Value, Vec<String>), String> {
    let mut dc = Dc::open(bytes, limits)?;
    dc.set_row(row);
    let feats = dc.ordered_features();
    if feats.is_empty() {
        return Err("the part has no features".into());
    }
    if feats.len() > MAX_FEATURES {
        return Err("the part has too many features".into());
    }
    // An imported base body (note AR): the file keeps its shape only inside the result.
    if derived_body.is_none() && dc.has_base_body() {
        return Err(
            "the part starts from an imported body, whose shape the file keeps only as part of the result".into()
        );
    }
    let r = Reader {
        dc,
        result_bytes: result.unwrap_or(bytes),
        limits,
        back: OnceCell::new(),
        owned: OnceCell::new(),
        history: OnceCell::new(),
        result: OnceCell::new(),
        solved: RefCell::new(HashMap::new()),
        changed: OnceCell::new(),
    };
    let names: HashMap<usize, String> =
        feats.iter().map(|f| (f.rec, f.name.clone().unwrap_or_else(|| format!("Feature{}", f.rec)))).collect();
    let mut st = State {
        out: Vec::new(),
        done: HashMap::new(),
        first_body: None,
        omitted: Vec::new(),
        names,
        holes: HashMap::new(),
        emitted: HashMap::new(),
        bodies: HashMap::new(),
        sheets: Vec::new(),
    };
    for (k, f) in feats.iter().enumerate() {
        let name = f.name.clone().unwrap_or_else(|| format!("Feature{}", f.rec));
        let Some(typ) = f.typ else {
            st.out.push(unsupported(&name, "unknown", None));
            continue;
        };
        // Suppressed in the source (flag bit 2 of the header): not part of the result body.
        if r.dc.r(f.rec).get(10).is_some_and(|b| b & 0x04 != 0) {
            st.omitted.push(json!({"name": name, "why": format!("{} suppressed in the source", typ.label())}));
            continue;
        }
        let before = st.out.len();
        let res = match typ {
            FeatType::Extrude | FeatType::Revolve => solid(&r, &mut st, f, &name, typ),
            FeatType::Fillet | FeatType::Chamfer => blend(&r, &mut st, f, &name, typ),
            FeatType::Hole => hole(&r, &mut st, f, &name),
            FeatType::RectPattern => pattern(&r, &mut st, f, &name),
            FeatType::CircPattern => circular_pattern(&r, &mut st, f, &name),
            FeatType::Mirror => mirror(&r, &mut st, f, &name),
            FeatType::Other(0x9a78_add3) => revolved_from_faces(&r, &mut st, f, &name),
            FeatType::Thicken => thicken(&r, &mut st, f, &name),
            FeatType::Shell => {
                let later: Vec<i64> = feats.iter().skip(k + 1).filter_map(|g| r.dc.seq(g.rec)).map(i64::from).collect();
                shell(&r, &mut st, f, &name, &later)
            }
            FeatType::Sweep => sweep(&r, &mut st, f, &name),
            FeatType::Coil => coil(&r, &mut st, f, &name),
            FeatType::Emboss => emboss(&r, &mut st, f, &name),
            FeatType::Combine => combine(&r, &mut st, f, &name),
            FeatType::Split => split(&r, &mut st, f, &name),
            FeatType::Plate => plate(&r, &mut st, f, &name),
            FeatType::Other(0x2f54_efe0) => loft(&r, &mut st, f, &name),
            FeatType::Other(0x4c37_ffe8) => fold(&r, &mut st, f, &name),
            FeatType::Other(0xf4fa_0900) => face_draft(&r, &mut st, f, &name),
            // Move Bodies, and the direct edits Move and Scale of whole bodies (note AQ).
            FeatType::Other(0x2b83_9fe9 | 0x8fa6_a6de | 0x5bcb_6fff) => body_transform(&r, &mut st, f, &name),
            // A cosmetic thread changes no geometry.
            FeatType::Thread => {
                st.omitted.push(json!({"name": name, "why": "cosmetic thread"}));
                Ok(())
            }
            // A derive whose body the caller made: the features after it work on that body.
            FeatType::Other(0x8e29_cbee) if derived_body.is_some() && st.first_body.is_none() => {
                st.first_body = derived_body.map(str::to_string);
                st.omitted.push(json!({"name": name, "why": "derive, made by the base part's features"}));
                Ok(())
            }
            // A surface (extrude, revolve, sweep or offset surface, boundary patch; note AQ):
            // no solid. A later feature that takes it as a tool refuses on its own.
            FeatType::Other(0xe597_a4f0 | 0x36ed_6f3a | 0xcc42_4542 | 0xa82c_f09e | 0x8a94_bf8f) => {
                st.omitted.push(json!({"name": name, "why": "surface feature, no solid"}));
                Ok(())
            }
            // Nor does the tagging feature a derive leaves (note AO).
            FeatType::Other(0xdfdb_99e8) => {
                st.omitted.push(json!({"name": name, "why": "tagging feature, no geometry"}));
                Ok(())
            }
            other => {
                st.out.push(unsupported(&name, &other.label(), None));
                Ok(())
            }
        };
        if let Err(why) = res {
            st.out.truncate(before);
            st.out.push(unsupported(&name, &typ.label(), Some(why)));
        }
        let made: Vec<String> = st
            .out
            .get(before..)
            .into_iter()
            .flatten()
            .filter(|o| !matches!(o.get("type").and_then(Value::as_str), Some("sketch")))
            .filter_map(|o| o.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        st.emitted.insert(name.clone(), made);
    }
    let file = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    if r.dc.has_row() {
        unused_planes(&r);
    }
    let used = r.dc.row_used();
    let tree = json!({"format": "gusset-feature-tree", "version": 2,
              "source": {"app": "inventor", "file": file, "omitted": st.omitted},
              "features": st.out});
    Ok((tree, used))
}
