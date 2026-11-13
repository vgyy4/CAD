//! ADR 0049: board texts added, changed and deleted; placement and mirroring in our stroke
//! font; the round trip to `.kicad_pcb`.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits};
use crate::font;
use crate::text_edit::{NewText, TextPatch};

fn lim() -> Limits {
    Limits::default()
}

fn new(text: &str, at: [f64; 2]) -> NewText {
    NewText {
        text: text.into(),
        at,
        layer: None,
        size: None,
        thickness: None,
        angle: None,
        mirrored: None,
        justify: None,
    }
}

const WITH_FP: &str = r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (36 "B.SilkS" user) (44 "Edge.Cuts" user))
  (net 0 "")
  (footprint "R:0603" (layer "F.Cu") (at 10 10 90)
    (property "Reference" "R1" (at 0 -2) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "10k" (at 0 2) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
    (fp_text user "note" (at 1 1 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask"))
  )
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
)
"#;

#[test]
fn a_text_is_added_with_defaults_and_read_back() {
    let mut b = Board::new_two_layer(40.0, 20.0).unwrap();
    let i = b.add_text(&new("HELLO", [5.0, 6.0]), &lim()).unwrap();
    let t = &b.texts[i];
    assert_eq!((t.text.as_str(), t.at, t.layer.as_str()), ("HELLO", [5.0, 6.0], "F.SilkS"));
    assert_eq!((t.size, t.thickness, t.mirrored, t.footprint), (1.0, 0.15, false, None));
    // Round trip: the written file reads back to the same text.
    let again = Board::read(&b.write(), &lim()).unwrap();
    assert_eq!(again.texts, b.texts);
    assert!(b.write().contains("(gr_text \"HELLO\""));
    // Back side: mirrored by default; a line break survives the round trip.
    let j = b
        .add_text(
            &NewText { layer: Some("B.SilkS".into()), size: Some(2.0), angle: Some(90.0), ..new("A\nB", [1.0, 2.0]) },
            &lim(),
        )
        .unwrap();
    let again = Board::read(&b.write(), &lim()).unwrap();
    let t = &again.texts[j];
    assert_eq!((t.text.as_str(), t.mirrored, t.size, t.angle, t.thickness), ("A\nB", true, 2.0, 90.0, 0.3));
}

#[test]
fn bad_texts_are_refused() {
    let mut b = Board::new_two_layer(40.0, 20.0).unwrap();
    let before = b.write();
    for t in [
        new("", [0.0, 0.0]),
        new("x", [f64::NAN, 0.0]),
        NewText { size: Some(0.0), ..new("x", [0.0, 0.0]) },
        NewText { layer: Some("In7.Cu".into()), ..new("x", [0.0, 0.0]) },
        NewText { justify: Some([2, 0]), ..new("x", [0.0, 0.0]) },
        new("a\u{7}b", [0.0, 0.0]),
    ] {
        assert!(b.add_text(&t, &lim()).is_err(), "{t:?}");
    }
    assert_eq!(b.write(), before);
}

#[test]
fn a_text_is_edited_moved_to_the_back_and_deleted() {
    let mut b = Board::new_two_layer(40.0, 20.0).unwrap();
    let i = b.add_text(&new("X1", [5.0, 5.0]), &lim()).unwrap();
    let p = TextPatch {
        text: Some("Y2".into()),
        at: Some([7.0, 8.0]),
        size: Some(2.0),
        angle: Some(45.0),
        justify: Some([-1, 1]),
        ..TextPatch::default()
    };
    b.edit_text(i, &p, &lim()).unwrap();
    let t = &b.texts[i];
    assert_eq!((t.text.as_str(), t.at, t.size, t.angle, t.justify), ("Y2", [7.0, 8.0], 2.0, 45.0, [-1, 1]));
    assert_eq!(t.thickness, 0.15);
    // To the back: the mirroring turns with it.
    b.edit_text(i, &TextPatch { layer: Some("B.SilkS".into()), ..TextPatch::default() }, &lim()).unwrap();
    assert!(b.texts[i].mirrored && b.texts[i].layer == "B.SilkS");
    b.edit_text(i, &TextPatch { mirrored: Some(false), ..TextPatch::default() }, &lim()).unwrap();
    assert!(!b.texts[i].mirrored);
    assert!(b.edit_text(9, &TextPatch::default(), &lim()).is_err());
    assert_eq!(b.delete_texts(&[i], &lim()).unwrap(), 1);
    assert!(b.texts.is_empty());
}

#[test]
fn footprint_texts_edit_in_the_footprints_frame() {
    let mut b = Board::read(WITH_FP, &lim()).unwrap();
    let note = b.texts.iter().position(|t| t.text == "note").unwrap();
    // Reference and value keep their words; the user text changes.
    let r = b.texts.iter().position(|t| t.text == "R1").unwrap();
    assert!(b.edit_text(r, &TextPatch { text: Some("R9".into()), ..TextPatch::default() }, &lim()).is_err());
    assert!(b.delete_texts(&[note], &lim()).is_err());
    // The footprint is at (10, 10) turned 90°: a board place maps back to its own frame.
    let at = b.texts[note].at;
    b.edit_text(note, &TextPatch { text: Some("changed".into()), at: Some(at), ..TextPatch::default() }, &lim())
        .unwrap();
    assert_eq!(b.texts[note].text, "changed");
    assert!((b.texts[note].at[0] - at[0]).abs() < 1e-9 && (b.texts[note].at[1] - at[1]).abs() < 1e-9);
    b.edit_text(note, &TextPatch { at: Some([12.0, 13.0]), ..TextPatch::default() }, &lim()).unwrap();
    assert!((b.texts[note].at[0] - 12.0).abs() < 1e-9 && (b.texts[note].at[1] - 13.0).abs() < 1e-9);
    b.edit_text(r, &TextPatch { size: Some(1.5), ..TextPatch::default() }, &lim()).unwrap();
    assert_eq!(b.texts[r].size, 1.5);
    let again = Board::read(&b.write(), &lim()).unwrap();
    assert_eq!(again.texts, b.texts);
}

#[test]
fn flipping_a_footprint_mirrors_its_texts() {
    let mut b = Board::read(WITH_FP, &lim()).unwrap();
    assert!(b.texts.iter().all(|t| !t.mirrored));
    b.flip_footprint(0, &lim()).unwrap();
    let fp: Vec<_> = b.texts.iter().filter(|t| t.footprint == Some(0)).collect();
    assert_eq!(fp.len(), 3);
    assert!(fp.iter().all(|t| t.mirrored), "{fp:?}");
    assert!(fp.iter().any(|t| t.layer == "B.SilkS"));
    b.flip_footprint(0, &lim()).unwrap();
    assert!(b.texts.iter().all(|t| !t.mirrored));
}

#[test]
fn mirrored_strokes_read_from_the_other_side() {
    let mut b = Board::new_two_layer(40.0, 20.0).unwrap();
    let i = b.add_text(&new("AB", [10.0, 10.0]), &lim()).unwrap();
    let front = b.texts[i].clone();
    let back = crate::board::Text { mirrored: true, ..front.clone() };
    let f = font::strokes(&front, "AB");
    let m = font::strokes(&back, "AB");
    assert_eq!(f.len(), m.len());
    // The same shape reflected about the text's vertical axis through its place.
    for (a, c) in f.iter().flatten().zip(m.iter().flatten()) {
        assert!((a[0] - 10.0 + (c[0] - 10.0)).abs() < 1e-9 && (a[1] - c[1]).abs() < 1e-9);
    }
}
