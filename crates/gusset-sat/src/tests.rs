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
    let moved = to_step_placed(sat.as_bytes(), "b", &Limits::default(), Some(&p)).unwrap();
    let (a, b) = (points(&plain.step), points(&moved.step));
    assert!(!a.is_empty() && a.len() == b.len());
    for (u, v) in a.iter().zip(&b) {
        let want = [100.0 - u[1], u[0], u[2]];
        assert!((0..3).all(|i| (v[i] - want[i]).abs() < 1e-9), "{u:?} -> {v:?}, want {want:?}");
    }
}
