#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;
use crate::fixtures::{box_sat, to_binary};

#[test]
fn both_layouts_parse_to_the_same_fields() {
    let a = parse(box_sat(600, [1.0, 2.0, 3.0]).as_bytes(), &Limits::default()).unwrap();
    let b = parse(box_sat(700, [1.0, 2.0, 3.0]).as_bytes(), &Limits::default()).unwrap();
    // Equal up to the edges' trailing convexity string (`7 unknown` against `@7 unknown`).
    let cut = |r: &Rec| Rec {
        ty: r.ty.clone(),
        f: r.f.iter().take(if r.ty == "edge" { 8 } else { usize::MAX }).cloned().collect(),
    };
    assert_eq!(a.recs.len(), b.recs.len());
    assert!(a.recs.iter().zip(&b.recs).all(|(x, y)| cut(x) == cut(y)), "the layouts differ");
    assert_eq!(a.recs[0].ty, "body");
    assert_eq!(a.recs[3].word(6), Some("forward"));
}

#[test]
fn a_box_converts_to_one_solid_with_six_faces() {
    let c = to_step(box_sat(700, [10.0, 20.0, 30.0]).as_bytes(), "box", &Limits::default()).unwrap();
    assert_eq!(c.bodies, 1);
    assert_eq!(c.step.matches("ADVANCED_FACE").count(), 6);
    assert_eq!(c.step.matches("EDGE_CURVE").count(), 12);
    assert_eq!(c.step.matches("VERTEX_POINT").count(), 8);
    assert!(c.step.contains("MANIFOLD_SOLID_BREP"));
}

#[test]
fn hostile_input_is_refused_not_panicked_on() {
    for bad in [
        &b""[..],
        b"700 0 1 0\nx\n1\n",
        b"105 0 1 0\nx\n1\nbody $1 #\n",
        b"700 0 1 0\nx\n1\ntransform $-1 -1 1 #\n",
        b"700 0 1 0\nx\n1\nbody $-1 -1 $-1 $5 $-1 $-1 #\n",
        b"700 0 1 0\nx\n1\nbody $-1 -1 $-1 $0 $-1 $-1 #\nlump $-1 -1 $-1 $1 $0 $0 #\n",
        b"700 0 1 0\nx\n1\n@99999999999 x #",
    ] {
        let _ = to_step(bad, "x", &Limits::default());
    }
    let sat = box_sat(700, [1.0, 1.0, 1.0]);
    let small = Limits { max_records: 10, max_tokens: 1_000_000 };
    assert!(matches!(to_step(sat.as_bytes(), "x", &small), Err(SatError::TooLarge(_))));
    // A surface not read yet is named, and its face left out: the rest comes in as a sheet.
    let odd = sat.replacen("plane-surface", "meshsurf-surface", 1);
    let c = to_step(odd.as_bytes(), "x", &Limits::default()).unwrap();
    assert!(c.warnings.iter().any(|w| w.contains("meshsurf-surface")), "{:?}", c.warnings);
    assert_eq!(c.step.matches("ADVANCED_FACE").count(), 5);
    assert!(c.step.contains("OPEN_SHELL"));
}

#[test]
fn a_bodys_tag_is_the_integer_after_its_attribute_pointer() {
    // Version 7 writes an integer and a pointer after each record's attribute pointer; in a
    // Fusion history blob the integer is the tag the design's body map names (section 60).
    let text = box_sat(700, [1.0, 2.0, 3.0]).replacen("body $-1 -1 $-1", "body $-1 1758 $-1", 1);
    for bytes in [text.as_bytes().to_vec(), to_binary(&text)] {
        assert_eq!(body_tags(&bytes, &Limits::default()).unwrap(), vec![Some(1758)]);
        assert_eq!(body_count(&bytes, &Limits::default()).unwrap(), 1);
    }
    // Before version 7 there is none.
    assert_eq!(body_tags(box_sat(600, [1.0, 2.0, 3.0]).as_bytes(), &Limits::default()).unwrap(), vec![None]);
    // The fields read the same with or without the tag.
    let a = parse(text.as_bytes(), &Limits::default()).unwrap();
    let b = parse(box_sat(700, [1.0, 2.0, 3.0]).as_bytes(), &Limits::default()).unwrap();
    assert_eq!(a.recs, b.recs);
}

#[test]
fn the_binary_form_converts_like_the_text() {
    let text = box_sat(700, [10.0, 20.0, 30.0]);
    let a = to_step(text.as_bytes(), "box", &Limits::default()).unwrap();
    let b = to_step(&to_binary(&text), "box", &Limits::default()).unwrap();
    assert_eq!(a.step, b.step);
    assert_eq!(b.version, 22500);
    // Truncated or unknown tags are refused, not panicked on.
    let bin = to_binary(&text);
    for n in [16, 50, 200, bin.len() - 3] {
        let _ = to_step(&bin[..n], "x", &Limits::default());
    }
    let mut odd = bin.clone();
    if let Some(x) = odd.get_mut(200) {
        *x = 0x7f;
    }
    let _ = to_step(&odd, "x", &Limits::default());
}

#[test]
fn a_64_bit_integer_tag_reads_as_an_integer() {
    // Fusion's derived bodies write `17` and 8 bytes (an attribute's source entity id).
    let text = box_sat(700, [10.0, 20.0, 30.0]);
    let bin = to_binary(&text);
    let int_minus_one = [&[0x04u8][..], &(-1i64).to_le_bytes()].concat();
    let at = bin.windows(9).position(|w| w == int_minus_one.as_slice()).unwrap();
    let mut wide = bin.clone();
    if let Some(x) = wide.get_mut(at) {
        *x = 0x17;
    }
    let a = to_step(&bin, "box", &Limits::default()).unwrap();
    let b = to_step(&wide, "box", &Limits::default()).unwrap();
    assert_eq!(a.step, b.step);
}

#[test]
fn an_end_record_without_terminator_ends_the_file() {
    // Inventor writes the history section's end and the file's end as bare type names with
    // no record terminator, then other data (here: a tag the reader does not know).
    let text = box_sat(700, [10.0, 20.0, 30.0]);
    let mut bin = to_binary(&text);
    let tail = [&[0x0du8, 15][..], b"End-of-ASM-data", &[0x11]].concat();
    assert!(bin.ends_with(&tail));
    bin.truncate(bin.len() - tail.len());
    for (tag, part) in [(0x0eu8, "End"), (0x0e, "of"), (0x0e, "ASM"), (0x0e, "History"), (0x0d, "Section")] {
        bin.extend([tag, part.len() as u8]);
        bin.extend(part.as_bytes());
    }
    for (tag, part) in [(0x0eu8, "End"), (0x0e, "of"), (0x0e, "ASM"), (0x0d, "data")] {
        bin.extend([tag, part.len() as u8]);
        bin.extend(part.as_bytes());
    }
    bin.extend([0x41, 1, 0, 0x80, 0x7f, 0x7f]);
    let a = to_step(text.as_bytes(), "box", &Limits::default()).unwrap();
    let b = to_step(&bin, "box", &Limits::default()).unwrap();
    assert_eq!(a.step, b.step);
}

#[test]
fn shared_loops_and_endless_helices_are_refused() {
    // Two faces naming one loop (the review's 1 MB file that became 1 GB of STEP).
    let sat = box_sat(700, [1.0, 1.0, 1.0]);
    let mut lines: Vec<String> = sat.lines().map(str::to_string).collect();
    let faces: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.starts_with("face ")).map(|(i, _)| i).collect();
    let first_loop = lines[faces[0]].split_whitespace().nth(5).unwrap().to_string();
    let second: Vec<String> = lines[faces[1]].split_whitespace().map(str::to_string).collect();
    lines[faces[1]] = lines[faces[1]].replacen(&second[5], &first_loop, 1);
    let e = to_step(lines.join("\n").as_bytes(), "x", &Limits::default()).unwrap_err().to_string();
    assert!(e.contains("more than one face"), "{e}");
    // A helix of a million turns (the review's 4 KB file that became 868 MB).
    let first = sat.lines().find(|l| l.starts_with("straight-curve")).unwrap().to_string();
    let helix =
        "intcurve-curve $-1 -1 $-1 forward { helix_int_cur 22310 F 0 F 6283185 0 0 0 1 0 0 0 1 0 0 0 1 0 0 0 1 } I I #";
    let e = to_step(sat.replacen(&first, helix, 1).as_bytes(), "x", &Limits::default());
    assert!(e.as_ref().map_or(true, |c| c.warnings.iter().any(|w| w.contains("left out"))), "{e:?}");
    assert!(e.as_ref().map_or(true, |c| c.step.len() < 1 << 20));
}

#[test]
fn a_placement_moves_every_point_after_the_bodys_own_transform() {
    let points = |s: &str| -> Vec<[f64; 3]> {
        s.lines()
            .filter_map(|l| l.split_once("CARTESIAN_POINT('',(")?.1.split_once("))").map(|(v, _)| v.to_string()))
            .map(|v| {
                let c: Vec<f64> = v.split(',').map(|x| x.trim().trim_end_matches('.').parse().unwrap()).collect();
                [c[0], c[1], c[2]]
            })
            .collect()
    };
    let sat = box_sat(700, [10.0, 20.0, 30.0]);
    let plain = to_step(sat.as_bytes(), "b", &Limits::default()).unwrap();
    // A quarter turn about Z, then 100 mm along X: (x, y, z) -> (100 - y, x, z).
    let p = fusion::Placement { r: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], t: [100.0, 0.0, 0.0] };
    let moved = to_step_placed(sat.as_bytes(), "b", &Limits::default(), Some(&p), true).unwrap();
    let (a, b) = (points(&plain.step), points(&moved.step));
    assert!(!a.is_empty() && a.len() == b.len());
    for (u, v) in a.iter().zip(&b) {
        let want = [100.0 - u[1], u[0], u[2]];
        assert!((0..3).all(|i| (v[i] - want[i]).abs() < 1e-9), "{u:?} -> {v:?}, want {want:?}");
    }
}

#[test]
fn sheets_are_left_out_when_not_asked_for() {
    // One face on a surface not read yet leaves an open shell: a sheet body.
    let sat = box_sat(700, [1.0, 1.0, 1.0]).replacen("plane-surface", "meshsurf-surface", 1);
    let with = to_step_placed(sat.as_bytes(), "x", &Limits::default(), None, true).unwrap();
    assert_eq!((with.bodies, with.skipped_sheets), (1, 0));
    let without = to_step_placed(sat.as_bytes(), "x", &Limits::default(), None, false).unwrap();
    assert_eq!((without.bodies, without.skipped_sheets), (0, 1));
    assert!(without.step.is_empty());
}

#[test]
fn faces_with_a_known_material_are_styled_with_its_colour() {
    let id = "617919e9-cd5c-4110-97a6-83c6bd6b90e3";
    let plain = box_sat(600, [10.0, 20.0, 30.0]);
    let n = parse(plain.as_bytes(), &Limits::default()).unwrap().recs.len();
    // The first face (record 3) and the second (record 4) carry a material; the body none.
    let mut text = plain.replacen("face $-1", &format!("face ${n}"), 1);
    text = text.replacen("face $-1", &format!("face ${}", n + 1), 1);
    let attr = |owner: usize| format!("ATTRIB_CUSTOM-attrib $-1 $-1 ${owner} NEUTRON_Material_attrib_def 1 {id} #\n");
    text = text.replace("End-of-ACIS-data", &format!("{}{}End-of-ACIS-data", attr(3), attr(4)));
    let sat = parse(text.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(sat.materials().get(&3), Some(&id));
    assert_eq!(sat.materials().len(), 2);
    let mut colours = Colours::new();
    colours.insert(id.to_string(), 0x00d4_ab21);
    let c = to_step_coloured(text.as_bytes(), "box", &Limits::default(), None, true, &colours).unwrap();
    assert_eq!(c.step.matches("STYLED_ITEM('color'").count(), 2, "{}", c.step);
    assert_eq!(c.step.matches("COLOUR_RGB").count(), 1);
    assert!(c.step.contains("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"));
    // Without a colour for it, nothing is styled.
    let none = to_step_coloured(text.as_bytes(), "box", &Limits::default(), None, true, &Colours::new()).unwrap();
    assert!(!none.step.contains("STYLED_ITEM"));
}

/// A box whose first two faces carry `material` (when given) and a tag each, as Fusion writes
/// them (`generic_tag_attrib_def` with the attribute's own definition name repeated).
fn tagged_box(material: Option<&str>, tags: [&str; 2]) -> String {
    let plain = box_sat(600, [10.0, 20.0, 30.0]);
    let n = parse(plain.as_bytes(), &Limits::default()).unwrap().recs.len();
    let mut text = plain.replacen("face $-1", &format!("face ${n}"), 1);
    text = text.replacen("face $-1", &format!("face ${}", n + 1), 1);
    let mut attrs = String::new();
    for (k, owner) in [3usize, 4].into_iter().enumerate() {
        if let Some(m) = material {
            attrs += &format!("ATTRIB_CUSTOM-attrib $-1 $-1 ${owner} NEUTRON_Material_attrib_def 1 {m} #\n");
        }
        let t = tags[k];
        attrs += &format!(
            "ATTRIB_CUSTOM-attrib $-1 $-1 ${owner} generic_tag_attrib_def 3 3 -1 generic_tag_attrib_def 1 3 {t} 0 1 484 0 #\n"
        );
    }
    text.replace("End-of-ACIS-data", &format!("{attrs}End-of-ACIS-data"))
}

#[test]
fn faces_without_a_material_take_the_one_their_history_blob_has_for_their_tag() {
    let id = "e852ce83-33d8-425f-8ebe-98d1ae611cf2";
    let history = tagged_box(Some(id), ["152", "150"]);
    let body = tagged_box(None, ["152", "150"]);
    let h = History::read(history.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(h.len(), 2);
    let mut colours = Colours::new();
    colours.insert(id.to_string(), 0x00fb_d4a9);
    let styled = |text: &str, h: Option<&History>, colours: &Colours| {
        let c = to_step_coloured_history(text.as_bytes(), "b", &Limits::default(), None, true, colours, h).unwrap();
        c.step.matches("STYLED_ITEM('color'").count()
    };
    assert_eq!(styled(&body, Some(&h), &colours), 2);
    // Without the history, or without a colour for the material, nothing is styled.
    assert_eq!(styled(&body, None, &colours), 0);
    assert_eq!(styled(&body, Some(&h), &Colours::new()), 0);
    // A face's own material wins over the history's.
    let own = "617919e9-cd5c-4110-97a6-83c6bd6b90e3";
    let mut both = colours.clone();
    both.insert(own.to_string(), 0x0040_4040);
    let mixed = tagged_box(Some(own), ["152", "150"]);
    let c = to_step_coloured_history(mixed.as_bytes(), "b", &Limits::default(), None, true, &both, Some(&h)).unwrap();
    assert_eq!(c.step.matches("COLOUR_RGB").count(), 1, "the history's colour must not override the face's own");
    assert!(c.step.contains("COLOUR_RGB('',0.25"), "{}", c.step);
    // Tags that name other faces are not matched: the history is another body's.
    let other = tagged_box(None, ["7", "8"]);
    assert_eq!(styled(&other, Some(&h), &colours), 0);
    // Half of the tags matching is enough; a tag two faces share is skipped.
    let twin = tagged_box(None, ["152", "152"]);
    assert_eq!(styled(&twin, Some(&h), &colours), 0);
}

#[test]
fn a_history_without_materials_or_with_conflicting_ones_gives_nothing() {
    assert!(History::read(tagged_box(None, ["1", "2"]).as_bytes(), &Limits::default()).is_none());
    // The same tag on two faces with different materials is dropped.
    let a = "617919e9-cd5c-4110-97a6-83c6bd6b90e3";
    let plain = tagged_box(Some(a), ["5", "5"]).replacen(a, "0bf2cf32-4921-49f1-b71a-f1fd22dcd453", 1);
    let h = History::read(plain.as_bytes(), &Limits::default());
    assert!(h.is_none_or(|h| h.len() <= 1));
    for n in [0usize, 1, 7, 100] {
        let noise: Vec<u8> = (0..n).map(|i| (i * 37 % 251) as u8).collect();
        assert!(History::read(&noise, &Limits::default()).is_none());
    }
    let text = tagged_box(Some(a), ["1", "2"]);
    for n in (0..text.len()).step_by(41) {
        let _ = History::read(&text.as_bytes()[..n], &Limits::default());
    }
}

/// Two boxes as two bodies, each with a transform (as Fusion writes imported features: the
/// first body is the feature's helper, the next ones are the bodies its references colour).
fn two_boxes() -> String {
    let a = box_sat(700, [10.0, 20.0, 30.0]);
    let b = box_sat(700, [1.0, 2.0, 3.0]);
    let recs = |s: &str| -> Vec<String> {
        s.lines().skip(3).take_while(|l| *l != "End-of-ACIS-data").map(String::from).collect()
    };
    let (ra, rb) = (recs(&a), recs(&b));
    let shift = ra.len();
    let moved = |l: &str| -> String {
        l.split(' ')
            .map(|t| match t.strip_prefix('$').and_then(|n| n.parse::<i64>().ok()) {
                Some(n) if n >= 0 => format!("${}", n + shift as i64),
                _ => t.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let n = ra.len() + rb.len();
    let mut all: Vec<String> = ra;
    all.extend(rb.iter().map(|l| moved(l)));
    all[0] = all[0].replacen("$-1 $-1 #", &format!("$-1 ${n} #"), 1);
    let second = shift;
    all[second] = all[second].replacen("$-1 $-1 #", &format!("$-1 ${} #", n + 1), 1);
    for _ in 0..2 {
        all.push("transform $-1 -1 $-1 1 0 0 0 1 0 0 0 1 0 0 0 1 no_rotate no_reflect no_shear #".into());
    }
    let head: Vec<&str> = a.lines().take(3).collect();
    format!("{}\n{}\nEnd-of-ACIS-data\n", head.join("\n"), all.join("\n"))
}

#[test]
fn bodies_without_a_material_take_the_colour_their_features_reference_gives() {
    let sat = two_boxes();
    let parsed = parse(sat.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(parsed.recs.iter().filter(|r| r.ty == "body").count(), 2);
    let styled = |h: &History| {
        let c = to_step_coloured_history(sat.as_bytes(), "b", &Limits::default(), None, true, &Colours::new(), Some(h))
            .unwrap();
        (c.step.matches("STYLED_ITEM('color'").count(), c.step.contains("COLOUR_RGB('',1."))
    };
    // The first body is the helper; the colour goes to the second body's six faces.
    let h = History::default().with_body_colours(vec![Some(0x00ff_0000)]);
    assert_eq!(styled(&h), (6, true));
    // A body the reference has no colour for stays plain; a count that does not fit is not
    // a pairing.
    assert_eq!(styled(&History::default().with_body_colours(vec![None])).0, 0);
    assert_eq!(styled(&History::default().with_body_colours(vec![Some(1), Some(2)])).0, 0);
    assert!(History::default().is_empty() && !History::default().with_body_colours(vec![None]).is_empty());
}

#[test]
fn a_history_blobs_header_unit_is_not_a_length_unit() {
    // Fusion's `.smbh` header carries 20..50 where the `.smb` blobs carry 10, with the same
    // centimetre coordinates: the nth-body reader takes the Fusion unit whatever the header says.
    let text = box_sat(700, [1.0, 2.0, 3.0]);
    let with_unit = |unit: f64| {
        let mut bin = to_binary(&text);
        let at = bin.iter().position(|b| *b == 0x06).unwrap();
        bin.get_mut(at + 1..at + 9).unwrap().copy_from_slice(&unit.to_le_bytes());
        bin
    };
    let (a, b) = (with_unit(10.0), with_unit(50.0));
    let a = crate::to_step_nth(&a, "box", &Limits::default(), None, 0, 1).unwrap();
    let b = crate::to_step_nth(&b, "box", &Limits::default(), None, 0, 1).unwrap();
    assert_eq!(a.step, b.step);
    // And it is centimetres: a 1 x 2 x 3 box is 10 x 20 x 30 mm.
    assert!(a.step.contains("30."), "{}", a.step.lines().find(|l| l.contains("CARTESIAN_POINT")).unwrap_or(""));
}

/// The box of [`box_sat`] (10 x 20 x 30) with its top face (z = 30) on the bilinear B-spline
/// surface `(u, v) -> (u, v, 30)`, its four control points listed u fastest or v fastest.
fn spline_top_box(version: u32, u_fastest: bool) -> String {
    let sat = box_sat(version, [10.0, 20.0, 30.0]);
    let pts = if u_fastest { "0 0 30 10 0 30 0 20 30 10 20 30" } else { "0 0 30 0 20 30 10 0 30 10 20 30" };
    // The top face's plane is the second surface record.
    let top = sat.lines().filter(|l| l.starts_with("plane-surface")).nth(1).unwrap().to_string();
    sat.replacen(
        &top,
        &format!(
            "spline-surface $-1 -1 $-1 forward {{ exactsur nubs 1 1 open open none none 2 2 0 1 10 1 0 1 20 1 {pts} 0 }} I I I I #"
        ),
        1,
    )
}

#[test]
fn shape_manager_files_list_surface_points_u_fastest() {
    let step = |version, u_fastest| {
        let c = to_step(spline_top_box(version, u_fastest).as_bytes(), "b", &Limits::default()).unwrap();
        c.step.lines().filter(|l| !l.starts_with("FILE_NAME")).collect::<Vec<_>>().join("\n")
    };
    // Version 7 (ACIS) lists them v fastest, ShapeManager (Fusion, Inventor) u fastest: both
    // give the surface the box's top edges lie on.
    let acis = step(700, false);
    assert!(acis.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
    assert_eq!(step(22300, true), acis);
    assert_ne!(step(22300, false), acis);
}

/// A torus face that reaches the axis at a point (3722's dome): the point is no vertex loop,
/// which OCCT's reader drops on a torus, but a seam to it from the top circle, written twice in
/// the face's one edge loop: an arc of the tube's circle from the rim up over the dome.
#[test]
fn a_spindle_torus_face_ends_at_the_axis_by_a_seam() {
    let (text, _) = crate::fixtures::spindle_dome_sat();
    let step = to_step(text.as_bytes(), "dome", &Limits::default()).unwrap().step;
    assert!(!step.contains("VERTEX_LOOP"), "{step}");
    let torus = step.lines().find(|l| l.contains("TOROIDAL_SURFACE")).unwrap().split('=').next().unwrap();
    let dome = step.lines().find(|l| l.contains("ADVANCED_FACE") && l.contains(&format!("{torus},"))).unwrap();
    // One bound: the circle, the seam there and back.
    assert_eq!(dome.matches('#').count(), 3, "{dome}");
    let seam = step.lines().rfind(|l| l.contains("EDGE_CURVE")).unwrap();
    assert!(seam.ends_with(".T.);"), "the arc runs counterclockwise from the rim: {seam}");
    let lp = step.lines().rfind(|l| l.contains("EDGE_LOOP")).unwrap();
    assert_eq!(lp.matches('#').count(), 4, "{lp}");
}
