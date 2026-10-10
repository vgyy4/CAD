//! OBJ reader: polygons, index forms, objects/groups and malformed input.

#![allow(
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::single_range_in_vec_init,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gusset_meshio::{Format, ImportOptions, MeshIoError, NormalMode, detect_format, import_bytes};

fn import(s: &str) -> gusset_meshio::Result<gusset_meshio::Imported> {
    let o = ImportOptions { normals: NormalMode::Smooth, ..Default::default() };
    import_bytes(s.as_bytes(), Some(Format::Obj), &o)
}

fn area_and_normal_z(m: &gusset_kernel::Mesh) -> (f64, Vec<f64>) {
    let mut area = 0.0;
    let mut nz = Vec::new();
    for t in m.indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| m.positions[t[k] as usize].map(f64::from));
        let z = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        area += z / 2.0;
        nz.push(z);
    }
    (area, nz)
}

#[test]
fn concave_polygon_is_ear_clipped() {
    // L-shaped hexagon (concave), counter-clockwise, area 3.
    let s = "v 0 0 0\nv 2 0 0\nv 2 1 0\nv 1 1 0\nv 1 2 0\nv 0 2 0\nf 1 2 3 4 5 6\n";
    let imp = import(s).unwrap();
    let m = &imp.meshes[0].mesh;
    assert_eq!(m.triangle_count(), 4);
    let (area, nz) = area_and_normal_z(m);
    assert!((area - 3.0).abs() < 1e-9, "{area}");
    assert!(nz.iter().all(|z| *z > 0.0), "all triangles keep the polygon's winding: {nz:?}");
    assert!(imp.report.warnings.iter().all(|w| !w.contains("ear-clipped")));
}

#[test]
fn index_forms_and_negative_indices() {
    let s = "# comment\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\nf 1/1/1 2/1/1 3/1/1\nv 1 1 0\nf -3//1 -1//1 -2//1 # trailing comment\nf 2/1 4/1 3/1\n";
    let imp = import(s).unwrap();
    let m = &imp.meshes[0].mesh;
    assert_eq!(m.triangle_count(), 3);
    assert_eq!(m.positions.len(), 4);
    assert_eq!(imp.meshes[0].units_note, "OBJ has no units; assumed millimetres");
}

#[test]
fn objects_and_groups() {
    let s = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\no first\nf 1 2 3\no second\nf 1 2 4\nf 1 3 4\n";
    let imp = import(s).unwrap();
    let names: Vec<_> = imp.meshes.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["first", "second"]);
    assert_eq!(imp.meshes[1].mesh.triangle_count(), 2);
    // Per-object vertex compaction.
    assert_eq!(imp.meshes[0].mesh.positions.len(), 3);

    // Only groups: groups become objects, re-opened groups merge.
    let g = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\ng a\nf 1 2 3\ng b\nf 1 2 4\ng a\nf 1 3 4\n";
    let imp = import(g).unwrap();
    let names: Vec<_> = imp.meshes.iter().map(|m| (m.name.as_str(), m.mesh.triangle_count())).collect();
    assert_eq!(names, [("a", 2), ("b", 1)]);

    // Faces before any object name use the base name.
    let o = ImportOptions { name_hint: Some("part".into()), ..Default::default() };
    let imp = import_bytes(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", None, &o).unwrap();
    assert_eq!(imp.meshes[0].name, "part");
}

#[test]
fn ignored_statements_are_reported() {
    let s = "mtllib x.mtl\nusemtl red\ns 1\nv 0 0 0\nv 1 0 0\nv 0 1 0\nl 1 2\nfoo bar\nf 1 2 3\nf 1 2\n";
    let imp = import(s).unwrap();
    let w = imp.report.warnings.join("\n");
    assert!(w.contains("ignored 1 OBJ line/point"), "{w}");
    assert!(w.contains("ignored 1 unrecognised"), "{w}");
    assert!(w.contains("line 10: face with 2 corners ignored"), "{w}");
}

#[test]
fn malformed_obj() {
    let e = import("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 0 1 2\n").unwrap_err().to_string();
    assert!(e.contains("OBJ line 4") && e.contains("index 0"), "{e}");
    let e = import("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 9\n").unwrap_err().to_string();
    assert!(e.contains("references vertex 9 but only 3"), "{e}");
    let e = import("v 0 0 0\nf -1 -2 -3\n").unwrap_err().to_string();
    assert!(e.contains("before the first vertex"), "{e}");
    let e = import("v 0 0 0\nv 1 nan 0\n").unwrap_err();
    assert!(matches!(e, MeshIoError::NonFinite { nan: true, .. }), "{e}");
    let e = import("v 0 0 1e39\n").unwrap_err();
    assert!(matches!(e, MeshIoError::NonFinite { nan: false, .. }), "{e}");
    let e = import("v 0 0\n").unwrap_err().to_string();
    assert!(e.contains("three numbers"), "{e}");
    let e = import("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 x\n").unwrap_err().to_string();
    assert!(e.contains("bad face index"), "{e}");
    assert!(matches!(import("# nothing\n").unwrap_err(), MeshIoError::Empty { .. }));
}

#[test]
fn detection() {
    assert_eq!(detect_format(None, b"# blender\nv 0 0 0\nv 1 0 0\nf 1 2 3\n"), Some(Format::Obj));
    assert_eq!(detect_format(Some("a.OBJ"), b"anything"), Some(Format::Obj));
    assert_eq!(detect_format(None, b"hello world"), None);
}

#[test]
fn cancellation() {
    let flag = Arc::new(AtomicBool::new(true));
    let o = ImportOptions { cancel: Some(flag), ..Default::default() };
    let e = import_bytes(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", None, &o).unwrap_err();
    assert!(matches!(e, MeshIoError::Cancelled));
}
