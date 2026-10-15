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

/// Most tool bodies one `combine` may merge (each costs a boolean operation).
pub const MAX_COMBINE_TOOLS: usize = 500;

/// Does this feature create a body others can target?
pub fn creates_body(f: &Feature) -> bool {
    match &f.kind {
        FeatureKind::Extrude(e) => e.op == BodyOp::New,
        FeatureKind::Revolve(r) => r.op == BodyOp::New,
        FeatureKind::Sweep(d) => d.op == BodyOp::New,
        FeatureKind::Loft(d) => d.op == BodyOp::New,
        FeatureKind::Import(_) | FeatureKind::Component(_) => true,
        FeatureKind::Mirror(m) => !m.merge,
        FeatureKind::Move(m) => m.copy,
        _ => false,
    }
}

/// The name of the body a body-creating feature produces: its `body_name`, else the
/// feature name. `None` for features that create no body.
pub fn created_body_name(f: &Feature) -> Option<&str> {
    match &f.kind {
        FeatureKind::Extrude(ExtrudeDef { op: BodyOp::New, body_name, .. })
        | FeatureKind::Revolve(RevolveDef { op: BodyOp::New, body_name, .. })
        | FeatureKind::Sweep(SweepDef { op: BodyOp::New, body_name, .. })
        | FeatureKind::Loft(LoftDef { op: BodyOp::New, body_name, .. }) => {
            Some(body_name.as_deref().unwrap_or(&f.name))
        }
        FeatureKind::Import(_) | FeatureKind::Component(_) => Some(&f.name),
        FeatureKind::Mirror(m) if !m.merge => Some(&f.name),
        FeatureKind::Move(m) if m.copy => Some(&f.name),
        _ => None,
    }
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
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

    // Configurations: unique names, setting existing parameters.
    let mut cnames = BTreeSet::new();
    for c in &doc.configurations {
        if let Err(e) = validate_feature_name(&c.name) {
            out.push(issue(&c.name, e));
        }
        if !cnames.insert(c.name.as_str()) {
            out.push(issue_h(&c.name, "configuration is defined twice", "rename or remove one of them"));
        }
        let mut set = BTreeSet::new();
        for (n, _) in &c.set {
            if !set.insert(n.as_str()) {
                out.push(issue_h(&c.name, format!("sets '{n}' twice"), "keep one value per parameter"));
            }
            if !pnames.contains(n.as_str()) {
                out.push(issue_h(
                    &c.name,
                    format!("sets '{n}', which is not a parameter"),
                    "remove it or add the parameter",
                ));
            }
        }
    }

    // Features: unique names, earlier-only references.
    let mut seen: BTreeMap<&str, &Feature> = BTreeMap::new();
    // Bodies that exist at this point (a combine consumes its tool bodies).
    let mut live_bodies = 0usize;
    let mut consumed: BTreeMap<&str, &str> = BTreeMap::new();
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
            FeatureKind::Sketch(s) => {
                check_plane(n, &s.plane, &seen, &mut out);
                validate_sketch(n, s, &mut out);
            }
            FeatureKind::Extrude(ExtrudeDef { profile, op, body, body_name, .. })
            | FeatureKind::Revolve(RevolveDef { profile, op, body, body_name, .. })
            | FeatureKind::Sweep(SweepDef { profile, op, body, body_name, .. }) => {
                check_profile(n, profile, &seen, &mut out);
                if let FeatureKind::Revolve(r) = &f.kind
                    && let AxisRef::SketchLine { sketch, entity } = &r.axis
                {
                    check_sketch_entity(n, sketch, entity, Some('l'), &seen, &mut out);
                }
                if let FeatureKind::Sweep(d) = &f.kind {
                    check_sweep(n, d, &seen, &mut out);
                }
                let target = BodyTarget { op: *op, body: body.as_deref(), body_name: body_name.as_deref() };
                check_body_op(n, &target, live_bodies, &seen, &consumed, &mut out);
            }
            FeatureKind::Loft(LoftDef { profiles, op, body, body_name, .. }) => {
                if profiles.len() < 2 {
                    out.push(issue_h(
                        n,
                        "a loft needs at least two profiles",
                        "list them in order: profiles: [Sketch1, Sketch2]",
                    ));
                }
                if profiles.len() > MAX_LOFT_PROFILES {
                    out.push(issue(n, format!("{} profiles; at most {MAX_LOFT_PROFILES} per loft", profiles.len())));
                }
                for p in profiles {
                    check_profile(n, p, &seen, &mut out);
                }
                let target = BodyTarget { op: *op, body: body.as_deref(), body_name: body_name.as_deref() };
                check_body_op(n, &target, live_bodies, &seen, &consumed, &mut out);
            }
            FeatureKind::Fillet(b) | FeatureKind::Chamfer(b) => {
                if b.edges.is_empty() {
                    out.push(issue_h(n, "no edges selected", "select at least one edge"));
                }
            }
            FeatureKind::Shell(d) => {
                check_body_target(n, d.body.as_deref(), live_bodies > 0, &seen, &consumed, &mut out);
            }
            FeatureKind::Draft(d) => {
                if d.faces.is_empty() {
                    out.push(issue_h(n, "no faces selected", "select the faces to draft"));
                }
                check_plane(n, &d.plane, &seen, &mut out);
                check_body_target(n, d.body.as_deref(), live_bodies > 0, &seen, &consumed, &mut out);
            }
            FeatureKind::Mirror(d) => {
                check_plane(n, &d.plane, &seen, &mut out);
                check_body_target(n, d.body.as_deref(), live_bodies > 0, &seen, &consumed, &mut out);
            }
            FeatureKind::PatternLinear(d) => {
                check_body_target(n, d.body.as_deref(), live_bodies > 0, &seen, &consumed, &mut out);
            }
            FeatureKind::PatternCircular(d) => {
                check_body_target(n, d.body.as_deref(), live_bodies > 0, &seen, &consumed, &mut out);
            }
            FeatureKind::Plane(d) => {
                if matches!(n, "XY" | "XZ" | "YZ") {
                    out.push(issue_h(n, "this name is a base plane", "choose another name for the datum plane"));
                }
                check_plane(n, &d.base, &seen, &mut out);
            }
            FeatureKind::Move(d) => {
                check_body_target(n, d.body.as_deref(), live_bodies > 0, &seen, &consumed, &mut out);
            }
            FeatureKind::Joint(d) => {
                for b in std::iter::once(&d.body).chain(&d.with) {
                    check_body_target(n, Some(b), true, &seen, &consumed, &mut out);
                }
                if d.with.contains(&d.body) {
                    out.push(issue_h(
                        n,
                        format!("'{}' is both the moving body and in 'with'", d.body),
                        "remove it from 'with'",
                    ));
                }
                let mut carried = BTreeSet::new();
                for w in &d.with {
                    if !carried.insert(w.as_str()) {
                        out.push(issue_h(
                            n,
                            format!("'{w}' is listed twice in 'with'"),
                            "list each carried body once (a body name and its feature name are the same body)",
                        ));
                    }
                }
            }
            FeatureKind::Combine(d) => {
                // Without a named target there must be a body besides the tools.
                let spare = live_bodies > d.tools.len();
                check_body_target(n, d.body.as_deref(), spare, &seen, &consumed, &mut out);
                if d.tools.len() > MAX_COMBINE_TOOLS {
                    out.push(issue_h(
                        n,
                        format!("{} tools; at most {MAX_COMBINE_TOOLS} per combine", d.tools.len()),
                        "combine in several steps",
                    ));
                }
                if d.tools.is_empty() {
                    out.push(issue_h(n, "no tool bodies", "list the bodies to combine in tools: [Extrude2]"));
                }
                let mut listed = BTreeSet::new();
                for t in &d.tools {
                    if !listed.insert(t.as_str()) {
                        out.push(issue(n, format!("tool '{t}' is listed twice")));
                    }
                    if d.body.as_deref() == Some(t.as_str()) {
                        out.push(issue_h(
                            n,
                            format!("'{t}' is both the target and a tool"),
                            "a body cannot be combined with itself",
                        ));
                    }
                    if seen.get(t.as_str()).is_some_and(|f| !creates_body(f)) {
                        out.push(issue_h(
                            n,
                            format!("tool '{t}' does not create a body"),
                            "name the feature that created the body, e.g. an extrude with op: new",
                        ));
                    }
                    check_not_consumed(n, t, &consumed, &mut out);
                }
                // A suppressed combine is skipped by the evaluator, so it consumes nothing.
                for t in d.tools.iter().filter(|_| !f.suppressed) {
                    if seen.get(t.as_str()).is_some_and(|g| creates_body(g)) && !consumed.contains_key(t.as_str()) {
                        consumed.insert(t, n);
                        live_bodies = live_bodies.saturating_sub(1);
                    }
                }
            }
            FeatureKind::Hole(d) => check_hole(n, d, &seen, &mut out),
            FeatureKind::Import(i) => {
                if !is_hash(&i.blob) {
                    out.push(issue(n, "blob must be a 64-character lowercase hex hash"));
                }
            }
            FeatureKind::Component(d) => {
                if !is_hash(&d.document) {
                    out.push(issue(n, "document must be a 64-character lowercase hex hash"));
                }
                if d.body.as_deref().is_some_and(|b| validate_feature_name(b).is_err()) {
                    out.push(issue(n, "body must be a body name of the component's document"));
                }
            }
        }
        if creates_body(f) {
            live_bodies += 1;
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

/// A hole feature: its sites exist and its arguments fit its type.
fn check_hole(n: &str, d: &HoleDef, seen: &BTreeMap<&str, &Feature>, out: &mut Vec<Issue>) {
    match &d.sites {
        HoleSites::Sketch(s) => {
            if let Some(f) = seen.get(s.as_str())
                && !matches!(f.kind, FeatureKind::Sketch(_))
            {
                out.push(issue(n, format!("points: '{s}' is not a sketch")));
            }
        }
        HoleSites::Points(p) if p.is_empty() => {
            out.push(issue_h(n, "the hole has no points", "give at: [[10 mm, 5 mm]] or points: <sketch>"));
        }
        HoleSites::Points(p) if p.len() > MAX_HOLE_SITES => {
            out.push(issue(n, format!("{} hole points; at most {MAX_HOLE_SITES} per feature", p.len())));
        }
        HoleSites::Points(p) => {
            let mut ids = BTreeSet::new();
            for h in p {
                if h.id == 0 || !ids.insert(h.id) {
                    out.push(issue(n, format!("hole point id {} is zero or used twice", h.name())));
                }
            }
        }
    }
    let extra = |has: bool, wanted: bool, what: &str, ty: &str, out: &mut Vec<Issue>| {
        if wanted && !has {
            out.push(issue_h(n, format!("type: {ty} needs {what}"), format!("add {what}, or change the type")));
        } else if has && !wanted {
            out.push(issue_h(
                n,
                format!("{what} only apply to type: {ty}"),
                format!("remove {what}, or use type: {ty}"),
            ));
        }
    };
    extra(
        d.counterbore.is_some(),
        d.kind == HoleType::Counterbore,
        "cbore_diameter and cbore_depth",
        "counterbore",
        out,
    );
    extra(
        d.countersink.is_some(),
        d.kind == HoleType::Countersink,
        "csink_diameter and csink_angle",
        "countersink",
        out,
    );
    extra(d.thread.is_some(), d.kind == HoleType::Tapped, "thread and pitch", "tapped", out);
    if let Some(t) = &d.thread {
        let bad = t.designation.is_empty()
            || t.designation.chars().count() > MAX_THREAD_DESIGNATION
            || t.designation.chars().any(|c| c.is_control() || c == '"');
        if bad {
            out.push(issue_h(n, "the thread designation must be 1 to 32 printable characters", "write it like \"M6\""));
        }
    }
    if d.point_angle.is_some() && d.depth == HoleDepth::Through {
        out.push(issue_h(n, "point_angle only applies to a blind hole", "remove it, or give a depth"));
    }
}

/// The body arguments of a feature that creates a swept tool and combines it with a body.
struct BodyTarget<'a> {
    op: BodyOp,
    body: Option<&'a str>,
    body_name: Option<&'a str>,
}

fn check_body_op(
    n: &str,
    t: &BodyTarget<'_>,
    live_bodies: usize,
    seen: &BTreeMap<&str, &Feature>,
    consumed: &BTreeMap<&str, &str>,
    out: &mut Vec<Issue>,
) {
    if let Some(bn) = t.body_name {
        if let Err(e) = validate_name(bn) {
            out.push(issue(n, e.replace("parameter", "body")));
        }
        if t.op != BodyOp::New {
            out.push(issue(n, "'body_name' can only be used with op: new"));
        }
    }
    if let Some(b) = t.body {
        match seen.get(b) {
            Some(g) if !creates_body(g) => out.push(issue_h(
                n,
                format!("'{b}' does not create a body"),
                "target a feature with op: new, or an import",
            )),
            _ => {}
        }
        check_not_consumed(n, b, consumed, out);
        if t.op == BodyOp::New {
            out.push(issue(n, "'body' cannot be combined with op: new"));
        }
    } else if t.op != BodyOp::New && live_bodies == 0 {
        out.push(issue_h(
            n,
            format!("op: {} needs an existing body", t.op.keyword()),
            "use op: new for the first body",
        ));
    }
}

/// The path of a sweep: a sketch (or some of its curves) or edges of earlier bodies.
fn check_sweep(n: &str, d: &SweepDef, seen: &BTreeMap<&str, &Feature>, out: &mut Vec<Issue>) {
    match &d.path {
        PathRef::Sketch { sketch, curves } => {
            if sketch == &d.profile.sketch {
                out.push(issue_h(
                    n,
                    "the path and the profile are in the same sketch",
                    "sketch the path in its own sketch, on a plane that crosses the profile",
                ));
            }
            for c in curves {
                check_sketch_entity(n, sketch, c, None, seen, out);
            }
            let mut listed = BTreeSet::new();
            for c in curves {
                if !listed.insert(c.as_str()) {
                    out.push(issue(n, format!("path curve '{c}' is listed twice")));
                }
            }
        }
        PathRef::Edges(edges) => {
            if edges.is_empty() {
                out.push(issue_h(n, "the path lists no edges", "select the edges to sweep along"));
            }
        }
    }
    if d.twist.is_some() && d.orientation == Orientation::Fixed {
        out.push(issue_h(
            n,
            "a twist needs the profile to follow the path",
            "set orientation: frenet, or remove the twist",
        ));
    }
}

/// A feature that modifies an existing body: the named body must be created by an earlier
/// feature, or (without a name) some body must exist.
fn check_body_target(
    n: &str,
    body: Option<&str>,
    body_available: bool,
    seen: &BTreeMap<&str, &Feature>,
    consumed: &BTreeMap<&str, &str>,
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
                    "target a feature with op: new, a mirror without merge, a move with copy, or an import",
                ));
            }
            check_not_consumed(n, b, consumed, out);
        }
        None if !body_available => {
            out.push(issue_h(n, "there is no body to work on", "create a body first, for example with extrude"));
        }
        None => {}
    }
}

/// A body that an earlier `combine` used as a tool no longer exists.
fn check_not_consumed(n: &str, body: &str, consumed: &BTreeMap<&str, &str>, out: &mut Vec<Issue>) {
    if let Some(by) = consumed.get(body) {
        out.push(issue_h(
            n,
            format!("body '{body}' was consumed by {by}"),
            format!("target the body {by} combined it into, or move this feature before {by}"),
        ));
    }
}

/// A datum plane reference must name an earlier `plane` feature (a missing one is reported
/// as a missing dependency).
fn check_plane(n: &str, plane: &PlaneRef, seen: &BTreeMap<&str, &Feature>, out: &mut Vec<Issue>) {
    if let PlaneRef::Datum(d) = plane
        && let Some(f) = seen.get(d.as_str())
        && !matches!(f.kind, FeatureKind::Plane(_))
    {
        out.push(issue_h(
            n,
            format!("'{d}' is not a datum plane"),
            "use XY, XZ, YZ, a face reference or a plane feature",
        ));
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
        check_sketch_entity(n, &p.sketch, region_entity(r), None, seen, out);
    }
}

/// The entity a region token starts from: `l5_r2` (region rank) names entity `l5`.
fn region_entity(token: &str) -> &str {
    match token.rsplit_once("_r") {
        Some((base, rank)) if !base.is_empty() && !rank.is_empty() && rank.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => token,
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
    // A pattern copy (`l5_2`) is named by its source entity plus the copy number.
    let copy_of = |id: &str| -> Option<String> {
        let (base, k) = id.rsplit_once('_')?;
        let ok = !k.is_empty()
            && k.bytes().all(|b| b.is_ascii_digit())
            && s.patterns.iter().any(|p| p.of.iter().any(|o| o == base));
        ok.then(|| base.to_string())
    };
    let entity = if s.entities.iter().any(|e| e.id == entity) {
        entity.to_string()
    } else {
        copy_of(entity).unwrap_or_else(|| entity.to_string())
    };
    let entity = entity.as_str();
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
        if let SkGeom::Spline { points, closed } = &e.geom {
            if points.len() < 3 {
                out.push(issue_h(n, format!("spline '{}' needs at least 3 points", e.id), "use a line for two points"));
            }
            if points.len() > MAX_SPLINE_POINTS {
                out.push(issue(n, format!("spline '{}' has more than {MAX_SPLINE_POINTS} points", e.id)));
            }
            let wrap = usize::from(*closed && points.len() > 1);
            let twice = points.windows(2).any(|w| matches!(w, [a, b] if a == b))
                || (wrap == 1 && points.first() == points.last());
            if twice {
                out.push(issue(n, format!("spline '{}' uses the same point twice in a row", e.id)));
            }
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
    let mut sourced: BTreeSet<&str> = BTreeSet::new();
    for p in &s.patterns {
        check_id(&p.id, &['g'], out);
        if !ids.insert(p.id.as_str()) {
            out.push(issue(n, format!("sketch id '{}' is used twice", p.id)));
        }
        if p.of.is_empty() {
            out.push(issue(n, format!("pattern '{}' has no source curves", p.id)));
        }
        if p.of.len() > MAX_PATTERN_SOURCES {
            out.push(issue(n, format!("pattern '{}' has more than {MAX_PATTERN_SOURCES} sources", p.id)));
        }
        for o in &p.of {
            match kinds.get(o.as_str()) {
                Some('l' | 'c' | 'a' | 's') => {
                    if !sourced.insert(o.as_str()) {
                        out.push(issue_h(
                            n,
                            format!("'{o}' is a source of two patterns (or twice in one)"),
                            "a curve can belong to one pattern only",
                        ));
                    }
                }
                Some(_) => {
                    out.push(issue(n, format!("pattern '{}': '{o}' is not a line, circle, arc or spline", p.id)))
                }
                None => out.push(issue(n, format!("pattern '{}' refers to unknown entity '{o}'", p.id))),
            }
        }
        if let SkPatternKind::Circular { center, .. } = &p.kind
            && kinds.get(center.as_str()) != Some(&'p')
        {
            out.push(issue(n, format!("pattern '{}': centre '{center}' is not a point of this sketch", p.id)));
        }
    }
}

/// Most source curves one sketch pattern may name.
pub const MAX_PATTERN_SOURCES: usize = 1000;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::parse;

    fn issues(src: &str) -> Vec<String> {
        validate(&parse(src).unwrap()).iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_joint_may_not_list_a_carried_body_twice() {
        let rect = "sketch S (plane: XY, next_id: 9) {\n  p1 = point(0, 0)\n  p2 = point(1, 0)\n  p3 = point(1, 1)\n  \
                    l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p1)\n}\n";
        let ext = |n: &str| format!("extrude {n} (profile: S, distance: 1 mm, op: new)\n");
        let src = |with: &str| {
            format!(
                "gusset 1\n{rect}{}{}{}joint J (type: rigid, body: A, with: [{with}], at: origin, to: origin)\n",
                ext("A"),
                ext("B"),
                ext("C")
            )
        };
        assert!(issues(&src("B, C")).is_empty(), "{:?}", issues(&src("B, C")));
        let v = issues(&src("B, C, B"));
        assert!(v.iter().any(|i| i.contains("'B' is listed twice in 'with'")), "{v:?}");
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
