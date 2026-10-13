// Test-only code (see the note at the top of profiles.rs).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp, clippy::disallowed_methods)]

mod common;

use common::{Rng, rect};
use core::f64::consts::PI;
use gusset_sketch::{Arrangement, EntityId, ProfileError, Sketch, arrange};

const TOL: f64 = 1e-6;

fn areas(a: &Arrangement) -> Vec<f64> {
    let mut v: Vec<f64> = a.profiles.iter().map(|p| p.area).collect();
    v.sort_by(f64::total_cmp);
    v
}

fn assert_areas(a: &Arrangement, want: &[f64]) {
    let got = areas(a);
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-6 * (1.0 + w.abs()), "{got:?} vs {want:?}");
    }
}

#[test]
fn two_overlapping_rectangles_make_three_regions() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 20.0, 10.0);
    rect(&mut s, 10.0, 5.0, 20.0, 10.0);
    let a = arrange(&s, TOL).unwrap();
    assert!(a.crossings);
    assert!(a.open_ends.is_empty());
    // A-only (L-shape 200-50), the 10x5 overlap, B-only (200-50).
    assert_areas(&a, &[50.0, 150.0, 150.0]);
    for p in &a.profiles {
        assert!(p.holes.is_empty());
        assert!(p.area > 0.0);
    }
}

#[test]
fn region_pieces_are_stable_under_parameter_edits() {
    let build = |dx: f64, dy: f64, scale: f64| {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 20.0 * scale, 10.0 * scale);
        rect(&mut s, 10.0 + dx, 5.0 + dy, 20.0, 10.0);
        arrange(&s, TOL).unwrap()
    };
    let base = build(0.0, 0.0, 1.0);
    let ids = |a: &Arrangement| a.profiles.iter().map(|p| p.pieces.clone()).collect::<Vec<_>>();
    for (dx, dy, k) in [(1.0, 0.5, 1.0), (-2.0, 1.0, 1.1), (3.0, -1.0, 0.9)] {
        let moved = build(dx, dy, k);
        assert_eq!(ids(&base), ids(&moved), "dx={dx} dy={dy} k={k}");
    }
    // All the regions have distinct piece lists.
    let mut v = ids(&base);
    v.sort();
    v.dedup();
    assert_eq!(v.len(), 3);
}

#[test]
fn a_line_across_a_circle_gives_two_half_discs_and_ignores_the_stubs() {
    let mut s = Sketch::new();
    let l = s.add_line([-20.0, 0.0], [20.0, 0.0]).unwrap();
    let c = s.add_circle([0.0, 0.0], 10.0).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert_areas(&a, &[50.0 * PI, 50.0 * PI]);
    assert_eq!(a.open_ends, vec![l]);
    // Each half uses the diameter piece 1 of the line (the middle one) and one arc piece.
    for p in &a.profiles {
        assert_eq!(p.pieces.len(), 2);
        assert!(p.pieces.contains(&(l, 1)));
        assert!(p.pieces.iter().any(|&(e, _)| e == c));
        assert!(p.label[1].abs() > 0.1);
    }
    // The two halves have different pieces, so different ids.
    assert_ne!(a.profiles[0].pieces, a.profiles[1].pieces);
}

#[test]
fn circle_piece_numbers_survive_moving_and_rotating_the_line() {
    let build = |angle: f64, shift: f64| {
        let mut s = Sketch::new();
        let (sn, cs) = angle.sin_cos();
        let _l = s.add_line([-20.0 * cs + shift, -20.0 * sn], [20.0 * cs + shift, 20.0 * sn]).unwrap();
        let _c = s.add_circle([0.0, 0.0], 10.0).unwrap();
        let a = arrange(&s, TOL).unwrap();
        // Which half (by the side of its label point) carries which pieces.
        a.profiles
            .iter()
            .map(|p| {
                let left = -sn * p.label[0] + cs * p.label[1] > 0.0;
                (left, p.pieces.clone())
            })
            .collect::<Vec<_>>()
    };
    let mut base = build(0.0, 0.0);
    base.sort_by_key(|x| x.0);
    for (angle, shift) in [(0.3, 0.5), (1.2, -1.0), (2.9, 0.0), (-2.0, 2.0), (PI, 0.0)] {
        let mut v = build(angle, shift);
        v.sort_by_key(|x| x.0);
        assert_eq!(base, v, "angle {angle} shift {shift}");
    }
}

#[test]
fn t_junction_and_dividing_line() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 20.0, 10.0);
    // A divider whose ends lie on the top and bottom edges (not at corners).
    let top = s.add_point([10.0, 10.0]).unwrap();
    let bottom = s.add_point([10.0, 0.0]).unwrap();
    s.add_line_between(bottom, top).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert!(a.crossings);
    assert_areas(&a, &[100.0, 100.0]);
    assert!(a.open_ends.is_empty());
}

#[test]
fn bridges_and_dangling_stubs_bound_nothing() {
    let mut s = Sketch::new();
    rect(&mut s, 0.0, 0.0, 40.0, 40.0);
    s.add_circle([20.0, 20.0], 5.0).unwrap();
    // A bridge from the left edge to the circle's leftmost point, and a stub in the plate.
    s.add_line([0.0, 20.0], [15.0, 20.0]).unwrap();
    let stub = s.add_line([30.0, 30.0], [35.0, 35.0]).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert_areas(&a, &[25.0 * PI, 1600.0 - 25.0 * PI]);
    let plate = a.profiles.iter().find(|p| p.area > 1000.0).unwrap();
    assert_eq!(plate.holes.len(), 1, "the circle is a hole of the plate");
    assert_eq!(a.open_ends, vec![stub]);
}

#[test]
fn two_circles_make_a_lens_and_two_lunes() {
    let mut s = Sketch::new();
    s.add_circle([0.0, 0.0], 10.0).unwrap();
    s.add_circle([10.0, 0.0], 10.0).unwrap();
    let a = arrange(&s, TOL).unwrap();
    assert_eq!(a.profiles.len(), 3);
    let lens = 200.0 * (PI / 3.0) - 5.0 * 300.0_f64.sqrt();
    let total: f64 = a.profiles.iter().map(|p| p.area).sum();
    assert!((total - (2.0 * 100.0 * PI - lens)).abs() < 1e-6, "{total}");
    let smallest = areas(&a)[0];
    assert!((smallest - lens).abs() < 1e-6, "{smallest} vs {lens}");
}

#[test]
fn overlapping_collinear_lines_are_an_error_with_a_hint_entity() {
    let mut s = Sketch::new();
    let a = s.add_line([0.0, 0.0], [10.0, 0.0]).unwrap();
    let b = s.add_line([5.0, 0.0], [15.0, 0.0]).unwrap();
    match arrange(&s, TOL) {
        Err(ProfileError::SelfIntersecting { a: x, b: y, .. }) => assert_eq!((x, y), (a, b)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn open_geometry_without_a_region_is_still_open() {
    let mut s = Sketch::new();
    let a = s.add_line([0.0, 0.0], [10.0, 0.0]).unwrap();
    let b = s.add_line([5.0, -5.0], [5.0, 5.0]).unwrap();
    let r = arrange(&s, TOL).unwrap();
    assert!(r.profiles.is_empty());
    assert_eq!(r.open_ends, vec![a, b]);
    assert!(matches!(gusset_sketch::profiles(&s, TOL), Err(ProfileError::Open { .. })));
}

/// Exact area of the union of axis-aligned rectangles by coordinate compression.
fn union_area(rects: &[[f64; 4]]) -> f64 {
    let mut xs: Vec<f64> = rects.iter().flat_map(|r| [r[0], r[2]]).collect();
    let mut ys: Vec<f64> = rects.iter().flat_map(|r| [r[1], r[3]]).collect();
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    let mut total = 0.0;
    for wx in xs.windows(2) {
        for wy in ys.windows(2) {
            let (cx, cy) = (0.5 * (wx[0] + wx[1]), 0.5 * (wy[0] + wy[1]));
            if rects.iter().any(|r| cx > r[0] && cx < r[2] && cy > r[1] && cy < r[3]) {
                total += (wx[1] - wx[0]) * (wy[1] - wy[0]);
            }
        }
    }
    total
}

#[test]
fn random_rectangles_partition_their_union() {
    let mut rng = Rng(0xA11CE);
    for round in 0..40 {
        let n = 2 + (round % 5);
        let mut rects = Vec::new();
        let mut s = Sketch::new();
        for _ in 0..n {
            // Distinct coordinates (random reals), so no edges overlap or pass through corners.
            let (x, y) = (rng.range(0.0, 50.0), rng.range(0.0, 50.0));
            let (w, h) = (rng.range(5.0, 30.0), rng.range(5.0, 30.0));
            rect(&mut s, x, y, w, h);
            rects.push([x, y, x + w, y + h]);
        }
        let a = arrange(&s, TOL).unwrap();
        assert!(a.open_ends.is_empty(), "round {round}");
        // Regions with holes count their own area minus the holes; all faces together tile the union.
        let total: f64 = a.profiles.iter().map(|p| p.area).sum();
        let want = union_area(&rects);
        assert!((total - want).abs() < 1e-6 * want, "round {round}: {total} vs {want}");
        for p in &a.profiles {
            assert!(p.area > 0.0);
            let inside = rects
                .iter()
                .filter(|r| p.label[0] > r[0] && p.label[0] < r[2] && p.label[1] > r[1] && p.label[1] < r[3]);
            assert!(inside.count() >= 1, "label of a region lies in the union");
        }
    }
}

#[test]
fn a_big_grid_of_crossings_stays_fast_and_exact() {
    let mut s = Sketch::new();
    let n = 30;
    // A frame plus n horizontal and n vertical full-width lines: (n+1)^2 cells.
    rect(&mut s, 0.0, 0.0, 100.0, 100.0);
    for i in 1..=n {
        let t = 100.0 * f64::from(i) / f64::from(n + 1) + 0.01 * f64::from(i % 7);
        s.add_line([0.0, t], [100.0, t]).unwrap();
        let u = 100.0 * f64::from(i) / f64::from(n + 1) + 0.013 * f64::from(i % 5);
        s.add_line([u, 0.0], [u, 100.0]).unwrap();
    }
    let t0 = std::time::Instant::now();
    let a = arrange(&s, TOL).unwrap();
    assert_eq!(a.profiles.len(), (n as usize + 1) * (n as usize + 1));
    let total: f64 = a.profiles.iter().map(|p| p.area).sum();
    assert!((total - 10_000.0).abs() < 1e-6, "{total}");
    assert!(t0.elapsed().as_secs() < 5, "{:?}", t0.elapsed());
    let _: Vec<EntityId> = a.open_ends;
}

#[test]
fn random_mixed_curves_never_panic_and_give_closed_loops() {
    use gusset_sketch::Curve;
    let mut rng = Rng(0xBEEF);
    let mut ok = 0;
    for round in 0..300 {
        let mut s = Sketch::new();
        for _ in 0..(3 + round % 9) {
            match (rng.next_u64() % 3) as u8 {
                0 => {
                    let (a, b) =
                        ([rng.range(0.0, 40.0), rng.range(0.0, 40.0)], [rng.range(0.0, 40.0), rng.range(0.0, 40.0)]);
                    s.add_line(a, b).ok();
                }
                1 => {
                    s.add_circle([rng.range(0.0, 40.0), rng.range(0.0, 40.0)], rng.range(2.0, 15.0)).ok();
                }
                _ => {
                    let c = [rng.range(0.0, 40.0), rng.range(0.0, 40.0)];
                    let (a, b) =
                        ([rng.range(0.0, 40.0), rng.range(0.0, 40.0)], [rng.range(0.0, 40.0), rng.range(0.0, 40.0)]);
                    s.add_arc(c, a, b).ok();
                }
            }
        }
        let Ok(a) = arrange(&s, TOL) else { continue };
        ok += 1;
        for p in &a.profiles {
            assert!(p.area > 0.0, "round {round}");
            for l in std::iter::once(&p.outer).chain(&p.holes) {
                let n = l.segments.len();
                for i in 0..n {
                    let end = |c: &Curve| match *c {
                        Curve::Line { end, .. } | Curve::Arc { end, .. } => Some(end),
                        Curve::Circle { .. } | Curve::Bezier { .. } => None,
                    };
                    let start = |c: &Curve| match *c {
                        Curve::Line { start, .. } | Curve::Arc { start, .. } => Some(start),
                        Curve::Circle { .. } | Curve::Bezier { .. } => None,
                    };
                    if let (Some(e), Some(s2)) = (end(&l.segments[i].curve), start(&l.segments[(i + 1) % n].curve)) {
                        assert!((e[0] - s2[0]).abs() < 1e-9 && (e[1] - s2[1]).abs() < 1e-9, "round {round}: gap");
                    }
                }
            }
        }
    }
    assert!(ok > 200, "{ok} arrangements succeeded");
}
