//! Holes with the real OCCT kernel (ADR 0019): removed volumes against analytic values for
//! every type, blind and through, presets against the table, positions, persistent face
//! names, parameter edits, other faces, and clean errors. Mock-kernel coverage (document,
//! text, commands) lives in the other crates.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use gusset_commands::{CmdCtx, Session, execute};
use gusset_doc::Document;
use gusset_eval::{ErrorCode, Evaluation, Evaluator, FeatureStatus};
use gusset_history::Actor;
use serde_json::{Value, json};
use std::f64::consts::PI;
use std::sync::Arc;

/// A 60 x 40 x 20 plate.
const PLATE: &str = "gusset 1
units mm

param t = 20 mm

sketch S1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point(60, 40)
  p4 = point(0, 40)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude E1 (profile: S1, distance: t, op: new)
";
const PLATE_VOLUME: f64 = 60.0 * 40.0 * 20.0;

fn kernel() -> Arc<dyn gusset_kernel::Kernel> {
    Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")))
}

fn eval(extra: &str) -> (Evaluator, Evaluation) {
    let doc = Document::from_text(&format!("{PLATE}\n{extra}\n")).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn ok(r: &Evaluation) {
    assert!(r.errors().is_empty(), "{:?}", r.errors());
}

fn volume(ev: &Evaluator, r: &Evaluation) -> f64 {
    ev.kernel().mass_properties(&r.body("E1").unwrap().shape).unwrap().volume
}

/// Volume removed by the holes of `extra`.
fn removed(extra: &str) -> f64 {
    let (ev, r) = eval(extra);
    ok(&r);
    PLATE_VOLUME - volume(&ev, &r)
}

fn names(r: &Evaluation) -> Vec<String> {
    r.body("E1").unwrap().names.faces.iter().map(ToString::to_string).collect()
}

fn face(r: &Evaluation, name: &str) -> gusset_kernel::FaceInfo {
    let b = r.body("E1").unwrap();
    let i =
        b.names.faces.iter().position(|f| f.to_string() == name).unwrap_or_else(|| panic!("{name} in {:?}", names(r)));
    b.topology.faces[i].clone()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

fn cone(r: f64, point_angle: f64) -> f64 {
    PI * r * r * (r / (point_angle / 2.0).tan()) / 3.0
}

const A118: f64 = 118.0 * PI / 180.0;
const TOP: &str = "E1/cap(end)";

#[test]
fn simple_holes_remove_the_analytic_volume() {
    // Blind with the default 118 degree point, an explicit point, and a flat end.
    let r = 3.0;
    for (args, expected) in [
        ("depth: 10 mm", PI * r * r * 10.0 + cone(r, A118)),
        ("depth: 10 mm, point_angle: 90 deg", PI * r * r * 10.0 + cone(r, PI / 2.0)),
        ("depth: 10 mm, point_angle: 180 deg", PI * r * r * 10.0),
        ("depth: 10 mm, point_angle: 140 deg", PI * r * r * 10.0 + cone(r, 140.0 * PI / 180.0)),
        ("through: true", PI * r * r * 20.0),
    ] {
        let v = removed(&format!("hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], type: simple, diameter: 6 mm, {args})"));
        assert!(close(v, expected, 1e-6), "{args}: {v} vs {expected}");
    }
    // A blind hole whose point breaks through the far side is a through hole with a cone cut.
    let v = removed(&format!(
        "hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], diameter: 6 mm, depth: 19 mm, point_angle: 180 deg)"
    ));
    assert!(close(v, PI * 9.0 * 19.0, 1e-6), "{v}");
}

#[test]
fn counterbores_countersinks_and_tapped_holes() {
    let (r, rc) = (3.3, 5.5);
    // Counterbore through and blind.
    let v = removed(&format!(
        "hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], type: counterbore, diameter: 6.6 mm, through: true, cbore_diameter: 11 mm, cbore_depth: 6.5 mm)"
    ));
    assert!(close(v, PI * rc * rc * 6.5 + PI * r * r * 13.5, 1e-6), "{v}");
    let v = removed(&format!(
        "hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], type: counterbore, diameter: 6.6 mm, depth: 12 mm, cbore_diameter: 11 mm, cbore_depth: 6.5 mm)"
    ));
    assert!(close(v, PI * rc * rc * 6.5 + PI * r * r * 5.5 + cone(r, A118), 1e-6), "{v}");
    // Countersink through and blind, 90 and 82 degrees.
    for (angle, deg) in [(PI / 2.0, "90 deg"), (82.0 * PI / 180.0, "82 deg")] {
        let rs = 6.72;
        let h = (rs - r) / (angle / 2.0).tan();
        let frustum = PI * h / 3.0 * (rs * rs + rs * r + r * r);
        let v = removed(&format!(
            "hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], type: countersink, diameter: 6.6 mm, through: true, csink_diameter: 13.44 mm, csink_angle: {deg})"
        ));
        assert!(close(v, frustum + PI * r * r * (20.0 - h), 1e-6), "{deg}: {v}");
        let v = removed(&format!(
            "hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], type: countersink, diameter: 6.6 mm, depth: 12 mm, csink_diameter: 13.44 mm, csink_angle: {deg})"
        ));
        assert!(close(v, frustum + PI * r * r * (12.0 - h) + cone(r, A118), 1e-6), "{deg} blind: {v}");
    }
    // Tapped: modelled at the tap-drill diameter; the thread is only recorded.
    let (ev, res) = eval(&format!(
        "hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], type: tapped, diameter: 5 mm, depth: 12 mm, thread: \"M6\", pitch: 1 mm, thread_depth: 9 mm)"
    ));
    ok(&res);
    let v = PLATE_VOLUME - volume(&ev, &res);
    assert!(close(v, PI * 2.5 * 2.5 * 12.0 + cone(2.5, A118), 1e-6), "{v}");
    let holes = &res.body("E1").unwrap().holes;
    assert_eq!(holes.len(), 1);
    let t = holes[0].thread.as_ref().unwrap();
    assert_eq!((t.designation.as_str(), t.pitch, t.depth), ("M6", 1.0, Some(9.0)));
    assert_eq!(holes[0].wall.to_string(), "H1/wall(e1)");
}

#[test]
fn positions_sites_and_other_faces() {
    // Sketch sites: isolated points and circle centres, not line ends or construction points.
    let (ev, r) = eval(&format!(
        "sketch Marks (plane: {TOP}, next_id: 12) {{
  p1 = point(10, 10)
  p2 = point(50, 10)
  p3 = point(30, 30) construction
  p4 = point(5, 35)
  p5 = point(15, 35)
  l6 = line(p4, p5)
  p7 = point(30, 20)
  c8 = circle(p7, 1)
}}

hole H1 (face: {TOP}, points: Marks, type: simple, diameter: 4 mm, through: true)"
    ));
    ok(&r);
    assert!(close(PLATE_VOLUME - volume(&ev, &r), 3.0 * PI * 4.0 * 20.0, 1e-6));
    for (id, x, y) in [("p1", 10.0, 10.0), ("p2", 50.0, 10.0), ("p7", 30.0, 20.0)] {
        let w = face(&r, &format!("H1/wall({id})"));
        assert!(
            (w.centroid[0] - x).abs() < 1e-6 && (w.centroid[1] - y).abs() < 1e-6 && (w.centroid[2] - 10.0).abs() < 1e-6,
            "{id}: {:?}",
            w.centroid
        );
    }
    let ids: Vec<&str> = r.body("E1").unwrap().holes.iter().map(|h| h.at.as_str()).collect();
    assert_eq!(ids, ["p1", "p2", "p7"]);
    // A sketch on a parallel plane is projected onto the face.
    let (_, r) = eval(&format!(
        "plane Up (base: XY, offset: 30 mm)

sketch Marks (plane: Up, next_id: 2) {{
  p1 = point(12, 13)
}}

hole H1 (face: {TOP}, points: Marks, diameter: 4 mm, through: true)"
    ));
    ok(&r);
    let w = face(&r, "H1/wall(p1)");
    assert!((w.centroid[0] - 12.0).abs() < 1e-6 && (w.centroid[1] - 13.0).abs() < 1e-6);
    // Pattern copies of a circle are sites too.
    let (ev, r) = eval(&format!(
        "param n = 3

sketch Marks (plane: {TOP}, next_id: 6) {{
  p1 = point(10, 10)
  c2 = circle(p1, 1)
  g3 = pattern_linear(of: [c2], count: n, dx: 15 mm, dy: 0 mm)
}}

hole H1 (face: {TOP}, points: Marks, diameter: 4 mm, depth: 8 mm, point_angle: 180 deg)"
    ));
    ok(&r);
    assert!(close(PLATE_VOLUME - volume(&ev, &r), 3.0 * PI * 4.0 * 8.0, 1e-6));
    assert!(face(&r, "H1/wall(p1_2)").centroid[0] > 39.0);

    // A side face: the hole goes in against its outward normal (-X from the face at x = 60).
    let (ev, r) = eval(
        "hole H1 (face: E1/side(S1/l6), at: [[20 mm, 10 mm]], diameter: 6 mm, depth: 15 mm, point_angle: 180 deg)",
    );
    ok(&r);
    assert!(close(PLATE_VOLUME - volume(&ev, &r), PI * 9.0 * 15.0, 1e-6));
    let w = face(&r, "H1/wall(e1)");
    assert!((w.centroid[0] - 52.5).abs() < 1e-6, "{:?}", w.centroid);
    // The bottom face of the plate: the hole goes up.
    let (ev, r) =
        eval("hole H1 (face: E1/cap(start), at: [[30 mm, -20 mm]], diameter: 6 mm, depth: 5 mm, point_angle: 180 deg)");
    ok(&r);
    assert!(close(PLATE_VOLUME - volume(&ev, &r), PI * 9.0 * 5.0, 1e-6));
    assert!((face(&r, "H1/wall(e1)").centroid[2] - 2.5).abs() < 1e-6);
}

#[test]
fn faces_are_named_by_part_and_site() {
    let (_, r) = eval(&format!(
        "hole H1 (face: {TOP}, at: [[20 mm, 20 mm]], type: counterbore, diameter: 6.6 mm, depth: 12 mm, cbore_diameter: 11 mm, cbore_depth: 6.5 mm)

hole H2 (face: {TOP}, at: [[45 mm, 20 mm]], type: countersink, diameter: 6.6 mm, through: true, csink_diameter: 13.44 mm, csink_angle: 90 deg)"
    ));
    ok(&r);
    let n = names(&r);
    for f in [
        "E1/cap(end)",
        "E1/cap(start)",
        "H1/cbore_wall(e1)",
        "H1/cbore_floor(e1)",
        "H1/wall(e1)",
        "H1/floor(e1)",
        "H2/csink(e1)",
        "H2/wall(e1)",
    ] {
        assert!(n.contains(&f.to_string()), "{f} in {n:?}");
    }
    // The faces are where they should be.
    assert!((face(&r, "H1/cbore_floor(e1)").centroid[2] - 13.5).abs() < 1e-6);
    assert_eq!(face(&r, "H1/cbore_floor(e1)").kind, gusset_kernel::SurfaceKind::Plane);
    assert_eq!(face(&r, "H1/wall(e1)").kind, gusset_kernel::SurfaceKind::Cylinder);
    assert_eq!(face(&r, "H1/floor(e1)").kind, gusset_kernel::SurfaceKind::Cone);
    assert_eq!(face(&r, "H2/csink(e1)").kind, gusset_kernel::SurfaceKind::Cone);
    // No face of a hole tool is left over, and the top face keeps its name.
    assert!(n.iter().all(|f| !f.contains("/top(")), "{n:?}");
    assert_eq!(n.iter().filter(|f| f.as_str() == "E1/cap(end)").count(), 1, "{n:?}");
    // Its area is the plate's minus the two mouths.
    let top = face(&r, "E1/cap(end)");
    let mouths = PI * 5.5 * 5.5 + PI * 6.72 * 6.72;
    assert!(close(top.area, 2400.0 - mouths, 1e-6), "{}", top.area);
}

#[test]
fn a_wall_split_by_a_gap_gets_indexed_names() {
    // A U bracket (legs 10 mm thick, 40 mm apart, 20 mm tall): a hole through the left leg
    // continues through the right one, so its wall is two faces told apart by `#k`.
    let text = "gusset 1
units mm

sketch U (plane: XY, next_id: 25) {
  p1 = point(0, 0)
  p2 = point(60, 0)
  p3 = point(60, 40)
  p4 = point(50, 40)
  p5 = point(50, 10)
  p6 = point(10, 10)
  p7 = point(10, 40)
  p8 = point(0, 40)
  l9 = line(p1, p2)
  l10 = line(p2, p3)
  l11 = line(p3, p4)
  l12 = line(p4, p5)
  l13 = line(p5, p6)
  l14 = line(p6, p7)
  l15 = line(p7, p8)
  l16 = line(p8, p1)
}

extrude E1 (profile: U, distance: 20 mm, op: new)

hole H1 (face: E1/side(U/l16), at: [[25 mm, -10 mm]], diameter: 6 mm, through: true)
";
    let doc = Document::from_text(text).unwrap_or_else(|e| panic!("{e}"));
    let mut ev = Evaluator::new(kernel());
    let r = ev.evaluate(&doc, None, &());
    ok(&r);
    let n = names(&r);
    assert!(n.contains(&"H1/wall(e1)#1".to_string()) && n.contains(&"H1/wall(e1)#2".to_string()), "{n:?}");
    assert!(!n.contains(&"H1/wall(e1)".to_string()));
    // The two tubes sit in the legs: x = 5 and x = 55.
    let (a, b) = (face(&r, "H1/wall(e1)#1").centroid[0], face(&r, "H1/wall(e1)#2").centroid[0]);
    assert!((a.min(b) - 5.0).abs() < 1e-6 && (a.max(b) - 55.0).abs() < 1e-6, "{a} {b}");
    // Only the legs lose material: 2 x 10 mm of a 6 mm hole.
    let area = 60.0 * 10.0 + 2.0 * 10.0 * 30.0;
    let v = ev.kernel().mass_properties(&r.body("E1").unwrap().shape).unwrap().volume;
    assert!(close(v, area * 20.0 - 2.0 * PI * 9.0 * 10.0, 1e-6), "{v}");
}

#[test]
fn a_fillet_can_name_the_edge_of_a_hole() {
    let (ev, r) = eval(&format!(
        "hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], diameter: 6 mm, through: true)

fillet F1 (edges: [edge(E1/cap(end), H1/wall(e1))], radius: 1 mm)"
    ));
    ok(&r);
    // A 1 mm round on a hole edge removes (1 - pi/4) * (2 pi r_edge ...) of material: the
    // ring around the edge adds material back as the blend is convex, so only check it moved.
    let v = volume(&ev, &r);
    assert!(v < PLATE_VOLUME - PI * 9.0 * 20.0, "{v}");
    assert!(names(&r).iter().any(|f| f.starts_with("F1/blend(")));
}

#[test]
fn parameters_reshape_the_hole() {
    let mut s = Session::new(&Document::from_text(PLATE).unwrap(), kernel(), &CmdCtx::user(0));
    let r = run(
        &mut s,
        "feature.hole",
        json!({"face": TOP, "at": [["30 mm", "20 mm"]], "standard": "ISO M6 counterbore DIN 974/ISO 4762 head"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let (rr, rc, rb) = (3.3, 5.5, 6.5);
    let expect = |rr: f64, rc: f64, rb: f64, t: f64| PI * rc * rc * rb + PI * rr * rr * (t - rb);
    assert!(close(removed_in(&mut s), expect(rr, rc, rb, 20.0), 1e-6));
    run(&mut s, "param.set", json!({"name": "hole1_diameter", "expr": "7 mm"}));
    run(&mut s, "param.set", json!({"name": "hole1_cbore_diameter", "expr": "12 mm"}));
    run(&mut s, "param.set", json!({"name": "hole1_cbore_depth", "expr": "8 mm"}));
    assert!(close(removed_in(&mut s), expect(3.5, 6.0, 8.0, 20.0), 1e-6));
    run(&mut s, "param.set", json!({"name": "t", "expr": "15 mm"}));
    assert!(close(removed_in(&mut s), expect(3.5, 6.0, 8.0, 15.0), 1e-6));
    // Making the hole wider than its counterbore is reported on the feature, the rebuild goes on.
    run(&mut s, "param.set", json!({"name": "hole1_diameter", "expr": "13 mm"}));
    let ev = s.evaluate(&Actor::User).unwrap();
    let Some(FeatureStatus::Failed(e)) = ev.status("Hole1") else { panic!("{:?}", ev.status("Hole1")) };
    assert!(e.message.contains("not larger than"), "{e}");
    assert!(e.hint.is_some());
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

/// Volume removed from the plate, whose height is the parameter `t`.
fn removed_in(s: &mut Session) -> f64 {
    let ev = s.evaluate(&Actor::User).unwrap();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    let t = ev.params.get("t").unwrap().as_ref().unwrap().value;
    let v = s.kernel().mass_properties(&ev.body("E1").unwrap().shape).unwrap().volume;
    60.0 * 40.0 * t - v
}

#[test]
fn presets_match_the_table() {
    let mut s = Session::new(&Document::from_text(PLATE).unwrap(), kernel(), &CmdCtx::user(0));
    // ISO 273 clearance: fine, normal and coarse for M6 and M10.
    for (std, d) in [
        ("ISO M6 clearance fine", 6.4),
        ("ISO M6 clearance normal", 6.6),
        ("ISO M6 clearance coarse", 7.0),
        ("ISO M10 clearance normal", 11.0),
    ] {
        let r =
            run(&mut s, "feature.hole", json!({"face": TOP, "at": [["30 mm", "20 mm"]], "standard": std, "name": "P"}));
        assert_eq!(r["rebuild_ok"], true, "{std}: {r}");
        assert!(close(removed_in(&mut s), PI * d * d / 4.0 * 20.0, 1e-6), "{std}");
        // Undo the hole and its parameter in one step.
        run(&mut s, "history.undo", json!({}));
    }
    // ISO 10642 countersink for M6: head diameter 13.44, 90 degrees.
    run(
        &mut s,
        "feature.hole",
        json!({"face": TOP, "at": [["30 mm", "20 mm"]], "standard": "ISO M6 countersink 90 deg"}),
    );
    let (r, rs) = (3.3, 6.72);
    let h = rs - r;
    let expected = PI * h / 3.0 * (rs * rs + rs * r + r * r) + PI * r * r * (20.0 - h);
    assert!(close(removed_in(&mut s), expected, 1e-6));
    run(&mut s, "history.undo", json!({}));
    // ISO 261/262 + 2306 tapped M8: tap drill 6.8, pitch 1.25; thread 12 deep, drill 16 deep.
    let r = run(&mut s, "feature.hole", json!({"face": TOP, "at": [["30 mm", "20 mm"]], "standard": "ISO M8 tapped"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    assert!(close(removed_in(&mut s), PI * 3.4 * 3.4 * 16.0 + cone(3.4, A118), 1e-6));
    let t = &r["holes"][0]["thread"];
    assert_eq!(t["designation_user_text"], "M8");
    assert_eq!(t["pitch_mm"], 1.25);
    assert_eq!(t["depth_mm"], 12.0);
    assert_eq!(t["cosmetic"], true);
    // query_body lists the same record.
    let q = run(&mut s, "query.body", json!({}));
    assert_eq!(q["holes"][0]["thread"]["designation_user_text"], "M8");
    assert_eq!(q["holes"][0]["diameter_mm"], 6.8);
    assert_eq!(q["holes"][0]["depth_mm"], 16.0);
    assert_eq!(q["holes"][0]["wall"], "Hole1/wall(e1)");
    // An explicit dimension overrides the preset and stays an editable parameter.
    run(&mut s, "history.undo", json!({}));
    run(
        &mut s,
        "feature.hole",
        json!({"face": TOP, "at": [["30 mm", "20 mm"]], "standard": "ISO M8 tapped", "depth": "14 mm"}),
    );
    assert!(close(removed_in(&mut s), PI * 3.4 * 3.4 * 14.0 + cone(3.4, A118), 1e-6));
    let text = s.document(&Actor::User).unwrap().to_text();
    assert!(text.contains("param hole1_depth = 14 mm"), "{text}");
}

#[test]
fn errors_are_clean() {
    let mut s = Session::new(&Document::from_text(PLATE).unwrap(), kernel(), &CmdCtx::user(0));
    let before = s.document(&Actor::User).unwrap().to_text();
    for (args, why) in [
        (
            json!({"face": TOP, "at": [["58 mm", "20 mm"]], "diameter": "6 mm", "through": true}),
            "does not fit inside face",
        ),
        (
            json!({"face": TOP, "at": [["30 mm", "-2 mm"]], "diameter": "6 mm", "through": true}),
            "does not fit inside face",
        ),
        (
            json!({"face": TOP, "at": [["30 mm", "20 mm"]], "diameter": "80 mm", "through": true}),
            "does not fit inside face",
        ),
        (
            json!({"face": TOP, "at": [["8 mm", "20 mm"]], "diameter": "4 mm", "through": true, "type": "counterbore", "cbore_diameter": "20 mm", "cbore_depth": "3 mm"}),
            "does not fit inside face",
        ),
        (
            json!({"face": TOP, "at": [["30 mm", "20 mm"]], "diameter": "8 mm", "through": true, "type": "counterbore", "cbore_diameter": "6 mm", "cbore_depth": "3 mm"}),
            "not larger than the hole",
        ),
        (
            json!({"face": TOP, "at": [["30 mm", "20 mm"]], "diameter": "8 mm", "depth": "3 mm", "type": "counterbore", "cbore_diameter": "12 mm", "cbore_depth": "3 mm"}),
            "not less than the hole depth",
        ),
        (
            json!({"face": TOP, "at": [["30 mm", "20 mm"]], "diameter": "6 mm", "through": true, "type": "counterbore", "cbore_diameter": "12 mm", "cbore_depth": "25 mm"}),
            "as deep as the part",
        ),
    ] {
        let e = execute(&mut s, &CmdCtx::user(1), "feature.hole", args.clone()).unwrap_err();
        assert_eq!(e.code, "invalid_args", "{args}: {e:?}");
        assert!(format!("{e:?}").contains(why), "{args}: {why}: {e:?}");
        assert!(e.hint.is_some(), "{args}: {e:?}");
    }
    assert_eq!(s.document(&Actor::User).unwrap().to_text(), before, "refused holes change nothing");
    // A face that is not flat.
    let (_, r) = eval(
        "sketch C (plane: XY, next_id: 3) {\n  p1 = point(0, 0)\n  c2 = circle(p1, 10)\n}\n\nextrude Cyl (profile: C, distance: 20 mm, op: new)\n\nhole H1 (face: Cyl/side(C/c2), at: [[0 mm, 0 mm]], diameter: 2 mm, through: true)",
    );
    let Some(FeatureStatus::Failed(e)) = r.status("H1") else { panic!("{:?}", r.status("H1")) };
    assert_eq!(e.code, ErrorCode::InvalidReference);
    assert!(e.message.contains("not planar") && e.hint.is_some(), "{e}");
    // A face that is gone: the feature fails with a suggestion and later features are blocked.
    let (_, r) = eval(
        "hole H1 (face: E1/cap(end, l9), at: [[30 mm, 20 mm]], diameter: 2 mm, through: true)\n\nsketch After (plane: H1/wall(e1), next_id: 1) {\n}",
    );
    let Some(FeatureStatus::Failed(e)) = r.status("H1") else { panic!() };
    assert_eq!(e.code, ErrorCode::MissingReference);
    assert!(e.suggestion.is_some());
    assert!(matches!(r.status("After"), Some(FeatureStatus::Blocked { .. })));
}

/// Review L7: the fit check tests the face's boundary, so an existing hole anywhere inside a
/// new hole's mouth is found (the old check sampled the centre and one circle only).
#[test]
fn a_mouth_over_an_existing_hole_does_not_fit() {
    let first = format!("hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], diameter: 2 mm, through: true)");
    // The 2 mm hole lies 6 mm from the centre of a 20 mm counterbore: inside its mouth.
    let (_, r) = eval(&format!(
        "{first}\n\nhole H2 (face: {TOP}, at: [[36 mm, 20 mm]], type: counterbore, diameter: 4 mm, through: true, \
         cbore_diameter: 20 mm, cbore_depth: 3 mm)"
    ));
    let Some(FeatureStatus::Failed(e)) = r.status("H2") else { panic!("{:?}", r.status("H2")) };
    assert_eq!(e.code, ErrorCode::InvalidReference);
    assert!(e.message.contains("does not fit inside face"), "{e}");
    // Clear of the first hole it fits (0.5 mm between the mouth and the hole).
    let (_, r) = eval(&format!(
        "{first}\n\nhole H2 (face: {TOP}, at: [[41.5 mm, 20 mm]], type: counterbore, diameter: 4 mm, through: true, \
         cbore_diameter: 20 mm, cbore_depth: 3 mm)"
    ));
    ok(&r);
}

/// A 20 x 20 x 30 mm block over the middle of the plate (x 20..40, y 10..30) with a 4 mm
/// hole through its top, as body T.
const TOOL_BLOCK: &str = "sketch TS (plane: XY, next_id: 9) {
  p1 = point(20, 10)
  p2 = point(40, 10)
  p3 = point(40, 30)
  p4 = point(20, 30)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude T (profile: TS, distance: 30 mm, op: new)

hole HT (face: T/cap(end), at: [[30 mm, 20 mm]], diameter: 4 mm, through: true)
";

/// Review L1: `combine` with cut does not copy the tool's hole records into the target, and
/// records whose wall is cut away are dropped.
#[test]
fn hole_records_follow_the_geometry() {
    let (_, r) = eval(&format!("{TOOL_BLOCK}\ncombine K (body: E1, tools: [T], op: cut)"));
    ok(&r);
    assert!(r.body("E1").unwrap().holes.is_empty(), "{:?}", r.body("E1").unwrap().holes);
    // A union keeps them (the hole's wall is still there: it runs through the block above
    // the plate).
    let (_, r) = eval(&format!("{TOOL_BLOCK}\ncombine K (body: E1, tools: [T], op: add)"));
    ok(&r);
    let ids: Vec<String> = r.body("E1").unwrap().holes.iter().map(|h| format!("{}/{}", h.feature, h.at)).collect();
    assert_eq!(ids, ["HT/e1"]);
    // A pocket cut over a hole removes its wall, and its record goes with it.
    let hole = format!("hole H1 (face: {TOP}, at: [[30 mm, 20 mm]], diameter: 6 mm, through: true)");
    let (_, r) = eval(&hole);
    assert_eq!(r.body("E1").unwrap().holes.len(), 1);
    let (_, r) = eval(&format!(
        "{hole}\n\nsketch Pocket (plane: XY, next_id: 9) {{\n  p1 = point(20, 10)\n  p2 = point(40, 10)\n  \
         p3 = point(40, 30)\n  p4 = point(20, 30)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  \
         l8 = line(p4, p1)\n}}\n\nextrude Cut1 (profile: Pocket, distance: 30 mm, op: cut, body: E1)"
    ));
    ok(&r);
    assert!(r.body("E1").unwrap().holes.is_empty());
}

/// Review L2: explicit points keep their stored ids, so removing one does not rename the
/// other holes' faces.
#[test]
fn explicit_points_keep_their_ids() {
    let (_, r) =
        eval(&format!("hole H1 (face: {TOP}, at: [[15 mm, 20 mm], [45 mm, 20 mm]], diameter: 4 mm, through: true)"));
    ok(&r);
    let walls = |r: &Evaluation| names(r).into_iter().filter(|n| n.starts_with("H1/wall")).collect::<Vec<_>>();
    assert_eq!(walls(&r), ["H1/wall(e1)", "H1/wall(e2)"]);
    // The first point deleted (as the code view now prints it): the other keeps e2.
    let (_, r) = eval(&format!("hole H1 (face: {TOP}, at: [e2: [45 mm, 20 mm]], diameter: 4 mm, through: true)"));
    ok(&r);
    assert_eq!(walls(&r), ["H1/wall(e2)"]);
    let wall = face(&r, "H1/wall(e2)");
    assert!(close(wall.centroid[0], 45.0, 1e-6), "{:?}", wall.centroid);
}
