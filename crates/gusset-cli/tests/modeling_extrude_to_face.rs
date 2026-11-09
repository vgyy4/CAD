//! Extrudes that end on a face (`to:`, ADR 0004 amendment) with the real OCCT kernel: blind
//! to a parallel plane, with offsets, add and cut (pockets) up to a face, reverse, the names
//! of the new faces, following a moved target, and the refusals.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

fn rect(name: &str, plane: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> String {
    format!(
        "sketch {name} (plane: {plane}, next_id: 9) {{
  p1 = point({x0}, {y0})
  p2 = point({x1}, {y0})
  p3 = point({x1}, {y1})
  p4 = point({x0}, {y1})
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}}

"
    )
}

/// A plate (z 0..10) and, 30 mm above the XY plane, a ceiling slab (z 30..40).
fn base(ceiling_z: f64) -> String {
    format!(
        "gusset 1\nunits mm\n\n{}extrude Plate (profile: Sk1, distance: 10 mm, op: new)\n\nplane Up (base: XY, offset: {ceiling_z} mm)\n\n{}extrude Ceiling (profile: Sk2, distance: 10 mm, op: new)\n\n{}",
        rect("Sk1", "XY", 0.0, 0.0, 40.0, 20.0),
        rect("Sk2", "Up", 0.0, 0.0, 40.0, 20.0),
        rect("Sk3", "XY", 5.0, 5.0, 15.0, 15.0),
    )
}

fn eval(text: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

fn post(extra: &str) -> String {
    format!("{}extrude Post (profile: Sk3, to: Ceiling/cap(start){extra}, op: new)\n", base(30.0))
}

#[test]
fn extrude_up_to_the_underside_of_a_slab() {
    let (ev, r) = eval(&post(""));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(volume(&ev, &r, "Post"), 3000.0));
    // The new faces are named like any extrude's: the end cap lies on the target plane.
    let b = r.body("Post").unwrap();
    let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
    assert!(names.contains(&"Post/cap(end)".to_string()), "{names:?}");
    assert!(names.contains(&"Post/side(Sk3/l5)".to_string()), "{names:?}");
    let i = b.names.faces.iter().position(|f| f.to_string() == "Post/cap(end)").unwrap();
    assert!((b.topology.faces[i].centroid[2] - 30.0).abs() < 1e-9);
    // Offsets: 2 mm into the slab, 5 mm short of it.
    for (off, v) in [(", offset: 2 mm", 3200.0), (", offset: -5 mm", 2500.0)] {
        let (ev, r) = eval(&post(off));
        assert!(r.errors().is_empty(), "{off}: {:?}", r.errors());
        assert!(close(volume(&ev, &r, "Post"), v), "{off}: {}", volume(&ev, &r, "Post"));
    }
}

#[test]
fn the_end_follows_the_target_face_and_its_names_are_usable() {
    // Move the slab: the post grows with it (the face is a persistent reference).
    let (ev, r) = eval(&post(""));
    assert!(r.errors().is_empty());
    let text = post("").replace("offset: 30 mm", "offset: 35 mm");
    let (ev2, r2) = eval(&text);
    assert!(r2.errors().is_empty(), "{:?}", r2.errors());
    assert!(close(volume(&ev, &r, "Post"), 3000.0) && close(volume(&ev2, &r2, "Post"), 3500.0));
    // A fillet on the end cap's edge builds.
    let filleted = format!("{}\nfillet F1 (edges: [edge(Post/cap(end), Post/side(Sk3/l5))], radius: 1 mm)\n", post(""));
    let (ev3, r3) = eval(&filleted);
    assert!(r3.errors().is_empty(), "{:?}", r3.errors());
    assert!(volume(&ev3, &r3, "Post") < 3000.0 - 0.1);
}

#[test]
fn add_and_pocket_up_to_a_face() {
    // Join the post to the plate: 8000 + 10 x 10 x 20 above it.
    let add = format!("{}extrude Post (profile: Sk3, to: Ceiling/cap(start), op: add, body: Plate)\n", base(30.0));
    let (ev, r) = eval(&add);
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(volume(&ev, &r, "Plate"), 10000.0), "{}", volume(&ev, &r, "Plate"));
    // A pocket from the plate's top down to its bottom face, and one stopping 3 mm short.
    for (extra, v) in [("", 7000.0), (", offset: -3 mm", 7300.0)] {
        let text = format!(
            "{}{}extrude Pocket (profile: Sk4, to: Plate/cap(start){extra}, reverse: true, op: cut, body: Plate)\n",
            base(30.0),
            rect("Sk4", "Plate/cap(end)", 5.0, 5.0, 15.0, 15.0)
        );
        let (ev, r) = eval(&text);
        assert!(r.errors().is_empty(), "{extra}: {:?}", r.errors());
        assert!(close(volume(&ev, &r, "Plate"), v), "{extra}: {}", volume(&ev, &r, "Plate"));
    }
}

#[test]
fn faces_that_cannot_end_the_sweep_are_refused_with_a_message() {
    let error = |text: String, feature: &str| {
        let (_, r) = eval(&text);
        r.errors().iter().find(|e| e.feature == feature).map(|e| e.message.clone()).unwrap_or_default()
    };
    // The plate's bottom lies in the sketch plane: not ahead.
    let m = error(format!("{}extrude Bad (profile: Sk3, to: Plate/cap(start), op: new)\n", base(30.0)), "Bad");
    assert!(m.contains("behind"), "{m}");
    // The slab does not cover a sketch that sticks out of it.
    let wide = base(30.0) + &rect("Sk5", "XY", 30.0, 5.0, 50.0, 15.0);
    let m = error(format!("{wide}extrude Bad (profile: Sk5, to: Ceiling/cap(start), op: new)\n"), "Bad");
    assert!(m.contains("cover"), "{m}");
    // A face that no longer exists is a missing reference with a suggestion.
    let (_, r) = eval(&format!("{}extrude Bad (profile: Sk3, to: Ceiling/side(Sk2/l99), op: new)\n", base(30.0)));
    let e = r.errors().into_iter().find(|e| e.feature == "Bad").unwrap();
    assert!(e.message.contains("no longer exists"), "{}", e.message);
}

#[test]
fn the_command_extrudes_up_to_a_face() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text(&base(30.0)).unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let r = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.up_to_face",
        json!({"sketch": "Sk3", "to": "Ceiling/cap(start)", "offset": "2 mm", "name": "Post"}),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(r["feature"], "Post");
    let b = execute(&mut s, &CmdCtx::user(1), "query.body", json!({"body": "Post"})).unwrap();
    assert!((b["volume_mm3"].as_f64().unwrap() - 3200.0).abs() < 1e-3, "{b}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("extrude Post (profile: Sk3, to: Ceiling/cap(start), offset: 2 mm, op: new)"), "{text}");
    // A bad face is an argument error.
    let e = execute(&mut s, &CmdCtx::user(1), "feature.up_to_face", json!({"sketch": "Sk3", "to": "nonsense"}));
    assert!(e.is_err());
}
