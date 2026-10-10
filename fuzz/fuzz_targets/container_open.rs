//! `container::open` must never panic, hang or allocate without bound on any input,
//! and everything it accepts must decode.
#![no_main]

use gusset_store::container;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(c) = container::open(data) {
        for r in c.records() {
            // A container that opened has only valid records.
            assert!(matches!(c.get(&r.hash), Ok(Some(_))));
        }
        assert!(container::verify(data).is_ok());
    }
});
