//! Circular detail views (ADR 0032 phase 4, ISO 128-30 §8): the curves of a parent view
//! inside a circle, enlarged. Curves are clipped to the circle; arcs and polylines are
//! flattened first.

use gusset_kernel::{HlrCurve, ProjCurve};
use std::f64::consts::TAU;

type P = [f64; 2];

/// The parameters (0..=1) where segment `a`-`b` crosses the circle, sorted.
fn crossings(a: P, b: P, c: P, r: f64) -> Vec<f64> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let f = [a[0] - c[0], a[1] - c[1]];
    let qa = d[0] * d[0] + d[1] * d[1];
    if qa < 1e-24 {
        return Vec::new();
    }
    let qb = 2.0 * (f[0] * d[0] + f[1] * d[1]);
    let qc = f[0] * f[0] + f[1] * f[1] - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc <= 0.0 {
        return Vec::new();
    }
    let s = libm::sqrt(disc);
    let mut t: Vec<f64> =
        [(-qb - s) / (2.0 * qa), (-qb + s) / (2.0 * qa)].into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect();
    t.sort_by(f64::total_cmp);
    t
}

fn inside(p: P, c: P, r: f64) -> bool {
    let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
    dx * dx + dy * dy <= r * r
}

/// The pieces of polyline `pts` inside the circle.
fn clip_polyline(pts: &[P], c: P, r: f64) -> Vec<Vec<P>> {
    let mut out: Vec<Vec<P>> = Vec::new();
    let mut cur: Vec<P> = Vec::new();
    let lerp = |a: P, b: P, t: f64| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
    for w in pts.windows(2) {
        let [a, b] = w else { continue };
        // Cut the segment at its crossings; keep the parts whose middle is inside.
        let mut ts = vec![0.0];
        ts.extend(crossings(*a, *b, c, r));
        ts.push(1.0);
        for tt in ts.windows(2) {
            let [t0, t1] = tt else { continue };
            let (p0, p1) = (lerp(*a, *b, *t0), lerp(*a, *b, *t1));
            if inside(lerp(*a, *b, (t0 + t1) / 2.0), c, r) {
                if cur.last().is_none_or(|l| (l[0] - p0[0]).abs() > 1e-12 || (l[1] - p0[1]).abs() > 1e-12) {
                    if cur.len() >= 2 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(p0);
                }
                cur.push(p1);
            } else if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// The part of segment `a`-`b` inside the circle (a chord of a convex region: one piece).
pub fn clip_segment(a: P, b: P, c: P, r: f64) -> Option<(P, P)> {
    match clip_polyline(&[a, b], c, r).as_slice() {
        [piece] => match piece.as_slice() {
            [p, .., q] => Some((*p, *q)),
            _ => None,
        },
        _ => None,
    }
}

/// `curves` clipped to the circle of radius `r` around `c` (all in view coordinates).
/// `tol` is the chordal tolerance for flattening arcs.
pub fn clip(curves: &[HlrCurve], c: P, r: f64, tol: f64) -> Vec<HlrCurve> {
    let mut out = Vec::new();
    for h in curves {
        let pts: Vec<P> = match &h.curve {
            ProjCurve::Line(a, b) => vec![*a, *b],
            ProjCurve::Polyline(p) => p.clone(),
            ProjCurve::Arc { center, radius, start, end, ccw } => {
                // Whole arc inside: kept as is.
                let (dx, dy) = (center[0] - c[0], center[1] - c[1]);
                if libm::sqrt(dx * dx + dy * dy) + radius <= r {
                    out.push(h.clone());
                    continue;
                }
                let (s, e0) = if *ccw { (*start, *end) } else { (*end, *start) };
                let d = (e0 - s).rem_euclid(TAU);
                let e = s + if d <= 0.0 || !d.is_finite() { TAU } else { d };
                if !(s.is_finite() && radius.is_finite()) {
                    continue;
                }
                let step = 2.0 * libm::acos((1.0 - tol / radius.max(tol)).clamp(-1.0, 1.0));
                let n = (((e - s) / step.max(1e-3)).ceil() as usize).clamp(8, 4096);
                (0..=n)
                    .map(|i| {
                        let a = s + (e - s) * i as f64 / n as f64;
                        [center[0] + radius * libm::cos(a), center[1] + radius * libm::sin(a)]
                    })
                    .collect()
            }
        };
        for piece in clip_polyline(&pts, c, r) {
            let curve = match piece.as_slice() {
                [a, b] => ProjCurve::Line(*a, *b),
                _ => ProjCurve::Polyline(piece),
            };
            out.push(HlrCurve { visible: h.visible, class: h.class, curve, source: h.source });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gusset_kernel::HlrClass;

    fn line(a: P, b: P) -> HlrCurve {
        HlrCurve { visible: true, class: HlrClass::Sharp, curve: ProjCurve::Line(a, b), source: None }
    }

    #[test]
    fn lines_are_cut_at_the_circle() {
        let out = clip(&[line([-10.0, 0.0], [10.0, 0.0]), line([-10.0, 5.0], [10.0, 5.0])], [0.0, 0.0], 3.0, 0.01);
        // The first crosses the circle (a chord of 6), the second misses it.
        assert_eq!(out.len(), 1, "{out:?}");
        let ProjCurve::Line(a, b) = out[0].curve else { panic!("{out:?}") };
        assert!((a[0] + 3.0).abs() < 1e-12 && (b[0] - 3.0).abs() < 1e-12, "{a:?} {b:?}");
        // A segment wholly inside stays whole.
        let out = clip(&[line([-1.0, 0.0], [1.0, 0.0])], [0.0, 0.0], 3.0, 0.01);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn arcs_are_flattened_and_cut() {
        let arc = HlrCurve {
            visible: true,
            class: HlrClass::Sharp,
            curve: ProjCurve::Arc { center: [0.0, 0.0], radius: 5.0, start: 0.0, end: TAU, ccw: true },
            source: None,
        };
        // A circle of radius 5 around the origin, seen through a circle of radius 2 at (5, 0).
        let out = clip(&[arc], [5.0, 0.0], 2.0, 0.001);
        assert!(!out.is_empty());
        for h in &out {
            let ProjCurve::Polyline(p) = &h.curve else { panic!("{h:?}") };
            for q in p {
                assert!(libm::hypot(q[0] - 5.0, q[1]) <= 2.0 + 1e-9, "{q:?}");
                assert!((libm::hypot(q[0], q[1]) - 5.0).abs() < 1e-3, "{q:?}");
            }
        }
    }
}
