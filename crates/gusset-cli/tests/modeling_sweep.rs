//! Sweep and loft features with the real OCCT kernel (ADR 0017): analytic volumes, twist,
//! persistent face names across parameter edits, edge-reference paths and clean errors.
//! Mock-kernel coverage (document, text, commands) lives in the other crates.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{ErrorCode, Evaluation, Evaluator, FeatureStatus};
use std::f64::consts::PI;
use std::sync::Arc;

/// A circle (r = 5) on XY and a vertical line path of length `len` on XZ.
const STRAIGHT: &str = "gusset 1
units mm

param len = 20 mm
param r = 5 mm

sketch Profile (plane: XY, next_id: 6) {
  p1 = point(0, 0)
  c2 = circle(p1, 5)
  k3 = fixed(p1)
  k4 = radius(c2, r)
}

sketch Path (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 20)
  l3 = line(p1, p2)
  k4 = fixed(p1)
  k5 = vertical(l3)
  k6 = distance(l3, l3, len)
}
";

/// A 10 x 10 square on XY and the same vertical path.
const SQUARE: &str = "gusset 1
units mm

param len = 20 mm
param turn = 90 deg

sketch Profile (plane: XY, next_id: 9) {
  p1 = point(-5, -5)
  p2 = point(5, -5)
  p3 = point(5, 5)
  p4 = point(-5, 5)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

sketch Path (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 20)
  l3 = line(p1, p2)
  k4 = fixed(p1)
  k5 = vertical(l3)
  k6 = distance(l3, l3, len)
}
";

/// A quarter circle path of radius 20 that starts at the origin heading up (+Z) and bends
/// towards +X, on the XZ plane.
const ARC_PATH: &str = "
sketch Bend (plane: XZ, next_id: 6) {
  p1 = point(20, 0)
  p2 = point(20, 20)
  p3 = point(0, 0)
  a4 = arc(p1, p2, p3)
}
";

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn eval(base: &str, extra: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(&format!("{base}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

fn names(r: &Evaluation, body: &str) -> Vec<String> {
    r.body(body).unwrap().names.faces.iter().map(ToString::to_string).collect()
}

fn centroid(r: &Evaluation, body: &str, face: &str) -> [f64; 3] {
    let b = r.body(body).unwrap();
    let i = b
        .names
        .faces
        .iter()
        .position(|f| f.to_string() == face)
        .unwrap_or_else(|| panic!("{face} in {:?}", names(r, body)));
    b.topology.faces[i].centroid
}

fn rel(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

fn ok(r: &Evaluation) {
    assert!(r.errors().is_empty(), "{:?}", r.errors());
}

#[test]
fn straight_sweep_has_the_volume_of_a_cylinder_and_named_faces() {
    let (ev, r) = eval(STRAIGHT, "sweep Sweep1 (profile: Profile, path: Path, orientation: frenet, op: new)");
    ok(&r);
    assert!(rel(volume(&ev, &r, "Sweep1"), PI * 25.0 * 20.0, 1e-9), "{}", volume(&ev, &r, "Sweep1"));
    let n = names(&r, "Sweep1");
    assert_eq!(n.len(), 3, "{n:?}");
    for f in ["Sweep1/cap(start)", "Sweep1/cap(end)", "Sweep1/side(Profile/c2)"] {
        assert!(n.contains(&f.to_string()), "{f} in {n:?}");
    }
    // The end cap sits at the end of the path and follows parameter edits.
    assert!(rel(centroid(&r, "Sweep1", "Sweep1/cap(end)")[2], 20.0, 1e-9));
    let (ev, r) = eval(
        &STRAIGHT.replace("param len = 20 mm", "param len = 33 mm"),
        "sweep Sweep1 (profile: Profile, path: Path, orientation: frenet, op: new)",
    );
    ok(&r);
    assert!(rel(centroid(&r, "Sweep1", "Sweep1/cap(end)")[2], 33.0, 1e-9));
    assert!(rel(volume(&ev, &r, "Sweep1"), PI * 25.0 * 33.0, 1e-9));
}

#[test]
fn sweep_along_a_bend_matches_pappus_for_both_orientations() {
    // Both profile kinds, with the path drawn in either direction.
    for profile in [
        "sketch Round (plane: XY, next_id: 3) {\n  p1 = point(0, 0)\n  c2 = circle(p1, 3)\n}",
        "sketch Round (plane: XY, next_id: 9) {\n  p1 = point(-3, -3)\n  p2 = point(3, -3)\n  p3 = point(3, 3)\n  p4 = point(-3, 3)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}",
    ] {
        let area = if profile.contains("circle") { PI * 9.0 } else { 36.0 };
        let (ev, r) = eval(
            "gusset 1\nunits mm\n",
            &format!("{profile}\n{ARC_PATH}\nsweep Sweep1 (profile: Round, path: Bend, orientation: frenet, op: new)"),
        );
        ok(&r);
        // The profile's centroid travels the quarter circle of radius 20.
        assert!(rel(volume(&ev, &r, "Sweep1"), area * PI / 2.0 * 20.0, 1e-5), "{}", volume(&ev, &r, "Sweep1"));
        let bb = ev.kernel().bbox(&r.body("Sweep1").unwrap().shape).unwrap();
        assert!(bb.max[0] > 19.0 && bb.max[2] > 19.0, "{bb:?}");
    }
}

#[test]
fn fixed_orientation_keeps_the_profile_flat() {
    // A rectangle swept along a straight line, either way, is a prism.
    let (ev, r) = eval(SQUARE, "sweep Sweep1 (profile: Profile, path: Path, orientation: fixed, op: new)");
    ok(&r);
    assert!(rel(volume(&ev, &r, "Sweep1"), 2000.0, 1e-9));
    // A flat circle slid along a 45 degree line is an oblique cylinder: V = A * height, for
    // both orientations (a straight path needs no turning).
    for orientation in ["fixed", "frenet"] {
        let (ev, r) = eval(
            "gusset 1\nunits mm\n",
            &format!(
                "sketch Round (plane: XY, next_id: 3) {{\n  p1 = point(0, 0)\n  c2 = circle(p1, 3)\n}}\nsketch Slant (plane: XZ, next_id: 4) {{\n  p1 = point(0, 0)\n  p2 = point(10, 10)\n  l3 = line(p1, p2)\n}}\nsweep Sweep1 (profile: Round, path: Slant, orientation: {orientation}, op: new)"
            ),
        );
        ok(&r);
        assert!(rel(volume(&ev, &r, "Sweep1"), PI * 9.0 * 10.0, 1e-4), "{orientation}: {}", volume(&ev, &r, "Sweep1"));
    }
}

#[test]
fn twist_turns_the_profile_but_keeps_the_volume() {
    let (ev, r) =
        eval(SQUARE, "sweep Sweep1 (profile: Profile, path: Path, orientation: frenet, twist: turn, op: new)");
    ok(&r);
    // Every cross-section is the same square, only turned: V = A * L (adaptive integration).
    assert!(rel(volume(&ev, &r, "Sweep1"), 2000.0, 1e-5), "{}", volume(&ev, &r, "Sweep1"));
    // The bottom edge (0, -5) of the profile ends up at (5, 0) after a quarter turn
    // (counter-clockwise about the path direction, +Z).
    let b = r.body("Sweep1").unwrap();
    let k = ev.kernel();
    let end_cap = b.names.faces.iter().position(|f| f.to_string() == "Sweep1/cap(end)").unwrap();
    assert!(end_cap < b.topology.faces.len());
    let edge = b
        .names
        .edges
        .iter()
        .position(|e| e.to_string() == "edge(Sweep1/cap(end), Sweep1/side(Profile/l5))")
        .unwrap_or_else(|| panic!("{:?}", b.names.edges.iter().map(ToString::to_string).collect::<Vec<_>>()));
    let m = b.topology.edges[edge].midpoint;
    assert!((m[0] - 5.0).abs() < 1e-4 && m[1].abs() < 1e-4 && (m[2] - 20.0).abs() < 1e-6, "{m:?}");
    let bb = k.bbox(&b.shape).unwrap();
    assert!(bb.max[2] > 19.99 && bb.min[2] < 0.01, "{bb:?}");
    // The twist follows its expression.
    let (_, r2) = eval(
        &SQUARE.replace("param turn = 90 deg", "param turn = 180 deg"),
        "sweep Sweep1 (profile: Profile, path: Path, orientation: frenet, twist: turn, op: new)",
    );
    ok(&r2);
    let b2 = r2.body("Sweep1").unwrap();
    let e2 =
        b2.names.edges.iter().position(|e| e.to_string() == "edge(Sweep1/cap(end), Sweep1/side(Profile/l5))").unwrap();
    let m2 = b2.topology.edges[e2].midpoint;
    assert!(m2[0].abs() < 1e-3 && (m2[1] - 5.0).abs() < 1e-3, "{m2:?}");
}

#[test]
fn twist_along_a_bend_keeps_pappus_volume() {
    let (ev, r) = eval(
        "gusset 1\nunits mm\n",
        &format!(
            "sketch Sq (plane: XY, next_id: 9) {{\n  p1 = point(-3, -3)\n  p2 = point(3, -3)\n  p3 = point(3, 3)\n  p4 = point(-3, 3)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}}\n{ARC_PATH}\nsweep Sweep1 (profile: Sq, path: Bend, orientation: frenet, twist: 120 deg, op: new)"
        ),
    );
    ok(&r);
    assert!(rel(volume(&ev, &r, "Sweep1"), 36.0 * PI / 2.0 * 20.0, 1e-4), "{}", volume(&ev, &r, "Sweep1"));
}

#[test]
fn tube_along_a_path_has_the_hole_and_names_for_it() {
    // An annulus (r 5 / r 3) swept 20 mm.
    let (ev, r) = eval(
        "gusset 1\nunits mm\n",
        "sketch Ring (plane: XY, next_id: 5) {
  p1 = point(0, 0)
  c2 = circle(p1, 5)
  c3 = circle(p1, 3)
}
sketch Path (plane: XZ, next_id: 4) {
  p1 = point(0, 0)
  p2 = point(0, 20)
  l3 = line(p1, p2)
}
sweep Sweep1 (profile: Ring.regions(c2), path: Path, orientation: frenet, op: new)",
    );
    ok(&r);
    assert!(rel(volume(&ev, &r, "Sweep1"), PI * (25.0 - 9.0) * 20.0, 1e-9), "{}", volume(&ev, &r, "Sweep1"));
    let n = names(&r, "Sweep1");
    assert_eq!(n.len(), 4, "{n:?}");
    assert!(
        n.contains(&"Sweep1/side(Ring/c2)".to_string()) && n.contains(&"Sweep1/side(Ring/c3)".to_string()),
        "{n:?}"
    );
}

/// The holes are swept with the outer loop and joined by shared caps, without a boolean
/// (dogfooding F3-5): along a bend, with a twist, and with two holes.
#[test]
fn holed_profiles_along_bends_and_twists_keep_exact_volumes_and_names() {
    let ring = "sketch Ring (plane: XY, next_id: 6) {
  p1 = point(0, 0)
  c2 = circle(p1, 5)
  c3 = circle(p1, 3)
}";
    let (ev, r) = eval(
        "gusset 1\nunits mm\n",
        &format!(
            "{ring}\n{ARC_PATH}\nsweep Sweep1 (profile: Ring.regions(c2), path: Bend, orientation: frenet, op: new)"
        ),
    );
    ok(&r);
    // Pappus: the annulus' centroid travels the quarter circle of radius 20.
    let v = volume(&ev, &r, "Sweep1");
    assert!(rel(v, PI * (25.0 - 9.0) * PI / 2.0 * 20.0, 1e-5), "{v}");
    let n = names(&r, "Sweep1");
    assert_eq!(n.len(), 4, "{n:?}");
    assert!(n.iter().any(|x| x.contains("cap(start)")) && n.iter().any(|x| x.contains("cap(end)")), "{n:?}");
    // Two holes in a square, twisted half a turn along a straight path: the volume is the
    // profile area times the length.
    let plate = "sketch Plate (plane: XY, next_id: 12) {
  p1 = point(-10, -6)
  p2 = point(10, -6)
  p3 = point(10, 6)
  p4 = point(-10, 6)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
  p9 = point(-5, 0)
  c10 = circle(p9, 2)
  c11 = circle(p1, 1)
}";
    let plate = plate
        .replace("c11 = circle(p1, 1)", "p11 = point(5, 0)\n  c12 = circle(p11, 2)")
        .replace("next_id: 12", "next_id: 13");
    let (ev, r) = eval(
        "gusset 1\nunits mm\n",
        &format!(
            "{plate}\nsketch Path (plane: XZ, next_id: 4) {{\n  p1 = point(0, 0)\n  p2 = point(0, 20)\n  l3 = line(p1, p2)\n}}\n\
             sweep Sweep1 (profile: Plate.regions(l5), path: Path, orientation: frenet, twist: 180 deg, op: new)"
        ),
    );
    ok(&r);
    let v = volume(&ev, &r, "Sweep1");
    assert!(rel(v, (240.0 - 2.0 * PI * 4.0) * 20.0, 1e-5), "{v}");
    assert_eq!(names(&r, "Sweep1").len(), 8, "{:?}", names(&r, "Sweep1"));
}

#[test]
fn polyline_path_is_mitred_and_faces_get_path_suffixes() {
    // An L path: up 20 mm, then along +X 30 mm. A circle r = 2 swept around the corner.
    let (ev, r) = eval(
        "gusset 1\nunits mm\n",
        "sketch Profile (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 2)
}
sketch Path (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 20)
  p3 = point(30, 20)
  l4 = line(p1, p2)
  l5 = line(p2, p3)
}
sweep Sweep1 (profile: Profile, path: Path, op: new)",
    );
    ok(&r);
    // A mitred pipe has the volume of the straight lengths: A * (L1 + L2).
    assert!(rel(volume(&ev, &r, "Sweep1"), PI * 4.0 * 50.0, 1e-6), "{}", volume(&ev, &r, "Sweep1"));
    let n = names(&r, "Sweep1");
    assert!(n.contains(&"Sweep1/side(Profile/c2)".to_string()), "{n:?}");
    assert!(n.contains(&"Sweep1/side(Profile/c2_p1)".to_string()), "{n:?}");
    // Selecting only the first curve gives the first leg alone.
    let (ev, r) = eval(
        "gusset 1\nunits mm\n",
        "sketch Profile (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 2)
}
sketch Path (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 20)
  p3 = point(30, 20)
  l4 = line(p1, p2)
  l5 = line(p2, p3)
}
sweep Sweep1 (profile: Profile, path: Path.curves(l4), op: new)",
    );
    ok(&r);
    assert!(rel(volume(&ev, &r, "Sweep1"), PI * 4.0 * 20.0, 1e-9));
}

#[test]
fn closed_path_makes_a_ring_without_caps() {
    // A circle r = 2 swept around a circle of radius 20 (a torus): V = A * 2 pi R.
    let (ev, r) = eval(
        "gusset 1\nunits mm\n",
        "sketch Profile (plane: XZ, next_id: 3) {
  p1 = point(20, 0)
  c2 = circle(p1, 2)
}
sketch Path (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 20)
}
sweep Sweep1 (profile: Profile, path: Path, op: new)",
    );
    ok(&r);
    assert!(rel(volume(&ev, &r, "Sweep1"), PI * 4.0 * 2.0 * PI * 20.0, 1e-6), "{}", volume(&ev, &r, "Sweep1"));
    let n = names(&r, "Sweep1");
    assert!(n.iter().all(|f| !f.contains("cap(")), "{n:?}");
}

#[test]
fn sweep_along_body_edges() {
    let base = "gusset 1
units mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)

sketch Wire (plane: YZ, next_id: 3) {
  p1 = point(0, 10)
  c2 = circle(p1, 2)
}
";
    // Two edges of the top face (40 mm along X, then 20 mm along Y), given out of order.
    let (ev, r) = eval(
        base,
        "sweep Sweep1 (profile: Wire, path: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6)), edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5))], op: new)",
    );
    ok(&r);
    assert!(rel(volume(&ev, &r, "Sweep1"), PI * 4.0 * 60.0, 1e-6), "{}", volume(&ev, &r, "Sweep1"));
    // A bad edge reference is a missing reference, not a panic.
    let (_, r) =
        eval(base, "sweep Sweep1 (profile: Wire, path: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l9))], op: new)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::MissingReference, "{e}");
}

#[test]
fn sweep_can_cut_and_add() {
    let base = "gusset 1
units mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(-20, -20)
  p2 = point(20, -20)
  p3 = point(20, 20)
  p4 = point(-20, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Block (profile: Sketch1, distance: 30 mm, op: new)

sketch Drill (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 4)
}

sketch Path (plane: XZ, next_id: 4) {
  p1 = point(0, 0)
  p2 = point(0, 30)
  l3 = line(p1, p2)
}
";
    let (ev, r) = eval(base, "sweep Sweep1 (profile: Drill, path: Path, op: cut, body: Block)");
    ok(&r);
    assert!(rel(volume(&ev, &r, "Block"), 40.0 * 40.0 * 30.0 - PI * 16.0 * 30.0, 1e-9), "{}", volume(&ev, &r, "Block"));
}

#[test]
fn bad_paths_fail_with_hints() {
    // The path does not touch the profile plane.
    let (_, r) = eval(
        STRAIGHT,
        "sketch Far (plane: XZ, next_id: 4) {
  p1 = point(0, 5)
  p2 = point(0, 25)
  l3 = line(p1, p2)
}
sweep Sweep1 (profile: Profile, path: Far, op: new)",
    );
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure, "{e}");
    assert!(e.message.contains("profile plane"), "{e}");
    assert!(e.hint.is_some());
    // Two separate pieces are not a chain.
    let (_, r) = eval(
        STRAIGHT,
        "sketch Broken (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 10)
  p3 = point(5, 12)
  p4 = point(5, 20)
  l5 = line(p1, p2)
  l6 = line(p3, p4)
}
sweep Sweep1 (profile: Profile, path: Broken, op: new)",
    );
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::InvalidReference, "{e}");
    assert!(e.message.contains("not one chain"), "{e}");
    // A T junction is a branch.
    let (_, r) = eval(
        STRAIGHT,
        "sketch Tee (plane: XZ, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(0, 10)
  p3 = point(5, 10)
  p4 = point(-5, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p2, p4)
}
sweep Sweep1 (profile: Profile, path: Tee, op: new)",
    );
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::InvalidReference, "{e}");
    assert!(e.message.contains("meet"), "{e}");
    // A twist on a closed path.
    let (_, r) = eval(
        "gusset 1\nunits mm\n",
        "sketch Profile (plane: XZ, next_id: 3) {
  p1 = point(20, 0)
  c2 = circle(p1, 2)
}
sketch Path (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 20)
}
sweep Sweep1 (profile: Profile, path: Path, twist: 30 deg, op: new)",
    );
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure, "{e}");
    assert!(e.hint.is_some());
    // A path sketch that failed to solve blocks the sweep instead of crashing it.
    let (_, r) = eval(
        STRAIGHT,
        "sketch Bad (plane: XZ, next_id: 7) {
  p1 = point(0, 0)
  p2 = point(0, 10)
  l3 = line(p1, p2)
  k4 = distance(l3, l3, 5 mm)
  k5 = distance(l3, l3, 6 mm)
}
sweep Sweep1 (profile: Profile, path: Bad, op: new)",
    );
    assert!(matches!(r.status("Sweep1"), Some(FeatureStatus::Blocked { .. })), "{:?}", r.status("Sweep1"));
}

#[test]
fn loft_between_squares_is_a_frustum() {
    let base = "gusset 1
units mm

param h = 15 mm
param top = 10 mm

sketch Bottom (plane: XY, next_id: 9) {
  p1 = point(-10, -10)
  p2 = point(10, -10)
  p3 = point(10, 10)
  p4 = point(-10, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

plane Up (base: XY, offset: h)

sketch Top (plane: Up, next_id: 9) {
  p1 = point(-5, -5)
  p2 = point(5, -5)
  p3 = point(5, 5)
  p4 = point(-5, 5)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}
";
    for ruled in ["true", "false"] {
        let (ev, r) = eval(base, &format!("loft Loft1 (profiles: [Bottom, Top], ruled: {ruled}, op: new)"));
        ok(&r);
        // h/3 (A1 + A2 + sqrt(A1 A2)) = 5 (400 + 100 + 200)
        assert!(rel(volume(&ev, &r, "Loft1"), 3500.0, 1e-6), "{ruled}: {}", volume(&ev, &r, "Loft1"));
        let n = names(&r, "Loft1");
        assert_eq!(n.len(), 6, "{n:?}");
        for f in ["Loft1/cap(start)", "Loft1/cap(end)", "Loft1/side(Bottom/l5)", "Loft1/side(Bottom/l8)"] {
            assert!(n.contains(&f.to_string()), "{f} in {n:?}");
        }
    }
    // The top cap follows the datum plane when the parameter changes.
    let (ev, r) =
        eval(&base.replace("param h = 15 mm", "param h = 24 mm"), "loft Loft1 (profiles: [Bottom, Top], op: new)");
    ok(&r);
    assert!(rel(centroid(&r, "Loft1", "Loft1/cap(end)")[2], 24.0, 1e-9));
    assert!(rel(volume(&ev, &r, "Loft1"), 24.0 / 3.0 * 700.0, 1e-6));
}

#[test]
fn loft_circle_to_circle_and_three_sections() {
    let base = "gusset 1
units mm

plane Mid (base: XY, offset: 10 mm)
plane Up (base: XY, offset: 20 mm)

sketch A (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 10)
}

sketch B (plane: Mid, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 5)
}

sketch C (plane: Up, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 10)
}
";
    // Cone frustum: pi h / 3 (r1^2 + r1 r2 + r2^2).
    let (ev, r) = eval(base, "loft Loft1 (profiles: [A, C], ruled: true, op: new)");
    ok(&r);
    assert!(rel(volume(&ev, &r, "Loft1"), PI * 20.0 / 3.0 * 300.0, 1e-9), "{}", volume(&ev, &r, "Loft1"));
    // A ruled loft through three circles is two cones.
    let (ev, r) = eval(base, "loft Loft1 (profiles: [A, B, C], ruled: true, op: new)");
    ok(&r);
    let cone = PI * 10.0 / 3.0 * (100.0 + 50.0 + 25.0);
    assert!(rel(volume(&ev, &r, "Loft1"), 2.0 * cone, 1e-6), "{}", volume(&ev, &r, "Loft1"));
    let n = names(&r, "Loft1");
    assert!(n.contains(&"Loft1/side(A/c2)".to_string()) && n.contains(&"Loft1/side(B/c2)".to_string()), "{n:?}");
    // Smooth through three: valid, between the two extremes.
    let (ev, r) = eval(base, "loft Loft1 (profiles: [A, B, C], op: new)");
    ok(&r);
    let v = volume(&ev, &r, "Loft1");
    assert!(v > 2000.0 && v < 6283.2, "{v}");
}

#[test]
fn loft_square_to_circle_names_are_unique() {
    let (ev, r) = eval(
        "gusset 1
units mm

plane Up (base: XY, offset: 20 mm)

sketch Sq (plane: XY, next_id: 9) {
  p1 = point(-8, -8)
  p2 = point(8, -8)
  p3 = point(8, 8)
  p4 = point(-8, 8)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

sketch Ci (plane: Up, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 8)
}
",
        "loft Loft1 (profiles: [Sq, Ci], op: new)",
    );
    ok(&r);
    let n = names(&r, "Loft1");
    let mut u = n.clone();
    u.sort();
    u.dedup();
    assert_eq!(u.len(), n.len(), "{n:?}");
    // A square's area is 256, a circle of r 8 is 201: the loft lies in between.
    let v = volume(&ev, &r, "Loft1");
    assert!(v > 201.0 * 20.0 && v < 256.0 * 20.0, "{v}");
}

#[test]
fn loft_errors_are_clean() {
    let two = "gusset 1
units mm

plane Up (base: XY, offset: 10 mm)

sketch A (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

sketch B (plane: Up, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

sketch Open (plane: Up, next_id: 4) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  l3 = line(p1, p2)
}
";
    let (_, r) = eval(two, "loft Loft1 (profiles: [A, Open], op: new)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::OpenProfile, "{e}");
    // The same plane twice is a degenerate loft: an error from the kernel, not a crash.
    let (_, r) = eval(two, "loft Loft1 (profiles: [A, A], op: new)");
    assert!(!r.errors().is_empty());
    assert!(r.errors()[0].hint.is_some() || r.errors()[0].code == ErrorCode::KernelFailure);
    // A top profile that lies in the other direction is still fine.
    let (ev, r) = eval(two, "loft Loft1 (profiles: [B, A], op: new)");
    ok(&r);
    assert!(rel(volume(&ev, &r, "Loft1"), 1000.0, 1e-9));
}
