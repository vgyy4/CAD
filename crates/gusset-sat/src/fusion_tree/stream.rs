//! Framing of a Fusion design stream: the class table and object index of `MetaStream.dat`,
//! the object records of `BulkStream.dat`, references, sections, attributes and the strings
//! the feature readers anchor on (layout notes: `docs/formats/fusion-design-stream.md`).
//! Every read is bounds-checked; damage gives `None`, never a panic.

use std::collections::HashMap;

/// Most classes in a stream's class table, and most objects in its index.
const MAX_CLASSES: usize = 20_000;
const MAX_OBJECTS: usize = 4_000_000;
/// Longest string the framing accepts.
const MAX_STR: usize = 1 << 20;

pub(super) fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    let s = b.get(o..o.checked_add(4)?)?;
    Some(u32::from_le_bytes(s.try_into().ok()?))
}

pub(super) fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    let s = b.get(o..o.checked_add(8)?)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

pub(super) fn i32_at(b: &[u8], o: usize) -> Option<i32> {
    let s = b.get(o..o.checked_add(4)?)?;
    Some(i32::from_le_bytes(s.try_into().ok()?))
}

pub(super) fn f64_at(b: &[u8], o: usize) -> Option<f64> {
    let s = b.get(o..o.checked_add(8)?)?;
    Some(f64::from_le_bytes(s.try_into().ok()?))
}

/// `N` doubles at `o`, all finite and below 1e9 in magnitude.
pub(super) fn dbl<const N: usize>(rec: &[u8], o: usize) -> Option<[f64; N]> {
    let mut out = [0.0; N];
    for (k, slot) in out.iter_mut().enumerate() {
        let v = f64_at(rec, o.checked_add(k.checked_mul(8)?)?)?;
        if !v.is_finite() || v.abs() >= 1e9 {
            return None;
        }
        *slot = v;
    }
    Some(out)
}

pub(super) fn dbl3(rec: &[u8], o: usize) -> Option<[f64; 3]> {
    dbl::<3>(rec, o)
}

/// [`dbl`] with every value also below `lim`.
pub(super) fn finite_doubles<const N: usize>(rec: &[u8], o: usize, lim: f64) -> Option<[f64; N]> {
    dbl::<N>(rec, o).filter(|v| v.iter().all(|x| x.abs() < lim))
}

/// Byte reader over a slice.
pub(super) struct Rd<'a> {
    pub b: &'a [u8],
    pub i: usize,
}

impl<'a> Rd<'a> {
    pub(super) fn new(b: &'a [u8], i: usize) -> Rd<'a> {
        Rd { b, i }
    }

    pub(super) fn u32(&mut self) -> Option<u32> {
        let v = u32_at(self.b, self.i)?;
        self.i += 4;
        Some(v)
    }

    pub(super) fn u64(&mut self) -> Option<u64> {
        let v = u64_at(self.b, self.i)?;
        self.i += 8;
        Some(v)
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }

    /// `u32` length and ASCII/Latin-1 text.
    pub(super) fn str_(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        if n > MAX_STR {
            return None;
        }
        Some(self.take(n)?.iter().map(|&c| char::from(c)).collect())
    }

    /// `u32` character count and UTF-16LE text.
    pub(super) fn wstr(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        if n > MAX_STR {
            return None;
        }
        let s = self.take(n.checked_mul(2)?)?;
        let units: Vec<u16> = s
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)]))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

/// A GUID in text form (`8-4-4-4-12` hexadecimal digits) at `o`.
pub(super) fn guid_at(b: &[u8], o: usize) -> bool {
    let Some(s) = b.get(o..o + 36) else { return false };
    s.iter().enumerate().all(|(i, &c)| if matches!(i, 8 | 13 | 18 | 23) { c == b'-' } else { is_hex(c) })
}

/// `str` of a GUID (u32 36 and the text) at `o`.
fn guid_str_at(b: &[u8], o: usize) -> bool {
    u32_at(b, o) == Some(36) && guid_at(b, o + 4)
}

/// A UUID as a UTF-16 string value.
pub(super) fn is_uuid_text(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == b'-' } else { is_hex(c) })
}

/// The class table and object index of a MetaStream.
struct Meta {
    classes: Vec<(String, Vec<u64>)>,
    order: Vec<u64>,
    index: Vec<(u64, u64)>,
}

fn parse_meta(m: &[u8]) -> Option<Meta> {
    let mut r = Rd::new(m, 0);
    r.str_()?;
    r.u32()?;
    r.wstr()?;
    let hdr_start = r.i;
    // The class table follows the header: `u32 count`, then entries starting with a GUID.
    let end = hdr_start.saturating_add(256).min(m.len().saturating_sub(48));
    let j = (hdr_start..end).find(|&j| {
        m.get(j + 4..j + 8) == Some(&[0x24, 0, 0, 0])
            && guid_at(m, j + 8)
            && u32_at(m, j).is_some_and(|n| 0 < n && n < 100_000)
    })?;
    r.i = j;
    let n = r.u32()? as usize;
    if n > MAX_CLASSES {
        return None;
    }
    let mut classes = Vec::new();
    for _ in 0..n {
        let c = r.str_()?;
        r.str_()?;
        r.u32()?;
        r.str_()?;
        let cnt = r.u32()? as usize;
        if cnt > 10_000_000 || cnt.checked_mul(8)? > m.len().saturating_sub(r.i) {
            return None;
        }
        let mut ids = Vec::with_capacity(cnt);
        for _ in 0..cnt {
            ids.push(r.u64()?);
        }
        classes.push((c, ids));
    }
    let n = r.u32()? as usize;
    if n.checked_mul(8)? > m.len().saturating_sub(r.i) {
        return None;
    }
    for _ in 0..n {
        r.u64()?;
    }
    let mut table = || -> Option<Vec<(u64, u64)>> {
        let n = r.u32()? as usize;
        if n > MAX_OBJECTS || n.checked_mul(16)? > m.len().saturating_sub(r.i) {
            return None;
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push((r.u64()?, r.u64()?));
        }
        Some(v)
    };
    let index = table()?;
    table()?;
    let order = index.iter().map(|p| p.0).collect();
    Some(Meta { classes, order, index })
}

/// One design stream: class of every object, and where its record lies in the BulkStream.
#[derive(Debug)]
pub(super) struct Design<'a> {
    bulk: &'a [u8],
    classes: Vec<String>,
    cls_of: HashMap<u64, usize>,
    /// Object ids in index order, each once (the iteration order of the stream's objects).
    pub ids: Vec<u64>,
    /// The index's ids in file order, repeats included (the compute order lists).
    pub order: Vec<u64>,
    objs: HashMap<u64, (usize, usize)>,
}

impl<'a> Design<'a> {
    pub(super) fn new(meta: &[u8], bulk: &'a [u8]) -> Option<Design<'a>> {
        let m = parse_meta(meta)?;
        let mut cls_of = HashMap::new();
        let mut classes = Vec::new();
        for (k, (g, ids)) in m.classes.into_iter().enumerate() {
            classes.push(g);
            for i in ids {
                cls_of.insert(i, k);
            }
        }
        let mut offs: Vec<usize> = m.index.iter().map(|p| usize::try_from(p.1).unwrap_or(usize::MAX)).collect();
        offs.sort_unstable();
        offs.dedup();
        let mut ids = Vec::new();
        let mut objs = HashMap::new();
        for (id, o) in &m.index {
            if objs.contains_key(id) {
                continue;
            }
            let o = usize::try_from(*o).unwrap_or(usize::MAX);
            let k = offs.partition_point(|x| *x <= o);
            let e = offs.get(k).copied().unwrap_or(bulk.len());
            objs.insert(*id, (o, e));
            ids.push(*id);
        }
        Some(Design { bulk, classes, cls_of, ids, order: m.order, objs })
    }

    pub(super) fn has(&self, id: u64) -> bool {
        self.objs.contains_key(&id)
    }

    /// The record of object `id` (empty when there is none).
    pub(super) fn rec(&self, id: u64) -> &'a [u8] {
        let Some((o, e)) = self.objs.get(&id) else { return &[] };
        let e = (*e).min(self.bulk.len());
        let o = (*o).min(e);
        self.bulk.get(o..e).unwrap_or(&[])
    }

    /// The class GUID of object `id` (empty when it has none).
    pub(super) fn cls(&self, id: u64) -> &str {
        self.cls_of.get(&id).and_then(|k| self.classes.get(*k)).map_or("", String::as_str)
    }

    /// Whether the class of `id` starts with `prefix` (the first 8 characters of its GUID).
    pub(super) fn is(&self, id: u64, prefix: &str) -> bool {
        self.cls(id).starts_with(prefix)
    }

    /// The class GUID of a record tag (`256 + position in the class table`).
    pub(super) fn tag_class(&self, tag: u32) -> Option<&str> {
        let k = (tag as usize).checked_sub(256)?;
        self.classes.get(k).map(String::as_str)
    }
}

/// If `rec[j..]` is a reference: its object id and the offset after it. A reference is `01`,
/// a `u64` id of an existing object, then `00` (plain) or the target's class GUID as `str`
/// (older streams: "typed" references).
pub(super) fn ref_end(d: &Design, rec: &[u8], j: usize) -> Option<(u64, usize)> {
    if *rec.get(j)? != 1 {
        return None;
    }
    let q = u64_at(rec, j + 1)?;
    if q == 0 || !d.has(q) {
        return None;
    }
    let e = j + 9;
    // Two classes spell their GUID with the dashes elsewhere (`40DCC3F41D-E94F-B3B1-3E95-9B6ABE572C`,
    // the blob-name holder, and `A7AEA631-985B-4DD1-8CE2-DE2C-14B54081`): a typed reference to
    // one carries that text, which is the target's class as the class table spells it.
    let own_class = || {
        let c = d.cls(q).as_bytes();
        c.len() == 36 && u32_at(rec, e) == Some(36) && rec.get(e + 4..e + 40) == Some(c)
    };
    if guid_str_at(rec, e) || own_class() {
        Some((q, e + 40))
    } else if rec.get(e) == Some(&0) {
        Some((q, e + 1))
    } else {
        None
    }
}

/// Every reference in `rec` from `start`: (offset, object id).
pub(super) fn refs_at(d: &Design, rec: &[u8], start: usize) -> Vec<(usize, u64)> {
    let mut out = Vec::new();
    let mut j = start;
    while j + 10 <= rec.len() {
        if let Some((q, e)) = ref_end(d, rec, j) {
            out.push((j, q));
            j = e;
        } else {
            j += 1;
        }
    }
    out
}

/// Ids of the references in `rec` (all of them).
pub(super) fn ref_ids(d: &Design, rec: &[u8]) -> Vec<u64> {
    refs_at(d, rec, 0).into_iter().map(|p| p.1).collect()
}

/// Every `u32 n` followed by `n` references (each padded with the same number of zero bytes):
/// (offset, ids).
pub(super) fn ref_lists(d: &Design, rec: &[u8], start: usize) -> Vec<(usize, Vec<u64>)> {
    let mut out = Vec::new();
    let mut j = start;
    while j + 4 <= rec.len() {
        let n = u32_at(rec, j).unwrap_or(0) as usize;
        let mut hit = None;
        if (1..=200_000).contains(&n) && j + 4 < rec.len() && rec.get(j + 4) == Some(&1) {
            for pad in [1usize, 2, 0, 3] {
                let mut o = j + 4;
                let mut ids = Vec::new();
                let mut ok = true;
                for _ in 0..n {
                    let Some((q, e)) = ref_end(d, rec, o) else {
                        ok = false;
                        break;
                    };
                    let Some(padding) = rec.get(e..e + pad) else {
                        ok = false;
                        break;
                    };
                    if padding.iter().any(|&c| c != 0) {
                        ok = false;
                        break;
                    }
                    ids.push(q);
                    o = e + pad;
                }
                if ok {
                    hit = Some((ids, o));
                    break;
                }
            }
        }
        if let Some((ids, o)) = hit {
            out.push((j, ids));
            j = o;
        } else {
            j += 1;
        }
    }
    out
}

/// The sections of a record: each starts with `u32 3`, three ASCII digits and the record's own
/// id. (offset, tag); the first is the record's own tag at 0.
pub(super) fn sections(rec: &[u8]) -> Option<Vec<(usize, u32)>> {
    let mut r = Rd::new(rec, 0);
    let tag: u32 = r.str_()?.parse().ok()?;
    let oid = r.u64()?;
    let key = oid.to_le_bytes();
    let mut out = vec![(0usize, tag)];
    let mut j = r.i;
    while j + 15 <= rec.len() {
        if rec.get(j..j + 4) == Some(&[3, 0, 0, 0]) {
            let t = rec.get(j + 4..j + 7).unwrap_or(&[]);
            if t.iter().all(u8::is_ascii_digit) && rec.get(j + 7..j + 15) == Some(&key[..]) {
                let n = t.iter().fold(0u32, |a, c| a * 10 + u32::from(c - b'0'));
                out.push((j, n));
                j += 15;
                continue;
            }
        }
        j += 1;
    }
    Some(out)
}

/// What a record starts with: its tag, id, attributes and the offset of its body.
#[derive(Debug)]
pub(super) struct Header {
    pub attrs: HashMap<String, Vec<u8>>,
    pub body: usize,
}

const MAX_ATTRS: u32 = 64;

fn attr_size(t: &str) -> Option<usize> {
    if t.ends_with("RefMetaType") {
        return Some(10);
    }
    match t {
        "IntrinsicMetaTypeuint64" | "IntrinsicMetaTypeint64" | "IntrinsicMetaTypedouble" => Some(8),
        "IntrinsicMetaTypeuint32" | "IntrinsicMetaTypeint32" => Some(4),
        "IntrinsicMetaTypebool" => Some(1),
        _ if t.starts_with("EA46307F") => Some(16),
        _ => None,
    }
}

fn parse_attrs(rec: &[u8], i: usize) -> Option<(HashMap<String, Vec<u8>>, usize)> {
    let mut r = Rd::new(rec, i);
    let n = r.u32()?;
    if n > MAX_ATTRS {
        return None;
    }
    let mut attrs = HashMap::new();
    for _ in 0..n {
        let k = r.str_()?;
        let t = r.str_()?;
        if k.is_empty() || !k.chars().all(|c| (' '..='~').contains(&c)) {
            return None;
        }
        if t == "IntrinsicMetaTypeIString" {
            let s = r.wstr()?;
            attrs.insert(k, s.into_bytes());
            continue;
        }
        let sz = attr_size(&t)?;
        let raw = rec.get(r.i..r.i.checked_add(sz)?)?;
        attrs.insert(k, raw.to_vec());
        r.i += sz;
    }
    Some((attrs, r.i))
}

/// A record's header: after the id `u32 0`, then `u8 0` and `u8 flag` (2021+ classes) or just the
/// flag (older class versions); flag 1 = `u32 n` and `n` attributes (`str` key, `str` type, value).
pub(super) fn parse_header(rec: &[u8]) -> Option<Header> {
    let mut r = Rd::new(rec, 0);
    let _tag: u32 = r.str_()?.parse().ok()?;
    r.u64()?;
    r.u32()?;
    let o = r.i;
    if o + 2 > rec.len() {
        return None;
    }
    for flag_at in [o + 1, o] {
        if rec.get(flag_at) == Some(&1)
            && let Some((attrs, body)) = parse_attrs(rec, flag_at + 1)
        {
            return Some(Header { attrs, body });
        }
    }
    Some(Header { attrs: HashMap::new(), body: o + 2 })
}

/// Length-prefixed UTF-16LE strings (`u32` count and characters), printable ASCII only, in
/// order: (start, end, text). Empty strings are not found.
pub(super) fn wstrings(rec: &[u8], start: usize, end: usize) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let end = end.min(rec.len());
    let mut j = start;
    while j + 6 <= end {
        let n = u32_at(rec, j).unwrap_or(0) as usize;
        if (1..=4096).contains(&n) && j + 4 + 2 * n <= end {
            let s = rec.get(j + 4..j + 4 + 2 * n).unwrap_or(&[]);
            if s.chunks_exact(2).all(|c| c.get(1) == Some(&0) && c.first().is_some_and(|&x| (32..127).contains(&x))) {
                let text: String = s.chunks_exact(2).map(|c| char::from(c.first().copied().unwrap_or(b'?'))).collect();
                out.push((j, j + 4 + 2 * n, text));
                j += 4 + 2 * n;
                continue;
            }
        }
        j += 1;
    }
    out
}

pub(super) fn wstrings_all(rec: &[u8]) -> Vec<(usize, usize, String)> {
    wstrings(rec, 0, rec.len())
}

/// A model parameter (`7A6A3D31`): expression text, role name and value (internal units: cm, rad).
#[derive(Debug, Clone)]
pub(super) struct Param {
    pub expr: String,
    pub name: Option<String>,
    pub value: Option<f64>,
}

pub(super) fn param(d: &Design, pid: u64) -> Option<Param> {
    let r = d.rec(pid);
    let ws = wstrings_all(r);
    if ws.len() < 2 {
        return None;
    }
    let value = ws.last().and_then(|l| f64_at(r, l.1));
    Some(Param { expr: ws.first()?.2.clone(), name: ws.get(1).map(|w| w.2.clone()), value })
}
