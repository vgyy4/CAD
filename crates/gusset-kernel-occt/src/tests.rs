//! Golden geometry tests against real OCCT (volume, area, topology counts, history).

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::panic)]

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
