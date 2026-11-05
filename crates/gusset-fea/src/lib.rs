//! Linear static analysis on a voxel grid (ADR 0041): a solid's closed triangle mesh is
//! filled with cubic voxels (a voxel is in when its centre is, by ray parity along X), each
//! voxel is an 8-node hexahedron (isotropic linear elasticity, 2×2×2 Gauss points), and the
//! system is solved matrix-free by conjugate gradients with a Jacobi preconditioner.
//! Fixed faces and face loads are given by the mesh's face index (its triangle range); they
//! act on the nodes of the voxel faces lying on that face. Units: mm, N, MPa.

use gusset_kernel::Mesh;

mod mg;

#[derive(Debug, Clone, PartialEq)]
pub enum FeaError {
    Invalid(String),
    TooLarge(String),
    /// The solver did not converge (the model may be free to move).
    NotConverged(String),
}

impl std::fmt::Display for FeaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeaError::Invalid(m) | FeaError::TooLarge(m) | FeaError::NotConverged(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for FeaError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Young's modulus (MPa).
    pub young: f64,
    pub poisson: f64,
}

/// A force (N, total over the face) on mesh face `face`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Load {
    pub face: usize,
    pub force: [f64; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem<'a> {
    pub mesh: &'a Mesh,
    /// Voxel edge (mm).
    pub voxel: f64,
    pub fixed: Vec<usize>,
    pub loads: Vec<Load>,
    pub material: Material,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub elements: usize,
    pub nodes: usize,
    pub iterations: usize,
    /// Largest displacement (mm) and where.
    pub max_displacement: f64,
    pub max_displacement_at: [f64; 3],
    /// Largest von Mises stress at an element centre (MPa) and where.
    pub max_von_mises: f64,
    pub max_von_mises_at: [f64; 3],
    /// Grid nodes held and loaded.
    pub fixed_nodes: usize,
    pub loaded_nodes: usize,
    /// The share of the load (by size) that fell on held nodes and so does nothing.
    pub held_load: f64,
    /// The voxel model's outer faces with the von Mises stress (MPa) of their voxel, for
    /// a stress picture.
    pub surface: Vec<SurfaceQuad>,
}

/// One outer face of a voxel: its corners counter-clockwise seen from outside.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceQuad {
    pub corners: [[f64; 3]; 4],
    pub von_mises: f64,
}

/// Most voxels one analysis may have.
pub const MAX_ELEMENTS: usize = 200_000;
const MAX_ITERATIONS: usize = 20_000;

type V3 = [f64; 3];

/// The 8 corners of a hexahedron as offsets (x fastest).
const CORNERS: [[usize; 3]; 8] =
    [[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 1, 0], [0, 0, 1], [1, 0, 1], [0, 1, 1], [1, 1, 1]];

/// The 24×24 stiffness of a cube of edge `h`, and the strain-displacement matrix at its
/// centre (6×24).
fn element(h: f64, m: &Material) -> (Vec<f64>, Vec<f64>) {
    let d = dmat(m);
    // B at natural coordinates (ξ, η, ζ).
    let b_at = |xi: [f64; 3]| {
        let mut b = vec![0.0; 6 * 24];
        for (n, cn) in CORNERS.iter().enumerate() {
            let s = cn.map(|v| if v == 0 { -1.0 } else { 1.0 });
            let f = |a: usize| 1.0 + s.get(a).copied().unwrap_or(0.0) * xi.get(a).copied().unwrap_or(0.0);
            // dN/dx = (2/h) dN/dξ, N = (1+sξ)(1+tη)(1+uζ)/8.
            let k = 2.0 / h / 8.0;
            let dx = k * s[0] * f(1) * f(2);
            let dy = k * s[1] * f(0) * f(2);
            let dz = k * s[2] * f(0) * f(1);
            let mut set = |row: usize, col: usize, v: f64| {
                if let Some(x) = b.get_mut(row * 24 + col) {
                    *x = v;
                }
            };
            let (u, v, w) = (3 * n, 3 * n + 1, 3 * n + 2);
            set(0, u, dx);
            set(1, v, dy);
            set(2, w, dz);
            set(3, u, dy);
            set(3, v, dx);
            set(4, v, dz);
            set(4, w, dy);
            set(5, u, dz);
            set(5, w, dx);
        }
        b
    };
    let g = 1.0 / libm::sqrt(3.0);
    let det = (h / 2.0) * (h / 2.0) * (h / 2.0);
    let mut ke = vec![0.0; 24 * 24];
    for gx in [-g, g] {
        for gy in [-g, g] {
            for gz in [-g, g] {
                let b = b_at([gx, gy, gz]);
                // ke += Bᵀ D B det
                let mut db = vec![0.0; 6 * 24];
                for (i, drow) in d.iter().enumerate() {
                    for j in 0..24 {
                        let mut acc = 0.0;
                        for (k, dik) in drow.iter().enumerate() {
                            acc += dik * b.get(k * 24 + j).copied().unwrap_or(0.0);
                        }
                        if let Some(x) = db.get_mut(i * 24 + j) {
                            *x = acc;
                        }
                    }
                }
                for i in 0..24 {
                    for j in 0..24 {
                        let mut acc = 0.0;
                        for k in 0..6 {
                            acc +=
                                b.get(k * 24 + i).copied().unwrap_or(0.0) * db.get(k * 24 + j).copied().unwrap_or(0.0);
                        }
                        if let Some(x) = ke.get_mut(i * 24 + j) {
                            *x += acc * det;
                        }
                    }
                }
            }
        }
    }
    (ke, b_at([0.0, 0.0, 0.0]))
}

fn dmat(m: &Material) -> [[f64; 6]; 6] {
    let (e, nu) = (m.young, m.poisson);
    let c = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
    let mut d = [[0.0; 6]; 6];
    for (i, row) in d.iter_mut().enumerate() {
        for (j, x) in row.iter_mut().enumerate() {
            *x = match (i < 3, j < 3) {
                (true, true) if i == j => c * (1.0 - nu),
                (true, true) => c * nu,
                (false, false) if i == j => c * (1.0 - 2.0 * nu) / 2.0,
                _ => 0.0,
            };
        }
    }
    d
}

struct Grid {
    origin: V3,
    h: f64,
    n: [usize; 3],
}

impl Grid {
    fn node_index(&self, i: usize, j: usize, k: usize) -> usize {
        i + (self.n[0] + 1) * (j + (self.n[1] + 1) * k)
    }
    fn node_pos(&self, i: usize, j: usize, k: usize) -> V3 {
        [self.origin[0] + i as f64 * self.h, self.origin[1] + j as f64 * self.h, self.origin[2] + k as f64 * self.h]
    }
}

fn tri(mesh: &Mesh, t: usize) -> Option<[V3; 3]> {
    let p = |i: usize| -> Option<V3> {
        let idx = usize::try_from(*mesh.indices.get(i)?).ok()?;
        let q = mesh.positions.get(idx)?;
        Some([f64::from(q[0]), f64::from(q[1]), f64::from(q[2])])
    };
    Some([p(3 * t)?, p(3 * t + 1)?, p(3 * t + 2)?])
}

/// Which voxels have their centre inside the mesh (one ray along X per voxel row).
fn voxelize(mesh: &Mesh, g: &Grid) -> Vec<bool> {
    let [nx, ny, nz] = g.n;
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); ny * nz];
    // A tiny irrational shift keeps rays off edges and vertices.
    let jit = g.h * 1.234_567e-5;
    let ray = |j: usize, k: usize| {
        (g.origin[1] + (j as f64 + 0.5) * g.h + jit, g.origin[2] + (k as f64 + 0.5) * g.h + jit * 0.618)
    };
    for t in 0..mesh.triangle_count() {
        let Some(p) = tri(mesh, t) else { continue };
        let (y0, y1) = (
            p.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min),
            p.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max),
        );
        let (z0, z1) = (
            p.iter().map(|q| q[2]).fold(f64::INFINITY, f64::min),
            p.iter().map(|q| q[2]).fold(f64::NEG_INFINITY, f64::max),
        );
        let idx = |v: f64, o: f64, n: usize| (((v - o) / g.h - 0.5).max(0.0) as usize).min(n.saturating_sub(1));
        let (ja, jb) = (idx(y0, g.origin[1], ny), idx(y1, g.origin[1], ny) + 1);
        let (ka, kb) = (idx(z0, g.origin[2], nz), idx(z1, g.origin[2], nz) + 1);
        for k in ka..=kb.min(nz.saturating_sub(1)) {
            for j in ja..=jb.min(ny.saturating_sub(1)) {
                if let Some(r) = rows.get_mut(j + ny * k) {
                    r.push(t);
                }
            }
        }
    }
    let mut inside = vec![false; nx * ny * nz];
    for k in 0..nz {
        for j in 0..ny {
            let (y, z) = ray(j, k);
            let mut xs: Vec<f64> = Vec::new();
            for &t in rows.get(j + ny * k).map_or(&[][..], Vec::as_slice) {
                let Some([a, b, c]) = tri(mesh, t) else { continue };
                // Barycentric coordinates in the YZ plane.
                let den = (b[1] - a[1]) * (c[2] - a[2]) - (c[1] - a[1]) * (b[2] - a[2]);
                if den.abs() < 1e-300 {
                    continue;
                }
                let u = ((y - a[1]) * (c[2] - a[2]) - (c[1] - a[1]) * (z - a[2])) / den;
                let v = ((b[1] - a[1]) * (z - a[2]) - (y - a[1]) * (b[2] - a[2])) / den;
                if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
                    xs.push(a[0] + u * (b[0] - a[0]) + v * (c[0] - a[0]));
                }
            }
            xs.sort_by(f64::total_cmp);
            for i in 0..nx {
                let x = g.origin[0] + (i as f64 + 0.5) * g.h;
                let crossings = xs.iter().filter(|v| **v < x).count();
                if crossings % 2 == 1
                    && let Some(c) = inside.get_mut(i + nx * (j + ny * k))
                {
                    *c = true;
                }
            }
        }
    }
    inside
}

/// The closest point of triangle `t` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: V3, [a, b, c]: [V3; 3]) -> V3 {
    let sub = |x: V3, y: V3| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let dot = |x: V3, y: V3| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let lerp = |x: V3, d: V3, t: f64| [x[0] + d[0] * t, x[1] + d[1] * t, x[2] + d[2] * t];
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return lerp(a, ab, d1 / (d1 - d3));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return lerp(a, ac, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return lerp(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let den = 1.0 / (va + vb + vc);
    let (v, w) = (vb * den, vc * den);
    [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
}

/// The boundary quads of the voxel model (faces between a voxel and empty space), as their
/// four grid nodes (grid indices, counter-clockwise seen from outside), centre, normal axis
/// and the voxel's index in the grid.
fn boundary_quads(g: &Grid, inside: &[bool]) -> Vec<([usize; 4], V3, usize, usize)> {
    let [nx, ny, nz] = g.n;
    let at = |i: i64, j: i64, k: i64| {
        i >= 0
            && j >= 0
            && k >= 0
            && (i as usize) < nx
            && (j as usize) < ny
            && (k as usize) < nz
            && inside.get(i as usize + nx * (j as usize + ny * k as usize)).copied().unwrap_or(false)
    };
    let mut out = Vec::new();
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                if !at(i as i64, j as i64, k as i64) {
                    continue;
                }
                // (axis, side): the face on that side of the voxel.
                for axis in 0..3 {
                    for side in [0usize, 1] {
                        let mut nb = [i as i64, j as i64, k as i64];
                        if let Some(v) = nb.get_mut(axis) {
                            *v += if side == 0 { -1 } else { 1 };
                        }
                        if at(nb[0], nb[1], nb[2]) {
                            continue;
                        }
                        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                        let mut corners = [0usize; 4];
                        let mut centre = [0.0; 3];
                        for (q, (du, dv)) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
                            let mut o = [i, j, k];
                            if let Some(x) = o.get_mut(axis) {
                                *x += side;
                            }
                            if let Some(x) = o.get_mut(u) {
                                *x += du;
                            }
                            if let Some(x) = o.get_mut(v) {
                                *x += dv;
                            }
                            if let Some(c) = corners.get_mut(q) {
                                *c = g.node_index(o[0], o[1], o[2]);
                            }
                            let pos = g.node_pos(o[0], o[1], o[2]);
                            for d in 0..3 {
                                if let Some(c) = centre.get_mut(d) {
                                    *c += pos.get(d).copied().unwrap_or(0.0) / 4.0;
                                }
                            }
                        }
                        if side == 0 {
                            corners.swap(1, 3);
                        }
                        out.push((corners, centre, axis, i + nx * (j + ny * k)));
                    }
                }
            }
        }
    }
    out
}

/// The nodes of the boundary quads lying on mesh face `face` (centre within half a voxel of
/// it), each with its share of the face's area in quarter quads.
fn face_nodes(
    mesh: &Mesh,
    face: usize,
    quads: &[([usize; 4], V3, usize, usize)],
    g: &Grid,
    used: &[Option<usize>],
) -> Result<Vec<(usize, usize)>, FeaError> {
    let r = mesh.face_ranges.get(face).ok_or_else(|| FeaError::Invalid(format!("no face {face}")))?;
    let count = mesh.indices.len() / 3;
    let tris: Vec<[V3; 3]> = (r.start as usize..(r.end as usize).min(count)).filter_map(|t| tri(mesh, t)).collect();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in tris.iter().flatten() {
        for d in 0..3 {
            if let (Some(l), Some(h)) = (lo.get_mut(d), hi.get_mut(d)) {
                *l = l.min(p.get(d).copied().unwrap_or(0.0));
                *h = h.max(p.get(d).copied().unwrap_or(0.0));
            }
        }
    }
    let reach = g.h * 0.51;
    let mut out: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for (corners, c, axis, _) in quads {
        if (0..3).any(|d| {
            let v = c.get(d).copied().unwrap_or(0.0);
            v < lo.get(d).copied().unwrap_or(0.0) - reach || v > hi.get(d).copied().unwrap_or(0.0) + reach
        }) {
            continue;
        }
        let near = tris.iter().any(|t| {
            // The triangle must face the quad's way (not a side face's edge nearby).
            let [a, b, cc] = *t;
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let len = libm::sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
            if len <= 0.0 || n.get(*axis).copied().unwrap_or(0.0).abs() < 0.3 * len {
                return false;
            }
            let q = closest_on_triangle(*c, *t);
            libm::sqrt((q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2) + (q[2] - c[2]).powi(2)) <= reach
        });
        if near {
            for gi in corners {
                if let Some(Some(id)) = used.get(*gi) {
                    *out.entry(*id).or_default() += 1;
                }
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// The voxel model of a body held at some faces: grid, elements, nodes, fixed degrees of
/// freedom, the element stiffness and the Jacobi preconditioner.
struct Model {
    g: Grid,
    n: [usize; 3],
    h: f64,
    elems: Vec<[usize; 3]>,
    conn: Vec<[usize; 8]>,
    used: Vec<Option<usize>>,
    node_pos: Vec<V3>,
    quads: Vec<([usize; 4], V3, usize, usize)>,
    fixed: Vec<bool>,
    fixed_nodes: usize,
    ke: Vec<f64>,
    b0: Vec<f64>,
    diag: Vec<f64>,
    material: Material,
}

fn build(mesh: &Mesh, voxel: f64, fixed_faces: &[usize], m: Material) -> Result<Model, FeaError> {
    let bad = |m: &str| Err(FeaError::Invalid(m.to_string()));
    if !(m.young.is_finite()
        && (1e-3..=1e9).contains(&m.young)
        && m.poisson.is_finite()
        && m.poisson > -0.99
        && m.poisson < 0.49)
    {
        return bad("the material needs a positive Young's modulus and a Poisson ratio below 0.49");
    }
    if !(voxel.is_finite() && voxel > 0.0) {
        return bad("the voxel size must be a positive length");
    }
    if fixed_faces.is_empty() {
        return bad("fix at least one face, or the part is free to move");
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for q in &mesh.positions {
        for d in 0..3 {
            let v = f64::from(q.get(d).copied().unwrap_or(0.0));
            if let (Some(l), Some(h)) = (lo.get_mut(d), hi.get_mut(d)) {
                *l = l.min(v);
                *h = h.max(v);
            }
        }
    }
    if !lo.iter().chain(&hi).all(|v| v.is_finite()) {
        return bad("the body has no geometry");
    }
    let h = voxel;
    let n = [0, 1, 2].map(|d| {
        let span = hi.get(d).copied().unwrap_or(0.0) - lo.get(d).copied().unwrap_or(0.0);
        ((span / h - 1e-9).ceil().max(1.0)) as usize
    });
    let cells = n.iter().try_fold(1usize, |a, v| a.checked_mul(*v));
    if cells.is_none_or(|c| c > MAX_ELEMENTS * 8) {
        return Err(FeaError::TooLarge("the voxel grid is too fine for the body: use a larger voxel".into()));
    }
    let g = Grid { origin: lo, h, n };
    let inside = voxelize(mesh, &g);
    let elems: Vec<[usize; 3]> = (0..n[2])
        .flat_map(|k| (0..n[1]).flat_map(move |j| (0..n[0]).map(move |i| [i, j, k])))
        .filter(|[i, j, k]| inside.get(i + n[0] * (j + n[1] * k)).copied().unwrap_or(false))
        .collect();
    if elems.is_empty() {
        return Err(FeaError::Invalid("no voxel lies inside the body: use a smaller voxel".into()));
    }
    if elems.len() > MAX_ELEMENTS {
        return Err(FeaError::TooLarge(format!("more than {MAX_ELEMENTS} voxels: use a larger voxel")));
    }
    // Number the nodes the elements use.
    let mut used: Vec<Option<usize>> = vec![None; (n[0] + 1) * (n[1] + 1) * (n[2] + 1)];
    let mut node_pos: Vec<V3> = Vec::new();
    let mut conn: Vec<[usize; 8]> = Vec::with_capacity(elems.len());
    for [i, j, k] in &elems {
        let mut c = [0usize; 8];
        for (slot, off) in c.iter_mut().zip(CORNERS) {
            let gi = g.node_index(i + off[0], j + off[1], k + off[2]);
            let id = match used.get(gi).copied().flatten() {
                Some(id) => id,
                None => {
                    let id = node_pos.len();
                    node_pos.push(g.node_pos(i + off[0], j + off[1], k + off[2]));
                    if let Some(u) = used.get_mut(gi) {
                        *u = Some(id);
                    }
                    id
                }
            };
            *slot = id;
        }
        conn.push(c);
    }
    let dofs = node_pos.len() * 3;
    let quads = boundary_quads(&g, &inside);
    let mut fixed = vec![false; dofs];
    let mut fixed_nodes = 0;
    for f in fixed_faces {
        for (id, _) in face_nodes(mesh, *f, &quads, &g, &used)? {
            if !fixed.get(3 * id).copied().unwrap_or(true) {
                fixed_nodes += 1;
            }
            for d in 0..3 {
                if let Some(x) = fixed.get_mut(3 * id + d) {
                    *x = true;
                }
            }
        }
    }
    if fixed_nodes == 0 {
        return bad("the fixed faces reach no voxel: use a smaller voxel");
    }
    // Every voxel must be held through faces: one joined to the rest only along an edge or
    // at a corner (or not at all) is free to turn, and the solvers never converge.
    let loose = loose_voxels(&elems, &conn, &fixed, n);
    if loose > 0 {
        return Err(FeaError::Invalid(format!(
            "{loose} voxels are not held: they touch the held part only at edges or corners, or not at all \
             (use a smaller voxel, or fix a face of each part)"
        )));
    }
    let (ke, b0) = element(h, &m);
    // Jacobi preconditioner.
    let mut diag = vec![0.0; dofs];
    for c in &conn {
        for (a, id) in c.iter().enumerate() {
            for d in 0..3 {
                if let Some(x) = diag.get_mut(3 * id + d) {
                    *x += ke.get((3 * a + d) * 25).copied().unwrap_or(0.0);
                }
            }
        }
    }
    for (i, x) in diag.iter_mut().enumerate() {
        if fixed.get(i).copied().unwrap_or(false) || *x <= 0.0 {
            *x = 1.0;
        }
    }
    Ok(Model { g, n, h, elems, conn, used, node_pos, quads, fixed, fixed_nodes, ke, b0, diag, material: m })
}

/// The elements not reached through shared faces from an element with a held node.
fn loose_voxels(elems: &[[usize; 3]], conn: &[[usize; 8]], fixed: &[bool], n: [usize; 3]) -> usize {
    let lin = |[i, j, k]: [usize; 3]| i + n[0] * (j + n[1] * k);
    // The elements are in grid order: find a neighbour by binary search.
    let keys: Vec<usize> = elems.iter().map(|e| lin(*e)).collect();
    let mut seen = vec![false; elems.len()];
    let mut stack: Vec<usize> = conn
        .iter()
        .enumerate()
        .filter(|(_, c)| c.iter().any(|id| fixed.get(3 * id).copied().unwrap_or(false)))
        .map(|(e, _)| e)
        .collect();
    for e in &stack {
        if let Some(x) = seen.get_mut(*e) {
            *x = true;
        }
    }
    while let Some(e) = stack.pop() {
        let Some(&[i, j, k]) = elems.get(e) else { continue };
        let nb = [
            (i > 0).then(|| [i - 1, j, k]),
            (i + 1 < n[0]).then_some([i + 1, j, k]),
            (j > 0).then(|| [i, j - 1, k]),
            (j + 1 < n[1]).then_some([i, j + 1, k]),
            (k > 0).then(|| [i, j, k - 1]),
            (k + 1 < n[2]).then_some([i, j, k + 1]),
        ];
        for c in nb.into_iter().flatten() {
            if let Ok(f) = keys.binary_search(&lin(c))
                && let Some(x) = seen.get_mut(f)
                && !*x
            {
                *x = true;
                stack.push(f);
            }
        }
    }
    seen.iter().filter(|s| !**s).count()
}

/// Element products one analysis may compute (elements × matrix products): about half a
/// minute of work on one core.
const MAX_WORK: u64 = 150_000_000;

/// Stiffness products of one preconditioned iteration (the multigrid cycle and the product).
const CYCLE_PRODUCTS: u64 = 6;

fn over_budget(work: u64) -> Result<(), FeaError> {
    if work > MAX_WORK {
        return Err(FeaError::TooLarge(
            "the analysis needs more work than allowed: use a larger voxel (or fewer modes)".into(),
        ));
    }
    Ok(())
}

impl Model {
    fn dofs(&self) -> usize {
        self.node_pos.len() * 3
    }

    /// `y = K u`, with the fixed degrees of freedom passed through.
    fn matvec(&self, u: &[f64], y: &mut [f64]) {
        mg::product(&self.conn, &[], &self.ke, &self.fixed, u, y);
    }
}

/// Solves the problem.
pub fn solve(p: &Problem<'_>) -> Result<Solution, FeaError> {
    let bad = |m: &str| Err(FeaError::Invalid(m.to_string()));
    let md = build(p.mesh, p.voxel, &p.fixed, p.material)?;
    let Model { g, n, h, elems, conn, used, node_pos, quads, fixed, fixed_nodes, b0, material: m, .. } = &md;
    let (h, n, fixed_nodes, m) = (*h, *n, *fixed_nodes, *m);
    let dofs = md.dofs();
    let matvec = |u: &[f64], y: &mut [f64]| md.matvec(u, y);
    let mut f = vec![0.0; dofs];
    let mut loaded_nodes = 0;
    if p.loads.is_empty() {
        return bad("add a load: with none, nothing moves");
    }
    let (mut asked, mut held) = (0.0, 0.0);
    for l in &p.loads {
        if !l.force.iter().all(|v| v.is_finite() && v.abs() <= 1e12) {
            return bad("a load is not a number (or above 1e12 N)");
        }
        let ids = face_nodes(p.mesh, l.face, quads, g, used)?;
        if ids.is_empty() {
            return bad("a loaded face reaches no voxel: use a smaller voxel");
        }
        loaded_nodes += ids.len();
        let total: usize = ids.iter().map(|(_, c)| c).sum();
        if ids.iter().all(|(id, _)| fixed.get(3 * id).copied().unwrap_or(false)) {
            return bad("a load is on held voxels only (its face is fixed): it would do nothing");
        }
        let size = libm::sqrt(l.force.iter().map(|v| v * v).sum::<f64>());
        asked += size;
        for (id, count) in ids {
            let share = count as f64 / total.max(1) as f64;
            if fixed.get(3 * id).copied().unwrap_or(false) {
                held += size * share;
            }
            for d in 0..3 {
                if let Some(x) = f.get_mut(3 * id + d) {
                    *x += l.force.get(d).copied().unwrap_or(0.0) * share;
                }
            }
        }
    }
    for (i, x) in f.iter_mut().enumerate() {
        if fixed.get(i).copied().unwrap_or(false) {
            *x = 0.0;
        }
    }
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut u = vec![0.0; dofs];
    let mut r = f.clone();
    let mg = mg::Hierarchy::new(elems, n, used, conn, fixed, h, m);
    let pre = |r: &[f64]| mg.apply(r);
    let mut z: Vec<f64> = pre(&r);
    let mut pdir = z.clone();
    let mut ap = vec![0.0; dofs];
    let mut rz = dot(&r, &z);
    let fnorm = libm::sqrt(dot(&f, &f)).max(1e-300);
    let mut iterations = 0;
    while libm::sqrt(dot(&r, &r)) > 1e-9 * fnorm {
        if iterations >= MAX_ITERATIONS {
            return Err(FeaError::NotConverged(
                "the solver did not converge: the part may be free to move (fix more faces)".into(),
            ));
        }
        over_budget(iterations as u64 * CYCLE_PRODUCTS * conn.len() as u64)?;
        iterations += 1;
        matvec(&pdir, &mut ap);
        let pap = dot(&pdir, &ap);
        if pap <= 0.0 || !pap.is_finite() {
            return Err(FeaError::NotConverged("the model is not held: fix more faces".into()));
        }
        let alpha = rz / pap;
        for i in 0..dofs {
            if let (Some(ui), Some(pi), Some(ri), Some(ai)) = (u.get_mut(i), pdir.get(i), r.get_mut(i), ap.get(i)) {
                *ui += alpha * pi;
                *ri -= alpha * ai;
            }
        }
        z = pre(&r);
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for (pi, zi) in pdir.iter_mut().zip(&z) {
            *pi = zi + beta * *pi;
        }
    }
    if !u.iter().all(|v| v.is_finite()) {
        return Err(FeaError::NotConverged("the results are not finite: check the material and the loads".into()));
    }
    // Results.
    let mut max_d = (0.0, [0.0; 3]);
    for (id, pos) in node_pos.iter().enumerate() {
        let v = [0, 1, 2].map(|d| u.get(3 * id + d).copied().unwrap_or(0.0));
        let m = libm::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
        if m > max_d.0 {
            max_d = (m, *pos);
        }
    }
    let dm = dmat(&m);
    let mut max_s = (0.0, [0.0; 3]);
    let mut vms = Vec::with_capacity(conn.len());
    for (c, [i, j, k]) in conn.iter().zip(elems) {
        let mut ue = [0.0; 24];
        for (a, id) in c.iter().enumerate() {
            for d in 0..3 {
                if let Some(x) = ue.get_mut(3 * a + d) {
                    *x = u.get(3 * id + d).copied().unwrap_or(0.0);
                }
            }
        }
        let eps: [f64; 6] = [0, 1, 2, 3, 4, 5].map(|row| {
            ue.iter().enumerate().map(|(col, uv)| b0.get(row * 24 + col).copied().unwrap_or(0.0) * uv).sum()
        });
        let s: [f64; 6] = dm.map(|drow| drow.iter().zip(&eps).map(|(dv, ev)| dv * ev).sum());
        let vm = libm::sqrt(
            0.5 * ((s[0] - s[1]).powi(2) + (s[1] - s[2]).powi(2) + (s[2] - s[0]).powi(2))
                + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5]),
        );
        if vm > max_s.0 {
            let c = g.node_pos(*i, *j, *k).map(|v| v + h / 2.0);
            max_s = (vm, c);
        }
        vms.push(vm);
    }
    // The elements are in grid order, so a voxel's element is found by its grid index.
    let lin: Vec<usize> = elems.iter().map(|[i, j, k]| i + n[0] * (j + n[1] * k)).collect();
    let pos_of = |gi: usize| {
        let (i, rest) = (gi % (n[0] + 1), gi / (n[0] + 1));
        g.node_pos(i, rest % (n[1] + 1), rest / (n[1] + 1))
    };
    let surface = quads
        .iter()
        .filter_map(|(corners, _, _, cell)| {
            let e = lin.binary_search(cell).ok()?;
            Some(SurfaceQuad { corners: corners.map(pos_of), von_mises: vms.get(e).copied().unwrap_or(0.0) })
        })
        .collect();
    Ok(Solution {
        elements: conn.len(),
        nodes: node_pos.len(),
        iterations,
        max_displacement: max_d.0,
        max_displacement_at: max_d.1,
        max_von_mises: max_s.0,
        max_von_mises_at: max_s.1,
        fixed_nodes,
        loaded_nodes,
        held_load: if asked > 0.0 { held / asked } else { 0.0 },
        surface,
    })
}

/// A natural-frequency analysis: the body held at `fixed` faces, `density` in t/mm³
/// (steel 7.85e-9), the `modes` lowest frequencies.
#[derive(Debug, Clone)]
pub struct ModalProblem<'a> {
    pub mesh: &'a Mesh,
    pub voxel: f64,
    pub fixed: Vec<usize>,
    pub material: Material,
    pub density: f64,
    pub modes: usize,
    /// A mode (0 = the lowest) whose shape to return on the outer voxel faces.
    pub shape: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Modes {
    pub elements: usize,
    pub iterations: usize,
    /// Lowest first (Hz).
    pub frequencies: Vec<f64>,
    /// The asked mode's shape on the outer voxel faces, the largest motion 1.
    pub shape: Vec<ShapeQuad>,
}

/// One outer face of a voxel (corners counter-clockwise seen from outside) and how far each
/// corner moves in a mode shape.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeQuad {
    pub corners: [[f64; 3]; 4],
    pub motion: [[f64; 3]; 4],
}

/// Most modes one analysis finds.
pub const MAX_MODES: usize = 10;
const MAX_EIGEN_ITERATIONS: usize = 3_000;
/// Most voxels of a frequency analysis (each iteration costs a block of products).
const MAX_MODAL_ELEMENTS: usize = 50_000;

/// Eigenvalues (ascending) and eigenvectors (columns, `v[row * k + col]`) of the symmetric
/// `k × k` matrix `a`, by cyclic Jacobi rotations.
fn sym_eig(mut a: Vec<f64>, k: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; k * k];
    for i in 0..k {
        if let Some(x) = v.get_mut(i * k + i) {
            *x = 1.0;
        }
    }
    let at = |a: &[f64], i: usize, j: usize| a.get(i * k + j).copied().unwrap_or(0.0);
    for _ in 0..100 {
        let off: f64 = (0..k)
            .flat_map(|i| (0..k).filter(move |j| *j != i).map(move |j| (i, j)))
            .map(|(i, j)| at(&a, i, j).powi(2))
            .sum();
        let scale: f64 = (0..k).map(|i| at(&a, i, i).powi(2)).sum::<f64>().max(1e-300);
        if off <= 1e-30 * scale {
            break;
        }
        for p in 0..k {
            for q in p + 1..k {
                let apq = at(&a, p, q);
                if apq.abs() < 1e-300 {
                    continue;
                }
                let theta = (at(&a, q, q) - at(&a, p, p)) / (2.0 * apq);
                let t = theta.signum().max(0.0).mul_add(2.0, -1.0) / (theta.abs() + libm::sqrt(theta * theta + 1.0));
                let c = 1.0 / libm::sqrt(t * t + 1.0);
                let sn = t * c;
                for r in 0..k {
                    let (arp, arq) = (at(&a, r, p), at(&a, r, q));
                    if let Some(x) = a.get_mut(r * k + p) {
                        *x = c * arp - sn * arq;
                    }
                    if let Some(x) = a.get_mut(r * k + q) {
                        *x = sn * arp + c * arq;
                    }
                }
                for r in 0..k {
                    let (apr, aqr) = (at(&a, p, r), at(&a, q, r));
                    if let Some(x) = a.get_mut(p * k + r) {
                        *x = c * apr - sn * aqr;
                    }
                    if let Some(x) = a.get_mut(q * k + r) {
                        *x = sn * apr + c * aqr;
                    }
                }
                for r in 0..k {
                    let (vrp, vrq) = (at(&v, r, p), at(&v, r, q));
                    if let Some(x) = v.get_mut(r * k + p) {
                        *x = c * vrp - sn * vrq;
                    }
                    if let Some(x) = v.get_mut(r * k + q) {
                        *x = sn * vrp + c * vrq;
                    }
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by(|i, j| at(&a, *i, *i).total_cmp(&at(&a, *j, *j)));
    let vals = order.iter().map(|i| at(&a, *i, *i)).collect();
    let mut vecs = vec![0.0; k * k];
    for (col, src) in order.iter().enumerate() {
        for r in 0..k {
            if let Some(x) = vecs.get_mut(r * k + col) {
                *x = at(&v, r, *src);
            }
        }
    }
    (vals, vecs)
}

/// The lowest natural frequencies: `K x = ω² M x` with a lumped mass, solved by LOBPCG on
/// `M^-1/2 K M^-1/2` with a Jacobi preconditioner.
pub fn modes(p: &ModalProblem<'_>) -> Result<Modes, FeaError> {
    if !(p.density.is_finite() && p.density > 0.0) {
        return Err(FeaError::Invalid("the density must be positive".into()));
    }
    if !(1..=MAX_MODES).contains(&p.modes) {
        return Err(FeaError::Invalid(format!("ask for 1 to {MAX_MODES} modes")));
    }
    let md = build(p.mesh, p.voxel, &p.fixed, p.material)?;
    if md.conn.len() > MAX_MODAL_ELEMENTS {
        return Err(FeaError::TooLarge(format!(
            "more than {MAX_MODAL_ELEMENTS} voxels for a frequency analysis: use a larger voxel"
        )));
    }
    let dofs = md.dofs();
    let free: Vec<bool> = md.fixed.iter().map(|f| !f).collect();
    let n_free = free.iter().filter(|f| **f).count();
    let b = (p.modes + 4).min(n_free);
    if b < p.modes {
        return Err(FeaError::Invalid("too few free voxels for that many modes: use a smaller voxel".into()));
    }
    // Lumped mass: an eighth of each voxel's mass on each of its nodes.
    let mut mass = vec![0.0; dofs];
    let me = p.density * md.h * md.h * md.h / 8.0;
    for c in &md.conn {
        for id in c {
            for d in 0..3 {
                if let Some(x) = mass.get_mut(3 * id + d) {
                    *x += me;
                }
            }
        }
    }
    let inv_sqrt_m: Vec<f64> = mass.iter().map(|m| if *m > 0.0 { 1.0 / libm::sqrt(*m) } else { 0.0 }).collect();
    // The preconditioner: `A⁻¹ = M^½ K⁻¹ M^½`, with the multigrid cycle for `K⁻¹`.
    let sqrt_m: Vec<f64> =
        mass.iter().zip(&free).map(|(m, f)| if *f && *m > 0.0 { libm::sqrt(*m) } else { 0.0 }).collect();
    let mg = mg::Hierarchy::new(&md.elems, md.n, &md.used, &md.conn, &md.fixed, md.h, md.material);
    let precondition = |r: &[f64]| -> Vec<f64> {
        let x: Vec<f64> = r.iter().zip(&sqrt_m).map(|(a, b)| a * b).collect();
        mg.apply(&x).iter().zip(&sqrt_m).map(|(a, b)| a * b).collect()
    };
    let mut tmp_in = vec![0.0; dofs];
    let mut tmp_out = vec![0.0; dofs];
    let mut apply = |x: &[f64], y: &mut [f64]| {
        for i in 0..dofs {
            if let Some(t) = tmp_in.get_mut(i) {
                let f = free.get(i).copied().unwrap_or(false);
                *t = if f { x.get(i).copied().unwrap_or(0.0) * inv_sqrt_m.get(i).copied().unwrap_or(0.0) } else { 0.0 };
            }
        }
        md.matvec(&tmp_in, &mut tmp_out);
        for i in 0..dofs {
            if let Some(v) = y.get_mut(i) {
                let f = free.get(i).copied().unwrap_or(false);
                *v = if f {
                    tmp_out.get(i).copied().unwrap_or(0.0) * inv_sqrt_m.get(i).copied().unwrap_or(0.0)
                } else {
                    0.0
                };
            }
        }
    };
    // Preconditioner: the inverse diagonal of the scaled operator.
    let precond: Vec<f64> = (0..dofs)
        .map(|i| {
            let m = mass.get(i).copied().unwrap_or(0.0);
            let k = md.diag.get(i).copied().unwrap_or(1.0);
            if free.get(i).copied().unwrap_or(false) && k > 0.0 { m / k } else { 0.0 }
        })
        .collect();
    // The operator's scale (its largest diagonal entry): residuals are measured against it
    // too, so a (near) zero frequency still converges, and is then reported.
    let scale = precond.iter().filter(|t| **t > 0.0).map(|t| 1.0 / t).fold(0.0f64, f64::max).max(1e-300);
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    // Modified Gram-Schmidt of the columns `s` (with their images `as_`), dropping the ones
    // that are (nearly) dependent.
    let orthonormalize = |s: &mut Vec<Vec<f64>>, as_: &mut Vec<Vec<f64>>| {
        let mut out: (Vec<Vec<f64>>, Vec<Vec<f64>>) = (Vec::new(), Vec::new());
        for (mut v, mut av) in s.drain(..).zip(as_.drain(..)) {
            let n0 = libm::sqrt(dot(&v, &v));
            for (q, aq) in out.0.iter().zip(&out.1) {
                let c = dot(q, &v);
                v.iter_mut().zip(q).for_each(|(x, y)| *x -= c * y);
                av.iter_mut().zip(aq).for_each(|(x, y)| *x -= c * y);
            }
            let n = libm::sqrt(dot(&v, &v));
            if n > 1e-10 * n0.max(1e-300) && n > 1e-300 {
                v.iter_mut().for_each(|x| *x /= n);
                av.iter_mut().for_each(|x| *x /= n);
                out.0.push(v);
                out.1.push(av);
            }
        }
        *s = out.0;
        *as_ = out.1;
    };
    // Deterministic start vectors.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut x: Vec<Vec<f64>> = (0..b)
        .map(|_| {
            (0..dofs)
                .map(|i| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    if free.get(i).copied().unwrap_or(false) {
                        (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    let image = |v: &Vec<f64>, apply: &mut dyn FnMut(&[f64], &mut [f64])| {
        let mut y = vec![0.0; dofs];
        apply(v, &mut y);
        y
    };
    let mut ax: Vec<Vec<f64>> = x.iter().map(|v| image(v, &mut apply)).collect();
    orthonormalize(&mut x, &mut ax);
    let mut pdir: Vec<Vec<f64>> = Vec::new();
    let mut apdir: Vec<Vec<f64>> = Vec::new();
    let mut theta: Vec<f64> = Vec::new();
    let combine = |cols: &[Vec<f64>], c: &[f64], k: usize, j: usize, from: usize| {
        let mut out = vec![0.0; dofs];
        for (r, col) in cols.iter().enumerate().skip(from) {
            let w = c.get(r * k + j).copied().unwrap_or(0.0);
            if w != 0.0 {
                out.iter_mut().zip(col).for_each(|(o, v)| *o += w * v);
            }
        }
        out
    };
    let mut iterations = 0;
    loop {
        // Rayleigh-Ritz on the basis [X, W, P].
        let mut s: Vec<Vec<f64>> = x.clone();
        let mut as_: Vec<Vec<f64>> = ax.clone();
        if !theta.is_empty() {
            let mut converged = true;
            let mut w = Vec::with_capacity(x.len());
            for (j, (xj, axj)) in x.iter().zip(&ax).enumerate() {
                let th = theta.get(j).copied().unwrap_or(0.0);
                let r: Vec<f64> = axj.iter().zip(xj).map(|(a, v)| a - th * v).collect();
                if j < p.modes && libm::sqrt(dot(&r, &r)) > 1e-4 * th.abs().max(1e-10 * scale) {
                    converged = false;
                }
                w.push(precondition(&r));
            }
            if converged {
                break;
            }
            if iterations >= MAX_EIGEN_ITERATIONS {
                return Err(FeaError::NotConverged(
                    "the frequency solver did not converge: fix more faces or use a larger voxel".into(),
                ));
            }
            iterations += 1;
            over_budget(iterations as u64 * CYCLE_PRODUCTS * b as u64 * md.conn.len() as u64)?;
            let aw: Vec<Vec<f64>> = w.iter().map(|v| image(v, &mut apply)).collect();
            s.extend(w);
            as_.extend(aw);
            s.extend(pdir.iter().cloned());
            as_.extend(apdir.iter().cloned());
            // Refresh the tracked images now and then against drift.
            if iterations % 25 == 0 {
                as_ = s.iter().map(|v| image(v, &mut apply)).collect();
            }
        }
        orthonormalize(&mut s, &mut as_);
        let k = s.len();
        let mut h = vec![0.0; k * k];
        for i in 0..k {
            for j in i..k {
                let v = 0.5
                    * (dot(s.get(i).map_or(&[][..], |v| v), as_.get(j).map_or(&[][..], |v| v))
                        + dot(s.get(j).map_or(&[][..], |v| v), as_.get(i).map_or(&[][..], |v| v)));
                if let Some(x) = h.get_mut(i * k + j) {
                    *x = v;
                }
                if let Some(x) = h.get_mut(j * k + i) {
                    *x = v;
                }
            }
        }
        let (vals, vecs) = sym_eig(h, k);
        let keep = b.min(k);
        theta = vals.into_iter().take(keep).collect();
        let nx = x.len().min(k);
        x = (0..keep).map(|j| combine(&s, &vecs, k, j, 0)).collect();
        ax = (0..keep).map(|j| combine(&as_, &vecs, k, j, 0)).collect();
        // The new search directions: the parts of the new vectors outside the old X.
        pdir = (0..keep).map(|j| combine(&s, &vecs, k, j, nx)).collect();
        apdir = (0..keep).map(|j| combine(&as_, &vecs, k, j, nx)).collect();
        if k == nx {
            pdir.clear();
            apdir.clear();
        }
    }
    if theta.iter().take(p.modes).any(|l| *l < 1e-9 * scale) {
        return Err(FeaError::Invalid(
            "the part is free to move (a frequency of zero): fix more faces, or join its parts by faces".into(),
        ));
    }
    let frequencies =
        theta.iter().take(p.modes).map(|l| libm::sqrt(l.max(0.0)) / (2.0 * std::f64::consts::PI)).collect();
    let mut shape = Vec::new();
    if let Some(k) = p.shape.filter(|k| *k < p.modes)
        && let Some(x) = x.get(k)
    {
        // Back from the scaled space: u = M^-1/2 x, then the largest motion to 1.
        let u: Vec<f64> = x.iter().zip(&inv_sqrt_m).map(|(a, b)| a * b).collect();
        let node = |id: usize| [0, 1, 2].map(|d| u.get(3 * id + d).copied().unwrap_or(0.0));
        let big = (0..u.len() / 3).map(|id| libm::sqrt(node(id).iter().map(|v| v * v).sum())).fold(0.0f64, f64::max);
        let n = md.n;
        let pos_of = |gi: usize| {
            let (i, rest) = (gi % (n[0] + 1), gi / (n[0] + 1));
            md.g.node_pos(i, rest % (n[1] + 1), rest / (n[1] + 1))
        };
        if big > 0.0 {
            for (corners, _, _, _) in &md.quads {
                let motion = corners.map(|gi| match md.used.get(gi).copied().flatten() {
                    Some(id) => node(id).map(|v| v / big),
                    None => [0.0; 3],
                });
                shape.push(ShapeQuad { corners: corners.map(pos_of), motion });
            }
        }
    }
    Ok(Modes { elements: md.conn.len(), iterations, frequencies, shape })
}

#[cfg(test)]
mod tests;
