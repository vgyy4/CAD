//! The hole command through the registry (mock kernel: the statement and its parameters are
//! written and the arguments are checked; the geometry is tested with OCCT in gusset-cli).

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> (Session, CmdCtx) {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    run(
        &mut s,
        &u,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20, "width_expr": "40 mm", "height_expr": "20 mm"}),
    );
    run(&mut s, &u, "feature.extrude", json!({"sketch": "Sketch1", "distance": "12 mm"}));
    (s, u)
}

fn run(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> Value {
    execute(s, ctx, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn fail(s: &mut Session, ctx: &CmdCtx, args: Value) -> CommandError {
    match execute(s, ctx, "feature.hole", args) {
        Ok(v) => panic!("feature.hole should fail: {v}"),
        Err(e) => e,
    }
}

fn text(s: &Session) -> String {
    s.document(&gusset_history::Actor::User).unwrap().to_text()
}

const TOP: &str = "Extrude1/cap(end)";

#[test]
fn the_command_is_registered_with_a_short_description() {
    let spec = find("feature.hole").unwrap();
    assert!(spec.description.len() > 40 && spec.description.len() < 300, "{}", spec.description.len());
    let schema = serde_json::to_string(&(spec.schema)()).unwrap();
    assert!(schema.contains("properties") && schema.contains("cbore_depth"), "{schema}");
}

#[test]
fn preset_values_become_ordinary_parameters() {
    let (mut s, u) = session();
    let r = run(
        &mut s,
        &u,
        "feature.hole",
        json!({"face": TOP, "at": [["10 mm", "10 mm"]], "standard": "ISO M6 clearance normal"}),
    );
    assert_eq!(r["feature"], "Hole1");
    assert_eq!(r["standard"], "ISO M6 clearance normal");
    assert_eq!(r["parameters"], json!(["hole1_diameter"]));
    let t = text(&s);
    assert!(t.contains("param hole1_diameter = 6.6 mm  # ISO 273 normal clearance hole for M6"), "{t}");
    assert!(
        t.contains("hole Hole1 (face: Extrude1/cap(end), at: [[10 mm, 10 mm]], type: simple, diameter: hole1_diameter, through: true)"),
        "{t}"
    );
    assert!(t.contains("# ISO M6 clearance normal (ISO 273)."), "{t}");
    // The parameter is editable like any other.
    run(&mut s, &u, "param.set", json!({"name": "hole1_diameter", "expr": "6.4 mm"}));
    assert!(text(&s).contains("param hole1_diameter = 6.4 mm"));

    // A tapped preset: tap drill, pitch, thread depth, drilled depth, point angle, designation.
    run(
        &mut s,
        &u,
        "feature.hole",
        json!({"face": TOP, "at": [["30 mm", "10 mm"]], "standard": "ISO M8 tapped", "name": "Tap"}),
    );
    let t = text(&s);
    for want in [
        "param tap_diameter = 6.8 mm",
        "param tap_pitch = 1.25 mm",
        "param tap_thread_depth = 12 mm",
        "param tap_depth = 16 mm",
        "param tap_point_angle = 118 deg",
        "type: tapped, diameter: tap_diameter, depth: tap_depth, point_angle: tap_point_angle, thread: \"M8\", pitch: tap_pitch, thread_depth: tap_thread_depth",
    ] {
        assert!(t.contains(want), "{want}\n{t}");
    }
}

#[test]
fn explicit_arguments_override_the_preset() {
    let (mut s, u) = session();
    run(
        &mut s,
        &u,
        "feature.hole",
        json!({"face": TOP, "at": [["10 mm", "10 mm"]], "standard": "ISO M6 counterbore DIN 974/ISO 4762 head",
               "diameter": "7 mm", "cbore_depth": "7 mm", "depth": "10 mm"}),
    );
    let t = text(&s);
    assert!(t.contains("param hole1_diameter = 7 mm  # set explicitly; the standard has 6.6 mm"), "{t}");
    assert!(t.contains("param hole1_cbore_diameter = 11 mm"), "{t}");
    assert!(t.contains("param hole1_cbore_depth = 7 mm"), "{t}");
    assert!(t.contains("param hole1_depth = 10 mm"), "{t}");
    assert!(t.contains("type: counterbore, diameter: hole1_diameter, depth: hole1_depth, point_angle: hole1_point_angle, cbore_diameter: hole1_cbore_diameter, cbore_depth: hole1_cbore_depth"), "{t}");
    // Through wins over a tapped preset's depth, and drops its thread length.
    run(
        &mut s,
        &u,
        "feature.hole",
        json!({"face": TOP, "at": [["30 mm", "10 mm"]], "standard": "ISO M6 tapped", "through": true, "name": "T"}),
    );
    let t = text(&s);
    assert!(t.contains("type: tapped, diameter: t_diameter, through: true, thread: \"M6\", pitch: t_pitch)"), "{t}");
    assert!(!t.contains("t_thread_depth") && !t.contains("t_depth"), "{t}");
    // A countersink preset.
    run(
        &mut s,
        &u,
        "feature.hole",
        json!({"face": TOP, "at": [["20 mm", "10 mm"]], "standard": "ISO M5 countersink 90 deg", "name": "Cs"}),
    );
    let t = text(&s);
    assert!(t.contains("param cs_csink_diameter = 11.2 mm") && t.contains("param cs_csink_angle = 90 deg"), "{t}");
}

#[test]
fn without_a_standard_the_expressions_go_into_the_statement() {
    let (mut s, u) = session();
    run(&mut s, &u, "param.set", json!({"name": "w", "expr": "5 mm"}));
    let r = run(
        &mut s,
        &u,
        "feature.hole",
        json!({"face": TOP, "at": [["w", "2 * w"]], "diameter": "w / 2", "depth": "8 mm", "point_angle": "120 deg"}),
    );
    assert_eq!(r["parameters"], json!([]));
    assert!(
        text(&s).contains("hole Hole1 (face: Extrude1/cap(end), at: [[w, 2 * w]], type: simple, diameter: w / 2, depth: 8 mm, point_angle: 120 deg)"),
        "{}",
        text(&s)
    );
    // A sketch for the points.
    run(&mut s, &u, "sketch.create", json!({"plane": TOP}));
    run(&mut s, &u, "sketch.add_circle", json!({"sketch": "Sketch2", "cx": 20, "cy": 10, "radius": 2}));
    let r =
        run(&mut s, &u, "feature.hole", json!({"face": TOP, "points": "Sketch2", "diameter": "3 mm", "through": true}));
    assert_eq!(r["feature"], "Hole2");
    // The mock kernel cannot revolve: the feature is there and says why.
    assert_eq!(r["rebuild_ok"], false, "{r}");
    assert_eq!(r["committed_with_errors"], true);
}

#[test]
fn mistakes_are_refused_cleanly_and_change_nothing() {
    let (mut s, u) = session();
    let before = text(&s);
    let at = json!([["10 mm", "10 mm"]]);
    let cases: Vec<(Value, &str)> = vec![
        (json!({"face": TOP, "diameter": "3 mm", "through": true}), "either points"),
        (json!({"face": TOP, "at": at, "points": "Sketch1", "diameter": "3 mm", "through": true}), "either points"),
        (json!({"face": TOP, "at": [], "diameter": "3 mm", "through": true}), "no points"),
        (json!({"face": "Extrude1/nope", "at": at, "diameter": "3 mm", "through": true}), "face"),
        (json!({"face": "Extrude9/cap(end)", "at": at, "diameter": "3 mm", "through": true}), "does not exist"),
        (json!({"face": TOP, "at": at, "through": true}), "needs diameter"),
        (json!({"face": TOP, "at": at, "diameter": "3 mm"}), "depth or through"),
        (json!({"face": TOP, "at": at, "diameter": "3 mm", "depth": "4 mm", "through": true}), "not both"),
        (json!({"face": TOP, "at": at, "diameter": "3 mm", "through": true, "point_angle": "118 deg"}), "blind"),
        (json!({"face": TOP, "at": at, "diameter": "3 mm", "through": true, "type": "drilled"}), "unknown hole type"),
        (
            json!({"face": TOP, "at": at, "diameter": "3 mm", "through": true, "type": "counterbore"}),
            "needs cbore_diameter",
        ),
        (
            json!({"face": TOP, "at": at, "diameter": "3 mm", "through": true, "cbore_depth": "1 mm"}),
            "only applies to type counterbore",
        ),
        (
            json!({"face": TOP, "at": at, "diameter": "3 mm", "through": true, "type": "tapped", "pitch": "0.5 mm"}),
            "needs thread",
        ),
        (
            json!({"face": TOP, "at": at, "diameter": "3 mm", "through": true, "thread": "M3"}),
            "only applies to type tapped",
        ),
        (json!({"face": TOP, "at": at, "standard": "ISO M7 clearance"}), "no thread M7"),
        (json!({"face": TOP, "at": at, "standard": "ISO M6 tapped", "type": "counterbore"}), "is a tapped hole"),
        (json!({"face": TOP, "at": at, "standard": "ISO M2 counterbore"}), "no counterbore for M2"),
        (json!({"face": TOP, "at": at, "diameter": "3 mm bananas", "through": true}), "diameter"),
        // Rejected by the trial evaluation.
        (
            json!({"face": TOP, "at": [["80 mm", "10 mm"]], "diameter": "3 mm", "through": true}),
            "does not fit inside face",
        ),
        (
            json!({"face": TOP, "at": [["3 mm", "10 mm"]], "diameter": "8 mm", "through": true}),
            "does not fit inside face",
        ),
        (
            json!({"face": TOP, "at": at, "diameter": "6 mm", "through": true, "type": "counterbore", "cbore_diameter": "5 mm", "cbore_depth": "2 mm"}),
            "not larger",
        ),
        (
            json!({"face": TOP, "at": at, "diameter": "6 mm", "depth": "2 mm", "type": "counterbore", "cbore_diameter": "9 mm", "cbore_depth": "3 mm"}),
            "not less than the hole depth",
        ),
        (
            json!({"face": TOP, "at": at, "diameter": "6 mm", "through": true, "type": "countersink", "csink_diameter": "9 mm", "csink_angle": "190 deg"}),
            "countersink angle",
        ),
    ];
    for (args, why) in cases {
        let e = fail(&mut s, &u, args.clone());
        let all = format!("{e:?}");
        assert!(all.contains(why), "{args}: {why}: {all}");
        if !matches!(e.code.as_str(), "invalid_args" | "not_found" | "invalid_document") {
            panic!("{args}: unexpected code {}", e.code);
        }
        if !all.contains("bananas") && !all.contains("Extrude1/nope") {
            assert!(e.hint.is_some(), "{args}: no hint: {all}");
        }
    }
    assert_eq!(text(&s), before, "a refused hole must change nothing");
    // A parameter name that is taken.
    run(&mut s, &u, "param.set", json!({"name": "hole1_diameter", "expr": "1 mm"}));
    let e = fail(&mut s, &u, json!({"face": TOP, "at": at, "standard": "ISO M6 clearance normal"}));
    assert!(format!("{e:?}").contains("already exists"), "{e:?}");
}

#[test]
fn feature_rename_follows_the_sketch_and_the_face() {
    let (mut s, u) = session();
    run(&mut s, &u, "sketch.create", json!({"plane": TOP}));
    run(&mut s, &u, "sketch.add_circle", json!({"sketch": "Sketch2", "cx": 20, "cy": 10, "radius": 2}));
    run(&mut s, &u, "feature.hole", json!({"face": TOP, "points": "Sketch2", "diameter": "3 mm", "through": true}));
    run(&mut s, &u, "feature.rename", json!({"from": "Sketch2", "to": "Marks"}));
    run(&mut s, &u, "feature.rename", json!({"from": "Extrude1", "to": "Plate"}));
    let t = text(&s);
    assert!(t.contains("hole Hole1 (face: Plate/cap(end), points: Marks, "), "{t}");
}
