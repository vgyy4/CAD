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

/// A 20 x 10 board with a GND zone (1,1)-(19,9), a 2 mm round GND pad at (5,5) and `extra`.
fn thermal_board(zone_extra: &str, extra: &str) -> Board {
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "GND")
  (net 2 "SIG")
  (gr_rect (start 0 0) (end 20 10) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (footprint "T:P" (layer "F.Cu") (at 5 5)
    (property "Reference" "J1" (at 0 0) (layer "F.SilkS"))
    (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers "*.Cu") (net 1 "GND")))
  {extra}
  (zone (net 1) (net_name "GND") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0.5)) (min_thickness 0.25)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1) {zone_extra})
    (polygon (pts (xy 1 1) (xy 19 1) (xy 19 9) (xy 1 9)))
  )
)"#
    );
    Board::read(&text, &Limits::default()).unwrap()
}

fn starved(b: &Board, rules: &Rules) -> Vec<usize> {
    zones::fill(b, rules, None).starved.iter().map(|s| s.spokes).collect()
}

fn drc_starved(b: &Board, rules: &Rules) -> Vec<String> {
    crate::drc::check(b, rules, 100).into_iter().filter(|v| v.code == "starved_thermal").map(|v| v.message).collect()
}

/// The board with its zones filled, as the file would hold them.
fn filled(b: &Board, rules: &Rules) -> Board {
    let mut b = b.clone();
    let f = zones::fill(&b, rules, None);
    b.set_zone_fills(&f.fills, &Limits::default()).unwrap();
    b
}

#[test]
fn starved_thermal_reliefs_are_reported() {
    let top = r#"(segment (start 2 3.7) (end 8 3.7) (width 0.25) (layer "F.Cu") (net 2))"#;
    let bottom = r#"(segment (start 2 6.3) (end 8 6.3) (width 0.25) (layer "F.Cu") (net 2))"#;
    let rules = Rules::for_board(&thermal_board("", ""));
    assert_eq!(rules.min_spokes, 2);
    // All four spokes: nothing to report, in the fill or in the check of the stored file.
    let b = thermal_board("", "");
    assert!(starved(&b, &rules).is_empty());
    assert!(drc_starved(&filled(&b, &rules), &rules).is_empty());
    // A track of another net cuts the two upper spokes: two are left, enough for 2 ...
    let b = thermal_board("", top);
    assert!(starved(&b, &rules).is_empty());
    // ... but not for a rule of 3, and the stored fill says the same.
    let strict = Rules { min_spokes: 3, ..rules.clone() };
    assert_eq!(starved(&b, &strict), vec![2]);
    let found = drc_starved(&filled(&b, &rules), &strict);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("J1.1") && found[0].contains("2 thermal spokes"), "{}", found[0]);
    // Both cut: no spoke at all.
    let b = thermal_board("", &format!("{top} {bottom}"));
    assert_eq!(starved(&b, &rules), vec![0]);
    let v: Vec<_> = crate::drc::check(&filled(&b, &rules), &rules, 100)
        .into_iter()
        .filter(|v| v.code == "starved_thermal")
        .collect();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].at, [5.0, 5.0]);
    assert_eq!(v[0].layer.as_deref(), Some("F.Cu"));
    assert_eq!(v[0].items.len(), 2);
    // Solid joins are never starved.
    let solid = b.write().replace("(net 1 \"GND\")", "(net 1 \"GND\") (zone_connect 2)");
    let solid = Board::read(&solid, &Limits::default()).unwrap();
    assert!(starved(&solid, &rules).is_empty());
}

#[test]
fn spoke_width_and_angle_follow_the_zone() {
    let rules = Rules::for_board(&thermal_board("", ""));
    // Spokes narrower than the minimum width do not survive: the pad is starved.
    let thin = thermal_board("", "").write().replace("(thermal_bridge_width 0.5)", "(thermal_bridge_width 0.2)");
    let thin = Board::read(&thin, &Limits::default()).unwrap();
    assert_eq!(starved(&thin, &rules), vec![0]);
    assert_eq!(drc_starved(&filled(&thin, &rules), &rules).len(), 1);
    // Spokes along the axes instead of 45 degrees: four again.
    let axes = thermal_board("(thermal_bridge_angle 0)", "");
    assert_eq!(axes.zones[0].settings.thermal_angle, Some(0.0));
    assert!(starved(&axes, &rules).is_empty());
    // A track along y = 5 cuts both horizontal spokes of the axis layout.
    let cut = r#"(segment (start 2 5) (end 3.2 5) (width 0.25) (layer "F.Cu") (net 2))
      (segment (start 6.8 5) (end 8 5) (width 0.25) (layer "F.Cu") (net 2))"#;
    let axes_cut = thermal_board("(thermal_bridge_angle 0)", cut);
    assert!(starved(&axes_cut, &rules).is_empty(), "two vertical spokes are left");
    assert_eq!(starved(&axes_cut, &Rules { min_spokes: 3, ..rules }), vec![2]);
}
