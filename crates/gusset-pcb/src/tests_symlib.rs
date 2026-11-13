//! The symbol library index, search and preview (ADR 0057), on hand-written libraries.

use crate::Limits;
use crate::symlib::{index, load_symbol, search};

/// Two small libraries' worth of symbols with descriptions and keywords.
const PASSIVES: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Description" "Resistor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "ki_keywords" "R res resistor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "R_0_1" (rectangle (start -1 -2.5) (end 1 2.5) (stroke (width 0.25) (type default)) (fill (type none))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.8 270) (length 1.3) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.8 90) (length 1.3) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "R_Small" (extends "R")
    (property "Reference" "R" (at 1 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "R_Small" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Description" "Resistor, small symbol" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "ki_keywords" "R resistor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes))))
  (symbol "C" (in_bom yes) (on_board yes)
    (property "Reference" "C" (at 2 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "C" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Description" "Unpolarized capacitor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "ki_keywords" "cap capacitor" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "C_1_1"
      (pin passive line (at 0 3.8 270) (length 3) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.8 90) (length 3) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27))))))))
"##;

const CHIPS: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "Dual_Opamp" (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 5 0) (effects (font (size 1.27 1.27))))
    (property "Value" "Dual_Opamp" (at 0 -5 0) (effects (font (size 1.27 1.27))))
    (property "Description" "Dual operational amplifier" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "ki_keywords" "dual opamp" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "Dual_Opamp_1_1"
      (polyline (pts (xy -5 5) (xy 5 0) (xy -5 -5) (xy -5 5)) (stroke (width 0.25) (type default)) (fill (type background)))
      (pin output line (at 7.5 0 180) (length 2.5) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))
    (symbol "Dual_Opamp_2_1"
      (circle (center 0 0) (radius 5) (stroke (width 0.25) (type default)) (fill (type none)))
      (pin output line (at 7.5 0 180) (length 2.5) (name "~" (effects (font (size 1.27 1.27)))) (number "7" (effects (font (size 1.27 1.27)))))))
  (symbol "Opamp_Single" (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 5 0) (effects (font (size 1.27 1.27))))
    (property "Value" "Opamp_Single" (at 0 -5 0) (effects (font (size 1.27 1.27))))
    (property "Description" "Single resistor-less amplifier" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))))
"##;

fn libs() -> Vec<crate::symlib::LibraryInfo> {
    let lim = Limits::default();
    vec![index("Passives.kicad_sym", PASSIVES, &lim).unwrap(), index("Chips.kicad_sym", CHIPS, &lim).unwrap()]
}

fn names(l: &[crate::symlib::LibraryInfo], hits: &[(usize, usize)]) -> Vec<String> {
    hits.iter()
        .map(|&(a, b)| {
            let lib = l.get(a).unwrap();
            format!("{}:{}", lib.nickname(), lib.symbols.get(b).unwrap().name)
        })
        .collect()
}

#[test]
fn the_index_lists_names_descriptions_keywords_and_units() {
    let l = libs();
    let p = l.first().unwrap();
    assert_eq!((p.nickname(), p.truncated), ("Passives", false));
    let r = p.symbols.first().unwrap();
    assert_eq!((r.name.as_str(), r.description.as_str(), r.keywords.as_str()), ("R", "Resistor", "R res resistor"));
    // The derived symbol takes its parent's one unit.
    assert_eq!(p.symbols.iter().find(|s| s.name == "R_Small").unwrap().units, 1);
    let amp = l.get(1).unwrap().symbols.first().unwrap();
    assert_eq!((amp.name.as_str(), amp.units), ("Dual_Opamp", 2));
    // A file that is not a symbol library is refused, not indexed.
    assert!(index("x.kicad_sym", "(kicad_pcb)", &Limits::default()).is_err());
    assert!(index("x.kicad_sym", "(kicad_symbol_lib (symbol", &Limits::default()).is_err());
}

#[test]
fn search_matches_name_description_and_keywords_and_ranks_names_first() {
    let l = libs();
    // Every word must match somewhere; the exact name comes first.
    let hits = search(&l, "r", 100);
    assert_eq!(names(&l, &hits).first().map(String::as_str), Some("Passives:R"));
    let hits = search(&l, "resistor", 100);
    // Name hits before description hits: none has "resistor" in the name, so file order.
    assert_eq!(names(&l, &hits), ["Passives:R", "Passives:R_Small", "Chips:Opamp_Single"]);
    // Case does not matter; several words narrow.
    assert_eq!(names(&l, &search(&l, "RESISTOR small", 100)), ["Passives:R_Small"]);
    assert_eq!(names(&l, &search(&l, "cap", 100)), ["Passives:C"]);
    // Keywords and the library name count too.
    assert_eq!(names(&l, &search(&l, "dual", 100)), ["Chips:Dual_Opamp"]);
    assert_eq!(names(&l, &search(&l, "chips amp", 100)).len(), 2);
    // A name prefix beats a name that merely contains the text.
    let hits = names(&l, &search(&l, "opamp", 100));
    assert_eq!(hits, ["Chips:Opamp_Single", "Chips:Dual_Opamp"]);
    // Nothing matches; empty lists all, bounded by max.
    assert!(search(&l, "zzz", 100).is_empty());
    assert_eq!(search(&l, "", 100).len(), 5);
    assert_eq!(search(&l, "  ", 2).len(), 2);
}

#[test]
fn a_symbol_loads_for_the_preview_with_its_units_and_bounds() {
    let lim = Limits::default();
    let amp = load_symbol(CHIPS, "Dual_Opamp", &lim).unwrap();
    assert_eq!(amp.unit_count(), 2);
    let b = amp.bounds(1, 1).unwrap();
    assert_eq!(b, [[-5.0, -5.0], [7.5, 5.0]]);
    assert_eq!(amp.bounds(2, 1).unwrap(), [[-5.0, -5.0], [7.5, 5.0]]);
    // The preview symbol clamps the unit and writes the reference with `?`.
    let s = amp.preview(9);
    assert_eq!(s.unit, 2);
    assert_eq!(s.property("Reference"), Some("U?"));
    // A derived symbol resolves to its parent's drawings.
    let small = load_symbol(PASSIVES, "R_Small", &lim).unwrap();
    assert_eq!(small.pins_of(1, 1).count(), 2);
    assert_eq!(small.property("Value"), Some("R_Small"));
    // A missing symbol is an error; an empty one has no bounds.
    assert!(load_symbol(PASSIVES, "Nope", &lim).is_err());
    let empty = load_symbol(CHIPS, "Opamp_Single", &lim).unwrap();
    assert!(empty.bounds(1, 1).is_none());
}

#[test]
fn a_library_past_the_symbol_cap_is_cut() {
    let mut src = String::from("(kicad_symbol_lib (version 20241209)");
    for i in 0..(crate::symlib::MAX_SYMBOLS_PER_LIB + 5) {
        src.push_str(&format!("(symbol \"S{i}\")"));
    }
    src.push(')');
    let i = index("Big.kicad_sym", &src, &Limits::default()).unwrap();
    assert_eq!((i.symbols.len(), i.truncated), (crate::symlib::MAX_SYMBOLS_PER_LIB, true));
}
