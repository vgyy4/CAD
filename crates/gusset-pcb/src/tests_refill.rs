#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! ADR 0053: filling on edit.

use crate::board::{Board, Limits};
use crate::rules::Rules;
use crate::zones;

fn zone_area(b: &Board, zone: usize) -> f64 {
    b.zones[zone].fills.iter().map(|(_, p)| crate::poly::total_area(&crate::poly::union(std::slice::from_ref(p)))).sum()
}

#[test]
fn edits_refill_only_the_zones_they_touch() {
    use crate::refill;
    // Two separate zones: left and right halves of the board.
    let zone = |x0: f64, x1: f64| {
        format!(
            r#"(zone (net 1) (net_name "GND") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0.5)) (min_thickness 0.25)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1))
    (polygon (pts (xy {x0} 1) (xy {x1} 1) (xy {x1} 9) (xy {x0} 9))))"#
        )
    };
    let split = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "GND")
  (net 2 "SIG")
  (gr_rect (start 0 0) (end 20 10) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  {}
  {}
)"#,
        zone(1.0, 9.0),
        zone(11.0, 19.0)
    );
    let limits = Limits::default();
    let mut old = Board::read(&split, &limits).unwrap();
    assert_eq!(old.zones.len(), 2, "{split}");
    let rules = Rules::for_board(&old);
    let all = zones::fill(&old, &rules, None);
    old.set_zone_fills(&all.fills, &limits).unwrap();
    let (left0, right0) = (zone_area(&old, 0), zone_area(&old, 1));
    // Nothing changed: nothing to pour.
    assert!(refill::affected_zones(&old, &old, &rules).is_empty());
    // A SIG track through the left zone only (this needs the net).
    let mut new = old.clone();
    new.add_track([0.5, 5.0], [8.0, 5.0], 0.25, "F.Cu", Some("SIG"), &limits).unwrap();
    assert_eq!(refill::affected_zones(&old, &new, &rules), vec![0]);
    let (poured, _) = refill::refill_after_edit(&old, &mut new, &rules, &limits).unwrap();
    assert_eq!(poured, vec![0]);
    assert!(zone_area(&new, 0) < left0 - 8.0 * 0.9, "{} vs {left0}", zone_area(&new, 0));
    assert_eq!(zone_area(&new, 1), right0, "the right zone is as it was");
    // The same edit on the other layer touches no zone.
    let mut back = old.clone();
    back.add_track([0.5, 5.0], [8.0, 5.0], 0.25, "B.Cu", Some("SIG"), &limits).unwrap();
    assert!(refill::affected_zones(&old, &back, &rules).is_empty());
    // Deleting the track pours the zone whole again.
    let mut undone = new.clone();
    undone.delete_items(&[0], &[], &limits).unwrap();
    let (poured, _) = refill::refill_after_edit(&new, &mut undone, &rules, &limits).unwrap();
    assert_eq!(poured, vec![0]);
    assert!((zone_area(&undone, 0) - left0).abs() < 1e-3);
    // A zone edit refills the zones next to it as well as itself.
    let mut moved = old.clone();
    moved.set_zone_outlines(0, &[vec![[1.0, 1.0], [10.0, 1.0], [10.0, 9.0], [1.0, 9.0]]], &limits).unwrap();
    assert_eq!(refill::affected_zones(&old, &moved, &rules), vec![0, 1]);
}
