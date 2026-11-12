//! A basic sanity check before fabrication output (full DRC is a later phase): the outline
//! closes, every pad and track is on a copper layer of the board, and every net named
//! exists.

use crate::board::{Board, NetRef, PadKind};
use crate::geom;

/// One problem found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// `outline_open`, `no_outline`, `pad_no_copper`, `unknown_net`, `track_layer`,
    /// `hole_outside`.
    pub code: &'static str,
    /// What and where (board text is untrusted: callers label it as such).
    pub message: String,
}

/// Joining distance for outline ends (mm), as loose as fabrication tools accept.
pub const OUTLINE_TOL: f64 = 0.005;

/// Runs the checks; at most `max` issues are listed.
pub fn check(board: &Board, max: usize) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut push = |code, message: String| {
        if out.len() < max {
            out.push(Issue { code, message });
        }
    };
    let (loops, gaps) = geom::outline_loops(board, OUTLINE_TOL);
    if loops.is_empty() && gaps.is_empty() {
        push("no_outline", "the board has no outline on Edge.Cuts".into());
    }
    for g in gaps.chunks(2) {
        if let [a, b] = g {
            push(
                "outline_open",
                format!(
                    "the outline does not close: a gap from ({:.3}, {:.3}) to ({:.3}, {:.3}) mm",
                    a[0], a[1], b[0], b[1]
                ),
            );
        }
    }
    let cu = board.copper_layers();
    let fp_ref = |i: usize| {
        board.footprints.iter().find(|f| f.pads.contains(&i)).map(|f| f.reference.clone()).unwrap_or_default()
    };
    let net_known = |r: &NetRef| {
        r.is_none()
            || match (r.id, &r.name) {
                (Some(id), _) => board.nets.iter().any(|n| n.id == id),
                (None, Some(name)) => board.nets.is_empty() || board.nets.iter().any(|n| &n.name == name),
                (None, None) => true,
            }
    };
    for (i, p) in board.pads.iter().enumerate() {
        // Pads without copper and net are paste or mask apertures (allowed).
        if p.kind != PadKind::Npth && !p.net.is_none() && !cu.iter().any(|l| board.on_layer(&p.layers, l)) {
            push("pad_no_copper", format!("pad {} of {} is on no copper layer", p.number, fp_ref(i)));
        }
        if !net_known(&p.net) {
            push("unknown_net", format!("pad {} of {} names a net the board does not list", p.number, fp_ref(i)));
        }
    }
    for t in &board.tracks {
        if !cu.contains(&t.layer) {
            push("track_layer", format!("a track is on '{}', not a copper layer of the board", t.layer));
        }
        if !net_known(&t.net) {
            push("unknown_net", "a track names a net the board does not list".into());
        }
    }
    for v in &board.vias {
        if !net_known(&v.net) {
            push("unknown_net", "a via names a net the board does not list".into());
        }
    }
    // Holes outside the outer outline (the largest loop).
    let outer = loops
        .iter()
        .map(|c| c.polygon(0.01))
        .max_by(|a, b| geom::area(a).abs().partial_cmp(&geom::area(b).abs()).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(outer) = outer {
        let (plated, npth) = crate::drill::holes(board);
        let outside = plated.iter().chain(&npth).filter(|h| !geom::inside(&outer, h.at)).count();
        if outside > 0 {
            push("hole_outside", format!("{outside} holes lie outside the board outline"));
        }
    }
    out
}
