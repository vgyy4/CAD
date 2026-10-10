#![no_main]
//! Auto-detecting import followed by re-export in every format and re-import: exports of
//! anything we accepted must be accepted again, and export must be deterministic.

use gusset_meshio::{ExportBody, ExportFormat, ImportOptions, Limits, export_to_vec, import_bytes};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let opts = ImportOptions {
        limits: Limits {
            max_triangles: 100_000,
            max_vertices: 300_000,
            max_decompressed_bytes: 64 << 20,
            ..Default::default()
        },
        ..Default::default()
    };
    let Ok(imp) = import_bytes(data, None, &opts) else { return };
    let bodies: Vec<ExportBody<'_>> =
        imp.meshes.iter().map(|m| ExportBody { name: &m.name, mesh: &m.mesh }).collect();
    for f in [ExportFormat::StlBinary, ExportFormat::StlAscii, ExportFormat::ThreeMf, ExportFormat::Obj { normals: true }]
    {
        let a = export_to_vec(&bodies, f).expect("imported meshes must be exportable").0;
        let b = export_to_vec(&bodies, f).expect("imported meshes must be exportable").0;
        assert_eq!(a, b, "export is not deterministic");
        // Re-import may legitimately fail only when everything collapsed (Empty).
        if let Err(e) = import_bytes(&a, None, &opts) {
            assert!(matches!(e, gusset_meshio::MeshIoError::Empty { .. }), "{f:?}: {e}");
        }
    }
});
