//! Import-worker request decode (`gusset_worker::decode_request`), the worker side of
//! the stdin protocol. Must never panic; anything accepted re-encodes to the same bytes.
#![no_main]

use gusset_worker::{decode_request, encode_request};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((format, payload)) = decode_request(data) {
        assert_eq!(encode_request(format, payload), data, "request frame is not canonical");
    }
});
