//! Phase 3 board work on any board the reader takes (ADR 0047): net renames and merges,
//! pad net edits, zone fills (hatched ones included) and the stroke font must never
//! panic, hang or exhaust memory; small limits keep each run fast.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    let lim = gusset_pcb::Limits {
        sexpr: gusset_pcb::sexpr::Limits { max_bytes: 1 << 18, max_depth: 64, max_nodes: 20_000 },
        max_items: 300,
    };
    let Ok(b) = gusset_pcb::Board::read(src, &lim) else { return };
    for t in b.texts.iter().take(50) {
        let _ = gusset_pcb::font::strokes(t, &t.text);
    }
    let mut e = b.clone();
    if let Some(n) = b.nets.iter().find(|n| !n.name.is_empty()) {
        let _ = e.rename_net(&n.name, "FUZZ", &lim);
        if let Some(m) = b.nets.iter().rev().find(|m| !m.name.is_empty() && m.name != n.name) {
            let _ = e.clone().rename_net("FUZZ", &m.name, &lim);
        }
    }
    if let Some(f) = b.footprints.first()
        && let Some(p) = f.pads.first().and_then(|&i| b.pads.get(i))
    {
        let _ = e.set_pad_nets(&[(f.reference.clone(), p.number.clone())], Some("X"), &lim);
    }
    // Fills cost time with size: only small boards.
    let points: usize = b.zones.iter().map(|z| z.outline.len()).sum::<usize>() + b.pads.len() + b.tracks.len();
    if b.zones.len() <= 4 && points <= 200 {
        let rules = gusset_pcb::rules::Rules::for_board(&b);
        let _ = gusset_pcb::zones::fill(&b, &rules, None);
    }
});
