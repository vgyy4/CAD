//! Hierarchical sheets: creation, pins, deletion and moving, round trips, netlist and ERC
//! across the sheet boundary (ADR 0057).
#![allow(clippy::float_cmp)]

use crate::netlist::{Design, netlist};
use crate::sch::{LabelKind, Mirror, Schematic};
use crate::sch_edit::Item;
use crate::sch_sheet::Side;
use crate::tests_sch::place;
use crate::{Limits, erc};

/// A root with R1 (pin 1 at 100, 46.19) and a sheet `sub.kicad_sch` at (120, 40), 30 x 20.
fn root() -> Schematic {
    let lim = Limits::default();
    let mut s = Schematic::new_empty("root").unwrap();
    place(&mut s, "R", [100.0, 50.0], 0.0, Mirror::None, 1, "R1");
    let path = format!("/{}", s.uuid.clone().unwrap());
    s.add_sheet("Sub", "sub.kicad_sch", [120.0, 40.0], [30.0, 20.0], "root", &path, "2", &lim).unwrap();
    s
}

/// The sub-sheet: R2 with a hierarchical label SIG wired to its pin 1.
fn sub() -> Schematic {
    let lim = Limits::default();
    let mut s = Schematic::new_empty("sub.kicad_sch").unwrap();
    place(&mut s, "R", [50.0, 50.0], 0.0, Mirror::None, 1, "R2");
    s.add_wires(&[[50.0, 46.19], [50.0, 40.0]], &lim).unwrap();
    s.add_label(LabelKind::Hierarchical, "SIG", [50.0, 40.0], 0.0, Some("input"), &lim).unwrap();
    s
}

fn design(root: Schematic, sub: Schematic) -> Design {
    Design { sheets: vec![("root.kicad_sch".into(), root), ("sub.kicad_sch".into(), sub)] }
}

fn joined(d: &Design, a: &str, b: &str) -> bool {
    let nl = netlist(d);
    nl.nets.iter().any(|n| {
        n.nodes.iter().any(|p| p.reference == a && p.pin == "1")
            && n.nodes.iter().any(|p| p.reference == b && p.pin == "1")
    })
}

fn mismatches(d: &Design) -> Vec<String> {
    let nl = netlist(d);
    erc::check(d, &nl, 100).into_iter().filter(|i| i.code == "hier_label_mismatch").map(|i| i.message).collect()
}

#[test]
fn a_new_sheet_reads_back_and_leaves_the_rest_untouched() {
    let lim = Limits::default();
    let before = {
        let mut s = Schematic::new_empty("root").unwrap();
        place(&mut s, "R", [100.0, 50.0], 0.0, Mirror::None, 1, "R1");
        s.write()
    };
    let mut s = Schematic::read(&before, &lim).unwrap();
    let path = format!("/{}", s.uuid.clone().unwrap());
    let i =
        s.add_sheet("Power supply", "psu.kicad_sch", [120.0, 40.0], [30.0, 20.0], "root", &path, "2", &lim).unwrap();
    let sh = s.sheets.get(i).unwrap();
    assert_eq!(
        (sh.name.as_str(), sh.file.as_str(), sh.at, sh.size),
        ("Power supply", "psu.kicad_sch", [120.0, 40.0], [30.0, 20.0])
    );
    assert!(sh.uuid.is_some() && sh.pins.is_empty());
    // The written file reads back to the same text, and the symbol part is as it was.
    let text = s.write();
    assert_eq!(Schematic::read(&text, &lim).unwrap().write(), text);
    assert!(text.contains("(property \"Sheetfile\" \"psu.kicad_sch\""));
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("(project \"root\" (path \"/") && flat.contains("(page \"2\")"), "{flat}");
    // Deleting the sheet gives the original file back, byte for byte.
    assert_eq!(s.delete(&[Item::Sheet(i)], &lim).unwrap(), 1);
    assert!(s.sheets.is_empty());
    assert_eq!(s.write(), before);
    // Bad input is refused.
    let bad = |name: &str, file: &str, size: [f64; 2]| {
        s.clone().add_sheet(name, file, [0.0, 0.0], size, "p", "/x", "2", &lim).is_err()
    };
    assert!(bad("", "a.kicad_sch", [10.0, 10.0]));
    assert!(bad("n", "", [10.0, 10.0]));
    assert!(bad("n", "a.kicad_sch", [1.0, 10.0]));
    assert!(bad("n", "a.kicad_sch", [f64::NAN, 10.0]));
    assert!(bad("n\nx", "a.kicad_sch", [10.0, 10.0]));
    assert_eq!(Item::parse(&root(), "sheet#0"), Some(Item::Sheet(0)));
    assert_eq!(Item::parse(&root(), "sheet#1"), None);
}

#[test]
fn sheet_pins_are_added_on_every_edge_renamed_and_removed() {
    let lim = Limits::default();
    let mut s = root();
    let a = s.add_sheet_pin(0, "A", "input", Side::Left, None, &lim).unwrap();
    let b = s.add_sheet_pin(0, "B", "output", Side::Left, None, &lim).unwrap();
    let c = s.add_sheet_pin(0, "C", "bidirectional", Side::Right, Some(10.0), &lim).unwrap();
    let d = s.add_sheet_pin(0, "D", "passive", Side::Top, None, &lim).unwrap();
    let e = s.add_sheet_pin(0, "E", "tri_state", Side::Bottom, Some(5.08), &lim).unwrap();
    let pins = &s.sheets.first().unwrap().pins;
    let at = |i: usize| (pins.get(i).unwrap().at, pins.get(i).unwrap().angle, pins.get(i).unwrap().kind.as_str());
    // The sheet is at (120, 40), 30 x 20: left edge x 120, free steps 2.54 apart.
    assert_eq!(at(a), ([120.0, 42.54], 180.0, "input"));
    assert_eq!(at(b), ([120.0, 45.08], 180.0, "output"));
    assert_eq!(at(c), ([150.0, 50.0], 0.0, "bidirectional"));
    assert_eq!(at(d), ([122.54, 40.0], 90.0, "passive"));
    assert_eq!(at(e), ([125.08, 60.0], 270.0, "tri_state"));
    // Refusals: a name twice, a bad kind, an offset on the corner or past the edge.
    assert!(s.add_sheet_pin(0, "A", "input", Side::Right, None, &lim).is_err());
    assert!(s.add_sheet_pin(0, "X", "sideways", Side::Right, None, &lim).is_err());
    assert!(s.add_sheet_pin(0, "X", "input", Side::Right, Some(0.0), &lim).is_err());
    assert!(s.add_sheet_pin(0, "X", "input", Side::Right, Some(20.0), &lim).is_err());
    assert!(s.add_sheet_pin(1, "X", "input", Side::Right, None, &lim).is_err());
    // Rename keeps the place; a taken or missing name is refused.
    s.rename_sheet_pin(0, "A", "SIG", &lim).unwrap();
    let pins = &s.sheets.first().unwrap().pins;
    assert_eq!((pins.first().unwrap().name.as_str(), pins.first().unwrap().at), ("SIG", [120.0, 42.54]));
    assert!(s.rename_sheet_pin(0, "SIG", "B", &lim).is_err());
    assert!(s.rename_sheet_pin(0, "nope", "Z", &lim).is_err());
    s.remove_sheet_pin(0, "SIG", &lim).unwrap();
    assert!(s.remove_sheet_pin(0, "SIG", &lim).is_err());
    assert_eq!(s.sheets.first().unwrap().pins.len(), 4);
    // A freed slot is used again.
    let again = s.add_sheet_pin(0, "F", "input", Side::Left, None, &lim).unwrap();
    assert_eq!(s.sheets.first().unwrap().pins.get(again).unwrap().at, [120.0, 42.54]);
    // The file still round-trips.
    let text = s.write();
    assert_eq!(Schematic::read(&text, &lim).unwrap().write(), text);
}

#[test]
fn sheet_pins_join_hierarchical_labels_in_the_netlist_and_erc_finds_mismatches() {
    let lim = Limits::default();
    let mut r = root();
    r.add_sheet_pin(0, "SIG", "input", Side::Left, None, &lim).unwrap();
    // R1 pin 1 to the sheet pin at (120, 42.54).
    r.add_wires(&[[100.0, 46.19], [100.0, 42.54], [120.0, 42.54]], &lim).unwrap();
    let d = design(r.clone(), sub());
    assert!(joined(&d, "R1", "R2"));
    assert!(mismatches(&d).is_empty(), "{:?}", mismatches(&d));
    // The net takes the hierarchical label's name below the root.
    let nl = netlist(&d);
    let net = nl.nets.iter().find(|n| n.nodes.iter().any(|p| p.reference == "R2")).unwrap();
    assert!(net.name.contains("SIG"), "{}", net.name);
    // Renaming the pin breaks the join; ERC names both ends.
    let mut r2 = r.clone();
    r2.rename_sheet_pin(0, "SIG", "OTHER", &lim).unwrap();
    let d = design(r2, sub());
    assert!(!joined(&d, "R1", "R2"));
    let m = mismatches(&d);
    assert_eq!(m.len(), 2, "{m:?}");
    assert!(m.iter().any(|x| x.contains("sheet pin OTHER")) && m.iter().any(|x| x.contains("hierarchical label SIG")));
    // Renaming the label inside to match joins them again.
    let mut s2 = sub();
    s2.delete(&[Item::Label(0)], &lim).unwrap();
    s2.add_label(LabelKind::Hierarchical, "OTHER", [50.0, 40.0], 0.0, Some("input"), &lim).unwrap();
    let mut r3 = r.clone();
    r3.rename_sheet_pin(0, "SIG", "OTHER", &lim).unwrap();
    let d = design(r3, s2);
    assert!(joined(&d, "R1", "R2") && mismatches(&d).is_empty());
    // A pin removed leaves the label without partner.
    let mut r4 = r;
    r4.remove_sheet_pin(0, "SIG", &lim).unwrap();
    let d = design(r4, sub());
    assert!(!joined(&d, "R1", "R2"));
    assert_eq!(mismatches(&d).len(), 1);
}

#[test]
fn a_moved_sheet_takes_its_pins_and_wires_along_and_a_sheet_can_be_deleted() {
    let lim = Limits::default();
    let mut s = root();
    s.add_sheet_pin(0, "SIG", "input", Side::Left, None, &lim).unwrap();
    s.add_wires(&[[110.0, 42.54], [120.0, 42.54]], &lim).unwrap();
    s.move_items(&[Item::Sheet(0)], [5.08, 2.54], &lim).unwrap();
    let sh = s.sheets.first().unwrap();
    assert_eq!((sh.at, sh.size), ([125.08, 42.54], [30.0, 20.0]));
    assert_eq!(sh.pins.first().unwrap().at, [125.08, 45.08]);
    // The wire on the pin followed it, orthogonally.
    assert!(s.wires.iter().all(|w| (w.a[0] - w.b[0]).abs() < 1e-9 || (w.a[1] - w.b[1]).abs() < 1e-9));
    assert!(s.wires.iter().any(|w| w.a == [125.08, 45.08] || w.b == [125.08, 45.08]), "{:?}", s.wires);
    // The name and file fields moved too, and the file reads back.
    let text = s.write();
    let back = Schematic::read(&text, &lim).unwrap();
    assert_eq!(back.sheets.first().unwrap().at, [125.08, 42.54]);
    assert!(text.contains("(at 125.08 41.8284 0)"), "{text}");
    s.delete(&[Item::Sheet(0)], &lim).unwrap();
    assert!(s.sheets.is_empty());
}
