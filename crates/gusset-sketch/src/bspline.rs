//! Exact (rational) B-spline curves: the checks on their data and their cut into one
//! rational Bézier segment per non-empty knot span (ADR 0016 amendment "exact B-splines").
//!
//! Conventions (the same as STEP, OCCT and the Fusion design stream):
//! * `degree` p in `1..=`[`MAX_BEZIER_DEGREE`], n poles, n weights (positive).
//! * Non-periodic: a flat knot vector of `n + p + 1` non-decreasing values; the curve runs
//!   over `[t_p, t_n]`. Clamped curves (end knots repeated `p + 1` times) start and end at
//!   their first and last pole; unclamped ones are allowed too. An interior knot may repeat
//!   at most `p` times.
//! * Periodic: `n + 1` non-decreasing knots `t_0 .. t_n`; the period is `T = t_n - t_0` and
//!   the knot sequence continues as `t_{i+n} = t_i + T`. Pole `i` weights the basis
//!   function that starts at `t_{i-p}` (OCCT's convention: its first pole belongs to the
//!   function that starts `p` knots before the first knot). A knot (the seam included)
//!   repeats at most `p` times.

use crate::bezier::{MAX_BEZIER_DEGREE, RationalBezier};
use crate::geom2::{V2, dist};

/// Most poles an exact B-spline may have.
pub const MAX_BSPLINE_POLES: usize = 1000;

/// Largest absolute knot value.
const MAX_KNOT: f64 = 1.0e9;

/// The data of one B-spline curve.
#[derive(Debug, Clone, Copy)]
pub struct BSplineData<'a> {
    pub degree: usize,
    pub knots: &'a [f64],
    pub poles: &'a [V2],
    pub weights: &'a [f64],
    pub periodic: bool,
}

/// How many knots a B-spline of `n` poles and degree `p` has.
pub fn knot_count(n: usize, p: usize, periodic: bool) -> usize {
    if periodic { n.saturating_add(1) } else { n.saturating_add(p).saturating_add(1) }
}

/// Checks degree, pole count, knots and weights (`n` poles); the message says what is wrong.
pub fn check(degree: usize, n: usize, knots: &[f64], weights: &[f64], periodic: bool) -> Result<(), String> {
    if !(1..=MAX_BEZIER_DEGREE).contains(&degree) {
        return Err(format!("a B-spline's degree must be 1 to {MAX_BEZIER_DEGREE}, not {degree}"));
    }
    if n < degree + 1 {
        return Err(format!("a B-spline of degree {degree} needs at least {} poles, not {n}", degree + 1));
    }
    if n > MAX_BSPLINE_POLES {
        return Err(format!("a B-spline has at most {MAX_BSPLINE_POLES} poles"));
    }
    let want = knot_count(n, degree, periodic);
    if knots.len() != want {
        return Err(format!(
            "a {}B-spline of degree {degree} with {n} poles has {want} knots ({}), not {}",
            if periodic { "periodic " } else { "" },
            if periodic { "poles + 1" } else { "poles + degree + 1" },
            knots.len()
        ));
    }
    if weights.len() != n {
        return Err(format!("a B-spline has one weight per pole: {n}, not {}", weights.len()));
    }
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_KNOT) {
        return Err("a B-spline knot is not a finite number".into());
    }
    if knots.windows(2).any(|w| matches!(w, [a, b] if b < a)) {
        return Err("a B-spline's knots must not decrease".into());
    }
    if weights.iter().any(|w| !(w.is_finite() && *w > 0.0 && *w <= MAX_KNOT && *w >= 1.0 / MAX_KNOT)) {
        return Err("a B-spline's weights must be positive finite numbers".into());
    }
    let (first, last) = match (knots.first(), knots.last()) {
        (Some(&f), Some(&l)) => (f, l),
        _ => return Err("a B-spline has no knots".into()),
    };
    // Knots are compared exactly: a multiplicity is a count of equal values.
    let same = |a: f64, b: f64| a.total_cmp(&b).is_eq();
    let mult = |v: f64| knots.iter().filter(|&&k| same(k, v)).count();
    if periodic {
        if last.partial_cmp(&first) != Some(core::cmp::Ordering::Greater) {
            return Err("a periodic B-spline's last knot must be greater than its first".into());
        }
        // The seam: t_0 and t_n are the same knot of the periodic sequence.
        let seam = mult(first) + mult(last);
        if seam > degree {
            return Err(format!("a periodic B-spline's seam knot repeats {seam} times, more than its degree {degree}"));
        }
        for &k in knots {
            if !same(k, first) && !same(k, last) && mult(k) > degree {
                return Err(format!("knot {k} repeats {} times, more than the degree {degree}", mult(k)));
            }
        }
    } else {
        let (lo, hi) = match (knots.get(degree), knots.get(n)) {
            (Some(&a), Some(&b)) => (a, b),
            _ => return Err("a B-spline has too few knots".into()),
        };
        if hi.partial_cmp(&lo) != Some(core::cmp::Ordering::Greater) {
            return Err(format!("a B-spline's domain [knot {degree}, knot {n}] = [{lo}, {hi}] is empty"));
        }
        for &k in knots {
            let m = mult(k);
            let limit = if k > lo && k < hi { degree } else { degree + 1 };
            if m > limit {
                return Err(format!("knot {k} repeats {m} times, more than {limit}"));
            }
        }
    }
    Ok(())
}

/// The equivalent non-periodic data of a periodic B-spline: `n + p` poles (the first `p`
/// repeated at the end) and `n + 2p + 1` knots; the domain is `[t_0, t_n]`.
fn unwrap(b: &BSplineData<'_>) -> (Vec<f64>, Vec<V2>, Vec<f64>) {
    let (p, n) = (b.degree, b.poles.len());
    if !b.periodic {
        return (b.knots.to_vec(), b.poles.to_vec(), b.weights.to_vec());
    }
    let (t0, tn) = (b.knots.first().copied().unwrap_or(0.0), b.knots.last().copied().unwrap_or(1.0));
    let period = tn - t0;
    let ni = n as isize;
    let knots = (0..=(n + 2 * p))
        .map(|j| {
            let i = j as isize - p as isize;
            let (q, r) = (i.div_euclid(ni), i.rem_euclid(ni));
            b.knots.get(r as usize).copied().unwrap_or(0.0) + q as f64 * period
        })
        .collect();
    let poles = (0..n + p).map(|j| b.poles.get(j % n).copied().unwrap_or([0.0; 2])).collect();
    let weights = (0..n + p).map(|j| b.weights.get(j % n).copied().unwrap_or(1.0)).collect();
    (knots, poles, weights)
}

/// The blossom of the curve's span `i` (`k_i <= .. < k_{i+1}`) at the `p` arguments `x`, in
/// homogeneous coordinates (de Boor's algorithm with one argument per level).
fn blossom(knots: &[f64], h: &[[f64; 3]], p: usize, i: usize, x: &[f64]) -> Option<[f64; 3]> {
    let mut d: Vec<[f64; 3]> = h.get(i.checked_sub(p)?..=i)?.to_vec();
    for r in 1..=p {
        let xr = *x.get(r - 1)?;
        for j in (r..=p).rev() {
            let idx = i - p + j;
            let (lo, hi) = (*knots.get(idx)?, *knots.get(idx + p + 1 - r)?);
            let a = if hi > lo { (xr - lo) / (hi - lo) } else { 0.0 };
            let (prev, cur) = (*d.get(j - 1)?, *d.get(j)?);
            *d.get_mut(j)? =
                [(1.0 - a) * prev[0] + a * cur[0], (1.0 - a) * prev[1] + a * cur[1], (1.0 - a) * prev[2] + a * cur[2]];
        }
    }
    d.get(p).copied()
}

/// The curve's non-empty knot spans as rational Bézier segments, in the order of travel,
/// with the knot interval each one covers. Empty when the data does not pass [`check`].
pub fn spans(b: &BSplineData<'_>) -> Vec<(RationalBezier, [f64; 2])> {
    if check(b.degree, b.poles.len(), b.knots, b.weights, b.periodic).is_err() {
        return Vec::new();
    }
    let p = b.degree;
    let (knots, poles, weights) = unwrap(b);
    let n = poles.len();
    let h: Vec<[f64; 3]> = poles.iter().zip(&weights).map(|(q, w)| [q[0] * w, q[1] * w, *w]).collect();
    let mut out = Vec::new();
    for i in p..n {
        let (Some(&a), Some(&c)) = (knots.get(i), knots.get(i + 1)) else { continue };
        if c.partial_cmp(&a) != Some(core::cmp::Ordering::Greater) {
            continue;
        }
        let mut hs = Vec::with_capacity(p + 1);
        for m in 0..=p {
            let args: Vec<f64> = (0..p).map(|k| if k < p - m { a } else { c }).collect();
            match blossom(&knots, &h, p, i, &args) {
                Some(q) => hs.push(q),
                None => return Vec::new(),
            }
        }
        let ps: Vec<V2> = hs.iter().map(|q| [q[0] / q[2], q[1] / q[2]]).collect();
        let ws: Vec<f64> = hs.iter().map(|q| q[2]).collect();
        match RationalBezier::new(&ps, &ws) {
            Some(z) => out.push((z, [a, c])),
            None => return Vec::new(),
        }
    }
    out
}

/// The point at knot parameter `u` (clamped to the domain; a periodic curve wraps).
pub fn point_at(b: &BSplineData<'_>, u: f64) -> Option<V2> {
    let sp = spans(b);
    let (lo, hi) = (sp.first()?.1[0], sp.last()?.1[1]);
    let mut u = u;
    if b.periodic {
        let period = hi - lo;
        u = lo + (u - lo).rem_euclid(period);
    }
    let u = u.clamp(lo, hi);
    let (z, [a, c]) = sp.iter().find(|(_, [a, c])| u >= *a && u <= *c)?;
    Some(z.eval((u - a) / (c - a)))
}

/// Points along the curve, `per_span` per span plus the end, for drawing.
pub fn polyline(b: &BSplineData<'_>, per_span: usize) -> Vec<V2> {
    let sp = spans(b);
    let per = per_span.max(1);
    let mut out = Vec::with_capacity(sp.len() * per + 1);
    for (z, _) in &sp {
        for k in 0..per {
            out.push(z.eval(k as f64 / per as f64));
        }
    }
    if let Some((z, _)) = sp.last() {
        out.push(z.last());
    }
    out
}

/// Length of a segment's control polygon (zero for a segment that is a single point).
pub(crate) fn hull_length(z: &RationalBezier) -> f64 {
    z.control_points().windows(2).map(|w| if let [a, b] = w { dist(*a, *b) } else { 0.0 }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2::norm;

    const H: f64 = core::f64::consts::FRAC_1_SQRT_2;

    /// The full circle of radius `r` as a clamped rational quadratic of 9 poles.
    fn circle(r: f64) -> (Vec<f64>, Vec<V2>, Vec<f64>) {
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
        let poles = vec![[r, 0.0], [r, r], [0.0, r], [-r, r], [-r, 0.0], [-r, -r], [0.0, -r], [r, -r], [r, 0.0]];
        let weights = vec![1.0, H, 1.0, H, 1.0, H, 1.0, H, 1.0];
        (knots, poles, weights)
    }

    #[test]
    fn a_rational_circle_is_exact() {
        let (k, p, w) = circle(5.0);
        let b = BSplineData { degree: 2, knots: &k, poles: &p, weights: &w, periodic: false };
        let sp = spans(&b);
        assert_eq!(sp.len(), 4);
        let area: f64 = sp.iter().map(|(z, _)| z.area_term()).sum();
        assert!((area - core::f64::consts::PI * 25.0).abs() < 1e-12, "{area}");
        for q in polyline(&b, 7) {
            assert!((norm(q) - 5.0).abs() < 1e-12);
        }
        assert!(dist(point_at(&b, 1.0).unwrap(), [0.0, 5.0]) < 1e-12);
    }

    #[test]
    fn a_clamped_cubic_is_its_bezier() {
        let p = [[0.0, 0.0], [1.0, 3.0], [4.0, 3.0], [6.0, 0.0]];
        let k = [0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0];
        let w = [1.0; 4];
        let sp = spans(&BSplineData { degree: 3, knots: &k, poles: &p, weights: &w, periodic: false });
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].0.as_cubic(), Some(p));
    }

    #[test]
    fn a_uniform_periodic_cubic_matches_the_closed_form() {
        // Uniform periodic cubic over a square: segment between knots j, j+1 is
        // (1/6)[(1-t)^3, 3t^3-6t^2+4, -3t^3+3t^2+3t+1, t^3] . (P_{j-3}, .., P_j) in OCCT's
        // numbering (pole i starts at t_{i-p}): the first span uses poles n-3.. wrapped.
        let p = [[10.0, 0.0], [0.0, 10.0], [-10.0, 0.0], [0.0, -10.0]];
        let k = [0.0, 1.0, 2.0, 3.0, 4.0];
        let w = [1.0; 4];
        let b = BSplineData { degree: 3, knots: &k, poles: &p, weights: &w, periodic: true };
        let sp = spans(&b);
        assert_eq!(sp.len(), 4);
        // Closed and C2: the end of the last span is the start of the first.
        assert!(dist(sp[3].0.last(), sp[0].0.first()) < 1e-12);
        // At t_0 the curve is (P_0 + 4 P_1 + P_2) / 6 with poles P_{0..2} of the first span:
        // the span [t_0, t_1] uses unwrapped poles 0..=3 = P0..P3, so its start is
        // (P0 + 4 P1 + P2) / 6.
        let s = sp[0].0.first();
        let want = [(p[0][0] + 4.0 * p[1][0] + p[2][0]) / 6.0, (p[0][1] + 4.0 * p[1][1] + p[2][1]) / 6.0];
        assert!(dist(s, want) < 1e-12, "{s:?} vs {want:?}");
        // OCCT's periodic Geom2d_BSplineCurve of these poles and knots (multiplicities 1)
        // gives (0, 20/3) at 0 too (checked with the occt-port translation of BSplCLib).
        assert!(dist(s, [0.0, 20.0 / 3.0]) < 1e-12);
        // The enclosed area of this curve: by symmetry its points at the knots lie on the
        // diagonals; the polygon's area (200) bounds it from above.
        let area: f64 = sp.iter().map(|(z, _)| z.area_term()).sum();
        assert!(area > 0.0 && area < 200.0, "{area}");
    }

    #[test]
    fn checks_report_what_is_wrong() {
        let w4 = [1.0; 4];
        let ok = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        assert!(check(3, 4, &ok, &w4, false).is_ok());
        assert!(check(0, 4, &ok, &w4, false).unwrap_err().contains("degree"));
        assert!(check(10, 4, &ok, &w4, false).unwrap_err().contains("degree"));
        assert!(check(3, 3, &ok, &w4[..3], false).unwrap_err().contains("at least 4 poles"));
        assert!(check(3, 4, &ok[..7], &w4, false).unwrap_err().contains("8 knots"));
        assert!(check(3, 4, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.5], &w4, false).unwrap_err().contains("decrease"));
        assert!(check(3, 4, &ok, &[1.0, 0.0, 1.0, 1.0], false).unwrap_err().contains("weights"));
        assert!(check(3, 4, &ok, &[1.0; 3], false).unwrap_err().contains("one weight per pole"));
        assert!(check(3, 4, &[0.0; 8], &w4, false).unwrap_err().contains("empty"));
        assert!(check(3, 4, &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &w4, false).is_err());
        assert!(check(3, 4, &[0.0, 1.0, 2.0, 3.0, 4.0], &w4, true).is_ok());
        assert!(check(3, 4, &[0.0, 0.0, 0.0, 3.0, 4.0], &w4, true).is_err());
        assert!(check(3, 4, &[0.0, 1.0, 2.0, 3.0, 0.0], &w4, true).is_err());
        assert!(check(2, 4, &[0.0, 0.0, 1.0, 2.0, 2.0], &w4, true).unwrap_err().contains("seam"));
        assert!(check(3, 4, &[0.0, f64::NAN, 2.0, 3.0, 4.0], &w4, true).is_err());
    }
}
