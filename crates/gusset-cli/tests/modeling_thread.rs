//! The `thread` feature with the real OCCT kernel (ADR 0034 amendment): a modeled thread's
//! exact volume on a shaft and in a hole, square ends inside the face, a run-out through a free
//! end, a stop at a hole's floor, and the moved face keeping its name.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{Evaluation, Evaluator};
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn eval(text: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(gusset_doc::Document::from_text(&doc.to_text()).unwrap(), doc, "round trip");
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

/// The groove's volume per mm of thread: its section between the diameters (the crest flat
/// P/8, flanks at half `angle`) turned about the axis, a pitch per turn. The width at radius
/// `r` is linear in `r`, so Simpson's rule is exact for the moment.
fn groove_per_mm(major: f64, minor: f64, p: f64, angle_deg: f64, internal: bool) -> f64 {
    let (rmaj, rmin) = (major / 2.0, minor / 2.0);
    let w = (rmaj - rmin) * libm::tan(angle_deg.to_radians() / 2.0);
    let width = |r: f64| {
        let f = (r - rmin) / (rmaj - rmin);
        if internal { 7.0 * p / 8.0 - 2.0 * w * f } else { 7.0 * p / 8.0 - 2.0 * w * (1.0 - f) }
    };
    let m = |r: f64| r * width(r);
    let rm = (rmaj + rmin) / 2.0;
    let moment = (rmaj - rmin) / 6.0 * (m(rmin) + 4.0 * m(rm) + m(rmaj));
    moment * TAU / p
}

const ROD: &str = "gusset 1
units mm

sketch S (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 8.2)
}

extrude E (profile: S, distance: 13 mm, op: new)
";

#[test]
fn a_modeled_thread_on_a_shaft_has_the_exact_volume() {
    // 1145's thread: M16x1 6g, 9 mm from 4 mm up a 16.4 mm rod 13 mm long. The rod is cut down
    // to the major diameter over its whole length, the groove ends square at z = 4 mm and runs
    // out through the top.
    let line = "thread T (face: E/side(S/c2), designation: \"M16x1 6g\", pitch: 1 mm, modeled: true, major: 15.884 mm, minor: 14.741 mm, length: 9 mm, offset: 4 mm, along: Z)";
    let (ev, r) = eval(&format!("{ROD}\n{line}\n"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let want = PI * 7.942 * 7.942 * 13.0 - 9.0 * groove_per_mm(15.884, 14.741, 1.0, 60.0, false);
    let v = volume(&ev, &r, "E");
    assert!((v - want).abs() < 2e-5 * want, "{v} vs {want}");
    // The moved face keeps its name; the groove's faces are the thread's.
    let names: Vec<String> = r.body("E").unwrap().names.faces.iter().map(ToString::to_string).collect();
    assert!(names.iter().any(|n| n.starts_with("E/side(S/c2)")), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("T/side(T/g")), "{names:?}");
    // From the top down: the same thread 4 mm below the top, the groove running out at the
    // bottom; left-handed: the same volume.
    let down = line.replace("along: Z", "along: [0, 0, -1]");
    let (ev, r) = eval(&format!("{ROD}\n{down}\n"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!((volume(&ev, &r, "E") - want).abs() < 2e-5 * want);
    let left = line.replace("along: Z)", "along: Z, left: true)");
    let (ev, r) = eval(&format!("{ROD}\n{left}\n"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!((volume(&ev, &r, "E") - want).abs() < 2e-5 * want);
    // Cosmetic: the rod is unchanged.
    let cosmetic = line.replace("modeled: true, ", "");
    let (ev, r) = eval(&format!("{ROD}\n{cosmetic}\n"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!((volume(&ev, &r, "E") - PI * 8.2 * 8.2 * 13.0).abs() < 1e-6 * want);
    // A thread wider than the rod builds the rod up to its major diameter (3692's 1/4-56 UNS
    // on a 6 mm shaft).
    let wide = line.replace("major: 15.884 mm, minor: 14.741 mm", "major: 17 mm, minor: 16 mm");
    let (ev, r) = eval(&format!("{ROD}\n{wide}\n"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let want = PI * 8.5 * 8.5 * 13.0 - 9.0 * groove_per_mm(17.0, 16.0, 1.0, 60.0, false);
    let v = volume(&ev, &r, "E");
    assert!((v - want).abs() < 2e-5 * want, "{v} vs {want}");
}

#[test]
fn a_modeled_thread_in_a_hole_fills_it_to_the_minor_diameter() {
    // 3350's Thread2: M16x1 6H in an 8 mm hole through a 3 mm plate; the hole is filled in to
    // the minor diameter, the groove cut out to the major one and out through both faces.
    let text = "gusset 1
units mm

sketch S (plane: XY, next_id: 7) {
  p1 = point(-15, -15)
  p2 = point(15, -15)
  p3 = point(15, 15)
  p4 = point(-15, 15)
  l1 = line(p1, p2)
  l2 = line(p2, p3)
  l3 = line(p3, p4)
  l4 = line(p4, p1)
  p5 = point(0, 0)
  c6 = circle(p5, 8)
}

extrude E (profile: S, distance: 3 mm, op: new)

thread T (face: E/side(S/c6), designation: \"M16x1 6H\", pitch: 1 mm, modeled: true, major: 16.1522 mm, minor: 15.0355 mm)
";
    let (ev, r) = eval(text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let rmin: f64 = 15.0355 / 2.0;
    let want = 30.0 * 30.0 * 3.0 - PI * rmin * rmin * 3.0 - 3.0 * groove_per_mm(16.1522, 15.0355, 1.0, 60.0, true);
    let v = volume(&ev, &r, "E");
    assert!((v - want).abs() < 2e-5 * want, "{v} vs {want}");
    // 4.5 mm deep: the plate's faces cross the groove at other angles.
    let (ev, r) = eval(&text.replace("distance: 3 mm", "distance: 4.5 mm"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let want = 900.0 * 4.5 - PI * rmin * rmin * 4.5 - 4.5 * groove_per_mm(16.1522, 15.0355, 1.0, 60.0, true);
    let v = volume(&ev, &r, "E");
    assert!((v - want).abs() < 2e-5 * want, "{v} vs {want}");
}

#[test]
fn a_thread_stops_square_at_a_hole_floor() {
    // A blind 8 mm hole 6 mm deep in a 10 mm block, threaded over its whole wall: the groove
    // must not run on into the block below the floor.
    let text = "gusset 1
units mm

sketch S (plane: XY, next_id: 7) {
  p1 = point(-15, -15)
  p2 = point(15, -15)
  p3 = point(15, 15)
  p4 = point(-15, 15)
  l1 = line(p1, p2)
  l2 = line(p2, p3)
  l3 = line(p3, p4)
  l4 = line(p4, p1)
}

extrude E (profile: S, distance: 10 mm, op: new)

sketch H (plane: E/cap(end), next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 4)
}

extrude C (profile: H, distance: -6 mm, op: cut, body: E)

thread T (face: C/side(H/c2), pitch: 1.25 mm, modeled: true, major: 9 mm)
";
    let (ev, r) = eval(text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The hole is 8 mm, the default minor of a 9 mm major 7.647 mm: the hole is filled in.
    let minor = 9.0 - gusset_eval::ISO_DEPTH_PITCHES * 1.25;
    let rmin = minor / 2.0;
    let want = 30.0 * 30.0 * 10.0 - PI * rmin * rmin * 6.0 - 6.0 * groove_per_mm(9.0, minor, 1.25, 60.0, true);
    let v = volume(&ev, &r, "E");
    assert!((v - want).abs() < 2e-5 * want, "{v} vs {want}");
}

/// 916's foot: a 15.884 mm rod 20 mm long with the M16x1 thread from 4 mm up, and below it a
/// groove turned into the rod (`V`) whose top wall, 0.942 mm wide, is the plane z = -16.5.
const FOOT: &str = "gusset 1
units mm

sketch S (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, RADIUS)
}

extrude E (profile: S, distance: -20 mm, op: new)

THREAD

sketch R (plane: YZ, next_id: 14) {
  p1 = point(5.5, -20)
  p2 = point(7, -18.5)
  p3 = point(7, -16.5)
  p4 = point(9.5, -16.5)
  p5 = point(9.5, -20)
  l6 = line(p1, p2)
  l7 = line(p2, p3)
  l8 = line(p3, p4)
  l9 = line(p4, p5)
  l10 = line(p5, p1)
  p11 = point(0, -25)
  p12 = point(0, 5)
  l13 = line(p11, p12)
}

revolve V (profile: R, axis: R.l13, angle: 360 deg, op: cut, body: E)
";

/// 916's thread on `FOOT`'s rod: from 4 mm up, its groove starting square at z -16.
const FOOT_THREAD: &str = "thread T (face: E/side(S/c2), pitch: 1 mm, modeled: true, major: 15.884 mm, minor: 14.741 mm, length: 16 mm, offset: 4 mm, along: Z)";

/// `FOOT` with or without the thread `line` (without it, the rod is turned to the thread's
/// major diameter, as the thread does), and with the three 0.5 mm rounds of the groove's edges
/// (r 7 and r 7.942 at z -16.5, r 7 at z -18.5) when `round`.
fn foot(line: Option<&str>, round: bool) -> f64 {
    let text = FOOT.replace("THREAD", line.unwrap_or("")).replace("RADIUS", if line.is_some() { "9" } else { "7.942" });
    let (ev, r) = eval(&text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    if !round {
        return volume(&ev, &r, "E");
    }
    let b = r.body("E").unwrap();
    let mut picks = Vec::new();
    for (i, e) in b.topology.edges.iter().enumerate() {
        let Some(c) = e.circle else { continue };
        let (z, rad) = (c.center[2], c.radius);
        if ((z + 16.5).abs() < 1e-6 && ((rad - 7.0).abs() < 1e-6 || (rad - 7.942).abs() < 1e-6))
            || ((z + 18.5).abs() < 1e-6 && (rad - 7.0).abs() < 1e-6)
        {
            picks.push(b.names.edges[i].to_string());
        }
    }
    assert_eq!(picks.len(), 3, "{picks:?}");
    let text = format!("{text}\nfillet F (edges: [{}], radius: 0.5 mm)\n", picks.join(", "));
    let (ev, r) = eval(&text);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    volume(&ev, &r, "E")
}

#[test]
fn rounds_at_a_threads_square_end_are_made_in_the_half_section() {
    // The convex round of the r 7.942 edge rolls on the concave one beside it (the wall is
    // narrower than both) and meets the rod at z -16.0017, 1.7 um below the groove's square
    // end: OCCT's fillet fails on the threaded rod and is made on the revolved half section.
    // The rounds and the thread do not meet: each changes the volume as it does alone.
    let (plain, rounded) = (foot(None, false), foot(None, true));
    let threaded = foot(Some(FOOT_THREAD), false);
    let v = foot(Some(FOOT_THREAD), true);
    let want = threaded + (rounded - plain);
    assert!((v - want).abs() < 1e-7 * want, "{v} vs {want}");
}

#[test]
fn rounds_at_a_threads_square_end_are_made_whatever_the_threads_phase() {
    // The thread run down from the rod's top, so that its groove ends square at z -16 0.8 of
    // a turn on from where 916's starts. The half section must be taken where the rod's wall
    // runs on past the square end (a tenth of a turn or so), whatever angles the circles'
    // frame puts first: the same holds for 916's on a platform whose rounding tilts the
    // circles' axis by 1e-17, which turns OCCT's default frame for it by a quarter turn.
    let line = "thread T (face: E/side(S/c2), pitch: 1 mm, modeled: true, major: 15.884 mm, minor: 14.741 mm, length: 15.8 mm, offset: 0.2 mm, along: [0, 0, -1])";
    let (plain, rounded) = (foot(None, false), foot(None, true));
    let threaded = foot(Some(line), false);
    let v = foot(Some(line), true);
    let want = threaded + (rounded - plain);
    assert!((v - want).abs() < 1e-7 * want, "{v} vs {want}");
}
