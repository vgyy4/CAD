//! More design rules (ADR 0048): hole to hole, drill sizes, courtyard overlap and silkscreen
//! over pads. Same report shape as [`crate::drc`].

use crate::board::{Board, Pt};
use crate::copper::{self, ItemRef, Prim};
use crate::drc::{SLACK, Violation, describe, mm};
use crate::drill::{self, Hole};
use crate::rules::Rules;

/// Every drilled hole with the item it belongs to (pads' holes, vias' drills).
fn drilled(board: &Board) -> Vec<(ItemRef, Hole)> {
    let mut v = Vec::new();
    for (i, p) in board.pads.iter().enumerate() {
        if let Some(d) = p.drill {
            v.push((ItemRef::Pad(i), drill::pad_hole(p.at, p.angle, &d)));
        }
    }
    for (i, via) in board.vias.iter().enumerate() {
        if via.drill > 0.0 {
            v.push((ItemRef::Via(i), Hole { at: via.at, diameter: via.drill, slot_end: None, via: true }));
        }
    }
    v
}

fn hole_prim(h: &Hole) -> Prim {
    Prim::Capsule { a: h.at, b: h.slot_end.unwrap_or(h.at), r: h.diameter / 2.0 }
}

/// The drilled-hole rules: `hole_to_hole`.
pub fn holes(board: &Board, rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    let list = drilled(board);
    hole_to_hole(board, &list, rules, max, out);
}

/// Edge to edge of two holes closer than the rule (a sweep over X).
fn hole_to_hole(board: &Board, list: &[(ItemRef, Hole)], rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    let need = rules.hole_to_hole;
    if need <= 0.0 {
        return;
    }
    let boxes: Vec<[Pt; 2]> = list
        .iter()
        .map(|(_, h)| {
            let (a, b, r) = (h.at, h.slot_end.unwrap_or(h.at), h.diameter / 2.0);
            [[a[0].min(b[0]) - r, a[1].min(b[1]) - r], [a[0].max(b[0]) + r, a[1].max(b[1]) + r]]
        })
        .collect();
    let mut order: Vec<usize> = (0..list.len()).collect();
    let lo_x = |i: usize| boxes.get(i).map_or(0.0, |b| b[0][0]);
    order.sort_by(|a, b| lo_x(*a).total_cmp(&lo_x(*b)));
    for (k, &i) in order.iter().enumerate() {
        for &j in order.iter().skip(k + 1) {
            let (Some(bi), Some(bj)) = (boxes.get(i), boxes.get(j)) else { continue };
            if bj[0][0] > bi[1][0] + need {
                break;
            }
            if out.len() >= max {
                return;
            }
            if !copper::boxes_near(bi, bj, need) {
                continue;
            }
            let (Some((ra, ha)), Some((rb, hb))) = (list.get(i), list.get(j)) else { continue };
            let (g, at) = copper::gap(&hole_prim(ha), &hole_prim(hb));
            if g < need - SLACK {
                let (lo, hi) = if ra <= rb { (*ra, *rb) } else { (*rb, *ra) };
                out.push(Violation {
                    code: "hole_to_hole",
                    message: format!(
                        "holes of {} and {}: {} apart, {} needed",
                        describe(board, lo),
                        describe(board, hi),
                        mm(g.max(0.0)),
                        mm(need)
                    ),
                    at,
                    layer: None,
                    items: vec![lo, hi],
                });
            }
        }
    }
}
