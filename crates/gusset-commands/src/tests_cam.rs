//! CAM commands (ADR 0040) through the registry.

use crate::*;
use gusset_doc::Document;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-cam-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    s.project_dir = Some(dir);
    s
}

fn run(s: &mut Session, id: &str, args: Value) -> Value {
    execute(s, &CmdCtx::user(1), id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

fn err(s: &mut Session, id: &str, args: Value) -> CommandError {
    execute(s, &CmdCtx::user(1), id, args).map(|v| panic!("{id} should fail: {v}")).unwrap_err()
}

#[test]
fn a_pocket_is_summarised_and_exported_as_gcode() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    let job = json!({"sketch": "Sketch1", "operation": "pocket", "tool_diameter": 6.0, "depth": 5.0, "stepdown": 2.0});
    let q = run(&mut s, "cam.toolpath", job.clone());
    assert_eq!(q["levels_mm"], json!([-1.67, -3.33, -5.0]), "{q}");
    assert!(q["rings"].as_u64().unwrap() >= 3 && q["minutes"].as_f64().unwrap() > 0.0, "{q}");
    let mut exp = job.clone();
    exp["path"] = json!("pocket.nc");
    let w = run(&mut s, "cam.export_gcode", exp.clone());
    assert_eq!(w["written"], "pocket.nc", "{w}");
    let g = std::fs::read_to_string(s.project_dir.clone().unwrap().join("pocket.nc")).unwrap();
    // Ramped by default: down to the stock top at the plunge feed, then a spiral.
    assert!(g.contains("G21 G90 G17") && g.contains("G1 Z0 F200") && g.ends_with("M30\n"), "{g}");
    assert!(!g.contains("G1 Z-5 F200"), "no straight plunge to the full depth");
    assert_eq!(err(&mut s, "cam.export_gcode", exp).code, "confirmation_required");
    let mut typo = job.clone();
    typo["tool_diametr"] = json!(6.0);
    assert_eq!(err(&mut s, "cam.toolpath", typo).code, "invalid_args");
    let mut bad = job.clone();
    bad["operation"] = json!("drill");
    assert_eq!(err(&mut s, "cam.toolpath", bad).code, "invalid_args");
    let mut big = job;
    big["tool_diameter"] = json!(30.0);
    assert_eq!(err(&mut s, "cam.toolpath", big).code, "evaluation");
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}
