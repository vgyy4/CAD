//! Orthogonal wire following and moving several items (ADR 0052).
#![allow(clippy::float_cmp)]

use crate::Limits;
use crate::netlist::{Design, netlist};
use crate::sch::{Mirror, Schematic};
use crate::sch_edit::Item;
use crate::tests_sch::{divider, place};

fn orthogonal(s: &Schematic) -> bool {
    s.wires.iter().all(|w| (w.a[0] - w.b[0]).abs() < 1e-9 || (w.a[1] - w.b[1]).abs() < 1e-9)
}

fn joined(s: &Schematic, a: (&str, &str), b: (&str, &str)) -> bool {
    let nl = netlist(&Design::single("d.kicad_sch".into(), s.clone()));
    nl.nets.iter().any(|n| {
        n.nodes.iter().any(|p| p.reference == a.0 && p.pin == a.1)
            && n.nodes.iter().any(|p| p.reference == b.0 && p.pin == b.1)
    })
}

/// Whether a power symbol sits on the net of a pin.
fn powered(s: &Schematic, r: &str, pin: &str) -> bool {
    let nl = netlist(&Design::single("d.kicad_sch".into(), s.clone()));
    nl.nets.iter().any(|n| n.power_symbols > 0 && n.nodes.iter().any(|p| p.reference == r && p.pin == pin))
}

#[test]
fn a_moved_symbol_keeps_its_wires_orthogonal() {
    let lim = Limits::default();
    let mut s = divider();
    let r2 = s.symbol_index("R2").unwrap();
    // Diagonal move: the horizontal wire slides to the new column, a vertical one follows.
    s.move_symbol(r2, [115.0, 60.0], 0.0, Mirror::None, &lim).unwrap();
    assert!(orthogonal(&s), "{:?}", s.wires);
    assert!(s.wires.iter().any(|w| (w.a, w.b) == ([100.0, 46.19], [115.0, 46.19])), "{:?}", s.wires);
    assert!(s.wires.iter().any(|w| (w.a, w.b) == ([115.0, 46.19], [115.0, 56.19])));
    assert!(joined(&s, ("R1", "1"), ("R2", "1")));
    // A vertical wire (R1 pin 2 down to ground) moved sideways: the same the other way.
    let r1 = s.symbol_index("R1").unwrap();
    s.move_symbol(r1, [95.0, 50.0], 0.0, Mirror::None, &lim).unwrap();
    assert!(orthogonal(&s), "{:?}", s.wires);
    assert!(powered(&s, "R1", "2"));
    assert!(joined(&s, ("R1", "1"), ("R2", "1")));
}

#[test]
fn junctions_stay_and_appear_where_wires_meet() {
    let lim = Limits::default();
    let mut s = divider();
    let r2 = s.symbol_index("R2").unwrap();
    // Wires from above and to the right end on R2 pin 1 with the one from R1: a junction.
    s.add_wires(&[[110.0, 40.0], [110.0, 46.19], [120.0, 46.19]], &lim).unwrap();
    assert_eq!(s.junctions.len(), 1);
    s.move_symbol(r2, [110.0, 60.0], 0.0, Mirror::None, &lim).unwrap();
    assert!(orthogonal(&s));
    assert!(s.junctions.iter().any(|j| j.at == [110.0, 46.19]), "{:?}", s.junctions);
    assert!(joined(&s, ("R1", "1"), ("R2", "1")));
    // The wires from above and from R1 still meet R2 pin 1: one net.
    let nl = netlist(&Design::single("d.kicad_sch".into(), s.clone()));
    assert_eq!(nl.nets.iter().filter(|n| n.nodes.iter().any(|p| p.reference == "R2" && p.pin == "1")).count(), 1);
}

#[test]
fn several_items_move_together() {
    let lim = Limits::default();
    let mut s = divider();
    let items =
        [Item::parse(&s, "R1").unwrap(), Item::parse(&s, "R2").unwrap(), Item::Wire(0), Item::Label(0)];
    let n = s.move_items(&items, [5.0, 10.0], &lim).unwrap();
    assert_eq!(n, 4);
    assert_eq!(s.symbols[s.symbol_index("R1").unwrap()].at, [105.0, 60.0]);
    assert_eq!(s.symbols[s.symbol_index("R2").unwrap()].at, [115.0, 60.0]);
    assert_eq!((s.wires[0].a, s.wires[0].b), ([105.0, 56.19], [115.0, 56.19]));
    assert_eq!(s.labels[0].at, [115.0, 63.81]);
    // The ground wires were not selected: they followed R1's pin 2 orthogonally.
    assert!(orthogonal(&s), "{:?}", s.wires);
    assert!(joined(&s, ("R1", "1"), ("R2", "1")));
    assert!(powered(&s, "R1", "2"));
    assert!(s.move_items(&[Item::Wire(99)], [1.0, 1.0], &lim).is_err());
    assert!(s.move_items(&items, [f64::NAN, 0.0], &lim).is_err());
    assert_eq!(Schematic::read(&s.write(), &lim).unwrap().tree, s.tree);
}
