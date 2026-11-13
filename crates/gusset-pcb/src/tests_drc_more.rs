//! ADR 0048: hole to hole, drill sizes, courtyard overlap and silkscreen over pads.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits};
use crate::copper::ItemRef;
use crate::drc::{self, Violation};
use crate::rules::Rules;

/// A 40 × 20 mm board with `body` (footprints, vias) and `setup` extras.
fn board(setup: &str, body: &str) -> Board {
    let src = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (36 "B.SilkS" user)
    (35 "F.CrtYd" user) (34 "B.CrtYd" user) (44 "Edge.Cuts" user))
  (setup {setup})
  (net 0 "")
  {body}
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
)"#
    );
    Board::read(&src, &Limits::default()).unwrap()
}

fn only(b: &Board, rules: &Rules, code: &str) -> Vec<Violation> {
    drc::check(b, rules, 1000).into_iter().filter(|v| v.code == code).collect()
}

fn via(x: f64, y: f64, size: f64, drill: f64) -> String {
    format!(r#"(via (at {x} {y}) (size {size}) (drill {drill}) (layers "F.Cu" "B.Cu") (net 0))"#)
}

fn thru(reference: &str, x: f64, y: f64, drill: &str, extra: &str) -> String {
    format!(
        r#"(footprint "T:{reference}" (layer "F.Cu") (at {x} {y})
    (property "Reference" "{reference}" (at 0 -3) (layer "F.SilkS"))
    (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill {drill}) (layers "*.Cu" "*.Mask") (net 0))
    {extra}
  )"#
    )
}

#[test]
fn hole_to_hole_checks_the_gap_between_hole_edges() {
    let rules = Rules::default();
    // Vias with 0.3 mm drills, 0.5 mm centre to centre: 0.2 mm between the edges.
    let b = board("", &format!("{} {}", via(10.0, 10.0, 0.6, 0.3), via(10.5, 10.0, 0.6, 0.3)));
    let v = only(&b, &rules, "hole_to_hole");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Via(0), ItemRef::Via(1)]);
    assert!((v[0].at[0] - 10.25).abs() < 1e-6 && v[0].message.contains("0.200 mm apart"), "{v:?}");
    // 0.6 mm apart centre to centre: 0.3 mm between edges, enough.
    let b = board("", &format!("{} {}", via(10.0, 10.0, 0.6, 0.3), via(10.6, 10.0, 0.6, 0.3)));
    assert!(only(&b, &rules, "hole_to_hole").is_empty());
    // A larger rule makes it a violation again; zero switches the rule off.
    let tight = Rules { hole_to_hole: 0.4, ..Rules::default() };
    assert_eq!(only(&b, &tight, "hole_to_hole").len(), 1);
    let off = Rules { hole_to_hole: 0.0, ..Rules::default() };
    assert!(
        only(&board("", &format!("{} {}", via(10.0, 10.0, 0.6, 0.3), via(10.5, 10.0, 0.6, 0.3))), &off, "hole_to_hole")
            .is_empty()
    );
}

#[test]
fn hole_to_hole_covers_pads_and_slots() {
    let rules = Rules::default();
    // Two 1 mm pad holes 1.1 mm apart (0.1 between edges), the second one an oval slot.
    let body = format!("{} {}", thru("J1", 10.0, 10.0, "1", ""), thru("J2", 11.1, 10.0, "oval 1 2", ""));
    let b = board("", &body);
    let v = only(&b, &rules, "hole_to_hole");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Pad(0), ItemRef::Pad(1)]);
    assert!(v[0].message.contains("J1.1") && v[0].message.contains("J2.1"), "{v:?}");
    // A via near the slot's far end: the slot is 2 mm long, so it ends at y 11 and the via, 0.35 mm off, is far enough; at 11.2 it is not.
    let body = format!("{} {}", thru("J2", 11.1, 10.0, "oval 1 2", ""), via(11.1, 11.5, 0.6, 0.3));
    let b = board("", &body);
    assert!(only(&b, &rules, "hole_to_hole").is_empty(), "{:?}", only(&b, &rules, "hole_to_hole"));
    let body = format!("{} {}", thru("J2", 11.1, 10.0, "oval 1 2", ""), via(11.1, 11.2, 0.6, 0.3));
    let b = board("", &body);
    let v = only(&b, &rules, "hole_to_hole");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Pad(0), ItemRef::Via(0)]);
}

#[test]
fn drill_sizes_are_checked_for_pads_and_vias() {
    let rules = Rules::default();
    // Pad holes: 0.25 mm is under the 0.3 mm minimum, a 0.8 × 0.25 slot too (narrow side).
    let body = format!(
        "{} {} {} {}",
        thru("J1", 5.0, 10.0, "0.25", ""),
        thru("J2", 15.0, 10.0, "0.3", ""),
        thru("J3", 25.0, 10.0, "oval 0.8 0.25", ""),
        thru("J4", 35.0, 10.0, "oval 0.8 0.4", "")
    );
    let v = only(&board("", &body), &rules, "drill_size");
    let items: Vec<ItemRef> = v.iter().flat_map(|x| x.items.clone()).collect();
    assert_eq!(items, [ItemRef::Pad(0), ItemRef::Pad(2)], "{v:?}");
    assert!(v[0].message.contains("0.250 mm") && v[0].message.contains("0.300 mm"), "{v:?}");
    // Vias: a 0.15 mm drill is under the default 0.2 mm; 0.2 passes.
    let body = format!("{} {}", via(5.0, 5.0, 0.6, 0.15), via(10.0, 5.0, 0.6, 0.2));
    let v = only(&board("", &body), &rules, "drill_size");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Via(0)]);
}

#[test]
fn via_minimums_come_from_the_board_setup() {
    let body = format!("{} {}", via(5.0, 5.0, 0.5, 0.25), via(10.0, 5.0, 0.8, 0.4));
    // Without the setup keys: no diameter limit, drills over 0.2 pass.
    let b = board("", &body);
    let rules = Rules::for_board(&b);
    assert!(only(&b, &rules, "drill_size").is_empty() && only(&b, &rules, "via_diameter").is_empty());
    // KiCad 5 style minimums.
    let b = board("(via_min_size 0.6) (via_min_drill 0.3)", &body);
    let rules = Rules::for_board(&b);
    assert_eq!((rules.min_via_diameter, rules.min_via_drill), (0.6, 0.3));
    let d = only(&b, &rules, "drill_size");
    let s = only(&b, &rules, "via_diameter");
    assert_eq!((d.len(), s.len()), (1, 1), "{d:?} {s:?}");
    assert_eq!((d[0].items.clone(), s[0].items.clone()), (vec![ItemRef::Via(0)], vec![ItemRef::Via(0)]));
}
