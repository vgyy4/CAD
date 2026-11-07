//! Waterline (constant-Z) and steep/shallow finishing (ADR 0040, "3D finishing" amendment).

use super::{
    CamError, Cutter, Dropper, MAX_SAMPLES, Move, Profile, RasterSpec, Setup, Surface3d, Toolpath, mesh_bounds,
    raster_impl, simplify,
};
use crate::MAX_MOVES;
use std::collections::BTreeMap;

/// Slope (degrees) above which a surface counts as steep unless the spec says otherwise.
pub const DEFAULT_STEEP_DEG: f64 = 45.0;
/// Most Z levels in one waterline job.
const MAX_LEVELS: f64 = 10_000.0;
/// Bisection steps that place a contour point on a grid edge.
const BISECT: usize = 14;

/// Waterline finishing parameters (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterlineSpec {
    /// Distance between Z levels, from the setup's top down to its bottom (`top - depth`).
    pub stepdown: f64,
    /// Contour points that deviate less than this from the line through their neighbours are
    /// dropped; it also sets the sampling grid (cell about `sqrt(8 R tolerance)`, clamped to
    /// `R/16 ..= R/2`).
    pub tolerance: f64,
    /// XY box the tool tip stays in; `None` is the mesh's XY bounds grown by the tool radius and a grid cell
    /// (so the tool is clear of the mesh at the box edge and every contour closes).
    /// With an explicit box a contour that leaves it is cut as open runs ending on the box edge (never
    /// along it); the box limits the tool centre and is not grown.
    pub bounds: Option<([f64; 2], [f64; 2])>,
    /// Material left on the surface, as a rise in Z.
    pub stock_to_leave: f64,
}

/// Steep/shallow finishing: waterlines where the surface is steeper than `steep_deg`, raster
/// elsewhere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteepShallowSpec {
    pub waterline: WaterlineSpec,
    pub raster: RasterSpec,
    /// Slope angle from horizontal, in degrees (0 .. 90), at and above which a point is steep.
    pub steep_deg: f64,
}

impl SteepShallowSpec {
    /// With the default 45 degree threshold.
    pub fn new(waterline: WaterlineSpec, raster: RasterSpec) -> Self {
        SteepShallowSpec { waterline, raster, steep_deg: DEFAULT_STEEP_DEG }
    }
}

/// Slope angle (degrees from horizontal) of the cutter-location field at `p`, by central
/// differences `delta` apart. The field is the raster's Z (plus `stock`, never below `bottom`),
/// so a wall or the rim of the mesh reads as steep.
pub(super) fn slope_deg(d: &mut Dropper, p: [f64; 2], delta: f64, bottom: f64, stock: f64) -> f64 {
    let mut f = |x: f64, y: f64| d.drop(x, y).map_or(bottom, |z| (z + stock).max(bottom));
    let gx = (f(p[0] + delta, p[1]) - f(p[0] - delta, p[1])) / (2.0 * delta);
    let gy = (f(p[0], p[1] + delta) - f(p[0], p[1] - delta)) / (2.0 * delta);
    libm::atan(libm::hypot(gx, gy)).to_degrees()
}

fn common(surface: &Surface3d, cutter: Cutter, s: &Setup) -> Result<(Profile, [f64; 2], [f64; 2]), CamError> {
    let fin = |v: f64| v.is_finite();
    if !(fin(s.tool_diameter) && s.tool_diameter > 0.0) {
        return Err(CamError::Invalid("tool diameter must be positive".into()));
    }
    if !(fin(s.depth) && s.depth > 0.0 && fin(s.top) && fin(s.safe)) {
        return Err(CamError::Invalid("depth must be positive; top and safe finite".into()));
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
    let (lo, hi) =
        mesh_bounds(surface.positions).ok_or_else(|| CamError::Invalid("the surface has no valid vertices".into()))?;
    Ok((Profile { r, c }, lo, hi))
}

/// The sampling grid of the cutter-location field and what is needed to evaluate it.
struct Grid {
    nx: usize,
    ny: usize,
    g0: [f64; 2],
    h: f64,
    bmin: [f64; 2],
    bmax: [f64; 2],
    stock: f64,
}

impl Grid {
    fn pos(&self, i: usize, j: usize) -> (f64, f64) {
        (self.g0[0] + i as f64 * self.h, self.g0[1] + j as f64 * self.h)
    }

    fn in_box(&self, x: f64, y: f64) -> bool {
        let e = 1e-9;
        x >= self.bmin[0] - e && x <= self.bmax[0] + e && y >= self.bmin[1] - e && y <= self.bmax[1] + e
    }

    /// Whether the grid edge `key` has one end in the box and one out of it.
    fn on_box_edge(&self, key: usize) -> bool {
        let (a, b) = self.ends(key);
        self.in_box(a.0, a.1) != self.in_box(b.0, b.1)
    }

    /// Cutter-location Z, `-inf` where the tool is free of the mesh or out of the box.
    fn cl(&self, d: &mut Dropper, x: f64, y: f64) -> f64 {
        if !self.in_box(x, y) {
            return f64::NEG_INFINITY;
        }
        d.drop(x, y).map_or(f64::NEG_INFINITY, |z| z + self.stock)
    }

    /// Edge keys: kind 0 joins node `(i, j)` to `(i+1, j)`, kind 1 `(i, j)` to `(i, j+1)`.
    fn key(&self, kind: usize, i: usize, j: usize) -> usize {
        (kind * self.ny + j) * self.nx + i
    }

    fn ends(&self, key: usize) -> ((f64, f64), (f64, f64)) {
        let i = key % self.nx;
        let rest = key / self.nx;
        let (j, kind) = (rest % self.ny, rest / self.ny);
        let (di, dj) = if kind == 0 { (1, 0) } else { (0, 1) };
        (self.pos(i, j), self.pos(i + di, j + dj))
    }
}

/// A piece of contour: a closed loop (first point not repeated) or an open run.
struct Piece {
    pts: Vec<(f64, f64)>,
    closed: bool,
}

/// Contours of the CL field at `level`, material (CL above the level) on the right of travel.
/// With `steep = Some((deg, delta, bottom))` only the steep parts are kept.
fn contours(g: &Grid, d: &mut Dropper, vals: &[f64], level: f64, steep: Option<(f64, f64, f64)>) -> Vec<Piece> {
    let ins: Vec<bool> = vals.iter().map(|&v| v > level + 1e-9).collect();
    let flag = |i: usize, j: usize| ins.get(j * g.nx + i).copied().unwrap_or(false);
    let mut next: BTreeMap<usize, usize> = BTreeMap::new();
    for j in 0..g.ny.saturating_sub(1) {
        for i in 0..g.nx.saturating_sub(1) {
            // Corners and edges counter-clockwise from the lower left.
            let [f0, f1, f2, f3] = [flag(i, j), flag(i + 1, j), flag(i + 1, j + 1), flag(i, j + 1)];
            let keys = [g.key(0, i, j), g.key(1, i + 1, j), g.key(0, i, j + 1), g.key(1, i, j)];
            let pairs = [(f0, f1), (f1, f2), (f2, f3), (f3, f0)];
            // Crossing edges in order, with whether the walk enters the material there.
            let cross: Vec<(usize, bool)> =
                pairs.iter().zip(keys).filter(|((a, b), _)| a != b).map(|((a, _), k)| (k, !*a)).collect();
            for (n, &(from, enters)) in cross.iter().enumerate() {
                if enters && let Some(&(to, _)) = cross.get((n + 1) % cross.len()) {
                    next.insert(from, to);
                }
            }
        }
    }
    let mut pieces = Vec::new();
    while let Some((&start, _)) = next.iter().next() {
        let mut chain = vec![start];
        let mut cur = start;
        let mut closed = false;
        while let Some(n) = next.remove(&cur) {
            if n == start {
                closed = true;
                break;
            }
            chain.push(n);
            cur = n;
        }
        let pts: Vec<(f64, f64)> = chain.iter().map(|&k| edge_point(g, d, level, k)).collect();
        // Where the contour leaves the box the field edge crosses the box, and the contour only
        // closes along the box. Those points are not on the part's contour: drop them and the
        // steps next to them, which leaves open runs (never a cut along the box).
        let n = pts.len();
        let bad: Vec<bool> = chain.iter().map(|&k| g.on_box_edge(k)).collect();
        let mut parts: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();
        if let Some(k0) = bad.iter().position(|&b| b) {
            let finish = |mut run: Vec<(f64, f64)>, parts: &mut Vec<(Vec<(f64, f64)>, bool)>| {
                // The run ends where the contour meets the box: carry both ends on, along the
                // contour's own direction, to the box edge (at most two cells).
                if run.len() >= 2 {
                    let first = extend_to_box(g, run.get(1).copied(), run.first().copied());
                    let last = extend_to_box(g, run.iter().rev().nth(1).copied(), run.last().copied());
                    if let Some(p) = first {
                        run.insert(0, p);
                    }
                    if let Some(p) = last {
                        run.push(p);
                    }
                    parts.push((run, false));
                }
            };
            let mut run = Vec::new();
            for m in 1..=n {
                let idx = (k0 + m) % n;
                match (bad.get(idx), pts.get(idx)) {
                    (Some(false), Some(&p)) => run.push(p),
                    _ => finish(std::mem::take(&mut run), &mut parts),
                }
            }
        } else {
            parts.push((pts, closed));
        }
        for (pts, closed) in parts {
            match steep {
                None => pieces.push(Piece { pts, closed }),
                Some((deg, delta, bottom)) => {
                    let st: Vec<bool> =
                        pts.iter().map(|&(x, y)| slope_deg(d, [x, y], delta, bottom, g.stock) >= deg).collect();
                    split_steep(&pts, &st, closed, &mut pieces);
                }
            }
        }
    }
    pieces
}

/// The point where the ray from `from` through `end` (the contour's last step) leaves the box,
/// if that is within two grid cells beyond `end` and not already there.
fn extend_to_box(g: &Grid, from: Option<(f64, f64)>, end: Option<(f64, f64)>) -> Option<(f64, f64)> {
    let (f, e) = (from?, end?);
    let (dx, dy) = (e.0 - f.0, e.1 - f.1);
    let len = libm::hypot(dx, dy);
    if len < 1e-12 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let mut t = f64::INFINITY;
    for (p, u, lo, hi) in [(e.0, ux, g.bmin[0], g.bmax[0]), (e.1, uy, g.bmin[1], g.bmax[1])] {
        if u > 1e-12 {
            t = t.min((hi - p) / u);
        } else if u < -1e-12 {
            t = t.min((lo - p) / u);
        }
    }
    (t.is_finite() && t > 1e-6 && t <= 2.0 * g.h).then_some((e.0 + ux * t, e.1 + uy * t))
}

/// Keeps the steep runs of a contour: all of it if every point is steep.
fn split_steep(pts: &[(f64, f64)], st: &[bool], closed: bool, out: &mut Vec<Piece>) {
    if st.iter().all(|&b| b) {
        out.push(Piece { pts: pts.to_vec(), closed });
        return;
    }
    // Start a closed contour at a point that is not steep so no run wraps around.
    let start = if closed { st.iter().position(|&b| !b).unwrap_or(0) } else { 0 };
    let mut run: Vec<(f64, f64)> = Vec::new();
    let n = pts.len();
    let mut found: Vec<Piece> = Vec::new();
    for k in 0..n {
        let idx = (start + k) % n.max(1);
        let (Some(&p), Some(&s)) = (pts.get(idx), st.get(idx)) else { continue };
        if s {
            run.push(p);
        } else if !run.is_empty() {
            found.push(Piece { pts: std::mem::take(&mut run), closed: false });
        }
    }
    if !run.is_empty() {
        found.push(Piece { pts: run, closed: false });
    }
    out.extend(found.into_iter().filter(|p| p.pts.len() >= 2));
}

/// Where the contour crosses an edge, by bisection on the true CL field (the free side).
fn edge_point(g: &Grid, d: &mut Dropper, level: f64, key: usize) -> (f64, f64) {
    let (mut a, mut b) = g.ends(key);
    let mut inside = |p: (f64, f64)| g.cl(d, p.0, p.1) > level + 1e-9;
    let sa = inside(a);
    for _ in 0..BISECT {
        let m = (0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
        if inside(m) == sa {
            a = m;
        } else {
            b = m;
        }
    }
    // The end that is clear of the mesh: a jump in the field (a wall) must not leave the point
    // on the gouging side.
    let p = if sa { b } else { a };
    // A point found at the box edge stays in the box.
    (p.0.clamp(g.bmin[0], g.bmax[0]), p.1.clamp(g.bmin[1], g.bmax[1]))
}

fn link_to(moves: &mut Vec<Move>, pts: &[(f64, f64)], z: f64, safe: f64) {
    let Some(&(x0, y0)) = pts.first() else { return };
    if let Some(Move::Line { to, .. }) = moves.last() {
        moves.push(Move::Rapid([to[0], to[1], safe]));
    }
    moves.push(Move::Rapid([x0, y0, safe]));
    moves.push(Move::Rapid([x0, y0, safe.min(z + 1.0)]));
    moves.push(Move::Line { to: [x0, y0, z], plunge: true });
    for &(x, y) in pts.iter().skip(1) {
        moves.push(Move::Line { to: [x, y, z], plunge: false });
    }
}

struct Waterlines {
    moves: Vec<Move>,
    levels: Vec<f64>,
    pieces: usize,
}

fn waterline_impl(
    surface: &Surface3d,
    cutter: Cutter,
    spec: &WaterlineSpec,
    s: &Setup,
    steep_deg: Option<f64>,
) -> Result<Waterlines, CamError> {
    let fin = |v: f64| v.is_finite();
    if !(fin(spec.stepdown) && spec.stepdown > 0.0) {
        return Err(CamError::Invalid("stepdown must be positive".into()));
    }
    if !(fin(spec.stock_to_leave) && spec.stock_to_leave >= 0.0 && fin(spec.tolerance) && spec.tolerance >= 0.0) {
        return Err(CamError::Invalid("stock to leave and tolerance must be finite and not negative".into()));
    }
    if steep_deg.is_some_and(|a| !(a > 0.0 && a < 90.0)) {
        return Err(CamError::Invalid("the steep angle must be between 0 and 90 degrees".into()));
    }
    let (prof, mlo, mhi) = common(surface, cutter, s)?;
    let r = prof.r;
    let h = libm::sqrt(8.0 * r * spec.tolerance).clamp(r / 16.0, r / 2.0);
    let grow = r + h;
    let (bmin, bmax) = spec.bounds.unwrap_or(([mlo[0] - grow, mlo[1] - grow], [mhi[0] + grow, mhi[1] + grow]));
    if !(bmin.iter().chain(&bmax).all(|v| fin(*v)) && bmax[0] > bmin[0] && bmax[1] > bmin[1]) {
        return Err(CamError::Invalid("empty or invalid bounds".into()));
    }
    let bottom = s.top - s.depth;
    let n_levels = libm::ceil(s.depth / spec.stepdown);
    if n_levels > MAX_LEVELS {
        return Err(CamError::TooLarge("too many levels".into()));
    }
    let nxf = libm::ceil((bmax[0] - bmin[0]) / h) + 3.0;
    let nyf = libm::ceil((bmax[1] - bmin[1]) / h) + 3.0;
    if nxf * nyf > MAX_SAMPLES as f64 {
        return Err(CamError::TooLarge("too many grid samples; raise the tolerance or shrink the bounds".into()));
    }
    let mut dropper = Dropper::new(surface, prof, mlo, mhi)?;
    let g = Grid {
        nx: nxf as usize,
        ny: nyf as usize,
        g0: [bmin[0] - h, bmin[1] - h],
        h,
        bmin,
        bmax,
        stock: spec.stock_to_leave,
    };
    // The field: one layer of nodes around the box is out by construction, so contours close.
    let mut vals = Vec::with_capacity(g.nx * g.ny);
    for j in 0..g.ny {
        for i in 0..g.nx {
            let (x, y) = g.pos(i, j);
            vals.push(g.cl(&mut dropper, x, y));
        }
    }
    let maxv = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if maxv > s.safe {
        return Err(CamError::Invalid("the safe height is below the surface".into()));
    }
    let mut levels: Vec<f64> =
        (0..n_levels as usize).map(|k| s.top - k as f64 * spec.stepdown).take_while(|z| *z > bottom + 1e-9).collect();
    levels.push(bottom);
    let steep = steep_deg.map(|a| (a, r / 4.0, bottom));
    let mut out = Waterlines { moves: Vec::new(), levels: Vec::new(), pieces: 0 };
    for &z in &levels {
        if z >= maxv - 1e-9 {
            continue;
        }
        let mut pieces = contours(&g, &mut dropper, &vals, z, steep);
        if pieces.is_empty() {
            continue;
        }
        if !s.climb {
            // As generated the material is on the right: climb milling.
            for p in &mut pieces {
                p.pts.reverse();
            }
        }
        let mut cur: Option<(f64, f64)> = None;
        let mut any = false;
        while !pieces.is_empty() {
            let mut best = (f64::INFINITY, 0usize, 0usize);
            if let Some(c) = cur {
                for (pi, p) in pieces.iter().enumerate() {
                    let cand: &[(f64, f64)] = if p.closed { &p.pts } else { p.pts.get(..1).unwrap_or(&[]) };
                    for (vi, &(x, y)) in cand.iter().enumerate() {
                        let dd = libm::hypot(x - c.0, y - c.1);
                        if dd < best.0 {
                            best = (dd, pi, vi);
                        }
                    }
                }
            }
            let piece = pieces.swap_remove(best.1);
            let mut pts = piece.pts;
            if piece.closed {
                let k = best.2.min(pts.len());
                pts.rotate_left(k);
                if let Some(&f) = pts.first() {
                    pts.push(f);
                }
            }
            let pts = simplify(&pts, spec.tolerance);
            if pts.len() < 2 {
                continue;
            }
            cur = if piece.closed { pts.first().copied() } else { pts.last().copied() };
            link_to(&mut out.moves, &pts, z, s.safe);
            out.pieces += 1;
            any = true;
            if out.moves.len() > MAX_MOVES {
                return Err(CamError::TooLarge("too many moves".into()));
            }
        }
        if any {
            out.levels.push(z);
        }
    }
    Ok(out)
}

/// Why no contour was found: with an explicit box it limits the tool centre, and the contour
/// lies about one tool radius outside the part.
fn no_contour_message(spec: &WaterlineSpec, s: &Setup) -> String {
    if spec.bounds.is_some() {
        let r = s.tool_diameter / 2.0;
        format!(
            "no waterline contour crosses the surface inside the region: the region limits the tool centre, which \
             stays about one tool radius ({} mm) outside the part's outline, so the contour lies outside the box; \
             grow the region by the tool radius ({} mm) on every side",
            (r * 1000.0).round() / 1000.0,
            (r * 1000.0).round() / 1000.0
        )
    } else {
        "no waterline contour crosses the surface".into()
    }
}

/// Waterline finishing: contours at constant Z, from the top down to the bottom
/// (`top - depth`) every `spec.stepdown`. At each level the tool tip sits at that Z, touching
/// the mesh without gouging it (drop-cutter); the contour of that cutter-location field is
/// found on a fine XY grid by marching squares, with every point placed on its grid edge by
/// bisection on the true field. Loops are simplified within `spec.tolerance` and cut with the
/// material on the right (climb) or left (conventional), nearest loop first, linked at the safe
/// height. Levels without a contour are skipped (`Toolpath::levels` lists the cut ones,
/// `rings` counts loops).
pub fn waterline_finish(
    surface: &Surface3d,
    cutter: Cutter,
    spec: &WaterlineSpec,
    s: &Setup,
) -> Result<Toolpath, CamError> {
    let w = waterline_impl(surface, cutter, spec, s, None)?;
    if w.moves.is_empty() {
        return Err(CamError::NothingToCut(no_contour_message(spec, s)));
    }
    Ok(Toolpath { moves: w.moves, rings: w.pieces, levels: w.levels, drill: None })
}

/// Whether the box is smaller than the mesh's XY bounds grown by the tool radius on some side.
fn box_cuts_into_outline(surface: &Surface3d, s: &Setup, b: ([f64; 2], [f64; 2])) -> bool {
    let Some((lo, hi)) = mesh_bounds(surface.positions) else { return false };
    let r = s.tool_diameter / 2.0;
    (0..2).any(|k| {
        let (l, h, bl, bh) = (lo.get(k), hi.get(k), b.0.get(k), b.1.get(k));
        match (l, h, bl, bh) {
            (Some(l), Some(h), Some(bl), Some(bh)) => *bl > l - r + 1e-9 || *bh < h + r - 1e-9,
            _ => false,
        }
    })
}

/// Steep/shallow finishing: waterline contours where the cutter-location field's slope is at
/// least `spec.steep_deg` (the contour pieces, and only those, that are steep), then raster
/// segments (only runs of samples below that slope).
pub fn steep_shallow_finish(
    surface: &Surface3d,
    cutter: Cutter,
    spec: &SteepShallowSpec,
    s: &Setup,
) -> Result<Toolpath, CamError> {
    let w = waterline_impl(surface, cutter, &spec.waterline, s, Some(spec.steep_deg))?;
    if w.moves.is_empty() && spec.waterline.bounds.is_some_and(|b| box_cuts_into_outline(surface, s, b)) {
        // The steep walls' contours may lie outside the box: the raster alone would leave them
        // uncut without a word, so say so (unless there is no contour at all inside the box).
        let all = waterline_impl(surface, cutter, &spec.waterline, s, None)?;
        if all.moves.is_empty() {
            return Err(CamError::NothingToCut(no_contour_message(&spec.waterline, s)));
        }
    }
    let rt = raster_impl(surface, cutter, &spec.raster, s, Some(spec.steep_deg))?;
    let mut moves = w.moves;
    if let (Some(Move::Line { to, .. }), true) = (moves.last(), !rt.moves.is_empty()) {
        moves.push(Move::Rapid([to[0], to[1], s.safe]));
    }
    moves.extend(rt.moves);
    if moves.is_empty() {
        return Err(CamError::NothingToCut("nothing to cut".into()));
    }
    if moves.len() > MAX_MOVES {
        return Err(CamError::TooLarge("too many moves".into()));
    }
    Ok(Toolpath { moves, rings: w.pieces + rt.rings, levels: w.levels, drill: None })
}
