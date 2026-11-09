#![no_main]
//! SolidWorks part containers (OLE2 and the chunk container) and the feature history read
//! from them. Must never panic, and must stay within its limits.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = gusset_sldprt::Limits {
        max_streams: 256,
        max_stream_bytes: 1 << 20,
        max_total_bytes: 4 << 20,
        max_ratio: 2048,
    };
    let _ = gusset_sldprt::extract_xt(data, &limits);
    // The same bytes behind each container's opening bytes.
    let mut ole = vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    ole.extend_from_slice(data);
    let _ = gusset_sldprt::extract_xt(&ole, &limits);
    let mut chunks = vec![0, 0, 0, 0, 0, 0, 0, 4, 0x14, 0, 6, 0, 8, 0, 1, 2, 3, 4];
    chunks.extend_from_slice(data);
    let _ = gusset_sldprt::extract_xt(&chunks, &limits);
    let _ = gusset_sldprt::features::feature_tree(data, "fuzz.sldprt", &limits);
    let _ = gusset_sldprt::features::feature_tree(&ole, "fuzz.sldprt", &limits);
    let _ = gusset_sldprt::features::feature_tree(&chunks, "fuzz.sldprt", &limits);
    let _ = gusset_sldprt::read_assembly(data, &limits);
    let _ = gusset_sldprt::read_assembly(&chunks, &limits);
    let _ = gusset_sldprt::embedded_part(data, &limits, "VirtualComp/Part1");
    let _ = gusset_sldprt::embedded_part(&chunks, &limits, "VirtualComp/Part1");
});
