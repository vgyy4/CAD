//! The OpenSCAD reader (ADR 0033) must never panic, hang or exhaust memory on any input;
//! small limits keep each run fast.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(src) = std::str::from_utf8(data) {
        let lim = gusset_scad::Limits { max_steps: 100_000, max_depth: 64, max_primitives: 1000, max_vector: 10_000 };
        let _ = gusset_scad::read_with(src, &lim);
    }
});
