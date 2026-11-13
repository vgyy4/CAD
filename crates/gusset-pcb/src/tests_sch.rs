//! Schematics, symbol libraries, netlists, ERC and board updates (ADR 0047 phase 2a), on
//! hand-written files (no KiCad library data in the repository).
#![allow(clippy::float_cmp)] // places are exact multiples of the file's numbers

use crate::netlist::{Design, netlist};
use crate::sch::{LabelKind, Mirror, PinType, Schematic, SymbolLib};
use crate::sch_edit::Item;
use crate::sexpr;
use crate::{Board, Limits, erc};

/// A small library: a two-pin resistor, one derived from it, a two-unit amplifier with
/// shared power pins, a ground symbol and a power flag.
pub(crate) const LIB: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (pin_numbers (hide yes)) (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Footprint" "Pads:Two" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "R_0_1"
      (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "R_Big" (extends "R")
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "R_Big" (at 0 0 90) (effects (font (size 1.27 1.27)))))
  (symbol "AMP" (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 5.08 0) (effects (font (size 1.27 1.27))))
    (property "Value" "AMP" (at 0 -5.08 0) (effects (font (size 1.27 1.27))))
    (symbol "AMP_1_1"
      (polyline (pts (xy -5.08 5.08) (xy 5.08 0) (xy -5.08 -5.08) (xy -5.08 5.08)) (stroke (width 0.254) (type default)) (fill (type background)))
      (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin input line (at -7.62 -2.54 0) (length 2.54) (name "-" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
      (pin input line (at -7.62 2.54 0) (length 2.54) (name "+" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27))))))
    (symbol "AMP_2_1"
      (circle (center 0 0) (radius 5.08) (stroke (width 0.254) (type default)) (fill (type none)))
      (arc (start -1 1) (mid 0 1.4142) (end 1 1) (stroke (width 0.1) (type default)) (fill (type none)))
      (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "7" (effects (font (size 1.27 1.27)))))
      (pin input line (at -7.62 -2.54 0) (length 2.54) (name "-" (effects (font (size 1.27 1.27)))) (number "6" (effects (font (size 1.27 1.27)))))
      (pin input line (at -7.62 2.54 0) (length 2.54) (name "+" (effects (font (size 1.27 1.27)))) (number "5" (effects (font (size 1.27 1.27))))))
    (symbol "AMP_0_1"
      (pin power_in line (at 0 7.62 270) (length 2.54) (hide yes) (name "V+" (effects (font (size 1.27 1.27)))) (number "8" (effects (font (size 1.27 1.27)))))
      (pin power_in line (at 0 -7.62 90) (length 2.54) (hide yes) (name "V-" (effects (font (size 1.27 1.27)))) (number "4" (effects (font (size 1.27 1.27)))))))
  (symbol "GND" (power) (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "#PWR" (at 0 -6.35 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Value" "GND" (at 0 -3.81 0) (effects (font (size 1.27 1.27))))
    (symbol "GND_0_1" (polyline (pts (xy 0 0) (xy 0 -1.27) (xy 1.27 -1.27) (xy 0 -2.54) (xy -1.27 -1.27) (xy 0 -1.27)) (stroke (width 0) (type default)) (fill (type none))))
    (symbol "GND_1_1" (pin power_in line (at 0 0 270) (length 0) (hide yes) (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))))
  (symbol "PWR_FLAG" (power) (pin_numbers (hide yes)) (pin_names (offset 0) (hide yes)) (in_bom yes) (on_board yes)
    (property "Reference" "#FLG" (at 0 1.905 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Value" "PWR_FLAG" (at 0 3.81 0) (effects (font (size 1.27 1.27))))
    (symbol "PWR_FLAG_0_0" (pin power_out line (at 0 0 90) (length 0) (name "pwr" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))
    (symbol "PWR_FLAG_0_1" (polyline (pts (xy 0 0) (xy 0 1.27) (xy -1.016 1.905) (xy 0 2.54) (xy 1.016 1.905) (xy 0 1.27)) (stroke (width 0) (type default)) (fill (type none)))))
)
"##;

pub(crate) fn lib() -> SymbolLib {
    SymbolLib::read(LIB, &Limits::default()).unwrap()
}

/// Places `name` from [`LIB`] as `Hand:<name>`.
pub(crate) fn place(
    s: &mut Schematic,
    name: &str,
    at: [f64; 2],
    angle: f64,
    mirror: Mirror,
    unit: u32,
    r: &str,
) -> usize {
    let l = lib();
    let node = l.embedded_node(name, &format!("Hand:{name}")).unwrap();
    s.add_symbol(&node, &format!("Hand:{name}"), at, angle, mirror, unit, r, "demo", &[], &Limits::default()).unwrap()
}

/// R1 (100, 50) and R2 (110, 50) joined at pin 1 by a wire; R2 pin 2 labelled OUT; R1 pin
/// 2 to ground through a wire; a power flag on ground.
pub(crate) fn divider() -> Schematic {
    let lim = Limits::default();
    let mut s = Schematic::new_empty("divider").unwrap();
    place(&mut s, "R", [100.0, 50.0], 0.0, Mirror::None, 1, "R1");
    place(&mut s, "R", [110.0, 50.0], 0.0, Mirror::None, 1, "R2");
    place(&mut s, "GND", [100.0, 60.0], 0.0, Mirror::None, 1, "#PWR01");
    place(&mut s, "PWR_FLAG", [95.0, 60.0], 0.0, Mirror::None, 1, "#FLG01");
    s.add_wires(&[[100.0, 46.19], [110.0, 46.19]], &lim).unwrap();
    s.add_wires(&[[100.0, 53.81], [100.0, 60.0]], &lim).unwrap();
    s.add_wires(&[[95.0, 60.0], [100.0, 60.0]], &lim).unwrap();
    s.add_label(LabelKind::Local, "OUT", [110.0, 53.81], 0.0, None, &lim).unwrap();
    s
}

#[test]
fn library_symbols_read_and_embed() {
    let l = lib();
    assert_eq!(l.names().collect::<Vec<_>>(), ["R", "R_Big", "AMP", "GND", "PWR_FLAG"]);
    let r = l.symbols.first().unwrap();
    assert_eq!((r.prefix(), r.unit_count(), r.hide_pin_numbers), ("R".to_string(), 1, true));
    assert_eq!(r.pins_of(1, 1).count(), 2);
    let amp = l.symbols.iter().find(|s| s.name == "AMP").unwrap();
    // Two units, each with its own three pins and the two shared power pins.
    assert_eq!(amp.unit_count(), 2);
    let pins: Vec<&str> = amp.pins_of(2, 1).map(|p| p.number.as_str()).collect();
    assert_eq!(pins, ["7", "6", "5", "8", "4"]);
    assert!(amp.units.iter().any(|u| u.graphics.iter().any(|g| matches!(g.shape, crate::sch::Shape::Arc { .. }))));
    // A derived symbol takes its parent's drawings and pins and its own value.
    let node = l.embedded_node("R_Big", "Hand:R_Big").unwrap();
    let big = crate::sch::LibSymbol::from_node(&node, 1000).unwrap().unwrap();
    assert_eq!((big.name.as_str(), big.property("Value"), big.pins_of(1, 1).count()), ("Hand:R_Big", Some("R_Big"), 2));
    assert!(node.children("symbol").any(|s| s.arg(0) == Some("R_Big_1_1")), "{}", sexpr::write(&node));
    assert!(l.embedded_node("Nope", "x").is_none());
    // Written back unchanged.
    assert_eq!(sexpr::parse(&l.write(), &Default::default()).unwrap(), l.tree);
}

#[test]
fn a_schematic_round_trips_and_places_pins() {
    let s = divider();
    let back = Schematic::read(&s.write(), &Limits::default()).unwrap();
    assert_eq!(back.tree, s.tree);
    assert_eq!((back.symbols.len(), back.wires.len(), back.labels.len(), back.lib_symbols.len()), (4, 3, 1, 3));
    let r1 = &back.symbols[0];
    assert_eq!((r1.reference(), r1.value(), r1.footprint(), r1.lib_id.as_str()), ("R1", "R", "Pads:Two", "Hand:R"));
    assert_eq!(r1.instances[0].reference, "R1");
    let pins = back.pins(r1);
    assert_eq!(pins[0].at, [100.0, 46.19]);
    assert_eq!(pins[1].at, [100.0, 53.81]);
    // Turned a quarter (counter-clockwise on screen) pin 1 points left; mirrored about
    // Y it points right.
    let mut t = r1.clone();
    t.angle = 90.0;
    assert_eq!(t.place([0.0, 3.81]), [96.19, 50.0]);
    t.mirror = Mirror::Y;
    assert_eq!(t.place([0.0, 3.81]), [103.81, 50.0]);
    assert_eq!(t.orient_inverse(t.orient([1.0, 2.0])), [1.0, 2.0]);
    assert!(back.is_power(&back.symbols[2]) && !back.is_power(r1));
    // Junction dot: the GND wire meets the flag's wire at its end only, so no dot there.
    assert!(back.junctions.is_empty());
}

#[test]
fn nets_follow_wires_labels_and_power() {
    let s = divider();
    let nl = netlist(&Design::single("d.kicad_sch".into(), s));
    let names: Vec<&str> = nl.nets.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"GND") && names.contains(&"/OUT") && names.contains(&"Net-(R1-Pad1)"), "{names:?}");
    let pins = nl.pin_map();
    assert_eq!(pins[&("R1".into(), "1".into())], "Net-(R1-Pad1)");
    assert_eq!(pins[&("R2".into(), "1".into())], "Net-(R1-Pad1)");
    assert_eq!(pins[&("R1".into(), "2".into())], "GND");
    assert_eq!(pins[&("R2".into(), "2".into())], "/OUT");
    assert_eq!(nl.components.iter().map(|c| c.reference.as_str()).collect::<Vec<_>>(), ["R1", "R2"]);
    let gnd = nl.nets.iter().find(|n| n.name == "GND").unwrap();
    assert!(gnd.power_out && gnd.power_symbols == 2);
    // Every wire knows its net.
    assert!(nl.wire_nets[0].iter().all(Option::is_some));
    // ERC: only the label net's lone pin is fine (a label is a connection), so clean.
    let issues = erc::check(&Design::single("d.kicad_sch".into(), divider()), &nl, 100);
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn crossing_wires_join_only_at_junctions_and_ends() {
    let lim = Limits::default();
    let mut s = Schematic::new_empty("x").unwrap();
    s.add_wires(&[[0.0, 10.0], [20.0, 10.0]], &lim).unwrap();
    s.add_wires(&[[10.0, 0.0], [10.0, 20.0]], &lim).unwrap();
    s.add_label(LabelKind::Local, "A", [0.0, 10.0], 0.0, None, &lim).unwrap();
    s.add_label(LabelKind::Local, "B", [10.0, 0.0], 0.0, None, &lim).unwrap();
    let nl = netlist(&Design::single("x".into(), s.clone()));
    assert_eq!(nl.nets.len(), 2, "{:?}", nl.nets);
    // A wire ending on another's middle joins it, and gets a dot.
    s.add_wires(&[[5.0, 10.0], [5.0, 30.0]], &lim).unwrap();
    assert_eq!(s.junctions.len(), 1);
    assert_eq!(s.junctions[0].at, [5.0, 10.0]);
    s.add_junction([10.0, 10.0], &lim).unwrap();
    let nl = netlist(&Design::single("x".into(), s));
    assert_eq!(nl.nets.len(), 1);
    // Two labels on one net: the first name in order wins.
    assert_eq!(nl.nets[0].name, "/A");
}

/// A root sheet using `sub.kicad_sch` twice; the sub-sheet's R's pin 1 goes to the
/// hierarchical label IN, pin 2 to ground.
fn hierarchy() -> Design {
    let lim = Limits::default();
    let mut sub = Schematic::new_empty("sub").unwrap();
    place(&mut sub, "R", [50.0, 50.0], 0.0, Mirror::None, 1, "R10");
    place(&mut sub, "GND", [50.0, 60.0], 0.0, Mirror::None, 1, "#PWR10");
    sub.add_wires(&[[50.0, 53.81], [50.0, 60.0]], &lim).unwrap();
    sub.add_label(LabelKind::Hierarchical, "IN", [50.0, 46.19], 0.0, Some("input"), &lim).unwrap();
    let mut root = Schematic::new_empty("root").unwrap();
    let root_id = root.uuid.clone().unwrap();
    place(&mut root, "R", [20.0, 20.0], 0.0, Mirror::None, 1, "R1");
    place(&mut root, "GND", [20.0, 30.0], 0.0, Mirror::None, 1, "#PWR01");
    root.add_wires(&[[20.0, 23.81], [20.0, 30.0]], &lim).unwrap();
    // Two uses of the sub-sheet, their pins wired to R1 pin 1 and to a label.
    let sheet = |uuid: &str, name: &str, y: f64| {
        format!(
            r#"(sheet (at 40 {y}) (size 20 10) (uuid "{uuid}")
                 (property "Sheetname" "{name}" (at 40 {y} 0)) (property "Sheetfile" "sub.kicad_sch" (at 40 {y} 0))
                 (pin "IN" input (at 40 {} 180) (uuid "{uuid}-p")))"#,
            y + 5.0
        )
    };
    let items = root.tree.items_mut().unwrap();
    for (u, n, y) in [("aaaa", "left", 10.0), ("bbbb", "right", 40.0)] {
        items.push(sexpr::parse(&sheet(u, n, y), &Default::default()).unwrap());
    }
    let mut root = Schematic::from_tree(std::mem::take(&mut root.tree), &lim).unwrap();
    root.add_wires(&[[20.0, 16.19], [20.0, 15.0], [40.0, 15.0]], &lim).unwrap();
    root.add_label(LabelKind::Local, "B_IN", [40.0, 45.0], 0.0, None, &lim).unwrap();
    // The sub-sheet's symbol has a reference per use.
    let items = sub.tree.items_mut().unwrap();
    let sym = items.iter_mut().find(|n| n.head() == Some("symbol")).unwrap();
    let inst = sym.child_mut("instances").unwrap().child_mut("project").unwrap().items_mut().unwrap();
    inst.truncate(2);
    for (u, r) in [("aaaa", "R10"), ("bbbb", "R20")] {
        let p = format!(r#"(path "/{root_id}/{u}" (reference "{r}") (unit 1))"#);
        inst.push(sexpr::parse(&p, &Default::default()).unwrap());
    }
    let sub = Schematic::from_tree(std::mem::take(&mut sub.tree), &lim).unwrap();
    Design { sheets: vec![("root.kicad_sch".into(), root), ("sub.kicad_sch".into(), sub)] }
}

#[test]
fn hierarchical_sheets_join_through_sheet_pins() {
    let d = hierarchy();
    let uses = d.uses();
    assert_eq!(uses.len(), 3);
    assert_eq!((uses[1].name_path.as_str(), uses[2].name_path.as_str()), ("/left/", "/right/"));
    let nl = netlist(&d);
    let pins = nl.pin_map();
    assert_eq!(nl.components.iter().map(|c| c.reference.as_str()).collect::<Vec<_>>(), ["R1", "R10", "R20"]);
    // R1 pin 1 reaches R10 pin 1 through the left sheet; the hierarchical label names it.
    assert_eq!(pins[&("R1".into(), "1".into())], pins[&("R10".into(), "1".into())]);
    assert_eq!(pins[&("R1".into(), "1".into())], "/left/IN");
    // The right use's pin 1 has the root label B_IN.
    assert_eq!(pins[&("R20".into(), "1".into())], "/B_IN");
    assert_eq!(pins[&("R10".into(), "2".into())], "GND");
    assert_eq!(pins[&("R20".into(), "2".into())], "GND");
    // A sheet that uses itself is not followed.
    let mut d2 = d.clone();
    d2.sheets[1].1 = d2.sheets[0].1.clone();
    d2.sheets[0].1.sheets[0].file = "root.kicad_sch".into();
    assert!(d2.uses().len() < 10);
}

#[test]
fn bus_members_join_through_sheet_pins_and_buses() {
    use crate::netlist::bus_members;
    assert_eq!(bus_members("D[0..2]"), ["D0", "D1", "D2"]);
    assert_eq!(bus_members("A[3..1]"), ["A1", "A2", "A3"]);
    assert_eq!(bus_members("{SDA SCL}"), ["SDA", "SCL"]);
    assert_eq!(bus_members("I2C{SDA D[0..1]}"), ["I2C.SDA", "I2C.D0", "I2C.D1"]);
    assert!(bus_members("PLAIN").is_empty() && bus_members("X[0..99999]").is_empty());
    let lim = Limits::default();
    // Two sheets, each an R whose pins carry the local labels P0 and P1 (P = D or Q).
    let sub = |p: &str| {
        let mut s = Schematic::new_empty(p).unwrap();
        place(&mut s, "R", [50.0, 50.0], 0.0, Mirror::None, 1, if p == "D" { "R10" } else { "R20" });
        s.add_label(LabelKind::Local, &format!("{p}0"), [50.0, 46.19], 0.0, None, &lim).unwrap();
        s.add_label(LabelKind::Local, &format!("{p}1"), [50.0, 53.81], 0.0, None, &lim).unwrap();
        s.add_label(LabelKind::Hierarchical, &format!("{p}[0..1]"), [30.0, 30.0], 0.0, Some("input"), &lim).unwrap();
        s
    };
    // The root: sheet pins D[0..1] and Q[0..1] on one bus labelled B[0..1].
    let mut root = Schematic::new_empty("root").unwrap();
    let items = root.tree.items_mut().unwrap();
    for (u, f, pin, y) in [("aaaa", "d.kicad_sch", "D[0..1]", 10.0), ("bbbb", "q.kicad_sch", "Q[0..1]", 40.0)] {
        let t = format!(
            r#"(sheet (at 40 {y}) (size 20 10) (uuid "{u}")
                 (property "Sheetname" "{u}" (at 40 {y} 0)) (property "Sheetfile" "{f}" (at 40 {y} 0))
                 (pin "{pin}" input (at 40 {} 180) (uuid "{u}-p")))"#,
            y + 5.0
        );
        items.push(sexpr::parse(&t, &Default::default()).unwrap());
    }
    for (a, b) in [([40.0, 15.0], [30.0, 15.0]), ([30.0, 15.0], [30.0, 45.0]), ([30.0, 45.0], [40.0, 45.0])] {
        let t = format!("(bus (pts (xy {} {}) (xy {} {})))", a[0], a[1], b[0], b[1]);
        items.push(sexpr::parse(&t, &Default::default()).unwrap());
    }
    let mut root = Schematic::from_tree(std::mem::take(&mut root.tree), &lim).unwrap();
    root.add_label(LabelKind::Local, "B[0..1]", [30.0, 30.0], 0.0, None, &lim).unwrap();
    let d = Design {
        sheets: vec![
            ("root.kicad_sch".into(), root.clone()),
            ("d.kicad_sch".into(), sub("D")),
            ("q.kicad_sch".into(), sub("Q")),
        ],
    };
    let pins = netlist(&d).pin_map();
    // Member by member, in order, named by the root's bus label.
    assert_eq!(pins[&("R10".into(), "1".into())], "/B0");
    assert_eq!(pins[&("R20".into(), "1".into())], "/B0");
    assert_eq!(pins[&("R10".into(), "2".into())], "/B1");
    assert_eq!(pins[&("R20".into(), "2".into())], "/B1");
    // Without the bus the sheets stay apart.
    root.buses.clear();
    let d = Design {
        sheets: vec![
            ("root.kicad_sch".into(), root),
            ("d.kicad_sch".into(), sub("D")),
            ("q.kicad_sch".into(), sub("Q")),
        ],
    };
    let pins = netlist(&d).pin_map();
    assert_ne!(pins[&("R10".into(), "1".into())], pins[&("R20".into(), "1".into())]);
}

#[test]
fn erc_finds_the_simple_problems() {
    let lim = Limits::default();
    let mut s = Schematic::new_empty("e").unwrap();
    place(&mut s, "AMP", [50.0, 50.0], 0.0, Mirror::None, 1, "U1");
    place(&mut s, "AMP", [80.0, 50.0], 0.0, Mirror::None, 1, "U2");
    place(&mut s, "R", [20.0, 20.0], 0.0, Mirror::None, 1, "R1");
    place(&mut s, "R", [30.0, 20.0], 0.0, Mirror::None, 1, "R1");
    // Two outputs on one wire.
    s.add_wires(&[[57.62, 50.0], [87.62, 50.0]], &lim).unwrap();
    s.add_label(LabelKind::Local, "LOST", [0.0, 0.0], 0.0, None, &lim).unwrap();
    s.add_no_connect([42.38, 47.46], &lim).unwrap();
    let d = Design::single("e".into(), s);
    let nl = netlist(&d);
    let issues = erc::check(&d, &nl, 100);
    let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
    for c in ["pin_conflict", "pin_not_connected", "duplicate_reference", "label_dangling", "power_not_driven"] {
        assert!(codes.contains(&c), "{c}: {issues:?}");
    }
    // The flagged pin is not reported, the others of U1 are.
    assert!(!issues.iter().any(|i| i.message.contains("pin 3 of U1 ")), "{issues:?}");
    assert!(issues.iter().any(|i| i.message.contains("pin 2 of U1 ")));
    // The shared power pins of U1 and U2 join V+ and V- globally; nothing drives them.
    let vplus = nl.nets.iter().find(|n| n.name == "V+").unwrap();
    assert_eq!(vplus.nodes.len(), 2);
    assert!(vplus.nodes.iter().all(|n| n.kind == PinType::PowerIn));
    assert_eq!(erc::check(&d, &nl, 2).len(), 2);
}

#[test]
fn wires_follow_a_moved_symbol_and_fields_change() {
    let lim = Limits::default();
    let mut s = divider();
    let r2 = s.symbol_index("R2").unwrap();
    // R2 down by 10: the wire from R1 pin 1 keeps its row and a vertical segment
    // reaches R2 pin 1's new place (no diagonal).
    s.move_symbol(r2, [110.0, 60.0], 0.0, Mirror::None, &lim).unwrap();
    let w = &s.wires[0];
    assert_eq!((w.a, w.b), ([100.0, 46.19], [110.0, 46.19]));
    assert!(s.wires.iter().any(|w| (w.a, w.b) == ([110.0, 46.19], [110.0, 56.19])));
    // Still one net: R1 pin 1 and R2 pin 1.
    let d = Design::single("d.kicad_sch".into(), s.clone());
    let nl = netlist(&d);
    let net = nl.nets.iter().find(|n| n.nodes.iter().any(|p| p.reference == "R2" && p.pin == "1")).unwrap();
    assert!(net.nodes.iter().any(|p| p.reference == "R1" && p.pin == "1"), "{net:?}");
    // Fields: the value changes, a footprint is added (hidden), the reference follows into
    // the instances.
    s.set_field(r2, "Value", "4k7", &lim).unwrap();
    s.set_field(r2, "Footprint", "Resistor_SMD:R_0603_1608Metric", &lim).unwrap();
    s.set_field(r2, "Reference", "R9", &lim).unwrap();
    let r = &s.symbols[r2];
    assert_eq!(r.value(), "4k7");
    assert_eq!(r.footprint(), "Resistor_SMD:R_0603_1608Metric");
    assert_eq!(s.symbol_index("R9"), Some(r2));
    assert!(s.set_field(r2, "Reference", " ", &lim).is_err());
    assert!(s.set_field(r2, "Value", "a\nb", &lim).is_err());
    assert_eq!(Schematic::read(&s.write(), &lim).unwrap().tree, s.tree);
}

#[test]
fn edits_move_turn_mirror_and_delete() {
    let lim = Limits::default();
    let mut s = divider();
    let r2 = s.symbol_index("R2").unwrap();
    let ref_at = s.symbols[r2].properties[0].at;
    s.move_symbol(r2, [120.0, 50.0], 90.0, Mirror::None, &lim).unwrap();
    let r = &s.symbols[r2];
    assert_eq!((r.at, r.angle), ([120.0, 50.0], 90.0));
    // The reference field turned with the body about the symbol's place.
    let p = r.properties[0].at;
    assert!((p[0] - 120.0).abs() < 1e-9 && (p[1] - (50.0 - (ref_at[0] - 110.0))).abs() < 1e-9, "{p:?}");
    s.move_symbol(r2, [120.0, 50.0], 0.0, Mirror::X, &lim).unwrap();
    assert_eq!(s.symbols[r2].mirror, Mirror::X);
    assert_eq!(s.pins(&s.symbols[r2])[0].at, [120.0, 53.81]);
    assert!(s.move_symbol(r2, [0.0, 0.0], 45.0, Mirror::None, &lim).is_err());
    s.move_label(0, [5.0, 5.0], 180.0, &lim).unwrap();
    assert_eq!((s.labels[0].at, s.labels[0].angle), ([5.0, 5.0], 180.0));
    // Delete R2 and the label; the library symbol stays (R1 uses it).
    let n = s.delete(&[Item::parse(&s, "R2").unwrap(), Item::parse(&s, "label#0").unwrap()], &lim).unwrap();
    assert_eq!(n, 2);
    assert!(s.symbol_index("R2").is_none() && s.labels.is_empty());
    assert_eq!(s.lib_symbols.len(), 3);
    // Deleting the flag drops its library symbol.
    let flg = s.symbols.iter().position(|x| x.reference() == "#FLG01").unwrap();
    s.delete(&[Item::Symbol(flg)], &lim).unwrap();
    assert!(!s.lib_symbols.iter().any(|l| l.name == "Hand:PWR_FLAG"));
    assert!(Item::parse(&s, "wire#99").is_none() && Item::parse(&s, "nope#1").is_none());
    assert_eq!(s.next_reference("R"), "R2");
    // A two-unit part's second unit, and a unit it does not have.
    let u = place(&mut s, "AMP", [10.0, 10.0], 0.0, Mirror::None, 2, "U1");
    assert_eq!(s.symbols[u].unit, 2);
    let node = lib().embedded_node("AMP", "Hand:AMP").unwrap();
    assert!(s.add_symbol(&node, "Hand:AMP", [0.0, 0.0], 0.0, Mirror::None, 3, "U2", "demo", &[], &lim).is_err());
    assert!(s.add_label(LabelKind::Global, "bad\nname", [0.0, 0.0], 0.0, None, &lim).is_err());
    let g = s.add_label(LabelKind::Global, "VBUS", [1.0, 1.0], 0.0, Some("output"), &lim).unwrap();
    assert_eq!((s.labels[g].kind, s.labels[g].shape.as_deref()), (LabelKind::Global, Some("output")));
    // Still a file that reads the same.
    assert_eq!(Schematic::read(&s.write(), &lim).unwrap().tree, s.tree);
}

/// A board with R1 (pads 1, 2) and a stray footprint X9.
fn board() -> Board {
    let mut b = Board::new_two_layer(30.0, 20.0).unwrap();
    for (r, x) in [("R1", 10.0), ("X9", 20.0)] {
        let fp = format!(
            r#"(footprint "Pads:Two" (layer "F.Cu") (at {x} 10)
            (property "Reference" "{r}") (property "Value" "R")
            (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu"))
            (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu") (net 0 "")))"#
        );
        b.tree.items_mut().unwrap().push(sexpr::parse(&fp, &Default::default()).unwrap());
    }
    Board::from_tree(b.tree, &Limits::default()).unwrap()
}

const MOD: &str = r#"(footprint "Two" (version 20241229) (generator "hand") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -2 0) (layer "F.SilkS"))
  (property "Value" "Two" (at 0 2 0) (layer "F.Fab"))
  (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask"))
  (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask")))"#;

#[test]
fn the_board_follows_the_schematic() {
    let lim = Limits::default();
    let nl = netlist(&Design::single("d".into(), divider()));
    let mut b = board();
    let mut asked = Vec::new();
    let r = crate::sync::update_board(
        &mut b,
        &nl,
        &mut |fp| {
            asked.push(fp.to_string());
            Some(MOD.to_string())
        },
        &lim,
    )
    .unwrap();
    assert_eq!(asked, ["Pads:Two"]);
    assert_eq!(r.added, ["R2"]);
    assert_eq!(r.extra, ["X9"]);
    assert_eq!(r.pads_changed, 4);
    let net = |b: &Board, r: &str, pad: &str| {
        let f = &b.footprints[b.footprint_index(r).unwrap()];
        let p = f.pads.iter().map(|&k| &b.pads[k]).find(|p| p.number == pad).unwrap();
        b.net_name(&p.net).unwrap_or_default()
    };
    assert_eq!(net(&b, "R1", "1"), "Net-(R1-Pad1)");
    assert_eq!(net(&b, "R2", "1"), "Net-(R1-Pad1)");
    assert_eq!(net(&b, "R2", "2"), "/OUT");
    assert_eq!(net(&b, "R1", "2"), "GND");
    let r2 = &b.footprints[b.footprint_index("R2").unwrap()];
    assert_eq!((r2.lib_id.as_str(), r2.value.as_str()), ("Pads:Two", "R"));
    assert!(r2.at[0] > 30.0, "placed beside the board: {:?}", r2.at);
    // A second update changes nothing; the file reads back the same.
    let again = crate::sync::update_board(&mut b, &nl, &mut |_| None, &lim).unwrap();
    assert_eq!((again.pads_changed, again.added.len()), (0, 0));
    assert_eq!(Board::read(&b.write(), &lim).unwrap().tree, b.tree);
    // Without the footprint file, the part is reported.
    let mut b = board();
    let r = crate::sync::update_board(&mut b, &nl, &mut |_| None, &lim).unwrap();
    assert_eq!(r.not_found, [("R2".to_string(), "Pads:Two".to_string())]);
    assert!(b.add_footprint("(kicad_pcb)", "a:b", "R3", "1k", [0.0, 0.0], &lim).is_err());
}

#[test]
fn hostile_schematics_are_refused() {
    let lim = Limits::default();
    assert!(matches!(Schematic::read("(kicad_pcb)", &lim), Err(crate::sch::Error::WrongKind(_))));
    assert!(SymbolLib::read("(kicad_sch)", &lim).is_err());
    assert!(Schematic::read("(kicad_sch (wire", &lim).is_err());
    let few = Limits { max_items: 3, ..Limits::default() };
    let many = "(kicad_sch (junction (at 0 0)) (junction (at 1 0)) (junction (at 2 0)) (junction (at 3 0)))";
    assert_eq!(Schematic::read(many, &few).unwrap_err(), crate::sch::Error::TooMany("junctions"));
    // Nonsense values are skipped, not trusted.
    let odd = r#"(kicad_sch (symbol (lib_id "x:y") (at 1e300 0 0)) (wire (pts (xy nan 0) (xy 1 1)))
        (label "a" (at 1 2 0)) (symbol (lib_id "q:r") (at 1 1 33) (unit 99999999)))"#;
    let s = Schematic::read(odd, &lim).unwrap();
    assert_eq!((s.symbols.len(), s.wires.len(), s.labels.len()), (1, 0, 1));
    assert!(s.pins(&s.symbols[0]).is_empty());
    let d = Design::single("o".into(), s);
    let issues = erc::check(&d, &netlist(&d), 10);
    assert!(issues.iter().any(|i| i.code == "missing_symbol"));
}
