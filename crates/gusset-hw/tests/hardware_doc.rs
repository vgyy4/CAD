//! CI drift check: `docs/HARDWARE.md` must match the registry.

use std::path::PathBuf;

#[test]
fn hardware_doc_is_up_to_date() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/HARDWARE.md");
    // Compare with LF line endings regardless of the checkout's conversion settings.
    let doc = std::fs::read_to_string(&path).expect("docs/HARDWARE.md must exist").replace("\r\n", "\n");
    let regenerated = gusset_hw::docgen::update_document(&doc).expect("markers must be well-formed");
    assert!(
        regenerated == doc,
        "docs/HARDWARE.md is out of date with crates/gusset-hw/src/registry.rs; \
         run `cargo run -p gusset-hw --bin gen-hardware-doc` and commit the result"
    );
}
