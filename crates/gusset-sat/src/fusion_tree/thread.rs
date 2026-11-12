//! Thread `584D9526` (layout notes, sections 29 and 36): the thread's designation and form, read
//! to say whether it is cosmetic (no geometry: the tree leaves it out) or modeled, and a modeled
//! thread on an extruded cylinder as the tree features that make Fusion's form: the face moved to
//! the thread's major (external) or minor (internal) diameter, then the groove of the basic
//! profile swept along exact helices and cut off square where the thread starts and ends.

use super::Ctx;
use super::acis;
use super::feature::{params_of, pget};
use super::geom::{V3, cross, dot, mul, norm, sub, unit, world};
use super::naming::FaceG;
use super::stream::{Rd, f64_at, parse_header};
use crate::cover;
use serde_json::{Value, json};

/// What a thread record says.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Thread {
    /// Flank angle (degrees, 60 for ISO metric).
    pub angle: f64,
    pub class: String,
    pub designation: String,
    pub internal: bool,
    /// Major and minor diameter, pitch (cm).
    pub major: f64,
    pub minor: f64,
    pub pitch: f64,
    /// Length and offset from the face's end (cm); `None` for a thread along the whole face.
    pub length: Option<f64>,
    pub offset: Option<f64>,
    /// The `u32` after the side: the end of the face the offset is measured from, read as 2 for
    /// where the face's extrude starts and 1 for where it ends (1145 and 3350's Thread1, the
    /// only two samples with an offset; section 36).
    pub end_code: u32,
}

/// The record body: `f64` angle, `u8`, four `wstr` (class, designation, size, form name), `u8`
/// internal, `u32`, `f64` major and minor diameter, `u8`, `f64` pitch and pitch diameter.
pub(super) fn read(cx: &Ctx, fid: u64) -> Option<Thread> {
    let rec = cx.d.rec(fid);
    let body = parse_header(rec)?.body;
    let angle = f64_at(rec, body)?;
    let mut r = Rd::new(rec, body + 9);
    let class = r.wstr()?;
    let designation = r.wstr()?;
    let _size = r.wstr()?;
    let _form = r.wstr()?;
    let internal = match rec.get(r.i)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let end_code = u32::from_le_bytes(rec.get(r.i + 1..r.i + 5)?.try_into().ok()?);
    let at = r.i + 5;
    let (major, minor) = (f64_at(rec, at)?, f64_at(rec, at + 8)?);
    let (pitch, pitch_d) = (f64_at(rec, at + 17)?, f64_at(rec, at + 25)?);
    let sane = |v: f64| v.is_finite() && v > 0.0 && v < 1e4;
    if !(angle.is_finite() && angle > 0.0 && angle < 180.0)
        || ![major, minor, pitch, pitch_d].into_iter().all(sane)
        || !(minor < pitch_d && pitch_d < major)
    {
        return None;
    }
    cover::field(rec, body, 8, "thread.angle");
    cover::field(rec, r.i, 1, "thread.internal");
    cover::field(rec, r.i + 1, 4, "thread.offset-end");
    cover::field(rec, at, 16, "thread.major-minor");
    cover::field(rec, at + 17, 16, "thread.pitch-pitch-diameter");
    let ps = params_of(cx.d, fid);
    let val = |n: &str| pget(&ps, n).and_then(|p| p.value);
    Some(Thread {
        angle,
        class,
        designation,
        internal,
        major,
        minor,
        pitch,
        length: val("ThreadLength"),
        offset: val("ThreadOffset"),
        end_code,
    })
}

/// Whether a stored body holds the thread's helical edges: a helix of its pitch on its major or
/// minor radius (a cosmetic thread leaves the faces cylinders; a modeled one cuts the form).
fn modeled(cx: &Ctx, t: &Thread) -> bool {
    cx.acis.iter().filter_map(|(_, b)| acis::helices(b)).flatten().any(|h| {
        let travel = norm(h.pitch);
        let r = |d: f64| (h.radius - d / 2.0).abs() <= 1e-6;
        (travel - t.pitch).abs() <= 1e-7 && (r(t.major) || r(t.minor)) && dot(h.pitch, h.pitch) > 0.0
    })
}

/// What the tree makes of a thread.
pub(super) enum Word {
    /// Left out of the tree, with the note for the skipped list.
    Cosmetic(String),
    /// The tree features that build it.
    Features(Vec<Value>),
}

/// The tree's word on a thread: cosmetic (left out), the features of a modeled one on an
/// extruded cylinder of the body `body` (the feature that made it), or `Err(feature)` for one
/// the tree cannot hold.
pub(super) fn json(
    cx: &mut Ctx,
    fid: u64,
    name: &str,
    fusion_type: &str,
    class: &str,
    body: Option<&str>,
) -> Result<Word, Value> {
    let Some(t) = read(cx, fid) else {
        return Err(json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "error": "its thread record is not recognised"}));
    };
    let mm = |v: f64| (v * 10.0 * 1e4).round() / 1e4;
    let what = format!(
        "{} {}, {}, major {} mm, minor {} mm, pitch {} mm, {}",
        t.designation,
        t.class,
        if t.internal { "internal" } else { "external" },
        mm(t.major),
        mm(t.minor),
        mm(t.pitch),
        match (t.length, t.offset) {
            (Some(l), Some(o)) => format!("{} mm long from {} mm", mm(l), mm(o)),
            _ => "full length".to_string(),
        }
    );
    if !modeled(cx, &t) {
        return Ok(Word::Cosmetic(format!(
            "a cosmetic thread ({what}): no helical face of it is stored, it changes no geometry"
        )));
    }
    let fail = |why: &str| {
        json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "thread": what.clone(), "error": format!("a modeled thread ({what}): {why}")})
    };
    let body = body.ok_or_else(|| fail("the body it is on is not one feature's"))?;
    let face = face_of(cx, fid).map_err(fail)?;
    let mut feats = features(&t, &face, name, body).map_err(fail)?;
    for f in &mut feats {
        if let Some(o) = f.as_object_mut() {
            o.insert("source_id".into(), fid.into());
        }
    }
    Ok(Word::Features(feats))
}

/// The threaded face, the side of a whole extruded circle (cm): its axis point on the sketch
/// plane, that plane's unit normal and x direction, the extrude's start and end along the
/// normal, and the circle's radius.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Face {
    pub centre: V3,
    pub normal: V3,
    pub x: V3,
    pub t0: f64,
    pub t1: f64,
    pub radius: f64,
}

fn face_of(cx: &mut Ctx, fid: u64) -> Result<Face, &'static str> {
    let rids = cx.input_recipes(fid);
    let [rid] = rids.as_slice() else { return Err("it is not on one face") };
    let (_, names) = super::naming::recipe(cx.d, *rid);
    let (g, ops) = names.as_ref().and_then(|n| n.first()).and_then(|f| f.first()).ok_or("its face has no name")?;
    let Some(FaceG::Cyl { feature, seg, pl, .. }) = cx.face_with_middle(g, ops) else {
        return stored_side(cx, g, ops).ok_or("its face is not the side of an extruded circle");
    };
    let eg = cx.ext_geom(feature).ok_or("its face's extrude is not read")?;
    let (Some(t0), Some(t1)) = (eg.t0, eg.t1) else { return Err("its face's extrude has no distance") };
    if eg.tapered {
        return Err("its face is tapered");
    }
    let arc = eg.sketch.arcs.get(&seg.curve).ok_or("its face's circle is not read")?;
    let closed = seg.poly.first().zip(seg.poly.last()).is_some_and(|(a, b)| super::geom::dist(*a, *b) < 1e-9);
    if !closed || seg.poly.len() < 3 {
        return Err("its face is not a whole cylinder");
    }
    let centre = world(&pl, [arc.center[0], arc.center[1]], 0.0);
    Ok(Face { centre, normal: pl.z, x: pl.x, t0, t1, radius: arc.radius })
}

/// The side of a whole extruded circle known from the stored bodies where the extrude's
/// generator numbering is not (a profile with holes the stored faces do not confirm: 3350's
/// Thread2 is on the 8 mm hole of a hexagon, generator 10 of `Extrude8`): the stored faces named
/// (generator, the extrude) lie on one cylinder, whose axis is the extrude's direction through
/// the centre of exactly one whole circle of the profile with that radius.
fn stored_side(cx: &mut Ctx, g: &str, ops: &[i32]) -> Option<Face> {
    let [op] = ops else { return None };
    let fid = *cx.opmap.get(&u64::try_from(*op).ok()?)?;
    if !cx.d.is(fid, super::EXTRUDE) {
        return None;
    }
    let super::naming::Surface::Cylinder { origin, axis, radius } = cx.stored_face(g, ops)?.surface else {
        return None;
    };
    let eg = cx.ext_geom(fid)?;
    let (Some(t0), Some(t1)) = (eg.t0, eg.t1) else { return None };
    let pl = eg.pl;
    if eg.tapered || (dot(unit(axis)?, pl.z).abs() - 1.0).abs() > 1e-9 {
        return None;
    }
    let mut found = None;
    for seg in &eg.segs {
        let Some(arc) = eg.sketch.arcs.get(&seg.curve) else { continue };
        let closed = seg.poly.first().zip(seg.poly.last()).is_some_and(|(a, b)| super::geom::dist(*a, *b) < 1e-9);
        let centre = world(&pl, [arc.center[0], arc.center[1]], 0.0);
        let w = sub(centre, origin);
        let off = norm(sub(w, mul(pl.z, dot(w, pl.z))));
        if closed && seg.poly.len() >= 3 && (arc.radius - radius).abs() < 1e-7 && off < 1e-7 {
            if found.is_some() {
                return None;
            }
            found = Some(Face { centre, normal: pl.z, x: pl.x, t0, t1, radius: arc.radius });
        }
    }
    found
}

/// The tree features of a modeled thread (mm). Fusion's form, read from the stored bodies of
/// 1145 and 3350 (section 36): the whole face moved to the major diameter (external) or the
/// minor one (internal); then the groove, its flat at the major diameter centred P/16 before
/// the face's start modulo P along the face's axis (the flat at the minor diameter 7P/16 after
/// it), the crest flat P/8 wide, the flanks at half the flank angle to the radial; the groove
/// ends square on the planes across the axis where the thread starts and ends (no run-out, no
/// chamfer), and runs out past a face end it reaches.
pub(super) fn features(t: &Thread, f: &Face, name: &str, body: &str) -> Result<Vec<Value>, &'static str> {
    let cm = 10.0;
    let (p, rmaj, rmin, r0) = (t.pitch * cm, t.major * cm / 2.0, t.minor * cm / 2.0, f.radius * cm);
    if !(rmin > 0.0 && rmin < rmaj && p > 0.0 && t.angle > 0.0 && t.angle < 180.0 && r0 > 0.0) {
        return Err("its form is not a thread's");
    }
    // The face's axis runs the way its extrude went, from the extrude's start.
    let len = (f.t1 - f.t0).abs() * cm;
    let axis = mul(f.normal, if f.t1 >= f.t0 { 1.0 } else { -1.0 });
    // The document's helix climbs a world axis through the origin.
    let k = (0..3)
        .find(|k| axis.get(*k).is_some_and(|c| (c.abs() - 1.0).abs() < 1e-9))
        .ok_or("its axis is not along a world axis")?;
    let along = |v: V3| v.get(k).copied().unwrap_or(0.0);
    let s = along(axis).signum();
    if (0..3).any(|i| i != k && f.centre.get(i).is_some_and(|v| v.abs() > 1e-9)) {
        return Err("its axis does not pass through the origin");
    }
    // Positions along the world axis (mm) of the face's start and of offsets along the face.
    let start = (along(f.centre) + f.t0 * along(f.normal)) * cm;
    let at = |u: f64| start + s * u;
    let (u0, u1) = match (t.length, t.offset) {
        (Some(l), Some(o)) => match t.end_code {
            2 => (o * cm, (o + l) * cm),
            1 => (len - (o + l) * cm, len - o * cm),
            _ => return Err("the end its offset is measured from is not known"),
        },
        (None, None) => (0.0, len),
        _ => return Err("it has a length or an offset without the other"),
    };
    let (u0, u1) = (u0.max(0.0), u1.min(len));
    if u1 - u0 < 1e-6 {
        return Err("it does not reach its face");
    }
    let (face_lo, face_hi) = (at(0.0).min(at(len)), at(0.0).max(at(len)));
    let (lo, hi) = (at(u0).min(at(u1)), at(u0).max(at(u1)));
    let (clip_lo, clip_hi) = (lo > face_lo + 1e-6, hi < face_hi - 1e-6);
    let tn = libm::tan(t.angle.to_radians() / 2.0);
    let w = (rmaj - rmin) * tn;
    if 2.0 * w >= 7.0 * p / 8.0 {
        return Err("its flanks meet before its root");
    }
    let mut e = [0.0; 3];
    if let Some(v) = e.get_mut(k) {
        *v = 1.0;
    }
    let x = unit(sub(f.x, mul(e, dot(f.x, e)))).ok_or("its sketch has no direction across the axis")?;
    let y = cross(e, x);
    let v3 = |v: V3| json!([r9(v[0]), r9(v[1]), r9(v[2])]);
    let mut out = Vec::new();
    // A disc or ring across the axis from `from` (along it, mm) `depth` up: joined to or cut
    // from `target`, or a body of its own.
    let ring =
        |out: &mut Vec<Value>, tag: &str, radii: &[f64], from: f64, depth: f64, op: &str, target: Option<&str>| {
            let curves: Vec<Value> = radii
                .iter()
                .enumerate()
                .map(|(i, r)| json!({"id": format!("c{i}"), "type": "circle", "center": "o", "radius": r9(*r)}))
                .collect();
            let pick = match radii {
                [a, b] => (a + b) / 2.0,
                _ => 0.0,
            };
            let sketch = format!("{name} {tag} sketch");
            out.push(json!({"type": "sketch", "name": sketch, "plane": {"origin": v3(mul(e, from)), "x_axis": v3(x),
            "y_axis": v3(y)}, "points": [{"id": "o", "x": 0.0, "y": 0.0}], "curves": curves}));
            let mut j = json!({"type": "extrude", "name": format!("{name} {tag}"), "sketch": sketch,
            "profiles": [{"point": [r9(pick), 0.0]}], "distance": r9(depth), "symmetric": false, "operation": op});
            if let (Some(o), Some(tg)) = (j.as_object_mut(), target) {
                o.insert("target".into(), tg.into());
            }
            out.push(j);
        };
    // 1. The face moved to the thread's diameter.
    if t.internal {
        // The hole filled in to the minor diameter (3350's Thread2: 8 mm to 7.51775 mm), or
        // opened out to it.
        if r0 < rmin - 1e-6 {
            return Err("its hole is narrower than the thread's minor diameter");
        }
        if r0 > rmin + 1e-6 {
            ring(&mut out, "face", &[rmin, r0], face_lo, face_hi - face_lo, "join", Some(body));
            moved(&mut out, k, r0, rmin, face_lo, face_hi);
        }
    } else {
        if r0 < rmaj - 1e-6 {
            return Err("its face is narrower than the thread's major diameter");
        }
        if r0 > rmaj + 1e-6 {
            ring(&mut out, "face", &[rmaj, r0 + p.max(0.1 * r0)], face_lo, face_hi - face_lo, "cut", Some(body));
            moved(&mut out, k, r0, rmaj, face_lo, face_hi);
        }
    }
    // 2. The groove at the sketch's x direction, as (radius, offset along the face's axis from
    // its start), run past the surface it cuts so that no edge of it lies there.
    let ext = 0.03 * p / tn;
    let prof: [(f64, f64); 4] = if t.internal {
        [
            (rmin - ext, p / 2.0 - ext * tn),
            (rmaj, p / 2.0 + w),
            (rmaj, 11.0 * p / 8.0 - w),
            (rmin - ext, 11.0 * p / 8.0 + ext * tn),
        ]
    } else {
        [(rmaj + ext, -ext * tn), (rmin, w), (rmin, 7.0 * p / 8.0 - w), (rmaj + ext, 7.0 * p / 8.0 + ext * tn)]
    };
    if prof.iter().any(|(r, _)| *r <= 0.0) {
        return Err("its groove reaches the axis");
    }
    let outer = rmaj + ext + p;
    let world_prof: Vec<(f64, f64)> = prof.iter().map(|(r, u)| (*r, at(*u))).collect();
    let low = world_prof.iter().map(|q| q.1).fold(f64::INFINITY, f64::min);
    // The first copy a pitch or more below where the thread starts, so that every angle is
    // covered from there; one turn a sweep: OCCT cut a ten-turn groove through 1145's end face
    // not at all and failed on one of two turns there, one turn at a time it cuts exactly.
    let turns = 1.0;
    let mut z = ((lo - p - low) / p).floor() * p;
    let mut i = 0usize;
    while low + z <= hi + 1e-9 {
        i += 1;
        if i > 1000 {
            return Err("it has too many turns");
        }
        let (bottom, top) = (low + z, low + z + (turns + 1.0) * p);
        let tag = format!("groove {i}");
        let sk = format!("{name} {tag} sketch");
        let points: Vec<Value> = world_prof
            .iter()
            .enumerate()
            .map(|(j, (r, h))| json!({"id": format!("p{j}"), "x": r9(*r), "y": r9(h + z)}))
            .collect();
        let curves: Vec<Value> = (0..4)
            .map(|j| {
                json!({"id": format!("l{j}"), "type": "line", "start": format!("p{j}"), "end": format!("p{}", (j + 1) % 4)})
            })
            .collect();
        out.push(json!({"type": "sketch", "name": sk, "plane": {"origin": v3([0.0; 3]), "x_axis": v3(x),
            "y_axis": v3(e)}, "points": points, "curves": curves}));
        let helix = json!({"origin": [0.0, 0.0, 0.0], "axis": v3(e), "x": v3(x), "radius": r9(rmaj),
            "pitch": r9(p), "height": r9(turns * p)});
        let groove = format!("{name} {tag}");
        let (cl, ch) = (clip_lo && bottom < lo, clip_hi && top > hi);
        if cl || ch {
            out.push(json!({"type": "sweep", "name": groove, "sketch": sk, "helix": helix, "operation": "new"}));
            let from = if cl { lo } else { bottom - p };
            let to = if ch { hi } else { top + p };
            ring(&mut out, &format!("{tag} ends"), &[outer], from, to - from, "new", None);
            out.push(json!({"type": "combine", "name": format!("{groove} cut off"), "target": groove,
                "tools": [format!("{name} {tag} ends")], "operation": "intersect"}));
            out.push(json!({"type": "combine", "name": format!("{groove} cut"), "target": body,
                "tools": [groove], "operation": "cut"}));
        } else {
            out.push(
                json!({"type": "sweep", "name": groove, "sketch": sk, "helix": helix, "operation": "cut", "target": body}),
            );
        }
        z += turns * p;
    }
    Ok(out)
}

/// Marks the last feature (the face's move) with where the face was and went: the world axis,
/// the radius before and after, and the face's extent along the axis (mm).
fn moved(out: &mut [Value], k: usize, from: f64, to: f64, lo: f64, hi: f64) {
    if let Some(o) = out.last_mut().and_then(Value::as_object_mut) {
        o.insert("moves_face".into(), json!({"axis": k, "radius": from, "to": to, "from": lo, "until": hi}));
    }
}

/// Edges a later blend picks on a face a thread moved: Fusion names them as before, and their
/// pick points (taken from the extrude that made the face) lie where the face was. They move
/// with the face, across the axis to its new radius (1145's Fillet1 rounds the bottom edge of
/// the threaded rod, at the major diameter since the thread). The marks are taken out.
pub(super) fn moved_edges(feats: &mut [Value]) {
    let mut moves: Vec<(usize, f64, f64, f64, f64)> = Vec::new();
    for f in feats.iter_mut() {
        let Some(o) = f.as_object_mut() else { continue };
        if let Some(m) = o.remove("moves_face") {
            let num = |key: &str| m.get(key).and_then(Value::as_f64);
            if let (Some(k), Some(r), Some(to), Some(lo), Some(hi)) =
                (m.get("axis").and_then(Value::as_u64), num("radius"), num("to"), num("from"), num("until"))
                && let Ok(k) = usize::try_from(k)
            {
                moves.push((k, r, to, lo, hi));
            }
            continue;
        }
        if moves.is_empty() || !matches!(o.get("type").and_then(Value::as_str), Some("fillet" | "chamfer")) {
            continue;
        }
        let Some(edges) = o.get_mut("edges").and_then(Value::as_array_mut) else { continue };
        for e in edges.iter_mut() {
            let Some(eo) = e.as_object_mut() else { continue };
            // Only an edge whose every point lay on the old face moves.
            let mut all: Vec<V3> = eo.get("midpoint").and_then(get3).into_iter().collect();
            if let Some(path) = eo.get("path").and_then(Value::as_array) {
                all.extend(path.iter().filter_map(get3));
            }
            let Some(&(k, _, to, _, _)) = moves
                .iter()
                .find(|(k, r, _, lo, hi)| !all.is_empty() && all.iter().all(|q| on_cylinder(*q, *k, *r, *lo, *hi)))
            else {
                continue;
            };
            let shift = |q: V3| -> Value {
                let mut a = q;
                if let Some(v) = a.get_mut(k) {
                    *v = 0.0;
                }
                let r = norm(a);
                let mut out = mul(a, if r > 0.0 { to / r } else { 1.0 });
                if let (Some(d), Some(v)) = (out.get_mut(k), q.get(k)) {
                    *d = *v;
                }
                json!([r9(out[0]), r9(out[1]), r9(out[2])])
            };
            if let Some(m) = eo.get("midpoint").and_then(get3) {
                eo.insert("midpoint".into(), shift(m));
            }
            if let Some(path) = eo.get("path").and_then(Value::as_array) {
                let moved: Vec<Value> = path.iter().filter_map(get3).map(shift).collect();
                eo.insert("path".into(), moved.into());
            }
        }
    }
}

fn get3(v: &Value) -> Option<V3> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}

/// Whether `q` (mm) lies on the cylinder of radius `r` about world axis `k` within `lo..hi`.
fn on_cylinder(q: V3, k: usize, r: f64, lo: f64, hi: f64) -> bool {
    let mut a = q;
    let h = a.get(k).copied().unwrap_or(0.0);
    if let Some(v) = a.get_mut(k) {
        *v = 0.0;
    }
    (norm(a) - r).abs() < 1e-4 && h > lo - 1e-4 && h < hi + 1e-4
}

fn r9(v: f64) -> f64 {
    (v * 1e9).round() / 1e9
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn m16(internal: bool) -> Thread {
        Thread {
            angle: 60.0,
            class: "6H".into(),
            designation: "M16x1".into(),
            internal,
            major: 1.61522,
            minor: 1.50355,
            pitch: 0.1,
            length: None,
            offset: None,
            end_code: 2,
        }
    }

    #[test]
    fn an_internal_thread_fills_its_hole_in_to_the_minor_diameter() {
        // 3350's Thread2: an 8 mm hole 3 mm deep, from z -5 up.
        let face = |r: f64| Face {
            centre: [0.0, 0.0, -0.5],
            normal: [0.0, 0.0, 1.0],
            x: [1.0, 0.0, 0.0],
            t0: 0.0,
            t1: 0.3,
            radius: r,
        };
        let out = features(&m16(true), &face(0.8), "Thread2", "Extrude8").unwrap();
        let ring = &out[1];
        assert_eq!(ring["operation"], "join");
        assert_eq!(ring["target"], "Extrude8");
        let radii: Vec<f64> =
            out[0]["curves"].as_array().unwrap().iter().map(|c| c["radius"].as_f64().unwrap()).collect();
        assert_eq!(radii, vec![7.51775, 8.0]);
        assert!((ring["moves_face"]["to"].as_f64().unwrap() - 7.51775).abs() < 1e-9);
        // A hole already narrower than the thread's minor diameter is not filled.
        assert!(features(&m16(true), &face(0.7), "Thread2", "Extrude8").is_err());
    }
}
