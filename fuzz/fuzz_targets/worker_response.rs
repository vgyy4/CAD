//! Import-worker response decode (`gusset_worker::decode_response`), the parent side of
//! the stdout protocol. The worker is assumed compromised, so its output is hostile.
//! Must never panic; anything accepted re-encodes and decodes to the same value.
#![no_main]

use gusset_worker::{decode_response, encode_response};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((result, report)) = decode_response(data) {
        let again = encode_response(&result, &report);
        match decode_response(&again) {
            Ok(decoded) => assert_eq!(decoded, (result, report)),
            Err(e) => panic!("a re-encoded response did not decode: {e}"),
        }
    }
});
