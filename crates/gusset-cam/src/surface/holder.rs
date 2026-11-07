//! Holder and shank collision check, and a lifting variant of the raster finish (ADR 0040,
//! "Holder check" amendment).

use super::{Cutter, Dropper, Profile, RasterSpec, Surface3d, mesh_bounds, raster_finish};
use crate::{CamError, MAX_MOVES, Move, Setup, Toolpath};

/// Most samples one check or lift may take.
const MAX_SAMPLES: usize = 3_000_000;
/// Most collisions listed in a report (the count is exact).
const MAX_LISTED: usize = 10_000;

/// The tool above its cutting end, all dimensions in mm. Measured from the tool tip: the flutes
/// reach `flute_length`, the shank (a cylinder of `shank_diameter`) goes up to `stickout`, where
/// the holder (a cylinder of `holder_diameter`) begins and continues upwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Holder {
    pub shank_diameter: f64,
    pub flute_length: f64,
    pub holder_diameter: f64,
    /// Tool tip to the holder's face.
    pub stickout: f64,
}

/// The part of the tool that hits the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderPart {
    Shank,
    Holder,
}

/// One sampled tool position where a part of the tool is inside the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HolderCollision {
    /// Tool tip position.
    pub at: [f64; 3],
    pub part: HolderPart,
    /// How far the highest surface point under the part is above the part's lowest face.
    pub depth: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HolderReport {
    /// The first 10 000 collisions in toolpath order.
    pub collisions: Vec<HolderCollision>,
    /// All collisions found (can exceed `collisions.len()`).
    pub total_collisions: usize,
    /// Smallest gap between a part's lowest face and the surface under it over all sampled
    /// positions (negative when colliding); `f64::INFINITY` when nothing is under the tool.
    pub min_clearance: f64,
}

/// Result of [`raster_finish_safe`].
#[derive(Debug, Clone, PartialEq)]
pub struct SafeRaster {
    /// The raster path with the tool raised wherever the holder or shank would hit. Material is
    /// left there: the surface under raised parts is not finished.
    pub toolpath: Toolpath,
    /// The check of the unmodified raster path.
    pub report: HolderReport,
    /// Largest rise applied to the tool tip.
    pub max_lift: f64,
    /// Samples that were raised.
    pub lifted_samples: usize,
}

struct Checker {
    shank: Dropper,
    holder: Dropper,
    h: Holder,
}

fn validate(setup: &Setup, h: &Holder) -> Result<(), CamError> {
    let fin = |v: f64| v.is_finite();
    if !(fin(setup.tool_diameter) && setup.tool_diameter > 0.0 && fin(setup.safe)) {
        return Err(CamError::Invalid("tool diameter must be positive and safe finite".into()));
    }
    if !(fin(h.shank_diameter)
        && h.shank_diameter > 0.0
        && fin(h.holder_diameter)
        && h.holder_diameter > 0.0
        && fin(h.flute_length)
        && h.flute_length > 0.0
        && fin(h.stickout)
        && h.stickout >= h.flute_length)
    {
        return Err(CamError::Invalid(
            "holder: diameters and flute length must be positive, stickout at least the flute length".into(),
        ));
    }
    Ok(())
}

impl Checker {
    fn new(surface: &Surface3d, setup: &Setup, h: &Holder) -> Result<Self, CamError> {
        validate(setup, h)?;
        let (lo, hi) = mesh_bounds(surface.positions)
            .ok_or_else(|| CamError::Invalid("the surface has no valid vertices".into()))?;
        let flat = |d: f64| Profile { r: d / 2.0, c: 0.0 };
        Ok(Checker {
            shank: Dropper::new(surface, flat(h.shank_diameter), lo, hi)?,
            holder: Dropper::new(surface, flat(h.holder_diameter), lo, hi)?,
            h: *h,
        })
    }

    /// Highest surface Z under the shank and under the holder at `(x, y)`.
    fn highest(&mut self, x: f64, y: f64) -> (Option<f64>, Option<f64>) {
        (self.shank.drop(x, y), self.holder.drop(x, y))
    }

    /// Lowest tip Z at which neither part touches the surface at `(x, y)`.
    fn required_tip(&mut self, x: f64, y: f64) -> f64 {
        let (s, h) = self.highest(x, y);
        let a = s.map_or(f64::NEG_INFINITY, |z| z - self.h.flute_length);
        let b = h.map_or(f64::NEG_INFINITY, |z| z - self.h.stickout);
        a.max(b)
    }
}

/// Positions along a cutting move after `from`, at most `step` apart (ending at the move's end).
/// An arc is followed along its circle. Rapids give nothing.
fn sample_move(from: [f64; 3], mv: &Move, step: f64) -> Vec<[f64; 3]> {
    match *mv {
        Move::Rapid(_) => Vec::new(),
        Move::Line { to, .. } => {
            let len = libm::hypot(to[0] - from[0], to[1] - from[1]);
            let n = libm::ceil(len / step).max(1.0) as usize;
            (1..=n)
                .map(|k| {
                    let t = if k == n { 1.0 } else { k as f64 / n as f64 };
                    [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, from[2] + (to[2] - from[2]) * t]
                })
                .collect()
        }
        Move::Arc { to, center, ccw } => {
            let a0 = libm::atan2(from[1] - center[1], from[0] - center[0]);
            let a1 = libm::atan2(to[1] - center[1], to[0] - center[0]);
            let tau = std::f64::consts::TAU;
            let mut sweep = (if ccw { a1 - a0 } else { a0 - a1 }).rem_euclid(tau);
            if sweep < 1e-12 {
                sweep = tau;
            }
            let rad = libm::hypot(from[0] - center[0], from[1] - center[1]);
            let n = libm::ceil(rad * sweep / step).max(1.0) as usize;
            (1..=n)
                .map(|k| {
                    if k == n {
                        return to;
                    }
                    let f = k as f64 / n as f64;
                    let a = if ccw { a0 + sweep * f } else { a0 - sweep * f };
                    [center[0] + rad * libm::cos(a), center[1] + rad * libm::sin(a), from[2] + (to[2] - from[2]) * f]
                })
                .collect()
        }
    }
}

fn end_of(mv: &Move) -> [f64; 3] {
    match *mv {
        Move::Rapid(p) => p,
        Move::Line { to, .. } | Move::Arc { to, .. } => to,
    }
}

/// Checks the holder and shank against the surface along every cutting move of `path`.
///
/// Positions are sampled at most one tool radius apart along each line and arc (and at both
/// ends); rapids are not checked. For a part, the highest surface point inside its cylinder's
/// radius (drop-cutter with a flat cutter of that radius) must lie below the part's lowest
/// face: the flute top for the shank, the stickout for the holder. The surface is taken as a
/// height field seen from +Z, as for the finishing itself. Between samples the surface is
/// not looked at, so features narrower than a tool radius can be missed.
pub fn check_holder(
    surface: &Surface3d,
    _cutter: Cutter,
    setup: &Setup,
    holder: &Holder,
    path: &Toolpath,
) -> Result<HolderReport, CamError> {
    let mut ck = Checker::new(surface, setup, holder)?;
    let step = setup.tool_diameter / 2.0;
    let mut rep = HolderReport { collisions: Vec::new(), total_collisions: 0, min_clearance: f64::INFINITY };
    let mut samples = 0usize;
    let mut pos: Option<[f64; 3]> = None;
    let mut prev_rapid = true;
    for mv in &path.moves {
        let cutting = !matches!(mv, Move::Rapid(_));
        if let (true, Some(p)) = (cutting, pos) {
            let mut pts = sample_move(p, mv, step);
            if prev_rapid {
                pts.insert(0, p);
            }
            for q in pts {
                samples += 1;
                if samples > MAX_SAMPLES {
                    return Err(CamError::TooLarge("too many holder samples".into()));
                }
                let (s, h) = ck.highest(q[0], q[1]);
                for (part, top, bottom) in [
                    (HolderPart::Shank, s, q[2] + holder.flute_length),
                    (HolderPart::Holder, h, q[2] + holder.stickout),
                ] {
                    let Some(top) = top else { continue };
                    let gap = bottom - top;
                    rep.min_clearance = rep.min_clearance.min(gap);
                    if gap < -1e-9 {
                        rep.total_collisions += 1;
                        if rep.collisions.len() < MAX_LISTED {
                            rep.collisions.push(HolderCollision { at: q, part, depth: -gap });
                        }
                    }
                }
            }
        }
        prev_rapid = !cutting;
        pos = Some(end_of(mv));
    }
    Ok(rep)
}

/// Raises the tool along every cutting move so neither shank nor holder touches the surface.
/// Returns the new path, the largest rise and the number of raised samples. Arcs become lines.
/// Material is left wherever the tool is raised.
pub fn lift_for_holder(
    surface: &Surface3d,
    setup: &Setup,
    holder: &Holder,
    path: &Toolpath,
) -> Result<(Toolpath, f64, usize), CamError> {
    let mut ck = Checker::new(surface, setup, holder)?;
    let step = setup.tool_diameter / 2.0;
    let mut out: Vec<Move> = Vec::with_capacity(path.moves.len());
    let mut orig: Option<[f64; 3]> = None;
    let (mut max_lift, mut lifted, mut samples) = (0.0f64, 0usize, 0usize);
    for mv in &path.moves {
        let end = end_of(mv);
        let Some(from) = orig.filter(|_| !matches!(mv, Move::Rapid(_))) else {
            out.push(*mv);
            orig = Some(end);
            continue;
        };
        let pts = sample_move(from, mv, step);
        samples += pts.len();
        if samples > MAX_SAMPLES {
            return Err(CamError::TooLarge("too many holder samples".into()));
        }
        let plunge = matches!(mv, Move::Line { plunge: true, .. });
        let mut raised = Vec::with_capacity(pts.len());
        let mut any = false;
        for q in &pts {
            let z = q[2].max(ck.required_tip(q[0], q[1]));
            if z > q[2] + 1e-9 {
                any = true;
                lifted += 1;
                max_lift = max_lift.max(z - q[2]);
            }
            if z > setup.safe {
                return Err(CamError::Invalid("the holder cannot clear the surface below the safe height".into()));
            }
            raised.push([q[0], q[1], z]);
        }
        if plunge {
            // A raised plunge needs its approach rapid above the new end point.
            if let (Some(last), Some(Move::Rapid(r))) = (raised.last().copied(), out.last_mut())
                && libm::hypot(r[0] - last[0], r[1] - last[1]) < 1e-9
                && r[2] < last[2] + 1.0
            {
                r[2] = (last[2] + 1.0).min(setup.safe);
            }
        }
        if any {
            let n = raised.len();
            for (k, q) in raised.into_iter().enumerate() {
                out.push(Move::Line { to: q, plunge: plunge && k + 1 == n });
            }
        } else {
            out.push(*mv);
        }
        if out.len() > MAX_MOVES {
            return Err(CamError::TooLarge("too many moves".into()));
        }
        orig = Some(end);
    }
    let tp = Toolpath { moves: out, rings: path.rings, levels: path.levels.clone(), drill: path.drill.clone() };
    Ok((tp, max_lift, lifted))
}

/// [`raster_finish`] followed by the holder check and a lift of every sample where the shank or
/// holder would collide. The lifted path is collision free (at the sampling of
/// [`check_holder`]) but leaves material under it; `max_lift` and `lifted_samples` say how much.
pub fn raster_finish_safe(
    surface: &Surface3d,
    cutter: Cutter,
    spec: &RasterSpec,
    setup: &Setup,
    holder: &Holder,
) -> Result<SafeRaster, CamError> {
    let path = raster_finish(surface, cutter, spec, setup)?;
    let report = check_holder(surface, cutter, setup, holder, &path)?;
    if report.total_collisions == 0 {
        return Ok(SafeRaster { toolpath: path, report, max_lift: 0.0, lifted_samples: 0 });
    }
    let (toolpath, max_lift, lifted_samples) = lift_for_holder(surface, setup, holder, &path)?;
    Ok(SafeRaster { toolpath, report, max_lift, lifted_samples })
}

#[cfg(test)]
mod tests;
