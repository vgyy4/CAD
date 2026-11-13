//! More design rules (ADR 0048): hole to hole, drill sizes, courtyard overlap and silkscreen
//! over pads. Same report shape as [`crate::drc`].

use crate::board::{Board, Pt, Shape};
use crate::copper::{self, ItemRef, Prim};
use crate::drc::{SLACK, Violation, describe, mm};
use crate::drill::{self, Hole};
use crate::rules::Rules;
use crate::{geom, poly};

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

/// The drilled-hole rules: `drill_size`, `via_diameter` and `hole_to_hole`.
pub(crate) fn holes(board: &Board, rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    let list = drilled(board);
    drill_sizes(board, &list, rules, max, out);
    hole_to_hole(board, &list, rules, max, out);
}

/// All the rules on footprints' drawings: `courtyard_overlap`.
pub(crate) fn drawings(board: &Board, _rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    courtyards(board, max, out);
}

/// Closed polygons from a footprint's courtyard drawings: rectangles, polygons and circles
/// as drawn, lines and arcs chained end to end into loops (unclosed chains are dropped).
fn courtyard_loops(shapes: &[&Shape]) -> Vec<Vec<Pt>> {
    const JOIN: f64 = 1e-3;
    let near = |a: Pt, b: Pt| libm::hypot(a[0] - b[0], a[1] - b[1]) <= JOIN;
    let mut loops = Vec::new();
    let mut chains: Vec<Vec<Pt>> = Vec::new();
    for s in shapes {
        match s {
            Shape::Poly { pts } if pts.len() >= 3 => loops.push(pts.clone()),
            Shape::Circle { center, radius } if *radius > 0.0 => loops.push(poly::circle(*center, *radius)),
            Shape::Line { a, b } => chains.push(vec![*a, *b]),
            Shape::Arc { start, mid, end } => chains.push(geom::arc_points(*start, *mid, *end, 0.01)),
            _ => {}
        }
    }
    while let Some(mut cur) = chains.pop() {
        loop {
            let (Some(first), Some(last)) = (cur.first().copied(), cur.last().copied()) else { break };
            if cur.len() >= 3 && near(first, last) {
                cur.pop();
                loops.push(cur);
                break;
            }
            let next = chains
                .iter()
                .position(|c| c.first().is_some_and(|p| near(*p, last)) || c.last().is_some_and(|p| near(*p, last)));
            let Some(k) = next else { break };
            let mut c = chains.swap_remove(k);
            if c.first().is_some_and(|p| !near(*p, last)) {
                c.reverse();
            }
            cur.extend(c.into_iter().skip(1));
        }
    }
    loops
}

/// Courtyards of footprints on the same side that overlap (`courtyard_overlap`).
fn courtyards(board: &Board, max: usize, out: &mut Vec<Violation>) {
    /// Overlap smaller than this (mm²) is rounding where courtyards touch.
    const MIN_AREA: f64 = 1e-3;
    for layer in ["F.CrtYd", "B.CrtYd"] {
        let mut yards: Vec<(usize, poly::Areas, [Pt; 2])> = Vec::new();
        for (fi, _) in board.footprints.iter().enumerate() {
            let shapes: Vec<&Shape> = board
                .graphics
                .iter()
                .filter(|g| g.footprint == Some(fi) && g.layer == layer)
                .map(|g| &g.shape)
                .collect();
            let areas = poly::union(&courtyard_loops(&shapes));
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for p in areas.iter().flatten().flatten() {
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
            if !areas.is_empty() {
                yards.push((fi, areas, [lo, hi]));
            }
        }
        yards.sort_by(|a, b| a.2[0][0].total_cmp(&b.2[0][0]));
        for (k, (fa, aa, ba)) in yards.iter().enumerate() {
            for (fb, ab, bb) in yards.iter().skip(k + 1) {
                if bb[0][0] >= ba[1][0] {
                    break;
                }
                if out.len() >= max {
                    return;
                }
                if !copper::boxes_near(ba, bb, 0.0) {
                    continue;
                }
                let both = poly::intersect(aa, ab);
                let area = poly::total_area(&both);
                if area < MIN_AREA {
                    continue;
                }
                let (lo, hi) = ((*fa).min(*fb), (*fa).max(*fb));
                let pts: Vec<Pt> = both.iter().flatten().flatten().copied().collect();
                let n = pts.len().max(1) as f64;
                let at = [pts.iter().map(|p| p[0]).sum::<f64>() / n, pts.iter().map(|p| p[1]).sum::<f64>() / n];
                out.push(Violation {
                    code: "courtyard_overlap",
                    message: format!(
                        "courtyards of {} and {} overlap by {:.2} mm\u{b2}",
                        describe(board, ItemRef::Footprint(lo)),
                        describe(board, ItemRef::Footprint(hi)),
                        area
                    ),
                    at,
                    layer: Some(layer.to_string()),
                    items: vec![ItemRef::Footprint(lo), ItemRef::Footprint(hi)],
                });
            }
        }
    }
}

/// Pad holes narrower than the through-hole minimum (slots by their narrow side), via
/// drills under the via minimum, and via copper under the via diameter minimum.
fn drill_sizes(board: &Board, list: &[(ItemRef, Hole)], rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    for (r, h) in list {
        let (min, what) = match r {
            ItemRef::Via(_) => (rules.min_via_drill, "via drill"),
            _ => (rules.min_through_hole, "hole"),
        };
        if h.diameter < min - 1e-9 && out.len() < max {
            out.push(Violation {
                code: "drill_size",
                message: format!("{}: {what} {}, at least {} needed", describe(board, *r), mm(h.diameter), mm(min)),
                at: h.at,
                layer: None,
                items: vec![*r],
            });
        }
    }
    if rules.min_via_diameter <= 0.0 {
        return;
    }
    for (i, v) in board.vias.iter().enumerate() {
        if v.size < rules.min_via_diameter - 1e-9 && out.len() < max {
            out.push(Violation {
                code: "via_diameter",
                message: format!(
                    "{}: diameter {}, at least {} needed",
                    describe(board, ItemRef::Via(i)),
                    mm(v.size),
                    mm(rules.min_via_diameter)
                ),
                at: v.at,
                layer: None,
                items: vec![ItemRef::Via(i)],
            });
        }
    }
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
