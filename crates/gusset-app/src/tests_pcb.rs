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
        blocked: None,
    };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    let b = h.state().snapshot.pcb.clone().unwrap();
    assert_eq!((b.board.tracks.len(), b.board.vias.len()), (1, 1));
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert!(h.state().snapshot.pcb.as_ref().unwrap().board.vias.is_empty());
    // DRC markers come from the check's result.
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![("pcb.drc", json!({"min_width": 1.0}))], blocked: None });
    h.state_mut().wait_idle();
    assert_eq!(h.state().pcb_state.markers.len(), 1);
    h.run();
    // Another tab leaves the editor.
    h.state_mut().ribbon_action(crate::ribbon::Action::SolidTab);
    assert!(!h.state().pcb_view_shown());
}

/// R1's pads (A at (9, 10), B at (11, 10)) with a second A pad on R2 at (21, 10) and a
/// wall of B copper between them, and a GND zone on B.Cu.
fn routing_board() -> gusset_commands::pcb_session::OpenBoard {
    let mut b = board();
    let fp = r#"(footprint "R" (layer "F.Cu") (at 22 10)
    (property "Reference" "R2")
    (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu") (net 2 "B")))"#;
    b.tree.items_mut().unwrap().push(gusset_pcb::sexpr::parse(fp, &Default::default()).unwrap());
    let zone = r#"(zone (net 0) (net_name "") (layer "B.Cu") (connect_pads (clearance 0.3)) (min_thickness 0.25)
    (fill (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1))
    (polygon (pts (xy 1 1) (xy 29 1) (xy 29 19) (xy 1 19))))"#;
    b.tree.items_mut().unwrap().push(gusset_pcb::sexpr::parse(zone, &Default::default()).unwrap());
    let mut b = Board::from_tree(b.tree, &Limits::default()).unwrap();
    b.add_track([15.0, 6.0], [15.0, 14.0], 0.5, "F.Cu", Some("B"), &Limits::default()).unwrap();
    let rules = gusset_pcb::rules::Rules::for_board(&b);
    let f = gusset_pcb::zones::fill(&b, &rules, None);
    b.set_zone_fills(&f.fills, &Limits::default()).unwrap();
    gusset_commands::pcb_session::OpenBoard {
        file: "r.kicad_pcb".into(),
        board_hash: "h1".into(),
        project_hash: None,
        ratsnest: gusset_pcb::ratsnest::ratsnest(&b),
        rules,
        board: b,
    }
}

#[test]
fn routing_previews_walk_around_and_finish_on_the_net() {
    let open = routing_board();
    let b = &open.board;
    let mut st = pcb_view::State::default();
    // A click on R1's A pad starts there, on net A.
    assert_eq!(pcb_view::route_target(b, [8.8, 10.2], "F.Cu", 0.5, None), ([9.0, 10.0], false));
    assert_eq!(pcb_view::net_at(b, [9.0, 10.0], "F.Cu").as_deref(), Some("A"));
    st.track_from = Some([9.0, 10.0]);
    st.route_net = Some("A".into());
    // R2's A pad finishes the route; its B pad does not.
    assert_eq!(pcb_view::route_target(b, [21.1, 10.0], "F.Cu", 0.5, Some("A")), ([21.0, 10.0], true));
    assert_eq!(pcb_view::route_target(b, [23.1, 10.0], "F.Cu", 0.5, Some("A")), ([23.0, 10.0], false));
    // The preview goes round the wall (past y 14 + clearance) in 45° steps.
    let path = pcb_view::route_preview(&open, &mut st, [21.0, 10.0]).unwrap();
    assert_eq!((path[0], *path.last().unwrap()), ([9.0, 10.0], [21.0, 10.0]));
    assert!(path.iter().any(|p| p[1] > 14.4 || p[1] < 5.6), "{path:?}");
    // Cached while nothing changes.
    assert_eq!(pcb_view::route_preview(&open, &mut st, [21.0, 10.0]).unwrap(), path);
    // Into the wall: no way.
    assert!(pcb_view::route_preview(&open, &mut st, [15.0, 10.0]).is_none());
    // The click lays the previewed path as it is, with the route's net.
    let (id, args) = pcb_view::route_command(&path, "F.Cu", Some("A"), 1.0);
    assert!(gusset_commands::find(id).is_some());
    assert_eq!(
        (args["around"].clone(), args["net"].clone(), args["arc_radius"].clone()),
        (json!(false), json!("A"), json!(1.0))
    );
    // A layer change: a via where the route stands (if it fits), then B.Cu.
    assert_eq!(pcb_view::other_layer(b, "F.Cu").as_deref(), Some("B.Cu"));
    assert!(pcb_view::via_fits(&open, [5.0, 5.0], Some("A")));
    assert!(!pcb_view::via_fits(&open, [15.2, 10.0], Some("A")));
    assert_eq!(pcb_view::route_via_command([5.0, 5.0], Some("A")).1["net"], "A");
    // Highlighting: a picked track's net.
    assert_eq!(pcb_view::net_of(b, Sel::Track(1)).as_deref(), Some("B"));
}

#[test]
fn zone_fills_are_triangulated_once_per_polygon() {
    let open = routing_board();
    let mut cache = pcb_view::FillCache::default();
    let tris = pcb_view::fill_triangles(&mut cache, "h1", &open.board, "B.Cu");
    assert_eq!(tris.len(), 1);
    // The triangles cover the fill exactly.
    let (poly, t) = &tris[0];
    let area: f64 = t
        .iter()
        .map(|t| {
            let (p, q, r) = (poly[t[0] as usize], poly[t[1] as usize], poly[t[2] as usize]);
            ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])).abs() / 2.0
        })
        .sum();
    let want = gusset_pcb::poly::total_area(&gusset_pcb::poly::even_odd(&[poly.to_vec()]));
    assert!((area - want).abs() < 1e-6 * want, "{area} vs {want}");
    let again = pcb_view::fill_triangles(&mut cache, "h2", &open.board, "B.Cu");
    assert!(std::sync::Arc::ptr_eq(&again[0].1, &tris[0].1), "kept across boards with the same fill");
    assert!(pcb_view::fill_triangles(&mut cache, "h2", &open.board, "F.Cu").is_empty());
}

#[test]
fn box_select_multi_delete_and_the_footprint_browser() {
    let b = board();
    let all = |_: &str| true;
    // A box round the right half picks the track and the via, not R1.
    let picked = pcb_view::in_box(&b, [18.0, 2.0], [28.0, 18.0], &all);
    assert_eq!(picked, vec![Sel::Track(0), Sel::Via(0)]);
    assert_eq!(pcb_view::in_box(&b, [5.0, 5.0], [15.0, 15.0], &all), vec![Sel::Footprint(0)]);
    let mut st = pcb_view::State::default();
    st.selected = Some(Sel::Footprint(0));
    st.multi = picked;
    assert!(pcb_view::is_picked(&st, Sel::Via(0)) && pcb_view::is_picked(&st, Sel::Footprint(0)));
    let (id, args) = pcb_view::delete_picked(&st).unwrap();
    assert_eq!((id, args), ("pcb.delete", json!({"tracks": [0], "vias": [0]})));
    st.multi.clear();
    assert!(pcb_view::delete_picked(&st).is_none(), "footprints are not deleted this way");
    // The browser lists Lib:Name from Lib.pretty folders, and a .pretty folder itself.
    let dir = std::env::temp_dir().join(format!("gusset-fp-lib-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("Res.pretty")).unwrap();
    std::fs::write(dir.join("Res.pretty/R_0603.kicad_mod"), "(footprint \"R_0603\")").unwrap();
    std::fs::write(dir.join("Res.pretty/R_0805.kicad_mod"), "(footprint \"R_0805\")").unwrap();
    std::fs::write(dir.join("Loose.kicad_mod"), "(footprint \"Loose\")").unwrap();
    let names = pcb_view::library_footprints(&dir.to_string_lossy(), 100);
    assert_eq!(names, ["Loose", "Res:R_0603", "Res:R_0805"]);
    let own = pcb_view::library_footprints(&dir.join("Res.pretty").to_string_lossy(), 1);
    assert_eq!(own, ["Res:R_0603"]);
    st.library = dir.join("Res.pretty").to_string_lossy().into_owned();
    st.footprint = "Res:R_0805".into();
    let (id, args) = pcb_view::place_command(&st, [3.0, 4.0]).unwrap();
    assert!(gusset_commands::find(id).is_some());
    assert_eq!(args["library"], dir.to_string_lossy().as_ref());
    assert_eq!((args["footprint"].clone(), args["at"].clone()), (json!("Res:R_0805"), json!([3.0, 4.0])));
    st.footprint.clear();
    assert!(pcb_view::place_command(&st, [3.0, 4.0]).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
