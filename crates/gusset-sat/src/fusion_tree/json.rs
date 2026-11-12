//! The feature tree JSON of a design: one entry per timeline feature, as the prototype wrote
//! it, then the check that every feature was read in full.

use super::feature::{self, Params, extrude_raw, feature_name, params_of, pget, point3};
use super::geom::{self, V3, dot, mm, round6, round9};
use super::naming::{self, RecipeKind};
use super::sketch::{self, SKETCH_FEAT};
use super::stream::ref_ids;
use super::stream::{self, Design};
use super::{Ctx, EXTRUDE, Graph, PARAM, SWEEP, component_bodies, component_name, mirrorcomp, timeline};
use crate::cover;
use crate::fusion::Placement;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

/// Timeline entries that create no body geometry (none is the creator of a body link or
/// references a body record in the samples).
const NON_GEOMETRIC: [(&str, &str); 13] = [
    ("78B9DEFC", "CreateComponent"),
    ("54F9ACE8", "component activation"),
    // A class derived from the component activation (its parent in the class table): it names
    // an occurrence and its CreateComponent, and no body or body link names it.
    ("8DBAF917", "component activation (of an occurrence)"),
    ("02E8AA49", "JointOrigin"),
    ("8DFDD543", "Assemble (joint)"),
    ("AA8B3D40", "As-built joint"),
    ("8EE00B00", "Position (assembly)"),
    ("0EE4A83E", "Canvas (image)"),
    ("1339A425", "Decal"),
    ("D869265F", "WorkPlane (construction)"),
    ("B0EEF272", "PCB link (bodies come from the extrudes that follow)"),
    ("A917FE80", "timeline group (a folder: its members follow it in the timeline)"),
    (super::CUSTOM_FEATURE, "custom feature (package generator): the features it made follow it in the timeline"),
];

/// Derived Base Feature: the bodies of another design, derived into this one (section 30).
const DERIVED_BASE: &str = "B1E5B971";

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

/// Sketch points that lie apart by less than 1e-6 cm (0.01 µm) but not on one another, each
/// mapped to the first of its group (by id): the ends of lines and arcs that Fusion's profiles
/// join there (4300's board outline: an arc's end 2.8e-6 mm from the line it runs on into,
/// where the model's sketch merges ends only within 1e-6 mm). Points on one another stay apart.
fn near_points(points: &std::collections::BTreeMap<u64, V3>) -> HashMap<u64, u64> {
    const NEAR: f64 = 1e-6;
    let mut by_x: Vec<(u64, V3)> = points.iter().map(|(q, p)| (*q, *p)).collect();
    by_x.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]).then(a.0.cmp(&b.0)));
    let mut out: HashMap<u64, u64> = HashMap::new();
    for (i, (qa, pa)) in by_x.iter().enumerate() {
        for (qb, pb) in by_x.iter().skip(i + 1) {
            if pb[0] - pa[0] > NEAR {
                break;
            }
            let dd = libm::hypot(pb[0] - pa[0], pb[1] - pa[1]);
            if dd > 0.0 && dd < NEAR {
                let (lo, hi) = if qa < qb { (*qa, *qb) } else { (*qb, *qa) };
                let root = out.get(&lo).copied().unwrap_or(lo);
                out.entry(hi).or_insert(root);
            }
        }
    }
    out
}

/// A line and an arc that run past each other's ends by a hair where they meet: the line's end
/// lies on the arc's circle and the arc's end on the line, the two ends less than 1e-3 cm
/// (10 µm) apart. Fusion cuts each at the other's end (4300's board outline: 1.9 µm, the profile
/// lists both pieces of each); the model's sketch merges ends only within 1e-6 mm and is left
/// with a gap. The line's end is mapped onto the arc's (it moves along the line).
fn crossing_ends(s: &sketch::Sketch, same: &mut HashMap<u64, u64>) {
    const NEAR: f64 = 1e-3;
    const ON: f64 = 1e-6;
    let at = |q: u64| s.points.get(&q).map(|p| [p[0], p[1]]);
    for l in s.lines.values() {
        let (Some(a), Some(b)) = (l.start_ref, l.end_ref) else { continue };
        let (Some(pa), Some(pb)) = (at(a), at(b)) else { continue };
        for arc in s.arcs.values() {
            let c = [arc.center[0], arc.center[1]];
            for &f in arc.refs.get(1..3).unwrap_or(&[]) {
                let Some(pf) = at(f) else { continue };
                for (e, pe) in [(a, pa), (b, pb)] {
                    let dd = geom::dist(pe, pf);
                    if dd <= 1e-9 || dd >= NEAR || same.contains_key(&e) {
                        continue;
                    }
                    let on_circle = (geom::dist(pe, c) - arc.radius).abs() < ON;
                    let on_line = geom::seg_dist(pf, pa, pb) < ON;
                    if on_circle && on_line {
                        same.insert(e, same.get(&f).copied().unwrap_or(f));
                    }
                }
            }
        }
    }
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
    let mut points: Vec<Value> =
        s.points.iter().map(|(q, p)| json!({"id": format!("p{q}"), "x": mm(p[0]), "y": mm(p[1])})).collect();
    let mut same = near_points(&s.points);
    crossing_ends(&s, &mut same);
    let pid = |q: u64| format!("p{}", same.get(&q).copied().unwrap_or(q));
    let mut curves = Vec::new();
    // A spline is its stored B-spline: the end control points are the curve's sketch points, the
    // inner ones points of their own; knots and weights as stored.
    for (q, sp) in &s.splines {
        let last = sp.poles.len().saturating_sub(1);
        let mut poles = Vec::with_capacity(sp.poles.len());
        for (k, p) in sp.poles.iter().enumerate() {
            if k == 0 {
                poles.push(format!("p{}", sp.start_ref));
            } else if k == last {
                poles.push(format!("p{}", sp.end_ref));
            } else {
                let id = format!("c{q}k{k}");
                points.push(json!({"id": id, "x": mm(p[0]), "y": mm(p[1])}));
                poles.push(id);
            }
        }
        let mut c = json!({"id": format!("c{q}"), "type": "bspline", "degree": sp.degree, "knots": sp.knots,
            "poles": poles, "periodic": false});
        if !sp.weights.is_empty()
            && let Some(o) = c.as_object_mut()
        {
            o.insert("weights".into(), json!(sp.weights));
        }
        curves.push(c);
    }
    for (q, l) in &s.lines {
        if let (Some(a), Some(b)) = (l.start_ref, l.end_ref)
            && s.points.contains_key(&a)
            && s.points.contains_key(&b)
        {
            // A line shorter than the merge distance keeps its own ends (4128's sketch has one).
            let (sa, sb) = if pid(a) == pid(b) { (format!("p{a}"), format!("p{b}")) } else { (pid(a), pid(b)) };
            curves.push(json!({"id": format!("c{q}"), "type": "line", "start": sa, "end": sb}));
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
            let (ss, se) = if pid(st) == pid(en) { (format!("p{st}"), format!("p{en}")) } else { (pid(st), pid(en)) };
            curves
                .push(json!({"id": format!("c{q}"), "type": "arc", "center": format!("p{c}"), "start": ss, "end": se}));
        }
    }
    j.insert("points".into(), points.into());
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
/// straight edges at offsets along the normal (cm). A cut through all to both sides (`symmetric`)
/// is as long as twice its farther reach: the whole length of a symmetric extrude.
fn through_reach(cx: &mut Ctx, fid: u64, symmetric: bool) -> Option<f64> {
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
    if symmetric {
        return ts.iter().map(|t| 2.0 * t.abs()).reduce(f64::max);
    }
    match (pos.is_empty(), neg.is_empty()) {
        (false, true) => Some(pos.into_iter().fold(f64::NEG_INFINITY, f64::max)),
        (true, false) => Some(neg.into_iter().fold(f64::INFINITY, f64::min)),
        _ => None,
    }
}

/// The signed length (cm, along the sketch normal) of the tool an extrude without a distance
/// swept, read off the tool itself where the file stores it: a stored body that has every
/// straight edge of the profile's outline at the sketch plane and at one other offset, and at no
/// third. Edges two picked regions share are inside the tool and not looked for. Several such
/// bodies must agree.
fn tool_reach(cx: &mut Ctx, fid: u64) -> Option<f64> {
    let g = cx.ext_geom(fid)?;
    let (pl, z) = (g.pl, g.pl.z);
    // The outline of the picked regions together: the straight pieces only one region has.
    let regions = cx.rc.regions(cx.d, fid, &g.sketch).ok()?;
    let near = |a: V3, b: V3| geom::norm(geom::sub(a, b)) < 1e-6;
    let same = |x: &(V3, V3), y: &(V3, V3)| (near(x.0, y.0) && near(x.1, y.1)) || (near(x.0, y.1) && near(x.1, y.0));
    let mut pieces: Vec<Vec<(V3, V3)>> = Vec::new();
    for r in regions.iter() {
        let mut own: Vec<(V3, V3)> = Vec::new();
        for sg in &r.segments {
            if !g.sketch.lines.contains_key(&sg.curve) {
                continue;
            }
            let (a, b) = (*sg.poly.first()?, *sg.poly.last()?);
            let l = (geom::world(&pl, a, 0.0), geom::world(&pl, b, 0.0));
            if !own.iter().any(|m| same(&l, m)) {
                own.push(l);
            }
        }
        pieces.push(own);
    }
    let outline: Vec<(V3, V3)> = pieces
        .iter()
        .flatten()
        .filter(|l| pieces.iter().filter(|own| own.iter().any(|m| same(l, m))).count() == 1)
        .copied()
        .collect();
    reach_of(&outline, z, cx.edge_sets())
}

/// The signed length along `z` of the tool whose outline (straight pieces, on the sketch plane)
/// is `outline`: a body of `bodies` that has every piece at the plane and at exactly one other
/// offset. Several such bodies must agree; an outline of one direction decides nothing.
pub(super) fn reach_of(outline: &[(V3, V3)], z: V3, bodies: &[Vec<super::acis::Edge>]) -> Option<f64> {
    // Two directions at least, or a slab of any outline would do.
    let dir = |l: &(V3, V3)| geom::unit(geom::sub(l.1, l.0));
    let near = |a: V3, b: V3| geom::norm(geom::sub(a, b)) < 1e-6;
    let same = |x: &(V3, V3), y: &(V3, V3)| (near(x.0, y.0) && near(x.1, y.1)) || (near(x.0, y.1) && near(x.1, y.0));
    let first = outline.first().and_then(dir)?;
    if !outline.iter().filter_map(dir).any(|u| dot(u, first).abs() < 1.0 - 1e-9) {
        return None;
    }
    let mut found: Option<f64> = None;
    for body in bodies {
        let mut reach: Option<f64> = None;
        let mut ok = true;
        'lines: for l in outline {
            let mut at: Vec<f64> = Vec::new();
            for (p, q) in body {
                let t = dot(geom::sub(*p, l.0), z);
                let moved = (geom::add(l.0, geom::mul(z, t)), geom::add(l.1, geom::mul(z, t)));
                if same(&moved, &(*p, *q)) && !at.iter().any(|x| (x - t).abs() < 1e-7) {
                    at.push(t);
                }
            }
            let far: Vec<f64> = at.iter().copied().filter(|t| t.abs() > 1e-7).collect();
            match (far.as_slice(), at.len()) {
                ([t], 2) if reach.is_none_or(|r| (r - t).abs() < 1e-7) => reach = Some(*t),
                _ => {
                    ok = false;
                    break 'lines;
                }
            }
        }
        if let (true, Some(t)) = (ok, reach) {
            if found.is_some_and(|f| (f - t).abs() > 1e-7) {
                return None;
            }
            found = Some(t);
        }
    }
    found.map(|t| (t * 1e7).round() / 1e7)
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
                    rg.iter()
                        .flat_map(|r| std::iter::once(&r.point).chain(r.more_points.iter()))
                        .map(|p| json!({"point": [mm(p[0]), mm(p[1])]}))
                        .collect::<Vec<_>>()
                        .into(),
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
    // The distances count along the sketch normal, signed. The stored direction vector does not
    // tell the side: 3350's four extrudes whose vector runs against their sketch's normal go the
    // way their distance's sign says (Extrude1, -14 mm: the stored rod is z -14..0; Extrude3,
    // 3 mm from a sketch facing down: z -17..-14), and no other extrude of the corpus has one.
    // A vector in the sketch plane is given in the frame of the assembly the component stands in
    // (4800: a component's standoffs and another's screen, drawn in place), and the placement of
    // the component's occurrence tells the side ([`place`]).
    let in_plane = match (&plane, e.vec) {
        (Some(pl), Some(v)) => dot(pl.z, v).abs() < 1e-6,
        _ => false,
    };
    if let (true, Some(pl), Some(v)) = (in_plane, &plane, e.vec) {
        j.insert("_assembly_direction".into(), json!({"normal": pl.z, "direction": v}));
    }
    // Where it starts, off the sketch plane (cm along its normal).
    let start = match &plane {
        Some(pl) => cx.extrude_start(fid, &e, pl),
        None => Ok(0.0),
    };
    let start = match start {
        Ok(t) => t,
        Err(why) => {
            j.insert("start_error".into(), why.into());
            0.0
        }
    };
    let mut distance = along.and_then(|a| a.value).map(mm);
    if let Some(a) = along {
        j.insert("distance_expr".into(), a.expr.as_str().into());
    }
    j.insert("symmetric".into(), (e.extent == 3).into());
    // A symmetric distance is the whole length here: Fusion's may be each side's.
    match (e.extent, e.half_length) {
        (3, Some(true)) => {
            distance = distance.map(|t| 2.0 * t);
            j.insert("symmetric_measure".into(), "each side's distance, doubled".into());
        }
        // Through all to both sides (1264's Extrude2 and Extrude3, no distance): the record
        // has no measure, and none is needed.
        (3, None) if along.is_some() => {
            j.insert(
                "error".into(),
                "whether its symmetric distance is the whole length or each side's is not read".into(),
            );
        }
        _ => {}
    }
    if let (2, Some(ag)) = (e.extent, against) {
        j.insert("two_sided".into(), true.into());
        j.insert("distance2".into(), ag.value.map_or(Value::Null, |v| (-mm(v)).into()));
        j.insert("distance2_expr".into(), ag.expr.as_str().into());
    }
    if distance.is_none() && pget(ps, "Side1Offset").is_some() {
        // To-object: the first face of the extrude's bounded-face or face recipe, when it is a
        // plane parallel to the sketch.
        let off = pget(ps, "Side1Offset").and_then(|p| p.value).unwrap_or(0.0);
        // From a face to a face: the first face is the start ([`Ctx::extrude_start`]).
        let n_faces = cx
            .edge_recipes_of(fid)
            .into_iter()
            .filter(|r| matches!(naming::recipe(d, *r).0, Some(RecipeKind::BoundedFace | RecipeKind::Face)))
            .count();
        let from_face = pget(ps, "ProfileOffset").is_some() && n_faces == 2 && !j.contains_key("start_error");
        let mut skip = usize::from(from_face);
        for rid in cx.edge_recipes_of(fid) {
            let (kind, faces) = naming::recipe(d, rid);
            let (Some(RecipeKind::BoundedFace | RecipeKind::Face), Some(faces)) = (kind, faces) else { continue };
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let f = faces.first().and_then(|f| f.first()).and_then(|(g, ops)| cx.face(g, ops));
            if let (Some(naming::FaceG::Plane { n, p, .. }), Some(pl)) = (f, &plane)
                && (dot(n, pl.z).abs() - 1.0).abs() < 1e-9
            {
                let t = dot(n, geom::sub(p, pl.origin)) / dot(n, pl.z);
                distance = Some(mm(t + if t >= 0.0 { off } else { -off } - start));
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
        // To a body, or through all: the length of the stored tool, where the file has it.
        if let Some(t) = tool_reach(cx, fid) {
            distance = Some(mm(t));
            j.insert("extent_from".into(), "the length of the stored tool body".into());
        } else if let Some(t) = through_reach(cx, fid, e.extent == 3).filter(|t| *t != 0.0) {
            // Through all: measured on the stored body.
            distance = Some(mm(t));
            j.insert("extent_from".into(), "through all (reach measured on the stored body)".into());
        }
    }
    j.insert("distance".into(), distance.map_or(Value::Null, Value::from));
    if in_plane && (start != 0.0 || e.extent == 2) {
        j.insert(
            "error".into(),
            "its direction lies in its sketch plane and it does not start there or goes both ways".into(),
        );
    }
    if start != 0.0 {
        j.insert("start".into(), mm(start).into());
    }
    if distance.is_none() {
        j.insert("extent".into(), "not a distance (to object / through all)".into());
    }
    j.insert("operation".into(), op_name(e.op).into());
    j.insert("target".into(), Value::Null);
    if let Some(t) = pget(ps, "TaperAngle").and_then(|p| p.value).filter(|t| *t != 0.0) {
        j.insert("taper_deg".into(), round6(t.to_degrees()).into());
    }
    // The second side of a two-sided extrude has its own taper (section 30); read by
    // `two_sided_from_a_moved_sketch` and not written to the tree.
    if let Some(t) = pget(ps, "Side2TaperAngle").and_then(|p| p.value) {
        j.insert("_taper2_deg".into(), round6(t.to_degrees()).into());
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
    // The bodies it takes part in are the ones its body selections name, where it has any: a body
    // reached only through a face selection is where it goes to, not a body it acts on (2441:
    // Extrude11 joins standoffs to Extrude9's body up to a face of Extrude1's, which stays a body
    // of its own in the file's end bodies).
    let mut selected: Vec<String> = g
        .selected_bodies(d, fid)
        .iter()
        .filter_map(|l| g.creator.get(l).copied())
        .filter(|c| *c != fid)
        .map(|c| names.get(&c).cloned().unwrap_or_else(|| c.to_string()))
        .collect();
    selected.sort();
    selected.dedup();
    if !selected.is_empty() {
        acted.retain(|a| selected.contains(a));
    }
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
/// An extrude whose profile is a face of a body, not a sketch: where the face is the end cap
/// of an earlier extrude (one name, that extrude's only operation) that later joins from the
/// same sketch have built over in part, its profile is the regions of that extrude's sketch
/// that no such join covers, at the cap (610: Extrude8 cuts 9 mm down from the part of
/// Extrude1's top that Extrude2, joined on the other region of Sketch1, leaves open; the
/// bounded face's other faces are those joins' sides). Nothing changes where that is not so.
fn profile_of_an_exposed_cap(cx: &mut Ctx, fid: u64, j: &mut Map<String, Value>, feats: &[Value]) {
    if !j.contains_key("profile_note") || !j.get("sketch").is_some_and(Value::is_null) || j.contains_key("start") {
        return;
    }
    let d = cx.d;
    let faces: Vec<Vec<naming::FaceName>> = cx
        .edge_recipes_of(fid)
        .into_iter()
        .filter_map(|r| match naming::recipe(d, r) {
            (Some(RecipeKind::BoundedFace | RecipeKind::Face), Some(names)) => Some(names),
            _ => None,
        })
        .collect();
    let [names] = faces.as_slice() else { return };
    let Some([(g, ops)]) = names.first().map(Vec::as_slice) else { return };
    let ([op], "1" | "2") = (ops.as_slice(), g.as_str()) else { return };
    let Some(x) = u64::try_from(*op).ok().and_then(|o| cx.opmap.get(&o).copied()) else { return };
    let Some(at) = feats.iter().position(|f| f.get("source_id").and_then(Value::as_u64) == Some(x)) else { return };
    let Some(xf) = feats.get(at) else { return };
    let plain = |f: &Value| {
        f.get("type").and_then(Value::as_str) == Some("extrude")
            && ["start", "extent_from", "two_sided", "taper_deg", "error"].iter().all(|k| f.get(*k).is_none())
            && f.get("symmetric").and_then(Value::as_bool) != Some(true)
    };
    let (Some(sid), Some(dx), Some(seeds)) = (
        xf.get("sketch_source_id").and_then(Value::as_u64),
        xf.get("distance").and_then(Value::as_f64),
        xf.get("profiles").and_then(Value::as_array),
    ) else {
        return;
    };
    if !plain(xf) || xf.get("name").and_then(Value::as_str).is_none() {
        return;
    }
    let point = |v: &Value| -> Option<[f64; 2]> {
        let p = v.get("point")?.as_array()?;
        Some([p.first()?.as_f64()?, p.get(1)?.as_f64()?])
    };
    let mut open: Vec<[f64; 2]> = seeds.iter().filter_map(point).collect();
    if open.len() != seeds.len() {
        return;
    }
    // The joins from the same sketch that reach past the cap (only the far cap is built over).
    let mut covering: Vec<u64> = Vec::new();
    for f in feats.iter().skip(at + 1) {
        let Some(df) = f.get("distance").and_then(Value::as_f64) else { continue };
        if plain(f)
            && f.get("operation").and_then(Value::as_str) == Some("join")
            && f.get("sketch_source_id").and_then(Value::as_u64) == Some(sid)
            && g == "2"
            && df * dx > 0.0
            && df.abs() > dx.abs()
        {
            let Some(fs) = f.get("profiles").and_then(Value::as_array) else { return };
            for q in fs.iter().filter_map(point) {
                open.retain(|p| (p[0] - q[0]).abs() > 1e-6 || (p[1] - q[1]).abs() > 1e-6);
            }
            covering.extend(f.get("source_id").and_then(Value::as_u64));
        }
    }
    // The faces that bound the face are those joins'.
    let bounded_by_them = names.iter().skip(1).all(|f| {
        f.iter().all(|(_, o)| {
            o.first()
                .and_then(|o| u64::try_from(*o).ok())
                .and_then(|o| cx.opmap.get(&o))
                .is_some_and(|c| covering.contains(c))
        })
    });
    if open.is_empty() || open.len() == seeds.len() || !bounded_by_them {
        return;
    }
    let Some(pl) = cx.sketch(sid).plane else { return };
    let Some(e) = extrude_raw(d, fid) else { return };
    let (Some(v), Some(along)) = (e.vec, pget(&e.params, "AlongDistance").and_then(|p| p.value)) else { return };
    if dot(pl.z, v).abs() < 1e-6 {
        return;
    }
    let t = if g == "2" { dx } else { 0.0 };
    j.remove("profile_note");
    j.insert("sketch".into(), xf.get("sketch").cloned().unwrap_or(Value::Null));
    j.insert("sketch_source_id".into(), sid.into());
    j.insert("profiles".into(), open.iter().map(|p| json!({"point": p})).collect::<Vec<_>>().into());
    j.insert("distance".into(), mm(along).into());
    if t != 0.0 {
        j.insert("start".into(), t.into());
    }
    j.insert(
        "profile_from".into(),
        "the regions of an earlier extrude's sketch left open at its end cap (the face it extrudes)".into(),
    );
}

/// An extrude to both sides by two distances has no form of its own in the tree: it becomes one
/// extrude from a copy of its sketch moved along the normal to where the second side ends,
/// over the length between the two ends (the same solid; the copy is named `<extrude> start`).
/// An extrude that starts off its sketch plane (`start`, mm: an offset plane or a face) is
/// written the same way, from a copy moved to its start.
fn two_sided_from_a_moved_sketch(cx: &mut Ctx, j: &mut Map<String, Value>, name: &str, feats: &mut Vec<Value>) {
    let start = j.get("start").and_then(Value::as_f64).unwrap_or(0.0);
    let two_sided = j.get("two_sided").and_then(Value::as_bool) == Some(true);
    let taper2 = j.remove("_taper2_deg").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let (Some(d1), Some(sid)) =
        (j.get("distance").and_then(Value::as_f64), j.get("sketch_source_id").and_then(Value::as_u64))
    else {
        return;
    };
    // A taper leans each side from the sketch plane, by its own angle (5905's PinSocket: a 45
    // degree lead-in 0.322 mm up, a straight hole 2.558 mm down): one extrude from a moved sketch
    // would lean the whole length. The extrude keeps both reaches and both tapers (section 30).
    let taper1 = j.get("taper_deg").and_then(Value::as_f64).unwrap_or(0.0);
    if two_sided
        && start == 0.0
        && (taper1 != 0.0 || taper2 != 0.0)
        && let Some(d2) = j.get("distance2").and_then(Value::as_f64)
        && d1 * d2 < 0.0
    {
        if taper2 != 0.0 {
            j.insert("taper2_deg".into(), taper2.into());
        }
        j.insert("two_sided".into(), false.into());
        j.insert("extent_from".into(), "both sides, each with its own taper".into());
        j.remove("distance2_expr");
        return;
    }
    // The solid lies between the two reaches (a negative second distance starts it off the
    // sketch plane, on the first side); both move with the start.
    let (d1, d2) = match j.get("distance2").and_then(Value::as_f64) {
        Some(d2) if two_sided => (d1 + start, d2 + start),
        _ if two_sided || start == 0.0 => return,
        _ => (d1 + start, start),
    };
    if (d1 - d2).abs() < 1e-9 {
        return;
    }
    let Some(pl) = cx.sketch(sid).plane else { return };
    let Some(Value::Object(mut c)) = feats
        .iter()
        .find(|f| {
            f.get("type").and_then(Value::as_str) == Some("sketch")
                && f.get("source_id").and_then(Value::as_u64) == Some(sid)
        })
        .cloned()
    else {
        return;
    };
    let o = geom::add(pl.origin, geom::mul(pl.z, d2 / 10.0));
    c.insert("plane".into(), json!({"origin": v3mm(o), "x_axis": pl.x, "y_axis": pl.y}));
    c.insert(
        "plane_note".into(),
        if two_sided {
            "moved along its normal to the far end of an extrude to both sides"
        } else {
            "moved along its normal to where the extrude starts"
        }
        .into(),
    );
    let copy = format!("{name} start");
    c.insert("name".into(), copy.clone().into());
    feats.push(Value::Object(c));
    j.insert("sketch".into(), copy.into());
    j.insert("distance".into(), round6(d1 - d2).into());
    j.insert("two_sided".into(), false.into());
    j.remove("start");
    let how = if two_sided {
        format!("two sides ({d1} and {d2} mm) as one extrude from a moved sketch")
    } else {
        format!("from {d2} mm off the sketch plane, as an extrude from a moved sketch")
    };
    j.insert("extent_from".into(), how.into());
    j.remove("distance_expr");
    j.remove("distance2");
    j.remove("distance2_expr");
}

/// Revolve `E3849A15`: the operation enum is the first u32 of the body (as an extrude's); the
/// profile a `4BD53E5A` (names its sketch) like an extrude's; the angle the model parameter
/// `AlongAngle`; the axis a `2CA5A1CD` selection -> `5A1BF548` -> `E2CEFD18` sketch-curve
/// reference, as a sweep path names its curves: its last 16 bytes are `u64 sketch data id`,
/// `u64 crv_primary_id` (at 0x1d in the 45-byte records whose tag has three decimal places, at
/// 0x21 in the 49-byte ones). Only an axis that is a line of the profile's own sketch and a whole turn are
/// read (every revolve of the corpus); which way a part turn goes is not decoded.
fn revolve_json(cx: &mut Ctx, fid: u64, name: &str, sketch_names: &HashMap<u64, String>) -> Map<String, Value> {
    let d = cx.d;
    let mut j = Map::new();
    j.insert("type".into(), "revolve".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), fid.into());
    let rec = d.rec(fid);
    let Some(h) = stream::parse_header(rec) else {
        j.insert("error".into(), "layout not recognised".into());
        return j;
    };
    let op = match stream::u32_at(rec, h.body) {
        Some(op @ 1..=4) => op as u8,
        _ => {
            j.insert("error".into(), "layout not recognised".into());
            return j;
        }
    };
    cover::field(rec, h.body, 4, "revolve.operation");
    j.insert("operation".into(), op_name(op).into());
    j.insert("target".into(), Value::Null);
    let profile = feature::profile_sketch_of(d, rec);
    j.insert("sketch".into(), profile.and_then(|s| sketch_names.get(&s)).map_or(Value::Null, |s| s.as_str().into()));
    j.insert("sketch_source_id".into(), profile.map_or(Value::Null, Value::from));
    let Some(sid) = profile else {
        j.insert("error".into(), "the profile is not a sketch profile".into());
        return j;
    };
    let s = cx.sketch(sid);
    match cx.rc.regions(d, fid, &s) {
        Ok(rg) => {
            j.insert(
                "profiles".into(),
                rg.iter()
                    .flat_map(|r| std::iter::once(&r.point).chain(r.more_points.iter()))
                    .map(|p| json!({"point": [mm(p[0]), mm(p[1])]}))
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        Err(why) => {
            j.insert("error".into(), format!("profile regions not decoded: {why}").into());
            return j;
        }
    }
    let ps = params_of(d, fid);
    let Some(angle) = pget(&ps, "AlongAngle").and_then(|p| p.value) else {
        j.insert("error".into(), "the revolve has no angle".into());
        return j;
    };
    if let Some((other, _)) = ps.iter().find(|(k, _)| k != "AlongAngle") {
        j.insert("error".into(), format!("the revolve has a parameter '{other}' (not decoded)").into());
        return j;
    }
    if (angle.abs() - std::f64::consts::TAU).abs() > 1e-9 {
        j.insert("error".into(), "a part revolve (which way it turns is not decoded)".into());
        return j;
    }
    j.insert("angle".into(), 360.0.into());
    let mut axes: Vec<(u64, u64)> = Vec::new();
    for q in ref_ids(d, rec).into_iter().filter(|q| d.is(*q, "2CA5A1CD")) {
        for q2 in ref_ids(d, d.rec(q)).into_iter().filter(|q2| d.is(*q2, "5A1BF548")) {
            for c in ref_ids(d, d.rec(q2)).into_iter().filter(|c| d.is(*c, "E2CEFD18")) {
                let r = d.rec(c);
                let word = |o: usize| r.get(o..o + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
                let at = r.len().saturating_sub(16);
                if let (Some(sk), Some(prim)) = (word(at), word(at + 8)) {
                    cover::field(r, at, 16, "revolve-axis.sketch-curve");
                    if !axes.contains(&(sk, prim)) {
                        axes.push((sk, prim));
                    }
                }
            }
        }
    }
    let line = match axes.as_slice() {
        [(sk, prim)] if *sk == sid => sketch::curve_by_primary(d, &s, *prim).filter(|c| s.lines.contains_key(c)),
        _ => None,
    };
    // A construction line of the sketch (dropped from its curves): the axis by its place.
    let construction_axis = match (line, axes.as_slice(), s.plane) {
        (None, [(sk, prim)], Some(pl)) if *sk == sid => sketch::any_line_by_primary(d, sid, *prim).and_then(|l| {
            let p = geom::world(&pl, [l.origin[0], l.origin[1]], l.origin[2]);
            let q = geom::world(&pl, [l.origin[0] + l.vector[0], l.origin[1] + l.vector[1]], l.origin[2] + l.vector[2]);
            Some((p, geom::unit(geom::sub(q, p))?))
        }),
        _ => None,
    };
    match (line, origin_axis(d, rec)) {
        (Some(c), _) => {
            j.insert("axis".into(), json!({"line": format!("c{c}")}));
        }
        (None, _) if let Some((p, v)) = construction_axis => {
            j.insert(
                "axis".into(),
                json!({"origin": [mm(p[0]), mm(p[1]), mm(p[2])], "direction": [round9(v[0]), round9(v[1]), round9(v[2])]}),
            );
        }
        (None, None)
            if axes.is_empty()
                && let Some((p, v)) = face_axis(cx, fid) =>
        {
            j.insert(
                "axis".into(),
                json!({"origin": [mm(p[0]), mm(p[1]), mm(p[2])], "direction": [round9(v[0]), round9(v[1]), round9(v[2])]}),
            );
        }
        (None, Some((p, v))) if axes.is_empty() => {
            j.insert(
                "axis".into(),
                json!({"origin": [mm(p[0]), mm(p[1]), mm(p[2])], "direction": [round9(v[0]), round9(v[1]), round9(v[2])]}),
            );
        }
        _ => {
            j.insert("error".into(), "the revolve axis is not a line of its sketch".into());
        }
    }
    j
}

/// A revolve's axis when it is the axis of a cylindrical face of a body: the selection
/// (`2CA5A1CD` -> `5662F619`) names one face recipe (`7ACC2A03`), and the stored bodies hold that
/// face as a cylinder (1054's Revolve1: about the side of Extrude1's round boss, generator `3`).
/// Which way the axis points does not matter for a whole turn.
fn face_axis(cx: &mut Ctx, fid: u64) -> Option<(V3, V3)> {
    let d = cx.d;
    let mut recipes: Vec<u64> = Vec::new();
    for q in ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "2CA5A1CD")) {
        for s in ref_ids(d, d.rec(q)).into_iter().filter(|s| d.is(*s, "5662F619")) {
            for r in ref_ids(d, d.rec(s)).into_iter().filter(|r| d.is(*r, "7ACC2A03")) {
                if !recipes.contains(&r) {
                    recipes.push(r);
                }
            }
        }
    }
    let [rid] = recipes.as_slice() else { return None };
    let (Some(RecipeKind::Face), Some(faces)) = naming::recipe(d, *rid) else { return None };
    let [face] = faces.as_slice() else { return None };
    let (g, ops) = face.first()?;
    match cx.stored_face(g, ops)?.surface {
        naming::Surface::Cylinder { origin, axis, radius } if radius > 0.0 => Some((origin, geom::unit(axis)?)),
        _ => None,
    }
}

/// A revolve's axis when it is an axis of its component's origin: the selection (`5A1BF548`)
/// names it by id (`90055C05`, a `u64` at its body), and the axis record (`19031004`, named `X`,
/// `Y` or `Z`) holds a point and a unit direction (cm, the component's frame) at its body (3350:
/// three revolves about Z, the record's point 0 and direction (0, 0, 1)).
fn origin_axis(d: &Design, rec: &[u8]) -> Option<(V3, V3)> {
    let mut found: Vec<u64> = Vec::new();
    for q in ref_ids(d, rec).into_iter().filter(|q| d.is(*q, "2CA5A1CD")) {
        for q2 in ref_ids(d, d.rec(q)).into_iter().filter(|q2| d.is(*q2, "5A1BF548")) {
            for a in ref_ids(d, d.rec(q2)).into_iter().filter(|a| d.is(*a, "90055C05")) {
                let r = d.rec(a);
                let at = stream::parse_header(r)?.body;
                let id = u64::from_le_bytes(r.get(at..at + 8)?.try_into().ok()?);
                cover::field(r, at, 8, "origin-axis.id");
                if !found.contains(&id) {
                    found.push(id);
                }
            }
        }
    }
    let [id] = found.as_slice() else { return None };
    if !d.has(*id) || !d.is(*id, "19031004") {
        return None;
    }
    let r = d.rec(*id);
    let named = stream::wstrings_all(r).into_iter().any(|w| matches!(w.2.as_str(), "X" | "Y" | "Z"));
    let at = stream::parse_header(r)?.body;
    let v = stream::finite_doubles::<6>(r, at, 1e6)?;
    let (p, dir) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
    if !named || (geom::norm(dir) - 1.0).abs() > 1e-9 {
        return None;
    }
    cover::field(r, at, 48, "origin-axis.point-direction");
    Some((p, dir))
}

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
    cover::field(rec, h.body, 1, "sweep.operation");
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
            // The record's last 16 bytes (at 0x21 in the 49-byte records, at 0x1d in the 45-byte
            // ones of 5358), as a revolve axis reads them.
            let r = d.rec(c);
            let word = |o: usize| r.get(o..o + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
            let at = r.len().saturating_sub(16);
            let (Some(sk), Some(prim)) = (word(at), word(at + 8)) else {
                ok = false;
                continue;
            };
            cover::field(r, at, 16, "sweep-path.sketch-curve");
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

/// The faces a feature's profile selection names (`2CA5A1CD` -> `5662F619` -> face recipes), by
/// their first (generator, operations) name.
fn profile_faces(d: &Design, fid: u64) -> Vec<(String, Vec<i32>)> {
    let mut faces: Vec<(String, Vec<i32>)> = Vec::new();
    for q in ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "2CA5A1CD")) {
        for s in ref_ids(d, d.rec(q)).into_iter().filter(|s| d.is(*s, "5662F619")) {
            for r in ref_ids(d, d.rec(s)).into_iter().filter(|r| d.is(*r, "7ACC2A03") || d.is(*r, "71D91267")) {
                if let (Some(naming::RecipeKind::Face | naming::RecipeKind::BoundedFace), Some(names)) =
                    naming::recipe(d, r)
                    && let Some(f) = names.first().and_then(|f| f.first())
                    && !faces.contains(f)
                {
                    faces.push(f.clone());
                }
            }
        }
    }
    faces
}

/// A sweep whose profile is a face (5358's `Sweep - Pin Right Angle`: a `5662F619` face
/// selection, no `4BD53E5A` profile): where the face is the end cap of an extrude that no later
/// feature renamed (one operation in its name), the face is that extrude's profile on the cap's
/// plane. The sketch of the extrude, its curves cut down to the profile's, moved along its normal
/// to the plane through the path's end nearest to the cap (Fusion sweeps a profile from the
/// plane through the path's start, as for a sketch profile). `Ok(None)`: no face selection.
fn face_profile_sketch(
    cx: &mut Ctx,
    fid: u64,
    feats: &[Value],
    j: &Map<String, Value>,
) -> Result<Option<Map<String, Value>>, String> {
    let d = cx.d;
    let faces = profile_faces(d, fid);
    let [(g, ops)] = faces.as_slice() else {
        return if faces.is_empty() { Ok(None) } else { Err(format!("{} faces, not one", faces.len())) };
    };
    let [op] = ops.as_slice() else { return Err("a face a later feature changed".into()) };
    if g != "1" && g != "2" {
        return Err("not the end cap of an extrude".into());
    }
    let efid = u64::try_from(*op).ok().and_then(|o| cx.opmap.get(&o).copied()).ok_or("its feature is unknown")?;
    if !d.is(efid, EXTRUDE) {
        return Err("not a face of an extrude".into());
    }
    let sid = extrude_raw(d, efid).and_then(|e| e.sketch).ok_or("its extrude has no sketch")?;
    let sk = cx.sketch(sid);
    let pl = sk.plane.ok_or("its extrude's sketch has no plane")?;
    let Some(naming::FaceG::Plane { n, p, .. }) = cx.face(g, ops) else { return Err("the cap is not modelled".into()) };
    if (dot(n, pl.z).abs() - 1.0).abs() > 1e-9 {
        return Err("the cap does not lie parallel to its sketch".into());
    }
    let t_face = dot(pl.z, geom::sub(p, pl.origin));
    // The free ends of the path's chain, as heights along the profile's normal.
    let path = j.get("path_source_id").and_then(Value::as_u64).ok_or("its path is not read")?;
    let ps = cx.sketch(path);
    let ppl = ps.plane.ok_or("its path sketch has no plane")?;
    let mut ends: Vec<(u64, V3)> = Vec::new();
    for c in j.get("path_curves").and_then(Value::as_array).into_iter().flatten() {
        let q: u64 = c.as_str().and_then(|c| c.strip_prefix('c')).and_then(|c| c.parse().ok()).ok_or("a path curve")?;
        let refs: Vec<u64> = if let Some(l) = ps.lines.get(&q) {
            [l.start_ref, l.end_ref].into_iter().flatten().collect()
        } else {
            ps.arcs.get(&q).ok_or("a path curve is neither line nor arc")?.refs.iter().skip(1).copied().collect()
        };
        for r in refs {
            let p = *ps.points.get(&r).ok_or("a path point")?;
            ends.push((r, geom::world(&ppl, [p[0], p[1]], 0.0)));
        }
    }
    let t = ends
        .iter()
        .filter(|(r, _)| ends.iter().filter(|(x, _)| x == r).count() == 1)
        .map(|(_, q)| dot(pl.z, geom::sub(*q, pl.origin)))
        .min_by(|a, b| (a - t_face).abs().total_cmp(&(b - t_face).abs()))
        .ok_or("its path has no free end")?;
    // The cap is one face: the extrude's regions united. A curve every piece of which bounds two
    // of the regions lies inside that face and is left out (5358's pin: the disc of r 0.45 and the
    // ring out to 0.5 are one cap, a disc of 0.5; swept as two regions, the 0.1 mm blend of the
    // end's rim ran into the circle between them).
    let regions = cx.rc.regions(d, efid, &sk)?;
    let key = |sg: &sketch::Seg| {
        let r = |v: f64| (v * 1e6).round() as i64;
        let (a, b) = (sg.poly.first().copied().unwrap_or_default(), sg.poly.last().copied().unwrap_or_default());
        let (a, b) = ([r(a[0]), r(a[1])], [r(b[0]), r(b[1])]);
        (sg.curve, a.min(b), a.max(b))
    };
    let mut uses: HashMap<(u64, [i64; 2], [i64; 2]), HashSet<usize>> = HashMap::new();
    for (k, r) in regions.iter().enumerate() {
        for sg in &r.segments {
            uses.entry(key(sg)).or_default().insert(k);
        }
    }
    let used: HashSet<String> = uses
        .keys()
        .map(|(c, _, _)| *c)
        .filter(|c| uses.iter().any(|((q, _, _), rs)| q == c && rs.len() < 2))
        .map(|c| format!("c{c}"))
        .collect();
    // The sketch as read, else a copy of it (its plane is set below; the curves are all there).
    let of_sid = |f: &&Value| {
        f.get("type").and_then(Value::as_str) == Some("sketch")
            && f.get("source_id").and_then(Value::as_u64) == Some(sid)
    };
    let found =
        feats.iter().filter(of_sid).find(|f| f.get("plane_note").is_none()).or_else(|| feats.iter().find(of_sid));
    let Some(Value::Object(mut c)) = found.cloned() else {
        return Err("its extrude's sketch is not in the tree".into());
    };
    if let Some(cs) = c.get_mut("curves").and_then(Value::as_array_mut) {
        cs.retain(|x| x.get("id").and_then(Value::as_str).is_some_and(|i| used.contains(i)));
    }
    let o = geom::add(pl.origin, geom::mul(pl.z, t));
    c.insert("plane".into(), json!({"origin": v3mm(o), "x_axis": pl.x, "y_axis": pl.y}));
    c.insert("plane_note".into(), "an extrude's profile, the face a sweep takes, on the path's start plane".into());
    Ok(Some(c))
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
        let parts: Vec<(f64, i64)> = if sy && n == 2 {
            // Two instances, symmetric: the original and one copy at the stored (signed)
            // spacing (5905's PinPattern2: the second row of a 2 x 20 header at -2.54 mm, where
            // the header body is; section 30). Other even counts are not established.
            vec![(sp, 2)]
        } else if sy {
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
            let mut j = json!({
                "type": "pattern_linear", "name": nm, "features": features,
                "direction": [round9(dir[0]), round9(dir[1]), round9(dir[2])], "spacing": round9(*sp_), "count": n_,
                "target": p.target, "pattern_axis": tag, "symmetric": sy,
                "note": format!("direction = the stored F2A7590D vector (sign included){}", if sy { "; symmetric about the original: split into + and - halves" } else { "" }),
            });
            unmerged(&mut j);
            out.push(j);
            names_now.push(nm);
        }
        made.extend(names_now);
    }
    Some(out)
}

/// The features that made the faces a mirror of faces selects, in the order first met: each
/// selection is a face recipe naming one face by its generator and one operation, and each
/// operation stands for one feature. `None` where a selection is not such a face.
fn mirrored_faces_makers(cx: &Ctx, fid: u64) -> Option<Vec<(u64, String, i32)>> {
    let mut out = Vec::new();
    for rid in cx.input_recipes(fid) {
        let (Some(RecipeKind::Face), Some(faces)) = naming::recipe(cx.d, rid) else { return None };
        let [name] = faces.as_slice() else { return None };
        let [(g, ops)] = name.as_slice() else { return None };
        let [op] = ops.as_slice() else { return None };
        let f = cx.opmap.get(&u64::try_from(*op).ok()?).copied()?;
        out.push((f, g.clone(), *op));
    }
    (!out.is_empty()).then_some(out)
}

/// The tree features for a mirror of faces (`mirror` its name, `o`, `n` its plane in mm), where
/// every face is made by a cut or join extrude of the tree or by a fillet or chamfer of it, and
/// at least one by an extrude. Without blends: a mirror of those extrudes. With blends (joins
/// only): each extrude copied as a body of its own, then a copy of each blend that blends only
/// the edges whose faces are selected, reflected, then the copies joined. The edge of a selected
/// blend face is the blend's edge nearest the face as the stored bodies hold it (2232's Mirror2
/// selects two of Chamfer3's eight faces, those on the edges of Extrude12, with two of
/// Extrude12's). `None` otherwise.
fn mirrored_faces_features(
    cx: &mut Ctx,
    feats: &[Value],
    sel: &[(u64, String, i32)],
    mirror: &str,
    (o, n): (V3, V3),
) -> Option<Vec<Value>> {
    let kind = |f: &Value| f.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
    let reflect = |p: V3| geom::sub(p, geom::mul(n, 2.0 * geom::dot(geom::sub(p, o), n)));
    let mut extrudes: Vec<u64> = Vec::new();
    // Per blend feature of the tree (index into `feats`): the indices of the edges selected.
    let mut picked: Vec<(usize, Vec<usize>)> = Vec::new();
    for (m, g, op) in sel {
        let mine: Vec<usize> = (0..feats.len())
            .filter(|i| feats.get(*i).and_then(|f| f.get("source_id")).and_then(Value::as_u64) == Some(*m))
            .filter(|i| feats.get(*i).is_some_and(|f| kind(f) != "sketch"))
            .collect();
        let first = feats.get(*mine.first()?)?;
        match kind(first).as_str() {
            "extrude" if matches!(first.get("operation").and_then(Value::as_str), Some("cut" | "join")) => {
                if !extrudes.contains(m) {
                    extrudes.push(*m);
                }
            }
            "fillet" | "chamfer" => {
                let pick = cx.stored_face(g, &[*op])?;
                let k = pick.points.len() as f64;
                let c = pick.points.iter().fold([0.0; 3], |a, p| geom::add(a, *p));
                let c = [mm(c[0] / k), mm(c[1] / k), mm(c[2] / k)];
                // The nearest edge over every set of the blend; it must be clearly the nearest.
                let mut best: Vec<(f64, usize, usize)> = Vec::new();
                for i in &mine {
                    let es = feats.get(*i)?.get("edges")?.as_array()?;
                    for (j, e) in es.iter().enumerate() {
                        let mid = get3(e.get("midpoint"))?;
                        best.push((geom::norm(geom::sub(mid, c)), *i, j));
                    }
                }
                best.sort_by(|a, b| a.0.total_cmp(&b.0));
                let (d0, i, j) = *best.first()?;
                if best.get(1).is_some_and(|b| b.0 < 2.0 * d0 + 1e-6) {
                    return None;
                }
                match picked.iter_mut().find(|(x, _)| *x == i) {
                    Some((_, js)) if !js.contains(&j) => js.push(j),
                    Some(_) => {}
                    None => picked.push((i, vec![j])),
                }
            }
            _ => return None,
        }
    }
    if extrudes.is_empty() {
        return None;
    }
    let names: Vec<String> = feats
        .iter()
        .filter(|f| f.get("source_id").and_then(Value::as_u64).is_some_and(|s| extrudes.contains(&s)))
        .filter(|f| kind(f) == "extrude")
        .filter_map(|f| f.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    let mut blends = Vec::new();
    picked.sort_by_key(|(i, _)| *i);
    for (i, js) in picked {
        let Some(Value::Object(f)) = feats.get(i) else { return None };
        let mut c = f.clone();
        let es = f.get("edges")?.as_array()?;
        let mut out = Vec::new();
        for j in js {
            let e = es.get(j)?;
            let mut e2 = Map::new();
            e2.insert("midpoint".into(), v3_9(reflect(get3(e.get("midpoint"))?)));
            if let Some(path) = e.get("path").and_then(Value::as_array) {
                let pts: Option<Vec<Value>> = path.iter().map(|p| get3(Some(p)).map(|p| v3_9(reflect(p)))).collect();
                e2.insert("path".into(), pts?.into());
            }
            out.push(Value::Object(e2));
        }
        let name = f.get("name").and_then(Value::as_str).unwrap_or_default();
        c.insert("name".into(), format!("{mirror} {name}").into());
        c.insert("edges".into(), out.into());
        c.insert("note".into(), format!("the edges of {name} whose faces {mirror} mirrors, reflected").into());
        blends.push(Value::Object(c));
    }
    if blends.is_empty() {
        return Some(vec![json!({"type": "mirror", "name": mirror, "features": names,
            "plane": {"origin": v3mm_raw(o), "normal": [round9(n[0]), round9(n[1]), round9(n[2])]},
            "note": "a mirror of faces, all of them made by these features"})]);
    }
    // With blends the copies are blended before they join: Fusion copies the blended faces, and
    // a blend of the joined copy would run into the body (2232: the mirrored wedge reaches 0.38 mm
    // out of a wall, its 1 mm chamfers would not fit). So each extrude is copied as a new body,
    // reflected like the tree's mirror of features reflects it (its sketch's frame reflected, the
    // distance negated), blended, then joined to the body the extrude joined or cut.
    let mut out = Vec::new();
    let mut joins: Vec<(String, String)> = Vec::new();
    for nm in &names {
        let f = feats.iter().rev().find(|f| f.get("name").and_then(Value::as_str) == Some(nm.as_str()))?;
        let target = f.get("target").and_then(Value::as_str)?.to_string();
        if f.get("operation").and_then(Value::as_str) != Some("join") {
            return None;
        }
        let known = [
            "type",
            "name",
            "sketch",
            "sketch_source_id",
            "source_id",
            "distance",
            "distance_expr",
            "operation",
            "target",
            "creates_bodies",
            "symmetric",
            "profiles",
            "component",
            "_component",
            "in_timeline",
        ];
        let Value::Object(fo) = f else { return None };
        if fo.keys().any(|k| !known.contains(&k.as_str())) || fo.get("symmetric").and_then(Value::as_bool) == Some(true)
        {
            return None;
        }
        let dist = fo.get("distance").and_then(Value::as_f64)?;
        let sk = fo.get("sketch").and_then(Value::as_str)?;
        let Some(Value::Object(s)) = feats.iter().rev().find(|g| {
            g.get("type").and_then(Value::as_str) == Some("sketch") && g.get("name").and_then(Value::as_str) == Some(sk)
        }) else {
            return None;
        };
        let mut s = s.clone();
        let plane = s.get("plane")?;
        let (po, px, py) = (get3(plane.get("origin"))?, get3(plane.get("x_axis"))?, get3(plane.get("y_axis"))?);
        let dir = |v: V3| geom::sub(v, geom::mul(n, 2.0 * geom::dot(v, n)));
        let sname = format!("{mirror} {sk}");
        s.insert("name".into(), sname.clone().into());
        s.insert(
            "plane".into(),
            json!({"origin": v3_9(reflect(po)), "x_axis": v3_9(dir(px)), "y_axis": v3_9(dir(py))}),
        );
        s.insert("plane_note".into(), format!("{sk} reflected across the plane of {mirror}").into());
        out.push(Value::Object(s));
        let mut e = fo.clone();
        let ename = format!("{mirror} {nm}");
        e.insert("name".into(), ename.clone().into());
        e.insert("sketch".into(), sname.into());
        e.insert("distance".into(), (-dist).into());
        e.remove("distance_expr");
        e.insert("operation".into(), "new".into());
        e.insert("target".into(), Value::Null);
        e.insert("creates_bodies".into(), 1.into());
        e.insert("note".into(), format!("{nm} reflected across the plane of {mirror}, as a body of its own").into());
        out.push(Value::Object(e));
        joins.push((ename, target));
    }
    out.extend(blends);
    for (k, (tool, target)) in joins.into_iter().enumerate() {
        let name = if k == 0 { mirror.to_string() } else { format!("{mirror} ({})", k + 1) };
        out.push(json!({"type": "combine", "name": name, "target": target, "tools": [tool],
            "operation": "join",
            "note": "the mirrored copy, blended, joins the body its feature changed"}));
    }
    Some(out)
}

/// A pattern of a body makes one new body per copy in the file: the tree's pattern keeps them
/// apart (`merge: false`; copy `i` is the body `<name>[i]`).
fn unmerged(pattern: &mut Value) {
    if pattern.get("target").is_some_and(Value::is_string)
        && let Some(o) = pattern.as_object_mut()
    {
        o.insert("merge".into(), false.into());
    }
}

/// The instances a pattern leaves out (1 is the original): its instance list `7493A99D` ends
/// with `u32 n` and `n` x `u32` instance numbers, ascending (512 C-Pattern1: 4 instances, `2, 3,
/// 4` = copies 3 and 4 left out, one body made; the Crickit's 12 leave out 4 and 10, 9 bodies).
/// `None` where the list is not found.
fn suppressed_instances(d: &Design, fid: u64, copies: usize) -> Option<Vec<u32>> {
    let list = ref_ids(d, d.rec(fid)).into_iter().find(|q| d.is(*q, "7493A99D"))?;
    let r = d.rec(list);
    let u = |o: usize| r.get(o..o + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
    for n in 0..copies.min(500) {
        let at = r.len().checked_sub(4 * (n + 1))?;
        if u(at) != Some(n as u32) {
            continue;
        }
        let ks: Vec<u32> = (0..n).filter_map(|i| u(at + 4 + 4 * i)).collect();
        if ks.len() == n
            && ks.iter().all(|k| (2..=copies as u32).contains(k))
            && ks.windows(2).all(|w| matches!(w, [a, b] if a < b))
        {
            cover::field(r, at, 4 * (n + 1), "pattern.suppressed");
            return Some(ks);
        }
    }
    None
}

/// `<name>[i]`, the tree's name of copy `i` of the unmerged body pattern `name`.
pub(super) fn copy_name(name: &str, i: u64) -> String {
    format!("{name}[{i}]")
}

/// A feature of one component that uses a sketch of another takes the sketch in its own
/// component, at the sketch's own coordinates: the stored bodies have it so (4863: Header
/// Female's Extrude1 of the header's Sketch1, the Pins' sweeps along the header's path), and
/// the occurrences that joints moved apart later do not carry it along. The feature gets a copy
/// of the sketch, which is placed with the feature's component. `key` names the sketch (by
/// tree name); sketches written for this feature (`nbefore` on) are tagged with its component
/// already.
fn in_own_component(
    j: &mut Map<String, Value>,
    key: &str,
    comp: Option<u64>,
    feats: &mut Vec<Value>,
    nbefore: usize,
    copy: String,
) {
    let Some(sk) = j.get(key).and_then(Value::as_str) else { return };
    let found = feats.iter().take(nbefore).rev().find(|f| {
        f.get("type").and_then(Value::as_str) == Some("sketch") && f.get("name").and_then(Value::as_str) == Some(sk)
    });
    if let Some(Value::Object(c)) = found
        && let (Some(theirs), Some(ours)) = (c.get("_component").and_then(Value::as_u64), comp)
        && theirs != ours
    {
        let mut c = c.clone();
        c.insert("name".into(), copy.clone().into());
        c.insert("plane_note".into(), "a sketch of another component, taken in this feature's".into());
        j.insert(key.into(), copy.into());
        feats.push(Value::Object(c));
    }
}

pub(super) fn design_json(cx: &mut Ctx) -> Value {
    let d = cx.d;
    let mut feats: Vec<Value> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut sketch_names: HashMap<u64, String> = HashMap::new();
    let (tl, extra) = timeline(d);
    let mut used_names: HashSet<String> = HashSet::new();
    let mut g = Graph::new(d);
    let moves = super::bodyops::carry_moved_bodies(d, &mut g);
    let halves: HashMap<u64, u64> = moves
        .iter()
        .filter_map(|(m, r)| r.as_ref().ok().map(|h| (*m, *h)))
        .flat_map(|(m, h)| h.map(|q| (q, m)))
        .collect();
    let g = g;
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
        if let Some(m) = halves.get(&fid) {
            skipped.push(json!({"name": name, "fusion_type": fusion_type, "source_id": fid,
                "reason": format!("a half of the body move {m}: the body is carried on as the body it moves")}));
            continue;
        }
        if let Some(r) = moves.get(&fid) {
            match r {
                Ok(_) => skipped.push(json!({"name": name, "fusion_type": fusion_type, "source_id": fid,
                    "reason": "moves a body to another component: the body is carried on"})),
                Err(why) => feats.push(json!({"type": "unsupported", "fusion_type": fusion_type, "name": nm,
                    "class_guid": class, "source_id": fid, "error": why})),
            }
            continue;
        }
        if d.is(fid, mirrorcomp::DERIVED_INSTANCE)
            && ref_ids(d, d.rec(fid)).iter().any(|q| d.is(*q, mirrorcomp::MIRROR_COMPONENT))
        {
            skipped.push(json!({"name": name, "fusion_type": fusion_type, "source_id": fid,
                "reason": "the occurrence of a mirrored component: read with its MirrorComponent"}));
            continue;
        }
        if d.is(fid, mirrorcomp::MIRROR_COMPONENT) {
            let made = mirror_component_json(cx, &mut comps, &feats, fid, &nm);
            match made {
                Ok((mut made, new)) => {
                    tag_feats(&mut made, 0, Some((new.0, new.1.as_deref())), extra.contains(&fid));
                    feats.extend(made);
                }
                Err(why) => {
                    feats.push(json!({"type": "unsupported", "fusion_type": fusion_type, "name": nm,
                        "class_guid": class, "source_id": fid, "error": why}));
                    tag_feats(&mut feats, nbefore, comp.map(|c| (c, comp_name.as_deref())), extra.contains(&fid));
                }
            }
            continue;
        }
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
            super::thin::thin_extrude(cx, fid, &mut j, &mut feats, &sketch_names, &nm);
            profile_of_an_exposed_cap(cx, fid, &mut j, &feats);
            two_sided_from_a_moved_sketch(cx, &mut j, &nm, &mut feats);
            in_own_component(&mut j, "sketch", comp, &mut feats, nbefore, format!("{nm} sketch"));
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
            if j.get("sketch_source_id").is_none_or(Value::is_null) {
                match face_profile_sketch(cx, fid, &feats, &j) {
                    Ok(Some(mut c)) => {
                        let copy = format!("{nm} profile");
                        c.insert("name".into(), copy.clone().into());
                        j.insert("sketch".into(), copy.into());
                        j.insert(
                            "profile_from".into(),
                            "the cap face of an extrude: its profile, on the path's start plane".into(),
                        );
                        feats.push(Value::Object(c));
                    }
                    Ok(None) => {}
                    Err(why) => {
                        j.insert("error".into(), format!("its profile is a face: {why}").into());
                    }
                }
            }
            in_own_component(&mut j, "sketch", comp, &mut feats, nbefore, format!("{nm} sketch"));
            in_own_component(&mut j, "path", comp, &mut feats, nbefore, format!("{nm} path"));
            let made = op_target(&mut j, &g, d, fid, &names);
            j.insert("creates_bodies".into(), made.into());
            feats.push(Value::Object(j));
        } else if (d.is(fid, "A07D5F17") || d.is(fid, "F757D611"))
            && ref_ids(d, d.rec(fid))
                .iter()
                .all(|q| !["4A557CE2", "D91D429C", "0D383DB2", "5662F619", "2CA5A1CD"].iter().any(|c| d.is(*q, c)))
        {
            // A fillet or chamfer that names no edge, set or value (5358's off-timeline Fillet1 in
            // Housing references nothing but the design root): it changes no body.
            skipped.push(json!({"name": name, "fusion_type": fusion_type, "source_id": fid,
                "reason": "a fillet or chamfer with no edges and no value: it changes nothing"}));
            continue;
        } else if d.is(fid, "A07D5F17") || d.is(fid, "F757D611") {
            let fil = d.is(fid, "A07D5F17");
            let key = if fil { "radius" } else { "distance" };
            // A chamfer of two distances (section 30): the first along a reference face.
            // Older records name the two `leftDistance` and `rightDistance` (2895's Chamfer2: 2
            // and 3 mm, its stored faces 2 mm across the top and 3 mm down the side); equal, they
            // are a chamfer of one distance.
            let value = |n: &str| feature::param_values(d, fid, &[n]).first().copied();
            let pair = if fil {
                None
            } else {
                match (value("Distance 1"), value("Distance 2"), value("leftDistance"), value("rightDistance")) {
                    (Some(d1), Some(d2), _, _) => Some(("Distance 1", d1, d2)),
                    (None, None, Some(d1), Some(d2)) if (d1 - d2).abs() > 1e-9 => Some(("leftDistance", d1, d2)),
                    _ => None,
                }
            };
            let two = pair.map(|(first, d1, d2)| cx.chamfer_reference(fid, first, d1, d2).map(|f| (d2, f)));
            let first = [pair.map_or("Distance 1", |p| p.0)];
            let role: &[&str] = match (fil, &two) {
                (true, _) => &["Radius"],
                (false, Some(_)) => &first,
                (false, None) => &["Distance", "leftDistance"],
            };
            // The body its edges are on, by the feature that made it, where that is one: the
            // importer needs it where edges of two bodies lie on one another (5358's magnet in
            // its pocket).
            let mut on: Vec<String> = g
                .body_links(d, fid, 2)
                .iter()
                .filter_map(|l| g.creator.get(l).copied())
                .filter(|c| *c != fid)
                .filter_map(|c| names.get(&c).cloned())
                .collect();
            on.sort();
            on.dedup();
            for (k, (v, es, bad)) in cx.blend_sets(fid, role).into_iter().enumerate() {
                let mut j = Map::new();
                if let [one] = on.as_slice() {
                    j.insert("on_body".into(), one.as_str().into());
                }
                match &two {
                    Some(Ok((d2, face))) => {
                        j.insert("distance2".into(), mm(*d2).into());
                        j.insert("face".into(), face.clone());
                    }
                    Some(Err(why)) => {
                        j.insert("error".into(), format!("a chamfer of two distances: {why}").into());
                    }
                    None => {}
                }
                j.insert("type".into(), if fil { "fillet" } else { "chamfer" }.into());
                j.insert("name".into(), if k == 0 { nm.clone() } else { format!("{nm} set {}", k + 1) }.into());
                j.insert("source_id".into(), fid.into());
                j.insert(
                    key.into(),
                    v.map_or(Value::Null, |v| {
                        if std::env::var_os("XR21").is_some() && nm == "Fillet2" {
                            (mm(v) - 0.0001).into()
                        } else {
                            mm(v).into()
                        }
                    }),
                );
                // A curved edge carries its points (`path`): the model's edge that holds them
                // stands for it where its own middle lies elsewhere (a whole circle).
                let edge = |(m, path): &(V3, Vec<V3>)| {
                    if path.is_empty() {
                        json!({"midpoint": v3mm(*m)})
                    } else {
                        json!({"midpoint": v3mm(*m), "path": path.iter().map(|q| v3mm(*q)).collect::<Vec<_>>()})
                    }
                };
                j.insert("edges".into(), es.iter().map(edge).collect::<Vec<_>>().into());
                if bad > 0 {
                    j.insert("edges_unresolved".into(), bad.into());
                }
                if es.is_empty() && bad == 0 {
                    j.insert("edges_note".into(), "no edge recipes found".into());
                }
                feats.push(Value::Object(j));
            }
        } else if d.is(fid, "E3849A15") {
            let mut j = revolve_json(cx, fid, &nm, &sketch_names);
            in_own_component(&mut j, "sketch", comp, &mut feats, nbefore, format!("{nm} sketch"));
            let made = op_target(&mut j, &g, d, fid, &names);
            j.insert("creates_bodies".into(), made.into());
            feats.push(Value::Object(j));
        } else if d.is(fid, "1C037A07") {
            feats.push(Value::Object(hole_json(cx, fid, &nm)));
        } else if d.is(fid, "D1728651") {
            // A construction plane among the inputs is the mirror plane, not a feature it repeats
            // (2232's Mirror2 names WorkPlane2 and nothing else).
            let ins: Vec<u64> = g.inputs(d, fid).into_iter().filter(|q| !d.is(*q, "D869265F")).collect();
            let in_names: Vec<String> =
                ins.iter().map(|q| names.get(q).cloned().unwrap_or_else(|| q.to_string())).collect();
            // The plane is a planar face where the plane holder (`C444E6AD`) selects one (a header
            // part's Mirror1: the extrude's side face x = 0); otherwise a construction plane.
            let plane = mirror_plane(cx, fid);
            match plane {
                Some((o, n)) if !ins.is_empty() && ins.iter().all(|q| names.contains_key(q)) => {
                    feats.push(json!({"type": "mirror", "name": nm, "source_id": fid, "features": in_names,
                        "plane": {"origin": v3mm(o), "normal": [round9(n[0]), round9(n[1]), round9(n[2])]}}));
                }
                // A mirror of bodies names no feature: each selected body, named by the feature
                // that made it, gets a copy of its own (the mirror is the creator of one new
                // body per selected one; a mirror that makes none joins, which is not read).
                Some((o, n)) if ins.is_empty() => {
                    let mut body_in: Vec<String> = Vec::new();
                    // Per named body, the selected bodies its feature made.
                    let mut per: Vec<u64> = Vec::new();
                    let mut known = true;
                    for l in g.selected_bodies(d, fid) {
                        match g.creator.get(&l).copied().filter(|c| *c != fid).and_then(|c| names.get(&c)) {
                            Some(n) => match body_in.iter().position(|b| b == n) {
                                Some(k) => per.get_mut(k).into_iter().for_each(|c| *c += 1),
                                None => {
                                    body_in.push(n.clone());
                                    per.push(1);
                                }
                            },
                            None => known = false,
                        }
                    }
                    let made = g.body_links(d, fid, 4).iter().filter(|l| g.creator.get(l) == Some(&fid)).count();
                    // A feature that made several bodies is one body of several solids here (3101:
                    // Sweep1's two legs): where the mirror selects all of them and makes one copy
                    // of each, the copy of that body is the copies of its solids.
                    let all_of_their_makers = body_in.iter().zip(&per).all(|(b, n)| {
                        *n == 1
                            || feats.iter().any(|f| {
                                f.get("name").and_then(Value::as_str) == Some(b.as_str())
                                    && matches!(
                                        f.get("type").and_then(Value::as_str),
                                        Some("extrude" | "sweep" | "revolve")
                                    )
                                    && f.get("operation").and_then(Value::as_str) == Some("new")
                                    && f.get("creates_bodies").and_then(Value::as_u64) == Some(*n)
                            })
                    });
                    let solids: u64 = per.iter().sum();
                    let o_mm = [mm(o[0]), mm(o[1]), mm(o[2])];
                    let faces = if body_in.is_empty() && made == 0 {
                        mirrored_faces_makers(cx, fid)
                            .and_then(|sel| mirrored_faces_features(cx, &feats, &sel, &nm, (o_mm, n)))
                    } else {
                        None
                    };
                    if let Some(made) = faces {
                        // A mirror of faces that joins or cuts (and blends of their edges) made.
                        feats.extend(made.into_iter().map(|mut f| {
                            if let Some(o) = f.as_object_mut() {
                                o.insert("source_id".into(), fid.into());
                            }
                            f
                        }));
                    } else if known && !body_in.is_empty() && made as u64 == solids && all_of_their_makers {
                        for (k, (t, solids_of)) in body_in.iter().zip(&per).enumerate() {
                            let name = if k == 0 { nm.clone() } else { format!("{nm} body {}", k + 1) };
                            feats.push(json!({"type": "mirror", "name": name, "source_id": fid, "features": [],
                                "target": t, "merge": false, "creates_bodies": solids_of,
                                "plane": {"origin": v3mm(o), "normal": [round9(n[0]), round9(n[1]), round9(n[2])]}}));
                        }
                    } else {
                        feats.push(json!({"type": "unsupported", "gtree_type": "mirror", "fusion_type": fusion_type,
                            "name": nm, "source_id": fid, "features": in_names,
                            "note": format!("it mirrors {} bodies and makes {made}: not one copy per body of a known feature", body_in.len())}));
                    }
                }
                _ => feats.push(
                    json!({"type": "unsupported", "gtree_type": "mirror", "fusion_type": fusion_type, "name": nm,
                    "source_id": fid, "features": in_names,
                    "note": "the mirror plane is not an origin/construction plane frame"}),
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
            // One new body per copy: fewer means the file suppresses some instances (the sample
            // C-Pattern of 12 makes 9 bodies), which a tree pattern cannot say.
            let made = g.body_links(d, fid, 4).iter().filter(|l| g.creator.get(l) == Some(&fid)).count();
            let copies = {
                let n = val("countU").unwrap_or(1.0).round().max(1.0) as usize;
                if linear { n * val("CountV").unwrap_or(1.0).round().max(1.0) as usize } else { n }
            };
            // A pattern of one feature that made a body of its own, a mirror of a body into a
            // new one (5905's R-Pattern3): the copies are copies of that body, a body pattern.
            // So is a pattern of a new-body extrude, revolve or sweep that makes one body per
            // copy (2190's R-Pattern1 of Extrude4: three bodies), where it is the feature just
            // before the pattern (no later feature has changed its body).
            let new_body = |f: &Value| {
                matches!(f.get("type").and_then(Value::as_str), Some("extrude" | "revolve" | "sweep"))
                    && f.get("operation").and_then(Value::as_str) == Some("new")
                    && f.get("creates_bodies").and_then(Value::as_u64) == Some(1)
                    && made + 1 == copies
                    && copies > 1
            };
            let last_solid = feats.iter().rev().find(|f| f.get("type").and_then(Value::as_str) != Some("sketch"));
            let body_feature: Option<String> = match ins.as_slice() {
                [one] => names
                    .get(one)
                    .filter(|n| {
                        feats.iter().any(|f| {
                            f.get("name").and_then(Value::as_str) == Some(n.as_str())
                                && f.get("type").and_then(Value::as_str) == Some("mirror")
                                && f.get("merge").and_then(Value::as_bool) == Some(false)
                                && f.get("creates_bodies").and_then(Value::as_u64) == Some(1)
                                && f.get("features").and_then(Value::as_array).is_some_and(Vec::is_empty)
                        }) || last_solid
                            .is_some_and(|f| f.get("name").and_then(Value::as_str) == Some(n.as_str()) && new_body(f))
                    })
                    .cloned(),
                _ => None,
            };
            let ins = if body_feature.is_some() { Vec::new() } else { ins };
            let in_names: Vec<String> =
                ins.iter().map(|q| names.get(q).cloned().unwrap_or_else(|| q.to_string())).collect();
            let mut dirs = feature::pattern_dirs(d, fid);
            // One stored direction and a second count: V is perpendicular to U, on the side the
            // stored bodies show.
            let mut v_from_bodies = false;
            if let ([(pt, u)], true, Some(nu), Some(nv)) =
                (dirs.as_slice(), linear && !ins.is_empty(), val("countU"), val("CountV").filter(|n| *n > 1.5))
            {
                let lat = super::patdir::Lattice {
                    u: *u,
                    spacing_u: val("uSpaceDistance").unwrap_or(0.0),
                    count_u: nu.round().max(1.0) as usize,
                    spacing_v: val("vSpaceDistance").unwrap_or(0.0),
                    count_v: nv.round() as usize,
                };
                if let Some(v) = cx.second_direction(&ins, &lat) {
                    let pt = *pt;
                    dirs.push((pt, v));
                    v_from_bodies = true;
                }
            }
            // A pattern of bodies names no feature: the bodies it copies are the ones its input
            // records reach, each named by the feature that made it.
            let mut body_in: Vec<String> = body_feature.into_iter().collect();
            if body_in.is_empty() && ins.is_empty() {
                for l in g.selected_bodies(d, fid) {
                    if let Some(c) = g.creator.get(&l).copied().filter(|c| *c != fid)
                        && let Some(n) = names.get(&c)
                        && !body_in.contains(n)
                    {
                        body_in.push(n.clone());
                    }
                }
            }
            // The instances the pattern leaves out (only a circular pattern of a body is
            // converted with some: one pattern per kept copy).
            let skipped_copies = suppressed_instances(d, fid, copies);
            let skips = if !linear && ins.is_empty() { skipped_copies.as_deref().unwrap_or(&[]) } else { &[] };
            let target: Option<&str> = match body_in.as_slice() {
                [one] if made + 1 + skips.len() == copies => Some(one.as_str()),
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
                            if v_from_bodies {
                                o.insert(
                                    "direction_v_from".into(),
                                    "perpendicular to the stored direction, on the side where the stored bodies have the copies' edges".into(),
                                );
                            }
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
                if !skips.is_empty() {
                    // Copy `k` (the original is 1) turned by (k - 1) / count of the circle: a
                    // pattern of two over that angle makes it alone.
                    let n = count.round() as u32;
                    for k in (2..=n).filter(|k| !skips.contains(k)) {
                        let turn = round9(360.0 * f64::from(k - 1) / f64::from(n));
                        let name = if k == 2 { nm.clone() } else { format!("{nm} copy {k}") };
                        let mut j = json!({"type": "pattern_circular", "name": name, "source_id": fid,
                            "features": in_names, "target": target,
                            "axis": {"origin": v3mm(*pt), "direction": [round9(dir[0]), round9(dir[1]), round9(dir[2])]},
                            "angle": turn, "count": 2,
                            "note": format!("copy {k} of {n}; the pattern leaves out copies {skips:?}")});
                        unmerged(&mut j);
                        feats.push(j);
                    }
                    tag_feats(&mut feats, nbefore, comp.map(|c| (c, comp_name.as_deref())), extra.contains(&fid));
                    continue;
                }
                let mut j = json!({"type": "pattern_circular", "name": nm, "source_id": fid,
                    "features": in_names, "target": target,
                    "axis": {"origin": v3mm(*pt), "direction": [round9(dir[0]), round9(dir[1]), round9(dir[2])]},
                    "angle": 360.0, "count": count.round() as i64,
                    "note": "axis = the stored F2A7590D point and vector"});
                unmerged(&mut j);
                feats.push(j);
                tag_feats(&mut feats, nbefore, comp.map(|c| (c, comp_name.as_deref())), extra.contains(&fid));
                continue;
            }
            // A pattern of the occurrences of a component: the copies are occurrences the file
            // places, so the component's features are written once per occurrence (`place`).
            let mut note =
                "pattern of components, of several bodies, with a partial angle, or no direction record: not converted"
                    .to_string();
            if ins.is_empty() && body_in.is_empty() {
                match super::occpattern::copies_of(d, fid) {
                    Ok(_) => {
                        skipped.push(json!({"name": name, "fusion_type": fusion_type, "source_id": fid,
                            "reason": "pattern of components: its copies are occurrences of the component, placed like its other occurrences"}));
                        continue;
                    }
                    Err(why) if linear && why != "the pattern names no occurrence" => {
                        note = format!("pattern of components: {why}");
                    }
                    Err(_) => {}
                }
            }
            let mut j = json!({"type": "unsupported", "gtree_type": if linear { "pattern_linear" } else { "pattern_circular" },
                "fusion_type": fusion_type, "name": nm, "source_id": fid, "features": in_names, "note": note});
            if let (Some(o), Some((_, dir))) = (j.as_object_mut(), dirs.first()) {
                o.insert("direction".into(), json!([round9(dir[0]), round9(dir[1]), round9(dir[2])]));
            }
            feats.push(j);
        } else if c8 == "65CA3206" {
            let ed = super::bodyops::Edit { d, g: &g, names: &names, feats: &feats };
            feats.push(split_json(cx, &ed, fid, &nm, &fusion_type, class));
        } else if c8 == "584D9526" {
            // The body its face is on, by the feature that made it (as a blend's).
            let on = one_body(g.body_links(d, fid, 2).iter().filter_map(|l| g.creator.get(l).copied()), fid, &names);
            let body = on.as_deref();
            match super::thread::json(cx, fid, &nm, &fusion_type, class, body) {
                Ok(super::thread::Word::Cosmetic(note)) => {
                    skipped.push(json!({"name": name, "fusion_type": fusion_type, "reason": note, "source_id": fid}));
                    continue;
                }
                Ok(super::thread::Word::Features(fs)) => feats.extend(fs),
                Err(j) => feats.push(j),
            }
        } else if c8 == "E8063467" {
            // A selection of a body (`9716F783`) and no face: the shell of the whole body.
            let whole = cx.input_recipes(fid).is_empty() && !g.selected_bodies(d, fid).is_empty();
            if whole {
                // A shell of a whole body (no face removed): the body with a closed void.
                let ed = super::bodyops::Edit { d, g: &g, names: &names, feats: &feats };
                feats.push(match ed.target(fid) {
                    Ok(target) => shell_json(cx, fid, &nm, &fusion_type, class, Some(&target)),
                    Err(e) => json!({"type": "unsupported", "fusion_type": fusion_type, "name": nm,
                        "class_guid": class, "source_id": fid, "error": format!("a shell of a whole body: {e}")}),
                });
            } else {
                feats.push(shell_json(cx, fid, &nm, &fusion_type, class, None));
            }
        } else if c8 == "D087EFE5" {
            let on = one_body(g.body_links(d, fid, 2).iter().filter_map(|l| g.creator.get(l).copied()), fid, &names);
            let mut j = offset_faces_json(cx, fid, &nm, &fusion_type, class);
            if let (Some(o), Some(b)) = (j.as_object_mut(), on)
                && o.get("type").and_then(Value::as_str) == Some("offset_face")
            {
                o.insert("on_body".into(), b.into());
            }
            feats.push(j);
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
        } else if c8 == "2A94257F" {
            let ed = super::bodyops::Edit { d, g: &g, names: &names, feats: &feats };
            match ed.combine(fid, &nm) {
                super::combine::Combined::Feature(j) => feats.push(j),
                super::combine::Combined::Refused(why) => {
                    feats.push(json!({"type": "unsupported", "fusion_type": fusion_type, "name": nm,
                        "class_guid": class, "source_id": fid, "error": why}));
                }
            }
        } else if c8 == "428B5C6E" {
            feats.push(insert_json(d, fid, &nm, &fusion_type, class));
        } else if matches!(c8.as_str(), super::pcb::BOX | super::pcb::LAYER | super::pcb::COPPER) {
            let read = match c8.as_str() {
                super::pcb::BOX => cx.box_primitive(&g, fid, &nm).map(Some),
                super::pcb::COPPER => cx.copper(&g, fid, &nm, &mut used_names).map(Some),
                _ => cx.layer(&g, fid, &nm, &names, &feats, &mut used_names),
            };
            match read {
                Ok(Some(made)) => feats.extend(made),
                Ok(None) => {
                    skipped.push(json!({"name": name, "fusion_type": fusion_type, "reason": "a drill without holes", "source_id": fid}));
                    continue;
                }
                Err(why) => feats.push(json!({"type": "unsupported", "fusion_type": fusion_type, "name": nm,
                    "class_guid": class, "source_id": fid, "error": why})),
            }
        } else if c8 == "D7015B81" || c8 == DERIVED_BASE {
            // Base Feature: its visible bodies are bodies the file stores (section 13), handed
            // to the importer as `stored_body` features.
            match base_feature_bodies(d, &g, fid, cx.acis) {
                Ok(bodies) if bodies.is_empty() => {
                    skipped.push(json!({"name": name, "fusion_type": fusion_type, "reason": "Base Feature that makes no body", "source_id": fid}));
                    continue;
                }
                Ok(bodies) => {
                    let n = bodies.len();
                    let inputs = base_feature_inputs(d, &g, fid);
                    for (k, (brep, index, count)) in bodies.into_iter().enumerate() {
                        let mut bn = if k == 0 { nm.clone() } else { format!("{nm} {}", k + 1) };
                        while k > 0 && used_names.contains(&bn) {
                            bn.push('_');
                        }
                        used_names.insert(bn.clone());
                        let mut f = json!({"type": "stored_body", "name": bn, "source_id": fid, "brep": brep,
                            "index": index, "count": count, "creates_bodies": n});
                        if let (Some(inputs), Some(o)) = (&inputs, f.as_object_mut()) {
                            o.insert("dropped_inputs".into(), inputs.clone());
                        }
                        feats.push(f);
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
    joins_that_make_bodies(&mut feats);
    joins_of_several_bodies(&mut feats);
    super::thread::moved_edges(&mut feats);
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

/// A join whose tool has parts that touch no body of its target makes those parts new bodies in
/// the file (610: Extrude5 joins to Extrude1's body and makes two bodies; Extrude6 then joins to
/// the bodies of Extrude1 and Extrude5). The tree's join leaves them in the target's body, as
/// solids of their own: a later join or cut that names the target's body and those bodies names
/// one body here. Only lists that name the target's own body are merged; any other is left as it
/// is (refused).
pub(super) fn joins_that_make_bodies(feats: &mut [Value]) {
    let mut lives_in: HashMap<String, String> = HashMap::new();
    for f in feats.iter_mut() {
        let Some(o) = f.as_object_mut() else { continue };
        if let Some(Value::Array(a)) = o.get("target") {
            let named: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            let mut to: Vec<&str> = named.iter().map(|n| lives_in.get(*n).map_or(*n, String::as_str)).collect();
            to.sort_unstable();
            to.dedup();
            if named.len() == a.len()
                && let [one] = to.as_slice()
                && named.contains(one)
            {
                let one = (*one).to_string();
                o.insert("target".into(), one.into());
            }
        }
        let joins_new = matches!(o.get("type").and_then(Value::as_str), Some("extrude" | "sweep" | "revolve"))
            && o.get("operation").and_then(Value::as_str) == Some("join")
            && o.get("creates_bodies").and_then(Value::as_u64).is_some_and(|n| n > 0);
        if joins_new
            && let (Some(name), Some(t)) =
                (o.get("name").and_then(Value::as_str), o.get("target").and_then(Value::as_str))
        {
            let t = lives_in.get(t).cloned().unwrap_or_else(|| t.to_string());
            lives_in.insert(name.to_string(), t);
        }
    }
}

/// An extrude or sweep that joins its tool to several bodies and makes none leaves one body:
/// the tree's feature joins the tool to one of them and a `combine` after it joins the others
/// to that one. The body that lives on is the one a later feature names (a join of the bodies of
/// Extrude1 and Extrude9 followed by a join to Extrude9's keeps Extrude9's), the first where none
/// does; where later features name two of them the feature stays as it is (refused).
/// The features of a MirrorComponent: one body mirror (into a new body) per body its source
/// component has, in the new component (section 33). Also lists the new component.
type NewComp = (u64, Option<String>);
fn mirror_component_json(
    cx: &Ctx,
    comps: &mut Vec<(u64, Option<String>, Vec<String>)>,
    feats: &[Value],
    fid: u64,
    nm: &str,
) -> Result<(Vec<Value>, NewComp), String> {
    let d = cx.d;
    let m = mirrorcomp::read(cx, fid)?;
    let bodies = mirrorcomp::component_bodies_so_far(feats, m.source)?;
    if bodies.is_empty() {
        return Err("the mirrored component has no body the tree built".into());
    }
    let id = Placement::IDENTITY;
    let plane = mirrorcomp::reflection_plane(&mirrorcomp::body_map(&m, &id, &id))
        .ok_or("the mirrored bodies are not a reflection of the source's in the new component")?;
    if !comps.iter().any(|x| x.0 == m.new) {
        comps.push((m.new, component_name(d, m.new), component_bodies(d, m.new)));
    }
    let keys = m.keys_json();
    let made = bodies
        .iter()
        .enumerate()
        .map(|(k, b)| {
            let name = if k == 0 { nm.to_string() } else { format!("{nm} body {}", k + 1) };
            json!({"type": "mirror", "name": name, "source_id": fid, "features": [], "target": b,
                "merge": false, "creates_bodies": 1, "plane": mirrorcomp::plane_json(plane),
                "_mirror_component": keys.clone()})
        })
        .collect();
    Ok((made, (m.new, component_name(d, m.new))))
}

pub(super) fn joins_of_several_bodies(feats: &mut Vec<Value>) {
    let mut i = 0;
    while i < feats.len() {
        let Some(f) = feats.get(i) else { break };
        let targets: Vec<String> = f
            .get("target")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let joins = matches!(f.get("type").and_then(Value::as_str), Some("extrude" | "sweep"))
            && f.get("operation").and_then(Value::as_str) == Some("join")
            && f.get("creates_bodies").and_then(Value::as_u64) == Some(0)
            && targets.len() >= 2
            && f.get("target").and_then(Value::as_array).is_some_and(|a| a.len() == targets.len());
        if !joins {
            i += 1;
            continue;
        }
        // The bodies later features name, as a target or a tool.
        let mut later: Vec<&String> = Vec::new();
        for g in feats.iter().skip(i + 1) {
            for key in ["target", "tools"] {
                let named: Vec<&str> = match g.get(key) {
                    Some(Value::String(t)) => vec![t.as_str()],
                    Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
                    _ => Vec::new(),
                };
                for t in &targets {
                    if named.contains(&t.as_str()) && !later.contains(&t) {
                        later.push(t);
                    }
                }
            }
        }
        let keep = match later.as_slice() {
            [] => targets.first().cloned(),
            [one] => Some((*one).clone()),
            _ => None,
        };
        let Some(keep) = keep else {
            i += 1;
            continue;
        };
        let tools: Vec<String> = targets.iter().filter(|t| **t != keep).cloned().collect();
        let Some(Value::Object(o)) = feats.get_mut(i) else { break };
        o.insert("target".into(), keep.as_str().into());
        let name = o.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut c = Map::new();
        c.insert("type".into(), "combine".into());
        c.insert("name".into(), format!("{name} bodies").into());
        c.insert("target".into(), keep.into());
        c.insert("tools".into(), tools.into());
        c.insert("operation".into(), "join".into());
        c.insert("note".into(), format!("{name} joins its tool to several bodies: they are one body after it").into());
        for key in ["source_id", "component", "_component", "in_timeline"] {
            if let Some(v) = o.get(key) {
                c.insert(key.into(), v.clone());
            }
        }
        feats.insert(i + 1, Value::Object(c));
        i += 2;
    }
}

/// The one body, by the feature that made it, of the creators `of` a feature's body links
/// (`fid` itself left out); `None` for none or several.
fn one_body(of: impl Iterator<Item = u64>, fid: u64, names: &HashMap<u64, String>) -> Option<String> {
    let mut on: Vec<String> = of.filter(|c| *c != fid).filter_map(|c| names.get(&c).cloned()).collect();
    on.sort();
    on.dedup();
    if let [one] = on.as_slice() { Some(one.clone()) } else { None }
}

/// OffsetFaces `D087EFE5`: the faces are `7ACC2A03` face recipes under its selection (planar
/// caps and sides of extrudes, resolved like a shell's), the offset its `distance` parameter,
/// positive away from the material (3464 OffsetFaces3: -1.437 mm takes material off; the rebuild
/// of that design against its stored bodies confirms the sign).
fn offset_faces_json(cx: &mut Ctx, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
    let d = cx.d;
    let fail = |why: &str| {
        json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "error": why})
    };
    let values = feature::param_values(d, fid, &["distance"]);
    let [distance] = values.as_slice() else {
        return fail("the offset has no single distance");
    };
    if !distance.is_finite() || *distance == 0.0 {
        return fail("the offset distance is zero");
    }
    let mut faces: Vec<Value> = Vec::new();
    for rid in cx.input_recipes(fid) {
        let (kind, names) = naming::recipe(d, rid);
        let (Some(naming::RecipeKind::Face | naming::RecipeKind::BoundedFace), Some(names)) = (kind, names) else {
            return fail("an offset face is not a face recipe");
        };
        let Some((g, ops)) = names.first().and_then(|f| f.first()) else {
            return fail("an offset face has no name");
        };
        match cx.face_with_middle(g, ops) {
            // A side's middle; a cap's point is its plane's (the importer takes the nearest face
            // of that plane).
            Some(naming::FaceG::Plane { mid, .. }) => faces.push(json!({"point": v3mm(mid)})),
            // Any other face: by its surface and points (an arc side, a revolve's or a
            // blend's face; section 28 of the layout notes).
            _ => match cx.general_face(g, ops) {
                Some(pick) => faces.push(pick.json()),
                None => return fail("an offset face is neither modelled nor on one stored surface"),
            },
        }
    }
    if faces.is_empty() {
        return fail("no offset face was found");
    }
    json!({"type": "offset_face", "name": name, "source_id": fid, "faces": faces, "distance": mm(*distance)})
}

/// Split Body `65CA3206`: the body it splits (a `2CA5A1CD` -> `9716F783` selection), the
/// splitting tool (a `2CA5A1CD` -> `5A1BF548` -> `CC54ECAD` construction plane, or a face) and the
/// body links it makes (`D26351F0` whose creator is the split): one, the new body (the body
/// selected keeps its own link). A split by a construction plane into two bodies is read; one
/// into more, or by a face, is not.
fn split_json(cx: &Ctx, ed: &super::bodyops::Edit<'_>, fid: u64, name: &str, fusion_type: &str, class: &str) -> Value {
    let d = cx.d;
    let fail = |why: String| {
        json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "error": why})
    };
    let made = ed.g.body_links(d, fid, 2).iter().filter(|l| ed.g.creator.get(l) == Some(&fid)).count();
    if made != 1 {
        return fail(format!("the split makes {made} new bodies, not one (a split into two bodies is read)"));
    }
    let target = match ed.target(fid) {
        Ok(t) => t,
        Err(e) => return fail(format!("the body it splits: {e}")),
    };
    let tools: Vec<u64> = ref_ids(d, d.rec(fid))
        .into_iter()
        .filter(|q| d.is(*q, "2CA5A1CD"))
        .flat_map(|q| ref_ids(d, d.rec(q)))
        .filter(|q| d.is(*q, "5A1BF548") || d.is(*q, "5662F619"))
        .collect::<HashSet<u64>>()
        .into_iter()
        .collect();
    let [tool] = tools.as_slice() else { return fail(format!("it names {} splitting tools, not one", tools.len())) };
    if !d.is(*tool, "5A1BF548") {
        return fail("its splitting tool is a face, not a construction plane".into());
    }
    let frames: Vec<(V3, V3)> = ref_ids(d, d.rec(*tool))
        .into_iter()
        .filter(|q| d.is(*q, "CC54ECAD"))
        .filter_map(|q| feature::plane_frame(d, q))
        .collect();
    let [(o, n)] = frames.as_slice() else { return fail("its splitting plane is not one construction plane".into()) };
    json!({"type": "split", "name": name, "source_id": fid, "target": target, "creates_bodies": 1,
        "plane": {"origin": v3mm(*o), "normal": [round9(n[0]), round9(n[1]), round9(n[2])]}})
}

/// The plane of a mirror whose plane holder (`C444E6AD`) selects a face (`2CA5A1CD` ->
/// `5662F619` -> a `7ACC2A03` face recipe) instead of a construction plane: a planar face's
/// point and normal (cm). `None` where the holder selects no face or the face is not planar.
/// The plane of a mirror (point, unit normal; cm): a planar face it selects, else a
/// construction plane.
pub(super) fn mirror_plane(cx: &mut Ctx, fid: u64) -> Option<(V3, V3)> {
    let d = cx.d;
    mirror_face_plane(cx, fid)
        .or_else(|| cx.under(fid, "CC54ECAD", 4, None).into_iter().find_map(|q| feature::plane_frame(d, q)))
}

fn mirror_face_plane(cx: &mut Ctx, fid: u64) -> Option<(V3, V3)> {
    let d = cx.d;
    let holders: Vec<u64> = ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "C444E6AD")).collect();
    let mut recipes: Vec<u64> = Vec::new();
    for h in holders {
        for r in cx.under(h, "7ACC2A03", 3, Some(&["2CA5A1CD", "5662F619"])) {
            if !recipes.contains(&r) {
                recipes.push(r);
            }
        }
    }
    let [rid] = recipes.as_slice() else { return None };
    let (kind, names) = naming::recipe(d, *rid);
    if !matches!(kind, Some(naming::RecipeKind::Face | naming::RecipeKind::BoundedFace)) {
        return None;
    }
    let (g, ops) = names.as_ref()?.first()?.first()?;
    match cx.face(g, ops)? {
        naming::FaceG::Plane { n, p, .. } => Some((p, n)),
        _ => None,
    }
}

/// Shell `E8063467`: the faces it removes are `7ACC2A03` face recipes under its selection records
/// (a cap or side of an extrude, resolved like the edges of a fillet); the wall is the
/// `innerThickness` parameter (walls grow inward) or, with no inner value, `outerThickness`.
/// With `body` (a selection of one whole body, no face removed) it is a closed hollow of that body.
fn shell_json(cx: &mut Ctx, fid: u64, name: &str, fusion_type: &str, class: &str, body: Option<&str>) -> Value {
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
    if let Some(body) = body {
        return json!({"type": "shell", "name": name, "source_id": fid, "body": body, "faces": [],
            "thickness": mm(t), "side": side});
    }
    if let Some(body) = body {
        return json!({"type": "shell", "name": name, "source_id": fid, "body": body, "faces": [],
            "thickness": mm(t), "side": side});
    }
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
        match cx.face_with_middle(g, ops) {
            // A cap's modelled point is its plane's (the sketch origin's projection), which may
            // lie on another body's face of that plane (512: Extrude10's bottom on the board's
            // top at z 1.6): the stored cap's points pick the face, as for a draft.
            Some(naming::FaceG::Plane { cap: true, .. })
                if (g == "1" || g == "2")
                    && ops.len() == 1
                    && let Some(sp) = cx.stored_face(g, ops) =>
            {
                faces.push(sp.json());
            }
            // No normal: the cap and side normals the names give are not oriented outward. A
            // point of the face itself (`mid`), not any point of its plane: a side's corner lies
            // on the faces around it too, and on the bodies it touches.
            Some(naming::FaceG::Plane { mid, .. }) => faces.push(json!({"point": v3mm(mid)})),
            _ => match cx.general_face(g, ops) {
                Some(pick) => faces.push(pick.json()),
                None => return fail("a removed face is neither modelled nor on one stored surface"),
            },
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
    cover::field(rec, end, 3, "insert.flags");
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
        cover::field(rec, end + 3, 96, "insert.placement");
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
        "insert_keys": keys, "_occurrence": cx,
        "placement": {"r": [rr(r[0]), rr(r[1]), rr(r[2])], "t": [round6(t[0]), round6(t[1]), round6(t[2])]}})
}

/// The bodies a Base Feature makes: per visible (DM) body, the history blob its state list names
/// (`BREP.<id>.smbh`), the body's place in that blob and the number of bodies the blob holds
/// (the DM bodies are its last ones, section 13). Empty for a base feature that makes no body.
/// Where a DM body's list is a later state of the component (4431: a Combine that follows took
/// one DM body out of it, or later extrudes added bodies), the bodies are the PM bodies they
/// stand for, taken from the PM blob ([`base_feature_pm_bodies`]).
fn base_feature_bodies(
    d: &Design,
    g: &Graph,
    fid: u64,
    acis: &[(&str, &[u8])],
) -> Result<Vec<(String, usize, usize)>, String> {
    let mut out = Vec::new();
    let mut dms = Vec::new();
    let mut later = false;
    for link in ref_ids(d, d.rec(fid)) {
        if !d.is(link, "D26351F0") || g.creator.get(&link) != Some(&fid) {
            continue;
        }
        let Some(dm) = ref_ids(d, d.rec(link)).into_iter().find(|q| d.is(*q, "D3937028")) else {
            return Err("a Base Feature body link names no body".into());
        };
        dms.push(dm);
        // The state list of the body that names a history blob.
        let Some((name, members)) = state_list(d, dm, ".smbh") else {
            return Err("a Base Feature body names no history blob".into());
        };
        match members.iter().position(|q| *q == dm) {
            Some(at) => out.push((name, at, members.len())),
            None => later = true,
        }
    }
    if !later && out.iter().all(|(_, at, count)| *at + out.len() >= *count) {
        // The visible bodies are the last of the list (section 13).
        return Ok(out);
    }
    base_feature_pm_bodies(d, &dms, acis).ok_or_else(|| {
        if later {
            "a Base Feature body is not in its own state list".into()
        } else {
            "a Base Feature body is not among the last bodies of its state list".into()
        }
    })
}

/// The state list `CD57BC48` of body `body` that names a blob ending in `ext` (`.smb` or
/// `.smbh`), and the bodies it lists (the record lists them twice, as two reference lists).
fn state_list(d: &Design, body: u64, ext: &str) -> Option<(String, Vec<u64>)> {
    for list in ref_ids(d, d.rec(body)).into_iter().filter(|q| d.is(*q, "CD57BC48")) {
        let members = ref_ids(d, d.rec(list));
        let name = members.iter().find_map(|q| {
            stream::wstrings_all(d.rec(*q))
                .into_iter()
                .map(|w| w.2)
                .find(|w| w.starts_with("BREP.") && w.ends_with(ext))
        });
        if let Some(name) = name {
            let mut bodies: Vec<u64> = Vec::new();
            for q in members.into_iter().filter(|q| d.is(*q, "D3937028")) {
                if !bodies.contains(&q) {
                    bodies.push(q);
                }
            }
            return Some((name, bodies));
        }
    }
    None
}

/// The DM bodies `dms` of a Base Feature as their PM bodies (the input bodies they stand for:
/// each names the other, section 13), from the PM bodies' blob (`BREP.<id>.smb`). That blob
/// keeps the component's bodies of earlier states too (4431: the switch's 1 PM body among 11
/// solids and 17 sheets; the connector's 5 among 6 solids, the first a copy of the second): the
/// PM list's bodies are, in list order, its last solid bodies, as the DM bodies are the last of a
/// history blob. `None` where a DM body has no PM body of one list, or the blob has fewer solids.
/// The importer's checks hold the bodies to every later stored state.
fn base_feature_pm_bodies(d: &Design, dms: &[u64], acis: &[(&str, &[u8])]) -> Option<Vec<(String, usize, usize)>> {
    let mut pms = Vec::with_capacity(dms.len());
    for dm in dms {
        let pm = ref_ids(d, d.rec(*dm))
            .into_iter()
            .find(|q| d.is(*q, "D3937028") && ref_ids(d, d.rec(*q)).first() == Some(dm))?;
        pms.push(pm);
    }
    let (name, members) = state_list(d, *pms.first()?, ".smb")?;
    if pms.iter().any(|pm| state_list(d, *pm, ".smb").is_none_or(|l| l.0 != name || l.1 != members)) {
        return None;
    }
    let blob = acis.iter().find(|(n, _)| n.rsplit('/').next() == Some(name.as_str()))?.1;
    let (solids, count) = crate::solid_bodies(blob, &crate::Limits::default()).ok()?;
    let first = solids.len().checked_sub(members.len())?;
    pms.iter()
        .map(|pm| {
            let at = members.iter().position(|q| q == pm)?;
            Some((name.clone(), *solids.get(first + at)?, count))
        })
        .collect()
}

/// The input (PM) bodies of a Base Feature that it does not hand on, as `{"brep": <the PM
/// bodies' blob, BREP.<id>.smb>, "count": <PM bodies>, "kept": [<place in the PM list of the PM
/// body of each DM body, in DM order>]}`; `None` where it hands on all of them or the records do
/// not say it plainly. The record names its PM bodies, then its DM bodies, then again the PM
/// bodies that become the DM bodies, in DM order (section 13): 5021's slide potentiometer has 15
/// PM bodies and 6 DM bodies, and the 9 PM bodies named once (three 0.028 mm3 blocks, pins and a
/// housing at other places) are in the component's `.smb` but in no later body.
pub(super) fn base_feature_inputs(d: &Design, g: &Graph, fid: u64) -> Option<Value> {
    let refs: Vec<u64> = ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "D3937028")).collect();
    let mut dm: Vec<u64> = Vec::new();
    for link in ref_ids(d, d.rec(fid)) {
        if d.is(link, "D26351F0") && g.creator.get(&link) == Some(&fid) {
            dm.push(ref_ids(d, d.rec(link)).into_iter().find(|q| d.is(*q, "D3937028"))?);
        }
    }
    // PM bodies, the DM block, then the kept PM bodies again.
    let first_dm = refs.iter().position(|q| dm.contains(q))?;
    let (pm, rest) = refs.split_at(first_dm);
    let (dm_block, again) = rest.split_at(rest.iter().position(|q| !dm.contains(q)).unwrap_or(rest.len()));
    if dm_block != dm.as_slice() || again.len() != dm.len() || pm.len() <= dm.len() || pm.len() > 4096 {
        return None;
    }
    let mut kept = Vec::with_capacity(again.len());
    for q in again {
        let at = pm.iter().position(|p| p == q)?;
        if kept.contains(&at) {
            return None;
        }
        kept.push(at);
    }
    // Every PM body is in one state list, and that list names the PM blob.
    let list_of = |b: u64| -> Option<u64> {
        let lists: Vec<u64> = ref_ids(d, d.rec(b)).into_iter().filter(|q| d.is(*q, "CD57BC48")).collect();
        let first = *lists.first()?;
        lists.iter().all(|l| *l == first).then_some(first)
    };
    let list = list_of(*pm.first()?)?;
    if pm.iter().any(|b| list_of(*b) != Some(list)) {
        return None;
    }
    let members = ref_ids(d, d.rec(list));
    let names: Vec<String> = members
        .iter()
        .filter(|q| d.is(**q, "40DCC3F4"))
        .flat_map(|q| stream::wstrings_all(d.rec(*q)).into_iter().map(|w| w.2))
        .filter(|w| w.starts_with("BREP.") && w.ends_with(".smb"))
        .collect();
    let [brep] = names.as_slice() else { return None };
    // The list names its bodies (twice, as two reference lists): the PM bodies and no others.
    let mut listed: Vec<u64> = Vec::new();
    for q in members.into_iter().filter(|q| d.is(*q, "D3937028")) {
        if !listed.contains(&q) {
            listed.push(q);
        }
    }
    if listed != pm {
        return None;
    }
    Some(json!({"brep": brep, "count": pm.len(), "kept": kept}))
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
            if let Some(why) = text("start_error") {
                return Some(why.to_string());
            }
            if has("start") {
                return Some("it starts off its sketch plane".into());
            }
            if f.get("two_sided").and_then(Value::as_bool) == Some(true) {
                return Some("it extrudes to both sides by different distances".into());
            }
            if has("taper_deg") && f.get("symmetric").and_then(Value::as_bool) == Some(true) {
                return Some("it is tapered and symmetric".into());
            }
            let op = text("operation").unwrap_or("");
            if !matches!(op, "new" | "join" | "cut" | "intersect") {
                return Some(format!("its operation is {op}"));
            }
            // A cut may name several bodies: the tree cuts each (858's Extrude6).
            let several_cut = op == "cut"
                && f.get("target").and_then(Value::as_array).is_some_and(|a| a.iter().all(Value::is_string));
            if matches!(op, "join" | "cut" | "intersect")
                && !f.get("target").is_some_and(Value::is_string)
                && !several_cut
            {
                return Some("the body it changes is not one body of one feature".into());
            }
            None
        }
        "fillet" | "chamfer" => {
            if has("error") {
                return Some(text("error").unwrap_or("its layout is not recognised").to_string());
            }
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
        "revolve" => {
            if has("error") {
                return Some(text("error").unwrap_or("its layout is not recognised").to_string());
            }
            if !f.get("profiles").is_some_and(Value::is_array) || !has("axis") || !has("angle") {
                return Some("its profile, axis or angle is not decoded".into());
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
        "sweep" => {
            if has("error") {
                return Some(text("error").unwrap_or("its layout is not recognised").to_string());
            }
            if !has("sketch") || !(has("path") || has("helix")) {
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
        "hole" | "mirror" | "shell" | "draft" | "offset_face" | "move_body" | "scale_body" | "remove_body"
        | "split" | "combine" => None,
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
    // A mirrored component's bodies: the reflection of its source's, each where the file places
    // its bodies (section 33; the parent's own placement cancels out). Its plane is not moved
    // again below.
    for f in feats.iter_mut() {
        let Some(m) = f.get("_mirror_component").and_then(mirrorcomp::keys_of) else { continue };
        let one = |c: u64| match by_comp.get(&c).map(Vec::as_slice) {
            None => Ok(crate::fusion::Placement::IDENTITY),
            Some([p]) => Ok(*p),
            Some(_) => Err("a mirrored component or its source stands in several places".to_string()),
        };
        let map = mirrorcomp::body_map(&m, &one(m.source)?, &one(m.new)?);
        let plane = mirrorcomp::reflection_plane(&map)
            .ok_or("the mirrored component's bodies are not a reflection of its source's where the file places them")?;
        if let Some(o) = f.as_object_mut() {
            o.insert("plane".into(), mirrorcomp::plane_json(plane));
            o.remove("_component");
        }
    }
    for f in feats.iter_mut() {
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
    // An extrude whose direction is given in the assembly: it goes along its distance where the
    // occurrence turns the sketch normal onto that direction, the other way where it turns it
    // against it; one that stays across it in the assembly too is refused.
    for f in feats.iter_mut() {
        let Some(a) = f.get("_assembly_direction") else { continue };
        let (Some(n), Some(v)) = (get3(a.get("normal")), get3(a.get("direction"))) else { continue };
        let inst = f.get("_instance").and_then(Value::as_u64).unwrap_or(0) as usize;
        let pl = f
            .get("_component")
            .and_then(Value::as_u64)
            .and_then(|c| by_comp.get(&c))
            .and_then(|v| v.get(inst))
            .copied()
            .unwrap_or(crate::fusion::Placement::IDENTITY);
        let along = dot(xf_dir(&pl, n), v);
        if (along.abs() - 1.0).abs() > 1e-6 {
            let name = f.get("name").and_then(Value::as_str).unwrap_or("?").to_string();
            return Err(format!("the direction of {name} lies across its sketch's normal in the assembly too"));
        }
        if along < 0.0
            && let Some(o) = f.as_object_mut()
            && let Some(d) = o.get("distance").and_then(Value::as_f64)
        {
            o.insert("distance".into(), (-d).into());
        }
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
/// Moves a face pick: its point and normal, and a stored pick's points and surface.
fn move_pick(fc: &mut Map<String, Value>, pl: &crate::fusion::Placement) {
    let pt = |v: Option<&Value>| get3(v).map(|p| v3mm_raw(xf_point(pl, p)));
    let dir = |v: Option<&Value>| get3(v).map(|p| v3_9(xf_dir(pl, p)));
    let (p, n) = (pt(fc.get("point")), dir(fc.get("normal")));
    for (k, v) in [("point", p), ("normal", n)] {
        if let Some(v) = v {
            fc.insert(k.into(), v);
        }
    }
    if let Some(ps) = fc.get_mut("points").and_then(Value::as_array_mut) {
        for q in ps.iter_mut() {
            if let Some(m) = pt(Some(q)) {
                *q = m;
            }
        }
    }
    if let Some(sf) = fc.get_mut("surface").and_then(Value::as_object_mut) {
        let (o, n, a) = (pt(sf.get("origin")), dir(sf.get("normal")), dir(sf.get("axis")));
        for (k, v) in [("origin", o), ("normal", n), ("axis", a)] {
            if let Some(v) = v {
                sf.insert(k.into(), v);
            }
        }
    }
}

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
            // A chamfer's reference face (two distances).
            if let Some(fc) = f.get_mut("face").and_then(Value::as_object_mut) {
                move_pick(fc, &pl);
            }
            if let Some(es) = f.get_mut("edges").and_then(Value::as_array_mut) {
                for e in es {
                    if let Some(m) = pt(e.get("midpoint"))
                        && let Some(o) = e.as_object_mut()
                    {
                        o.insert("midpoint".into(), m);
                    }
                    // The points of a curved edge go with its middle.
                    if let Some(path) = e.get_mut("path").and_then(Value::as_array_mut) {
                        for q in path.iter_mut() {
                            if let Some(m) = pt(Some(q)) {
                                *q = m;
                            }
                        }
                    }
                }
            }
        }
        "shell" | "offset_face" => {
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
        "mirror" | "split" => {
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
            } else if let Some((base, copy)) = s.rsplit_once('[')
                && copy.strip_suffix(']').is_some_and(|i| !i.is_empty() && i.bytes().all(|c| c.is_ascii_digit()))
                && let Some(n) = map.get(base)
            {
                // A copy of a body pattern (`<pattern>[i]`) follows its pattern's name.
                *s = format!("{n}[{copy}");
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
