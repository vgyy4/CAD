//! 3D surface finishing: raster (parallel-line) drop-cutter over a triangle mesh (ADR 0040,
//! "3D finishing" amendment).

use crate::{CamError, MAX_MOVES, Move, Setup, Toolpath};

/// Most samples (drop-cutter evaluations) one raster job may take.
const MAX_SAMPLES: usize = 3_000_000;
/// Most raster lines, grid cells per side and grid entries.
const MAX_LINES: usize = 1_000_000;
const MAX_GRID: usize = 512;
const MAX_GRID_ENTRIES: usize = 20_000_000;
/// Most triangles.
const MAX_TRIS: usize = 4_000_000;

/// A triangle mesh to finish: Z up, the tool comes from +Z.
#[derive(Debug, Clone, Copy)]
pub struct Surface3d<'a> {
    pub positions: &'a [[f32; 3]],
    /// Three indices per triangle.
    pub indices: &'a [u32],
}

/// The cutter's end shape (its diameter is the setup's `tool_diameter`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cutter {
    /// Flat end mill.
    Flat,
    /// Ball nose (corner radius = D/2).
    Ball,
    /// Bull nose: flat end with a rounded corner of `corner_radius` (0 ..= D/2).
    Bull { corner_radius: f64 },
}

/// Raster finishing parameters (mm, degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterSpec {
    /// Direction of the lines, counter-clockwise from +X.
    pub angle_deg: f64,
    /// Distance between neighbouring lines.
    pub stepover: f64,
    /// Sampling distance along a line (at most; lines are divided evenly).
    pub step: f64,
    /// XY box `(min, max)` the tool tip stays in; `None` is the mesh's XY bounds.
    pub bounds: Option<([f64; 2], [f64; 2])>,
    /// Material left on the surface, as a rise in Z (not a normal offset).
    pub stock_to_leave: f64,
    /// Points that deviate less than this from the line through their neighbours are dropped.
    pub tolerance: f64,
}

/// Radial profile of the cutter's underside: height above the tip at radius `rho`.
#[derive(Clone, Copy)]
struct Profile {
    r: f64,
    c: f64,
}

impl Profile {
    fn h(&self, rho: f64) -> f64 {
        let s = rho - (self.r - self.c);
        if s <= 0.0 { 0.0 } else { self.c - libm::sqrt((self.c * self.c - s * s).max(0.0)) }
    }
}

struct Dropper {
    tris: Vec<[[f64; 3]; 3]>,
    zmax: Vec<f64>,
    /// XY bounding box `(x0, y0, x1, y1)` per triangle.
    bbox: Vec<[f64; 4]>,
    min: [f64; 2],
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    stamp: Vec<u32>,
    epoch: u32,
    prof: Profile,
}

fn cell_of(v: f64, min: f64, cell: f64, n: usize) -> usize {
    let i = libm::floor((v - min) / cell);
    if i <= 0.0 { 0 } else { (i as usize).min(n.saturating_sub(1)) }
}

impl Dropper {
    fn new(s: &Surface3d, prof: Profile, lo_xy: [f64; 2], hi_xy: [f64; 2]) -> Result<Self, CamError> {
        if !s.indices.len().is_multiple_of(3) {
            return Err(CamError::Invalid("indices must be a multiple of 3".into()));
        }
        if s.indices.len() / 3 > MAX_TRIS {
            return Err(CamError::TooLarge("too many triangles".into()));
        }
        let mut tris = Vec::with_capacity(s.indices.len() / 3);
        for t in s.indices.chunks_exact(3) {
            let mut v = [[0.0; 3]; 3];
            for (slot, &i) in v.iter_mut().zip(t) {
                let p = s.positions.get(i as usize).ok_or_else(|| CamError::Invalid("index out of range".into()))?;
                *slot = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
                if !slot.iter().all(|c| c.is_finite()) {
                    return Err(CamError::Invalid("non-finite position".into()));
                }
            }
            tris.push(v);
        }
        let span = (hi_xy[0] - lo_xy[0]).max(hi_xy[1] - lo_xy[1]).max(1e-9);
        let cell = prof.r.max(span / MAX_GRID as f64);
        let nx = ((((hi_xy[0] - lo_xy[0]) / cell) as usize) + 1).min(MAX_GRID);
        let ny = ((((hi_xy[1] - lo_xy[1]) / cell) as usize) + 1).min(MAX_GRID);
        let mut cells = vec![Vec::new(); nx * ny];
        let mut entries = 0usize;
        let mut zmax = Vec::with_capacity(tris.len());
        let mut bbox = Vec::with_capacity(tris.len());
        for (ti, t) in tris.iter().enumerate() {
            zmax.push(t.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max));
            let lo = |k: usize| t.iter().map(|p| p.get(k).copied().unwrap_or(0.0)).fold(f64::INFINITY, f64::min);
            let hi = |k: usize| t.iter().map(|p| p.get(k).copied().unwrap_or(0.0)).fold(f64::NEG_INFINITY, f64::max);
            bbox.push([lo(0), lo(1), hi(0), hi(1)]);
            let (x0, x1) = (cell_of(lo(0), lo_xy[0], cell, nx), cell_of(hi(0), lo_xy[0], cell, nx));
            let (y0, y1) = (cell_of(lo(1), lo_xy[1], cell, ny), cell_of(hi(1), lo_xy[1], cell, ny));
            entries += (x1 - x0 + 1) * (y1 - y0 + 1);
            if entries > MAX_GRID_ENTRIES {
                return Err(CamError::TooLarge("mesh too large for the triangle index".into()));
            }
            for iy in y0..=y1 {
                for ix in x0..=x1 {
                    if let Some(c) = cells.get_mut(iy * nx + ix) {
                        c.push(ti as u32);
                    }
                }
            }
        }
        let stamp = vec![0; tris.len()];
        Ok(Dropper { tris, zmax, bbox, min: lo_xy, cell, nx, ny, cells, stamp, epoch: 0, prof })
    }

    /// Indices of the triangles whose XY box is within `r` of `(x, y)`.
    fn near_tris(&mut self, x: f64, y: f64, r: f64) -> Vec<u32> {
        let (x0, x1) =
            (cell_of(x - r, self.min[0], self.cell, self.nx), cell_of(x + r, self.min[0], self.cell, self.nx));
        let (y0, y1) =
            (cell_of(y - r, self.min[1], self.cell, self.ny), cell_of(y + r, self.min[1], self.cell, self.ny));
        let mut out: Vec<u32> = Vec::new();
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let Some(list) = self.cells.get(iy * self.nx + ix) else { continue };
                for &ti in list {
                    let Some(bb) = self.bbox.get(ti as usize) else { continue };
                    let dx = (bb[0] - x).max(x - bb[2]).max(0.0);
                    let dy = (bb[1] - y).max(y - bb[3]).max(0.0);
                    if dx * dx + dy * dy <= r * r {
                        out.push(ti);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Lowest tool tip Z at `(x, y)` that does not gouge any triangle; `None` if no triangle
    /// is within the cutter's radius.
    fn drop(&mut self, x: f64, y: f64) -> Option<f64> {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.epoch = 1;
        }
        let r = self.prof.r;
        let (x0, x1) =
            (cell_of(x - r, self.min[0], self.cell, self.nx), cell_of(x + r, self.min[0], self.cell, self.nx));
        let (y0, y1) =
            (cell_of(y - r, self.min[1], self.cell, self.ny), cell_of(y + r, self.min[1], self.cell, self.ny));
        let mut best: Option<f64> = None;
        for iy in y0..=y1 {
            for ix in x0..=x1 {
                let Some(list) = self.cells.get(iy * self.nx + ix) else { continue };
                for &ti in list {
                    let Some(st) = self.stamp.get_mut(ti as usize) else { continue };
                    if *st == self.epoch {
                        continue;
                    }
                    *st = self.epoch;
                    let (Some(t), Some(&zm)) = (self.tris.get(ti as usize), self.zmax.get(ti as usize)) else {
                        continue;
                    };
                    // The tip cannot be pushed above the triangle's highest point.
                    if best.is_some_and(|b| zm <= b) {
                        continue;
                    }
                    // Nothing of the triangle within the cutter's radius.
                    if let Some(bb) = self.bbox.get(ti as usize) {
                        let dx = (bb[0] - x).max(x - bb[2]).max(0.0);
                        let dy = (bb[1] - y).max(y - bb[3]).max(0.0);
                        if dx * dx + dy * dy > r * r {
                            continue;
                        }
                    }
                    if let Some(z) = tri_drop(t, x, y, self.prof) {
                        best = Some(best.map_or(z, |b| b.max(z)));
                    }
                }
            }
        }
        best
    }
}

/// Tip Z for one triangle: the maximum of `z(p) - h(|p - xy|)` over the triangle's points
/// within the cutter's radius (a concave function, so the maximum is at a facet contact, on
/// an edge or at a vertex).
fn tri_drop(t: &[[f64; 3]; 3], x: f64, y: f64, prof: Profile) -> Option<f64> {
    let [a, b, c] = *t;
    let mut best: Option<f64> = None;
    let mut take = |z: f64| best = Some(best.map_or(z, |b| b.max(z)));
    let r = prof.r;
    // Vertices.
    for p in [a, b, c] {
        let rho = libm::hypot(p[0] - x, p[1] - y);
        if rho <= r {
            take(p[2] - prof.h(rho));
        }
    }
    // Edges.
    for (p, q) in [(a, b), (b, c), (c, a)] {
        if let Some(z) = edge_drop(p, q, x, y, prof) {
            take(z);
        }
    }
    // Facet.
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let nl = libm::sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
    // Slivers (a pole of a sphere mesh) have no usable facet plane, and the point-in-triangle
    // tolerance would accept any point for them.
    if nl > 1e-9 && n[2].abs() > 1e-12 * nl {
        let (ga, gb) = (-n[0] / n[2], -n[1] / n[2]);
        let m = libm::hypot(ga, gb);
        let (rho, px, py) = if m < 1e-12 {
            (0.0, x, y)
        } else {
            let rho = if prof.c <= 1e-12 { r } else { (r - prof.c) + prof.c * m / libm::sqrt(1.0 + m * m) };
            (rho, x + rho * ga / m, y + rho * gb / m)
        };
        if in_tri_xy(t, px, py) {
            take(a[2] + ga * (px - a[0]) + gb * (py - a[1]) - prof.h(rho));
        }
    }
    best
}

fn in_tri_xy(t: &[[f64; 3]; 3], px: f64, py: f64) -> bool {
    let [a, b, c] = *t;
    let s = |p: [f64; 3], q: [f64; 3]| (q[0] - p[0]) * (py - p[1]) - (q[1] - p[1]) * (px - p[0]);
    let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
    let eps = 1e-12 * ((b[0] - a[0]).abs() + (b[1] - a[1]).abs() + (c[0] - a[0]).abs() + (c[1] - a[1]).abs() + 1.0);
    let neg = d1 < -eps || d2 < -eps || d3 < -eps;
    let pos = d1 > eps || d2 > eps || d3 > eps;
    !(neg && pos)
}

/// Best tip Z over the part of segment `p..q` within the cutter's radius of `(x, y)`.
fn edge_drop(p: [f64; 3], q: [f64; 3], x: f64, y: f64, prof: Profile) -> Option<f64> {
    let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let w = [p[0] - x, p[1] - y];
    let qa = d[0] * d[0] + d[1] * d[1];
    let qb = 2.0 * (w[0] * d[0] + w[1] * d[1]);
    let qc = w[0] * w[0] + w[1] * w[1] - prof.r * prof.r;
    if qa < 1e-18 {
        // Vertical or empty edge: only its top end matters (the vertex test has it).
        return None;
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let sq = libm::sqrt(disc);
    let mut t0 = f64::max(0.0, (-qb - sq) / (2.0 * qa));
    let mut t1 = f64::min(1.0, (-qb + sq) / (2.0 * qa));
    if t0 > t1 {
        return None;
    }
    let f = |t: f64| {
        let rho = libm::hypot(w[0] + t * d[0], w[1] + t * d[1]);
        p[2] + t * d[2] - prof.h(rho.min(prof.r))
    };
    if prof.c <= 1e-12 {
        // Flat: h is 0 inside the radius, so the maximum is at an end of the interval.
        return Some(f(t0).max(f(t1)));
    }
    if (prof.c - prof.r).abs() <= 1e-12 {
        // Ball: the centre touches the edge's line at distance r (closed form); the maximum of
        // the concave f over [t0, t1] is there if the contact is inside, else at an end.
        let mut best = f(t0).max(f(t1));
        let dd = qa + d[2] * d[2];
        let a_dot = w[0] * d[0] + w[1] * d[1];
        let ww = w[0] * w[0] + w[1] * w[1];
        // With a = p_z - centre_z: qa a^2 - 2 (A dz) a + (ww dd - A^2 - r^2 dd) = 0.
        let (qa2, qb2, qc2) = (qa, -2.0 * a_dot * d[2], ww * dd - a_dot * a_dot - prof.r * prof.r * dd);
        let disc2 = qb2 * qb2 - 4.0 * qa2 * qc2;
        if disc2 >= 0.0 {
            let sq2 = libm::sqrt(disc2);
            for a in [(-qb2 - sq2) / (2.0 * qa2), (-qb2 + sq2) / (2.0 * qa2)] {
                let t = -(a_dot + a * d[2]) / dd;
                if t >= t0 && t <= t1 {
                    best = best.max(f(t));
                }
            }
        }
        return Some(best);
    }
    // Concave in t (a line minus a convex function): ternary search to a tenth of a micron.
    let span_mm = libm::sqrt(qa + d[2] * d[2]) * (t1 - t0);
    let iters = (libm::log(span_mm.max(1e-9) / 1e-7) / libm::log(1.5)).clamp(1.0, 70.0) as usize;
    for _ in 0..iters {
        let m1 = t0 + (t1 - t0) / 3.0;
        let m2 = t1 - (t1 - t0) / 3.0;
        if f(m1) < f(m2) {
            t0 = m1;
        } else {
            t1 = m2;
        }
    }
    Some(f(0.5 * (t0 + t1)))
}

/// Deepest subdivision of a chord (the sampling step divided by 2^this).
const MAX_REFINE_DEPTH: usize = 9;

/// Pushes the interior points that keep the chord `a..b` of a cutter-location profile from
/// passing below the true height by more than `eps`: probes the chord (the middle, and the
/// quarter points when the heights differ much) and splits at the middle where it is too low.
#[allow(clippy::too_many_arguments)]
fn refine_chord(
    sample: &mut dyn FnMut(f64) -> f64,
    a: (f64, f64),
    b: (f64, f64),
    eps: f64,
    depth: usize,
    samples: &mut usize,
    max_z: &mut f64,
    out: &mut Vec<(f64, f64)>,
) {
    if depth >= MAX_REFINE_DEPTH || *samples > MAX_SAMPLES || (b.0 - a.0).abs() < 1e-6 {
        return;
    }
    let mid = 0.5 * (a.0 + b.0);
    let zm = sample(mid);
    *samples += 1;
    let chord = |t: f64| a.1 + (b.1 - a.1) * t;
    let mut low = zm > chord(0.5) + eps;
    let (du, dz) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    if !low && dz > 20.0 * eps {
        // A steep chord may span a wall (a step in the height): probe it close to both ends
        // as well, where a step hides longest (halving distances down to 1/128).
        const PROBES: [f64; 12] =
            [0.25, 0.75, 0.125, 0.875, 0.0625, 0.9375, 0.03125, 0.96875, 0.015625, 0.984375, 0.0078125, 0.9921875];
        let n = if dz > du { PROBES.len() } else { 2 };
        for &t in PROBES.iter().take(n) {
            let z = sample(a.0 + (b.0 - a.0) * t);
            *samples += 1;
            if z > chord(t) + eps {
                low = true;
                break;
            }
        }
    }
    if low {
        let m = (mid, zm);
        *max_z = max_z.max(zm);
        refine_chord(sample, a, m, eps, depth + 1, samples, max_z, out);
        out.push(m);
        refine_chord(sample, m, b, eps, depth + 1, samples, max_z, out);
    }
}

/// Most runs ordered by nearest neighbour (quadratic).
const MAX_ORDERED_RUNS: usize = 6000;

/// Orders runs greedily by the distance from the end of the last one to either end of the next
/// (a run may be reversed), starting with the first.
fn order_runs(mut runs: Vec<Vec<[f64; 3]>>) -> Vec<Vec<[f64; 3]>> {
    let mut out = Vec::with_capacity(runs.len());
    if runs.is_empty() {
        return out;
    }
    let mut cur = runs.remove(0);
    loop {
        let end = cur.last().copied();
        out.push(cur);
        let Some(e) = end else { break };
        let d2 = |q: &[f64; 3]| (q[0] - e[0]) * (q[0] - e[0]) + (q[1] - e[1]) * (q[1] - e[1]);
        let mut best: Option<(f64, usize, bool)> = None;
        for (k, r) in runs.iter().enumerate() {
            for (p, rev) in [(r.first(), false), (r.last(), true)] {
                if let Some(p) = p {
                    let d = d2(p);
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, k, rev));
                    }
                }
            }
        }
        let Some((_, k, rev)) = best else { break };
        cur = runs.swap_remove(k);
        if rev {
            cur.reverse();
        }
    }
    out
}

/// Keeps the points of a polyline `(u, z)` that matter within `tol` (Douglas-Peucker).
pub(super) fn simplify(pts: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    simplify_impl(pts, tol, false)
}

/// Like `simplify` for a cutter-location profile `(u, z)`: a point that lies above the chord
/// that would replace it by more than `tol` is kept as well (the chord would gouge there).
fn simplify_cl(pts: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    simplify_impl(pts, tol, true)
}

fn simplify_impl(pts: &[(f64, f64)], tol: f64, rise: bool) -> Vec<(f64, f64)> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut stack = vec![(0usize, n - 1)];
    while let Some((i, j)) = stack.pop() {
        let (Some(&(u0, z0)), Some(&(u1, z1))) = (pts.get(i), pts.get(j)) else { continue };
        let len = libm::hypot(u1 - u0, z1 - z0);
        let mut worst = (0.0, None);
        for k in i + 1..j {
            let Some(&(u, z)) = pts.get(k) else { continue };
            let mut dev = if len < 1e-12 {
                libm::hypot(u - u0, z - z0)
            } else {
                ((u1 - u0) * (z0 - z) - (u0 - u) * (z1 - z0)).abs() / len
            };
            if rise && (u1 - u0).abs() > 1e-12 {
                let chord = z0 + (z1 - z0) * (u - u0) / (u1 - u0);
                dev = dev.max(z - chord);
            }
            if dev > worst.0 {
                worst = (dev, Some(k));
            }
        }
        if let (true, Some(k)) = (worst.0 > tol, worst.1) {
            if let Some(f) = keep.get_mut(k) {
                *f = true;
            }
            stack.push((i, k));
            stack.push((k, j));
        }
    }
    pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

/// XY bounds of the mesh vertices; `None` if there are none or one is not finite.
fn mesh_bounds(positions: &[[f32; 3]]) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in positions {
        let v = [f64::from(p[0]), f64::from(p[1])];
        for k in 0..2 {
            if let (Some(l), Some(h), Some(x)) = (lo.get_mut(k), hi.get_mut(k), v.get(k)) {
                *l = l.min(*x);
                *h = h.max(*x);
            }
        }
    }
    lo.iter().chain(&hi).all(|v| v.is_finite()).then_some((lo, hi))
}

/// Raster finishing: parallel lines at `spec.angle_deg` across the XY bounds, the tool tip Z
/// at each sample by drop-cutter against the mesh, never below the stock bottom
/// (`top - depth`), lines linked in a zig-zag at the safe height.
pub fn raster_finish(surface: &Surface3d, cutter: Cutter, spec: &RasterSpec, s: &Setup) -> Result<Toolpath, CamError> {
    raster_impl(surface, cutter, spec, s, None)
}

/// Raster with an optional shallow-only filter: with `Some(deg)` only runs of samples whose
/// slope is below `deg` are cut, and nothing cut is not an error.
fn raster_impl(
    surface: &Surface3d,
    cutter: Cutter,
    spec: &RasterSpec,
    s: &Setup,
    shallow: Option<f64>,
) -> Result<Toolpath, CamError> {
    let fin = |v: f64| v.is_finite();
    if !(fin(s.tool_diameter) && s.tool_diameter > 0.0) {
        return Err(CamError::Invalid("tool diameter must be positive".into()));
    }
    if !(fin(s.depth) && s.depth > 0.0 && fin(s.top) && fin(s.safe)) {
        return Err(CamError::Invalid("depth must be positive; top and safe finite".into()));
    }
    if !(fin(spec.stepover) && spec.stepover > 0.0 && fin(spec.step) && spec.step > 0.0) {
        return Err(CamError::Invalid("stepover and step must be positive".into()));
    }
    if !(fin(spec.angle_deg)
        && fin(spec.stock_to_leave)
        && spec.stock_to_leave >= 0.0
        && fin(spec.tolerance)
        && spec.tolerance >= 0.0)
    {
        return Err(CamError::Invalid(
            "angle, stock to leave and tolerance must be finite, the latter two not negative".into(),
        ));
    }
    let r = s.tool_diameter / 2.0;
    let c = match cutter {
        Cutter::Flat => 0.0,
        Cutter::Ball => r,
        Cutter::Bull { corner_radius } => {
            if !(fin(corner_radius) && (0.0..=r + 1e-12).contains(&corner_radius)) {
                return Err(CamError::Invalid("corner radius must be between 0 and the tool radius".into()));
            }
            corner_radius.min(r)
        }
    };
    let prof = Profile { r, c };
    let (mlo, mhi) =
        mesh_bounds(surface.positions).ok_or_else(|| CamError::Invalid("the surface has no valid vertices".into()))?;
    let (bmin, bmax) = spec.bounds.unwrap_or((mlo, mhi));
    if !(bmin.iter().chain(&bmax).all(|v| fin(*v)) && bmax[0] > bmin[0] && bmax[1] > bmin[1]) {
        return Err(CamError::Invalid("empty or invalid bounds".into()));
    }
    let mut dropper = Dropper::new(surface, prof, mlo, mhi)?;

    let a = spec.angle_deg.to_radians();
    let d = [libm::cos(a), libm::sin(a)];
    let nrm = [-d[1], d[0]];
    let proj = |p: [f64; 2]| p[0] * nrm[0] + p[1] * nrm[1];
    let corners = [[bmin[0], bmin[1]], [bmax[0], bmin[1]], [bmax[0], bmax[1]], [bmin[0], bmax[1]]];
    let omin = corners.iter().map(|p| proj(*p)).fold(f64::INFINITY, f64::min);
    let omax = corners.iter().map(|p| proj(*p)).fold(f64::NEG_INFINITY, f64::max);
    let lines_f = libm::ceil((omax - omin) / spec.stepover) + 1.0;
    if lines_f > MAX_LINES as f64 {
        return Err(CamError::TooLarge("too many raster lines".into()));
    }
    let nlines = lines_f as usize;
    let bottom = s.top - s.depth;
    let mut moves: Vec<Move> = Vec::new();
    let mut samples = 0usize;
    let mut max_z = f64::NEG_INFINITY;
    let mut lines = 0usize;
    let mut n_pts = 0usize;
    let mut out_runs: Vec<Vec<[f64; 3]>> = Vec::new();
    for li in 0..nlines {
        let o = (omin + li as f64 * spec.stepover).min(omax);
        let q = [nrm[0] * o, nrm[1] * o];
        // Clip the line to the box.
        let (mut u0, mut u1) = (f64::NEG_INFINITY, f64::INFINITY);
        let mut inside = true;
        for (((&qk, &dk), &lo), &hi) in q.iter().zip(&d).zip(&bmin).zip(&bmax) {
            if dk.abs() < 1e-12 {
                inside &= qk >= lo - 1e-9 && qk <= hi + 1e-9;
            } else {
                let (t0, t1) = ((lo - qk) / dk, (hi - qk) / dk);
                u0 = u0.max(t0.min(t1));
                u1 = u1.min(t0.max(t1));
            }
        }
        if !inside || u1 - u0 < 1e-9 {
            continue;
        }
        let nseg = libm::ceil((u1 - u0) / spec.step).max(1.0);
        samples += nseg as usize + 1;
        if samples > MAX_SAMPLES {
            return Err(CamError::TooLarge("too many samples; raise the step or stepover".into()));
        }
        let mut pts = Vec::with_capacity(nseg as usize + 1);
        for k in 0..=(nseg as usize) {
            let u = if k == nseg as usize { u1 } else { u0 + (u1 - u0) * k as f64 / nseg };
            let (x, y) = (q[0] + d[0] * u, q[1] + d[1] * u);
            let z = dropper.drop(x, y).map_or(bottom, |z| (z + spec.stock_to_leave).max(bottom));
            max_z = max_z.max(z);
            pts.push((u, z));
        }
        // Between two samples the chord can pass below the true cutter-location height (a steep
        // wall, a fillet): subdivide where it does.
        let mut sample = |u: f64| {
            let (x, y) = (q[0] + d[0] * u, q[1] + d[1] * u);
            dropper.drop(x, y).map_or(bottom, |z| (z + spec.stock_to_leave).max(bottom))
        };
        let eps = (spec.tolerance * 0.5).clamp(1e-4, 2e-3);
        let mut refined: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
        for w in pts.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
            refined.push(a);
            refine_chord(&mut sample, a, b, eps, 0, &mut samples, &mut max_z, &mut refined);
            if samples > MAX_SAMPLES {
                return Err(CamError::TooLarge("too many samples; raise the step or stepover".into()));
            }
        }
        if let Some(&l) = pts.last() {
            refined.push(l);
        }
        let pts = refined;
        let mut runs: Vec<Vec<(f64, f64)>> = match shallow {
            None => vec![pts],
            Some(deg) => {
                let mut runs = Vec::new();
                let mut cur: Vec<(f64, f64)> = Vec::new();
                for &(u, z) in &pts {
                    let (x, y) = (q[0] + d[0] * u, q[1] + d[1] * u);
                    let sl = waterline::slope_deg(&mut dropper, [x, y], prof.r / 4.0, bottom, spec.stock_to_leave);
                    if sl < deg {
                        cur.push((u, z));
                    } else if !cur.is_empty() {
                        runs.push(std::mem::take(&mut cur));
                    }
                }
                runs.push(cur);
                runs.retain(|r| r.len() >= 2);
                runs
            }
        };
        if lines % 2 == 1 {
            runs.reverse();
        }
        let mut emitted = false;
        for run in runs {
            let mut pts = simplify_cl(&run, spec.tolerance);
            if lines % 2 == 1 {
                pts.reverse();
            }
            let xyz = |&(u, z): &(f64, f64)| [q[0] + d[0] * u, q[1] + d[1] * u, z];
            if pts.is_empty() {
                continue;
            }
            n_pts += pts.len() + 3;
            out_runs.push(pts.iter().map(xyz).collect());
            emitted = true;
        }
        if emitted {
            lines += 1;
        }
        if n_pts > MAX_MOVES {
            return Err(CamError::TooLarge("too many moves".into()));
        }
    }
    // Shallow runs are scattered over the part: visit them nearest first (either end) rather than
    // line by line, which cuts the rapid travel; full rasters keep their zig-zag.
    if shallow.is_some() && out_runs.len() <= MAX_ORDERED_RUNS {
        out_runs = order_runs(out_runs);
    }
    for run in &out_runs {
        let Some(&first) = run.first() else { continue };
        // Link: up and across at the safe height, then down to just above the first point.
        if let Some(Move::Line { to, .. }) = moves.last() {
            moves.push(Move::Rapid([to[0], to[1], s.safe]));
        }
        moves.push(Move::Rapid([first[0], first[1], s.safe]));
        moves.push(Move::Rapid([first[0], first[1], s.safe.min(first[2] + 1.0)]));
        moves.push(Move::Line { to: first, plunge: true });
        for p in run.iter().skip(1) {
            moves.push(Move::Line { to: *p, plunge: false });
        }
    }
    if moves.is_empty() && shallow.is_none() {
        return Err(CamError::NothingToCut("no raster line crosses the bounds".into()));
    }
    if max_z > s.safe {
        return Err(CamError::Invalid("the safe height is below the surface".into()));
    }
    Ok(Toolpath { moves, rings: lines, levels: vec![bottom], drill: None })
}

mod waterline;
pub use waterline::{DEFAULT_STEEP_DEG, SteepShallowSpec, WaterlineSpec, steep_shallow_finish, waterline_finish};
mod holder;
mod pencil;
pub use holder::{
    Holder, HolderCollision, HolderPart, HolderReport, SafeRaster, check_holder, lift_for_holder, raster_finish_safe,
};
pub use pencil::{PencilSpec, pencil_finish};

#[cfg(test)]
mod tests;
