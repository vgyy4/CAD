#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;

/// A box mesh from the origin to `s`, faces: 0 x=0, 1 x=s, 2 y=0, 3 y=s, 4 z=0, 5 z=s
/// (two outward-wound triangles each).
fn box_mesh(s: [f32; 3]) -> Mesh {
    let c = |i: usize| {
        [if i & 1 > 0 { s[0] } else { 0.0 }, if i & 2 > 0 { s[1] } else { 0.0 }, if i & 4 > 0 { s[2] } else { 0.0 }]
    };
    let quads: [[usize; 4]; 6] = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    let positions: Vec<[f32; 3]> = (0..8).map(c).collect();
    let mut indices = Vec::new();
    let mut face_ranges = Vec::new();
    for q in quads {
        // Face ranges count triangles.
        let start = (indices.len() / 3) as u32;
        indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]].map(|v| v as u32));
        face_ranges.push(start..(indices.len() / 3) as u32);
    }
    Mesh { normals: vec![[0.0; 3]; positions.len()], positions, indices, face_ranges, edges: Vec::new() }
}

#[test]
fn a_bar_in_tension_stretches_by_pl_over_ea() {
    // 10 × 10 × 100 mm, fixed at z = 0, pulled by 1 kN at z = 100, steel, no Poisson effect.
    let mesh = box_mesh([10.0, 10.0, 100.0]);
    let p = Problem {
        mesh: &mesh,
        voxel: 5.0,
        fixed: vec![4],
        loads: vec![Load { face: 5, force: [0.0, 0.0, 1000.0] }],
        material: Material { young: 200_000.0, poisson: 0.0 },
    };
    let s = solve(&p).unwrap();
    assert_eq!(s.elements, 2 * 2 * 20);
    let exact = 1000.0 * 100.0 / (200_000.0 * 100.0);
    assert!((s.max_displacement - exact).abs() < 1e-6 * exact, "{} vs {exact}", s.max_displacement);
    // Uniform stress P/A = 10 MPa.
    assert!((s.max_von_mises - 10.0).abs() < 1e-6, "{}", s.max_von_mises);
    assert_eq!(s.fixed_nodes, 9);
    // The stress picture: the bar's outside, 2·(2·2) + 4·(2·20) voxel faces at 10 MPa, each
    // wound to face outwards.
    assert_eq!(s.surface.len(), 8 + 160);
    for q in &s.surface {
        assert!((q.von_mises - 10.0).abs() < 1e-6, "{}", q.von_mises);
        let [a, b, c, _] = q.corners;
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let mid = [0, 1, 2].map(|d| q.corners.iter().map(|p| p[d]).sum::<f64>() / 4.0);
        let out = [mid[0] - 5.0, mid[1] - 5.0, mid[2] - 50.0];
        assert!(n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0, "{q:?}");
    }
}

#[test]
fn a_cantilever_bends_close_to_beam_theory() {
    // 100 mm long (x), 10 wide (y), 10 high (z), fixed at x = 0, 100 N down at x = 100.
    let mesh = box_mesh([100.0, 10.0, 10.0]);
    let p = Problem {
        mesh: &mesh,
        voxel: 2.0,
        fixed: vec![0],
        loads: vec![Load { face: 1, force: [0.0, 0.0, -100.0] }],
        material: Material { young: 200_000.0, poisson: 0.3 },
    };
    let s = solve(&p).unwrap();
    let i = 10.0 * 10f64.powi(3) / 12.0;
    let exact = 100.0 * 100f64.powi(3) / (3.0 * 200_000.0 * i);
    // The multigrid cycle keeps the iterations few (Jacobi took 207 here).
    assert!(s.iterations < 120, "{} iterations", s.iterations);
    // Full-integration bricks are a little stiff in bending.
    assert!(s.max_displacement > 0.8 * exact && s.max_displacement < 1.05 * exact, "{} vs {exact}", s.max_displacement);
    // Root stress M c / I = 100·100·5/I = 60 MPa, read at element centres half a voxel in.
    assert!(s.max_von_mises > 40.0 && s.max_von_mises < 70.0, "{}", s.max_von_mises);
}

#[test]
fn unheld_or_bad_problems_are_refused() {
    let mesh = box_mesh([10.0, 10.0, 10.0]);
    let base = Problem {
        mesh: &mesh,
        voxel: 2.0,
        fixed: vec![],
        loads: vec![],
        material: Material { young: 200_000.0, poisson: 0.3 },
    };
    assert!(solve(&base).is_err());
    let mut p = base.clone();
    p.fixed = vec![0];
    p.voxel = 0.0001;
    assert!(matches!(solve(&p), Err(FeaError::TooLarge(_))));
    p.voxel = 2.0;
    p.material.poisson = 0.5;
    assert!(solve(&p).is_err());
    p.material.poisson = 0.3;
    p.fixed = vec![99];
    assert!(solve(&p).is_err());
}

#[test]
fn a_small_symmetric_matrix_has_its_eigenvalues() {
    let (vals, vecs) = sym_eig(vec![2.0, 1.0, 0.0, 1.0, 2.0, 0.0, 0.0, 0.0, 5.0], 3);
    for (got, want) in vals.iter().zip([1.0, 3.0, 5.0]) {
        assert!((got - want).abs() < 1e-12, "{vals:?}");
    }
    // The first eigenvector is (1, -1, 0) / sqrt 2.
    assert!((vecs[0].abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9 && (vecs[0] + vecs[3]).abs() < 1e-9);
}

#[test]
fn a_cantilever_rings_close_to_beam_theory() {
    // 100 × 10 × 10 mm steel, fixed at x = 0: f1 = (1.8751² / 2π) sqrt(EI / (ρ A L⁴)), twice
    // (the section is square, so it bends the same way both ways).
    let mesh = box_mesh([100.0, 10.0, 10.0]);
    let p = ModalProblem {
        mesh: &mesh,
        voxel: 2.5,
        fixed: vec![0],
        material: Material { young: 210_000.0, poisson: 0.3 },
        density: 7.85e-9,
        modes: 3,
    };
    let m = modes(&p).unwrap();
    let ei = 210_000.0 * 10.0 * 10f64.powi(3) / 12.0;
    let exact = 1.8751f64.powi(2) / (2.0 * std::f64::consts::PI) * (ei / (7.85e-9 * 100.0 * 100f64.powi(4))).sqrt();
    let f = &m.frequencies;
    assert_eq!(f.len(), 3);
    // Multigrid-preconditioned LOBPCG: 30 iterations here (Jacobi took 525).
    assert!(m.iterations < 120, "{} iterations", m.iterations);
    assert!(f[0] > 0.95 * exact && f[0] < 1.2 * exact, "{f:?} vs {exact} ({} iterations)", m.iterations);
    assert!((f[1] - f[0]).abs() < 0.01 * f[0], "{f:?}");
    assert!(f[2] > 1.5 * f[0], "{f:?}");
    let mut bad = p.clone();
    bad.modes = 0;
    assert!(modes(&bad).is_err());
    bad.modes = 2;
    bad.density = 0.0;
    assert!(modes(&bad).is_err());
}

/// Two meshes as one (the second moved by `d`), faces of the second after the first's.
fn joined(a: Mesh, b: Mesh, d: [f32; 3]) -> Mesh {
    let mut m = a;
    let base = m.positions.len() as u32;
    let tris = (m.indices.len() / 3) as u32;
    m.positions.extend(b.positions.iter().map(|p| [p[0] + d[0], p[1] + d[1], p[2] + d[2]]));
    m.normals.extend(b.normals);
    m.indices.extend(b.indices.iter().map(|i| i + base));
    m.face_ranges.extend(b.face_ranges.iter().map(|r| r.start + tris..r.end + tris));
    m
}

#[test]
fn parts_held_only_along_an_edge_and_loads_on_held_faces_are_refused() {
    // A second cube touching the first along one edge only: free to turn about it.
    let mesh = joined(box_mesh([10.0; 3]), box_mesh([10.0; 3]), [10.0, 10.0, 0.0]);
    let steel = Material { young: 210_000.0, poisson: 0.3 };
    let p = Problem {
        mesh: &mesh,
        voxel: 2.5,
        fixed: vec![0],
        loads: vec![Load { face: 7, force: [1.0, 0.0, 0.0] }],
        material: steel,
    };
    let e = solve(&p).unwrap_err().to_string();
    assert!(e.contains("not held"), "{e}");
    let m = ModalProblem { mesh: &mesh, voxel: 2.5, fixed: vec![0], material: steel, density: 7.85e-9, modes: 1 };
    assert!(modes(&m).unwrap_err().to_string().contains("not held"));
    // A load on the fixed face does nothing; no load at all is refused too.
    let bar = box_mesh([10.0, 10.0, 50.0]);
    let on_held = Problem {
        mesh: &bar,
        voxel: 2.5,
        fixed: vec![4],
        loads: vec![Load { face: 4, force: [0.0, 0.0, 10.0] }],
        material: steel,
    };
    assert!(solve(&on_held).unwrap_err().to_string().contains("held voxels only"));
    let none = Problem { loads: Vec::new(), ..on_held.clone() };
    assert!(solve(&none).is_err());
    // A side load shares its edge nodes with the held face: that share is reported.
    let side = Problem { loads: vec![Load { face: 0, force: [10.0, 0.0, 0.0] }], ..on_held };
    let s = solve(&side).unwrap();
    assert!(s.held_load > 0.0 && s.held_load < 0.2, "{}", s.held_load);
    // Out-of-range material and loads.
    let soft = Problem { material: Material { young: 1e-300, poisson: 0.3 }, ..side.clone() };
    assert!(solve(&soft).is_err());
    let huge = Problem { loads: vec![Load { face: 5, force: [1e300, 0.0, 0.0] }], ..side };
    assert!(solve(&huge).is_err());
    assert!(over_budget(MAX_WORK + 1).is_err() && over_budget(MAX_WORK).is_ok());
}
