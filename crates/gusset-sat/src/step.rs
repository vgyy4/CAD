//! STEP AP214 text from the SAT records, in millimetres.
//!
//! Field layouts (after the attribute pointer `f[0]`), read from sample files:
//! `body`: lump, wire, transform. `lump`: next, shell, body. `shell`: next, subshell, face,
//! wire, lump. `face`: next, loop, shell, subshell, surface, sense, sidedness. `loop`: next,
//! coedge, face. `coedge`: next, previous, partner, edge, sense, loop. `edge`: start vertex,
//! start parameter, end vertex, end parameter, coedge, curve, sense. `vertex`: edge, point.
//! `point`: x y z. `transform`: a 3x3 matrix by rows (points are row vectors), translation,
//! scale. `straight-curve`: root, direction. `ellipse-curve`: centre, normal, major axis
//! (its length the major radius), radius ratio. `plane-surface`: root, normal, u direction.
//! `cone-surface`: centre, axis, major axis, ratio, an interval, then the sine and cosine of
//! the half angle: the radius at height `h` along the axis is `|major| + h sin / cos` (checked
//! on the circular edges of the samples' cones); a negative cosine turns the normal inward.

use crate::SatError;
use crate::parse::{Rec, Sat};
use std::collections::BTreeMap;
use std::fmt::Write as _;

mod exact;

type V3 = [f64; 3];

fn len(a: V3) -> f64 {
    libm::sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2])
}
fn unit(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-300 && l.is_finite()).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

fn real(v: f64) -> String {
    if !v.is_finite() {
        return "0.".into();
    }
    let s = format!("{v:?}");
    let (mant, exp) = match s.split_once('e') {
        Some((m, e)) => (m.to_string(), Some(e.to_string())),
        None => (s, None),
    };
    let mant = if mant.contains('.') { mant } else { format!("{mant}.") };
    match exp {
        Some(e) => format!("{mant}E{e}"),
        None => mant,
    }
}

fn string(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .map(|c| if c == '\'' || c == '\\' { ' ' } else { c })
        .take(200)
        .collect();
    format!("'{clean}'")
}

fn list(ids: &[usize]) -> String {
    let v: Vec<String> = ids.iter().map(|i| format!("#{i}")).collect();
    format!("({})", v.join(","))
}

/// Model to millimetres: `p' = s (p R) + t` scaled by the file's unit.
#[derive(Clone, Copy)]
struct Xf {
    r: [V3; 3],
    t: V3,
    s: f64,
    unit: f64,
}

impl Xf {
    fn point(&self, p: V3) -> V3 {
        let d = self.dir_raw(p);
        [
            self.unit * (self.s * d[0] + self.t[0]),
            self.unit * (self.s * d[1] + self.t[1]),
            self.unit * (self.s * d[2] + self.t[2]),
        ]
    }
    fn dir_raw(&self, d: V3) -> V3 {
        let [a, b, c] = self.r;
        [
            d[0] * a[0] + d[1] * b[0] + d[2] * c[0],
            d[0] * a[1] + d[1] * b[1] + d[2] * c[1],
            d[0] * a[2] + d[1] * b[2] + d[2] * c[2],
        ]
    }
    fn dir(&self, d: V3) -> Option<V3> {
        unit(self.dir_raw(d))
    }
    fn length(&self, l: f64) -> f64 {
        l * self.s.abs() * self.unit
    }
}

/// A B-spline curve as ACIS writes it inside `{ ... }`: `<subtype> [full] nubs|nurbs
/// <degree> open|closed <n> <knot> <multiplicity>...` then the control points (`x y z`, or
/// `x y z w` for nurbs). End knots carry multiplicity `degree` (one less than STEP's).
#[derive(Clone)]
struct Bs3 {
    degree: usize,
    knots: Vec<f64>,
    mults: Vec<usize>,
    points: Vec<V3>,
    weights: Option<Vec<f64>>,
}

/// Most `nubs` candidates tried in a procedural surface for its own approximation.
const MAX_SURFACE_CANDIDATES: usize = 8;

/// Highest B-spline degree read.
const MAX_DEGREE: usize = 25;

fn bs3_curve(r: &Rec, from: usize) -> Result<Bs3, SatError> {
    bs3_curve_end(r, from).map(|(b, _)| b)
}

/// As [`bs3_curve`], with the field after the curve's data.
fn bs3_curve_end(r: &Rec, from: usize) -> Result<(Bs3, usize), SatError> {
    let bad = |m: &str| SatError::Malformed(format!("{}: {m}", r.ty));
    let mut k = from;
    // Skip to the representation word.
    let mut rational = None;
    for _ in 0..6 {
        match r.word(k) {
            Some("nubs") => rational = Some(false),
            Some("nurbs") => rational = Some(true),
            Some("nullbs") => {
                return Err(SatError::Unsupported(format!("{} without a B-spline is not read yet", r.ty)));
            }
            _ => {}
        }
        k += 1;
        if rational.is_some() {
            break;
        }
    }
    let rational = rational
        .ok_or_else(|| SatError::Unsupported(format!("{} of a kind without B-spline data is not read yet", r.ty)))?;
    let degree = r.num(k).map(|d| d as usize).filter(|d| (1..=MAX_DEGREE).contains(d)).ok_or_else(|| bad("degree"))?;
    // `open`, `closed` or `periodic`: the samples' periodic curves are written like open ones
    // (end knots of multiplicity `degree`, first and last points equal), so all read alike.
    let n = r.num(k + 2).filter(|n| *n >= 2.0 && *n <= 1e6).ok_or_else(|| bad("knot count"))? as usize;
    k += 3;
    let mut knots = Vec::with_capacity(n.min(1 << 16));
    let mut mults = Vec::with_capacity(n.min(1 << 16));
    for _ in 0..n {
        let t = r.num(k).ok_or_else(|| bad("knot"))?;
        let m = r
            .num(k + 1)
            .map(|m| m as usize)
            .filter(|m| (1..=degree + 1).contains(m))
            .ok_or_else(|| bad("multiplicity"))?;
        if knots.last().is_some_and(|l| t <= *l) {
            return Err(bad("knots do not increase"));
        }
        knots.push(t);
        mults.push(m);
        k += 2;
    }
    let total: usize = mults.iter().sum();
    let count = (total + 1).checked_sub(degree).filter(|c| *c > degree).ok_or_else(|| bad("too few control points"))?;
    let dim = if rational { 4 } else { 3 };
    let mut points = Vec::with_capacity(count.min(1 << 16));
    let mut weights = rational.then(|| Vec::with_capacity(count.min(1 << 16)));
    for _ in 0..count {
        let p = r.vec3(k).ok_or_else(|| bad("control point"))?;
        points.push(p);
        if let Some(w) = weights.as_mut() {
            let wt = r.num(k + 3).filter(|w| *w > 0.0).ok_or_else(|| bad("weight"))?;
            w.push(wt);
        }
        k += dim;
    }
    // STEP's end knots repeat once more.
    if let Some(m) = mults.first_mut() {
        *m = (*m + 1).min(degree + 1);
    }
    if let Some(m) = mults.last_mut() {
        *m = (*m + 1).min(degree + 1);
    }
    if mults.iter().sum::<usize>() != points.len() + degree + 1 {
        return Err(bad("knots and control points do not match"));
    }
    Ok((Bs3 { degree, knots, mults, points, weights }, k))
}

/// The field of the `}` that closes the block opening at field `open`.
fn block_end(r: &Rec, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in open..r.f.len() {
        match r.word(i) {
            Some("{") => depth += 1,
            Some("}") => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A B-spline's knots one by one (each repeated by its multiplicity).
fn flat_knots(b: &Bs3) -> Vec<f64> {
    b.knots.iter().zip(&b.mults).flat_map(|(k, m)| std::iter::repeat_n(*k, *m)).collect()
}

/// A B-spline from knots one by one and homogeneous control points (`x w, y w, z w, w`).
fn from_flat(degree: usize, flat: &[f64], hom: &[[f64; 4]], rational: bool) -> Option<Bs3> {
    let mut knots: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for k in flat {
        match (knots.last(), mults.last_mut()) {
            (Some(l), Some(m)) if l.total_cmp(k).is_eq() => *m += 1,
            _ => {
                knots.push(*k);
                mults.push(1);
            }
        }
    }
    let mut points = Vec::with_capacity(hom.len());
    let mut weights = Vec::with_capacity(hom.len());
    for h in hom {
        if h[3].is_nan() || h[3] <= 0.0 {
            return None;
        }
        points.push([h[0] / h[3], h[1] / h[3], h[2] / h[3]]);
        weights.push(h[3]);
    }
    (flat.len() == hom.len() + degree + 1).then_some(Bs3 {
        degree,
        knots,
        mults,
        points,
        weights: rational.then_some(weights),
    })
}

/// Inserts knot `u` once (Boehm) into a B-spline given by flat knots and homogeneous points.
fn insert_knot(degree: usize, flat: &mut Vec<f64>, hom: &mut Vec<[f64; 4]>, u: f64) -> Option<()> {
    // The span: flat[k] <= u < flat[k + 1], within the curve's range.
    let k = (degree..flat.len().checked_sub(degree + 1)?).rev().find(|k| flat.get(*k).is_some_and(|x| *x <= u))?;
    let mut out = Vec::with_capacity(hom.len() + 1);
    for i in 0..=hom.len() {
        let p = if i + degree <= k {
            *hom.get(i)?
        } else if i > k {
            *hom.get(i - 1)?
        } else {
            let (a, b) = (*flat.get(i)?, *flat.get(i + degree)?);
            let t = if b > a { (u - a) / (b - a) } else { 0.0 };
            let (p0, p1) = (*hom.get(i.checked_sub(1)?)?, *hom.get(i)?);
            let mut q = [0.0; 4];
            for ((q, x0), x1) in q.iter_mut().zip(p0).zip(p1) {
                *q = (1.0 - t) * x0 + t * x1;
            }
            q
        };
        out.push(p);
    }
    flat.insert(k + 1, u);
    *hom = out;
    Some(())
}

/// The part of a closed B-spline (first and last points equal) between `a` and `b` (`a < b`,
/// at most one turn apart): the curve continued past its end by another turn where `b` lies
/// beyond it, cut at both ends by knot insertion. Exact. `None` where the curve is not closed
/// or the range does not fit.
fn closed_bs3_part(c: &Bs3, a: f64, b: f64) -> Option<Bs3> {
    let d = c.degree;
    let (k0, kn) = (*c.knots.first()?, *c.knots.last()?);
    let period = kn - k0;
    let (first, last) = (*c.points.first()?, *c.points.last()?);
    let span = c.points.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs())).max(1.0);
    if period.is_nan()
        || period <= 0.0
        || first.iter().zip(last).any(|(x, y)| (x - y).abs() > 1e-9 * span)
        || b <= a
        || b - a > period
    {
        return None;
    }
    // Start within the curve's first turn.
    let turns = libm::floor((a - k0) / period);
    let (a, b) = (a - turns * period, b - turns * period);
    let w = |i: usize| c.weights.as_ref().and_then(|w| w.get(i).copied()).unwrap_or(1.0);
    let hom = |i: usize, scale: f64| {
        let p = c.points.get(i).copied().unwrap_or([0.0; 3]);
        let wi = w(i) * scale;
        [p[0] * wi, p[1] * wi, p[2] * wi, wi]
    };
    let mut flat = flat_knots(c);
    let mut pts: Vec<[f64; 4]> = (0..c.points.len()).map(|i| hom(i, 1.0)).collect();
    if b > kn {
        // A second turn: the end knot repeated `degree` times, the inner knots and the end
        // shifted by a period, the points but the first (the shared joint) again, with
        // weights scaled to the joint's (a rational curve's weights may all be scaled).
        let scale = w(c.points.len() - 1) / w(0);
        let inner: Vec<f64> = flat.iter().copied().filter(|k| *k > k0).map(|k| k + period).collect();
        flat.truncate(flat.len() - 1);
        flat.extend(inner);
        pts.extend((1..c.points.len()).map(|i| hom(i, scale)));
    }
    let end = *flat.last()?;
    if b > end {
        return None;
    }
    for u in [a, b] {
        // An existing knot close to the cut is the cut.
        let u = flat.iter().copied().find(|k| (k - u).abs() <= 1e-12 * period).unwrap_or(u);
        let have = flat.iter().filter(|k| k.total_cmp(&u).is_eq()).count();
        for _ in have..d {
            insert_knot(d, &mut flat, &mut pts, u)?;
        }
    }
    let snap = |u: f64| flat.iter().copied().find(|k| (k - u).abs() <= 1e-12 * period).unwrap_or(u);
    let (a, b) = (snap(a), snap(b));
    let last_a = flat.iter().rposition(|k| k.total_cmp(&a).is_eq())?;
    let first_b = flat.iter().position(|k| k.total_cmp(&b).is_eq())?;
    let s = (last_a + 1).checked_sub(d)?;
    let mut knots = vec![a; d + 1];
    knots.extend(flat.get(last_a + 1..first_b)?.iter().copied());
    knots.extend(std::iter::repeat_n(b, d + 1));
    let part = pts.get(s.checked_sub(1)?..first_b)?;
    from_flat(d, &knots, part, c.weights.is_some())
}

/// Most turns one helix may have (a long thread, not a hostile file's million).
const MAX_HELIX_TURNS: f64 = 500.0;

/// Most STEP text one conversion may write (the worker refuses 1 GiB).
const MAX_OUTPUT: usize = 256 << 20;

/// Most warnings kept.
const MAX_WARNINGS: usize = 100;

/// Segments per turn of a helix fitted as a B-spline.
const HELIX_SEGMENTS_PER_TURN: f64 = 64.0;

/// A helix (`helix_int_cur [version] F t0 F t1 centre major minor pitch taper ...`): the
/// point at `t` is `centre + major cos t + minor sin t + pitch t / 2π` (checked against the
/// vertices of its edges), written as a C1 cubic B-spline through points at most 1/16 turn
/// apart with the exact tangents there.
fn helix(r: &Rec, from: usize) -> Result<Bs3, SatError> {
    let bad = |m: &str| SatError::Malformed(format!("helix: {m}"));
    let mut k = from;
    if r.word(k).is_some_and(|w| w.parse::<i64>().is_ok()) && matches!(r.word(k + 1), Some("F" | "I" | "T")) {
        k += 1;
    }
    let mut ends = [0.0f64; 2];
    for e in &mut ends {
        if !matches!(r.word(k), Some("F" | "T")) {
            return Err(SatError::Unsupported("helices without a finite range are not read yet".into()));
        }
        *e = r.num(k + 1).ok_or_else(|| bad("range"))?;
        k += 2;
    }
    let c = r.vec3(k).ok_or_else(|| bad("centre"))?;
    let m = r.vec3(k + 3).ok_or_else(|| bad("major axis"))?;
    let n = r.vec3(k + 6).ok_or_else(|| bad("minor axis"))?;
    let p = r.vec3(k + 9).ok_or_else(|| bad("pitch"))?;
    if r.num(k + 12).is_some_and(|t| t.abs() > 1e-12) {
        return Err(SatError::Unsupported("tapered helices are not read yet".into()));
    }
    helix_fit(c, m, n, p, ends)
}

/// The helix `c + m cos t + n sin t + p t / 2π` over `ends` as a cubic B-spline: Hermite pieces
/// between the points where the helix crosses the half-planes `k / HELIX_SEGMENTS_PER_TURN` of a
/// turn about its axis, the angles counted from a direction fixed by the axis alone. Two curves
/// on one helix (a thread's edge and the row of its flank) then share every inner piece exactly,
/// so the edge lies on its face.
fn helix_fit(c: V3, m: V3, n: V3, p: V3, ends: [f64; 2]) -> Result<Bs3, SatError> {
    let bad = |m: &str| SatError::Malformed(format!("helix: {m}"));
    let [t0, t1] = ends;
    let turns = (t1 - t0).abs() / std::f64::consts::TAU;
    if t1 <= t0 || t1.is_nan() || t0.is_nan() || turns > MAX_HELIX_TURNS {
        return Err(bad("range"));
    }
    let tau = std::f64::consts::TAU;
    let h = tau / HELIX_SEGMENTS_PER_TURN;
    // The turning axis m x n, put in a fixed half-space, and a reference direction across it
    // taken from the world axis least along it: the angle of `m` there anchors the pieces.
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let w = cross(m, n);
    let wl = dot(w, w).sqrt();
    if !(wl.is_finite() && wl > 0.0) {
        return Err(bad("radius vectors"));
    }
    let w = [w[0] / wl, w[1] / wl, w[2] / wl];
    let first = w.iter().copied().find(|v| v.abs() > 1e-9).unwrap_or(1.0);
    let sense = if first < 0.0 { -1.0 } else { 1.0 };
    let w = [w[0] * sense, w[1] * sense, w[2] * sense];
    let mag = |i: usize| w.get(i).map_or(f64::INFINITY, |v| v.abs());
    let k = (0..3).min_by(|a, b| mag(*a).total_cmp(&mag(*b))).unwrap_or(0);
    let mut e = [0.0; 3];
    if let Some(v) = e.get_mut(k) {
        *v = 1.0;
    }
    let ew = dot(e, w);
    let x = [e[0] - ew * w[0], e[1] - ew * w[1], e[2] - ew * w[2]];
    let xl = dot(x, x).sqrt();
    let x = [x[0] / xl, x[1] / xl, x[2] / xl];
    let y = cross(w, x);
    // The angle about w at parameter t is phi + sense t.
    let phi = libm::atan2(dot(m, y), dot(m, x));
    let grid = |t: f64| (phi + sense * t) / h;
    let (g0, g1) = (grid(t0), grid(t1));
    let (lo, hi) = (g0.min(g1), g0.max(g1));
    let mut breaks: Vec<f64> = Vec::new();
    let mut i = (lo + 1e-9).ceil();
    while i < hi - 1e-9 {
        breaks.push((i * h - phi) * sense);
        i += 1.0;
    }
    if sense < 0.0 {
        breaks.reverse();
    }
    breaks.push(t1);
    let comb = |a: V3, b: V3, cc: V3, d: V3, (x, y, z, w): (f64, f64, f64, f64)| {
        [
            a[0] * x + b[0] * y + cc[0] * z + d[0] * w,
            a[1] * x + b[1] * y + cc[1] * z + d[1] * w,
            a[2] * x + b[2] * y + cc[2] * z + d[2] * w,
        ]
    };
    let at = |t: f64| comb(c, m, n, p, (1.0, libm::cos(t), libm::sin(t), t / tau));
    let tangent = |t: f64| comb(c, m, n, p, (0.0, -libm::sin(t), libm::cos(t), 1.0 / tau));
    let step = |a: V3, d: V3, f: f64| [a[0] + d[0] * f, a[1] + d[1] * f, a[2] + d[2] * f];
    let mut points = vec![at(t0)];
    let mut knots = vec![t0];
    let mut mults = vec![4];
    let mut a = t0;
    let last = breaks.len();
    for (j, b) in breaks.into_iter().enumerate() {
        let (pa, pb, da, db) = (at(a), at(b), tangent(a), tangent(b));
        points.push(step(pa, da, (b - a) / 3.0));
        points.push(step(pb, db, -(b - a) / 3.0));
        points.push(pb);
        knots.push(b);
        mults.push(if j + 1 == last { 4 } else { 3 });
        a = b;
    }
    Ok(Bs3 { degree: 3, knots, mults, points, weights: None })
}

/// A procedural surface's B-spline approximation: `nubs|nurbs <du> <dv> ...` after its own data,
/// in the block that opens at `at`. The first that reads as a surface: a sweep's path curve or a
/// blend's spine can come before it (their counts have no second value). Only the surface's own
/// block counts: a blend's support surfaces and a revolved curve are nested `{ ... }` blocks with
/// approximations of their own (1145's fillet on a revolved spline was read as the revolved
/// surface), and a definition shared through `{ ref n }` ends at its closing brace. `None`: no
/// candidate; else the first that reads, or the first candidate's error.
fn own_approximation(def: &Rec, at: usize, u_fastest: bool) -> Option<Result<Bs3Surface, SatError>> {
    let is_int = |i: usize| def.num(i).is_some_and(|v| v.fract() == 0.0 && v >= 1.0);
    let mut depth = 0usize;
    let mut own = Vec::new();
    for i in at + 2..def.f.len() {
        match def.word(i) {
            Some("{") => depth += 1,
            Some("}") if depth == 0 => break,
            Some("}") => depth -= 1,
            Some("nubs" | "nurbs") if depth == 0 && is_int(i + 1) && is_int(i + 2) => own.push(i),
            _ => {}
        }
    }
    let mut found: Option<Result<Bs3Surface, SatError>> = None;
    for start in own.into_iter().take(MAX_SURFACE_CANDIDATES) {
        let r = bs3_surface(def, start, u_fastest);
        let done = r.is_ok();
        if done || found.is_none() {
            found = Some(r);
        }
        if done {
            break;
        }
    }
    found
}

/// The point at `t` of a B-spline given by flat knots and homogeneous points (de Boor).
fn de_boor(p: usize, flat: &[f64], hom: &[[f64; 4]], t: f64) -> Option<[f64; 4]> {
    let n = hom.len();
    if flat.len() != n + p + 1 || n <= p {
        return None;
    }
    let t = t.clamp(*flat.get(p)?, *flat.get(n)?);
    // The span: the last non-empty [flat[k], flat[k + 1]) that starts at or before t.
    let k = (p..n).rev().find(|k| flat.get(*k).zip(flat.get(k + 1)).is_some_and(|(a, b)| *a <= t && a < b))?;
    let mut d: Vec<[f64; 4]> = hom.get(k - p..=k)?.to_vec();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let (a0, a1) = (*flat.get(i)?, *flat.get(i + p + 1 - r)?);
            let alpha = if a1 > a0 { (t - a0) / (a1 - a0) } else { 0.0 };
            let (lo, hi) = (*d.get(j - 1)?, *d.get(j)?);
            let mut q = [0.0; 4];
            for ((q, x0), x1) in q.iter_mut().zip(lo).zip(hi) {
                *q = (1.0 - alpha) * x0 + alpha * x1;
            }
            *d.get_mut(j)? = q;
        }
    }
    d.get(p).copied()
}

fn homogeneous(p: V3, w: f64) -> [f64; 4] {
    [p[0] * w, p[1] * w, p[2] * w, w]
}

fn cartesian(h: [f64; 4]) -> Option<V3> {
    (h[3] > 0.0).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]])
}

/// A curve's point at `t`.
fn bs3_point(b: &Bs3, t: f64) -> Option<V3> {
    let hom: Vec<[f64; 4]> = b
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| homogeneous(*p, b.weights.as_ref().and_then(|w| w.get(i).copied()).unwrap_or(1.0)))
        .collect();
    cartesian(de_boor(b.degree, &flat_knots(b), &hom, t)?)
}

/// A surface's point at `(u, v)`.
fn bs3_surface_point(s: &Bs3Surface, u: f64, v: f64) -> Option<V3> {
    let flat = |knots: &[f64], mults: &[usize]| -> Vec<f64> {
        knots.iter().zip(mults).flat_map(|(k, m)| std::iter::repeat_n(*k, *m)).collect()
    };
    let [ku, kv] = &s.knots;
    let [mu, mv] = &s.mults;
    let (fu, fv) = (flat(ku, mu), flat(kv, mv));
    let mut column = Vec::with_capacity(s.points.len());
    for (iu, row) in s.points.iter().enumerate() {
        let w = |iv: usize| s.weights.as_ref().and_then(|w| w.get(iu)?.get(iv).copied()).unwrap_or(1.0);
        let hom: Vec<[f64; 4]> = row.iter().enumerate().map(|(iv, p)| homogeneous(*p, w(iv))).collect();
        column.push(de_boor(s.degree[1], &fv, &hom, v)?);
    }
    cartesian(de_boor(s.degree[0], &fu, &column, u)?)
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
/// `a + k b`.
fn axpy(a: V3, k: f64, b: V3) -> V3 {
    [a[0] + k * b[0], a[1] + k * b[1], a[2] + k * b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A torus as a rolling-ball blend can be one: centre, axis, the direction to the centre
/// circle's start (the blend's own seam), major and minor radius, and whether the blend's
/// approximation faces into the tube (a fillet) rather than out of it.
struct BlendTorus {
    centre: V3,
    axis: V3,
    start: V3,
    major: f64,
    minor: f64,
    inward: bool,
}

/// A constant-radius rolling-ball blend (`rb_blend_spl_sur`) whose ball centre runs round a
/// circle is a piece of a torus: the envelope of one sphere moved round a circle. Fusion keeps
/// such blends procedural where a support is itself procedural (1145: the fillet between a
/// cylinder and a revolved spline about the same axis), with a coarse approximation (cubic, three
/// spans across: 3.4 um off the torus, 0.05 mm3 of the body). The ball centre is the blend's
/// defining curve (`intcurve { off_int_cur ... }` in the blend's own block); it must lie on one
/// circle, the approximation's four corners at one distance from it and its other points near
/// that distance. `None` where any of it fails: the approximation is used.
fn blend_torus(def: &Rec, at: usize, approx: &Bs3Surface) -> Option<BlendTorus> {
    // The defining curve: the first `intcurve { <x>_int_cur ... }` of the blend's own block.
    let mut depth = 0usize;
    let mut spine = None;
    for i in at + 2..def.f.len() {
        match def.word(i) {
            Some("{") => depth += 1,
            Some("}") if depth == 0 => break,
            Some("}") => depth -= 1,
            Some("intcurve") if depth == 0 => {
                let open = (i + 1..i + 3).find(|j| def.word(*j) == Some("{"))?;
                if def.word(open + 1).is_some_and(|w| w.ends_with("_int_cur")) {
                    spine = bs3_curve(def, open + 2).ok();
                    break;
                }
            }
            _ => {}
        }
    }
    let spine = spine?;
    let (t0, t1) = (*spine.knots.first()?, *spine.knots.last()?);
    let at_t = |f: f64| bs3_point(&spine, t0 + (t1 - t0) * f);
    let (a, b, c) = (at_t(0.0)?, at_t(1.0 / 3.0)?, at_t(2.0 / 3.0)?);
    // The circle through three points.
    let (ab, ac) = (sub(b, a), sub(c, a));
    let n = cross(ab, ac);
    let nn = dot(n, n);
    if nn <= 1e-300 {
        return None;
    }
    let (s1, s2) = (cross(n, ab), cross(ac, n));
    let k = 0.5 / nn;
    let o = axpy(axpy(a, k * dot(ac, ac), s1), k * dot(ab, ab), s2);
    let axis = unit(n)?;
    let major = len(sub(a, o));
    let on_circle = |p: V3| {
        let d = sub(p, o);
        let h = dot(d, axis);
        let rho = len(axpy(d, -h, axis));
        (rho, h)
    };
    let tol = 1e-5 * major.max(1.0);
    for i in 0..=16 {
        let (rho, h) = on_circle(at_t(f64::from(i) / 16.0)?);
        if (rho - major).abs() > tol || h.abs() > tol {
            return None;
        }
    }
    let tube = |p: V3| {
        let (rho, h) = on_circle(p);
        libm::hypot(rho - major, h)
    };
    let range = |i: usize| -> Option<(f64, f64)> {
        let ks = approx.knots.get(i)?;
        Some((*ks.first()?, *ks.last()?))
    };
    let ((u0, u1), (v0, v1)) = (range(0)?, range(1)?);
    let corners = [(u0, v0), (u0, v1), (u1, v0), (u1, v1)];
    let ds: Vec<f64> =
        corners.iter().map(|(u, v)| bs3_surface_point(approx, *u, *v).map(tube)).collect::<Option<_>>()?;
    let minor = *ds.first()?;
    if minor <= tol || ds.iter().any(|d| (d - minor).abs() > 1e-5 * minor.max(major)) {
        return None;
    }
    for i in 0..=4 {
        for j in 0..=8 {
            let (u, v) = (u0 + (u1 - u0) * f64::from(i) / 4.0, v0 + (v1 - v0) * f64::from(j) / 8.0);
            if (tube(bs3_surface_point(approx, u, v)?) - minor).abs() > 0.02 * minor {
                return None;
            }
        }
    }
    // Which way the approximation faces: its normal at the middle against the tube's outward one.
    let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let (du, dv) = ((u1 - u0) * 1e-4, (v1 - v0) * 1e-4);
    let p = bs3_surface_point(approx, um, vm)?;
    let su = sub(bs3_surface_point(approx, um + du, vm)?, bs3_surface_point(approx, um - du, vm)?);
    let sv = sub(bs3_surface_point(approx, um, vm + dv)?, bs3_surface_point(approx, um, vm - dv)?);
    let d = sub(p, o);
    let h = dot(d, axis);
    let radial = unit(axpy(d, -h, axis))?;
    let centre_at = axpy(o, major, radial);
    let outward = sub(p, centre_at);
    let facing = dot(cross(su, sv), outward);
    if facing.abs() <= 1e-300 {
        return None;
    }
    Some(BlendTorus { centre: o, axis, start: unit(sub(a, o))?, major, minor, inward: facing < 0.0 })
}

/// A helical ruled surface (`spline-surface ... { helix_spl_line [version] F u0 F u1 F t0 F t1
/// F t0 F t1 centre major minor pitch taper axis null_surface null_surface nullbs nullbs
/// rise twist climb }`, a thread's flank): the helix of [`helix`] with its radius vectors scaled
/// by `1 + u rise` and moved `u climb` along the unit axis, so each ruling runs from the helix at
/// `u0` straight to the one at `u1` (round 22 L: the flanks of four modeled threads, whose rise is
/// the flank's radial depth over the helix radius per unit of `u` and whose climb is its axial
/// run; every edge of their faces lies on it). The twist (a turn about the axis per `u`) is zero
/// in every sample and refused otherwise. Written as degree 1 across and the helices' cubic fit
/// along, over the helix's full range.
fn helix_surface(r: &Rec, from: usize) -> Result<Bs3Surface, SatError> {
    let bad = |m: &str| SatError::Malformed(format!("helix surface: {m}"));
    let mut k = from;
    if r.word(k).is_some_and(|w| w.parse::<i64>().is_ok()) && matches!(r.word(k + 1), Some("F" | "I" | "T")) {
        k += 1;
    }
    let mut ends = [[0.0f64; 2]; 3];
    for pair in &mut ends {
        for e in pair.iter_mut() {
            if !matches!(r.word(k), Some("F" | "T")) {
                return Err(SatError::Unsupported("helical surfaces without a finite range are not read yet".into()));
            }
            *e = r.num(k + 1).filter(|v| v.is_finite()).ok_or_else(|| bad("range"))?;
            k += 2;
        }
    }
    let c = r.vec3(k).ok_or_else(|| bad("centre"))?;
    let m = r.vec3(k + 3).ok_or_else(|| bad("major axis"))?;
    let n = r.vec3(k + 6).ok_or_else(|| bad("minor axis"))?;
    let p = r.vec3(k + 9).ok_or_else(|| bad("pitch"))?;
    if r.num(k + 12).is_some_and(|t| t.abs() > 1e-12) {
        return Err(SatError::Unsupported("tapered helical surfaces are not read yet".into()));
    }
    let axis = r.vec3(k + 13).ok_or_else(|| bad("axis"))?;
    let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if !(len.is_finite() && len > 1e-12) {
        return Err(bad("axis"));
    }
    let axis = [axis[0] / len, axis[1] / len, axis[2] / len];
    if (0..4).any(|i| r.word(k + 16 + i) != Some(if i < 2 { "null_surface" } else { "nullbs" })) {
        return Err(SatError::Unsupported("helical surfaces with support surfaces are not read yet".into()));
    }
    let [rise, twist, climb] = r.vec3(k + 20).ok_or_else(|| bad("direction"))?;
    if !(rise.is_finite() && climb.is_finite()) {
        return Err(bad("direction"));
    }
    if twist.abs() > 1e-9 {
        return Err(SatError::Unsupported("twisted helical surfaces are not read yet".into()));
    }
    let [u0, u1] = ends[0];
    if u1.partial_cmp(&u0) != Some(std::cmp::Ordering::Greater) {
        return Err(bad("range"));
    }
    let row = |u: f64| {
        let f = 1.0 + u * rise;
        let at = |v: V3, s: f64| [v[0] * s, v[1] * s, v[2] * s];
        let cu = [c[0] + axis[0] * u * climb, c[1] + axis[1] * u * climb, c[2] + axis[2] * u * climb];
        helix_fit(cu, at(m, f), at(n, f), p, ends[1])
    };
    let (a, b) = (row(u0)?, row(u1)?);
    Ok(Bs3Surface {
        degree: [1, 3],
        knots: [vec![u0, u1], a.knots],
        mults: [vec![2, 2], a.mults],
        points: vec![a.points, b.points],
        weights: None,
    })
}

/// A B-spline surface (`exactsur`): `[full] nubs|nurbs <du> <dv> <words> <nu> <nv>`, the u
/// then v knot vectors as for curves, then the control points: v fastest in ACIS's own files
/// (each u row lists its points along v), u fastest in ShapeManager's ([`SHAPE_MANAGER`]).
struct Bs3Surface {
    degree: [usize; 2],
    knots: [Vec<f64>; 2],
    mults: [Vec<usize>; 2],
    /// `points[iu][iv]`.
    points: Vec<Vec<V3>>,
    weights: Option<Vec<Vec<f64>>>,
}

/// Most control points one B-spline surface may have.
const MAX_SURFACE_POINTS: usize = 4_000_000;

fn knot_vector(r: &Rec, k: &mut usize, n: usize, degree: usize) -> Result<(Vec<f64>, Vec<usize>), SatError> {
    let bad = |m: &str| SatError::Malformed(format!("{}: {m}", r.ty));
    let mut knots = Vec::with_capacity(n.min(1 << 16));
    let mut mults = Vec::with_capacity(n.min(1 << 16));
    for _ in 0..n {
        let t = r.num(*k).ok_or_else(|| bad("knot"))?;
        let m = r
            .num(*k + 1)
            .map(|m| m as usize)
            .filter(|m| (1..=degree + 1).contains(m))
            .ok_or_else(|| bad("multiplicity"))?;
        if knots.last().is_some_and(|l| t <= *l) {
            return Err(bad("knots do not increase"));
        }
        knots.push(t);
        mults.push(m);
        *k += 2;
    }
    Ok((knots, mults))
}

/// STEP's end knots repeat once more than ACIS's.
fn step_ends(mults: &mut [usize], degree: usize) {
    if let Some(m) = mults.first_mut() {
        *m = (*m + 1).min(degree + 1);
    }
    if let Some(m) = mults.last_mut() {
        *m = (*m + 1).min(degree + 1);
    }
}

fn bs3_surface(r: &Rec, from: usize, u_fastest: bool) -> Result<Bs3Surface, SatError> {
    let bad = |m: &str| SatError::Malformed(format!("{}: {m}", r.ty));
    let mut k = from;
    let mut rational = None;
    for _ in 0..4 {
        match r.word(k) {
            Some("nubs") => rational = Some(false),
            Some("nurbs") => rational = Some(true),
            Some("nullbs") => {
                return Err(SatError::Unsupported("a spline surface without B-spline data is not read yet".into()));
            }
            _ => {}
        }
        k += 1;
        if rational.is_some() {
            break;
        }
    }
    let rational = rational.ok_or_else(|| bad("no B-spline data"))?;
    let deg = |i: usize| r.num(i).map(|d| d as usize).filter(|d| (1..=MAX_DEGREE).contains(d));
    let (du, dv) = (deg(k).ok_or_else(|| bad("u degree"))?, deg(k + 1).ok_or_else(|| bad("v degree"))?);
    k += 2;
    // Rational/closure/singularity words up to the two knot counts.
    let mut words: Vec<String> = Vec::new();
    while r.num(k).is_none() {
        words.push(r.word(k).ok_or_else(|| bad("knot counts"))?.to_string());
        k += 1;
        if words.len() > 8 {
            return Err(bad("knot counts"));
        }
    }
    // The binary form writes the four closure/singularity words as enumeration values
    // (`0 0 0 0` for open, open, none, none): numbers before the counts. Read that layout
    // first when it is there, else the counts come next.
    if words.is_empty()
        && (k..k + 4).all(|i| r.num(i).is_some_and(|v| (0.0..=3.0).contains(&v) && v.fract().abs() < 1e-12))
    {
        let mut kb = k + 4;
        if let Ok(sf) = bs3_surface_rest(r, &mut kb, du, dv, rational, u_fastest) {
            return Ok(sf);
        }
    }
    let mut k1 = k;
    let first = bs3_surface_rest(r, &mut k1, du, dv, rational, u_fastest);
    // A rational surface of the 2025 layout (Fusion's `exact_spl_sur` inside a `pcurve`):
    // `both` and four enumeration values stand before the counts.
    if first.is_err()
        && !words.is_empty()
        && (k..k + 4).all(|i| r.num(i).is_some_and(|v| (0.0..=3.0).contains(&v) && v.fract().abs() < 1e-12))
    {
        let mut kb = k + 4;
        if let Ok(sf) = bs3_surface_rest(r, &mut kb, du, dv, rational, u_fastest) {
            return Ok(sf);
        }
    }
    first
}

/// A B-spline surface from its knot counts on (`k` at the u count).
fn bs3_surface_rest(
    r: &Rec,
    k: &mut usize,
    du: usize,
    dv: usize,
    rational: bool,
    u_fastest: bool,
) -> Result<Bs3Surface, SatError> {
    let bad = |m: &str| SatError::Malformed(format!("{}: {m}", r.ty));
    let count = |i: usize| r.num(i).filter(|n| *n >= 2.0 && *n <= 1e6).map(|n| n as usize);
    let (nu, nv) = (count(*k).ok_or_else(|| bad("u knot count"))?, count(*k + 1).ok_or_else(|| bad("v knot count"))?);
    *k += 2;
    let mut k = *k;
    let (ku, mut mu) = knot_vector(r, &mut k, nu, du)?;
    let (kv, mut mv) = knot_vector(r, &mut k, nv, dv)?;
    let cu = (mu.iter().sum::<usize>() + 1).checked_sub(du).filter(|c| *c > du).ok_or_else(|| bad("too few points"))?;
    let cv = (mv.iter().sum::<usize>() + 1).checked_sub(dv).filter(|c| *c > dv).ok_or_else(|| bad("too few points"))?;
    if cu.saturating_mul(cv) > MAX_SURFACE_POINTS {
        return Err(SatError::TooLarge("a spline surface has too many control points".into()));
    }
    let dim = if rational { 4 } else { 3 };
    let n = cu * cv;
    let mut flat = Vec::with_capacity(n);
    let mut wflat = Vec::with_capacity(if rational { n } else { 0 });
    for _ in 0..n {
        flat.push(r.vec3(k).ok_or_else(|| bad("control point"))?);
        if rational {
            wflat.push(r.num(k + 3).filter(|w| *w > 0.0).ok_or_else(|| bad("weight"))?);
        }
        k += dim;
    }
    // Where point (iu, iv) is in the list.
    let at = |iu: usize, iv: usize| if u_fastest { iv * cu + iu } else { iu * cv + iv };
    let points: Vec<Vec<V3>> =
        (0..cu).map(|iu| (0..cv).filter_map(|iv| flat.get(at(iu, iv)).copied()).collect()).collect();
    let weights: Option<Vec<Vec<f64>>> =
        rational.then(|| (0..cu).map(|iu| (0..cv).filter_map(|iv| wflat.get(at(iu, iv)).copied()).collect()).collect());
    step_ends(&mut mu, du);
    step_ends(&mut mv, dv);
    Ok(Bs3Surface { degree: [du, dv], knots: [ku, kv], mults: [mu, mv], points, weights })
}

/// The first ShapeManager version (Autodesk's ACIS, in Fusion's and Inventor's files: 222.0 and
/// on are read). Its B-spline surfaces list their control points u fastest (each v row along
/// u): only in that order do the faces' vertices, and the parameter-space curves stored for
/// their coedges, lie on the surface (ADR 0039 amendment). ACIS's own files (NIST's, versions 1
/// to 7) list them v fastest.
const SHAPE_MANAGER: u32 = 20_000;

/// Most entries walked along one chain (faces of a shell, coedges of a loop, ...).
const MAX_CHAIN: usize = 10_000_000;
/// Most faces one file may produce.
const MAX_FACES: usize = 2_000_000;

/// Where version-dependent fields are.
#[derive(Clone, Copy)]
struct Layout {
    edge_end: usize,
    edge_curve: usize,
    edge_sense: usize,
}

impl Layout {
    fn of(version: u32) -> Layout {
        if version < 400 {
            Layout { edge_end: 2, edge_curve: 4, edge_sense: 5 }
        } else {
            Layout { edge_end: 3, edge_curve: 6, edge_sense: 7 }
        }
    }
}

/// A sense field: `reversed` (or `1` in version 1 files) against `forward` / `0`.
fn reversed(w: Option<&str>) -> bool {
    matches!(w, Some("reversed" | "1" | "T"))
}

struct Writer<'a> {
    sat: &'a Sat,
    lay: Layout,
    out: String,
    next: usize,
    xf: Xf,
    vertices: BTreeMap<i64, usize>,
    edges: BTreeMap<i64, usize>,
    curves: BTreeMap<i64, (usize, bool)>,
    surfaces: BTreeMap<i64, (usize, bool)>,
    faces: usize,
    /// Faces left out for geometry not read yet.
    skipped: usize,
    /// Coedges per edge in the shell being written (an edge used once is a free edge).
    uses: BTreeMap<i64, u32>,
    /// Shells, faces, loops and coedges already walked in the body being written: a file
    /// that shares them (or nests subshells as a lattice) is walked once, not exponentially.
    seen: std::collections::BTreeSet<i64>,
    /// Lumps already written (by any body).
    lumps: std::collections::BTreeSet<i64>,
    /// Surfaces and curves that could not be read, with why (each is tried once).
    failed: BTreeMap<i64, String>,
    /// B-spline surfaces by their definition block (`{ ref n }` shares one).
    defs: BTreeMap<(usize, usize), usize>,
    /// B-spline surfaces list their control points u fastest ([`SHAPE_MANAGER`]).
    u_fastest: bool,
    /// The output passed [`MAX_OUTPUT`].
    full: bool,
    /// Material colours, the colour of the body being written, and the faces written with
    /// theirs.
    colours: &'a crate::Colours,
    body_colour: Option<u32>,
    /// Colour of each body (by record) from its feature's body references, and of the body
    /// being written: the colour of its faces that have no material.
    body_defaults: std::collections::HashMap<i64, u32>,
    body_default: Option<u32>,
    /// Material id per entity (record number).
    materials: std::collections::HashMap<i64, &'a str>,
    styled: Vec<(usize, u32)>,
    warnings: Vec<String>,
}

impl<'a> Writer<'a> {
    fn add(&mut self, e: &str) -> usize {
        self.next += 1;
        if self.out.len() + e.len() > MAX_OUTPUT {
            self.full = true;
        } else {
            let _ = writeln!(self.out, "#{}={e};", self.next);
        }
        self.next
    }

    fn warn(&mut self, m: String) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(m);
        }
    }

    fn check_full(&self) -> Result<(), SatError> {
        if self.full {
            Err(SatError::TooLarge(format!("the converted model passes {} MiB", MAX_OUTPUT >> 20)))
        } else {
            Ok(())
        }
    }

    fn rec(&self, i: i64) -> Result<&'a Rec, SatError> {
        let sat: &'a Sat = self.sat;
        sat.rec(i).ok_or_else(|| SatError::Malformed(format!("record ${i} does not exist")))
    }

    fn rec_of(&self, i: i64, ty: &str) -> Result<&'a Rec, SatError> {
        let r = self.rec(i)?;
        // Tolerant entities are subtypes: `tedge-edge`, `tcoedge-coedge`, `tvertex-vertex`.
        if r.ty == ty || r.ty.strip_suffix(ty).is_some_and(|p| p.ends_with('-')) {
            Ok(r)
        } else {
            Err(SatError::Malformed(format!("record ${i} is a {}, not a {ty}", r.ty)))
        }
    }

    fn chain(&self, first: Option<i64>, next_field: usize) -> Result<Vec<i64>, SatError> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut at = first;
        while let Some(i) = at {
            if !seen.insert(i) {
                break;
            }
            if out.len() >= MAX_CHAIN {
                return Err(SatError::TooLarge("a chain of entities is too long".into()));
            }
            out.push(i);
            at = self.rec(i)?.ptr(next_field);
        }
        Ok(out)
    }

    /// The subtype block of a record: its own `{ ...` or the definition `{ ref n }` names,
    /// as (record, field of the `{`).
    fn subtype(&self, r: &'a Rec) -> Result<(&'a Rec, usize), SatError> {
        let open = (1..8)
            .find(|i| r.word(*i) == Some("{"))
            .ok_or_else(|| SatError::Malformed(format!("{} has no data block", r.ty)))?;
        if r.word(open + 1) != Some("ref") {
            return Ok((r, open));
        }
        let n = r.num(open + 2).filter(|n| *n >= 0.0).ok_or_else(|| SatError::Malformed("a bad ref".into()))? as usize;
        let (ri, fi) =
            *self.sat.subtypes.get(n).ok_or_else(|| SatError::Malformed(format!("ref {n} names no subtype")))?;
        let sat: &'a Sat = self.sat;
        let def = sat.recs.get(ri).ok_or_else(|| SatError::Malformed("a bad ref".into()))?;
        Ok((def, fi))
    }

    /// The block (`{ ...` or the definition a `{ ref n }` names) that opens at field `at`.
    fn block(&self, r: &'a Rec, at: usize) -> Option<(&'a Rec, usize)> {
        if r.word(at) != Some("{") {
            return None;
        }
        if r.word(at + 1) != Some("ref") {
            return Some((r, at));
        }
        let n = r.num(at + 2).filter(|n| *n >= 0.0)? as usize;
        let (ri, fi) = *self.sat.subtypes.get(n)?;
        let sat: &'a Sat = self.sat;
        Some((sat.recs.get(ri)?, fi))
    }

    /// An `int_int_cur`'s fit `fit` rebuilt on its two surfaces where both are planes or
    /// revolved exact B-splines (see [`exact`]); `end` is the field after the fit's data (its
    /// fit tolerance, then the surfaces). `None`: the fit is used as it is.
    fn exact_intersection(&self, def: &'a Rec, end: usize, fit: &Bs3) -> Option<Bs3> {
        let (a, next) = self.exact_surface(def, end + 1)?;
        // The second surface follows the first one's flag and range bounds.
        let at = (next..next + 12).find(|i| matches!(def.word(*i), Some("plane" | "spline")))?;
        let (b, _) = self.exact_surface(def, at)?;
        exact::refit(fit, &a, &b)
    }

    /// A plane or a `rot_spl_sur` spline surface of an exact curve, written inline at field
    /// `at`, and the field after its data.
    fn exact_surface(&self, def: &'a Rec, at: usize) -> Option<(exact::Exact, usize)> {
        match def.word(at)? {
            "plane" => Some((exact::Exact::plane(def.vec3(at + 1)?, def.vec3(at + 4)?)?, at + 10)),
            "spline" => {
                let open = (at + 1..at + 4).find(|i| def.word(*i) == Some("{"))?;
                let next = block_end(def, open)? + 1;
                let (sdef, sopen) = self.block(def, open)?;
                if sdef.word(sopen + 1) != Some("rot_spl_sur") {
                    return None;
                }
                // `rot_spl_sur <version> intcurve <flag> { exact_int_cur ... } <range> <axis>`
                let popen = (sopen + 2..sopen + 6).find(|i| sdef.word(*i) == Some("{"))?;
                let (pdef, pat) = self.block(sdef, popen)?;
                if !matches!(pdef.word(pat + 1), Some("exact_int_cur" | "exactcur")) {
                    return None;
                }
                let profile = bs3_curve(pdef, pat + 1).ok()?;
                let mut k = block_end(sdef, popen)? + 1;
                for _ in 0..2 {
                    k += match sdef.word(k)? {
                        "I" => 1,
                        "T" | "F" => 2,
                        _ => return None,
                    };
                }
                Some((exact::Exact::revolution(sdef.vec3(k)?, sdef.vec3(k + 3)?, profile)?, next))
            }
            _ => None,
        }
    }

    fn point(&mut self, p: V3) -> usize {
        let p = self.xf.point(p);
        self.add(&format!("CARTESIAN_POINT('',({},{},{}))", real(p[0]), real(p[1]), real(p[2])))
    }

    fn direction(&mut self, d: V3) -> Result<usize, SatError> {
        let d = self.xf.dir(d).ok_or_else(|| SatError::Malformed("a zero direction".into()))?;
        Ok(self.add(&format!("DIRECTION('',({},{},{}))", real(d[0]), real(d[1]), real(d[2]))))
    }

    fn placement(&mut self, origin: V3, axis: V3, refd: V3) -> Result<usize, SatError> {
        let o = self.point(origin);
        let a = self.direction(axis)?;
        // A missing or parallel reference direction (old files write planes with a zero u
        // direction): any direction across the axis.
        let ax = unit(axis).unwrap_or([0.0, 0.0, 1.0]);
        let refd = match unit(refd) {
            Some(r) if (r[0] * ax[0] + r[1] * ax[1] + r[2] * ax[2]).abs() < 1.0 - 1e-9 => r,
            _ => {
                let pick = if ax[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                [
                    pick[1] * ax[2] - pick[2] * ax[1],
                    pick[2] * ax[0] - pick[0] * ax[2],
                    pick[0] * ax[1] - pick[1] * ax[0],
                ]
            }
        };
        let r = self.direction(refd)?;
        Ok(self.add(&format!("AXIS2_PLACEMENT_3D('',#{o},#{a},#{r})")))
    }

    fn vertex(&mut self, v: i64) -> Result<usize, SatError> {
        if let Some(id) = self.vertices.get(&v) {
            return Ok(*id);
        }
        let r = self.rec_of(v, "vertex")?;
        // Newer versions write an integer between the edge and the point.
        let at = if r.word(2).is_some_and(|w| w.parse::<i64>().is_ok()) { 3 } else { 2 };
        let p = r.ptr(at).ok_or_else(|| SatError::Malformed(format!("vertex ${v} has no point")))?;
        let pr = self.rec_of(p, "point")?;
        let xyz = pr.vec3(1).ok_or_else(|| SatError::Malformed(format!("point ${p} has no coordinates")))?;
        let cp = self.point(xyz);
        let id = self.add(&format!("VERTEX_POINT('',#{cp})"));
        self.vertices.insert(v, id);
        Ok(id)
    }

    /// The STEP curve and whether it runs against the ACIS curve.
    fn curve(&mut self, c: i64) -> Result<(usize, bool), SatError> {
        if let Some(v) = self.curves.get(&c) {
            return Ok(*v);
        }
        if let Some(m) = self.failed.get(&c) {
            return Err(SatError::Unsupported(m.clone()));
        }
        let r = self.curve_once(c);
        if let Err(SatError::Unsupported(m)) = &r {
            self.failed.insert(c, m.clone());
        }
        r
    }

    fn curve_once(&mut self, c: i64) -> Result<(usize, bool), SatError> {
        let r = self.rec(c)?;
        let bad = || SatError::Malformed(format!("{} ${c} is incomplete", r.ty));
        let mut against = false;
        let id = match r.ty.as_str() {
            "intcurve-curve" => {
                // `forward`/`reversed` (or `0`/`1`): the curve against its B-spline.
                against = reversed(r.word(1));
                let (def, open) = self.subtype(r)?;
                let b = if def.word(open + 1) == Some("helix_int_cur") {
                    helix(def, open + 2)?
                } else if def.word(open + 1) == Some("int_int_cur") {
                    let (b, end) = bs3_curve_end(def, open + 1)?;
                    self.exact_intersection(def, end, &b).unwrap_or(b)
                } else {
                    bs3_curve(def, open + 1)?
                };
                self.bspline_curve(&b)
            }
            "straight-curve" => {
                let root = r.vec3(1).ok_or_else(bad)?;
                let dir = r.vec3(4).ok_or_else(bad)?;
                let p = self.point(root);
                let d = self.direction(dir)?;
                let v = self.add(&format!("VECTOR('',#{d},1.)"));
                self.add(&format!("LINE('',#{p},#{v})"))
            }
            "ellipse-curve" => {
                let centre = r.vec3(1).ok_or_else(bad)?;
                let normal = r.vec3(4).ok_or_else(bad)?;
                let major = r.vec3(7).ok_or_else(bad)?;
                let ratio = r.num(10).unwrap_or(1.0);
                let a = self.xf.length(len(major));
                let pl = self.placement(centre, normal, major)?;
                if (ratio - 1.0).abs() < 1e-12 {
                    self.add(&format!("CIRCLE('',#{pl},{})", real(a)))
                } else {
                    self.add(&format!("ELLIPSE('',#{pl},{},{})", real(a), real(a * ratio.abs())))
                }
            }
            other => {
                return Err(SatError::Unsupported(format!(
                    "ACIS curves of type '{other}' are not read yet (only lines, circles and ellipses)"
                )));
            }
        };
        self.curves.insert(c, (id, against));
        Ok((id, against))
    }

    fn bspline_curve(&mut self, b: &Bs3) -> usize {
        let pts: Vec<usize> = b.points.iter().map(|p| self.point(*p)).collect();
        let mults: Vec<String> = b.mults.iter().map(usize::to_string).collect();
        let knots: Vec<String> = b.knots.iter().map(|k| real(*k)).collect();
        let (m, k) = (mults.join(","), knots.join(","));
        match &b.weights {
            None => self.add(&format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',{},{},.UNSPECIFIED.,.F.,.F.,({m}),({k}),.UNSPECIFIED.)",
                b.degree,
                list(&pts)
            )),
            Some(w) => {
                let ws: Vec<String> = w.iter().map(|x| real(*x)).collect();
                self.add(&format!(
                    "(BOUNDED_CURVE() B_SPLINE_CURVE({},{},.UNSPECIFIED.,.F.,.F.) B_SPLINE_CURVE_WITH_KNOTS(({m}),({k}),.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({})) REPRESENTATION_ITEM(''))",
                    b.degree,
                    list(&pts),
                    ws.join(",")
                ))
            }
        }
    }

    /// The point of a vertex (model units).
    fn vertex_xyz(&self, v: i64) -> Option<V3> {
        let r = self.sat.rec(v).filter(|r| r.ty == "vertex")?;
        let at = if r.word(2).is_some_and(|w| w.parse::<i64>().is_ok()) { 3 } else { 2 };
        self.sat.rec(r.ptr(at)?).filter(|r| r.ty == "point")?.vec3(1)
    }

    /// An edge on a closed B-spline intcurve whose parameter range runs over the curve's end
    /// (ShapeManager writes such curves periodic, one turn of knots, and an edge's range past
    /// that turn): its own piece of the curve, from its start vertex to its end vertex. OCCT
    /// would otherwise place the vertices on the one turn and take the other side of the
    /// curve between them (BQ25185 Buck Case: every edge of a body's rounded outline, its
    /// faces the rest of their surfaces). The piece must start and end at the vertices.
    fn edge_past_curve_end(&mut self, r: &Rec, v0: i64, v1: i64, c: i64) -> Option<usize> {
        let (t0, t1) = (r.num(2)?, r.num(self.lay.edge_end + 1)?);
        let cr = self.sat.rec(c).filter(|cr| cr.ty == "intcurve-curve")?;
        let (def, open) = self.subtype(cr).ok()?;
        if def.word(open + 1) == Some("helix_int_cur") {
            return None;
        }
        let b = bs3_curve(def, open + 1).ok()?;
        let (k0, kn) = (*b.knots.first()?, *b.knots.last()?);
        let (p0, p1) = (self.vertex_xyz(v0)?, self.vertex_xyz(v1)?);
        let span = b.points.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs())).max(1.0);
        let near = |p: V3, q: V3| p.iter().zip(q).all(|(x, y)| (x - y).abs() <= 1e-6 * span);
        // The B-spline's parameters of the two ends: the edge's own or their negatives (an
        // intcurve or edge reversed), whichever puts the vertices at the piece's ends.
        for sign in [1.0, -1.0] {
            let (s0, s1) = (sign * t0, sign * t1);
            let (lo, hi) = (s0.min(s1), s0.max(s1));
            let tol = 1e-12 * (kn - k0).abs().max(1.0);
            if lo >= k0 - tol && hi <= kn + tol {
                continue;
            }
            let Some(part) = closed_bs3_part(&b, lo, hi) else { continue };
            let (first, last) = (*part.points.first()?, *part.points.last()?);
            let (start, end) = if s0 <= s1 { (first, last) } else { (last, first) };
            if near(start, p0) && near(end, p1) {
                let (a, b) = (*self.vertices.get(&v0)?, *self.vertices.get(&v1)?);
                let cid = self.bspline_curve(&part);
                let same = if s0 <= s1 { ".T." } else { ".F." };
                return Some(self.add(&format!("EDGE_CURVE('',#{a},#{b},#{cid},{same})")));
            }
        }
        None
    }

    fn edge(&mut self, e: i64) -> Result<usize, SatError> {
        if let Some(id) = self.edges.get(&e) {
            return Ok(*id);
        }
        let r = self.rec_of(e, "edge")?;
        let bad = |w: &str| SatError::Malformed(format!("edge ${e} has no {w}"));
        let (sv0, sv1) =
            (r.ptr(1).ok_or_else(|| bad("start vertex"))?, r.ptr(self.lay.edge_end).ok_or_else(|| bad("end vertex"))?);
        let v0 = self.vertex(sv0)?;
        let v1 = self.vertex(sv1)?;
        let curve = r.ptr(self.lay.edge_curve).ok_or_else(|| bad("curve"))?;
        if self.lay.edge_end == 3
            && let Some(id) = self.edge_past_curve_end(r, sv0, sv1, curve)
        {
            self.edges.insert(e, id);
            return Ok(id);
        }
        let (c, against) = self.curve(curve)?;
        let same = reversed(r.word(self.lay.edge_sense)) == against;
        let id = self.add(&format!("EDGE_CURVE('',#{v0},#{v1},#{c},{})", if same { ".T." } else { ".F." }));
        self.edges.insert(e, id);
        Ok(id)
    }

    /// The STEP surface and whether its normal is the opposite of the ACIS surface's.
    fn surface(&mut self, s: i64) -> Result<(usize, bool), SatError> {
        if let Some(v) = self.surfaces.get(&s) {
            return Ok(*v);
        }
        if let Some(m) = self.failed.get(&s) {
            return Err(SatError::Unsupported(m.clone()));
        }
        let r = self.surface_once(s);
        if let Err(SatError::Unsupported(m)) = &r {
            self.failed.insert(s, m.clone());
        }
        r
    }

    fn surface_once(&mut self, s: i64) -> Result<(usize, bool), SatError> {
        let r = self.rec(s)?;
        let bad = || SatError::Malformed(format!("{} ${s} is incomplete", r.ty));
        let v = match r.ty.as_str() {
            "plane-surface" => {
                let root = r.vec3(1).ok_or_else(bad)?;
                let normal = r.vec3(4).ok_or_else(bad)?;
                let udir = r.vec3(7).ok_or_else(bad)?;
                let pl = self.placement(root, normal, udir)?;
                (self.add(&format!("PLANE('',#{pl})")), false)
            }
            "cone-surface" => {
                let c = r.vec3(1).ok_or_else(bad)?;
                let n = r.vec3(4).ok_or_else(bad)?;
                let major = r.vec3(7).ok_or_else(bad)?;
                let ratio = r.num(10).ok_or_else(bad)?;
                if (ratio - 1.0).abs() > 1e-9 {
                    return Err(SatError::Unsupported("elliptical ACIS cones are not read yet".into()));
                }
                // The interval: two ends, each `I` or `F <value>`.
                let mut k = 11;
                // Version 1.05 cones have no interval; 1.06 and later do.
                if matches!(r.word(k), Some("I" | "F" | "T")) {
                    for _ in 0..2 {
                        k += if matches!(r.word(k), Some("F" | "T")) { 2 } else { 1 };
                    }
                }
                let sin = r.num(k).ok_or_else(bad)?;
                let cos = r.num(k + 1).ok_or_else(bad)?;
                let radius = self.xf.length(len(major));
                let flipped = cos < 0.0;
                if sin.abs() < 1e-12 {
                    let pl = self.placement(c, n, major)?;
                    (self.add(&format!("CYLINDRICAL_SURFACE('',#{pl},{})", real(radius))), flipped)
                } else {
                    let slope = sin / cos;
                    let axis = if slope > 0.0 { n } else { [-n[0], -n[1], -n[2]] };
                    let pl = self.placement(c, axis, major)?;
                    let angle = libm::atan(slope.abs());
                    (self.add(&format!("CONICAL_SURFACE('',#{pl},{},{})", real(radius), real(angle))), flipped)
                }
            }
            "sphere-surface" => {
                let c = r.vec3(1).ok_or_else(bad)?;
                let radius = r.num(4).ok_or_else(bad)?;
                let uref = r.vec3(5).ok_or_else(bad)?;
                let pole = r.vec3(8).ok_or_else(bad)?;
                let pl = self.placement(c, pole, uref)?;
                // A negative radius turns the normal inward.
                (self.add(&format!("SPHERICAL_SURFACE('',#{pl},{})", real(self.xf.length(radius.abs())))), radius < 0.0)
            }
            "torus-surface" => {
                let c = r.vec3(1).ok_or_else(bad)?;
                let n = r.vec3(4).ok_or_else(bad)?;
                let major = r.num(7).ok_or_else(bad)?;
                let minor = r.num(8).ok_or_else(bad)?;
                let uref = r.vec3(9).ok_or_else(bad)?;
                let pl = self.placement(c, n, uref)?;
                let (big, small) = (self.xf.length(major.abs()), self.xf.length(minor.abs()));
                (self.add(&format!("TOROIDAL_SURFACE('',#{pl},{},{})", real(big), real(small))), minor < 0.0)
            }
            "spline-surface" => {
                let against = reversed(r.word(1));
                let (def, at) = self.subtype(r)?;
                // A definition shared through `{ ref n }` is written once.
                let key = (std::ptr::from_ref(def) as usize, at);
                if let Some(id) = self.defs.get(&key) {
                    return Ok((*id, against));
                }
                let name = def.word(at + 1).unwrap_or("").to_string();
                let b = if name == "exactsur" || name == "exact_spl_sur" {
                    bs3_surface(def, at + 2, self.u_fastest)?
                } else if name == "helix_spl_line" {
                    let note =
                        "'helix_spl_line' surfaces (threads) are fitted as B-splines within 0.001 mm".to_string();
                    if !self.warnings.contains(&note) {
                        self.warn(note);
                    }
                    helix_surface(def, at + 2)?
                } else {
                    let b = own_approximation(def, at, self.u_fastest).ok_or_else(|| {
                        SatError::Unsupported(format!(
                            "ACIS spline surfaces of kind '{name}' without a B-spline approximation are not read"
                        ))
                    })??;
                    if name == "rb_blend_spl_sur"
                        && let Some(t) = blend_torus(def, at, &b)
                    {
                        let pl = self.placement(t.centre, t.axis, t.start)?;
                        let (big, small) = (self.xf.length(t.major), self.xf.length(t.minor));
                        let v = (
                            self.add(&format!("TOROIDAL_SURFACE('',#{pl},{},{})", real(big), real(small))),
                            against != t.inward,
                        );
                        self.surfaces.insert(s, v);
                        return Ok(v);
                    }
                    let note = format!("'{name}' surfaces are read as their stored B-spline approximation");
                    if !self.warnings.contains(&note) {
                        self.warn(note);
                    }
                    b
                };
                let rows: Vec<String> = b
                    .points
                    .iter()
                    .map(|row| {
                        let ids: Vec<usize> = row.iter().map(|p| self.point(*p)).collect();
                        list(&ids)
                    })
                    .collect();
                let ints = |v: &[usize]| v.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
                let reals = |v: &[f64]| v.iter().map(|x| real(*x)).collect::<Vec<_>>().join(",");
                let tail = format!(
                    "({}),({}),({}),({}),.UNSPECIFIED.",
                    ints(&b.mults[0]),
                    ints(&b.mults[1]),
                    reals(&b.knots[0]),
                    reals(&b.knots[1])
                );
                let id = match &b.weights {
                    None => self.add(&format!(
                        "B_SPLINE_SURFACE_WITH_KNOTS('',{},{},({}),.UNSPECIFIED.,.F.,.F.,.F.,{tail})",
                        b.degree[0],
                        b.degree[1],
                        rows.join(",")
                    )),
                    Some(w) => {
                        let wr: Vec<String> = w.iter().map(|row| format!("({})", reals(row))).collect();
                        self.add(&format!(
                            "(BOUNDED_SURFACE() B_SPLINE_SURFACE({},{},({}),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS({tail}) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({})) REPRESENTATION_ITEM('') SURFACE())",
                            b.degree[0],
                            b.degree[1],
                            rows.join(","),
                            wr.join(",")
                        ))
                    }
                };
                self.defs.insert(key, id);
                (id, against)
            }
            other => {
                return Err(SatError::Unsupported(format!(
                    "ACIS surfaces of type '{other}' are not read yet (only planes, cylinders, cones, spheres, tori and B-spline surfaces)"
                )));
            }
        };
        self.surfaces.insert(s, v);
        Ok(v)
    }

    fn face(&mut self, f: i64) -> Result<usize, SatError> {
        self.faces += 1;
        if self.faces > MAX_FACES {
            return Err(SatError::TooLarge(format!("more than {MAX_FACES} faces")));
        }
        self.check_full()?;
        let r = self.rec_of(f, "face")?;
        let surf = r.ptr(5).ok_or_else(|| SatError::Malformed(format!("face ${f} has no surface")))?;
        let (sid, flipped) = self.surface(surf)?;
        // Each loop: its oriented edges and the vertex it starts at, or the apex of a loop that
        // is a point.
        let mut loops: Vec<(Vec<usize>, Option<i64>, Option<i64>)> = Vec::new();
        for l in self.chain(r.ptr(2), 1)? {
            if !self.seen.insert(l) {
                return Err(SatError::Malformed(format!("loop ${l} belongs to more than one face")));
            }
            let lr = self.rec_of(l, "loop")?;
            let mut oriented = Vec::new();
            let mut apex = None;
            let mut start = None;
            for ce in self.chain(lr.ptr(2), 1)? {
                if !self.seen.insert(ce) {
                    return Err(SatError::Malformed(format!("coedge ${ce} belongs to more than one loop")));
                }
                let cr = self.rec_of(ce, "coedge")?;
                let e = cr.ptr(4).ok_or_else(|| SatError::Malformed(format!("coedge ${ce} has no edge")))?;
                // An edge without a curve is a point (a cone's apex): no edge in STEP.
                let er = self.rec_of(e, "edge")?;
                if er.ptr(self.lay.edge_curve).is_none() {
                    apex = er.ptr(1);
                    continue;
                }
                let eid = self.edge(e)?;
                *self.uses.entry(e).or_default() += 1;
                let fwd = !reversed(cr.word(5));
                if start.is_none() {
                    start = if fwd { er.ptr(1) } else { er.ptr(self.lay.edge_end) };
                }
                oriented.push(self.add(&format!("ORIENTED_EDGE('',*,*,#{eid},{})", if fwd { ".T." } else { ".F." })));
            }
            loops.push((oriented, start, apex));
        }
        // A torus face that reaches the axis at a point (a spindle torus: 3722's dome): OCCT's
        // STEP reader drops a vertex loop on a torus, and its repair then bounds the face by
        // the other point where the tube crosses the axis (the dome came in as the bowl under
        // it, 46358 mm3 for 725030). The point is written as OCCT writes such faces instead:
        // a seam from the other loop's start to the point and back, an arc of the tube's
        // circle; the reader puts the degenerate edge at the point.
        let edged: Vec<usize> = (0..loops.len()).filter(|k| loops.get(*k).is_some_and(|l| !l.0.is_empty())).collect();
        let pointed: Vec<usize> =
            (0..loops.len()).filter(|k| loops.get(*k).is_some_and(|l| l.0.is_empty() && l.2.is_some())).collect();
        if let ([e], [p]) = (edged.as_slice(), pointed.as_slice())
            && let Some(start) = loops.get(*e).and_then(|l| l.1)
            && let Some(apex) = loops.get(*p).and_then(|l| l.2)
            && let Some(seam) = self.torus_seam(surf, start, apex)?
        {
            let there = self.add(&format!("ORIENTED_EDGE('',*,*,#{seam},.T.)"));
            let back = self.add(&format!("ORIENTED_EDGE('',*,*,#{seam},.F.)"));
            if let Some(l) = loops.get_mut(*e) {
                l.0.extend([there, back]);
            }
            loops.retain(|l| !l.0.is_empty());
        }
        let mut bounds = Vec::new();
        for (oriented, _, apex) in loops {
            if oriented.is_empty() {
                if let Some(v) = apex {
                    let vp = self.vertex(v)?;
                    let vl = self.add(&format!("VERTEX_LOOP('',#{vp})"));
                    bounds.push(self.add(&format!("FACE_BOUND('',#{vl},.T.)")));
                }
                continue;
            }
            let lp = self.add(&format!("EDGE_LOOP('',{})", list(&oriented)));
            bounds.push(self.add(&format!("FACE_BOUND('',#{lp},.T.)")));
        }
        let same = reversed(r.word(6)) == flipped;
        let id = self.add(&format!("ADVANCED_FACE('',{},#{sid},{})", list(&bounds), if same { ".T." } else { ".F." }));
        if !self.colours.is_empty() || !self.body_defaults.is_empty() {
            let own = self.materials.get(&f).and_then(|m| self.colours.get(*m)).copied();
            if let Some(c) = own.or(self.body_colour).or(self.body_default) {
                self.styled.push((id, c));
            }
        }
        Ok(id)
    }

    /// The seam of a torus face from the vertex `start` to the point `apex` where the tube
    /// crosses the axis ([`Self::face`]): an edge on the arc of the tube's circle through
    /// `start` that does not pass the other crossing. `None` where the surface is no spindle
    /// torus or the points are not on it (1e-6 of its size).
    fn torus_seam(&mut self, surf: i64, start: i64, apex: i64) -> Result<Option<usize>, SatError> {
        let Some(r) = self.sat.rec(surf).filter(|r| r.ty == "torus-surface") else { return Ok(None) };
        let (Some(c), Some(n), Some(major), Some(minor)) = (r.vec3(1), r.vec3(4), r.num(7), r.num(8)) else {
            return Ok(None);
        };
        let (Some(v), Some(a)) = (self.vertex_xyz(start), self.vertex_xyz(apex)) else { return Ok(None) };
        // In the STEP file's millimetres.
        let (c, v, a) = (self.xf.point(c), self.xf.point(v), self.xf.point(a));
        let Some(n) = self.xf.dir(n) else { return Ok(None) };
        let (big, small) = (self.xf.length(major.abs()), self.xf.length(minor.abs()));
        if big >= small {
            return Ok(None);
        }
        let tol = 1e-6 * small.max(1.0);
        let along = |p: V3| dot(sub(p, c), n);
        let across = |p: V3| {
            let h = along(p);
            let d = sub(p, c);
            [d[0] - h * n[0], d[1] - h * n[1], d[2] - h * n[2]]
        };
        // The apex lies on the axis, where the tube's circle crosses it.
        let crossing = (small * small - big * big).sqrt();
        if len(across(a)) > tol || (along(a).abs() - crossing).abs() > tol {
            return Ok(None);
        }
        let Some(x) = unit(across(v)) else { return Ok(None) };
        // The tube's circle through `start`: centred `big` from the axis on its side, or on the
        // other (a lemon's part of the tube).
        let centre = |s: f64| [c[0] + s * big * x[0], c[1] + s * big * x[1], c[2] + s * big * x[2]];
        let Some(side) = [1.0, -1.0].into_iter().find(|s| (len(sub(v, centre(*s))) - small).abs() <= tol) else {
            return Ok(None);
        };
        let m = centre(side);
        // Angles about the circle's centre: x outward, n along the axis.
        let angle = |p: V3| {
            let d = sub(p, m);
            libm::atan2(dot(d, n), dot(d, x))
        };
        let h = along(a);
        let other = [c[0] - h * n[0], c[1] - h * n[1], c[2] - h * n[2]];
        let ccw = |from: f64, to: f64| (to - from).rem_euclid(2.0 * core::f64::consts::PI);
        let (tv, ta, to) = (angle(v), angle(a), angle(other));
        // Counterclockwise from `start` to the apex unless the other crossing lies on the way.
        let forward = ccw(tv, to) > ccw(tv, ta);
        let fmt = |p: V3| format!("({},{},{})", real(p[0]), real(p[1]), real(p[2]));
        let o = self.add(&format!("CARTESIAN_POINT('',{})", fmt(m)));
        // The axis x cross n, so that the circle runs from x towards n.
        let ad = self.add(&format!("DIRECTION('',{})", fmt(cross(x, n))));
        let rd = self.add(&format!("DIRECTION('',{})", fmt(x)));
        let pl = self.add(&format!("AXIS2_PLACEMENT_3D('',#{o},#{ad},#{rd})"));
        let circle = self.add(&format!("CIRCLE('',#{pl},{})", real(small)));
        let (v0, v1) = (self.vertex(start)?, self.vertex(apex)?);
        Ok(Some(self.add(&format!("EDGE_CURVE('',#{v0},#{v1},#{circle},{})", if forward { ".T." } else { ".F." }))))
    }

    /// Gives the faces without a material of their own the one the history blob has for their
    /// tag (see [`crate::History`]).
    fn inherit_history(&mut self, h: &'a crate::History) {
        let mut faces: std::collections::HashMap<&str, Vec<i64>> = std::collections::HashMap::new();
        let tags = self.sat.face_tags();
        for (face, tag) in &tags {
            if let Some((t, _)) = h.by_tag.get_key_value(tag.as_str()) {
                faces.entry(t.as_str()).or_default().push(*face);
            }
        }
        let unique = faces.values().filter(|v| v.len() == 1).count();
        if unique * 2 < h.by_tag.len() {
            return;
        }
        for (tag, list) in faces {
            if let ([face], Some(m)) = (list.as_slice(), h.by_tag.get(tag)) {
                self.materials.entry(*face).or_insert(m.as_str());
            }
        }
    }

    /// The coloured faces as AP214 styled items, one style per colour.
    fn face_colours(&mut self, ctx: usize) {
        let mut styles: BTreeMap<u32, usize> = BTreeMap::new();
        let mut items = Vec::new();
        for (face, rgb) in std::mem::take(&mut self.styled) {
            let style = match styles.get(&rgb) {
                Some(&st) => st,
                None => {
                    let [r, g, b] = [16, 8, 0].map(|sh| real(f64::from((rgb >> sh) & 0xff) / 255.0));
                    let c = self.add(&format!("COLOUR_RGB('',{r},{g},{b})"));
                    let fc = self.add(&format!("FILL_AREA_STYLE_COLOUR('',#{c})"));
                    let fa = self.add(&format!("FILL_AREA_STYLE('',(#{fc}))"));
                    let sf = self.add(&format!("SURFACE_STYLE_FILL_AREA(#{fa})"));
                    let side = self.add(&format!("SURFACE_SIDE_STYLE('',(#{sf}))"));
                    let usage = self.add(&format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
                    let st = self.add(&format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
                    styles.insert(rgb, st);
                    st
                }
            };
            items.push(self.add(&format!("STYLED_ITEM('color',(#{style}),#{face})")));
        }
        if !items.is_empty() {
            self.add(&format!("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',{},#{ctx})", list(&items)));
        }
    }

    /// Faces of a shell and its subshells (nested at most 64 deep).
    fn shell_faces(&mut self, sh: i64, depth: u32, out: &mut Vec<i64>) -> Result<(), SatError> {
        if depth > 64 {
            return Err(SatError::Malformed("subshells nest too deep".into()));
        }
        if !self.seen.insert(sh) {
            return Ok(());
        }
        let r = self.rec(sh)?;
        let faces_at = if r.ty == "shell" { 3 } else { 2 };
        for f in self.chain(r.ptr(faces_at), 1)? {
            if self.seen.insert(f) {
                out.push(f);
            }
        }
        let sub_at = if r.ty == "shell" { 2 } else { 3 };
        for s in self.chain(r.ptr(sub_at), 1)? {
            self.shell_faces(s, depth + 1, out)?;
        }
        Ok(())
    }
}

fn transform(sat: &Sat, body: &Rec) -> Xf {
    let unit = sat.scale;
    let ident = Xf { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3], s: 1.0, unit };
    let Some(t) = body.ptr(3).and_then(|i| sat.rec(i)).filter(|r| r.ty == "transform") else { return ident };
    let (Some(a), Some(b), Some(c), Some(tr)) = (t.vec3(1), t.vec3(4), t.vec3(7), t.vec3(10)) else { return ident };
    let s = t.num(13).filter(|s| s.abs() > 1e-300).unwrap_or(1.0);
    Xf { r: [a, b, c], t: tr, s, unit }
}

/// `xf` followed by the placement `p` (millimetres, column vectors).
fn placed(xf: Xf, p: &crate::fusion::Placement) -> Xf {
    // Row vectors: p' = q Rᵀ + t, so the combined matrix is R_xf Rᵀ and the translation
    // t_xf Rᵀ + t / unit (the writer scales by the unit last).
    let rt = |v: V3| {
        [
            v[0] * p.r[0][0] + v[1] * p.r[0][1] + v[2] * p.r[0][2],
            v[0] * p.r[1][0] + v[1] * p.r[1][1] + v[2] * p.r[1][2],
            v[0] * p.r[2][0] + v[1] * p.r[2][1] + v[2] * p.r[2][2],
        ]
    };
    let u = if xf.unit.abs() > 1e-300 { xf.unit } else { 1.0 };
    let t = rt(xf.t);
    Xf { r: xf.r.map(rt), t: [t[0] + p.t[0] / u, t[1] + p.t[1] / u, t[2] + p.t[2] / u], s: xf.s, unit: xf.unit }
}

pub(crate) fn write(
    sat: &Sat,
    name: &str,
    place: Option<&crate::fusion::Placement>,
    keep_sheets: bool,
    colours: &crate::Colours,
    history: Option<&crate::History>,
    only: Option<(usize, usize)>,
) -> Result<(String, usize, usize, Vec<String>), SatError> {
    let ident = Xf { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3], s: 1.0, unit: sat.scale };
    let mut w = Writer {
        sat,
        lay: Layout::of(sat.version),
        out: String::new(),
        next: 0,
        xf: ident,
        vertices: BTreeMap::new(),
        edges: BTreeMap::new(),
        curves: BTreeMap::new(),
        surfaces: BTreeMap::new(),
        faces: 0,
        skipped: 0,
        uses: BTreeMap::new(),
        seen: std::collections::BTreeSet::new(),
        lumps: std::collections::BTreeSet::new(),
        failed: BTreeMap::new(),
        defs: BTreeMap::new(),
        u_fastest: sat.version >= SHAPE_MANAGER,
        full: false,
        colours,
        body_colour: None,
        body_defaults: std::collections::HashMap::new(),
        body_default: None,
        materials: if colours.is_empty() { std::collections::HashMap::new() } else { sat.materials() },
        styled: Vec::new(),
        warnings: Vec::new(),
    };
    if let Some(h) = history.filter(|h| !colours.is_empty() && !h.by_tag.is_empty()) {
        w.inherit_history(h);
    }
    let app = w.add("APPLICATION_CONTEXT('automotive design')");
    w.add(&format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
    let pc = w.add(&format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let pdc = w.add(&format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let len_u = w.add("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))");
    let ang = w.add("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))");
    let sol = w.add("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())");
    let unc =
        w.add(&format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#{len_u},'distance_accuracy_value','')"));
    let ctx = w.add(&format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len_u},#{ang},#{sol})) REPRESENTATION_CONTEXT('',''))"
    ));
    let bodies: Vec<i64> = sat
        .recs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.ty == "body")
        .filter_map(|(i, _)| i64::try_from(i).ok())
        .collect();
    // One body of a blob that must hold `of` bodies (a Fusion base feature's output).
    let bodies: Vec<i64> = match only {
        None => bodies,
        Some((nth, of)) => {
            if bodies.len() != of {
                return Err(SatError::Unsupported(format!(
                    "the blob holds {} bodies, {of} were expected",
                    bodies.len()
                )));
            }
            let one =
                bodies.get(nth).copied().ok_or_else(|| SatError::Unsupported(format!("no body {nth} in the blob")))?;
            vec![one]
        }
    };
    let many = bodies.len() > 1;
    // The bodies of a feature's blob that its body references colour: those with a transform
    // after the first, only when the counts agree (else the pairing is not known).
    if let Some(h) = history.filter(|h| !h.bodies.is_empty()) {
        let indexed: Vec<i64> = bodies
            .iter()
            .copied()
            .filter(|&b| {
                sat.rec(b).and_then(|r| r.ptr(3)).and_then(|t| sat.rec(t)).is_some_and(|t| t.ty == "transform")
            })
            .skip(1)
            .collect();
        if indexed.len() == h.bodies.len() {
            w.body_defaults = indexed.into_iter().zip(&h.bodies).filter_map(|(b, c)| Some((b, (*c)?))).collect();
        }
    }
    let mut written = 0;
    let mut skipped = 0;
    for (n, b) in bodies.iter().enumerate() {
        let br = w.rec(*b)?;
        // Geometry is written per body (each has its own transform).
        w.xf = transform(sat, br);
        if let Some(p) = place {
            w.xf = placed(w.xf, p);
        }
        w.vertices.clear();
        w.edges.clear();
        w.curves.clear();
        w.surfaces.clear();
        w.defs.clear();
        w.seen.clear();
        w.body_colour = w.materials.get(b).and_then(|m| colours.get(*m)).copied();
        w.body_default = w.body_defaults.get(b).copied();
        let mut solids = Vec::new();
        let mut sheets = Vec::new();
        for lump in w.chain(br.ptr(1), 1)? {
            if !w.lumps.insert(lump) {
                w.warn(format!("body {} shares a lump with an earlier body; it is written once", n + 1));
                continue;
            }
            let lr = w.rec_of(lump, "lump")?;
            let mut shells = Vec::new();
            for sh in w.chain(lr.ptr(2), 1)? {
                let mut faces = Vec::new();
                w.shell_faces(sh, 0, &mut faces)?;
                let mut ids = Vec::new();
                w.uses.clear();
                for f in faces {
                    // A face with geometry not read yet is left out (its shell becomes a
                    // sheet), so the rest of the part still comes in.
                    match w.face(f) {
                        Ok(id) => ids.push(id),
                        Err(SatError::Unsupported(m)) => {
                            w.skipped += 1;
                            if w.skipped <= 20 {
                                w.warn(format!("a face was left out: {m}"));
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
                if ids.is_empty() {
                    continue;
                }
                // A shell with free edges bounds no volume: written as a sheet.
                if w.uses.values().any(|n| *n < 2) {
                    sheets.push(w.add(&format!("OPEN_SHELL('',{})", list(&ids))));
                } else {
                    shells.push(w.add(&format!("CLOSED_SHELL('',{})", list(&ids))));
                }
            }
            // The first shell of a lump is its outside; the others are cavities.
            let mut it = shells.into_iter();
            let Some(outer) = it.next() else { continue };
            let voids: Vec<usize> = it.map(|s| w.add(&format!("ORIENTED_CLOSED_SHELL('',*,#{s},.F.)"))).collect();
            solids.push(if voids.is_empty() {
                w.add(&format!("MANIFOLD_SOLID_BREP('',#{outer})"))
            } else {
                w.add(&format!("BREP_WITH_VOIDS('',#{outer},{})", list(&voids)))
            });
        }
        if !sheets.is_empty() && !keep_sheets {
            skipped += 1;
            sheets.clear();
        }
        if !sheets.is_empty() {
            w.warn(format!("body {} has open shells (free edges); they are imported as sheets", n + 1));
            let model = w.add(&format!("SHELL_BASED_SURFACE_MODEL('',{})", list(&sheets)));
            let label = string(&format!("{name} {} sheet", n + 1));
            let prod = w.add(&format!("PRODUCT({label},{label},'',(#{pc}))"));
            let pdf = w.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
            let pd = w.add(&format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdc})"));
            let pds = w.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
            let rep = w.add(&format!("MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#{model}),#{ctx})"));
            w.add(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
            written += 1;
        }
        if solids.is_empty() {
            if sheets.is_empty() && keep_sheets {
                w.warn(format!("body {} has no faces", n + 1));
            }
            continue;
        }
        written += 1;
        let label = if many { string(&format!("{name} {}", n + 1)) } else { string(name) };
        let prod = w.add(&format!("PRODUCT({label},{label},'',(#{pc}))"));
        let pdf = w.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
        let pd = w.add(&format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdc})"));
        let pds = w.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
        let rep = w.add(&format!("ADVANCED_BREP_SHAPE_REPRESENTATION('',{},#{ctx})", list(&solids)));
        w.add(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
    }
    w.face_colours(ctx);
    w.check_full()?;
    if written == 0 {
        if skipped > 0 {
            return Ok((String::new(), 0, skipped, w.warnings));
        }
        return Err(SatError::Unsupported("the file holds no body with faces".into()));
    }
    let mut text = String::from("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ACIS SAT converted by Gusset'),'2;1');\n");
    let _ = writeln!(text, "FILE_NAME({},'',(''),(''),'gusset-sat','','');", string(name));
    text.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n");
    text.push_str(&w.out);
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok((text, written, skipped, w.warnings))
}

#[cfg(test)]
mod surface_layout_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::parse::{Rec, Tok};

    fn rec(s: &str) -> Rec {
        Rec { ty: "spline-surface".into(), f: s.split_whitespace().map(|w| Tok::Word(w.into())).collect() }
    }

    #[test]
    fn the_binary_forms_enumeration_values_read_like_the_texts_words() {
        let pts = "0 0 0 1 0 0 0 1 0 1 1 1";
        let text = rec(&format!("nubs 1 1 open open none none 2 2 0 1 1 1 0 1 1 1 {pts}"));
        let bin = rec(&format!("nubs 1 1 0 0 0 0 2 2 0 1 1 1 0 1 1 1 {pts}"));
        let (a, b) = (bs3_surface(&text, 0, false).unwrap(), bs3_surface(&bin, 0, false).unwrap());
        assert_eq!(a.points, b.points);
        assert_eq!(a.knots, b.knots);
        assert_eq!(a.points.len(), 2);
        // Without descriptors at all, the counts come right after the degrees.
        let bare = rec(&format!("nubs 1 1 2 2 0 1 1 1 0 1 1 1 {pts}"));
        assert_eq!(bs3_surface(&bare, 0, false).unwrap().points, a.points);
    }

    #[test]
    fn a_helical_ruled_surface_runs_between_two_helices() {
        // Two turns, pitch -3 along z, radius 10; a flank 0.2 across that grows the radius by
        // 0.5 x 10 per unit and climbs 2 per unit along the axis (0, 0, -1).
        let two_turns = 4.0 * std::f64::consts::PI;
        let head = "22310 F 0 F 0.2 F 0 F 12.566370614359172 F 0 F 12.566370614359172";
        let tail = "0 0 3 0 -10 0 -10 0 0 0 0 -3 0 0 0 -1 null_surface null_surface nullbs nullbs 0.5 0 2 }";
        let r = |h: &str| rec(&format!("{h} {tail}"));
        let sf = helix_surface(&r(head), 0).unwrap();
        assert_eq!(sf.degree, [1, 3]);
        assert_eq!(sf.points.len(), 2);
        let (row0, row1) = (&sf.points[0], &sf.points[1]);
        assert_eq!(row0.len(), row1.len());
        let near = |a: V3, b: V3| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9);
        assert!(near(row0[0], [0.0, -10.0, 3.0]));
        // At u = 0.2: radius 10 x 1.1, 0.4 down the axis.
        assert!(near(row1[0], [0.0, -11.0, 2.6]));
        // Two turns later the helix is back at the start, six lower.
        assert!(near(*row0.last().unwrap(), [0.0, -10.0, -3.0]));
        assert!(near(*row1.last().unwrap(), [0.0, -11.0, -3.4]));
        assert_eq!(sf.knots[1].last().copied(), Some(two_turns));
        assert_eq!(sf.mults[1].iter().sum::<usize>(), row0.len() + 4);
        // Hostile: a million turns, a tapered helix, a twist, no direction, a missing range,
        // non-numbers.
        let many = head.replace("12.566370614359172", "6283185.3");
        assert!(helix_surface(&r(&many), 0).is_err());
        assert!(
            helix_surface(&rec(&format!("{head} {}", tail.replacen("-3 0 0 0 -1", "-3 0.5 0 0 -1", 1))), 0).is_err()
        );
        assert!(helix_surface(&rec(&format!("{head} {}", tail.replace("0.5 0 2", "0.5 0.1 2"))), 0).is_err());
        assert!(helix_surface(&rec(&format!("{head} 0 0 3 0 -10 0 -10 0 0 0 0 -3 0 0 0 -1")), 0).is_err());
        assert!(helix_surface(&rec("22310 F 0 F 0.2"), 0).is_err());
        assert!(helix_surface(&rec("22310 F 0 F nan F 0 F 1 F 0 F 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"), 0).is_err());
        assert!(helix_surface(&rec("22310 F 0 F 0 F 0 F 1 F 0 F 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1"), 0).is_err());
    }

    #[test]
    fn shape_managers_control_points_are_listed_u_fastest() {
        // Two by three points: (iu, iv) is (10 iu, iv, 0), listed v fastest then u fastest.
        let v_fast = "nubs 1 2 open open none none 2 2 0 1 1 1 0 2 1 2 0 0 0 0 1 0 0 2 0 10 0 0 10 1 0 10 2 0";
        let u_fast = "nubs 1 2 open open none none 2 2 0 1 1 1 0 2 1 2 0 0 0 10 0 0 0 1 0 10 1 0 0 2 0 10 2 0";
        let a = bs3_surface(&rec(v_fast), 0, false).unwrap();
        let b = bs3_surface(&rec(u_fast), 0, true).unwrap();
        assert_eq!(a.points, b.points);
        assert!(a.points[1][2].iter().zip([10.0, 2.0, 0.0]).all(|(p, q)| (p - q).abs() < 1e-12));
        assert_ne!(bs3_surface(&rec(u_fast), 0, false).unwrap().points, a.points);
    }

    #[test]
    fn a_curves_nubs_is_not_read_as_a_surface_and_the_next_candidate_is() {
        // A procedural surface can hold a path curve (`nubs <degree> <closure> <count> ...`)
        // before its own approximation: the curve fails as a surface, cleanly.
        let curve = rec("nubs 5 2 3 0.0 5 0.5 1 1.0 5 0 0 0 1 1 1");
        assert!(bs3_surface(&curve, 0, false).is_err());
        let pts = "0 0 0 1 0 0 0 1 0 1 1 1";
        let both = format!("{} nubs 1 1 open open none none 2 2 0 1 1 1 0 1 1 1 {pts}", "nubs 5 2 3 0.0 5 0.5 1 1.0 5");
        let r = rec(&both);
        let start = (0..r.f.len())
            .filter(|i| r.word(*i) == Some("nubs"))
            .map(|i| bs3_surface(&r, i, false))
            .find(Result::is_ok);
        assert!(start.is_some());
    }

    #[test]
    fn a_procedural_surfaces_own_approximation_is_not_one_of_a_nested_support() {
        // A blend whose support is itself procedural: the support's approximation comes first,
        // inside its own block; the blend's comes after it.
        let a = "nubs 1 1 2 2 0 1 1 1 0 1 1 1 0 0 0 1 0 0 0 1 0 1 1 0";
        let b = "nubs 1 1 2 2 0 1 1 1 0 1 1 1 0 0 5 1 0 5 0 1 5 1 1 5";
        let r = rec(&format!("{{ rb_blend_spl_sur 1 spline T {{ rot_spl_sur {a} }} 0.5 {b} }} {a}"));
        let own = own_approximation(&r, 0, false).unwrap().unwrap();
        assert!(own.points.iter().flatten().all(|p| (p[2] - 5.0).abs() < 1e-12));
        // A shared definition ends at its brace: what follows is another record's.
        let r = rec(&format!("{{ rot_spl_sur 1 }} {a}"));
        assert!(own_approximation(&r, 0, false).is_none());
    }

    /// A rational quadratic full circle (ACIS's curve layout): radius `rad` about z at height `z`.
    fn circle_curve(rad: f64, z: f64) -> String {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let pts =
            [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0)];
        let mut s = String::from("nurbs 2 open 5 0 2 0.25 2 0.5 2 0.75 2 1 2");
        for (i, (x, y)) in pts.iter().chain([(1.0, 0.0)].iter()).enumerate() {
            let w = if i % 2 == 1 { h } else { 1.0 };
            s += &format!(" {} {} {z} {w}", x * rad, y * rad);
        }
        s
    }

    /// A quarter of the tube of a torus (major `big`, minor `small`, about z): its outer upper
    /// quarter, from the outer equator up, all round, at height `z0`; `flip` runs across the other way.
    fn torus_patch(big: f64, small: f64, z0: f64, flip: bool) -> Bs3Surface {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let mut profile = [(big + small, 0.0, 1.0), (big + small, small, h), (big, small, 1.0)];
        if flip {
            profile.reverse();
        }
        let ring =
            [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0)];
        let ring: Vec<(f64, f64, f64)> = ring
            .iter()
            .chain([(1.0, 0.0)].iter())
            .enumerate()
            .map(|(i, (x, y))| (*x, *y, if i % 2 == 1 { h } else { 1.0 }))
            .collect();
        let points = profile
            .iter()
            .map(|(rho, z, _)| ring.iter().map(|(x, y, _)| [x * rho, y * rho, z + z0]).collect())
            .collect();
        let weights = profile.iter().map(|(_, _, wu)| ring.iter().map(|(_, _, wv)| wu * wv).collect()).collect();
        Bs3Surface {
            degree: [2, 2],
            knots: [vec![0.0, 1.0], vec![0.0, 0.25, 0.5, 0.75, 1.0]],
            mults: [vec![3, 3], vec![3, 2, 2, 2, 3]],
            points,
            weights: Some(weights),
        }
    }

    #[test]
    fn a_rolling_ball_blend_round_a_circle_is_a_torus() {
        let r = rec(&format!(
            "{{ rb_blend_spl_sur 1 intcurve T {{ off_int_cur 1 0 {} }} -0.5 -0.5 }}",
            circle_curve(5.0, 2.0)
        ));
        let patch = torus_patch(5.0, 0.5, 2.0, false);
        // The evaluator: the patch's middle is on the tube.
        let p = bs3_surface_point(&patch, 0.5, 0.3).unwrap();
        assert!((libm::hypot(libm::hypot(p[0], p[1]) - 5.0, p[2] - 2.0) - 0.5).abs() < 1e-12);
        let t = blend_torus(&r, 0, &patch).unwrap();
        assert!((t.major - 5.0).abs() < 1e-9 && (t.minor - 0.5).abs() < 1e-9, "{} {}", t.major, t.minor);
        assert!(len(sub(t.centre, [0.0, 0.0, 2.0])) < 1e-9);
        assert!((t.axis[2].abs() - 1.0).abs() < 1e-12);
        assert!(len(sub(t.start, [1.0, 0.0, 0.0])) < 1e-9);
        // Across the other way, the approximation faces the other way.
        let flipped = blend_torus(&r, 0, &torus_patch(5.0, 0.5, 2.0, true)).unwrap();
        assert_ne!(t.inward, flipped.inward);
        // A blend whose ball centre does not run round its tube, or a spine that is no circle:
        // the approximation stays.
        assert!(blend_torus(&r, 0, &torus_patch(5.0, 0.8, 2.3, false)).is_none());
        let oval = rec(&format!(
            "{{ rb_blend_spl_sur 1 intcurve T {{ off_int_cur 1 0 {} }} }}",
            circle_curve(5.0, 2.0).replacen(" 5 5 2 ", " 5 6 2 ", 1)
        ));
        assert!(blend_torus(&oval, 0, &patch).is_none());
    }

    #[test]
    fn a_rational_surface_may_put_both_and_four_enumeration_values_before_the_counts() {
        // Fusion's `exact_spl_sur` inside a `pcurve` (the 2025 layout, HDMI cable's pins).
        let pts = "0 0 0 1 1 0 0 1 0 1 0 1 1 1 0 1";
        let plain = rec(&format!("nurbs 1 1 2 2 0 1 1 1 0 1 1 1 {pts}"));
        let both = rec(&format!("nurbs 1 1 both 2 0 0 0 2 2 0 1 1 1 0 1 1 1 {pts}"));
        let (a, b) = (bs3_surface(&plain, 0, false).unwrap(), bs3_surface(&both, 0, false).unwrap());
        assert_eq!(a.points, b.points);
        assert_eq!(a.knots, b.knots);
        assert_eq!(b.weights, a.weights);
        // Damaged counts are an error, not a panic.
        assert!(bs3_surface(&rec("nurbs 1 1 both 2 0 0 0 2"), 0, false).is_err());
    }
}

#[cfg(test)]
mod closed_curve_tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    /// De Boor: the point of a B-spline at `u`.
    fn eval(b: &Bs3, u: f64) -> V3 {
        let flat = flat_knots(b);
        let d = b.degree;
        let w = |i: usize| b.weights.as_ref().map_or(1.0, |w| w[i]);
        let k = (d..flat.len() - d - 1).rev().find(|k| flat[*k] <= u).unwrap();
        let mut h: Vec<[f64; 4]> = (0..=d)
            .map(|j| {
                let (p, wi) = (b.points[j + k - d], w(j + k - d));
                [p[0] * wi, p[1] * wi, p[2] * wi, wi]
            })
            .collect();
        for r in 1..=d {
            for j in (r..=d).rev() {
                let i = j + k - d;
                let a = (u - flat[i]) / (flat[i + d + 1 - r] - flat[i]);
                let prev = h[j - 1];
                for (x, p) in h[j].iter_mut().zip(prev) {
                    *x = (1.0 - a) * p + a * *x;
                }
            }
        }
        [h[d][0] / h[d][3], h[d][1] / h[d][3], h[d][2] / h[d][3]]
    }

    #[test]
    fn a_piece_of_a_closed_curve_past_its_end_is_that_piece() {
        // A closed rational quadratic loop over [0, 1] (first and last points equal).
        let pts = vec![
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.5],
            [-1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.2],
            [1.0, 0.0, 0.0],
        ];
        let c = Bs3 {
            degree: 2,
            knots: vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
            mults: vec![3, 1, 1, 1, 1, 3],
            points: pts,
            weights: Some(vec![1.0, 0.7, 1.2, 0.9, 1.1, 0.8, 2.0]),
        };
        let near = |p: V3, q: V3| p.iter().zip(q).all(|(x, y)| (x - y).abs() < 1e-12);
        for (a, b) in [(0.65, 1.3), (-0.35, 0.3), (0.3, 0.7), (0.5, 1.0)] {
            let part = closed_bs3_part(&c, a, b).unwrap();
            let (k0, kn) = (part.knots[0], *part.knots.last().unwrap());
            for i in 0..=8 {
                let t = f64::from(i) / 8.0;
                let u = a + t * (b - a);
                let wrapped = u - libm::floor(u);
                let on = eval(&part, k0 + t * (kn - k0));
                assert!(near(on, eval(&c, wrapped)), "{a}..{b} at {u}: {on:?}");
            }
        }
        // More than a turn, or a curve that is not closed, has no such piece.
        assert!(closed_bs3_part(&c, 0.2, 1.3).is_none());
        let mut open = c.clone();
        open.points[6] = [2.0, 0.0, 0.0];
        assert!(closed_bs3_part(&open, 0.6, 1.2).is_none());
    }
}
