#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use super::*;
use crate::skin::{mid_surface, nodes_on_face};
use gusset_kernel::Mesh;

const STEEL: Material = Material { young: 210_000.0, poisson: 0.3 };
const RHO: f64 = 7.85e-9;
const TWO_PI: f64 = 2.0 * std::f64::consts::PI;

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

fn problem(mesh: ShellMesh, t: f64, fixed: Vec<u32>, modes: usize) -> ShellModalProblem {
    ShellModalProblem { mesh, thickness: t, material: STEEL, density: RHO, fixed, modes }
}

/// Euler-Bernoulli cantilever frequency (Hz) for root `beta_l`, length `l`, second moment `i`
/// and area `a`.
fn beam_f(beta_l: f64, l: f64, i: f64, a: f64) -> f64 {
    beta_l * beta_l / (TWO_PI * l * l) * libm::sqrt(STEEL.young * i / (RHO * a))
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b
}

fn strip_fixed(nx: usize, ny: usize) -> Vec<u32> {
    (0..=ny).map(|j| (j * (nx + 1)) as u32).collect()
}

#[test]
fn cantilever_strip_matches_euler_bernoulli() {
    let m = shell_modes(&problem(grid(100.0, 10.0, 20, 2), 1.0, strip_fixed(20, 2), 5)).unwrap();
    let f = &m.frequencies;
    assert!(f.windows(2).all(|w| w[0] <= w[1]));
    let (a, i_out, i_in) = (10.0, 10.0 / 12.0, 1000.0 / 12.0);
    let e1 = rel(f[0], beam_f(1.8751, 100.0, i_out, a));
    let e2 = rel(f[1], beam_f(4.6941, 100.0, i_out, a));
    assert!(e1 < 0.02, "f1 {} err {e1}", f[0]);
    assert!(e2 < 0.03, "f2 {} err {e2}", f[1]);
    // First in-plane bending: the mode closest to it moves mostly along y.
    let target = beam_f(1.8751, 100.0, i_in, a);
    let (k, _) = f.iter().enumerate().min_by(|x, y| rel(*x.1, target).total_cmp(&rel(*y.1, target))).unwrap();
    let e3 = rel(f[k], target);
    assert!(e3 < 0.03, "in-plane {} vs {target} err {e3}", f[k]);
    let shape = &m.shapes[k];
    let sy = shape.iter().map(|d| d[1].abs()).fold(0.0, f64::max);
    let sz = shape.iter().map(|d| d[2].abs()).fold(0.0, f64::max);
    assert!(sy > 10.0 * sz, "in-plane mode shape {sy} {sz}");
    // The first shape is a bending shape along z, normalised to 1.
    let big = m.shapes[0].iter().map(|d| d[2].abs()).fold(0.0, f64::max);
    assert!((big - 1.0).abs() < 1e-9);
    println!("strip: {f:?} iterations {}, errors {e1:.4} {e2:.4} {e3:.4}", m.iterations);
}

#[test]
fn clamped_square_plate_matches_the_classical_coefficient() {
    let (a, t, n) = (100.0, 1.0, 16usize);
    let w = n + 1;
    let fixed: Vec<u32> =
        (0..w * w).filter(|k| k % w == 0 || k % w == n || k / w == 0 || k / w == n).map(|k| k as u32).collect();
    let m = shell_modes(&problem(grid(a, a, n, n), t, fixed, 3)).unwrap();
    let d = STEEL.young * t.powi(3) / (12.0 * (1.0 - STEEL.poisson * STEEL.poisson));
    let exact = 35.99 / (TWO_PI * a * a) * libm::sqrt(d / (RHO * t));
    let e = rel(m.frequencies[0], exact);
    assert!(e < 0.03, "{} vs {exact} err {e}", m.frequencies[0]);
    println!("plate: {} vs {exact}, err {e:.4}", m.frequencies[0]);
}

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
fn strip_from_a_surface_mesh_has_the_same_first_frequency() {
    let m = box_mesh([100.0, 10.0, 1.0]);
    let mid = mid_surface(&m, 1.0, 5.0).unwrap();
    let fixed = nodes_on_face(&m, 0, &mid, 1e-6);
    let r = shell_modes(&problem(mid.mesh, 1.0, fixed, 2)).unwrap();
    let e = rel(r.frequencies[0], beam_f(1.8751, 100.0, 10.0 / 12.0, 10.0));
    assert!(e < 0.02, "{} err {e}", r.frequencies[0]);
    println!("skin strip: {} err {e:.4}", r.frequencies[0]);
}

#[test]
fn a_free_shell_is_refused() {
    let e = shell_modes(&problem(grid(100.0, 10.0, 10, 1), 1.0, Vec::new(), 2)).unwrap_err();
    assert!(e.to_string().contains("nothing is fixed"), "{e}");
}

#[test]
fn bad_inputs_are_refused() {
    let p = problem(grid(100.0, 10.0, 10, 1), 1.0, strip_fixed(10, 1), 11);
    assert!(shell_modes(&p).is_err());
    let p = ShellModalProblem { density: 0.0, modes: 1, ..p };
    assert!(shell_modes(&p).is_err());
}
