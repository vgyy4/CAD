#![no_main]
//! STL importer (binary and ASCII). Must never panic, and must stay within limits.

use gusset_meshio::{Format, ImportOptions, Limits, import_bytes};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = ImportOptions {
        limits: Limits { max_triangles: 1_000_000, ..Default::default() },
        ..Default::default()
    };
    if let Ok(imp) = import_bytes(data, Some(Format::Stl), &opts) {
        for m in &imp.meshes {
            assert_eq!(m.mesh.positions.len(), m.mesh.normals.len());
            assert!(m.mesh.indices.iter().all(|&i| (i as usize) < m.mesh.positions.len()));
        }
    }
});
