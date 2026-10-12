//! Semantic validation of a parsed document: names, ordering, sketch integrity.
//!
//! Validation never mutates and reports every problem it finds, so the code view and
//! MCP can show them all at once.

use crate::model::*;
use gusset_units::params::validate_name;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    /// The feature (or parameter) the issue belongs to.
    pub subject: Option<String>,
    pub message: String,
    pub hint: Option<String>,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(s) = &self.subject {
            write!(f, "{s}: ")?;
        }
        f.write_str(&self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

fn issue(subject: &str, message: impl Into<String>) -> Issue {
    Issue { subject: Some(subject.to_string()), message: message.into(), hint: None }
}

fn issue_h(subject: &str, message: impl Into<String>, hint: impl Into<String>) -> Issue {
    Issue { subject: Some(subject.to_string()), message: message.into(), hint: Some(hint.into()) }
}

/// Does this feature create a body others can target?
pub fn creates_body(f: &Feature) -> bool {
    match &f.kind {
        FeatureKind::Extrude(e) => e.op == BodyOp::New,
        FeatureKind::Revolve(r) => r.op == BodyOp::New,
        FeatureKind::Import(_) => true,
        FeatureKind::Mirror(m) => !m.merge,
        _ => false,
    }
}

/// The name of the body a body-creating feature produces: its `body_name`, else the
/// feature name. `None` for features that create no body.
pub fn created_body_name(f: &Feature) -> Option<&str> {
    match &f.kind {
        FeatureKind::Extrude(ExtrudeDef { op: BodyOp::New, body_name, .. })
        | FeatureKind::Revolve(RevolveDef { op: BodyOp::New, body_name, .. }) => {
            Some(body_name.as_deref().unwrap_or(&f.name))
        }
        FeatureKind::Import(_) => Some(&f.name),
        _ => None,
    }
}

pub fn validate(doc: &Document) -> Vec<Issue> {
    let mut out = Vec::new();

    // Parameters.
    let mut pnames = BTreeSet::new();
    for p in &doc.params {
        if let Err(e) = validate_name(&p.name) {
            out.push(issue(&p.name, e));
        }
        if !pnames.insert(p.name.as_str()) {
            out.push(issue_h(&p.name, "parameter is defined twice", "rename or remove one of them"));
        }
    }

    // Features: unique names, earlier-only references.
    let mut seen: BTreeMap<&str, &Feature> = BTreeMap::new();
    let mut any_body_before = false;
    let mut body_names: BTreeSet<&str> = BTreeSet::new();
    for f in &doc.features {
        let n = f.name.as_str();
        if let Err(e) = validate_feature_name(n) {
            out.push(issue(n, e));
        }
        if seen.contains_key(n) {
            out.push(issue_h(n, "feature name is used twice", "feature names must be unique"));
        }
        for dep in f.kind.dependencies() {
            if dep == n {
                out.push(issue(n, "feature refers to itself"));
            } else if !seen.contains_key(dep.as_str()) {
                let later = doc.features.iter().any(|g| g.name == dep);
                out.push(if later {
                    issue_h(
                        n,
                        format!("refers to '{dep}', which comes later in the timeline"),
                        "move this feature after it",
                    )
                } else {
                    issue(n, format!("refers to '{dep}', which does not exist"))
                });
            }
        }
        for p in f.kind.parameters() {
            if !pnames.contains(p.as_str()) && !matches!(p.as_str(), "pi" | "PI" | "e") {
                out.push(issue_h(n, format!("uses unknown parameter '{p}'"), format!("add 'param {p} = ...'")));
            }
        }
        match &f.kind {
            FeatureKind::Sketch(s) => validate_sketch(n, s, &mut out),
            FeatureKind::Extrude(ExtrudeDef { profile, op, body, body_name, .. })
            | FeatureKind::Revolve(RevolveDef { profile, op, body, body_name, .. }) => {
                if let Some(bn) = body_name {
                    if let Err(e) = validate_name(bn) {
                        out.push(issue(n, e.replace("parameter", "body")));
                    }
                    if *op != BodyOp::New {
                        out.push(issue(n, "'body_name' can only be used with op: new"));
                    }
                }
                check_profile(n, profile, &seen, &mut out);
                if let FeatureKind::Revolve(r) = &f.kind
                    && let AxisRef::SketchLine { sketch, entity } = &r.axis
                {
                    check_sketch_entity(n, sketch, entity, Some('l'), &seen, &mut out);
                }
                if let Some(b) = body {
                    match seen.get(b.as_str()) {
                        Some(t) if !creates_body(t) => out.push(issue_h(
                            n,
                            format!("'{b}' does not create a body"),
                            "target a feature with op: new, or an import",
                        )),
                        _ => {}
                    }
                    if *op == BodyOp::New {
                        out.push(issue(n, "'body' cannot be combined with op: new"));
                    }
                } else if *op != BodyOp::New && !any_body_before {
                    out.push(issue_h(
                        n,
                        format!("op: {} needs an existing body", op.keyword()),
                        "use op: new for the first body",
                    ));
                }
            }
            FeatureKind::Fillet(b) | FeatureKind::Chamfer(b) => {
                if b.edges.is_empty() {
                    out.push(issue_h(n, "no edges selected", "select at least one edge"));
                }
            }
            FeatureKind::Shell(d) => check_body_target(n, d.body.as_deref(), any_body_before, &seen, &mut out),
            FeatureKind::Mirror(d) => check_body_target(n, d.body.as_deref(), any_body_before, &seen, &mut out),
            FeatureKind::PatternLinear(d) => {
                check_body_target(n, d.body.as_deref(), any_body_before, &seen, &mut out);
            }
            FeatureKind::PatternCircular(d) => {
                check_body_target(n, d.body.as_deref(), any_body_before, &seen, &mut out);
            }
            FeatureKind::Import(i) => {
                if i.blob.len() != 64 || !i.blob.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
                    out.push(issue(n, "blob must be a 64-character lowercase hex hash"));
                }
            }
        }
        if creates_body(f) {
            any_body_before = true;
        }
        if let Some(bn) = created_body_name(f)
            && !body_names.insert(bn)
        {
            out.push(issue_h(n, format!("body name '{bn}' is used twice"), "give the body a different body_name"));
        }
        seen.insert(n, f);
    }
    out
}

/// A feature that modifies an existing body: the named body must be created by an earlier
/// feature, or (without a name) some body must exist.
fn check_body_target(
    n: &str,
    body: Option<&str>,
    any_body_before: bool,
    seen: &BTreeMap<&str, &Feature>,
    out: &mut Vec<Issue>,
) {
    match body {
        Some(b) => {
            if let Some(t) = seen.get(b)
                && !creates_body(t)
            {
                out.push(issue_h(
                    n,
                    format!("'{b}' does not create a body"),
                    "target a feature with op: new, a mirror without merge, or an import",
                ));
            }
        }
        None if !any_body_before => {
            out.push(issue_h(n, "there is no body to work on", "create a body first, for example with extrude"));
        }
        None => {}
    }
}

/// Feature names follow parameter-name rules.
pub fn validate_feature_name(n: &str) -> Result<(), String> {
    validate_name(n).map_err(|e| e.replace("parameter", "feature"))
}

fn check_profile(n: &str, p: &ProfileRef, seen: &BTreeMap<&str, &Feature>, out: &mut Vec<Issue>) {
    match seen.get(p.sketch.as_str()) {
        Some(f) if !matches!(f.kind, FeatureKind::Sketch(_)) => {
            out.push(issue(n, format!("profile '{}' is not a sketch", p.sketch)));
        }
        _ => {}
    }
    for r in &p.regions {
        check_sketch_entity(n, &p.sketch, r, None, seen, out);
    }
}

fn check_sketch_entity(
    n: &str,
    sketch: &str,
    entity: &str,
    prefix: Option<char>,
    seen: &BTreeMap<&str, &Feature>,
    out: &mut Vec<Issue>,
) {
    let Some(f) = seen.get(sketch) else { return };
    let FeatureKind::Sketch(s) = &f.kind else {
        out.push(issue(n, format!("'{sketch}' is not a sketch")));
        return;
    };
    match s.entities.iter().find(|e| e.id == entity) {
        None => out.push(issue(n, format!("sketch '{sketch}' has no entity '{entity}'"))),
        Some(e) => {
            if let Some(p) = prefix
                && e.geom.id_prefix() != p
            {
                out.push(issue(
                    n,
                    format!("'{sketch}.{entity}' must be a {}", if p == 'l' { "line" } else { "entity" }),
                ));
            }
        }
    }
}

fn id_number(id: &str) -> Option<(char, u32)> {
    let mut chars = id.chars();
    let p = chars.next()?;
    let num: u32 = chars.as_str().parse().ok()?;
    if chars.as_str().starts_with('0') {
        return None;
    }
    Some((p, num))
}

fn validate_sketch(n: &str, s: &SketchDef, out: &mut Vec<Issue>) {
    let mut kinds: BTreeMap<&str, char> = BTreeMap::new();
    let check_id = |id: &str, expected: &[char], out: &mut Vec<Issue>| -> bool {
        match id_number(id) {
            Some((p, num)) if expected.contains(&p) => {
                if num >= s.next_id {
                    out.push(issue_h(n, format!("id '{id}' is not below next_id {}", s.next_id), "increase next_id"));
                }
                true
            }
            _ => {
                out.push(issue(
                    n,
                    format!("'{id}' is not a valid id here (expected {expected:?} followed by a number)"),
                ));
                false
            }
        }
    };
    let mut ids = BTreeSet::new();
    for e in &s.entities {
        let ok = check_id(&e.id, &[e.geom.id_prefix()], out);
        if !ids.insert(e.id.as_str()) {
            out.push(issue(n, format!("sketch id '{}' is used twice", e.id)));
        }
        if ok {
            kinds.insert(&e.id, e.geom.id_prefix());
        }
        match &e.geom {
            SkGeom::Point { x, y } if !(x.is_finite() && y.is_finite()) => {
                out.push(issue(n, format!("point '{}' has non-finite coordinates", e.id)));
            }
            SkGeom::Circle { radius, .. } if !(radius.is_finite() && *radius > 0.0) => {
                out.push(issue(n, format!("circle '{}' needs a positive radius", e.id)));
            }
            _ => {}
        }
    }
    // Geometry references must point to earlier points.
    let mut defined = BTreeSet::new();
    for e in &s.entities {
        for r in e.geom.refs() {
            if kinds.get(r) != Some(&'p') {
                out.push(issue(n, format!("'{}' refers to '{r}', which is not a point of this sketch", e.id)));
            } else if !defined.contains(r) {
                out.push(issue(n, format!("'{}' refers to point '{r}' before it is defined", e.id)));
            }
        }
        if let SkGeom::Line { a, b } = &e.geom
            && a == b
        {
            out.push(issue(n, format!("line '{}' starts and ends at the same point", e.id)));
        }
        defined.insert(e.id.as_str());
    }
    for c in &s.constraints {
        check_id(&c.id, &['k'], out);
        if !ids.insert(c.id.as_str()) {
            out.push(issue(n, format!("sketch id '{}' is used twice", c.id)));
        }
        for r in &c.entities {
            if !kinds.contains_key(r.as_str()) {
                out.push(issue(n, format!("constraint '{}' refers to unknown entity '{r}'", c.id)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::parse;

    fn issues(src: &str) -> Vec<String> {
        validate(&parse(src).unwrap()).iter().map(ToString::to_string).collect()
    }

    #[test]
    fn detects_problems() {
        let src = "gusset 1\nunits mm\nparam w = 1 mm\nparam w = 2 mm\n\
            sketch S1 (plane: XY, next_id: 3) {\n  p1 = point(0, 0)\n  l2 = line(p1, p9)\n  k5 = horizontal(l2)\n}\n\
            extrude E1 (profile: S2, distance: depth, op: cut)\n\
            fillet F1 (edges: [], radius: 1 mm)\n";
        let v = issues(src);
        let has = |s: &str| v.iter().any(|i| i.contains(s));
        assert!(has("defined twice"), "{v:?}");
        assert!(has("'p9'"), "{v:?}");
        assert!(has("next_id"), "{v:?}");
        assert!(has("'S2', which does not exist"), "{v:?}");
        assert!(has("unknown parameter 'depth'"), "{v:?}");
        assert!(has("needs an existing body"), "{v:?}");
        assert!(has("no edges"), "{v:?}");
    }

    #[test]
    fn forward_reference_hint() {
        let src = "gusset 1\nunits mm\nextrude E1 (profile: S1, distance: 1 mm, op: new)\n\
            sketch S1 (plane: XY, next_id: 1) {\n}\n";
        let v = issues(src);
        assert!(v.iter().any(|i| i.contains("comes later")), "{v:?}");
    }
}
