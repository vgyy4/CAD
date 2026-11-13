//! ADR 0054: per-class limits and `.kicad_dru` custom rules.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use crate::board::{Board, Limits};
use crate::copper::ItemRef;
use crate::drc::{self, Violation};
use crate::dru::CustomRules;
use crate::rules::{NetClass, Rules};

fn board(body: &str) -> Board {
    let src = format!(
        r#"(kicad_pcb (version 20240108) (generator "test")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (setup)
  (net 0 "") (net 1 "PWR") (net 2 "SIG") (net 3 "CLK1")
  {body}
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
)"#
    );
    Board::read(&src, &Limits::default()).unwrap()
}

fn seg(y: f64, w: f64, layer: &str, net: u32) -> String {
    format!(r#"(segment (start 10 {y}) (end 20 {y}) (width {w}) (layer "{layer}") (net {net}))"#)
}

fn via(x: f64, size: f64, drill: f64, net: u32) -> String {
    format!(r#"(via (at {x} 5) (size {size}) (drill {drill}) (layers "F.Cu" "B.Cu") (net {net}))"#)
}

fn power_class(f: impl FnOnce(&mut NetClass)) -> Rules {
    let mut c = NetClass { name: "Power".into(), nets: vec!["PWR".into()], ..NetClass::default() };
    f(&mut c);
    Rules { classes: vec![c], ..Rules::default() }
}

fn with_dru(text: &str) -> Rules {
    let custom = CustomRules::parse(text);
    assert!(custom.warnings.is_empty(), "{:?}", custom.warnings);
    let class = NetClass { name: "Power".into(), nets: vec!["PWR".into()], ..NetClass::default() };
    Rules { classes: vec![class], custom, ..Rules::default() }
}

fn only(b: &Board, rules: &Rules, code: &str) -> Vec<Violation> {
    drc::check(b, rules, 1000).into_iter().filter(|v| v.code == code).collect()
}

fn dru(rules: &str) -> String {
    format!("(version 1)\n{rules}")
}

#[test]
fn class_track_width_minimum() {
    let b = board(&format!("{} {} {}", seg(5.0, 0.3, "F.Cu", 1), seg(8.0, 0.3, "F.Cu", 2), seg(12.0, 0.5, "F.Cu", 1)));
    let rules = power_class(|c| c.min_track_width = Some(0.5));
    let v = only(&b, &rules, "track_width");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].items, [ItemRef::Track(0)]);
    assert!(v[0].message.contains("0.500 mm needed") && v[0].rule.is_none(), "{v:?}");
    // The class's `track_width` is a default for new tracks, not a limit.
    let default_only = power_class(|c| c.track_width = Some(0.5));
    assert!(only(&b, &default_only, "track_width").is_empty());
}

#[test]
fn class_via_sizes() {
    let b = board(&format!("{} {}", via(10.0, 0.6, 0.3, 1), via(20.0, 0.6, 0.3, 2)));
    let rules = power_class(|c| {
        c.min_via_diameter = Some(0.8);
        c.min_via_drill = Some(0.4);
    });
    let d = only(&b, &rules, "via_diameter");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].items, [ItemRef::Via(0)]);
    let h = only(&b, &rules, "drill_size");
    assert_eq!(h.len(), 1, "{h:?}");
    assert_eq!(h[0].items, [ItemRef::Via(0)]);
    // A class limit below the board's own does not loosen it.
    let loose = power_class(|c| c.min_track_width = Some(0.05));
    let b = board(&seg(5.0, 0.1, "F.Cu", 1));
    assert_eq!(only(&b, &loose, "track_width").len(), 1);
    assert_eq!(only(&b, &Rules::default(), "track_width").len(), 1);
}

#[test]
fn custom_clearance_by_net_class_names_the_rule() {
    // 0.3 mm gap between PWR and SIG tracks.
    let b = board(&format!("{} {}", seg(5.0, 0.25, "F.Cu", 1), seg(5.55, 0.25, "F.Cu", 2)));
    assert!(only(&b, &Rules::default(), "clearance").is_empty());
    let rules = with_dru(&dru(
        r#"(rule "power apart" (condition "A.NetClass == 'Power'") (constraint clearance (min 0.5mm)))"#,
    ));
    let v = only(&b, &rules, "clearance");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].rule.as_deref(), Some("power apart"));
    assert!(v[0].message.contains("power apart") && v[0].message.contains("0.500 mm needed"), "{v:?}");
    // Two signal nets: the condition matches neither.
    let c = board(&format!("{} {}", seg(5.0, 0.25, "F.Cu", 2), seg(5.55, 0.25, "F.Cu", 3)));
    assert!(only(&c, &rules, "clearance").is_empty());
    // A custom rule can also loosen the board's clearance.
    let tight = board(&format!("{} {}", seg(5.0, 0.25, "F.Cu", 2), seg(5.4, 0.25, "F.Cu", 3)));
    assert_eq!(only(&tight, &Rules::default(), "clearance").len(), 1);
    let loose =
        with_dru(&dru(r#"(rule "tight ok" (condition "A.NetName == 'SIG'") (constraint clearance (min 0.1)))"#));
    assert!(only(&tight, &loose, "clearance").is_empty());
}

#[test]
fn later_rules_override_earlier_ones() {
    let b = board(&format!("{} {}", seg(5.0, 0.25, "F.Cu", 1), seg(5.55, 0.25, "F.Cu", 2)));
    let wide = r#"(rule "wide" (constraint clearance (min 0.5)))"#;
    let narrow = r#"(rule "narrow" (condition "A.NetClass == 'Power'") (constraint clearance (min 0.1)))"#;
    // narrow last: it wins for the pair.
    let r = with_dru(&dru(&format!("{wide}\n{narrow}")));
    assert!(only(&b, &r, "clearance").is_empty());
    // wide last: it wins.
    let r = with_dru(&dru(&format!("{narrow}\n{wide}")));
    let v = only(&b, &r, "clearance");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule.as_deref(), Some("wide"));
}

#[test]
fn custom_track_width_and_via_diameter_by_type_and_layer() {
    let b = board(&format!("{} {} {}", seg(5.0, 0.25, "F.Cu", 2), seg(8.0, 0.25, "B.Cu", 2), via(30.0, 0.6, 0.3, 2)));
    let r = with_dru(&dru(
        r#"(rule "back wide" (condition "A.Type == 'track' && A.Layer == 'B.Cu'") (constraint track_width (min 0.4) (max 1)))
           (rule "big vias" (condition "A.Type == 'Via'") (constraint via_diameter (min 0.8)))"#,
    ));
    let w = only(&b, &r, "track_width");
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(w[0].layer.as_deref(), Some("B.Cu"));
    assert_eq!(w[0].rule.as_deref(), Some("back wide"));
    let v = only(&b, &r, "via_diameter");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].rule.as_deref(), Some("big vias"));
    // max
    let wide = board(&seg(5.0, 2.0, "F.Cu", 2));
    let r = with_dru(&dru(r#"(rule "thin" (constraint track_width (max 1.0mm)))"#));
    let w = only(&wide, &r, "track_width");
    assert_eq!(w.len(), 1);
    assert!(w[0].message.contains("at most 1.000 mm"), "{w:?}");
}

#[test]
fn boolean_operators_globs_and_units() {
    let b = board(&format!("{} {}", seg(5.0, 0.25, "F.Cu", 3), seg(8.0, 0.25, "F.Cu", 2)));
    let case = |cond: &str| -> usize {
        let text = format!(r#"(rule "r" (condition "{cond}") (constraint track_width (min 1mil) (max 0.1)))"#);
        only(&b, &with_dru(&dru(&text)), "track_width").len()
    };
    assert_eq!(case("A.NetName == 'CLK*'"), 1);
    assert_eq!(case("A.NetName != 'CLK*'"), 1);
    assert_eq!(case("!(A.NetName == 'CLK*')"), 1);
    assert_eq!(case("A.NetName == 'CLK?' || A.NetName == 'SIG'"), 2);
    assert_eq!(case("(A.NetName == 'CLK1' || A.NetName == 'SIG') && A.Layer == 'B.Cu'"), 0);
    assert_eq!(case("A.NetName == 'SIG' && A.NetClass == 'Default'"), 1);
    assert_eq!(case("A.NetClass == A.NetName"), 0);
    assert_eq!(case("A.Type == 'pad'"), 0);
}

#[test]
fn hole_rules_and_edge_clearance() {
    let b = board(&format!("{} {} {}", via(10.0, 0.6, 0.3, 2), via(10.5, 0.6, 0.3, 2), via(39.0, 0.6, 0.3, 2)));
    // The pair is 0.2 mm apart: a rule lowering the limit removes the report, one raising it keeps it.
    let low = with_dru(&dru(r#"(rule "h2h" (constraint hole_to_hole (min 0.1)))"#));
    assert!(only(&b, &low, "hole_to_hole").is_empty());
    let high = with_dru(&dru(
        r#"(rule "h2h" (condition "A.Type == 'via' && B.Type == 'via'") (constraint hole_to_hole (min 0.5)))"#,
    ));
    let v = only(&b, &high, "hole_to_hole");
    assert!(!v.is_empty() && v.iter().all(|v| v.rule.as_deref() == Some("h2h")), "{v:?}");
    // hole_size
    let r = with_dru(&dru(r#"(rule "drills" (condition "A.Type == 'via'") (constraint hole_size (min 0.35)))"#));
    let v = only(&b, &r, "drill_size");
    assert_eq!(v.len(), 3);
    assert!(v[0].message.contains("drills"));
    // edge_clearance: the third via is 0.7 mm from the edge.
    let r = with_dru(&dru(r#"(rule "edge" (constraint edge_clearance (min 0.05)))"#));
    assert!(only(&b, &r, "copper_to_edge").is_empty());
    let r = with_dru(&dru(r#"(rule "edge" (constraint edge_clearance (min 2)))"#));
    let v = only(&b, &r, "copper_to_edge");
    assert!(v.iter().any(|v| v.rule.as_deref() == Some("edge")), "{v:?}");
}

#[test]
fn unsupported_rules_are_skipped_with_a_warning() {
    let text = dru(r#"
# a comment
(rule "area" (condition "A.insideArea('Zone1')") (constraint clearance (min 1)))
(rule "skew" (constraint skew (max 1)))
(rule "b track" (condition "B.NetClass == 'x'") (constraint track_width (min 1)))
(rule "silk" (layer "F.SilkS") (constraint clearance (min 1)))
(rule "ignored" (severity ignore) (constraint clearance (min 1)))
(rule "prop" (condition "A.Name == 'x'") (constraint clearance (min 1)))
(rule "layer cond" (condition "A.Layer == 'outer'") (constraint clearance (min 1)))
(rule "unit" (constraint clearance (min 1furlong)))
(rule "max only" (constraint clearance (max 1)))
(rule "good" (condition "A.NetClass == 'Power'") (constraint clearance (min 0.4)))
(disallow track)
"#);
    let c = CustomRules::parse(&text);
    assert_eq!(c.rules.len(), 1, "{c:?}");
    assert_eq!(c.rules[0].name, "good");
    assert_eq!(c.warnings.len(), 10, "{:#?}", c.warnings);
    assert!(c.warnings.iter().any(|w| w.contains("\"area\"") && w.contains("insideArea")));
    assert!(c.warnings.iter().any(|w| w.contains("\"skew\"")));
    assert!(c.warnings.iter().any(|w| w.contains("disallow")));
    // The skipped rules change nothing.
    let b = board(&format!("{} {}", seg(5.0, 0.25, "F.Cu", 2), seg(5.4, 0.25, "F.Cu", 3)));
    let rules = Rules { custom: c, ..Rules::default() };
    let v = only(&b, &rules, "clearance");
    assert_eq!(v.len(), 1);
    assert!(v[0].rule.is_none());
}

#[test]
fn version_and_hostile_files() {
    assert!(CustomRules::parse("(rule \"x\" (constraint clearance (min 1)))").rules.is_empty());
    assert!(CustomRules::parse("(version 2)").warnings[0].contains("version 1"));
    assert!(CustomRules::parse("(version 1) (rule").warnings[0].contains("does not read"));
    assert!(CustomRules::parse(&"x".repeat(crate::dru::MAX_BYTES + 1)).rules.is_empty());
    // Deep nesting, in the condition and in the file.
    let deep = format!("({}A.Type == 'via'{})", "(".repeat(100_000), ")".repeat(100_000));
    let c = CustomRules::parse(&dru(&format!(r#"(rule "d" (condition "{deep}") (constraint clearance (min 1)))"#)));
    assert!(c.rules.is_empty() && !c.warnings.is_empty());
    let nots = "!".repeat(3000);
    let text = format!(r#"(rule "d" (condition "{nots}A.Type == 'via'") (constraint clearance (min 1)))"#);
    let c = CustomRules::parse(&dru(&text));
    assert!(c.rules.is_empty() && c.warnings[0].contains("deeply"), "{c:?}");
    let chain = vec!["A.Type == 'via'"; 500].join(" && ");
    let text = format!(r#"(rule "d" (condition "{chain}") (constraint clearance (min 1)))"#);
    let c = CustomRules::parse(&dru(&text));
    assert!(c.rules.is_empty() && (c.warnings[0].contains("too many") || c.warnings[0].contains("too long")), "{c:?}");
    let c = CustomRules::parse(&format!("(version 1){}", "(".repeat(100_000)));
    assert!(c.rules.is_empty());
    // Many rules: kept up to the cap.
    let many = dru(&r#"(rule "r" (constraint clearance (min 0.1)))"#.repeat(crate::dru::MAX_RULES + 50));
    let c = CustomRules::parse(&many);
    assert_eq!(c.rules.len(), crate::dru::MAX_RULES);
    assert!(c.warnings.iter().any(|w| w.contains("rest were left out")));
    // Odd values.
    let c = CustomRules::parse(&dru(
        "(rule \"a\u{7}b\" (constraint clearance (min nan)))(rule (constraint clearance (min -1)))",
    ));
    assert!(c.rules.is_empty());
}
