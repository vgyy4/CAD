//! Zone fills (ADR 0047 phase 2b): the copper pour of each zone computed from its outline,
//! net and settings, written back as KiCad's `filled_polygon`s.
//!
//! Per zone and copper layer, highest priority first:
//! 1. the zone's outline, inside the board outline shrunk by the copper-to-edge rule;
//! 2. minus the copper of other nets (pads, tracks, vias, copper drawings, fills of zones
//!    filled before) grown by the clearance (the zone's, or the larger net class one),
//!    non-plated holes grown by it, and keep-out areas that forbid pours;
//! 3. minus the pads of its own net grown by the thermal gap (thermal reliefs), or by the
//!    clearance when the zone does not join them; solid joins leave them in;
//! 4. opened by the minimum width (shrunk and grown by half of it), so thinner necks go;
//! 5. thermal spokes (the zone's bridge width) along the pad's axes (45° for round pads)
//!    where they reach the fill, clipped clear of other nets again;
//! 6. islands that touch no copper of the zone's net removed (or those under the minimum
//!    area, as the zone says).
//!
//! Hatched fills are filled solid (noted). Copper text is not an obstacle yet.

use crate::board::{Board, Error, Limits, PadKind, PadShape, Place, Pt, Shape, ZoneConnect};
use crate::copper::{Copper, EdgeGrid, ItemRef, Prim};
use crate::geom::{self, Contour};
use crate::poly::{self, Areas};
use crate::rules::Rules;
use crate::sexpr::Node;

/// Extra growth of obstacles over the clearance (mm): covers chord error of round shapes
/// and file rounding, so fills pass a check with no slack.
const MARGIN: f64 = 0.006;

/// The fill of one zone on one layer.
#[derive(Debug, Clone)]
pub struct Fill {
    pub zone: usize,
    pub layer: String,
    pub areas: Areas,
}

/// The result of filling: the fills, and notes for the user.
#[derive(Debug, Clone, Default)]
pub struct Filled {
    pub fills: Vec<Fill>,
    pub notes: Vec<String>,
}

fn bbox(pts: &[Pt]) -> [Pt; 2] {
    pts.iter().fold([[f64::MAX; 2], [f64::MIN; 2]], |[lo, hi], p| {
        [[lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]]
    })
}

fn near(a: &[Pt; 2], b: &[Pt; 2], m: f64) -> bool {
    crate::copper::boxes_near(a, b, m)
}

/// A contour as a polygon whose edges lie on or outside it (`grow` added to make up for
/// chords cut inside arcs).
fn contour_poly(c: &Contour) -> Vec<Pt> {
    match c {
        Contour::Circle { center, radius } => poly::circle(*center, *radius),
        c => c.polygon(poly::TOL),
    }
}

/// A pad's copper grown by `d`, as polygons.
fn pad_polys(board: &Board, i: usize, d: f64) -> Vec<Vec<Pt>> {
    let Some(pad) = board.pads.get(i) else { return Vec::new() };
    let mut out = Vec::new();
    // Rounded shapes' chords lie up to TOL inside: grow by that too.
    if let Some(c) = geom::pad_contour(pad, d + poly::TOL) {
        out.push(contour_poly(&c));
    }
    for s in &pad.primitives {
        match s {
            Shape::Poly { pts } if pts.len() >= 3 => {
                out.extend(poly::offset(&poly::union(std::slice::from_ref(pts)), d + poly::TOL).into_iter().flatten());
            }
            Shape::Circle { center, radius } => out.push(poly::circle(*center, radius + d)),
            _ => {}
        }
    }
    out
}

/// A piece of copper grown by `d`.
fn prim_polys(p: &Prim, d: f64) -> Vec<Vec<Pt>> {
    match p {
        Prim::Capsule { a, b, r } => vec![poly::capsule(*a, *b, r + d)],
        Prim::Poly { pts } => {
            poly::offset(&poly::union(std::slice::from_ref(pts)), d + poly::TOL).into_iter().flatten().collect()
        }
    }
}

/// A copper drawing (stroke or filled shape) grown by `d`.
fn graphic_polys(s: &Shape, width: f64, filled: bool, d: f64) -> Vec<Vec<Pt>> {
    let r = width / 2.0 + d;
    match s {
        Shape::Line { a, b } => vec![poly::capsule(*a, *b, r)],
        Shape::Arc { start, mid, end } => geom::arc_points(*start, *mid, *end, poly::TOL)
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(poly::capsule(*a, *b, r)),
                _ => None,
            })
            .collect(),
        Shape::Circle { center, radius } if filled => vec![poly::circle(*center, radius + r)],
        Shape::Circle { center, radius } => {
            let ring = poly::circle(*center, radius + r);
            if *radius - r > 0.0 {
                // A ring: the outer circle minus the inner disc.
                let inner = Contour::Circle { center: *center, radius: radius - r }.polygon(poly::TOL);
                poly::difference(&poly::union(&[ring]), &poly::union(&[inner])).into_iter().flatten().collect()
            } else {
                vec![ring]
            }
        }
        Shape::Poly { pts } => {
            let mut v = Vec::new();
            if filled {
                v.extend(poly::offset(&poly::union(std::slice::from_ref(pts)), r + poly::TOL).into_iter().flatten());
            }
            let n = pts.len();
            if width > 0.0 || !filled {
                for k in 0..n {
                    if let (Some(a), Some(b)) = (pts.get(k), pts.get((k + 1) % n)) {
                        v.push(poly::capsule(*a, *b, r));
                    }
                }
            }
            v
        }
    }
}

/// The board's area: inside the outline, cut-outs taken away, shrunk by `edge`.
fn board_area(board: &Board, edge: f64) -> Option<Areas> {
    let (loops, _) = geom::outline_loops(board, crate::check::OUTLINE_TOL);
    if loops.is_empty() {
        return None;
    }
    let contours: Vec<Vec<Pt>> = loops.iter().map(|l| l.polygon(poly::TOL)).collect();
    Some(poly::offset(&poly::even_odd(&contours), -edge))
}

/// The copper layers a zone covers.
fn zone_layers(board: &Board, layers: &[String]) -> Vec<String> {
    board.copper_layers().into_iter().filter(|l| board.on_layer(layers, l)).collect()
}

/// Fills the zones of `board` (those in `only`, else every copper zone) with `rules`.
pub fn fill(board: &Board, rules: &Rules, only: Option<&[usize]>) -> Filled {
    let mut out = Filled::default();
    let copper = Copper::of(board);
    let cu = &copper.layers;
    // Holes without copper of their own (plated holes sit in their pad's or via's copper).
    let npth: Vec<crate::drill::Hole> = board
        .pads
        .iter()
        .filter(|p| p.kind == PadKind::Npth)
        .filter_map(|p| p.drill.map(|d| crate::drill::pad_hole(p.at, p.angle, &d)))
        .collect();
    let board_area = board_area(board, rules.edge_clearance);
    if board_area.is_none() {
        out.notes.push("the board has no closed outline: zones are not clipped to it".into());
    }
    // Highest priority first; the file's order among equals.
    let mut order: Vec<usize> = (0..board.zones.len())
        .filter(|&i| board.zones.get(i).is_some_and(|z| !z.keepout && z.outline.len() >= 3))
        .collect();
    order.sort_by_key(|&i| std::cmp::Reverse(board.zones.get(i).map_or(0, |z| z.settings.priority)));
    let pad_tht = |i: usize| board.pads.get(i).is_some_and(|p| p.kind == PadKind::Tht);
    let mut done: Vec<Fill> = Vec::new();
    for zi in order {
        let Some(z) = board.zones.get(zi) else { continue };
        let net = board.net_name(&z.net);
        let st = z.settings;
        if board
            .tree
            .items()
            .get(z.node)
            .and_then(|n| n.child("fill"))
            .and_then(|f| f.child("mode"))
            .and_then(|m| m.arg(0))
            == Some("hatch")
        {
            out.notes.push(format!("zone {zi} is hatched in KiCad; it was filled solid"));
        }
        let outline = poly::union(std::slice::from_ref(&z.outline));
        let zb = bbox(&z.outline);
        let reach = st.clearance.max(rules.clearance_of(net.as_deref())) + 1.0;
        for layer in zone_layers(board, &z.layers) {
            let Some(li) = cu.iter().position(|l| *l == layer) else { continue };
            let mut area = outline.clone();
            if let Some(b) = &board_area {
                area = poly::intersect(&area, b);
            }
            // Obstacles: other nets' copper, holes, keep-outs, earlier fills.
            let mut obst: Vec<Vec<Pt>> = Vec::new();
            let mut relief: Vec<Vec<Pt>> = Vec::new();
            let mut thermals: Vec<usize> = Vec::new();
            let clear_to = |other: Option<&str>| {
                st.clearance.max(rules.clearance_of(other)).max(rules.clearance_of(net.as_deref())) + MARGIN
            };
            for it in copper.items.iter().filter(|it| it.layers.contains(&li) && near(&it.bbox, &zb, reach + 1.0)) {
                let same = net.is_some() && it.net == net;
                if !same {
                    let d = clear_to(it.net.as_deref());
                    match it.what {
                        ItemRef::Pad(p) => obst.extend(pad_polys(board, p, d)),
                        _ => it.prims.iter().for_each(|p| obst.extend(prim_polys(p, d))),
                    }
                    continue;
                }
                let ItemRef::Pad(p) = it.what else { continue };
                let mode = board.pads.get(p).and_then(|x| x.zone_connect).unwrap_or(st.connect);
                let mode = match mode {
                    ZoneConnect::ThtThermal if pad_tht(p) => ZoneConnect::Thermal,
                    ZoneConnect::ThtThermal => ZoneConnect::Solid,
                    m => m,
                };
                match mode {
                    ZoneConnect::Solid | ZoneConnect::ThtThermal => {}
                    ZoneConnect::None => obst.extend(pad_polys(board, p, clear_to(None))),
                    ZoneConnect::Thermal => {
                        relief.extend(pad_polys(board, p, st.thermal_gap));
                        thermals.push(p);
                    }
                }
            }
            for h in &npth {
                let d = clear_to(None);
                let r = h.diameter / 2.0;
                if near(&[[h.at[0] - r, h.at[1] - r], [h.at[0] + r, h.at[1] + r]], &zb, d + 1.0) {
                    obst.push(poly::capsule(h.at, h.slot_end.unwrap_or(h.at), r + d));
                }
            }
            for g in board.graphics.iter().filter(|g| g.layer == layer) {
                obst.extend(graphic_polys(&g.shape, g.width, g.filled, clear_to(None)));
            }
            for k in board.zones.iter().filter(|k| k.keepout && k.settings.no_pour && k.outline.len() >= 3) {
                if board.on_layer(&k.layers, &layer) {
                    obst.push(k.outline.clone());
                }
            }
            for f in done.iter().filter(|f| f.layer == layer) {
                let other = board.zones.get(f.zone).and_then(|o| board.net_name(&o.net));
                if net.is_some() && other == net {
                    continue;
                }
                let d = clear_to(other.as_deref());
                obst.extend(poly::offset(&f.areas, d + poly::TOL).into_iter().flatten());
            }
            let obstacles = poly::union(&obst);
            let mut fill = poly::difference(&area, &obstacles);
            if !relief.is_empty() {
                fill = poly::difference(&fill, &poly::union(&relief));
            }
            // Thinner than the minimum width goes.
            let half = st.min_width / 2.0;
            if half > 0.0 {
                let opened = poly::offset(&poly::offset(&fill, -half), half);
                fill = poly::intersect(&opened, &fill);
            }
            // Thermal spokes that reach the fill.
            let mut spokes: Vec<Vec<Pt>> = Vec::new();
            for &p in &thermals {
                let Some(pad) = board.pads.get(p) else { continue };
                spokes.extend(spokes_of(pad, &st, &fill));
            }
            if !spokes.is_empty() {
                fill = poly::join(&fill, &poly::union(&spokes));
                fill = poly::difference(&fill, &obstacles);
                fill = poly::intersect(&fill, &area);
            }
            let kept = islands(&copper, li, net.as_deref(), &st, fill);
            done.push(Fill { zone: zi, layer: layer.clone(), areas: kept });
        }
    }
    out.fills = done.into_iter().filter(|f| only.is_none_or(|o| o.contains(&f.zone))).collect();
    out
}

/// The spokes of a thermal relief: bars from the pad's centre along its axes (45° turned
/// for round pads) out past the gap, kept where their ends land in the fill.
fn spokes_of(pad: &crate::board::Pad, st: &crate::board::ZoneSettings, fill: &Areas) -> Vec<Vec<Pt>> {
    let place = Place { at: pad.at, angle: pad.angle };
    let round = matches!(pad.shape, PadShape::Circle | PadShape::Custom { anchor_circle: true });
    let (hx, hy) = (pad.size[0] / 2.0, pad.size[1] / 2.0);
    let w = st.thermal_width.max(1e-3);
    let mut out = Vec::new();
    for k in 0..4 {
        let a = std::f64::consts::FRAC_PI_2 * f64::from(k) + if round { std::f64::consts::FRAC_PI_4 } else { 0.0 };
        let (c, s) = (libm::cos(a), libm::sin(a));
        let reach = if round || k % 2 == 0 { hx } else { hy };
        let probe = reach + st.thermal_gap + st.min_width.max(w) / 2.0;
        let tip = reach + st.thermal_gap + st.min_width.max(w);
        let at = |d: f64| place.apply([c * d, s * d]);
        if fill.iter().any(|ar| poly::contains(ar, at(probe))) {
            out.push(poly::bar(pad.at, at(tip), w));
        }
    }
    out
}

/// The parts of a fill to keep: those touching copper of the zone's net on the layer, or
/// all of them for a zone without a net or one that keeps islands; mode 2 keeps islands
/// of at least the minimum area.
fn islands(copper: &Copper, layer: usize, net: Option<&str>, st: &crate::board::ZoneSettings, fill: Areas) -> Areas {
    let Some(net) = net else { return fill };
    if st.island_mode == 1 {
        return fill;
    }
    let mine: Vec<&crate::copper::Item> =
        copper.items.iter().filter(|it| it.net.as_deref() == Some(net) && it.layers.contains(&layer)).collect();
    fill.into_iter()
        .filter(|a| {
            let flat = poly::fracture(a);
            if flat.len() < 3 {
                return false;
            }
            let grid = EdgeGrid::new(&flat);
            let joined = mine
                .iter()
                .filter(|it| near(&it.bbox, &grid.bbox, 1e-3))
                .any(|it| it.prims.iter().any(|p| grid.touches(p, 1e-4)));
            joined || (st.island_mode == 2 && poly::area_of(a) >= st.island_min_area)
        })
        .collect()
}

impl Board {
    /// Replaces the fills of the zones named in `fills` (their `filled_polygon`s and
    /// KiCad 5 `fill_segments`) with the given areas, each written as one polygon with its
    /// holes bridged in, and marks them filled.
    ///
    /// # Errors
    /// A failed re-read, or a zone past the list.
    pub fn set_zone_fills(&mut self, fills: &[Fill], limits: &Limits) -> Result<usize, Error> {
        let mut written = 0usize;
        let mut zones: Vec<usize> = fills.iter().map(|f| f.zone).collect();
        zones.sort_unstable();
        zones.dedup();
        for zi in zones {
            let z = self.zones.get(zi).ok_or_else(|| Error::Edit(format!("there is no zone {zi}")))?;
            let node = z.node;
            let single_layer = z.layers.len() == 1 && self.version.is_some_and(|v| v < 20_210_000);
            let Some(n) = self.tree.items_mut().and_then(|v| v.get_mut(node)) else { continue };
            let Some(items) = n.items_mut() else { continue };
            items.retain(|c| !matches!(c.head(), Some("filled_polygon" | "fill_segments")));
            // `(fill yes ...)`.
            match items.iter_mut().find(|c| c.head() == Some("fill")).and_then(Node::items_mut) {
                Some(f) => {
                    if f.get(1).and_then(Node::atom) != Some("yes") {
                        f.insert(1, Node::sym("yes"));
                    }
                }
                None => items.push(Node::List(vec![Node::sym("fill"), Node::sym("yes")])),
            }
            for f in fills.iter().filter(|f| f.zone == zi) {
                for a in &f.areas {
                    let pts = poly::fracture(a);
                    if pts.len() < 3 {
                        continue;
                    }
                    let mut list = vec![Node::sym("pts")];
                    list.extend(pts.iter().map(|p| {
                        Node::List(vec![
                            Node::sym("xy"),
                            Node::num((p[0] * 1e6).round() / 1e6),
                            Node::num((p[1] * 1e6).round() / 1e6),
                        ])
                    }));
                    let mut fp = vec![Node::sym("filled_polygon")];
                    if !single_layer {
                        fp.push(Node::List(vec![Node::sym("layer"), Node::str(f.layer.clone())]));
                    }
                    fp.push(Node::List(list));
                    items.push(Node::List(fp));
                    written += 1;
                }
            }
        }
        let tree = std::mem::replace(&mut self.tree, Node::List(Vec::new()));
        *self = Board::from_tree(tree, limits)?;
        Ok(written)
    }
}

/// The area of every fill, mm² (for tests and reports).
pub fn fill_area(f: &Fill) -> f64 {
    poly::total_area(&f.areas)
}
