//! The Schematic tab (ADR 0047 phase 2a): editor logic, edits going through the registry,
//! and cross-probing with the board tab.
#![allow(clippy::float_cmp)] // snapped values are rounded to whole micrometres

use crate::sch_view::{self, State, Tool};
use crate::tests::{app, harness};
use gusset_pcb::Limits;
use gusset_pcb::sch::{LabelKind, Mirror, Schematic, SymbolLib};
use gusset_pcb::sch_edit::Item;
use serde_json::json;

const LIB: &str = r##"(kicad_symbol_lib (version 20241209) (generator "hand")
  (symbol "R" (pin_names (offset 0)) (in_bom yes) (on_board yes)
    (property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
    (property "Value" "10k" (at 0 0 90) (effects (font (size 1.27 1.27))))
    (symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type background))))
    (symbol "R_1_1"
      (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
      (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27))))))))
"##;

/// R1 at (100, 50) with a wire from pin 1 and a label on pin 2.
fn sheet() -> Schematic {
    let lim = Limits::default();
    let lib = SymbolLib::read(LIB, &lim).unwrap();
    let node = lib.embedded_node("R", "hand:R").unwrap();
    let mut s = Schematic::new_empty("t").unwrap();
    s.add_symbol(&node, "hand:R", [100.0, 50.0], 0.0, Mirror::None, 1, "R1", "t", &lim).unwrap();
    s.add_wires(&[[100.0, 46.19], [100.0, 40.0], [110.0, 40.0]], &lim).unwrap();
    s.add_label(LabelKind::Local, "OUT", [100.0, 53.81], 0.0, None, &lim).unwrap();
    s
}

#[test]
fn picking_snapping_and_drops() {
    let s = sheet();
    assert_eq!(sch_view::hit(&s, [100.5, 50.0], 0.2), Some(Item::Symbol(0)));
    assert_eq!(sch_view::hit(&s, [105.0, 40.1], 0.2), Some(Item::Wire(1)));
    assert_eq!(sch_view::hit(&s, [100.2, 53.9], 0.2), Some(Item::Label(0)));
    assert_eq!(sch_view::hit(&s, [150.0, 150.0], 0.2), None);
    let b = sch_view::symbol_box(&s, &s.symbols[0]);
    assert_eq!((b[0][1], b[1][1]), (46.19, 53.81));
    // Wire corners snap to pin ends near the pointer, else to the grid.
    assert_eq!(sch_view::wire_snap(&s, [100.3, 46.0], 0.5, 1.27), [100.0, 46.19]);
    assert_eq!(sch_view::wire_snap(&s, [120.3, 46.0], 0.5, 1.27), [120.65, 45.72]);
    assert_eq!(sch_view::drop_place(&s, Item::Symbol(0), [2.6, -1.2], 1.27), Some([102.87, 48.26]));
    assert_eq!(sch_view::drop_place(&s, Item::Wire(0), [1.0, 1.0], 1.27), None);
}

#[test]
fn editor_actions_are_registry_commands() {
    let s = sheet();
    let mut st = State::default();
    st.tool = Tool::Wire;
    st.library = "lib/hand.kicad_sym".into();
    st.symbol = "R".into();
    let cmds = [
        sch_view::move_command(&st, Item::Symbol(0), [1.0, 2.0]),
        sch_view::rotate_command(&st, &s, Item::Symbol(0)).unwrap(),
        sch_view::mirror_command(&st, &s, Item::Symbol(0), Mirror::Y).unwrap(),
        sch_view::delete_command(&st, Item::Wire(0)),
        sch_view::wire_command(&st, &[[0.0, 0.0], [1.27, 0.0]]).unwrap(),
        sch_view::label_command(&st, [1.0, 1.0]).unwrap(),
        sch_view::place_command(&st, [5.0, 5.0]).unwrap(),
    ];
    for (id, _) in &cmds {
        assert!(gusset_commands::find(id).is_some(), "{id}");
    }
    assert_eq!(cmds[1].1["angle"], 90.0);
    assert_eq!(cmds[2].1["mirror"], "y");
    assert_eq!(cmds[3].1["items"], json!(["wire#0"]));
    assert!(sch_view::wire_command(&st, &[[0.0, 0.0]]).is_none());
    assert!(sch_view::rotate_command(&st, &s, Item::Wire(0)).is_none());
    // On a sub-sheet, commands name it.
    st.sheet = Some("sub.kicad_sch".into());
    assert_eq!(sch_view::delete_command(&st, Item::Label(0)).1["sheet"], "sub.kicad_sch");
    st.label_text = "  ".into();
    assert!(sch_view::label_command(&st, [0.0, 0.0]).is_none());
    // Every Schematic ribbon tool exists in the registry.
    let ids = crate::ribbon::sch_command_ids();
    assert!(ids.len() >= 11 && ids.contains(&"pcb.update_from_schematic"), "{ids:?}");
}

#[test]
fn the_schematic_tab_edits_with_undo_and_cross_probes() {
    let dir = std::env::temp_dir().join(format!("gusset-sch-tab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lib = dir.join("hand.kicad_sym");
    std::fs::write(&lib, LIB).unwrap();
    let lib = lib.to_string_lossy().into_owned();
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::SchTab);
    h.run();
    assert!(h.state().sch_view_shown() && !h.state().pcb_view_shown());
    h.state_mut().run("sch.new", json!({})).unwrap();
    h.state_mut().wait_idle();
    h.run();
    assert!(h.state().snapshot.sch.is_some());
    // The editor's place, wire and label tools send registry commands.
    let st = &mut h.state_mut().sch_state;
    st.library = lib;
    st.symbol = "R".into();
    let out = crate::pcb_view::Out {
        commands: vec![
            sch_view::place_command(st, [100.0, 50.0]).unwrap(),
            sch_view::wire_command(st, &[[100.0, 46.19], [100.0, 40.0]]).unwrap(),
            sch_view::label_command(st, [100.0, 40.0]).unwrap(),
        ],
    };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    let o = h.state().snapshot.sch.clone().unwrap();
    let s = &o.design.sheets[0].1;
    assert_eq!((s.symbols.len(), s.wires.len(), s.labels.len()), (1, 1, 1));
    // R1 pin 2 is open: the ERC marker is on this sheet.
    let m = sch_view::markers(&o, &h.state().sch_state);
    assert!(m.iter().any(|x| x.text.contains("pin 2 of R1")), "{m:?}");
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert!(h.state().snapshot.sch.as_ref().unwrap().design.sheets[0].1.labels.is_empty());
    h.run();
    // Cross-probing: a board with R1; picking the symbol selects the footprint.
    h.state_mut().run("pcb.new", json!({"width": "30 mm", "height": "20 mm"})).unwrap();
    h.state_mut().wait_idle();
    let b = h.state().snapshot.pcb.clone().unwrap();
    let mut board = b.board.clone();
    let fp = r#"(footprint "R" (layer "F.Cu") (at 10 10) (property "Reference" "R1")
        (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu")) (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu")))"#;
    board.tree.items_mut().unwrap().push(gusset_pcb::sexpr::parse(fp, &Default::default()).unwrap());
    let path = dir.join("b.kicad_pcb");
    std::fs::write(&path, board.write()).unwrap();
    h.state_mut().run("pcb.open", json!({"path": path.to_string_lossy()})).unwrap();
    h.state_mut().wait_idle();
    h.state_mut().sch_state.probe = Some("R1".into());
    h.state_mut().cross_probe();
    assert_eq!(h.state().pcb_state.selected, Some(crate::pcb_view::Sel::Footprint(0)));
    // And back: the board tab's pick selects the symbol.
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    assert!(!h.state().sch_view_shown());
    h.state_mut().sch_state.selected = None;
    h.state_mut().sch_state.probe = None;
    h.state_mut().cross_probe();
    // Already probed R1: nothing new to do until another part is picked.
    assert_eq!(h.state().sch_state.selected, None);
    h.run();
    let _ = std::fs::remove_dir_all(&dir);
}
