//! Mesh utilities: vertex welding, normals, degenerate-triangle removal, validation and
//! polygon triangulation.
//!
//! Everything here is deterministic: the same input gives bit-identical output on every
//! run and every CPU (scalar `f64` arithmetic in a fixed order, no hash-map iteration
//! order dependence).

use std::collections::HashMap;
use std::ops::Range;

use gusset_kernel::Mesh;

/// A triangle as three vertex indices.
pub type Tri = [u32; 3];

/// How vertex normals are generated for display meshes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NormalMode {
    /// Every triangle gets its own three vertices carrying the face normal (faceted look;
    /// 3 vertices per triangle).
    Flat,
    /// Shared vertices with area-weighted averaged normals (smooth look everywhere).
    Smooth,
    /// Vertices are shared between adjacent triangles whose normals differ by less than
    /// `angle_deg`, and split along sharper creases. Good default for CAD-like meshes.
    Crease { angle_deg: f32 },
}

impl Default for NormalMode {
    fn default() -> Self {
        NormalMode::Crease { angle_deg: 30.0 }
    }
}

// ---------------------------------------------------------------------------------------
// small vector helpers (f64, fixed evaluation order)

pub(crate) fn v64(p: [f32; 3]) -> [f64; 3] {
    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(n: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(n, n).sqrt();
    if l > 0.0 && l.is_finite() { Some([n[0] / l, n[1] / l, n[2] / l]) } else { None }
}
fn to32(n: [f64; 3]) -> [f32; 3] {
    [n[0] as f32, n[1] as f32, n[2] as f32]
}

#[inline]
pub(crate) fn pos(positions: &[[f32; 3]], i: u32) -> [f32; 3] {
    positions.get(i as usize).copied().unwrap_or([0.0; 3])
}

/// Unnormalised face normal (its length is twice the triangle area), computed in `f64`.
pub fn face_normal_raw(positions: &[[f32; 3]], t: Tri) -> [f64; 3] {
    let a = v64(pos(positions, t[0]));
    let b = v64(pos(positions, t[1]));
    let c = v64(pos(positions, t[2]));
    cross(sub(b, a), sub(c, a))
}

/// Unit face normal, or `None` for a degenerate triangle.
pub fn face_normal(positions: &[[f32; 3]], t: Tri) -> Option<[f32; 3]> {
    unit(face_normal_raw(positions, t)).map(to32)
}

/// Iterates the triangles of a mesh's index buffer (a trailing partial triangle is ignored).
pub fn triangles(mesh: &Mesh) -> impl Iterator<Item = Tri> + '_ {
    mesh.indices.chunks_exact(3).map(|c| match c {
        [a, b, c] => [*a, *b, *c],
        _ => [0, 0, 0],
    })
}

// ---------------------------------------------------------------------------------------
// welding

/// Packs a position into a 96-bit key; -0.0 and +0.0 weld together.
fn exact_key(p: [f32; 3]) -> u128 {
    let k = p.map(|c| {
        let b = c.to_bits();
        if b & 0x7fff_ffff == 0 { 0 } else { b }
    });
    (u128::from(k[0]) << 64) | (u128::from(k[1]) << 32) | u128::from(k[2])
}

/// Fast hasher for packed position keys: a folded 64×64→128 multiply mix with a
/// per-map random seed (taken from std's `RandomState`), so adversarial inputs cannot
/// precompute collisions. Only used for lookups, never for iteration order, so results
/// stay deterministic.
#[derive(Debug, Clone, Copy)]
struct SeededMix {
    seed: u64,
}

impl SeededMix {
    fn new() -> Self {
        use std::hash::BuildHasher;
        SeededMix { seed: std::hash::RandomState::new().hash_one(0x9E37_79B9_7F4A_7C15u64) | 1 }
    }
}

#[derive(Debug)]
struct MixHasher {
    h: u64,
}

fn fold_mul(a: u64, b: u64) -> u64 {
    let r = u128::from(a) * u128::from(b);
    (r as u64) ^ ((r >> 64) as u64)
}

impl std::hash::Hasher for MixHasher {
    fn write(&mut self, bytes: &[u8]) {
        for c in bytes.chunks(8) {
            let mut w = [0u8; 8];
            for (d, s) in w.iter_mut().zip(c) {
                *d = *s;
            }
            self.write_u64(u64::from_le_bytes(w));
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.h = fold_mul(self.h ^ v, 0x5851_F42D_4C95_7F2D);
    }
    fn write_u128(&mut self, v: u128) {
        self.h = fold_mul(self.h ^ (v as u64), 0x5851_F42D_4C95_7F2D ^ ((v >> 64) as u64));
    }
    fn finish(&self) -> u64 {
        fold_mul(self.h, 0x2D35_8DCC_AA6C_78A5)
    }
}

impl std::hash::BuildHasher for SeededMix {
    type Hasher = MixHasher;
    fn build_hasher(&self) -> MixHasher {
        MixHasher { h: self.seed }
    }
}

/// Merges bit-identical positions (`-0.0` equals `0.0`). Returns the unique positions in
/// first-occurrence order and the old→new index map.
pub fn weld_exact(positions: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<u32>) {
    let mut map: HashMap<u128, u32, SeededMix> =
        HashMap::with_capacity_and_hasher(positions.len() / 2 + 1, SeededMix::new());
    let mut out = Vec::with_capacity(positions.len() / 2 + 1);
    let mut remap = Vec::with_capacity(positions.len());
    for &p in positions {
        let next = out.len() as u32;
        let id = *map.entry(exact_key(p)).or_insert_with(|| {
            out.push(p);
            next
        });
        remap.push(id);
    }
    (out, remap)
}

/// Merges positions closer than `tolerance` (Euclidean). Each output vertex is the first
/// input vertex of its cluster (no averaging), and a vertex joins the lowest-numbered
/// existing vertex within tolerance, so the result is deterministic. A tolerance that is
/// not positive and finite falls back to [`weld_exact`].
pub fn weld_with_tolerance(positions: &[[f32; 3]], tolerance: f32) -> (Vec<[f32; 3]>, Vec<u32>) {
    if !(tolerance > 0.0 && tolerance.is_finite()) {
        return weld_exact(positions);
    }
    let cell = f64::from(tolerance);
    let tol2 = cell * cell;
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut out: Vec<[f32; 3]> = Vec::new();
    let mut remap = Vec::with_capacity(positions.len());
    for &p in positions {
        let q = v64(p);
        // `as` saturates for out-of-range floats, which is fine for bucketing.
        let c = [(q[0] / cell).floor() as i64, (q[1] / cell).floor() as i64, (q[2] / cell).floor() as i64];
        let mut best: Option<u32> = None;
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                for dz in -1i64..=1 {
                    let k = [c[0].saturating_add(dx), c[1].saturating_add(dy), c[2].saturating_add(dz)];
                    if let Some(list) = grid.get(&k) {
                        for &i in list {
                            let d = sub(v64(pos(&out, i)), q);
                            if dot(d, d) <= tol2 && best.is_none_or(|b| i < b) {
                                best = Some(i);
                            }
                        }
                    }
                }
            }
        }
        let id = match best {
            Some(i) => i,
            None => {
                let i = out.len() as u32;
                out.push(p);
                grid.entry(c).or_default().push(i);
                i
            }
        };
        remap.push(id);
    }
    (out, remap)
}

/// Returns a copy of `mesh` with vertices welded (`tolerance` ≤ 0 means exact), smooth
/// normals recomputed, and triangle order, `face_ranges` and `edges` kept. Triangles that
/// collapse are kept (use [`remove_degenerate_triangles`] to drop them).
pub fn weld_mesh(mesh: &Mesh, tolerance: f32) -> Mesh {
    let (positions, remap) = weld_with_tolerance(&mesh.positions, tolerance);
    let indices: Vec<u32> = mesh.indices.iter().map(|&i| remap.get(i as usize).copied().unwrap_or(0)).collect();
    let tris: Vec<Tri> =
        indices.chunks_exact(3).map(|c| if let [a, b, c] = c { [*a, *b, *c] } else { [0; 3] }).collect();
    let normals = smooth_normals(&positions, &tris);
    Mesh { positions, normals, indices, face_ranges: mesh.face_ranges.clone(), edges: mesh.edges.clone() }
}

/// Drops unreferenced vertices; vertices are renumbered in first-use order.
pub(crate) fn compact(positions: &[[f32; 3]], tris: &mut [Tri]) -> Vec<[f32; 3]> {
    let mut map = vec![u32::MAX; positions.len()];
    let mut out = Vec::new();
    for t in tris.iter_mut() {
        for v in t.iter_mut() {
            if let Some(m) = map.get_mut(*v as usize) {
                if *m == u32::MAX {
                    *m = out.len() as u32;
                    out.push(pos(positions, *v));
                }
                *v = *m;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// degenerate triangles

/// A triangle is degenerate when two corners share an index, or when its area is
/// negligible relative to its longest edge (collinear corners).
pub fn is_degenerate(positions: &[[f32; 3]], t: Tri) -> bool {
    if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
        return true;
    }
    let a = v64(pos(positions, t[0]));
    let b = v64(pos(positions, t[1]));
    let c = v64(pos(positions, t[2]));
    let (e0, e1, e2) = (sub(b, a), sub(c, b), sub(a, c));
    let emax2 = dot(e0, e0).max(dot(e1, e1)).max(dot(e2, e2));
    let n = cross(e0, sub(c, a));
    let thr = 1e-10 * emax2;
    dot(n, n) <= thr * thr
}

/// Removes degenerate triangles in place, keeping `ranges` (a partition of the triangle
/// list) consistent. Returns how many were removed.
pub(crate) fn remove_degenerate(positions: &[[f32; 3]], tris: &mut Vec<Tri>, ranges: &mut [Range<u32>]) -> usize {
    let mut kept = Vec::with_capacity(tris.len());
    let mut new_start = 0u32;
    for r in ranges.iter_mut() {
        let lo = r.start as usize;
        let hi = (r.end as usize).min(tris.len());
        let before = kept.len();
        if let Some(slice) = tris.get(lo..hi) {
            kept.extend(slice.iter().copied().filter(|t| !is_degenerate(positions, *t)));
        }
        let n = (kept.len() - before) as u32;
        *r = new_start..new_start + n;
        new_start += n;
    }
    let removed = tris.len() - kept.len();
    *tris = kept;
    removed
}

/// Removes degenerate triangles from a mesh, keeping `face_ranges` consistent. Returns
/// the number removed. Normals and positions are left untouched.
pub fn remove_degenerate_triangles(mesh: &mut Mesh) -> usize {
    let mut tris: Vec<Tri> = triangles(mesh).collect();
    let total = tris.len() as u32;
    let mut ranges = if mesh.face_ranges.is_empty() { crate::one_range(total) } else { mesh.face_ranges.clone() };
    // Ranges must cover the triangle list; anything else is left alone.
    let covers = ranges.first().is_some_and(|r| r.start == 0)
        && ranges.last().is_some_and(|r| r.end == total)
        && ranges.windows(2).all(|w| matches!(w, [a, b] if a.end == b.start && a.start <= a.end));
    if !covers {
        ranges = crate::one_range(total);
    }
    let removed = remove_degenerate(&mesh.positions, &mut tris, &mut ranges);
    mesh.indices = tris.iter().flatten().copied().collect();
    if covers && !mesh.face_ranges.is_empty() {
        mesh.face_ranges = ranges;
    }
    removed
}

// ---------------------------------------------------------------------------------------
// normals

/// Area-weighted per-vertex normals for an indexed triangle list.
pub fn smooth_normals(positions: &[[f32; 3]], tris: &[Tri]) -> Vec<[f32; 3]> {
    let mut acc = vec![[0.0f64; 3]; positions.len()];
    for &t in tris {
        let n = face_normal_raw(positions, t);
        for v in t {
            if let Some(a) = acc.get_mut(v as usize) {
                *a = add(*a, n);
            }
        }
    }
    acc.into_iter().map(|n| unit(n).map(to32).unwrap_or([0.0, 0.0, 1.0])).collect()
}

/// Builds display buffers (positions, normals, index buffer) for an indexed triangle list.
/// Triangle order is preserved, so triangle ranges stay valid.
pub(crate) fn build_display(
    positions: Vec<[f32; 3]>,
    tris: &[Tri],
    mode: NormalMode,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    match mode {
        NormalMode::Flat => {
            let mut p = Vec::with_capacity(tris.len() * 3);
            let mut n = Vec::with_capacity(tris.len() * 3);
            for &t in tris {
                let fnrm = face_normal(&positions, t).unwrap_or([0.0, 0.0, 1.0]);
                for v in t {
                    p.push(pos(&positions, v));
                    n.push(fnrm);
                }
            }
            let idx = (0..p.len() as u32).collect();
            (p, n, idx)
        }
        NormalMode::Smooth => {
            let n = smooth_normals(&positions, tris);
            (positions, n, tris.iter().flatten().copied().collect())
        }
        NormalMode::Crease { angle_deg } => crease(&positions, tris, angle_deg),
    }
}

/// Maximum normal clusters (output vertices) per input vertex in crease mode.
const MAX_CREASE_CLUSTERS: usize = 64;

fn crease(positions: &[[f32; 3]], tris: &[Tri], angle_deg: f32) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    let cos_thr = libm::cos(f64::from(angle_deg.clamp(0.0, 180.0)).to_radians());
    let nv = positions.len();
    let raw: Vec<[f64; 3]> = tris.iter().map(|&t| face_normal_raw(positions, t)).collect();
    let unit_n: Vec<[f64; 3]> = raw.iter().map(|&n| unit(n).unwrap_or([0.0, 0.0, 0.0])).collect();

    // CSR vertex → incident triangles.
    let mut offsets = vec![0u32; nv + 1];
    for &t in tris {
        for v in t {
            if let Some(o) = offsets.get_mut(v as usize + 1) {
                *o += 1;
            }
        }
    }
    for i in 1..offsets.len() {
        let prev = offsets.get(i - 1).copied().unwrap_or(0);
        if let Some(o) = offsets.get_mut(i) {
            *o += prev;
        }
    }
    let mut fill = offsets.clone();
    let mut incident = vec![0u32; offsets.last().copied().unwrap_or(0) as usize];
    for (ti, &t) in tris.iter().enumerate() {
        for v in t {
            if let Some(f) = fill.get_mut(v as usize) {
                if let Some(slot) = incident.get_mut(*f as usize) {
                    *slot = ti as u32;
                }
                *f += 1;
            }
        }
    }

    let mut out_p = Vec::with_capacity(nv);
    let mut out_n: Vec<[f32; 3]> = Vec::with_capacity(nv);
    let mut out_i = vec![0u32; tris.len() * 3];
    // (seed unit normal, accumulated raw normal, output index)
    let mut clusters: Vec<([f64; 3], [f64; 3], u32)> = Vec::new();
    for v in 0..nv {
        clusters.clear();
        let lo = offsets.get(v).copied().unwrap_or(0) as usize;
        let hi = offsets.get(v + 1).copied().unwrap_or(0) as usize;
        for &ti in incident.get(lo..hi).unwrap_or(&[]) {
            let n = unit_n.get(ti as usize).copied().unwrap_or([0.0; 3]);
            let r = raw.get(ti as usize).copied().unwrap_or([0.0; 3]);
            let k = match clusters.iter().position(|c| dot(c.0, n) >= cos_thr) {
                Some(k) => k,
                // Bound the work per vertex (adversarial fans, tiny angles, zero normals):
                // past the cap, join the closest existing cluster.
                None if clusters.len() >= MAX_CREASE_CLUSTERS => {
                    clusters
                        .iter()
                        .enumerate()
                        .fold((0usize, f64::NEG_INFINITY), |best, (i, c)| {
                            let d = dot(c.0, n);
                            if d > best.1 { (i, d) } else { best }
                        })
                        .0
                }
                None => {
                    out_p.push(pos(positions, v as u32));
                    out_n.push([0.0; 3]);
                    clusters.push((n, [0.0; 3], (out_p.len() - 1) as u32));
                    clusters.len() - 1
                }
            };
            if let Some(c) = clusters.get_mut(k) {
                c.1 = add(c.1, r);
                let t = tris.get(ti as usize).copied().unwrap_or([0; 3]);
                for (corner, &tv) in t.iter().enumerate() {
                    if tv as usize == v
                        && let Some(slot) = out_i.get_mut(ti as usize * 3 + corner)
                    {
                        *slot = c.2;
                    }
                }
            }
        }
        for c in &clusters {
            if let Some(slot) = out_n.get_mut(c.2 as usize) {
                *slot = unit(c.1).or_else(|| unit(c.0)).map(to32).unwrap_or([0.0, 0.0, 1.0]);
            }
        }
    }
    (out_p, out_n, out_i)
}

/// Recomputes `mesh.normals` (and, for [`NormalMode::Flat`] and [`NormalMode::Crease`],
/// the vertex layout) for display. Triangle order and `face_ranges` are kept.
pub fn apply_normals(mesh: &mut Mesh, mode: NormalMode) {
    let tris: Vec<Tri> = triangles(mesh).collect();
    let (p, n, i) = build_display(std::mem::take(&mut mesh.positions), &tris, mode);
    mesh.positions = p;
    mesh.normals = n;
    mesh.indices = i;
}

// ---------------------------------------------------------------------------------------
// validation

/// Topological health of a triangle mesh, computed on exactly welded positions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshValidation {
    pub triangles: u64,
    /// Distinct vertex positions.
    pub vertices: u64,
    pub degenerate_triangles: u64,
    /// Edges used by exactly one triangle (holes / open borders).
    pub boundary_edges: u64,
    /// Edges shared by more than two triangles.
    pub non_manifold_edges: u64,
    /// Edges whose two triangles traverse them in the same direction (flipped normals).
    pub inconsistent_edges: u64,
    /// Signed volume in mesh units³ (positive when normals point outwards).
    pub signed_volume: f64,
}

impl MeshValidation {
    /// Closed two-manifold: every edge has exactly two triangles.
    pub fn is_watertight(&self) -> bool {
        self.triangles > 0 && self.boundary_edges == 0 && self.non_manifold_edges == 0
    }
    pub fn is_consistently_oriented(&self) -> bool {
        self.inconsistent_edges == 0
    }
    /// Whether the mesh could be turned into a valid solid as-is: watertight,
    /// consistently oriented, no degenerate triangles and positive volume.
    pub fn can_become_solid(&self) -> bool {
        self.is_watertight()
            && self.is_consistently_oriented()
            && self.degenerate_triangles == 0
            && self.signed_volume > 0.0
    }
}

/// Validates a mesh (positions are welded exactly first, so per-face duplicated
/// vertices of display meshes do not show up as holes).
pub fn validate(mesh: &Mesh) -> MeshValidation {
    let (positions, remap) = weld_exact(&mesh.positions);
    let tris: Vec<Tri> = triangles(mesh).map(|t| t.map(|i| remap.get(i as usize).copied().unwrap_or(0))).collect();
    validate_indexed(&positions, &tris)
}

/// Validates an indexed triangle list whose positions are already welded.
pub(crate) fn validate_indexed(positions: &[[f32; 3]], tris: &[Tri]) -> MeshValidation {
    let mut v = MeshValidation { triangles: tris.len() as u64, ..Default::default() };
    let mut used = vec![false; positions.len()];
    let mut edges: Vec<u64> = Vec::with_capacity(tris.len() * 3);
    let mut vol = 0.0f64;
    for &t in tris {
        for i in t {
            if let Some(u) = used.get_mut(i as usize) {
                *u = true;
            }
        }
        if is_degenerate(positions, t) {
            v.degenerate_triangles += 1;
        }
        let a = v64(pos(positions, t[0]));
        let b = v64(pos(positions, t[1]));
        let c = v64(pos(positions, t[2]));
        vol += dot(a, cross(b, c));
        for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if x != y {
                edges.push((u64::from(x) << 32) | u64::from(y));
            }
        }
    }
    v.vertices = used.iter().filter(|u| **u).count() as u64;
    v.signed_volume = vol / 6.0;

    if positions.len() < (1 << 31) {
        // One sort: key = (min << 32) | (max << 1) | direction.
        for e in edges.iter_mut() {
            let (x, y) = (*e >> 32, *e & 0xffff_ffff);
            *e = (x.min(y) << 32) | (x.max(y) << 1) | u64::from(x > y);
        }
        edges.sort_unstable();
        let mut it = edges.iter().peekable();
        while let Some(&e) = it.next() {
            let (mut n0, mut n1) = (0u64, 0u64);
            if e & 1 == 0 {
                n0 += 1
            } else {
                n1 += 1
            }
            while let Some(&&x) = it.peek() {
                if x >> 1 != e >> 1 {
                    break;
                }
                if x & 1 == 0 {
                    n0 += 1
                } else {
                    n1 += 1
                }
                it.next();
            }
            match n0 + n1 {
                1 => v.boundary_edges += 1,
                2 => {}
                _ => v.non_manifold_edges += 1,
            }
            v.inconsistent_edges += u64::from(n0 >= 2) + u64::from(n1 >= 2);
        }
        return v;
    }
    // Directed duplicates: two triangles running an edge the same way.
    edges.sort_unstable();
    v.inconsistent_edges = count_runs(&edges, |n| n >= 2);
    // Undirected multiplicity.
    for e in edges.iter_mut() {
        let (x, y) = (*e >> 32, *e & 0xffff_ffff);
        *e = (x.min(y) << 32) | x.max(y);
    }
    edges.sort_unstable();
    v.boundary_edges = count_runs(&edges, |n| n == 1);
    v.non_manifold_edges = count_runs(&edges, |n| n > 2);
    v
}

fn count_runs(sorted: &[u64], pred: impl Fn(usize) -> bool) -> u64 {
    let mut count = 0u64;
    let mut it = sorted.iter().peekable();
    while let Some(&e) = it.next() {
        let mut n = 1usize;
        while it.peek().is_some_and(|&&x| x == e) {
            it.next();
            n += 1;
        }
        if pred(n) {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------------------
// polygon triangulation

/// Polygons with more corners than this are fan-triangulated (ear clipping is O(n²)–O(n³)).
pub const EAR_CLIP_MAX: usize = 256;

/// Triangulates a planar-ish simple polygon by ear clipping in its best-fit plane,
/// preserving the polygon's winding. Returns the triangles (indices into `points`) and
/// whether ear clipping succeeded; on failure (self-intersecting or degenerate polygons,
/// or more than [`EAR_CLIP_MAX`] corners) a triangle fan is returned.
pub fn triangulate_polygon(points: &[[f64; 3]]) -> (Vec<[usize; 3]>, bool) {
    let mut unlimited = u64::MAX;
    triangulate_polygon_budgeted(points, &mut unlimited)
}

/// [`triangulate_polygon`] with a shared work budget (point-in-triangle tests). When the
/// budget runs out the polygon is fan-triangulated, so a file full of large polygons
/// cannot make an import take quadratic-times-file-size time.
pub fn triangulate_polygon_budgeted(points: &[[f64; 3]], budget: &mut u64) -> (Vec<[usize; 3]>, bool) {
    let n = points.len();
    let fan = || (1..n.saturating_sub(1)).map(|i| [0, i, i + 1]).collect::<Vec<_>>();
    if n < 3 {
        return (Vec::new(), true);
    }
    if n == 3 {
        return (vec![[0, 1, 2]], true);
    }
    if n > EAR_CLIP_MAX {
        return (fan(), false);
    }
    // Newell normal → dominant axis → 2D projection.
    let mut nrm = [0.0f64; 3];
    for i in 0..n {
        let p = points.get(i).copied().unwrap_or_default();
        let q = points.get((i + 1) % n).copied().unwrap_or_default();
        nrm[0] += (p[1] - q[1]) * (p[2] + q[2]);
        nrm[1] += (p[2] - q[2]) * (p[0] + q[0]);
        nrm[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let ax = [nrm[0].abs(), nrm[1].abs(), nrm[2].abs()];
    let (u, w) = if ax[0] >= ax[1] && ax[0] >= ax[2] {
        (1, 2)
    } else if ax[1] >= ax[2] {
        (2, 0)
    } else {
        (0, 1)
    };
    let p2: Vec<[f64; 2]> =
        points.iter().map(|p| [p.get(u).copied().unwrap_or(0.0), p.get(w).copied().unwrap_or(0.0)]).collect();
    let pt = |i: usize| p2.get(i).copied().unwrap_or([0.0, 0.0]);
    let cross2 = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut area = 0.0;
    for i in 0..n {
        let a = pt(i);
        let b = pt((i + 1) % n);
        area += a[0] * b[1] - b[0] * a[1];
    }
    if !(area.abs() > 0.0 && area.is_finite()) {
        return (fan(), false);
    }
    let o = area.signum();

    let mut ring: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = n * n + 8;
    while ring.len() > 3 {
        guard = guard.saturating_sub(1);
        if guard == 0 {
            return (fan(), false);
        }
        let m = ring.len();
        let mut found = None;
        for k in 0..m {
            let ia = ring.get((k + m - 1) % m).copied().unwrap_or(0);
            let ib = ring.get(k).copied().unwrap_or(0);
            let ic = ring.get((k + 1) % m).copied().unwrap_or(0);
            let (a, b, c) = (pt(ia), pt(ib), pt(ic));
            if cross2(a, b, c) * o <= 0.0 {
                continue; // reflex or flat corner
            }
            *budget = budget.saturating_sub(m as u64);
            if *budget == 0 {
                return (fan(), false);
            }
            let blocked = ring.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = pt(j);
                cross2(a, b, p) * o >= 0.0 && cross2(b, c, p) * o >= 0.0 && cross2(c, a, p) * o >= 0.0
            });
            if !blocked {
                found = Some((k, [ia, ib, ic]));
                break;
            }
        }
        match found {
            Some((k, tri)) => {
                out.push(tri);
                ring.remove(k);
            }
            None => return (fan(), false),
        }
    }
    if let [a, b, c] = ring[..] {
        out.push([a, b, c]);
    }
    (out, true)
}
