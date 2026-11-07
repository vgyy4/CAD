//! Pencil finishing: a ball cutter traced along concave creases of the surface (ADR 0040,
//! "Pencil finishing" amendment).

use std::collections::{HashMap, HashSet};

use super::{Cutter, Dropper, Profile, Surface3d, mesh_bounds};
use crate::{CamError, MAX_MOVES, Move, Setup, Toolpath};

/// Most grid nodes of the cutter-location field.
const MAX_NODES: usize = 3_000_000;
/// Most refined crease points.
const MAX_POINTS: usize = 500_000;
/// Fine samples per grid step used to locate a crease.
const FINE: usize = 16;
/// Slope of the cutter-location field (rise per run) from which a step counts as a vertical
/// wall: tan 80 degrees.
const WALL_SLOPE: f64 = 5.67;
/// Shortest crease line kept, in grid steps.
const MIN_CHAIN_STEPS: f64 = 4.0;

/// Pencil finishing parameters (mm, degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PencilSpec {
    /// Smallest angle between the two surfaces the ball touches (the turning angle of the
    /// cutter-location profile across the crease) that gets a pencil pass.
    pub bitangency_angle_deg: f64,
    /// Chains of crease points are simplified within this distance.
    pub tolerance: f64,
    /// XY box `(min, max)` searched; `None` is the mesh's XY bounds.
    pub bounds: Option<([f64; 2], [f64; 2])>,
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    libm::sqrt((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1]) + (a[2] - b[2]) * (a[2] - b[2]))
}

/// Free boundary of the mesh (vertices welded at 0.1 micron): a ball resting on a boundary edge
/// is not held there, so such a contact does not support a crease. Neither is a ball on a sharp
/// (convex) edge or vertex, whose faces differ by more than the bitangency angle: it rolls over
/// the feature (a pocket's rim corner) rather than sitting in a valley (F10-4).
struct Topo {
    ids: Vec<[u32; 3]>,
    boundary_edges: HashSet<(u32, u32)>,
    boundary_vertices: HashSet<u32>,
    sharp_edges: HashSet<(u32, u32)>,
    sharp_vertices: HashSet<u32>,
}

impl Topo {
    fn new(s: &Surface3d, ang: f64) -> Self {
        let mut weld: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut vid: Vec<u32> = Vec::with_capacity(s.positions.len());
        for p in s.positions {
            let q = |v: f32| libm::round(f64::from(v) * 1e4) as i64;
            let n = weld.len() as u32;
            vid.push(*weld.entry((q(p[0]), q(p[1]), q(p[2]))).or_insert(n));
        }
        let mut ids = Vec::new();
        let mut count: HashMap<(u32, u32), u32> = HashMap::new();
        let mut normals: HashMap<(u32, u32), Vec<[f64; 3]>> = HashMap::new();
        for t in s.indices.chunks_exact(3) {
            let g = |k: usize| t.get(k).and_then(|&i| vid.get(i as usize)).copied().unwrap_or(0);
            let v = [g(0), g(1), g(2)];
            ids.push(v);
            let pt = |k: usize| {
                t.get(k)
                    .and_then(|&i| s.positions.get(i as usize))
                    .map_or([0.0; 3], |p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
            };
            let (e1, e2) = (sub(pt(1), pt(0)), sub(pt(2), pt(0)));
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let nl = libm::sqrt(dot(n, n));
            for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
                *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                if nl > 1e-12 {
                    normals.entry((a.min(b), a.max(b))).or_default().push([n[0] / nl, n[1] / nl, n[2] / nl]);
                }
            }
        }
        // Edges whose faces (as lines: orientation does not matter) differ by more than `ang`.
        let sharp_edges: HashSet<(u32, u32)> = normals
            .iter()
            .filter(|(_, ns)| ns.iter().any(|a| ns.iter().any(|b| libm::acos(dot(*a, *b).abs().clamp(0.0, 1.0)) > ang)))
            .map(|(k, _)| *k)
            .collect();
        let sharp_vertices = sharp_edges.iter().flat_map(|&(a, b)| [a, b]).collect();
        let boundary_edges: HashSet<(u32, u32)> = count.iter().filter(|(_, c)| **c == 1).map(|(k, _)| *k).collect();
        let boundary_vertices = boundary_edges.iter().flat_map(|&(a, b)| [a, b]).collect();
        Topo { ids, boundary_edges, boundary_vertices, sharp_edges, sharp_vertices }
    }
}

#[derive(Clone, Copy)]
enum Region {
    Face,
    Edge(usize),
    Vertex(usize),
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Closest point of triangle `t` to `p`, and where on the triangle it lies (edge `k` joins
/// vertex `k` to `k + 1`).
fn closest_on_tri(p: [f64; 3], t: &[[f64; 3]; 3]) -> ([f64; 3], Region) {
    let [a, b, c] = *t;
    let at = |o: [f64; 3], u: [f64; 3], k: f64| [o[0] + k * u[0], o[1] + k * u[1], o[2] + k * u[2]];
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, Region::Vertex(0));
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (b, Region::Vertex(1));
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return (at(a, ab, d1 / (d1 - d3)), Region::Edge(0));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (c, Region::Vertex(2));
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return (at(a, ac, d2 / (d2 - d6)), Region::Edge(2));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (at(b, sub(c, b), w), Region::Edge(1));
    }
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return (a, Region::Vertex(0));
    }
    let (v, w) = (vb / den, vc / den);
    (at(at(a, ab, v), ac, w), Region::Face)
}

/// Whether a ball of radius `r` with its centre at `c` rests on two surfaces whose normals
/// differ by at least `ang`: contacts on the mesh's free boundary do not count.
fn supported(d: &mut Dropper, topo: &Topo, c: [f64; 3], r: f64, ang: f64) -> bool {
    let tol = 0.005 * r + 1e-4;
    let mut us: Vec<([f64; 3], bool)> = Vec::new();
    for ti in d.near_tris(c[0], c[1], r + tol) {
        let (Some(t), Some(ids)) = (d.tris.get(ti as usize), topo.ids.get(ti as usize)) else { continue };
        let (q, region) = closest_on_tri(c, t);
        let u = sub(q, c);
        let dist = libm::sqrt(dot(u, u));
        if dist > r + tol || dist < 1e-12 {
            continue;
        }
        // (free boundary, sharp edge or vertex)
        let (free, sharp) = match region {
            Region::Face => (false, false),
            Region::Edge(k) => {
                let (a, b) = (ids.get(k).copied().unwrap_or(0), ids.get((k + 1) % 3).copied().unwrap_or(0));
                let key = (a.min(b), a.max(b));
                (topo.boundary_edges.contains(&key), topo.sharp_edges.contains(&key))
            }
            Region::Vertex(k) => {
                let v = ids.get(k).copied().unwrap_or(0);
                (topo.boundary_vertices.contains(&v), topo.sharp_vertices.contains(&v))
            }
        };
        if !free {
            us.push(([u[0] / dist, u[1] / dist, u[2] / dist], sharp));
        }
    }
    // Two contacts, at least one of them on a face (or a smooth edge): a ball that touches two
    // sharp edges only sits on a rim and rolls off.
    us.iter().any(|(a, sa)| us.iter().any(|(b, sb)| !(*sa && *sb) && libm::acos(dot(*a, *b).clamp(-1.0, 1.0)) >= ang))
}

/// A uniform XY bucket index over a point list.
struct Buckets {
    cell: f64,
    map: HashMap<(i64, i64), Vec<usize>>,
}

impl Buckets {
    fn new(cell: f64) -> Self {
        Buckets { cell, map: HashMap::new() }
    }
    fn key(&self, p: [f64; 3]) -> (i64, i64) {
        (libm::floor(p[0] / self.cell) as i64, libm::floor(p[1] / self.cell) as i64)
    }
    fn insert(&mut self, p: [f64; 3], idx: usize) {
        let k = self.key(p);
        self.map.entry(k).or_default().push(idx);
    }
    fn near(&self, p: [f64; 3]) -> impl Iterator<Item = usize> + '_ {
        let (kx, ky) = self.key(p);
        (-1..=1)
            .flat_map(move |dx| (-1..=1).map(move |dy| (kx + dx, ky + dy)))
            .flat_map(move |k| self.map.get(&k).into_iter().flat_map(|v| v.iter().copied()))
    }
}

/// Douglas-Peucker in 3D.
fn simplify3(pts: &[[f64; 3]], tol: f64) -> Vec<[f64; 3]> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    if let Some(k) = keep.first_mut() {
        *k = true;
    }
    if let Some(k) = keep.last_mut() {
        *k = true;
    }
    let mut stack = vec![(0usize, n - 1)];
    while let Some((i, j)) = stack.pop() {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get(j)) else { continue };
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let mut worst = (0.0, None);
        for k in i + 1..j {
            let Some(&p) = pts.get(k) else { continue };
            let w = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
            let t = if dd < 1e-24 { 0.0 } else { ((w[0] * d[0] + w[1] * d[1] + w[2] * d[2]) / dd).clamp(0.0, 1.0) };
            let q = [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]];
            let dev = dist3(p, q);
            if dev > worst.0 {
                worst = (dev, Some(k));
            }
        }
        if let (true, Some(k)) = (worst.0 > tol, worst.1) {
            if let Some(f) = keep.get_mut(k) {
                *f = true;
            }
            stack.push((i, k));
            stack.push((k, j));
        }
    }
    pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

/// Crease points accepted so far, and what decides on the next one.
struct Accepted {
    pts: Vec<[f64; 3]>,
    dedup: Buckets,
    h: f64,
    r: f64,
    ang: f64,
    bmin: [f64; 2],
    bmax: [f64; 2],
    bottom: f64,
    /// Crease points found below the stock bottom (and so dropped).
    below: usize,
    /// Highest of them.
    below_top: f64,
}

impl Accepted {
    /// Adds the tool tip position `p` if it is in the box, above the stock bottom, held by two
    /// surfaces (not by the mesh's free boundary) and not within half a grid step of another.
    fn add(&mut self, d: &mut Dropper, topo: &Topo, p: [f64; 3]) -> Result<(), CamError> {
        let [x, y, z] = p;
        if x < self.bmin[0] || x > self.bmax[0] || y < self.bmin[1] || y > self.bmax[1] {
            return Ok(());
        }
        if z < self.bottom {
            self.below += 1;
            self.below_top = self.below_top.max(z);
            return Ok(());
        }
        if self.dedup.near(p).any(|q| self.pts.get(q).is_some_and(|o| libm::hypot(o[0] - x, o[1] - y) < self.h / 2.0)) {
            return Ok(());
        }
        if !supported(d, topo, [x, y, z + self.r], self.r, self.ang) {
            return Ok(());
        }
        self.dedup.insert(p, self.pts.len());
        self.pts.push(p);
        if self.pts.len() > MAX_POINTS {
            return Err(CamError::TooLarge("too many crease points; raise the bitangency angle".into()));
        }
        Ok(())
    }
}

fn no_crease_below_depth(bottom: f64, highest: f64) -> String {
    let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
    format!(
        "the creases lie below the depth: the stock bottom is at z {} and no crease is higher than z {}; \
         increase the depth",
        r3(bottom),
        r3(highest)
    )
}

/// Pencil finishing with a ball cutter: traces the concave creases where the ball touches two
/// surfaces at once, as cutting moves at the exact cutter-location height.
///
/// 1. The cutter-location (CL) field, the drop-cutter height on a grid of `r / 8` (`r` the tool
///    radius), is scanned in four directions for nodes where the CL profile turns upward (a
///    valley) by at least `bitangency_angle_deg` between the chords to the neighbours. Smooth
///    valleys (a fillet larger than the ball), ridges and flat areas do not turn sharply enough.
/// 2. Around each such node the profile is sampled at 1/16 of the grid step; the straight pieces
///    on either side of the kink are intersected, which places the crease to rounding for
///    planar facets, and the CL height there is a fresh drop-cutter.
/// 3. Points are merged (within half a grid step), chained by nearest neighbour (links up to
///    three grid steps, in 3D), single points dropped, chains simplified within `tolerance` and
///    emitted as `Line` moves, linked at the safe height.
///
/// A step in the crease direction smaller than the grid step is not resolved; creases close to
/// the mesh boundary (less than one grid step) and nodes with no surface within the tool radius
/// are skipped. Only `Cutter::Ball` is supported. No crease found is `NothingToCut`; more than
/// 3 million grid nodes or 500 000 crease points is `TooLarge`.
pub fn pencil_finish(surface: &Surface3d, cutter: Cutter, spec: &PencilSpec, s: &Setup) -> Result<Toolpath, CamError> {
    let fin = |v: f64| v.is_finite();
    if cutter != Cutter::Ball {
        return Err(CamError::Invalid("pencil finishing needs a ball cutter".into()));
    }
    if !(fin(s.tool_diameter) && s.tool_diameter > 0.0) {
        return Err(CamError::Invalid("tool diameter must be positive".into()));
    }
    if !(fin(s.depth) && s.depth > 0.0 && fin(s.top) && fin(s.safe)) {
        return Err(CamError::Invalid("depth must be positive; top and safe finite".into()));
    }
    if !(fin(spec.bitangency_angle_deg) && spec.bitangency_angle_deg > 0.0 && spec.bitangency_angle_deg < 180.0) {
        return Err(CamError::Invalid("bitangency angle must be between 0 and 180 degrees".into()));
    }
    if !(fin(spec.tolerance) && spec.tolerance >= 0.0) {
        return Err(CamError::Invalid("tolerance must be finite and not negative".into()));
    }
    let r = s.tool_diameter / 2.0;
    let (mlo, mhi) =
        mesh_bounds(surface.positions).ok_or_else(|| CamError::Invalid("the surface has no valid vertices".into()))?;
    let (bmin, bmax) = spec.bounds.unwrap_or((mlo, mhi));
    if !(bmin.iter().chain(&bmax).all(|v| fin(*v)) && bmax[0] > bmin[0] && bmax[1] > bmin[1]) {
        return Err(CamError::Invalid("empty or invalid bounds".into()));
    }
    let mut dropper = Dropper::new(surface, Profile { r, c: r }, mlo, mhi)?;

    let h = r / 8.0;
    let nxf = libm::floor((bmax[0] - bmin[0]) / h) + 1.0;
    let nyf = libm::floor((bmax[1] - bmin[1]) / h) + 1.0;
    if nxf * nyf > MAX_NODES as f64 {
        return Err(CamError::TooLarge("too many grid nodes; use a larger tool or smaller bounds".into()));
    }
    let (nx, ny) = (nxf as usize, nyf as usize);
    let node = |i: usize, j: usize| [bmin[0] + h * i as f64, bmin[1] + h * j as f64];
    let mut field = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            let [x, y] = node(i, j);
            field.push(dropper.drop(x, y).unwrap_or(f64::NAN));
        }
    }
    let at = |i: usize, j: usize| field.get(j * nx + i).copied().unwrap_or(f64::NAN);
    let ang = spec.bitangency_angle_deg.to_radians();
    let bottom = s.top - s.depth;
    // The tool tip never rises above the highest vertex: a stock bottom above it leaves no
    // crease to find (checked before the long scan).
    let zmax = surface.positions.iter().map(|p| f64::from(p[2])).fold(f64::NEG_INFINITY, f64::max);
    if bottom > zmax + 1e-9 {
        return Err(CamError::NothingToCut(no_crease_below_depth(bottom, zmax)));
    }

    let topo = Topo::new(surface, ang);
    let mut acc = Accepted {
        pts: Vec::new(),
        dedup: Buckets::new(h),
        h,
        r,
        ang,
        bmin,
        bmax,
        bottom,
        below: 0,
        below_top: f64::NEG_INFINITY,
    };
    for j in 1..ny.saturating_sub(1) {
        for i in 1..nx.saturating_sub(1) {
            let f0 = at(i, j);
            // Best direction: the largest upward turn of the CL profile.
            let mut best: Option<(f64, [f64; 2])> = None;
            for (di, dj) in [(1i64, 0i64), (0, 1), (1, 1), (1, -1)] {
                let (ip, jp) = ((i as i64 + di) as usize, (j as i64 + dj) as usize);
                let (im, jm) = ((i as i64 - di) as usize, (j as i64 - dj) as usize);
                let (fp, fm) = (at(ip, jp), at(im, jm));
                if !(f0.is_finite() && fp.is_finite() && fm.is_finite()) {
                    continue;
                }
                let sp = h * libm::hypot(di as f64, dj as f64);
                let turn = libm::atan((fp - f0) / sp) - libm::atan((f0 - fm) / sp);
                if turn >= ang && best.is_none_or(|(t, _)| turn > t) {
                    let l = libm::hypot(di as f64, dj as f64);
                    best = Some((turn, [di as f64 / l, dj as f64 / l]));
                }
            }
            // A vertical (or nearly vertical) wall rising from this node: the cutter-location
            // field steps up by more than a grid step times `WALL_SLOPE`. The ball touches the
            // wall and the floor where the field leaves the floor's height.
            let mut wall = false;
            for (di, dj) in [(1i64, 0i64), (0, 1), (-1, 0), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
                let (ni, nj) = ((i as i64 + di) as usize, (j as i64 + dj) as usize);
                let fnb = at(ni, nj);
                let sp = h * libm::hypot(di as f64, dj as f64);
                if !(f0.is_finite() && fnb.is_finite() && (fnb - f0) / sp >= WALL_SLOPE) {
                    continue;
                }
                wall = true;
                let ([xa, ya], [xb, yb]) = (node(i, j), node(ni, nj));
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..30 {
                    let m = 0.5 * (lo + hi);
                    match dropper.drop(xa + m * (xb - xa), ya + m * (yb - ya)) {
                        Some(z) if z <= f0 + 1e-4 => lo = m,
                        _ => hi = m,
                    }
                }
                let (x, y) = (xa + lo * (xb - xa), ya + lo * (yb - ya));
                if let Some(z) = dropper.drop(x, y) {
                    acc.add(&mut dropper, &topo, [x, y, z])?;
                }
            }
            if wall {
                continue;
            }
            let Some((_, e)) = best else { continue };
            let [cx, cy] = node(i, j);
            // Fine profile along `e` over -2h ..= 2h.
            let dt = h / FINE as f64;
            let m = 4 * FINE;
            let mut prof = Vec::with_capacity(m + 1);
            for k in 0..=m {
                let t = (k as f64 - (m / 2) as f64) * dt;
                match dropper.drop(cx + e[0] * t, cy + e[1] * t) {
                    Some(z) => prof.push(z),
                    None => break,
                }
            }
            if prof.len() != m + 1 {
                continue;
            }
            let g = |k: usize| prof.get(k).copied().unwrap_or(f64::NAN);
            // The kink: largest second difference within one grid step of the node.
            let mut kink: Option<(f64, usize)> = None;
            for k in m / 2 - FINE..=m / 2 + FINE {
                let d2 = g(k - 1) - 2.0 * g(k) + g(k + 1);
                if d2 > 1e-9 && kink.is_none_or(|(b, _)| d2 > b) {
                    kink = Some((d2, k));
                }
            }
            let Some((_, k)) = kink else { continue };
            let tk = |k: usize| (k as f64 - (m / 2) as f64) * dt;
            let sl = (g(k - 2) - g(k - 3)) / dt;
            let sr = (g(k + 3) - g(k + 2)) / dt;
            if libm::atan(sr) - libm::atan(sl) < ang {
                continue;
            }
            let t = ((g(k + 2) - sr * tk(k + 2)) - (g(k - 2) - sl * tk(k - 2))) / (sl - sr);
            if !(t.is_finite() && t >= tk(k - 1) - dt && t <= tk(k + 1) + dt) {
                continue;
            }
            let (x, y) = (cx + e[0] * t, cy + e[1] * t);
            let Some(z) = dropper.drop(x, y) else { continue };
            acc.add(&mut dropper, &topo, [x, y, z])?;
        }
    }
    let pts = acc.pts;
    if pts.is_empty() && acc.below > 0 {
        return Err(CamError::NothingToCut(no_crease_below_depth(bottom, acc.below_top)));
    }
    if pts.is_empty() {
        return Err(CamError::NothingToCut("no concave crease sharper than the bitangency angle".into()));
    }

    // Chain by nearest neighbour.
    let link = 3.0 * h;
    let mut idx = Buckets::new(link);
    for (n, p) in pts.iter().enumerate() {
        idx.insert(*p, n);
    }
    let mut used = vec![false; pts.len()];
    let mut chains: Vec<Vec<[f64; 3]>> = Vec::new();
    for start in 0..pts.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        let Some(&p0) = pts.get(start) else { continue };
        let grow = |from: [f64; 3], used: &mut [bool]| -> Vec<[f64; 3]> {
            let mut out = Vec::new();
            let mut cur = from;
            loop {
                let next = idx
                    .near(cur)
                    .filter(|&q| !used.get(q).copied().unwrap_or(true))
                    .filter_map(|q| pts.get(q).map(|p| (q, *p, dist3(cur, *p))))
                    .filter(|(_, _, d)| *d <= link)
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                let Some((q, p, _)) = next else { break };
                if let Some(u) = used.get_mut(q) {
                    *u = true;
                }
                out.push(p);
                cur = p;
            }
            out
        };
        let fwd = grow(p0, &mut used);
        let mut back = grow(p0, &mut used);
        back.reverse();
        let mut chain = back;
        chain.push(p0);
        chain.extend(fwd);
        let length: f64 = chain.iter().zip(chain.iter().skip(1)).map(|(a, b)| dist3(*a, *b)).sum();
        if chain.len() >= 2 && length >= MIN_CHAIN_STEPS * h {
            // A ring (a boss on a floor): close it.
            if let (Some(&f), Some(&l)) = (chain.first(), chain.last())
                && chain.len() >= 4
                && dist3(f, l) <= link
            {
                chain.push(f);
            }
            chains.push(simplify3(&chain, spec.tolerance));
        }
    }
    if chains.is_empty() {
        return Err(CamError::NothingToCut("only isolated crease points found".into()));
    }

    let mut moves: Vec<Move> = Vec::new();
    let mut max_z = f64::NEG_INFINITY;
    for chain in &chains {
        let Some(&first) = chain.first() else { continue };
        if let Some(Move::Line { to, .. }) = moves.last() {
            moves.push(Move::Rapid([to[0], to[1], s.safe]));
        }
        moves.push(Move::Rapid([first[0], first[1], s.safe]));
        moves.push(Move::Rapid([first[0], first[1], s.safe.min(first[2] + 1.0)]));
        moves.push(Move::Line { to: first, plunge: true });
        for p in chain.iter().skip(1) {
            moves.push(Move::Line { to: *p, plunge: false });
        }
        max_z = chain.iter().map(|p| p[2]).fold(max_z, f64::max);
        if moves.len() > MAX_MOVES {
            return Err(CamError::TooLarge("too many moves".into()));
        }
    }
    if max_z > s.safe {
        return Err(CamError::Invalid("the safe height is below the surface".into()));
    }
    Ok(Toolpath { moves, rings: chains.len(), levels: vec![bottom], drill: None })
}

#[cfg(test)]
mod tests;
