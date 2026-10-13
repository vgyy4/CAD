//! Region arrangement and trim through the registry (mock kernel).

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0))
}

fn run(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> Value {
    execute(s, ctx, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn fail(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> CommandError {
    execute(s, ctx, id, args).expect_err("expected an error")
}

fn regions(s: &mut Session, ctx: &CmdCtx, sketch: &str) -> Vec<(String, f64)> {
    let q = run(s, ctx, "query.sketch", json!({"sketch": sketch}));
    let mut v: Vec<(String, f64)> = q["regions"]
        .as_array()
        .unwrap_or_else(|| panic!("{q}"))
        .iter()
        .map(|r| (r["id"].as_str().unwrap().to_string(), r["area_mm2"].as_f64().unwrap()))
        .collect();
    v.sort_by(|a, b| a.1.total_cmp(&b.1));
    v
}

#[test]
fn rectangle_dimensions_can_come_from_expressions() {
    let mut s = session();
    let ctx = CmdCtx::user(1);
    run(&mut s, &ctx, "param.set", json!({"name": "w", "expr": "20 mm"}));
    run(&mut s, &ctx, "sketch.create", json!({"plane": "XY"}));
    let result = run(
        &mut s,
        &ctx,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width_expr": "w", "height_expr": "10 mm"}),
    );
    assert!(result["lines"].as_array().is_some(), "{result}");
    for args in [
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "height": 10}),
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 10}),
    ] {
        let error = fail(&mut s, &ctx, "sketch.add_rectangle", args);
        assert!(error.message.contains("rectangle"), "{error:?}");
    }
}

fn volume(s: &mut Session, ctx: &CmdCtx) -> f64 {
    run(s, ctx, "query.body", json!({}))["volume_mm3"].as_f64().unwrap()
}

#[test]
fn overlapping_rectangles_are_selectable_regions_with_stable_ids() {
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "param.set", json!({"name": "w", "expr": "20 mm"}));
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    run(
        &mut s,
        &u,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 20, "height": 10, "width_expr": "w", "height_expr": "10 mm"}),
    );
    run(
        &mut s,
        &u,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 10, "y": 5, "width": 20, "height": 10, "width_expr": "20 mm", "height_expr": "10 mm"}),
    );
    let q = run(&mut s, &u, "query.sketch", json!({"sketch": "Sketch1"}));
    let regs = q["regions"].as_array().unwrap();
    assert_eq!(regs.len(), 3, "{q}");
    for r in regs {
        assert!(r["at"].is_array(), "{r}");
    }
    let before = regions(&mut s, &u, "Sketch1");
    // The overlap is the 10 x 5 region; pick it by id and extrude it alone.
    assert!((before[0].1 - 50.0).abs() < 1e-6, "{before:?}");
    let overlap = before[0].0.clone();
    let e = run(
        &mut s,
        &u,
        "feature.extrude",
        json!({"sketch": "Sketch1", "regions": [overlap.clone()], "distance": "2 mm", "body_name": "Lens"}),
    );
    assert_eq!(e["rebuild_ok"], true, "{e}");
    assert!((volume(&mut s, &u) - 100.0).abs() < 1e-6);
    // Ids survive a parameter edit (the first rectangle gets wider; same crossing structure).
    run(&mut s, &u, "param.set", json!({"name": "w", "expr": "25 mm"}));
    let after = regions(&mut s, &u, "Sketch1");
    let ids = |v: &[(String, f64)]| {
        let mut x: Vec<String> = v.iter().map(|r| r.0.clone()).collect();
        x.sort();
        x
    };
    assert_eq!(ids(&before), ids(&after));
    // The solver widens the first rectangle symmetrically, so the overlap is now 12.5 x 5.
    assert!((volume(&mut s, &u) - 125.0).abs() < 1e-6, "{}", volume(&mut s, &u));
    // An unknown region is reported with the ids that exist.
    let e = fail(&mut s, &u, "feature.extrude", json!({"sketch": "Sketch1", "regions": ["l99"], "distance": "1 mm"}));
    assert_eq!(e.code, "invalid_document", "{e}");
}

#[test]
fn trim_cuts_a_line_between_two_crossings() {
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 0, "y1": 0, "x2": 30, "y2": 0}));
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 10, "y1": -5, "x2": 10, "y2": 5}));
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 20, "y1": -5, "x2": 20, "y2": 5}));
    // l3 is the horizontal line (points p1, p2); l6 and l9 the verticals.
    run(&mut s, &u, "sketch.add_constraint", json!({"sketch": "Sketch1", "kind": "horizontal", "entities": ["l3"]}));
    run(
        &mut s,
        &u,
        "sketch.add_constraint",
        json!({"sketch": "Sketch1", "kind": "distance", "entities": ["l3", "l3"], "value": "30 mm"}),
    );
    let before = s.history().head(s.history().main()).unwrap();
    let r = run(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "l3", "x": 15, "y": 0}));
    assert_eq!(r["kept"].as_array().unwrap().len(), 2, "{r}");
    // The length dimension of the trimmed line no longer applies; the orientation stays.
    assert_eq!(r["constraints_dropped"].as_array().unwrap().len(), 1, "{r}");
    // One history step.
    let head = s.history().head(s.history().main()).unwrap();
    assert_eq!(s.history().node(head).unwrap().parent, Some(before));
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("on_line("), "{text}");
    let q = run(&mut s, &u, "query.sketch", json!({"sketch": "Sketch1"}));
    // p12 and p14 are the new ends, on the vertical lines l6 (p4-p5) and l9 (p7-p8).
    assert_eq!(q["points"]["p12"], json!([10.0, 0.0]), "{q}");
    assert_eq!(q["points"]["p14"], json!([20.0, 0.0]), "{q}");
    // The new ends follow the crossing lines when one of them moves.
    run(&mut s, &u, "sketch.move_point", json!({"sketch": "Sketch1", "point": "p5", "x": 12, "y": 5}));
    let q = run(&mut s, &u, "query.sketch", json!({"sketch": "Sketch1"}));
    let p10 = q["points"]["p12"].as_array().unwrap();
    // p12 must lie on the vertical line l6 wherever it moved.
    let (p3, p4) = (q["points"]["p4"].as_array().unwrap(), q["points"]["p5"].as_array().unwrap());
    let cross = (p4[0].as_f64().unwrap() - p3[0].as_f64().unwrap())
        * (p10[1].as_f64().unwrap() - p3[1].as_f64().unwrap())
        - (p4[1].as_f64().unwrap() - p3[1].as_f64().unwrap()) * (p10[0].as_f64().unwrap() - p3[0].as_f64().unwrap());
    assert!(cross.abs() < 1e-6, "{q}");
    // Undo twice (the move, then the trim) restores the original line.
    run(&mut s, &u, "history.undo", json!({}));
    run(&mut s, &u, "history.undo", json!({}));
    assert!(!s.document(&gusset_history::Actor::User).unwrap().to_text().contains("on_line("));
}

#[test]
fn trim_turns_a_circle_into_an_arc_and_keeps_its_radius() {
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    run(
        &mut s,
        &u,
        "sketch.add_circle",
        json!({"sketch": "Sketch1", "cx": 0, "cy": 0, "radius": 10, "radius_expr": "10 mm"}),
    );
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": -15, "y1": 5, "x2": 15, "y2": 5}));
    let regs = regions(&mut s, &u, "Sketch1");
    assert_eq!(regs.len(), 2, "{regs:?}");
    // Remove the small cap above the chord.
    let r = run(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "c2", "x": 0, "y": 10}));
    assert_eq!(r["deleted"], json!(["c2"]), "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("= arc(p1,"), "{text}");
    assert!(text.contains("radius(a"), "the radius dimension moved to the arc: {text}");
    // The arc and the chord now bound one region (circle minus cap).
    let regs = regions(&mut s, &u, "Sketch1");
    assert_eq!(regs.len(), 1, "{regs:?}");
    assert!(regs[0].1 > 250.0 && regs[0].1 < 314.2, "{regs:?}");
}

#[test]
fn trim_without_crossings_deletes_and_bad_picks_are_clean_errors() {
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 0, "y1": 0, "x2": 30, "y2": 0}));
    let r = run(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "l3", "x": 5, "y": 0}));
    assert_eq!(r["deleted"].as_array().unwrap().len(), 3, "{r}");
    let doc = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(!doc.contains("line("), "{doc}");
    let e = fail(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "l3", "x": 5, "y": 0}));
    assert_eq!(e.code, "not_found");
    let e = fail(&mut s, &u, "sketch.trim", json!({"sketch": "Nope", "entity": "l3", "x": 5, "y": 0}));
    assert_eq!(e.code, "not_found");
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 0, "y1": 0, "x2": 30, "y2": 0}));
    let e = fail(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "p4", "x": 0, "y": 0}));
    assert!(e.message.contains("not a line, arc or circle") || e.message.contains("point"), "{e}");
    let e = fail(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "l6", "x": f64::NAN, "y": 0}));
    assert_eq!(e.code, "invalid_args");
}

#[test]
fn trim_keeps_the_end_on_an_existing_point_at_a_t_junction() {
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    // A long horizontal line and a vertical line ending exactly on it.
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 0, "y1": 0, "x2": 30, "y2": 0}));
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 12, "y1": 0, "x2": 12, "y2": 8}));
    // Trim the horizontal line's left part up to the T-junction.
    let r = run(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "l3", "x": 4, "y": 0}));
    assert_eq!(r["kept"], json!(["l3"]), "{r}");
    // The line now starts at the vertical line's foot point (no new point).
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("l3 = line(p4, p2)"), "{text}");
    assert!(
        !r["created"].as_array().is_some_and(|c| c.iter().any(|x| x.as_str().is_some_and(|x| x.starts_with('p')))),
        "{r}"
    );
}

#[test]
fn splines_are_ordinary_points_plus_a_curve_and_form_regions() {
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    let e = fail(
        &mut s,
        &u,
        "sketch.add_polyline",
        json!({"sketch": "Sketch1", "points": [[0, 0], [5, 5]], "spline": true}),
    );
    assert_eq!(e.code, "invalid_args");
    assert!(e.hint.is_some(), "{e:?}");
    let r = run(
        &mut s,
        &u,
        "sketch.add_polyline",
        json!({"sketch": "Sketch1", "points": [[10, 0], [7, 7], [0, 10], [-7, 7], [-10, 0], [-7, -7], [0, -10], [7, -7]], "spline": true, "closed": true}),
    );
    assert_eq!(r["spline"], "s9", "{r}");
    assert_eq!(r["points"]["count"], 8, "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("s9 = spline(p1, p2, p3, p4, p5, p6, p7, p8) closed"), "{text}");
    let regs = regions(&mut s, &u, "Sketch1");
    assert_eq!(regs.len(), 1, "{regs:?}");
    // Close to a circle of radius ~10 through the same points.
    assert!(regs[0].1 > 280.0 && regs[0].1 < 330.0, "{regs:?}");
    // The fit points are ordinary points: constrain one and move another.
    run(&mut s, &u, "sketch.add_constraint", json!({"sketch": "Sketch1", "kind": "fixed", "entities": ["p1"]}));
    run(&mut s, &u, "sketch.move_point", json!({"sketch": "Sketch1", "point": "p3", "x": 0, "y": 14}));
    let regs2 = regions(&mut s, &u, "Sketch1");
    assert!(regs2[0].1 > regs[0].1, "{regs2:?}");
    // Splines take no trim and no line constraints, with clear errors.
    let e = fail(&mut s, &u, "sketch.trim", json!({"sketch": "Sketch1", "entity": "s9", "x": 10, "y": 0}));
    assert!(e.message.contains("spline") || e.message.contains("trimmed"), "{e:?}");
    let e = run(
        &mut s,
        &u,
        "sketch.add_constraint",
        json!({"sketch": "Sketch1", "kind": "horizontal", "entities": ["s9"]}),
    );
    assert_eq!(e["rebuild_ok"], false, "{e}");
    assert!(e["new_errors"].to_string().contains("must be a line"), "{e}");
    run(&mut s, &u, "history.undo", json!({}));
    // A line crossing it makes two regions.
    run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": -20, "y1": 2, "x2": 20, "y2": 2}));
    assert_eq!(regions(&mut s, &u, "Sketch1").len(), 2);
    // Deleting a fit point deletes the spline.
    let d = run(&mut s, &u, "sketch.delete_items", json!({"sketch": "Sketch1", "ids": ["p5"]}));
    assert!(d["deleted"].to_string().contains("s9"), "{d}");
}

#[test]
fn the_mock_kernel_reports_splines_as_unsupported_not_a_panic() {
    let mut s = session();
    let u = CmdCtx::user(1);
    run(&mut s, &u, "sketch.create", json!({"plane": "XY"}));
    run(
        &mut s,
        &u,
        "sketch.add_polyline",
        json!({"sketch": "Sketch1", "points": [[10, 0], [0, 10], [-10, 0], [0, -10]], "spline": true, "closed": true}),
    );
    let e = run(&mut s, &u, "feature.extrude", json!({"sketch": "Sketch1", "distance": "2 mm"}));
    assert_eq!(e["rebuild_ok"], false, "{e}");
    assert!(e["new_errors"].to_string().contains("straight edges"), "{e}");
}

fn rect_sketch(s: &mut Session, u: &CmdCtx) {
    run(s, u, "param.set", json!({"name": "n", "expr": "3"}));
    run(s, u, "param.set", json!({"name": "pitch", "expr": "30 mm"}));
    run(s, u, "sketch.create", json!({"plane": "XY"}));
    run(
        s,
        u,
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 10, "height": 6, "width_expr": "10 mm", "height_expr": "6 mm"}),
    );
}

#[test]
fn linear_sketch_pattern_makes_copies_that_follow_parameters() {
    let mut s = session();
    let u = CmdCtx::user(1);
    rect_sketch(&mut s, &u);
    // The rectangle's lines are l5..l8.
    let r = run(
        &mut s,
        &u,
        "sketch.pattern",
        json!({"sketch": "Sketch1", "kind": "linear", "entities": ["l5", "l6", "l7", "l8"], "count": "n", "dx": "pitch"}),
    );
    assert_eq!(r["pattern"], "g15", "{r}");
    assert_eq!(r["copies"], 2.0, "{r}");
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    assert!(text.contains("g15 = pattern_linear(of: [l5, l6, l7, l8], count: n, dx: pitch, dy: 0 mm)"), "{text}");
    let regs = regions(&mut s, &u, "Sketch1");
    assert_eq!(regs.len(), 3, "{regs:?}");
    assert!(regs.iter().all(|r| (r.1 - 60.0).abs() < 1e-6), "{regs:?}");
    assert!(regs.iter().any(|r| r.0 == "l5_2"), "copy ids are l5_2 etc: {regs:?}");
    let q = run(&mut s, &u, "query.sketch", json!({"sketch": "Sketch1"}));
    assert_eq!(q["points"]["p1_2"], json!([60.0, 0.0]), "{q}");
    // Extrude all three copies, then change count and spacing: the solid follows.
    let e = run(&mut s, &u, "feature.extrude", json!({"sketch": "Sketch1", "distance": "2 mm"}));
    assert_eq!(e["rebuild_ok"], true, "{e}");
    assert!((volume(&mut s, &u) - 3.0 * 60.0 * 2.0).abs() < 1e-6);
    run(&mut s, &u, "param.set", json!({"name": "n", "expr": "5"}));
    assert!((volume(&mut s, &u) - 5.0 * 60.0 * 2.0).abs() < 1e-6, "{}", volume(&mut s, &u));
    run(&mut s, &u, "param.set", json!({"name": "pitch", "expr": "12 mm"}));
    // 12 mm pitch overlaps 10 mm wide copies? no: they are 2 mm apart, still separate.
    assert!((volume(&mut s, &u) - 5.0 * 60.0 * 2.0).abs() < 1e-6);
    // One copy by id.
    let e = run(
        &mut s,
        &u,
        "feature.extrude",
        json!({"sketch": "Sketch1", "regions": ["l5_3"], "distance": "1 mm", "body_name": "One"}),
    );
    assert_eq!(e["rebuild_ok"], true, "{e}");
    // A count that is not a whole number fails the sketch with a hint instead of rounding.
    let bad = run(&mut s, &u, "param.set", json!({"name": "n", "expr": "2.5"}));
    assert_eq!(bad["rebuild_ok"], false, "{bad}");
    assert!(bad["new_errors"].to_string().contains("whole number"), "{bad}");
    run(&mut s, &u, "history.undo", json!({}));
    // Deleting the pattern's sources is refused while a feature names one of its copies.
    let e = fail(&mut s, &u, "sketch.delete_items", json!({"sketch": "Sketch1", "ids": ["l5", "l6", "l7", "l8"]}));
    assert_eq!(e.code, "invalid_document", "{e:?}");
    run(&mut s, &u, "history.undo", json!({}));
    // Deleting a source removes it from the pattern; deleting all removes the pattern.
    let d = run(&mut s, &u, "sketch.delete_items", json!({"sketch": "Sketch1", "ids": ["l5", "l6", "l7", "l8"]}));
    assert!(d["deleted"].to_string().contains("l5"), "{d}");
    assert!(!s.document(&gusset_history::Actor::User).unwrap().to_text().contains("pattern_linear"));
}

#[test]
fn circular_sketch_pattern_and_clean_errors() {
    let mut s = session();
    let u = CmdCtx::user(1);
    rect_sketch(&mut s, &u);
    let c = run(&mut s, &u, "sketch.add_line", json!({"sketch": "Sketch1", "x1": 50, "y1": 50, "x2": 51, "y2": 50}));
    let _ = c;
    // Centre: the origin point of the rectangle's first corner is p1; rotate the rectangle
    // about a far point p9 four times.
    let p = run(
        &mut s,
        &u,
        "sketch.add_line",
        json!({"sketch": "Sketch1", "x1": 100, "y1": 100, "x2": 101, "y2": 100, "construction": true}),
    );
    let centre = p["points"][0].as_str().unwrap().to_string();
    for (args, why) in [
        (json!({"kind": "linear", "entities": ["l5"], "count": "1", "dx": "5 mm"}), "whole number from 2"),
        (json!({"kind": "linear", "entities": ["l5"], "count": "2.5", "dx": "5 mm"}), "whole number"),
        (json!({"kind": "linear", "entities": ["l5"], "count": "2"}), "overlap"),
        (json!({"kind": "linear", "entities": ["p1"], "count": "2", "dx": "5 mm"}), "point"),
        (json!({"kind": "linear", "entities": ["l99"], "count": "2", "dx": "5 mm"}), "no entity"),
        (json!({"kind": "linear", "entities": [], "count": "2", "dx": "5 mm"}), "non-empty"),
        (json!({"kind": "circular", "entities": ["l5"], "count": "3"}), "center"),
        (json!({"kind": "circular", "entities": ["l5"], "count": "3", "center": "l6"}), "not a point"),
        (json!({"kind": "spiral", "entities": ["l5"], "count": "3"}), "linear or circular"),
        (json!({"kind": "linear", "entities": ["l5"], "count": "501", "dx": "5 mm"}), "from 2 to"),
        (json!({"kind": "linear", "entities": ["l5"], "count": "3", "dx": "5 foo"}), "dx"),
    ] {
        let mut a = args.clone();
        a["sketch"] = json!("Sketch1");
        let e = fail(&mut s, &u, "sketch.pattern", a);
        assert!(e.message.contains(why), "{why}: {e:?}");
        assert!(matches!(e.code.as_str(), "invalid_args" | "not_found"), "{e:?}");
    }
    run(
        &mut s,
        &u,
        "sketch.pattern",
        json!({"sketch": "Sketch1", "kind": "circular", "entities": ["l5", "l6", "l7", "l8"], "count": "4", "center": centre, "angle": "270 deg"}),
    );
    assert_eq!(regions(&mut s, &u, "Sketch1").len(), 4);
    // A curve belongs to one pattern.
    let e = fail(
        &mut s,
        &u,
        "sketch.pattern",
        json!({"sketch": "Sketch1", "kind": "linear", "entities": ["l5"], "count": "2", "dx": "5 mm"}),
    );
    assert!(e.message.contains("already belongs"), "{e:?}");
    // The text round-trips.
    let text = s.document(&gusset_history::Actor::User).unwrap().to_text();
    let again = gusset_doc::Document::from_text(&text).unwrap();
    assert_eq!(again.to_text(), text);
}
