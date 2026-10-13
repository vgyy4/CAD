//! Generic curve access for the arrangement: evaluation, parameter lookup and numeric
//! intersection (used when a spline is involved; lines, arcs and circles meet each other
//! analytically in [`crate::arrange`]).
//!
//! Every curve has a parameter `u` over a domain: the fraction `0..1` for a line, the
//! counter-clockwise angle from the start for an arc (from +x for a circle), and the
//! chain parameter `0..n` (segment `k` covers `[k, k + 1]`) for a spline.

use crate::geom2::{V2, add, cross, dist, dot, norm, scale, sub};
use crate::profile::{Cv, Shape};
use crate::spline;

const TAU: f64 = core::f64::consts::TAU;

/// Most elementary steps (cell-pair tests, segment-pair tests and Newton iterations; about
/// 0.2 µs each in a release build) one arrangement may spend intersecting curves with
/// splines (security review V2). Splines that run close together along long stretches cost
/// work in proportion to their length for every pair of them: twenty 1,000-point splines
/// 0.0001 mm apart needed 193 million steps (35 s). The densest sketches in the tests (six
/// crossing 60-point splines, six parallel 1,000-point ones 1 mm apart) need at most 70,000.
pub(crate) const MAX_INTERSECT_WORK: u64 = 5_000_000;

/// The intersection work limit was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TooMuchWork;

/// Work spent so far against [`MAX_INTERSECT_WORK`].
#[derive(Debug, Default)]
pub(crate) struct Work(u64);

impl Work {
    pub(crate) fn charge(&mut self, n: u64) -> Result<(), TooMuchWork> {
        self.0 = self.0.saturating_add(n);
        if self.0 > MAX_INTERSECT_WORK { Err(TooMuchWork) } else { Ok(()) }
    }
}

/// Parameter range of a curve.
pub(crate) fn domain(cv: &Cv) -> (f64, f64) {
    match &cv.shape {
        Shape::Line => (0.0, 1.0),
        Shape::Arc { span, .. } => (0.0, *span),
        Shape::Circle { .. } => (0.0, TAU),
        Shape::Spline { chain, .. } => (0.0, chain.len() as f64),
    }
}

/// Position and derivative (with respect to `u`) at parameter `u`.
pub(crate) fn eval(cv: &Cv, u: f64) -> (V2, V2) {
    match &cv.shape {
        Shape::Line => {
            let d = sub(cv.p[1], cv.p[0]);
            (add(cv.p[0], scale(d, u)), d)
        }
        Shape::Arc { c, r, a0, .. } => {
            let a = a0 + u;
            ([c[0] + r * libm::cos(a), c[1] + r * libm::sin(a)], [-r * libm::sin(a), r * libm::cos(a)])
        }
        Shape::Circle { c, r } => {
            ([c[0] + r * libm::cos(u), c[1] + r * libm::sin(u)], [-r * libm::sin(u), r * libm::cos(u)])
        }
        Shape::Spline { chain, .. } => {
            let n = chain.len();
            let k = (libm::floor(u).max(0.0) as usize).min(n.saturating_sub(1));
            match chain.get(k) {
                Some(b) => (spline::eval(b, u - k as f64), spline::deriv(b, u - k as f64)),
                None => ([0.0, 0.0], [1.0, 0.0]),
            }
        }
    }
}

/// A stretch of a curve flattened to a polyline: `(u, point)` samples and their box.
pub(crate) struct Cell {
    pub(crate) pts: Vec<(f64, V2)>,
    pub(crate) bbox: [f64; 4],
}

fn cell_of(cv: &Cv, u0: f64, u1: f64, samples: usize) -> Cell {
    let mut pts = Vec::with_capacity(samples + 1);
    let mut bbox = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for k in 0..=samples {
        let u = u0 + (u1 - u0) * k as f64 / samples as f64;
        let p = eval(cv, u).0;
        bbox = [bbox[0].min(p[0]), bbox[1].min(p[1]), bbox[2].max(p[0]), bbox[3].max(p[1])];
        pts.push((u, p));
    }
    Cell { pts, bbox }
}

/// The curve as a few polyline cells (one per spline segment, per quarter turn of an arc).
pub(crate) fn cells(cv: &Cv) -> Vec<Cell> {
    let (u0, u1) = domain(cv);
    match &cv.shape {
        Shape::Line => vec![cell_of(cv, u0, u1, 1)],
        Shape::Arc { .. } | Shape::Circle { .. } => {
            let n = (((u1 - u0) / (core::f64::consts::FRAC_PI_4)).ceil() as usize).max(1);
            (0..n)
                .map(|k| {
                    cell_of(cv, u0 + (u1 - u0) * k as f64 / n as f64, u0 + (u1 - u0) * (k + 1) as f64 / n as f64, 8)
                })
                .collect()
        }
        Shape::Spline { chain, .. } => (0..chain.len()).map(|k| cell_of(cv, k as f64, (k + 1) as f64, 24)).collect(),
    }
}

fn overlap(a: &[f64; 4], b: &[f64; 4], eps: f64) -> bool {
    !(a[2] + eps < b[0] || b[2] + eps < a[0] || a[3] + eps < b[1] || b[3] + eps < a[1])
}

/// Distance between two segments (zero when they cross).
fn seg_dist(a0: V2, a1: V2, b0: V2, b1: V2) -> f64 {
    let (d1, d2) = (sub(a1, a0), sub(b1, b0));
    let den = cross(d1, d2);
    if den.abs() > 1e-300 {
        let w = sub(b0, a0);
        let (t, s) = (cross(w, d2) / den, cross(w, d1) / den);
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s) {
            return 0.0;
        }
    }
    let pt_seg = |p: V2, q0: V2, q1: V2| {
        let d = sub(q1, q0);
        let l2 = dot(d, d);
        let t = if l2 > 0.0 { (dot(sub(p, q0), d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        dist(p, add(q0, scale(d, t)))
    };
    pt_seg(a0, b0, b1).min(pt_seg(a1, b0, b1)).min(pt_seg(b0, a0, a1)).min(pt_seg(b1, a0, a1))
}

/// Newton refinement of `a(ua) = b(ub)`. `None` when it does not converge to a point of both
/// curves (or the curves are tangent there). Every iteration is one unit of `work`.
fn refine(a: &Cv, b: &Cv, mut ua: f64, mut ub: f64, tol: f64, work: &mut u64) -> Option<(f64, f64, V2)> {
    let (da, db) = (domain(a), domain(b));
    let slack = |cv: &Cv, u: f64| tol / norm(eval(cv, u).1).max(1e-12);
    for _ in 0..50 {
        *work += 1;
        let ((pa, va), (pb, vb)) = (eval(a, ua), eval(b, ub));
        let f = sub(pa, pb);
        if norm(f) <= tol * 0.01 {
            break;
        }
        let det = cross(va, vb);
        if det.abs() <= 1e-9 * norm(va) * norm(vb) {
            return None;
        }
        // Solve va * du + (-vb) * dv = -f.
        let du = -(cross(f, vb) / det);
        let dv = -(cross(f, va) / det);
        let max_a = (da.1 - da.0) * 0.25;
        let max_b = (db.1 - db.0) * 0.25;
        ua = (ua + du.clamp(-max_a, max_a)).clamp(da.0 - slack(a, ua), da.1 + slack(a, ua));
        ub = (ub + dv.clamp(-max_b, max_b)).clamp(db.0 - slack(b, ub), db.1 + slack(b, ub));
    }
    let (pa, pb) = (eval(a, ua).0, eval(b, ub).0);
    if dist(pa, pb) > tol * 0.1 {
        return None;
    }
    let inside = |cv: &Cv, u: f64, d: (f64, f64)| u >= d.0 - slack(cv, u) && u <= d.1 + slack(cv, u);
    if !inside(a, ua, da) || !inside(b, ub, db) {
        return None;
    }
    Some((ua.clamp(da.0, da.1), ub.clamp(db.0, db.1), scale(add(pa, pb), 0.5)))
}

fn diag(c: &Cell) -> f64 {
    norm([c.bbox[2] - c.bbox[0], c.bbox[3] - c.bbox[1]])
}

/// How close two cells must come to be searched for hits. Flattening error is well below
/// this; Newton decides what is a real hit.
fn cell_eps(x: &Cell, y: &Cell, tol: f64) -> f64 {
    2e-3 * diag(x).max(diag(y)) + tol
}

/// The pairs `(i, j)` of cells of `ca` and `cb` whose boxes overlap (within [`cell_eps`]),
/// in `(i, j)` order. The cells of `cb` are sorted by their left edge, so each cell of `ca`
/// only meets those within reach in x (security review V2: every pair was tested before).
fn overlapping_cells(ca: &[Cell], cb: &[Cell], tol: f64, work: &mut Work) -> Result<Vec<(usize, usize)>, TooMuchWork> {
    let finite = ca.iter().chain(cb).all(|c| c.bbox.iter().all(|v| v.is_finite()));
    let mut pairs = Vec::new();
    if !finite {
        // Unreachable for validated sketches; keep the plain all-pairs search for them.
        for (i, x) in ca.iter().enumerate() {
            work.charge(cb.len() as u64)?;
            for (j, y) in cb.iter().enumerate() {
                if overlap(&x.bbox, &y.bbox, cell_eps(x, y, tol)) {
                    pairs.push((i, j));
                }
            }
        }
        return Ok(pairs);
    }
    // Every pair's eps is at most `pad`; a cell of `cb` that reaches back to `x` starts at
    // most its width (at most `wide`) before `x`'s left edge minus `pad`.
    let pad = 2e-3 * ca.iter().chain(cb).map(diag).fold(0.0, f64::max) + tol;
    let wide = cb.iter().map(|c| c.bbox[2] - c.bbox[0]).fold(0.0, f64::max);
    let mut order: Vec<usize> = (0..cb.len()).collect();
    order.sort_by(|&i, &j| {
        let key = |k: usize| cb.get(k).map_or(0.0, |c| c.bbox[0]);
        key(i).total_cmp(&key(j)).then(i.cmp(&j))
    });
    let left: Vec<f64> = order.iter().map(|&j| cb.get(j).map_or(0.0, |c| c.bbox[0])).collect();
    for (i, x) in ca.iter().enumerate() {
        let lo = left.partition_point(|&m| m < x.bbox[0] - pad - wide);
        let hi = left.partition_point(|&m| m <= x.bbox[2] + pad);
        let near = order.get(lo..hi.max(lo)).unwrap_or(&[]);
        work.charge(near.len() as u64 + 1)?;
        for &j in near {
            if let Some(y) = cb.get(j)
                && overlap(&x.bbox, &y.bbox, cell_eps(x, y, tol))
            {
                pairs.push((i, j));
            }
        }
    }
    pairs.sort_unstable();
    Ok(pairs)
}

/// Intersections of two curves of which at least one is a spline. Tangential touches (no
/// transversal crossing) are not found. Fails when `work` passes [`MAX_INTERSECT_WORK`].
pub(crate) fn numeric_hits(a: &Cv, b: &Cv, tol: f64, work: &mut Work) -> Result<Vec<V2>, TooMuchWork> {
    let (ca, cb) = (cells(a), cells(b));
    let mut out: Vec<V2> = Vec::new();
    for (i, j) in overlapping_cells(&ca, &cb, tol, work)? {
        let (Some(x), Some(y)) = (ca.get(i), cb.get(j)) else { continue };
        let eps = cell_eps(x, y, tol);
        // A cell pair costs a few units for the box tests below; each distance test and
        // Newton iteration one more.
        let mut steps = 4u64;
        // Segments whose boxes are more than `eps` apart are further apart than that.
        let apart = |p0: V2, p1: V2, q0: V2, q1: V2| {
            p0[0].max(p1[0]) + eps < q0[0].min(q1[0])
                || q0[0].max(q1[0]) + eps < p0[0].min(p1[0])
                || p0[1].max(p1[1]) + eps < q0[1].min(q1[1])
                || q0[1].max(q1[1]) + eps < p0[1].min(p1[1])
        };
        for sa in x.pts.windows(2) {
            let &[(ua0, pa0), (ua1, pa1)] = sa else { continue };
            for sb in y.pts.windows(2) {
                let &[(ub0, pb0), (ub1, pb1)] = sb else { continue };
                if apart(pa0, pa1, pb0, pb1) {
                    continue;
                }
                steps += 1;
                if seg_dist(pa0, pa1, pb0, pb1) > eps {
                    continue;
                }
                if let Some((_, _, p)) = refine(a, b, 0.5 * (ua0 + ua1), 0.5 * (ub0 + ub1), tol, &mut steps)
                    && !out.iter().any(|q| dist(*q, p) <= 10.0 * tol)
                {
                    out.push(p);
                }
            }
        }
        work.charge(steps)?;
    }
    Ok(out)
}

/// Parameter of the curve point nearest to `p` (for a spline, a hit position found earlier).
pub(crate) fn nearest_param(cv: &Cv, p: V2) -> f64 {
    let (d0, d1) = domain(cv);
    let mut best = (f64::INFINITY, d0);
    for cell in cells(cv) {
        let pad = 1e-3 * norm([cell.bbox[2] - cell.bbox[0], cell.bbox[3] - cell.bbox[1]]);
        if !overlap(&cell.bbox, &[p[0], p[1], p[0], p[1]], best.0.min(1e300) + pad) {
            continue;
        }
        for w in cell.pts.windows(2) {
            let &[(u0, q0), (u1, q1)] = w else { continue };
            let d = sub(q1, q0);
            let l2 = dot(d, d);
            let t = if l2 > 0.0 { (dot(sub(p, q0), d) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let dd = dist(p, add(q0, scale(d, t)));
            if dd < best.0 {
                best = (dd, u0 + (u1 - u0) * t);
            }
        }
    }
    // Polish with Newton on (c(u) - p) . c'(u) = 0.
    let mut u = best.1;
    for _ in 0..20 {
        let (c, d1v) = eval(cv, u);
        let h = 1e-5 * (d1 - d0);
        let (_, dp) = eval(cv, (u + h).min(d1));
        let (_, dm) = eval(cv, (u - h).max(d0));
        let d2v = scale(sub(dp, dm), 1.0 / (2.0 * h));
        let g = dot(sub(c, p), d1v);
        let gp = dot(d1v, d1v) + dot(sub(c, p), d2v);
        if gp.abs() < 1e-14 {
            break;
        }
        let nu = (u - g / gp).clamp(d0, d1);
        if (nu - u).abs() < 1e-14 {
            u = nu;
            break;
        }
        u = nu;
    }
    if dist(eval(cv, u).0, p) <= best.0 + 1e-12 { u } else { best.1 }
}

/// A place where a spline crosses itself, if any.
pub(crate) fn self_crossing(cv: &Cv) -> Option<V2> {
    let Shape::Spline { closed, .. } = &cv.shape else { return None };
    let cs = cells(cv);
    let n = cs.len();
    for i in 0..n {
        for j in (i + 2)..n {
            if *closed && i == 0 && j == n - 1 {
                continue;
            }
            let (Some(x), Some(y)) = (cs.get(i), cs.get(j)) else { continue };
            if !overlap(&x.bbox, &y.bbox, 0.0) {
                continue;
            }
            for sa in x.pts.windows(2) {
                for sb in y.pts.windows(2) {
                    let &[(_, a0), (_, a1)] = sa else { continue };
                    let &[(_, b0), (_, b1)] = sb else { continue };
                    let (d1, d2) = (sub(a1, a0), sub(b1, b0));
                    let den = cross(d1, d2);
                    if den.abs() < 1e-300 {
                        continue;
                    }
                    let w = sub(b0, a0);
                    let (t, s) = (cross(w, d2) / den, cross(w, d1) / den);
                    if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&s) {
                        return Some(add(a0, scale(d1, t)));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spline_cv(pts: &[V2], closed: bool) -> Cv {
        let chain = crate::spline::chain(pts, closed);
        let total = chain.len() as u32;
        let bbox =
            crate::profile::bbox_of(&Shape::Spline { chain: chain.clone(), first: 0, total, closed }, [[0.0; 2]; 2]);
        Cv {
            id: crate::sketch::EntityId(0),
            shape: Shape::Spline { total: chain.len() as u32, first: 0, chain, closed },
            p: [[0.0; 2]; 2],
            v: [usize::MAX; 2],
            bbox,
            piece: 0,
        }
    }

    #[test]
    fn finds_the_crossing_of_a_figure_eight() {
        let cv = spline_cv(&[[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]], true);
        let at = self_crossing(&cv).expect("crossing");
        assert!((at[0] - 5.0).abs() < 1.0 && (at[1] - 5.0).abs() < 1.0, "{at:?}");
        let ring = spline_cv(&[[10.0, 0.0], [0.0, 10.0], [-10.0, 0.0], [0.0, -10.0]], true);
        assert!(self_crossing(&ring).is_none());
    }

    #[test]
    fn nearest_param_inverts_eval() {
        let cv = spline_cv(&[[0.0, 0.0], [10.0, 5.0], [20.0, 0.0], [30.0, 8.0]], false);
        for u in [0.2, 0.9, 1.5, 2.7] {
            let p = eval(&cv, u).0;
            assert!((nearest_param(&cv, p) - u).abs() < 1e-6, "{u}");
        }
    }
}
