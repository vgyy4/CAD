//! Connectivity and the ratsnest: which copper of each net already touches, and the
//! shortest lines (a minimum spanning tree per net, nearest anchors) that would join the
//! islands still apart. Filled zones join the pads and tracks whose anchors lie in them.

use crate::board::{Board, Pt};
use crate::copper::{self, Copper};
use std::collections::BTreeMap;

/// An unrouted connection.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub net: String,
    pub a: Pt,
    pub b: Pt,
}

/// The ratsnest of a board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ratsnest {
    pub lines: Vec<Line>,
    /// Nets with copper on the board and how many of them are fully connected.
    pub nets: usize,
    pub complete_nets: usize,
}

impl Ratsnest {
    /// Connections still to route.
    pub fn unrouted(&self) -> usize {
        self.lines.len()
    }
}

/// Copper closer than this touches (mm).
const TOUCH: f64 = 1e-4;

struct Dsu(Vec<usize>);

impl Dsu {
    fn find(&mut self, mut i: usize) -> usize {
        while let Some(&p) = self.0.get(i) {
            if p == i {
                break;
            }
            let gp = self.0.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.0.get_mut(i) {
                *slot = gp;
            }
            i = p;
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb
            && let Some(slot) = self.0.get_mut(ra)
        {
            *slot = rb;
        }
    }
}

/// The islands of connected copper: for each item of `copper`, a representative.
pub fn islands(board: &Board, copper: &Copper) -> Vec<usize> {
    let items = &copper.items;
    let mut dsu = Dsu((0..items.len()).collect());
    for (i, j) in copper::near_pairs(items, TOUCH) {
        let (Some(a), Some(b)) = (items.get(i), items.get(j)) else { continue };
        if a.net.is_none() || a.net != b.net {
            continue;
        }
        if copper::item_gap(a, b).0 <= TOUCH {
            dsu.union(i, j);
        }
    }
    // Zone fills join the copper of their net that touches them on their layer (pads in
    // thermal reliefs touch the spokes).
    for z in board.zones.iter().filter(|z| !z.keepout) {
        let Some(net) = board.net_name(&z.net) else { continue };
        for (layer, poly) in &z.fills {
            let Some(l) = copper.layers.iter().position(|x| x == layer) else { continue };
            if poly.len() < 3 {
                continue;
            }
            let fill = copper::EdgeGrid::new(poly);
            let mut first: Option<usize> = None;
            for (i, it) in items.iter().enumerate() {
                if it.net.as_deref() != Some(net.as_str())
                    || !it.layers.contains(&l)
                    || !copper::boxes_near(&it.bbox, &fill.bbox, TOUCH)
                {
                    continue;
                }
                if it.prims.iter().any(|p| fill.touches(p, TOUCH)) {
                    match first {
                        Some(f) => dsu.union(f, i),
                        None => first = Some(i),
                    }
                }
            }
        }
    }
    (0..items.len()).map(|i| dsu.find(i)).collect()
}

/// The board's ratsnest.
pub fn ratsnest(board: &Board) -> Ratsnest {
    let copper = Copper::of(board);
    let root = islands(board, &copper);
    // Anchors per net: (point, island).
    let mut nets: BTreeMap<&str, Vec<(Pt, usize)>> = BTreeMap::new();
    for (i, it) in copper.items.iter().enumerate() {
        let Some(net) = it.net.as_deref().filter(|n| !n.is_empty()) else { continue };
        let r = root.get(i).copied().unwrap_or(i);
        let v = nets.entry(net).or_default();
        v.extend(it.anchors.iter().map(|p| (*p, r)));
    }
    let mut out = Ratsnest::default();
    for (net, anchors) in nets {
        out.nets += 1;
        let lines = mst(&anchors);
        if lines.is_empty() {
            out.complete_nets += 1;
        }
        out.lines.extend(lines.into_iter().map(|(a, b)| Line { net: net.to_string(), a, b }));
    }
    out
}

/// Prim's algorithm over the anchors, where anchors of one island are already joined:
/// the lines between different islands.
fn mst(anchors: &[(Pt, usize)]) -> Vec<(Pt, Pt)> {
    // One anchor per distinct point and island keeps big nets small.
    let mut pts: Vec<(Pt, usize)> = Vec::with_capacity(anchors.len());
    for a in anchors {
        if !pts
            .iter()
            .rev()
            .take(4)
            .any(|b| b.1 == a.1 && (b.0[0] - a.0[0]).abs() < 1e-9 && (b.0[1] - a.0[1]).abs() < 1e-9)
        {
            pts.push(*a);
        }
    }
    let n = pts.len();
    let islands: std::collections::BTreeSet<usize> = pts.iter().map(|p| p.1).collect();
    if islands.len() < 2 {
        return Vec::new();
    }
    let mut in_tree = vec![false; n];
    let mut best = vec![(f64::INFINITY, usize::MAX); n];
    let mut lines = Vec::new();
    if let Some(b) = best.get_mut(0) {
        *b = (0.0, usize::MAX);
    }
    for _ in 0..n {
        let Some(u) = (0..n).filter(|&i| !in_tree.get(i).copied().unwrap_or(true)).min_by(|&a, &b| {
            best.get(a).map_or(f64::INFINITY, |x| x.0).total_cmp(&best.get(b).map_or(f64::INFINITY, |x| x.0))
        }) else {
            break;
        };
        if let Some(t) = in_tree.get_mut(u) {
            *t = true;
        }
        let (Some(&(pu, iu)), Some(&(_, from))) = (pts.get(u), best.get(u)) else { break };
        if let Some(&(pf, i_from)) = pts.get(from)
            && i_from != iu
        {
            lines.push((pf, pu));
        }
        for v in 0..n {
            if in_tree.get(v).copied().unwrap_or(true) {
                continue;
            }
            let Some(&(pv, iv)) = pts.get(v) else { continue };
            let w = if iv == iu { 0.0 } else { libm::hypot(pv[0] - pu[0], pv[1] - pu[1]) };
            if let Some(b) = best.get_mut(v)
                && w < b.0
            {
                *b = (w, u);
            }
        }
    }
    lines
}
