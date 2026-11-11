//! Regions of one profile that share an edge are one region.
//!
//! A sketch with crossing or overlapping curves has several regions side by side; a feature may
//! take some of them. The kernel sweeps every region of a profile into a solid of its own, so two
//! regions with a common edge gave two solids that touch along a wall: one body in name, but
//! later cuts and joins change only some of its solids (a slot across such a body was cut out of
//! one of them). The regions a feature takes are therefore united first: the edges two of them
//! share drop out and the rest is chained into one outline and its holes. Regions that only meet
//! in a point, or overlap along part of an edge, stay as they are.

use gusset_kernel::{Loop2, P2, ProfileEdge, Region2, Seg2};

const EPS: f64 = 1e-9;

fn near(a: P2, b: P2) -> bool {
    (a[0] - b[0]).abs() <= EPS && (a[1] - b[1]).abs() <= EPS
}

/// The two ends of an open segment; `None` for a circle or an ellipse (a loop on its own).
fn ends(s: &Seg2) -> Option<(P2, P2)> {
    match *s {
        Seg2::Line { a, b } => Some((a, b)),
        Seg2::Arc { start, end, .. } | Seg2::EllipseArc { start, end, .. } => Some((start, end)),
        Seg2::Bezier { p } => Some((p[0], p[3])),
        Seg2::RationalBezier { degree, poles, .. } => {
            Some((poles[0], poles.get(usize::from(degree)).copied().unwrap_or(poles[0])))
        }
        Seg2::Circle { .. } | Seg2::Ellipse { .. } => None,
    }
}

/// The same segment travelled the other way.
fn reversed(s: &Seg2) -> Seg2 {
    match *s {
        Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
        Seg2::Arc { center, start, end, ccw } => Seg2::Arc { center, start: end, end: start, ccw: !ccw },
        Seg2::Bezier { p } => Seg2::Bezier { p: [p[3], p[2], p[1], p[0]] },
        Seg2::RationalBezier { degree, mut poles, mut weights } => {
            let n = usize::from(degree) + 1;
            if let (Some(p), Some(w)) = (poles.get_mut(..n), weights.get_mut(..n)) {
                p.reverse();
                w.reverse();
            }
            Seg2::RationalBezier { degree, poles, weights }
        }
        Seg2::EllipseArc { center, major, a, b, start, end, ccw } => {
            Seg2::EllipseArc { center, major, a, b, start: end, end: start, ccw: !ccw }
        }
        whole => whole,
    }
}

/// The same piece of curve, whichever way each is travelled.
fn same_piece(x: &ProfileEdge, y: &ProfileEdge) -> bool {
    if x.tag != y.tag {
        return false;
    }
    let same_way = |a: &Seg2, b: &Seg2| match (*a, *b) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => near(a, c) && near(b, d),
        (Seg2::Arc { center, start, end, ccw }, Seg2::Arc { center: c, start: s, end: e, ccw: w }) => {
            near(center, c) && near(start, s) && near(end, e) && ccw == w
        }
        (Seg2::Bezier { p }, Seg2::Bezier { p: q }) => p.iter().zip(q).all(|(a, b)| near(*a, b)),
        (
            Seg2::RationalBezier { degree, poles, weights },
            Seg2::RationalBezier { degree: d2, poles: p2, weights: w2 },
        ) => {
            degree == d2
                && poles.iter().zip(p2).all(|(a, b)| near(*a, b))
                && weights.iter().zip(w2).all(|(a, b)| (a - b).abs() <= EPS)
        }
        (Seg2::Circle { center, radius }, Seg2::Circle { center: c, radius: r }) => {
            near(center, c) && (radius - r).abs() <= EPS
        }
        (Seg2::Ellipse { center, major, a, b }, Seg2::Ellipse { center: c, major: m, a: a2, b: b2 }) => {
            near(center, c) && near(major, m) && (a - a2).abs() <= EPS && (b - b2).abs() <= EPS
        }
        (
            Seg2::EllipseArc { center, major, a, b, start, end, ccw },
            Seg2::EllipseArc { center: c, major: m, a: a2, b: b2, start: s, end: e, ccw: w },
        ) => {
            near(center, c)
                && near(major, m)
                && (a - a2).abs() <= EPS
                && (b - b2).abs() <= EPS
                && near(start, s)
                && near(end, e)
                && ccw == w
        }
        _ => false,
    };
    same_way(&x.seg, &y.seg) || same_way(&x.seg, &reversed(&y.seg))
}

/// Points along a segment, enough to compare the sizes of loops.
fn samples(s: &Seg2) -> Vec<P2> {
    match *s {
        Seg2::Line { a, b } => vec![a, b],
        Seg2::Bezier { p } => p.to_vec(),
        Seg2::RationalBezier { .. } => s.rational_points(8),
        Seg2::Arc { center, start, end, ccw } => {
            let r = libm::hypot(start[0] - center[0], start[1] - center[1]);
            let (t0, t1) = (
                libm::atan2(start[1] - center[1], start[0] - center[0]),
                libm::atan2(end[1] - center[1], end[0] - center[0]),
            );
            let mut sweep = t1 - t0;
            if ccw && sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            if !ccw && sweep >= 0.0 {
                sweep -= std::f64::consts::TAU;
            }
            (0..=8)
                .map(|k| {
                    let t = t0 + sweep * f64::from(k) / 8.0;
                    [center[0] + r * libm::cos(t), center[1] + r * libm::sin(t)]
                })
                .collect()
        }
        Seg2::Circle { center, radius } => (0..16)
            .map(|k| {
                let t = std::f64::consts::TAU * f64::from(k) / 16.0;
                [center[0] + radius * libm::cos(t), center[1] + radius * libm::sin(t)]
            })
            .collect(),
        Seg2::Ellipse { .. } => s.ellipse_points(16),
        Seg2::EllipseArc { end, .. } => {
            let mut pts = s.ellipse_points(8);
            pts.push(end);
            pts
        }
    }
}

/// The area of the polygon through the sample points of a loop (unsigned).
fn loop_area(l: &Loop2) -> f64 {
    let pts: Vec<P2> = l.edges.iter().flat_map(|e| samples(&e.seg)).collect();
    let mut twice = 0.0;
    for (k, p) in pts.iter().enumerate() {
        if let Some(q) = pts.get((k + 1) % pts.len()) {
            twice += p[0] * q[1] - q[0] * p[1];
        }
    }
    twice.abs() / 2.0
}

/// The edges chained into closed loops; `None` when they do not chain in one way (an end that
/// meets no other edge, or more than one).
fn chain(mut edges: Vec<ProfileEdge>) -> Option<Vec<Loop2>> {
    let mut loops = Vec::new();
    while !edges.is_empty() {
        // From the first edge left, so that an outline keeps the start and the sense it had.
        let first = edges.remove(0);
        let Some((start, mut at)) = ends(&first.seg) else {
            loops.push(Loop2 { edges: vec![first] });
            continue;
        };
        let mut ring = vec![first];
        while !near(at, start) {
            let mut next = edges.iter().enumerate().filter_map(|(k, e)| {
                let (a, b) = ends(&e.seg)?;
                if near(a, at) {
                    Some((k, false, b))
                } else if near(b, at) {
                    Some((k, true, a))
                } else {
                    None
                }
            });
            let (Some((k, flip, far)), None) = (next.next(), next.next()) else { return None };
            let e = edges.swap_remove(k);
            ring.push(if flip { ProfileEdge { seg: reversed(&e.seg), tag: e.tag } } else { e });
            at = far;
        }
        loops.push(Loop2 { edges: ring });
    }
    Some(loops)
}

/// Unites the regions that share an edge; `ids[i]` names `regions[i]`, and a united region keeps
/// the first of its ids. A group that cannot be united stays as it was.
pub(crate) fn merge_touching(regions: Vec<Region2>, ids: Vec<String>) -> (Vec<Region2>, Vec<String>) {
    if regions.len() < 2 || regions.len() != ids.len() {
        return (regions, ids);
    }
    let all = |r: &Region2| -> Vec<ProfileEdge> {
        r.outer.edges.iter().chain(r.holes.iter().flat_map(|h| h.edges.iter())).copied().collect()
    };
    let edges: Vec<Vec<ProfileEdge>> = regions.iter().map(all).collect();
    // Groups of regions joined through shared edges.
    let mut group: Vec<usize> = (0..regions.len()).collect();
    for i in 0..regions.len() {
        for j in i + 1..regions.len() {
            let (Some(a), Some(b)) = (edges.get(i), edges.get(j)) else { continue };
            if a.iter().any(|x| b.iter().any(|y| same_piece(x, y))) {
                let (Some(gi), Some(gj)) = (group.get(i).copied(), group.get(j).copied()) else { continue };
                for g in &mut group {
                    if *g == gj {
                        *g = gi;
                    }
                }
            }
        }
    }
    let mut out_regions = Vec::new();
    let mut out_ids = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        if group.get(i) != Some(&i) {
            continue;
        }
        let members: Vec<usize> = (0..regions.len()).filter(|j| group.get(*j) == Some(&i)).collect();
        let merged = if members.len() > 1 { unite(&members, &edges) } else { None };
        match merged {
            Some(one) => {
                out_regions.push(one);
                out_ids.push(id.clone());
            }
            None => {
                // On its own, or not united: the members as they were.
                for j in members {
                    if let (Some(r), Some(n)) = (regions.get(j), ids.get(j)) {
                        out_regions.push(r.clone());
                        out_ids.push(n.clone());
                    }
                }
            }
        }
    }
    (out_regions, out_ids)
}

/// One region from the edges of several: every edge two of them have drops out.
/// The one segment two consecutive pieces of an entity make: a line on through a cut, or an
/// arc on around the same centre. `None` when they bend there.
fn joined(x: &ProfileEdge, y: &ProfileEdge) -> Option<ProfileEdge> {
    if x.tag != y.tag {
        return None;
    }
    let seg = match (x.seg, y.seg) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) if near(b, c) => {
            let (u, v) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
            let (lu, lv) = (libm::hypot(u[0], u[1]), libm::hypot(v[0], v[1]));
            let cross = (u[0] * v[1] - u[1] * v[0]) / (lu * lv);
            let dot = u[0] * v[0] + u[1] * v[1];
            (lu > EPS && lv > EPS && cross.abs() <= EPS && dot > 0.0).then_some(Seg2::Line { a, b: d })?
        }
        (Seg2::Arc { center, start, end, ccw }, Seg2::Arc { center: c, start: s, end: e, ccw: w })
            if near(center, c) && near(end, s) && ccw == w && !near(start, e) =>
        {
            Seg2::Arc { center, start, end: e, ccw }
        }
        _ => return None,
    };
    Some(ProfileEdge { seg, tag: x.tag })
}

/// Joins the pieces of one entity that follow each other in a united outline: the curve that
/// cut them there was a shared edge and dropped out, so they are one edge again (one wall when
/// swept, as in the source; two walls left the edge between them split).
fn join_pieces(l: &mut Loop2) {
    let mut k = 0;
    while l.edges.len() > 1 && k < l.edges.len() {
        let next = (k + 1) % l.edges.len();
        let one = match (l.edges.get(k), l.edges.get(next)) {
            (Some(x), Some(y)) => joined(x, y),
            _ => None,
        };
        match one {
            Some(e) if next > k => {
                if let Some(slot) = l.edges.get_mut(k) {
                    *slot = e;
                }
                l.edges.remove(next);
            }
            Some(e) => {
                // The last piece runs on into the first: the loop starts at the joined edge.
                l.edges.remove(k);
                if let Some(slot) = l.edges.get_mut(0) {
                    *slot = e;
                }
                break;
            }
            None => k += 1,
        }
    }
}

fn unite(members: &[usize], edges: &[Vec<ProfileEdge>]) -> Option<Region2> {
    let mut left: Vec<ProfileEdge> = Vec::new();
    let mut total = 0.0;
    for (n, m) in members.iter().enumerate() {
        for e in edges.get(*m)? {
            let shared = members
                .iter()
                .enumerate()
                .any(|(k, o)| k != n && edges.get(*o).is_some_and(|other| other.iter().any(|x| same_piece(e, x))));
            if !shared {
                left.push(*e);
            }
        }
        // The member's own area: its outline less its holes.
        let mut loops = edges.get(*m)?.clone();
        let own = chain(std::mem::take(&mut loops))?;
        let biggest = own.iter().map(loop_area).fold(0.0, f64::max);
        total += 2.0 * biggest - own.iter().map(loop_area).sum::<f64>();
    }
    let mut loops = chain(left)?;
    for l in &mut loops {
        join_pieces(l);
    }
    // The outline is the largest loop; the others are holes in it.
    let at = (0..loops.len()).max_by(|a, b| {
        let (x, y) = (loops.get(*a).map_or(0.0, loop_area), loops.get(*b).map_or(0.0, loop_area));
        x.total_cmp(&y)
    })?;
    let outer = loops.swap_remove(at);
    // United, the regions cover what they covered apart (as far as the sampled outlines tell).
    let area = loop_area(&outer) - loops.iter().map(loop_area).sum::<f64>();
    if (area - total).abs() > 1e-6 * (1.0 + total.abs()) {
        return None;
    }
    Some(Region2 { outer, holes: loops })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn line(a: P2, b: P2, tag: u64) -> ProfileEdge {
        ProfileEdge { seg: Seg2::Line { a, b }, tag }
    }

    /// An axis-parallel rectangle, counter-clockwise; edge `k` carries `tags[k]` (bottom, right,
    /// top, left).
    fn rect(x: (f64, f64), y: (f64, f64), tags: [u64; 4]) -> Region2 {
        let c = [[x.0, y.0], [x.1, y.0], [x.1, y.1], [x.0, y.1]];
        Region2 {
            outer: Loop2 { edges: (0..4).map(|k| line(c[k], c[(k + 1) % 4], tags[k])).collect() },
            holes: Vec::new(),
        }
    }

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|k| format!("r{k}")).collect()
    }

    #[test]
    fn two_regions_with_a_common_edge_become_one() {
        // Left and right half of a 4 x 2 rectangle; the wall between them is curve 9.
        let left = rect((0.0, 2.0), (0.0, 2.0), [1, 9, 3, 4]);
        let right = rect((2.0, 4.0), (0.0, 2.0), [1, 2, 3, 9]);
        let (regions, ids) = merge_touching(vec![left, right], names(2));
        assert_eq!(ids, vec!["r0"]);
        let [one] = regions.as_slice() else { panic!("one region") };
        assert!(one.holes.is_empty());
        // The two pieces of curve 1 (and of curve 3) are one edge again: the wall that cut them
        // dropped out.
        assert_eq!(one.outer.edges.len(), 4);
        assert!(one.outer.edges.iter().all(|e| e.tag != 9));
        for tag in [1, 3] {
            let e = one.outer.edges.iter().find(|e| e.tag == tag).unwrap();
            let (a, b) = ends(&e.seg).unwrap();
            assert!((a[0] - b[0]).abs() > 4.0 - 1e-12, "{a:?} {b:?}");
        }
        assert!((loop_area(&one.outer) - 8.0).abs() < 1e-12);
        // The loop is closed and ordered: each edge starts where the one before ends.
        for (k, e) in one.outer.edges.iter().enumerate() {
            let next = &one.outer.edges[(k + 1) % 4];
            assert!(near(ends(&e.seg).unwrap().1, ends(&next.seg).unwrap().0));
        }
    }

    #[test]
    fn regions_apart_or_meeting_in_a_point_stay_as_they_are() {
        let a = rect((0.0, 1.0), (0.0, 1.0), [1, 2, 3, 4]);
        let apart = rect((5.0, 6.0), (0.0, 1.0), [5, 6, 7, 8]);
        let corner = rect((1.0, 2.0), (1.0, 2.0), [5, 6, 7, 8]);
        for other in [apart, corner] {
            let before = vec![a.clone(), other];
            let (regions, ids) = merge_touching(before.clone(), names(2));
            assert_eq!((regions, ids), (before, names(2)));
        }
        // Edges that lie on each other without being the same piece are not shared.
        let longer = rect((1.0, 2.0), (0.0, 3.0), [5, 6, 7, 2]);
        let before = vec![a, longer];
        assert_eq!(merge_touching(before.clone(), names(2)).0, before);
    }

    #[test]
    fn a_ring_and_the_disc_in_it_become_the_whole_disc() {
        // A square with a round hole, and the disc that fills the hole.
        let hole = ProfileEdge { seg: Seg2::Circle { center: [1.0, 1.0], radius: 0.5 }, tag: 7 };
        let mut ring = rect((0.0, 2.0), (0.0, 2.0), [1, 2, 3, 4]);
        ring.holes.push(Loop2 { edges: vec![hole] });
        let disc = Region2 { outer: Loop2 { edges: vec![hole] }, holes: Vec::new() };
        let (regions, ids) = merge_touching(vec![ring.clone(), disc], names(2));
        assert_eq!(ids, vec!["r0"]);
        assert_eq!(regions, vec![Region2 { outer: ring.outer.clone(), holes: Vec::new() }]);
    }

    #[test]
    fn three_in_a_row_and_one_apart_give_two_regions_and_a_hole_survives() {
        // Three cells in a row (walls 8 and 9) around nothing, a fourth far away; the middle cell
        // has a hole of its own. The pieces of curves 1 and 3 are joined: the outline has four edges.
        let a = rect((0.0, 1.0), (0.0, 1.0), [1, 8, 3, 4]);
        let mut b = rect((1.0, 2.0), (0.0, 1.0), [1, 9, 3, 8]);
        b.holes
            .push(Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [1.5, 0.5], radius: 0.2 }, tag: 6 }] });
        let c = rect((2.0, 3.0), (0.0, 1.0), [1, 2, 3, 9]);
        let far = rect((7.0, 8.0), (0.0, 1.0), [11, 12, 13, 14]);
        let (regions, ids) = merge_touching(vec![a, far.clone(), b, c], names(4));
        assert_eq!(ids, vec!["r0", "r1"]);
        assert_eq!(regions[1], far);
        assert_eq!(regions[0].outer.edges.len(), 4);
        assert_eq!(regions[0].holes.len(), 1);
    }

    #[test]
    fn an_arc_wall_is_shared_whichever_way_it_is_travelled() {
        // A half disc and the rest of a square, split along an arc (curve 5).
        let arc = |ccw: bool| {
            let (start, end) = if ccw { ([2.0, 0.0], [0.0, 0.0]) } else { ([0.0, 0.0], [2.0, 0.0]) };
            ProfileEdge { seg: Seg2::Arc { center: [1.0, 0.0], start, end, ccw }, tag: 5 }
        };
        let half =
            Region2 { outer: Loop2 { edges: vec![line([0.0, 0.0], [2.0, 0.0], 1), arc(true)] }, holes: Vec::new() };
        let rest = Region2 {
            outer: Loop2 {
                edges: vec![
                    arc(false),
                    line([2.0, 0.0], [2.0, 2.0], 2),
                    line([2.0, 2.0], [0.0, 2.0], 3),
                    line([0.0, 2.0], [0.0, 0.0], 4),
                ],
            },
            holes: Vec::new(),
        };
        let (regions, _) = merge_touching(vec![half, rest], names(2));
        let [one] = regions.as_slice() else { panic!("one region") };
        assert_eq!(one.outer.edges.len(), 4);
        assert!((loop_area(&one.outer) - 4.0).abs() < 1e-12);
    }
}
