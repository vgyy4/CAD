//! PCB editing commands (ADR 0047 phase 1b): open, edit with undo, save, check, assembly
//! files and the open board in pcb.import.

use crate::tests_pcb::{run, session};
use crate::*;
use serde_json::json;

fn u(t: u64) -> CmdCtx {
    CmdCtx::user(t)
}

fn open(s: &mut Session) -> std::sync::Arc<pcb_session::OpenBoard> {
    pcb_session::open_board(s, &gusset_history::Actor::User).unwrap().unwrap()
}

#[test]
fn a_board_opens_edits_undoes_and_saves() {
    let mut s = session();
    // Nothing open yet.
    let e = execute(&mut s, &u(1), "pcb.add_via", json!({"at": [5, 5]})).unwrap_err();
    assert_eq!(e.code, "not_found");
    let r = run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    assert_eq!((r["footprints"].as_u64(), r["tracks"].as_u64()), (Some(3), Some(1)), "{r}");
    let doc = s.document(&gusset_history::Actor::User).unwrap();
    assert_eq!(doc.pcb.as_ref().unwrap().file, "board.kicad_pcb");
    assert!(doc.to_text().contains("pcb (file: \"board.kicad_pcb\", board: \""));
    // A track from R1's pad 1 takes its net (GND) and the default width.
    let r = run(&mut s, &u(2), "pcb.add_track", json!({"start": [9, 10], "end": [9, 15]}));
    assert_eq!((r["track"].as_u64(), r["width"].as_f64()), (Some(1), Some(0.25)), "{r}");
    assert_eq!(r["net_user_text"], "GND", "{r}");
    run(&mut s, &u(3), "pcb.add_via", json!({"at": [9, 15]}));
    run(&mut s, &u(4), "pcb.move_footprint", json!({"footprint": "U1", "at": [20, 12], "angle": 0, "flip": true}));
    let b = open(&mut s);
    assert_eq!((b.board.tracks.len(), b.board.vias.len()), (2, 1));
    assert_eq!(b.board.net_name(&b.board.vias[0].net).as_deref(), Some("GND"));
    let u1 = &b.board.footprints[1];
    assert!(u1.bottom && (u1.at[0] - 20.0).abs() < 1e-9 && (u1.at[1] - 12.0).abs() < 1e-9, "{u1:?}");
    // Undo goes back one edit at a time.
    run(&mut s, &u(5), "history.undo", json!({}));
    assert!(!open(&mut s).board.footprints[1].bottom);
    run(&mut s, &u(6), "history.undo", json!({}));
    assert!(open(&mut s).board.vias.is_empty());
    run(&mut s, &u(7), "history.redo", json!({}));
    assert_eq!(open(&mut s).board.vias.len(), 1);
    // Delete, outline and save; the saved file reads back with the edits.
    run(&mut s, &u(8), "pcb.delete", json!({"tracks": [1], "vias": [0]}));
    run(&mut s, &u(9), "pcb.set_outline", json!({"points": [[0, 0], [45, 0], [45, 25], [0, 25]]}));
    let e = execute(&mut s, &u(10), "pcb.save", json!({})).unwrap_err();
    assert_eq!(e.code, "confirmation_required");
    run(&mut s, &u(10), "pcb.save", json!({"path": "out.kicad_pcb"}));
    let dir = s.project_dir.clone().unwrap();
    let text = std::fs::read_to_string(dir.join("out.kicad_pcb")).unwrap();
    let back = gusset_pcb::Board::read(&text, &gusset_pcb::Limits::default()).unwrap();
    assert_eq!((back.tracks.len(), back.vias.len()), (1, 0));
    let (loops, _) = gusset_pcb::geom::outline_loops(&back, 1e-3);
    assert!((gusset_pcb::geom::area(&loops[0].polygon(0.01)).abs() - 1125.0).abs() < 1e-6);
    // Opened and saved without edits, the file is written back with the same tree.
    run(&mut s, &u(11), "pcb.open", json!({"path": "board.kicad_pcb"}));
    run(&mut s, &u(12), "pcb.save", json!({"path": "same.kicad_pcb"}));
    let same = std::fs::read_to_string(dir.join("same.kicad_pcb")).unwrap();
    let orig = gusset_pcb::sexpr::parse(crate::tests_pcb::BOARD, &Default::default()).unwrap();
    assert_eq!(gusset_pcb::sexpr::parse(&same, &Default::default()).unwrap(), orig);
}

#[test]
fn a_new_board_and_project_rules() {
    let mut s = session();
    let r = run(&mut s, &u(1), "pcb.new", json!({"width": "50 mm", "height": "30", "name": "ctl"}));
    assert_eq!(r["size_mm"], json!([50.0, 30.0]), "{r}");
    assert_eq!(open(&mut s).file, "ctl.kicad_pcb");
    assert!(execute(&mut s, &u(2), "pcb.new", json!({"width": "0 mm", "height": "3 mm"})).is_err());
    // A project file beside a board gives its net classes.
    let dir = s.project_dir.clone().unwrap();
    let pro = json!({
        "net_settings": {
            "classes": [{"name": "Default", "clearance": 0.15, "track_width": 0.2}, {"name": "Power", "clearance": 0.3, "track_width": 0.6}],
            "netclass_patterns": [{"netclass": "Power", "pattern": "GN*"}]
        },
        "board": {"design_settings": {"rules": {"min_track_width": 0.1, "min_hole_to_hole": 0.4, "min_through_hole_diameter": 0.35, "min_via_diameter": 0.5, "min_silk_clearance": 0.1}}}
    });
    std::fs::write(dir.join("board.kicad_pro"), pro.to_string()).unwrap();
    let r = run(&mut s, &u(3), "pcb.open", json!({"path": "board.kicad_pcb"}));
    assert_eq!(r["project_rules"], true, "{r}");
    let b = open(&mut s);
    assert_eq!((b.rules.clearance, b.rules.track_width, b.rules.min_track_width), (0.15, 0.2, 0.1));
    let r = run(&mut s, &u(4), "pcb.add_track", json!({"start": [9, 10], "end": [9, 15]}));
    assert_eq!(r["width"], 0.6, "{r}");
    let r = run(&mut s, &u(5), "pcb.drc", json!({}));
    assert_eq!(r["rules_mm"]["clearance"], 0.15, "{r}");
    assert_eq!(r["rules_mm"]["net_classes"], 2, "{r}");
    // ADR 0048 minimums from the project file.
    let m = &r["rules_mm"];
    assert_eq!(
        (&m["hole_to_hole"], &m["min_through_hole"], &m["min_via_drill"], &m["min_via_diameter"], &m["silk_clearance"]),
        (&json!(0.4), &json!(0.35), &json!(0.35), &json!(0.5), &json!(0.1)),
        "{r}"
    );
}

#[test]
fn drc_reports_violations_with_places() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let r = run(&mut s, &u(2), "pcb.drc", json!({}));
    let clean = r["violations"].as_array().unwrap().len();
    // A thin no-net track beside the GND track.
    run(&mut s, &u(3), "pcb.add_track", json!({"start": [15, 13], "end": [20, 13], "width": 0.1, "net": ""}));
    let r = run(&mut s, &u(4), "pcb.drc", json!({}));
    let v = r["violations"].as_array().unwrap();
    assert!(v.len() > clean, "{r}");
    assert!(v.iter().any(|x| x["code"] == "track_width" && x["at"] == json!([17.5, 13.0])), "{r}");
    // Rules given in the call win.
    let r = run(&mut s, &u(5), "pcb.drc", json!({"min_width": 0.05}));
    assert!(!r["violations"].as_array().unwrap().iter().any(|x| x["code"] == "track_width"), "{r}");
    assert!(execute(&mut s, &u(6), "pcb.drc", json!({"clearance": -1})).is_err());
}

#[test]
fn placement_and_bom_files_from_the_open_board() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let r = run(&mut s, &u(2), "pcb.export_pos", json!({"file": "fab/pos.csv"}));
    assert_eq!(r["components"], 3, "{r}");
    let dir = s.project_dir.clone().unwrap();
    let pos = std::fs::read_to_string(dir.join("fab/pos.csv")).unwrap();
    assert!(pos.starts_with("Ref,Val,Package,PosX,PosY,Rot,Side\n"), "{pos}");
    assert!(pos.contains("J1,\"\",P,35.0000,-15.0000,0.0000,bottom"), "{pos}");
    let r = run(&mut s, &u(3), "pcb.export_bom", json!({"file": "fab/bom.csv", "path": "board.kicad_pcb"}));
    assert_eq!(r["lines"], 3, "{r}");
    assert!(execute(&mut s, &u(4), "pcb.export_bom", json!({"file": "fab/bom.txt"})).is_err());
    // Agents write inside the project only.
    let agent = CmdCtx::agent("a", 5);
    let e = execute(&mut s, &agent, "pcb.export_pos", json!({"file": "../pos.csv"})).unwrap_err();
    assert_eq!(e.code, "not_allowed", "{e:?}");
}

#[test]
fn import_info_and_exports_use_the_open_board() {
    let mut s = session();
    let e = execute(&mut s, &u(1), "pcb.info", json!({})).unwrap_err();
    assert_eq!(e.code, "not_found");
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    run(&mut s, &u(2), "pcb.move_footprint", json!({"footprint": "R1", "at": [5, 5]}));
    let r = run(&mut s, &u(3), "pcb.info", json!({}));
    assert_eq!(r["footprints"], 3, "{r}");
    assert!(r["unrouted"].is_u64(), "{r}");
    let r = run(&mut s, &u(4), "pcb.import", json!({}));
    assert_eq!(r["features"].as_array().unwrap().len(), 4, "{r}");
    let r = run(&mut s, &u(5), "pcb.export_drill", json!({"folder": "fab"}));
    assert_eq!(r["plated"], 1, "{r}");
    assert!(s.project_dir.clone().unwrap().join("fab/board-PTH.drl").exists());
}

#[test]
fn board_texts_are_added_edited_deleted_and_undone_one_step_each() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let n0 = open(&mut s).board.texts.len();
    let r = run(&mut s, &u(2), "pcb.text", json!({"text": "REV A", "at": [12, 3], "size": 1.5, "layer": "B.SilkS"}));
    assert_eq!((r["text"].as_u64(), r["mirrored"].as_bool()), (Some(n0 as u64), Some(true)), "{r}");
    let t = open(&mut s).board.texts[n0].clone();
    assert_eq!((t.text.as_str(), t.at, t.size, t.layer.as_str()), ("REV A", [12.0, 3.0], 1.5, "B.SilkS"));
    // pcb.info lists it with its index.
    let info = run(&mut s, &u(3), "pcb.info", json!({}));
    let listed =
        info["board_texts"].as_array().unwrap().iter().any(|x| x["index"] == n0 && x["text_user_text"] == "REV A");
    assert!(listed, "{info}");
    // Editing is one step too.
    run(
        &mut s,
        &u(4),
        "pcb.text",
        json!({"index": n0, "text": "REV B", "at": [13, 4], "angle": 90, "layer": "F.SilkS"}),
    );
    let t = open(&mut s).board.texts[n0].clone();
    assert_eq!((t.text.as_str(), t.at, t.angle, t.mirrored), ("REV B", [13.0, 4.0], 90.0, false));
    run(&mut s, &u(5), "history.undo", json!({}));
    assert_eq!(open(&mut s).board.texts[n0].text, "REV A");
    run(&mut s, &u(6), "history.redo", json!({}));
    assert_eq!(open(&mut s).board.texts[n0].text, "REV B");
    run(&mut s, &u(7), "pcb.text", json!({"index": n0, "delete": true}));
    assert_eq!(open(&mut s).board.texts.len(), n0);
    run(&mut s, &u(8), "history.undo", json!({}));
    assert_eq!(open(&mut s).board.texts.len(), n0 + 1);
    // Mistakes are refused and leave the board as it was.
    for args in [
        json!({"text": "x"}),
        json!({"at": [1, 1]}),
        json!({"text": "x", "at": [1, 1], "layer": "F.Cu1"}),
        json!({"index": 999, "text": "x"}),
        json!({"index": n0}),
        json!({"delete": true}),
        json!({"text": "x", "at": [1, 1], "size": -1}),
    ] {
        assert!(execute(&mut s, &u(9), "pcb.text", args.clone()).is_err(), "{args}");
    }
    assert_eq!(open(&mut s).board.texts.len(), n0 + 1);
}

#[test]
#[allow(clippy::float_cmp)] // coordinates are written and read back exactly
fn tracks_and_vias_move_with_attached_ends_in_one_undo_step() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let (t0, v0) = {
        let b = open(&mut s);
        (b.board.tracks.len(), b.board.vias.len())
    };
    run(&mut s, &u(2), "pcb.add_track", json!({"start": [9, 10], "end": [9, 15]}));
    run(&mut s, &u(3), "pcb.add_via", json!({"at": [9, 15]}));
    run(&mut s, &u(4), "pcb.add_track", json!({"start": [9, 15], "end": [14, 15], "layer": "B.Cu"}));
    // The via drags both ends sitting on it.
    let r = run(&mut s, &u(5), "pcb.move", json!({"vias": [v0], "by": [1, 2]}));
    assert_eq!((r["ends_carried"].as_u64(), r["vias"].as_u64()), (Some(2), Some(1)), "{r}");
    let b = open(&mut s);
    assert_eq!(b.board.vias[v0].at, [10.0, 17.0]);
    assert_eq!(b.board.tracks[t0].end, [10.0, 17.0]);
    assert_eq!(b.board.tracks[t0].start, [9.0, 10.0]);
    assert_eq!(b.board.tracks[t0 + 1].start, [10.0, 17.0]);
    run(&mut s, &u(6), "history.undo", json!({}));
    let b = open(&mut s);
    assert_eq!((b.board.vias[v0].at, b.board.tracks[t0].end), ([9.0, 15.0], [9.0, 15.0]));
    // A track alone, without following; then undo and redo.
    run(&mut s, &u(7), "pcb.move", json!({"tracks": [t0 + 1], "by": [0, -1], "attached": false}));
    let b = open(&mut s);
    assert_eq!((b.board.tracks[t0 + 1].start, b.board.tracks[t0 + 1].end), ([9.0, 14.0], [14.0, 14.0]));
    run(&mut s, &u(8), "history.undo", json!({}));
    assert_eq!(open(&mut s).board.tracks[t0 + 1].start, [9.0, 15.0]);
    run(&mut s, &u(9), "history.redo", json!({}));
    assert_eq!(open(&mut s).board.tracks[t0 + 1].start, [9.0, 14.0]);
    // Mistakes are refused.
    for args in [json!({"by": [1, 1]}), json!({"tracks": [999], "by": [1, 1]}), json!({"vias": [999], "by": [1, 1]})] {
        assert!(execute(&mut s, &u(10), "pcb.move", args.clone()).is_err(), "{args}");
    }
}

#[test]
#[allow(clippy::float_cmp)]
fn moving_a_track_stretches_its_neighbours_in_one_undo_step() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let t0 = open(&mut s).board.tracks.len();
    run(&mut s, &u(2), "pcb.add_track", json!({"start": [9, 10], "end": [9, 15]}));
    run(&mut s, &u(3), "pcb.add_track", json!({"start": [9, 15], "end": [14, 15], "net": "GND"}));
    let r = run(&mut s, &u(4), "pcb.move", json!({"tracks": [t0], "by": [1, 0]}));
    assert_eq!(r["ends_carried"], 2, "{r}");
    let b = open(&mut s);
    assert_eq!((b.board.tracks[t0].start, b.board.tracks[t0].end), ([10.0, 10.0], [10.0, 15.0]));
    assert_eq!(b.board.tracks[t0 + 1].start, [10.0, 15.0]);
    run(&mut s, &u(5), "history.undo", json!({}));
    assert_eq!(open(&mut s).board.tracks[t0 + 1].start, [9.0, 15.0]);
}

#[test]
#[allow(clippy::float_cmp)]
fn a_footprint_move_carries_track_ends_on_its_pads_in_one_undo_step() {
    let mut s = session();
    run(&mut s, &u(1), "pcb.open", json!({"path": "board.kicad_pcb"}));
    let t = run(&mut s, &u(2), "pcb.add_track", json!({"start": [9, 10], "end": [9, 15]}))["track"].as_u64().unwrap();
    let t = usize::try_from(t).unwrap();
    let at = open(&mut s).board.footprints[0].at;
    assert_eq!(open(&mut s).board.footprints[0].reference, "R1");
    let to = [at[0] + 2.0, at[1] + 3.0];
    let r = run(&mut s, &u(3), "pcb.move_footprint", json!({"footprint": "R1", "at": to}));
    assert_eq!(r["ends_carried"], 2, "{r}");
    let b = open(&mut s);
    assert_eq!((b.board.tracks[t].start, b.board.tracks[t].end), ([11.0, 13.0], [9.0, 15.0]));
    run(&mut s, &u(4), "history.undo", json!({}));
    assert_eq!(open(&mut s).board.tracks[t].start, [9.0, 10.0]);
    // Switched off: the track stays.
    let r = run(&mut s, &u(5), "pcb.move_footprint", json!({"footprint": "R1", "at": to, "carry": false}));
    assert_eq!(r["ends_carried"], 0, "{r}");
    assert_eq!(open(&mut s).board.tracks[t].start, [9.0, 10.0]);
}
