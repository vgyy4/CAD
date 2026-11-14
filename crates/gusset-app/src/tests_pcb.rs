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

#[test]
fn zones_pad_nets_classes_and_the_autorouter_from_the_panel() {
    let b = board();
    // Pad nets of R1, and the command for one.
    assert_eq!(pcb_view::pad_nets_of(&b, 0), vec![("1".into(), "A".into()), ("2".into(), "B".into())]);
    assert!(pcb_view::pad_nets_of(&b, 5).is_empty());
    assert_eq!(pcb_view::pad_net_command("R1", "2", " GND "), ("pcb.set_net", json!({"pads": ["R1.2"], "net": "GND"})));
    assert_eq!(pcb_view::pad_net_command("R1", "2", ""), ("pcb.set_net", json!({"pads": ["R1.2"]})));
    // Net classes: only the sizes given.
    let c = pcb_view::ClassEdit { name: "Power".into(), track_width: 0.5, ..Default::default() };
    assert_eq!(
        pcb_view::class_command(&c, &["GND".into()]),
        Some(("pcb.net_class", json!({"name": "Power", "nets": ["GND"], "track_width": 0.5})))
    );
    assert_eq!(pcb_view::class_command(&pcb_view::ClassEdit::default(), &[]), None);
    // A click where no way shows: the router is asked to shove.
    assert_eq!(
        pcb_view::shove_command([1.0, 2.0], [3.0, 2.0], "B.Cu", Some("A")),
        ("pcb.route", json!({"points": [[1.0, 2.0], [3.0, 2.0]], "layer": "B.Cu", "net": "A"}))
    );
    // The Zone tool's corners.
    let mut st = pcb_view::State::default();
    st.tool = Tool::Zone;
    st.zone_pts = vec![[0.0, 0.0], [10.0, 0.0]];
    assert_eq!(pcb_view::zone_command(&st), None);
    st.zone_pts.push([10.0, 10.0]);
    st.zone_net = "GND".into();
    st.zone_hatch = true;
    assert_eq!(
        pcb_view::zone_command(&st),
        Some((
            "pcb.add_zone",
            json!({"outline": [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], "layers": ["F.Cu"], "net": "GND",
                   "hatch": {"thickness": 0.5, "gap": 1.0}})
        ))
    );
    pcb_view::end_route(&mut st);
    assert!(st.zone_pts.is_empty());

    // Through the app: a zone, a pad net and a class, each undoable; the autorouter
    // refuses cleanly while no Freerouting is named.
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.state_mut().run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).unwrap();
    h.state_mut().wait_idle();
    let mut st = pcb_view::State::default();
    st.tool = Tool::Zone;
    st.zone_pts = vec![[2.0, 2.0], [38.0, 2.0], [38.0, 28.0], [2.0, 28.0]];
    st.zone_net = "GND".into();
    let commands = vec![
        pcb_view::zone_command(&st).unwrap(),
        pcb_view::class_command(
            &pcb_view::ClassEdit { name: "Power".into(), clearance: 0.3, ..Default::default() },
            &["GND".into()],
        )
        .unwrap(),
    ];
    h.state_mut().pcb_commands(pcb_view::Out { commands, blocked: None });
    h.state_mut().wait_idle();
    let open = h.state().snapshot.pcb.clone().unwrap();
    assert_eq!(open.board.zones.len(), 1);
    assert!((open.rules.clearance_of(Some("GND")) - 0.3).abs() < 1e-9);
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![("pcb.autoroute", json!({}))], blocked: None });
    h.state_mut().wait_idle();
    h.run();
    let notice = h.state().notice.clone().map(|n| n.text).unwrap_or_default();
    assert!(notice.contains("Freerouting"), "{notice}");
    // The panel's setting reaches the session (agents use it too).
    h.state_mut().pcb_state.freerouting = "/nonexistent/freerouting.jar".into();
    h.run();
    h.state_mut().wait_idle();
    let cfg = h.state().host().with_session(|s| s.freerouting.clone()).unwrap();
    assert_eq!(cfg.map(|c| c.path), Some(std::path::PathBuf::from("/nonexistent/freerouting.jar")));
    // KiCad's own checks: refused until the user names a kicad-cli; then it reaches the session.
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![("pcb.kicad_drc", json!({}))], blocked: None });
    h.state_mut().wait_idle();
    h.run();
    let notice = h.state().notice.clone().map(|n| n.text).unwrap_or_default();
    assert!(notice.contains("no kicad-cli"), "{notice}");
    assert!(h.state().host().with_session(|s| s.kicad_cli.clone()).unwrap().is_none());
    h.state_mut().pcb_state.kicad_cli = "/nonexistent/kicad-cli".into();
    h.state_mut().pcb_state.kicad_cli_timeout = 42;
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![("pcb.kicad_drc", json!({}))], blocked: None });
    h.state_mut().wait_idle();
    h.run();
    let k = h.state().host().with_session(|s| s.kicad_cli.clone()).unwrap().unwrap();
    assert_eq!((k.path, k.timeout_s), (std::path::PathBuf::from("/nonexistent/kicad-cli"), 42));
    let notice = h.state().notice.clone().map(|n| n.text).unwrap_or_default();
    assert!(notice.contains("could not start kicad-cli"), "{notice}");
    // Findings of the report show as markers and list lines marked as KiCad's.
    let report = json!({"violations": [{"source": "kicad", "message_user_text": "Clearance", "at": [1.0, 2.0]}]});
    assert_eq!(pcb_view::markers_from(&report)[0].text, "[KiCad] Clearance");
    let lines = crate::sch_view::kicad_issues_from(
        &json!({"issues": [{"severity": "error", "sheet_user_text": "/", "message_user_text": "Pin not connected"}]}),
    );
    assert_eq!(lines, ["error /: Pin not connected"]);
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert!(h.state().snapshot.pcb.as_ref().unwrap().board.zones.is_empty());
}

#[test]
fn a_pad_is_picked_on_its_own_and_its_net_set_through_the_registry() {
    let b = board();
    let all = |_: &str| true;
    // On the pad: the pad; beside it in the part's box: the footprint.
    assert_eq!(pcb_view::hit(&b, [9.2, 10.2], 0.05, &all), Some(Sel::Pad(0)));
    assert_eq!(pcb_view::hit(&b, [11.0, 10.0], 0.05, &all), Some(Sel::Pad(1)));
    assert_eq!(pcb_view::hit(&b, [10.0, 10.8], 0.05, &all), Some(Sel::Footprint(0)));
    // Its F.Cu layer hidden, a pad is not picked.
    assert_eq!(pcb_view::hit(&b, [9.2, 10.2], 0.05, &|l: &str| l != "F.Cu"), Some(Sel::Footprint(0)));
    assert_eq!(pcb_view::pad_name(&b, 1), Some(("R1".into(), "2".into())));
    assert_eq!(pcb_view::pad_owner(&b, 1), Some(0));
    assert_eq!(pcb_view::net_of(&b, Sel::Pad(1)).as_deref(), Some("B"));
    let c = pcb_view::pad_net_command("R1", "2", " GND ");
    assert_eq!((c.0, c.1["pads"].clone(), c.1["net"].clone()), ("pcb.set_net", json!(["R1.2"]), json!("GND")));
    // A pad dragged moves its footprint; a pad has no delete command of its own.
    assert!(pcb_view::delete_command(Sel::Pad(0)).is_none());
    // The command it sends is the registry's `pcb.set_net` (run and undone in gusset-commands).
    assert!(gusset_commands::find("pcb.set_net").is_some());
}

#[test]
fn tracks_and_vias_are_dragged_through_pcb_move() {
    let b = board();
    let mut st = pcb_view::State::default();
    st.selected = Some(Sel::Via(0));
    // The drop snaps the via's centre to the grid: (25, 15) + (1.1, -0.9) → (26, 14).
    let by = pcb_view::item_drop_delta(&b, Sel::Via(0), [1.1, -0.9], 0.5).unwrap();
    assert_eq!(by, [1.0, -1.0]);
    let c = pcb_view::move_items_command(&st, Sel::Via(0), by).unwrap();
    assert_eq!((c.0, c.1["vias"].clone(), c.1["tracks"].clone()), ("pcb.move", json!([0]), json!([])));
    assert_eq!(c.1["by"], json!([1.0, -1.0]));
    // A track is snapped by its start; a drag that lands where it was sends nothing.
    assert_eq!(pcb_view::item_drop_delta(&b, Sel::Track(0), [0.1, 0.1], 0.5), Some([0.0, 0.0]));
    assert!(pcb_view::move_items_command(&st, Sel::Via(0), [0.0, 0.0]).is_none());
    // Dragging a picked item moves all picked tracks and vias; an unpicked one moves alone.
    st.multi = vec![Sel::Track(0)];
    let (t, v) = pcb_view::dragged_items(&st, Sel::Track(0));
    assert_eq!((t, v), (vec![0], vec![0]));
    assert_eq!(pcb_view::dragged_items(&st, Sel::Track(1)), (vec![1], vec![]));
    // Through the app: the via and the track end on it move in one undo step.
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.run();
    h.state_mut().run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).unwrap();
    h.state_mut().wait_idle();
    h.state_mut().run("pcb.add_track", json!({"start": [10, 10], "end": [20, 10]})).unwrap();
    h.state_mut().run("pcb.add_via", json!({"at": [20, 10]})).unwrap();
    h.state_mut().wait_idle();
    let mut st = pcb_view::State::default();
    st.selected = Some(Sel::Via(0));
    let by = {
        let o = h.state().snapshot.pcb.as_ref().unwrap();
        pcb_view::item_drop_delta(&o.board, Sel::Via(0), [2.2, 3.1], 0.5).unwrap()
    };
    let out = pcb_view::Out {
        commands: pcb_view::move_items_command(&st, Sel::Via(0), by).into_iter().collect(),
        blocked: None,
    };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    {
        let b = &h.state().snapshot.pcb.as_ref().unwrap().board;
        assert_eq!((b.vias[0].at, b.tracks[0].end, b.tracks[0].start), ([22.0, 13.0], [22.0, 13.0], [10.0, 10.0]));
    }
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    let b = &h.state().snapshot.pcb.as_ref().unwrap().board;
    assert_eq!((b.vias[0].at, b.tracks[0].end), ([20.0, 10.0], [20.0, 10.0]));
}

#[test]
fn drag_previews_show_stretched_tracks() {
    let mut b = board();
    let l = Limits::default();
    b.add_track([9.0, 10.0], [9.0, 5.0], 0.25, "F.Cu", Some("A"), &l).unwrap(); // 1: on R1 pad 1
    b.add_track([9.0, 5.0], [15.0, 5.0], 0.25, "F.Cu", Some("A"), &l).unwrap(); // 2: joins 1 at its end
    b.add_track([9.0, 5.0], [9.0, 2.0], 0.25, "B.Cu", Some("A"), &l).unwrap(); // 3: other layer
    let st = pcb_view::State::default();
    // Dragging track 1 moves it whole and its neighbour's coincident end follows.
    let (t, v, by, ends) = pcb_view::drag_preview(&st, &b, Some(Sel::Track(1)), None, [0.1, 1.1]).unwrap();
    assert_eq!((t, v, by), (vec![1], vec![], [0.0, 1.0]));
    assert_eq!(ends, vec![(2, [true, false])]);
    // Dragging the footprint carries the end on its pad, where the drop lands.
    let (t, v, by, ends) = pcb_view::drag_preview(&st, &b, None, Some(0), [1.1, 0.0]).unwrap();
    assert_eq!((t, v, by), (vec![], vec![], [1.0, 0.0]));
    assert_eq!(ends, vec![(1, [true, false])]);
    // The preview agrees with the command: moving the footprint stretches the same end.
    let old = b.footprint_pads(0);
    b.move_footprint(0, [11.0, 10.0], 0.0, &l).unwrap();
    assert_eq!(b.carry_track_ends(0, &old, 0.0, &l).unwrap(), 1);
    assert_eq!(b.tracks[1].start, [10.0, 10.0]);
}

#[test]
fn the_outline_tool_sends_set_outline() {
    let mut st = pcb_view::State::default();
    st.tool = Tool::Outline;
    // Fewer than three corners send nothing.
    st.outline_pts = vec![[0.0, 0.0], [10.0, 0.0]];
    assert!(pcb_view::outline_command(&st).is_none());
    st.outline_pts = vec![[0.0, 0.0], [30.0, 0.0], [30.0, 20.0], [0.0, 20.0]];
    let c = pcb_view::outline_command(&st).unwrap();
    assert_eq!((c.0, c.1["points"].clone()), ("pcb.set_outline", json!(st.outline_pts)));
    // Through the app: the board's outline becomes the polygon, in one undo step; Esc-like
    // ending clears the corners.
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.run();
    h.state_mut().run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).unwrap();
    h.state_mut().wait_idle();
    let out = pcb_view::Out { commands: vec![c], blocked: None };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    let area = |b: &Board| {
        let (loops, _) = gusset_pcb::geom::outline_loops(b, 1e-3);
        gusset_pcb::geom::area(&loops[0].polygon(0.01)).abs()
    };
    assert!((area(&h.state().snapshot.pcb.as_ref().unwrap().board) - 600.0).abs() < 1e-6);
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert!((area(&h.state().snapshot.pcb.as_ref().unwrap().board) - 1200.0).abs() < 1e-6);
    pcb_view::end_route(&mut st);
    assert!(st.outline_pts.is_empty());
    h.run();
}

#[test]
fn texts_are_picked_and_added_through_the_registry() {
    let mut b = board();
    let l = Limits::default();
    let i = b
        .add_text(
            &gusset_pcb::text_edit::NewText {
                text: "HELLO".into(),
                at: [5.0, 15.0],
                layer: Some("B.SilkS".into()),
                size: Some(2.0),
                thickness: None,
                angle: None,
                mirrored: None,
                justify: None,
            },
            &l,
        )
        .unwrap();
    assert!(b.texts[i].mirrored);
    let all = |_: &str| true;
    // A point on the first letter's stem picks the text; one far away does not.
    let strokes = gusset_pcb::font::strokes(&b.texts[i], "HELLO");
    let on = strokes[0][0];
    assert_eq!(pcb_view::hit(&b, on, 0.05, &all), Some(Sel::Text(i)));
    assert!(pcb_view::text_dist(&b, i, [0.0, 0.0]).is_some_and(|d| d > 5.0));
    // A hidden layer's text is not picked.
    assert_ne!(pcb_view::hit(&b, on, 0.05, &|l: &str| l != "B.SilkS"), Some(Sel::Text(i)));
    // The commands the editor sends exist and carry the panel's settings.
    let mut st = pcb_view::State::default();
    st.text_words = "REV".into();
    st.text_back = true;
    let add = pcb_view::text_add_command(&st, [3.0, 4.0]);
    assert_eq!((add.0, add.1["layer"].as_str(), add.1["at"].clone()), ("pcb.text", Some("B.SilkS"), json!([3.0, 4.0])));
    assert!(gusset_commands::find("pcb.text").is_some());
    let del = pcb_view::delete_command(Sel::Text(i)).unwrap();
    assert_eq!((del.0, del.1["delete"].as_bool()), ("pcb.text", Some(true)));
    // Through the app: add, edit and undo each take one step.
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.run();
    h.state_mut().run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).unwrap();
    h.state_mut().wait_idle();
    h.state_mut().pcb_state.tool = Tool::Text;
    let out = pcb_view::Out { commands: vec![pcb_view::text_add_command(&st, [10.0, 10.0])], blocked: None };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    let n = h.state().snapshot.pcb.as_ref().unwrap().board.texts.len();
    assert_eq!(n, 1);
    let out = pcb_view::Out { commands: vec![pcb_view::text_edit_command(0, "NEW", 2.0, 90.0, false)], blocked: None };
    h.state_mut().pcb_commands(out);
    h.state_mut().wait_idle();
    let t = h.state().snapshot.pcb.as_ref().unwrap().board.texts[0].clone();
    assert_eq!((t.text.as_str(), t.size, t.angle, t.mirrored), ("NEW", 2.0, 90.0, false));
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert_eq!(h.state().snapshot.pcb.as_ref().unwrap().board.texts[0].text, "REV");
    h.run();
}

#[test]
fn zone_outlines_are_edited_by_corner_through_the_registry() {
    use crate::pcb_zone_edit as ze;
    let sq = vec![vec![[2.0, 2.0], [12.0, 2.0], [12.0, 12.0], [2.0, 12.0]]];
    // Hits: a corner, an edge's middle handle, and the picked zone.
    assert_eq!(ze::corner_at(&sq, [12.05, 2.05], 0.2), Some((0, 1)));
    assert_eq!(ze::corner_at(&sq, [7.0, 7.0], 0.2), None);
    assert_eq!(ze::edge_mid(&sq, (0, 3)), Some([2.0, 7.0]));
    assert_eq!(ze::edge_at(&sq, [7.1, 1.9], 0.3), Some((0, 0)));
    assert_eq!(ze::edge_at(&sq, [2.1, 2.1], 0.3), None, "a corner is not a middle");
    // Move, insert, delete (always at least three corners).
    let moved = ze::move_corner(&sq, (0, 2), [14.0003, 13.0]).unwrap();
    assert_eq!(moved[0][2], [14.0, 13.0]);
    assert!(ze::move_corner(&sq, (0, 9), [0.0, 0.0]).is_none());
    let (ins, new) = ze::insert_corner(&sq, (0, 0)).unwrap();
    assert_eq!((ins[0].len(), new, ins[0][1]), (5, (0, 1), [7.0, 2.0]));
    let del = ze::delete_corner(&ins, new).unwrap();
    assert_eq!(del, sq);
    assert!(ze::delete_corner(&sq, (0, 1)).is_some_and(|o| o[0].len() == 3));
    assert!(
        ze::delete_corner(&[vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]], (0, 0)).is_none(),
        "the last triangle stays"
    );
    let two = vec![sq[0].clone(), vec![[20.0, 2.0], [25.0, 2.0], [25.0, 5.0]]];
    assert_eq!(ze::delete_corner(&two, (1, 0)), Some(vec![sq[0].clone()]), "a spare triangle goes whole");

    // Through the app: a zone, then each edit is one undo step with its refill.
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.state_mut().run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).unwrap();
    h.state_mut().wait_idle();
    h.state_mut()
        .run("pcb.add_zone", json!({"outline": [[2, 2], [12, 2], [12, 12], [2, 12]], "layers": ["F.Cu"]}))
        .unwrap();
    h.state_mut().wait_idle();
    let board = h.state().snapshot.pcb.clone().unwrap().board.clone();
    let all = |_: &str| true;
    assert_eq!(ze::zone_at(&board, [7.0, 7.0], 0.1, &all), Some(0));
    assert_eq!(ze::zone_at(&board, [20.0, 20.0], 0.1, &all), None);
    assert_eq!(ze::zone_at(&board, [7.0, 7.0], 0.1, &|l: &str| l != "F.Cu"), None, "its layer is hidden");
    let mut st = pcb_view::State::default();
    st.tool = Tool::ZoneEdit;
    st.zone_edit = Some(ze::ZoneEdit { zone: 0, ..Default::default() });
    let outlines = ze::outlines_of(&board, 0);
    assert_eq!(outlines, sq);
    let area = |h: &egui_kittest::Harness<'static, crate::app::App>| -> f64 {
        let b = &h.state().snapshot.pcb.as_ref().unwrap().board;
        b.zones[0]
            .fills
            .iter()
            .map(|(_, p)| gusset_pcb::poly::total_area(&gusset_pcb::poly::union(std::slice::from_ref(p))))
            .sum()
    };
    let before = area(&h);
    // Drag the corner (12, 12) to (22, 12): the zone grows.
    let cmd = ze::zone_outline_command(0, &ze::move_corner(&outlines, (0, 2), [22.0, 12.0]).unwrap());
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![cmd], blocked: None });
    h.state_mut().wait_idle();
    let board = h.state().snapshot.pcb.clone().unwrap().board.clone();
    assert_eq!(board.zones[0].outline[2], [22.0, 12.0]);
    assert!(area(&h) > before + 20.0, "{} vs {before}", area(&h));
    // Insert a corner in the middle of the top edge, then delete it: two more steps.
    let o = ze::outlines_of(&board, 0);
    let (ins, new) = ze::insert_corner(&o, (0, 0)).unwrap();
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![ze::zone_outline_command(0, &ins)], blocked: None });
    h.state_mut().wait_idle();
    assert_eq!(h.state().snapshot.pcb.as_ref().unwrap().board.zones[0].outline.len(), 5);
    let o = ze::outlines_of(&h.state().snapshot.pcb.as_ref().unwrap().board, 0);
    let del = ze::delete_corner(&o, new).unwrap();
    h.state_mut().pcb_commands(pcb_view::Out { commands: vec![ze::zone_outline_command(0, &del)], blocked: None });
    h.state_mut().wait_idle();
    assert_eq!(h.state().snapshot.pcb.as_ref().unwrap().board.zones[0].outline.len(), 4);
    // Each undo takes one edit back, and the fill with it.
    for want in [5, 4] {
        h.state_mut().run("history.undo", json!({})).unwrap();
        h.state_mut().wait_idle();
        assert_eq!(h.state().snapshot.pcb.as_ref().unwrap().board.zones[0].outline.len(), want);
    }
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
    assert!((area(&h) - before).abs() < 1e-6, "{} vs {before}", area(&h));
    // The fill-on-edit toggle reaches the session.
    h.state_mut().pcb_state.fill_on_edit = true;
    h.run();
    h.state_mut().wait_idle();
    assert!(h.state().host().with_session(|s| s.pcb_fill_on_edit).unwrap());
    h.run();
}

/// 40 x 24 mm: USB_P/USB_N pads at both ends, a SIG wall between them, a short USB_P run
/// (12 mm) and a longer jogged USB_N run (ADR 0055's test board).
const PAIR_BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "USB_P") (net 2 "USB_N") (net 3 "SIG")
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
  (segment (start 31 22) (end 34 22) (width 0.2) (layer "F.Cu") (net 2)))"#;

type TestApp = egui_kittest::Harness<'static, crate::app::App>;

/// An app with the pair board open on the PCB tab.
fn pair_app(tag: &str) -> TestApp {
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.run();
    let dir = std::env::temp_dir().join(format!("gusset-pcb-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pair.kicad_pcb");
    std::fs::write(&path, PAIR_BOARD).unwrap();
    h.state_mut().run("pcb.open", json!({"path": path.to_string_lossy()})).unwrap();
    h.state_mut().wait_idle();
    h
}

fn send(h: &mut TestApp, commands: Vec<(&'static str, serde_json::Value)>) {
    h.state_mut().pcb_commands(pcb_view::Out { commands, blocked: None });
    h.state_mut().wait_idle();
}

fn undo(h: &mut TestApp) {
    h.state_mut().run("history.undo", json!({})).unwrap();
    h.state_mut().wait_idle();
}

fn tracks_now(h: &TestApp) -> usize {
    h.state().snapshot.pcb.as_ref().unwrap().board.tracks.len()
}

#[test]
fn arc_segments_are_previewed_and_laid_through_add_track() {
    // The preview is the arc through the three points; a line has no arc.
    let path = pcb_view::arc_preview([0.0, 0.0], [5.0, 5.0], [10.0, 0.0]).unwrap();
    assert_eq!((path[0], *path.last().unwrap()), ([0.0, 0.0], [10.0, 0.0]));
    assert!(path.len() > 8 && path.iter().all(|p| (libm::hypot(p[0] - 5.0, p[1]) - 5.0).abs() < 1e-6));
    assert!(pcb_view::arc_preview([0.0, 0.0], [5.0, 0.0], [10.0, 0.0]).is_none());
    // The command: `pcb.add_track` with `mid`, on the route's net.
    let (id, args) = pcb_view::arc_track_command([0.0, 0.0], [5.0, 5.0], [10.0, 0.0], "F.Cu", Some("A"));
    assert_eq!((id, args["mid"].clone(), args["net"].clone()), ("pcb.add_track", json!([5.0, 5.0]), json!("A")));
    assert!(pcb_view::arc_track_command([0.0, 0.0], [5.0, 5.0], [10.0, 0.0], "F.Cu", None).1.get("net").is_none());
    // Esc or leaving the tool drops the point.
    let mut st = pcb_view::State::default();
    st.arc_through = Some([5.0, 5.0]);
    pcb_view::end_route(&mut st);
    assert!(st.arc_through.is_none());
    // Through the app: one arc track, one undo step; three points on a line are refused.
    let mut h = harness(app());
    h.run();
    h.state_mut().ribbon_action(crate::ribbon::Action::PcbTab);
    h.run();
    h.state_mut().run("pcb.new", json!({"width": "40 mm", "height": "30 mm"})).unwrap();
    h.state_mut().wait_idle();
    send(&mut h, vec![pcb_view::arc_track_command([10.0, 10.0], [15.0, 15.0], [20.0, 10.0], "F.Cu", None)]);
    {
        let b = &h.state().snapshot.pcb.as_ref().unwrap().board;
        assert_eq!(b.tracks.len(), 1);
        assert_eq!(b.tracks[0].mid, Some([15.0, 15.0]));
        let len = gusset_pcb::length::track_length(&b.tracks[0]);
        assert!((len - std::f64::consts::PI * 5.0).abs() < 1e-6, "{len}");
    }
    let (id, args) = pcb_view::arc_track_command([10.0, 10.0], [15.0, 10.0], [20.0, 10.0], "F.Cu", None);
    assert!(h.state_mut().run(id, args).is_err());
    undo(&mut h);
    assert_eq!(tracks_now(&h), 0);
}

#[test]
fn the_pair_tool_previews_both_tracks_and_routes_the_pair() {
    let mut h = pair_app("pair");
    let open = h.state().snapshot.pcb.clone().unwrap();
    let b = &open.board;
    // A pad of a pair net names the pair (either half); other nets and bare board do not.
    let pr = pcb_view::pair_at(b, [4.0, 11.0], "F.Cu").unwrap();
    assert_eq!((pr.p.as_str(), pr.n.as_str()), ("USB_P", "USB_N"));
    assert_eq!(pcb_view::pair_at(b, [4.0, 9.0], "F.Cu").unwrap().base, "USB");
    assert!(pcb_view::pair_at(b, [20.0, 5.0], "F.Cu").is_none());
    assert!(pcb_view::pair_at(b, [10.0, 10.0], "F.Cu").is_none());
    // Fewer than two points: nothing to send, nothing to preview.
    let mut st = pcb_view::State::default();
    st.tool = Tool::Pair;
    st.pair_net = Some("USB_P".into());
    st.pair_pts = vec![[6.0, 10.0]];
    assert!(pcb_view::pair_command("USB_P", &st.pair_pts, "F.Cu").is_none());
    assert!(pcb_view::pair_preview(&open, &mut st, None).is_none());
    // With the pointer as the end: both offset tracks round the wall.
    let r = pcb_view::pair_preview(&open, &mut st, Some([34.0, 10.0])).unwrap().unwrap();
    assert!(!r.p.is_empty() && !r.n.is_empty());
    assert!(r.p.iter().chain(r.n.iter()).any(|s| s.start[1] > 14.0), "round the wall");
    // A centre line that cannot be laid says why (the command's own message).
    let e = pcb_view::pair_preview(&open, &mut st, Some([20.0, 5.0])).unwrap().unwrap_err();
    assert!(!e.is_empty());
    // Clicking is the centre line; Enter sends it.
    st.pair_pts = vec![[6.0, 10.0], [34.0, 10.0]];
    let c = pcb_view::pair_command("USB_P", &st.pair_pts, "F.Cu").unwrap();
    assert_eq!(
        (c.0, c.1["pair"].clone(), c.1["points"].clone()),
        ("pcb.route_pair", json!("USB_P"), json!(st.pair_pts))
    );
    let before = b.tracks.len();
    send(&mut h, vec![c]);
    assert!(tracks_now(&h) > before);
    undo(&mut h);
    assert_eq!(tracks_now(&h), before);
    // A refused route changes nothing; the error reaches the status bar.
    let bad = pcb_view::pair_command("USB_P", &[[6.0, 10.0], [20.0, 5.0]], "F.Cu").unwrap();
    send(&mut h, vec![bad]);
    assert_eq!(tracks_now(&h), before);
    assert!(h.state().notice.as_ref().is_some_and(|n| n.error));
    // Esc drops the pair and its points.
    pcb_view::end_route(&mut st);
    assert!(st.pair_net.is_none() && st.pair_pts.is_empty());
}

#[test]
fn the_tune_tool_shows_lengths_and_tunes_through_the_registry() {
    let mut h = pair_app("tune");
    let open = h.state().snapshot.pcb.clone().unwrap();
    let b = &open.board;
    // The picked track's net: its length, and the pair's skew.
    let p = pcb_view::net_info(b, "USB_P");
    assert_eq!((p.tracks, p.vias, p.total), (1, 0, 12.0));
    let pair = p.pair.unwrap();
    assert_eq!((pair.partner.as_str(), pair.shorter.as_str()), ("USB_N", "USB_P"));
    assert!((pair.skew - (10.0 + 2.0 * std::f64::consts::SQRT_2 - 12.0)).abs() < 1e-3, "{}", pair.skew);
    let sig = pcb_view::net_info(b, "SIG");
    assert!(sig.total == 12.0 && sig.pair.is_none());
    // The panel draws the lengths and the tune form for the picked net without trouble.
    {
        let st = &mut h.state_mut().pcb_state;
        st.tool = Tool::Tune;
        st.selected = Some(Sel::Track(1));
        st.highlight = Some("USB_P".into());
    }
    h.run();
    assert_eq!(h.state().pcb_state.tune_target, 12.0);
    h.state_mut().pcb_state.tool = Tool::Pair;
    h.run();
    // The picked track names the track to meander, only when it is on the net.
    assert_eq!(pcb_view::tune_track(b, Some(Sel::Track(1)), "USB_P"), Some(1));
    assert_eq!(pcb_view::tune_track(b, Some(Sel::Track(0)), "USB_P"), None);
    assert_eq!(pcb_view::tune_track(b, Some(Sel::Via(0)), "USB_P"), None);
    // A click on the track picks it (the Tune tool uses the Select hit test).
    assert_eq!(pcb_view::hit(b, [28.0, 20.0], 0.05, &|_: &str| true), Some(Sel::Track(1)));
    // Command construction: target and options, or none (match the pair).
    let c = pcb_view::tune_command("USB_P", Some(16.0), 1.5, 0.3, Some(1));
    assert_eq!(c.1, json!({"net": "USB_P", "amplitude": 1.5, "target": 16.0, "spacing": 0.3, "track": 1}));
    let m = pcb_view::tune_command("USB_P", None, 1.0, 0.0, None);
    assert_eq!(m.1, json!({"net": "USB_P", "amplitude": 1.0}));
    // Through the app: to a target, one undo step.
    let before = b.tracks.len();
    send(&mut h, vec![c]);
    let o = h.state().snapshot.pcb.clone().unwrap();
    let len = pcb_view::net_info(&o.board, "USB_P").total;
    assert!((len - 16.0).abs() <= 0.05, "{len}");
    assert!(o.board.tracks.len() > before);
    undo(&mut h);
    assert_eq!(tracks_now(&h), before);
    // Match pair: the shorter net reaches the other's length.
    send(&mut h, vec![m]);
    let o = h.state().snapshot.pcb.clone().unwrap();
    assert!(pcb_view::net_info(&o.board, "USB_P").pair.unwrap().skew <= 0.05);
    undo(&mut h);
    // Already at the target: refused, the status bar says so, nothing changes.
    send(&mut h, vec![pcb_view::tune_command("USB_P", Some(12.0), 1.0, 0.0, None)]);
    assert_eq!(tracks_now(&h), before);
    assert!(h.state().notice.as_ref().is_some_and(|n| n.error));
}

/// A's pads in a pocket closed by C's pads, the board edge and B's track (ADR 0058): no way
/// round, but B can be pushed up.
fn pushing_board() -> gusset_commands::pcb_session::OpenBoard {
    let fp = |r: &str, at: [f64; 2], d: f64, net: &str| {
        format!(
            "(footprint \"P\" (layer \"F.Cu\") (at {} {}) (property \"Reference\" \"{r}\") \
             (pad \"1\" smd circle (at 0 0) (size {d} {d}) (layers \"F.Cu\") {net}))",
            at[0], at[1]
        )
    };
    let parts = [
        fp("B1", [2.0, 5.0], 0.6, "(net 2 \"B\")"),
        fp("B2", [28.0, 5.0], 0.6, "(net 2 \"B\")"),
        fp("A1", [8.0, 6.0], 0.5, "(net 1 \"A\")"),
        fp("A2", [22.0, 6.0], 0.5, "(net 1 \"A\")"),
        fp("C1", [3.0, 6.2], 1.2, "(net 3 \"C\")"),
        fp("C2", [27.0, 6.2], 1.2, "(net 3 \"C\")"),
        fp("C3", [15.0, 6.325], 1.2, "(net 3 \"C\")"),
    ]
    .join("\n");
    let text = format!(
        "(kicad_pcb (version 20240108) (generator \"test\") (general (thickness 1.6))
  (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (44 \"Edge.Cuts\" user))
  (net 0 \"\") (net 1 \"A\") (net 2 \"B\") (net 3 \"C\")
  (gr_rect (start 0 0) (end 30 7) (stroke (width 0.05) (type default)) (fill none) (layer \"Edge.Cuts\"))
  {parts}
  (segment (start 2 5) (end 28 5) (width 0.25) (layer \"F.Cu\") (net 2)))"
    );
    let b = Board::read(&text, &Limits::default()).unwrap();
    gusset_commands::pcb_session::OpenBoard {
        file: "p.kicad_pcb".into(),
        board_hash: "p1".into(),
        project_hash: None,
        ratsnest: gusset_pcb::ratsnest::ratsnest(&b),
        rules: gusset_pcb::rules::Rules::for_board(&b),
        board: b,
    }
}

#[test]
fn the_route_preview_shows_pushed_tracks() {
    let open = pushing_board();
    let mut st = pcb_view::State::default();
    st.track_from = Some([8.0, 6.0]);
    st.route_net = Some("A".into());
    // No way round, so the preview shows the push: the new track and B moved up.
    assert!(pcb_view::route_preview(&open, &mut st, [22.0, 6.0]).is_none());
    let p = pcb_view::push_preview(&open, &mut st, [22.0, 6.0]).expect("a push");
    assert_eq!((p.path[0], *p.path.last().unwrap()), ([8.0, 6.0], [22.0, 6.0]));
    assert_eq!(p.lines.len(), 1);
    assert_eq!(p.lines[0].2.as_deref(), Some("B"));
    assert!(p.lines[0].0.iter().any(|q| q[1] < 4.9), "{:?}", p.lines[0].0);
    // Cached while nothing changes; the click asks the router, which pushes the same way.
    assert_eq!(pcb_view::push_preview(&open, &mut st, [22.0, 6.0]), Some(p));
    let (id, args) = pcb_view::shove_command([8.0, 6.0], [22.0, 6.0], "F.Cu", Some("A"));
    assert!(gusset_commands::find(id).is_some());
    assert!(args.get("around").is_none() && args.get("shove").is_none());
    // Into C's pad: nothing to push.
    assert!(pcb_view::push_preview(&open, &mut st, [15.0, 6.325]).is_none());
}

/// Asks for the threaded push preview until it is ready (at most 60 s).
fn push_ready(
    open: &std::sync::Arc<gusset_commands::pcb_session::OpenBoard>,
    st: &mut pcb_view::State,
    to: [f64; 2],
) -> Option<gusset_pcb::push::Push> {
    let t = std::time::Instant::now();
    loop {
        if let pcb_view::Pending::Ready(r) = pcb_view::push_preview_async(open, st, to) {
            return r;
        }
        assert!(t.elapsed() < std::time::Duration::from_secs(60), "the preview never came");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn the_push_preview_runs_off_the_ui_thread() {
    let open = std::sync::Arc::new(pushing_board());
    let mut st = pcb_view::State::default();
    st.track_from = Some([8.0, 6.0]);
    st.route_net = Some("A".into());
    let mut sync = pcb_view::State::default();
    sync.track_from = st.track_from;
    sync.route_net = st.route_net.clone();
    // A new target starts a job and returns at once; the result is the push itself.
    assert_eq!(pcb_view::push_preview_async(&open, &mut st, [22.0, 6.0]), pcb_view::Pending::Waiting);
    let p = push_ready(&open, &mut st, [22.0, 6.0]).expect("a push");
    assert_eq!(Some(p.clone()), pcb_view::push_preview(&open, &mut sync, [22.0, 6.0]));
    assert_eq!(p.lines.len(), 1);
    // Kept for its target: no new job.
    let started = pcb_view::push_worker_started(&st);
    assert_eq!(pcb_view::push_preview_async(&open, &mut st, [22.0, 6.0]), pcb_view::Pending::Ready(Some(p)));
    assert_eq!(pcb_view::push_worker_started(&st), started);
    // The pointer moves on while a job runs: that job is cancelled, the new target's
    // result arrives and matches the push computed here.
    assert_eq!(pcb_view::push_preview_async(&open, &mut st, [20.0, 6.0]), pcb_view::Pending::Waiting);
    let none = push_ready(&open, &mut st, [15.0, 6.325]);
    assert_eq!(none, pcb_view::push_preview(&open, &mut sync, [15.0, 6.325]));
    assert!(none.is_none(), "into C's pad: nothing to push");
    assert_eq!(pcb_view::push_worker_started(&st), started + 2);
    let moved = push_ready(&open, &mut st, [20.0, 6.0]);
    assert_eq!(moved, pcb_view::push_preview(&open, &mut sync, [20.0, 6.0]));
}
