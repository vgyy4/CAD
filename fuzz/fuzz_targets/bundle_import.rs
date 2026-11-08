#![no_main]
//! History bundles (`.gusb`, ADR 0044) read into a fresh history: must never panic, stay
//! within its limits, and leave the history unchanged when the bundle is refused.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = gusset_doc::Document::from_text("gusset 1\nunits mm\n") else { return };
    let ctx = gusset_history::Ctx::user(1);
    let mut h = gusset_history::History::new(&doc, &ctx);
    let limits = gusset_history::BundleLimits {
        max_bundle_bytes: 1 << 20,
        max_decompressed_bytes: 4 << 20,
        max_nodes: 1000,
        max_objects: 10_000,
        max_object_bytes: 1 << 20,
    };
    let before = h.nodes().count();
    let mut with_magic = b"GUSB\x01".to_vec();
    with_magic.extend_from_slice(data);
    for bytes in [data, &with_magic[..]] {
        if h.import_bundle(&ctx, bytes, &limits).is_err() {
            assert_eq!(h.nodes().count(), before);
        }
    }
});
