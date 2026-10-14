#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
//! THROWAWAY review reproductions (deleted after the review).

use gusset_commands::{CmdCtx, Session, execute};
use gusset_doc::Document;
use gusset_history::{Ctx, Hash, History, NodeState};
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0))
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn plate_and_block(s: &mut Session) {
    run(s, "param.set", json!({"name": "slide", "expr": "3 mm"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm", "body_name": "Plate"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.add_rectangle", json!({"sketch": "Sketch2", "x": 100, "y": 0, "width": 10, "height": 10}));
    run(s, "feature.extrude", json!({"sketch": "Sketch2", "distance": "5 mm", "body_name": "Block"}));
}

#[test]
fn t1_configuration_with_duplicate_params() {
    let mut s = session();
    run(&mut s, "param.set", json!({"name": "w", "expr": "5 mm"}));
    let r = execute(&mut s, &CmdCtx::user(1), "configuration.save", json!({"name": "A", "params": ["w", "w"]}));
    println!("save with duplicates: {r:?}");
    println!("document(): {:?}", s.document(&gusset_history::Actor::User).map(|_| ()));
    println!("query.model: {:?}", execute(&mut s, &CmdCtx::user(1), "query.model", json!({})).map(|_| ()));
    println!(
        "param.set: {:?}",
        execute(&mut s, &CmdCtx::user(1), "param.set", json!({"name": "z", "expr": "1 mm"})).map(|_| ())
    );
    println!("undo: {:?}", execute(&mut s, &CmdCtx::user(1), "history.undo", json!({})).map(|v| v.to_string()));
    println!("document() after undo: {:?}", s.document(&gusset_history::Actor::User).map(|_| ()));
    let h = s.history();
    let head = h.head(h.main()).unwrap();
    println!("history document(head): {:?}", h.document(head).map(|d| d.configurations.len()));
    let meta = gusset_project::ProjectMeta {
        id: gusset_store::paths::ProjectId::new_random().unwrap(),
        save_seq: 1,
        base_seq: 1,
        created: 1,
    };
    let bytes = gusset_project::encode(h, &meta).unwrap();
    match gusset_project::decode(&bytes) {
        Ok((h2, _)) => {
            let head = h2.head(h2.main()).unwrap();
            println!("reload: ok, document: {:?}", h2.document(head).map(|d| d.configurations.len()));
        }
        Err(e) => println!("reload FAILED: {e}"),
    }
}

#[test]
fn t2_range_units() {
    // inch document: plain-number bounds
    let doc = Document::from_text("gusset 1\nunits in\n\nparam w = 5 in, min: 1, max: 3\nparam a = 100 deg, max: 90\nparam n = 40, min: 10 mm\nparam q = 0.5 in, max: 1 mm\nparam ok = 2 in, min: 1, max: 3\nparam ang = 45 deg, min: 10, max: 90\n").unwrap();
    let v = gusset_units::params::evaluate(&doc.params);
    for (k, r) in &v {
        println!("{k}: {r:?}");
    }
}

fn pin_box(s: &mut Session, name: &str) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    let _ = name;
}

#[test]
fn t3_failed_joint_leaves_phantom_record() {
    let mut s = session();
    plate_and_block(&mut s);
    let _ = pin_box;
    run(&mut s, "param.set", json!({"name": "slide", "expr": "12 mm"}));
    let r = run(
        &mut s,
        "feature.joint",
        json!({"type": "slider", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(end)",
               "distance": "slide", "distance_limits": ["0 mm", "10 mm"]}),
    );
    println!("J1 (out of limits): rebuild_ok={} errors={}", r["rebuild_ok"], r["new_errors"]);
    // The user fixes it by adding a second joint (instead of editing the first).
    let r = run(
        &mut s,
        "feature.joint",
        json!({"type": "rigid", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(end)", "flip": true}),
    );
    println!("J2: rebuild_ok={} errors={}", r["rebuild_ok"], r["new_errors"]);
    let q = run(&mut s, "query.joints", json!({}));
    for j in q["joints"].as_array().unwrap() {
        println!("{} status={} centroid={}", j["name"], j["status"], j["body_centroid_mm"]);
    }
}

#[test]
fn t4_at_frame_on_other_body_and_with() {
    let mut s = session();
    plate_and_block(&mut s);
    // A third body.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch3", "x": 300, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch3", "distance": "5 mm", "body_name": "Third"}));
    // `at` frame lies on Plate but the moving body is Block.
    let r = execute(
        &mut s,
        &CmdCtx::user(1),
        "feature.joint",
        json!({"type": "rigid", "body": "Block", "at": "Extrude1/cap(start)", "to": "origin"}),
    );
    println!("at-on-other-body: {:?}", r.as_ref().map(|v| v["rebuild_ok"].clone()));
    let q = run(&mut s, "query.joints", json!({}));
    println!("Block centroid {}", q["joints"][0]["body_centroid_mm"]);
    // Third carried along by a joint on Block (already placed above): `with` of an already-placed body
    run(
        &mut s,
        "feature.joint",
        json!({"type": "rigid", "body": "Third", "with": ["Block"], "at": "Extrude3/cap(start)", "to": "XY", "offset": "50 mm"}),
    );
    let q = run(&mut s, "query.joints", json!({}));
    for j in q["joints"].as_array().unwrap() {
        println!("{} status={} centroid={}", j["name"], j["status"], j["body_centroid_mm"]);
    }
}

#[test]
fn t4b_closing_joint_with_is_ignored() {
    let mut s = session();
    plate_and_block(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch3", "x": 300, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch3", "distance": "5 mm", "body_name": "Third"}));
    run(
        &mut s,
        "feature.joint",
        json!({"type": "slider", "body": "Block", "at": "Extrude2/cap(start)", "to": "Extrude1/cap(end)",
               "flip": true, "free": ["distance"]}),
    );
    let r = run(
        &mut s,
        "feature.joint",
        json!({"type": "rigid", "body": "Block", "with": ["Third"], "at": "Extrude2/cap(start)", "to": "Extrude1/cap(start)"}),
    );
    println!("closing with `with`: rebuild_ok={} errors={}", r["rebuild_ok"], r["new_errors"]);
    let q = run(&mut s, "query.joints", json!({}));
    for j in q["joints"].as_array().unwrap() {
        println!("{} status={} centroid={}", j["name"], j["status"], j["body_centroid_mm"]);
    }
    let b = run(&mut s, "query.body", json!({"body": "Third"}));
    println!("Third bbox/centroid: {}", b["centroid_mm"]);
}

fn dir(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let d = std::env::temp_dir().join(format!("gusset-review-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_history(path: &Path, h: &History) {
    let meta = gusset_project::ProjectMeta {
        id: gusset_store::paths::ProjectId::new_random().unwrap(),
        save_seq: 1,
        base_seq: 1,
        created: 1,
    };
    std::fs::write(path, gusset_project::encode(h, &meta).unwrap()).unwrap();
}

const LEAF: &str = "gusset 1
units mm

sketch S (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(10, 0)
  p3 = point(10, 4)
  p4 = point(0, 4)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Pin (profile: S, distance: 2 mm, op: new)
";

/// A nested chain `levels` deep with `fan` components per level, all of the next level.
fn fan_history(levels: u32, fan: u32) -> History {
    let u = Ctx::user(1);
    let mut h = History::new(&Document::from_text(LEAF).unwrap(), &u);
    let mut prev: Hash = h.put_blob(LEAF.as_bytes());
    for lvl in 0..levels {
        let mut t = String::from("gusset 1\nunits mm\n\n");
        for i in 0..fan {
            t.push_str(&format!(
                "component C{i} (source: \"x.gus\", version: \"n1\", document: \"{}\")\n",
                prev.to_hex()
            ));
        }
        if lvl + 1 == levels {
            let d = Document::from_text(&t).unwrap();
            let main = h.main();
            h.commit(&u, main, &d, "top", NodeState::Ok).unwrap();
        } else {
            prev = h.put_blob(t.as_bytes());
        }
    }
    h
}

#[test]
fn t5_insert_component_fanout_cost() {
    for fan in [2u32, 3, 4] {
        let d = dir("fan");
        write_history(&d.join("asm.gus"), &fan_history(8, fan));
        let mut s = session();
        s.project_dir = Some(d.clone());
        let t = std::time::Instant::now();
        let r = execute(&mut s, &CmdCtx::user(1), "assembly.insert_component", json!({"path": "asm.gus"}));
        println!(
            "fan {fan}, 8 levels: {:?} in {:?}",
            r.as_ref().map(|v| v["feature"].clone()).map_err(|e| e.code.clone()),
            t.elapsed()
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

struct B(History);
impl gusset_eval::BlobSource for B {
    fn blob(&self, hex: &str) -> Option<Arc<[u8]>> {
        Hash::from_hex(hex).and_then(|h| self.0.objects().get(h).ok().map(|b| Arc::from(&b[..])))
    }
}

#[test]
fn t6_component_eval_fanout_cost() {
    for fan in [2u32, 3, 4] {
        let h = fan_history(8, fan);
        let head = h.head(h.main()).unwrap();
        let doc = h.document(head).unwrap();
        let mut ev = gusset_eval::Evaluator::new(Arc::new(MockKernel));
        let blobs = B(h);
        let t = std::time::Instant::now();
        let e = ev.evaluate(&doc, None, &blobs);
        println!(
            "eval fan {fan}, 8 levels: {} bodies, {} errors in {:?}",
            e.bodies.len(),
            e.errors().len(),
            t.elapsed()
        );
    }
}

#[test]
fn t7_query_components_reads_any_path_as_user() {
    let outside = dir("outside");
    let inside = dir("inside");
    let u = Ctx::user(1);
    let h = History::new(&Document::from_text(LEAF).unwrap(), &u);
    write_history(&outside.join("secret.gus"), &h);
    let mut s = session();
    s.project_dir = Some(inside.clone());
    // a component whose source is an absolute path outside the project / relative with ..
    let blob = {
        // put via an insert from inside first
        write_history(&inside.join("pin.gus"), &h);
        run(&mut s, "assembly.insert_component", json!({"path": "pin.gus"}));
        s.document(&gusset_history::Actor::User).unwrap()
    };
    let text = blob.to_text().replace("source: \"pin.gus\"", &format!("source: \"{}/secret.gus\"", outside.display()));
    let d2 = Document::from_text(&text).unwrap();
    let mut s2 = Session::new(&d2, Arc::new(MockKernel), &CmdCtx::user(0));
    s2.project_dir = Some(inside.clone());
    let q = run(&mut s2, "query.components", json!({}));
    println!("as user, absolute source outside project: {q}");
    let q = execute(&mut s2, &CmdCtx::agent("bot", 1), "query.components", json!({}));
    println!("as agent: {q:?}");
}

#[test]
fn t8_loop_solver_cost() {
    for (nj, nclose) in [(50usize, 1usize), (100, 1), (200, 1), (200, 4)] {
        let mut t = String::from("gusset 1\nunits mm\n\nsketch S1 (plane: XY, next_id: 9) {\n  p1 = point(0, 0)\n  p2 = point(10, 0)\n  p3 = point(10, 4)\n  p4 = point(0, 4)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\n");
        for i in 0..nj {
            t.push_str(&format!("extrude E{i} (profile: S1, distance: 5 mm, op: new)\n"));
        }
        for i in 0..nj {
            t.push_str(&format!("joint J{i} (type: ball, body: E{i}, at: E{i}/cap(start), to: XY, angles: [0 deg, 0 deg, 0 deg], free: [angles])\n"));
        }
        for c in 0..nclose {
            t.push_str(&format!("joint C{c} (type: rigid, body: E0, at: E0/cap(start), to: XY, offset: {} mm)\n", 7 + c));
        }
        let doc = Document::from_text(&t).unwrap();
        let mut ev = gusset_eval::Evaluator::new(Arc::new(MockKernel));
        let start = std::time::Instant::now();
        let e = ev.evaluate(&doc, None, &());
        println!("{nj} free ball joints ({} unknowns), {nclose} unsatisfiable closing joint(s): {:?}, first error: {:?}", nj * 3, start.elapsed(), e.errors().first().map(|x| x.to_string().chars().take(70).collect::<String>()));
    }
}
