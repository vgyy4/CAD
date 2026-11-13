//! Hierarchical sheets in the Schematic tab (ADR 0057): creating a sheet with the tool,
//! pins, entering and leaving a sheet with breadcrumbs, picking and moving a sheet, all
//! through registry commands.
#![allow(clippy::float_cmp)]

use crate::sch_view::{self, State, Tool};
use crate::tests::{app, harness};
use gusset_pcb::sch_edit::Item;
use serde_json::json;

fn commands(h: &mut egui_kittest::Harness<'static, crate::app::App>, cmds: Vec<(&'static str, serde_json::Value)>) {
    h.state_mut().pcb_commands(crate::pcb_view::Out { commands: cmds, blocked: None });
    h.state_mut().wait_idle();
}

#[test]
fn the_sheet_tool_makes_sheets_with_pins_and_the_view_enters_and_leaves_them() {
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::SchTab);
    h.state_mut().run("sch.new", json!({})).unwrap();
    h.state_mut().wait_idle();
    h.run();
    let open = h.state().snapshot.sch.clone().unwrap();
    // A dragged rectangle (snapped by the editor) becomes `sch.add_sheet`; the file name
    // comes from the sheet name, made unique.
    let mut st = State::default();
    st.tool = Tool::Sheet;
    st.sheet_name = "Power Supply".into();
    let (id, args) = sch_view::sheet_command(&st, &open, [150.0, 90.0], [120.0, 70.0]).unwrap();
    assert_eq!(id, "sch.add_sheet");
    assert_eq!(args["at"], json!([120.0, 70.0]));
    assert_eq!(args["size"], json!([30.0, 20.0]));
    assert_eq!(args["file"], "power_supply.kicad_sch");
    assert_eq!(args["name"], "Power Supply");
    assert!(args.get("sheet").is_none());
    // Too small, or no name: nothing is sent. A named file wins.
    assert!(sch_view::sheet_command(&st, &open, [0.0, 0.0], [2.0, 2.0]).is_none());
    st.sheet_name = "  ".into();
    assert!(sch_view::sheet_command(&st, &open, [0.0, 0.0], [20.0, 20.0]).is_none());
    st.sheet_name = "Amp".into();
    st.sheet_file = "my_amp.kicad_sch".into();
    assert_eq!(sch_view::sheet_command(&st, &open, [0.0, 0.0], [20.0, 20.0]).unwrap().1["file"], "my_amp.kicad_sch");

    // Through the app: two sheets, the second's default file avoids the first's.
    h.state_mut().sch_state.tool = Tool::Sheet;
    h.state_mut().sch_state.sheet_name = "Power Supply".into();
    let c1 = sch_view::sheet_command(&h.state().sch_state, &open, [120.0, 70.0], [150.0, 90.0]).unwrap();
    commands(&mut h, vec![c1]);
    let open = h.state().snapshot.sch.clone().unwrap();
    assert_eq!(open.design.sheets.len(), 2);
    let c2 = sch_view::sheet_command(&h.state().sch_state, &open, [160.0, 70.0], [190.0, 90.0]).unwrap();
    assert_eq!(c2.1["file"], "power_supply_2.kicad_sch");
    commands(&mut h, vec![c2]);
    let open = h.state().snapshot.sch.clone().unwrap();
    assert_eq!(open.design.sheets.len(), 3);
    assert_eq!(open.design.sheets[0].1.sheets.len(), 2);

    // Pins: the sheet panel's commands.
    let st = &mut h.state_mut().sch_state;
    st.pin_kind = 0;
    st.pin_side = 1;
    let add = sch_view::sheet_pin_command(st, 0, "add", "VIN", None);
    assert_eq!(add.1["kind"], "input");
    assert_eq!(add.1["side"], "right");
    commands(&mut h, vec![add]);
    let open = h.state().snapshot.sch.clone().unwrap();
    let pin = &open.design.sheets[0].1.sheets[0].pins[0];
    assert_eq!((pin.name.as_str(), pin.at, pin.kind.as_str()), ("VIN", [150.0, 72.54], "input"));
    let rename = sch_view::sheet_pin_command(&h.state().sch_state, 0, "rename", "VIN", Some("V_IN"));
    commands(&mut h, vec![rename]);
    let open = h.state().snapshot.sch.clone().unwrap();
    assert_eq!(open.design.sheets[0].1.sheets[0].pins[0].name, "V_IN");
    // The ERC shows the pin has no label inside; undo restores the name.
    assert!(open.erc.iter().any(|i| i.code == "hier_label_mismatch"));
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    let open = h.state().snapshot.sch.clone().unwrap();
    assert_eq!(open.design.sheets[0].1.sheets[0].pins[0].name, "VIN");
    let rm = sch_view::sheet_pin_command(&h.state().sch_state, 0, "remove", "VIN", None);
    commands(&mut h, vec![rm]);
    assert!(h.state().snapshot.sch.as_ref().unwrap().design.sheets[0].1.sheets[0].pins.is_empty());

    // Picking and moving a sheet: hit, box, one `sch.move` that undoes in one step.
    let open = h.state().snapshot.sch.clone().unwrap();
    let root = &open.design.sheets[0].1;
    assert_eq!(sch_view::hit(root, [130.0, 80.0], 0.2), Some(Item::Sheet(0)));
    assert_eq!(sch_view::hit(root, [175.0, 80.0], 0.2), Some(Item::Sheet(1)));
    assert_eq!(sch_view::hit(root, [100.0, 20.0], 0.2), None);
    assert_eq!(sch_view::box_select(root, [100.0, 60.0], [155.0, 100.0]), [Item::Sheet(0)]);
    assert_eq!(sch_view::box_select(root, [155.0, 100.0], [100.0, 60.0]), [Item::Sheet(0)]);
    assert_eq!(sch_view::box_select(root, [100.0, 60.0], [200.0, 100.0]), [Item::Sheet(0), Item::Sheet(1)]);
    let st = &h.state().sch_state;
    let d = sch_view::drag_delta(root, Item::Sheet(0), [2.6, 1.0], 1.27);
    let (id, args) = sch_view::group_move_command(st, Item::Sheet(0), &[Item::Sheet(0)], d).unwrap();
    assert_eq!((id, args["item"].clone()), ("sch.move", json!("sheet#0")));
    commands(&mut h, vec![(id, args)]);
    let open = h.state().snapshot.sch.clone().unwrap();
    assert_eq!(open.design.sheets[0].1.sheets[0].at, [122.54, 71.27]);
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert_eq!(h.state().snapshot.sch.as_ref().unwrap().design.sheets[0].1.sheets[0].at, [120.0, 70.0]);

    // Entering a sheet shows its file; the breadcrumbs name the way; leaving goes back.
    let open = h.state().snapshot.sch.clone().unwrap();
    let mut view = State::default();
    assert_eq!(sch_view::breadcrumbs(&open, &view), [(open.file().to_string(), None)]);
    assert!(!sch_view::leave_sheet(&open, &mut view), "the root has no parent");
    assert!(sch_view::enter_sheet(&open, &mut view, 1));
    assert_eq!(view.sheet.as_deref(), Some("power_supply_2.kicad_sch"));
    let crumbs = sch_view::breadcrumbs(&open, &view);
    assert_eq!(
        crumbs,
        [(open.file().to_string(), None), ("Power Supply".to_string(), Some("power_supply_2.kicad_sch".to_string()))]
    );
    // Edits there name the sheet; a nested sheet deepens the trail.
    let c = sch_view::sheet_command(&view, &open, [10.0, 10.0], [40.0, 30.0]).unwrap();
    assert_eq!(c.1["sheet"], "power_supply_2.kicad_sch");
    commands(&mut h, vec![c]);
    let open = h.state().snapshot.sch.clone().unwrap();
    assert!(sch_view::enter_sheet(&open, &mut view, 0));
    assert_eq!(sch_view::breadcrumbs(&open, &view).len(), 3);
    assert!(sch_view::leave_sheet(&open, &mut view));
    assert_eq!(view.sheet.as_deref(), Some("power_supply_2.kicad_sch"));
    assert!(sch_view::leave_sheet(&open, &mut view));
    assert_eq!(view.sheet, None);
    // Without a trail (a sheet chosen from the list) leaving goes to the parent by the hierarchy.
    sch_view::show_sheet(&mut view, Some("power_supply_2.kicad_sch".into()));
    assert!(sch_view::leave_sheet(&open, &mut view));
    assert_eq!(view.sheet, None);
    // A sheet whose file is missing from the design cannot be entered.
    assert!(!sch_view::enter_sheet(&open, &mut view, 9));

    // The frame draws sheets, in both the Select and Sheet tools, entered or not.
    h.state_mut().sch_state.selected = Some(Item::Sheet(0));
    h.run();
    h.state_mut().sch_state.tool = Tool::Sheet;
    h.run();
    h.state_mut().sch_state.sheet = Some("power_supply_2.kicad_sch".into());
    h.run();
}
