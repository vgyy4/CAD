//! Tests on synthetic input (the real files are not in the repository; the sample-gated test in
//! `gusset-cli/tests/f3d_history.rs` covers them).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::geom::{self, P2};
use super::json::{check, place};
use super::stream::{Design, parse_header, ref_end, ref_lists, refs_at, sections, wstrings_all};
use super::{acis, feature, naming, sketch};
use serde_json::json;

fn le32(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn le64(v: u64) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn s(text: &str) -> Vec<u8> {
    let mut v = le32(text.len() as u32);
    v.extend_from_slice(text.as_bytes());
    v
}
fn ws(text: &str) -> Vec<u8> {
    let mut v = le32(text.chars().count() as u32);
    v.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    v
}
fn r#ref(id: u64) -> Vec<u8> {
    let mut v = vec![1];
    v.extend(le64(id));
    v.push(0);
    v
}

const G1: &str = "11111111-2222-3333-4444-555555555555";
const G2: &str = "AAAAAAAA-2222-3333-4444-555555555555";

/// A MetaStream with two classes and the given objects (id, offset into the bulk).
fn meta(objs: &[(u64, u64)], class_of: &dyn Fn(u64) -> usize) -> Vec<u8> {
    let mut m = s("Design");
    m.extend(le32(0));
    m.extend(ws(""));
    m.extend(le32(2));
    for (k, g) in [G1, G2].iter().enumerate() {
        m.extend(s(g));
        m.extend(s(""));
        m.extend(le32(1));
        m.extend(s("Fusion"));
        let ids: Vec<u64> = objs.iter().map(|o| o.0).filter(|i| class_of(*i) == k).collect();
        m.extend(le32(ids.len() as u32));
        for i in ids {
            m.extend(le64(i));
        }
    }
    m.extend(le32(0));
    m.extend(le32(objs.len() as u32));
    for (i, o) in objs {
        m.extend(le64(*i));
        m.extend(le64(*o));
    }
    m.extend(le32(0));
    m.extend(vec![0; 64]);
    m
}

/// A record: tag text, id, `u32 0`, `u8 0`, flag 0, then the body.
fn record(tag: u32, id: u64, body: &[u8]) -> Vec<u8> {
    let mut v = s(&tag.to_string());
    v.extend(le64(id));
    v.extend(le32(0));
    v.extend([0, 0]);
    v.extend_from_slice(body);
    v
}

#[test]
fn a_synthetic_stream_frames_and_its_references_resolve() {
    let r5 = record(256, 5, &[9, 9, 9]);
    let mut body6 = r#ref(5);
    body6.extend(le32(2));
    body6.extend(r#ref(5));
    body6.extend(r#ref(6));
    let r6 = record(257, 6, &body6);
    let off6 = r5.len() as u64;
    let bulk = [r5.clone(), r6.clone()].concat();
    let m = meta(&[(5, 0), (6, off6)], &|i| usize::from(i == 6));
    let d = Design::new(&m, &bulk).unwrap();
    assert_eq!(d.ids, vec![5, 6]);
    assert_eq!(d.rec(5), &r5[..]);
    assert_eq!(d.rec(6), &r6[..]);
    assert!(d.is(6, "AAAAAAAA"));
    assert!(!d.is(5, "AAAAAAAA"));
    assert_eq!(d.tag_class(257), Some(G2));
    assert_eq!(d.tag_class(255), None);
    let h = parse_header(d.rec(6)).unwrap();
    assert_eq!(h.body, 21);
    let refs = refs_at(&d, d.rec(6), h.body);
    assert_eq!(refs.iter().map(|r| r.1).collect::<Vec<_>>(), vec![5, 5, 6]);
    // `u32 2` then two references.
    let lists = ref_lists(&d, d.rec(6), h.body);
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].1, vec![5, 6]);
    // A reference to an object that does not exist is none.
    assert!(ref_end(&d, &r#ref(77), 0).is_none());
    // Sections: the own tag first.
    assert_eq!(sections(d.rec(6)).unwrap(), vec![(0, 257)]);
}

#[test]
fn a_damaged_class_table_is_refused() {
    let m = meta(&[(5, 0)], &|_| 0);
    assert!(Design::new(&m[..40], &[]).is_none());
    assert!(Design::new(&[], &[]).is_none());
    assert!(Design::new(&[0xff; 200], &[]).is_none());
}

#[test]
fn headers_carry_typed_attributes() {
    let mut rec = s("300");
    rec.extend(le64(7));
    rec.extend(le32(0));
    rec.extend([0, 1]);
    rec.extend(le32(2));
    rec.extend(s("crv_primary_id"));
    rec.extend(s("IntrinsicMetaTypeuint64"));
    rec.extend(le64(0x65));
    rec.extend(s("name"));
    rec.extend(s("IntrinsicMetaTypeIString"));
    rec.extend(ws("abc"));
    rec.extend([7, 7]);
    let h = parse_header(&rec).unwrap();
    assert_eq!(h.attrs.get("crv_primary_id"), Some(&le64(0x65)));
    assert_eq!(h.body, rec.len() - 2);
    // An unknown attribute type leaves the header without attributes.
    let mut bad = s("300");
    bad.extend(le64(7));
    bad.extend(le32(0));
    bad.extend([0, 1]);
    bad.extend(le32(1));
    bad.extend(s("k"));
    bad.extend(s("NoSuchType"));
    bad.extend([0; 8]);
    assert!(parse_header(&bad).unwrap().attrs.is_empty());
}

#[test]
fn utf16_strings_are_found_and_empty_ones_are_not() {
    let mut b = vec![5, 5];
    b.extend(ws("Radius"));
    b.extend(ws(""));
    b.extend(ws("5 mm"));
    let w = wstrings_all(&b);
    assert_eq!(w.iter().map(|x| x.2.as_str()).collect::<Vec<_>>(), vec!["Radius", "5 mm"]);
}

fn square() -> Vec<Vec<P2>> {
    vec![
        vec![[0.0, 0.0], [4.0, 0.0]],
        vec![[4.0, 4.0], [4.0, 0.0]], // reversed
        vec![[4.0, 4.0], [0.0, 4.0]],
        vec![[0.0, 4.0], [0.0, 0.0]],
    ]
}

#[test]
fn segments_chain_into_a_ring_whatever_their_direction() {
    let ring = geom::chain(&square()).unwrap();
    assert_eq!(ring.len(), 4);
    let p = geom::interior_point(&ring, &[]).unwrap();
    assert!(geom::inside(p, &ring));
    // A gap leaves no ring.
    let mut open = square();
    open.pop();
    assert!(geom::chain(&open).is_none());
    // A hole keeps the point out of it.
    let hole = geom::chain(&[
        vec![[1.0, 1.0], [3.0, 1.0]],
        vec![[3.0, 1.0], [3.0, 3.0]],
        vec![[3.0, 3.0], [1.0, 3.0]],
        vec![[1.0, 3.0], [1.0, 1.0]],
    ])
    .unwrap();
    let q = geom::interior_point(&ring, &[&hole]).unwrap();
    assert!(geom::inside(q, &ring) && !geom::inside(q, &hole));
}

#[test]
fn a_closed_curve_cut_at_its_start_has_two_pieces_not_three() {
    // A circle starting on the cutting line: the crossing at its start comes out as 0 and as 1.
    let circle: Vec<P2> = (0..=64)
        .map(|k| {
            let t = f64::from(k) / 64.0 * std::f64::consts::TAU;
            [libm::cos(t), libm::sin(t)]
        })
        .collect();
    let cut: Vec<P2> = vec![[-2.0, 0.0], [2.0, 0.0]];
    let others: Vec<&[P2]> = vec![&cut];
    let a = geom::piece(&circle, true, &others, 1, 2).unwrap();
    let b = geom::piece(&circle, true, &others, 2, 2).unwrap();
    assert!(a.iter().all(|p| p[1] >= -1e-9) && b.iter().all(|p| p[1] <= 1e-9));
}

#[test]
fn open_ends_of_a_loop_are_joined_by_the_one_curve_that_fits() {
    let sq: Vec<Vec<P2>> =
        vec![vec![[0.0, 0.0], [1.0, 0.0]], vec![[1.0, 1.0], [0.0, 1.0]], vec![[0.0, 1.0], [0.0, 0.0]]];
    let cands: Vec<(u64, Option<Vec<P2>>)> =
        vec![(1, Some(vec![[1.0, 0.0], [2.0, 2.0]])), (2, Some(vec![[1.0, 0.0], [1.0, 0.5], [1.0, 1.0]])), (3, None)];
    let mut polys = sq.clone();
    assert!(geom::chain(&polys).is_none());
    let added = geom::close_gaps(&mut polys, &cands, &[]);
    assert_eq!(added.iter().map(|a| a.0).collect::<Vec<_>>(), vec![2]);
    assert!(geom::chain(&polys).is_some());
    // A curve the loop already uses is not added again.
    let mut again = sq;
    assert!(geom::close_gaps(&mut again, &cands, &[2]).is_empty());
}

#[test]
fn a_body_pattern_names_its_body_and_does_not_chain() {
    let dirs: Vec<(geom::V3, geom::V3)> = vec![([0.0; 3], [0.0, 1.0, 0.0])];
    let mk = |count_v: f64, dirs: &[(geom::V3, geom::V3)]| {
        let p = super::json::PatternInputs {
            name: "R-Pattern1",
            originals: &[],
            count: 4.0,
            count_v,
            spacing: 5.0,
            spacing_v: 3.0,
            dirs,
            sym: [false; 3],
            target: Some("Extrude5"),
        };
        super::json::linear_pattern_chain(&p)
    };
    let one = mk(1.0, &dirs).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0]["target"], "Extrude5");
    assert_eq!(one[0]["features"], json!([]));
    // Two directions would need the first copies as features.
    let two: Vec<(geom::V3, geom::V3)> = vec![dirs[0], ([0.0; 3], [1.0, 0.0, 0.0])];
    assert!(mk(2.0, &two).is_none());
}

#[test]
fn a_curve_is_cut_at_its_crossings_into_numbered_pieces() {
    let line: Vec<P2> = vec![[0.0, 0.0], [10.0, 0.0]];
    let c1: Vec<P2> = vec![[3.0, -1.0], [3.0, 1.0]];
    let c2: Vec<P2> = vec![[7.0, -1.0], [7.0, 1.0]];
    let others: Vec<&[P2]> = vec![&c1, &c2];
    let mid = geom::piece(&line, false, &others, 2, 3).unwrap();
    assert!((mid[0][0] - 3.0).abs() < 1e-9 && (mid[mid.len() - 1][0] - 7.0).abs() < 1e-9);
    assert!(geom::piece(&line, false, &others, 1, 2).is_none());
    assert!(geom::piece(&line, false, &others, 0, 3).is_none());
    let first = geom::trim_by_neighbours(&line, Some(&c1), Some(&c2), 1, 3).unwrap();
    assert!((first[first.len() - 1][0] - 3.0).abs() < 1e-9);
}

#[test]
fn a_profile_record_parses_exactly_up_to_the_next_section() {
    // Objects: 5 the profile (owner), 6 the geometry record.
    let r5 = record(256, 5, &[1, 2, 3]);
    let mut body = r#ref(5);
    body.push(0); // skip byte
    body.extend(le32(1)); // regions
    body.extend(le32(1)); // loops
    body.extend(le32(2)); // curves
    for (p, sec) in [(11u64, 12u64), (13, 14)] {
        body.extend(le32(2));
        body.extend(le64(p));
        body.extend(le64(sec));
        body.extend(le32(0));
        body.extend(le32(1));
        body.extend(le32(1));
        body.extend([0; 8]);
    }
    body.push(1); // outer
    body.extend(le32(0)); // no faces
    let r6 = record(257, 6, &body);
    let off6 = r5.len() as u64;
    let bulk = [r5, r6].concat();
    let m = meta(&[(5, 0), (6, off6)], &|i| usize::from(i == 6));
    let d = Design::new(&m, &bulk).unwrap();
    let regs = sketch::parse_profiles(&d, 6).unwrap();
    assert_eq!(regs.len(), 1);
    let l = &regs[0].loops[0];
    assert_eq!((l.flag, l.curves.len()), (1, 2));
    assert_eq!((l.curves[1].primary, l.curves[1].secondary), (13, 14));
    // One byte of trailing noise: not an exact parse.
    let mut noisy = bulk.clone();
    noisy.extend([9]);
    let d2 = Design::new(&m, &noisy).unwrap();
    assert!(sketch::parse_profiles(&d2, 6).is_none());
}

#[test]
fn a_recipe_names_its_faces_by_generator_and_operation() {
    let mut body = Vec::new();
    body.extend(le32(1));
    body.extend(le32(3));
    body.extend(le32(2)); // two faces
    for (g, op) in [("3", 303i32), ("-4", 304)] {
        body.extend(le32(1)); // one entry
        body.extend(s(g));
        body.extend(le32(0));
        body.extend(le32(1));
        body.extend(op.to_le_bytes());
        body.extend(le32(0));
    }
    body.extend(s("edge_recipe_data"));
    let mut rec = record(256, 5, &[]);
    rec.truncate(15);
    rec.extend(&body);
    let m = meta(&[(5, 0)], &|_| 0);
    let d = Design::new(&m, &rec).unwrap();
    let (kind, faces) = naming::recipe(&d, 5);
    assert_eq!(kind, Some(naming::RecipeKind::Edge));
    let faces = faces.unwrap();
    assert_eq!(faces, vec![vec![("3".to_string(), vec![303])], vec![("-4".to_string(), vec![304])]]);
}

#[test]
fn the_planes_of_two_faces_meet_in_a_line() {
    let (p, u) = naming::plane_plane([1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 3.0, 0.0]).unwrap();
    assert!((p[0] - 2.0).abs() < 1e-12 && (p[1] - 3.0).abs() < 1e-12);
    assert!((u[2].abs() - 1.0).abs() < 1e-12);
    assert!(naming::plane_plane([1.0, 0.0, 0.0], [0.0; 3], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]).is_none());
}

fn token_str(tag: u8, text: &str) -> Vec<u8> {
    let mut v = vec![tag, text.len() as u8];
    v.extend(text.as_bytes());
    v
}

/// A record: type name parts and pointers, then the end mark.
fn sab_record(names: &[&str], ptrs: &[i32], pos: Option<[f64; 3]>) -> Vec<u8> {
    let mut v = Vec::new();
    for (i, n) in names.iter().enumerate() {
        v.extend(token_str(if i + 1 == names.len() { 0x0d } else { 0x0e }, n));
    }
    for p in ptrs {
        v.push(0x0c);
        v.extend(p.to_le_bytes());
    }
    if let Some(p) = pos {
        v.push(0x13);
        for x in p {
            v.extend(x.to_le_bytes());
        }
    }
    v.push(0x11);
    v
}

#[test]
fn an_acis_blob_gives_the_edges_of_each_body() {
    // 0 header, 1 body -> edge 2 -> vertices 3, 4 -> points 5, 6.
    let mut b = b"ASM BinaryFile4tY".to_vec();
    b.extend(sab_record(&["asmheader"], &[], None));
    b.extend(sab_record(&["body"], &[2], None));
    b.extend(sab_record(&["edge"], &[3, 4], None));
    b.extend(sab_record(&["vertex"], &[5], None));
    b.extend(sab_record(&["vertex"], &[6], None));
    b.extend(sab_record(&["point"], &[], Some([0.0, 0.0, 0.0])));
    b.extend(sab_record(&["point"], &[], Some([1.0, 2.0, 3.0])));
    let edges = acis::body_edges(&b).unwrap();
    assert_eq!(edges, vec![vec![([0.0, 0.0, 0.0], [1.0, 2.0, 3.0])]]);
    // An unknown token stops the whole blob.
    let mut bad = b.clone();
    bad.push(0xee);
    assert!(acis::body_edges(&bad).is_none());
    assert!(acis::body_edges(b"not acis").is_none());
}

#[test]
fn a_symmetric_pattern_direction_becomes_two_chained_patterns() {
    let out = {
        let dirs = [([0.0; 3], [0.0, 0.0, -1.0]), ([0.0; 3], [1.0, 0.0, 0.0])];
        let originals = vec!["Hole1".to_string()];
        let p = super::json::PatternInputs {
            name: "R-Pattern2",
            originals: &originals,
            count: 3.0,
            count_v: 3.0,
            spacing: 7.0,
            spacing_v: 30.0,
            dirs: &dirs,
            sym: [true, false, false],
            target: None,
        };
        super::json::linear_pattern_chain(&p).unwrap()
    };
    let names: Vec<&str> = out.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["R-Pattern2", "R-Pattern2 U2", "R-Pattern2 V"]);
    assert_eq!(out[1]["spacing"], json!(-7.0));
    assert_eq!(out[1]["count"], json!(2));
    assert_eq!(out[2]["features"], json!(["Hole1", "R-Pattern2", "R-Pattern2 U2"]));
    // An even symmetric count has no middle original.
    let dirs = [([0.0; 3], [1.0, 0.0, 0.0])];
    let originals = vec!["E".to_string()];
    let p = super::json::PatternInputs {
        name: "P",
        originals: &originals,
        count: 4.0,
        count_v: 1.0,
        spacing: 1.0,
        spacing_v: 0.0,
        dirs: &dirs,
        sym: [true, false, false],
        target: None,
    };
    assert!(super::json::linear_pattern_chain(&p).is_none());
}

#[test]
fn the_check_names_the_first_feature_it_cannot_read() {
    let tree = json!({"features": [
        {"type": "sketch", "name": "Sketch1", "plane": {"origin": [0, 0, 0]}},
        {"type": "extrude", "name": "Extrude1", "profiles": [{"point": [1, 1]}], "distance": 5.0, "operation": "new"},
        {"type": "unsupported", "name": "Draft1", "fusion_type": "Draft"},
        {"type": "unsupported", "name": "Shell1", "fusion_type": "Shell"},
    ]});
    let why = check(&tree).unwrap_err();
    assert!(why.contains("Draft1") && why.contains("Draft") && !why.contains("Shell1"), "{why}");
    let two_sided = json!({"features": [
        {"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new", "two_sided": true},
    ]});
    assert!(check(&two_sided).unwrap_err().contains("both sides"));
    let unresolved = json!({"features": [
        {"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"},
        {"type": "fillet", "name": "F", "radius": 1.0, "edges": [{"midpoint": [0, 0, 0]}], "edges_unresolved": 2},
    ]});
    assert!(check(&unresolved).unwrap_err().contains("fillet edges"));
    assert!(check(&json!({"features": []})).unwrap_err().contains("no feature timeline"));
}

#[test]
fn a_components_features_move_with_its_placement() {
    let mut tree = json!({
    "components": [{"source_id": 9, "bodies": ["BREP.abc.smb"]}],
    "features": [
        {"type": "sketch", "name": "S", "_component": 9,
         "plane": {"origin": [1.0, 0.0, 0.0], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]}},
        {"type": "fillet", "name": "F", "_component": 9, "edges": [{"midpoint": [0.0, 0.0, 0.0]}]},
        {"type": "pattern_linear", "name": "P", "_component": 9, "direction": [1.0, 0.0, 0.0]},
    ]});
    // A quarter turn about z and a lift of 5 mm.
    let pl = crate::fusion::Placement { r: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0, 0.0, 5.0] };
    let placed = vec![crate::fusion::Placed { brep: "abc".into(), occurrence: "o".into(), placement: pl }];
    place(&mut tree, &placed).unwrap();
    assert_eq!(tree["features"][0]["plane"]["origin"], json!([0.0, 1.0, 5.0]));
    assert_eq!(tree["features"][0]["plane"]["x_axis"], json!([0.0, 1.0, 0.0]));
    assert_eq!(tree["features"][1]["edges"][0]["midpoint"], json!([0.0, 0.0, 5.0]));
    assert_eq!(tree["features"][2]["direction"], json!([0.0, 1.0, 0.0]));
    // Two placements of one blob are two occurrences: the features are written once for each.
    let mut two = placed.clone();
    two.push(crate::fusion::Placed {
        brep: "abc".into(),
        occurrence: "p".into(),
        placement: crate::fusion::Placement { r: crate::fusion::Placement::IDENTITY.r, t: [10.0, 0.0, 0.0] },
    });
    let mut tree = json!({
    "components": [{"source_id": 9, "bodies": ["BREP.abc.smb"]}],
    "features": [
        {"type": "sketch", "name": "S", "_component": 9,
         "plane": {"origin": [0.0, 0.0, 0.0], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]}},
        {"type": "extrude", "name": "E", "_component": 9, "sketch": "S", "operation": "new"},
        {"type": "extrude", "name": "Other", "sketch": "S2", "operation": "new"},
        {"type": "extrude", "name": "Cut", "_component": 9, "sketch": "S", "operation": "cut", "target": "E"},
    ]});
    place(&mut tree, &two).unwrap();
    let names: Vec<&str> = tree["features"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["S", "S [2]", "E", "E [2]", "Other", "Cut", "Cut [2]"]);
    assert_eq!(tree["features"][3]["sketch"], "S [2]");
    assert_eq!(tree["features"][6]["target"], "E [2]");
    assert_eq!(tree["features"][0]["plane"]["origin"], json!([0.0, 0.0, 5.0]));
    assert_eq!(tree["features"][1]["plane"]["origin"], json!([10.0, 0.0, 0.0]));
    // Different blobs of one component placed in different ways are refused.
    let mut mixed = json!({
    "components": [{"source_id": 9, "bodies": ["BREP.abc.smb", "BREP.def.smb"]}],
    "features": [{"type": "sketch", "name": "S", "_component": 9}]});
    let mut other = two.clone();
    if let Some(o) = other.get_mut(1) {
        o.brep = "def".into();
    }
    assert!(place(&mut mixed, &other).is_err());
}

#[test]
fn a_sweep_needs_its_profile_and_path() {
    let ok = json!({"type": "sweep", "name": "Sweep1", "sketch": "S2", "path": "S1", "path_curves": ["c1"],
        "operation": "new"});
    assert!(
        check(&json!({"features": [
        {"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"}, ok.clone()]}))
        .is_ok()
    );
    let mut bad = ok.clone();
    bad["error"] = "sweep path is not a list of sketch curves of one sketch".into();
    let why = check(&json!({"features": [
        {"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"}, bad]}))
    .unwrap_err();
    assert!(why.contains("Sweep1") && why.contains("path"), "{why}");
    let mut join = ok;
    join["operation"] = "join".into();
    assert!(
        check(&json!({"features": [
        {"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"}, join]}))
        .unwrap_err()
        .contains("body it changes")
    );
}

/// Deterministic noise and damage: nothing may panic.
#[test]
fn damaged_input_is_refused_without_panics() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    // Random meta/bulk pairs.
    for round in 0..200 {
        let n = (next() % 600) as usize;
        let junk: Vec<u8> = (0..n).map(|_| next() as u8).collect();
        let files = vec![
            ("FusionAssetName[Active]/Design1/MetaStream.dat".to_string(), junk.clone()),
            ("FusionAssetName[Active]/Design1/BulkStream.dat".to_string(), junk.iter().rev().copied().collect()),
            ("FusionAssetName[Active]/Breps.BlobParts/BREP.x.smb".to_string(), junk),
        ];
        assert!(super::feature_tree(&files, "f.f3d").is_err(), "round {round}");
    }
    // A valid frame with damaged records and damaged ACIS.
    let r5 = record(256, 5, &[0xff; 40]);
    let m = meta(&[(5, 0)], &|_| 0);
    for flip in 0..r5.len() {
        let mut bulk = r5.clone();
        bulk[flip] ^= 0xa5;
        let files = vec![
            ("FusionAssetName[Active]/Design1/MetaStream.dat".to_string(), m.clone()),
            ("FusionAssetName[Active]/Design1/BulkStream.dat".to_string(), bulk),
        ];
        assert!(super::feature_tree(&files, "f.f3d").is_err());
    }
    for round in 0..300 {
        let n = (next() % 400) as usize;
        let mut b = b"ASM BinaryFile4tY\x0d\x09asmheader\x11".to_vec();
        b.extend((0..n).map(|_| [0x0c, 0x0d, 0x11, 0x13, 0x07, 0x0e, next() as u8][(next() % 7) as usize]));
        let _ = (acis::body_edges(&b), acis::tagged_faces(&b), round);
    }
    // Profile, recipe and operation-table readers on noise.
    for _ in 0..200 {
        let n = (next() % 300) as usize;
        let mut rec = record(256, 5, &[]);
        rec.extend((0..n).map(|_| next() as u8));
        let m = meta(&[(5, 0)], &|_| 0);
        let d = Design::new(&m, &rec).unwrap();
        let _ = (sketch::parse_profiles(&d, 5), naming::recipe(&d, 5), naming::op_map(&d), feature::extrude_raw(&d, 5));
        let _ = (feature::feature_name(&d, 5), feature::params_of(&d, 5), sections(d.rec(5)), refs_at(&d, d.rec(5), 0));
    }
}

#[test]
fn a_file_without_a_design_stream_is_not_a_design() {
    let why = super::feature_tree(&[("a.txt".to_string(), vec![1])], "f").unwrap_err();
    assert!(why.starts_with("the file has no"), "{why}");
}

#[test]
fn a_closed_curve_cut_off_its_start_numbers_the_piece_holding_the_start_first() {
    // A circle starting on the right, touched by two lines at the top and the bottom: piece 1
    // is the half through the start (the right one), piece 2 the left half.
    let circle: Vec<P2> = (0..=64)
        .map(|k| {
            let t = f64::from(k) / 64.0 * std::f64::consts::TAU;
            [libm::cos(t), libm::sin(t)]
        })
        .collect();
    let top: Vec<P2> = vec![[-0.5, 1.0], [0.5, 1.0]];
    let bottom: Vec<P2> = vec![[-0.5, -1.0], [0.5, -1.0]];
    let others: Vec<&[P2]> = vec![&top, &bottom];
    let a = geom::piece(&circle, true, &others, 1, 2).unwrap();
    let b = geom::piece(&circle, true, &others, 2, 2).unwrap();
    assert!(a.iter().all(|p| p[0] >= -1e-9) && b.iter().all(|p| p[0] <= 1e-9));
}

/// A placement that turns 90 degrees about z and shifts by (10, 0, 5) mm.
fn quarter_turn() -> crate::fusion::Placement {
    crate::fusion::Placement { r: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], t: [10.0, 0.0, 5.0] }
}

fn inserted_features() -> Vec<serde_json::Value> {
    vec![
        json!({"type": "sketch", "name": "Sketch1", "plane": {"origin": [1.0, 0.0, 0.0], "x_axis": [1.0, 0.0, 0.0],
            "y_axis": [0.0, 1.0, 0.0]}, "_component": 7}),
        json!({"type": "extrude", "name": "Extrude1", "sketch": "Sketch1", "operation": "new", "_component": 7}),
        json!({"type": "extrude", "name": "Extrude2", "sketch": "Sketch1", "operation": "cut", "target": "Extrude1"}),
        json!({"type": "stored_body", "name": "Base", "brep": "BREP.a.smbh", "index": 1, "count": 2, "body": 0,
            "placement": {"r": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "t": [1.0, 2.0, 3.0]}}),
    ]
}

#[test]
fn an_inserted_design_is_moved_and_named_apart() {
    let out = super::insert::bring(inserted_features(), "Component Insert1", &quarter_turn()).unwrap();
    let names: Vec<&str> = out.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "Sketch1 [Component Insert1]",
            "Extrude1 [Component Insert1]",
            "Extrude2 [Component Insert1]",
            "Base [Component Insert1]"
        ]
    );
    // References follow the new names; the reader's own keys are gone.
    assert_eq!(out[1]["sketch"], "Sketch1 [Component Insert1]");
    assert_eq!(out[2]["target"], "Extrude1 [Component Insert1]");
    assert_eq!(out[1]["operation"], "new");
    assert!(out.iter().all(|f| f.get("_component").is_none() && f.get("body").is_none()));
    // The sketch plane: origin (1, 0, 0) -> (10, 1, 5); x axis -> y, y axis -> -x.
    assert_eq!(out[0]["plane"]["origin"], json!([10.0, 1.0, 5.0]));
    assert_eq!(out[0]["plane"]["x_axis"], json!([0.0, 1.0, 0.0]));
    assert_eq!(out[0]["plane"]["y_axis"], json!([-1.0, 0.0, 0.0]));
    // A stored body that already stood somewhere moves on: turned, then shifted.
    let p = super::json::placement_of(&out[3]["placement"]).unwrap();
    assert_eq!(p.t, [8.0, 1.0, 8.0]);
    assert_eq!(p.r, quarter_turn().r);
}

#[test]
fn an_insert_at_the_origin_keeps_the_features_in_place() {
    let out =
        super::insert::bring(inserted_features(), "Component Insert1", &crate::fusion::Placement::IDENTITY).unwrap();
    assert_eq!(out[0]["plane"]["origin"], json!([1.0, 0.0, 0.0]));
    assert_eq!(out[3]["placement"]["t"], json!([1.0, 2.0, 3.0]));
}

#[test]
fn features_that_cannot_be_moved_are_refused_for_a_moved_insert() {
    let sweep = vec![json!({"type": "sweep", "name": "Sweep1"})];
    let e = super::insert::bring(sweep.clone(), "Component Insert1", &quarter_turn()).unwrap_err();
    assert!(e.contains("Sweep1"), "{e}");
    assert!(super::insert::bring(sweep, "Component Insert1", &crate::fusion::Placement::IDENTITY).is_ok());
}

#[test]
fn an_insert_nobody_resolves_stays_and_the_check_names_it() {
    let mut tree = json!({"features": [
        {"type": "component_insert", "name": "Component Insert1", "fusion_type": "Component Insert",
         "insert_keys": ["aa"], "placement": {"r": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "t": [0.0, 0.0, 0.0]}},
        {"type": "extrude", "name": "E", "operation": "new", "sketch": "S", "profiles": "all", "distance": 1.0,
         "creates_bodies": 1}]});
    let asked = std::cell::Cell::new(0);
    super::insert::splice(
        &mut tree,
        &mut |keys| {
            assert_eq!(keys, ["aa".to_string()]);
            asked.set(asked.get() + 1);
            None
        },
        0,
    )
    .unwrap();
    assert_eq!(asked.get(), 1);
    assert_eq!(tree["features"].as_array().unwrap().len(), 2);
    let why = check(&tree).unwrap_err();
    assert!(why.contains("Component Insert1") && why.contains("not merged"), "{why}");
}
