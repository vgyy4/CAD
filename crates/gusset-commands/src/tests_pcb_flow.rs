//! The PCB workflow end to end from nothing (ADR 0047 phase 3): a new schematic from a
//! symbol library, wired, annotated and checked; the board updated from it, placed,
//! given a net class and a test point, routed (one track shoved aside), a hatched ground
//! zone filled, checked, and every fabrication file written; then saved and opened again.
//! Hand-written library and footprints only (no KiCad library content).

#![allow(clippy::float_cmp)]

use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::{Value, json};

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

/// A resistor, a two-pin connector and the power symbols.
const LIB: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "10k" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Footprint" "Pads:R0805" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "Conn" (in_bom yes) (on_board yes)
    (property "Reference" "J" (at 0 2.54 0) (effects (font (size 1.27 1.27))))
    (property "Value" "Power" (at 0 -5.08 0) (effects (font (size 1.27 1.27))))
    (property "Footprint" "Pads:Header2" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "Conn_1_1"
      (pin passive line (at -5.08 0 0) (length 3.81) (name "Pin_1" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at -5.08 -2.54 0) (length 3.81) (name "Pin_2" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "+5V" (power) (in_bom yes) (on_board yes)
    (property "Reference" "#PWR" (at 0 -3.81 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Value" "+5V" (at 0 3.556 0) (effects (font (size 1.27 1.27))))
    (symbol "+5V_0_1" (pin power_in line (at 0 0 90) (length 0) (hide yes) (name "+5V" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))))
  (symbol "GND" (power) (in_bom yes) (on_board yes)
    (property "Reference" "#PWR" (at 0 -6.35 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Value" "GND" (at 0 -3.81 0) (effects (font (size 1.27 1.27))))
    (symbol "GND_0_1" (pin power_in line (at 0 0 270) (length 0) (hide yes) (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))))
  (symbol "PWR_FLAG" (power) (in_bom yes) (on_board yes)
    (property "Reference" "#FLG" (at 0 1.905 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Value" "PWR_FLAG" (at 0 3.81 0) (effects (font (size 1.27 1.27))))
    (symbol "PWR_FLAG_0_0" (pin power_out line (at 0 0 90) (length 0) (name "pwr" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))))
"##;

const R0805: &str = r#"(footprint "R0805" (version 20241229) (generator "hand") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -1.65 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "R0805" (at 0 1.65 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (fp_line (start -1 -0.6) (end 1 -0.6) (stroke (width 0.1) (type solid)) (layer "F.Fab"))
  (pad "1" smd roundrect (at -0.95 0) (size 1 1.25) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25))
  (pad "2" smd roundrect (at 0.95 0) (size 1 1.25) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25)))"#;

const HEADER: &str = r#"(footprint "Header2" (version 20241229) (generator "hand") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -2.5 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Header2" (at 0 5 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (pad "1" thru_hole rect (at 0 0) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask"))
  (pad "2" thru_hole circle (at 0 2.54) (size 1.7 1.7) (drill 1) (layers "*.Cu" "*.Mask")))"#;

const TP: &str = r#"(footprint "TestPoint" (version 20241229) (generator "hand") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -1.5 0) (layer "F.SilkS") (effects (font (size 0.8 0.8) (thickness 0.12))))
  (pad "1" smd circle (at 0 0) (size 1.5 1.5) (layers "F.Cu" "F.Mask")))"#;

fn setup() -> Session {
    let s = session();
    let dir = s.project_dir.clone().unwrap();
    std::fs::write(dir.join("parts.kicad_sym"), LIB).unwrap();
    std::fs::create_dir_all(dir.join("fp/Pads.pretty")).unwrap();
    std::fs::write(dir.join("fp/Pads.pretty/R0805.kicad_mod"), R0805).unwrap();
    std::fs::write(dir.join("fp/Pads.pretty/Header2.kicad_mod"), HEADER).unwrap();
    std::fs::write(dir.join("fp/Pads.pretty/TestPoint.kicad_mod"), TP).unwrap();
    std::fs::create_dir_all(dir.join("fab")).unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    s
}

fn board(s: &Session) -> std::sync::Arc<pcb_session::OpenBoard> {
    pcb_session::open_board(s, &gusset_history::Actor::User).unwrap().unwrap()
}

/// The centre of pad `number` of footprint `reference`.
fn pad(s: &Session, reference: &str, number: &str) -> [f64; 2] {
    let b = board(s);
    let f = b.board.footprints.iter().find(|f| f.reference == reference).unwrap();
    let p = f.pads.iter().map(|&i| &b.board.pads[i]).find(|p| p.number == number).unwrap();
    p.at
}

fn add(s: &mut Session, t: u64, symbol: &str, at: [f64; 2], reference: Option<&str>) {
    let mut a = json!({"library": "parts.kicad_sym", "symbol": symbol, "at": at});
    if let Some(r) = reference {
        a["reference"] = json!(r);
    }
    run(s, &u(t), "sch.add_symbol", a);
}

#[test]
fn a_board_is_made_from_nothing_through_to_fabrication_files() {
    let mut s = setup();
    // --- Schematic: J1 brings +5V and GND; R1 and R2 divide it into MID.
    run(&mut s, &u(1), "sch.new", json!({"name": "divider"}));
    add(&mut s, 2, "Conn", [80.0, 50.0], None); // pins at (74.92, 50) and (74.92, 52.54)
    add(&mut s, 3, "R", [100.0, 50.0], Some("R?")); // pins at (100, 46.19) and (100, 53.81)
    add(&mut s, 4, "R", [110.0, 50.0], Some("R?"));
    for (t, sym, at) in [
        (5, "+5V", [74.92, 50.0]),
        (6, "GND", [74.92, 52.54]),
        (7, "PWR_FLAG", [74.92, 50.0]),
        (8, "PWR_FLAG", [74.92, 52.54]),
        (9, "+5V", [100.0, 46.19]),
        (10, "GND", [110.0, 53.81]),
    ] {
        add(&mut s, t, sym, at, Some(if sym == "PWR_FLAG" { "#FLG?" } else { "#PWR?" }));
    }
    run(&mut s, &u(11), "sch.add_wire", json!({"points": [[100, 53.81], [105, 53.81], [105, 46.19], [110, 46.19]]}));
    run(&mut s, &u(12), "sch.add_label", json!({"text": "MID", "at": [105, 50]}));
    // Unannotated parts are an ERC finding until annotated.
    let r = run(&mut s, &u(13), "sch.erc", json!({}));
    assert!(r["issues"].as_array().unwrap().iter().any(|i| i["code"] == "unannotated"), "{r}");
    let r = run(&mut s, &u(14), "sch.annotate", json!({}));
    assert_eq!(r["count"], 8, "{r}");
    let r = run(&mut s, &u(15), "sch.erc", json!({}));
    assert_eq!(r["issues"], json!([]), "{r}");
    let r = run(&mut s, &u(16), "sch.netlist", json!({}));
    let nets = r["nets"].as_array().unwrap();
    let net = |name: &str| nets.iter().find(|n| n["name_user_text"] == name).map(|n| n["pins_user_text"].clone());
    assert_eq!(net("+5V"), Some(json!(["J1.1", "R1.1"])), "{r}");
    assert_eq!(net("GND"), Some(json!(["J1.2", "R2.2"])), "{r}");
    assert_eq!(net("/MID"), Some(json!(["R1.2", "R2.1"])), "{r}");
    // Annotation again changes nothing.
    assert_eq!(run(&mut s, &u(17), "sch.annotate", json!({}))["changed"], false);

    // --- Board: footprints from the schematic, placed.
    run(&mut s, &u(20), "pcb.new", json!({"width": "30 mm", "height": "20 mm", "name": "divider"}));
    let r = run(&mut s, &u(21), "pcb.update_from_schematic", json!({"footprints": "fp"}));
    assert_eq!(r["added_user_text"], json!(["J1", "R1", "R2"]), "{r}");
    for (t, fp, at, angle) in [(22, "J1", [5.0, 8.0], 0.0), (23, "R1", [15.0, 6.0], 0.0), (24, "R2", [15.0, 13.0], 0.0)]
    {
        run(&mut s, &u(t), "pcb.move_footprint", json!({"footprint": fp, "at": at, "angle": angle}));
    }
    assert_eq!(board(&s).ratsnest.unrouted(), 3);
    // A test point placed by hand, put on MID by a pad net assignment.
    run(
        &mut s,
        &u(25),
        "pcb.add_footprint",
        json!({"library": "fp", "footprint": "Pads:TestPoint", "at": [25, 10], "reference": "TP1"}),
    );
    let r = run(&mut s, &u(26), "pcb.set_net", json!({"pads": ["TP1.1"], "net": "/MID"}));
    assert_eq!(r["pads_changed"], 1, "{r}");
    assert_eq!(board(&s).ratsnest.unrouted(), 4);
    let e = execute(&mut s, &u(26), "pcb.set_net", json!({"pads": ["TP9.1"], "net": "X"})).unwrap_err();
    assert_eq!(e.code, "not_found");
    // Power nets get wider tracks.
    run(
        &mut s,
        &u(27),
        "pcb.net_class",
        json!({"name": "Power", "track_width": 0.5, "clearance": 0.2, "nets": ["+5V", "GND"]}),
    );
    assert_eq!(board(&s).rules.track_width_of(Some("+5V")), 0.5);
    assert_eq!(board(&s).rules.track_width_of(Some("/MID")), 0.25);

    // --- Routing on F.Cu.
    let pts = json!({"points": [pad(&s, "J1", "1"), pad(&s, "R1", "1")]});
    let r = run(&mut s, &u(30), "pcb.route", pts);
    assert_eq!(r["width"], 0.5, "{r}");
    let pts = json!({"points": [pad(&s, "R1", "2"), pad(&s, "R2", "1")]});
    run(&mut s, &u(31), "pcb.route", pts);
    let pts = json!({"points": [pad(&s, "R2", "1"), pad(&s, "TP1", "1")]});
    run(&mut s, &u(32), "pcb.route", pts);
    // GND: R2 pad 2 down to a via into a hatched ground zone on B.Cu; J1 pad 2 is through-hole.
    let g = pad(&s, "R2", "2");
    let via = [g[0] + 2.0, g[1] + 3.0];
    run(&mut s, &u(33), "pcb.route", json!({"points": [g, via]}));
    run(&mut s, &u(34), "pcb.add_via", json!({"at": via, "net": "GND"}));
    let r = run(
        &mut s,
        &u(35),
        "pcb.add_zone",
        json!({"outline": [[0, 0], [30, 0], [30, 20], [0, 20]], "layers": ["B.Cu"], "net": "GND",
               "hatch": {"thickness": 0.6, "gap": 1.2, "angle": 45}}),
    );
    assert!(r["filled_mm2"].as_f64().unwrap() > 100.0, "{r}");
    assert_eq!(r["unrouted"], 0, "{r}");
    let r = run(&mut s, &u(36), "pcb.fill_zones", json!({}));
    assert_eq!(r["unrouted"], 0, "{r}");
    let r = run(&mut s, &u(37), "pcb.drc", json!({}));
    assert_eq!(r["violations"], json!([]), "{r}");

    // --- Fabrication files.
    let r = run(&mut s, &u(40), "pcb.export_gerbers", json!({"folder": "fab"}));
    let written: Vec<&str> = r["written"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(written.contains(&"fab/divider-F_SilkS.gbr") && written.contains(&"fab/divider-B_Cu.gbr"), "{r}");
    let dir = s.project_dir.clone().unwrap();
    let silk = std::fs::read_to_string(dir.join("fab/divider-F_SilkS.gbr")).unwrap();
    assert!(silk.matches("D01*").count() > 20, "the references are plotted");
    let bcu = std::fs::read_to_string(dir.join("fab/divider-B_Cu.gbr")).unwrap();
    assert!(bcu.matches("G36*").count() >= 1, "the ground zone");
    let r = run(&mut s, &u(41), "pcb.export_drill", json!({"folder": "fab"}));
    assert_eq!(r["plated"], 3, "two header pins and a via: {r}");
    let r = run(&mut s, &u(42), "pcb.export_bom", json!({"file": "fab/bom.csv"}));
    assert_eq!(r["lines"], 3, "{r}");
    let bom = std::fs::read_to_string(dir.join("fab/bom.csv")).unwrap();
    assert!(bom.contains("R1 R2") || bom.contains("\"R1,R2\"") || bom.contains("R1,R2"), "{bom}");
    let r = run(&mut s, &u(43), "pcb.export_pos", json!({"file": "fab/pos.csv"}));
    assert_eq!(r["components"], 4, "{r}");
    // STEP needs the real kernel (gusset-cli's OCCT test); the mock refuses cleanly.
    let e = execute(&mut s, &u(44), "pcb.export_step", json!({"file": "fab/divider.step"})).unwrap_err();
    assert_eq!(e.code, "io", "{e:?}");

    // --- Saved and opened again: the board, its classes and the schematic come back.
    let r = run(&mut s, &u(50), "pcb.save", json!({"path": "out/divider.kicad_pcb"}));
    assert_eq!(r["written"], json!(["out/divider.kicad_pcb", "out/divider.kicad_pro"]), "{r}");
    run(&mut s, &u(51), "sch.save", json!({"path": "out/divider.kicad_sch"}));
    let before = board(&s).board.tree.clone();
    run(&mut s, &u(52), "pcb.open", json!({"path": "out/divider.kicad_pcb"}));
    let b = board(&s);
    assert_eq!(b.board.tree, before);
    assert_eq!(b.rules.track_width_of(Some("GND")), 0.5);
    assert_eq!(b.ratsnest.unrouted(), 0);
    run(&mut s, &u(53), "sch.open", json!({"path": "out/divider.kicad_sch"}));
    let r = run(&mut s, &u(54), "pcb.update_from_schematic", json!({"dry_run": true}));
    assert_eq!(r["pads_changed"], 0, "the board matches the schematic: {r}");

    // --- Net edits after the fact: rename, merge, class delete, undo.
    let r = run(&mut s, &u(60), "pcb.rename_net", json!({"from": "/MID", "to": "MID"}));
    assert_eq!(r["merged"], false, "{r}");
    assert!(board(&s).board.nets.iter().any(|n| n.name == "MID"));
    let r = run(&mut s, &u(61), "pcb.rename_net", json!({"from": "MID", "to": "GND"}));
    assert_eq!(r["merged"], true, "{r}");
    let b = board(&s);
    let tp = b.board.footprints.iter().find(|f| f.reference == "TP1").unwrap();
    assert_eq!(b.board.net_name(&b.board.pads[tp.pads[0]].net).as_deref(), Some("GND"));
    assert!(!b.board.nets.iter().any(|n| n.name == "MID"));
    run(&mut s, &u(63), "history.undo", json!({}));
    run(&mut s, &u(64), "history.undo", json!({}));
    assert!(board(&s).board.nets.iter().any(|n| n.name == "/MID"));
    run(&mut s, &u(65), "pcb.net_class", json!({"name": "Power", "delete": true}));
    assert_eq!(board(&s).rules.track_width_of(Some("GND")), 0.25);
    let e = execute(&mut s, &u(66), "pcb.net_class", json!({"name": "Default", "delete": true})).unwrap_err();
    assert_eq!(e.code, "invalid_args");
    let e = execute(&mut s, &u(66), "pcb.rename_net", json!({"from": "nope", "to": "x"})).unwrap_err();
    assert_eq!(e.code, "not_found");
    // Zones go by index.
    run(&mut s, &u(67), "pcb.delete", json!({"zones": [0]}));
    assert!(board(&s).board.zones.is_empty());
}

#[test]
fn a_blocking_track_is_shoved_through_the_registry() {
    let mut s = setup();
    run(&mut s, &u(1), "pcb.new", json!({"width": "30 mm", "height": "12 mm"}));
    // B across the top of a channel that C closes below and at both ends; A's pads in it.
    let fp = |r: &str, at: [f64; 2], d: f64| {
        format!(
            "(footprint \"P\" (layer \"F.Cu\") (at {} {}) (property \"Reference\" \"{r}\" (at 0 0) (layer \"F.Fab\")) (pad \"1\" smd circle (at 0 0) (size {d} {d}) (layers \"F.Cu\")))",
            at[0], at[1]
        )
    };
    let parts = [
        fp("B1", [2.0, 5.0], 0.6),
        fp("B2", [28.0, 5.0], 0.6),
        fp("A1", [8.0, 6.0], 0.5),
        fp("A2", [22.0, 6.0], 0.5),
        fp("C1", [3.0, 6.2], 1.2),
        fp("C2", [27.0, 6.2], 1.2),
        fp("C3", [15.0, 6.3], 1.4),
    ];
    let dir = s.project_dir.clone().unwrap();
    let text = board(&s).board.write().replacen("(net 0 \"\")", &format!("(net 0 \"\") {}", parts.join(" ")), 1);
    std::fs::write(dir.join("ch.kicad_pcb"), text).unwrap();
    run(&mut s, &u(2), "pcb.open", json!({"path": "ch.kicad_pcb"}));
    for (r, n) in [("B1", "B"), ("B2", "B"), ("A1", "A"), ("A2", "A"), ("C1", "C"), ("C2", "C"), ("C3", "C")] {
        run(&mut s, &u(3), "pcb.set_net", json!({"pads": [format!("{r}.1")], "net": n}));
    }
    run(&mut s, &u(4), "pcb.route", json!({"points": [[2, 5], [28, 5]], "around": false}));
    run(&mut s, &u(5), "pcb.route", json!({"points": [[3, 7], [27, 7]], "width": 1.0, "net": "C", "around": false}));
    // Without shoving there is no way.
    let e = execute(&mut s, &u(6), "pcb.route", json!({"points": [[8, 6], [22, 6]], "shove": false})).unwrap_err();
    assert_eq!(e.code, "not_found");
    let r = run(&mut s, &u(7), "pcb.route", json!({"points": [[8, 6], [22, 6]]}));
    assert_eq!(r["shoved_tracks"], 1, "{r}");
    assert_eq!(r["unrouted"], 0, "{r}");
    assert_eq!(run(&mut s, &u(8), "pcb.drc", json!({}))["violations"], json!([]));
    // One undo takes the track and the shove back.
    run(&mut s, &u(9), "history.undo", json!({}));
    assert_eq!(board(&s).board.tracks.len(), 2);
    // B through a via instead (ADR 0058): the via is pushed with its tracks, the back
    // one stretched.
    run(&mut s, &u(10), "pcb.delete", json!({"tracks": [0]}));
    run(&mut s, &u(11), "pcb.route", json!({"points": [[2, 5], [15, 4.8], [28, 5]], "around": false}));
    run(&mut s, &u(12), "pcb.add_via", json!({"at": [15, 4.8], "net": "B"}));
    run(
        &mut s,
        &u(13),
        "pcb.route",
        json!({"points": [[15, 4.8], [15, 10]], "layer": "B.Cu", "net": "B", "around": false}),
    );
    let before = board(&s).board.clone();
    let r = run(&mut s, &u(14), "pcb.route", json!({"points": [[8, 6], [22, 6]]}));
    assert_eq!((r["shoved_vias"].as_u64(), r["unrouted"].as_u64()), (Some(1), Some(0)), "{r}");
    assert!(r["shoved_tracks"].as_u64().unwrap() >= 1, "{r}");
    let after = board(&s).board.clone();
    let v = after.vias[0].at;
    assert!(v[1] < 4.75, "{v:?}");
    assert!(after.tracks.iter().any(|t| t.layer == "B.Cu" && t.start == v), "the back track follows");
    assert_eq!(run(&mut s, &u(15), "pcb.drc", json!({}))["violations"], json!([]));
    run(&mut s, &u(16), "history.undo", json!({}));
    assert_eq!(board(&s).board.tree, before.tree, "one undo step");
}

#[test]
fn a_route_dives_under_a_wall_through_vias() {
    let mut s = setup();
    run(&mut s, &u(1), "pcb.new", json!({"width": "30 mm", "height": "10 mm"}));
    // B runs across the whole board at x = 15 on F.Cu; A's pads either side of it, and
    // A3 on B.Cu only.
    let fp = |r: &str, at: [f64; 2], layer: &str| {
        format!(
            "(footprint \"P\" (layer \"{layer}\") (at {} {}) (property \"Reference\" \"{r}\" (at 0 0) (layer \"F.Fab\")) (pad \"1\" smd circle (at 0 0) (size 0.6 0.6) (layers \"{layer}\")))",
            at[0], at[1]
        )
    };
    let parts = [
        fp("A1", [3.0, 5.0], "F.Cu"),
        fp("A2", [27.0, 5.0], "F.Cu"),
        fp("A3", [27.0, 8.0], "B.Cu"),
        fp("B1", [15.0, 1.0], "F.Cu"),
        fp("B2", [15.0, 9.0], "F.Cu"),
    ];
    let dir = s.project_dir.clone().unwrap();
    let text = board(&s).board.write().replacen("(net 0 \"\")", &format!("(net 0 \"\") {}", parts.join(" ")), 1);
    std::fs::write(dir.join("wall.kicad_pcb"), text).unwrap();
    run(&mut s, &u(2), "pcb.open", json!({"path": "wall.kicad_pcb"}));
    for (r, n) in [("A1", "A"), ("A2", "A"), ("A3", "A"), ("B1", "B"), ("B2", "B")] {
        run(&mut s, &u(3), "pcb.set_net", json!({"pads": [format!("{r}.1")], "net": n}));
    }
    run(&mut s, &u(4), "pcb.route", json!({"points": [[15, 1], [15, 9]], "around": false}));
    // On F.Cu alone there is no way.
    let e = execute(&mut s, &u(5), "pcb.route", json!({"points": [[3, 5], [27, 5]], "shove": false})).unwrap_err();
    assert_eq!(e.code, "not_found");
    let bad = execute(&mut s, &u(5), "pcb.route", json!({"points": [[3, 5], [27, 5]], "layers": "F.Cu,In9.Cu"}));
    assert_eq!(bad.unwrap_err().code, "invalid_args");
    let before = board(&s).board.clone();
    let r = run(&mut s, &u(6), "pcb.route", json!({"points": [[3, 5], [27, 5]], "layers": "any"}));
    assert_eq!((r["vias"].as_u64(), r["unrouted"].as_u64()), (Some(2), Some(1)), "{r}");
    assert_eq!(r["via_size"], json!([0.6, 0.3]), "{r}");
    let b = board(&s).board.clone();
    assert_eq!(b.vias.len(), 2);
    assert!(b.vias.iter().all(|v| b.net_name(&v.net).as_deref() == Some("A")));
    assert!(b.tracks.iter().any(|t| t.layer == "B.Cu"));
    assert_eq!(run(&mut s, &u(7), "pcb.drc", json!({}))["violations"], json!([]));
    // One undo takes the tracks and both vias back.
    run(&mut s, &u(8), "history.undo", json!({}));
    assert_eq!(board(&s).board.tree, before.tree, "one undo step");
    // An end on B.Cu only lets the route change layer without being asked: one via.
    let r = run(&mut s, &u(9), "pcb.route", json!({"points": [[3, 5], [27, 8]]}));
    assert_eq!(r["vias"], 1, "{r}");
    assert_eq!(run(&mut s, &u(10), "pcb.drc", json!({}))["violations"], json!([]));
    run(&mut s, &u(11), "history.undo", json!({}));
    assert_eq!(board(&s).board.tree, before.tree);
}

#[test]
fn a_track_necks_down_to_reach_a_tight_pad() {
    let mut s = setup();
    run(&mut s, &u(1), "pcb.new", json!({"width": "30 mm", "height": "10 mm"}));
    // A1 in a row of 0.3 × 1.6 mm pads 0.5 mm apart (B either side); A2 clear.
    let fp = |r: &str, at: [f64; 2], size: [f64; 2]| {
        format!(
            "(footprint \"P\" (layer \"F.Cu\") (at {} {}) (property \"Reference\" \"{r}\" (at 0 0) (layer \"F.Fab\")) (pad \"1\" smd rect (at 0 0) (size {} {}) (layers \"F.Cu\")))",
            at[0], at[1], size[0], size[1]
        )
    };
    let parts = [
        fp("A1", [10.0, 5.0], [0.3, 1.6]),
        fp("B1", [9.5, 5.0], [0.3, 1.6]),
        fp("B2", [10.5, 5.0], [0.3, 1.6]),
        fp("A2", [25.0, 5.0], [0.6, 0.6]),
    ];
    let dir = s.project_dir.clone().unwrap();
    let text = board(&s).board.write().replacen("(net 0 \"\")", &format!("(net 0 \"\") {}", parts.join(" ")), 1);
    std::fs::write(dir.join("row.kicad_pcb"), text).unwrap();
    run(&mut s, &u(2), "pcb.open", json!({"path": "row.kicad_pcb"}));
    for (r, n) in [("A1", "A"), ("A2", "A"), ("B1", "B"), ("B2", "B")] {
        run(&mut s, &u(3), "pcb.set_net", json!({"pads": [format!("{r}.1")], "net": n}));
    }
    let route = json!({"points": [[10, 5], [25, 5]], "width": 0.4, "shove": false});
    // At 0.4 mm the pad cannot be left, and with neck-down refused there is no way.
    let mut no_neck = route.clone();
    no_neck["neck"] = json!(false);
    let e = execute(&mut s, &u(4), "pcb.route", no_neck).unwrap_err();
    assert_eq!(e.code, "not_found");
    let before = board(&s).board.clone();
    let r = run(&mut s, &u(5), "pcb.route", route);
    assert_eq!(r["neck_width"], json!(0.15), "{r}");
    assert!(r["necked_mm"].as_f64().unwrap() > 0.0 && r["necked_mm"].as_f64().unwrap() < 2.0, "{r}");
    assert_eq!(r["unrouted"], 1, "only B left: {r}");
    let b = board(&s).board.clone();
    let mut widths: Vec<f64> = b.tracks.iter().map(|t| t.width).collect();
    widths.dedup();
    assert_eq!(widths, [0.15, 0.4], "narrow at the pad, then full width");
    assert_eq!(run(&mut s, &u(6), "pcb.drc", json!({}))["violations"], json!([]));
    // One undo takes the whole track back.
    run(&mut s, &u(7), "history.undo", json!({}));
    assert_eq!(board(&s).board.tree, before.tree, "one undo step");
}

/// A stand-in for Freerouting: a shell script in a temporary folder (nothing is
/// downloaded or run but this). `body` runs with `$out` set to the `-do` path.
#[cfg(unix)]
fn fake_router(name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("gusset-fake-fr-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let script = format!(
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -do) out=\"$2\"; shift;; esac; shift; done\n{body}\n"
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn freerouting_runs_only_when_set_up_and_can_be_stopped() {
    let mut s = setup();
    run(&mut s, &u(1), "pcb.new", json!({"width": "30 mm", "height": "20 mm"}));
    for (t, r, at) in [(2, "TP1", [5, 10]), (3, "TP2", [25, 10])] {
        run(
            &mut s,
            &u(t),
            "pcb.add_footprint",
            json!({"library": "fp", "footprint": "Pads:TestPoint", "at": at, "reference": r}),
        );
    }
    run(&mut s, &u(4), "pcb.set_net", json!({"pads": ["TP1.1", "TP2.1"], "net": "SIG"}));
    assert_eq!(board(&s).ratsnest.unrouted(), 1);
    // Not set up: refused, nothing run.
    let e = execute(&mut s, &u(5), "pcb.autoroute", json!({})).unwrap_err();
    assert_eq!(e.code, "not_available", "{e:?}");
    // A router that writes a session joining the two pads (um, Y up).
    let ses = "(session board (routes (resolution um 1) (network_out (net SIG (wire (path F.Cu 250 5000 -10000 25000 -10000))))))";
    let good = fake_router("router", &format!("test -f board.dsn || exit 9\necho '{ses}' > \"$out\""));
    s.freerouting =
        Some(pcb_session::Freerouting { path: good.clone(), java: None, timeout_s: 30, args: vec!["--x".into()] });
    // Agents need the user's approval to start it.
    let e = execute(&mut s, &CmdCtx::agent("a", 6), "pcb.autoroute", json!({})).unwrap_err();
    assert_eq!(e.code, "approval_required", "{e:?}");
    let r = run(&mut s, &u(7), "pcb.autoroute", json!({}));
    assert_eq!((r["tracks"].as_u64(), r["unrouted"].as_u64()), (Some(1), Some(0)), "{r}");
    run(&mut s, &u(8), "history.undo", json!({}));
    assert_eq!(board(&s).ratsnest.unrouted(), 1);
    // A router that fails: its error output comes back.
    let bad = fake_router("router", "echo 'no licence for this' >&2\nexit 3");
    s.freerouting.as_mut().unwrap().path = bad;
    let e = execute(&mut s, &u(9), "pcb.autoroute", json!({})).unwrap_err();
    assert_eq!(e.code, "io");
    assert!(e.details.unwrap()["stderr_user_text"].as_str().unwrap().contains("no licence"));
    // One that hangs: stopped at the timeout, and by the cancel flag.
    let slow = fake_router("router", "sleep 30");
    s.freerouting.as_mut().unwrap().path = slow;
    let t = std::time::Instant::now();
    let e = execute(&mut s, &u(10), "pcb.autoroute", json!({"timeout_s": 1})).unwrap_err();
    assert_eq!(e.code, "timeout");
    assert!(t.elapsed() < std::time::Duration::from_secs(10));
    let flag = std::sync::Arc::clone(&s.autoroute_cancel);
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let e = execute(&mut s, &u(11), "pcb.autoroute", json!({})).unwrap_err();
    stopper.join().unwrap();
    assert_eq!(e.code, "cancelled");
    assert!(t.elapsed() < std::time::Duration::from_secs(15));
    // A missing program.
    s.freerouting.as_mut().unwrap().path = "/nonexistent/freerouting".into();
    assert_eq!(execute(&mut s, &u(12), "pcb.autoroute", json!({})).unwrap_err().code, "io");
    assert_eq!(board(&s).ratsnest.unrouted(), 1);
}
