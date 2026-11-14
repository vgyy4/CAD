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
}

impl Layered {
    /// The obstacles for a track of `net` and `width` on each of `layers` (copper layers of
    /// the board; others are left out) and for its vias.
    pub fn new(board: &Board, rules: &Rules, layers: &[String], net: Option<&str>, width: f64) -> Layered {
        let copper = Copper::of(board);
        let cu = board.copper_layers();
        let layers: Vec<String> = layers.iter().filter(|l| cu.contains(l)).cloned().collect();
        let tracks = layers
            .iter()
            .map(|l| {
                Obstacles::with_copper(board, &copper, rules, l, net, width).extended(reliefs(
                    board,
                    rules,
                    l,
                    net,
                    width / 2.0,
                ))
            })
            .collect::<Vec<_>>();
        let via = via_size(rules, net);
        let vias = cu
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
        Layered { layers, tracks, vias, holes: Obstacles::from_prims(prims, 0.0), via, width }
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
    let nl = lay.tracks.len();
    if nl == 0 {
        return Err("no routable layer");
    }
    let cancelled = || cancel.is_some_and(|c| c.load(AtomicOrdering::Relaxed));
    let dist = libm::hypot(to[0] - from[0], to[1] - from[1]);
    let pad = 5.0 + dist * 0.5;
    let lo = [from[0].min(to[0]) - pad, from[1].min(to[1]) - pad];
    let hi = [from[0].max(to[0]) + pad, from[1].max(to[1]) + pad];
    let mut step = grid.clamp(0.02, 1.0);
    let cells = |s: f64| ((hi[0] - lo[0]) / s + 1.0) * ((hi[1] - lo[1]) / s + 1.0);
    while cells(step) > route::MAX_CELLS as f64 || cells(step) * nl as f64 > MAX_STATES as f64 {
        step *= 1.5;
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
    // Per layer and point: 0 unknown, 1 free, 2 blocked. Per point: via 0 unknown, 1 fits,
    // 2 does not.
    let mut state = vec![0u8; n * nl];
    let mut via_ok = vec![0u8; n];
    let free = |s: usize, state: &mut Vec<u8>| -> bool {
        match state.get(s) {
            Some(1) => true,
            Some(0) => {
                let ok = lay.tracks.get(s / n).is_some_and(|o| o.free(pos(s % n), spare));
                if let Some(x) = state.get_mut(s) {
                    *x = if ok { 1 } else { 2 };
                }
                ok
            }
            _ => false,
        }
    };
    // Grid ends on layer `l`: the nearest free point whose link to the end is clear.
    let link = |end: Pt, l: usize, state: &mut Vec<u8>| -> Option<usize> {
        let obs = lay.tracks.get(l)?;
        if !obs.on_board(end) || obs.margin(end, end, 0.0) < -SLACK {
            return None;
        }
        let c0 = cell_at(end);
        let (cx, cy) = ((c0 % nx) as i64, (c0 / nx) as i64);
        let mut best: Option<(f64, usize)> = None;
        for r in 0..6i64 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (x, y) = (cx + dx, cy + dy);
                    if x < 0 || y < 0 || x >= nx as i64 || y >= ny as i64 {
                        continue;
                    }
                    let c = y as usize * nx + x as usize;
                    if !free(l * n + c, state) {
                        continue;
                    }
                    let p = pos(c);
                    let ok = [true, false]
                        .into_iter()
                        .any(|f| octi(end, p, f).windows(2).all(|w| matches!(w, [a, b] if obs.clear(*a, *b))));
                    let d = libm::hypot(p[0] - end[0], p[1] - end[1]);
                    if ok && best.is_none_or(|b| d < b.0) {
                        best = Some((d, c));
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        best.map(|b| l * n + b.1)
    };
    let mut starts: Vec<usize> = Vec::new();
    for &l in from_layers.iter().filter(|&&l| l < nl) {
        starts.extend(link(from, l, &mut state));
    }
    let mut goals: Vec<usize> = Vec::new();
    for &l in to_layers.iter().filter(|&&l| l < nl) {
        goals.extend(link(to, l, &mut state));
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
    let goal_layers: Vec<usize> = goals.iter().map(|g| g / n).collect();
    let h = |s: usize| {
        let d = goals.iter().map(|g| octile(s % n, g % n)).fold(f64::INFINITY, f64::min) * 1.0001;
        if goal_layers.contains(&(s / n)) { d } else { d + via_cost }
    };
    let mut g = vec![f64::INFINITY; n * nl];
    let mut came = vec![u32::MAX; n * nl];
    let mut heap = BinaryHeap::new();
    for &s in &starts {
        if let Some(x) = g.get_mut(s) {
            *x = 0.0;
        }
        heap.push(Open { f: h(s), g: 0.0, s });
    }
    let mut visited = 0usize;
    let mut reached = None;
    while let Some(Open { g: gc, s, .. }) = heap.pop() {
        if goals.contains(&s) {
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
            if !free(s2, &mut state) {
                continue;
            }
            let mut cost = if dx != 0 && dy != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
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
            if g.get(s2).copied().unwrap_or(0.0) <= gc + via_cost || !free(s2, &mut state) {
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
    // Legs: runs of one layer; a layer change is a via at that point.
    let mut runs: Vec<(usize, Vec<usize>)> = Vec::new();
    for &s in &path {
        match runs.last_mut() {
            Some((l, cs)) if *l == s / n => cs.push(s % n),
            _ => runs.push((s / n, vec![s % n])),
        }
    }
    let dir = |a: usize, b: usize| ((b % nx) as i64 - (a % nx) as i64, (b / nx) as i64 - (a / nx) as i64);
    let mut out = LayerRoute { legs: Vec::new(), vias: Vec::new() };
    let count = runs.len();
    for (k, (l, cs)) in runs.iter().enumerate() {
        let obs = lay.tracks.get(*l).ok_or("no routable layer")?;
        let start = if k == 0 { from } else { cs.first().map(|&c| pos(c)).unwrap_or(from) };
        let mut pts = vec![start];
        for (i, &c) in cs.iter().enumerate() {
            let keep = match (i.checked_sub(1).and_then(|j| cs.get(j)), cs.get(i + 1)) {
                (Some(&p), Some(&q)) => dir(p, c) != dir(c, q),
                _ => true,
            };
            if keep {
                pts.push(pos(c));
            }
        }
        if k + 1 == count {
            pts.push(to);
        } else if let Some(&c) = cs.last() {
            out.vias.push(pos(c));
        }
        let pts = route::simplify(&pts);
        let tight = route::pull_tight(&|a, b| obs.clear(a, b), &pts, diagonal_first);
        let name = lay.layers.get(*l).cloned().unwrap_or_default();
        out.legs.push((name, tight));
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
