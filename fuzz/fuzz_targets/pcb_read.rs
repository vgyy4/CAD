//! The KiCad board reader and its outputs (ADR 0047) must never panic, hang or exhaust
//! memory on any input; small limits keep each run fast.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(src) = std::str::from_utf8(data) {
        let lim = gusset_pcb::Limits {
            sexpr: gusset_pcb::sexpr::Limits { max_bytes: 1 << 20, max_depth: 64, max_nodes: 100_000 },
            max_items: 2000,
        };
        if let Ok(b) = gusset_pcb::Board::read(src, &lim) {
            let _ = b.write();
            let _ = gusset_pcb::check::check(&b, 20);
            let _ = gusset_pcb::gerber::write_all(&b);
            let (plated, npth) = gusset_pcb::drill::holes(&b);
            let _ = gusset_pcb::drill::write(&b, &plated, true);
            let _ = gusset_pcb::drill::write(&b, &npth, false);
            let _ = gusset_pcb::board3d::board_profile(&b, &gusset_pcb::board3d::Options::default());
        }
    }
});
