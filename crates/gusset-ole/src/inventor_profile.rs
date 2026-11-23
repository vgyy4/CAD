//! Profile picks: which regions of a sketch an extrude or revolve uses, from the loops it
//! stored in the history body, as one sketch-plane point (mm) per region.
//!
//! The sketch's curves are drawn on a grid; the cells they leave are flooded into regions;
//! a region is kept when it lies inside an odd number of the feature's loops. Everything is
//! bounded by the grid ([`GRID`] cells a side) and the sketch's curve count.

use std::collections::VecDeque;

/// Cells along the longest side of the sketch.
const GRID: usize = 500;

pub(crate) type P2 = (f64, f64);

/// One sketch curve in millimetres, non-construction only.
#[derive(Debug, Clone)]
pub(crate) enum Poly {
    Line(P2, P2),
    Circle(P2, f64),
    Arc {
        c: P2,
        s: P2,
        e: P2,
    },
    /// Centre, unit major direction, semi-axes.
    Ellipse {
        c: P2,
        major: P2,
        a: f64,
        b: f64,
    },
    /// A curve given as points along it (a B-spline, sampled).
    Path(Vec<P2>),
}

fn dist(a: P2, b: P2) -> f64 {
    libm::hypot(a.0 - b.0, a.1 - b.1)
}

pub(crate) fn sample(c: &Poly) -> Vec<P2> {
    let tau = 2.0 * std::f64::consts::PI;
    match *c {
        Poly::Path(ref pts) => pts.clone(),
        Poly::Line(a, b) => vec![a, b],
        Poly::Circle(c, r) => (0..=96)
            .map(|k| (c.0 + r * libm::cos(tau * f64::from(k) / 96.0), c.1 + r * libm::sin(tau * f64::from(k) / 96.0)))
            .collect(),
        Poly::Ellipse { c, major, a, b } => (0..=96)
            .map(|k| {
                let t = tau * f64::from(k) / 96.0;
                let (u, v) = (a * libm::cos(t), b * libm::sin(t));
                (c.0 + u * major.0 - v * major.1, c.1 + u * major.1 + v * major.0)
            })
            .collect(),
        Poly::Arc { c, s, e } => {
            let r = dist(c, s);
            let a0 = libm::atan2(s.1 - c.1, s.0 - c.0);
            let mut a1 = libm::atan2(e.1 - c.1, e.0 - c.0);
            while a1 <= a0 + 1e-12 {
                a1 += tau;
            }
            let n = ((48.0 * (a1 - a0) / std::f64::consts::PI) as usize).max(4);
            (0..=n)
                .map(|k| {
                    let a = a0 + (a1 - a0) * k as f64 / n as f64;
                    (c.0 + r * libm::cos(a), c.1 + r * libm::sin(a))
                })
                .collect()
        }
    }
}

/// Points along a B-spline (degree, knots, poles, weights: empty for a polynomial curve),
/// [`SPLINE_SAMPLES`] per knot span, by de Boor's algorithm. `None` for an inconsistent curve.
pub(crate) fn bspline_points(degree: u32, knots: &[f64], poles: &[P2], weights: &[f64]) -> Option<Vec<P2>> {
    const SPLINE_SAMPLES: usize = 16;
    let p = degree as usize;
    let n = poles.len();
    if n <= p || knots.len() != n + p + 1 || !(weights.is_empty() || weights.len() == n) {
        return None;
    }
    // Homogeneous poles: (w x, w y, w).
    let hp: Vec<[f64; 3]> = poles
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let w = weights.get(i).copied().unwrap_or(1.0);
            [q.0 * w, q.1 * w, w]
        })
        .collect();
    let mut out = Vec::new();
    for span in p..n {
        let (t0, t1) = (*knots.get(span)?, *knots.get(span + 1)?);
        if t1 <= t0 {
            continue;
        }
        for k in 0..=SPLINE_SAMPLES {
            let t = t0 + (t1 - t0) * k as f64 / SPLINE_SAMPLES as f64;
            let mut d: Vec<[f64; 3]> = (0..=p).map(|j| hp.get(span + j - p).copied()).collect::<Option<_>>()?;
            for r in 1..=p {
                for j in (r..=p).rev() {
                    let i = span + j - p;
                    let (a0, a1) = (*knots.get(i)?, *knots.get(i + p + 1 - r)?);
                    let alpha = if a1 > a0 { (t - a0) / (a1 - a0) } else { 0.0 };
                    let (lo, hi) = (*d.get(j - 1)?, *d.get(j)?);
                    let [l0, l1, l2] = lo;
                    let [h0, h1, h2] = hi;
                    *d.get_mut(j)? = [l0 + alpha * (h0 - l0), l1 + alpha * (h1 - l1), l2 + alpha * (h2 - l2)];
                }
            }
            let q = *d.get(p)?;
            if q[2] <= 0.0 {
                return None;
            }
            out.push((q[0] / q[2], q[1] / q[2]));
        }
    }
    (out.len() >= 2).then_some(out)
}

fn inside(poly: &[P2], p: P2) -> bool {
    let mut c = false;
    for (i, a) in poly.iter().enumerate() {
        let Some(b) = poly.get((i + 1) % poly.len()) else { continue };
        if (a.1 > p.1) != (b.1 > p.1) {
            let xx = a.0 + (p.1 - a.1) * (b.0 - a.0) / (b.1 - a.1);
            if p.0 < xx {
                c = !c;
            }
        }
    }
    c
}

fn round6(x: f64) -> f64 {
    libm::round(x * 1e6) / 1e6
}

/// One point (mm) inside each region of the sketch that an odd number of the feature's loops
/// surround (`bodies`: the loops of each body the history blob keeps for it; mostly one wire per
/// loop, so the count runs over all of them). A region inside two bodies whose loops cross (each
/// runs partly inside and partly outside the other: a slot drawn as a rectangle and its two end
/// circles, each picked whole) is kept too: the picked regions overlap there, they do not make
/// a hole (a loop wholly inside another is a hole). When that leaves nothing (one loop listed
/// twice, note AR), a region is kept that lies in one of the bodies (an odd number of its own
/// loops). `None` when there is nothing to draw.
pub(crate) fn picks(curves: &[Poly], bodies: &[Vec<Vec<P2>>]) -> Option<Vec<P2>> {
    let all: Vec<&Vec<P2>> = bodies.iter().flatten().collect();
    let odd = |ls: &[&Vec<P2>], p: P2| ls.iter().filter(|l| l.len() > 2 && inside(l, p)).count() % 2 == 1;
    let own: Vec<Vec<&Vec<P2>>> = bodies.iter().map(|b| b.iter().collect()).collect();
    let crossing: Vec<(usize, usize)> = (0..bodies.len())
        .flat_map(|i| (i + 1..bodies.len()).map(move |j| (i, j)))
        .filter(|&(i, j)| {
            let (Some(a), Some(b)) = (bodies.get(i), bodies.get(j)) else { return false };
            a.iter().any(|la| b.iter().any(|lb| crosses(la, lb) || crosses(lb, la)))
        })
        .collect();
    let keep = |p: P2| {
        odd(&all, p) || {
            let ins: Vec<usize> = own.iter().enumerate().filter(|(_, b)| odd(b, p)).map(|(i, _)| i).collect();
            crossing.iter().any(|(i, j)| ins.contains(i) && ins.contains(j))
        }
    };
    let res = picks_by(curves, &all, &keep)?;
    if !res.is_empty() || bodies.len() < 2 {
        return Some(res);
    }
    picks_by(curves, &all, &|p| own.iter().any(|b| odd(b, p)))
}

/// Does loop `a` run partly inside and partly outside loop `b`?
fn crosses(a: &[P2], b: &[P2]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    let n = a.iter().filter(|&&p| inside(b, p)).count();
    n > 0 && n < a.len()
}

/// [`picks`] with a region kept when `keep` holds at its point. A loop that no point falls in
/// after the pass over the whole sketch (a small hole far from the rest: its inside is under a
/// few cells) is looked at again on a grid over its own box; so is each circle or arc a few
/// cells across (the half discs a row of small circles on a line leaves on either side of it:
/// a PCB edge's castellations), on a finer grid over its box.
fn picks_by(curves: &[Poly], loops: &[&Vec<P2>], keep: &dyn Fn(P2) -> bool) -> Option<Vec<P2>> {
    let polys: Vec<Vec<P2>> = curves.iter().map(sample).collect();
    let all = || polys.iter().flatten().chain(loops.iter().copied().flatten()).copied();
    all().next()?;
    let whole = extent(all())?;
    let mut res = picks_in(&polys, keep, whole, &[], GRID)?;
    for l in loops {
        if l.len() < 3 || res.iter().any(|&p| inside(l, p)) {
            continue;
        }
        let Some(b) = extent(l.iter().copied()) else { continue };
        for p in picks_in(&polys, keep, b, &[], GRID).unwrap_or_default() {
            if !res.contains(&p) {
                res.push(p);
            }
        }
    }
    let cell = (whole[1] - whole[0]).max(whole[3] - whole[2]) * 1.04 / GRID as f64;
    let small = curves.iter().zip(&polys).filter_map(|(c, pl)| {
        let b = extent(pl.iter().copied())?;
        let round = matches!(c, Poly::Circle(..) | Poly::Arc { .. } | Poly::Ellipse { .. });
        (round && (b[1] - b[0]).max(b[3] - b[2]) < SMALL_CELLS * cell).then_some(b)
    });
    for b in small.take(MAX_SMALL) {
        for p in picks_in(&polys, keep, b, &res, FINE_GRID).unwrap_or_default() {
            if !res.contains(&p) {
                res.push(p);
            }
        }
    }
    Some(res)
}

/// A circle or arc less than this many cells across is looked at again on its own grid.
const SMALL_CELLS: f64 = 16.0;
/// Cells along the longest side of such a curve's grid.
const FINE_GRID: usize = 100;
/// Most such curves looked at.
const MAX_SMALL: usize = 256;

/// The part of the segment `a`-`b` inside the box (`x0, x1, y0, y1`), if any (Liang-Barsky).
fn clip(a: P2, b: P2, bx: [f64; 4]) -> Option<(P2, P2)> {
    let [x0, x1, y0, y1] = bx;
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0 - x0), (dx, x1 - a.0), (-dy, a.1 - y0), (dy, y1 - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then_some(((a.0 + t0 * dx, a.1 + t0 * dy), (a.0 + t1 * dx, a.1 + t1 * dy)))
}

/// The box (`x0, x1, y0, y1`) of some points; `None` when it is not finite.
fn extent(pts: impl Iterator<Item = P2> + Clone) -> Option<[f64; 4]> {
    let x0 = pts.clone().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let x1 = pts.clone().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let y0 = pts.clone().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let y1 = pts.map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    (x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()).then_some([x0, x1, y0, y1])
}

/// [`picks`] on a grid of `grid` cells a side over the box `b` (with a margin): curves outside
/// it are left out, and so are regions that hold a point of `known`.
fn picks_in(polys: &[Vec<P2>], keep: &dyn Fn(P2) -> bool, b: [f64; 4], known: &[P2], grid: usize) -> Option<Vec<P2>> {
    let [x0, x1, y0, y1] = b;
    let span = (x1 - x0).max(y1 - y0).max(1e-6);
    let x0 = x0 - span * 0.02;
    let y0 = y0 - span * 0.02;
    let h = span * 1.04 / grid as f64;
    let nx = ((x1 - x0) / h) as usize + 3;
    let ny = ((y1 - y0) / h) as usize + 3;
    let mut wall = vec![false; nx * ny];
    for pl in polys {
        for w in pl.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
            // Only the part of the segment over the grid is drawn.
            let Some((a, b)) = clip(a, b, [x0, x0 + nx as f64 * h, y0, y0 + ny as f64 * h]) else { continue };
            let m = ((dist(a, b) / (h * 0.25)) as usize).max(1);
            for k in 0..=m {
                let t = k as f64 / m as f64;
                let x = a.0 + (b.0 - a.0) * t;
                let y = a.1 + (b.1 - a.1) * t;
                let (i, j) = (((x - x0) / h) as i64, ((y - y0) / h) as i64);
                if (0..nx as i64).contains(&i)
                    && (0..ny as i64).contains(&j)
                    && let Some(c) = wall.get_mut(j as usize * nx + i as usize)
                {
                    *c = true;
                }
            }
        }
    }
    let nb = |c: usize| {
        let (i, j) = ((c % nx) as i64, (c / nx) as i64);
        [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)].into_iter().filter_map(move |(di, dj)| {
            let (ii, jj) = (i + di, j + dj);
            ((0..nx as i64).contains(&ii) && (0..ny as i64).contains(&jj)).then(|| jj as usize * nx + ii as usize)
        })
    };
    let mut comp = vec![usize::MAX; nx * ny];
    let mut regions: Vec<(bool, Vec<usize>)> = Vec::new();
    for s in 0..nx * ny {
        if wall.get(s) != Some(&false) || comp.get(s) != Some(&usize::MAX) {
            continue;
        }
        let cid = regions.len();
        let mut q = VecDeque::from([s]);
        if let Some(c) = comp.get_mut(s) {
            *c = cid;
        }
        let mut cells = Vec::new();
        let mut border = false;
        while let Some(c) = q.pop_front() {
            cells.push(c);
            let (i, j) = (c % nx, c / nx);
            if i == 0 || j == 0 || i == nx - 1 || j == ny - 1 {
                border = true;
            }
            for t in nb(c) {
                if wall.get(t) == Some(&false) && comp.get(t) == Some(&usize::MAX) {
                    if let Some(x) = comp.get_mut(t) {
                        *x = cid;
                    }
                    q.push_back(t);
                }
            }
        }
        regions.push((border, cells));
    }
    let mut res = Vec::new();
    let mut depth = vec![-1i32; nx * ny];
    let known_cells: Vec<usize> = known
        .iter()
        .filter_map(|p| {
            let (i, j) = (((p.0 - x0) / h) as i64, ((p.1 - y0) / h) as i64);
            ((0..nx as i64).contains(&i) && (0..ny as i64).contains(&j)).then(|| j as usize * nx + i as usize)
        })
        .collect();
    for (cid, (border, cells)) in regions.iter().enumerate() {
        if *border || cells.len() < 4 || known_cells.iter().any(|&c| comp.get(c) == Some(&cid)) {
            continue;
        }
        // The deepest cell: multi-source BFS from the region's boundary.
        let mut q = VecDeque::new();
        for &c in cells {
            if nb(c).any(|t| comp.get(t) != Some(&cid)) {
                if let Some(d) = depth.get_mut(c) {
                    *d = 0;
                }
                q.push_back(c);
            }
        }
        while let Some(c) = q.pop_front() {
            let dc = depth.get(c).copied().unwrap_or(0);
            for t in nb(c) {
                if comp.get(t) == Some(&cid) && depth.get(t) == Some(&-1) {
                    if let Some(d) = depth.get_mut(t) {
                        *d = dc + 1;
                    }
                    q.push_back(t);
                }
            }
        }
        let mut best = *cells.first()?;
        let mut best_d = i32::MIN;
        for &c in cells {
            let d = depth.get(c).copied().unwrap_or(0).max(0);
            if d > best_d {
                best = c;
                best_d = d;
            }
        }
        let p = (x0 + ((best % nx) as f64 + 0.5) * h, y0 + ((best / nx) as f64 + 0.5) * h);
        if keep(p) {
            res.push((round6(p.0), round6(p.1)));
        }
    }
    Some(res)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn square(c: P2, r: f64) -> Vec<P2> {
        vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 - r), (c.0 + r, c.1 + r), (c.0 - r, c.1 + r), (c.0 - r, c.1 - r)]
    }

    /// A small hole far from the rest of a large sketch is found on a grid over its own box;
    /// a loop listed twice, which leaves no region inside an odd number of loops, is the region
    /// it bounds (note AR).
    #[test]
    fn small_loops_and_loops_listed_twice() {
        let curves = vec![
            Poly::Line((0.0, 0.0), (900.0, 0.0)),
            Poly::Line((900.0, 0.0), (900.0, 500.0)),
            Poly::Line((900.0, 500.0), (0.0, 500.0)),
            Poly::Line((0.0, 500.0), (0.0, 0.0)),
            Poly::Circle((20.0, 30.0), 1.5),
        ];
        let hole = square((20.0, 30.0), 1.5);
        let p = picks(&curves, &[vec![hole]]).unwrap();
        assert_eq!(p.len(), 1);
        assert!(dist(p[0], (20.0, 30.0)) < 1.5, "{p:?}");
        let outer = square((450.0, 250.0), 450.0);
        let p = picks(&curves, &[vec![outer.clone()], vec![outer]]).unwrap();
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(dist(p[0], (20.0, 30.0)) > 1.5, "{p:?}");
    }

    /// De Boor on a quadratic Bezier and a rational quarter circle.
    #[test]
    fn bspline_points_follow_the_curve() {
        let k = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let pts = bspline_points(2, &k, &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)], &[]).unwrap();
        assert_eq!(pts.first(), Some(&(0.0, 0.0)));
        assert_eq!(pts.last(), Some(&(2.0, 0.0)));
        let mid = pts[pts.len() / 2];
        assert!(dist(mid, (1.0, 1.0)) < 1e-12, "{mid:?}");
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = bspline_points(2, &k, &[(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], &[1.0, w, 1.0]).unwrap();
        assert!(arc.iter().all(|p| (libm::hypot(p.0, p.1) - 1.0).abs() < 1e-12));
        assert!(bspline_points(3, &k, &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)], &[]).is_none());
    }
}
