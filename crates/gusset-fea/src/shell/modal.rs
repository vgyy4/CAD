//! Natural frequencies and mode shapes of a flat-shell model (ADR 0041, "Modes on shells").
//!
//! `K x = lambda M x` with the shell stiffness of `shell_static` and a lumped (diagonal)
//! mass, fixed dofs eliminated. The lowest modes come from shift-free inverse subspace
//! iteration with Rayleigh-Ritz: each pass solves `K Y = M X` for a block of vectors with the
//! ICT-preconditioned CG of the static solver, M-orthonormalises `Y`, and rotates it to the
//! eigenvectors of the projected `Yᵀ K Y`. Units: mm, N, t/mm^3, so `lambda = omega^2` in 1/s^2
//! and `f = sqrt(lambda) / (2 pi)` in Hz.

use super::{Bsr, FeaError, Ic, ShellMesh, assemble, check, dot, dot_n, g};
use crate::{Material, sym_eig};

/// Most modes one analysis may ask for (as the voxel `modes`).
const MAX_SHELL_MODES: usize = 10;
const MAX_SUBSPACE_ITERATIONS: usize = 300;
const MAX_CG_ITERATIONS: usize = 50_000;
const CG_TOLERANCE: f64 = 1e-10;
/// Relative change of the wanted eigenvalues below which the iteration stops.
const EIGEN_TOLERANCE: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq)]
pub struct ShellModalProblem {
    pub mesh: ShellMesh,
    /// Sheet thickness (mm), uniform.
    pub thickness: f64,
    pub material: Material,
    /// Density (t/mm^3; steel 7.85e-9).
    pub density: f64,
    /// Nodes clamped in all 6 degrees of freedom. At least one is required: a free shell has
    /// rigid-body modes at 0 Hz, which are refused with an error rather than returned.
    pub fixed: Vec<u32>,
    /// How many modes (1 to 10).
    pub modes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellModes {
    /// Natural frequencies (Hz), ascending.
    pub frequencies: Vec<f64>,
    /// Per mode, the translation of every node (global axes), scaled so the largest node
    /// translation has length 1.
    pub shapes: Vec<Vec<[f64; 3]>>,
    /// Subspace iterations used.
    pub iterations: usize,
}

/// Lumped mass per dof: each element gives every corner a quarter of `rho t A` as
/// translational mass (all three axes) and a quarter of `rho t^3 A / 12` as rotational inertia
/// about all three axes. The in-plane (drilling) rotation gets the same small inertia as the
/// two bending rotations; it is a penalty dof without physical inertia, so this only keeps `M`
/// positive definite and puts its spurious modes far above the structural ones.
fn lumped_mass(mesh: &ShellMesh, t: f64, rho: f64) -> Vec<f64> {
    let mut m = vec![0.0; 6 * mesh.nodes.len()];
    for q in &mesh.quads {
        let p = |k: usize| q.get(k).and_then(|&i| mesh.nodes.get(i as usize)).copied().unwrap_or([0.0; 3]);
        let d1 = [p(2)[0] - p(0)[0], p(2)[1] - p(0)[1], p(2)[2] - p(0)[2]];
        let d2 = [p(3)[0] - p(1)[0], p(3)[1] - p(1)[1], p(3)[2] - p(1)[2]];
        let c = [d1[1] * d2[2] - d1[2] * d2[1], d1[2] * d2[0] - d1[0] * d2[2], d1[0] * d2[1] - d1[1] * d2[0]];
        let area = 0.5 * libm::sqrt(dot(c, c));
        let (mt, mr) = (0.25 * rho * t * area, 0.25 * rho * t * t * t * area / 12.0);
        for &n in q {
            for k in 0..6 {
                if let Some(x) = m.get_mut(6 * n as usize + k) {
                    *x += if k < 3 { mt } else { mr };
                }
            }
        }
    }
    m
}

/// `K y = b` on the free dofs by ICT-preconditioned CG.
fn solve(k: &Bsr, pre: &Ic, free: &[bool], b: &[f64]) -> Result<Vec<f64>, FeaError> {
    let n = b.len();
    let mask = |v: &mut [f64]| {
        for (x, &f) in v.iter_mut().zip(free) {
            if !f {
                *x = 0.0;
            }
        }
    };
    let mut r = b.to_vec();
    mask(&mut r);
    let bnorm = libm::sqrt(dot_n(&r, &r));
    let mut u = vec![0.0; n];
    if bnorm <= 0.0 {
        return Ok(u);
    }
    let mut z = vec![0.0; n];
    pre.apply(&r, &mut z);
    let mut dir = z.clone();
    let mut rz = dot_n(&r, &z);
    let mut kd = vec![0.0; n];
    for _ in 0..MAX_CG_ITERATIONS {
        k.matvec(&dir, &mut kd);
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
        if libm::sqrt(dot_n(&r, &r)) <= CG_TOLERANCE * bnorm {
            return Ok(u);
        }
        pre.apply(&r, &mut z);
        let rz_new = dot_n(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for (d, zz) in dir.iter_mut().zip(&z) {
            *d = zz + beta * *d;
        }
    }
    Err(FeaError::NotConverged("the shell solver did not converge (the model may be free to move)".into()))
}

fn m_dot(m: &[f64], a: &[f64], b: &[f64]) -> f64 {
    m.iter().zip(a).zip(b).map(|((m, a), b)| m * a * b).sum()
}

/// The lowest natural frequencies and mode shapes of a flat-shell model.
pub fn shell_modes(p: &ShellModalProblem) -> Result<ShellModes, FeaError> {
    if !(p.density.is_finite() && p.density > 0.0) {
        return Err(FeaError::Invalid("the density must be positive".into()));
    }
    if !(1..=MAX_SHELL_MODES).contains(&p.modes) {
        return Err(FeaError::Invalid(format!("ask for 1 to {MAX_SHELL_MODES} modes")));
    }
    let mesh = &p.mesh;
    let n = mesh.nodes.len();
    let used = check(mesh, p.thickness, &p.material)?;
    if p.fixed.is_empty() {
        return Err(FeaError::Invalid(
            "nothing is fixed: a free shell has rigid-body modes at 0 Hz. Fix a face or edge".into(),
        ));
    }
    let mut free = vec![true; 6 * n];
    for (i, u) in used.iter().enumerate() {
        if !u {
            for f in free.iter_mut().skip(6 * i).take(6) {
                *f = false;
            }
        }
    }
    for &i in &p.fixed {
        if !used.get(i as usize).copied().unwrap_or(false) {
            return Err(FeaError::Invalid(format!("fixed node {i} is not part of an element")));
        }
        for f in free.iter_mut().skip(6 * i as usize).take(6) {
            *f = false;
        }
    }
    let nfree = free.iter().filter(|f| **f).count();
    if nfree < p.modes {
        return Err(FeaError::Invalid("fewer free degrees of freedom than modes asked for".into()));
    }
    let (kmat, _elems) = assemble(mesh, &p.material, p.thickness)?;
    let pre = Ic::build(&kmat, &free);
    let mut mass = lumped_mass(mesh, p.thickness, p.density);
    for (m, &f) in mass.iter_mut().zip(&free) {
        if !f {
            *m = 0.0;
        }
    }

    let dofs = 6 * n;
    let size = (p.modes + p.modes.max(4)).min(nfree);
    // Deterministic start block.
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut x: Vec<Vec<f64>> = (0..size)
        .map(|_| {
            (0..dofs)
                .map(|i| {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    let r = ((seed >> 11) as f64) / ((1u64 << 53) as f64) - 0.5;
                    if free.get(i).copied().unwrap_or(false) { r } else { 0.0 }
                })
                .collect()
        })
        .collect();
    let mut lambda_old = vec![f64::INFINITY; p.modes];
    let mut iterations = 0;
    let mut lambda = vec![0.0; size];
    let mut converged = false;
    while iterations < MAX_SUBSPACE_ITERATIONS {
        iterations += 1;
        // Y = K^-1 M X.
        let mut y: Vec<Vec<f64>> = Vec::with_capacity(size);
        for xv in &x {
            let rhs: Vec<f64> = xv.iter().zip(&mass).map(|(a, m)| a * m).collect();
            y.push(solve(&kmat, &pre, &free, &rhs)?);
        }
        // M-orthonormalise (modified Gram-Schmidt, twice).
        for j in 0..size {
            for _ in 0..2 {
                for i in 0..j {
                    let (head, tail) = y.split_at_mut(j);
                    let (Some(yi), Some(yj)) = (head.get(i), tail.first_mut()) else { continue };
                    let c = m_dot(&mass, yi, yj);
                    for (a, b) in yj.iter_mut().zip(yi) {
                        *a -= c * b;
                    }
                }
            }
            let Some(yj) = y.get_mut(j) else { continue };
            let nrm = libm::sqrt(m_dot(&mass, yj, yj));
            if nrm.is_nan() || nrm <= 1e-300 {
                return Err(FeaError::NotConverged("the mode subspace collapsed".into()));
            }
            for a in yj.iter_mut() {
                *a /= nrm;
            }
        }
        // Rayleigh-Ritz with the projected stiffness.
        let mut ky: Vec<Vec<f64>> = Vec::with_capacity(size);
        let mut tmp = vec![0.0; dofs];
        for yv in &y {
            kmat.matvec(yv, &mut tmp);
            ky.push(tmp.clone());
        }
        let mut a = vec![0.0; size * size];
        for i in 0..size {
            for j in 0..size {
                if let (Some(yi), Some(kj), Some(c)) = (y.get(i), ky.get(j), a.get_mut(i * size + j)) {
                    *c = dot_n(yi, kj);
                }
            }
        }
        for i in 0..size {
            for j in 0..i {
                let s = 0.5 * (g(&a, i * size + j) + g(&a, j * size + i));
                for idx in [i * size + j, j * size + i] {
                    if let Some(c) = a.get_mut(idx) {
                        *c = s;
                    }
                }
            }
        }
        let (vals, vecs) = sym_eig(a, size);
        lambda = vals;
        x = (0..size)
            .map(|c| {
                let mut out = vec![0.0; dofs];
                for (r, yr) in y.iter().enumerate() {
                    let w = g(&vecs, r * size + c);
                    for (o, v) in out.iter_mut().zip(yr) {
                        *o += w * v;
                    }
                }
                out
            })
            .collect();
        let done =
            lambda.iter().zip(&lambda_old).take(p.modes).all(|(l, o)| (l - o).abs() <= EIGEN_TOLERANCE * l.abs());
        for (o, l) in lambda_old.iter_mut().zip(&lambda) {
            *o = *l;
        }
        if done {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(FeaError::NotConverged(format!(
            "the frequency solver did not converge in {MAX_SUBSPACE_ITERATIONS} iterations"
        )));
    }

    let two_pi = 2.0 * std::f64::consts::PI;
    let frequencies = lambda.iter().take(p.modes).map(|l| libm::sqrt(l.max(0.0)) / two_pi).collect();
    let shapes = x
        .iter()
        .take(p.modes)
        .map(|v| {
            let mut s: Vec<[f64; 3]> = v.chunks(6).map(|c| [g(c, 0), g(c, 1), g(c, 2)]).collect();
            let big = s.iter().map(|d| libm::sqrt(dot(*d, *d))).fold(0.0, f64::max);
            if big > 0.0 {
                for d in &mut s {
                    *d = d.map(|c| c / big);
                }
            }
            s
        })
        .collect();
    Ok(ShellModes { frequencies, shapes, iterations })
}

#[cfg(test)]
mod tests;
