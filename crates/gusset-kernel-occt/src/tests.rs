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
    assert!(close(m.volume, want, 1e-9), "{} vs {want}", m.volume);
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
