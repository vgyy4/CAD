//! Solid Edge feature history: each decoded rule on hand-built bytes (no sample bytes).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::fixtures::ole;
use crate::solidedge_features::{
    Curve2, Edge3, Profile, SetCurve, Store, axis_in_plane, axis_line, curve, edge_pick, edge_set, feature_tree,
    history, key_point_reach, parse_func, plane, value,
};
use serde_json::json;

/// One object: `QQ`, kind, serial, payload.
fn obj(kind: u16, serial: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = b"QQ".to_vec();
    v.extend(kind.to_le_bytes());
    v.extend(serial.to_le_bytes());
    v.extend(payload);
    v
}

/// A function: some head bytes, the signature, the class and its slots (each a list).
fn func(class: &str, slots: &[&[u32]]) -> Vec<u8> {
    let sig = "%>r".to_string() + &"%<r".repeat(slots.len().saturating_sub(1)) + "%n";
    let mut v = vec![0u8; 4];
    v.extend((sig.len() as u32 + 1).to_le_bytes());
    v.extend(sig.as_bytes());
    v.push(0);
    v.extend((class.len() as u32 + 1).to_le_bytes());
    v.extend(class.as_bytes());
    v.push(0);
    v.extend((slots.len() as u32).to_le_bytes());
    for s in slots {
        if let [one] = s {
            v.extend(0x10u32.to_le_bytes());
            v.extend(0x8000_0000u32.to_le_bytes());
            v.extend(one.to_le_bytes());
            v.extend([0u8; 16]);
        } else {
            v.extend(0x8000_0010u32.to_le_bytes());
            v.extend((s.len() as u32).to_le_bytes());
            if !s.is_empty() {
                v.extend(0x8000_0000u32.to_le_bytes());
                for x in *s {
                    v.extend(x.to_le_bytes());
                    v.extend([0u8; 16]);
                }
            }
        }
    }
    v
}

fn f64s(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// A plane object payload: u32, u8 14, origin, normal, x direction.
fn plane_payload(o: [f64; 3], n: [f64; 3], x: [f64; 3]) -> Vec<u8> {
    let mut v = vec![0u8; 4];
    v.push(0x0e);
    v.extend(f64s(&o));
    v.extend(f64s(&n));
    v.extend(f64s(&x));
    v.extend([0u8; 16]);
    v
}

fn small(x: f64) -> Vec<u8> {
    let mut v = x.to_le_bytes().to_vec();
    v.extend([0u8; 4]);
    v
}

fn line(a: [f64; 2], b: [f64; 2]) -> Vec<u8> {
    let mut v = vec![0, 0];
    v.extend(f64s(&[a[0], a[1], b[0], b[1]]));
    v.extend([1, 0, 0, 0x80]);
    v
}

#[test]
fn a_function_gives_its_class_and_slots() {
    let b = obj(5, 7, &func("JMProtrusionFunction", &[&[7], &[0], &[3, 4], &[]]));
    let f = parse_func(&b).unwrap();
    assert_eq!(f.class, "JMProtrusionFunction");
    assert_eq!(f.slots, vec![vec![7], vec![0], vec![3, 4], vec![]]);
    // A '%' with no signature before it is skipped; a bad slot tag ends the function.
    let mut b = vec![b'%', 1, 2, 3];
    b.extend(func("JMCutoutFunction", &[&[1]]));
    assert_eq!(parse_func(&b).unwrap().class, "JMCutoutFunction");
    let mut bad = func("JMCutoutFunction", &[&[1]]);
    let n = bad.len();
    bad[n - 28] = 0x11;
    assert!(parse_func(&bad).is_none());
}

#[test]
fn objects_are_cut_at_rising_serials() {
    let mut c = vec![0x44, 0xf5, 0x90, 0x6c, 0, 0, 0, 0];
    c.extend(obj(1, 9, &[1; 3]));
    c.extend(obj(1, 10, b"QQ\x01\x00\x05\x00\x00\x00abc"));
    c.extend(obj(2, 40000, &[9; 5]));
    let st = Store::new(c, &[]).unwrap();
    // The "QQ" inside object 10 names serial 5, below 10: it breaks the rise, not an object.
    // Serials may jump far.
    assert_eq!(st.obj(10).unwrap().len(), 8 + 11);
    assert_eq!(st.obj(40000).unwrap(), obj(2, 40000, &[9; 5]).as_slice());
    assert!(st.obj(5).is_none());
    assert!(Store::new(vec![1, 2, 3], &[]).is_err());
}

#[test]
fn planes_axes_and_values() {
    let p = obj(6, 1, &plane_payload([0.0, 0.0, -0.004], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]));
    assert_eq!(plane(&p), Some(([0.0, 0.0, -0.004], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0])));
    let mut a = vec![0u8; 4];
    a.push(0x03);
    a.extend(f64s(&[0.0, 0.0, 0.01, 0.0, 0.0, -2.0, 0.0, 1.0]));
    assert_eq!(axis_line(&obj(1, 2, &a)), Some(([0.0, 0.0, 0.01], [0.0, 0.0, -1.0])));
    assert_eq!(value(&obj(1, 3, &small(0.012))), Some(0.012));
    // A relation: u32 1, u32 k at +0x90, later k again and the value.
    let mut r = vec![0u8; 0x1d2 - 8];
    r[0x88..0x8c].copy_from_slice(&1u32.to_le_bytes());
    r[0x8c..0x90].copy_from_slice(&44u32.to_le_bytes());
    r[0xbe..0xc2].copy_from_slice(&44u32.to_le_bytes());
    r[0xc2..0xca].copy_from_slice(&0.0381f64.to_le_bytes());
    assert_eq!(value(&obj(1, 45, &r)), Some(0.0381));
}

#[test]
fn profile_curves() {
    assert_eq!(curve(&obj(35, 1, &line([0.0, 0.0], [0.1, 0.0]))), Some(Curve2::Line([0.0, 0.0], [0.1, 0.0])));
    let mut c = vec![0, 0];
    c.extend(f64s(&[0.0, 0.0, 0.0035, 0.0, 1.0]));
    c.extend([0, 1, 0, 0, 0x80]);
    assert_eq!(curve(&obj(28, 2, &c)), Some(Curve2::Circle([0.0, 0.0], 0.0035)));
    // An arc: centre, radius, u8 counter-clockwise, start and end angles; clockwise ones are
    // stored with their ends swapped.
    let arc = |ccw: u8| {
        let mut a = vec![0, 0];
        a.extend(f64s(&[1.0, 2.0, 0.5]));
        a.push(ccw);
        a.extend(f64s(&[0.5, 1.5]));
        a.extend([1, 0, 0, 0x80]);
        obj(37, 3, &a)
    };
    assert_eq!(curve(&arc(1)), Some(Curve2::Arc([1.0, 2.0], 0.5, 0.5, 1.5)));
    assert_eq!(curve(&arc(0)), Some(Curve2::Arc([1.0, 2.0], 0.5, 1.5, 0.5)));
    assert_eq!(curve(&obj(1, 4, &[0u8; 30])), None);
}

#[test]
fn sketch_curves_come_into_the_profile_plane() {
    let o = [0.0, 0.0, 0.0];
    let n = [0.0, -1.0, 0.0];
    let x = [1.0, 0.0, 0.0];
    // y = n x x = +z.
    let l = Edge3::Line([0.1, 0.0, 0.2], [0.3, 0.0, 0.2]);
    assert_eq!(l.flat(o, n, x), Some(Curve2::Line([0.1, 0.2], [0.3, 0.2])));
    assert_eq!(Edge3::Line([0.1, 0.5, 0.2], [0.3, 0.0, 0.2]).flat(o, n, x), None);
    // An arc counter-clockwise about -n is clockwise in the plane: its ends swap.
    let a = Edge3::Circle([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, Some(([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])));
    let Some(Curve2::Arc(_, r, a0, a1)) = a.flat(o, n, x) else { panic!() };
    assert_eq!(r, 1.0);
    assert!((a0 - std::f64::consts::FRAC_PI_2).abs() < 1e-12 && a1.abs() < 1e-12);
}

/// A part with one protrusion: a 20 x 10 mm rectangle on XY, `side` and depth 5 mm.
fn part(side: f64, extent: f64) -> Vec<u8> {
    part2(side, extent, None)
}

/// The same with a second extent: side and depth 2 mm.
fn part2(side: f64, extent: f64, second: Option<f64>) -> Vec<u8> {
    part_with(side, extent, second, &[])
}

/// The same with more objects after the thread (serials above 18).
fn part_with(side: f64, extent: f64, second: Option<f64>, more: &[u8]) -> Vec<u8> {
    let mut c = vec![0x44, 0xf5, 0x90, 0x6c, 0, 0, 0, 0];
    c.extend(obj(6, 1, &plane_payload([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])));
    // The profile: a function whose slot 0 is the plane, then "Profile2d" naming object 3.
    let mut prof = func("JProfileRecomputeEx", &[&[1], &[2]]);
    prof.extend(3u32.to_le_bytes());
    prof.push(9 * 8);
    prof.extend("Profile2d".encode_utf16().flat_map(u16::to_le_bytes));
    c.extend(obj(26, 2, &prof));
    let mut p2 = 4u32.to_le_bytes().to_vec();
    for m in [4u32, 5, 6, 7] {
        p2.extend(m.to_le_bytes());
    }
    c.extend(obj(65, 3, &p2));
    let pts = [[0.0, 0.0], [0.02, 0.0], [0.02, 0.01], [0.0, 0.01]];
    for (k, s) in (4u32..8).enumerate() {
        c.extend(obj(35, s, &line(pts[k], pts[(k + 1) % 4])));
    }
    c.extend(obj(36, 8, &small(extent)));
    c.extend(obj(36, 9, &small(side)));
    c.extend(obj(36, 10, &small(0.005)));
    let mut slots: Vec<&[u32]> = vec![&[11], &[0], &[2], &[8], &[9], &[0], &[10]];
    if second.is_some() {
        slots.extend([&[0u32] as &[u32], &[0], &[0], &[0], &[0], &[16], &[17], &[18]]);
    }
    c.extend(obj(38, 11, &func("JMProtrusionFunction", &slots)));
    c.extend(obj(38, 12, &func("JMThreadFunction", &[&[12], &[11]])));
    c.extend(obj(36, 16, &small(13.0)));
    c.extend(obj(36, 17, &small(second.unwrap_or(0.0))));
    c.extend(obj(36, 18, &small(0.002)));
    c.extend(more);
    ole(&[("PSMcluster0", &c), ("PSMtypetable", &[])])
}

#[test]
fn a_protrusion_becomes_a_sketch_and_an_extrude() {
    let t = feature_tree(&part(1.0, 13.0), "dir/p.par", &crate::Limits::default()).unwrap();
    let f = t["features"].as_array().unwrap();
    assert_eq!(f.len(), 2, "{t}");
    assert_eq!(f[0]["type"], "sketch");
    assert_eq!(f[0]["plane"], json!({"origin": [0.0, 0.0, 0.0], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]}));
    assert_eq!(f[0]["curves"].as_array().unwrap().len(), 4);
    assert_eq!(f[0]["points"].as_array().unwrap().len(), 4);
    assert_eq!(f[1]["type"], "extrude");
    assert_eq!(f[1]["operation"], "new");
    // Side 1: against the plane normal.
    assert_eq!(f[1]["distance"], json!(-5.0));
    assert_eq!(t["source"]["file"], "p.par");
    assert_eq!(t["source"]["omitted"][0]["why"], "cosmetic thread");
    let t = feature_tree(&part(3.0, 13.0), "p.par", &crate::Limits::default()).unwrap();
    assert_eq!(t["features"][1]["symmetric"], json!(true));
    assert_eq!(t["features"][1]["distance"], json!(5.0));
    // Two sides: the second reaches 2 mm the other way.
    let t = feature_tree(&part2(2.0, 13.0, Some(1.0)), "p.par", &crate::Limits::default()).unwrap();
    assert_eq!(t["features"][1]["distance"], json!(5.0));
    assert_eq!(t["features"][1]["distance2"], json!(-2.0));
    let t = feature_tree(&part2(2.0, 13.0, Some(2.0)), "p.par", &crate::Limits::default()).unwrap();
    assert_eq!(t["features"][0]["type"], "unsupported");
    // An extent not read: the feature says so.
    let t = feature_tree(&part(2.0, 99.0), "p.par", &crate::Limits::default()).unwrap();
    assert_eq!(t["features"][0]["type"], "unsupported");
    assert!(t["features"][0]["reason"].as_str().unwrap().contains("extent 99"));
}

#[test]
fn the_history_follows_previous_feature_links() {
    let mut c = vec![0u8; 8];
    c.extend(obj(38, 20, &func("JMCutoutFunction", &[&[20], &[30]])));
    c.extend(obj(38, 30, &func("JMProtrusionFunction", &[&[30], &[0]])));
    c.extend(obj(38, 31, &func("JMTransmigrateAssocFunction", &[&[31], &[20]])));
    let st = Store::new(c, &[]).unwrap();
    let h: Vec<u32> = history(&st).unwrap().into_iter().map(|(k, _)| k).collect();
    assert_eq!(h, vec![30, 20]);
}

#[test]
fn a_part_without_ordered_features_has_no_history() {
    let mut c = vec![0u8; 8];
    c.extend(obj(38, 5, &func("JMHoleFunction", &[&[5], &[0]])));
    let f = ole(&[("PSMcluster0", &c), ("PSMtypetable", &[])]);
    assert_eq!(feature_tree(&f, "s.par", &crate::Limits::default()), Err(String::new()));
    assert!(feature_tree(b"not ole", "s.par", &crate::Limits::default()).is_err());
}

#[test]
fn a_polynomial_spline_member() {
    let mut b = vec![0, 0];
    b.extend(1u32.to_le_bytes());
    b.extend(0x8000_0001u32.to_le_bytes());
    b.extend(4u32.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(f64s(&[0.0, 0.0, 0.01, 0.02, 0.03, 0.02, 0.04, 0.0]));
    b.push(4);
    b.extend(2u32.to_le_bytes());
    b.extend(4u32.to_le_bytes());
    b.extend(4u32.to_le_bytes());
    b.extend(f64s(&[0.0, 1.0]));
    b.extend([0u8; 20]);
    let Some(Curve2::Spline(3, poles, knots)) = curve(&obj(51, 9, &b)) else { panic!() };
    assert_eq!(poles.len(), 4);
    assert_eq!(knots, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    // Rational (2 at +10) or with a tail of another length: not read.
    let mut r = b.clone();
    r[2] = 2;
    assert_eq!(curve(&obj(51, 9, &r)), None);
    b.push(0);
    assert_eq!(curve(&obj(51, 9, &b)), None);
}

#[test]
fn a_key_point_extent_stores_its_reach() {
    let mut b = func("JMLinearKeyPointFunction", &[&[3], &[1], &[2], &[0]]);
    b.extend(1u32.to_le_bytes());
    b.extend(f64s(&[0.0025, 0.0, 0.0, -2.0]));
    assert_eq!(key_point_reach(&obj(1, 3, &b)), Some((0.0025, [0.0, 0.0, -1.0])));
    let other = func("JMCutoutFunction", &[&[3]]);
    assert_eq!(key_point_reach(&obj(1, 3, &other)), None);
}

/// An edge set's curve list: a head, the list's class id, `u32 1, n, 0, u8 2` and the curves
/// (class id, type, numbers, `u32 0`; `extra` adds a second zero word after each but the last).
fn edge_set_bytes(curves: &[(u8, Vec<f64>)], extra: bool) -> Vec<u8> {
    let mut v = b"\x16\x00\x00\x00SEGetRoundableEdgeSeq\0".to_vec();
    v.push(0xff);
    v.extend([0xf0, 0xcb, 0xaa, 0xf5, 0x51, 0xd1]);
    v.extend([0xcd, 0x11, 0x8b, 0xa4, 0x08, 0x00, 0x36, 0x24, 0xfc, 0x01]);
    v.extend(1u32.to_le_bytes());
    v.extend((curves.len() as u32).to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.push(2);
    for (k, (t, nums)) in curves.iter().enumerate() {
        v.extend([0x74, 0x0c, 0x54, 0x25, 0x15]);
        v.extend([0x3a, 0xce, 0x11, 0x9a, 0x5e, 0x08, 0x00, 0x36, 0x9b, 0x41, 0x02]);
        v.push(*t);
        v.extend(f64s(nums));
        v.extend(0u32.to_le_bytes());
        if extra && k + 1 < curves.len() {
            v.extend(0u32.to_le_bytes());
        }
    }
    v.extend([2, 0, 0, 0]);
    v
}

#[test]
fn an_edge_set_lists_its_curves() {
    let line = (3u8, vec![0.001, 0.002, 0.0, -1.0, 0.0, 0.0, 0.0, 0.004]);
    let circle = (15u8, vec![0.0, 0.0, 0.002, 0.0, 0.0, -1.0, 0.005, 0.0, 0.0, 1.0]);
    let arc = (16u8, vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.003, 0.0, 1.0, 0.0, core::f64::consts::FRAC_PI_2]);
    for extra in [false, true] {
        let set = edge_set(&edge_set_bytes(&[line.clone(), circle.clone(), arc.clone()], extra)).unwrap();
        assert_eq!(set.len(), 3);
        assert_eq!(set[0], SetCurve::Line([0.001, 0.002, 0.0], [-1.0, 0.0, 0.0], 0.0, 0.004));
        assert_eq!(set[1], SetCurve::Conic([0.0, 0.0, 0.002], [0.0, 0.0, -1.0], [0.005, 0.0, 0.0], 1.0, None));
        // The line's pick: its middle (x 1 - 2 = -1 mm) and 0.1 mm either way.
        let p = edge_pick(&set[0]).unwrap();
        assert_eq!(p["midpoint"], json!([-1.0, 2.0, 0.0]));
        assert_eq!(p["path"], json!([[-0.9, 2.0, 0.0], [-1.1, 2.0, 0.0]]));
        // A quarter arc of radius 3 from +y (major axis) toward -x (normal x major).
        let p = edge_pick(&set[2]).unwrap();
        let r = 3.0 * core::f64::consts::FRAC_1_SQRT_2;
        let m = p["midpoint"].as_array().unwrap();
        assert!((m[0].as_f64().unwrap() + r).abs() < 1e-9 && (m[1].as_f64().unwrap() - r).abs() < 1e-9);
        // A whole circle: a point of it, at the radius from the centre in its plane.
        let p = edge_pick(&set[1]).unwrap();
        let m: Vec<f64> = p["midpoint"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert!((libm::hypot(m[0], m[1]) - 5.0).abs() < 1e-9 && (m[2] - 2.0).abs() < 1e-9);
    }
    // An ellipse, an unknown curve type and a list without curves are refused.
    let ellipse = (15u8, vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.5]);
    assert!(edge_pick(&edge_set(&edge_set_bytes(&[ellipse], false)).unwrap()[0]).is_err());
    assert!(edge_set(&edge_set_bytes(&[(27, vec![0.0; 4])], false)).is_err());
    assert!(edge_set(&edge_set_bytes(&[], false)).is_err());
    assert!(edge_set(b"QQ no list").is_err());
    // Cut short anywhere: an error, never a panic.
    let b = edge_set_bytes(&[line, circle], true);
    for n in 0..b.len() {
        let _ = edge_set(&b[..n]);
    }
}

#[test]
fn rounds_and_chamfers_pick_their_stored_edges() {
    // The protrusion part, then a round of r 1 mm (kind 1) and an equal chamfer of 0.5 mm
    // (kind 41) on the box's edge along x at y = 0, z = 0.
    let set = edge_set_bytes(&[(3, vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.02])], false);
    let mut more = obj(40, 21, &set);
    more.extend(obj(36, 22, &small(1.0)));
    more.extend(obj(36, 23, &small(0.001)));
    more.extend(obj(38, 24, &func("JMRoundFunction", &[&[24], &[12], &[21], &[22], &[23]])));
    more.extend(obj(36, 25, &small(41.0)));
    more.extend(obj(36, 26, &small(0.0005)));
    more.extend(obj(38, 27, &func("JMChamferFunction", &[&[27], &[24], &[25], &[0], &[21], &[26]])));
    // A round of another kind (variable radius) is not read.
    more.extend(obj(36, 28, &small(17.0)));
    more.extend(obj(38, 29, &func("JMRoundFunction", &[&[29], &[27], &[21], &[28], &[23]])));
    let t = feature_tree(&part_with(1.0, 13.0, None, &more), "p.par", &crate::Limits::default()).unwrap();
    let f = t["features"].as_array().unwrap();
    assert_eq!(f.len(), 5, "{t}");
    assert_eq!(f[2]["type"], "fillet");
    assert_eq!(f[2]["radius"], json!(1.0));
    assert_eq!(f[2]["edges"], json!([{"midpoint": [10.0, 0.0, 0.0], "path": [[9.9, 0.0, 0.0], [10.1, 0.0, 0.0]]}]));
    assert_eq!(f[3]["type"], "chamfer");
    assert_eq!(f[3]["distance"], json!(0.5));
    assert_eq!(f[3]["edges"], f[2]["edges"]);
    assert_eq!(f[4]["type"], "unsupported");
    assert!(f[4]["reason"].as_str().unwrap().contains("round kind"), "{t}");
}

#[test]
fn a_revolve_axis_off_the_sketch_axes_is_a_construction_line() {
    // A profile on the plane y = 2 (normal -y, x along x): the importer's sketch origin is the
    // foot of the world origin, (0, 2, 0).
    let p = Profile { origin: [5.0, 2.0, 0.0], normal: [0.0, -1.0, 0.0], x: [1.0, 0.0, 0.0], curves: Vec::new() };
    // The plane's z axis through that foot: one of the sketch's own axes.
    assert_eq!(axis_in_plane(&p, ([0.0, 2.0, 7.0], [0.0, 0.0, 1.0])), None);
    // Parallel to it 3 m off: a line in the plane's frame (y = normal x x = z).
    assert_eq!(axis_in_plane(&p, ([3.0, 2.0, 0.0], [0.0, 0.0, 1.0])), Some(([-2.0, 0.0], [-2.0, 1.0])));
    // Out of the plane: none.
    assert_eq!(axis_in_plane(&p, ([3.0, 2.5, 0.0], [0.0, 0.0, 1.0])), None);
}
