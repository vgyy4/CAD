#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
//! Neck-down near tight pads (ADR 0060).

use crate::board::{Board, Limits, Pt};
use crate::rules::{NetClass, Rules};
use crate::{drc, layers, route};

fn lim() -> Limits {
    Limits::default()
}

fn pad(r: &str, at: Pt, size: Pt, net: &str) -> String {
    format!(
        r#"(footprint "P" (layer "F.Cu") (at {} {})
    (property "Reference" "{r}" (at 0 -1.5) (layer "F.SilkS") (hide yes) (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd rect (at 0 0) (size {} {}) (layers "F.Cu") {net}))"#,
        at[0], at[1], size[0], size[1]
    )
}

/// A's pad at (10, 5), 0.3 × 1.6, in a row of pads `pitch` apart (B's on either side);
/// A's second pad at (25, 5), clear.
fn row(pitch: f64) -> Board {
    let parts = [
        pad("A1", [10.0, 5.0], [0.3, 1.6], "(net 1 \"A\")"),
        pad("B1", [10.0 - pitch, 5.0], [0.3, 1.6], "(net 2 \"B\")"),
        pad("B2", [10.0 + pitch, 5.0], [0.3, 1.6], "(net 2 \"B\")"),
        pad("A2", [25.0, 5.0], [0.6, 0.6], "(net 1 \"A\")"),
    ];
    let text = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (37 "F.SilkS" user) (44 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B")
  (gr_rect (start 0 0) (end 30 10) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  {}
)"#,
        parts.join("\n")
    );
    Board::read(&text, &lim()).unwrap()
}

const W: f64 = 0.4;
const FROM: Pt = [10.0, 5.0];
const TO: Pt = [25.0, 5.0];

fn f_cu() -> Vec<String> {
    vec!["F.Cu".into()]
}

fn laid(b: &Board, r: &layers::LayerRoute) -> Board {
    let mut nb = b.clone();
    let routes: Vec<crate::edit::NewRoute> = r
        .pieces()
        .into_iter()
        .map(|(l, w, pts)| {
            let segs = pts.windows(2).map(|x| route::Seg { start: x[0], end: x[1], mid: None }).collect();
            (segs, w, l, Some("A".to_string()))
        })
        .collect();
    nb.add_batch(&routes, &[], &lim()).unwrap();
    nb
}

fn necked(b: &Board, rules: &Rules) -> (layers::Layered, Result<layers::LayerRoute, &'static str>) {
    let nw = layers::neck_width(rules, Some("A"));
    let lay = layers::Layered::with_neck(b, rules, &f_cu(), Some("A"), W, nw);
    let radii = [layers::neck_radius(b, FROM, Some("A")), layers::neck_radius(b, TO, Some("A"))];
    let r = layers::route_necked(&lay, rules, FROM, &[0], TO, &[0], 0.1, true, None, radii);
    (lay, r)
}

#[test]
fn a_pad_in_a_tight_row_is_reached_only_by_necking_down() {
    let b = row(0.5);
    let rules = Rules::for_board(&b);
    // At 0.4 mm the pad's centre is 0.05 mm too close to its neighbours.
    let obs = route::Obstacles::new(&b, &rules, "F.Cu", Some("A"), W);
    assert!(route::walkaround(&obs, FROM, TO, 0.1, true).is_none());
    let full = layers::Layered::new(&b, &rules, &f_cu(), Some("A"), W);
    assert!(layers::route(&full, &rules, FROM, &[0], TO, &[0], 0.1, true, None).is_err());
    let radii = [layers::neck_radius(&b, FROM, Some("A")), layers::neck_radius(&b, TO, Some("A"))];
    assert_eq!(radii, [layers::NECK_CAP_MM, 1.2], "two pad sizes, capped");
    assert!(layers::route_necked(&full, &rules, FROM, &[0], TO, &[0], 0.1, true, None, radii).is_err());
    // Necked down to the board's minimum (0.15 mm), it routes; the check is unchanged.
    let (lay, r) = necked(&b, &rules);
    let r = r.unwrap();
    assert_eq!(lay.neck, 0.15);
    assert_eq!(r.narrowest(), 0.15);
    assert!(r.widths.iter().flatten().any(|&w| w == W), "back to full width: {r:?}");
    assert_eq!(r.legs[0].1[0], FROM);
    assert_eq!(*r.legs[0].1.last().unwrap(), TO);
    let nb = laid(&b, &r);
    assert_eq!(drc::check(&nb, &rules, 100), drc::check(&b, &rules, 100));
    assert!(crate::ratsnest::ratsnest(&nb).lines.iter().all(|l| l.net != "A"));
    // Deterministic.
    assert_eq!(necked(&b, &rules).1.unwrap(), r);
}

#[test]
fn necking_never_goes_below_the_class_minimum() {
    let b = row(0.5);
    let mut rules = Rules::for_board(&b);
    rules.classes.push(NetClass {
        name: "Fine".into(),
        min_track_width: Some(0.2),
        nets: vec!["A".into()],
        ..NetClass::default()
    });
    assert_eq!(layers::neck_width(&rules, Some("A")), 0.2);
    let (_, r) = necked(&b, &rules);
    let r = r.unwrap();
    assert_eq!(r.narrowest(), 0.2);
    let nb = laid(&b, &r);
    assert!(drc::check(&nb, &rules, 100).iter().all(|v| v.code != "track_width"));
    assert_eq!(drc::check(&nb, &rules, 100), drc::check(&b, &rules, 100));
    // A custom track_width rule raises it too.
    let mut strict = Rules::for_board(&b);
    strict.custom = crate::dru::CustomRules::parse(
        "(version 1) (rule \"w\" (condition \"A.NetName == 'A'\") (constraint track_width (min 0.18mm)))",
    );
    assert_eq!(layers::neck_width(&strict, Some("A")), 0.18);
}

#[test]
fn the_neck_stays_near_the_pad_and_widens_where_the_full_width_fits() {
    let b = row(0.5);
    let rules = Rules::for_board(&b);
    let (lay, r) = necked(&b, &rules);
    let r = r.unwrap();
    let full = layers::Layered::new(&b, &rules, &f_cu(), Some("A"), W);
    let pieces = r.pieces();
    // One narrow piece from the pad, then the full width: one transition.
    let ws: Vec<f64> = pieces.iter().map(|p| p.1).collect();
    assert_eq!(ws, [0.15, W], "{pieces:?}");
    let (narrow, wide) = (&pieces[0].2, &pieces[1].2);
    assert_eq!(narrow[0], FROM);
    // Joined at one point, where the full width is clear (its round end is the shoulder).
    let j = *narrow.last().unwrap();
    assert_eq!(wide[0], j);
    assert!(full.track(0).unwrap().clear(j, j), "{j:?}");
    // The neck stays within its radius of the pad (a grid step of slack), and is short.
    let reach = layers::NECK_CAP_MM + 0.1;
    assert!(narrow.iter().all(|p| libm::hypot(p[0] - FROM[0], p[1] - FROM[1]) <= reach), "{narrow:?}");
    let len: f64 = narrow.windows(2).map(|x| libm::hypot(x[1][0] - x[0][0], x[1][1] - x[0][1])).sum();
    assert!(len < 1.5, "{len}");
    // 45° segments throughout.
    for (_, _, pts) in &pieces {
        for x in pts.windows(2) {
            let (dx, dy) = ((x[1][0] - x[0][0]).abs(), (x[1][1] - x[0][1]).abs());
            assert!(dx < 1e-9 || dy < 1e-9 || (dx - dy).abs() < 1e-9, "{x:?}");
        }
    }
    let _ = lay;
}

#[test]
fn necking_is_refused_when_even_the_minimum_does_not_fit() {
    // 0.4 mm pitch: the centre is 0.25 mm from the neighbours' copper, below the 0.2 mm
    // clearance plus half of 0.15 mm.
    let b = row(0.4);
    let rules = Rules::for_board(&b);
    let (_, r) = necked(&b, &rules);
    assert_eq!(r.unwrap_err(), "the start is not clear on its layers");
    // Without a minimum below the width there is nothing to neck to.
    let mut wide = Rules::for_board(&b);
    wide.min_track_width = W;
    let (lay, r) = necked(&row(0.5), &wide);
    assert_eq!(lay.neck, W);
    assert!(r.is_err());
}
