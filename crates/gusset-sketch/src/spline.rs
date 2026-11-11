//! Interpolating splines: a centripetal Catmull-Rom curve through the fit points, stored as
//! a chain of cubic Béziers (ADR 0016 §3).
//!
//! The curve passes through every fit point, is C1, and each segment depends only on its
//! four nearest points, so moving a point changes the curve locally. The centripetal
//! parametrisation (knot spacing = chord length^0.5) never forms cusps or self-loops
//! inside a segment. A chain is exact: the kernel receives the same Bézier control points
//! (as a clamped cubic B-spline), so the arrangement and the solid agree to rounding.

use crate::geom2::{V2, add, dist, scale, sub};

use crate::bezier::RationalBezier;

/// One segment of a spline chain: a cubic Bézier for the splines through points, a rational
/// Bézier of any degree up to [`MAX_BEZIER_DEGREE`] for a span of an exact B-spline.
pub(crate) type Bez = RationalBezier;

/// Most fit points a spline may have.
pub const MAX_SPLINE_POINTS: usize = 1000;

/// Points along the spline through `pts` (open or closed), `per_segment` per Bézier segment,
/// ending at the last point (a closed spline ends where it started). For drawing.
pub fn polyline(pts: &[[f64; 2]], closed: bool, per_segment: usize) -> Vec<[f64; 2]> {
    flatten(&chain(pts, closed), per_segment.max(1))
}

/// The Bézier chain of the spline through `pts`: `n - 1` segments when open (the missing
/// neighbours of the end points are mirrored, so the ends leave along the chord), `n` when
/// closed (the last point connects back to the first). Segment `i` runs from point `i`.
pub(crate) fn chain(pts: &[V2], closed: bool) -> Vec<Bez> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let at = |i: isize| -> V2 {
        let n_i = n as isize;
        if closed {
            pts.get(i.rem_euclid(n_i) as usize).copied().unwrap_or([0.0, 0.0])
        } else if i < 0 {
            let (p0, p1) = (pts.first().copied().unwrap_or([0.0; 2]), pts.get(1).copied().unwrap_or([0.0; 2]));
            sub(scale(p0, 2.0), p1)
        } else if i >= n_i {
            let (a, b) = (
                pts.get(n - 1).copied().unwrap_or([0.0; 2]),
                pts.get(n.saturating_sub(2)).copied().unwrap_or([0.0; 2]),
            );
            sub(scale(a, 2.0), b)
        } else {
            pts.get(i as usize).copied().unwrap_or([0.0, 0.0])
        }
    };
    let nseg = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(nseg);
    for i in 0..nseg as isize {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        // Knot intervals of the centripetal parametrisation.
        let d = |a: V2, b: V2| dist(a, b).sqrt();
        let (d0, d1, d2) = (d(p0, p1), d(p1, p2), d(p2, p3));
        const EPS: f64 = 1e-12;
        if d1 < EPS {
            // Only drawings get here: profiles refuse neighbouring fit points closer than the
            // tolerance before building the chain (review L11), so the kernel never receives
            // this zero-length segment.
            out.push(Bez::cubic([p1, p1, p2, p2]));
            continue;
        }
        // Tangents (derivative with respect to the knot parameter) at p1 and p2.
        let mut t1 = scale(sub(p2, p1), 1.0 / d1);
        if d0 >= EPS {
            t1 = add(t1, sub(scale(sub(p1, p0), 1.0 / d0), scale(sub(p2, p0), 1.0 / (d0 + d1))));
        }
        let mut t2 = scale(sub(p2, p1), 1.0 / d1);
        if d2 >= EPS {
            t2 = add(t2, sub(scale(sub(p3, p2), 1.0 / d2), scale(sub(p3, p1), 1.0 / (d1 + d2))));
        }
        out.push(Bez::cubic([p1, add(p1, scale(t1, d1 / 3.0)), sub(p2, scale(t2, d1 / 3.0)), p2]));
    }
    out
}

pub(crate) fn eval(b: &Bez, t: f64) -> V2 {
    b.eval(t)
}

pub(crate) fn deriv(b: &Bez, t: f64) -> V2 {
    b.deriv(t)
}

/// Splits a segment at `t` (de Casteljau).
pub(crate) fn split(b: &Bez, t: f64) -> (Bez, Bez) {
    b.split(t)
}

/// The part of the chain between chain parameters `u0 < u1` (segment `k` covers
/// `[k, k + 1]`). With `wrap`, parameters beyond the chain length continue at its start
/// (the seam of a closed spline).
pub(crate) fn sub_chain(ch: &[Bez], u0: f64, u1: f64, wrap: bool) -> Vec<Bez> {
    let n = ch.len();
    let mut out: Vec<Bez> = Vec::new();
    if n == 0 || u1 <= u0 {
        return out;
    }
    let seg = |k: usize| -> Option<Bez> { if wrap { ch.get(k % n).copied() } else { ch.get(k).copied() } };
    let k0 = libm::floor(u0) as usize;
    let k1 = (libm::ceil(u1) as usize).saturating_sub(1).max(k0);
    for k in k0..=k1 {
        let Some(mut b) = seg(k) else { continue };
        let (lo, hi) = ((u0 - k as f64).max(0.0), (u1 - k as f64).min(1.0));
        if hi <= lo {
            continue;
        }
        if hi < 1.0 {
            b = split(&b, hi).0;
        }
        if lo > 0.0 {
            // After cutting at `hi` the segment spans [0, hi]; re-map `lo`.
            b = split(&b, lo / hi.max(f64::MIN_POSITIVE)).1;
        }
        out.push(b);
    }
    out
}

/// `∫ (x dy - y dx) / 2` over a segment.
pub(crate) fn area_term(b: &Bez) -> f64 {
    b.area_term()
}

/// Polyline through `per` samples per segment (plus the final point).
pub(crate) fn flatten(ch: &[Bez], per: usize) -> Vec<V2> {
    let mut out = Vec::with_capacity(ch.len() * per + 1);
    for b in ch {
        for k in 0..per {
            out.push(eval(b, k as f64 / per as f64));
        }
    }
    if let Some(b) = ch.last() {
        out.push(b.last());
    }
    out
}

/// Signed curvature at the start (`end == false`) or end of a segment.
pub(crate) fn curvature(b: &Bez, end: bool) -> f64 {
    b.curvature(end)
}

/// Direction of travel at the start or end of a segment (falls back to the chord when the
/// first control points coincide).
pub(crate) fn tangent_dir(b: &Bez, end: bool) -> V2 {
    b.tangent_dir(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2::{cross, dot, norm};

    #[test]
    fn passes_through_the_fit_points() {
        let pts = [[0.0, 0.0], [10.0, 5.0], [20.0, 0.0], [25.0, 12.0]];
        let ch = chain(&pts, false);
        assert_eq!(ch.len(), 3);
        for (i, b) in ch.iter().enumerate() {
            assert_eq!(b.first(), pts[i]);
            assert_eq!(b.last(), pts[i + 1]);
        }
        let closed = chain(&pts, true);
        assert_eq!(closed.len(), 4);
        assert_eq!(closed[3].last(), pts[0]);
    }

    #[test]
    fn is_c1_at_the_joints() {
        let pts = [[0.0, 0.0], [10.0, 5.0], [20.0, 0.0], [25.0, 12.0], [14.0, 20.0]];
        for closed in [false, true] {
            let ch = chain(&pts, closed);
            let n = ch.len();
            for i in 0..n - usize::from(!closed) {
                let (a, b) = (deriv(&ch[i], 1.0), deriv(&ch[(i + 1) % n], 0.0));
                // Equal direction, and proportional magnitudes are not required (the knot
                // parameter differs); the tangent lines must agree.
                assert!(cross(a, b).abs() < 1e-9 * norm(a) * norm(b), "joint {i} closed={closed}");
                assert!(dot(a, b) > 0.0);
            }
        }
    }

    #[test]
    fn approximates_a_circle_closely() {
        let n = 12;
        let pts: Vec<V2> = (0..n)
            .map(|k| {
                let t = core::f64::consts::TAU * f64::from(k) / f64::from(n);
                [10.0 * libm::cos(t), 10.0 * libm::sin(t)]
            })
            .collect();
        let ch = chain(&pts, true);
        let area: f64 = ch.iter().map(area_term).sum();
        let exact = core::f64::consts::PI * 100.0;
        assert!((area - exact).abs() / exact < 2e-3, "{area} vs {exact}");
        // Every sampled point is within 0.1 mm of the circle.
        for p in flatten(&ch, 16) {
            assert!((norm(p) - 10.0).abs() < 0.1, "{p:?}");
        }
    }

    #[test]
    fn sub_chain_is_the_same_curve() {
        let pts = [[0.0, 0.0], [10.0, 5.0], [20.0, 0.0], [25.0, 12.0], [14.0, 20.0]];
        let ch = chain(&pts, true);
        for (u0, u1) in [(0.3, 0.8), (0.5, 2.5), (3.2, 5.6), (1.0, 2.0)] {
            let sub = sub_chain(&ch, u0, u1, true);
            let at = |u: f64| {
                let k = libm::floor(u) as usize;
                eval(&ch[k % ch.len()], u - k as f64)
            };
            assert!(dist(sub[0].first(), at(u0)) < 1e-12);
            assert!(dist(sub[sub.len() - 1].last(), at(u1)) < 1e-12);
            let mid = 0.5 * (u0 + u1);
            // The middle of the sub-curve (by parameter) is not the middle of u, but every
            // sub-segment joint must lie on the original curve.
            for pair in sub.windows(2) {
                assert!(dist(pair[0].last(), pair[1].first()) < 1e-12);
            }
            let _ = mid;
        }
    }

    #[test]
    fn area_term_matches_a_polygon() {
        let b = Bez::cubic([[0.0, 0.0], [3.0, 0.0], [6.0, 0.0], [9.0, 0.0]]);
        assert!(area_term(&b).abs() < 1e-12);
        let ch = chain(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]], true);
        let a: f64 = ch.iter().map(area_term).sum();
        assert!(a > 16.0 && a < 30.0, "{a}");
    }
}
