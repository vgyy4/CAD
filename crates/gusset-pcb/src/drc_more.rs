//! More design rules (ADR 0048): hole to hole, drill sizes, courtyard overlap and silkscreen
//! over pads. Same report shape as [`crate::drc`].

use crate::board::{Board, Pt, Shape};
use crate::copper::{self, ItemRef, Prim};
use crate::drc::{SLACK, Violation, describe, mm};
use crate::drill::{self, Hole};
use crate::rules::Rules;
use crate::{font, geom, poly};

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
pub(crate) fn drawings(board: &Board, rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    courtyards(board, max, out);
    silk_over_pads(board, rules, max, out);
}

/// The exposed area of a pad on one side: its copper grown by the mask margin where the
/// pad has a mask opening on that side, else the copper itself (`None`: not on that side).
fn exposed(board: &Board, pad: &crate::board::Pad, side: &str) -> Option<Vec<Prim>> {
    let (cu, mask) = (format!("{side}.Cu"), format!("{side}.Mask"));
    let opening = board.on_layer(&pad.layers, &mask);
    if !opening && !board.on_layer(&pad.layers, &cu) {
        return None;
    }
    let grow = if opening { pad.mask_margin.unwrap_or(board.mask_margin) } else { 0.0 };
    let mut prims: Vec<Prim> =
        geom::pad_contour(pad, grow).map(|c| Prim::Poly { pts: c.polygon(0.005) }).into_iter().collect();
    for sh in &pad.primitives {
        match sh {
            Shape::Poly { pts } if pts.len() >= 3 => prims.push(Prim::Poly { pts: pts.clone() }),
            Shape::Circle { center, radius } => prims.push(Prim::Poly { pts: poly::circle(*center, *radius + grow) }),
            _ => {}
        }
    }
    (!prims.is_empty()).then_some(prims)
}

/// The ink of a silkscreen drawing: capsules along strokes, polygons for filled shapes.
fn ink(g: &crate::board::Graphic) -> Vec<Prim> {
    let r = g.width.max(0.01) / 2.0;
    let chain = |pts: &[Pt], closed: bool| -> Vec<Prim> {
        let mut v: Vec<Prim> = pts
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(Prim::Capsule { a: *a, b: *b, r }),
                _ => None,
            })
            .collect();
        if let (true, Some(f), Some(l)) = (closed, pts.first(), pts.last()) {
            v.push(Prim::Capsule { a: *l, b: *f, r });
        }
        v
    };
    match &g.shape {
        Shape::Line { a, b } => vec![Prim::Capsule { a: *a, b: *b, r }],
        Shape::Arc { start, mid, end } => chain(&geom::arc_points(*start, *mid, *end, 0.01), false),
        Shape::Circle { center, radius } if g.filled => vec![Prim::Poly { pts: poly::circle(*center, *radius + r) }],
        Shape::Circle { center, radius } => chain(&poly::circle(*center, *radius), true),
        Shape::Poly { pts } if g.filled && pts.len() >= 3 => vec![Prim::Poly { pts: pts.clone() }],
        Shape::Poly { pts } => chain(pts, true),
    }
}

/// Smallest gap between any of `ink` and any of the pad's `exposed` pieces.
fn closest(ink: &[Prim], exposed: &[Prim]) -> Option<(f64, Pt, bool)> {
    let mut best: Option<(f64, Pt, bool)> = None;
    for a in ink {
        for b in exposed {
            let (g, at) = copper::gap(a, b);
            // Two polygons that meet have a gap of zero: that is an overlap.
            let both_poly = matches!((a, b), (Prim::Poly { .. }, Prim::Poly { .. }));
            let g = if both_poly && g <= 0.0 { -1.0 } else { g };
            if best.is_none_or(|x| g < x.0) {
                best = Some((g, at, both_poly));
            }
        }
    }
    best
}

/// Silkscreen strokes and text over the exposed pads of their side: `silk_over_pad`.
fn silk_over_pads(board: &Board, rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    let need = rules.silk_clearance;
    let bbox = |prims: &[Prim]| -> [Pt; 2] {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in prims {
            let (pts, r): (Vec<Pt>, f64) = match p {
                Prim::Capsule { a, b, r } => (vec![*a, *b], *r),
                Prim::Poly { pts } => (pts.clone(), 0.0),
            };
            for q in pts {
                lo = [lo[0].min(q[0] - r), lo[1].min(q[1] - r)];
                hi = [hi[0].max(q[0] + r), hi[1].max(q[1] + r)];
            }
        }
        [lo, hi]
    };
    for side in ["F", "B"] {
        let layer = format!("{side}.SilkS");
        let pads: Vec<(usize, Vec<Prim>, [Pt; 2])> = board
            .pads
            .iter()
            .enumerate()
            .filter_map(|(i, p)| exposed(board, p, side).map(|e| (i, e.clone(), bbox(&e))))
            .collect();
        if pads.is_empty() {
            continue;
        }
        let mut marks: Vec<(ItemRef, Vec<Prim>)> = Vec::new();
        for (i, g) in board.graphics.iter().enumerate().filter(|(_, g)| g.layer == layer) {
            marks.push((ItemRef::Graphic(i), ink(g)));
        }
        for (i, t) in board.texts.iter().enumerate().filter(|(_, t)| t.layer == layer && !t.hidden) {
            let fp = t.footprint.and_then(|k| board.footprints.get(k));
            let text = font::shown(&t.text, fp.map_or("", |f| &f.reference), fp.map_or("", |f| &f.value));
            let r = font::thickness(t) / 2.0;
            let prims: Vec<Prim> = font::strokes(t, &text)
                .iter()
                .flat_map(|line| match line.as_slice() {
                    [p] => vec![Prim::Capsule { a: *p, b: *p, r }],
                    l => l
                        .windows(2)
                        .filter_map(|w| match w {
                            [a, b] => Some(Prim::Capsule { a: *a, b: *b, r }),
                            _ => None,
                        })
                        .collect(),
                })
                .collect();
            marks.push((ItemRef::Text(i), prims));
        }
        for (what, prims) in &marks {
            if prims.is_empty() {
                continue;
            }
            let bb = bbox(prims);
            for (pi, e, eb) in &pads {
                if out.len() >= max {
                    return;
                }
                if !copper::boxes_near(&bb, eb, need) {
                    continue;
                }
                let Some((g, at, _)) = closest(prims, e) else { continue };
                if g < need - SLACK {
                    out.push(Violation {
                        code: "silk_over_pad",
                        message: if g <= 0.0 {
                            format!("{} covers {}", describe(board, *what), describe(board, ItemRef::Pad(*pi)))
                        } else {
                            format!(
                                "{} is {} from {}, {} needed",
                                describe(board, *what),
                                mm(g),
                                describe(board, ItemRef::Pad(*pi)),
                                mm(need)
                            )
                        },
                        at,
                        layer: Some(layer.clone()),
                        items: vec![*what, ItemRef::Pad(*pi)],
                    });
                }
            }
        }
    }
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
