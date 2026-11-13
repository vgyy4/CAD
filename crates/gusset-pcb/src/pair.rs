//! Differential pairs (ADR 0055): recognising the two nets of a pair by name and routing
//! both tracks together, coupled along a 45° centre line.
//!
//! Names follow KiCad's convention: `X_P`/`X_N` and `X+`/`X-` (the `_p`/`_n` spellings
//! too). The pair is routed by finding one centre line for a track as wide as both
//! tracks and the gap between them, then offsetting it to each side. The ends fan out
//! straight to the nearest pad of each net.

use crate::board::{Board, Pt};
use crate::copper::{self, Copper};
use crate::route::{self, Obstacles, Seg};
use crate::rules::Rules;

/// Gaps may fall short of the rule by this (mm), as in the DRC.
const SLACK: f64 = 4e-4;
/// Pads farther than this from the end of the centre line are not fanned out to (mm).
const FAN_REACH: f64 = 10.0;

/// The two nets of a differential pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// The shared name without the polarity suffix.
    pub base: String,
    /// The positive net (`_P`, `+`).
    pub p: String,
    /// The negative net (`_N`, `-`).
    pub n: String,
}

/// Splits a net name into its pair name and polarity (`true` for the positive net).
pub fn split(name: &str) -> Option<(&str, bool)> {
    const SUFFIXES: [(&str, bool); 6] =
        [("_P", true), ("_N", false), ("_p", true), ("_n", false), ("+", true), ("-", false)];
    SUFFIXES
        .iter()
        .find_map(|(s, pos)| name.strip_suffix(s).filter(|b| !b.is_empty() && !b.ends_with('/')).map(|b| (b, *pos)))
}

/// The other net of a pair, by name (`USB_P` gives `USB_N`).
pub fn partner(name: &str) -> Option<String> {
    const SWAP: [(&str, &str); 6] = [("_P", "_N"), ("_N", "_P"), ("_p", "_n"), ("_n", "_p"), ("+", "-"), ("-", "+")];
    let (base, _) = split(name)?;
    SWAP.iter().find_map(|(from, to)| name.strip_suffix(from).filter(|b| *b == base).map(|b| format!("{b}{to}")))
}

/// Every pair whose two nets both exist on the board, by name.
pub fn pairs(board: &Board) -> Vec<Pair> {
    let names: std::collections::BTreeSet<&str> = board.nets.iter().map(|n| n.name.as_str()).collect();
    let mut out = Vec::new();
    for n in &names {
        let Some((base, true)) = split(n) else { continue };
        if let Some(q) = partner(n).filter(|q| names.contains(q.as_str())) {
            out.push(Pair { base: base.to_string(), p: (*n).to_string(), n: q });
        }
    }
    out
}

/// The pair a name stands for: either of its nets or its shared name.
pub fn find(board: &Board, name: &str) -> Option<Pair> {
    pairs(board).into_iter().find(|p| p.p == name || p.n == name || p.base == name)
}

/// A routed pair.
#[derive(Debug, Clone)]
pub struct PairRoute {
    /// The centre line the coupled part follows.
    pub centre: Vec<Pt>,
    /// The positive net's pieces, fan-out included.
    pub p: Vec<Seg>,
    /// The negative net's pieces.
    pub n: Vec<Seg>,
    /// Track width and the gap between the two tracks' edges used (mm).
    pub width: f64,
    pub gap: f64,
}

fn unit(a: Pt, b: Pt) -> Pt {
    let l = libm::hypot(b[0] - a[0], b[1] - a[1]);
    if l > 0.0 { [(b[0] - a[0]) / l, (b[1] - a[1]) / l] } else { [0.0, 0.0] }
}

/// Removes the short jogs a grid search leaves: an inner segment shorter than `min` is
/// replaced by the corner where its two neighbours meet (every direction stays a multiple
/// of 45°), when that corner is close by.
fn tidy(pts: &[Pt], min: f64) -> Vec<Pt> {
    let mut p = pts.to_vec();
    let dist = |a: Pt, b: Pt| libm::hypot(b[0] - a[0], b[1] - a[1]);
    for _ in 0..pts.len() {
        let short = (1..p.len().saturating_sub(2))
            .find(|&i| matches!((p.get(i), p.get(i + 1)), (Some(a), Some(b)) if dist(*a, *b) < min));
        let Some(i) = short else { break };
        let (Some(&a0), Some(&a1), Some(&b0), Some(&b1)) = (p.get(i - 1), p.get(i), p.get(i + 1), p.get(i + 2)) else {
            break;
        };
        let (d1, d2) = ([a1[0] - a0[0], a1[1] - a0[1]], [b1[0] - b0[0], b1[1] - b0[1]]);
        let cross = d1[0] * d2[1] - d1[1] * d2[0];
        let replacement: Vec<Pt> = if cross.abs() < 1e-9 {
            Vec::new()
        } else {
            let t = ((b0[0] - a0[0]) * d2[1] - (b0[1] - a0[1]) * d2[0]) / cross;
            let x = [a0[0] + d1[0] * t, a0[1] + d1[1] * t];
            if dist(x, a1) > 3.0 * min {
                break;
            }
            vec![x]
        };
        p.splice(i..i + 2, replacement);
        p = route::simplify(&p);
    }
    p
}

/// The polyline moved `d` to its left (right when negative), corners mitred.
pub fn offset(pts: &[Pt], d: f64) -> Option<Vec<Pt>> {
    let left = |u: Pt| [-u[1], u[0]];
    let mut out = Vec::with_capacity(pts.len());
    for (i, &p) in pts.iter().enumerate() {
        let before = i.checked_sub(1).and_then(|k| pts.get(k)).map(|&q| unit(q, p));
        let after = pts.get(i + 1).map(|&q| unit(p, q));
        let o = match (before, after) {
            (Some(u), Some(v)) => {
                let (nu, nv) = (left(u), left(v));
                let k = 1.0 + nu[0] * nv[0] + nu[1] * nv[1];
                if k < 0.05 {
                    return None;
                }
                [p[0] + (nu[0] + nv[0]) * d / k, p[1] + (nu[1] + nv[1]) * d / k]
            }
            (Some(u), None) | (None, Some(u)) => [p[0] + left(u)[0] * d, p[1] + left(u)[1] * d],
            (None, None) => return None,
        };
        out.push(o);
    }
    Some(out)
}

fn pieces(pts: &[Pt]) -> Vec<Seg> {
    pts.windows(2)
        .filter_map(|w| match w {
            [a, b] if libm::hypot(b[0] - a[0], b[1] - a[1]) > 1e-6 => Some(Seg { start: *a, end: *b, mid: None }),
            _ => None,
        })
        .collect()
}

/// The pad of `net` on `layer` nearest to `at`, within [`FAN_REACH`].
fn nearest_pad(board: &Board, net: &str, layer: &str, at: Pt) -> Option<Pt> {
    board
        .pads
        .iter()
        .filter(|p| board.net_name(&p.net).as_deref() == Some(net) && board.on_layer(&p.layers, layer))
        .map(|p| (libm::hypot(p.at[0] - at[0], p.at[1] - at[1]), p.at))
        .filter(|(d, _)| *d <= FAN_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, at)| at)
}

/// Routes the pair `pair` on `layer` through the centre points `points` (the first and
/// last are the ends of the coupled part), with tracks `width` wide and `gap` apart
/// (raised to the net's clearance when smaller).
///
/// # Errors
/// A message when there is no way for the centre line or a fan-out, or the tracks would
/// break a clearance.
pub fn route_pair(
    board: &Board,
    rules: &Rules,
    layer: &str,
    pair: &Pair,
    points: &[Pt],
    width: f64,
    gap: f64,
) -> Result<PairRoute, String> {
    // A centre line that starts straight leaves the fan-out room; one that starts on the
    // diagonal can pinch it, so each is tried.
    let first = try_route(board, rules, layer, pair, points, width, gap, false);
    first.or_else(|e| try_route(board, rules, layer, pair, points, width, gap, true).map_err(|_| e))
}

#[allow(clippy::too_many_arguments)]
fn try_route(
    board: &Board,
    rules: &Rules,
    layer: &str,
    pair: &Pair,
    points: &[Pt],
    width: f64,
    gap: f64,
    diagonal_first: bool,
) -> Result<PairRoute, String> {
    let clearance = rules.clearance_of(Some(&pair.p)).max(rules.clearance_of(Some(&pair.n)));
    let gap = gap.max(clearance);
    let d = (width + gap) / 2.0;
    let copper = Copper::of(board);
    let skip = [pair.p.as_str(), pair.n.as_str()];
    let wide = Obstacles::skipping(board, &copper, rules, layer, Some(&pair.p), &skip, 2.0 * width + gap);
    let grid = (clearance.min(width) / 2.0).clamp(0.05, 0.25);
    // Each end starts with a short straight stub along the line from the pads, so the
    // fan-out meets the coupled tracks square instead of pinching on a diagonal start.
    let mut way: Vec<Pt> = points.to_vec();
    let stub = (4.0 * (width + gap)).max(1.0);
    let long_enough = match (points.first(), points.last()) {
        (Some(a), Some(b)) => libm::hypot(b[0] - a[0], b[1] - a[1]) > 3.0 * stub,
        _ => false,
    };
    let mut ends: [Option<Pt>; 2] = [None, None];
    if long_enough {
        for (k, at) in [points.first(), points.last()].into_iter().enumerate() {
            let Some(&at) = at else { continue };
            let pads: Vec<Pt> = [&pair.p, &pair.n].iter().filter_map(|n| nearest_pad(board, n, layer, at)).collect();
            if pads.is_empty() {
                continue;
            }
            let m =
                pads.iter().fold([0.0, 0.0], |m, p| [m[0] + p[0] / pads.len() as f64, m[1] + p[1] / pads.len() as f64]);
            let (dx, dy) = (at[0] - m[0], at[1] - m[1]);
            if libm::hypot(dx, dy) < 1e-6 {
                continue;
            }
            // The nearest of the eight 45° directions.
            let ang = (libm::atan2(dy, dx) / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
            let q = [at[0] + stub * libm::cos(ang), at[1] + stub * libm::sin(ang)];
            if wide.clear(at, q)
                && let Some(e) = ends.get_mut(k)
            {
                *e = Some(q);
            }
        }
        if let (Some(q), Some(f)) = (ends[0], way.first_mut()) {
            *f = q;
        }
        if let (Some(q), Some(l)) = (ends[1], way.last_mut()) {
            *l = q;
        }
    }
    let mut centre: Vec<Pt> = way.first().copied().into_iter().collect();
    for (i, w) in way.windows(2).enumerate() {
        let &[a, c] = w else { continue };
        let leg = route::walkaround(&wide, a, c, grid, diagonal_first)
            .ok_or_else(|| format!("no clear way for the pair from point {i} to point {}", i + 1))?;
        centre.extend(leg.into_iter().skip(1));
    }
    if let (Some(_), Some(&p)) = (ends[0], points.first()) {
        centre.insert(0, p);
    }
    if let (Some(_), Some(&p)) = (ends[1], points.last()) {
        centre.push(p);
    }
    let centre = route::simplify(&centre);
    let tidy = tidy(&centre, width + gap);
    let centre = if tidy.windows(2).all(|w| matches!(w, [a, b] if wide.clear(*a, *b))) { tidy } else { centre };
    let (Some(&first), Some(&last)) = (centre.first(), centre.last()) else {
        return Err("a pair needs two points".into());
    };
    let left = offset(&centre, d).ok_or("the centre line turns back on itself")?;
    let right = offset(&centre, -d).ok_or("the centre line turns back on itself")?;
    let (Some(&l0), Some(&r0), Some(&l1), Some(&r1)) = (left.first(), right.first(), left.last(), right.last()) else {
        return Err("a pair needs two points".into());
    };
    // The pads each end fans out to; which offset line is the positive net's follows from
    // them (summed over both ends, so the tracks never swap sides along the way).
    let pad = |net: &str, at: Pt| nearest_pad(board, net, layer, at);
    let ends = [(first, l0, r0), (last, l1, r1)].map(|(at, l, r)| {
        let (pp, np) = (pad(&pair.p, at), pad(&pair.n, at));
        let dist = |a: Option<Pt>, b: Pt| a.map_or(0.0, |a| libm::hypot(a[0] - b[0], a[1] - b[1]));
        ((pp, np), dist(pp, l) + dist(np, r), dist(pp, r) + dist(np, l))
    });
    let straight: f64 = ends.iter().map(|e| e.1).sum();
    let crossed: f64 = ends.iter().map(|e| e.2).sum();
    let p_left = straight <= crossed;
    let (pl, nl) = if p_left { (&left, &right) } else { (&right, &left) };
    let (start, end) = (ends[0].0, ends[1].0);
    let track = |line: &[Pt], s: Option<Pt>, e: Option<Pt>| -> Vec<Seg> {
        let mut pts: Vec<Pt> = s.into_iter().collect();
        pts.extend_from_slice(line);
        pts.extend(e);
        pieces(&pts)
    };
    let p = track(pl, start.0, end.0);
    let n = track(nl, start.1, end.1);
    // Every piece keeps its net's clearances; the two nets keep theirs from each other.
    for (net, segs) in [(&pair.p, &p), (&pair.n, &n)] {
        let obs = Obstacles::with_copper(board, &copper, rules, layer, Some(net), width);
        if segs.iter().any(|s| !obs.clear(s.start, s.end)) {
            let which = if net == &pair.p { "positive" } else { "negative" };
            return Err(format!("the {which} track does not fit (a pad or copper is in the way)"));
        }
    }
    let need = width + clearance - SLACK;
    for a in &p {
        for b in &n {
            if copper::segment_segment(a.start, a.end, b.start, b.end).0 < need {
                return Err("the two tracks come closer than the clearance (the pads are too close for the gap)".into());
            }
        }
    }
    Ok(PairRoute { centre, p, n, width, gap })
}
