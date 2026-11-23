//! Offsets of tube-like B-surfaces that are only C0 in their parameters (an interior knot of
//! full multiplicity, as where two rational half circles meet). OCCT refuses an offset of a C0
//! basis when it reads STEP and leaves the face out (054's thin-walled pipes), so where the
//! basis's u lines are circles of one radius (two half circles meeting at the C0 knot), the
//! offset is written as the basis with each row of poles scaled about the centre of the circle
//! (or ellipse) the row describes: a tube of the offset radius, in the basis's parameters,
//! checked against the true offset at a grid of points (ADR 0035).

use crate::geom::{self, Nurbs, V3, add, cross, norm, scale, sub, unit};

/// Whether a B-surface has an interior knot of multiplicity at least its degree in either
/// direction (it is only C0 there in its parameters).
pub(crate) fn is_c0(n: &Nurbs) -> bool {
    (0..2).any(|dir| {
        let (Some(d), Some(m)) = (n.degree.get(dir), n.mults.get(dir)) else { return false };
        let d = i64::try_from(*d).unwrap_or(i64::MAX);
        m.len() > 2 && m.iter().skip(1).take(m.len() - 2).any(|x| *x >= d)
    })
}

/// The parameter range of one direction.
fn domain(n: &Nurbs, dir: usize) -> Option<(f64, f64)> {
    let mut flat = Vec::new();
    for (k, m) in n.knots.get(dir)?.iter().zip(n.mults.get(dir)?) {
        for _ in 0..*m {
            flat.push(*k);
        }
    }
    Some((*flat.get(*n.degree.get(dir)?)?, *flat.get(*n.n.get(dir)?)?))
}

/// Row `j` of poles (fixed v index) as a curve in u, with its weights.
fn row(n: &Nurbs, j: usize) -> Option<Nurbs> {
    let (nu, nv) = (n.n[0], n.n[1]);
    let poles: Vec<V3> = (0..nu).map(|i| n.poles.get(i * nv + j).copied()).collect::<Option<_>>()?;
    let weights = match &n.weights {
        Some(w) => Some((0..nu).map(|i| w.get(i * nv + j).copied()).collect::<Option<Vec<f64>>>()?),
        None => None,
    };
    Some(Nurbs {
        degree: [n.degree[0], 0],
        n: [nu, 1],
        poles,
        weights,
        mults: [n.mults[0].clone(), Vec::new()],
        knots: [n.knots[0].clone(), Vec::new()],
        closed: [n.closed[0], false],
    })
}

/// The derivative of `f` at `x` by differences of second order with step `h`: one-sided at
/// the ends of `[lo, hi]`, central elsewhere (the basis is G1 at its C0 knots, so central
/// differences there give the normal's direction).
fn derivative(f: &dyn Fn(f64) -> Option<V3>, x: f64, (lo, hi): (f64, f64), h: f64) -> Option<V3> {
    let one_sided = |s: f64| -> Option<V3> {
        let (f0, f1, f2) = (f(x)?, f(x + s * h)?, f(x + 2.0 * s * h)?);
        Some(scale(add(add(scale(f0, -3.0), scale(f1, 4.0)), scale(f2, -1.0)), s / (2.0 * h)))
    };
    if x - h < lo {
        one_sided(1.0)
    } else if x + h > hi {
        one_sided(-1.0)
    } else {
        Some(scale(sub(f(x + h)?, f(x - h)?), 0.5 / h))
    }
}

/// The true offset of `n` by `d` (along the unit natural normal) at `(u, v)`.
fn offset_at(n: &Nurbs, d: f64, u: f64, v: f64, h: [f64; 2]) -> Option<V3> {
    let (du, dv) = (domain(n, 0)?, domain(n, 1)?);
    let su = derivative(&|a| geom::eval_surface(n, a, v), u, du, h[0])?;
    let sv = derivative(&|b| geom::eval_surface(n, u, b), v, dv, h[1])?;
    Some(add(geom::eval_surface(n, u, v)?, scale(unit(cross(su, sv))?, d)))
}

/// The offset of `n` by `d` (model units, along its natural normal) as a B-surface in the same
/// parameters, if `n` is a tube (every u line a circle of one radius) and the scaled tube is
/// within `tol` of the true offset everywhere sampled.
pub(crate) fn tube(n: &Nurbs, d: f64, tol: f64) -> Option<Nurbs> {
    let (lu, hu) = domain(n, 0)?;
    let (lv, hv) = domain(n, 1)?;
    if hu <= lu || hv <= lv {
        return None;
    }
    let h = [1e-5 * (hu - lu), 1e-5 * (hv - lv)];
    // The u lines are taken as two half circles meeting at a C0 knot (or affine images of
    // them in the rows of poles): the row's points there and at the start are opposite ends of
    // a diameter, their midpoint the centre.
    let deg = i64::try_from(n.degree[0]).ok()?;
    let mid = n.knots[0].iter().zip(&n.mults[0]).find(|(k, m)| **k > lu && **k < hu && **m >= deg).map(|(k, _)| *k)?;
    let nv = n.n[1];
    let mut centres = Vec::with_capacity(nv);
    for j in 0..nv {
        let r = row(n, j)?;
        centres.push(scale(add(geom::eval_curve(&r, lu)?, geom::eval_curve(&r, mid)?), 0.5));
    }
    // Each row's scale moves the u line at its Greville abscissa by the offset (the tube's
    // radius may drift a little along it, as in a fitted sweep).
    let q = 0.5 * (lu + mid);
    let scale_at = |v: f64| -> Option<f64> {
        let c = scale(add(geom::eval_surface(n, lu, v)?, geom::eval_surface(n, mid, v)?), 0.5);
        let k = norm(sub(offset_at(n, d, q, v, h)?, c)) / norm(sub(geom::eval_surface(n, q, v)?, c));
        (k.is_finite() && k > 0.0).then_some(k)
    };
    let mut flat = Vec::new();
    for (kn, m) in n.knots[1].iter().zip(&n.mults[1]) {
        for _ in 0..*m {
            flat.push(*kn);
        }
    }
    let p = n.degree[1].max(1);
    let mut scales = Vec::with_capacity(nv);
    for j in 0..nv {
        #[allow(clippy::cast_precision_loss)]
        let g = flat.get(j + 1..=j + p)?.iter().sum::<f64>() / p as f64;
        scales.push(scale_at(g.clamp(lv, hv))?);
    }
    let mut poles = n.poles.clone();
    for (idx, p) in poles.iter_mut().enumerate() {
        let (cj, kj) = (*centres.get(idx % nv)?, *scales.get(idx % nv)?);
        *p = add(cj, scale(sub(*p, cj), kj));
    }
    let t = Nurbs { poles, ..n.clone() };
    // Checked against the true offset on a grid, four points to a row of poles in v: the
    // distance across the tube (along its normal; a point may slide along the surface where
    // the tube's sections are not quite square to it).
    let (gu, gv) = (64, 4 * nv.max(8));
    let mut worst: f64 = 0.0;
    for i in 0..=gu {
        for j in 0..=gv {
            #[allow(clippy::cast_precision_loss)]
            let (u, v) = (lu + (hu - lu) * i as f64 / gu as f64, lv + (hv - lv) * j as f64 / gv as f64);
            let gap = sub(offset_at(n, d, u, v, h)?, geom::eval_surface(&t, u, v)?);
            let normal = sub(offset_at(&t, 1.0, u, v, h)?, geom::eval_surface(&t, u, v)?);
            worst = worst.max(geom::dot(gap, normal).abs());
        }
    }
    if worst > tol {
        return None;
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed tube of radius 1 round a quarter circle of radius 3 in the xy plane (a quarter
    /// torus): its sections two rational cubic half circles meeting at a triple knot (C0 in u,
    /// though tangent-continuous), revolved as a rational quadratic arc.
    fn bent_tube() -> Nurbs {
        let ring = [[1.0, 0.0], [1.0, 2.0], [-1.0, 2.0], [-1.0, 0.0], [-1.0, -2.0], [1.0, -2.0], [1.0, 0.0]];
        let wu = [1.0, 1.0 / 3.0, 1.0 / 3.0, 1.0, 1.0 / 3.0, 1.0 / 3.0, 1.0];
        // The arc's pole directions, their lengths' scale and weights.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let arc = [([1.0, 0.0], 1.0, 1.0), ([s, s], 1.0 / s, s), ([0.0, 1.0], 1.0, 1.0)];
        let (mut poles, mut weights) = (Vec::new(), Vec::new());
        for (p, wi) in ring.iter().zip(wu) {
            for (dir, grow, wv) in arc {
                let rho = (3.0 + p[0]) * grow;
                poles.push([dir[0] * rho, dir[1] * rho, p[1]]);
                weights.push(wi * wv);
            }
        }
        Nurbs {
            degree: [3, 2],
            n: [7, 3],
            poles,
            weights: Some(weights),
            mults: [vec![4, 3, 4], vec![3, 3]],
            knots: [vec![0.0, 0.5, 1.0], vec![0.0, 1.0]],
            closed: [true, false],
        }
    }

    /// The least and greatest distance of the surface from the tube's spine.
    fn radii(n: &Nurbs) -> (f64, f64) {
        let mut r = (f64::MAX, f64::MIN);
        for i in 0..=40 {
            for j in 0..=40 {
                let p = geom::eval_surface(n, f64::from(i) / 40.0, f64::from(j) / 40.0).unwrap_or([9.0; 3]);
                let d = libm::hypot(libm::hypot(p[0], p[1]) - 3.0, p[2]);
                r = (r.0.min(d), r.1.max(d));
            }
        }
        r
    }

    #[test]
    fn an_offset_of_a_c0_tube_is_the_tube_scaled() {
        let n = bent_tube();
        assert!(is_c0(&n));
        let r = radii(&n);
        assert!((r.0 - 1.0).abs() < 1e-12 && (r.1 - 1.0).abs() < 1e-12, "{r:?}");
        // Offsets by 0.25 either way: tubes of radius 1.25 and 0.75 round the same spine.
        let mut got = Vec::new();
        for d in [0.25, -0.25] {
            let t = tube(&n, d, 1e-9).unwrap_or_else(|| panic!("a tube for {d}"));
            let r = radii(&t);
            assert!(r.1 - r.0 < 1e-12, "{r:?}");
            got.push(r.0);
        }
        got.sort_by(f64::total_cmp);
        assert!(matches!(got.as_slice(), [a, b] if (a - 0.75).abs() < 1e-12 && (b - 1.25).abs() < 1e-12), "{got:?}");
        // A surface whose u lines are not circles of one radius is left alone.
        let mut flat = n.clone();
        for (idx, p) in flat.poles.iter_mut().enumerate() {
            if idx % 3 == 2 {
                p[2] *= 2.0;
            }
        }
        assert!(tube(&flat, 0.25, 1e-9).is_none());
        // A Bezier patch is not C0.
        let poles = n.poles.iter().take(12).copied().collect();
        let bezier = Nurbs { mults: [vec![4, 4], vec![3, 3]], n: [4, 3], poles, weights: None, ..n };
        assert!(!is_c0(&bezier));
    }
}
