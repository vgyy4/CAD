#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;

const STEEL: Material = Material { young: 210_000.0, poisson: 0.3 };

/// A structured grid of quads over the rectangle `[0, lx] x [0, ly]` at `z = 0`, `nx` by
/// `ny` elements; node (i, j) has index `j * (nx + 1) + i`.
fn grid(lx: f64, ly: f64, nx: usize, ny: usize) -> ShellMesh {
    let mut nodes = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            nodes.push([lx * i as f64 / nx as f64, ly * j as f64 / ny as f64, 0.0]);
        }
    }
    let mut quads = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let a = (j * (nx + 1) + i) as u32;
            let w = (nx + 1) as u32;
            quads.push([a, a + 1, a + w + 1, a + w]);
        }
    }
    ShellMesh { nodes, quads, tris: Vec::new() }
}

fn problem(mesh: ShellMesh, t: f64, fixed: Vec<u32>, loads: Vec<(u32, [f64; 3])>) -> ShellProblem {
    ShellProblem { mesh, thickness: t, material: STEEL, fixed, loads }
}

/// Cantilever strip (100 x 10) along x, clamped at x = 0, load `p` (N, total) in direction
/// `dir` spread over the tip nodes (trapezoidal shares). Returns the mean tip displacement
/// along `dir`.
fn strip(t: f64, nx: usize, ny: usize, dir: V3, rot: Option<[[f64; 3]; 3]>) -> (f64, ShellSolution) {
    let mut mesh = grid(100.0, 10.0, nx, ny);
    let w = nx + 1;
    let fixed: Vec<u32> = (0..=ny).map(|j| (j * w) as u32).collect();
    let tip: Vec<usize> = (0..=ny).map(|j| j * w + nx).collect();
    let mut dir = dir;
    if let Some(r) = rot {
        let ap = |v: V3| -> V3 { [dot(r[0], v), dot(r[1], v), dot(r[2], v)] };
        for n in &mut mesh.nodes {
            *n = ap(*n);
        }
        dir = ap(dir);
    }
    let total = 1.0;
    let loads = tip
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let share = if k == 0 || k == ny { 0.5 } else { 1.0 } / ny as f64;
            (i as u32, dir.map(|c| c * total * share))
        })
        .collect();
    let s = shell_static(&problem(mesh, t, fixed, loads)).unwrap();
    let tipd = tip.iter().map(|&i| dot(s.displacement[i], dir)).sum::<f64>() / tip.len() as f64;
    (tipd, s)
}

fn euler(p: f64, l: f64, i: f64) -> f64 {
    p * l * l * l / (3.0 * STEEL.young * i)
}

#[test]
fn cantilever_strip_bends_like_a_beam() {
    let (d, s) = strip(1.0, 20, 2, [0.0, 0.0, 1.0], None);
    let theory = euler(1.0, 100.0, 10.0 / 12.0);
    let err = (d - theory).abs() / theory;
    eprintln!("bend: {d} vs {theory}: {:.3}% ({} its)", err * 100.0, s.iterations);
    assert!(err < 0.03, "{err}");
    assert!(s.max_von_mises > 0.0);
    // Root stress: M c / I = P L (t/2) / I = 100 * 0.5 / (10/12) = 60 MPa (element centre
    // reads a little less than the clamped edge).
    assert!(s.max_von_mises > 40.0 && s.max_von_mises < 70.0, "{}", s.max_von_mises);
}

#[test]
fn tilted_strip_gives_the_same_deflection() {
    // A rotation about the axis (1, 2, 3), 0.7 rad (Rodrigues).
    let ax = {
        let l = libm::sqrt(14.0);
        [1.0 / l, 2.0 / l, 3.0 / l]
    };
    let (c, s) = (libm::cos(0.7), libm::sin(0.7));
    let k = [[0.0, -ax[2], ax[1]], [ax[2], 0.0, -ax[0]], [-ax[1], ax[0], 0.0]];
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let kk: f64 = (0..3).map(|m| k[i][m] * k[m][j]).sum();
            r[i][j] = (if i == j { 1.0 } else { 0.0 }) + s * k[i][j] + (1.0 - c) * kk;
        }
    }
    let (flat, _) = strip(1.0, 20, 2, [0.0, 0.0, 1.0], None);
    let (tilted, _) = strip(1.0, 20, 2, [0.0, 0.0, 1.0], Some(r));
    eprintln!("tilted: {tilted} vs {flat}");
    assert!((tilted - flat).abs() / flat < 1e-6);
}

#[test]
fn strip_loaded_in_plane_matches_beam_theory() {
    // Tolerance 5%: the Q4 membrane with incompatible modes is exact for rectangles in
    // pure bending; the clamped-end Poisson restraint and shear add a little.
    let (d, s) = strip(1.0, 40, 4, [0.0, 1.0, 0.0], None);
    let theory = euler(1.0, 100.0, 1.0 * 1000.0 / 12.0);
    let err = (d - theory).abs() / theory;
    eprintln!("in-plane 40x4: {d} vs {theory}: {:.3}% ({} its)", err * 100.0, s.iterations);
    assert!(err < 0.05, "{err}");
    let (d2, _) = strip(1.0, 20, 2, [0.0, 1.0, 0.0], None);
    let err2 = (d2 - theory).abs() / theory;
    eprintln!("in-plane 20x2: {d2}: {:.3}%", err2 * 100.0);
    assert!(err2 < 0.05, "{err2}");
}

#[test]
fn clamped_square_plate_under_uniform_load() {
    // a = 100, t = 2, q = 0.1 MPa: w_max = 0.00126 q a^4 / D at the centre.
    let (a, t, q) = (100.0, 2.0, 0.1);
    let n = 16;
    let mesh = grid(a, a, n, n);
    let w = n + 1;
    let h = a / n as f64;
    let mut fixed = Vec::new();
    let mut loads = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            let id = (j * w + i) as u32;
            if i == 0 || j == 0 || i == n || j == n {
                fixed.push(id);
            } else {
                loads.push((id, [0.0, 0.0, q * h * h]));
            }
        }
    }
    let s = shell_static(&problem(mesh, t, fixed, loads)).unwrap();
    let d = STEEL.young * t * t * t / (12.0 * (1.0 - STEEL.poisson * STEEL.poisson));
    let theory = 0.00126 * q * a.powi(4) / d;
    let got = s.displacement[(n / 2) * w + n / 2][2].abs();
    let err = (got - theory).abs() / theory;
    eprintln!("plate: {got} vs {theory}: {:.3}% ({} its)", err * 100.0, s.iterations);
    assert!(err < 0.05, "{err}");
}

#[test]
fn thin_limit_scales_with_the_cube_of_thickness() {
    let (thick, _) = strip(1.0, 20, 2, [0.0, 0.0, 1.0], None);
    let (thin, s) = strip(0.1, 20, 2, [0.0, 0.0, 1.0], None);
    let ratio = thin / thick;
    eprintln!("thin: ratio {ratio} ({} its)", s.iterations);
    assert!((ratio / 1000.0 - 1.0).abs() < 0.05, "{ratio}");
}

/// Two strips (10 wide, 2 elements): leg A along x (0..100) at z = 0, clamped at x = 0; leg B
/// rises from x = 100 to z = 50. Returns the tip's mean displacement under a unit load
/// along `dir` on the leg-B tip.
fn bracket(dir: V3) -> V3 {
    let (na, nb) = (20usize, 10usize);
    let mut nodes = Vec::new();
    let mut quads = Vec::new();
    let w = na + 1; // A: index j * w + i
    for j in 0..=2 {
        for i in 0..=na {
            nodes.push([5.0 * i as f64, 5.0 * j as f64, 0.0]);
        }
    }
    for j in 0..2 {
        for i in 0..na {
            let a = (j * w + i) as u32;
            quads.push([a, a + 1, a + w as u32 + 1, a + w as u32]);
        }
    }
    // B: layer k = 1..=nb; layer 0 is A's x = 100 column.
    let base = nodes.len();
    let layer =
        |k: usize, j: usize| -> u32 { if k == 0 { (j * w + na) as u32 } else { (base + (k - 1) * 3 + j) as u32 } };
    for k in 1..=nb {
        for j in 0..=2 {
            nodes.push([100.0, 5.0 * j as f64, 5.0 * k as f64]);
        }
    }
    for k in 0..nb {
        for j in 0..2 {
            quads.push([layer(k, j), layer(k, j + 1), layer(k + 1, j + 1), layer(k + 1, j)]);
        }
    }
    let fixed = (0..=2).map(|j| (j * w) as u32).collect();
    let loads = (0..=2).map(|j| (layer(nb, j), dir.map(|c| c * if j == 1 { 0.5 } else { 0.25 }))).collect();
    let s = shell_static(&problem(ShellMesh { nodes, quads, tris: Vec::new() }, 1.0, fixed, loads)).unwrap();
    let mut m = [0.0; 3];
    for j in 0..=2 {
        let d = s.displacement[layer(nb, j) as usize];
        for (a, b) in m.iter_mut().zip(d) {
            *a += b / 3.0;
        }
    }
    assert!(s.max_displacement.is_finite() && s.max_von_mises > 0.0);
    m
}

#[test]
fn l_bracket_matches_beam_superposition() {
    let (l, h) = (100.0, 50.0);
    let (i, area) = (10.0 / 12.0, 10.0);
    let e = STEEL.young;
    // Load along z (leg B's axis): leg A is a cantilever, leg B stretches.
    let z = bracket([0.0, 0.0, 1.0]);
    let zt = euler(1.0, l, i) + h / (e * area);
    eprintln!("bracket z: {} vs {zt}", z[2]);
    assert!((z[2] - zt).abs() / zt < 0.10, "{}", z[2]);
    // Load along x: leg B bends as a cantilever and leg A bends under the moment M = P H, rotating the corner by M L / E I.
    let x = bracket([1.0, 0.0, 0.0]);
    // Leg B bends about its thin direction (x is its thickness direction): I = w t^3 / 12.
    let beam = h * h * h / (3.0 * e * i) + h * h * l / (e * i);
    eprintln!("bracket x: {} vs {beam}", x[0]);
    assert!((x[0] - beam).abs() / beam < 0.10, "{}", x[0]);
}

#[test]
fn rejects_bad_input() {
    let m = grid(10.0, 10.0, 2, 2);
    let mut p = problem(m.clone(), 1.0, vec![0], vec![(8, [0.0, 0.0, 1.0])]);
    p.mesh.tris.push([0, 1, 2]);
    assert!(matches!(shell_static(&p), Err(FeaError::Invalid(_))));
    let p = problem(m.clone(), 1.0, vec![], vec![(8, [0.0, 0.0, 1.0])]);
    assert!(matches!(shell_static(&p), Err(FeaError::Invalid(_))));
    let p = problem(m.clone(), 0.0, vec![0], vec![]);
    assert!(matches!(shell_static(&p), Err(FeaError::Invalid(_))));
    let big = ShellMesh { nodes: vec![[0.0; 3]; MAX_SHELL_NODES + 1], quads: vec![[0, 1, 2, 3]], tris: Vec::new() };
    let p = problem(big, 1.0, vec![0], vec![]);
    assert!(matches!(shell_static(&p), Err(FeaError::TooLarge(_))));
}
