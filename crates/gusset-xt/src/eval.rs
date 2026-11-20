//! Evaluating the file's geometry: points on curves and surfaces from their parametric
//! definitions in the Reference, closest points on surfaces, and the two pieces of
//! procedural geometry STEP has no counterpart for: SP-curves (a 2D curve in a surface's
//! parameters), sampled and fitted, and rolling-ball blends (`blend.rs`) (ADR 0035).

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

/// The point at `(u, v)` of a NURBS surface.
pub(crate) fn nurbs_surface_at(s: &Nurbs, u: f64, v: f64) -> Option<V3> {
    geom::eval_surface(s, u, v)
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
        "B_CURVE" => geom::eval_curve(&*geom::curve_rc(g, g.ptr(c, "nurbs"))?, t),
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
            let nb = geom::curve_rc(g, g.ptr(c, "nurbs"))?;
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
        "B_SURFACE" => nurbs_surface_at(&*geom::surface_rc(g, g.ptr(s, "nurbs"))?, u, v),
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
    let nb = geom::curve_rc(g, g.ptr(b, "nurbs"))?;
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
        "SWEPT_SURF" | "BLENDED_EDGE" => crate::blend::foot(g, s, p, 0).map(|f| f.p),
        _ => project_uv(g, s, p).map(|x| x.0),
    }
}

/// The parameter range of a bounded curve (a whole turn of a circle or ellipse).
pub(crate) fn curve_range(g: &Graph, c: u32) -> Option<(f64, f64)> {
    match g.type_name(c)? {
        "CIRCLE" | "ELLIPSE" => Some((0.0, core::f64::consts::TAU)),
        "B_CURVE" => domain(&*geom::curve_rc(g, g.ptr(c, "nurbs"))?, 0),
        "TRIMMED_CURVE" => Some((g.real(c, "parm_1")?, g.real(c, "parm_2")?)),
        _ => None,
    }
}

/// The closest point to `p` on a parametric surface and its parameters: Newton steps from the
/// best of a coarse grid over the surface's parameter box.
pub(crate) fn project_uv(g: &Graph, s: u32, p: V3) -> Option<(V3, [f64; 2])> {
    let (lo, hi, closed) = surface_box_closed(g, s)?;
    let h = 1e-6 * (1.0 + (hi[0] - lo[0]).abs().max((hi[1] - lo[1]).abs()));
    // A closed direction wraps round (a foot across the seam is reached), an open one stops
    // at its edge.
    let keep = |x: f64, k: usize| -> f64 {
        let (a, b) = (lo.get(k).copied().unwrap_or(0.0), hi.get(k).copied().unwrap_or(1.0));
        if closed.get(k).copied().unwrap_or(false) && b > a { a + (x - a).rem_euclid(b - a) } else { x.clamp(a, b) }
    };
    // Gauss-Newton on |S(u, v) - p|, halving steps that do not bring the point nearer;
    // whether it settled.
    let newton = |mut uv: [f64; 2]| -> Option<([f64; 2], bool)> {
        let mut q = surface_at(g, s, uv[0], uv[1], 0)?;
        let mut dist = norm(sub(p, q));
        for _ in 0..30 {
            let du =
                scale(sub(surface_at(g, s, uv[0] + h, uv[1], 0)?, surface_at(g, s, uv[0] - h, uv[1], 0)?), 0.5 / h);
            let dv =
                scale(sub(surface_at(g, s, uv[0], uv[1] + h, 0)?, surface_at(g, s, uv[0], uv[1] - h, 0)?), 0.5 / h);
            let r = sub(p, q);
            let (a, b, c) = (dot(du, du), dot(du, dv), dot(dv, dv));
            let (e, f) = (dot(r, du), dot(r, dv));
            let det = a * c - b * b;
            if det.abs() < 1e-300 {
                return Some((uv, false));
            }
            let (su, sv) = ((e * c - f * b) / det, (a * f - b * e) / det);
            let mut k = 1.0;
            let mut better = None;
            for _ in 0..12 {
                let next = [keep(uv[0] + k * su, 0), keep(uv[1] + k * sv, 1)];
                let nq = surface_at(g, s, next[0], next[1], 0)?;
                let nd = norm(sub(p, nq));
                if nd <= dist {
                    better = Some((next, nq, nd));
                    break;
                }
                k *= 0.5;
            }
            // Settled: no step brings the point nearer, or the step is too small to matter.
            let Some((next, nq, nd)) = better else { return Some((uv, true)) };
            let moved = (next[0] - uv[0]).abs() + (next[1] - uv[1]).abs();
            let gain = dist - nd;
            (uv, q, dist) = (next, nq, nd);
            if su.abs() + sv.abs() < 1e-12 || moved < 1e-13 || gain <= 1e-15 * (1.0 + nd) {
                return Some((uv, true));
            }
        }
        Some((uv, false))
    };
    // Successive queries on one surface are close: start from the last answer when the point is
    // near the last one asked for (a far point may have its foot on another part of the
    // surface), and search the whole box when that does not settle.
    let warm = g.cache.borrow().uv.get(&s).copied();
    let near = warm.filter(|w| norm(sub(p, w.1)) <= 0.02 * w.2);
    let (found, size) = match near.and_then(|w| newton(w.0).map(|r| (r, w.2))) {
        Some(((uv, true), size)) => (uv, size),
        _ => {
            let n = 12;
            let mut best = (f64::INFINITY, lo);
            let (mut bl, mut bh) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for i in 0..=n {
                for j in 0..=n {
                    let uv =
                        [lo[0] + (hi[0] - lo[0]) * i as f64 / n as f64, lo[1] + (hi[1] - lo[1]) * j as f64 / n as f64];
                    if let Some(q) = surface_at(g, s, uv[0], uv[1], 0) {
                        let d = norm(sub(q, p));
                        if d < best.0 {
                            best = (d, uv);
                        }
                        (bl, bh) = geom::grow((bl, bh), q);
                    }
                }
            }
            (newton(best.1)?.0, norm(sub(bh, bl)))
        }
    };
    g.cache.borrow_mut().uv.insert(s, (found, p, size));
    Some((surface_at(g, s, found[0], found[1], 0)?, found))
}

/// A parameter box to search on surfaces without a closed-form projection, and whether the
/// surface closes on itself in each direction.
fn surface_box_closed(g: &Graph, s: u32) -> Option<([f64; 2], [f64; 2], [bool; 2])> {
    match g.type_name(s)? {
        "B_SURFACE" => {
            let n = geom::surface_rc(g, g.ptr(s, "nurbs"))?;
            let ((u0, u1), (v0, v1)) = (domain(&n, 0)?, domain(&n, 1)?);
            Some(([u0, v0], [u1, v1], n.closed))
        }
        "SPUN_SURF" => {
            let c = g.ptr(s, "profile");
            let (lo, hi) = match g.type_name(c)? {
                "B_CURVE" => domain(&*geom::curve_rc(g, g.ptr(c, "nurbs"))?, 0)?,
                "TRIMMED_CURVE" => (g.real(c, "parm_1")?, g.real(c, "parm_2")?),
                _ => (0.0, core::f64::consts::TAU),
            };
            Some(([lo, 0.0], [hi, core::f64::consts::TAU], [false, true]))
        }
        "OFFSET_SURF" => surface_box_closed(g, g.ptr(s, "surface")),
        _ => None,
    }
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
