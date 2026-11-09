//! Intersection curves (ADR 0035 amendment): the file's chart is a polyline accurate only to
//! its chordal tolerance, so a cubic through its points can lie microns off the two surfaces
//! and shift the trimmed faces after a STEP read. For analytic supports every chart point is
//! snapped onto both surfaces (Newton on the two signed distances, the smallest step), and
//! the fitted curve is refined by inserting snapped points where it strays from either
//! surface until it is within a tight tolerance. Anything that does not converge keeps the
//! chart points as they are.

use crate::decode::Graph;
use crate::geom::{self, Nurbs, V3, add, cross, dot, norm, scale, sub, unit};
use crate::reader::Val;

/// Points of one fitted curve, at most (the fit stays far below the reader's limits).
const MAX_POINTS: usize = 4000;
/// Rounds of inserting points where the fit strays.
const ROUNDS: usize = 12;

/// The outward normal of an analytic surface at `p`, which lies on it.
fn normal(g: &Graph, s: u32, p: V3) -> Option<V3> {
    match g.type_name(s)? {
        "PLANE" => unit(g.vec3(s, "normal")?),
        "CYLINDER" => {
            let (o, a) = (g.vec3(s, "pvec")?, unit(g.vec3(s, "axis")?)?);
            let q = sub(p, o);
            unit(sub(q, scale(a, dot(q, a))))
        }
        "SPHERE" => unit(sub(p, g.vec3(s, "centre")?)),
        "TORUS" => {
            let (c, a) = (g.vec3(s, "centre")?, unit(g.vec3(s, "axis")?)?);
            let q = sub(p, c);
            let radial = unit(sub(q, scale(a, dot(q, a))))?;
            unit(sub(p, add(c, scale(radial, g.real(s, "major_radius")?))))
        }
        "CONE" => {
            let (o, a) = (g.vec3(s, "pvec")?, unit(g.vec3(s, "axis")?)?);
            let (sn, cs) = (g.real(s, "sin_half_angle")?, g.real(s, "cos_half_angle")?);
            let q = sub(p, o);
            let radial = unit(sub(q, scale(a, dot(q, a))))?;
            unit(sub(scale(radial, cs), scale(a, sn)))
        }
        _ => None,
    }
}

/// The signed distance of `x` from an analytic surface and the normal it is measured along.
fn signed(g: &Graph, s: u32, x: V3) -> Option<(f64, V3)> {
    let p = crate::eval::project(g, s, x)?;
    let n = normal(g, s, p)?;
    Some((dot(sub(x, p), n), n))
}

/// Newton from `x0` onto both surfaces: each step is the smallest move that cancels both
/// distances. `None` when the surfaces are (nearly) tangent there, it does not converge or
/// the point travels farther than `max_move`.
fn snap(g: &Graph, s: [u32; 2], x0: V3, max_move: f64) -> Option<V3> {
    let mut x = x0;
    for _ in 0..24 {
        let (d1, n1) = signed(g, s[0], x)?;
        let (d2, n2) = signed(g, s[1], x)?;
        if d1.abs().max(d2.abs()) < 1e-13 {
            return (norm(sub(x, x0)) <= max_move).then_some(x);
        }
        let c = dot(n1, n2);
        let det = 1.0 - c * c;
        if det < 1e-6 {
            return None;
        }
        // dx = a n1 + b n2 with n1.dx = -d1 and n2.dx = -d2.
        let (a, b) = ((-d1 + c * d2) / det, (-d2 + c * d1) / det);
        x = add(x, add(scale(n1, a), scale(n2, b)));
        if norm(sub(x, x0)) > max_move || !x.iter().all(|v| v.is_finite()) {
            return None;
        }
    }
    None
}

/// The unit tangent of the intersection of the two surfaces at `x`, which lies on both.
fn tangent(g: &Graph, s: [u32; 2], x: V3) -> Option<V3> {
    let (_, n1) = signed(g, s[0], x)?;
    let (_, n2) = signed(g, s[1], x)?;
    let t = cross(n1, n2);
    (norm(t) > 1e-6).then(|| unit(t)).flatten()
}

/// A C1 piecewise cubic through `p` whose tangents at the snapped points (`on`) are the
/// curve's own, so a segment is as accurate as a Hermite cubic can be (fourth order);
/// elsewhere the tangent follows the neighbouring points.
fn hermite(g: &Graph, s: [u32; 2], p: &[V3], on: &[bool]) -> Option<Nurbs> {
    let n = p.len();
    if n < 2 || on.len() != n {
        return None;
    }
    let mut tans = Vec::with_capacity(n);
    for i in 0..n {
        let a = *p.get(i.saturating_sub(1))?;
        let b = *p.get((i + 1).min(n - 1))?;
        let chord = unit(sub(b, a))?;
        let t = match (on.get(i), p.get(i)) {
            (Some(true), Some(&x)) => tangent(g, s, x),
            _ => None,
        };
        tans.push(match t {
            Some(t) if dot(t, chord) < 0.0 => scale(t, -1.0),
            Some(t) => t,
            None => chord,
        });
    }
    let first = *p.first()?;
    let mut poles = vec![first];
    let mut knots = vec![0.0];
    let mut mults = vec![4];
    let mut t = 0.0;
    for (i, w) in p.windows(2).enumerate() {
        let &[p0, p1] = w else { return None };
        let (t0, t1) = (*tans.get(i)?, *tans.get(i + 1)?);
        let chord = norm(sub(p1, p0));
        // The arc length of a circular arc with this chord and turn.
        let theta = libm::acos(dot(t0, t1).clamp(-1.0, 1.0));
        let len = if theta > 1e-6 { chord * (0.5 * theta) / libm::sin(0.5 * theta) } else { chord };
        poles.push(add(p0, scale(t0, len / 3.0)));
        poles.push(sub(p1, scale(t1, len / 3.0)));
        poles.push(p1);
        t += chord;
        knots.push(t);
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

fn is_analytic(g: &Graph, s: u32) -> bool {
    matches!(g.type_name(s), Some("PLANE" | "CYLINDER" | "SPHERE" | "TORUS" | "CONE"))
}

/// The distance of `q` from the farther of the two surfaces.
fn deviation(g: &Graph, s: [u32; 2], q: V3) -> Option<f64> {
    let mut worst: f64 = 0.0;
    for &si in &s {
        worst = worst.max(norm(sub(q, crate::eval::project(g, si, q)?)));
    }
    Some(worst)
}

/// Refine the chart points `pts` of the intersection curve node `c` and fit them; `None`
/// leaves the caller to fit the chart as given.
pub(crate) fn intersection(g: &Graph, c: u32, pts: &[V3]) -> Option<Nurbs> {
    let supports: Vec<u32> =
        g.list(c, "surface").iter().filter_map(|v| if let Val::Ptr(p) = v { Some(*p) } else { None }).collect();
    let &[s1, s2] = supports.as_slice() else { return None };
    if !is_analytic(g, s1) || !is_analytic(g, s2) || pts.len() < 3 || pts.len() > MAX_POINTS {
        return None;
    }
    let s = [s1, s2];
    let mut p: Vec<V3> = Vec::with_capacity(pts.len());
    for q in pts {
        if p.last().is_none_or(|l| norm(sub(*q, *l)) > 1e-12) {
            p.push(*q);
        }
    }
    if p.len() < 3 {
        return None;
    }
    // Chart points onto both surfaces; a point that does not snap stays where it is.
    let snapped: Vec<(V3, bool)> = p
        .iter()
        .enumerate()
        .map(|(i, &q)| {
            let gap = |j: Option<usize>| j.and_then(|j| p.get(j)).map_or(f64::INFINITY, |r| norm(sub(*r, q)));
            let near = gap(i.checked_sub(1)).min(gap(Some(i + 1)));
            snap(g, s, q, 0.25 * near).map_or((q, false), |r| (r, true))
        })
        .collect();
    // Where it did not (tangent supports, a crossing), the fit around it is left alone.
    let mut on: Vec<bool> = snapped.iter().map(|x| x.1).collect();
    p = snapped.iter().map(|x| x.0).collect();
    let first = *p.first()?;
    let extent = p.iter().map(|q| norm(sub(*q, first))).fold(0.0, f64::max);
    let tol = (1e-8 * extent).max(1e-12);
    let mut fit = hermite(g, s, &p, &on)?;
    for _ in 0..ROUNDS {
        let knots = fit.knots[0].clone();
        if knots.len() != p.len() {
            return None;
        }
        let mut next = Vec::with_capacity(p.len() * 2);
        let mut next_on = Vec::with_capacity(p.len() * 2);
        let mut grew = false;
        for (i, &pi) in p.iter().enumerate() {
            next.push(pi);
            next_on.push(on.get(i).copied().unwrap_or(false));
            let (Some(&t0), Some(&t1), Some(&pj)) = (knots.get(i), knots.get(i + 1), p.get(i + 1)) else { continue };
            let good = on.get(i).copied().unwrap_or(false) && on.get(i + 1).copied().unwrap_or(false);
            if !good || p.len() + next.len() > MAX_POINTS {
                continue;
            }
            // The worst of three samples along the segment.
            let mut worst = (tol, None);
            for f in [0.25, 0.5, 0.75] {
                let Some(q) = geom::eval_curve(&fit, t0 + (t1 - t0) * f) else { continue };
                let Some(d) = deviation(g, s, q) else { continue };
                if d > worst.0 {
                    worst = (d, Some(q));
                }
            }
            if let Some(q) = worst.1
                && let Some(r) = snap(g, s, q, 0.25 * norm(sub(pj, pi)))
            {
                next.push(r);
                next_on.push(true);
                grew = true;
            }
        }
        if !grew {
            break;
        }
        p = next;
        on = next_on;
        fit = hermite(g, s, &p, &on)?;
    }
    // Keep the refinement only if it is no worse than the chart's own fit would be.
    let fits = |n: &Nurbs| -> Option<f64> {
        let k = n.knots[0].clone();
        let mut w: f64 = 0.0;
        for (t0, t1) in k.iter().zip(k.iter().skip(1)) {
            w = w.max(deviation(g, s, geom::eval_curve(n, 0.5 * (t0 + t1))?)?);
        }
        Some(w)
    };
    let chart = geom::through(pts)?;
    match (fits(&fit), fits(&chart)) {
        (Some(a), Some(b)) if a <= b => Some(fit),
        (Some(_), None) => Some(fit),
        _ => None,
    }
}
