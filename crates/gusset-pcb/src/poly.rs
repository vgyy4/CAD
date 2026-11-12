//! Polygon areas for zone fills and the editor: booleans and offsets (through the
//! `i_overlay` crate, MIT OR Apache-2.0), simple shapes as polygons, holes bridged into
//! their outline ("fractured", as Gerber regions and KiCad's filled polygons are written)
//! and ear-clipping triangulation for drawing (egui fills convex shapes only).

use crate::board::Pt;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};

/// One area: its outline, then its holes.
pub type Area = Vec<Vec<Pt>>;
/// Several areas.
pub type Areas = Vec<Area>;

/// Chord tolerance of round shapes (mm).
pub const TOL: f64 = 0.005;

fn finite(p: &[Pt]) -> bool {
    p.iter().all(|q| q[0].is_finite() && q[1].is_finite() && q[0].abs() < 1e7 && q[1].abs() < 1e7)
}

fn clean(contours: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    contours.iter().filter(|c| c.len() >= 3 && finite(c)).cloned().collect()
}

fn areas_contours(a: &Areas) -> Vec<Vec<Pt>> {
    clean(&a.iter().flatten().cloned().collect::<Vec<_>>())
}

/// The union of polygons (non-zero winding: overlapping polygons of either orientation).
pub fn union(polys: &[Vec<Pt>]) -> Areas {
    let polys = clean(polys);
    if polys.is_empty() {
        return Vec::new();
    }
    // Each polygon its own orientation: make them all one way so non-zero means "any".
    let polys: Vec<Vec<Pt>> = polys.into_iter().map(ccw).collect();
    let empty: Vec<Vec<Pt>> = Vec::new();
    polys.overlay(&empty, OverlayRule::Subject, FillRule::NonZero)
}

/// `a` with `b` taken away.
pub fn difference(a: &Areas, b: &Areas) -> Areas {
    let (sa, sb) = (areas_contours(a), areas_contours(b));
    if sa.is_empty() {
        return Vec::new();
    }
    if sb.is_empty() {
        return a.clone();
    }
    sa.overlay(&sb, OverlayRule::Difference, FillRule::EvenOdd)
}

/// Where `a` and `b` overlap.
pub fn intersect(a: &Areas, b: &Areas) -> Areas {
    let (sa, sb) = (areas_contours(a), areas_contours(b));
    if sa.is_empty() || sb.is_empty() {
        return Vec::new();
    }
    sa.overlay(&sb, OverlayRule::Intersect, FillRule::EvenOdd)
}

/// `a` and `b` together.
pub fn join(a: &Areas, b: &Areas) -> Areas {
    let (sa, sb) = (areas_contours(a), areas_contours(b));
    if sb.is_empty() {
        return a.clone();
    }
    if sa.is_empty() {
        return b.clone();
    }
    sa.overlay(&sb, OverlayRule::Union, FillRule::EvenOdd)
}

/// Areas grown (`d > 0`) or shrunk (`d < 0`) by `d` with round corners.
pub fn offset(a: &Areas, d: f64) -> Areas {
    if !d.is_finite() || d.abs() < 1e-9 {
        return a.clone();
    }
    let shapes: Areas = a.iter().map(|s| clean(s)).filter(|s| !s.is_empty()).collect();
    if shapes.is_empty() {
        return Vec::new();
    }
    // Segment length over radius for the round joins: chords within TOL of the arc.
    let step = (2.0 * libm::acos(1.0 - (TOL / d.abs()).min(1.0))).clamp(0.02, 0.5);
    let style = OutlineStyle::new(d).line_join(LineJoin::Round(step));
    shapes.outline(&style)
}

/// The even-odd area of closed contours (outlines and holes in any orientation).
pub fn even_odd(contours: &[Vec<Pt>]) -> Areas {
    let c = clean(contours);
    if c.is_empty() {
        return Vec::new();
    }
    let empty: Vec<Vec<Pt>> = Vec::new();
    c.overlay(&empty, OverlayRule::Subject, FillRule::EvenOdd)
}

/// Twice the signed area (positive: counter-clockwise with Y up).
fn area2(p: &[Pt]) -> f64 {
    let n = p.len();
    (0..n)
        .filter_map(|i| {
            let (a, b) = (p.get(i)?, p.get((i + 1) % n)?);
            Some(a[0] * b[1] - b[0] * a[1])
        })
        .sum()
}

fn ccw(mut p: Vec<Pt>) -> Vec<Pt> {
    if area2(&p) < 0.0 {
        p.reverse();
    }
    p
}

/// The area covered (outline minus holes), mm².
pub fn area_of(a: &Area) -> f64 {
    let mut it = a.iter();
    let outer = it.next().map_or(0.0, |o| area2(o).abs() / 2.0);
    outer - it.map(|h| area2(h).abs() / 2.0).sum::<f64>()
}

/// The total area of several areas, mm².
pub fn total_area(a: &Areas) -> f64 {
    a.iter().map(area_of).sum()
}

/// Whether `q` lies in the area (inside the outline, outside every hole).
pub fn contains(a: &Area, q: Pt) -> bool {
    let mut it = a.iter();
    match it.next() {
        Some(o) if crate::geom::inside(o, q) => !it.any(|h| crate::geom::inside(h, q)),
        _ => false,
    }
}

/// A circle as a polygon whose edges are tangent to it (so it covers the disc and lies at
/// most `TOL` beyond it).
pub fn circle(c: Pt, r: f64) -> Vec<Pt> {
    capsule(c, c, r)
}

/// The set of points within `r` of the segment `a`–`b`, as a polygon covering it: its
/// straight sides at `r`, its round ends circumscribed (edges tangent to the arcs, so no
/// point of the capsule is left out and none lies more than `TOL` beyond it).
pub fn capsule(a: Pt, b: Pt, r: f64) -> Vec<Pt> {
    let r = r.max(1e-6);
    let step = (2.0 * libm::acos(1.0 - (TOL / r).min(1.0))).clamp(0.05, std::f64::consts::FRAC_PI_4);
    let n = ((std::f64::consts::PI / step).ceil() as usize).clamp(4, 180);
    let step = std::f64::consts::PI / n as f64;
    let rr = r / libm::cos(step / 2.0);
    let dir = if libm::hypot(b[0] - a[0], b[1] - a[1]) > 1e-12 { libm::atan2(b[1] - a[1], b[0] - a[0]) } else { 0.0 };
    let half = std::f64::consts::FRAC_PI_2;
    let at = |c: Pt, rad: f64, t: f64| [c[0] + rad * libm::cos(t), c[1] + rad * libm::sin(t)];
    let mut out = Vec::with_capacity(2 * n + 4);
    for (c, start) in [(b, dir - half), (a, dir + half)] {
        out.push(at(c, r, start));
        for i in 0..n {
            out.push(at(c, rr, start + step * (i as f64 + 0.5)));
        }
        out.push(at(c, r, start + std::f64::consts::PI));
    }
    out
}

/// A rectangle of `width` along the segment `a`–`b` (square ends at a and b).
pub fn bar(a: Pt, b: Pt, width: f64) -> Vec<Pt> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = libm::hypot(dx, dy).max(1e-12);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    vec![[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]]
}

fn eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

fn same(a: Pt, b: Pt) -> bool {
    eq(a[0], b[0]) && eq(a[1], b[1])
}

fn cross(o: Pt, a: Pt, b: Pt) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Whether the direction from corner `x` to `d` lies strictly inside the triangle's angle
/// at `x` (between the directions to `u` and `v`).
fn enters(x: Pt, u: Pt, v: Pt, d: Pt) -> bool {
    let s = cross(x, u, v);
    if s == 0.0 || same(x, d) {
        return false;
    }
    cross(x, u, d) * s > 0.0 && cross(x, d, v) * s > 0.0
}

/// Whether `p` is inside or on the triangle `a b c` (either orientation).
fn in_triangle(p: Pt, a: Pt, b: Pt, c: Pt) -> bool {
    let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// One area as a single contour: each hole joined to the outline (or an earlier hole) by a
/// cut that runs out and back along the same line. Gerber regions and KiCad's filled
/// polygons are written this way; even-odd and non-zero filling both give the area back.
pub fn fracture(a: &Area) -> Vec<Pt> {
    let mut it = a.iter();
    let Some(outer) = it.next() else { return Vec::new() };
    let mut poly = ccw(outer.clone());
    // Holes clockwise, the one reaching furthest right first.
    let mut holes: Vec<Vec<Pt>> =
        it.filter(|h| h.len() >= 3).map(|h| ccw(h.clone()).into_iter().rev().collect()).collect();
    let max_x = |h: &Vec<Pt>| h.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
    holes.sort_by(|x, y| max_x(y).total_cmp(&max_x(x)));
    for h in holes {
        let Some((mi, m)) = h.iter().copied().enumerate().max_by(|x, y| x.1[0].total_cmp(&y.1[0])) else { continue };
        let Some(k) = bridge_vertex(&poly, m).map(|k| copy_facing(&poly, k, m)) else { continue };
        let Some(&pk) = poly.get(k) else { continue };
        let n = h.len();
        let mut spliced = Vec::with_capacity(poly.len() + n + 2);
        spliced.extend(poly.get(..=k).unwrap_or_default());
        spliced.extend((0..=n).filter_map(|j| h.get((mi + j) % n)));
        spliced.push(pk);
        spliced.extend(poly.get(k + 1..).unwrap_or_default());
        poly = spliced;
    }
    poly
}

/// Among the copies of vertex `k` (earlier bridges repeat vertices), the one whose
/// inside angle holds the direction to `m`, so the new cut runs through the polygon.
fn copy_facing(poly: &[Pt], k: usize, m: Pt) -> usize {
    let Some(&v) = poly.get(k) else { return k };
    let n = poly.len();
    let tau = std::f64::consts::TAU;
    let ang = |q: Pt| libm::atan2(q[1] - v[1], q[0] - v[0]);
    let copies: Vec<usize> = (0..n).filter(|&j| poly.get(j).is_some_and(|q| same(*q, v))).collect();
    if copies.len() < 2 {
        return k;
    }
    let d = ang(m);
    for &j in &copies {
        let (Some(&u), Some(&w)) = (poly.get((j + n - 1) % n), poly.get((j + 1) % n)) else { continue };
        let start = ang(w);
        let span = (ang(u) - start).rem_euclid(tau);
        if (d - start).rem_euclid(tau) < span {
            return j;
        }
    }
    k
}

/// The vertex of `poly` (counter-clockwise) that a hole's rightmost point `m` can see
/// along a cut to the right (Eberly's method).
fn bridge_vertex(poly: &[Pt], m: Pt) -> Option<usize> {
    let n = poly.len();
    // The nearest edge crossing the ray from m to +X.
    let mut best: Option<(f64, usize)> = None;
    for i in 0..n {
        let (a, b) = (*poly.get(i)?, *poly.get((i + 1) % n)?);
        if (a[1] > m[1]) == (b[1] > m[1]) {
            // Touching the ray at a vertex.
            if eq(a[1], m[1]) && a[0] >= m[0] && best.is_none_or(|(x, _)| a[0] < x) {
                best = Some((a[0], i));
            }
            continue;
        }
        let x = a[0] + (m[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
        if x >= m[0] && best.is_none_or(|(bx, _)| x < bx) {
            best = Some((x, i));
        }
    }
    let (ix, i) = best?;
    let (a, b) = (*poly.get(i)?, *poly.get((i + 1) % n)?);
    if eq(a[1], m[1]) && eq(a[0], ix) {
        return Some(i);
    }
    let hit = [ix, m[1]];
    // The edge's endpoint further right, then any vertex inside the triangle m-hit-p that
    // is closer in angle to the ray.
    let (mut k, p) = if a[0] > b[0] { (i, a) } else { ((i + 1) % n, b) };
    let mut best_ang = f64::INFINITY;
    let mut best_d = f64::INFINITY;
    for (j, q) in poly.iter().enumerate() {
        if j == k || q[0] < m[0] || same(*q, p) {
            continue;
        }
        if in_triangle(*q, m, hit, p) {
            let (dx, dy) = (q[0] - m[0], q[1] - m[1]);
            let ang = libm::atan2(dy.abs(), dx);
            let d = libm::hypot(dx, dy);
            if ang < best_ang - 1e-12 || ((ang - best_ang).abs() <= 1e-12 && d < best_d) {
                best_ang = ang;
                best_d = d;
                k = j;
            }
        }
    }
    Some(k)
}

/// Triangles (indices into `poly`) covering a simple polygon, or one whose holes were
/// joined by [`fracture`], by ear clipping (vertices bucketed on a grid so an ear test
/// looks only near the ear). Degenerate input gives fewer triangles, never a panic; past
/// `max_vertices` vertices the result is empty.
pub fn triangulate(poly: &[Pt], max_vertices: usize) -> Vec<[usize; 3]> {
    let n = poly.len();
    if !(3..=max_vertices).contains(&n) || !finite(poly) {
        return Vec::new();
    }
    let sign = if area2(poly) >= 0.0 { 1.0 } else { -1.0 };
    let p = |i: usize| poly.get(i).copied().unwrap_or([0.0, 0.0]);
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut alive = vec![true; n];
    // The grid of vertices.
    let (lo, hi) = poly.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), q| {
        ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])])
    });
    let side = ((n as f64).sqrt().ceil() as usize).clamp(1, 512);
    let span = [(hi[0] - lo[0]).max(1e-9), (hi[1] - lo[1]).max(1e-9)];
    let cell = |q: Pt| -> (usize, usize) {
        let f = |v: f64, l: f64, s: f64| (((v - l) / s * side as f64).floor().max(0.0) as usize).min(side - 1);
        (f(q[0], lo[0], span[0]), f(q[1], lo[1], span[1]))
    };
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); side * side];
    for i in 0..n {
        let (x, y) = cell(p(i));
        if let Some(c) = grid.get_mut(y * side + x) {
            c.push(i);
        }
    }
    let mut out = Vec::with_capacity(n.saturating_sub(2));
    let mut left = n;
    let mut i = 0usize;
    let mut stalled = 0usize;
    let get = |v: &Vec<usize>, k: usize| v.get(k).copied().unwrap_or(0);
    while left > 3 {
        let (ia, ib) = (get(&prev, i), i);
        let ic = get(&next, i);
        let (a, b, c) = (p(ia), p(ib), p(ic));
        let turn = cross(a, b, c) * sign;
        let mut ear = turn > 0.0;
        if ear {
            let (x0, y0) = cell([a[0].min(b[0]).min(c[0]), a[1].min(b[1]).min(c[1])]);
            let (x1, y1) = cell([a[0].max(b[0]).max(c[0]), a[1].max(b[1]).max(c[1])]);
            'scan: for y in y0..=y1 {
                for x in x0..=x1 {
                    for &j in grid.get(y * side + x).map(Vec::as_slice).unwrap_or_default() {
                        if j == ia || j == ib || j == ic || !alive.get(j).copied().unwrap_or(false) {
                            continue;
                        }
                        let q = p(j);
                        // A copy of a corner (bridged holes) blocks the ear only when one
                        // of its edges runs into the triangle.
                        let corner = if same(q, a) {
                            Some((a, b, c))
                        } else if same(q, b) {
                            Some((b, c, a))
                        } else if same(q, c) {
                            Some((c, a, b))
                        } else {
                            None
                        };
                        if let Some((x, u, v)) = corner {
                            let (qp, qn) = (p(get(&prev, j)), p(get(&next, j)));
                            if enters(x, u, v, qp) || enters(x, u, v, qn) {
                                ear = false;
                                break 'scan;
                            }
                            continue;
                        }
                        if in_triangle(q, a, b, c) {
                            ear = false;
                            break 'scan;
                        }
                    }
                }
            }
        }
        if ear || (turn == 0.0 && stalled >= left) {
            if turn != 0.0 {
                out.push([ia, ib, ic]);
            }
            if let Some(s) = next.get_mut(ia) {
                *s = ic;
            }
            if let Some(s) = prev.get_mut(ic) {
                *s = ia;
            }
            if let Some(s) = alive.get_mut(ib) {
                *s = false;
            }
            left -= 1;
            stalled = 0;
            i = ia;
        } else {
            i = ic;
            stalled += 1;
            if stalled > 2 * left {
                // No ear (self-overlapping input): give what was found.
                break;
            }
        }
    }
    if left == 3 {
        let (a, b) = (i, get(&next, i));
        let c = get(&next, b);
        if cross(p(a), p(b), p(c)) != 0.0 {
            out.push([a, b, c]);
        }
    }
    out
}
