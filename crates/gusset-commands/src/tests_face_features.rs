//! `feature.thread`, `feature.bend` and face profiles for extrude, sweep and revolve (mock kernel:
//! the text a command writes and the refusals; geometry is tested with OCCT).

use crate::*;
use gusset_doc::Document;
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

fn err(s: &mut Session, id: &str, args: Value) -> CommandError {
    execute(s, &CmdCtx::user(1), id, args).map(|v| panic!("{id} should fail: {v}")).unwrap_err()
}

fn text(s: &Session) -> String {
    s.document(&gusset_history::Actor::User).unwrap().to_text()
}

fn block(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 20, "height": 10}));
    run(s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "5 mm", "body_name": "Base"}));
}

#[test]
fn a_thread_is_written_with_its_options_and_refused_when_wrong() {
    let mut s = session();
    block(&mut s);
    let r = run(
        &mut s,
        "feature.thread",
        json!({"face": "Extrude1/cap(end)", "pitch": "1.5 mm", "designation": "M10x1.5", "left": true,
               "offset": "1 mm", "length": "4 mm", "along": "Z"}),
    );
    assert_eq!(r["feature"], "Thread1", "{r}");
    let t = text(&s);
    assert!(t.contains("thread Thread1 (face: Extrude1/cap(end), designation: \"M10x1.5\", pitch: 1.5 mm"), "{t}");
    assert!(t.contains("offset: 1 mm") && t.contains("along: Z") && t.contains("left: true"), "{t}");
    assert!(!t.contains("modeled"), "{t}");
    for (args, what) in [
        (json!({"face": "nonsense", "pitch": "1 mm"}), "face"),
        (json!({"face": "Extrude1/cap(end)", "pitch": "1 +"}), "pitch"),
        (json!({"face": "Extrude1/cap(end)", "pitch": "1 mm", "along": "Q"}), "along"),
        (json!({"face": "Extrude1/cap(end)", "pitch": "1 mm", "designation": ""}), "designation"),
        (json!({"face": "Extrude1/cap(end)", "pitch": "1 mm", "x": 1}), "unknown field"),
    ] {
        assert_eq!(err(&mut s, "feature.thread", args).code, "invalid_args", "{what}");
    }
    let e = err(&mut s, "feature.thread", json!({"face": "Ghost/side(c1)", "pitch": "1 mm"}));
    assert_ne!(e.code, "internal", "{e:?}");
}

#[test]
fn a_bend_is_written_and_validated() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.tab", json!({"profile": "Sketch1", "thickness": "2 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_polyline", json!({"sketch": "Sketch2", "points": [[20, -5], [20, 25]]}));
    let r = run(
        &mut s,
        "feature.bend",
        json!({"body": "Tab1", "line": "Sketch2.l3", "angle": "90 deg", "side": "right", "position": "start"}),
    );
    assert_eq!(r["feature"], "Bend1", "{r}");
    let t = text(&s);
    assert!(t.contains("bend Bend1 (body: Tab1, line: Sketch2.l3, angle: 90 deg, bend_radius: 2 mm"), "{t}");
    assert!(t.contains("side: right") && t.contains("position: start"), "{t}");
    for (args, what) in [
        (json!({"body": "Tab1", "line": "l3", "angle": "90 deg"}), "line without sketch"),
        (json!({"body": "Tab1", "line": "Sketch2.l3", "angle": "90 deg", "side": "up"}), "side"),
        (json!({"body": "Tab1", "line": "Sketch2.l3", "angle": "90 deg", "position": "mid"}), "position"),
        (json!({"body": "Tab1", "line": "Sketch2.l3", "angle": "90 de"}), "angle"),
        (json!({"body": "Tab1) x", "line": "Sketch2.l3", "angle": "90 deg"}), "body"),
        (json!({"body": "Sketch1", "line": "Sketch2.l3", "angle": "90 deg"}), "body is not sheet metal, no radius"),
    ] {
        assert_eq!(err(&mut s, "feature.bend", args).code, "invalid_args", "{what}");
    }
}

#[test]
fn extrude_and_sweep_take_a_face_as_the_profile() {
    let mut s = session();
    block(&mut s);
    let r =
        run(&mut s, "feature.extrude", json!({"face": "Extrude1/cap(end)", "distance": "3 mm", "body_name": "Lid"}));
    assert_eq!(r["feature"], "Extrude2", "{r}");
    let t = text(&s);
    assert!(
        t.contains("extrude Extrude2 (profile: Extrude1/cap(end), distance: 3 mm, op: new, body_name: Lid)"),
        "{t}"
    );
    let ev = s.evaluate(&gusset_history::Actor::User).unwrap();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    for args in [
        json!({"sketch": "Sketch1", "face": "Extrude1/cap(end)", "distance": "3 mm"}),
        json!({"distance": "3 mm"}),
        json!({"face": "nonsense", "distance": "3 mm"}),
    ] {
        assert_eq!(err(&mut s, "feature.extrude", args).code, "invalid_args");
    }
    // A sweep and a loft write the face profile too.
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.add_polyline", json!({"sketch": "Sketch2", "points": [[0, 0], [0, 30]]}));
    let _ = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.sweep",
        json!({"face": "Extrude1/cap(end)", "path": "Sketch2", "name": "Sw"}),
    );
    assert!(text(&s).contains("sweep Sw (profile: Extrude1/cap(end)"), "{}", text(&s));
    assert_eq!(err(&mut s, "feature.sweep", json!({"path": "Sketch2"})).code, "invalid_args");
    let _ = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.loft",
        json!({"profiles": ["Extrude1/cap(end)", "Sketch1"], "name": "Lo"}),
    );
    assert!(text(&s).contains("loft Lo (profiles: [Extrude1/cap(end), Sketch1]"), "{}", text(&s));
}

#[test]
fn revolve_takes_a_face_and_an_edge_axis() {
    let mut s = session();
    block(&mut s);
    let _ = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.revolve",
        json!({"face": "Extrude1/cap(end)", "axis": "edge(Extrude1/cap(end), Extrude1/side(Sketch1/l1))",
               "angle": "90 deg", "name": "Rv"}),
    );
    let t = text(&s);
    assert!(
        t.contains("revolve Rv (profile: Extrude1/cap(end), axis: edge(Extrude1/cap(end), Extrude1/side(Sketch1/l1)), angle: 90 deg"),
        "{t}"
    );
    for args in [
        json!({"sketch": "Sketch1", "face": "Extrude1/cap(end)", "axis": "X"}),
        json!({"axis": "X"}),
        json!({"face": "Extrude1/cap(end)", "axis": "edge(nonsense"}),
        json!({"sketch": "Sketch1", "axis": "Z"}),
    ] {
        assert_eq!(err(&mut s, "feature.revolve", args).code, "invalid_args");
    }
}
