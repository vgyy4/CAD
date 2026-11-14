//! Board layout commands (ADR 0047 phase 2b): zone fills, routing, the board as STEP and
//! the Specctra hand-off to an external autorouter. Coordinates are KiCad's: mm, Y down.

use super::pcb::{PcbImport, board_source, no_board, write_outputs};
use super::pcb_edit::commit_board;
use super::*;
use crate::pcb_session;
use gusset_pcb::{Limits, layers, length, pair, push, route, shove, specctra, tune, zones};
use std::sync::atomic::AtomicBool;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<PcbFillZones>(),
        spec::<PcbRoute>(),
        spec::<PcbRoutePair>(),
        spec::<PcbLengths>(),
        spec::<PcbTune>(),
        spec::<PcbAddFootprint>(),
        spec::<PcbExportStep>(),
        spec::<PcbExportDsn>(),
        spec::<PcbImportSes>(),
    ]
}

fn pcb_err(e: gusset_pcb::board::Error) -> CommandError {
    match e {
        gusset_pcb::board::Error::Edit(m) => CommandError::args(m),
        e => CommandError::new("io", format!("KiCad board: {e}")),
    }
}

/// The pads left with too few thermal spokes, as JSON (ADR 0053).
pub(super) fn starved_json(b: &gusset_pcb::Board, f: &zones::Filled) -> Vec<Value> {
    f.starved
        .iter()
        .map(|x| {
            let pad = gusset_pcb::drc::describe(b, gusset_pcb::copper::ItemRef::Pad(x.pad));
            json!({ "zone": x.zone, "layer_user_text": x.layer, "pad_user_text": pad, "spokes": x.spokes })
        })
        .collect()
}

/// Fills the open board's copper zones.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbFillZones {
    /// Zone indices (default: all).
    #[serde(default)]
    pub zones: Vec<usize>,
}

impl Command for PcbFillZones {
    const ID: &'static str = "pcb.fill_zones";
    const TITLE: &'static str = "Fill zones";
    const DESCRIPTION: &'static str = "Pour the open board's copper zones: clearances, thermals, minimum width, islands removed; lists starved thermal pads.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        if let Some(z) = self.zones.iter().find(|&&z| z >= open.board.zones.len()) {
            return Err(CommandError::not_found(format!("there is no zone {z}"))
                .hint(format!("the board has {} zones", open.board.zones.len())));
        }
        let only = (!self.zones.is_empty()).then_some(self.zones.as_slice());
        let filled = zones::fill(&open.board, &open.rules, only);
        if filled.fills.is_empty() {
            return Err(CommandError::new("not_found", "the board has no copper zones to fill"));
        }
        let mut b = open.board.clone();
        let polygons = b.set_zone_fills(&filled.fills, &Limits::default()).map_err(pcb_err)?;
        let mut per_zone: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
        for f in &filled.fills {
            *per_zone.entry(f.zone).or_default() += zones::fill_area(f);
        }
        let areas: Vec<Value> =
            per_zone.iter().map(|(z, a)| json!({ "zone": z, "area_mm2": (a * 100.0).round() / 100.0 })).collect();
        let starved = starved_json(&b, &filled);
        let extra = json!({ "zones": areas, "polygons": polygons, "starved_thermal": starved, "notes_user_text": filled.notes });
        commit_board(s, ctx, "Fill zones", open.file.clone(), &b, None, extra)
    }
}

/// Routes a track through points, around other nets' copper.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbRoute {
    /// Start, waypoints, end (mm).
    pub points: Vec<[f64; 2]>,
    /// Default F.Cu.
    #[serde(default)]
    pub layer: Option<String>,
    /// Default: start's.
    #[serde(default)]
    pub net: Option<String>,
    /// mm (default: class's).
    #[serde(default)]
    pub width: Option<f64>,
    /// false: straight.
    #[serde(default)]
    pub around: Option<bool>,
    /// Arc corners, mm.
    #[serde(default)]
    pub arc_radius: Option<f64>,
    /// false: no shove.
    #[serde(default)]
    pub shove: Option<bool>,
    /// any or F.Cu,B.Cu
    #[serde(default)]
    pub layers: Option<String>,
    // false: full width near pads too (no description: the pcb toolset is at its cap).
    #[serde(default)]
    pub neck: Option<bool>,
}

/// The layers a route may use (ADR 0059): `layer` alone unless the caller names more
/// ("any": every copper layer) or the end's copper of the net is only on other layers.
fn route_layers(
    b: &gusset_pcb::Board,
    asked: Option<&str>,
    layer: &str,
    net: Option<&str>,
    end: [f64; 2],
) -> Result<Vec<String>, CommandError> {
    let cu = b.copper_layers();
    let mut out: Vec<String> = vec![layer.to_string()];
    match asked.map(str::trim) {
        None => {
            let at_end = |l: &str| net.is_some() && b.net_at(end, l).as_deref() == net;
            if !at_end(layer) {
                out.extend(cu.iter().filter(|l| at_end(l)).cloned());
            }
        }
        Some("any") => out.extend(cu.iter().cloned()),
        Some(list) => {
            for l in list.split(',').map(str::trim).filter(|l| !l.is_empty()) {
                if !cu.iter().any(|c| c == l) {
                    return Err(CommandError::args(format!("{l} is not a copper layer of this board")));
                }
                out.push(l.to_string());
            }
        }
    }
    out.sort_by_key(|l| cu.iter().position(|c| c == l));
    out.dedup();
    Ok(out)
}

impl Command for PcbRoute {
    const ID: &'static str = "pcb.route";
    const TITLE: &'static str = "Route track";
    const DESCRIPTION: &'static str = "Lay a 45° track through points around other nets (pushes others aside).";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !(2..=1000).contains(&self.points.len()) {
            return Err(CommandError::args("a route has 2 to 1000 points"));
        }
        if self.points.iter().flatten().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
            return Err(CommandError::args("points must be within 10 m"));
        }
        if self.arc_radius.is_some_and(|r| !(r.is_finite() && (0.0..=100.0).contains(&r))) {
            return Err(CommandError::args("arc_radius is 0 to 100 mm"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let (b, rules) = (&open.board, &open.rules);
        let layer = self.layer.clone().unwrap_or_else(|| "F.Cu".into());
        if !b.copper_layers().contains(&layer) {
            return Err(CommandError::args("the layer is not a copper layer of this board"));
        }
        let first = self.points.first().copied().unwrap_or_default();
        let last = self.points.last().copied().unwrap_or_default();
        let mut net = self.net.clone().or_else(|| b.net_at(first, &layer));
        if net.is_none() && self.layers.is_some() {
            net = b.copper_layers().iter().find_map(|l| b.net_at(first, l));
        }
        let width = self.width.unwrap_or_else(|| rules.track_width_of(net.as_deref()));
        if !(width.is_finite() && width > 0.0 && width <= 100.0) {
            return Err(CommandError::args("the width is above 0 and at most 100 mm"));
        }
        let around = self.around.unwrap_or(true);
        let allowed = if around {
            route_layers(b, self.layers.as_deref(), &layer, net.as_deref(), last)?
        } else {
            vec![layer.clone()]
        };
        let idx = |l: &str| allowed.iter().position(|x| x == l).unwrap_or(0);
        let on = |p: [f64; 2], k: usize| net.is_some() && allowed.get(k).is_some_and(|l| b.net_at(p, l) == net);
        let grid = (rules.clearance_of(net.as_deref()).min(width) / 2.0).clamp(0.05, 0.25);
        // The start's layers: the allowed ones with its copper (else `layer`).
        let mut start_layers: Vec<usize> = (0..allowed.len()).filter(|&k| on(first, k)).collect();
        if start_layers.is_empty() {
            start_layers.push(idx(&layer));
        }
        let mut cur = if start_layers.contains(&idx(&layer)) {
            layer.clone()
        } else {
            start_layers.first().and_then(|&k| allowed.get(k)).cloned().unwrap_or_else(|| layer.clone())
        };
        // The board as other nets' tracks are shoved; obstacles rebuilt after a shove.
        let mut nb = b.clone();
        let mut obs: std::collections::BTreeMap<String, route::Obstacles> = std::collections::BTreeMap::new();
        let mut lay: Option<layers::Layered> = None;
        let mut lay_neck: Option<layers::Layered> = None;
        // Neck-down near pads (ADR 0060): to the net's minimum width, unless refused.
        let neck = if self.neck == Some(false) { width } else { layers::neck_width(rules, net.as_deref()).min(width) };
        let (mut shoved, mut shoved_vias) = (0usize, 0usize);
        // Runs of the new track on one layer (with each segment's width), and the vias
        // between them.
        let mut runs: Vec<(String, Vec<[f64; 2]>, Vec<f64>)> = vec![(cur.clone(), vec![first], Vec::new())];
        let mut vias: Vec<[f64; 2]> = Vec::new();
        let n = self.points.len();
        for (i, w) in self.points.windows(2).enumerate() {
            let &[a, c] = w else { continue };
            let starts = if i == 0 { start_layers.clone() } else { vec![idx(&cur)] };
            let mut targets: Vec<usize> = Vec::new();
            if i + 2 == n {
                targets = (0..allowed.len()).filter(|&k| on(c, k)).collect();
            }
            if targets.is_empty() {
                targets = (0..allowed.len()).collect();
            }
            let here = idx(&cur);
            let one_layer = starts.contains(&here) && targets.contains(&here);
            let o = obs
                .entry(cur.clone())
                .or_insert_with(|| route::Obstacles::new(&nb, rules, &cur, net.as_deref(), width));
            let leg = if !around {
                Some(vec![a, c])
            } else if one_layer {
                route::walkaround(o, a, c, grid, true)
            } else {
                None
            };
            if let Some(leg) = leg {
                if let Some(r) = runs.last_mut() {
                    r.2.extend(std::iter::repeat_n(width, leg.len().saturating_sub(1)));
                    r.1.extend(leg.into_iter().skip(1));
                }
                continue;
            }
            let refused = |why: &str| {
                CommandError::not_found(format!("no clear way from point {i} to point {} ({why})", i + 1))
                    .hint("move the points, allow layers: \"any\", or pass around: false")
            };
            // Across layers in one search (ADR 0059), then near pads with neck-down (ADR 0060).
            let mut why = "no way round";
            let mut found: Option<layers::LayerRoute> = None;
            if allowed.len() > 1 {
                let l = lay.get_or_insert_with(|| layers::Layered::new(&nb, rules, &allowed, net.as_deref(), width));
                match layers::route(l, rules, a, &starts, c, &targets, grid, true, None) {
                    Ok(r) => found = Some(r),
                    Err(e) => why = e,
                }
            }
            let radii = [layers::neck_radius(&nb, a, net.as_deref()), layers::neck_radius(&nb, c, net.as_deref())];
            if found.is_none() && radii.iter().any(|&r| r > 0.0) {
                let l = lay_neck.get_or_insert_with(|| {
                    layers::Layered::with_neck(&nb, rules, &allowed, net.as_deref(), width, neck)
                });
                match layers::route_necked(l, rules, a, &starts, c, &targets, grid, true, None, radii) {
                    Ok(r) => found = Some(r),
                    Err(e) => why = e,
                }
            }
            if let Some(r) = found {
                for (k, (layer_k, pts)) in r.legs.into_iter().enumerate() {
                    let ws = r.widths.get(k).cloned().unwrap_or_default();
                    match runs.last_mut() {
                        Some(run) if run.0 == layer_k => {
                            run.1.extend(pts.into_iter().skip(1));
                            run.2.extend(ws);
                        }
                        Some(run) if run.1.len() < 2 => *run = (layer_k, pts, ws),
                        _ => runs.push((layer_k, pts, ws)),
                    }
                }
                vias.extend(r.vias);
                cur = runs.last().map(|r| r.0.clone()).unwrap_or_default();
                continue;
            }
            if !one_layer || !self.shove.unwrap_or(true) {
                return Err(refused(why));
            }
            // Push-and-shove on this layer (ADR 0058), else the phase 3 shove.
            let keep: Vec<[f64; 2]> = runs.last().map(|r| r.1.clone()).unwrap_or_default();
            let path = if let Some(p) = push::push(&nb, rules, &cur, net.as_deref(), width, a, c, grid, &keep) {
                nb.apply_push(&p, &cur, &Limits::default()).map_err(pcb_err)?;
                shoved += p.lines.len();
                shoved_vias += p.vias.len();
                p.path
            } else {
                let sh = shove::shove(&nb, rules, &cur, net.as_deref(), width, a, c, grid, &keep)
                    .ok_or_else(|| refused(why))?;
                nb.apply_shove(&sh, &cur, &Limits::default()).map_err(pcb_err)?;
                shoved += 1;
                sh.path
            };
            obs.clear();
            lay = None;
            lay_neck = None;
            if let Some(r) = runs.last_mut() {
                r.2.extend(std::iter::repeat_n(width, path.len().saturating_sub(1)));
                r.1.extend(path.into_iter().skip(1));
            }
        }
        let radius = self.arc_radius.unwrap_or(0.0);
        let mut batch: Vec<gusset_pcb::edit::NewRoute> = Vec::new();
        let (mut length, mut necked) = (0.0, 0.0);
        for (l, pts, ws) in &runs {
            // Pieces of one width; corners rounded at full width only.
            let mut pieces: Vec<(f64, Vec<[f64; 2]>)> = Vec::new();
            for (k, w) in pts.windows(2).enumerate() {
                let &[p, q] = w else { continue };
                let wk = ws.get(k).copied().unwrap_or(width);
                match pieces.last_mut() {
                    Some((pw, pp)) if (*pw - wk).abs() < 1e-12 => pp.push(q),
                    _ => pieces.push((wk, vec![p, q])),
                }
            }
            for (w, pp) in pieces {
                let pp = route::simplify(&pp);
                if pp.len() < 2 {
                    continue;
                }
                let o =
                    obs.entry(l.clone()).or_insert_with(|| route::Obstacles::new(&nb, rules, l, net.as_deref(), width));
                let segs = route::with_arcs(o, &pp, if w < width { 0.0 } else { radius });
                length += route::length(&segs);
                if w < width {
                    necked += route::length(&segs);
                }
                batch.push((segs, w, l.clone(), net.clone()));
            }
        }
        let via = layers::via_size(rules, net.as_deref());
        let new_vias: Vec<gusset_pcb::edit::NewVia> =
            vias.iter().map(|&at| (at, via.size, via.drill, net.clone())).collect();
        let (index, first_via) = (nb.tracks.len(), nb.vias.len());
        nb.add_batch(&batch, &new_vias, &Limits::default()).map_err(pcb_err)?;
        let mut extra = json!({
            "tracks": nb.tracks.len() - index,
            "first_track": index,
            "length_mm": (length * 1000.0).round() / 1000.0,
            "width": width,
            "net_user_text": net,
            "shoved_tracks": shoved,
            "shoved_vias": shoved_vias,
            "vias": vias.len(),
        });
        if let (false, Value::Object(m)) = (vias.is_empty(), &mut extra) {
            m.insert("first_via".into(), json!(first_via));
            m.insert("via_size".into(), json!([via.size, via.drill]));
        }
        if let (true, Value::Object(m)) = (necked > 0.0, &mut extra) {
            m.insert("neck_width".into(), json!(neck));
            m.insert("necked_mm".into(), json!(r3(necked)));
        }
        commit_board(s, ctx, "Route track", open.file.clone(), &nb, None, extra)
    }
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Routes a differential pair coupled (ADR 0055).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbRoutePair {
    /// Either net (USB_P, CLK-) or the shared name (USB).
    pub pair: String,
    /// Centre line: start, waypoints, end (mm).
    pub points: Vec<[f64; 2]>,
    /// Default F.Cu.
    #[serde(default)]
    pub layer: Option<String>,
    /// mm (default: class's).
    #[serde(default)]
    pub width: Option<f64>,
    /// Edge gap, mm.
    #[serde(default)]
    pub gap: Option<f64>,
}

impl Command for PcbRoutePair {
    const ID: &'static str = "pcb.route_pair";
    const TITLE: &'static str = "Route pair";
    const DESCRIPTION: &'static str =
        "Lay a differential pair (X_P/X_N, X+/X-) coupled along a 45° centre line, fanning out to the pads.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !(2..=1000).contains(&self.points.len()) {
            return Err(CommandError::args("a route has 2 to 1000 points"));
        }
        if self.points.iter().flatten().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
            return Err(CommandError::args("points must be within 10 m"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let b = &open.board;
        let layer = self.layer.clone().unwrap_or_else(|| "F.Cu".into());
        if !b.copper_layers().contains(&layer) {
            return Err(CommandError::args("the layer is not a copper layer of this board"));
        }
        let pr = pair::find(b, &self.pair).ok_or_else(|| {
            CommandError::not_found("that is not a differential pair on this board")
                .hint("both nets must exist, named X_P/X_N or X+/X-")
        })?;
        let (cw, cg) = open.rules.diff_pair_of(&pr.p, &pr.n);
        let (width, gap) = (self.width.unwrap_or(cw), self.gap.unwrap_or(cg));
        if !(width.is_finite() && width > 0.0 && width <= 100.0 && gap.is_finite() && (0.0..=100.0).contains(&gap)) {
            return Err(CommandError::args("the width is above 0 and the gap 0 or more, both at most 100 mm"));
        }
        let r = pair::route_pair(b, &open.rules, &layer, &pr, &self.points, width, gap)
            .map_err(|m| CommandError::not_found(m).hint("move the points, or route on another layer"))?;
        let mut nb = b.clone();
        let first = nb.add_route(&r.p, r.width, &layer, Some(&pr.p), &Limits::default()).map_err(pcb_err)?;
        nb.add_route(&r.n, r.width, &layer, Some(&pr.n), &Limits::default()).map_err(pcb_err)?;
        let (lp, ln) = (route::length(&r.p), route::length(&r.n));
        let extra = json!({
            "tracks": nb.tracks.len() - first,
            "first_track": first,
            "width": r.width,
            "gap": r3(r.gap),
            "p_user_text": pr.p,
            "n_user_text": pr.n,
            "p_length_mm": r3(lp),
            "n_length_mm": r3(ln),
            "skew_mm": r3((lp - ln).abs()),
        });
        commit_board(s, ctx, "Route pair", open.file.clone(), &nb, None, extra)
    }
}

/// Routed length per net and the skew of differential pairs (ADR 0055).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbLengths {
    /// One net or pair (default: all with routing).
    #[serde(default)]
    pub net: Option<String>,
}

impl Command for PcbLengths {
    const ID: &'static str = "pcb.lengths";
    const TITLE: &'static str = "Net lengths";
    const DESCRIPTION: &'static str =
        "Routed length of each net (tracks, arcs, vias at board thickness) and each differential pair's skew.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let b = &open.board;
        let keep: Option<Vec<String>> = self.net.as_ref().map(|n| match pair::find(b, n) {
            Some(p) => vec![p.p, p.n],
            None => vec![n.clone()],
        });
        let mut nets = length::net_lengths(b);
        nets.retain(|l| keep.as_ref().is_none_or(|k| k.contains(&l.net)));
        nets.sort_by(|a, c| c.total().total_cmp(&a.total()));
        let shown: Vec<Value> = nets
            .iter()
            .take(200)
            .map(|l| {
                json!({ "net_user_text": l.net, "tracks": l.tracks, "vias": l.vias,
                        "track_mm": r3(l.track_mm), "via_mm": r3(l.via_mm), "length_mm": r3(l.total()) })
            })
            .collect();
        let pairs: Vec<Value> = length::skews(b)
            .iter()
            .filter(|k| keep.as_ref().is_none_or(|n| n.contains(&k.pair.p)))
            .map(|k| {
                json!({ "pair_user_text": k.pair.base, "p_mm": r3(k.p.total()), "n_mm": r3(k.n.total()),
                        "skew_mm": r3(k.skew()), "shorter_user_text": k.shorter() })
            })
            .collect();
        Ok(json!({ "nets": shown, "pairs": pairs, "thickness_mm": r3(b.thickness) }))
    }
}

/// Adds a meander to a net's straight track to reach a length (ADR 0055).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbTune {
    /// The net; for a pair without target, the shorter of its two is tuned.
    pub net: String,
    /// Total length wanted, mm (default: the partner's length).
    #[serde(default)]
    pub target: Option<f64>,
    /// mm (default 0.05).
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// Tallest bump, mm (default 1).
    #[serde(default)]
    pub amplitude: Option<f64>,
    /// Gap between legs, mm (default: clearance).
    #[serde(default)]
    pub spacing: Option<f64>,
    /// Track index (default: the longest straight one that fits).
    #[serde(default)]
    pub track: Option<usize>,
}

impl Command for PcbTune {
    const ID: &'static str = "pcb.tune";
    const TITLE: &'static str = "Tune length";
    const DESCRIPTION: &'static str =
        "Meander a straight track of a net to a target length within clearances; refuses when there is no room.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let b = &open.board;
        let mut net = self.net.clone();
        let mut target = self.target;
        if target.is_none() {
            let k = pair::find(b, &self.net)
                .and_then(|p| length::skews(b).into_iter().find(|k| k.pair == p))
                .ok_or_else(|| CommandError::args("without a target the net must belong to a differential pair"))?;
            net = k.shorter().to_string();
            target = Some(k.p.total().max(k.n.total()));
        }
        let o = tune::TuneOpts {
            target: target.unwrap_or_default(),
            tolerance: self.tolerance.unwrap_or(0.05),
            amplitude: self.amplitude.unwrap_or(1.0),
            spacing: self.spacing.unwrap_or_else(|| open.rules.clearance_of(Some(&net))),
        };
        if !(o.target > 0.0 && o.target <= 100_000.0) {
            return Err(CommandError::args("the target is above 0 and at most 100000 mm"));
        }
        let before = length::length_of(b, &net).total();
        let t = tune::tune(b, &open.rules, &net, self.track, &o).map_err(CommandError::not_found)?;
        let old = b.tracks.get(t.track).ok_or_else(|| CommandError::not_found("the track vanished"))?;
        let (width, layer) = (old.width, old.layer.clone());
        let mut nb = b.clone();
        nb.delete_items(&[t.track], &[], &Limits::default()).map_err(pcb_err)?;
        let first = nb.add_route(&t.pieces, width, &layer, Some(&net), &Limits::default()).map_err(pcb_err)?;
        let after = length::length_of(&nb, &net).total();
        let extra = json!({
            "net_user_text": net,
            "replaced_track": t.track,
            "first_track": first,
            "pieces": t.pieces.len(),
            "bumps": t.bumps,
            "before_mm": r3(before),
            "after_mm": r3(after),
            "target_mm": r3(o.target),
        });
        commit_board(s, ctx, "Tune length", open.file.clone(), &nb, None, extra)
    }
}

/// Places a footprint from a footprint library folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbAddFootprint {
    /// Folder of Lib.pretty libraries.
    pub library: String,
    /// Lib:Name (or Name).
    pub footprint: String,
    pub at: [f64; 2],
    /// Degrees.
    #[serde(default)]
    pub angle: Option<f64>,
    /// Default: the next free FP<n>.
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

impl Command for PcbAddFootprint {
    const ID: &'static str = "pcb.add_footprint";
    const TITLE: &'static str = "Place footprint";
    const DESCRIPTION: &'static str = "Place a footprint from a KiCad library folder (Lib.pretty) on the open board.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if self.angle.is_some_and(|a| !a.is_finite()) {
            return Err(CommandError::args("the angle must be a number of degrees"));
        }
        let text = super::sch::footprint_candidates(&self.library, &self.footprint)
            .into_iter()
            .find_map(|p| super::sch::read_text(s, ctx, &p, super::sch::MAX_FOOTPRINT_BYTES).ok())
            .ok_or_else(|| {
                CommandError::not_found(format!(
                    "no footprint '{}' in the library folder",
                    crate::paths::display_name(&self.footprint)
                ))
                .hint("name it Lib:Name for Lib.pretty/Name.kicad_mod in that folder")
            })?;
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let mut b = open.board.clone();
        let reference = match &self.reference {
            Some(r) => r.clone(),
            None => (1..100_000).map(|n| format!("FP{n}")).find(|r| b.footprint_index(r).is_none()).unwrap_or_default(),
        };
        if b.footprint_index(&reference).is_some() {
            return Err(CommandError::args(format!(
                "'{}' is already on the board",
                crate::paths::display_name(&reference)
            )));
        }
        let name = self.footprint.rsplit(':').next().unwrap_or(&self.footprint).to_string();
        let value = self.value.clone().unwrap_or(name);
        let i = b
            .add_footprint(&text, &self.footprint, &reference, &value, self.at, &Limits::default())
            .map_err(pcb_err)?;
        if let Some(a) = self.angle.filter(|a| a.rem_euclid(360.0).abs() > 1e-9) {
            b.move_footprint(i, self.at, a.rem_euclid(360.0), &Limits::default()).map_err(pcb_err)?;
        }
        let pads = b.footprints.get(i).map_or(0, |f| f.pads.len());
        let label = format!("Place {}", crate::paths::display_name(&reference));
        let extra = json!({ "footprint": i, "reference_user_text": reference, "pads": pads });
        commit_board(s, ctx, &label, open.file.clone(), &b, None, extra)
    }
}

/// Writes the board and its components as a STEP file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportStep {
    /// Output .step.
    pub file: String,
    /// .kicad_pcb (default: the open board).
    #[serde(default)]
    pub path: Option<String>,
    /// Outer copper as thin bodies.
    #[serde(default)]
    pub copper: bool,
    /// Folder with the footprints' STEP models.
    #[serde(default)]
    pub models_dir: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportStep {
    const ID: &'static str = "pcb.export_step";
    const TITLE: &'static str = "Export PCB STEP";
    const DESCRIPTION: &'static str =
        "Write the board (outline, holes) and its components (STEP models from models_dir, else boxes) as STEP.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let lower = self.file.to_ascii_lowercase();
        if !(lower.ends_with(".step") || lower.ends_with(".stp")) {
            return Err(CommandError::args("the file name ends in .step or .stp"));
        }
        let (b, _) = board_source(s, ctx, self.path.as_deref())?;
        let imp = PcbImport {
            path: None,
            holes: None,
            vias: false,
            copper: self.copper,
            components: None,
            component_height: None,
            models_dir: self.models_dir.clone(),
        };
        let (bodies, notes) = imp.bodies(s, ctx, &b)?;
        let names: Vec<String> = bodies.iter().map(|(n, _, c)| format!("{n}\u{1f}#{:06x}", c & 0xff_ffff)).collect();
        let list: Vec<(&gusset_kernel::Shape, &str)> =
            bodies.iter().zip(&names).map(|((_, sh, _), n)| (sh, n.as_str())).collect();
        // Check the target before the (slow) kernel write.
        let (folder, name) = self.file.rsplit_once(['/', '\\']).unwrap_or(("", &self.file));
        let ext = if lower.ends_with(".stp") { "stp" } else { "step" };
        let full = crate::paths::export_target(s.project_dir.as_deref(), &self.file, &ctx.actor, &[ext])?;
        if full.exists() && !self.overwrite {
            return Err(CommandError::new("confirmation_required", format!("'{}' already exists", self.file))
                .hint("pass overwrite: true to replace it"));
        }
        let bytes = s.kernel().export_step(&list).map_err(|e| CommandError::new("io", e.to_string()))?;
        let text = String::from_utf8(bytes).map_err(|_| CommandError::new("io", "the STEP writer gave binary data"))?;
        let written = write_outputs(s, ctx, folder, self.overwrite, ext, &[(name.to_string(), text)])?;
        Ok(json!({ "written": written, "bodies": list.len(), "notes_user_text": notes }))
    }
}

/// Largest session file read.
const MAX_SES_BYTES: u64 = 64 << 20;

/// Writes the open board as a Specctra DSN design for an external autorouter.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbExportDsn {
    /// Output .dsn.
    pub file: String,
    #[serde(default)]
    pub overwrite: bool,
}

impl Command for PcbExportDsn {
    const ID: &'static str = "pcb.export_dsn";
    const TITLE: &'static str = "Export DSN";
    const DESCRIPTION: &'static str =
        "Write the open board as Specctra DSN for an external autorouter; read its .ses back with pcb_import_ses.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.file.to_ascii_lowercase().ends_with(".dsn") {
            return Err(CommandError::args("the file name ends in .dsn"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        let (folder, name) = self.file.rsplit_once(['/', '\\']).unwrap_or(("", &self.file));
        let text = specctra::write_dsn(&open.board, &open.rules, name);
        let written = write_outputs(s, ctx, folder, self.overwrite, "dsn", &[(name.to_string(), text)])?;
        Ok(json!({
            "written": written,
            "nets": open.board.nets.len(),
            "unrouted": open.ratsnest.unrouted(),
        }))
    }
}

/// Lays the routes of a Specctra session file on the open board.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcbImportSes {
    /// .ses from the autorouter.
    pub path: String,
    /// Keep the board's tracks and vias (default: replaced).
    #[serde(default)]
    pub keep_routes: bool,
}

impl Command for PcbImportSes {
    const ID: &'static str = "pcb.import_ses";
    const TITLE: &'static str = "Import SES";
    const DESCRIPTION: &'static str =
        "Lay an autorouter's Specctra session (.ses) on the open board, replacing its tracks and vias (undoable).";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        if !self.path.to_ascii_lowercase().ends_with(".ses") {
            return Err(CommandError::args("pcb.import_ses reads .ses files"));
        }
        let open = pcb_session::open_board(s, &ctx.actor)?.ok_or_else(no_board)?;
        crate::paths::refuse_remote(&self.path)?;
        let full = crate::paths::read_target(s.project_dir.as_deref(), &self.path, &ctx.actor)?;
        let f = crate::import::open_regular(&full, &self.path, MAX_SES_BYTES)?;
        let bytes = crate::import::read_capped(f, MAX_SES_BYTES, &AtomicBool::new(false))?;
        let text =
            String::from_utf8(bytes).map_err(|_| CommandError::new("io", "the session file is not UTF-8 text"))?;
        let ses = specctra::read_ses(&text, &specctra::Limits::default())
            .map_err(|e| CommandError::new("io", format!("Specctra session: {e}")))?;
        let unknown: Vec<String> = ses
            .nets
            .iter()
            .filter(|n| !open.board.nets.iter().any(|b| b.name == n.name))
            .map(|n| n.name.chars().take(80).collect())
            .take(20)
            .collect();
        let mut b = open.board.clone();
        let (tracks, vias) =
            b.apply_session(&ses, &open.rules, !self.keep_routes, &Limits::default()).map_err(pcb_err)?;
        let after = gusset_pcb::ratsnest::ratsnest(&b).unrouted();
        let extra = json!({
            "tracks": tracks,
            "vias": vias,
            "unrouted_after": after,
            "unknown_nets_user_text": unknown,
        });
        commit_board(s, ctx, "Import routes", open.file.clone(), &b, None, extra)
    }
}
