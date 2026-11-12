//! Excellon drill output: plated holes (through-hole pads and vias) and non-plated holes in
//! separate files, metric, decimal coordinates, oval holes as G85 slots.
//!
//! Written from the public Excellon description and common fabrication practice. Y is
//! negated like in the Gerber files so drills and copper line up.

use crate::board::{Board, Drill, PadKind, Place, Pt};
use std::fmt::Write as _;

/// One hole or slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole {
    /// Board coordinates (Y down).
    pub at: Pt,
    pub diameter: f64,
    /// The other end of a slot (`at` is the first end).
    pub slot_end: Option<Pt>,
    /// A via's hole.
    pub via: bool,
}

/// The holes of the board: (plated, non-plated).
pub fn holes(board: &Board) -> (Vec<Hole>, Vec<Hole>) {
    let mut plated = Vec::new();
    let mut npth = Vec::new();
    for pad in &board.pads {
        let Some(d) = pad.drill else { continue };
        let h = pad_hole(pad.at, pad.angle, &d);
        match pad.kind {
            PadKind::Npth => npth.push(h),
            // An SMD pad with a drill is unusual; KiCad treats holes in pads as plated.
            _ => plated.push(h),
        }
    }
    for v in &board.vias {
        if v.drill > 0.0 {
            plated.push(Hole { at: v.at, diameter: v.drill, slot_end: None, via: true });
        }
    }
    (plated, npth)
}

/// A pad's hole on the board: round, or a slot along the drill's longer side.
pub fn pad_hole(at: Pt, angle: f64, d: &Drill) -> Hole {
    let place = Place { at, angle };
    let [a, b] = d.size;
    let dia = a.min(b);
    if d.is_round() {
        return Hole { at: place.apply(d.offset), diameter: dia, slot_end: None, via: false };
    }
    let half = (a.max(b) - dia) / 2.0;
    let (p, q) = if a > b {
        ([d.offset[0] - half, d.offset[1]], [d.offset[0] + half, d.offset[1]])
    } else {
        ([d.offset[0], d.offset[1] - half], [d.offset[0], d.offset[1] + half])
    };
    Hole { at: place.apply(p), diameter: dia, slot_end: Some(place.apply(q)), via: false }
}

/// The Excellon file for `holes`. `plated` sets the X2-style file function comment.
pub fn write(board: &Board, holes: &[Hole], plated: bool) -> String {
    let n_cu = board.copper_layers().len().max(1);
    let mut tools: Vec<i64> = holes.iter().map(|h| um(h.diameter)).collect();
    tools.sort_unstable();
    tools.dedup();
    let mut s = String::new();
    s.push_str("M48\n");
    let _ = writeln!(s, "; DRILL file Gusset gusset-pcb {}", env!("CARGO_PKG_VERSION"));
    s.push_str("; FORMAT={-:-/ absolute / metric / decimal}\n");
    let _ = writeln!(
        s,
        "; #@! TF.FileFunction,{},1,{n_cu},{}",
        if plated { "Plated" } else { "NonPlated" },
        if plated { "PTH" } else { "NPTH" }
    );
    s.push_str("FMAT,2\nMETRIC\n");
    for (i, d) in tools.iter().enumerate() {
        let _ = writeln!(s, "T{}C{:.3}", i + 1, *d as f64 / 1000.0);
    }
    s.push_str("%\nG90\nG05\n");
    for (i, d) in tools.iter().enumerate() {
        let _ = writeln!(s, "T{}", i + 1);
        // Holes in a stable order (by position), then slots.
        let mut mine: Vec<&Hole> = holes.iter().filter(|h| um(h.diameter) == *d).collect();
        mine.sort_by(|a, b| {
            (a.slot_end.is_some(), a.at[0], a.at[1])
                .partial_cmp(&(b.slot_end.is_some(), b.at[0], b.at[1]))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for h in mine {
            match h.slot_end {
                None => {
                    let _ = writeln!(s, "{}", xy(h.at));
                }
                Some(e) => {
                    let _ = writeln!(s, "{}G85{}", xy(h.at), xy(e));
                }
            }
        }
    }
    s.push_str("M30\n");
    s
}

fn um(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

fn xy(p: Pt) -> String {
    let f = |v: f64| {
        let t = format!("{v:.4}");
        if t == "-0.0000" { "0.0000".to_string() } else { t }
    };
    format!("X{}Y{}", f(p[0]), f(-p[1]))
}
