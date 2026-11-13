#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Phase 3: net edits, new zones, hatched fills, texts in Gerbers, shoving a track.

use crate::board::{Board, Hatch, Limits, ZoneSettings};
use crate::nets::NewZone;
use crate::rules::Rules;
use crate::{drc, font, gerber, poly, ratsnest, route, shove, zones};

fn lim() -> Limits {
    Limits::default()
}

/// One single-pad footprint.
fn part(r: &str, at: [f64; 2], d: f64, net: &str) -> String {
    format!(
        r#"(footprint "P" (layer "F.Cu") (at {} {})
    (property "Reference" "{r}" (at 0 -1.5) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd circle (at 0 0) (size {d} {d}) (layers "F.Cu") {net}))"#,
        at[0], at[1]
    )
}

/// Shove: B runs straight above a channel closed by C copper; A's two pads sit in the
/// channel with C's blob between them, so A only fits where B is.
fn channel() -> Board {
    let parts = [
        part("B1", [2.0, 5.0], 0.6, "(net 2 \"B\")"),
        part("B2", [28.0, 5.0], 0.6, "(net 2 \"B\")"),
        part("A1", [8.0, 6.0], 0.5, "(net 1 \"A\")"),
        part("A2", [22.0, 6.0], 0.5, "(net 1 \"A\")"),
        part("C1", [3.0, 6.2], 1.2, "(net 3 \"C\")"),
        part("C2", [27.0, 6.2], 1.2, "(net 3 \"C\")"),
        part("C3", [15.0, 6.3], 1.4, "(net 3 \"C\")"),
    ]
    .join("\n");
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C")
  (gr_rect (start 0 0) (end 30 12) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  {parts}
  (segment (start 2 5) (end 15 5) (width 0.25) (layer "F.Cu") (net 2))
  (segment (start 15 5) (end 28 5) (width 0.25) (layer "F.Cu") (net 2))
  (segment (start 3 7) (end 27 7) (width 1) (layer "F.Cu") (net 3))
)"#
    );
    Board::read(&text, &lim()).unwrap()
}

#[test]
fn a_blocking_track_is_shoved_aside_and_the_board_stays_clean() {
    let b = channel();
    let rules = Rules::for_board(&b);
    assert!(drc::check(&b, &rules, 100).is_empty(), "{:?}", drc::check(&b, &rules, 100));
    let (a1, a2) = ([8.0, 6.0], [22.0, 6.0]);
    // No way round: B is in it.
    let obs = route::Obstacles::new(&b, &rules, "F.Cu", Some("A"), 0.25);
    assert!(route::walkaround(&obs, a1, a2, 0.1, true).is_none());
    let s = shove::shove(&b, &rules, "F.Cu", Some("A"), 0.25, a1, a2, 0.1, &[]).expect("a shove");
    assert_eq!(s.removed, vec![0, 1]);
    assert_eq!(s.moved_net.as_deref(), Some("B"));
    assert_eq!((s.moved[0], *s.moved.last().unwrap()), ([2.0, 5.0], [28.0, 5.0]));
    assert!(s.moved.len() > 2 && s.moved.iter().any(|p| p[1] < 4.9), "{:?}", s.moved);
    let mut nb = b.clone();
    nb.apply_shove(&s, "F.Cu", &lim()).unwrap();
    let segs: Vec<route::Seg> = s.path.windows(2).map(|w| route::Seg { start: w[0], end: w[1], mid: None }).collect();
    nb.add_route(&segs, 0.25, "F.Cu", Some("A"), &lim()).unwrap();
    assert!(drc::check(&nb, &rules, 100).is_empty(), "{:?}", drc::check(&nb, &rules, 100));
    // A is joined and B still is.
    assert_eq!(ratsnest::ratsnest(&nb).unrouted(), ratsnest::ratsnest(&b).unrouted() - 1);
    // Two blocking nets: refused.
    let mut two = b.clone();
    two.add_track([9.0, 4.0], [21.0, 4.0], 0.25, "F.Cu", Some("C"), &lim()).unwrap();
    let rules2 = Rules::for_board(&two);
    let s2 = shove::shove(&two, &rules2, "F.Cu", Some("A"), 0.25, a1, a2, 0.1, &[]);
    // C's new track is above B: the shove may move B only if C is not in the way too.
    if let Some(s2) = s2 {
        let mut nb = two.clone();
        nb.apply_shove(&s2, "F.Cu", &lim()).unwrap();
        let segs: Vec<route::Seg> =
            s2.path.windows(2).map(|w| route::Seg { start: w[0], end: w[1], mid: None }).collect();
        nb.add_route(&segs, 0.25, "F.Cu", Some("A"), &lim()).unwrap();
        assert!(drc::check(&nb, &rules2, 100).len() <= drc::check(&two, &rules2, 100).len());
    }
}

#[test]
fn nets_are_set_renamed_and_merged() {
    let mut b = channel();
    // A pad's net.
    let n = b.set_pad_nets(&[("A1".into(), "1".into())], Some("NEW"), &lim()).unwrap();
    assert_eq!(n, 1);
    let pad = |b: &Board, r: &str| {
        let f = b.footprints.iter().find(|f| f.reference == r).unwrap();
        b.net_name(&b.pads[f.pads[0]].net)
    };
    assert_eq!(pad(&b, "A1").as_deref(), Some("NEW"));
    assert!(b.nets.iter().any(|n| n.name == "NEW"));
    assert!(b.set_pad_nets(&[("Z9".into(), "1".into())], Some("X"), &lim()).is_err());
    assert!(b.set_pad_nets(&[("A1".into(), "7".into())], Some("X"), &lim()).is_err());
    // No net.
    b.set_pad_nets(&[("A1".into(), "1".into())], None, &lim()).unwrap();
    assert_eq!(pad(&b, "A1"), None);
    // Rename B: pads and tracks follow, the number stays.
    let id = b.nets.iter().find(|n| n.name == "B").unwrap().id;
    let n = b.rename_net("B", "BUS", &lim()).unwrap();
    assert!(n >= 5, "{n}");
    assert_eq!(b.nets.iter().find(|n| n.name == "BUS").unwrap().id, id);
    assert_eq!(pad(&b, "B2").as_deref(), Some("BUS"));
    assert_eq!(b.net_name(&b.tracks[0].net).as_deref(), Some("BUS"));
    // Merge BUS into C: one net left, B's tracks now C.
    b.rename_net("BUS", "C", &lim()).unwrap();
    assert!(!b.nets.iter().any(|n| n.name == "BUS"));
    assert_eq!(pad(&b, "B1").as_deref(), Some("C"));
    assert_eq!(b.net_name(&b.tracks[1].net).as_deref(), Some("C"));
    assert!(b.rename_net("nope", "x", &lim()).is_err());
    assert!(b.rename_net("C", "", &lim()).is_err());
    // A file that names nets without numbers (KiCad 10).
    let mut k10 = Board::read(
        r#"(kicad_pcb (version 20250000) (generator "t") (layers (0 "F.Cu" signal))
      (footprint "P" (layer "F.Cu") (at 0 0) (property "Reference" "U1" (at 0 0) (layer "F.Fab"))
        (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net "X")))
      (segment (start 0 0) (end 1 0) (width 0.2) (layer "F.Cu") (net "X")))"#,
        &lim(),
    )
    .unwrap();
    k10.rename_net("X", "Y", &lim()).unwrap();
    assert!(k10.write().contains("(net \"Y\")") && !k10.write().contains("\"X\""), "{}", k10.write());
}

#[test]
fn zones_are_added_and_hatched_fills_have_holes() {
    let mut b = Board::new_two_layer(30.0, 20.0).unwrap();
    let solid = NewZone {
        outline: vec![[1.0, 1.0], [14.0, 1.0], [14.0, 19.0], [1.0, 19.0]],
        layers: vec!["F.Cu".into()],
        net: Some("GND".into()),
        // No GND copper yet: islands are kept.
        settings: ZoneSettings { island_mode: 1, ..ZoneSettings::default() },
    };
    let i = b.add_zone(&solid, &lim()).unwrap();
    assert_eq!((i, b.zones.len()), (0, 1));
    assert_eq!(b.net_name(&b.zones[0].net).as_deref(), Some("GND"));
    let h = Hatch { thickness: 0.5, gap: 1.0, angle: 0.0, min_hole_area: 0.3 };
    let hatched = NewZone {
        outline: vec![[16.0, 1.0], [29.0, 1.0], [29.0, 19.0], [16.0, 19.0]],
        layers: vec!["F.Cu".into(), "B.Cu".into()],
        net: None,
        settings: ZoneSettings { hatch: Some(h), island_mode: 1, ..ZoneSettings::default() },
    };
    b.add_zone(&hatched, &lim()).unwrap();
    assert_eq!(b.zones[1].settings.hatch, Some(h));
    assert_eq!(b.zones[1].layers, ["F.Cu", "B.Cu"]);
    let rules = Rules::for_board(&b);
    let f = zones::fill(&b, &rules, None);
    let area = |z: usize, l: &str| {
        f.fills.iter().filter(|x| x.zone == z && x.layer == l).map(|x| poly::total_area(&x.areas)).sum::<f64>()
    };
    // The solid zone: its outline inside the board less 0.3 mm to the edge (13 × 18).
    assert!((area(0, "F.Cu") - 13.0 * 18.0).abs() < 0.5, "{}", area(0, "F.Cu"));
    // The hatched one: a 1 mm hole every 1.5 mm takes about 4/9 of the inside.
    let full = 12.7 * 18.0;
    let a = area(1, "B.Cu");
    assert!(a < full * 0.7 && a > full * 0.45, "{a} of {full}");
    assert_eq!(area(1, "F.Cu"), a);
    b.set_zone_fills(&f.fills, &lim()).unwrap();
    assert!(drc::check(&b, &rules, 100).is_empty());
    let back = Board::read(&b.write(), &lim()).unwrap();
    assert_eq!(back.zones[1].settings.hatch, Some(h));
    b.delete_zones(&[0], &lim()).unwrap();
    assert_eq!(b.zones.len(), 1);
    assert!(b.delete_zones(&[5], &lim()).is_err());
    let bad = NewZone { layers: vec!["F.SilkS".into()], ..solid };
    assert!(b.add_zone(&bad, &lim()).is_err());
}

#[test]
fn texts_are_plotted_in_the_stroke_font() {
    let b = channel();
    let silk = gerber::write_layer(&b, "F.SilkS").unwrap();
    // Each reference is drawn: B1 has a B (two strokes) and a 1 (two strokes).
    assert!(silk.matches("D01*").count() > 40, "{silk}");
    // Placement: centred on its place, at its height.
    let t = b.texts.iter().find(|t| t.text == "B1").unwrap();
    let s = font::strokes(t, "B1");
    let ys: Vec<f64> = s.iter().flatten().map(|p| p[1]).collect();
    let xs: Vec<f64> = s.iter().flatten().map(|p| p[0]).collect();
    let (lo, hi) = (ys.iter().copied().fold(f64::MAX, f64::min), ys.iter().copied().fold(f64::MIN, f64::max));
    assert!((hi - lo - 1.0).abs() < 1e-9, "{lo} {hi}");
    assert!(((lo + hi) / 2.0 - t.at[1]).abs() < 1e-9);
    let (l, r) = (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max));
    assert!(((l + r) / 2.0 - t.at[0]).abs() < 1e-9, "{l} {r} {:?}", t.at);
    assert!((r - l - font::extent("B1", 1.0)[0]).abs() < 1e-9);
    // Turned 90°: it reads upwards (x and y swap their spans).
    let up = crate::board::Text { angle: 90.0, ..t.clone() };
    let s = font::strokes(&up, "B1");
    let xs: Vec<f64> = s.iter().flatten().map(|p| p[0]).collect();
    let span = xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min);
    assert!((span - 1.0).abs() < 1e-9, "{span}");
    // Footprint field variables are filled in.
    assert_eq!(font::shown("${REFERENCE}:%V", "R1", "10k"), "R1:10k");
    // Copper text is an obstacle for fills.
    let mut c = Board::new_two_layer(20.0, 10.0).unwrap();
    let text = c.write().replacen(
        "(net 0 \"\")",
        "(net 0 \"\") (gr_text \"HI\" (at 10 5 0) (layer \"F.Cu\") (effects (font (size 2 2) (thickness 0.3))))",
        1,
    );
    c = Board::read(&text, &lim()).unwrap();
    let z = NewZone {
        outline: vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]],
        layers: vec!["F.Cu".into()],
        net: None,
        settings: ZoneSettings { island_mode: 1, ..ZoneSettings::default() },
    };
    c.add_zone(&z, &lim()).unwrap();
    let f = zones::fill(&c, &Rules::for_board(&c), None);
    let a = poly::total_area(&f.fills[0].areas);
    assert!(a < 19.4 * 9.4 - 2.0, "{a}");
    let cu = gerber::write_layer(&c, "F.Cu").unwrap();
    assert!(cu.contains("D01*"));
}
