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
    assert!(d.get("warnings").is_none(), "{d}");
    // A drill of another size than the holes is warned about (F6-11).
    let w = run(
        &mut s,
        "cam.toolpath",
        json!({"sketch": "Sketch1", "operation": "drill", "tool_diameter": 3.0, "depth": 6.0}),
    );
    assert!(w["warnings"][0].as_str().unwrap().starts_with("2 of 2 holes are not 3 mm across"), "{w}");
    let g = std::fs::read_to_string(s.project_dir.clone().unwrap().join("holes.nc")).unwrap();
    assert!(g.contains("G98 G83 X10 Y10 Z-6 R1 Q2") && g.contains("\nX30 Y10\n"), "{g}");
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
    // The peak re-solved finer: uniform tension stays 10 MPa.
    let fine = run(
        &mut s,
        "sim.static",
        json!({"fixed": ["Extrude1/cap(start)"], "loads": [{"face": "Extrude1/cap(end)", "force": [0, 0, 1000]}],
               "poisson": 0.0, "voxel": 5.0, "refine": true}),
    );
    let f = &fine["refined"];
    assert!((f["max_von_mises_mpa"].as_f64().unwrap() - 10.0).abs() < 1e-2 && f["voxel_mm"] == 1.25, "{fine}");
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
    assert!((bands[7]["upto"].as_f64().unwrap() - 10.0).abs() < 1e-3);
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
    // The default voxel: four across the 10 mm side (2.5 mm), so no caveat about it; at
    // 5 mm (two across) the result says it is too stiff.
    let st = run(
        &mut s,
        "sim.static",
        json!({"fixed": ["Extrude1/cap(start)"], "loads": [{"face": "Extrude1/cap(end)", "force": [100, 0, 0]}]}),
    );
    assert_eq!(st["voxel_mm"], 2.5, "{st}");
    let notes = st["caveats"].as_array().cloned().unwrap_or_default();
    assert!(notes.iter().all(|n| !n.as_str().unwrap().contains("fewer than 3")), "{st}");
    let coarse = run(
        &mut s,
        "sim.static",
        json!({"fixed": ["Extrude1/cap(start)"], "loads": [{"face": "Extrude1/cap(end)", "force": [100, 0, 0]}], "voxel": 5.0}),
    );
    assert!(coarse["caveats"].to_string().contains("fewer than 3"), "{coarse}");
    // Beam theory: 835.5 Hz, twice; stiff bricks read a little high.
    let f1 = f[0].as_f64().unwrap();
    assert!(f.len() == 2 && f1 > 800.0 && f1 < 1000.0, "{r}");
    assert_eq!(err(&mut s, "sim.modal", json!({"fixed": ["Extrude1/cap(start)"], "modes": 11})).code, "invalid_args");
    assert!(r.get("shape_bands").is_none());
    let sh = run(&mut s, "sim.modal", json!({"fixed": ["Extrude1/cap(start)"], "modes": 1, "voxel": 5.0, "shape": 1}));
    let bands = sh["shape_bands"].as_array().unwrap();
    assert_eq!(bands.len(), 8);
    // The free end moves most: the top band holds faces near z = 100 (moved by up to 10 mm).
    let top = bands[7]["quads"].as_array().unwrap();
    assert!(!top.is_empty() && top.iter().all(|q| q[2].as_f64().unwrap() > 60.0), "{}", bands[7]);
    // The scale is stated (F6-14): the free end swings sideways, drawn 10 mm.
    let sc = &sh["shape_scale"];
    assert!((sc["drawn_mm"].as_f64().unwrap() - 10.0).abs() < 1e-6, "{sc}");
    assert!(sc["largest_at_mm"][2].as_f64().unwrap() > 95.0, "{sc}");
    assert!(sc["direction"][2].as_f64().unwrap().abs() < 0.2, "{sc}");
    assert_eq!(
        err(&mut s, "sim.modal", json!({"fixed": ["Extrude1/cap(start)"], "modes": 1, "shape": 2})).code,
        "invalid_args"
    );
}

/// Fusion component placements on a real design: runs when `GUSSET_F3D_SAMPLES` names a
/// folder holding Adafruit's Crickit HAT archive as `a.f3z` (not in the repository).
#[test]
fn fusion_components_are_placed_by_their_occurrences() {
    let Ok(dir) = std::env::var("GUSSET_F3D_SAMPLES") else { return };
    let bytes = std::fs::read(std::path::Path::new(&dir).join("a.f3z")).unwrap();
    let limits = gusset_meshio::Limits::default();
    let mut budget = limits.max_decompressed_bytes;
    let designs =
        gusset_meshio::archive::read_entries_within(&bytes, &limits, &|n| n.ends_with(".f3d"), u64::MAX, &mut budget)
            .unwrap();
    let mut placed = Vec::new();
    for (_, d) in &designs {
        let s = gusset_meshio::archive::read_entries_within(
            d,
            &limits,
            &|n| n.ends_with("/BulkStream.dat"),
            u64::MAX,
            &mut budget,
        )
        .unwrap();
        for (_, b) in &s {
            placed.extend(gusset_sat::fusion::placements(b));
        }
    }
    // 36 component bodies; the board's own 3 bodies stay in the root.
    assert_eq!(placed.len(), 36, "{:?}", placed.iter().map(|p| &p.occurrence).collect::<Vec<_>>());
    let sw1 = placed.iter().find(|p| p.occurrence == "BTN_KMR2_4.6X2.8:SW1").unwrap();
    let t = sw1.placement.t;
    assert!((t[0] - 8.001).abs() < 1e-2 && (t[1] - 20.447).abs() < 1e-2 && (t[2] - 1.57).abs() < 1e-2, "{t:?}");
    // Every part lies on the 65 x 56 mm board.
    for p in &placed {
        let t = p.placement.t;
        assert!((0.0..=66.0).contains(&t[0]) && (0.0..=57.0).contains(&t[1]), "{}: {t:?}", p.occurrence);
    }
}

#[test]
fn a_cam_job_in_the_document_runs_by_name_and_follows_parameters() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    let text = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    let text = text.replace("gusset 1\nunits mm\n", "gusset 1\nunits mm\n\nparam d = 4 mm\n")
        + "\ncam_job Job1 (sketch: Sketch1, operation: pocket, tool_diameter: 6 mm, depth: d, stepdown: 2 mm)\n";
    run(&mut s, "document.set_text", json!({ "text": text }));
    let back = run(&mut s, "document.get_text", json!({}))["text_user_text"].as_str().unwrap().to_string();
    assert!(back.contains("cam_job Job1 (sketch: Sketch1, operation: pocket, tool_diameter: 6 mm, depth: d"), "{back}");
    let by_job = run(&mut s, "cam.toolpath", json!({"job": "Job1"}));
    let explicit = run(
        &mut s,
        "cam.toolpath",
        json!({"sketch": "Sketch1", "operation": "pocket", "tool_diameter": 6.0, "depth": 4.0, "stepdown": 2.0}),
    );
    assert_eq!(by_job, explicit);
    assert_eq!(by_job["levels_mm"], json!([-2.0, -4.0]), "{by_job}");
    // The job follows its parameters.
    run(&mut s, "param.set", json!({"name": "d", "expr": "6 mm"}));
    let deeper = run(&mut s, "cam.toolpath", json!({"job": "Job1"}));
    assert_eq!(deeper["levels_mm"], json!([-2.0, -4.0, -6.0]), "{deeper}");
    // job and the fields together, or neither, are refused; an unknown job is not found.
    assert_eq!(err(&mut s, "cam.toolpath", json!({"job": "Job1", "depth": 3.0})).code, "invalid_args");
    assert_eq!(err(&mut s, "cam.toolpath", json!({"sketch": "Sketch1"})).code, "invalid_args");
    assert_eq!(err(&mut s, "cam.toolpath", json!({"job": "Nope"})).code, "not_found");
}

#[test]
fn move_face_onto_refuses_targets_it_cannot_reach_before_committing() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_rectangle", json!({"sketch": "Sketch1", "x": 0, "y": 0, "width": 40, "height": 20}));
    run(&mut s, "feature.extrude", json!({"sketch": "Sketch1", "distance": "10 mm"}));
    let n = s.document(&gusset_history::Actor::User).unwrap().features.len();
    for (target, why) in [
        ("Extrude1/side(Sketch1/l5)", "perpendicular"),
        ("Extrude1/cap(end)", "already lies"),
        ("Extrude1/cap(start)", "no material"),
    ] {
        let e = err(&mut s, "feature.move_face", json!({"faces": ["Extrude1/cap(end)"], "onto": target}));
        assert!(e.message.contains(why), "{target}: {}", e.message);
    }
    assert_eq!(s.document(&gusset_history::Actor::User).unwrap().features.len(), n, "nothing committed");
}

#[test]
fn circles_are_bored_to_their_size_on_a_helix() {
    let mut s = session();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.add_circle", json!({"sketch": "Sketch1", "cx": 20, "cy": 10, "radius": 5}));
    let job = json!({"path": "bore.nc", "sketch": "Sketch1", "operation": "bore", "tool_diameter": 6.0, "depth": 4.0, "stepdown": 2.0});
    run(&mut s, "cam.export_gcode", job);
    let g = std::fs::read_to_string(s.project_dir.clone().unwrap().join("bore.nc")).unwrap();
    // Climb in a hole: counter-clockwise arcs of radius 2 about the circle's centre.
    let arcs: Vec<&str> = g.lines().filter(|l| l.starts_with("G3 ")).collect();
    assert!(!arcs.is_empty() && !g.contains("G2 "), "{g}");
    assert!(g.contains("Z-4"), "{g}");
    let too_wide = json!({"sketch": "Sketch1", "operation": "bore", "tool_diameter": 4.0, "depth": 4.0});
    assert_eq!(err(&mut s, "cam.toolpath", too_wide).code, "invalid_args");
    let _ = std::fs::remove_dir_all(s.project_dir.clone().unwrap());
}
