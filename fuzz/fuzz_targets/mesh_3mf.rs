#![no_main]
//! 3MF importer (ZIP container + XML). Must never panic, and must stay within limits.
//! Seed the corpus with files from `export_to_vec(.., ExportFormat::ThreeMf)`.

use gusset_meshio::{Format, ImportOptions, Limits, import_bytes};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = ImportOptions {
        limits: Limits {
            max_triangles: 1_000_000,
            max_vertices: 1_000_000,
            max_decompressed_bytes: 64 << 20,
            ..Default::default()
        },
        ..Default::default()
    };
    if let Ok(imp) = import_bytes(data, Some(Format::ThreeMf), &opts) {
        for m in &imp.meshes {
            assert!(m.mesh.indices.iter().all(|&i| (i as usize) < m.mesh.positions.len()));
        }
    }
});
