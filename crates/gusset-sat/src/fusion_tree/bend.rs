//! Sheet-metal Bend `1CF7B9E3` (sections 33 and 51 of the format notes): a flat stretch of a
//! sheet body bent along a sketch line.
//!
//! * The record names its parameters `BendAngle` (signed; -90 deg in both of 3502's bends) and
//!   `BendRadius` (the inside radius), the sheet-metal rule (`E75C86D6` -> `B42ED662`: the rule's
//!   `Thickness` parameter and, as a double right after the rule's 36-character id text, its
//!   K-factor, 0.44), the face the line lies on (`2CA5A1CD` -> `5662F619` -> the body link
//!   `D26351F0`), the line (`88277bc9` -> `2CA5A1CD` -> `5A1BF548` -> `E2CEFD18`, read as a
//!   sweep path), and in a `69EE2FA7` the face that stays: two doubles (not read), a zero byte
//!   and a point of that face (cm, in the frame of the body's component).
//! * The `88277bc9`'s body is `01 00`, then a `u32`: 2 in both bends, whose zones end at their
//!   lines on the side that stays (the stored bent flex fits only that: its first straight is
//!   41.586 mm of the 49 mm to the line, the zone the 7.414 mm left; section 51). Other codes
//!   are refused.

use super::Ctx;
use super::feature::{params_of, pget};
use super::geom::{mm, round9};
use super::stream::{f64_at, parse_header, ref_ids, u32_at};
use crate::cover;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub(super) const BEND: &str = "1CF7B9E3";

/// The body links (`D26351F0`) the face selections under `fid` name, to `depth` levels.
pub(super) fn face_links(d: &super::stream::Design, fid: u64) -> Vec<u64> {
    let mut out = Vec::new();
    for q in ref_ids(d, d.rec(fid)) {
        if !(d.is(q, "2CA5A1CD") || d.is(q, "69EE2FA7")) {
            continue;
        }
        for f in ref_ids(d, d.rec(q)).into_iter().filter(|f| d.is(*f, "5662F619")) {
            for l in ref_ids(d, d.rec(f)).into_iter().filter(|l| d.is(*l, "D26351F0")) {
                if !out.contains(&l) {
                    out.push(l);
                }
            }
        }
    }
    out
}

/// The K-factor of a sheet-metal rule `B42ED662`: the double right after its id text (a
/// 36-character UTF-16 string), between 0 and 1.
fn k_factor(d: &super::stream::Design, rule: u64) -> Option<f64> {
    let rec = d.rec(rule);
    let (_, end, _) = super::stream::wstrings_all(rec).into_iter().find(|(_, _, s)| super::stream::is_uuid_text(s))?;
    let k = f64_at(rec, end).filter(|k| (0.0..=1.0).contains(k))?;
    cover::field(rec, end, 8, "sheet-rule.k-factor");
    Some(k)
}

pub(super) fn bend_json(
    cx: &mut Ctx,
    fid: u64,
    name: &str,
    sketch_names: &HashMap<u64, String>,
    target: Option<String>,
) -> Map<String, Value> {
    let d = cx.d;
    let mut j = Map::new();
    j.insert("type".into(), "bend".into());
    j.insert("name".into(), name.into());
    j.insert("source_id".into(), fid.into());
    let fail = |mut j: Map<String, Value>, why: &str| {
        j.insert("error".into(), why.into());
        j
    };
    let ps = params_of(d, fid);
    let val = |n: &str| pget(&ps, n).and_then(|p| p.value);
    let (Some(angle), Some(radius)) = (val("BendAngle"), val("BendRadius")) else {
        return fail(j, "the bend has no angle or radius");
    };
    if !(angle.is_finite() && angle != 0.0 && angle.abs() <= std::f64::consts::PI + 1e-9 && radius >= 0.0) {
        return fail(j, "the bend's angle or radius is out of range");
    }
    let refs = ref_ids(d, d.rec(fid));
    let rule = refs
        .iter()
        .filter(|q| d.is(**q, "E75C86D6"))
        .flat_map(|q| ref_ids(d, d.rec(*q)))
        .find(|q| d.is(*q, "B42ED662"));
    let Some(k) = rule.and_then(|r| k_factor(d, r)) else {
        return fail(j, "the bend's sheet-metal rule has no K-factor");
    };
    let thickness = rule.and_then(|r| pget(&params_of(d, r), "Thickness").and_then(|p| p.value));
    // The line, and where the zone lies against it.
    let Some(holder) = refs.iter().copied().find(|q| d.is(*q, "88277bc9")) else {
        return fail(j, "the bend names no line");
    };
    let hrec = d.rec(holder);
    let code = parse_header(hrec).and_then(|h| (hrec.get(h.body..h.body + 2) == Some(&[1, 0])).then_some(h.body + 2));
    let code = code.and_then(|at| {
        cover::field(hrec, at, 4, "bend.position");
        u32_at(hrec, at)
    });
    if code != Some(2) {
        return fail(j, "the bend's zone position is not the one seen (its zone ending at the line)");
    }
    let Some(sel) = ref_ids(d, hrec).into_iter().find(|q| d.is(*q, "2CA5A1CD")) else {
        return fail(j, "the bend names no line");
    };
    let (sk, curves) = match super::json::sweep_path(cx, sel) {
        Ok(p) => p,
        Err(_) => return fail(j, "the bend's line is not a sketch line"),
    };
    let [curve] = curves.as_slice() else { return fail(j, "the bend names several curves") };
    let sketch = cx.sketch(sk);
    if !sketch.lines.contains_key(curve) {
        return fail(j, "the bend's curve is not a line");
    }
    let Some(sname) = sketch_names.get(&sk) else { return fail(j, "the bend's sketch was not read") };
    // The point of the face that stays.
    let keep = refs.iter().copied().find(|q| d.is(*q, "69EE2FA7")).and_then(|q| {
        let r = d.rec(q);
        let h = parse_header(r)?;
        let at = h.body + 17;
        (r.get(h.body + 16) == Some(&0)).then_some(())?;
        cover::field(r, at, 24, "bend.keep-point");
        Some([f64_at(r, at)?, f64_at(r, at + 8)?, f64_at(r, at + 16)?]).filter(|p| p.iter().all(|c| c.is_finite()))
    });
    let Some(keep) = keep else { return fail(j, "the bend names no face that stays") };
    let Some(target) = target else { return fail(j, "the body it bends is not one body of one feature") };
    j.insert("target".into(), target.into());
    j.insert("sketch".into(), sname.as_str().into());
    j.insert("line".into(), format!("c{curve}").into());
    j.insert("angle".into(), round9(angle.to_degrees()).into());
    j.insert("bend_radius".into(), mm(radius).into());
    j.insert("k_factor".into(), k.into());
    if let Some(t) = thickness {
        j.insert("thickness".into(), mm(t).into());
    }
    j.insert("position".into(), "end".into());
    j.insert("keep_point".into(), json!([mm(keep[0]), mm(keep[1]), mm(keep[2])]));
    j
}
