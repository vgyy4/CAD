//! Mesh utilities: welding, validation, degenerate removal, triangulation.

#![allow(
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::single_range_in_vec_init,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

mod common;

use common::{cube, cube_unwelded};
use gusset_kernel::Mesh;
use gusset_meshio::utils::{
    NormalMode, apply_normals, remove_degenerate_triangles, triangulate_polygon, validate, weld_mesh,
    weld_with_tolerance,
};

#[test]
fn validation_of_closed_open_and_broken_meshes() {
    let v = validate(&cube(1.0));
    assert!(v.is_watertight() && v.is_consistently_oriented() && v.can_become_solid());
    assert!((v.signed_volume - 1.0).abs() < 1e-12);
    assert_eq!(v.vertices, 8);

    // Display mesh with duplicated vertices validates the same (welded internally).
    assert_eq!(validate(&cube_unwelded(1.0)), v);

    // Remove the top face: 4 boundary edges.
    let mut open = cube(1.0);
    open.indices.drain(6..12);
    let v = validate(&open);
    assert_eq!(v.boundary_edges, 4);
    assert!(!v.is_watertight() && !v.can_become_solid());

    // Flip one triangle: 3 inconsistent edges.
    let mut flipped = cube(1.0);
    flipped.indices.swap(0, 1);
    let v = validate(&flipped);
    assert_eq!(v.inconsistent_edges, 3);
    assert!(!v.can_become_solid());

    // Inside-out cube: watertight and consistent but negative volume.
    let mut inv = cube(1.0);
    for t in inv.indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }
    let v = validate(&inv);
    assert!(v.is_watertight() && v.is_consistently_oriented() && v.signed_volume < 0.0 && !v.can_become_solid());

    // Three triangles on one edge: non-manifold.
    let fan = Mesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]],
        indices: vec![0, 1, 2, 1, 0, 3, 0, 1, 4],
        ..Default::default()
    };
    assert_eq!(validate(&fan).non_manifold_edges, 1);
}

#[test]
fn tolerance_weld_is_deterministic() {
    let p = [[0.0, 0.0, 0.0], [0.0005, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0004, 0.0], [1.0009, 0.0, 0.0]];
    let (w, remap) = weld_with_tolerance(&p, 0.001);
    assert_eq!(w, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    assert_eq!(remap, vec![0, 0, 1, 0, 1]);
    let (w2, remap2) = weld_with_tolerance(&p, 0.001);
    assert_eq!((w, remap), (w2, remap2));
    // exact fallback merges ±0
    let (w, _) = weld_with_tolerance(&[[0.0, -0.0, 0.0], [-0.0, 0.0, 0.0]], 0.0);
    assert_eq!(w.len(), 1);
}

#[test]
fn weld_mesh_and_normals() {
    let m = weld_mesh(&cube_unwelded(2.0), 0.0);
    assert_eq!(m.positions.len(), 8);
    assert_eq!(m.normals.len(), 8);
    assert_eq!(m.face_ranges, vec![0..12]);
    let mut c = m.clone();
    apply_normals(&mut c, NormalMode::Flat);
    assert_eq!(c.positions.len(), 36);
    let mut c = m;
    apply_normals(&mut c, NormalMode::Crease { angle_deg: 100.0 });
    assert_eq!(c.positions.len(), 8, "no crease sharper than 100° on a cube");
}

#[test]
fn crease_work_per_vertex_is_bounded() {
    // 20 000 triangles fanning around vertex 0 with wildly varying normals and a tiny
    // crease angle: the hub may split into at most 64 copies.
    let mut m = Mesh { positions: vec![[0.0, 0.0, 0.0]], ..Default::default() };
    for i in 0..20_000u32 {
        let a = i as f32 * 0.37;
        m.positions.push([a.cos(), a.sin(), (i % 7) as f32 - 3.0]);
        m.positions.push([(a + 1.3).cos(), (a + 1.3).sin(), (i % 5) as f32]);
        m.indices.extend_from_slice(&[0, 2 * i + 1, 2 * i + 2]);
    }
    let n_before = m.positions.len();
    apply_normals(&mut m, NormalMode::Crease { angle_deg: 0.01 });
    assert!(m.positions.len() <= n_before - 1 + 64, "{}", m.positions.len());
    assert_eq!(m.positions.len(), m.normals.len());
}

#[test]
fn degenerate_removal_keeps_ranges() {
    let mut m = cube(1.0);
    m.indices.extend_from_slice(&[0, 0, 1, 0, 1, 2]);
    m.face_ranges = vec![0..6, 6..12, 12..14];
    let removed = remove_degenerate_triangles(&mut m);
    assert_eq!(removed, 1);
    assert_eq!(m.face_ranges, vec![0..6, 6..12, 12..13]);
    assert_eq!(m.triangle_count(), 13);
}

#[test]
fn triangulation() {
    // Square: two triangles.
    let sq = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let (t, ok) = triangulate_polygon(&sq);
    assert!(ok);
    assert_eq!(t.len(), 2);
    // Concave "arrow" in the XZ plane, clockwise when viewed from +Y.
    let arrow = [[0.0, 0.0, 0.0], [2.0, 0.0, 1.0], [0.0, 0.0, 2.0], [0.5, 0.0, 1.0]];
    let (t, ok) = triangulate_polygon(&arrow);
    assert!(ok);
    assert_eq!(t.len(), 2);
    assert!(!t.contains(&[2, 3, 0]) || !t.contains(&[0, 1, 2]), "must not use the reflex diagonal: {t:?}");
    // Degenerate (all collinear) falls back to a fan.
    let line = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]];
    let (t, ok) = triangulate_polygon(&line);
    assert!(!ok);
    assert_eq!(t.len(), 2);
    assert_eq!(triangulate_polygon(&sq[..2]).0.len(), 0);
}
