//! PCB commands (ADR 0047) through the registry, with the mock kernel.

use crate::*;
use gusset_doc::Document;
use gusset_kernel::Kernel;
use gusset_kernel::mock::MockKernel;
use serde_json::{Value, json};
use std::sync::Arc;

/// A 40 × 20 mm board with a resistor (pads only), a part with a fabrication outline and a
/// 3D model, and a through-hole pin.
pub(crate) const BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
  (general (thickness 1.2))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (39 "F.Mask" user) (38 "B.Mask" user)
    (35 "F.Paste" user) (34 "B.Paste" user) (36 "B.SilkS" user) (44 "Edge.Cuts" user) (49 "F.Fab" user))
  (net 0 "") (net 1 "GND")
  (footprint "R" (layer "F.Cu") (at 10 10)
    (property "Reference" "R1")
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (net 1 "GND"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))
  (footprint "U" (layer "F.Cu") (at 25 10 90)
    (property "Reference" "U1")
    (fp_rect (start -3 -2) (end 3 2) (stroke (width 0.1)) (layer "F.Fab"))
    (pad "1" smd rect (at -2 0) (size 0.5 1) (layers "F.Cu"))
    (model "${KICAD8_3DMODEL_DIR}/Package.3dshapes/U.wrl" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0))))
  (footprint "P" (layer "B.Cu") (at 35 15)
    (property "Reference" "J1")
    (pad "1" thru_hole circle (at 0 0) (size 1.8 1.8) (drill 1) (layers "*.Cu" "*.Mask") (net 1 "GND")))
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.1)) (layer "Edge.Cuts"))
  (segment (start 9 10) (end 35 15) (width 0.3) (layer "F.Cu") (net 1)))
"#;

pub(crate) fn session() -> Session {
    let doc = Document::from_text("gusset 1\nunits mm\n").unwrap();
    let mut s = Session::new(&doc, Arc::new(MockKernel), &CmdCtx::user(0));
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = format!("{n}-{}", SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let dir = std::env::temp_dir().join(format!("gusset-pcb-{}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("fab")).unwrap();
    std::fs::write(dir.join("board.kicad_pcb"), BOARD).unwrap();
    s.project_dir = Some(dir);
    s
}

pub(crate) fn run(s: &mut Session, ctx: &CmdCtx, id: &str, args: Value) -> Value {
    execute(s, ctx, id, args).unwrap_or_else(|e| panic!("{id}: {e:?}"))
}

#[test]
fn pcb_info_reports_the_board() {
    let mut s = session();
    let r = run(&mut s, &CmdCtx::user(1), "pcb.info", json!({"path": "board.kicad_pcb"}));
    assert_eq!(r["footprints"], 3, "{r}");
    assert_eq!(r["pads"], 4, "{r}");
    assert_eq!(r["thickness_mm"], 1.2, "{r}");
    assert_eq!(r["size_mm"], json!([40.0, 20.0]), "{r}");
    assert_eq!(r["holes"], json!({"plated": 1, "non_plated": 0}), "{r}");
    assert_eq!(r["issues"], json!([]), "{r}");
    assert_eq!(r["model_references"], 1, "{r}");
    // Not a board file.
    std::fs::write(s.project_dir.clone().unwrap().join("x.kicad_pcb"), "(kicad_sch)").unwrap();
    let e = execute(&mut s, &CmdCtx::user(1), "pcb.info", json!({"path": "x.kicad_pcb"})).unwrap_err();
    assert_eq!(e.code, "io");
    let e = execute(&mut s, &CmdCtx::user(1), "pcb.info", json!({"path": "board.step"})).unwrap_err();
    assert_eq!(e.code, "invalid_args");
}

#[test]
fn pcb_import_adds_the_board_and_component_bodies() {
    let mut s = session();
    let r = run(&mut s, &CmdCtx::user(1), "pcb.import", json!({"path": "board.kicad_pcb", "component_height": "3 mm"}));
    let features = r["features"].as_array().unwrap();
    // Board, R1, U1 and J1 (on the bottom).
    assert_eq!(features.len(), 4, "{r}");
    assert_eq!(r["tier"], json!(crate::commands::pcb::TIER_PCB), "{r}");
    // The mock kernel cannot cut holes: said, and the board still comes in.
    let w = r["warnings_user_text"].to_string();
    assert!(w.contains("holes could not be cut"), "{r}");
    let ev = s.evaluate(&gusset_history::Actor::User).unwrap();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    assert_eq!(ev.bodies.len(), 4);
    let k = MockKernel;
    let boxes: Vec<gusset_kernel::BBox> = ev.bodies.iter().map(|b| k.bbox(&b.shape).unwrap()).collect();
    // The board: 40 × 20 × 1.2, Y flipped.
    let b = &boxes[0];
    assert!((b.max[0] - 40.0).abs() < 1e-9 && (b.min[1] + 20.0).abs() < 1e-9 && (b.max[2] - 1.2).abs() < 1e-9);
    // U1: its 6 × 4 fabrication outline turned 90°, standing 3 mm on the top.
    let u = &boxes[2];
    assert!((u.max[0] - u.min[0] - 4.0).abs() < 1e-9 && (u.max[1] - u.min[1] - 6.0).abs() < 1e-9, "{u:?}");
    assert!((u.min[2] - 1.2).abs() < 1e-9 && (u.max[2] - 4.2).abs() < 1e-9, "{u:?}");
    // J1 hangs under the bottom.
    assert!((boxes[3].max[2]).abs() < 1e-9 && (boxes[3].min[2] + 3.0).abs() < 1e-9, "{:?}", boxes[3]);
    // Bad height.
    let e = execute(
        &mut s,
        &CmdCtx::user(1),
        "pcb.import",
        json!({"path": "board.kicad_pcb", "component_height": "-1 mm"}),
    )
    .unwrap_err();
    assert_eq!(e.code, "invalid_args");
}

/// An importer that turns any STEP into one 2 × 2 × 2 mm cube at the origin.
struct CubeImporter(std::sync::Mutex<usize>);

impl session::Importer for CubeImporter {
    fn import(
        &self,
        _format: &str,
        _bytes: &[u8],
        _cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<(String, Vec<u8>)>, String> {
        *self.0.lock().unwrap() += 1;
        let k = MockKernel;
        let sq = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
        let edges = (0..4)
            .map(|i| gusset_kernel::ProfileEdge {
                seg: gusset_kernel::Seg2::Line { a: sq[i], b: sq[(i + 1) % 4] },
                tag: i as u64 + 1,
            })
            .collect();
        let p = gusset_kernel::Profile {
            frame: gusset_kernel::Frame::XY,
            regions: vec![gusset_kernel::Region2 { outer: gusset_kernel::Loop2 { edges }, holes: vec![] }],
        };
        let shape = k.extrude(&p, &gusset_kernel::ExtrudeSpec { distance: 2.0, symmetric: false }).unwrap().shape;
        Ok(vec![("cube".into(), k.write_native(&shape).unwrap())])
    }
}

#[test]
fn pcb_import_places_step_models_from_the_models_folder() {
    let mut s = session();
    let dir = s.project_dir.clone().unwrap();
    std::fs::create_dir_all(dir.join("models/Package.3dshapes")).unwrap();
    std::fs::write(dir.join("models/Package.3dshapes/U.step"), "ISO-10303-21;").unwrap();
    let imp = Arc::new(CubeImporter(std::sync::Mutex::new(0)));
    s.importer = Some(imp.clone());
    let r =
        run(&mut s, &CmdCtx::agent("bot", 1), "pcb.import", json!({"path": "board.kicad_pcb", "models_dir": "models"}));
    assert!(r["warnings_user_text"].to_string().contains("1 components from STEP models, 2 as boxes"), "{r}");
    assert_eq!(*imp.0.lock().unwrap(), 1);
    let ev = s.evaluate(&gusset_history::Actor::Agent("bot".into())).unwrap();
    let k = MockKernel;
    let u = k.bbox(&ev.bodies[2].shape).unwrap();
    // The cube, turned with U1 and moved to (25, -10) on the top face.
    assert!((u.min[0] - 24.0).abs() < 1e-9 && (u.max[1] + 9.0).abs() < 1e-9, "{u:?}");
    assert!((u.min[2] - 1.2).abs() < 1e-9 && (u.max[2] - 3.2).abs() < 1e-9, "{u:?}");
}

#[test]
fn model_paths_stay_inside_the_models_folder() {
    use crate::commands::pcb::model_candidates;
    let c = model_candidates("${KICAD8_3DMODEL_DIR}/Package.3dshapes/U.wrl");
    assert_eq!(c.first().map(String::as_str), Some("Package.3dshapes/U.step"));
    assert!(c.contains(&"U.stp".to_string()));
    assert!(model_candidates("../../etc/passwd.step").is_empty());
    assert!(model_candidates("a/.hidden/x.step").is_empty());
    let abs = model_candidates("C:\\Users\\x\\m.step");
    assert!(abs.iter().all(|p| !p.contains(':') && !p.starts_with('/')), "{abs:?}");
}

#[test]
fn pcb_exports_write_gerber_and_drill_files() {
    let mut s = session();
    let user = CmdCtx::user(1);
    let r = run(&mut s, &user, "pcb.export_gerbers", json!({"path": "board.kicad_pcb", "folder": "fab"}));
    let written: Vec<&str> = r["written"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(written.len(), 9, "{r}");
    assert!(written.contains(&"fab/board-F_Cu.gbr") && written.contains(&"fab/board-Edge_Cuts.gbr"), "{r}");
    let dir = s.project_dir.clone().unwrap();
    let cu = std::fs::read_to_string(dir.join("fab/board-F_Cu.gbr")).unwrap();
    assert!(cu.contains("%TF.FileFunction,Copper,L1,Top*%") && cu.ends_with("M02*\n"));
    // Again: refused without overwrite, done with it.
    let e =
        execute(&mut s, &user, "pcb.export_gerbers", json!({"path": "board.kicad_pcb", "folder": "fab"})).unwrap_err();
    assert_eq!(e.code, "confirmation_required");
    run(&mut s, &user, "pcb.export_gerbers", json!({"path": "board.kicad_pcb", "folder": "fab", "overwrite": true}));
    // Agents: inside the project only.
    let bot = CmdCtx::agent("bot", 2);
    let r = run(&mut s, &bot, "pcb.export_drill", json!({"path": "board.kicad_pcb", "folder": "fab"}));
    assert_eq!(r["written"], json!(["fab/board-PTH.drl", "fab/board-NPTH.drl"]), "{r}");
    assert_eq!(r["plated"], 1);
    let drl = std::fs::read_to_string(dir.join("fab/board-PTH.drl")).unwrap();
    assert!(drl.contains("T1C1.000") && drl.contains("X35.0000Y-15.0000"), "{drl}");
    let e =
        execute(&mut s, &bot, "pcb.export_drill", json!({"path": "board.kicad_pcb", "folder": "../out"})).unwrap_err();
    assert_eq!(e.code, "not_allowed");
    let e = execute(&mut s, &bot, "pcb.info", json!({"path": "/etc/board.kicad_pcb"})).unwrap_err();
    assert_eq!(e.code, "not_allowed");
}
