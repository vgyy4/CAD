//! The KiCad schematic and symbol library readers, the netlist and the ERC (ADR 0047
//! phase 2a) must never panic, hang or exhaust memory on any input; small limits keep
//! each run fast.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(src) = std::str::from_utf8(data) {
        let lim = gusset_pcb::Limits {
            sexpr: gusset_pcb::sexpr::Limits { max_bytes: 1 << 20, max_depth: 64, max_nodes: 100_000 },
            max_items: 2000,
        };
        if let Ok(s) = gusset_pcb::sch::Schematic::read(src, &lim) {
            let _ = s.write();
            let design = gusset_pcb::netlist::Design::single("f.kicad_sch".into(), s);
            let nl = gusset_pcb::netlist::netlist(&design);
            let _ = gusset_pcb::erc::check(&design, &nl, 50);
        }
        if let Ok(l) = gusset_pcb::sch::SymbolLib::read(src, &lim) {
            let _ = l.write();
            for name in l.names().take(50) {
                let _ = l.embedded_node(name, "L:x");
            }
        }
    }
});
