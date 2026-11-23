//! The records of an Inventor part's `PmDCSegment` (feature tree, sketches, parameters), read
//! from sample bytes only. The layout notes are `docs/formats/inventor-feature-tree.md`.
//!
//! A segment is a pair of streams `M<id>` (meta: record sizes, class table, its own zlib
//! stream) and `B<id>` (the records, one zlib stream). Every record starts with a class
//! (index into the meta's class table of GUIDs) and ends with its size, a zero and the next
//! record's class. A reference is `u32 (index + 1) | 0x80000000`.
//!
//! Everything read is bounded: counts, offsets and walks are checked, and a record that does
//! not have the shape expected reads as `None`/`Err`, never a panic.

use crate::inventor::{B_HEADER, ZSTD_MAGIC, b_data, is_zlib, unpack};
use crate::{Limits, open};
use std::cell::RefCell;
use std::collections::HashMap;

/// Most records one segment may have.
const MAX_RECORDS: usize = 1_000_000;
/// Most classes one meta may list.
const MAX_CLASSES: usize = 5000;
/// Most packed-data start candidates tried in a meta stream.
const MAX_META_CANDIDATES: usize = 256;
/// Longest block skipped between two records.
const MAX_BLOCK: usize = 4096;
/// Longest list read from a record.
pub(crate) const MAX_LIST: usize = 4096;

pub(crate) fn u32_at(r: &[u8], k: usize) -> Option<u32> {
    let b = r.get(k..k.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

pub(crate) fn u16_at(r: &[u8], k: usize) -> Option<u16> {
    let b = r.get(k..k.checked_add(2)?)?;
    Some(u16::from_le_bytes(b.try_into().ok()?))
}

pub(crate) fn f64_at(r: &[u8], k: usize) -> Option<f64> {
    let b = r.get(k..k.checked_add(8)?)?;
    Some(f64::from_le_bytes(b.try_into().ok()?))
}

/// A reference value as a record index below `n`.
pub(crate) fn reference(v: u32, n: usize) -> Option<usize> {
    let i = (v & 0x7fff_ffff) as usize;
    (v & 0x8000_0000 != 0 && i > 0 && i <= n).then(|| i - 1)
}

/// What a class (by the first 32 bits of its GUID) is, as far as the feature reader goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Name,
    PartFeature,
    PatternFeature,
    Sketch,
    SkPoint,
    SkLine,
    SkCircle,
    SkEllipse,
    /// A sketch B-spline (`317b7346`, note AR).
    SkSpline,
    Param,
    SkPlacement,
    SkDir,
    HistoryBlob,
    Other,
}

fn role_of(g: u32) -> Role {
    match g {
        0x2b48_a42b => Role::Name,
        0x9087_4d91 => Role::PartFeature,
        0x2067_3244 => Role::PatternFeature,
        0x9087_4d11 => Role::Sketch,
        0xce52_df35 => Role::SkPoint,
        0xce52_df3a => Role::SkLine,
        0xce52_df3b => Role::SkCircle,
        0x4507_d460 => Role::SkEllipse,
        0x317b_7346 | 0xf937_2fd4 => Role::SkSpline,
        0x9087_4d26 => Role::Param,
        0x9087_4d18 => Role::SkPlacement,
        0xce52_df40 => Role::SkDir,
        0xf645_595c => Role::HistoryBlob,
        _ => Role::Other,
    }
}

/// The class ids used by name.
pub(crate) const G_ORDER: u32 = 0x4521_21b6;
pub(crate) const G_PROFILE: u32 = 0xf988_4c43;
pub(crate) const G_LOOP: u32 = 0xa477_243b;
pub(crate) const G_LOOP_BODY: u32 = 0xcc0f_7521;
/// The record that ties a sweep's path loop to its sketch (as `G_PROFILE` does a profile's).
pub(crate) const G_PATH: u32 = 0x424e_b7d7;
/// A feature's list of the solid bodies it made or changed.
pub(crate) const G_BODY_LIST: u32 = 0x0e68_70ae;
/// A solid body of the part.
pub(crate) const G_SOLID_BODY: u32 = 0x9087_4d47;
/// The type GUID (first 32 bits) a solid body's name carries.
const T_BODY: u32 = 0x0ca9_112d;
/// The record that names a solid body (a part-feature record that is a body, not a feature).
pub(crate) const G_BODY_NAME: u32 = 0x656d_d01e;
/// Feature records of their own classes (not `90874d91`): circular pattern, mirror and
/// sketch-driven pattern. Named like part features; left out of the walk before, which
/// dropped them from the history without a word.
const G_FEATURE_CLASSES: [u32; 3] = [0x0697_7131, 0xfad9_a9b5, 0xa37b_053c];

/// A feature's kind, by the GUID written after its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatType {
    Extrude,
    Revolve,
    Hole,
    Thread,
    Thicken,
    RectPattern,
    CircPattern,
    Mirror,
    Fillet,
    Chamfer,
    Shell,
    Sweep,
    Coil,
    Emboss,
    Combine,
    Split,
    Plate,
    Other(u32),
}

impl FeatType {
    fn of(g: u32) -> FeatType {
        match g {
            0x0ca9_1131 => FeatType::Extrude,
            0x0054_015f => FeatType::Revolve,
            0x1f75_7d1a => FeatType::Hole,
            0x28aa_1906 => FeatType::Thread,
            0x7281_f4ec => FeatType::Thicken,
            0x767a_c2da => FeatType::RectPattern,
            0xb6c2_2694 => FeatType::CircPattern,
            0xb6c2_26a3 => FeatType::Mirror,
            0xf1f7_15dc => FeatType::Fillet,
            0xf900_713f => FeatType::Chamfer,
            0xb83c_9612 => FeatType::Shell,
            0x90ba_da05 => FeatType::Sweep,
            0xb419_14da => FeatType::Coil,
            0x285e_398c => FeatType::Emboss,
            0xc023_6733 => FeatType::Combine,
            0x8371_a8ba => FeatType::Split,
            0x5558_5fc6 => FeatType::Plate,
            o => FeatType::Other(o),
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            FeatType::Extrude => "extrude".into(),
            FeatType::Revolve => "revolve".into(),
            FeatType::Hole => "hole".into(),
            FeatType::Thread => "thread".into(),
            FeatType::Thicken => "thicken".into(),
            FeatType::RectPattern => "rectangular pattern".into(),
            FeatType::CircPattern => "circular pattern".into(),
            FeatType::Mirror => "mirror".into(),
            FeatType::Fillet => "fillet".into(),
            FeatType::Chamfer => "chamfer".into(),
            FeatType::Shell => "shell".into(),
            FeatType::Sweep => "sweep".into(),
            FeatType::Coil => "coil".into(),
            FeatType::Emboss => "emboss".into(),
            FeatType::Combine => "combine".into(),
            FeatType::Split => "split".into(),
            FeatType::Plate => "sheet metal face".into(),
            FeatType::Other(g) => format!("feature {g:08x}"),
        }
    }
}

/// One feature of the part: its record, name and kind.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Feature {
    pub(crate) rec: usize,
    pub(crate) name: Option<String>,
    pub(crate) typ: Option<FeatType>,
}

/// The decoded segment: records with their classes.
pub(crate) struct Dc {
    data: Vec<u8>,
    spans: Vec<(usize, usize)>,
    pub(crate) g: Vec<u32>,
    pub(crate) role: Vec<Role>,
    /// The version word that follows the common header of the first feature record.
    ver: Option<u32>,
    /// Parameter values that replace the stored ones, by name (an iPart member's row read
    /// against its factory, note AO).
    row: Vec<(String, f64)>,
    /// The names of the replaced parameters a reading took.
    row_used: RefCell<Vec<String>>,
}

fn inflate(z: &[u8], limits: &Limits) -> Option<Vec<u8>> {
    let cap = limits.max_stream_bytes.min(z.len().saturating_mul(limits.max_ratio));
    unpack(z, cap).ok()
}

/// Where a meta stream's packed body may start: a zlib header (`78 01/5e/9c/da`) or the
/// Zstandard magic (newer files).
fn is_packed_head(b: &[u8]) -> bool {
    matches!((b.first(), b.get(1)), (Some(0x78), Some(0x01 | 0x5e | 0x9c | 0xda))) && is_zlib(b)
        || b.starts_with(&ZSTD_MAGIC)
}

/// The meta stream's segment name and inflated body.
fn read_meta(raw: &[u8], limits: &Limits) -> Option<(String, Option<Vec<u8>>)> {
    let n = u32_at(raw, 0)? as usize;
    if n > 256 {
        return None;
    }
    let i = 4 + n + 22;
    let l = u32_at(raw, i - 4)? as usize;
    if l > 256 {
        return None;
    }
    let name_bytes = raw.get(i..i + 2 * l)?;
    let units: Vec<u16> = name_bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
        .collect();
    let name = String::from_utf16(&units).ok()?;
    let mut tried = 0;
    for j in i + 2 * l..raw.len() {
        if !is_packed_head(raw.get(j..)?) {
            continue;
        }
        tried += 1;
        if tried > MAX_META_CANDIDATES {
            break;
        }
        if let Some(d) = raw.get(j..).and_then(|z| inflate(z, limits)) {
            return Some((name, Some(d)));
        }
    }
    Some((name, None))
}

impl Dc {
    /// Reads the `PmDCSegment` of a part.
    pub(crate) fn open(bytes: &[u8], limits: &Limits) -> Result<Dc, String> {
        let mut warnings = Vec::new();
        let f = open(bytes, limits, &mut warnings).map_err(|e| e.to_string())?;
        let mut metas: Vec<usize> = (0..f.streams.len())
            .filter(|&i| f.streams.get(i).is_some_and(|e| e.name.starts_with('M') && e.name.len() > 8))
            .collect();
        metas.truncate(limits.max_streams);
        for mi in metas {
            let Some(name) = f.streams.get(mi).map(|e| e.name.clone()) else { continue };
            let raw = f.read(mi, limits).map_err(|e| e.to_string())?;
            let Some((seg, meta)) = read_meta(&raw, limits) else { continue };
            if seg != "PmDCSegment" {
                continue;
            }
            let meta = meta.ok_or("the feature segment's meta stream does not inflate")?;
            let bname = format!("B{}", name.get(1..).unwrap_or_default());
            let bi = (0..f.streams.len())
                .find(|&i| f.streams.get(i).is_some_and(|e| e.name == bname))
                .ok_or("the feature segment has no data stream")?;
            let braw = f.read(bi, limits).map_err(|e| e.to_string())?;
            if braw.len() <= B_HEADER {
                return Err("the feature data is cut short".into());
            }
            let b = b_data(&braw, limits).map_err(|e| e.to_string())?.ok_or("the feature data does not unpack")?;
            return Dc::new(&meta, b);
        }
        Err("the part has no feature segment (PmDCSegment)".into())
    }

    fn new(meta: &[u8], b: Vec<u8>) -> Result<Dc, String> {
        let n = u32_at(meta, 14).ok_or("the meta stream is cut short")? as usize;
        if n == 0 || n > MAX_RECORDS {
            return Err("the feature segment has no records or too many".into());
        }
        let mut sizes = Vec::with_capacity(n);
        for i in 0..n {
            sizes.push((u32_at(meta, 18 + 4 * i).ok_or("the meta stream is cut short")? & 0x7fff_ffff) as usize);
        }
        let classes = classes(meta, n)?;
        let mut spans = Vec::with_capacity(n);
        let mut g = Vec::with_capacity(n);
        let mut p = 4usize;
        let mut cur = u32_at(&b, 0).ok_or("the feature data is cut short")?;
        for (i, &s) in sizes.iter().enumerate() {
            let e = p.checked_add(s).ok_or("record size overflow")?;
            if e.checked_add(9).is_none_or(|x| x > b.len()) || u32_at(&b, e) != Some(s as u32) {
                return Err(format!("the feature records do not frame at record {i}"));
            }
            spans.push((p, e));
            g.push(cur.checked_sub(0x100).and_then(|c| classes.get(c as usize)).copied().unwrap_or(0));
            cur = u32_at(&b, e + 5).ok_or("the feature data is cut short")?;
            p = e + 9;
            // A non-zero byte after the size: a block follows the record (named entries such as
            // "RDxSweepTwistAngle" or class GUID strings, not decoded) and the next class word
            // comes after it. The next record is where its class word, content of the size the
            // meta gives, size and separator line up.
            if b.get(e + 4).is_some_and(|&x| x != 0)
                && let Some(&next) = sizes.get(i + 1)
            {
                let ok_class =
                    |c: Option<u32>| c.is_some_and(|c| (0x100..0x100 + classes.len() as u64).contains(&u64::from(c)));
                let q = (e + 9..=e + 9 + MAX_BLOCK).find(|&q| {
                    ok_class(u32_at(&b, q - 4))
                        && u32_at(&b, q + next) == Some(next as u32)
                        && matches!(b.get(q + next + 4), Some(0 | 1))
                        && (ok_class(u32_at(&b, q + next + 5)) || u32_at(&b, q + next + 5) == Some(0xffff_ffff))
                });
                let q = q.ok_or_else(|| format!("the feature records do not frame after record {i}"))?;
                cur = u32_at(&b, q - 4).unwrap_or(0);
                p = q;
            }
        }
        if b.len() - p > 64 {
            return Err("the feature data has trailing bytes".into());
        }
        let role: Vec<Role> = g.iter().map(|&x| role_of(x)).collect();
        let (b, spans) = unshift(b, spans, &g, &role);
        let mut d = Dc { data: b, spans, g, role, ver: None, row: Vec::new(), row_used: RefCell::new(Vec::new()) };
        let first = d.role.iter().position(|r| *r == Role::PartFeature);
        d.ver = first.and_then(|i| u32_at(d.r(i), 22)).filter(|&v| v != 0);
        Ok(d)
    }

    pub(crate) fn n(&self) -> usize {
        self.spans.len()
    }

    /// The bytes of record `i` (empty when out of range).
    pub(crate) fn r(&self, i: usize) -> &[u8] {
        self.spans.get(i).and_then(|&(a, e)| self.data.get(a..e)).unwrap_or_default()
    }

    pub(crate) fn role(&self, i: usize) -> Role {
        self.role.get(i).copied().unwrap_or(Role::Other)
    }

    pub(crate) fn class(&self, i: usize) -> u32 {
        self.g.get(i).copied().unwrap_or(0)
    }

    /// `(offset, record)` of every reference value at any byte offset of record `i`.
    pub(crate) fn refs(&self, i: usize) -> Vec<(usize, usize)> {
        let r = self.r(i);
        let n = self.n();
        (0..r.len().saturating_sub(3)).filter_map(|k| reference(u32_at(r, k)?, n).map(|x| (k, x))).collect()
    }

    /// The offset where the class body starts: after the common header (to 22), optional
    /// `0xffffffff` words and the version word. A record kept from an older save of the part
    /// carries that save's version word (0x0a92 among 0x0a96 records): after `0xffffffff`, any
    /// word of the version range (0x0800..0x1000) is one.
    pub(crate) fn body(&self, i: usize) -> usize {
        let r = self.r(i);
        let k = 22usize;
        if let Some(ver) = self.ver {
            for j in 0..3usize {
                let q = k + 4 * j;
                let w = u32_at(r, q);
                let older =
                    j > 0 && u32_at(r, q - 4) == Some(0xffff_ffff) && w.is_some_and(|w| (0x0800..0x1000).contains(&w));
                if (w == Some(ver) || older) && (0..j).all(|t| matches!(u32_at(r, k + 4 * t), Some(0 | 0xffff_ffff))) {
                    return q + 4;
                }
            }
        }
        let mut k = k;
        while u32_at(r, k) == Some(0xffff_ffff) {
            k += 4;
        }
        k
    }

    /// The name record that record `i` points at (offset 6), as text.
    pub(crate) fn name(&self, i: usize) -> Option<String> {
        let v = reference(u32_at(self.r(i), 6)?, self.n())?;
        (self.role(v) == Role::Name).then(|| self.namestr(v)).flatten()
    }

    pub(crate) fn namestr(&self, j: usize) -> Option<String> {
        let r = self.r(j);
        for q in 0x1a..r.len().saturating_sub(4) {
            let l = u32_at(r, q)? as usize;
            if !(0 < l && l < 200 && q + 4 + 2 * l <= r.len()) {
                continue;
            }
            let units: Vec<u16> = r
                .get(q + 4..q + 4 + 2 * l)?
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
                .collect();
            let Ok(s) = String::from_utf16(&units) else { continue };
            let tail = r.get(q + 4 + 2 * l..)?;
            if s.chars().all(|c| !c.is_control() && (c == ' ' || !c.is_whitespace()))
                && (tail.iter().all(|&b| b == 0) || tail.len() == 16)
            {
                return Some(s);
            }
        }
        None
    }

    /// The first 32 bits of the 16-byte type GUID that ends a feature's name record.
    fn nametype(&self, j: usize) -> Option<u32> {
        self.namestr(j)?;
        let r = self.r(j);
        let tail = r.get(r.len().checked_sub(16)?..)?;
        let before = r.get(r.len().checked_sub(18)?..r.len() - 16);
        (tail.iter().any(|&b| b != 0) && before != Some(&[0, 0][..])).then(|| u32_at(tail, 0)).flatten()
    }

    /// The part and pattern features (with the circular-pattern, mirror and sketch-driven
    /// pattern records), in record order. A part-feature record named through a
    /// `656dd01e` record, or by a name of type `0ca9112d`, is a solid body of the part (its
    /// name, e.g. "Solid1" or a body the user renamed, is behind that record), not a feature:
    /// it is left out.
    pub(crate) fn features(&self) -> Vec<Feature> {
        self.features_and_bodies().0
    }

    /// Whether the part starts from a solid body no feature made: a solid body record whose id
    /// comes before every feature's (an imported base body, note AR). Its shape is stored only
    /// as part of the result.
    pub(crate) fn has_base_body(&self) -> bool {
        let (feats, bodies) = self.features_and_bodies();
        let first = feats.iter().filter_map(|f| self.seq(f.rec)).min();
        bodies.iter().filter_map(|&b| self.seq(b)).min().is_some_and(|b| first.is_none_or(|f| b < f))
    }

    /// The features, and the part-feature records that are solid bodies.
    fn features_and_bodies(&self) -> (Vec<Feature>, Vec<usize>) {
        let mut out = Vec::new();
        let mut bodies = Vec::new();
        let mut owned: Option<HashMap<usize, usize>> = None;
        for i in 0..self.n() {
            if !matches!(self.role(i), Role::PartFeature | Role::PatternFeature)
                && !G_FEATURE_CLASSES.contains(&self.class(i))
            {
                continue;
            }
            let v = u32_at(self.r(i), 6).and_then(|v| reference(v, self.n()));
            if v.is_some_and(|v| self.class(v) == G_BODY_NAME) {
                bodies.push(i);
                continue;
            }
            let (mut name, mut typ) = (None, None);
            // A feature whose name word points at another record (a surface feature's, a
            // move or scale body's, some extrudes and holes, note AQ): its name record is the
            // one whose owner word is the feature.
            let v = match v {
                Some(v) if self.role(v) != Role::Name => {
                    owned.get_or_insert_with(|| self.owned_names()).get(&i).copied().or(Some(v))
                }
                v => v,
            };
            if let Some(v) = v.filter(|&v| self.role(v) == Role::Name) {
                name = self.namestr(v);
                typ = self.nametype(v).map(FeatType::of);
                // The type a body's name carries (the bodies named through `656dd01e` have it
                // too): an imported part whose only "feature" is its body.
                if typ == Some(FeatType::Other(T_BODY)) {
                    bodies.push(i);
                    continue;
                }
            }
            out.push(Feature { rec: i, name, typ });
        }
        (out, bodies)
    }

    /// The typed name records (a feature's name) by the record their owner word (offset 14 or
    /// 18, by the save's header) points at. A record named twice is left out.
    fn owned_names(&self) -> HashMap<usize, usize> {
        let mut m: HashMap<usize, Option<usize>> = HashMap::new();
        for j in (0..self.n()).filter(|&j| self.role(j) == Role::Name) {
            if self.nametype(j).is_none() {
                continue;
            }
            let r = self.r(j);
            let mut owners: Vec<usize> =
                [14, 18].iter().filter_map(|&o| u32_at(r, o).and_then(|w| reference(w, self.n()))).collect();
            owners.dedup();
            for x in owners.into_iter().filter(|&x| x != j) {
                m.entry(x).and_modify(|e| *e = None).or_insert(Some(j));
            }
        }
        m.into_iter().filter_map(|(k, v)| Some((k, v?))).collect()
    }

    /// The id written at offset 18 of a record.
    pub(crate) fn seq(&self, i: usize) -> Option<u32> {
        u32_at(self.r(i), 18)
    }

    /// Features in history order: the order of their ids in the record of class `452121b6`.
    pub(crate) fn ordered_features(&self) -> Vec<Feature> {
        let all = self.features();
        let ids: HashMap<u32, usize> =
            all.iter().enumerate().filter_map(|(k, f)| Some((self.seq(f.rec)?, k))).collect();
        let Some(o) = (0..self.n()).find(|&i| self.class(i) == G_ORDER) else { return all };
        let r = self.r(o);
        // Each entry is `0x80000003, id`; an id read at any other offset is part of another
        // word (a feature whose id is 256 matched the bytes `00 01 00 00` of a list header and
        // came first, note AQ). The loose reading stays for a record whose entries are not
        // all found so.
        let scan = |strict: bool| {
            let mut order: Vec<usize> = Vec::new();
            for k in 0..r.len().saturating_sub(3) {
                if strict && (k < 4 || u32_at(r, k - 4) != Some(0x8000_0003)) {
                    continue;
                }
                if let Some(&f) = u32_at(r, k).and_then(|v| ids.get(&v))
                    && !order.contains(&f)
                {
                    order.push(f);
                }
            }
            order
        };
        let (strict, loose) = (scan(true), scan(false));
        let order = if strict.len() == loose.len() { self.browser_order(r, strict, &ids, &all) } else { loose };
        let rest: Vec<usize> = (0..all.len()).filter(|k| !order.contains(k)).collect();
        order.into_iter().chain(rest).filter_map(|k| all.get(k).cloned()).collect()
    }

    /// The order record's features, moved where a copy needs them: each entry's id is followed
    /// by a list of two (`0x30000002`, count 2, `2`, `0x108`) pairs `u8, u32` (the second's
    /// `u32` a place; a feature has two entries, its id and its id with the top bit set, at
    /// places `2k` and `2k + 1`). A feature a mirror or pattern copies but made after it (the
    /// LEGO bricks' Fillet1, EY4180 A-02's Extrusion11), whose place comes before the copy's,
    /// moves to just before the copy. Places are not the browser order in every file (EY4180's
    /// bay adapters place an emboss before the base extrude), so nothing else moves; unless
    /// every feature's entry has this layout, the record's order is kept.
    fn browser_order(&self, r: &[u8], order: Vec<usize>, ids: &HashMap<u32, usize>, all: &[Feature]) -> Vec<usize> {
        const HEAD: [u8; 16] = [0x02, 0, 0, 0x30, 0x02, 0, 0, 0, 0x02, 0, 0, 0, 0x08, 0x01, 0, 0];
        let mut place: HashMap<usize, u32> = HashMap::new();
        for k in 4..r.len().saturating_sub(3) {
            if u32_at(r, k - 4) != Some(0x8000_0003) {
                continue;
            }
            let Some(&f) = u32_at(r, k).and_then(|v| ids.get(&v)) else { continue };
            let at = |a: usize, b: usize| r.get(k + a..k + b);
            let laid = at(4, 20) == Some(&HEAD[..])
                && at(20, 25) == Some(&[2, 0, 0, 0, 1][..])
                && at(29, 35) == Some(&[0, 3, 0, 0, 0, 0][..]);
            match (laid, u32_at(r, k + 35)) {
                (true, Some(p)) if !place.contains_key(&f) => {
                    place.insert(f, p);
                }
                _ => return order,
            }
        }
        if order.iter().any(|f| !place.contains_key(f)) {
            return order;
        }
        let of_rec: HashMap<usize, usize> = all.iter().enumerate().map(|(k, f)| (f.rec, k)).collect();
        let mut out = order;
        let copies: Vec<usize> = out
            .iter()
            .copied()
            .filter(|&k| {
                all.get(k).is_some_and(|f| {
                    matches!(f.typ, Some(FeatType::Mirror | FeatType::RectPattern | FeatType::CircPattern))
                })
            })
            .collect();
        for c in copies {
            let Some(f) = all.get(c) else { continue };
            for (_, x) in self.refs(f.rec) {
                let Some(&m) = of_rec.get(&x) else { continue };
                let (Some(pc), Some(pm)) = (out.iter().position(|&k| k == c), out.iter().position(|&k| k == m)) else {
                    continue;
                };
                if pm > pc && place.get(&m) < place.get(&c) {
                    out.remove(pm);
                    out.insert(pc, m);
                }
            }
        }
        out
    }

    /// The items of the first `0x30000002` list at or after `start` (default: the body), and
    /// the offset after it.
    pub(crate) fn first_list(&self, i: usize, start: Option<usize>) -> Option<(Vec<Option<usize>>, usize)> {
        let r = self.r(i);
        let from = start.unwrap_or_else(|| self.body(i));
        let k = from + r.get(from..)?.windows(4).position(|w| w == [0x02, 0x00, 0x00, 0x30])?;
        let n = u32_at(r, k + 4)? as usize;
        if n == 0 {
            return Some((Vec::new(), k + 8));
        }
        if n > MAX_LIST {
            return None;
        }
        let mut items = Vec::with_capacity(n);
        for j in 0..n {
            items.push(reference(u32_at(r, k + 16 + 4 * j)?, self.n()));
        }
        Some((items, k + 16 + 4 * n))
    }

    /// A feature's slots: the first list in its body (zero reads as `None`).
    pub(crate) fn slots(&self, i: usize) -> Option<Vec<Option<usize>>> {
        self.first_list(i, None).map(|x| x.0)
    }

    /// Parameter record: name and value (cm or radians); a parameter of the row reads as the
    /// row's value and counts as read.
    pub(crate) fn param(&self, i: usize) -> Option<(String, f64)> {
        let (name, val) = self.param_saved(i)?;
        if let Some(&(_, v)) = self.row.iter().find(|(n, _)| *n == name) {
            let mut used = self.row_used.borrow_mut();
            if !used.contains(&name) {
                used.push(name.clone());
            }
            return Some((name, v));
        }
        Some((name, val))
    }

    /// Parameter record: name and the value the file stores.
    pub(crate) fn param_saved(&self, i: usize) -> Option<(String, f64)> {
        let r = self.r(i);
        let val = f64_at(r, r.len().checked_sub(20)?)?;
        let mut name = String::new();
        for q in 22..r.len().saturating_sub(36) {
            let l = u32_at(r, q)? as usize;
            if 0 < l && l < 64 && q + 4 + 2 * l + 12 == r.len() - 20 {
                let units: Vec<u16> = r
                    .get(q + 4..q + 4 + 2 * l)?
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
                    .collect();
                if let Ok(s) = String::from_utf16(&units) {
                    name = s;
                }
                break;
            }
        }
        Some((name, val))
    }

    /// Whether the row replaces the value of parameter record `i` (without counting it as read).
    pub(crate) fn in_row(&self, i: usize) -> bool {
        if self.row.is_empty() || self.role(i) != Role::Param {
            return false;
        }
        self.param_saved(i).is_some_and(|(n, _)| self.row.iter().any(|(m, _)| *m == n))
    }

    /// Whether parameter record `i` is driven (a reference dimension's measured value: bit 0 of
    /// header byte 13; note AP).
    pub(crate) fn driven(&self, i: usize) -> bool {
        self.role(i) == Role::Param && self.r(i).get(13).is_some_and(|b| b & 1 != 0)
    }

    /// Whether a row is set.
    pub(crate) fn has_row(&self) -> bool {
        !self.row.is_empty()
    }

    /// Reads parameters named in `row` as the given values instead of the stored ones.
    pub(crate) fn set_row(&mut self, row: &[(String, f64)]) {
        self.row = row.to_vec();
    }

    /// The names of the replaced parameters read so far.
    pub(crate) fn row_used(&self) -> Vec<String> {
        self.row_used.borrow().clone()
    }

    /// The value of the parameter in slot `j` of a feature.
    pub(crate) fn pval(&self, slots: &[Option<usize>], j: usize) -> Option<f64> {
        let s = slots.get(j).copied().flatten()?;
        (self.role(s) == Role::Param).then(|| self.param(s).map(|p| p.1)).flatten()
    }

    /// A sketch placement: the 4x4 row-major matrix (cm) from a mask word (bit `k`: entry `k`
    /// is +-1, bit `16+k`: it is 0, both: -1) and an f64 for every other entry.
    pub(crate) fn placement(&self, i: usize) -> Option<[f64; 16]> {
        let r = self.r(i);
        let mut at = None;
        for b in (22..40).step_by(4) {
            if b + 4 > r.len() {
                return None;
            }
            let m = u32_at(r, b)?;
            let n = (0..16).filter(|e| (m >> e) & 1 == 0 && (m >> (16 + e)) & 1 == 0).count();
            if b + 4 + 8 * n == r.len() && (m >> 15) & 1 == 1 {
                at = Some(b);
                break;
            }
        }
        let b = at.unwrap_or_else(|| self.body(i));
        let m = u32_at(r, b)?;
        let mut k = b + 4;
        let mut out = [0.0; 16];
        for (e, slot) in out.iter_mut().enumerate() {
            let (one, zero) = ((m >> e) & 1, (m >> (16 + e)) & 1);
            *slot = match (one, zero) {
                (1, 1) => -1.0,
                (1, _) => 1.0,
                (_, 1) => 0.0,
                _ => {
                    let v = f64_at(r, k)?;
                    k += 8;
                    v
                }
            };
        }
        Some(out)
    }

    /// A direction record: five f64 at its end, the last three the sketch normal.
    pub(crate) fn skdir(&self, i: usize) -> Option<[f64; 5]> {
        let r = self.r(i);
        let s = r.len().checked_sub(40)?;
        let mut out = [0.0; 5];
        for (k, o) in out.iter_mut().enumerate() {
            *o = f64_at(r, s + 8 * k)?;
        }
        Some(out)
    }
}

/// Newer files (version word 2803 and later) put one more `u32` (zero) at offset 14 of the
/// common header: `u32 0, u16, ref name, 4 flag bytes, u32 0, ref owner, u32 id, ...`. Those
/// records are rewritten without it, so every offset read here is the older layout's. The file
/// has the newer layout when its first part feature has a zero there and a reference after
/// it (the older layout has the owner reference at 14 and the id at 18). Only the classes
/// whose every record in the file has that shape are rewritten: some classes have no common
/// header at all (ShapeManager blobs, edge geometry), in either layout.
fn unshift(b: Vec<u8>, spans: Vec<(usize, usize)>, g: &[u32], role: &[Role]) -> (Vec<u8>, Vec<(usize, usize)>) {
    let n = spans.len();
    let rec = |i: usize| spans.get(i).and_then(|&(a, e)| b.get(a..e)).unwrap_or_default();
    let is_ref = |v: u32| v & 0x8000_0000 != 0 && reference(v, n).is_some();
    let shaped = |i: usize| {
        let r = rec(i);
        u32_at(r, 14) == Some(0) && u32_at(r, 18).is_some_and(|v| v == 0 || is_ref(v)) && r.len() >= 26
    };
    let Some(first) = role.iter().position(|r| *r == Role::PartFeature) else { return (b, spans) };
    if !(shaped(first) && u32_at(rec(first), 18).is_some_and(is_ref)) {
        return (b, spans);
    }
    let mut keep: HashMap<u32, bool> = HashMap::new();
    for i in 0..n {
        let k = keep.entry(g.get(i).copied().unwrap_or(0)).or_insert(true);
        *k = *k && shaped(i);
    }
    let mut out = Vec::with_capacity(b.len());
    let mut sp = Vec::with_capacity(n);
    for i in 0..n {
        let r = rec(i);
        let a = out.len();
        if keep.get(&g.get(i).copied().unwrap_or(0)).copied().unwrap_or(false) {
            out.extend(r.get(..14).unwrap_or_default());
            out.extend(r.get(18..).unwrap_or_default());
        } else {
            out.extend(r);
        }
        sp.push((a, out.len()));
    }
    (out, sp)
}

/// The class table: `u32 count`, then 28-byte entries (GUID, ...), after the bytes
/// `00 x6, 04 00 00 00` that follow the record sizes. Returns the first 32 bits of each GUID.
fn classes(meta: &[u8], nrec: usize) -> Result<Vec<u32>, String> {
    let p0 = 18 + 4 * nrec;
    for q in p0 + 16..meta.len().saturating_sub(4) {
        if meta.get(q - 4..q) != Some(&[4, 0, 0, 0][..]) || meta.get(q - 10..q - 4) != Some(&[0u8; 6][..]) {
            continue;
        }
        let Some(nc) = u32_at(meta, q).map(|x| x as usize) else { continue };
        if !(0 < nc && nc < MAX_CLASSES && q + 4 + 28 * nc <= meta.len()) {
            continue;
        }
        let ok = (0..nc).all(|j| {
            let g = q + 4 + 28 * j;
            u16_at(meta, g + 18).is_some_and(|v| v <= 3) && u32_at(meta, g + 20) == Some(0)
        });
        if ok {
            return Ok((0..nc).map(|j| u32_at(meta, q + 4 + 28 * j).unwrap_or(0)).collect());
        }
    }
    Err("the feature segment has no class table".into())
}
