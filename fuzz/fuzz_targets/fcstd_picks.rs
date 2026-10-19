//! The FreeCAD document readers (ADR 0033) must never panic or hang on any XML, and
//! never return more than their cap of shapes.
#![no_main]

use gusset_commands::fcstd::{MAX_SHAPES, picks};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The first half is Document.xml, the rest GuiDocument.xml.
    let mid = data.len() / 2;
    let (doc, gui) = data.split_at(mid);
    if let Ok(p) = picks(doc, Some(gui)) {
        assert!(p.len() <= MAX_SHAPES);
    }
    let _ = picks(data, None);
});
