//! A subset of the OCCT goldens (gusset-kernel-occt/src/tests.rs) on SolidCore: volumes,
//! topology counts and face origins, plus the closed forms of the comparison in
//! docs/research/solidcore-2026-11.md.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use std::f64::consts::PI;

use super::SolidcoreKernel;
use gusset_kernel::*;

fn k() -> SolidcoreKernel {
    SolidcoreKernel::new().unwrap()
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

fn block() -> OpResult {
    k().extrude(&boxp(40.0, 20.0), &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap()
}

fn vol(s: &Shape) -> f64 {
    k().mass_properties(s).unwrap().volume
}

fn counts(s: &Shape) -> (usize, usize, usize, u32) {
    let t = k().topology(s).unwrap();
    (t.faces.len(), t.edges.len(), t.vertices.len(), t.solids)
}

/// A cylinder of radius `r` about the vertical through (x, y), from z0 up `h`.
fn cylinder(x: f64, y: f64, z0: f64, r: f64, h: f64, tag: u64) -> Shape {
    let frame = Frame { origin: [0.0, 0.0, z0], ..Frame::XY };
    let p = Profile {
        frame,
        regions: vec![Region2 {
            outer: Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [x, y], radius: r }, tag }] },
            holes: vec![],
        }],
    };
    k().extrude(&p, &ExtrudeSpec { distance: h, symmetric: false }).unwrap().shape
}

#[test]
fn extrude_box_golden() {
    let r = block();
    assert_eq!(counts(&r.shape), (6, 12, 8, 1));
    let m = k().mass_properties(&r.shape).unwrap();
    assert!(close(m.volume, 8000.0, 1e-9), "{}", m.volume);
    assert!(close(m.area, 2.0 * 800.0 + 120.0 * 10.0, 1e-9));
    assert!(close(m.centroid[2], 5.0, 1e-9));
    let t = k().topology(&r.shape).unwrap();
    let mut lat: Vec<u64> = r
        .face_origins
        .iter()
        .flatten()
        .filter_map(|o| if let FaceOrigin::Lateral { tag } = o { Some(*tag) } else { None })
        .collect();
    lat.sort_unstable();
    assert_eq!(lat, vec![10, 11, 12, 13]);
    assert!(r.face_origins.iter().all(|o| o.len() == 1));
    let fi = r.face_origins.iter().position(|o| o == &vec![FaceOrigin::Lateral { tag: 10 }]).unwrap();
    assert!(close(t.faces[fi].normal[1], -1.0, 1e-9), "{:?}", t.faces[fi].normal);
    let s = r.face_origins.iter().position(|o| matches!(o[0], FaceOrigin::Cap { end: CapEnd::Start, .. })).unwrap();
    assert!(t.faces[s].centroid[2].abs() < 1e-9);
    assert!(r.face_origins.iter().any(|o| matches!(o[0], FaceOrigin::Cap { end: CapEnd::End, .. })));
    let bb = k().bbox(&r.shape).unwrap();
    assert!(close(bb.size()[0], 40.0, 1e-6), "{bb:?}");
    // Per-face measures: the box's faces are exact.
    let areas: f64 = t.faces.iter().map(|f| f.area).sum();
    assert!(close(areas, 2800.0, 1e-9), "{areas}");
    assert!(t.edges.iter().all(|e| e.kind == CurveKind::Line && e.faces.len() == 2 && e.vertices.len() == 2));
}

#[test]
fn extrude_with_hole_and_arcs() {
    let mut region = Region2 { outer: rect(40.0, 40.0, 1), holes: vec![] };
    region
        .holes
        .push(Loop2 { edges: vec![ProfileEdge { seg: Seg2::Circle { center: [20.0, 20.0], radius: 5.0 }, tag: 99 }] });
    let p = Profile { frame: Frame::XY, regions: vec![region] };
    let r = k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: true }).unwrap();
    let expected = (1600.0 - PI * 25.0) * 10.0;
    assert!(close(vol(&r.shape), expected, 1e-9), "{} vs {expected}", vol(&r.shape));
    let bb = k().bbox(&r.shape).unwrap();
    assert!(close(bb.min[2], -5.0, 1e-6) && close(bb.max[2], 5.0, 1e-6));
    assert!(r.face_origins.iter().flatten().any(|o| *o == FaceOrigin::Lateral { tag: 99 }));
    // The hole's circles: their length and circle are exact.
    let t = k().topology(&r.shape).unwrap();
    let circ: f64 = t.edges.iter().filter(|e| e.kind == CurveKind::Circle).map(|e| e.length).sum();
    assert!(close(circ, 2.0 * 10.0 * PI, 1e-9), "{circ}");
    assert!(t.edges.iter().filter_map(|e| e.circle).all(|c| close(c.radius, 5.0, 1e-12)));
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
    let expected = (40.0 + PI * 4.0) * 2.0;
    assert!(close(vol(&r.shape), expected, 1e-9), "{} vs {expected}", vol(&r.shape));
    let mut tags: Vec<u64> = r
        .face_origins
        .iter()
        .flatten()
        .filter_map(|o| if let FaceOrigin::Lateral { tag } = o { Some(*tag) } else { None })
        .collect();
    tags.sort_unstable();
    assert_eq!(tags, vec![1, 2, 3, 4]);
}

#[test]
fn revolve_cylinder() {
    let p = Profile { frame: Frame::XZ, regions: vec![Region2 { outer: rect(5.0, 10.0, 1), holes: vec![] }] };
    let spec = |angle| RevolveSpec { axis_origin: [0.0, 0.0], axis_dir: [0.0, 1.0], angle };
    let full = k().revolve(&p, &spec(2.0 * PI)).unwrap();
    assert!(close(vol(&full.shape), PI * 250.0, 1e-9), "{}", vol(&full.shape));
    let half = k().revolve(&p, &spec(PI)).unwrap();
    assert!(close(vol(&half.shape), PI * 125.0, 1e-9), "{}", vol(&half.shape));
    assert!(half.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Cap { end: CapEnd::Start, .. })));
    assert!(half.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Cap { end: CapEnd::End, .. })));
    // The cylinder wall comes from the profile's outer vertical edge (tag 2), the top from tag 3.
    let t = k().topology(&full.shape).unwrap();
    let wall = t.faces.iter().position(|f| f.kind == SurfaceKind::Cylinder).unwrap();
    assert_eq!(full.face_origins[wall], vec![FaceOrigin::Lateral { tag: 2 }]);
    let top = t.faces.iter().position(|f| f.kind == SurfaceKind::Plane && f.centroid[2] > 9.0).unwrap();
    assert_eq!(full.face_origins[top], vec![FaceOrigin::Lateral { tag: 3 }]);
    let seams = |shape: &Shape| {
        k().topology(shape)
            .unwrap()
            .edges
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
    let a = block();
    let mut tool_p = boxp(10.0, 10.0);
    tool_p.frame.origin = [15.0, 5.0, 0.0];
    let b = k().extrude(&tool_p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap();
    let c = k().boolean(BoolOp::Cut, &a.shape, &b.shape).unwrap();
    assert!(close(vol(&c.shape), 7000.0, 1e-9), "{}", vol(&c.shape));
    assert_eq!(counts(&c.shape), (10, 24, 16, 1));
    assert!(c.face_origins.iter().all(|o| o.iter().any(|x| matches!(x, FaceOrigin::Input { .. }))));
    assert!(c.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Input { input: 1, .. })));
    // The block's top (its End cap) is still the top, now with the tool's top on it too.
    let top =
        a.face_origins.iter().position(|o| matches!(o[0], FaceOrigin::Cap { end: CapEnd::End, .. })).unwrap() as u32;
    assert!(c.face_origins.iter().any(|o| o.contains(&FaceOrigin::Input { input: 0, face: top })));
}

#[test]
fn box_minus_cylinder_and_union() {
    let a = block();
    let c = k().boolean(BoolOp::Cut, &a.shape, &cylinder(20.0, 10.0, -1.0, 5.0, 12.0, 7)).unwrap();
    assert!(close(vol(&c.shape), 8000.0 - 250.0 * PI, 1e-9), "{}", vol(&c.shape));
    // OCCT: 7/15/10. SolidCore's extruded circle is two half-cylinder faces (8/18/12).
    assert_eq!(counts(&c.shape), (8, 18, 12, 1));
    // The hole's walls come from the cylinder's side faces.
    let t = k().topology(&c.shape).unwrap();
    let walls: Vec<usize> = (0..t.faces.len()).filter(|&f| t.faces[f].kind == SurfaceKind::Cylinder).collect();
    assert_eq!(walls.len(), 2);
    for w in walls {
        assert!(!c.face_origins[w].is_empty());
        assert!(c.face_origins[w].iter().all(|o| matches!(o, FaceOrigin::Input { input: 1, .. })));
        let axis = t.faces[w].axis.unwrap();
        assert!(close(axis.dir[2].abs(), 1.0, 1e-12));
    }
    let u = k().boolean(BoolOp::Union, &a.shape, &cylinder(40.0, 10.0, -1.0, 5.0, 12.0, 7)).unwrap();
    assert!(close(vol(&u.shape), 8000.0 + 175.0 * PI, 1e-9), "{}", vol(&u.shape));
}

#[test]
fn boolean_many_cuts_and_fuses_with_history() {
    let a = block();
    let tool_at = |x: f64| {
        let mut p = boxp(5.0, 5.0);
        p.frame.origin = [x, 5.0, 0.0];
        k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap().shape
    };
    let tools = [tool_at(2.0), tool_at(12.0), tool_at(22.0)];
    let c = k().boolean_many(BoolOp::Cut, &a.shape, &tools).unwrap();
    let seq = gusset_kernel::sequential_boolean(&k(), BoolOp::Cut, &a.shape, &tools).unwrap();
    assert!(close(vol(&c.shape), 8000.0 - 3.0 * 250.0, 1e-9), "{}", vol(&c.shape));
    assert!(close(vol(&seq.shape), vol(&c.shape), 1e-9));
    assert_eq!(counts(&c.shape).0, counts(&seq.shape).0);
    for input in 0..=3u32 {
        assert!(
            c.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Input { input: i, .. } if *i == input)),
            "input {input}"
        );
    }
    assert!(c.face_origins.iter().all(|o| !o.is_empty()));
    let far = |x: f64| {
        let mut p = boxp(5.0, 5.0);
        p.frame.origin = [x, 50.0, 0.0];
        k().extrude(&p, &ExtrudeSpec { distance: 10.0, symmetric: false }).unwrap().shape
    };
    let u = k().boolean_many(BoolOp::Union, &a.shape, &[far(0.0), far(10.0)]).unwrap();
    assert!(close(vol(&u.shape), 8500.0, 1e-9));
    let i = k().boolean_many(BoolOp::Intersect, &a.shape, &[tool_at(2.0), tool_at(4.0)]).unwrap();
    assert!(close(vol(&i.shape), 150.0, 1e-9), "{}", vol(&i.shape));
    let same = k().boolean_many(BoolOp::Cut, &a.shape, &[]).unwrap();
    assert!(close(vol(&same.shape), 8000.0, 1e-12));
}

#[test]
fn fillet_and_chamfer_history() {
    let a = block();
    let t = k().topology(&a.shape).unwrap();
    let e = t.edges.iter().position(|e| (e.length - 10.0).abs() < 1e-9).unwrap() as u32;
    let f = k().fillet(&a.shape, &[e], 2.0).unwrap();
    assert!(close(vol(&f.shape), 8000.0 - (4.0 - PI) * 10.0, 1e-9), "{}", vol(&f.shape));
    assert!(f.face_origins.iter().flatten().any(|o| *o == FaceOrigin::FromEdge { input: 0, edge: e }));
    assert_eq!(counts(&f.shape).0, 7);
    // The six old faces keep their origins.
    assert_eq!(f.face_origins.iter().filter(|o| matches!(o[..], [FaceOrigin::Input { input: 0, .. }])).count(), 6);
    let c = k().chamfer(&a.shape, &[e], 1.0).unwrap();
    assert!(close(vol(&c.shape), 8000.0 - 5.0, 1e-9), "{}", vol(&c.shape));
    // OCCT refuses a round of 50 on this 10 mm edge between faces 40 and 20 wide. SolidCore
    // builds a valid body whose round runs over the 20 mm face (5 faces, not the closed form of
    // a corner round): a policy difference the router has to know about.
    let over = k().fillet(&a.shape, &[e], 50.0).unwrap();
    assert_eq!(counts(&over.shape).0, 5);
    assert!(vol(&over.shape) < 8000.0);
    assert!(k().fillet(&a.shape, &[9999], 1.0).is_err());
    let err = k().fillet(&a.shape, &[e], 0.0).unwrap_err();
    assert_eq!(err.kind, KernelErrorKind::InvalidInput);
    // The input is unchanged (operations work on copies).
    assert!(close(vol(&a.shape), 8000.0, 1e-12));
}

/// The rounded block: a box with all 12 edges filleted has a closed-form volume.
#[test]
fn filleted_block_volume() {
    let a = block();
    let all: Vec<u32> = (0..12).collect();
    let r = 2.0;
    let f = k().fillet(&a.shape, &all, r).unwrap();
    let (a2, b2, c2) = (40.0 - 2.0 * r, 20.0 - 2.0 * r, 10.0 - 2.0 * r);
    let expected = a2 * b2 * c2
        + 2.0 * r * (a2 * b2 + a2 * c2 + b2 * c2)
        + PI * r * r * (a2 + b2 + c2)
        + 4.0 / 3.0 * PI * r * r * r;
    assert!(close(vol(&f.shape), expected, 1e-9), "{} vs {expected}", vol(&f.shape));
    assert_eq!(counts(&f.shape).0, 26);
    assert!(f.face_origins.iter().all(|o| !o.is_empty()));
    let blends = f.face_origins.iter().filter(|o| matches!(o[..], [FaceOrigin::FromEdge { .. }])).count();
    assert_eq!(blends, 20, "12 rounds and 8 corner patches");
}

#[test]
fn tessellation_is_consistent() {
    let a = block();
    let m = k().tessellate(&a.shape, &TessellationParams::default()).unwrap();
    assert_eq!(m.face_ranges.len(), 6);
    assert_eq!(m.edges.len(), 12);
    // OCCT: 12 (two per planar face). SolidCore's mesher grids planar faces too.
    assert!(m.triangle_count() >= 12);
    assert_eq!(m.positions.len(), m.normals.len());
    assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
    // A curved body: every face meshed, the mesh close to the exact volume.
    let c = k().boolean(BoolOp::Cut, &a.shape, &cylinder(20.0, 10.0, -1.0, 5.0, 12.0, 7)).unwrap();
    let m =
        k().tessellate(&c.shape, &TessellationParams { linear_deflection: 0.01, angular_deflection: 0.35 }).unwrap();
    assert!(m.face_ranges.iter().all(|r| !r.is_empty()));
    let p = |i: u32| m.positions[i as usize].map(f64::from);
    let v: f64 = m
        .indices
        .chunks(3)
        .map(|t| {
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum();
    assert!(close(v, 8000.0 - 250.0 * PI, 1e-3), "{v}");
}

#[test]
fn shell_and_transform() {
    let a = block();
    let top =
        a.face_origins.iter().position(|o| matches!(o[0], FaceOrigin::Cap { end: CapEnd::End, .. })).unwrap() as u32;
    let s = k().shell(&a.shape, &[top], 1.0).unwrap();
    let inner = 38.0 * 18.0 * 9.0;
    assert!(close(vol(&s.shape), 8000.0 - inner, 1e-9), "{}", vol(&s.shape));
    assert!(s.face_origins.iter().flatten().any(|o| matches!(o, FaceOrigin::Offset { input: 0, .. })));
    let moved = k().transform(&a.shape, &Transform::Translate([1.0, 2.0, 3.0])).unwrap();
    let bb = k().bbox(&moved.shape).unwrap();
    assert!(close(bb.min[0], 1.0, 1e-9) && close(bb.min[2], 3.0, 1e-9), "{bb:?}");
    let turned = k()
        .transform(&a.shape, &Transform::Rotate { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], angle: PI / 2.0 })
        .unwrap();
    let bb = k().bbox(&turned.shape).unwrap();
    assert!(close(bb.size()[1], 40.0, 1e-9), "{bb:?}");
    let mirrored = k().transform(&a.shape, &Transform::Mirror { origin: [0.0; 3], normal: [1.0, 0.0, 0.0] }).unwrap();
    assert!(close(vol(&mirrored.shape), 8000.0, 1e-9));
    assert!(close(k().bbox(&mirrored.shape).unwrap().min[0], -40.0, 1e-9));
}

#[test]
fn results_are_deterministic() {
    let run = || {
        let a = block();
        let c = k().boolean(BoolOp::Cut, &a.shape, &cylinder(20.0, 10.0, -1.0, 5.0, 12.0, 7)).unwrap();
        let t = k().topology(&c.shape).unwrap();
        (vol(&c.shape).to_bits(), t, c.face_origins)
    };
    assert_eq!(run(), run());
}

#[test]
fn unsupported_operations_say_so() {
    let a = block();
    let e = k().compound(std::slice::from_ref(&a.shape)).unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::Unsupported);
    let e = k().export_step(&[(&a.shape, "a")]).unwrap_err();
    assert_eq!(e.kind, KernelErrorKind::Unsupported);
}
