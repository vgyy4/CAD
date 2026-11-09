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
const SKIPPED: [&str; 23] = [
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
    "moSelectionSetFolder_c",
    "moNotesAreaFtrFolder_c",
    "moFtrFolder_c",
    "moFeatSurfaceBodyFolder_c",
    "moFeatSolidBodyFolder_c",
    "moRefAxisFtrFolder_c",
    "moRefPlaneFtrFolder_c",
    "moProfileFtrFolder_c",
    "moRefPlane_c",
    "moOriginProfileFeature_c",
    "moProfileFeature_c",
    // Annotations: a cosmetic thread and the configuration table add no geometry.
    "moCosmeticThread_c",
    "moConfigTableFolder_c",
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
/// The empty string some newer files write before the end of a parameter.
/// The low 20 bits of a feature node's flags word that mark a cut (a boss has `0x140`).
const CUT_FLAGS: u32 = 0x2_01ca;
const PARAM_CSTRING: [u8; 4] = [0xff, 0xfe, 0xff, 0];
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

/// `Config-<N>-ResolvedFeatures`, `Config-<N>`, and whether the part is a chunk-container file.
type Streams = (Vec<u8>, Vec<u8>, bool);

/// The configuration number of a stream called `[Contents/]Config-<N><suffix>`.
pub(crate) fn config_number(name: &str, suffix: &str) -> Option<u32> {
    let rest = name.strip_prefix("Contents/").unwrap_or(name).strip_prefix("Config-")?;
    let digits = rest.strip_suffix(suffix)?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The lowest configuration that has both streams among `names`.
fn lowest_config<'a>(names: impl Iterator<Item = &'a str> + Clone) -> Option<u32> {
    names
        .clone()
        .filter_map(|n| config_number(n, "-ResolvedFeatures"))
        .filter(|&k| names.clone().any(|n| config_number(n, "") == Some(k)))
        .min()
}

/// `Config-<N>-ResolvedFeatures` and `Config-<N>` of the lowest configuration N that has
/// both, or `None` when the part has no feature stream.
pub(crate) fn streams(bytes: &[u8], limits: &Limits) -> Result<Option<Streams>, String> {
    let limits = Limits { max_stream_bytes: limits.max_stream_bytes.min(MAX_FEATURE_STREAM), ..*limits };
    let mut found: HashMap<String, Vec<u8>> = HashMap::new();
    let chunk = match detect(bytes).ok_or("not a SolidWorks part file")? {
        Container::Ole2 => {
            let f = cfb::Cfb::open(bytes, &limits).map_err(|e| e.to_string())?;
            let Some(k) = lowest_config(f.streams.iter().map(|e| e.name.as_str())) else { return Ok(None) };
            for name in [format!("Config-{k}-ResolvedFeatures"), format!("Config-{k}")] {
                // A later entry of the same name replaces an earlier one, as in the prototype.
                if let Some(i) = f.streams.iter().rposition(|e| e.name == name) {
                    found.insert(name, f.read(i, &limits).map_err(|e| e.to_string())?);
                }
            }
            false
        }
        Container::Chunks => {
            let any = |n: &str| config_number(n, "-ResolvedFeatures").is_some() || config_number(n, "").is_some();
            let names = chunks::names(bytes, &limits, any);
            let Some(k) = lowest_config(names.iter().map(String::as_str)) else { return Ok(None) };
            let wanted = |n: &str| config_number(n, "-ResolvedFeatures") == Some(k) || config_number(n, "") == Some(k);
            for st in chunks::streams(bytes, &limits, wanted).map_err(|e| e.to_string())? {
                found.insert(st.name, st.data);
            }
            true
        }
    };
    let Some(k) = found.keys().find_map(|n| config_number(n, "-ResolvedFeatures")) else { return Ok(None) };
    // The plain name first, unless it is empty (the prototype's `or`).
    let mut take = |name: String| match found.remove(&name) {
        Some(v) if !v.is_empty() => Some(v),
        plain => found.remove(&format!("Contents/{name}")).or(plain),
    };
    let (rf, c0) = (take(format!("Config-{k}-ResolvedFeatures")), take(format!("Config-{k}")));
    match (rf, c0) {
        (Some(rf), Some(c0)) => Ok(Some((rf, c0, chunk))),
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

/// The class tag of `moUnitComponent_c` learned from the bytes of a chunk-container stream
/// (`py/uc.py`): the most frequent `XX 80 02 00 <10 x 00>`, the first seen winning ties.
/// The numbering of these files does not follow `Config-0`.
pub(crate) fn unit_component_tag(rf: &[u8]) -> Option<usize> {
    let mut counts: Vec<(u16, usize)> = Vec::new();
    let mut i = 0usize;
    while let Some(w) = rf.get(i..i + 14) {
        if w.get(1) == Some(&0x80)
            && w.get(2..4) == Some(&[2, 0][..])
            && w.get(4..).is_some_and(|z| z.iter().all(|&c| c == 0))
        {
            let key = u16::from_le_bytes([w.first().copied().unwrap_or(0), 0x80]) & 0x7fff;
            match counts.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => *v += 1,
                None => counts.push((key, 1)),
            }
            i += 14;
        } else {
            i += 1;
        }
    }
    counts
        .iter()
        .fold(None::<(u16, usize)>, |b, &(k, v)| if b.is_none_or(|(_, bv)| v > bv) { Some((k, v)) } else { b })
        .map(|(k, _)| usize::from(k))
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
    /// The flags word of the node: the low 20 bits tell a boss (`0x140`) from a cut (`0x201ca`).
    flags: u32,
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
        let flags = need(u32_at(d, e + 4), "a node")?;
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
        out.push(Node { start, cls, tag, name, flags, id, post, head_len: k - (e + 12).min(k) });
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
    Ok(named_params(d, a, b, nt)?.into_iter().map(|(_, v)| v).collect())
}

/// The same with each parameter's name (the hole wizard's are told apart by name).
fn named_params(d: &[u8], a: usize, b: usize, nt: [u8; 2]) -> Result<Vec<(String, f64)>, String> {
    let pat = [nt[0], nt[1], UTF16_MARK[0], UTF16_MARK[1], UTF16_MARK[2]];
    let mut out = Vec::new();
    let mut at = a;
    while let Some(i) = find(d, &pat, at, b) {
        // The length byte is part of the match (any byte: the prototype matches with DOTALL).
        let Some(&n) = d.get(i + 5).filter(|_| i + 6 <= b) else { break };
        at = i + 6;
        let q = i + 6 + 2 * usize::from(n);
        // `00 00 00 00, X 00 00 {40|c0}, ff ff ff ff, 00 00 00 00`, then an empty CString
        // (`ff fe ff 00`, newer files only) and `00 00`.
        let head = d.get(q..q + 16);
        let head_ok = head.is_some_and(|t| {
            t.get(..4) == Some(&[0, 0, 0, 0][..])
                && t.get(5..7) == Some(&[0, 0][..])
                && matches!(t.get(7), Some(0x40 | 0xc0))
                && t.get(8..16) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0][..])
        });
        let end = if d.get(q + 16..q + 20) == Some(&PARAM_CSTRING[..]) { q + 22 } else { q + 18 };
        if !head_ok || d.get(end - 2..end) != Some(&[0, 0][..]) {
            continue;
        }
        let name = utf16(d.get(i + 6..q).unwrap_or_default());
        out.push((name, need(f64_at(d, end), "a parameter")?));
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
        let matched = w.get(2..4) == Some(&[0, 0][..])
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
    /// The record id (`u32 -1` marks dimension helpers).
    id: u32,
    /// The u16 after the coordinates: 1 for a stand-alone point (a hole centre in a
    /// hole-wizard placement sketch), 0 for the end of a segment.
    alone: u16,
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
    let hdrs = find_headers(d, body, end)?;
    if hdrs.is_empty() {
        return Some(Geometry::default()); // an empty sketch has no entity headers at all
    }
    let npts = usize::try_from(u32_at(d, body)?).ok()?;
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
            let alone = u16_at(d, p + 18)?;
            g.pts.push(Some(Pt { x, y, ext, id: rid, alone }));
        } else {
            let (s, e, one) = (u16_at(d, p)?, u16_at(d, p + 2)?, u16_at(d, p + 4)?);
            let m1 = f64_at(d, p + 8)?;
            if one > 1 || !is(m1, -1.0) {
                continue;
            }
            let (s, e) = (usize::from(s), usize::from(e));
            // Bit 0x08 is construction; bit 0x01 is not part of the profile either
            // (confirmed by volume on baseJoint and Top_mainplate).
            let construction = flags & 9 != 0;
            // A line of kind 0 is a centerline only when its flags say so (0x8000, or the
            // construction bit): newer files write ordinary profile lines with kind 0.
            let centerline = one == 0 && flags & 0x8008 != 0;
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
                    construction: construction || centerline,
                    centerline,
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
    // The earliest candidate; a plane-data block also keeps its rotation (a face sketch's own
    // frame is looked for after it).
    let mut best: Option<(usize, Plane, Option<[f64; 9]>)> = None;
    let mut offer = |at: usize, p: Plane, face: Option<[f64; 9]>| {
        if best.is_none_or(|(b, _, _)| at < b) {
            best = Some((at, p, face));
        }
    };
    for i0 in hits {
        let i = i0 - 2;
        // The component base is 31 bytes after the unit component in older files and 55 in
        // chunk-container ones: try the id (`u32 id, u32 stamp, u16 0`) at both places.
        for off in [43usize, 67] {
            let p = i0 + off;
            let (Some(cid), Some(_)) = (u32_at(d, p), u32_at(d, p + 4)) else { continue };
            let p = p + 8;
            if !(1..0x400).contains(&cid) || d.get(p..p + 2) != Some(&[0, 0][..]) {
                continue;
            }
            let (Some(k), Some(&hr)) = (u32_at(d, p + 2), d.get(p + 6)) else { continue };
            if hr > 1 || k >= 16 {
                continue;
            }
            let mut q = p + 7;
            let r = if hr == 1 {
                let Some(r) = f64s_at::<9>(d, q) else { continue };
                q += 72;
                r
            } else {
                IDENT
            };
            let (Some(t), Some(sc)) = (f64s_at::<3>(d, q), f64_at(d, q + 24)) else { continue };
            if is(sc, 1.0) && orthonormal(&r) && t.iter().all(|x| x.abs() < 10.0) {
                offer(i, frame(&r, &t, Some(cid)), None);
                break;
            }
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
        offer(i, frame(&r, &t, None), Some(r));
    }
    let Some((at, plane, face)) = best else { return Ok(None) };
    let Some(r) = face else { return Ok(Some(plane)) };
    // A sketch on a model face: the first block is the face's plane data (its axes may be
    // turned 180 degrees about the normal); the sketch's own frame follows as
    // `01 00 00 00 <12 x 00> 01 <R 3x3> <t> 1.0` with the same normal column (confirmed by
    // the bounding box of FrameBracket's Sketch11).
    let mut mark = vec![1u8, 0, 0, 0];
    mark.extend([0u8; 12]);
    mark.push(1);
    let mut from = at + 49;
    while let Some(j) = find(d, &mark, from, b) {
        let q = j + mark.len();
        let r2 = need(f64s_at::<9>(d, q), "a sketch frame")?;
        if orthonormal(&r2)
            && (0..3).all(|row| {
                (r2.get(3 * row + 2).copied().unwrap_or(0.0) - r.get(3 * row + 2).copied().unwrap_or(0.0)).abs() < 1e-9
            })
        {
            let t2 = need(f64s_at::<3>(d, q + 72), "a sketch frame")?;
            let sc = need(f64_at(d, q + 96), "a sketch frame")?;
            if is(sc, 1.0) {
                return Ok(Some(frame(&r2, &t2, None)));
            }
        }
        from = j + 1;
    }
    Ok(Some(plane))
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
    normalize_lines(
        json!({"type": "sketch", "name": name, "plane": Value::Object(pl), "points": points, "curves": curves}),
    )
}

/// Most point-in-line tests `normalize_lines` makes for one sketch; a sketch needing more is
/// left as read.
const MAX_NORMALIZE_WORK: usize = 50_000_000;

/// Cleans a sketch for the region finder (`swfeat.normalize_lines`): merges coincident points,
/// splits lines at points lying on them, drops the walls two different segments share
/// (SolidWorks fills both loops; Gusset would ignore the sketch silently) and zero-length lines.
/// Construction lines, centerlines, arcs and circles are kept as they are.
pub(crate) fn normalize_lines(mut sk: Value) -> Value {
    let str_of = |c: &Value, k: &str| c.get(k).and_then(Value::as_str).map(str::to_string);
    let flag = |c: &Value, k: &str| c.get(k).and_then(Value::as_bool).unwrap_or(false);
    let Some(points) = sk.get("points").and_then(Value::as_array) else { return sk };
    let mut at: HashMap<String, (f64, f64)> = HashMap::new();
    let mut canon: HashMap<(u64, u64), String> = HashMap::new();
    let mut remap: HashMap<String, String> = HashMap::new();
    for q in points {
        let (Some(id), Some(x), Some(y)) =
            (str_of(q, "id"), q.get("x").and_then(Value::as_f64), q.get("y").and_then(Value::as_f64))
        else {
            return sk;
        };
        let key = ((round_to(x, 6) + 0.0).to_bits(), (round_to(y, 6) + 0.0).to_bits());
        let first = canon.entry(key).or_insert_with(|| id.clone()).clone();
        remap.insert(id.clone(), first);
        at.insert(id, (x, y));
    }
    let Some(curves) = sk.get("curves").and_then(Value::as_array) else { return sk };
    let mut curves = curves.clone();
    for c in &mut curves {
        for k in ["start", "end", "center"] {
            if let Some(v) = str_of(c, k)
                && let Some(to) = remap.get(&v)
                && let Some(slot) = c.get_mut(k)
            {
                *slot = json!(to);
            }
        }
    }
    let is_line = |c: &Value| c.get("type").and_then(Value::as_str) == Some("line");
    let profile = |c: &Value| !flag(c, "construction");
    let ids: std::collections::BTreeSet<String> = curves
        .iter()
        .filter(|c| matches!(c.get("type").and_then(Value::as_str), Some("line" | "arc")) && profile(c))
        .flat_map(|c| ["start", "end"].into_iter().filter_map(|k| str_of(c, k)))
        .collect();
    if curves.len().saturating_mul(ids.len()) > MAX_NORMALIZE_WORK {
        return sk;
    }
    type Pair = (String, String);
    let pair = |a: &str, b: &str| -> Pair { if a <= b { (a.into(), b.into()) } else { (b.into(), a.into()) } };
    // (curve, the piece's ends, the curve's own ends)
    let mut pieces: Vec<(Value, Option<Pair>, Option<Pair>)> = Vec::new();
    for c in &curves {
        let (Some(a_id), Some(b_id)) = (str_of(c, "start"), str_of(c, "end")) else {
            pieces.push((c.clone(), None, None));
            continue;
        };
        if is_line(c) && a_id == b_id {
            continue; // a merged zero-length helper
        }
        if !is_line(c) || flag(c, "construction") || flag(c, "centerline") {
            pieces.push((c.clone(), None, None));
            continue;
        }
        let (Some(&(ax, ay)), Some(&(bx, by))) = (at.get(&a_id), at.get(&b_id)) else {
            pieces.push((c.clone(), None, None));
            continue;
        };
        let (dx, dy) = (bx - ax, by - ay);
        let l2 = dx * dx + dy * dy;
        let mut cut: Vec<(f64, String)> = Vec::new();
        for i in &ids {
            if *i == a_id || *i == b_id {
                continue;
            }
            let Some(&(qx, qy)) = at.get(i) else { continue };
            let t = ((qx - ax) * dx + (qy - ay) * dy) / l2;
            if t > 1e-9 && t < 1.0 - 1e-9 && ((qx - ax) * dy - (qy - ay) * dx).abs() / libm::sqrt(l2) < 1e-6 {
                cut.push((t, i.clone()));
            }
        }
        cut.sort_by(|x, y| x.0.total_cmp(&y.0).then_with(|| x.1.cmp(&y.1)));
        let mut chain = vec![a_id.clone()];
        chain.extend(cut.into_iter().map(|(_, i)| i));
        chain.push(b_id.clone());
        for w in chain.windows(2) {
            if let [u, v] = w {
                pieces.push((c.clone(), Some((u.clone(), v.clone())), Some(pair(&a_id, &b_id))));
            }
        }
    }
    // A piece covered by two segments that differ is the shared wall of two adjacent loops.
    let mut owners: HashMap<Pair, Vec<Pair>> = HashMap::new();
    for (_, piece, orig) in &pieces {
        if let (Some((u, v)), Some(orig)) = (piece, orig) {
            let list = owners.entry(pair(u, v)).or_default();
            if !list.contains(orig) {
                list.push(orig.clone());
            }
        }
    }
    let (mut out, mut seen, mut nid) = (Vec::new(), std::collections::HashSet::new(), 0usize);
    for (c, piece, orig) in pieces {
        let (Some((u, v)), Some(orig)) = (piece, orig) else {
            out.push(c);
            continue;
        };
        let key = pair(&u, &v);
        if owners.get(&key).is_some_and(|o| o.len() > 1) || !seen.insert(key.clone()) {
            continue;
        }
        nid += 1;
        let mut d = c;
        let id = str_of(&d, "id").unwrap_or_default();
        if let Some(o) = d.as_object_mut() {
            o.insert("start".into(), json!(u));
            o.insert("end".into(), json!(v));
            if orig != key {
                o.insert("id".into(), json!(format!("{id}_{nid}")));
            }
        }
        out.push(d);
    }
    if let Some(slot) = sk.get_mut("curves") {
        *slot = Value::Array(out);
    }
    sk
}

// --- fillet and chamfer edges ----------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Rep {
    tag: u16,
    fid: u32,
    idx: u32,
    /// Followed by exactly one extra word and not the record's last reference: a cap face.
    ext: bool,
    /// Followed by two extra words, the first not -1 (sides have -1, 0): also a cap face.
    ext2: bool,
}

/// An edge record's header at `i` (`edges.HDR`): `01 00*7 n`, then either `00{4,6} kind(1..4)
/// 00 00 <u32 id>` (an edge) or `00{11}` (a face list, e.g. a mirror's), the two sentinels and
/// `00 00`. Returns `n` and the end of the header.
fn edge_header(d: &[u8], i: usize) -> Option<(usize, usize)> {
    let w = d.get(i..i + 9)?;
    let n = *w.get(8)?;
    if w.get(..8) != Some(&[1, 0, 0, 0, 0, 0, 0, 0][..]) || !(1..=16).contains(&n) {
        return None;
    }
    let sentinels_at = |s: usize| -> Option<usize> {
        (d.get(s..s + 8) == Some(&SENTINEL[..])
            && d.get(s + 8..s + 16) == Some(&SENTINEL[..])
            && d.get(s + 16..s + 18) == Some(&[0, 0][..]))
        .then_some(s + 18)
    };
    let zeros = d.get(i + 9..)?.iter().take(11).take_while(|&&c| c == 0).count();
    if (4..=6).contains(&zeros) {
        let k = i + 9 + zeros;
        if matches!(d.get(k), Some(1..=4)) && d.get(k + 1..k + 3) == Some(&[0, 0][..]) {
            // k + 3 .. k + 7 is the id.
            if let Some(end) = sentinels_at(k + 7) {
                return Some((usize::from(n), end));
            }
        }
    }
    if zeros == 11 {
        return sentinels_at(i + 20).map(|end| (usize::from(n), end));
    }
    None
}

/// Edge records in `[a, b)` (`edges.edge_records`). Each reference is a 20-byte core followed
/// by 0 to 2 extra words, depending on its class.
fn edge_records(d: &[u8], a: usize, b: usize) -> Result<Vec<(usize, Vec<Rep>)>, String> {
    let mut out = Vec::new();
    let mut i = a;
    let end = b.min(d.len());
    while i < end {
        let Some((n, mut p)) = edge_header(d, i).filter(|h| h.1 <= end) else {
            i += 1;
            continue;
        };
        let start = i;
        i = p;
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
            let last = k + 1 == n;
            let mut extra = 0;
            let mut first_extra = None;
            while extra < 2 && p + 4 <= d.len() {
                let n0 = need(u16_at(d, p), "an edge record")?;
                let n1 = need(u16_at(d, p + 2), "an edge record")?;
                if !last && n0 & 0x8000 != 0 && n1 == 0 {
                    break;
                }
                let w = need(u32_at(d, p), "an edge record")?;
                if last && !(w < 16 || w == 0xffff_ffff) {
                    break;
                }
                if last && extra == 0 && !matches!(w, 0 | 1 | 0xffff_ffff) {
                    break;
                }
                p += 4;
                extra += 1;
                if extra == 1 {
                    first_extra = Some(w);
                }
                if last && w != 0xffff_ffff {
                    break;
                }
            }
            let ext2 = extra == 2 && !last && idx <= 1 && first_extra.is_some_and(|w| w != 0xffff_ffff);
            reps.push(Rep { tag: t & 0x7fff, fid, idx, ext: extra == 1 && !last, ext2 });
        }
        // Face lists (mirror planes) hold n - 1 references.
        if reps.len() + 1 == n {
            // The last reference of a face list has no `last` treatment: its extras are not a cap's.
            for r in &mut reps {
                r.ext2 = false;
            }
        }
        if reps.len() == n || (reps.len() + 1 == n && !reps.is_empty()) {
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

/// Classes of cap references (`edges.cap_tags_of`): those followed by one extra word, or whose
/// index is only ever 0 or 1 while bounding a side/side edge (the `3Int` caps carry two extra
/// words like the sides).
fn cap_tags_of(records: &[(usize, Vec<Rep>)]) -> HashSet<u16> {
    let mut caps = HashSet::new();
    let mut idx: HashMap<u16, HashSet<u32>> = HashMap::new();
    for (_, reps) in records {
        for r in reps {
            idx.entry(r.tag).or_default().insert(r.idx);
            if r.ext || r.ext2 {
                caps.insert(r.tag);
            }
        }
    }
    for (_, reps) in records {
        let [r0, _, r2, r3] = reps.as_slice() else { continue };
        let both = (r2.idx == 0 && r3.idx == 1) || (r2.idx == 1 && r3.idx == 0);
        if r2.tag == r3.tag && both && r0.tag != r2.tag && idx.get(&r2.tag).is_some_and(|s| s.iter().all(|&v| v <= 1)) {
            caps.insert(r2.tag);
        }
    }
    caps
}

/// A solid feature an edge can name: its sketch, plane and signed depth (mm).
#[derive(Debug, Clone, Copy)]
struct Swept {
    sketch: usize,
    plane: JsonPlane,
    distance: f64,
    symmetric: bool,
    through: bool,
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normal(f: &Swept) -> V3 {
    cross(f.plane.x, f.plane.y)
}

fn cap_height(f: &Swept, cap: u32) -> f64 {
    let d = f.distance;
    if f.symmetric {
        if cap == 1 { d / 2.0 } else { -d / 2.0 }
    } else if cap == 1 {
        d
    } else {
        0.0
    }
}

fn lift(f: &Swept, x: f64, y: f64, h: f64) -> V3 {
    let (o, xa, ya) = (f.plane.origin, f.plane.x, f.plane.y);
    let n = cross(xa, ya);
    let at = |i: usize| -> f64 {
        let (Some(o), Some(a), Some(b), Some(n)) = (o.get(i), xa.get(i), ya.get(i), n.get(i)) else { return 0.0 };
        round_to(o + x * a + y * b + h * n, 6)
    };
    [at(0), at(1), at(2)]
}

fn point(pts: &[Option<Pt>], i: usize) -> Option<Pt> {
    pts.get(i).copied().flatten()
}

/// Number of samples on an arc when edges are intersected.
const ARC_STEPS: usize = 2000;

/// The 3D polyline (mm, on the sketch plane) of a sketch segment (`edges._poly`).
fn poly(f: &Swept, g: &Geometry, s: &Seg, work: &mut u64) -> Result<Option<Vec<V3>>, String> {
    let (Some(p), Some(q)) = (point(&g.pts, s.s), point(&g.pts, s.e)) else { return Ok(None) };
    let uv: Vec<(f64, f64)> = if s.kind == SegKind::Line {
        vec![(p.x, p.y), (q.x, q.y)]
    } else {
        let Some(c) = point(&g.pts, s.c) else { return Ok(None) };
        let r = libm::hypot(p.x - c.x, p.y - c.y);
        let a0 = libm::atan2(p.y - c.y, p.x - c.x);
        let mut a1 = libm::atan2(q.y - c.y, q.x - c.x);
        if s.s == s.e {
            a1 = a0 + TAU * if s.dir > 0 { 1.0 } else { -1.0 };
        } else {
            // atan2 lies in [-pi, pi]: two turns at most.
            for _ in 0..4 {
                if s.dir > 0 && a1 <= a0 {
                    a1 += TAU;
                } else if s.dir <= 0 && a1 >= a0 {
                    a1 -= TAU;
                }
            }
        }
        (0..=ARC_STEPS)
            .map(|k| {
                let a = a0 + (a1 - a0) * (k as f64) / (ARC_STEPS as f64);
                (c.x + r * libm::cos(a), c.y + r * libm::sin(a))
            })
            .collect()
    };
    *work = work.saturating_add(uv.len() as u64);
    if *work > MAX_EDGE_WORK {
        return Err(TOO_MUCH_WORK.into());
    }
    Ok(Some(uv.into_iter().map(|(u, v)| lift(f, u * 1000.0, v * 1000.0, 0.0)).collect()))
}

/// Moves `p` along `along` onto the plane of `f` at height `h` (`edges._to_plane`).
fn to_plane(p: V3, f: &Swept, along: V3, h: f64) -> Option<V3> {
    let n = normal(f);
    let o = lift(f, 0.0, 0.0, h);
    let den = dot(along, n);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = dot(sub(o, p), n) / den;
    Some([p[0] + t * along[0], p[1] + t * along[1], p[2] + t * along[2]])
}

/// 2D coordinates of `p` in a frame normal to `n` (`edges._flat`).
fn flat(p: V3, n: V3) -> (f64, f64) {
    let mut x = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let d = dot(x, n);
    x = [x[0] - d * n[0], x[1] - d * n[1], x[2] - d * n[2]];
    let l = libm::sqrt(dot(x, x));
    x = [x[0] / l, x[1] / l, x[2] / l];
    let y = [n[1] * x[2] - n[2] * x[1], n[2] * x[0] - n[0] * x[2], n[0] * x[1] - n[1] * x[0]];
    (dot(p, x), dot(p, y))
}

fn lengths(a: &[(f64, f64)]) -> Vec<f64> {
    let mut l = vec![0.0];
    for w in a.windows(2) {
        let (Some(p), Some(q), Some(&last)) = (w.first(), w.get(1), l.last()) else { break };
        l.push(last + libm::hypot(q.0 - p.0, q.1 - p.1));
    }
    l
}

const TOO_MUCH_WORK: &str = "the fillet edges need too much work to locate";

/// Most segment pairs intersected for one part: polylines of 2000 samples cost millions each.
const MAX_EDGE_WORK: u64 = 100_000_000;

/// The arc-length fractions on `a` where `a` and `b` cross, seen along `n`
/// (`edges._cross_params`).
fn cross_params(a: &[V3], b: &[V3], n: V3, work: &mut u64) -> Result<Vec<f64>, String> {
    *work = work.saturating_add((a.len() as u64).saturating_mul(b.len() as u64));
    if *work > MAX_EDGE_WORK {
        return Err(TOO_MUCH_WORK.into());
    }
    let a2: Vec<(f64, f64)> = a.iter().map(|p| flat(*p, n)).collect();
    let b2: Vec<(f64, f64)> = b.iter().map(|p| flat(*p, n)).collect();
    let l = lengths(&a2);
    let total = l.last().copied().unwrap_or(0.0);
    let mut out = Vec::new();
    for (k, pq) in a2.windows(2).enumerate() {
        let (Some(&p), Some(&q)) = (pq.first(), pq.get(1)) else { continue };
        for rs in b2.windows(2) {
            let (Some(&r), Some(&s)) = (rs.first(), rs.get(1)) else { continue };
            let den = (q.0 - p.0) * (s.1 - r.1) - (q.1 - p.1) * (s.0 - r.0);
            if den.abs() < 1e-15 {
                continue;
            }
            let t = ((r.0 - p.0) * (s.1 - r.1) - (r.1 - p.1) * (s.0 - r.0)) / den;
            let u = ((r.0 - p.0) * (q.1 - p.1) - (r.1 - p.1) * (q.0 - p.0)) / den;
            if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) {
                let (Some(&l0), Some(&l1)) = (l.get(k), l.get(k + 1)) else { continue };
                out.push(if total != 0.0 { (l0 + t * (l1 - l0)) / total } else { 0.0 });
                if out.len() > MAX_MATCHES {
                    return Err("too many edge crossings".into());
                }
            }
        }
    }
    Ok(out)
}

/// The point at arc-length fraction `frac` of a polyline (`edges._at`).
fn point_at(a: &[V3], frac: f64) -> Option<V3> {
    let mut l = vec![0.0];
    for w in a.windows(2) {
        let (Some(p), Some(q), Some(&last)) = (w.first(), w.get(1), l.last()) else { break };
        l.push(last + libm::sqrt(dot(sub(*q, *p), sub(*q, *p))));
    }
    let target = frac * l.last().copied().unwrap_or(0.0);
    for k in 1..a.len() {
        let (Some(&lk), Some(&lp)) = (l.get(k), l.get(k - 1)) else { break };
        if lk >= target {
            let t = if lk > lp { (target - lp) / (lk - lp) } else { 0.0 };
            let (p, q) = (a.get(k - 1)?, a.get(k)?);
            return Some([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1]), p[2] + t * (q[2] - p[2])]);
        }
    }
    a.last().copied()
}

/// Puts a point computed on a sampled arc back onto the exact circle (`edges._snap`).
fn snap(p: V3, f: &Swept, g: &Geometry, s: &Seg) -> V3 {
    if s.kind != SegKind::Arc {
        return p;
    }
    let (Some(c), Some(q)) = (point(&g.pts, s.c), point(&g.pts, s.s)) else { return p };
    let r = libm::hypot(q.x - c.x, q.y - c.y) * 1000.0;
    let n = normal(f);
    let c3 = lift(f, c.x * 1000.0, c.y * 1000.0, 0.0);
    let v = sub(p, c3);
    let h = dot(v, n);
    let w = [v[0] - h * n[0], v[1] - h * n[1], v[2] - h * n[2]];
    let l = libm::sqrt(dot(w, w));
    if l == 0.0 || l.is_nan() {
        return p;
    }
    [c3[0] + w[0] * r / l + h * n[0], c3[1] + w[1] * r / l + h * n[1], c3[2] + w[2] * r / l + h * n[2]]
}

/// A face an edge record names: a cap of a swept feature or one of its sides.
#[derive(Clone, Copy)]
enum Face<'a> {
    Cap(u32, &'a Swept, u32),
    Side(u32, &'a Swept, Seg),
}

fn round3(p: V3) -> V3 {
    p.map(|v| round_to(v, 6))
}

/// The midpoint of an edge record (`edges.midpoint`): the first two references are the
/// edge's faces, the last two bound its end vertices. The faces may belong to different
/// features (a cut's wall against a boss's cap).
fn midpoint(
    reps: &[Rep],
    swept: &HashMap<u32, Swept>,
    sketches: &[Geometry],
    cap_tags: &HashSet<u16>,
    work: &mut u64,
) -> Result<Option<V3>, String> {
    let [r0, r1, r2, r3] = reps else { return Ok(None) };
    let geo = |f: &Swept| sketches.get(f.sketch);
    let face = |r: &Rep| -> Option<Face<'_>> {
        let f = swept.get(&r.fid)?;
        if cap_tags.contains(&r.tag) {
            return Some(Face::Cap(r.fid, f, r.idx));
        }
        let mut c = geo(f)?.segs.iter().filter(|s| s.a == r.idx && !s.construction);
        match (c.next(), c.next()) {
            (Some(s), None) => Some(Face::Side(r.fid, f, *s)),
            _ => None,
        }
    };
    let (Some(fa), Some(fb)) = (face(r0), face(r1)) else { return Ok(None) };
    let (fc, fd) = (face(r2), face(r3));
    let sides: Vec<Face> = [fa, fb].into_iter().filter(|x| matches!(x, Face::Side(..))).collect();
    let caps: Vec<Face> = [fa, fb].into_iter().filter(|x| matches!(x, Face::Cap(..))).collect();
    match (sides.as_slice(), caps.as_slice()) {
        ([Face::Side(fid, f, s)], [Face::Cap(_, g, c)]) => {
            let Some(gf) = geo(f) else { return Ok(None) };
            let Some(a) = poly(f, gf, s, work)? else { return Ok(None) };
            let (mut lo, mut hi) = (0.0, 1.0);
            let mut cuts = Vec::new();
            for b in [fc, fd].into_iter().flatten() {
                if let Face::Side(bid, bf, bs) = b
                    && bid != *fid
                    && let Some(bg) = geo(bf)
                    && let Some(pb) = poly(bf, bg, &bs, work)?
                {
                    cuts.extend(cross_params(&a, &pb, normal(f), work)?);
                }
            }
            if cuts.len() >= 2 {
                lo = cuts.iter().copied().fold(f64::INFINITY, f64::min);
                hi = cuts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            }
            let Some(m) = point_at(&a, f64::midpoint(lo, hi)) else { return Ok(None) };
            let Some(m) = to_plane(m, g, normal(f), cap_height(g, *c)) else { return Ok(None) };
            Ok(Some(round3(snap(m, f, gf, s))))
        }
        ([Face::Side(ida, f, sa), Face::Side(idb, g, sb)], _) => {
            let (Some(gf), Some(gg)) = (geo(f), geo(g)) else { return Ok(None) };
            let p0 = if ida == idb {
                let mut common: Vec<usize> = [sa.s, sa.e].into_iter().filter(|v| *v == sb.s || *v == sb.e).collect();
                common.dedup();
                let [shared] = common.as_slice() else { return Ok(None) };
                let Some(p) = point(&gf.pts, *shared) else { return Ok(None) };
                lift(f, p.x * 1000.0, p.y * 1000.0, 0.0)
            } else {
                if (dot(normal(f), normal(g)).abs() - 1.0).abs() > 1e-9 {
                    return Ok(None);
                }
                let (Some(pa), Some(pb)) = (poly(f, gf, sa, work)?, poly(g, gg, sb, work)?) else { return Ok(None) };
                let ts = cross_params(&pa, &pb, normal(f), work)?;
                let [t] = ts.as_slice() else { return Ok(None) };
                let Some(at) = point_at(&pa, *t) else { return Ok(None) };
                snap(at, f, gf, sa)
            };
            // Height: halfway between the bounding caps, or between this feature's own caps.
            let bc: Vec<(u32, &Swept, u32)> = [fc, fd]
                .into_iter()
                .flatten()
                .filter_map(|x| if let Face::Cap(i, s, c) = x { Some((i, s, c)) } else { None })
                .collect();
            let (g2, hs) = match bc.as_slice() {
                [(i0, s0, c0), (i1, _, c1)] if i0 == i1 && !s0.through => {
                    (*s0, [cap_height(s0, *c0), cap_height(s0, *c1)])
                }
                _ => {
                    let Some(g2) = [*f, *g].into_iter().find(|x| !x.through) else { return Ok(None) };
                    (g2, [cap_height(g2, 0), cap_height(g2, 1)])
                }
            };
            Ok(to_plane(p0, g2, normal(f), f64::midpoint(hs[0], hs[1])).map(round3))
        }
        _ => Ok(None),
    }
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
    let Some((rf, c0, chunk)) = streams(bytes, limits)? else { return Ok(None) };
    let file = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let tree = convert_in(&rf, &c0, file, chunk)?;
    check(&tree)?;
    Ok(Some(tree))
}

/// A hole-wizard feature being read: its node is followed by a placement sketch and a
/// section sketch, and then the hole's dimension parameters.
struct HoleState {
    /// Index of the placeholder in the feature list.
    at: usize,
    name: String,
    /// Sketches seen after the node (two belong to the hole).
    sketches: u8,
    params: Vec<(String, f64)>,
    positions: Vec<V3>,
    face: Option<(V3, V3)>,
    target: Option<String>,
}

fn json_v3(v: V3) -> Value {
    json!([v[0], v[1], v[2]])
}

/// `swfeat.finish_hole`: the hole's parameters by name (values in metres and radians).
fn finish_hole(h: HoleState) -> Value {
    let ps: HashMap<&str, f64> = h.params.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let get = |names: &[&str]| names.iter().find_map(|k| ps.get(k).copied());
    let nonzero = |v: Option<f64>| v.filter(|x| *x != 0.0);
    let (mut dia, mut dep) = (
        get(&["Hole Dia.", "Thru Hole Dia.", "Tap Drill Dia."]),
        get(&["Hole Depth", "Thru Hole Depth", "Tap Drill Depth"]),
    );
    if dia.is_none() {
        // A localised file ("Diamètre du perçage", "Profondeur du perçage", "Angle de pointe"): the
        // user-facing parameters are those not called `D<n>`, in the order diameter, depth, tip angle.
        let mut named: Vec<(&str, f64)> = Vec::new();
        for (k, v) in &h.params {
            let all_digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
            if all_digits(k) || k.strip_prefix('D').is_some_and(all_digits) {
                continue;
            }
            match named.iter_mut().find(|(n, _)| n == k) {
                Some(slot) => slot.1 = *v,
                None => named.push((k, *v)),
            }
        }
        if let [(_, d), (_, p), ..] = named[..] {
            (dia, dep) = (Some(d), Some(p));
        }
    }
    let (dia, dep) = (nonzero(dia), nonzero(dep));
    let cs_d = nonzero(get(&["C'Sink Dia.", "Near C'Sink Dia."]));
    let cs_a = nonzero(get(&["C'Sink Angle", "Near C'Sink Angle"]));
    let clearance = nonzero(get(&["Head Clearance"]));
    let mut o = Map::new();
    o.insert("type".into(), json!("hole"));
    o.insert("name".into(), json!(h.name));
    o.insert("positions".into(), Value::Array(h.positions.iter().map(|p| json_v3(*p)).collect()));
    if h.target.is_some() {
        o.insert("target".into(), json!(h.target));
    }
    if let Some(d) = dia {
        o.insert("diameter".into(), json!(mm(d)));
    }
    if let Some(d) = dep {
        o.insert("depth".into(), json!(mm(d)));
        o.insert("point_angle".into(), json!(118.0));
    }
    if let Some(d) = cs_d {
        o.insert("variant".into(), json!("countersink"));
        o.insert("csink_diameter".into(), json!(mm(d)));
        o.insert("csink_angle".into(), json!(cs_a.map_or(90.0, |a| round_to(a.to_degrees(), 9))));
    } else {
        o.insert("variant".into(), json!("simple"));
    }
    if let Some(c) = clearance {
        o.insert("head_clearance".into(), json!(mm(c)));
    }
    match (&h.face, dia, h.positions.is_empty()) {
        (Some((point, normal)), Some(_), false) => {
            o.insert("face".into(), json!({"point": json_v3(*point), "normal": json_v3(*normal)}));
        }
        _ => {
            o.insert("unresolved".into(), json!(true));
        }
    }
    Value::Object(o)
}

/// `swfeat.expand_holes`: head clearance (confirmed by volume on motorClip) is a cylinder of
/// the countersink diameter, `head_clearance` deep, above the countersink. It is written as a
/// circle pocket per position and then one hole per position from the pocket floor (Gusset
/// drops a hole whose positions lie on different faces, so each position gets its own hole).
fn expand_holes(feats: Vec<Value>) -> Vec<Value> {
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    let mut out = Vec::new();
    for f in feats {
        let plan = (|| {
            if f.get("type")?.as_str()? != "hole"
                || f.get("unresolved").is_some()
                || f.get("variant")?.as_str()? != "countersink"
            {
                return None;
            }
            let c = f.get("head_clearance")?.as_f64().filter(|c| *c != 0.0)?;
            let n = v3(f.pointer("/face/normal"))?;
            let r = f.get("csink_diameter")?.as_f64()? / 2.0;
            let pos: Vec<V3> = f.get("positions")?.as_array()?.iter().map(|p| v3(Some(p))).collect::<Option<_>>()?;
            Some((c, n, r, pos))
        })();
        let Some((c, n, r, pos)) = plan else {
            out.push(f);
            continue;
        };
        let mut x = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let dd = dot(x, n);
        x = [x[0] - dd * n[0], x[1] - dd * n[1], x[2] - dd * n[2]];
        let l = libm::sqrt(dot(x, x));
        x = [x[0] / l, x[1] / l, x[2] / l];
        let y = cross(n, x);
        let name = f.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        for (k, p) in pos.iter().enumerate() {
            let nm = format!("{name}_{}", k + 1);
            out.push(json!({
                "type": "sketch", "name": format!("{nm}_clearance"),
                "plane": {"origin": json_v3(*p), "x_axis": json_v3(x), "y_axis": json_v3(y)},
                "points": [{"id": "c", "x": 0.0, "y": 0.0}],
                "curves": [{"id": "k", "type": "circle", "center": "c", "radius": r}],
            }));
            out.push(json!({
                "type": "extrude", "name": format!("{nm}_clearance_cut"), "sketch": format!("{nm}_clearance"),
                "profiles": "all", "distance": -c, "operation": "cut", "target": f.get("target"),
            }));
            let q = round3([p[0] - c * n[0], p[1] - c * n[1], p[2] - c * n[2]]);
            let mut g = f.as_object().cloned().unwrap_or_default();
            g.remove("head_clearance");
            g.insert("name".into(), json!(nm));
            g.insert("face".into(), json!({"point": json_v3(q), "normal": json_v3(n)}));
            g.insert("positions".into(), json!([json_v3(q)]));
            out.push(Value::Object(g));
        }
    }
    out
}

/// `swfeat.convert`: the tree, with unresolved features marked as the prototype marks them.
#[cfg(test)]
pub(crate) fn convert(rf: &[u8], c0: &[u8], file: &str) -> Result<Value, String> {
    convert_in(rf, c0, file, false)
}

/// `convert` for a part of the given container: a chunk-container part (2015 and newer)
/// learns its unit-component tag from the bytes of the feature stream.
/// A distance-angle chamfer (`D1` a length, `D2` an angle in radians) as real `chamfer`
/// features with `angle` (degrees) and a `face` pick: the face `D1` runs along.
///
/// For an edge between two side faces of one sketch (two lines meeting at a corner) `D1` runs
/// along the face shared by the most selected edges (swfeat.py, guessed from one sample: the
/// truth of plaque_superieure puts it on the face two selected corners have in common; ties
/// go to the lower face key). The edges are grouped by that face and each group becomes one
/// chamfer (`name`, `name_2`, ...) whose `face` pick is a point on the face next to the first
/// edge, so the pick (which applies to every selected edge bordering the face) names it for
/// the whole group. An edge that is not such a corner has no known reference face: those go
/// in a last chamfer without a pick (the default face). `None` where no edge could be placed.
fn chamfer_angle(
    name: &str,
    (d1, angle): (f64, f64),
    edges: &[Option<V3>],
    sel: &[(usize, Vec<Rep>)],
    c: &Corners,
) -> Option<Vec<Value>> {
    // Per edge: the two candidate (fid, line key) faces, and for each the 3D direction away
    // from the corner and the line's length (mm).
    type Cand = ((u32, u32), V3, f64);
    let cands: Vec<Option<[Cand; 2]>> = sel
        .iter()
        .map(|(_, reps)| {
            let (fa, fb) = (reps.first()?, reps.get(1)?);
            if fa.fid != fb.fid {
                return None;
            }
            let f = c.swept.get(&fa.fid)?;
            let g = c.sketches.get(f.sketch)?;
            let lines = |idx: u32| -> Vec<Seg> {
                g.segs.iter().filter(|q| q.a == idx && q.kind == SegKind::Line && !q.construction).copied().collect()
            };
            let (sa, sb) = (lines(fa.idx), lines(fb.idx));
            let ([sa], [sb]) = (&sa[..], &sb[..]) else { return None };
            let ends = |q: &Seg| [q.s, q.e];
            let common: Vec<usize> = ends(sa).into_iter().filter(|e| ends(sb).contains(e)).collect();
            let [corner] = common[..] else { return None };
            let p = point(&g.pts, corner)?;
            let origin = lift(f, 0.0, 0.0, 0.0);
            let far = |q: &Seg, key: u32| -> Option<Cand> {
                let other = if q.s == corner { q.e } else { q.s };
                let o = point(&g.pts, other)?;
                let (dx, dy) = (o.x - p.x, o.y - p.y);
                let len = libm::hypot(dx, dy);
                if len < 1e-9 {
                    return None;
                }
                let d = sub(lift(f, dx / len, dy / len, 0.0), origin);
                Some(((fa.fid, key), d, len * 1000.0))
            };
            Some([far(sa, fa.idx)?, far(sb, fb.idx)?])
        })
        .collect();
    let mut groups: Vec<(Option<Cand>, Vec<V3>)> = Vec::new();
    let mut left: Vec<usize> = (0..sel.len()).filter(|&k| cands.get(k).is_some_and(Option::is_some)).collect();
    let mut loose: Vec<V3> = Vec::new();
    while !left.is_empty() {
        // Faces counted over the edges not placed yet.
        let mut uses: HashMap<(u32, u32), usize> = HashMap::new();
        for cd in left.iter().filter_map(|&k| cands.get(k)?.as_ref()).flatten() {
            *uses.entry(cd.0).or_default() += 1;
        }
        let best = left
            .iter()
            .filter_map(|&k| cands.get(k)?.as_ref())
            .flatten()
            .map(|cd| (uses.get(&cd.0).copied().unwrap_or(0), std::cmp::Reverse(cd.0)))
            .max()?;
        let face = best.1.0;
        let mut mids = Vec::new();
        let mut pick = None;
        left.retain(|&k| {
            let Some(Some(pair)) = cands.get(k) else { return true };
            let Some(cd) = pair.iter().find(|cd| cd.0 == face) else { return true };
            if let Some(Some(m)) = edges.get(k) {
                mids.push(*m);
                pick.get_or_insert(*cd);
            }
            false
        });
        groups.push((pick, mids));
    }
    for (k, m) in edges.iter().enumerate() {
        if let Some(m) = m
            && cands.get(k).is_none_or(Option::is_none)
        {
            loose.push(*m);
        }
    }
    let deg = round_to(angle * 180.0 / std::f64::consts::PI, 9);
    let mut outs = Vec::new();
    for (pick, mids) in groups.into_iter().chain(std::iter::once((None, loose))) {
        if mids.is_empty() {
            continue;
        }
        let nm = if outs.is_empty() { name.to_string() } else { format!("{name}_{}", outs.len() + 1) };
        let mut js = Map::new();
        js.insert("type".into(), json!("chamfer"));
        js.insert("name".into(), json!(nm));
        js.insert("distance".into(), json!(mm(d1)));
        js.insert("angle".into(), json!(deg));
        js.insert("edges".into(), json!(mids.iter().map(|m| json!({"midpoint": m})).collect::<Vec<_>>()));
        if let Some((_, dir, len)) = pick
            && let Some(m) = mids.first()
        {
            // A point on the face, off the edge (which borders two faces).
            let t = (0.3 * len).min(5.0);
            let q = [m[0] + dir[0] * t, m[1] + dir[1] * t, m[2] + dir[2] * t].map(|v| round_to(v, 6));
            js.insert("face".into(), json!({"point": q}));
        }
        outs.push(Value::Object(js));
    }
    (!outs.is_empty()).then_some(outs)
}

/// What `chamfer_angle` reads from the features before it.
struct Corners<'a> {
    swept: &'a HashMap<u32, Swept>,
    sketches: &'a [Geometry],
}

pub(crate) fn convert_in(rf: &[u8], c0: &[u8], file: &str, chunk: bool) -> Result<Value, String> {
    let uc = if chunk { unit_component_tag(rf) } else { unit_component_index(c0) };
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
    // Cap classes are learned per file from every edge record in the stream.
    let cap_tags = cap_tags_of(&edge_records(rf, 0, rf.len())?);
    let mut work = 0u64;

    let mut feats: Vec<Value> = Vec::new();
    let mut sketches: Vec<Geometry> = Vec::new();
    // The written plane of each sketch (`sketches` indexes it too).
    let mut planes: Vec<Value> = Vec::new();
    // name -> (index into `sketches`, written plane)
    let mut sketch_by_name: HashMap<String, (usize, JsonPlane)> = HashMap::new();
    let mut last_sketch: Option<String> = None;
    let mut solid = false;
    let mut first_solid: Option<String> = None;
    let mut swept: HashMap<u32, Swept> = HashMap::new();
    let mut hole: Option<HoleState> = None;
    for (ni, n) in nodes.iter().enumerate() {
        let mut cls = n.cls.clone();
        let (a, b) = span(n);
        if hole.as_ref().is_some_and(|h| cls.as_deref() != Some("moProfileFeature_c") || h.sketches >= 2)
            && let Some(h) = hole.take()
        {
            let at = h.at;
            if let Some(slot) = feats.get_mut(at) {
                *slot = finish_hole(h);
            }
        }
        if cls.as_deref() == Some("moHoleWzd_c") {
            let mut j = ni + 1;
            while nodes.get(j).is_some_and(|m| m.cls.as_deref() == Some("moProfileFeature_c")) {
                j += 1;
            }
            let hb = nodes.get(j).map_or(rf.len(), |m| m.start);
            hole = Some(HoleState {
                at: feats.len(),
                name: n.name.clone(),
                sketches: 0,
                params: named_params(rf, a, hb, nt)?,
                positions: Vec::new(),
                face: None,
                target: if solid { first_solid.clone() } else { None },
            });
            feats.push(Value::Null);
            continue;
        }
        if cls.as_deref() == Some("moProfileFeature_c") {
            let h = [find(rf, &ENT_HDR, n.post, b), find(rf, &ENT_HDR3, n.post, b)]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(b);
            let Some(geo) = h.checked_sub(14).and_then(|body| parse_sketch(rf, body, b)) else {
                last_sketch = None;
                if let Some(h) = hole.as_mut() {
                    h.sketches += 1;
                }
                continue;
            };
            let mut plane = find_plane(rf, a, b, uc)?;
            if plane.is_none()
                // The first sketch of a file: its plane object is written just before the node.
                && let Some(j) = rfind(rf, b"moCompRefPlane_c", a.saturating_sub(400), a)
            {
                plane = find_plane(rf, j.saturating_sub(6), b, uc)?;
            }
            let jp = json_plane(plane.as_ref());
            let sk = sketch_json(&n.name, &geo, plane.as_ref(), &jp);
            if let Some(h) = hole.as_mut() {
                h.sketches += 1;
                // The placement sketch holds the hole centres as stand-alone sketch points;
                // the other one is the hole's section.
                let centres: Vec<Pt> = geo
                    .pts
                    .iter()
                    .flatten()
                    .filter(|p| p.alone == 1 && p.id != 0xffff_ffff && !p.ext)
                    .copied()
                    .collect();
                if !centres.is_empty() && h.positions.is_empty() && plane.is_some() {
                    let mut seen: Vec<V3> = Vec::new();
                    for q in centres {
                        let at = |i: usize| -> f64 {
                            let (Some(o), Some(x), Some(y)) = (jp.origin.get(i), jp.x.get(i), jp.y.get(i)) else {
                                return 0.0;
                            };
                            round_to(o + q.x * 1000.0 * x + q.y * 1000.0 * y, 6)
                        };
                        let p = [at(0), at(1), at(2)];
                        if seen.iter().all(|o| p.iter().zip(o).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max) > 1e-6) {
                            seen.push(p);
                        }
                    }
                    let nrm = cross(jp.x, jp.y).map(|v| round_to(v, 12) + 0.0);
                    h.face = seen.first().map(|p| (*p, nrm));
                    h.positions = seen;
                }
                planes.push(sk.get("plane").cloned().unwrap_or(Value::Null));
                feats.push(sk);
                sketches.push(geo);
                sketch_by_name.insert(n.name.clone(), (sketches.len() - 1, jp));
                continue;
            }
            planes.push(sk.get("plane").cloned().unwrap_or(Value::Null));
            feats.push(sk);
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
            } else if cls.as_deref() == Some(cut_class)
                || n.flags & 0xf_ffff == CUT_FLAGS
                || n.name.to_lowercase().starts_with("cut")
            {
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
                swept.insert(n.id, Swept { sketch: i, plane, distance: dist, symmetric, through: through_all });
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
            js.insert("angle".into(), json!(ang.map_or(360.0, |v| round_to(v.to_degrees(), 9))));
            if op != "new" {
                js.insert("target".into(), json!(first_solid));
            }
            if let Some(&(i, _)) = last_sketch.as_ref().and_then(|s| sketch_by_name.get(s))
                && let Some(g) = sketches.get(i)
            {
                let line = |s: &Seg| s.kind == SegKind::Line;
                let first = g
                    .segs
                    .iter()
                    .position(|s| line(s) && s.centerline && s.id != 0xffff_ffff)
                    .or_else(|| g.segs.iter().position(|s| line(s) && s.construction));
                // A revolve axis has the whole profile on one side. When the first centerline
                // cuts the profile (a sketch with several centerlines), take the first line that
                // does not: a centerline, a construction line, or a profile edge on the sketch's
                // own x or y axis (the revolve's axis is then a profile line).
                let one_sided = |k: usize| -> bool {
                    let Some(s) = g.segs.get(k) else { return false };
                    let (Some(a), Some(b)) = (point(&g.pts, s.s), point(&g.pts, s.e)) else { return false };
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    if libm::hypot(dx, dy) < 1e-9 {
                        return false;
                    }
                    let side = |q: &Pt| dx * (q.y - a.y) - dy * (q.x - a.x);
                    let (mut pos, mut neg) = (false, false);
                    for o in g.segs.iter().filter(|o| !o.construction && !o.centerline) {
                        for i in [o.s, o.e] {
                            let Some(q) = point(&g.pts, i) else { continue };
                            let v = side(&q) / libm::hypot(dx, dy);
                            pos |= v > 1e-9;
                            neg |= v < -1e-9;
                        }
                    }
                    !(pos && neg)
                };
                let on_sketch_axis = |s: &Seg| {
                    let (Some(a), Some(b)) = (point(&g.pts, s.s), point(&g.pts, s.e)) else { return false };
                    (a.x.abs() < 1e-9 && b.x.abs() < 1e-9) || (a.y.abs() < 1e-9 && b.y.abs() < 1e-9)
                };
                let axis = if first.is_some_and(one_sided) {
                    first
                } else {
                    (0..g.segs.len())
                        .find(|&k| {
                            g.segs.get(k).is_some_and(|s| {
                                line(s) && s.id != 0xffff_ffff && (s.centerline || s.construction) && one_sided(k)
                            })
                        })
                        .or_else(|| {
                            (0..g.segs.len())
                                .find(|&k| g.segs.get(k).is_some_and(|s| line(s) && on_sketch_axis(s)) && one_sided(k))
                        })
                        .or(first)
                };
                js.insert("axis".into(), axis.map_or(Value::Null, |k| json!({"line": format!("c{k}")})));
            }
            feats.push(Value::Object(js));
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if matches!(cls.as_deref(), Some("moMirrorPattern_c" | "moMirrorSolid_c")) {
            // The bodies to mirror are back-references into Config-0, i.e. the whole solid
            // body; the plane is the mirror plane's own frame:
            // `<4 f64 display extents> 01 <R 3x3> <t> 1.0` (confirmed by the box of
            // FrameBracket's Mirror1: the Right plane, x = 0). The face list in the same body
            // (the faces it was picked by) is only the fallback.
            let mut plane: Option<(V3, V3)> = None;
            for i in a..b.saturating_sub(129) {
                if rf.get(i) != Some(&1) {
                    continue;
                }
                let (Some(r), Some(t), Some(sc)) =
                    (f64s_at::<9>(rf, i + 1), f64s_at::<3>(rf, i + 73), f64_at(rf, i + 97))
                else {
                    continue;
                };
                if !orthonormal(&r) || !is(sc, 1.0) {
                    continue;
                }
                let fr = frame(&r, &t, None);
                let z = [r[2], r[5], r[8]];
                plane = Some((fr.origin.map(|v| round_to(v * 1000.0, 6) + 0.0), z.map(|v| round_to(v, 12) + 0.0)));
                break;
            }
            // The mirror face: the first reference of the last face list of the span (the
            // `moSingleFaceRef_w` object; the list before it is another selection).
            if plane.is_none()
                && let Some((_, reps)) = edge_records(rf, a, b)?.last()
                && let Some(r0) = reps.first()
                && let Some(f) = swept.get(&r0.fid)
            {
                if cap_tags.contains(&r0.tag) {
                    plane = Some((lift(f, 0.0, 0.0, cap_height(f, r0.idx)), normal(f)));
                } else if let Some(g) = sketches.get(f.sketch) {
                    // A side face of the extrusion: the plane through its sketch line, along
                    // the extrusion normal.
                    let mut lines =
                        g.segs.iter().filter(|q| q.a == r0.idx && q.kind == SegKind::Line && !q.construction);
                    if let (Some(sg), None) = (lines.next(), lines.next())
                        && let (Some(p0), Some(p1)) = (point(&g.pts, sg.s), point(&g.pts, sg.e))
                    {
                        let o3 = lift(f, p0.x * 1000.0, p0.y * 1000.0, 0.0);
                        let q3 = lift(f, p1.x * 1000.0, p1.y * 1000.0, 0.0);
                        let c = cross(sub(q3, o3), normal(f));
                        let len = libm::sqrt(dot(c, c));
                        if len > 1e-9 {
                            plane = Some((o3, c.map(|v| round_to(v / len, 12) + 0.0)));
                        }
                    }
                }
            }
            let mut js = Map::new();
            js.insert("type".into(), json!("mirror"));
            js.insert("name".into(), json!(n.name));
            js.insert("features".into(), json!([]));
            js.insert("target".into(), json!(first_solid));
            js.insert("merge".into(), json!(true));
            match plane {
                Some((origin, nrm)) => {
                    js.insert("plane".into(), json!({"origin": json_v3(origin), "normal": json_v3(nrm)}));
                }
                None => {
                    js.insert("unresolved".into(), json!(true));
                }
            }
            feats.push(Value::Object(js));
        } else if matches!(cls.as_deref(), Some("Fillet_c" | "Chamfer_c")) {
            let fillet = cls.as_deref() == Some("Fillet_c");
            let ps = params(rf, a, b, nt)?;
            let sel = selection(edge_records(rf, a, b)?);
            let mut edges = Vec::new();
            let mut mids: Vec<Option<V3>> = Vec::new();
            let mut stale = 0usize;
            let mut unresolved = false;
            for (_, reps) in &sel {
                let m = midpoint(reps, &swept, &sketches, &cap_tags, &mut work)?;
                mids.push(m);
                match m {
                    Some(m) => edges.push(json!({"midpoint": m})),
                    // References to faces the sketch no longer has are superseded selections.
                    None if reps.iter().all(|r| swept.contains_key(&r.fid)) => stale += 1,
                    None => unresolved = true,
                }
            }
            unresolved |= edges.is_empty();
            // A distance-angle chamfer; 45 degrees is an equal one, 90 has no tangent.
            if !fillet
                && !unresolved
                && let (Some(&d1), Some(&angle)) = (ps.first(), ps.get(1))
                && angle > 0.1
                && (angle - TAU / 8.0).abs() > 1e-6
                && angle < TAU / 4.0 - 1e-6
                && let Some(chs) =
                    chamfer_angle(&n.name, (d1, angle), &mids, &sel, &Corners { swept: &swept, sketches: &sketches })
            {
                feats.extend(chs);
                continue;
            }
            let mut js = Map::new();
            js.insert("type".into(), json!(if fillet { "fillet" } else { "chamfer" }));
            js.insert("name".into(), json!(n.name));
            js.insert(if fillet { "radius" } else { "distance" }.into(), json!(ps.first().map(|v| mm(*v))));
            js.insert("edges".into(), json!(edges));
            if stale > 0 {
                js.insert("stale_edges".into(), json!(stale));
            }
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
    if let Some(h) = hole.take() {
        let at = h.at;
        if let Some(slot) = feats.get_mut(at) {
            *slot = finish_hole(h);
        }
    }
    Ok(json!({
        "format": "gusset-feature-tree",
        "version": 2,
        "source": {"app": "solidworks", "file": file},
        "features": expand_holes(feats),
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
            "hole" | "mirror" if f.get("unresolved").is_some() => {
                return Err(format!("{name}: the {} was not resolved", s("type")));
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
