//! Feature-tree import (ADR 0042): a `.gtree.json` written by an exporter plugin inside
//! Fusion, SolidWorks or Onshape becomes editable Gusset features: parameters, sketches,
//! extrudes, revolves, fillets and chamfers, generated as code-view text and validated like
//! any edit. Anything else is reported per feature.

use crate::{CmdCtx, CommandError, Session};
use gusset_kernel::{Region2, Seg2};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

/// Most features one tree may hold, and sketch entities per sketch.
const MAX_FEATURES: usize = 2000;
const MAX_ENTITIES: usize = 20_000;

#[derive(Debug, Clone, Deserialize)]
pub struct Tree {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub source: Value,
    #[serde(default)]
    pub parameters: Vec<TreeParam>,
    #[serde(default)]
    pub features: Vec<Value>,
    #[serde(default)]
    pub bodies: Vec<TreeBody>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TreeParam {
    pub name: String,
    #[serde(default)]
    pub expression: Option<String>,
    pub value: f64,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TreeBody {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub volume_mm3: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Plane {
    origin: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
}

#[derive(Debug, Clone, Deserialize)]
struct SkPoint {
    id: String,
    x: f64,
    y: f64,
    #[serde(default)]
    construction: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SkCurve {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    center: Option<String>,
    #[serde(default)]
    radius: Option<f64>,
    #[serde(default)]
    construction: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SkConstraint {
    #[serde(rename = "type")]
    kind: String,
    entities: Vec<String>,
    #[serde(default)]
    value: Option<f64>,
    #[serde(default)]
    parameter: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SketchF {
    name: String,
    plane: Plane,
    #[serde(default)]
    points: Vec<SkPoint>,
    #[serde(default)]
    curves: Vec<SkCurve>,
    #[serde(default)]
    constraints: Vec<SkConstraint>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProfilePick {
    point: [f64; 2],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Profiles {
    All(String),
    Picks(Vec<ProfilePick>),
}

#[derive(Debug, Clone, Deserialize)]
struct Axis {
    #[serde(default)]
    line: Option<String>,
    #[serde(default)]
    origin: Option<[f64; 3]>,
    #[serde(default)]
    direction: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
struct SolidF {
    name: String,
    sketch: String,
    #[serde(default)]
    profiles: Option<Profiles>,
    #[serde(default)]
    distance: Option<f64>,
    #[serde(default)]
    symmetric: bool,
    #[serde(default)]
    distance_parameter: Option<String>,
    #[serde(default)]
    angle: Option<f64>,
    #[serde(default)]
    axis: Option<Axis>,
    #[serde(default)]
    operation: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EdgePick {
    midpoint: [f64; 3],
}

#[derive(Debug, Clone, Deserialize)]
struct EdgeF {
    name: String,
    edges: Vec<EdgePick>,
    #[serde(default)]
    radius: Option<f64>,
    #[serde(default)]
    distance: Option<f64>,
}

/// Tier note for feature-tree imports.
pub const TIER: &str = "editable features from the source application's feature tree (exporter plugin)";

/// Parses and checks a tree file.
///
/// # Errors
/// `io` when it is not a version 1 `gusset-feature-tree` or holds too much.
pub fn parse(bytes: &[u8]) -> Result<Tree, CommandError> {
    let t: Tree = serde_json::from_slice(bytes)
        .map_err(|e| CommandError::new("io", format!("the feature tree cannot be read: {e}")))?;
    if t.format != "gusset-feature-tree" {
        return Err(CommandError::new("io", "this JSON file is not a Gusset feature tree")
            .hint("feature trees come from the exporter plugins (plugins/fusion, solidworks, onshape)"));
    }
    if t.version != 1 {
        return Err(CommandError::new("io", format!("feature tree version {} is not supported (1 is)", t.version)));
    }
    if t.features.len() > MAX_FEATURES {
        return Err(CommandError::new("too_large", format!("more than {MAX_FEATURES} features")));
    }
    Ok(t)
}

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    libm::sqrt(dot(a, a))
}

/// A number for the text: up to 6 decimals, no trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{:.6}", if v.abs() < 5e-7 { 0.0 } else { v });
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

/// A name for the text: `[A-Za-z0-9_]`, starting with a letter, at most 48 characters.
fn ident(s: &str, fallback: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(48).collect();
    if !out.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        out = format!("{fallback}_{out}");
    }
    out
}

/// Gusset's base plane frames: name, origin-free x and y axes.
const BASES: [(&str, V3, V3); 3] = [
    ("XY", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ("XZ", [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ("YZ", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
];

/// How a source sketch lands in Gusset: the plane argument, an optional datum plane line to
/// add first, the frame points are re-projected into, and whether its normal is reversed.
struct Placed {
    plane: String,
    datum: Option<String>,
    origin: V3,
    x: V3,
    y: V3,
    flipped: bool,
}

fn place_sketch(name: &str, p: &Plane) -> Option<Placed> {
    let n = cross(p.x_axis, p.y_axis);
    let nl = norm(n);
    if nl < 1e-9 {
        return None;
    }
    let n = n.map(|c| c / nl);
    for (base, x, y) in BASES {
        let bn = cross(x, y);
        let d = dot(n, bn);
        if (d.abs() - 1.0).abs() > 1e-9 {
            continue;
        }
        let offset = dot(p.origin, bn);
        let origin = bn.map(|c| c * offset);
        let (plane, datum) = if offset.abs() < 1e-9 {
            (base.to_string(), None)
        } else {
            let dn = format!("{name}_plane");
            (dn.clone(), Some(format!("plane {dn} (base: {base}, offset: {} mm)", num(offset))))
        };
        return Some(Placed { plane, datum, origin, x, y, flipped: d < 0.0 });
    }
    None
}

/// What the generator learnt about a source sketch.
#[derive(Clone)]
struct SketchMap {
    name: String,
    ids: HashMap<String, String>,
    flipped: bool,
    origin: V3,
    x: V3,
    y: V3,
    src: Plane,
}

impl SketchMap {
    /// A source sketch point (its own coordinates) in the Gusset sketch's.
    fn map(&self, u: f64, v: f64) -> [f64; 2] {
        let w = [
            self.src.origin[0] + u * self.src.x_axis[0] + v * self.src.y_axis[0],
            self.src.origin[1] + u * self.src.x_axis[1] + v * self.src.y_axis[1],
            self.src.origin[2] + u * self.src.x_axis[2] + v * self.src.y_axis[2],
        ];
        let d = sub(w, self.origin);
        [dot(d, self.x), dot(d, self.y)]
    }
}

/// Points along a region's loops (lines exact, curves sampled).
fn loop_points(edges: &[gusset_kernel::ProfileEdge]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for e in edges {
        match e.seg {
            Seg2::Line { a, .. } => out.push(a),
            Seg2::Arc { center, start, end, ccw } => {
                let r = libm::hypot(start[0] - center[0], start[1] - center[1]);
                let a0 = libm::atan2(start[1] - center[1], start[0] - center[0]);
                let mut sw = libm::atan2(end[1] - center[1], end[0] - center[0]) - a0;
                if ccw && sw <= 0.0 {
                    sw += std::f64::consts::TAU;
                }
                if !ccw && sw >= 0.0 {
                    sw -= std::f64::consts::TAU;
                }
                for i in 0..16 {
                    let t = a0 + sw * f64::from(i) / 16.0;
                    out.push([center[0] + r * libm::cos(t), center[1] + r * libm::sin(t)]);
                }
            }
            Seg2::Circle { center, radius } => {
                for i in 0..48 {
                    let t = std::f64::consts::TAU * f64::from(i) / 48.0;
                    out.push([center[0] + radius * libm::cos(t), center[1] + radius * libm::sin(t)]);
                }
            }
            Seg2::Bezier { p } => {
                for i in 0..16 {
                    let u = f64::from(i) / 16.0;
                    let w = [(1.0 - u).powi(3), 3.0 * u * (1.0 - u).powi(2), 3.0 * u * u * (1.0 - u), u.powi(3)];
                    let x = w.iter().zip(&p).map(|(w, q)| w * q[0]).sum();
                    let y = w.iter().zip(&p).map(|(w, q)| w * q[1]).sum();
                    out.push([x, y]);
                }
            }
        }
    }
    out
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let n = poly.len();
    let mut c = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

fn region_holds(r: &Region2, p: [f64; 2]) -> bool {
    inside(&loop_points(&r.outer.edges), p) && !r.holes.iter().any(|h| inside(&loop_points(&h.edges), p))
}

/// The generator's state.
struct Gen {
    text: String,
    taken: HashSet<String>,
    params: HashMap<String, String>,
    sketches: HashMap<String, SketchMap>,
    features: HashMap<String, String>,
    made: Vec<String>,
    skipped: Vec<Value>,
    warnings: Vec<String>,
}

impl Gen {
    fn fresh(&mut self, wanted: &str, fallback: &str) -> String {
        let base = ident(wanted, fallback);
        let mut name = base.clone();
        let mut i = 2;
        while self.taken.contains(&name.to_ascii_lowercase()) {
            name = format!("{base}_{i}");
            i += 1;
        }
        self.taken.insert(name.to_ascii_lowercase());
        name
    }

    fn skip(&mut self, name: &str, why: impl Into<String>) {
        self.skipped.push(json!({"feature": name, "reason": why.into()}));
    }

    /// A length or angle: the parameter it is driven by when known, else the number.
    fn value(&self, v: f64, unit: &str, parameter: Option<&String>, negate: bool) -> String {
        match parameter.and_then(|p| self.params.get(p)) {
            Some(p) if negate => format!("-({p})"),
            Some(p) => p.clone(),
            None => format!("{} {unit}", num(if negate { -v } else { v })),
        }
    }

    /// The document as text so far, evaluated (for regions and edges).
    fn evaluate(&self, s: &Session) -> Result<gusset_eval::Evaluation, CommandError> {
        let doc = gusset_doc::text::parse(&self.text)
            .map_err(|e| CommandError::new("evaluation", format!("generated text: {e}")))?;
        Ok(gusset_eval::Evaluator::new(s.kernel().clone()).evaluate(&doc, None, &()))
    }
}

/// Appends the tree's parameters and features to the session's document as one edit.
///
/// # Errors
/// An evaluation or commit error; features that cannot be imported are reported, not errors.
pub fn apply(s: &mut Session, ctx: &CmdCtx, tree: &Tree, file_name: &str) -> Result<Value, CommandError> {
    let doc = s.document(&ctx.actor)?;
    let mut taken: HashSet<String> = doc.features.iter().map(|f| f.name.to_ascii_lowercase()).collect();
    taken.extend(doc.params.iter().map(|p| p.name.to_ascii_lowercase()));
    for b in ["xy", "xz", "yz", "x", "y", "z"] {
        taken.insert(b.into());
    }
    let mut g = Gen {
        text: doc.to_text(),
        taken,
        params: HashMap::new(),
        sketches: HashMap::new(),
        features: HashMap::new(),
        made: Vec::new(),
        skipped: Vec::new(),
        warnings: Vec::new(),
    };
    // Parameters go after the header lines.
    let mut param_lines = String::new();
    for p in &tree.parameters {
        let name = g.fresh(&p.name, "p");
        let unit = match p.kind.as_deref() {
            Some("angle") => "deg",
            Some("number") => "",
            _ => "mm",
        };
        let known: HashSet<&String> = g.params.values().collect();
        let expr = p
            .expression
            .as_deref()
            .and_then(|e| gusset_units::expr::parse(e).ok().map(|x| (e, x)))
            .filter(|(_, x)| x.variables().iter().all(|v| known.contains(v)))
            .map(|(e, _)| e.to_string());
        let expr = match expr {
            Some(e) => e,
            None => {
                if p.expression.is_some() {
                    g.warnings.push(format!("parameter {}: its expression was replaced by its value", p.name));
                }
                format!("{} {unit}", num(p.value)).trim().to_string()
            }
        };
        let _ = writeln!(param_lines, "param {name} = {expr}");
        g.params.insert(p.name.clone(), name);
    }
    if !param_lines.is_empty() {
        let at =
            g.text.find("\nunits ").and_then(|i| g.text.get(i + 1..).and_then(|t| t.find('\n')).map(|j| i + 1 + j + 1));
        let at = at.or_else(|| g.text.find('\n').map(|i| i + 1)).unwrap_or(g.text.len());
        g.text.insert_str(at, &param_lines);
    }
    for f in &tree.features {
        let kind = f.get("type").and_then(Value::as_str).unwrap_or("");
        let src_name = f.get("name").and_then(Value::as_str).unwrap_or("Feature").to_string();
        let r = match kind {
            "sketch" => {
                serde_json::from_value::<SketchF>(f.clone()).map_err(|e| e.to_string()).and_then(|d| sketch(&mut g, &d))
            }
            "extrude" | "revolve" => serde_json::from_value::<SolidF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| solid(&mut g, s, &d, kind == "revolve")),
            "fillet" | "chamfer" => serde_json::from_value::<EdgeF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| edges(&mut g, s, &d, kind == "fillet")),
            "unsupported" => {
                let what = f.get("original_type").and_then(Value::as_str).unwrap_or("?");
                Err(format!("not exported by the plugin ({what})"))
            }
            other => Err(format!("feature type '{other}' is not in version 1")),
        };
        if let Err(why) = r {
            g.skip(&src_name, why);
        }
    }
    // Commit the whole as one edit, validated like document_set_text.
    let new = gusset_doc::text::parse(&g.text)
        .map_err(|e| CommandError::new("invalid_document", format!("the generated features do not parse: {e}")))?;
    let made = g.made.clone();
    let source = tree.source.clone();
    crate::commands::edit(s, ctx, &format!("Import {file_name}"), |doc, _| {
        *doc = new;
        Ok(Value::Null)
    })?;
    // Compare with the source's bodies.
    let ev = s.evaluate(&ctx.actor)?;
    let k = s.kernel().clone();
    let ours: f64 = ev.bodies.iter().filter_map(|b| k.mass_properties(&b.shape).ok()).map(|m| m.volume).sum();
    let theirs: f64 = tree.bodies.iter().filter_map(|b| b.volume_mm3).sum();
    let mut out = json!({
        "features": made,
        "skipped": g.skipped,
        "parameters": g.params.values().collect::<Vec<_>>(),
        "source_user_text": source,
        "tier": TIER,
    });
    if theirs > 0.0
        && let Some(o) = out.as_object_mut()
    {
        o.insert("volume_mm3".into(), json!({"source": theirs, "imported": ours}));
        if (ours - theirs).abs() > 1e-3 * theirs {
            g.warnings.push(format!("the model's volume {ours:.3} mm3 differs from the source's {theirs:.3} mm3"));
        }
    }
    if let Some(o) = out.as_object_mut() {
        o.insert("warnings_user_text".into(), json!(g.warnings));
    }
    Ok(out)
}

fn sketch(g: &mut Gen, d: &SketchF) -> Result<(), String> {
    if d.points.len() + d.curves.len() + d.constraints.len() > MAX_ENTITIES {
        return Err("too many sketch entities".into());
    }
    let name = g.fresh(&d.name, "Sketch");
    let placed = place_sketch(&name, &d.plane)
        .ok_or("its plane is not parallel to XY, XZ or YZ (tilted planes are not imported yet)")?;
    let mut map = SketchMap {
        name: name.clone(),
        ids: HashMap::new(),
        flipped: placed.flipped,
        origin: placed.origin,
        x: placed.x,
        y: placed.y,
        src: d.plane.clone(),
    };
    let mut n = 0usize;
    let mut next = |prefix: &str| {
        n += 1;
        format!("{prefix}{n}")
    };
    let mut body = String::new();
    for p in &d.points {
        let id = next("p");
        let [u, v] = map.map(p.x, p.y);
        let c = if p.construction { " construction" } else { "" };
        let _ = writeln!(body, "  {id} = point({}, {}){c}", num(u), num(v));
        map.ids.insert(p.id.clone(), id);
    }
    let pid = |map: &SketchMap, s: &Option<String>| -> Result<String, String> {
        let k = s.as_ref().ok_or("a curve lacks a point")?;
        map.ids.get(k).cloned().ok_or_else(|| format!("unknown sketch point '{k}'"))
    };
    for c in &d.curves {
        let line = match c.kind.as_str() {
            "line" => {
                let id = next("l");
                let s = format!("  {id} = line({}, {})", pid(&map, &c.start)?, pid(&map, &c.end)?);
                (id, s)
            }
            "circle" => {
                let id = next("c");
                let r = c.radius.filter(|r| r.is_finite() && *r > 0.0).ok_or("a circle without a radius")?;
                let s = format!("  {id} = circle({}, {})", pid(&map, &c.center)?, num(r));
                (id, s)
            }
            "arc" => {
                let id = next("a");
                // Counter-clockwise from start to end about the source normal: about a
                // reversed Gusset normal that runs from end to start.
                let (s0, s1) = if map.flipped { (&c.end, &c.start) } else { (&c.start, &c.end) };
                let s = format!("  {id} = arc({}, {}, {})", pid(&map, &c.center)?, pid(&map, s0)?, pid(&map, s1)?);
                (id, s)
            }
            other => return Err(format!("sketch curve type '{other}' is not imported")),
        };
        let cons = if c.construction { " construction" } else { "" };
        let _ = writeln!(body, "{}{cons}", line.1);
        map.ids.insert(c.id.clone(), line.0);
    }
    const KINDS: [&str; 16] = [
        "coincident",
        "on_line",
        "on_circle",
        "midpoint",
        "horizontal",
        "vertical",
        "parallel",
        "perpendicular",
        "tangent",
        "equal",
        "distance",
        "angle",
        "radius",
        "diameter",
        "fixed",
        "symmetric",
    ];
    for k in &d.constraints {
        if !KINDS.contains(&k.kind.as_str()) {
            continue;
        }
        let Some(ents) = k.entities.iter().map(|e| map.ids.get(e).cloned()).collect::<Option<Vec<_>>>() else {
            continue;
        };
        let mut args = ents.join(", ");
        if let Some(v) = k.value {
            let (unit, negate) = if k.kind == "angle" { ("deg", map.flipped) } else { ("mm", false) };
            let _ = write!(args, ", {}", g.value(v, unit, k.parameter.as_ref(), negate));
        }
        let id = next("k");
        let _ = writeln!(body, "  {id} = {}({args})", k.kind);
    }
    let mut text = String::new();
    if let Some(dl) = &placed.datum {
        if !g.taken.insert(placed.plane.to_ascii_lowercase()) {
            return Err(format!("the name '{}' for its plane is taken", placed.plane));
        }
        let _ = writeln!(text, "\n{dl}");
        g.made.push(placed.plane.clone());
    }
    let _ = write!(text, "\nsketch {name} (plane: {}) {{\n{body}}}\n", placed.plane);
    g.text.push_str(&text);
    g.made.push(name.clone());
    g.sketches.insert(d.name.clone(), map);
    Ok(())
}

fn solid(g: &mut Gen, s: &Session, d: &SolidF, revolve: bool) -> Result<(), String> {
    let sm = g.sketches.get(&d.sketch).ok_or_else(|| format!("its sketch '{}' was not imported", d.sketch))?.clone();
    let (sk_name, flipped) = (sm.name.clone(), sm.flipped);
    let profile = match &d.profiles {
        None => sk_name.clone(),
        Some(Profiles::All(a)) if a == "all" => sk_name.clone(),
        Some(Profiles::All(other)) => return Err(format!("profiles '{other}' is not \"all\" or a list")),
        Some(Profiles::Picks(picks)) => {
            let ev = g.evaluate(s).map_err(|e| e.message)?;
            let sr = ev.sketches.get(&sk_name).ok_or("its sketch did not solve")?;
            let infos = sr.regions.clone().map_err(|e| format!("its sketch has no regions: {e}"))?;
            let doc = gusset_doc::text::parse(&g.text).map_err(|e| e.to_string())?;
            let Some(gusset_doc::FeatureKind::Sketch(def)) = doc.feature(&sk_name).map(|f| &f.kind) else {
                return Err("its sketch is missing".into());
            };
            let mut ids = Vec::new();
            for pk in picks {
                let p = sm.map(pk.point[0], pk.point[1]);
                let hit = infos.iter().find(|ri| {
                    gusset_eval::sketch_regions(&sk_name, def, &ev.params, doc.units, std::slice::from_ref(&ri.id))
                        .ok()
                        .is_some_and(|rs| rs.iter().any(|r| region_holds(r, p)))
                });
                match hit {
                    Some(ri) => ids.push(ri.id.clone()),
                    None => return Err(format!("no region of its sketch holds the profile point {:?}", pk.point)),
                }
            }
            ids.dedup();
            format!("{sk_name}.regions({})", ids.join(", "))
        }
    };
    let op = match d.operation.as_deref().unwrap_or("new") {
        "new" => "new",
        "join" => "add",
        "cut" => "cut",
        "intersect" => "intersect",
        other => return Err(format!("operation '{other}' is not new, join, cut or intersect")),
    };
    let name = g.fresh(&d.name, if revolve { "Revolve" } else { "Extrude" });
    let mut args = vec![format!("profile: {profile}")];
    if revolve {
        let axis = d.axis.as_ref().ok_or("a revolve without an axis")?;
        let a = match (&axis.line, axis.origin, axis.direction) {
            (Some(l), _, _) => {
                format!("{sk_name}.{}", sm.ids.get(l).ok_or_else(|| format!("unknown axis line '{l}'"))?)
            }
            (None, Some(o), Some(dir)) if norm(o) < 1e-9 => {
                let dl = norm(dir).max(1e-300);
                match dir.map(|c| c / dl) {
                    [x, _, _] if (x.abs() - 1.0).abs() < 1e-9 => "X".into(),
                    [_, y, _] if (y.abs() - 1.0).abs() < 1e-9 => "Y".into(),
                    [_, _, z] if (z.abs() - 1.0).abs() < 1e-9 => "Z".into(),
                    _ => return Err("a revolve axis not along X, Y or Z is not imported yet".into()),
                }
            }
            _ => return Err("a revolve axis off the origin and not a sketch line is not imported yet".into()),
        };
        args.push(format!("axis: {a}"));
        args.push(format!("angle: {} deg", num(d.angle.unwrap_or(360.0))));
    } else {
        let dist = d.distance.filter(|v| v.is_finite() && v.abs() > 1e-9).ok_or("an extrude without a distance")?;
        args.push(format!("distance: {}", g.value(dist, "mm", d.distance_parameter.as_ref(), flipped)));
        if d.symmetric {
            args.push("symmetric: true".into());
        }
    }
    args.push(format!("op: {op}"));
    if op != "new"
        && let Some(t) = &d.target
    {
        let tn = g.features.get(t).ok_or_else(|| format!("its target '{t}' was not imported"))?;
        args.push(format!("body: {tn}"));
    }
    let kw = if revolve { "revolve" } else { "extrude" };
    let _ = writeln!(g.text, "\n{kw} {name} ({})", args.join(", "));
    // The new feature must build: a failure is reported and the line taken back out.
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| e.feature == name) {
        let line = format!("\n{kw} {name} ({})\n", args.join(", "));
        if let Some(i) = g.text.rfind(&line) {
            g.text.replace_range(i..i + line.len(), "\n");
        }
        return Err(format!("it does not build here: {}", e.message));
    }
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

fn edges(g: &mut Gen, s: &Session, d: &EdgeF, fillet: bool) -> Result<(), String> {
    let size = if fillet { d.radius } else { d.distance };
    let size = size.filter(|v| v.is_finite() && *v > 0.0).ok_or("no positive size")?;
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    // Each edge: the nearest one of the model as built so far, within 0.01 mm.
    let mut by_body: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pick in &d.edges {
        let mut best: Option<(f64, String, String)> = None;
        for b in &ev.bodies {
            for (i, e) in b.topology.edges.iter().enumerate() {
                let dd = norm(sub(e.midpoint, pick.midpoint));
                if best.as_ref().is_none_or(|x| dd < x.0)
                    && let Some(n) = b.names.edges.get(i)
                {
                    best = Some((dd, b.feature.clone(), n.to_string()));
                }
            }
        }
        match best {
            Some((dd, body, n)) if dd <= 0.01 => by_body.entry(body).or_default().push(n),
            _ => return Err(format!("no edge of the model has its midpoint at {:?}", pick.midpoint)),
        }
    }
    if by_body.len() != 1 {
        return Err("its edges are on several bodies".into());
    }
    let (body, names) = by_body.into_iter().next().ok_or("no edges")?;
    let _ = body;
    let name = g.fresh(&d.name, if fillet { "Fillet" } else { "Chamfer" });
    let (kw, key) = if fillet { ("fillet", "radius") } else { ("chamfer", "distance") };
    let line = format!("\n{kw} {name} (edges: [{}], {key}: {} mm)\n", names.join(", "), num(size));
    g.text.push_str(&line);
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| e.feature == name) {
        if let Some(i) = g.text.rfind(&line) {
            g.text.replace_range(i..i + line.len(), "\n");
        }
        return Err(format!("it does not build here: {}", e.message));
    }
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}
