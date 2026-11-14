#![no_main]
//! Solid Edge and Inventor part containers (OLE2 compound files), and the Inventor feature
//! histories read from their records. Must never panic, and must stay within its limits.

use libfuzzer_sys::fuzz_target;

/// Class ids (first 32 bits of the GUIDs) the feature reader knows, to give cut records a role.
const CLASSES: [u32; 14] = [
    0x2b48_a42b,
    0x9087_4d18,
    0xce52_df35,
    0x9087_4d11,
    0xce52_df3a,
    0xce52_df3b,
    0xf988_4c43,
    0x9087_4d26,
    0x9087_4d91,
    0x4521_21b6,
    0xa477_243b,
    0xcc0f_7521,
    0xf645_595c,
    0x2067_3244,
];

fuzz_target!(|data: &[u8]| {
    let limits =
        gusset_ole::Limits { max_streams: 256, max_stream_bytes: 1 << 20, max_total_bytes: 4 << 20, max_ratio: 2048 };
    let _ = gusset_ole::solidedge::extract_xt(data, &limits);
    let _ = gusset_ole::inventor::extract_sab(data, &limits);
    let _ = gusset_ole::inventor_features::feature_tree(data, "f.ipt", &limits);
    let _ = gusset_ole::solidedge_features::feature_tree(data, "f.par", &limits);
    // The same bytes behind the compound file signature.
    let mut ole = vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    ole.extend_from_slice(data);
    let _ = gusset_ole::solidedge::extract_xt(&ole, &limits);
    let _ = gusset_ole::inventor::extract_sab(&ole, &limits);
    let _ = gusset_ole::inventor_features::feature_tree(&ole, "f.ipt", &limits);
    let _ = gusset_ole::solidedge_features::feature_tree(&ole, "f.par", &limits);
    // The bytes as a Solid Edge object store, so the object and function readers run.
    let part = gusset_ole::fixtures::ole(&[("PSMcluster0", data), ("PSMtypetable", &[])]);
    let _ = gusset_ole::solidedge_features::feature_tree(&part, "f.par", &limits);
    // The bytes cut into records of a well-formed feature segment, so the record readers run:
    // each record is a length byte, a class byte and that many bytes.
    let mut recs: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut rest = &data[..data.len().min(8192)];
    while let [len, class, tail @ ..] = rest {
        let n = (usize::from(*len) % 96).min(tail.len());
        let (body, next) = tail.split_at(n);
        recs.push((usize::from(*class) % CLASSES.len(), body.to_vec()));
        rest = next;
        if recs.len() >= 512 {
            break;
        }
    }
    if !recs.is_empty() {
        let part = gusset_ole::fixtures::inventor_dc_part(&CLASSES, &recs, &[]);
        let _ = gusset_ole::inventor_features::feature_tree(&part, "f.ipt", &limits);
    }
});
