//! Gusset's document model (ADR 0004) and its canonical text form (the code view).
//!
//! ```
//! let text = "gusset 1\nunits mm\n\nparam width = 40 mm\n";
//! let doc = gusset_doc::Document::from_text(text).unwrap();
//! assert_eq!(doc.to_text(), text);
//! ```

#![cfg_attr(test, allow(clippy::float_cmp))]

mod cursor;
pub mod model;
pub mod refs;
pub mod text;
pub mod validate;

/// The code-view text grammar for agents (published as the MCP resource
/// `gusset://docs/text-grammar`). Every ```gusset block in it is a complete document that
/// parses and validates (checked by a test).
pub const TEXT_GRAMMAR: &str = include_str!("grammar.md");

pub use cursor::ParseError;
pub use model::*;
pub use refs::{CapSide, EdgeRef, FaceKey, FaceRef, HolePart};
pub use validate::Issue;

/// Why text could not be turned into a valid document.
#[derive(Debug, Clone, PartialEq)]
pub enum DocError {
    Parse(ParseError),
    Invalid(Vec<Issue>),
}

impl std::fmt::Display for DocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocError::Parse(e) => write!(f, "{e}"),
            DocError::Invalid(issues) => {
                for (i, is) in issues.iter().enumerate() {
                    if i > 0 {
                        f.write_str("; ")?;
                    }
                    write!(f, "{is}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for DocError {}

impl Document {
    /// Parses and validates document text.
    pub fn from_text(text: &str) -> Result<Document, DocError> {
        let doc = text::parse(text).map_err(DocError::Parse)?;
        let issues = validate::validate(&doc);
        if issues.is_empty() { Ok(doc) } else { Err(DocError::Invalid(issues)) }
    }

    /// Canonical text (deterministic).
    pub fn to_text(&self) -> String {
        text::print(self)
    }

    pub fn feature(&self, name: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.name == name)
    }

    pub fn feature_index(&self, name: &str) -> Option<usize> {
        self.features.iter().position(|f| f.name == name)
    }

    /// A fresh feature name `<Base><N>` not used yet (`Extrude3`).
    pub fn unique_feature_name(&self, base: &str) -> String {
        (1..).map(|i| format!("{base}{i}")).find(|n| self.feature(n).is_none()).unwrap_or_else(|| base.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"gusset 1
units mm

param width = 40 mm  # overall width
# depth is derived
# from the width
param depth = width / 2

# Base plate outline
sketch Sketch1 (plane: XY, next_id: 12, label: "Base \"plate\"") {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
  k9 = horizontal(l5)
  k10 = distance(p1, p2, width)
  k11 = fixed(p1)
}

extrude Extrude1 (profile: Sketch1, distance: depth, op: new)

sketch Sketch2 (plane: Extrude1/cap(end), next_id: 4) {
  p1 = point(20, 10)
  c2 = circle(p1, 5)
  p3 = point(0.1, -2.5e-7) construction
}

extrude Extrude2 (profile: Sketch2.regions(c2), distance: -5 mm, op: cut, body: Extrude1)

revolve Revolve1 (profile: Sketch2, axis: X, angle: 180 deg, op: new, body_name: Ring, suppressed: true)

fillet Fillet1 (edges: [edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5)), edge(Extrude1/cap(end), Extrude1/side(Sketch1/l6))#2], radius: 2 mm)

chamfer Chamfer1 (edges: [edge(Extrude1/cap(start), Extrude1/side(Sketch1/l7))], distance: 0.5 mm)

import Import1 (blob: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", format: step, file: "bracket.step")
"#;

    #[test]
    fn canonical_roundtrip() {
        let doc = Document::from_text(SAMPLE).unwrap_or_else(|e| panic!("{e}"));
        let printed = doc.to_text();
        assert_eq!(printed, SAMPLE);
        assert_eq!(Document::from_text(&printed).unwrap(), doc);
        let f = doc.feature("Sketch1").unwrap();
        assert_eq!(f.label.as_deref(), Some("Base \"plate\""));
        assert_eq!(f.note.as_deref(), Some("Base plate outline"));
        assert_eq!(doc.params[0].comment, "overall width");
        assert_eq!(doc.params[1].comment, "depth is derived\nfrom the width");
        assert!(doc.feature("Revolve1").unwrap().suppressed);
        assert!(
            matches!(&doc.feature("Revolve1").unwrap().kind, FeatureKind::Revolve(r) if r.body_name.as_deref() == Some("Ring"))
        );
    }

    #[test]
    fn grammar_examples_parse_and_cover_the_language() {
        let mut blocks = Vec::new();
        let mut cur: Option<String> = None;
        for line in TEXT_GRAMMAR.lines() {
            match (&mut cur, line.trim_end()) {
                (None, "```gusset") => cur = Some(String::new()),
                (Some(_), "```") => blocks.extend(cur.take()),
                (Some(b), l) => {
                    b.push_str(l);
                    b.push('\n');
                }
                (None, _) => {}
            }
        }
        assert!(blocks.len() >= 3, "{} examples", blocks.len());
        for b in &blocks {
            let doc = Document::from_text(b).unwrap_or_else(|e| panic!("{e}\n{b}"));
            // Canonical printing reads back to the same document.
            assert_eq!(Document::from_text(&doc.to_text()).unwrap(), doc);
        }
        // Every constraint kind, feature keyword and entity id prefix is documented.
        for k in SkConstraintKind::ALL {
            assert!(TEXT_GRAMMAR.contains(&format!("`{}`", k.keyword())), "{}", k.keyword());
        }
        for kw in [
            "extrude",
            "revolve",
            "fillet",
            "chamfer",
            "import",
            "body_name",
            "construction",
            "next_id",
            "plane",
            "move",
            "combine",
            "sweep",
            "loft",
            "orientation",
            "twist",
            "ruled",
            "curves(",
            "hole",
            "point_angle",
            "thread_depth",
        ] {
            assert!(TEXT_GRAMMAR.contains(kw), "{kw}");
        }
        for t in HoleType::ALL {
            assert!(TEXT_GRAMMAR.contains(t.keyword()), "{}", t.keyword());
        }
        for p in HolePart::ALL.into_iter().filter(|p| *p != HolePart::Top) {
            assert!(TEXT_GRAMMAR.contains(&format!("{}(", p.keyword())), "{}", p.keyword());
        }
        // The examples between them use these statement kinds.
        let all = blocks.join("\n");
        for kw in [
            "extrude ",
            "revolve ",
            "fillet ",
            "chamfer ",
            "angle(",
            "circle(",
            "op: cut",
            "body_name:",
            "plane Plane1 ",
            "move Move1 ",
            "combine Combine1 ",
            "pattern_linear ",
            "spline(",
            "pattern_linear(of:",
            "sweep Sweep1 ",
            "loft Loft1 ",
            "twist: 90 deg",
            "ruled: true",
            "hole Hole1 ",
            "type: tapped",
            "type: countersink",
            "through: true",
            "at: [[",
        ] {
            assert!(all.contains(kw), "no example uses {kw}");
        }
    }

    const HOLE_BASE: &str = "gusset 1\nunits mm\n\nparam d = 6.6 mm\nparam deep = 12 mm\n\nsketch P (plane: XY, next_id: 6) {\n  p1 = point(0, 0)\n  p2 = point(30, 0)\n  p3 = point(30, 20)\n  l4 = line(p1, p2)\n  l5 = line(p2, p3)\n}\n\nextrude E (profile: P, distance: deep, op: new)\n\nsketch S (plane: E/cap(end), next_id: 3) {\n  p1 = point(5, 5)\n  c2 = circle(p1, 2)\n}\n\n";

    #[test]
    fn hole_round_trips_and_is_validated() {
        let ok = format!(
            "{HOLE_BASE}hole H1 (face: E/cap(end), points: S, type: counterbore, diameter: d, through: true, cbore_diameter: d * 1.7, cbore_depth: 6 mm)\n\nhole H2 (face: E/cap(end), at: [[10 mm, 5 mm], [2 * d, 4 mm]], type: tapped, diameter: 5 mm, depth: deep, point_angle: 120 deg, thread: \"M6\", pitch: 1 mm, thread_depth: 8 mm)\n\nhole H3 (face: H1/cbore_floor(p1), at: [[0 mm, 0 mm]], type: countersink, diameter: 3 mm, depth: 5 mm, csink_diameter: 6 mm, csink_angle: 90 deg)\n\nhole H4 (face: E/side(P/l5)#2, at: [], type: simple, diameter: 1 mm, depth: 2 mm)\n"
        );
        // The last statement is invalid (no points) but must print and parse.
        let doc = text::parse(&ok).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(text::print(&doc), ok);
        let FeatureKind::Hole(h) = &doc.feature("H2").unwrap().kind else { panic!() };
        assert_eq!(h.kind, HoleType::Tapped);
        assert_eq!(h.thread.as_ref().map(|t| t.designation.as_str()), Some("M6"));
        assert!(matches!(&h.sites, HoleSites::Points(p) if p.len() == 2));
        assert_eq!(doc.feature("H2").unwrap().kind.parameters(), ["d", "deep"]);
        assert_eq!(doc.feature("H1").unwrap().kind.dependencies(), ["E", "S"]);
        assert_eq!(doc.feature("H3").unwrap().kind.dependencies(), ["H1"]);
        assert!(!validate::creates_body(doc.feature("H1").unwrap()));
        // Defaults: type simple.
        let short = format!("{HOLE_BASE}hole H1 (face: E/cap(end), points: S, diameter: d, depth: 5 mm)\n");
        let d = Document::from_text(&short).unwrap_or_else(|e| panic!("{e}"));
        assert!(d.to_text().contains("type: simple, diameter: d, depth: 5 mm"));
        // Every truncation and single-character deletion parses to a fixed point or fails cleanly.
        let chars: Vec<char> = ok.chars().collect();
        for i in 0..chars.len() {
            for src in [chars[..i].iter().collect::<String>(), chars[..i].iter().chain(&chars[i + 1..]).collect()] {
                if let Ok(d) = text::parse(&src) {
                    let printed = text::print(&d);
                    let again = text::parse(&printed).unwrap_or_else(|e| panic!("{src:?}: {e}"));
                    assert_eq!(text::print(&again), printed, "{src:?}");
                }
            }
        }
        for (bad, why) in [
            ("hole H (face: E/cap(end), at: [], diameter: 1 mm, depth: 2 mm)", "no points"),
            ("hole H (face: E/cap(end), points: E, diameter: 1 mm, depth: 2 mm)", "is not a sketch"),
            (
                "hole H (face: E/cap(end), points: S, type: counterbore, diameter: 1 mm, depth: 2 mm)",
                "needs cbore_diameter",
            ),
            (
                "hole H (face: E/cap(end), points: S, type: tapped, diameter: 1 mm, depth: 2 mm)",
                "needs thread and pitch",
            ),
            (
                "hole H (face: E/cap(end), points: S, type: simple, diameter: 1 mm, depth: 2 mm, thread: \"M3\", pitch: 1 mm)",
                "only apply to type: tapped",
            ),
            ("hole H (face: E/cap(end), points: S, diameter: 1 mm, through: true, point_angle: 90 deg)", "blind"),
            (
                "hole H (face: E/cap(end), points: S, type: tapped, diameter: 1 mm, depth: 2 mm, thread: \"\", pitch: 1 mm)",
                "printable",
            ),
            ("hole H (face: E/cap(end), points: S, diameter: nope, depth: 2 mm)", "unknown parameter"),
            ("hole H (face: Later/cap(end), points: S, diameter: 1 mm, depth: 2 mm)", "does not exist"),
        ] {
            let d = text::parse(&format!("{HOLE_BASE}{bad}\n")).unwrap_or_else(|e| panic!("{bad}: {e}"));
            let issues = validate::validate(&d);
            assert!(issues.iter().any(|i| i.to_string().contains(why)), "{bad}: {why}: {issues:?}");
        }
        for bad in [
            "hole H (points: S, diameter: 1 mm, depth: 2 mm)",
            "hole H (face: E/cap(end), diameter: 1 mm, depth: 2 mm)",
            "hole H (face: E/cap(end), points: S, depth: 2 mm)",
            "hole H (face: E/cap(end), points: S, diameter: 1 mm)",
            "hole H (face: E/cap(end), points: S, at: [], diameter: 1 mm, depth: 2 mm)",
            "hole H (face: E/cap(end), points: S, diameter: 1 mm, depth: 2 mm, through: true)",
            "hole H (face: E/cap(end), points: S, diameter: 1 mm, depth: 2 mm, type: drilled)",
            "hole H (face: E/cap(end), points: S, diameter: 1 mm, depth: 2 mm, cbore_diameter: 3 mm)",
            "hole H (face: E/cap(end), points: S, diameter: 1 mm, depth: 2 mm, thread: \"M3\")",
            "hole H (face: E/cap(end), points: S, diameter: 1 mm, depth: 2 mm, thread_depth: 1 mm)",
            "hole H (face: E/cap(end), at: [[1 mm]], diameter: 1 mm, depth: 2 mm)",
            "hole H (face: E/cap(end), points: S, diameter: 1 mm, depth: 2 mm, speed: 3)",
        ] {
            assert!(text::parse(&format!("{HOLE_BASE}{bad}\n")).is_err(), "{bad}");
        }
    }

    const SWEEP_BASE: &str = "gusset 1\nunits mm\n\nparam turn = 30 deg\n\nsketch P (plane: XY, next_id: 3) {\n  p1 = point(0, 0)\n  c2 = circle(p1, 3)\n}\n\nsketch Q (plane: XZ, next_id: 7) {\n  p1 = point(0, 0)\n  p2 = point(0, 9)\n  p3 = point(5, 9)\n  l4 = line(p1, p2)\n  l5 = line(p2, p3)\n}\n\n";

    #[test]
    fn sweep_and_loft_round_trip_and_are_validated() {
        let ok = format!(
            "{SWEEP_BASE}sweep S1 (profile: P.regions(c2), path: Q.curves(l4, l5), orientation: frenet, twist: turn, op: new, body_name: Pipe)\n\nsweep S2 (profile: P, path: Q, orientation: fixed, op: add, body: S1)\n\nsweep S3 (profile: P, path: [edge(S1/cap(end), S1/side(P/c2)), edge(S1/cap(end), S1/side(P/c2_p1))], orientation: frenet, op: add, body: S1)\n\nloft L1 (profiles: [P, P.regions(c2)], ruled: true, op: new)\n"
        );
        let doc = Document::from_text(&ok).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.to_text(), ok);
        let FeatureKind::Sweep(d) = &doc.feature("S1").unwrap().kind else { panic!() };
        assert_eq!(d.path, PathRef::Sketch { sketch: "Q".into(), curves: vec!["l4".into(), "l5".into()] });
        assert_eq!(d.orientation, Orientation::Frenet);
        assert_eq!(d.body_name.as_deref(), Some("Pipe"));
        assert_eq!(doc.feature("S1").unwrap().kind.parameters(), ["turn"]);
        assert_eq!(doc.feature("S1").unwrap().kind.dependencies(), ["P", "Q"]);
        assert!(
            validate::creates_body(doc.feature("S1").unwrap()) && !validate::creates_body(doc.feature("S2").unwrap())
        );
        // Every truncation and single-character deletion parses to a fixed point or fails cleanly.
        let chars: Vec<char> = ok.chars().collect();
        for i in 0..chars.len() {
            for src in [chars[..i].iter().collect::<String>(), chars[..i].iter().chain(&chars[i + 1..]).collect()] {
                if let Ok(d) = text::parse(&src) {
                    let printed = text::print(&d);
                    let again = text::parse(&printed).unwrap_or_else(|e| panic!("{src:?}: {e}"));
                    assert_eq!(text::print(&again), printed, "{src:?}");
                }
            }
        }
        // Defaults: orientation frenet, op new.
        let short = format!("{SWEEP_BASE}sweep S1 (profile: P, path: Q)\n");
        let d = Document::from_text(&short).unwrap();
        assert_eq!(d.to_text(), format!("{SWEEP_BASE}sweep S1 (profile: P, path: Q, orientation: frenet, op: new)\n"));
        // Problems are reported with their feature.
        for (bad, why) in [
            ("sweep S1 (profile: P, path: P, op: new)", "same sketch"),
            ("sweep S1 (profile: P, path: Q.curves(l9), op: new)", "no entity 'l9'"),
            ("sweep S1 (profile: P, path: Q.curves(l4, l4), op: new)", "listed twice"),
            ("sweep S1 (profile: P, path: Q, orientation: fixed, twist: 10 deg, op: new)", "follow the path"),
            ("sweep S1 (profile: P, path: Q, op: cut)", "needs an existing body"),
            ("sweep S1 (profile: P, path: Nope, op: new)", "does not exist"),
            ("sweep S1 (profile: P, path: [], op: new)", "no edges"),
            ("sweep S1 (profile: P, path: Q, body_name: B, op: add)", "only be used with op: new"),
            ("loft L1 (profiles: [P], op: new)", "at least two"),
            ("loft L1 (profiles: [P, Q], op: cut)", "needs an existing body"),
        ] {
            let d = text::parse(&format!("{SWEEP_BASE}{bad}\n")).unwrap_or_else(|e| panic!("{bad}: {e}"));
            let issues = validate::validate(&d);
            assert!(issues.iter().any(|i| i.to_string().contains(why)), "{bad}: {why}: {issues:?}");
        }
        for bad in [
            "sweep S1 (profile: P, op: new)",
            "sweep S1 (path: Q, op: new)",
            "sweep S1 (profile: P, path: Q, orientation: sideways)",
            "sweep S1 (profile: P, path: Q.wires(l4))",
            "loft L1 (profile: P)",
            "loft L1 (profiles: P)",
            "loft L1 (profiles: [P, ])",
            "loft L1 (profiles: [P, P], twist: 1 deg)",
        ] {
            assert!(text::parse(&format!("{SWEEP_BASE}{bad}\n")).is_err(), "{bad}");
        }
    }

    #[test]
    fn body_names_round_trip_and_are_validated() {
        let base = "gusset 1\nunits mm\n\nsketch S (plane: XY, next_id: 1) {\n}\n\n";
        let ok = format!("{base}extrude A (profile: S, distance: 1 mm, op: new, body_name: Bracket)\n");
        let doc = Document::from_text(&ok).unwrap();
        assert_eq!(doc.to_text(), ok);
        // Old files without body_name stay valid and print unchanged.
        let old = format!("{base}extrude A (profile: S, distance: 1 mm, op: new)\n");
        assert_eq!(Document::from_text(&old).unwrap().to_text(), old);
        let bad = |extra: &str| Document::from_text(&format!("{base}{extra}")).unwrap_err().to_string();
        // Duplicate between an explicit name and another feature's default body name.
        let e = bad(
            "extrude A (profile: S, distance: 1 mm, op: new, body_name: B)\nextrude B (profile: S, distance: 1 mm, op: new)\n",
        );
        assert!(e.contains("body name 'B' is used twice"), "{e}");
        let e = bad(
            "extrude A (profile: S, distance: 1 mm, op: new)\nextrude B (profile: S, distance: 1 mm, op: cut, body_name: C)\n",
        );
        assert!(e.contains("only be used with op: new"), "{e}");
        assert!(
            Document::from_text(&format!("{base}extrude A (profile: S, distance: 1 mm, body_name: 9x)\n")).is_err()
        );
        let e = bad("revolve R (profile: S, axis: X, angle: 90 deg, body_name: R2, body_name: R3)\n");
        assert!(e.contains("given twice"), "{e}");
    }

    #[test]
    fn noncanonical_input_normalises() {
        let messy = "gusset 1\r\nunits in\n\n\n   param   a=1+2   \nsketch S (next_id: 1,plane:XZ) {\n\n}\n";
        let doc = Document::from_text(messy).unwrap();
        assert_eq!(doc.to_text(), "gusset 1\nunits in\n\nparam a = 1 + 2\n\nsketch S (plane: XZ, next_id: 1) {\n}\n");
    }

    #[test]
    fn splines_round_trip_and_are_validated() {
        let ok = "gusset 1\nunits mm\n\nsketch S (plane: XY, next_id: 8) {\n  p1 = point(0, 0)\n  p2 = point(5, 3)\n  p3 = point(9, 0)\n  s4 = spline(p1, p2, p3)\n  s5 = spline(p1, p2, p3) closed construction\n}\n";
        let doc = Document::from_text(ok).unwrap();
        assert_eq!(doc.to_text(), ok);
        let FeatureKind::Sketch(s) = &doc.features[0].kind else { panic!() };
        assert_eq!(
            s.entities[3].geom,
            SkGeom::Spline { points: vec!["p1".into(), "p2".into(), "p3".into()], closed: false }
        );
        assert!(s.entities[4].construction);
        // Too few points, a repeated point, an unknown point and a bad id are reported.
        for (bad, why) in [
            (ok.replace("s4 = spline(p1, p2, p3)\n", "s4 = spline(p1, p2)\n"), "at least 3"),
            (ok.replace("s4 = spline(p1, p2, p3)\n", "s4 = spline(p1, p2, p2)\n"), "twice"),
            (ok.replace("s4 = spline(p1, p2, p3)\n", "s4 = spline(p1, p2, p9)\n"), "p9"),
            (ok.replace("s4 = spline(p1, p2, p3)\n", "l4 = spline(p1, p2, p3)\n"), "l4"),
        ] {
            let d = text::parse(&bad).unwrap();
            let issues = validate::validate(&d);
            assert!(issues.iter().any(|i| i.to_string().contains(why)), "{why}: {issues:?}");
        }
    }

    /// Review v0.2 M1: the profile sketch of a generated gear names its gear's parameter
    /// prefix; the gear's inputs count as parameters the sketch uses.
    #[test]
    fn gear_sketches_name_their_parameters() {
        let params = ["module", "teeth", "pressure_angle", "shift", "backlash", "fillet", "bore", "transverse_module"];
        let mut ok = String::from("gusset 1\nunits mm\n\n");
        for p in params {
            ok.push_str(&format!("param g1_{p} = 1\n"));
        }
        ok.push_str("\nsketch S (plane: XY, next_id: 2, gear: g1, label: \"teeth\") {\n  p1 = point(0, 0)\n}\n");
        let doc = Document::from_text(&ok).unwrap();
        assert_eq!(doc.to_text(), ok);
        let FeatureKind::Sketch(s) = &doc.features[0].kind else { panic!() };
        assert_eq!(s.gear.as_deref(), Some("g1"));
        let want: Vec<String> = params.iter().map(|p| format!("g1_{p}")).collect();
        assert_eq!(doc.features[0].kind.parameters(), want);
        let e = Document::from_text(&ok.replace("param g1_shift = 1\n", "")).unwrap_err().to_string();
        assert!(e.contains("unknown parameter 'g1_shift'"), "{e}");
        let e = Document::from_text(&ok.replace("gear: g1", "gear: 1g")).unwrap_err().to_string();
        assert!(!e.is_empty());
        // Without the key nothing extra is used.
        let plain = Document::from_text(&ok.replace(", gear: g1", "")).unwrap();
        assert!(plain.features[0].kind.parameters().is_empty());
    }

    #[test]
    fn sketch_patterns_round_trip_and_are_validated() {
        let ok = "gusset 1\nunits mm\n\nparam n = 3\n\nsketch S (plane: XY, next_id: 9) {\n  p1 = point(0, 0)\n  p2 = point(5, 0)\n  l3 = line(p1, p2)\n  p4 = point(1, 1)\n  c5 = circle(p4, 1)\n  g6 = pattern_linear(of: [l3], count: n, dx: 10 mm, dy: 2 mm)\n  g7 = pattern_circular(of: [c5], center: p1, count: 6, angle: 360 deg)\n}\n\nextrude E (profile: S.regions(c5_2), distance: 1 mm, op: new)\n";
        let doc = Document::from_text(ok).unwrap();
        assert_eq!(doc.to_text(), ok);
        let FeatureKind::Sketch(s) = &doc.features[0].kind else { panic!() };
        assert_eq!(s.patterns.len(), 2);
        assert_eq!(doc.features[0].kind.parameters(), vec!["n".to_string()]);
        for (bad, why) in [
            (ok.replace("of: [l3], count: n", "of: [], count: n"), "no source"),
            (ok.replace("of: [l3], count: n", "of: [p1], count: n"), "not a line"),
            (ok.replace("of: [l3], count: n", "of: [l9], count: n"), "unknown entity"),
            (ok.replace("of: [c5], center: p1", "of: [l3], center: p1"), "two patterns"),
            (ok.replace("center: p1", "center: l3"), "centre"),
            (ok.replace("g7 = ", "g6 = "), "twice"),
            (ok.replace("g7 = ", "k7 = "), "k7"),
            (ok.replace("regions(c5_2)", "regions(c9_2)"), "c9"),
            (ok.replace("count: n, dx", "count: m, dx"), "unknown parameter"),
        ] {
            let d = text::parse(&bad).unwrap();
            let issues = validate::validate(&d);
            assert!(issues.iter().any(|i| i.to_string().contains(why)), "{why}: {issues:?}");
        }
        // Syntax errors have positions and hints.
        let e = text::parse(&ok.replace("dx: 10 mm, dy: 2 mm", "dx: 10 mm")).unwrap_err();
        assert!(e.message.contains("dy"), "{e:?}");
        let e = text::parse(&ok.replace("count: 6, angle", "count: 6, dz: 1, angle")).unwrap_err();
        assert!(e.message.contains("dz"), "{e:?}");
    }

    #[test]
    fn next_id_is_optional_in_hand_written_text() {
        let src = "gusset 1\nsketch S (plane: XY) {\n  p1 = point(0, 0)\n  p2 = point(5, 0)\n  l7 = line(p1, p2)\n  k9 = horizontal(l7)\n}\nsketch E (plane: XZ) {\n}\n";
        let doc = Document::from_text(src).unwrap();
        let printed = doc.to_text();
        // Derived as max id + 1 (k9 -> 10; empty -> 1) and printed canonically.
        assert!(printed.contains("sketch S (plane: XY, next_id: 10) {"), "{printed}");
        assert!(printed.contains("sketch E (plane: XZ, next_id: 1) {"), "{printed}");
        assert_eq!(Document::from_text(&printed).unwrap(), doc);
        // An explicit value is kept.
        let doc =
            Document::from_text("gusset 1\nsketch S (plane: XY, next_id: 40) {\n  p1 = point(0, 0)\n}\n").unwrap();
        assert!(doc.to_text().contains("next_id: 40"));
    }

    #[test]
    fn comments_ending_in_carriage_returns_are_stable() {
        // Found by fuzzing: extra `\r`s before the line end used to stay in the comment,
        // so every print/parse cycle removed one.
        let src = "gusset 1\nunits mm\nparam a = 1  # note\r\r\r\n# above\r\r\nparam b = 2\r";
        let doc = Document::from_text(src).unwrap();
        assert_eq!(doc.params[0].comment, "note");
        assert_eq!(doc.params[1].comment, "above");
        let printed = doc.to_text();
        assert_eq!(Document::from_text(&printed).unwrap().to_text(), printed);
    }

    #[test]
    fn parse_errors_have_positions() {
        let e = text::parse("gusset 1\nunits mm\nextrude E1 (profile: S1, distance: 5 mm,, op: new)\n").unwrap_err();
        assert_eq!(e.line, 3);
        assert!(e.column > 30, "{e}");
        let e = text::parse("gusset 1\nwidget W1 ()\n").unwrap_err();
        assert!(e.message.contains("unknown statement"));
        let e = text::parse("gusset 9\n").unwrap_err();
        assert!(e.message.contains("not supported"));
        let e = text::parse("units mm\n").unwrap_err();
        assert!(e.hint.unwrap().contains("gusset 1"));
        let e = text::parse("gusset 1\nsketch S (plane: XY, next_id: 1) {\n  p1 = point(0, 0)\n").unwrap_err();
        assert!(e.message.contains("not closed"));
        let e = text::parse("gusset 1\nextrude E (profile: S, distance: 1 mm, op: sideways)\n").unwrap_err();
        assert!(e.hint.unwrap().contains("new, add, cut"));
        let e = text::parse("gusset 1\nfillet F (edges: [], radius: 1 mm, color: red)\n").unwrap_err();
        assert!(e.message.contains("no argument 'color'"));
        let e = text::parse("gusset 1\nparam a = 10 furlongs\n").unwrap_err();
        assert_eq!(e.line, 2);
    }

    #[test]
    fn hostile_strings_are_safe() {
        // Control characters are escaped on print and round-trip exactly.
        let mut doc = Document::from_text(SAMPLE).unwrap();
        doc.features[0].label = Some("evil\u{202e}label\u{7}\nnewline".into());
        let t = doc.to_text();
        assert!(!t.contains('\u{7}'));
        assert_eq!(Document::from_text(&t).unwrap(), doc);
        // Raw control characters in strings are rejected.
        assert!(text::parse("gusset 1\nimport I (blob: \"a\u{1}\", format: step)\n").is_err());
    }

    #[test]
    fn unique_names() {
        let doc = Document::from_text(SAMPLE).unwrap();
        assert_eq!(doc.unique_feature_name("Extrude"), "Extrude3");
        assert_eq!(doc.unique_feature_name("Shell"), "Shell1");
    }

    #[test]
    fn dependencies() {
        let doc = Document::from_text(SAMPLE).unwrap();
        let deps = doc.feature("Fillet1").unwrap().kind.dependencies();
        assert_eq!(deps, vec!["Extrude1".to_string(), "Sketch1".to_string()]);
        let deps = doc.feature("Sketch2").unwrap().kind.dependencies();
        assert_eq!(deps, vec!["Extrude1".to_string()]);
    }
}

#[cfg(test)]
mod object_tests {
    use super::*;

    #[test]
    fn feature_and_param_objects_roundtrip() {
        let doc = Document::from_text(
            "gusset 1\nunits mm\n\nparam a = 2 mm  # note\n\n# hello\nextrude E1 (profile: S1, distance: a, op: new)\n",
        );
        // E1 refers to a missing sketch: that is a validation issue, not a parse issue.
        assert!(doc.is_err());
        let doc = text::parse(
            "gusset 1\nunits mm\n\nparam a = 2 mm  # note\n\n# hello\nextrude E1 (profile: S1, distance: a, op: new)\n",
        )
        .unwrap();
        let f = &doc.features[0];
        let obj = text::print_feature_object(f);
        assert_eq!(&text::parse_feature_object(&obj).unwrap(), f);
        let p = text::print_params_object(&doc.params);
        assert_eq!(text::parse_params_object(&p).unwrap(), doc.params);
        assert!(text::parse_feature_object(&format!("{obj}{obj}")).is_err());
    }
}

#[cfg(test)]
mod shell_mirror_pattern_tests {
    use super::*;

    const SAMPLE: &str = r#"gusset 1
units mm

param n_bolts = 6
param pitch = 12 mm

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)

shell Shell1 (body: Extrude1, faces: [Extrude1/cap(end)], thickness: 2 mm)

shell Shell2 (faces: [], thickness: 1 mm)

mirror Mirror1 (body: Extrude1, plane: YZ)

mirror Mirror2 (plane: Extrude1/side(Sketch1/l6), merge: true)

pattern_linear Pattern1 (body: Extrude1, direction: X, count: 4, spacing: pitch)

pattern_linear Pattern2 (direction: [1, 0.5, 0], count: n_bolts - 1, spacing: -2 * pitch, suppressed: true)

pattern_circular Pattern3 (body: Extrude1, axis: Z, count: n_bolts, angle: 360 deg)

pattern_circular Pattern4 (axis: [0, 1, 0], through: [10 mm, 0 mm, 5 mm], count: 3, angle: 90 deg, label: "Fan")
"#;

    #[test]
    fn canonical_roundtrip() {
        // The text parser accepts the sample; validation is checked as well.
        let doc = text::parse(SAMPLE).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.to_text(), SAMPLE);
        assert_eq!(text::parse(&doc.to_text()).unwrap(), doc);
        for f in &doc.features {
            let obj = text::print_feature_object(f);
            assert_eq!(&text::parse_feature_object(&obj).unwrap(), f, "{obj}");
        }
        assert_eq!(doc.feature("Pattern4").unwrap().label.as_deref(), Some("Fan"));
        assert!(doc.feature("Pattern2").unwrap().suppressed);
        assert_eq!(validate::validate(&doc), vec![]);
    }

    #[test]
    fn dependencies_and_parameters() {
        let doc = text::parse(SAMPLE).unwrap();
        let deps = |n: &str| doc.feature(n).unwrap().kind.dependencies();
        assert_eq!(deps("Shell1"), vec!["Extrude1".to_string()]);
        assert!(deps("Shell2").is_empty());
        assert_eq!(deps("Mirror2"), vec!["Extrude1".to_string(), "Sketch1".to_string()]);
        assert_eq!(deps("Pattern1"), vec!["Extrude1".to_string()]);
        let params = |n: &str| doc.feature(n).unwrap().kind.parameters();
        assert_eq!(params("Pattern1"), vec!["pitch".to_string()]);
        assert_eq!(params("Pattern2"), vec!["n_bolts".to_string(), "pitch".to_string()]);
        assert_eq!(params("Pattern3"), vec!["n_bolts".to_string()]);
        assert!(params("Mirror1").is_empty());
    }

    #[test]
    fn mirror_without_merge_creates_a_body() {
        let doc = text::parse(SAMPLE).unwrap();
        assert!(validate::creates_body(doc.feature("Mirror1").unwrap()));
        assert!(!validate::creates_body(doc.feature("Mirror2").unwrap()));
        let mut ok = SAMPLE.to_string();
        ok.push_str("\nshell Shell3 (body: Mirror1, faces: [], thickness: 1 mm)\n");
        assert!(Document::from_text(&ok).is_ok());
        let mut bad = SAMPLE.to_string();
        bad.push_str("\nshell Shell3 (body: Mirror2, faces: [], thickness: 1 mm)\n");
        assert!(Document::from_text(&bad).is_err());
    }

    #[test]
    fn validation_problems() {
        // An unknown plane name reads as a datum plane, which does not exist.
        let e = Document::from_text("gusset 1\nunits mm\nmirror M (plane: ZZ)\n").unwrap_err().to_string();
        assert!(e.contains("'ZZ', which does not exist"), "{e}");
        let e = Document::from_text("gusset 1\nunits mm\npattern_linear P (direction: X, count: 2, spacing: 5 mm)\n");
        assert!(e.unwrap_err().to_string().contains("no body"));
        let t = "gusset 1\nsketch S (plane: XY, next_id: 1) {\n}\nextrude E (profile: S, distance: 1 mm, op: new)\npattern_linear P (direction: X, count: nn, spacing: 5 mm)\n";
        assert!(Document::from_text(t).unwrap_err().to_string().contains("unknown parameter 'nn'"));
        let t = "gusset 1\nsketch S (plane: XY, next_id: 1) {\n}\nextrude E (profile: S, distance: 1 mm, op: new)\nshell H (faces: [Z/cap(end)], thickness: 1 mm)\n";
        assert!(Document::from_text(t).unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn damaged_text_never_panics_and_prints_to_a_fixed_point() {
        // A cheap deterministic stand-in for the fuzzer: every truncation and every
        // single-character deletion of the sample.
        let chars: Vec<char> = SAMPLE.chars().collect();
        let check = |src: String| {
            if let Ok(doc) = text::parse(&src) {
                let printed = text::print(&doc);
                let again = text::parse(&printed).unwrap_or_else(|e| panic!("{src:?}: {e}"));
                assert_eq!(text::print(&again), printed, "{src:?}");
            }
        };
        for i in 0..chars.len() {
            check(chars[..i].iter().collect());
            check(chars[..i].iter().chain(&chars[i + 1..]).collect());
        }
    }

    #[test]
    fn parse_errors() {
        let bad = [
            "shell S (thickness: 1 mm)",
            "shell S (faces: [], thickness: 1 mm, count: 2)",
            "shell S (faces: [Extrude1/cap(end)], thickness: )",
            "shell S (faces: [Extrude1/cap(end), thickness: 1 mm)",
            "mirror M (merge: maybe, plane: XY)",
            "mirror M (merge: true)",
            "pattern_linear P (direction: W, count: 2, spacing: 1 mm)",
            "pattern_linear P (direction: [1, 0], count: 2, spacing: 1)",
            "pattern_linear P (direction: X, count: 2)",
            "pattern_circular P (axis: Z, count: 3)",
            "pattern_circular P (axis: Z, through: [1, 2], count: 3, angle: 90 deg)",
            "pattern_circular P (axis: Z, count: 3, angle: 90 deg, spacing: 1 mm)",
            "pattern_circular P (axis: Z, count: 3, count: 4, angle: 90 deg)",
        ];
        for b in bad {
            assert!(text::parse(&format!("gusset 1\n{b}\n")).is_err(), "{b}");
        }
    }
}

#[cfg(test)]
mod datum_move_combine_tests {
    use super::*;

    const SAMPLE: &str = r#"gusset 1
units mm

param lift = 12 mm
param tilt = 30 deg

sketch Sketch1 (plane: XY, next_id: 9) {
  p1 = point(0, 0)
  p2 = point(40, 0)
  p3 = point(40, 20)
  p4 = point(0, 20)
  l5 = line(p1, p2)
  l6 = line(p2, p3)
  l7 = line(p3, p4)
  l8 = line(p4, p1)
}

extrude Extrude1 (profile: Sketch1, distance: 10 mm, op: new)

plane Plane1 (base: XY, offset: lift)

plane Plane2 (base: Extrude1/cap(end), offset: -2 * lift)

plane Plane3 (base: Plane1, axis: X, angle: tilt)

plane Plane4 (base: XZ, axis: edge(Extrude1/cap(end), Extrude1/side(Sketch1/l5)), angle: 45 deg, label: "Slope")

sketch Sketch2 (plane: Plane3, next_id: 3) {
  p1 = point(5, 5)
  c2 = circle(p1, 2)
}

extrude Extrude2 (profile: Sketch2, distance: 5 mm, op: new)

mirror Mirror1 (body: Extrude1, plane: Plane2)

move Move1 (body: Extrude2, translate: [10 mm, 0 mm, lift], copy: true)

move Move2 (translate: [1 mm, 2 mm, 3 mm], suppressed: true)

move Move3 (body: Extrude1, axis: [0, 0, 1], angle: tilt, through: [20 mm, 10 mm, 0 mm])

move Move4 (translate: [0 mm, 0 mm, 5 mm], axis: X, angle: 90 deg)

combine Combine1 (body: Extrude1, tools: [Extrude2, Move1], op: cut)

combine Combine2 (tools: [Mirror1], op: add, label: "Weld")
"#;

    #[test]
    fn canonical_roundtrip() {
        let doc = text::parse(SAMPLE).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(doc.to_text(), SAMPLE);
        assert_eq!(text::parse(&doc.to_text()).unwrap(), doc);
        for f in &doc.features {
            let obj = text::print_feature_object(f);
            assert_eq!(&text::parse_feature_object(&obj).unwrap(), f, "{obj}");
        }
        assert_eq!(validate::validate(&doc), vec![]);
        assert!(matches!(
            &doc.feature("Sketch2").unwrap().kind,
            FeatureKind::Sketch(s) if s.plane == PlaneRef::Datum("Plane3".into())
        ));
        assert!(matches!(
            &doc.feature("Mirror1").unwrap().kind,
            FeatureKind::Mirror(m) if m.plane == PlaneRef::Datum("Plane2".into())
        ));
        // The default operation is add and is printed.
        let d = text::parse("gusset 1\ncombine C (tools: [A])\n").unwrap();
        assert_eq!(d.to_text(), "gusset 1\nunits mm\n\ncombine C (tools: [A], op: add)\n");
    }

    #[test]
    fn dependencies_and_parameters() {
        let doc = text::parse(SAMPLE).unwrap();
        let deps = |n: &str| doc.feature(n).unwrap().kind.dependencies();
        let strs = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert!(deps("Plane1").is_empty());
        assert_eq!(deps("Plane2"), strs(&["Extrude1"]));
        assert_eq!(deps("Plane3"), strs(&["Plane1"]));
        assert_eq!(deps("Plane4"), strs(&["Extrude1", "Sketch1"]));
        assert_eq!(deps("Sketch2"), strs(&["Plane3"]));
        assert_eq!(deps("Mirror1"), strs(&["Extrude1", "Plane2"]));
        assert_eq!(deps("Move1"), strs(&["Extrude2"]));
        assert_eq!(deps("Combine1"), strs(&["Extrude1", "Extrude2", "Move1"]));
        let params = |n: &str| doc.feature(n).unwrap().kind.parameters();
        assert_eq!(params("Plane1"), strs(&["lift"]));
        assert_eq!(params("Plane3"), strs(&["tilt"]));
        assert_eq!(params("Move1"), strs(&["lift"]));
        assert_eq!(params("Move3"), strs(&["tilt"]));
        assert!(params("Combine1").is_empty());
    }

    #[test]
    fn bodies_created_and_consumed() {
        let doc = text::parse(SAMPLE).unwrap();
        let creates = |n: &str| validate::creates_body(doc.feature(n).unwrap());
        let body = |n: &str| validate::created_body_name(doc.feature(n).unwrap()).map(str::to_string);
        assert!(!creates("Plane1") && !creates("Move2") && !creates("Combine1"));
        assert!(creates("Move1"));
        assert_eq!(body("Move1").as_deref(), Some("Move1"));
        assert_eq!(body("Move3"), None);
        // A combined tool no longer exists as a target; the target keeps existing.
        let ok = |extra: &str| Document::from_text(&format!("{SAMPLE}\n{extra}\n"));
        let e = ok("shell Shell1 (body: Extrude2, faces: [], thickness: 1 mm)").unwrap_err().to_string();
        assert!(e.contains("body 'Extrude2' was consumed by Combine1"), "{e}");
        let e = ok("combine C3 (body: Extrude1, tools: [Move1], op: add)").unwrap_err().to_string();
        assert!(e.contains("'Move1' was consumed by Combine1"), "{e}");
        assert!(ok("shell Shell1 (body: Extrude1, faces: [], thickness: 1 mm)").is_ok());
        // A suppressed combine consumes nothing, so its tools stay available.
        let sup = SAMPLE.replace("op: cut)", "op: cut, suppressed: true)");
        assert!(
            Document::from_text(&format!("{sup}\nshell Shell1 (body: Extrude2, faces: [], thickness: 1 mm)\n")).is_ok()
        );
        // A body created by a copy can be targeted before it is consumed.
        let early = SAMPLE.replace(
            "combine Combine1",
            "pattern_linear P1 (body: Move1, direction: X, count: 2, spacing: 5 mm)\n\ncombine Combine1",
        );
        assert!(Document::from_text(&early).is_ok());
    }

    #[test]
    fn validation_problems() {
        let base = "gusset 1\nunits mm\nsketch S (plane: XY, next_id: 1) {\n}\nextrude E (profile: S, distance: 1 mm, op: new)\nextrude F (profile: S, distance: 1 mm, op: new)\n";
        let bad = |extra: &str| Document::from_text(&format!("{base}{extra}\n")).unwrap_err().to_string();
        let has = |extra: &str, want: &str| {
            let e = bad(extra);
            assert!(e.contains(want), "{extra}: {e}");
        };
        has("plane P (base: Q, offset: 1 mm)", "'Q', which does not exist");
        has("plane P (base: S, offset: 1 mm)", "'S' is not a datum plane");
        has("plane XY (base: XY, offset: 1 mm)", "base plane");
        has("plane P (base: XY, offset: depth)", "unknown parameter 'depth'");
        has("sketch T (plane: E, next_id: 1) {\n}", "'E' is not a datum plane");
        has("mirror M (plane: Nope)", "'Nope', which does not exist");
        has("move M (body: S, translate: [1, 2, 3])", "does not create a body");
        has("move M (body: Z, translate: [1, 2, 3])", "'Z', which does not exist");
        has("combine C (tools: [], body: E)", "no tool bodies");
        has("combine C (body: E, tools: [F, F])", "listed twice");
        has("combine C (body: E, tools: [E])", "both the target and a tool");
        has("combine C (body: E, tools: [S])", "does not create a body");
        has("combine C (body: E, tools: [Missing])", "'Missing', which does not exist");
        has("move M (translate: [1, 2, 3], copy: true)\nmove M (translate: [1, 2, 3])", "used twice");
        // Another combine cannot reuse a consumed tool, but its target survives.
        has("combine C (body: E, tools: [F])\ncombine D (body: E, tools: [F])", "consumed by C");
        assert!(Document::from_text(&format!("{base}combine C (body: E, tools: [F])\n")).is_ok());
        assert!(
            Document::from_text(&format!("{base}combine C (tools: [E])\n")).is_ok(),
            "the target defaults to the other body"
        );
        has("combine C (tools: [E, F])", "there is no body to work on");
        let many = (0..=validate::MAX_COMBINE_TOOLS).map(|i| format!("T{i}")).collect::<Vec<_>>().join(", ");
        has(&format!("combine C (body: E, tools: [{many}])"), "at most 500 per combine");
    }

    #[test]
    fn parse_errors() {
        let bad = [
            "plane P (offset: 1 mm)",
            "plane P (base: XY)",
            "plane P (base: XY, offset: 1 mm, angle: 3 deg, axis: X)",
            "plane P (base: XY, axis: X)",
            "plane P (base: XY, angle: 3 deg)",
            "plane P (base: XY, axis: W, angle: 3 deg)",
            "plane P (base: XY, axis: edge(), angle: 3 deg)",
            "plane P (base: XY, offset: 1 mm, color: red)",
            "plane P (base: Extrude1/wat(1), offset: 1 mm)",
            "plane P (base: XY, offset: )",
            "move M ()",
            "move M (body: E)",
            "move M (axis: Z)",
            "move M (angle: 3 deg)",
            "move M (through: [1 mm, 2 mm, 3 mm], translate: [1, 2, 3])",
            "move M (translate: [1, 2], copy: true)",
            "move M (translate: [1, 2, 3], copy: maybe)",
            "move M (translate: [1, 2, 3], op: add)",
            "combine C (op: add)",
            "combine C (tools: [A], op: new)",
            "combine C (tools: [A], op: sideways)",
            "combine C (tools: [A, ])",
            "combine C (tools: A)",
            "combine C (tools: [A], tools: [B])",
        ];
        for b in bad {
            assert!(text::parse(&format!("gusset 1\n{b}\n")).is_err(), "{b}");
        }
        let e = text::parse("gusset 1\ncombine C (tools: [A], op: new)\n").unwrap_err();
        assert!(e.hint.unwrap().contains("add, cut or intersect"));
    }

    #[test]
    fn damaged_text_never_panics_and_prints_to_a_fixed_point() {
        let chars: Vec<char> = SAMPLE.chars().collect();
        let check = |src: String| {
            if let Ok(doc) = text::parse(&src) {
                let printed = text::print(&doc);
                let again = text::parse(&printed).unwrap_or_else(|e| panic!("{src:?}: {e}"));
                assert_eq!(text::print(&again), printed, "{src:?}");
            }
        };
        for i in 0..chars.len() {
            check(chars[..i].iter().collect());
            check(chars[..i].iter().chain(&chars[i + 1..]).collect());
        }
    }

    #[test]
    fn plane_references_parse_outside_documents() {
        assert_eq!(text::parse_plane_ref("YZ").unwrap(), PlaneRef::YZ);
        assert_eq!(text::parse_plane_ref("Plane1").unwrap(), PlaneRef::Datum("Plane1".into()));
        assert!(matches!(text::parse_plane_ref("Extrude1/cap(end)").unwrap(), PlaneRef::Face(_)));
        assert!(text::parse_plane_ref("Plane1 extra").is_err());
        assert!(text::parse_plane_ref("").is_err());
        let mut p = PlaneRef::Datum("Plane1".into());
        p.rename_feature("Plane1", "Datum");
        assert_eq!(p, PlaneRef::Datum("Datum".into()));
    }
}
