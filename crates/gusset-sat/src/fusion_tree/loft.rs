//! Loft (`B6678572`) and Pipe (`73615646`) features (section 48 of
//! `docs/formats/fusion-design-stream.md`).
//!
//! A loft names its sections as a list of `2F3200BA` records, each with a profile selection
//! (`2CA5A1CD` -> `4BD53E5A`, a sketch profile as an extrude's). It becomes a tree `loft` of
//! copies of the sections' sketches that hold just the profile's curves.
//!
//! A pipe is a circle or a square of the `SectionSize` across swept along a path of sketch curves
//! (the sweep's `5A1BF548` -> `E2CEFD18` references): a section sketch across the path's start
//! and a tree `sweep`.

use super::Ctx;
use super::geom::{self, V3, cross, mm, round9, unit};
use super::naming;
use super::sketch;
use super::stream::{self, ref_ids, ref_lists};
use crate::cover;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

/// The loft feature class.
pub(super) const LOFT: &str = "B6678572";
/// The pipe feature class.
pub(super) const PIPE: &str = "73615646";
/// One section of a loft.
const SECTION: &str = "2F3200BA";
/// Most sections one loft may name.
const MAX_SECTIONS: usize = 64;

/// A copy of the tree sketch `sketch` (read from sketch `sid`, in `feats`) that holds only the
/// curves of the one region the profile selection `sel` names, as `copy`. An arc drawn over a
/// circle of the region (5998's `Sketch5 (2)`: the end of a U and a circle on it) is left out.
fn profile_sketch(
    cx: &mut Ctx,
    feats: &[Value],
    sel: u64,
    sid: u64,
    sketch: &str,
    copy: &str,
) -> Result<(Value, [f64; 2]), String> {
    let d = cx.d;
    let sk = cx.sketch(sid);
    let regions = cx.rc.regions(d, sel, &sk)?;
    let [region] = regions.as_slice() else {
        return Err(format!("a section of {} profile regions, not one", regions.len()));
    };
    let used: HashSet<String> = region.segments.iter().map(|sg| format!("c{}", sg.curve)).collect();
    let found = feats.iter().rev().find(|f| {
        f.get("type").and_then(Value::as_str) == Some("sketch") && f.get("name").and_then(Value::as_str) == Some(sketch)
    });
    let Some(Value::Object(mut c)) = found.cloned() else { return Err("a section's sketch is not in the tree".into()) };
    let point = |id: Option<&str>| -> Option<[f64; 2]> {
        let ps = c.get("points")?.as_array()?;
        let p = ps.iter().find(|p| p.get("id").and_then(Value::as_str) == id)?;
        Some([p.get("x")?.as_f64()?, p.get("y")?.as_f64()?])
    };
    // (centre, radius, whole) of every circle and arc of the region, by curve id.
    let mut rounds: HashMap<String, ([f64; 2], f64, bool)> = HashMap::new();
    for x in c.get("curves").and_then(Value::as_array).into_iter().flatten() {
        let Some(id) = x.get("id").and_then(Value::as_str).filter(|i| used.contains(*i)) else { continue };
        let text = |k: &str| x.get(k).and_then(Value::as_str);
        let Some(o) = point(text("center")) else { continue };
        match text("type") {
            Some("circle") => {
                if let Some(r) = x.get("radius").and_then(Value::as_f64) {
                    rounds.insert(id.to_string(), (o, r, true));
                }
            }
            Some("arc") => {
                if let Some(p) = point(text("start")) {
                    rounds.insert(id.to_string(), (o, libm::hypot(p[0] - o[0], p[1] - o[1]), false));
                }
            }
            _ => {}
        }
    }
    let on_a_circle = |id: &str| {
        rounds.get(id).is_some_and(|(o, r, full)| {
            !full
                && rounds.values().any(|(o2, r2, full2)| {
                    *full2 && libm::hypot(o[0] - o2[0], o[1] - o2[1]) < 1e-6 && (r - r2).abs() < 1e-6
                })
        })
    };
    if let Some(cs) = c.get_mut("curves").and_then(Value::as_array_mut) {
        cs.retain(|x| x.get("id").and_then(Value::as_str).is_some_and(|i| used.contains(i) && !on_a_circle(i)));
        let dropped = used.iter().filter(|i| on_a_circle(i)).count();
        if cs.len() + dropped != used.len() {
            return Err("a section's curves are not all in its sketch".into());
        }
    }
    c.insert("name".into(), copy.into());
    c.insert("plane_note".into(), "the one profile region of a loft section".into());
    Ok((Value::Object(c), region.point))
}

/// A loft: the tree `loft` and the section sketches it takes (pushed to `feats` first).
pub(super) fn loft_json(
    cx: &mut Ctx,
    fid: u64,
    name: &str,
    sketch_names: &HashMap<u64, String>,
    feats: &mut Vec<Value>,
) -> Map<String, Value> {
    let d = cx.d;
    let mut j = Map::new();
    j.insert("type".into(), "loft".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), fid.into());
    let rec = d.rec(fid);
    let Some(h) = stream::parse_header(rec) else {
        j.insert("error".into(), "its layout is not recognised".into());
        return j;
    };
    // The operation enum is the body's first byte, as a sweep's.
    let op = rec.get(h.body).copied().unwrap_or(0);
    cover::field(rec, h.body, 1, "loft.operation");
    j.insert("operation".into(), super::json::op_name(op).into());
    j.insert("target".into(), Value::Null);
    let lists: Vec<Vec<u64>> = ref_lists(d, rec, h.body)
        .into_iter()
        .map(|(_, ids)| ids)
        .filter(|ids| ids.iter().all(|q| d.is(*q, SECTION)))
        .collect();
    let [sections] = lists.as_slice() else {
        j.insert("error".into(), format!("{} lists of loft sections, not one", lists.len()).into());
        return j;
    };
    if !(2..=MAX_SECTIONS).contains(&sections.len()) {
        j.insert("error".into(), format!("a loft of {} sections", sections.len()).into());
        return j;
    }
    // Rails or a centre line would be further references the samples do not have: every
    // reference of the loft is to its sections, its participant bodies' selection, its parameter
    // and owner records, never to another `2CA5A1CD` profile or `5A1BF548` curve selection.
    if ref_ids(d, rec).iter().any(|q| d.is(*q, "5A1BF548")) {
        j.insert("error".into(), "a loft with rails or a centre line".into());
        return j;
    }
    let mut names = Vec::new();
    let mut points: Vec<Value> = Vec::new();
    for (k, s) in sections.iter().enumerate() {
        let sels: Vec<u64> = ref_ids(d, d.rec(*s)).into_iter().filter(|q| d.is(*q, "2CA5A1CD")).collect();
        let [sel] = sels.as_slice() else {
            j.insert("error".into(), "a loft section is not one profile selection".into());
            return j;
        };
        let Some(sid) = super::feature::profile_sketch_of(d, d.rec(*sel)) else {
            // A face (3221: the end caps of two bodies): the face itself is the section.
            let faces = super::json::profile_faces(d, *s);
            let pick = match faces.as_slice() {
                [(g, ops)] => super::json::face_pick(cx, g, ops),
                _ => None,
            };
            let Some(pick) = pick else {
                j.insert("error".into(), "a loft section is not a sketch profile or one face (a point?)".into());
                return j;
            };
            names.push(json!({ "face": pick }));
            points.push(Value::Null);
            continue;
        };
        let Some(sketch) = sketch_names.get(&sid) else {
            j.insert("error".into(), "a loft section's sketch is not in the tree".into());
            return j;
        };
        let copy = format!("{name} section {}", k + 1);
        match profile_sketch(cx, feats, *sel, sid, sketch, &copy) {
            Ok((sk, p)) => {
                feats.push(sk);
                points.push(json!([mm(p[0]), mm(p[1])]));
            }
            Err(why) => {
                j.insert("error".into(), why.into());
                return j;
            }
        }
        names.push(Value::from(copy));
    }
    j.insert("sketches".into(), names.into());
    j.insert("section_points".into(), points.into());
    j
}

/// Where a chain of path curves starts: a free end of it (world, cm) and the direction the path
/// leaves it in. `None` for a closed chain, one with a spline, or several free ends but two.
fn path_start(s: &sketch::Sketch, curves: &[u64]) -> Option<(V3, V3)> {
    let pl = s.plane?;
    // (end point ref, the curve's direction away from that end, in sketch space)
    let mut ends: Vec<(u64, [f64; 2])> = Vec::new();
    for q in curves {
        if let Some(l) = s.lines.get(q) {
            let (a, b) = (l.start_ref?, l.end_ref?);
            let (pa, pb) = (s.points.get(&a)?, s.points.get(&b)?);
            ends.push((a, [pb[0] - pa[0], pb[1] - pa[1]]));
            ends.push((b, [pa[0] - pb[0], pa[1] - pb[1]]));
        } else {
            let a = s.arcs.get(q)?;
            let c = a.center;
            let n = unit(a.normal)?;
            let x = unit(a.xdir)?;
            let y = cross(n, x);
            let am = f64::midpoint(a.a0, a.a1);
            let mid = [
                c[0] + a.radius * (libm::cos(am) * x[0] + libm::sin(am) * y[0]),
                c[1] + a.radius * (libm::cos(am) * x[1] + libm::sin(am) * y[1]),
            ];
            for r in a.refs.iter().skip(1).take(2) {
                let p = s.points.get(r)?;
                let (rx, ry) = (p[0] - c[0], p[1] - c[1]);
                let mut t = [-ry, rx];
                if t[0] * (mid[0] - p[0]) + t[1] * (mid[1] - p[1]) < 0.0 {
                    t = [ry, -rx];
                }
                ends.push((*r, t));
            }
        }
    }
    let mut count: HashMap<u64, usize> = HashMap::new();
    for (r, _) in &ends {
        *count.entry(*r).or_default() += 1;
    }
    let free: Vec<&(u64, [f64; 2])> = ends.iter().filter(|(r, _)| count.get(r) == Some(&1)).collect();
    let [first, _] = free.as_slice() else { return None };
    let p = s.points.get(&first.0)?;
    let o = geom::world(&pl, [p[0], p[1]], 0.0);
    let t = geom::add(geom::mul(pl.x, first.1[0]), geom::mul(pl.y, first.1[1]));
    Some((o, unit(t)?))
}

/// A pipe: the section sketch (pushed to `feats`) and the tree `sweep`.
pub(super) fn pipe_json(
    cx: &mut Ctx,
    fid: u64,
    name: &str,
    sketch_names: &HashMap<u64, String>,
    feats: &mut Vec<Value>,
) -> Map<String, Value> {
    let d = cx.d;
    let mut j = Map::new();
    j.insert("type".into(), "sweep".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), fid.into());
    let rec = d.rec(fid);
    // The body: `u8 0`, then the operation enum as a `u32`.
    let op = stream::parse_header(rec)
        .filter(|h| rec.get(h.body) == Some(&0))
        .and_then(|h| rec.get(h.body + 1..h.body + 5).map(|b| (h.body, b)))
        .and_then(|(at, b)| Some((at, u32::from_le_bytes(b.try_into().ok()?))));
    let Some((at, op)) = op.filter(|(_, op)| (1..=4).contains(op)) else {
        j.insert("error".into(), "its layout is not recognised".into());
        return j;
    };
    cover::field(rec, at, 5, "pipe.operation");
    j.insert("operation".into(), super::json::op_name(u8::try_from(op).unwrap_or(0)).into());
    j.insert("target".into(), Value::Null);
    let value = |n: &str| super::feature::param_values(d, fid, &[n]);
    let (along, against, size) = (value("AlongDistance"), value("AgainstDistance"), value("SectionSize"));
    if along != [1.0] || against.iter().any(|v| *v != 0.0) {
        j.insert("error".into(), "a pipe along part of its path".into());
        return j;
    }
    let Some(dia) = size.first().copied().filter(|v| *v > 0.0 && v.is_finite()) else {
        j.insert("error".into(), "a pipe without a section size".into());
        return j;
    };
    let (path, curves) = match super::json::sweep_path(cx, fid) {
        Ok(p) => p,
        Err(why) => {
            j.insert("error".into(), why.into());
            return j;
        }
    };
    let square = match section_kind(d, rec) {
        Some(Section::Circle) => false,
        Some(Section::Square) => true,
        None => {
            j.insert("error".into(), "a pipe section that is not a circle or a square".into());
            return j;
        }
    };
    let ps = cx.sketch(path);
    let (Some((o, t)), Some(pl)) = (path_start(&ps, &curves), ps.plane) else {
        j.insert("error".into(), "a pipe path that is closed, branched or not of lines and arcs".into());
        return j;
    };
    // The section's plane across the path at its start: y along the path sketch's normal (a
    // square's sides lie along it and across it: 64's square cut tools), x across both.
    let Some(x) = unit(cross(pl.z, t)) else {
        j.insert("error".into(), "a pipe path that leaves its sketch plane".into());
        return j;
    };
    let y = cross(t, x);
    let v3 = |v: V3| json!([round9(v[0]), round9(v[1]), round9(v[2])]);
    let section = format!("{name} section");
    let h = mm(dia / 2.0);
    let (points, curves_json) = if square {
        let corners = [[-h, -h], [h, -h], [h, h], [-h, h]];
        let points: Vec<Value> =
            corners.iter().enumerate().map(|(k, c)| json!({"id": format!("p{k}"), "x": c[0], "y": c[1]})).collect();
        let lines: Vec<Value> = (0..4)
            .map(|k| json!({"id": format!("c{k}"), "type": "line", "start": format!("p{k}"), "end": format!("p{}", (k + 1) % 4)}))
            .collect();
        (points, lines)
    } else {
        (
            vec![json!({"id": "o", "x": 0.0, "y": 0.0})],
            vec![json!({"id": "c0", "type": "circle", "center": "o", "radius": h})],
        )
    };
    feats.push(json!({"type": "sketch", "name": section,
        "plane": {"origin": v3(o.map(mm)), "x_axis": v3(x), "y_axis": v3(y)},
        "plane_note": "a pipe's section across its path's start",
        "points": points, "curves": curves_json}));
    // One straight line has no Frenet frame: the section keeps its own.
    let straight = curves.len() == 1 && curves.first().is_some_and(|c| ps.lines.contains_key(c));
    j.insert("orientation".into(), if straight { "fixed" } else { "frenet" }.into());
    j.insert("sketch".into(), section.into());
    j.insert("path".into(), sketch_names.get(&path).map_or(Value::Null, |s| s.as_str().into()));
    j.insert("path_source_id".into(), path.into());
    j.insert("path_curves".into(), curves.iter().map(|c| format!("c{c}")).collect::<Vec<_>>().into());
    j
}

/// A pipe's section shape.
enum Section {
    Circle,
    Square,
}

/// The section shape: after the reference to the path's selection (`2CA5A1CD` -> `5A1BF548`) and
/// two zero bytes, `u32` 0 for a circle (3221, its stored faces a cylinder of half the section
/// size), 1 for a square (64, its stored cut tools square prisms of the section size), the
/// square's followed by a `f64` 0 (an angle?). Other values, or a square turned, are refused.
fn section_kind(d: &stream::Design, rec: &[u8]) -> Option<Section> {
    let (at, _) = stream::refs_at(d, rec, 0)
        .into_iter()
        .find(|(_, q)| d.is(*q, "2CA5A1CD") && ref_ids(d, d.rec(*q)).iter().any(|c| d.is(*c, "5A1BF548")))?;
    let (_, e) = stream::ref_end(d, rec, at)?;
    if rec.get(e..e + 2) != Some(&[0, 0]) {
        return None;
    }
    cover::field(rec, e + 2, 4, "pipe.section");
    match stream::u32_at(rec, e + 2)? {
        0 => Some(Section::Circle),
        1 if stream::f64_at(rec, e + 6) == Some(0.0) => {
            cover::field(rec, e + 6, 8, "pipe.section-angle");
            Some(Section::Square)
        }
        _ => None,
    }
}

/// An extrude whose profile is an end face of a loft (5998's `Extrude7 (2)`: the loft's top
/// circle, 0.4 mm on with a -45 deg taper): the loft's section that lies on that face's plane,
/// as the stored bodies hold it. Nothing changes where the face is not one such end.
pub(super) fn profile_of_a_loft_cap(cx: &mut Ctx, fid: u64, j: &mut Map<String, Value>, feats: &[Value]) {
    if !j.contains_key("profile_note") || !j.get("sketch").is_some_and(Value::is_null) || j.contains_key("start") {
        return;
    }
    let d = cx.d;
    let faces: Vec<Vec<naming::FaceName>> = cx
        .edge_recipes_of(fid)
        .into_iter()
        .filter_map(|r| match naming::recipe(d, r) {
            (Some(naming::RecipeKind::BoundedFace | naming::RecipeKind::Face), Some(names)) => Some(names),
            _ => None,
        })
        .collect();
    let [names] = faces.as_slice() else { return };
    let Some([(g, ops)]) = names.first().map(Vec::as_slice) else { return };
    let [op] = ops.as_slice() else { return };
    let Some(x) = u64::try_from(*op).ok().and_then(|o| cx.opmap.get(&o).copied()) else { return };
    if !d.is(x, LOFT) {
        return;
    }
    let Some(loft) = feats.iter().find(|f| {
        f.get("type").and_then(Value::as_str) == Some("loft") && f.get("source_id").and_then(Value::as_u64) == Some(x)
    }) else {
        return;
    };
    let (Some(sketches), Some(points)) =
        (loft.get("sketches").and_then(Value::as_array), loft.get("section_points").and_then(Value::as_array))
    else {
        return;
    };
    let (g, ops) = (g.clone(), ops.clone());
    let Some(naming::FaceG::Plane { n, p, .. }) = cx.face(&g, &ops) else { return };
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    // The sections whose plane is the face's (mm in the tree, cm in the face).
    let on: Vec<(&Value, &Value)> = sketches
        .iter()
        .zip(points)
        .filter(|(s, _)| {
            let Some(sk) = feats
                .iter()
                .find(|f| f.get("type").and_then(Value::as_str) == Some("sketch") && f.get("name") == Some(*s))
            else {
                return false;
            };
            let pl = sk.get("plane");
            let at = |k: &str| v3(pl.and_then(|p| p.get(k)));
            let (Some(o), Some(xa), Some(ya)) = (at("origin"), at("x_axis"), at("y_axis")) else { return false };
            let z = cross(xa, ya);
            let off = geom::dot(z, geom::sub(p.map(mm), o));
            (geom::dot(z, n).abs() - 1.0).abs() < 1e-9 && off.abs() < 1e-6
        })
        .collect();
    let [(sketch, point)] = on.as_slice() else { return };
    j.remove("profile_note");
    j.insert("sketch".into(), (*sketch).clone());
    j.insert("profiles".into(), json!([{ "point": point }]));
    j.insert("profile_from".into(), "the end face of a loft: the loft's section on that face's plane".into());
}

#[cfg(test)]
mod tests {
    use super::path_start;
    use crate::fusion_tree::geom::Plane;
    use crate::fusion_tree::sketch::{Arc, Line, Sketch};

    /// A path of a line from (0, 0) to (2, 0) and a quarter arc on to (3, 1) about (2, 1), on a
    /// sketch plane turned to the world's XZ: it starts at a free end, leaving along its curve.
    #[test]
    fn a_path_starts_at_a_free_end_along_its_curve() {
        let mut s = Sketch {
            plane: Some(Plane { x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0], z: [0.0, -1.0, 0.0], origin: [0.0, 5.0, 0.0] }),
            ..Sketch::default()
        };
        for (k, p) in [(1, [0.0, 0.0, 0.0]), (2, [2.0, 0.0, 0.0]), (3, [2.0, 1.0, 0.0]), (4, [3.0, 1.0, 0.0])] {
            s.points.insert(k, p);
        }
        s.lines.insert(10, Line { origin: [0.0; 3], vector: [2.0, 0.0, 0.0], end_ref: Some(2), start_ref: Some(1) });
        let half_pi = std::f64::consts::FRAC_PI_2;
        let arc = Arc {
            center: [2.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            xdir: [1.0, 0.0, 0.0],
            radius: 1.0,
            a0: -half_pi,
            a1: 0.0,
            refs: vec![3, 4, 2],
        };
        s.arcs.insert(11, arc);
        let (o, t) = path_start(&s, &[10, 11]).expect("an open chain");
        // The line's free end (0, 0) leaves along +x; the arc's (3, 1) leaves along -y in the sketch.
        let line_end = (o, t) == ([0.0, 5.0, 0.0], [1.0, 0.0, 0.0]);
        let arc_end = (o, t) == ([3.0, 5.0, 1.0], [0.0, 0.0, -1.0]);
        assert!(line_end || arc_end, "{o:?} {t:?}");
        // A closed chain has no start.
        s.lines.insert(12, Line { origin: [0.0; 3], vector: [3.0, 1.0, 0.0], end_ref: Some(4), start_ref: Some(1) });
        assert!(path_start(&s, &[10, 11, 12]).is_none());
    }

    /// A loft or pipe that does not read names its reason; one that reads changes a body of one
    /// feature.
    #[test]
    fn lofts_are_checked_like_sweeps() {
        use crate::fusion_tree::json::check;
        use serde_json::json;
        let loft = |extra: serde_json::Value| {
            let mut f = json!({"type": "loft", "name": "Loft1", "sketches": ["A", "B"], "operation": "join"});
            if let (Some(o), Some(e)) = (f.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            let e = json!({"type": "extrude", "name": "Extrude1", "profiles": [], "distance": 1.0, "operation": "new"});
            json!({"features": [e, f]})
        };
        assert!(check(&loft(json!({"target": "Extrude1"}))).is_ok());
        assert!(check(&loft(json!({"target": ["Extrude1", "Pipe1"]}))).unwrap_err().contains("not one body"));
        let why = check(&loft(json!({"error": "a loft section is not a sketch profile (a point or a face)"})));
        assert!(why.unwrap_err().contains("not a sketch profile"));
    }
}
