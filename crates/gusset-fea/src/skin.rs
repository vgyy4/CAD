//! A shell model of a thin body from its surface mesh (sheet metal, ADR 0041 amendment):
//! one side of the sheet (the faces reached from the largest one across smooth edges: its
//! flats and bends, not the thickness strips or hole walls) moved in by half the thickness,
//! refined to an element size by splitting longest edges first, and each triangle cut into
//! three quads for the flat-shell elements.

use crate::FeaError;
use crate::shell::ShellMesh;
use gusset_kernel::Mesh;
use std::collections::{BinaryHeap, HashMap};

/// Most triangles the refinement makes (three quads each).
pub const MAX_SKIN_TRIANGLES: usize = 40_000;

/// Cosine of the largest angle between facets across an edge that still counts as smooth
/// (37 degrees: a coarsely facetted small bend still joins, while thickness strips and hole
/// walls meet the sheet at 90).
const SMOOTH: f64 = 0.8;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    libm::sqrt(dot(a, a))
}
fn unit(a: V3) -> V3 {
    let l = norm(a);
    if l > 1e-300 { scale(a, 1.0 / l) } else { a }
}

fn wide(p: [f32; 3]) -> V3 {
    p.map(f64::from)
}

/// The mid-surface of a sheet: the shell mesh, each node's unit normal (towards the skin
/// side) and, per input mesh face, whether it is on the skin.
#[derive(Debug, Clone)]
pub struct MidSurface {
    pub mesh: ShellMesh,
    pub normals: Vec<[f64; 3]>,
    pub skin: Vec<bool>,
    pub thickness: f64,
}

/// The mid-surface shell mesh of a sheet `thickness` thick, with elements about `h` across.
///
/// # Errors
/// `Invalid` for an empty mesh, a bad size or thickness; `TooLarge` past
/// [`MAX_SKIN_TRIANGLES`].
pub fn mid_surface(mesh: &Mesh, thickness: f64, h: f64) -> Result<MidSurface, FeaError> {
    if !(thickness.is_finite() && thickness > 0.0 && h.is_finite() && h > 0.0) {
        return Err(FeaError::Invalid("the thickness and element size must be positive".into()));
    }
    // Weld vertices across faces (each face has its own copies): points within 1e-4 mm are
    // one, found in their grid cell or a neighbouring one (no split at a cell border).
    const WELD: f64 = 1e-4;
    let q = |p: V3| p.map(|c| libm::floor(c / (4.0 * WELD)) as i64);
    let mut cells: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut pos: Vec<V3> = Vec::new();
    let mut weld: Vec<u32> = Vec::with_capacity(mesh.positions.len());
    for p in &mesh.positions {
        let p = wide(*p);
        let c = q(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &i in cells.get(&[c[0] + dx, c[1] + dy, c[2] + dz]).into_iter().flatten() {
                        if pos.get(i as usize).is_some_and(|o| norm(sub(*o, p)) <= WELD) {
                            found = Some(i);
                            break 'search;
                        }
                    }
                }
            }
        }
        let id = found.unwrap_or_else(|| {
            pos.push(p);
            let i = (pos.len() - 1) as u32;
            cells.entry(c).or_default().push(i);
            i
        });
        weld.push(id);
    }
    // Triangles per face, with outward normals (wound as the mesh, signed by its normals).
    struct Tri {
        v: [u32; 3],
        face: usize,
        n: V3,
        area: f64,
        /// The mesh's own (exact surface) normals at the corners.
        vn: [V3; 3],
    }
    let mut tris: Vec<Tri> = Vec::new();
    for (face, r) in mesh.face_ranges.iter().enumerate() {
        for t in r.start..r.end {
            let k = t as usize * 3;
            let Some(&[a, b, c]) = mesh.indices.get(k..k + 3).and_then(|s| <&[u32; 3]>::try_from(s).ok()) else {
                continue;
            };
            let at = |i: u32| mesh.positions.get(i as usize).map(|p| wide(*p));
            let (Some(pa), Some(pb), Some(pc)) = (at(a), at(b), at(c)) else { continue };
            let nn = cross(sub(pb, pa), sub(pc, pa));
            let area = norm(nn) / 2.0;
            if area < 1e-12 {
                continue;
            }
            let mut n = unit(nn);
            let given: V3 =
                [a, b, c].iter().filter_map(|i| mesh.normals.get(*i as usize)).fold([0.0; 3], |s, m| add(s, wide(*m)));
            if dot(given, n) < 0.0 {
                n = scale(n, -1.0);
            }
            let w = |i: u32| weld.get(i as usize).copied().unwrap_or(0);
            let vn = [a, b, c].map(|i| match mesh.normals.get(i as usize).map(|m| unit(wide(*m))) {
                Some(m) if norm(m) > 0.5 => m,
                _ => n,
            });
            tris.push(Tri { v: [w(a), w(b), w(c)], face, n, area, vn });
        }
    }
    if tris.is_empty() {
        return Err(FeaError::Invalid("the body has no surface mesh".into()));
    }
    // Faces joined across smooth edges, flooded from the largest face.
    let faces = mesh.face_ranges.len();
    let mut area = vec![0.0; faces];
    let mut edges: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        if let Some(a) = area.get_mut(t.face) {
            *a += t.area;
        }
        for (a, b) in [(t.v[0], t.v[1]), (t.v[1], t.v[2]), (t.v[2], t.v[0])] {
            edges.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); faces];
    for list in edges.values() {
        for (i, a) in list.iter().enumerate() {
            for b in list.iter().skip(i + 1) {
                let (Some(ta), Some(tb)) = (tris.get(*a), tris.get(*b)) else { continue };
                if ta.face != tb.face && dot(ta.n, tb.n) > SMOOTH {
                    if let Some(l) = next.get_mut(ta.face) {
                        l.push(tb.face);
                    }
                    if let Some(l) = next.get_mut(tb.face) {
                        l.push(ta.face);
                    }
                }
            }
        }
    }
    let start = area.iter().enumerate().fold((0, 0.0), |best, (i, a)| if *a > best.1 { (i, *a) } else { best }).0;
    let mut skin = vec![false; faces];
    let mut stack = vec![start];
    while let Some(f) = stack.pop() {
        if skin.get(f).copied().unwrap_or(true) {
            continue;
        }
        if let Some(s) = skin.get_mut(f) {
            *s = true;
        }
        stack.extend(next.get(f).into_iter().flatten().copied());
    }
    // The skin's triangles and per-vertex outward normals (area weighted).
    let mut normal = vec![[0.0; 3]; pos.len()];
    let mut tri: Vec<Option<[u32; 3]>> = Vec::new();
    // The surface's exact normals (OCCT's vertex normals): on a bend every node then moves
    // along its own radius and no thin facet can fold (dogfooding F8-1).
    for t in tris.iter().filter(|t| skin.get(t.face).copied().unwrap_or(false)) {
        for (v, vn) in t.v.iter().zip(t.vn) {
            if let Some(n) = normal.get_mut(*v as usize) {
                *n = add(*n, vn);
            }
        }
        tri.push(Some(t.v));
    }
    if tri.len() > MAX_SKIN_TRIANGLES {
        return Err(FeaError::TooLarge(format!(
            "the body's surface mesh has more than {MAX_SKIN_TRIANGLES} triangles on one side of the sheet"
        )));
    }
    // Refine: split the longest edge (with both its triangles) until every edge is at most h.
    let p = |pos: &[V3], i: u32| pos.get(i as usize).copied().unwrap_or([0.0; 3]);
    let elen = |pos: &[V3], a: u32, b: u32| norm(sub(p(pos, a), p(pos, b)));
    let mut on_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    let mut heap: BinaryHeap<(u64, u32, u32)> = BinaryHeap::new();
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    for (i, t) in tri.iter().enumerate() {
        let Some(v) = t else { continue };
        for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
            let k = key(a, b);
            let list = on_edge.entry(k).or_default();
            if list.is_empty() {
                heap.push((elen(&pos, a, b).to_bits(), k.0, k.1));
            }
            list.push(i);
        }
    }
    while let Some((bits, a, b)) = heap.pop() {
        if f64::from_bits(bits) <= h {
            break;
        }
        let Some(list) = on_edge.remove(&(a, b)) else { continue };
        if tri.len() + list.len() > MAX_SKIN_TRIANGLES {
            return Err(FeaError::TooLarge(format!(
                "the shell model needs more than {MAX_SKIN_TRIANGLES} triangles: give a larger element size"
            )));
        }
        let m = add(p(&pos, a), scale(sub(p(&pos, b), p(&pos, a)), 0.5));
        let mn = add(
            normal.get(a as usize).copied().unwrap_or([0.0; 3]),
            normal.get(b as usize).copied().unwrap_or([0.0; 3]),
        );
        pos.push(m);
        normal.push(mn);
        let mid = (pos.len() - 1) as u32;
        for ti in list {
            let Some(Some(v)) = tri.get(ti).copied() else { continue };
            // The vertex opposite the edge, keeping the winding.
            let turns = [(v[0], v[1], v[2]), (v[1], v[2], v[0]), (v[2], v[0], v[1])];
            let Some((x, y, o)) = turns.into_iter().find(|(x, y, _)| key(*x, *y) == (a, b)) else {
                continue;
            };
            let (t1, t2) = ([x, mid, o], [mid, y, o]);
            if let Some(slot) = tri.get_mut(ti) {
                *slot = Some(t1);
            }
            tri.push(Some(t2));
            let n2 = tri.len() - 1;
            // Re-file the edges: the opposite edges move to the new triangles.
            if let Some(l) = on_edge.get_mut(&key(y, o)) {
                for e in l.iter_mut().filter(|e| **e == ti) {
                    *e = n2;
                }
            }
            for ((u, w), owner) in [((x, mid), ti), ((mid, y), n2), ((mid, o), ti), ((mid, o), n2)] {
                let k = key(u, w);
                let l = on_edge.entry(k).or_default();
                if l.is_empty() {
                    heap.push((elen(&pos, u, w).to_bits(), k.0, k.1));
                }
                l.push(owner);
            }
        }
    }
    // Slivers (OCCT leaves very thin triangles where a bend meets a flat) would make folded
    // quads: collapse edges shorter than a fifth of the size, keeping the outline's vertices.
    collapse_short(&mut tri, &pos, h / 5.0);
    // Flat "caps" (a vertex on the opposite edge) give quads with a node twice: flip their
    // long edge with the neighbour across it; a cap on the outline goes.
    flip_caps(&mut tri, &pos, &normal);
    // Each triangle as three quads: corner, edge midpoint, centre, edge midpoint.
    let mut nodes = pos.clone();
    let mut nrm = normal.clone();
    let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
    let mut quads: Vec<[u32; 4]> = Vec::new();
    let mut midpoint = |nodes: &mut Vec<V3>, nrm: &mut Vec<V3>, a: u32, b: u32| -> u32 {
        *mids.entry(key(a, b)).or_insert_with(|| {
            let (pa, pb) = (p(nodes, a), p(nodes, b));
            nodes.push(scale(add(pa, pb), 0.5));
            let n =
                add(nrm.get(a as usize).copied().unwrap_or([0.0; 3]), nrm.get(b as usize).copied().unwrap_or([0.0; 3]));
            nrm.push(n);
            (nodes.len() - 1) as u32
        })
    };
    for v in tri.iter().flatten() {
        let [a, b, c] = *v;
        let (mab, mbc, mca) = (
            midpoint(&mut nodes, &mut nrm, a, b),
            midpoint(&mut nodes, &mut nrm, b, c),
            midpoint(&mut nodes, &mut nrm, c, a),
        );
        let g = scale(add(add(p(&nodes, a), p(&nodes, b)), p(&nodes, c)), 1.0 / 3.0);
        let gn = add(
            add(nrm.get(a as usize).copied().unwrap_or([0.0; 3]), nrm.get(b as usize).copied().unwrap_or([0.0; 3])),
            nrm.get(c as usize).copied().unwrap_or([0.0; 3]),
        );
        nodes.push(g);
        nrm.push(gn);
        let gi = (nodes.len() - 1) as u32;
        quads.push([a, mab, gi, mca]);
        quads.push([b, mbc, gi, mab]);
        quads.push([c, mca, gi, mbc]);
    }
    // Every node moved in by half the thickness; a quad that comes out folded or flat there
    // (rare, where facets are thin) is left out rather than fail the whole model.
    let unit_n: Vec<V3> = nrm.iter().map(|n| unit(*n)).collect();
    let moved: Vec<V3> = nodes.iter().zip(&unit_n).map(|(x, n)| sub(*x, scale(*n, thickness / 2.0))).collect();
    quads.retain(|q| convex(q.map(|i| p(&moved, i))));
    // Keep the nodes the quads use.
    let mut used = vec![u32::MAX; nodes.len()];
    let (mut out, mut normals) = (Vec::new(), Vec::new());
    for q in &mut quads {
        for v in q.iter_mut() {
            let Some(slot) = used.get_mut(*v as usize) else { continue };
            if *slot == u32::MAX {
                out.push(p(&moved, *v));
                normals.push(unit_n.get(*v as usize).copied().unwrap_or([0.0; 3]));
                *slot = (out.len() - 1) as u32;
            }
            *v = *slot;
        }
    }
    Ok(MidSurface { mesh: ShellMesh { nodes: out, quads, tris: Vec::new() }, normals, skin, thickness })
}

/// Merges the ends of edges shorter than `min` (shortest first), keeping a vertex on the
/// mesh's outline where there is one, and drops the triangles that collapse.
fn collapse_short(tri: &mut [Option<[u32; 3]>], pos: &[V3], min: f64) {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
    for v in tri.iter().flatten() {
        for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
            *uses.entry(key(a, b)).or_insert(0) += 1;
        }
    }
    let mut outline = vec![false; pos.len()];
    for ((a, b), n) in &uses {
        if *n == 1 {
            for v in [a, b] {
                if let Some(o) = outline.get_mut(*v as usize) {
                    *o = true;
                }
            }
        }
    }
    let at = |i: u32| pos.get(i as usize).copied().unwrap_or([0.0; 3]);
    let mut short: Vec<(f64, u32, u32)> =
        uses.keys().map(|(a, b)| (norm(sub(at(*a), at(*b))), *a, *b)).filter(|(l, _, _)| *l < min).collect();
    short.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut parent: Vec<u32> = (0..pos.len() as u32).collect();
    fn find(parent: &mut [u32], mut i: u32) -> u32 {
        while let Some(&p) = parent.get(i as usize) {
            if p == i {
                break;
            }
            let gp = parent.get(p as usize).copied().unwrap_or(p);
            if let Some(slot) = parent.get_mut(i as usize) {
                *slot = gp;
            }
            i = p;
        }
        i
    }
    for (_, a, b) in short {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra == rb || norm(sub(at(ra), at(rb))) >= min {
            continue;
        }
        let keep_b =
            outline.get(rb as usize).copied().unwrap_or(false) && !outline.get(ra as usize).copied().unwrap_or(false);
        let (from, to) = if keep_b { (ra, rb) } else { (rb, ra) };
        if let Some(slot) = parent.get_mut(from as usize) {
            *slot = to;
        }
    }
    for t in tri.iter_mut() {
        if let Some(v) = t {
            let w = v.map(|i| find(&mut parent, i));
            *t = (w[0] != w[1] && w[1] != w[2] && w[0] != w[2]).then_some(w);
        }
    }
}

/// Whether a quad is convex and not flat, seen along its own normal.
fn convex(p: [V3; 4]) -> bool {
    let n = cross(sub(p[2], p[0]), sub(p[3], p[1]));
    let nn = norm(n);
    if nn < 1e-12 {
        return false;
    }
    let corners = [(p[0], p[1], p[3]), (p[1], p[2], p[0]), (p[2], p[3], p[1]), (p[3], p[0], p[2])];
    corners.iter().all(|(c, next, prev)| dot(cross(sub(*next, *c), sub(*prev, *c)), n) > 1e-6 * nn * nn)
}

/// Quality of a triangle: 1 equilateral, 0 flat (4 sqrt3 area / sum of squared edges).
fn quality(a: V3, b: V3, c: V3) -> f64 {
    let area = norm(cross(sub(b, a), sub(c, a))) / 2.0;
    let s = dot(sub(b, a), sub(b, a)) + dot(sub(c, b), sub(c, b)) + dot(sub(a, c), sub(a, c));
    if s > 0.0 { 4.0 * libm::sqrt(3.0) * area / s } else { 0.0 }
}

/// Flips the longest edge of each nearly flat triangle with its neighbour when that makes
/// both better, and drops flat triangles that cannot be flipped (on the outline).
fn flip_caps(tri: &mut [Option<[u32; 3]>], pos: &[V3], normal: &[V3]) {
    const FLAT: f64 = 0.02;
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let at = |i: u32| pos.get(i as usize).copied().unwrap_or([0.0; 3]);
    let nrm = |i: u32| normal.get(i as usize).copied().unwrap_or([0.0; 3]);
    for _ in 0..4 {
        let mut on_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (i, v) in tri.iter().enumerate() {
            if let Some(v) = v {
                for (a, b) in [(v[0], v[1]), (v[1], v[2]), (v[2], v[0])] {
                    on_edge.entry(key(a, b)).or_default().push(i);
                }
            }
        }
        let mut changed = false;
        for i in 0..tri.len() {
            let Some(Some(v)) = tri.get(i).copied() else { continue };
            if quality(at(v[0]), at(v[1]), at(v[2])) >= FLAT {
                continue;
            }
            // The longest edge (u -> w in winding order) and the vertex opposite.
            let turns = [(v[0], v[1], v[2]), (v[1], v[2], v[0]), (v[2], v[0], v[1])];
            let Some((u, w, o)) =
                turns.into_iter().max_by(|x, y| norm(sub(at(x.0), at(x.1))).total_cmp(&norm(sub(at(y.0), at(y.1)))))
            else {
                continue;
            };
            let other = on_edge.get(&key(u, w)).and_then(|l| l.iter().copied().find(|j| *j != i));
            let Some(j) = other else {
                // On the outline: nothing to flip with, and it holds no area.
                if let Some(t) = tri.get_mut(i) {
                    *t = None;
                }
                changed = true;
                continue;
            };
            let Some(Some(vj)) = tri.get(j).copied() else { continue };
            let Some(p) = vj.into_iter().find(|x| *x != u && *x != w) else { continue };
            let (t1, t2) = ([o, u, p], [p, w, o]);
            // Both new triangles must face the way the surface does and be better.
            let faces = |t: [u32; 3]| {
                let n = cross(sub(at(t[1]), at(t[0])), sub(at(t[2]), at(t[0])));
                let m = add(add(nrm(t[0]), nrm(t[1])), nrm(t[2]));
                dot(n, m) > 0.0
            };
            let q = |t: [u32; 3]| quality(at(t[0]), at(t[1]), at(t[2]));
            if faces(t1) && faces(t2) && q(t1).min(q(t2)) > q(v).min(q(vj)) {
                if let Some(t) = tri.get_mut(i) {
                    *t = Some(t1);
                }
                if let Some(t) = tri.get_mut(j) {
                    *t = Some(t2);
                }
                changed = true;
                // The edge map is stale for these two: finish this round.
                break;
            }
        }
        if !changed {
            break;
        }
    }
    // Whatever stays flat holds no area worth an element.
    for t in tri.iter_mut() {
        if let Some(v) = t
            && quality(at(v[0]), at(v[1]), at(v[2])) < 1e-6
        {
            *t = None;
        }
    }
}

/// The shell nodes on a face of the surface mesh: the node itself (on a thickness strip)
/// or its point on either surface of the sheet within `tol` of the face's triangles.
pub fn nodes_on_face(mesh: &Mesh, face: usize, mid: &MidSurface, tol: f64) -> Vec<u32> {
    let half = mid.thickness / 2.0;
    let Some(r) = mesh.face_ranges.get(face) else { return Vec::new() };
    let tris: Vec<[V3; 3]> = (r.start..r.end)
        .filter_map(|t| {
            let k = t as usize * 3;
            let v = mesh.indices.get(k..k + 3)?;
            let at = |i: &u32| mesh.positions.get(*i as usize).map(|p| wide(*p));
            Some([at(v.first()?)?, at(v.get(1)?)?, at(v.get(2)?)?])
        })
        .collect();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let reach = tol + half;
    for p in tris.iter().flatten() {
        for ((l, h), c) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(*c - reach);
            *h = h.max(*c + reach);
        }
    }
    mid.mesh
        .nodes
        .iter()
        .zip(&mid.normals)
        .enumerate()
        .filter(|(_, (p, _))| p.iter().zip(&lo).zip(&hi).all(|((c, l), h)| c >= l && c <= h))
        .filter(|(_, (p, n))| {
            [0.0, half, -half].iter().any(|o| {
                let q = add(**p, scale(**n, *o));
                tris.iter().any(|t| point_triangle(q, t) <= tol)
            })
        })
        .map(|(i, _)| i as u32)
        .collect()
}

/// Distance from `p` to the triangle `t`.
fn point_triangle(p: V3, t: &[V3; 3]) -> f64 {
    let [a, b, c] = *t;
    let n = cross(sub(b, a), sub(c, a));
    let nn = dot(n, n);
    if nn > 1e-24 {
        // Inside the prism over the triangle: the distance to its plane.
        let inside = [(a, b), (b, c), (c, a)].iter().all(|(x, y)| dot(cross(sub(*y, *x), sub(p, *x)), n) >= 0.0);
        if inside {
            return (dot(sub(p, a), n) / libm::sqrt(nn)).abs();
        }
    }
    [(a, b), (b, c), (c, a)].iter().map(|(x, y)| point_segment(p, *x, *y)).fold(f64::INFINITY, f64::min)
}

fn point_segment(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let l = dot(ab, ab);
    let s = if l > 1e-24 { (dot(sub(p, a), ab) / l).clamp(0.0, 1.0) } else { 0.0 };
    norm(sub(p, add(a, scale(ab, s))))
}

#[cfg(test)]
mod tests;
