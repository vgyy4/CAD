//! `.gus` project decode (`gusset_project::decode` and `salvage`): the container plus
//! the history graph, metadata and every text record inside. Must never panic on any
//! input; anything that decodes must re-encode, and the re-encoded file must decode
//! again with the same metadata.
#![no_main]

use gusset_project::{decode, encode, salvage};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = salvage(data, 1_700_000_000);
    let Ok((history, meta)) = decode(data) else { return };
    let bytes = match encode(&history, &meta) {
        Ok(b) => b,
        Err(e) => panic!("a decoded project could not be re-encoded: {e}"),
    };
    match decode(&bytes) {
        Ok((_, meta2)) => assert_eq!(meta2, meta),
        Err(e) => panic!("a re-encoded project did not decode: {e}"),
    }
});
