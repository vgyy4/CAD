//! Solid Edge feature history: each decoded rule on hand-built bytes (no sample bytes).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::fixtures::ole;
use crate::solidedge_features::{
    Curve2, Edge3, Profile, SetCurve, Store, axis_in_plane, axis_line, close_open, copy_frame, curve, edge_pick,
    edge_set, feature_tree, history, key_point_reach, parse_func, plane, value,
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
    if extent == 72.0 {
        // Up to a key point 4 mm along +z.
        let mut kp = func("JMLinearKeyPointFunction", &[&[10], &[1], &[2], &[0]]);
        kp.extend(1u32.to_le_bytes());
        kp.extend(f64s(&[0.004, 0.0, 0.0, 1.0]));
        c.extend(obj(37, 10, &kp));
    } else {
        c.extend(obj(36, 10, &small(0.005)));
    }
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
    // Drawn through points (2 at +10, flags 1): the same curve, the points' data after it.
    let mut r = b.clone();
    r[2] = 2;
    r[6] = 1;
    r[9] = 0;
    r.extend([1, 1, 2, 0, 0, 0]);
    r.extend(f64s(&[0.0, 1.0]));
    assert!(matches!(curve(&obj(51, 9, &r)), Some(Curve2::Spline(3, ..))));
    // Another kind, other flags, a tail not opening with a zero word, a short one: not read.
    for (at, v) in [(2, 3), (6, 5), (111, 1)] {
        let mut x = b.clone();
        x[at] = v;
        assert_eq!(curve(&obj(51, 9, &x)), None, "{at}");
    }
    b.pop();
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
    // One side: the reach along the normal; symmetric (side 3): that reach each way.
    let t = feature_tree(&part(2.0, 72.0), "p.par", &crate::Limits::default()).unwrap();
    assert_eq!((&t["features"][1]["distance"], &t["features"][1]["symmetric"]), (&json!(4.0), &json!(false)), "{t}");
    let t = feature_tree(&part(3.0, 72.0), "p.par", &crate::Limits::default()).unwrap();
    assert_eq!((&t["features"][1]["distance"], &t["features"][1]["symmetric"]), (&json!(8.0), &json!(true)), "{t}");
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
    let p = Profile {
        origin: [5.0, 2.0, 0.0],
        normal: [0.0, -1.0, 0.0],
        x: [1.0, 0.0, 0.0],
        curves: Vec::new(),
        turned: false,
        backwards: Vec::new(),
    };
    // The plane's z axis through that foot: one of the sketch's own axes.
    assert_eq!(axis_in_plane(&p, ([0.0, 2.0, 7.0], [0.0, 0.0, 1.0])), None);
    // Parallel to it 3 m off: a line in the plane's frame (y = normal x x = z).
    assert_eq!(axis_in_plane(&p, ([3.0, 2.0, 0.0], [0.0, 0.0, 1.0])), Some(([-2.0, 0.0], [-2.0, 1.0])));
    // Out of the plane: none.
    assert_eq!(axis_in_plane(&p, ([3.0, 2.5, 0.0], [0.0, 0.0, 1.0])), None);
}

/// A profile copied by a pattern or mirror (`from`, slot 1): header, the members, a stand-in
/// for the curve copies, the copy's plane (origin, normal, x) and the bytes that end it.
fn copied(members: &[u32], o: [f64; 3], n: [f64; 3], x: [f64; 3], from: u32) -> Vec<u8> {
    let mut v = vec![0x20, 0, 0, 2];
    v.extend((members.len() as u32).to_le_bytes());
    for m in members {
        v.extend(m.to_le_bytes());
    }
    v.extend([1, 0, 0, 0, 1, 2, 0x67]);
    v.extend(f64s(&[0.0; 6]));
    v.extend(f64s(&o));
    v.extend(f64s(&n));
    v.extend(f64s(&x));
    v.extend([0, 0, 0, 0, 0, 1, 1, 0, 0, 0]);
    v.extend([0x40, 0x11, 0x7d, 0xba, 0x44, 0x76, 0x1b, 0x10, 0xac, 0x07, 0x08, 0x00, 0x36, 0x01, 0xb1, 0x4e]);
    v.extend(func("JMDummyComputeFunction", &[&[0], &[from]]));
    v
}

#[test]
fn a_pattern_copied_profile_lies_on_its_stored_plane() {
    let b = copied(&[4], [0.0, 0.0, 0.01], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], 31);
    assert_eq!(copy_frame(&b), Some(([0.0, 0.0, 0.01], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0])));
    // Not unit or not square: none.
    assert_eq!(copy_frame(&copied(&[4], [0.0; 3], [0.0, 0.0, 2.0], [0.0, 1.0, 0.0], 31)), None);
    assert_eq!(copy_frame(&copied(&[4], [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.6, 0.8], 31)), None);
    // A cutout of the protrusion's rectangle copied by a smart pattern onto z = 10 mm, turned a
    // quarter; then one copied by a mirror in the plane x = 5 mm (its seed: the first cutout).
    let mut more = obj(40, 30, &copied(&[4, 5, 6, 7], [0.0, 0.0, 0.01], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], 31));
    more.extend(obj(38, 31, &func("JMSmartPatternFunction", &[&[31], &[12]])));
    more.extend(obj(36, 32, &small(14.0)));
    more.extend(obj(36, 33, &small(1.0)));
    more.extend(obj(38, 34, &func("JMCutoutFunction", &[&[34], &[12], &[30], &[32], &[33]])));
    more.extend(obj(40, 35, &copied(&[4], [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 34)));
    more.extend(obj(6, 38, &plane_payload([0.005, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])));
    // The seed of the mirror: a cutout of the protrusion's own profile.
    more.extend(obj(38, 39, &func("JMCutoutFunction", &[&[39], &[0], &[2], &[32], &[33]])));
    more.extend(obj(38, 40, &func("JMMirrorCopyFunction", &[&[40], &[12], &[38], &[39]])));
    more.extend(obj(38, 41, &func("JMCutoutFunction", &[&[41], &[34], &[42], &[32], &[33]])));
    more.extend(obj(40, 42, &copied(&[4, 5, 6, 7], [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 40)));
    // A copy by anything else is not read.
    more.extend(obj(38, 43, &func("JMCutoutFunction", &[&[43], &[41], &[35], &[32], &[33]])));
    let t = feature_tree(&part_with(1.0, 13.0, None, &more), "p.par", &crate::Limits::default()).unwrap();
    let f = t["features"].as_array().unwrap();
    assert_eq!(f[2]["type"], "sketch", "{t}");
    assert_eq!(
        f[2]["plane"],
        json!({"origin": [0.0, 0.0, 10.0], "x_axis": [0.0, 1.0, 0.0], "y_axis": [-1.0, 0.0, 0.0]})
    );
    assert_eq!(f[2]["curves"].as_array().unwrap().len(), 4);
    assert_eq!(f[3]["type"], "extrude");
    assert_eq!(f[3]["operation"], "cut");
    // The mirrored rectangle's first side, (0, 0)-(20, 0) mm on the plane z = 0, lies at x 10
    // to -10 mm: on the reflected plane (origin x = 10 mm, x direction -x), turned over.
    assert_eq!(
        f[4]["plane"],
        json!({"origin": [10.0, 0.0, 0.0], "x_axis": [-1.0, 0.0, 0.0], "y_axis": [0.0, -1.0, 0.0]})
    );
    assert_eq!(f[4]["points"][1], json!({"id": "p1", "x": 20.0, "y": 0.0}));
    assert_eq!(f[5]["type"], "extrude");
    assert_eq!(f[6]["type"], "unsupported");
    assert!(f[6]["reason"].as_str().unwrap().contains("copied by a Cutout"), "{t}");
}

#[test]
fn an_open_cutout_profile_is_closed_on_its_side() {
    // A line from (0, 0) to (2, 0): side 1 closes it round its left (+y), 2 round its right.
    for (side, y) in [(1, 1.0), (2, -1.0)] {
        let mut c = vec![Curve2::Line([0.0, 0.0], [2.0, 0.0])];
        close_open(&mut c, &[false], Some(side)).unwrap();
        assert_eq!(c.len(), 4, "{c:?}");
        let Curve2::Arc(centre, r, a0, a1) = c[2] else { panic!("{c:?}") };
        assert_eq!((centre, r), ([1.0, 0.0], 11.0));
        // Counter-clockwise from a0 to a1 through +y (side 1) or -y (side 2).
        let mid = if a1 > a0 { (a0 + a1) / 2.0 } else { (a0 + a1) / 2.0 + core::f64::consts::PI };
        assert!(libm::sin(mid) * y > 0.99, "{c:?}");
    }
    // A half circle drawn clockwise over the top from (-1, 0) to (1, 0) (kept counter-clockwise
    // from its end): its left is outside, its right the inside of the arch, closed below.
    let arc = Curve2::Arc([0.0, 0.0], 1.0, 0.0, core::f64::consts::PI);
    let mut c = vec![arc.clone()];
    close_open(&mut c, &[true], Some(2)).unwrap();
    let Curve2::Line(p, q) = c[1] else { panic!("{c:?}") };
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
    assert!(near(p, [-1.0, 0.0]) && near(q, [-1.0, -libm::sqrt(11.0 * 11.0 - 1.0)]), "{c:?}");
    let Curve2::Arc(_, _, a0, a1) = c[2] else { panic!("{c:?}") };
    assert!(a0 < -core::f64::consts::FRAC_PI_2 && a1 > -core::f64::consts::FRAC_PI_2 && a1 < 0.0, "{c:?}");
    // A closed profile is left alone; side unknown or three ends: refused.
    let mut c = vec![Curve2::Circle([0.0, 0.0], 1.0)];
    close_open(&mut c, &[false], None).unwrap();
    assert_eq!(c.len(), 1);
    assert!(close_open(&mut vec![arc], &[false], Some(4)).is_err());
    let three = [
        Curve2::Line([0.0, 0.0], [1.0, 0.0]),
        Curve2::Line([1.0, 0.0], [2.0, 0.0]),
        Curve2::Line([1.0, 0.0], [1.0, 1.0]),
    ];
    assert!(close_open(&mut three.to_vec(), &[false; 3], Some(1)).is_err());
}

#[test]
fn a_sweep_of_one_section_along_one_path() {
    // The rectangle swept along itself (only the layout matters here); two sections: not read.
    let mut more = obj(38, 30, &func("JMMultiSweepProtrusionFunction", &[&[30], &[12], &[2], &[2]]));
    more.extend(obj(38, 31, &func("JMMultiSweepCutoutFunction", &[&[31], &[30], &[2], &[2, 2]])));
    let t = feature_tree(&part_with(1.0, 13.0, None, &more), "p.par", &crate::Limits::default()).unwrap();
    let f = t["features"].as_array().unwrap();
    assert_eq!(f[2]["name"], "MultiSweepProtrusion_1_Path", "{t}");
    assert_eq!(f[3]["name"], "MultiSweepProtrusion_1_Sketch");
    assert_eq!(
        f[4],
        json!({"type": "sweep", "name": "MultiSweepProtrusion_1", "sketch": "MultiSweepProtrusion_1_Sketch",
               "path": "MultiSweepProtrusion_1_Path", "operation": "join", "target": "Protrusion_1"})
    );
    assert_eq!(f[5]["type"], "unsupported");
    assert!(f[5]["reason"].as_str().unwrap().contains("2 cross sections"), "{t}");
}

#[test]
fn an_edge_set_spline_is_picked_along_its_whole_length() {
    // A cubic Bezier from (0, 0, 0) to (3, 0, 0) mm bulging to y 1.5 mm, then a line: the
    // spline's numbers are not all f64-aligned (u32 counts, a byte tail).
    let mut nums = Vec::new();
    nums.extend(4u32.to_le_bytes());
    nums.extend(f64s(&[0.0, 0.0, 0.0, 0.001, 0.002, 0.0, 0.002, 0.002, 0.0, 0.003, 0.0, 0.0]));
    nums.extend(0u32.to_le_bytes());
    nums.extend(8u32.to_le_bytes());
    nums.extend(f64s(&[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]));
    nums.extend(f64s(&[-1.0]));
    nums.extend([0, 4, 1, 1, 0]);
    let mut b = edge_set_bytes(&[(3, vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.001])], false);
    // The spline goes first: rebuild the list by hand with a count-and-zero tail after it.
    let at = b.windows(4).position(|w| w == [0x74, 0x0c, 0x54, 0x25]).unwrap();
    let mut v = b[..at].to_vec();
    v[at - 13 + 4] = 2;
    v.extend([0x2e, 0x54, 0xc8, 0x71, 0x16]);
    v.extend([0x3a, 0xce, 0x11, 0x9a, 0x5e, 0x08, 0x00, 0x36, 0x9b, 0x41, 0x02]);
    v.push(27);
    v.extend(&nums);
    v.extend(1u32.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend(b.split_off(at));
    let set = edge_set(&v).unwrap();
    assert_eq!(set.len(), 2);
    let SetCurve::Spline(4, poles, knots) = &set[0] else { panic!("{set:?}") };
    assert_eq!((poles.len(), knots.len()), (4, 8));
    let p = edge_pick(&set[0]).unwrap();
    // The middle (t = 0.5): (1.5, 1.5, 0) mm; the path runs from one end to the other.
    assert_eq!(p["midpoint"], json!([1.5, 1.5, 0.0]));
    let path = p["path"].as_array().unwrap();
    assert!(path.len() > 2);
    assert_eq!((&path[0], path.last().unwrap()), (&json!([0.0, 0.0, 0.0]), &json!([3.0, 0.0, 0.0])));
    assert!(matches!(set[1], SetCurve::Line(..)));
    // Weights: not read.
    let w = v.windows(4).position(|w| w == 8u32.to_le_bytes()).unwrap() - 4;
    let mut r = v.clone();
    r[w] = 1;
    assert!(edge_set(&r).unwrap_err().contains("rational"));
}
