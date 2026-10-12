//! Persistent-naming golden scenarios at document level (ADR 0003, review H7), shared by
//! the mock-kernel unit tests and the OCCT suite (`gusset-cli/tests/naming_golden.rs`).
//!
//! References to generated faces must keep resolving to the same geometry through
//! reordering of independent features, branching and editing, cherry-picking, and an
//! unrelated second region in a swept sketch (review L4).

use crate::{CmdCtx, Session, execute};
use gusset_doc::Document;
use gusset_history::Actor;
use gusset_kernel::Kernel;
use serde_json::{Value, json};
use std::sync::Arc;

/// A 40 × 20 mm plate extruded by `d`, with sketches on its top cap and right side.
pub const NAMED_DOC: &str = "gusset 1\nunits mm\nparam d = 10 mm\n\
sketch S1 (plane: XY, next_id: 9) {\n\
  p1 = point(0, 0)\n  p2 = point(40, 0)\n  p3 = point(40, 20)\n  p4 = point(0, 20)\n\
  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\
extrude E1 (profile: S1, distance: d, op: new)\n\
sketch Top (plane: E1/cap(end), next_id: 3) {\n  p1 = point(5, 5)\n  c2 = circle(p1, 2)\n}\n\
sketch Side (plane: E1/side(S1/l6), next_id: 3) {\n  p1 = point(5, 5)\n  c2 = circle(p1, 2)\n}\n";

type R = Result<(), String>;
type Scenario = fn(Arc<dyn Kernel>) -> R;

fn session(kernel: Arc<dyn Kernel>) -> Result<Session, String> {
    let doc = Document::from_text(NAMED_DOC).map_err(|e| format!("{e:?}"))?;
    Ok(Session::new(&doc, kernel, &CmdCtx::user(0)))
}

fn run(s: &mut Session, id: &str, args: Value) -> Result<Value, String> {
    execute(s, &CmdCtx::user(1), id, args).map_err(|e| format!("{id}: {e:?}"))
}

fn str_of(v: &Value, key: &str) -> Result<String, String> {
    v.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("no {key} in {v}"))
}

/// Checks that sketch `name` evaluates and its plane origin has `axis` = `value`.
fn on_plane(s: &mut Session, name: &str, axis: usize, value: f64) -> R {
    let ev = s.evaluate(&Actor::User).map_err(|e| format!("{e:?}"))?;
    let status = ev.statuses.iter().find(|(n, _)| n == name).map(|(_, st)| st.clone());
    if !matches!(status, Some(gusset_eval::FeatureStatus::Ok)) {
        return Err(format!("{name}: {status:?}"));
    }
    let o = ev.sketches.get(name).ok_or_else(|| format!("no sketch {name}"))?.frame.origin;
    match o.get(axis) {
        Some(v) if (v - value).abs() < 1e-6 => Ok(()),
        _ => Err(format!("{name}: plane origin {o:?}, axis {axis} should be {value}")),
    }
}

/// Moving an independent feature to the front changes no reference.
///
/// # Errors
/// A description of the first reference that no longer resolves to the same face.
pub fn reorder_independent_features(kernel: Arc<dyn Kernel>) -> R {
    let mut s = session(kernel)?;
    on_plane(&mut s, "Top", 2, 10.0)?;
    on_plane(&mut s, "Side", 0, 40.0)?;
    let other = str_of(&run(&mut s, "sketch.create", json!({"plane": "XZ"}))?, "sketch")?;
    run(&mut s, "sketch.add_rectangle", json!({"sketch": other, "x": 0, "y": 0, "width": 3, "height": 3}))?;
    run(&mut s, "feature.move", json!({"name": other, "index": 0}))?;
    let doc = s.document(&Actor::User).map_err(|e| format!("{e:?}"))?;
    if doc.features.first().map(|f| f.name.as_str()) != Some(other.as_str()) {
        return Err("feature.move did not move the sketch".into());
    }
    on_plane(&mut s, "Top", 2, 10.0)?;
    on_plane(&mut s, "Side", 0, 40.0)
}

/// Editing a parameter on a new branch moves the referenced faces with the geometry;
/// the original branch is unaffected.
///
/// # Errors
/// As [`reorder_independent_features`].
pub fn branch_and_edit(kernel: Arc<dyn Kernel>) -> R {
    let mut s = session(kernel)?;
    run(&mut s, "branch.create", json!({"name": "taller"}))?;
    run(&mut s, "param.set", json!({"name": "d", "expr": "15 mm"}))?;
    on_plane(&mut s, "Top", 2, 15.0)?;
    on_plane(&mut s, "Side", 0, 40.0)?;
    run(&mut s, "branch.switch", json!({"branch": "main"}))?;
    on_plane(&mut s, "Top", 2, 10.0)
}

/// A feature cherry-picked onto another branch resolves its references there.
///
/// # Errors
/// As [`reorder_independent_features`].
pub fn cherry_pick(kernel: Arc<dyn Kernel>) -> R {
    let mut s = session(kernel)?;
    run(&mut s, "branch.create", json!({"name": "extra"}))?;
    let extra = str_of(&run(&mut s, "sketch.create", json!({"plane": "E1/side(S1/l5)"}))?, "sketch")?;
    run(&mut s, "sketch.add_rectangle", json!({"sketch": extra, "x": 2, "y": 2, "width": 3, "height": 3}))?;
    on_plane(&mut s, &extra, 1, 0.0)?;
    run(&mut s, "branch.switch", json!({"branch": "main"}))?;
    run(&mut s, "branch.cherry_pick", json!({"from": "extra", "features": [extra]}))?;
    on_plane(&mut s, &extra, 1, 0.0)?;
    on_plane(&mut s, "Side", 0, 40.0)
}

/// Review L4: a second closed loop in the swept sketch keeps `cap(end)` on the first
/// region.
///
/// # Errors
/// As [`reorder_independent_features`].
pub fn unrelated_second_region(kernel: Arc<dyn Kernel>) -> R {
    let mut s = session(kernel)?;
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "S1", "x": 80, "y": 0, "width": 10, "height": 10}))?;
    on_plane(&mut s, "Top", 2, 10.0)?;
    on_plane(&mut s, "Side", 0, 40.0)
}

/// Datum planes, moved copies and combined bodies keep their names: references to the
/// faces of a datum plane, a moved copy and the body it was combined into resolve to the
/// moved geometry and follow parameter edits and later moves.
///
/// # Errors
/// As [`reorder_independent_features`].
pub fn moved_and_combined_bodies(kernel: Arc<dyn Kernel>) -> R {
    let mut s = session(kernel)?;
    run(&mut s, "feature.plane", json!({"base": "E1/cap(end)", "offset": "5 mm", "name": "Up"}))?;
    let on_up = str_of(&run(&mut s, "sketch.create", json!({"plane": "Up"}))?, "sketch")?;
    on_plane(&mut s, &on_up, 2, 15.0)?;
    let dup = json!({"body": "E1", "translate": ["100 mm", "0 mm", "0 mm"], "copy": true, "name": "Dup"});
    run(&mut s, "feature.move_body", dup)?;
    run(&mut s, "feature.combine", json!({"tools": ["Dup"], "body": "E1"}))?;
    // The copy's faces are now faces of E1, still under the names the copy gave them.
    let dup_side = str_of(&run(&mut s, "sketch.create", json!({"plane": "Dup/copy(E1/side(S1/l6))"}))?, "sketch")?;
    let dup_top = str_of(&run(&mut s, "sketch.create", json!({"plane": "Dup/copy(E1/cap(end))"}))?, "sketch")?;
    on_plane(&mut s, &dup_side, 0, 140.0)?;
    on_plane(&mut s, &dup_top, 2, 10.0)?;
    on_plane(&mut s, "Side", 0, 40.0)?;
    // A parameter edit moves the top faces and the datum plane with them.
    run(&mut s, "param.set", json!({"name": "d", "expr": "15 mm"}))?;
    on_plane(&mut s, &on_up, 2, 20.0)?;
    on_plane(&mut s, &dup_top, 2, 15.0)?;
    on_plane(&mut s, &dup_side, 0, 140.0)?;
    on_plane(&mut s, "Top", 2, 15.0)?;
    // Moving the combined body carries every name along.
    run(&mut s, "feature.move_body", json!({"body": "E1", "translate": ["10 mm", "0 mm", "0 mm"]}))?;
    // Sketches made before the move are unaffected: the timeline runs in order.
    on_plane(&mut s, "Side", 0, 40.0)?;
    on_plane(&mut s, &dup_side, 0, 140.0)?;
    let side = str_of(&run(&mut s, "sketch.create", json!({"plane": "E1/side(S1/l6)"}))?, "sketch")?;
    let moved_side = str_of(&run(&mut s, "sketch.create", json!({"plane": "Dup/copy(E1/side(S1/l6))"}))?, "sketch")?;
    let moved_top = str_of(&run(&mut s, "sketch.create", json!({"plane": "Dup/copy(E1/cap(end))"}))?, "sketch")?;
    on_plane(&mut s, &side, 0, 50.0)?;
    on_plane(&mut s, &moved_side, 0, 150.0)?;
    on_plane(&mut s, &moved_top, 2, 15.0)?;
    run(&mut s, "param.set", json!({"name": "d", "expr": "8 mm"}))?;
    on_plane(&mut s, &moved_top, 2, 8.0)?;
    on_plane(&mut s, &dup_top, 2, 8.0)?;
    on_plane(&mut s, &on_up, 2, 13.0)?;
    on_plane(&mut s, &moved_side, 0, 150.0)
}

/// A swept bar (square profile along a vertical path) and a lofted frustum on a datum plane,
/// each with sketches on its cap and side faces.
pub const SWEEP_DOC: &str = "gusset 1\nunits mm\nparam len = 20 mm\nparam top = 12 mm\n\
sketch Profile (plane: XY, next_id: 9) {\n\
  p1 = point(-5, -5)\n  p2 = point(5, -5)\n  p3 = point(5, 5)\n  p4 = point(-5, 5)\n\
  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\
sketch Path (plane: XZ, next_id: 7) {\n  p1 = point(0, 0)\n  p2 = point(0, 20)\n  l3 = line(p1, p2)\n\
  k4 = fixed(p1)\n  k5 = vertical(l3)\n  k6 = distance(l3, l3, len)\n}\n\
sweep Sweep1 (profile: Profile, path: Path, orientation: frenet, twist: 30 deg, op: new)\n\
plane Up (base: XY, offset: top)\n\
sketch Foot (plane: XY, next_id: 9) {\n\
  p1 = point(40, -10)\n  p2 = point(60, -10)\n  p3 = point(60, 10)\n  p4 = point(40, 10)\n\
  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\
sketch Head (plane: Up, next_id: 9) {\n\
  p1 = point(46, -4)\n  p2 = point(54, -4)\n  p3 = point(54, 4)\n  p4 = point(46, 4)\n\
  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\
loft Loft1 (profiles: [Foot, Head], ruled: true, op: new)\n\
sketch OnBarEnd (plane: Sweep1/cap(end), next_id: 3) {\n  p1 = point(1, 1)\n  c2 = circle(p1, 1)\n}\n\
sketch OnBarStart (plane: Sweep1/cap(start), next_id: 3) {\n  p1 = point(1, 1)\n  c2 = circle(p1, 1)\n}\n\
sketch OnFrustumTop (plane: Loft1/cap(end), next_id: 3) {\n  p1 = point(50, 0)\n  c2 = circle(p1, 1)\n}\n\
sketch Profile2 (plane: XY, next_id: 9) {\n\
  p1 = point(95, -5)\n  p2 = point(105, -5)\n  p3 = point(105, 5)\n  p4 = point(95, 5)\n\
  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\
sketch Path2 (plane: XZ, next_id: 7) {\n  p1 = point(100, 0)\n  p2 = point(100, 20)\n  l3 = line(p1, p2)\n\
  k4 = fixed(p1)\n  k5 = vertical(l3)\n  k6 = distance(l3, l3, len)\n}\n\
sweep Sweep2 (profile: Profile2, path: Path2, op: new)\n\
sketch OnBarSide (plane: Sweep2/side(Profile2/l6), next_id: 3) {\n  p1 = point(0, 5)\n  c2 = circle(p1, 1)\n}\n";

/// Sweep and loft faces keep their names through parameter edits, an unrelated feature moved to
/// the front and a branch: sketches on the caps and sides follow the geometry (needs a kernel
/// that sweeps and lofts, i.e. OCCT).
///
/// # Errors
/// As [`reorder_independent_features`].
pub fn sweep_and_loft_names(kernel: Arc<dyn Kernel>) -> R {
    let doc = Document::from_text(SWEEP_DOC).map_err(|e| format!("{e:?}"))?;
    let mut s = Session::new(&doc, kernel, &CmdCtx::user(0));
    let at = |s: &mut Session, name: &str, axis: usize, value: f64| on_plane(s, name, axis, value);
    at(&mut s, "OnBarEnd", 2, 20.0)?;
    at(&mut s, "OnBarStart", 2, 0.0)?;
    at(&mut s, "OnFrustumTop", 2, 12.0)?;
    at(&mut s, "OnBarSide", 0, 105.0)?;
    run(&mut s, "param.set", json!({"name": "len", "expr": "33 mm"}))?;
    at(&mut s, "OnBarEnd", 2, 33.0)?;
    at(&mut s, "OnBarStart", 2, 0.0)?;
    at(&mut s, "OnFrustumTop", 2, 12.0)?;
    run(&mut s, "param.set", json!({"name": "top", "expr": "25 mm"}))?;
    at(&mut s, "OnFrustumTop", 2, 25.0)?;
    at(&mut s, "OnBarEnd", 2, 33.0)?;
    // An unrelated sketch moved to the front changes no reference.
    let other = str_of(&run(&mut s, "sketch.create", json!({"plane": "XZ"}))?, "sketch")?;
    run(&mut s, "sketch.add_rectangle", json!({"sketch": other, "x": 0, "y": 0, "width": 3, "height": 3}))?;
    run(&mut s, "feature.move", json!({"name": other, "index": 0}))?;
    at(&mut s, "OnBarEnd", 2, 33.0)?;
    at(&mut s, "OnFrustumTop", 2, 25.0)?;
    // A branch edits the sweep's length and leaves the first branch alone.
    run(&mut s, "branch.create", json!({"name": "long"}))?;
    run(&mut s, "param.set", json!({"name": "len", "expr": "50 mm"}))?;
    at(&mut s, "OnBarEnd", 2, 50.0)?;
    run(&mut s, "branch.switch", json!({"branch": "main"}))?;
    at(&mut s, "OnBarEnd", 2, 33.0)
}

/// A 60 x 40 plate with a counterbored through hole at each point of a sketch, a flat-bottomed
/// blind hole at an explicit point, and sketches on the step and on the floor.
pub const HOLE_DOC: &str = "gusset 1\nunits mm\nparam t = 12 mm\nparam d = 6.6 mm\nparam cb = 11 mm\n\
sketch S1 (plane: XY, next_id: 9) {\n\
  p1 = point(0, 0)\n  p2 = point(60, 0)\n  p3 = point(60, 40)\n  p4 = point(0, 40)\n\
  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\
extrude E1 (profile: S1, distance: t, op: new)\n\
sketch Marks (plane: E1/cap(end), next_id: 3) {\n  p1 = point(15, 20)\n  p2 = point(45, 20)\n}\n\
hole H1 (face: E1/cap(end), points: Marks, type: counterbore, diameter: d, through: true, cbore_diameter: cb, cbore_depth: 5 mm)\n\
hole H2 (face: E1/cap(end), at: [[30 mm, 8 mm]], type: simple, diameter: 4 mm, depth: 6 mm, point_angle: 180 deg)\n\
sketch OnStep (plane: H1/cbore_floor(p1), next_id: 3) {\n  p1 = point(15, 20)\n  c2 = circle(p1, 1)\n}\n\
sketch OnStep2 (plane: H1/cbore_floor(p2), next_id: 3) {\n  p1 = point(45, 20)\n  c2 = circle(p1, 1)\n}\n\
sketch OnFloor (plane: H2/floor(e1), next_id: 3) {\n  p1 = point(30, 8)\n  c2 = circle(p1, 1)\n}\n";

/// Hole faces keep their names through parameter edits, an unrelated feature moved to the
/// front and a branch: sketches on the step of a counterbore and on the floor of a blind hole
/// follow the geometry (needs revolve and booleans, i.e. OCCT).
///
/// # Errors
/// As [`reorder_independent_features`].
pub fn hole_names(kernel: Arc<dyn Kernel>) -> R {
    let doc = Document::from_text(HOLE_DOC).map_err(|e| format!("{e:?}"))?;
    let mut s = Session::new(&doc, kernel, &CmdCtx::user(0));
    let at = |s: &mut Session, name: &str, axis: usize, value: f64| on_plane(s, name, axis, value);
    at(&mut s, "OnStep", 2, 7.0)?;
    at(&mut s, "OnStep2", 2, 7.0)?;
    at(&mut s, "OnFloor", 2, 6.0)?;
    // The dimensions of the holes and of the plate.
    run(&mut s, "param.set", json!({"name": "d", "expr": "8 mm"}))?;
    run(&mut s, "param.set", json!({"name": "cb", "expr": "14 mm"}))?;
    at(&mut s, "OnStep", 2, 7.0)?;
    run(&mut s, "param.set", json!({"name": "t", "expr": "20 mm"}))?;
    at(&mut s, "OnStep", 2, 15.0)?;
    at(&mut s, "OnStep2", 2, 15.0)?;
    at(&mut s, "OnFloor", 2, 14.0)?;
    // An unrelated sketch moved to the front changes no reference.
    let other = str_of(&run(&mut s, "sketch.create", json!({"plane": "XZ"}))?, "sketch")?;
    run(&mut s, "sketch.add_rectangle", json!({"sketch": other, "x": 0, "y": 0, "width": 3, "height": 3}))?;
    run(&mut s, "feature.move", json!({"name": other, "index": 0}))?;
    at(&mut s, "OnStep", 2, 15.0)?;
    at(&mut s, "OnFloor", 2, 14.0)?;
    // A branch edits the plate and leaves the first branch alone.
    run(&mut s, "branch.create", json!({"name": "thin"}))?;
    run(&mut s, "param.set", json!({"name": "t", "expr": "9 mm"}))?;
    at(&mut s, "OnStep", 2, 4.0)?;
    at(&mut s, "OnFloor", 2, 3.0)?;
    run(&mut s, "branch.switch", json!({"branch": "main"}))?;
    at(&mut s, "OnStep", 2, 15.0)
}

/// Every scenario, for a kernel.
///
/// # Errors
/// The failures, one per line.
pub fn all(kernel: &Arc<dyn Kernel>) -> R {
    let scenarios: [(&str, Scenario); 5] = [
        ("reorder", reorder_independent_features),
        ("branch", branch_and_edit),
        ("cherry_pick", cherry_pick),
        ("second_region", unrelated_second_region),
        ("moved_and_combined", moved_and_combined_bodies),
    ];
    let errs: Vec<String> =
        scenarios.iter().filter_map(|(n, f)| f(kernel.clone()).err().map(|e| format!("{n}: {e}"))).collect();
    if errs.is_empty() { Ok(()) } else { Err(errs.join("\n")) }
}

/// [`all`] plus the scenarios that need a kernel with sweeps, lofts, revolve and booleans (OCCT).
///
/// # Errors
/// The failures, one per line.
pub fn all_with_sweeps(kernel: &Arc<dyn Kernel>) -> R {
    let mut errs: Vec<String> = all(kernel).err().into_iter().collect();
    errs.extend(sweep_and_loft_names(kernel.clone()).err().map(|e| format!("sweep_and_loft: {e}")));
    errs.extend(hole_names(kernel.clone()).err().map(|e| format!("hole_names: {e}")));
    if errs.is_empty() { Ok(()) } else { Err(errs.join("\n")) }
}
