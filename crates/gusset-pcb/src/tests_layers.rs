#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Routing across layers in one search (ADR 0059).

use crate::board::{Board, Limits, Pt};
use crate::rules::{NetClass, Rules};
use crate::{drc, layers, route};

fn lim() -> Limits {
    Limits::default()
}

fn pad(r: &str, at: Pt, layer: &str, net: &str) -> String {
    format!(
        r#"(footprint "P" (layer "{layer}") (at {} {})
    (property "Reference" "{r}" (at 0 -1.5) (layer "F.SilkS") (hide yes) (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd circle (at 0 0) (size 0.6 0.6) (layers "{layer}") {net}))"#,
        at[0], at[1]
    )
}

/// A's pads at (3, 5) and (27, 5) on F.Cu (or the second on `end_layer`); B's track
/// crosses the whole board at x = 15 on F.Cu, so A must dive under it. `extra` adds items.
fn walled(end_layer: &str, extra: &str) -> Board {
    let parts = [pad("A1", [3.0, 5.0], "F.Cu", "(net 1 \"A\")"), pad("A2", [27.0, 5.0], end_layer, "(net 1 \"A\")")];
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C")
  (gr_rect (start 0 0) (end 30 10) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  {}
  (segment (start 15 0) (end 15 10) (width 0.25) (layer "F.Cu") (net 2))
  {extra}
)"#,
        parts.join("\n")
    );
    Board::read(&text, &lim()).unwrap()
}

fn both() -> Vec<String> {
    vec!["F.Cu".into(), "B.Cu".into()]
}

/// The board with the route laid: tracks per leg, vias.
fn laid(b: &Board, r: &layers::LayerRoute, net: &str, w: f64, via: layers::ViaSize) -> Board {
    let mut nb = b.clone();
    let routes: Vec<crate::edit::NewRoute> = r
        .legs
        .iter()
        .map(|(l, pts)| {
            let segs = pts
                .windows(2)
                .filter(|x| x[0] != x[1])
                .map(|x| route::Seg { start: x[0], end: x[1], mid: None })
                .collect();
            (segs, w, l.clone(), Some(net.to_string()))
        })
        .collect();
    let vias: Vec<crate::edit::NewVia> =
        r.vias.iter().map(|&at| (at, via.size, via.drill, Some(net.to_string()))).collect();
    nb.add_batch(&routes, &vias, &lim()).unwrap();
    nb
}

#[test]
fn a_route_changes_layer_through_vias_to_pass_a_wall() {
    let b = walled("F.Cu", "");
    let rules = Rules::for_board(&b);
    // One layer: no way.
    let obs = route::Obstacles::new(&b, &rules, "F.Cu", Some("A"), 0.25);
    assert!(route::walkaround(&obs, [3.0, 5.0], [27.0, 5.0], 0.1, true).is_none());
    let lay = layers::Layered::new(&b, &rules, &both(), Some("A"), 0.25);
    let r = layers::route(&lay, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[0], 0.1, true, None).unwrap();
    // Down before the wall, up after it.
    let names: Vec<&str> = r.legs.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(names, ["F.Cu", "B.Cu", "F.Cu"]);
    assert_eq!(r.vias.len(), 2);
    assert!(r.vias[0][0] < 15.0 && r.vias[1][0] > 15.0, "{:?}", r.vias);
    assert_eq!(r.legs[0].1[0], [3.0, 5.0]);
    assert_eq!(*r.legs[2].1.last().unwrap(), [27.0, 5.0]);
    for k in 0..2 {
        assert_eq!(*r.legs[k].1.last().unwrap(), r.vias[k]);
        assert_eq!(r.legs[k + 1].1[0], r.vias[k]);
    }
    // Laid: the check finds nothing, and A is joined.
    let nb = laid(&b, &r, "A", 0.25, lay.via);
    assert_eq!(drc::check(&nb, &rules, 100), drc::check(&b, &rules, 100));
    assert!(crate::ratsnest::ratsnest(&nb).lines.iter().all(|l| l.net != "A"));
    // Deterministic.
    let again = layers::route(&lay, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[0], 0.1, true, None).unwrap();
    assert_eq!(again, r);
}

#[test]
fn an_end_on_the_other_layer_takes_one_via() {
    let b = walled("B.Cu", "");
    let rules = Rules::for_board(&b);
    let lay = layers::Layered::new(&b, &rules, &both(), Some("A"), 0.25);
    let r = layers::route(&lay, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[1], 0.1, true, None).unwrap();
    assert_eq!(r.vias.len(), 1);
    assert!(r.vias[0][0] < 15.0);
    assert_eq!(r.legs.last().unwrap().0, "B.Cu");
}

#[test]
fn vias_keep_their_clearance_on_both_layers() {
    // C's wide track on B.Cu runs along y = 5 left of the wall, so no via may sit near it
    // there (its copper on B.Cu), though F.Cu is clear; a D pad on F.Cu right of the wall
    // keeps vias off it there.
    let extra = r#"(segment (start 4.5 5) (end 14 5) (width 1.0) (layer "B.Cu") (net 3))
  (footprint "P" (layer "F.Cu") (at 20 5)
    (property "Reference" "D1" (at 0 -1.5) (layer "F.SilkS") (hide yes) (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd rect (at 0 0) (size 3 1.5) (layers "F.Cu") (net 3 "C")))"#;
    let b = walled("F.Cu", extra);
    let rules = Rules::for_board(&b);
    let lay = layers::Layered::new(&b, &rules, &both(), Some("A"), 0.25);
    assert!(!lay.via_fits([10.0, 5.6]), "on C's track on B.Cu");
    assert!(!lay.via_fits([20.0, 5.0]), "on D's pad on F.Cu");
    assert!(lay.via_fits([10.0, 8.0]));
    let r = layers::route(&lay, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[0], 0.1, true, None).unwrap();
    assert_eq!(r.vias.len(), 2);
    let nb = laid(&b, &r, "A", 0.25, lay.via);
    assert_eq!(drc::check(&nb, &rules, 100), drc::check(&b, &rules, 100));
    // Each via against the C track (B.Cu) and the D pad (F.Cu): the gap is kept.
    let need = rules.clearance + lay.via.size / 2.0;
    for at in &r.vias {
        let to_track = crate::copper::point_segment(*at, [4.5, 5.0], [14.0, 5.0]).0 - 0.5;
        assert!(to_track >= need - 1e-3, "{at:?} {to_track}");
        let dx = (at[0] - 20.0).abs() - 1.5;
        let dy = (at[1] - 5.0).abs() - 0.75;
        let to_pad = libm::hypot(dx.max(0.0), dy.max(0.0)).max(dx.min(0.0).max(dy.min(0.0)));
        assert!(to_pad >= lay.via.size / 2.0 + rules.clearance - 1e-3, "{at:?} {to_pad}");
    }
}

#[test]
fn vias_take_the_class_size_raised_to_the_minimums() {
    let b = walled("F.Cu", "");
    let mut rules = Rules::for_board(&b);
    let r = layers::via_size(&rules, Some("A"));
    assert_eq!((r.size, r.drill), (0.6, 0.3));
    rules.classes.push(NetClass {
        name: "Power".into(),
        via_diameter: Some(1.0),
        via_drill: Some(0.5),
        nets: vec!["A".into()],
        ..NetClass::default()
    });
    let lay = layers::Layered::new(&b, &rules, &both(), Some("A"), 0.25);
    assert_eq!((lay.via.size, lay.via.drill), (1.0, 0.5));
    let route = layers::route(&lay, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[0], 0.1, true, None).unwrap();
    let nb = laid(&b, &route, "A", 0.25, lay.via);
    assert!(nb.vias.iter().all(|v| (v.size, v.drill) == (1.0, 0.5)));
    assert_eq!(drc::check(&nb, &rules, 100), drc::check(&b, &rules, 100));
    // A drill under the minimum is raised, and the ring with it.
    rules.min_via_drill = 0.9;
    let v = layers::via_size(&rules, Some("A"));
    assert_eq!(v.drill, 0.9);
    assert!((v.size - 1.1).abs() < 1e-9, "{v:?}");
}

#[test]
fn vias_keep_the_hole_to_hole_gap() {
    // A THT pad of C at (10, 5): no via within hole-to-hole of its drill.
    let extra = r#"(footprint "P" (layer "F.Cu") (at 10 8.5)
    (property "Reference" "T1" (at 0 -1.5) (layer "F.SilkS") (hide yes) (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" thru_hole circle (at 0 0) (size 0.4 0.4) (drill 0.3) (layers "*.Cu") (net 1 "A")))"#;
    let b = walled("F.Cu", extra);
    let rules = Rules::for_board(&b);
    let lay = layers::Layered::new(&b, &rules, &both(), Some("A"), 0.25);
    // Own net: copper is no obstacle, the hole is.
    let edge = 0.15 + rules.hole_to_hole + lay.via.drill / 2.0;
    assert!(!lay.via_fits([10.0 + edge - 0.02, 8.5]));
    assert!(lay.via_fits([10.0 + edge + 0.02, 8.5]));
}

#[test]
fn a_route_is_refused_when_no_via_site_clears() {
    // A keep-out forbidding vias over the whole board.
    let extra = r#"(zone (net 0) (net_name "") (layers "F.Cu" "B.Cu") (hatch edge 0.5)
    (keepout (tracks allowed) (vias not_allowed) (pads allowed) (copperpour allowed) (footprints allowed))
    (polygon (pts (xy 0 0) (xy 30 0) (xy 30 10) (xy 0 10))))"#;
    let b = walled("F.Cu", extra);
    let rules = Rules::for_board(&b);
    let lay = layers::Layered::new(&b, &rules, &both(), Some("A"), 0.25);
    assert!(!lay.via_fits([10.0, 8.0]));
    let e = layers::route(&lay, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[0], 0.1, true, None).unwrap_err();
    assert_eq!(e, "no way across the layers");
    // One routable layer: no via to place.
    let one = layers::Layered::new(&walled("F.Cu", ""), &rules, &["F.Cu".to_string()], Some("A"), 0.25);
    assert!(layers::route(&one, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[0], 0.1, true, None).is_err());
    // Cancelled before it starts expanding.
    let stop = std::sync::atomic::AtomicBool::new(true);
    let lay = layers::Layered::new(&walled("F.Cu", ""), &rules, &both(), Some("A"), 0.25);
    let e = layers::route(&lay, &rules, [3.0, 5.0], &[0], [27.0, 5.0], &[0], 0.02, true, Some(&stop)).unwrap_err();
    assert_eq!(e, "cancelled");
}
