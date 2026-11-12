//! A planar face of a body as a profile: its boundary loops in a frame on the face's plane
//! (a face used as the profile of an extrude, sweep or loft).

use crate::KResult;
use crate::types::*;

/// Most edges one face profile may have.
const MAX_FACE_EDGES: usize = 4096;

/// A face edge as a segment: its tag and its end vertices (none for a full circle).
type FaceSeg = (Seg2, u64, Option<(u32, u32)>);

fn bad(msg: impl Into<String>) -> KernelError {
    KernelError::new(KernelErrorKind::InvalidInput, msg)
}

/// The planar face `face` of a shape with topology `topo`, as a profile of one region whose
/// frame lies on the face's plane with its normal along the face's outward normal. The outer
/// loop runs counter-clockwise, holes clockwise. Each profile edge's tag is the index of the
/// body edge it comes from. Lines, circles and circular arcs are read; other curves are refused.
pub fn face_profile_from_topology(topo: &Topology, face: u32) -> KResult<Profile> {
    let f = topo.faces.get(face as usize).ok_or_else(|| bad(format!("face {face} does not exist")))?;
    if f.kind != SurfaceKind::Plane {
        return Err(bad("a profile face must be planar"));
    }
    let n = normalize(f.normal).ok_or_else(|| bad("the face has no normal"))?;
    // The frame's x: the world axis least along the normal, projected onto the plane.
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let a =
        axes.iter().copied().min_by(|p, q| dot(*p, n).abs().total_cmp(&dot(*q, n).abs())).unwrap_or([1.0, 0.0, 0.0]);
    let x = normalize(sub(a, scale(n, dot(a, n)))).ok_or_else(|| bad("the face has no frame"))?;
    let frame = Frame { origin: f.centroid, x, y: cross(n, x) };
    let to2 = |p: V3| -> P2 {
        let d = sub(p, frame.origin);
        [dot(d, frame.x), dot(d, frame.y)]
    };
    let vertex = |i: u32| topo.vertices.get(i as usize).map(|v| to2(v.point)).ok_or_else(|| bad("a vertex is missing"));

    // The face's edges as segments, with their end vertices (none for a full circle).
    let mut segs: Vec<FaceSeg> = Vec::new();
    for (i, e) in topo.edges.iter().enumerate() {
        if !e.faces.contains(&face) {
            continue;
        }
        if segs.len() >= MAX_FACE_EDGES {
            return Err(bad("the face has too many edges"));
        }
        let tag = i as u64;
        match (e.kind, e.vertices.as_slice(), e.circle) {
            (CurveKind::Line, [a, b], _) => {
                segs.push((Seg2::Line { a: vertex(*a)?, b: vertex(*b)? }, tag, Some((*a, *b))));
            }
            (CurveKind::Circle, [_] | [], Some(c)) => {
                segs.push((Seg2::Circle { center: to2(c.center), radius: c.radius }, tag, None));
            }
            (CurveKind::Circle, [a, b], Some(c)) => {
                let (center, s, t, m) = (to2(c.center), vertex(*a)?, vertex(*b)?, to2(e.midpoint));
                let ang = |p: P2| libm::atan2(p[1] - center[1], p[0] - center[0]);
                let tau = std::f64::consts::TAU;
                let (a0, am, a1) = (ang(s), ang(m), ang(t));
                let ccw = (am - a0).rem_euclid(tau) < (a1 - a0).rem_euclid(tau);
                segs.push((Seg2::Arc { center, start: s, end: t, ccw }, tag, Some((*a, *b))));
            }
            _ => return Err(bad("a profile face bounded by curves other than lines, circles and arcs")),
        }
    }
    if segs.is_empty() {
        return Err(bad("the face has no edges"));
    }

    // Chain the edges into loops by their vertices.
    let mut loops: Vec<Vec<ProfileEdge>> = Vec::new();
    let mut used = vec![false; segs.len()];
    for (i, (seg, tag, ends)) in segs.iter().enumerate() {
        if ends.is_none() {
            loops.push(vec![ProfileEdge { seg: *seg, tag: *tag }]);
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
        }
    }
    while let Some(start) = used.iter().position(|u| !u) {
        let mut lp: Vec<ProfileEdge> = Vec::new();
        let Some((seg, tag, Some((v0, mut at)))) = segs.get(start).copied() else { break };
        if let Some(u) = used.get_mut(start) {
            *u = true;
        }
        lp.push(ProfileEdge { seg, tag });
        while at != v0 {
            let mut next = segs.iter().enumerate().filter(|(j, (_, _, ends))| {
                !used.get(*j).copied().unwrap_or(true) && ends.is_some_and(|(a, b)| a == at || b == at)
            });
            let (j, (seg, tag, ends)) = next.next().ok_or_else(|| bad("the face's boundary does not close"))?;
            if next.next().is_some() {
                return Err(bad("the face's boundary branches at a vertex"));
            }
            let Some((a, b)) = *ends else { break };
            let (seg, other) = if a == at { (*seg, b) } else { (reversed(*seg), a) };
            lp.push(ProfileEdge { seg, tag: *tag });
            if let Some(u) = used.get_mut(j) {
                *u = true;
            }
            at = other;
        }
        loops.push(lp);
    }

    // The loop of the largest area is the outer one.
    let mut areas: Vec<f64> = loops.iter().map(|l| signed_area(l)).collect();
    let outer = (0..loops.len())
        .max_by(|i, j| {
            let a = areas.get(*i).map_or(0.0, |v| v.abs());
            let b = areas.get(*j).map_or(0.0, |v| v.abs());
            a.total_cmp(&b)
        })
        .ok_or_else(|| bad("the face has no boundary"))?;
    let mut region = Region2::default();
    for (i, mut lp) in loops.into_iter().enumerate() {
        let area = areas.get_mut(i).map_or(0.0, |a| *a);
        let want_ccw = i == outer;
        if (area > 0.0) != want_ccw {
            lp.reverse();
            for e in &mut lp {
                e.seg = reversed(e.seg);
            }
        }
        if want_ccw {
            region.outer = Loop2 { edges: lp };
        } else {
            region.holes.push(Loop2 { edges: lp });
        }
    }
    Ok(Profile { frame, regions: vec![region] })
}

/// The segment run the other way (a full circle stays as it is).
fn reversed(s: Seg2) -> Seg2 {
    match s {
        Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
        Seg2::Arc { center, start, end, ccw } => Seg2::Arc { center, start: end, end: start, ccw: !ccw },
        other => other,
    }
}

/// Signed area of a loop of lines, arcs and circles (positive: counter-clockwise; a full
/// circle counts as counter-clockwise).
fn signed_area(lp: &[ProfileEdge]) -> f64 {
    let mut a = 0.0;
    for e in lp {
        match e.seg {
            Seg2::Line { a: p, b: q } => a += 0.5 * (p[0] * q[1] - q[0] * p[1]),
            Seg2::Circle { radius, .. } => a += std::f64::consts::PI * radius * radius,
            Seg2::Arc { center, start, end, ccw } => {
                // The chord's triangle from the origin plus the circular segment.
                let r = libm::hypot(start[0] - center[0], start[1] - center[1]);
                let ang = |p: P2| libm::atan2(p[1] - center[1], p[0] - center[0]);
                let tau = std::f64::consts::TAU;
                let mut sweep = (ang(end) - ang(start)).rem_euclid(tau);
                if !ccw {
                    sweep -= tau;
                }
                if sweep == 0.0 {
                    sweep = if ccw { tau } else { -tau };
                }
                let segment = 0.5 * r * r * (sweep - libm::sin(sweep));
                a += 0.5 * (start[0] * end[1] - end[0] * start[1]) + segment;
            }
            _ => {}
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(kind: CurveKind, faces: Vec<u32>, vertices: Vec<u32>, mid: V3, circle: Option<Circle3>) -> EdgeInfo {
        EdgeInfo { kind, length: 1.0, midpoint: mid, faces, vertices, circle }
    }

    /// A 10 x 10 square face at z 5 (normal +z) with a circular hole of r 2 about its centre.
    #[test]
    fn square_with_a_round_hole() {
        let pts = [[0.0, 0.0, 5.0], [10.0, 0.0, 5.0], [10.0, 10.0, 5.0], [0.0, 10.0, 5.0], [7.0, 5.0, 5.0]];
        let topo = Topology {
            faces: vec![FaceInfo {
                kind: SurfaceKind::Plane,
                area: 100.0 - 4.0 * std::f64::consts::PI,
                centroid: [5.0, 5.0, 5.0],
                normal: [0.0, 0.0, 1.0],
                axis: None,
            }],
            edges: vec![
                edge(CurveKind::Line, vec![0], vec![1, 0], [5.0, 0.0, 5.0], None),
                edge(CurveKind::Line, vec![0], vec![2, 3], [5.0, 10.0, 5.0], None),
                edge(CurveKind::Line, vec![0], vec![1, 2], [10.0, 5.0, 5.0], None),
                edge(CurveKind::Line, vec![0], vec![3, 0], [0.0, 5.0, 5.0], None),
                edge(
                    CurveKind::Circle,
                    vec![0],
                    vec![4],
                    [3.0, 5.0, 5.0],
                    Some(Circle3 { center: [5.0, 5.0, 5.0], normal: [0.0, 0.0, 1.0], radius: 2.0 }),
                ),
            ],
            vertices: pts.iter().map(|p| VertexInfo { point: *p }).collect(),
            solids: 1,
            shells: 1,
        };
        let p = face_profile_from_topology(&topo, 0).unwrap();
        assert!((dot(p.frame.normal(), [0.0, 0.0, 1.0]) - 1.0).abs() < 1e-12);
        let r = &p.regions[0];
        assert_eq!(r.outer.edges.len(), 4);
        assert!(signed_area(&r.outer.edges) > 99.9);
        assert_eq!(r.holes.len(), 1);
        assert!(matches!(r.holes[0].edges[0].seg, Seg2::Circle { radius, .. } if (radius - 2.0).abs() < 1e-12));
        assert_eq!(r.holes[0].edges[0].tag, 4);
        // The loop closes: each edge starts where the last ended.
        let ends: Vec<(P2, P2)> = r
            .outer
            .edges
            .iter()
            .map(|e| match e.seg {
                Seg2::Line { a, b } => (a, b),
                _ => unreachable!(),
            })
            .collect();
        for k in 0..4 {
            let (_, b) = ends[k];
            let (a, _) = ends[(k + 1) % 4];
            assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
        }
    }

    #[test]
    fn a_curved_face_is_refused() {
        let topo = Topology {
            faces: vec![FaceInfo {
                kind: SurfaceKind::Cylinder,
                area: 1.0,
                centroid: [0.0; 3],
                normal: [1.0, 0.0, 0.0],
                axis: None,
            }],
            ..Topology::default()
        };
        assert!(face_profile_from_topology(&topo, 0).is_err());
    }
}
