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
}

fn dist(a: P2, b: P2) -> f64 {
    libm::hypot(a.0 - b.0, a.1 - b.1)
}

fn sample(c: &Poly) -> Vec<P2> {
    let tau = 2.0 * std::f64::consts::PI;
    match *c {
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

/// One point (mm) inside each region of the sketch that an odd number of `loops` surround.
/// `None` when there is nothing to draw.
pub(crate) fn picks(curves: &[Poly], loops: &[Vec<P2>]) -> Option<Vec<P2>> {
    let polys: Vec<Vec<P2>> = curves.iter().map(sample).collect();
    let all = || polys.iter().flatten().chain(loops.iter().flatten()).copied();
    all().next()?;
    let x0 = all().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let x1 = all().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let y0 = all().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let y1 = all().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) {
        return None;
    }
    let span = (x1 - x0).max(y1 - y0).max(1e-6);
    let x0 = x0 - span * 0.02;
    let y0 = y0 - span * 0.02;
    let h = span * 1.04 / GRID as f64;
    let nx = ((x1 - x0) / h) as usize + 3;
    let ny = ((y1 - y0) / h) as usize + 3;
    let mut wall = vec![false; nx * ny];
    for pl in &polys {
        for w in pl.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
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
    for (cid, (border, cells)) in regions.iter().enumerate() {
        if *border || cells.len() < 4 {
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
        let k = loops.iter().filter(|l| l.len() > 2 && inside(l, p)).count();
        if k % 2 == 1 {
            res.push((round6(p.0), round6(p.1)));
        }
    }
    Some(res)
}
