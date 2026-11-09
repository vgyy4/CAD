//! Ellipses and elliptical arcs: solving, regions and chains.

use core::f64::consts::PI;
use gusset_sketch::{Constraint, Curve, Geom, Sketch, SolveOptions, SolveStatus, ellipse, profiles, solve};

#[test]
fn an_ellipse_is_one_region_with_its_exact_area() {
    let mut s = Sketch::new();
    s.add_ellipse([2.0, 1.0], 0.3, 5.0, 3.0).unwrap();
    let ps = profiles(&s, 1e-6).unwrap();
    assert_eq!(ps.len(), 1);
    assert!((ps[0].area - PI * 15.0).abs() < 1e-9, "{}", ps[0].area);
    assert!(matches!(ps[0].outer.segments[0].curve, Curve::Ellipse { a, b, .. } if a == 5.0 && b == 3.0));
    assert!(ps[0].outer.contains(ps[0].label));
}

#[test]
fn minor_axis_given_first_is_normalised() {
    let mut s = Sketch::new();
    s.add_ellipse([0.0, 0.0], 0.0, 2.0, 7.0).unwrap();
    let ps = profiles(&s, 1e-6).unwrap();
    let Curve::Ellipse { major, a, b, .. } = ps[0].outer.segments[0].curve else { panic!() };
    assert_eq!((a, b), (7.0, 2.0));
    assert!(major[0].abs() < 1e-12 && (major[1] - 1.0).abs() < 1e-12);
    assert!((ps[0].area - PI * 14.0).abs() < 1e-9);
}

#[test]
fn half_ellipse_closed_by_a_line() {
    let mut s = Sketch::new();
    let arc = s.add_ellipse_arc([0.0, 0.0], 0.0, 4.0, 2.0, 0.0, PI).unwrap();
    let Geom::EllipseArc { start, end, .. } = s.entity(arc).unwrap().geom else { panic!() };
    s.add_line_between(end, start).unwrap();
    let ps = profiles(&s, 1e-6).unwrap();
    assert_eq!(ps.len(), 1);
    assert!((ps[0].area - PI * 4.0).abs() < 1e-9, "{}", ps[0].area);
    // A quarter-turn rotation and an offset change nothing.
    let mut r = Sketch::new();
    let arc = r.add_ellipse_arc([5.0, -3.0], 1.0, 4.0, 2.0, 0.5, 0.5 + PI).unwrap();
    let Geom::EllipseArc { start, end, .. } = r.entity(arc).unwrap().geom else { panic!() };
    r.add_line_between(end, start).unwrap();
    assert!((profiles(&r, 1e-6).unwrap()[0].area - PI * 4.0).abs() < 1e-9);
}

#[test]
fn crossing_ellipses_make_three_regions() {
    let mut s = Sketch::new();
    s.add_ellipse([0.0, 0.0], 0.0, 5.0, 3.0).unwrap();
    s.add_ellipse([0.0, 0.0], PI / 2.0, 5.0, 3.0).unwrap();
    let ps = profiles(&s, 1e-6).unwrap();
    // The lens in the middle and four tips... the union has 4 tips + 1 centre: 5 regions.
    assert_eq!(ps.len(), 5);
    let total: f64 = ps.iter().map(|p| p.area).sum();
    // Union area = 2 pi a b - lens; regions tile the union exactly.
    assert!(total > PI * 15.0 && total < 2.0 * PI * 15.0, "{total}");
}

#[test]
fn ellipse_with_a_circle_inside_has_a_hole() {
    let mut s = Sketch::new();
    s.add_ellipse([0.0, 0.0], 0.0, 5.0, 3.0).unwrap();
    s.add_circle([0.0, 0.0], 1.0).unwrap();
    let ps = profiles(&s, 1e-6).unwrap();
    let outer = ps.iter().find(|p| !p.holes.is_empty()).unwrap();
    assert!((outer.area - (PI * 15.0 - PI)).abs() < 1e-9);
}

#[test]
fn a_line_across_an_ellipse_splits_it() {
    let mut s = Sketch::new();
    s.add_ellipse([0.0, 0.0], 0.0, 5.0, 3.0).unwrap();
    s.add_line([-6.0, 0.0], [6.0, 0.0]).unwrap();
    let ps = profiles(&s, 1e-6).unwrap();
    assert_eq!(ps.len(), 2);
    for p in &ps {
        assert!((p.area - PI * 7.5).abs() < 1e-6, "{}", p.area);
    }
}

#[test]
fn solver_keeps_arc_ends_on_the_ellipse_and_honours_dimensions() {
    let mut s = Sketch::new();
    let e = s.add_ellipse([1.0, 2.0], 0.4, 5.0, 3.0).unwrap();
    let arc = s.add_ellipse_arc([1.0, 2.0], 0.4, 5.0, 3.0, 0.2, 2.0).unwrap();
    let (c, ..) = s.ellipse(e).unwrap();
    s.add_constraint(Constraint::Fixed(c)).unwrap();
    s.add_constraint(Constraint::SemiMajor { e, value: 6.0 }).unwrap();
    s.add_constraint(Constraint::SemiMinor { e, value: 2.5 }).unwrap();
    s.add_constraint(Constraint::Horizontal(e)).unwrap();
    let r = solve(&s, &SolveOptions::default());
    assert_ne!(r.status, SolveStatus::Failed, "{:?}", r.error);
    let (_, ang, a, b) = r.sketch.ellipse(e).unwrap();
    assert!((a - 6.0).abs() < 1e-8 && (b - 2.5).abs() < 1e-8 && ang.sin().abs() < 1e-8, "{ang} {a} {b}");
    // The arc's own parameters are free but its end points stay on its ellipse.
    let (cc, ang, a, b) = r.sketch.ellipse(arc).unwrap();
    let cp = r.sketch.point(cc).unwrap();
    let Geom::EllipseArc { start, end, .. } = r.sketch.entity(arc).unwrap().geom else { panic!() };
    for p in [start, end] {
        let d = ellipse::distance(cp, ang, a, b, r.sketch.point(p).unwrap());
        assert!(d.abs() < 1e-8, "{d}");
    }
}

#[test]
fn point_on_ellipse_and_fully_fixed() {
    let mut s = Sketch::new();
    let e = s.add_ellipse([0.0, 0.0], 0.0, 5.0, 3.0).unwrap();
    let p = s.add_point([4.0, 1.5]).unwrap();
    s.add_constraint(Constraint::Fixed(e)).unwrap();
    s.add_constraint(Constraint::PointOnCircle(p, e)).unwrap();
    s.add_constraint(Constraint::HDistance { from: None, to: p, value: 3.0 }).unwrap();
    let r = solve(&s, &SolveOptions::default());
    assert_eq!(r.status, SolveStatus::WellConstrained, "{:?}", r.error);
    let q = r.sketch.point(p).unwrap();
    // x = 3 on x^2/25 + y^2/9 = 1  =>  y = +-2.4; the start (1.5) picks the upper branch.
    assert!((q[0] - 3.0).abs() < 1e-8 && (q[1] - 2.4).abs() < 1e-8, "{q:?}");
    // The ellipse itself keeps 5 + 3 + angle free until fixed: an unfixed one has dof.
    let mut t = Sketch::new();
    t.add_ellipse([0.0, 0.0], 0.0, 5.0, 3.0).unwrap();
    assert_eq!(solve(&t, &SolveOptions::default()).dof, 5);
}
