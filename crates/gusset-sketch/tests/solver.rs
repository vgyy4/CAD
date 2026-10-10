// Integration-test helpers live outside `#[test]` functions, where clippy.toml's
// `allow-*-in-tests` settings do not reach; this file is test-only code.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

mod common;

use common::{Rect, Rng, close, rect};
use core::f64::consts::PI;
use gusset_sketch::{
    Constraint as C, Drag, EntityId, Geom, Sketch, SolveError, SolveOptions, SolveResult, SolveStatus, solve,
    solve_drag,
};
use std::time::{Duration, Instant};

fn opts() -> SolveOptions {
    SolveOptions::default()
}

/// A rectangle drawn slightly skewed, with the usual sketch constraints.
fn skewed_rect(s: &mut Sketch) -> Rect {
    let r = rect(s, 0.0, 0.0, 38.0, 21.0);
    s.set_point(r.p[1], [38.5, 1.2]).unwrap();
    s.set_point(r.p[2], [41.0, 19.0]).unwrap();
    s.set_point(r.p[3], [-0.7, 22.4]).unwrap();
    s.add_constraint(C::Fixed(r.p[0])).unwrap();
    s.add_constraint(C::Horizontal(r.l[0])).unwrap();
    s.add_constraint(C::Horizontal(r.l[2])).unwrap();
    s.add_constraint(C::Vertical(r.l[1])).unwrap();
    s.add_constraint(C::Vertical(r.l[3])).unwrap();
    s.add_constraint(C::Distance { a: r.l[0], b: r.l[0], value: 40.0 }).unwrap();
    r
}

fn pos(r: &SolveResult, p: EntityId) -> [f64; 2] {
    r.sketch.point(p).unwrap()
}

#[test]
fn constrained_rectangle_is_well_constrained() {
    let mut s = Sketch::new();
    let r = skewed_rect(&mut s);
    s.add_constraint(C::Distance { a: r.l[1], b: r.l[1], value: 20.0 }).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::WellConstrained, "{res:?}");
    assert_eq!(res.dof, 0);
    assert!(res.free_entities.is_empty());
    assert!(res.redundant.is_empty() && res.conflicting.is_empty());
    assert!(res.max_residual <= 1e-10);
    for (p, e) in r.p.iter().zip([[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]) {
        assert!(close(pos(&res, *p), e, 1e-9), "{:?} vs {e:?}", pos(&res, *p));
    }
}

#[test]
fn rectangle_missing_height_has_one_dof() {
    let mut s = Sketch::new();
    let r = skewed_rect(&mut s);
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::UnderConstrained);
    assert_eq!(res.dof, 1);
    // The top edge can slide up and down: its corners and the three edges that
    // touch them are free; the fixed corner, its neighbour and the bottom are not.
    let mut expected = vec![r.p[2], r.p[3], r.l[1], r.l[2], r.l[3]];
    expected.sort();
    assert_eq!(res.free_entities, expected);
}

#[test]
fn extra_parallel_is_redundant() {
    let mut s = Sketch::new();
    let r = skewed_rect(&mut s);
    s.add_constraint(C::Distance { a: r.l[1], b: r.l[1], value: 20.0 }).unwrap();
    let extra = s.add_constraint(C::Parallel(r.l[0], r.l[2])).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::OverConstrained);
    assert_eq!(res.redundant, vec![extra]);
    assert!(res.conflicting.is_empty());
    assert_eq!(res.dof, 0);
    assert!(res.max_residual <= 1e-10);
    assert!(close(pos(&res, r.p[2]), [40.0, 20.0], 1e-9));
}

#[test]
fn two_different_distances_conflict() {
    let mut s = Sketch::new();
    let p = s.add_point([0.0, 0.0]).unwrap();
    let q = s.add_point([12.0, 1.0]).unwrap();
    s.add_constraint(C::Fixed(p)).unwrap();
    let d1 = s.add_constraint(C::Distance { a: p, b: q, value: 10.0 }).unwrap();
    let d2 = s.add_constraint(C::Distance { a: p, b: q, value: 20.0 }).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::OverConstrained, "{res:?}");
    assert_eq!(res.conflicting, vec![d1, d2]);
    assert!(res.redundant.is_empty());
    assert!(res.error.is_none());
    // Least-squares compromise: |pq| = 15, p stays fixed.
    let qq = pos(&res, q);
    assert!(((qq[0] * qq[0] + qq[1] * qq[1]).sqrt() - 15.0).abs() < 1e-6);
    // With a non-zero optimal cost (25 mm²) the cost can only resolve movements of
    // about sqrt(eps · 25) ≈ 1e-7 mm, so the fixed point stays put to that level.
    assert!(close(pos(&res, p), [0.0, 0.0], 1e-6), "{:?}", pos(&res, p));
}

#[test]
fn conflicting_rectangle_dimension_is_reported_with_its_partners() {
    let mut s = Sketch::new();
    let r = skewed_rect(&mut s);
    s.add_constraint(C::Distance { a: r.l[1], b: r.l[1], value: 20.0 }).unwrap();
    let bad = s.add_constraint(C::Distance { a: r.p[0], b: r.p[1], value: 30.0 }).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::OverConstrained);
    assert!(res.conflicting.contains(&bad));
    // The width constraint (id 5) fights the 30 mm distance.
    assert!(res.conflicting.contains(&gusset_sketch::ConstraintId(5)), "{:?}", res.conflicting);
    assert!(!res.conflicting.contains(&gusset_sketch::ConstraintId(1)), "horizontal is satisfied");
}

/// Slot: two half-circle arcs joined by two tangent lines.
fn slot(perturb: f64) -> (Sketch, [EntityId; 4], [EntityId; 6]) {
    let mut s = Sketch::new();
    let e = perturb;
    let c2 = s.add_point([0.0, 0.0]).unwrap();
    let c1 = s.add_point([39.0 + e, 0.5 * e]).unwrap();
    let pa = s.add_point([0.3 * e, -9.0 - e]).unwrap();
    let pb = s.add_point([39.0 + e, -9.5]).unwrap();
    let pc = s.add_point([39.5, 9.0 + e]).unwrap();
    let pd = s.add_point([-0.2 * e, 9.5]).unwrap();
    let right = s.add_arc_between(c1, pb, pc).unwrap();
    let left = s.add_arc_between(c2, pd, pa).unwrap();
    let bottom = s.add_line_between(pa, pb).unwrap();
    let top = s.add_line_between(pc, pd).unwrap();
    let axis = s.add_line_between(c2, c1).unwrap();
    s.set_construction(axis, true).unwrap();
    s.add_constraint(C::Tangent(bottom, right)).unwrap();
    s.add_constraint(C::Tangent(left, bottom)).unwrap();
    s.add_constraint(C::Tangent(top, right)).unwrap();
    s.add_constraint(C::Tangent(top, left)).unwrap();
    s.add_constraint(C::Equal(left, right)).unwrap();
    s.add_constraint(C::Fixed(c2)).unwrap();
    s.add_constraint(C::Radius { c: left, value: 10.0 }).unwrap();
    s.add_constraint(C::Horizontal(axis)).unwrap();
    s.add_constraint(C::Distance { a: axis, b: axis, value: 40.0 }).unwrap();
    (s, [right, left, bottom, top], [c2, c1, pa, pb, pc, pd])
}

#[test]
fn tangent_slot_solves() {
    let (s, [right, left, ..], [c2, c1, pa, pb, pc, pd]) = slot(0.6);
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::WellConstrained, "{res:?}");
    assert_eq!(res.dof, 0);
    assert!(close(pos(&res, c2), [0.0, 0.0], 1e-9));
    assert!(close(pos(&res, c1), [40.0, 0.0], 1e-8));
    assert!(close(pos(&res, pa), [0.0, -10.0], 1e-8));
    assert!(close(pos(&res, pb), [40.0, -10.0], 1e-8));
    assert!(close(pos(&res, pc), [40.0, 10.0], 1e-8));
    assert!(close(pos(&res, pd), [0.0, 10.0], 1e-8));
    assert!((res.sketch.radius(right).unwrap() - 10.0).abs() < 1e-9);
    assert!((res.sketch.radius(left).unwrap() - 10.0).abs() < 1e-9);
}

/// Line and arc with their own endpoints joined by `Coincident`: tangency is still
/// solved in its well-conditioned endpoint form and converges to round-off.
#[test]
fn tangent_at_coincident_endpoint() {
    let mut s = Sketch::new();
    let arc = s.add_arc([0.0, 0.0], [0.4, -10.3], [0.0, 10.0]).unwrap();
    let Geom::Arc { center, start, .. } = s.entity(arc).unwrap().geom else { panic!() };
    let line = s.add_line([0.3, -10.0], [-30.0, -9.0]).unwrap();
    let (la, lb) = s.line_points(line).unwrap();
    s.add_constraint(C::Fixed(center)).unwrap();
    s.add_constraint(C::Radius { c: arc, value: 10.0 }).unwrap();
    s.add_constraint(C::Coincident(la, start)).unwrap();
    s.add_constraint(C::Tangent(line, arc)).unwrap();
    s.add_constraint(C::Horizontal(line)).unwrap();
    s.add_constraint(C::Distance { a: line, b: line, value: 30.0 }).unwrap();
    let res = solve(&s, &opts());
    assert!(res.max_residual < 1e-12, "{res:?}");
    assert!(close(pos(&res, la), [0.0, -10.0], 1e-10), "{:?}", pos(&res, la));
    assert!(close(pos(&res, lb), [-30.0, -10.0], 1e-10));
}

#[test]
fn circle_inscribed_in_square() {
    let mut s = Sketch::new();
    let r = skewed_rect(&mut s);
    s.add_constraint(C::Distance { a: r.l[1], b: r.l[1], value: 40.0 }).unwrap();
    // Drawn roughly square, with the circle inside it: tangency keeps the side of
    // each line the centre starts on.
    s.set_point(r.p[2], [41.0, 38.0]).unwrap();
    s.set_point(r.p[3], [-0.7, 39.4]).unwrap();
    let circ = s.add_circle([18.0, 22.0], 15.0).unwrap();
    s.add_constraint(C::Tangent(circ, r.l[0])).unwrap();
    s.add_constraint(C::Tangent(r.l[3], circ)).unwrap();
    s.add_constraint(C::Tangent(r.l[2], circ)).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::WellConstrained, "{res:?}");
    let Geom::Circle { center, radius } = res.sketch.entity(circ).unwrap().geom else { panic!() };
    assert!((radius - 20.0).abs() < 1e-9);
    assert!(close(res.sketch.point(center).unwrap(), [20.0, 20.0], 1e-9));

    // A fourth tangency is implied by the other three: redundant, not conflicting.
    let fourth = s.add_constraint(C::Tangent(r.l[1], circ)).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::OverConstrained);
    assert_eq!(res.redundant, vec![fourth]);
    assert!(res.conflicting.is_empty());
}

#[test]
fn triangle_with_angles() {
    let mut s = Sketch::new();
    let a = s.add_point([0.0, 0.0]).unwrap();
    let b = s.add_point([28.0, 2.0]).unwrap();
    let c = s.add_point([12.0, 17.0]).unwrap();
    let ab = s.add_line_between(a, b).unwrap();
    let bc = s.add_line_between(b, c).unwrap();
    let ac = s.add_line_between(a, c).unwrap();
    s.add_constraint(C::Fixed(a)).unwrap();
    s.add_constraint(C::Horizontal(ab)).unwrap();
    s.add_constraint(C::Distance { a: ab, b: ab, value: 30.0 }).unwrap();
    s.add_constraint(C::Angle { a: ab, b: ac, value: PI / 3.0 }).unwrap();
    // Interior angle 45° at B: direction B→C is 135° from A→B.
    s.add_constraint(C::Angle { a: ab, b: bc, value: 0.75 * PI }).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::WellConstrained, "{res:?}");
    let ac_len = 30.0 * (PI / 4.0).sin() / (5.0 * PI / 12.0).sin();
    let expect = [ac_len * (PI / 3.0).cos(), ac_len * (PI / 3.0).sin()];
    assert!(close(pos(&res, c), expect, 1e-9), "{:?} vs {expect:?}", pos(&res, c));
}

/// A 100-segment polyline (201 entities) with lengths and relative angles.
fn long_chain(rng: &mut Rng, noise: f64) -> (Sketch, Vec<[f64; 2]>, Vec<EntityId>) {
    let mut s = Sketch::new();
    let mut truth = vec![[0.0, 0.0]];
    let mut heading: f64 = 0.0;
    let mut turns = Vec::new();
    let mut lengths = Vec::new();
    for i in 0..100 {
        let turn = if i == 0 { 0.0 } else { rng.range(-0.8, 0.8) };
        heading += turn;
        let len = rng.range(5.0, 15.0);
        let last = *truth.last().unwrap();
        truth.push([last[0] + len * heading.cos(), last[1] + len * heading.sin()]);
        turns.push(turn);
        lengths.push(len);
    }
    let pts: Vec<EntityId> = truth
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let n = if i == 0 { 0.0 } else { noise };
            s.add_point([p[0] + rng.range(-n, n), p[1] + rng.range(-n, n)]).unwrap()
        })
        .collect();
    let lines: Vec<EntityId> = pts.windows(2).map(|w| s.add_line_between(w[0], w[1]).unwrap()).collect();
    s.add_constraint(C::Fixed(pts[0])).unwrap();
    s.add_constraint(C::Horizontal(lines[0])).unwrap();
    for (i, &l) in lines.iter().enumerate() {
        s.add_constraint(C::Distance { a: l, b: l, value: lengths[i] }).unwrap();
        if i > 0 {
            s.add_constraint(C::Angle { a: lines[i - 1], b: l, value: turns[i] }).unwrap();
        }
    }
    (s, truth, pts)
}

#[test]
fn two_hundred_entities_within_budget() {
    let mut rng = Rng(7);
    let (s, truth, pts) = long_chain(&mut rng, 0.3);
    assert!(s.entities().len() >= 200);
    let t = Instant::now();
    let res = solve(&s, &opts());
    let elapsed = t.elapsed();
    assert_eq!(res.status, SolveStatus::WellConstrained, "{:?} {:?}", res.status, res.error);
    for (p, e) in pts.iter().zip(&truth) {
        assert!(close(pos(&res, *p), *e, 1e-7));
    }
    // Generous for unoptimised debug builds on slow CI machines.
    assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
}

#[test]
fn many_independent_components_solve_separately_and_fast() {
    let mut s = Sketch::new();
    let mut rects = Vec::new();
    for i in 0..60 {
        let x = (i % 10) as f64 * 100.0;
        let y = (i / 10) as f64 * 100.0;
        let r = rect(&mut s, x, y, 30.0, 10.0);
        s.set_point(r.p[2], [x + 31.0, y + 12.0]).unwrap();
        s.add_constraint(C::Fixed(r.p[0])).unwrap();
        s.add_constraint(C::Horizontal(r.l[0])).unwrap();
        s.add_constraint(C::Horizontal(r.l[2])).unwrap();
        s.add_constraint(C::Vertical(r.l[1])).unwrap();
        s.add_constraint(C::Vertical(r.l[3])).unwrap();
        s.add_constraint(C::Distance { a: r.l[0], b: r.l[0], value: 30.0 + i as f64 }).unwrap();
        s.add_constraint(C::Distance { a: r.l[1], b: r.l[1], value: 10.0 }).unwrap();
        rects.push((r, x, y, 30.0 + i as f64));
    }
    assert!(s.entities().len() >= 480);
    let t = Instant::now();
    // A component limit far below the whole sketch's unknowns proves decomposition.
    let o = SolveOptions { max_component_params: 16, ..opts() };
    let res = solve(&s, &o);
    assert!(t.elapsed() < Duration::from_secs(2));
    assert_eq!(res.status, SolveStatus::WellConstrained, "{:?}", res.error);
    for (r, x, y, w) in rects {
        assert!(close(pos(&res, r.p[2]), [x + w, y + 10.0], 1e-9));
    }
}

#[test]
fn component_limit_is_enforced() {
    let mut rng = Rng(1);
    let (s, _, _) = long_chain(&mut rng, 0.1);
    let res = solve(&s, &SolveOptions { max_component_params: 50, ..opts() });
    assert_eq!(res.status, SolveStatus::Failed);
    assert!(matches!(res.error, Some(SolveError::ComponentTooLarge { limit: 50, .. })));
}

#[test]
fn underconstrained_moves_minimally() {
    let mut s = Sketch::new();
    let l = s.add_line([0.0, 0.0], [10.0, 1.0]).unwrap();
    s.add_constraint(C::Horizontal(l)).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::UnderConstrained);
    assert_eq!(res.dof, 3);
    let (a, b) = s.line_points(l).unwrap();
    assert!(close(pos(&res, a), [0.0, 0.5], 1e-9));
    assert!(close(pos(&res, b), [10.0, 0.5], 1e-9));
}

#[test]
fn drag_pulls_geometry_along() {
    let mut s = Sketch::new();
    let l = s.add_line([0.0, 0.0], [10.0, 0.0]).unwrap();
    let (a, b) = s.line_points(l).unwrap();
    s.add_constraint(C::Fixed(a)).unwrap();
    s.add_constraint(C::Distance { a: l, b: l, value: 10.0 }).unwrap();
    let res = solve_drag(&s, &opts(), &[Drag { point: b, target: [0.0, 20.0] }]);
    assert_eq!(res.status, SolveStatus::UnderConstrained);
    assert!(close(pos(&res, a), [0.0, 0.0], 1e-9));
    assert!(close(pos(&res, b), [0.0, 10.0], 1e-6), "{:?}", pos(&res, b));

    // A free point dragged around follows exactly.
    let mut s2 = Sketch::new();
    let l2 = s2.add_line([0.0, 0.0], [10.0, 0.0]).unwrap();
    let (a2, b2) = s2.line_points(l2).unwrap();
    s2.add_constraint(C::Distance { a: l2, b: l2, value: 10.0 }).unwrap();
    let res = solve_drag(&s2, &opts(), &[Drag { point: b2, target: [12.0, 0.0] }]);
    // The other end moves, the dragged end (heavily weighted) almost stays put.
    assert!(close(pos(&res, b2), [12.0, 0.0], 1e-3), "{:?}", pos(&res, b2));
    assert!(close(pos(&res, a2), [2.0, 0.0], 1e-3), "{:?}", pos(&res, a2));

    let bad = solve_drag(&s, &opts(), &[Drag { point: l, target: [0.0, 0.0] }]);
    assert_eq!(bad.error, Some(SolveError::BadDrag(l)));
    let bad = solve_drag(&s, &opts(), &[Drag { point: b, target: [f64::NAN, 0.0] }]);
    assert_eq!(bad.status, SolveStatus::Failed);
}

#[test]
fn degenerate_geometry_never_panics() {
    let mut s = Sketch::new();
    // Zero-length lines, coincident circle centres, huge coordinates.
    let p = s.add_point([1.0, 1.0]).unwrap();
    let q = s.add_point([1.0, 1.0]).unwrap();
    let l1 = s.add_line_between(p, q).unwrap();
    let l2 = s.add_line([5.0, 5.0], [5.0, 5.0]).unwrap();
    let c1 = s.add_circle([0.0, 0.0], 1.0).unwrap();
    let c2 = s.add_circle([0.0, 0.0], 1.0).unwrap();
    let far = s.add_line([1e9, -1e9], [-1e9, 1e9]).unwrap();
    for c in [
        C::Parallel(l1, l2),
        C::Perpendicular(l1, far),
        C::Angle { a: l1, b: l2, value: 1.0 },
        C::Tangent(c1, c2),
        C::Tangent(l1, c1),
        C::Distance { a: p, b: q, value: 0.0 },
        C::Distance { a: p, b: l2, value: 3.0 },
        C::Equal(l1, far),
        C::Symmetric { a: p, b: q, axis: l2 },
        C::Midpoint(p, l2),
        C::PointOnLine(q, far),
    ] {
        s.add_constraint(c).unwrap();
    }
    let res = solve(&s, &opts());
    // Whatever the outcome, it is a structured result with finite numbers.
    assert!(res.max_residual.is_finite() || res.status == SolveStatus::Failed);
    for e in res.sketch.entities() {
        if let Geom::Point { p } = e.geom {
            assert!(p[0].is_finite() && p[1].is_finite());
        }
    }
    // Hostile options are clamped rather than trusted.
    let wild = SolveOptions {
        tolerance: f64::NAN,
        max_iterations: u32::MAX,
        rank_tolerance: -1.0,
        max_component_params: usize::MAX,
    };
    let o = wild.sanitized();
    assert_eq!(o.tolerance, 1e-10);
    assert!(o.max_iterations <= 10_000 && o.max_component_params <= 4_000 && o.rank_tolerance > 0.0);
    let _ = solve(&s, &wild);
}

#[test]
fn solving_is_bit_for_bit_deterministic() {
    let run = || {
        let mut rng = Rng(42);
        let (s, _, _) = long_chain(&mut rng, 0.5);
        solve(&s, &opts())
    };
    let (a, b) = (run(), run());
    let bits = |r: &SolveResult| -> Vec<u64> {
        r.sketch
            .entities()
            .iter()
            .filter_map(|e| if let Geom::Point { p } = e.geom { Some(p) } else { None })
            .flat_map(|p| [p[0].to_bits(), p[1].to_bits()])
            .collect()
    };
    assert_eq!(bits(&a), bits(&b));
    assert_eq!(a, b);
    // The slot (arcs, tangents) too.
    assert_eq!(solve(&slot(0.4).0, &opts()), solve(&slot(0.4).0, &opts()));
}

#[test]
fn every_constraint_kind_solves_in_a_mixed_sketch() {
    let mut s = Sketch::new();
    let r = skewed_rect(&mut s);
    s.add_constraint(C::Distance { a: r.l[1], b: r.l[1], value: 20.0 }).unwrap();
    // A centre mark on the bottom edge, a symmetric pair about the vertical axis.
    let m = s.add_point([19.0, 1.0]).unwrap();
    s.add_constraint(C::Midpoint(m, r.l[0])).unwrap();
    let axis_top = s.add_point([21.0, 19.0]).unwrap();
    let axis = s.add_line_between(m, axis_top).unwrap();
    s.set_construction(axis, true).unwrap();
    s.add_constraint(C::Perpendicular(axis, r.l[0])).unwrap();
    s.add_constraint(C::PointOnLine(axis_top, r.l[2])).unwrap();
    let h1 = s.add_circle([10.0, 9.0], 3.0).unwrap();
    let h2 = s.add_circle([31.0, 11.0], 3.5).unwrap();
    let (Geom::Circle { center: c1, .. }, Geom::Circle { center: c2, .. }) =
        (s.entity(h1).unwrap().geom, s.entity(h2).unwrap().geom)
    else {
        panic!()
    };
    s.add_constraint(C::Symmetric { a: c1, b: c2, axis }).unwrap();
    s.add_constraint(C::Equal(h1, h2)).unwrap();
    s.add_constraint(C::Diameter { c: h1, value: 6.0 }).unwrap();
    s.add_constraint(C::Distance { a: c1, b: r.l[3], value: 8.0 }).unwrap();
    s.add_constraint(C::Distance { a: r.l[0], b: c1, value: 10.0 }).unwrap();
    // A point on a circle at a given angle via a construction spoke.
    let rim = s.add_point([13.0, 9.5]).unwrap();
    s.add_constraint(C::PointOnCircle(rim, h1)).unwrap();
    let spoke = s.add_line_between(c1, rim).unwrap();
    s.set_construction(spoke, true).unwrap();
    s.add_constraint(C::Angle { a: r.l[0], b: spoke, value: PI / 6.0 }).unwrap();
    let pin = s.add_point([0.0, 0.0]).unwrap();
    s.add_constraint(C::Coincident(pin, r.p[0])).unwrap();
    let res = solve(&s, &opts());
    assert_eq!(res.status, SolveStatus::WellConstrained, "{res:?}");
    assert!(close(res.sketch.point(c1).unwrap(), [8.0, 10.0], 1e-9));
    assert!(close(res.sketch.point(c2).unwrap(), [32.0, 10.0], 1e-9));
    assert!(close(res.sketch.point(m).unwrap(), [20.0, 0.0], 1e-9));
    let rp = res.sketch.point(rim).unwrap();
    assert!(close(rp, [8.0 + 3.0 * (PI / 6.0).cos(), 10.0 + 3.0 * (PI / 6.0).sin()], 1e-9));
    assert_eq!(res.sketch.radius(h2), Some(3.0));
}

/// Property: random well-constrained sketches, perturbed, solve back to the truth.
#[test]
fn perturbed_well_constrained_sketches_converge_back() {
    for seed in 0..60u64 {
        let mut rng = Rng(seed);
        let n = 3 + (rng.next_u64() % 6) as usize;
        // Ground truth: an open chain plus a circle centred on one of its points
        // and tangent to the first segment's line via a second chain point.
        let mut truth = vec![[rng.range(-50.0, 50.0), rng.range(-50.0, 50.0)]];
        let mut heading = rng.range(-PI, PI);
        let mut segs = Vec::new();
        for _ in 1..n {
            let turn = rng.range(-1.2, 1.2);
            heading += turn;
            let len = rng.range(5.0, 30.0);
            let last = *truth.last().unwrap();
            truth.push([last[0] + len * heading.cos(), last[1] + len * heading.sin()]);
            segs.push((turn, len, heading));
        }
        let radius = rng.range(1.0, 8.0);
        let mut s = Sketch::new();
        let ref_line = s.add_line([0.0, 0.0], [10.0, 0.0]).unwrap();
        s.set_construction(ref_line, true).unwrap();
        s.add_constraint(C::Fixed(ref_line)).unwrap();
        let scale = 0.04;
        let pts: Vec<EntityId> = truth
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let k = if i == 0 { 0.0 } else { scale * 20.0 };
                s.add_point([p[0] + rng.range(-k, k), p[1] + rng.range(-k, k)]).unwrap()
            })
            .collect();
        s.add_constraint(C::Fixed(pts[0])).unwrap();
        let lines: Vec<EntityId> = pts.windows(2).map(|w| s.add_line_between(w[0], w[1]).unwrap()).collect();
        for (i, (&l, &(turn, len, heading))) in lines.iter().zip(&segs).enumerate() {
            s.add_constraint(C::Distance { a: l, b: l, value: len }).unwrap();
            let c = if i == 0 {
                C::Angle { a: ref_line, b: l, value: heading }
            } else {
                C::Angle { a: lines[i - 1], b: l, value: turn }
            };
            s.add_constraint(c).unwrap();
        }
        let centre_truth = truth[n - 1];
        let circ = s.add_circle([centre_truth[0] + 0.3, centre_truth[1] - 0.2], radius * 1.1).unwrap();
        let Geom::Circle { center, .. } = s.entity(circ).unwrap().geom else { panic!() };
        s.add_constraint(C::Coincident(center, pts[n - 1])).unwrap();
        s.add_constraint(C::Radius { c: circ, value: radius }).unwrap();

        let res = solve(&s, &opts());
        assert_eq!(res.status, SolveStatus::WellConstrained, "seed {seed}: {res:?}");
        for (p, t) in pts.iter().zip(&truth) {
            assert!(close(res.sketch.point(*p).unwrap(), *t, 1e-7), "seed {seed}");
        }
        assert!((res.sketch.radius(circ).unwrap() - radius).abs() < 1e-9);
    }
}
