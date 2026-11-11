//! Tests on synthetic input (the real files are not in the repository; the sample-gated test in
//! `gusset-cli/tests/f3d_history.rs` covers them).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::geom::{self, P2};
use super::json::{self, check, place};
use super::stream::{Design, parse_header, ref_end, ref_lists, refs_at, sections, wstrings_all};
use super::{acis, feature, naming, patdir, pcb, sketch};
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
fn a_circle_drawn_twice_is_cut_by_the_other_copys_seam_not_everywhere() {
    // A chord's two ends on a circle and the same circle drawn again: the copy touches the
    // circle everywhere, so only its ends (the shared seam) cut it, giving 3 pieces.
    let circle: Vec<P2> = (0..=64)
        .map(|k| {
            let t = f64::from(k) / 64.0 * std::f64::consts::TAU;
            [libm::cos(t), libm::sin(t)]
        })
        .collect();
    let y = 0.8;
    let x = libm::sqrt(1.0 - y * y);
    let chord: Vec<P2> = vec![[x, y], [-x, y]];
    let copy = circle.clone();
    let others: Vec<&[P2]> = vec![&chord, &copy];
    assert!(geom::piece(&circle, true, &others, 1, 2).is_none());
    // Piece 2 of 3 is the cap above the chord.
    let cap = geom::piece(&circle, true, &others, 2, 3).unwrap();
    assert!(cap.iter().all(|p| p[1] >= y - 1e-9), "{cap:?}");
    // (Where the chord meets the circle's chords, a little inside the true circle.)
    assert!(geom::dist(*cap.first().unwrap(), [x, y]) < 1e-2);
    assert!(geom::dist(*cap.last().unwrap(), [-x, y]) < 1e-2);
}

#[test]
fn a_loop_walked_out_and_back_revisits_its_points() {
    let sq: Vec<P2> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
    assert!(!geom::revisits(&sq));
    // Round twice (a circle drawn twice, chained at its seam).
    let mut twice = sq.clone();
    twice.extend(sq.iter().skip(1));
    assert!(geom::revisits(&twice));
    // Out along an edge and back on its copy.
    let spike: Vec<P2> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    assert!(geom::revisits(&spike));
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
    // Each copy of a body is a body of its own in the file.
    assert_eq!(one[0]["merge"], false);
    assert_eq!(super::json::copy_name("R-Pattern1", 2), "R-Pattern1[2]");
    // A component that stands in a second place: the copies follow their pattern's new name.
    let mut combine = json!({"type": "combine", "name": "Combine1", "target": "Extrude5",
        "tools": ["R-Pattern1[1]", "R-Pattern1[3]", "Other[1]"]});
    let map = [("R-Pattern1".to_string(), "R-Pattern1 [2]".to_string())].into_iter().collect();
    super::json::rename(&mut combine, &map);
    assert_eq!(combine["tools"], json!(["R-Pattern1 [2][1]", "R-Pattern1 [2][3]", "Other[1]"]));
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
    // An extrude whose start face is not known, or whose start was not moved into its sketch.
    let start = |extra: serde_json::Value| {
        let mut e = json!({"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"});
        if let (Some(e), serde_json::Value::Object(x)) = (e.as_object_mut(), extra) {
            e.extend(x);
        }
        check(&json!({"features": [e]})).unwrap_err()
    };
    assert!(
        start(json!({"start_error": "its start face is not a plane parallel to the sketch"})).contains("start face")
    );
    assert!(start(json!({"start": 1.6})).contains("off its sketch plane"));
    let unresolved = json!({"features": [
        {"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"},
        {"type": "fillet", "name": "F", "radius": 1.0, "edges": [{"midpoint": [0, 0, 0]}], "edges_unresolved": 2},
    ]});
    assert!(check(&unresolved).unwrap_err().contains("fillet edges"));
    assert!(check(&json!({"features": []})).unwrap_err().contains("no feature timeline"));
    // A revolve needs its profile, axis and angle; a taper is refused only with a symmetric extent.
    let rev = |extra: serde_json::Value| {
        let mut r = json!({"type": "revolve", "name": "R", "profiles": [{"point": [1, 1]}],
            "axis": {"line": "c1"}, "angle": 360.0, "operation": "new"});
        if let (Some(r), serde_json::Value::Object(x)) = (r.as_object_mut(), extra) {
            r.extend(x);
        }
        let e = json!({"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"});
        check(&json!({"features": [e, r]}))
    };
    assert!(rev(json!({})).is_ok());
    assert!(rev(json!({"axis": null})).unwrap_err().contains("axis"));
    assert!(rev(json!({"error": "a part revolve"})).unwrap_err().contains("a part revolve"));
    assert!(rev(json!({"operation": "join"})).unwrap_err().contains("not one body"));
    let taper = |sym: bool| {
        check(&json!({"features": [{"type": "extrude", "name": "E", "profiles": [], "distance": 5.0,
            "operation": "new", "taper_deg": -3.0, "symmetric": sym}]}))
    };
    assert!(taper(false).is_ok());
    assert!(taper(true).unwrap_err().contains("tapered and symmetric"));
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
        let _ = (acis::body_edges(&b), acis::tagged_faces(&b), acis::named_edges(&b), round);
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

#[test]
fn a_root_feature_that_names_the_insert_is_pointed_at_the_merged_body() {
    use super::insert::{final_bodies, retarget};
    let brought = vec![
        json!({"type": "extrude", "name": "Board [In]", "operation": "new", "creates_bodies": 1}),
        json!({"type": "extrude", "name": "Hole [In]", "operation": "cut", "target": "Board [In]", "creates_bodies": 0}),
        json!({"type": "stored_body", "name": "Part [In]", "creates_bodies": 1}),
    ];
    // The cut keeps its target's name: the board and the part are what the insert leaves.
    assert_eq!(final_bodies(&brought), vec!["Board [In]".to_string(), "Part [In]".to_string()]);
    let one = [("In".to_string(), vec!["Part [In]".to_string()])];
    let mut feats = vec![
        json!({"type": "extrude", "name": "A", "operation": "cut", "target": "In"}),
        json!({"type": "extrude", "name": "B", "operation": "join", "target": ["In", "Other"]}),
        json!({"type": "extrude", "name": "C", "operation": "cut", "target": "Other"}),
    ];
    retarget(&mut feats, &one);
    assert_eq!(feats[0]["target"], json!("Part [In]"));
    assert_eq!(feats[1]["target"], json!(["Part [In]", "Other"]));
    assert_eq!(feats[2]["target"], json!("Other"));
    // An insert that left two bodies makes the target a list, which the check refuses.
    let two = [("In".to_string(), vec!["P".to_string(), "Q".to_string()])];
    let mut feats = vec![json!({"type": "extrude", "name": "A", "operation": "cut", "target": "In"})];
    retarget(&mut feats, &two);
    assert_eq!(feats[0]["target"], json!(["P", "Q"]));
}

#[test]
fn a_shell_with_faces_and_a_thickness_is_readable() {
    let tree = json!({"features": [
        {"type": "extrude", "name": "E", "profiles": [], "distance": 5.0, "operation": "new"},
        {"type": "shell", "name": "Shell1", "faces": [{"point": [0, 0, 5]}], "thickness": 1.6, "side": "inside"},
    ]});
    assert!(check(&tree).is_ok());
}

#[test]
fn the_construction_flag_is_read_only_for_the_layouts_it_was_checked_in() {
    use super::sketch::construction_flag_offset as off;
    assert_eq!(off(true, 307), Some(7));
    assert_eq!(off(true, 303), Some(7));
    assert_eq!(off(true, 396), Some(2));
    assert_eq!(off(true, 447), Some(2));
    assert_eq!(off(true, 300), Some(1));
    assert_eq!(off(true, 276), Some(1));
    assert_eq!(off(false, 471), Some(2));
    // Layouts of the other samples: no flagged curve was seen, so none is read as construction.
    for len in [413, 417, 440, 444, 448, 496, 500] {
        assert_eq!(off(true, len), None);
    }
    assert_eq!(off(false, 431), None);
}

fn matrix_record(m: [f64; 16], lead: usize) -> Vec<u8> {
    let mut rec = vec![0x5a; lead];
    for v in m {
        rec.extend_from_slice(&v.to_le_bytes());
    }
    rec.extend_from_slice(&[0, 0, 1]);
    rec
}

#[test]
fn a_move_matrix_is_found_wherever_the_record_puts_it() {
    // Row major, translation in the last column (centimetres), at any byte offset.
    let m = [1.0, 0.0, 0.0, 0.25, 0.0, 1.0, 0.0, -0.5, 0.0, 0.0, 1.0, 1.8, 0.0, 0.0, 0.0, 1.0];
    for lead in [8usize, 41, 48, 93] {
        let (r, t) = super::bodyops::matrix(&matrix_record(m, lead)).unwrap();
        assert_eq!(r, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(t, [0.25, -0.5, 1.8]);
    }
    // A scaled or sheared block, or a last row that is not (0, 0, 0, 1), is not a rigid move.
    let mut scaled = m;
    scaled[0] = 2.0;
    assert!(super::bodyops::matrix(&matrix_record(scaled, 8)).is_none());
    let mut proj = m;
    proj[13] = 0.5;
    assert!(super::bodyops::matrix(&matrix_record(proj, 8)).is_none());
}

#[test]
fn body_edits_are_features_the_check_accepts_and_an_insert_moves() {
    let features = vec![
        json!({"type": "sketch", "name": "S", "plane": {"origin": [0, 0, 0], "x_axis": [1, 0, 0], "y_axis": [0, 1, 0]}}),
        json!({"type": "extrude", "name": "E", "profiles": [{"point": [1, 1]}], "distance": 5.0, "operation": "new"}),
        json!({"type": "scale_body", "name": "Scale", "target": "E", "factor": 0.5, "center": [1.0, 2.0, 3.0]}),
        json!({"type": "move_body", "name": "Move", "target": "E", "translate": [1.0, 0.0, 0.0], "copy": false}),
        json!({"type": "remove_body", "name": "Gone", "target": "E"}),
        json!({"type": "draft", "name": "Draft", "faces": [{"point": [1.0, 0.0, 0.0], "normal": [1.0, 0.0, 0.0]}],
               "plane": {"origin": [0.0, 0.0, 0.0], "normal": [0.0, 0.0, 1.0]}, "angle": 5.0, "flip": false}),
    ];
    assert!(check(&json!({ "features": features })).is_ok());
    // Placed by a quarter turn about z (x -> y) and a shift of (10, 0, 5): points turn and shift,
    // directions and translations only turn, the factor stays.
    let out = super::insert::bring(features, "Insert", &quarter_turn()).unwrap();
    let at = |n: &str| out.iter().find(|f| f["name"].as_str().is_some_and(|x| x.starts_with(n))).unwrap().clone();
    assert_eq!(at("Scale")["center"], json!([10.0 - 2.0, 1.0, 8.0]));
    assert_eq!(at("Scale")["factor"], json!(0.5));
    assert_eq!(at("Move")["translate"], json!([0.0, 1.0, 0.0]));
    assert_eq!(at("Draft")["faces"][0]["point"], json!([10.0, 1.0, 5.0]));
    assert_eq!(at("Draft")["faces"][0]["normal"], json!([0.0, 1.0, 0.0]));
    assert_eq!(at("Draft")["plane"]["normal"], json!([0.0, 0.0, 1.0]));
    assert_eq!(at("Move")["target"], at("Gone")["target"]);
}

#[test]
fn a_fillet_face_gives_the_edges_of_its_corner() {
    use super::naming::{FaceG, fillet_edge};
    // A pocket corner at the origin: sides x = 0 and y = 0 with normals into the void, rounded
    // with r = 1; a second corner at y = 10 (sides x = 0 and y = 10); the wall is the plane z = 5.
    let side = |p: [f64; 3], n: [f64; 3]| (p, n);
    let f1 = FaceG::Fillet { r: 1.0, a: side([0.0; 3], [1.0, 0.0, 0.0]), b: side([0.0; 3], [0.0, 1.0, 0.0]) };
    let f2 = FaceG::Fillet {
        r: 1.0,
        a: side([0.0, 10.0, 0.0], [1.0, 0.0, 0.0]),
        b: side([0.0, 10.0, 0.0], [0.0, -1.0, 0.0]),
    };
    let wall = FaceG::Plane { n: [0.0, 0.0, 1.0], p: [0.0, 0.0, 5.0], cap: false, mid: [0.0, 0.0, 5.0] };
    let sx = FaceG::Plane { n: [1.0, 0.0, 0.0], p: [0.0; 3], cap: false, mid: [0.0, 5.0, 2.0] };
    // The arc: the circle's centre is (1, 1, 5); its point nearest the corner is r along the bisector away.
    let (m, arc) = fillet_edge(&[Some(f1.clone()), Some(wall.clone())]).unwrap();
    let k = 1.0 - std::f64::consts::FRAC_1_SQRT_2;
    assert!(arc && (m[0] - k).abs() < 1e-12 && (m[1] - k).abs() < 1e-12 && (m[2] - 5.0).abs() < 1e-12, "{m:?}");
    // The straight edge between the two fillets: the middle of the tangent points (0, 1, 5), (0, 9, 5).
    let (m, arc) = fillet_edge(&[Some(wall), Some(sx), Some(f1), Some(f2)]).unwrap();
    assert!(!arc && m[0].abs() < 1e-12 && (m[1] - 5.0).abs() < 1e-12 && (m[2] - 5.0).abs() < 1e-12, "{m:?}");
}

/// A MetaStream with the given classes (GUID texts) and objects (id, bulk offset, class index).
fn meta_classes(classes: &[&str], objs: &[(u64, u64, usize)]) -> Vec<u8> {
    let mut m = s("Design");
    m.extend(le32(0));
    m.extend(ws(""));
    m.extend(le32(classes.len() as u32));
    for (k, g) in classes.iter().enumerate() {
        m.extend(s(g));
        m.extend(s(""));
        m.extend(le32(1));
        m.extend(s("Fusion"));
        let ids: Vec<u64> = objs.iter().filter(|o| o.2 == k).map(|o| o.0).collect();
        m.extend(le32(ids.len() as u32));
        for i in ids {
            m.extend(le64(i));
        }
    }
    m.extend(le32(0));
    m.extend(le32(objs.len() as u32));
    for (i, o, _) in objs {
        m.extend(le64(*i));
        m.extend(le64(*o));
    }
    m.extend(le32(0));
    m.extend(vec![0; 64]);
    m
}

/// A 2 x 2 rectangular pattern of the occurrences of one component (spacings 5 and -8 cm along
/// x and y): the original occurrence, then copies at the given translations (cm).
fn component_pattern(copies: &[[f64; 3]]) -> (Vec<u8>, Vec<u8>) {
    const CLASSES: [&str; 10] = [
        "E8816367-0000-0000-0000-000000000000",
        "2D6E13A1-0000-0000-0000-000000000000",
        "CE2913AA-0000-0000-0000-000000000000",
        "E03784ED-0000-0000-0000-000000000000",
        "CD57BC48-0000-0000-0000-000000000000",
        "A085449A-0000-0000-0000-000000000000",
        "D91D429C-0000-0000-0000-000000000000",
        "7A6A3D31-0000-0000-0000-000000000000",
        "F2A7590D-0000-0000-0000-000000000000",
        "11111111-0000-0000-0000-000000000000",
    ];
    let refs = |ids: &[u64]| ids.iter().flat_map(|i| r#ref(*i)).collect::<Vec<u8>>();
    let mut recs: Vec<(u64, usize, Vec<u8>)> = Vec::new();
    let occs: Vec<u64> = (0..=copies.len() as u64).map(|k| 11 + k).collect();
    let mut pattern = occs.clone();
    pattern.extend([20, 21, 22, 23, 24]);
    recs.push((10, 0, refs(&pattern)));
    for (k, o) in occs.iter().enumerate() {
        recs.push((*o, 1, refs(&[31 + k as u64])));
        let mut ce = refs(&[40]);
        if let Some(t) = k.checked_sub(1).and_then(|c| copies.get(c)) {
            ce.extend([0; 3]);
            for v in [1.0, 0.0, 0.0, t[0], 0.0, 1.0, 0.0, t[1], 0.0, 0.0, 1.0, t[2], 0.0, 0.0, 0.0, 1.0] {
                ce.extend(f64::to_le_bytes(v));
            }
        }
        recs.push((31 + k as u64, 2, ce));
    }
    recs.push((40, 3, refs(&[41])));
    recs.push((41, 4, refs(&[42])));
    recs.push((42, 9, ws("BREP.abc.smb")));
    for (holder, class, param, expr, name, v) in [
        (20, 5, 25, "2", "countU", 2.0),
        (21, 5, 26, "2", "CountV", 2.0),
        (22, 6, 27, "50 mm", "uSpaceDistance", 5.0),
        (23, 6, 28, "-80 mm", "vSpaceDistance", -8.0),
    ] {
        recs.push((holder, class, refs(&[param])));
        let mut p = ws(expr);
        p.extend(ws(name));
        p.extend(ws("d1"));
        p.extend(f64::to_le_bytes(v));
        recs.push((param, 7, p));
    }
    let mut dir = vec![0; 4];
    for v in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
        dir.extend(f64::to_le_bytes(v));
    }
    recs.push((24, 8, dir));
    let mut bulk = Vec::new();
    let mut objs = Vec::new();
    for (id, class, body) in recs {
        objs.push((id, bulk.len() as u64, class));
        bulk.extend(record(256 + class as u32, id, &body));
    }
    (meta_classes(&CLASSES, &objs), bulk)
}

#[test]
fn a_pattern_of_components_gives_its_stored_copies_only_on_its_lattice() {
    let lattice = [[5.0, 0.0, 0.0], [0.0, -8.0, 0.0], [5.0, -8.0, 0.0]];
    let (m, bulk) = component_pattern(&lattice);
    let got = super::pattern_copies(&m, &bulk);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].blobs, vec!["BREP.abc.smb".to_string()]);
    let ts: Vec<[f64; 3]> = got[0].copies.iter().map(|c| c.t).collect();
    assert_eq!(ts, vec![[50.0, 0.0, 0.0], [0.0, -80.0, 0.0], [50.0, -80.0, 0.0]]);
    // Each copy is placed by the original occurrence's placement, then its own.
    let mut placed =
        vec![crate::fusion::Placed { brep: "abc".into(), occurrence: "c:1".into(), placement: quarter_turn() }];
    super::add_pattern_copies(&mut placed, &got);
    assert_eq!(placed.len(), 4);
    assert_eq!(placed[1].occurrence, "c:1 [2]");
    assert_eq!(placed[1].placement, quarter_turn().then(&got[0].copies[0]));
    // A copy that is not where the parameters put it, or one too few: no copies at all.
    let (m, bulk) = component_pattern(&[[5.0, 0.0, 0.0], [0.0, -8.0, 0.0], [5.0, -8.5, 0.0]]);
    assert!(super::pattern_copies(&m, &bulk).is_empty());
    let (m, bulk) = component_pattern(&lattice[..2]);
    assert!(super::pattern_copies(&m, &bulk).is_empty());
}

#[test]
fn a_loop_that_lists_an_edge_twice_closes_on_one_of_them() {
    // A square whose bottom edge is also a piece of a longer line drawn on it.
    let polys: Vec<Vec<P2>> = vec![
        vec![[0.0, 0.0], [1.0, 0.0]],
        vec![[1.0, 0.0], [1.0, 1.0]],
        vec![[1.0, 0.0], [0.0, 0.0]],
        vec![[1.0, 1.0], [0.0, 1.0]],
        vec![[0.0, 1.0], [0.0, 0.0]],
    ];
    assert!(geom::chain(&polys).is_none());
    let fewer = geom::drop_covered(&polys).unwrap();
    assert_eq!(fewer.len(), 4);
    assert_eq!(fewer.first(), polys.first(), "of two equal polylines the first stays");
    assert_eq!(geom::chain(&fewer).unwrap().len(), 4);
    // A shorter line lying on an edge goes, the edge stays; nothing to drop gives `None`.
    let mut part = fewer.clone();
    part.push(vec![[0.2, 0.0], [0.6, 0.0]]);
    assert_eq!(geom::drop_covered(&part).unwrap(), fewer);
    assert!(geom::drop_covered(&fewer).is_none());
}

fn unit_circle_at(c: P2, r: f64) -> Vec<P2> {
    (0..=64)
        .map(|k| {
            let t = f64::from(k) / 64.0 * std::f64::consts::TAU;
            [c[0] + r * libm::cos(t), c[1] + r * libm::sin(t)]
        })
        .collect()
}

#[test]
fn a_hole_joined_to_the_outline_is_walked_in_the_outline_loop() {
    // A 4 x 4 square, a circle of radius 1 in its middle and the line from the bottom edge up
    // to the circle, walked there and back.
    let mut polys: Vec<Vec<P2>> = vec![
        vec![[0.0, 0.0], [2.0, 0.0]],
        vec![[2.0, 0.0], [2.0, 1.0]],
        unit_circle_at([2.0, 2.0], 1.0),
        vec![[2.0, 0.0], [4.0, 0.0]],
        vec![[4.0, 0.0], [4.0, 4.0]],
        vec![[4.0, 4.0], [0.0, 4.0]],
        vec![[0.0, 4.0], [0.0, 0.0]],
    ];
    assert!(geom::chain(&polys).is_none());
    let (outer, holes) = geom::bridged(&polys).unwrap();
    assert_eq!(outer.len(), 5);
    assert_eq!(holes.len(), 1);
    assert!(geom::inside([0.5, 0.5], &outer) && geom::inside([2.0, 2.0], holes.first().unwrap()));
    // A circle that touches the bottom edge needs no line; one outside the outline is refused,
    // and so is one that neither touches nor hangs on a line.
    polys.remove(1);
    if let Some(c) = polys.get_mut(1) {
        *c = unit_circle_at([2.0, 1.0], 1.0);
    }
    assert_eq!(geom::bridged(&polys).unwrap().1.len(), 1);
    if let Some(c) = polys.get_mut(1) {
        *c = unit_circle_at([2.0, -1.0], 1.0);
    }
    assert!(geom::bridged(&polys).is_none());
    if let Some(c) = polys.get_mut(1) {
        *c = unit_circle_at([2.0, 2.0], 1.0);
    }
    assert!(geom::bridged(&polys).is_none());
}

#[test]
fn a_line_is_cut_by_the_ends_of_a_line_lying_on_it() {
    let long: Vec<P2> = vec![[0.0, 0.0], [6.0, 0.0]];
    let on: Vec<P2> = vec![[4.0, 0.0], [2.0, 0.0]];
    let others: Vec<&[P2]> = vec![&on];
    assert!(geom::piece(&long, false, &others, 2, 3).is_none(), "parallel lines do not cross");
    let mid = geom::piece_ex(&long, false, &others, 2, 3, true).unwrap();
    assert_eq!(mid, vec![[2.0, 0.0], [4.0, 0.0]]);
    // A parallel line beside it cuts nothing.
    let beside: Vec<P2> = vec![[4.0, 0.1], [2.0, 0.1]];
    let others: Vec<&[P2]> = vec![&beside];
    assert!(geom::piece_ex(&long, false, &others, 2, 3, true).is_none());
}

#[test]
fn a_chord_and_the_arc_it_cuts_off_close_within_the_sag_of_the_sampled_arc() {
    // The cut of a sampled circle lies on a chord of the polyline, a little off the true circle.
    let circle = unit_circle_at([0.0, 0.0], 1.0);
    let y = 0.72;
    let x = libm::sqrt(1.0 - y * y);
    let chord: Vec<P2> = vec![[-x, y], [x, y]];
    let others: Vec<&[P2]> = vec![&chord];
    let cap = geom::piece(&circle, true, &others, 2, 2).unwrap();
    assert!(cap.iter().all(|p| p[1] >= y - 1e-3));
    assert!(geom::chain(&[chord, cap]).is_some());
}

enum Tok<'a> {
    P(i32),
    I(i32),
    D(f64),
    Pos([f64; 3]),
    Vec([f64; 3]),
    S(&'a str),
}

fn sab_tokens(names: &[&str], toks: &[Tok]) -> Vec<u8> {
    let mut v = Vec::new();
    for (i, n) in names.iter().enumerate() {
        v.extend(token_str(if i + 1 == names.len() { 0x0d } else { 0x0e }, n));
    }
    for t in toks {
        match t {
            Tok::P(p) => {
                v.push(0x0c);
                v.extend(p.to_le_bytes());
            }
            Tok::I(i) => {
                v.push(0x04);
                v.extend(i.to_le_bytes());
            }
            Tok::D(d) => {
                v.push(0x06);
                v.extend(d.to_le_bytes());
            }
            Tok::Pos(p) | Tok::Vec(p) => {
                v.push(if matches!(t, Tok::Pos(_)) { 0x13 } else { 0x14 });
                for x in p {
                    v.extend(x.to_le_bytes());
                }
            }
            Tok::S(s) => v.extend(token_str(0x07, s)),
        }
    }
    v.push(0x11);
    v
}

/// A blob with one straight edge from (1, 0, 0) to (3, 0, 0) between two named faces; `t1` is
/// the edge's stored end parameter (2 is right).
fn named_edge_blob(t1: f64) -> Vec<u8> {
    use Tok::{D, I, P, Pos, S, Vec as V};
    let tag = |face: i32, entries: &[(&'static str, &[i32])]| {
        let mut t = vec![P(-1), I(-1), P(-1), P(-1), P(face), S("generic_tag_attrib_def"), I(3), I(3), I(-1)];
        // As the files have it: the entry count, then per entry the length of its text first
        // (858's `14`: `1, 2, "14", 0, 1, 321, 0`; a face of two entries: `2, 1, "1", 0, 1, 301,
        // 0, 1, "1", 0, 1, 304, 0`).
        t.extend([S("generic_tag_attrib_def "), I(entries.len() as i32)]);
        for (g, ops) in entries {
            t.extend([I(g.len() as i32), S(g), I(0), I(ops.len() as i32)]);
            t.extend(ops.iter().map(|o| I(*o)));
            t.push(I(0));
        }
        sab_tokens(&["ATTRIB_CUSTOM", "attrib"], &t)
    };
    let mut b = b"ASM BinaryFile4tY".to_vec();
    b.extend(sab_tokens(&["asmheader"], &[]));
    b.extend(sab_tokens(&["face"], &[P(3)]));
    b.extend(sab_tokens(&["face"], &[P(4)]));
    b.extend(sab_tokens(&["loop"], &[P(5), P(1)]));
    b.extend(sab_tokens(&["loop"], &[P(6), P(2)]));
    b.extend(sab_tokens(&["coedge"], &[P(7), P(3)]));
    b.extend(sab_tokens(&["coedge"], &[P(7), P(4)]));
    b.extend(sab_tokens(&["edge"], &[P(8), D(0.0), P(9), D(t1), P(5), P(12)]));
    b.extend(sab_tokens(&["vertex"], &[P(10)]));
    b.extend(sab_tokens(&["vertex"], &[P(11)]));
    b.extend(sab_tokens(&["point"], &[Pos([1.0, 0.0, 0.0])]));
    b.extend(sab_tokens(&["point"], &[Pos([3.0, 0.0, 0.0])]));
    b.extend(sab_tokens(&["straight", "curve"], &[Pos([1.0, 0.0, 0.0]), V([1.0, 0.0, 0.0])]));
    b.extend(tag(1, &[("15", &[313])]));
    b.extend(tag(2, &[("3", &[313, 320]), ("4", &[313])]));
    b
}

#[test]
fn a_stored_edge_is_known_by_the_names_of_its_two_faces() {
    let edges = acis::named_edges(&named_edge_blob(2.0)).unwrap();
    let [e] = edges.as_slice() else { panic!("one edge, got {}", edges.len()) };
    // A generator of two digits and a name of two entries.
    assert_eq!(e.faces[0], vec![("15".to_string(), vec![313])]);
    assert_eq!(e.faces[1], vec![("3".to_string(), vec![313, 320]), ("4".to_string(), vec![313])]);
    assert_eq!((e.a, e.b, e.mid), ([1.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
    assert!(e.around.is_empty());
    // Parameters that do not give the edge's end vertex: the middle is unknown, the edge is left out.
    assert!(acis::named_edges(&named_edge_blob(5.0)).unwrap().is_empty());
}

#[test]
fn a_typed_reference_carries_its_targets_class_as_the_class_table_spells_it() {
    // The blob-name holder's class is written with its dashes out of place.
    const ODD: &str = "40DCC3F41D-E94F-B3B1-3E95-9B6ABE572C";
    let typed = |id: u64, class: &str| {
        let mut v = vec![1];
        v.extend(le64(id));
        v.extend(s(class));
        v
    };
    let r5 = record(257, 5, &[]);
    let mut body6 = typed(5, ODD);
    body6.extend(typed(6, G1));
    let r6 = record(256, 6, &body6);
    let mut bulk = r5.clone();
    bulk.extend(&r6);
    let mut m = meta(&[(5, 0), (6, r5.len() as u64)], &|i| usize::from(i == 5));
    // The second class of the table, spelled the odd way (same length).
    let at = m.windows(36).position(|w| w == G2.as_bytes()).unwrap();
    m.splice(at..at + 36, ODD.bytes());
    let d = Design::new(&m, &bulk).unwrap();
    assert_eq!(d.cls(5), ODD);
    assert_eq!(super::stream::ref_ids(&d, d.rec(6)), vec![5, 6]);
    // The same text in front of an object of another class is not a reference to it.
    let mut wrong = typed(6, ODD);
    wrong.extend([0; 12]);
    let r7 = record(256, 6, &wrong);
    let mut bulk = r5.clone();
    bulk.extend(&r7);
    let d = Design::new(&m, &bulk).unwrap();
    assert!(super::stream::ref_ids(&d, d.rec(6)).is_empty());
}

#[test]
fn an_extrude_that_names_its_target_by_a_typed_reference_is_still_recognised() {
    // Flag, a typed reference to the target's selection (49 bytes), two zero bytes, then the
    // enum block (cut, one side, distance) and the direction.
    let r5 = record(256, 5, &[]);
    let mut body = vec![1u8, 1];
    body.extend(le64(5));
    body.extend(s(G1));
    body.extend([0, 0]);
    for v in [2u32, 1, 2] {
        body.extend(le32(v));
    }
    body.extend([0, 1]);
    for v in [0.0f64, 0.0, 1.0] {
        body.extend(v.to_le_bytes());
    }
    body.extend([0; 64]);
    let r6 = record(257, 6, &body);
    let mut bulk = r5.clone();
    bulk.extend(&r6);
    let m = meta(&[(5, 0), (6, r5.len() as u64)], &|i| usize::from(i == 6));
    let d = Design::new(&m, &bulk).unwrap();
    let e = feature::extrude_raw(&d, 6).unwrap();
    assert_eq!((e.op, e.extent, e.vec), (2, 1, Some([0.0, 0.0, 1.0])));
}

/// A Combine of the body of feature 101 (the tool) into the body of feature 100 (the target),
/// with the given operation value.
fn combine_stream(op: u32) -> (Vec<u8>, Vec<u8>) {
    const CLASSES: [&str; 5] = [
        "2A94257F-0000-0000-0000-000000000000",
        "2CA5A1CD-0000-0000-0000-000000000000",
        "9716F783-0000-0000-0000-000000000000",
        "D26351F0-0000-0000-0000-000000000000",
        "DD405BC2-0000-0000-0000-000000000000",
    ];
    let mut body = le32(op);
    body.extend(le32(0));
    body.extend([0, 0, 1, 0, 0, 0, 0]);
    body.extend(le64(0x1e9));
    for list in [401u64, 201, 400] {
        body.extend(le32(1));
        body.extend(r#ref(list));
        body.extend([0; 4]);
    }
    let recs: Vec<(u64, usize, Vec<u8>)> = vec![
        (100, 4, Vec::new()),
        (101, 4, Vec::new()),
        (200, 3, r#ref(100)),
        (201, 3, r#ref(101)),
        (300, 2, r#ref(200)),
        (301, 2, r#ref(201)),
        (400, 1, r#ref(300)),
        (401, 1, r#ref(301)),
        (500, 0, body),
    ];
    let mut bulk = Vec::new();
    let mut objs = Vec::new();
    for (id, class, body) in recs {
        objs.push((id, bulk.len() as u64, class));
        bulk.extend(record(256 + class as u32, id, &body));
    }
    (meta_classes(&CLASSES, &objs), bulk)
}

#[test]
fn a_combine_joins_its_tool_bodies_to_its_target_and_an_unconfirmed_operation_is_refused() {
    use super::combine::Combined;
    let feats = vec![
        json!({"type": "extrude", "name": "Extrude1", "source_id": 100, "creates_bodies": 1}),
        json!({"type": "mirror", "name": "Mirror1", "source_id": 101, "creates_bodies": 1}),
    ];
    let names = std::collections::HashMap::new();
    let (m, bulk) = combine_stream(0);
    let d = Design::new(&m, &bulk).unwrap();
    let g = super::Graph::new(&d);
    let ed = super::bodyops::Edit { d: &d, g: &g, names: &names, feats: &feats };
    let Combined::Feature(f) = ed.combine(500, "Combine1") else { panic!("not read") };
    assert_eq!(f["type"], "combine");
    assert_eq!(f["target"], "Extrude1");
    assert_eq!(f["tools"], json!(["Mirror1"]));
    assert_eq!(f["operation"], "join");
    // A tool whose maker makes several bodies is not one body the tree can name.
    let several = vec![feats[0].clone(), json!({"type": "mirror", "name": "Mirror1", "source_id": 101})];
    let ed = super::bodyops::Edit { d: &d, g: &g, names: &names, feats: &several };
    assert!(matches!(ed.combine(500, "Combine1"), Combined::Refused(_)));
    let (m, bulk) = combine_stream(1);
    let d = Design::new(&m, &bulk).unwrap();
    let g = super::Graph::new(&d);
    let ed = super::bodyops::Edit { d: &d, g: &g, names: &names, feats: &feats };
    assert!(matches!(ed.combine(500, "Combine1"), Combined::Refused(_)));
}

/// A record with pointers, a position, vectors and doubles (a surface or a transform).
fn sab_geometry(names: &[&str], ptrs: &[i32], pos: Option<[f64; 3]>, vecs: &[[f64; 3]], nums: &[f64]) -> Vec<u8> {
    let mut v = sab_record(names, ptrs, pos);
    v.pop();
    for p in vecs {
        v.push(0x14);
        for x in p {
            v.extend(x.to_le_bytes());
        }
    }
    for x in nums {
        v.push(0x06);
        v.extend(x.to_le_bytes());
    }
    v.push(0x11);
    v
}

#[test]
fn a_blob_body_is_described_by_its_lumps_box_and_cylinders() {
    // 1 body -> transform 2 (identity), lumps 3 and 4 -> faces 5, 6 -> cylinders 7, 8;
    // vertices 9, 10 under the first lump; 13 a second body: a flat sheet with a real transform.
    let mut b = b"ASM BinaryFile4tY".to_vec();
    b.extend(sab_record(&["asmheader"], &[], None));
    b.extend(sab_record(&["body"], &[3, 4, 2], None));
    let rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0]];
    b.extend(sab_geometry(&["transform"], &[], None, &rows, &[1.0]));
    b.extend(sab_record(&["lump"], &[5, 9, 10], None));
    b.extend(sab_record(&["lump"], &[6], None));
    b.extend(sab_record(&["face"], &[7], None));
    b.extend(sab_record(&["face"], &[8], None));
    let up = [[0.0, 0.0, 2.0], [0.25, 0.0, 0.0]];
    b.extend(sab_geometry(&["cone", "surface"], &[], Some([1.0, 2.0, -20.0]), &up, &[1.0, 0.0, 1.0, 0.25]));
    b.extend(sab_geometry(&["cone", "surface"], &[], Some([3.0, 2.0, -20.0]), &up, &[1.0, 0.0, 1.0, 0.25]));
    b.extend(sab_record(&["vertex"], &[11], None));
    b.extend(sab_record(&["vertex"], &[12], None));
    b.extend(sab_record(&["point"], &[], Some([1.25, 2.0, -20.0])));
    b.extend(sab_record(&["point"], &[], Some([1.25, 2.0, 20.0])));
    b.extend(sab_record(&["body"], &[14, 15, 18], None));
    b.extend(sab_record(&["lump"], &[16, 17], None));
    b.extend(sab_geometry(&["spline", "surface"], &[], None, &[], &[]));
    b.extend(sab_record(&["vertex"], &[19], None));
    b.extend(sab_record(&["vertex"], &[20], None));
    let moved = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [5.0, 0.0, 0.0]];
    b.extend(sab_geometry(&["transform"], &[], None, &moved, &[1.0]));
    b.extend(sab_record(&["point"], &[], Some([0.0, 0.0, 0.0])));
    b.extend(sab_record(&["point"], &[], Some([4.0, 3.0, 0.0])));
    let shapes = acis::body_shapes(&b).unwrap();
    assert_eq!(shapes.len(), 2);
    let tool = &shapes[0];
    // Its only vertices are the ends of one seam: flat by its vertices, a tool all the same.
    assert_eq!((tool.lumps, tool.transformed, tool.other_surfaces, tool.flat()), (2, false, false, true));
    assert_eq!(tool.vertex_box, Some(([1.25, 2.0, -20.0], [1.25, 2.0, 20.0])));
    assert_eq!(
        tool.cylinders,
        vec![([1.0, 2.0, -20.0], [0.0, 0.0, 1.0], 0.25), ([3.0, 2.0, -20.0], [0.0, 0.0, 1.0], 0.25)]
    );
    let sheet = &shapes[1];
    assert_eq!((sheet.lumps, sheet.transformed, sheet.other_surfaces, sheet.flat()), (1, true, true, true));
    // The whole cylinders along z, one per lump, are a drill; the sheet is none.
    let drills = pcb::drills_of(&shapes);
    assert_eq!(drills, vec![pcb::Drill { holes: vec![(1.0, 2.0, 0.25), (3.0, 2.0, 0.25)], z: (-20.0, 20.0) }]);
}

fn shape(lumps: usize, cylinders: Vec<(geom::V3, geom::V3, f64)>, z: (f64, f64)) -> acis::BodyShape {
    acis::BodyShape {
        lumps,
        vertex_box: Some(([0.0, 0.0, z.0], [1.0, 1.0, z.1])),
        cylinders,
        other_surfaces: false,
        transformed: false,
    }
}

#[test]
fn only_bodies_of_whole_upright_cylinders_are_drill_tools() {
    let up = [0.0, 0.0, 1.0];
    let hole = |x: f64, r: f64| ([x, 0.5, -20.0], up, r);
    // A plate with two holes: one lump, two cylinders.
    assert!(pcb::drills_of(&[shape(1, vec![hole(0.2, 0.1), hole(0.6, 0.1)], (0.0, 0.1))]).is_empty());
    // Plated barrels: an inner and an outer cylinder per lump.
    assert!(
        pcb::drills_of(&[shape(2, vec![hole(0.2, 0.1), hole(0.2, 0.12), hole(0.6, 0.1), hole(0.6, 0.12)], (0.0, 0.1))])
            .is_empty()
    );
    // A cylinder that lies down, a body with another curved surface, a moved body.
    assert!(pcb::drills_of(&[shape(1, vec![([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0.1)], (0.0, 0.2))]).is_empty());
    let mut curved = shape(1, vec![hole(0.2, 0.1)], (-20.0, 20.0));
    curved.other_surfaces = true;
    let mut moved = shape(1, vec![hole(0.2, 0.1)], (-20.0, 20.0));
    moved.transformed = true;
    assert!(pcb::drills_of(&[curved, moved]).is_empty());
    // Two tools of one blob are both read (a face of one cylinder stored twice counts once).
    let long = shape(2, vec![hole(0.2, 0.1), hole(0.6, 0.05), hole(0.6, 0.05)], (-20.0, 20.0));
    let short = shape(1, vec![hole(0.4, 0.07)], (-0.1, 0.2));
    let drills = pcb::drills_of(&[long, short]);
    assert_eq!(drills.len(), 2);
    assert_eq!(drills[0].holes, vec![(0.2, 0.5, 0.1), (0.6, 0.5, 0.05)]);
    assert_eq!(drills[1].z, (-0.1, 0.2));
}

#[test]
fn a_drill_is_a_sketch_of_circles_and_a_cut_as_long_as_its_tool() {
    let drill = pcb::Drill { holes: vec![(0.254, 0.254, 0.125), (0.635, 0.254, 0.05)], z: (-19.9, 20.1) };
    let feats = pcb::drill_json(&drill, "PCBExtrude1", "PCBExtrude1 holes", "Extrude1", 346);
    let [sketch, cut] = feats.as_slice() else { panic!("two features") };
    assert_eq!(sketch["plane"]["origin"], json!([0.0, 0.0, 1.0]));
    assert_eq!(sketch["curves"][1], json!({"id": "c1", "type": "circle", "center": "p1", "radius": 0.5}));
    assert_eq!(sketch["points"][1], json!({"id": "p1", "x": 6.35, "y": 2.54}));
    assert_eq!((cut["operation"].as_str(), cut["target"].as_str()), (Some("cut"), Some("Extrude1")));
    assert_eq!((cut["distance"].as_f64(), cut["symmetric"].as_bool()), (Some(400.0), Some(true)));
    assert_eq!(cut["profiles"], json!([{"point": [2.54, 2.54]}, {"point": [6.35, 2.54]}]));
    assert!(check(&json!({"features": feats})).is_ok());
}

#[test]
fn the_side_of_a_box_is_the_side_of_its_stored_body() {
    // Corner (-0.2, -0.2, 0), axes x, y, z; the stored body lies above or below the corner's plane.
    let m = [1.0, 0.0, 0.0, -0.2, 0.0, 1.0, 0.0, -0.2, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let size = [0.4, 0.4, 0.01];
    assert_eq!(pcb::box_height(&m, size, ([-0.2, -0.2, 0.0], [0.2, 0.2, 0.01])), Some(0.01));
    assert_eq!(pcb::box_height(&m, size, ([-0.2, -0.2, -0.01], [0.2, 0.2, 0.0])), Some(-0.01));
    // Another size, or a body elsewhere (an offset the parameters do not explain): not this box.
    assert_eq!(pcb::box_height(&m, size, ([-0.2, -0.2, 0.0], [0.2, 0.2, 0.02])), None);
    assert_eq!(pcb::box_height(&m, size, ([-0.1, -0.2, 0.0], [0.3, 0.2, 0.01])), None);
    // A turned frame: x along world y, y along world -x.
    let turned = [0.0, -1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(pcb::box_height(&turned, [0.3, 0.2, 0.1], ([0.8, 2.0, 0.0], [1.0, 2.3, 0.1])), Some(0.1));
}

#[test]
fn a_drill_table_is_a_count_and_entries_of_forty_bytes() {
    let mut rec = vec![0xaa; 3];
    rec.extend(le32(2));
    for (x, y) in [(0.254f64, 0.254f64), (2.286, 1.524)] {
        rec.extend(le32(0));
        rec.extend(x.to_le_bytes());
        rec.extend(y.to_le_bytes());
        rec.extend([0u8; 20]);
    }
    assert_eq!(pcb::hole_table(&rec, 3), Some(vec![(0.254, 0.254), (2.286, 1.524)]));
    // No holes; a cut-off table; an entry with something in its zero bytes.
    assert_eq!(pcb::hole_table(&le32(0), 0), Some(Vec::new()));
    assert_eq!(pcb::hole_table(&rec[..rec.len() - 1], 3), None);
    let mut other = rec.clone();
    other[3 + 4 + 30] = 1;
    assert_eq!(pcb::hole_table(&other, 3), None);
}

/// The four edges of an axis-parallel rectangle at height `z`.
fn rect_edges(x: (f64, f64), y: (f64, f64), z: f64) -> Vec<(geom::V3, geom::V3)> {
    let c = [[x.0, y.0, z], [x.1, y.0, z], [x.1, y.1, z], [x.0, y.1, z]];
    (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect()
}

#[test]
fn the_second_pattern_direction_is_the_side_where_the_stored_copies_are() {
    // A 1 x 1 square hole patterned 3 x 2: U = +x by 2.54, V perpendicular by -2.54.
    let segs = rect_edges((0.0, 1.0), (0.0, 1.0), 0.0);
    let lat =
        |count_v: usize| patdir::Lattice { u: [1.0, 0.0, 0.0], spacing_u: 2.54, count_u: 3, spacing_v: -2.54, count_v };
    let n = [0.0, 0.0, 1.0];
    // The stored body has the second row at y = -2.54 (and its edges at another height).
    let copies = |dy: f64| -> Vec<(geom::V3, geom::V3)> {
        (0..3)
            .flat_map(|i| {
                let x = 2.54 * f64::from(i);
                [rect_edges((x, x + 1.0), (0.0, 1.0), 0.3), rect_edges((x, x + 1.0), (dy, dy + 1.0), 0.3)].concat()
            })
            .collect()
    };
    // n x U = +y: with the negative spacing the copies go towards -y.
    assert_eq!(patdir::side_of(&segs, n, &lat(2), &copies(-2.54)), Some([0.0, 1.0, 0.0]));
    assert_eq!(patdir::side_of(&segs, n, &lat(2), &copies(2.54)), Some([0.0, -1.0, 0.0]));
    // Copies on both sides, on neither, or one copy missing: not decided.
    let both: Vec<_> = [copies(-2.54), copies(2.54)].concat();
    assert_eq!(patdir::side_of(&segs, n, &lat(2), &both), None);
    assert_eq!(patdir::side_of(&segs, n, &lat(2), &copies(-5.0)), None);
    let mut missing = copies(-2.54);
    missing.truncate(missing.len() - 4);
    assert_eq!(patdir::side_of(&segs, n, &lat(2), &missing), None);
    // A third row the bodies lack, a single row, a direction out of the sketch plane, no edges.
    assert_eq!(patdir::side_of(&segs, n, &lat(3), &copies(-2.54)), None);
    assert_eq!(patdir::side_of(&segs, n, &lat(1), &copies(-2.54)), None);
    assert_eq!(patdir::side_of(&segs, [1.0, 0.0, 0.0], &lat(2), &copies(-2.54)), None);
    assert_eq!(patdir::side_of(&[], n, &lat(2), &copies(-2.54)), None);
}

#[test]
fn the_reach_of_an_extrude_to_a_body_is_read_off_its_stored_tool() {
    let z = [0.0, 0.0, 1.0];
    let outline = rect_edges((0.0, 5.1), (0.0, 2.0), 0.0);
    let tool =
        |depth: f64| [rect_edges((0.0, 5.1), (0.0, 2.0), 0.0), rect_edges((0.0, 5.1), (0.0, 2.0), depth)].concat();
    // Another body, with other edges, says nothing.
    let other = rect_edges((0.0, 9.0), (0.0, 9.0), 0.0);
    assert_eq!(json::reach_of(&outline, z, &[other.clone(), tool(-0.4)]), Some(-0.4));
    assert_eq!(json::reach_of(&outline, z, &[tool(0.4), tool(0.4)]), Some(0.4));
    // Two tools that disagree; none; a body with the outline at three heights (not one tool).
    assert_eq!(json::reach_of(&outline, z, &[tool(0.4), tool(-0.4)]), None);
    assert_eq!(json::reach_of(&outline, z, &[other]), None);
    let stacked = [tool(0.4), rect_edges((0.0, 5.1), (0.0, 2.0), 0.9)].concat();
    assert_eq!(json::reach_of(&outline, z, &[stacked]), None);
    // A tool that lacks one edge of the outline at its far side.
    let mut open = tool(0.4);
    open.pop();
    assert_eq!(json::reach_of(&outline, z, &[open]), None);
    // An outline of one direction would fit any slab: not decided.
    let parallel = [outline[0], outline[2]];
    assert_eq!(json::reach_of(&parallel, z, &[tool(0.4)]), None);
}

#[test]
fn a_feature_belongs_to_the_component_whose_first_timeline_is_its_own() {
    // The root (1) names its own timeline (3) and then the one of the component being edited (4);
    // that component (2) names only its own. Features 7 and 8 sit in the timelines' records.
    const CLASSES: [&str; 4] = [
        "E03784ED-0000-0000-0000-000000000000",
        "C3423E04-0000-0000-0000-000000000000",
        "3A6D1E62-0000-0000-0000-000000000000",
        "D7015B81-0000-0000-0000-000000000000",
    ];
    let refs = |ids: &[u64]| ids.iter().flat_map(|i| r#ref(*i)).collect::<Vec<u8>>();
    let recs: Vec<(u64, usize, Vec<u8>)> = vec![
        (1, 0, refs(&[3, 4])),
        (2, 0, refs(&[4])),
        (3, 1, refs(&[5])),
        (4, 1, refs(&[6])),
        (5, 2, Vec::new()),
        (6, 2, Vec::new()),
        (7, 3, refs(&[5])),
        (8, 3, refs(&[6])),
    ];
    let mut bulk = Vec::new();
    let mut objs = Vec::new();
    for (id, class, body) in recs {
        objs.push((id, bulk.len() as u64, class));
        bulk.extend(record(256 + class as u32, id, &body));
    }
    let meta = meta_classes(&CLASSES, &objs);
    let d = Design::new(&meta, &bulk).unwrap();
    let g = super::Graph::new(&d);
    assert_eq!(g.component(&d, 7), Some(1));
    assert_eq!(g.component(&d, 8), Some(2));
}

#[test]
fn a_stored_spline_is_evaluated_as_the_b_spline_it_is() {
    // A cubic Bezier: its midpoint is (p0 + 3 p1 + 3 p2 + p3) / 8.
    let cubic = sketch::Spline {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        weights: Vec::new(),
        poles: vec![[0.0, 0.0, 0.0], [0.0, 2.0, 0.0], [4.0, 2.0, 0.0], [4.0, 0.0, 0.0]],
        start_ref: 1,
        end_ref: 2,
    };
    let m = cubic.at(0.5).unwrap();
    assert!((m[0] - 2.0).abs() < 1e-12 && (m[1] - 1.5).abs() < 1e-12);
    let pl = cubic.polyline().unwrap();
    assert_eq!((pl.first(), pl.last()), (Some(&[0.0, 0.0]), Some(&[4.0, 0.0])));
    // A rational quadratic quarter circle stays on the circle; two spans of a clamped quadratic
    // meet at the inner knot.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let arc = sketch::Spline {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        weights: vec![1.0, w, 1.0],
        poles: vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        start_ref: 1,
        end_ref: 2,
    };
    for p in arc.polyline().unwrap() {
        assert!((libm::hypot(p[0], p[1]) - 1.0).abs() < 1e-12);
    }
    let two = sketch::Spline {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        weights: Vec::new(),
        poles: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 2.0, 0.0], [3.0, 2.0, 0.0]],
        start_ref: 1,
        end_ref: 2,
    };
    let j = two.at(0.5).unwrap();
    assert!((j[0] - 1.5).abs() < 1e-12 && (j[1] - 1.0).abs() < 1e-12);
    assert_eq!(two.polyline().unwrap().len(), 49);
}

#[test]
fn a_spline_array_is_count_capacity_size_and_doubles() {
    let mut rec = [le32(2), le32(4), le32(8)].concat();
    rec.extend(1.5f64.to_le_bytes());
    rec.extend(2.5f64.to_le_bytes());
    assert_eq!(sketch::spline_array(&rec, 0, 1), Some((vec![1.5, 2.5], 28)));
    // A capacity below the count, another element size or a cut-off array is not one.
    let mut short = rec.clone();
    short.truncate(27);
    assert_eq!(sketch::spline_array(&short, 0, 1), None);
    let mut size = rec.clone();
    size[8] = 4;
    assert_eq!(sketch::spline_array(&size, 0, 1), None);
    let mut cap = rec;
    cap[4] = 1;
    assert_eq!(sketch::spline_array(&cap, 0, 1), None);
}

#[test]
fn a_records_own_id_is_not_a_reference() {
    // Record 0x0501 (1281): its id's bytes `01 05 00 ...` read like a reference to object 5, in
    // the header and behind the second tag a body link ends with.
    let r5 = record(257, 5, &[]);
    let mut body = r#ref(5);
    body.extend(s("261"));
    body.extend(le64(0x0501));
    body.extend([0; 12]);
    let r = record(405, 0x0501, &body);
    let mut bulk = r5.clone();
    bulk.extend(&r);
    let m = meta(&[(5, 0), (0x0501, r5.len() as u64)], &|i| usize::from(i == 5));
    let d = Design::new(&m, &bulk).unwrap();
    // One reference: the real one of the body.
    assert_eq!(super::stream::ref_ids(&d, d.rec(0x0501)), vec![5]);
    // The same bytes behind no tag stay a reference.
    let mut body = le64(0x0501);
    body.extend([0; 12]);
    let r = record(405, 0x0501, &body);
    let mut bulk = r5.clone();
    bulk.extend(&r);
    let d = Design::new(&m, &bulk).unwrap();
    assert_eq!(super::stream::ref_ids(&d, d.rec(0x0501)), vec![5]);
}

#[test]
fn a_region_cut_by_other_curves_gets_a_point_in_every_face() {
    let square: Vec<geom::P2> = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
    let circle = |c: geom::P2, r: f64| -> Vec<geom::P2> {
        (0..=64)
            .map(|k| {
                let t = std::f64::consts::TAU * f64::from(k) / 64.0;
                [c[0] + r * libm::cos(t), c[1] + r * libm::sin(t)]
            })
            .collect()
    };
    // Nothing runs through it: the one interior point.
    let outside = circle([9.0, 9.0], 1.0);
    let own: Vec<geom::P2> = vec![[0.0, 0.0], [4.0, 0.0]];
    let pts = geom::face_points(&square, &[], &[&outside, &own]).unwrap();
    assert_eq!(pts.len(), 1);
    // A circle inside: the disc and the rest.
    let disc = circle([2.0, 2.0], 1.0);
    let pts = geom::face_points(&square, &[], &[&disc]).unwrap();
    assert_eq!(pts.len(), 2);
    let in_disc = |p: &geom::P2| libm::hypot(p[0] - 2.0, p[1] - 2.0) < 1.0;
    assert_eq!(pts.iter().filter(|p| in_disc(p)).count(), 1);
    // A circle across a corner: the lens in the square and the rest (the part outside is not
    // the region's).
    let corner = circle([0.0, 0.0], 1.5);
    let pts = geom::face_points(&square, &[], &[&corner]).unwrap();
    assert_eq!(pts.len(), 2);
    assert!(pts.iter().all(|p| geom::inside(*p, &square)));
    assert_eq!(pts.iter().filter(|p| libm::hypot(p[0], p[1]) < 1.5).count(), 1);
    // A line that ends inside cuts nothing off; a hole is no face.
    let spur: Vec<geom::P2> = vec![[2.0, -1.0], [2.0, 3.0]];
    assert_eq!(geom::face_points(&square, &[], &[&spur]).unwrap().len(), 1);
    let hole = circle([2.0, 2.0], 1.0);
    let pts = geom::face_points(&square, &[&hole], &[&hole]).unwrap();
    assert_eq!(pts.len(), 1);
    assert!(!in_disc(&pts[0]));
    // Two lines across a ring: two halves, not joined over the hole.
    let across: Vec<geom::P2> = vec![[-1.0, 2.0], [5.0, 2.0]];
    assert_eq!(geom::face_points(&square, &[&hole], &[&across]).unwrap().len(), 2);
}

#[test]
fn a_join_of_several_bodies_is_a_join_and_a_combine() {
    let join = |name: &str, target: serde_json::Value| json!({"type": "extrude", "name": name, "operation": "join", "creates_bodies": 0, "target": target, "component": "Top"});
    // The body a later feature names lives on.
    let mut feats = vec![join("Extrude11", json!(["Extrude1", "Extrude9"])), join("Extrude12", json!("Extrude9"))];
    super::json::joins_of_several_bodies(&mut feats);
    assert_eq!(feats.len(), 3);
    assert_eq!(feats[0]["target"], "Extrude9");
    assert_eq!(feats[1]["type"], "combine");
    assert_eq!(feats[1]["name"], "Extrude11 bodies");
    assert_eq!((&feats[1]["target"], &feats[1]["tools"]), (&json!("Extrude9"), &json!(["Extrude1"])));
    assert_eq!(feats[1]["component"], "Top");
    // No later feature names one: the first.
    let mut feats = vec![join("Extrude7", json!(["Extrude1", "Extrude6"]))];
    super::json::joins_of_several_bodies(&mut feats);
    assert_eq!((&feats[0]["target"], &feats[1]["tools"]), (&json!("Extrude1"), &json!(["Extrude6"])));
    // Later features name both, or the join also makes a body: left as it is.
    let mut feats =
        vec![join("E3", json!(["A", "B"])), json!({"type": "combine", "name": "C", "target": "A", "tools": ["B"]})];
    super::json::joins_of_several_bodies(&mut feats);
    assert_eq!((feats.len(), &feats[0]["target"]), (2, &json!(["A", "B"])));
    let mut makes = join("E4", json!(["A", "B"]));
    makes["creates_bodies"] = 1.into();
    let mut feats = vec![makes];
    super::json::joins_of_several_bodies(&mut feats);
    assert_eq!(feats.len(), 1);
}

#[test]
fn a_frame_narrower_than_the_grid_has_an_interior_point() {
    // 2232 Extrude8: a 121.5 x 76.6 mm rectangle with a hole 0.4 mm inside it.
    let outer: Vec<geom::P2> = vec![[6.075, -3.83], [6.075, 3.83], [-6.075, 3.83], [-6.075, -3.83]];
    let hole: Vec<geom::P2> = vec![[-6.035, -3.79], [6.035, -3.79], [6.035, 3.79], [-6.035, 3.79]];
    let p = geom::interior_point(&outer, &[&hole]).unwrap();
    assert!(geom::inside(p, &outer) && !geom::inside(p, &hole));
    // In the middle of the frame's width.
    let d = (6.075 - p[0].abs()).min(3.83 - p[1].abs());
    assert!(d > 0.01 && d < 0.03, "{p:?}");
}

#[test]
fn edge_paths_and_picked_faces_move_with_a_placement() {
    // A quarter turn about z, then 10 mm along x: (x, y, z) -> (10 - y, x, z).
    let pl = crate::fusion::Placement { r: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], t: [10.0, 0.0, 0.0] };
    let mut fillet = json!({"type": "fillet", "name": "F", "radius": 1.0,
        "edges": [{"midpoint": [1.0, 2.0, 3.0], "path": [[1.0, 0.0, 3.0], [1.0, 2.0, 3.0]]}]});
    super::json::move_feature(&mut fillet, &pl);
    assert_eq!(fillet["edges"][0]["midpoint"], json!([8.0, 1.0, 3.0]));
    assert_eq!(fillet["edges"][0]["path"], json!([[10.0, 1.0, 3.0], [8.0, 1.0, 3.0]]));
    for ty in ["shell", "offset_face"] {
        let mut f = json!({"type": ty, "name": "S", "faces": [{"point": [1.0, 2.0, 3.0], "normal": [1.0, 0.0, 0.0]}]});
        super::json::move_feature(&mut f, &pl);
        assert_eq!(f["faces"][0], json!({"point": [8.0, 1.0, 3.0], "normal": [0.0, 1.0, 0.0]}), "{ty}");
    }
}

#[test]
fn coverage_tiles_every_record_into_known_and_unknown_spans() {
    let r5 = record(256, 5, &[9, 9, 9]);
    let mut body6 = r#ref(5);
    body6.extend(le32(2));
    body6.extend(r#ref(5));
    body6.extend(r#ref(6));
    let r6 = record(257, 6, &body6);
    let off6 = r5.len() as u64;
    let bulk = [r5.clone(), r6.clone()].concat();
    let m = meta(&[(5, 0), (6, off6)], &|i| usize::from(i == 6));
    let dir = "FusionAssetName[Active]/Design1/";
    let files = vec![(format!("{dir}MetaStream.dat"), m), (format!("{dir}BulkStream.dat"), bulk.clone())];
    let c = super::coverage(&files).unwrap();
    assert_eq!(c.bulk_len, bulk.len());
    assert_eq!(c.outside_records, 0);
    assert_eq!(c.records.len(), 2);
    for (r, want) in c.records.iter().zip([&r5, &r6]) {
        // The spans tile the record exactly.
        assert_eq!(&r.bytes, want);
        let mut at = 0;
        for sp in &r.spans {
            assert_eq!(sp.start, at);
            at += sp.len;
        }
        assert_eq!(at, r.bytes.len());
        // The header (tag, id) is decoded framing.
        assert_eq!(r.spans[0].kind, super::SpanKind::Decoded);
        assert_eq!(r.spans[0].name, "header.tag-id");
    }
    // No reader reads these classes: the bodies stay unknown, the references are guessed.
    let r = &c.records[1];
    assert_eq!(r.tally()[0], body6.len());
    assert_eq!(r.guesses.iter().filter(|g| matches!(g.2, super::Guess::Ref { .. })).count(), 3);
    assert_eq!(r.tag, Some(257));
    assert!(r.class.starts_with("AAAAAAAA"));
    let dump = c.dump();
    assert!(dump.contains("#6 tag 257"), "{dump}");
    assert!(dump.contains("UNKNOWN"));
    // Outside a coverage run nothing is recorded.
    assert!(!crate::cover::on());
}

/// A header jointed onto a board: the root component 3, the board's occurrence 31 and the
/// header's 32 (component 50, blob `BREP.hdr.smb`), the joint 10 with its snapshot 70, and the
/// path records 80 (the header: where the insert 90 put it, then where the joint did) and 81
/// (the board, never moved). Frame 1 is at (4, 0, 0) cm on the header, flipped and set off by
/// 0.5 cm along z; frame 2 is at (10, 20, 1) cm on the board.
fn jointed_header(flip: u8) -> (Vec<u8>, Vec<u8>) {
    const CLASSES: [&str; 11] = [
        "8DFDD543-0000-0000-0000-000000000000",
        "7B6C3C2D-0000-0000-0000-000000000000",
        "02E8AA49-0000-0000-0000-000000000000",
        "7F5A0426-0000-0000-0000-000000000000",
        "549FBB80-0000-0000-0000-000000000000",
        "CE2913AA-0000-0000-0000-000000000000",
        "E03784ED-0000-0000-0000-000000000000",
        "CD57BC48-0000-0000-0000-000000000000",
        "D91D429C-0000-0000-0000-000000000000",
        "7A6A3D31-0000-0000-0000-000000000000",
        "11111111-0000-0000-0000-000000000000",
    ];
    let refs = |ids: &[u64]| ids.iter().flat_map(|i| r#ref(*i)).collect::<Vec<u8>>();
    let mat = |r: [[f64; 3]; 3], t: [f64; 3]| -> Vec<u8> {
        let rows =
            [[r[0][0], r[0][1], r[0][2], t[0]], [r[1][0], r[1][1], r[1][2], t[1]], [r[2][0], r[2][1], r[2][2], t[2]]];
        rows.iter().flatten().chain(&[0.0, 0.0, 0.0, 1.0]).flat_map(|v| f64::to_le_bytes(*v)).collect()
    };
    let one = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let over = [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]];
    // A list entry: the reference and a zero byte.
    let item = |id: u64| [r#ref(id), vec![0]].concat();
    let path = |occ: u64| [le32(1), item(occ), item(3)].concat();
    let mut recs: Vec<(u64, usize, Vec<u8>)> = Vec::new();
    // The joint: flip, the two origins with their frames, the four holders, the snapshot.
    let mut j = vec![flip, 0, 0];
    j.extend([r#ref(60), vec![0, 0], mat(one, [4.0, 0.0, 0.0])].concat());
    j.extend([r#ref(61), vec![0, 0], mat(one, [10.0, 20.0, 1.0])].concat());
    j.extend([0; 4]);
    j.extend(refs(&[20, 21, 22, 23, 70]));
    recs.push((10, 0, j));
    recs.push((60, 1, Vec::new()));
    recs.push((61, 2, Vec::new()));
    for (holder, param, expr, name, v) in [
        (20, 25, "0 deg", "alignAngle", 0.0),
        (21, 26, "5 mm", "alignOffsetZ", 0.5),
        (22, 27, "0 mm", "alignOffsetX", 0.0),
        (23, 28, "0 mm", "alignOffsetY", 0.0),
    ] {
        recs.push((holder, 8, refs(&[param])));
        let mut p = ws(expr);
        p.extend(ws(name));
        p.extend(ws("d1"));
        p.extend(f64::to_le_bytes(v));
        recs.push((param, 9, p));
    }
    // The snapshot: every occurrence involved, then the two sides.
    let mut snap = [vec![1], le32(2), item(31), le32(7), item(32), le32(1), vec![0; 9], le32(2)].concat();
    snap.extend([vec![0], mat(over, [4.0, 0.0, 0.5]), path(32)].concat());
    snap.extend([vec![0], mat(one, [10.0, 20.0, 1.0]), path(31)].concat());
    recs.push((70, 3, snap));
    // The header's path: where the insert put it, then where the joint did.
    let step = |id: u64, m: Option<Vec<u8>>| match m {
        Some(m) => [vec![1, 0], item(id), vec![0], m].concat(),
        None => [vec![1, 0], item(id), vec![1]].concat(),
    };
    let header = [
        vec![0; 9],
        path(32),
        le32(1),
        le32(2),
        step(90, Some(mat(over, [0.0, -8.0, 1.0]))),
        step(10, Some(mat(over, [6.0, 20.0, 1.5]))),
    ];
    recs.push((80, 4, header.concat()));
    recs.push((81, 4, [vec![0; 9], path(31), le32(1), le32(1), step(10, None)].concat()));
    recs.push((90, 10, Vec::new()));
    recs.push((31, 5, refs(&[40])));
    recs.push((32, 5, refs(&[50])));
    recs.push((3, 6, Vec::new()));
    recs.push((40, 6, Vec::new()));
    recs.push((50, 6, [refs(&[51]), ws("Header")].concat()));
    recs.push((51, 7, refs(&[52])));
    recs.push((52, 10, ws("BREP.hdr.smb")));
    let mut bulk = Vec::new();
    let mut objs = Vec::new();
    for (id, class, body) in recs {
        objs.push((id, bulk.len() as u64, class));
        bulk.extend(record(256 + class as u32, id, &body));
    }
    (meta_classes(&CLASSES, &objs), bulk)
}

#[test]
fn a_verified_joint_places_its_occurrence_where_the_path_record_has_it() {
    let (m, bulk) = jointed_header(1);
    let got = super::joint_placements(&m, &bulk);
    assert_eq!((got.verified, got.unverified.len()), (1, 0), "{got:?}");
    let header = got.placements.iter().find(|p| p.path == [32]).unwrap();
    assert_eq!((header.name.as_str(), header.blobs.as_slice()), ("Header", &["BREP.hdr.smb".to_string()][..]));
    assert_eq!(header.placement.t, [60.0, 200.0, 15.0]);
    assert_eq!(header.placement.r[1], [0.0, -1.0, 0.0]);
    // The body stood where the insert put it; other bodies stay.
    let at = |t: [f64; 3]| crate::fusion::Placement { t, ..crate::fusion::Placement::IDENTITY };
    let body = |brep: &str, t: [f64; 3]| crate::fusion::Placed {
        brep: brep.into(),
        occurrence: "x:1".into(),
        placement: at(t),
    };
    let mut placed = vec![body("hdr", [0.0, -80.0, 10.0]), body("other", [1.0, 2.0, 3.0])];
    super::place_by_joints(&mut placed, &got);
    assert_eq!(placed.len(), 2);
    assert_eq!((placed[0].occurrence.as_str(), placed[0].placement), ("Header:1", header.placement));
    assert_eq!(placed[1], body("other", [1.0, 2.0, 3.0]));
    // A body the occurrence records did not place at all is placed too.
    let mut placed = vec![body("other", [1.0, 2.0, 3.0])];
    super::place_by_joints(&mut placed, &got);
    assert_eq!(placed.len(), 2);
    assert_eq!(placed[1].placement, header.placement);
    // An inserted design in the root component takes the placement of its occurrence's path.
    let mut tree = json!({"features": [{"type": "component_insert", "_occurrence": 32,
        "placement": {"r": [[1, 0, 0], [0, 1, 0], [0, 0, 1]], "t": [0, 0, 0]}}]});
    super::joint::place_inserts(&mut tree, &got);
    assert_eq!(tree["features"][0]["placement"]["t"], json!([60.0, 200.0, 15.0]));
}

#[test]
fn a_joint_that_disagrees_with_the_stored_placement_places_nothing() {
    // The flip byte says the frames look the same way; the stored frame is turned over.
    let (m, bulk) = jointed_header(0);
    let got = super::joint_placements(&m, &bulk);
    assert_eq!((got.verified, got.unverified.len()), (0, 1), "{got:?}");
    assert!(got.placements.is_empty());
    let mut placed = vec![crate::fusion::Placed {
        brep: "hdr".into(),
        occurrence: "x:1".into(),
        placement: crate::fusion::Placement::IDENTITY,
    }];
    let before = placed.clone();
    super::place_by_joints(&mut placed, &got);
    assert_eq!(placed, before);
    // A stream without joints says nothing.
    assert_eq!(super::joint_placements(&meta_classes(&[], &[]), &[]), super::Joints::default());
    // Damaged records are refused without panics.
    for cut in (0..bulk.len()).step_by(7) {
        let _ = super::joint_placements(&m, &bulk[..cut]);
    }
}

/// A Base Feature (10) with the PM bodies `pm`, the one DM body 31 (its body link 41) and the PM
/// bodies named again `again`; the PM bodies' state list (50) names the blob `BREP.pm.smb`.
fn base_feature(pm: &[u64], again: &[u64]) -> (Vec<u8>, Vec<u8>) {
    const CLASSES: [&str; 5] = [
        "D7015B81-0000-0000-0000-000000000000",
        "D3937028-0000-0000-0000-000000000000",
        "CD57BC48-0000-0000-0000-000000000000",
        "40DCC3F4-0000-0000-0000-000000000000",
        "D26351F0-0000-0000-0000-000000000000",
    ];
    let refs = |ids: &[u64]| ids.iter().flat_map(|i| r#ref(*i)).collect::<Vec<u8>>();
    let mut bf: Vec<u64> = pm.to_vec();
    bf.push(31);
    bf.extend(again);
    bf.push(41);
    let mut list: Vec<u64> = pm.to_vec();
    list.push(60);
    list.extend(pm);
    let mut recs: Vec<(u64, usize, Vec<u8>)> = vec![(10, 0, refs(&bf)), (31, 1, refs(&[51]))];
    recs.extend(pm.iter().map(|b| (*b, 1, refs(&[50]))));
    recs.extend([(50, 2, refs(&list)), (51, 2, refs(&[31])), (60, 3, ws("BREP.pm.smb")), (41, 4, refs(&[31, 10]))]);
    let mut bulk = Vec::new();
    let mut objs = Vec::new();
    for (id, class, body) in recs {
        objs.push((id, bulk.len() as u64, class));
        bulk.extend(record(256 + class as u32, id, &body));
    }
    (meta_classes(&CLASSES, &objs), bulk)
}

#[test]
fn the_inputs_a_base_feature_drops_are_the_pm_bodies_it_names_once() {
    // Three PM bodies; the third becomes the DM body: the first two are dropped.
    let (m, bulk) = base_feature(&[21, 22, 23], &[23]);
    let d = Design::new(&m, &bulk).unwrap();
    let g = super::Graph::new(&d);
    assert_eq!(json::base_feature_inputs(&d, &g, 10), Some(json!({"brep": "BREP.pm.smb", "count": 3, "kept": [2]})));
    // One PM body that becomes the DM body: nothing is dropped.
    let (m, bulk) = base_feature(&[21], &[21]);
    let d = Design::new(&m, &bulk).unwrap();
    assert_eq!(json::base_feature_inputs(&d, &super::Graph::new(&d), 10), None);
    // The kept body is not one of the PM bodies: the records do not say it plainly.
    let (m, bulk) = base_feature(&[21, 22, 23], &[24]);
    let d = Design::new(&m, &bulk).unwrap();
    assert_eq!(json::base_feature_inputs(&d, &super::Graph::new(&d), 10), None);
}
