//! A basic design rule check (ADR 0047 phase 1b): copper clearance per layer between
//! different nets (the board's clearance, or the larger of the two nets' classes), track
//! width, annular rings of vias and plated pads, holes and copper near the board edge.
//! Zone fills are checked against other nets' copper and each other; copper inside one
//! footprint's own pads of one net is not.

use crate::board::{Board, PadKind, PadShape, Pt, ZoneConnect};
use crate::copper::{self, Copper, ItemRef, Prim};
use crate::dru::{Kind, Subject};
use crate::geom;
use crate::rules::Rules;

/// One rule broken somewhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// `clearance`, `short`, `track_width`, `annular_ring`, `hole_to_edge`, `copper_to_edge`, and ADR 0048's
    /// `hole_to_hole`, `drill_size`, `courtyard_overlap`, `silk_over_pad`, and ADR 0053's `starved_thermal`.
    pub code: &'static str,
    pub message: String,
    /// Where (board mm, Y down).
    pub at: Pt,
    pub layer: Option<String>,
    /// The items involved.
    pub items: Vec<ItemRef>,
    /// The custom rule (`.kicad_dru`, ADR 0054) that set the limit, when one did.
    pub rule: Option<String>,
}

/// What a gap may fall short by and still pass (mm): rounding in the file and in arc
/// chords, well below any fabrication tolerance.
pub(crate) const SLACK: f64 = 5e-4;

/// Rounds for messages (µm).
pub(crate) fn mm(v: f64) -> String {
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
        ItemRef::Footprint(i) => match board.footprints.get(i) {
            Some(f) => format!("footprint {}", f.reference),
            None => format!("footprint {i}"),
        },
        ItemRef::Graphic(i) => format!("silkscreen line {i}"),
        ItemRef::Text(i) => match board.texts.get(i) {
            Some(t) => format!("text \"{}\"", t.text.chars().take(30).collect::<String>()),
            None => format!("text {i}"),
        },
        ItemRef::Zone(i) => match board.zones.get(i).and_then(|z| board.net_name(&z.net)) {
            Some(n) => format!("zone {i} ({n})"),
            None => format!("zone {i}"),
        },
    }
}

/// How custom rules see an item: its type, net, class and the copper layers (ADR 0054).
pub(crate) fn subject(rules: &Rules, what: ItemRef, net: Option<&str>, layers: Vec<String>) -> Subject {
    let kind = match what {
        ItemRef::Track(_) => "track",
        ItemRef::Via(_) => "via",
        ItemRef::Pad(_) => "pad",
        ItemRef::Zone(_) => "zone",
        _ => "",
    };
    Subject {
        net_class: net.and_then(|n| rules.class_of(n)).map_or_else(|| "Default".to_string(), |c| c.name.clone()),
        net_name: net.unwrap_or_default().to_string(),
        kind,
        layers,
    }
}

/// The custom rule governing the clearance between two copper items on a layer: its
/// minimum and the rule's name.
pub(crate) fn custom_clearance(
    rules: &Rules,
    a: (ItemRef, Option<&str>),
    b: (ItemRef, Option<&str>),
    layer: Option<&str>,
    layers_of: impl Fn(ItemRef) -> Vec<String>,
) -> Option<(f64, String)> {
    if !rules.custom.has(Kind::Clearance) {
        return None;
    }
    let ls = |w: ItemRef| layer.map_or_else(|| layers_of(w), |l| vec![l.to_string()]);
    let (sa, sb) = (subject(rules, a.0, a.1, ls(a.0)), subject(rules, b.0, b.1, ls(b.0)));
    let (r, c) = rules.custom.find(Kind::Clearance, &sa, Some(&sb))?;
    Some((c.min?, r.name.clone()))
}

/// Appends the custom rule's name to a message.
pub(crate) fn named(message: String, rule: &Option<String>) -> String {
    match rule {
        Some(r) => format!("{message} (rule \"{r}\")"),
        None => message,
    }
}

/// Checks `board` against `rules`; at most `max` violations.
pub fn check(board: &Board, rules: &Rules, max: usize) -> Vec<Violation> {
    let mut out = Vec::new();
    let copper = Copper::of(board);
    let items = &copper.items;
    let widest = rules
        .classes
        .iter()
        .filter_map(|c| c.clearance)
        .fold(rules.clearance, f64::max)
        .max(rules.edge_clearance)
        .max(rules.custom.largest_min(Kind::Clearance));
    let layer_names = |w: ItemRef| -> Vec<String> {
        items
            .iter()
            .find(|i| i.what == w)
            .map_or_else(Vec::new, |i| i.layers.iter().filter_map(|l| copper.layers.get(*l).cloned()).collect())
    };
    // Copper of different nets.
    for (i, j) in copper::near_pairs(items, widest) {
        if out.len() >= max {
            return out;
        }
        let (Some(a), Some(b)) = (items.get(i), items.get(j)) else { continue };
        if a.net.is_some() && a.net == b.net {
            continue;
        }
        let mut need = rules.clearance_of(a.net.as_deref()).max(rules.clearance_of(b.net.as_deref()));
        let layer = a.layers.iter().find(|l| b.layers.contains(l)).and_then(|l| copper.layers.get(*l)).cloned();
        let mut rule = None;
        if let Some((v, name)) = custom_clearance(
            rules,
            (a.what, a.net.as_deref()),
            (b.what, b.net.as_deref()),
            layer.as_deref(),
            layer_names,
        ) {
            (need, rule) = (v, Some(name));
        }
        if !copper::boxes_near(&a.bbox, &b.bbox, need) {
            continue;
        }
        let (g, at) = copper::item_gap(a, b);
        if g < need - SLACK {
            let (da, db) = (describe(board, a.what), describe(board, b.what));
            let (code, message) = if g <= 0.0 {
                ("short", format!("{da} and {db} of different nets touch"))
            } else {
                ("clearance", format!("{da} and {db}: {} apart, {} needed", mm(g), mm(need)))
            };
            let message = named(message, &rule);
            out.push(Violation { code, message, at, layer, items: vec![a.what, b.what], rule });
        }
    }
    track_widths(board, rules, max, &mut out);
    zones(board, rules, &copper, max, &mut out);
    starved_thermals(board, rules, &copper, max, &mut out);
    annular_rings(board, rules, max, &mut out);
    edges(board, rules, &copper, max, &mut out);
    crate::drc_more::holes(board, rules, max, &mut out);
    crate::drc_more::drawings(board, rules, max, &mut out);
    out.truncate(max);
    out
}

/// Track widths against the board's minimum, the net class's and the custom rules'.
fn track_widths(board: &Board, rules: &Rules, max: usize, out: &mut Vec<Violation>) {
    for (i, t) in board.tracks.iter().enumerate() {
        if out.len() >= max {
            return;
        }
        let net = board.net_name(&t.net);
        let (mut min, mut upper, mut rule) = (rules.min_track_width_of(net.as_deref()), None, None);
        if rules.custom.has(Kind::TrackWidth) {
            let s = subject(rules, ItemRef::Track(i), net.as_deref(), vec![t.layer.clone()]);
            if let Some((r, c)) = rules.custom.find(Kind::TrackWidth, &s, None) {
                // The custom rule replaces the board's and the class's minimum.
                min = c.min.unwrap_or(0.0);
                upper = c.max;
                rule = Some(r.name.clone());
            }
        }
        let at = [(t.start[0] + t.end[0]) / 2.0, (t.start[1] + t.end[1]) / 2.0];
        let message = if t.width < min - 1e-9 {
            format!("track {i} is {} wide, at least {} needed", mm(t.width), mm(min))
        } else if let Some(u) = upper.filter(|u| t.width > u + 1e-9) {
            format!("track {i} is {} wide, at most {} allowed", mm(t.width), mm(u))
        } else {
            continue;
        };
        out.push(Violation {
            code: "track_width",
            message: named(message, &rule),
            at,
            layer: Some(t.layer.clone()),
            items: vec![ItemRef::Track(i)],
            rule,
        });
    }
}

/// Pads of a zone's net in a thermal relief that fewer than `rules.min_spokes` spokes
/// reach (ADR 0053), judged on the fills stored in the board: a spoke is there when copper
/// lies in the middle of the relief's gap along its direction. Zones not filled are not
/// checked.
fn starved_thermals(board: &Board, rules: &Rules, copper: &Copper, max: usize, out: &mut Vec<Violation>) {
    for (zi, z) in board.zones.iter().enumerate().filter(|(_, z)| !z.keepout) {
        let Some(net) = board.net_name(&z.net) else { continue };
        for (li, layer) in copper.layers.iter().enumerate() {
            let polys: Vec<&Vec<Pt>> = z.fills.iter().filter(|(l, _)| l == layer).map(|(_, p)| p).collect();
            if polys.is_empty() {
                continue;
            }
            let covered = |q: Pt| polys.iter().any(|p| geom::inside(p, q));
            for it in
                copper.items.iter().filter(|it| it.net.as_deref() == Some(net.as_str()) && it.layers.contains(&li))
            {
                let ItemRef::Pad(p) = it.what else { continue };
                let Some(pad) = board.pads.get(p) else { continue };
                if crate::zones::pad_connect(board, p, &z.settings) != ZoneConnect::Thermal
                    || !z.outlines().any(|o| geom::inside(o, pad.at))
                {
                    continue;
                }
                let n = crate::zones::spoke_count(pad, &z.settings, &covered);
                if n < rules.min_spokes && out.len() < max {
                    out.push(Violation {
                        code: "starved_thermal",
                        message: format!(
                            "{} in {}: {n} thermal spoke{} reach it, {} needed",
                            describe(board, it.what),
                            describe(board, ItemRef::Zone(zi)),
                            if n == 1 { "" } else { "s" },
                            rules.min_spokes
                        ),
                        at: pad.at,
                        layer: Some(layer.clone()),
                        items: vec![ItemRef::Zone(zi), it.what],
                        rule: None,
                    });
                }
            }
        }
    }
}

/// Zone fills against the copper of other nets on their layer, and against each other.
fn zones(board: &Board, rules: &Rules, copper: &Copper, max: usize, out: &mut Vec<Violation>) {
    let mut fills: Vec<(usize, Option<String>, usize, copper::EdgeGrid, f64)> = Vec::new();
    for (zi, z) in board.zones.iter().enumerate().filter(|(_, z)| !z.keepout) {
        let net = board.net_name(&z.net);
        for (layer, poly) in z.fills.iter().filter(|(_, p)| p.len() >= 3) {
            let Some(l) = copper.layers.iter().position(|x| x == layer) else { continue };
            let need = z.settings.clearance.max(rules.clearance_of(net.as_deref()));
            fills.push((zi, net.clone(), l, copper::EdgeGrid::new(poly), need));
        }
    }
    let widest = rules
        .classes
        .iter()
        .filter_map(|c| c.clearance)
        .fold(rules.clearance, f64::max)
        .max(rules.custom.largest_min(Kind::Clearance));
    let mut seen = std::collections::BTreeSet::new();
    for (zi, net, l, grid, zone_need) in &fills {
        for it in copper.items.iter().filter(|it| it.layers.contains(l)) {
            if out.len() >= max {
                return;
            }
            if net.is_some() && it.net == *net {
                continue;
            }
            let mut need = rules.clearance_of(it.net.as_deref()).max(*zone_need).max(rules.clearance);
            let mut rule = None;
            let lname = copper.layers.get(*l).map(String::as_str);
            if let Some((v, name)) = custom_clearance(
                rules,
                (ItemRef::Zone(*zi), net.as_deref()),
                (it.what, it.net.as_deref()),
                lname,
                |_| Vec::new(),
            ) {
                (need, rule) = (v, Some(name));
            }
            if !copper::boxes_near(&it.bbox, &grid.bbox, need.max(widest)) || seen.contains(&(*zi, it.what)) {
                continue;
            }
            let g = it
                .prims
                .iter()
                .filter_map(|p| grid.gap(p, need))
                .fold(None, |b: Option<(f64, Pt)>, x| if b.is_none_or(|b| x.0 < b.0) { Some(x) } else { b });
            let Some((g, at)) = g else { continue };
            if g < need - SLACK {
                seen.insert((*zi, it.what));
                let (da, db) = (describe(board, ItemRef::Zone(*zi)), describe(board, it.what));
                let (code, message) = if g <= 0.0 {
                    ("short", format!("{da} and {db} of different nets touch"))
                } else {
                    ("clearance", format!("{da} and {db}: {} apart, {} needed", mm(g), mm(need)))
                };
                let message = named(message, &rule);
                let layer = copper.layers.get(*l).cloned();
                out.push(Violation { code, message, at, layer, items: vec![ItemRef::Zone(*zi), it.what], rule });
            }
        }
    }
    // Fills of different nets on one layer: each one's corners against the other.
    for (i, (za, na, la, ga, need_a)) in fills.iter().enumerate() {
        for (zb, nb, lb, gb, need_b) in fills.iter().skip(i + 1) {
            if la != lb || (na.is_some() && na == nb) || za == zb || out.len() >= max {
                continue;
            }
            let need = need_a.max(*need_b);
            if !copper::boxes_near(&ga.bbox, &gb.bbox, need) {
                continue;
            }
            let found = board.zones.get(*za).into_iter().flat_map(|z| z.fills.iter()).find_map(|(_, p)| {
                p.iter().find_map(|q| {
                    let prim = Prim::Capsule { a: *q, b: *q, r: 0.0 };
                    gb.gap(&prim, need).filter(|g| g.0 < need - SLACK)
                })
            });
            if let Some((g, at)) = found {
                let (da, db) = (describe(board, ItemRef::Zone(*za)), describe(board, ItemRef::Zone(*zb)));
                out.push(Violation {
                    code: if g <= 0.0 { "short" } else { "clearance" },
                    message: format!("{da} and {db}: {} apart, {} needed", mm(g.max(0.0)), mm(need)),
                    at,
                    layer: copper.layers.get(*la).cloned(),
                    items: vec![ItemRef::Zone(*za), ItemRef::Zone(*zb)],
                    rule: None,
                });
            }
        }
    }
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
                rule: None,
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
                rule: None,
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
                rule: None,
            });
        }
    }
    let (lo, hi) = segs.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), (a, b)| {
        ([lo[0].min(a[0]).min(b[0]), lo[1].min(a[1]).min(b[1])], [hi[0].max(a[0]).max(b[0]), hi[1].max(a[1]).max(b[1])])
    });
    let widest_edge = rules.edge_clearance.max(rules.custom.largest_min(Kind::EdgeClearance));
    for it in &copper.items {
        if out.len() >= max {
            return;
        }
        // Only items whose box comes near an outline piece (cut-outs included), or that
        // reach past the outline's box, can be too close to the edge.
        let near = segs.iter().any(|(a, b)| {
            let sb = [[a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]];
            copper::boxes_near(&it.bbox, &sb, widest_edge)
        });
        let outside = it.bbox[0][0] < lo[0] || it.bbox[0][1] < lo[1] || it.bbox[1][0] > hi[0] || it.bbox[1][1] > hi[1];
        if !near && !outside {
            continue;
        }
        let (mut m, mut rule) = (rules.edge_clearance, None);
        if rules.custom.has(Kind::EdgeClearance) {
            let layers = it.layers.iter().filter_map(|l| copper.layers.get(*l).cloned()).collect();
            let s = subject(rules, it.what, it.net.as_deref(), layers);
            if let Some((r, c)) = rules.custom.find(Kind::EdgeClearance, &s, None) {
                (m, rule) = (c.min.unwrap_or(m), Some(r.name.clone()));
            }
        }
        let (g, at) =
            it.prims.iter().map(near_edge).fold((f64::INFINITY, [0.0, 0.0]), |b, x| if x.0 < b.0 { x } else { b });
        if g < m - SLACK {
            let who = describe(board, it.what);
            out.push(Violation {
                code: "copper_to_edge",
                message: named(format!("{who} is {} from the board edge, {} needed", mm(g.max(0.0)), mm(m)), &rule),
                at,
                layer: it.layers.first().and_then(|l| copper.layers.get(*l)).cloned(),
                items: vec![it.what],
                rule,
            });
        }
    }
}
