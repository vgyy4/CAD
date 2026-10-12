//! Datum planes, body move/copy and combine with the real OCCT kernel: exact volumes,
//! persistent face names across parameter edits and the registry commands. Mock-kernel
//! coverage lives in gusset-eval / gusset-commands.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use gusset_eval::{ErrorCode, Evaluation, Evaluator, FeatureStatus};
use std::sync::Arc;

const BOX: &str = "gusset 1
units mm

param n = 3
param pitch = 12 mm

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

/// A pin (r = 3) on the top of the box, as a body of its own, patterned `n` times along X.
const PIN: &str = "
sketch Sketch2 (plane: Extrude1/cap(end), next_id: 3) {
  p1 = point(8, 10)
  c2 = circle(p1, 3)
}

extrude Extrude2 (profile: Sketch2, distance: 5 mm, op: new)

pattern_linear Pattern1 (body: Extrude2, direction: X, count: n, spacing: pitch)
";

/// A drill (r = 3, from 2 mm above the top to 2 mm below the bottom), patterned the same way.
const DRILL: &str = "
plane Plane1 (base: Extrude1/cap(end), offset: 2 mm)

sketch Sketch2 (plane: Plane1, next_id: 3) {
  p1 = point(8, 10)
  c2 = circle(p1, 3)
}

extrude Extrude2 (profile: Sketch2, distance: -14 mm, op: new)

pattern_linear Pattern1 (body: Extrude2, direction: X, count: n, spacing: pitch)
";

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn doc(base: &str, extra: &str) -> gusset_doc::Document {
    gusset_doc::Document::from_text(&format!("{base}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"))
}

fn eval(base: &str, extra: &str) -> (Evaluator, Evaluation) {
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc(base, extra), None, &());
    (ev, r)
}

fn volume(ev: &Evaluator, r: &Evaluation, body: &str) -> f64 {
    ev.kernel().mass_properties(&r.body(body).unwrap().shape).unwrap().volume
}

fn bbox(ev: &Evaluator, r: &Evaluation, body: &str) -> gusset_kernel::BBox {
    ev.kernel().bbox(&r.body(body).unwrap().shape).unwrap()
}

fn names(r: &Evaluation, body: &str) -> Vec<String> {
    r.body(body).unwrap().names.faces.iter().map(ToString::to_string).collect()
}

fn centroid(r: &Evaluation, body: &str, face: &str) -> [f64; 3] {
    let b = r.body(body).unwrap();
    let i = b
        .names
        .faces
        .iter()
        .position(|f| f.to_string() == face)
        .unwrap_or_else(|| panic!("{face} in {:?}", names(r, body)));
    b.topology.faces[i].centroid
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

fn frame(r: &Evaluation, plane: &str) -> gusset_kernel::Frame {
    r.planes.iter().find(|(n, _)| n == plane).map(|(_, f)| *f).unwrap_or_else(|| panic!("no plane {plane}"))
}

#[test]
fn datum_planes_place_sketches_and_mirrors() {
    // A sketch on an angled plane: XY turned 45 degrees about X, 10 x 10 extruded by 5.
    let (ev, r) = eval(
        BOX,
        "plane Plane1 (base: XY, axis: X, angle: 45 deg)
sketch Sketch2 (plane: Plane1, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 10)
  p4 = point(0, 10)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}
extrude Extrude2 (profile: Sketch2, distance: 5 mm, op: new)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude2"), 500.0));
    let h = std::f64::consts::FRAC_1_SQRT_2;
    assert!(close(frame(&r, "Plane1").normal(), [0.0, -h, h]), "{:?}", frame(&r, "Plane1"));
    // The slab's far corner: the sketch y axis (0, h, h) runs 10 mm, the normal 5 mm more.
    let bb = bbox(&ev, &r, "Extrude2");
    assert!(near(bb.max[2], 10.0 * h + 5.0 * h) && near(bb.min[1], -5.0 * h), "{bb:?}");

    // A plane through a straight edge of the box: the top face turned 90 degrees about its
    // front edge stands up through that edge, facing -Y.
    let (_, r) = eval(
        BOX,
        "plane Plane1 (base: Extrude1/cap(end), axis: edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5)), angle: 90 deg)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(close(frame(&r, "Plane1").normal(), [0.0, -1.0, 0.0]), "{:?}", frame(&r, "Plane1"));
    assert!(close(frame(&r, "Plane1").origin, [0.0, 0.0, 10.0]), "{:?}", frame(&r, "Plane1"));

    // Mirror across a plane offset from a side face, merged: one 80 x 20 x 10 block.
    let (ev, r) = eval(
        BOX,
        "plane Plane1 (base: Extrude1/side(Sketch1/l6), offset: 0 mm)
mirror Mirror1 (plane: Plane1, merge: true)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 16000.0), "{}", volume(&ev, &r, "Extrude1"));
    // Across a datum plane in the middle of the gap: a separate mirrored body.
    let (ev, r) = eval(BOX, "plane Mid (base: YZ, offset: 60 mm)\nmirror Mirror1 (plane: Mid)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    let bb = bbox(&ev, &r, "Mirror1");
    assert!(near(bb.min[0], 80.0) && near(bb.max[0], 120.0), "{bb:?}");
}

#[test]
fn datum_plane_errors_with_the_real_kernel() {
    let cyl = "gusset 1
units mm

sketch Sketch1 (plane: XY, next_id: 3) {
  p1 = point(0, 0)
  c2 = circle(p1, 5)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)
";
    // A curved edge is no axis.
    let (_, r) =
        eval(cyl, "plane P (base: XY, axis: edge(Extrude1/cap(end), Extrude1/side(Sketch1/c2)), angle: 10 deg)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::InvalidReference);
    assert!(e.message.contains("not a straight edge") && e.hint.is_some(), "{e}");
    // A cylinder is no plane to offset from.
    let (_, r) = eval(cyl, "plane P (base: Extrude1/side(Sketch1/c2), offset: 1 mm)");
    let e = r.errors()[0].clone();
    assert_eq!(e.code, ErrorCode::InvalidReference);
    assert!(e.message.contains("not planar"), "{e}");
    // A sketch on a failed plane is blocked, not an error of its own.
    let (_, r) =
        eval(cyl, "plane P (base: Extrude1/side(Sketch1/c2), offset: 1 mm)\nsketch S2 (plane: P, next_id: 1) {\n}");
    assert!(matches!(r.status("S2"), Some(FeatureStatus::Blocked { by }) if by == "P"));
}

#[test]
fn moves_keep_names_and_copies_are_new_bodies() {
    // Quarter turn about a vertical axis through the plate's centre.
    let (ev, r) = eval(BOX, "move Move1 (axis: Z, angle: 90 deg, through: [20 mm, 10 mm, 0 mm])");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0));
    let bb = bbox(&ev, &r, "Extrude1");
    assert!(close(bb.min, [10.0, -10.0, 0.0]) && close(bb.max, [30.0, 30.0, 10.0]), "{bb:?}");
    let (_, base) = eval(BOX, "");
    assert_eq!(names(&r, "Extrude1"), names(&base, "Extrude1"), "a move keeps every name");
    // The side that faced +X (l6, at x = 40) now faces +Y at y = 30.
    let c = centroid(&r, "Extrude1", "Extrude1/side(Sketch1/l6)");
    assert!(close(c, [20.0, 30.0, 5.0]), "{c:?}");

    // A copy: rotated, then shifted. The original stays.
    let (ev, r) = eval(BOX, "move Move1 (translate: [100 mm, 0 mm, 5 mm], axis: X, angle: 180 deg, copy: true)");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 2);
    assert!(near(volume(&ev, &r, "Move1"), 8000.0) && near(volume(&ev, &r, "Extrude1"), 8000.0));
    // Turned over about X (y -> -y, z -> -z), then moved: y -20..0, z -5..5 at x 100..140.
    let bb = bbox(&ev, &r, "Move1");
    assert!(close(bb.min, [100.0, -20.0, -5.0]) && close(bb.max, [140.0, 0.0, 5.0]), "{bb:?}");
    let n = names(&r, "Move1");
    assert!(n.contains(&"Move1/copy(Extrude1/cap(end))".to_string()), "{n:?}");
    // The copy's old top face now faces down.
    let b = r.body("Move1").unwrap();
    let i = b.names.faces.iter().position(|f| f.to_string() == "Move1/copy(Extrude1/cap(end))").unwrap();
    assert!(close(b.topology.faces[i].normal, [0.0, 0.0, -1.0]), "{:?}", b.topology.faces[i]);

    // A sketch on a face of a moved body follows it; so does a pocket cut through it.
    let (ev, r) = eval(
        BOX,
        "move Move1 (translate: [0 mm, 0 mm, 20 mm])
sketch Sketch2 (plane: Extrude1/cap(end), next_id: 3) {
  p1 = point(20, 10)
  c2 = circle(p1, 4)
}
extrude Extrude2 (profile: Sketch2, distance: -3 mm, op: cut, body: Extrude1)",
    );
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0 - 3.0 * std::f64::consts::PI * 16.0));
    assert!(near(bbox(&ev, &r, "Extrude1").max[2], 30.0));
}

#[test]
fn a_patterned_boss_is_combined_into_the_base() {
    let boss = std::f64::consts::PI * 9.0 * 5.0;
    let (ev, r) = eval(BOX, &format!("{PIN}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: add)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1, "the boss body is consumed");
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0 + 3.0 * boss), "{}", volume(&ev, &r, "Extrude1"));
    // The pins stand on the plate, so the result is one solid.
    assert_eq!(ev.kernel().topology(&r.body("Extrude1").unwrap().shape).unwrap().solids, 1);
    let n = names(&r, "Extrude1");
    for f in [
        "Extrude1/cap(start)",
        "Extrude1/cap(end)",
        "Extrude2/side(Sketch2/c2)",
        "Extrude2/cap(end)",
        "Pattern1/instance(1, Extrude2/side(Sketch2/c2))",
        "Pattern1/instance(2, Extrude2/side(Sketch2/c2))",
        "Pattern1/instance(2, Extrude2/cap(end))",
    ] {
        assert!(n.contains(&f.to_string()), "{f} in {n:?}");
    }
    let mut uniq = n.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), n.len(), "face names are unique: {n:?}");
    // Default target: the latest body that is not a tool.
    let (ev2, r2) = eval(BOX, &format!("{PIN}\ncombine Combine1 (tools: [Extrude2])"));
    assert!(near(volume(&ev2, &r2, "Extrude1"), 8000.0 + 3.0 * boss));
    assert_eq!(names(&r2, "Extrude1"), n);
    // A fillet at the foot of the second pin, named through the combined body, works.
    let foot = "edge(Extrude1/cap(end), Pattern1/instance(1, Extrude2/side(Sketch2/c2)))";
    let (ev3, r3) = eval(
        BOX,
        &format!("{PIN}\ncombine Combine1 (tools: [Extrude2])\nfillet Fillet1 (edges: [{foot}], radius: 1 mm)"),
    );
    assert!(r3.errors().is_empty(), "{:?}", r3.errors());
    assert!(volume(&ev3, &r3, "Extrude1") > 8000.0 + 3.0 * boss);
}

#[test]
fn a_patterned_drill_is_cut_from_the_base() {
    let hole = std::f64::consts::PI * 9.0 * 10.0;
    let (ev, r) = eval(BOX, &format!("{DRILL}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: cut)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert_eq!(r.bodies.len(), 1);
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0 - 3.0 * hole), "{}", volume(&ev, &r, "Extrude1"));
    let n = names(&r, "Extrude1");
    for f in [
        "Extrude2/side(Sketch2/c2)",
        "Pattern1/instance(1, Extrude2/side(Sketch2/c2))",
        "Pattern1/instance(2, Extrude2/side(Sketch2/c2))",
    ] {
        assert!(n.contains(&f.to_string()), "{f} in {n:?}");
    }
    // The bore walls are named after the drill; the first bore is at x = 8.
    let c = centroid(&r, "Extrude1", "Extrude2/side(Sketch2/c2)");
    assert!((c[0] - 8.0).abs() < 1e-6 && (c[1] - 10.0).abs() < 1e-6, "{c:?}");
    // Intersect keeps only the drilled-out material.
    let (ev, r) = eval(BOX, &format!("{DRILL}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: intersect)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 3.0 * hole), "{}", volume(&ev, &r, "Extrude1"));
}

#[test]
fn combined_names_survive_parameter_edits() {
    for (extra, op_name) in [(PIN, "add"), (DRILL, "cut")] {
        let d = doc(BOX, &format!("{extra}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: {op_name})"));
        let mut ev = Evaluator::new(kernel());
        let r1 = ev.evaluate(&d, None, &());
        assert!(r1.errors().is_empty(), "{op_name}: {:?}", r1.errors());
        let face = "Pattern1/instance(2, Extrude2/side(Sketch2/c2))";
        let c1 = centroid(&r1, "Extrude1", face);
        assert!((c1[0] - (8.0 + 24.0)).abs() < 1e-6, "{c1:?}");
        // More instances and a larger pitch: the same name keeps pointing at the same
        // instance, now further along.
        let mut d2 = d.clone();
        d2.params[0].expr = gusset_units::expr::parse("4").unwrap();
        d2.params[1].expr = gusset_units::expr::parse("9 mm").unwrap();
        let r2 = ev.evaluate(&d2, None, &());
        assert!(r2.errors().is_empty(), "{op_name}: {:?}", r2.errors());
        let c2 = centroid(&r2, "Extrude1", face);
        assert!((c2[0] - (8.0 + 18.0)).abs() < 1e-6, "{c2:?}");
        assert!(names(&r2, "Extrude1").contains(&"Pattern1/instance(3, Extrude2/side(Sketch2/c2))".to_string()));
        // Only the pattern and what follows it were recomputed (plus the combine).
        assert!(r2.recomputed <= 2, "{op_name}: recomputed {}", r2.recomputed);
        // A base height change moves the pins/bores; the names stay.
        let mut d3 = d.clone();
        if let gusset_doc::FeatureKind::Extrude(e) = &mut d3.features[2].kind {
            e.distance = gusset_units::expr::parse("14 mm").unwrap();
        }
        let r3 = ev.evaluate(&d3, None, &());
        // Everything after the sketch on the top face follows the new height.
        assert!(r3.errors().is_empty(), "{op_name}: {:?}", r3.errors());
        let c3 = centroid(&r3, "Extrude1", face);
        assert!((c3[0] - (8.0 + 24.0)).abs() < 1e-6, "{c3:?}");
    }
}

#[test]
fn combine_cut_intersect_and_failures() {
    let two = "
sketch Sketch2 (plane: XY, next_id: 9) {
  p1 = point(30, 5)
  p2 = point(60, 5)
  p3 = point(60, 15)
  p4 = point(30, 15)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude2 (profile: Sketch2, distance: 10 mm, op: new)
";
    // The overlap is 10 x 10 x 10; the second block is 30 x 10 x 10.
    let (ev, r) = eval(BOX, &format!("{two}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: cut)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0 - 1000.0));
    let (ev, r) = eval(BOX, &format!("{two}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: intersect)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 1000.0));
    let (ev, r) = eval(BOX, &format!("{two}\ncombine Combine1 (body: Extrude1, tools: [Extrude2])"));
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0 + 2000.0));
    // Faces of the second block keep their names in the union; its side inside the box is gone.
    let n = names(&r, "Extrude1");
    assert!(n.contains(&"Extrude2/side(Sketch2/l6)".to_string()), "{n:?}");
    assert!(n.contains(&"Extrude1/side(Sketch1/l5)".to_string()));

    // Tools that do not overlap: a cut changes nothing, an intersect leaves nothing, which is
    // a clean error with a hint (and the base stays as it was).
    let far = two
        .replace("30, 5", "300, 5")
        .replace("60, 5", "330, 5")
        .replace("60, 15", "330, 15")
        .replace("30, 15", "300, 15");
    let (ev, r) = eval(BOX, &format!("{far}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: cut)"));
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0));
    let (ev, r) = eval(BOX, &format!("{far}\ncombine Combine1 (body: Extrude1, tools: [Extrude2], op: intersect)"));
    let e = r.errors()[0].clone();
    assert_eq!(e.feature, "Combine1");
    assert!(matches!(e.code, ErrorCode::EmptyResult | ErrorCode::KernelFailure), "{e}");
    assert!(e.hint.is_some(), "{e}");
    assert_eq!(r.bodies.len(), 2, "a failed combine consumes nothing");
    assert!(near(volume(&ev, &r, "Extrude1"), 8000.0));
}

#[test]
fn commands_drive_the_real_kernel() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let d = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&d, kernel(), &CmdCtx::user(0));
    let u = CmdCtx::user(1);
    let mut run =
        |id: &str, args: serde_json::Value| execute(&mut s, &u, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"));
    run("param.set", json!({"name": "pitch", "expr": "12 mm"}));
    run("sketch.create", json!({"plane": "XY"}));
    run(
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20, "width_expr": "40 mm", "height_expr": "20 mm"}),
    );
    run("feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm", "body_name": "Base"}));
    // A boss on a datum plane above the top, patterned, then united with the base.
    let r = run("feature.plane", json!({"base": "Extrude1/cap(end)", "offset": "0 mm", "name": "Top"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run("sketch.create", json!({"plane": "Top"}));
    let sk = r["sketch"].as_str().unwrap().to_string();
    run("sketch.add_circle", json!({"sketch": sk, "cx": 8, "cy": 10, "radius": 3}));
    let r = run("feature.extrude", json!({"sketch": sk, "distance": "5 mm", "body_name": "Boss"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run("feature.pattern_linear", json!({"direction": "X", "count": "3", "spacing": "pitch", "body": "Boss"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run("feature.combine", json!({"tools": ["Boss"], "body": "Base"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert!(r["body_changes"].to_string().contains("removed"), "{r}");
    let b = run("query.body", json!({"body": "Base"}));
    let pin = std::f64::consts::PI * 45.0;
    assert!(near(b["volume_mm3"].as_f64().unwrap(), 8000.0 + 3.0 * pin), "{b}");
    // Move a copy of the whole part, combine it back as a cut (the copy overlaps nothing in
    // the base but its own footprint is irrelevant to the volume check).
    let r = run("feature.move_body", json!({"body": "Base", "translate": ["0 mm", "30 mm", "0 mm"], "copy": true}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let m = run("query.model", json!({}));
    assert_eq!(m["bodies"].as_array().unwrap().len(), 2, "{m}");
    assert_eq!(m["datum_planes"][0]["name"], "Top");
    // Parameter edits flow through the whole chain.
    run("param.set", json!({"name": "pitch", "expr": "14 mm"}));
    let b = run("query.body", json!({"body": "Copy1", "names": true}));
    assert!(b.to_string().contains("Copy1/copy(Pattern1/instance(2, Extrude2/side(Sketch2/c2)))"), "{b}");
    assert!(near(b["volume_mm3"].as_f64().unwrap(), 8000.0 + 3.0 * pin), "{b}");
}
