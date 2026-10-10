//! 3MF reader on hand-written fixtures: transforms, units, components, multi-part
//! packages, and hostile packages (zip bombs, path traversal, XML bombs).

#![allow(
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::single_range_in_vec_init,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

mod common;

use common::{CONTENT_TYPES, RELS, ZEntry, model, package, tetra_object, zip};
use gusset_meshio::{Format, ImportOptions, Limits, MeshIoError, import_bytes};

fn import(b: &[u8]) -> gusset_meshio::Result<gusset_meshio::Imported> {
    import_bytes(b, Some(Format::ThreeMf), &ImportOptions::default())
}

fn bbox(m: &gusset_kernel::Mesh) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in &m.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

#[test]
fn units_and_item_transforms() {
    let xml = model(
        "inch",
        &tetra_object(1, "Tet"),
        r#"<item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 0 0"/><item objectid="1" transform="2 0 0 0 2 0 0 0 2 0 0 0"/>"#,
    );
    let imp = import(&package(&xml)).unwrap();
    assert_eq!(imp.meshes.len(), 2);
    assert_eq!(imp.meshes[0].name, "Tet");
    assert_eq!(imp.meshes[1].name, "Tet (2)");
    assert_eq!(imp.meshes[0].units_note, "3MF unit 'inch' converted to mm (×25.4)");
    let (lo, hi) = bbox(&imp.meshes[0].mesh);
    assert_eq!(lo, [254.0, 0.0, 0.0]);
    assert_eq!(hi, [279.4, 25.4, 25.4]);
    let (_, hi) = bbox(&imp.meshes[1].mesh);
    assert_eq!(hi, [50.8, 50.8, 50.8]);
    assert!(imp.meshes[1].validation.can_become_solid());
    assert!(imp.report.warnings.is_empty(), "{:?}", imp.report.warnings);

    for (unit, f) in [("micron", 0.001f32), ("centimeter", 10.0), ("meter", 1000.0), ("foot", 304.8)] {
        let xml = model(unit, &tetra_object(1, "t"), r#"<item objectid="1"/>"#);
        let imp = import(&package(&xml)).unwrap();
        assert_eq!(bbox(&imp.meshes[0].mesh).1, [f, f, f], "{unit}");
    }
    let xml = model("parsec", &tetra_object(1, "t"), r#"<item objectid="1"/>"#);
    assert!(import(&package(&xml)).unwrap_err().to_string().contains("unknown unit"));
}

#[test]
fn components_mirror_and_unnamed_objects() {
    let res = format!(
        r#"{}<object id="2"><components><component objectid="1" transform="-1 0 0 0 1 0 0 0 1 0 0 0"/><component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/></components></object>"#,
        tetra_object(1, "")
    );
    let imp = import(&package(&model("millimeter", &res, r#"<item objectid="2"/>"#))).unwrap();
    assert_eq!(imp.meshes.len(), 1);
    let m = &imp.meshes[0];
    assert_eq!(m.name, "Object 2");
    assert_eq!(m.mesh.face_ranges, vec![0..4, 4..8]);
    // The mirrored copy has its winding flipped back, so the volume stays positive.
    assert!((m.validation.signed_volume - 2.0 / 6.0).abs() < 1e-6, "{}", m.validation.signed_volume);
    assert_eq!(m.validation.inconsistent_edges, 0);
    let (lo, hi) = bbox(&m.mesh);
    assert_eq!((lo[0], hi[0]), (-1.0, 6.0));
}

#[test]
fn production_extension_parts() {
    let root = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06" requiredextensions="p">
<resources><object id="9" name="Assembly"><components><component p:path="/3D/Objects/part%201.model" objectid="1"/></components></object></resources>
<build><item objectid="9" p:UUID="0000"/></build></model>"#;
    let part = model("millimeter", &tetra_object(1, "Inner"), "");
    let pkg = zip(&[
        ZEntry::new("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ZEntry::new("_rels/.rels", RELS.as_bytes()),
        ZEntry::new("3D/3dmodel.model", root.as_bytes()),
        ZEntry { deflate: false, ..ZEntry::new("3D/Objects/part 1.model", part.as_bytes()) },
    ]);
    let imp = import(&pkg).unwrap();
    assert_eq!(imp.meshes[0].name, "Assembly");
    assert_eq!(imp.meshes[0].mesh.triangle_count(), 4);
    assert!(imp.report.warnings.is_empty(), "{:?}", imp.report.warnings);
}

#[test]
fn unknown_required_extension_and_missing_rels_warn() {
    let xml = r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:x="urn:example:ext" requiredextensions="x"><resources><object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object></resources><build><item objectid="1"/></build></model>"#;
    let pkg = zip(&[ZEntry::new("3D/3dmodel.model", xml.as_bytes())]);
    let imp = import(&pkg).unwrap();
    let w = imp.report.warnings.join("\n");
    assert!(w.contains("urn:example:ext"), "{w}");
    assert!(w.contains("no _rels/.rels"), "{w}");
    assert!(w.contains("no [Content_Types].xml"), "{w}");
}

#[test]
fn malformed_models() {
    let cases: &[(&str, &str)] = &[
        (
            r#"<object id="1"><mesh><vertices><vertex x="0" y="0" z="0"/></vertices><triangles><triangle v1="0" v2="0" v3="7"/></triangles></mesh></object>"#,
            "references vertex 7 but the object has 1 vertices",
        ),
        (r#"<object id="1"><mesh><vertices><vertex x="0" y="NaN" z="0"/></vertices></mesh></object>"#, "NaN"),
        (r#"<object id="1"><mesh><vertices><vertex x="0" z="0"/></vertices></mesh></object>"#, "lacks x, y or z"),
        (r#"<object><mesh/></object>"#, "object without an id"),
        (r#"<object id="1"/><object id="1"/>"#, "duplicate object id 1"),
        (r#"<object id="1"><mesh><vertices><vertex x="1" y="2" z="3" x="4"/></vertices></mesh></object>"#, "attribute"),
    ];
    for (res, needle) in cases {
        let e = import(&package(&model("millimeter", res, r#"<item objectid="1"/>"#))).unwrap_err().to_string();
        assert!(e.contains(needle), "{needle}: {e}");
    }
    let e = import(&package(&model("millimeter", &tetra_object(1, "t"), r#"<item objectid="5"/>"#)))
        .unwrap_err()
        .to_string();
    assert!(e.contains("missing object 5"), "{e}");
    let e = import(&package(&model("millimeter", &tetra_object(1, "t"), r#"<item objectid="1" transform="1 0 0"/>"#)))
        .unwrap_err()
        .to_string();
    assert!(e.contains("12 numbers"), "{e}");
    let e = import(&package("<notamodel/>")).unwrap_err().to_string();
    assert!(e.contains("expected <model>"), "{e}");
    let e = import(&package("<model><resources>")).unwrap_err().to_string();
    assert!(e.contains("3MF"), "{e}");
}

#[test]
fn component_cycles_and_depth() {
    let res = r#"<object id="1"><components><component objectid="2"/></components></object><object id="2"><components><component objectid="1"/></components></object>"#;
    let e = import(&package(&model("millimeter", res, r#"<item objectid="1"/>"#))).unwrap_err().to_string();
    assert!(e.contains("cycle"), "{e}");

    // A fan-out chain: 40 levels × 2 children would be 2^40 instances.
    let mut res = tetra_object(0, "leaf");
    for i in 1..=40 {
        res.push_str(&format!(
            r#"<object id="{i}"><components><component objectid="{p}"/><component objectid="{p}"/></components></object>"#,
            p = i - 1
        ));
    }
    let e = import(&package(&model("millimeter", &res, r#"<item objectid="40"/>"#))).unwrap_err();
    assert!(matches!(e, MeshIoError::LimitExceeded { .. }), "{e}");
}

#[test]
fn instancing_respects_triangle_limit() {
    let items: String = (0..10).map(|_| r#"<item objectid="1"/>"#).collect();
    let pkg = package(&model("millimeter", &tetra_object(1, "t"), &items));
    let o = ImportOptions { limits: Limits { max_triangles: 30, ..Default::default() }, ..Default::default() };
    let e = import_bytes(&pkg, None, &o).unwrap_err().to_string();
    assert!(e.contains("triangle count after instancing"), "{e}");
}

#[test]
fn zip_bomb_by_ratio() {
    // 8 MiB of spaces deflates to ~8 KiB: ratio ~1000:1.
    let mut xml = model("millimeter", &tetra_object(1, "t"), r#"<item objectid="1"/>"#).into_bytes();
    xml.extend(std::iter::repeat_n(b' ', 8 << 20));
    let pkg = zip(&[ZEntry::new("_rels/.rels", RELS.as_bytes()), ZEntry::new("3D/3dmodel.model", &xml)]);
    let e = import(&pkg).unwrap_err();
    assert!(e.to_string().contains("compression ratio"), "{e}");
}

#[test]
fn zip_bomb_with_lying_size() {
    // Declares 100 bytes but inflates to 2 MiB: inflation stops at the declared size.
    let mut xml = model("millimeter", &tetra_object(1, "t"), r#"<item objectid="1"/>"#).into_bytes();
    xml.extend(std::iter::repeat_n(b' ', 2 << 20));
    let pkg = zip(&[
        ZEntry::new("_rels/.rels", RELS.as_bytes()),
        ZEntry { declared_size: Some(100), ..ZEntry::new("3D/3dmodel.model", &xml) },
    ]);
    let e = import(&pkg).unwrap_err().to_string();
    assert!(e.contains("more than its declared size"), "{e}");
}

#[test]
fn decompressed_budget() {
    let xml = model("millimeter", &tetra_object(1, "t"), r#"<item objectid="1"/>"#);
    let o =
        ImportOptions { limits: Limits { max_decompressed_bytes: 300, ..Default::default() }, ..Default::default() };
    let e = import_bytes(&package(&xml), None, &o).unwrap_err().to_string();
    assert!(e.contains("total decompressed size"), "{e}");
}

#[test]
fn path_traversal_is_rejected() {
    let xml = model("millimeter", &tetra_object(1, "t"), r#"<item objectid="1"/>"#);
    for evil in ["../evil.model", "/etc/passwd", "a/../../b", "C:\\x", "3D\\..\\x"] {
        let pkg = zip(&[
            ZEntry::new("_rels/.rels", RELS.as_bytes()),
            ZEntry::new("3D/3dmodel.model", xml.as_bytes()),
            ZEntry::new(evil, b"x"),
        ]);
        assert!(matches!(import(&pkg).unwrap_err(), MeshIoError::UnsafePath { .. }), "{evil}");
    }
    for target in ["/../../outside.model", "file:///etc/passwd", "http://example.com/x.model", "//host/share/x"] {
        let rels = RELS.replace("/3D/3dmodel.model", target);
        let pkg = zip(&[ZEntry::new("_rels/.rels", rels.as_bytes()), ZEntry::new("3D/3dmodel.model", xml.as_bytes())]);
        assert!(matches!(import(&pkg).unwrap_err(), MeshIoError::UnsafePath { .. }), "{target}");
    }
    let rels = RELS.replace("Id=\"rel0\"", "Id=\"rel0\" TargetMode=\"External\"");
    let pkg = zip(&[ZEntry::new("_rels/.rels", rels.as_bytes()), ZEntry::new("3D/3dmodel.model", xml.as_bytes())]);
    assert!(matches!(import(&pkg).unwrap_err(), MeshIoError::UnsafePath { .. }));
    // p:path traversal in a component
    let res = r#"<object id="2"><components><component path="/../x.model" objectid="1"/></components></object>"#;
    let e = import(&package(&model("millimeter", res, r#"<item objectid="2"/>"#))).unwrap_err();
    assert!(matches!(e, MeshIoError::UnsafePath { .. }), "{e}");
}

#[test]
fn xml_bombs() {
    // Deep nesting.
    let deep = format!(
        r#"<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">{}{}</model>"#,
        "<a>".repeat(1000),
        "</a>".repeat(1000)
    );
    let e = import(&package(&deep)).unwrap_err().to_string();
    assert!(e.contains("XML nesting depth"), "{e}");
    // DTD / entity expansion is refused outright.
    let dtd =
        r#"<?xml version="1.0"?><!DOCTYPE model [<!ENTITY a "aaaaaaaa"><!ENTITY b "&a;&a;&a;&a;">]><model>&b;</model>"#;
    let e = import(&package(dtd)).unwrap_err().to_string();
    assert!(e.contains("DOCTYPE"), "{e}");
    // Too many attributes on one element.
    let attrs: String = (0..500).map(|i| format!(" a{i}=\"1\"")).collect();
    let many = model("millimeter", &format!("<object id=\"1\"{attrs}/>"), "");
    let e = import(&package(&many)).unwrap_err().to_string();
    assert!(e.contains("XML attributes on one element"), "{e}");
    // Huge attribute value.
    let big = model("millimeter", &format!("<object id=\"1\" name=\"{}\"/>", "n".repeat(100_000)), "");
    let e = import(&package(&big)).unwrap_err().to_string();
    assert!(e.contains("XML attribute length"), "{e}");
}

#[test]
fn corrupt_containers() {
    let pkg = package(&model("millimeter", &tetra_object(1, "t"), r#"<item objectid="1"/>"#));
    // Every truncation must fail cleanly (no panic).
    for cut in [0, 10, 22, 100, pkg.len() / 2, pkg.len() - 1] {
        assert!(import(&pkg[..cut]).is_err(), "cut {cut}");
    }
    // Bit flips in the payload are caught by CRC or inflate.
    let mut bad = pkg.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 0xff;
    assert!(import(&bad).is_err());
    // Too many entries.
    let o = ImportOptions { limits: Limits { max_zip_entries: 2, ..Default::default() }, ..Default::default() };
    let e = import_bytes(&pkg, None, &o).unwrap_err().to_string();
    assert!(e.contains("ZIP entry count 3"), "{e}");
}
