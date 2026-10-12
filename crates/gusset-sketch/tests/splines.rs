// Test-only code (see the note at the top of profiles.rs).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

mod common;

use common::rect;
use core::f64::consts::{PI, TAU};
use gusset_sketch::{Curve, EntityId, Geom, ProfileError, Sketch, arrange};

const TOL: f64 = 1e-6;

fn points(s: &mut Sketch, pts: &[[f64; 2]]) -> Vec<EntityId> {
    pts.iter().map(|&p| s.add_point(p).unwrap()).collect()
}

fn ring(n: u32, r: f64, c: [f64; 2]) -> Vec<[f64; 2]> {
    (0..n)
        .map(|k| {
            let t = TAU * f64::from(k) / f64::from(n);
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect()
}

/// Area of a spline loop by a fine polyline of its Bézier segments.
fn polyline_area(segs: &[[[f64; 2]; 4]]) -> f64 {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for seg in segs {
        for k in 0..2000 {
            let t = f64::from(k) / 2000.0;
            let s = 1.0 - t;
            let w = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
            pts.push([
                w[0] * seg[0][0] + w[1] * seg[1][0] + w[2] * seg[2][0] + w[3] * seg[3][0],
                w[0] * seg[0][1] + w[1] * seg[1][1] + w[2] * seg[2][1] + w[3] * seg[3][1],
            ]);
        }
    }
    let n = pts.len();
    0.5 * (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum::<f64>()
}

#[test]
fn a_closed_spline_is_a_region() {
    let mut s = Sketch::new();
    let ids = points(&mut s, &ring(12, 10.0, [0.0, 0.0]));
    let sp = s.add_spline(&ids, true).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert!(!a.crossings);
    assert_eq!(a.profiles.len(), 1);
    let p = &a.profiles[0];
    // One Bézier segment per fit point, all from the spline, numbered by `sub`.
    assert_eq!(p.outer.segments.len(), 12);
    assert!(p.outer.segments.iter().all(|s| s.entity == sp));
    let mut subs: Vec<u32> = p.outer.segments.iter().map(|s| s.sub).collect();
    subs.sort_unstable();
    assert_eq!(subs, (0..12).collect::<Vec<u32>>());
    let segs: Vec<[[f64; 2]; 4]> = p
        .outer
        .segments
        .iter()
        .map(|s| match s.curve {
            Curve::Bezier { p } => p,
            other => panic!("{other:?}"),
        })
        .collect();
    for (i, b) in segs.iter().enumerate() {
        let next = segs[(i + 1) % segs.len()];
        assert!((b[3][0] - next[0][0]).abs() < 1e-12 && (b[3][1] - next[0][1]).abs() < 1e-12);
    }
    // The exact area agrees with a high-resolution polygon, and is close to a circle's.
    let want = polyline_area(&segs);
    assert!((p.area - want).abs() < 1e-5 * want, "{} vs {want}", p.area);
    assert!((p.area - PI * 100.0).abs() < 0.003 * PI * 100.0, "{}", p.area);
    assert!(p.label[0].hypot(p.label[1]) < 10.0);
}

#[test]
fn clockwise_fit_points_still_give_a_counter_clockwise_region() {
    let mut s = Sketch::new();
    let mut pts = ring(6, 5.0, [3.0, 4.0]);
    pts.reverse();
    let ids = points(&mut s, &pts);
    s.add_spline(&ids, true).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert_eq!(a.profiles.len(), 1);
    assert!(a.profiles[0].area > 0.0 && a.profiles[0].outer.signed_area() > 0.0);
}

#[test]
fn a_line_across_a_closed_spline_splits_it() {
    let mut s = Sketch::new();
    let ids = points(&mut s, &ring(8, 10.0, [0.0, 0.0]));
    s.add_spline(&ids, true).unwrap();
    let whole = arrange(&s, TOL).unwrap().profiles[0].area;
    s.add_line([-20.0, 1.0], [20.0, 1.0]).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert!(a.crossings);
    assert_eq!(a.profiles.len(), 2);
    let total: f64 = a.profiles.iter().map(|p| p.area).sum();
    assert!((total - whole).abs() < 1e-5 * whole, "{total} vs {whole}");
    assert!(a.profiles.iter().all(|p| p.area > 0.0));
    for p in &a.profiles {
        // One line plus several Bézier segments, with no gap anywhere.
        assert!(p.outer.segments.iter().any(|s| matches!(s.curve, Curve::Line { .. })));
        assert!(p.outer.segments.iter().any(|s| matches!(s.curve, Curve::Bezier { .. })));
        let l = &p.outer;
        for (i, seg) in l.segments.iter().enumerate() {
            let next = &l.segments[(i + 1) % l.segments.len()];
            let end = match seg.curve {
                Curve::Bezier { p } => p[3],
                Curve::Line { end, .. } => end,
                other => panic!("{other:?}"),
            };
            let start = match next.curve {
                Curve::Bezier { p } => p[0],
                Curve::Line { start, .. } => start,
                other => panic!("{other:?}"),
            };
            assert!((end[0] - start[0]).abs() < 1e-12 && (end[1] - start[1]).abs() < 1e-12);
        }
    }
}

#[test]
fn an_open_spline_across_a_rectangle_divides_it() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 40.0, 20.0);
    let ids = points(&mut s, &[[-10.0, 10.0], [10.0, 5.0], [25.0, 15.0], [50.0, 10.0]]);
    let sp = s.add_spline(&ids, false).unwrap();
    let a = arrange(&s, TOL).unwrap();
    let total: f64 = a.profiles.iter().map(|p| p.area).sum();
    assert_eq!(a.profiles.len(), 2, "{:?}", a.profiles.iter().map(|p| p.area).collect::<Vec<_>>());
    assert!((total - 800.0).abs() < 1e-6, "{total}");
    assert!(a.open_ends.contains(&sp));
    // The middle piece (number 1) is shared by both regions.
    assert!(a.profiles.iter().all(|p| p.pieces.contains(&(sp, 1))));
}

#[test]
fn two_splines_and_a_circle_cross() {
    let mut s = Sketch::new();
    let a_ids = points(&mut s, &[[0.0, 0.0], [10.0, 8.0], [20.0, 0.0], [30.0, 8.0]]);
    s.add_spline(&a_ids, false).unwrap();
    let b_ids = points(&mut s, &[[0.0, 8.0], [10.0, 0.0], [20.0, 8.0], [30.0, 0.0]]);
    s.add_spline(&b_ids, false).unwrap();
    let arr = arrange(&s, TOL).unwrap();
    // Two snaking curves cross three times: two closed lobes between the first and last crossing.
    assert_eq!(arr.profiles.len(), 2, "{:?}", arr.profiles.iter().map(|p| p.area).collect::<Vec<_>>());
    assert!(arr.crossings);
    let mut s2 = s.clone();
    s2.add_circle([15.0, 4.0], 5.0).unwrap();
    let arr2 = arrange(&s2, TOL).unwrap();
    assert!(arr2.profiles.len() > 2);
}

#[test]
fn a_spline_loop_that_crosses_itself_is_an_error() {
    let mut s = Sketch::new();
    // A figure of eight through four points.
    let ids = points(&mut s, &[[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]]);
    let sp = s.add_spline(&ids, true).unwrap();
    match arrange(&s, TOL) {
        Err(ProfileError::SelfIntersecting { a, b, .. }) => assert_eq!((a, b), (sp, sp)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_spline_inside_a_rectangle_is_a_hole() {
    let mut s = Sketch::new();
    rect(&mut s, -20.0, -20.0, 40.0, 40.0);
    let ids = points(&mut s, &ring(6, 5.0, [0.0, 0.0]));
    s.add_spline(&ids, true).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert_eq!(a.profiles.len(), 2);
    let plate = a.profiles.iter().find(|p| p.area > 1000.0).unwrap();
    assert_eq!(plate.holes.len(), 1);
    assert!(matches!(plate.holes[0].segments[0].curve, Curve::Bezier { .. }));
    assert!(plate.holes[0].signed_area() < 0.0);
}

#[test]
fn moving_fit_points_keeps_the_piece_numbers() {
    let build = |wobble: f64| {
        let mut s = Sketch::new();
        let mut pts = ring(8, 10.0, [0.0, 0.0]);
        pts[1][0] += wobble;
        pts[5][1] -= wobble;
        let ids = points(&mut s, &pts);
        s.add_spline(&ids, true).unwrap();
        s.add_line([-20.0, 1.0], [20.0, 1.5]).unwrap();
        let a = arrange(&s, TOL).unwrap();
        let mut v: Vec<(bool, Vec<(EntityId, u32)>)> =
            a.profiles.iter().map(|p| (p.label[1] > 0.5 + 0.0 * p.label[0], p.pieces.clone())).collect();
        v.sort_by_key(|x| x.0);
        v
    };
    assert_eq!(build(0.0), build(0.8));
    assert_eq!(build(0.0), build(-0.6));
}

#[test]
fn bad_splines_are_rejected_cleanly() {
    let mut s = Sketch::new();
    let ids = points(&mut s, &[[0.0, 0.0], [1.0, 0.0]]);
    assert!(s.add_spline(&ids, false).is_err(), "two points make a line, not a spline");
    let ids = points(&mut s, &[[0.0, 0.0], [5.0, 5.0], [5.0, 5.0]]);
    // The same point twice in a row is refused; two distinct points at one place are degenerate.
    assert!(s.add_spline(&[ids[0], ids[1], ids[1]], false).is_err());
    let p = s.add_spline(&ids, false).unwrap();
    assert!(matches!(arrange(&s, TOL), Err(ProfileError::Degenerate { entity }) if entity == p));
    let line = s.add_line([0.0, 0.0], [1.0, 1.0]).unwrap();
    assert!(s.add_spline(&[line, ids[0], ids[1]], false).is_err());
    // Deleting a fit point deletes the spline.
    let removed = s.remove_entity(ids[0]).unwrap();
    assert!(removed.contains(&p));
    assert!(s.entity(p).is_none());
    // A spline is not a valid argument of line constraints.
    let ids2 = points(&mut s, &ring(4, 3.0, [0.0, 0.0]));
    let sp2 = s.add_spline(&ids2, true).unwrap();
    assert!(s.add_constraint(gusset_sketch::Constraint::Horizontal(sp2)).is_err());
    assert!(matches!(s.entity(sp2).unwrap().geom, Geom::Spline { closed: true, .. }));
}
