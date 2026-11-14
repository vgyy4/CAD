//! Push-and-shove (ADR 0058): a new track is laid where the copper of other nets is too
//! close, and that copper is pushed aside keeping its shape.
//!
//! * The new track (the head) is searched with other nets' straight tracks and vias as
//!   soft obstacles: it may pass closer to them than the rules ask (never across their
//!   centre lines), at a cost, so it prefers clear ways; pads, arcs, the edge and the head's own net stay
//!   hard.
//! * Tracks are cut into lines: chains of straight segments of one net and width between
//!   anchors (pads, vias, branches, width changes, free ends). A line's ends stay put.
//! * A line in the way of a placed item (the head, or copper already pushed) is walked
//!   around that item's hull: an octagon round each segment grown by the clearance and
//!   half widths, so the pushed part runs parallel at the needed gap and joins the rest of
//!   the line at 45°. The side is the one that does not cross the pusher.
//! * A pushed line that now hits another line pushes it in turn (at most [`MAX_DEPTH`]
//!   deep and [`MAX_WORK`] pushes in all), across nets. A via in the way is moved the
//!   least distance that clears it (eight directions and away from the pusher), the lines
//!   ending on it re-joined at 45° on this layer and stretched on the others (as dragging
//!   a via does, ADR 0050). Pads never move.
//! * Every pushed line and via is checked against the hard copper of the board; the whole
//!   result is kept only when the design rule check finds no more problems than before.

use crate::board::{Board, Limits, Pt};
use crate::copper::{self, Copper, Prim};
use crate::route::{self, Obstacles, Seg, Soft};
use crate::rules::Rules;
use std::collections::{BTreeMap, VecDeque};

/// How many pushes deep a chain may go.
pub const MAX_DEPTH: usize = 8;
/// Most lines and vias pushed (counting repeats) for one new track.
pub const MAX_WORK: usize = 96;
/// Pushed copper keeps this much beyond the rule (mm).
const EXTRA: f64 = 2e-3;
/// Copper closer than the rule by more than this is in the way (mm).
const HIT: f64 = 1e-4;
/// Longest line that is pushed (points); longer ones stay as they are.
const MAX_LINE_PTS: usize = 400;
/// Extra cost per grid step of the head taken too close to pushable copper.
const PENALTY: f64 = 2.0;
/// Least gap the head's centre line keeps from pushable copper's centre lines while
/// searching (mm, plus half a grid diagonal, so no grid step crosses one): it may run
/// over that copper, which is then pushed, never across it.
const MIN_GAP: f64 = 0.01;

/// A push that works: the new track and what moved to make room.
#[derive(Debug, Clone, PartialEq)]
pub struct Push {
    /// The new track's corner points, `from` first.
    pub path: Vec<Pt>,
    /// Track segments taken up (indices in [`Board::tracks`]).
    pub tracks: Vec<usize>,
    /// The pushed lines laid again: corner points, width, net.
    pub lines: Vec<(Vec<Pt>, f64, Option<String>)>,
    /// Vias moved: index in [`Board::vias`] and the move.
    pub vias: Vec<(usize, Pt)>,
}

#[derive(Debug, Clone)]
struct Line {
    segs: Vec<usize>,
    pts: Vec<Pt>,
    width: f64,
    net: String,
    /// The pushable via (index in `Model::vias`) at each end.
    ends_via: [Option<usize>; 2],
    moved: bool,
}

#[derive(Debug, Clone)]
struct ViaItem {
    idx: usize,
    orig: Pt,
    at: Pt,
    size: f64,
    net: String,
    moved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum It {
    Head(usize),
    Line(usize),
    Via(usize),
}

/// Why a hull walk failed.
enum Block {
    /// The line's end (0 start, 1 end) lies in the way.
    End(usize),
    Fail,
}

struct Model<'a> {
    rules: &'a Rules,
    layer: String,
    lines: Vec<Line>,
    vias: Vec<ViaItem>,
    head: Vec<Vec<Pt>>,
    head_half: f64,
    head_net: Option<String>,
    hard: Board,
    copper: Copper,
    obs: BTreeMap<(String, Option<String>, u64), Obstacles>,
    work: usize,
    /// Why the push failed.
    fail: &'static str,
}

/// A net and a place.
type EndKey = (Option<String>, (i64, i64));

fn key(p: Pt) -> (i64, i64) {
    ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64)
}

fn dist(a: Pt, b: Pt) -> f64 {
    libm::hypot(a[0] - b[0], a[1] - b[1])
}

/// The pieces of a polyline (a single point is one piece of no length).
fn pieces(pts: &[Pt]) -> Vec<(Pt, Pt)> {
    match pts {
        [p] => vec![(*p, *p)],
        _ => pts.windows(2).filter_map(|w| if let [a, b] = w { Some((*a, *b)) } else { None }).collect(),
    }
}

fn bbox(pts: &[Pt]) -> [Pt; 2] {
    pts.iter().fold([[f64::MAX; 2], [f64::MIN; 2]], |[lo, hi], q| {
        [[lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]]
    })
}

/// Smallest distance between two polylines' centre lines.
fn poly_dist(a: &[Pt], b: &[Pt], within: f64) -> f64 {
    if !copper::boxes_near(&bbox(a), &bbox(b), within) {
        return f64::INFINITY;
    }
    let pb = pieces(b);
    let mut m = f64::INFINITY;
    for (p, q) in pieces(a) {
        for &(r, s) in &pb {
            m = m.min(copper::segment_segment(p, q, r, s).0);
        }
    }
    m
}

/// The convex hull of points (counter-clockwise, Y up).
fn convex_hull(mut pts: Vec<Pt>) -> Vec<Pt> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup_by(|a, b| dist(*a, *b) < 1e-12);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: Pt, a: Pt, b: Pt| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Vec<Pt> = Vec::new();
    for &p in &pts {
        while let [.., a, b] = lower.as_slice() {
            if cross(*a, *b, p) <= 0.0 {
                lower.pop();
            } else {
                break;
            }
        }
        lower.push(p);
    }
    let mut upper: Vec<Pt> = Vec::new();
    for &p in pts.iter().rev() {
        while let [.., a, b] = upper.as_slice() {
            if cross(*a, *b, p) <= 0.0 {
                upper.pop();
            } else {
                break;
            }
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The octagonal hull of a segment grown by `g`: its edges keep at least `g` from it.
fn hull(a: Pt, b: Pt, g: f64) -> Vec<Pt> {
    let r = g / libm::cos(std::f64::consts::PI / 8.0);
    let mut pts = Vec::with_capacity(16);
    for c in [a, b] {
        for k in 0..8 {
            let ang = std::f64::consts::PI / 8.0 + k as f64 * std::f64::consts::FRAC_PI_4;
            pts.push([c[0] + r * libm::cos(ang), c[1] + r * libm::sin(ang)]);
        }
    }
    convex_hull(pts)
}

fn length(pts: &[Pt]) -> f64 {
    pts.windows(2).map(|w| if let [a, b] = w { dist(*a, *b) } else { 0.0 }).sum()
}

/// Repeated and collinear points dropped.
fn tidy(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::new();
    for &p in pts {
        if out.last().is_some_and(|q| dist(*q, p) < 1e-5) {
            continue;
        }
        out.push(p);
    }
    route::simplify(&out)
}

/// Where a polyline crosses a closed outline: (piece, along it 0..1, edge, along it 0..1,
/// point), in order along the polyline.
fn crossings(c: &[Pt], o: &[Pt]) -> Vec<(usize, f64, usize, f64, Pt)> {
    let m = o.len();
    let ob = bbox(o);
    let mut xs = Vec::new();
    for (k, w) in c.windows(2).enumerate() {
        let [p, q] = w else { continue };
        if !copper::boxes_near(&bbox(&[*p, *q]), &ob, 0.0) {
            continue;
        }
        for e in 0..m {
            let (Some(&a), Some(&b)) = (o.get(e), o.get((e + 1) % m)) else { continue };
            let r = [q[0] - p[0], q[1] - p[1]];
            let s = [b[0] - a[0], b[1] - a[1]];
            let den = r[0] * s[1] - r[1] * s[0];
            if den.abs() < 1e-15 {
                continue;
            }
            let ap = [a[0] - p[0], a[1] - p[1]];
            let t = (ap[0] * s[1] - ap[1] * s[0]) / den;
            let u = (ap[0] * r[1] - ap[1] * r[0]) / den;
            if (0.0..1.0).contains(&t) && (0.0..1.0).contains(&u) {
                xs.push((k, t, e, u, [p[0] + t * r[0], p[1] + t * r[1]]));
            }
        }
    }
    xs.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
    xs
}

/// The line `c` walked around the outlines of `areas` (see the module notes); `samples`
/// are points of the pushers, which the pushed line must not close round.
fn walk(c: &[Pt], areas: &[Vec<Vec<Pt>>], samples: &[Pt]) -> Result<Vec<Pt>, Block> {
    let mut c = c.to_vec();
    for area in areas {
        let Some(o) = area.first() else { continue };
        let m = o.len();
        if m < 3 {
            continue;
        }
        if c.first().is_some_and(|p| crate::geom::inside(o, *p)) {
            return Err(Block::End(0));
        }
        if c.last().is_some_and(|p| crate::geom::inside(o, *p)) {
            return Err(Block::End(1));
        }
        let xs = crossings(&c, o);
        if xs.is_empty() {
            continue;
        }
        if xs.len() % 2 == 1 {
            return Err(Block::Fail);
        }
        let mf = m as f64;
        let fwd_d = |a: f64, b: f64| (b - a).rem_euclid(mf);
        let mut out: Vec<Pt> = Vec::new();
        let mut next = 0usize;
        for pair in xs.chunks(2) {
            let [(k1, _, e1, t1, x1), (k2, _, e2, t2, x2)] = pair else { return Err(Block::Fail) };
            out.extend(c.get(next..=*k1).ok_or(Block::Fail)?);
            let mut inside = vec![*x1];
            inside.extend(c.get(k1 + 1..=*k2).ok_or(Block::Fail)?);
            inside.push(*x2);
            let (p1, p2) = (*e1 as f64 + t1, *e2 as f64 + t2);
            let mut fwd = vec![*x1];
            for step in 0..m {
                let j = (e1 + 1 + step) % m;
                if fwd_d(p1, j as f64) < fwd_d(p1, p2) {
                    fwd.extend(o.get(j));
                } else {
                    break;
                }
            }
            fwd.push(*x2);
            let mut bwd = vec![*x1];
            for step in 0..m {
                let j = (e1 + m - step % m) % m;
                if fwd_d(j as f64, p1) < fwd_d(p2, p1) {
                    bwd.extend(o.get(j));
                } else {
                    break;
                }
            }
            bwd.push(*x2);
            let enclosed = |path: &[Pt]| {
                let mut ring = inside.clone();
                ring.extend(path.iter().rev().skip(1).take(path.len().saturating_sub(2)));
                samples.iter().filter(|s| crate::geom::inside(&ring, **s)).count()
            };
            let (nf, nb) = (enclosed(&fwd), enclosed(&bwd));
            let chosen = match (nf, nb) {
                (0, 0) => {
                    if length(&fwd) <= length(&bwd) {
                        fwd
                    } else {
                        bwd
                    }
                }
                (0, _) => fwd,
                (_, 0) => bwd,
                _ => return Err(Block::Fail),
            };
            out.extend(chosen);
            next = k2 + 1;
        }
        out.extend(c.get(next..).ok_or(Block::Fail)?);
        c = tidy(&out);
    }
    Ok(c)
}

impl<'a> Model<'a> {
    fn new(board: &Board, rules: &'a Rules, layer: &str, net: Option<&str>, width: f64) -> Option<Model<'a>> {
        let pushable_net = |n: &Option<String>| n.as_deref().is_some_and(|n| !n.is_empty() && Some(n) != net);
        // Every via by place; pushable ones get a model index.
        let mut vias = Vec::new();
        let mut via_at: BTreeMap<(i64, i64), Option<usize>> = BTreeMap::new();
        for (i, v) in board.vias.iter().enumerate() {
            let vn = board.net_name(&v.net);
            let id = if pushable_net(&vn) && board.on_layer(&v.layers, layer) {
                vias.push(ViaItem {
                    idx: i,
                    orig: v.at,
                    at: v.at,
                    size: v.size,
                    net: vn.unwrap_or_default(),
                    moved: false,
                });
                Some(vias.len() - 1)
            } else {
                None
            };
            via_at.entry(key(v.at)).or_insert(id);
        }
        // Tracks of each net on the layer, by end.
        let on_layer: Vec<usize> =
            (0..board.tracks.len()).filter(|&i| board.tracks.get(i).is_some_and(|t| t.layer == layer)).collect();
        let mut ends: BTreeMap<EndKey, Vec<usize>> = BTreeMap::new();
        for &i in &on_layer {
            let Some(t) = board.tracks.get(i) else { continue };
            let n = board.net_name(&t.net);
            for p in [t.start, t.end] {
                ends.entry((n.clone(), key(p))).or_default().push(i);
            }
        }
        let straight =
            |i: usize| board.tracks.get(i).is_some_and(|t| t.mid.is_none() && pushable_net(&board.net_name(&t.net)));
        let on_pad = |p: Pt| {
            board
                .pads
                .iter()
                .any(|pad| board.on_layer(&pad.layers, layer) && dist(pad.at, p) < pad.size[0].max(pad.size[1]) / 2.0)
        };
        let mut used = vec![false; board.tracks.len()];
        let mut lines = Vec::new();
        for &s in &on_layer {
            if used.get(s).copied().unwrap_or(true) || !straight(s) {
                continue;
            }
            let Some(t0) = board.tracks.get(s) else { continue };
            let n = board.net_name(&t0.net);
            if let Some(u) = used.get_mut(s) {
                *u = true;
            }
            let mut segs: VecDeque<usize> = VecDeque::from([s]);
            let mut pts: VecDeque<Pt> = VecDeque::from([t0.start, t0.end]);
            for side in 0..2 {
                loop {
                    let p = if side == 0 { pts.front() } else { pts.back() };
                    let Some(&p) = p else { break };
                    let at = ends.get(&(n.clone(), key(p))).map_or(&[][..], Vec::as_slice);
                    if at.len() != 2 || via_at.contains_key(&key(p)) || on_pad(p) {
                        break;
                    }
                    let Some(&nx) = at.iter().find(|&&i| !segs.contains(&i)) else { break };
                    let Some(t) = board.tracks.get(nx) else { break };
                    if !straight(nx) || used.get(nx).copied().unwrap_or(true) || (t.width - t0.width).abs() > 1e-9 {
                        break;
                    }
                    if let Some(u) = used.get_mut(nx) {
                        *u = true;
                    }
                    let other = if key(t.start) == key(p) { t.end } else { t.start };
                    if side == 0 {
                        segs.push_front(nx);
                        pts.push_front(other);
                    } else {
                        segs.push_back(nx);
                        pts.push_back(other);
                    }
                }
            }
            if pts.len() > MAX_LINE_PTS {
                continue;
            }
            let pts: Vec<Pt> = pts.into_iter().collect();
            let end_via = |p: Option<&Pt>| p.and_then(|p| via_at.get(&key(*p)).copied().flatten());
            let ends_via = [end_via(pts.first()), end_via(pts.last())];
            lines.push(Line {
                segs: segs.into_iter().collect(),
                pts,
                width: t0.width,
                net: n.unwrap_or_default(),
                ends_via,
                moved: false,
            });
        }
        // The board without what may be pushed: the hard copper.
        let mut hard = board.clone();
        let gone: Vec<usize> = lines.iter().flat_map(|l| l.segs.iter().copied()).collect();
        let gone_vias: Vec<usize> = vias.iter().map(|v| v.idx).collect();
        if !gone.is_empty() || !gone_vias.is_empty() {
            hard.delete_items(&gone, &gone_vias, &Limits::default()).ok()?;
        }
        let copper = Copper::of(&hard);
        Some(Model {
            rules,
            layer: layer.to_string(),
            lines,
            vias,
            head: Vec::new(),
            head_half: width / 2.0,
            head_net: net.map(str::to_string),
            hard,
            copper,
            obs: BTreeMap::new(),
            work: 0,
            fail: "",
        })
    }

    /// Hard obstacles on `layer` for copper of `net` and `width`.
    fn hard_obs(&mut self, layer: &str, net: Option<&str>, width: f64) -> &Obstacles {
        let k = (layer.to_string(), net.map(str::to_string), width.to_bits());
        let (hard, copper, rules) = (&self.hard, &self.copper, self.rules);
        self.obs.entry(k).or_insert_with(|| Obstacles::with_copper(hard, copper, rules, layer, net, width))
    }

    fn shape(&self, it: It) -> (Vec<Pt>, f64, Option<&str>) {
        match it {
            It::Head(k) => (self.head.get(k).cloned().unwrap_or_default(), self.head_half, self.head_net.as_deref()),
            It::Line(l) => self
                .lines
                .get(l)
                .map_or((Vec::new(), 0.0, None), |l| (l.pts.clone(), l.width / 2.0, Some(l.net.as_str()))),
            It::Via(v) => {
                self.vias.get(v).map_or((Vec::new(), 0.0, None), |v| (vec![v.at], v.size / 2.0, Some(v.net.as_str())))
            }
        }
    }

    /// The centre distance two items need (`None` for one net).
    fn need(&self, a: (f64, Option<&str>), b: (f64, Option<&str>)) -> Option<f64> {
        if a.1.is_some() && a.1 == b.1 {
            return None;
        }
        Some(self.rules.clearance_of(a.1).max(self.rules.clearance_of(b.1)) + a.0 + b.0)
    }

    /// Whether `pts` (half width, net) comes too close to item `y`.
    fn hits(&self, pts: &[Pt], half: f64, net: Option<&str>, y: It) -> bool {
        let (q, h, n) = self.shape(y);
        let Some(need) = self.need((half, net), (h, n)) else { return false };
        !q.is_empty() && poly_dist(pts, &q, need) < need - HIT
    }

    fn placed(&self) -> Vec<It> {
        let mut out: Vec<It> = (0..self.head.len()).map(It::Head).collect();
        out.extend((0..self.lines.len()).filter(|&l| self.lines.get(l).is_some_and(|l| l.moved)).map(It::Line));
        out.extend((0..self.vias.len()).filter(|&v| self.vias.get(v).is_some_and(|v| v.moved)).map(It::Via));
        out
    }

    /// Pushable items (vias first) that `x` comes too close to.
    fn colliders(&self, x: It) -> Vec<It> {
        let (p, h, n) = self.shape(x);
        let mut out: Vec<It> =
            (0..self.vias.len()).map(It::Via).filter(|&y| y != x && self.hits(&p, h, n, y)).collect();
        out.extend((0..self.lines.len()).map(It::Line).filter(|&y| y != x && self.hits(&p, h, n, y)));
        out
    }

    /// Lays a line clear of every placed item it comes too close to, its ends fixed.
    fn push_line(&mut self, l: usize, queue: &mut VecDeque<(It, usize, bool)>, depth: usize) -> Option<()> {
        self.work += 1;
        if self.work > MAX_WORK {
            self.fail = "too much work";
            return None;
        }
        let mut tries = 0;
        loop {
            tries += 1;
            if tries > 4 {
                self.fail = "no settled push";
                return None;
            }
            let line = self.lines.get(l)?.clone();
            let (half, net) = (line.width / 2.0, Some(line.net.as_str()));
            let placed: Vec<It> = self.placed().into_iter().filter(|&y| y != It::Line(l)).collect();
            // The placed pieces too close: (item, piece index).
            let near = |m: &Self, pts: &[Pt], have: &[(It, usize)]| -> Vec<(It, usize)> {
                let mut out = Vec::new();
                for &y in &placed {
                    let (q, h, n) = m.shape(y);
                    let Some(need) = m.need((half, net), (h, n)) else { continue };
                    for (k, (a, b)) in pieces(&q).into_iter().enumerate() {
                        if !have.contains(&(y, k)) && poly_dist(pts, &[a, b], need) < need - HIT {
                            out.push((y, k));
                        }
                    }
                }
                out
            };
            let mut set = near(self, &line.pts, &[]);
            if set.is_empty() {
                return Some(());
            }
            let mut result = None;
            for _ in 0..6 {
                let mut hulls = Vec::new();
                let mut samples = Vec::new();
                for &(y, k) in &set {
                    let (q, h, n) = self.shape(y);
                    let Some(need) = self.need((half, net), (h, n)) else { continue };
                    let Some(&(a, b)) = pieces(&q).get(k) else { continue };
                    hulls.push(hull(a, b, need + EXTRA));
                    samples.extend([a, b, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]]);
                }
                let areas = crate::poly::union(&hulls);
                match walk(&line.pts, &areas, &samples) {
                    Ok(new) => {
                        let more = near(self, &new, &set);
                        if more.is_empty() {
                            result = Some(Ok(new));
                            break;
                        }
                        set.extend(more);
                    }
                    Err(b) => {
                        result = Some(Err(b));
                        break;
                    }
                }
            }
            let Some(result) = result else {
                self.fail = "no settled push";
                return None;
            };
            match result {
                Ok(new) => {
                    let layer = self.layer.clone();
                    let obs = self.hard_obs(&layer, net, line.width);
                    if new.len() < 2 || !new.windows(2).all(|w| matches!(w, [a, b] if obs.clear(*a, *b))) {
                        self.fail = "fixed copper in the way";
                        return None;
                    }
                    let ln = self.lines.get_mut(l)?;
                    ln.pts = new;
                    ln.moved = true;
                    return Some(());
                }
                // The end in the way is a via: move it, then try again.
                Err(Block::End(e)) => {
                    let Some(v) = line.ends_via.get(e).copied().flatten() else {
                        self.fail = "a line end in the way";
                        return None;
                    };
                    if self.vias.get(v)?.moved {
                        return None;
                    }
                    self.push_via(v, queue, depth)?;
                }
                Err(Block::Fail) => {
                    self.fail = "a line across the pusher";
                    return None;
                }
            }
        }
    }

    /// Moves a via the least distance that clears every placed item; the lines ending on
    /// it follow.
    fn push_via(&mut self, v: usize, queue: &mut VecDeque<(It, usize, bool)>, depth: usize) -> Option<()> {
        self.work += 1;
        if self.work > MAX_WORK {
            self.fail = "too much work";
            return None;
        }
        let via = self.vias.get(v)?.clone();
        let (half, net) = (via.size / 2.0, Some(via.net.as_str()));
        let placed: Vec<It> = self.placed().into_iter().filter(|&y| y != It::Via(v)).collect();
        let near: Vec<It> = placed.iter().copied().filter(|&y| self.hits(&[via.at], half + 3.0, None, y)).collect();
        // Away from the nearest pusher, then the eight directions.
        let mut dirs: Vec<Pt> = Vec::new();
        if let Some(q) = near
            .iter()
            .flat_map(|&y| {
                let (q, _, _) = self.shape(y);
                pieces(&q).into_iter().map(|(a, b)| copper::point_segment(via.at, a, b)).collect::<Vec<_>>()
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|(d, _)| *d > 1e-6)
        {
            dirs.push([(via.at[0] - q.1[0]) / q.0, (via.at[1] - q.1[1]) / q.0]);
        }
        for k in 0..8 {
            let a = k as f64 * std::f64::consts::FRAC_PI_4;
            dirs.push([libm::cos(a), libm::sin(a)]);
        }
        let clear = |m: &Self, at: Pt| {
            near.iter().all(|&y| {
                let (q, h, n) = m.shape(y);
                m.need((half, net), (h, n)).is_none_or(|need| poly_dist(&[at], &q, need) >= need + EXTRA / 2.0)
            })
        };
        // Hard copper on every layer the via reaches.
        let layers = self.copper.layers.clone();
        for layer in &layers {
            self.hard_obs(layer, net, via.size);
        }
        let hard: Vec<&Obstacles> = layers
            .iter()
            .filter_map(|l| self.obs.get(&(l.clone(), net.map(str::to_string), via.size.to_bits())))
            .collect();
        let mut best: Option<(f64, Pt)> = None;
        let mut against_fixed = false;
        for d in &dirs {
            for step in 1..=300 {
                let s = step as f64 * 0.01;
                if best.is_some_and(|b| s >= b.0) {
                    break;
                }
                let at = [via.at[0] + d[0] * s, via.at[1] + d[1] * s];
                if clear(self, at) {
                    if hard.iter().all(|o| o.clear(at, at)) {
                        best = Some((s, at));
                        break;
                    }
                    against_fixed = true;
                }
            }
        }
        let Some((_, at)) = best else {
            self.fail = if against_fixed { "a via against fixed copper" } else { "no room for a via" };
            return None;
        };
        if let Some(m) = self.vias.get_mut(v) {
            m.at = at;
            m.moved = true;
        }
        // Lines ending on it: the last piece re-joined at 45°.
        for l in 0..self.lines.len() {
            let Some(line) = self.lines.get(l) else { continue };
            let mut pts = line.pts.clone();
            let mut touched = false;
            for e in 0..2 {
                if line.ends_via.get(e).copied().flatten() != Some(v) {
                    continue;
                }
                if e == 0 {
                    pts.reverse();
                }
                pts.pop();
                let prev = pts.pop()?;
                let joins = [route::octi(prev, at, true), route::octi(prev, at, false)];
                let score = |j: &Vec<Pt>| {
                    placed
                        .iter()
                        .map(|&y| {
                            let (q, _, _) = self.shape(y);
                            poly_dist(j, &q, 5.0)
                        })
                        .fold(f64::INFINITY, f64::min)
                };
                let [j0, j1] = joins;
                let j = if score(&j1) > score(&j0) + 1e-9 { j1 } else { j0 };
                pts.extend(j);
                if e == 0 {
                    pts.reverse();
                }
                touched = true;
            }
            if touched && let Some(ln) = self.lines.get_mut(l) {
                ln.pts = tidy(&pts);
                ln.moved = true;
                queue.push_back((It::Line(l), depth, true));
            }
        }
        queue.push_back((It::Via(v), depth, false));
        Some(())
    }

    /// Pushes until nothing placed is too close to anything else; `None` when a push fails
    /// or the bounds are reached.
    fn run(&mut self) -> Option<()> {
        let mut queue: VecDeque<(It, usize, bool)> = (0..self.head.len()).map(|k| (It::Head(k), 0, false)).collect();
        while let Some((x, d, fix)) = queue.pop_front() {
            if fix && let It::Line(l) = x {
                self.push_line(l, &mut queue, d)?;
            }
            for y in self.colliders(x) {
                let (p, h, n) = self.shape(x);
                if !self.hits(&p, h, n, y) {
                    continue;
                }
                if d + 1 > MAX_DEPTH {
                    self.fail = "too deep";
                    return None;
                }
                match y {
                    It::Via(v) => self.push_via(v, &mut queue, d + 1)?,
                    It::Line(l) => {
                        self.push_line(l, &mut queue, d + 1)?;
                        queue.push_back((y, d + 1, false));
                    }
                    It::Head(_) => {}
                }
            }
        }
        Some(())
    }
}

/// Tries to lay a track of `net` and `width` from `from` to `to` on `layer` by pushing
/// other nets' tracks and vias aside (see the module notes). `keep` is copper of the new
/// track laid before this leg (its corner points), which pushed copper keeps clear of too.
#[allow(clippy::too_many_arguments)]
pub fn push(
    board: &Board,
    rules: &Rules,
    layer: &str,
    net: Option<&str>,
    width: f64,
    from: Pt,
    to: Pt,
    grid: f64,
    keep: &[Pt],
) -> Option<Push> {
    push_with(board, rules, layer, net, width, from, to, grid, keep, None)
}

/// As [`push`], with the board's design rule check count before when known.
#[allow(clippy::too_many_arguments)]
pub fn push_with(
    board: &Board,
    rules: &Rules,
    layer: &str,
    net: Option<&str>,
    width: f64,
    from: Pt,
    to: Pt,
    grid: f64,
    keep: &[Pt],
    before: Option<usize>,
) -> Option<Push> {
    push_reason(board, rules, layer, net, width, from, to, grid, keep, before).ok()
}

/// As [`push_with`], saying why when there is no push.
///
/// # Errors
/// Why the track could not be laid by pushing.
#[allow(clippy::too_many_arguments)]
pub fn push_reason(
    board: &Board,
    rules: &Rules,
    layer: &str,
    net: Option<&str>,
    width: f64,
    from: Pt,
    to: Pt,
    grid: f64,
    keep: &[Pt],
    before: Option<usize>,
) -> Result<Push, &'static str> {
    let mut m = Model::new(board, rules, layer, net, width).ok_or("a failed edit")?;
    // The head: hard copper as walls, pushable copper soft.
    let half = width / 2.0;
    let mine = rules.clearance_of(net);
    let mut full = Vec::new();
    let mut min = Vec::new();
    for l in &m.lines {
        let need = mine.max(rules.clearance_of(Some(&l.net))) + half;
        for (a, b) in pieces(&l.pts) {
            full.push((Prim::Capsule { a, b, r: l.width / 2.0 }, need));
            min.push((Prim::Capsule { a, b, r: 0.0 }, MIN_GAP));
        }
    }
    for v in &m.vias {
        let need = mine.max(rules.clearance_of(Some(&v.net))) + half;
        full.push((Prim::Capsule { a: v.at, b: v.at, r: v.size / 2.0 }, need));
        min.push((Prim::Capsule { a: v.at, b: v.at, r: 0.0 }, MIN_GAP));
    }
    let soft =
        Soft { full: Obstacles::from_prims(full, half), min: Obstacles::from_prims(min, half), penalty: PENALTY };
    let layer_s = layer.to_string();
    let obs = m.hard_obs(&layer_s, net, width).clone();
    let path = [true, false]
        .into_iter()
        .find_map(|d| route::walkaround_soft(&obs, &soft, from, to, grid, d))
        .ok_or("no way even pushing")?;
    let path = route::simplify(&path);
    m.head.push(path.clone());
    if keep.len() >= 2 {
        m.head.push(keep.to_vec());
    }
    if m.run().is_none() {
        return Err(if m.fail.is_empty() { "a failed push" } else { m.fail });
    }
    let mut out = Push { path, tracks: Vec::new(), lines: Vec::new(), vias: Vec::new() };
    for l in m.lines.iter().filter(|l| l.moved) {
        out.tracks.extend(&l.segs);
        out.lines.push((l.pts.clone(), l.width, Some(l.net.clone())));
    }
    for v in m.vias.iter().filter(|v| v.moved) {
        out.vias.push((v.idx, [v.at[0] - v.orig[0], v.at[1] - v.orig[1]]));
    }
    // Kept only when the check finds nothing new.
    let before = before.unwrap_or_else(|| crate::drc::check(board, rules, 100_000).len());
    let mut after = board.clone();
    after.apply_push(&out, layer, &Limits::default()).map_err(|_| "a failed edit")?;
    let segs = lines(&out.path);
    if !segs.is_empty() {
        after.add_route(&segs, width, layer, net, &Limits::default()).map_err(|_| "a failed edit")?;
    }
    if crate::drc::check(&after, rules, 100_000).len() > before {
        return Err("the check would be worse");
    }
    Ok(out)
}

fn lines(pts: &[Pt]) -> Vec<Seg> {
    pts.windows(2)
        .filter_map(|w| match w {
            [a, b] if dist(*a, *b) > 1e-6 => Some(Seg { start: *a, end: *b, mid: None }),
            _ => None,
        })
        .collect()
}

impl Board {
    /// Moves what `p` pushed (the new track itself is laid by the caller): vias moved
    /// with the track ends on them, the pushed lines' old segments replaced by their new
    /// ones, in one edit each.
    ///
    /// # Errors
    /// A failed edit.
    pub fn apply_push(&mut self, p: &Push, layer: &str, limits: &Limits) -> Result<(), crate::board::Error> {
        for &(v, d) in &p.vias {
            self.move_items(&[], &[v], d, true, limits)?;
        }
        if !p.tracks.is_empty() {
            self.delete_items(&p.tracks, &[], limits)?;
        }
        let batch: Vec<crate::edit::NewRoute> = p
            .lines
            .iter()
            .map(|(pts, w, n)| (lines(pts), *w, layer.to_string(), n.clone()))
            .filter(|r| !r.0.is_empty())
            .collect();
        if !batch.is_empty() {
            self.add_batch(&batch, &[], limits)?;
        }
        Ok(())
    }
}
