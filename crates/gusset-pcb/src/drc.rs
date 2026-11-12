//! A basic design rule check (ADR 0047 phase 1b): copper clearance per layer between
//! different nets (the board's clearance, or the larger of the two nets' classes), track
//! width, annular rings of vias and plated pads, holes and copper near the board edge.
//! Zones are not checked (their fills are KiCad's result) and neither is copper inside
//! one footprint's own pads of one net.

use crate::board::{Board, PadKind, PadShape, Pt};
use crate::copper::{self, Copper, ItemRef, Prim};
use crate::rules::Rules;

/// One rule broken somewhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// `clearance`, `short`, `track_width`, `annular_ring`, `hole_to_edge`, `copper_to_edge`.
    pub code: &'static str,
    pub message: String,
    /// Where (board mm, Y down).
    pub at: Pt,
    pub layer: Option<String>,
    /// The items involved.
    pub items: Vec<ItemRef>,
}

/// What a gap may fall short by and still pass (mm): rounding in the file and in arc
/// chords, well below any fabrication tolerance.
const SLACK: f64 = 5e-4;

/// Rounds for messages (µm).
fn mm(v: f64) -> String {
    format!("{:.3} mm", (v * 1000.0).round() / 1000.0)
}

/// A short name of an item for messages.
pub fn describe(board: &Board, it: ItemRef) -> String {
    match it {
        ItemRef::Pad(i) => {
            let fp = board.footprints.iter().find(|f| f.pads.contains(&i));
            let num = board.pads.get(i).map(|p| p.number.as_str()).unwrap_or_default();
            match fp {
                Some(f) => format!("pad {}.{num}", f.reference),
                None => format!("pad {num}"),
            }
        }
        ItemRef::Track(i) => format!("track {i}"),
        ItemRef::Via(i) => format!("via {i}"),
    }
}

/// Checks `board` against `rules`; at most `max` violations.
pub fn check(board: &Board, rules: &Rules, max: usize) -> Vec<Violation> {
    let mut out = Vec::new();
    let copper = Copper::of(board);
    let items = &copper.items;
    let widest =
        rules.classes.iter().filter_map(|c| c.clearance).fold(rules.clearance, f64::max).max(rules.edge_clearance);
    // Copper of different nets.
    for (i, j) in copper::near_pairs(items, widest) {
        if out.len() >= max {
            return out;
        }
        let (Some(a), Some(b)) = (items.get(i), items.get(j)) else { continue };
        if a.net.is_some() && a.net == b.net {
            continue;
        }
        let need = rules.clearance_of(a.net.as_deref()).max(rules.clearance_of(b.net.as_deref()));
        if !copper::boxes_near(&a.bbox, &b.bbox, need) {
            continue;
        }
        let (g, at) = copper::item_gap(a, b);
        if g < need - SLACK {
            let layer = a.layers.iter().find(|l| b.layers.contains(l)).and_then(|l| copper.layers.get(*l)).cloned();
            let (da, db) = (describe(board, a.what), describe(board, b.what));
            let (code, message) = if g <= 0.0 {
                ("short", format!("{da} and {db} of different nets touch"))
            } else {
                ("clearance", format!("{da} and {db}: {} apart, {} needed", mm(g), mm(need)))
            };
            out.push(Violation { code, message, at, layer, items: vec![a.what, b.what] });
        }
    }
    for (i, t) in board.tracks.iter().enumerate() {
        if t.width < rules.min_track_width - 1e-9 && out.len() < max {
            out.push(Violation {
                code: "track_width",
                message: format!("track {i} is {} wide, at least {} needed", mm(t.width), mm(rules.min_track_width)),
                at: [(t.start[0] + t.end[0]) / 2.0, (t.start[1] + t.end[1]) / 2.0],
                layer: Some(t.layer.clone()),
                items: vec![ItemRef::Track(i)],
            });
        }
    }
    annular_rings(board, rules, max, &mut out);
    edges(board, rules, &copper, max, &mut out);
    out.truncate(max);
    out
}

fn annular_rings(board: &Board, rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    for (i, v) in board.vias.iter().enumerate() {
        let ring = (v.size - v.drill) / 2.0;
        if ring < rules.min_annular_ring - 1e-9 && out.len() < max {
            out.push(Violation {
                code: "annular_ring",
                message: format!("via {i}: ring {}, at least {} needed", mm(ring), mm(rules.min_annular_ring)),
                at: v.at,
                layer: None,
                items: vec![ItemRef::Via(i)],
            });
        }
    }
    for (i, p) in board.pads.iter().enumerate() {
        if p.kind != PadKind::Tht || matches!(p.shape, PadShape::Custom { .. }) {
            continue;
        }
        let Some(d) = p.drill else { continue };
        // The narrowest copper beside the hole, along the pad's axes.
        let off = [d.offset[0].abs(), d.offset[1].abs()];
        let ring = ((p.size[0] - d.size[0]) / 2.0 - off[0]).min((p.size[1] - d.size[1]) / 2.0 - off[1]);
        if ring < rules.min_annular_ring - 1e-9 && out.len() < max {
            let who = describe(board, ItemRef::Pad(i));
            out.push(Violation {
                code: "annular_ring",
                message: format!("{who}: ring {}, at least {} needed", mm(ring), mm(rules.min_annular_ring)),
                at: p.at,
                layer: None,
                items: vec![ItemRef::Pad(i)],
            });
        }
    }
}

/// Holes and copper near the board outline.
fn edges(board: &Board, rules: &Rules, copper: &Copper, max: usize, out: &mut Vec<Violation>) {
    let (loops, _) = crate::geom::outline_loops(board, crate::check::OUTLINE_TOL);
    let mut segs: Vec<(Pt, Pt)> = Vec::new();
    for l in &loops {
        let p = l.polygon(0.01);
        let n = p.len();
        segs.extend((0..n).filter_map(|i| Some((*p.get(i)?, *p.get((i + 1) % n)?))));
    }
    if segs.is_empty() {
        return;
    }
    let near_edge = |prim: &Prim| -> (f64, Pt) {
        let mut best = (f64::INFINITY, [0.0, 0.0]);
        for (a, b) in &segs {
            let g = copper::gap(prim, &Prim::Capsule { a: *a, b: *b, r: 0.0 });
            if g.0 < best.0 {
                best = g;
            }
        }
        best
    };
    let (plated, npth) = crate::drill::holes(board);
    for h in plated.iter().chain(&npth) {
        if out.len() >= max {
            return;
        }
        let prim = Prim::Capsule { a: h.at, b: h.slot_end.unwrap_or(h.at), r: h.diameter / 2.0 };
        let (g, at) = near_edge(&prim);
        if g < rules.hole_to_edge - SLACK {
            out.push(Violation {
                code: "hole_to_edge",
                message: format!(
                    "a {} hole is {} from the board edge, {} needed",
                    mm(h.diameter),
                    mm(g.max(0.0)),
                    mm(rules.hole_to_edge)
                ),
                at,
                layer: None,
                items: Vec::new(),
            });
        }
    }
    let (lo, hi) = segs.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), (a, b)| {
        ([lo[0].min(a[0]).min(b[0]), lo[1].min(a[1]).min(b[1])], [hi[0].max(a[0]).max(b[0]), hi[1].max(a[1]).max(b[1])])
    });
    let m = rules.edge_clearance;
    for it in &copper.items {
        if out.len() >= max {
            return;
        }
        // Only items whose box comes near an outline piece (cut-outs included), or that
        // reach past the outline's box, can be too close to the edge.
        let near = segs.iter().any(|(a, b)| {
            let sb = [[a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]];
            copper::boxes_near(&it.bbox, &sb, m)
        });
        let outside = it.bbox[0][0] < lo[0] || it.bbox[0][1] < lo[1] || it.bbox[1][0] > hi[0] || it.bbox[1][1] > hi[1];
        if !near && !outside {
            continue;
        }
        let (g, at) =
            it.prims.iter().map(near_edge).fold((f64::INFINITY, [0.0, 0.0]), |b, x| if x.0 < b.0 { x } else { b });
        if g < m - SLACK {
            let who = describe(board, it.what);
            out.push(Violation {
                code: "copper_to_edge",
                message: format!("{who} is {} from the board edge, {} needed", mm(g.max(0.0)), mm(m)),
                at,
                layer: it.layers.first().and_then(|l| copper.layers.get(*l)).cloned(),
                items: vec![it.what],
            });
        }
    }
}
