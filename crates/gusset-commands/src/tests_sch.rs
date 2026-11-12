//! Schematic commands (ADR 0047 phase 2a): open, new, edit with undo, save, ERC,
//! netlist, and the board updated from the schematic. Hand-written files only.

use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::json;

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

/// A two-pin part and a ground symbol.
const LIB: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "10k" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (property "Footprint" "Pads:Two" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
  (symbol "GND" (power) (in_bom yes) (on_board yes)
    (property "Reference" "#PWR" (at 0 -6.35 0) (effects (font (size 1.27 1.27)) (hide yes)))
    (property "Value" "GND" (at 0 -3.81 0) (effects (font (size 1.27 1.27))))
    (symbol "GND_1_1" (pin power_in line (at 0 0 270) (length 0) (hide yes) (name "GND" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27))))))))
"##;

const MOD: &str = r#"(footprint "Two" (version 20241229) (generator "hand") (layer "F.Cu")
  (property "Reference" "REF**" (at 0 -2 0) (layer "F.SilkS"))
  (property "Value" "Two" (at 0 2 0) (layer "F.Fab"))
  (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask"))
  (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask")))"#;

fn open(s: &Session) -> std::sync::Arc<sch_session::OpenSchematic> {
    sch_session::open_schematic(s, &gusset_history::Actor::User).unwrap().unwrap()
}

fn setup() -> Session {
    let s = session();
    let dir = s.project_dir.clone().unwrap();
    std::fs::write(dir.join("hand.kicad_sym"), LIB).unwrap();
    std::fs::create_dir_all(dir.join("fp/Pads.pretty")).unwrap();
    std::fs::write(dir.join("fp/Pads.pretty/Two.kicad_mod"), MOD).unwrap();
    s
}

/// R1 and R2 joined at pin 1, R1 pin 2 to ground, R2 pin 2 labelled OUT.
fn draw(s: &mut Session) {
    run(s, &u(1), "sch.new", json!({"name": "demo"}));
    let r = run(s, &u(2), "sch.add_symbol", json!({"library": "hand.kicad_sym", "symbol": "R", "at": [100, 50]}));
    assert_eq!((r["reference_user_text"].as_str(), r["pins"].as_u64()), (Some("R1"), Some(2)), "{r}");
    let r = run(s, &u(3), "sch.add_symbol", json!({"library": "hand.kicad_sym", "symbol": "R", "at": [110, 50]}));
    assert_eq!(r["reference_user_text"], "R2", "{r}");
    run(s, &u(4), "sch.add_symbol", json!({"library": "hand.kicad_sym", "symbol": "GND", "at": [100, 60]}));
    run(s, &u(5), "sch.add_wire", json!({"points": [[100, 46.19], [110, 46.19]]}));
    run(s, &u(6), "sch.add_wire", json!({"points": [[100, 53.81], [100, 60]]}));
    run(s, &u(7), "sch.add_label", json!({"text": "OUT", "at": [110, 53.81]}));
}

#[test]
fn a_schematic_is_drawn_checked_undone_and_saved() {
    let mut s = setup();
    // Nothing open yet.
    let e = execute(&mut s, &u(1), "sch.erc", json!({})).unwrap_err();
    assert_eq!(e.code, "not_found");
    draw(&mut s);
    let doc = s.document(&gusset_history::Actor::User).unwrap();
    assert!(doc.to_text().contains("schematic (file: \"demo.kicad_sch\", sheet: \""), "{}", doc.to_text());
    let o = open(&s);
    assert_eq!(o.design.sheets[0].1.symbols.len(), 3);
    assert_eq!(o.design.sheets[0].1.symbols[2].reference(), "#PWR1");
    let r = run(&mut s, &u(8), "sch.netlist", json!({}));
    let nets = r["nets"].as_array().unwrap();
    let net = |name: &str| nets.iter().find(|n| n["name_user_text"] == name).map(|n| n["pins_user_text"].clone());
    assert_eq!(net("Net-(R1-Pad1)"), Some(json!(["R1.1", "R2.1"])), "{r}");
    assert_eq!(net("GND"), Some(json!(["R1.2"])));
    assert_eq!(net("/OUT"), Some(json!(["R2.2"])));
    assert_eq!(r["parts"].as_array().unwrap().len(), 2);
    // ERC: ground has no power flag.
    let r = run(&mut s, &u(9), "sch.erc", json!({}));
    let issues = r["issues"].as_array().unwrap();
    assert!(issues.iter().all(|i| i["code"] != "pin_not_connected"), "{r}");
    // Turn R2, mirror it, then undo both.
    run(&mut s, &u(10), "sch.move", json!({"item": "R2", "angle": 90}));
    run(&mut s, &u(11), "sch.move", json!({"item": "R2", "mirror": "y", "at": [120, 50]}));
    let r2 = |s: &Session| open(s).design.sheets[0].1.symbols[1].clone();
    assert_eq!((r2(&s).at, r2(&s).angle), ([120.0, 50.0], 90.0));
    assert!(execute(&mut s, &u(12), "sch.move", json!({"item": "R9", "angle": 90})).is_err());
    assert!(execute(&mut s, &u(12), "sch.move", json!({"item": "R2", "angle": 45})).is_err());
    run(&mut s, &u(13), "history.undo", json!({}));
    run(&mut s, &u(14), "history.undo", json!({}));
    assert_eq!((r2(&s).at, r2(&s).angle), ([110.0, 50.0], 0.0));
    // Delete the label and a wire: R1 pin 1 and R2 pin 1 come apart.
    run(&mut s, &u(15), "sch.delete", json!({"items": ["label#0", "wire#0"]}));
    let o = open(&s);
    assert!(o.erc.iter().any(|i| i.code == "pin_not_connected"));
    run(&mut s, &u(16), "history.undo", json!({}));
    assert_eq!(open(&s).design.sheets[0].1.labels.len(), 1);
    run(&mut s, &u(17), "history.redo", json!({}));
    assert!(open(&s).design.sheets[0].1.labels.is_empty());
    run(&mut s, &u(17), "history.undo", json!({}));
    // Save, and read the saved file back unchanged.
    run(&mut s, &u(18), "sch.save", json!({"path": "fp/demo.kicad_sch"}));
    let e = execute(&mut s, &u(19), "sch.save", json!({"path": "fp/demo.kicad_sch"})).unwrap_err();
    assert_eq!(e.code, "confirmation_required", "{e:?}");
    let dir = s.project_dir.clone().unwrap();
    let text = std::fs::read_to_string(dir.join("fp/demo.kicad_sch")).unwrap();
    let back = gusset_pcb::sch::Schematic::read(&text, &Default::default()).unwrap();
    assert_eq!(back.tree, open(&s).design.sheets[0].1.tree);
    // Bad arguments.
    for (id, args) in [
        ("sch.add_symbol", json!({"library": "hand.kicad_sym", "symbol": "Nope", "at": [0, 0]})),
        ("sch.add_symbol", json!({"library": "board.kicad_pcb", "symbol": "R", "at": [0, 0]})),
        ("sch.add_label", json!({"text": "A", "at": [0, 0], "kind": "odd"})),
        ("sch.add_wire", json!({"points": [[0, 0]]})),
        ("sch.delete", json!({"items": ["wire#99"]})),
        ("sch.add_wire", json!({"points": [[0, 0], [1, 0]], "sheet": "other.kicad_sch"})),
    ] {
        assert!(execute(&mut s, &u(20), id, args.clone()).is_err(), "{id} {args}");
    }
}

#[test]
fn a_hierarchical_schematic_opens_with_its_sheets() {
    let mut s = setup();
    let dir = s.project_dir.clone().unwrap();
    let root = r#"(kicad_sch (version 20250114) (generator "hand") (uuid "00000000-0000-4000-8000-000000000001") (paper "A4")
      (lib_symbols)
      (sheet (at 40 10) (size 20 10) (uuid "00000000-0000-4000-8000-0000000000aa")
        (property "Sheetname" "left" (at 40 10 0)) (property "Sheetfile" "sub.kicad_sch" (at 40 20 0)))
      (sheet (at 40 40) (size 20 10) (uuid "00000000-0000-4000-8000-0000000000bb")
        (property "Sheetname" "bad" (at 40 40 0)) (property "Sheetfile" "../outside.kicad_sch" (at 40 50 0))))"#;
    let sub = r#"(kicad_sch (version 20250114) (generator "hand") (uuid "00000000-0000-4000-8000-000000000002") (paper "A4")
      (lib_symbols) (label "X" (at 1 1 0)))"#;
    std::fs::write(dir.join("top.kicad_sch"), root).unwrap();
    std::fs::write(dir.join("sub.kicad_sch"), sub).unwrap();
    let r = run(&mut s, &u(1), "sch.open", json!({"path": "top.kicad_sch"}));
    assert_eq!(r["sheets"], 2, "{r}");
    assert_eq!(r["missing_sheets_user_text"], json!(["../outside.kicad_sch"]), "{r}");
    let doc = s.document(&gusset_history::Actor::User).unwrap();
    assert_eq!(doc.schematic.as_ref().unwrap().sheets[0].0, "sub.kicad_sch");
    // Edits on a sub-sheet store that sheet.
    run(&mut s, &u(2), "sch.add_label", json!({"text": "Y", "at": [5, 5], "sheet": "sub.kicad_sch"}));
    let o = open(&s);
    assert_eq!(o.sheet(Some("sub.kicad_sch")).unwrap().1.labels.len(), 2);
    assert_eq!(o.netlist.uses.len(), 2);
    // Saving writes both files.
    let r = run(&mut s, &u(3), "sch.save", json!({"path": "fp/top.kicad_sch"}));
    assert_eq!(r["written"], json!(["fp/top.kicad_sch", "fp/sub.kicad_sch"]), "{r}");
    assert!(execute(&mut s, &u(4), "sch.open", json!({"path": "board.kicad_pcb"})).is_err());
    // Agents read only inside the project.
    let agent = CmdCtx::agent("a", 5);
    assert!(execute(&mut s, &agent, "sch.open", json!({"path": "/etc/x.kicad_sch"})).is_err());
}

#[test]
fn the_board_is_updated_from_the_schematic_with_undo() {
    let mut s = setup();
    draw(&mut s);
    run(&mut s, &u(20), "pcb.new", json!({"width": "30 mm", "height": "20 mm"}));
    // Without a footprint folder nothing can be added.
    let r = run(&mut s, &u(21), "pcb.update_from_schematic", json!({}));
    assert_eq!(r["not_found_user_text"], json!([["R1", "Pads:Two"], ["R2", "Pads:Two"]]), "{r}");
    assert_eq!(r["changed"], false);
    // A dry run reports without changing the board.
    let r = run(&mut s, &u(22), "pcb.update_from_schematic", json!({"footprints": "fp", "dry_run": true}));
    assert_eq!(r["added_user_text"], json!(["R1", "R2"]), "{r}");
    let board = |s: &Session| pcb_session::open_board(s, &gusset_history::Actor::User).unwrap().unwrap();
    assert!(board(&s).board.footprints.is_empty());
    let r = run(&mut s, &u(23), "pcb.update_from_schematic", json!({"footprints": "fp"}));
    assert_eq!((r["pads_changed"].as_u64(), r["changed"].as_bool()), (Some(4), Some(true)), "{r}");
    let b = board(&s);
    let nets: Vec<String> = b.board.pads.iter().map(|p| b.board.net_name(&p.net).unwrap_or_default()).collect();
    assert_eq!(nets, ["Net-(R1-Pad1)", "GND", "Net-(R1-Pad1)", "/OUT"]);
    // The ratsnest follows: R1 pin 1 to R2 pin 1 is unrouted.
    assert_eq!(b.ratsnest.unrouted(), 1);
    run(&mut s, &u(24), "history.undo", json!({}));
    assert!(board(&s).board.footprints.is_empty());
    // A footprint name with path parts is never looked up.
    assert!(crate::commands::sch_footprint_candidates("fp", "../x:y").len() == 1);
    assert!(crate::commands::sch_footprint_candidates("fp", "a:../../y").is_empty());
}
