//! The `generate.gear` command (mock kernel: the sketch is solved and its regions measured;
//! solids need OCCT, see gusset-cli/tests/modeling_gear.rs).
// Test-only code: expected values may use std float functions.
#![allow(clippy::disallowed_methods)]

use crate::gear::{self, GearValues, Geometry};
use crate::*;
use gusset_doc::Document;
use gusset_eval::SketchStatus;
use gusset_history::Actor;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0))
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn fail(s: &mut Session, args: Value) -> String {
    match execute(s, &CmdCtx::user(1), "generate.gear", args) {
        Ok(v) => panic!("should fail: {v}"),
        Err(e) => format!("{e:?}"),
    }
}

fn text(s: &Session) -> String {
    s.document(&Actor::User).unwrap().to_text()
}

/// Area of the toothed region of `sketch` (the one with a bore is the annulus), mm2.
fn tooth_area(s: &mut Session, sketch: &str) -> f64 {
    let ev = s.evaluate(&Actor::User).unwrap();
    let r = ev.sketches.get(sketch).unwrap_or_else(|| panic!("no sketch {sketch}"));
    assert_eq!(r.status, SketchStatus::WellConstrained, "{sketch}: dof {} conflicts {:?}", r.dof, r.conflicting);
    assert!(r.redundant.is_empty(), "redundant constraints: {:?}", r.redundant);
    let regions = r.regions.as_ref().unwrap_or_else(|e| panic!("{e}"));
    regions.iter().map(|i| i.area).fold(0.0, f64::max)
}

/// Area of the gear's toothed region with a bore of diameter `bore`: the exact outline
/// (from the formulas, finely sampled) minus the bore.
fn reference_area(v: &GearValues, bore: f64) -> f64 {
    Geometry::new(v).unwrap().outline_area() - std::f64::consts::PI * (bore / 2.0).powi(2)
}

fn defaults(teeth: u32) -> GearValues {
    GearValues {
        module: 2.0,
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

#[test]
fn the_command_is_registered_and_works_without_arguments() {
    let spec = find("generate.gear").expect("registered");
    assert!(spec.description.len() > 40 && spec.description.len() < 220);
    let schema = serde_json::to_string(&(spec.schema)()).unwrap();
    for f in ["module", "teeth", "pressure_angle", "face_width", "bore", "shift", "backlash", "fillet", "helix", "name"]
    {
        assert!(schema.contains(&format!("\"{f}\"")), "{f}");
    }
    let mut s = session();
    let r = run(&mut s, "generate.gear", json!({}));
    assert_eq!(r["feature"], "Gear1");
    assert_eq!(r["helical"], false);
    // The extrude of a curved profile needs OCCT; the rest of the rebuild is fine.
    let t = text(&s);
    for want in [
        "param gear1_module = 2 mm  # normal module",
        "param gear1_teeth = 20  # number of teeth",
        "param gear1_pressure_angle = 20 deg",
        "param gear1_shift = 0",
        "param gear1_pitch_diameter = gear1_transverse_module * gear1_teeth",
        "sketch Gear1_profile (plane: XY",
        "pattern_circular(of: [",
        "count: gear1_teeth, angle: 360 deg)",
        "extrude Gear1 (profile: Gear1_profile.regions(",
        "distance: gear1_face_width, op: new)",
    ] {
        assert!(t.contains(want), "{want}\n{}", &t[..t.len().min(3000)]);
    }
    // Everything is ordinary text and survives a round trip through the code view.
    let doc = Document::from_text(&t).unwrap();
    assert_eq!(doc.to_text(), t);
    assert!(!t.contains("helix"));
    assert_eq!(r["pitch_diameter_mm"], 40.0);
    assert!((r["tip_diameter_mm"].as_f64().unwrap() - 44.0).abs() < 1e-9);
    assert!((r["root_diameter_mm"].as_f64().unwrap() - 35.0).abs() < 1e-9);
}

#[test]
fn the_sketch_is_fully_constrained_and_has_the_analytic_area() {
    let mut s = session();
    run(&mut s, "generate.gear", json!({}));
    let area = tooth_area(&mut s, "Gear1_profile");
    let want = reference_area(&defaults(20), 6.0);
    // The annulus (outline minus bore): the spline flanks are within 0.5 um of the exact curves.
    assert!((area - want).abs() < 0.1, "{area} vs {want}");
    // Between the root and tip circles.
    let lo = std::f64::consts::PI * (17.5f64.powi(2) - 3.0f64.powi(2));
    let hi = std::f64::consts::PI * (22.0f64.powi(2) - 3.0f64.powi(2));
    assert!(area > lo && area < hi, "{area}");
}

#[test]
fn parameter_edits_reshape_the_teeth() {
    let mut s = session();
    run(&mut s, "generate.gear", json!({"module": "2 mm", "teeth": "20"}));
    let a20 = tooth_area(&mut s, "Gear1_profile");
    // More teeth on the same module: a bigger gear, with the right area.
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": "30"}));
    let a30 = tooth_area(&mut s, "Gear1_profile");
    let want = reference_area(&defaults(30), 6.0);
    assert!(a30 > a20 && (a30 - want).abs() < 0.1, "{a30} vs {want}");
    // The module scales everything: area grows with its square.
    run(&mut s, "param.set", json!({"name": "gear1_module", "expr": "3 mm"}));
    let big = tooth_area(&mut s, "Gear1_profile");
    let want = reference_area(&GearValues { module: 3.0, ..defaults(30) }, 6.0);
    assert!((big - want).abs() < 0.1, "{big} vs {want}");
    // Shift and backlash change the tooth thickness; the bore follows its parameter.
    run(&mut s, "param.set", json!({"name": "gear1_shift", "expr": "0.3"}));
    run(&mut s, "param.set", json!({"name": "gear1_backlash", "expr": "0.05 mm"}));
    run(&mut s, "param.set", json!({"name": "gear1_bore", "expr": "10 mm"}));
    let v = GearValues { module: 3.0, shift: 0.3, backlash: 0.05, ..defaults(30) };
    let shifted = tooth_area(&mut s, "Gear1_profile");
    assert!((shifted - reference_area(&v, 10.0)).abs() < 0.1, "{shifted}");
    // A value that cannot make a gear is an error on the sketch, not a crash.
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": "5"}));
    let ev = s.evaluate(&Actor::User).unwrap();
    assert!(!ev.errors().is_empty());
}

#[test]
fn prefix_rename_keeps_a_generated_gear_sketch_rebuilding() {
    let mut s = session();
    run(&mut s, "generate.gear", json!({}));
    let area = tooth_area(&mut s, "Gear1_profile");
    let r = run(&mut s, "param.rename", json!({"from": "gear1_", "to": "pinion_", "prefix": true}));
    assert!(r["renamed"].as_array().unwrap().len() > 9);
    let t = text(&s);
    assert!(t.contains("sketch Gear1_profile (plane: XY") && t.contains("gear: pinion"), "{t}");
    assert!(
        t.contains("param pinion_module")
            && t.contains("param pinion_pitch_diameter = pinion_transverse_module * pinion_teeth"),
        "{t}"
    );
    assert!(!t.contains("gear1_"), "{t}");
    assert!((tooth_area(&mut s, "Gear1_profile") - area).abs() < 1e-9);
    // Without the trailing underscore the key follows too; one gear input alone cannot move.
    run(&mut s, "param.rename", json!({"from": "pinion", "to": "wheel", "prefix": true}));
    assert!(text(&s).contains("gear: wheel"), "{}", text(&s));
    let e = execute(&mut s, &CmdCtx::user(1), "param.rename", json!({"from": "wheel_teeth", "to": "z"})).unwrap_err();
    assert!(e.message.contains("belongs to the gear") && e.hint.as_deref().unwrap_or("").contains("prefix"), "{e:?}");
    assert!((tooth_area(&mut s, "Gear1_profile") - area).abs() < 1e-9);
}

#[test]
fn options_and_names() {
    let mut s = session();
    let r = run(
        &mut s,
        "generate.gear",
        json!({"name": "Pinion", "module": "1.5 mm", "teeth": "24", "bore": "0 mm", "pressure_angle": "25 deg", "fillet": "0.2",
               "shift": "0.2", "backlash": "0.03 mm", "face_width": "8 mm"}),
    );
    assert_eq!(r["feature"], "Pinion");
    assert!(r["parameters"].to_string().contains("pinion_module"));
    let t = text(&s);
    assert!(
        t.contains("param pinion_module = 1.5 mm") && t.contains("param pinion_pressure_angle = 25 deg"),
        "{}",
        &t[..800]
    );
    assert!(t.contains("param pinion_bore = 0 mm"));
    assert!(!t.contains("diameter(c"), "no bore circle without a bore");
    // A second gear with other parameters coexists; names do not clash.
    let r = run(&mut s, "generate.gear", json!({"teeth": "40", "module": "pinion_module"}));
    assert_eq!(r["feature"], "Gear1");
    assert!(text(&s).contains("param gear1_module = pinion_module"));
    assert!(tooth_area(&mut s, "Gear1_profile") > tooth_area(&mut s, "Pinion_profile"));
    // Expressions with parameters of the document work for any option.
    run(&mut s, "param.set", json!({"name": "pinion_module", "expr": "2 mm"}));
    let _ = tooth_area(&mut s, "Gear1_profile");
}

#[test]
fn helical_gears_get_a_path_and_a_twisted_sweep() {
    let mut s = session();
    let r = run(&mut s, "generate.gear", json!({"helix": "20 deg", "name": "Hel"}));
    assert_eq!(r["helical"], true);
    let t = text(&s);
    for want in [
        "param hel_helix = 20 deg",
        "param hel_transverse_module = hel_module / cos(hel_helix)",
        "param hel_transverse_angle = atan(tan(hel_pressure_angle) / cos(hel_helix))",
        "sketch Hel_path (plane: XZ",
        "k6 = distance(l3, l3, hel_face_width)",
        "sweep Hel (profile: Hel_profile.regions(",
        "path: Hel_path, orientation: frenet, twist: hel_face_width * tan(hel_helix) / (hel_pitch_diameter / 2) * 1 rad, op: new)",
    ] {
        assert!(t.contains(want), "{want}");
    }
    // The transverse profile of a helical gear is wider than the normal module suggests.
    let ev = s.evaluate(&Actor::User).unwrap();
    assert!((ev.params["hel_pitch_diameter"].as_ref().unwrap().value - 40.0 / 20f64.to_radians().cos()).abs() < 1e-9);
    assert_eq!(ev.sketches["Hel_profile"].status, SketchStatus::WellConstrained);
    let doc = Document::from_text(&t).unwrap();
    assert_eq!(doc.to_text(), t);
    // Left-hand: negative helix angle.
    run(&mut s, "generate.gear", json!({"helix": "-15 deg", "name": "Left"}));
    assert!(text(&s).contains("param left_helix = -15 deg"));
}

#[test]
fn bad_options_are_clean_errors() {
    let mut s = session();
    for (args, why) in [
        (json!({"teeth": "8"}), "undercut"),
        (json!({"teeth": "4"}), "out of range"),
        (json!({"teeth": "20.5"}), "whole number"),
        (json!({"teeth": "2 mm"}), "teeth"),
        (json!({"module": "-2 mm"}), "module"),
        (json!({"module": "banana"}), "banana"),
        (json!({"bore": "60 mm"}), "bore"),
        (json!({"bore": "-1 mm"}), "bore"),
        (json!({"pressure_angle": "20"}), "an angle"),
        (json!({"module": "2"}), "a length"),
        (json!({"pressure_angle": "89 deg"}), "pressure angle"),
        (json!({"face_width": "0 mm"}), "face width"),
        (json!({"shift": "5"}), "shift"),
        (json!({"backlash": "5 mm"}), "backlash"),
        (json!({"fillet": "2"}), "fillet"),
        (json!({"helix": "70 deg"}), "helix"),
        (json!({"name": "9x"}), "name"),
        (json!({"unknown": 1}), "unknown"),
    ] {
        let e = fail(&mut s, args.clone());
        assert!(e.contains(why), "{args}: {e}");
        assert!(!text(&s).contains("gear1_"), "{args} left something behind");
    }
    // The hint of an undercut tells how to fix it, and the fix works.
    let e = fail(&mut s, json!({"teeth": "10"}));
    assert!(e.contains("at least") && e.contains("shift"), "{e}");
    run(&mut s, "generate.gear", json!({"teeth": "10", "shift": "0.6"}));
    // Names that are taken.
    let e = fail(&mut s, json!({"name": "Gear1"}));
    assert!(e.contains("already exists"), "{e}");
}

/// Replaces `param <name> = ...` in the code view text (no stored solution: the evaluator
/// must reach the new shape from the generation-time coordinates).
fn set_param_text(s: &mut Session, name: &str, value: &str) {
    let t = text(s);
    let start = t.find(&format!("param {name} = ")).unwrap();
    let end = start + t[start..].find("  #").unwrap();
    let new = format!("{}param {name} = {value}{}", &t[..start], &t[end..]);
    run(s, "document.set_text", json!({ "text": new }));
}

/// Review v0.2 H1: edits that make the gear smaller (and large jumps either way) solve,
/// whether the change comes as a parameter edit or as text.
#[test]
fn smaller_and_far_parameter_edits_solve() {
    let cases: [(&str, &str, &str, f64, u32); 9] = [
        ("20", "gear1_teeth", "18", 2.0, 18),
        ("30", "gear1_teeth", "28", 2.0, 28),
        ("30", "gear1_teeth", "20", 2.0, 20),
        ("100", "gear1_teeth", "95", 2.0, 95),
        ("100", "gear1_teeth", "20", 2.0, 20),
        ("20", "gear1_teeth", "100", 2.0, 100),
        ("20", "gear1_module", "1.5 mm", 1.5, 20),
        ("20", "gear1_module", "1 mm", 1.0, 20),
        ("20", "gear1_module", "5 mm", 5.0, 20),
    ];
    for by_text in [true, false] {
        for (start, name, value, module, teeth) in cases {
            let mut s = session();
            run(&mut s, "generate.gear", json!({ "teeth": start }));
            let t0 = std::time::Instant::now();
            if by_text {
                set_param_text(&mut s, name, value);
            } else {
                run(&mut s, "param.set", json!({ "name": name, "expr": value }));
            }
            let area = tooth_area(&mut s, "Gear1_profile");
            let want = reference_area(&GearValues { module, ..defaults(teeth) }, 6.0);
            assert!((area - want).abs() < 0.1 * module, "{start} -> {name} = {value}: {area} vs {want}");
            eprintln!("{start} -> {name} = {value} (text {by_text}): {:?}", t0.elapsed());
        }
    }
}

/// The evaluation error of the profile sketch `Gear1_profile`, if any.
fn profile_error(s: &mut Session) -> Option<gusset_eval::EvalError> {
    let ev = s.evaluate(&Actor::User).unwrap();
    ev.errors().into_iter().find(|e| e.feature == "Gear1_profile").cloned()
}

/// Review v0.2 M1: an edit that makes the gear impossible fails the profile sketch with
/// the generator's message and hint, on every rebuild, instead of solving to a wrong
/// profile.
#[test]
fn impossible_parameter_edits_fail_with_the_generator_hint() {
    let mut s = session();
    run(&mut s, "generate.gear", json!({"teeth": "12", "shift": "0.4"}));
    assert!(text(&s).contains("sketch Gear1_profile (plane: XY, next_id: ") && text(&s).contains(", gear: gear1)"));
    assert!(profile_error(&mut s).is_none());
    let v12 = GearValues { shift: 0.4, ..defaults(12) };
    // Undercut by a smaller shift: still well constrained before this check existed.
    for (name, value, v) in [
        ("gear1_shift", "0.25", GearValues { shift: 0.25, ..v12 }),
        ("gear1_shift", "0.2", GearValues { shift: 0.2, ..v12 }),
        ("gear1_teeth", "9", GearValues { teeth: 9, shift: 0.2, ..v12 }),
    ] {
        run(&mut s, "param.set", json!({"name": name, "expr": value}));
        let e = profile_error(&mut s).unwrap_or_else(|| panic!("{name} = {value} should fail"));
        let want = Geometry::new(&v).unwrap_err();
        assert_eq!(e.code, gusset_eval::ErrorCode::InvalidGear);
        assert!(e.message.contains("undercut"), "{e:?}");
        assert_eq!((e.message.as_str(), e.hint.as_deref()), (want.message.as_str(), Some(want.hint.as_str())));
    }
    // The hint's way out works.
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": "12"}));
    run(&mut s, "param.set", json!({"name": "gear1_shift", "expr": "0.4"}));
    assert!(profile_error(&mut s).is_none());
    let _ = tooth_area(&mut s, "Gear1_profile");
    // Pointed teeth, a bore that does not fit, a tooth count that is not whole.
    for (name, value, why) in [
        ("gear1_shift", "1.5", "point"),
        ("gear1_bore", "24 mm", "bore"),
        ("gear1_teeth", "12.5", "whole number"),
        ("gear1_fillet", "0.7", "fillet"),
    ] {
        let before = text(&s);
        run(&mut s, "param.set", json!({"name": name, "expr": value}));
        let e = profile_error(&mut s).unwrap_or_else(|| panic!("{name} = {value} should fail"));
        assert!(e.message.contains(why) && e.hint.is_some(), "{name}: {e:?}");
        run(&mut s, "document.set_text", json!({ "text": before }));
        assert!(profile_error(&mut s).is_none());
    }
    // The checked parameters are in use: deleting one is refused.
    let e = execute(&mut s, &CmdCtx::user(1), "param.delete", json!({"name": "gear1_backlash"})).unwrap_err();
    assert!(format!("{e:?}").contains("Gear1_profile"), "{e:?}");
}

/// The check sees a helical gear's transverse profile, as its sketch does.
#[test]
fn helical_gears_are_checked_in_the_transverse_plane() {
    let mut s = session();
    run(&mut s, "generate.gear", json!({"teeth": "16", "helix": "30 deg"}));
    assert!(profile_error(&mut s).is_none());
    let _ = tooth_area(&mut s, "Gear1_profile");
    // The transverse pressure angle of a helical gear is larger, so it can have fewer teeth
    // than a spur gear before undercut sets in.
    let hel = GearValues { helix: 30f64.to_radians(), ..defaults(16) };
    let (zh, zs) = (gear::min_teeth(&hel), gear::min_teeth(&defaults(16)));
    assert!(zh < zs && zh > 7, "{zh} {zs}");
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": zh.to_string()}));
    assert!(profile_error(&mut s).is_none());
    run(&mut s, "param.set", json!({"name": "gear1_teeth", "expr": (zh - 1).to_string()}));
    let e = profile_error(&mut s).expect("undercut");
    let want = Geometry::new(&GearValues { teeth: zh - 1, ..hel }).unwrap_err();
    assert_eq!((e.message, e.hint), (want.message, Some(want.hint)));
}

/// Review v0.2 L9: a tip land between the hard limit (0.1 m) and 0.25 m is generated with a
/// warning.
#[test]
fn narrow_tip_lands_are_warned_about() {
    let mut s = session();
    let r = run(&mut s, "generate.gear", json!({}));
    assert_eq!(r["warnings"], json!([]));
    let shift = (10..40)
        .map(|k| f64::from(k) * 0.05)
        .find(|&x| {
            Geometry::new(&GearValues { shift: x, ..defaults(20) })
                .is_ok_and(|g| g.tip_land() < gear::NARROW_TIP_LAND * 2.0)
        })
        .unwrap();
    let r = run(&mut s, "generate.gear", json!({"shift": format!("{shift}"), "name": "Thin"}));
    let w = r["warnings"].to_string();
    assert!(w.contains("tip land") && w.contains("profile shift"), "{w}");
}
