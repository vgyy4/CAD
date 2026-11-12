#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits, ZoneConnect};
use crate::rules::Rules;
use crate::{drc, poly, zones};
use std::f64::consts::PI;

/// A 20 × 10 mm board with one GND zone (1,1)–(19,9) on F.Cu; `items` are added inside.
fn board_with(zone_extra: &str, items: &str) -> Board {
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "GND")
  (net 2 "SIG")
  (gr_rect (start 0 0) (end 20 10) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  {items}
  (zone (net 1) (net_name "GND") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0.5)) (min_thickness 0.25)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5) {zone_extra})
    (polygon (pts (xy 1 1) (xy 19 1) (xy 19 9) (xy 1 9)))
  )
)"#
    );
    Board::read(&text, &Limits::default()).unwrap()
}

/// What rounding the corners of the minimum-width opening (radius h) takes from a convex
/// right-angled corner.
fn corner(h: f64) -> f64 {
    (1.0 - PI / 4.0) * h * h
}

fn filled_area(b: &Board) -> (f64, usize) {
    let f = zones::fill(b, &Rules::for_board(b), None);
    assert_eq!(f.fills.len(), 1);
    (zones::fill_area(&f.fills[0]), f.fills[0].areas.len())
}

#[test]
fn polygon_operations_and_triangles() {
    let sq = |x: f64, y: f64, s: f64| vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]];
    let u = poly::union(&[sq(0.0, 0.0, 2.0), sq(1.0, 1.0, 2.0)]);
    assert!((poly::total_area(&u) - 7.0).abs() < 1e-9);
    let d =
        poly::difference(&poly::union(&[sq(0.0, 0.0, 10.0)]), &poly::union(&[sq(2.0, 2.0, 2.0), sq(6.0, 6.0, 2.0)]));
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].len(), 3, "an outline and two holes");
    assert!((poly::total_area(&d) - 92.0).abs() < 1e-9);
    // Holes bridged in: one contour with the same area, triangles covering it exactly.
    let flat = poly::fracture(&d[0]);
    let tri = poly::triangulate(&flat, 1_000_000);
    let a: f64 = tri
        .iter()
        .map(|t| {
            let (p, q, r) = (flat[t[0]], flat[t[1]], flat[t[2]]);
            ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])).abs() / 2.0
        })
        .sum();
    assert!((a - 92.0).abs() < 1e-9, "{a}");
    assert!((poly::total_area(&poly::union(&[flat])) - 92.0).abs() < 1e-9);
    // Offsets: a 10 mm square grown by 1 has round corners.
    let g = poly::offset(&poly::union(&[sq(0.0, 0.0, 10.0)]), 1.0);
    assert!((poly::total_area(&g) - (100.0 + 40.0 + PI)).abs() < 0.03, "{}", poly::total_area(&g));
    // Hostile input gives nothing, not a panic.
    assert!(poly::triangulate(&[[f64::NAN, 0.0], [1.0, 0.0], [0.0, 1.0]], 10).is_empty());
    assert!(poly::union(&[vec![[0.0, 0.0], [1.0, 1.0]]]).is_empty());
}

fn tri_area(flat: &[[f64; 2]], tri: &[[usize; 3]]) -> f64 {
    tri.iter()
        .map(|t| {
            let (p, q, r) = (flat[t[0]], flat[t[1]], flat[t[2]]);
            ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])).abs() / 2.0
        })
        .sum()
}

#[test]
fn many_holes_triangulate_exactly() {
    // A plate with a grid of round holes, some on one row (bridges along one line).
    let mut holes = Vec::new();
    for i in 0..6 {
        for j in 0..5 {
            holes.push(poly::circle([1.5 + 1.6 * f64::from(i), 1.5 + 1.7 * f64::from(j)], 0.5));
        }
    }
    let plate = poly::union(&[vec![[0.0, 0.0], [11.0, 0.0], [11.0, 10.0], [0.0, 10.0]]]);
    let d = poly::difference(&plate, &poly::union(&holes));
    assert_eq!(d.len(), 1);
    let flat = poly::fracture(&d[0]);
    let tri = poly::triangulate(&flat, 1_000_000);
    let (want, got) = (poly::total_area(&d), tri_area(&flat, &tri));
    assert!((want - got).abs() < 1e-6, "{got} vs {want}");
}

#[test]
fn zone_settings_are_read() {
    let b = board_with("(island_removal_mode 1)", "");
    let s = b.zones[0].settings;
    assert_eq!(s.clearance, 0.5);
    assert_eq!(s.min_width, 0.25);
    assert_eq!(s.connect, ZoneConnect::Thermal);
    assert_eq!(s.island_mode, 1);
    assert_eq!((s.thermal_gap, s.thermal_width), (0.5, 0.5));
}

#[test]
fn empty_zone_fills_its_outline() {
    let b = board_with("(island_removal_mode 1)", "");
    let (a, n) = filled_area(&b);
    // 18 × 8 less the opening's rounded corners.
    let want = 144.0 - 4.0 * corner(0.125);
    assert_eq!(n, 1);
    assert!((a - want).abs() < 2e-3, "{a} vs {want}");
    // Without copper of its net, the island goes (KiCad's default).
    let b = board_with("", "");
    assert_eq!(filled_area(&b).1, 0);
}

#[test]
fn other_net_track_cuts_the_zone_with_clearance() {
    let b = board_with(
        "(island_removal_mode 1)",
        r#"(segment (start 0 5) (end 20 5) (width 0.25) (layer "F.Cu") (net 2))"#,
    );
    let (a, n) = filled_area(&b);
    // A strip of 0.25 + 2 × (0.5 clearance + 6 µm margin) across the 18 mm zone.
    let strip = 18.0 * (0.25 + 2.0 * 0.506);
    let want = 144.0 - strip - 8.0 * corner(0.125);
    assert_eq!(n, 2);
    assert!((a - want).abs() < 5e-3, "{a} vs {want}");
    // On B.Cu the track is no obstacle; the zone is on F.Cu only.
}

#[test]
fn thermal_relief_and_islands() {
    // A GND through-hole pad (2 mm round, 1 mm hole) in the zone; spokes at 45°.
    let pad = r#"(footprint "T:P" (layer "F.Cu") (at 5 5)
      (property "Reference" "J1" (at 0 0) (layer "F.SilkS"))
      (pad "1" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers "*.Cu") (net 1 "GND")))"#;
    let b = board_with("", pad);
    let (a, n) = filled_area(&b);
    assert_eq!(n, 1, "the pad joins the zone, so it stays");
    // The relief: a disc of radius 1 + 0.5 gap (+ 5 µm chord allowance) taken away and
    // two crossing 0.5 mm bars put back inside it.
    let r: f64 = 1.505;
    let w: f64 = 0.5;
    let strip = 2.0 * ((w / 2.0) * libm::sqrt(r * r - w * w / 4.0) + r * r * libm::asin(w / (2.0 * r)));
    let want = 144.0 - 4.0 * corner(0.125) - PI * r * r + (2.0 * strip - w * w);
    assert!((a - want).abs() < 0.06, "{a} vs {want}");
    // Solid connection: no relief at all.
    let solid = pad.replace("(net 1 \"GND\"))", "(net 1 \"GND\") (zone_connect 2))");
    let (a2, _) = filled_area(&board_with("", &solid));
    assert!((a2 - (144.0 - 4.0 * corner(0.125))).abs() < 2e-3, "{a2}");
    // The pad on another net: the clearance all round, the island kept only by mode 1.
    let other = pad.replace("(net 1 \"GND\")", "(net 2 \"SIG\")");
    assert_eq!(filled_area(&board_with("", &other)).1, 0);
    let (a3, _) = filled_area(&board_with("(island_removal_mode 1)", &other));
    let want3 = 144.0 - 4.0 * corner(0.125) - PI * 1.511 * 1.511;
    assert!((a3 - want3).abs() < 0.06, "{a3} vs {want3}");
}

#[test]
fn fills_are_written_back_and_pass_drc() {
    let items = r#"(segment (start 0 5) (end 20 5) (width 0.25) (layer "F.Cu") (net 2))
      (via (at 4 7) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1))
      (via (at 10 7) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 2))
      (footprint "T:P" (layer "F.Cu") (at 15 3)
        (property "Reference" "J1" (at 0 0) (layer "F.SilkS"))
        (pad "1" thru_hole rect (at 0 0) (size 1.5 1.5) (drill 0.8) (layers "*.Cu") (net 1 "GND")))"#;
    let mut b = board_with("", items);
    let rules = Rules::for_board(&b);
    let f = zones::fill(&b, &rules, None);
    let total: f64 = f.fills.iter().map(zones::fill_area).sum();
    let n = b.set_zone_fills(&f.fills, &Limits::default()).unwrap();
    assert_eq!(n, 2, "both halves hold GND copper (the via below, the pad above)");
    assert_eq!(b.zones[0].fills.len(), 2);
    // Read back: the same area (holes bridged into the outline).
    let back: f64 = b.zones[0].fills.iter().map(|(_, p)| poly::total_area(&poly::union(std::slice::from_ref(p)))).sum();
    assert!((back - total).abs() < 1e-3, "{back} vs {total}");
    assert!(b.write().contains("(fill yes"));
    // Filling again gives the same file.
    let again = zones::fill(&b, &rules, None);
    let mut b2 = b.clone();
    b2.set_zone_fills(&again.fills, &Limits::default()).unwrap();
    assert_eq!(b2.write(), b.write());
    // The zone keeps its clearance: no DRC findings about it.
    let v = drc::check(&b, &rules, 100);
    assert!(v.iter().all(|x| !x.message.contains("zone")), "{v:?}");
    // A fill drawn over the other net's track is caught.
    let bad = board_with(
        "",
        r#"(segment (start 0 5) (end 20 5) (width 0.25) (layer "F.Cu") (net 2))
           (via (at 4 3) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1))"#,
    );
    let mut bad = bad;
    let fill = zones::Fill {
        zone: 0,
        layer: "F.Cu".into(),
        areas: poly::union(&[vec![[1.0, 1.0], [19.0, 1.0], [19.0, 5.3], [1.0, 5.3]]]),
    };
    bad.set_zone_fills(&[fill], &Limits::default()).unwrap();
    let v = drc::check(&bad, &Rules::for_board(&bad), 100);
    assert!(v.iter().any(|x| x.code == "short" && x.message.contains("zone")), "{v:?}");
}

#[test]
fn priorities_and_keepouts() {
    // A higher-priority SIG zone over the left half: GND keeps clear of it.
    let items = r#"(via (at 15 5) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1))
      (via (at 3 5) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 2))
      (zone (net 2) (net_name "SIG") (layer "F.Cu") (priority 1) (connect_pads (clearance 0.5)) (min_thickness 0.25)
        (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
        (polygon (pts (xy 1 1) (xy 6 1) (xy 6 9) (xy 1 9))))
      (zone (net 0) (net_name "") (layer "F.Cu") (keepout (tracks allowed) (vias allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))
        (polygon (pts (xy 10 1) (xy 12 1) (xy 12 9) (xy 10 9))))"#;
    let b = board_with("(island_removal_mode 1)", items);
    let f = zones::fill(&b, &Rules::for_board(&b), None);
    let sig = f.fills.iter().find(|x| x.zone == 0).unwrap();
    let gnd = f.fills.iter().find(|x| x.zone == 2).unwrap();
    assert!(poly::total_area(&sig.areas) > 30.0);
    // GND: x from 6 + 0.506 (+ 5 µm for chords) to 19, less the keep-out strip 10..12
    // and the opening's rounded corners (four per piece).
    let want = (19.0 - 6.511 - 2.0) * 8.0 - 8.0 * corner(0.125);
    let a = poly::total_area(&gnd.areas);
    assert!((a - want).abs() < 0.01, "{a} vs {want}");
}
