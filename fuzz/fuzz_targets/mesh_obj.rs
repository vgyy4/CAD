#![no_main]
//! OBJ importer. Must never panic, and must stay within limits.

use gusset_meshio::{Format, ImportOptions, Limits, import_bytes};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = ImportOptions {
        limits: Limits { max_triangles: 1_000_000, max_vertices: 1_000_000, ..Default::default() },
        ..Default::default()
    };
    if let Ok(imp) = import_bytes(data, Some(Format::Obj), &opts) {
        for m in &imp.meshes {
            assert!(m.mesh.indices.iter().all(|&i| (i as usize) < m.mesh.positions.len()));
        }
    }
});
