//! What is in a design stream: every byte of every BulkStream record marked as decoded (taken by
//! a field the reader knows), known-skipped, or unknown (a reading aid, not used by the import).
//!
//! [`coverage`] runs the ordinary reading of the design (the same calls an import makes) with
//! [`crate::cover`] recording on, adds the framing every record has (header, attributes, section
//! heads), and tiles each record into named spans. The unknown spans carry guesses (references,
//! strings, plausible doubles and the integers at their start) and the doubles or integers in
//! them that equal a model parameter of a nearby feature: what a decoder could name next.

use super::stream::{Design, f64_at, parse_header, ref_ids, refs_at, sections, u32_at, wstrings};
use super::{Ctx, PARAM, PARAM_HOLDER, json, pattern_copies, pick_stream};
use crate::cover::{self, Claim, Kind};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// What a byte of a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpanKind {
    Unknown,
    Skipped,
    Decoded,
}

/// A run of bytes of one kind and name, relative to the record's start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
    pub kind: SpanKind,
    /// The field's name (`""` for unknown bytes).
    pub name: &'static str,
}

/// A typed value found inside an unknown span (a guess: nothing reads it there).
#[derive(Debug, Clone, PartialEq)]
pub enum Guess {
    /// A reference to an object, with the target's class.
    Ref { id: u64, class: String },
    /// A UTF-16 string.
    WStr(String),
    /// A `u32`-length ASCII string.
    Str(String),
    /// A double of plausible size.
    F64(f64),
}

/// A value in an unknown span equal to a model parameter of a nearby feature.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamHit {
    /// Offset in the record.
    pub at: usize,
    /// `"f64"` or `"u32"`.
    pub width: &'static str,
    /// The parameter's role name (`AlongDistance`, `CountU`, ...).
    pub param: String,
    pub value: f64,
}

/// One record of the BulkStream.
#[derive(Debug, Clone)]
pub struct RecordCover {
    pub id: u64,
    pub tag: Option<u32>,
    pub class: String,
    /// Offset in the BulkStream.
    pub offset: usize,
    pub bytes: Vec<u8>,
    /// The record tiled into spans, in order.
    pub spans: Vec<Span>,
    /// Where each section's body starts in the record, with the section's class (the first is
    /// the record's own class; its body starts after the header).
    pub sections: Vec<(usize, String)>,
    /// Typed values in the unknown spans: (offset, length, value).
    pub guesses: Vec<(usize, usize, Guess)>,
    pub param_hits: Vec<ParamHit>,
}

impl RecordCover {
    /// Bytes of each kind.
    pub fn tally(&self) -> [usize; 3] {
        let mut t = [0usize; 3];
        for s in &self.spans {
            if let Some(slot) = t.get_mut(s.kind as usize) {
                *slot += s.len;
            }
        }
        t
    }

    /// The section an offset is in: its class and the offset relative to the section's body.
    pub fn section_of(&self, at: usize) -> (&str, isize) {
        let mut best: (&str, usize) = (self.class.as_str(), 0);
        for (o, c) in &self.sections {
            if *o <= at {
                best = (c.as_str(), *o);
            }
        }
        (best.0, at as isize - best.1 as isize)
    }
}

/// Byte coverage of one design stream.
#[derive(Debug, Clone)]
pub struct Coverage {
    pub meta_len: usize,
    /// MetaStream bytes the framing reads (class table, index tables).
    pub meta_used: usize,
    pub bulk_len: usize,
    /// BulkStream bytes outside every indexed record.
    pub outside_records: usize,
    pub records: Vec<RecordCover>,
}

impl Coverage {
    /// Bytes of each kind ([`SpanKind`] order) over all records.
    pub fn tally(&self) -> [usize; 3] {
        let mut t = [0usize; 3];
        for r in &self.records {
            for (a, b) in t.iter_mut().zip(r.tally()) {
                *a += b;
            }
        }
        t
    }

    /// The record-by-record listing: each record's id, tag, class, offset and length, its known
    /// fields by name and its unknown spans in hex with the guesses inside them.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        let [u, s, d] = self.tally();
        let total = (u + s + d).max(1) as f64;
        let _ = writeln!(
            out,
            "MetaStream {} bytes, {} read by the framing\nBulkStream {} bytes, {} records, {} bytes outside records\n\
             decoded {d} ({:.1}%), known-skipped {s} ({:.1}%), unknown {u} ({:.1}%)\n",
            self.meta_len,
            self.meta_used,
            self.bulk_len,
            self.records.len(),
            self.outside_records,
            100.0 * d as f64 / total,
            100.0 * s as f64 / total,
            100.0 * u as f64 / total,
        );
        for r in &self.records {
            let [u, s, d] = r.tally();
            let tag = r.tag.map_or_else(|| "?".to_string(), |t| t.to_string());
            let _ = writeln!(
                out,
                "#{} tag {tag} {} @{:#x} len {}: decoded {d} skipped {s} unknown {u}",
                r.id,
                if r.class.is_empty() { "(no class)" } else { r.class.as_str() },
                r.offset,
                r.bytes.len()
            );
            for sp in &r.spans {
                let (sec, rel) = r.section_of(sp.start);
                let sec = sec.get(..8).unwrap_or(sec);
                match sp.kind {
                    SpanKind::Decoded | SpanKind::Skipped => {
                        let k = if sp.kind == SpanKind::Decoded { "decoded" } else { "skipped" };
                        let _ = writeln!(out, "  +{:04x} {:>5} {k:7} {} [{sec}{rel:+}]", sp.start, sp.len, sp.name);
                    }
                    SpanKind::Unknown => {
                        let bytes = r.bytes.get(sp.start..sp.start + sp.len).unwrap_or(&[]);
                        let _ = write!(out, "  +{:04x} {:>5} UNKNOWN [{sec}{rel:+}] ", sp.start, sp.len);
                        for b in bytes.iter().take(64) {
                            let _ = write!(out, "{b:02x}");
                        }
                        if bytes.len() > 64 {
                            out.push('…');
                        }
                        out.push('\n');
                        let ints: Vec<String> = bytes
                            .chunks_exact(4)
                            .take(8)
                            .map(|c| c.try_into().map_or(0, u32::from_le_bytes).to_string())
                            .collect();
                        if !ints.is_empty() {
                            let _ = writeln!(out, "         u32: {}", ints.join(" "));
                        }
                        for (at, len, g) in r.guesses.iter().filter(|g| g.0 >= sp.start && g.0 < sp.start + sp.len) {
                            let what = match g {
                                Guess::Ref { id, class } => format!("ref #{id} {}", class.get(..8).unwrap_or(class)),
                                Guess::WStr(t) => format!("wstr {t:?}"),
                                Guess::Str(t) => format!("str {t:?}"),
                                Guess::F64(v) => format!("f64 {v}"),
                            };
                            let _ = writeln!(out, "         +{at:04x} {len:>3} {what}");
                        }
                        for h in r.param_hits.iter().filter(|h| h.at >= sp.start && h.at < sp.start + sp.len) {
                            let _ = writeln!(
                                out,
                                "         +{:04x} {} = parameter {} ({})",
                                h.at, h.width, h.param, h.value
                            );
                        }
                    }
                }
            }
        }
        out
    }
}

/// Most bytes of design stream a coverage is computed for.
const MAX_BYTES: usize = 512 << 20;

/// The byte coverage of the design stream in `files` (as [`super::feature_tree`] takes them).
///
/// # Errors
/// When the files hold no single readable design stream.
pub fn coverage(files: &[(String, Vec<u8>)]) -> Result<Coverage, String> {
    let (meta, bulk) = pick_stream(files)?;
    if meta.len().saturating_add(bulk.len()) > MAX_BYTES {
        return Err("the design stream is too large".into());
    }
    let d = Design::new(meta, bulk).ok_or("the design stream cannot be framed")?;
    let acis: Vec<(&str, &[u8])> = files
        .iter()
        .filter(|(n, _)| n.ends_with(".smb") || n.ends_with(".smbh"))
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let rec = cover::Recording::start();
    {
        // The reading an import does: the feature tree and the pattern copies.
        let mut cx = Ctx::new(&d, &acis);
        let _ = json::design_json(&mut cx);
        let _ = pattern_copies(meta, bulk);
        let _ = crate::fusion::read_design(bulk);
        // The framing every record has.
        for &i in &d.ids {
            let r = d.rec(i);
            let _ = parse_header(r);
            let _ = sections(r);
        }
    }
    let claims = rec.finish(bulk);

    // Records by offset, each once.
    let mut spans: Vec<(usize, usize, u64)> = d.ids.iter().filter_map(|&i| d.span(i).map(|(o, e)| (o, e, i))).collect();
    spans.sort_unstable();
    spans.dedup_by_key(|s| s.0);
    let inside: usize = spans.iter().map(|(o, e, _)| e.saturating_sub(*o)).sum();
    let near = Neighbours::new(&d);
    let mut records = Vec::with_capacity(spans.len());
    // Claims sorted by start: each record takes the ones that start inside it.
    let mut k = 0;
    for &(o, e, id) in &spans {
        while claims.get(k).is_some_and(|c| c.start < o) {
            k += 1;
        }
        let mut mine: Vec<&Claim> = Vec::new();
        let mut j = k;
        while let Some(c) = claims.get(j).filter(|c| c.start < e) {
            mine.push(c);
            j += 1;
        }
        records.push(record(&d, &near, id, o, e, &mine));
    }
    Ok(Coverage {
        meta_len: meta.len(),
        meta_used: d.meta_used,
        bulk_len: bulk.len(),
        outside_records: bulk.len().saturating_sub(inside),
        records,
    })
}

fn record(d: &Design, near: &Neighbours, id: u64, o: usize, e: usize, claims: &[&Claim]) -> RecordCover {
    let bytes = d.rec(id);
    let n = e - o;
    // Per byte: kind and name; generic names first so a reader's own name overrides them.
    let mut kind = vec![SpanKind::Unknown; n];
    let mut name: Vec<&'static str> = vec![""; n];
    let mut order: Vec<&&Claim> = claims.iter().collect();
    order.sort_by_key(|c| (!cover::generic(c.name), c.kind == Kind::Decoded));
    for c in order {
        let k = if c.kind == Kind::Decoded { SpanKind::Decoded } else { SpanKind::Skipped };
        let from = c.start - o;
        for b in from..(from + c.len).min(n) {
            if let (Some(kb), Some(nb)) = (kind.get_mut(b), name.get_mut(b))
                && k >= *kb
            {
                *kb = k;
                *nb = c.name;
            }
        }
    }
    let mut spans: Vec<Span> = Vec::new();
    for (b, (k, nm)) in kind.iter().zip(&name).enumerate() {
        match spans.last_mut() {
            Some(s) if s.kind == *k && s.name == *nm => s.len += 1,
            _ => spans.push(Span { start: b, len: 1, kind: *k, name: nm }),
        }
    }
    let mut secs: Vec<(usize, String)> = Vec::new();
    if let Some(h) = parse_header(bytes) {
        secs.push((h.body, d.cls(id).to_string()));
    }
    for (so, tag) in sections(bytes).unwrap_or_default().into_iter().skip(1) {
        secs.push((so + 19, d.tag_class(tag).unwrap_or("").to_string()));
    }
    let unknown = |a: usize, len: usize| (a..a + len).all(|b| kind.get(b) == Some(&SpanKind::Unknown));
    let guesses = guesses(d, bytes, &unknown);
    let params = near.params(d, id);
    let mut param_hits = Vec::new();
    if !params.is_empty() {
        for a in 0..n.saturating_sub(3) {
            if unknown(a, 8)
                && let Some(v) = f64_at(bytes, a).filter(|v| v.is_finite() && v.abs() > 1e-9)
            {
                for (pn, pv) in &params {
                    if (v - pv).abs() <= 1e-12 * pv.abs().max(1.0) && (pv.abs() - 1.0).abs() > 1e-12 {
                        param_hits.push(ParamHit { at: a, width: "f64", param: pn.clone(), value: *pv });
                    }
                }
            }
            if unknown(a, 4)
                && let Some(v) = u32_at(bytes, a).filter(|v| *v >= 2)
            {
                for (pn, pv) in &params {
                    if *pv == f64::from(v) {
                        param_hits.push(ParamHit { at: a, width: "u32", param: pn.clone(), value: *pv });
                    }
                }
            }
        }
    }
    RecordCover {
        id,
        tag: bytes.get(4..7).and_then(|t| std::str::from_utf8(t).ok()).and_then(|t| t.parse().ok()),
        class: d.cls(id).to_string(),
        offset: o,
        bytes: bytes.to_vec(),
        spans,
        sections: secs,
        guesses,
        param_hits,
    }
}

/// References, strings and plausible doubles inside the unknown bytes of a record.
fn guesses(d: &Design, rec: &[u8], unknown: &dyn Fn(usize, usize) -> bool) -> Vec<(usize, usize, Guess)> {
    let mut out: Vec<(usize, usize, Guess)> = Vec::new();
    let mut taken = vec![false; rec.len()];
    let mut take = |a: usize, len: usize, g: Guess, out: &mut Vec<(usize, usize, Guess)>| {
        if unknown(a, len) && taken.get(a..a + len).is_some_and(|t| t.iter().all(|x| !x)) {
            if let Some(t) = taken.get_mut(a..a + len) {
                t.iter_mut().for_each(|x| *x = true);
            }
            out.push((a, len, g));
        }
    };
    for (at, q) in refs_at(d, rec, 0) {
        let len = if rec.get(at + 9) == Some(&0) { 10 } else { 49 };
        take(at, len, Guess::Ref { id: q, class: d.cls(q).to_string() }, &mut out);
    }
    for (a, e, t) in wstrings(rec, 0, rec.len()) {
        take(a, e - a, Guess::WStr(t), &mut out);
    }
    let mut a = 0;
    while a + 5 <= rec.len() {
        let n = u32_at(rec, a).unwrap_or(0) as usize;
        if (2..=256).contains(&n)
            && let Some(s) = rec.get(a + 4..a + 4 + n)
            && s.iter().all(|c| (0x20..0x7f).contains(c))
        {
            take(a, 4 + n, Guess::Str(s.iter().map(|&c| char::from(c)).collect()), &mut out);
            a += 4 + n;
        } else {
            a += 1;
        }
    }
    let mut a = 0;
    while a + 8 <= rec.len() {
        if let Some(v) = f64_at(rec, a).filter(|v| v.is_finite() && (1e-6..1e7).contains(&v.abs()))
            && unknown(a, 8)
            && taken.get(a..a + 8).is_some_and(|t| t.iter().all(|x| !x))
        {
            if let Some(t) = taken.get_mut(a..a + 8) {
                t.iter_mut().for_each(|x| *x = true);
            }
            out.push((a, 8, Guess::F64(v)));
            a += 8;
        } else {
            a += 1;
        }
    }
    out.sort_by_key(|g| g.0);
    out
}

/// Which records reference which, for finding the parameters near a record.
struct Neighbours {
    children: HashMap<u64, Vec<u64>>,
    parents: HashMap<u64, Vec<u64>>,
}

/// A record with more references than this is a list (a timeline, a compute order): not a
/// feature, and not followed.
const HUB: usize = 64;

impl Neighbours {
    fn new(d: &Design) -> Neighbours {
        let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut parents: HashMap<u64, Vec<u64>> = HashMap::new();
        for &i in &d.ids {
            let c = ref_ids(d, d.rec(i));
            for q in &c {
                parents.entry(*q).or_default().push(i);
            }
            children.insert(i, c);
        }
        Neighbours { children, parents }
    }

    fn kids(&self, i: u64) -> &[u64] {
        self.children.get(&i).map_or(&[], |v| v.as_slice())
    }

    /// Parameters (role name, value) of the records within two references up or down of `id`,
    /// lists left out; at most 64.
    fn params(&self, d: &Design, id: u64) -> Vec<(String, f64)> {
        let mut nodes: HashSet<u64> = HashSet::from([id]);
        let mut ups = vec![id];
        for _ in 0..2 {
            let mut next = Vec::new();
            for x in ups {
                for p in self.parents.get(&x).map_or(&[][..], |v| v.as_slice()) {
                    if self.kids(*p).len() <= HUB && nodes.insert(*p) {
                        next.push(*p);
                    }
                }
            }
            ups = next;
        }
        let mut downs: Vec<u64> = nodes.iter().copied().collect();
        for _ in 0..2 {
            let mut next = Vec::new();
            for x in downs {
                if self.kids(x).len() > HUB {
                    continue;
                }
                for q in self.kids(x) {
                    if nodes.insert(*q) {
                        next.push(*q);
                    }
                }
            }
            downs = next;
        }
        let mut pids: Vec<u64> = Vec::new();
        for &x in &nodes {
            if d.is(x, PARAM) {
                pids.push(x);
            } else if d.is(x, PARAM_HOLDER) {
                pids.extend(self.kids(x).iter().copied().filter(|q| d.is(*q, PARAM)));
            }
        }
        pids.sort_unstable();
        pids.dedup();
        pids.into_iter()
            .filter_map(|p| {
                let pr = super::stream::param(d, p)?;
                Some((pr.name?, pr.value?))
            })
            .take(64)
            .collect()
    }
}
