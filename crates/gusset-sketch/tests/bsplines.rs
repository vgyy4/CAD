// Test-only code (see the note at the top of profiles.rs).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp, clippy::disallowed_methods)]

//! Exact B-spline sketch curves: regions, crossings, solving (ADR 0016 amendment).

use core::f64::consts::{FRAC_1_SQRT_2, PI};
use gusset_sketch::{Constraint, Curve, EntityId, Geom, Sketch, SketchError, SolveStatus, arrange, solve};

const TOL: f64 = 1e-6;

fn points(s: &mut Sketch, pts: &[[f64; 2]]) -> Vec<EntityId> {
    pts.iter().map(|&p| s.add_point(p).unwrap()).collect()
}

/// The full circle of radius `r` around `c` as a clamped rational quadratic: nine poles on
/// the square around it (the last is the first point again), weights 1 and √2/2.
fn circle(s: &mut Sketch, c: [f64; 2], r: f64) -> EntityId {
    let q = [[r, 0.0], [r, r], [0.0, r], [-r, r], [-r, 0.0], [-r, -r], [0.0, -r], [r, -r]];
    let mut ids = points(s, &q.map(|p| [c[0] + p[0], c[1] + p[1]]));
    ids.push(ids[0]);
    let h = FRAC_1_SQRT_2;
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
    s.add_bspline(2, &ids, &knots, &[1.0, h, 1.0, h, 1.0, h, 1.0, h, 1.0], false).unwrap()
}

#[test]
fn a_rational_circle_encloses_pi_r_squared() {
    let mut s = Sketch::new();
    let b = circle(&mut s, [3.0, -2.0], 10.0);
    let a = arrange(&s, TOL).unwrap();
    assert_eq!(a.profiles.len(), 1);
    let p = &a.profiles[0];
    assert!((p.area - PI * 100.0).abs() < 1e-9, "{}", p.area);
    // One rational quadratic per knot span, numbered by `sub`.
    assert_eq!(p.outer.segments.len(), 4);
    assert!(p.outer.segments.iter().all(|g| g.entity == b));
    for g in &p.outer.segments {
        let Curve::RationalBezier { b } = g.curve else { panic!("{:?}", g.curve) };
        assert_eq!(b.degree, 2);
        for k in 0..=10 {
            let q = b.eval(f64::from(k) / 10.0);
            assert!(((q[0] - 3.0).hypot(q[1] + 2.0) - 10.0).abs() < 1e-12);
        }
    }
    let mut subs: Vec<u32> = p.outer.segments.iter().map(|g| g.sub).collect();
    subs.sort_unstable();
    assert_eq!(subs, vec![0, 1, 2, 3]);
    assert!(p.outer.contains([3.0, -2.0]) && !p.outer.contains([14.0, -2.0]));
}

#[test]
fn a_periodic_quadratic_over_a_square_has_its_analytic_area() {
    // Uniform periodic quadratic over the square of corners (±a, ±a): four parabolic arcs
    // from edge midpoint to edge midpoint, each bulging towards a corner. Area: the diamond
    // of the midpoints (2a²) plus four parabolic segments of 2/3 · a²/2: 10a²/3.
    let a = 6.0;
    let mut s = Sketch::new();
    let ids = points(&mut s, &[[a, a], [-a, a], [-a, -a], [a, -a]]);
    let b = s.add_bspline(2, &ids, &[0.0, 1.0, 2.0, 3.0, 4.0], &[1.0; 4], true).unwrap();
    assert!(matches!(s.entity(b).unwrap().geom, Geom::BSpline { periodic: true, degree: 2, .. }));
    let ar = arrange(&s, TOL).unwrap();
    assert_eq!(ar.profiles.len(), 1);
    let p = &ar.profiles[0];
    assert!((p.area - 10.0 * a * a / 3.0).abs() < 1e-9, "{}", p.area);
    assert_eq!(p.outer.segments.len(), 4);
    // Polynomial quadratics are rational Béziers with unit weights.
    for g in &p.outer.segments {
        let Curve::RationalBezier { b } = g.curve else { panic!() };
        assert_eq!(b.control_weights(), &[1.0, 1.0, 1.0]);
    }
}

#[test]
fn a_clamped_cubic_is_cut_into_beziers_and_closes_with_a_line() {
    let mut s = Sketch::new();
    let ids = points(&mut s, &[[0.0, 0.0], [2.0, 6.0], [5.0, 8.0], [9.0, 5.0], [10.0, 0.0]]);
    let b = s.add_bspline(3, &ids, &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0], &[1.0; 5], false).unwrap();
    s.add_line_between(ids[4], ids[0]).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert_eq!(a.profiles.len(), 1);
    let p = &a.profiles[0];
    let beziers: Vec<[[f64; 2]; 4]> = p
        .outer
        .segments
        .iter()
        .filter(|g| g.entity == b)
        .map(|g| match g.curve {
            Curve::Bezier { p } => p,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(beziers.len(), 2);
    // The polygon of a fine sampling agrees with the exact area.
    let mut pts = Vec::new();
    for g in &p.outer.segments {
        match g.curve {
            Curve::Bezier { p } => {
                for k in 0..4000 {
                    let t = f64::from(k) / 4000.0;
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    pts.push([(0..4).map(|i| w[i] * p[i][0]).sum::<f64>(), (0..4).map(|i| w[i] * p[i][1]).sum()]);
                }
            }
            Curve::Line { start, .. } => pts.push(start),
            other => panic!("{other:?}"),
        }
    }
    let n = pts.len();
    let poly: f64 =
        0.5 * (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum::<f64>();
    assert!((p.area - poly.abs()).abs() < 1e-5, "{} vs {poly}", p.area);
}

#[test]
fn a_line_across_a_bspline_circle_splits_it_in_two() {
    let mut s = Sketch::new();
    circle(&mut s, [0.0, 0.0], 10.0);
    s.add_line([-15.0, 0.0], [15.0, 0.0]).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert!(a.crossings);
    assert_eq!(a.profiles.len(), 2);
    for p in &a.profiles {
        assert!((p.area - PI * 50.0).abs() < 1e-6, "{}", p.area);
    }
}

#[test]
fn two_bspline_circles_overlap_in_three_regions() {
    let mut s = Sketch::new();
    circle(&mut s, [0.0, 0.0], 10.0);
    circle(&mut s, [10.0, 0.0], 10.0);
    let a = arrange(&s, TOL).unwrap();
    assert_eq!(a.profiles.len(), 3);
    // The lens of two circles of radius r a distance r apart: (2π/3 − √3/2) r².
    let lens = (2.0 * PI / 3.0 - 3f64.sqrt() / 2.0) * 100.0;
    let total: f64 = a.profiles.iter().map(|p| p.area).sum();
    assert!(a.profiles.iter().any(|p| (p.area - lens).abs() < 1e-5), "{:?}", a.profiles.iter().map(|p| p.area));
    assert!((total - (2.0 * PI * 100.0 - lens)).abs() < 1e-5);
}

#[test]
fn the_solver_moves_poles_and_the_curve_follows() {
    let mut s = Sketch::new();
    let a = 6.0;
    let ids = points(&mut s, &[[a, a], [-a, a], [-a, -a], [a, -a]]);
    s.add_bspline(2, &ids, &[0.0, 1.0, 2.0, 3.0, 4.0], &[1.0; 4], true).unwrap();
    // Pin three corners; drive the fourth to (8, 8) in x and y.
    for &p in &ids[1..] {
        s.add_constraint(Constraint::Fixed(p)).unwrap();
    }
    s.add_constraint(Constraint::HDistance { from: None, to: ids[0], value: 8.0 }).unwrap();
    s.add_constraint(Constraint::VDistance { from: None, to: ids[0], value: 8.0 }).unwrap();
    let r = solve(&s, &Default::default());
    assert_eq!(r.status, SolveStatus::WellConstrained);
    assert_eq!(r.sketch.point(ids[0]).unwrap(), [8.0, 8.0]);
    let p = &arrange(&r.sketch, TOL).unwrap().profiles[0];
    let before = 10.0 * a * a / 3.0;
    assert!(p.area > before + 1.0, "{} vs {before}", p.area);
}

#[test]
fn bad_data_is_refused_with_a_reason() {
    let mut s = Sketch::new();
    let ids = points(&mut s, &[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]);
    let e = s.add_bspline(3, &ids, &[0.0; 7], &[1.0; 3], false).unwrap_err();
    assert!(matches!(&e, SketchError::BadBSpline(m) if m.contains("at least 4 poles")), "{e}");
    let e = s.add_bspline(2, &ids, &[0.0, 0.0, 0.0, 1.0, 1.0], &[1.0; 3], false).unwrap_err();
    assert!(e.to_string().contains("6 knots"), "{e}");
    let e = s.add_bspline(2, &ids, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[1.0, -1.0, 1.0], false).unwrap_err();
    assert!(e.to_string().contains("weights"), "{e}");
    let line = s.add_line([5.0, 5.0], [6.0, 6.0]).unwrap();
    let e = s.add_bspline(2, &[ids[0], line, ids[2]], &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[1.0; 3], false).unwrap_err();
    assert!(matches!(e, SketchError::WrongKind { .. }), "{e}");
    // A degenerate curve (all poles at one place) is no region.
    let same = points(&mut s, &[[3.0, 3.0], [3.0, 3.0], [3.0, 3.0]]);
    let mut t = Sketch::new();
    let same2 = points(&mut t, &[[3.0, 3.0], [3.0, 3.0], [3.0, 3.0]]);
    let b = t.add_bspline(2, &same2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &[1.0; 3], false).unwrap();
    assert!(arrange(&t, TOL).is_err(), "{b}");
    let _ = same;
}
