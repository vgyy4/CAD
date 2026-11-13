//! Routed length of nets and the skew of differential pairs (ADR 0055).
//!
//! A net's length is the sum of its tracks (lines and arcs, exact arc lengths) plus one
//! board thickness for each via, since a via carries the signal through the board. Zone
//! fills and pads do not count.

use crate::board::{Board, Track};
use crate::geom;
use crate::pair::{self, Pair};

/// The routed length of one net.
#[derive(Debug, Clone, PartialEq)]
pub struct NetLength {
    pub net: String,
    pub tracks: usize,
    pub track_mm: f64,
    pub vias: usize,
    pub via_mm: f64,
}

impl NetLength {
    /// Tracks and vias together (mm).
    pub fn total(&self) -> f64 {
        self.track_mm + self.via_mm
    }
}

/// A differential pair's lengths and the difference between them.
#[derive(Debug, Clone, PartialEq)]
pub struct Skew {
    pub pair: Pair,
    pub p: NetLength,
    pub n: NetLength,
}

impl Skew {
    /// |P - N| (mm).
    pub fn skew(&self) -> f64 {
        (self.p.total() - self.n.total()).abs()
    }

    /// The shorter net of the pair (P when equal).
    pub fn shorter(&self) -> &str {
        if self.n.total() < self.p.total() { &self.pair.n } else { &self.pair.p }
    }
}

/// The length of one track piece (mm).
pub fn track_length(t: &Track) -> f64 {
    match t.mid {
        Some(m) => geom::arc_length(t.start, m, t.end),
        None => libm::hypot(t.end[0] - t.start[0], t.end[1] - t.start[1]),
    }
}

/// The routed length of `net`.
pub fn length_of(board: &Board, net: &str) -> NetLength {
    let mut l = NetLength { net: net.to_string(), tracks: 0, track_mm: 0.0, vias: 0, via_mm: 0.0 };
    for t in board.tracks.iter().filter(|t| board.net_name(&t.net).as_deref() == Some(net)) {
        l.tracks += 1;
        l.track_mm += track_length(t);
    }
    l.vias = board.vias.iter().filter(|v| board.net_name(&v.net).as_deref() == Some(net)).count();
    l.via_mm = l.vias as f64 * board.thickness;
    l
}

/// Every net with a track or a via, by name.
pub fn net_lengths(board: &Board) -> Vec<NetLength> {
    let mut by: std::collections::BTreeMap<String, NetLength> = std::collections::BTreeMap::new();
    for t in &board.tracks {
        if let Some(n) = board.net_name(&t.net) {
            let e = by.entry(n.clone()).or_insert(NetLength { net: n, tracks: 0, track_mm: 0.0, vias: 0, via_mm: 0.0 });
            e.tracks += 1;
            e.track_mm += track_length(t);
        }
    }
    for v in &board.vias {
        if let Some(n) = board.net_name(&v.net) {
            let e = by.entry(n.clone()).or_insert(NetLength { net: n, tracks: 0, track_mm: 0.0, vias: 0, via_mm: 0.0 });
            e.vias += 1;
            e.via_mm += board.thickness;
        }
    }
    by.into_values().collect()
}

/// The skew of every differential pair of the board.
pub fn skews(board: &Board) -> Vec<Skew> {
    pair::pairs(board)
        .into_iter()
        .map(|pair| {
            let (p, n) = (length_of(board, &pair.p), length_of(board, &pair.n));
            Skew { pair, p, n }
        })
        .collect()
}
