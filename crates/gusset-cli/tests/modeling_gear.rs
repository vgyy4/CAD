//! The gear generator with the real OCCT kernel (ADR 0018): volumes against the analytic
//! outline, tooth count, tooth thickness on the pitch circle measured with a thin ring,
//! meshing pairs without interference, helical twist, and parameter edits.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::disallowed_methods)]

use gusset_commands::gear::{GearValues, Geometry};
use gusset_commands::{CmdCtx, Session, execute};
use gusset_doc::Document;
use gusset_history::Actor;
use gusset_kernel::BoolOp;
use serde_json::{Value, json};
use std::f64::consts::PI;
use std::sync::Arc;

fn session() -> Session {
    let kernel: Arc<dyn gusset_kernel::Kernel> =
        Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap_or_else(|e| panic!("OCCT: {e}")));
    Session::new(&Document::from_text("gusset 1\nunits mm\n").unwrap(), kernel, &CmdCtx::user(0))
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn gear(s: &mut Session, args: Value) -> Value {
    let r = run(s, "generate.gear", args);
    assert_eq!(r["rebuild_ok"], true, "{r}");
    r
}

fn volume(s: &mut Session, body: &str) -> f64 {
    let ev = s.evaluate(&Actor::User).unwrap();
    let b = ev.body(body).unwrap_or_else(|| panic!("no body {body}: {:?}", ev.errors()));
    s.kernel().mass_properties(&b.shape).unwrap().volume
}

fn values(module: f64, teeth: u32) -> GearValues {
    GearValues {
        module,
        teeth,
        pressure_angle: 20f64.to_radians(),
        shift: 0.0,
        backlash: 0.0,
        fillet: 0.38,
        helix: 0.0,
        face_width: 10.0,
        bore: 6.0,
    }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// Volume of the part of `a` that lies in `b` (0 when they only touch or are apart).
fn overlap(s: &mut Session, a: &str, b: &str) -> f64 {
    let ev = s.evaluate(&Actor::User).unwrap();
    let (ba, bb) = (ev.body(a).unwrap().shape.clone(), ev.body(b).unwrap().shape.clone());
    match s.kernel().boolean(BoolOp::Intersect, &ba, &bb) {
        Ok(r) => s.kernel().mass_properties(&r.shape).map_or(0.0, |m| m.volume.abs()),
        // No common material at all is reported as an error.
        Err(_) => 0.0,
    }
}

#[test]
fn spur_gear_volume_bounds_and_tooth_count() {
    let mut s = session();
    let r = gear(&mut s, json!({}));
    assert_eq!(r["helical"], false);
    // The solid is the exact outline (from the formulas) minus the bore, times the width.
    let want = (Geometry::new(&values(2.0, 20)).unwrap().outline_area() - PI * 9.0) * 10.0;
    let v = volume(&mut s, "Gear1");
    eprintln!("spur gear volume {v:.4} vs {want:.4} ({:.2e})", rel(v, want));
    assert!(rel(v, want) < 2e-5, "{v} vs {want}");
    // Between the root and tip cylinders (minus the bore).
    assert!(v > PI * (17.5f64.powi(2) - 9.0) * 10.0 && v < PI * (22.0f64.powi(2) - 9.0) * 10.0);
    let ev = s.evaluate(&Actor::User).unwrap();
    let body = ev.body("Gear1").unwrap();
    let bb = s.kernel().bbox(&body.shape).unwrap();
    assert!(
        rel(bb.size()[0], 44.0) < 1e-3 && rel(bb.size()[1], 44.0) < 1e-3 && rel(bb.size()[2], 10.0) < 1e-6,
        "{bb:?}"
    );
    // One tip face and one root face per tooth: 20 of each, named after the tip and root arcs
    // of the first tooth and their pattern copies.
    let text = s.document(&Actor::User).unwrap().to_text();
    let arcs: Vec<&str> =
        text.lines().filter(|l| l.contains("= arc(")).map(|l| l.trim().split(' ').next().unwrap()).collect();
    assert_eq!(arcs.len(), 2, "{arcs:?}");
    let names: Vec<String> = body.names.faces.iter().map(ToString::to_string).collect();
    for arc in arcs {
        let n = names
            .iter()
            .filter(|f| {
                let base = f.split("Gear1_profile/").nth(1).unwrap_or("").split(')').next().unwrap_or("");
                base == arc || base.starts_with(&format!("{arc}_"))
            })
            .count();
        assert_eq!(n, 20, "{arc}: {names:?}");
    }
    // The bore and the two caps.
    assert!(names.contains(&"Gear1/cap(start)".to_string()) && names.contains(&"Gear1/cap(end)".to_string()));
    let topo = &body.topology;
    assert_eq!(topo.solids, 1);
    assert!(topo.faces.iter().any(|f| f.kind == gusset_kernel::SurfaceKind::Cylinder));
}

#[test]
fn parameter_edits_regenerate_the_solid() {
    let mut s = session();
    gear(&mut s, json!({}));
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": "30"}));
    run(&mut s, "param.set", json!({"name": "gear1_module", "expr": "3 mm"}));
    run(&mut s, "param.set", json!({"name": "gear1_face_width", "expr": "12 mm"}));
    let want =
        (Geometry::new(&GearValues { face_width: 12.0, ..values(3.0, 30) }).unwrap().outline_area() - PI * 9.0) * 12.0;
    let v = volume(&mut s, "Gear1");
    assert!(rel(v, want) < 2e-5, "{v} vs {want}");
    let bb = {
        let ev = s.evaluate(&Actor::User).unwrap();
        s.kernel().bbox(&ev.body("Gear1").unwrap().shape).unwrap()
    };
    assert!(rel(bb.size()[2], 12.0) < 1e-6 && rel(bb.size()[0], 3.0 * 32.0) < 1e-3, "{bb:?}");
    // A profile shift and backlash.
    run(&mut s, "param.set", json!({"name": "gear1_shift", "expr": "0.25"}));
    run(&mut s, "param.set", json!({"name": "gear1_backlash", "expr": "0.04 mm"}));
    let g = GearValues { face_width: 12.0, shift: 0.25, backlash: 0.04, ..values(3.0, 30) };
    let want = (Geometry::new(&g).unwrap().outline_area() - PI * 9.0) * 12.0;
    assert!(rel(volume(&mut s, "Gear1"), want) < 2e-5);
    // A sketch on the end cap follows the new width.
    run(&mut s, "sketch.create", json!({"plane": "Gear1/cap(end)"}));
    let ev = s.evaluate(&Actor::User).unwrap();
    assert!((ev.sketches["Sketch1"].frame.origin[2] - 12.0).abs() < 1e-6);
}

/// Thickness of the teeth on the pitch circle, measured: a thin ring around the pitch circle
/// is intersected with the gear; its volume is (tooth count) * (width) * (thickness) * (ring
/// thickness), because the ring is thin.
fn measured_thickness(args: Value, module: f64, teeth: u32) -> f64 {
    let mut s = session();
    gear(&mut s, args);
    let text = s.document(&Actor::User).unwrap().to_text();
    let r = module * f64::from(teeth) / 2.0;
    let half = 0.05;
    let ring = format!(
        "\nsketch RingSketch (plane: XY, next_id: 5) {{\n  p1 = point(0, 0)\n  c2 = circle(p1, {})\n  c3 = circle(p1, {})\n}}\n\nextrude Ring (profile: RingSketch.regions(c2), distance: 10 mm, op: new)\n",
        r + half,
        r - half
    );
    run(&mut s, "document.set_text", json!({"text": format!("{text}{ring}")}));
    let v = overlap(&mut s, "Gear1", "Ring");
    v / (f64::from(teeth) * 10.0 * 2.0 * half)
}

#[test]
fn tooth_thickness_on_the_pitch_circle() {
    // x = 0: pi m / 2, to the accuracy of a 0.1 mm thick ring.
    let t = measured_thickness(json!({"module": "2 mm", "teeth": "20", "bore": "6 mm"}), 2.0, 20);
    eprintln!("pitch-circle tooth thickness {t:.5} vs {:.5}", PI);
    assert!((t - PI).abs() < 2e-3, "{t}");
    // Profile shift thickens by 2 x m tan(a); backlash thins.
    let shifted = measured_thickness(json!({"shift": "0.3"}), 2.0, 20);
    assert!((shifted - (PI + 2.0 * 0.3 * 2.0 * 20f64.to_radians().tan())).abs() < 2e-3, "{shifted}");
    let loose = measured_thickness(json!({"backlash": "0.1 mm"}), 2.0, 20);
    assert!((loose - (PI - 0.1)).abs() < 2e-3, "{loose}");
    // Another module and tooth count.
    let t = measured_thickness(json!({"module": "1.5 mm", "teeth": "34", "bore": "4 mm"}), 1.5, 34);
    assert!((t - PI * 0.75).abs() < 2e-3, "{t}");
}

fn mesh_pair(z2: u32, extra1: Value, extra2: Value, spin_deg: f64, distance_shift: f64, helical: bool) -> (f64, f64) {
    let mut s = session();
    let mut a1 = json!({"module": "2 mm", "teeth": "20", "name": "Gear1"});
    let mut a2 = json!({"module": "2 mm", "teeth": z2.to_string(), "name": "Gear2"});
    for (a, e) in [(&mut a1, extra1), (&mut a2, extra2)] {
        if let (Value::Object(m), Value::Object(x)) = (a, e) {
            m.extend(x);
        }
    }
    gear(&mut s, a1);
    gear(&mut s, a2);
    let mt = if helical { 2.0 / 15f64.to_radians().cos() } else { 2.0 };
    let centre = mt * f64::from(20 + z2) / 2.0 + distance_shift;
    let mut args = json!({"body": "Gear2", "translate": [format!("{centre} mm"), "0 mm", "0 mm"]});
    if spin_deg != 0.0 {
        args["axis"] = json!("Z");
        args["angle"] = json!(format!("{spin_deg} deg"));
    }
    let r = run(&mut s, "feature.move_body", args);
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let single = volume(&mut s, "Gear1");
    (overlap(&mut s, "Gear1", "Gear2"), single)
}

#[test]
fn spur_gears_mesh_at_the_right_centre_distance_without_interference() {
    // 20 and 31 teeth (an odd count needs no turn): centre distance m (z1 + z2) / 2 = 51.
    let (touching, single) = mesh_pair(31, json!({}), json!({}), 0.0, 0.0, false);
    eprintln!("touching pair: overlap {touching:.5} mm3 of {single:.1}");
    assert!(touching < 0.5, "overlap {touching}");
    // A pair with an even tooth count needs half a pitch of turn on the second gear.
    let (even, _) = mesh_pair(30, json!({}), json!({}), 180.0 / 30.0, 0.0, false);
    assert!(even < 0.5, "overlap {even}");
    // The wrong phase, or a centre distance 0.5 mm too short, interferes clearly.
    let (wrong, _) = mesh_pair(31, json!({}), json!({}), 180.0 / 31.0, 0.0, false);
    assert!(wrong > 50.0, "overlap {wrong}");
    let (close, _) = mesh_pair(31, json!({}), json!({}), 0.0, -0.5, false);
    assert!(close > 5.0, "overlap {close}");
    // With backlash on both gears the teeth clear each other.
    let (loose, _) = mesh_pair(31, json!({"backlash": "0.08 mm"}), json!({"backlash": "0.08 mm"}), 0.0, 0.0, false);
    assert!(loose < 1e-6, "overlap {loose}");
    // A profile-shifted pair keeps the centre distance of the unshifted one when the shifts
    // cancel (+0.3 and -0.3).
    let (shifted, _) = mesh_pair(31, json!({"shift": "0.3"}), json!({"shift": "-0.3"}), 0.0, 0.0, false);
    assert!(shifted < 0.5, "overlap {shifted}");
}

#[test]
fn helical_gear_volume_and_twist() {
    let mut s = session();
    let r = gear(&mut s, json!({"name": "Hel", "teeth": "24", "helix": "20 deg", "face_width": "15 mm"}));
    assert_eq!(r["helical"], true);
    let v = GearValues { helix: 20f64.to_radians(), face_width: 15.0, ..values(2.0, 24) };
    let g = Geometry::new(&v).unwrap();
    let want = (g.outline_area() - PI * 9.0) * 15.0;
    let got = volume(&mut s, "Hel");
    eprintln!("helical volume {got:.4} vs {want:.4} ({:.2e})", rel(got, want));
    // A twisted extrusion has the volume of the straight one (Cavalieri).
    assert!(rel(got, want) < 3e-4, "{got} vs {want}");
    // The tooth tip at the end cap is turned by W tan(beta) / r_pitch, counter-clockwise.
    let twist = 15.0 * 20f64.to_radians().tan() / g.pitch_radius;
    let text = s.document(&Actor::User).unwrap().to_text();
    let tip =
        text.lines().find(|l| l.contains("= arc(")).map(|l| l.trim().split(' ').next().unwrap().to_string()).unwrap();
    let ev = s.evaluate(&Actor::User).unwrap();
    let body = ev.body("Hel").unwrap();
    let want_edge = format!("edge(Hel/cap(end), Hel/side(Hel_profile/{tip}))");
    let e = body.names.edges.iter().position(|e| e.to_string() == want_edge).unwrap_or_else(|| {
        panic!("{want_edge} in {:?}", body.names.edges.iter().map(ToString::to_string).take(20).collect::<Vec<_>>())
    });
    let m = body.topology.edges[e].midpoint;
    let angle = m[1].atan2(m[0]);
    assert!((m[0].hypot(m[1]) - g.tip_radius).abs() < 2e-3 && (m[2] - 15.0).abs() < 1e-6, "{m:?}");
    assert!((angle - twist).abs() < 1e-3, "tip at {angle} rad, expected {twist}");
    // Changing the helix angle changes the twist, not the volume.
    run(&mut s, "param.set", json!({"name": "hel_helix", "expr": "30 deg"}));
    let v30 = GearValues { helix: 30f64.to_radians(), face_width: 15.0, ..values(2.0, 24) };
    let want30 = (Geometry::new(&v30).unwrap().outline_area() - PI * 9.0) * 15.0;
    assert!(rel(volume(&mut s, "Hel"), want30) < 3e-4);
}

#[test]
fn helical_gears_of_opposite_hand_mesh() {
    let right = json!({"helix": "15 deg", "face_width": "10 mm"});
    let left = json!({"helix": "-15 deg", "face_width": "10 mm"});
    let (mesh, single) = mesh_pair(31, right.clone(), left.clone(), 0.0, 0.0, true);
    eprintln!("helical pair: overlap {mesh:.4} mm3 of {single:.1}");
    assert!(mesh < 2.0, "overlap {mesh}");
    // The same hand on both, or the wrong phase, collides.
    let (same_hand, _) = mesh_pair(31, right.clone(), right.clone(), 0.0, 0.0, true);
    assert!(same_hand > 20.0, "overlap {same_hand}");
    let (wrong, _) = mesh_pair(31, right, left, 180.0 / 31.0, 0.0, true);
    assert!(wrong > 50.0, "overlap {wrong}");
}

#[test]
fn undercut_and_other_bad_gears_fail_cleanly() {
    let mut s = session();
    let e = execute(&mut s, &CmdCtx::user(1), "generate.gear", json!({"teeth": "9"})).unwrap_err();
    assert!(format!("{e:?}").contains("undercut"));
    // Editing a parameter into an impossible gear gives an error on the gear's features, and
    // fixing it brings the solid back.
    gear(&mut s, json!({}));
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": "8"}));
    let ev = s.evaluate(&Actor::User).unwrap();
    assert!(!ev.errors().is_empty());
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": "22"}));
    assert!(volume(&mut s, "Gear1") > 0.0);
}

#[test]
fn a_generated_spring_has_the_volume_of_its_section_times_its_length() {
    // A sweep of a section across the axis, twisted, keeps the section's area at every
    // height (Cavalieri): volume = pi (wire/2)^2 * pitch * turns.
    let mut s = session();
    let r =
        run(&mut s, "generate.spring", json!({"wire": "1.5 mm", "diameter": "12 mm", "pitch": "4 mm", "turns": "5"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let want = PI * 0.75 * 0.75 * 4.0 * 5.0;
    let v = volume(&mut s, "Spring1");
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
    let b = run(&mut s, "query.body", json!({"body": "Spring1"}));
    let (lo, hi) = (&b["bbox_min_mm"], &b["bbox_max_mm"]);
    let span = |i: usize| hi[i].as_f64().unwrap() - lo[i].as_f64().unwrap();
    // Outer diameter 13.5, free length 20.
    assert!((span(2) - 20.0).abs() < 1e-3, "{b}");
    assert!((span(0) - 13.5).abs() < 0.05, "{b}");
    // A parameter edit rebuilds it.
    run(&mut s, "param.set", json!({"name": "spring1_turns", "expr": "8"}));
    let v8 = volume(&mut s, "Spring1");
    assert!((v8 - want * 8.0 / 5.0).abs() < 1e-4 * want, "{v8}");
    // Touching coils are refused.
    let e = execute(&mut s, &CmdCtx::user(1), "generate.spring", json!({"wire": "2 mm", "pitch": "2 mm"})).unwrap_err();
    assert_eq!(e.code, "invalid_args");
}

#[test]
fn a_generated_thread_is_the_core_plus_the_screwed_tooth() {
    // ADR 0034: the tooth outside the core, screwed (L - P) / P turns, adds
    // 2 pi * (first moment of its section about the axis) per turn.
    let mut s = session();
    let t0 = std::time::Instant::now();
    let r = run(&mut s, "generate.thread", json!({"diameter": "10 mm", "pitch": "1.5 mm", "length": "20 mm"}));
    eprintln!("M10 x 1.5 x 20 built in {:?}", t0.elapsed());
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let (d, p, l) = (10.0, 1.5, 20.0);
    let r1 = (d - 1.082_531_754_7 * p) / 2.0;
    let pts = [[r1, 0.0], [d / 2.0, 5.0 * p / 16.0], [d / 2.0, 7.0 * p / 16.0], [r1, 0.75 * p]];
    let moment: f64 = (0..4)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % 4]);
            (a[0] + b[0]) * (a[0] * b[1] - b[0] * a[1]) / 6.0
        })
        .sum::<f64>()
        .abs();
    let want = PI * r1 * r1 * l + (l - p) / p * 2.0 * PI * moment;
    let v = volume(&mut s, "Thread1");
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
    let b = run(&mut s, "query.body", json!({"body": "Thread1"}));
    let (lo, hi) = (&b["bbox_min_mm"], &b["bbox_max_mm"]);
    let span = |i: usize| hi[i].as_f64().unwrap() - lo[i].as_f64().unwrap();
    assert!((span(0) - 10.0).abs() < 0.01 && (span(2) - 20.0).abs() < 1e-6, "{b}");
    // With a hex head (ISO 4017 M10) it is a bolt: the rod plus the head prism below it.
    let r = run(&mut s, "generate.thread", json!({"name": "Bolt", "head_width": "16 mm", "head_height": "6.4 mm"}));
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let head = 3f64.sqrt() / 2.0 * 16.0 * 16.0 * 6.4;
    let vb = volume(&mut s, "Bolt");
    assert!((vb - want - head).abs() < 1e-4 * (want + head), "{vb} vs {}", want + head);
}

#[test]
fn a_generated_nut_is_the_hex_minus_the_hole_and_the_groove() {
    // ADR 0034: the groove runs a pitch past both faces, so inside the nut its section is
    // the same at every height: 2 pi * (first moment of the tooth) / pitch per unit height.
    let mut s = session();
    let t0 = std::time::Instant::now();
    let r = run(&mut s, "generate.nut", json!({}));
    eprintln!("M10 nut built in {:?}", t0.elapsed());
    assert_eq!(r["rebuild_ok"], true, "{r}");
    let (d, p, w, m) = (10.0, 1.5, 16.0, 8.4);
    let r1 = (d - 1.082_531_754_7 * p) / 2.0;
    let pts = [[r1, 0.0], [d / 2.0, 5.0 * p / 16.0], [d / 2.0, 7.0 * p / 16.0], [r1, 0.75 * p]];
    let moment: f64 = (0..4)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % 4]);
            (a[0] + b[0]) * (a[0] * b[1] - b[0] * a[1]) / 6.0
        })
        .sum::<f64>()
        .abs();
    let hex = 3f64.sqrt() / 2.0 * w * w;
    let want = (hex - PI * r1 * r1) * m - m * 2.0 * PI * moment / p;
    let v = volume(&mut s, "Nut1");
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
}
