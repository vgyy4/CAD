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
    assert!(q.get("path").is_none(), "the path only on request");
    let mut with = job.clone();
    with["path"] = json!(true);
    let p = run(&mut s, "cam.toolpath", with);
    let lines = p["path"].as_array().unwrap();
    assert!(!lines.is_empty(), "{p}");
    // Inside the pocket, a tool radius in, and down to the depth.
    for v in lines.iter().flat_map(|l| l.as_array().unwrap()) {
        let [x, y, z] = [0, 1, 2].map(|i| v[i].as_f64().unwrap());
        assert!((3.0 - 1e-6..=37.0 + 1e-6).contains(&x) && (3.0 - 1e-6..=17.0 + 1e-6).contains(&y), "{v}");
        assert!((-5.0 - 1e-9..=5.0).contains(&z), "{v}");
    }
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
    bad["operation"] = json!("engrave");
    assert_eq!(err(&mut s, "cam.toolpath", bad).code, "invalid_args");
    let mut big = job;
    big["tool_diameter"] = json!(30.0);
    assert_eq!(err(&mut s, "cam.toolpath", big).code, "evaluation");
    // Holes at the sketch's circles.
    run(&mut s, "sketch.add_circle", json!({"sketch": "Sketch1", "cx": 10, "cy": 10, "radius": 2}));
    run(&mut s, "sketch.add_circle", json!({"sketch": "Sketch1", "cx": 30, "cy": 10, "radius": 2}));
    let d = run(
        &mut s,
        "cam.export_gcode",
        json!({"path": "holes.nc", "sketch": "Sketch1", "operation": "drill", "tool_diameter": 4.0, "depth": 6.0, "peck": 2.0}),
    );
    assert_eq!(d["written"], "holes.nc", "{d}");
    let g = std::fs::read_to_string(s.project_dir.clone().unwrap().join("holes.nc")).unwrap();
    assert!(g.contains("G98 G83 X10 Y10 Z-6 R0 Q2") && g.contains("\nX30 Y10\n"), "{g}");
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}

#[test]
fn a_bar_is_analysed_by_its_face_names() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "100 mm"}));
    let r = run(
        &mut s,
        "sim.static",
        json!({"fixed": ["Extrude1/cap(start)"], "loads": [{"face": "Extrude1/cap(end)", "force": [0, 0, 1000]}],
               "poisson": 0.0, "voxel": 5.0}),
    );
    // PL / EA = 1000 · 100 / (210000 · 100).
    let exact = 1000.0 * 100.0 / (210_000.0 * 100.0);
    let d = r["max_displacement_mm"].as_f64().unwrap();
    assert!((d - exact).abs() < 1e-5, "{r}");
    assert!((r["max_von_mises_mpa"].as_f64().unwrap() - 10.0).abs() < 1e-3, "{r}");
    assert!(r.get("stress_bands").is_none());
    let f = run(
        &mut s,
        "sim.static",
        json!({"fixed": ["Extrude1/cap(start)"], "loads": [{"face": "Extrude1/cap(end)", "force": [0, 0, 1000]}],
               "poisson": 0.0, "voxel": 5.0, "field": true}),
    );
    // Uniform stress: every voxel face in the top band.
    let bands = f["stress_bands"].as_array().unwrap();
    assert_eq!(bands.len(), 8);
    let counts: Vec<usize> = bands.iter().map(|b| b["quads"].as_array().unwrap().len()).collect();
    assert_eq!(counts.iter().sum::<usize>(), 168, "{counts:?}");
    assert!((bands[7]["upto_mpa"].as_f64().unwrap() - 10.0).abs() < 1e-3);
    assert_eq!(bands[7]["quads"][0].as_array().unwrap().len(), 12);
    assert_eq!(err(&mut s, "sim.static", json!({"fixed": [], "loads": []})).code, "invalid_args");
    assert_eq!(err(&mut s, "sim.static", json!({"fixed": ["Nope/cap(end)"], "loads": []})).code, "not_found");
}

#[test]
fn a_cantilever_rod_has_its_natural_frequencies() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 10, "height": 10}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "100 mm"}));
    let r = run(&mut s, "sim.modal", json!({"fixed": ["Extrude1/cap(start)"], "modes": 2, "voxel": 2.5}));
    let f = r["frequencies_hz"].as_array().unwrap();
    // Beam theory: 835.5 Hz, twice; stiff bricks read a little high.
    let f1 = f[0].as_f64().unwrap();
    assert!(f.len() == 2 && f1 > 800.0 && f1 < 1000.0, "{r}");
    assert_eq!(err(&mut s, "sim.modal", json!({"fixed": ["Extrude1/cap(start)"], "modes": 11})).code, "invalid_args");
}
