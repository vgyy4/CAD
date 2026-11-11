//! Rational Bézier segments of any degree up to [`MAX_BEZIER_DEGREE`]: the pieces spline
//! chains are made of. A spline through points is a chain of polynomial cubics (ADR 0016
//! §3); an exact B-spline is cut at its knots into one rational Bézier per span (ADR 0016
//! amendment "exact B-splines"), which is the same curve, not an approximation.
//!
//! The polynomial cubic keeps its closed formulas (bit for bit what the spline code used
//! before rational segments existed); every other segment is evaluated with de Casteljau in
//! homogeneous coordinates.

use crate::geom2::{V2, add, cross, norm, scale, sub};

/// Highest degree of a Bézier segment, and so of an exact B-spline sketch curve.
pub const MAX_BEZIER_DEGREE: usize = 9;
const N: usize = MAX_BEZIER_DEGREE + 1;

/// A rational Bézier segment: `degree + 1` control points with positive weights (all 1 for
/// a polynomial segment). Entries past `degree` are unused and zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RationalBezier {
    pub degree: usize,
    pub poles: [[f64; 2]; N],
    pub weights: [f64; N],
}

type H = [f64; 3];

fn hlerp(a: H, b: H, t: f64) -> H {
    let s = 1.0 - t;
    [s * a[0] + t * b[0], s * a[1] + t * b[1], s * a[2] + t * b[2]]
}

fn lerp(a: V2, b: V2, t: f64) -> V2 {
    add(scale(a, 1.0 - t), scale(b, t))
}

impl RationalBezier {
    /// A polynomial cubic.
    pub fn cubic(p: [V2; 4]) -> Self {
        let mut poles = [[0.0; 2]; N];
        let mut weights = [0.0; N];
        for (k, q) in p.into_iter().enumerate() {
            if let (Some(d), Some(w)) = (poles.get_mut(k), weights.get_mut(k)) {
                *d = q;
                *w = 1.0;
            }
        }
        RationalBezier { degree: 3, poles, weights }
    }

    /// A segment from control points and weights (`None` for an empty list, a degree above
    /// [`MAX_BEZIER_DEGREE`] or lists of different lengths).
    pub fn new(poles: &[V2], weights: &[f64]) -> Option<Self> {
        if poles.is_empty() || poles.len() > N || poles.len() != weights.len() {
            return None;
        }
        let mut b = RationalBezier { degree: poles.len() - 1, poles: [[0.0; 2]; N], weights: [0.0; N] };
        b.poles.get_mut(..poles.len())?.copy_from_slice(poles);
        b.weights.get_mut(..weights.len())?.copy_from_slice(weights);
        Some(b)
    }

    /// The `degree + 1` control points.
    pub fn control_points(&self) -> &[V2] {
        self.poles.get(..=self.degree).unwrap_or(&[])
    }

    /// The `degree + 1` weights.
    pub fn control_weights(&self) -> &[f64] {
        self.weights.get(..=self.degree).unwrap_or(&[])
    }

    /// The control points of a polynomial cubic.
    pub fn as_cubic(&self) -> Option<[V2; 4]> {
        if self.degree != 3 || self.control_weights().iter().any(|w| w.total_cmp(&1.0).is_ne()) {
            return None;
        }
        match self.control_points() {
            &[a, b, c, d] => Some([a, b, c, d]),
            _ => None,
        }
    }

    pub fn first(&self) -> V2 {
        self.poles.first().copied().unwrap_or([0.0; 2])
    }

    pub fn last(&self) -> V2 {
        self.poles.get(self.degree).copied().unwrap_or([0.0; 2])
    }

    /// The same curve travelled the other way.
    pub fn reversed(&self) -> Self {
        let mut r = *self;
        let n = self.degree + 1;
        if let (Some(p), Some(w)) = (r.poles.get_mut(..n), r.weights.get_mut(..n)) {
            p.reverse();
            w.reverse();
        }
        r
    }

    fn homogeneous(&self) -> ([H; N], usize) {
        let mut h = [[0.0; 3]; N];
        for (d, (p, w)) in h.iter_mut().zip(self.control_points().iter().zip(self.control_weights())) {
            *d = [p[0] * w, p[1] * w, *w];
        }
        (h, self.degree + 1)
    }

    fn from_homogeneous(h: &[H]) -> Self {
        let mut b = RationalBezier { degree: h.len().saturating_sub(1), poles: [[0.0; 2]; N], weights: [0.0; N] };
        for ((p, w), q) in b.poles.iter_mut().zip(b.weights.iter_mut()).zip(h) {
            *w = q[2];
            *p = [q[0] / q[2], q[1] / q[2]];
        }
        b
    }

    /// The homogeneous point and its first and second derivatives at `t`.
    fn hderivs(&self, t: f64) -> [H; 3] {
        let (mut h, n) = self.homogeneous();
        let d = self.degree;
        // De Casteljau down to one point; the levels of three and two points give the
        // second and first derivatives.
        let mut out = [[0.0; 3]; 3];
        for level in 0..=d {
            let left = n - level;
            if left == 3
                && let (Some(&a), Some(&b), Some(&c)) = (h.first(), h.get(1), h.get(2))
            {
                let df = (d * (d - 1)) as f64;
                out[2] =
                    [df * (a[0] - 2.0 * b[0] + c[0]), df * (a[1] - 2.0 * b[1] + c[1]), df * (a[2] - 2.0 * b[2] + c[2])];
            }
            if left == 2
                && let (Some(&a), Some(&b)) = (h.first(), h.get(1))
            {
                let df = d as f64;
                out[1] = [df * (b[0] - a[0]), df * (b[1] - a[1]), df * (b[2] - a[2])];
            }
            for k in 0..left.saturating_sub(1) {
                if let (Some(&a), Some(&b)) = (h.get(k), h.get(k + 1))
                    && let Some(x) = h.get_mut(k)
                {
                    *x = hlerp(a, b, t);
                }
            }
        }
        out[0] = h.first().copied().unwrap_or([0.0, 0.0, 1.0]);
        out
    }

    pub fn eval(&self, t: f64) -> V2 {
        if let Some(b) = self.as_cubic() {
            let s = 1.0 - t;
            let (w0, w1, w2, w3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
            return [
                w0 * b[0][0] + w1 * b[1][0] + w2 * b[2][0] + w3 * b[3][0],
                w0 * b[0][1] + w1 * b[1][1] + w2 * b[2][1] + w3 * b[3][1],
            ];
        }
        let [p, _, _] = self.hderivs(t);
        [p[0] / p[2], p[1] / p[2]]
    }

    /// First derivative with respect to `t`.
    pub fn deriv(&self, t: f64) -> V2 {
        if let Some([p0, p1, p2, p3]) = self.as_cubic() {
            let s = 1.0 - t;
            let (w0, w1, w2) = (3.0 * s * s, 6.0 * s * t, 3.0 * t * t);
            let (d0, d1, d2) = (sub(p1, p0), sub(p2, p1), sub(p3, p2));
            return [w0 * d0[0] + w1 * d1[0] + w2 * d2[0], w0 * d0[1] + w1 * d1[1] + w2 * d2[1]];
        }
        let [p, d, _] = self.hderivs(t);
        let c = [p[0] / p[2], p[1] / p[2]];
        [(d[0] - d[2] * c[0]) / p[2], (d[1] - d[2] * c[1]) / p[2]]
    }

    /// Second derivative with respect to `t`.
    pub fn deriv2(&self, t: f64) -> V2 {
        if let Some([p0, p1, p2, p3]) = self.as_cubic() {
            let s = 1.0 - t;
            let a = add(sub(p2, scale(p1, 2.0)), p0);
            let c = add(sub(p3, scale(p2, 2.0)), p1);
            return [6.0 * (s * a[0] + t * c[0]), 6.0 * (s * a[1] + t * c[1])];
        }
        let [p, d, dd] = self.hderivs(t);
        let w = p[2];
        let c = [p[0] / w, p[1] / w];
        let c1 = [(d[0] - d[2] * c[0]) / w, (d[1] - d[2] * c[1]) / w];
        [(dd[0] - 2.0 * d[2] * c1[0] - dd[2] * c[0]) / w, (dd[1] - 2.0 * d[2] * c1[1] - dd[2] * c[1]) / w]
    }

    /// Splits the segment at `t` (de Casteljau).
    pub fn split(&self, t: f64) -> (Self, Self) {
        if let Some(b) = self.as_cubic() {
            let p01 = lerp(b[0], b[1], t);
            let p12 = lerp(b[1], b[2], t);
            let p23 = lerp(b[2], b[3], t);
            let p012 = lerp(p01, p12, t);
            let p123 = lerp(p12, p23, t);
            let m = lerp(p012, p123, t);
            return (Self::cubic([b[0], p01, p012, m]), Self::cubic([m, p123, p23, b[3]]));
        }
        let (mut h, n) = self.homogeneous();
        let mut left = [[0.0; 3]; N];
        let mut right = [[0.0; 3]; N];
        for level in 0..n {
            if let (Some(l), Some(&f)) = (left.get_mut(level), h.first()) {
                *l = f;
            }
            if let (Some(r), Some(&b)) = (right.get_mut(n - 1 - level), h.get(n - 1 - level)) {
                *r = b;
            }
            for k in 0..(n - 1 - level) {
                if let (Some(&a), Some(&b)) = (h.get(k), h.get(k + 1))
                    && let Some(x) = h.get_mut(k)
                {
                    *x = hlerp(a, b, t);
                }
            }
        }
        (Self::from_homogeneous(left.get(..n).unwrap_or(&[])), Self::from_homogeneous(right.get(..n).unwrap_or(&[])))
    }

    /// `∫ (x dy - y dx) / 2` over the segment. Exact for a polynomial segment (the integrand
    /// has degree `2n - 1`, Gauss-Legendre with `n` nodes is exact to degree `2n - 1`); a
    /// rational one is integrated with 16 nodes, accurate to rounding for the smooth
    /// integrands of segments with sane weights.
    pub fn area_term(&self) -> f64 {
        if self.as_cubic().is_some() {
            const W: [f64; 3] = [5.0 / 18.0, 8.0 / 18.0, 5.0 / 18.0];
            let h = 0.5 * (0.6_f64).sqrt();
            let nodes = [0.5 - h, 0.5, 0.5 + h];
            let mut sum = 0.0;
            for (w, t) in W.iter().zip(nodes) {
                sum += w * cross(self.eval(t), self.deriv(t));
            }
            return 0.5 * sum;
        }
        let rational = self.control_weights().iter().any(|w| w.total_cmp(&1.0).is_ne());
        let n = if rational { 16 } else { self.degree.max(1) };
        let mut sum = 0.0;
        for (x, w) in gauss_legendre(n) {
            let t = 0.5 * (x + 1.0);
            sum += 0.5 * w * cross(self.eval(t), self.deriv(t));
        }
        0.5 * sum
    }

    /// Signed curvature at the start (`end == false`) or end.
    pub fn curvature(&self, end: bool) -> f64 {
        let t = if end { 1.0 } else { 0.0 };
        let (d1, d2) = (self.deriv(t), self.deriv2(t));
        let n = norm(d1);
        if n < 1e-12 { 0.0 } else { cross(d1, d2) / (n * n * n) }
    }

    /// Direction of travel at the start or end (falls back to the chord when the first
    /// control points coincide).
    pub fn tangent_dir(&self, end: bool) -> V2 {
        let d = self.degree;
        let pole = |k: usize| self.poles.get(k).copied().unwrap_or([0.0; 2]);
        let (a, c) = if end { (pole(d.saturating_sub(1)), pole(d)) } else { (pole(0), pole(1)) };
        let mut v = sub(c, a);
        if norm(v) < 1e-12 {
            v = sub(self.last(), self.first());
        }
        v
    }
}

/// Nodes and weights of the `n`-point Gauss-Legendre rule on `[-1, 1]` (Newton on the
/// Legendre polynomial; accurate to rounding).
pub(crate) fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(n);
    let nf = n as f64;
    for i in 0..n {
        let mut x = libm::cos(core::f64::consts::PI * (i as f64 + 0.75) / (nf + 0.5));
        let mut dp = 1.0;
        for _ in 0..100 {
            // P_n(x) and P_n'(x) by the three-term recurrence.
            let (mut p0, mut p1) = (1.0, x);
            for k in 2..=n {
                let kf = k as f64;
                let p2 = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
                p0 = p1;
                p1 = p2;
            }
            let pn = if n == 0 { 1.0 } else { p1 };
            let pm = if n == 0 { 0.0 } else { p0 };
            dp = nf * (x * pn - pm) / (x * x - 1.0);
            let dx = pn / dp;
            x -= dx;
            if dx.abs() < 1e-16 {
                break;
            }
        }
        out.push((x, 2.0 / ((1.0 - x * x) * dp * dp)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quarter_circle(r: f64) -> RationalBezier {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        RationalBezier::new(&[[r, 0.0], [r, r], [0.0, r]], &[1.0, h, 1.0]).unwrap()
    }

    #[test]
    fn gauss_legendre_integrates_polynomials() {
        for n in [1, 2, 3, 5, 9, 16] {
            let g = gauss_legendre(n);
            let s: f64 = g.iter().map(|(_, w)| w).sum();
            assert!((s - 2.0).abs() < 1e-14, "{n}");
            // x^(2n-2) integrates to 2 / (2n - 1).
            let k = 2 * n - 2;
            let i: f64 = g.iter().map(|(x, w)| w * x.powi(k as i32)).sum();
            assert!((i - 2.0 / (k as f64 + 1.0)).abs() < 1e-13, "{n}");
        }
    }

    #[test]
    fn rational_quadratic_is_an_exact_quarter_circle() {
        let b = quarter_circle(10.0);
        for k in 0..=20 {
            let t = f64::from(k) / 20.0;
            assert!((norm(b.eval(t)) - 10.0).abs() < 1e-12, "{t}");
            // The derivative is tangent to the circle.
            let (p, d) = (b.eval(t), b.deriv(t));
            assert!((p[0] * d[0] + p[1] * d[1]).abs() < 1e-9 * norm(d) * 10.0);
            // Curvature 1/r anywhere along it (checked at the ends).
        }
        assert!((b.curvature(false) - 0.1).abs() < 1e-12);
        assert!((b.curvature(true) - 0.1).abs() < 1e-12);
        let exact = core::f64::consts::PI * 100.0 / 4.0;
        assert!((b.area_term() - exact).abs() < 1e-12, "{}", b.area_term());
        let (l, r) = b.split(0.3);
        assert!((l.area_term() + r.area_term() - exact).abs() < 1e-12);
        assert!((norm(l.eval(0.5)) - 10.0).abs() < 1e-12 && (norm(r.eval(0.7)) - 10.0).abs() < 1e-12);
        assert_eq!(l.last(), r.first());
        let rev = b.reversed();
        assert!((rev.area_term() + exact).abs() < 1e-12);
    }

    #[test]
    fn general_path_agrees_with_the_cubic_formulas() {
        let p = [[0.0, 0.0], [3.0, 7.0], [9.0, -2.0], [12.0, 4.0]];
        let c = RationalBezier::cubic(p);
        // The same cubic with all weights 2 takes the general path.
        let mut d = RationalBezier::new(&p, &[2.0, 2.0, 2.0, 2.0]).unwrap();
        assert!(d.as_cubic().is_none());
        for k in 0..=10 {
            let t = f64::from(k) / 10.0;
            let (a, b) = (c.eval(t), d.eval(t));
            assert!((a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12);
            let (a, b) = (c.deriv(t), d.deriv(t));
            assert!((a[0] - b[0]).abs() < 1e-11 && (a[1] - b[1]).abs() < 1e-11);
            let (a, b) = (c.deriv2(t), d.deriv2(t));
            assert!((a[0] - b[0]).abs() < 1e-10 && (a[1] - b[1]).abs() < 1e-10);
        }
        assert!((c.area_term() - d.area_term()).abs() < 1e-12);
        d.weights[0] = 2.0;
        let (l, r) = d.split(0.4);
        assert!((l.last()[0] - c.eval(0.4)[0]).abs() < 1e-12 && l.last() == r.first());
    }
}
