//! Feature-tree import (ADR 0042): a `.gtree.json` written by an exporter plugin inside
//! Fusion, SolidWorks or Onshape becomes editable Gusset features: parameters, sketches,
//! extrudes, revolves, fillets and chamfers (version 1), holes, patterns, mirrors and shells
//! (version 2), generated as code-view text and validated like any edit. Anything else is
//! reported per feature.

mod v2;

use crate::{CmdCtx, CommandError, Session};
use gusset_kernel::{Region2, Seg2};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

/// Most features one tree may hold, and sketch entities per sketch.
const MAX_FEATURES: usize = 2000;
const MAX_ENTITIES: usize = 20_000;
/// Most parameters, sketch entities in all, and profile or edge picks per feature.
const MAX_PARAMS: usize = 5000;
const MAX_TOTAL_ENTITIES: usize = 200_000;
const MAX_PICKS: usize = 256;
/// Most profile picks of one extrude or revolve: a perforated board picks every hole (the
/// Perma-Proto boards 488 and 840). Each pick is one bounding-box test per region and a
/// point-in-polygon test only where it lands; the regions' polygons are made once.
const MAX_PROFILE_PICKS: usize = 4096;
/// Most model evaluations an import may run (each solid, fillet and chamfer is built to
/// check it); later features that need one are skipped. One evaluator keeps every feature
/// built so far, so an evaluation builds only the new feature (tens of milliseconds for the
/// rest); the corpus needs up to about 500 (MEMENTO 494, TFP401 423). [`MAX_IMPORT_TIME`]
/// bounds the building itself.
const MAX_EVALUATIONS: usize = 2000;
/// Longest an import may spend building the model before features that need a build are
/// skipped.
const MAX_IMPORT_TIME: std::time::Duration = std::time::Duration::from_secs(300);
/// Largest tree file (bytes).
pub const MAX_TREE_BYTES: usize = 32 << 20;
/// Longest untrusted string kept in the report, and most report entries.
const MAX_REPORT_CHARS: usize = 200;
const MAX_REPORT_ENTRIES: usize = 50;

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
    /// B-rep bytes a native importer read from the source file (a frozen base body, a
    /// mirrored stock body), named by index from `stored_body` features. Never read from
    /// the JSON: a tree file alone cannot bring in geometry.
    #[serde(skip)]
    pub stored_bodies: Vec<Vec<u8>>,
    /// Keep a feature that fails in the timeline, suppressed with its error as a note, instead
    /// of leaving it out (a native history imported although it differs from the file). Never
    /// read from the JSON.
    #[serde(skip)]
    pub keep_failed: bool,
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
    /// Ellipses: the unit direction of the major axis in sketch coordinates, and the
    /// semi-major and semi-minor lengths (mm).
    #[serde(default)]
    major: Option<[f64; 2]>,
    #[serde(default)]
    a: Option<f64>,
    #[serde(default)]
    b: Option<f64>,
    /// B-splines (`type: bspline`): degree, flat knot vector, pole point ids, weights (one per
    /// pole, or none for a polynomial curve) and periodicity (ADR 0042 amendment).
    #[serde(default)]
    degree: Option<u32>,
    #[serde(default)]
    knots: Option<Vec<f64>>,
    #[serde(default)]
    poles: Option<Vec<String>>,
    #[serde(default)]
    weights: Vec<f64>,
    #[serde(default)]
    periodic: bool,
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
    /// Written by the FreeCAD importer's retry: the arcs' points and circle radii are rounded to
    /// 6 decimals like the others (see [`coord`]).
    #[serde(default)]
    coarse: bool,
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

/// The body a feature changes, or (a cut through several bodies) the bodies.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Target {
    One(String),
    Many(Vec<String>),
}

impl Target {
    /// The one body, if there is one.
    fn one(&self) -> Result<&str, String> {
        match self {
            Target::One(t) => Ok(t),
            Target::Many(v) => match v.as_slice() {
                [t] => Ok(t),
                _ => Err("it changes several bodies".into()),
            },
        }
    }
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
    target: Option<Target>,
    /// `false` keeps the boolean's split faces (FreeCAD's `Refine = false`, ADR 0042).
    #[serde(default = "refine_default")]
    refine: bool,
    /// Extrude: `"blind"` (default, `distance`) or `"to_face"` (the sweep ends on `face`).
    #[serde(default)]
    end_condition: Option<String>,
    /// Extrude to a face: a point on the face (and its outward normal) as it is before this
    /// feature, like a hole's `face`.
    #[serde(default)]
    face: Option<v2::FacePick>,
    /// Extrude to a face: mm past the face along the sweep (negative: short of it).
    #[serde(default)]
    offset: Option<f64>,
    #[serde(default)]
    offset_parameter: Option<String>,
    /// Extrude to a face: sweep against the source sketch's normal.
    #[serde(default)]
    reverse: bool,
    /// Extrude: the side walls lean by this angle (degrees) from the sweep direction;
    /// negative narrows the solid away from its sketch (Fusion's taper angle).
    #[serde(default)]
    taper_deg: Option<f64>,
    /// Extrude to both sides, each with its own taper (mm, signed along the sketch normal as
    /// `distance` is, on the other side of the sketch plane): the second side's reach.
    #[serde(default)]
    distance2: Option<f64>,
    /// Extrude to both sides: the second side's taper (degrees, as `taper_deg`).
    #[serde(default)]
    taper2_deg: Option<f64>,
}

/// The default of the `refine` field of a body-combining tree feature: unified.
pub(super) fn refine_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
struct EdgePick {
    midpoint: [f64; 3],
    /// Points along the source edge, when a source may merge several of Gusset's edges into
    /// one (FreeCAD's refine): the model's edges lying on it stand in for it.
    #[serde(default)]
    path: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
struct EdgeF {
    name: String,
    #[serde(default)]
    edges: Vec<EdgePick>,
    /// Fillet, full round (version 2): the middle face between two parallel faces, replaced
    /// by a half cylinder; instead of `edges` and `radius`.
    #[serde(default)]
    full_round: Option<v2::FacePick>,
    #[serde(default)]
    radius: Option<f64>,
    #[serde(default)]
    distance: Option<f64>,
    /// Chamfer, unequal (mm): the distance on the other face.
    #[serde(default)]
    distance2: Option<f64>,
    /// Chamfer, unequal (degrees, 0 to 90 exclusive): the angle from the reference face.
    #[serde(default)]
    angle: Option<f64>,
    /// Chamfer, unequal: the reference face, the one `distance` runs along (like a hole's
    /// `face`). Edges it does not border take the default rule, with a warning.
    #[serde(default)]
    face: Option<v2::FacePick>,
    /// Fillet, chamfer: the body the edges are on (the tree name of the feature that made it),
    /// for edges that lie on edges of another body.
    #[serde(default)]
    on_body: Option<String>,
}

/// Tier note for feature-tree imports.
pub const TIER: &str = "editable features from the source application's feature tree (exporter plugin)";

/// Parses and checks a tree file.
///
/// # Errors
/// `io` when it is not a version 1 or 2 `gusset-feature-tree` or holds too much.
pub fn parse(bytes: &[u8]) -> Result<Tree, CommandError> {
    if bytes.len() > MAX_TREE_BYTES {
        return Err(CommandError::new("too_large", format!("a feature tree is at most {} MiB", MAX_TREE_BYTES >> 20)));
    }
    let t: Tree = serde_json::from_slice(bytes)
        .map_err(|e| CommandError::new("io", format!("the feature tree cannot be read: {e}")))?;
    if t.format != "gusset-feature-tree" {
        return Err(CommandError::new("io", "this JSON file is not a Gusset feature tree")
            .hint("feature trees come from the exporter plugins (plugins/fusion, solidworks, onshape)"));
    }
    if !(1..=2).contains(&t.version) {
        return Err(CommandError::new(
            "io",
            format!("feature tree version {} is not supported (1 and 2 are)", t.version),
        ));
    }
    if t.features.len() > MAX_FEATURES {
        return Err(CommandError::new("too_large", format!("more than {MAX_FEATURES} features")));
    }
    if t.features.is_empty() && t.parameters.is_empty() {
        return Err(CommandError::new("io", "the feature tree is empty"));
    }
    if t.parameters.len() > MAX_PARAMS {
        return Err(CommandError::new("too_large", format!("more than {MAX_PARAMS} parameters")));
    }
    let entities: usize = t
        .features
        .iter()
        .map(|f| {
            ["points", "curves", "constraints"]
                .iter()
                .map(|k| f.get(k).and_then(Value::as_array).map_or(0, Vec::len))
                .sum::<usize>()
        })
        .sum();
    if entities > MAX_TOTAL_ENTITIES {
        return Err(CommandError::new("too_large", format!("more than {MAX_TOTAL_ENTITIES} sketch entities in all")));
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

/// A finite number, else an error naming it.
fn finite(v: f64, what: &str) -> Result<f64, String> {
    if v.is_finite() && v.abs() < 1e12 { Ok(v) } else { Err(format!("{what} is not a usable number")) }
}

/// Truncates untrusted text for the report.
fn short(s: &str) -> String {
    s.chars().take(MAX_REPORT_CHARS).collect()
}

/// A number for the text: up to 6 decimals, no trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{:.6}", if v.abs() < 5e-7 { 0.0 } else { v });
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

/// A coordinate or radius of a sketch arc or circle: 8 decimals, no trailing zeros. An arc's start
/// and end points are separate points, so rounding them to 6 decimals leaves them up to 5e-7 off
/// one circle, which the solver then repairs by moving points (the radii of two pads of one
/// cylinder then differ by more than the kernel's tolerance, and the faces stay apart: Page_020).
fn coord(v: f64) -> String {
    let s = format!("{:.8}", if v.abs() < 5e-9 { 0.0 } else { v });
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

/// How a source sketch lands in Gusset: the plane argument, datum planes to add first (name,
/// line), the frame points are re-projected into, and whether its normal is reversed.
struct Placed {
    plane: String,
    datums: Vec<(String, String)>,
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
        let (plane, datums) = if offset.abs() < 1e-9 {
            (base.to_string(), Vec::new())
        } else {
            let dn = format!("{name}_plane");
            let line = format!("plane {dn} (base: {base}, offset: {} mm)", num(offset));
            (dn.clone(), vec![(dn, line)])
        };
        return Some(Placed { plane, datums, origin, x, y, flipped: d < 0.0 });
    }
    tilted(name, p.origin, n)
}

/// A plane at any angle: XY turned about X by a, then about Y by b (so its normal is
/// (cos a sin b, -sin a, cos a cos b)), then offset along the normal. The frame is worked out
/// from the angles as written, the way the evaluator turns datum planes.
fn tilted(name: &str, origin: V3, n: V3) -> Option<Placed> {
    let a = libm::asin((-n[1]).clamp(-1.0, 1.0)).to_degrees();
    let b = libm::atan2(n[0], n[2]).to_degrees();
    let (a_txt, b_txt) = (fine_deg(a), fine_deg(b));
    let (a, b) = (a_txt.parse::<f64>().ok()?.to_radians(), b_txt.parse::<f64>().ok()?.to_radians());
    let turn = |v: V3, axis: V3, t: f64| {
        let (sn, cs) = (libm::sin(t), libm::cos(t));
        let c = cross(axis, v);
        let k = dot(axis, v) * (1.0 - cs);
        [0, 1, 2].map(|i| {
            let (vi, ci, ai) = (v.get(i), c.get(i), axis.get(i));
            vi.zip(ci).zip(ai).map_or(0.0, |((vi, ci), ai)| vi * cs + ci * sn + ai * k)
        })
    };
    let (mut x, mut y) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let (mut datums, mut base) = (Vec::new(), "XY".to_string());
    for (i, (axis, t, txt, v)) in
        [("X", a, &a_txt, [1.0, 0.0, 0.0]), ("Y", b, &b_txt, [0.0, 1.0, 0.0])].into_iter().enumerate()
    {
        if t.abs() < 1e-15 {
            continue;
        }
        x = turn(x, v, t);
        y = turn(y, v, t);
        let dn = format!("{name}_tilt{}", i + 1);
        datums.push((dn.clone(), format!("plane {dn} (base: {base}, axis: {axis}, angle: {txt} deg)")));
        base = dn;
    }
    let normal = cross(x, y);
    let offset = dot(origin, normal);
    let o_txt = fine_deg(offset);
    let offset = o_txt.parse::<f64>().ok()?;
    if offset.abs() > 1e-9 {
        let dn = format!("{name}_plane");
        datums.push((dn.clone(), format!("plane {dn} (base: {base}, offset: {o_txt} mm)")));
        base = dn;
    }
    let flipped = dot(normal, n) < 0.0;
    Some(Placed { plane: base, datums, origin: normal.map(|c| c * offset), x, y, flipped })
}

/// 12 decimals: angles and offsets that put a tilted plane where the source has it.
fn fine_deg(v: f64) -> String {
    let s = format!("{:.12}", if v.abs() < 5e-13 { 0.0 } else { v });
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
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
                // At least 16 chords, and at most 3.75 degrees each: a board outline that is a
                // circle less a notch, one arc of 343 degrees, lay up to 0.44 mm outside its 16
                // chords, and a pick in the 0.5 mm between it and a hole near the rim fell
                // outside the region (3000's Extrude1).
                let turns = sw.abs() / std::f64::consts::TAU * 96.0;
                let n = (16..=96u32).find(|k| f64::from(*k) >= turns).unwrap_or(96);
                for i in 0..n {
                    let t = a0 + sw * f64::from(i) / f64::from(n);
                    out.push([center[0] + r * libm::cos(t), center[1] + r * libm::sin(t)]);
                }
            }
            Seg2::Circle { center, radius } => {
                for i in 0..48 {
                    let t = std::f64::consts::TAU * f64::from(i) / 48.0;
                    out.push([center[0] + radius * libm::cos(t), center[1] + radius * libm::sin(t)]);
                }
            }
            Seg2::Ellipse { .. } | Seg2::EllipseArc { .. } => out.extend(e.seg.ellipse_points(48)),
            Seg2::RationalBezier { .. } => out.extend(e.seg.rational_points(16)),
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

/// A region's loops as polygons, made once so that many profile picks (a perforated board's
/// hundreds of holes) do not tessellate every loop again for each pick.
struct RegionPolys {
    outer: Vec<[f64; 2]>,
    /// The outer polygon's box: most picks are outside it.
    lo: [f64; 2],
    hi: [f64; 2],
    holes: Vec<Vec<[f64; 2]>>,
}

impl RegionPolys {
    fn new(r: &Region2) -> Self {
        let outer = loop_points(&r.outer.edges);
        let (lo, hi) = outer.iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), q| {
            ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])])
        });
        Self { outer, lo, hi, holes: r.holes.iter().map(|h| loop_points(&h.edges)).collect() }
    }

    fn holds(&self, p: [f64; 2]) -> bool {
        let [x, y] = p;
        (self.lo[0]..=self.hi[0]).contains(&x)
            && (self.lo[1]..=self.hi[1]).contains(&y)
            && inside(&self.outer, p)
            && !self.holes.iter().any(|h| inside(h, p))
    }
}

/// The generator's state.
struct Gen {
    text: String,
    taken: HashSet<String>,
    params: HashMap<String, String>,
    sketches: HashMap<String, SketchMap>,
    features: HashMap<String, String>,
    /// Source names of bodies a `remove_body` dropped -> the removing feature's name.
    removed: HashMap<String, String>,
    made: Vec<String>,
    skipped: Vec<Value>,
    warnings: Vec<String>,
    /// The next suffix to try per base name (no quadratic retries).
    suffix: HashMap<String, usize>,
    evaluations: usize,
    /// Source feature name -> its index in the tree (the features a mirror can copy).
    index: HashMap<String, usize>,
    /// Sketch entities generated so far (mirrors copy sketches: review of v2).
    entities: usize,
    started: std::time::Instant,
    /// One evaluator for the whole import: its cache keeps every feature built so far, so
    /// each trial build only builds what the new feature adds.
    evaluator: gusset_eval::Evaluator,
}

impl Gen {
    fn fresh(&mut self, wanted: &str, fallback: &str) -> String {
        let mut base = ident(wanted, fallback);
        // Unit and built-in names (mm, pi, sin) cannot name anything.
        if gusset_units::params::validate_name(&base).is_err() {
            base = format!("{fallback}_{base}");
        }
        let mut name = base.clone();
        let i = self.suffix.entry(base.clone()).or_insert(2);
        while self.taken.contains(&name.to_ascii_lowercase()) {
            name = format!("{base}_{i}");
            *i += 1;
        }
        self.taken.insert(name.to_ascii_lowercase());
        name
    }

    /// The body feature a source name stands for; a removed body says so.
    fn body_named(&self, t: &str, what: &str) -> Result<String, String> {
        if let Some(by) = self.removed.get(t) {
            return Err(format!("its {what} '{}' was removed by '{}'", short(t), short(by)));
        }
        self.features.get(t).cloned().ok_or_else(|| format!("its {what} '{}' was not imported", short(t)))
    }

    fn skip(&mut self, name: &str, why: impl Into<String>) {
        if self.skipped.len() < MAX_REPORT_ENTRIES {
            self.skipped.push(json!({"feature": short(name), "reason": short(&why.into())}));
        }
    }

    fn warn(&mut self, w: String) {
        if self.warnings.len() < MAX_REPORT_ENTRIES {
            self.warnings.push(short(&w));
        }
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
    fn evaluate(&mut self, s: &Session) -> Result<gusset_eval::Evaluation, CommandError> {
        self.evaluations += 1;
        if self.evaluations > MAX_EVALUATIONS {
            return Err(CommandError::new("too_large", "the import has built the model too many times"));
        }
        if self.started.elapsed() > MAX_IMPORT_TIME {
            return Err(CommandError::new("too_large", "the import has taken too long"));
        }
        let doc = gusset_doc::text::parse(&self.text)
            .map_err(|e| CommandError::new("evaluation", format!("generated text: {e}")))?;
        Ok(self.evaluator.evaluate(&doc, None, &s.blobs()))
    }
}

/// Appends the tree's parameters and features to the session's document as one edit.
///
/// # Errors
/// An evaluation or commit error; features that cannot be imported are reported, not errors.
pub fn apply(s: &mut Session, ctx: &CmdCtx, tree: &Tree, file_name: &str) -> Result<Value, CommandError> {
    // The trial builds use the session's own evaluator: they start from what it has built of the
    // document already, and the session keeps them (and its cancel flag and limits apply).
    let mut evaluator = gusset_eval::Evaluator::new(s.kernel().clone());
    s.swap_evaluator(&mut evaluator);
    let r = apply_with(s, ctx, tree, file_name, &mut evaluator);
    s.swap_evaluator(&mut evaluator);
    r
}

/// [`apply`] with the caller's evaluator for the trial builds, so that a caller importing
/// several prefixes of one tree (Fusion's state check) builds each feature once. The commit builds
/// with it too.
///
/// # Errors
/// As [`apply`].
pub fn apply_with(
    s: &mut Session,
    ctx: &CmdCtx,
    tree: &Tree,
    file_name: &str,
    evaluator: &mut gusset_eval::Evaluator,
) -> Result<Value, CommandError> {
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
        removed: HashMap::new(),
        features: HashMap::new(),
        made: Vec::new(),
        skipped: Vec::new(),
        warnings: Vec::new(),
        suffix: HashMap::new(),
        evaluations: 0,
        index: HashMap::new(),
        entities: 0,
        started: std::time::Instant::now(),
        evaluator: std::mem::replace(evaluator, gusset_eval::Evaluator::new(s.kernel().clone())),
    };
    // Parameters go after the header lines; each is evaluated with those before it, so one
    // that cannot be (1/0, a unit clash) falls back to its value (dogfooding F7-4).
    struct Known(HashMap<String, gusset_units::Quantity>);
    impl gusset_units::expr::Scope for Known {
        fn lookup(&self, name: &str) -> Option<gusset_units::Quantity> {
            self.0.get(name).copied()
        }
    }
    let mut known = Known(HashMap::new());
    let mut param_lines = String::new();
    for p in &tree.parameters {
        if g.params.contains_key(&p.name) {
            g.warn(format!("parameter {} appears twice: the later one is imported under a new name", p.name));
        }
        let name = g.fresh(&p.name, "p");
        let unit = match p.kind.as_deref() {
            Some("angle") => "deg",
            Some("number") => "",
            _ => "mm",
        };
        // The expression, its variables renamed to the parameters made so far, printed
        // canonically (one line); else the value.
        let expr = p
            .expression
            .as_deref()
            .filter(|e| e.len() <= 1000)
            .and_then(|e| gusset_units::expr::parse(e).ok())
            .and_then(|x| {
                x.variables().iter().try_fold(x.clone(), |acc, v| g.params.get(v).map(|to| acc.rename_var(v, to)))
            })
            .map(|x| x.to_string())
            .filter(|e| {
                gusset_units::expr::eval_str(e, &known).is_ok_and(|q| q.value.is_finite() && q.value.abs() < 1e12)
            });
        let expr = match expr {
            Some(e) => e,
            None => {
                if p.expression.is_some() {
                    g.warn(format!("parameter {}: its expression was replaced by its value", p.name));
                }
                let Ok(v) = finite(p.value, "the value") else {
                    g.warn(format!("parameter {}: no usable value, left out", p.name));
                    continue;
                };
                format!("{} {unit}", num(v)).trim().to_string()
            }
        };
        match gusset_units::expr::eval_str(&expr, &known) {
            Ok(q) => {
                known.0.insert(name.clone(), q);
            }
            Err(_) => {
                g.warn(format!("parameter {}: left out (its value does not evaluate)", p.name));
                continue;
            }
        }
        let _ = writeln!(param_lines, "param {name} = {expr}");
        g.params.insert(p.name.clone(), name);
    }
    if !param_lines.is_empty() {
        let at =
            g.text.find("\nunits ").and_then(|i| g.text.get(i + 1..).and_then(|t| t.find('\n')).map(|j| i + 1 + j + 1));
        let at = at.or_else(|| g.text.find('\n').map(|i| i + 1)).unwrap_or(g.text.len());
        g.text.insert_str(at, &param_lines);
    }
    for (fi, f) in tree.features.iter().enumerate() {
        let kind = f.get("type").and_then(Value::as_str).unwrap_or("");
        let src_name = f.get("name").and_then(Value::as_str).unwrap_or("Feature").to_string();
        // Whatever a feature added is taken back out if it fails.
        let (text_len, made_len) = (g.text.len(), g.made.len());
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
            "hole" | "pattern_linear" | "pattern_circular" | "mirror" | "shell" | "sweep" | "loft" | "stored_body"
            | "move_body" | "draft" | "scale_body" | "remove_body" | "offset_face" | "split"
                if tree.version < 2 =>
            {
                Err(format!("feature type '{kind}' is in version 2 files; this file says version 1"))
            }
            "hole" => serde_json::from_value::<v2::HoleF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::hole(&mut g, s, &d)),
            "pattern_linear" | "pattern_circular" => serde_json::from_value::<v2::PatternF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::pattern(&mut g, s, &d, kind == "pattern_circular")),
            "mirror" => serde_json::from_value::<v2::MirrorF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::mirror(&mut g, s, &d, &tree.features)),
            "combine" => serde_json::from_value::<v2::CombineF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::combine(&mut g, s, &d)),
            "loft" => serde_json::from_value::<v2::LoftF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::loft(&mut g, s, &d)),
            "sweep" => serde_json::from_value::<v2::SweepF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::sweep(&mut g, s, &d)),
            "stored_body" => serde_json::from_value::<v2::StoredBodyF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::stored_body(&mut g, s, &d, &tree.stored_bodies)),
            "move_body" => serde_json::from_value::<v2::MoveBodyF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::move_body(&mut g, s, &d)),
            "scale_body" => serde_json::from_value::<v2::ScaleBodyF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::scale_body(&mut g, s, &d)),
            "remove_body" => serde_json::from_value::<v2::RemoveBodyF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::remove_body(&mut g, s, &d)),
            "split" => serde_json::from_value::<v2::SplitF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::split(&mut g, s, &d)),
            "draft" => serde_json::from_value::<v2::DraftF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::draft(&mut g, s, &d)),
            "shell" => serde_json::from_value::<v2::ShellF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::shell(&mut g, s, &d)),
            "offset_face" => serde_json::from_value::<v2::OffsetFaceF>(f.clone())
                .map_err(|e| e.to_string())
                .and_then(|d| v2::offset_face(&mut g, s, &d)),
            "unsupported" => {
                let what = f.get("original_type").and_then(Value::as_str).unwrap_or("?");
                Err(format!("not exported by the plugin ({what})"))
            }
            other => Err(format!("feature type '{}' is not known", short(other))),
        };
        match r {
            Ok(()) => {
                if matches!(kind, "sketch" | "extrude" | "revolve" | "hole" | "fillet" | "chamfer" | "draft") {
                    g.index.insert(src_name.clone(), fi);
                }
            }
            Err(why) => {
                if !(tree.keep_failed && keep_suppressed(&mut g, text_len, &why)) {
                    g.text.truncate(text_len);
                }
                g.made.truncate(made_len);
                g.skip(&src_name, why);
            }
        }
    }
    // The caller keeps the evaluator and what it has built.
    std::mem::swap(evaluator, &mut g.evaluator);
    // Commit the whole as one edit, validated like document_set_text.
    let new = gusset_doc::text::parse(&g.text)
        .map_err(|e| CommandError::new("invalid_document", format!("the generated features do not parse: {e}")))?;
    let made = g.made.clone();
    // The source block, unless it is large (untrusted, echoed back).
    let source = if serde_json::to_string(&tree.source).map_or(usize::MAX, |t| t.len()) <= 2000 {
        tree.source.clone()
    } else {
        Value::Null
    };
    // The commit and the comparison below build with the trial evaluator: every feature is in its
    // cache, so nothing is built twice and the bodies are the very shapes the trials measured.
    s.swap_evaluator(evaluator);
    let committed = crate::commands::edit(s, ctx, &format!("Import {file_name}"), |doc, _| {
        *doc = new;
        Ok(Value::Null)
    })
    .and_then(|_| s.evaluate(&ctx.actor));
    s.swap_evaluator(evaluator);
    // Compare with the source's bodies.
    let ev = committed?;
    let k = s.kernel().clone();
    // Only the bodies the import made (the document may hold others: F7-6), the copies its
    // patterns keep apart included (their feature is the copy's name).
    let made_by = |f: &str| {
        made.iter().any(|m| m == f) || gusset_doc::pattern_copy_of(f).is_some_and(|(p, _)| made.iter().any(|m| m == p))
    };
    let made_bodies: Vec<_> = ev.bodies.iter().filter(|b| made_by(&b.feature)).collect();
    let ours: f64 = made_bodies.iter().filter_map(|b| k.mass_properties(&b.shape).ok()).map(|m| m.volume).sum();
    // A join the source did not apply (or the other way round) changes the count (F9-10).
    if !tree.bodies.is_empty() && made_bodies.len() != tree.bodies.len() {
        g.warn(format!("the import made {} bodies; the source lists {}", made_bodies.len(), tree.bodies.len()));
    }
    let theirs: f64 = tree.bodies.iter().filter_map(|b| b.volume_mm3).sum();
    let mut out = json!({
        "features": made,
        "skipped_user_text": g.skipped,
        "parameters": g.params.values().collect::<Vec<_>>(),
        "source_user_text": source,
        "tier": TIER,
    });
    if theirs > 0.0
        && let Some(o) = out.as_object_mut()
    {
        o.insert("volume_mm3".into(), json!({"source": theirs, "imported": ours}));
        if (ours - theirs).abs() > 1e-3 * theirs {
            g.warn(format!("the model's volume {ours:.3} mm3 differs from the source's {theirs:.3} mm3"));
        }
    }
    if let Some(o) = out.as_object_mut() {
        o.insert("warnings_user_text".into(), json!(g.warnings));
    }
    Ok(out)
}

/// Keeps what a failed feature wrote after `text_len` as suppressed features noting `why`, when
/// it is whole statements that parse. Whether it was kept.
fn keep_suppressed(g: &mut Gen, text_len: usize, why: &str) -> bool {
    let (Some(before), Some(added)) = (g.text.get(..text_len), g.text.get(text_len..)) else { return false };
    if added.trim().is_empty() {
        return false;
    }
    let (Ok(old), Ok(mut doc)) = (gusset_doc::text::parse(before), gusset_doc::text::parse(&g.text)) else {
        return false;
    };
    let had: HashSet<&str> = old.features.iter().map(|f| f.name.as_str()).collect();
    let why: String = short(why).chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut any = false;
    for f in doc.features.iter_mut().filter(|f| !had.contains(f.name.as_str())) {
        f.suppressed = true;
        f.note = Some(format!("Import: this feature failed to rebuild: {why}"));
        any = true;
    }
    if !any {
        return false;
    }
    g.text = doc.to_text();
    true
}

fn sketch(g: &mut Gen, d: &SketchF) -> Result<(), String> {
    let count = d.points.len() + d.curves.len() + d.constraints.len();
    if count > MAX_ENTITIES {
        return Err("too many sketch entities".into());
    }
    g.entities += count;
    if g.entities > MAX_TOTAL_ENTITIES {
        return Err(format!("more than {MAX_TOTAL_ENTITIES} sketch entities generated in all"));
    }
    for v in d.plane.origin.iter().chain(&d.plane.x_axis).chain(&d.plane.y_axis) {
        finite(*v, "its plane")?;
    }
    let name = g.fresh(&d.name, "Sketch");
    let placed = place_sketch(&name, &d.plane).ok_or("its plane is degenerate")?;
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
    let mut points: HashSet<String> = HashSet::new();
    // Points that an arc runs through keep more digits (see `coord`).
    let fine = if d.coarse { num } else { coord };
    let mut on_arc: HashSet<&str> = d
        .curves
        .iter()
        .filter(|c| matches!(c.kind.as_str(), "arc" | "ellipse_arc"))
        .flat_map(|c| [&c.start, &c.end, &c.center])
        .filter_map(|p| p.as_deref())
        .collect();
    // So do the poles of B-splines (they define an exact curve).
    on_arc.extend(d.curves.iter().filter_map(|c| c.poles.as_ref()).flatten().map(String::as_str));
    for p in &d.points {
        let id = next("p");
        let [u, v] = map.map(finite(p.x, "a point")?, finite(p.y, "a point")?);
        let (u, v) = (finite(u, "a point")?, finite(v, "a point")?);
        let c = if p.construction { " construction" } else { "" };
        let w = if on_arc.contains(p.id.as_str()) { fine } else { num };
        let _ = writeln!(body, "  {id} = point({}, {}){c}", w(u), w(v));
        map.ids.insert(p.id.clone(), id);
        points.insert(p.id.clone());
    }
    // Curves end on points only (a curve id is not a point).
    let pid = |map: &SketchMap, s: &Option<String>| -> Result<String, String> {
        let k = s.as_ref().ok_or("a curve lacks a point")?;
        points
            .contains(k)
            .then(|| map.ids.get(k).cloned())
            .flatten()
            .ok_or_else(|| format!("unknown sketch point '{}'", short(k)))
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
                let r =
                    c.radius.filter(|r| r.is_finite() && *r > 0.0 && *r < 1e12).ok_or("a circle without a radius")?;
                let s = format!("  {id} = circle({}, {})", pid(&map, &c.center)?, fine(r));
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
            kind @ ("ellipse" | "ellipse_arc") => {
                let (Some(a), Some(b), Some(m)) = (c.a, c.b, c.major) else {
                    return Err("an ellipse needs major, a and b".into());
                };
                let ok = |v: f64| v.is_finite() && v > 0.0 && v < 1e12;
                if !(ok(a) && ok(b)) || !m.iter().all(|v| v.is_finite()) || libm::hypot(m[0], m[1]) < 1e-9 {
                    return Err("an ellipse needs positive semi-axes and a major direction".into());
                }
                // The axis direction in the Gusset sketch's frame.
                let (o, t) = (map.map(0.0, 0.0), map.map(m[0], m[1]));
                let angle = fine_deg(libm::atan2(t[1] - o[1], t[0] - o[0]));
                let centre = pid(&map, &c.center)?;
                if kind == "ellipse" {
                    let id = next("o");
                    (id.clone(), format!("  {id} = ellipse({centre}, {angle}, {}, {})", fine(a), fine(b)))
                } else {
                    let id = next("q");
                    // Counter-clockwise about the source normal, as for arcs.
                    let (s0, s1) = if map.flipped { (&c.end, &c.start) } else { (&c.start, &c.end) };
                    let s = format!(
                        "  {id} = ellipse_arc({centre}, {angle}, {}, {}, {}, {})",
                        fine(a),
                        fine(b),
                        pid(&map, s0)?,
                        pid(&map, s1)?
                    );
                    (id, s)
                }
            }
            "bspline" => {
                let (Some(degree), Some(knots), Some(poles)) = (c.degree, &c.knots, &c.poles) else {
                    return Err("a B-spline needs degree, knots and poles".into());
                };
                if poles.is_empty() || poles.len() > gusset_doc::MAX_BSPLINE_POLES {
                    return Err(format!("a B-spline needs 1 to {} poles", gusset_doc::MAX_BSPLINE_POLES));
                }
                if knots.len() > poles.len() + gusset_doc::MAX_BSPLINE_DEGREE as usize + 1
                    || c.weights.len() > poles.len()
                {
                    return Err("a B-spline has more knots or weights than its poles allow".into());
                }
                for v in knots.iter().chain(&c.weights) {
                    finite(*v, "a B-spline's knots or weights")?;
                }
                let ones = vec![1.0; poles.len()];
                let w = if c.weights.is_empty() { &ones } else { &c.weights };
                gusset_sketch::bspline::check(degree as usize, poles.len(), knots, w, c.periodic)?;
                // Mirrored planes map the poles; knots and weights stay (the curve's direction
                // of travel does not matter to a region).
                let ps = poles.iter().map(|q| pid(&map, &Some(q.clone()))).collect::<Result<Vec<_>, _>>()?;
                let exact = |v: &[f64]| v.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(", ");
                let w =
                    if c.weights.is_empty() { String::new() } else { format!(", weights: [{}]", exact(&c.weights)) };
                let id = next("b");
                let s = format!(
                    "  {id} = bspline(degree: {degree}, poles: [{}], knots: [{}]{w}){}",
                    ps.join(", "),
                    exact(knots),
                    if c.periodic { " periodic" } else { "" }
                );
                (id, s)
            }
            other => return Err(format!("sketch curve type '{}' is not imported", short(other))),
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
    let (mut unknown, mut dangling) = (0usize, 0usize);
    for k in &d.constraints {
        if !KINDS.contains(&k.kind.as_str()) {
            unknown += 1;
            continue;
        }
        let Some(ents) = k.entities.iter().map(|e| map.ids.get(e).cloned()).collect::<Option<Vec<_>>>() else {
            dangling += 1;
            continue;
        };
        if let Some(pn) = &k.parameter
            && !g.params.contains_key(pn)
        {
            g.warn(format!(
                "sketch {}: a dimension names parameter {pn}, which was not imported; its value is used",
                d.name
            ));
        }
        let mut args = ents.join(", ");
        let dimension = matches!(k.kind.as_str(), "distance" | "angle" | "radius" | "diameter");
        if let Some(v) = k.value.filter(|_| dimension) {
            let v = finite(v, "a dimension")?;
            let (unit, negate) = if k.kind == "angle" { ("deg", map.flipped) } else { ("mm", false) };
            let _ = write!(args, ", {}", g.value(v, unit, k.parameter.as_ref(), negate));
        }
        let id = next("k");
        let _ = writeln!(body, "  {id} = {}({args})", k.kind);
    }
    if unknown + dangling > 0 {
        g.warn(format!(
            "sketch {}: {unknown} constraints of unknown types and {dangling} on unknown entities left out",
            d.name
        ));
    }
    let mut text = String::new();
    for (dn, dl) in &placed.datums {
        if !g.taken.insert(dn.to_ascii_lowercase()) {
            return Err(format!("the name '{dn}' for its plane is taken"));
        }
        let _ = writeln!(text, "\n{dl}");
        g.made.push(dn.clone());
    }
    let _ = write!(text, "\nsketch {name} (plane: {}) {{\n{body}}}\n", placed.plane);
    g.text.push_str(&text);
    // A constraint of the wrong arity (or the like) must not spoil the rest: the sketch
    // is kept only if the text still parses (the caller takes it back out).
    gusset_doc::text::parse(&g.text).map_err(|e| format!("it does not read back: {}", short(&e.to_string())))?;
    g.made.push(name.clone());
    g.sketches.insert(d.name.clone(), map);
    Ok(())
}

fn solid(g: &mut Gen, s: &Session, d: &SolidF, revolve: bool) -> Result<(), String> {
    let sm =
        g.sketches.get(&d.sketch).ok_or_else(|| format!("its sketch '{}' was not imported", short(&d.sketch)))?.clone();
    let (sk_name, flipped) = (sm.name.clone(), sm.flipped);
    let profile = match &d.profiles {
        None => sk_name.clone(),
        Some(Profiles::All(a)) if a == "all" => sk_name.clone(),
        Some(Profiles::All(_)) => return Err("profiles is not \"all\" or a list".into()),
        Some(Profiles::Picks(picks)) if picks.len() > MAX_PROFILE_PICKS => {
            return Err("too many profile picks".into());
        }
        Some(Profiles::Picks(picks)) => {
            let ev = g.evaluate(s).map_err(|e| e.message)?;
            let sr = ev.sketches.get(&sk_name).ok_or("its sketch did not solve")?;
            let infos = sr.regions.clone().map_err(|e| format!("its sketch has no regions: {e}"))?;
            let doc = gusset_doc::text::parse(&g.text).map_err(|e| e.to_string())?;
            let Some(gusset_doc::FeatureKind::Sketch(def)) = doc.feature(&sk_name).map(|f| &f.kind) else {
                return Err("its sketch is missing".into());
            };
            // Every region's outline from one solve (not one solve per pick and region).
            let all: Vec<String> = infos.iter().map(|ri| ri.id.clone()).collect();
            let outlines = gusset_eval::sketch_regions_each(&sk_name, def, &ev.params, doc.units, &all)
                .map_err(|e| format!("its sketch does not solve: {}", e.message))?;
            let polys: Vec<Vec<RegionPolys>> =
                outlines.iter().map(|rs| rs.iter().map(RegionPolys::new).collect()).collect();
            let mut ids = Vec::new();
            for pk in picks {
                let p = sm.map(pk.point[0], pk.point[1]);
                let hit = all.iter().zip(&polys).find(|(_, rs)| rs.iter().any(|r| r.holds(p)));
                match hit {
                    Some((id, _)) => ids.push(id.clone()),
                    None => return Err(format!("no region of its sketch holds the profile point {:?}", pk.point)),
                }
            }
            ids.sort();
            ids.dedup();
            format!("{sk_name}.regions({})", ids.join(", "))
        }
    };
    let op = match d.operation.as_deref().unwrap_or("new") {
        "new" => "new",
        "join" => "add",
        "cut" => "cut",
        "intersect" => "intersect",
        other => return Err(format!("operation '{}' is not new, join, cut or intersect", short(other))),
    };
    let name = g.fresh(&d.name, if revolve { "Revolve" } else { "Extrude" });
    let mut args = vec![format!("profile: {profile}")];
    if revolve {
        let axis = d.axis.as_ref().ok_or("a revolve without an axis")?;
        let angle = finite(d.angle.unwrap_or(360.0), "the angle")?;
        let a = match (&axis.line, axis.origin, axis.direction) {
            (Some(l), _, _) => {
                let line = sm.ids.get(l).filter(|id| id.starts_with('l'));
                format!("{sk_name}.{}", line.ok_or_else(|| format!("unknown axis line '{}'", short(l)))?)
            }
            (None, Some(o), Some(dir))
                if norm(dir).is_finite() && norm(dir) > 1e-9 && o.iter().all(|c| c.is_finite()) =>
            {
                // Gusset's X and Y are the sketch's own axes through its origin (dogfooding
                // F8-2): the world axis must be one of them; pointing the other way turns
                // the other way.
                let d = dir.map(|c| c / norm(dir));
                if norm(cross(sub(o, sm.origin), d)) > 1e-6 {
                    return Err(
                        "a revolve axis off the sketch's origin and not a sketch line is not imported yet".into()
                    );
                }
                let (name, sign) = if (dot(d, sm.x).abs() - 1.0).abs() < 1e-9 {
                    ("X", dot(d, sm.x))
                } else if (dot(d, sm.y).abs() - 1.0).abs() < 1e-9 {
                    ("Y", dot(d, sm.y))
                } else {
                    return Err("a revolve axis not along the sketch's x or y axis is not imported yet".into());
                };
                if sign < 0.0 && angle.abs() < 360.0 - 1e-9 {
                    // Gusset turns one way only: a part turn the other way has no form yet.
                    return Err("a part revolve about the reversed sketch axis is not imported yet".into());
                }
                name.to_string()
            }
            _ => return Err("a revolve needs an axis line or an origin and direction".into()),
        };
        args.push(format!("axis: {a}"));
        args.push(format!("angle: {} deg", num(angle)));
    } else {
        match d.end_condition.as_deref().unwrap_or("blind") {
            "blind" => {
                let dist = d.distance.filter(|v| v.abs() > 1e-9).ok_or("an extrude without a distance")?;
                let dist = finite(dist, "the distance")?;
                args.push(format!("distance: {}", g.value(dist, "mm", d.distance_parameter.as_ref(), flipped)));
                if d.symmetric {
                    args.push("symmetric: true".into());
                }
            }
            "to_face" => {
                if d.symmetric {
                    return Err("an extrude up to a face cannot be symmetric".into());
                }
                if d.distance.is_some() {
                    g.warn(format!("{}: the distance of an extrude up to a face is ignored", d.name));
                }
                let pick = d.face.as_ref().ok_or("an extrude up to a face without a face")?;
                let ev = g.evaluate(s).map_err(|e| e.message)?;
                let (_, face) = v2::pick_face_name(g, s, &ev, pick)?;
                args.push(format!("to: {face}"));
                if let Some(o) = d.offset.filter(|o| o.abs() > 1e-9) {
                    let o = finite(o, "the offset")?;
                    // Along the sweep, so it does not depend on the sketch's orientation.
                    args.push(format!("offset: {}", g.value(o, "mm", d.offset_parameter.as_ref(), false)));
                }
                if d.reverse != flipped {
                    args.push("reverse: true".into());
                }
            }
            other => return Err(format!("end condition '{}' is not blind or to_face", short(other))),
        }
    }
    let taper = if revolve { None } else { d.taper_deg.filter(|t| *t != 0.0) };
    if revolve && d.distance2.is_some() {
        return Err("a revolve has no second distance".into());
    }
    if taper.is_some() || d.distance2.is_some() {
        return tapered(g, s, d, &name, args, op, &sm, taper.unwrap_or(0.0));
    }
    args.push(format!("op: {op}"));
    let unrefined = if !d.refine && op != "new" { ", refine: false" } else { "" };
    // A cut through several bodies (858's Extrude6 cuts the case and the shaft) is one cut per
    // body, each with the same profile and extent.
    let targets: Vec<String> = match &d.target {
        Some(Target::Many(v)) if op == "cut" && v.len() > 1 => v.clone(),
        Some(t) if op != "new" => vec![t.one()?.to_string()],
        _ => Vec::new(),
    };
    let kw = if revolve { "revolve" } else { "extrude" };
    let mut made = Vec::new();
    if targets.is_empty() {
        let _ = writeln!(g.text, "\n{kw} {name} ({}{unrefined})", args.join(", "));
        made.push(name.clone());
    }
    for (k, t) in targets.iter().enumerate() {
        let tn = g.body_named(t, "target")?;
        let this = if k == 0 { name.clone() } else { g.fresh(&d.name, "Extrude") };
        let _ = writeln!(g.text, "\n{kw} {this} ({}, body: {tn}{unrefined})", args.join(", "));
        made.push(this);
    }
    // The new feature must build: a failure is reported and the line taken back out.
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| made.contains(&e.feature)) {
        return Err(format!("it does not build here: {}", e.message));
    }
    // The tree's name stands for the first; the others are only made.
    g.made.extend(made);
    g.features.insert(d.name.clone(), name);
    Ok(())
}

/// A tapered extrude: the straight extrude as a body of its own, its side faces drafted about
/// the sketch plane, then combined with the body it changes. Gusset's extrude has no taper; a
/// draft of the side walls with the sketch plane as neutral plane gives the same solid (the
/// profile keeps its place on the sketch plane and every wall leans by the angle). An extrude to
/// both sides (`distance2`, the second side's reach, on the other side of the sketch plane) has
/// a taper per side (`taper2_deg` for the second): each side is such a body, and the two go into
/// the one combine, so the feature's tool is the whole of both.
#[allow(clippy::too_many_arguments)]
fn tapered(
    g: &mut Gen,
    s: &Session,
    d: &SolidF,
    name: &str,
    args: Vec<String>,
    op: &str,
    sm: &SketchMap,
    taper: f64,
) -> Result<(), String> {
    if d.symmetric || d.end_condition.as_deref().is_some_and(|e| e != "blind") {
        return Err("a tapered extrude that is symmetric or ends on a face is not imported yet".into());
    }
    let dist = d.distance.ok_or("an extrude without a distance")?;
    tapered_side(g, s, name, args.clone(), dist, taper, sm)?;
    let mut tools = vec![name.to_string()];
    if let Some(d2) = d.distance2 {
        let d2 = finite(d2, "the second distance")?;
        if d2 * dist >= 0.0 || d2.abs() > 1e7 {
            return Err("the second side does not lie on the other side of the sketch plane".into());
        }
        let name2 = g.fresh(&format!("{name} side 2"), "Extrude");
        let mut args2: Vec<String> = args.into_iter().filter(|a| !a.starts_with("distance:")).collect();
        args2.push(format!("distance: {}", g.value(d2, "mm", None, sm.flipped)));
        tapered_side(g, s, &name2, args2, d2, d.taper2_deg.unwrap_or(0.0), sm)?;
        tools.push(name2);
    }
    let unrefined = if d.refine { "" } else { ", refine: false" };
    if op == "new" {
        if let Some(rest) = tools.get(1..).filter(|r| !r.is_empty()) {
            let comb = g.fresh(&format!("{name} combine"), "Combine");
            let _ =
                writeln!(g.text, "\ncombine {comb} (body: {name}, tools: [{}], op: add{unrefined})", rest.join(", "));
            let ev = g.evaluate(s).map_err(|e| e.message)?;
            if let Some(e) = ev.errors().iter().find(|e| e.feature == comb) {
                return Err(format!("its second side does not build here: {}", e.message));
            }
            g.made.push(comb);
        }
        g.features.insert(d.name.clone(), name.to_string());
        return Ok(());
    }
    let t = d.target.as_ref().ok_or("the body it changes is not named")?.one()?;
    let target = g.body_named(t, "target")?;
    let comb = g.fresh(&format!("{name} combine"), "Combine");
    let _ = writeln!(g.text, "\ncombine {comb} (body: {target}, tools: [{}], op: {op}{unrefined})", tools.join(", "));
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| e.feature == comb) {
        return Err(format!("it does not build here: {}", e.message));
    }
    // The feature is the combine, as an untapered cut or join is its own extrude: a feature
    // pattern then repeats what it removed or added (5905's PinPattern2 of a tapered cut).
    g.made.push(comb.clone());
    g.features.insert(d.name.clone(), comb);
    Ok(())
}

/// One side of a tapered extrude: the straight extrude `name` (`args` hold its profile and
/// distance), a new body, its side faces drafted by `taper` about the sketch plane (none for 0).
fn tapered_side(
    g: &mut Gen,
    s: &Session,
    name: &str,
    mut args: Vec<String>,
    dist: f64,
    taper: f64,
    sm: &SketchMap,
) -> Result<(), String> {
    let taper = finite(taper, "the taper angle")?;
    if taper.abs() >= 89.0 {
        return Err("a taper of 89 degrees or more is not imported".into());
    }
    args.push("op: new".into());
    let _ = writeln!(
        g.text,
        "
extrude {name} ({})",
        args.join(", ")
    );
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| e.feature == name) {
        return Err(format!("it does not build here: {}", e.message));
    }
    g.made.push(name.to_string());
    if taper == 0.0 {
        return Ok(());
    }
    let prefix = format!("{name}/side(");
    let mut sides: Vec<String> = Vec::new();
    for b in ev.bodies.iter().filter(|b| b.feature == name) {
        for f in &b.names.faces {
            let f = f.to_string();
            if f.starts_with(&prefix) && !sides.contains(&f) {
                sides.push(f);
            }
        }
    }
    if sides.is_empty() || sides.len() > MAX_PICKS {
        return Err("the tapered extrude has no side faces to lean".into());
    }
    // The sweep runs along the Gusset sketch's normal when the distance written (negated for a
    // flipped plane) is positive; the draft pulls along the sweep, and a positive draft angle
    // narrows the solid towards the pull side.
    let ng = cross(sm.x, sm.y);
    let pull = if (dist > 0.0) != sm.flipped { ng } else { ng.map(|c| -c) };
    let draft = g.fresh(&format!("{name} taper"), "Draft");
    let (plane, against) = v2::datum_plane_facing(g, &draft, sm.origin, pull)?;
    let flip = if against { ", flip: true" } else { "" };
    let _ = writeln!(
        g.text,
        "\ndraft {draft} (body: {name}, faces: [{}], plane: {}, angle: {} deg{flip})",
        sides.join(", "),
        plane,
        num(-taper)
    );
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| e.feature == draft) {
        return Err(format!("its taper does not build here: {}", e.message));
    }
    g.made.push(draft);
    Ok(())
}

/// The model's edges whose midpoint and ends all lie within 0.01 mm of a source edge's
/// points (at most 256 points are looked at); their bodies and names.
fn on_path_edges(ev: &gusset_eval::Evaluation, path: &[[f64; 3]]) -> Vec<(String, String)> {
    let path = path.get(..path.len().min(MAX_PATH_POINTS)).unwrap_or_default();
    if path.len() < 2 {
        return Vec::new();
    }
    let near = |q: V3| {
        path.windows(2).any(|w| {
            let [a, b] = [w.first().copied().unwrap_or_default(), w.last().copied().unwrap_or_default()];
            let ab = sub(b, a);
            let l2 = dot(ab, ab);
            let u = if l2 > 1e-18 { (dot(sub(q, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
            norm(sub(q, [a[0] + ab[0] * u, a[1] + ab[1] * u, a[2] + ab[2] * u])) <= 0.01
        })
    };
    let mut out = Vec::new();
    for b in &ev.bodies {
        let t = &b.topology;
        for (i, e) in t.edges.iter().enumerate() {
            let ends_on = e.vertices.iter().all(|v| t.vertices.get(*v as usize).is_some_and(|v| near(v.point)));
            if near(e.midpoint)
                && ends_on
                && let Some(n) = b.names.edges.get(i)
            {
                out.push((b.feature.clone(), n.to_string()));
            }
        }
    }
    out
}

/// The model's edges that hold a source edge: its midpoint and both ends lie on one edge of
/// the model (a line, or a circle's arc). FreeCAD splits an edge where the faces of a joined
/// pattern meet; Gusset keeps the whole edge, and a fillet takes all of it.
fn containing_edges(ev: &gusset_eval::Evaluation, pick: &EdgePick) -> Vec<(String, String)> {
    let (Some(first), Some(last)) = (pick.path.first().copied(), pick.path.last().copied()) else {
        return Vec::new();
    };
    let pts = [pick.midpoint, first, last];
    let mut out = Vec::new();
    for b in &ev.bodies {
        let t = &b.topology;
        for (i, e) in t.edges.iter().enumerate() {
            let ends: Vec<V3> =
                e.vertices.iter().filter_map(|v| t.vertices.get(*v as usize)).map(|v| v.point).collect();
            if edge_holds(e.kind, &ends, e.midpoint, e.circle, &pts)
                && let Some(n) = b.names.edges.get(i)
            {
                out.push((b.feature.clone(), n.to_string()));
            }
        }
    }
    out
}

/// The free-form (B-spline and other) edges of the model that share an end with a source edge.
/// FreeCAD splits a closed spline edge where a cut's vertex touches it, at points Gusset's
/// boolean does not make, so the pieces of a pick do not match Gusset's pieces; the last resort
/// is the model's spline edges that have an end where the pick starts or stops (the fillet
/// check on the rebuilt volume rejects a wrong guess).
fn spline_edges_at_ends(ev: &gusset_eval::Evaluation, pick: &EdgePick) -> Vec<(String, String)> {
    let (Some(first), Some(last)) = (pick.path.first().copied(), pick.path.last().copied()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for b in &ev.bodies {
        let t = &b.topology;
        for (i, e) in t.edges.iter().enumerate() {
            if !matches!(e.kind, gusset_kernel::CurveKind::BSpline | gusset_kernel::CurveKind::Other) {
                continue;
            }
            let at_end = e
                .vertices
                .iter()
                .filter_map(|v| t.vertices.get(*v as usize))
                .any(|v| norm(sub(v.point, first)) <= 0.01 || norm(sub(v.point, last)) <= 0.01);
            if at_end && let Some(n) = b.names.edges.get(i) {
                out.push((b.feature.clone(), n.to_string()));
            }
        }
    }
    out
}

/// Whether every point lies (within 0.01 mm) on a line or circular arc given by its ends and
/// midpoint.
fn edge_holds(
    kind: gusset_kernel::CurveKind,
    ends: &[V3],
    mid: V3,
    circle: Option<gusset_kernel::Circle3>,
    pts: &[V3],
) -> bool {
    let tol = 0.01;
    match (kind, ends, circle) {
        (gusset_kernel::CurveKind::Line, [a, b], _) => pts.iter().all(|p| {
            let ab = sub(*b, *a);
            let l2 = dot(ab, ab);
            let u = if l2 > 1e-18 { (dot(sub(*p, *a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let q = [a[0] + ab[0] * u, a[1] + ab[1] * u, a[2] + ab[2] * u];
            norm(sub(*p, q)) <= tol
        }),
        // A whole circle (one vertex, or both ends together): FreeCAD splits it into arcs.
        (gusset_kernel::CurveKind::Circle, [_] | [_, _], Some(c))
            if ends.len() == 1 || ends.first().zip(ends.get(1)).is_some_and(|(a, b)| norm(sub(*a, *b)) <= 1e-9) =>
        {
            pts.iter().all(|p| {
                let w = sub(*p, c.center);
                (norm(w) - c.radius).abs() <= tol && dot(w, c.normal).abs() <= tol
            })
        }
        (gusset_kernel::CurveKind::Circle, [a, b], Some(c)) => {
            let u0 = sub(*a, c.center);
            let v0 = cross(c.normal, u0);
            let ang = |p: V3| {
                let w = sub(p, c.center);
                libm::atan2(dot(w, v0), dot(w, u0))
            };
            let two_pi = 2.0 * core::f64::consts::PI;
            // The arc runs from its first end through its midpoint to the second.
            let dir = if ang(mid) >= 0.0 { 1.0 } else { -1.0 };
            let sweep = (dir * ang(*b)).rem_euclid(two_pi);
            let slack = tol / c.radius.max(1e-9);
            pts.iter().all(|p| {
                let w = sub(*p, c.center);
                let a = (dir * ang(*p)).rem_euclid(two_pi);
                (norm(w) - c.radius).abs() <= tol
                    && dot(w, c.normal).abs() <= tol
                    && (a <= sweep + slack || a >= two_pi - slack)
            })
        }
        _ => false,
    }
}

/// The `, distance2: ...` / `, angle: ...` and `, faces: [...]` arguments of an unequal
/// chamfer, or nothing for an equal one. The reference face of each edge is the picked
/// face when the edge borders it, else the default (see the chamfer ADR amendment).
fn chamfer_second(g: &mut Gen, s: &Session, d: &EdgeF, body: &str, names: &[String]) -> Result<String, String> {
    let (second, face) = match (d.distance2, d.angle) {
        (None, None) => {
            if d.face.is_some() {
                g.warn(format!("{}: a reference face needs distance2 or angle: ignored", d.name));
            }
            return Ok(String::new());
        }
        (Some(_), Some(_)) => return Err("a chamfer takes distance2 or angle, not both".into()),
        (Some(v), None) => {
            let v = Some(v).filter(|v| v.is_finite() && *v > 0.0 && *v < 1e12).ok_or("distance2 is not positive")?;
            (format!(", distance2: {} mm", num(v)), d.face.as_ref())
        }
        (None, Some(a)) => {
            let a = Some(a)
                .filter(|a| a.is_finite() && *a > 0.0 && *a < 90.0)
                .ok_or("angle is not between 0 and 90 degrees")?;
            (format!(", angle: {} deg", num(a)), d.face.as_ref())
        }
    };
    let Some(pick) = face else { return Ok(second) };
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    let (face_body, face_name) = v2::pick_face_name(g, s, &ev, pick)?;
    if face_body != body {
        return Err("the reference face is on another body than the edges".into());
    }
    let b = ev.bodies.iter().find(|b| b.feature == body).ok_or("the edges' body is gone")?;
    let mut refs = Vec::new();
    for n in names {
        // Faces next to the edge, by name; the picked one if among them, else the smallest.
        let adjacent: Vec<gusset_doc::FaceRef> = b
            .names
            .edges
            .iter()
            .position(|e| e.to_string() == *n)
            .and_then(|i| b.topology.edges.get(i))
            .map(|e| e.faces.iter().filter_map(|f| b.names.faces.get(*f as usize)).cloned().collect())
            .unwrap_or_default();
        if adjacent.iter().any(|f| f.to_string() == face_name) {
            refs.push(face_name.clone());
        } else {
            g.warn(format!("{}: the reference face {face_name} does not border {n}: the default face is used", d.name));
            refs.push(adjacent.into_iter().min().map_or_else(|| face_name.clone(), |f| f.to_string()));
        }
    }
    Ok(format!("{second}, faces: [{}]", refs.join(", ")))
}

/// Most points of a source edge's path looked at.
const MAX_PATH_POINTS: usize = 512;

/// A full-round fillet of the face the pick names (ADR 0042 amendment).
fn full_round(g: &mut Gen, s: &Session, d: &EdgeF, pick: &v2::FacePick, fillet: bool) -> Result<(), String> {
    if !fillet {
        return Err("only a fillet can be a full round".into());
    }
    if !d.edges.is_empty() || d.radius.is_some() || d.distance.is_some() {
        return Err("a full round takes a face, not edges and a radius".into());
    }
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    let (_, face) = v2::pick_face_name(g, s, &ev, pick)?;
    let name = g.fresh(&d.name, "Fillet");
    g.text.push_str(&format!("\nfillet {name} (full_round: [{face}])\n"));
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| e.feature == name) {
        return Err(format!("it does not build here: {}", e.message));
    }
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

fn edges(g: &mut Gen, s: &Session, d: &EdgeF, fillet: bool) -> Result<(), String> {
    if let Some(pick) = &d.full_round {
        return full_round(g, s, d, pick, fillet);
    }
    let size = if fillet { d.radius } else { d.distance };
    let size = size.filter(|v| v.is_finite() && *v > 0.0 && *v < 1e12).ok_or("no positive size")?;
    if d.edges.len() > MAX_PICKS {
        return Err("too many edges".into());
    }
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    // Each edge: the nearest one of the model as built so far, within 0.01 mm; of the bodies
    // `only` names alone, if given.
    let collect = |only: Option<&str>| -> Result<BTreeMap<String, Vec<String>>, String> {
        let mut by_body: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mine = |body: &str| only.is_none_or(|o| o == body);
        for pick in &d.edges {
            let mut best: Option<(f64, String, String)> = None;
            for b in ev.bodies.iter().filter(|b| mine(&b.feature)) {
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
                _ => {
                    let mut on_path = on_path_edges(&ev, &pick.path);
                    if on_path.is_empty() {
                        on_path = containing_edges(&ev, pick);
                    }
                    if on_path.is_empty() {
                        on_path = spline_edges_at_ends(&ev, pick);
                    }
                    on_path.retain(|(body, _)| mine(body));
                    if on_path.is_empty() {
                        let near =
                            best.as_ref().map_or(String::new(), |b| format!(" (the nearest is {:.3} mm away)", b.0));
                        return Err(format!("no edge of the model has its midpoint at {:?}{near}", pick.midpoint));
                    }
                    for (body, n) in on_path {
                        by_body.entry(body).or_default().push(n);
                    }
                }
            }
        }
        Ok(by_body)
    };
    let mut by_body = collect(None)?;
    // A pick whose nearest edge lies in more than one body (a magnet's rim on its pocket's): the
    // nearest-edge search keeps the first body's, whichever the source means.
    let shared = || {
        d.edges.iter().any(|pick| {
            let mut nearest: Vec<(f64, &str)> = Vec::new();
            for b in &ev.bodies {
                let dd =
                    b.topology.edges.iter().map(|e| norm(sub(e.midpoint, pick.midpoint))).fold(f64::INFINITY, f64::min);
                if dd <= 0.01 {
                    nearest.push((dd, b.feature.as_str()));
                }
            }
            let Some(min) = nearest.iter().map(|(dd, _)| *dd).reduce(f64::min) else { return false };
            let mut bodies: Vec<&str> = nearest.iter().filter(|(dd, _)| *dd <= min + 1e-7).map(|(_, b)| *b).collect();
            bodies.dedup();
            bodies.len() > 1
        })
    };
    // Edges of several bodies that lie on one another (a magnet in its pocket): the body the
    // source names decides.
    if (by_body.len() > 1 || shared())
        && let Some(t) = &d.on_body
    {
        let tb = g.body_named(t, "body")?;
        by_body = collect(Some(&tb))?;
    }
    if by_body.is_empty() {
        return Err("no edges".into());
    }
    // Edges on several bodies (Fusion blends them in one feature): one blend per body, each
    // with the same size.
    let (kw, key) = if fillet { ("fillet", "radius") } else { ("chamfer", "distance") };
    let mut made = Vec::new();
    for (body, mut names) in by_body {
        // Several source pieces of one edge of the model name it once.
        let mut seen = HashSet::new();
        names.retain(|n| seen.insert(n.clone()));
        let name = g.fresh(&d.name, if fillet { "Fillet" } else { "Chamfer" });
        let second = if fillet { String::new() } else { chamfer_second(g, s, d, &body, &names)? };
        let line = format!("\n{kw} {name} (edges: [{}], {key}: {} mm{second})\n", names.join(", "), num(size));
        g.text.push_str(&line);
        made.push(name);
    }
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    if let Some(e) = ev.errors().iter().find(|e| made.contains(&e.feature)) {
        return Err(format!("it does not build here: {}", e.message));
    }
    // The tree's name stands for the first; the others are only made.
    if let Some(first) = made.first() {
        g.features.insert(d.name.clone(), first.clone());
    }
    g.made.extend(made);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::edge_holds;
    use gusset_kernel::{Circle3, CurveKind};

    #[test]
    fn a_source_piece_of_an_arc_lies_on_the_whole_arc() {
        // A quarter circle of radius 10 about the origin, from (10, 0) to (0, 10).
        let c = Circle3 { center: [0.0; 3], normal: [0.0, 0.0, 1.0], radius: 10.0 };
        let r = core::f64::consts::FRAC_1_SQRT_2 * 10.0;
        let ends = [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0]];
        let mid = [r, r, 0.0];
        let at = |deg: f64| {
            let a = deg.to_radians();
            [10.0 * libm::cos(a), 10.0 * libm::sin(a), 0.0]
        };
        // A piece in the middle, and one starting exactly at the first end.
        assert!(edge_holds(CurveKind::Circle, &ends, mid, Some(c), &[at(50.0), at(30.0), at(60.0)]));
        assert!(edge_holds(CurveKind::Circle, &ends, mid, Some(c), &[at(10.0), at(0.0), at(20.0)]));
        // Past the end, off the circle and off its plane do not.
        assert!(!edge_holds(CurveKind::Circle, &ends, mid, Some(c), &[at(100.0), at(80.0), at(95.0)]));
        assert!(!edge_holds(CurveKind::Circle, &ends, mid, Some(c), &[[5.0, 0.0, 0.0]; 3]));
        assert!(!edge_holds(CurveKind::Circle, &ends, mid, Some(c), &[[r, r, 1.0]; 3]));
        // A line holds a piece of itself.
        let line = [[0.0; 3], [10.0, 0.0, 0.0]];
        assert!(edge_holds(
            CurveKind::Line,
            &line,
            [5.0, 0.0, 0.0],
            None,
            &[[4.0, 0.0, 0.0], [3.0, 0.0, 0.0], [6.0, 0.0, 0.0]]
        ));
        assert!(!edge_holds(CurveKind::Line, &line, [5.0, 0.0, 0.0], None, &[[4.0, 1.0, 0.0]; 3]));
        assert!(!edge_holds(CurveKind::Line, &line, [5.0, 0.0, 0.0], None, &[[11.0, 0.0, 0.0]; 3]));
    }
}
