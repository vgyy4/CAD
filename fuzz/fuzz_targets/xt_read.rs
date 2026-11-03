#![no_main]
//! Parasolid XT reader (text and neutral binary). Must never panic, and must stay within
//! its limits.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = gusset_xt::Limits { max_nodes: 100_000, max_elements: 1_000_000 };
    let _ = gusset_xt::to_step(data, "fuzz", &limits);
    // The same bytes behind a text header, so the node stream is reached.
    let mut text = b"**PART2;SCH=SCH_1200000_12006;USFLD_SIZE=0;\n**END_OF_HEADER**\nT".to_vec();
    text.extend_from_slice(data);
    let _ = gusset_xt::to_step(&text, "fuzz", &limits);
});
