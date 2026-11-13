//! KiCad custom rules (`.kicad_dru`, ADR 0054): any text must parse to rules and warnings
//! without panic, deep recursion or large allocation, and every kept rule must evaluate.
#![no_main]

use gusset_pcb::dru::{CustomRules, Kind, Subject};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    let rules = CustomRules::parse(src);
    let a = Subject { net_class: "Default".into(), net_name: "GND".into(), kind: "track", layers: vec!["F.Cu".into()] };
    let b = Subject {
        net_class: "Power".into(),
        net_name: "VCC".into(),
        kind: "via",
        layers: vec!["F.Cu".into(), "B.Cu".into()],
    };
    for k in [Kind::Clearance, Kind::TrackWidth, Kind::ViaDiameter, Kind::HoleSize, Kind::HoleToHole, Kind::EdgeClearance]
    {
        let _ = rules.find(k, &a, Some(&b));
        let _ = rules.find(k, &a, None);
        let _ = rules.largest_min(k);
    }
});
