#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits};
use crate::route::{self, Obstacles, Seg};
use crate::rules::Rules;
use crate::{drc, ratsnest};

/// A 40 × 20 mm board: R1 pad 2 (A) at (5.825, 10) and R2 pad 1 (A) at (34.175, 10), with
/// a wall of SIG copper across the middle (x = 20, y 2..16) leaving a way round the bottom.
fn board(extra: &str) -> Board {
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "SIG") (net 3 "GND")
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (footprint "R" (layer "F.Cu") (at 5 10)
    (property "Reference" "R1" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu") (net 3 "GND"))
    (pad "2" smd rect (at 0.825 0) (size 0.8 0.95) (layers "F.Cu") (net 1 "A")))
  (footprint "R" (layer "F.Cu") (at 35 10)
    (property "Reference" "R2" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at -0.825 0) (size 0.8 0.95) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 0.825 0) (size 0.8 0.95) (layers "F.Cu") (net 3 "GND")))
  (segment (start 20 2) (end 20 16) (width 0.5) (layer "F.Cu") (net 2))
  {extra}
)"#
    );
    Board::read(&text, &Limits::default()).unwrap()
}

const A1: [f64; 2] = [5.825, 10.0];
const A2: [f64; 2] = [34.175, 10.0];

fn octilinear(pts: &[[f64; 2]]) -> bool {
    pts.windows(2).all(|w| {
        let (dx, dy) = ((w[1][0] - w[0][0]).abs(), (w[1][1] - w[0][1]).abs());
        dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6
    })
}

fn add(b: &mut Board, segs: &[Seg], layer: &str) {
    b.add_route(segs, 0.25, layer, Some("A"), &Limits::default()).unwrap();
}

#[test]
fn a_free_route_is_one_or_two_45_degree_segments() {
    let b = board("");
    let rules = Rules::for_board(&b);
    let obs = Obstacles::new(&b, &rules, "B.Cu", Some("A"), 0.25);
    // On B.Cu the wall is not there (the pads are on F.Cu only: they are no obstacle either).
    let p = route::walkaround(&obs, [5.0, 5.0], [15.0, 8.0], 0.25, true).unwrap();
    assert_eq!(p, vec![[5.0, 5.0], [8.0, 8.0], [15.0, 8.0]]);
    let p = route::walkaround(&obs, [5.0, 5.0], [15.0, 8.0], 0.25, false).unwrap();
    assert_eq!(p, vec![[5.0, 5.0], [12.0, 5.0], [15.0, 8.0]]);
    // Outside the board there is no way.
    assert!(route::walkaround(&obs, [5.0, 5.0], [45.0, 8.0], 0.25, true).is_none());
}

#[test]
fn routes_walk_around_other_nets_and_pass_the_drc() {
    let mut b = board("");
    let rules = Rules::for_board(&b);
    assert_eq!(ratsnest::ratsnest(&b).unrouted(), 2);
    let obs = Obstacles::new(&b, &rules, "F.Cu", Some("A"), 0.25);
    let p = route::walkaround(&obs, A1, A2, 0.25, true).unwrap();
    assert_eq!((p[0], *p.last().unwrap()), (A1, A2));
    assert!(octilinear(&p), "{p:?}");
    // Round the wall's bottom end: below y = 16 + 0.25 + 0.2 + 0.125.
    assert!(p.iter().any(|q| q[1] >= 16.575 - 1e-3), "{p:?}");
    // Hugging: not much longer than the way round the wall's end.
    let len: f64 = p.windows(2).map(|w| libm::hypot(w[1][0] - w[0][0], w[1][1] - w[0][1])).sum();
    assert!(len < 2.0 * 30.0, "{len}");
    assert!(p.len() <= 8, "few corners: {p:?}");
    let segs = route::with_arcs(&obs, &p, 0.0);
    add(&mut b, &segs, "F.Cu");
    assert_eq!(ratsnest::ratsnest(&b).unrouted(), 1, "the A connection is routed");
    let v = drc::check(&b, &rules, 50);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn arc_corners_are_written_read_and_clear() {
    let mut b = board("");
    let rules = Rules::for_board(&b);
    let obs = Obstacles::new(&b, &rules, "F.Cu", Some("A"), 0.25);
    let p = route::walkaround(&obs, A1, A2, 0.25, true).unwrap();
    let segs = route::with_arcs(&obs, &p, 1.0);
    assert!(segs.iter().any(|s| s.mid.is_some()), "{segs:?}");
    // The pieces join end to end.
    for w in segs.windows(2) {
        assert!((w[0].end[0] - w[1].start[0]).abs() < 1e-9 && (w[0].end[1] - w[1].start[1]).abs() < 1e-9);
    }
    let straight: f64 = route::length(&route::with_arcs(&obs, &p, 0.0));
    assert!(route::length(&segs) < straight);
    add(&mut b, &segs, "F.Cu");
    let arcs = b.tracks.iter().filter(|t| t.mid.is_some()).count();
    assert_eq!(arcs, segs.iter().filter(|s| s.mid.is_some()).count());
    let text = b.write();
    assert!(text.contains("(arc") && text.contains("(mid"), "arcs written");
    assert!(drc::check(&b, &rules, 50).is_empty());
    assert_eq!(ratsnest::ratsnest(&b).unrouted(), 1);
}

#[test]
fn blocked_ends_and_vias() {
    // A SIG ring of track around R2's A pad: no way in.
    let ring = r#"(segment (start 31 7) (end 37 7) (width 0.3) (layer "F.Cu") (net 2))
      (segment (start 37 7) (end 37 13) (width 0.3) (layer "F.Cu") (net 2))
      (segment (start 37 13) (end 31 13) (width 0.3) (layer "F.Cu") (net 2))
      (segment (start 31 13) (end 31 7) (width 0.3) (layer "F.Cu") (net 2))"#;
    let b = board(ring);
    let rules = Rules::for_board(&b);
    let obs = Obstacles::new(&b, &rules, "F.Cu", Some("A"), 0.25);
    assert!(route::walkaround(&obs, A1, A2, 0.25, true).is_none());
    // A target inside another net's clearance is refused at once.
    assert!(route::walkaround(&obs, A1, [20.3, 10.0], 0.25, true).is_none());
    // Vias: clear in the open, not on the wall.
    assert!(route::via_clear(&b, &rules, [10.0, 4.0], Some("A"), 0.6));
    assert!(!route::via_clear(&b, &rules, [20.5, 10.0], Some("A"), 0.6));
    assert!(!route::via_clear(&b, &rules, [0.2, 10.0], Some("A"), 0.6), "too near the edge");
}
