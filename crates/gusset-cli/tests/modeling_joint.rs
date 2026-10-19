//! Joints with the real kernel (ADR 0026): frames on cylinder axes, a shaft placed in a
//! bore with a cylindrical joint, and motion driven by parameters.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp, clippy::disallowed_methods)]

use gusset_doc::Document;
use gusset_eval::{Evaluation, Evaluator};
use std::sync::Arc;

const PLATE_AND_SHAFT: &str = "gusset 1
units mm

param slide = 0 mm
param turn = 0 deg

sketch Plate_s (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 30)
  p4 = point(0, 30)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Plate (profile: Plate_s, distance: 6 mm, op: new)

sketch Bore_s (plane: XY, next_id: 3) {
  p1 = point(25, 12)
  c2 = circle(p1, 4)
}

extrude Bore (profile: Bore_s, distance: 6 mm, op: cut, body: Plate)

sketch Shaft_s (plane: XZ, next_id: 3) {
  p1 = point(-50, 7)
  c2 = circle(p1, 4)
}

extrude Shaft (profile: Shaft_s, distance: 20 mm, op: new)

joint J1 (type: cylindrical, body: Shaft, at: Shaft/side(Shaft_s/c2), to: Bore/side(Bore_s/c2), distance: slide, angle: turn)
";

fn eval(slide: &str) -> (Evaluator, Evaluation) {
    let text = PLATE_AND_SHAFT.replace("param slide = 0 mm", &format!("param slide = {slide}"));
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(k);
    let r = ev.evaluate(&doc, None, &());
    (ev, r)
}

fn shaft_axis(ev: &Evaluator, r: &Evaluation) -> gusset_kernel::Axis3 {
    let t = ev.kernel().topology(&r.body("Shaft").unwrap().shape).unwrap();
    t.faces.iter().find_map(|f| f.axis).unwrap()
}

#[test]
fn a_cylindrical_joint_puts_the_shaft_on_the_bore_axis_and_slides_it() {
    let (ev, r) = eval("0 mm");
    assert!(r.errors().is_empty(), "{:?}", r.errors());
    // The shaft's axis is the bore's: vertical through (25, 12).
    let a = shaft_axis(&ev, &r);
    assert!((a.dir[2].abs() - 1.0).abs() < 1e-9, "{a:?}");
    assert!((a.origin[0] - 25.0).abs() < 1e-9 && (a.origin[1] - 12.0).abs() < 1e-9, "{a:?}");
    // The shaft's middle sits at the bore's middle (z = 3).
    let bb = ev.kernel().bbox(&r.body("Shaft").unwrap().shape).unwrap();
    assert!(((bb.min[2] + bb.max[2]) / 2.0 - 3.0).abs() < 1e-6, "{bb:?}");
    // Sliding moves it along the axis; the volume does not change.
    let (ev2, r2) = eval("5 mm");
    assert!(r2.errors().is_empty(), "{:?}", r2.errors());
    let bb2 = ev2.kernel().bbox(&r2.body("Shaft").unwrap().shape).unwrap();
    assert!(((bb2.min[2] - bb.min[2]).abs() - 5.0).abs() < 1e-6, "{bb:?} {bb2:?}");
    let vol =
        |ev: &Evaluator, r: &Evaluation| ev.kernel().mass_properties(&r.body("Shaft").unwrap().shape).unwrap().volume;
    assert!((vol(&ev, &r) - vol(&ev2, &r2)).abs() < 1e-6);
    assert!((vol(&ev, &r) - std::f64::consts::PI * 16.0 * 20.0).abs() < 1e-3);
}

/// A session on the plate-and-shaft document with the shaft's radius `r`.
fn session(r: &str) -> gusset_commands::Session {
    use gusset_commands::{CmdCtx, Session};
    let text = PLATE_AND_SHAFT
        .replace("p1 = point(-50, 7)\n  c2 = circle(p1, 4)", &format!("p1 = point(-50, 7)\n  c2 = circle(p1, {r})"));
    let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{e}"));
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    Session::new(&doc, k, &CmdCtx::user(0))
}

#[test]
fn interference_counts_overlap_not_contact() {
    use gusset_commands::{CmdCtx, execute};
    use serde_json::json;
    let u = CmdCtx::user(1);
    // A shaft as wide as the bore touches it all round: no interference.
    let mut s = session("4");
    let q = execute(&mut s, &u, "query.interference", json!({})).unwrap();
    assert_eq!(q["interference"], json!([]), "{q}");
    // One 1 mm wider overlaps a ring of the plate: pi (5² - 4²) 6 mm³.
    let mut s = session("5");
    let q = execute(&mut s, &u, "query.interference", json!({})).unwrap();
    let pairs = q["interference"].as_array().unwrap();
    assert_eq!(pairs.len(), 1, "{q}");
    let v = pairs[0]["volume_mm3"].as_f64().unwrap();
    assert!((v - std::f64::consts::PI * 9.0 * 6.0).abs() < 1e-6, "{q}");
    // Sliding the shaft out of the plate ends the interference.
    let m = execute(
        &mut s,
        &u,
        "study.motion",
        json!({"parameter": "slide", "from": "0 mm", "to": "20 mm", "steps": 3, "interference": true}),
    )
    .unwrap();
    let counts: Vec<usize> =
        m["steps"].as_array().unwrap().iter().map(|f| f["interference"].as_array().unwrap().len()).collect();
    assert_eq!(counts, [1, 1, 0], "{m}");
}

#[test]
fn a_joint_that_puts_a_body_inside_another_says_so() {
    // Dogfooding F4-4: the overlap went unnoticed. The fat shaft's second joint (a rigid
    // copy of J1's placement) reports the ring it shares with the plate.
    use gusset_commands::{CmdCtx, execute};
    use serde_json::json;
    let u = CmdCtx::user(1);
    let mut s = session("5");
    execute(&mut s, &u, "feature.suppress", json!({"name": "J1", "suppressed": true})).unwrap();
    let r = execute(
        &mut s,
        &u,
        "feature.joint",
        json!({"type": "rigid", "body": "Shaft", "at": "Shaft/side(Shaft_s/c2)", "to": "Bore/side(Bore_s/c2)"}),
    )
    .unwrap();
    assert!(r["warning"].as_str().unwrap_or("").contains("overlaps"), "{r}");
    let v = r["interference"][0]["volume_mm3"].as_f64().unwrap();
    assert!((v - std::f64::consts::PI * 9.0 * 6.0).abs() < 1e-6, "{r}");
    // The frame is on the bore's axis.
    let o = r["to_frame"]["origin_mm"].as_array().unwrap();
    assert!((o[0].as_f64().unwrap() - 25.0).abs() < 1e-9 && (o[1].as_f64().unwrap() - 12.0).abs() < 1e-9, "{r}");
}

#[test]
fn a_shaft_from_its_own_document_goes_into_the_bore() {
    use gusset_commands::{CmdCtx, Session, execute};
    use serde_json::json;
    let dir = std::env::temp_dir().join(format!("gusset-occt-component-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The shaft alone, saved as its own project file.
    let shaft = "gusset 1\nunits mm\n\nsketch S (plane: XY, next_id: 3) {\n  p1 = point(0, 0)\n  c2 = circle(p1, 4)\n}\n\n\
                 extrude Shaft (profile: S, distance: 20 mm, op: new)\n";
    let u = gusset_history::Ctx::user(1);
    let h = gusset_history::History::new(&Document::from_text(shaft).unwrap(), &u);
    let meta = gusset_project::ProjectMeta {
        id: gusset_store::paths::ProjectId::new_random().unwrap(),
        save_seq: 1,
        base_seq: 1,
        created: 1,
    };
    std::fs::write(dir.join("shaft.gus"), gusset_project::encode(&h, &meta).unwrap()).unwrap();
    // The plate with its bore, and the shaft as a component.
    let plate = PLATE_AND_SHAFT.split("sketch Shaft_s").next().unwrap();
    let k: Arc<dyn gusset_kernel::Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = Session::new(&Document::from_text(plate).unwrap(), k, &CmdCtx::user(0));
    s.project_dir = Some(dir.clone());
    let c = CmdCtx::user(1);
    let mut run =
        |id: &str, args: serde_json::Value| execute(&mut s, &c, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"));
    let r = run("assembly.insert_component", json!({"path": "shaft.gus"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let r = run(
        "feature.joint",
        json!({"type": "cylindrical", "body": "Shaft", "at": "Shaft/part(Shaft/side(S/c2))", "to": "Bore/side(Bore_s/c2)"}),
    );
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let b = run("query.body", json!({"body": "Shaft"}));
    let c = b["centroid_mm"].as_array().unwrap();
    // On the bore's axis, centred in the plate's thickness.
    let want = [25.0, 12.0, 3.0];
    assert!(c.iter().zip(want).all(|(a, w)| (a.as_f64().unwrap() - w).abs() < 1e-6), "{b}");
    assert!((b["volume_mm3"].as_f64().unwrap() - std::f64::consts::PI * 16.0 * 20.0).abs() < 1e-3, "{b}");
    let q = run("query.interference", json!({}));
    assert_eq!(q["interference"], json!([]), "{q}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A four-bar linkage (ground 40, crank 10, coupler 35, rocker 25) with pivots on the
/// links' vertical corner edges; the loop solver closes it for the crank angle.
#[test]
fn a_four_bar_linkage_closes_for_every_crank_angle() {
    let rect = |n: &str, w: f64| {
        format!(
            "sketch {n} (plane: XY, next_id: 9) {{\n  p1 = point(0, 0)\n  p2 = point({w}, 0)\n  p3 = point({w}, 2)\n  \
             p4 = point(0, 2)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}}\n"
        )
    };
    let pivot = |b: &str, s: &str, end: bool| {
        let (a, c) = if end { ("l5", "l6") } else { ("l8", "l5") };
        format!("edge({b}/side({s}/{a}), {b}/side({s}/{c}))")
    };
    let text = |crank: f64| {
        format!(
            "gusset 1\nunits mm\nparam crank = {crank} deg\n{}{}{}{}\
             extrude Ground (profile: G, distance: 1 mm, op: new)\nextrude Crank (profile: C, distance: 1 mm, op: new)\n\
             extrude Coupler (profile: K, distance: 1 mm, op: new)\nextrude Rocker (profile: R, distance: 1 mm, op: new)\n\
             joint J1 (type: revolute, body: Crank, at: {}, to: {}, angle: crank)\n\
             joint J2 (type: revolute, body: Coupler, at: {}, to: {}, free: [angle])\n\
             joint J3 (type: revolute, body: Rocker, at: {}, to: {}, angle: 90 deg, free: [angle])\n\
             joint J4 (type: revolute, body: Coupler, at: {}, to: {}, free: [angle])\n",
            rect("G", 40.0),
            rect("C", 10.0),
            rect("K", 35.0),
            rect("R", 25.0),
            pivot("Crank", "C", false),
            pivot("Ground", "G", false),
            pivot("Coupler", "K", false),
            pivot("Crank", "C", true),
            pivot("Rocker", "R", false),
            pivot("Ground", "G", true),
            pivot("Coupler", "K", true),
            pivot("Rocker", "R", true),
        )
    };
    let kernel = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut ev = Evaluator::new(kernel.clone());
    for crank in [0.0, 45.0, 120.0, 200.0] {
        let doc = Document::from_text(&text(crank)).unwrap();
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "crank {crank}: {:?}", r.errors());
        // The coupler's and the rocker's far pivots meet: a bottom vertex in common.
        let bottom = |body: &str| {
            let t = gusset_kernel::Kernel::topology(kernel.as_ref(), &r.body(body).unwrap().shape).unwrap();
            t.vertices.iter().map(|v| v.point).filter(|p| p[2].abs() < 1e-6).collect::<Vec<_>>()
        };
        let (k, rk) = (bottom("Coupler"), bottom("Rocker"));
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5);
        assert!(k.iter().any(|p| rk.iter().any(|q| near(*p, *q))), "crank {crank}: {k:?} {rk:?}");
    }
}
