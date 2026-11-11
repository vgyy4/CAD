//! Thread `584D9526` (layout notes, section 29): the thread's designation and form, read to say
//! whether it is cosmetic (no geometry: the tree leaves it out) or modeled (helical faces the
//! tree has no feature for: refused with what it is).

use super::Ctx;
use super::acis;
use super::feature::{params_of, pget};
use super::geom::{dot, norm};
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

/// The tree's word on a thread: `Ok(note)` for a cosmetic one (left out of the tree, the note
/// for the skipped list), `Err(feature)` for one the tree cannot hold.
pub(super) fn json(cx: &Ctx, fid: u64, name: &str, fusion_type: &str, class: &str) -> Result<String, Value> {
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
    if modeled(cx, &t) {
        return Err(json!({"type": "unsupported", "fusion_type": fusion_type, "name": name, "class_guid": class,
            "source_id": fid, "thread": what.clone(),
            "error": format!("a modeled thread ({what}): the stored bodies hold its helical faces, and the tree has no thread form")}));
    }
    Ok(format!("a cosmetic thread ({what}): no helical face of it is stored, it changes no geometry"))
}
