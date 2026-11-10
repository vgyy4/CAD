//! Version 2 of the feature-tree format (ADR 0042 amendment): holes, linear and circular
//! patterns, mirrors and shells. Faces are found by a point on them, like edges by midpoint.

use super::{
    Axis, BASES, EdgeF, Gen, MAX_PICKS, SketchF, SolidF, V3, cross, dot, edges, finite, norm, num, refine_default,
    short, sketch, solid, sub,
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
    /// `false` keeps the united copies' split faces (FreeCAD's `Refine = false`, ADR 0042).
    #[serde(default = "refine_default")]
    refine: bool,
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
    /// `false` keeps the united copies' split faces (FreeCAD's `Refine = false`, ADR 0042).
    #[serde(default = "refine_default")]
    refine: bool,
}

/// Bodies combined into a target body: `join`, `cut` or `intersect`.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CombineF {
    name: String,
    target: String,
    tools: Vec<String>,
    operation: String,
    /// `false` keeps the boolean's split faces (FreeCAD's `Refine = false`, ADR 0042).
    #[serde(default = "refine_default")]
    refine: bool,
}

/// A loft through the closed profiles of two or more sketches, in order.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct LoftF {
    name: String,
    sketches: Vec<String>,
    #[serde(default)]
    ruled: bool,
    #[serde(default)]
    operation: Option<String>,
    #[serde(default)]
    target: Option<String>,
    /// `false` keeps the boolean's split faces (FreeCAD's `Refine = false`, ADR 0042).
    #[serde(default = "refine_default")]
    refine: bool,
}

/// A profile sketch swept along curves of a path sketch.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct SweepF {
    name: String,
    sketch: String,
    /// The path sketch; exactly one of it and `helix`.
    #[serde(default)]
    path: Option<String>,
    /// Curve ids of the path sketch; all of its curves when empty.
    #[serde(default)]
    path_curves: Vec<String>,
    /// A helix about a world axis as the path (ADR 0034).
    #[serde(default)]
    helix: Option<HelixF>,
    /// `frenet` (default) or `fixed`.
    #[serde(default)]
    orientation: Option<String>,
    #[serde(default)]
    operation: Option<String>,
    #[serde(default)]
    target: Option<String>,
    /// `false` keeps the boolean's split faces (FreeCAD's `Refine = false`, ADR 0042).
    #[serde(default = "refine_default")]
    refine: bool,
}

/// The helical path of a sweep (ADR 0042 amendment 2026-11-10): the screw motion that carries the
/// profile. The solid depends on the axis, pitch, height and hand; `radius` and `x` only
/// describe where the source drew the helix (the profile itself fixes the document's).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HelixF {
    origin: V3,
    axis: V3,
    /// Direction of the helix start from the axis.
    #[serde(default)]
    x: Option<V3>,
    radius: f64,
    pitch: f64,
    /// The axial travel; exactly one of it and `turns`.
    #[serde(default)]
    height: Option<f64>,
    #[serde(default)]
    turns: Option<f64>,
    #[serde(default)]
    left_handed: bool,
    /// Degrees; only 0 (a cylindrical helix) is supported.
    #[serde(default)]
    taper_angle: Option<f64>,
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
#[derive(Clone)]
struct FaceHit {
    /// The feature that made the body.
    body: String,
    name: String,
    planar: bool,
    normal: V3,
    centroid: V3,
    /// No face on the surface contains the point: this is the nearest centre instead.
    fallback: bool,
}

/// The tessellation of each body of an evaluation, made once per pick round and only for
/// bodies that have a face on the picked surface.
type Meshes = Vec<Option<gusset_kernel::Mesh>>;

fn v3f(p: Option<&[f32; 3]>) -> V3 {
    p.map_or([0.0; 3], |p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
}

/// The closest point of triangle abc to `p` (Ericson, Real-Time Collision Detection).
fn closest_on_triangle(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let at = |o: V3, k: V3, s: f64| [o[0] + k[0] * s, o[1] + k[1] * s, o[2] + k[2] * s];
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 && d1 - d3 > 0.0 {
        return at(a, ab, d1 / (d1 - d3));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 && d2 - d6 > 0.0 {
        return at(a, ac, d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 && (d4 - d3) + (d5 - d6) > 0.0 {
        return at(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let sum = va + vb + vc;
    if sum.abs() < 1e-300 {
        return a;
    }
    let (v, w) = (vb / sum, vc / sum);
    [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
}

/// Whether `point` lies on the mesh face `face` within `tol` mm (a bbox test first).
fn mesh_face_holds(mesh: &gusset_kernel::Mesh, face: usize, point: V3, tol: f64) -> bool {
    let Some(range) = mesh.face_ranges.get(face) else { return false };
    for t in range.start..range.end {
        let i = t as usize * 3;
        let Some(idx) = mesh.indices.get(i..i + 3) else { continue };
        let v = |k: usize| v3f(idx.get(k).and_then(|j| mesh.positions.get(*j as usize)));
        let (a, b, c) = (v(0), v(1), v(2));
        let outside = (0..3).any(|k| {
            let (lo, hi) = (
                a.get(k)
                    .copied()
                    .unwrap_or(0.0)
                    .min(b.get(k).copied().unwrap_or(0.0))
                    .min(c.get(k).copied().unwrap_or(0.0)),
                a.get(k)
                    .copied()
                    .unwrap_or(0.0)
                    .max(b.get(k).copied().unwrap_or(0.0))
                    .max(c.get(k).copied().unwrap_or(0.0)),
            );
            let pk = point.get(k).copied().unwrap_or(0.0);
            pk < lo - tol || pk > hi + tol
        });
        if !outside && norm(sub(point, closest_on_triangle(point, a, b, c))) <= tol {
            return true;
        }
    }
    false
}

/// The face whose surface holds `point` within 0.01 mm (and, when given, whose outward normal
/// is within about 2.5 degrees of `normal`) and whose bounded region, by the body's
/// tessellation, contains it. Planes and cylinders are told apart this way; of several faces
/// holding the point the one with the nearest centre wins. When no face region holds it (the
/// tessellation is coarse, or the point is off the face) the nearest centre among the
/// surface matches is taken and `fallback` is set.
fn find_face(
    kernel: &dyn gusset_kernel::Kernel,
    ev: &gusset_eval::Evaluation,
    meshes: &mut Meshes,
    point: V3,
    normal: Option<V3>,
) -> Result<FaceHit, String> {
    use gusset_kernel::SurfaceKind;
    let want = match normal {
        Some(n) => Some(unit(finite3(n, "a face normal")?).ok_or("a face normal has no direction")?),
        None => None,
    };
    meshes.resize_with(ev.bodies.len(), || None);
    let mut best: Option<(bool, f64, FaceHit)> = None;
    for (bi, b) in ev.bodies.iter().enumerate() {
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
            let mut tol: f64 = 0.01;
            let hit = match f.kind {
                SurfaceKind::Plane => fnorm.is_some_and(|n| {
                    dot(sub(point, f.centroid), n).abs() <= 0.01 && want.is_none_or(|w| dot(n, w) > 0.999)
                }),
                SurfaceKind::Cylinder => f.axis.is_some_and(|a| {
                    let radius = u32::try_from(i).ok().and_then(|i| radius_of.get(&i).copied());
                    // Chords sag below the arc: allow for the tessellation.
                    tol = tol.max(0.05 * radius.unwrap_or(0.0));
                    radius
                        .zip(unit(a.dir))
                        .is_some_and(|(r, d)| (norm(cross(sub(point, a.origin), d)) - r).abs() <= 0.01)
                }),
                _ => false,
            };
            if !hit {
                continue;
            }
            if meshes.get(bi).is_some_and(Option::is_none) {
                let m = kernel.tessellate(&b.shape, &gusset_kernel::TessellationParams::default()).ok();
                if let Some(slot) = meshes.get_mut(bi) {
                    *slot = m;
                }
            }
            let inside = meshes.get(bi).and_then(Option::as_ref).is_some_and(|m| mesh_face_holds(m, i, point, tol));
            let score = norm(sub(point, f.centroid));
            let better = best.as_ref().is_none_or(|x| (inside && !x.0) || (inside == x.0 && score < x.1));
            if better {
                best = Some((
                    inside,
                    score,
                    FaceHit {
                        body: b.feature.clone(),
                        name: name.to_string(),
                        planar: f.kind == SurfaceKind::Plane,
                        normal: fnorm.unwrap_or([0.0, 0.0, 1.0]),
                        centroid: f.centroid,
                        fallback: !inside,
                    },
                ));
            }
        }
    }
    best.map(|x| x.2).ok_or_else(|| format!("no planar or cylindrical face of the model holds the point {point:?}"))
}

/// The persistent name (and body feature) of the face a pick names, for a chamfer's
/// reference face; the same search as holes use.
pub(super) fn pick_face_name(
    g: &mut Gen,
    s: &Session,
    ev: &gusset_eval::Evaluation,
    pick: &FacePick,
) -> Result<(String, String), String> {
    let fp = finite3(pick.point, "the face point")?;
    let mut meshes: Meshes = Vec::new();
    let f = find_face(s.kernel().as_ref(), ev, &mut meshes, fp, pick.normal)?;
    warn_fallback(g, &f, fp);
    Ok((f.body, f.name))
}

fn warn_fallback(g: &mut Gen, f: &FaceHit, point: V3) {
    if f.fallback {
        g.warn(format!("no face contains the point {point:?}: the face {} with the nearest centre was taken", f.name));
    }
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
    let fp = finite3(d.face.point, "the face point")?;
    let mut meshes: Meshes = Vec::new();
    let face = find_face(s.kernel().as_ref(), &ev, &mut meshes, fp, d.face.normal)?;
    warn_fallback(g, &face, fp);
    if !face.planar {
        return Err("a hole goes into a planar face".into());
    }
    // Positions may lie on several faces of the face's plane (separate pocket floors): each
    // face gets its own hole, in the order the faces are first met.
    let mut groups: Vec<(FaceHit, Vec<V3>)> = Vec::new();
    for p in &d.positions {
        let p = finite3(*p, "a hole position")?;
        if dot(sub(p, face.centroid), face.normal).abs() > 0.01 {
            return Err(format!("hole position {p:?} is not on the face"));
        }
        let hit = find_face(s.kernel().as_ref(), &ev, &mut meshes, p, Some(face.normal))?;
        let hit = if hit.planar && !hit.fallback && dot(hit.normal, face.normal) > 0.999 { hit } else { face.clone() };
        match groups.iter_mut().find(|(f, _)| f.name == hit.name && f.body == hit.body) {
            Some((_, ps)) => ps.push(p),
            None => groups.push((hit, vec![p])),
        }
    }
    let first = g.fresh(&d.name, "Hole");
    for (k, (face, ps)) in groups.iter().enumerate() {
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
        let sites: Vec<String> = ps
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let r = sub(*p, origin);
                format!("e{}: [{} mm, {} mm]", i + 1, num(dot(r, fx)), num(dot(r, fy)))
            })
            .collect();
        let name = if k == 0 { first.clone() } else { g.fresh(&format!("{first}_{}", k + 1), "Hole") };
        let _ =
            writeln!(g.text, "\nhole {name} (face: {}, at: [{}], {})", face.name, sites.join(", "), args.join(", "));
        builds(g, s, &name)?;
        g.made.push(name);
    }
    g.features.insert(d.name.clone(), first);
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
            let tn = &g.body_named(t, "target")?;
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
    if !d.refine {
        args.push("refine: false".into());
    }
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
    let mut meshes: Meshes = Vec::new();
    for pick in &d.faces {
        let fp = finite3(pick.point, "a face point")?;
        let f = find_face(s.kernel().as_ref(), &ev, &mut meshes, fp, pick.normal)?;
        warn_fallback(g, &f, fp);
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

/// The text name of the plane through `o` with normal `n`: a base plane when it is one, else
/// an offset or tilted datum plane written to the text (named after `name`).
fn datum_plane(g: &mut Gen, name: &str, o: V3, n: V3) -> Result<String, String> {
    let mut found = None;
    for (base, x, y) in BASES {
        let bn = cross(x, y);
        if (dot(n, bn).abs() - 1.0).abs() < 1e-9 {
            found = Some((base, dot(o, bn)));
            break;
        }
    }
    let plane = match found {
        Some((base, offset)) if offset.abs() < 1e-9 => base.to_string(),
        Some((base, offset)) => {
            let pn = g.fresh(&format!("{name}_plane"), "Plane");
            let _ = writeln!(g.text, "\nplane {pn} (base: {base}, offset: {} mm)", num(offset));
            g.made.push(pn.clone());
            pn
        }
        // A tilted plane: chained datum planes, as for tilted sketches.
        None => {
            let prefix = g.fresh(&format!("{name}_m"), "Plane");
            let placed = super::tilted(&prefix, o, n).ok_or("its plane is degenerate")?;
            for (dn, dl) in &placed.datums {
                if !g.taken.insert(dn.to_ascii_lowercase()) {
                    return Err(format!("the name '{dn}' for its plane is taken"));
                }
                let _ = writeln!(g.text, "\n{dl}");
                g.made.push(dn.clone());
            }
            placed.plane
        }
    };
    Ok(plane)
}

/// The whole body, across a base plane or an offset datum plane.
fn mirror_body(g: &mut Gen, s: &Session, d: &MirrorF, o: V3, n: V3) -> Result<(), String> {
    let name = g.fresh(&d.name, "Mirror");
    let plane = datum_plane(g, &name, o, n)?;
    let mut args = vec![format!("plane: {plane}")];
    if let Some(t) = &d.target {
        let tn = &g.body_named(t, "target")?;
        args.push(format!("body: {tn}"));
    }
    if d.merge.unwrap_or(true) {
        args.push("merge: true".into());
        if !d.refine {
            args.push("refine: false".into());
        }
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
        let mut rf = reflect_feature(f, o, n, &rename)?;
        // A copy's target is the copy of the target when that is copied too.
        if let Some(tg) = f.get("target").and_then(Value::as_str)
            && d.features.iter().any(|x| x == tg)
        {
            set(&mut rf, "target", json!(rename(tg)));
        }
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

pub(super) fn sweep(g: &mut Gen, s: &Session, d: &SweepF) -> Result<(), String> {
    let profile = g
        .sketches
        .get(&d.sketch)
        .ok_or_else(|| format!("its sketch '{}' was not imported", short(&d.sketch)))?
        .name
        .clone();
    let path = match (&d.path, &d.helix) {
        (Some(_), Some(_)) => return Err("a sweep takes a path sketch or a helix, not both".into()),
        (None, None) => return Err("a sweep needs a path sketch or a helix".into()),
        (None, Some(h)) => {
            if !d.path_curves.is_empty() {
                return Err("path_curves belong to a path sketch, not a helix".into());
            }
            helix_path(h)?
        }
        (Some(path), None) => {
            let pm = g.sketches.get(path).ok_or_else(|| format!("its path '{}' was not imported", short(path)))?;
            if d.path_curves.len() > MAX_PICKS {
                return Err("too many path curves".into());
            }
            if d.path_curves.is_empty() {
                pm.name.clone()
            } else {
                let mut ids = Vec::new();
                for c in &d.path_curves {
                    ids.push(pm.ids.get(c).ok_or_else(|| format!("unknown path curve '{}'", short(c)))?.clone());
                }
                format!("{}.curves({})", pm.name, ids.join(", "))
            }
        }
    };
    let orientation = match d.orientation.as_deref().unwrap_or("frenet") {
        o @ ("frenet" | "fixed") => o,
        other => return Err(format!("orientation '{}' is not frenet or fixed", short(other))),
    };
    if d.helix.is_some() && orientation != "frenet" {
        return Err("a helical sweep turns the profile with the helix: orientation must be frenet".into());
    }
    let op = match d.operation.as_deref().unwrap_or("new") {
        "new" => "new",
        "join" => "add",
        "cut" => "cut",
        "intersect" => "intersect",
        other => return Err(format!("operation '{}' is not new, join, cut or intersect", short(other))),
    };
    let name = g.fresh(&d.name, "Sweep");
    let mut args = vec![format!("profile: {profile}"), format!("path: {path}"), format!("orientation: {orientation}")];
    args.push(format!("op: {op}"));
    if op != "new"
        && let Some(t) = &d.target
    {
        let tn = &g.body_named(t, "target")?;
        args.push(format!("body: {tn}"));
    }
    if !d.refine && op != "new" {
        args.push("refine: false".into());
    }
    let _ = writeln!(g.text, "\nsweep {name} ({})", args.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

/// The document's `helix(...)` path of a tree helix. The document's helix climbs a world axis
/// (`X`, `Y` or `Z` through the origin) in its positive direction; its radius and start come
/// from the profile, and it is cylindrical.
fn helix_path(h: &HelixF) -> Result<String, String> {
    finite3(h.origin, "the helix origin")?;
    finite3(h.axis, "the helix axis")?;
    let a = unit(h.axis).ok_or("the helix axis has no direction")?;
    let (name, k) = [("X", 0), ("Y", 1), ("Z", 2)]
        .into_iter()
        .find(|(_, k)| a.iter().enumerate().all(|(i, c)| (c - if i == *k { 1.0 } else { 0.0 }).abs() < 1e-9))
        .ok_or("a helix climbs a world axis in its positive direction (X, Y or Z)")?;
    // The origin must lie on that axis line.
    if h.origin.iter().enumerate().any(|(i, c)| i != k && c.abs() > 1e-6) {
        return Err(format!("the helix axis must pass through the origin (world {name})"));
    }
    positive(Some(h.radius), "the helix radius")?;
    if let Some(x) = h.x {
        finite3(x, "the helix start direction")?;
        let n = norm(x);
        if n.is_nan() || n <= 1e-9 || x.get(k).is_some_and(|c| c.abs() > 1e-6 * n) {
            return Err("the helix start direction must be across the axis".into());
        }
    }
    if h.taper_angle.is_some_and(|t| !t.is_finite() || t.abs() > 1e-9) {
        return Err("a tapered helix is not supported: the document's helix is cylindrical".into());
    }
    let pitch = positive(Some(h.pitch), "the helix pitch")?;
    let height = match (h.height, h.turns) {
        (Some(_), Some(_)) => return Err("a helix takes a height or turns, not both".into()),
        (Some(v), None) => positive(Some(v), "the helix height")?,
        (None, Some(t)) => positive(Some(t * pitch), "the helix turns")?,
        (None, None) => return Err("a helix needs a height or turns".into()),
    };
    if height / pitch > 2000.0 {
        return Err("the helix has more than 2000 turns".into());
    }
    let left = if h.left_handed { ", left: true" } else { "" };
    Ok(format!("helix(axis: {name}, pitch: {} mm, height: {} mm{left})", num(pitch), num(height)))
}

/// The Gusset operation of a tree operation.
fn body_op(op: Option<&str>) -> Result<&'static str, String> {
    match op.unwrap_or("new") {
        "new" => Ok("new"),
        "join" => Ok("add"),
        "cut" => Ok("cut"),
        "intersect" => Ok("intersect"),
        other => Err(format!("operation '{}' is not new, join, cut or intersect", short(other))),
    }
}

pub(super) fn loft(g: &mut Gen, s: &Session, d: &LoftF) -> Result<(), String> {
    if !(2..=MAX_PICKS).contains(&d.sketches.len()) {
        return Err("a loft needs from 2 to 256 sketches".into());
    }
    let mut names = Vec::new();
    for sk in &d.sketches {
        names.push(
            g.sketches.get(sk).ok_or_else(|| format!("its sketch '{}' was not imported", short(sk)))?.name.clone(),
        );
    }
    let op = body_op(d.operation.as_deref())?;
    let name = g.fresh(&d.name, "Loft");
    let mut args = vec![format!("profiles: [{}]", names.join(", "))];
    if d.ruled {
        args.push("ruled: true".into());
    }
    args.push(format!("op: {op}"));
    if op != "new"
        && let Some(t) = &d.target
    {
        let tn = &g.body_named(t, "target")?;
        args.push(format!("body: {tn}"));
    }
    if !d.refine && op != "new" {
        args.push("refine: false".into());
    }
    let _ = writeln!(g.text, "\nloft {name} ({})", args.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

/// A body the source file stores as geometry, not as features (a Fusion base feature, a
/// SolidWorks mirrored stock body): an import of the B-rep the native reader handed over.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct StoredBodyF {
    name: String,
    /// Index into the stored bodies the importer passed with the tree.
    body: usize,
}

pub(super) fn stored_body(g: &mut Gen, s: &mut Session, d: &StoredBodyF, stored: &[Vec<u8>]) -> Result<(), String> {
    let bytes = stored.get(d.body).ok_or_else(|| {
        if stored.is_empty() {
            "a stored body comes only from a native importer, not a tree file".to_string()
        } else {
            format!("stored body {} does not exist ({} were read)", d.body, stored.len())
        }
    })?;
    let hex = s.put_blob(bytes).to_hex();
    let name = g.fresh(&d.name, "Body");
    let _ = writeln!(g.text, "\nimport {name} (blob: \"{hex}\", format: brep, file: \"stored.brep\")");
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), name);
    Ok(())
}

pub(super) fn combine(g: &mut Gen, s: &Session, d: &CombineF) -> Result<(), String> {
    if d.tools.is_empty() || d.tools.len() > MAX_PICKS {
        return Err("a combine takes between 1 and 256 tool bodies".into());
    }
    let op = match body_op(Some(&d.operation))? {
        "new" => return Err("a combine joins, cuts or intersects".into()),
        op => op,
    };
    let named = |t: &String| g.body_named(t, "body");
    let target = named(&d.target)?;
    let mut tools = Vec::new();
    for t in &d.tools {
        let n = named(t)?;
        if n == target || tools.contains(&n) {
            return Err("a combine's bodies must differ".into());
        }
        tools.push(n);
    }
    let name = g.fresh(&d.name, "Combine");
    let unrefined = if d.refine { "" } else { ", refine: false" };
    let _ = writeln!(g.text, "\ncombine {name} (body: {target}, tools: [{}], op: {op}{unrefined})", tools.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    // The combined body keeps its target's name for later references.
    g.features.insert(d.name.clone(), target);
    Ok(())
}

/// A turn about an axis (degrees), applied before the translation.
#[derive(Debug, Clone, Deserialize)]
struct Rotate {
    axis: [f64; 3],
    #[serde(default)]
    origin: [f64; 3],
    angle: f64,
}

/// A whole body moved rigidly (or copied): the rotation first, then the translation.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct MoveBodyF {
    name: String,
    target: String,
    #[serde(default)]
    translate: Option<[f64; 3]>,
    #[serde(default)]
    rotate: Option<Rotate>,
    #[serde(default)]
    copy: bool,
}

fn mm3(v: V3) -> String {
    format!("[{} mm, {} mm, {} mm]", num(v[0]), num(v[1]), num(v[2]))
}

pub(super) fn move_body(g: &mut Gen, s: &Session, d: &MoveBodyF) -> Result<(), String> {
    let target = g.body_named(&d.target, "target")?;
    let mut args = vec![format!("body: {target}")];
    let mut motion = false;
    if let Some(t) = d.translate {
        let t = finite3(t, "the translation")?;
        for c in t {
            if c.abs() > MAX_LENGTH {
                return Err("the translation is too long".into());
            }
        }
        args.push(format!("translate: {}", mm3(t)));
        motion = true;
    }
    if let Some(r) = &d.rotate {
        let axis = unit(finite3(r.axis, "the rotation axis")?).ok_or("the rotation axis has no direction")?;
        let origin = finite3(r.origin, "the rotation origin")?;
        finite(r.angle, "the rotation angle")?;
        args.push(format!("axis: [{}, {}, {}]", fine(axis[0]), fine(axis[1]), fine(axis[2])));
        args.push(format!("angle: {} deg", num(r.angle)));
        args.push(format!("through: {}", mm3(origin)));
        motion = true;
    }
    if !motion {
        return Err("it has neither a translation nor a rotation".into());
    }
    if d.copy {
        args.push("copy: true".into());
    }
    let name = g.fresh(&d.name, if d.copy { "Copy" } else { "Move" });
    let _ = writeln!(g.text, "\nmove {name} ({})", args.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    // A moved body keeps its name for later references; a copy is the new one.
    g.features.insert(d.name.clone(), if d.copy { name } else { target });
    Ok(())
}

/// A whole body scaled about a point: one factor, or one per world axis.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ScaleBodyF {
    name: String,
    target: String,
    #[serde(default)]
    factor: Option<f64>,
    #[serde(default)]
    factors: Option<[f64; 3]>,
    #[serde(default)]
    center: Option<[f64; 3]>,
}

pub(super) fn scale_body(g: &mut Gen, s: &Session, d: &ScaleBodyF) -> Result<(), String> {
    let target = g.body_named(&d.target, "target")?;
    let factor = match (d.factor, d.factors) {
        (Some(f), None) => format!("factor: {}", fine(positive(Some(f), "the scale factor")?)),
        (None, Some(f)) => {
            let [x, y, z] = f.map(|c| positive(Some(c), "a scale factor"));
            format!("factors: [{}, {}, {}]", fine(x?), fine(y?), fine(z?))
        }
        _ => return Err("give either 'factor' or 'factors'".into()),
    };
    let mut args = vec![format!("body: {target}"), factor];
    if let Some(c) = d.center {
        let c = finite3(c, "the scaling centre")?;
        args.push(format!("about: {}", mm3(c)));
    }
    let name = g.fresh(&d.name, "Scale");
    let _ = writeln!(g.text, "\nscale {name} ({})", args.join(", "));
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), target);
    Ok(())
}

/// A body dropped from the result; later features cannot name it.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct RemoveBodyF {
    name: String,
    target: String,
}

pub(super) fn remove_body(g: &mut Gen, s: &Session, d: &RemoveBodyF) -> Result<(), String> {
    let target = g.body_named(&d.target, "target")?;
    let name = g.fresh(&d.name, "Remove");
    let _ = writeln!(g.text, "\nremove {name} (body: {target})");
    builds(g, s, &name)?;
    g.made.push(name.clone());
    let gone: Vec<String> = g.features.iter().filter(|(_, v)| **v == target).map(|(k, _)| k.clone()).collect();
    for k in gone {
        g.features.remove(&k);
        g.removed.insert(k, d.name.clone());
    }
    Ok(())
}

/// Taper faces for molding about their line on a neutral plane.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct DraftF {
    name: String,
    faces: Vec<FacePick>,
    plane: MirrorPlane,
    angle: f64,
    #[serde(default)]
    flip: bool,
}

pub(super) fn draft(g: &mut Gen, s: &Session, d: &DraftF) -> Result<(), String> {
    if d.faces.is_empty() || d.faces.len() > MAX_PICKS {
        return Err("a draft tapers between 1 and 256 faces".into());
    }
    finite(d.angle, "the draft angle")?;
    let n = unit(finite3(d.plane.normal, "the neutral plane")?).ok_or("the neutral plane has no normal")?;
    let o = finite3(d.plane.origin, "the neutral plane")?;
    let ev = g.evaluate(s).map_err(|e| e.message)?;
    let mut body: Option<String> = None;
    let mut names: Vec<String> = Vec::new();
    for pick in &d.faces {
        let (b, face) = pick_face_name(g, s, &ev, pick)?;
        if body.as_ref().is_some_and(|x| *x != b) {
            return Err("its faces are on several bodies".into());
        }
        body = Some(b);
        if !names.contains(&face) {
            names.push(face);
        }
    }
    let body = body.ok_or("no faces")?;
    let name = g.fresh(&d.name, "Draft");
    let plane = datum_plane(g, &name, o, n)?;
    let flip = if d.flip { ", flip: true" } else { "" };
    let _ = writeln!(
        g.text,
        "\ndraft {name} (body: {body}, faces: [{}], plane: {plane}, angle: {} deg{flip})",
        names.join(", "),
        num(d.angle)
    );
    builds(g, s, &name)?;
    g.made.push(name.clone());
    g.features.insert(d.name.clone(), body);
    Ok(())
}
