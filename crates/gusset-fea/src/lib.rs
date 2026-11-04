//! Linear static analysis on a voxel grid (ADR 0041): a solid's closed triangle mesh is
//! filled with cubic voxels (a voxel is in when its centre is, by ray parity along X), each
//! voxel is an 8-node hexahedron (isotropic linear elasticity, 2×2×2 Gauss points), and the
//! system is solved matrix-free by conjugate gradients with a Jacobi preconditioner.
//! Fixed faces and face loads are given by the mesh's face index (its triangle range); they
//! act on the nodes of the voxel faces lying on that face. Units: mm, N, MPa.

use gusset_kernel::Mesh;

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
    let tris: Vec<[V3; 3]> = (r.start as usize..r.end as usize).filter_map(|t| tri(mesh, t)).collect();
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

/// Solves the problem.
pub fn solve(p: &Problem<'_>) -> Result<Solution, FeaError> {
    let bad = |m: &str| Err(FeaError::Invalid(m.to_string()));
    let m = p.material;
    if !(m.young.is_finite() && m.young > 0.0 && m.poisson.is_finite() && m.poisson > -0.99 && m.poisson < 0.49) {
        return bad("the material needs a positive Young's modulus and a Poisson ratio below 0.49");
    }
    if !(p.voxel.is_finite() && p.voxel > 0.0) {
        return bad("the voxel size must be a positive length");
    }
    if p.fixed.is_empty() {
        return bad("fix at least one face, or the part is free to move");
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for q in &p.mesh.positions {
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
    let h = p.voxel;
    let n = [0, 1, 2].map(|d| {
        let span = hi.get(d).copied().unwrap_or(0.0) - lo.get(d).copied().unwrap_or(0.0);
        ((span / h - 1e-9).ceil().max(1.0)) as usize
    });
    let cells = n.iter().try_fold(1usize, |a, v| a.checked_mul(*v));
    if cells.is_none_or(|c| c > MAX_ELEMENTS * 8) {
        return Err(FeaError::TooLarge("the voxel grid is too fine for the body: use a larger voxel".into()));
    }
    let g = Grid { origin: lo, h, n };
    let inside = voxelize(p.mesh, &g);
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
    for f in &p.fixed {
        for (id, _) in face_nodes(p.mesh, *f, &quads, &g, &used)? {
            fixed_nodes += 1;
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
    let mut f = vec![0.0; dofs];
    let mut loaded_nodes = 0;
    for l in &p.loads {
        if !l.force.iter().all(|v| v.is_finite()) {
            return bad("a load is not a number");
        }
        let ids = face_nodes(p.mesh, l.face, &quads, &g, &used)?;
        if ids.is_empty() {
            return bad("a loaded face reaches no voxel: use a smaller voxel");
        }
        loaded_nodes += ids.len();
        let total: usize = ids.iter().map(|(_, c)| c).sum();
        for (id, count) in ids {
            let share = count as f64 / total.max(1) as f64;
            for d in 0..3 {
                if let Some(x) = f.get_mut(3 * id + d) {
                    *x += l.force.get(d).copied().unwrap_or(0.0) * share;
                }
            }
        }
    }
    let (ke, b0) = element(h, &m);
    let matvec = |u: &[f64], y: &mut [f64]| {
        y.iter_mut().for_each(|v| *v = 0.0);
        for c in &conn {
            let mut ue = [0.0; 24];
            for (a, id) in c.iter().enumerate() {
                for d in 0..3 {
                    if let Some(x) = ue.get_mut(3 * a + d) {
                        *x = u.get(3 * id + d).copied().unwrap_or(0.0);
                    }
                }
            }
            for (a, id) in c.iter().enumerate() {
                for d in 0..3 {
                    let row = 3 * a + d;
                    let mut acc = 0.0;
                    for (col, uv) in ue.iter().enumerate() {
                        acc += ke.get(row * 24 + col).copied().unwrap_or(0.0) * uv;
                    }
                    if let Some(x) = y.get_mut(3 * id + d) {
                        *x += acc;
                    }
                }
            }
        }
        for (i, x) in y.iter_mut().enumerate() {
            if fixed.get(i).copied().unwrap_or(false) {
                *x = u.get(i).copied().unwrap_or(0.0);
            }
        }
    };
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
    for (i, x) in f.iter_mut().enumerate() {
        if fixed.get(i).copied().unwrap_or(false) {
            *x = 0.0;
        }
    }
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut u = vec![0.0; dofs];
    let mut r = f.clone();
    let mut z: Vec<f64> = r.iter().zip(&diag).map(|(r, d)| r / d).collect();
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
        for (zi, (ri, di)) in z.iter_mut().zip(r.iter().zip(&diag)) {
            *zi = ri / di;
        }
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for (pi, zi) in pdir.iter_mut().zip(&z) {
            *pi = zi + beta * *pi;
        }
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
    for (c, [i, j, k]) in conn.iter().zip(&elems) {
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
        surface,
    })
}

#[cfg(test)]
mod tests;
