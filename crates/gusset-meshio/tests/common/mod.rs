//! Shared helpers for the integration tests: hand-built meshes and a tiny ZIP builder
//! that can also produce deliberately broken archives.

#![allow(
    dead_code,
    unreachable_pub,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::single_range_in_vec_init,
    reason = "integration-test helpers get the same allowances clippy.toml gives #[test] functions"
)]

use gusset_kernel::Mesh;

/// Axis-aligned cube `[0, s]^3` with outward-facing triangles, shared vertices.
pub fn cube(s: f32) -> Mesh {
    let p = vec![
        [0.0, 0.0, 0.0],
        [s, 0.0, 0.0],
        [s, s, 0.0],
        [0.0, s, 0.0],
        [0.0, 0.0, s],
        [s, 0.0, s],
        [s, s, s],
        [0.0, s, s],
    ];
    let idx = vec![
        0, 2, 1, 0, 3, 2, // bottom (-z)
        4, 5, 6, 4, 6, 7, // top (+z)
        0, 1, 5, 0, 5, 4, // front (-y)
        2, 3, 7, 2, 7, 6, // back (+y)
        1, 2, 6, 1, 6, 5, // right (+x)
        3, 0, 4, 3, 4, 7, // left (-x)
    ];
    Mesh { positions: p, normals: Vec::new(), indices: idx, face_ranges: vec![0..12], edges: Vec::new() }
}

/// The cube as a display mesh: every triangle has its own 3 vertices (like a kernel
/// tessellation with per-face normals).
pub fn cube_unwelded(s: f32) -> Mesh {
    let c = cube(s);
    let mut m = Mesh::default();
    for &i in &c.indices {
        m.positions.push(c.positions[i as usize]);
        m.normals.push([0.0, 0.0, 1.0]);
        m.indices.push(m.indices.len() as u32);
    }
    m.face_ranges = vec![0..12];
    m
}

/// Sorted, de-duplicated triangle corner positions: a representation-independent
/// fingerprint of a mesh's geometry.
pub fn tri_set(m: &Mesh) -> Vec<[[u32; 3]; 3]> {
    let mut v: Vec<[[u32; 3]; 3]> = m
        .indices
        .chunks_exact(3)
        .map(|t| {
            let mut c = [0usize; 3].map(|_| [0u32; 3]);
            for k in 0..3 {
                c[k] = m.positions[t[k] as usize].map(|x| if x == 0.0 { 0 } else { x.to_bits() });
            }
            // rotate so the smallest corner comes first (keeps winding)
            let r = (0..3).min_by_key(|&k| c[k]).unwrap();
            [c[r], c[(r + 1) % 3], c[(r + 2) % 3]]
        })
        .collect();
    v.sort();
    v
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// One entry for [`zip`].
pub struct ZEntry<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub deflate: bool,
    /// Override the declared uncompressed size (to build lying archives).
    pub declared_size: Option<u32>,
}

impl<'a> ZEntry<'a> {
    pub fn new(name: &'a str, data: &'a [u8]) -> Self {
        ZEntry { name, data, deflate: true, declared_size: None }
    }
}

/// Builds a ZIP archive. Deliberately simple and independent of the crate's writer.
pub fn zip(entries: &[ZEntry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for e in entries {
        let payload = if e.deflate { miniz_oxide::deflate::compress_to_vec(e.data, 6) } else { e.data.to_vec() };
        let method: u16 = if e.deflate { 8 } else { 0 };
        let crc = crc32(e.data);
        let usize_ = e.declared_size.unwrap_or(e.data.len() as u32);
        let off = out.len() as u32;
        let mut h = Vec::new();
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(&method.to_le_bytes());
        h.extend_from_slice(&[0; 4]);
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        h.extend_from_slice(&usize_.to_le_bytes());
        h.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(e.name.as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(&payload);

        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&method.to_le_bytes());
        cd.extend_from_slice(&[0; 4]);
        cd.extend_from_slice(&crc.to_le_bytes());
        cd.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        cd.extend_from_slice(&usize_.to_le_bytes());
        cd.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        cd.extend_from_slice(&[0; 12]);
        cd.extend_from_slice(&off.to_le_bytes());
        cd.extend_from_slice(e.name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&cd);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

pub const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#;

pub const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/></Types>"#;

/// A 3MF package with the standard rels/content types and the given root model XML.
pub fn package(model: &str) -> Vec<u8> {
    zip(&[
        ZEntry::new("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ZEntry::new("_rels/.rels", RELS.as_bytes()),
        ZEntry::new("3D/3dmodel.model", model.as_bytes()),
    ])
}

/// Unit tetrahedron object XML (outward winding) with the given id and name.
pub fn tetra_object(id: u32, name: &str) -> String {
    format!(
        r#"<object id="{id}" name="{name}" type="model"><mesh><vertices>
<vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="0" y="0" z="1"/>
</vertices><triangles>
<triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/><triangle v1="0" v2="3" v3="2"/><triangle v1="1" v2="2" v3="3"/>
</triangles></mesh></object>"#
    )
}

pub fn model(unit: &str, resources: &str, build: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="{unit}" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources>{resources}</resources><build>{build}</build></model>"#
    )
}
