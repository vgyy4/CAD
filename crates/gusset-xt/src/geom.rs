//! Small geometry helpers: vectors, NURBS data from the file, and a C1 cubic through a
//! polyline (for intersection curves, which the file stores as a chart of points).

use crate::decode::Graph;
use crate::reader::Val;

pub(crate) type V3 = [f64; 3];

pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(crate) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(crate) fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn unit(a: V3) -> Option<V3> {
    let n = norm(a);
    (n > 1e-300 && n.is_finite()).then(|| scale(a, 1.0 / n))
}

/// A NURBS curve or surface's data in Cartesian form.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Nurbs {
    pub(crate) degree: [usize; 2],
    /// Pole counts in u and v (v is 1 for a curve).
    pub(crate) n: [usize; 2],
    /// Poles, u-major (`iu * n_v + iv`).
    pub(crate) poles: Vec<V3>,
    pub(crate) weights: Option<Vec<f64>>,
    pub(crate) mults: [Vec<i64>; 2],
    pub(crate) knots: [Vec<f64>; 2],
    pub(crate) closed: [bool; 2],
}

fn reals(g: &Graph, node: u32, field: &str) -> Vec<f64> {
    g.list(node, field)
        .iter()
        .map(|v| match v {
            Val::Real(r) => *r,
            Val::Int(i) => *i as f64,
            _ => 0.0,
        })
        .collect()
}

fn ints(g: &Graph, node: u32, field: &str) -> Vec<i64> {
    g.list(node, field).iter().filter_map(|v| if let Val::Int(i) = v { Some(*i) } else { None }).collect()
}

fn poles(g: &Graph, verts: u32, dim: usize, rational: bool, count: usize) -> Option<(Vec<V3>, Option<Vec<f64>>)> {
    let flat = reals(g, verts, "vertices");
    if dim == 0 || flat.len() != dim.checked_mul(count)? {
        return None;
    }
    let mut ps = Vec::with_capacity(count);
    let mut ws = Vec::with_capacity(count);
    for c in flat.chunks_exact(dim) {
        let get = |i: usize| c.get(i).copied().unwrap_or(0.0);
        if rational {
            let w = get(dim - 1);
            if w.abs() < 1e-300 {
                return None;
            }
            // Stored premultiplied: (w x, w y, w z, w).
            ps.push([get(0) / w, get(1) / w, if dim >= 4 { get(2) / w } else { 0.0 }]);
            ws.push(w);
        } else {
            ps.push([get(0), get(1), if dim >= 3 { get(2) } else { 0.0 }]);
        }
    }
    Some((ps, rational.then_some(ws)))
}

/// A knot (or multiplicity) list cut to the count its NURBS node declares: files of schema
/// 25001 (SolidWorks 2017 on) store these lists padded with unset values and zeros to a
/// longer length, which would break the knot-count check.
fn trimmed<T>(mut v: Vec<T>, declared: Option<i64>) -> Vec<T> {
    if let Some(n) = declared.and_then(|n| usize::try_from(n).ok()) {
        v.truncate(n);
    }
    v
}

/// Highest NURBS degree read (OCCT's own limit); higher degrees are refused, not evaluated.
const MAX_DEGREE: usize = 25;

fn valid(degree: usize, n: usize, mults: &[i64], knots: &[f64]) -> bool {
    // Bounds first, so the sum below cannot overflow.
    if !(1..=MAX_DEGREE).contains(&degree)
        || n < degree + 1
        || mults.len() != knots.len()
        || knots.len() < 2
        || !mults.iter().all(|m| *m >= 1 && usize::try_from(*m).is_ok_and(|m| m <= degree + 1))
    {
        return false;
    }
    let total = mults.iter().try_fold(0usize, |acc, m| acc.checked_add(usize::try_from(*m).ok()?));
    total == n.checked_add(degree + 1) && knots.windows(2).all(|w| matches!(w, [a, b] if b > a))
}

/// A B_CURVE's NURBS_CURVE.
pub(crate) fn nurbs_curve(g: &Graph, nurbs: u32) -> Option<Nurbs> {
    let degree = usize::try_from(g.int(nurbs, "degree")?).ok()?;
    let n = usize::try_from(g.int(nurbs, "n_vertices")?).ok()?;
    let dim = usize::try_from(g.int(nurbs, "vertex_dim")?).ok()?;
    let rational = g.int(nurbs, "rational") == Some(1);
    let (ps, ws) = poles(g, g.ptr(nurbs, "bspline_vertices"), dim, rational, n)?;
    let declared = g.int(nurbs, "n_knots");
    let mults = trimmed(ints(g, g.ptr(nurbs, "knot_mult"), "mult"), declared);
    let knots = trimmed(reals(g, g.ptr(nurbs, "knots"), "knots"), declared);
    if !valid(degree, n, &mults, &knots) {
        return None;
    }
    let closed = g.int(nurbs, "closed") == Some(1) || g.int(nurbs, "periodic") == Some(1);
    Some(Nurbs {
        degree: [degree, 0],
        n: [n, 1],
        poles: ps,
        weights: ws,
        mults: [mults, Vec::new()],
        knots: [knots, Vec::new()],
        closed: [closed, false],
    })
}

/// A B_SURFACE's NURBS_SURF. The pole order (u-major) is not stated in the Reference and is
/// unverified against a sample file.
pub(crate) fn nurbs_surface(g: &Graph, nurbs: u32) -> Option<Nurbs> {
    let du = usize::try_from(g.int(nurbs, "u_degree")?).ok()?;
    let dv = usize::try_from(g.int(nurbs, "v_degree")?).ok()?;
    let nu = usize::try_from(g.int(nurbs, "n_u_vertices")?).ok()?;
    let nv = usize::try_from(g.int(nurbs, "n_v_vertices")?).ok()?;
    let dim = usize::try_from(g.int(nurbs, "vertex_dim")?).ok()?;
    let rational = g.int(nurbs, "rational") == Some(1);
    let (ps, ws) = poles(g, g.ptr(nurbs, "bspline_vertices"), dim, rational, nu.checked_mul(nv)?)?;
    let (cu_n, cv_n) = (g.int(nurbs, "n_u_knots"), g.int(nurbs, "n_v_knots"));
    let mu = trimmed(ints(g, g.ptr(nurbs, "u_knot_mult"), "mult"), cu_n);
    let mv = trimmed(ints(g, g.ptr(nurbs, "v_knot_mult"), "mult"), cv_n);
    let ku = trimmed(reals(g, g.ptr(nurbs, "u_knots"), "knots"), cu_n);
    let kv = trimmed(reals(g, g.ptr(nurbs, "v_knots"), "knots"), cv_n);
    if !valid(du, nu, &mu, &ku) || !valid(dv, nv, &mv, &kv) {
        return None;
    }
    let cu = g.int(nurbs, "u_closed") == Some(1) || g.int(nurbs, "u_periodic") == Some(1);
    let cv = g.int(nurbs, "v_closed") == Some(1) || g.int(nurbs, "v_periodic") == Some(1);
    Some(Nurbs {
        degree: [du, dv],
        n: [nu, nv],
        poles: ps,
        weights: ws,
        mults: [mu, mv],
        knots: [ku, kv],
        closed: [cu, cv],
    })
}

/// A C1 piecewise cubic through `pts` (centripetal Catmull-Rom, ends mirrored), as a
/// B-spline with interior knots of multiplicity 3; two points give a line segment.
pub(crate) fn through(pts: &[V3]) -> Option<Nurbs> {
    let mut p: Vec<V3> = Vec::with_capacity(pts.len());
    for q in pts {
        if p.last().is_none_or(|l| norm(sub(*q, *l)) > 1e-12) {
            p.push(*q);
        }
    }
    let n = p.len();
    if n < 2 {
        return None;
    }
    if n == 2 {
        return Some(Nurbs {
            degree: [1, 0],
            n: [2, 1],
            poles: p,
            weights: None,
            mults: [vec![2, 2], Vec::new()],
            knots: [vec![0.0, 1.0], Vec::new()],
            closed: [false, false],
        });
    }
    let at = |i: isize| -> V3 {
        let first = p.first().copied().unwrap_or([0.0; 3]);
        let last = p.last().copied().unwrap_or([0.0; 3]);
        if i < 0 {
            sub(scale(first, 2.0), p.get(1).copied().unwrap_or(first))
        } else if i as usize >= n {
            sub(scale(last, 2.0), p.get(n - 2).copied().unwrap_or(last))
        } else {
            p.get(i as usize).copied().unwrap_or(first)
        }
    };
    let mut poles = Vec::new();
    let mut knots = Vec::new();
    let mut mults = Vec::new();
    let mut t = 0.0;
    knots.push(0.0);
    mults.push(4);
    for i in 0..(n as isize - 1) {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let d = |a: V3, b: V3| norm(sub(a, b)).sqrt().max(1e-12);
        let (d0, d1, d2) = (d(p0, p1), d(p1, p2), d(p2, p3));
        // Tangents of the centripetal Catmull-Rom spline, scaled to the segment.
        let t1 =
            add(sub(scale(sub(p1, p0), 1.0 / d0), scale(sub(p2, p0), 1.0 / (d0 + d1))), scale(sub(p2, p1), 1.0 / d1));
        let t2 =
            add(sub(scale(sub(p2, p1), 1.0 / d1), scale(sub(p3, p1), 1.0 / (d1 + d2))), scale(sub(p3, p2), 1.0 / d2));
        let (a, b) = (add(p1, scale(t1, d1 / 3.0)), sub(p2, scale(t2, d1 / 3.0)));
        if poles.is_empty() {
            poles.push(p1);
        }
        poles.push(a);
        poles.push(b);
        poles.push(p2);
        t += d1;
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

/// The point at parameter `u` of a NURBS curve (de Boor), for vertices of ring edges.
pub(crate) fn eval_curve(c: &Nurbs, u: f64) -> Option<V3> {
    let p = c.degree[0];
    let mut flat = Vec::new();
    for (k, m) in c.knots[0].iter().zip(&c.mults[0]) {
        for _ in 0..*m {
            flat.push(*k);
        }
    }
    let n = c.poles.len();
    let lo = *flat.get(p)?;
    let hi = *flat.get(n)?;
    let u = u.clamp(lo, hi);
    let mut span = p;
    while span + 1 < n && *flat.get(span + 1)? <= u {
        span += 1;
    }
    let w = |i: usize| c.weights.as_ref().and_then(|w| w.get(i).copied()).unwrap_or(1.0);
    let mut d: Vec<[f64; 4]> = (0..=p)
        .map(|j| {
            let i = span + j - p;
            let q = c.poles.get(i).copied().unwrap_or([0.0; 3]);
            let wi = w(i);
            [q[0] * wi, q[1] * wi, q[2] * wi, wi]
        })
        .collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = span + j - p;
            let (a, b) = (*flat.get(i)?, *flat.get(i + p - r + 1)?);
            let alpha = if b > a { (u - a) / (b - a) } else { 0.0 };
            let prev = *d.get(j - 1)?;
            let cur = d.get_mut(j)?;
            for (c, p) in cur.iter_mut().zip(prev) {
                *c = (1.0 - alpha) * p + alpha * *c;
            }
        }
    }
    let r = d.get(p)?;
    (r[3].abs() > 1e-300).then(|| [r[0] / r[3], r[1] / r[3], r[2] / r[3]])
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn padded_knot_lists_are_cut_to_their_declared_count() {
        assert_eq!(trimmed(vec![2, 2, 0, 0], Some(2)), vec![2, 2]);
        assert_eq!(trimmed(vec![1.0, 2.0], Some(5)), vec![1.0, 2.0]);
        assert_eq!(trimmed(vec![1, 2], None), vec![1, 2]);
    }

    #[test]
    fn hostile_knot_vectors_are_refused() {
        // A cubic Bezier is fine.
        assert!(valid(3, 4, &[4, 4], &[0.0, 1.0]));
        // No poles at all (the multiplicities still add up): refused, not looped over.
        assert!(!valid(1, 0, &[1, 1], &[0.0, 1.0]));
        // A degree past OCCT's limit.
        assert!(!valid(1_000_000_000, 1_000_000_001, &[1_000_000_001, 1_000_000_001], &[0.0, 1.0]));
        // Multiplicities that would overflow a sum.
        assert!(!valid(3, 4, &[i64::MAX, i64::MAX], &[0.0, 1.0]));
    }

    #[test]
    fn the_cubic_through_points_passes_through_them() {
        let pts = [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0], [3.0, 1.0, 0.5]];
        let c = through(&pts).unwrap_or_else(|| panic!("a curve"));
        assert_eq!(c.poles.len(), 10);
        for (k, p) in c.knots[0].iter().zip(&pts) {
            let q = eval_curve(&c, *k).unwrap_or([9.0; 3]);
            assert!(norm(sub(q, *p)) < 1e-9, "{q:?} vs {p:?}");
        }
        assert!(through(&pts[..1]).is_none());
        assert_eq!(through(&pts[..2]).map(|c| c.degree[0]), Some(1));
    }
}
