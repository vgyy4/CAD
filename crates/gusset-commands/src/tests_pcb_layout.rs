//! Board layout commands (ADR 0047 phase 2b): zone fills, routing, STEP export and the
//! Specctra hand-off.

use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::json;

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

fn open(s: &mut Session) -> std::sync::Arc<pcb_session::OpenBoard> {
    pcb_session::open_board(s, &gusset_history::Actor::User).unwrap().unwrap()
}

/// A 30 × 20 mm board: two resistors (R1 GND/A, R2 A/GND), a SIG via in the way and a
/// GND zone on B.Cu.
pub(crate) const LAYOUT: &str = r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user) (37 "F.SilkS" user) (49 "F.Fab" user))
  (net 0 "")
  (net 1 "GND")
  (net 2 "A")
  (net 3 "SIG")
  (gr_rect (start 0 0) (end 30 20) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (footprint "R:R" (layer "F.Cu") (at 5 10)
    (property "Reference" "R1" (at 0 -1.5) (layer "F.SilkS"))
    (property "Value" "1k" (at 0 1.5) (layer "F.Fab"))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu") (net 1 "GND"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu") (net 2 "A")))
  (footprint "R:R" (layer "F.Cu") (at 25 10)
    (property "Reference" "R2" (at 0 -1.5) (layer "F.SilkS"))
    (property "Value" "1k" (at 0 1.5) (layer "F.Fab"))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu") (net 2 "A"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu") (net 1 "GND")))
  (via (at 15 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 3))
  (via (at 3 4) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1))
  (zone (net 1) (net_name "GND") (layer "B.Cu") (hatch edge 0.5) (connect_pads (clearance 0.3)) (min_thickness 0.25)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
    (polygon (pts (xy 1 1) (xy 29 1) (xy 29 19) (xy 1 19))))
)"#;

fn layout_session() -> Session {
    let s = session();
    let dir = s.project_dir.clone().unwrap();
    std::fs::write(dir.join("layout.kicad_pcb"), LAYOUT).unwrap();
    s
}

#[test]
fn zones_fill_with_undo_and_reach_the_gerbers() {
    let mut s = layout_session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "layout.kicad_pcb"}));
    let e = execute(&mut s, &u(2), "pcb.fill_zones", json!({"zones": [3]})).unwrap_err();
    assert_eq!(e.code, "not_found");
    let r = run(&mut s, &u(2), "pcb.fill_zones", json!({}));
    let area = r["zones"][0]["area_mm2"].as_f64().unwrap();
    // 28 × 18 less the SIG via's clearance disc (r 0.4 + 0.3) and the corners.
    let want = 28.0 * 18.0 - std::f64::consts::PI * 0.706 * 0.706;
    assert!((area - want).abs() < 0.1, "{r}");
    let b = open(&mut s);
    assert_eq!(b.board.zones[0].fills.len(), 1);
    assert!(gusset_pcb::drc::check(&b.board, &b.rules, 50).iter().all(|v| !v.message.contains("zone")));
    // The Gerber of B.Cu carries the pour.
    let dir = s.project_dir.clone().unwrap();
    std::fs::create_dir_all(dir.join("g")).unwrap();
    run(&mut s, &u(3), "pcb.export_gerbers", json!({"folder": "g"}));
    let gbr = std::fs::read_to_string(dir.join("g/layout-B_Cu.gbr")).unwrap();
    assert!(gbr.contains("G36*"), "a region");
    run(&mut s, &u(4), "history.undo", json!({}));
    assert!(open(&mut s).board.zones[0].fills.is_empty());
}

#[test]
fn step_export_checks_its_target_and_reaches_the_kernel() {
    let mut s = layout_session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "layout.kicad_pcb"}));
    let a = CmdCtx::agent("bot", 2);
    let e = execute(&mut s, &a, "pcb.export_step", json!({"file": "b.stl"})).unwrap_err();
    assert_eq!(e.code, "invalid_args");
    let e = execute(&mut s, &a, "pcb.export_step", json!({"file": "../b.step"})).unwrap_err();
    assert_eq!(e.code, "not_allowed", "{e:?}");
    // The mock kernel writes no STEP: a clean io error, no file.
    let e = execute(&mut s, &a, "pcb.export_step", json!({"file": "b.step"})).unwrap_err();
    assert_eq!(e.code, "io", "{e:?}");
    assert!(!s.project_dir.clone().unwrap().join("b.step").exists());
}
