#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Push-and-shove (ADR 0058): lines pushed aside keeping their shape, chains, vias.

use crate::board::{Board, Limits, Pt};
use crate::rules::Rules;
use crate::{drc, push, ratsnest, route};

fn lim() -> Limits {
    Limits::default()
}

fn part(r: &str, at: Pt, d: f64, net: &str) -> String {
    format!(
        r#"(footprint "P" (layer "F.Cu") (at {} {})
    (property "Reference" "{r}" (at 0 -1.5) (layer "F.SilkS") (hide yes) (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd circle (at 0 0) (size {d} {d}) (layers "F.Cu") {net}))"#,
        at[0], at[1]
    )
}

/// A's pads sit on either side of C's blob in a pocket closed by C's pads, the board edge
/// and B's track; the only way past the blob is where B runs, so B must be pushed up. `extra` adds items.
fn channel(extra: &str) -> Board {
    let parts = [
        part("B1", [2.0, 5.0], 0.6, "(net 2 \"B\")"),
        part("B2", [28.0, 5.0], 0.6, "(net 2 \"B\")"),
        part("A1", [8.0, 6.0], 0.5, "(net 1 \"A\")"),
        part("A2", [22.0, 6.0], 0.5, "(net 1 \"A\")"),
        part("C1", [3.0, 6.2], 1.2, "(net 3 \"C\")"),
        part("C2", [27.0, 6.2], 1.2, "(net 3 \"C\")"),
        part("C3", [15.0, 6.325], 1.2, "(net 3 \"C\")"),
    ]
    .join("\n");
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C") (net 4 "D") (net 5 "E")
  (gr_rect (start 0 0) (end 30 7) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  {parts}
  {extra}
)"#
    );
    Board::read(&text, &lim()).unwrap()
}

const B_TRACK: &str = r#"(segment (start 2 5) (end 15 5) (width 0.25) (layer "F.Cu") (net 2))
  (segment (start 15 5) (end 28 5) (width 0.25) (layer "F.Cu") (net 2))"#;

fn drc_count(b: &Board, rules: &Rules) -> usize {
    drc::check(b, rules, 1000).len()
}

/// The board with the push applied and the new track laid.
fn applied(b: &Board, p: &push::Push, net: &str, w: f64) -> Board {
    let mut nb = b.clone();
    nb.apply_push(p, "F.Cu", &lim()).unwrap();
    let segs: Vec<route::Seg> = p.path.windows(2).map(|x| route::Seg { start: x[0], end: x[1], mid: None }).collect();
    nb.add_route(&segs, w, "F.Cu", Some(net), &lim()).unwrap();
    nb
}

fn octilinear(pts: &[Pt]) -> bool {
    pts.windows(2).all(|w| {
        let (dx, dy) = ((w[1][0] - w[0][0]).abs(), (w[1][1] - w[0][1]).abs());
        dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6
    })
}

const A1: Pt = [8.0, 6.0];
const A2: Pt = [22.0, 6.0];

#[test]
fn a_pushed_track_keeps_its_shape_and_the_clearance() {
    let b = channel(B_TRACK);
    let rules = Rules::for_board(&b);
    let before = drc_count(&b, &rules);
    let obs = route::Obstacles::new(&b, &rules, "F.Cu", Some("A"), 0.25);
    assert!(route::walkaround(&obs, A1, A2, 0.1, true).is_none());
    let p = push::push(&b, &rules, "F.Cu", Some("A"), 0.25, A1, A2, 0.1, &[]).expect("a push");
    assert!(p.vias.is_empty());
    assert_eq!(p.lines.len(), 1, "{p:?}");
    assert_eq!(p.tracks, vec![0, 1]);
    for (pts, w, net) in &p.lines {
        assert_eq!((*w, net.as_deref()), (0.25, Some("B")));
        assert!(octilinear(pts), "{pts:?}");
    }
    // The ends stay; the far parts keep their place, the middle runs parallel higher up.
    let all: Vec<Pt> = p.lines.iter().flat_map(|l| l.0.clone()).collect();
    assert!(all.iter().any(|q| q[0] == 2.0 && q[1] == 5.0) && all.iter().any(|q| q[0] == 28.0 && q[1] == 5.0));
    assert!(all.iter().any(|q| q[1] < 4.9), "{all:?}");
    assert!(all.iter().all(|q| q[1] > 3.5), "pushed only as far as needed: {all:?}");
    let nb = applied(&b, &p, "A", 0.25);
    assert!(drc_count(&nb, &rules) <= before, "{:?}", drc::check(&nb, &rules, 100));
    assert_eq!(ratsnest::ratsnest(&nb).unrouted(), ratsnest::ratsnest(&b).unrouted() - 1);
}

#[test]
fn a_chain_of_three_tracks_is_pushed() {
    let extra = format!(
        "{B_TRACK}\n  (segment (start 6 4.55) (end 24 4.55) (width 0.25) (layer \"F.Cu\") (net 4))\n  \
         (segment (start 6 4.1) (end 24 4.1) (width 0.25) (layer \"F.Cu\") (net 5))"
    );
    let b = channel(&extra);
    let rules = Rules::for_board(&b);
    let before = drc_count(&b, &rules);
    let p = push::push(&b, &rules, "F.Cu", Some("A"), 0.25, A1, A2, 0.1, &[]).expect("a push");
    let nets: Vec<&str> = p.lines.iter().filter_map(|l| l.2.as_deref()).collect();
    for n in ["B", "D", "E"] {
        assert!(nets.contains(&n), "{n} pushed: {nets:?}");
    }
    for (pts, ..) in &p.lines {
        assert!(octilinear(pts), "{pts:?}");
    }
    let nb = applied(&b, &p, "A", 0.25);
    assert!(drc_count(&nb, &rules) <= before, "{:?}", drc::check(&nb, &rules, 100));
}

#[test]
fn a_via_is_pushed_with_its_tracks() {
    let extra = r#"(segment (start 2 5) (end 15 4.8) (width 0.25) (layer "F.Cu") (net 2))
  (segment (start 15 4.8) (end 28 5) (width 0.25) (layer "F.Cu") (net 2))
  (via (at 15 4.8) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
  (segment (start 15 4.8) (end 15 6.5) (width 0.25) (layer "B.Cu") (net 2))"#;
    let b = channel(extra);
    let rules = Rules::for_board(&b);
    let before = drc_count(&b, &rules);
    let p = push::push(&b, &rules, "F.Cu", Some("A"), 0.25, A1, A2, 0.05, &[]).expect("a push");
    assert_eq!(p.vias.len(), 1, "{p:?}");
    let d = p.vias[0].1;
    assert!(d[1] < -0.05, "{d:?}");
    let nb = applied(&b, &p, "A", 0.25);
    let v = nb.vias[0].at;
    assert!((v[0] - 15.0 - d[0]).abs() < 1e-6 && (v[1] - 4.8 - d[1]).abs() < 1e-6);
    // The back track followed; the front lines end on the via.
    assert!(nb.tracks.iter().any(|t| t.layer == "B.Cu" && (t.start == v || t.end == v)));
    assert_eq!(nb.tracks.iter().filter(|t| t.layer == "F.Cu" && (t.start == v || t.end == v)).count(), 2);
    assert!(drc_count(&nb, &rules) <= before, "{:?}", drc::check(&nb, &rules, 100));
    assert_eq!(ratsnest::ratsnest(&nb).unrouted(), ratsnest::ratsnest(&b).unrouted() - 1);
}

#[test]
fn pads_in_the_way_refuse_the_push() {
    let extra = format!("{B_TRACK}\n  {}", part("B3", [15.0, 5.0], 1.0, "(net 2 \"B\")"));
    let b = channel(&extra);
    let rules = Rules::for_board(&b);
    assert!(push::push(&b, &rules, "F.Cu", Some("A"), 0.25, A1, A2, 0.1, &[]).is_none());
}

#[test]
fn the_check_is_never_worse() {
    let extra = format!(
        "{B_TRACK}\n  (segment (start 6 4.4) (end 24 4.4) (width 0.25) (layer \"F.Cu\") (net 4))\n  \
         (via (at 12 3.4) (size 0.6) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (net 5))"
    );
    let b = channel(&extra);
    let rules = Rules::for_board(&b);
    let before = drc_count(&b, &rules);
    let mut kept = 0;
    for to in [A2, [18.0, 6.0], [20.0, 4.0], [12.0, 2.0], [26.0, 2.5], [15.0, 3.0]] {
        if let Some(p) = push::push(&b, &rules, "F.Cu", Some("A"), 0.25, A1, to, 0.1, &[]) {
            kept += 1;
            let nb = applied(&b, &p, "A", 0.25);
            assert!(drc_count(&nb, &rules) <= before, "to {to:?}: {:?}", drc::check(&nb, &rules, 100));
        }
    }
    assert!(kept >= 1);
}
