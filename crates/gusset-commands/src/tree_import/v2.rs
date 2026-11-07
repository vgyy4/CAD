//! Version 2 of the feature-tree format (ADR 0042 amendment): holes, linear and circular
//! patterns, mirrors and shells. Faces are found by a point on them, like edges by midpoint.

use super::{
    Axis, BASES, EdgeF, Gen, MAX_PICKS, SketchF, SolidF, V3, cross, dot, edges, finite, norm, num, short, sketch,
    solid, sub,
};
use crate::Session;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// A face picked by a point on it (world mm) and optionally its outward normal.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct FacePick {
    point: [f64; 3],
    #[serde(default)]
    normal: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct HoleF {
    name: String,
    face: FacePick,
    positions: Vec<[f64; 3]>,
    diameter: f64,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    through: bool,
    #[serde(default)]
    depth: Option<f64>,
    #[serde(default)]
    cbore_diameter: Option<f64>,
    #[serde(default)]
    cbore_depth: Option<f64>,
    #[serde(default)]
    csink_diameter: Option<f64>,
    #[serde(default)]
    csink_angle: Option<f64>,
    #[serde(default)]
    point_angle: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct PatternF {
    name: String,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    direction: Option<[f64; 3]>,
    #[serde(default)]
    spacing: Option<f64>,
    #[serde(default)]
    axis: Option<Axis>,
    #[serde(default)]
    angle: Option<f64>,
    count: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct MirrorPlane {
    origin: [f64; 3],
    normal: [f64; 3],
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct MirrorF {
    name: String,
    plane: MirrorPlane,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    merge: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ShellF {
    name: String,
    faces: Vec<FacePick>,
    thickness: f64,
    #[serde(default)]
    side: Option<String>,
}

fn unit(a: V3) -> Option<V3> {
    let l = norm(a);
    (l.is_finite() && l > 1e-9).then(|| a.map(|c| c / l))
}

fn finite3(v: V3, what: &str) -> Result<V3, String> {
    for c in v {
        finite(c, what)?;
    }
    Ok(v)
}

/// A face found on the model so far.
struct FaceHit {
    /// The feature that made the body.
    body: String,
    name: String,
    planar: bool,
    normal: V3,
    centroid: V3,
}

/// The face whose surface holds `point` within 0.01 mm (and, when given, whose outward normal
/// is within about 2.5 degrees of `normal`). Planes and cylinders are told apart this way; of
/// several faces on one surface the one with the nearest centre wins.
fn find_face(ev: &gusset_eval::Evaluation, point: V3, normal: Option<V3>) -> Result<FaceHit, String> {
    use gusset_kernel::SurfaceKind;
    let want = match normal {
        Some(n) => Some(unit(finite3(n, "a face normal")?).ok_or("a face normal has no direction")?),
        None => None,
    };
    let mut best: Option<(f64, FaceHit)> = None;
    for b in &ev.bodies {
        // A cylinder's radius from its circular edges, in one pass over the edges.
        let mut radius_of: HashMap<u32, f64> = HashMap::new();
        for e in &b.topology.edges {
            if let Some(c) = e.circle {
                for f in &e.faces {
                    radius_of.entry(*f).or_insert(c.radius);
                }
            }
        }
        for (i, f) in b.topology.faces.iter().enumerate() {
            let Some(name) = b.names.faces.get(i) else { continue };
            let fnorm = unit(f.normal);
            let hit = match f.kind {
                SurfaceKind::Plane => fnorm.is_some_and(|n| {
                    dot(sub(point, f.centroid), n).abs() <= 0.01 && want.is_none_or(|w| dot(n, w) > 0.999)
                }),
                SurfaceKind::Cylinder => f.axis.is_some_and(|a| {
                    let radius = u32::try_from(i).ok().and_then(|i| radius_of.get(&i).copied());
                    radius
                        .zip(unit(a.dir))
                        .is_some_and(|(r, d)| (norm(cross(sub(point, a.origin), d)) - r).abs() <= 0.01)
                }),
                _ => false,
            };
            let score = norm(sub(point, f.centroid));
            if hit && best.as_ref().is_none_or(|x| score < x.0) {
                best = Some((
                    score,
                    FaceHit {
                        body: b.feature.clone(),
                        name: name.to_string(),
                        planar: f.kind == SurfaceKind::Plane,
                        normal: fnorm.unwrap_or([0.0, 0.0, 1.0]),
                        centroid: f.centroid,
                    },
                ));
            }
        }
    }
    best.map(|x| x.1).ok_or_else(|| format!("no planar or cylindrical face of the model holds the point {point:?}"))
}

/// The feature must build here, else the error is reported (the caller takes the text out).
fn builds(g: &mut Gen, s: &Session, name: &str) -> Result<(), String> {
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    match ev.errors().iter().find(|e| e.feature == name) {
        Some(e) => Err(format!("it does not build here: {}", e.message)),
        None => Ok(()),
    }
}

/// Longest length a v2 feature may give (mm): OCCT is not made for larger models.
const MAX_LENGTH: f64 = 1e7;

/// A unit vector's component with 12 decimals (6 would move far pattern copies: review).
fn fine(v: f64) -> String {
    let s = format!("{:.12}", if v.abs() < 5e-13 { 0.0 } else { v });
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

fn positive(v: Option<f64>, what: &str) -> Result<f64, String> {
    v.filter(|v| v.is_finite() && *v > 0.0 && *v < MAX_LENGTH).ok_or_else(|| format!("{what} is not a positive number"))
}

pub(super) fn hole(g: &mut Gen, s: &Session, d: &HoleF) -> Result<(), String> {
    if d.positions.is_empty() || d.positions.len() > MAX_PICKS {
        return Err("a hole needs between 1 and 256 positions".into());
    }
    let diameter = positive(Some(d.diameter), "the diameter")?;
    let variant = d.variant.as_deref().unwrap_or("simple");
    let mut args = Vec::new();
    match variant {
        "simple" => args.push("type: simple".to_string()),
        "counterbore" => {
            let (cd, cdep) = (positive(d.cbore_diameter, "cbore_diameter")?, positive(d.cbore_depth, "cbore_depth")?);
            if cd <= diameter {
                return Err("the counterbore is not wider than the hole".into());
            }
            args.push("type: counterbore".to_string());
            args.push(format!("cbore_diameter: {} mm", num(cd)));
            args.push(format!("cbore_depth: {} mm", num(cdep)));
        }
        "countersink" => {
            let cd = positive(d.csink_diameter, "csink_diameter")?;
            let angle = finite(d.csink_angle.unwrap_or(90.0), "csink_angle")?;
            if cd <= diameter || angle <= 0.0 || angle >= 180.0 {
                return Err("the countersink is not wider than the hole, or its angle is not in 0..180".into());
            }
            args.push("type: countersink".to_string());
            args.push(format!("csink_diameter: {} mm", num(cd)));
            args.push(format!("csink_angle: {} deg", num(angle)));
        }
        other => return Err(format!("hole variant '{}' is not imported", short(other))),
    }
    args.push(format!("diameter: {} mm", num(diameter)));
    if d.through {
        args.push("through: true".into());
    } else {
        args.push(format!("depth: {} mm", num(positive(d.depth, "the depth")?)));
    }
    // A through hole has a flat end: the drill point only applies to a blind one.
    if let Some(pa) = d.point_angle.filter(|_| !d.through) {
        let pa = finite(pa, "point_angle")?;
        if pa <= 0.0 || pa > 180.0 {
            return Err("point_angle is not in 0..180".into());
        }
        args.push(format!("point_angle: {} deg", num(pa)));
    }
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    let face = find_face(&ev, finite3(d.face.point, "the face point")?, d.face.normal)?;
    if !face.planar {
        return Err("a hole goes into a planar face".into());
    }
    // Gusset's frame on a planar face (the evaluator's `face_frame`): the origin is the
    // world origin projected onto the plane, x the world X projected (world Y when X is
    // nearly the normal), y = normal x x.
    let n = face.normal;
    let project = |v: V3| sub(v, n.map(|c| c * dot(v, n)));
    let fx = unit(project([1.0, 0.0, 0.0]))
        .filter(|x| dot(*x, [1.0, 0.0, 0.0]).abs() > 1e-3)
        .or_else(|| unit(project([0.0, 1.0, 0.0])))
        .ok_or("the face has no usable frame")?;
    let fy = cross(n, fx);
    let origin = n.map(|c| c * dot(face.centroid, n));
    let mut sites = Vec::new();
    for (i, p) in d.positions.iter().enumerate() {
        let p = finite3(*p, "a hole position")?;
        if dot(sub(p, origin), n).abs() > 0.01 {
            return Err(format!("hole position {p:?} is not on the face"));
        }
        let r = sub(p, origin);
        sites.push(format!("e{}: [{} mm, {} mm]", i + 1, num(dot(r, fx)), num(dot(r, fy))));
    }
    let name = g.fresh(&d.name, "Hole");
    let _ = writeln!(g.text, "\nhole {name} (face: {}, at: [{}], {})", face.name, sites.join(", "), args.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

pub(super) fn pattern(g: &mut Gen, s: &Session, d: &PatternF, circular: bool) -> Result<(), String> {
    if !(2..=500).contains(&d.count) {
        return Err("a pattern count is from 2 to 500".into());
    }
    if d.features.len() > MAX_PICKS {
        return Err("too many features".into());
    }
    let mut args = Vec::new();
    if circular {
        let axis = d.axis.as_ref().ok_or("a circular pattern needs an axis")?;
        let dir = axis.direction.ok_or("a circular pattern axis needs a direction")?;
        let dir = unit(finite3(dir, "the axis")?).ok_or("the axis has no direction")?;
        let o = finite3(axis.origin.unwrap_or([0.0; 3]), "the axis")?;
        let angle = finite(d.angle.unwrap_or(360.0), "the angle")?;
        if angle.abs() < 1e-9 {
            return Err("the pattern angle is zero".into());
        }
        args.push(format!("axis: [{}, {}, {}]", fine(dir[0]), fine(dir[1]), fine(dir[2])));
        args.push(format!("through: [{} mm, {} mm, {} mm]", num(o[0]), num(o[1]), num(o[2])));
        args.push(format!("count: {}", d.count));
        args.push(format!("angle: {} deg", num(angle)));
    } else {
        let dir = d.direction.ok_or("a linear pattern needs a direction")?;
        let dir = unit(finite3(dir, "the direction")?).ok_or("the direction has no length")?;
        let spacing = finite(d.spacing.ok_or("a linear pattern needs a spacing")?, "the spacing")?;
        if spacing.abs() < 1e-9 {
            return Err("the pattern spacing is zero".into());
        }
        args.push(format!("direction: [{}, {}, {}]", fine(dir[0]), fine(dir[1]), fine(dir[2])));
        if (spacing * f64::from(d.count)).abs() > MAX_LENGTH {
            return Err("the pattern reaches too far".into());
        }
        args.push(format!("count: {}", d.count));
        args.push(format!("spacing: {} mm", num(spacing)));
    }
    if d.features.is_empty() {
        // The whole body.
        if let Some(t) = &d.target {
            let tn = g.features.get(t).ok_or_else(|| format!("its target '{}' was not imported", short(t)))?;
            args.push(format!("body: {tn}"));
        }
    } else {
        let mut names = Vec::new();
        for f in &d.features {
            names
                .push(g.features.get(f).ok_or_else(|| format!("its feature '{}' was not imported", short(f)))?.clone());
        }
        args.push(format!("features: [{}]", names.join(", ")));
    }
    let name = g.fresh(&d.name, "Pattern");
    let kw = if circular { "pattern_circular" } else { "pattern_linear" };
    let _ = writeln!(g.text, "\n{kw} {name} ({})", args.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

pub(super) fn shell(g: &mut Gen, s: &Session, d: &ShellF) -> Result<(), String> {
    if d.faces.is_empty() || d.faces.len() > MAX_PICKS {
        return Err("a shell removes between 1 and 256 faces".into());
    }
    let t = positive(Some(d.thickness), "the thickness")?;
    // Gusset: positive walls grow inward, negative outward.
    let t = match d.side.as_deref().unwrap_or("inside") {
        "inside" => t,
        "outside" => -t,
        other => return Err(format!("shell side '{}' is not inside or outside", short(other))),
    };
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    let mut body: Option<String> = None;
    let mut names = Vec::new();
    for pick in &d.faces {
        let f = find_face(&ev, finite3(pick.point, "a face point")?, pick.normal)?;
        if body.as_ref().is_some_and(|b| *b != f.body) {
            return Err("its faces are on several bodies".into());
        }
        body = Some(f.body);
        if !names.contains(&f.name) {
            names.push(f.name);
        }
    }
    let body = body.ok_or("no faces")?;
    let name = g.fresh(&d.name, "Shell");
    let _ = writeln!(g.text, "\nshell {name} (body: {body}, faces: [{}], thickness: {} mm)", names.join(", "), num(t));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

fn reflect_point(p: V3, o: V3, n: V3) -> V3 {
    let k = 2.0 * dot(sub(p, o), n);
    [p[0] - k * n[0], p[1] - k * n[1], p[2] - k * n[2]]
}

fn reflect_dir(v: V3, n: V3) -> V3 {
    let k = 2.0 * dot(v, n);
    [v[0] - k * n[0], v[1] - k * n[1], v[2] - k * n[2]]
}

fn v3_of(v: &Value) -> Option<V3> {
    match v.as_array()?.as_slice() {
        [x, y, z] => Some([x.as_f64()?, y.as_f64()?, z.as_f64()?]),
        _ => None,
    }
}

/// Rewrites the 3-vector at `v[key]` with `f`, if there is one.
fn map_v3(v: &mut Value, key: &str, f: &dyn Fn(V3) -> V3) -> Result<(), String> {
    if let Some(slot) = v.get_mut(key) {
        let p = v3_of(slot).ok_or_else(|| format!("{key} is not three numbers"))?;
        *slot = json!(f(p));
    }
    Ok(())
}

fn set(v: &mut Value, key: &str, to: Value) {
    if let Some(m) = v.as_object_mut() {
        m.insert(key.to_string(), to);
    }
}

/// A source feature's image across the plane: the same feature with its world geometry
/// reflected. A sketch keeps its coordinates on the reflected plane; an extrusion's direction
/// flips because a reflection reverses handedness (x' x y' is -reflect(x x y)).
fn reflect_feature(f: &Value, o: V3, n: V3, rename: &dyn Fn(&str) -> String) -> Result<Value, String> {
    let mut out = f.clone();
    let kind = f.get("type").and_then(Value::as_str).unwrap_or("");
    let pt = |p: V3| reflect_point(p, o, n);
    let dr = |p: V3| reflect_dir(p, n);
    if let Some(nm) = f.get("name").and_then(Value::as_str) {
        set(&mut out, "name", json!(rename(nm)));
    }
    match kind {
        "sketch" => {
            let plane = out.get_mut("plane").ok_or("a sketch without a plane")?;
            map_v3(plane, "origin", &pt)?;
            map_v3(plane, "x_axis", &dr)?;
            map_v3(plane, "y_axis", &dr)?;
        }
        "extrude" | "revolve" => {
            if let Some(sk) = f.get("sketch").and_then(Value::as_str) {
                set(&mut out, "sketch", json!(rename(sk)));
            }
            if kind == "extrude" {
                if let Some(d) = f.get("distance").and_then(Value::as_f64) {
                    set(&mut out, "distance", json!(-d));
                }
                // The parameter's sign convention does not carry over: the mirrored copy
                // uses the number.
                if let Some(m) = out.as_object_mut() {
                    m.remove("distance_parameter");
                }
            } else {
                // A reflection reverses the sense of a turn, and Gusset turns one way only.
                if f.get("angle").and_then(Value::as_f64).is_some_and(|a| a.abs() < 360.0 - 1e-9) {
                    return Err("a part revolve cannot be mirrored".into());
                }
                if let Some(ax) = out.get_mut("axis") {
                    map_v3(ax, "origin", &pt)?;
                    map_v3(ax, "direction", &dr)?;
                }
            }
        }
        "hole" => {
            if let Some(face) = out.get_mut("face") {
                map_v3(face, "point", &pt)?;
                map_v3(face, "normal", &dr)?;
            }
            if let Some(ps) = out.get_mut("positions").and_then(Value::as_array_mut) {
                for p in ps {
                    let v = v3_of(p).ok_or("a hole position is not three numbers")?;
                    *p = json!(pt(v));
                }
            }
        }
        "fillet" | "chamfer" => {
            if let Some(es) = out.get_mut("edges").and_then(Value::as_array_mut) {
                for e in es {
                    map_v3(e, "midpoint", &pt)?;
                }
            }
        }
        other => return Err(format!("a {} feature cannot be mirrored", short(other))),
    }
    Ok(out)
}

pub(super) fn mirror(g: &mut Gen, s: &Session, d: &MirrorF, feats: &[Value]) -> Result<(), String> {
    let n = unit(finite3(d.plane.normal, "the mirror plane")?).ok_or("the mirror plane has no normal")?;
    let o = finite3(d.plane.origin, "the mirror plane")?;
    if d.features.len() > MAX_PICKS {
        return Err("too many features".into());
    }
    if d.features.is_empty() {
        return mirror_body(g, s, d, o, n);
    }
    // Gusset mirrors bodies, so each listed feature is built again, reflected.
    let before: (HashSet<String>, HashSet<String>) =
        (g.sketches.keys().cloned().collect(), g.features.keys().cloned().collect());
    let r = mirror_features(g, s, d, feats, o, n);
    if r.is_err() {
        g.sketches.retain(|k, _| before.0.contains(k));
        g.features.retain(|k, _| before.1.contains(k));
    }
    r
}

/// The whole body, across a base plane or an offset datum plane.
fn mirror_body(g: &mut Gen, s: &Session, d: &MirrorF, o: V3, n: V3) -> Result<(), String> {
    let mut found = None;
    for (base, x, y) in BASES {
        let bn = cross(x, y);
        if (dot(n, bn).abs() - 1.0).abs() < 1e-9 {
            found = Some((base, dot(o, bn)));
            break;
        }
    }
    let (base, offset) =
        found.ok_or("its plane is not parallel to XY, XZ or YZ (tilted planes are not imported yet)")?;
    let name = g.fresh(&d.name, "Mirror");
    let plane = if offset.abs() < 1e-9 {
        base.to_string()
    } else {
        let pn = g.fresh(&format!("{name}_plane"), "Plane");
        let _ = writeln!(g.text, "\nplane {pn} (base: {base}, offset: {} mm)", num(offset));
        g.made.push(pn.clone());
        pn
    };
    let mut args = vec![format!("plane: {plane}")];
    if let Some(t) = &d.target {
        let tn = g.features.get(t).ok_or_else(|| format!("its target '{}' was not imported", short(t)))?;
        args.push(format!("body: {tn}"));
    }
    if d.merge.unwrap_or(true) {
        args.push("merge: true".into());
    }
    let _ = writeln!(g.text, "\nmirror {name} ({})", args.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

fn mirror_features(g: &mut Gen, s: &Session, d: &MirrorF, feats: &[Value], o: V3, n: V3) -> Result<(), String> {
    // A tag of its own per mirror (two mirrors named alike must not share copies), joined
    // with a character no source name has in practice.
    let tag = g.fresh(&d.name, "Mirror");
    let rename = |nm: &str| format!("{nm}\u{1}{tag}");
    let mut mirrored: HashSet<String> = HashSet::new();
    let mut last = None;
    for fname in &d.features {
        let idx = *g.index.get(fname).ok_or_else(|| format!("its feature '{}' was not imported", short(fname)))?;
        let f = feats.get(idx).ok_or("its feature is missing")?;
        let kind = f.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(kind, "extrude" | "revolve") {
            // The sketch is mirrored once per mirror feature.
            let sk = f.get("sketch").and_then(Value::as_str).ok_or("a solid without a sketch")?;
            if mirrored.insert(sk.to_string()) {
                let sidx = *g.index.get(sk).ok_or_else(|| format!("its sketch '{}' was not imported", short(sk)))?;
                let src = feats.get(sidx).ok_or("its sketch is missing")?;
                let rf = reflect_feature(src, o, n, &rename)?;
                let sd = serde_json::from_value::<SketchF>(rf).map_err(|e| e.to_string())?;
                sketch(g, &sd)?;
            }
        }
        let rf = reflect_feature(f, o, n, &rename)?;
        match kind {
            "extrude" | "revolve" => {
                let sd = serde_json::from_value::<SolidF>(rf).map_err(|e| e.to_string())?;
                solid(g, s, &sd, kind == "revolve")?;
                last = Some(sd.name);
            }
            "hole" => {
                let hd = serde_json::from_value::<HoleF>(rf).map_err(|e| e.to_string())?;
                hole(g, s, &hd)?;
                last = Some(hd.name);
            }
            "fillet" | "chamfer" => {
                let ed = serde_json::from_value::<EdgeF>(rf).map_err(|e| e.to_string())?;
                edges(g, s, &ed, kind == "fillet")?;
                last = Some(ed.name);
            }
            other => return Err(format!("a {} feature cannot be mirrored", short(other))),
        }
    }
    // Patterns and later mirrors name the mirror by its last copy.
    let last = last.and_then(|l| g.features.get(&l).cloned()).ok_or("nothing was mirrored")?;
    g.features.insert(d.name.clone(), last);
    Ok(())
}
