//! The Specctra session reader (ADR 0047 phase 2b) must never panic, hang or exhaust
//! memory on any input, and laying what it reads on a board must fail cleanly at worst;
//! small limits keep each run fast.
#![no_main]

use libfuzzer_sys::fuzz_target;

const BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "fuzz")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "A")
  (gr_rect (start 0 0) (end 40 20) (layer "Edge.Cuts")))"#;

fuzz_target!(|data: &[u8]| {
    if let Ok(src) = std::str::from_utf8(data) {
        let lim = gusset_pcb::specctra::Limits { max_bytes: 1 << 20, max_depth: 64, max_nodes: 100_000 };
        let _ = gusset_pcb::specctra::parse(src, &lim);
        if let Ok(ses) = gusset_pcb::specctra::read_ses(src, &lim)
            && let Ok(mut b) = gusset_pcb::Board::read(BOARD, &gusset_pcb::Limits::default())
        {
            let rules = gusset_pcb::rules::Rules::for_board(&b);
            let limits = gusset_pcb::Limits { max_items: 5000, ..Default::default() };
            let _ = b.apply_session(&ses, &rules, true, &limits);
        }
    }
});
