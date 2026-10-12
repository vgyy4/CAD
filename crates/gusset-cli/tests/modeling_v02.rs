//! Shell, mirror and pattern features with the real OCCT kernel: exact volumes, face names
//! and the registry commands. Mock-kernel coverage lives in gusset-eval / gusset-commands.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{ErrorCode, Evaluation, Evaluator, FeatureStatus};
use std::sync::Arc;

const BOX: &str = "gusset 1
units mm

param n_bolts = 6

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)
";

const PEG: &str = "gusset 1
units mm

param n_bolts = 6

sketch Sketch1 (plane: XY, next_id: 3) {
  p1 = point(30, 0)
  c2 = circle(p1, 5)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)
";

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn eval(base: &str, extra: &str) -> (Evaluator, Evaluation) {
    let doc = gusset_doc::Document::from_text(&format!("{base}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

fn names(r: &Evaluation, body: &str) -> Vec<String> {
    r.body(body).unwrap().names.faces.iter().map(ToString::to_string).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn shell_open_box() {
    let (ev, r) = eval(BOX, "shell Shell1 (faces: [Extrude1/cap(end)], thickness: 2 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // 40x20x10 minus the 36x16x8 cavity.
    assert!(near(volume(&ev, &r, "Extrude1"), 3392.0), "{}", volume(&ev, &r, "Extrude1"));
    let n = names(&r, "Extrude1");
    assert_eq!(n.len(), 11, "{n:?}");
    for f in ["Extrude1/cap(start)", "Extrude1/side(Sketch1/l6)", "Extrude1/cap(end)"] {
        assert!(n.contains(&f.to_string()), "{f} in {n:?}");
    }
    for f in ["Shell1/inner(Extrude1/cap(start))", "Shell1/inner(Extrude1/side(Sketch1/l8))"] {
        assert!(n.contains(&f.to_string()), "{f} in {n:?}");
    }
    assert!(!n.contains(&"Shell1/inner(Extrude1/cap(end))".to_string()), "the removed face has no inner wall");
    let mut u = n.clone();
    u.sort();
    u.dedup();
    assert_eq!(u.len(), 11);
    // The rim (named after the removed face) is still a valid sketch plane.
    let (_, r) = eval(
        BOX,
        "shell Shell1 (faces: [Extrude1/cap(end)], thickness: 2 mm)
sketch Sketch2 (plane: Extrude1/cap(end), next_id: 2) {
  p1 = point(1, 1)
}",
    );
    assert_eq!(r.status("Sketch2"), Some(&FeatureStatus::Ok));
    // A fillet on an inner edge, named through the inner faces, works.
    let (ev, r) = eval(
        BOX,
        "shell Shell1 (faces: [Extrude1/cap(end)], thickness: 2 mm)
fillet Fillet1 (edges: [edge(Shell1/inner(Extrude1/cap(start)), Shell1/inner(Extrude1/side(Sketch1/l6)))], radius: 1 mm)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(volume(&ev, &r, "Extrude1") > 3392.0);
}

#[test]
fn shell_outward_and_too_thick() {
    let (ev, r) = eval(BOX, "shell Shell1 (faces: [Extrude1/cap(end)], thickness: -2 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 44.0 * 24.0 * 12.0 - 40.0 * 20.0 * 10.0));
    let (_, r) = eval(BOX, "shell Shell1 (faces: [Extrude1/cap(end)], thickness: 12 mm)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::KernelFailure);
    assert!(e.message.contains("too thick"), "{e}");
    assert!(e.hint.is_some());
}

#[test]
fn mirror_new_body_and_merged() {
    let (ev, r) = eval(BOX, "mirror Mirror1 (body: Extrude1, plane: YZ)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Mirror1"), 8000.0));
    let bb = ev.kernel().bbox(&r.body("Mirror1").unwrap().shape).unwrap();
    assert!(near(bb.min[0], -40.0) && bb.max[0].abs() < 1e-6);
    assert!(names(&r, "Mirror1").contains(&"Mirror1/mirror(Extrude1/side(Sketch1/l6))".to_string()));
    // Merged across the face x = 40: one 80x20x10 block, coplanar faces united.
    let (ev, r) = eval(BOX, "mirror Mirror1 (plane: Extrude1/side(Sketch1/l6), merge: true)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1);
    assert!(near(volume(&ev, &r, "Extrude1"), 16000.0), "{}", volume(&ev, &r, "Extrude1"));
    let n = names(&r, "Extrude1");
    assert_eq!(n.len(), 6, "{n:?}");
    // The united faces keep the source's names.
    assert!(n.contains(&"Extrude1/cap(end)".to_string()));
    assert!(n.contains(&"Extrude1/side(Sketch1/l5)".to_string()));
}

#[test]
fn linear_pattern_volumes() {
    // Four separate blocks.
    let (ev, r) = eval(BOX, "pattern_linear Pattern1 (direction: X, count: 4, spacing: 60 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 32000.0));
    assert_eq!(ev.kernel().topology(&r.body("Extrude1").unwrap().shape).unwrap().solids, 4);
    assert!(names(&r, "Extrude1").contains(&"Pattern1/instance(3, Extrude1/cap(end))".to_string()));
    // Touching copies fuse into one 120x20x10 block.
    let (ev, r) = eval(BOX, "pattern_linear Pattern1 (direction: X, count: 3, spacing: 40 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 24000.0));
    assert_eq!(names(&r, "Extrude1").len(), 6);
    // Overlapping copies: 40 + 2 x 30 = 100 long.
    let (ev, r) = eval(BOX, "pattern_linear Pattern1 (direction: X, count: 3, spacing: 30 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 20000.0));
    // Vector direction, negative spacing and a count expression.
    let (ev, r) = eval(BOX, "pattern_linear Pattern1 (direction: [0, 1, 0], count: n_bolts / 2, spacing: -50 mm)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let bb = ev.kernel().bbox(&r.body("Extrude1").unwrap().shape).unwrap();
    assert!(near(bb.min[1], -100.0) && near(bb.max[1], 20.0), "{bb:?}");
}

#[test]
fn circular_pattern_bolt_circle() {
    let peg = std::f64::consts::PI * 25.0 * 10.0;
    let (ev, r) = eval(PEG, "pattern_circular Pattern1 (axis: Z, count: n_bolts, angle: 360 deg)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 6.0 * peg), "{}", volume(&ev, &r, "Extrude1"));
    assert_eq!(ev.kernel().topology(&r.body("Extrude1").unwrap().shape).unwrap().solids, 6);
    let n = names(&r, "Extrude1");
    assert!(n.contains(&"Pattern1/instance(5, Extrude1/side(Sketch1/c2))".to_string()), "{n:?}");
    // Partial angle: 3 pegs from 0 to 90 degrees; the last is at (0, 30).
    let (ev, r) = eval(PEG, "pattern_circular Pattern1 (axis: Z, count: 3, angle: 90 deg)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 3.0 * peg));
    let bb = ev.kernel().bbox(&r.body("Extrude1").unwrap().shape).unwrap();
    assert!(near(bb.max[1], 35.0) && near(bb.min[0], -5.0), "{bb:?}");
    // An axis through another point: the copy lands at (0, 0).
    let (ev, r) =
        eval(PEG, "pattern_circular Pattern1 (axis: Z, through: [15 mm, 0 mm, 0 mm], count: 2, angle: 180 deg)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 2.0 * peg));
    let bb = ev.kernel().bbox(&r.body("Extrude1").unwrap().shape).unwrap();
    assert!(near(bb.min[0], -5.0) && near(bb.max[0], 35.0), "{bb:?}");
}

#[test]
fn parametric_count_rebuilds_incrementally() {
    let doc = gusset_doc::Document::from_text(&format!(
        "{PEG}\npattern_circular Pattern1 (axis: Z, count: n_bolts, angle: 360 deg)\n"
    ))
    .unwrap();
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc, None, &());
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let mut d2 = doc.clone();
    d2.params[0].expr = gusset_units::expr::parse("4").unwrap();
    let r2 = ev.evaluate(&d2, None, &());
    assert_eq!(r2.recomputed, 1);
    assert_eq!(names(&r2, "Extrude1").len(), 4 * 3);
}

#[test]
fn commands_drive_the_real_kernel() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, kernel(), &CmdCtx::user(0));
    let u = CmdCtx::user(1);
    let mut run =
        |id: &str, args: serde_json::Value| execute(&mut s, &u, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"));
    run("sketch.create", json!({"plane": "XY"}));
    run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20, "width_expr": "40 mm", "height_expr": "20 mm"}),
    );
    run("feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    let r = run("feature.shell", json!({"faces": ["Extrude1/cap(end)"], "thickness": "2 mm"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run("query.body", json!({}));
    assert!(near(b["volume_mm3"].as_f64().unwrap(), 3392.0), "{b}");
    let r = run("feature.pattern_linear", json!({"direction": "Y", "count": "3", "spacing": "30 mm"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run("query.body", json!({}));
    // Pitch 30 mm in Y with 20 mm bodies: three separate shells.
    assert!(near(b["volume_mm3"].as_f64().unwrap(), 3.0 * 3392.0), "{b}");
    let r = run("feature.mirror", json!({"plane": "XY", "merge": true}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
}
