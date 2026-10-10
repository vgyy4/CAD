//! `container::salvage` (and the recovery-file slot scanner) must never panic on any
//! input, and every record it returns must match its hash.
#![no_main]

use gusset_store::{Hash, container, recovery};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = container::salvage(data);
    for r in &s.records {
        assert_eq!(Hash::of(&r.data), r.hash);
    }
    let _ = recovery::decode_slots(data);
});
