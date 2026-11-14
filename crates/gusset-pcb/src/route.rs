//! Interactive routing (ADR 0047 phase 2b): a track of one net from a point to another on
//! one copper layer, in 45° segments, walking around the copper of other nets with the
//! clearance the rules ask (the larger of the two nets' classes), the board edge (copper
//! to edge) and non-plated holes, inside the board outline.
//!
//! The search is A* over an octilinear grid in a window around the two ends; a grid point
//! is free when its gap to every obstacle is at least the needed gap plus half a grid
//! diagonal, so the straight moves between free points are clear too. The grid path is
//! then pulled tight: from each corner, the farthest corner reachable by one or two 45°
//! segments that are clear (checked exactly) is taken, so the track hugs the obstacles.
//! Corners can be rounded into arcs where the arc stays clear.

use crate::board::{Board, PadKind, Pt};
use crate::copper::{self, Copper, Prim};
use crate::geom;
use crate::rules::Rules;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Most grid points one search may visit.
const MAX_CELLS: usize = 1_500_000;
/// Gaps may fall short of the rule by this (mm), as in the DRC.
const SLACK: f64 = 4e-4;

/// One piece of a routed track: a line, or an arc through `mid`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    pub start: Pt,
    pub end: Pt,
    pub mid: Option<Pt>,
}

/// What a route keeps clear of on one layer.
#[derive(Debug, Clone)]
pub struct Obstacles {
    /// Pieces of copper (or edge, hole) and the gap a track's centre line must keep.
    prims: Vec<(Prim, f64)>,
    lo: Pt,
    cell: f64,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<u32>>,
    /// Board outline loops (a point is on the board inside an odd number of them).
    outline: Vec<Vec<Pt>>,
    /// The track's half width.
    pub half_width: f64,
}

fn bbox_of(p: &Prim) -> [Pt; 2] {
    match p {
        Prim::Capsule { a, b, r } => {
            [[a[0].min(b[0]) - r, a[1].min(b[1]) - r], [a[0].max(b[0]) + r, a[1].max(b[1]) + r]]
        }
        Prim::Poly { pts } => pts.iter().fold([[f64::MAX; 2], [f64::MIN; 2]], |[lo, hi], q| {
            [[lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]]
        }),
    }
}

impl Obstacles {
    /// The obstacles for a track of `net` and `width` on `layer`: copper of other nets
    /// (and copper on no net), keep-outs that forbid tracks, non-plated holes, the board
    /// edge.
    pub fn new(board: &Board, rules: &Rules, layer: &str, net: Option<&str>, width: f64) -> Obstacles {
        Obstacles::with_copper(board, &Copper::of(board), rules, layer, net, width)
    }

    /// As [`Obstacles::new`] with the board's copper already gathered.
    pub fn with_copper(
        board: &Board,
        copper: &Copper,
        rules: &Rules,
        layer: &str,
        net: Option<&str>,
        width: f64,
    ) -> Obstacles {
        let skip: Vec<&str> = net.into_iter().collect();
        Obstacles::skipping(board, copper, rules, layer, net, &skip, width)
    }

    /// As [`Obstacles::with_copper`] for a track that may cross the copper of every net in
    /// `skip` (a differential pair's two nets); `net` still names the clearance class.
    pub fn skipping(
        board: &Board,
        copper: &Copper,
        rules: &Rules,
        layer: &str,
        net: Option<&str>,
        skip: &[&str],
        width: f64,
    ) -> Obstacles {
        let half = width / 2.0;
        let mine = rules.clearance_of(net);
        let mut prims: Vec<(Prim, f64)> = Vec::new();
        if let Some(li) = copper.layers.iter().position(|l| l == layer) {
            for it in copper.items.iter().filter(|it| it.layers.contains(&li)) {
                if it.net.as_deref().is_some_and(|n| skip.contains(&n)) {
                    continue;
                }
                let need = mine.max(rules.clearance_of(it.net.as_deref())) + half;
                prims.extend(it.prims.iter().map(|p| (p.clone(), need)));
            }
        }
        for g in board.graphics.iter().filter(|g| g.layer == layer) {
            let need = mine + half + g.width / 2.0;
            if let crate::board::Shape::Line { a, b } = g.shape {
                prims.push((Prim::Capsule { a, b, r: 0.0 }, need));
            }
        }
        for (a, b, r) in crate::font::layer_strokes(board, layer) {
            prims.push((Prim::Capsule { a, b, r }, mine + half));
        }
        for p in board.pads.iter().filter(|p| p.kind == PadKind::Npth) {
            if let Some(d) = p.drill {
                let h = crate::drill::pad_hole(p.at, p.angle, &d);
                prims
                    .push((Prim::Capsule { a: h.at, b: h.slot_end.unwrap_or(h.at), r: h.diameter / 2.0 }, mine + half));
            }
        }
        for z in board.zones.iter().filter(|z| z.keepout && board.on_layer(&z.layers, layer)) {
            let no_tracks = board
                .tree
                .items()
                .get(z.node)
                .and_then(|n| n.child("keepout"))
                .and_then(|k| k.child("tracks"))
                .and_then(|t| t.arg(0))
                == Some("not_allowed");
            if no_tracks {
                prims.extend(z.outlines().map(|o| (Prim::Poly { pts: o.clone() }, half)));
            }
        }
        let (loops, _) = geom::outline_loops(board, crate::check::OUTLINE_TOL);
        let outline: Vec<Vec<Pt>> = loops.iter().map(|l| l.polygon(0.01)).collect();
        for l in &outline {
            let n = l.len();
            for i in 0..n {
                if let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) {
                    prims.push((Prim::Capsule { a: *a, b: *b, r: 0.0 }, rules.edge_clearance + half));
                }
            }
        }
        Obstacles::build(prims, outline, half)
    }

    /// Obstacles made of the given pieces and the gaps a centre line keeps from them, with
    /// no board outline (every point is on the board).
    pub fn from_prims(prims: Vec<(Prim, f64)>, half_width: f64) -> Obstacles {
        Obstacles::build(prims, Vec::new(), half_width)
    }

    fn build(prims: Vec<(Prim, f64)>, outline: Vec<Vec<Pt>>, half: f64) -> Obstacles {
        // Buckets over the obstacles' grown boxes.
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for (p, need) in &prims {
            let b = bbox_of(p);
            lo = [lo[0].min(b[0][0] - need), lo[1].min(b[0][1] - need)];
            hi = [hi[0].max(b[1][0] + need), hi[1].max(b[1][1] + need)];
        }
        if lo[0] > hi[0] {
            (lo, hi) = ([0.0, 0.0], [1.0, 1.0]);
        }
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
        let side = ((prims.len() as f64).sqrt().ceil() as usize).clamp(1, 512);
        let cell = span / side as f64;
        let nx = (((hi[0] - lo[0]) / cell).floor() as usize + 1).min(513);
        let ny = (((hi[1] - lo[1]) / cell).floor() as usize + 1).min(513);
        let mut o = Obstacles {
            prims: Vec::new(),
            lo,
            cell,
            nx,
            ny,
            buckets: vec![Vec::new(); nx * ny],
            outline,
            half_width: half,
        };
        for (k, (p, need)) in prims.iter().enumerate() {
            let b = bbox_of(p);
            let (x0, y0) = o.cell_of([b[0][0] - need, b[0][1] - need]);
            let (x1, y1) = o.cell_of([b[1][0] + need, b[1][1] + need]);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if let Some(c) = o.buckets.get_mut(y * nx + x) {
                        c.push(k as u32);
                    }
                }
            }
        }
        o.prims = prims;
        o
    }

    fn cell_of(&self, p: Pt) -> (usize, usize) {
        let f = |v: f64, lo: f64, n: usize| (((v - lo) / self.cell).floor().max(0.0) as usize).min(n.saturating_sub(1));
        (f(p[0], self.lo[0], self.nx), f(p[1], self.lo[1], self.ny))
    }

    fn near(&self, lo: Pt, hi: Pt) -> Vec<u32> {
        let (x0, y0) = self.cell_of(lo);
        let (x1, y1) = self.cell_of(hi);
        let mut ids = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some(c) = self.buckets.get(y * self.nx + x) {
                    ids.extend_from_slice(c);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Whether `p` is on the board (inside the outline; anywhere when there is none).
    pub fn on_board(&self, p: Pt) -> bool {
        self.outline.is_empty() || self.outline.iter().filter(|l| geom::inside(l, p)).count() % 2 == 1
    }

    /// The smallest margin (gap minus the needed gap) of a segment's centre line, looking
    /// only at obstacles within `extra` of what they need; `f64::INFINITY` when none.
    pub fn margin(&self, a: Pt, b: Pt, extra: f64) -> f64 {
        let lo = [a[0].min(b[0]) - extra, a[1].min(b[1]) - extra];
        let hi = [a[0].max(b[0]) + extra, a[1].max(b[1]) + extra];
        let seg = Prim::Capsule { a, b, r: 0.0 };
        let mut m = f64::INFINITY;
        for k in self.near(lo, hi) {
            let Some((p, need)) = self.prims.get(k as usize) else { continue };
            let pb = bbox_of(p);
            if !copper::boxes_near(&pb, &[lo, hi], *need) {
                continue;
            }
            m = m.min(copper::gap(&seg, p).0 - need);
        }
        m
    }

    /// Whether a straight track piece from `a` to `b` keeps every clearance.
    pub fn clear(&self, a: Pt, b: Pt) -> bool {
        self.margin(a, b, 0.0) >= -SLACK && self.on_board(a) && self.on_board(b)
    }

    /// Whether a point is free with `extra` to spare.
    fn free(&self, p: Pt, extra: f64) -> bool {
        self.margin(p, p, extra) >= extra && self.on_board(p)
    }
}

/// One or two 45° segments from `a` to `b`: the diagonal part first (`diagonal_first`)
/// or last. A straight or diagonal line gives one segment.
pub fn octi(a: Pt, b: Pt, diagonal_first: bool) -> Vec<Pt> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax < 1e-9 || ay < 1e-9 || (ax - ay).abs() < 1e-9 {
        return vec![a, b];
    }
    let d = ax.min(ay);
    let diag = [d * dx.signum(), d * dy.signum()];
    let mid = if diagonal_first { [a[0] + diag[0], a[1] + diag[1]] } else { [b[0] - diag[0], b[1] - diag[1]] };
    vec![a, mid, b]
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Open {
    f: f64,
    g: f64,
    cell: usize,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then_with(|| self.g.total_cmp(&o.g))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// A route from `from` to `to` around the obstacles in 45° segments (its corner points,
/// `from` first, `to` last), or `None` when there is no way within the search window.
/// `diagonal_first` picks the bend of a free two-segment route.
pub fn walkaround(obs: &Obstacles, from: Pt, to: Pt, grid: f64, diagonal_first: bool) -> Option<Vec<Pt>> {
    search(obs, None, from, to, grid, diagonal_first)
}

/// Copper a route may pass closer to than the rules ask, as it can be pushed aside
/// (ADR 0058): `full` with the gaps the rules ask, `min` with the least gap allowed (no
/// overlap), and the extra cost per grid step taken within `full`.
#[derive(Debug, Clone)]
pub struct Soft {
    pub full: Obstacles,
    pub min: Obstacles,
    pub penalty: f64,
}

/// As [`walkaround`], but the copper in `soft` may be come closer to than the rules ask
/// (never overlapped), at a cost per step; the route prefers clear ways.
pub fn walkaround_soft(
    obs: &Obstacles,
    soft: &Soft,
    from: Pt,
    to: Pt,
    grid: f64,
    diagonal_first: bool,
) -> Option<Vec<Pt>> {
    search(obs, Some(soft), from, to, grid, diagonal_first)
}

fn search(obs: &Obstacles, soft: Option<&Soft>, from: Pt, to: Pt, grid: f64, diagonal_first: bool) -> Option<Vec<Pt>> {
    if !obs.on_board(to) || obs.margin(to, to, 0.0) < -SLACK {
        return None;
    }
    // Clear for a laid piece: the hard obstacles, and the soft ones' least gap.
    let clear_min = |a: Pt, b: Pt| obs.clear(a, b) && soft.is_none_or(|s| s.min.clear(a, b));
    // Clear with every gap the rules ask.
    let clear_full = |a: Pt, b: Pt| obs.clear(a, b) && soft.is_none_or(|s| s.full.clear(a, b));
    // The direct ways first.
    for first in [diagonal_first, !diagonal_first] {
        let p = octi(from, to, first);
        if p.windows(2).all(|w| matches!(w, [a, b] if clear_full(*a, *b))) {
            return Some(p);
        }
    }
    let dist = libm::hypot(to[0] - from[0], to[1] - from[1]);
    let pad = 5.0 + dist * 0.5;
    let lo = [from[0].min(to[0]) - pad, from[1].min(to[1]) - pad];
    let hi = [from[0].max(to[0]) + pad, from[1].max(to[1]) + pad];
    let mut step = grid.clamp(0.02, 1.0);
    while ((hi[0] - lo[0]) / step) * ((hi[1] - lo[1]) / step) > MAX_CELLS as f64 {
        step *= 1.5;
    }
    let nx = ((hi[0] - lo[0]) / step).ceil() as usize + 1;
    let ny = ((hi[1] - lo[1]) / step).ceil() as usize + 1;
    let pos = |c: usize| [lo[0] + (c % nx) as f64 * step, lo[1] + (c / nx) as f64 * step];
    let cell_at = |p: Pt| {
        let x = (((p[0] - lo[0]) / step).round().max(0.0) as usize).min(nx - 1);
        let y = (((p[1] - lo[1]) / step).round().max(0.0) as usize).min(ny - 1);
        y * nx + x
    };
    let spare = step * std::f64::consts::FRAC_1_SQRT_2;
    // 0 unknown, 1 free, 2 blocked, 3 free but within a soft obstacle's full gap.
    let mut state = vec![0u8; nx * ny];
    let classify = |c: usize| -> u8 {
        let p = pos(c);
        if !obs.free(p, spare) {
            return 2;
        }
        match soft {
            None => 1,
            Some(s) if !s.min.free(p, spare) => 2,
            Some(s) if !s.full.free(p, spare) => 3,
            Some(_) => 1,
        }
    };
    let mut free = |c: usize, state: &mut Vec<u8>| -> bool {
        match state.get(c) {
            Some(1 | 3) => true,
            Some(2) | None => false,
            _ => {
                let f = classify(c);
                if let Some(s) = state.get_mut(c) {
                    *s = f;
                }
                f != 2
            }
        }
    };
    // Grid ends: free points near the true ends whose link to them is clear.
    let link = |end: Pt, state: &mut Vec<u8>, free: &mut dyn FnMut(usize, &mut Vec<u8>) -> bool| {
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
                    if !free(c, state) {
                        continue;
                    }
                    let p = pos(c);
                    let ok = octi(end, p, true).windows(2).all(|w| matches!(w, [a, b] if clear_min(*a, *b)))
                        || octi(end, p, false).windows(2).all(|w| matches!(w, [a, b] if clear_min(*a, *b)));
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
        best.map(|b| b.1)
    };
    let start = link(from, &mut state, &mut free)?;
    let goal = link(to, &mut state, &mut free)?;
    // A*: octile moves, octile distance as the estimate.
    let h = |c: usize| {
        let (dx, dy) = (((c % nx) as f64 - (goal % nx) as f64).abs(), ((c / nx) as f64 - (goal / nx) as f64).abs());
        (dx.max(dy) + (std::f64::consts::SQRT_2 - 1.0) * dx.min(dy)) * 1.0001
    };
    let mut g = vec![f64::INFINITY; nx * ny];
    let mut came = vec![usize::MAX; nx * ny];
    let mut heap = BinaryHeap::new();
    if let Some(s) = g.get_mut(start) {
        *s = 0.0;
    }
    heap.push(Open { f: h(start), g: 0.0, cell: start });
    let mut visited = 0usize;
    let mut found = false;
    while let Some(Open { g: gc, cell, .. }) = heap.pop() {
        if cell == goal {
            found = true;
            break;
        }
        if gc > g.get(cell).copied().unwrap_or(f64::INFINITY) {
            continue;
        }
        visited += 1;
        if visited > MAX_CELLS {
            break;
        }
        let (x, y) = ((cell % nx) as i64, (cell / nx) as i64);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (x2, y2) = (x + dx, y + dy);
            if x2 < 0 || y2 < 0 || x2 >= nx as i64 || y2 >= ny as i64 {
                continue;
            }
            let c2 = y2 as usize * nx + x2 as usize;
            if !free(c2, &mut state) {
                continue;
            }
            let mut cost = if dx != 0 && dy != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
            if state.get(c2) == Some(&3) {
                cost += soft.map_or(0.0, |s| s.penalty);
            }
            // A small charge for turning keeps runs straight.
            let back = came.get(cell).copied().unwrap_or(usize::MAX);
            let turn = if back != usize::MAX {
                let (px, py) = ((back % nx) as i64, (back / nx) as i64);
                if (x - px, y - py) == (dx, dy) { 0.0 } else { 0.05 }
            } else {
                0.0
            };
            let ng = gc + cost + turn;
            if ng < g.get(c2).copied().unwrap_or(f64::INFINITY) {
                if let Some(s) = g.get_mut(c2) {
                    *s = ng;
                }
                if let Some(s) = came.get_mut(c2) {
                    *s = cell;
                }
                heap.push(Open { f: ng + h(c2), g: ng, cell: c2 });
            }
        }
    }
    if !found {
        return None;
    }
    let mut cells = vec![goal];
    let mut c = goal;
    while c != start {
        c = *came.get(c)?;
        cells.push(c);
        if cells.len() > nx * ny {
            return None;
        }
    }
    cells.reverse();
    // Corner points of the grid path (direction changes), between the true ends.
    let mut pts = vec![from];
    let dir = |a: usize, b: usize| ((b % nx) as i64 - (a % nx) as i64, (b / nx) as i64 - (a / nx) as i64);
    for (i, &c) in cells.iter().enumerate() {
        let keep = match (i.checked_sub(1).and_then(|k| cells.get(k)), cells.get(i + 1)) {
            (Some(&p), Some(&n)) => dir(p, c) != dir(c, n),
            _ => true,
        };
        if keep {
            pts.push(pos(c));
        }
    }
    pts.push(to);
    Some(pull_tight(&clear_full, &pts, diagonal_first))
}

/// Shortens a clear path: from each kept point, the farthest later point reachable by one
/// or two 45° segments that are `clear` is joined directly.
fn pull_tight(clear: &dyn Fn(Pt, Pt) -> bool, pts: &[Pt], diagonal_first: bool) -> Vec<Pt> {
    let n = pts.len();
    let mut out: Vec<Pt> = Vec::new();
    let mut i = 0usize;
    let Some(&first) = pts.first() else { return Vec::new() };
    out.push(first);
    while i + 1 < n {
        let a = pts.get(i).copied().unwrap_or(first);
        let mut done = false;
        for j in (i + 1..n).rev() {
            let b = pts.get(j).copied().unwrap_or(a);
            for f in [diagonal_first, !diagonal_first] {
                let p = octi(a, b, f);
                if p.windows(2).all(|w| matches!(w, [u, v] if clear(*u, *v))) {
                    out.extend(p.iter().skip(1));
                    i = j;
                    done = true;
                    break;
                }
            }
            if done {
                break;
            }
        }
        if !done {
            // The grid step itself (clear by construction): keep it.
            let b = pts.get(i + 1).copied().unwrap_or(a);
            out.extend(octi(a, b, diagonal_first).into_iter().skip(1));
            i += 1;
        }
    }
    simplify(&out)
}

/// Drops repeated points and joins collinear runs.
pub fn simplify(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::new();
    for &p in pts {
        if out.last().is_some_and(|q| libm::hypot(q[0] - p[0], q[1] - p[1]) < 1e-9) {
            continue;
        }
        if let [.., a, b] = out.as_slice() {
            let c = (b[0] - a[0]) * (p[1] - b[1]) - (b[1] - a[1]) * (p[0] - b[0]);
            let dot = (b[0] - a[0]) * (p[0] - b[0]) + (b[1] - a[1]) * (p[1] - b[1]);
            if c.abs() < 1e-9 && dot > 0.0 {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// The path as track pieces, its corners rounded into arcs of up to `radius` where the
/// arc keeps clear (else the corner stays sharp).
pub fn with_arcs(obs: &Obstacles, pts: &[Pt], radius: f64) -> Vec<Seg> {
    let lines = |p: &[Pt]| -> Vec<Seg> {
        p.windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(Seg { start: *a, end: *b, mid: None }),
                _ => None,
            })
            .collect()
    };
    if radius <= 0.0 || pts.len() < 3 {
        return lines(pts);
    }
    let mut out: Vec<Seg> = Vec::new();
    let mut cur = pts.first().copied().unwrap_or([0.0, 0.0]);
    for k in 1..pts.len() - 1 {
        let (Some(&b), Some(&a0), Some(&c)) = (pts.get(k), pts.get(k - 1), pts.get(k + 1)) else { continue };
        let u = norm([b[0] - a0[0], b[1] - a0[1]]);
        let v = norm([c[0] - b[0], c[1] - b[1]]);
        let turn = libm::acos((u[0] * v[0] + u[1] * v[1]).clamp(-1.0, 1.0));
        let room_in = libm::hypot(b[0] - cur[0], b[1] - cur[1]);
        let room_out = libm::hypot(c[0] - b[0], c[1] - b[1]) / 2.0;
        let mut r = radius;
        let mut placed = false;
        for _ in 0..4 {
            let t = r * libm::tan(turn / 2.0);
            if turn > 1e-6 && t <= room_in + 1e-9 && t <= room_out + 1e-9 {
                let p1 = [b[0] - u[0] * t, b[1] - u[1] * t];
                let p2 = [b[0] + v[0] * t, b[1] + v[1] * t];
                // The arc's middle: from the corner towards the centre.
                let bis = norm([v[0] - u[0], v[1] - u[1]]);
                let dc = r / libm::cos(turn / 2.0);
                let centre = [b[0] + bis[0] * dc, b[1] + bis[1] * dc];
                let mid = [centre[0] - bis[0] * r, centre[1] - bis[1] * r];
                let chords = geom::arc_points(p1, mid, p2, 0.005);
                let ok = obs.clear(cur, p1) && chords.windows(2).all(|w| matches!(w, [x, y] if obs.clear(*x, *y)));
                if ok {
                    if libm::hypot(p1[0] - cur[0], p1[1] - cur[1]) > 1e-9 {
                        out.push(Seg { start: cur, end: p1, mid: None });
                    }
                    out.push(Seg { start: p1, end: p2, mid: Some(mid) });
                    cur = p2;
                    placed = true;
                    break;
                }
            }
            r /= 2.0;
        }
        if !placed {
            out.push(Seg { start: cur, end: b, mid: None });
            cur = b;
        }
    }
    if let Some(&last) = pts.last()
        && libm::hypot(last[0] - cur[0], last[1] - cur[1]) > 1e-9
    {
        out.push(Seg { start: cur, end: last, mid: None });
    }
    out
}

fn norm(v: Pt) -> Pt {
    let l = libm::hypot(v[0], v[1]);
    if l > 0.0 { [v[0] / l, v[1] / l] } else { [0.0, 0.0] }
}

/// Whether a through via of `net` and `size` at `at` keeps its clearances on every
/// copper layer.
pub fn via_clear(board: &Board, rules: &Rules, at: Pt, net: Option<&str>, size: f64) -> bool {
    let copper = Copper::of(board);
    copper.layers.iter().all(|l| {
        let obs = Obstacles::with_copper(board, &copper, rules, l, net, size);
        obs.clear(at, at)
    })
}

/// The total length of a route (mm).
pub fn length(segs: &[Seg]) -> f64 {
    segs.iter()
        .map(|s| match s.mid {
            Some(m) => geom::arc_length(s.start, m, s.end),
            None => libm::hypot(s.end[0] - s.start[0], s.end[1] - s.start[1]),
        })
        .sum()
}
