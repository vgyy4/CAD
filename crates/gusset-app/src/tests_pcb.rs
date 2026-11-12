//! The PCB tab (ADR 0047 phase 1b): editor logic, and edits going through the registry.
#![allow(clippy::float_cmp)] // snapped values are rounded to whole micrometres

use crate::pcb_view::{self, Sel, Tool};
use crate::tests::{app, harness};
use gusset_pcb::{Board, Limits};
use serde_json::json;

/// A 30 × 20 board with one two-pin part (R1) at (10, 10) and a track and a via.
fn board() -> Board {
    let mut b = Board::new_two_layer(30.0, 20.0).unwrap();
    let fp = r#"(footprint "R" (layer "F.Cu") (at 10 10)
    (property "Reference" "R1")
    (fp_rect (start -2 -1) (end 2 1) (stroke (width 0.1)) (layer "F.Fab"))
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu") (net 2 "B")))"#;
    let node = gusset_pcb::sexpr::parse(fp, &Default::default()).unwrap();
    b.tree.items_mut().unwrap().push(node);
    let mut b = Board::from_tree(b.tree, &Limits::default()).unwrap();
    b.add_track([20.0, 5.0], [25.0, 5.0], 0.25, "B.Cu", None, &Limits::default()).unwrap();
    b.add_via([25.0, 15.0], 0.6, 0.3, None, &Limits::default()).unwrap();
    b
}

#[test]
fn snapping_hits_and_drops() {
    assert_eq!(pcb_view::snap([1.26, -0.74], 0.5), [1.5, -0.5]);
    assert_eq!(pcb_view::snap([2.6, 0.0], 1.27), [2.54, 0.0]);
    let b = board();
    let all = |_: &str| true;
    assert_eq!(pcb_view::hit(&b, [25.1, 15.0], 0.05, &all), Some(Sel::Via(0)));
    assert_eq!(pcb_view::hit(&b, [22.0, 5.1], 0.05, &all), Some(Sel::Track(0)));
    // A hidden layer's tracks are not picked.
    assert_eq!(pcb_view::hit(&b, [22.0, 5.1], 0.05, &|l: &str| l != "B.Cu"), None);
    assert_eq!(pcb_view::hit(&b, [11.5, 10.8], 0.05, &all), Some(Sel::Footprint(0)));
    assert_eq!(pcb_view::hit(&b, [5.0, 18.0], 0.05, &all), None);
    assert_eq!(pcb_view::pad_center_at(&b, [8.8, 10.2], "F.Cu"), Some([9.0, 10.0]));
    assert_eq!(pcb_view::pad_center_at(&b, [8.8, 10.2], "B.Cu"), None);
    assert_eq!(pcb_view::drop_place(&b, 0, [2.3, -1.1], 0.5), Some([12.5, 9.0]));
}

#[test]
fn editor_actions_are_registry_commands() {
    let b = board();
    let ids: Vec<&str> = [
        pcb_view::move_command(0, [1.0, 2.0]),
        pcb_view::rotate_command(&b, 0).unwrap(),
        pcb_view::flip_command(0),
        pcb_view::delete_command(Sel::Track(0)).unwrap(),
        pcb_view::track_command([0.0, 0.0], [1.0, 0.0], "F.Cu"),
        pcb_view::via_command([1.0, 1.0]),
    ]
    .iter()
    .map(|c| c.0)
    .collect();
    for id in &ids {
        assert!(gusset_commands::find(id).is_some(), "{id}");
    }
    assert_eq!(pcb_view::rotate_command(&b, 0).unwrap().1["angle"], 90.0);
    assert!(pcb_view::delete_command(Sel::Footprint(0)).is_none());
    // The active copper layer is drawn above the other copper.
    let order = pcb_view::draw_order(&b, "B.Cu");
    let pos = |l: &str| order.iter().position(|x| x == l).unwrap();
    assert!(pos("B.Cu") > pos("F.Cu") && pos("F.SilkS") > pos("B.Cu") && pos("Edge.Cuts") == order.len() - 1);
    let m = pcb_view::markers_from(&json!({"violations": [{"at": [1.5, 2.0], "message_user_text": "too close"}]}));
    assert_eq!(m, [pcb_view::Marker { at: [1.5, 2.0], text: "too close".into() }]);
    assert!(pcb_view::unrouted_text(3).ends_with(": 3"));
}

#[test]
fn the_pcb_tab_edits_the_board_with_undo() {
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.run();
    assert!(h.state().pcb_view_shown());
    // Every PCB ribbon tool exists in the registry.
    assert!(crate::ribbon::pcb_command_ids().len() >= 14);
    h.state_mut().run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).unwrap();
    h.state_mut().wait_idle();
    h.run();
    assert!(h.state().snapshot.pcb.is_some());
    // The editor's track and via tools send registry commands.
    let st = &mut h.state_mut().pcb_state;
    st.tool = Tool::Route;
    let out = pcb_view::Out {
        commands: vec![pcb_view::track_command([5.0, 5.0], [15.0, 5.0], "F.Cu"), pcb_view::via_command([15.0, 5.0])],
    };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    let b = h.state().snapshot.pcb.clone().unwrap();
    assert_eq!((b.board.tracks.len(), b.board.vias.len()), (1, 1));
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert!(h.state().snapshot.pcb.as_ref().unwrap().board.vias.is_empty());
    // DRC markers come from the check's result.
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![("pcb.drc", json!({"min_width": 1.0}))] });
    h.state_mut().wait_idle();
    assert_eq!(h.state().pcb_state.markers.len(), 1);
    h.run();
    // Another tab leaves the editor.
    h.state_mut().ribbon_action(crate::ribbon::Action::SolidTab);
    assert!(!h.state().pcb_view_shown());
}
