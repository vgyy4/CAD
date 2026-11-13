#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits};
use crate::copper::segment_segment;
use crate::gerber_read;
use crate::route::Seg;
use crate::rules::{NetClass, Rules};
use crate::tune::{self, TuneOpts};
use crate::{drc, gerber, length, pair, ratsnest};

/// 40 × 24 mm. Connectors J1 (left) and J2 (right) each carry both nets of USB (pads 2 mm
/// apart), plus a SIG wall across the middle the pair must go round.
fn board(extra: &str) -> Board {
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "USB_P") (net 2 "USB_N") (net 3 "SIG") (net 4 "CLK+") (net 5 "CLK-") (net 6 "GND")
  (gr_rect (start 0 0) (end 40 24) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (footprint "J" (layer "F.Cu") (at 4 10)
    (property "Reference" "J1" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 -1) (size 0.8 0.5) (layers "F.Cu") (net 1 "USB_P"))
    (pad "2" smd rect (at 0 1) (size 0.8 0.5) (layers "F.Cu") (net 2 "USB_N")))
  (footprint "J" (layer "F.Cu") (at 36 10)
    (property "Reference" "J2" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 -1) (size 0.8 0.5) (layers "F.Cu") (net 1 "USB_P"))
    (pad "2" smd rect (at 0 1) (size 0.8 0.5) (layers "F.Cu") (net 2 "USB_N")))
  (segment (start 20 2) (end 20 14) (width 0.5) (layer "F.Cu") (net 3))
  {extra}
)"#
    );
    Board::read(&text, &Limits::default()).unwrap()
}

fn usb() -> pair::Pair {
    pair::Pair { base: "USB".into(), p: "USB_P".into(), n: "USB_N".into() }
}

fn lay(b: &mut Board, r: &pair::PairRoute, pr: &pair::Pair) {
    b.add_route(&r.p, r.width, "F.Cu", Some(&pr.p), &Limits::default()).unwrap();
    b.add_route(&r.n, r.width, "F.Cu", Some(&pr.n), &Limits::default()).unwrap();
}

#[test]
fn pairs_are_recognised_by_kicad_names() {
    assert_eq!(pair::split("USB_P"), Some(("USB", true)));
    assert_eq!(pair::split("USB_N"), Some(("USB", false)));
    assert_eq!(pair::split("/hub/CLK+"), Some(("/hub/CLK", true)));
    assert_eq!(pair::split("CLK-"), Some(("CLK", false)));
    assert_eq!(pair::split("usb_p"), Some(("usb", true)));
    assert_eq!(pair::split("GND"), None);
    assert_eq!(pair::split("_P"), None);
    assert_eq!(pair::partner("USB_P").as_deref(), Some("USB_N"));
    assert_eq!(pair::partner("CLK-").as_deref(), Some("CLK+"));
    assert_eq!(pair::partner("SIG"), None);
    let b = board("");
    let ps = pair::pairs(&b);
    assert_eq!(ps.len(), 2, "{ps:?}");
    assert_eq!(ps.iter().find(|p| p.base == "CLK").map(|p| (p.p.as_str(), p.n.as_str())), Some(("CLK+", "CLK-")));
    assert_eq!(pair::find(&b, "USB_N"), Some(usb()));
    assert_eq!(pair::find(&b, "USB"), Some(usb()));
    assert_eq!(pair::find(&b, "SIG"), None);
    // A net without its partner on the board is no pair.
    let b = Board::read(
        r#"(kicad_pcb (version 20240108) (general (thickness 1.6)) (layers (0 "F.Cu" signal)) (net 0 "") (net 1 "A_P"))"#,
        &Limits::default(),
    )
    .unwrap();
    assert!(pair::pairs(&b).is_empty());
}

#[test]
fn width_and_gap_come_from_the_class_or_the_board() {
    let b = board("");
    let mut rules = Rules::for_board(&b);
    assert_eq!(rules.diff_pair_of("USB_P", "USB_N"), (rules.track_width, rules.clearance));
    rules.classes.push(NetClass {
        name: "HS".into(),
        diff_pair_width: Some(0.15),
        diff_pair_gap: Some(0.3),
        nets: vec!["USB_N".into()],
        ..NetClass::default()
    });
    assert_eq!(rules.diff_pair_of("USB_P", "USB_N"), (0.15, 0.3));
}

/// The coupled part: pieces of both tracks that are parallel and as long as the centre
/// line's segment run at exactly the centre pitch.
fn coupled_distance(r: &pair::PairRoute) -> f64 {
    let mut worst: f64 = 0.0;
    for w in r.centre.windows(2) {
        let mid = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
        for p in &r.p {
            // Pieces of the offset lines run beside the centre segment.
            let dp = segment_segment(mid, mid, p.start, p.end).0;
            for n in &r.n {
                let dn = segment_segment(mid, mid, n.start, n.end).0;
                if (dp - (r.width + r.gap) / 2.0).abs() < 0.3 && (dn - (r.width + r.gap) / 2.0).abs() < 0.3 {
                    worst = worst.max(((dp + dn) - (r.width + r.gap)).abs());
                }
            }
        }
    }
    worst
}

#[test]
fn a_pair_is_routed_coupled_at_the_class_width_and_gap_and_passes_the_drc() {
    let mut b = board("");
    let mut rules = Rules::for_board(&b);
    rules.classes.push(NetClass {
        name: "HS".into(),
        diff_pair_width: Some(0.15),
        diff_pair_gap: Some(0.25),
        nets: vec!["USB_P".into(), "USB_N".into()],
        ..NetClass::default()
    });
    let (w, g) = rules.diff_pair_of("USB_P", "USB_N");
    let pr = usb();
    assert_eq!(ratsnest::ratsnest(&b).unrouted(), 2, "USB twice");
    let r = pair::route_pair(&b, &rules, "F.Cu", &pr, &[[6.0, 10.0], [34.0, 10.0]], w, g).unwrap();
    assert_eq!((r.width, r.gap), (0.15, 0.25));
    // The centre line goes round the wall's end (y > 14 + 0.25 + clearance + half the wide track).
    assert!(r.centre.iter().any(|q| q[1] >= 14.0 + 0.25 + 0.2 + (2.0 * 0.15 + 0.25) / 2.0 - 1e-3), "{:?}", r.centre);
    // Both tracks end on their pads (the fan-out is the first and last piece).
    let p0 = &r.p;
    assert_eq!(p0.first().unwrap().start, [4.0, 9.0]);
    assert_eq!(p0.last().unwrap().end, [36.0, 9.0]);
    assert_eq!(r.n.first().unwrap().start, [4.0, 11.0]);
    assert_eq!(r.n.last().unwrap().end, [36.0, 11.0]);
    // Coupled along the centre line at exactly the pitch; never closer than width + gap.
    assert!(coupled_distance(&r) < 1e-6);
    let min =
        r.p.iter()
            .flat_map(|a| r.n.iter().map(move |c| segment_segment(a.start, a.end, c.start, c.end).0))
            .fold(f64::MAX, f64::min);
    assert!(min >= 0.15 + 0.25 - 1e-6, "{min}");
    // Both nets are 45° or straight to the pads only.
    lay(&mut b, &r, &pr);
    assert_eq!(ratsnest::ratsnest(&b).unrouted(), 0);
    assert!(b.tracks.iter().filter(|t| t.width == 0.15).count() >= 6);
    let v = drc::check(&b, &rules, 50);
    assert!(v.is_empty(), "{v:?}");
    // The Gerber of the paired board reads back.
    let f = gerber::write_layer(&b, "F.Cu").unwrap();
    let parsed = gerber_read::check_file(&f).unwrap();
    assert!(parsed.objects.iter().any(|o| o.net() == Some("USB_N")));
}

#[test]
fn without_a_class_the_board_defaults_apply_and_a_small_gap_is_raised() {
    let b = board("");
    let rules = Rules::for_board(&b);
    let (w, g) = rules.diff_pair_of("USB_P", "USB_N");
    let r = pair::route_pair(&b, &rules, "F.Cu", &usb(), &[[6.0, 10.0], [34.0, 10.0]], w, g).unwrap();
    assert_eq!((r.width, r.gap), (0.25, 0.2));
    let r = pair::route_pair(&b, &rules, "F.Cu", &usb(), &[[6.0, 10.0], [34.0, 10.0]], 0.2, 0.05).unwrap();
    assert_eq!(r.gap, 0.2, "never below the clearance");
}

#[test]
fn a_pair_with_no_way_is_refused() {
    // The wall reaches both board edges.
    let b = board(
        r#"(segment (start 20 0) (end 20 2) (width 0.5) (layer "F.Cu") (net 3)) (segment (start 20 14) (end 20 24) (width 0.5) (layer "F.Cu") (net 3))"#,
    );
    let rules = Rules::for_board(&b);
    let e = pair::route_pair(&b, &rules, "F.Cu", &usb(), &[[6.0, 10.0], [34.0, 10.0]], 0.2, 0.2).unwrap_err();
    assert!(e.contains("no clear way"), "{e}");
    // A point on the wall's copper has no way out.
    let b = board("");
    let e = pair::route_pair(&b, &rules, "F.Cu", &usb(), &[[6.0, 10.0], [20.0, 5.0]], 0.2, 0.2).unwrap_err();
    assert!(e.contains("no clear way"), "{e}");
}

fn arcs_board() -> Board {
    // 3-4-5 segment, a quarter circle of radius 2 and one via on net A.
    Board::read(
        r#"(kicad_pcb (version 20240108) (generator "test") (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal)) (net 0 "") (net 1 "A") (net 2 "B")
  (segment (start 0 0) (end 3 4) (width 0.2) (layer "F.Cu") (net 1))
  (arc (start 12 10) (mid 11.414213562 11.414213562) (end 10 12) (width 0.2) (layer "F.Cu") (net 1))
  (segment (start 0 20) (end 5 20) (width 0.2) (layer "B.Cu") (net 2))
  (via (at 3 4) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1)))"#,
        &Limits::default(),
    )
    .unwrap()
}

#[test]
fn lengths_sum_segments_arcs_and_vias() {
    let b = arcs_board();
    let a = length::length_of(&b, "A");
    assert_eq!((a.tracks, a.vias), (2, 1));
    // The quarter circle: pi / 2 * r (the middle point is on the circle to nine digits).
    assert!((a.track_mm - (5.0 + std::f64::consts::PI)).abs() < 1e-6, "{}", a.track_mm);
    assert!((a.via_mm - 1.6).abs() < 1e-9);
    assert!((a.total() - (5.0 + std::f64::consts::PI + 1.6)).abs() < 1e-6);
    let all = length::net_lengths(&b);
    assert_eq!(all.iter().map(|l| l.net.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    assert_eq!(all[1].total(), 5.0);
    assert_eq!(length::length_of(&b, "nothing").total(), 0.0);
    // A track in the wrong direction round the same arc has the same length.
    assert!(
        (crate::geom::arc_length([10.0, 12.0], [11.414213562, 11.414213562], [12.0, 10.0]) - std::f64::consts::PI)
            .abs()
            < 1e-6
    );
}

#[test]
fn skew_is_the_difference_between_the_two_nets() {
    let b = board(
        r#"(segment (start 22 9) (end 34 9) (width 0.2) (layer "F.Cu") (net 1))
        (segment (start 22 11) (end 25 11) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 25 11) (end 26 12) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 26 12) (end 30 12) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 30 12) (end 31 11) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 31 11) (end 34 11) (width 0.2) (layer "F.Cu") (net 2))"#,
    );
    let s = length::skews(&b);
    let usb = s.iter().find(|s| s.pair.base == "USB").unwrap();
    assert!((usb.skew() - (10.0 + 2.0 * std::f64::consts::SQRT_2 - 12.0)).abs() < 1e-9, "{}", usb.skew());
    assert_eq!(usb.shorter(), "USB_P");
    let clk = s.iter().find(|s| s.pair.base == "CLK").unwrap();
    assert_eq!(clk.skew(), 0.0);
}

fn opts(target: f64) -> TuneOpts {
    TuneOpts { target, tolerance: 0.02, amplitude: 1.0, spacing: 0.2 }
}

/// Replaces track `i` by the meander and returns the new board.
fn apply(b: &Board, t: &tune::Tuned, net: &str) -> Board {
    let mut nb = b.clone();
    let (w, layer) = (nb.tracks[t.track].width, nb.tracks[t.track].layer.clone());
    nb.delete_items(&[t.track], &[], &Limits::default()).unwrap();
    nb.add_route(&t.pieces, w, &layer, Some(net), &Limits::default()).unwrap();
    nb
}

#[test]
fn a_meander_reaches_the_target_within_the_tolerance_and_the_drc_stays_clean() {
    // One A track along y = 18 with SIG copper 3 mm above it.
    let b = board(
        r#"(segment (start 5 18) (end 35 18) (width 0.2) (layer "F.Cu") (net 6))
        (segment (start 5 15) (end 35 15) (width 0.2) (layer "F.Cu") (net 3))"#,
    );
    let rules = Rules::for_board(&b);
    let now = length::length_of(&b, "GND").total();
    assert_eq!(now, 30.0);
    for add in [0.9, 4.0, 12.0, 40.0] {
        let t = tune::tune(&b, &rules, "GND", None, &opts(now + add)).unwrap();
        assert_eq!(t.track, b.tracks.len() - 2);
        let nb = apply(&b, &t, "GND");
        let got = length::length_of(&nb, "GND").total();
        assert!((got - (now + add)).abs() <= 0.02, "{add}: {got}");
        assert!((t.added - add).abs() <= 0.02);
        assert!(t.bumps >= 1);
        // The ends still sit where they did, so the net stays joined.
        assert_eq!(nb.tracks.last().unwrap().end, [35.0, 18.0]);
        let v = drc::check(&nb, &rules, 50);
        assert!(v.is_empty(), "{add}: {v:?}");
        // Bumps stay below the SIG copper (side picked by the clearances).
        let up = nb.tracks.iter().filter(|t| t.net.id == Some(6)).flat_map(|t| [t.start[1], t.end[1]]);
        assert!(up.clone().all(|y| y > 15.5 + 0.2), "{add}");
    }
    // Both sides tried: the open side is below the track here (SIG is above, close).
    let t = tune::tune(&b, &rules, "GND", None, &opts(now + 12.0)).unwrap();
    let nb = apply(&b, &t, "GND");
    assert!(nb.tracks.iter().filter(|t| t.net.id == Some(6)).any(|t| t.start[1] > 18.5));
    // The Gerber with the meander reads back.
    gerber_read::check_file(&gerber::write_layer(&nb, "F.Cu").unwrap()).unwrap();
}

#[test]
fn tuning_is_refused_when_there_is_no_room_or_nothing_to_do() {
    let wall = |y: f64| format!(r#"(segment (start 5 {y}) (end 35 {y}) (width 0.2) (layer "F.Cu") (net 3))"#);
    // SIG copper 0.5 mm either side of the track: no bump fits.
    let b = board(&format!(
        r#"(segment (start 5 18) (end 35 18) (width 0.2) (layer "F.Cu") (net 6)) {} {}"#,
        wall(17.5),
        wall(18.5)
    ));
    let rules = Rules::for_board(&b);
    let e = tune::tune(&b, &rules, "GND", None, &opts(40.0)).unwrap_err();
    assert!(e.contains("no room") || e.contains("too short"), "{e}");
    // A target already reached, or below.
    let b = board(r#"(segment (start 5 18) (end 35 18) (width 0.2) (layer "F.Cu") (net 6))"#);
    let e = tune::tune(&b, &rules, "GND", None, &opts(30.01)).unwrap_err();
    assert!(e.contains("within the tolerance"), "{e}");
    let e = tune::tune(&b, &rules, "GND", None, &opts(20.0)).unwrap_err();
    assert!(e.contains("already"), "{e}");
    // A net with no tracks, a bad track, a target the track is too short for.
    assert!(tune::tune(&b, &rules, "USB_P", None, &opts(40.0)).unwrap_err().contains("no tracks"));
    assert!(tune::tune(&b, &rules, "GND", Some(99), &opts(40.0)).is_err());
    assert!(tune::tune(&b, &rules, "GND", None, &opts(f64::NAN)).is_err());
    let e = tune::tune(&b, &rules, "GND", None, &opts(5000.0)).unwrap_err();
    assert!(e.contains("too short") || e.contains("no room"), "{e}");
    // A gap of target below the smallest meander is refused, not faked.
    let e = tune::tune(&b, &rules, "GND", None, &TuneOpts { tolerance: 0.001, ..opts(30.1) }).unwrap_err();
    assert!(e.contains("smallest meander"), "{e}");
}

#[test]
fn the_shorter_side_of_a_pair_is_tuned_to_remove_the_skew() {
    let b = board(
        r#"(segment (start 22 9) (end 34 9) (width 0.2) (layer "F.Cu") (net 1))
        (segment (start 22 11) (end 25 11) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 25 11) (end 26 12) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 26 12) (end 30 12) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 30 12) (end 31 11) (width 0.2) (layer "F.Cu") (net 2))
        (segment (start 31 11) (end 34 11) (width 0.2) (layer "F.Cu") (net 2))"#,
    );
    let rules = Rules::for_board(&b);
    let before = length::skews(&b).into_iter().find(|s| s.pair.base == "USB").unwrap();
    let shorter = before.shorter().to_string();
    let target = before.p.total().max(before.n.total());
    let t = tune::tune(&b, &rules, &shorter, None, &opts(target)).unwrap();
    let nb = apply(&b, &t, &shorter);
    let after = length::skews(&nb).into_iter().find(|s| s.pair.base == "USB").unwrap();
    assert!(after.skew() <= 0.02, "{}", after.skew());
    let v = drc::check(&nb, &rules, 50);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn offsets_keep_the_distance_round_45_degree_corners() {
    let pts = [[0.0, 0.0], [5.0, 0.0], [8.0, 3.0], [8.0, 9.0]];
    let l = pair::offset(&pts, 0.3).unwrap();
    let r = pair::offset(&pts, -0.3).unwrap();
    let seg =
        |p: &[[f64; 2]]| -> Vec<Seg> { p.windows(2).map(|w| Seg { start: w[0], end: w[1], mid: None }).collect() };
    for (a, c) in seg(&l).iter().zip(seg(&r).iter()) {
        let d = segment_segment(a.start, a.end, c.start, c.end).0;
        assert!((d - 0.6).abs() < 1e-9, "{d}");
    }
    assert!(pair::offset(&[[0.0, 0.0], [5.0, 0.0], [0.0, 0.0]], 0.3).is_none());
}
