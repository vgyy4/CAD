//! Code-view text parser (`gusset_doc::text::parse`). Must never panic on any input, and
//! every document it accepts must print to text that parses back to a document that
//! prints identically. That is the `parse(print(doc)) == doc` guarantee of `text.rs`,
//! checked on the printed text: documents hold source spans, so a document parsed
//! from hand-spaced text equals its round trip only up to spans.
#![no_main]

use gusset_doc::text;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    let Ok(doc) = text::parse(src) else { return };
    let _ = gusset_doc::validate::validate(&doc);
    let printed = text::print(&doc);
    match text::parse(&printed) {
        Ok(again) => {
            assert_eq!(text::print(&again), printed, "round trip changed the document {src:?}");
        }
        Err(e) => panic!("accepted {src:?}, but its canonical form did not parse: {e}\n{printed}"),
    }
});
