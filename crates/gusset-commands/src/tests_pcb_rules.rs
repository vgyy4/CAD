//! ADR 0054: per-class limits and the board's `.kicad_dru` custom rules through the commands.

use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::json;

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

#[test]
fn class_limits_and_custom_rules_reach_the_drc_report() {
    let mut s = session();
    let dir = s.project_dir.clone().unwrap();
    let pro = json!({
        "net_settings": {
            "classes": [
                {"name": "Default"},
                {"name": "Power", "track_width": 0.6, "via_diameter": 1.0, "via_drill": 0.5,
                 "min_track_width": 0.5, "min_via_diameter": 0.9, "diff_pair_width": 0.2, "diff_pair_gap": 0.15}
            ],
            "netclass_patterns": [{"netclass": "Power", "pattern": "GN*"}]
        }
    });
    std::fs::write(dir.join("board.kicad_pro"), pro.to_string()).unwrap();
    let dru = "(version 1)\n(rule \"gnd wide\" (condition \"A.NetName == 'GND' && A.Type == 'track'\") (constraint track_width (min 2mm)))\n\
               (rule \"areas\" (condition \"A.insideArea('x')\") (constraint clearance (min 1)))\n";
    std::fs::write(dir.join("board.kicad_dru"), dru).unwrap();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let open = pcb_session::open_board(&s, &gusset_history::Actor::User).unwrap().unwrap();
    let c = open.rules.classes.iter().find(|c| c.name == "Power").unwrap();
    assert_eq!((c.min_track_width, c.diff_pair_width, c.diff_pair_gap), (Some(0.5), Some(0.2), Some(0.15)));
    // New items take the class's sizes.
    let r = run(&mut s, &u(2), "pcb.add_track", json!({"start": [9, 10], "end": [9, 15]}));
    assert_eq!(r["width"], 0.6, "{r}");
    let r = run(&mut s, &u(3), "pcb.add_via", json!({"at": [9, 15]}));
    assert_eq!((&r["size"], &r["drill"]), (&json!(1.0), &json!(0.5)), "{r}");
    // The report names the custom rule and lists what was skipped.
    let r = run(&mut s, &u(4), "pcb.drc", json!({}));
    let v = r["violations"].as_array().unwrap();
    assert!(v.iter().any(|x| x["code"] == "track_width" && x["rule_user_text"] == "gnd wide"), "{r}");
    assert_eq!(r["rules_mm"]["custom_rules"], 1, "{r}");
    let w = r["rule_warnings_user_text"].as_array().unwrap();
    assert_eq!(w.len(), 1, "{r}");
    assert!(w[0].as_str().unwrap().contains("insideArea"), "{r}");
    // Saving writes the rules back beside the board, and keeps them out of the project file.
    run(&mut s, &u(5), "pcb.save", json!({"path": "out.kicad_pcb"}));
    assert_eq!(std::fs::read_to_string(dir.join("out.kicad_dru")).unwrap(), dru);
    let pro_out = std::fs::read_to_string(dir.join("out.kicad_pro")).unwrap();
    assert!(!pro_out.contains("gusset_custom_rules"), "{pro_out}");
    // The net class command stores the limits in the project; the rules survive it.
    run(&mut s, &u(6), "pcb.net_class", json!({"name": "Power", "min_via_drill": 0.45, "diff_pair_gap": 0.2}));
    let open = pcb_session::open_board(&s, &gusset_history::Actor::User).unwrap().unwrap();
    let c = open.rules.classes.iter().find(|c| c.name == "Power").unwrap();
    assert_eq!((c.min_via_drill, c.diff_pair_gap), (Some(0.45), Some(0.2)));
    assert_eq!(open.rules.custom.rules.len(), 1, "rules survive project edits");
}

#[test]
fn a_hostile_rules_file_is_only_warnings() {
    let mut s = session();
    let dir = s.project_dir.clone().unwrap();
    std::fs::write(dir.join("board.kicad_dru"), format!("(version 1) {}", "(".repeat(50_000))).unwrap();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let r = run(&mut s, &u(2), "pcb.drc", json!({}));
    assert_eq!(r["rules_mm"]["custom_rules"], 0, "{r}");
    assert_eq!(r["rule_warnings_user_text"].as_array().unwrap().len(), 1, "{r}");
}
