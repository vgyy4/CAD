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

pub use cursor::ParseError;
pub use model::*;
pub use refs::{CapSide, EdgeRef, FaceKey, FaceRef};
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

revolve Revolve1 (profile: Sketch2, axis: X, angle: 180 deg, op: new, suppressed: true)

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
    }

    #[test]
    fn noncanonical_input_normalises() {
        let messy = "gusset 1\r\nunits in\n\n\n   param   a=1+2   \nsketch S (next_id: 1,plane:XZ) {\n\n}\n";
        let doc = Document::from_text(messy).unwrap();
        assert_eq!(doc.to_text(), "gusset 1\nunits in\n\nparam a = 1 + 2\n\nsketch S (plane: XZ, next_id: 1) {\n}\n");
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
