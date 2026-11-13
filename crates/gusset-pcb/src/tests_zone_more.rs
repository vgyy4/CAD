#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! ADR 0053: zones over several outlines, starved thermal reliefs, filling on edit.

use crate::board::{Board, Limits};
use crate::rules::Rules;
use crate::zones;
use std::f64::consts::PI;

/// What rounding the corners of the minimum-width opening (radius h) takes from a convex
/// right-angled corner.
fn corner(h: f64) -> f64 {
    (1.0 - PI / 4.0) * h * h
}

/// One GND zone of two separate outlines (left 1..9 and right 11..19, y 1..9).
fn two_outline_board() -> (String, Board) {
    let text = r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "GND")
  (gr_rect (start 0 0) (end 20 10) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (zone (net 1) (net_name "GND") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0.5)) (min_thickness 0.25)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1))
    (polygon (pts (xy 1 1) (xy 9 1) (xy 9 9) (xy 1 9)))
    (polygon (pts (xy 11 1) (xy 19 1) (xy 19 9) (xy 11 9)))
  )
)
"#
    .to_string();
    // As the writer lays a file out, so an untouched board can be compared byte for byte.
    let text = Board::read(&text, &Limits::default()).unwrap().write();
    let b = Board::read(&text, &Limits::default()).unwrap();
    (text, b)
}

#[test]
fn zones_over_several_outlines() {
    let (text, mut b) = two_outline_board();
    assert_eq!(b.zones.len(), 1);
    assert_eq!(b.zones[0].more_outlines.len(), 1);
    assert_eq!(b.zones[0].outlines().count(), 2);
    // Untouched, the file comes back identical.
    assert_eq!(b.write(), text);
    // Both outlines fill: 2 x (8 x 8 less the opening's corners).
    let rules = Rules::for_board(&b);
    let f = zones::fill(&b, &rules, None);
    assert_eq!(f.fills.len(), 1);
    assert_eq!(f.fills[0].areas.len(), 2);
    let want = 2.0 * (64.0 - 4.0 * corner(0.125));
    assert!((zones::fill_area(&f.fills[0]) - want).abs() < 4e-3);
    // Written back as two polygons, read back the same; the outlines are kept.
    b.set_zone_fills(&f.fills, &Limits::default()).unwrap();
    assert_eq!(b.zones[0].fills.len(), 2);
    assert_eq!(b.zones[0].more_outlines, vec![vec![[11.0, 1.0], [19.0, 1.0], [19.0, 9.0], [11.0, 9.0]]]);
    // Overlapping outlines are one pour.
    let mut c = b.clone();
    let both = vec![
        vec![[1.0, 1.0], [12.0, 1.0], [12.0, 9.0], [1.0, 9.0]],
        vec![[8.0, 1.0], [19.0, 1.0], [19.0, 9.0], [8.0, 9.0]],
    ];
    c.set_zone_outlines(0, &both, &Limits::default()).unwrap();
    assert!(c.zones[0].fills.is_empty(), "the old fills no longer match");
    let f = zones::fill(&c, &rules, None);
    assert_eq!(f.fills[0].areas.len(), 1);
    assert!((zones::fill_area(&f.fills[0]) - (144.0 - 4.0 * corner(0.125))).abs() < 4e-3);
    // Bad outlines are refused.
    assert!(c.set_zone_outlines(0, &[vec![[0.0, 0.0], [1.0, 1.0]]], &Limits::default()).is_err());
    assert!(c.set_zone_outlines(3, &both, &Limits::default()).is_err());
    assert!(c.set_zone_outlines(0, &[], &Limits::default()).is_err());
}
