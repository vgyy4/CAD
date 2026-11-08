//! Topology optimisation on the voxel model (generative design, phase 1): SIMP densities
//! per voxel, minimum compliance for a given share of the material, optimality-criteria
//! updates with a sensitivity filter (Sigmund's 99-line method, in 3D on our hexahedra).
//!
//! The design space is the body as voxelised for the static analysis; fixed and loaded
//! faces keep their voxels solid. The result is a density per voxel and the outer faces of the
//! voxels kept (density at least one half), ready to show or to write as a mesh.

use super::*;

/// What to optimise.
#[derive(Debug)]
pub struct TopoProblem<'a> {
    pub mesh: &'a Mesh,
    pub voxel: f64,
    pub fixed: Vec<usize>,
    pub loads: Vec<Load>,
    pub material: Material,
    /// Share of the body's volume to keep, 0.05..0.95.
    pub keep: f64,
    /// Most design iterations (each one a static solve).
    pub iterations: usize,
    /// Mirror symmetry about the voxel grid's middle across X, Y and Z (the layout is the
    /// same on both sides; supports and loads should be symmetric too).
    pub symmetry: [bool; 3],
    /// Smallest member (mm): the sensitivity filter's diameter; `None` = 3 voxels.
    pub min_member: Option<f64>,
}

/// The optimised layout.
#[derive(Debug, Clone, PartialEq)]
pub struct TopoResult {
    pub elements: usize,
    pub iterations: usize,
    /// Compliance (N mm: the work the loads do) of each iteration; it should fall.
    pub compliance: Vec<f64>,
    /// Share of the design space kept (density at least one half).
    pub kept_share: f64,
    /// Kept voxels: their volume (mm³).
    pub kept_volume: f64,
    /// Largest density change of the last iteration (below 0.01: converged).
    pub change: f64,
    /// Outer faces of the kept voxels (corners counter-clockwise seen from outside).
    pub faces: Vec<[[f64; 3]; 4]>,
}

/// SIMP penalty, lowest density (keeps the stiffness positive), filter radius (voxels), move
/// limit per iteration.
const PENALTY: f64 = 3.0;
const MIN_DENSITY: f64 = 1e-3;
const FILTER_RADIUS: f64 = 1.5;
const MOVE: f64 = 0.2;
/// The whole optimisation may do this many times one analysis's work.
const TOPO_WORK: u64 = 8 * MAX_WORK;

/// Runs the optimisation.
///
/// # Errors
/// Invalid input, a model too large for the work budget, or a solve that does not converge.
pub fn optimise(p: &TopoProblem<'_>) -> Result<TopoResult, FeaError> {
    let bad = |m: &str| Err(FeaError::Invalid(m.to_string()));
    if !(p.keep.is_finite() && (0.05..=0.95).contains(&p.keep)) {
        return bad("keep is the share of material to keep, from 0.05 to 0.95");
    }
    if p.iterations == 0 || p.iterations > 200 {
        return bad("iterations must be from 1 to 200");
    }
    if p.loads.is_empty() {
        return bad("add a load: with none, nothing moves");
    }
    let md = build(p.mesh, p.voxel, &p.fixed, p.material, None, &|_| false)?;
    let Model { g, n, h, elems, conn, used, quads, fixed, ke, .. } = &md;
    let (h, n) = (*h, *n);
    let dofs = md.dofs();
    let ne = conn.len();
    // Loads, shared over the nodes of their faces as in the static analysis.
    let mut f = vec![0.0; dofs];
    let mut loaded = vec![false; md.node_pos.len()];
    for l in &p.loads {
        if !l.force.iter().all(|v| v.is_finite() && v.abs() <= 1e12) {
            return bad("a load is not a number (or above 1e12 N)");
        }
        let ids = face_nodes(p.mesh, l.face, quads, g, used)?;
        if ids.is_empty() {
            return bad("a loaded face reaches no voxel: use a smaller voxel");
        }
        let total: usize = ids.iter().map(|(_, c)| c).sum();
        for (id, count) in ids {
            let share = count as f64 / total.max(1) as f64;
            if let Some(x) = loaded.get_mut(id) {
                *x = true;
            }
            for (d, fv) in l.force.iter().enumerate() {
                if let Some(x) = f.get_mut(3 * id + d) {
                    *x += fv * share;
                }
            }
        }
    }
    for (x, held) in f.iter_mut().zip(fixed) {
        if *held {
            *x = 0.0;
        }
    }
    // A zero force, or loads only on held faces, would leave nothing to optimise for
    // (dogfooding F12-3).
    if f.iter().all(|v| *v == 0.0) {
        return bad("the loads push nothing: give a non-zero force on a face that is not fixed");
    }
    // Voxels touching a held or loaded node stay solid (the supports and the load path).
    let frozen: Vec<bool> = conn
        .iter()
        .map(|c| {
            c.iter().any(|id| loaded.get(*id).copied().unwrap_or(false) || fixed.get(3 * id).copied().unwrap_or(false))
        })
        .collect();
    // Filter neighbours: voxels whose centres are within the radius, by grid index.
    let lin = |[i, j, k]: [usize; 3]| i + n[0] * (j + n[1] * k);
    let index: Vec<usize> = elems.iter().map(|c| lin(*c)).collect();
    let radius = match p.min_member {
        Some(m) if m.is_finite() && m > 0.0 => (m / h / 2.0).max(1.0),
        Some(_) => return bad("min_member must be a positive length"),
        None => FILTER_RADIUS,
    };
    if radius > 8.0 {
        return bad("min_member is more than 16 voxels: use a larger voxel");
    }
    let reach = libm::floor(radius) as i64;
    // The neighbour lists cost (2 reach + 1)^3 entries a voxel at most (security review S9).
    let per = (2 * reach + 1).pow(3) as u64;
    if per.saturating_mul(conn.len() as u64) > 60_000_000 {
        return bad("min_member is too large for this many voxels: use a larger voxel or a smaller min_member");
    }
    let mut neigh: Vec<Vec<(usize, f64)>> = Vec::with_capacity(ne);
    for c in elems {
        let mut list = Vec::new();
        for dk in -reach..=reach {
            for dj in -reach..=reach {
                for di in -reach..=reach {
                    let w = radius - libm::sqrt((di * di + dj * dj + dk * dk) as f64);
                    if w <= 0.0 {
                        continue;
                    }
                    let q = [c[0] as i64 + di, c[1] as i64 + dj, c[2] as i64 + dk];
                    if q.iter().zip(n).any(|(v, nn)| *v < 0 || *v >= nn as i64) {
                        continue;
                    }
                    if let Ok(e) = index.binary_search(&lin(q.map(|v| v as usize))) {
                        list.push((e, w));
                    }
                }
            }
        }
        neigh.push(list);
    }
    let mut rho: Vec<f64> = frozen.iter().map(|fz| if *fz { 1.0 } else { p.keep }).collect();
    let target = p.keep * ne as f64;
    let mut u = vec![0.0; dofs];
    let mut history = Vec::new();
    let (mut change, mut work, mut its) = (1.0f64, 0u64, 0usize);
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    while its < p.iterations && change > 0.01 {
        its += 1;
        let scale: Vec<f64> = rho.iter().map(|r| libm::pow(r.max(MIN_DENSITY), PENALTY)).collect();
        // Preconditioned CG from the last displacement (the layout changes a little a step).
        let mg = mg::Hierarchy::new_scaled(elems, n, used, conn, fixed, h, md.material, &scale);
        let mut ku = vec![0.0; dofs];
        mg::product(conn, &scale, ke, fixed, &u, &mut ku);
        let mut r: Vec<f64> = f.iter().zip(&ku).map(|(a, b)| a - b).collect();
        let mut z = mg.apply(&r);
        let mut d = z.clone();
        let mut rz = dot(&r, &z);
        let fnorm = libm::sqrt(dot(&f, &f)).max(1e-300);
        let mut cg = 0;
        while libm::sqrt(dot(&r, &r)) > 1e-8 * fnorm {
            cg += 1;
            work += CYCLE_PRODUCTS * ne as u64;
            if cg > MAX_ITERATIONS || work > TOPO_WORK {
                return Err(if work > TOPO_WORK {
                    FeaError::TooLarge(
                        "the optimisation needs more work than allowed: use a larger voxel or fewer iterations".into(),
                    )
                } else {
                    FeaError::NotConverged("the solver did not converge: the part may be free to move".into())
                });
            }
            mg::product(conn, &scale, ke, fixed, &d, &mut ku);
            let dad = dot(&d, &ku);
            if dad <= 0.0 || !dad.is_finite() {
                return Err(FeaError::NotConverged("the model is not held: fix more faces".into()));
            }
            let a = rz / dad;
            for ((ui, ri), (di, ki)) in u.iter_mut().zip(r.iter_mut()).zip(d.iter().zip(&ku)) {
                *ui += a * di;
                *ri -= a * ki;
            }
            z = mg.apply(&r);
            let rz2 = dot(&r, &z);
            let b = rz2 / rz;
            rz = rz2;
            for (di, zi) in d.iter_mut().zip(&z) {
                *di = zi + b * *di;
            }
        }
        if !u.iter().all(|v| v.is_finite()) {
            return Err(FeaError::NotConverged("the results are not finite".into()));
        }
        // Element energies u_eᵀ k0 u_e, compliance and its sensitivities.
        let mut energy = vec![0.0; ne];
        for (en, c) in energy.iter_mut().zip(conn) {
            let mut ue = [0.0; 24];
            for (chunk, id) in ue.chunks_exact_mut(3).zip(c) {
                if let Some(src) = u.get(3 * id..3 * id + 3) {
                    chunk.copy_from_slice(src);
                }
            }
            *en = ke
                .chunks_exact(24)
                .zip(&ue)
                .map(|(row, ui)| ui * row.iter().zip(&ue).map(|(k, v)| k * v).sum::<f64>())
                .sum();
        }
        history.push(scale.iter().zip(&energy).map(|(s, e)| s * e).sum::<f64>());
        let sens: Vec<f64> =
            rho.iter().zip(&energy).map(|(r, e)| -PENALTY * libm::pow(r.max(MIN_DENSITY), PENALTY - 1.0) * e).collect();
        // Sensitivity filter (density-weighted average over the neighbours).
        let filt: Vec<f64> = neigh
            .iter()
            .zip(&rho)
            .map(|(list, re)| {
                let (mut num, mut den) = (0.0, 0.0);
                for (j, w) in list {
                    let (rj, sj) = (rho.get(*j).copied().unwrap_or(0.0), sens.get(*j).copied().unwrap_or(0.0));
                    num += w * rj * sj;
                    den += w;
                }
                num / (den * re.max(MIN_DENSITY))
            })
            .collect();
        // Optimality criteria: bisect the multiplier that meets the volume.
        let (mut lo, mut hi) = (1e-12, 1e12);
        let mut next = rho.clone();
        for _ in 0..80 {
            let mid = libm::sqrt(lo * hi);
            for ((nx, r), (s, fz)) in next.iter_mut().zip(&rho).zip(filt.iter().zip(&frozen)) {
                *nx = if *fz {
                    1.0
                } else {
                    let be = r * libm::sqrt((-s).max(0.0) / mid);
                    be.clamp((r - MOVE).max(MIN_DENSITY), (r + MOVE).min(1.0))
                };
            }
            if next.iter().sum::<f64>() > target {
                lo = mid;
            } else {
                hi = mid;
            }
            if hi / lo < 1.0 + 1e-6 {
                break;
            }
        }
        // Symmetry: each density and its mirror images share their mean.
        if p.symmetry.iter().any(|v| *v) {
            let mut sym = next.clone();
            for (e, c) in elems.iter().enumerate() {
                let (mut sum, mut cnt) = (0.0, 0usize);
                for mask in 0..8u8 {
                    if (0..3).any(|a| mask & (1 << a) != 0 && !p.symmetry.get(a).copied().unwrap_or(false)) {
                        continue;
                    }
                    let mut q = *c;
                    for (a, (qv, nv)) in q.iter_mut().zip(n).enumerate() {
                        if mask & (1 << a) != 0 {
                            *qv = nv - 1 - *qv;
                        }
                    }
                    if let Ok(m) = index.binary_search(&lin(q)) {
                        sum += next.get(m).copied().unwrap_or(0.0);
                        cnt += 1;
                    }
                }
                if let Some(v) = sym.get_mut(e)
                    && cnt > 0
                    && !frozen.get(e).copied().unwrap_or(false)
                {
                    *v = sum / cnt as f64;
                }
            }
            next = sym;
        }
        change = next.iter().zip(&rho).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        rho = next;
    }
    // The kept voxels: the densest ones up to the asked share (dogfooding F12-4: a 0.5 cut
    // missed it on designs still grey), held and loaded voxels always, ties (mirror images)
    // together.
    let want = (libm::round(target) as usize).clamp(1, ne.max(1));
    let mut order: Vec<f64> = rho.iter().zip(&frozen).filter(|(_, fz)| !**fz).map(|(r, _)| *r).collect();
    let free_want = want.saturating_sub(frozen.iter().filter(|fz| **fz).count());
    order.sort_by(|a, b| b.total_cmp(a));
    let cut = if free_want == 0 {
        f64::INFINITY
    } else {
        order.get(free_want.min(order.len()).saturating_sub(1)).copied().unwrap_or(f64::INFINITY)
    };
    let kept: Vec<bool> = rho.iter().zip(&frozen).map(|(r, fz)| *fz || *r >= cut).collect();
    let is_kept = |q: [i64; 3]| {
        if q.iter().zip(n).any(|(v, nn)| *v < 0 || *v >= nn as i64) {
            return false;
        }
        index.binary_search(&lin(q.map(|v| v as usize))).is_ok_and(|e| kept.get(e).copied().unwrap_or(false))
    };
    let mut faces = Vec::new();
    for (c, k) in elems.iter().zip(&kept) {
        if !k {
            continue;
        }
        let o = g.node_pos(c[0], c[1], c[2]);
        let at = |a: f64, b: f64, cc: f64| [o[0] + a * h, o[1] + b * h, o[2] + cc * h];
        let q = [c[0] as i64, c[1] as i64, c[2] as i64];
        // (neighbour offset, the face's corners counter-clockwise seen from outside)
        let sides: [([i64; 3], [[f64; 3]; 4]); 6] = [
            ([-1, 0, 0], [at(0., 0., 0.), at(0., 0., 1.), at(0., 1., 1.), at(0., 1., 0.)]),
            ([1, 0, 0], [at(1., 0., 0.), at(1., 1., 0.), at(1., 1., 1.), at(1., 0., 1.)]),
            ([0, -1, 0], [at(0., 0., 0.), at(1., 0., 0.), at(1., 0., 1.), at(0., 0., 1.)]),
            ([0, 1, 0], [at(0., 1., 0.), at(0., 1., 1.), at(1., 1., 1.), at(1., 1., 0.)]),
            ([0, 0, -1], [at(0., 0., 0.), at(0., 1., 0.), at(1., 1., 0.), at(1., 0., 0.)]),
            ([0, 0, 1], [at(0., 0., 1.), at(1., 0., 1.), at(1., 1., 1.), at(0., 1., 1.)]),
        ];
        for (d, corners) in sides {
            if !is_kept([q[0] + d[0], q[1] + d[1], q[2] + d[2]]) {
                faces.push(corners);
            }
        }
    }
    let count = kept.iter().filter(|k| **k).count();
    Ok(TopoResult {
        elements: ne,
        iterations: its,
        compliance: history,
        kept_share: count as f64 / ne.max(1) as f64,
        kept_volume: count as f64 * h * h * h,
        change,
        faces,
    })
}

/// Smooths the kept shape's outer faces in place: corners shared by faces are welded by
/// position, then moved by Taubin's two-step Laplacian (lambda 0.5, mu -0.53), which rounds
/// the voxel steps without shrinking the shape. `iterations` pairs of steps, at most 50.
pub fn smooth_faces(faces: &mut [[[f64; 3]; 4]], iterations: usize) {
    use std::collections::BTreeMap;
    let key = |p: [f64; 3]| p.map(|v| (v * 1e4).round() as i64);
    let mut ids: BTreeMap<[i64; 3], usize> = BTreeMap::new();
    let mut pos: Vec<[f64; 3]> = Vec::new();
    let mut quads: Vec<[usize; 4]> = Vec::with_capacity(faces.len());
    for f in faces.iter() {
        quads.push(f.map(|p| {
            *ids.entry(key(p)).or_insert_with(|| {
                pos.push(p);
                pos.len() - 1
            })
        }));
    }
    // Neighbours: the corners next to each other around a face.
    let mut neigh: Vec<Vec<usize>> = vec![Vec::new(); pos.len()];
    for q in &quads {
        for k in 0..4 {
            let (a, b) = (q.get(k).copied().unwrap_or(0), q.get((k + 1) % 4).copied().unwrap_or(0));
            for (x, y) in [(a, b), (b, a)] {
                if let Some(l) = neigh.get_mut(x)
                    && !l.contains(&y)
                {
                    l.push(y);
                }
            }
        }
    }
    let step = |pos: &mut Vec<[f64; 3]>, f: f64| {
        let old = pos.clone();
        for (p, l) in pos.iter_mut().zip(&neigh) {
            if l.is_empty() {
                continue;
            }
            let mut c = [0.0; 3];
            for j in l {
                let q = old.get(*j).copied().unwrap_or(*p);
                for (cv, qv) in c.iter_mut().zip(q) {
                    *cv += qv / l.len() as f64;
                }
            }
            for (pv, cv) in p.iter_mut().zip(c) {
                *pv += f * (cv - *pv);
            }
        }
    };
    for _ in 0..iterations.min(50) {
        step(&mut pos, 0.5);
        step(&mut pos, -0.53);
    }
    for (f, q) in faces.iter_mut().zip(&quads) {
        *f = q.map(|i| pos.get(i).copied().unwrap_or([0.0; 3]));
    }
}

#[cfg(test)]
mod tests;
