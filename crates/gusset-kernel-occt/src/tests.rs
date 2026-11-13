//! Golden geometry tests against real OCCT (volume, area, topology counts, history).

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::panic, clippy::disallowed_methods)]

use super::OcctKernel;
use gusset_kernel::*;

fn k() -> OcctKernel {
    OcctKernel::new().unwrap()
}

fn rect(w: f64, h: f64, tag0: u64) -> Loop2 {
    let pts = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
    Loop2 {
        edges: (0..4)
            .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: tag0 + i as u64 })
            .collect(),
    }
}

fn boxp(w: f64, h: f64) -> Profile {
    Profile { frame: Frame::XY, regions: vec![Region2 { outer: rect(w, h, 10), holes: vec![] }] }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[test]
fn extrude_box_golden() {
    let r = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let t = k().topology(&r.shape).unwrap();
    assert_eq!((t.faces.len(), t.edges.len(), t.vertices.len(), t.solids), (6, 12, 8, 1));
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, 8000.0, 1e-9), "{}", m.volume);
    assert!(close(m.area, 2.0 * 800.0 + 120.0 * 10.0, 1e-9));
    assert!(close(m.centroid[2], 5.0, 1e-9));
    // Every face has exactly one origin: 4 laterals with tags 10..13 and 2 caps.
    let mut lat: Vec<u64> = r
        .face_origins
        .iter()
        .flatten()
        .filter_map(|o| if let FaceOrigin::Lateral { tag } = o { Some(*tag) } else { None })
        .collect();
    lat.sort_unstable();
    assert_eq!(lat, vec![10, 11, 12, 13]);
    assert!(r.face_origins.iter().all(|o| o.len() == 1));
    // Lateral face from edge (0,0)->(40,0) faces -Y.
    let fi = r.face_origins.iter().position(|o| o == &vec![FaceOrigin::Lateral { tag: 10 }]).unwrap();
    assert!(close(t.faces[fi].normal[1], -1.0, 1e-9));
    // Start cap is in the sketch plane.
    let s = r.face_origins.iter().position(|o| matches!(o[0], FaceOrigin::Cap { end: CapEnd::Start, .. })).unwrap();
    assert!(t.faces[s].centroid[2].abs() < 1e-9);
    let bb = k().bbox(&r.shape).unwrap();
    assert!(close(bb.size()[0], 40.0, 1e-6));
}

/// A loop that touches itself at a point (a notch whose corner meets the outline, as on a board of
/// 2348) makes an edge four side faces share: closed and valid, so it builds.
#[test]
fn an_outline_touching_itself_at_a_point_extrudes() {
    let pts = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [0.0, 10.0],
        [0.0, 7.0],
        [4.0, 7.0],
        [4.0, 3.0],
        [0.1, 3.0],
        [0.0, 7.0],
    ];
    let edges = (0..pts.len())
        .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }, tag: i as u64 })
        .collect();
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let r = k().extrude(&p, &ExtrudeSpec { distance: 1.5, symmetric: false }).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, (100.0 - 15.8) * 1.5, 1e-9), "{}", m.volume);
}

#[test]
fn extrude_with_hole_and_arcs() {
    let mut region = Region2 { outer: rect(40.0, 40.0, 1), holes: vec![] };
    region
        .holes
        .push(Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [20.0, 20.0], radius: 5.0 }, tag: 99 }] });
    let p = Profile { frame: Frame::XY, regions: vec![region] };
    let r = k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: true }).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    let expected = (1600.0 - std::f64::consts::PI * 25.0) * 10.0;
    assert!(close(m.volume, expected, 1e-6), "{} vs {expected}", m.volume);
    let bb = k().bbox(&r.shape).unwrap();
    assert!(close(bb.min[2], -5.0, 1e-6) && close(bb.max[2], 5.0, 1e-6));
    assert!(r.face_origins.iter().flatten().any(|o| *o == FaceOrigin::Lateral { tag: 99 }));
    // A slot (lines + arcs).
    let slot = Loop2 {
        edges: vec![
            ProfileEdge { seg: Seg2::Line { a: [0.0, 0.0], b: [10.0, 0.0] }, tag: 1 },
            ProfileEdge {
                seg: Seg2::Arc { center: [10.0, 2.0], start: [10.0, 0.0], end: [10.0, 4.0], ccw: true },
                tag: 2,
            },
            ProfileEdge { seg: Seg2::Line { a: [10.0, 4.0], b: [0.0, 4.0] }, tag: 3 },
            ProfileEdge {
                seg: Seg2::Arc { center: [0.0, 2.0], start: [0.0, 4.0], end: [0.0, 0.0], ccw: true },
                tag: 4,
            },
        ],
    };
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: slot, holes: vec![] }] };
    let r = k().extrude(&p, &ExtrudeSpec { distance: 2.0, symmetric: false }).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    let expected = (40.0 + std::f64::consts::PI * 4.0) * 2.0;
    assert!(close(m.volume, expected, 1e-6), "{} vs {expected}", m.volume);
}

/// An arc whose ends lie on its circle to 1.1e-7 mm only (6299's chord across a 2.5 mm circle,
/// its end rounded where the file stored it, the other end of the arc an exact crossing): the
/// profile builds, the cap above the chord of its area.
#[test]
fn an_arc_whose_ends_are_a_hair_off_its_circle_builds() {
    let (r, d) = (2.5f64, 1.8f64);
    let exact = (r * r - d * d).sqrt();
    let rounded = (exact * 1e6).round() / 1e6;
    assert!(((rounded * rounded + d * d).sqrt() - r).abs() > 1e-7);
    let cap = Loop2 {
        edges: vec![
            ProfileEdge {
                seg: Seg2::Arc { center: [0.0, 0.0], start: [exact, d], end: [-rounded, d], ccw: true },
                tag: 1,
            },
            ProfileEdge { seg: Seg2::Line { a: [-rounded, d], b: [exact, d] }, tag: 2 },
        ],
    };
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: cap, holes: vec![] }] };
    let made = k().extrude(&p, &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap();
    let m = k().mass_properties(&made.shape).unwrap();
    let area = r * r * (d / r).acos() - d * exact;
    assert!(close(m.volume, area, 1e-5), "{} vs {area}", m.volume);
}

#[test]
fn revolve_cylinder() {
    let p = Profile { frame: Frame::XZ, regions: vec![Region2 { outer: rect(5.0, 10.0, 1), holes: vec![] }] };
    let full = k()
        .revolve(&p, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::TAU })
        .unwrap();
    let m = k().mass_properties(&full.shape).unwrap();
    assert!(close(m.volume, std::f64::consts::PI * 25.0 * 10.0, 1e-6), "{}", m.volume);
    let half = k()
        .revolve(&p, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::PI })
        .unwrap();
    assert!(half.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Cap { end: CapEnd::End, .. })));
    // Seams (F2-9): the full cylinder's cut line borders its side face on both sides;
    // the half cylinder has no seam.
    let seams = |shape: &Shape| {
        let t = k().topology(shape).unwrap();
        t.edges
            .iter()
            .filter(|e| {
                let mut f = e.faces.clone();
                f.dedup();
                f.len() < 2
            })
            .count()
    };
    assert_eq!(seams(&full.shape), 1);
    assert_eq!(seams(&half.shape), 0);
}

#[test]
fn boolean_cut_and_history() {
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let mut tool_p = boxp(10.0, 10.0);
    tool_p.frame.origin = [15.0, 5.0, 0.0];
    let b = k().extrude(&tool_p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let c = k().boolean(BoolOp::Cut, &a.shape, &b.shape).unwrap();
    let m = k().mass_properties(&c.shape).unwrap();
    assert!(close(m.volume, 8000.0 - 1000.0, 1e-6), "{}", m.volume);
    // Every output face traces back to an input face.
    assert!(c.face_origins.iter().all(|o| o.iter().any(|x| matches!(x, FaceOrigin::Input { .. }))));
    // The pocket walls come from the tool (input 1).
    assert!(c.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Input { input: 1, .. })));
}

/// Review H3: several tools in one operation, with per-tool history, the same volume as one
/// tool at a time, and the same result for every op the trait allows.
#[test]
fn boolean_many_cuts_and_fuses_with_history() {
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let tool_at = |x: f64| {
        let mut p = boxp(5.0, 5.0);
        p.frame.origin = [x, 5.0, 0.0];
        k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap().shape
    };
    let tools = [tool_at(2.0), tool_at(12.0), tool_at(22.0)];
    let c = k().boolean_many(BoolOp::Cut, &a.shape, &tools).unwrap();
    let seq = gusset_kernel::sequential_boolean(&k(), BoolOp::Cut, &a.shape, &tools).unwrap();
    let vol = |s: &Shape| k().mass_properties(s).unwrap().volume;
    assert!(close(vol(&c.shape), 8000.0 - 3.0 * 250.0, 1e-6), "{}", vol(&c.shape));
    assert!(close(vol(&seq.shape), vol(&c.shape), 1e-9));
    assert_eq!(k().topology(&c.shape).unwrap().faces.len(), k().topology(&seq.shape).unwrap().faces.len());
    // Each pocket's walls come from its own tool (inputs 1, 2, 3); input 0 is the target.
    for input in 0..=3u32 {
        assert!(
            c.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Input { input: i, .. } if *i == input))
        );
    }
    assert!(c.face_origins.iter().all(|o| !o.is_empty()));
    let mut seq_o = seq.face_origins.clone();
    let mut many_o: Vec<_> = c.face_origins.clone();
    seq_o.sort();
    many_o.sort();
    assert_eq!(seq_o, many_o, "the same faces come from the same inputs either way");
    // A fuse of disjoint copies keeps them all; intersect goes one tool at a time.
    let far = |x: f64| {
        let mut p = boxp(5.0, 5.0);
        p.frame.origin = [x, 50.0, 0.0];
        k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap().shape
    };
    let u = k().boolean_many(BoolOp::Union, &a.shape, &[far(0.0), far(10.0)]).unwrap();
    assert!(close(vol(&u.shape), 8000.0 + 500.0, 1e-6));
    let i = k().boolean_many(BoolOp::Intersect, &a.shape, &[tool_at(2.0), tool_at(4.0)]).unwrap();
    assert!(close(vol(&i.shape), 3.0 * 5.0 * 10.0, 1e-6), "{}", vol(&i.shape));
    // No tools: the target itself.
    let same = k().boolean_many(BoolOp::Cut, &a.shape, &[]).unwrap();
    assert!(close(vol(&same.shape), 8000.0, 1e-9));
}

#[test]
fn fillet_and_chamfer_history() {
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let t = k().topology(&a.shape).unwrap();
    // A vertical edge (along Z).
    let e = t.edges.iter().position(|e| (e.length - 10.0).abs() < 1e-9).unwrap() as u32;
    let f = k().fillet(&a.shape, &[e], 2.0).unwrap();
    let m = k().mass_properties(&f.shape).unwrap();
    let removed = (4.0 - std::f64::consts::PI) * 10.0; // r² - πr²/4 = 4 - π for r = 2
    assert!(close(m.volume, 8000.0 - removed, 1e-6), "{}", m.volume);
    assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    assert_eq!(k().topology(&f.shape).unwrap().faces.len(), 7);
    let c = k().chamfer(&a.shape, &[e], 1.0).unwrap();
    let m = k().mass_properties(&c.shape).unwrap();
    assert!(close(m.volume, 8000.0 - 0.5 * 10.0, 1e-6), "{}", m.volume);
    // Too-large fillet fails cleanly with a message.
    let err = k().fillet(&a.shape, &[e], 50.0).unwrap_err();
    assert!(!err.message.is_empty());
    // Bad edge index fails cleanly.
    assert!(k().fillet(&a.shape, &[9999], 1.0).is_err());
}

/// A 10 mm cube and one of its edges with both adjacent faces (ascending indices).
fn cube_edge() -> (OpResult, u32, [u32; 2]) {
    let a = k().extrude(&boxp(10.0, 10.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let t = k().topology(&a.shape).unwrap();
    let e = t.edges.iter().position(|e| (e.length - 10.0).abs() < 1e-9 && e.faces.len() == 2).unwrap();
    let f = &t.edges[e].faces;
    (a, e as u32, [f[0], f[1]])
}

/// Area of the output face that descends from input face `f`.
fn area_of(r: &OpResult, f: u32) -> f64 {
    let i = r.face_origins.iter().position(|o| o.contains(&FaceOrigin::Input { input: 0, face: f })).unwrap();
    k().topology(&r.shape).unwrap().faces[i].area
}

fn face_areas(r: &OpResult) -> Vec<f64> {
    let mut v: Vec<f64> = k().topology(&r.shape).unwrap().faces.iter().map(|f| f.area).collect();
    v.sort_by(f64::total_cmp);
    v
}

#[test]
fn chamfer_two_distances_follows_the_reference_face() {
    let (a, e, faces) = cube_edge();
    let spec = ChamferSpec::TwoDistances { first: 2.0, second: 3.0 };
    let mut shapes = Vec::new();
    for rf in faces {
        let c = k().chamfer_ref(&a.shape, &[e], &[rf], &spec).unwrap();
        let m = k().mass_properties(&c.shape).unwrap();
        assert!(close(m.volume, 1000.0 - 0.5 * 2.0 * 3.0 * 10.0, 1e-6), "{}", m.volume);
        assert_eq!(k().topology(&c.shape).unwrap().faces.len(), 7);
        // The reference face loses 2 mm of its width, the other 3 mm.
        let areas = face_areas(&c);
        assert!(
            areas.iter().any(|x| close(*x, 80.0, 1e-6)) && areas.iter().any(|x| close(*x, 70.0, 1e-6)),
            "{areas:?}"
        );
        // The chamfer face is a blend of the edge.
        assert!(c.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
        shapes.push(c);
    }
    // Swapping the reference face swaps which neighbour is cut by 2 mm.
    assert!(close(area_of(&shapes[0], faces[0]), 80.0, 1e-6) && close(area_of(&shapes[0], faces[1]), 70.0, 1e-6));
    assert!(close(area_of(&shapes[1], faces[1]), 80.0, 1e-6) && close(area_of(&shapes[1], faces[0]), 70.0, 1e-6));
}

#[test]
fn chamfer_distance_and_angle() {
    let (a, e, faces) = cube_edge();
    // 45 degrees: both legs equal.
    let spec = ChamferSpec::DistanceAngle { distance: 2.0, angle: std::f64::consts::FRAC_PI_4 };
    let c = k().chamfer_ref(&a.shape, &[e], &[faces[0]], &spec).unwrap();
    let v = k().mass_properties(&c.shape).unwrap().volume;
    assert!(close(v, 1000.0 - 0.5 * 2.0 * 2.0 * 10.0, 1e-6), "{v}");
    // 30 degrees from the reference face: the other leg is d * tan(30 deg).
    let spec = ChamferSpec::DistanceAngle { distance: 3.0, angle: std::f64::consts::FRAC_PI_6 };
    let other = 3.0 * std::f64::consts::FRAC_PI_6.tan();
    let c = k().chamfer_ref(&a.shape, &[e], &[faces[1]], &spec).unwrap();
    let v = k().mass_properties(&c.shape).unwrap().volume;
    assert!(close(v, 1000.0 - 0.5 * 3.0 * other * 10.0, 1e-6), "{v}");
    assert!(close(area_of(&c, faces[1]), 100.0 - 3.0 * 10.0, 1e-6));
    assert!(close(area_of(&c, faces[0]), 100.0 - other * 10.0, 1e-6));
}

#[test]
fn chamfer_ref_errors_are_clean() {
    let (a, e, faces) = cube_edge();
    let two = ChamferSpec::TwoDistances { first: 2.0, second: 3.0 };
    assert!(k().chamfer_ref(&a.shape, &[e], &[9999], &two).is_err());
    assert!(
        k().chamfer_ref(&a.shape, &[e], &[faces[0]], &ChamferSpec::TwoDistances { first: 2.0, second: -1.0 }).is_err()
    );
    let bad = ChamferSpec::DistanceAngle { distance: 2.0, angle: std::f64::consts::FRAC_PI_2 };
    assert!(k().chamfer_ref(&a.shape, &[e], &[faces[0]], &bad).is_err());
    // Equal goes to the plain chamfer.
    let c = k().chamfer_ref(&a.shape, &[e], &[], &ChamferSpec::Equal(1.0)).unwrap();
    assert!(close(k().mass_properties(&c.shape).unwrap().volume, 1000.0 - 5.0, 1e-6));
    // Too large for the cube.
    assert!(
        k().chamfer_ref(&a.shape, &[e], &[faces[0]], &ChamferSpec::TwoDistances { first: 50.0, second: 1.0 }).is_err()
    );
}

#[test]
fn tessellation_is_consistent() {
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let m = k().tessellate(&a.shape, &TessellationParams::default()).unwrap();
    assert_eq!(m.face_ranges.len(), 6);
    assert_eq!(m.edges.len(), 12);
    assert_eq!(m.triangle_count(), 12);
    assert_eq!(m.positions.len(), m.normals.len());
    assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
}

#[test]
fn step_face_colours_round_trip() {
    // ADR 0045: a body colour and two face colours ride on the name both ways.
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let topo = k().topology(&a.shape).unwrap();
    let step = k().export_step(&[(&a.shape, "Plate\u{1f}#808080\u{1f}1=d02020,4=2020d0")]).unwrap();
    let back = k().import_step(&step).unwrap();
    assert_eq!(back.len(), 1);
    let mut parts = back[0].1.split('\u{1f}');
    assert_eq!(parts.next(), Some("Plate"));
    assert_eq!(parts.next(), Some("#808080"));
    let faces: Vec<(usize, &str)> = parts
        .next()
        .unwrap_or_default()
        .split(',')
        .filter_map(|e| e.split_once('=').map(|(i, c)| (i.parse().unwrap(), c)))
        .collect();
    assert_eq!(faces.len(), 2, "{faces:?}");
    // The same faces (by place) came back in the same colours, whatever their order.
    let back_topo = k().topology(&back[0].0).unwrap();
    for (orig, colour) in [(1, "d02020"), (4, "2020d0")] {
        let c = topo.faces[orig].centroid;
        let (i, _) = faces.iter().find(|(_, col)| *col == colour).unwrap();
        let d = back_topo.faces[*i].centroid;
        assert!((0..3).all(|j| (c[j] - d[j]).abs() < 1e-6), "face {orig}: {c:?} vs {d:?}");
    }
}

#[test]
fn one_coloured_face_on_an_uncoloured_body_stays_one_face() {
    // Face colours review #1: the body does not take its only coloured face's colour.
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let step = k().export_step(&[(&a.shape, "Plate\u{1f}-\u{1f}1=d02020")]).unwrap();
    let back = k().import_step(&step).unwrap();
    let mut parts = back[0].1.split('\u{1f}');
    assert_eq!(parts.next(), Some("Plate"));
    assert_eq!(parts.next(), Some("#b4bcc8"), "{:?}", back[0].1);
    assert_eq!(parts.next().map(|f| f.split(',').count()), Some(1), "{:?}", back[0].1);
}

#[test]
fn exchange_roundtrips() {
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let step = k().export_step(&[(&a.shape, "Plate")]).unwrap();
    let text = String::from_utf8_lossy(&step);
    assert!(text.starts_with("ISO-10303-21;"));
    assert!(text.contains("Plate"));
    let back = k().import_step(&step).unwrap();
    assert_eq!(back.len(), 1);
    let m = k().mass_properties(&back[0].0).unwrap();
    assert!(close(m.volume, 8000.0, 1e-6));
    let iges = k().export_iges(&[(&a.shape, "Plate")]).unwrap();
    assert!(!iges.is_empty());
    let brep = k().write_native(&a.shape).unwrap();
    let s = k().read_native(&brep).unwrap();
    assert!(close(k().mass_properties(&s).unwrap().volume, 8000.0, 1e-9));
    // Garbage input fails cleanly.
    assert!(k().import_step(b"not a step file").is_err() || k().import_step(b"not a step file").unwrap().is_empty());
    assert!(k().read_native(b"garbage").is_err());
}

#[test]
fn rejects_bad_profiles() {
    let mut p = boxp(1.0, 1.0);
    p.frame.x = [2.0, 0.0, 0.0];
    assert!(k().extrude(&p, &ExtrudeSpec { distance: 1.0, symmetric: false }).is_err());
    let p = Profile { frame: Frame::XY, regions: vec![] };
    assert!(k().extrude(&p, &ExtrudeSpec { distance: 1.0, symmetric: false }).is_err());
    assert!(k().extrude(&boxp(1.0, 1.0), &ExtrudeSpec { distance: 0.0, symmetric: false }).is_err());
    // Open loop.
    let open = Loop2 {
        edges: vec![
            ProfileEdge { seg: Seg2::Line { a: [0.0, 0.0], b: [1.0, 0.0] }, tag: 1 },
            ProfileEdge { seg: Seg2::Line { a: [5.0, 5.0], b: [6.0, 6.0] }, tag: 2 },
        ],
    };
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: open, holes: vec![] }] };
    assert!(k().extrude(&p, &ExtrudeSpec { distance: 1.0, symmetric: false }).is_err());
}

fn plain_box(w: f64, h: f64, d: f64) -> OpResult {
    k().extrude(&boxp(w, h), &ExtrudeSpec { distance: d, symmetric: false }).unwrap()
}

#[test]
fn shell_with_no_opened_face_is_a_hollow_solid() {
    let b = plain_box(30.0, 20.0, 10.0);
    let r = k().shell(&b.shape, &[], 2.0).unwrap();
    // The body with a closed void: 30x20x10 minus the 26x16x6 cavity.
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, 6000.0 - 2496.0, 1e-9), "{}", m.volume);
    let rt = k().topology(&r.shape).unwrap();
    assert_eq!((rt.solids, rt.faces.len()), (1, 12));
    assert_eq!(r.face_origins.len(), rt.faces.len());
    let offset = r.face_origins.iter().filter(|o| matches!(o.as_slice(), [FaceOrigin::Offset { .. }])).count();
    let kept = r.face_origins.iter().filter(|o| matches!(o.as_slice(), [FaceOrigin::Input { .. }])).count();
    assert_eq!((offset, kept), (6, 6));
    // Outward: the body itself is the void of a skin 2 mm outside it.
    let g = k().shell(&b.shape, &[], -2.0).unwrap();
    let gm = k().mass_properties(&g.shape).unwrap();
    assert!(close(gm.volume, 34.0 * 24.0 * 14.0 - 6000.0, 1e-9), "{}", gm.volume);
}

#[test]
fn shell_box_golden() {
    let b = plain_box(40.0, 20.0, 10.0);
    let t = k().topology(&b.shape).unwrap();
    // Remove the top cap (outward normal +Z).
    let top = t.faces.iter().position(|f| f.normal[2] > 0.9).unwrap() as u32;
    let r = k().shell(&b.shape, &[top], 2.0).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    // Outer 40x20x10 minus the cavity 36x16x8 = 8000 - 4608.
    assert!(close(m.volume, 3392.0, 1e-9), "{}", m.volume);
    let rt = k().topology(&r.shape).unwrap();
    assert_eq!((rt.solids, rt.faces.len()), (1, 11));
    assert_eq!(r.face_origins.len(), rt.faces.len());
    let inner: Vec<usize> =
        (0..rt.faces.len()).filter(|&i| matches!(r.face_origins[i].as_slice(), [FaceOrigin::Offset { .. }])).collect();
    assert_eq!(inner.len(), 5);
    // The ring left where the top face was keeps that face's identity (a 224 mm2 plane).
    let ring = r.face_origins.iter().position(|o| o == &vec![FaceOrigin::Input { input: 0, face: top }]).unwrap();
    assert!(close(rt.faces[ring].area, 40.0 * 20.0 - 36.0 * 16.0, 1e-9));
    // Every other original face survives exactly once as an outer face.
    assert!(r.face_origins.iter().all(|o| o.len() == 1));
    // Growing outward adds material: (44x24x12 minus the 40x20x10 top-open hull) volume.
    let g = k().shell(&b.shape, &[top], -2.0).unwrap();
    let gm = k().mass_properties(&g.shape).unwrap();
    assert!(close(gm.volume, 44.0 * 24.0 * 12.0 - 40.0 * 20.0 * 10.0, 1e-9), "{}", gm.volume);
    // A shell needs a real thickness and valid faces.
    assert!(k().shell(&b.shape, &[top], 0.0).is_err());
    assert!(k().shell(&b.shape, &[99], 2.0).is_err());
    // Too thick for the part.
    let thick = k().shell(&b.shape, &[top], 8.0).unwrap();
    assert!(close(k().mass_properties(&thick.shape).unwrap().volume, 8000.0 - 24.0 * 4.0 * 2.0, 1e-9));
    assert!(k().shell(&b.shape, &[top], 12.0).is_err());
}

#[test]
fn offset_faces_golden() {
    let b = plain_box(40.0, 20.0, 10.0);
    let t = k().topology(&b.shape).unwrap();
    let top = t.faces.iter().position(|f| f.normal[2] > 0.9).unwrap() as u32;
    let side = t.faces.iter().position(|f| f.normal[0] > 0.9).unwrap() as u32;
    // Top up 2 mm: 40 x 20 x 12; and down 3 mm: 40 x 20 x 7.
    let r = k().offset_faces(&b.shape, &[top], 2.0).unwrap();
    assert!(close(k().mass_properties(&r.shape).unwrap().volume, 9600.0, 1e-9));
    let rt = k().topology(&r.shape).unwrap();
    assert_eq!((rt.solids, rt.faces.len()), (1, 6));
    assert!(
        r.face_origins.iter().all(|o| matches!(o.as_slice(), [FaceOrigin::Input { input: 0, .. }])),
        "{:?}",
        r.face_origins
    );
    let r = k().offset_faces(&b.shape, &[top], -3.0).unwrap();
    assert!(close(k().mass_properties(&r.shape).unwrap().volume, 5600.0, 1e-9));
    // Two faces at once: 42 x 20 x 12.
    let r = k().offset_faces(&b.shape, &[top, side], 2.0).unwrap();
    assert!(close(k().mass_properties(&r.shape).unwrap().volume, 42.0 * 20.0 * 12.0, 1e-9));
    assert!(k().offset_faces(&b.shape, &[top], 0.0).is_err());
    assert!(k().offset_faces(&b.shape, &[99], 1.0).is_err());
    assert!(k().offset_faces(&b.shape, &[], 1.0).is_err());
}

#[test]
fn a_shafts_end_between_flats_is_pushed_out_by_its_prism() {
    // 858's shaft: a rod of radius 2.5 from z 1.6 to 4, on it the same rod with two flats 1.5
    // from its axis up to z 10, its right arc in two at the seam (as the sketch's circle drawn
    // twice leaves it). Pushing its lower end out by 0.4 mm adds the end's area times
    // 0.4 (the cylinder runs on straight); the faces' move and BRepOffset give no shape here.
    let (arc, line) = (
        |a: [f64; 2], b: [f64; 2], tag| ProfileEdge {
            seg: Seg2::Arc { center: [0.0, 0.0], start: a, end: b, ccw: true },
            tag,
        },
        |a, b, tag| ProfileEdge { seg: Seg2::Line { a, b }, tag },
    );
    let outer = Loop2 {
        edges: vec![
            arc([2.0, -1.5], [2.5, 0.0], 1),
            arc([2.5, 0.0], [2.0, 1.5], 5),
            line([2.0, 1.5], [-2.0, 1.5], 2),
            arc([-2.0, 1.5], [-2.0, -1.5], 3),
            line([-2.0, -1.5], [2.0, -1.5], 4),
        ],
    };
    let flat = Profile {
        frame: Frame { origin: [0.0, 0.0, 4.0], ..Frame::XY },
        regions: vec![Region2 { outer, holes: vec![] }],
    };
    let b = k().extrude(&flat, &ExtrudeSpec { distance: 6.0, symmetric: false }).unwrap().shape;
    let b = k().boolean(BoolOp::Union, &b, &disc(2.5, 2.4, 1.6).shape).unwrap().shape;
    let t = k().topology(&b).unwrap();
    let end = t.faces.iter().position(|f| f.normal[2] < -0.9).unwrap() as u32;
    let area = t.faces.get(end as usize).unwrap().area;
    let before = k().mass_properties(&b).unwrap().volume;
    let r = k().offset_faces(&b, &[end], 0.4).unwrap();
    assert!(close(k().mass_properties(&r.shape).unwrap().volume, before + area * 0.4, 1e-9));
    let rt = k().topology(&r.shape).unwrap();
    let moved = rt.faces.iter().position(|f| f.normal[2] < -0.9 && (f.centroid[2] - 1.2).abs() < 1e-9).unwrap();
    assert_eq!(r.face_origins.get(moved).unwrap().as_slice(), [FaceOrigin::Input { input: 0, face: end }]);
    // Every face comes from one of the input's (a piece of the cylinder the unification leaves
    // at the seam from the cylinder).
    assert!(
        r.face_origins.iter().all(|o| matches!(o.as_slice(), [FaceOrigin::Input { input: 0, .. }, ..])),
        "{:?}",
        r.face_origins
    );
    assert_eq!(r.face_origins.len(), rt.faces.len());
}

#[test]
fn a_clips_slanted_side_moves_between_its_neighbours_planes() {
    // 5998's lid clip, in short: a plate with a clip under it, reaching out past the plate's
    // edge, whose side runs at 45 degrees between two sides along y. Moving that side 0.15 in
    // takes off the parallelogram between it and its moved plane, ended by the neighbours'
    // planes: 1.6 long in x, 0.15 * sqrt 2 in y, 2 deep. BRepOffset gives back the plate
    // without the clip here (a valid solid, but not the body).
    let plate = plain_box(20.0, 16.0, 2.0);
    let p = |x: f64, y: f64| [x, y];
    let line = |a: [f64; 2], b: [f64; 2], tag| ProfileEdge { seg: Seg2::Line { a, b }, tag };
    let corners = [p(1.0, 11.774), p(2.6, 13.374), p(2.6, 14.374), p(3.687, 15.461), p(2.556, 16.593), p(1.0, 15.037)];
    let edges = (0..corners.len())
        .map(|i| line(corners[i], corners[(i + 1) % corners.len()], i as u64 + 1))
        .collect::<Vec<_>>();
    let clip = Profile {
        frame: Frame { origin: [0.0, 0.0, -2.0], ..Frame::XY },
        regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }],
    };
    let clip = k().extrude(&clip, &ExtrudeSpec { distance: 2.0, symmetric: false }).unwrap();
    let b = k().boolean(BoolOp::Union, &plate.shape, &clip.shape).unwrap().shape;
    let t = k().topology(&b).unwrap();
    let s2 = 0.5f64.sqrt();
    let side = t
        .faces
        .iter()
        .position(|f| (f.normal[0] - s2).abs() < 1e-9 && (f.normal[1] + s2).abs() < 1e-9 && f.centroid[0] < 2.0)
        .unwrap() as u32;
    let before = k().mass_properties(&b).unwrap().volume;
    let r = k().offset_faces(&b, &[side], -0.15).unwrap();
    let after = k().mass_properties(&r.shape).unwrap().volume;
    assert!(close(after, before - 1.6 * 0.15 * 2f64.sqrt() * 2.0, 1e-9), "{before} -> {after}");
    let rt = k().topology(&r.shape).unwrap();
    assert_eq!(rt.faces.len(), t.faces.len());
    let moved = rt
        .faces
        .iter()
        .position(|f| (f.normal[0] - s2).abs() < 1e-9 && (f.normal[1] + s2).abs() < 1e-9 && f.centroid[0] < 2.0)
        .unwrap();
    assert_eq!(r.face_origins.get(moved).unwrap().as_slice(), [FaceOrigin::Input { input: 0, face: side }]);
    assert!(r.face_origins.iter().all(|o| matches!(o.as_slice(), [FaceOrigin::Input { input: 0, .. }])));
}

fn disc(r: f64, h: f64, z0: f64) -> OpResult {
    let frame = Frame { origin: [0.0, 0.0, z0], ..Frame::XY };
    let outer = Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [0.0, 0.0], radius: r }, tag: 1 }] };
    let p = Profile { frame, regions: vec![Region2 { outer, holes: vec![] }] };
    k().extrude(&p, &ExtrudeSpec { distance: h, symmetric: false }).unwrap()
}

#[test]
fn a_hole_beside_slots_is_widened() {
    // A 1.2 mm deep hole of radius 1.5 under a rod of radius 4, crossed at its mouth by a slot
    // 1 mm wide and 0.5 mm deep. Offsetting the hole's wall by -0.1 widens it to 1.6; the slot's
    // walls run on to it. (BRepOffset gives no shape here; the ring between the radii is cut.)
    let rod = disc(4.0, 5.0, 0.0);
    let hole = disc(1.5, 1.2, 0.0);
    let b = k().boolean(BoolOp::Cut, &rod.shape, &hole.shape).unwrap();
    let slot = k()
        .extrude(
            &Profile {
                frame: Frame { origin: [-3.4, -0.5, 0.0], ..Frame::XY },
                regions: vec![Region2 { outer: rect(6.8, 1.0, 10), holes: vec![] }],
            },
            &ExtrudeSpec { distance: 0.5, symmetric: false },
        )
        .unwrap();
    let b = k().boolean(BoolOp::Cut, &b.shape, &slot.shape).unwrap();
    let t = k().topology(&b.shape).unwrap();
    let wall = t
        .faces
        .iter()
        .position(|f| f.kind == SurfaceKind::Cylinder && f.centroid[0].hypot(f.centroid[1]) < 1.5 && f.area < 15.0)
        .unwrap() as u32;
    let before = k().mass_properties(&b.shape).unwrap().volume;
    let r = k().offset_faces(&b.shape, &[wall], -0.1).unwrap();
    // The ring from 1.5 to 1.6 over 1.2 mm, less its two pieces in the slot (0.5 mm deep).
    let n = 20000;
    let piece: f64 = (0..n)
        .map(|i| {
            let y = -0.5 + (f64::from(i) + 0.5) / f64::from(n);
            (1.6f64 * 1.6 - y * y).sqrt() - (1.5f64 * 1.5 - y * y).sqrt()
        })
        .sum::<f64>()
        / f64::from(n);
    let ring = std::f64::consts::PI * (1.6 * 1.6 - 1.5 * 1.5);
    let removed = before - k().mass_properties(&r.shape).unwrap().volume;
    assert!(close(removed, 1.2 * ring - 0.5 * 2.0 * piece, 1e-6), "{removed}");
    assert!(r.face_origins.iter().all(|o| !o.is_empty()), "{:?}", r.face_origins);
    assert!(r.face_origins.iter().flatten().any(|o| *o == FaceOrigin::Input { input: 0, face: wall }));
}

#[test]
fn a_pocket_wall_offset_carries_its_round() {
    // A disc of radius 9 with an annular pocket from radius 4.9 to 6 and 0.4 deep, its two top
    // rims rounded by 0.2. Offsetting both walls by -0.1 widens the pocket to 4.8 .. 6.1; the
    // rounds go with the walls (the stored body of such an offset has the same rounds on the
    // moved rims).
    let base = disc(9.0, 1.5, 0.0);
    let outer = disc(6.0, 0.4, 1.1);
    let inner = disc(4.9, 0.4, 1.1);
    let ring = k().boolean(BoolOp::Cut, &outer.shape, &inner.shape).unwrap();
    let b = k().boolean(BoolOp::Cut, &base.shape, &ring.shape).unwrap();
    let t = k().topology(&b.shape).unwrap();
    let rim = |r: f64| {
        t.edges
            .iter()
            .position(|e| {
                e.circle.as_ref().is_some_and(|c| (c.radius - r).abs() < 1e-9 && (c.center[2] - 1.5).abs() < 1e-9)
            })
            .unwrap() as u32
    };
    let f = k().fillet(&b.shape, &[rim(4.9), rim(6.0)], 0.2).unwrap();
    let t = k().topology(&f.shape).unwrap();
    let wall = |r: f64| {
        t.faces
            .iter()
            .position(|x| {
                x.kind == SurfaceKind::Cylinder
                    && x.axis.as_ref().is_some_and(|a| a.origin[0].abs() < 1e-9)
                    && (x.area - std::f64::consts::TAU * r * 0.2).abs() < 1e-6
            })
            .unwrap() as u32
    };
    let walls = [wall(4.9), wall(6.0)];
    let before = k().mass_properties(&f.shape).unwrap().volume;
    let r = k().offset_faces(&f.shape, &walls, -0.1).unwrap();
    // Pappus: each wall's section grows by 0.1 x 0.2 below its round, and the round moved 0.1
    // sideways sweeps a band of 0.1 x 0.2 whose centre is 0.05 pi in from the round's centre
    // (the mean width of a quarter circle of radius 0.2 is 0.05 pi), plus half the move:
    // 6.2 - 0.05 pi + 0.05 outside, 4.7 + 0.05 pi - 0.05 inside; the two sum to 10.9.
    let removed = before - k().mass_properties(&r.shape).unwrap().volume;
    let want = std::f64::consts::TAU * 0.02 * (6.05 + 4.85 + 10.9);
    assert!(close(removed, want, 1e-6), "{removed} vs {want}");
    assert!(r.face_origins.iter().all(|o| !o.is_empty()), "{:?}", r.face_origins);
    let rt = k().topology(&r.shape).unwrap();
    assert_eq!(rt.faces.iter().filter(|x| x.kind == SurfaceKind::Torus).count(), 2);
}

/// 5998's `Chamfer3`: a plate 1.6 thick with a slot 3 wide (straight sides 8 long, round ends)
/// pocketed 0.6 deep, its rim chamfered by 0.6, which uses up the slot's sides (ChFi3d does
/// not end a chamfer on the far edge of the face it uses up). The chamfer is the cut of the
/// slot tapered at 45 degrees: it removes the ring between the rim and its offset by t over
/// the depth, P d^2 / 2 + pi d^3 / 3 (P the rim's length, the round ends' offset growing by
/// pi t^2). Each rim edge names its chamfer face, a plane or a cone.
#[test]
fn a_chamfer_that_uses_up_a_pockets_sides_is_its_rim_tapered() {
    let plate = plain_box(20.0, 20.0, 1.6);
    let side = |x: f64, y0: f64, y1: f64| Seg2::Line { a: [x, y0], b: [x, y1] };
    let end = |y: f64, from: f64, to: f64| Seg2::Arc { center: [10.0, y], start: [from, y], end: [to, y], ccw: true };
    let slot = Loop2 {
        edges: [side(11.5, 6.0, 14.0), end(14.0, 11.5, 8.5), side(8.5, 14.0, 6.0), end(6.0, 8.5, 11.5)]
            .into_iter()
            .enumerate()
            .map(|(i, seg)| ProfileEdge { seg, tag: 50 + i as u64 })
            .collect(),
    };
    let frame = Frame { origin: [0.0, 0.0, 1.0], ..Frame::XY };
    let p = Profile { frame, regions: vec![Region2 { outer: slot, holes: vec![] }] };
    let pocket = k().extrude(&p, &ExtrudeSpec { distance: 0.6, symmetric: false }).unwrap();
    let b = k().boolean(BoolOp::Cut, &plate.shape, &pocket.shape).unwrap();
    let t = k().topology(&b.shape).unwrap();
    let rim: Vec<u32> = (0..t.edges.len() as u32)
        .filter(|i| {
            let m = t.edges[*i as usize].midpoint;
            (m[2] - 1.6).abs() < 1e-9 && (m[0] - 10.0).abs() < 2.0 && (m[1] - 10.0).abs() < 6.0
        })
        .collect();
    assert_eq!(rim.len(), 4);
    let before = k().mass_properties(&b.shape).unwrap().volume;
    let c = k().chamfer(&b.shape, &rim, 0.6).unwrap();
    let removed = before - k().mass_properties(&c.shape).unwrap().volume;
    let (d, perimeter) = (0.6f64, 16.0 + 3.0 * std::f64::consts::PI);
    let want = perimeter * d * d / 2.0 + std::f64::consts::PI * d.powi(3) / 3.0;
    assert!(close(removed, want, 1e-9), "{removed} vs {want}");
    assert!(c.face_origins.iter().all(|o| !o.is_empty()));
    let ct = k().topology(&c.shape).unwrap();
    for e in &rim {
        let faces: Vec<usize> = (0..c.face_origins.len())
            .filter(|f| c.face_origins[*f].contains(&FaceOrigin::FromEdge { input: 0, edge: *e }))
            .collect();
        assert_eq!(faces.len(), 1, "edge {e}");
        assert!(matches!(ct.faces[faces[0]].kind, SurfaceKind::Plane | SurfaceKind::Cone), "edge {e}");
    }
}

/// 5998's `Fillet5`: a slot 0.4 wide and 1.6 deep, turned 20 degrees, its two inner corners
/// at one end rounded by 0.2, which uses up the end (a full round). Fusion stores the angles to
/// 7 digits, so the end stands 1e-7 rad off square to the sides; the rounds are still the
/// corners filled, r^2 (1 - pi / 4) each along the depth.
#[test]
fn rounds_of_a_slots_end_a_hair_off_square_fill_its_corners() {
    let plate = plain_box(10.0, 10.0, 2.0);
    let (s, c) = (libm::sin(20f64.to_radians()), libm::cos(20f64.to_radians()));
    let (u, v) = ([c, s], [-s, c]);
    let tilt = 1e-7;
    let w = [v[0] * libm::cos(tilt) - v[1] * libm::sin(tilt), v[0] * libm::sin(tilt) + v[1] * libm::cos(tilt)];
    let at = |a: f64, b: f64| [5.0 + a * u[0] + b * v[0], 5.0 + a * u[1] + b * v[1]];
    let p1 = at(1.5, -0.2);
    let p2 = [p1[0] + 0.4 * w[0], p1[1] + 0.4 * w[1]];
    let pts = [at(-1.5, -0.2), p1, p2, at(-1.5, 0.2)];
    let slot = Loop2 {
        edges: (0..4)
            .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: 60 + i as u64 })
            .collect(),
    };
    let frame = Frame { origin: [0.0, 0.0, 0.4], ..Frame::XY };
    let p = Profile { frame, regions: vec![Region2 { outer: slot, holes: vec![] }] };
    let pocket = k().extrude(&p, &ExtrudeSpec { distance: 1.6, symmetric: false }).unwrap();
    let b = k().boolean(BoolOp::Cut, &plate.shape, &pocket.shape).unwrap();
    let t = k().topology(&b.shape).unwrap();
    let corner = |q: [f64; 2]| {
        t.edges.iter().position(|e| (e.midpoint[0] - q[0]).abs() < 1e-6 && (e.midpoint[1] - q[1]).abs() < 1e-6).unwrap()
            as u32
    };
    let edges = [corner(p1), corner(p2)];
    let before = k().mass_properties(&b.shape).unwrap().volume;
    let f = k().fillet(&b.shape, &edges, 0.2).unwrap();
    let added = k().mass_properties(&f.shape).unwrap().volume - before;
    let want = 2.0 * 0.04 * (1.0 - std::f64::consts::FRAC_PI_4) * 1.6;
    assert!(close(added, want, 1e-6), "{added} vs {want}");
    for e in edges {
        assert!(f.face_origins.iter().any(|o| o.contains(&FaceOrigin::FromEdge { input: 0, edge: e })), "edge {e}");
    }
}

fn plate_with_hole(cx: f64, cy: f64) -> OpResult {
    let mut region = Region2 { outer: rect(40.0, 40.0, 1), holes: vec![] };
    region
        .holes
        .push(Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [cx, cy], radius: 5.0 }, tag: 99 }] });
    let p = Profile { frame: Frame::XY, regions: vec![region] };
    k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap()
}

fn hole_axis(r: &OpResult) -> Axis3 {
    let t = k().topology(&r.shape).unwrap();
    t.faces.iter().find(|f| f.kind == SurfaceKind::Cylinder).unwrap().axis.unwrap()
}

#[test]
fn move_faces_golden() {
    let plate = plate_with_hole(20.0, 20.0);
    let t = k().topology(&plate.shape).unwrap();
    let hole = t.faces.iter().position(|f| f.kind == SurfaceKind::Cylinder).unwrap() as u32;
    let volume = (1600.0 - std::f64::consts::PI * 25.0) * 10.0;
    // The hole slides along the plate: same volume, axis 8 mm further along X.
    let r = k().move_faces(&plate.shape, &[hole], &[Transform::Translate([8.0, 0.0, 0.0])]).unwrap();
    assert!(close(k().mass_properties(&r.shape).unwrap().volume, volume, 1e-6));
    let a = hole_axis(&r);
    assert!(close(a.origin[0], 28.0, 1e-9) && close(a.origin[1], 20.0, 1e-9), "{a:?}");
    let rt = k().topology(&r.shape).unwrap();
    assert_eq!((rt.solids, rt.faces.len()), (1, t.faces.len()));
    assert!(
        r.face_origins.iter().all(|o| matches!(o.as_slice(), [FaceOrigin::Input { input: 0, .. }])),
        "{:?}",
        r.face_origins
    );
    // A quarter turn about a vertical axis through (10, 20), then 2 mm along Y: (10, 32).
    let steps = [
        Transform::Rotate { origin: [10.0, 20.0, 0.0], axis: [0.0, 0.0, 1.0], angle: std::f64::consts::FRAC_PI_2 },
        Transform::Translate([0.0, 2.0, 0.0]),
    ];
    let r = k().move_faces(&plate.shape, &[hole], &steps).unwrap();
    let a = hole_axis(&r);
    assert!(close(a.origin[0], 10.0, 1e-9) && close(a.origin[1], 32.0, 1e-9), "{a:?}");
    // A side face along its normal: the faces around stretch (43 x 20 x 10).
    let b = plain_box(40.0, 20.0, 10.0);
    let bt = k().topology(&b.shape).unwrap();
    let side = bt.faces.iter().position(|f| f.normal[0] > 0.9).unwrap() as u32;
    let top = bt.faces.iter().position(|f| f.normal[2] > 0.9).unwrap() as u32;
    let r = k().move_faces(&b.shape, &[side], &[Transform::Translate([3.0, 0.0, 0.0])]).unwrap();
    assert!(close(k().mass_properties(&r.shape).unwrap().volume, 43.0 * 20.0 * 10.0, 1e-9));
    // Refused: the side faces would have to tilt; a hole through the plate's edge; no move.
    let e = k().move_faces(&b.shape, &[top], &[Transform::Translate([5.0, 0.0, 0.0])]).unwrap_err();
    assert!(e.message.contains("change shape"), "{e:?}");
    assert!(k().move_faces(&plate.shape, &[hole], &[Transform::Translate([18.0, 0.0, 0.0])]).is_err());
    assert!(k().move_faces(&plate.shape, &[hole], &[Transform::Translate([0.0, 0.0, 0.0])]).is_err());
    assert!(k().move_faces(&plate.shape, &[hole], &[]).is_err());
    assert!(k().move_faces(&plate.shape, &[99], &[Transform::Translate([1.0, 0.0, 0.0])]).is_err());
}

#[test]
fn draft_box_golden() {
    let b = plain_box(40.0, 20.0, 10.0);
    let t = k().topology(&b.shape).unwrap();
    let sides: Vec<u32> = (0..t.faces.len() as u32).filter(|&i| t.faces[i as usize].normal[2].abs() < 0.1).collect();
    assert_eq!(sides.len(), 4);
    let bottom = t.faces.iter().map(|f| f.centroid[2]).fold(f64::INFINITY, f64::min);
    let spec = |angle: f64| DraftSpec { pull: [0.0, 0.0, 1.0], neutral: [0.0, 0.0, bottom], angle };
    // 5 degrees inward along +Z about the bottom: a frustum, prismatoid volume
    // h/6 (A_bottom + A_top + 4 A_middle).
    let a = 5f64.to_radians();
    let r = k().draft(&b.shape, &sides, &spec(a)).unwrap();
    let off = 10.0 * libm::tan(a);
    let rect = |s: f64| (40.0 - 2.0 * s) * (20.0 - 2.0 * s);
    let want = 10.0 / 6.0 * (rect(0.0) + rect(off) + 4.0 * rect(off / 2.0));
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, want, 1e-8), "{} vs {want}", m.volume);
    // Every face is an input face, once.
    let rt = k().topology(&r.shape).unwrap();
    assert_eq!((rt.solids, rt.faces.len()), (1, 6));
    assert!(
        r.face_origins.iter().all(|o| matches!(o.as_slice(), [FaceOrigin::Input { input: 0, .. }])),
        "{:?}",
        r.face_origins
    );
    // Negative angles widen it; bad input is refused.
    let g = k().draft(&b.shape, &sides, &spec(-a)).unwrap();
    assert!(k().mass_properties(&g.shape).unwrap().volume > 8000.0);
    assert!(k().draft(&b.shape, &sides, &spec(0.0)).is_err());
    assert!(k().draft(&b.shape, &sides, &spec(1.6)).is_err());
    assert!(k().draft(&b.shape, &[99], &spec(a)).is_err());
    assert!(k().draft(&b.shape, &[], &spec(a)).is_err());
}

#[test]
fn transform_golden() {
    let b = plain_box(40.0, 20.0, 10.0);
    let tb = k().topology(&b.shape).unwrap();
    let mir = k().transform(&b.shape, &Transform::Mirror { origin: [0.0; 3], normal: [1.0, 0.0, 0.0] }).unwrap();
    let m = k().mass_properties(&mir.shape).unwrap();
    assert!(close(m.volume, 8000.0, 1e-9), "{}", m.volume);
    assert!(close(m.centroid[0], -20.0, 1e-9));
    let mt = k().topology(&mir.shape).unwrap();
    assert_eq!(mt.faces.len(), 6);
    for (i, o) in mir.face_origins.iter().enumerate() {
        let &[FaceOrigin::Input { input: 0, face }] = o.as_slice() else { panic!("{o:?}") };
        let (a, c) = (&tb.faces[face as usize], &mt.faces[i]);
        assert!(close(c.normal[0], -a.normal[0], 1e-9) && close(c.normal[2], a.normal[2], 1e-9), "{i}");
        assert!(close(c.centroid[0], -a.centroid[0], 1e-9));
    }
    let tr = k().transform(&b.shape, &Transform::Translate([100.0, 0.0, 0.0])).unwrap();
    let bb = k().bbox(&tr.shape).unwrap();
    assert!(close(bb.min[0], 100.0, 1e-6));
    let rot = k()
        .transform(
            &b.shape,
            &Transform::Rotate { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], angle: std::f64::consts::PI },
        )
        .unwrap();
    let rm = k().mass_properties(&rot.shape).unwrap();
    assert!(close(rm.centroid[0], -20.0, 1e-9) && close(rm.centroid[1], -10.0, 1e-9));
}

#[test]
fn scale_golden() {
    let k = k();
    let pi = std::f64::consts::PI;
    // Uniform: volume * f^3, the box keeps its planes, the centre of scaling stays put.
    let b = plain_box(40.0, 20.0, 10.0);
    let sc = k.transform(&b.shape, &Transform::Scale { center: [0.0; 3], factors: [2.0; 3] }).unwrap();
    let m = k.mass_properties(&sc.shape).unwrap();
    assert!(close(m.volume, 8000.0 * 8.0, 1e-9), "{}", m.volume);
    assert!(close(m.centroid[0], 40.0, 1e-9), "{:?}", m.centroid);
    let t = k.topology(&sc.shape).unwrap();
    assert_eq!(t.faces.len(), 6);
    assert!(t.faces.iter().all(|f| f.kind == SurfaceKind::Plane));
    for (i, o) in sc.face_origins.iter().enumerate() {
        assert!(matches!(o.as_slice(), [FaceOrigin::Input { input: 0, face }] if *face as usize == i), "{o:?}");
    }
    // About the box centre: the centroid does not move.
    let c = k.transform(&b.shape, &Transform::Scale { center: [20.0, 10.0, 5.0], factors: [0.5; 3] }).unwrap();
    let mc = k.mass_properties(&c.shape).unwrap();
    assert!(close(mc.volume, 1000.0, 1e-9) && close(mc.centroid[0], 20.0, 1e-9), "{mc:?}");
    // A cylinder stays a cylinder under a uniform scale.
    let circle = Profile {
        frame: Frame::XY,
        regions: vec![Region2 {
            outer: Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [0.0, 0.0], radius: 5.0 }, tag: 1 }] },
            holes: vec![],
        }],
    };
    let cyl = k.extrude(&circle, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let cs = k.transform(&cyl.shape, &Transform::Scale { center: [0.0; 3], factors: [1.5; 3] }).unwrap();
    assert!(close(k.mass_properties(&cs.shape).unwrap().volume, pi * 250.0 * 3.375, 1e-9));
    assert!(k.topology(&cs.shape).unwrap().faces.iter().any(|f| f.kind == SurfaceKind::Cylinder));
    // Per axis: volume * fx * fy * fz, also for a curved body; faces map one to one.
    let g = k.transform(&b.shape, &Transform::Scale { center: [0.0; 3], factors: [2.0, 0.5, 3.0] }).unwrap();
    assert!(close(k.mass_properties(&g.shape).unwrap().volume, 8000.0 * 3.0, 1e-9));
    assert_eq!(g.face_origins.len(), 6);
    let gc = k.transform(&cyl.shape, &Transform::Scale { center: [0.0; 3], factors: [2.0, 1.0, 0.5] }).unwrap();
    let v = k.mass_properties(&gc.shape).unwrap().volume;
    // The ellipse is a B-spline surface now: the volume is integrated numerically.
    assert!(close(v, pi * 250.0, 1e-5), "{v}");
    for bad in [[0.0, 1.0, 1.0], [-1.0, 1.0, 1.0], [1.0, f64::NAN, 1.0]] {
        assert!(k.transform(&b.shape, &Transform::Scale { center: [0.0; 3], factors: bad }).is_err());
    }
}

/// A closed Bézier chain through `n` points of a circle of radius `r` (Catmull-Rom tangents).
fn circle_chain(n: usize, r: f64) -> Vec<[f64; 2]> {
    let pt = |i: usize| {
        let t = std::f64::consts::TAU * (i % n) as f64 / n as f64;
        [r * t.cos(), r * t.sin()]
    };
    let mut poles = vec![pt(0)];
    for i in 0..n {
        let (p0, p1, p2, p3) = (pt(i + n - 1), pt(i), pt(i + 1), pt(i + 2));
        // Uniform Catmull-Rom (equal spacing): tangents (p2 - p0) / 2 and (p3 - p1) / 2.
        let b1 = [p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0];
        let b2 = [p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0];
        poles.extend([b1, b2, p2]);
    }
    poles
}

fn bezier_area(poles: &[[f64; 2]]) -> f64 {
    let mut pts = Vec::new();
    for seg in poles.windows(4).step_by(3) {
        for k in 0..4000 {
            let t = f64::from(k) / 4000.0;
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

/// Signed volume of a closed triangle mesh.
fn mesh_volume(m: &Mesh) -> f64 {
    let p = |i: u32| m.positions[i as usize].map(f64::from);
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

#[test]
fn closed_spline_extrusion_has_the_volume_of_its_curve() {
    for n in [6usize, 12] {
        let poles = circle_chain(n, 10.0);
        assert_eq!(poles.len(), 3 * n + 1);
        // One profile edge per Bézier segment, each with its own tag.
        let edges = poles
            .windows(4)
            .step_by(3)
            .enumerate()
            .map(|(i, w)| ProfileEdge { seg: Seg2::Bezier { p: [w[0], w[1], w[2], w[3]] }, tag: 100 + i as u64 })
            .collect();
        let profile = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
        let r = k().extrude(&profile, &ExtrudeSpec { distance: 7.0, symmetric: false }).unwrap();
        let t = k().topology(&r.shape).unwrap();
        assert_eq!((t.faces.len(), t.solids), (n + 2, 1), "n={n}");
        let lat: Vec<u64> = {
            let mut v: Vec<u64> = r
                .face_origins
                .iter()
                .flatten()
                .filter_map(|o| if let FaceOrigin::Lateral { tag } = o { Some(*tag) } else { None })
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(lat, (100..100 + n as u64).collect::<Vec<_>>());
        let want = bezier_area(&poles) * 7.0;
        let m = k().mass_properties(&r.shape).unwrap();
        let mesh = k()
            .tessellate(&r.shape, &TessellationParams { linear_deflection: 0.001, angular_deflection: 0.05 })
            .unwrap();
        eprintln!(
            "spline prism n={n}: mass {:.6} mm3, mesh {:.6}, polygon estimate {want:.6}",
            m.volume,
            mesh_volume(&mesh)
        );
        assert!(close(m.volume, want, 1e-6), "n={n}: {} vs {want}", m.volume);
        // An independent check: the tessellated volume agrees to the tessellation accuracy.
        assert!(close(mesh_volume(&mesh), want, 2e-4), "n={n}: mesh {} vs {want}", mesh_volume(&mesh));
        // Centroid on the axis of this symmetric prism, half way up.
        assert!(m.centroid[0].abs() < 1e-3 && m.centroid[1].abs() < 1e-3 && close(m.centroid[2], 3.5, 1e-6));
    }
}

fn line_path(from: [f64; 2], to: [f64; 2]) -> SweepPath {
    SweepPath::Curves(Path2 {
        frame: Frame::XZ,
        edges: vec![ProfileEdge { seg: Seg2::Line { a: from, b: to }, tag: 0 }],
    })
}

fn centred_square(h: f64, frame: Frame) -> Profile {
    let pts = [[-h, -h], [h, -h], [h, h], [-h, h]];
    let edges = (0..4)
        .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 4] }, tag: 20 + i as u64 })
        .collect();
    Profile { frame, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] }
}

#[test]
fn sweep_golden_volumes_history_and_errors() {
    let frenet = SweepSpec { orientation: SweepOrientation::Frenet, twist: 0.0 };
    let r = k().sweep(&centred_square(5.0, Frame::XY), &line_path([0.0, 0.0], [0.0, 20.0]), &frenet).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, 2000.0, 1e-9), "{}", m.volume);
    let t = k().topology(&r.shape).unwrap();
    assert_eq!((t.faces.len(), t.solids), (6, 1));
    // Four swept sides (path edge 0) and the two caps.
    let mut sides: Vec<(u64, u32)> = r
        .face_origins
        .iter()
        .flatten()
        .filter_map(|o| if let FaceOrigin::Swept { tag, path } = o { Some((*tag, *path)) } else { None })
        .collect();
    sides.sort_unstable();
    assert_eq!(sides, vec![(20, 0), (21, 0), (22, 0), (23, 0)]);
    assert_eq!(r.face_origins.iter().flatten().filter(|o| matches!(o, FaceOrigin::Cap { .. })).count(), 2);
    // The path may be drawn from the far end: it is turned around.
    let r2 = k().sweep(&centred_square(5.0, Frame::XY), &line_path([0.0, 20.0], [0.0, 0.0]), &frenet).unwrap();
    assert!(close(k().mass_properties(&r2.shape).unwrap().volume, 2000.0, 1e-9));
    let bb = k().bbox(&r2.shape).unwrap();
    assert!(bb.min[2].abs() < 1e-6 && close(bb.max[2], 20.0, 1e-6));
    // A twist of a quarter turn keeps the volume (adaptive integration for freeform faces).
    let twisted = SweepSpec { orientation: SweepOrientation::Frenet, twist: std::f64::consts::FRAC_PI_2 };
    let r3 = k().sweep(&centred_square(5.0, Frame::XY), &line_path([0.0, 0.0], [0.0, 20.0]), &twisted).unwrap();
    assert!(close(k().mass_properties(&r3.shape).unwrap().volume, 2000.0, 1e-6));
    // Errors: a path away from the profile plane, an empty path, a twist with fixed orientation.
    let e = k().sweep(&centred_square(5.0, Frame::XY), &line_path([0.0, 4.0], [0.0, 24.0]), &frenet).unwrap_err();
    assert!(e.message.contains("profile plane") && e.hint.is_some(), "{e}");
    let empty = SweepPath::Curves(Path2 { frame: Frame::XZ, edges: vec![] });
    assert_eq!(
        k().sweep(&centred_square(5.0, Frame::XY), &empty, &frenet).unwrap_err().kind,
        KernelErrorKind::InvalidInput
    );
    let bad = SweepSpec { orientation: SweepOrientation::Fixed, twist: 0.3 };
    let e = k().sweep(&centred_square(5.0, Frame::XY), &line_path([0.0, 0.0], [0.0, 20.0]), &bad).unwrap_err();
    assert!(e.hint.is_some(), "{e}");
    let nan = SweepSpec { orientation: SweepOrientation::Frenet, twist: f64::NAN };
    assert!(k().sweep(&centred_square(5.0, Frame::XY), &line_path([0.0, 0.0], [0.0, 20.0]), &nan).is_err());
    // A path of disconnected pieces is an error, not a crash.
    let gap = SweepPath::Curves(Path2 {
        frame: Frame::XZ,
        edges: vec![
            ProfileEdge { seg: Seg2::Line { a: [0.0, 0.0], b: [0.0, 5.0] }, tag: 0 },
            ProfileEdge { seg: Seg2::Line { a: [3.0, 9.0], b: [3.0, 20.0] }, tag: 1 },
        ],
    });
    let e = k().sweep(&centred_square(5.0, Frame::XY), &gap, &frenet).unwrap_err();
    assert!(e.message.contains("connected") || e.message.contains("chain"), "{e}");
}

#[test]
fn loft_golden_volumes_history_and_errors() {
    let up = Frame { origin: [0.0, 0.0, 15.0], ..Frame::XY };
    let spec = LoftSpec { ruled: true };
    let r = k().loft(&[centred_square(10.0, Frame::XY), centred_square(5.0, up)], &spec).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, 3500.0, 1e-9), "{}", m.volume);
    let t = k().topology(&r.shape).unwrap();
    assert_eq!((t.faces.len(), t.solids), (6, 1));
    let sections: Vec<(u32, u64)> = {
        let mut v: Vec<(u32, u64)> = r
            .face_origins
            .iter()
            .flatten()
            .filter_map(|o| if let FaceOrigin::Section { section, tag } = o { Some((*section, *tag)) } else { None })
            .collect();
        v.sort_unstable();
        v
    };
    assert!(sections.contains(&(0, 20)) && sections.contains(&(0, 23)), "{sections:?}");
    // Needs two profiles; holes are refused; the same plane twice fails cleanly.
    assert!(k().loft(&[centred_square(5.0, up)], &spec).is_err());
    let mut holed = centred_square(8.0, up);
    holed.regions[0].holes.push(rect(2.0, 2.0, 50));
    let e = k().loft(&[centred_square(10.0, Frame::XY), holed], &spec).unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::InvalidInput);
    assert!(k().loft(&[centred_square(10.0, Frame::XY), centred_square(5.0, Frame::XY)], &spec).is_err());
}

#[test]
fn topology_reports_cylinder_axes_and_circles() {
    let k = OcctKernel::new().unwrap();
    let circle = Profile {
        frame: Frame::XY,
        regions: vec![Region2 {
            outer: Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [3.0, 4.0], radius: 2.5 }, tag: 1 }] },
            holes: vec![],
        }],
    };
    let r = k.extrude(&circle, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let t = k.topology(&r.shape).unwrap();
    let side = t.faces.iter().find(|f| f.kind == SurfaceKind::Cylinder).unwrap();
    let axis = side.axis.unwrap();
    assert!((axis.origin[0] - 3.0).abs() < 1e-9 && (axis.origin[1] - 4.0).abs() < 1e-9, "{axis:?}");
    assert!((axis.dir[2].abs() - 1.0).abs() < 1e-12, "{axis:?}");
    assert!(t.faces.iter().filter(|f| f.kind == SurfaceKind::Plane).all(|f| f.axis.is_none()));
    let circles: Vec<_> = t.edges.iter().filter_map(|e| e.circle).collect();
    assert_eq!(circles.len(), 2, "top and bottom rims");
    for c in circles {
        assert!(
            (c.radius - 2.5).abs() < 1e-9
                && (c.center[0] - 3.0).abs() < 1e-9
                && (c.normal[2].abs() - 1.0).abs() < 1e-12
        );
        assert!(c.center[2].abs() < 1e-9 || (c.center[2] - 10.0).abs() < 1e-9, "{c:?}");
    }
    assert!(t.edges.iter().filter(|e| e.kind == CurveKind::Line).all(|e| e.circle.is_none()));
}

#[test]
fn hidden_lines_of_a_plate_with_a_hole() {
    // ADR 0032: a 40 x 40 x 10 plate with a 5 mm hole at (20, 20).
    let r = plate_with_hole(20.0, 20.0);
    let span = |c: &[HlrCurve]| {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        let mut add = |p: [f64; 2]| {
            b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
        };
        for c in c {
            match &c.curve {
                ProjCurve::Line(a, z) => {
                    add(*a);
                    add(*z);
                }
                ProjCurve::Arc { center, radius, .. } => {
                    add([center[0] - radius, center[1] - radius]);
                    add([center[0] + radius, center[1] + radius]);
                }
                ProjCurve::Polyline(p) => p.iter().for_each(|q| add(*q)),
            }
        }
        b
    };
    for exact in [true, false] {
        // Top: the square and the hole's circle, seen from +Z.
        let top = HlrView { toward: [0.0, 0.0, 1.0], x: [1.0, 0.0, 0.0], exact, deflection: 0.01 };
        let c = k().hidden_lines(&r.shape, &top).unwrap();
        let vis: Vec<HlrCurve> = c.iter().filter(|c| c.visible).cloned().collect();
        let b = span(&vis);
        assert!(b.iter().zip([0.0, 0.0, 40.0, 40.0]).all(|(a, w)| close(*a, w, 1e-6)), "{exact}: {b:?}");
        if exact {
            let arc = vis
                .iter()
                .find(|c| {
                    matches!(&c.curve, ProjCurve::Arc { center, radius, .. }
                    if close(*radius, 5.0, 1e-9) && close(center[0], 20.0, 1e-9) && close(center[1], 20.0, 1e-9))
                })
                .unwrap_or_else(|| panic!("{vis:?}"));
            // It knows the model edge it shows: the hole's top circle (z = 10).
            let t = k().topology(&r.shape).unwrap();
            let e = &t.edges[arc.source.expect("a source edge") as usize];
            let c = e.circle.expect("a circle edge");
            assert!(close(c.radius, 5.0, 1e-9) && close(c.center[2], 10.0, 1e-9), "{e:?}");
            // Every sharp visible line of the square has a source edge too.
            assert!(
                vis.iter().filter(|c| matches!(c.curve, ProjCurve::Line(..))).all(|c| c.source.is_some()),
                "{vis:?}"
            );
        }
        // Front (from -Y, Z up): a 40 x 10 outline; the bore shows as two hidden lines.
        let front = HlrView { toward: [0.0, -1.0, 0.0], x: [1.0, 0.0, 0.0], exact, deflection: 0.01 };
        let c = k().hidden_lines(&r.shape, &front).unwrap();
        let vis: Vec<HlrCurve> = c.iter().filter(|c| c.visible).cloned().collect();
        let b = span(&vis);
        assert!(b.iter().zip([0.0, 0.0, 40.0, 10.0]).all(|(a, w)| close(*a, w, 1e-3)), "{exact}: {b:?}");
        let hidden: Vec<f64> = c
            .iter()
            .filter(|c| !c.visible)
            .filter_map(|c| match &c.curve {
                ProjCurve::Line(a, z) if close(a[0], z[0], 1e-6) => Some(a[0]),
                _ => None,
            })
            .collect();
        assert!(
            hidden.iter().any(|x| close(*x, 15.0, 1e-3)) && hidden.iter().any(|x| close(*x, 25.0, 1e-3)),
            "{exact}: {c:?}"
        );
    }
    let bad = HlrView { toward: [1.0, 0.0, 0.0], x: [2.0, 0.0, 0.0], exact: true, deflection: 0.01 };
    assert!(k().hidden_lines(&r.shape, &bad).is_err());
}

#[test]
fn a_helical_sweep_of_an_axial_section_has_the_screw_volume() {
    // ADR 0034: a section in a plane through the axis, screwed about it, sweeps
    // area * 2 pi * centroid radius per turn, whatever the pitch.
    let frame = Frame { origin: [5.0, 0.0, 0.0], ..Frame::XZ };
    let spec = SweepSpec { orientation: SweepOrientation::Frenet, twist: 0.0 };
    for pitch in [2.0, -2.0] {
        let path = SweepPath::Helix {
            origin: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            x: [1.0, 0.0, 0.0],
            radius: 5.0,
            pitch,
            height: 10.0,
        };
        let t0 = std::time::Instant::now();
        let r = k().sweep(&centred_square(0.25, frame), &path, &spec).unwrap();
        let m = k().mass_properties(&r.shape).unwrap();
        let want = 0.25 * 5.0 * std::f64::consts::TAU * 5.0;
        // OCCT approximates the swept surfaces (2e-6 measured).
        assert!(close(m.volume, want, 1e-5), "{pitch}: {} vs {want}", m.volume);
        let b = k().bbox(&r.shape).unwrap();
        assert!((b.max[2] - b.min[2] - 10.5).abs() < 1e-3, "{b:?}");
        eprintln!("helix pitch {pitch}: {:?}", t0.elapsed());
    }
    // A twist on a helix is refused.
    let path = SweepPath::Helix {
        origin: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        x: [1.0, 0.0, 0.0],
        radius: 5.0,
        pitch: 2.0,
        height: 10.0,
    };
    let twisted = SweepSpec { orientation: SweepOrientation::Frenet, twist: 1.0 };
    assert!(k().sweep(&centred_square(0.25, frame), &path, &twisted).is_err());
}

/// Parasolid XT (ADR 0035): the NIST samples, converted to STEP by gusset-xt, import as
/// one solid each with as many faces as the STEP files NIST published next to them, inside
/// the extent of those files' points (inches there). Runs when `GUSSET_XT_SAMPLES` names
/// usnistgov/engineering-design-models/models. (OCCT does not read NIST's own STEP files.)
#[test]
fn xt_samples_match_their_step_exports() {
    let Ok(dir) = std::env::var("GUSSET_XT_SAMPLES") else { return };
    let dir = std::path::Path::new(&dir);
    for (stem, solids) in [("Pete_Brown/foo/foo", 1), ("Pete_Brown/flange/flange", 2)] {
        let base = dir.join("Parasolid/NIST").join(stem);
        let xt = std::fs::read(base.with_extension("xmt_txt")).unwrap();
        let c = gusset_xt::to_step(&xt, "sample", &gusset_xt::Limits::default()).unwrap();
        let ours = k().import_step(c.step.as_bytes()).unwrap();
        assert_eq!(ours.len(), 1, "{stem}");
        let shape = &ours[0].0;
        let theirs = std::fs::read_to_string(base.with_extension("step")).unwrap();
        let faces = theirs.matches("ADVANCED_FACE").count() / solids;
        let topo = k().topology(shape).unwrap();
        assert_eq!((topo.solids, topo.faces.len()), (1, faces), "{stem}");
        // Every vertex of their file (inches there) is a vertex of ours.
        let mut points = std::collections::HashMap::new();
        let mut vertex_refs = Vec::new();
        for ent in theirs.split(';') {
            let line = ent.replace('\n', " ");
            let Some((id, rest)) = line.split_once('=') else { continue };
            let id = id.trim().to_string();
            if rest.contains("CARTESIAN_POINT") {
                let inner = rest.rsplit('(').next().unwrap_or("");
                let nums: Vec<f64> = inner
                    .trim_end_matches(&[')', ';', ' '][..])
                    .split(',')
                    .filter_map(|t| t.trim().parse().ok())
                    .collect();
                if let [x, y, z] = nums[..] {
                    points.insert(id, [x * 25.4, y * 25.4, z * 25.4]);
                }
            } else if rest.contains("VERTEX_POINT") {
                vertex_refs
                    .push(rest.rsplit('#').next().unwrap_or("").trim_end_matches(&[')', ';', ' '][..]).to_string());
            }
        }
        let mine: Vec<V3> = topo.vertices.iter().map(|v| v.point).collect();
        let mut matched = 0;
        for r in &vertex_refs {
            let p = points[&format!("#{r}")];
            if mine.iter().any(|q| (0..3).all(|i| (q[i] - p[i]).abs() < 1e-3)) {
                matched += 1;
            }
        }
        let m = k().mass_properties(shape).unwrap();
        eprintln!("{stem}: {matched}/{} vertices, volume {}", vertex_refs.len() / solids, m.volume);
        assert!(m.volume > 0.0);
        assert_eq!(matched, vertex_refs.len(), "{stem}");
    }
}

/// Surfaces (ADR 0036): sheets from open curves and regions, thickened and stitched.
#[test]
fn surfaces_extrude_patch_thicken_and_stitch() {
    // An open L of two lines (30 + 20 mm) swept 10 mm: a sheet of 500 mm^2, two faces.
    let l = Loop2 {
        edges: vec![
            ProfileEdge { seg: Seg2::Line { a: [0.0, 0.0], b: [30.0, 0.0] }, tag: 1 },
            ProfileEdge { seg: Seg2::Line { a: [30.0, 0.0], b: [30.0, 20.0] }, tag: 2 },
        ],
    };
    let curves = Profile { frame: Frame::XY, regions: vec![Region2 { outer: l, holes: vec![] }] };
    let wall = k().extrude_curves(&curves, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let m = k().mass_properties(&wall.shape).unwrap();
    assert!(close(m.area, 500.0, 1e-6), "{}", m.area);
    assert_eq!(k().topology(&wall.shape).unwrap().solids, 0);
    assert!(wall.face_origins.iter().flatten().any(|o| *o == FaceOrigin::Lateral { tag: 2 }));
    // A 20 x 10 patch, thickened 2 mm: 400 mm^3; both sides gives the same volume.
    let lid = k().patch(&boxp(20.0, 10.0)).unwrap();
    assert!(close(k().mass_properties(&lid.shape).unwrap().area, 200.0, 1e-9));
    for both in [false, true] {
        let solid = k().thicken(&lid.shape, 2.0, both).unwrap();
        let v = k().mass_properties(&solid.shape).unwrap().volume.abs();
        assert!(close(v, 400.0, 1e-6), "{both}: {v}");
    }
    // A 20 x 10 x 5 box from its four walls and two patches, stitched: a solid.
    let tube = k().extrude_curves(&boxp(20.0, 10.0), &ExtrudeSpec { distance: 5.0, symmetric: false }).unwrap();
    let top_frame = Frame { origin: [0.0, 0.0, 5.0], ..Frame::XY };
    let top = k().patch(&Profile { frame: top_frame, ..boxp(20.0, 10.0) }).unwrap();
    let closed = k().stitch(&[tube.shape.clone(), lid.shape.clone(), top.shape.clone()], 1e-3).unwrap();
    let t = k().topology(&closed.shape).unwrap();
    assert_eq!((t.solids, t.faces.len()), (1, 6));
    let v = k().mass_properties(&closed.shape).unwrap().volume.abs();
    assert!(close(v, 1000.0, 1e-6), "{v}");
    assert!(closed.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Input { input: 2, .. })));
    // Without the top it stays a sheet.
    let open = k().stitch(&[tube.shape, lid.shape], 1e-3).unwrap();
    assert_eq!(k().topology(&open.shape).unwrap().solids, 0);
    assert!(k().stitch(&[], 1e-3).is_err());
}

/// Writes `step` to `GUSSET_SAT_DUMP` (if set) and imports it.
fn dump_and_import(name: &str, step: &str) -> KResult<Vec<(Shape, String)>> {
    if let Ok(d) = std::env::var("GUSSET_SAT_DUMP") {
        let _ = std::fs::write(std::path::Path::new(&d).join(format!("{name}.ours.step")), step);
    }
    k().import_step(step.as_bytes())
}

/// ACIS SAT (ADR 0039): exploratory pass over the files in `GUSSET_SAT_DIR` (prints, never
/// fails). The asserting tests are below.
#[test]
fn sat_survey() {
    let Ok(dir) = std::env::var("GUSSET_SAT_DIR") else { return };
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    for p in names.into_iter().filter(|p| p.extension().is_some_and(|e| e == "sat")) {
        let bytes = std::fs::read(&p).unwrap();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        match gusset_sat::to_step(&bytes, "s", &gusset_sat::Limits::default()) {
            Err(e) => eprintln!("SURVEY {name}: convert: {e}"),
            Ok(c) => match dump_and_import(&name, &c.step) {
                Err(e) => eprintln!("SURVEY {name}: import: {e}"),
                Ok(shapes) => {
                    let mut s = String::new();
                    for (shape, _) in &shapes {
                        let t = k().topology(shape).unwrap();
                        let m = k().mass_properties(shape).unwrap();
                        s.push_str(&format!(" [solids {} faces {} vol {:.4}]", t.solids, t.faces.len(), m.volume));
                    }
                    if let Ok(theirs) = std::fs::read(p.with_extension("stp")) {
                        match k().import_step(&theirs) {
                            Ok(t) => {
                                let vols: Vec<f64> =
                                    t.iter().map(|(sh, _)| k().mass_properties(sh).map_or(0.0, |m| m.volume)).collect();
                                let v: f64 = vols.iter().sum();
                                s.push_str(&format!(" oracle vol {v:.4} {vols:.4?}"));
                            }
                            Err(e) => s.push_str(&format!(" oracle: {e}")),
                        }
                    }
                    for w in c.warnings.iter().take(5) {
                        eprintln!("SURVEY   warning: {w}");
                    }
                    eprintln!(
                        "SURVEY {name}: bodies {} sat-faces {}{s}",
                        c.bodies,
                        String::from_utf8_lossy(&bytes).lines().filter(|l| l.starts_with("face ")).count()
                    );
                }
            },
        }
    }
}

/// ACIS SAT (ADR 0039): a generated box imports as one solid of its exact volume, in both
/// record layouts.
#[test]
fn a_sat_box_imports_with_its_volume() {
    for v in [600, 700] {
        let sat = gusset_sat::fixtures::box_sat(v, [10.0, 20.0, 30.0]);
        let c = gusset_sat::to_step(sat.as_bytes(), "box", &gusset_sat::Limits::default()).unwrap();
        let shapes = k().import_step(c.step.as_bytes()).unwrap();
        assert_eq!(shapes.len(), 1);
        let m = k().mass_properties(&shapes[0].0).unwrap();
        assert!((m.volume - 6000.0).abs() < 1e-6, "{v}: {}", m.volume);
    }
}

/// A dome on a spindle torus that reaches the axis at a point (3722's `Revolve2` as the file
/// stores it): it imports as the dome, of its volume by Pappus to 1e-9, not as the bowl under
/// it (46358 mm3, OCCT's reader having dropped the point's vertex loop).
#[test]
fn a_sat_dome_on_a_spindle_torus_imports_with_its_volume() {
    let (sat, pappus) = gusset_sat::fixtures::spindle_dome_sat();
    let c = gusset_sat::to_step(sat.as_bytes(), "dome", &gusset_sat::Limits::default()).unwrap();
    let shapes = k().import_step(c.step.as_bytes()).unwrap();
    assert_eq!(shapes.len(), 1);
    let m = k().mass_properties(&shapes[0].0).unwrap();
    assert!((m.volume - pappus).abs() <= 1e-9 * pappus, "{} against {pappus}", m.volume);
    assert_eq!(k().topology(&shapes[0].0).unwrap().solids, 1);
}

/// An elliptical cylinder (6126's logo petals, an ACIS cone of ratio 0.5) imports as a solid
/// of its volume, pi a b h.
#[test]
fn a_sat_elliptical_cylinder_imports_with_its_volume() {
    let (sat, volume) = gusset_sat::fixtures::elliptic_cylinder_sat();
    let c = gusset_sat::to_step(sat.as_bytes(), "petal", &gusset_sat::Limits::default()).unwrap();
    let shapes = k().import_step(c.step.as_bytes()).unwrap();
    assert_eq!(shapes.len(), 1);
    let m = k().mass_properties(&shapes[0].0).unwrap();
    assert!((m.volume - volume).abs() <= 1e-9 * volume, "{} against {volume}", m.volume);
    assert_eq!(k().topology(&shapes[0].0).unwrap().solids, 1);
}

/// ACIS SAT against Spatial's own STEP translations of the same files (NIST, CMU
/// spectrometer): the same volume. Runs when `GUSSET_SAT_SAMPLES` names
/// usnistgov/engineering-design-models/models.
#[test]
fn sat_samples_match_their_step_translations() {
    let Ok(dir) = std::env::var("GUSSET_SAT_SAMPLES") else { return };
    let dir = std::path::Path::new(&dir).join("CMU/spectrometer");
    for stem in [
        "bearing_208_back_shorts_inner_link_outer.hh",
        "bearing_208_link_42_inner_43.hh",
        "bearing_shaft_57_link1_43.hh",
        "bellow_upper_right.hh",
        "bellows_spacer_right.hh",
    ] {
        let sat = std::fs::read(dir.join(format!("{stem}.sat"))).unwrap();
        let c = gusset_sat::to_step(&sat, stem, &gusset_sat::Limits::default()).unwrap();
        let ours = k().import_step(c.step.as_bytes()).unwrap();
        let theirs = k().import_step(&std::fs::read(dir.join(format!("{stem}.sat.stp"))).unwrap()).unwrap();
        let vol = |v: &[(Shape, String)]| v.iter().map(|(s, _)| k().mass_properties(s).unwrap().volume).sum::<f64>();
        let (a, b) = (vol(&ours), vol(&theirs));
        assert!((a - b).abs() <= 1e-6 * b.abs(), "{stem}: {a} vs {b}");
        assert!(ours.iter().all(|(s, _)| k().topology(s).unwrap().solids == 1), "{stem}");
    }
}

/// ACIS B-spline curves: a box edge given as an `intcurve` (forward, and reversed against
/// its B-spline) still closes the solid.
#[test]
fn a_sat_box_with_a_spline_edge_imports() {
    let sat = gusset_sat::fixtures::box_sat(700, [10.0, 20.0, 30.0]);
    let first = sat.lines().find(|l| l.starts_with("straight-curve")).unwrap().to_string();
    assert!(first.contains(" 0 0 0 0 1 0 "), "{first}");
    for spline in [
        "intcurve-curve $-1 -1 $-1 forward { exactcur full nubs 1 open 2 0 1 20 1 0 0 0 0 20 0 } I I #",
        "intcurve-curve $-1 -1 $-1 reversed { exactcur full nubs 1 open 2 0 1 20 1 0 20 0 0 0 0 } I I #",
    ] {
        let text = sat.replacen(&first, spline, 1);
        let c = gusset_sat::to_step(text.as_bytes(), "box", &gusset_sat::Limits::default()).unwrap();
        assert!(c.step.contains("B_SPLINE_CURVE_WITH_KNOTS"));
        let shapes = k().import_step(c.step.as_bytes()).unwrap();
        let t = k().topology(&shapes[0].0).unwrap();
        let m = k().mass_properties(&shapes[0].0).unwrap();
        assert_eq!(t.solids, 1, "{spline}");
        assert!((m.volume - 6000.0).abs() < 1e-6, "{spline}: {}", m.volume);
    }
}

/// The id of the first `#N = <KIND>(` entity in STEP text.
fn entity(t: &str, kind: &str) -> usize {
    let at = t.find(&format!(" = {kind}(")).unwrap();
    let start = t[..at].rfind('#').unwrap();
    t[start + 1..at].trim().parse().unwrap()
}

#[test]
fn a_placed_body_keeps_its_name_and_colours_through_step() {
    // Dogfooding F15-1: an imported part placed by its assembly (a located shape) was written
    // as a reference whose part was named "SOLID", without the name or colours.
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let plain = String::from_utf8(k().export_step(&[(&a.shape, "Part")]).unwrap()).unwrap();
    // Wrap the part in an assembly that places it 100 mm along X.
    let (pd, rep, pc, pdc) = (
        entity(&plain, "PRODUCT_DEFINITION"),
        entity(&plain, "ADVANCED_BREP_SHAPE_REPRESENTATION"),
        entity(&plain, "PRODUCT_CONTEXT"),
        entity(&plain, "PRODUCT_DEFINITION_CONTEXT"),
    );
    let ctx = {
        let at = plain.find(" = ADVANCED_BREP_SHAPE_REPRESENTATION(").unwrap();
        let line = &plain[at..plain[at..].find(';').unwrap() + at];
        line.rsplit('#').next().unwrap().trim_end_matches(')').parse::<usize>().unwrap()
    };
    let n = 900_000usize;
    let extra = format!(
        "#{a}=CARTESIAN_POINT('',(0.,0.,0.));\n#{b}=DIRECTION('',(0.,0.,1.));\n#{c}=DIRECTION('',(1.,0.,0.));\n\
         #{o}=AXIS2_PLACEMENT_3D('',#{a},#{b},#{c});\n#{p}=CARTESIAN_POINT('',(100.,0.,0.));\n\
         #{q}=AXIS2_PLACEMENT_3D('',#{p},#{b},#{c});\n#{pr}=PRODUCT('Asm','Asm','',(#{pc}));\n\
         #{pf}=PRODUCT_DEFINITION_FORMATION('','',#{pr});\n#{apd}=PRODUCT_DEFINITION('design','',#{pf},#{pdc});\n\
         #{aps}=PRODUCT_DEFINITION_SHAPE('','',#{apd});\n#{asr}=SHAPE_REPRESENTATION('',(#{o},#{q}),#{ctx});\n\
         #{sdr}=SHAPE_DEFINITION_REPRESENTATION(#{aps},#{asr});\n\
         #{nauo}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','Part','',#{apd},#{pd},$);\n#{nps}=PRODUCT_DEFINITION_SHAPE('','',#{nauo});\n\
         #{idt}=ITEM_DEFINED_TRANSFORMATION('','',#{o},#{q});\n\
         #{rr}=(REPRESENTATION_RELATIONSHIP('','',#{rep},#{asr})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{idt})SHAPE_REPRESENTATION_RELATIONSHIP());\n\
         #{cd}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr},#{nps});\n",
        a = n,
        b = n + 1,
        c = n + 2,
        o = n + 3,
        p = n + 4,
        q = n + 5,
        pr = n + 6,
        pf = n + 7,
        apd = n + 8,
        aps = n + 9,
        asr = n + 10,
        sdr = n + 11,
        nauo = n + 12,
        nps = n + 13,
        idt = n + 14,
        rr = n + 15,
        cd = n + 16,
    );
    let at = plain.rfind("ENDSEC;").unwrap();
    let asm = format!("{}{extra}{}", &plain[..at], &plain[at..]);
    let placed = k().import_step(asm.as_bytes()).unwrap();
    assert_eq!(placed.len(), 1, "{:?}", placed.iter().map(|p| &p.1).collect::<Vec<_>>());
    assert!((k().bbox(&placed[0].0).unwrap().min[0] - 100.0).abs() < 1e-6);
    // Written again with a name, a colour and a face colour, then read: all there.
    let step = k().export_step(&[(&placed[0].0, "Board\u{1f}#2050c0\u{1f}1=d02020")]).unwrap();
    let text = String::from_utf8_lossy(&step);
    assert!(!text.contains("PRODUCT('SOLID'"), "an unnamed part was written");
    let back = k().import_step(&step).unwrap();
    let mut parts = back[0].1.split('\u{1f}');
    assert_eq!(parts.next(), Some("Board"), "{:?}", back[0].1);
    assert_eq!(parts.next(), Some("#2050c0"), "{:?}", back[0].1);
    assert_eq!(parts.next().map(|f| f.split(',').count()), Some(1), "{:?}", back[0].1);
    assert!((k().bbox(&back[0].0).unwrap().min[0] - 100.0).abs() < 1e-6);
}

fn ellipse_profile(frame: Frame, center: P2, major: P2, a: f64, b: f64) -> Profile {
    Profile {
        frame,
        regions: vec![Region2 {
            outer: Loop2 { edges: vec![ProfileEdge { seg: Seg2::Ellipse { center, major, a, b }, tag: 7 }] },
            holes: vec![],
        }],
    }
}

#[test]
fn an_extruded_ellipse_has_the_volume_of_its_area() {
    let p = ellipse_profile(Frame::XY, [1.0, 2.0], [0.6, 0.8], 5.0, 3.0);
    let r = k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let want = std::f64::consts::PI * 5.0 * 3.0 * 10.0;
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, want, 1e-8), "{} vs {want}", m.volume);
    let t = k().topology(&r.shape).unwrap();
    assert_eq!((t.faces.len(), t.solids), (3, 1));
    assert!(r.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Lateral { tag: 7 })));
}

#[test]
fn a_revolved_ellipse_is_an_o_ring() {
    // Inventor's 10x5 O-ring section: centre x = 7.85, a = 3.24 along y, b = 2.15.
    let (rr, a, b) = (7.85, 3.24, 2.15);
    let p = ellipse_profile(Frame::XZ, [rr, 0.0], [0.0, 1.0], a, b);
    let r = k()
        .revolve(&p, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::TAU })
        .unwrap();
    let want = 2.0 * std::f64::consts::PI * rr * std::f64::consts::PI * a * b;
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, want, 1e-8), "{} vs {want}", m.volume);
}

/// A rod of radius 1.5 and length 4 with both end edges rounded by radius 1: each round is
/// a spindle torus (major radius 1.5 - 1 = 0.5 < minor radius 1), the fillet of the
/// SolidWorks idler arms. Each end is the solid of revolution of the disc of radius 1
/// about its own centre line, so by Pappus a rounded end of height 1 has the volume
/// pi * (5/4 + pi/4 - 1/3) and the rest is a cylinder.
#[test]
fn a_rod_with_spindle_torus_rounds_has_its_exact_volume() {
    let rod = Profile {
        frame: Frame::XY,
        regions: vec![Region2 {
            outer: Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [0.0, 0.0], radius: 1.5 }, tag: 1 }] },
            holes: vec![],
        }],
    };
    let a = k().extrude(&rod, &ExtrudeSpec { distance: 4.0, symmetric: false }).unwrap();
    let t = k().topology(&a.shape).unwrap();
    let ends: Vec<u32> = t
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == CurveKind::Circle)
        .map(|(i, _)| u32::try_from(i).unwrap())
        .collect();
    assert_eq!(ends.len(), 2);
    let f = k().fillet(&a.shape, &ends, 1.0).unwrap();
    let t = k().topology(&f.shape).unwrap();
    assert_eq!(t.faces.iter().filter(|f| f.kind == SurfaceKind::Torus).count(), 2);
    let pi = std::f64::consts::PI;
    let end = pi * (1.25 + pi / 4.0 - 1.0 / 3.0);
    let want = pi * 1.5 * 1.5 * 2.0 + 2.0 * end;
    let m = k().mass_properties(&f.shape).unwrap();
    assert!(close(m.volume, want, 1e-9), "{} vs {want}", m.volume);
}

#[test]
fn elliptical_arcs_join_lines_either_way_round() {
    // Half of a 4 x 2 ellipse closed by its major axis, travelled both ways.
    let arc = |ccw: bool| {
        let (s, e, line) = if ccw {
            ([4.0, 0.0], [-4.0, 0.0], Seg2::Line { a: [-4.0, 0.0], b: [4.0, 0.0] })
        } else {
            ([-4.0, 0.0], [4.0, 0.0], Seg2::Line { a: [4.0, 0.0], b: [-4.0, 0.0] })
        };
        let seg = Seg2::EllipseArc { center: [0.0, 0.0], major: [1.0, 0.0], a: 4.0, b: 2.0, start: s, end: e, ccw };
        Profile {
            frame: Frame::XY,
            regions: vec![Region2 {
                outer: Loop2 { edges: vec![ProfileEdge { seg, tag: 1 }, ProfileEdge { seg: line, tag: 2 }] },
                holes: vec![],
            }],
        }
    };
    for ccw in [true, false] {
        let r = k().extrude(&arc(ccw), &ExtrudeSpec { distance: 3.0, symmetric: false }).unwrap();
        let m = k().mass_properties(&r.shape).unwrap();
        let want = std::f64::consts::PI * 4.0 * 3.0;
        assert!(close(m.volume, want, 1e-8), "ccw={ccw}: {} vs {want}", m.volume);
    }
}

/// Index of the face of `t` whose outward normal is `n` (planar faces of a box).
fn face_with_normal(t: &Topology, n: [f64; 3]) -> u32 {
    let i = t.faces.iter().position(|f| f.normal.iter().zip(n).all(|(a, b)| (a - b).abs() < 1e-9)).unwrap();
    i as u32
}

#[test]
fn full_round_replaces_an_end_face_by_a_half_cylinder() {
    // 20 x 4 x 10 slab, a full round on the 4 x 10 end face: radius 2 about the z direction.
    let a = k().extrude(&boxp(20.0, 4.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let t = k().topology(&a.shape).unwrap();
    let end = face_with_normal(&t, [1.0, 0.0, 0.0]);
    let r = k().full_round(&a.shape, &[end]).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    let expect = 800.0 - (4.0 * 2.0 * 10.0 - std::f64::consts::PI * 4.0 / 2.0 * 10.0);
    assert!(close(m.volume, expect, 1e-9), "{} vs {expect}", m.volume);
    let t2 = k().topology(&r.shape).unwrap();
    assert_eq!((t2.faces.len(), t2.solids), (6, 1));
    // The end face is gone; the rounded face is a cylinder of radius 2, half a turn, 10 long.
    let cyl: Vec<usize> = (0..t2.faces.len()).filter(|&i| t2.faces[i].kind == SurfaceKind::Cylinder).collect();
    assert_eq!(cyl.len(), 1);
    let f = &t2.faces[cyl[0]];
    assert!(close(f.area, std::f64::consts::PI * 2.0 * 10.0, 1e-9), "{}", f.area);
    let ax = f.axis.unwrap();
    assert!(ax.dir[2].abs() > 0.999999 && close(ax.origin[0], 18.0, 1e-9) && close(ax.origin[1], 2.0, 1e-9));
    let bb = k().bbox(&r.shape).unwrap();
    assert!(close(bb.max[0], 20.0, 1e-9), "{:?}", bb.max);
    // History: the cylinder descends from the two edges of the end face; the sides survive.
    assert!(r.face_origins[cyl[0]].iter().all(|o| matches!(o, FaceOrigin::FromEdge { input: 0, .. })));
    assert_eq!(r.face_origins[cyl[0]].len(), 2);
    for n in [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0]] {
        let side = face_with_normal(&t, n);
        assert!(close(area_of(&r, side), 18.0 * 10.0, 1e-9));
    }
}

#[test]
fn full_round_several_faces_and_a_cube() {
    let a = k().extrude(&boxp(20.0, 4.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let t = k().topology(&a.shape).unwrap();
    let both = [face_with_normal(&t, [1.0, 0.0, 0.0]), face_with_normal(&t, [-1.0, 0.0, 0.0])];
    let r = k().full_round(&a.shape, &both).unwrap();
    let v = k().mass_properties(&r.shape).unwrap().volume;
    assert!(close(v, 800.0 - 2.0 * (80.0 - 20.0 * std::f64::consts::PI), 1e-9), "{v}");
    // A cube face has two pairs of parallel sides at the same distance: it still rounds.
    let (c, _, _) = cube_edge();
    let tc = k().topology(&c.shape).unwrap();
    let top = face_with_normal(&tc, [0.0, 0.0, 1.0]);
    let r = k().full_round(&c.shape, &[top]).unwrap();
    let v = k().mass_properties(&r.shape).unwrap().volume;
    assert!(close(v, 1000.0 - (10.0 * 5.0 - std::f64::consts::PI * 25.0 / 2.0) * 10.0, 1e-9), "{v}");
}

#[test]
fn full_round_errors_are_clean() {
    let a = k().extrude(&boxp(20.0, 4.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    assert!(k().full_round(&a.shape, &[9999]).is_err());
    assert!(k().full_round(&a.shape, &[]).is_err());
    // A triangular prism: its caps lie between no parallel sides.
    let tri = Loop2 {
        edges: [([0.0, 0.0], [10.0, 0.0]), ([10.0, 0.0], [0.0, 8.0]), ([0.0, 8.0], [0.0, 0.0])]
            .iter()
            .enumerate()
            .map(|(i, &(a, b))| ProfileEdge { seg: Seg2::Line { a, b }, tag: 20 + i as u64 })
            .collect(),
    };
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: tri, holes: vec![] }] };
    let prism = k().extrude(&p, &ExtrudeSpec { distance: 5.0, symmetric: false }).unwrap();
    let t = k().topology(&prism.shape).unwrap();
    for n in [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0]] {
        assert!(k().full_round(&prism.shape, &[face_with_normal(&t, n)]).is_err());
    }
    // Its slanted side lies between the two caps, 5 mm apart: that one rounds.
    let hyp = t.faces.iter().position(|f| f.normal[0] > 0.1 && f.normal[1] > 0.1).unwrap() as u32;
    let r = k().full_round(&prism.shape, &[hyp]).unwrap();
    let v = k().mass_properties(&r.shape).unwrap().volume;
    assert!(v < 200.0 && v > 150.0, "{v}");
}

#[test]
fn unrefined_boolean_keeps_the_split_faces() {
    // Two 10 mm cubes side by side share a face: refined, the union is one 20 x 10 x 10 box
    // (6 faces); unrefined, the four faces around the seam stay split (10 faces).
    let cube = |x: f64| {
        let mut p = boxp(10.0, 10.0);
        p.frame.origin = [x, 0.0, 0.0];
        k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap().shape
    };
    let (a, b) = (cube(0.0), cube(10.0));
    let faces = |s: &Shape| k().topology(s).unwrap().faces.len();
    let vol = |s: &Shape| k().mass_properties(s).unwrap().volume;
    let refined = k().boolean(BoolOp::Union, &a, &b).unwrap();
    let raw = k().boolean_unrefined(BoolOp::Union, &a, &b).unwrap();
    assert_eq!(faces(&refined.shape), 6);
    assert_eq!(faces(&raw.shape), 10);
    assert!(close(vol(&raw.shape), 2000.0, 1e-9) && close(vol(&refined.shape), 2000.0, 1e-9));
    assert!(raw.face_origins.iter().all(|o| !o.is_empty()), "every split face keeps its history");
    // The same with several tools in one operation.
    let c = cube(20.0);
    let many = k().boolean_many(BoolOp::Union, &a, &[b.clone(), c.clone()]).unwrap();
    let many_raw = k().boolean_many_unrefined(BoolOp::Union, &a, &[b, c]).unwrap();
    assert_eq!((faces(&many.shape), faces(&many_raw.shape)), (6, 14));
    assert!(close(vol(&many_raw.shape), 3000.0, 1e-9));
}

// ---- extrusion up to a face ----

/// A slab `w` x `h` x `d` whose bottom face is at height `z` (sketch plane XY shifted up).
fn slab(z: f64, w: f64, h: f64, d: f64) -> Shape {
    let mut p = boxp(w, h);
    p.frame.origin = [0.0, 0.0, z];
    k().extrude(&p, &ExtrudeSpec { distance: d, symmetric: false }).unwrap().shape
}

/// The index of the planar face of `s` with normal `n` and centroid height `z`.
fn flat_face(s: &Shape, n: [f64; 3], z: f64) -> u32 {
    let t = k().topology(s).unwrap();
    t.faces
        .iter()
        .position(|f| {
            f.kind == SurfaceKind::Plane
                && (0..3).all(|i| (f.normal[i] - n[i]).abs() < 1e-9)
                && (f.centroid[2] - z).abs() < 1e-9
        })
        .unwrap() as u32
}

fn volume(s: &Shape) -> f64 {
    k().mass_properties(s).unwrap().volume
}

#[test]
fn extrude_to_parallel_plane_with_offset() {
    let plate = slab(30.0, 40.0, 20.0, 10.0);
    let f = flat_face(&plate, [0.0, 0.0, -1.0], 30.0);
    let r = k().extrude_to_face(&boxp(10.0, 10.0), &plate, f, 0.0, false).unwrap();
    assert!(close(volume(&r.shape), 3000.0, 1e-9));
    let t = k().topology(&r.shape).unwrap();
    assert_eq!((t.faces.len(), t.solids), (6, 1));
    // Same history as a plain extrude: four laterals, a start cap and an end cap in the plane.
    let end = r.face_origins.iter().position(|o| o == &vec![FaceOrigin::Cap { end: CapEnd::End, region: 0 }]).unwrap();
    assert!((t.faces[end].centroid[2] - 30.0).abs() < 1e-9);
    let lat = r.face_origins.iter().flatten().filter(|o| matches!(o, FaceOrigin::Lateral { .. })).count();
    assert_eq!(lat, 4);
    assert!(r.face_origins.iter().any(|o| matches!(o[..], [FaceOrigin::Cap { end: CapEnd::Start, .. }])));
    // Offset beyond the face and short of it.
    let past = k().extrude_to_face(&boxp(10.0, 10.0), &plate, f, 2.0, false).unwrap();
    assert!(close(volume(&past.shape), 3200.0, 1e-9));
    let short = k().extrude_to_face(&boxp(10.0, 10.0), &plate, f, -5.0, false).unwrap();
    assert!(close(volume(&short.shape), 2500.0, 1e-9));
}

#[test]
fn extrude_to_face_reverse_and_refusals() {
    // A plate below the sketch: only the reverse direction reaches its top.
    let below = slab(-15.0, 40.0, 20.0, 10.0);
    let top = flat_face(&below, [0.0, 0.0, 1.0], -5.0);
    let r = k().extrude_to_face(&boxp(10.0, 10.0), &below, top, 0.0, true).unwrap();
    assert!(close(volume(&r.shape), 500.0, 1e-9));
    let e = k().extrude_to_face(&boxp(10.0, 10.0), &below, top, 0.0, false).unwrap_err();
    assert!(e.message.contains("behind"), "{}", e.message);
    // A face smaller than the profile does not cover the sweep.
    let small = slab(30.0, 4.0, 4.0, 10.0);
    let f = flat_face(&small, [0.0, 0.0, -1.0], 30.0);
    let e = k().extrude_to_face(&boxp(10.0, 10.0), &small, f, 0.0, false).unwrap_err();
    assert!(e.message.contains("cover"), "{}", e.message);
    // A face edge-on to the sweep is never reached.
    let wall = slab(30.0, 40.0, 20.0, 10.0);
    let side = k().topology(&wall).unwrap().faces.iter().position(|f| f.normal[0] < -0.9).unwrap() as u32;
    assert!(k().extrude_to_face(&boxp(10.0, 10.0), &wall, side, 0.0, false).is_err());
    // Out-of-range face.
    assert!(k().extrude_to_face(&boxp(10.0, 10.0), &wall, 999, 0.0, false).is_err());
}

fn xz_poly(pts: &[[f64; 2]]) -> Profile {
    let edges = (0..pts.len())
        .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }, tag: 1 + i as u64 })
        .collect();
    Profile { frame: Frame::XZ, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] }
}

#[test]
fn extrude_to_tilted_plane() {
    // A wedge whose underside rises from z = 20 at x = 0 to z = 30 at x = 40 (y 0..30).
    let wedge = k()
        .extrude(
            &xz_poly(&[[0.0, 20.0], [40.0, 30.0], [40.0, 50.0], [0.0, 50.0]]),
            &ExtrudeSpec { distance: -30.0, symmetric: false },
        )
        .unwrap()
        .shape;
    let t = k().topology(&wedge).unwrap();
    let under = t.faces.iter().position(|f| f.normal[2] < -0.9).unwrap() as u32;
    let r = k().extrude_to_face(&boxp(10.0, 10.0), &wedge, under, 0.0, false).unwrap();
    // Height 20 + x / 4 over the 10 x 10 square: 10 * (200 + 12.5).
    assert!(close(volume(&r.shape), 2125.0, 1e-9), "{}", volume(&r.shape));
    let rt = k().topology(&r.shape).unwrap();
    let end = r.face_origins.iter().position(|o| o == &vec![FaceOrigin::Cap { end: CapEnd::End, region: 0 }]).unwrap();
    // The end cap lies on the wedge's underside, which rises toward +x: its outward normal leans to -x.
    assert!(rt.faces[end].normal[2] > 0.9 && rt.faces[end].normal[0] < -0.1);
    // Offset is measured along the sweep direction perpendicular to the face.
    let off = k().extrude_to_face(&boxp(10.0, 10.0), &wedge, under, 1.0, false).unwrap();
    let cos = (1.0f64 + 0.25f64 * 0.25).sqrt().recip();
    assert!(close(volume(&off.shape), 2125.0 + 100.0 * 1.0 / cos, 1e-6), "{}", volume(&off.shape));
}

#[test]
fn extrude_to_cylinder_face() {
    // A cylinder along Y (radius 8, axis at x = 5, z = 20, y -10..30); the sweep from z = 0
    // ends on its lower surface z = 20 - sqrt(64 - (x - 5)^2).
    let circle = Profile {
        frame: Frame::XZ,
        regions: vec![Region2 {
            outer: Loop2 {
                edges: vec![ProfileEdge { seg: Seg2::Circle { center: [5.0, 20.0], radius: 8.0 }, tag: 1 }],
            },
            holes: vec![],
        }],
    };
    let mut c = circle;
    c.frame.origin = [0.0, -10.0, 0.0];
    let cyl = k().extrude(&c, &ExtrudeSpec { distance: -40.0, symmetric: false }).unwrap().shape;
    let t = k().topology(&cyl).unwrap();
    let side = t.faces.iter().position(|f| f.kind == SurfaceKind::Cylinder).unwrap() as u32;
    let r = k().extrude_to_face(&boxp(10.0, 10.0), &cyl, side, 0.0, false).unwrap();
    let a = 5.0f64;
    let rad = 8.0f64;
    let integral = a * (rad * rad - a * a).sqrt() + rad * rad * (a / rad).asin();
    let expect = 10.0 * (200.0 - integral);
    assert!(close(volume(&r.shape), expect, 1e-6), "{} vs {expect}", volume(&r.shape));
    assert!(r.face_origins.iter().any(|o| o == &vec![FaceOrigin::Cap { end: CapEnd::End, region: 0 }]));
    // A cylinder face takes no offset.
    assert!(k().extrude_to_face(&boxp(10.0, 10.0), &cyl, side, 1.0, false).is_err());
}

/// A rational Bézier kernel segment from its control points and weights.
fn rational(poles: &[P2], weights: &[f64]) -> Seg2 {
    let mut p = [[0.0; 2]; gusset_kernel::MAX_SEG_DEGREE + 1];
    let mut w = [0.0; gusset_kernel::MAX_SEG_DEGREE + 1];
    p[..poles.len()].copy_from_slice(poles);
    w[..weights.len()].copy_from_slice(weights);
    Seg2::RationalBezier { degree: u8::try_from(poles.len() - 1).unwrap(), poles: p, weights: w }
}

/// The circle of radius `r` around `c` as four rational quadratics (the spans of the
/// nine-pole NURBS circle), counter-clockwise from (c + r, c).
fn nurbs_circle(c: P2, r: f64, tag: u64) -> Loop2 {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let q = [[r, 0.0], [r, r], [0.0, r], [-r, r], [-r, 0.0], [-r, -r], [0.0, -r], [r, -r], [r, 0.0]]
        .map(|p| [c[0] + p[0], c[1] + p[1]]);
    Loop2 {
        edges: (0..4)
            .map(|k| ProfileEdge { seg: rational(&q[2 * k..2 * k + 3], &[1.0, h, 1.0]), tag: tag + k as u64 })
            .collect(),
    }
}

#[test]
fn an_extruded_nurbs_circle_has_the_volume_of_a_cylinder() {
    let p = Profile {
        frame: Frame::XY,
        regions: vec![Region2 { outer: nurbs_circle([2.0, -1.0], 5.0, 10), holes: vec![] }],
    };
    let r = k().extrude(&p, &ExtrudeSpec { distance: 7.0, symmetric: false }).unwrap();
    let want = std::f64::consts::PI * 25.0 * 7.0;
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, want, 1e-9), "{} vs {want}", m.volume);
    // One lateral face per span, named by its tag; two caps.
    let t = k().topology(&r.shape).unwrap();
    assert_eq!((t.faces.len(), t.solids), (6, 1));
    for tag in 10..14 {
        assert!(r.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Lateral { tag: t } if *t == tag)));
    }
}

#[test]
fn a_revolved_nurbs_circle_is_a_torus() {
    let (rr, r) = (12.0, 3.0);
    let p =
        Profile { frame: Frame::XZ, regions: vec![Region2 { outer: nurbs_circle([rr, 0.0], r, 1), holes: vec![] }] };
    let rv = k()
        .revolve(&p, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::TAU })
        .unwrap();
    let want = 2.0 * std::f64::consts::PI * rr * std::f64::consts::PI * r * r;
    let m = k().mass_properties(&rv.shape).unwrap();
    assert!(close(m.volume, want, 1e-8), "{} vs {want}", m.volume);
}

#[test]
fn a_periodic_quadratic_bspline_and_a_quintic_edge_have_exact_areas() {
    // The uniform periodic quadratic over the square of corners (±a, ±a): from edge
    // midpoint to edge midpoint with the corner as middle pole; area 10 a² / 3.
    let a = 6.0;
    let mids = [[a, 0.0], [0.0, a], [-a, 0.0], [0.0, -a]];
    let corners = [[a, a], [-a, a], [-a, -a], [a, -a]];
    let edges = (0..4)
        .map(|k| ProfileEdge { seg: rational(&[mids[k], corners[k], mids[(k + 1) % 4]], &[1.0; 3]), tag: k as u64 })
        .collect();
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let r = k().extrude(&p, &ExtrudeSpec { distance: 2.0, symmetric: false }).unwrap();
    let want = 10.0 * a * a / 3.0 * 2.0;
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, want, 1e-9), "{} vs {want}", m.volume);
    // A degree-5 segment whose poles lie evenly on a straight edge closes a 4 x 3 rectangle.
    let quintic = rational(&(0..6).map(|i| [4.0 - 0.8 * f64::from(i), 3.0]).collect::<Vec<_>>(), &[1.0; 6]);
    let edges = vec![
        ProfileEdge { seg: Seg2::Line { a: [0.0, 0.0], b: [4.0, 0.0] }, tag: 1 },
        ProfileEdge { seg: Seg2::Line { a: [4.0, 0.0], b: [4.0, 3.0] }, tag: 2 },
        ProfileEdge { seg: quintic, tag: 3 },
        ProfileEdge { seg: Seg2::Line { a: [0.0, 3.0], b: [0.0, 0.0] }, tag: 4 },
    ];
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let r = k().extrude(&p, &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, 12.0, 1e-9), "{}", m.volume);
}

/// The straight edge of `r` that is `len` long, parallel to Z, at `(x, y)`.
fn vertical_edge_at(r: &OpResult, x: f64, y: f64, len: f64) -> u32 {
    let t = k().topology(&r.shape).unwrap();
    t.edges
        .iter()
        .position(|e| {
            (e.length - len).abs() < 1e-9 && (e.midpoint[0] - x).abs() < 1e-9 && (e.midpoint[1] - y).abs() < 1e-9
        })
        .unwrap() as u32
}

#[test]
fn fillets_that_meet_make_a_full_round() {
    // An 8 mm wide end: radius 4 on both of its corners leaves nothing of the end face (OCCT's
    // blend fails there; the corners are cut away instead).
    let b = plain_box(8.0, 20.0, 2.0);
    let e0 = vertical_edge_at(&b, 0.0, 0.0, 2.0);
    let e1 = vertical_edge_at(&b, 8.0, 0.0, 2.0);
    let f = k().fillet(&b.shape, &[e0, e1], 4.0).unwrap();
    let corner = 16.0 - 4.0 * std::f64::consts::PI;
    let m = k().mass_properties(&f.shape).unwrap();
    assert!(close(m.volume, 320.0 - 2.0 * 2.0 * corner, 1e-7), "{}", m.volume);
    for e in [e0, e1] {
        assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    }
    // A radius whose corners reach past the faces is still refused.
    assert!(k().fillet(&b.shape, &[e0, e1], 4.5).is_err());
}

#[test]
fn fillets_that_round_a_window_end() {
    // A 4 mm wide window through a 1 mm plate: radius 2 on its two top corners (concave edges)
    // leaves nothing of the window's top face; the corners are filled instead.
    let mut window = rect(4.0, 10.0, 20);
    for e in &mut window.edges {
        if let Seg2::Line { a, b } = &mut e.seg {
            for p in [a, b] {
                p[0] += 8.0;
                p[1] += 5.0;
            }
        }
    }
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: rect(20.0, 20.0, 10), holes: vec![window] }] };
    let b = k().extrude(&p, &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap();
    let e0 = vertical_edge_at(&b, 8.0, 15.0, 1.0);
    let e1 = vertical_edge_at(&b, 12.0, 15.0, 1.0);
    let f = k().fillet(&b.shape, &[e0, e1], 2.0).unwrap();
    let m = k().mass_properties(&f.shape).unwrap();
    assert!(close(m.volume, 360.0 + 2.0 * (4.0 - std::f64::consts::PI), 1e-7), "{}", m.volume);
    for e in [e0, e1] {
        assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    }
}

#[test]
fn fillets_between_a_bar_and_a_disc() {
    // A 2 mm wide bar out of a disc of radius 5: the concave edges where the bar's sides meet the
    // disc's rim (a plane and a cylinder) rounded with radius 0.5. The bar ends where the rounds
    // end, so they leave nothing of its sides (OCCT's blend fails); the corners are filled.
    let s = 24f64.sqrt();
    let end = 28f64.sqrt();
    let p = Profile {
        frame: Frame::XY,
        regions: vec![Region2 {
            outer: Loop2 {
                edges: vec![
                    ProfileEdge {
                        seg: Seg2::Arc { center: [0.0, 0.0], start: [s, 1.0], end: [s, -1.0], ccw: true },
                        tag: 1,
                    },
                    ProfileEdge { seg: Seg2::Line { a: [s, -1.0], b: [end, -1.0] }, tag: 2 },
                    ProfileEdge { seg: Seg2::Line { a: [end, -1.0], b: [end, 1.0] }, tag: 3 },
                    ProfileEdge { seg: Seg2::Line { a: [end, 1.0], b: [s, 1.0] }, tag: 4 },
                ],
            },
            holes: vec![],
        }],
    };
    let b = k().extrude(&p, &ExtrudeSpec { distance: 2.0, symmetric: false }).unwrap();
    let e0 = vertical_edge_at(&b, s, 1.0, 2.0);
    let e1 = vertical_edge_at(&b, s, -1.0, 2.0);
    let f = k().fillet(&b.shape, &[e0, e1], 0.5).unwrap();
    // One corner's section: from the edge along the bar's side to the round, round it to the rim,
    // back along the rim (shoelace over a fine polyline).
    let (r, big) = (0.5, 5.0);
    let o = [(28f64).sqrt(), 1.5];
    let t2 = [o[0] * big / 5.5, o[1] * big / 5.5];
    let mut pts: Vec<[f64; 2]> = vec![[s, 1.0], [o[0], 1.0]];
    let n = 20000;
    let (a0, a1) = (-std::f64::consts::FRAC_PI_2, (t2[1] - o[1]).atan2(t2[0] - o[0]));
    for i in 1..=n {
        let a = a0 + (a1 - a0) * f64::from(i) / f64::from(n);
        pts.push([o[0] + r * a.cos(), o[1] + r * a.sin()]);
    }
    let (b0, b1) = (t2[1].atan2(t2[0]), 1f64.atan2(s));
    for i in 1..n {
        let a = b0 + (b1 - b0) * f64::from(i) / f64::from(n);
        pts.push([big * a.cos(), big * a.sin()]);
    }
    let area = 0.5
        * (0..pts.len())
            .map(|i| {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                p[0] * q[1] - q[0] * p[1]
            })
            .sum::<f64>()
            .abs();
    let before = k().mass_properties(&b.shape).unwrap().volume;
    let m = k().mass_properties(&f.shape).unwrap();
    assert!(close(m.volume, before + 2.0 * 2.0 * area, 1e-6), "{} vs {}", m.volume, before + 4.0 * area);
    for e in [e0, e1] {
        assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    }
}

#[test]
fn a_convex_fillet_beside_a_concave_one_rolls_on_it() {
    // A rod of radius 7.942 with a groove 0.942 deep below z -10.5 and a 45 degree chamfer at its
    // foot. Rounding the groove's inner top edge (concave), the rod's rim above it (convex) and the
    // groove's foot (convex) with 0.5 mm: the two rounds at z -10.5 need 1 mm of a 0.942 mm step,
    // so OCCT fails on them at once; the concave one first, the convex ball then rolls between the
    // rod and the concave round, as Fusion's stored body has it.
    let pts = [[0.0, 0.0], [7.942, 0.0], [7.942, -10.5], [7.0, -10.5], [7.0, -12.5], [5.5, -14.0], [0.0, -14.0]];
    let edges = (0..pts.len())
        .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }, tag: i as u64 + 1 })
        .collect();
    let p = Profile { frame: Frame::XZ, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let rod = k()
        .revolve(&p, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::TAU })
        .unwrap();
    let t = k().topology(&rod.shape).unwrap();
    let circle = |r: f64, z: f64| {
        t.edges
            .iter()
            .position(|e| {
                e.circle.as_ref().is_some_and(|c| (c.radius - r).abs() < 1e-9 && (c.center[2] - z).abs() < 1e-9)
            })
            .unwrap() as u32
    };
    let picked = [circle(7.0, -10.5), circle(7.942, -10.5), circle(7.0, -12.5)];
    let f = k().fillet(&rod.shape, &picked, 0.5).unwrap();
    // Pappus over the section: the rim's ball centre is 1 from the concave round's (7.5, -11).
    let arc = |c: [f64; 2], a0: f64, a1: f64| -> Vec<[f64; 2]> {
        (0..=4000)
            .map(|i| a0 + (a1 - a0) * f64::from(i) / 4000.0)
            .map(|a| [c[0] + 0.5 * a.cos(), c[1] + 0.5 * a.sin()])
            .collect()
    };
    let cc = [7.5, -11.0];
    let cv = [7.442, cc[1] + (1.0f64 - 0.058 * 0.058).sqrt()];
    let towards = (cc[1] - cv[1]).atan2(cc[0] - cv[0]);
    let back = (cv[1] - cc[1]).atan2(cv[0] - cc[0]);
    let mut sec = vec![[0.0, 0.0], [7.942, 0.0]];
    sec.extend(arc(cv, 0.0, towards));
    sec.extend(arc(cc, back, std::f64::consts::PI));
    sec.extend(arc([6.5, -13.0 + 0.5 * std::f64::consts::SQRT_2], 0.0, -std::f64::consts::FRAC_PI_4));
    sec.extend([[5.5, -14.0], [0.0, -14.0]]);
    let v = (0..sec.len())
        .map(|i| {
            let (a, b) = (sec[i], sec[(i + 1) % sec.len()]);
            (a[0] * a[0] + a[0] * b[0] + b[0] * b[0]) * (b[1] - a[1])
        })
        .sum::<f64>()
        .abs()
        * std::f64::consts::PI
        / 3.0;
    let m = k().mass_properties(&f.shape).unwrap();
    assert!(close(m.volume, v, 1e-7), "{} vs {v}", m.volume);
    for e in picked {
        assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    }
}

#[test]
fn a_convex_and_a_concave_round_that_use_up_their_face() {
    // A pocket through a 2 mm plate with a 6 mm notch out of its side: rounding the notch's
    // convex corner at (15, 8) and its concave corner at (21, 8) by 3 mm leaves nothing of the
    // 6 mm face between them (OCCT's blend fails); the convex corners are cut away, the concave
    // one filled. The convex corner at (15, 12) is rounded too.
    let pts =
        [[5.0, 2.0], [15.0, 2.0], [15.0, 8.0], [21.0, 8.0], [21.0, 12.0], [15.0, 12.0], [15.0, 15.0], [5.0, 15.0]];
    let pocket = Loop2 {
        edges: (0..pts.len())
            .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }, tag: 20 + i as u64 })
            .collect(),
    };
    let p = Profile { frame: Frame::XY, regions: vec![Region2 { outer: rect(30.0, 20.0, 10), holes: vec![pocket] }] };
    let b = k().extrude(&p, &ExtrudeSpec { distance: 2.0, symmetric: false }).unwrap();
    let picked = [
        vertical_edge_at(&b, 15.0, 8.0, 2.0),
        vertical_edge_at(&b, 21.0, 8.0, 2.0),
        vertical_edge_at(&b, 15.0, 12.0, 2.0),
    ];
    let f = k().fillet(&b.shape, &picked, 3.0).unwrap();
    let corner = 2.0 * (9.0 - 9.0 * std::f64::consts::FRAC_PI_4);
    let m = k().mass_properties(&f.shape).unwrap();
    assert!(close(m.volume, 2.0 * (600.0 - 154.0) - corner, 1e-7), "{}", m.volume);
    // No face is left on y = 8 between the two rounds.
    let t = k().topology(&f.shape).unwrap();
    assert!(!t.faces.iter().any(|x| x.kind == SurfaceKind::Plane
        && (x.normal[1].abs() - 1.0).abs() < 1e-9
        && (x.centroid[1] - 8.0).abs() < 1e-9));
    for e in picked {
        assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    }
}

fn block_at(x0: f64, y0: f64, w: f64, h: f64, z0: f64, d: f64) -> OpResult {
    let p = Profile {
        frame: Frame { origin: [x0, y0, z0], ..Frame::XY },
        regions: vec![Region2 { outer: rect(w, h, 10), holes: vec![] }],
    };
    k().extrude(&p, &ExtrudeSpec { distance: d, symmetric: false }).unwrap()
}

#[test]
fn fillet_of_a_rim_between_two_flats_is_the_whole_rims_round() {
    // A disc of radius 10 with flats at y = 8 and y = -7: its foot is two arcs. Rounding both by
    // 2.5 (OCCT's blend fails at the flats) gives the round of the whole rim cut off by the
    // flats, as Fusion's one torus face is: the same as rounding a plain disc and then cutting.
    let flats = |s: &Shape| {
        let s = k().boolean(BoolOp::Cut, s, &block_at(-20.0, 8.0, 40.0, 10.0, -1.0, 7.0).shape).unwrap();
        k().boolean(BoolOp::Cut, &s.shape, &block_at(-20.0, -17.0, 40.0, 10.0, -1.0, 7.0).shape).unwrap()
    };
    let d = flats(&disc(10.0, 5.0, 0.0).shape);
    let t = k().topology(&d.shape).unwrap();
    let arcs: Vec<u32> = (0..t.edges.len() as u32)
        .filter(|&i| {
            let e = &t.edges[i as usize];
            e.circle.as_ref().is_some_and(|c| (c.radius - 10.0).abs() < 1e-9 && c.center[2].abs() < 1e-9)
        })
        .collect();
    assert_eq!(arcs.len(), 2);
    let f = k().fillet(&d.shape, &arcs, 2.5).unwrap();
    let plain = disc(10.0, 5.0, 0.0);
    let pt = k().topology(&plain.shape).unwrap();
    let foot = pt
        .edges
        .iter()
        .position(|e| e.circle.as_ref().is_some_and(|c| (c.radius - 10.0).abs() < 1e-9 && c.center[2].abs() < 1e-9))
        .unwrap() as u32;
    let round = k().fillet(&plain.shape, &[foot], 2.5).unwrap();
    let want = k().mass_properties(&flats(&round.shape).shape).unwrap().volume;
    let got = k().mass_properties(&f.shape).unwrap().volume;
    assert!(close(got, want, 1e-7), "{got} vs {want}");
    for e in arcs {
        assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    }
}

#[test]
fn an_intersection_of_two_flatted_bars_on_one_cylinder() {
    // A bar of radius 2.7 along z, flatted to |x| <= 1.85 and bored (r 0.975) along 7.8 mm at
    // each end, and the same bar flatted and bored along its whole length: the second lies in
    // the first, so their intersection is the second. (OCCT fails one way round.)
    let bar = disc(2.7, 36.6, -18.3);
    let flatted = |s: &Shape, z0: f64, d: f64| {
        let s = k().boolean(BoolOp::Cut, s, &block_at(1.85, -5.0, 5.0, 10.0, z0, d).shape).unwrap();
        k().boolean(BoolOp::Cut, &s.shape, &block_at(-6.85, -5.0, 5.0, 10.0, z0, d).shape).unwrap()
    };
    let mut a = bar.shape.clone();
    for z0 in [-18.3, 10.5] {
        a = flatted(&a, z0, 7.8).shape;
        a = k().boolean(BoolOp::Cut, &a, &disc(0.975, 7.8, z0).shape).unwrap().shape;
    }
    let b = flatted(&bar.shape, -18.3, 36.6);
    let b = k().boolean(BoolOp::Cut, &b.shape, &disc(0.975, 36.6, -18.3).shape).unwrap();
    let want = k().mass_properties(&b.shape).unwrap().volume;
    for (x, y) in [(&a, &b.shape), (&b.shape, &a)] {
        let i = k().boolean(BoolOp::Intersect, x, y).unwrap();
        let got = k().mass_properties(&i.shape).unwrap().volume;
        assert!(close(got, want, 1e-7), "{got} vs {want}");
    }
}

#[test]
fn fillet_of_an_arc_that_takes_its_whole_face() {
    // A 1 mm half disc of radius 10: a 1 mm fillet of its top arc leaves nothing of the
    // cylinder (OCCT's blend fails); the corner is cut away and ends on the flat side.
    let half = Profile {
        frame: Frame::XY,
        regions: vec![Region2 {
            outer: Loop2 {
                edges: vec![
                    ProfileEdge {
                        seg: Seg2::Arc { center: [0.0, 0.0], start: [10.0, 0.0], end: [-10.0, 0.0], ccw: true },
                        tag: 1,
                    },
                    ProfileEdge { seg: Seg2::Line { a: [-10.0, 0.0], b: [10.0, 0.0] }, tag: 2 },
                ],
            },
            holes: vec![],
        }],
    };
    let d = k().extrude(&half, &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap();
    let t = k().topology(&d.shape).unwrap();
    let rim = t.edges.iter().position(|e| (e.midpoint[2] - 1.0).abs() < 1e-9 && e.length > 30.0).unwrap() as u32;
    let f = k().fillet(&d.shape, &[rim], 1.0).unwrap();
    // Half of the revolved corner: area 1 - pi/4, its centroid 9 + (1/6) / (1 - pi/4) from the axis.
    let pi = std::f64::consts::PI;
    let corner = pi * (9.0 * (1.0 - pi / 4.0) + 1.0 / 6.0);
    let m = k().mass_properties(&f.shape).unwrap();
    assert!(close(m.volume, 50.0 * pi - corner, 1e-7), "{}", m.volume);
    assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: rim }));
}

#[test]
fn chamfer_around_a_block_as_high_as_the_chamfer() {
    // A 1 mm chamfer around the top of a 10 x 4 x 1 block: a frustum (prismatoid volume).
    let b = plain_box(10.0, 4.0, 1.0);
    let t = k().topology(&b.shape).unwrap();
    let top: Vec<u32> =
        (0..t.edges.len()).filter(|&i| (t.edges[i].midpoint[2] - 1.0).abs() < 1e-9).map(|i| i as u32).collect();
    assert_eq!(top.len(), 4);
    let c = k().chamfer(&b.shape, &top, 1.0).unwrap();
    let m = k().mass_properties(&c.shape).unwrap();
    assert!(close(m.volume, (40.0 + 16.0 + 4.0 * 27.0) / 6.0, 1e-7), "{}", m.volume);
}

#[test]
fn a_chamfer_next_to_a_chamfers_slope_runs_on_to_it() {
    // A 2.5 x 0.6 x 0.5 mm tab under a 6 x 6 x 2 mm block (5358's housing tab). Chamfer1 takes
    // 0.3 mm of the tab's cap and its whole 0.5 mm front; Chamfer2 then 0.2 mm of the cap and the
    // whole height of both sides, from the slope to the back. At the slope the body goes on
    // inside each side's corner: the chamfer runs on until it leaves the body (a tetrahedron of
    // 0.2 x 0.5 / 2 x 0.3 / 3 mm3 per side besides the 0.3 mm of prism along the edge).
    let at = |w: f64, h: f64, d: f64, o: [f64; 3]| {
        let mut p = boxp(w, h);
        p.frame.origin = o;
        k().extrude(&p, &ExtrudeSpec { distance: d, symmetric: false }).unwrap().shape
    };
    let body = k()
        .boolean(BoolOp::Union, &at(6.0, 6.0, 2.0, [-3.0, -3.0, 0.0]), &at(2.5, 0.6, 0.5, [-1.25, 1.9, -0.5]))
        .unwrap();
    let edge_at = |s: &Shape, m: [f64; 3]| {
        let t = k().topology(s).unwrap();
        t.edges.iter().position(|e| (0..3).all(|i| (e.midpoint[i] - m[i]).abs() < 1e-9)).unwrap() as u32
    };
    let cap = |s: &Shape, area: f64| {
        let t = k().topology(s).unwrap();
        t.faces.iter().position(|f| f.normal[2] < -0.9 && close(f.area, area, 1e-9)).unwrap() as u32
    };
    let first = ChamferSpec::TwoDistances { first: 0.3, second: 0.5 };
    let e = edge_at(&body.shape, [0.0, 1.9, -0.5]);
    let c1 = k().chamfer_ref(&body.shape, &[e], &[cap(&body.shape, 1.5)], &first).unwrap();
    let v1 = k().mass_properties(&c1.shape).unwrap().volume;
    assert!(close(v1, 72.75 - 0.5 * 0.3 * 0.5 * 2.5, 1e-9), "{v1}");
    let second = ChamferSpec::TwoDistances { first: 0.2, second: 0.5 };
    let sides = [edge_at(&c1.shape, [-1.25, 2.35, -0.5]), edge_at(&c1.shape, [1.25, 2.35, -0.5])];
    let f = cap(&c1.shape, 0.75);
    let c2 = k().chamfer_ref(&c1.shape, &sides, &[f, f], &second).unwrap();
    let v2 = k().mass_properties(&c2.shape).unwrap().volume;
    let per_side = 0.5 * 0.2 * 0.5 * 0.3 + 0.5 * 0.2 * 0.5 * 0.3 / 3.0;
    assert!(close(v2, v1 - 2.0 * per_side, 1e-9), "{v2}");
    for e in sides {
        assert!(c2.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    }
    assert!(c2.face_origins.iter().all(|o| !o.is_empty()));
}

#[test]
fn chamfer_that_takes_a_whole_face() {
    // A 1 mm disc: a 1 mm chamfer of its bottom rim leaves no cylinder below the top rim.
    let disc = Profile {
        frame: Frame::XY,
        regions: vec![Region2 {
            outer: Loop2 {
                edges: vec![ProfileEdge { seg: Seg2::Circle { center: [0.0, 0.0], radius: 10.0 }, tag: 1 }],
            },
            holes: vec![],
        }],
    };
    let d = k().extrude(&disc, &ExtrudeSpec { distance: 1.0, symmetric: false }).unwrap();
    let t = k().topology(&d.shape).unwrap();
    let rim = t.edges.iter().position(|e| e.midpoint[2].abs() < 1e-9 && e.length > 60.0).unwrap() as u32;
    let c = k().chamfer(&d.shape, &[rim], 1.0).unwrap();
    let ring = 0.5 * 2.0 * std::f64::consts::PI * (10.0 - 1.0 / 3.0);
    let m = k().mass_properties(&c.shape).unwrap();
    assert!(close(m.volume, 100.0 * std::f64::consts::PI - ring, 1e-7), "{}", m.volume);
    assert!(c.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: rim }));
    assert_eq!(k().topology(&c.shape).unwrap().faces.len(), 3);
}

#[test]
fn shell_of_a_body_of_two_solids() {
    // Two boxes as one body: each is shelled with its own opened face.
    let mut far = rect(10.0, 10.0, 20);
    for e in &mut far.edges {
        if let Seg2::Line { a, b } = &mut e.seg {
            a[0] += 20.0;
            b[0] += 20.0;
        }
    }
    let p = Profile {
        frame: Frame::XY,
        regions: vec![Region2 { outer: rect(10.0, 10.0, 10), holes: vec![] }, Region2 { outer: far, holes: vec![] }],
    };
    let b = k().extrude(&p, &ExtrudeSpec { distance: 5.0, symmetric: false }).unwrap();
    let t = k().topology(&b.shape).unwrap();
    let tops: Vec<u32> = (0..t.faces.len()).filter(|&i| t.faces[i].normal[2] > 0.9).map(|i| i as u32).collect();
    assert_eq!(tops.len(), 2);
    let r = k().shell(&b.shape, &tops, 1.0).unwrap();
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, 2.0 * (500.0 - 8.0 * 8.0 * 4.0), 1e-9), "{}", m.volume);
    // One opened face: the other box stays solid.
    let one = k().shell(&b.shape, &tops[..1], 1.0).unwrap();
    let m = k().mass_properties(&one.shape).unwrap();
    assert!(close(m.volume, 1000.0 - 8.0 * 8.0 * 4.0, 1e-9), "{}", m.volume);
    let rt = k().topology(&one.shape).unwrap();
    assert_eq!(one.face_origins.len(), rt.faces.len());
    assert!(one.face_origins.iter().all(|o| !o.is_empty()));
}

/// A circle as a profile loop.
fn circle_loop(r: f64, tag: u64) -> Loop2 {
    Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [0.0, 0.0], radius: r }, tag }] }
}

/// A shape with a wound face (the default quadrature, which a face winding several turns
/// needs) and a rational B-spline face trimmed by planes (the adaptive one, which it needs) is
/// measured face by face. A hexagon nut whose corners a cone cuts and whose hole a helical
/// groove cuts measures the same with the cone a lofted B-spline (rational: between two
/// circles) as with the cone exact. On the whole shape the default quadrature put the lofted
/// one off (3350's stored nut, a revolved spline trimmed by the hexagon's sides: 1.4e-4).
#[test]
fn a_wound_face_beside_a_trimmed_spline_is_measured_face_by_face() {
    let apothem = 9.5;
    let corner = apothem / 30f64.to_radians().cos();
    let pts: Vec<[f64; 2]> = (0..6)
        .map(|i| {
            [corner * (f64::from(i) * 60f64.to_radians()).cos(), corner * (f64::from(i) * 60f64.to_radians()).sin()]
        })
        .collect();
    let hexagon = Loop2 {
        edges: (0..6)
            .map(|i| ProfileEdge { seg: Seg2::Line { a: pts[i], b: pts[(i + 1) % 6] }, tag: 2 + i as u64 })
            .collect(),
    };
    let low = Frame { origin: [0.0, 0.0, -5.0], ..Frame::XY };
    let prism = Profile { frame: low, regions: vec![Region2 { outer: hexagon, holes: vec![circle_loop(5.0, 9)] }] };
    let prism = k().extrude(&prism, &ExtrudeSpec { distance: 3.0, symmetric: false }).unwrap().shape;
    // The groove: a square section screwed round the hole, past both ends, a turn at a time
    // (OCCT cuts one turn exactly).
    let spec = SweepSpec { orientation: SweepOrientation::Frenet, twist: 0.0 };
    let mut threaded = prism.clone();
    for turn in 0..5 {
        let z = -6.0 + f64::from(turn);
        let path = SweepPath::Helix {
            origin: [0.0, 0.0, z],
            axis: [0.0, 0.0, 1.0],
            x: [1.0, 0.0, 0.0],
            radius: 5.0,
            pitch: 1.0,
            height: 1.0,
        };
        let frame = Frame { origin: [5.0, 0.0, z], ..Frame::XZ };
        let groove = k().sweep(&centred_square(0.2, frame), &path, &spec).unwrap().shape;
        threaded = k().boolean(BoolOp::Cut, &threaded, &groove).unwrap().shape;
    }
    // The cone from radius 12 at z -5 to 9.6 at z -2, lofted and revolved.
    let at = |z: f64| Frame { origin: [0.0, 0.0, z], ..Frame::XY };
    let disc =
        |r: f64, z: f64| Profile { frame: at(z), regions: vec![Region2 { outer: circle_loop(r, 1), holes: vec![] }] };
    let sections = [disc(12.0, -5.0), disc(11.6, -4.0), disc(10.4, -3.0), disc(9.6, -2.0)];
    let lofted = k().loft(&sections, &LoftSpec { ruled: false }).unwrap().shape;
    let v = |sh: &Shape| k().mass_properties(sh).unwrap().volume;
    let whole = k().boolean(BoolOp::Intersect, &prism, &lofted).unwrap().shape;
    let nut = k().boolean(BoolOp::Intersect, &threaded, &lofted).unwrap().shape;
    let kinds: Vec<SurfaceKind> = k().topology(&nut).unwrap().faces.iter().map(|f| f.kind).collect();
    assert!(kinds.contains(&SurfaceKind::BSpline), "{kinds:?}");
    // The groove lies inside the loft: the nut is the cut-off prism less what the groove took.
    let want = v(&whole) - (v(&prism) - v(&threaded));
    assert!(close(v(&nut), want, 1e-8), "{} vs {want}", v(&nut));
}

/// A helical sweep's side made by a profile line parallel to the axis is an exact cylinder face
/// (3722's coil: a triangle with one side on the globe's base, apex out, a turn and a half):
/// joined to a rod of that radius it becomes part of the rod's side, so the coil's ends can be
/// chamfered. While the coil's side was a B-spline beside the rod's cylinder OCCT refused both
/// ("ChFi3d_Builder: only 2 faces", "TopOpeBRepDS_DataStructure::Point"). The rod's seam is
/// turned away from the coil's start: a blend that ends on a seam still fails.
#[test]
fn a_helical_sweep_of_an_axial_line_is_a_cylinder_face() {
    let h = 2.0 / 3f64.sqrt();
    let (r, z0) = (33.5, 4.0);
    let tri = Profile {
        frame: Frame::XZ,
        regions: vec![Region2 {
            outer: Loop2 {
                edges: [[r, z0 - h], [r, z0 + h], [r + 2.0, z0]]
                    .iter()
                    .zip([[r, z0 + h], [r + 2.0, z0], [r, z0 - h]])
                    .enumerate()
                    .map(|(i, (a, b))| ProfileEdge { seg: Seg2::Line { a: *a, b }, tag: 30 + i as u64 })
                    .collect(),
            },
            holes: vec![],
        }],
    };
    let path = SweepPath::Helix {
        origin: [0.0, 0.0, z0],
        axis: [0.0, 0.0, 1.0],
        x: [1.0, 0.0, 0.0],
        radius: r,
        pitch: 13.0 / 3.0,
        height: 6.5,
    };
    let spec = SweepSpec { orientation: SweepOrientation::Frenet, twist: 0.0 };
    let coil = k().sweep(&tri, &path, &spec).unwrap();
    let t = k().topology(&coil.shape).unwrap();
    assert!(
        t.faces.iter().any(|f| f.kind == SurfaceKind::Cylinder),
        "{:?}",
        t.faces.iter().map(|f| f.kind).collect::<Vec<_>>()
    );
    // Pappus: area x 2 pi x centroid radius per turn (section 40: 743.6554 mm3).
    let want = h * 2.0 * std::f64::consts::TAU * (r + 2.0 / 3.0) * 1.5;
    let v = |s: &Shape| k().mass_properties(s).unwrap().volume;
    // OCCT fits the slanted sides (2e-6 measured).
    assert!(close(v(&coil.shape), want, 1e-5), "{} vs {want}", v(&coil.shape));
    let rod = Profile { frame: Frame::XY, regions: vec![Region2 { outer: circle_loop(r, 1), holes: vec![] }] };
    let rod = k().extrude(&rod, &ExtrudeSpec { distance: 20.0, symmetric: false }).unwrap().shape;
    // The rod's seam a half radian from the coil's start: OCCT does not end a blend on a seam.
    let rod =
        k().transform(&rod, &Transform::Rotate { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], angle: 0.5 }).unwrap().shape;
    let joined = k().boolean(BoolOp::Union, &rod, &coil.shape).unwrap().shape;
    let rod_v = std::f64::consts::PI * r * r * 20.0;
    assert!(close(v(&joined), rod_v + want, 1e-6), "{} vs {}", v(&joined), rod_v + want);
    // The start's upper side and the end's foot on the rod (at -x, a turn and a half on).
    let t = k().topology(&joined).unwrap();
    let near = |p: [f64; 3]| {
        t.edges
            .iter()
            .position(|e| e.midpoint.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-3))
            .unwrap_or_else(|| panic!("no edge at {p:?}"))
    };
    let edges = [near([r + 1.0, 0.0, z0 + h / 2.0]) as u32, near([-r, 0.0, z0 + 6.5]) as u32];
    // The start's chamfer takes material, the end's (a concave edge) adds it.
    let start = k().chamfer(&joined, &edges[..1], 1.99).unwrap().shape;
    let end = k().chamfer(&joined, &edges[1..], 1.99).unwrap().shape;
    assert!(v(&start) < v(&joined) && v(&end) > v(&joined));
    assert!(k().chamfer(&joined, &edges, 1.99).is_ok());
}

/// 3722's coil on its globe's base, the rod turned by `turn` about its axis: the joined body,
/// and its edges at the coil's start (the upper side) and at its end's foot.
fn coil_on_rod(turn: f64) -> (Shape, [u32; 2]) {
    let h = 2.0 / 3f64.sqrt();
    let (r, z0) = (33.5, 4.0);
    let tri = Profile {
        frame: Frame::XZ,
        regions: vec![Region2 {
            outer: Loop2 {
                edges: [[r, z0 - h], [r, z0 + h], [r + 2.0, z0]]
                    .iter()
                    .zip([[r, z0 + h], [r + 2.0, z0], [r, z0 - h]])
                    .enumerate()
                    .map(|(i, (a, b))| ProfileEdge { seg: Seg2::Line { a: *a, b }, tag: 30 + i as u64 })
                    .collect(),
            },
            holes: vec![],
        }],
    };
    let path = SweepPath::Helix {
        origin: [0.0, 0.0, z0],
        axis: [0.0, 0.0, 1.0],
        x: [1.0, 0.0, 0.0],
        radius: r,
        pitch: 13.0 / 3.0,
        height: 6.5,
    };
    let spec = SweepSpec { orientation: SweepOrientation::Frenet, twist: 0.0 };
    let coil = k().sweep(&tri, &path, &spec).unwrap();
    let rod = Profile { frame: Frame::XY, regions: vec![Region2 { outer: circle_loop(r, 1), holes: vec![] }] };
    let rod = k().extrude(&rod, &ExtrudeSpec { distance: 20.0, symmetric: false }).unwrap().shape;
    let rod =
        k().transform(&rod, &Transform::Rotate { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], angle: turn }).unwrap().shape;
    let joined = k().boolean(BoolOp::Union, &rod, &coil.shape).unwrap().shape;
    let t = k().topology(&joined).unwrap();
    let near = |p: [f64; 3]| {
        t.edges
            .iter()
            .position(|e| e.midpoint.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-3))
            .unwrap_or_else(|| panic!("no edge at {p:?}"))
    };
    let edges = [near([r + 1.0, 0.0, z0 + h / 2.0]) as u32, near([-r, 0.0, z0 + 6.5]) as u32];
    (joined, edges)
}

#[test]
fn a_chamfer_that_ends_on_a_seam_is_made_with_the_seam_moved() {
    // The coil starts on the rod's seam (3722): OCCT's chamfer of the start's upper side, which
    // ends there, fails; it is made on a copy whose seam is cut away and set at the angle
    // farthest from the edges' ends. Same volumes as with the rod turned half a radian, to
    // OCCT's own spread: the start's chamfer on the rod turned by -1.5 to 0.5 rad ranges over
    // 0.0043 mm3 (6e-8 of the body).
    let v = |s: &Shape| k().mass_properties(s).unwrap().volume;
    let (on_seam, e0) = coil_on_rod(0.0);
    let (turned, e1) = coil_on_rod(0.5);
    assert!(close(v(&on_seam), v(&turned), 1e-9), "{} vs {}", v(&on_seam), v(&turned));
    for pick in [0..1, 1..2, 0..2] {
        let a = k().chamfer(&on_seam, &e0[pick.clone()], 1.99).unwrap();
        let b = k().chamfer(&turned, &e1[pick.clone()], 1.99).unwrap();
        assert!(close(v(&a.shape), v(&b.shape), 1e-7), "{pick:?}: {} vs {}", v(&a.shape), v(&b.shape));
        // Every face has an origin: a chamfer face its picked edge, the others the body's faces.
        assert!(a.face_origins.iter().all(|o| !o.is_empty()), "{pick:?}: {:?}", a.face_origins);
        for e in &e0[pick.clone()] {
            let from_edge =
                a.face_origins.iter().flatten().filter(|o| **o == FaceOrigin::FromEdge { input: 0, edge: *e });
            assert_eq!(from_edge.count(), 1, "{pick:?}: {:?}", a.face_origins);
        }
    }
}

#[test]
fn a_revolved_dome_measures_its_exact_volume_however_it_is_turned() {
    // 3722's globe: the section under an arc of radius 54 about (0.658, 61.865), from the base's
    // side (r 33.5) out and up to the axis, revolved. OCCT's adaptive quadrature put this
    // surface of revolution 7.4 mm3 (1e-5) off and 3722's coil body 0.4 mm3 off, moving with
    // the shape's place; each face is integrated by Gauss-Kronrod now.
    let (c, zc, r, x1): (f64, f64, f64, f64) = (0.658, 61.865, 54.0, 33.5);
    let z1 = zc - (r * r - (x1 - c) * (x1 - c)).sqrt();
    let top = zc + (r * r - c * c).sqrt();
    let line = |a: [f64; 2], b: [f64; 2], tag: u64| ProfileEdge { seg: Seg2::Line { a, b }, tag };
    let edges = vec![
        line([0.0, 0.0], [x1, 0.0], 1),
        line([x1, 0.0], [x1, z1], 2),
        ProfileEdge { seg: Seg2::Arc { center: [c, zc], start: [x1, z1], end: [0.0, top], ccw: true }, tag: 3 },
        line([0.0, top], [0.0, 0.0], 4),
    ];
    let p = Profile { frame: Frame::XZ, regions: vec![Region2 { outer: Loop2 { edges }, holes: vec![] }] };
    let dome = k()
        .revolve(&p, &RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::TAU })
        .unwrap()
        .shape;
    // Pappus: 2 pi times the integral of x times the section's height at x; up to x1 the height
    // is the upper half circle's, beyond it (x = c + r cos t, t from t1 down to 0) the chord's.
    let s = |u: f64| (r * r - u * u).sqrt();
    let (u0, u1) = (-c, x1 - c);
    let upper = zc * (x1 * x1 / 2.0)
        + (-(s(u1).powi(3)) + s(u0).powi(3)) / 3.0
        + c * ((u1 * s(u1) + r * r * (u1 / r).asin()) - (u0 * s(u0) + r * r * (u0 / r).asin())) / 2.0;
    let t1 = (u1 / r).acos();
    let outer = 2.0 * r * r * (c * (t1 / 2.0 - (2.0 * t1).sin() / 4.0) + r * t1.sin().powi(3) / 3.0);
    let want = std::f64::consts::TAU * (upper + outer);
    // The dome is a surface of revolution (its arc's centre is off the axis).
    let kinds: Vec<SurfaceKind> = k().topology(&dome).unwrap().faces.iter().map(|f| f.kind).collect();
    assert!(kinds.contains(&SurfaceKind::Other), "{kinds:?}");
    let v = |s: &Shape| k().mass_properties(s).unwrap().volume;
    assert!(close(v(&dome), want, 1e-9), "{} vs {want}", v(&dome));
    for angle in [0.3, 1.0, 2.5] {
        let turned =
            k().transform(&dome, &Transform::Rotate { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], angle }).unwrap().shape;
        let moved = k().transform(&turned, &Transform::Translate([40.0, -25.0, 7.0])).unwrap().shape;
        assert!(close(v(&moved), want, 1e-9), "{angle}: {} vs {want}", v(&moved));
    }
}

#[test]
fn a_shell_of_a_spindle_dome_is_its_section_offset_and_revolved() {
    // 3722's globe: a base cylinder r 33.5 mm, 19 mm tall, under a dome of r 54 mm about a
    // centre 0.658 mm off the axis (a spindle torus, a point of it on the axis), the corner
    // rounded by 2 mm; shelled 2 mm with the base opened. OCCT's thick solid fails on the
    // point; the section offset in its plane and revolved gives the cavity.
    let (cx, cz) = (0.658311, 61.865178);
    let r = ((33.5 - cx) * (33.5f64 - cx) + (19.0 - cz) * (19.0f64 - cz)).sqrt();
    let top = cz + (r * r - cx * cx).sqrt();
    let edges = vec![
        Seg2::Line { a: [0.0, 0.0], b: [33.5, 0.0] },
        Seg2::Line { a: [33.5, 0.0], b: [33.5, 19.0] },
        Seg2::Arc { center: [cx, cz], start: [33.5, 19.0], end: [0.0, top], ccw: true },
        Seg2::Line { a: [0.0, top], b: [0.0, 0.0] },
    ];
    let outer =
        Loop2 { edges: edges.into_iter().enumerate().map(|(i, seg)| ProfileEdge { seg, tag: 1 + i as u64 }).collect() };
    let p = Profile { frame: Frame::XZ, regions: vec![Region2 { outer, holes: vec![] }] };
    let spec = RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle: std::f64::consts::TAU };
    let dome = k().revolve(&p, &spec).unwrap().shape;
    let t = k().topology(&dome).unwrap();
    let rim = t.edges.iter().position(|e| e.kind == CurveKind::Circle && (e.midpoint[2] - 19.0).abs() < 1e-6).unwrap();
    let dome = k().fillet(&dome, &[rim as u32], 2.0).unwrap().shape;
    let t = k().topology(&dome).unwrap();
    let base = t.faces.iter().position(|f| f.normal[2] < -0.9).unwrap() as u32;
    let shell = k().shell(&dome, &[base], 2.0).unwrap();
    // Pappus, by Simpson's rule over the section: the wall between the outer radius (the base's
    // 33.5 mm, the round r 2 mm about its centre, the dome's circle r 54 mm) and the inner one
    // (31.5 mm, the round offset to r 4 mm, the circle r 52 mm), each up to the axis.
    let fz = cz - ((r + 2.0) * (r + 2.0) - (35.5 - cx) * (35.5 - cx)).sqrt();
    let (ux, uz) = (35.5 - cx, fz - cz);
    let tz = |w: f64| cz + (r - w) * uz / (ux * ux + uz * uz).sqrt();
    let rad = |w: f64, z: f64| {
        if z <= fz {
            33.5 - w
        } else if z <= tz(w) {
            35.5 - ((2.0 + w) * (2.0 + w) - (z - fz) * (z - fz)).max(0.0).sqrt()
        } else {
            cx + ((r - w) * (r - w) - (z - cz) * (z - cz)).max(0.0).sqrt()
        }
    };
    let solid = |w: f64| {
        let simpson = |a: f64, b: f64, n: usize| {
            let h = (b - a) / n as f64;
            let f = |z: f64| rad(w, z) * rad(w, z);
            (1..n).fold(f(a) + f(b), |s, i| s + f(a + i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 }) * h / 3.0
        };
        let top = cz + ((r - w) * (r - w) - cx * cx).sqrt();
        std::f64::consts::PI * (simpson(0.0, fz, 2) + simpson(fz, tz(w), 2000) + simpson(tz(w), top, 200_000))
    };
    let want = solid(0.0) - solid(2.0);
    // The kernel's measure of the dome's face near the axis is 1e-5 off (72500.31 against 72499.74).
    let got = k().mass_properties(&shell.shape).unwrap().volume;
    assert!(close(got, want, 2e-5), "{got} vs {want}");
    assert_eq!(k().topology(&shell.shape).unwrap().solids, 1);
    // The cavity's faces are offsets of the dome's: the wall's, the round's and the dome's.
    let offsets = shell.face_origins.iter().filter(|o| matches!(o.as_slice(), [FaceOrigin::Offset { .. }])).count();
    assert_eq!(offsets, 3, "{:?}", shell.face_origins);
}

/// A sheet whose faces have no edges: a whole torus (finite) and a whole plane (unbounded).
/// The torus sends the volume through the per-face Gauss-Kronrod integration, which crashed
/// the process on the plane's infinite domain (an imported SolidWorks sheet, RJ45-LED: its plane,
/// as placed about the point the volume is integrated round); the shape has no volume, so
/// measuring it fails cleanly.
#[test]
fn unbounded_face_has_no_volume() {
    let step = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('','',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#1=APPLICATION_CONTEXT('automotive design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('Sheet','Sheet','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('design','',#4,#5);
#7=PRODUCT_DEFINITION_SHAPE('','',#6);
#8=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#9=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));
#10=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());
#11=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#8,'distance_accuracy_value','');
#12=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#11))GLOBAL_UNIT_ASSIGNED_CONTEXT((#8,#9,#10))REPRESENTATION_CONTEXT('',''));
#20=CARTESIAN_POINT('',(0.,0.,0.));
#21=DIRECTION('',(0.,0.,1.));
#22=DIRECTION('',(1.,0.,0.));
#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);
#24=CARTESIAN_POINT('',(3.1213676516119,0.122069905615873,-1.4947664681521));
#34=DIRECTION('',(-1.,1.1102230246251533E-16,2.9170068379752627E-16));
#35=DIRECTION('',(2.9170068379752627E-16,0.,1.));
#25=AXIS2_PLACEMENT_3D('',#24,#34,#35);
#26=TOROIDAL_SURFACE('',#23,10.,2.);
#27=PLANE('',#25);
#28=ADVANCED_FACE('',(),#26,.T.);
#29=ADVANCED_FACE('',(),#27,.T.);
#30=OPEN_SHELL('',(#28,#29));
#31=SHELL_BASED_SURFACE_MODEL('',(#30));
#32=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#23,#31),#12);
#33=SHAPE_DEFINITION_REPRESENTATION(#7,#32);
ENDSEC;
END-ISO-10303-21;
";
    let bodies = k().import_step(step.as_bytes()).unwrap();
    assert_eq!(bodies.len(), 1, "one sheet");
    assert_eq!(k().topology(&bodies[0].0).unwrap().faces.len(), 2, "both faces read");
    let e = k().mass_properties(&bodies[0].0).unwrap_err();
    assert!(e.to_string().contains("unbounded"), "{e}");
}

/// One face meshed alone: every face keeps its range, only that face has triangles, as many
/// as in the whole body's mesh.
#[test]
fn tessellate_face_meshes_one_face() {
    let a = k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let faces = k().topology(&a.shape).unwrap().faces.len();
    let p = TessellationParams { linear_deflection: 0.01, angular_deflection: 0.5 };
    let whole = k().tessellate(&a.shape, &p).unwrap();
    for fi in 0..faces {
        let m = k().tessellate_face(&a.shape, fi as u32, &p).unwrap();
        assert_eq!(m.face_ranges.len(), faces);
        for (i, r) in m.face_ranges.iter().enumerate() {
            assert_eq!(r.is_empty(), i != fi, "face {i} of the mesh of face {fi}: {r:?}");
        }
        assert_eq!(m.indices.len() / 3, m.face_ranges[fi].len());
        assert_eq!(whole.face_ranges[fi].len(), m.face_ranges[fi].len());
    }
    assert!(k().tessellate_face(&a.shape, faces as u32, &p).is_err());
}
