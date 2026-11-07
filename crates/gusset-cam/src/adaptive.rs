//! Adaptive (constant-engagement) pocket clearing (ADR 0040, "Adaptive clearing").
//!
//! The pocket is cleared by offset rings from the middle out, like `Operation::Pocket`, with
//! two changes that keep the tool's radial engagement at or below the stepover:
//! the rings are spaced closer, so that the sharpest convex corner of the region (where a ring
//! reaches `1 / sin(angle / 2)` times further than the ring inside it) still stays within the
//! stepover; and the innermost ring of each branch, which would otherwise be cut as a
//! full-width slot through untouched stock, is cleared by trochoidal loops along it, entered
//! by a spiral from the plunge. Rings lying wholly inside the cleared band are not cut.

use super::{CamError, MAX_MOVES, MAX_RINGS, Move, Setup, plines_of, segment};
use cavalier_contours::polyline::{PlineSource, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use std::f64::consts::{PI, TAU};

/// A trochoidally cleared slot: loops of radius `rho`, their centres `delta` apart along the
/// closed `path` (the innermost ring of a branch of the pocket).
#[derive(Debug, Clone)]
pub(crate) struct Slot {
    path: Vec<[f64; 2]>,
    rho: f64,
    delta: f64,
}

/// Chord length when an arc is flattened for distance tests and resampling.
const FLAT_CHORD: f64 = 0.2;

fn flatten(p: &Polyline<f64>) -> Vec<[f64; 2]> {
    let n = p.vertex_count();
    let mut out = Vec::new();
    for i in 0..n {
        let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
        out.push([a.x, a.y]);
        if a.bulge.abs() < 1e-12 {
            continue;
        }
        if let Move::Arc { center, .. } = segment(a, b, 0.0) {
            let r = libm::hypot(a.x - center[0], a.y - center[1]);
            let a0 = libm::atan2(a.y - center[1], a.x - center[0]);
            let sweep = 4.0 * libm::atan(a.bulge);
            let k = (r * sweep.abs() / FLAT_CHORD).ceil().max(1.0) as usize;
            for j in 1..k {
                let ang = a0 + sweep * j as f64 / k as f64;
                out.push([center[0] + r * libm::cos(ang), center[1] + r * libm::sin(ang)]);
            }
        }
    }
    out
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 1e-18 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    libm::hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy))
}

/// Distance from `p` to the closed polyline `pts`.
fn closed_dist(p: [f64; 2], pts: &[[f64; 2]]) -> f64 {
    pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| seg_dist(p, *a, *b)).fold(f64::INFINITY, f64::min)
}

/// Points `step` apart along the closed polyline `pts` (the first one included, the start
/// not repeated).
fn resample(pts: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let segs: Vec<([f64; 2], [f64; 2], f64)> = pts
        .iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(a, b)| (*a, *b, libm::hypot(b[0] - a[0], b[1] - a[1])))
        .collect();
    let total: f64 = segs.iter().map(|s| s.2).sum();
    let n = (total / step).floor().max(1.0) as usize;
    let mut out = Vec::with_capacity(n);
    let mut it = segs.iter();
    let mut cur = it.next();
    let mut start = 0.0;
    for k in 0..n {
        let s = k as f64 * step;
        while let Some(c) = cur {
            if start + c.2 < s {
                start += c.2;
                cur = it.next();
            } else {
                break;
            }
        }
        let Some((a, b, l)) = cur else { break };
        let t = if *l > 1e-12 { ((s - start) / l).clamp(0.0, 1.0) } else { 0.0 };
        out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
    out
}

/// How much closer than the stepover the rings must be so that the region's sharpest convex
/// corner does not make a ring reach further than the stepover: `sin(angle / 2)`, between 0.5
/// (60 degrees and sharper corners are not compensated) and 1.
fn corner_factor(base: &Shape<f64>) -> f64 {
    let mut worst: f64 = 1.0;
    for pl in base.ccw_plines.iter().chain(&base.cw_plines).map(|p| &p.polyline) {
        let n = pl.vertex_count();
        for i in 0..n {
            let (Some(a), Some(b), Some(c)) = (pl.get((i + n - 1) % n), pl.get(i), pl.get((i + 1) % n)) else {
                continue;
            };
            if a.bulge.abs() > 1e-12 || b.bulge.abs() > 1e-12 {
                continue;
            }
            let (u, v) = ([b.x - a.x, b.y - a.y], [c.x - b.x, c.y - b.y]);
            let turn = libm::atan2(u[0] * v[1] - u[1] * v[0], u[0] * v[0] + u[1] * v[1]);
            // Left turn: convex for the region (outer loops run counter-clockwise, holes clockwise).
            if turn > 1e-6 {
                worst = worst.min(libm::sin((PI - turn) / 2.0));
            }
        }
    }
    worst.clamp(0.5, 1.0)
}

/// Largest loop advance for loops of radius `rho` so that a loop's leading edge, against the
/// disc cleared by the loop before (radius `r + rho`), engages at most as deep as a straight
/// cut of width `w` would (tool radius `r`): the contact half-angle of the tool circle outside
/// that disc is `acos((2 rho (r - d) - d^2) / (2 r (rho + d)))` for an advance `d`.
fn loop_advance(r: f64, rho: f64, w: f64) -> f64 {
    let cos_max = 1.0 - (w / r).min(2.0);
    let cos_at = |d: f64| (2.0 * rho * (r - d) - d * d) / (2.0 * r * (rho + d));
    if cos_at(w) >= cos_max {
        return w;
    }
    let (mut lo, mut hi) = (0.0, w);
    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        if cos_at(mid) >= cos_max {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The rings (innermost first) and slots of an adaptive pocket.
pub(crate) fn plan(base: &Shape<f64>, stepover: f64, s: &Setup) -> Result<(Vec<Polyline<f64>>, Vec<Slot>), CamError> {
    let opts = ShapeOffsetOptions::default();
    let r = s.tool_diameter / 2.0;
    let w = stepover * s.tool_diameter;
    let f = corner_factor(base);
    let g = w * f;
    let mut all: Vec<Vec<Polyline<f64>>> = Vec::new();
    let mut cur = base.parallel_offset(r, &opts);
    loop {
        let ring = plines_of(&cur);
        if ring.is_empty() {
            break;
        }
        all.push(ring);
        if all.len() > MAX_RINGS {
            return Err(CamError::TooLarge("more than 10000 pocket rings: use a larger stepover".into()));
        }
        cur = cur.parallel_offset(g, &opts);
    }
    let flat: Vec<Vec<Vec<[f64; 2]>>> = all.iter().map(|l| l.iter().map(flatten).collect()).collect();
    // Slots: rings with a stretch that has no ring inside it near (a leaf, or an arm of a
    // shape that narrows) enclose untouched stock the ring itself would slot through; they are
    // cleared by loops if the pocket is wide enough there.
    let mut slots: Vec<(usize, Slot)> = Vec::new();
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for (j, level) in flat.iter().enumerate() {
        for (i, ring) in level.iter().enumerate() {
            let next = flat.get(j + 1).map(Vec::as_slice).unwrap_or_default();
            let bare = ring.iter().any(|p| next.iter().all(|c| closed_dist(*p, c) > 2.2 * g));
            // Every point of the ring is `j g` from the boundary of the tool's region.
            let rho = r.min(j as f64 * g - 0.01);
            if !bare || rho < 0.25 * r {
                continue;
            }
            let delta = loop_advance(r, rho, w * 0.95).max(w * 0.02);
            taken.push((j, i));
            slots.push((j, Slot { path: ring.clone(), rho, delta }));
        }
    }
    // The innermost first.
    slots.reverse();
    // The innermost first; a ring wholly inside a slot's cleared band is left out.
    let mut rings = Vec::new();
    for (j, level) in all.iter().enumerate().rev() {
        for (i, ring) in level.iter().enumerate() {
            if taken.contains(&(j, i)) {
                continue;
            }
            let covered = flat.get(j).and_then(|l| l.get(i)).is_some_and(|pts| {
                slots.iter().any(|(sj, sl)| {
                    sj.saturating_sub(j) as f64 * g <= sl.rho * f + 1e-9
                        && pts.iter().all(|p| closed_dist(*p, &sl.path) <= sl.rho * f)
                })
            });
            if !covered {
                rings.push(ring.clone());
            }
        }
    }
    Ok((rings, slots.into_iter().map(|(_, s)| s).collect()))
}

impl Slot {
    /// The moves clearing the slot at height `z` (entered from `above`): a spiral out from the
    /// plunge, then loops advancing along the path.
    pub(crate) fn moves(&self, above: f64, z: f64, slope: f64, s: &Setup, out: &mut Vec<Move>) -> Result<(), CamError> {
        let centres = resample(&self.path, self.delta);
        let dir_at = |k: usize| -> [f64; 2] {
            let (a, b) = match (centres.get(k), centres.get(k + 1)) {
                (Some(a), Some(b)) => (*a, *b),
                (Some(b), None) => (centres.get(k.wrapping_sub(1)).copied().unwrap_or([b[0] - 1.0, b[1]]), *b),
                _ => ([0.0, 0.0], [1.0, 0.0]),
            };
            let l = libm::hypot(b[0] - a[0], b[1] - a[1]);
            if l > 1e-12 { [(b[0] - a[0]) / l, (b[1] - a[1]) / l] } else { [1.0, 0.0] }
        };
        let Some(c0) = centres.first().copied() else { return Ok(()) };
        if centres.len() as f64 * 6.0 + out.len() as f64 > MAX_MOVES as f64 {
            return Err(CamError::TooLarge("the toolpath has more than 5M moves".into()));
        }
        let rho = self.rho;
        let sign = if s.climb { 1.0 } else { -1.0 };
        let u0 = dir_at(0);
        let rear_angle = libm::atan2(-u0[1], -u0[0]);
        // Entry: plunge at the first centre, spiral out to the first loop's rear point.
        out.push(Move::Rapid([c0[0], c0[1], s.safe]));
        let turns = (rho / self.delta).ceil().max(1.0);
        let n = (turns * 72.0) as usize;
        let mut pts: Vec<[f64; 2]> = (1..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                let ang = rear_angle - sign * TAU * turns * (1.0 - t);
                [c0[0] + rho * t * libm::cos(ang), c0[1] + rho * t * libm::sin(ang)]
            })
            .collect();
        if slope > 0.0 && above - z > 1e-9 {
            out.push(Move::Line { to: [c0[0], c0[1], above], plunge: true });
            let have = pts.iter().fold((c0, 0.0), |(p, l), q| (*q, l + libm::hypot(q[0] - p[0], q[1] - p[1]))).1;
            let need = (above - z) / slope;
            if have < need {
                // Whole extra turns on the loop's circle until the depth is reached.
                let extra = ((need - have) / (TAU * rho)).ceil() as usize;
                for _ in 0..extra {
                    for i in 1..=36 {
                        let ang = rear_angle + sign * TAU * f64::from(i) / 36.0;
                        pts.push([c0[0] + rho * libm::cos(ang), c0[1] + rho * libm::sin(ang)]);
                    }
                }
            }
            let mut run = 0.0;
            let mut prev = c0;
            for q in pts {
                run += libm::hypot(q[0] - prev[0], q[1] - prev[1]);
                prev = q;
                out.push(Move::Line { to: [q[0], q[1], (above - slope * run).max(z)], plunge: false });
            }
        } else {
            out.push(Move::Line { to: [c0[0], c0[1], z], plunge: true });
            for q in pts {
                out.push(Move::Line { to: [q[0], q[1], z], plunge: false });
            }
        }
        // Loops: two half circles about each centre, then on to the next centre's rear point.
        let mut last = c0;
        for (k, c) in centres.iter().enumerate() {
            let u = dir_at(k);
            let (front, rear) = ([c[0] + rho * u[0], c[1] + rho * u[1]], [c[0] - rho * u[0], c[1] - rho * u[1]]);
            if k > 0 {
                out.push(Move::Line { to: [rear[0], rear[1], z], plunge: false });
            }
            out.push(Move::Arc { to: [front[0], front[1], z], center: *c, ccw: s.climb });
            out.push(Move::Arc { to: [rear[0], rear[1], z], center: *c, ccw: s.climb });
            last = rear;
        }
        out.push(Move::Rapid([last[0], last[1], s.safe]));
        Ok(())
    }
}
