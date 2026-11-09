#![no_main]
//! Fusion design streams (`BulkStream.dat`, ADR 0039 amendment): `read_design` must never
//! panic, stay linear in the stream (security review S3) and keep to its placement cap. The same
//! bytes, split into a MetaStream, a BulkStream and an ACIS blob, go through the feature
//! timeline reader (`fusion_tree`), which must refuse or return a tree without panicking.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let d = gusset_sat::fusion::read_design(data);
    assert!(d.bodies.len() <= 10_000);
    for x in &d.xrefs {
        assert!(x.instances.len() <= 10_000);
    }
    // The first two bytes say where the MetaStream ends and the third where the BulkStream does.
    let Some((head, rest)) = data.split_first_chunk::<3>() else { return };
    let cut = |at: usize, of: &[u8]| of.split_at(at.min(of.len()));
    let (meta, rest) = cut(usize::from(u16::from_le_bytes([head[0], head[1]])), rest);
    let (bulk, acis) = cut(usize::from(head[2]) * rest.len() / 255, rest);
    let files = vec![
        ("FusionAssetName[Active]/Design1/MetaStream.dat".to_string(), meta.to_vec()),
        ("FusionAssetName[Active]/Design1/BulkStream.dat".to_string(), bulk.to_vec()),
        ("FusionAssetName[Active]/Breps.BlobParts/BREP.f.smb".to_string(), acis.to_vec()),
    ];
    let _ = gusset_sat::fusion_tree::feature_tree(&files, "fuzz.f3d");
});
