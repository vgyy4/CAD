//! Sketch text (`E0618268`) and the extrudes of it (section 51 of the format notes).
//!
//! * A text record holds the font's name, the height (cm, also a `D91D429C` holder), two `u32`
//!   codes (alignment or box kind; not read), the string, a `u32` code and the weight (400, 700),
//!   then a 4x4 (row-major, cm) whose first column is the baseline's direction in the sketch and
//!   whose translation is the text box's corner (5998: the stored glyphs lie up and right of it),
//!   and last the sketch record it belongs to.
//! * An extrude of text names one `92F9A5F4` profile per text; the profile's `90055C05` child
//!   ends in the text record's id (a plain `u64`, not a reference).
//! * The glyph outlines come from the font, which the file does not hold. A cut of text stores
//!   its tools: one solid per text (5998: four glyph prisms, exactly the cut's depth deep, from
//!   the sketch plane). Where every text has its stored tools, the tree cuts (or joins) with
//!   those solids and keeps the text as a suppressed `text` feature in a font Gusset has, so it
//!   stays editable; without them the text feature does the work in that font, and the rebuild
//!   check judges it.

use super::Ctx;
use super::acis;
use super::geom::{self, V3, mm, round9};
use super::sketch;
use super::stream::{f64_at, ref_end, ref_ids, refs_at, u32_at, u64_at};
use crate::cover;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub(super) const TEXT: &str = "E0618268";
const TEXT_PROFILE: &str = "92F9A5F4";
const TEXT_LINK: &str = "90055C05";

/// Longest text read (characters), as the document allows.
const MAX_CHARS: usize = 200;
/// Most texts one extrude is read with.
const MAX_TEXTS: usize = 64;

/// One sketch text as the record holds it (lengths in cm, in the sketch's own frame).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SketchText {
    pub(super) text: String,
    pub(super) font: String,
    pub(super) height: f64,
    /// The text box's corner (sketch coordinates).
    pub(super) at: [f64; 2],
    /// The baseline's direction (unit, sketch coordinates).
    pub(super) dir: [f64; 2],
    pub(super) weight: u32,
    pub(super) sketch: u64,
}

/// A text record: the font name is the first string, then `u8 0`, the height, the holder
/// reference, `u8 0`, a `u32` code, three zero bytes, the string (`u32` length + UTF-16), a `u32`
/// code, `u8 0`, the weight, five zero bytes and the 4x4. The last reference names the sketch.
pub(super) fn read_text(d: &super::stream::Design, id: u64) -> Result<SketchText, String> {
    let rec = d.rec(id);
    let (_, font_end, font) = super::stream::wstrings_all(rec).into_iter().next().ok_or("the text names no font")?;
    let height = f64_at(rec, font_end + 1).filter(|h| h.is_finite() && *h > 0.0).ok_or("the text has no height")?;
    cover::field(rec, font_end + 1, 8, "text.height");
    let (holder, after) = ref_end(d, rec, font_end + 9).ok_or("the text names no height parameter")?;
    if !d.is(holder, "D91D429C") {
        return Err("the text's height is not a parameter".into());
    }
    let n = u32_at(rec, after + 8).map(|n| n as usize).filter(|n| (1..=MAX_CHARS).contains(n));
    let n = n.ok_or("the text's string is empty or too long")?;
    let start = after + 12;
    let units: Vec<u16> = rec
        .get(start..start + 2 * n)
        .ok_or("the text's string runs past its record")?
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
        .collect();
    let text = String::from_utf16(&units).map_err(|_| "the text's string is not UTF-16")?;
    if text.chars().any(char::is_control) || text.trim().is_empty() {
        return Err("the text holds control characters or only spaces".into());
    }
    cover::field(rec, after + 8, 4 + 2 * n, "text.string");
    cover::field(rec, after + 1, 4, "text.code1");
    let end = start + 2 * n;
    cover::field(rec, end, 4, "text.code2");
    let weight = u32_at(rec, end + 5).ok_or("the text has no weight")?;
    cover::field(rec, end + 5, 4, "text.weight");
    let at = end + 14;
    let mut m = [0.0f64; 16];
    for (k, v) in m.iter_mut().enumerate() {
        *v = f64_at(rec, at + 8 * k).filter(|v| v.is_finite()).ok_or("the text has no placement")?;
    }
    cover::field(rec, at, 128, "text.placement");
    // A turn in the sketch plane and a shift: no z, no scale (6124's carry 3e-14 out of the
    // plane and 7e-15 cm along its normal).
    let (c, s) = (m[0], m[4]);
    let tiny = |v: f64| v.abs() < 1e-9;
    let rigid = ((c * c + s * s) - 1.0).abs() < 1e-6
        && (m[1] + s).abs() < 1e-6
        && (m[5] - c).abs() < 1e-6
        && [m[2], m[6], m[8], m[9], m[11]].into_iter().all(tiny)
        && (m[10] - 1.0).abs() < 1e-9
        && m.get(12..) == Some(&[0.0, 0.0, 0.0, 1.0][..]);
    if !rigid {
        return Err("the text's placement is not a turn in its sketch plane".into());
    }
    let sketch = refs_at(d, rec, 0)
        .into_iter()
        .map(|(_, q)| q)
        .rfind(|q| d.is(*q, sketch::SKETCH))
        .ok_or("the text names no sketch")?;
    let len = libm::hypot(c, s);
    Ok(SketchText {
        text,
        font: font.trim().to_string(),
        height,
        at: [m[3], m[7]],
        dir: [c / len, s / len],
        weight,
        sketch,
    })
}

/// The texts an extrude's profiles name: `None` when it names no text profile, `Err` when some
/// of its profiles are text and others are not, or a text cannot be read.
pub(super) fn extrude_texts(d: &super::stream::Design, fid: u64) -> Option<Result<Vec<(u64, SketchText)>, String>> {
    let profs: Vec<u64> = {
        let mut v = Vec::new();
        for q in ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, TEXT_PROFILE)) {
            if !v.contains(&q) {
                v.push(q);
            }
        }
        v
    };
    if profs.is_empty() {
        return None;
    }
    if ref_ids(d, d.rec(fid)).into_iter().any(|q| d.is(q, "4BD53E5A")) {
        return Some(Err("it extrudes text and sketch regions together".into()));
    }
    if profs.len() > MAX_TEXTS {
        return Some(Err(format!("it extrudes more than {MAX_TEXTS} texts")));
    }
    let mut out = Vec::new();
    for p in profs {
        let link = ref_ids(d, d.rec(p)).into_iter().find(|q| d.is(*q, TEXT_LINK));
        let id = link.and_then(|l| {
            let rec = d.rec(l);
            rec.len().checked_sub(8).and_then(|o| u64_at(rec, o))
        });
        let Some(id) = id.filter(|id| d.has(*id) && d.is(*id, TEXT)) else {
            return Some(Err("a text profile does not name a sketch text".into()));
        };
        if out.iter().any(|(t, _)| *t == id) {
            continue;
        }
        match read_text(d, id) {
            Ok(t) => out.push((id, t)),
            Err(why) => return Some(Err(why)),
        }
    }
    if out.windows(2).any(|w| matches!(w, [a, b] if a.1.sketch != b.1.sketch)) {
        return Some(Err("its texts lie in different sketches".into()));
    }
    Some(Ok(out))
}

/// The font Gusset uses for a font the file names (it holds no font): `mono` for a fixed-pitch
/// family, `sans` for the rest.
pub(super) fn substitute(font: &str) -> &'static str {
    let f = font.to_ascii_lowercase();
    let mono = ["mono", "courier", "consol", "code", "typewriter", "fixed", "terminal", "ocr"];
    if mono.iter().any(|m| f.contains(m)) { "mono" } else { "sans" }
}

/// A stored solid that is a tool of a text extrude: the blob, the body's place in it, the
/// blob's body count, and the box of the body in the sketch frame (u, v ranges, cm).
#[derive(Debug, Clone)]
struct Tool {
    blob: String,
    index: usize,
    count: usize,
    lo: [f64; 2],
    hi: [f64; 2],
}

/// The stored tools of the texts: per text, the solids of the file that span exactly the
/// extrude's reach from the sketch plane and whose box, grown by the text's height, holds the
/// text's corner; every such solid must belong to one text only. `Ok(None)` where no text has a
/// tool; `Err` where some have and others have none, or a solid fits several texts.
fn stored_tools(
    cx: &Ctx,
    pl: &geom::Plane,
    reach: (f64, f64),
    texts: &[(u64, SketchText)],
) -> Result<Option<Vec<Vec<Tool>>>, String> {
    // The box test needs the sketch's axes along the blob's: a box of a turned prism is wider
    // than the prism.
    let axis_of = |v: V3| (0..3).find(|i| v.get(*i).is_some_and(|c| (c.abs() - 1.0).abs() < 1e-9));
    let (Some(ax), Some(ay), Some(az)) = (axis_of(pl.x), axis_of(pl.y), axis_of(pl.z)) else {
        return Err("its sketch plane is not along the model's axes, so its stored glyphs are not found".into());
    };
    let comp = |p: V3, i: usize, axis: V3| p.get(i).copied().unwrap_or(0.0) * axis.get(i).copied().unwrap_or(0.0);
    let (lo_w, hi_w) = (reach.0.min(reach.1), reach.0.max(reach.1));
    let mut seen: Vec<Tool> = Vec::new();
    for (name, bytes) in cx.acis {
        let Some(shapes) = acis::body_shapes(bytes) else { continue };
        let count = shapes.len();
        let blob = name.rsplit('/').next().unwrap_or(name).to_string();
        for (index, sh) in shapes.iter().enumerate() {
            let Some((lo, hi)) = sh.vertex_box.filter(|_| !sh.flat() && !sh.transformed && sh.lumps > 0) else {
                continue;
            };
            // Sketch coordinates of the box: each axis is one of the model's, either way.
            let rel = |p: V3| geom::sub(p, pl.origin);
            let span = |i: usize, axis: V3| {
                let (a, b) = (comp(rel(lo), i, axis), comp(rel(hi), i, axis));
                (a.min(b), a.max(b))
            };
            let (u, v, w) = (span(ax, pl.x), span(ay, pl.y), span(az, pl.z));
            // The side sheets of the glyphs (5998 stores them too) are a few nanometres wide.
            if (w.0 - lo_w).abs() > 1e-7 || (w.1 - hi_w).abs() > 1e-7 || u.1 - u.0 < 1e-3 || v.1 - v.0 < 1e-3 {
                continue;
            }
            let tool = Tool { blob: blob.clone(), index, count, lo: [u.0, v.0], hi: [u.1, v.1] };
            // The same solid in another blob (a body blob and a history blob).
            let same = |t: &Tool| {
                t.lo.iter().chain(&t.hi).zip(tool.lo.iter().chain(&tool.hi)).all(|(a, b)| (a - b).abs() < 1e-9)
            };
            if !seen.iter().any(same) {
                seen.push(tool);
            }
        }
    }
    // The share of a solid's box inside the box a text takes, roughly: from its corner along the
    // baseline 0.8 of a height per character, and from half a height below the corner to one and
    // a half above (5998's glyphs lie up and right of their corners, within a third of a height,
    // and are at most 0.66 of a height wide).
    let share = |t: &Tool, x: &SketchText| -> f64 {
        let h = x.height;
        let n = x.text.chars().count() as f64;
        let (along, up) = (x.dir, [-x.dir[1], x.dir[0]]);
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in [[t.lo[0], t.lo[1]], [t.hi[0], t.lo[1]], [t.lo[0], t.hi[1]], [t.hi[0], t.hi[1]]] {
            let r = [p[0] - x.at[0], p[1] - x.at[1]];
            let q = [r[0] * along[0] + r[1] * along[1], r[0] * up[0] + r[1] * up[1]];
            for i in 0..2 {
                if let (Some(l), Some(hh), Some(v)) = (lo.get_mut(i), hi.get_mut(i), q.get(i)) {
                    *l = l.min(*v);
                    *hh = hh.max(*v);
                }
            }
        }
        let (bl, bh) = ([-0.15 * h, -0.5 * h], [(0.8 * n + 0.2) * h, 1.5 * h]);
        let over = |a: f64, b: f64, c: f64, d: f64| (b.min(d) - a.max(c)).max(0.0);
        let area = (hi[0] - lo[0]) * (hi[1] - lo[1]);
        let common = over(lo[0], hi[0], bl[0], bh[0]) * over(lo[1], hi[1], bl[1], bh[1]);
        if area > 0.0 { common / area } else { 0.0 }
    };
    let mut per_text: Vec<Vec<Tool>> = vec![Vec::new(); texts.len()];
    for t in &seen {
        let mut by: Vec<(f64, usize)> =
            texts.iter().enumerate().map(|(k, (_, x))| (share(t, x), k)).filter(|(f, _)| *f > 0.0).collect();
        by.sort_by(|a, b| b.0.total_cmp(&a.0));
        match by.as_slice() {
            [] => {}
            // Mostly inside one text's box, and far less inside any other's.
            [(f, k), rest @ ..] if *f >= 0.5 && rest.first().is_none_or(|(g, _)| *g <= f / 4.0) => {
                if let Some(v) = per_text.get_mut(*k) {
                    v.push(t.clone());
                }
            }
            [(f, _), ..] if *f < 0.25 => {}
            _ => return Err("a stored solid of the extrude's depth lies by several of its texts".into()),
        }
    }
    if per_text.iter().all(Vec::is_empty) {
        return Ok(None);
    }
    if per_text.iter().any(Vec::is_empty) {
        return Err("some of its texts have stored glyphs and others none".into());
    }
    Ok(Some(per_text))
}

/// An extrude of sketch text as tree features. `j` is the extrude as read (distance, start,
/// operation, target). With the texts' stored tools: a suppressed `text` per text, a
/// `stored_body` per tool and a `combine` named after the extrude; without: a `text` per text that
/// does the extrude's work. `Err` where the extrude is not one these rules cover.
pub(super) fn text_extrude(
    cx: &mut Ctx,
    fid: u64,
    j: &Map<String, Value>,
    texts: &[(u64, SketchText)],
    sketch_names: &HashMap<u64, String>,
) -> Result<Vec<Value>, String> {
    let d = cx.d;
    let name = j.get("name").and_then(Value::as_str).unwrap_or("Text").to_string();
    let sid = texts.first().map(|(_, t)| t.sketch).ok_or("it names no text")?;
    let sketch = sketch_names.get(&sid).ok_or("its sketch was not read")?.clone();
    let pl = cx.sketch(sid).plane.ok_or("its sketch has no plane")?;
    if j.get("symmetric").and_then(Value::as_bool) == Some(true)
        || j.contains_key("two_sided")
        || j.contains_key("taper_deg")
        || j.contains_key("error")
    {
        return Err("a text extrude that is symmetric, two-sided or tapered is not read".into());
    }
    let distance = j.get("distance").and_then(Value::as_f64).filter(|t| *t != 0.0).ok_or("it has no distance")?;
    let raw = super::feature::extrude_raw(d, fid).ok_or("its layout is not recognised")?;
    let start = cx.extrude_start(fid, &raw, &pl)?;
    let op = j.get("operation").and_then(Value::as_str).unwrap_or("").to_string();
    let target = j.get("target").cloned().unwrap_or(Value::Null);
    if op != "new" && !target.is_string() {
        return Err("the body it changes is not one body of one feature".into());
    }
    let tools = stored_tools(cx, &pl, (start, start + distance / 10.0), texts)?;
    let mut out = Vec::new();
    let one = texts.len() == 1;
    for (k, (tid, t)) in texts.iter().enumerate() {
        let font = substitute(&t.font);
        let tname = if one && tools.is_none() { name.clone() } else { format!("{name} text {}", k + 1) };
        // Where the file stores the glyphs, the substitute is centred on them; else it starts at
        // the text box's corner.
        let (at, align) = match tools.as_ref().and_then(|ts| ts.get(k)) {
            Some(ts) => {
                let lo = ts.iter().fold([f64::INFINITY; 2], |a, x| [a[0].min(x.lo[0]), a[1].min(x.lo[1])]);
                let hi = ts.iter().fold([f64::NEG_INFINITY; 2], |a, x| [a[0].max(x.hi[0]), a[1].max(x.hi[1])]);
                let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                // The baseline half a capital's height below the middle, across the direction.
                let up = [-t.dir[1], t.dir[0]];
                ([c[0] - up[0] * t.height / 2.0, c[1] - up[1] * t.height / 2.0], "center")
            }
            None => (t.at, "left"),
        };
        let mut f = json!({"type": "text", "name": tname, "source_id": fid, "text_source_id": tid,
            "sketch": sketch, "text": t.text, "font": font, "source_font": t.font,
            "height": mm(t.height), "at": [mm(at[0]), mm(at[1])],
            "direction": [round9(t.dir[0]), round9(t.dir[1])], "align": align,
            "distance": distance, "operation": op, "target": target});
        if let Some(o) = f.as_object_mut() {
            if start != 0.0 {
                o.insert("start".into(), mm(start).into());
            }
            if t.weight >= 600 {
                o.insert("source_weight".into(), t.weight.into());
            }
            if tools.is_some() {
                o.insert("suppressed".into(), true.into());
                o.insert(
                    "note".into(),
                    "the glyphs the file stores make the extrude; this text is kept to edit".into(),
                );
            }
        }
        out.push(f);
    }
    if let Some(per_text) = tools {
        let mut names = Vec::new();
        for (k, ts) in per_text.iter().enumerate() {
            for (i, t) in ts.iter().enumerate() {
                let bn = if ts.len() == 1 {
                    format!("{name} glyphs {}", k + 1)
                } else {
                    format!("{name} glyphs {} {}", k + 1, i + 1)
                };
                out.push(json!({"type": "stored_body", "name": bn, "source_id": fid, "brep": t.blob,
                    "index": t.index, "count": t.count, "creates_bodies": 1,
                    "note": "a glyph solid of a sketch text, as the file stores it"}));
                names.push(bn);
            }
        }
        let op = match op.as_str() {
            "join" | "cut" | "intersect" => op,
            _ => return Err("a new body of stored glyphs is not read".into()),
        };
        out.push(json!({"type": "combine", "name": name, "source_id": fid, "target": target, "tools": names,
            "operation": op, "note": "the extrude of sketch text, with the glyph solids the file stores"}));
    }
    Ok(out)
}
