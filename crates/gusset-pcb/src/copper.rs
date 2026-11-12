//! The board's copper as simple shapes for connectivity and clearance: pads as polygons
//! (round pads as discs), tracks as capsules (arcs as short chords), vias as discs.

use crate::board::{Board, PadKind, Pt, Shape};
use crate::geom::{self, Contour};

/// A piece of copper: a capsule (segment `a`–`b` grown by `r`; a disc when `a == b`) or a
/// filled polygon.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Capsule { a: Pt, b: Pt, r: f64 },
    Poly { pts: Vec<Pt> },
}

/// Which board item a piece of copper is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemRef {
    Pad(usize),
    Track(usize),
    Via(usize),
}

/// One copper item: its pieces, copper layers (indices into [`Copper::layers`]) and net.
#[derive(Debug, Clone)]
pub struct Item {
    pub what: ItemRef,
    pub layers: Vec<usize>,
    /// The net's name; `None` for copper on no net.
    pub net: Option<String>,
    pub prims: Vec<Prim>,
    /// Bounding box (min, max).
    pub bbox: [Pt; 2],
    /// Points ratsnest lines start from (pad centre, via centre, track ends).
    pub anchors: Vec<Pt>,
}

/// The copper of a board.
#[derive(Debug, Clone)]
pub struct Copper {
    /// Copper layer names, front to back.
    pub layers: Vec<String>,
    pub items: Vec<Item>,
}

/// Chord tolerance for arcs and rounded pads (mm).
const TOL: f64 = 0.005;

fn bbox_of(prims: &[Prim]) -> [Pt; 2] {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let mut add = |p: Pt, r: f64| {
        lo = [lo[0].min(p[0] - r), lo[1].min(p[1] - r)];
        hi = [hi[0].max(p[0] + r), hi[1].max(p[1] + r)];
    };
    for p in prims {
        match p {
            Prim::Capsule { a, b, r } => {
                add(*a, *r);
                add(*b, *r);
            }
            Prim::Poly { pts } => pts.iter().for_each(|q| add(*q, 0.0)),
        }
    }
    [lo, hi]
}

fn contour_prim(c: Contour) -> Prim {
    match c {
        Contour::Circle { center, radius } => Prim::Capsule { a: center, b: center, r: radius },
        c => Prim::Poly { pts: c.polygon(TOL) },
    }
}

impl Copper {
    /// The copper of `board` (no zones: their fills are KiCad's computed result).
    pub fn of(board: &Board) -> Copper {
        let layers = board.copper_layers();
        let index = |name: &str| layers.iter().position(|l| l == name);
        let mut items = Vec::new();
        let net_of = |r: &crate::board::NetRef| board.net_name(r);
        for (i, pad) in board.pads.iter().enumerate() {
            // A non-plated hole has no copper unless its pad is larger than the hole.
            if pad.kind == PadKind::Npth
                && pad.drill.is_some_and(|d| d.size[0].min(d.size[1]) >= pad.size[0].min(pad.size[1]))
            {
                continue;
            }
            let on: Vec<usize> =
                layers.iter().enumerate().filter(|(_, l)| board.on_layer(&pad.layers, l)).map(|(k, _)| k).collect();
            if on.is_empty() {
                continue;
            }
            let mut prims: Vec<Prim> = geom::pad_contour(pad, 0.0).map(contour_prim).into_iter().collect();
            for s in &pad.primitives {
                match s {
                    Shape::Poly { pts } if pts.len() >= 3 => prims.push(Prim::Poly { pts: pts.clone() }),
                    Shape::Circle { center, radius } => {
                        prims.push(Prim::Capsule { a: *center, b: *center, r: *radius })
                    }
                    _ => {}
                }
            }
            if prims.is_empty() {
                continue;
            }
            items.push(Item {
                what: ItemRef::Pad(i),
                layers: on,
                net: net_of(&pad.net),
                bbox: bbox_of(&prims),
                prims,
                anchors: vec![pad.at],
            });
        }
        for (i, t) in board.tracks.iter().enumerate() {
            let Some(l) = index(&t.layer) else { continue };
            let r = t.width / 2.0;
            let pts = match t.mid {
                Some(m) => geom::arc_points(t.start, m, t.end, TOL),
                None => vec![t.start, t.end],
            };
            let prims: Vec<Prim> = pts
                .windows(2)
                .filter_map(|w| match w {
                    [a, b] => Some(Prim::Capsule { a: *a, b: *b, r }),
                    _ => None,
                })
                .collect();
            if prims.is_empty() {
                continue;
            }
            items.push(Item {
                what: ItemRef::Track(i),
                layers: vec![l],
                net: net_of(&t.net),
                bbox: bbox_of(&prims),
                prims,
                anchors: vec![t.start, t.end],
            });
        }
        for (i, v) in board.vias.iter().enumerate() {
            let [la, lb] = &v.layers;
            let on: Vec<usize> = board.span(la, lb).iter().filter_map(|l| index(l)).collect();
            let prims = vec![Prim::Capsule { a: v.at, b: v.at, r: v.size / 2.0 }];
            items.push(Item {
                what: ItemRef::Via(i),
                layers: on,
                net: net_of(&v.net),
                bbox: bbox_of(&prims),
                prims,
                anchors: vec![v.at],
            });
        }
        Copper { layers, items }
    }
}

/// Distance from `p` to the segment `a`–`b`, and the nearest point on it.
pub fn point_segment(p: Pt, a: Pt, b: Pt) -> (f64, Pt) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + t * d[0], a[1] + t * d[1]];
    (libm::hypot(p[0] - q[0], p[1] - q[1]), q)
}

fn cross(o: Pt, a: Pt, b: Pt) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Distance between two segments and the nearest points (on the first, on the second).
pub fn segment_segment(a: Pt, b: Pt, c: Pt, d: Pt) -> (f64, Pt, Pt) {
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        // Proper crossing: the crossing point.
        let t = d3 / (d3 - d4);
        let x = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
        return (0.0, x, x);
    }
    let cands = [
        {
            let (dd, q) = point_segment(a, c, d);
            (dd, a, q)
        },
        {
            let (dd, q) = point_segment(b, c, d);
            (dd, b, q)
        },
        {
            let (dd, q) = point_segment(c, a, b);
            (dd, q, c)
        },
        {
            let (dd, q) = point_segment(d, a, b);
            (dd, q, d)
        },
    ];
    cands.into_iter().fold((f64::INFINITY, a, c), |best, x| if x.0 < best.0 { x } else { best })
}

fn edges(pts: &[Pt]) -> impl Iterator<Item = (Pt, Pt)> + '_ {
    let n = pts.len();
    (0..n).filter_map(move |i| Some((*pts.get(i)?, *pts.get((i + 1) % n)?)))
}

/// The gap between two pieces of copper (negative or zero when they touch or overlap) and
/// a point between them.
pub fn gap(p: &Prim, q: &Prim) -> (f64, Pt) {
    let mid = |u: Pt, v: Pt| [(u[0] + v[0]) / 2.0, (u[1] + v[1]) / 2.0];
    match (p, q) {
        (Prim::Capsule { a, b, r }, Prim::Capsule { a: c, b: d, r: s }) => {
            let (dist, u, v) = segment_segment(*a, *b, *c, *d);
            (dist - r - s, mid(u, v))
        }
        (Prim::Capsule { a, b, r }, Prim::Poly { pts }) | (Prim::Poly { pts }, Prim::Capsule { a, b, r }) => {
            if geom::inside(pts, *a) {
                return (-r, *a);
            }
            let mut best = (f64::INFINITY, *a);
            for (c, d) in edges(pts) {
                let (dist, u, v) = segment_segment(*a, *b, c, d);
                if dist < best.0 {
                    best = (dist, mid(u, v));
                }
            }
            (best.0 - r, best.1)
        }
        (Prim::Poly { pts: p1 }, Prim::Poly { pts: p2 }) => {
            if let Some(v) = p1.first().filter(|v| geom::inside(p2, **v)) {
                return (0.0, *v);
            }
            if let Some(v) = p2.first().filter(|v| geom::inside(p1, **v)) {
                return (0.0, *v);
            }
            let mut best = (f64::INFINITY, [0.0, 0.0]);
            for (a, b) in edges(p1) {
                for (c, d) in edges(p2) {
                    let (dist, u, v) = segment_segment(a, b, c, d);
                    if dist < best.0 {
                        best = (dist, mid(u, v));
                    }
                }
            }
            best
        }
    }
}

/// The smallest gap between two items' pieces.
pub fn item_gap(a: &Item, b: &Item) -> (f64, Pt) {
    let mut best = (f64::INFINITY, [0.0, 0.0]);
    for p in &a.prims {
        for q in &b.prims {
            let g = gap(p, q);
            if g.0 < best.0 {
                best = g;
            }
        }
    }
    best
}

/// Whether two bounding boxes come within `margin` of each other.
pub fn boxes_near(a: &[Pt; 2], b: &[Pt; 2], margin: f64) -> bool {
    a[0][0] - margin <= b[1][0]
        && b[0][0] - margin <= a[1][0]
        && a[0][1] - margin <= b[1][1]
        && b[0][1] - margin <= a[1][1]
}

/// Pairs of items whose boxes come within `margin` (a sweep over X), sharing a layer.
pub fn near_pairs(items: &[Item], margin: f64) -> Vec<(usize, usize)> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    let lo_x = |i: usize| items.get(i).map_or(0.0, |t| t.bbox[0][0]);
    order.sort_by(|a, b| lo_x(*a).total_cmp(&lo_x(*b)));
    let mut out = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        let Some(a) = items.get(i) else { continue };
        for &j in order.iter().skip(k + 1) {
            let Some(b) = items.get(j) else { continue };
            if b.bbox[0][0] > a.bbox[1][0] + margin {
                break;
            }
            if boxes_near(&a.bbox, &b.bbox, margin) && a.layers.iter().any(|l| b.layers.contains(l)) {
                out.push((i.min(j), i.max(j)));
            }
        }
    }
    out
}

/// A large polygon (a zone fill) with its edges bucketed on a grid, for touch tests that
/// do not walk every edge.
#[derive(Debug, Clone)]
pub struct EdgeGrid {
    lo: Pt,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    edges: Vec<(Pt, Pt)>,
    pub bbox: [Pt; 2],
}

impl EdgeGrid {
    pub fn new(poly: &[Pt]) -> EdgeGrid {
        let edges: Vec<(Pt, Pt)> = edges(poly).collect();
        let (lo, hi) = poly.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])
        });
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-3);
        let n = ((edges.len() as f64).sqrt().ceil() as usize).clamp(1, 256);
        let cell = span / n as f64;
        let nx = (((hi[0] - lo[0]) / cell).floor() as usize + 1).min(257);
        let ny = (((hi[1] - lo[1]) / cell).floor() as usize + 1).min(257);
        let mut g = EdgeGrid { lo, cell, nx, ny, cells: vec![Vec::new(); nx * ny], edges: Vec::new(), bbox: [lo, hi] };
        for (k, (a, b)) in edges.iter().enumerate() {
            let (x0, y0) = g.cell_of([a[0].min(b[0]), a[1].min(b[1])]);
            let (x1, y1) = g.cell_of([a[0].max(b[0]), a[1].max(b[1])]);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if let Some(c) = g.cells.get_mut(y * nx + x) {
                        c.push(k as u32);
                    }
                }
            }
        }
        g.edges = edges;
        g
    }

    fn cell_of(&self, p: Pt) -> (usize, usize) {
        let f = |v: f64, lo: f64, n: usize| (((v - lo) / self.cell).floor().max(0.0) as usize).min(n.saturating_sub(1));
        (f(p[0], self.lo[0], self.nx), f(p[1], self.lo[1], self.ny))
    }

    fn ids_in(&self, lo: Pt, hi: Pt) -> Vec<u32> {
        let (x0, y0) = self.cell_of(lo);
        let (x1, y1) = self.cell_of(hi);
        let mut ids = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some(c) = self.cells.get(y * self.nx + x) {
                    ids.extend_from_slice(c);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Whether `q` is inside (even-odd), casting a ray to +X through the grid row.
    pub fn inside(&self, q: Pt) -> bool {
        if q[0] < self.bbox[0][0] || q[0] > self.bbox[1][0] || q[1] < self.bbox[0][1] || q[1] > self.bbox[1][1] {
            return false;
        }
        let mut c = false;
        for k in self.ids_in(q, [self.bbox[1][0], q[1]]) {
            let Some((a, b)) = self.edges.get(k as usize) else { continue };
            if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
                c = !c;
            }
        }
        c
    }

    /// Whether a piece of copper touches (comes within `tol` of) the polygon.
    pub fn touches(&self, p: &Prim, tol: f64) -> bool {
        let bb = bbox_of(std::slice::from_ref(p));
        if !boxes_near(&bb, &self.bbox, tol) {
            return false;
        }
        let lo = [bb[0][0] - tol, bb[0][1] - tol];
        let hi = [bb[1][0] + tol, bb[1][1] + tol];
        for k in self.ids_in(lo, hi) {
            let Some((a, b)) = self.edges.get(k as usize) else { continue };
            if gap(p, &Prim::Capsule { a: *a, b: *b, r: 0.0 }).0 <= tol {
                return true;
            }
        }
        let q = match p {
            Prim::Capsule { a, .. } => *a,
            Prim::Poly { pts } => match pts.first() {
                Some(v) => *v,
                None => return false,
            },
        };
        self.inside(q)
    }
}
