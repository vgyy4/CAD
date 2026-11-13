//! Length tuning (ADR 0055): serpentine meanders in a straight track of a net to reach a
//! target length.
//!
//! A meander is a run of square bumps on one side of the track, all legs the same pitch
//! apart, the corners chamfered at 45°. For a given number of bumps the height is solved
//! so the length added is exact; the bump count is the fewest that fits the amplitude.
//! Each side of the track and a few smaller amplitudes are tried until every piece keeps
//! its clearances.

use crate::board::{Board, Pt};
use crate::copper::Copper;
use crate::length;
use crate::route::{self, Obstacles, Seg};
use crate::rules::Rules;

/// 8 - 4 sqrt 2: what a chamfered bump of height h and chamfer c adds is 2h - K c.
const K: f64 = 8.0 - 4.0 * std::f64::consts::SQRT_2;

/// What to reach and how the meanders look.
#[derive(Debug, Clone, Copy)]
pub struct TuneOpts {
    /// The net's total length wanted (mm, tracks and vias).
    pub target: f64,
    /// How far from the target the result may be (mm).
    pub tolerance: f64,
    /// The tallest bump (mm, centre line to centre line).
    pub amplitude: f64,
    /// The gap between neighbouring legs, edge to edge (mm).
    pub spacing: f64,
}

/// A meander for one track of the net.
#[derive(Debug, Clone)]
pub struct Tuned {
    /// The track (index in `Board::tracks`) the pieces replace.
    pub track: usize,
    pub pieces: Vec<Seg>,
    pub bumps: usize,
    /// Length added (mm).
    pub added: f64,
}

/// Bump shapes along a straight run: points in (along, aside) coordinates.
fn bump_points(len: f64, n: usize, pitch: f64, c: f64, h: f64) -> Vec<Pt> {
    let extent = (2 * n - 1) as f64 * pitch + 2.0 * c;
    let x0 = (len - extent) / 2.0;
    let mut pts = vec![[0.0, 0.0]];
    for k in 0..n {
        let l = x0 + c + (2 * k) as f64 * pitch;
        pts.extend([
            [l - c, 0.0],
            [l, c],
            [l, h - c],
            [l + c, h],
            [l + pitch - c, h],
            [l + pitch, h - c],
            [l + pitch, c],
            [l + pitch + c, 0.0],
        ]);
    }
    pts.push([len, 0.0]);
    route::simplify(&pts)
}

/// Tries one track, side and amplitude. `Err` carries why not (for the message).
fn attempt(
    obs: &Obstacles,
    (a, b): (Pt, Pt),
    width: f64,
    delta: f64,
    o: &TuneOpts,
    amplitude: f64,
    side: f64,
) -> Result<(Vec<Seg>, usize), &'static str> {
    let len = libm::hypot(b[0] - a[0], b[1] - a[1]);
    let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
    let nrm = [-u[1] * side, u[0] * side];
    let pitch = width + o.spacing;
    let c_min = width.min(pitch / 2.0);
    let c0 = (amplitude / 4.0).min(pitch / 2.0).max(c_min);
    let amp = amplitude.max(2.0 * c0);
    let max_add = 2.0 * amp - K * c0;
    let n = ((delta / max_add).ceil() as usize).max(1);
    if n > 2000 || (2 * n - 1) as f64 * pitch + 2.0 * c0 + 2.0 * pitch > len {
        return Err("the track is too short");
    }
    let add = delta / n as f64;
    let (c, h) = if add >= (4.0 - K) * c0 {
        (c0, (add + K * c0) / 2.0)
    } else {
        let c = add / (4.0 - K);
        if c >= c_min {
            (c, 2.0 * c)
        } else if ((4.0 - K) * c_min - delta).abs() <= o.tolerance {
            (c_min, 2.0 * c_min)
        } else {
            return Err("the extra length is below the smallest meander");
        }
    };
    let pts: Vec<Pt> = bump_points(len, n, pitch, c, h)
        .into_iter()
        .map(|[x, y]| [a[0] + u[0] * x + nrm[0] * y, a[1] + u[1] * x + nrm[1] * y])
        .collect();
    let pieces: Vec<Seg> = pts
        .windows(2)
        .filter_map(|w| match w {
            [s, e] => Some(Seg { start: *s, end: *e, mid: None }),
            _ => None,
        })
        .collect();
    if pieces.iter().any(|s| !obs.clear(s.start, s.end)) {
        return Err("copper is in the way");
    }
    Ok((pieces, n))
}

/// Meanders a straight track of `net` so the net's length reaches the target.
///
/// `track` picks the track (an index in `Board::tracks`); none tries the net's straight
/// tracks from the longest.
///
/// # Errors
/// A message when the net is already long enough, has no straight track, or no meander
/// fits (too short, too little room, or the target is out of reach within the tolerance).
pub fn tune(board: &Board, rules: &Rules, net: &str, track: Option<usize>, o: &TuneOpts) -> Result<Tuned, String> {
    if !(o.target.is_finite() && o.tolerance.is_finite() && o.amplitude.is_finite() && o.spacing.is_finite())
        || o.tolerance < 0.0
        || o.amplitude <= 0.0
        || o.spacing < 0.0
    {
        return Err("target, tolerance, amplitude and spacing must be numbers (amplitude above 0)".into());
    }
    let now = length::length_of(board, net);
    if now.tracks == 0 {
        return Err("the net has no tracks".into());
    }
    let delta = o.target - now.total();
    if delta < -o.tolerance {
        return Err(format!("the net is already {:.3} mm longer than the target", -delta));
    }
    if delta.abs() <= o.tolerance {
        return Err("the net is already within the tolerance of the target".into());
    }
    let mut cands: Vec<usize> = match track {
        Some(i) => {
            let t = board.tracks.get(i).ok_or_else(|| format!("there is no track {i}"))?;
            if board.net_name(&t.net).as_deref() != Some(net) {
                return Err(format!("track {i} is not on this net"));
            }
            if t.mid.is_some() {
                return Err("an arc track cannot be meandered; pick a straight one".into());
            }
            vec![i]
        }
        None => (0..board.tracks.len())
            .filter(|&i| {
                board.tracks.get(i).is_some_and(|t| t.mid.is_none() && board.net_name(&t.net).as_deref() == Some(net))
            })
            .collect(),
    };
    cands.sort_by(|&x, &y| {
        let l = |i: usize| board.tracks.get(i).map_or(0.0, length::track_length);
        l(y).total_cmp(&l(x))
    });
    if cands.is_empty() {
        return Err("the net has no straight track to meander".into());
    }
    let copper = Copper::of(board);
    let mut why = "no meander fits";
    for &i in cands.iter().take(8) {
        let Some(t) = board.tracks.get(i) else { continue };
        let obs = Obstacles::with_copper(board, &copper, rules, &t.layer, Some(net), t.width);
        for f in [1.0, 0.75, 0.5, 0.35] {
            for side in [1.0, -1.0] {
                match attempt(&obs, (t.start, t.end), t.width, delta, o, o.amplitude * f, side) {
                    Ok((pieces, bumps)) => {
                        let old = length::track_length(t);
                        let added = route::length(&pieces) - old;
                        if (added - delta).abs() <= o.tolerance + 1e-9 {
                            return Ok(Tuned { track: i, pieces, bumps, added });
                        }
                        why = "the meander does not reach the target";
                    }
                    Err(e) => why = e,
                }
            }
        }
    }
    Err(format!("{why}: no room to add {delta:.3} mm to this net"))
}
