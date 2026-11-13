//! Reading single features out of the design stream: names, parameters, extrudes, the entries a
//! fillet or chamfer picks, holes, pattern directions and mirror planes.

use super::geom::{Plane, V3, cross, dot};
use super::naming::{self, ExtGeom, RecipeKind};
use super::sketch::{self, Sketch};
use super::stream::{
    Design, Param, dbl3, finite_doubles, is_uuid_text, param, parse_header, ref_ids, u32_at, wstrings_all,
};
use super::{Ctx, PARAM, PARAM_HOLDER};
use crate::cover;
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
        if is_uuid_text(base) || base.starts_with("Prism") || !(is_name(base) || base == "Body->Comp") {
            continue;
        }
        if *e2 + 12 > rec.len() {
            return (Some(base.clone()), Some(base.clone()));
        }
        let n = u32_at(rec, *e2).unwrap_or(u32::MAX);
        cover::field(rec, *e2, 4, "feature.index");
        cover::skip(rec, e2 + 4, 4, "feature.name-gap");
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
                cover::field(rec, e2 + 8, 4 + 2 * l, "feature.user-name");
                name = custom;
            }
        } else if l == 0 {
            cover::field(rec, e2 + 8, 4, "feature.user-name");
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
    /// A symmetric extent's measurement: `Some(true)` where its distance is each side's,
    /// `Some(false)` where it is the whole length, `None` where it was not found.
    pub half_length: Option<bool>,
    /// The `u32` after the flags: 0 the profile plane, 1 an offset, 2 an object.
    pub start_kind: Option<u8>,
    pub sketch: Option<u64>,
    pub params: Params,
}

/// `u32 operation, u32 extent, u32 kind, 00|01 01` (position varies with the class version).
/// The second flag is 0 in some extrudes that start at an offset (2530's Extrude2 and Extrude3,
/// 4632's 5067): there the `u32` start kind that follows is 1 or 2, never 0.
fn enum_search(rec: &[u8], from: usize, limit: usize) -> Option<usize> {
    let limit = limit.min(rec.len());
    let mut s = from;
    while s + 14 <= limit {
        let w = rec.get(s..(s + 18).min(rec.len()))?;
        let z = |k: usize| w.get(k..k + 3) == Some(&[0, 0, 0]);
        let flag = match w.get(13) {
            Some(1) => true,
            Some(0) => matches!(w.get(14), Some(1 | 2)) && z(15),
            _ => false,
        };
        if matches!(w.first(), Some(1..=5))
            && z(1)
            && matches!(w.get(4), Some(1..=4))
            && z(5)
            && matches!(w.get(8), Some(0..=3))
            && z(9)
            && matches!(w.get(12), Some(0 | 1))
            && flag
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
    cover::field(rec, s, 4, "extrude.operation");
    cover::field(rec, s + 4, 4, "extrude.extent");
    cover::skip(rec, s + 8, 6, "extrude.kind-flags");
    let end = s + 14;
    let dir = (end..end + 40).find_map(|o| {
        let v = dbl3(rec, o)?;
        let n = super::geom::norm(v);
        ((n - 1.0).abs() < 1e-6 && v.iter().any(|x| x.abs() > 0.5)).then(|| {
            cover::field(rec, o, 24, "extrude.direction");
            (o, v)
        })
    });
    let half_length = match dir {
        Some((o, _)) if extent == 3 => symmetric_measure(rec, o + 24),
        _ => None,
    };
    Some(ExtRaw {
        op,
        extent,
        vec: dir.map(|(_, v)| v),
        half_length,
        start_kind: u32_at(rec, end).filter(|k| *k <= 2).and_then(|k| u8::try_from(k).ok()),
        sketch: profile_sketch_of(d, rec),
        params: params_of(d, fid),
    })
}

/// A symmetric extrude's measurement, read after its direction (`from`): a `u32`, 1 for the
/// whole length and 2 for each side's, followed by zeros (11 bytes in the records with plain
/// references, none in those with typed ones) and the feature's selection list (`u32 1` and a
/// reference). Each side's: 6299 Extrude2 (2) (11 mm, its stored tool 22 mm long), 324
/// Extrude1 and Extrude11 (16.5 and 6 mm, bodies 33 and 12 mm long); the whole length:
/// 4632 Extrude1 (12.4 mm, its body 12.4 mm long) and the L-Bracket's 110 mm. In records with
/// typed references and a start kind (817's Extrude2 and Extrude3, "( 4.2 / 2 ) * 1 mm") the
/// list stands 236 bytes after the direction, past five typed references.
pub(super) fn symmetric_measure(rec: &[u8], from: usize) -> Option<bool> {
    let tail = rec.get(from..)?;
    // The selection list: `01 00 00 00`, then a reference `01 <u32 id> 00 00 00 00`.
    let list = (0..tail.len().min(320)).find(|i| {
        tail.get(*i..i + 5) == Some(&[1, 0, 0, 0, 1][..]) && tail.get(i + 9..i + 13) == Some(&[0, 0, 0, 0][..])
    })?;
    // The zeros before it: the measure's three high bytes and the gap.
    let zeros = tail.get(..list)?.iter().rev().take_while(|b| **b == 0).count();
    if !matches!(zeros, 3 | 14) {
        return None;
    }
    let at = list.checked_sub(zeros + 1)?;
    cover::field(rec, from + at, 4, "extrude.symmetric-measure");
    match tail.get(at)? {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
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
        // A start face that names this extrude itself must not recurse.
        self.exts.insert(fid, None);
        let r = self.ext_geom_once(fid).map(Rc::new);
        self.exts.insert(fid, r.clone());
        r
    }

    /// Where an extrude starts (cm along its sketch plane's normal; 0 on the sketch plane).
    /// A `ProfileOffset` parameter marks a start off the profile plane: with no face recipe it
    /// is that offset (an offset plane), with one face recipe (and no `Side1Offset` end, whose
    /// target is a face recipe too) the plane of that face, a cap or planar side, plus the
    /// offset ("from object"; every such start in the corpus has offset 0). The offset counts
    /// along the normal like `AlongDistance`. `Err` where the start is a face that is not a
    /// known plane parallel to the sketch, or which face is the start is not known.
    pub(super) fn extrude_start(&mut self, fid: u64, e: &ExtRaw, pl: &Plane) -> Result<f64, &'static str> {
        let Some(off) = pget(&e.params, "ProfileOffset") else { return Ok(0.0) };
        let off = off.value.ok_or("the start offset has no value")?;
        let d = self.d;
        let mut faces = Vec::new();
        for rid in self.edge_recipes_of(fid) {
            if let (Some(RecipeKind::BoundedFace | RecipeKind::Face), Some(names)) = naming::recipe(d, rid) {
                faces.push(names);
            }
        }
        let to_object = pget(&e.params, "Side1Offset").is_some();
        match (faces.as_slice(), to_object) {
            // From a construction plane (817's Extrude2 and Extrude3: start kind 2, the selection
            // `2CA5A1CD` -> `5A1BF548` -> `CC54ECAD`, a plane through the middle of the box they
            // join, where the stored tabs are centred): that plane, parallel to the sketch, plus
            // the offset.
            ([], false) if e.start_kind == Some(2) => {
                // The record names its selections twice (in its body and in its list).
                let holders: std::collections::BTreeSet<u64> = ref_ids(d, d.rec(fid))
                    .into_iter()
                    .filter(|q| d.is(*q, "2CA5A1CD"))
                    .flat_map(|q| ref_ids(d, d.rec(q)))
                    .filter(|q| d.is(*q, "5A1BF548"))
                    .flat_map(|q| ref_ids(d, d.rec(q)))
                    .filter(|q| d.is(*q, "CC54ECAD"))
                    .collect();
                let planes: Vec<(V3, V3)> = holders.into_iter().filter_map(|q| plane_frame(d, q)).collect();
                match planes.as_slice() {
                    [] => Ok(off),
                    [(o, n)] if (dot(*n, pl.z).abs() - 1.0).abs() < 1e-9 => {
                        Ok(dot(*n, super::geom::sub(*o, pl.origin)) / dot(*n, pl.z) + off)
                    }
                    _ => Err("its start plane is not one plane parallel to the sketch"),
                }
            }
            ([], _) | ([_], true) => Ok(off),
            ([names], false) => {
                let (g, ops) = names.first().and_then(|f| f.first()).ok_or("the start face has no name")?;
                match self.face(g, ops) {
                    Some(naming::FaceG::Plane { n, p, .. }) if (dot(n, pl.z).abs() - 1.0).abs() < 1e-9 => {
                        Ok(dot(n, super::geom::sub(p, pl.origin)) / dot(n, pl.z) + off)
                    }
                    _ => Err("its start face is not a plane parallel to the sketch"),
                }
            }
            // From a face to a face, both offsets 0 and no taper (5358's pad and its clearance):
            // the solid is the slab between the two planes whichever is the start. The first
            // face recipe is taken for the start (its selection stands before the
            // `ProfileOffset` holder in the record, the other before `Side1Offset`'s) and the
            // to-object end reads the second.
            ([first, _], true)
                if off == 0.0
                    && pget(&e.params, "Side1Offset").and_then(|p| p.value) == Some(0.0)
                    && pget(&e.params, "TaperAngle").and_then(|p| p.value).is_none_or(|t| t == 0.0) =>
            {
                let (g, ops) = first.first().and_then(|f| f.first()).ok_or("the start face has no name")?;
                match self.face(g, ops) {
                    Some(naming::FaceG::Plane { n, p, .. }) if (dot(n, pl.z).abs() - 1.0).abs() < 1e-9 => {
                        Ok(dot(n, super::geom::sub(p, pl.origin)) / dot(n, pl.z))
                    }
                    _ => Err("its start face is not a plane parallel to the sketch"),
                }
            }
            _ => Err("its start and end are both faces"),
        }
    }

    fn ext_geom_once(&mut self, fid: u64) -> Option<ExtGeom> {
        let d = self.d;
        let e = extrude_raw(d, fid)?;
        let s = self.sketch(e.sketch?);
        let pl = s.plane?;
        let rg = self.rc.regions(d, fid, &s).ok()?;
        let mut segs: Vec<Rc<sketch::Seg>> = Vec::new();
        let mut uses: Vec<(Rc<sketch::Seg>, usize)> = Vec::new();
        for r in rg.iter() {
            for sg in &r.segments {
                match uses.iter_mut().find(|(x, _)| x.id == sg.id) {
                    Some((_, k)) => *k += 1,
                    None => uses.push((sg.clone(), 1)),
                }
            }
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
        // The distances count along the sketch normal, signed, whatever the stored direction
        // vector says (see `extrude_json`).
        let (mut t0, mut t1) = (None, None);
        if let Some(a) = pget(&e.params, "AlongDistance").and_then(|p| p.value) {
            let s0 = self.extrude_start(fid, &e, &pl).ok()?;
            if e.extent == 3 {
                let side = if e.half_length? { a } else { a / 2.0 };
                (t0, t1) = (Some(s0 - side), Some(s0 + side));
            } else if let (2, Some(ag)) = (e.extent, pget(&e.params, "AgainstDistance").and_then(|p| p.value)) {
                (t0, t1) = (Some(s0 - ag), Some(s0 + a));
            } else {
                (t0, t1) = (Some(s0), Some(s0 + a));
            }
        }
        let tapered = pget(&e.params, "TaperAngle").and_then(|p| p.value).is_some_and(|t| t != 0.0);
        let bounds = uses.into_iter().filter(|(_, k)| *k == 1).map(|(sg, _)| sg).collect();
        Some(ExtGeom { pl, sketch: s, segs, t0, t1, tapered, bounds })
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
        let mut sets: Vec<u64> =
            ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "4A557CE2") && own(q)).collect();
        if sets.is_empty() {
            // A set whose selection names no feature back (5360's `Fillet - Contacts`): this
            // feature's, where its selections name no other blend either.
            let other = |q: &u64| {
                ref_ids(d, d.rec(*q)).into_iter().filter(|s| d.is(*s, "2CA5A1CD")).any(|s| {
                    ref_ids(d, d.rec(s)).into_iter().any(|f| f != fid && (d.is(f, "A07D5F17") || d.is(f, "F757D611")))
                })
            };
            sets = ref_ids(d, d.rec(fid))
                .into_iter()
                .filter(|q| d.is(*q, "4A557CE2") && !other(q) && !self.input_recipes(*q).is_empty())
                .collect();
        }
        if sets.is_empty() {
            let mut recs = self.input_recipes(fid);
            if recs.is_empty() {
                // A chamfer of one distance whose selection hangs under its value holder
                // (`0D383DB2`, the holder of two-distance chamfers: 5998's Chamfer2).
                recs = self.under(fid, "7ACC2A03", 4, Some(&["0D383DB2", "2CA5A1CD", "5662F619"]));
            }
            vec![(param_values(d, fid, names).first().copied(), recs)]
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

    /// A chamfer of two distances (`Distance 1`, `Distance 2`, cm, in a `0D383DB2` holder): the
    /// face `Distance 1` runs along, as the tree's face pick. The record holds no side flag that
    /// differs between the samples, so the stored bodies decide (section 30): per edge, of its
    /// two planar faces A and B, A is the reference when a stored point of A lies `Distance 1`
    /// from the edge (`d1 sin` of the faces' angle from plane B) and a stored point of B lies
    /// `Distance 2` from it (or one of the two, where the other way round shows neither), and the
    /// other way round not; every edge must name the same face. `first` names the parameter of
    /// the first distance (`Distance 1`, or `leftDistance` in older records).
    pub(super) fn chamfer_reference(
        &mut self,
        fid: u64,
        first: &str,
        d1: f64,
        d2: f64,
    ) -> Result<serde_json::Value, String> {
        const TOL: f64 = 1e-6;
        if (d1 - d2).abs() < 1e-9 {
            return Err("the two distances are equal".into());
        }
        let d = self.d;
        let mut chosen: Option<(String, Vec<i32>)> = None;
        let mut edges = 0;
        for (_, recs) in self.blend_groups(fid, &[first]) {
            for rid in recs {
                let (kind, faces) = naming::recipe(d, rid);
                let (Some(RecipeKind::Edge), Some(faces)) = (kind, faces) else { continue };
                let (Some(a), Some(b)) =
                    (faces.first().and_then(|f| f.first()).cloned(), faces.get(1).and_then(|f| f.first()).cloned())
                else {
                    return Err("an edge does not name two faces".into());
                };
                let (Some(pa), Some(pb)) = (self.stored_face(&a.0, &a.1), self.stored_face(&b.0, &b.1)) else {
                    return Err("a face of an edge is not on the stored bodies".into());
                };
                let (
                    naming::Surface::Plane { origin: oa, normal: na },
                    naming::Surface::Plane { origin: ob, normal: nb },
                ) = (&pa.surface, &pb.surface)
                else {
                    return Err("a face of an edge is not planar".into());
                };
                let sin = super::geom::norm(cross(*na, *nb));
                if sin < 1e-6 {
                    return Err("the two faces of an edge are parallel".into());
                }
                // Points of one face at `dist` from the other face's plane.
                let has = |pts: &[V3], o: V3, n: V3, dist: f64| {
                    pts.iter().any(|p| (dot(n, super::geom::sub(*p, o)).abs() - dist * sin).abs() < TOL)
                };
                // Each way round, how many of its two lines the stored faces show. A later blend
                // can take one away (2895's Chamfer2: a fillet runs along the line 2 mm across the
                // top; the line 3 mm down the side is still there, and neither line of the other
                // way round is).
                let a_ref = usize::from(has(&pa.points, *ob, *nb, d1)) + usize::from(has(&pb.points, *oa, *na, d2));
                let b_ref = usize::from(has(&pb.points, *oa, *na, d1)) + usize::from(has(&pa.points, *ob, *nb, d2));
                let face = match (a_ref, b_ref) {
                    (2, 0 | 1) | (1, 0) => a,
                    (0 | 1, 2) | (0, 1) => b,
                    _ => return Err("the stored bodies do not tell which face the first distance runs along".into()),
                };
                if chosen.as_ref().is_some_and(|c| *c != face) {
                    return Err("its edges run the first distance along different faces".into());
                }
                chosen = Some(face);
                edges += 1;
            }
        }
        let face = chosen.filter(|_| edges > 0).ok_or("it has no edges")?;
        self.stored_face(&face.0, &face.1).map(|p| p.json()).ok_or_else(|| "the reference face is not stored".into())
    }

    /// Per set of a fillet or chamfer: its value (cm), its edges (the middle, and for a curved
    /// edge points along it) and the count of edges not resolved.
    #[allow(clippy::type_complexity)]
    pub(super) fn blend_sets(&mut self, fid: u64, names: &[&str]) -> Vec<(Option<f64>, Vec<(V3, Vec<V3>)>, usize)> {
        let d = self.d;
        let groups = self.blend_groups(fid, names);
        self.blend_ops =
            self.opmap.iter().filter(|(_, f)| **f == fid).filter_map(|(k, _)| i32::try_from(*k).ok()).collect();
        let mut out = Vec::new();
        for (v, recs) in groups {
            let mut es: Vec<(V3, Vec<V3>)> = Vec::new();
            let mut seen: Vec<[i64; 3]> = Vec::new();
            let mut bad = 0;
            let mut done = std::collections::HashSet::new();
            for rid in recs {
                if !done.insert(rid) {
                    continue;
                }
                let (kind, faces) = naming::recipe(d, rid);
                // A face picked whole (5998's Chamfer2 names the bottom cap of a box): every edge
                // of its boundary.
                let edges: Vec<(Vec<naming::FaceName>, Option<naming::EdgeAt>)> = match (kind, faces) {
                    (Some(RecipeKind::Edge), Some(faces)) => {
                        let e = self.edge_midpoint(&faces);
                        vec![(faces, e)]
                    }
                    (Some(RecipeKind::BoundedFace), Some(faces)) if faces.len() >= 2 => {
                        let Some((face, ring)) = faces.split_first() else { continue };
                        let es = self.face_loop_edges(face, ring);
                        ring.iter().zip(es).map(|(b, e)| (vec![face.clone(), b.clone()], e)).collect()
                    }
                    _ => continue,
                };
                for (_faces, e) in edges {
                    let Some((mid, _, path)) = e else {
                        bad += 1;
                        continue;
                    };
                    let key =
                        [(mid[0] * 1e6).round() as i64, (mid[1] * 1e6).round() as i64, (mid[2] * 1e6).round() as i64];
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key);
                    es.push((mid, path));
                }
            }
            out.push((v, es, bad));
        }
        self.blend_ops.clear();
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
            cover::field(rec, o, 72, "plane.origin-x-y");
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
            cover::field(rec, o, 24, "point3.xyz");
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
        cover::field(rec, 25, 48, "pattern.direction-point-vector");
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
            cover::field(rec, k, 4, "pattern.symmetric-marker");
            cover::field(rec, k + 4, 24, "pattern.symmetric");
            return Some([v[0] != 0.0, v[1] != 0.0, v[2] != 0.0]);
        }
    }
    None
}
