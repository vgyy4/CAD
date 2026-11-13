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

/// A footprint at (x, y) with a 4 x 4 mm courtyard drawn as `style` (`rect` or `lines`) on
/// the `side` ("F" or "B").
fn part(reference: &str, x: f64, y: f64, side: &str, style: &str) -> String {
    let layer = format!("{side}.CrtYd");
    let shape = if style == "rect" {
        format!(r#"(fp_rect (start -2 -2) (end 2 2) (stroke (width 0.05) (type solid)) (fill none) (layer "{layer}"))"#)
    } else {
        [((-2, -2), (2, -2)), ((2, -2), (2, 2)), ((2, 2), (-2, 2)), ((-2, 2), (-2, -2))]
            .iter()
            .map(|((a, b), (c, d))| {
                format!(
                    r#"(fp_line (start {a} {b}) (end {c} {d}) (stroke (width 0.05) (type solid)) (layer "{layer}"))"#
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        r#"(footprint "P:{reference}" (layer "{side}.Cu") (at {x} {y})
    (property "Reference" "{reference}" (at 0 -3) (layer "{side}.SilkS"))
    {shape}
  )"#
    )
}

#[test]
fn courtyards_of_one_side_that_overlap_are_reported() {
    let rules = Rules::default();
    // 1 mm overlap of two 4 x 4 courtyards: 4 mm2.
    let b = board("", &format!("{} {}", part("U1", 10.0, 10.0, "F", "rect"), part("U2", 13.0, 10.0, "F", "rect")));
    let v = only(&b, &rules, "courtyard_overlap");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Footprint(0), ItemRef::Footprint(1)]);
    assert_eq!(v[0].layer.as_deref(), Some("F.CrtYd"));
    assert!((v[0].at[0] - 11.5).abs() < 1e-6 && (v[0].at[1] - 10.0).abs() < 1e-6, "{v:?}");
    assert!(v[0].message.contains("U1") && v[0].message.contains("U2") && v[0].message.contains("4.00"), "{v:?}");
    // Courtyards drawn as four lines count the same.
    let b = board("", &format!("{} {}", part("U1", 10.0, 10.0, "F", "lines"), part("U2", 13.0, 12.0, "F", "lines")));
    assert_eq!(only(&b, &rules, "courtyard_overlap").len(), 1);
}

#[test]
fn courtyards_that_touch_or_sit_on_other_sides_are_fine() {
    let rules = Rules::default();
    // Sharing an edge (4 mm pitch).
    let b = board("", &format!("{} {}", part("U1", 10.0, 10.0, "F", "rect"), part("U2", 14.0, 10.0, "F", "rect")));
    assert!(only(&b, &rules, "courtyard_overlap").is_empty());
    // One front, one back, right on top of each other.
    let b = board("", &format!("{} {}", part("U1", 10.0, 10.0, "F", "rect"), part("U2", 10.0, 10.0, "B", "rect")));
    assert!(only(&b, &rules, "courtyard_overlap").is_empty());
    // Two backs overlap.
    let b = board("", &format!("{} {}", part("U1", 10.0, 10.0, "B", "rect"), part("U2", 11.0, 10.0, "B", "rect")));
    let v = only(&b, &rules, "courtyard_overlap");
    assert_eq!((v.len(), v[0].layer.as_deref()), (1, Some("B.CrtYd")));
    // A footprint without a courtyard is never reported.
    let bare = r#"(footprint "P:X" (layer "F.Cu") (at 10 10) (property "Reference" "X1" (at 0 0) (layer "F.SilkS")))"#;
    let b = board("", &format!("{} {bare}", part("U1", 10.0, 10.0, "F", "rect")));
    assert!(only(&b, &rules, "courtyard_overlap").is_empty());
}

/// An SMD pad 1 x 1 mm at (10, 10) on the front, with `extra` items in the footprint.
fn smd(extra: &str) -> String {
    format!(
        r#"(footprint "S:R" (layer "F.Cu") (at 10 10)
    (property "Reference" "R1" (at 0 -6) (layer "F.SilkS"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (net 0))
    {extra}
  )"#
    )
}

fn silk_line(layer: &str, y: f64) -> String {
    format!(r#"(fp_line (start -2 {y}) (end 2 {y}) (stroke (width 0.1) (type solid)) (layer "{layer}"))"#)
}

#[test]
fn silkscreen_over_a_pad_is_reported_and_clear_silk_is_not() {
    let rules = Rules::default();
    // A line through the pad.
    let b = board("", &smd(&silk_line("F.SilkS", 0.0)));
    let v = only(&b, &rules, "silk_over_pad");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Graphic(0), ItemRef::Pad(0)]);
    assert_eq!(v[0].layer.as_deref(), Some("F.SilkS"));
    assert!(v[0].message.contains("covers pad R1.1"), "{v:?}");
    assert!((v[0].at[1] - 10.0).abs() < 0.6, "{v:?}");
    // 0.8 mm beside the pad centre: pad edge at 0.5, line edge at 0.75: 0.25 mm of room.
    let b = board("", &smd(&silk_line("F.SilkS", 0.8)));
    assert!(only(&b, &rules, "silk_over_pad").is_empty());
    // A silk clearance rule turns that into a violation (0.25 < 0.4), reported with the gap.
    let strict = Rules { silk_clearance: 0.4, ..Rules::default() };
    let v = only(&b, &strict, "silk_over_pad");
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("0.250 mm") && v[0].message.contains("0.400 mm"), "{v:?}");
    // Silk on the back does not touch a front-only pad.
    let b = board("", &smd(&silk_line("B.SilkS", 0.0)));
    assert!(only(&b, &rules, "silk_over_pad").is_empty());
}

#[test]
fn the_mask_opening_counts_as_exposed() {
    let rules = Rules::default();
    // Line edge 0.05 mm outside the copper (centre 0.6, half width 0.05 -> 0.55 from the middle).
    let line = silk_line("F.SilkS", 0.6);
    let b = board("", &smd(&line));
    assert!(only(&b, &rules, "silk_over_pad").is_empty());
    // With a 0.1 mm mask expansion the opening reaches 0.6: the line is in it.
    let b = board("(pad_to_mask_clearance 0.1)", &smd(&line));
    assert_eq!(only(&b, &rules, "silk_over_pad").len(), 1);
}

#[test]
fn silk_text_over_pads_and_through_hole_pads_on_both_sides() {
    let rules = Rules::default();
    // The reference text sits on the pad (at 0 0 instead of above).
    let text = r#"(property "Reference" "R1" (at 0 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))"#;
    let src = smd("").replace(r#"(property "Reference" "R1" (at 0 -6) (layer "F.SilkS"))"#, text);
    let v = only(&board("", &src), &rules, "silk_over_pad");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Text(0), ItemRef::Pad(0)]);
    assert!(v[0].message.contains("text \"R1\""), "{v:?}");
    // Hidden text is not drawn, so it is not checked.
    let hidden = text.replace("(effects", "(effects hide").replace("(layer \"F.SilkS\")", "(layer \"F.SilkS\") hide");
    let src = smd("").replace(r#"(property "Reference" "R1" (at 0 -6) (layer "F.SilkS"))"#, &hidden);
    assert!(only(&board("", &src), &rules, "silk_over_pad").is_empty());
    // A through-hole pad is exposed on both sides.
    let line = |layer: &str| {
        format!(r#"(fp_line (start -2 0) (end 2 0) (stroke (width 0.1) (type solid)) (layer "{layer}"))"#)
    };
    for layer in ["F.SilkS", "B.SilkS"] {
        let b = board("", &thru("J1", 10.0, 10.0, "1", &line(layer)));
        assert_eq!(only(&b, &rules, "silk_over_pad").len(), 1, "{layer}");
    }
}
