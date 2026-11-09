//! Evaluating the file's geometry: points on curves and surfaces from their parametric
//! definitions in the Reference, closest points on surfaces, and the two pieces of
//! procedural geometry STEP has no counterpart for: SP-curves (a 2D curve in a surface's
//! parameters) and rolling-ball blends. Both are sampled and fitted (ADR 0035).

use crate::decode::Graph;
use crate::geom::{self, Nurbs, V3, add, cross, dot, norm, scale, sub, unit};
use crate::reader::Val;

/// The parameter range of a NURBS in one direction.
fn domain(n: &Nurbs, dir: usize) -> Option<(f64, f64)> {
    let mut flat = Vec::new();
    for (k, m) in n.knots.get(dir)?.iter().zip(n.mults.get(dir)?) {
        for _ in 0..*m {
            flat.push(*k);
        }
    }
    let d = *n.degree.get(dir)?;
    let count = *n.n.get(dir)?;
    Some((*flat.get(d)?, *flat.get(count)?))
}

/// The point at `(u, v)` of a NURBS surface: each row of poles evaluated in v, then the
/// resulting curve in u.
fn nurbs_surface_at(s: &Nurbs, u: f64, v: f64) -> Option<V3> {
    let (nu, nv) = (s.n[0], s.n[1]);
    let mut col = Vec::with_capacity(nu);
    let mut wcol = Vec::with_capacity(nu);
    for iu in 0..nu {
        let row: Vec<V3> = (0..nv).filter_map(|iv| s.poles.get(iu * nv + iv).copied()).collect();
        let w: Option<Vec<f64>> =
            s.weights.as_ref().map(|w| (0..nv).filter_map(|iv| w.get(iu * nv + iv).copied()).collect());
        // Homogeneous evaluation: weight the poles, evaluate, keep the weight.
        let (hp, hw) = match &w {
            Some(w) => (row.iter().zip(w).map(|(p, x)| scale(*p, *x)).collect::<Vec<_>>(), Some(w.clone())),
            None => (row, None),
        };
        let c = Nurbs {
            degree: [s.degree[1], 0],
            n: [nv, 1],
            poles: hp,
            weights: None,
            mults: [s.mults[1].clone(), Vec::new()],
            knots: [s.knots[1].clone(), Vec::new()],
            closed: [false, false],
        };
        col.push(geom::eval_curve(&c, v)?);
        let weight = match hw {
            Some(w) => {
                let wc = Nurbs { poles: w.iter().map(|x| [*x, 0.0, 0.0]).collect(), ..c.clone() };
                geom::eval_curve(&wc, v)?[0]
            }
            None => 1.0,
        };
        wcol.push(weight);
    }
    let c = Nurbs {
        degree: [s.degree[0], 0],
        n: [nu, 1],
        poles: col,
        weights: None,
        mults: [s.mults[0].clone(), Vec::new()],
        knots: [s.knots[0].clone(), Vec::new()],
        closed: [false, false],
    };
    let p = geom::eval_curve(&c, u)?;
    if s.weights.is_none() {
        return Some(p);
    }
    let wc = Nurbs { poles: wcol.iter().map(|x| [*x, 0.0, 0.0]).collect(), ..c };
    let w = geom::eval_curve(&wc, u)?[0];
    (w.abs() > 1e-300).then(|| scale(p, 1.0 / w))
}

/// A curve's point at parameter `t` (curves with a parameter law the Reference gives).
pub(crate) fn curve_at(g: &Graph, c: u32, t: f64, depth: u32) -> Option<V3> {
    if depth > 8 {
        return None;
    }
    match g.type_name(c)? {
        "LINE" => Some(add(g.vec3(c, "pvec")?, scale(g.vec3(c, "direction")?, t))),
        "CIRCLE" | "ELLIPSE" => {
            let (cn, n, x) = (g.vec3(c, "centre")?, unit(g.vec3(c, "normal")?)?, unit(g.vec3(c, "x_axis")?)?);
            let y = cross(n, x);
            let (a, b) = match g.real(c, "radius") {
                Some(r) => (r, r),
                None => (g.real(c, "major_radius")?, g.real(c, "minor_radius")?),
            };
            Some(add(cn, add(scale(x, a * libm::cos(t)), scale(y, b * libm::sin(t)))))
        }
        "B_CURVE" => geom::eval_curve(&geom::nurbs_curve(g, g.ptr(c, "nurbs"))?, t),
        "TRIMMED_CURVE" => curve_at(g, g.ptr(c, "basis_curve"), t, depth + 1),
        _ => None,
    }
}

/// Points along a whole curve, in its parameter direction: `n` samples of a bounded curve,
/// the chart of an intersection curve; `None` for an unbounded curve (a line).
pub(crate) fn curve_samples(g: &Graph, c: u32, n: usize, depth: u32) -> Option<Vec<V3>> {
    if depth > 8 {
        return None;
    }
    let even =
        |lo: f64, hi: f64| -> Vec<f64> { (0..n).map(|i| lo + (hi - lo) * i as f64 / (n - 1).max(1) as f64).collect() };
    match g.type_name(c)? {
        "CIRCLE" | "ELLIPSE" => {
            even(0.0, core::f64::consts::TAU).into_iter().map(|t| curve_at(g, c, t, depth)).collect()
        }
        "B_CURVE" => {
            let nb = geom::nurbs_curve(g, g.ptr(c, "nurbs"))?;
            let (lo, hi) = domain(&nb, 0)?;
            even(lo, hi).into_iter().map(|t| geom::eval_curve(&nb, t)).collect()
        }
        "TRIMMED_CURVE" => {
            let (a, b) = (g.real(c, "parm_1")?, g.real(c, "parm_2")?);
            even(a, b).into_iter().map(|t| curve_at(g, g.ptr(c, "basis_curve"), t, depth + 1)).collect()
        }
        "INTERSECTION" => {
            let pts: Vec<V3> = g
                .list(g.ptr(c, "chart"), "hvec")
                .iter()
                .filter_map(|v| if let Val::Vec3(p) = v { Some(*p) } else { None })
                .collect();
            (pts.len() >= 2).then_some(pts)
        }
        "SP_CURVE" => sp_points(g, c, n),
        _ => None,
    }
}

/// A surface's point at `(u, v)`.
pub(crate) fn surface_at(g: &Graph, s: u32, u: f64, v: f64, depth: u32) -> Option<V3> {
    if depth > 8 {
        return None;
    }
    let (cu, su) = (libm::cos(u), libm::sin(u));
    match g.type_name(s)? {
        "PLANE" => {
            let (n, x) = (unit(g.vec3(s, "normal")?)?, unit(g.vec3(s, "x_axis")?)?);
            Some(add(g.vec3(s, "pvec")?, add(scale(x, u), scale(cross(n, x), v))))
        }
        "CYLINDER" => {
            let (a, x) = (unit(g.vec3(s, "axis")?)?, unit(g.vec3(s, "x_axis")?)?);
            let r = g.real(s, "radius")?;
            let radial = add(scale(x, cu), scale(cross(a, x), su));
            Some(add(g.vec3(s, "pvec")?, add(scale(radial, r), scale(a, v))))
        }
        "CONE" => {
            let (a, x) = (unit(g.vec3(s, "axis")?)?, unit(g.vec3(s, "x_axis")?)?);
            let (r, sn, cs) = (g.real(s, "radius")?, g.real(s, "sin_half_angle")?, g.real(s, "cos_half_angle")?);
            let radial = add(scale(x, cu), scale(cross(a, x), su));
            Some(add(add(g.vec3(s, "pvec")?, scale(a, v)), scale(radial, r + v * sn / cs)))
        }
        "SPHERE" => {
            let (a, x) = (unit(g.vec3(s, "axis")?)?, unit(g.vec3(s, "x_axis")?)?);
            let r = g.real(s, "radius")?;
            let radial = add(scale(x, cu), scale(cross(a, x), su));
            Some(add(g.vec3(s, "centre")?, add(scale(radial, r * libm::cos(v)), scale(a, r * libm::sin(v)))))
        }
        "TORUS" => {
            let (a, x) = (unit(g.vec3(s, "axis")?)?, unit(g.vec3(s, "x_axis")?)?);
            let (ma, mi) = (g.real(s, "major_radius")?, g.real(s, "minor_radius")?);
            let radial = add(scale(x, cu), scale(cross(a, x), su));
            Some(add(g.vec3(s, "centre")?, add(scale(radial, ma + mi * libm::cos(v)), scale(a, mi * libm::sin(v)))))
        }
        "B_SURFACE" => nurbs_surface_at(&geom::nurbs_surface(g, g.ptr(s, "nurbs"))?, u, v),
        "SWEPT_SURF" => {
            Some(add(curve_at(g, g.ptr(s, "section"), u, depth + 1)?, scale(unit(g.vec3(s, "sweep")?)?, v)))
        }
        "SPUN_SURF" => {
            let c = curve_at(g, g.ptr(s, "profile"), u, depth + 1)?;
            let (p, a) = (g.vec3(s, "base")?, unit(g.vec3(s, "axis")?)?);
            let z = add(p, scale(a, dot(sub(c, p), a)));
            let d = sub(c, z);
            Some(add(z, add(scale(d, libm::cos(v)), scale(cross(a, d), libm::sin(v)))))
        }
        "OFFSET_SURF" => {
            let base = g.ptr(s, "surface");
            let p = surface_at(g, base, u, v, depth + 1)?;
            let n = natural_normal(g, base, u, v, depth + 1)?;
            let mut d = g.real(s, "offset")?;
            if g.ch(base, "sense") == Some(b'-') {
                d = -d;
            }
            Some(add(p, scale(n, d)))
        }
        _ => None,
    }
}

/// The unit natural normal (dP/du x dP/dv) by central differences.
pub(crate) fn natural_normal(g: &Graph, s: u32, u: f64, v: f64, depth: u32) -> Option<V3> {
    let h = 1e-6;
    let du = sub(surface_at(g, s, u + h, v, depth)?, surface_at(g, s, u - h, v, depth)?);
    let dv = sub(surface_at(g, s, u, v + h, depth)?, surface_at(g, s, u, v - h, depth)?);
    unit(cross(du, dv))
}

/// Points of an SP-curve: its 2D B-curve sampled in the surface's parameters.
fn sp_points(g: &Graph, c: u32, n: usize) -> Option<Vec<V3>> {
    let surface = g.ptr(c, "surface");
    let b = g.ptr(c, "b_curve");
    let nb = geom::nurbs_curve(g, g.ptr(b, "nurbs"))?;
    let (lo, hi) = domain(&nb, 0)?;
    (0..n)
        .map(|i| {
            let t = lo + (hi - lo) * i as f64 / (n - 1).max(1) as f64;
            let uv = geom::eval_curve(&nb, t)?;
            surface_at(g, surface, uv[0], uv[1], 1)
        })
        .collect()
}

/// The closest point to `p` on a surface: exact for the analytic surfaces, otherwise by
/// Newton steps from the best of a coarse grid over `box_uv`.
pub(crate) fn project(g: &Graph, s: u32, p: V3) -> Option<V3> {
    match g.type_name(s)? {
        "PLANE" => {
            let n = unit(g.vec3(s, "normal")?)?;
            Some(sub(p, scale(n, dot(sub(p, g.vec3(s, "pvec")?), n))))
        }
        "CYLINDER" => {
            let (o, a, r) = (g.vec3(s, "pvec")?, unit(g.vec3(s, "axis")?)?, g.real(s, "radius")?);
            let q = sub(p, o);
            let along = scale(a, dot(q, a));
            let radial = unit(sub(q, along))?;
            Some(add(o, add(along, scale(radial, r))))
        }
        "SPHERE" => {
            let (c, r) = (g.vec3(s, "centre")?, g.real(s, "radius")?);
            Some(add(c, scale(unit(sub(p, c))?, r)))
        }
        "TORUS" => {
            let (c, a) = (g.vec3(s, "centre")?, unit(g.vec3(s, "axis")?)?);
            let (ma, mi) = (g.real(s, "major_radius")?, g.real(s, "minor_radius")?);
            let q = sub(p, c);
            let radial = unit(sub(q, scale(a, dot(q, a))))?;
            let ring = add(c, scale(radial, ma));
            Some(add(ring, scale(unit(sub(p, ring))?, mi)))
        }
        "CONE" => {
            // In the plane of the axis and p: the nearest point on the generator line.
            let (o, a, x) = (g.vec3(s, "pvec")?, unit(g.vec3(s, "axis")?)?, unit(g.vec3(s, "x_axis")?)?);
            let (r, sn, cs) = (g.real(s, "radius")?, g.real(s, "sin_half_angle")?, g.real(s, "cos_half_angle")?);
            let q = sub(p, o);
            let radial = unit(sub(q, scale(a, dot(q, a)))).unwrap_or(x);
            let start = add(o, scale(radial, r));
            // Direction of increasing v: A + tan(a) * radial.
            let dir = unit(add(scale(a, cs), scale(radial, sn)))?;
            Some(add(start, scale(dir, dot(sub(p, start), dir))))
        }
        _ => project_numeric(g, s, p),
    }
}

fn project_numeric(g: &Graph, s: u32, p: V3) -> Option<V3> {
    let (lo, hi) = surface_box(g, s)?;
    let n = 12;
    let mut best = (f64::INFINITY, lo);
    for i in 0..=n {
        for j in 0..=n {
            let uv = [lo[0] + (hi[0] - lo[0]) * i as f64 / n as f64, lo[1] + (hi[1] - lo[1]) * j as f64 / n as f64];
            if let Some(q) = surface_at(g, s, uv[0], uv[1], 0) {
                let d = norm(sub(q, p));
                if d < best.0 {
                    best = (d, uv);
                }
            }
        }
    }
    let mut uv = best.1;
    let h = 1e-6 * (1.0 + (hi[0] - lo[0]).abs().max((hi[1] - lo[1]).abs()));
    for _ in 0..30 {
        let q = surface_at(g, s, uv[0], uv[1], 0)?;
        let du = scale(sub(surface_at(g, s, uv[0] + h, uv[1], 0)?, surface_at(g, s, uv[0] - h, uv[1], 0)?), 0.5 / h);
        let dv = scale(sub(surface_at(g, s, uv[0], uv[1] + h, 0)?, surface_at(g, s, uv[0], uv[1] - h, 0)?), 0.5 / h);
        let r = sub(p, q);
        // Gauss-Newton on |S(u, v) - p|.
        let (a, b, c) = (dot(du, du), dot(du, dv), dot(dv, dv));
        let (e, f) = (dot(r, du), dot(r, dv));
        let det = a * c - b * b;
        if det.abs() < 1e-300 {
            break;
        }
        let (su, sv) = ((e * c - f * b) / det, (a * f - b * e) / det);
        uv = [(uv[0] + su).clamp(lo[0], hi[0]), (uv[1] + sv).clamp(lo[1], hi[1])];
        if su.abs() + sv.abs() < 1e-12 {
            break;
        }
    }
    surface_at(g, s, uv[0], uv[1], 0)
}

/// A parameter box to search on surfaces without a closed-form projection.
fn surface_box(g: &Graph, s: u32) -> Option<([f64; 2], [f64; 2])> {
    match g.type_name(s)? {
        "B_SURFACE" => {
            let n = geom::nurbs_surface(g, g.ptr(s, "nurbs"))?;
            let ((u0, u1), (v0, v1)) = (domain(&n, 0)?, domain(&n, 1)?);
            Some(([u0, v0], [u1, v1]))
        }
        "SPUN_SURF" => {
            let c = g.ptr(s, "profile");
            let (lo, hi) = match g.type_name(c)? {
                "B_CURVE" => domain(&geom::nurbs_curve(g, g.ptr(c, "nurbs"))?, 0)?,
                "TRIMMED_CURVE" => (g.real(c, "parm_1")?, g.real(c, "parm_2")?),
                _ => (0.0, core::f64::consts::TAU),
            };
            Some(([lo, 0.0], [hi, core::f64::consts::TAU]))
        }
        "OFFSET_SURF" => surface_box(g, g.ptr(s, "surface")),
        _ => None,
    }
}

/// A grid of points on a rolling-ball blend: rows along the spine, each a circular arc of
/// the ball from its contact with the first support surface to the second (reversed when
/// the spine's sense is negative, as the Reference's v parameter is).
pub(crate) fn blend_grid(g: &Graph, s: u32, hint: &[V3]) -> Option<Vec<Vec<V3>>> {
    let spine = g.ptr(s, "spine");
    let supports: Vec<u32> =
        g.list(s, "surface").iter().filter_map(|v| if let Val::Ptr(p) = v { Some(*p) } else { None }).collect();
    let (&[s1, s2], true) = (supports.as_slice(), g.ch(s, "blend_type") != Some(b'E')) else { return None };
    let mut centres = match curve_samples(g, spine, 33, 0) {
        Some(c) => c,
        None => line_samples(g, spine, hint, 9)?,
    };
    if centres.len() > 64 {
        let step = centres.len().div_ceil(64);
        let last = centres.last().copied();
        centres = centres.into_iter().step_by(step).collect();
        if let (Some(l), Some(c)) = (last, centres.last())
            && norm(sub(l, *c)) > 0.0
        {
            centres.push(l);
        }
    }
    let flip = g.ch(spine, "sense") == Some(b'-');
    let arc = 9;
    let mut rows = Vec::new();
    for c in centres {
        let (p1, p2) = (project(g, s1, c)?, project(g, s2, c)?);
        let (a, b) = if flip { (p2, p1) } else { (p1, p2) };
        let (e1, e2) = (unit(sub(a, c))?, unit(sub(b, c))?);
        let r = 0.5 * (norm(sub(a, c)) + norm(sub(b, c)));
        let cosang = dot(e1, e2).clamp(-1.0, 1.0);
        let angle = libm::acos(cosang);
        let w = unit(sub(e2, scale(e1, cosang)))?;
        let row: Vec<V3> = (0..arc)
            .map(|k| {
                let phi = angle * k as f64 / (arc - 1) as f64;
                add(c, scale(add(scale(e1, libm::cos(phi)), scale(w, libm::sin(phi))), r))
            })
            .collect();
        rows.push(row);
    }
    (rows.len() >= 2).then_some(rows)
}

/// Points along a line spine over the extent of `hint` (the blend faces' vertices) projected
/// onto it, a little beyond.
fn line_samples(g: &Graph, c: u32, hint: &[V3], n: usize) -> Option<Vec<V3>> {
    if g.type_name(c)? != "LINE" {
        return None;
    }
    let (p, d) = (g.vec3(c, "pvec")?, unit(g.vec3(c, "direction")?)?);
    let ts: Vec<f64> = hint.iter().map(|q| dot(sub(*q, p), d)).collect();
    let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !(lo.is_finite() && hi > lo) {
        return None;
    }
    let pad = 0.05 * (hi - lo);
    let (lo, hi) = (lo - pad, hi + pad);
    // The direction sense of the line stays: samples run with increasing t.
    Some((0..n).map(|i| add(p, scale(d, lo + (hi - lo) * i as f64 / (n - 1) as f64))).collect())
}

/// A bicubic B-spline through a grid (uniform Catmull-Rom in both directions, as Bézier
/// patches), rows along u.
pub(crate) fn surface_through(rows: &[Vec<V3>]) -> Option<Nurbs> {
    let nu = rows.len();
    let nv = rows.first()?.len();
    if nu < 2 || nv < 2 || rows.iter().any(|r| r.len() != nv) {
        return None;
    }
    let curve = |pts: &[V3]| -> Vec<V3> {
        let n = pts.len();
        let at = |i: isize| -> V3 {
            let first = pts.first().copied().unwrap_or([0.0; 3]);
            let last = pts.last().copied().unwrap_or([0.0; 3]);
            if i < 0 {
                sub(scale(first, 2.0), pts.get(1).copied().unwrap_or(first))
            } else if i as usize >= n {
                sub(scale(last, 2.0), pts.get(n.saturating_sub(2)).copied().unwrap_or(last))
            } else {
                pts.get(i as usize).copied().unwrap_or(first)
            }
        };
        let mut out = Vec::new();
        for i in 0..(n as isize - 1) {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            if out.is_empty() {
                out.push(p1);
            }
            out.push(add(p1, scale(sub(p2, p0), 1.0 / 6.0)));
            out.push(sub(p2, scale(sub(p3, p1), 1.0 / 6.0)));
            out.push(p2);
        }
        out
    };
    let bez_rows: Vec<Vec<V3>> = rows.iter().map(|r| curve(r)).collect();
    let mv = bez_rows.first()?.len();
    let mut cols: Vec<Vec<V3>> =
        (0..mv).map(|j| curve(&bez_rows.iter().filter_map(|r| r.get(j).copied()).collect::<Vec<_>>())).collect();
    let mu = cols.first()?.len();
    let mut poles = Vec::with_capacity(mu * mv);
    for iu in 0..mu {
        for col in &mut cols {
            poles.push(col.get(iu).copied()?);
        }
    }
    let knots = |n: usize| -> (Vec<i64>, Vec<f64>) {
        let segs = n - 1;
        let mut m = vec![4];
        m.extend(std::iter::repeat_n(3, segs.saturating_sub(1)));
        m.push(4);
        (m, (0..=segs).map(|k| k as f64).collect())
    };
    let ((ku_m, ku), (kv_m, kv)) = (knots(nu), knots(nv));
    Some(Nurbs {
        degree: [3, 3],
        n: [mu, mv],
        poles,
        weights: None,
        mults: [ku_m, kv_m],
        knots: [ku, kv],
        closed: [false, false],
    })
}

/// The same surface data read with the poles in v-major order (`iv * n_u + iu`).
pub(crate) fn transposed_poles(n: &Nurbs) -> Nurbs {
    let (nu, nv) = (n.n[0], n.n[1]);
    let mut out = n.clone();
    for iu in 0..nu {
        for iv in 0..nv {
            if let (Some(dst), Some(src)) = (out.poles.get_mut(iu * nv + iv), n.poles.get(iv * nu + iu)) {
                *dst = *src;
            }
        }
    }
    if let (Some(w), Some(src)) = (out.weights.as_mut(), n.weights.as_ref()) {
        for iu in 0..nu {
            for iv in 0..nv {
                if let (Some(dst), Some(x)) = (w.get_mut(iu * nv + iv), src.get(iv * nu + iu)) {
                    *dst = *x;
                }
            }
        }
    }
    out
}

/// How far `p` lies from a NURBS surface (grid search, then Gauss-Newton).
pub(crate) fn nurbs_distance(s: &Nurbs, p: V3) -> Option<f64> {
    let ((u0, u1), (v0, v1)) = (domain(s, 0)?, domain(s, 1)?);
    let at = |u: f64, v: f64| nurbs_surface_at(s, u, v);
    let n = 16;
    let mut best = (f64::INFINITY, [u0, v0]);
    for i in 0..=n {
        for j in 0..=n {
            let uv = [u0 + (u1 - u0) * i as f64 / n as f64, v0 + (v1 - v0) * j as f64 / n as f64];
            if let Some(q) = at(uv[0], uv[1]) {
                let d = norm(sub(q, p));
                if d < best.0 {
                    best = (d, uv);
                }
            }
        }
    }
    let mut uv = best.1;
    let h = 1e-7 * (1.0 + (u1 - u0).abs().max((v1 - v0).abs()));
    for _ in 0..20 {
        let q = at(uv[0], uv[1])?;
        let du = scale(sub(at((uv[0] + h).min(u1), uv[1])?, at((uv[0] - h).max(u0), uv[1])?), 0.5 / h);
        let dv = scale(sub(at(uv[0], (uv[1] + h).min(v1))?, at(uv[0], (uv[1] - h).max(v0))?), 0.5 / h);
        let r = sub(p, q);
        let (a, b, c) = (dot(du, du), dot(du, dv), dot(dv, dv));
        let det = a * c - b * b;
        if det.abs() < 1e-300 {
            break;
        }
        let (e, f) = (dot(r, du), dot(r, dv));
        uv = [(uv[0] + (e * c - f * b) / det).clamp(u0, u1), (uv[1] + (a * f - b * e) / det).clamp(v0, v1)];
    }
    Some(norm(sub(at(uv[0], uv[1])?, p)).min(best.0))
}
