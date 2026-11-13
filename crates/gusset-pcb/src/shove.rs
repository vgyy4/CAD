//! Push-and-shove, kept simple (ADR 0047 phase 3): when a new track has no way around
//! the copper in its way and what blocks it is one track of one other net, that track is
//! taken up, the new track laid, and the other track laid again around it (between the
//! same two ends, with its own width and net). The result is kept only when the design
//! rule check finds nothing it did not find before.
//!
//! "One track" is a chain of segments of one net on the layer joined end to end, with
//! two free ends (at pads, vias or branches of that net). Several blocking nets, or a
//! chain with branches, are refused: the caller reports no way.

use crate::board::{Board, Limits, Pt};
use crate::copper;
use crate::route::{self, Obstacles, Seg};
use crate::rules::Rules;

/// Gap a new track's centre line keeps from a chain to count it as in the way (mm).
const NEAR: f64 = 1e-6;
/// Ends closer than this join (mm).
const JOIN: f64 = 1e-4;

/// A shove that works: the new track's corners and the moved track.
#[derive(Debug, Clone, PartialEq)]
pub struct Shove {
    /// The new track's corner points, `from` first.
    pub path: Vec<Pt>,
    /// The other net's segments taken up (indices in [`Board::tracks`]).
    pub removed: Vec<usize>,
    /// Their new corner points, and their width and net.
    pub moved: Vec<Pt>,
    pub moved_width: f64,
    pub moved_net: Option<String>,
}

fn lines(pts: &[Pt]) -> Vec<Seg> {
    pts.windows(2)
        .filter_map(|w| match w {
            [a, b] => Some(Seg { start: *a, end: *b, mid: None }),
            _ => None,
        })
        .collect()
}

fn same(a: Pt, b: Pt) -> bool {
    libm::hypot(a[0] - b[0], a[1] - b[1]) < JOIN
}

/// The chain of `net`'s straight segments on `layer` through track `seed`, as segment
/// indices from one end to the other and the two end points; `None` when it branches or
/// holds arcs.
fn chain(board: &Board, layer: &str, seed: usize) -> Option<(Vec<usize>, Pt, Pt)> {
    let t0 = board.tracks.get(seed)?;
    let net = board.net_name(&t0.net);
    let mine: Vec<usize> = (0..board.tracks.len())
        .filter(|&i| board.tracks.get(i).is_some_and(|t| t.layer == layer && board.net_name(&t.net) == net))
        .collect();
    let degree = |p: Pt| {
        mine.iter().filter(|&&i| board.tracks.get(i).is_some_and(|t| same(t.start, p) || same(t.end, p))).count()
    };
    // Whether the chain may run on through `p`: two segments meet there and nothing else.
    let joint = |p: Pt| {
        degree(p) == 2
            && !board.vias.iter().any(|v| same(v.at, p))
            && !board
                .pads
                .iter()
                .any(|pad| libm::hypot(pad.at[0] - p[0], pad.at[1] - p[1]) < pad.size[0].max(pad.size[1]) / 2.0)
    };
    if t0.mid.is_some() {
        return None;
    }
    let mut order = vec![seed];
    let mut ends = [t0.start, t0.end];
    for side in 0..2 {
        while let Some(&p) = ends.get(side) {
            if !joint(p) {
                break;
            }
            let next = mine.iter().copied().find(|i| {
                !order.contains(i) && board.tracks.get(*i).is_some_and(|t| same(t.start, p) || same(t.end, p))
            });
            let Some(n) = next else { break };
            let t = board.tracks.get(n)?;
            if t.mid.is_some() || order.len() > 10_000 {
                return None;
            }
            let other = if same(t.start, p) { t.end } else { t.start };
            if side == 0 {
                order.insert(0, n);
            } else {
                order.push(n);
            }
            if let Some(e) = ends.get_mut(side) {
                *e = other;
            }
        }
    }
    let [a, b] = ends;
    Some((order, a, b))
}

/// The board with tracks `removed` taken out and `routes` (pieces, width, net) laid on
/// `layer`.
fn edited(board: &Board, removed: &[usize], layer: &str, routes: &[(&[Pt], f64, Option<&str>)]) -> Option<Board> {
    let mut b = board.clone();
    let lim = Limits::default();
    if !removed.is_empty() {
        b.delete_items(removed, &[], &lim).ok()?;
    }
    let batch: Vec<crate::edit::NewRoute> = routes
        .iter()
        .filter(|r| r.0.len() >= 2)
        .map(|(p, w, n)| (lines(p), *w, layer.to_string(), n.map(str::to_string)))
        .collect();
    b.add_batch(&batch, &[], &lim).ok()?;
    Some(b)
}

/// Tries to lay a track of `net` and `width` from `from` to `to` on `layer` by moving one
/// other net's track aside (see the module notes). `keep` is copper of the new track laid
/// before this leg (its corner points), which the moved track keeps clear of too.
#[allow(clippy::too_many_arguments)]
pub fn shove(
    board: &Board,
    rules: &Rules,
    layer: &str,
    net: Option<&str>,
    width: f64,
    from: Pt,
    to: Pt,
    grid: f64,
    keep: &[Pt],
) -> Option<Shove> {
    // The way the new track would take if other nets' tracks on the layer were not there.
    let others: Vec<usize> = (0..board.tracks.len())
        .filter(|&i| {
            board
                .tracks
                .get(i)
                .is_some_and(|t| t.layer == layer && (net.is_none() || board.net_name(&t.net).as_deref() != net))
        })
        .collect();
    let bare = edited(board, &others, layer, &[])?;
    let path = route::walkaround(&Obstacles::new(&bare, rules, layer, net, width), from, to, grid, true)?;
    // The tracks in its way, and their nets.
    let half = width / 2.0;
    let mut blocking: Vec<usize> = Vec::new();
    for &i in &others {
        let Some(t) = board.tracks.get(i) else { continue };
        let tn = board.net_name(&t.net);
        let need = rules.clearance_of(net).max(rules.clearance_of(tn.as_deref())) + half + t.width / 2.0;
        let hit = path.windows(2).any(|w| match w {
            [a, b] => copper::segment_segment(*a, *b, t.start, t.end).0 < need - NEAR,
            _ => false,
        });
        if hit {
            blocking.push(i);
        }
    }
    let first = *blocking.first()?;
    let t0 = board.tracks.get(first)?;
    let other_net = board.net_name(&t0.net);
    let (moved_ids, a, b) = chain(board, layer, first)?;
    // Everything in the way belongs to that one chain.
    if blocking.iter().any(|i| !moved_ids.contains(i)) {
        return None;
    }
    let other_width = t0.width;
    if moved_ids.iter().any(|&i| board.tracks.get(i).is_none_or(|t| (t.width - other_width).abs() > 1e-9)) {
        return None;
    }
    // Lay the new track where the chain was, then the chain around it.
    let without = edited(board, &moved_ids, layer, &[])?;
    let path = route::walkaround(&Obstacles::new(&without, rules, layer, net, width), from, to, grid, true)?;
    let path = route::simplify(&path);
    let with_new = edited(&without, &[], layer, &[(&path, width, net), (keep, width, net)])?;
    let other_grid = (rules.clearance_of(other_net.as_deref()).min(other_width) / 2.0).clamp(0.05, 0.25);
    let obs = Obstacles::new(&with_new, rules, layer, other_net.as_deref(), other_width);
    let moved = [true, false].into_iter().find_map(|d| route::walkaround(&obs, a, b, other_grid, d))?;
    let moved = route::simplify(&moved);
    // Kept only when the check finds nothing new.
    let after = edited(&with_new, &[], layer, &[(&moved, other_width, other_net.as_deref())])?;
    let before = crate::drc::check(board, rules, 10_000).len();
    if crate::drc::check(&after, rules, 10_000).len() > before {
        return None;
    }
    Some(Shove { path, removed: moved_ids, moved, moved_width: other_width, moved_net: other_net })
}

impl Board {
    /// Moves the other net's track as `s` says (the new track itself is laid by the
    /// caller): its old segments go, its new ones come, in one edit.
    ///
    /// # Errors
    /// A failed edit.
    pub fn apply_shove(&mut self, s: &Shove, layer: &str, limits: &Limits) -> Result<(), crate::board::Error> {
        self.delete_items(&s.removed, &[], limits)?;
        self.add_route(&lines(&s.moved), s.moved_width, layer, s.moved_net.as_deref(), limits)?;
        Ok(())
    }
}
