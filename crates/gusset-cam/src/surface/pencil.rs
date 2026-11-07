//! Pencil finishing: a ball cutter traced along concave creases of the surface (ADR 0040,
//! "Pencil finishing" amendment).

use std::collections::HashMap;

use super::{Cutter, Dropper, Profile, Surface3d, mesh_bounds};
use crate::{CamError, MAX_MOVES, Move, Setup, Toolpath};

/// Most grid nodes of the cutter-location field.
const MAX_NODES: usize = 3_000_000;
/// Most refined crease points.
const MAX_POINTS: usize = 500_000;
/// Fine samples per grid step used to locate a crease.
const FINE: usize = 16;

/// Pencil finishing parameters (mm, degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PencilSpec {
    /// Smallest angle between the two surfaces the ball touches (the turning angle of the
    /// cutter-location profile across the crease) that gets a pencil pass.
    pub bitangency_angle_deg: f64,
    /// Chains of crease points are simplified within this distance.
    pub tolerance: f64,
    /// XY box `(min, max)` searched; `None` is the mesh's XY bounds.
    pub bounds: Option<([f64; 2], [f64; 2])>,
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    libm::sqrt((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]) + (a[2] - b[2]) * (a[2] - b[2]))
}

/// A uniform XY bucket index over a point list.
struct Buckets {
    cell: f64,
    map: HashMap<(i64, i64), Vec<usize>>,
}

impl Buckets {
    fn new(cell: f64) -> Self {
        Buckets { cell, map: HashMap::new() }
    }
    fn key(&self, p: [f64; 3]) -> (i64, i64) {
        (libm::floor(p[0] / self.cell) as i64, libm::floor(p[1] / self.cell) as i64)
    }
    fn insert(&mut self, p: [f64; 3], idx: usize) {
        let k = self.key(p);
        self.map.entry(k).or_default().push(idx);
    }
    fn near(&self, p: [f64; 3]) -> impl Iterator<Item = usize> + '_ {
        let (kx, ky) = self.key(p);
        (-1..=1)
            .flat_map(move |dx| (-1..=1).map(move |dy| (kx + dx, ky + dy)))
            .flat_map(move |k| self.map.get(&k).into_iter().flat_map(|v| v.iter().copied()))
    }
}

/// Douglas-Peucker in 3D.
fn simplify3(pts: &[[f64; 3]], tol: f64) -> Vec<[f64; 3]> {
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
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get(j)) else { continue };
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let mut worst = (0.0, None);
        for k in i + 1..j {
            let Some(&p) = pts.get(k) else { continue };
            let w = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
            let t = if dd < 1e-24 { 0.0 } else { ((w[0] * d[0] + w[1] * d[1] + w[2] * d[2]) / dd).clamp(0.0, 1.0) };
            let q = [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]];
            let dev = dist3(p, q);
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

/// Pencil finishing with a ball cutter: traces the concave creases where the ball touches two
/// surfaces at once, as cutting moves at the exact cutter-location height.
///
/// 1. The cutter-location (CL) field, the drop-cutter height on a grid of `r / 8` (`r` the tool
///    radius), is scanned in four directions for nodes where the CL profile turns upward (a
///    valley) by at least `bitangency_angle_deg` between the chords to the neighbours. Smooth
///    valleys (a fillet larger than the ball), ridges and flat areas do not turn sharply enough.
/// 2. Around each such node the profile is sampled at 1/16 of the grid step; the straight pieces
///    on either side of the kink are intersected, which places the crease to rounding for
///    planar facets, and the CL height there is a fresh drop-cutter.
/// 3. Points are merged (within half a grid step), chained by nearest neighbour (links up to
///    three grid steps, in 3D), single points dropped, chains simplified within `tolerance` and
///    emitted as `Line` moves, linked at the safe height.
///
/// A step in the crease direction smaller than the grid step is not resolved; creases close to
/// the mesh boundary (less than one grid step) and nodes with no surface within the tool radius
/// are skipped. Only `Cutter::Ball` is supported. No crease found is `NothingToCut`; more than
/// 3 million grid nodes or 500 000 crease points is `TooLarge`.
pub fn pencil_finish(surface: &Surface3d, cutter: Cutter, spec: &PencilSpec, s: &Setup) -> Result<Toolpath, CamError> {
    let fin = |v: f64| v.is_finite();
    if cutter != Cutter::Ball {
        return Err(CamError::Invalid("pencil finishing needs a ball cutter".into()));
    }
    if !(fin(s.tool_diameter) && s.tool_diameter > 0.0) {
        return Err(CamError::Invalid("tool diameter must be positive".into()));
    }
    if !(fin(s.depth) && s.depth > 0.0 && fin(s.top) && fin(s.safe)) {
        return Err(CamError::Invalid("depth must be positive; top and safe finite".into()));
    }
    if !(fin(spec.bitangency_angle_deg) && spec.bitangency_angle_deg > 0.0 && spec.bitangency_angle_deg < 180.0) {
        return Err(CamError::Invalid("bitangency angle must be between 0 and 180 degrees".into()));
    }
    if !(fin(spec.tolerance) && spec.tolerance >= 0.0) {
        return Err(CamError::Invalid("tolerance must be finite and not negative".into()));
    }
    let r = s.tool_diameter / 2.0;
    let (mlo, mhi) =
        mesh_bounds(surface.positions).ok_or_else(|| CamError::Invalid("the surface has no valid vertices".into()))?;
    let (bmin, bmax) = spec.bounds.unwrap_or((mlo, mhi));
    if !(bmin.iter().chain(&bmax).all(|v| fin(*v)) && bmax[0] > bmin[0] && bmax[1] > bmin[1]) {
        return Err(CamError::Invalid("empty or invalid bounds".into()));
    }
    let mut dropper = Dropper::new(surface, Profile { r, c: r }, mlo, mhi)?;

    let h = r / 8.0;
    let nxf = libm::floor((bmax[0] - bmin[0]) / h) + 1.0;
    let nyf = libm::floor((bmax[1] - bmin[1]) / h) + 1.0;
    if nxf * nyf > MAX_NODES as f64 {
        return Err(CamError::TooLarge("too many grid nodes; use a larger tool or smaller bounds".into()));
    }
    let (nx, ny) = (nxf as usize, nyf as usize);
    let node = |i: usize, j: usize| [bmin[0] + h * i as f64, bmin[1] + h * j as f64];
    let mut field = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            let [x, y] = node(i, j);
            field.push(dropper.drop(x, y).unwrap_or(f64::NAN));
        }
    }
    let at = |i: usize, j: usize| field.get(j * nx + i).copied().unwrap_or(f64::NAN);
    let ang = spec.bitangency_angle_deg.to_radians();
    let bottom = s.top - s.depth;

    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut dedup = Buckets::new(h);
    for j in 1..ny.saturating_sub(1) {
        for i in 1..nx.saturating_sub(1) {
            let f0 = at(i, j);
            // Best direction: the largest upward turn of the CL profile.
            let mut best: Option<(f64, [f64; 2])> = None;
            for (di, dj) in [(1i64, 0i64), (0, 1), (1, 1), (1, -1)] {
                let (ip, jp) = ((i as i64 + di) as usize, (j as i64 + dj) as usize);
                let (im, jm) = ((i as i64 - di) as usize, (j as i64 - dj) as usize);
                let (fp, fm) = (at(ip, jp), at(im, jm));
                if !(f0.is_finite() && fp.is_finite() && fm.is_finite()) {
                    continue;
                }
                let sp = h * libm::hypot(di as f64, dj as f64);
                let turn = libm::atan((fp - f0) / sp) - libm::atan((f0 - fm) / sp);
                if turn >= ang && best.is_none_or(|(t, _)| turn > t) {
                    let l = libm::hypot(di as f64, dj as f64);
                    best = Some((turn, [di as f64 / l, dj as f64 / l]));
                }
            }
            let Some((_, e)) = best else { continue };
            let [cx, cy] = node(i, j);
            // Fine profile along `e` over -2h ..= 2h.
            let dt = h / FINE as f64;
            let m = 4 * FINE;
            let mut prof = Vec::with_capacity(m + 1);
            for k in 0..=m {
                let t = (k as f64 - (m / 2) as f64) * dt;
                match dropper.drop(cx + e[0] * t, cy + e[1] * t) {
                    Some(z) => prof.push(z),
                    None => break,
                }
            }
            if prof.len() != m + 1 {
                continue;
            }
            let g = |k: usize| prof.get(k).copied().unwrap_or(f64::NAN);
            // The kink: largest second difference within one grid step of the node.
            let mut kink: Option<(f64, usize)> = None;
            for k in m / 2 - FINE..=m / 2 + FINE {
                let d2 = g(k - 1) - 2.0 * g(k) + g(k + 1);
                if d2 > 1e-9 && kink.is_none_or(|(b, _)| d2 > b) {
                    kink = Some((d2, k));
                }
            }
            let Some((_, k)) = kink else { continue };
            let tk = |k: usize| (k as f64 - (m / 2) as f64) * dt;
            let sl = (g(k - 2) - g(k - 3)) / dt;
            let sr = (g(k + 3) - g(k + 2)) / dt;
            if libm::atan(sr) - libm::atan(sl) < ang {
                continue;
            }
            let t = ((g(k + 2) - sr * tk(k + 2)) - (g(k - 2) - sl * tk(k - 2))) / (sl - sr);
            if !(t.is_finite() && t >= tk(k - 1) - dt && t <= tk(k + 1) + dt) {
                continue;
            }
            let (x, y) = (cx + e[0] * t, cy + e[1] * t);
            if x < bmin[0] || x > bmax[0] || y < bmin[1] || y > bmax[1] {
                continue;
            }
            let Some(z) = dropper.drop(x, y) else { continue };
            if z < bottom {
                continue;
            }
            let p = [x, y, z];
            if dedup.near(p).any(|q| pts.get(q).is_some_and(|o| libm::hypot(o[0] - x, o[1] - y) < h / 2.0)) {
                continue;
            }
            dedup.insert(p, pts.len());
            pts.push(p);
            if pts.len() > MAX_POINTS {
                return Err(CamError::TooLarge("too many crease points; raise the bitangency angle".into()));
            }
        }
    }
    if pts.is_empty() {
        return Err(CamError::NothingToCut("no concave crease sharper than the bitangency angle".into()));
    }

    // Chain by nearest neighbour.
    let link = 3.0 * h;
    let mut idx = Buckets::new(link);
    for (n, p) in pts.iter().enumerate() {
        idx.insert(*p, n);
    }
    let mut used = vec![false; pts.len()];
    let mut chains: Vec<Vec<[f64; 3]>> = Vec::new();
    for start in 0..pts.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let Some(&p0) = pts.get(start) else { continue };
        let grow = |from: [f64; 3], used: &mut [bool]| -> Vec<[f64; 3]> {
            let mut out = Vec::new();
            let mut cur = from;
            loop {
                let next = idx
                    .near(cur)
                    .filter(|&q| !used.get(q).copied().unwrap_or(true))
                    .filter_map(|q| pts.get(q).map(|p| (q, *p, dist3(cur, *p))))
                    .filter(|(_, _, d)| *d <= link)
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                let Some((q, p, _)) = next else { break };
                if let Some(u) = used.get_mut(q) {
                    *u = true;
                }
                out.push(p);
                cur = p;
            }
            out
        };
        let fwd = grow(p0, &mut used);
        let mut back = grow(p0, &mut used);
        back.reverse();
        let mut chain = back;
        chain.push(p0);
        chain.extend(fwd);
        if chain.len() >= 2 {
            chains.push(simplify3(&chain, spec.tolerance));
        }
    }
    if chains.is_empty() {
        return Err(CamError::NothingToCut("only isolated crease points found".into()));
    }

    let mut moves: Vec<Move> = Vec::new();
    let mut max_z = f64::NEG_INFINITY;
    for chain in &chains {
        let Some(&first) = chain.first() else { continue };
        if let Some(Move::Line { to, .. }) = moves.last() {
            moves.push(Move::Rapid([to[0], to[1], s.safe]));
        }
        moves.push(Move::Rapid([first[0], first[1], s.safe]));
        moves.push(Move::Rapid([first[0], first[1], s.safe.min(first[2] + 1.0)]));
        moves.push(Move::Line { to: first, plunge: true });
        for p in chain.iter().skip(1) {
            moves.push(Move::Line { to: *p, plunge: false });
        }
        max_z = chain.iter().map(|p| p[2]).fold(max_z, f64::max);
        if moves.len() > MAX_MOVES {
            return Err(CamError::TooLarge("too many moves".into()));
        }
    }
    if max_z > s.safe {
        return Err(CamError::Invalid("the safe height is below the surface".into()));
    }
    Ok(Toolpath { moves, rings: chains.len(), levels: vec![bottom], drill: None })
}

#[cfg(test)]
mod tests;
