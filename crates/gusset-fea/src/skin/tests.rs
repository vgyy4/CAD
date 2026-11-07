#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;
use crate::{Material, ShellProblem, shell_static};
use gusset_kernel::Mesh;

/// A box mesh from the origin to `s` with outward normals, faces: 0 x=0, 1 x=s, 2 y=0,
/// 3 y=s, 4 z=0, 5 z=s (two triangles each, own vertices per face).
fn box_mesh(s: [f32; 3]) -> Mesh {
    let c = |i: usize| {
        [if i & 1 > 0 { s[0] } else { 0.0 }, if i & 2 > 0 { s[1] } else { 0.0 }, if i & 4 > 0 { s[2] } else { 0.0 }]
    };
    let quads: [([usize; 4], [f32; 3]); 6] = [
        ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
        ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
        ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
    ];
    let (mut positions, mut normals, mut indices, mut face_ranges) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (q, n) in quads {
        let start = (indices.len() / 3) as u32;
        let base = positions.len() as u32;
        for v in q {
            positions.push(c(v));
            normals.push(n);
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|i| base + i));
        face_ranges.push(start..(indices.len() / 3) as u32);
    }
    Mesh { positions, normals, indices, face_ranges, edges: Vec::new() }
}

#[test]
fn a_plate_becomes_its_mid_surface_in_quads() {
    let m = box_mesh([100.0, 10.0, 1.0]);
    let mid = mid_surface(&m, 1.0, 2.0).unwrap();
    let (sh, skin) = (&mid.mesh, &mid.skin);
    // The skin is one big face (top or bottom), nothing else.
    assert_eq!(skin.iter().filter(|s| **s).count(), 1);
    assert!(skin[4] || skin[5]);
    assert!(sh.nodes.iter().all(|p| (p[2] - 0.5).abs() < 1e-9), "mid-plane");
    assert!(!sh.quads.is_empty() && sh.tris.is_empty());
    // Every node is used and the quads cover the plate's area.
    let area: f64 = sh
        .quads
        .iter()
        .map(|q| {
            let p = |i: usize| sh.nodes[q[i] as usize];
            let d1 = sub(p(2), p(0));
            let d2 = sub(p(3), p(1));
            norm(cross(d1, d2)) / 2.0
        })
        .sum();
    assert!((area - 1000.0).abs() < 1e-6, "{area}");
    // The end faces hold the nodes on the plate's ends.
    let left = nodes_on_face(&m, 0, &mid, 1e-6);
    assert!(!left.is_empty() && left.iter().all(|i| sh.nodes[*i as usize][0].abs() < 1e-9));
}

#[test]
fn a_plate_cantilever_from_its_surface_mesh_bends_as_a_beam() {
    // 100 x 10 x 1 steel strip held at x = 0, 1 N down at x = 100: P L^3 / (3 E I).
    let m = box_mesh([100.0, 10.0, 1.0]);
    let mid = mid_surface(&m, 1.0, 5.0).unwrap();
    let fixed = nodes_on_face(&m, 0, &mid, 1e-6);
    let tip = nodes_on_face(&m, 1, &mid, 1e-6);
    // A load on the top face reaches every node.
    assert_eq!(nodes_on_face(&m, 5, &mid, 1e-6).len(), mid.mesh.nodes.len());
    let sh = mid.mesh;
    let share = 1.0 / tip.len() as f64;
    let p = ShellProblem {
        mesh: sh,
        thickness: 1.0,
        material: Material { young: 210_000.0, poisson: 0.3 },
        fixed,
        loads: tip.iter().map(|i| (*i, [0.0, 0.0, -share])).collect(),
    };
    let s = shell_static(&p).unwrap();
    let exact = 100f64.powi(3) / (3.0 * 210_000.0 * (10.0 / 12.0));
    let err = (s.max_displacement - exact).abs() / exact;
    assert!(err < 0.05, "{} vs {exact} ({:.1} %)", s.max_displacement, err * 100.0);
}

#[test]
fn skin_strip_solver_iteration_count_stays_low() {
    let m = box_mesh([100.0, 10.0, 1.0]);
    let mid = mid_surface(&m, 1.0, 2.5).unwrap();
    let fixed = nodes_on_face(&m, 0, &mid, 1e-6);
    let tip = nodes_on_face(&m, 1, &mid, 1e-6);
    let share = 1.0 / tip.len() as f64;
    let nodes = mid.mesh.nodes.len();
    let p = ShellProblem {
        mesh: mid.mesh,
        thickness: 1.0,
        material: Material { young: 210_000.0, poisson: 0.3 },
        fixed,
        loads: tip.iter().map(|i| (*i, [0.0, 0.0, -share])).collect(),
    };
    let s = shell_static(&p).unwrap();
    // Measured: 297 with threshold incomplete Cholesky (10 902 with plain Jacobi).
    assert!(s.iterations <= 600, "{} iterations on {nodes} nodes", s.iterations);
}
