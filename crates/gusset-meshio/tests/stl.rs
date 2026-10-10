//! STL reader: ASCII/binary detection edge cases and malformed input.

#![allow(
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::single_range_in_vec_init,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

mod common;

use gusset_meshio::{Format, ImportOptions, Limits, MeshIoError, detect_format, import_bytes};

fn binary(header: &[u8], tris: &[[[f32; 3]; 3]]) -> Vec<u8> {
    let mut b = vec![0u8; 80];
    b[..header.len()].copy_from_slice(header);
    b.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in tris {
        b.extend_from_slice(&[0; 12]);
        for v in t {
            for c in v {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
        b.extend_from_slice(&[0, 0]);
    }
    b
}

const TRI: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
const TRI2: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn import(b: &[u8]) -> gusset_meshio::Result<gusset_meshio::Imported> {
    import_bytes(b, Some(Format::Stl), &ImportOptions::default())
}

#[test]
fn binary_starting_with_solid_is_binary() {
    let b = binary(b"solid exported by some CAD tool", &[TRI, TRI2]);
    assert_eq!(detect_format(None, &b), Some(Format::Stl));
    let imp = import(&b).unwrap();
    assert_eq!(imp.meshes.len(), 1);
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 2);
    assert_eq!(imp.meshes[0].name, "STL mesh");
    assert_eq!(imp.meshes[0].units_note, "STL has no units; assumed millimetres");
    // open surface: reported, not an error
    assert!(imp.report.warnings.iter().any(|w| w.contains("not watertight")), "{:?}", imp.report.warnings);
}

#[test]
fn ascii_variants() {
    let text = "\u{feff}  SOLID  part one \r\n facet normal 0 0 1\r\n  OUTER LOOP\r\n   vertex 0 0 0\r\n   vertex 1e0 0 0\r\n\t vertex 0 1.0 -0\r\n  endloop\r\n endfacet\r\nendsolid part one\r\n";
    let imp = import(text.as_bytes()).unwrap();
    assert_eq!(imp.meshes[0].name, "part one");
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 1);

    // facet without "normal" values, two solids, no names
    let two = "solid\nfacet\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\nsolid\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
    let o = ImportOptions { name_hint: Some("plate".into()), ..Default::default() };
    let imp = import_bytes(two.as_bytes(), None, &o).unwrap();
    let names: Vec<_> = imp.meshes.iter().map(|m| m.name.clone()).collect();
    assert_eq!(names, ["plate 1", "plate 2"]);
}

#[test]
fn ascii_quad_is_triangulated_and_missing_endsolid_is_a_warning() {
    let t = "solid q\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 1 1 0\nvertex 0 1 0\nendloop\nendfacet\n";
    let imp = import(t.as_bytes()).unwrap();
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 2);
    let w = imp.report.warnings.join("\n");
    assert!(w.contains("fan-triangulated"), "{w}");
    assert!(w.contains("no 'endsolid'"), "{w}");
}

#[test]
fn ascii_errors_have_locations() {
    let t = "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 zz 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
    let e = import(t.as_bytes()).unwrap_err().to_string();
    assert!(e.contains("STL line 5") && e.contains("expected a number"), "{e}");

    let t = "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 nan 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
    let e = import(t.as_bytes()).unwrap_err();
    assert!(matches!(e, MeshIoError::NonFinite { nan: true, .. }), "{e}");
    assert!(e.to_string().contains("facet 1"), "{e}");

    let t = "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nendloop\nendfacet\nendsolid\n";
    let e = import(t.as_bytes()).unwrap_err().to_string();
    assert!(e.contains("has 2 vertices"), "{e}");

    let e = import(b"solid x\nbogus\n").unwrap_err().to_string();
    assert!(e.contains("expected 'facet' or 'endsolid', found 'bogus'"), "{e}");
}

#[test]
fn binary_nan_reports_triangle_number() {
    let mut t = TRI;
    t[1][2] = f32::NAN;
    let b = binary(b"x", &[TRI2, t]);
    let e = import(&b).unwrap_err();
    assert_eq!(e.to_string(), "STL triangle 2 has a NaN coordinate");
    let mut t = TRI;
    t[0][0] = f32::INFINITY;
    let e = import(&binary(b"x", &[t])).unwrap_err();
    assert_eq!(e.to_string(), "STL triangle 1 has an infinite coordinate");
}

#[test]
fn truncated_and_trailing_binary() {
    let mut b = binary(b"x", &[TRI, TRI2]);
    b.truncate(b.len() - 10);
    let e = import(&b).unwrap_err().to_string();
    assert!(e.contains("declares 2 triangles") && e.contains("truncated"), "{e}");

    let mut b = binary(b"x", &[TRI, TRI2]);
    b.extend_from_slice(b"junk");
    let imp = import(&b).unwrap();
    assert!(imp.report.warnings.iter().any(|w| w.contains("4 trailing bytes")));

    let e = import(b"tiny").unwrap_err().to_string();
    assert!(e.contains("too small"), "{e}");
}

#[test]
fn huge_declared_count_is_rejected_without_allocating() {
    let mut b = binary(b"x", &[TRI]);
    b[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    let e = import(&b).unwrap_err();
    assert!(matches!(e, MeshIoError::LimitExceeded { .. }), "{e}");
    // Within the triangle limit but larger than the file: truncation error.
    b[80..84].copy_from_slice(&1_000_000u32.to_le_bytes());
    assert!(matches!(import(&b).unwrap_err(), MeshIoError::Malformed { .. }));
    // Custom limits apply.
    let o = ImportOptions { limits: Limits { max_triangles: 1, ..Default::default() }, ..Default::default() };
    let e = import_bytes(&binary(b"x", &[TRI, TRI2]), None, &o).unwrap_err();
    assert!(e.to_string().contains("triangle count 2 exceeds the limit of 1"), "{e}");
}

#[test]
fn degenerate_triangles_are_removed_and_reported() {
    let line = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    let point = [[5.0, 5.0, 5.0]; 3];
    let imp = import(&binary(b"x", &[TRI, line, point])).unwrap();
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 1);
    assert!(imp.report.warnings.iter().any(|w| w.contains("2 degenerate triangles removed")));

    let keep = ImportOptions { keep_degenerate: true, ..Default::default() };
    let imp = import_bytes(&binary(b"x", &[TRI, line]), None, &keep).unwrap();
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 2);
    assert_eq!(imp.meshes[0].validation.degenerate_triangles, 1);
}

#[test]
fn ascii_with_stray_control_byte_still_parses() {
    // Long enough to be >= 84 bytes, starts with solid, contains a DOS EOF marker.
    let mut t = b"solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid x\n".to_vec();
    t.push(0x1a);
    let imp = import(&t).unwrap();
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 1);
}

#[test]
fn empty_inputs() {
    assert!(matches!(import(b"solid empty\nendsolid empty\n").unwrap_err(), MeshIoError::Empty { .. }));
    assert!(matches!(import(&binary(b"x", &[])).unwrap_err(), MeshIoError::Empty { .. }));
    assert!(matches!(import_bytes(b"", None, &ImportOptions::default()).unwrap_err(), MeshIoError::UnknownFormat));
}
