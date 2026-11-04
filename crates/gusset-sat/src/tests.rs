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
    // A spline surface is named, not guessed.
    let spl = sat.replacen("plane-surface", "spline-surface", 1);
    let e = to_step(spl.as_bytes(), "x", &Limits::default()).unwrap_err().to_string();
    assert!(e.contains("spline-surface"), "{e}");
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
