//! Phase 1b: edits, ratsnest, design rule check, placement and BOM files.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits};
use crate::copper::ItemRef;
use crate::rules::{NetClass, Rules, glob};
use crate::{drc, fab, ratsnest};

/// Two 2-pin connectors 20 mm apart on a 40 × 20 mm board; nets A (pins 1) and B (pins 2),
/// nothing routed.
const PAIR: &str = r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (36 "B.SilkS" user) (44 "Edge.Cuts" user))
  (setup (aux_axis_origin 0 20))
  (net 0 "")
  (net 1 "A")
  (net 2 "B")
  (footprint "Conn:Pins_1x02" (layer "F.Cu") (at 10 10)
    (property "Reference" "J1" (at 0 -2) (layer "F.SilkS"))
    (property "Value" "Conn" (at 0 4) (layer "F.SilkS"))
    (attr through_hole)
    (fp_line (start -1 -1) (end 1 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
    (pad "1" thru_hole circle (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (net 1 "A"))
    (pad "2" thru_hole circle (at 0 2.54) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (net 2 "B"))
  )
  (footprint "Conn:Pins_1x02" (layer "F.Cu") (at 30 10)
    (property "Reference" "J2" (at 0 -2) (layer "F.SilkS"))
    (property "Value" "Conn" (at 0 4) (layer "F.SilkS"))
    (attr through_hole)
    (pad "1" thru_hole circle (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (net 1 "A"))
    (pad "2" thru_hole circle (at 0 2.54) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask") (net 2 "B"))
  )
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
)
"#;

fn pair() -> Board {
    Board::read(PAIR, &Limits::default()).unwrap()
}

/// The file's text on one line, for `contains` checks.
fn flat(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").replace(" )", ")")
}

fn l() -> Limits {
    Limits::default()
}

#[test]
fn a_new_board_has_an_outline_and_two_copper_layers() {
    let b = Board::new_two_layer(50.0, 30.0).unwrap();
    assert_eq!(b.copper_layers(), ["F.Cu", "B.Cu"]);
    assert!((b.thickness - 1.6).abs() < 1e-9);
    let (loops, gaps) = crate::geom::outline_loops(&b, 1e-3);
    assert_eq!((loops.len(), gaps.len()), (1, 0));
    assert!((crate::geom::area(&loops[0].polygon(0.01)).abs() - 1500.0).abs() < 1e-6);
    let again = Board::read(&b.write(), &l()).unwrap();
    assert_eq!(again.tree, b.tree);
    assert!(Board::new_two_layer(0.0, 10.0).is_err() && Board::new_two_layer(f64::NAN, 1.0).is_err());
}

#[test]
fn tracks_and_vias_are_added_and_deleted_losslessly() {
    let mut b = pair();
    let t = b.add_track([10.0, 10.0], [30.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap();
    assert_eq!(b.tracks[t].width, 0.25);
    assert_eq!(b.net_name(&b.tracks[t].net).as_deref(), Some("A"));
    let v = b.add_via([20.0, 15.0], 0.6, 0.3, Some("B"), &l()).unwrap();
    assert_eq!(b.vias[v].layers, ["F.Cu".to_string(), "B.Cu".to_string()]);
    let text = flat(&b.write());
    assert!(text.contains("(segment (start 10 10) (end 30 10) (width 0.25) (layer \"F.Cu\") (net 1) (uuid"), "{text}");
    // Deleting what was added gives the file back as it was.
    b.delete_items(&[t], &[v], &l()).unwrap();
    assert_eq!(b.tree, pair().tree);
    // Bad edits change nothing.
    assert!(b.add_track([0.0, 0.0], [1.0, 0.0], 0.25, "F.SilkS", None, &l()).is_err());
    assert!(b.add_track([0.0, 0.0], [0.0, 0.0], 0.25, "F.Cu", None, &l()).is_err());
    assert!(b.add_via([0.0, 0.0], 0.3, 0.3, None, &l()).is_err());
    assert!(b.delete_items(&[3], &[], &l()).is_err());
    assert_eq!(b.tree, pair().tree);
}

#[test]
fn a_new_net_is_declared() {
    let mut b = pair();
    b.add_track([1.0, 1.0], [2.0, 1.0], 0.2, "B.Cu", Some("NEW"), &l()).unwrap();
    assert!(b.nets.iter().any(|n| n.id == 3 && n.name == "NEW"));
    assert!(flat(&b.write()).contains("(net 3 \"NEW\")"));
    assert_eq!(b.net_name(&b.tracks[0].net).as_deref(), Some("NEW"));
}

#[test]
fn footprints_move_turn_and_flip() {
    let mut b = pair();
    let j2 = b.footprint_index("J2").unwrap();
    assert_eq!(b.footprint_index("#0"), Some(0));
    assert_eq!(b.footprint_index("X9"), None);
    b.move_footprint(j2, [25.0, 5.0], 90.0, &l()).unwrap();
    // Pad 2 at local (0, 2.54) turned 90° counter-clockwise on screen: (27.54, 5).
    let p = b.pads[b.footprints[j2].pads[1]].at;
    assert!((p[0] - 27.54).abs() < 1e-9 && (p[1] - 5.0).abs() < 1e-9, "{p:?}");
    // Flip: to the bottom, mirrored about its horizontal axis.
    b.flip_footprint(0, &l()).unwrap();
    let j1 = &b.footprints[0];
    assert!(j1.bottom);
    let p2 = b.pads[j1.pads[1]].at;
    assert!((p2[0] - 10.0).abs() < 1e-9 && (p2[1] - 7.46).abs() < 1e-9, "{p2:?}");
    assert!(b.graphics.iter().any(|g| g.footprint == Some(0) && g.layer == "B.SilkS"));
    // Flipping back restores the footprint.
    b.flip_footprint(0, &l()).unwrap();
    let n = b.footprints[0].node;
    assert_eq!(b.tree.items()[n], pair().tree.items()[n]);
}

#[test]
fn the_outline_is_replaced() {
    let mut b = pair();
    b.set_outline(&[[0.0, 0.0], [50.0, 0.0], [50.0, 25.0], [0.0, 25.0]], &l()).unwrap();
    assert!(flat(&b.write()).contains("(gr_rect (start 0 0) (end 50 25)"));
    let tri = [[0.0, 0.0], [50.0, 0.0], [25.0, 30.0]];
    b.set_outline(&tri, &l()).unwrap();
    let (loops, _) = crate::geom::outline_loops(&b, 1e-3);
    assert_eq!(loops.len(), 1);
    assert!((crate::geom::area(&loops[0].polygon(0.01)).abs() - 750.0).abs() < 1e-6);
    assert!(b.set_outline(&[[0.0, 0.0], [1.0, 1.0]], &l()).is_err());
    assert!(b.set_outline(&[[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]], &l()).is_err());
}

#[test]
fn ratsnest_counts_unrouted_connections() {
    let mut b = pair();
    let r = ratsnest::ratsnest(&b);
    assert_eq!((r.unrouted(), r.nets, r.complete_nets), (2, 2, 0));
    let a = r.lines.iter().find(|x| x.net == "A").unwrap();
    let ends = [a.a, a.b];
    assert!(ends.contains(&[10.0, 10.0]) && ends.contains(&[30.0, 10.0]), "{a:?}");
    // Routing net A on the front leaves B.
    b.add_track([10.0, 10.0], [30.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap();
    assert_eq!(ratsnest::ratsnest(&b).unrouted(), 1);
    // B in two pieces through a via: front, then back.
    b.add_track([10.0, 12.54], [20.0, 16.0], 0.25, "F.Cu", Some("B"), &l()).unwrap();
    assert_eq!(ratsnest::ratsnest(&b).unrouted(), 1);
    b.add_via([20.0, 16.0], 0.6, 0.3, Some("B"), &l()).unwrap();
    b.add_track([20.0, 16.0], [30.0, 12.54], 0.25, "B.Cu", Some("B"), &l()).unwrap();
    let r = ratsnest::ratsnest(&b);
    assert_eq!((r.unrouted(), r.complete_nets), (0, 2));
}

#[test]
fn drc_finds_clearance_width_ring_and_edge_problems() {
    let mut b = pair();
    let rules = Rules::default();
    b.add_track([10.0, 10.0], [30.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap();
    assert!(drc::check(&b, &rules, 100).is_empty(), "{:?}", drc::check(&b, &rules, 100));
    // B 0.4 mm beside A: 0.15 mm gap.
    b.add_track([15.0, 10.4], [25.0, 10.4], 0.25, "F.Cu", Some("B"), &l()).unwrap();
    let v = drc::check(&b, &rules, 100);
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].code, "clearance");
    assert_eq!(v[0].layer.as_deref(), Some("F.Cu"));
    assert!((v[0].at[1] - 10.2).abs() < 1e-6 && v[0].message.contains("0.150 mm apart"), "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Track(0), ItemRef::Track(1)]);
    // On the back there is room.
    b.delete_items(&[1], &[], &l()).unwrap();
    b.add_track([15.0, 10.4], [25.0, 10.4], 0.25, "B.Cu", Some("B"), &l()).unwrap();
    assert!(drc::check(&b, &rules, 100).is_empty());
    // A crossing is a short; a thin track, a small ring and a via at the edge.
    b.add_track([20.0, 12.0], [20.0, 8.0], 0.2, "F.Cu", Some("B"), &l()).unwrap();
    b.add_track([5.0, 18.0], [8.0, 18.0], 0.1, "B.Cu", None, &l()).unwrap();
    b.add_via([35.0, 5.0], 0.5, 0.4, Some("A"), &l()).unwrap();
    b.add_via([0.3, 5.0], 0.4, 0.2, Some("A"), &l()).unwrap();
    let codes: Vec<&str> = drc::check(&b, &rules, 100).iter().map(|v| v.code).collect();
    for c in ["short", "track_width", "annular_ring", "hole_to_edge", "copper_to_edge"] {
        assert!(codes.contains(&c), "{c} missing in {codes:?}");
    }
    assert_eq!(drc::check(&b, &rules, 2).len(), 2);
}

#[test]
fn net_classes_set_clearance_and_widths() {
    let mut b = pair();
    b.add_track([10.0, 10.0], [30.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap();
    b.add_track([15.0, 10.6], [25.0, 10.6], 0.25, "F.Cu", Some("B"), &l()).unwrap();
    let mut rules = Rules::default();
    assert!(drc::check(&b, &rules, 10).is_empty());
    rules.classes.push(NetClass {
        name: "Power".into(),
        clearance: Some(0.5),
        track_width: Some(0.8),
        via_diameter: Some(1.0),
        via_drill: Some(0.5),
        nets: vec!["A".into()],
    });
    assert_eq!(rules.track_width_of(Some("A")), 0.8);
    assert_eq!(rules.track_width_of(Some("B")), 0.25);
    assert_eq!(rules.via_of(Some("A")), (1.0, 0.5));
    assert_eq!(drc::check(&b, &rules, 10).len(), 1);
    rules.classes[0].nets.clear();
    rules.patterns.push(("V*".into(), "Power".into()));
    assert_eq!(rules.clearance_of(Some("VCC")), 0.5);
    assert_eq!(rules.clearance_of(Some("GND")), 0.2);
    assert!(glob("*", "") && glob("a?c*", "abcde") && !glob("a?c", "abcd") && glob("*B*", "xxBy"));
    // KiCad 5 classes in the board.
    let k5 = PAIR.replace(
        "(net 0 \"\")",
        "(net 0 \"\") (net_class Default \"\" (clearance 0.3) (trace_width 0.4) (via_dia 0.9) (via_drill 0.45) (add_net \"A\") (add_net \"B\"))",
    );
    let r = Rules::for_board(&Board::read(&k5, &l()).unwrap());
    assert_eq!((r.clearance, r.track_width, r.via_diameter, r.via_drill), (0.3, 0.4, 0.9, 0.45));
}

#[test]
fn placement_and_bom_files() {
    let b = Board::read(crate::tests::BOARD, &l()).unwrap();
    let (pos, n) = fab::positions(&b);
    assert_eq!(n, 2, "{pos}");
    let lines: Vec<&str> = pos.lines().collect();
    assert_eq!(lines[0], "Ref,Val,Package,PosX,PosY,Rot,Side");
    assert_eq!(lines[1], "J1,\"\",Header_1x02,120.0000,-120.0000,0.0000,top");
    assert_eq!(lines[2], "R1,10k,R_0603_1608Metric,110.0000,-110.0000,90.0000,top");
    let mut p = pair();
    p.flip_footprint(1, &l()).unwrap();
    let (pos, _) = fab::positions(&p);
    // From the auxiliary origin (0, 20), Y up.
    assert!(pos.contains("J1,Conn,Pins_1x02,10.0000,10.0000,0.0000,top"), "{pos}");
    assert!(pos.contains("J2,Conn,Pins_1x02,30.0000,10.0000,0.0000,bottom"), "{pos}");
    let (bom, groups) = fab::bom(&p);
    assert_eq!(groups, 1);
    assert_eq!(bom, "Qty,Value,Footprint,References,DNP\n2,Conn,Conn:Pins_1x02,\"J1 J2\",\n");
    assert_eq!(fab::csv_field("=cmd()"), "'=cmd()");
    assert_eq!(fab::csv_field("a\"b"), "\"a\"\"b\"");
    assert_eq!(fab::natural_cmp("R2", "R10"), std::cmp::Ordering::Less);
}

#[test]
fn zone_fills_connect_the_pads_they_touch() {
    // A net A fill on the back that overlaps both pin 1 pads but not their centres (as
    // with thermal reliefs).
    let zone = r#"(zone (net 1) (net_name "A") (layer "B.Cu")
    (polygon (pts (xy 10.5 9) (xy 29.5 9) (xy 29.5 11) (xy 10.5 11)))
    (filled_polygon (layer "B.Cu") (pts (xy 10.5 9) (xy 29.5 9) (xy 29.5 11) (xy 10.5 11))))
  (gr_rect"#;
    let b = Board::read(&PAIR.replacen("(gr_rect", zone, 1), &l()).unwrap();
    let r = ratsnest::ratsnest(&b);
    assert_eq!(r.unrouted(), 1);
    assert_eq!(r.lines[0].net, "B");
}

#[test]
fn tracks_and_vias_move_and_attached_ends_follow() {
    let mut b = pair();
    b.add_track([10.0, 10.0], [20.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap();
    b.add_track([20.0, 10.0], [30.0, 10.0], 0.25, "B.Cu", Some("A"), &l()).unwrap();
    b.add_track([5.0, 5.0], [6.0, 5.0], 0.25, "F.Cu", None, &l()).unwrap();
    b.add_via([20.0, 10.0], 0.8, 0.4, Some("A"), &l()).unwrap();
    // A via drags the ends on it (both layers), not the far ends or other tracks.
    let (t, c) = b.move_items(&[], &[0], [0.0, 3.0], true, &l()).unwrap();
    assert_eq!((t, c), (0, 2));
    assert_eq!(b.vias[0].at, [20.0, 13.0]);
    assert_eq!((b.tracks[0].start, b.tracks[0].end), ([10.0, 10.0], [20.0, 13.0]));
    assert_eq!((b.tracks[1].start, b.tracks[1].end), ([20.0, 13.0], [30.0, 10.0]));
    assert_eq!(b.tracks[2].start, [5.0, 5.0]);
    // Without `attached` the ends stay.
    b.move_items(&[], &[0], [1.0, 0.0], false, &l()).unwrap();
    assert_eq!(b.tracks[0].end, [20.0, 13.0]);
    // A track moves whole; the file reads back the same.
    let (t, c) = b.move_items(&[2, 2], &[], [-1.0, 2.0], true, &l()).unwrap();
    assert_eq!((t, c), (1, 0));
    assert_eq!((b.tracks[2].start, b.tracks[2].end), ([4.0, 7.0], [5.0, 7.0]));
    let again = Board::read(&b.write(), &l()).unwrap();
    assert_eq!(again.tracks, b.tracks);
    assert_eq!(again.vias, b.vias);
}

#[test]
fn moving_items_refuses_bad_input_and_leaves_the_board() {
    let mut b = pair();
    b.add_track([10.0, 10.0], [20.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap();
    let before = b.write();
    assert!(b.move_items(&[], &[], [1.0, 0.0], true, &l()).is_err());
    assert!(b.move_items(&[5], &[], [1.0, 0.0], true, &l()).is_err());
    assert!(b.move_items(&[], &[0], [1.0, 0.0], true, &l()).is_err());
    assert!(b.move_items(&[0], &[], [f64::NAN, 0.0], true, &l()).is_err());
    assert!(b.move_items(&[0], &[], [1e9, 0.0], true, &l()).is_err());
    assert_eq!(b.write(), before);
}

#[test]
fn moving_a_track_stretches_its_neighbours() {
    let mut b = pair();
    b.add_track([10.0, 10.0], [20.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap(); // 0: moved
    b.add_track([10.0, 10.0], [10.0, 5.0], 0.25, "F.Cu", Some("A"), &l()).unwrap(); // 1: follows at start
    b.add_track([20.0, 10.0], [30.0, 10.0], 0.25, "B.Cu", Some("A"), &l()).unwrap(); // 2: other layer
    b.add_track([20.0, 10.0], [20.0, 15.0], 0.25, "F.Cu", Some("B"), &l()).unwrap(); // 3: other net
    b.add_track([20.0, 10.0], [25.0, 15.0], 0.25, "F.Cu", Some("A"), &l()).unwrap(); // 4: follows at start
    b.add_via([20.0, 10.0], 0.8, 0.4, Some("A"), &l()).unwrap();
    let (t, c) = b.move_items(&[0], &[], [0.0, 2.0], true, &l()).unwrap();
    assert_eq!((t, c), (1, 2));
    assert_eq!((b.tracks[0].start, b.tracks[0].end), ([10.0, 12.0], [20.0, 12.0]));
    assert_eq!((b.tracks[1].start, b.tracks[1].end), ([10.0, 12.0], [10.0, 5.0]));
    assert_eq!(b.tracks[2].start, [20.0, 10.0]);
    assert_eq!(b.tracks[3].start, [20.0, 10.0]);
    assert_eq!((b.tracks[4].start, b.tracks[4].end), ([20.0, 12.0], [25.0, 15.0]));
    // Vias stay put.
    assert_eq!(b.vias[0].at, [20.0, 10.0]);
    // Without `attached` nothing follows; named tracks never count as neighbours.
    b.move_items(&[0], &[], [0.0, 1.0], false, &l()).unwrap();
    assert_eq!(b.tracks[1].start, [10.0, 12.0]);
    let (_, c) = b.move_items(&[0, 1], &[], [1.0, 0.0], true, &l()).unwrap();
    assert_eq!(c, 0);
    assert_eq!(b.tracks[1].start, [11.0, 12.0]);
}

#[test]
fn a_footprint_carries_the_track_ends_on_its_pads() {
    let mut b = pair();
    b.add_track([10.0, 10.0], [20.0, 10.0], 0.25, "F.Cu", Some("A"), &l()).unwrap(); // pad J1.1 centre
    b.add_track([10.0, 12.54], [20.0, 12.54], 0.25, "F.Cu", Some("A"), &l()).unwrap(); // pad J1.2 is net B
    b.add_track([10.3, 10.0], [20.0, 8.0], 0.25, "F.Cu", Some("A"), &l()).unwrap(); // inside J1.1 copper
    b.add_track([10.0, 10.0], [20.0, 7.0], 0.25, "B.Cu", Some("A"), &l()).unwrap(); // thru pad: both layers
    let mut m = b.clone();
    let old = m.footprint_pads(0);
    m.move_footprint(0, [12.0, 11.0], 0.0, &l()).unwrap();
    assert_eq!(m.carry_track_ends(0, &old, 0.0, &l()).unwrap(), 3);
    assert_eq!(m.tracks[0].start, [12.0, 11.0]);
    assert_eq!(m.tracks[0].end, [20.0, 10.0]);
    assert_eq!(m.tracks[1].start, [10.0, 12.54]);
    assert!((m.tracks[2].start[0] - 12.3).abs() < 1e-9 && (m.tracks[2].start[1] - 11.0).abs() < 1e-9);
    assert_eq!(m.tracks[3].start, [12.0, 11.0]);
    // Turning by 90 degrees about the footprint's place turns the offset end too.
    let mut r = b.clone();
    let old = r.footprint_pads(0);
    r.move_footprint(0, [10.0, 10.0], 90.0, &l()).unwrap();
    assert_eq!(r.carry_track_ends(0, &old, 90.0, &l()).unwrap(), 3);
    assert_eq!(r.tracks[0].start, [10.0, 10.0]);
    // Offset (0.3, 0) from pad 1 turns to (0, -0.3).
    assert!((r.tracks[2].start[0] - 10.0).abs() < 1e-9 && (r.tracks[2].start[1] - 9.7).abs() < 1e-9);
    assert_eq!(Board::read(&r.write(), &l()).unwrap().tracks, r.tracks);
    // Nothing on the pads of the other footprint: nothing carried.
    let old = b.footprint_pads(1);
    b.move_footprint(1, [31.0, 10.0], 0.0, &l()).unwrap();
    assert_eq!(b.carry_track_ends(1, &old, 0.0, &l()).unwrap(), 0);
}
