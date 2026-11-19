//! Rolling-ball blends (`BLENDED_EDGE`, ADR 0035 amendment) from their definition: a ball of
//! radius `|range|` touching both support surfaces rolls along the spine, the curve of its
//! centres. The file's spine curve is an approximation (an intersection chart, a circle, a
//! line), so every centre is put back onto the true spine by Newton steps on the distances to
//! the two supports, which may themselves be offsets, swept or spun surfaces, B-surfaces or
//! other blends (a zero-range blend is the edge curve a cliff-edge blend rolls along).
//!
//! The surface is written as a rational B-spline: each row across the blend is the exact
//! circular arc of the ball between its two contact points (two rational quadratic spans),
//! rows are joined along the spine by cubic Hermite spans whose tangents are the rows' own
//! derivatives, and rows are added where the surface strays from the ball's envelope.

use crate::decode::Graph;
use crate::geom::{Nurbs, V3, add, cross, dot, norm, scale, sub, unit};
use crate::reader::Val;

/// How deeply supports may nest (a blend on a blend on an offset ...).
const DEPTH: u32 = 6;
/// Most rows of one blend surface.
const MAX_ROWS: usize = 600;
/// Rounds of adding rows where the surface strays.
const ROUNDS: usize = 8;

/// The nearest point `p` of a surface to `x`, the unit normal `n` there (the natural normal for
/// analytic, parametric and offset surfaces; away from the spine for a blend) and the signed
/// distance `d` of `x` along it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Foot {
    pub(crate) p: V3,
    pub(crate) n: V3,
    pub(crate) d: f64,
}

fn ptrs(g: &Graph, node: u32, field: &str) -> Vec<u32> {
    g.list(node, field).iter().filter_map(|v| if let Val::Ptr(p) = v { Some(*p) } else { None }).collect()
}

/// The two supports of a blend and the ball's distance from each.
fn supports(g: &Graph, b: u32) -> Option<([u32; 2], [f64; 2])> {
    let s = ptrs(g, b, "surface");
    let &[s1, s2] = s.as_slice() else { return None };
    let r: Vec<f64> =
        g.list(b, "range").iter().filter_map(|v| if let Val::Real(x) = v { Some(x.abs()) } else { None }).collect();
    let &[r1, r2] = r.as_slice() else { return None };
    (s1 != 0 && s2 != 0 && r1.is_finite() && r2.is_finite()).then_some(([s1, s2], [r1, r2]))
}

/// The ball radius of a blend.
fn radius(g: &Graph, b: u32) -> Option<f64> {
    supports(g, b).map(|(_, r)| 0.5 * (r[0] + r[1]))
}

/// The foot of `x` on surface `s` (see [`Foot`]).
pub(crate) fn foot(g: &Graph, s: u32, x: V3, depth: u32) -> Option<Foot> {
    if depth > DEPTH {
        return None;
    }
    let on = |p: V3, n: V3| Some(Foot { p, n, d: dot(sub(x, p), n) });
    match g.type_name(s)? {
        "PLANE" | "CYLINDER" | "SPHERE" | "TORUS" | "CONE" => {
            let p = crate::eval::project(g, s, x)?;
            on(p, crate::refine::normal(g, s, p)?)
        }
        "OFFSET_SURF" => {
            let base = g.ptr(s, "surface");
            let f = foot(g, base, x, depth + 1)?;
            let mut o = g.real(s, "offset")?;
            if g.ch(base, "sense") == Some(b'-') {
                o = -o;
            }
            on(add(f.p, scale(f.n, o)), f.n)
        }
        "BLENDED_EDGE" => {
            let r = radius(g, s)?;
            let q = spine_point(g, s, x, depth + 1)?;
            let v = sub(x, q);
            let n = unit(v)?;
            Some(Foot { p: add(q, scale(n, r)), n, d: norm(v) - r })
        }
        "SWEPT_SURF" => swept_foot(g, s, x, depth),
        _ => {
            let (p, uv) = crate::eval::project_uv(g, s, x)?;
            on(p, crate::eval::natural_normal(g, s, uv[0], uv[1], depth + 1)?)
        }
    }
}

/// The foot on a swept surface `C(u) + v D`: the section point whose line along `D` passes
/// nearest `x` (a one-parameter search over the section curve).
fn swept_foot(g: &Graph, s: u32, x: V3, depth: u32) -> Option<Foot> {
    let c = g.ptr(s, "section");
    let d = unit(g.vec3(s, "sweep")?)?;
    let (lo, hi) = crate::eval::curve_range(g, c)?;
    let at = |u: f64| crate::eval::curve_at(g, c, u, depth + 1);
    let off = |u: f64| -> Option<f64> {
        let q = sub(x, at(u)?);
        Some(norm(sub(q, scale(d, dot(q, d)))))
    };
    // Golden-section search of a bracket: round the last answer on this surface when the
    // minimum lies inside that narrow bracket, else round the best of a coarse sampling.
    let golden = |mut a: f64, mut b: f64| -> Option<f64> {
        let gr = 0.5 * (5f64.sqrt() - 1.0);
        let (mut c1, mut c2) = (b - gr * (b - a), a + gr * (b - a));
        let (mut f1, mut f2) = (off(c1)?, off(c2)?);
        for _ in 0..80 {
            if (b - a).abs() < 1e-15 * (1.0 + (hi - lo).abs()) {
                break;
            }
            if f1 < f2 {
                b = c2;
                c2 = c1;
                f2 = f1;
                c1 = b - gr * (b - a);
                f1 = off(c1)?;
            } else {
                a = c1;
                c1 = c2;
                f1 = f2;
                c2 = a + gr * (b - a);
                f2 = off(c2)?;
            }
        }
        Some(0.5 * (a + b))
    };
    let warm = g.cache.borrow().uv.get(&s).map(|w| w[0]);
    let narrow = 0.01 * (hi - lo);
    let near = warm.and_then(|w| {
        let (a, b) = ((w - narrow).max(lo), (w + narrow).min(hi));
        let u = golden(a, b)?;
        // At a bracket end that is not the curve's own end, the minimum may lie outside.
        let inside = (u - a > 1e-3 * narrow || a <= lo) && (b - u > 1e-3 * narrow || b >= hi);
        inside.then_some(u)
    });
    let u = match near {
        Some(u) => u,
        None => {
            let n = 64;
            let mut best = (f64::INFINITY, lo);
            for i in 0..=n {
                let u = lo + (hi - lo) * i as f64 / n as f64;
                if let Some(e) = off(u)
                    && e < best.0
                {
                    best = (e, u);
                }
            }
            let step = (hi - lo) / n as f64;
            golden((best.1 - step).max(lo), (best.1 + step).min(hi))?
        }
    };
    // Golden section finds a flat minimum only to the square root of the rounding: finish with
    // secant steps on the derivative's sign function, (x - C(u)) . C'(u) across the sweep.
    let hh = 1e-7 * (1.0 + (hi - lo).abs());
    let slope = |u: f64| -> Option<f64> {
        let q = sub(x, at(u)?);
        let dc = sub(at(u + hh)?, at(u - hh)?);
        Some(dot(sub(q, scale(d, dot(q, d))), sub(dc, scale(d, dot(dc, d)))))
    };
    let (mut u0, mut u1) = (u, u + hh);
    let mut u = u;
    if let (Some(mut f0), Some(mut f1)) = (slope(u0), slope(u1)) {
        for _ in 0..8 {
            if (f1 - f0).abs() <= f64::MIN_POSITIVE {
                break;
            }
            let u2 = (u1 - f1 * (u1 - u0) / (f1 - f0)).clamp(lo, hi);
            if (u2 - u).abs().is_nan() || (u2 - u).abs() > 10.0 * hh {
                break;
            }
            let Some(f2) = slope(u2) else { break };
            (u0, f0, u1, f1) = (u1, f1, u2, f2);
            u = u2;
            if (u1 - u0).abs() < 1e-15 * (1.0 + (hi - lo).abs()) {
                break;
            }
        }
    }
    g.cache.borrow_mut().uv.insert(s, [u, 0.0]);
    let base = at(u)?;
    let p = add(base, scale(d, dot(sub(x, base), d)));
    let h = 1e-6 * (1.0 + (hi - lo).abs());
    let du = sub(at((u + h).min(hi))?, at((u - h).max(lo))?);
    let n = unit(cross(du, d))?;
    Some(Foot { p, n, d: dot(sub(x, p), n) })
}

/// A point of the true spine of blend `b` near `x0`, and the spine's unit tangent there: Newton
/// on the ball's distances to the two supports (each kept on the side `x0` is on).
pub(crate) fn centre(g: &Graph, b: u32, x0: V3, depth: u32) -> Option<(V3, V3)> {
    let ([s1, s2], [r1, r2]) = supports(g, b)?;
    let f1 = foot(g, s1, x0, depth)?;
    let f2 = foot(g, s2, x0, depth)?;
    let target = |r: f64, d: f64| if d < 0.0 { -r } else { r };
    let (t1, t2) = (target(r1, f1.d), target(r2, f2.d));
    let scale_len = 1.0 + norm(x0);
    let mut x = x0;
    let (mut a1, mut a2) = (f1, f2);
    for k in 0..40 {
        if k > 0 {
            a1 = foot(g, s1, x, depth)?;
            a2 = foot(g, s2, x, depth)?;
        }
        let (e1, e2) = (a1.d - t1, a2.d - t2);
        let t = unit(cross(a1.n, a2.n))?;
        if e1.abs().max(e2.abs()) < 1e-13 * scale_len {
            return Some((x, t));
        }
        let c = dot(a1.n, a2.n);
        let det = 1.0 - c * c;
        if det < 1e-10 {
            return None;
        }
        let (p, q) = ((-e1 + c * e2) / det, (-e2 + c * e1) / det);
        let step = add(scale(a1.n, p), scale(a2.n, q));
        x = add(x, step);
        if !x.iter().all(|v| v.is_finite()) {
            return None;
        }
        if norm(step) < 1e-15 * scale_len {
            break;
        }
    }
    // Rounding (and numerical projections) can keep the last digits moving: accept a point
    // within 1e-10 of the targets.
    let (a1, a2) = (foot(g, s1, x, depth)?, foot(g, s2, x, depth)?);
    ((a1.d - t1).abs().max((a2.d - t2).abs()) < 1e-10 * scale_len).then_some((x, unit(cross(a1.n, a2.n))?))
}

/// Points along the spine curve of `b` in its parameter direction: the chart of an
/// intersection, samples of a bounded curve, or a stretch of a line around `hint`.
fn spine_points(g: &Graph, b: u32, hint: &[V3]) -> Option<Vec<V3>> {
    let spine = g.ptr(b, "spine");
    if g.type_name(spine)? == "LINE" {
        let (p, d) = (g.vec3(spine, "pvec")?, unit(g.vec3(spine, "direction")?)?);
        let ts: Vec<f64> = hint.iter().map(|q| dot(sub(*q, p), d)).collect();
        let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let (lo, hi) = if lo.is_finite() && hi > lo { (lo, hi) } else { (-1.0, 1.0) };
        let pad = 0.05 * (hi - lo);
        return Some((0..9).map(|i| add(p, scale(d, lo - pad + (hi - lo + 2.0 * pad) * i as f64 / 8.0))).collect());
    }
    if matches!(g.type_name(spine)?, "CIRCLE" | "ELLIPSE") {
        return closed_spine(g, b, spine);
    }
    crate::eval::curve_samples(g, spine, 33, 0)
}

/// Samples of a circle or ellipse spine: between the blend's start and end limits when it
/// has both, else round the whole curve.
fn closed_spine(g: &Graph, b: u32, c: u32) -> Option<Vec<V3>> {
    let (cn, n, x) = (g.vec3(c, "centre")?, unit(g.vec3(c, "normal")?)?, unit(g.vec3(c, "x_axis")?)?);
    let y = cross(n, x);
    let param = |p: V3| {
        let q = sub(p, cn);
        let (a, bb) = match g.real(c, "radius") {
            Some(r) => (r, r),
            None => (g.real(c, "major_radius").unwrap_or(1.0), g.real(c, "minor_radius").unwrap_or(1.0)),
        };
        libm::atan2(dot(q, y) / bb, dot(q, x) / a)
    };
    let limit = |field: &str| -> Option<V3> {
        let l = g.ptr(b, field);
        g.list(l, "hvec").iter().find_map(|v| if let Val::Vec3(p) = v { Some(*p) } else { None })
    };
    let tau = core::f64::consts::TAU;
    let (lo, hi) = match (limit("start"), limit("end")) {
        (Some(s), Some(e)) => {
            let (a, mut z) = (param(s), param(e));
            if g.ch(c, "sense") == Some(b'-') {
                // The curve runs against its parameter: from the start limit backwards.
                while z > a {
                    z -= tau;
                }
            } else {
                while z < a {
                    z += tau;
                }
            }
            (a.min(z), a.max(z))
        }
        _ => (0.0, tau),
    };
    let n = 33;
    (0..n).map(|i| crate::eval::curve_at(g, c, lo + (hi - lo) * i as f64 / (n - 1) as f64, 0)).collect()
}

/// The spine point of blend `b` nearest `x`, for `b` as another blend's support: on a fitted
/// curve of its true spine (each level of nesting would otherwise multiply the Newton steps).
fn spine_point(g: &Graph, b: u32, x: V3, depth: u32) -> Option<V3> {
    if g.type_name(g.ptr(b, "spine"))? == "LINE" {
        return spine_nearest(g, b, x, depth);
    }
    let cached = g.cache.borrow().spine_fit.get(&b).cloned();
    let fit = match cached {
        Some(f) => f,
        None => {
            let f = spine_fit(g, b, depth).map(std::rc::Rc::new);
            g.cache.borrow_mut().spine_fit.insert(b, f.clone());
            f
        }
    };
    match fit {
        Some(f) => nearest_on_curve(&f, x),
        None => spine_nearest(g, b, x, depth),
    }
}

/// The true spine of `b` as a C1 cubic through its centres with their own tangents, with
/// centres added until the curve stays within 1e-11 m (and 1e-9 of the radius) of the spine.
fn spine_fit(g: &Graph, b: u32, depth: u32) -> Option<Nurbs> {
    let pts = spine_points(g, b, &[])?;
    let n = pts.len();
    let mut st: Vec<(V3, V3)> = Vec::new();
    for (i, x) in pts.iter().enumerate() {
        let dir = sub(*pts.get((i + 1).min(n - 1))?, *pts.get(i.saturating_sub(1))?);
        if let Some((c, t)) = centre(g, b, *x, depth) {
            let t = if dot(t, dir) < 0.0 { scale(t, -1.0) } else { t };
            if st.last().is_none_or(|l| norm(sub(c, l.0)) > 1e-12) {
                st.push((c, t));
            }
        }
    }
    if st.len() < 2 {
        return None;
    }
    let tol = (1e-9 * radius(g, b)?).max(1e-11);
    let mut fit = hermite(&st)?;
    for _ in 0..12 {
        let knots = fit.knots[0].clone();
        let mut next = Vec::with_capacity(st.len() * 2);
        let mut grew = false;
        for (i, s0) in st.iter().enumerate() {
            next.push(*s0);
            let (Some(&k0), Some(&k1), Some(s1)) = (knots.get(i), knots.get(i + 1), st.get(i + 1)) else { continue };
            if st.len() + next.len() > 4000 {
                continue;
            }
            let Some(m) = crate::geom::eval_curve(&fit, 0.5 * (k0 + k1)) else { continue };
            let Some((c, t)) = centre(g, b, m, depth) else { continue };
            if norm(sub(c, m)) > tol && dot(sub(c, s0.0), s0.1) > 0.0 && dot(sub(s1.0, c), s1.1) > 0.0 {
                next.push((c, if dot(t, s0.1) < 0.0 { scale(t, -1.0) } else { t }));
                grew = true;
            }
        }
        if !grew {
            break;
        }
        st = next;
        fit = hermite(&st)?;
    }
    Some(fit)
}

/// A C1 cubic through points with their unit tangents (spans as long as the circular arc
/// with those end tangents).
fn hermite(st: &[(V3, V3)]) -> Option<Nurbs> {
    let first = st.first()?;
    let (mut poles, mut knots, mut mults) = (vec![first.0], vec![0.0], vec![4]);
    let mut u = 0.0;
    for w in st.windows(2) {
        let &[(p0, t0), (p1, t1)] = w else { return None };
        let chord = norm(sub(p1, p0));
        let theta = libm::acos(dot(t0, t1).clamp(-1.0, 1.0));
        let len = if theta > 1e-6 { chord * (0.5 * theta) / libm::sin(0.5 * theta) } else { chord };
        poles.push(add(p0, scale(t0, len / 3.0)));
        poles.push(sub(p1, scale(t1, len / 3.0)));
        poles.push(p1);
        u += len;
        knots.push(u);
        mults.push(3);
    }
    if let Some(m) = mults.last_mut() {
        *m = 4;
    }
    let count = poles.len();
    Some(Nurbs {
        degree: [3, 0],
        n: [count, 1],
        poles,
        weights: None,
        mults: [mults, Vec::new()],
        knots: [knots, Vec::new()],
        closed: [false, false],
    })
}

/// The point nearest `x` of a C1 cubic made by [`hermite`] (Bezier spans of four poles):
/// the span of the nearest pole, Newton on `(B(t) - x) . B'(t)` within it, then its
/// neighbours when the answer lies on an end.
fn nearest_on_curve(c: &Nurbs, x: V3) -> Option<V3> {
    let spans = (c.poles.len().checked_sub(1)?) / 3;
    if spans == 0 {
        return c.poles.first().copied();
    }
    let bez = |i: usize, t: f64| -> Option<(V3, V3, V3)> {
        let p = |k: usize| c.poles.get(3 * i + k).copied();
        let (p0, p1, p2, p3) = (p(0)?, p(1)?, p(2)?, p(3)?);
        let s = 1.0 - t;
        let pt = add(
            add(scale(p0, s * s * s), scale(p1, 3.0 * s * s * t)),
            add(scale(p2, 3.0 * s * t * t), scale(p3, t * t * t)),
        );
        let d1 =
            add(add(scale(sub(p1, p0), 3.0 * s * s), scale(sub(p2, p1), 6.0 * s * t)), scale(sub(p3, p2), 3.0 * t * t));
        let d2 =
            add(scale(add(sub(p2, scale(p1, 2.0)), p0), 6.0 * s), scale(add(sub(p3, scale(p2, 2.0)), p1), 6.0 * t));
        Some((pt, d1, d2))
    };
    let solve = |i: usize| -> Option<(f64, V3, f64)> {
        let mut best = (f64::INFINITY, [0.0; 3], 0.0);
        for t0 in [0.0, 0.5, 1.0] {
            let mut t: f64 = t0;
            for _ in 0..30 {
                let (p, d1, d2) = bez(i, t)?;
                let f = dot(sub(p, x), d1);
                let fp = dot(d1, d1) + dot(sub(p, x), d2);
                if fp.is_nan() || fp <= 0.0 {
                    break;
                }
                let next = (t - f / fp).clamp(0.0, 1.0);
                let moved = (next - t).abs();
                t = next;
                if moved < 1e-15 {
                    break;
                }
            }
            let p = bez(i, t)?.0;
            let d = norm(sub(p, x));
            if d < best.0 {
                best = (d, p, t);
            }
        }
        Some(best)
    };
    // The span whose end poles are nearest.
    let mut i0 = 0;
    let mut dmin = f64::INFINITY;
    for i in 0..=spans {
        let d = norm(sub(*c.poles.get(3 * i)?, x));
        if d < dmin {
            dmin = d;
            i0 = i;
        }
    }
    let mut best: Option<(f64, V3, f64)> = None;
    for i in [i0.wrapping_sub(1), i0, i0 + 1] {
        if i < spans
            && let Some(r) = solve(i)
            && best.is_none_or(|b| r.0 < b.0)
        {
            best = Some(r);
        }
    }
    best.map(|b| b.1)
}

/// The point of the true spine of `b` nearest `x`: from the nearer of the spine curve's
/// nearest sample and the last point found on this spine.
pub(crate) fn spine_nearest(g: &Graph, b: u32, x: V3, depth: u32) -> Option<V3> {
    let spine = g.ptr(b, "spine");
    let start = if g.type_name(spine)? == "LINE" {
        let (p, d) = (g.vec3(spine, "pvec")?, unit(g.vec3(spine, "direction")?)?);
        add(p, scale(d, dot(sub(x, p), d)))
    } else {
        let cached = g.cache.borrow().spine.get(&b).cloned();
        let pts = match cached {
            Some(p) => p,
            None => {
                let p = std::rc::Rc::new(spine_points(g, b, &[])?);
                g.cache.borrow_mut().spine.insert(b, p.clone());
                p
            }
        };
        nearest_on_polyline(&pts, x)?
    };
    let warm = g.cache.borrow().spine_last.get(&b).copied();
    let q = match warm {
        Some(w) if norm(sub(w, x)) <= norm(sub(start, x)) => w,
        _ => start,
    };
    let c = spine_nearest_from(g, b, x, q, depth)?;
    g.cache.borrow_mut().spine_last.insert(b, c);
    Some(c)
}

fn nearest_on_polyline(pts: &[V3], x: V3) -> Option<V3> {
    let mut best: Option<(f64, V3)> = None;
    for w in pts.windows(2) {
        let &[a, b] = w else { continue };
        let ab = sub(b, a);
        let l2 = dot(ab, ab);
        let t = if l2 > 0.0 { (dot(sub(x, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let q = add(a, scale(ab, t));
        let d = norm(sub(x, q));
        if best.is_none_or(|(e, _)| d < e) {
            best = Some((d, q));
        }
    }
    if pts.len() == 1 {
        return pts.first().copied();
    }
    best.map(|b| b.1)
}

/// One row across the blend at spine point `c`: the ball's arc from its contact with the first
/// support to the second (the other way round when the spine runs against its curve), as five
/// homogeneous poles of two rational quadratic spans.
type Row = [[f64; 4]; 5];

fn row(g: &Graph, b: u32, c: V3, flip: bool) -> Option<Row> {
    let ([s1, s2], _) = supports(g, b)?;
    let (p1, p2) = (foot(g, s1, c, 0)?.p, foot(g, s2, c, 0)?.p);
    let (a, z) = if flip { (p2, p1) } else { (p1, p2) };
    let (e1, e2) = (unit(sub(a, c))?, unit(sub(z, c))?);
    let r = 0.5 * (norm(sub(a, c)) + norm(sub(z, c)));
    let em = unit(add(e1, e2))?;
    let cos_half = dot(e1, em).clamp(-1.0, 1.0);
    if cos_half < 1e-3 {
        return None;
    }
    let w = libm::sqrt(0.5 * (1.0 + cos_half));
    let k = |u: V3, v: V3| add(c, scale(add(u, v), r / (1.0 + cos_half)));
    let h = |p: V3, w: f64| [p[0] * w, p[1] * w, p[2] * w, w];
    Some([
        h(add(c, scale(e1, r)), 1.0),
        h(k(e1, em), w),
        h(add(c, scale(em, r)), 1.0),
        h(k(em, e2), w),
        h(add(c, scale(e2, r)), 1.0),
    ])
}

/// A spine station: the centre, the spine tangent (in the curve's direction), the row and its
/// derivative along the spine (per unit length).
#[derive(Clone, Copy)]
struct Station {
    c: V3,
    t: V3,
    row: Row,
    drow: Row,
}

fn station(g: &Graph, b: u32, x: V3, dir: V3, flip: bool) -> Option<Station> {
    let (c, t) = centre(g, b, x, 0)?;
    let t = if dot(t, dir) < 0.0 { scale(t, -1.0) } else { t };
    let r = radius(g, b)?;
    let h = 1e-4 * r.max(1e-6);
    let (cp, _) = centre(g, b, add(c, scale(t, h)), 0)?;
    let (cm, _) = centre(g, b, sub(c, scale(t, h)), 0)?;
    let (sp, sm) = (dot(sub(cp, c), t), dot(sub(cm, c), t));
    let (rp, rm, r0) = (row(g, b, cp, flip)?, row(g, b, cm, flip)?, row(g, b, c, flip)?);
    let mut drow = [[0.0; 4]; 5];
    let span = sp - sm;
    if span.abs() < 1e-300 {
        return None;
    }
    for (i, d) in drow.iter_mut().enumerate() {
        for (j, v) in d.iter_mut().enumerate() {
            *v = (rp.get(i)?.get(j)? - rm.get(i)?.get(j)?) / span;
        }
    }
    Some(Station { c, t, row: r0, drow })
}

/// The length of spine between two stations (the arc through both with their tangents).
fn gap(a: &Station, b: &Station) -> f64 {
    let chord = norm(sub(b.c, a.c));
    let theta = libm::acos(dot(a.t, b.t).clamp(-1.0, 1.0));
    if theta > 1e-6 { chord * (0.5 * theta) / libm::sin(0.5 * theta) } else { chord }
}

fn surface(st: &[Station]) -> Option<Nurbs> {
    let first = st.first()?;
    let mut hp: Vec<Row> = vec![first.row];
    let mut knots = vec![0.0];
    let mut mults = vec![4];
    let mut u = 0.0;
    for w in st.windows(2) {
        let &[a, b] = w else { return None };
        let l = gap(&a, &b);
        let lerp = |r: &Row, d: &Row, f: f64| -> Row {
            let mut o = *r;
            for (oi, di) in o.iter_mut().zip(d) {
                for (x, y) in oi.iter_mut().zip(di) {
                    *x += f * y;
                }
            }
            o
        };
        hp.push(lerp(&a.row, &a.drow, l / 3.0));
        hp.push(lerp(&b.row, &b.drow, -l / 3.0));
        hp.push(b.row);
        u += l;
        knots.push(u);
        mults.push(3);
    }
    if let Some(m) = mults.last_mut() {
        *m = 4;
    }
    let mut poles = Vec::with_capacity(hp.len() * 5);
    let mut weights = Vec::with_capacity(hp.len() * 5);
    for r in &hp {
        for h in r {
            if h[3].is_nan() || h[3] <= 0.0 {
                return None;
            }
            poles.push([h[0] / h[3], h[1] / h[3], h[2] / h[3]]);
            weights.push(h[3]);
        }
    }
    Some(Nurbs {
        degree: [3, 2],
        n: [hp.len(), 5],
        poles,
        weights: Some(weights),
        mults: [mults, vec![3, 2, 3]],
        knots: [knots, vec![0.0, 0.5, 1.0]],
        closed: [false, false],
    })
}

/// How far the surface strays from the ball's envelope between stations `i` and `i + 1`.
fn stray(g: &Graph, b: u32, n: &Nurbs, u0: f64, u1: f64, r: f64, near: V3) -> Option<f64> {
    let mut worst: f64 = 0.0;
    let u = 0.5 * (u0 + u1);
    for v in [0.1, 0.5, 0.9] {
        let p = crate::eval::nurbs_surface_at(n, u, v)?;
        // The nearest spine point to p, from the station's centre.
        let q = spine_nearest_from(g, b, p, near, 0)?;
        worst = worst.max((norm(sub(p, q)) - r).abs());
    }
    Some(worst)
}

/// The point of the true spine of `b` nearest `x`, from `start`: steps along the spine's
/// tangent, scaled by the secant of the remaining offset (a curved spine seen from a point far
/// off it overshoots a plain projection).
fn spine_nearest_from(g: &Graph, b: u32, x: V3, start: V3, depth: u32) -> Option<V3> {
    let mut q = start;
    let mut last = None;
    let mut prev: Option<(f64, f64)> = None;
    for _ in 0..40 {
        let (c, t) = centre(g, b, q, depth)?;
        let s = dot(sub(x, c), t);
        last = Some(c);
        if s.abs() < 1e-12 * (1.0 + norm(x)) {
            return Some(c);
        }
        let step = match prev {
            Some((sp, dp)) if dp.abs() > 0.0 && (s - sp).abs() > 0.0 => {
                let m = (s - sp) / dp;
                if m < -0.05 { (-s / m).clamp(-4.0 * s.abs(), 4.0 * s.abs()) } else { s }
            }
            _ => s,
        };
        prev = Some((s, step));
        q = add(c, scale(t, step));
    }
    last
}

/// The blend surface of `b` as a rational B-spline over the part of the spine its faces use
/// (`hint`: their vertices), or `None` when the ball cannot be placed.
pub(crate) fn blend_surface(g: &Graph, b: u32, hint: &[V3]) -> Option<Nurbs> {
    if g.ch(b, "blend_type") != Some(b'R') && g.ch(b, "blend_type") != Some(b'E') {
        return None;
    }
    let r = radius(g, b)?;
    if r.is_nan() || r <= 0.0 {
        return None;
    }
    let spine = g.ptr(b, "spine");
    let flip = g.ch(spine, "sense") == Some(b'-');
    let mut pts = spine_points(g, b, hint)?;
    if pts.len() < 2 {
        return None;
    }
    // Keep the stretch of an open spine the faces use, one sample beyond each end.
    let closed = pts.len() > 2 && norm(sub(*pts.first()?, *pts.last()?)) < 1e-9;
    if !closed && !hint.is_empty() && g.type_name(spine) != Some("LINE") {
        let idx = |x: V3| {
            let mut best = (f64::INFINITY, 0usize);
            for (i, w) in pts.windows(2).enumerate() {
                let &[a, bb] = w else { continue };
                let ab = sub(bb, a);
                let l2 = dot(ab, ab);
                let t = if l2 > 0.0 { (dot(sub(x, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = norm(sub(x, add(a, scale(ab, t))));
                if d < best.0 {
                    best = (d, i);
                }
            }
            best.1
        };
        let lo = hint.iter().map(|x| idx(*x)).min()?.saturating_sub(1);
        let hi = (hint.iter().map(|x| idx(*x)).max()? + 3).min(pts.len());
        pts = pts.get(lo..hi)?.to_vec();
    }
    // Stations at the samples; one more beyond each open end when the ball still fits there.
    let mut st: Vec<Station> = Vec::new();
    let n = pts.len();
    for (i, x) in pts.iter().enumerate() {
        let dir = sub(*pts.get((i + 1).min(n - 1))?, *pts.get(i.saturating_sub(1))?);
        if let Some(s) = station(g, b, *x, dir, flip)
            && st.last().is_none_or(|l: &Station| norm(sub(s.c, l.c)) > 1e-9 * (1.0 + r))
        {
            st.push(s);
        }
    }
    if st.len() < 2 {
        return None;
    }
    // Unbounded supports (analytic surfaces and their offsets) still hold the ball a little
    // past the spine's ends; bounded ones may not, so their blends end at the spine's ends.
    let unbounded = |mut q: u32| {
        for _ in 0..4 {
            match g.type_name(q) {
                Some("PLANE" | "CYLINDER" | "CONE" | "SPHERE" | "TORUS") => return true,
                Some("OFFSET_SURF") => q = g.ptr(q, "surface"),
                _ => return false,
            }
        }
        false
    };
    let ([s1, s2], _) = supports(g, b)?;
    if !closed && unbounded(s1) && unbounded(s2) {
        // Stations beyond the ends, oriented along the spine like the rest.
        let total: f64 = st.windows(2).map(|w| if let [a, b] = w { gap(a, b) } else { 0.0 }).sum();
        let len = 0.02 * total;
        let ext = |s: &Station, sign: f64| station(g, b, add(s.c, scale(s.t, sign * len)), s.t, flip);
        if let Some(e) = st.first().and_then(|s| ext(s, -1.0)) {
            st.insert(0, e);
        }
        if let Some(e) = st.last().and_then(|s| ext(s, 1.0)) {
            st.push(e);
        }
    }
    let tol = (1e-6 * r).max(1e-10);
    let mut fit = surface(&st)?;
    for _ in 0..ROUNDS {
        let knots = fit.knots[0].clone();
        let mut next = Vec::with_capacity(st.len() * 2);
        let mut grew = false;
        for (i, s) in st.iter().enumerate() {
            next.push(*s);
            let (Some(&u0), Some(&u1), Some(t)) = (knots.get(i), knots.get(i + 1), st.get(i + 1)) else { continue };
            if st.len() + next.len() > MAX_ROWS {
                continue;
            }
            // A span that cannot be measured is left as it is.
            let e = stray(g, b, &fit, u0, u1, r, s.c).unwrap_or(0.0);
            if e > tol {
                let mid = scale(add(s.c, t.c), 0.5);
                if let Some(m) = station(g, b, mid, sub(t.c, s.c), flip)
                    && dot(sub(m.c, s.c), s.t) > 0.0
                    && dot(sub(t.c, m.c), t.t) > 0.0
                {
                    next.push(m);
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
        st = next;
        fit = surface(&st)?;
    }
    Some(fit)
}

/// An SP-curve on a blend, in 3D: a circular arc when it runs across the blend at one spine
/// parameter (the edge between two blends), else points along it.
pub(crate) enum SpCurve {
    /// Centre, axis (the arc turns positively about it as the SP parameter grows), a point on
    /// the circle, radius.
    Arc(V3, V3, V3, f64),
    Points(Vec<V3>),
}

/// The blend's spine parameter law: a point of the spine curve at parameter `u` (an
/// intersection's chart runs from `base_parameter` by `base_scale` times the chord length along its
/// points), and the parameter of the spine curve's point nearest `x`.
fn spine_param_point(g: &Graph, spine: u32, u: f64) -> Option<V3> {
    match g.type_name(spine)? {
        "INTERSECTION" => {
            let chart = g.ptr(spine, "chart");
            let (base, k) = (g.real(chart, "base_parameter")?, g.real(chart, "base_scale").unwrap_or(1.0));
            let pts: Vec<V3> = g
                .list(chart, "hvec")
                .iter()
                .filter_map(|v| if let Val::Vec3(p) = v { Some(*p) } else { None })
                .collect();
            let mut s = (u - base) / k;
            for w in pts.windows(2) {
                let &[a, b] = w else { continue };
                let l = norm(sub(b, a));
                if s <= l || std::ptr::eq(w.as_ptr(), pts.get(pts.len().saturating_sub(2))?) {
                    return Some(add(a, scale(sub(b, a), if l > 0.0 { s / l } else { 0.0 })));
                }
                s -= l;
            }
            pts.first().copied()
        }
        _ => crate::eval::curve_at(g, spine, u, 0),
    }
}

fn spine_param_of(g: &Graph, spine: u32, x: V3) -> Option<f64> {
    match g.type_name(spine)? {
        "INTERSECTION" => {
            let chart = g.ptr(spine, "chart");
            let (base, k) = (g.real(chart, "base_parameter")?, g.real(chart, "base_scale").unwrap_or(1.0));
            let pts: Vec<V3> = g
                .list(chart, "hvec")
                .iter()
                .filter_map(|v| if let Val::Vec3(p) = v { Some(*p) } else { None })
                .collect();
            let (mut best, mut acc) = ((f64::INFINITY, 0.0), 0.0);
            for w in pts.windows(2) {
                let &[a, b] = w else { continue };
                let ab = sub(b, a);
                let l2 = dot(ab, ab);
                let t = if l2 > 0.0 { (dot(sub(x, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let d = norm(sub(x, add(a, scale(ab, t))));
                if d < best.0 {
                    best = (d, acc + t * l2.sqrt());
                }
                acc += l2.sqrt();
            }
            Some(base + k * best.1)
        }
        "LINE" => Some(dot(sub(x, g.vec3(spine, "pvec")?), unit(g.vec3(spine, "direction")?)?)),
        "CIRCLE" | "ELLIPSE" => {
            let (cn, n, xa) =
                (g.vec3(spine, "centre")?, unit(g.vec3(spine, "normal")?)?, unit(g.vec3(spine, "x_axis")?)?);
            let q = sub(x, cn);
            let (a, b) = match g.real(spine, "radius") {
                Some(r) => (r, r),
                None => (g.real(spine, "major_radius")?, g.real(spine, "minor_radius")?),
            };
            let t = libm::atan2(dot(q, cross(n, xa)) / b, dot(q, xa) / a);
            Some(if t < 0.0 { t + core::f64::consts::TAU } else { t })
        }
        _ => None,
    }
}

/// The 3D form of SP-curve `sp` on blend `b`. Its u is the spine's parameter and v runs
/// across the ball's arc in proportion to the angle; both are calibrated on `ends` (two
/// points of the curve with their SP parameters, from the trimmed curve that bounds it), so
/// the spine's parameter law need only be monotone.
pub(crate) fn sp_curve(g: &Graph, sp: u32, ends: Option<[(f64, V3); 2]>) -> Option<SpCurve> {
    let b = g.ptr(sp, "surface");
    let spine = g.ptr(b, "spine");
    let ([s1, s2], _) = supports(g, b)?;
    let nb = crate::geom::curve_rc(g, g.ptr(g.ptr(sp, "b_curve"), "nurbs"))?;
    let k = &nb.knots[0];
    let (lo, hi) = (*k.first()?, *k.last()?);
    let uv = |t: f64| crate::geom::eval_curve(&nb, t).map(|p| [p[0], p[1]]);
    // A spine centre and its arc's ends (from the first support to the second).
    let row = |c0: V3| -> Option<(V3, V3, V3, V3)> {
        let (c, t) = centre(g, b, c0, 0)?;
        Some((c, t, foot(g, s1, c, 0)?.p, foot(g, s2, c, 0)?.p))
    };
    let angle = |c: V3, a: V3, p: V3| {
        libm::acos(dot(unit(sub(a, c)).unwrap_or([0.0; 3]), unit(sub(p, c)).unwrap_or([0.0; 3])).clamp(-1.0, 1.0))
    };
    // Calibration: spine parameter = a + m u, v measured from the first support or the second.
    let (mut a, mut m, mut from_first) = (0.0, 1.0, true);
    if let Some([(ta, pa), (tb, pb)]) = ends {
        let (ua, ub) = (uv(ta)?, uv(tb)?);
        let (ca, cb) = (spine_nearest(g, b, pa, 0)?, spine_nearest(g, b, pb, 0)?);
        let (sa, sb) = (spine_param_of(g, spine, ca)?, spine_param_of(g, spine, cb)?);
        if (ub[0] - ua[0]).abs() > 1e-9 * (1.0 + ua[0].abs()) {
            m = (sb - sa) / (ub[0] - ua[0]);
            a = sa - m * ua[0];
        } else {
            // One spine parameter: an exact arc round the centre both ends share.
            let (c, t, p1, p2) = row(ca)?;
            let th = angle(c, p1, p2);
            let fa = angle(c, p1, pa) / th;
            from_first = (fa - ua[1]).abs() <= (1.0 - fa - ua[1]).abs();
            let (lo_p, hi_p) = if uv(lo)?[1] <= uv(hi)?[1] { (p1, p2) } else { (p2, p1) };
            let (lo_p, hi_p) = if from_first { (lo_p, hi_p) } else { (hi_p, lo_p) };
            let axis = if dot(cross(sub(lo_p, c), sub(hi_p, c)), t) >= 0.0 { t } else { scale(t, -1.0) };
            let r = 0.5 * (norm(sub(p1, c)) + norm(sub(p2, c)));
            return Some(SpCurve::Arc(c, axis, add(c, scale(unit(sub(lo_p, c))?, r)), r));
        }
        let (c, _, p1, p2) = row(ca)?;
        let fa = angle(c, p1, pa) / angle(c, p1, p2);
        from_first = (fa - ua[1]).abs() <= (1.0 - fa - ua[1]).abs();
    }
    let n = 65;
    (0..n)
        .map(|i| {
            let t = lo + (hi - lo) * i as f64 / (n - 1) as f64;
            let q = uv(t)?;
            let (c, _, p1, p2) = row(spine_param_point(g, spine, a + m * q[0])?)?;
            let (e1, e2) = (unit(sub(p1, c))?, unit(sub(p2, c))?);
            let th = libm::acos(dot(e1, e2).clamp(-1.0, 1.0));
            let w = unit(sub(e2, scale(e1, dot(e1, e2))))?;
            let f = if from_first { q[1] } else { 1.0 - q[1] };
            let r = 0.5 * (norm(sub(p1, c)) + norm(sub(p2, c)));
            Some(add(c, scale(add(scale(e1, libm::cos(f * th)), scale(w, libm::sin(f * th))), r)))
        })
        .collect::<Option<Vec<V3>>>()
        .map(SpCurve::Points)
}
