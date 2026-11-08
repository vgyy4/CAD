//! A geometric multigrid V-cycle on the voxel grids (ADR 0041 amendment), used as the
//! preconditioner of both solvers in place of plain Jacobi.
//!
//! Each coarser level is made of the voxels of twice the edge that cover the finer ones,
//! with its own element stiffness (rediscretised, not Galerkin). Grid nodes of a coarse
//! level sit on every other fine node, so the prolongation is trilinear: a fine node takes
//! its coincident coarse node, or the mean of the two, four or eight around it. Restriction
//! is its transpose. Smoothing is weighted Jacobi, the weight from a short power iteration,
//! with as many sweeps after the coarse correction as before, so the cycle is a symmetric
//! positive operator, as conjugate gradients and LOBPCG need. Held degrees of freedom are
//! zero on every level.

use super::{CORNERS, Material, element};

/// Jacobi sweeps before and after the coarse correction, and on the coarsest level.
const SWEEPS: usize = 1;
const COARSEST_SWEEPS: usize = 40;
/// A level this small (voxels) is the coarsest.
const COARSEST: usize = 64;
const MAX_LEVELS: usize = 10;

struct Level {
    conn: Vec<[usize; 8]>,
    /// Per element: its stiffness scale (the filled share of a coarse voxel's children).
    scale: Vec<f64>,
    /// Per degree of freedom.
    fixed: Vec<bool>,
    ke: Vec<f64>,
    /// The smoother: weight over the diagonal, zero where held.
    smooth: Vec<f64>,
    /// For each node: the nodes of the next coarser level and their weights.
    prolong: Vec<Vec<(usize, f64)>>,
}

impl Level {
    fn dofs(&self) -> usize {
        self.fixed.len()
    }

    /// `y = K u`, held rows passed through.
    fn matvec(&self, u: &[f64], y: &mut [f64]) {
        product(&self.conn, &self.scale, &self.ke, &self.fixed, u, y);
    }
}

/// `y = K u` element by element (element `e` scaled by `scale[e]`, 1 where missing), held
/// rows passed through.
pub(crate) fn product(conn: &[[usize; 8]], scale: &[f64], ke: &[f64], fixed: &[bool], u: &[f64], y: &mut [f64]) {
    y.fill(0.0);
    for (e, c) in conn.iter().enumerate() {
        let sc = scale.get(e).copied().unwrap_or(1.0);
        let mut ue = [0.0; 24];
        for (chunk, id) in ue.chunks_exact_mut(3).zip(c) {
            if let Some(src) = u.get(3 * id..3 * id + 3) {
                chunk.copy_from_slice(src);
            }
        }
        let mut ye = [0.0; 24];
        for (yv, krow) in ye.iter_mut().zip(ke.chunks_exact(24)) {
            // Four partial sums: independent chains the processor overlaps.
            let mut acc = [0.0; 4];
            for (k4, u4) in krow.chunks_exact(4).zip(ue.chunks_exact(4)) {
                for ((a, k), v) in acc.iter_mut().zip(k4).zip(u4) {
                    *a += k * v;
                }
            }
            *yv = (acc[0] + acc[1]) + (acc[2] + acc[3]);
        }
        for (chunk, id) in ye.chunks_exact(3).zip(c) {
            if let Some(dst) = y.get_mut(3 * id..3 * id + 3) {
                for (d, v) in dst.iter_mut().zip(chunk) {
                    *d += sc * v;
                }
            }
        }
    }
    for ((yv, uv), f) in y.iter_mut().zip(u).zip(fixed) {
        if *f {
            *yv = *uv;
        }
    }
}

pub(crate) struct Hierarchy {
    levels: Vec<Level>,
}

/// The diagonal of the assembled stiffness (per degree of freedom).
fn diagonal(conn: &[[usize; 8]], scale: &[f64], ke: &[f64], dofs: usize) -> Vec<f64> {
    let mut diag = vec![0.0; dofs];
    for (e, c) in conn.iter().enumerate() {
        let sc = scale.get(e).copied().unwrap_or(1.0);
        for (a, id) in c.iter().enumerate() {
            for d in 0..3 {
                if let Some(x) = diag.get_mut(3 * id + d) {
                    *x += sc * ke.get((3 * a + d) * 25).copied().unwrap_or(0.0);
                }
            }
        }
    }
    diag
}

/// The smoother weights: `ω / diag` with `ω` just under `1 / λmax(D⁻¹K)` (a power
/// iteration), zero on held degrees of freedom.
fn smoother(conn: &[[usize; 8]], scale: &[f64], ke: &[f64], fixed: &[bool]) -> Vec<f64> {
    let dofs = fixed.len();
    let diag = diagonal(conn, scale, ke, dofs);
    let inv: Vec<f64> = diag.iter().zip(fixed).map(|(d, f)| if *f || *d <= 0.0 { 0.0 } else { 1.0 / d }).collect();
    let probe = Level {
        conn: conn.to_vec(),
        scale: scale.to_vec(),
        fixed: fixed.to_vec(),
        ke: ke.to_vec(),
        smooth: Vec::new(),
        prolong: Vec::new(),
    };
    let mut v: Vec<f64> = (0..dofs)
        .map(|i| if inv.get(i).copied().unwrap_or(0.0) > 0.0 { 1.0 + (i % 7) as f64 * 0.1 } else { 0.0 })
        .collect();
    let mut y = vec![0.0; dofs];
    let mut lambda = 1.0;
    for _ in 0..15 {
        let n = libm::sqrt(v.iter().map(|x| x * x).sum::<f64>());
        if n <= 0.0 {
            break;
        }
        v.iter_mut().for_each(|x| *x /= n);
        probe.matvec(&v, &mut y);
        for (yi, ii) in y.iter_mut().zip(&inv) {
            *yi *= ii;
        }
        lambda = v.iter().zip(&y).map(|(a, b)| a * b).sum::<f64>();
        std::mem::swap(&mut v, &mut y);
    }
    // The estimate is from below: keep a margin.
    let omega = 0.9 / lambda.max(1e-12);
    inv.iter().map(|i| omega * i).collect()
}

impl Hierarchy {
    /// The levels below the fine model: `cells` its voxels (grid order), `n` its grid,
    /// `node_ids` its node numbering over the grid's nodes, `fixed` per degree of freedom.
    pub(crate) fn new(
        cells: &[[usize; 3]],
        n: [usize; 3],
        node_ids: &[Option<usize>],
        conn: &[[usize; 8]],
        fixed: &[bool],
        h: f64,
        m: Material,
    ) -> Hierarchy {
        Hierarchy::new_scaled(cells, n, node_ids, conn, fixed, h, m, &[])
    }

    /// As [`Hierarchy::new`], with element `e`'s stiffness scaled by `scale[e]` (1 where
    /// missing): a density-weighted model (topology optimisation).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_scaled(
        cells: &[[usize; 3]],
        n: [usize; 3],
        node_ids: &[Option<usize>],
        conn: &[[usize; 8]],
        fixed: &[bool],
        h: f64,
        m: Material,
        scale: &[f64],
    ) -> Hierarchy {
        let (ke, _) = element(h, &m);
        let fine: Vec<f64> = (0..conn.len()).map(|e| scale.get(e).copied().unwrap_or(1.0)).collect();
        let mut levels = vec![Level {
            conn: conn.to_vec(),
            fixed: fixed.to_vec(),
            smooth: smoother(conn, &fine, &ke, fixed),
            scale: fine,
            ke,
            prolong: Vec::new(),
        }];
        let (mut cells, mut n, mut ids, mut h) = (cells.to_vec(), n, node_ids.to_vec(), h);
        while cells.len() > COARSEST && levels.len() < MAX_LEVELS && n.iter().any(|v| *v > 1) {
            let nc = n.map(|v| v.div_ceil(2));
            let lin = |[i, j, k]: [usize; 3]| i + nc[0] * (j + nc[1] * k);
            // Each coarse voxel's stiffness is scaled by its filled share of the fine ones
            // (scaled again by the finer level's own shares).
            let fine_scale = levels.last().map(|l| l.scale.clone()).unwrap_or_default();
            let mut child: Vec<([usize; 3], f64)> = cells
                .iter()
                .enumerate()
                .map(|(e, c)| (c.map(|v| v / 2), fine_scale.get(e).copied().unwrap_or(1.0)))
                .collect();
            child.sort_by_key(|(c, _)| lin(*c));
            let mut cc: Vec<[usize; 3]> = Vec::new();
            let mut cscale: Vec<f64> = Vec::new();
            for (c, w) in child {
                if cc.last() == Some(&c) {
                    if let Some(x) = cscale.last_mut() {
                        *x += w / 8.0;
                    }
                } else {
                    cc.push(c);
                    cscale.push(w / 8.0);
                }
            }
            let nidx = |[i, j, k]: [usize; 3]| i + (nc[0] + 1) * (j + (nc[1] + 1) * k);
            let mut cids: Vec<Option<usize>> = vec![None; (nc[0] + 1) * (nc[1] + 1) * (nc[2] + 1)];
            let mut grid_of: Vec<[usize; 3]> = Vec::new();
            let mut cconn = Vec::with_capacity(cc.len());
            for [i, j, k] in &cc {
                let mut c = [0usize; 8];
                for (slot, off) in c.iter_mut().zip(CORNERS) {
                    let g = [i + off[0], j + off[1], k + off[2]];
                    let gi = nidx(g);
                    *slot = match cids.get(gi).copied().flatten() {
                        Some(id) => id,
                        None => {
                            let id = grid_of.len();
                            grid_of.push(g);
                            if let Some(x) = cids.get_mut(gi) {
                                *x = Some(id);
                            }
                            id
                        }
                    };
                }
                cconn.push(c);
            }
            // A coarse node is held where the fine node on it is.
            let fine_idx = |[i, j, k]: [usize; 3]| i + (n[0] + 1) * (j + (n[1] + 1) * k);
            let fine = levels.last().map(|l| &l.fixed);
            let mut cfixed = vec![false; 3 * grid_of.len()];
            for (id, g) in grid_of.iter().enumerate() {
                let f = g.map(|v| 2 * v);
                if f.iter().zip(n).all(|(v, nn)| *v <= nn)
                    && let Some(Some(fid)) = ids.get(fine_idx(f))
                    && fine.is_some_and(|fx| fx.get(3 * fid).copied().unwrap_or(false))
                {
                    for d in 0..3 {
                        if let Some(x) = cfixed.get_mut(3 * id + d) {
                            *x = true;
                        }
                    }
                }
            }
            // Trilinear prolongation from the coarse nodes to every fine node.
            let fine_nodes = levels.last().map_or(0, |l| l.dofs() / 3);
            let mut prolong = vec![Vec::new(); fine_nodes];
            for k in 0..=n[2] {
                for j in 0..=n[1] {
                    for i in 0..=n[0] {
                        let Some(Some(fid)) = ids.get(fine_idx([i, j, k])) else { continue };
                        let axis = |v: usize| -> Vec<(usize, f64)> {
                            if v.is_multiple_of(2) {
                                vec![(v / 2, 1.0)]
                            } else {
                                vec![((v - 1) / 2, 0.5), (v.div_ceil(2), 0.5)]
                            }
                        };
                        let mut ws = Vec::with_capacity(8);
                        for (a, wa) in axis(i) {
                            for (b, wb) in axis(j) {
                                for (c, wc) in axis(k) {
                                    if let Some(Some(cid)) = cids.get(nidx([a, b, c])) {
                                        ws.push((*cid, wa * wb * wc));
                                    }
                                }
                            }
                        }
                        if let Some(p) = prolong.get_mut(*fid) {
                            *p = ws;
                        }
                    }
                }
            }
            if let Some(l) = levels.last_mut() {
                l.prolong = prolong;
            }
            h *= 2.0;
            let (ke, _) = element(h, &m);
            levels.push(Level {
                smooth: smoother(&cconn, &cscale, &ke, &cfixed),
                conn: cconn,
                scale: cscale,
                fixed: cfixed,
                ke,
                prolong: Vec::new(),
            });
            cells = cc;
            n = nc;
            ids = cids;
        }
        Hierarchy { levels }
    }

    /// The V-cycle applied to `r` (an approximation of `K⁻¹ r`).
    pub(crate) fn apply(&self, r: &[f64]) -> Vec<f64> {
        self.cycle(0, r)
    }

    fn cycle(&self, at: usize, r: &[f64]) -> Vec<f64> {
        let Some(l) = self.levels.get(at) else { return vec![0.0; r.len()] };
        let dofs = l.dofs();
        let mut x = vec![0.0; dofs];
        let mut ax = vec![0.0; dofs];
        let sweep = |x: &mut Vec<f64>, ax: &mut Vec<f64>| {
            l.matvec(x, ax);
            for i in 0..dofs {
                if let (Some(xi), Some(ri), Some(ai), Some(si)) = (x.get_mut(i), r.get(i), ax.get(i), l.smooth.get(i)) {
                    *xi += si * (ri - ai);
                }
            }
        };
        let coarsest = at + 1 >= self.levels.len();
        for _ in 0..if coarsest { COARSEST_SWEEPS } else { SWEEPS } {
            sweep(&mut x, &mut ax);
        }
        if coarsest {
            return x;
        }
        let Some(c) = self.levels.get(at + 1) else { return x };
        // Restrict the residual, correct from the coarse level, prolong back.
        l.matvec(&x, &mut ax);
        let mut rc = vec![0.0; c.dofs()];
        for (f, ws) in l.prolong.iter().enumerate() {
            for d in 0..3 {
                let rv = r.get(3 * f + d).copied().unwrap_or(0.0) - ax.get(3 * f + d).copied().unwrap_or(0.0);
                if l.fixed.get(3 * f + d).copied().unwrap_or(false) {
                    continue;
                }
                for (cid, w) in ws {
                    if let Some(x) = rc.get_mut(3 * cid + d) {
                        *x += w * rv;
                    }
                }
            }
        }
        for (i, v) in rc.iter_mut().enumerate() {
            if c.fixed.get(i).copied().unwrap_or(false) {
                *v = 0.0;
            }
        }
        let ec = self.cycle(at + 1, &rc);
        for (f, ws) in l.prolong.iter().enumerate() {
            for d in 0..3 {
                if l.fixed.get(3 * f + d).copied().unwrap_or(false) {
                    continue;
                }
                let e: f64 = ws.iter().map(|(cid, w)| w * ec.get(3 * cid + d).copied().unwrap_or(0.0)).sum();
                if let Some(xi) = x.get_mut(3 * f + d) {
                    *xi += e;
                }
            }
        }
        for _ in 0..SWEEPS {
            sweep(&mut x, &mut ax);
        }
        x
    }
}
