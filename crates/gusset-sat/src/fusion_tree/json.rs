//! The feature tree JSON of a design: one entry per timeline feature, as the prototype wrote
//! it, then the check that every feature was read in full.

use super::feature::{self, Params, extrude_raw, feature_name, params_of, pget, point3};
use super::geom::{self, V3, dot, mm, round6, round9};
use super::naming::{self, RecipeKind};
use super::sketch::{self, SKETCH_FEAT};
use super::stream::ref_ids;
use super::stream::{self, Design};
use super::{Ctx, EXTRUDE, Graph, PARAM, SWEEP, component_bodies, component_name, timeline};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

/// Timeline entries that create no body geometry (none is the creator of a body link or
/// references a body record in the samples).
const NON_GEOMETRIC: [(&str, &str); 11] = [
    ("78B9DEFC", "CreateComponent"),
    ("54F9ACE8", "component activation"),
    ("02E8AA49", "JointOrigin"),
    ("8DFDD543", "Assemble (joint)"),
    ("AA8B3D40", "As-built joint"),
    ("8EE00B00", "Position (assembly)"),
    ("0EE4A83E", "Canvas (image)"),
    ("1339A425", "Decal"),
    ("D869265F", "WorkPlane (construction)"),
    ("B0EEF272", "PCB link (bodies come from the extrudes that follow)"),
    ("A917FE80", "timeline group (a folder: its members follow it in the timeline)"),
];

fn op_name(op: u8) -> String {
    match op {
        1 => "join".into(),
        2 => "cut".into(),
        3 => "intersect".into(),
        4 => "new".into(),
        n => format!("unknown({n})"),
    }
}

fn v3mm(p: V3) -> Value {
    json!([mm(p[0]), mm(p[1]), mm(p[2])])
}

fn sketch_json(cx: &mut Ctx, fid: u64, name: &str) -> Option<(Value, u64)> {
    let d = cx.d;
    let sid = ref_ids(d, d.rec(fid)).into_iter().find(|q| d.is(*q, super::sketch::SKETCH))?;
    let s = cx.sketch(sid);
    let mut j = Map::new();
    j.insert("type".into(), "sketch".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), sid.into());
    match &s.plane {
        Some(pl) => {
            j.insert("plane".into(), json!({"origin": v3mm(pl.origin), "x_axis": pl.x, "y_axis": pl.y}));
            if s.identity_assumed {
                j.insert("plane_note".into(), "no matrix stored; identity (component XY) assumed".into());
            }
        }
        None => {
            j.insert("plane".into(), Value::Null);
        }
    }
    let points: Vec<Value> =
        s.points.iter().map(|(q, p)| json!({"id": format!("p{q}"), "x": mm(p[0]), "y": mm(p[1])})).collect();
    j.insert("points".into(), points.into());
    let mut curves = Vec::new();
    for (q, l) in &s.lines {
        if let (Some(a), Some(b)) = (l.start_ref, l.end_ref)
            && s.points.contains_key(&a)
            && s.points.contains_key(&b)
        {
            curves
                .push(json!({"id": format!("c{q}"), "type": "line", "start": format!("p{a}"), "end": format!("p{b}")}));
        }
    }
    for (q, a) in &s.arcs {
        let full = ((a.a1 - a.a0).abs() - 2.0 * std::f64::consts::PI).abs() < 1e-9;
        let (c, e, st) = (a.refs.first().copied(), a.refs.get(1).copied(), a.refs.get(2).copied());
        // Stored order: centre, end, start; counter-clockwise about +z swaps them for a -z normal.
        let (st, en) = if a.normal[2] < 0.0 { (e, st) } else { (st, e) };
        let has = |x: Option<u64>| x.filter(|x| s.points.contains_key(x));
        if full && has(c).is_some() {
            curves.push(json!({"id": format!("c{q}"), "type": "circle", "center": format!("p{}", c.unwrap_or(0)), "radius": mm(a.radius)}));
        } else if let (Some(c), Some(st), Some(en)) = (has(c), has(st), has(en)) {
            curves.push(json!({"id": format!("c{q}"), "type": "arc", "center": format!("p{c}"), "start": format!("p{st}"), "end": format!("p{en}")}));
        }
    }
    j.insert("curves".into(), curves.into());
    if !s.other.is_empty() {
        let mut kinds: HashMap<String, u64> = HashMap::new();
        for c in s.other.values() {
            *kinds.entry(c.get(..8).unwrap_or("").to_string()).or_default() += 1;
        }
        j.insert("unsupported_entities".into(), json!(kinds));
    }
    Some((Value::Object(j), sid))
}

/// The side and reach of a through-all cut, measured on the stored bodies: copies of the profile's
/// straight edges at offsets along the normal (cm).
fn through_reach(cx: &mut Ctx, fid: u64) -> Option<f64> {
    let g = cx.ext_geom(fid)?;
    let (pl, z) = (g.pl, g.pl.z);
    let sets = cx.edge_sets();
    let mut ts: Vec<f64> = Vec::new();
    for sg in &g.segs {
        if !g.sketch.lines.contains_key(&sg.curve) {
            continue;
        }
        let (a, b) = (*sg.poly.first()?, *sg.poly.last()?);
        let (pa, pb) = (geom::world(&pl, a, 0.0), geom::world(&pl, b, 0.0));
        let u = geom::sub(pb, pa);
        if geom::norm(u) < 1e-12 {
            continue;
        }
        for es in sets {
            for (p0, p1) in es {
                let v = geom::sub(*p1, *p0);
                if geom::norm(v) < 1e-9 || ((dot(v, u).abs() / (geom::norm(v) * geom::norm(u))) - 1.0).abs() > 1e-9 {
                    continue;
                }
                let w = geom::sub(*p0, pa);
                let t = dot(w, z);
                let off = geom::sub(w, geom::mul(z, t));
                let perp = geom::sub(off, geom::mul(u, dot(off, u) / dot(u, u)));
                if geom::norm(perp) < 1e-6 && t.abs() > 1e-6 {
                    let t = (t * 1e7).round() / 1e7;
                    if !ts.contains(&t) {
                        ts.push(t);
                    }
                }
            }
        }
    }
    let pos: Vec<f64> = ts.iter().copied().filter(|t| *t > 0.0).collect();
    let neg: Vec<f64> = ts.iter().copied().filter(|t| *t < 0.0).collect();
    match (pos.is_empty(), neg.is_empty()) {
        (false, true) => Some(pos.into_iter().fold(f64::NEG_INFINITY, f64::max)),
        (true, false) => Some(neg.into_iter().fold(f64::INFINITY, f64::min)),
        _ => None,
    }
}

fn extrude_json(cx: &mut Ctx, fid: u64, name: &str, sketch_names: &HashMap<u64, String>) -> Map<String, Value> {
    let d = cx.d;
    let mut j = Map::new();
    j.insert("type".into(), "extrude".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), fid.into());
    let Some(e) = extrude_raw(d, fid) else {
        j.insert("error".into(), "layout not recognised".into());
        return j;
    };
    j.insert("sketch".into(), e.sketch.and_then(|s| sketch_names.get(&s)).map_or(Value::Null, |s| s.as_str().into()));
    j.insert("sketch_source_id".into(), e.sketch.map_or(Value::Null, Value::from));
    j.insert("profiles".into(), "all".into());
    let plane = e.sketch.map(|s| cx.sketch(s)).and_then(|s| s.plane);
    if let Some(sid) = e.sketch {
        let s = cx.sketch(sid);
        match cx.rc.regions(d, fid, &s) {
            Ok(rg) => {
                j.insert(
                    "profiles".into(),
                    rg.iter().map(|r| json!({"point": [mm(r.point[0]), mm(r.point[1])]})).collect::<Vec<_>>().into(),
                );
            }
            Err(why) => {
                j.insert("profiles".into(), Value::Null);
                j.insert("profile_note".into(), format!("profile regions not decoded: {why}").into());
            }
        }
    }
    let ps = &e.params;
    let along = pget(ps, "AlongDistance");
    let against = pget(ps, "AgainstDistance");
    let sign = match (&plane, e.vec) {
        (Some(pl), Some(v)) => {
            if dot(pl.z, v) >= 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        _ => 1.0,
    };
    let mut distance = along.and_then(|a| a.value).map(|v| mm(v) * sign);
    if let Some(a) = along {
        j.insert("distance_expr".into(), a.expr.as_str().into());
    }
    j.insert("symmetric".into(), (e.extent == 3).into());
    if let (2, Some(ag)) = (e.extent, against) {
        j.insert("two_sided".into(), true.into());
        j.insert("distance2".into(), ag.value.map_or(Value::Null, |v| (mm(v) * -sign).into()));
        j.insert("distance2_expr".into(), ag.expr.as_str().into());
    }
    if distance.is_none() && pget(ps, "Side1Offset").is_some() {
        // To-object: the first face of the extrude's bounded-face recipe, when it is a plane
        // parallel to the sketch.
        let off = pget(ps, "Side1Offset").and_then(|p| p.value).unwrap_or(0.0);
        for rid in cx.edge_recipes_of(fid) {
            let (kind, faces) = naming::recipe(d, rid);
            let (Some(RecipeKind::BoundedFace), Some(faces)) = (kind, faces) else { continue };
            let f = faces.first().and_then(|f| f.first()).and_then(|(g, ops)| cx.face(g, ops));
            if let (Some(naming::FaceG::Plane { n, p, .. }), Some(pl)) = (f, &plane)
                && (dot(n, pl.z).abs() - 1.0).abs() < 1e-9
            {
                let t = dot(n, geom::sub(p, pl.origin)) / dot(n, pl.z);
                distance = Some(mm(t + if t >= 0.0 { off } else { -off }));
                j.insert("extent_from".into(), "to object".into());
            }
            break;
        }
    }
    if distance.is_none()
        && along.is_none()
        && pget(ps, "Side1Offset").is_none()
        && e.sketch.is_some()
        && !cx.acis.is_empty()
    {
        // Through all: measured on the stored body.
        if let Some(t) = through_reach(cx, fid).filter(|t| *t != 0.0) {
            distance = Some(mm(t));
            j.insert("extent_from".into(), "through all (reach measured on the stored body)".into());
        }
    }
    j.insert("distance".into(), distance.map_or(Value::Null, Value::from));
    if distance.is_none() {
        j.insert("extent".into(), "not a distance (to object / through all)".into());
    }
    j.insert("operation".into(), op_name(e.op).into());
    j.insert("target".into(), Value::Null);
    if let Some(t) = pget(ps, "TaperAngle").and_then(|p| p.value).filter(|t| *t != 0.0) {
        j.insert("taper_deg".into(), round6(t.to_degrees()).into());
    }
    if e.sketch.is_none() {
        j.insert("profile_note".into(), "profile is not a sketch profile (face or PCB data)".into());
    }
    j
}

/// The bodies a feature made, and for a join/cut/intersect the feature(s) whose bodies it acted on.
fn op_target(j: &mut Map<String, Value>, g: &Graph, d: &Design, fid: u64, names: &HashMap<u64, String>) -> usize {
    let links = g.body_links(d, fid, 2);
    let made = links.iter().filter(|l| g.creator.get(l) == Some(&fid)).count();
    let mut acted: Vec<String> = links
        .iter()
        .filter_map(|l| g.creator.get(l).copied())
        .filter(|c| *c != fid)
        .map(|c| names.get(&c).cloned().unwrap_or_else(|| c.to_string()))
        .collect();
    acted.sort();
    acted.dedup();
    if matches!(j.get("operation").and_then(Value::as_str), Some("join" | "cut" | "intersect")) {
        let t = match acted.as_slice() {
            [] => Value::Null,
            [one] => one.as_str().into(),
            many => many.to_vec().into(),
        };
        j.insert("target".into(), t);
    }
    made
}

/// Sweep `FCBB1707`: the operation enum is the first u32 of the body; the profile is a `4BD53E5A`
/// (names its sketch), the path a list of `5A1BF548` -> `E2CEFD18` sketch-curve references
/// (`u64 sketch data id`, `u64 crv_primary_id`).
fn sweep_json(
    cx: &mut Ctx,
    fid: u64,
    name: &str,
    sketch_names: &HashMap<u64, String>,
) -> (Map<String, Value>, Option<f64>) {
    let d = cx.d;
    let mut j = Map::new();
    j.insert("type".into(), "sweep".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), fid.into());
    let rec = d.rec(fid);
    let Some(h) = stream::parse_header(rec) else {
        j.insert("error".into(), "layout not recognised".into());
        return (j, None);
    };
    let op = rec.get(h.body).copied().unwrap_or(0);
    let profile = feature::profile_sketch_of(d, rec);
    j.insert("sketch".into(), profile.and_then(|s| sketch_names.get(&s)).map_or(Value::Null, |s| s.as_str().into()));
    j.insert("sketch_source_id".into(), profile.map_or(Value::Null, Value::from));
    let mut path_sketch: Option<u64> = None;
    let mut curves: Vec<String> = Vec::new();
    let mut curve_ids: Vec<u64> = Vec::new();
    let mut ok = true;
    for q in ref_ids(d, rec) {
        if !d.is(q, "5A1BF548") {
            continue;
        }
        for c in ref_ids(d, d.rec(q)) {
            if !d.is(c, "E2CEFD18") {
                continue;
            }
            let r = d.rec(c);
            let word = |o: usize| r.get(o..o + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
            let (Some(sk), Some(prim)) = (word(0x21), word(0x29)) else {
                ok = false;
                continue;
            };
            if path_sketch.is_some_and(|p| p != sk) {
                ok = false;
            }
            path_sketch = Some(sk);
            let s = cx.sketch(sk);
            match sketch::curve_by_primary(d, &s, prim) {
                Some(cid) => {
                    curves.push(format!("c{cid}"));
                    curve_ids.push(cid);
                }
                None => ok = false,
            }
        }
    }
    j.insert("path".into(), path_sketch.and_then(|s| sketch_names.get(&s)).map_or(Value::Null, |s| s.as_str().into()));
    j.insert("path_source_id".into(), path_sketch.map_or(Value::Null, Value::from));
    j.insert("path_curves".into(), curves.into());
    if !ok || path_sketch.is_none() {
        j.insert("error".into(), "sweep path is not a list of sketch curves of one sketch".into());
    }
    j.insert("orientation".into(), "frenet".into());
    j.insert("operation".into(), op_name(op).into());
    j.insert("target".into(), Value::Null);
    let shift = profile.filter(|_| ok).and_then(|p| path_start_offset(cx, p, path_sketch?, &curve_ids));
    (j, shift)
}

/// How far (cm, along the profile's normal) the plane through the start of the path lies from the
/// profile's plane: the chain's end nearest to the profile, or `None` when the path is closed or
/// the profile lies on it already.
fn path_start_offset(cx: &mut Ctx, profile: u64, path: u64, curves: &[u64]) -> Option<f64> {
    let pl = cx.sketch(profile).plane?;
    let s = cx.sketch(path);
    let ppl = s.plane?;
    let mut ends: Vec<(u64, V3)> = Vec::new();
    for q in curves {
        let refs: Vec<u64> = if let Some(l) = s.lines.get(q) {
            [l.start_ref, l.end_ref].into_iter().flatten().collect()
        } else {
            let a = s.arcs.get(q)?;
            a.refs.iter().skip(1).copied().collect()
        };
        for r in refs {
            let p = *s.points.get(&r)?;
            ends.push((r, geom::world(&ppl, [p[0], p[1]], 0.0)));
        }
    }
    let t = ends
        .iter()
        .filter(|(r, _)| ends.iter().filter(|(x, _)| x == r).count() == 1)
        .map(|(_, p)| dot(pl.z, geom::sub(*p, pl.origin)))
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))?;
    (t.abs() > 1e-9).then_some(t)
}

fn hole_json(cx: &mut Ctx, fid: u64, name: &str) -> Map<String, Value> {
    let d = cx.d;
    let ps = params_of(d, fid);
    let mut j = Map::new();
    j.insert("type".into(), "hole".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), fid.into());
    let mut pts: Vec<V3> = Vec::new();
    for q in cx.under(fid, "994C518A", 4, None) {
        if let Some(p) = point3(d, q)
            && !pts.contains(&p)
        {
            pts.push(p);
        }
    }
    let frames: Vec<(V3, V3)> =
        cx.under(fid, "CC54ECAD", 4, None).into_iter().filter_map(|q| feature::plane_frame(d, q)).collect();
    let unsupported = |mut j: Map<String, Value>, why: &str| {
        j.insert("type".into(), "unsupported".into());
        j.insert("fusion_type".into(), "Hole".into());
        j.insert("error".into(), why.into());
        j
    };
    let (Some(p0), Some((_, normal))) = (pts.first().copied(), frames.first().copied()) else {
        return unsupported(j, "hole position / face not found");
    };
    j.insert(
        "face".into(),
        json!({"point": v3mm(p0), "normal": [round9(normal[0]), round9(normal[1]), round9(normal[2])]}),
    );
    j.insert("positions".into(), pts.iter().map(|p| v3mm(*p)).collect::<Vec<_>>().into());
    let Some(diam) = pget(&ps, "HoleDiameter").and_then(|p| p.value) else {
        return unsupported(j, "no HoleDiameter");
    };
    j.insert("diameter".into(), mm(diam).into());
    match pget(&ps, "HoleDepth").and_then(|p| p.value) {
        Some(dep) => {
            j.insert("depth".into(), mm(dep).into());
        }
        None => {
            j.insert("through".into(), true.into());
            j.insert("extent_note".into(), "GUESS: no HoleDepth parameter => through (no sample)".into());
        }
    }
    if let Some(tip) = pget(&ps, "TipAngle").and_then(|p| p.value) {
        j.insert("point_angle".into(), round6(tip.to_degrees()).into());
    }
    let mut variant = "simple";
    // Counterbore / countersink role names are not in any sample: guessed by substring.
    for (k, p) in &ps {
        let kl = k.to_ascii_lowercase();
        let Some(v) = p.value else { continue };
        if kl.contains("bore") && kl.contains("diam") {
            j.insert("cbore_diameter".into(), mm(v).into());
            variant = "counterbore";
        } else if kl.contains("bore") && kl.contains("depth") {
            j.insert("cbore_depth".into(), mm(v).into());
        } else if kl.contains("sink") && kl.contains("diam") {
            j.insert("csink_diameter".into(), mm(v).into());
            variant = "countersink";
        } else if kl.contains("sink") && kl.contains("angle") {
            j.insert("csink_angle".into(), round6(v.to_degrees()).into());
        }
    }
    j.insert("variant".into(), variant.into());
    if variant != "simple" {
        j.insert("variant_note".into(), "GUESS: role names not seen in any sample".into());
    }
    j
}

pub(super) struct PatternInputs<'a> {
    pub(super) name: &'a str,
    pub(super) originals: &'a [String],
    pub(super) count: f64,
    pub(super) count_v: f64,
    pub(super) spacing: f64,
    pub(super) spacing_v: f64,
    pub(super) dirs: &'a [(V3, V3)],
    pub(super) sym: [bool; 3],
    /// A pattern of one whole body (named by the feature that made it) instead of features.
    pub(super) target: Option<&'a str>,
}

/// A rectangular pattern as chained `pattern_linear` features (U first; V takes the originals and
/// the U results). A symmetric direction (the original in the middle, odd count) becomes two
/// patterns, with positive and negative spacing.
pub(super) fn linear_pattern_chain(p: &PatternInputs) -> Option<Vec<Value>> {
    let nu = if p.count == 0.0 { 1 } else { p.count.round() as i64 };
    let nv = if p.count_v == 0.0 { 1 } else { p.count_v.round() as i64 };
    type Job<'a> = (&'a str, (V3, V3), f64, i64, bool);
    let mut jobs: Vec<Job> = Vec::new();
    if nu > 1 {
        jobs.push(("U", *p.dirs.first()?, p.spacing, nu, p.sym[0]));
    }
    if nv > 1 {
        let dv = if p.dirs.len() > 1 {
            p.dirs.get(1)
        } else if nu <= 1 {
            p.dirs.first()
        } else {
            None
        };
        jobs.push(("V", *dv?, p.spacing_v, nv, p.sym[1]));
    }
    // Chained directions copy the earlier copies as features; a body pattern cannot.
    if jobs.is_empty() || (p.target.is_some() && jobs.len() > 1) {
        return None;
    }
    let mut out: Vec<Value> = Vec::new();
    let mut made: Vec<String> = Vec::new();
    for (tag, (_, dir), sp, n, sy) in jobs {
        let parts: Vec<(f64, i64)> = if sy {
            if n % 2 == 0 {
                return None;
            }
            vec![(sp, (n + 1) / 2), (-sp, (n + 1) / 2)]
        } else {
            vec![(sp, n)]
        };
        let mut names_now = Vec::new();
        for (k, (sp_, n_)) in parts.iter().enumerate() {
            let nm = if out.is_empty() {
                p.name.to_string()
            } else {
                format!("{} {}{}", p.name, tag, if parts.len() == 1 { String::new() } else { (k + 1).to_string() })
            };
            let features: Vec<String> = p.originals.iter().chain(made.iter()).cloned().collect();
            out.push(json!({
                "type": "pattern_linear", "name": nm, "features": features,
                "direction": [round9(dir[0]), round9(dir[1]), round9(dir[2])], "spacing": round9(*sp_), "count": n_,
                "target": p.target, "pattern_axis": tag, "symmetric": sy,
                "note": format!("direction = the stored F2A7590D vector (sign included){}", if sy { "; symmetric about the original: split into + and - halves" } else { "" }),
            }));
            names_now.push(nm);
        }
        made.extend(names_now);
    }
    Some(out)
}

pub(super) fn design_json(cx: &mut Ctx) -> Value {
    let d = cx.d;
    let mut feats: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut sketch_names: HashMap<u64, String> = HashMap::new();
    let (tl, extra) = timeline(d);
    let mut used_names: HashSet<String> = HashSet::new();
    let g = Graph::new(d);
    let mut names: HashMap<u64, String> = HashMap::new();
    let mut comps: Vec<(u64, Option<String>, Vec<String>)> = Vec::new();
    for &fid in tl.iter().chain(extra.iter()) {
        let (base, name0) = feature_name(d, fid);
        let class = d.cls(fid);
        let c8 = class.get(..8).unwrap_or("").to_string();
        let mut name = name0;
        if let Some(n) = &mut name {
            if used_names.contains(n.as_str()) {
                // Names restart per component: keep them unique in the tree.
                let mut k = 2;
                while used_names.contains(&format!("{n} ({k})")) {
                    k += 1;
                }
                *n = format!("{n} ({k})");
            }
            used_names.insert(n.clone());
            names.insert(fid, n.clone());
        }
        let nm = name.clone().unwrap_or_default();
        let comp = g.component(d, fid);
        if let Some(c) = comp
            && !comps.iter().any(|x| x.0 == c)
        {
            comps.push((c, component_name(d, c), component_bodies(d, c)));
        }
        let comp_name = comp.and_then(|c| comps.iter().find(|x| x.0 == c)).and_then(|x| x.1.clone());
        let nbefore = feats.len();
        if let Some((_, why)) = NON_GEOMETRIC.iter().find(|(p, _)| *p == c8) {
            skipped.push(json!({"name": name, "fusion_type": base.clone().unwrap_or_else(|| c8.clone()), "reason": why, "source_id": fid}));
            continue;
        }
        let fusion_type = base.clone().unwrap_or_else(|| c8.clone());
        if d.is(fid, SKETCH_FEAT) {
            match sketch_json(cx, fid, &nm) {
                Some((j, sid)) => {
                    feats.push(j);
                    sketch_names.insert(sid, nm.clone());
                }
                None => feats.push(json!({"type": "sketch", "name": nm, "error": "sketch data not found"})),
            }
        } else if d.is(fid, EXTRUDE) {
            let mut j = extrude_json(cx, fid, &nm, &sketch_names);
            let made = op_target(&mut j, &g, d, fid, &names);
            j.insert("creates_bodies".into(), made.into());
            feats.push(Value::Object(j));
        } else if d.is(fid, SWEEP) {
            let (mut j, shift) = sweep_json(cx, fid, &nm, &sketch_names);
            if let Some(sid) = j.get("sketch_source_id").and_then(Value::as_u64) {
                // The profile is one loop of its sketch, not all of it; and Fusion sweeps a
                // profile that is not on the path's start plane from the plane through the path's
                // start. Gusset wants the sketch to hold exactly the profile, on that plane: a
                // copy of the sketch with only the profile's curves, moved along its normal.
                let sk = cx.sketch(sid);
                let used: Option<HashSet<String>> =
                    cx.rc.regions(d, fid, &sk).ok().map(|rg| {
                        rg.iter().flat_map(|r| r.segments.iter().map(|sg| format!("c{}", sg.curve))).collect()
                    });
                let moved = feats
                    .iter()
                    .find(|f| {
                        f.get("type").and_then(Value::as_str) == Some("sketch")
                            && f.get("source_id").and_then(Value::as_u64) == Some(sid)
                    })
                    .cloned();
                if let Some(Value::Object(mut c)) = moved {
                    let before = c.get("curves").and_then(Value::as_array).map_or(0, Vec::len);
                    if let (Some(used), Some(cs)) = (&used, c.get_mut("curves").and_then(Value::as_array_mut)) {
                        cs.retain(|x| x.get("id").and_then(Value::as_str).is_some_and(|i| used.contains(i)));
                    }
                    let after = c.get("curves").and_then(Value::as_array).map_or(0, Vec::len);
                    if let (Some(t), Some(pl)) = (shift, sk.plane.as_ref()) {
                        let o = geom::add(pl.origin, geom::mul(pl.z, t));
                        c.insert("plane".into(), json!({"origin": v3mm(o), "x_axis": pl.x, "y_axis": pl.y}));
                        c.insert(
                            "plane_note".into(),
                            "moved along its normal to the start plane of a sweep path".into(),
                        );
                    }
                    if shift.is_some() || after != before {
                        let copy = format!("{nm} profile");
                        c.insert("name".into(), copy.clone().into());
                        j.insert("sketch".into(), copy.into());
                        feats.push(Value::Object(c));
                    }
                }
            }
            let made = op_target(&mut j, &g, d, fid, &names);
            j.insert("creates_bodies".into(), made.into());
            feats.push(Value::Object(j));
        } else if d.is(fid, "A07D5F17") || d.is(fid, "F757D611") {
            let fil = d.is(fid, "A07D5F17");
            let key = if fil { "radius" } else { "distance" };
            let role: &[&str] = if fil { &["Radius"] } else { &["Distance", "leftDistance"] };
            for (k, (v, es, bad)) in cx.blend_sets(fid, role).into_iter().enumerate() {
                let mut j = Map::new();
                j.insert("type".into(), if fil { "fillet" } else { "chamfer" }.into());
                j.insert("name".into(), if k == 0 { nm.clone() } else { format!("{nm} set {}", k + 1) }.into());
                j.insert("source_id".into(), fid.into());
                j.insert(key.into(), v.map_or(Value::Null, |v| mm(v).into()));
                j.insert("edges".into(), es.iter().map(|m| json!({"midpoint": v3mm(*m)})).collect::<Vec<_>>().into());
                if bad > 0 {
                    j.insert("edges_unresolved".into(), bad.into());
                }
                if es.is_empty() && bad == 0 {
                    j.insert("edges_note".into(), "no edge recipes found".into());
                }
                feats.push(Value::Object(j));
            }
        } else if d.is(fid, "E3849A15") {
            feats.push(json!({"type": "unsupported", "fusion_type": "Revolve", "name": nm, "source_id": fid,
                "error": "the revolve axis is a topological reference (not decoded)"}));
        } else if d.is(fid, "1C037A07") {
            feats.push(Value::Object(hole_json(cx, fid, &nm)));
        } else if d.is(fid, "D1728651") {
            let ins = g.inputs(d, fid);
            let in_names: Vec<String> =
                ins.iter().map(|q| names.get(q).cloned().unwrap_or_else(|| q.to_string())).collect();
            let plane = cx.under(fid, "CC54ECAD", 4, None).into_iter().find_map(|q| feature::plane_frame(d, q));
            match plane {
                Some((o, n)) if !ins.is_empty() && ins.iter().all(|q| names.contains_key(q)) => {
                    feats.push(json!({"type": "mirror", "name": nm, "source_id": fid, "features": in_names,
                        "plane": {"origin": v3mm(o), "normal": [round9(n[0]), round9(n[1]), round9(n[2])]}}));
                }
                _ => feats.push(
                    json!({"type": "unsupported", "gtree_type": "mirror", "fusion_type": fusion_type, "name": nm,
                    "source_id": fid, "features": in_names,
                    "note": "the mirror plane is not an origin/construction plane frame, or it mirrors bodies"}),
                ),
            }
        } else if d.is(fid, "AE12AD3F") || d.is(fid, "E8816367") || d.is(fid, "11F1A5CE") {
            let linear = !d.is(fid, "11F1A5CE");
            let ins = g.inputs(d, fid);
            let mut ps: Params = Vec::new();
            for q in ref_ids(d, d.rec(fid)) {
                if d.is(q, "D91D429C") || d.is(q, "A085449A") {
                    for q2 in ref_ids(d, d.rec(q)) {
                        if d.is(q2, PARAM)
                            && let Some(pp) = super::stream::param(d, q2)
                            && let Some(n) = pp.name.clone()
                        {
                            ps.retain(|(k, _)| *k != n);
                            ps.push((n, pp));
                        }
                    }
                }
            }
            let val = |n: &str| pget(&ps, n).and_then(|p| p.value);
            let in_names: Vec<String> =
                ins.iter().map(|q| names.get(q).cloned().unwrap_or_else(|| q.to_string())).collect();
            let dirs = feature::pattern_dirs(d, fid);
            // A pattern of bodies names no feature: the bodies it copies are the ones its input
            // records reach, each named by the feature that made it.
            let mut body_in: Vec<String> = Vec::new();
            if ins.is_empty() {
                for l in g.selected_bodies(d, fid) {
                    if let Some(c) = g.creator.get(&l).copied().filter(|c| *c != fid)
                        && let Some(n) = names.get(&c)
                        && !body_in.contains(n)
                    {
                        body_in.push(n.clone());
                    }
                }
            }
            // One new body per copy: fewer means the file suppresses some instances (the sample
            // C-Pattern of 12 makes 9 bodies), which a tree pattern cannot say.
            let made = g.body_links(d, fid, 4).iter().filter(|l| g.creator.get(l) == Some(&fid)).count();
            let copies = {
                let n = val("countU").unwrap_or(1.0).round().max(1.0) as usize;
                if linear { n * val("CountV").unwrap_or(1.0).round().max(1.0) as usize } else { n }
            };
            let target: Option<&str> = match body_in.as_slice() {
                [one] if made + 1 == copies => Some(one.as_str()),
                _ => None,
            };
            let inputs_ok = if ins.is_empty() { target.is_some() } else { ins.iter().all(|q| names.contains_key(q)) };
            if linear && inputs_ok && !dirs.is_empty() && val("countU").is_some() {
                let sym = feature::pattern_symmetric(d, fid).unwrap_or([false; 3]);
                let p = PatternInputs {
                    name: &nm,
                    originals: &in_names,
                    count: val("countU").unwrap_or(1.0),
                    count_v: val("CountV").unwrap_or(1.0),
                    spacing: val("uSpaceDistance").map_or(0.0, mm),
                    spacing_v: val("vSpaceDistance").map_or(0.0, mm),
                    dirs: &dirs,
                    sym,
                    target,
                };
                if let Some(chain) = linear_pattern_chain(&p) {
                    feats.extend(chain.into_iter().map(|mut v| {
                        if let Some(o) = v.as_object_mut() {
                            o.insert("source_id".into(), fid.into());
                        }
                        v
                    }));
                    tag_feats(&mut feats, nbefore, comp.map(|c| (c, comp_name.as_deref())), extra.contains(&fid));
                    continue;
                }
            }
            // Circular: `countU` copies over `TotalAngle` about the picked axis (point and vector
            // as for the linear directions). Only the full circle is converted: how a partial
            // angle spreads the copies is not established by a sample.
            if !linear
                && inputs_ok
                && let Some((pt, dir)) = dirs.first()
                && let Some(count) = val("countU").filter(|c| (2.0..=500.0).contains(c))
                && let Some(angle) = val("TotalAngle").map(f64::to_degrees)
                && (angle - 360.0).abs() < 1e-6
            {
                feats.push(json!({"type": "pattern_circular", "name": nm, "source_id": fid,
                    "features": in_names, "target": target,
                    "axis": {"origin": v3mm(*pt), "direction": [round9(dir[0]), round9(dir[1]), round9(dir[2])]},
                    "angle": 360.0, "count": count.round() as i64,
                    "note": "axis = the stored F2A7590D point and vector"}));
                tag_feats(&mut feats, nbefore, comp.map(|c| (c, comp_name.as_deref())), extra.contains(&fid));
                continue;
            }
            // A pattern of the occurrences of a component: the copies are occurrences the file
            // places, so the component's features are written once per occurrence (`place`).
            if ins.is_empty() && body_in.is_empty() && super::occpattern::copies_of(d, fid).is_ok() {
                skipped.push(json!({"name": name, "fusion_type": fusion_type, "source_id": fid,
                    "reason": "pattern of components: its copies are occurrences of the component, placed like its other occurrences"}));
                continue;
            }
            let mut j = json!({"type": "unsupported", "gtree_type": if linear { "pattern_linear" } else { "pattern_circular" },
                "fusion_type": fusion_type, "name": nm, "source_id": fid, "features": in_names,
                "note": "pattern of components, of several bodies, with a partial angle, or no direction record: not converted"});
            if let (Some(o), Some((_, dir))) = (j.as_object_mut(), dirs.first()) {
                o.insert("direction".into(), json!([round9(dir[0]), round9(dir[1]), round9(dir[2])]));
            }
            feats.push(j);
        } else if c8 == "E8063467" {
            feats.push(shell_json(cx, fid, &nm, &fusion_type, class));
        } else if c8 == "F4163A07" {
            feats.push(super::bodyops::draft_json(cx, fid, &nm, &fusion_type, class));
        } else if matches!(c8.as_str(), "64E40C8E" | "D45FD6C4" | "4A782808") {
            let ed = super::bodyops::Edit { d, g: &g, names: &names, feats: &feats };
            let j = match c8.as_str() {
                "64E40C8E" => ed.move_json(fid, &nm, &fusion_type, class),
                "D45FD6C4" => ed.scale_json(fid, &nm, &fusion_type, class),
                _ => ed.remove_json(fid, &nm, &fusion_type, class),
            };
            feats.push(j);
        } else if c8 == "428B5C6E" {
            feats.push(insert_json(d, fid, &nm, &fusion_type, class));
        } else if c8 == "D7015B81" {
            // Base Feature: its visible bodies are bodies the file stores (section 13), handed
            // to the importer as `stored_body` features.
            match base_feature_bodies(d, &g, fid) {
                Ok(bodies) if bodies.is_empty() => {
                    skipped.push(json!({"name": name, "fusion_type": fusion_type, "reason": "Base Feature that makes no body", "source_id": fid}));
                    continue;
                }
                Ok(bodies) => {
                    let n = bodies.len();
                    for (k, (brep, index, count)) in bodies.into_iter().enumerate() {
                        let mut bn = if k == 0 { nm.clone() } else { format!("{nm} {}", k + 1) };
                        while k > 0 && used_names.contains(&bn) {
                            bn.push('_');
                        }
                        used_names.insert(bn.clone());
                        feats.push(json!({"type": "stored_body", "name": bn, "source_id": fid, "brep": brep,
                            "index": index, "count": count, "creates_bodies": n}));
                    }
                }
                Err(why) => feats.push(json!({"type": "unsupported", "fusion_type": fusion_type, "name": name,
                    "class_guid": class, "source_id": fid, "error": why})),
            }
        } else {
            let mut j = json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class, "source_id": fid});
            if c8 == "D7015B81" {
                let n = g.creator.values().filter(|f| **f == fid).count();
                if let Some(o) = j.as_object_mut() {
                    o.insert("creates_bodies".into(), n.into());
                }
            }
            feats.push(j);
        }
        tag_feats(&mut feats, nbefore, comp.map(|c| (c, comp_name.as_deref())), extra.contains(&fid));
    }
    let comp_json: Vec<Value> = comps.iter().map(|(c, n, b)| json!({"name": n, "source_id": c, "bodies": b})).collect();
    let with_bodies = comps.iter().filter(|c| !c.2.is_empty()).count();
    let distinct: HashSet<Option<&str>> = feats.iter().map(|f| f.get("component").and_then(Value::as_str)).collect();
    json!({
        "format": "gusset-feature-tree", "version": 2, "source": {"app": "fusion"},
        "features": feats, "skipped": skipped, "components": comp_json,
        "single_component": distinct.len() <= 1 && with_bodies <= 1,
        "component_records": d.ids.iter().filter(|i| d.is(**i, "E03784ED")).count(),
    })
}

/// Shell `E8063467`: the faces it removes are `7ACC2A03` face recipes under its selection records
/// (a cap or side of an extrude, resolved like the edges of a fillet); the wall is the
/// `innerThickness` parameter (walls grow inward) or, with no inner value, `outerThickness`.
fn shell_json(cx: &mut Ctx, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
    let d = cx.d;
    let fail = |why: &str| {
        json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "error": why})
    };
    let inner = feature::param_values(d, fid, &["innerThickness"]).first().copied().filter(|v| *v > 0.0);
    let outer = feature::param_values(d, fid, &["outerThickness"]).first().copied().filter(|v| *v > 0.0);
    let (side, t) = match (inner, outer) {
        (Some(t), None) => ("inside", t),
        (None, Some(t)) => ("outside", t),
        _ => return fail("the shell has no single wall thickness (inner and outer, or none)"),
    };
    let mut faces: Vec<Value> = Vec::new();
    for rid in cx.input_recipes(fid) {
        let (kind, names) = naming::recipe(d, rid);
        // A bounded-face recipe names the face first and the faces that bound it after.
        let (Some(naming::RecipeKind::Face | naming::RecipeKind::BoundedFace), Some(names)) = (kind, names) else {
            return fail("a removed face is not a face recipe");
        };
        let Some((g, ops)) = names.first().and_then(|f| f.first()) else {
            return fail("a removed face has no name");
        };
        match cx.face(g, ops) {
            // No normal: the cap and side normals the names give are not oriented outward.
            Some(naming::FaceG::Plane { p, .. }) => faces.push(json!({"point": v3mm(p)})),
            _ => return fail("a removed face is not a cap or planar side of an extrude"),
        }
    }
    if faces.is_empty() {
        return fail("no removed face was found");
    }
    json!({"type": "shell", "name": name, "source_id": fid, "faces": faces, "thickness": mm(t), "side": side})
}

/// Component Insert `428B5C6E`: the placement of the occurrence it inserts and the keys that name
/// the inserted design. The record holds a reference to the occurrence (`2D6E13A1`), then the bytes
/// `00 01` and a byte that is `00` when the occurrence's placement follows, as 12 doubles (a
/// row-major 3 x 4 [R | t], centimetres), and `01` when there is none (the origin). The
/// occurrence's own `CE2913AA` record repeats the 12 doubles and holds the design's keys as wide
/// strings that start with a UUID (in one sample the key goes on with a `_urn:` text). The two
/// copies of the matrix must agree byte for byte.
fn insert_json(d: &Design, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
    let fail = |why: &str| {
        json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "error": why})
    };
    let rec = d.rec(fid);
    let found = stream::refs_at(d, rec, 0).into_iter().find_map(|(at, occ)| {
        let (_, end) = stream::ref_end(d, rec, at)?;
        (d.is(occ, "2D6E13A1")
            && rec.get(end) == Some(&0)
            && rec.get(end + 1) == Some(&1)
            && matches!(rec.get(end + 2), Some(0 | 1)))
        .then_some((occ, end))
    });
    let Some((occ, end)) = found else { return fail("the occurrence it inserts is not recognised") };
    let Some(cx) = ref_ids(d, d.rec(occ)).into_iter().find(|q| d.is(*q, "CE2913AA")) else {
        return fail("the occurrence has no transform record");
    };
    let crec = d.rec(cx);
    let mut keys: Vec<String> = Vec::new();
    for (_, _, w) in stream::wstrings_all(crec) {
        if let Some(head) = w.get(..36)
            && stream::is_uuid_text(head)
            && !head.starts_with("00000000-0000-0000-0000")
        {
            let k = w.to_ascii_lowercase();
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    let (r, t) = if rec.get(end + 2) == Some(&0) {
        let Some(m) = stream::dbl::<12>(rec, end + 3) else { return fail("its placement is not readable") };
        // The occurrence repeats the same bytes.
        let raw = rec.get(end + 3..end + 3 + 96).unwrap_or(&[]);
        if raw.len() != 96 || !crec.windows(96).any(|w| w == raw) {
            return fail("its placement differs from the occurrence's");
        }
        let (r0, r1, r2) = ([m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]);
        // A placement is a rotation: orthonormal rows.
        let ok = |a: V3, b: V3| (dot(a, b)).abs() < 1e-6;
        if !(ok(r0, r1) && ok(r0, r2) && ok(r1, r2)) || [r0, r1, r2].iter().any(|r| (dot(*r, *r) - 1.0).abs() > 1e-6) {
            return fail("its placement is not a rotation");
        }
        ([r0, r1, r2], [m[3] * 10.0, m[7] * 10.0, m[11] * 10.0])
    } else {
        ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3])
    };
    let rr = |v: V3| json!([round9(v[0]), round9(v[1]), round9(v[2])]);
    json!({"type": "component_insert", "fusion_type": fusion_type, "name": name, "source_id": fid,
        "insert_keys": keys,
        "placement": {"r": [rr(r[0]), rr(r[1]), rr(r[2])], "t": [round6(t[0]), round6(t[1]), round6(t[2])]}})
}

/// The bodies a Base Feature makes: per visible (DM) body, the history blob its state list names
/// (`BREP.<id>.smbh`), the body's place in that blob and the number of bodies the blob holds
/// (the DM bodies are its last ones, section 13). Empty for a base feature that makes no body.
fn base_feature_bodies(d: &Design, g: &Graph, fid: u64) -> Result<Vec<(String, usize, usize)>, String> {
    let mut out = Vec::new();
    for link in ref_ids(d, d.rec(fid)) {
        if !d.is(link, "D26351F0") || g.creator.get(&link) != Some(&fid) {
            continue;
        }
        let Some(dm) = ref_ids(d, d.rec(link)).into_iter().find(|q| d.is(*q, "D3937028")) else {
            return Err("a Base Feature body link names no body".into());
        };
        // The state list of the body that names a history blob.
        let mut found: Option<(String, Vec<u64>)> = None;
        for list in ref_ids(d, d.rec(dm)).into_iter().filter(|q| d.is(*q, "CD57BC48")) {
            let members = ref_ids(d, d.rec(list));
            let name = members.iter().find_map(|q| {
                stream::wstrings_all(d.rec(*q))
                    .into_iter()
                    .map(|w| w.2)
                    .find(|w| w.starts_with("BREP.") && w.ends_with(".smbh"))
            });
            if let Some(name) = name {
                // The record lists its bodies twice (two reference lists).
                let mut bodies: Vec<u64> = Vec::new();
                for q in members.into_iter().filter(|q| d.is(*q, "D3937028")) {
                    if !bodies.contains(&q) {
                        bodies.push(q);
                    }
                }
                found = Some((name, bodies));
                break;
            }
        }
        let Some((name, members)) = found else { return Err("a Base Feature body names no history blob".into()) };
        let Some(at) = members.iter().position(|q| *q == dm) else {
            return Err("a Base Feature body is not in its own state list".into());
        };
        // The visible bodies are the last of the list (section 13).
        out.push((name, at, members.len()));
    }
    for (_, at, count) in &out {
        if *at + out.len() < *count {
            return Err("a Base Feature body is not among the last bodies of its state list".into());
        }
    }
    Ok(out)
}

fn tag_feats(feats: &mut [Value], from: usize, comp: Option<(u64, Option<&str>)>, off_timeline: bool) {
    for f in feats.iter_mut().skip(from) {
        if let Some(o) = f.as_object_mut() {
            if let Some((id, name)) = comp {
                if let Some(n) = name {
                    o.insert("component".into(), n.into());
                }
                o.insert("_component".into(), id.into());
            }
            if off_timeline {
                o.insert("in_timeline".into(), false.into());
            }
        }
    }
}

/// Why a feature cannot be rebuilt from what was read (the first reason), if it cannot.
fn blocker(f: &Value) -> Option<String> {
    let t = f.get("type").and_then(Value::as_str).unwrap_or("?");
    let has = |k: &str| f.get(k).is_some_and(|v| !v.is_null());
    let text = |k: &str| f.get(k).and_then(Value::as_str);
    match t {
        "sketch" => (!has("plane") || has("error")).then(|| "the sketch has no readable placement".to_string()),
        "extrude" => {
            if has("error") {
                return Some("its layout is not recognised".into());
            }
            if !f.get("profiles").is_some_and(Value::is_array) {
                return Some(
                    text("profile_note")
                        .map_or_else(|| "its profile regions are not decoded".to_string(), str::to_string),
                );
            }
            if !has("distance") {
                return Some("its extent is not a distance (to object / through all)".into());
            }
            if f.get("two_sided").and_then(Value::as_bool) == Some(true) {
                return Some("it extrudes to both sides by different distances".into());
            }
            if has("taper_deg") {
                return Some("it is tapered".into());
            }
            let op = text("operation").unwrap_or("");
            if !matches!(op, "new" | "join" | "cut" | "intersect") {
                return Some(format!("its operation is {op}"));
            }
            if matches!(op, "join" | "cut" | "intersect") && !f.get("target").is_some_and(Value::is_string) {
                return Some("the body it changes is not one body of one feature".into());
            }
            None
        }
        "fillet" | "chamfer" => {
            let empty = f.get("edges").and_then(Value::as_array).is_none_or(Vec::is_empty);
            if empty || has("edges_unresolved") {
                return Some(format!(
                    "{t} edges are not all resolved ({} unresolved)",
                    f.get("edges_unresolved").and_then(Value::as_u64).unwrap_or(0)
                ));
            }
            let key = if t == "fillet" { "radius" } else { "distance" };
            (!has(key)).then(|| format!("the {t} has no {key}"))
        }
        "sweep" => {
            if has("error") {
                return Some(text("error").unwrap_or("its layout is not recognised").to_string());
            }
            if !has("sketch") || !has("path") {
                return Some("its profile or path sketch is not decoded".into());
            }
            let op = text("operation").unwrap_or("");
            if !matches!(op, "new" | "join" | "cut" | "intersect") {
                return Some(format!("its operation is {op}"));
            }
            if matches!(op, "join" | "cut" | "intersect") && !f.get("target").is_some_and(Value::is_string) {
                return Some("the body it changes is not one body of one feature".into());
            }
            None
        }
        "hole" | "mirror" | "shell" | "draft" | "move_body" | "scale_body" | "remove_body" => None,
        "component_insert" => Some("the design it inserts is not merged into the tree".into()),
        "stored_body" => (!has("brep")).then(|| "the stored body names no blob".to_string()),
        "pattern_linear" => (!has("direction") || !has("features")).then(|| "the pattern has no direction".to_string()),
        "pattern_circular" => (!has("axis") || !has("count")).then(|| "the circular pattern has no axis".to_string()),
        _ => Some(text("error").or_else(|| text("note")).map(str::to_string).unwrap_or_else(|| {
            format!("{} is not supported", text("fusion_type").or_else(|| text("gtree_type")).unwrap_or(t))
        })),
    }
}

/// `Ok` when every feature was read in full; else the first feature that was not, by name.
pub(super) fn check(tree: &Value) -> Result<(), String> {
    let feats = tree.get("features").and_then(Value::as_array).ok_or("the design has no features")?;
    if feats.is_empty() {
        return Err("the design has no feature timeline".into());
    }
    if !feats.iter().any(|f| matches!(f.get("type").and_then(Value::as_str), Some("extrude"))) {
        return Err("the design has no extrude".into());
    }
    for f in feats {
        if let Some(why) = blocker(f) {
            let name = f.get("name").and_then(Value::as_str).unwrap_or("?");
            let ty = f
                .get("fusion_type")
                .and_then(Value::as_str)
                .or_else(|| f.get("type").and_then(Value::as_str))
                .unwrap_or("?");
            return Err(format!("the feature {name} ({ty}) cannot be read: {why}"));
        }
    }
    Ok(())
}

/// Drops the reader's own keys (`_component`).
pub(super) fn finish(tree: &mut Value) {
    if let Some(feats) = tree.get_mut("features").and_then(Value::as_array_mut) {
        // The stored bodies are numbered in tree order: the importer hands them over in that order.
        let mut stored = 0u64;
        for f in feats {
            if let Some(o) = f.as_object_mut() {
                o.retain(|k, _| !k.starts_with('_'));
                if o.get("type").and_then(Value::as_str) == Some("stored_body") {
                    o.insert("body".into(), stored.into());
                    stored += 1;
                }
            }
        }
    }
}

fn xf_point(pl: &crate::fusion::Placement, p: V3) -> V3 {
    let row = |i: usize| pl.r.get(i).copied().unwrap_or([0.0; 3]);
    let t = |i: usize| pl.t.get(i).copied().unwrap_or(0.0);
    [dot(row(0), p) + t(0), dot(row(1), p) + t(1), dot(row(2), p) + t(2)]
}

fn xf_dir(pl: &crate::fusion::Placement, p: V3) -> V3 {
    let row = |i: usize| pl.r.get(i).copied().unwrap_or([0.0; 3]);
    [dot(row(0), p), dot(row(1), p), dot(row(2), p)]
}

fn get3(v: Option<&Value>) -> Option<V3> {
    let a = v?.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}

/// Puts the features of each component where the occurrence of the component stands: every
/// point (sketch planes, edge midpoints, hole positions, mirror planes) moves with it and every
/// direction turns with it. A component whose bodies have different placements is refused.
pub(super) fn place(tree: &mut Value, placed: &[crate::fusion::Placed]) -> Result<(), String> {
    let comps: Vec<(u64, Vec<String>)> = tree
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c.get("source_id")?.as_u64()?;
            let bodies = c.get("bodies")?.as_array()?.iter().filter_map(|b| b.as_str().map(str::to_string)).collect();
            Some((id, bodies))
        })
        .collect();
    // The placements of each component's occurrences; a component with several occurrences (one
    // blob, several placements) has its features written once per occurrence.
    let mut by_comp: HashMap<u64, Vec<crate::fusion::Placement>> = HashMap::new();
    for (id, bodies) in &comps {
        let mut found: Vec<crate::fusion::Placement> = Vec::new();
        let mut breps: Vec<&str> = Vec::new();
        for p in placed {
            if bodies
                .iter()
                .any(|b| b.strip_prefix("BREP.").and_then(|b| b.strip_suffix(".smb")) == Some(p.brep.as_str()))
            {
                if !found.contains(&p.placement) {
                    found.push(p.placement);
                }
                if !breps.contains(&p.brep.as_str()) {
                    breps.push(p.brep.as_str());
                }
            }
        }
        if found.len() > 1 && breps.len() > 1 {
            return Err("a component's bodies are placed in different ways".into());
        }
        if found.len() > 1 || found.first().is_some_and(|f| *f != crate::fusion::Placement::IDENTITY) {
            by_comp.insert(*id, found);
        }
    }
    if let Some(feats) = tree.get_mut("features").and_then(Value::as_array_mut) {
        replicate_instances(feats, &by_comp);
    }
    let Some(feats) = tree.get_mut("features").and_then(Value::as_array_mut) else { return Ok(()) };
    for f in feats {
        let inst = f.get("_instance").and_then(Value::as_u64).unwrap_or(0) as usize;
        let Some(pl) = f
            .get("_component")
            .and_then(Value::as_u64)
            .and_then(|c| by_comp.get(&c))
            .and_then(|v| v.get(inst))
            .copied()
        else {
            continue;
        };
        move_feature(f, &pl);
    }
    Ok(())
}

/// A `{"r": [[..]x3], "t": [..]}` placement.
pub(super) fn placement_of(v: &Value) -> Option<crate::fusion::Placement> {
    let mut r = [[0.0; 3]; 3];
    let mut t = [0.0; 3];
    for (i, row) in v.get("r")?.as_array()?.iter().enumerate().take(3) {
        for (j, x) in row.as_array()?.iter().enumerate().take(3) {
            *r.get_mut(i)?.get_mut(j)? = x.as_f64()?;
        }
    }
    for (i, x) in v.get("t")?.as_array()?.iter().enumerate().take(3) {
        *t.get_mut(i)? = x.as_f64()?;
    }
    Some(crate::fusion::Placement { r, t })
}

/// Moves one feature by a placement: every point turns and shifts, every direction turns.
pub(super) fn move_feature(f: &mut Value, pl: &crate::fusion::Placement) {
    let pl = *pl;
    let ty = f.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let pt = |v: Option<&Value>| get3(v).map(|p| v3mm_raw(xf_point(&pl, p)));
    let dir = |v: Option<&Value>| get3(v).map(|p| v3_9(xf_dir(&pl, p)));
    match ty.as_str() {
        "sketch" => {
            if let Some(p) = f.get_mut("plane").and_then(Value::as_object_mut) {
                let (o, x, y) = (pt(p.get("origin")), dir(p.get("x_axis")), dir(p.get("y_axis")));
                for (k, v) in [("origin", o), ("x_axis", x), ("y_axis", y)] {
                    if let Some(v) = v {
                        p.insert(k.into(), v);
                    }
                }
            }
        }
        "fillet" | "chamfer" => {
            if let Some(es) = f.get_mut("edges").and_then(Value::as_array_mut) {
                for e in es {
                    if let Some(m) = pt(e.get("midpoint"))
                        && let Some(o) = e.as_object_mut()
                    {
                        o.insert("midpoint".into(), m);
                    }
                }
            }
        }
        "hole" => {
            if let Some(fc) = f.get_mut("face").and_then(Value::as_object_mut) {
                let (p, n) = (pt(fc.get("point")), dir(fc.get("normal")));
                for (k, v) in [("point", p), ("normal", n)] {
                    if let Some(v) = v {
                        fc.insert(k.into(), v);
                    }
                }
            }
            if let Some(ps) = f.get_mut("positions").and_then(Value::as_array_mut) {
                for p in ps {
                    if let Some(m) = pt(Some(&*p)) {
                        *p = m;
                    }
                }
            }
        }
        "mirror" => {
            if let Some(fc) = f.get_mut("plane").and_then(Value::as_object_mut) {
                let (p, n) = (pt(fc.get("origin")), dir(fc.get("normal")));
                for (k, v) in [("origin", p), ("normal", n)] {
                    if let Some(v) = v {
                        fc.insert(k.into(), v);
                    }
                }
            }
        }
        "stored_body" => {
            // A body that already stands somewhere (the placement of its component) moves on.
            let before = f.get("placement").and_then(placement_of);
            let pl = before.map_or(pl, |old| pl.then(&old));
            if let Some(o) = f.as_object_mut() {
                o.insert("placement".into(), json!({"r": pl.r, "t": pl.t}));
            }
        }
        "move_body" => {
            // A translation follows the turn of the placement; its own rotation is never read.
            if let Some(t) = get3(f.get("translate")).map(|p| v3mm_raw(xf_dir(&pl, p)))
                && let Some(o) = f.as_object_mut()
            {
                o.insert("translate".into(), t);
            }
        }
        "draft" => {
            if let Some(fs) = f.get_mut("faces").and_then(Value::as_array_mut) {
                for fc in fs.iter_mut().filter_map(Value::as_object_mut) {
                    let (p, n) = (pt(fc.get("point")), dir(fc.get("normal")));
                    for (k, v) in [("point", p), ("normal", n)] {
                        if let Some(v) = v {
                            fc.insert(k.into(), v);
                        }
                    }
                }
            }
            if let Some(pl) = f.get_mut("plane").and_then(Value::as_object_mut) {
                let (p, n) = (pt(pl.get("origin")), dir(pl.get("normal")));
                for (k, v) in [("origin", p), ("normal", n)] {
                    if let Some(v) = v {
                        pl.insert(k.into(), v);
                    }
                }
            }
        }
        "scale_body" => {
            if let Some(c) = pt(f.get("center"))
                && let Some(o) = f.as_object_mut()
            {
                o.insert("center".into(), c);
            }
        }
        "pattern_circular" => {
            if let Some(a) = f.get_mut("axis").and_then(Value::as_object_mut) {
                let (o, d) = (pt(a.get("origin")), dir(a.get("direction")));
                for (k, v) in [("origin", o), ("direction", d)] {
                    if let Some(v) = v {
                        a.insert(k.into(), v);
                    }
                }
            }
        }
        "pattern_linear" => {
            if let Some(d) = dir(f.get("direction"))
                && let Some(o) = f.as_object_mut()
            {
                o.insert("direction".into(), d);
            }
        }
        _ => {}
    }
}

/// Replaces every string equal to a key of `map` (except under the keys that hold kinds and names).
pub(super) fn rename(v: &mut Value, map: &HashMap<String, String>) {
    match v {
        Value::String(s) => {
            if let Some(n) = map.get(s.as_str()) {
                *s = n.clone();
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| rename(x, map)),
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                if !matches!(k.as_str(), "type" | "operation" | "component" | "fusion_type" | "name") {
                    rename(x, map);
                }
            }
        }
        _ => {}
    }
}

/// Writes the features of a component that stands in `k` places `k` times (instance `i` carries
/// `_instance: i`; copies after the first get " [i+1]" after their names and every reference to a
/// renamed feature follows), each copy right after the original so the timeline order holds.
fn replicate_instances(feats: &mut Vec<Value>, by_comp: &HashMap<u64, Vec<crate::fusion::Placement>>) {
    let count =
        |f: &Value| f.get("_component").and_then(Value::as_u64).and_then(|c| by_comp.get(&c)).map_or(1, Vec::len);
    if feats.iter().all(|f| count(f) <= 1) {
        return;
    }
    let old = std::mem::take(feats);
    // Per component, the names its features carry.
    let names: Vec<Option<String>> =
        old.iter().map(|f| f.get("name").and_then(Value::as_str).map(str::to_string)).collect();
    let mut out = Vec::with_capacity(old.len());
    for (i, f) in old.iter().enumerate() {
        let k = count(f);
        for inst in 0..k {
            let mut c = f.clone();
            if k > 1 {
                let map: HashMap<String, String> = old
                    .iter()
                    .zip(&names)
                    .filter(|(g, _)| g.get("_component") == f.get("_component"))
                    .filter_map(|(_, n)| n.clone())
                    .map(|n| (n.clone(), format!("{n} [{}]", inst + 1)))
                    .collect();
                if inst > 0 {
                    rename(&mut c, &map);
                    if let (Some(n), Some(o)) = (names.get(i).and_then(Clone::clone), c.as_object_mut()) {
                        o.insert("name".into(), format!("{n} [{}]", inst + 1).into());
                    }
                }
                if let Some(o) = c.as_object_mut() {
                    o.insert("_instance".into(), inst.into());
                }
            }
            out.push(c);
        }
    }
    *feats = out;
}

fn v3mm_raw(p: V3) -> Value {
    json!([round6(p[0]), round6(p[1]), round6(p[2])])
}

fn v3_9(p: V3) -> Value {
    json!([round9(p[0]), round9(p[1]), round9(p[2])])
}
