//! Flat-shell static analysis (ADR 0041, "Shell elements"): a mid-surface mesh of 4-node
//! quadrilaterals (possibly folded, e.g. an L-bracket) with six degrees of freedom per node.
//! Each element is flat in its own plane: a bilinear plane-stress membrane with Wilson-Taylor
//! incompatible modes (condensed out), a Mindlin plate with MITC4 assumed transverse shear
//! (Bathe-Dvorkin; no shear locking), and a small drilling stiffness tying the in-plane
//! rotation to the membrane's rotation. Elements are rotated to global axes and assembled
//! into a 6x6-block CSR matrix, solved by Jacobi-preconditioned CG. Triangles are not
//! supported (rejected with an error). Units: mm, N, MPa.

use crate::{FeaError, Material};

/// Most nodes one shell analysis may have.
pub const MAX_SHELL_NODES: usize = 200_000;
const MAX_ITERATIONS: usize = 200_000;
const TOLERANCE: f64 = 1e-10;
/// Transverse-shear correction factor of the Mindlin plate.
const SHEAR_FACTOR: f64 = 5.0 / 6.0;
/// Drilling stiffness as a fraction of `G t A` (penalty on `theta_z - (v,x - u,y)/2`).
const DRILL_ALPHA: f64 = 0.05;

/// A mid-surface mesh in mm. Quads list their nodes in order around the element.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellMesh {
    pub nodes: Vec<[f64; 3]>,
    pub quads: Vec<[u32; 4]>,
    /// Not supported yet: a non-empty list is rejected.
    pub tris: Vec<[u32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellProblem {
    pub mesh: ShellMesh,
    /// Sheet thickness (mm), uniform.
    pub thickness: f64,
    pub material: Material,
    /// Nodes clamped in all 6 degrees of freedom.
    pub fixed: Vec<u32>,
    /// Nodal forces (N) in global axes.
    pub loads: Vec<(u32, [f64; 3])>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellSolution {
    /// Displacement per node (mm).
    pub displacement: Vec<[f64; 3]>,
    /// Rotation per node (rad, global axes).
    pub rotation: Vec<[f64; 3]>,
    pub max_displacement: f64,
    /// Von Mises stress (MPa) at the element centre on the top (+normal) and bottom
    /// surfaces, one entry per quad. Transverse shear is not included.
    pub von_mises_top: Vec<f64>,
    pub von_mises_bottom: Vec<f64>,
    pub max_von_mises: f64,
    pub iterations: usize,
}

// ---- small dense helpers (flat row-major, no indexing) ----

fn g(a: &[f64], i: usize) -> f64 {
    a.get(i).copied().unwrap_or(0.0)
}

fn ad(a: &mut [f64], i: usize, v: f64) {
    if let Some(x) = a.get_mut(i) {
        *x += v;
    }
}

#[derive(Clone)]
struct Dense {
    r: usize,
    c: usize,
    a: Vec<f64>,
}

impl Dense {
    fn zeros(r: usize, c: usize) -> Self {
        Dense { r, c, a: vec![0.0; r * c] }
    }
    fn at(&self, i: usize, j: usize) -> f64 {
        g(&self.a, i * self.c + j)
    }
    fn add(&mut self, i: usize, j: usize, v: f64) {
        ad(&mut self.a, i * self.c + j, v);
    }
    fn set(&mut self, i: usize, j: usize, v: f64) {
        if let Some(x) = self.a.get_mut(i * self.c + j) {
            *x = v;
        }
    }
    fn mul(&self, o: &Dense) -> Dense {
        let mut out = Dense::zeros(self.r, o.c);
        for i in 0..self.r {
            for k in 0..self.c {
                let v = self.at(i, k);
                if v.abs() > 0.0 {
                    for j in 0..o.c {
                        out.add(i, j, v * o.at(k, j));
                    }
                }
            }
        }
        out
    }
    fn t(&self) -> Dense {
        let mut out = Dense::zeros(self.c, self.r);
        for i in 0..self.r {
            for j in 0..self.c {
                out.set(j, i, self.at(i, j));
            }
        }
        out
    }
    /// `self += w * Bᵀ D B`.
    fn add_btdb(&mut self, w: f64, b: &Dense, d: &Dense) {
        let k = b.t().mul(&d.mul(b));
        for (x, y) in self.a.iter_mut().zip(&k.a) {
            *x += w * y;
        }
    }
    fn inverse(&self) -> Option<Dense> {
        let n = self.r;
        let mut m = self.clone();
        let mut inv = Dense::zeros(n, n);
        for i in 0..n {
            inv.set(i, i, 1.0);
        }
        for col in 0..n {
            let mut piv = col;
            for row in col..n {
                if m.at(row, col).abs() > m.at(piv, col).abs() {
                    piv = row;
                }
            }
            let p = m.at(piv, col);
            if p.abs() < 1e-300 {
                return None;
            }
            for j in 0..n {
                let (a, b) = (m.at(col, j), m.at(piv, j));
                m.set(col, j, b);
                m.set(piv, j, a);
                let (a, b) = (inv.at(col, j), inv.at(piv, j));
                inv.set(col, j, b);
                inv.set(piv, j, a);
            }
            for j in 0..n {
                m.set(col, j, m.at(col, j) / p);
                inv.set(col, j, inv.at(col, j) / p);
            }
            for row in 0..n {
                if row != col {
                    let f = m.at(row, col);
                    if f.abs() > 0.0 {
                        for j in 0..n {
                            m.add(row, j, -f * m.at(col, j));
                            inv.add(row, j, -f * inv.at(col, j));
                        }
                    }
                }
            }
        }
        Some(inv)
    }
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    [a0 - b0, a1 - b1, a2 - b2]
}
fn dot(a: V3, b: V3) -> f64 {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    a0 * b0 + a1 * b1 + a2 * b2
}
fn cross(a: V3, b: V3) -> V3 {
    let [a0, a1, a2] = a;
    let [b0, b1, b2] = b;
    [a1 * b2 - a2 * b1, a2 * b0 - a0 * b2, a0 * b1 - a1 * b0]
}
fn unit(a: V3) -> Option<V3> {
    let l = libm::sqrt(dot(a, a));
    (l > 1e-12).then(|| a.map(|x| x / l))
}

const XI: [f64; 4] = [-1.0, 1.0, 1.0, -1.0];
const ETA: [f64; 4] = [-1.0, -1.0, 1.0, 1.0];

/// Shape functions and natural derivatives at (r, s).
fn shape(r: f64, s: f64) -> ([f64; 4], [f64; 4], [f64; 4]) {
    let mut n = [0.0; 4];
    let mut dr = [0.0; 4];
    let mut ds = [0.0; 4];
    for ((&xi, &eta), ((n, dr), ds)) in XI.iter().zip(&ETA).zip(n.iter_mut().zip(dr.iter_mut()).zip(ds.iter_mut())) {
        *n = 0.25 * (1.0 + xi * r) * (1.0 + eta * s);
        *dr = 0.25 * xi * (1.0 + eta * s);
        *ds = 0.25 * eta * (1.0 + xi * r);
    }
    (n, dr, ds)
}

fn comb(d: &[f64; 4], v: &[f64; 4]) -> f64 {
    d.iter().zip(v).map(|(a, b)| a * b).sum::<f64>()
}

/// Geometry at a point of a flat quad with local corner coordinates `xs`, `ys`.
struct Geo {
    n: [f64; 4],
    /// d/dx and d/dy of the shape functions.
    dx: [f64; 4],
    dy: [f64; 4],
    /// Jacobian `[x,r, y,r, x,s, y,s]`.
    j: [f64; 4],
    det: f64,
}

fn geo(r: f64, s: f64, xs: &[f64; 4], ys: &[f64; 4]) -> Option<Geo> {
    let (n, dr, ds) = shape(r, s);
    let (xr, yr, xs_, ys_) = (comb(&dr, xs), comb(&dr, ys), comb(&ds, xs), comb(&ds, ys));
    let det = xr * ys_ - yr * xs_;
    if det <= 1e-12 {
        return None;
    }
    let mut dx = [0.0; 4];
    let mut dy = [0.0; 4];
    for ((a, b), (&p, &q)) in dx.iter_mut().zip(dy.iter_mut()).zip(dr.iter().zip(&ds)) {
        *a = (ys_ * p - yr * q) / det;
        *b = (-xs_ * p + xr * q) / det;
    }
    Some(Geo { n, dx, dy, j: [xr, yr, xs_, ys_], det })
}

fn gauss() -> [(f64, f64); 4] {
    let a = 1.0 / libm::sqrt(3.0);
    [(-a, -a), (a, -a), (a, a), (-a, a)]
}

/// Local element: frame (e1, e2, normal), corner coordinates, 24x24 stiffness.
struct Elem {
    frame: [V3; 3],
    xs: [f64; 4],
    ys: [f64; 4],
    k: Dense,
}

fn frame_of(p: &[V3; 4]) -> Option<([V3; 3], [f64; 4], [f64; 4])> {
    let [p0, p1, p2, p3] = *p;
    let n = unit(cross(sub(p2, p0), sub(p3, p1)))?;
    let d1 = sub(p1, p0);
    let d2 = sub(p2, p3);
    let d = [d1[0] + d2[0], d1[1] + d2[1], d1[2] + d2[2]];
    let along = dot(d, n);
    let e1 = unit([d[0] - along * n[0], d[1] - along * n[1], d[2] - along * n[2]])?;
    let e2 = cross(n, e1);
    let c = [
        p.iter().map(|q| q[0]).sum::<f64>() / 4.0,
        p.iter().map(|q| q[1]).sum::<f64>() / 4.0,
        p.iter().map(|q| q[2]).sum::<f64>() / 4.0,
    ];
    let mut xs = [0.0; 4];
    let mut ys = [0.0; 4];
    for ((x, y), q) in xs.iter_mut().zip(ys.iter_mut()).zip(p) {
        let rel = sub(*q, c);
        *x = dot(rel, e1);
        *y = dot(rel, e2);
    }
    Some(([e1, e2, n], xs, ys))
}

/// Plane-stress matrix (3x3) times `scale`.
fn plane_d(m: &Material, scale: f64) -> Dense {
    let c = m.young / (1.0 - m.poisson * m.poisson) * scale;
    let mut d = Dense::zeros(3, 3);
    d.set(0, 0, c);
    d.set(1, 1, c);
    d.set(0, 1, c * m.poisson);
    d.set(1, 0, c * m.poisson);
    d.set(2, 2, c * (1.0 - m.poisson) / 2.0);
    d
}

/// Membrane (Q4 + Wilson-Taylor modes, condensed) -> 8x8, in dofs (u0,v0,...,u3,v3).
fn membrane(xs: &[f64; 4], ys: &[f64; 4], m: &Material, t: f64) -> Option<Dense> {
    let g0 = geo(0.0, 0.0, xs, ys)?;
    let det0 = g0.det;
    let [xr, yr, xs_, ys_] = g0.j;
    // J0^-1 (row-major), mapping natural derivatives to physical ones.
    let inv0 = [ys_ / det0, -yr / det0, -xs_ / det0, xr / det0];
    let dm = plane_d(m, t);
    let mut k = Dense::zeros(12, 12);
    for (r, s) in gauss() {
        let ge = geo(r, s, xs, ys)?;
        let mut b = Dense::zeros(3, 12);
        for i in 0..4 {
            let (dx, dy) = (g(&ge.dx, i), g(&ge.dy, i));
            b.set(0, 2 * i, dx);
            b.set(1, 2 * i + 1, dy);
            b.set(2, 2 * i, dy);
            b.set(2, 2 * i + 1, dx);
        }
        let sc = det0 / ge.det;
        // Modes: (xi^2-1) and (eta^2-1) for u, then for v.
        for mode in 0..4 {
            let (dxi, deta) = if mode % 2 == 0 { (2.0 * r, 0.0) } else { (0.0, 2.0 * s) };
            let dx = sc * (g(&inv0, 0) * dxi + g(&inv0, 1) * deta);
            let dy = sc * (g(&inv0, 2) * dxi + g(&inv0, 3) * deta);
            if mode < 2 {
                b.set(0, 8 + mode, dx);
                b.set(2, 8 + mode, dy);
            } else {
                b.set(1, 8 + mode, dy);
                b.set(2, 8 + mode, dx);
            }
        }
        k.add_btdb(ge.det, &b, &dm);
    }
    let mut kuu = Dense::zeros(8, 8);
    let mut kua = Dense::zeros(8, 4);
    let mut kaa = Dense::zeros(4, 4);
    for i in 0..12 {
        for j in 0..12 {
            let v = k.at(i, j);
            match (i < 8, j < 8) {
                (true, true) => kuu.set(i, j, v),
                (true, false) => kua.set(i, j - 8, v),
                (false, false) => kaa.set(i - 8, j - 8, v),
                _ => {}
            }
        }
    }
    let corr = kua.mul(&kaa.inverse()?).mul(&kua.t());
    for (x, y) in kuu.a.iter_mut().zip(&corr.a) {
        *x -= y;
    }
    Some(kuu)
}

/// Natural shear rows (gamma_r, gamma_s) of the Mindlin plate at (r, s), 2x12, dofs
/// (w, bx, by) per node, before the MITC interpolation.
fn shear_nat(r: f64, s: f64, xs: &[f64; 4], ys: &[f64; 4]) -> Dense {
    let (n, dr, ds) = shape(r, s);
    let (xr, yr, xs_, ys_) = (comb(&dr, xs), comb(&dr, ys), comb(&ds, xs), comb(&ds, ys));
    let mut b = Dense::zeros(2, 12);
    for i in 0..4 {
        let ni = g(&n, i);
        b.set(0, 3 * i, g(&dr, i));
        b.set(0, 3 * i + 1, ni * xr);
        b.set(0, 3 * i + 2, ni * yr);
        b.set(1, 3 * i, g(&ds, i));
        b.set(1, 3 * i + 1, ni * xs_);
        b.set(1, 3 * i + 2, ni * ys_);
    }
    b
}

/// Mindlin plate (MITC4) -> 12x12 in dofs (w, bx, by) per node, `bx = theta_y`,
/// `by = -theta_x`.
fn plate(xs: &[f64; 4], ys: &[f64; 4], m: &Material, t: f64) -> Option<Dense> {
    let db = plane_d(m, t * t * t / 12.0);
    let gs = m.young / (2.0 * (1.0 + m.poisson)) * SHEAR_FACTOR * t;
    let mut ds = Dense::zeros(2, 2);
    ds.set(0, 0, gs);
    ds.set(1, 1, gs);
    // Tying rows: gamma_r at (0, +-1), gamma_s at (+-1, 0).
    let ra = shear_nat(0.0, 1.0, xs, ys);
    let rc = shear_nat(0.0, -1.0, xs, ys);
    let sd = shear_nat(1.0, 0.0, xs, ys);
    let sb = shear_nat(-1.0, 0.0, xs, ys);
    let mut k = Dense::zeros(12, 12);
    for (r, s) in gauss() {
        let ge = geo(r, s, xs, ys)?;
        let mut bb = Dense::zeros(3, 12);
        for i in 0..4 {
            let (dx, dy) = (g(&ge.dx, i), g(&ge.dy, i));
            bb.set(0, 3 * i + 1, dx);
            bb.set(1, 3 * i + 2, dy);
            bb.set(2, 3 * i + 1, dy);
            bb.set(2, 3 * i + 2, dx);
        }
        k.add_btdb(ge.det, &bb, &db);
        // MITC4 natural rows, then to physical via J^-1.
        let [xr, yr, xs_, ys_] = ge.j;
        let mut bs = Dense::zeros(2, 12);
        for c in 0..12 {
            let a = 0.5 * (1.0 + s) * ra.at(0, c) + 0.5 * (1.0 - s) * rc.at(0, c);
            let b = 0.5 * (1.0 + r) * sd.at(1, c) + 0.5 * (1.0 - r) * sb.at(1, c);
            bs.set(0, c, (ys_ * a - yr * b) / ge.det);
            bs.set(1, c, (-xs_ * a + xr * b) / ge.det);
        }
        k.add_btdb(ge.det, &bs, &ds);
    }
    Some(k)
}

/// Local 24x24 stiffness: dof `k` of node `i` is `6 i + k`, k = (u, v, w, tx, ty, tz).
fn element(p: &[V3; 4], m: &Material, t: f64) -> Option<Elem> {
    let (frame, xs, ys) = frame_of(p)?;
    let mem = membrane(&xs, &ys, m, t)?;
    let pl = plate(&xs, &ys, m, t)?;
    let mut k = Dense::zeros(24, 24);
    let mmap = |a: usize| 6 * (a / 2) + a % 2;
    for a in 0..8 {
        for b in 0..8 {
            k.add(mmap(a), mmap(b), mem.at(a, b));
        }
    }
    // Plate: (w, bx = ty, by = -tx).
    let pmap = |a: usize| -> (usize, f64) {
        let (node, c) = (a / 3, a % 3);
        match c {
            0 => (6 * node + 2, 1.0),
            1 => (6 * node + 4, 1.0),
            _ => (6 * node + 3, -1.0),
        }
    };
    for a in 0..12 {
        for b in 0..12 {
            let ((ia, sa), (ib, sb)) = (pmap(a), pmap(b));
            k.add(ia, ib, sa * sb * pl.at(a, b));
        }
    }
    // Drilling: kd * (theta_z - (v,x - u,y) / 2)^2.
    let kd = DRILL_ALPHA * m.young / (2.0 * (1.0 + m.poisson)) * t;
    for (r, s) in gauss() {
        let ge = geo(r, s, &xs, &ys)?;
        let mut row = [0.0; 24];
        for i in 0..4 {
            let (n, dx, dy) = (g(&ge.n, i), g(&ge.dx, i), g(&ge.dy, i));
            ad(&mut row, 6 * i, 0.5 * dy);
            ad(&mut row, 6 * i + 1, -0.5 * dx);
            ad(&mut row, 6 * i + 5, n);
        }
        for (a, &ra) in row.iter().enumerate() {
            if ra.abs() > 0.0 {
                for (b, &rb) in row.iter().enumerate() {
                    k.add(a, b, kd * ge.det * ra * rb);
                }
            }
        }
    }
    Some(Elem { frame, xs, ys, k })
}

/// `R M Rᵀ` for a 3x3 block `m` (flat), R having the frame vectors as columns.
fn rotate(frame: &[V3; 3], m: &[f64; 9]) -> [f64; 9] {
    let mut out = [0.0; 9];
    for (b, fb) in frame.iter().enumerate() {
        for (d, fd) in frame.iter().enumerate() {
            let v = g(m, 3 * b + d);
            for (a, &ua) in fb.iter().enumerate() {
                for (c, &vc) in fd.iter().enumerate() {
                    ad(&mut out, 3 * a + c, v * ua * vc);
                }
            }
        }
    }
    out
}

/// Block-CSR matrix of 6x6 node blocks.
struct Bsr {
    ptr: Vec<usize>,
    cols: Vec<u32>,
    vals: Vec<f64>,
}

impl Bsr {
    fn pattern(n: usize, quads: &[[u32; 4]]) -> Bsr {
        let mut adj: Vec<Vec<u32>> = vec![Vec::new(); n];
        for q in quads {
            for &a in q {
                if let Some(row) = adj.get_mut(a as usize) {
                    row.extend_from_slice(q);
                }
            }
        }
        let mut ptr = vec![0usize];
        let mut cols = Vec::new();
        for row in &mut adj {
            row.sort_unstable();
            row.dedup();
            cols.extend_from_slice(row);
            ptr.push(cols.len());
        }
        let vals = vec![0.0; cols.len() * 36];
        Bsr { ptr, cols, vals }
    }
    fn block_index(&self, i: usize, j: u32) -> Option<usize> {
        let (a, b) = (*self.ptr.get(i)?, *self.ptr.get(i + 1)?);
        let row = self.cols.get(a..b)?;
        row.binary_search(&j).ok().map(|k| a + k)
    }
    fn add_block(&mut self, i: usize, j: u32, f: impl Fn(usize, usize) -> f64) {
        let Some(bi) = self.block_index(i, j) else { return };
        if let Some(blk) = self.vals.get_mut(bi * 36..bi * 36 + 36) {
            for (k, x) in blk.iter_mut().enumerate() {
                *x += f(k / 6, k % 6);
            }
        }
    }
    fn matvec(&self, x: &[f64], y: &mut [f64]) {
        for (i, yi) in y.chunks_mut(6).enumerate() {
            let (Some(&a), Some(&b)) = (self.ptr.get(i), self.ptr.get(i + 1)) else { continue };
            let mut acc = [0.0; 6];
            for (bi, &j) in (a..b).zip(self.cols.get(a..b).unwrap_or(&[])) {
                let (Some(blk), Some(xj)) =
                    (self.vals.get(bi * 36..bi * 36 + 36), x.get(6 * j as usize..6 * j as usize + 6))
                else {
                    continue;
                };
                for (row, out) in blk.chunks(6).zip(acc.iter_mut()) {
                    *out += row.iter().zip(xj).map(|(a, b)| a * b).sum::<f64>();
                }
            }
            for (o, a) in yi.iter_mut().zip(acc) {
                *o = a;
            }
        }
    }
}

fn von_mises(sx: f64, sy: f64, txy: f64) -> f64 {
    libm::sqrt((sx * sx - sx * sy + sy * sy + 3.0 * txy * txy).max(0.0))
}

fn dot_n(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Solve a flat-shell problem. Triangles are rejected.
pub fn shell_static(p: &ShellProblem) -> Result<ShellSolution, FeaError> {
    let mesh = &p.mesh;
    let n = mesh.nodes.len();
    if !mesh.tris.is_empty() {
        return Err(FeaError::Invalid("shell triangles are not supported yet: use quads".into()));
    }
    if mesh.quads.is_empty() {
        return Err(FeaError::Invalid("the shell mesh has no elements".into()));
    }
    if n > MAX_SHELL_NODES {
        return Err(FeaError::TooLarge(format!(
            "{n} shell nodes; the limit is {MAX_SHELL_NODES}. Use a coarser mesh."
        )));
    }
    if !(p.thickness.is_finite() && p.thickness > 0.0) {
        return Err(FeaError::Invalid("the shell thickness must be positive".into()));
    }
    let nu = p.material.poisson;
    if !(p.material.young > 0.0 && nu > -1.0 && nu < 0.5) {
        return Err(FeaError::Invalid("the material needs E > 0 and -1 < nu < 0.5".into()));
    }
    if mesh.nodes.iter().any(|q| q.iter().any(|c| !c.is_finite())) {
        return Err(FeaError::Invalid("a shell node has a non-finite coordinate".into()));
    }
    let mut used = vec![false; n];
    for q in &mesh.quads {
        for &i in q {
            match used.get_mut(i as usize) {
                Some(u) => *u = true,
                None => return Err(FeaError::Invalid(format!("a quad refers to node {i}, which does not exist"))),
            }
        }
    }
    let mut free = vec![true; 6 * n];
    for (i, u) in used.iter().enumerate() {
        if !u {
            for f in free.iter_mut().skip(6 * i).take(6) {
                *f = false;
            }
        }
    }
    if p.fixed.is_empty() {
        return Err(FeaError::Invalid("nothing is fixed: the shell would be free to move".into()));
    }
    for &i in &p.fixed {
        if !used.get(i as usize).copied().unwrap_or(false) {
            return Err(FeaError::Invalid(format!("fixed node {i} is not part of an element")));
        }
        for f in free.iter_mut().skip(6 * i as usize).take(6) {
            *f = false;
        }
    }
    let mut b = vec![0.0; 6 * n];
    for (i, f) in &p.loads {
        if !used.get(*i as usize).copied().unwrap_or(false) {
            return Err(FeaError::Invalid(format!("loaded node {i} is not part of an element")));
        }
        for (k, &c) in f.iter().enumerate() {
            ad(&mut b, 6 * *i as usize + k, c);
        }
    }

    // Assembly.
    let mut kmat = Bsr::pattern(n, &mesh.quads);
    let mut elems = Vec::with_capacity(mesh.quads.len());
    for (qi, q) in mesh.quads.iter().enumerate() {
        let pos = |k: usize| -> V3 { q.get(k).and_then(|&i| mesh.nodes.get(i as usize)).copied().unwrap_or([0.0; 3]) };
        let pts = [pos(0), pos(1), pos(2), pos(3)];
        let e = element(&pts, &p.material, p.thickness)
            .ok_or_else(|| FeaError::Invalid(format!("shell element {qi} is degenerate, inverted or too warped")))?;
        for (a, &na) in q.iter().enumerate() {
            for (c, &nc) in q.iter().enumerate() {
                // Node block (a, c): four 3x3 parts (translation/rotation), rotated to global.
                let mut rot = Vec::with_capacity(4);
                for pi in 0..4 {
                    let (r0, c0) = (6 * a + 3 * (pi / 2), 6 * c + 3 * (pi % 2));
                    let mut part = [0.0; 9];
                    for r in 0..3 {
                        for cc in 0..3 {
                            ad(&mut part, 3 * r + cc, e.k.at(r0 + r, c0 + cc));
                        }
                    }
                    rot.push(rotate(&e.frame, &part));
                }
                kmat.add_block(na as usize, nc, |r, cc| {
                    rot.get(2 * (r / 3) + cc / 3).map_or(0.0, |m| g(m, 3 * (r % 3) + cc % 3))
                });
            }
        }
        elems.push(e);
    }

    // Jacobi-preconditioned CG on the free dofs (fixed and unused dofs stay zero).
    let mut diag = vec![1.0; 6 * n];
    for i in 0..n {
        if let Some(bi) = kmat.block_index(i, i as u32) {
            for k in 0..6 {
                let d = g(&kmat.vals, bi * 36 + 7 * k);
                if let Some(x) = diag.get_mut(6 * i + k) {
                    *x = if d > 0.0 { d } else { 1.0 };
                }
            }
        }
    }
    let mask = |v: &mut [f64]| {
        for (x, &f) in v.iter_mut().zip(&free) {
            if !f {
                *x = 0.0;
            }
        }
    };
    mask(&mut b);
    let bnorm = libm::sqrt(dot_n(&b, &b));
    let mut u = vec![0.0; 6 * n];
    let mut iterations = 0;
    if bnorm > 0.0 {
        let mut r = b.clone();
        let mut z: Vec<f64> = r.iter().zip(&diag).map(|(a, d)| a / d).collect();
        let mut dir = z.clone();
        let mut rz = dot_n(&r, &z);
        let mut kd = vec![0.0; 6 * n];
        let mut converged = false;
        while iterations < MAX_ITERATIONS {
            kmat.matvec(&dir, &mut kd);
            mask(&mut kd);
            let denom = dot_n(&dir, &kd);
            if denom <= 0.0 {
                return Err(FeaError::NotConverged("the shell is free to move or its stiffness is singular".into()));
            }
            let alpha = rz / denom;
            for (x, d) in u.iter_mut().zip(&dir) {
                *x += alpha * d;
            }
            for (x, d) in r.iter_mut().zip(&kd) {
                *x -= alpha * d;
            }
            iterations += 1;
            if libm::sqrt(dot_n(&r, &r)) <= TOLERANCE * bnorm {
                converged = true;
                break;
            }
            for ((zz, rr), d) in z.iter_mut().zip(&r).zip(&diag) {
                *zz = rr / d;
            }
            let rz_new = dot_n(&r, &z);
            let beta = rz_new / rz;
            rz = rz_new;
            for (d, zz) in dir.iter_mut().zip(&z) {
                *d = zz + beta * *d;
            }
        }
        if !converged {
            return Err(FeaError::NotConverged(format!(
                "the shell solver did not converge in {MAX_ITERATIONS} iterations (the model may be free to move)"
            )));
        }
    }

    // Results.
    let mut displacement = Vec::with_capacity(n);
    let mut rotation = Vec::with_capacity(n);
    for c in u.chunks(6) {
        displacement.push([g(c, 0), g(c, 1), g(c, 2)]);
        rotation.push([g(c, 3), g(c, 4), g(c, 5)]);
    }
    let max_displacement = displacement.iter().map(|d| libm::sqrt(dot(*d, *d))).fold(0.0, f64::max);
    let (mut top, mut bottom) = (Vec::new(), Vec::new());
    let t = p.thickness;
    let dp = plane_d(&p.material, 1.0);
    for (q, e) in mesh.quads.iter().zip(&elems) {
        // Local nodal displacements.
        let mut ul = [0.0; 24];
        for (a, &na) in q.iter().enumerate() {
            let ug = [
                displacement.get(na as usize).copied().unwrap_or([0.0; 3]),
                rotation.get(na as usize).copied().unwrap_or([0.0; 3]),
            ];
            for (part, v) in ug.iter().enumerate() {
                for (k, ax) in e.frame.iter().enumerate() {
                    ad(&mut ul, 6 * a + 3 * part + k, dot(*v, *ax));
                }
            }
        }
        let Some(gc) = geo(0.0, 0.0, &e.xs, &e.ys) else {
            top.push(0.0);
            bottom.push(0.0);
            continue;
        };
        let mut eps = [0.0; 3];
        let mut kap = [0.0; 3];
        for i in 0..4 {
            let (dx, dy) = (g(&gc.dx, i), g(&gc.dy, i));
            let (u0, v0) = (g(&ul, 6 * i), g(&ul, 6 * i + 1));
            let (bx, by) = (g(&ul, 6 * i + 4), -g(&ul, 6 * i + 3));
            ad(&mut eps, 0, dx * u0);
            ad(&mut eps, 1, dy * v0);
            ad(&mut eps, 2, dy * u0 + dx * v0);
            ad(&mut kap, 0, dx * bx);
            ad(&mut kap, 1, dy * by);
            ad(&mut kap, 2, dy * bx + dx * by);
        }
        let stress = |zf: f64| {
            let s: Vec<f64> =
                (0..3).map(|r| (0..3).map(|c| dp.at(r, c) * (g(&eps, c) + zf * g(&kap, c))).sum::<f64>()).collect();
            von_mises(g(&s, 0), g(&s, 1), g(&s, 2))
        };
        top.push(stress(0.5 * t));
        bottom.push(stress(-0.5 * t));
    }
    let max_von_mises = top.iter().chain(&bottom).copied().fold(0.0, f64::max);
    Ok(ShellSolution {
        displacement,
        rotation,
        max_displacement,
        von_mises_top: top,
        von_mises_bottom: bottom,
        max_von_mises,
        iterations,
    })
}

#[cfg(test)]
mod tests;
