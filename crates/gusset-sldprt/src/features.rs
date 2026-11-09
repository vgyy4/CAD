//! The feature history of a SolidWorks part (ADR 0043 amendment): `Config-0-ResolvedFeatures`
//! read into a Gusset feature tree (`.gtree.json`, format version 2; ADR 0042) with
//! sketches, extrudes, revolves and fillets/chamfers picked by edge midpoint.
//!
//! A port of the clean-room prototype in `scripts/re/solidworks/` (`swfeat.py` and its
//! modules); the layout notes are `docs/formats/solidworks-feature-tree.md`. The stream is
//! not walked object by object: features are found by anchors (the node-name object and
//! the 46-byte block after it), and their data by patterns inside each feature's span. Some
//! of it is guessed exactly as the prototype guesses (boss or cut from the class or a
//! "Cut-" name, the profile sketch is the one just before the feature); the caller rebuilds
//! the tree and keeps it only when it gives the part's own body.
//!
//! Input is untrusted: every scan is linear in a bounded stream, lists are capped, nothing
//! recurses, and any read past the data is an error (where the prototype raised) or a
//! skipped record (where it caught the error).

use crate::{Container, Limits, cfb, chunks, detect};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Largest feature stream (and `Config-0` stream) read.
pub const MAX_FEATURE_STREAM: usize = 64 << 20;
/// Most feature nodes in one part.
const MAX_NODES: usize = 2048;
/// Most node-name objects (features, parameters, ...) in one stream.
const MAX_NODE_NAMES: usize = 1 << 16;
/// Most matches of one pattern in one feature's span (entity records, parameters, end
/// conditions, edge records, plane candidates).
const MAX_MATCHES: usize = 1 << 16;
/// Longest feature name kept (UTF-16 units).
const MAX_NAME_UNITS: usize = 256;
/// Distance of a through-all extrude (mm), as the prototype emits it.
const BIG: f64 = 500.0;
const TAU: f64 = 2.0 * core::f64::consts::PI;

const PLANE_NAMES: [(u32, &str); 3] = [(2, "Front"), (3, "Top"), (4, "Right")];
const EXTRUDE: [&str; 3] = ["moExtrusion_c", "moICE_c", "moCut_c"];
const REVOLVE: [&str; 2] = ["moRevolution_c", "moRevCut_c"];
/// Classes that carry nothing the tree needs (sketches are handled before this list).
const SKIPPED: [&str; 13] = [
    "moCommentsFolder_c",
    "moFavoriteFolder_c",
    "moHistoryFolder_c",
    "moSensorFolder_c",
    "moDocsFolder_c",
    "moDetailCabinet_c",
    "moSurfaceBodyFolder_c",
    "moSolidBodyFolder_c",
    "moEqnFolder_c",
    "moMaterialFolder_c",
    "moRefPlane_c",
    "moOriginProfileFeature_c",
    "moProfileFeature_c",
];

/// The 46-byte block after a feature node's comment (older files, then newer ones).
const NODE_BLOCK: [u8; 18] = [0, 0xff, 0xff, 0, 0, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf];
const NODE_BLOCK3: [u8; 18] =
    [0, 0xff, 0xff, 0, 0, 3, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf];
/// A sketch entity header with all-`FF` middle bytes (versions 1 and 3).
const ENT_HDR: [u8; 17] = [0xff, 0xff, 0x1f, 0, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf];
const ENT_HDR3: [u8; 17] = [0xff, 0xff, 0x1f, 0, 3, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf];
/// f64 0.0 then f64 1.0.
const ONE: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xf0, 0x3f];
/// The two-f64 sentinel of an edge record.
const SENTINEL: [u8; 8] = [0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54];
const CLASS_DECL: [u8; 4] = [0xff, 0xff, 1, 0];
const UTF16_MARK: [u8; 3] = [0xff, 0xfe, 0xff];
const IDENT: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

// --- bounded reads ---------------------------------------------------------------------------

fn bytes_at<const N: usize>(d: &[u8], off: usize) -> Option<[u8; N]> {
    d.get(off..off.checked_add(N)?)?.try_into().ok()
}
fn u16_at(d: &[u8], off: usize) -> Option<u16> {
    bytes_at(d, off).map(u16::from_le_bytes)
}
fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    bytes_at(d, off).map(u32::from_le_bytes)
}
fn i32_at(d: &[u8], off: usize) -> Option<i32> {
    bytes_at(d, off).map(i32::from_le_bytes)
}
fn f64_at(d: &[u8], off: usize) -> Option<f64> {
    bytes_at(d, off).map(f64::from_le_bytes)
}
fn f64s_at<const N: usize>(d: &[u8], off: usize) -> Option<[f64; N]> {
    let mut out = [0.0; N];
    for (i, v) in out.iter_mut().enumerate() {
        *v = f64_at(d, off.checked_add(i.checked_mul(8)?)?)?;
    }
    Some(out)
}

/// A read the prototype does not guard: past the data, the part has no usable history.
fn need<T>(v: Option<T>, what: &str) -> Result<T, String> {
    v.ok_or_else(|| format!("the feature stream ends inside {what}"))
}

/// Exact comparison with a stored sentinel (1.0, -1.0): the bit pattern.
fn is(v: f64, want: f64) -> bool {
    v.to_bits() == want.to_bits()
}

/// First occurrence of `pat` lying wholly inside `d[from..to]`.
fn find(d: &[u8], pat: &[u8], from: usize, to: usize) -> Option<usize> {
    let hay = d.get(from..to.min(d.len()))?;
    hay.windows(pat.len()).position(|w| w == pat).map(|i| from + i)
}

/// Last occurrence of `pat` lying wholly inside `d[from..to]`.
fn rfind(d: &[u8], pat: &[u8], from: usize, to: usize) -> Option<usize> {
    let hay = d.get(from..to.min(d.len()))?;
    hay.windows(pat.len()).rposition(|w| w == pat).map(|i| from + i)
}

/// Python's `round(v, digits)` for the values written (ties to even).
fn round_to(v: f64, digits: i32) -> f64 {
    let p = libm::pow(10.0, f64::from(digits));
    let y = v * p;
    if !y.is_finite() {
        return v;
    }
    y.round_ties_even() / p
}

/// Metres to millimetres, 6 decimals.
fn mm(v: f64) -> f64 {
    round_to(v * 1000.0, 6)
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// UTF-16LE text, at most [`MAX_NAME_UNITS`] units; a dangling odd byte is a replacement
/// character.
fn utf16(raw: &[u8]) -> String {
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .take(MAX_NAME_UNITS)
        .filter_map(|c| <[u8; 2]>::try_from(c).ok().map(u16::from_le_bytes))
        .collect();
    let mut s = String::from_utf16_lossy(&units);
    if raw.len() % 2 == 1 && raw.len() / 2 < MAX_NAME_UNITS {
        s.push(char::REPLACEMENT_CHARACTER);
    }
    s
}

// --- the streams -----------------------------------------------------------------------------

/// `Config-0-ResolvedFeatures` and `Config-0`.
type Streams = (Vec<u8>, Vec<u8>);

/// `Config-0-ResolvedFeatures` and `Config-0`, or `None` when the part has no feature stream.
pub(crate) fn streams(bytes: &[u8], limits: &Limits) -> Result<Option<Streams>, String> {
    const RF: &str = "Config-0-ResolvedFeatures";
    const C0: &str = "Config-0";
    let limits = Limits { max_stream_bytes: limits.max_stream_bytes.min(MAX_FEATURE_STREAM), ..*limits };
    let mut found: HashMap<String, Vec<u8>> = HashMap::new();
    match detect(bytes).ok_or("not a SolidWorks part file")? {
        Container::Ole2 => {
            let f = cfb::Cfb::open(bytes, &limits).map_err(|e| e.to_string())?;
            for name in [RF, C0] {
                // A later entry of the same name replaces an earlier one, as in the prototype.
                if let Some(i) = f.streams.iter().rposition(|e| e.name == name) {
                    found.insert(name.to_string(), f.read(i, &limits).map_err(|e| e.to_string())?);
                }
            }
        }
        Container::Chunks => {
            let wanted = |n: &str| [RF, C0].iter().any(|w| n == *w || n.strip_prefix("Contents/") == Some(*w));
            for st in chunks::streams(bytes, &limits, wanted).map_err(|e| e.to_string())? {
                found.insert(st.name, st.data);
            }
        }
    }
    // The plain name first, unless it is empty (the prototype's `or`).
    let mut take = |name: &str| match found.remove(name) {
        Some(v) if !v.is_empty() => Some(v),
        plain => found.remove(&format!("Contents/{name}")).or(plain),
    };
    let (rf, c0) = (take(RF), take(C0));
    match (rf, c0) {
        (Some(rf), Some(c0)) => Ok(Some((rf, c0))),
        _ => Ok(None),
    }
}

// --- class declarations ----------------------------------------------------------------------

/// A class declaration at `i`: `FF FF <schema 0..9> 00 <len 2..64> 00 <name>`. Returns the
/// declared name and the position after it.
fn decl_at(d: &[u8], i: usize) -> Option<(&[u8], usize)> {
    let [a, b, schema, z0, len, z1] = bytes_at::<6>(d, i)?;
    if a != 0xff || b != 0xff || schema > 9 || z0 != 0 || !(2..=0x40).contains(&len) || z1 != 0 {
        return None;
    }
    let first = *d.get(i + 6)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let run = d.get(i + 6..)?.iter().take(64).take_while(|&&c| is_word(c)).count();
    let len = usize::from(len);
    if run < 2 || run < len {
        return None;
    }
    Some((d.get(i + 6..i + 6 + len)?, i + 6 + len))
}

/// The map index of the first `moUnitComponent_c` declared in `Config-0` (classes and
/// objects share one counter; the scan follows the prototype's `scan.py`).
fn unit_component_index(c0: &[u8]) -> Option<usize> {
    let mut classes: HashSet<usize> = HashSet::new();
    let mut next = 1usize;
    let mut i = 0usize;
    while i + 1 < c0.len() {
        if let Some((name, end)) = decl_at(c0, i) {
            if name == b"moUnitComponent_c" {
                return Some(next);
            }
            classes.insert(next);
            next += 2;
            i = end;
            continue;
        }
        let t = usize::from(u16_at(c0, i)?);
        if t & 0x8000 != 0 && t != 0xffff && classes.contains(&(t & 0x7fff)) {
            next += 1;
            i += 2;
            continue;
        }
        i += 1;
    }
    None
}

/// The two-byte tag of node-name objects: the first `mo..._c` class declaration directly
/// followed by an object tag and a UTF-16 string.
fn node_tag(d: &[u8]) -> Option<[u8; 2]> {
    let mut from = 0usize;
    while let Some(i) = find(d, &CLASS_DECL, from, d.len()) {
        from = i + 1;
        let Some([l0, l1]) = bytes_at::<2>(d, i + 4) else { continue };
        if l0 == b'\n' || l1 == b'\n' || d.get(i + 6..i + 8) != Some(b"mo") {
            continue;
        }
        if !d.get(i + 8).is_some_and(|&c| is_word(c)) {
            continue;
        }
        // `\w+?_c`: the shortest name that is followed by the tag and the string mark.
        let mut j = i + 9;
        loop {
            if d.get(j..j + 2) == Some(b"_c")
                && let Some([a, b]) = bytes_at::<2>(d, j + 2)
                && a != b'\n'
                && b != b'\n'
                && d.get(j + 4..j + 7) == Some(&UTF16_MARK[..])
            {
                return Some([a, b]);
            }
            if d.get(j).is_some_and(|&c| is_word(c)) {
                j += 1;
            } else {
                break;
            }
        }
    }
    None
}

/// Non-overlapping occurrences of `pat` in `d[from..to]`, at most `cap` (more is an error).
fn find_all(d: &[u8], pat: &[u8], from: usize, to: usize, what: &str) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    let mut at = from;
    while let Some(i) = find(d, pat, at, to) {
        out.push(i);
        if out.len() > MAX_MATCHES {
            return Err(format!("too many {what}"));
        }
        at = i + pat.len();
    }
    Ok(out)
}

// --- feature nodes ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Node {
    start: usize,
    cls: Option<String>,
    tag: Option<u16>,
    name: String,
    id: u32,
    post: usize,
    head_len: usize,
}

/// The feature nodes (`nodes2.feature_nodes`): a node-name object followed within 1500 bytes
/// by the 46-byte block; parameters (feature id -1) are not nodes.
fn feature_nodes(d: &[u8], nt: [u8; 2]) -> Result<Vec<Node>, String> {
    let pat = [nt[0], nt[1], UTF16_MARK[0], UTF16_MARK[1], UTF16_MARK[2]];
    let all = find_all(d, &pat, 0, d.len(), "node names")?;
    if all.len() > MAX_NODE_NAMES {
        return Err("too many node names".into());
    }
    let mut out = Vec::new();
    for p in all {
        let mut q = p + 5;
        let mut len = usize::from(*need(d.get(q), "a node name")?);
        if len == 0xff {
            len = usize::from(need(u16_at(d, q + 1), "a node name")?);
            q += 2;
        }
        let raw = d.get(q + 1..(q + 1 + 2 * len).min(d.len())).unwrap_or_default();
        let name = utf16(raw);
        let e = q + 1 + 2 * len;
        let window = e.saturating_add(1500 + NODE_BLOCK.len());
        let k = match (find(d, &NODE_BLOCK, e, window), find(d, &NODE_BLOCK3, e, window)) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => continue,
        };
        if k - e > 1500 {
            continue;
        }
        if need(u32_at(d, e + 8), "a node")? == 0xffff_ffff {
            continue; // a parameter
        }
        let id = need(u32_at(d, e + 8), "a node")?;
        let post = k + 56;
        need(u16_at(d, post - 2), "a node")?;
        // The class: declared right before the name object, or a class reference tag.
        let decl = rfind(d, &CLASS_DECL, p.saturating_sub(80), p)
            .filter(|&j| d.get(j + 4).is_some_and(|&l| j + 6 + usize::from(l) == p));
        let (start, cls, tag) = if let Some(j) = decl {
            let name = std::str::from_utf8(d.get(j + 6..p).unwrap_or_default())
                .map_err(|_| "a class name is not text".to_string())?;
            (j, Some(name.to_string()), None)
        } else {
            match p.checked_sub(2).and_then(|s| u16_at(d, s).map(|t| (s, t))) {
                Some((s, t)) if t & 0x8000 != 0 => (s, None, Some(t & 0x7fff)),
                _ => return Err(format!("feature node '{name}' has no class")),
            }
        };
        out.push(Node { start, cls, tag, name, id, post, head_len: k - (e + 12).min(k) });
        if out.len() > MAX_NODES {
            return Err(format!("more than {MAX_NODES} feature nodes"));
        }
    }
    Ok(out)
}

/// Maps class-reference tags to declared classes by signature (`nodes2.classify`, a guess
/// the prototype settles this way).
fn classify(d: &[u8], nodes: &mut [Node]) {
    let sig = |n: &Node| d.get(n.post..(n.post + 16).min(d.len())).unwrap_or_default();
    let strip = |s: &str| s.chars().filter(|c| !c.is_ascii_digit()).collect::<String>().trim().to_string();
    let mut tags: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if let Some(t) = n.tag {
            tags.entry(t).or_default().push(i);
        }
    }
    let decl: Vec<usize> = (0..nodes.len()).filter(|&i| nodes.get(i).is_some_and(|n| n.cls.is_some())).collect();
    let mut mapping: HashMap<u16, Option<String>> = HashMap::new();
    for (t, xs) in &tags {
        let xs: Vec<&Node> = xs.iter().filter_map(|&i| nodes.get(i)).collect();
        let Some(first) = xs.iter().map(|x| x.start).min() else { continue };
        let mut best: Option<((usize, usize), &str)> = None;
        for c in decl.iter().filter_map(|&i| nodes.get(i)) {
            if c.start >= first {
                continue;
            }
            let Some(cls) = c.cls.as_deref() else { continue };
            let cname = strip(&c.name);
            let sc = xs
                .iter()
                .map(|x| {
                    let same = sig(c).iter().zip(sig(x)).filter(|(a, b)| a == b).count();
                    same + if cname == strip(&x.name) { 8 } else { 0 } + if c.head_len == x.head_len { 4 } else { 0 }
                })
                .max()
                .unwrap_or(0);
            let key = (sc, c.start);
            if best.is_none_or(|(b, _)| key > b) {
                best = Some((key, cls));
            }
        }
        mapping.insert(*t, best.map(|(_, c)| c.to_string()));
    }
    for n in nodes.iter_mut() {
        if let Some(t) = n.tag {
            n.cls = mapping.get(&t).cloned().flatten();
        }
    }
}

// --- parameters and end conditions -----------------------------------------------------------

/// Dimension values in `[a, b)` (`feat.params`): node name, CString name, `u32 0, u32 flags,
/// u32 -1, u32 0, u16 0, f64 value`.
fn params(d: &[u8], a: usize, b: usize, nt: [u8; 2]) -> Result<Vec<f64>, String> {
    let pat = [nt[0], nt[1], UTF16_MARK[0], UTF16_MARK[1], UTF16_MARK[2]];
    let mut out = Vec::new();
    let mut at = a;
    while let Some(i) = find(d, &pat, at, b) {
        // The length byte is part of the match (and is not a newline).
        let Some(&n) = d.get(i + 5).filter(|_| i + 6 <= b) else { break };
        if n == b'\n' {
            at = i + 1;
            continue;
        }
        at = i + 6;
        let q = i + 6 + 2 * usize::from(n);
        let tail = d.get(q..q + 18);
        let ok = tail.is_some_and(|t| {
            t.get(..4) == Some(&[0, 0, 0, 0][..])
                && t.get(4) != Some(&b'\n')
                && t.get(5..7) == Some(&[0, 0][..])
                && matches!(t.get(7), Some(0x40 | 0xc0))
                && t.get(8..18) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0][..])
        });
        if !ok {
            continue;
        }
        out.push(need(f64_at(d, q + 18), "a parameter")?);
        if out.len() > MAX_MATCHES {
            return Err("too many parameters".into());
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
struct EndSpec {
    cls: Option<u16>,
    x: u32,
    kind: u32,
    flag: u32,
}

/// End-condition-like objects in `[a, b)` (`feat.endspec_candidates`): class reference tag,
/// `u16 0, u32 X (0/1), 10 x 00, u32 type, u32 flag`.
fn endspec_candidates(d: &[u8], a: usize, b: usize) -> Result<Vec<EndSpec>, String> {
    let mut out = Vec::new();
    let mut i = a;
    while i + 18 <= b.min(d.len()) {
        let Some(w) = d.get(i..i + 18) else { break };
        let matched = w.first() != Some(&b'\n')
            && w.get(1) != Some(&b'\n')
            && w.get(2..4) == Some(&[0, 0][..])
            && matches!(w.get(4), Some(0 | 1))
            && w.get(5..18).is_some_and(|z| z.iter().all(|&c| c == 0));
        if !matched {
            i += 1;
            continue;
        }
        let p = i;
        i += 18;
        let t = need(u16_at(d, p), "an end condition")?;
        if t == 0xffff || t & 0x8000 == 0 {
            continue;
        }
        let kind = need(u32_at(d, p + 18), "an end condition")?;
        let flag = need(u32_at(d, p + 22), "an end condition")?;
        if kind > 64 || flag > 1 {
            continue;
        }
        let x = u32::from(*need(d.get(p + 4), "an end condition")?);
        out.push(EndSpec { cls: Some(t & 0x7fff), x, kind, flag });
        if out.len() > MAX_MATCHES {
            return Err("too many end conditions".into());
        }
    }
    Ok(out)
}

/// A declared end-condition class in `[a, b)` (`feat.endspec_decl`).
fn endspec_decl(d: &[u8], a: usize, b: usize, name: &[u8]) -> Result<Option<EndSpec>, String> {
    let mut pat = CLASS_DECL.to_vec();
    pat.extend([u8::try_from(name.len()).unwrap_or(0), 0]);
    pat.extend_from_slice(name);
    let Some(i) = find(d, &pat, a, b) else { return Ok(None) };
    let p = i + 6 + name.len() - 2;
    let x = need(u32_at(d, p + 4), "an end condition")?;
    let kind = need(u32_at(d, p + 18), "an end condition")?;
    let flag = need(u32_at(d, p + 22), "an end condition")?;
    Ok(Some(EndSpec { cls: None, x, kind, flag }))
}

// --- sketches --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Pt {
    x: f64,
    y: f64,
    ext: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegKind {
    Line,
    Arc,
}

#[derive(Debug, Clone, Copy)]
struct Seg {
    kind: SegKind,
    id: u32,
    a: u32,
    s: usize,
    e: usize,
    /// Arc centre (0 for lines).
    c: usize,
    dir: i32,
    construction: bool,
    centerline: bool,
}

#[derive(Debug, Clone, Default)]
struct Geometry {
    pts: Vec<Option<Pt>>,
    segs: Vec<Seg>,
}

/// Entity headers in `[a, b)`: `FF FF 1F 00 <1|3> <8 bytes> 00 00 80 BF`, non-overlapping.
fn find_headers(d: &[u8], a: usize, b: usize) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    let mut i = a;
    let end = b.min(d.len());
    while i + 17 <= end {
        let w = d.get(i..i + 17)?;
        if w.get(..4) == Some(&[0xff, 0xff, 0x1f, 0][..])
            && matches!(w.get(4), Some(1 | 3))
            && w.get(13..17) == Some(&[0, 0, 0x80, 0xbf][..])
        {
            out.push(i);
            if out.len() > MAX_MATCHES {
                return None;
            }
            i += 17;
        } else {
            i += 1;
        }
    }
    Some(out)
}

/// The payload of the entity whose header is at `h`, and whether it references outside
/// geometry.
fn payload(d: &[u8], h: usize) -> Option<(Option<usize>, bool)> {
    let extra = if d.get(h + 4) == Some(&3) { 8 } else { 0 };
    let ext = u16_at(d, h + 35)?;
    if ext == 0 {
        let p = h + 40;
        if d.get(p..p + 16) == Some(&ONE[..]) {
            return Some((Some(p + 16 + extra), false));
        }
    }
    // An object chain in the reference slot: the payload follows the next 0.0/1.0 pair.
    Some((find(d, &ONE, h + 35, h + 2000).map(|k| k + 16 + extra), true))
}

/// The sketch lists (`sketch.parse_sketch`); `None` where the prototype raised (the sketch
/// is then skipped).
fn parse_sketch(d: &[u8], body: usize, end: usize) -> Option<Geometry> {
    let npts = usize::try_from(u32_at(d, body)?).ok()?;
    let hdrs = find_headers(d, body, end)?;
    if npts > hdrs.len() {
        return None;
    }
    let mut g = Geometry::default();
    for (k, &h) in hdrs.iter().enumerate() {
        let kind = u16_at(d, h + 29)?;
        let flags = u16_at(d, h + 37)?;
        let rid = u32_at(d, h.checked_sub(4)?)?;
        let aval = u32_at(d, h.checked_sub(8)?)?;
        let (p, ext) = payload(d, h)?;
        let Some(p) = p else { continue };
        if k < npts {
            let ptype = u16_at(d, p)?;
            if kind != 0 || !(ptype == 0x1e || ptype == 0x1f) {
                g.pts.push(None);
                continue;
            }
            let [x, y] = f64s_at::<2>(d, p + 2)?;
            g.pts.push(Some(Pt { x, y, ext }));
        } else {
            let (s, e, one) = (u16_at(d, p)?, u16_at(d, p + 2)?, u16_at(d, p + 4)?);
            let m1 = f64_at(d, p + 8)?;
            if one > 1 || !is(m1, -1.0) {
                continue;
            }
            let (s, e) = (usize::from(s), usize::from(e));
            let construction = flags & 8 != 0;
            if d.get(p + 22..p + 38).is_some_and(|w| w.chunks(4).all(|c| c == [0xfe, 0xff, 0xff, 0xff])) {
                let dir = i32_at(d, p + 16)?;
                let c = usize::from(u16_at(d, p + 20)?);
                g.segs.push(Seg {
                    kind: SegKind::Arc,
                    id: rid,
                    a: aval,
                    s,
                    e,
                    c,
                    dir,
                    construction,
                    centerline: false,
                });
            } else {
                g.segs.push(Seg {
                    kind: SegKind::Line,
                    id: rid,
                    a: aval,
                    s,
                    e,
                    c: 0,
                    dir: 0,
                    construction: construction || one == 0,
                    centerline: one == 0,
                });
            }
        }
    }
    Some(g)
}

#[derive(Debug, Clone, Copy)]
struct Plane {
    feature_id: Option<u32>,
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
}

fn orthonormal(v: &[f64; 9]) -> bool {
    if v.iter().any(|x| x.is_nan() || x.abs() > 1.000_000_1) {
        return false;
    }
    let [a0, a1, a2, b0, b1, b2, c0, c1, c2] = *v;
    let rows = [[a0, a1, a2], [b0, b1, b2], [c0, c1, c2]];
    if rows.iter().any(|[x, y, z]| (x * x + y * y + z * z - 1.0).abs() > 1e-9) {
        return false;
    }
    let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
    let [r0, r1, r2] = rows;
    dot(r0, r1).abs() < 1e-9 && dot(r0, r2).abs() < 1e-9 && dot(r1, r2).abs() < 1e-9
}

/// `v_sketch = R^T v_model + t`: the columns of R are the sketch axes, the origin is `-R t`.
fn frame(r: &[f64; 9], t: &[f64; 3], feature_id: Option<u32>) -> Plane {
    let [r00, r01, r02, r10, r11, r12, r20, r21, r22] = *r;
    let (x, y, z) = ([r00, r10, r20], [r01, r11, r21], [r02, r12, r22]);
    let o = |i: usize| -> f64 {
        let (Some(a), Some(b), Some(c)) = (x.get(i), y.get(i), z.get(i)) else { return 0.0 };
        -(a * t[0] + b * t[1] + c * t[2])
    };
    Plane { feature_id, origin: [o(0), o(1), o(2)], x, y }
}

/// The sketch plane inside a sketch's span (`sketch.find_plane`): a reference-plane
/// component, or a plane-data block, whichever comes first.
fn find_plane(d: &[u8], a: usize, b: usize, uc: Option<usize>) -> Result<Option<Plane>, String> {
    let mut tags: Vec<Vec<u8>> = Vec::new();
    let mut t = b"moUnitComponent_c\x02\x00".to_vec();
    t.extend([0u8; 10]);
    tags.push(t);
    if let Some(uc) = uc {
        let tag = u16::try_from(0x8000 | uc).map_err(|_| "a class index is out of range".to_string())?;
        let mut t = tag.to_le_bytes().to_vec();
        t.extend([2, 0]);
        t.extend([0u8; 10]);
        tags.push(t);
    }
    let mut hits = Vec::new();
    for tag in &tags {
        let mut i = a;
        while let Some(j) = find(d, tag, i, b) {
            hits.push(j + tag.len() - 12);
            if hits.len() > MAX_MATCHES {
                return Err("too many plane references".into());
            }
            i = j + 1;
        }
    }
    hits.sort_unstable();
    let mut best: Option<(usize, Plane)> = None;
    let mut offer = |at: usize, p: Plane| {
        if best.is_none_or(|(b, _)| at < b) {
            best = Some((at, p));
        }
    };
    for i0 in hits {
        let i = i0 - 2;
        let mut p = i0 + 43;
        let cid = need(u32_at(d, p), "a plane reference")?;
        need(u32_at(d, p + 4), "a plane reference")?;
        p += 8;
        if d.get(p..p + 2) != Some(&[0, 0][..]) {
            continue;
        }
        let k = need(u32_at(d, p + 2), "a plane reference")?;
        let hr = *need(d.get(p + 6), "a plane reference")?;
        let mut q = p + 7;
        if hr > 1 || k >= 16 {
            continue;
        }
        let r = if hr == 1 { need(f64s_at::<9>(d, q), "a plane reference")? } else { IDENT };
        if hr == 1 {
            q += 72;
        }
        let t = need(f64s_at::<3>(d, q), "a plane reference")?;
        let sc = need(f64_at(d, q + 24), "a plane reference")?;
        if is(sc, 1.0) && orthonormal(&r) && t.iter().all(|x| x.abs() < 10.0) {
            offer(i, frame(&r, &t, Some(cid)));
        }
    }
    // Plane-data blocks: origin, normal, hasRot, [R], t, 1.0.
    for i in a..b.saturating_sub(160) {
        let Some(&hr) = d.get(i + 48) else { break };
        if hr > 1 {
            continue;
        }
        let Some(nrm) = f64s_at::<3>(d, i + 24) else { break };
        if !nrm.iter().all(|x| x.abs() <= 1.000_000_1) || (nrm.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() > 1e-9 {
            continue;
        }
        let Some(org) = f64s_at::<3>(d, i) else { break };
        if !org.iter().all(|x| x.abs() < 10.0) {
            continue;
        }
        let r = if hr == 1 {
            let Some(r) = f64s_at::<9>(d, i + 49) else { break };
            r
        } else {
            IDENT
        };
        let q = i + 49 + if hr == 1 { 72 } else { 0 };
        if !orthonormal(&r) {
            continue;
        }
        if (0..3)
            .any(|row| (r.get(3 * row + 2).copied().unwrap_or(0.0) - nrm.get(row).copied().unwrap_or(0.0)).abs() > 1e-9)
        {
            continue;
        }
        let (Some(t), Some(sc)) = (f64s_at::<3>(d, q), f64_at(d, q + 24)) else { break };
        if !is(sc, 1.0) || !t.iter().all(|x| x.abs() < 10.0) {
            continue;
        }
        offer(i, frame(&r, &t, None));
    }
    Ok(best.map(|(_, p)| p))
}

/// The plane as written: origin in mm, axes to 12 decimals.
#[derive(Debug, Clone, Copy)]
struct JsonPlane {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
}

fn json_plane(plane: Option<&Plane>) -> JsonPlane {
    let axis = |v: [f64; 3]| v.map(|x| round_to(x, 12) + 0.0);
    match plane {
        Some(p) => JsonPlane { origin: p.origin.map(mm), x: axis(p.x), y: axis(p.y) },
        None => JsonPlane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] },
    }
}

fn sketch_json(name: &str, g: &Geometry, plane: Option<&Plane>, jp: &JsonPlane) -> Value {
    let mut pl = Map::new();
    pl.insert("origin".into(), json!(jp.origin));
    pl.insert("x_axis".into(), json!(jp.x));
    pl.insert("y_axis".into(), json!(jp.y));
    match plane {
        None => {
            pl.insert("unresolved".into(), json!(true));
        }
        Some(p) => {
            if let Some((_, n)) = PLANE_NAMES.iter().find(|(id, _)| Some(*id) == p.feature_id) {
                pl.insert("ref".into(), json!(n));
            }
        }
    }
    let pid = |i: usize| format!("p{i}");
    let mut points = Vec::new();
    for (i, p) in g.pts.iter().enumerate() {
        let Some(p) = p else { continue };
        let mut q = Map::new();
        q.insert("id".into(), json!(pid(i)));
        q.insert("x".into(), json!(mm(p.x)));
        q.insert("y".into(), json!(mm(p.y)));
        if p.ext {
            q.insert("external".into(), json!(true));
        }
        points.push(Value::Object(q));
    }
    let pt = |i: usize| g.pts.get(i).copied().flatten();
    let mut curves = Vec::new();
    for (k, s) in g.segs.iter().enumerate() {
        let c = if s.kind == SegKind::Arc { s.c } else { s.s };
        let (Some(ps), Some(_), Some(pc)) = (pt(s.s), pt(s.e), pt(c)) else { continue };
        let mut o = Map::new();
        o.insert("id".into(), json!(format!("c{k}")));
        match s.kind {
            SegKind::Line => {
                o.insert("type".into(), json!("line"));
                o.insert("start".into(), json!(pid(s.s)));
                o.insert("end".into(), json!(pid(s.e)));
            }
            SegKind::Arc if s.s == s.e => {
                o.insert("type".into(), json!("circle"));
                o.insert("center".into(), json!(pid(s.c)));
                o.insert("radius".into(), json!(mm(libm::hypot(ps.x - pc.x, ps.y - pc.y))));
            }
            SegKind::Arc => {
                // Counter-clockwise from start to end.
                let (a, b) = if s.dir > 0 { (s.s, s.e) } else { (s.e, s.s) };
                o.insert("type".into(), json!("arc"));
                o.insert("center".into(), json!(pid(s.c)));
                o.insert("start".into(), json!(pid(a)));
                o.insert("end".into(), json!(pid(b)));
            }
        }
        if s.construction {
            o.insert("construction".into(), json!(true));
        }
        if s.centerline {
            o.insert("centerline".into(), json!(true));
        }
        curves.push(Value::Object(o));
    }
    json!({"type": "sketch", "name": name, "plane": Value::Object(pl), "points": points, "curves": curves})
}

// --- fillet and chamfer edges ----------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Rep {
    tag: u16,
    fid: u32,
    idx: u32,
    ext: bool,
}

/// Edge records in `[a, b)` (`edges.edge_records`).
fn edge_records(d: &[u8], a: usize, b: usize) -> Result<Vec<(usize, Vec<Rep>)>, String> {
    const LEN: usize = 38;
    let mut out = Vec::new();
    let mut i = a;
    let end = b.min(d.len());
    while i + LEN <= end {
        let Some(w) = d.get(i..i + LEN) else { break };
        let matched = w.get(..8) == Some(&[1, 0, 0, 0, 0, 0, 0, 0][..])
            && w.get(9..13) == Some(&[0, 0, 0, 0][..])
            && w.get(14..16) == Some(&[0, 0][..])
            && w.get(20..28) == Some(&SENTINEL[..])
            && w.get(28..36) == Some(&SENTINEL[..])
            && w.get(36..38) == Some(&[0, 0][..]);
        if !matched {
            i += 1;
            continue;
        }
        let start = i;
        i += LEN;
        let n = usize::from(w.get(8).copied().unwrap_or(0));
        if n > 8 {
            continue;
        }
        let mut p = start + LEN;
        let mut reps = Vec::new();
        for k in 0..n {
            if p + 20 > d.len() {
                break;
            }
            let t = need(u16_at(d, p), "an edge record")?;
            let z = need(u16_at(d, p + 2), "an edge record")?;
            let fid = need(u32_at(d, p + 8), "an edge record")?;
            let idx = need(u32_at(d, p + 16), "an edge record")?;
            if t & 0x8000 == 0 || z != 0 {
                break;
            }
            p += 20;
            let next = need(u16_at(d, p), "an edge record")?;
            let ext =
                if k + 1 < n { next & 0x8000 == 0 } else { need(u32_at(d, p), "an edge record")? < 16 && idx <= 1 };
            if ext {
                p += 4;
            }
            reps.push(Rep { tag: t & 0x7fff, fid, idx, ext });
        }
        if reps.len() == n {
            out.push((start, reps));
            if out.len() > MAX_MATCHES {
                return Err("too many edge records".into());
            }
        }
    }
    Ok(out)
}

/// The edges the user picked: the first run of records one record apart.
fn selection(recs: Vec<(usize, Vec<Rep>)>) -> Vec<(usize, Vec<Rep>)> {
    let mut run: Vec<(usize, Vec<Rep>)> = Vec::new();
    for r in recs {
        match run.last() {
            Some(last) if r.0 - last.0 > 0xa0 => break,
            _ => run.push(r),
        }
    }
    run
}

/// A solid feature an edge can name: its sketch, plane and signed depth (mm).
#[derive(Debug, Clone, Copy)]
struct Swept {
    sketch: usize,
    plane: JsonPlane,
    distance: f64,
    symmetric: bool,
}

fn point(pts: &[Option<Pt>], i: usize) -> Result<Pt, String> {
    pts.get(i).copied().flatten().ok_or_else(|| "an edge names a sketch point that was not read".to_string())
}

fn seg_mid(pts: &[Option<Pt>], s: &Seg) -> Result<Option<(f64, f64)>, String> {
    let (p, q) = (point(pts, s.s)?, point(pts, s.e)?);
    if s.kind == SegKind::Line {
        return Ok(Some(((p.x + q.x) / 2.0, (p.y + q.y) / 2.0)));
    }
    let c = point(pts, s.c)?;
    let r = libm::hypot(p.x - c.x, p.y - c.y);
    let a0 = libm::atan2(p.y - c.y, p.x - c.x);
    let mut a1 = libm::atan2(q.y - c.y, q.x - c.x);
    if s.s == s.e {
        return Ok(None); // a full circle has no single midpoint
    }
    // atan2 lies in [-pi, pi]: two turns at most.
    for _ in 0..4 {
        if s.dir > 0 && a1 <= a0 {
            a1 += TAU;
        } else if s.dir <= 0 && a1 >= a0 {
            a1 -= TAU;
        }
    }
    let am = f64::midpoint(a0, a1);
    Ok(Some((c.x + r * libm::cos(am), c.y + r * libm::sin(am))))
}

fn lift(f: &Swept, x: f64, y: f64, h: f64) -> [f64; 3] {
    let (o, xa, ya) = (f.plane.origin, f.plane.x, f.plane.y);
    let n = [xa[1] * ya[2] - xa[2] * ya[1], xa[2] * ya[0] - xa[0] * ya[2], xa[0] * ya[1] - xa[1] * ya[0]];
    let at = |i: usize| -> f64 {
        let (Some(o), Some(a), Some(b), Some(n)) = (o.get(i), xa.get(i), ya.get(i), n.get(i)) else { return 0.0 };
        round_to(o + x * a + y * b + h * n, 6)
    };
    [at(0), at(1), at(2)]
}

/// The midpoint of an edge record (`edges.midpoint`): the first two references are the
/// edge's faces, the last two bound its end vertices.
fn midpoint(reps: &[Rep], swept: &HashMap<u32, Swept>, sketches: &[Geometry]) -> Result<Option<[f64; 3]>, String> {
    let [f1, f2, _, _] = reps else { return Ok(None) };
    let cap_tags: HashSet<u16> = reps.iter().filter(|r| r.ext).map(|r| r.tag).collect();
    let Some(f) = swept.get(&f1.fid) else { return Ok(None) };
    if f2.fid != f1.fid {
        return Ok(None);
    }
    let Some(g) = sketches.get(f.sketch) else { return Ok(None) };
    let seg = |r: &Rep| {
        let mut c = g.segs.iter().filter(|s| s.a == r.idx && !s.construction);
        match (c.next(), c.next()) {
            (Some(s), None) => Some(*s),
            _ => None,
        }
    };
    let d = f.distance;
    let height = |cap: u32| {
        if f.symmetric {
            if cap == 1 { d / 2.0 } else { -d / 2.0 }
        } else if cap == 1 {
            d
        } else {
            0.0
        }
    };
    let (c1, c2) = (cap_tags.contains(&f1.tag), cap_tags.contains(&f2.tag));
    if !c1 && !c2 {
        let (Some(a), Some(b)) = (seg(f1), seg(f2)) else { return Ok(None) };
        let mut common: Vec<usize> = [a.s, a.e].into_iter().filter(|v| *v == b.s || *v == b.e).collect();
        common.dedup();
        let [shared] = common.as_slice() else { return Ok(None) };
        let p = point(&g.pts, *shared)?;
        let h = if f.symmetric { 0.0 } else { d / 2.0 };
        return Ok(Some(lift(f, p.x * 1000.0, p.y * 1000.0, h)));
    }
    if c1 != c2 {
        let (side, cap) = if c1 { (f2, f1) } else { (f1, f2) };
        let Some(a) = seg(side) else { return Ok(None) };
        let Some((x, y)) = seg_mid(&g.pts, &a)? else { return Ok(None) };
        return Ok(Some(lift(f, x * 1000.0, y * 1000.0, height(cap.idx))));
    }
    Ok(None)
}

// --- the tree --------------------------------------------------------------------------------

/// The part's feature history as a Gusset feature tree (`gusset-feature-tree` version 2).
///
/// `Ok(None)`: the part has no feature stream (nothing to say). `Err(reason)`: the history
/// is there but cannot be written as a tree (a feature the reader does not know, an end
/// condition or edge it cannot resolve, a damaged stream); the reason names the feature.
///
/// # Errors
/// A short human-readable reason.
pub fn feature_tree(bytes: &[u8], file_name: &str, limits: &Limits) -> Result<Option<Value>, String> {
    let Some((rf, c0)) = streams(bytes, limits)? else { return Ok(None) };
    let file = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let tree = convert(&rf, &c0, file)?;
    check(&tree)?;
    Ok(Some(tree))
}

/// `swfeat.convert`: the tree, with unresolved features marked as the prototype marks them.
pub(crate) fn convert(rf: &[u8], c0: &[u8], file: &str) -> Result<Value, String> {
    let uc = unit_component_index(c0);
    let nt = node_tag(rf).ok_or("the feature stream has no node names")?;
    let mut nodes = feature_nodes(rf, nt)?;
    classify(rf, &mut nodes);
    let mut starts: Vec<usize> = nodes.iter().map(|n| n.start).collect();
    starts.push(rf.len());
    starts.sort_unstable();
    let span = |n: &Node| {
        let i = starts.partition_point(|&s| s <= n.start);
        (n.start, starts.get(i).copied().unwrap_or(rf.len()))
    };
    let is_in = |cls: &Option<String>, set: &[&str]| cls.as_deref().is_some_and(|c| set.contains(&c));
    // The end-condition class: the most frequent end-condition-shaped class reference with
    // X = 1 in solid features (ties: the first seen).
    let mut counts: Vec<(u16, usize)> = Vec::new();
    for n in &nodes {
        if is_in(&n.cls, &EXTRUDE) || is_in(&n.cls, &REVOLVE) {
            let (a, b) = span(n);
            for c in endspec_candidates(rf, a, b)? {
                let Some(cls) = c.cls.filter(|&k| k < 0x3000 && c.x == 1) else { continue };
                match counts.iter_mut().find(|(k, _)| *k == cls) {
                    Some((_, v)) => *v += 1,
                    None => counts.push((cls, 1)),
                }
            }
        }
    }
    let es_cls: Option<u16> = counts
        .iter()
        .fold(None::<(u16, usize)>, |b, &(k, v)| if b.is_none_or(|(_, bv)| v > bv) { Some((k, v)) } else { b })
        .map(|(k, _)| k);
    let in_es = |c: &EndSpec| c.cls.is_some() && c.cls == es_cls;

    let mut feats: Vec<Value> = Vec::new();
    let mut sketches: Vec<Geometry> = Vec::new();
    // name -> (index into `sketches`, written plane)
    let mut sketch_by_name: HashMap<String, (usize, JsonPlane)> = HashMap::new();
    let mut last_sketch: Option<String> = None;
    let mut solid = false;
    let mut first_solid: Option<String> = None;
    let mut swept: HashMap<u32, Swept> = HashMap::new();
    for n in &nodes {
        let mut cls = n.cls.clone();
        let (a, b) = span(n);
        if cls.as_deref() == Some("moProfileFeature_c") {
            let h = [find(rf, &ENT_HDR, n.post, b), find(rf, &ENT_HDR3, n.post, b)]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(b);
            let Some(geo) = h.checked_sub(14).and_then(|body| parse_sketch(rf, body, b)) else {
                last_sketch = None;
                continue;
            };
            let plane = find_plane(rf, a, b, uc)?;
            let jp = json_plane(plane.as_ref());
            feats.push(sketch_json(&n.name, &geo, plane.as_ref(), &jp));
            sketches.push(geo);
            sketch_by_name.insert(n.name.clone(), (sketches.len() - 1, jp));
            last_sketch = Some(n.name.clone());
        } else if is_in(&cls, &REVOLVE)
            && endspec_decl(rf, a, b, b"moRevEndSpec_c")?.is_none()
            && endspec_candidates(rf, a, b)?.iter().any(in_es)
            && !params(rf, a, b, nt)?.iter().any(|v| (v - TAU).abs() < 1e-9)
        {
            // A tag mapped to a revolve whose body looks like an extrude.
            cls = Some("moICE_c".into());
        }
        let op_for = |cut_class: &str, solid: bool| {
            if !solid {
                "new"
            } else if cls.as_deref() == Some(cut_class) || n.name.to_lowercase().starts_with("cut") {
                "cut"
            } else {
                "join"
            }
        };
        if is_in(&cls, &EXTRUDE) {
            let dec = endspec_decl(rf, a, b, b"moEndSpec_c")?;
            let cands: Vec<EndSpec> =
                dec.into_iter().chain(endspec_candidates(rf, a, b)?.into_iter().filter(in_es)).collect();
            let es = cands.first().copied();
            let ps = params(rf, a, b, nt)?;
            let op = op_for("moCut_c", solid);
            let typ = es.map(|e| e.kind);
            let end = match typ {
                Some(0) => "blind".to_string(),
                Some(1 | 2) => "through_all?".to_string(),
                Some(6) => "mid_plane".to_string(),
                Some(9) => "through_all_both".to_string(),
                Some(t) => format!("unknown({t})"),
                None => "unknown(None)".to_string(),
            };
            let flag1 = es.is_some_and(|e| e.flag == 1);
            let (mut distance, through_all, symmetric) = match typ {
                Some(t @ (0 | 6)) if !ps.is_empty() => (ps.first().map(|v| mm(*v)), false, t == 6),
                Some(1 | 2 | 9) if flag1 && op == "cut" => (Some(2.0 * BIG), true, true),
                Some(9) => (Some(2.0 * BIG), true, true),
                Some(2) => (Some(BIG), true, false),
                Some(1) if op == "cut" => (Some(BIG), true, false),
                _ => (ps.first().map(|v| mm(*v)), false, false),
            };
            // Cuts go into the material: against the sketch normal.
            if op == "cut" && !symmetric && distance.is_some_and(|v| v > 0.0) {
                distance = distance.map(|v| -v);
            }
            let mut js = Map::new();
            js.insert("type".into(), json!("extrude"));
            js.insert("name".into(), json!(n.name));
            js.insert("sketch".into(), json!(last_sketch));
            js.insert("profiles".into(), json!("all"));
            js.insert("operation".into(), json!(op));
            js.insert("target".into(), if op == "new" { Value::Null } else { json!(first_solid) });
            js.insert("end_condition".into(), json!(end));
            js.insert("distance".into(), json!(distance));
            if through_all {
                js.insert("through_all".into(), json!(true));
            }
            js.insert("symmetric".into(), json!(symmetric));
            let sw: Vec<Value> =
                cands.iter().take(3).map(|c| json!({"X": c.x, "type": c.kind, "flag": c.flag})).collect();
            js.insert("sw_end".into(), json!(sw));
            if let (Some(sk), Some(dist)) = (last_sketch.as_ref(), distance)
                && let Some(&(i, plane)) = sketch_by_name.get(sk)
            {
                swept.insert(n.id, Swept { sketch: i, plane, distance: dist, symmetric });
            }
            feats.push(Value::Object(js));
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if is_in(&cls, &REVOLVE) {
            let ps = params(rf, a, b, nt)?;
            let ang = ps.iter().find(|v| v.abs() <= TAU + 1e-9);
            let op = op_for("moRevCut_c", solid);
            let mut js = Map::new();
            js.insert("type".into(), json!("revolve"));
            js.insert("name".into(), json!(n.name));
            js.insert("sketch".into(), json!(last_sketch));
            js.insert("profiles".into(), json!("all"));
            js.insert("operation".into(), json!(op));
            js.insert("angle".into(), json!(ang.map_or(360.0, |v| round_to(v / (core::f64::consts::PI / 180.0), 9))));
            if op != "new" {
                js.insert("target".into(), json!(first_solid));
            }
            if let Some(&(i, _)) = last_sketch.as_ref().and_then(|s| sketch_by_name.get(s))
                && let Some(g) = sketches.get(i)
            {
                let line = |s: &Seg| s.kind == SegKind::Line;
                let axis = g
                    .segs
                    .iter()
                    .position(|s| line(s) && s.centerline && s.id != 0xffff_ffff)
                    .or_else(|| g.segs.iter().position(|s| line(s) && s.construction));
                js.insert("axis".into(), axis.map_or(Value::Null, |k| json!({"line": format!("c{k}")})));
            }
            feats.push(Value::Object(js));
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if matches!(cls.as_deref(), Some("Fillet_c" | "Chamfer_c")) {
            let fillet = cls.as_deref() == Some("Fillet_c");
            let ps = params(rf, a, b, nt)?;
            let sel = selection(edge_records(rf, a, b)?);
            let mut edges = Vec::new();
            let mut unresolved = sel.is_empty();
            for (_, reps) in &sel {
                match midpoint(reps, &swept, &sketches)? {
                    Some(m) => edges.push(json!({"midpoint": m})),
                    None => unresolved = true,
                }
            }
            let mut js = Map::new();
            js.insert("type".into(), json!(if fillet { "fillet" } else { "chamfer" }));
            js.insert("name".into(), json!(n.name));
            js.insert(if fillet { "radius" } else { "distance" }.into(), json!(ps.first().map(|v| mm(*v))));
            js.insert("edges".into(), json!(edges));
            if unresolved {
                js.insert("edges_unresolved".into(), json!(true));
                js.insert("edges_selected".into(), json!(sel.len()));
            }
            feats.push(Value::Object(js));
        } else if cls.is_none() || is_in(&cls, &SKIPPED) {
            continue;
        } else {
            feats.push(json!({"type": "unsupported", "name": n.name, "original_type": cls}));
        }
    }
    Ok(json!({
        "format": "gusset-feature-tree",
        "version": 2,
        "source": {"app": "solidworks", "file": file},
        "features": feats,
    }))
}

/// The first feature the tree cannot rebuild, as the prototype's rebuild script counts them
/// (unsupported features, unresolved planes, end conditions and edges).
fn check(tree: &Value) -> Result<(), String> {
    let feats = tree.get("features").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let mut solids = 0usize;
    for f in feats {
        let s = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or_default();
        let name = s("name");
        match s("type") {
            "unsupported" => {
                return Err(format!("{name} ({}) is not supported", s("original_type")));
            }
            "sketch" => {
                if f.pointer("/plane/unresolved").is_some() {
                    return Err(format!("{name}: the sketch plane was not found"));
                }
            }
            "extrude" | "revolve" => {
                solids += 1;
                if f.get("sketch").is_none_or(Value::is_null) {
                    return Err(format!("{name}: its sketch could not be read"));
                }
                if s("type") == "extrude" && f.get("distance").is_none_or(Value::is_null) {
                    return Err(format!("{name}: end condition {} is not supported", s("end_condition")));
                }
                if s("type") == "revolve" && f.get("axis").is_none_or(Value::is_null) {
                    return Err(format!("{name}: no axis line in its sketch"));
                }
            }
            kind @ ("fillet" | "chamfer") => {
                if f.get("edges_unresolved").is_some() {
                    let got = f.get("edges").and_then(Value::as_array).map_or(0, Vec::len);
                    let of = f.get("edges_selected").and_then(Value::as_u64).unwrap_or(0);
                    return Err(format!("{name}: {got} of {of} {kind} edges resolved"));
                }
                let size = if kind == "fillet" { "radius" } else { "distance" };
                if f.get(size).is_none_or(Value::is_null) {
                    return Err(format!("{name}: no {size} found"));
                }
            }
            _ => {}
        }
    }
    if solids == 0 {
        return Err("no extrude or revolve was read".into());
    }
    Ok(())
}
