//! Export → import round trips, determinism and normal modes.

#![allow(
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::single_range_in_vec_init,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

mod common;

use common::{cube, cube_unwelded, tri_set};
use gusset_kernel::Mesh;
use gusset_meshio::{
    ExportBody, ExportFormat, Format, ImportOptions, NormalMode, detect_format, export_to_vec, import_bytes,
};

const ALL: [ExportFormat; 5] = [
    ExportFormat::StlBinary,
    ExportFormat::StlAscii,
    ExportFormat::ThreeMf,
    ExportFormat::Obj { normals: false },
    ExportFormat::Obj { normals: true },
];

fn format_of(f: ExportFormat) -> Format {
    match f {
        ExportFormat::StlBinary | ExportFormat::StlAscii => Format::Stl,
        ExportFormat::ThreeMf => Format::ThreeMf,
        ExportFormat::Obj { .. } => Format::Obj,
    }
}

/// A slightly irregular mesh with non-round coordinates to exercise float formatting.
fn odd_tetra() -> Mesh {
    Mesh {
        positions: vec![
            [0.1, -0.2, 0.3],
            [10.123_457, 1e-7, -0.0],
            [0.333_333_34, 7.777_777_7, 1.0e5],
            [-3.25, 2.5, 12.000_001],
        ],
        indices: vec![0, 1, 2, 0, 3, 1, 0, 2, 3, 1, 3, 2],
        ..Default::default()
    }
}

#[test]
fn odd_tetra_is_outward() {
    assert!(gusset_meshio::utils::validate(&odd_tetra()).can_become_solid());
}

#[test]
fn roundtrip_every_format_is_exact() {
    let c = cube(12.5);
    let t = odd_tetra();
    let bodies = [ExportBody { name: "Cube body", mesh: &c }, ExportBody { name: "Tetra", mesh: &t }];
    for f in ALL {
        let (bytes, rep) = export_to_vec(&bodies, f).unwrap();
        assert_eq!(rep.triangles, 16, "{f:?}");
        assert_eq!(detect_format(None, &bytes), Some(format_of(f)), "{f:?}");
        let imp = import_bytes(&bytes, None, &ImportOptions::default()).unwrap();
        assert_eq!(imp.format, format_of(f));
        match f {
            ExportFormat::StlBinary => {
                // Binary STL has a single unnamed solid.
                assert_eq!(imp.meshes.len(), 1);
                let mut all = tri_set(&c);
                all.extend(tri_set(&t));
                all.sort();
                assert_eq!(tri_set(&imp.meshes[0].mesh), all);
            }
            _ => {
                assert_eq!(imp.meshes.len(), 2, "{f:?}");
                let names: Vec<_> = imp.meshes.iter().map(|m| m.name.as_str()).collect();
                let expect_cube = if matches!(f, ExportFormat::Obj { .. }) { "Cube_body" } else { "Cube body" };
                assert_eq!(names, [expect_cube, "Tetra"], "{f:?}");
                assert_eq!(tri_set(&imp.meshes[0].mesh), tri_set(&c), "{f:?}");
                assert_eq!(tri_set(&imp.meshes[1].mesh), tri_set(&t), "{f:?}");
                assert!(imp.meshes[0].validation.can_become_solid(), "{f:?}");
                assert!(imp.meshes[1].validation.can_become_solid(), "{f:?}");
                assert!((imp.meshes[0].validation.signed_volume - 12.5f64.powi(3)).abs() < 1e-6);
            }
        }
        assert!(imp.report.warnings.is_empty(), "{f:?}: {:?}", imp.report.warnings);
        for m in &imp.meshes {
            assert_eq!(m.mesh.positions.len(), m.mesh.normals.len());
            assert_eq!(m.mesh.face_ranges.len(), 1);
            assert_eq!(m.mesh.face_ranges[0], 0..m.mesh.triangle_count() as u32);
        }
    }
}

#[test]
fn exports_are_deterministic() {
    let c = cube_unwelded(3.0);
    let t = odd_tetra();
    let bodies = [ExportBody { name: "a", mesh: &c }, ExportBody { name: "b & <c>", mesh: &t }];
    for f in ALL {
        let a = export_to_vec(&bodies, f).unwrap().0;
        let b = export_to_vec(&bodies, f).unwrap().0;
        assert_eq!(a, b, "{f:?}");
    }
}

#[test]
fn threemf_export_welds_display_meshes() {
    let c = cube_unwelded(1.0);
    let (bytes, rep) = export_to_vec(&[ExportBody { name: "c", mesh: &c }], ExportFormat::ThreeMf).unwrap();
    assert!(rep.warnings.is_empty());
    let imp = import_bytes(&bytes, Some(Format::ThreeMf), &ImportOptions::default()).unwrap();
    assert_eq!(imp.meshes[0].validation.vertices, 8);
    assert!(imp.meshes[0].validation.is_watertight());
    assert_eq!(imp.meshes[0].units_note, "3MF unit 'millimeter'");
    // Model XML uses the expected namespace, unit and name; ZIP entries are in fixed order.
    let s = String::from_utf8_lossy(&bytes);
    let ct = s.find("[Content_Types].xml").unwrap();
    let rels = s.find("_rels/.rels").unwrap();
    let model = s.find("3D/3dmodel.model").unwrap();
    assert!(ct < rels && rels < model);
}

#[test]
fn threemf_export_drops_collapsed_triangles_and_escapes_names() {
    let mut m = cube(1.0);
    m.indices.extend_from_slice(&[0, 0, 1]);
    let (bytes, rep) = export_to_vec(&[ExportBody { name: "a\"b<c>&'", mesh: &m }], ExportFormat::ThreeMf).unwrap();
    assert_eq!(rep.triangles, 12);
    assert_eq!(rep.warnings.len(), 1);
    let imp = import_bytes(&bytes, None, &ImportOptions::default()).unwrap();
    assert_eq!(imp.meshes[0].name, "a\"b<c>&'");
}

#[test]
fn export_rejects_invalid_meshes() {
    let mut bad = cube(1.0);
    bad.indices.push(99);
    bad.indices.extend_from_slice(&[0, 1]);
    let e = export_to_vec(&[ExportBody { name: "x", mesh: &bad }], ExportFormat::StlBinary).unwrap_err();
    assert!(e.to_string().contains("references vertex 99"), "{e}");

    let mut nan = cube(1.0);
    nan.positions[3][1] = f32::NAN;
    let e = export_to_vec(&[ExportBody { name: "n", mesh: &nan }], ExportFormat::ThreeMf).unwrap_err();
    assert!(e.to_string().contains("vertex 3 has a NaN"), "{e}");

    let mut ragged = cube(1.0);
    ragged.indices.pop();
    assert!(export_to_vec(&[ExportBody { name: "r", mesh: &ragged }], ExportFormat::Obj { normals: false }).is_err());
}

#[test]
fn normal_modes() {
    let c = cube(2.0);
    let (bytes, _) = export_to_vec(&[ExportBody { name: "c", mesh: &c }], ExportFormat::StlBinary).unwrap();
    let with = |normals| {
        let o = ImportOptions { normals, ..Default::default() };
        import_bytes(&bytes, None, &o).unwrap().meshes.remove(0).mesh
    };
    let flat = with(NormalMode::Flat);
    assert_eq!(flat.positions.len(), 36);
    let smooth = with(NormalMode::Smooth);
    assert_eq!(smooth.positions.len(), 8);
    let crease = with(NormalMode::Crease { angle_deg: 30.0 });
    assert_eq!(crease.positions.len(), 24); // each corner split into its 3 faces
    for m in [&flat, &crease] {
        // every normal is axis aligned and unit length
        for n in &m.normals {
            assert_eq!(n.iter().filter(|c| c.abs() == 1.0).count(), 1, "{n:?}");
        }
    }
    for m in [&flat, &smooth, &crease] {
        assert_eq!(tri_set(m), tri_set(&c));
        assert_eq!(m.normals.len(), m.positions.len());
    }
}

#[test]
fn import_file_uses_stem_as_name() {
    let dir = std::env::temp_dir().join(format!("gusset-meshio-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bracket.stl");
    let c = cube(1.0);
    let (bytes, _) = export_to_vec(&[ExportBody { name: "ignored", mesh: &c }], ExportFormat::StlBinary).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let imp = gusset_meshio::import_file(&path, &ImportOptions::default()).unwrap();
    assert_eq!(imp.meshes[0].name, "bracket");
    let small = ImportOptions {
        limits: gusset_meshio::Limits { max_file_bytes: 100, ..Default::default() },
        ..Default::default()
    };
    let e = gusset_meshio::import_file(&path, &small).unwrap_err();
    assert!(e.to_string().contains("exceeds the limit of 100"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Rough throughput check for large binary STL files. Run with
/// `cargo test --release -p gusset-meshio -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf_binary_stl_2m_triangles() {
    // A 1000 × 1000 grid surface → 2 M triangles.
    let n = 1000u32;
    let mut m = Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            m.positions.push([i as f32 * 0.1, j as f32 * 0.1, ((i * 7 + j * 13) % 17) as f32 * 0.01]);
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = j * (n + 1) + i;
            let b = a + 1;
            let c = a + n + 1;
            let d = c + 1;
            m.indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    let t0 = std::time::Instant::now();
    let (bytes, _) = export_to_vec(&[ExportBody { name: "grid", mesh: &m }], ExportFormat::StlBinary).unwrap();
    let t1 = std::time::Instant::now();
    let imp = import_bytes(&bytes, None, &ImportOptions::default()).unwrap();
    let t2 = std::time::Instant::now();
    let (tmf, _) = export_to_vec(&[ExportBody { name: "grid", mesh: &m }], ExportFormat::ThreeMf).unwrap();
    let t3 = std::time::Instant::now();
    let imp3 = import_bytes(&tmf, None, &ImportOptions::default()).unwrap();
    let t4 = std::time::Instant::now();
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 2_000_000);
    assert_eq!(imp3.meshes[0].mesh.triangle_count(), 2_000_000);
    println!(
        "2M tris: STL write {:?} ({} MB), STL read+weld+validate+normals {:?}; 3MF write {:?} ({} MB), 3MF read {:?}",
        t1 - t0,
        bytes.len() >> 20,
        t2 - t1,
        t3 - t2,
        tmf.len() >> 20,
        t4 - t3
    );
}
