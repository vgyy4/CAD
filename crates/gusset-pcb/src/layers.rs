//! Routing across copper layers in one search (ADR 0059): the 45° walkaround of ADR 0047
//! on a layered grid, where a step may change layer by placing a through via.
//!
//! * One grid per routable layer over the same window; a grid point is free on a layer as
//!   in the one-layer walkaround (its gap to that layer's obstacles at least the needed
//!   gap plus half a grid diagonal).
//! * A step to the same point on another layer places a through via there: allowed when
//!   the point is free on both layers and a via of the net's size ([`via_size`]) keeps its
//!   clearance on every copper layer (routable or not), keeps the hole-to-hole gap to every
//!   drilled hole (pads and vias of any net) and the hole-to-edge gap, stays off surface
//!   pads (of its own net too: no via in pad) and out of keep-outs that forbid vias.
//! * Tracks and vias keep clear of other nets' thermal reliefs (the pad, its relief gap
//!   and a spoke), so zones poured again still tie their pads in. It costs as much as [`VIA_COST_MM`] of track.
//! * A* with the octile distance as the estimate (plus one via's cost where the layer is
//!   not one the route may end on), bounded by [`MAX_STATES`] grid points over all layers
//!   and [`MAX_VISITS`] expanded; cancellable.
//! * The grid path is cut into legs at the vias; each leg is pulled tight on its layer as
//!   in the one-layer walkaround (vias stay at their grid points).
//! * Near pads ([`route_necked`], ADR 0060): within a bounded radius of each end the track
//!   may neck down to the net's minimum width ([`neck_width`]) where the full width does
//!   not fit, at a cost per step, and an end may reach the grid by a longer straight
//!   escape; the route comes out as pieces of two widths ([`LayerRoute::pieces`]).

use crate::board::{Board, PadKind, Pt};
use crate::copper::{Copper, ItemRef, Prim};
use crate::route::{self, Obstacles, SLACK, octi};
use crate::rules::Rules;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

/// Most grid points over all layers of one search.
pub const MAX_STATES: usize = 3_000_000;
/// Most grid points one search expands.
pub const MAX_VISITS: usize = 1_500_000;
/// A via costs as much as this much track (mm).
pub const VIA_COST_MM: f64 = 2.0;

/// The size and drill of a new through via.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViaSize {
    pub size: f64,
    pub drill: f64,
}

/// The via a route of `net` places: its class's size and drill (ADR 0054), else the
/// board's, raised to the minimum drill, the minimum diameter and the minimum annular ring.
pub fn via_size(rules: &Rules, net: Option<&str>) -> ViaSize {
    let (size, drill) = rules.via_of(net);
    let drill = drill.max(rules.min_via_drill_of(net));
    let size = size.max(rules.min_via_diameter_of(net)).max(drill + 2.0 * rules.min_annular_ring);
    ViaSize { size, drill }
}

/// What a route of one net keeps clear of on each layer, and where its vias may go.
#[derive(Debug, Clone)]
pub struct Layered {
    /// The layers the route may run on.
    pub layers: Vec<String>,
    tracks: Vec<Obstacles>,
    /// Every copper layer at the via's size.
    vias: Vec<Obstacles>,
    /// Drilled holes at the hole-to-hole gap, the edge at the hole-to-edge gap.
    holes: Obstacles,
    pub via: ViaSize,
    pub width: f64,
    /// Each routable layer's obstacles at the necked-down width (empty: no neck-down).
    narrow: Vec<Obstacles>,
    /// The necked-down width (`width` when there is none).
    pub neck: f64,
}

impl Layered {
    /// The obstacles for a track of `net` and `width` on each of `layers` (copper layers of
    /// the board; others are left out) and for its vias.
    pub fn new(board: &Board, rules: &Rules, layers: &[String], net: Option<&str>, width: f64) -> Layered {
        Layered::with_neck(board, rules, layers, net, width, width)
    }

    /// As [`Layered::new`], with the obstacles of a track necked down to `neck` wide near
    /// its ends (ADR 0060); none when `neck` is not below `width`.
    pub fn with_neck(
        board: &Board,
        rules: &Rules,
        layers: &[String],
        net: Option<&str>,
        width: f64,
        neck: f64,
    ) -> Layered {
        let copper = Copper::of(board);
        let cu = board.copper_layers();
        let layers: Vec<String> = layers.iter().filter(|l| cu.contains(l)).cloned().collect();
        let track = |l: &str, w: f64| {
            Obstacles::with_copper(board, &copper, rules, l, net, w).extended(reliefs(board, rules, l, net, w / 2.0))
        };
        let tracks = layers.iter().map(|l| track(l, width)).collect::<Vec<_>>();
        let neck = if neck > 0.0 && neck < width - 1e-9 { neck } else { width };
        let narrow = if neck < width { layers.iter().map(|l| track(l, neck)).collect::<Vec<_>>() } else { Vec::new() };
        let via = via_size(rules, net);
        // One routable layer places no via: nothing to gather.
        let via_layers: &[String] = if layers.len() > 1 { &cu } else { &[] };
        let vias = via_layers
            .iter()
            .enumerate()
            .map(|(li, l)| {
                let o = Obstacles::with_copper(board, &copper, rules, l, net, via.size);
                let mut keep = via_keepouts(board, l, via.size / 2.0);
                keep.extend(reliefs(board, rules, l, net, via.size / 2.0));
                // No via in a surface pad, even of the route's own net.
                for it in copper.items.iter().filter(|it| it.layers.contains(&li) && it.net.as_deref() == net) {
                    if let ItemRef::Pad(p) = it.what
                        && board.pads.get(p).is_some_and(|p| p.kind == PadKind::Smd)
                    {
                        keep.extend(it.prims.iter().map(|q| (q.clone(), via.size / 2.0)));
                    }
                }
                if keep.is_empty() { o } else { o.extended(keep) }
            })
            .collect();
        let gap = rules.hole_to_hole.max(rules.custom.largest_min(crate::dru::Kind::HoleToHole)) + via.drill / 2.0;
        let mut prims: Vec<(Prim, f64)> = Vec::new();
        if layers.len() > 1 {
            for p in &board.pads {
                if let Some(d) = p.drill {
                    let h = crate::drill::pad_hole(p.at, p.angle, &d);
                    prims.push((Prim::Capsule { a: h.at, b: h.slot_end.unwrap_or(h.at), r: h.diameter / 2.0 }, gap));
                }
            }
            for v in board.vias.iter().filter(|v| v.drill > 0.0) {
                prims.push((Prim::Capsule { a: v.at, b: v.at, r: v.drill / 2.0 }, gap));
            }
            let (loops, _) = crate::geom::outline_loops(board, crate::check::OUTLINE_TOL);
            for l in loops.iter().map(|l| l.polygon(0.01)) {
                let k = l.len();
                for i in 0..k {
                    if let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % k)) {
                        prims.push((Prim::Capsule { a: *a, b: *b, r: 0.0 }, rules.hole_to_edge + via.drill / 2.0));
                    }
                }
            }
        }
        Layered { layers, tracks, narrow, vias, holes: Obstacles::from_prims(prims, 0.0), via, width, neck }
    }

    /// The obstacles of the `i`-th routable layer.
    pub fn track(&self, i: usize) -> Option<&Obstacles> {
        self.tracks.get(i)
    }

    /// Whether a via of the route fits at `p` (every layer's copper, holes, keep-outs,
    /// the edge).
    pub fn via_fits(&self, p: Pt) -> bool {
        self.vias.iter().all(|o| o.clear(p, p)) && self.holes.margin(p, p, 0.0) >= -SLACK
    }
}

/// Keep-outs on `layer` that forbid vias, as pieces a via's centre keeps `r` from.
fn via_keepouts(board: &Board, layer: &str, r: f64) -> Vec<(Prim, f64)> {
    let mut out = Vec::new();
    for z in board.zones.iter().filter(|z| z.keepout && board.on_layer(&z.layers, layer)) {
        let no_vias = board
            .tree
            .items()
            .get(z.node)
            .and_then(|n| n.child("keepout"))
            .and_then(|k| k.child("vias"))
            .and_then(|t| t.arg(0))
            == Some("not_allowed");
        if no_vias {
            out.extend(z.outlines().map(|o| (Prim::Poly { pts: o.clone() }, r)));
        }
    }
    out
}

/// Thermal reliefs of other nets' zones on `layer`: each pad a zone ties in with spokes,
/// as a piece new copper of `net` with half width `half` keeps clear of by the clearance,
/// the relief gap and a spoke (or the zone's narrowest copper), so the zone, poured again,
/// still reaches the spokes.
fn reliefs(board: &Board, rules: &Rules, layer: &str, net: Option<&str>, half: f64) -> Vec<(Prim, f64)> {
    let mut out = Vec::new();
    for z in board.zones.iter().filter(|z| !z.keepout && board.on_layer(&z.layers, layer)) {
        let Some(zn) = board.net_name(&z.net) else { continue };
        if Some(zn.as_str()) == net {
            continue;
        }
        let st = &z.settings;
        let need = st.clearance.max(rules.clearance_of(net)).max(rules.clearance_of(Some(&zn)))
            + half
            + st.thermal_gap
            + st.thermal_width.max(st.min_width);
        for (i, p) in board.pads.iter().enumerate() {
            if board.net_name(&p.net).as_deref() != Some(zn.as_str())
                || !board.on_layer(&p.layers, layer)
                || crate::zones::pad_connect(board, i, st) != crate::board::ZoneConnect::Thermal
                || !z.outlines().any(|o| crate::geom::inside(o, p.at))
            {
                continue;
            }
            if let Some(c) = crate::geom::pad_contour(p, 0.0) {
                out.push((Prim::Poly { pts: c.polygon(0.02) }, need));
            }
        }
    }
    out
}

/// A route across layers: legs (layer, corner points) meeting at vias.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerRoute {
    /// Each leg's layer and corner points; leg `k + 1` starts where leg `k` ends, at
    /// `vias[k]`.
    pub legs: Vec<(String, Vec<Pt>)>,
    pub vias: Vec<Pt>,
    /// Each leg's segment widths (one per pair of corner points): the track's width, or
    /// the necked-down width near an end (ADR 0060).
    pub widths: Vec<Vec<f64>>,
}

impl LayerRoute {
    /// The route as track pieces of one layer and one width each (layer, width, corner
    /// points), in order.
    pub fn pieces(&self) -> Vec<(String, f64, Vec<Pt>)> {
        let mut out: Vec<(String, f64, Vec<Pt>)> = Vec::new();
        for (k, (layer, pts)) in self.legs.iter().enumerate() {
            let ws = self.widths.get(k);
            let mut cur: Option<(f64, Vec<Pt>)> = None;
            for (i, w) in pts.windows(2).enumerate() {
                let &[a, b] = w else { continue };
                let wi = ws.and_then(|ws| ws.get(i)).copied().unwrap_or(0.0);
                match cur.as_mut() {
                    Some((cw, cp)) if *cw == wi => cp.push(b),
                    _ => {
                        if let Some((cw, cp)) = cur.take() {
                            out.push((layer.clone(), cw, cp));
                        }
                        cur = Some((wi, vec![a, b]));
                    }
                }
            }
            if let Some((cw, cp)) = cur {
                out.push((layer.clone(), cw, cp));
            }
        }
        out
    }

    /// The narrowest segment width (`f64::INFINITY` with no segment).
    pub fn narrowest(&self) -> f64 {
        self.widths.iter().flatten().copied().fold(f64::INFINITY, f64::min)
    }
}

/// Extra cost of a grid step taken at the necked-down width, as a share of the step: the
/// route stays at full width where it can.
pub const NECK_COST: f64 = 1.0;
/// A neck-down reaches at most this far from the pad's centre (mm) ...
pub const NECK_CAP_MM: f64 = 2.5;
/// ... and at most this many times the pad's larger side.
pub const NECK_PADS: f64 = 2.0;

/// The width a track of `net` may neck down to (ADR 0060): the board's minimum track
/// width raised by the net's class (ADR 0054) and by the largest minimum of any custom
/// `track_width` rule.
pub fn neck_width(rules: &Rules, net: Option<&str>) -> f64 {
    rules.min_track_width_of(net).max(rules.custom.largest_min(crate::dru::Kind::TrackWidth))
}

/// How far from `at` a track of `net` may run necked down: [`NECK_PADS`] times the larger
/// side of the net's pad there (the largest when several), at most [`NECK_CAP_MM`]; 0 when
/// no pad of the net is there.
pub fn neck_radius(board: &Board, at: Pt, net: Option<&str>) -> f64 {
    let Some(net) = net else { return 0.0 };
    let side = board
        .pads
        .iter()
        .filter(|p| board.net_name(&p.net).as_deref() == Some(net))
        .filter(|p| crate::geom::pad_contour(p, 0.0).is_some_and(|c| crate::geom::inside(&c.polygon(0.02), at)))
        .map(|p| p.size[0].max(p.size[1]))
        .fold(0.0, f64::max);
    (side * NECK_PADS).min(NECK_CAP_MM)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Open {
    f: f64,
    g: f64,
    s: usize,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then_with(|| self.g.total_cmp(&o.g)).then_with(|| o.s.cmp(&self.s))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// A route from `from` (on any of the routable layers numbered in `from_layers`) to `to`
/// (on any of `to_layers`), changing layer through vias where that helps.
///
/// # Errors
/// Why there is none: no end clear on its layers, no way within the window and the caps,
/// the route's own vias too close, or `cancel` set.
#[allow(clippy::too_many_arguments)]
pub fn route(
    lay: &Layered,
    rules: &Rules,
    from: Pt,
    from_layers: &[usize],
    to: Pt,
    to_layers: &[usize],
    grid: f64,
    diagonal_first: bool,
    cancel: Option<&AtomicBool>,
) -> Result<LayerRoute, &'static str> {
    route_necked(lay, rules, from, from_layers, to, to_layers, grid, diagonal_first, cancel, [0.0, 0.0])
}

/// As [`route`], near pads (ADR 0060): within `radii[0]` of `from` and `radii[1]` of `to`
/// (at most [`NECK_CAP_MM`]) the track may neck down to `lay.neck` where the full width
/// does not fit, at [`NECK_COST`] more per grid step, and an end may be joined to the grid
/// by a straight escape as long as its radius; the grid is twice as fine (within the same
/// caps) with the necked end on a grid point. Obstacles built without a neck-down width
/// ([`Layered::new`]) still escape but never neck.
///
/// # Errors
/// As [`route`].
#[allow(clippy::too_many_arguments)]
pub fn route_necked(
    lay: &Layered,
    rules: &Rules,
    from: Pt,
    from_layers: &[usize],
    to: Pt,
    to_layers: &[usize],
    grid: f64,
    diagonal_first: bool,
    cancel: Option<&AtomicBool>,
    radii: [f64; 2],
) -> Result<LayerRoute, &'static str> {
    let nl = lay.tracks.len();
    if nl == 0 {
        return Err("no routable layer");
    }
    // Radii: how far from each end the link may reach and the track may run necked down.
    let [r_from, r_to] = radii.map(|r| if r.is_finite() { r.clamp(0.0, NECK_CAP_MM) } else { 0.0 });
    let cancelled = || cancel.is_some_and(|c| c.load(AtomicOrdering::Relaxed));
    let dist = libm::hypot(to[0] - from[0], to[1] - from[1]);
    let pad = 5.0 + dist * 0.5;
    let mut lo = [from[0].min(to[0]) - pad, from[1].min(to[1]) - pad];
    let hi = [from[0].max(to[0]) + pad, from[1].max(to[1]) + pad];
    // Necking down: a grid twice as fine, so narrow ways between fine-pitch pads hold
    // grid points (within the same caps).
    let mut step = (if r_from > 0.0 || r_to > 0.0 { grid / 2.0 } else { grid }).clamp(0.02, 1.0);
    let cells = |s: f64| ((hi[0] - lo[0]) / s + 2.0) * ((hi[1] - lo[1]) / s + 2.0);
    while cells(step) > route::MAX_CELLS as f64 || cells(step) * nl as f64 > MAX_STATES as f64 {
        step *= 1.5;
    }
    // ... and one end, the one necked down (else the end), on a grid point.
    if r_from > 0.0 || r_to > 0.0 {
        let e = if r_to > 0.0 { to } else { from };
        lo = [e[0] - ((e[0] - lo[0]) / step).ceil() * step, e[1] - ((e[1] - lo[1]) / step).ceil() * step];
    }
    let nx = ((hi[0] - lo[0]) / step).ceil() as usize + 1;
    let ny = ((hi[1] - lo[1]) / step).ceil() as usize + 1;
    let n = nx * ny;
    let pos = |c: usize| [lo[0] + (c % nx) as f64 * step, lo[1] + (c / nx) as f64 * step];
    let cell_at = |p: Pt| {
        let x = (((p[0] - lo[0]) / step).round().max(0.0) as usize).min(nx - 1);
        let y = (((p[1] - lo[1]) / step).round().max(0.0) as usize).min(ny - 1);
        y * nx + x
    };
    let spare = step * std::f64::consts::FRAC_1_SQRT_2;
    let d = |a: Pt, b: Pt| libm::hypot(a[0] - b[0], a[1] - b[1]);
    // Where necked grid points may be (within a neck radius of an end), and, with `slack`,
    // where necked track may run (a narrow step's other end may lie a step outside).
    let in_neck =
        |p: Pt, slack: f64| (r_from > 0.0 && d(p, from) <= r_from + slack) || (r_to > 0.0 && d(p, to) <= r_to + slack);
    // Per layer and point: 0 unknown, 1 free, 2 blocked, 3 free only necked down. Per
    // point: via 0 unknown, 1 fits, 2 does not.
    let mut state = vec![0u8; n * nl];
    let mut via_ok = vec![0u8; n];
    let classify = |s: usize| -> u8 {
        let p = pos(s % n);
        if lay.tracks.get(s / n).is_some_and(|o| o.free(p, spare)) {
            1
        } else if in_neck(p, 0.0) && lay.narrow.get(s / n).is_some_and(|o| o.free(p, spare)) {
            3
        } else {
            2
        }
    };
    let look = |s: usize, state: &mut Vec<u8>| -> u8 {
        match state.get(s) {
            Some(0) => {
                let k = classify(s);
                if let Some(x) = state.get_mut(s) {
                    *x = k;
                }
                k
            }
            Some(&k) => k,
            None => 2,
        }
    };
    // Grid ends on layer `l`: the nearest free point whose link to the end is clear, at
    // full width, else (within a neck) necked down; with whether the link is necked. With
    // a neck the link may reach as far as its radius (a straight escape out of a row of
    // fine-pitch pads, whose narrow way holds no grid point free with a step to spare).
    let link = |end: Pt, r: f64, l: usize, state: &mut Vec<u8>| -> Option<(usize, bool)> {
        let rings = if r > 0.0 { ((r / step).ceil() as i64).clamp(6, 200) } else { 6 };
        for narrow in [false, true] {
            if narrow && r <= 0.0 {
                break;
            }
            let obs = if narrow { lay.narrow.get(l)? } else { lay.tracks.get(l)? };
            if !obs.on_board(end) || obs.margin(end, end, 0.0) < -SLACK {
                continue;
            }
            let c0 = cell_at(end);
            let (cx, cy) = ((c0 % nx) as i64, (c0 / nx) as i64);
            let mut best: Option<(f64, usize)> = None;
            for rr in 0..rings {
                for dy in -rr..=rr {
                    for dx in -rr..=rr {
                        if dx.abs() != rr && dy.abs() != rr {
                            continue;
                        }
                        let (x, y) = (cx + dx, cy + dy);
                        if x < 0 || y < 0 || x >= nx as i64 || y >= ny as i64 {
                            continue;
                        }
                        let c = y as usize * nx + x as usize;
                        let k = look(l * n + c, state);
                        if !(k == 1 || narrow && k == 3) {
                            continue;
                        }
                        let p = pos(c);
                        let inside = |a: Pt| !narrow || d(a, end) <= r + step;
                        let ok = [true, false].into_iter().any(|f| {
                            octi(end, p, f)
                                .windows(2)
                                .all(|w| matches!(w, [a, b] if obs.clear(*a, *b) && inside(*a) && inside(*b)))
                        });
                        let dd = d(p, end);
                        if ok && best.is_none_or(|b| dd < b.0) {
                            best = Some((dd, c));
                        }
                    }
                }
                if best.is_some() {
                    break;
                }
            }
            if let Some(b) = best {
                return Some((l * n + b.1, narrow));
            }
        }
        None
    };
    let mut starts: Vec<(usize, bool)> = Vec::new();
    for &l in from_layers.iter().filter(|&&l| l < nl) {
        starts.extend(link(from, r_from, l, &mut state));
    }
    let mut goals: Vec<(usize, bool)> = Vec::new();
    for &l in to_layers.iter().filter(|&&l| l < nl) {
        goals.extend(link(to, r_to, l, &mut state));
    }
    if starts.is_empty() {
        return Err("the start is not clear on its layers");
    }
    if goals.is_empty() {
        return Err("the end is not clear on its layers");
    }
    let via_cost = (VIA_COST_MM / step).max(8.0);
    let octile = |a: usize, b: usize| {
        let dx = ((a % nx) as f64 - (b % nx) as f64).abs();
        let dy = ((a / nx) as f64 - (b / nx) as f64).abs();
        dx.max(dy) + (std::f64::consts::SQRT_2 - 1.0) * dx.min(dy)
    };
    let goal_layers: Vec<usize> = goals.iter().map(|g| g.0 / n).collect();
    let h = |s: usize| {
        let d = goals.iter().map(|g| octile(s % n, g.0 % n)).fold(f64::INFINITY, f64::min) * 1.0001;
        if goal_layers.contains(&(s / n)) { d } else { d + via_cost }
    };
    let mut g = vec![f64::INFINITY; n * nl];
    let mut came = vec![u32::MAX; n * nl];
    let mut heap = BinaryHeap::new();
    for &(s, narrow) in &starts {
        // A necked link costs its length in necked steps.
        let g0 = if narrow { NECK_COST * d(from, pos(s % n)) / step } else { 0.0 };
        if g0 < g.get(s).copied().unwrap_or(f64::INFINITY) {
            if let Some(x) = g.get_mut(s) {
                *x = g0;
            }
            heap.push(Open { f: g0 + h(s), g: g0, s });
        }
    }
    let mut visited = 0usize;
    let mut reached = None;
    while let Some(Open { g: gc, s, .. }) = heap.pop() {
        if goals.iter().any(|g| g.0 == s) {
            reached = Some(s);
            break;
        }
        if gc > g.get(s).copied().unwrap_or(f64::INFINITY) {
            continue;
        }
        visited += 1;
        if visited > MAX_VISITS {
            break;
        }
        if visited.is_multiple_of(1024) && cancelled() {
            return Err("cancelled");
        }
        let (l, c) = (s / n, s % n);
        let (x, y) = ((c % nx) as i64, (c / nx) as i64);
        let back = came.get(s).copied().unwrap_or(u32::MAX) as usize;
        let here = state.get(s).copied().unwrap_or(1);
        let relax = |s2: usize, cost: f64, g: &mut Vec<f64>, came: &mut Vec<u32>, heap: &mut BinaryHeap<Open>| {
            let ng = gc + cost;
            if ng < g.get(s2).copied().unwrap_or(f64::INFINITY) {
                if let Some(v) = g.get_mut(s2) {
                    *v = ng;
                }
                if let Some(v) = came.get_mut(s2) {
                    *v = s as u32;
                }
                heap.push(Open { f: ng + h(s2), g: ng, s: s2 });
            }
        };
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (x2, y2) = (x + dx, y + dy);
            if x2 < 0 || y2 < 0 || x2 >= nx as i64 || y2 >= ny as i64 {
                continue;
            }
            let s2 = l * n + y2 as usize * nx + x2 as usize;
            let k2 = look(s2, &mut state);
            if k2 != 1 && k2 != 3 {
                continue;
            }
            let mut cost = if dx != 0 && dy != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
            // A step at the necked-down width costs more.
            if k2 == 3 || here == 3 {
                cost *= 1.0 + NECK_COST;
            }
            // A small charge for turning keeps runs straight.
            if back != u32::MAX as usize && back / n == l {
                let b = back % n;
                let (px, py) = ((b % nx) as i64, (b / nx) as i64);
                if (x - px, y - py) != (dx, dy) {
                    cost += 0.05;
                }
            }
            relax(s2, cost, &mut g, &mut came, &mut heap);
        }
        // A via to the same point on another layer.
        for l2 in (0..nl).filter(|&l2| l2 != l) {
            let s2 = l2 * n + c;
            if g.get(s2).copied().unwrap_or(0.0) <= gc + via_cost {
                continue;
            }
            let k2 = look(s2, &mut state);
            if k2 != 1 && k2 != 3 {
                continue;
            }
            let fits = match via_ok.get(c) {
                Some(1) => true,
                Some(2) | None => false,
                _ => {
                    let ok = lay.via_fits(pos(c));
                    if let Some(v) = via_ok.get_mut(c) {
                        *v = if ok { 1 } else { 2 };
                    }
                    ok
                }
            };
            if fits {
                relax(s2, via_cost, &mut g, &mut came, &mut heap);
            }
        }
    }
    let goal = reached.ok_or("no way across the layers")?;
    let mut path = vec![goal];
    let mut s = goal;
    while let Some(&c) = came.get(s).filter(|&&c| c != u32::MAX) {
        s = c as usize;
        path.push(s);
        if path.len() > n * nl {
            return Err("no way across the layers");
        }
    }
    path.reverse();
    let start_narrow = starts.iter().any(|&(x, nw)| x == s && nw);
    let end_narrow = goals.iter().any(|&(x, nw)| x == goal && nw);
    // Legs: runs of one layer; a layer change is a via at that point.
    let mut runs: Vec<(usize, Vec<usize>)> = Vec::new();
    for &s in &path {
        match runs.last_mut() {
            Some((l, cs)) if *l == s / n => cs.push(s % n),
            _ => runs.push((s / n, vec![s % n])),
        }
    }
    let mut out = LayerRoute { legs: Vec::new(), vias: Vec::new(), widths: Vec::new() };
    let count = runs.len();
    for (k, (l, cs)) in runs.iter().enumerate() {
        let obs = lay.tracks.get(*l).ok_or("no routable layer")?;
        // The leg's points, each with whether it is free only necked down.
        let narrow_at = |c: usize| state.get(*l * n + c) == Some(&3);
        let mut pts: Vec<(Pt, bool)> = Vec::new();
        if k == 0 {
            pts.push((from, start_narrow));
        }
        pts.extend(cs.iter().map(|&c| (pos(c), narrow_at(c))));
        if k + 1 == count {
            pts.push((to, end_narrow));
        } else if let Some(&c) = cs.last() {
            out.vias.push(pos(c));
        }
        // Pieces of one width: a step is necked when either end is.
        let mut pieces: Vec<(bool, Vec<Pt>)> = Vec::new();
        for w in pts.windows(2) {
            let &[(a, na), (b, nb)] = w else { continue };
            let narrow = na || nb;
            match pieces.last_mut() {
                Some((pn, pp)) if *pn == narrow => pp.push(b),
                _ => pieces.push((narrow, vec![a, b])),
            }
        }
        let narrow_obs = lay.narrow.get(*l);
        let mut leg: Vec<Pt> = Vec::new();
        let mut widths: Vec<f64> = Vec::new();
        for (narrow, pp) in &pieces {
            let pp = route::simplify(pp);
            let tight = match (narrow, narrow_obs) {
                (true, Some(no)) => route::pull_tight(
                    &|a, b| no.clear(a, b) && in_neck(a, 2.0 * step) && in_neck(b, 2.0 * step),
                    &pp,
                    diagonal_first,
                ),
                _ => route::pull_tight(&|a, b| obs.clear(a, b), &pp, diagonal_first),
            };
            let w = if *narrow { lay.neck } else { lay.width };
            for p in tight {
                if leg.last().is_some_and(|q| d(*q, p) < 1e-9) {
                    continue;
                }
                if !leg.is_empty() {
                    widths.push(w);
                }
                leg.push(p);
            }
        }
        if leg.is_empty() {
            leg = pts.first().map(|p| vec![p.0]).unwrap_or_default();
        }
        let name = lay.layers.get(*l).cloned().unwrap_or_default();
        out.legs.push((name, leg));
        out.widths.push(widths);
    }
    // The route's own vias keep the hole-to-hole gap from each other.
    let need = lay.via.drill + rules.hole_to_hole - SLACK;
    for (i, a) in out.vias.iter().enumerate() {
        if out.vias.iter().skip(i + 1).any(|b| libm::hypot(a[0] - b[0], a[1] - b[1]) < need) {
            return Err("the route's own vias are too close");
        }
    }
    Ok(out)
}
