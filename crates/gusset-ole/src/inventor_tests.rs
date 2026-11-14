//! Tests of the Inventor feature reader on synthetic parts (records built here from the
//! layout notes in `docs/formats/inventor-feature-tree.md`).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::cast_possible_truncation)]

use crate::Limits;
use crate::fixtures::{dc_segment_blocks, dc_segment_packed, inventor_dc_part, ole};
use crate::inventor_features::feature_tree;
use serde_json::{Value, json};

// Class indices of the synthetic segment.
const NAME: usize = 0;
const PLACEMENT: usize = 1;
const POINT: usize = 2;
const SKETCH: usize = 3;
const LINE: usize = 4;
const OTHER: usize = 5;
const PROFILE: usize = 6;
const PARAM: usize = 7;
const FEATURE: usize = 8;
const ORDER: usize = 9;
const BODY_NAME: usize = 10;
const CLASSES: [u32; 11] = [
    0x2b48_a42b,
    0x9087_4d18,
    0xce52_df35,
    0x9087_4d11,
    0xce52_df3a,
    0x1111_1111,
    0xf988_4c43,
    0x9087_4d26,
    0x9087_4d91,
    0x4521_21b6,
    0x656d_d01e,
];

// Type GUIDs (first 32 bits) of the features.
const T_EXTRUDE: u32 = 0x0ca9_1131;
const T_HOLE: u32 = 0x1f75_7d1a;
const T_THREAD: u32 = 0x28aa_1906;
const T_FILLET: u32 = 0xf1f7_15dc;
const T_THICKEN: u32 = 0x7281_f4ec;
const T_SHELL: u32 = 0xb83c_9612;
const T_SWEEP: u32 = 0x90ba_da05;

fn rf(i: usize) -> u32 {
    (i as u32 + 1) | 0x8000_0000
}

fn hdr(name_ref: u32, flags: u8, id: u32) -> Vec<u8> {
    let mut v = vec![0u8; 22];
    v[6..10].copy_from_slice(&name_ref.to_le_bytes());
    v[10] = flags;
    v[18..22].copy_from_slice(&id.to_le_bytes());
    v
}

fn list(items: &[u32]) -> Vec<u8> {
    let mut v = vec![0x02, 0, 0, 0x30];
    v.extend((items.len() as u32).to_le_bytes());
    if !items.is_empty() {
        v.extend((items.len() as u32).to_le_bytes());
        v.extend(0x10u32.to_le_bytes());
        for i in items {
            v.extend(i.to_le_bytes());
        }
    }
    v
}

fn utf16(s: &str) -> Vec<u8> {
    let mut v = (s.chars().count() as u32).to_le_bytes().to_vec();
    for u in s.encode_utf16() {
        v.extend(u.to_le_bytes());
    }
    v
}

fn name_rec(s: &str, typ: Option<u32>) -> Vec<u8> {
    let mut v = vec![0u8; 0x1a];
    v.extend(utf16(s));
    if let Some(t) = typ {
        v.extend(t.to_le_bytes());
        v.extend([0x11u8; 12]);
    }
    v
}

fn param(name: &str, value: f64) -> Vec<u8> {
    let mut v = hdr(0, 0, 0);
    v.extend(utf16(name));
    v.extend([0u8; 12]);
    v.extend(value.to_le_bytes());
    v.extend(value.to_le_bytes());
    v.extend([0, 0, 0xff, 0xff]);
    v
}

fn tail(n: u16) -> Vec<u8> {
    let mut v = hdr(0, 0, 0);
    v.extend(n.to_le_bytes());
    v
}

fn point(owner: usize, u: f64, v: f64) -> Vec<u8> {
    let mut r = hdr(0, 0, 0);
    r.extend(0u32.to_le_bytes());
    r.extend(rf(owner).to_le_bytes());
    r.extend(u.to_le_bytes());
    r.extend(v.to_le_bytes());
    r
}

fn line(owner: usize, a: usize, b: usize) -> Vec<u8> {
    let mut r = hdr(0, 0, 0);
    r.extend(0u32.to_le_bytes());
    r.extend(rf(owner).to_le_bytes());
    r.extend(list(&[rf(a), rf(b)]));
    r
}

fn feature(name_ref: usize, flags: u8, id: u32, slots: &[u32]) -> Vec<u8> {
    let mut r = hdr(rf(name_ref), flags, id);
    r.extend(0u32.to_le_bytes());
    r.extend(list(slots));
    r
}

/// A 20 x 10 mm rectangle (in cm) on the XY plane, extruded 5 mm. Record 0..=16, the extrude
/// feature is record 18 (its name 1), `more` follows.
fn base_records() -> Vec<(usize, Vec<u8>)> {
    let mut placement = hdr(0, 0, 0);
    placement.extend(0x7bde_8421u32.to_le_bytes());
    vec![
        (NAME, name_rec("Sketch1", None)),               // 0
        (NAME, name_rec("Extrusion1", Some(T_EXTRUDE))), // 1
        (PLACEMENT, placement),                          // 2
        (POINT, point(7, 0.0, 0.0)),                     // 3
        (POINT, point(7, 2.0, 0.0)),                     // 4
        (POINT, point(7, 2.0, 1.0)),                     // 5
        (POINT, point(7, 0.0, 1.0)),                     // 6
        (SKETCH, {
            let mut s = hdr(rf(0), 0, 0);
            s.extend(rf(2).to_le_bytes());
            s
        }), // 7
        (LINE, line(7, 3, 4)),                           // 8
        (LINE, line(7, 4, 5)),                           // 9
        (LINE, line(7, 5, 6)),                           // 10
        (LINE, line(7, 6, 3)),                           // 11
        (OTHER, {
            let mut l = hdr(0, 0, 0);
            l.extend(list(&[]));
            l
        }), // 12 loops
        (PROFILE, {
            let mut p = hdr(0, 0, 0);
            p.extend(rf(12).to_le_bytes());
            p.extend(rf(7).to_le_bytes());
            p
        }), // 13
        (OTHER, tail(1)),                                // 14 operation: new
        (OTHER, {
            let mut f = hdr(0, 0, 0);
            f.push(0);
            f
        }), // 15 not reversed
        (PARAM, param("d0", 0.5)),                       // 16
        (OTHER, tail(1)),                                // 17 extent: distance
        (FEATURE, feature(1, 0, 5, &[rf(14), rf(12), 0, rf(15), rf(16), 0, rf(17)])), // 18
    ]
}

fn tree(recs: &[(usize, Vec<u8>)]) -> Result<Value, String> {
    let part = inventor_dc_part(&CLASSES, recs, &[]);
    feature_tree(&part, "dir/part.ipt", &Limits::default())
}

fn types(t: &Value) -> Vec<(String, String)> {
    t["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["type"].as_str().unwrap().to_string(), f["name"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn the_newer_header_with_a_word_at_14_reads_like_the_older_one() {
    // Version 2803 and later insert a zero u32 at offset 14 of the common header, so the
    // owner reference moves to 18 and the id to 22.
    let base = base_records();
    let shifted: Vec<(usize, Vec<u8>)> = base
        .iter()
        .map(|(c, r)| {
            let mut r = r.clone();
            if *c == FEATURE {
                // The owner (a document record in real files) is what tells the layouts apart.
                r[14..18].copy_from_slice(&rf(2).to_le_bytes());
            }
            let mut v = r[..14].to_vec();
            v.extend([0u8; 4]);
            v.extend(&r[14..]);
            (*c, v)
        })
        .collect();
    let want = tree(&base).unwrap();
    assert_eq!(tree(&shifted).unwrap(), want);
    // Without the owner reference after the zero word the file reads as the older layout, and
    // the sketch's curves are lost (its records do not frame as sketch entities).
    let plain: Vec<(usize, Vec<u8>)> = shifted
        .iter()
        .map(|(c, r)| {
            let mut r = r.clone();
            if *c == FEATURE {
                r[18..22].copy_from_slice(&0u32.to_le_bytes());
            }
            (*c, r)
        })
        .collect();
    assert_ne!(tree(&plain).unwrap(), want);
}

/// Records carry the version word of the save that wrote them: features have it at 22, other
/// classes after `0xffffffff`. A record kept from an older save (0x0a92 in a 0x0a96 part, the
/// hole centres of `27in_2_125 belly pan`) has its own, and its body starts after it all the
/// same.
#[test]
fn a_record_from_an_older_save_has_its_own_version_word() {
    let base = base_records();
    let with = |old_points: u32| -> Vec<(usize, Vec<u8>)> {
        base.iter()
            .map(|(c, r)| {
                let mut v = r[..22].to_vec();
                match *c {
                    FEATURE => v.extend(0x0a96u32.to_le_bytes()),
                    NAME => {}
                    POINT => v.extend([0xff, 0xff, 0xff, 0xff].into_iter().chain(old_points.to_le_bytes())),
                    _ => v.extend([0xff, 0xff, 0xff, 0xff].into_iter().chain(0x0a96u32.to_le_bytes())),
                }
                v.extend(&r[22..]);
                (*c, v)
            })
            .collect()
    };
    let same = tree(&with(0x0a96)).unwrap();
    assert_eq!(same["features"][0]["points"], tree(&base).unwrap()["features"][0]["points"]);
    assert_eq!(tree(&with(0x0a92)).unwrap(), same);
}

#[test]
fn a_solid_body_record_is_not_a_feature() {
    // A part-feature record named through a `656dd01e` record is one of the part's solid bodies.
    let mut recs = base_records();
    let mut body_name = hdr(0, 0, 0);
    body_name.extend(rf(20).to_le_bytes());
    recs.push((BODY_NAME, body_name)); // 19
    recs.push((FEATURE, feature(19, 0, 9, &[0, 0, 0]))); // 20
    assert_eq!(tree(&recs).unwrap(), tree(&base_records()).unwrap());
    // Named directly by a name of the body type, it is a body too.
    recs[19] = (NAME, name_rec("Solid1", Some(0x0ca9_112d)));
    assert_eq!(tree(&recs).unwrap(), tree(&base_records()).unwrap());
    // Named by an ordinary name record without a type, it is an unknown feature.
    recs[19] = (NAME, name_rec("Solid1", None));
    assert_eq!(types(&tree(&recs).unwrap()).last().unwrap().0, "unsupported");
}

#[test]
fn a_zstd_packed_segment_reads_like_a_zlib_one() {
    // Newer files pack the meta and data streams as Zstandard (`04 02` in the B header).
    let (m, b) = dc_segment_packed(&CLASSES, &base_records(), true);
    let part = ole(&[("MdcSegment0001", &m), ("BdcSegment0001", &b)]);
    let t = feature_tree(&part, "dir/part.ipt", &Limits::default()).unwrap();
    assert_eq!(t, tree(&base_records()).unwrap());
}

#[test]
fn a_block_after_a_record_is_skipped() {
    // A separator byte 1 after a record's size: a block of named entries follows (here the
    // sweep's "RDxSweepTwistAngle" entry as the samples have it) before the next class word.
    let mut blk = 1u32.to_le_bytes().to_vec();
    blk.extend(18u32.to_le_bytes());
    blk.extend(b"RDxSweepTwistAngle");
    blk.extend([7, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0x30, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    blk.extend(18u32.to_le_bytes());
    blk.extend(b"RDxSweepTwistAngle");
    blk.extend(rf(3).to_le_bytes());
    let (m, b) = dc_segment_blocks(&CLASSES, &base_records(), false, &[(2, blk.clone()), (9, blk)]);
    let part = ole(&[("MdcSegment0001", &m), ("BdcSegment0001", &b)]);
    let t = feature_tree(&part, "dir/part.ipt", &Limits::default()).unwrap();
    assert_eq!(t, tree(&base_records()).unwrap());
    // A block that never lines up with the next record is an error, not a guess.
    let (m, b) = dc_segment_blocks(&CLASSES, &base_records(), false, &[(2, vec![0xee; 5000])]);
    let part = ole(&[("MdcSegment0001", &m), ("BdcSegment0001", &b)]);
    assert!(feature_tree(&part, "dir/part.ipt", &Limits::default()).unwrap_err().contains("do not frame"));
}

#[test]
fn a_sketch_and_an_extrude_come_out_in_millimetres() {
    let t = tree(&base_records()).unwrap();
    assert_eq!(t["format"], "gusset-feature-tree");
    assert_eq!(t["source"]["app"], "inventor");
    assert_eq!(t["source"]["file"], "part.ipt");
    assert_eq!(types(&t), [("sketch".into(), "Sketch1".into()), ("extrude".into(), "Extrusion1".into())]);
    let sk = &t["features"][0];
    assert_eq!(sk["plane"], json!({"origin": [0.0, 0.0, 0.0], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]}));
    let pts: Vec<(f64, f64)> =
        sk["points"].as_array().unwrap().iter().map(|p| (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())).collect();
    assert_eq!(pts, [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)]);
    assert_eq!(sk["curves"].as_array().unwrap().len(), 4);
    assert_eq!(sk["curves"][0], json!({"id": "l8", "type": "line", "start": "p3", "end": "p4"}));
    let e = &t["features"][1];
    assert_eq!(e["sketch"], "Sketch1");
    assert_eq!(e["operation"], "new");
    assert_eq!(e["distance"], 5.0);
    assert_eq!(e["symmetric"], false);
    assert_eq!(e["profiles"], "all");
}

#[test]
fn extrude_direction_extent_and_operation() {
    let mut r = base_records();
    // Reversed, symmetric.
    if let Some(f) = r.get_mut(15) {
        f.1 = {
            let mut v = hdr(0, 0, 0);
            v.push(1);
            v
        };
    }
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][1]["distance"], -5.0);
    r[17].1 = tail(2);
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][1]["distance"], 5.0);
    assert_eq!(t["features"][1]["symmetric"], true);
    r[17].1 = tail(5);
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][1]["distance"], -1e4);
    assert_eq!(t["features"][1]["through_all"], true);
    // Through all with slot 7 set: both ways, whatever the direction flag.
    r.push((OTHER, {
        let mut v = hdr(0, 0, 0);
        v.push(1);
        v
    })); // 19
    r[18].1 = feature(1, 0, 5, &[rf(14), rf(12), 0, rf(15), rf(16), 0, rf(17), rf(19)]);
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][1]["distance"], 2e4);
    assert_eq!(t["features"][1]["symmetric"], true);
    r.pop();
    r[18].1 = feature(1, 0, 5, &[rf(14), rf(12), 0, rf(15), rf(16), 0, rf(17)]);
    // To the next face / to a face: the reach is measured on the result body, which this
    // file does not have.
    for e in [4, 7] {
        r[17].1 = tail(e);
        let t = tree(&r).unwrap();
        assert_eq!(t["features"][0]["type"], "unsupported");
        let why = t["features"][0]["reason"].as_str().unwrap();
        assert!(why.contains(&format!("extent {e}: the result body is not readable")), "{why}");
    }
    // An extent that is not read: the feature is unsupported, with the reason.
    r[17].1 = tail(9);
    let t = tree(&r).unwrap();
    assert_eq!(t["features"].as_array().unwrap().len(), 1);
    assert_eq!(t["features"][0]["type"], "unsupported");
    assert!(t["features"][0]["reason"].as_str().unwrap().contains("extent"));
    // A cut before any body has no target.
    r[17].1 = tail(1);
    r[14].1 = tail(2);
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][0]["type"], "unsupported");
}

#[test]
fn history_order_suppressed_thread_and_unknown_features() {
    let mut r = base_records();
    // 19: a second, later-created feature of an unread kind, and a thread and a suppressed
    // fillet, listed in record order before the extrude's id in the order record.
    r.push((NAME, name_rec("Thicken1", Some(T_THICKEN)))); // 19
    r.push((NAME, name_rec("Gewinde1", Some(T_THREAD)))); // 20
    r.push((NAME, name_rec("Fillet1", Some(T_FILLET)))); // 21
    r.push((FEATURE, feature(19, 0, 9, &[]))); // 22
    r.push((FEATURE, feature(20, 0, 8, &[]))); // 23
    r.push((FEATURE, feature(21, 0x04, 7, &[]))); // 24
    let mut order = hdr(0, 0, 0);
    for id in [5u32, 7, 8, 9] {
        order.extend(id.to_le_bytes());
    }
    r.push((ORDER, order));
    let t = tree(&r).unwrap();
    assert_eq!(
        types(&t),
        [
            ("sketch".into(), "Sketch1".into()),
            ("extrude".into(), "Extrusion1".into()),
            ("unsupported".into(), "Thicken1".into())
        ]
    );
    let om: Vec<&str> =
        t["source"]["omitted"].as_array().unwrap().iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert_eq!(om, ["Fillet1", "Gewinde1"]);
    // Without the order record the records' own order is used.
    r.pop();
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][2]["name"], "Thicken1");
}

fn hole_records() -> Vec<(usize, Vec<u8>)> {
    let mut r = base_records();
    let mut centre_sketch_placement = hdr(0, 0, 0);
    centre_sketch_placement.extend(0x7bde_8421u32.to_le_bytes());
    r.push((NAME, name_rec("Hole1", Some(T_HOLE)))); // 19
    r.push((OTHER, {
        let mut v = hdr(0, 0, 0);
        v.extend(3u32.to_le_bytes());
        v
    })); // 20 type
    r.push((PARAM, param("d1", 0.4))); // 21 diameter
    r.push((PARAM, param("d2", 1.0))); // 22 depth
    r.push((PARAM, param("d3", 0.0))); // 23 point angle
    r.push((POINT, point(7, 1.0, 0.5))); // 24 centre
    r.push((OTHER, {
        let mut v = hdr(0, 0, 0);
        v.extend(0u32.to_le_bytes());
        v.extend(list(&[rf(24)]));
        v
    })); // 25 centres
    r.push((PLACEMENT, centre_sketch_placement)); // 26
    r.push((OTHER, tail(5))); // 27 through all
    let slots = [rf(20), rf(21), rf(22), 0, 0, 0, rf(23), rf(25), rf(26), rf(27)];
    r.push((FEATURE, feature(19, 0, 6, &slots))); // 28
    r
}

#[test]
fn a_hole_has_its_centres_and_a_plain_cut_fallback() {
    let t = tree(&hole_records()).unwrap();
    let h = &t["features"][2];
    assert_eq!(h["type"], "hole");
    assert_eq!(h["diameter"], 4.0);
    assert_eq!(h["through"], true);
    assert_eq!(h["positions"], json!([[10.0, 5.0, 0.0]]));
    assert_eq!(h["face"]["normal"], json!([0.0, 0.0, 1.0]));
    let fb = h["_fallback"].as_array().unwrap();
    assert_eq!(fb[0]["type"], "sketch");
    assert_eq!(fb[1]["operation"], "cut");
    assert_eq!(fb[1]["target"], "Extrusion1");
    assert_eq!(fb[0]["curves"][0]["radius"], 2.0);
    // Blind: depth from the parameter, one revolve per centre as the fallback.
    let mut r = hole_records();
    r[27].1 = tail(1);
    let t = tree(&r).unwrap();
    let h = &t["features"][2];
    assert_eq!(h["depth"], 10.0);
    assert!(h.get("through").is_none());
    assert_eq!(h["_fallback"][1]["type"], "revolve");
    assert_eq!(h["_fallback"][1]["name"], "Hole1_0");
    // A drill point angle adds a tip to the fallback's profile.
    r[23].1 = param("d3", 2.0943951023931953);
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][2]["point_angle"], 120.0);
    let pts = t["features"][2]["_fallback"][0]["points"].as_array().unwrap().len();
    assert_eq!(pts, 4);
}

/// Hole variant 1 (the `u16` after the type 3) is a countersink: slot 3 its diameter, slot 5
/// its angle (`ho_csink`: 10 mm at 90 degrees round a 6 mm hole).
#[test]
fn a_countersunk_hole() {
    let mut r = hole_records();
    r[20].1 = {
        let mut v = hdr(0, 0, 0);
        v.extend((3u32 | 1 << 16).to_le_bytes());
        v
    };
    r.push((PARAM, param("d4", 0.8))); // 29 countersink diameter
    r.push((PARAM, param("d5", std::f64::consts::FRAC_PI_2))); // 30 countersink angle
    let slots = [rf(20), rf(21), rf(22), rf(29), 0, rf(30), rf(23), rf(25), rf(26), rf(27)];
    r[28].1 = feature(19, 0, 6, &slots);
    let t = tree(&r).unwrap();
    let h = &t["features"][2];
    assert_eq!(h["variant"], "countersink", "{h}");
    assert_eq!(h["csink_diameter"], 8.0);
    assert_eq!(h["csink_angle"], 90.0);
    // Through all with a countersink: the fallback is a revolve (cone, then the bore).
    let fb = h["_fallback"].as_array().unwrap();
    assert_eq!(fb[1]["type"], "revolve");
    let ys: Vec<f64> = fb[0]["points"].as_array().unwrap().iter().map(|p| p["y"].as_f64().unwrap()).collect();
    assert_eq!(ys[..3], [0.0, 0.0, -2.0]);
    // A countersink no wider than the hole is refused.
    r[29].1 = param("d4", 0.3);
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][2]["type"], "unsupported");
}

#[test]
fn a_hole_before_any_body_has_no_fallback() {
    let mut r = hole_records();
    // Drop the extrude from the order: the hole comes first.
    let mut order = hdr(0, 0, 0);
    for id in [6u32, 5] {
        order.extend(id.to_le_bytes());
    }
    r.push((ORDER, order));
    let t = tree(&r).unwrap();
    assert_eq!(t["features"][0]["type"], "hole");
    assert!(t["features"][0].get("_fallback").is_none());
}

#[test]
fn a_fillet_with_several_radii_splits_by_the_blend_faces_radius() {
    use crate::inventor_features::radius_groups;
    let g = radius_groups(&[1.0, 2.5, 4.0], &[Some(2.5), Some(1.0), Some(2.5)]).unwrap();
    assert_eq!(g, [(1.0, vec![1]), (2.5, vec![0, 2])]);
    // An edge whose face has no radius, or a radius of no set, refuses the whole feature.
    assert!(radius_groups(&[1.0, 2.5], &[Some(1.0), None]).is_none());
    assert!(radius_groups(&[1.0, 2.5], &[Some(1.0), Some(3.0)]).is_none());
}

#[test]
fn fillets_and_chamfers_need_the_result_body() {
    let mut r = base_records();
    r.push((NAME, name_rec("Fillet1", Some(T_FILLET)))); // 19
    r.push((FEATURE, feature(19, 0, 6, &[0]))); // 20
    let t = tree(&r).unwrap();
    let f = &t["features"][2];
    assert_eq!(f["type"], "unsupported");
    assert!(f["reason"].as_str().unwrap().contains("result body"));
}

/// A shell: slot 0 ends with the side (0 inside, 1 outside), slot 1 lists the open faces,
/// slot 3 is the thickness. With no open face it hollows the whole body; open faces are found
/// in the result body.
#[test]
fn a_shell_reads_its_side_thickness_and_open_faces() {
    let mut r = base_records();
    r.push((NAME, name_rec("Shell1", Some(T_SHELL)))); // 19
    r.push((OTHER, tail(0))); // 20 side
    r.push((OTHER, {
        let mut v = hdr(0, 0, 0);
        v.extend(list(&[]));
        v
    })); // 21 open faces
    r.push((PARAM, param("d9", 0.2))); // 22 thickness
    r.push((FEATURE, feature(19, 0, 6, &[rf(20), rf(21), 0, rf(22)]))); // 23
    let t = tree(&r).unwrap();
    assert_eq!(
        t["features"][2],
        json!({"type": "shell", "name": "Shell1", "thickness": 2.0, "side": "inside", "body": "Extrusion1"})
    );
    r[20].1 = tail(1);
    assert_eq!(tree(&r).unwrap()["features"][2]["side"], "outside");
    r[20].1 = tail(2);
    assert!(tree(&r).unwrap()["features"][2]["reason"].as_str().unwrap().contains("side"));
    // Open faces need the result body.
    r[20].1 = tail(0);
    r[21].1 = {
        let mut v = hdr(0, 0, 0);
        v.extend(list(&[rf(20)]));
        v
    };
    assert!(tree(&r).unwrap()["features"][2]["reason"].as_str().unwrap().contains("result body"));
}

/// A sweep (`90bada05`): slot 0 the profile loops, slot 1 the path loop, whose sketch a
/// `424eb7d7` record names; without it the sweep says so. Path curves are those on the path
/// wire: [`polyline_dist`] measures that.
#[test]
fn a_sweep_needs_its_path_sketch() {
    use crate::inventor_features::polyline_dist;
    let mut r = base_records();
    r.push((NAME, name_rec("Sweep1", Some(T_SWEEP)))); // 19
    r.push((OTHER, tail(0))); // 20 not a path loop
    r.push((FEATURE, feature(19, 0, 6, &[rf(12), rf(20), rf(14)]))); // 21
    let t = tree(&r).unwrap();
    let f = &t["features"][2];
    assert_eq!(f["type"], "unsupported");
    assert!(f["reason"].as_str().unwrap().contains("no path sketch"), "{f}");
    let l = vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)]];
    assert!(polyline_dist((5.0, 0.0), &l).abs() < 1e-12);
    assert!((polyline_dist((12.0, 2.0), &l) - 2.0).abs() < 1e-12);
    assert!(polyline_dist((0.0, 0.0), &[]).is_infinite());
}

#[test]
fn files_that_are_not_parts_are_errors() {
    let l = Limits::default();
    assert!(feature_tree(b"hello", "x.ipt", &l).is_err());
    // An Inventor result body only, no feature segment.
    let part = crate::fixtures::inventor_part(b"ASM BinaryFile4 x");
    assert!(feature_tree(&part, "x.ipt", &l).unwrap_err().contains("feature segment"));
    // Streams that are not what they say.
    let junk = ole(&[("MdcSegment0001", b"x"), ("BdcSegment0001", b"y")]);
    assert!(feature_tree(&junk, "x.ipt", &l).is_err());
    // A part with a segment but no features.
    let part = inventor_dc_part(&CLASSES, &[(NAME, name_rec("a", None))], &[]);
    assert!(feature_tree(&part, "x.ipt", &l).unwrap_err().contains("no features"));
    // The limits apply.
    let tiny = Limits { max_stream_bytes: 100, ..Limits::default() };
    assert!(feature_tree(&inventor_dc_part(&CLASSES, &base_records(), &[]), "x.ipt", &tiny).is_err());
}

#[test]
fn damaged_records_never_panic() {
    // Every byte of every record set to a few values (and the records cut short): the reader
    // gives a tree or an error.
    let base = hole_records();
    let mut n = 0;
    for ri in 0..base.len() {
        let len = base[ri].1.len();
        for bi in 0..len {
            for v in [0x00u8, 0xff, 0x80, 0x7f] {
                let mut r = base.clone();
                r[ri].1[bi] = v;
                let _ = tree(&r);
                n += 1;
            }
        }
        let mut r = base.clone();
        r[ri].1.truncate(len / 2);
        let _ = tree(&r);
    }
    assert!(n > 1000);
}

#[test]
fn placement_masks() {
    // The sketch placement of an inclined plane: x -> Y, y -> Z, origin at (5.5, 1, 0) cm.
    let mut r = base_records();
    let mut p = hdr(0, 0, 0);
    // x = (0,1,0), y = (0,0,1), n = (1,0,0): entries 2, 4, 9, 15 are 1; 3, 7, 11 are doubles.
    let one: [usize; 4] = [2, 4, 9, 15];
    let dbl: [usize; 3] = [3, 7, 11];
    let mut mask = 0u32;
    for e in 0..16usize {
        if dbl.contains(&e) {
            continue;
        }
        mask |= if one.contains(&e) { 1 << e } else { 1 << (16 + e) };
    }
    p.extend(mask.to_le_bytes());
    for v in [5.5f64, 1.0, 0.0] {
        p.extend(v.to_le_bytes());
    }
    r[2].1 = p;
    let t = tree(&r).unwrap();
    assert_eq!(
        t["features"][0]["plane"],
        json!({"origin": [55.0, 10.0, 0.0], "x_axis": [0.0, 1.0, 0.0], "y_axis": [0.0, 0.0, 1.0]})
    );
}

// --- ShapeManager topology ----------------------------------------------------------------------

mod sab {
    use crate::acis::Acis;
    use crate::inventor_profile::{P2, Poly, picks};

    enum It {
        P(i32),
        I(i32),
        D(f64),
        Pos([f64; 3]),
        Vec([f64; 3]),
        /// A string token (0x07): the name of a custom attribute.
        S(&'static str),
    }
    use It::{D, I, P, Pos, S, Vec as V};

    fn blob(recs: &[(&[&str], Vec<It>)]) -> Vec<u8> {
        let mut b = b"ASM BinaryFile4\0\0\0\0".to_vec();
        let mut rec = |names: &[&str], items: &[It]| {
            for n in names {
                b.push(0x0d);
                b.push(n.len() as u8);
                b.extend(n.as_bytes());
            }
            for it in items {
                match it {
                    It::P(v) => {
                        b.push(0x0c);
                        b.extend(v.to_le_bytes());
                    }
                    It::I(v) => {
                        b.push(0x04);
                        b.extend(v.to_le_bytes());
                    }
                    It::D(v) => {
                        b.push(0x06);
                        b.extend(v.to_le_bytes());
                    }
                    It::S(t) => {
                        b.push(0x07);
                        b.push(t.len() as u8);
                        b.extend(t.as_bytes());
                    }
                    It::Pos(v) | It::Vec(v) => {
                        b.push(if matches!(it, It::Pos(_)) { 0x13 } else { 0x14 });
                        for c in v {
                            b.extend(c.to_le_bytes());
                        }
                    }
                }
            }
            b.push(0x11);
        };
        rec(&["asmheader"], &[]);
        for (n, items) in recs {
            rec(n, items);
        }
        b.push(0x0d);
        b.push(15);
        b.extend(b"End-of-ASM-data");
        b
    }

    /// A square wire body (tag 7), 2 x 2 cm, in the XY plane.
    fn square() -> Vec<u8> {
        let corners = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0]];
        let mut r: Vec<(&[&str], Vec<It>)> = vec![(&["body"], vec![I(7), P(-1), P(2)]), (&["wire"], vec![P(-1), P(3)])];
        for k in 0..4 {
            let (prev, next) = (3 + (k + 3) % 4, 3 + (k + 1) % 4);
            r.push((&["coedge"], vec![P(-1), P(-1), P(prev), P(next), P(-1), P(7 + k), P(2)]));
        }
        for k in 0..4 {
            r.push((&["edge"], vec![P(-1), P(11 + k), P(11 + (k + 1) % 4)]));
        }
        for k in 0..4 {
            r.push((&["vertex"], vec![P(-1), P(-1), P(15 + k)]));
        }
        for c in corners {
            r.push((&["point"], vec![Pos(c)]));
        }
        blob(&r)
    }

    /// Two planes (x = 0 and z = 0, 2 cm long along y) and the cylinder of radius 0.5 cm that
    /// blends them, tagged `NMx_blend_tag` with feature id `fid`.
    fn blend(fid: i32, with_second_support: bool) -> Vec<u8> {
        blend_with(fid, with_second_support, Vec::new())
    }

    /// [`blend`] with more records after it (numbered from 30).
    fn blend_with(fid: i32, with_second_support: bool, more: Vec<(&[&str], Vec<It>)>) -> Vec<u8> {
        let tag: &[&str] = &["NMx_blend_tag", "NMx_Brep_Name_tag", "NMx_Brep_tag", "NamingMatching", "attrib"];
        let p2_partner = if with_second_support { 16 } else { -1 };
        // Record numbers (the header is 0): body 1, faces 2 (the blend), 3 (x = 0), 4 (z = 0),
        // surfaces 5..=7, loops 8..=10, coedges 11..=16, edges 17..=20, vertices 21..=24,
        // points 25..=28, the tag 29.
        let r: Vec<(&[&str], Vec<It>)> = vec![
            (&["body"], vec![I(1), P(2)]),
            (&["face"], vec![P(-1), P(8), P(5)]),
            (&["face"], vec![P(-1), P(9), P(6)]),
            (&["face"], vec![P(-1), P(10), P(7)]),
            (
                &["cone", "surface"],
                vec![Pos([0.5, 0.0, 0.5]), V([0.0, 1.0, 0.0]), V([0.5, 0.0, 0.0]), D(1.0), D(0.0), D(1.0)],
            ),
            (&["plane", "surface"], vec![Pos([0.0, 0.0, 0.0]), V([1.0, 0.0, 0.0])]),
            (&["plane", "surface"], vec![Pos([0.0, 0.0, 0.0]), V([0.0, 0.0, 1.0])]),
            (&["loop"], vec![P(-1), P(11), P(2)]),
            (&["loop"], vec![P(-1), P(15), P(3)]),
            (&["loop"], vec![P(-1), P(16), P(4)]),
            (&["coedge"], vec![P(-1), P(-1), P(14), P(12), P(15), P(17), P(8)]),
            (&["coedge"], vec![P(-1), P(-1), P(11), P(13), P(-1), P(18), P(8)]),
            (&["coedge"], vec![P(-1), P(-1), P(12), P(14), P(p2_partner), P(19), P(8)]),
            (&["coedge"], vec![P(-1), P(-1), P(13), P(11), P(-1), P(20), P(8)]),
            (&["coedge"], vec![P(-1), P(-1), P(15), P(15), P(11), P(17), P(9)]),
            (&["coedge"], vec![P(-1), P(-1), P(16), P(16), P(13), P(19), P(10)]),
            (&["edge"], vec![P(-1), P(21), P(22)]),
            (&["edge"], vec![P(-1), P(22), P(23)]),
            (&["edge"], vec![P(-1), P(23), P(24)]),
            (&["edge"], vec![P(-1), P(24), P(21)]),
            (&["vertex"], vec![P(-1), P(-1), P(25)]),
            (&["vertex"], vec![P(-1), P(-1), P(26)]),
            (&["vertex"], vec![P(-1), P(-1), P(27)]),
            (&["vertex"], vec![P(-1), P(-1), P(28)]),
            (&["point"], vec![Pos([0.0, 0.0, 0.5])]),
            (&["point"], vec![Pos([0.0, 2.0, 0.5])]),
            (&["point"], vec![Pos([0.5, 2.0, 0.0])]),
            (&["point"], vec![Pos([0.5, 0.0, 0.0])]),
            (tag, vec![I(0), I(0), I(0), I(fid), P(2)]),
        ];
        let mut r = r;
        r.extend(more);
        blob(&r)
    }

    #[test]
    fn a_wire_body_gives_its_loop() {
        let a = Acis::parse(&square()).unwrap();
        let l = a.loops_of_body(7).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].len(), 5);
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(p, q)| (p - q).abs() < 1e-12);
        assert!(near(l[0][0], [0.0, 0.0, 0.0]));
        assert!(near(l[0][2], [2.0, 2.0, 0.0]));
        assert!(near(l[0][4], l[0][0]));
        assert!(a.loops_of_body(8).is_none());
    }

    #[test]
    fn a_blend_face_gives_the_edge_it_replaced() {
        let a = Acis::parse(&blend(42, true)).unwrap();
        assert_eq!(a.blend_faces(42), [2]);
        assert!(a.blend_faces(43).is_empty());
        let (m, info) = a.edge_mid(2).unwrap();
        assert!(m.iter().zip([0.0, 1.0, 0.0]).all(|(a, b)| (a - b).abs() < 1e-9), "{m:?}");
        // A straight edge: the path runs along it, 0.1 mm (0.01 cm) either side of the midpoint.
        let p = crate::acis::local_path(m, &info, 0.01).unwrap();
        assert!((crate::acis::dist(p[0], p[1]) - 0.02).abs() < 1e-12, "{p:?}");
        // One support missing: the reason.
        let a = Acis::parse(&blend(42, false)).unwrap();
        assert!(a.edge_mid(2).unwrap_err().contains("1 usable supports"));
    }

    /// The faces a sweep made carry its id in `NMx_Sweep_Generated_Brep_tag` (side faces) and
    /// `NMx_Generated_Brep_tag` (caps), or in newer files in `ATTRIB_CUSTOM` records named
    /// `INV_NMX_SWEEPGENERATED_TAG` / `INV_NMX_GEN_TAG` with one more int before the version.
    #[test]
    fn the_faces_a_sweep_made_are_found_by_its_id() {
        let side: &[&str] =
            &["NMx_Sweep_Generated_Brep_tag", "NMx_Brep_Name_tag", "NMx_Brep_tag", "NamingMatching", "attrib"];
        let custom: &[&str] = &["ATTRIB_CUSTOM", "attrib"];
        let a = Acis::parse(&blend_with(
            42,
            true,
            vec![
                (side, vec![I(-1), I(43), I(1), I(7), P(3)]),
                (custom, vec![S("INV_NMX_GEN_TAG"), I(-1), I(1), I(44), I(1), I(7), P(4)]),
                (custom, vec![S("INV_NMX_SWEEPGENERATED_TAG"), I(-1), I(1), I(44), I(1), I(8), P(2)]),
                (custom, vec![S("INV_NMX_SHELL_TAG"), I(-1), I(1), I(44), I(1), I(9), P(2)]),
            ],
        ))
        .unwrap();
        assert_eq!(a.sweep_faces(7), [3, 4]);
        assert_eq!(a.sweep_faces(8), [2]);
        assert!(a.sweep_faces(9).is_empty());
        // Face 3 (x = 0) runs 2 cm along y.
        let ys: Vec<f64> = a.verts(3).iter().map(|v| v[1]).collect();
        assert!(ys.iter().any(|y| (y - 2.0).abs() < 1e-12) && ys.iter().any(|y| y.abs() < 1e-12), "{ys:?}");
    }

    /// The faces a shell opened are the planes beside its inner (shell-tagged) faces: one
    /// point per plane, a corner of the rim's inner boundary.
    #[test]
    fn a_shell_finds_the_faces_it_opened() {
        let custom: &[&str] = &["ATTRIB_CUSTOM", "attrib"];
        let a = Acis::parse(&blend_with(
            42,
            true,
            vec![(custom, vec![S("INV_NMX_SHELL_TAG"), I(-1), I(1), I(44), I(1), I(9), P(2)])],
        ))
        .unwrap();
        let open = a.shell_openings(9, &[]);
        assert_eq!(open.len(), 2, "{open:?}");
        let normals: Vec<[f64; 3]> = open.iter().map(|o| o.1.map(f64::abs)).collect();
        assert!(normals.contains(&[1.0, 0.0, 0.0]) && normals.contains(&[0.0, 0.0, 1.0]), "{normals:?}");
        // Each point is a corner on the edge the rim shares with the inner face.
        for (p, n) in &open {
            let c = if n[0].abs() > 0.5 { [0.0, p[1], 0.5] } else { [0.5, p[1], 0.0] };
            assert!(crate::acis::dist(*p, c) < 1e-12, "{p:?}");
        }
        assert!(a.shell_openings(10, &[]).is_empty());
        // A plane a later feature made is no rim.
        let b = Acis::parse(&blend_with(
            42,
            true,
            vec![
                (custom, vec![S("INV_NMX_SHELL_TAG"), I(-1), I(1), I(44), I(1), I(9), P(2)]),
                (custom, vec![S("INV_NMX_GEN_TAG"), I(-1), I(1), I(44), I(1), I(11), P(3)]),
            ],
        ))
        .unwrap();
        assert_eq!(b.shell_openings(9, &[11]).len(), 1);
        assert_eq!(b.shell_openings(9, &[]).len(), 2);
    }

    #[test]
    fn damaged_shape_data_never_panics() {
        for blob in [square(), blend(42, true)] {
            for n in 0..blob.len() {
                for v in [0x00u8, 0xff, 0x0c, 0x11] {
                    let mut b = blob.clone();
                    b[n] = v;
                    if let Ok(a) = Acis::parse(&b) {
                        let _ = a.loops_of_body(7);
                        for f in a.blend_faces(42) {
                            let _ = a.edge_mid(f);
                        }
                        for f in 0..40 {
                            let _ = a.edge_mid(f);
                            let _ = a.kind(f);
                        }
                    }
                }
                let _ = Acis::parse(&blob[..n]);
            }
        }
        assert!(Acis::parse(b"nothing").is_err());
    }

    fn circle(c: P2, r: f64) -> Vec<P2> {
        (0..64).map(|k| (c.0 + r * libm::cos(f64::from(k) * 0.1), c.1 + r * libm::sin(f64::from(k) * 0.1))).collect()
    }

    #[test]
    fn picks_are_the_regions_inside_an_odd_number_of_loops() {
        let sq = |s: f64| vec![(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)];
        let curves = vec![
            Poly::Line((0.0, 0.0), (20.0, 0.0)),
            Poly::Line((20.0, 0.0), (20.0, 20.0)),
            Poly::Line((20.0, 20.0), (0.0, 20.0)),
            Poly::Line((0.0, 20.0), (0.0, 0.0)),
            Poly::Circle((10.0, 10.0), 5.0),
        ];
        // Outer loop only: the annulus and the disc are both inside it once.
        let p = picks(&curves, &[sq(20.0)]).unwrap();
        assert_eq!(p.len(), 2);
        // Outer and inner loops: the disc is inside twice, so only the annulus is left.
        let p = picks(&curves, &[sq(20.0), circle((10.0, 10.0), 5.0)]).unwrap();
        assert_eq!(p.len(), 1);
        let d = ((p[0].0 - 10.0).powi(2) + (p[0].1 - 10.0).powi(2)).sqrt();
        assert!(d > 5.0, "{p:?}");
        assert!(picks(&[], &[]).is_none());
    }

    #[test]
    fn a_tilted_ellipse_is_a_region_inside_its_loop() {
        let curves = vec![Poly::Ellipse { c: (5.0, 3.0), major: (0.6, 0.8), a: 4.0, b: 2.0 }];
        let lp: Vec<P2> = (0..64)
            .map(|k| {
                let t = f64::from(k) * std::f64::consts::TAU / 64.0;
                let (u, v) = (4.0 * libm::cos(t), 2.0 * libm::sin(t));
                (5.0 + 0.6 * u - 0.8 * v, 3.0 + 0.8 * u + 0.6 * v)
            })
            .collect();
        let p = picks(&curves, &[lp]).unwrap();
        assert_eq!(p.len(), 1);
        assert!((p[0].0 - 5.0).abs() < 1.0 && (p[0].1 - 3.0).abs() < 1.0, "{p:?}");
    }
}

/// Chamfer type 2 is two distances (slots 2 and 3): the first reference line runs the first
/// distance into its face, the second the second distance into the other face, from the same
/// point on the edge. Equal distances are an equal chamfer; type 1 takes the angle.
#[test]
fn a_chamfer_of_two_distances_names_the_face_of_the_first() {
    use crate::inventor_features::{Second, chamfer_second};
    // `ch_two_dist`: 2 mm along -x, 4 mm along -z from the edge point (5, 3.5, 2) cm.
    let lines = [(2, [5.0, 3.5, 2.0], [4.8, 3.5, 2.0]), (2, [5.0, 3.5, 2.0], [5.0, 3.5, 1.6])];
    assert_eq!(chamfer_second(2, 0.2, 0.4, 1, &lines), Ok(Some(Second::Distance(0.4, [48.0, 35.0, 20.0]))));
    // The distances the other way round do not match the lines.
    assert!(chamfer_second(2, 0.4, 0.2, 1, &lines).unwrap_err().contains("first distance"));
    // Equal distances: an equal chamfer, on any number of edges.
    assert_eq!(chamfer_second(2, 0.2, 0.2, 5, &[]), Ok(None));
    // Unequal on several edges, or without its second line: refused.
    assert!(chamfer_second(2, 0.2, 0.4, 2, &lines).unwrap_err().contains("several edges"));
    assert!(chamfer_second(2, 0.2, 0.4, 1, &lines[..1]).unwrap_err().contains("second reference"));
    // Type 1: the angle and the first line's end.
    assert_eq!(chamfer_second(1, 0.2, 0.5, 1, &lines), Ok(Some(Second::Angle(0.5, [48.0, 35.0, 20.0]))));
    assert!(chamfer_second(1, 0.2, 2.0, 1, &lines).is_err());
    assert!(chamfer_second(3, 0.2, 0.4, 1, &lines).unwrap_err().contains("type 3"));
}

/// The classes of the copy features' records, after the common ones.
const COPY_CLASSES: [u32; 17] = [
    0x2b48_a42b,
    0x9087_4d18,
    0xce52_df35,
    0x9087_4d11,
    0xce52_df3a,
    0x1111_1111,
    0xf988_4c43,
    0x9087_4d26,
    0x9087_4d91,
    0x4521_21b6,
    0x656d_d01e,
    0x0697_7131, // 11 circular pattern
    0xfad9_a9b5, // 12 mirror
    0x8ef0_6c89, // 13 work axis
    0xce52_df42, // 14 work plane
    0xbb1d_d5df, // 15 count
    0x9087_4d28, // 16 flag
];

fn copy_tree(recs: &[(usize, Vec<u8>)]) -> Value {
    feature_tree(&inventor_dc_part(&COPY_CLASSES, recs, &[]), "dir/part.ipt", &Limits::default()).unwrap()
}

fn doubles(rec: &mut Vec<u8>, v: &[f64]) {
    for x in v {
        rec.extend(x.to_le_bytes());
    }
}

/// A circular pattern (`06977131`) of the base extrude: the copied features before its sketch
/// placement record, the count's last word, the angle parameter and a work axis (cm), with
/// the flag after the axis turning it the other way.
#[test]
fn a_circular_pattern_reads_its_features_count_angle_and_axis() {
    let with = |flip: u8, angle: f64| {
        let mut r = base_records();
        r.push((NAME, name_rec("Circular Pattern1", Some(0xb6c2_2694)))); // 19
        r.push((PLACEMENT, hdr(0, 0, 0))); // 20
        r.push((15, {
            let mut v = hdr(0, 0, 0);
            v.extend(utf16("RDxVar2"));
            v.extend(0u32.to_le_bytes());
            v
        })); // 21: the other count (0)
        r.push((15, {
            let mut v = hdr(0, 0, 0);
            v.extend(utf16("d3"));
            v.extend(6u32.to_le_bytes());
            v.extend(6u32.to_le_bytes());
            v
        })); // 22: the count
        r.push((PARAM, param("d4", angle))); // 23
        r.push((13, {
            let mut v = hdr(0, 0, 0);
            v.extend(list(&[]));
            doubles(&mut v, &[1.0, 2.0, 0.0, 0.0, 0.0, 2.0]);
            v
        })); // 24: axis through (1, 2, 0) cm along z
        r.push((16, {
            let mut v = hdr(0, 0, 0);
            v.push(flip);
            v
        })); // 25
        let mut f = hdr(rf(19), 0, 6);
        f.extend(0u32.to_le_bytes());
        f.extend(list(&[rf(18)]));
        for x in [20, 21, 22, 23, 24, 25] {
            f.extend(rf(x).to_le_bytes());
        }
        r.push((11, f)); // 26
        copy_tree(&r)
    };
    let t = with(0, std::f64::consts::TAU);
    assert_eq!(
        t["features"][2],
        json!({"type": "pattern_circular", "name": "Circular Pattern1", "features": ["Extrusion1"],
               "axis": {"origin": [10.0, 20.0, 0.0], "direction": [0.0, 0.0, 1.0]}, "angle": 360.0, "count": 6})
    );
    let t = with(1, std::f64::consts::FRAC_PI_2);
    assert_eq!(t["features"][2]["axis"]["direction"], json!([0.0, 0.0, -1.0]));
    assert_eq!(t["features"][2]["angle"], 90.0);
}

/// A mirror (`fad9a9b5`): the copied features, and the plane of its last work-plane record
/// (a point and two unit vectors, cm). A copy of a feature that was not read is refused.
#[test]
fn a_mirror_reads_its_features_and_plane() {
    let mut r = base_records();
    r.push((NAME, name_rec("Mirror1", Some(0xb6c2_26a3)))); // 19
    r.push((PLACEMENT, hdr(0, 0, 0))); // 20
    r.push((14, {
        let mut v = hdr(0, 0, 0);
        doubles(&mut v, &[3.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        v
    })); // 21
    let mut f = hdr(rf(19), 0, 6);
    f.extend(0u32.to_le_bytes());
    f.extend(list(&[rf(18)]));
    f.extend(rf(20).to_le_bytes());
    f.extend(rf(21).to_le_bytes());
    r.push((12, f)); // 22
    let t = copy_tree(&r);
    assert_eq!(
        t["features"][2],
        json!({"type": "mirror", "name": "Mirror1", "features": ["Extrusion1"],
               "plane": {"origin": [30.0, 0.0, 0.0], "normal": [1.0, 0.0, 0.0]}})
    );
    // The extrude not read (an extent that is not read): the mirror says why.
    r[17].1 = tail(9);
    let t = copy_tree(&r);
    assert_eq!(t["features"][1]["type"], "unsupported");
    assert!(t["features"][1]["reason"].as_str().unwrap().contains("Extrusion1 is not read"), "{t}");
}
