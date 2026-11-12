//! KiCad board import with the real OCCT kernel (ADR 0047): the board is the outline
//! extruded to its thickness with its holes and cut-out cut (exact volume), copper is
//! optional, and a footprint's STEP model, read through the sandboxed worker, replaces its
//! box at the footprint's place.

#![cfg(gusset_occt)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use gusset_commands::{CmdCtx, Session, execute};
use gusset_history::Actor;
use gusset_kernel::Kernel;
use serde_json::json;
use std::f64::consts::PI;
use std::path::Path;
use std::sync::Arc;

/// 60 × 40 × 1.6 mm with rounded corners (r 3), a round cut-out (r 4), a slot hole
/// (oval drill 1 × 2), two round holes (1 mm) and a 3.2 mm mounting hole; one via; a part
/// with a fabrication outline and a STEP model.
const BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user) (49 "F.Fab" user))
  (net 0 "") (net 1 "GND")
  (footprint "Conn" (layer "F.Cu") (at 20 20)
    (property "Reference" "J1")
    (pad "1" thru_hole circle (at 0 0) (size 1.8 1.8) (drill 1) (layers "*.Cu" "*.Mask") (net 1 "GND"))
    (pad "2" thru_hole circle (at 2.54 0) (size 1.8 1.8) (drill 1) (layers "*.Cu" "*.Mask"))
    (pad "3" thru_hole oval (at 5.08 0) (size 1.8 2.8) (drill oval 1 2) (layers "*.Cu" "*.Mask")))
  (footprint "Hole" (layer "F.Cu") (at 52 32)
    (property "Reference" "H1")
    (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask")))
  (footprint "Chip" (layer "F.Cu") (at 40 12 90)
    (property "Reference" "U1")
    (fp_rect (start -2 -1) (end 2 1) (stroke (width 0.1)) (layer "F.Fab"))
    (pad "1" smd rect (at -1.5 0) (size 0.6 0.8) (layers "F.Cu"))
    (model "${KICAD8_3DMODEL_DIR}/Chip.3dshapes/Chip.wrl"))
  (gr_line (start 3 0) (end 57 0) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_arc (start 57 0) (mid 59.12132 0.87868) (end 60 3) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_line (start 60 3) (end 60 37) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_arc (start 60 37) (mid 59.12132 39.12132) (end 57 40) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_line (start 57 40) (end 3 40) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_arc (start 3 40) (mid 0.87868 39.12132) (end 0 37) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_line (start 0 37) (end 0 3) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_arc (start 0 3) (mid 0.87868 0.87868) (end 3 0) (layer "Edge.Cuts") (stroke (width 0.1)))
  (gr_circle (center 10 30) (end 14 30) (layer "Edge.Cuts") (stroke (width 0.1)))
  (segment (start 20 20) (end 30 20) (width 0.5) (layer "F.Cu") (net 1))
  (via (at 30 20) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1)))
"#;

fn session(k: Arc<dyn Kernel>, tag: &str) -> Session {
    let doc = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, k, &CmdCtx::user(0));
    let dir = std::env::temp_dir().join(format!("gusset-pcb-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("board.kicad_pcb"), BOARD).unwrap();
    s.project_dir = Some(dir);
    s
}

#[test]
fn a_kicad_board_comes_in_with_exact_holes() {
    let k: Arc<dyn Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::new().unwrap());
    let mut s = session(k.clone(), "holes");
    let r =
        execute(&mut s, &CmdCtx::user(1), "pcb.import", json!({"path": "board.kicad_pcb", "copper": true})).unwrap();
    assert_eq!(r["warnings_user_text"], json!([]), "{r}");
    let ev = s.evaluate(&Actor::User).unwrap();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    let labels: Vec<String> =
        s.document(&Actor::User).unwrap().features.iter().filter_map(|f| f.label.clone()).collect();
    assert_eq!(labels, vec!["Board", "F.Cu", "B.Cu", "J1", "U1"], "{r}");
    let board = &ev.bodies[0].shape;
    let rounded = 60.0 * 40.0 - (4.0 - PI) * 9.0;
    let slot = PI * 0.25 + 1.0 * 1.0; // a 1 mm wide, 2 mm long slot
    let holes = PI * 16.0 + 2.0 * PI * 0.25 + slot + PI * 1.6 * 1.6;
    let want = (rounded - holes) * 1.6;
    let v = k.mass_properties(board).unwrap().volume;
    assert!((v - want).abs() < 1e-3 * want, "board volume {v}, expected {want}");
    let bb = k.bbox(board).unwrap();
    assert!((bb.max[0] - 60.0).abs() < 1e-6 && (bb.min[1] + 40.0).abs() < 1e-6 && (bb.max[2] - 1.6).abs() < 1e-6);
    // Copper on the top face, 35 µm thick.
    let cu = k.bbox(&ev.bodies[1].shape).unwrap();
    assert!((cu.min[2] - 1.6).abs() < 1e-6 && (cu.max[2] - 1.635).abs() < 1e-6, "{cu:?}");
    // With vias cut too, the board loses the via's hole.
    let mut s2 = session(k.clone(), "vias");
    execute(
        &mut s2,
        &CmdCtx::user(1),
        "pcb.import",
        json!({"path": "board.kicad_pcb", "vias": true, "components": false}),
    )
    .unwrap();
    let ev2 = s2.evaluate(&Actor::User).unwrap();
    let v2 = k.mass_properties(&ev2.bodies[0].shape).unwrap().volume;
    assert!((v - v2 - PI * 0.04 * 1.6).abs() < 1e-4, "{v} {v2}");
}

#[test]
fn a_footprint_step_model_replaces_its_box() {
    let worker = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/gusset-worker");
    if !worker.exists() {
        panic!("build gusset-worker first (cargo build -p gusset-worker)");
    }
    // The kernel reads the worker's bodies only through the worker's check, as in the app.
    let k: Arc<dyn Kernel> = Arc::new(gusset_kernel_occt::OcctKernel::with_worker(Some(worker.clone())).unwrap());
    let mut s = session(k.clone(), "step");
    // A 4 × 2 × 1 mm STEP "model" centred on its origin, as library models are.
    let t = gusset_doc::Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut model = Session::new(&t, k.clone(), &CmdCtx::user(0));
    execute(&mut model, &CmdCtx::user(1), "sketch.create", json!({"plane": "XY"})).unwrap();
    execute(
        &mut model,
        &CmdCtx::user(1),
        "sketch.add_rectangle",
        json!({"sketch": "Sketch1", "x": -2, "y": -1, "width": 4, "height": 2}),
    )
    .unwrap();
    execute(&mut model, &CmdCtx::user(1), "feature.extrude", json!({"sketch": "Sketch1", "distance": "1 mm"})).unwrap();
    let ev = model.evaluate(&Actor::User).unwrap();
    let step = k.export_step(&[(&ev.bodies[0].shape, "chip")]).unwrap();
    let dir = s.project_dir.clone().unwrap();
    std::fs::create_dir_all(dir.join("models/Chip.3dshapes")).unwrap();
    std::fs::write(dir.join("models/Chip.3dshapes/Chip.step"), step).unwrap();
    s.importer = Some(Arc::new(gusset_worker::WorkerImporter::new(worker)));
    let r = execute(&mut s, &CmdCtx::user(1), "pcb.import", json!({"path": "board.kicad_pcb", "models_dir": "models"}))
        .unwrap();
    assert!(r["warnings_user_text"].to_string().contains("1 components from STEP models, 1 as boxes"), "{r}");
    let ev = s.evaluate(&Actor::User).unwrap();
    let u1 = &ev.bodies[2].shape;
    let bb = k.bbox(u1).unwrap();
    // Turned 90° with U1 and set on the top face at (40, -12).
    assert!((bb.min[0] - 39.0).abs() < 1e-6 && (bb.max[0] - 41.0).abs() < 1e-6, "{bb:?}");
    assert!((bb.min[1] + 14.0).abs() < 1e-6 && (bb.max[1] + 10.0).abs() < 1e-6, "{bb:?}");
    assert!((bb.min[2] - 1.6).abs() < 1e-6 && (bb.max[2] - 2.6).abs() < 1e-6, "{bb:?}");
    assert!((k.mass_properties(u1).unwrap().volume - 8.0).abs() < 1e-6);
}
