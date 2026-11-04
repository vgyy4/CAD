#![no_main]
//! ACIS SAT reader (ADR 0039). Must never panic, and must stay within its limits.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = gusset_sat::Limits { max_records: 100_000, max_tokens: 1_000_000 };
    let _ = gusset_sat::to_step(data, "fuzz", &limits);
    // The same bytes behind a version 7 header, so the records are reached.
    let mut text = b"700 0 1 0\n@4 fuzz\n1 1e-06 1e-10\n".to_vec();
    text.extend_from_slice(data);
    let _ = gusset_sat::to_step(&text, "fuzz", &limits);
});
