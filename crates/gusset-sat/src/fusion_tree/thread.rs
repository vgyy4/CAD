//! Thread `584D9526` (layout notes, sections 29 and 36): the thread's designation and form, read
//! to say whether it is cosmetic (no geometry: the tree leaves it out) or modeled, and a modeled
//! thread on an extruded cylinder as the tree features that make Fusion's form: the face moved to
//! the thread's major (external) or minor (internal) diameter, then the groove of the basic
//! profile swept along exact helices and cut off square where the thread starts and ends.

use super::Ctx;
use super::acis;
use super::feature::{params_of, pget};
use super::geom::{V3, dot, mul, norm, sub, unit, world};
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

/// The record body: `f64` angle, `u8` external (1) or internal (0), four `wstr` (class,
/// designation, size, form name), `u8` (1 for every full-length thread of the corpus, 0 for
/// every one with a length; not read), `u32`, `f64` major and minor diameter, `u8`, `f64` pitch
/// and pitch diameter. The side byte follows the class in all ten records of the three sets (6g
/// and 2A external, 6H internal); the byte after the form name had been read as it, and 3692's
/// external 1/4-56 UNS 2A thread, full length, came in as an internal one.
pub(super) fn read(cx: &Ctx, fid: u64) -> Option<Thread> {
    let rec = cx.d.rec(fid);
    let body = parse_header(rec)?.body;
    let angle = f64_at(rec, body)?;
    let internal = match rec.get(body + 8)? {
        0 => true,
        1 => false,
        _ => return None,
    };
    let mut r = Rd::new(rec, body + 9);
    let class = r.wstr()?;
    let designation = r.wstr()?;
    let _size = r.wstr()?;
    let _form = r.wstr()?;
    if *rec.get(r.i)? > 1 {
        return None;
    }
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
    cover::field(rec, body + 8, 1, "thread.external");
    cover::field(rec, r.i, 1, "thread.full-length");
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

/// The tree's word on a thread: a `thread` feature (modeled where a stored body holds its
/// helices, else cosmetic) on the face it names, or `Err(feature)` for a modeled one the tree
/// cannot place. A cosmetic thread whose face is not found is left out with a note (it changes
/// no geometry).
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
    let is_modeled = modeled(cx, &t);
    let placed = body
        .ok_or("the body it is on is not one feature's")
        .and_then(|_| face_of(cx, fid))
        .and_then(|face| feature(&t, &face, name, is_modeled));
    match placed {
        Ok(mut f) => {
            if let Some(o) = f.as_object_mut() {
                o.insert("source_id".into(), fid.into());
            }
            Ok(Word::Features(vec![f]))
        }
        Err(why) if !is_modeled => Ok(Word::Cosmetic(format!(
            "a cosmetic thread ({what}): no helical face of it is stored, it changes no geometry ({why})"
        ))),
        Err(why) => Err(json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "thread": what, "error": format!("a modeled thread ({what}): {why}")})),
    }
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
/// Thread2 is on the 8 mm hole of a hexagon, generator 10 of `Extrude8`): every stored face named
/// (generator, the extrude) lies on a cylinder about one axis, the extrude's direction through
/// the centre of exactly one whole circle of the profile that has the radius of one of them (a
/// state after the thread holds the face moved to the thread's diameter under the same name).
fn stored_side(cx: &mut Ctx, g: &str, ops: &[i32]) -> Option<Face> {
    let [op] = ops else { return None };
    let fid = *cx.opmap.get(&u64::try_from(*op).ok()?)?;
    if !cx.d.is(fid, super::EXTRUDE) {
        return None;
    }
    let mut cyls = Vec::new();
    for s in cx.stored_surfaces(g, ops) {
        let super::naming::Surface::Cylinder { origin, axis, radius } = s else { return None };
        cyls.push((origin, unit(axis)?, radius));
    }
    let &(origin, _, _) = cyls.first()?;
    let eg = cx.ext_geom(fid)?;
    let (Some(t0), Some(t1)) = (eg.t0, eg.t1) else { return None };
    let pl = eg.pl;
    let across = |p: V3| {
        let w = sub(p, origin);
        norm(sub(w, mul(pl.z, dot(w, pl.z))))
    };
    if eg.tapered || cyls.iter().any(|(o, a, _)| (dot(*a, pl.z).abs() - 1.0).abs() > 1e-9 || across(*o) > 1e-7) {
        return None;
    }
    let mut found = None;
    for seg in &eg.segs {
        let Some(arc) = eg.sketch.arcs.get(&seg.curve) else { continue };
        let closed = seg.poly.first().zip(seg.poly.last()).is_some_and(|(a, b)| super::geom::dist(*a, *b) < 1e-9);
        let centre = world(&pl, [arc.center[0], arc.center[1]], 0.0);
        let named = cyls.iter().any(|(_, _, r)| (arc.radius - r).abs() < 1e-7);
        if closed && seg.poly.len() >= 3 && named && across(centre) < 1e-7 {
            if found.is_some() {
                return None;
            }
            found = Some(Face { centre, normal: pl.z, x: pl.x, t0, t1, radius: arc.radius });
        }
    }
    found
}

/// The tree's `thread` feature (mm): the face picked at its middle, the thread's diameters,
/// pitch and flank angle, where it lies (`along` runs from the end its offset is measured from)
/// and, for a modeled thread that moves the face, where the face was and went (`moves_face`,
/// for later blends, taken out by [`moved_edges`]). Fusion's form (section 36) is the
/// document's: the whole face moved to the major (external) or minor (internal) diameter, the
/// groove square at the ends inside the face.
pub(super) fn feature(t: &Thread, f: &Face, name: &str, modeled: bool) -> Result<Value, &'static str> {
    let cm = 10.0;
    let (p, rmaj, rmin, r0) = (t.pitch * cm, t.major * cm / 2.0, t.minor * cm / 2.0, f.radius * cm);
    if !(rmin > 0.0 && rmin < rmaj && p > 0.0 && t.angle > 0.0 && t.angle < 180.0 && r0 > 0.0) {
        return Err("its form is not a thread's");
    }
    // The face's axis runs the way its extrude went, from the extrude's start.
    let len = (f.t1 - f.t0).abs() * cm;
    let axis = mul(f.normal, if f.t1 >= f.t0 { 1.0 } else { -1.0 });
    let x = unit(sub(f.x, mul(axis, dot(f.x, axis)))).ok_or("its sketch has no direction across the axis")?;
    let start = add(mul(f.centre, cm), mul(f.normal, f.t0 * cm));
    let along = match (t.length, t.offset) {
        (Some(_), Some(_)) => match t.end_code {
            2 => axis,
            1 => mul(axis, -1.0),
            _ => return Err("the end its offset is measured from is not known"),
        },
        (None, None) => axis,
        _ => return Err("it has a length or an offset without the other"),
    };
    if t.offset.is_some_and(|o| o * cm >= len - 1e-6) {
        return Err("it does not reach its face");
    }
    let mid = add(start, mul(axis, len / 2.0));
    let point = add(mid, mul(x, r0));
    let normal = if t.internal { mul(x, -1.0) } else { x };
    let mut j = json!({"type": "thread", "name": name, "face": {"point": v3(point), "normal": v3(normal)},
        "pitch": r9(p), "major": r9(2.0 * rmaj), "minor": r9(2.0 * rmin), "angle": r9(t.angle),
        "along": v3(along), "modeled": modeled});
    if let Some(o) = j.as_object_mut() {
        let designation = format!("{} {}", t.designation, t.class);
        o.insert("designation".into(), designation.trim().into());
        if let (Some(l), Some(off)) = (t.length, t.offset) {
            o.insert("length".into(), r9(l * cm).into());
            o.insert("offset".into(), r9(off * cm).into());
        }
        let to = if t.internal { rmin } else { rmaj };
        if modeled && (r0 - to).abs() > 1e-6 {
            o.insert(
                "moves_face".into(),
                json!({"origin": v3(start), "axis": v3(axis), "radius": r9(r0), "to": r9(to), "from": 0.0,
                    "until": r9(len)}),
            );
        }
    }
    Ok(j)
}

fn v3(v: V3) -> Value {
    json!([r9(v[0]), r9(v[1]), r9(v[2])])
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Edges a later blend picks on a face a thread moved: Fusion names them as before, and their
/// pick points (taken from the extrude that made the face) lie where the face was. They move
/// with the face, across the axis to its new radius (1145's Fillet1 rounds the bottom edge of
/// the threaded rod, at the major diameter since the thread). The marks are taken out.
pub(super) fn moved_edges(feats: &mut [Value]) {
    // (axis origin, axis, radius before, after, extent along the axis from the origin)
    let mut moves: Vec<(V3, V3, f64, f64, f64, f64)> = Vec::new();
    for f in feats.iter_mut() {
        let Some(o) = f.as_object_mut() else { continue };
        if let Some(m) = o.remove("moves_face") {
            let num = |key: &str| m.get(key).and_then(Value::as_f64);
            if let (Some(c), Some(a), Some(r), Some(to), Some(lo), Some(hi)) = (
                m.get("origin").and_then(get3),
                m.get("axis").and_then(get3).and_then(unit),
                num("radius"),
                num("to"),
                num("from"),
                num("until"),
            ) {
                moves.push((c, a, r, to, lo, hi));
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
            let Some(&(c, a, _, to, _, _)) = moves.iter().find(|(c, a, r, _, lo, hi)| {
                !all.is_empty() && all.iter().all(|q| on_cylinder(*q, *c, *a, *r, *lo, *hi))
            }) else {
                continue;
            };
            let shift = |q: V3| -> Value {
                let foot = add(c, mul(a, dot(sub(q, c), a)));
                let w = sub(q, foot);
                let r = norm(w);
                v3(add(foot, mul(w, if r > 0.0 { to / r } else { 1.0 })))
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

/// Whether `q` (mm) lies on the cylinder of radius `r` about the axis through `c` along `a`
/// within `lo..hi` of `c`.
fn on_cylinder(q: V3, c: V3, a: V3, r: f64, lo: f64, hi: f64) -> bool {
    let h = dot(sub(q, c), a);
    let w = sub(sub(q, c), mul(a, h));
    (norm(w) - r).abs() < 1e-4 && h > lo - 1e-4 && h < hi + 1e-4
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
        let f = feature(&m16(true), &face(0.8), "Thread2", true).unwrap();
        assert_eq!(f["type"], "thread");
        // Picked on the hole's wall at its middle, its normal towards the axis.
        assert_eq!(f["face"]["point"], json!([8.0, 0.0, -3.5]));
        assert_eq!(f["face"]["normal"], json!([-1.0, 0.0, 0.0]));
        assert_eq!((f["major"].as_f64(), f["minor"].as_f64()), (Some(16.1522), Some(15.0355)));
        assert!((f["moves_face"]["to"].as_f64().unwrap() - 7.51775).abs() < 1e-9);
        // A hole narrower than the thread's minor diameter is opened out to it (as 3692's shaft,
        // narrower than its thread, is built up to the major diameter).
        let narrow = feature(&m16(true), &face(0.7), "Thread2", true).unwrap();
        assert!((narrow["moves_face"]["to"].as_f64().unwrap() - 7.51775).abs() < 1e-9);
        // A cosmetic thread there is only recorded.
        let c = feature(&m16(true), &face(0.7), "Thread2", false).unwrap();
        assert_eq!(c["modeled"], false);
        assert!(c.get("moves_face").is_none());
    }
}
