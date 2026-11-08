#![no_main]
//! Fusion design streams (`BulkStream.dat`, ADR 0039 amendment): `read_design` must never
//! panic, stay linear in the stream (security review S3) and keep to its placement cap.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let d = gusset_sat::fusion::read_design(data);
    assert!(d.bodies.len() <= 10_000);
    for x in &d.xrefs {
        assert!(x.instances.len() <= 10_000);
    }
});
