//! Hole features against the mock kernel: arguments, faces, sites and the face-fit check are
//! decided before the kernel is asked, and a kernel without revolve fails cleanly. The real
//! geometry (volumes, names, edits) is tested with OCCT in `gusset-cli/tests/modeling_hole.rs`.
// Test-only code: expected values may use std float functions.
#![allow(clippy::disallowed_methods)]

use super::*;
use crate::hole::Dims;
use gusset_kernel::mock::MockKernel;
use std::f64::consts::PI;

const BASE: &str = "gusset 1
units mm

param d = 6 mm

sketch Plate (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Plate, distance: 10 mm, op: new)

sketch Marks (plane: Extrude1/cap(end), next_id: 12) {
  p1 = point(10, 10)
  p2 = point(30, 10)
  p3 = point(20, 5) construction
  p4 = point(5, 15)
  p5 = point(8, 15)
  l6 = line(p4, p5)
  p7 = point(30, 15)
  c8 = circle(p7, 2)
}
";

fn run(extra: &str) -> Evaluation {
    let doc = Document::from_text(&format!("{BASE}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    Evaluator::new(Arc::new(MockKernel)).evaluate(&doc, None, &())
}

fn failure(r: &Evaluation, feature: &str) -> EvalError {
    match r.status(feature) {
        Some(FeatureStatus::Failed(e)) => e.clone(),
        other => panic!("{feature}: {other:?}"),
    }
}

const TOP: &str = "Extrude1/cap(end)";

#[test]
fn sketch_sites_are_points_and_circle_centres_in_id_order() {
    let doc = Document::from_text(&format!("{BASE}\n")).unwrap();
    let FeatureKind::Sketch(def) = &doc.feature("Marks").unwrap().kind else { panic!() };
    let params = gusset_units::params::evaluate(&doc.params);
    let solved = sketch::solve_sketch("Marks", def, &params, LengthUnit::Millimetre, Frame::XY).unwrap();
    let ids: Vec<&str> = solved.hole_sites.iter().map(|(i, _)| i.as_str()).collect();
    // p3 is construction, p4/p5 end a line; p7 is a circle centre.
    assert_eq!(ids, ["p1", "p2", "p7"]);
    assert_eq!(solved.hole_sites[2].1, [30.0, 15.0]);
}

#[test]
fn a_kernel_without_revolve_fails_cleanly_after_the_checks() {
    for extra in [
        format!("hole Hole1 (face: {TOP}, points: Marks, diameter: d, through: true)"),
        format!(
            "hole Hole1 (face: {TOP}, at: [[10 mm, 10 mm]], type: counterbore, diameter: d, depth: 8 mm, cbore_diameter: 10 mm, cbore_depth: 3 mm)"
        ),
        format!(
            "hole Hole1 (face: {TOP}, at: [[10 mm, 10 mm]], type: tapped, diameter: 5 mm, depth: 8 mm, thread: \"M6\", pitch: 1 mm)"
        ),
    ] {
        let r = run(&format!("{extra}\n\nextrude After (profile: Plate, distance: 1 mm, op: new, body_name: Later)"));
        let e = failure(&r, "Hole1");
        assert_eq!(e.code, ErrorCode::KernelFailure);
        assert!(e.message.contains("revolve is not supported"), "{e}");
        // The rebuild goes on with the features that do not depend on it.
        assert!(matches!(r.status("After"), Some(FeatureStatus::Ok)), "{:?}", r.status("After"));
    }
}

#[test]
fn bad_dimensions_have_hints() {
    for (args, why) in [
        ("type: simple, diameter: 0 mm, through: true", "diameter must be positive"),
        ("type: simple, diameter: 2 mm, depth: 0 mm", "depth must be positive"),
        ("type: simple, diameter: 2 mm, depth: 5 mm, point_angle: 10 deg", "drill point angle"),
        ("type: simple, diameter: 2 mm, depth: 5 mm, point_angle: 200 deg", "drill point angle"),
        ("type: counterbore, diameter: 6 mm, through: true, cbore_diameter: 6 mm, cbore_depth: 2 mm", "not larger"),
        (
            "type: counterbore, diameter: 6 mm, depth: 4 mm, cbore_diameter: 10 mm, cbore_depth: 4 mm",
            "not less than the hole depth",
        ),
        ("type: countersink, diameter: 6 mm, through: true, csink_diameter: 5 mm, csink_angle: 90 deg", "not larger"),
        (
            "type: countersink, diameter: 6 mm, through: true, csink_diameter: 9 mm, csink_angle: 180 deg",
            "countersink angle",
        ),
        (
            "type: countersink, diameter: 6 mm, depth: 1 mm, csink_diameter: 12 mm, csink_angle: 90 deg",
            "as deep as the hole",
        ),
        ("type: tapped, diameter: 5 mm, depth: 6 mm, thread: \"M6\", pitch: 1 mm, thread_depth: 8 mm", "thread depth"),
        ("type: tapped, diameter: 5 mm, depth: 6 mm, thread: \"M6\", pitch: 0 mm", "thread pitch must be positive"),
        ("diameter: 30 deg, through: true", ""),
    ] {
        let r = run(&format!("hole Hole1 (face: {TOP}, at: [[10 mm, 10 mm]], {args})"));
        let e = failure(&r, "Hole1");
        assert!(matches!(e.code, ErrorCode::Expression), "{args}: {e}");
        assert!(e.message.contains(why) || e.to_string().contains(why), "{args}: {e}");
        assert!(why.is_empty() || e.hint.is_some(), "{args}: {e}");
    }
}

#[test]
fn faces_and_sites_are_checked() {
    // A missing face, with a suggestion.
    let r = run("hole Hole1 (face: Extrude1/cap(start, l9), at: [[1 mm, 1 mm]], diameter: 2 mm, through: true)");
    let e = failure(&r, "Hole1");
    assert_eq!(e.code, ErrorCode::MissingReference);
    assert!(e.suggestion.is_some() && e.hint.is_some(), "{e:?}");
    // A face that is not flat is refused (the mock has only planar prisms, so use a
    // reference to a side that exists: it is planar, hence fine up to the kernel).
    // Two holes at one place.
    let r = run(&format!("hole Hole1 (face: {TOP}, at: [[5 mm, 5 mm], [5 mm, 5 mm]], diameter: 2 mm, through: true)"));
    assert!(failure(&r, "Hole1").message.contains("same place"));
    // No sites in a sketch.
    let r = run(&format!(
        "sketch Empty (plane: {TOP}, next_id: 1) {{\n}}\n\nhole Hole1 (face: {TOP}, points: Empty, diameter: 2 mm, through: true)"
    ));
    assert!(failure(&r, "Hole1").message.contains("no points"));
    // A sketch that is not parallel to the face.
    let r = run(&format!(
        "sketch Side (plane: XZ, next_id: 2) {{\n  p1 = point(5, 5)\n}}\n\nhole Hole1 (face: {TOP}, points: Side, diameter: 2 mm, through: true)"
    ));
    assert!(failure(&r, "Hole1").message.contains("not parallel"));
}

#[test]
fn holes_outside_the_face_are_refused() {
    // The top face is 40 x 20. Centre outside, circle sticking out, and a circle that just fits.
    for (at, fits) in [
        ("[[50 mm, 10 mm]]", false),
        ("[[2 mm, 10 mm]]", false),
        ("[[20 mm, 18 mm]]", false),
        ("[[-1 mm, 10 mm]]", false),
        ("[[4 mm, 10 mm], [36 mm, 10 mm], [20 mm, 4 mm]]", true),
        ("[[20 mm, 10 mm], [200 mm, 10 mm]]", false),
    ] {
        let r = run(&format!("hole Hole1 (face: {TOP}, at: {at}, diameter: 8 mm, through: true)"));
        let e = failure(&r, "Hole1");
        if fits {
            assert!(e.message.contains("revolve is not supported"), "{at}: {e}");
        } else {
            assert_eq!(e.code, ErrorCode::InvalidReference, "{at}: {e}");
            assert!(e.message.contains("does not fit inside face"), "{at}: {e}");
            assert!(e.hint.is_some());
        }
    }
    // The counterbore decides the size of the mouth.
    let r = run(&format!(
        "hole Hole1 (face: {TOP}, at: [[5 mm, 10 mm]], type: counterbore, diameter: 4 mm, through: true, cbore_diameter: 12 mm, cbore_depth: 3 mm)"
    ));
    assert!(failure(&r, "Hole1").message.contains("does not fit inside face"));
}

/// Volume of revolution of a closed polygon (x = radius, y = depth) about the y axis.
fn revolved_volume(edges: &[gusset_kernel::ProfileEdge]) -> f64 {
    let mut first_moment = 0.0;
    for e in edges {
        let gusset_kernel::Seg2::Line { a, b } = e.seg else { panic!("{e:?}") };
        first_moment += (a[0] + b[0]) * (a[0] * b[1] - b[0] * a[1]) / 6.0;
    }
    2.0 * PI * first_moment
}

#[test]
fn the_tool_outline_is_closed_and_has_the_analytic_volume() {
    let tan = |a: f64| (a / 2.0).tan();
    let (r, l) = (3.0, 10.0);
    let angle = DEFAULT_POINT_ANGLE;
    let cone = PI * r * r * (r / tan(angle)) / 3.0;
    let cases = [
        (
            Dims { diameter: 6.0, depth: Some(l), point_angle: angle, counterbore: None, countersink: None },
            PI * r * r * l + cone,
        ),
        (Dims { diameter: 6.0, depth: Some(l), point_angle: PI, counterbore: None, countersink: None }, PI * r * r * l),
        (
            Dims { diameter: 6.0, depth: None, point_angle: angle, counterbore: None, countersink: None },
            PI * r * r * 25.0,
        ),
        (
            Dims {
                diameter: 6.0,
                depth: Some(l),
                point_angle: angle,
                counterbore: Some((11.0, 6.5)),
                countersink: None,
            },
            PI * 5.5 * 5.5 * 6.5 + PI * r * r * (l - 6.5) + cone,
        ),
        (
            Dims {
                diameter: 6.0,
                depth: Some(l),
                point_angle: angle,
                counterbore: None,
                countersink: Some((12.0, PI / 2.0)),
            },
            // Frustum of height 3 from radius 6 to 3, then the cylinder, then the cone.
            PI * 3.0 / 3.0 * (36.0 + 18.0 + 9.0) + PI * r * r * (l - 3.0) + cone,
        ),
    ];
    for (dims, expected) in cases {
        let edges = dims.outline(25.0);
        // Closed: each edge ends where the next begins.
        for (i, e) in edges.iter().enumerate() {
            let gusset_kernel::Seg2::Line { b, .. } = e.seg else { panic!() };
            let gusset_kernel::Seg2::Line { a: next, .. } = edges[(i + 1) % edges.len()].seg else { panic!() };
            assert_eq!(b, next);
        }
        let v = revolved_volume(&edges);
        assert!((v - expected).abs() < 1e-9 * expected, "{dims:?}: {v} vs {expected}");
    }
}

#[test]
fn text_edit_keeps_the_hole_in_a_state_the_cache_understands() {
    // Same document twice: the second evaluation comes from the cache.
    let doc = Document::from_text(&format!(
        "{BASE}\nhole Hole1 (face: {TOP}, at: [[10 mm, 10 mm]], diameter: d, through: true)\n"
    ))
    .unwrap();
    let mut ev = Evaluator::new(Arc::new(MockKernel));
    let a = ev.evaluate(&doc, None, &());
    let b = ev.evaluate(&doc, None, &());
    assert_eq!(a.statuses, b.statuses);
    assert_eq!(b.recomputed, 0);
}

/// Review H3: holes from a sketch are limited like explicit points (a sketch pattern could
/// give 20,000 sites, each a boolean tool).
#[test]
fn sketch_sites_are_limited() {
    let sketch = |extra: &str| {
        format!(
            "sketch Many (plane: {TOP}, next_id: 9) {{\n  p1 = point(1, 1)\n  c2 = circle(p1, 0.01)\n  \
             p3 = point(1, 3)\n  c4 = circle(p3, 0.01)\n{extra}  \
             g6 = pattern_linear(of: [c2], count: 500, dx: 0.03 mm, dy: 0 mm)\n  \
             g7 = pattern_linear(of: [c4], count: 500, dx: 0.03 mm, dy: 0 mm)\n}}\n\n\
             hole Hole1 (face: {TOP}, points: Many, diameter: 0.01 mm, through: true)"
        )
    };
    // 1,000 sites pass the count check (and then need a kernel with revolve).
    let e = failure(&run(&sketch("")), "Hole1");
    assert_eq!(e.code, ErrorCode::KernelFailure, "{e}");
    // 1,001 do not.
    let e = failure(&run(&sketch("  p5 = point(1, 5)\n")), "Hole1");
    assert_eq!(e.code, ErrorCode::TooComplex);
    assert!(e.message.contains("1001 hole positions") && e.hint.is_some(), "{e}");
}
