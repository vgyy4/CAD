#![no_main]
//! Solid Edge and Inventor part containers (OLE2 compound files). Must never panic, and must
//! stay within its limits.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits =
        gusset_ole::Limits { max_streams: 256, max_stream_bytes: 1 << 20, max_total_bytes: 4 << 20, max_ratio: 2048 };
    let _ = gusset_ole::solidedge::extract_xt(data, &limits);
    let _ = gusset_ole::inventor::extract_sab(data, &limits);
    // The same bytes behind the compound file signature.
    let mut ole = vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    ole.extend_from_slice(data);
    let _ = gusset_ole::solidedge::extract_xt(&ole, &limits);
    let _ = gusset_ole::inventor::extract_sab(&ole, &limits);
});
