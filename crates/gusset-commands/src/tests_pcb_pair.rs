//! Differential pairs, net lengths and length tuning through the registry (ADR 0055).

use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::json;

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

fn open(s: &mut Session) -> std::sync::Arc<pcb_session::OpenBoard> {
    pcb_session::open_board(s, &gusset_history::Actor::User).unwrap().unwrap()
}

/// 40 x 24 mm: USB_P/USB_N pads at both ends, a SIG wall across the middle, a short USB_P
/// run and a longer jogged USB_N run (0.83 mm of skew), and a via on net A.
const PAIR: &str = r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "USB_P") (net 2 "USB_N") (net 3 "SIG") (net 4 "A")
  (gr_rect (start 0 0) (end 40 24) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (footprint "J" (layer "F.Cu") (at 4 10)
    (property "Reference" "J1" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 -1) (size 0.8 0.5) (layers "F.Cu") (net 1 "USB_P"))
    (pad "2" smd rect (at 0 1) (size 0.8 0.5) (layers "F.Cu") (net 2 "USB_N")))
  (footprint "J" (layer "F.Cu") (at 36 10)
    (property "Reference" "J2" (at 0 0) (layer "F.Fab"))
    (pad "1" smd rect (at 0 -1) (size 0.8 0.5) (layers "F.Cu") (net 1 "USB_P"))
    (pad "2" smd rect (at 0 1) (size 0.8 0.5) (layers "F.Cu") (net 2 "USB_N")))
  (segment (start 20 2) (end 20 14) (width 0.5) (layer "F.Cu") (net 3))
  (segment (start 22 20) (end 34 20) (width 0.2) (layer "F.Cu") (net 1))
  (segment (start 22 22) (end 25 22) (width 0.2) (layer "F.Cu") (net 2))
  (segment (start 25 22) (end 26 23) (width 0.2) (layer "F.Cu") (net 2))
  (segment (start 26 23) (end 30 23) (width 0.2) (layer "F.Cu") (net 2))
  (segment (start 30 23) (end 31 22) (width 0.2) (layer "F.Cu") (net 2))
  (segment (start 31 22) (end 34 22) (width 0.2) (layer "F.Cu") (net 2))
  (via (at 8 4) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 4)))"#;

fn pair_session() -> Session {
    let s = session();
    std::fs::write(s.project_dir.clone().unwrap().join("pair.kicad_pcb"), PAIR).unwrap();
    s
}

#[test]
fn a_pair_is_routed_with_its_class_width_and_gap_and_undone_in_one_step() {
    let mut s = pair_session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "pair.kicad_pcb"}));
    let class = json!({"name": "HS", "nets": ["USB_P", "USB_N"], "diff_pair_width": 0.15, "diff_pair_gap": 0.25});
    run(&mut s, &u(2), "pcb.net_class", class);
    let tracks = open(&mut s).board.tracks.len();
    let r = run(&mut s, &u(3), "pcb.route_pair", json!({"pair": "USB", "points": [[6, 10], [34, 10]]}));
    assert_eq!((r["width"].as_f64(), r["gap"].as_f64()), (Some(0.15), Some(0.25)), "{r}");
    assert_eq!(r["p_user_text"], "USB_P");
    assert!(r["skew_mm"].as_f64().unwrap() < 0.05, "{r}");
    let b = open(&mut s);
    assert_eq!(b.board.tracks.len(), tracks + r["tracks"].as_u64().unwrap() as usize);
    assert!(b.board.tracks.iter().skip(tracks).all(|t| (t.width - 0.15).abs() < 1e-9));
    assert!(b.board.tracks.iter().skip(tracks).any(|t| t.start[1] > 14.0), "round the wall");
    let v = gusset_pcb::drc::check(&b.board, &b.rules, 50);
    assert!(v.is_empty(), "{v:?}");
    run(&mut s, &u(4), "history.undo", json!({}));
    assert_eq!(open(&mut s).board.tracks.len(), tracks);
    // Either net name and overrides work; bad input is refused and changes nothing.
    let pts = json!([[6, 10], [34, 10]]);
    let r = run(&mut s, &u(5), "pcb.route_pair", json!({"pair": "USB_N", "points": pts, "width": 0.2, "gap": 0.3}));
    assert_eq!((r["width"].as_f64(), r["gap"].as_f64()), (Some(0.2), Some(0.3)), "{r}");
    run(&mut s, &u(6), "history.undo", json!({}));
    let bad = |s: &mut Session, t: u64, a: serde_json::Value| execute(s, &u(t), "pcb.route_pair", a).unwrap_err().code;
    assert_eq!(bad(&mut s, 7, json!({"pair": "SIG", "points": pts})), "not_found");
    assert_eq!(bad(&mut s, 8, json!({"pair": "USB", "points": [[6, 10]]})), "invalid_args");
    assert_eq!(bad(&mut s, 9, json!({"pair": "USB", "points": pts, "layer": "F.SilkS"})), "invalid_args");
    assert_eq!(bad(&mut s, 10, json!({"pair": "USB", "points": [[6, 10], [20, 5]]})), "not_found");
    assert_eq!(open(&mut s).board.tracks.len(), tracks);
}

#[test]
fn lengths_report_nets_vias_and_pair_skew() {
    let mut s = pair_session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "pair.kicad_pcb"}));
    let r = run(&mut s, &u(2), "pcb.lengths", json!({}));
    let net = |n: &str| r["nets"].as_array().unwrap().iter().find(|x| x["net_user_text"] == n).cloned().unwrap();
    assert_eq!(net("USB_P")["length_mm"], 12.0);
    assert_eq!(net("A")["via_mm"], 1.6);
    assert_eq!(net("A")["vias"], 1);
    let skew = r["pairs"][0]["skew_mm"].as_f64().unwrap();
    assert!((skew - (10.0 + 2.0 * std::f64::consts::SQRT_2 - 12.0)).abs() < 1e-3, "{r}");
    assert_eq!(r["pairs"][0]["shorter_user_text"], "USB_P");
    let r = run(&mut s, &u(3), "pcb.lengths", json!({"net": "USB"}));
    assert_eq!(r["nets"].as_array().unwrap().len(), 2);
    let r = run(&mut s, &u(3), "pcb.lengths", json!({"net": "nothing"}));
    assert!(r["nets"].as_array().unwrap().is_empty());
}

#[test]
fn the_shorter_side_is_tuned_in_one_undo_step() {
    let mut s = pair_session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "pair.kicad_pcb"}));
    let before = open(&mut s).board.tracks.len();
    let r = run(&mut s, &u(2), "pcb.tune", json!({"net": "USB"}));
    assert_eq!(r["net_user_text"], "USB_P", "{r}");
    assert!(r["bumps"].as_u64().unwrap() >= 1, "{r}");
    let b = open(&mut s);
    assert!(b.board.tracks.len() > before);
    let k = gusset_pcb::length::skews(&b.board).remove(0);
    assert!(k.skew() <= 0.05, "{}", k.skew());
    let v = gusset_pcb::drc::check(&b.board, &b.rules, 50);
    assert!(v.is_empty(), "{v:?}");
    run(&mut s, &u(3), "history.undo", json!({}));
    assert_eq!(open(&mut s).board.tracks.len(), before);
    // A target on a named net, with the options; already there; no room; bad input.
    let a = json!({"net": "USB_P", "target": 16.0, "tolerance": 0.02, "amplitude": 1.5, "spacing": 0.3});
    let r = run(&mut s, &u(4), "pcb.tune", a);
    assert!((r["after_mm"].as_f64().unwrap() - 16.0).abs() <= 0.02, "{r}");
    let e = execute(&mut s, &u(5), "pcb.tune", json!({"net": "USB_P", "target": 16.0})).unwrap_err();
    assert_eq!(e.code, "not_found", "{e:?}");
    let e = execute(&mut s, &u(6), "pcb.tune", json!({"net": "SIG"})).unwrap_err();
    assert_eq!(e.code, "invalid_args");
    let e = execute(&mut s, &u(6), "pcb.tune", json!({"net": "A", "target": 99.0})).unwrap_err();
    assert_eq!(e.code, "not_found");
    let e = execute(&mut s, &u(6), "pcb.tune", json!({"net": "SIG", "target": -1.0})).unwrap_err();
    assert_eq!(e.code, "invalid_args");
}
