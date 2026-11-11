//! Reading single features out of the design stream: names, parameters, extrudes, the entries a
//! fillet or chamfer picks, holes, pattern directions and mirror planes.

use super::geom::{V3, cross, dot};
use super::naming::{self, ExtGeom, RecipeKind};
use super::sketch::{self, Sketch};
use super::stream::{
    Design, Param, dbl3, finite_doubles, is_uuid_text, param, parse_header, ref_ids, u32_at, wstrings_all,
};
use super::{Ctx, PARAM, PARAM_HOLDER};
use std::rc::Rc;

/// Parameters by role name, in the order found (the first of a name wins).
pub(super) type Params = Vec<(String, Param)>;

pub(super) fn pget<'a>(ps: &'a Params, name: &str) -> Option<&'a Param> {
    ps.iter().find(|(k, _)| k == name).map(|(_, p)| p)
}

fn is_name(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic())
        && c.all(|x| x.is_ascii_alphanumeric() || matches!(x, ' ' | '_' | '-'))
}

fn printable(s: &str) -> bool {
    s.chars().all(|c| c == ' ' || (!c.is_control() && !c.is_whitespace()))
}

/// A feature's base name and display name: a UUID string (zero in newer files), the base name,
/// a `u32` index, 4 bytes, then the user's name (length 0 when none).
pub(super) fn feature_name(d: &Design, i: u64) -> (Option<String>, Option<String>) {
    let rec = d.rec(i);
    let ws = wstrings_all(rec);
    for (j, (_, e, s)) in ws.iter().enumerate() {
        let Some((o2, e2, base)) = ws.get(j + 1) else { continue };
        if !(is_uuid_text(s) && *o2 >= *e && *o2 - *e < 1024) {
            continue;
        }
        if is_uuid_text(base) || base.starts_with("Prism") || !is_name(base) {
            continue;
        }
        if *e2 + 12 > rec.len() {
            return (Some(base.clone()), Some(base.clone()));
        }
        let n = u32_at(rec, *e2).unwrap_or(u32::MAX);
        let mut name = if n < 100_000 { format!("{base}{n}") } else { base.clone() };
        let l = u32_at(rec, e2 + 8).unwrap_or(0) as usize;
        if 0 < l && l < 256 && e2 + 12 + 2 * l <= rec.len() {
            let units: Vec<u16> = rec
                .get(e2 + 12..e2 + 12 + 2 * l)
                .unwrap_or(&[])
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
                .collect();
            if let Ok(custom) = String::from_utf16(&units)
                && printable(&custom)
            {
                name = custom;
            }
        }
        return (Some(base.clone()), Some(name));
    }
    (None, None)
}

/// The model parameters a record references (`D91D429C` holders to `7A6A3D31`), named by role.
pub(super) fn params_of(d: &Design, i: u64) -> Params {
    let mut out: Params = Vec::new();
    for q in ref_ids(d, d.rec(i)) {
        if d.is(q, PARAM_HOLDER) {
            for q2 in ref_ids(d, d.rec(q)) {
                if d.is(q2, PARAM)
                    && let Some(p) = param(d, q2)
                    && let Some(n) = p.name.clone()
                    && pget(&out, &n).is_none()
                {
                    out.push((n, p));
                }
            }
        }
    }
    out
}

/// All distinct values of the parameters with these role names that record `i` references.
pub(super) fn param_values(d: &Design, i: u64, names: &[&str]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for q in ref_ids(d, d.rec(i)) {
        if d.is(q, PARAM_HOLDER) {
            for q2 in ref_ids(d, d.rec(q)) {
                if d.is(q2, PARAM)
                    && let Some(p) = param(d, q2)
                    && p.name.as_deref().is_some_and(|n| names.contains(&n))
                    && let Some(v) = p.value
                    && !out.contains(&v)
                {
                    out.push(v);
                }
            }
        }
    }
    out
}

#[derive(Debug)]
pub(super) struct ExtRaw {
    pub op: u8,
    pub extent: u8,
    pub vec: Option<V3>,
    pub sketch: Option<u64>,
    pub params: Params,
}

/// `u32 operation, u32 extent, u32 kind, 00|01 01` (position varies with the class version).
fn enum_search(rec: &[u8], from: usize, limit: usize) -> Option<usize> {
    let limit = limit.min(rec.len());
    let mut s = from;
    while s + 14 <= limit {
        let w = rec.get(s..s + 14)?;
        let z = |k: usize| w.get(k..k + 3) == Some(&[0, 0, 0]);
        if matches!(w.first(), Some(1..=5))
            && z(1)
            && matches!(w.get(4), Some(1..=4))
            && z(5)
            && matches!(w.get(8), Some(0..=3))
            && z(9)
            && matches!(w.get(12), Some(0 | 1))
            && w.get(13) == Some(&1)
        {
            return Some(s);
        }
        s += 1;
    }
    None
}

fn find_bytes(rec: &[u8], pat: &[u8]) -> Option<usize> {
    rec.windows(pat.len()).position(|w| w == pat)
}

/// The sketch a profile record (`4BD53E5A`) names: its UTF-16 decimal text is the sketch's id.
pub(super) fn profile_sketch_of(d: &Design, rec: &[u8]) -> Option<u64> {
    let mut found = None;
    for q in ref_ids(d, rec) {
        if d.is(q, "4BD53E5A") {
            for w in wstrings_all(d.rec(q)) {
                if let Ok(n) = w.2.parse::<u64>()
                    && d.has(n)
                    && d.is(n, sketch::SKETCH)
                {
                    found = Some(n);
                }
            }
        }
    }
    found
}

pub(super) fn extrude_raw(d: &Design, fid: u64) -> Option<ExtRaw> {
    let rec = d.rec(fid);
    let h = parse_header(rec)?;
    let b = h.body.saturating_sub(8).max(15);
    // A join or cut names its target's selection first: 10 bytes as a plain reference, 49 as a
    // typed one (older streams), which puts the enum block past the first window.
    let mut m = enum_search(rec, b, b + 64).or_else(|| enum_search(rec, b, b + 104));
    if m.is_none() && find_bytes(rec.get(..200.min(rec.len()))?, b"SpecifiedColor").is_some() {
        // The colour attribute is a nested object of variable size: search after its appearance name.
        let utf16: Vec<u8> = "PrismOpaque".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let k = find_bytes(rec, &utf16).or_else(|| find_bytes(rec, b"P\0r\0i\0s\0m\0"));
        if let Some(k) = k.filter(|k| *k > 0) {
            m = enum_search(rec, k, k + 200);
        }
    }
    let s = m?;
    let (op, extent) = (*rec.get(s)?, *rec.get(s + 4)?);
    let end = s + 14;
    let vec = (end..end + 40).find_map(|o| {
        let v = dbl3(rec, o)?;
        let n = super::geom::norm(v);
        ((n - 1.0).abs() < 1e-6 && v.iter().any(|x| x.abs() > 0.5)).then_some(v)
    });
    Some(ExtRaw { op, extent, vec, sketch: profile_sketch_of(d, rec), params: params_of(d, fid) })
}

impl Ctx<'_> {
    pub(super) fn sketch(&mut self, sid: u64) -> Rc<Sketch> {
        let d = self.d;
        self.sketches.entry(sid).or_insert_with(|| Rc::new(sketch::read_sketch(d, sid))).clone()
    }

    /// Sketch placement, profile pieces and cap offsets of an extrude.
    pub(super) fn ext_geom(&mut self, fid: u64) -> Option<Rc<ExtGeom>> {
        if let Some(r) = self.exts.get(&fid) {
            return r.clone();
        }
        let r = self.ext_geom_once(fid).map(Rc::new);
        self.exts.insert(fid, r.clone());
        r
    }

    fn ext_geom_once(&mut self, fid: u64) -> Option<ExtGeom> {
        let d = self.d;
        let e = extrude_raw(d, fid)?;
        let s = self.sketch(e.sketch?);
        let pl = s.plane?;
        let rg = self.rc.regions(d, fid, &s).ok()?;
        let mut segs: Vec<Rc<sketch::Seg>> = Vec::new();
        for r in rg.iter() {
            let list = match &r.naming_segments {
                Some(n) if !n.is_empty() => n,
                _ => &r.segments,
            };
            for sg in list {
                if !segs.iter().any(|x| x.id == sg.id) {
                    segs.push(sg.clone());
                }
            }
        }
        let sign = e.vec.map_or(1.0, |v| if dot(pl.z, v) >= 0.0 { 1.0 } else { -1.0 });
        let (mut t0, mut t1) = (None, None);
        if let Some(a) = pget(&e.params, "AlongDistance").and_then(|p| p.value) {
            let a = a * sign;
            if e.extent == 3 {
                (t0, t1) = (Some(-a / 2.0), Some(a / 2.0));
            } else if let (2, Some(ag)) = (e.extent, pget(&e.params, "AgainstDistance").and_then(|p| p.value)) {
                (t0, t1) = (Some(-ag * sign), Some(a));
            } else {
                (t0, t1) = (Some(0.0), Some(a));
            }
        }
        Some(ExtGeom { pl, sketch: s, segs, t0, t1 })
    }

    pub(super) fn gen_map(&mut self, fid: u64) -> Option<Rc<naming::GenMap>> {
        if let Some(r) = self.gmaps.get(&fid) {
            return r.clone();
        }
        let r = self.ext_geom(fid).and_then(|g| naming::gen_map(&g.segs)).map(Rc::new);
        self.gmaps.insert(fid, r.clone());
        r
    }

    /// Records of class `want` reachable from `fid` through child records, breadth first. A
    /// match is not followed further; `via` limits which classes are followed (`None`: all).
    pub(super) fn under(&self, fid: u64, want: &str, depth: usize, via: Option<&[&str]>) -> Vec<u64> {
        let d = self.d;
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::from([fid]);
        let mut front = vec![fid];
        for _ in 0..depth {
            let mut next = Vec::new();
            for x in front {
                for q in ref_ids(d, d.rec(x)) {
                    if !seen.insert(q) {
                        continue;
                    }
                    if d.is(q, want) {
                        out.push(q);
                    } else if via.is_none_or(|v| v.iter().any(|c| d.is(q, c))) {
                        next.push(q);
                    }
                }
            }
            front = next;
        }
        out
    }

    /// Edge recipes under a fillet/chamfer/pattern: through selection records.
    pub(super) fn input_recipes(&self, fid: u64) -> Vec<u64> {
        self.under(fid, "7ACC2A03", 4, Some(&["2CA5A1CD", "5662F619"]))
    }

    pub(super) fn edge_recipes_of(&self, fid: u64) -> Vec<u64> {
        self.under(
            fid,
            "7ACC2A03",
            3,
            Some(&["2CA5A1CD", "5662F619", "9716F783", "4B31DA1B", "5A1BF548", "6CCF41D5", "F2A7590D"]),
        )
    }

    /// Edge midpoints (mm) of a fillet/chamfer set: for each radius/distance set its value (cm),
    /// the midpoints found and the count of edges left unresolved.
    /// The value (cm) and edge recipes of each radius/distance set of a fillet/chamfer.
    fn blend_groups(&self, fid: u64, names: &[&str]) -> Vec<(Option<f64>, Vec<u64>)> {
        let d = self.d;
        // A set names a selection that names the feature back. (The edge numbers of a 2025
        // chamfer, `01 00` and a `u64`, read like references to the objects 512, 768, ...: a
        // set of another feature among them is not this one's.)
        let own = |q: &u64| {
            ref_ids(d, d.rec(*q)).into_iter().any(|s| d.is(s, "2CA5A1CD") && ref_ids(d, d.rec(s)).contains(&fid))
        };
        let sets: Vec<u64> = ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "4A557CE2") && own(q)).collect();
        if sets.is_empty() {
            vec![(param_values(d, fid, names).first().copied(), self.input_recipes(fid))]
        } else {
            sets.iter().map(|q| (param_values(d, *q, names).first().copied(), self.input_recipes(*q))).collect()
        }
    }

    /// The `idx`-th edge recipe (0-based, over all sets in order, repeats dropped) of a fillet and
    /// the radius (cm) of its set: a fillet's generator `k` names the face of its `k`-th edge.
    pub(super) fn blend_edge(&self, fid: u64, idx: usize) -> Option<(f64, u64)> {
        let mut seen = std::collections::HashSet::new();
        let mut n = 0;
        for (v, recs) in self.blend_groups(fid, &["Radius"]) {
            for rid in recs {
                if !seen.insert(rid) {
                    continue;
                }
                if n == idx {
                    return Some((v.filter(|v| *v > 0.0)?, rid));
                }
                n += 1;
            }
        }
        None
    }

    pub(super) fn blend_sets(&mut self, fid: u64, names: &[&str]) -> Vec<(Option<f64>, Vec<V3>, usize)> {
        let d = self.d;
        let groups = self.blend_groups(fid, names);
        let mut out = Vec::new();
        for (v, recs) in groups {
            let mut es: Vec<V3> = Vec::new();
            let mut seen: Vec<[i64; 3]> = Vec::new();
            let mut bad = 0;
            let mut done = std::collections::HashSet::new();
            for rid in recs {
                if !done.insert(rid) {
                    continue;
                }
                let (kind, faces) = naming::recipe(d, rid);
                let (Some(RecipeKind::Edge), Some(faces)) = (kind, faces) else { continue };
                let Some((mid, _)) = self.edge_midpoint(&faces) else {
                    bad += 1;
                    continue;
                };
                let key = [(mid[0] * 1e6).round() as i64, (mid[1] * 1e6).round() as i64, (mid[2] * 1e6).round() as i64];
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                es.push(mid);
            }
            out.push((v, es, bad));
        }
        out
    }
}

/// A plane stored as origin, x axis and y axis (3 doubles each, cm); normal = x cross y.
pub(super) fn plane_frame(d: &Design, q: u64) -> Option<(V3, V3)> {
    let rec = d.rec(q);
    for o in 15..rec.len().saturating_sub(72).min(40) {
        let Some(v) = finite_doubles::<9>(rec, o, 1e6) else { continue };
        let (origin, x, y) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]);
        if (dot(x, x) - 1.0).abs() < 1e-9 && (dot(y, y) - 1.0).abs() < 1e-9 && dot(x, y).abs() < 1e-9 {
            return Some((origin, cross(x, y)));
        }
    }
    None
}

/// A 3D point stored right after the record preamble.
pub(super) fn point3(d: &Design, q: u64) -> Option<V3> {
    let rec = d.rec(q);
    for o in [21usize, 22, 20, 23] {
        if let Some(v) = finite_doubles::<3>(rec, o, 1e6)
            && v.iter().any(|a| a.abs() > 1e-12)
        {
            return Some([v[0], v[1], v[2]]);
        }
    }
    None
}

/// The direction records of a pattern, in U, V order: point, unit direction, vector length.
pub(super) fn pattern_dirs(d: &Design, fid: u64) -> Vec<(V3, V3)> {
    let mut out = Vec::new();
    for q in ref_ids(d, d.rec(fid)) {
        if !d.is(q, "F2A7590D") {
            continue;
        }
        let rec = d.rec(q);
        let Some(v) = finite_doubles::<6>(rec, 25, 1e6) else { continue };
        let vec = [v[3], v[4], v[5]];
        let n = super::geom::norm(vec);
        if n < 1e-12 {
            continue;
        }
        out.push(([v[0], v[1], v[2]], [vec[0] / n, vec[1] / n, vec[2] / n]));
    }
    out
}

/// Per-direction symmetric flags: three doubles after the first `ff ff ff ff` of the pattern's
/// `EA0E146E` record.
pub(super) fn pattern_symmetric(d: &Design, fid: u64) -> Option<[bool; 3]> {
    for q in ref_ids(d, d.rec(fid)) {
        if !d.is(q, "EA0E146E") {
            continue;
        }
        let rec = d.rec(q);
        let Some(k) = find_bytes(rec, &[0xff; 4]) else { continue };
        if let Some(v) = finite_doubles::<3>(rec, k + 4, 1e6)
            && v.iter().all(|a| a.abs() == 0.0 || (a.abs() - 1.0).abs() == 0.0)
        {
            return Some([v[0] != 0.0, v[1] != 0.0, v[2] != 0.0]);
        }
    }
    None
}
