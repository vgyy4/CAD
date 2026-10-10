// Integration-test helpers live outside `#[test]` functions, where clippy.toml's
// `allow-*-in-tests` settings do not reach; this file is test-only code.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

mod common;

use common::rect;
use core::f64::consts::PI;
use gusset_sketch::{Curve, EntityId, Profile, ProfileError, Sketch, profiles};

const TOL: f64 = 1e-6;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

/// Every loop is closed: each segment starts where the previous one ended.
fn assert_closed(p: &Profile) {
    let ends = |c: &Curve| match *c {
        Curve::Line { start, end } | Curve::Arc { start, end, .. } => Some((start, end)),
        Curve::Circle { .. } => None,
    };
    for l in std::iter::once(&p.outer).chain(&p.holes) {
        let n = l.segments.len();
        for i in 0..n {
            if let (Some((_, e)), Some((s, _))) = (ends(&l.segments[i].curve), ends(&l.segments[(i + 1) % n].curve)) {
                assert!((e[0] - s[0]).abs() < 1e-12 && (e[1] - s[1]).abs() < 1e-12, "gap in {l:?}");
            }
        }
    }
    assert!(p.outer.signed_area() > 0.0, "outer loops are counter-clockwise");
    for h in &p.holes {
        assert!(h.signed_area() < 0.0, "holes are clockwise");
    }
}

#[test]
fn rectangle() {
    let mut s = Sketch::new();
    let r = rect(&mut s, 5.0, 5.0, 40.0, 20.0);
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 1);
    assert!(approx(ps[0].area, 800.0));
    assert!(ps[0].holes.is_empty());
    assert_eq!(ps[0].outer.entities(), r.l.to_vec());
    assert!(ps[0].outer.segments.iter().all(|s| !s.reversed));
    assert_closed(&ps[0]);
}

#[test]
fn rectangle_drawn_clockwise_is_reoriented() {
    let mut s = Sketch::new();
    let p: Vec<EntityId> =
        [[0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]].iter().map(|&q| s.add_point(q).unwrap()).collect();
    for i in 0..4 {
        s.add_line_between(p[i], p[(i + 1) % 4]).unwrap();
    }
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 1);
    assert!(approx(ps[0].area, 100.0));
    assert!(ps[0].outer.segments.iter().all(|s| s.reversed));
    assert_closed(&ps[0]);
}

#[test]
fn rectangle_with_circular_hole() {
    let mut s = Sketch::new();
    let r = rect(&mut s, 0.0, 0.0, 40.0, 20.0);
    let hole = s.add_circle([20.0, 10.0], 5.0).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 2);
    // Ordered by the first entity of the outer loop: the rectangle's lines come first.
    assert_eq!(ps[0].outer.entities(), r.l.to_vec());
    assert_eq!(ps[0].holes.len(), 1);
    assert_eq!(ps[0].holes[0].entities(), vec![hole]);
    assert!(approx(ps[0].area, 800.0 - 25.0 * PI));
    assert_eq!(ps[1].outer.entities(), vec![hole]);
    assert!(approx(ps[1].area, 25.0 * PI));
    ps.iter().for_each(assert_closed);
}

/// A slot of two lines and two half-circle arcs.
fn slot_sketch() -> (Sketch, [EntityId; 4]) {
    let mut s = Sketch::new();
    let c1 = s.add_point([40.0, 0.0]).unwrap();
    let c2 = s.add_point([0.0, 0.0]).unwrap();
    let pa = s.add_point([0.0, -10.0]).unwrap();
    let pb = s.add_point([40.0, -10.0]).unwrap();
    let pc = s.add_point([40.0, 10.0]).unwrap();
    let pd = s.add_point([0.0, 10.0]).unwrap();
    let right = s.add_arc_between(c1, pb, pc).unwrap();
    let left = s.add_arc_between(c2, pd, pa).unwrap();
    let bottom = s.add_line_between(pa, pb).unwrap();
    let top = s.add_line_between(pc, pd).unwrap();
    (s, [right, left, bottom, top])
}

#[test]
fn slot_of_lines_and_arcs() {
    let (s, [right, left, bottom, top]) = slot_sketch();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 1);
    assert!(approx(ps[0].area, 800.0 + 100.0 * PI));
    assert_eq!(ps[0].outer.entities(), vec![right, top, left, bottom]);
    assert!(matches!(ps[0].outer.segments[0].curve, Curve::Arc { ccw: true, .. }));
    assert_closed(&ps[0]);
}

#[test]
fn two_disjoint_circles() {
    let mut s = Sketch::new();
    let a = s.add_circle([0.0, 0.0], 3.0).unwrap();
    let b = s.add_circle([10.0, 0.0], 4.0).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].outer.entities(), vec![a]);
    assert_eq!(ps[1].outer.entities(), vec![b]);
    assert!(approx(ps[0].area, 9.0 * PI) && approx(ps[1].area, 16.0 * PI));
    assert!(ps.iter().all(|p| p.holes.is_empty()));
}

#[test]
fn three_nested_levels() {
    let mut s = Sketch::new();
    let outer = rect(&mut s, 0.0, 0.0, 100.0, 100.0);
    let middle = rect(&mut s, 20.0, 20.0, 60.0, 60.0);
    let inner = s.add_circle([50.0, 50.0], 10.0).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 3);
    assert_eq!(ps[0].outer.entities(), outer.l.to_vec());
    assert_eq!(ps[0].holes.len(), 1);
    assert_eq!(ps[0].holes[0].segments.len(), 4);
    assert!(ps[0].holes[0].entities().iter().all(|e| middle.l.contains(e)));
    assert!(approx(ps[0].area, 10000.0 - 3600.0));
    assert_eq!(ps[1].outer.entities(), middle.l.to_vec());
    assert_eq!(ps[1].holes[0].entities(), vec![inner]);
    assert!(approx(ps[1].area, 3600.0 - 100.0 * PI));
    assert_eq!(ps[2].outer.entities(), vec![inner]);
    assert!(approx(ps[2].area, 100.0 * PI));
    ps.iter().for_each(assert_closed);
}

#[test]
fn open_polyline_is_an_error() {
    let mut s = Sketch::new();
    let p: Vec<EntityId> =
        [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]].iter().map(|&q| s.add_point(q).unwrap()).collect();
    let l: Vec<EntityId> = p.windows(2).map(|w| s.add_line_between(w[0], w[1]).unwrap()).collect();
    assert_eq!(profiles(&s, TOL), Err(ProfileError::Open { entities: vec![l[0], l[2]] }));
}

#[test]
fn crossing_lines_are_an_error() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    // A closed triangle poking into the square, so the error is the crossing.
    let t: Vec<EntityId> = [[-5.0, 5.0], [5.0, 5.0], [5.0, -5.0]].iter().map(|&q| s.add_point(q).unwrap()).collect();
    for i in 0..3 {
        s.add_line_between(t[i], t[(i + 1) % 3]).unwrap();
    }
    match profiles(&s, TOL) {
        Err(ProfileError::SelfIntersecting { a: x, b: y, .. }) => assert!(x < y),
        other => panic!("{other:?}"),
    }
}

#[test]
fn circle_touching_a_line_is_an_error() {
    let mut s = Sketch::new();
    let r = rect(&mut s, 0.0, 0.0, 40.0, 40.0);
    let c = s.add_circle([20.0, 10.0], 10.0).unwrap();
    assert!(matches!(
        profiles(&s, TOL),
        Err(ProfileError::SelfIntersecting { a, b, at }) if a == r.l[0] && b == c && (at[0] - 20.0).abs() < 1e-3
    ));
}

#[test]
fn construction_geometry_is_ignored() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 10.0, 10.0);
    let diag = s.add_line([0.0, 0.0], [10.0, 10.0]).unwrap();
    s.set_construction(diag, true).unwrap();
    let dangling = s.add_line([20.0, 0.0], [30.0, 0.0]).unwrap();
    s.set_construction(dangling, true).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 1);
    assert!(approx(ps[0].area, 100.0));
}

#[test]
fn a_dividing_line_splits_the_region() {
    let mut s = Sketch::new();
    let r = rect(&mut s, 0.0, 0.0, 20.0, 10.0);
    // Split the bottom and top edges at x = 10 by drawing them in two pieces.
    s.remove_entity(r.l[0]).unwrap();
    s.remove_entity(r.l[2]).unwrap();
    let mb = s.add_point([10.0, 0.0]).unwrap();
    let mt = s.add_point([10.0, 10.0]).unwrap();
    for (a, b) in [(r.p[0], mb), (mb, r.p[1]), (r.p[2], mt), (mt, r.p[3]), (mb, mt)] {
        s.add_line_between(a, b).unwrap();
    }
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 2);
    assert!(ps.iter().all(|p| approx(p.area, 100.0) && p.outer.segments.len() == 4));
    ps.iter().for_each(assert_closed);
}

#[test]
fn endpoints_within_tolerance_are_joined() {
    let mut s = Sketch::new();
    let a = s.add_line([0.0, 0.0], [10.0, 0.0]).unwrap();
    let b = s.add_line([10.0 + 1e-8, 0.0], [10.0, 10.0]).unwrap();
    let c = s.add_line([10.0, 10.0 - 1e-8], [0.0, 0.0 + 1e-8]).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].outer.entities(), vec![a, b, c]);
    assert!((ps[0].area - 50.0).abs() < 1e-6);
    assert_closed(&ps[0]);
    // With a tighter tolerance the gaps are open ends.
    assert!(matches!(profiles(&s, 1e-9), Err(ProfileError::Open { .. })));
}

#[test]
fn degenerate_and_bad_inputs_are_structured_errors() {
    let mut s = Sketch::new();
    let l = s.add_line([1.0, 1.0], [1.0, 1.0]).unwrap();
    assert_eq!(profiles(&s, TOL), Err(ProfileError::Degenerate { entity: l }));
    assert_eq!(profiles(&Sketch::new(), f64::NAN), Err(ProfileError::BadTolerance));
    assert_eq!(profiles(&Sketch::new(), 0.0), Err(ProfileError::BadTolerance));
    assert_eq!(profiles(&Sketch::new(), TOL), Ok(vec![]));
    // Two identical lines overlap.
    let mut s = Sketch::new();
    let p = s.add_point([0.0, 0.0]).unwrap();
    let q = s.add_point([5.0, 0.0]).unwrap();
    s.add_line_between(p, q).unwrap();
    s.add_line_between(q, p).unwrap();
    assert!(matches!(profiles(&s, TOL), Err(ProfileError::SelfIntersecting { .. })));
}

#[test]
fn lens_of_two_arcs_and_d_shape() {
    // Two arcs sharing both endpoints (a lens) and a half-disc closed by a line.
    let mut s = Sketch::new();
    let a = s.add_point([0.0, 0.0]).unwrap();
    let b = s.add_point([10.0, 0.0]).unwrap();
    let c_up = s.add_point([5.0, -5.0]).unwrap();
    let c_dn = s.add_point([5.0, 5.0]).unwrap();
    s.add_arc_between(c_up, b, a).unwrap();
    s.add_arc_between(c_dn, a, b).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 1);
    let r2 = 50.0;
    let lens = 2.0 * r2 * (PI / 2.0 - 1.0) / 2.0;
    assert!((ps[0].area - lens).abs() < 1e-9, "{}", ps[0].area);
    ps.iter().for_each(assert_closed);

    // The complementary (major) arcs bound the union of the two discs instead.
    let mut s = Sketch::new();
    let a = s.add_point([0.0, 0.0]).unwrap();
    let b = s.add_point([10.0, 0.0]).unwrap();
    let c_up = s.add_point([5.0, -5.0]).unwrap();
    let c_dn = s.add_point([5.0, 5.0]).unwrap();
    s.add_arc_between(c_up, a, b).unwrap();
    s.add_arc_between(c_dn, b, a).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert_eq!(ps.len(), 1);
    assert!((ps[0].area - (2.0 * PI * r2 - lens)).abs() < 1e-9, "{}", ps[0].area);

    let mut s = Sketch::new();
    let arc = s.add_arc([0.0, 0.0], [5.0, 0.0], [-5.0, 0.0]).unwrap();
    let gusset_sketch::Geom::Arc { start, end, .. } = s.entity(arc).unwrap().geom else { panic!() };
    s.add_line_between(end, start).unwrap();
    let ps = profiles(&s, TOL).unwrap();
    assert!(approx(ps[0].area, 12.5 * PI));
}

#[test]
fn output_is_deterministic() {
    let build = || {
        let mut s = Sketch::new();
        for i in 0..20 {
            let x = (i % 5) as f64 * 30.0;
            let y = (i / 5) as f64 * 30.0;
            rect(&mut s, x, y, 20.0, 20.0);
            s.add_circle([x + 10.0, y + 10.0], 3.0 + i as f64 * 0.1).unwrap();
        }
        profiles(&s, TOL).unwrap()
    };
    let a = build();
    assert_eq!(a.len(), 40);
    assert_eq!(a, build());
}
