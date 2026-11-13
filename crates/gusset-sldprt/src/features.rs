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

mod patterns;

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
const SKIPPED: [&str; 36] = [
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
    // Geometry-free nodes of the newer parts (sw-more corpus): markups, comments, attributes,
    // annotation alignment groups, sensors and the centre of mass, folders (cut lists, details,
    // graphics bodies) and reference geometry other than planes (points, axes, coordinate
    // systems; a feature that relies on one fails the rebuild check instead).
    "moInkMarkupFolder_c",
    "moConfigCommentsFolder_c",
    "moAttribute_c",
    "moAlignGroup_c",
    "moSensor_c",
    "moCenterOfMass_c",
    "moCutListFolder_c",
    "moDetailFolder_c",
    "moMeshBodyFeatureFolder_c",
    "moRefPoint_c",
    "moRefAxis_c",
    "moCoordSys_c",
    "moFeatGraphicsBodyFolder_c",
];

/// The 46-byte block after a feature node's comment (older files, then newer ones).
const NODE_BLOCK: [u8; 18] = [0, 0xff, 0xff, 0, 0, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf];
const NODE_BLOCK3: [u8; 18] =
    [0, 0xff, 0xff, 0, 0, 3, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf];
/// The schema word of a sketch entity header (`FF FF <word>`): 0x1f, or 0x07 in parts written by
/// older modellers (setScrew, Spring), which some newer parts keep for library sketches.
const ENT_SCHEMAS: [u8; 2] = [0x1f, 0x07];
/// A sketch entity header with all-`FF` middle bytes (versions 1 and 3), for each schema word.
const fn ent_hdr(schema: u8, version: u8) -> [u8; 17] {
    [0xff, 0xff, schema, 0, version, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0x80, 0xbf]
}
const ENT_HDRS: [[u8; 17]; 4] = [ent_hdr(0x1f, 1), ent_hdr(0x1f, 3), ent_hdr(0x07, 1), ent_hdr(0x07, 3)];
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
/// (`py/uc.py`): the most frequent `XX YY 02 00 <10 x 00>` with the tag bit 0x80 set in `YY` and
/// a reference-plane frame after it ([`comp_frame`]), the first seen winning ties. The numbering of these files does not follow `Config-0`; in
/// files with more than 255 classes before it the index takes the high byte too (`24 81`).
pub(crate) fn unit_component_tag(rf: &[u8]) -> Option<usize> {
    let mut counts: Vec<(u16, usize)> = Vec::new();
    let mut i = 0usize;
    while let Some(w) = rf.get(i..i + 14) {
        if w.get(1).is_some_and(|&h| h & 0x80 != 0 && h != 0xff)
            && w.get(2..4) == Some(&[2, 0][..])
            && w.get(4..).is_some_and(|z| z.iter().all(|&c| c == 0))
            // Only tags whose object carries a component frame vote: a stray `XX YY 02 00` run
            // of zeros (V-Slot Spacer Block's `ad 94`) outnumbered the real tag otherwise.
            && comp_frame(rf, i + 2).is_some()
        {
            let key = u16::from_le_bytes([w.first().copied().unwrap_or(0), w.get(1).copied().unwrap_or(0x80)]) & 0x7fff;
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

/// The sketch objects (`sgSketch`) a configuration stream keeps, as `(body, end)` for
/// [`parse_sketch`]: some parts keep the entities of sketches driven by equations or
/// configurations there, and the feature stream's sketch node holds none (NEMA23_mount,
/// Pan-Slot Screw M4). The first object follows the class declaration, the others its class tag;
/// each is `tag, u32 point count, u16, u32 key, u32 id` and then its first entity header.
fn config_sketches(c: &[u8]) -> Vec<(usize, usize)> {
    let Some(decl) = find(c, b"\xff\xff\x01\x00\x08\x00sgSketch", 0, c.len()) else { return Vec::new() };
    let first = decl + 14;
    let Some(hdrs) = find_headers(c, first, c.len()) else { return Vec::new() };
    if hdrs.first() != Some(&(first + 14)) {
        return Vec::new();
    }
    // The class tag: the most frequent high-bit word two bytes before a point count that is
    // followed by a first entity header (key 0).
    let starts = |h: usize| -> Option<u16> {
        let t = u16_at(c, h.checked_sub(16)?)?;
        let n = u32_at(c, h - 14)?;
        (t & 0x8000 != 0
            && t != 0xffff
            && n < 100_000
            && c.get(h - 10..h - 4).is_some_and(|w| w.iter().all(|&b| b == 0)))
        .then_some(t)
    };
    let mut counts: Vec<(u16, usize)> = Vec::new();
    for &h in hdrs.iter().skip(1) {
        if let Some(t) = starts(h) {
            match counts.iter_mut().find(|(k, _)| *k == t) {
                Some((_, v)) => *v += 1,
                None => counts.push((t, 1)),
            }
        }
    }
    let tag = counts.iter().max_by_key(|(_, v)| *v).map(|(k, _)| *k);
    let mut bodies = vec![first];
    if let Some(tag) = tag {
        bodies.extend(hdrs.iter().skip(1).filter(|&&h| starts(h) == Some(tag)).map(|&h| h - 14));
    }
    let ends: Vec<usize> = bodies.iter().skip(1).map(|&b| b - 2).chain([c.len()]).collect();
    bodies.into_iter().zip(ends).collect()
}

/// Drops node names written inside the header of the node before them (both find the same
/// block): an attribute in a sketch's header (zsCAD's `Assoc1`), or the `Sketch3<3>` name a
/// feature that reuses Sketch3 writes in its own header (Connecting Plate_BOTTOM_M3's
/// Boss-Extrude2, V-Slot Rail_PIVOT's Boss-Extrude3: the feature's end condition follows, and the
/// name had cut the feature's span short). Other nested names (a hole wizard's sketches) stay.
fn drop_nested(nodes: &mut Vec<Node>) {
    let reused = |n: &str| {
        n.strip_suffix('>')
            .and_then(|r| r.rsplit_once('<'))
            .is_some_and(|(base, k)| !base.is_empty() && !k.is_empty() && k.bytes().all(|c| c.is_ascii_digit()))
    };
    let mut prev: Option<(usize, usize)> = None;
    nodes.retain(|n| {
        let nested = prev.is_some_and(|(post, start)| post == n.post && start < n.start);
        let drop = nested
            && (n.cls.as_deref() == Some("moAttribute_c")
                || (n.cls.as_deref() == Some("moProfileFeature_c") && reused(&n.name)));
        if !drop {
            prev = Some((n.post, n.start));
        }
        !drop
    });
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
    param_slots(d, a, b, nt)?.into_iter().map(|(name, at)| Ok((name, need(f64_at(d, at), "a parameter")?))).collect()
}

/// Each parameter's name and the offset of its value: an `f64`, or a `u32` for the integer
/// parameters (`moIntegerParameter_c`: a pattern's instance counts).
fn param_slots(d: &[u8], a: usize, b: usize, nt: [u8; 2]) -> Result<Vec<(String, usize)>, String> {
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
        need(f64_at(d, end), "a parameter")?;
        out.push((name, end));
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
    /// Byte 12 of the head (`u32` 0/1).
    b12: u8,
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
            // Byte 12 (a `u32` 0/1, set on some cuts of the newer parts: Lock Collar
            // Cut-Extrude2) is not read; the written direction vector says the direction.
            && w.iter().enumerate().skip(5).all(|(k, &c)| c == 0 || (k == 12 && c == 1));
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
        let b12 = *need(d.get(p + 12), "an end condition")?;
        out.push(EndSpec { cls: Some(t & 0x7fff), x, kind, flag, b12 });
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
    let b12 = *need(d.get(p + 12), "an end condition")?;
    Ok(Some(EndSpec { cls: None, x, kind, flag, b12 }))
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
    /// An elliptical arc: its axis end points are in [`Geometry::ellipse_axes`].
    Ellipse,
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
    /// References outside geometry (a projected model edge).
    external: bool,
}

#[derive(Debug, Clone, Default)]
struct Geometry {
    pts: Vec<Option<Pt>>,
    segs: Vec<Seg>,
    /// Per elliptical segment (its index in `segs`), the point indices of its axis ends:
    /// major +, minor +, major -, minor -.
    ellipse_axes: Vec<(usize, [usize; 4])>,
}

/// Entity headers in `[a, b)`: `FF FF 1F 00 <1|3> <8 bytes> 00 00 80 BF`, non-overlapping.
fn find_headers(d: &[u8], a: usize, b: usize) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    let mut i = a;
    let end = b.min(d.len());
    while i + 17 <= end {
        let w = d.get(i..i + 17)?;
        if w.get(..2) == Some(&[0xff, 0xff][..])
            && w.get(2).is_some_and(|s| ENT_SCHEMAS.contains(s))
            && w.get(3) == Some(&0)
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
    // Version 3 headers of the newer parts put 8 zero bytes before the payload; some
    // (adpator, a chunk-container part of an intermediate version) do not. A payload never
    // starts with 8 zero bytes (a point starts with its type 0x1e/0x1f, a segment with two
    // different point indices), so the bytes say which.
    let v3 = d.get(h + 4) == Some(&3);
    let after = |q: usize| if v3 && d.get(q..q + 8).is_some_and(|w| w.iter().all(|&b| b == 0)) { q + 8 } else { q };
    let ext = u16_at(d, h + 35)?;
    if ext == 0 {
        let p = h + 40;
        if d.get(p..p + 16) == Some(&ONE[..]) {
            return Some((Some(after(p + 16)), false));
        }
    }
    // An object chain in the reference slot: the payload follows the next 0.0/1.0 pair.
    Some((find(d, &ONE, h + 35, h + 2000).map(|k| after(k + 16)), true))
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
            // An ellipse (arc) lists the four axis end points after the arc fields where an arc
            // writes four -2 words (Y-Plate's notch: a circle-shaped ellipse arc, `dir` 1 =
            // counter-clockwise from start to end as for arcs).
            let axes = [22usize, 24, 26, 28].map(|o| u16_at(d, p + o).map(usize::from));
            if let [Some(a0), Some(a1), Some(a2), Some(a3)] = axes
                && [a0, a1, a2, a3].iter().all(|&q| q < npts && q != s && q != e)
                && a0 != a2
                && a1 != a3
                && d.get(p + 30..p + 38).is_some_and(|w| w.iter().all(|&b| b == 0))
                && one == 1
            {
                let dir = i32_at(d, p + 16)?;
                g.ellipse_axes.push((g.segs.len(), [a0, a1, a2, a3]));
                g.segs.push(Seg {
                    kind: SegKind::Ellipse,
                    id: rid,
                    a: aval,
                    s,
                    e,
                    c: 0,
                    dir,
                    construction,
                    centerline: false,
                    external: ext,
                });
            } else if d.get(p + 22..p + 38).is_some_and(|w| w.chunks(4).all(|c| c == [0xfe, 0xff, 0xff, 0xff])) {
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
                    external: ext,
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
                    external: ext,
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

/// The frame of a reference-plane component whose unit-component tag ends at `i0 - 2` (`i0` is
/// two bytes into the tag). The component base is 31 bytes after the unit component in older
/// files, 55 in chunk-container ones and 67 or 69 in later versions (more zero bytes, or a word,
/// after the run of 16 FF bytes; sw-more): the id (`u32 id, u32 stamp, u16 0`) is tried at each
/// place up to 112 bytes on, the first that holds a whole frame.
fn comp_frame(d: &[u8], i0: usize) -> Option<Plane> {
    for off in 43usize..=112 {
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
            return Some(frame(&r, &t, Some(cid)));
        }
    }
    None
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
        if let Some(p) = comp_frame(d, i0) {
            offer(i0 - 2, p, None);
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
        if s.kind == SegKind::Ellipse {
            // The centre is halfway between the major axis ends; it is written as a point of
            // its own (`pe<k>`).
            let Some(&(_, [a0, a1, a2, _])) = g.ellipse_axes.iter().find(|(i, _)| *i == k) else { continue };
            let (Some(q0), Some(q1), Some(q2)) = (pt(a0), pt(a1), pt(a2)) else { continue };
            let (cx, cy) = (f64::midpoint(q0.x, q2.x), f64::midpoint(q0.y, q2.y));
            let (ra, rb) = (libm::hypot(q0.x - cx, q0.y - cy), libm::hypot(q1.x - cx, q1.y - cy));
            if ra < 1e-12 || rb < 1e-12 {
                continue;
            }
            let id = format!("pe{k}");
            points.push(json!({"id": id, "x": mm(cx), "y": mm(cy)}));
            let (a, b) = if s.dir > 0 { (s.s, s.e) } else { (s.e, s.s) };
            o.insert("type".into(), json!(if s.s == s.e { "ellipse" } else { "ellipse_arc" }));
            o.insert("center".into(), json!(id));
            o.insert(
                "major".into(),
                json!([round_to((q0.x - cx) / ra, 12) + 0.0, round_to((q0.y - cy) / ra, 12) + 0.0]),
            );
            o.insert("a".into(), json!(mm(ra)));
            o.insert("b".into(), json!(mm(rb)));
            if s.s != s.e {
                o.insert("start".into(), json!(pid(a)));
                o.insert("end".into(), json!(pid(b)));
            }
            if s.construction {
                o.insert("construction".into(), json!(true));
            }
            curves.push(Value::Object(o));
            continue;
        }
        match s.kind {
            SegKind::Line => {
                o.insert("type".into(), json!("line"));
                o.insert("start".into(), json!(pid(s.s)));
                o.insert("end".into(), json!(pid(s.e)));
            }
            SegKind::Arc if s.s == s.e => {
                // A circle through its own centre bounds nothing (mid_elbow's Sketch9 has one about
                // a projected point, beside the 1.5 mm hole there); Gusset refuses it.
                if libm::hypot(ps.x - pc.x, ps.y - pc.y) < 1e-12 {
                    continue;
                }
                o.insert("type".into(), json!("circle"));
                o.insert("center".into(), json!(pid(s.c)));
                o.insert("radius".into(), json!(mm(libm::hypot(ps.x - pc.x, ps.y - pc.y))));
            }
            SegKind::Arc | SegKind::Ellipse => {
                // A construction arc whose ends are not both on its circle (or on its centre) is
                // not an arc as read (Hobby Motor 3v's sketch: centres on end points); it shapes
                // nothing, and Gusset refuses it.
                let pe = pt(s.e).unwrap_or(ps);
                let (r0, r1) = (libm::hypot(ps.x - pc.x, ps.y - pc.y), libm::hypot(pe.x - pc.x, pe.y - pc.y));
                if s.construction && (r0 < 1e-9 || r1 < 1e-9 || (r0 - r1).abs() > 1e-6 * r0.max(r1)) {
                    continue;
                }
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
    /// A face of a mirror's copy: the mirror's feature id (the reference names the seed face).
    via: Option<u32>,
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
    Ok(edge_records_to(d, a, b, &HashSet::new())?.into_iter().map(|(s, _, r)| (s, r)).collect())
}

/// [`edge_records`] with the end of each record (after its last reference). A face of a
/// mirror's copy is written as a 16-byte reference to the mirror (`<tag> 00 00 <class> u16
/// <u32 mirror id> <u32 stamp>`, no face number) followed by the seed face's own reference
/// (the bearings' Chamfer1: Mirror1, then Cut-Extrude2 face 3); `mirrors` are the ids read so.
fn edge_records_to(
    d: &[u8],
    a: usize,
    b: usize,
    mirrors: &HashSet<u32>,
) -> Result<Vec<(usize, usize, Vec<Rep>)>, String> {
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
            let mut via = None;
            let outer = need(u32_at(d, p + 8), "an edge record")?;
            if mirrors.contains(&outer)
                && u16_at(d, p).is_some_and(|t| t & 0x8000 != 0)
                && u16_at(d, p + 2) == Some(0)
                && u16_at(d, p + 16).is_some_and(|t| t & 0x8000 != 0)
                && u16_at(d, p + 18) == Some(0)
                && p + 36 <= d.len()
            {
                via = Some(outer);
                p += 16;
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
            reps.push(Rep { tag: t & 0x7fff, fid, idx, ext: extra == 1 && !last, ext2, via });
        }
        // Face lists (mirror planes) hold n - 1 references.
        if reps.len() + 1 == n {
            // The last reference of a face list has no `last` treatment: its extras are not a cap's.
            for r in &mut reps {
                r.ext2 = false;
            }
        }
        if reps.len() == n || (reps.len() + 1 == n && !reps.is_empty()) {
            out.push((start, p, reps));
            if out.len() > MAX_MATCHES {
                return Err("too many edge records".into());
            }
        }
    }
    Ok(out)
}

/// The edges the user picked: the first run of records one record apart.
/// A record also continues the run when it starts right after the previous one ends (its class
/// tag and eight zero bytes between them): records of four references with their extra words
/// are longer than 0xa0 bytes (the bearings' Chamfer1 picks eight edges 150 to 200 bytes apart).
fn selection(recs: Vec<(usize, usize, Vec<Rep>)>) -> Vec<(usize, Vec<Rep>)> {
    let mut run: Vec<(usize, Vec<Rep>)> = Vec::new();
    let mut last_end = 0usize;
    for (start, end, reps) in recs {
        match run.last() {
            Some(last) if start - last.0 > 0xa0 && start.saturating_sub(last_end) > 16 => break,
            _ => {
                run.push((start, reps));
                last_end = end;
            }
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

/// The extrusion direction an extrude node writes: a unit vector one byte after the marker
/// `0005 0002 0002 0002`. An extrusion along z (a sketch on a Front-like plane) is written as
/// the y component, negated; the other axes are written as they are.
fn extrude_direction(d: &[u8], a: usize, b: usize) -> Option<V3> {
    const MARK: [u8; 8] = [0, 5, 0, 2, 0, 2, 0, 2];
    let i = find(d, &MARK, a, b)?;
    let v = f64s_at::<3>(d, i + MARK.len() + 1)?;
    (v.iter().all(|c| c.is_finite()) && (dot(v, v) - 1.0).abs() < 1e-6).then_some(v)
}

/// Whether the signed depth `dist` along the sketch normal `n` runs the way the written
/// direction `v` says; `None` when the sketch is not axis aligned or the vector says nothing
/// about the axis.
fn direction_agrees(n: V3, dist: f64, v: V3) -> Option<bool> {
    let axis = (0..3).find(|&k| n.get(k).is_some_and(|c| c.abs() > 1.0 - 1e-6))?;
    let written = if axis == 2 { -v.get(1)? } else { *v.get(axis)? };
    if written.abs() < 1.0 - 1e-6 {
        return None;
    }
    Some(written * n.get(axis)? * dist > 0.0)
}

/// The line a feature names by a line reference (`moLineRef_w`: a reference axis, a sketch
/// line or an edge): `c7 cf ff ff c7 cf ff ff 00 00 00 00 70 17 00 00`, 8 or 16 zero bytes, then
/// `f64 length`, the start point and the unit direction in model coordinates (m). Returns the
/// start in mm and the direction. Dual V-Wheel's Revolve1 turns about Axis1, the z axis.
fn line_ref(d: &[u8], a: usize, b: usize) -> Option<(V3, V3)> {
    const MARK: [u8; 16] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0x70, 0x17, 0, 0];
    let mut from = a;
    while let Some(i) = find(d, &MARK, from, b) {
        from = i + 1;
        // Eight zero bytes before the length, sixteen in newer parts (Solid V-Wheel Xtreme).
        for at in [i + 24, i + 32] {
            let (Some(len), Some(o), Some(dir)) = (f64_at(d, at), f64s_at::<3>(d, at + 8), f64s_at::<3>(d, at + 32))
            else {
                continue;
            };
            if len.is_finite()
                && len > 0.0
                && o.iter().all(|c| c.is_finite() && c.abs() < 100.0)
                && (dot(dir, dir) - 1.0).abs() < 1e-9
            {
                return Some((o.map(mm), dir.map(|c| round_to(c, 12) + 0.0)));
            }
        }
    }
    None
}

/// The point a feature names by a point reference (`moEndPointRef_w`, an end of an edge):
/// `c7 cf ff ff c7 cf ff ff 00 00 00 00 f8 2a 00 00`, eight zero bytes, then the point in model
/// coordinates (m). Returns it in mm.
fn point_ref(d: &[u8], a: usize, b: usize) -> Option<V3> {
    const MARK: [u8; 16] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0xf8, 0x2a, 0, 0];
    let i = find(d, &MARK, a, b)?;
    let p = f64s_at::<3>(d, i + 24)?;
    p.iter().all(|c| c.is_finite() && c.abs() < 100.0).then(|| p.map(mm))
}

/// A full revolve an edge can name: the sketch (and its plane) and two points of the axis in
/// sketch coordinates (m).
#[derive(Debug, Clone, Copy)]
struct Revolved {
    on: Swept,
    axis: [Pt; 2],
}

/// Chamfers whose bevel stands square on the reference face (a 90-degree distance-angle
/// chamfer), one per edge, for edges that are circles of a full revolve between two profile
/// lines: `distance` d on the reference face (the plane or cylinder of the two, the other being
/// a cone; one sample, adpator) and `distance2` d / cos(corner angle) on the
/// other, which the square bevel meets only at an acute corner. `None` when an edge is not such
/// a circle.
fn square_chamfer(
    name: &str,
    d: f64,
    sel: &[(usize, Vec<Rep>)],
    revolved: &HashMap<u32, Revolved>,
    sketches: &[Geometry],
) -> Option<Vec<Value>> {
    let mut out = Vec::new();
    for (_, reps) in sel {
        let (m, path) = revolve_circle(reps, revolved, sketches)?;
        let [r0, r1, ..] = reps.as_slice() else { return None };
        let rv = revolved.get(&r0.fid)?;
        let g = sketches.get(rv.on.sketch)?;
        let line = |k: u32| {
            let mut it = g.segs.iter().filter(|s| s.a == k && !s.construction && !s.centerline);
            match (it.next(), it.next()) {
                (Some(s), None) if s.kind == SegKind::Line => Some(*s),
                _ => None,
            }
        };
        let (s0, s1) = (line(r0.idx)?, line(r1.idx)?);
        let shared = [s0.s, s0.e].into_iter().find(|v| *v == s1.s || *v == s1.e)?;
        let v = point(&g.pts, shared)?;
        let [a0, a1] = rv.axis;
        let al = libm::hypot(a1.x - a0.x, a1.y - a0.y);
        if al < 1e-12 {
            return None;
        }
        let (tx, ty) = ((a1.x - a0.x) / al, (a1.y - a0.y) / al);
        // The reference face is the plane or cylinder of the two (a profile line square to or
        // along the axis), the other a cone; any other pair is not read.
        let simple = |s: &Seg| -> Option<bool> {
            let (p, q) = (point(&g.pts, s.s)?, point(&g.pts, s.e)?);
            let (dx, dy) = (q.x - p.x, q.y - p.y);
            let len = libm::hypot(dx, dy);
            let c = ((dx * tx + dy * ty) / len.max(1e-300)).abs();
            Some(len > 1e-12 && !(1e-9..=1.0 - 1e-12).contains(&c))
        };
        let (sa, sb) = match (simple(&s0)?, simple(&s1)?) {
            (true, false) => (s0, s1),
            (false, true) => (s1, s0),
            _ => return None,
        };
        // Unit directions in the sketch from the corner along each line.
        let away = |s: &Seg| -> Option<(f64, f64)> {
            let o = point(&g.pts, if s.s == shared { s.e } else { s.s })?;
            let (dx, dy) = (o.x - v.x, o.y - v.y);
            let len = libm::hypot(dx, dy);
            (len > 1e-12).then(|| (dx / len, dy / len))
        };
        let (ua, ub) = (away(&sa)?, away(&sb)?);
        let cos = ua.0 * ub.0 + ua.1 * ub.1;
        // Only an acute corner (not too sharp) has a square bevel that meets the other face.
        if !(0.05..0.999).contains(&cos) {
            return None;
        }
        // A point on the reference face beside the edge, where the edge's midpoint is: half way
        // round from the profile, so the radial part of the direction turns over.
        let along = ua.0 * tx + ua.1 * ty;
        let (rx, ry) = (ua.0 - along * tx, ua.1 - along * ty);
        let flipped = (along * tx - rx, along * ty - ry);
        let origin = lift(&rv.on, 0.0, 0.0, 0.0);
        let dir = sub(lift(&rv.on, flipped.0, flipped.1, 0.0), origin);
        let t = (0.5 * d).min(1.0);
        let q = [0, 1, 2]
            .map(|i| round_to(m.get(i).copied().unwrap_or(0.0) + t * dir.get(i).copied().unwrap_or(0.0), 6) + 0.0);
        let nm = if out.is_empty() { name.to_string() } else { format!("{name}_{}", out.len() + 1) };
        out.push(json!({
            "type": "chamfer",
            "name": nm,
            "distance": round_to(d, 9),
            "distance2": round_to(d / cos, 9),
            "edges": [{"midpoint": m, "path": path}],
            "face": {"point": q},
        }));
    }
    (!out.is_empty()).then_some(out)
}

/// The circle where two profile segments of a full revolve meet, from an edge record whose
/// first two references are the revolve's faces (the index is the key of the profile
/// segment): a point on it, opposite the profile plane, and the whole circle as a closed
/// path. `None` for anything else (a construction segment, segments that do not meet, the
/// axis).
fn revolve_circle(reps: &[Rep], revolved: &HashMap<u32, Revolved>, sketches: &[Geometry]) -> Option<(V3, Vec<V3>)> {
    let [r0, r1, ..] = reps else { return None };
    if r0.fid != r1.fid {
        return None;
    }
    let rv = revolved.get(&r0.fid)?;
    let g = sketches.get(rv.on.sketch)?;
    let seg = |k: u32| {
        let mut it = g.segs.iter().filter(|s| s.a == k && !s.construction && !s.centerline);
        match (it.next(), it.next()) {
            (Some(s), None) => Some(*s),
            _ => None,
        }
    };
    let (sa, sb) = (seg(r0.idx)?, seg(r1.idx)?);
    let mut common: Vec<usize> = [sa.s, sa.e].into_iter().filter(|v| *v == sb.s || *v == sb.e).collect();
    common.dedup();
    let [shared] = common.as_slice() else { return None };
    let v = point(&g.pts, *shared)?;
    let [a0, a1] = rv.axis;
    let (dx, dy) = (a1.x - a0.x, a1.y - a0.y);
    let len = libm::hypot(dx, dy);
    if len < 1e-12 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let t = (v.x - a0.x) * ux + (v.y - a0.y) * uy;
    let foot = (a0.x + t * ux, a0.y + t * uy);
    let c = lift(&rv.on, foot.0 * 1000.0, foot.1 * 1000.0, 0.0);
    let r0v = sub(lift(&rv.on, v.x * 1000.0, v.y * 1000.0, 0.0), c);
    let radius = libm::sqrt(dot(r0v, r0v));
    if radius < 1e-6 {
        return None;
    }
    let along = sub(lift(&rv.on, foot.0 * 1000.0 + ux, foot.1 * 1000.0 + uy, 0.0), c);
    let e2 = cross(along, r0v);
    let steps = 200u32;
    let at = |th: f64| -> V3 {
        let (cs, sn) = (libm::cos(th), libm::sin(th));
        [0, 1, 2].map(|i| {
            let (c, a, b) = (
                c.get(i).copied().unwrap_or(0.0),
                r0v.get(i).copied().unwrap_or(0.0),
                e2.get(i).copied().unwrap_or(0.0),
            );
            round_to(c + cs * a + sn * b, 6) + 0.0
        })
    };
    let path = (0..=steps).map(|k| at(TAU * f64::from(k) / f64::from(steps))).collect();
    Some((at(TAU / 2.0), path))
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
/// `a s + b t`.
fn comb(a: V3, s: f64, b: V3, t: f64) -> V3 {
    [a[0] * s + b[0] * t, a[1] * s + b[1] * t, a[2] * s + b[2] * t]
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

/// The face an up-to-surface or offset-from-surface end (`swEndConditions_e` 4 and 5) names: the
/// first reference of the node's last `moSingleFaceRef_w` object (the object is the last thing
/// the node writes; the `moEndFace3IntSurfIdRep_c` before it holds the extrude's own end face).
/// The object's header ends with a pair of 8-byte words (the same value twice) and `00 00`; the
/// reference follows, as an edge record's reference: class word, feature id, stamp, face number.
/// A cap has the class of the caps, a side the sketch key of its line.
fn end_face_ref(d: &[u8], a: usize, b: usize) -> Option<Rep> {
    const WORD: [u8; 8] = [0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54];
    let mut from = a;
    let mut last = None;
    while let Some(p) = find(d, &WORD, from, b) {
        from = p + 1;
        if d.get(p + 8..p + 16) == Some(&WORD[..]) {
            last = Some(p);
        }
    }
    let c = last? + 18;
    let t = u16_at(d, c)?;
    if t & 0x8000 == 0 || u16_at(d, c + 2)? != 0 {
        return None;
    }
    Some(Rep { tag: t & 0x7fff, fid: u32_at(d, c + 8)?, idx: u32_at(d, c + 16)?, ext: false, ext2: false, via: None })
}

/// Whether `(u, v)` lies inside the profile (even-odd over its drawn segments; m).
fn inside(g: &Geometry, u: f64, v: f64) -> bool {
    let mut odd = false;
    for s in g.segs.iter().filter(|s| !s.construction) {
        let Some(pts) = seg_uv(g, s) else { continue };
        for w in pts.windows(2) {
            let [(x0, y0), (x1, y1)] = w else { continue };
            if (*y0 > v) != (*y1 > v) && u < x0 + (v - y0) / (y1 - y0) * (x1 - x0) {
                odd = !odd;
            }
        }
    }
    odd
}

/// The extent of an end face within its plane.
#[derive(Clone, Copy)]
enum Region {
    /// A cap: the profile of the feature.
    Cap(Swept),
    /// A side: the line from `a` to `b` (m, in the sketch's axes), over the feature's height.
    Wall(Swept, (f64, f64), (f64, f64)),
}

/// Whether the point `q` (mm) of the face's plane lies on the face, up to 0.01 mm.
fn on_face(region: &Region, sketches: &[Geometry], q: V3) -> bool {
    let (Region::Cap(f) | Region::Wall(f, ..)) = region;
    let Some(g) = sketches.get(f.sketch) else { return false };
    let d = sub(q, f.plane.origin);
    let (u, v) = (dot(d, f.plane.x) / 1000.0, dot(d, f.plane.y) / 1000.0);
    match region {
        Region::Cap(_) => {
            inside(g, u, v)
                || g.segs.iter().filter(|s| !s.construction).any(|s| {
                    seg_uv(g, s).is_some_and(|pts| {
                        pts.windows(2).any(|w| match w {
                            [(x0, y0), (x1, y1)] => dist_to_segment((u, v), (*x0, *y0), (*x1, *y1)) < 1e-5,
                            _ => false,
                        })
                    })
                })
        }
        Region::Wall(_, a, b) => {
            let h = dot(d, normal(f));
            let (lo, hi) = if f.symmetric {
                (-f.distance.abs() / 2.0, f.distance.abs() / 2.0)
            } else {
                (f.distance.min(0.0), f.distance.max(0.0))
            };
            h >= lo - 0.01 && h <= hi + 0.01 && dist_to_segment((u, v), *a, *b) < 1e-5
        }
    }
}

fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 < 1e-24 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
    libm::hypot(p.0 - (a.0 + t * dx), p.1 - (a.1 + t * dy))
}

/// The plane of the face a reference names, for a pick: a point on the face (mm), its outward
/// normal and its extent. Only the caps and the straight sides of an extruded feature.
fn face_pick(
    r: &Rep,
    swept: &HashMap<u32, Swept>,
    sketches: &[Geometry],
    cap_tags: &HashSet<u16>,
) -> Option<(V3, V3, Region)> {
    type Side = ((f64, f64), (f64, f64), f64);
    let f = swept.get(&r.fid).filter(|f| !f.through && f.distance.abs() > 1e-9)?;
    let g = sketches.get(f.sketch)?;
    let sweep = if f.distance > 0.0 { 1.0 } else { -1.0 };
    // The middle of a line of the profile (m), its outward direction (unit, in the sketch's axes) and its length.
    let side = |s: &Seg| -> Option<Side> {
        let (p, q) = (point(&g.pts, s.s)?, point(&g.pts, s.e)?);
        let (dx, dy) = (q.x - p.x, q.y - p.y);
        let len = libm::hypot(dx, dy);
        if s.kind != SegKind::Line || len < 1e-9 {
            return None;
        }
        let mid = ((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
        let nrm = (dy / len, -dx / len);
        let e = 1e-6 + 0.01 * len;
        let out = if inside(g, mid.0 + nrm.0 * e, mid.1 + nrm.1 * e) { (-nrm.0, -nrm.1) } else { nrm };
        Some((mid, out, len))
    };
    let unit = |v: V3| -> V3 { v.map(|c| round_to(c, 12) + 0.0) };
    // A class of caps the edge records never showed: face 0 or 1 of a profile with no such side.
    if cap_tags.contains(&r.tag) || (r.idx <= 1 && !g.segs.iter().any(|s| s.a == r.idx && !s.construction)) {
        if r.idx > 1 {
            return None;
        }
        let longest = g
            .segs
            .iter()
            .filter(|s| !s.construction)
            .filter_map(&side)
            .fold(None::<Side>, |b, x| if b.is_none_or(|b| x.2 > b.2) { Some(x) } else { b })?;
        let (mid, out, len) = longest;
        let e = 1e-6 + 0.01 * len;
        let (u, v) = (mid.0 - out.0 * e, mid.1 - out.1 * e);
        if !inside(g, u, v) {
            return None;
        }
        let o = if r.idx == 1 { sweep } else { -sweep };
        return Some((
            lift(f, u * 1000.0, v * 1000.0, cap_height(f, r.idx)),
            unit(normal(f).map(|c| c * o)),
            Region::Cap(*f),
        ));
    }
    let mut c = g.segs.iter().filter(|s| s.a == r.idx && !s.construction);
    let (Some(s), None) = (c.next(), c.next()) else { return None };
    let (mid, out, _) = side(s)?;
    let (pa, pb) = (point(&g.pts, s.s)?, point(&g.pts, s.e)?);
    let (x, y) = (f.plane.x, f.plane.y);
    let nrm = [0, 1, 2].map(|i| out.0 * x.get(i).copied().unwrap_or(0.0) + out.1 * y.get(i).copied().unwrap_or(0.0));
    Some((
        lift(f, mid.0 * 1000.0, mid.1 * 1000.0, f.distance / 2.0),
        unit(nrm),
        Region::Wall(*f, (pa.x, pa.y), (pb.x, pb.y)),
    ))
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
    let Some(uv) = seg_uv(g, s) else { return Ok(None) };
    *work = work.saturating_add(uv.len() as u64);
    if *work > MAX_EDGE_WORK {
        return Err(TOO_MUCH_WORK.into());
    }
    Ok(Some(uv.into_iter().map(|(u, v)| lift(f, u * 1000.0, v * 1000.0, 0.0)).collect()))
}

/// A segment as points of its sketch (m): the two ends of a line, the points of an arc.
fn seg_uv(g: &Geometry, s: &Seg) -> Option<Vec<(f64, f64)>> {
    let (p, q) = (point(&g.pts, s.s)?, point(&g.pts, s.e)?);
    Some(if s.kind == SegKind::Line {
        vec![(p.x, p.y), (q.x, q.y)]
    } else if s.kind == SegKind::Ellipse {
        return None;
    } else {
        let c = point(&g.pts, s.c)?;
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
    })
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

/// A face an edge record names: a cap of a swept feature or one of its sides, or the face a
/// fillet made of a corner of its profile.
#[derive(Clone, Copy)]
enum Face<'a> {
    Cap(u32, &'a Swept, u32),
    Side(u32, &'a Swept, Seg),
    Round(Round),
}

/// The rounded corner of a swept feature's profile that one edge of a fillet made: the two
/// straight sides (their sketch keys) meeting at `corner`, the fillet circle's tangent points
/// on them and the middle of its arc, all in sketch coordinates (m). A later feature names the
/// fillet's face by the fillet's id and the 1-based number of the edge in its selection.
#[derive(Clone, Copy)]
struct Round {
    f: Swept,
    keys: [u32; 2],
    corner: (f64, f64),
    tangent: [(f64, f64); 2],
    arc_mid: (f64, f64),
}

/// The corner a fillet edge record rounds: an edge between two straight sides of one feature
/// (`radius` in m).
fn round_of(
    reps: &[Rep],
    swept: &HashMap<u32, Swept>,
    sketches: &[Geometry],
    cap_tags: &HashSet<u16>,
    radius: f64,
) -> Option<Round> {
    let (ra, rb) = (reps.first()?, reps.get(1)?);
    if ra.fid != rb.fid || cap_tags.contains(&ra.tag) || cap_tags.contains(&rb.tag) {
        return None;
    }
    let f = swept.get(&ra.fid)?;
    let g = sketches.get(f.sketch)?;
    let line = |idx: u32| -> Option<Seg> {
        let mut c = g.segs.iter().filter(|q| q.a == idx && !q.construction);
        match (c.next(), c.next()) {
            (Some(q), None) if q.kind == SegKind::Line => Some(*q),
            _ => None,
        }
    };
    let (sa, sb) = (line(ra.idx)?, line(rb.idx)?);
    let common: Vec<usize> = [sa.s, sa.e].into_iter().filter(|v| *v == sb.s || *v == sb.e).collect();
    let [corner] = common[..] else { return None };
    let v = point(&g.pts, corner)?;
    let away = |q: &Seg| -> Option<(f64, f64)> {
        let o = point(&g.pts, if q.s == corner { q.e } else { q.s })?;
        let l = libm::hypot(o.x - v.x, o.y - v.y);
        (l > 1e-12).then(|| ((o.x - v.x) / l, (o.y - v.y) / l))
    };
    let (u1, u2) = (away(&sa)?, away(&sb)?);
    let sum = (u1.0 + u2.0, u1.1 + u2.1);
    let sl = libm::hypot(sum.0, sum.1);
    if sl < 1e-9 || radius.is_nan() || radius <= 0.0 {
        return None;
    }
    // Half the angle between the sides: cos = |u1 + u2| / 2.
    let cos_h = sl / 2.0;
    let sin_h = libm::sqrt(1.0 - cos_h * cos_h);
    if sin_h < 1e-9 {
        return None;
    }
    let (bx, by) = (sum.0 / sl, sum.1 / sl);
    let tl = radius * cos_h / sin_h;
    let d = radius / sin_h;
    Some(Round {
        f: *f,
        keys: [ra.idx, rb.idx],
        corner: (v.x, v.y),
        tangent: [(v.x + u1.0 * tl, v.y + u1.1 * tl), (v.x + u2.0 * tl, v.y + u2.1 * tl)],
        arc_mid: (v.x + bx * (d - radius), v.y + by * (d - radius)),
    })
}

/// The arc-length fraction of the polyline `a` nearest to `p`.
fn frac_on(a: &[V3], p: V3) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    let mut run = 0.0;
    let mut spans = Vec::new();
    for w in a.windows(2) {
        let (q, r) = (w.first()?, w.get(1)?);
        let e = sub(*r, *q);
        let l2 = dot(e, e);
        let l = libm::sqrt(l2);
        let u = if l2 > 1e-18 { (dot(sub(p, *q), e) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let c = [q[0] + e[0] * u, q[1] + e[1] * u, q[2] + e[2] * u];
        let dist = sub(p, c);
        let dd = dot(dist, dist);
        if best.is_none_or(|(b, _)| dd < b) {
            best = Some((dd, run + u * l));
        }
        run += l;
        spans.push(l);
    }
    (run > 0.0).then(|| best.map(|(_, at)| at / run)).flatten()
}

fn round3(p: V3) -> V3 {
    p.map(|v| round_to(v, 6))
}

/// The face a reference names, as the feature drew it.
fn face_of<'a>(
    r: &Rep,
    swept: &'a HashMap<u32, Swept>,
    rounds: &HashMap<u32, Vec<Option<Round>>>,
    sketches: &[Geometry],
    cap_tags: &HashSet<u16>,
) -> Option<Face<'a>> {
    if let Some(list) = rounds.get(&r.fid) {
        return list.get((r.idx as usize).checked_sub(1)?).copied().flatten().map(Face::Round);
    }
    let f = swept.get(&r.fid)?;
    if cap_tags.contains(&r.tag) {
        return Some(Face::Cap(r.fid, f, r.idx));
    }
    let mut c = sketches.get(f.sketch)?.segs.iter().filter(|s| s.a == r.idx && !s.construction);
    match (c.next(), c.next()) {
        (Some(s), None) => Some(Face::Side(r.fid, f, *s)),
        _ => None,
    }
}

/// The plane of a planar face as the feature drew it: a point on it and a unit normal.
fn face_plane(face: &Face, sketches: &[Geometry]) -> Option<(V3, V3)> {
    let unit = |v: V3| -> Option<V3> {
        let l = libm::sqrt(dot(v, v));
        (l > 1e-9).then(|| [v[0] / l, v[1] / l, v[2] / l])
    };
    match face {
        Face::Cap(_, g, c) => Some((lift(g, 0.0, 0.0, cap_height(g, *c)), unit(normal(g))?)),
        Face::Side(_, f, s) if s.kind == SegKind::Line => {
            let gf = sketches.get(f.sketch)?;
            let (p0, p1) = (point(&gf.pts, s.s)?, point(&gf.pts, s.e)?);
            let o = lift(f, p0.x * 1000.0, p0.y * 1000.0, 0.0);
            let q = lift(f, p1.x * 1000.0, p1.y * 1000.0, 0.0);
            Some((o, unit(cross(sub(q, o), normal(f)))?))
        }
        _ => None,
    }
}

/// An edge between two planar faces, found from the planes alone: the line where they meet,
/// between the planes of the two vertex faces. This is the edge when a later feature's face
/// is coplanar with an earlier one (SolidWorks merges the two, and the record keeps naming the
/// earlier feature's face), so the two faces need not be neighbours in one profile.
fn plane_edge(faces: &[Option<Face>], sketches: &[Geometry]) -> Option<V3> {
    let [Some(fa), Some(fb), Some(fc), Some(fd)] = faces else { return None };
    let ((p1, n1), (p2, n2)) = (face_plane(fa, sketches)?, face_plane(fb, sketches)?);
    let (p3, n3) = face_plane(fc, sketches)?;
    let (p4, n4) = face_plane(fd, sketches)?;
    let d = cross(n1, n2);
    let dl2 = dot(d, d);
    if dl2 < 1e-9 {
        return None;
    }
    // A point on both planes: o = (h1 (n2 x d) + h2 (d x n1)) / |d|^2 with h = n . p.
    let (h1, h2) = (dot(n1, p1), dot(n2, p2));
    let (u, v) = (cross(n2, d), cross(d, n1));
    let o = comb(u, h1 / dl2, v, h2 / dl2);
    let end = |p: V3, n: V3| -> Option<V3> {
        let den = dot(d, n);
        (den.abs() > 1e-9).then(|| comb(o, 1.0, d, dot(n, sub(p, o)) / den))
    };
    let (a, b) = (end(p3, n3)?, end(p4, n4)?);
    let e = sub(b, a);
    if dot(e, e) < 1e-12 {
        return None;
    }
    Some(round3(comb(a, 0.5, b, 0.5)))
}

/// The midpoint of an edge record (`edges.midpoint`): the first two references are the
/// edge's faces, the last two bound its end vertices. The faces may belong to different
/// features (a cut's wall against a boss's cap), or be the merged coplanar face of an
/// earlier one ([`plane_edge`]).
fn midpoint(
    reps: &[Rep],
    swept: &HashMap<u32, Swept>,
    rounds: &HashMap<u32, Vec<Option<Round>>>,
    sketches: &[Geometry],
    cap_tags: &HashSet<u16>,
    work: &mut u64,
) -> Result<Option<V3>, String> {
    if let Some(m) = midpoint_sketch(reps, swept, rounds, sketches, cap_tags, work)? {
        return Ok(Some(m));
    }
    let faces: Vec<Option<Face>> = reps.iter().map(|r| face_of(r, swept, rounds, sketches, cap_tags)).collect();
    Ok(plane_edge(&faces, sketches))
}

/// What [`closed_edge`] reads from the features before the fillet.
struct Model<'a> {
    swept: &'a HashMap<u32, Swept>,
    rounds: &'a HashMap<u32, Vec<Option<Round>>>,
    revolved: &'a HashMap<u32, Revolved>,
    sketches: &'a [Geometry],
    cap_tags: &'a HashSet<u16>,
}

/// The profile line a revolve face is swept from, and the revolve's axis: a point on it and
/// its unit direction (mm, model axes).
fn revolve_line(r: &Rep, m: &Model) -> Option<(V3, V3, V3, V3)> {
    let rv = m.revolved.get(&r.fid)?;
    let g = m.sketches.get(rv.on.sketch)?;
    let mut it = g.segs.iter().filter(|s| s.a == r.idx && !s.construction && !s.centerline);
    let (Some(s), None) = (it.next(), it.next()) else { return None };
    if s.kind != SegKind::Line {
        return None;
    }
    let at = |p: Pt| lift(&rv.on, p.x * 1000.0, p.y * 1000.0, 0.0);
    let (p, q) = (at(point(&g.pts, s.s)?), at(point(&g.pts, s.e)?));
    let [a0, a1] = rv.axis;
    let (o, o1) = (at(a0), at(a1));
    let d = sub(o1, o);
    let l = libm::sqrt(dot(d, d));
    (l > 1e-9).then(|| (p, q, o, d.map(|c| c / l)))
}

/// The plane of a face a closed edge lies on: a flat face of an extrusion, or the disc a
/// revolve sweeps from a profile line square to its axis.
fn loop_plane(r: &Rep, m: &Model) -> Option<(V3, V3)> {
    if let Some((p, q, _, u)) = revolve_line(r, m) {
        return (dot(sub(q, p), u).abs() < 1e-9).then_some((p, u));
    }
    let face = face_of(r, m.swept, m.rounds, m.sketches, m.cap_tags)?;
    if matches!(face, Face::Cap(_, g, _) if g.through) {
        return None;
    }
    face_plane(&face, m.sketches)
}

/// A closed edge: a record `[A, B, B, B]` names the loop where face A meets face B, with B in
/// both vertex slots (ballscrew_nut_block's Chamfer1: the rims of the holes Cut-Extrude3
/// drilled into a side wall of Boss-Extrude1). B is a plane across A; A is the wall of a full
/// circle of an extrusion, or a revolve's cylinder or cone about an axis square to B. The point
/// returned lies on the loop opposite the circle's start (as [`revolve_circle`]'s).
fn closed_edge(reps: &[Rep], m: &Model, work: &mut u64) -> Result<Option<V3>, String> {
    let same = |a: &Rep, b: &Rep| (a.tag, a.fid, a.idx, a.via) == (b.tag, b.fid, b.idx, b.via);
    let [r0, r1, r2, r3] = reps else { return Ok(None) };
    if !same(r1, r2) || !same(r1, r3) || same(r0, r1) {
        return Ok(None);
    }
    for (wall, across) in [(r0, r1), (r1, r0)] {
        let Some((o, n)) = loop_plane(across, m) else { continue };
        if let Some((p, q, a, u)) = revolve_line(wall, m) {
            // The line's point where the plane cuts the axis' height.
            let (tp, tq) = (dot(sub(p, a), u), dot(sub(q, a), u));
            let den = dot(u, n);
            if (tq - tp).abs() < 1e-9 || (den.abs() - 1.0).abs() > 1e-6 {
                continue;
            }
            let t = dot(sub(o, a), n) / den;
            let k = (t - tp) / (tq - tp);
            if !(-1e-9..=1.0 + 1e-9).contains(&k) {
                continue;
            }
            let x = comb(p, 1.0 - k, q, k);
            let c = comb(a, 1.0, u, t);
            let w = sub(x, c);
            if dot(w, w) < 1e-12 {
                continue;
            }
            return Ok(Some(round3(sub(c, w)).map(|v| v + 0.0)));
        }
        let Some(Face::Side(_, f, s)) = face_of(wall, m.swept, m.rounds, m.sketches, m.cap_tags) else { continue };
        if s.kind != SegKind::Arc || s.s != s.e {
            continue;
        }
        let (Some(g), along) = (m.sketches.get(f.sketch), normal(f)) else { continue };
        let den = dot(along, n);
        if den.abs() < 1e-6 {
            continue;
        }
        let Some(a) = poly(f, g, &s, work)? else { continue };
        let Some(p) = point_at(&a, 0.5) else { continue };
        let p = comb(p, 1.0, along, dot(sub(o, p), n) / den);
        return Ok(Some(round3(snap(p, f, g, &s))));
    }
    Ok(None)
}

fn midpoint_sketch(
    reps: &[Rep],
    swept: &HashMap<u32, Swept>,
    rounds: &HashMap<u32, Vec<Option<Round>>>,
    sketches: &[Geometry],
    cap_tags: &HashSet<u16>,
    work: &mut u64,
) -> Result<Option<V3>, String> {
    let [r0, r1, r2, r3] = reps else { return Ok(None) };
    let geo = |f: &Swept| sketches.get(f.sketch);
    let face = |r: &Rep| face_of(r, swept, rounds, sketches, cap_tags);
    let (Some(fa), Some(fb)) = (face(r0), face(r1)) else { return Ok(None) };
    let (fc, fd) = (face(r2), face(r3));
    let sides: Vec<Face> = [fa, fb].into_iter().filter(|x| matches!(x, Face::Side(..))).collect();
    let caps: Vec<Face> = [fa, fb].into_iter().filter(|x| matches!(x, Face::Cap(..))).collect();
    let round: Vec<Round> =
        [fa, fb].into_iter().filter_map(|x| if let Face::Round(r) = x { Some(r) } else { None }).collect();
    match (sides.as_slice(), caps.as_slice()) {
        ([], [Face::Cap(_, g, c)]) if round.len() == 1 => {
            let Some(r) = round.first() else { return Ok(None) };
            Ok(Some(round3(lift(&r.f, r.arc_mid.0 * 1000.0, r.arc_mid.1 * 1000.0, cap_height(g, *c)))))
        }
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
            // A fillet that rounded the corner at an end of this side shortens it to the
            // circle's tangent point (whatever neighbours the record lists).
            for r in rounds.values().flatten().flatten() {
                if r.f.sketch == f.sketch
                    && let Some(k) = r.keys.iter().position(|k| *k == s.a)
                    && let Some(&(tx, ty)) = r.tangent.get(k)
                {
                    let at = |x: f64, y: f64| frac_on(&a, lift(f, x * 1000.0, y * 1000.0, 0.0));
                    if let (Some(t), Some(c)) = (at(tx, ty), at(r.corner.0, r.corner.1)) {
                        if c < 0.5 {
                            lo = lo.max(t);
                        } else {
                            hi = hi.min(t);
                        }
                    }
                }
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
            // The ends where the two vertex faces cut the edge's line, when both are planes
            // across it: a cap, or a flat side of another feature (T-Nut-M4-DropIn's Fillet2: a
            // corner of the boss from its top down to the wall of a cut made from the side).
            let dir = normal(f);
            let ends: Vec<f64> = [fc, fd]
                .into_iter()
                .flatten()
                .filter(|x| !matches!(x, Face::Cap(_, s, _) if s.through))
                .filter_map(|x| face_plane(&x, sketches))
                .filter_map(|(o, nv)| {
                    let den = dot(dir, nv);
                    (den.abs() > 1e-6).then(|| dot(sub(o, p0), nv) / den)
                })
                .collect();
            if let [t0, t1] = ends[..]
                && (t0 - t1).abs() > 1e-6
            {
                return Ok(Some(round3(comb(p0, 1.0, dir, f64::midpoint(t0, t1)))));
            }
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
    let tree = feature_tree_unchecked(bytes, file_name, limits)?;
    if let Some(t) = &tree {
        check(t)?;
    }
    Ok(tree)
}

/// [`feature_tree`] without the check for features it cannot rebuild: the tree as far as it
/// is read (unsupported features appear as `unsupported`), for study and tests.
///
/// # Errors
/// A short human-readable reason.
pub fn feature_tree_unchecked(bytes: &[u8], file_name: &str, limits: &Limits) -> Result<Option<Value>, String> {
    let Some((rf, c0, chunk)) = streams(bytes, limits)? else { return Ok(None) };
    let file = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    convert_in(&rf, &c0, file, chunk).map(Some)
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
    drop_nested(&mut nodes);
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
    let mut rounds: HashMap<u32, Vec<Option<Round>>> = HashMap::new();
    let mut revolved: HashMap<u32, Revolved> = HashMap::new();
    let mut hole: Option<HoleState> = None;
    // Sketch nodes without entities take the configuration stream's sketch objects in order,
    // when there are as many of each.
    // The end of a sketch's entities: its span, or, when that holds none, past the alignment
    // groups written right after the sketch node (Arm Tab Large PolyCarb First's Sketch2: thirteen
    // `moAlignGroup_c` nodes, then the sketch's entities).
    let sketch_end = |ni: usize, n: &Node| -> usize {
        let (_, b) = span(n);
        if ENT_HDRS.iter().any(|p| find(rf, p, n.post.min(b), b).is_some()) {
            return b;
        }
        let mut j = ni + 1;
        while nodes.get(j).is_some_and(|m| m.cls.as_deref() == Some("moAlignGroup_c")) {
            j += 1;
        }
        if j == ni + 1 {
            return b;
        }
        let e = nodes.get(j).map_or(rf.len(), |m| m.start);
        if ENT_HDRS.iter().any(|p| find(rf, p, n.post.min(e), e).is_some()) { e } else { b }
    };
    let empty_sketches = nodes
        .iter()
        .enumerate()
        .filter(|&(ni, n)| {
            let (a, _) = span(n);
            let b = sketch_end(ni, n);
            n.cls.as_deref() == Some("moProfileFeature_c")
                && a < b
                && ENT_HDRS.iter().all(|p| find(rf, p, n.post.min(b), b).is_none())
        })
        .count();
    let in_config = config_sketches(c0);
    let mut from_config = (empty_sketches > 0 && in_config.len() == empty_sketches).then(|| in_config.into_iter());
    // The archive index of each node (patterns name their seeds by it), read when needed.
    let mut node_index: Option<HashMap<u32, usize>> = None;
    // Mirror id -> its plane (origin mm, unit normal), for edges on the mirrored copy.
    let mut mirror_planes: HashMap<u32, (V3, V3)> = HashMap::new();
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
            let eb = sketch_end(ni, n);
            let h = ENT_HDRS.iter().filter_map(|p| find(rf, p, n.post, eb)).min().unwrap_or(eb);
            let geo = match (h == eb, from_config.as_mut()) {
                // A sketch whose entities the configuration stream keeps.
                (true, Some(it)) => it.next().and_then(|(body, end)| parse_sketch(c0, body, end)),
                _ => h.checked_sub(14).and_then(|body| parse_sketch(rf, body, eb)),
            };
            let Some(geo) = geo else {
                last_sketch = None;
                if let Some(h) = hole.as_mut() {
                    h.sketches += 1;
                }
                continue;
            };
            let mut plane = find_plane(rf, a, eb, uc)?;
            if plane.is_none()
                // The first sketch of a file: its plane object is written just before the node.
                && let Some(j) = rfind(rf, b"moCompRefPlane_c", a.saturating_sub(400), a)
            {
                plane = find_plane(rf, j.saturating_sub(6), b, uc)?;
            }
            if plane.is_none()
                // A sketch on a fixed reference plane (zsCAD): its plane data, written just
                // before the node, is a plane-data block (origin, normal, rotation, t, 1.0).
                && let Some(j) = rfind(rf, b"moFixedRefPlnData_c", a.saturating_sub(400), a)
            {
                plane = find_plane(rf, j, b, uc)?;
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
                Some(4) => "up_to_surface".to_string(),
                Some(5) => "offset_from_surface".to_string(),
                Some(3) => "up_to_vertex".to_string(),
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
            // Byte 12 of the end condition reverses that default direction (a boss against its
            // sketch normal, a cut along it): where a direction vector is written it agrees on
            // every sample, and parts that write none need it (200mm Topmost Platform's cuts).
            if es.is_some_and(|e| e.b12 == 1) && !symmetric {
                distance = distance.map(|v| -v);
            }
            // The written direction vector says which way the extrusion really runs; a distance
            // against it is reversed (MicroGearMotor Boss-Extrude1).
            if let (Some(d), Some(sk), false, false) = (distance, last_sketch.as_ref(), symmetric, through_all)
                && let Some(&(_, plane)) = sketch_by_name.get(sk)
                && let Some(dir) = extrude_direction(rf, a, b)
                && let Some(agree) = direction_agrees(cross(plane.x, plane.y), d, dir)
                && !agree
            {
                distance = Some(-d);
            }
            // Up to a surface (4) or offset from a surface (5): the face the end names. A face ahead
            // of the sweep is the end condition itself (`to_face`); a face behind the sketch (an
            // offset from the part's bottom face, say) only gives a distance.
            // Up to a vertex (3): the end is the plane through the vertex parallel to the
            // sketch; the vertex is written in model coordinates (V-Slot Rail_PIVOT Boss-Extrude2).
            if let (Some(3), Some(sk)) = (typ, last_sketch.as_ref()) {
                distance = None;
                if let (Some(&(_, plane)), Some(p)) = (sketch_by_name.get(sk), point_ref(rf, a, b)) {
                    let h = dot(sub(p, plane.origin), cross(plane.x, plane.y));
                    distance = (h.abs() > 1e-6).then_some(round_to(h, 6) + 0.0);
                }
            }
            let mut up_to: Option<(V3, V3, f64, bool)> = None;
            let mut end_s: Option<f64> = None;
            if let (Some(t @ (4 | 5)), Some(sk)) = (typ, last_sketch.as_ref()) {
                distance = None;
                if let Some(&(si, plane)) = sketch_by_name.get(sk)
                    && let Some(r) = end_face_ref(rf, a, b)
                    && let Some((pt, nf, region)) = face_pick(&r, &swept, &sketches, &cap_tags)
                {
                    let n = cross(plane.x, plane.y);
                    let den = dot(n, nf);
                    let parallel = (den.abs() - 1.0).abs() < 1e-9;
                    // Where the face's plane cuts the sketch's normal axis (mm from the sketch plane).
                    let s_face = dot(sub(pt, plane.origin), nf) / den;
                    // Whether the whole profile, swept towards the face, lands on the face.
                    let covered = |dir: f64| -> bool {
                        let Some(g) = sketches.get(si) else { return false };
                        g.segs.iter().filter(|q| !q.construction).all(|q| {
                            let Some(pts) = seg_uv(g, q) else { return false };
                            let step = (pts.len() / 24).max(1);
                            pts.iter().step_by(step).chain(pts.last()).all(|&(u, v)| {
                                let p = [0, 1, 2].map(|i| {
                                    plane.origin.get(i).copied().unwrap_or(0.0)
                                        + u * 1000.0 * plane.x.get(i).copied().unwrap_or(0.0)
                                        + v * 1000.0 * plane.y.get(i).copied().unwrap_or(0.0)
                                });
                                let t = dot(sub(pt, p), nf) / dot([n[0] * dir, n[1] * dir, n[2] * dir], nf);
                                let hit = [0, 1, 2].map(|i| {
                                    p.get(i).copied().unwrap_or(0.0) + t * dir * n.get(i).copied().unwrap_or(0.0)
                                });
                                on_face(&region, &sketches, hit)
                            })
                        })
                    };
                    if t == 4 && den.abs() > 1e-9 {
                        let dir = extrude_direction(rf, a, b)
                            .and_then(|v| direction_agrees(n, 1.0, v))
                            .map_or(s_face.signum(), |agree| if agree { 1.0 } else { -1.0 });
                        if s_face * dir > 1e-9 && covered(dir) {
                            up_to = Some((pt, nf, 0.0, dir < 0.0));
                            end_s = parallel.then_some(s_face);
                        } else if s_face * dir > 1e-9 && parallel {
                            // The face is smaller than the profile: SolidWorks extends its surface.
                            distance = Some(round_to(s_face, 6) + 0.0);
                            end_s = distance;
                        }
                    } else if t == 5
                        && parallel
                        && let Some(off) = ps.first().map(|v| mm(*v))
                    {
                        // The end plane is the face's plane moved `D1` into the material (against the
                        // face's outward normal): spring_base_v1 Boss-Extrude2 (bottom face y = 0, D1 10,
                        // ends at y = 10), FrameBracket Cut-Extrude3 (a boss's far face, D1 1, leaves
                        // a floor of 1 mm). The sweep runs from the sketch to that plane.
                        let s_end = s_face - off / den;
                        let dir = s_end.signum();
                        if s_end.abs() > 1e-9 {
                            end_s = Some(round_to(s_end, 6) + 0.0);
                            if s_face * dir > 1e-9 && covered(dir) {
                                up_to = Some((pt, nf, round_to((s_end - s_face) * dir, 6) + 0.0, dir < 0.0));
                            } else {
                                distance = end_s;
                            }
                        }
                    }
                }
            }
            // A blind cut with a second length (`D2`, or `D<n+1>` after the `n` dimensions of its
            // own sketch) starts that far from its sketch plane. With an end spec of X 1 the start
            // lies ahead along the cut direction (MicroGearMotor Cut-Extrude2: D1 4.25 mm, offset
            // 0.8 mm, the void spans z -5.05 .. -0.8 from the Front plane); with X 0 it lies
            // behind (idler_single_v1: D1 10 mm, D2 1 mm, the truth's bore spans x -1 .. 2.9 from
            // the Right plane, running along the normal).
            let start_offset = named_params(rf, a, b, nt)?
                .iter()
                .find(|(k, _)| k.strip_prefix('D').is_some_and(|n| n != "1" && n.bytes().all(|c| c.is_ascii_digit())))
                .map(|(_, v)| mm(*v))
                .filter(|_| op == "cut" && typ == Some(0) && !symmetric);
            if let (Some(off), Some(sk), Some(d)) = (start_offset, last_sketch.as_ref(), distance)
                && let Some(entry) = sketch_by_name.get_mut(sk)
                && swept.values().all(|w| w.sketch != entry.0)
            {
                let ahead = es.is_some_and(|e| e.x == 1);
                let dir = cross(entry.1.x, entry.1.y).map(|c| c * d.signum() * if ahead { 1.0 } else { -1.0 });
                let mut o = entry.1.origin;
                for (v, n) in o.iter_mut().zip(dir) {
                    *v = round_to(*v + off * n, 6) + 0.0;
                }
                entry.1.origin = o;
                let origin = entry.1.origin;
                if let Some(Value::Object(f)) = feats.iter_mut().find(|f| {
                    f.get("type").and_then(Value::as_str) == Some("sketch")
                        && f.get("name").and_then(Value::as_str) == Some(sk.as_str())
                }) && let Some(Value::Object(pl)) = f.get_mut("plane")
                {
                    pl.insert("origin".into(), json!(origin));
                }
            }
            let mut js = Map::new();
            js.insert("type".into(), json!("extrude"));
            js.insert("name".into(), json!(n.name));
            js.insert("sketch".into(), json!(last_sketch));
            js.insert("profiles".into(), json!("all"));
            js.insert("operation".into(), json!(op));
            js.insert("target".into(), if op == "new" { Value::Null } else { json!(first_solid) });
            js.insert("sw_end_kind".into(), json!(end));
            if let Some((pt, nf, off, reverse)) = up_to {
                js.insert("end_condition".into(), json!("to_face"));
                js.insert("face".into(), json!({"point": json_v3(pt), "normal": json_v3(nf)}));
                if off.abs() > 1e-9 {
                    js.insert("offset".into(), json!(round_to(off, 6) + 0.0));
                }
                if reverse {
                    js.insert("reverse".into(), json!(true));
                }
            } else {
                js.insert("distance".into(), json!(distance));
            }
            if through_all {
                js.insert("through_all".into(), json!(true));
            }
            js.insert("symmetric".into(), json!(symmetric));
            if let Some(v) = extrude_direction(rf, a, b) {
                js.insert("sw_dir".into(), json!(v));
            }
            let sw: Vec<Value> =
                cands.iter().take(3).map(|c| json!({"X": c.x, "type": c.kind, "flag": c.flag, "b12": c.b12})).collect();
            js.insert("sw_end".into(), json!(sw));
            if let (Some(sk), Some(dist)) = (last_sketch.as_ref(), end_s.or(distance))
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
                // A centerline that is a projected model edge (M5_bolt's Sketch3: the bolt's bottom
                // silhouette, flags 0x80d1) is not the axis the sketch drew.
                let first = g
                    .segs
                    .iter()
                    .position(|s| line(s) && s.centerline && !s.external && s.id != 0xffff_ffff)
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
                // The axis the revolve names, when it is a line reference (a reference axis or
                // an edge): written in model coordinates. A sketch line on it is the axis;
                // without one the axis goes in as a line through a point.
                let named = line_ref(rf, a, b);
                let on_named = |k: usize| -> bool {
                    let (Some((o, dir)), Some(&(_, jp))) =
                        (named, sketch_by_name.get(last_sketch.as_deref().unwrap_or("")))
                    else {
                        return false;
                    };
                    let Some(s) = g.segs.get(k).filter(|s| line(s)) else { return false };
                    let model =
                        |q: Pt| -> V3 { comb(comb(jp.origin, 1.0, jp.x, q.x * 1000.0), 1.0, jp.y, q.y * 1000.0) };
                    let off = |q: Pt| {
                        let w = sub(model(q), o);
                        let c = cross(w, dir);
                        dot(c, c) < 1e-12
                    };
                    matches!((point(&g.pts, s.s), point(&g.pts, s.e)), (Some(p), Some(q)) if off(p) && off(q))
                };
                let axis = if named.is_some() {
                    (0..g.segs.len()).find(|&k| on_named(k))
                } else if first.is_some_and(one_sided) {
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
                let axis_json = match (axis, named) {
                    (Some(k), _) => json!({"line": format!("c{k}")}),
                    (None, Some((o, dir))) => json!({"origin": json_v3(round3(o)), "direction": json_v3(dir)}),
                    (None, None) => Value::Null,
                };
                js.insert("axis".into(), axis_json);
                // A full revolve's circles can be named by edge records.
                if let Some(&(_, plane)) = last_sketch.as_ref().and_then(|s| sketch_by_name.get(s))
                    && ang.is_none_or(|v| (v.abs() - TAU).abs() < 1e-9)
                {
                    // The axis in sketch coordinates (m): its line, or the named line taken
                    // onto the sketch plane.
                    let in_sketch = |p: V3| -> Pt {
                        let w = sub(p, plane.origin);
                        Pt { x: dot(w, plane.x) / 1000.0, y: dot(w, plane.y) / 1000.0, ext: false, id: 0, alone: 0 }
                    };
                    let ends = match (axis.and_then(|k| g.segs.get(k)), named) {
                        (Some(s), _) => point(&g.pts, s.s).zip(point(&g.pts, s.e)),
                        (None, Some((o, dir))) => Some((in_sketch(o), in_sketch(comb(o, 1.0, dir, 1.0)))),
                        (None, None) => None,
                    };
                    if let Some((p, q)) = ends {
                        let on = Swept { sketch: i, plane, distance: 0.0, symmetric: false, through: false };
                        revolved.insert(n.id, Revolved { on, axis: [p, q] });
                    }
                }
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
            // A mirror on a plane feature names it: the plane's id 38 bytes before the display
            // block of the plane the mirror keeps (`<tag> 00 00 <u32 id> <u32 stamp> ... c7 cf ff ff`;
            // pulley_large's Mirror1: the Front plane). Only the three standard planes are read
            // so: a plane feature of the part (base v2's Plane2, a plane through a line square to
            // another) keeps its definition, not its frame.
            if plane.is_none()
                && let Some(m) = find(rf, &[0xc7, 0xcf, 0xff, 0xff], a, b)
                && let Some(at) = m.checked_sub(38)
                && rf.get(at.saturating_sub(2)..at) == Some(&[0, 0][..])
            {
                plane = match u32_at(rf, at) {
                    Some(2) => Some(([0.0; 3], [0.0, 0.0, 1.0])),
                    Some(3) => Some(([0.0; 3], [0.0, 1.0, 0.0])),
                    Some(4) => Some(([0.0; 3], [1.0, 0.0, 0.0])),
                    _ => None,
                };
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
            // A mirror of features (`moMirrorPattern_c`) names them in a seed list as patterns
            // do (all 54 in the corpora have one); a mirror of the body (`moMirrorSolid_c`) has none.
            let mut mirrored: Vec<String> = Vec::new();
            if cls.as_deref() == Some("moMirrorPattern_c")
                && let Some(seeds) = patterns::seed_list(rf, a, b)
            {
                let index = node_index.get_or_insert_with(|| node_indices(rf, &nodes, &span));
                for p in &seeds.ptrs {
                    let Some(m) = index.get(p).and_then(|&k| nodes.get(k)) else { continue };
                    if emitted(&feats, &m.name) && !mirrored.contains(&m.name) {
                        mirrored.push(m.name.clone());
                    }
                }
                if mirrored.len() != seeds.ptrs.len() {
                    mirrored.clear();
                }
            }
            if let Some(pl) = plane {
                mirror_planes.insert(n.id, pl);
            }
            let mut js = Map::new();
            js.insert("type".into(), json!("mirror"));
            js.insert("name".into(), json!(n.name));
            js.insert("features".into(), json!(mirrored));
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
            let mirror_ids: HashSet<u32> = mirror_planes.keys().copied().collect();
            let sel = selection(edge_records_to(rf, a, b, &mirror_ids)?);
            let mut edges = Vec::new();
            let mut mids: Vec<Option<V3>> = Vec::new();
            let mut stale = 0usize;
            let mut unresolved = false;
            let mut made: Vec<Option<Round>> = Vec::new();
            let mut missed: Vec<String> = Vec::new();
            // The references of an edge left out, by feature name and face number (`c`: a cap).
            let describe = |reps: &[Rep]| -> String {
                let names: Vec<String> = reps
                    .iter()
                    .map(|r| {
                        let nm = nodes.iter().find(|q| q.id == r.fid).map_or("?", |q| q.name.as_str());
                        format!("{nm}/{}{}", r.idx, if cap_tags.contains(&r.tag) { "c" } else { "" })
                    })
                    .collect();
                names.join(" ")
            };
            for (_, reps) in &sel {
                if let Some((m, path)) = revolve_circle(reps, &revolved, &sketches) {
                    mids.push(Some(m));
                    made.push(None);
                    edges.push(json!({"midpoint": m, "path": path}));
                    continue;
                }
                // A record that starts with a face of this very fillet names its edge as
                // `[own face, vertex face, face, vertex face, face]` (Power Supply Unit's Fillet1:
                // the corner between two sides, between the caps): read as the usual four.
                let own = match reps.as_slice() {
                    [r0, r1, r2, r3, r4] if r0.fid == n.id && r1.fid != n.id && r3.fid != n.id => {
                        Some(vec![*r2, *r4, *r1, *r3])
                    }
                    _ => None,
                };
                let reps = own.as_deref().unwrap_or(reps);
                let m = match mirrored_edge(reps, &swept, &mirror_planes) {
                    Some((plane, reps, swept)) => midpoint(&reps, &swept, &rounds, &sketches, &cap_tags, &mut work)?
                        .map(|m| reflect(m, plane).map(|c| round_to(c, 6) + 0.0)),
                    None => match midpoint(reps, &swept, &rounds, &sketches, &cap_tags, &mut work)? {
                        Some(m) => Some(m),
                        None => {
                            let model = Model {
                                swept: &swept,
                                rounds: &rounds,
                                revolved: &revolved,
                                sketches: &sketches,
                                cap_tags: &cap_tags,
                            };
                            closed_edge(reps, &model, &mut work)?
                        }
                    },
                };
                mids.push(m);
                made.push(ps.first().filter(|_| fillet).and_then(|r| round_of(reps, &swept, &sketches, &cap_tags, *r)));
                match m {
                    Some(m) => edges.push(json!({"midpoint": m})),
                    // References to faces the sketch no longer has are superseded selections.
                    None if reps.iter().all(|r| {
                        swept.contains_key(&r.fid) || rounds.contains_key(&r.fid) || revolved.contains_key(&r.fid)
                    }) =>
                    {
                        stale += 1;
                        if missed.len() < 8 {
                            missed.push(format!("stale: {}", describe(reps)));
                        }
                    }
                    None => {
                        unresolved = true;
                        if missed.len() < 8 {
                            missed.push(describe(reps));
                        }
                    }
                }
            }
            unresolved |= edges.is_empty();
            if made.iter().any(Option::is_some) {
                rounds.insert(n.id, made);
            }
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
            // A 90-degree distance-angle chamfer: the bevel stands square on the face D1 runs
            // along (adpator 倒角1: D1 0.3 mm, the part's top face ends 0.3 mm short of the rim
            // and the bevel is a cylinder). On the circle edges of a revolve that is two distances.
            if !fillet
                && !unresolved
                && let (Some(&d1), Some(&angle)) = (ps.first(), ps.get(1))
                && (angle - TAU / 4.0).abs() < 1e-6
                && let Some(chs) = square_chamfer(&n.name, mm(d1), &sel, &revolved, &sketches)
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
                js.insert("sw_unresolved".into(), json!(missed));
            }
            feats.push(Value::Object(js));
        } else if matches!(cls.as_deref(), Some("moLPattern_c" | "moCirPattern_c")) {
            let index = node_index.get_or_insert_with(|| node_indices(rf, &nodes, &span));
            let circular = cls.as_deref() == Some("moCirPattern_c");
            let slots = param_slots(rf, a, b, nt)?;
            let slot = |k: &str| slots.iter().find(|(name, _)| name == k).map(|(_, at)| *at);
            let int = |k: &str| slot(k).and_then(|at| u32_at(rf, at));
            let real = |k: &str| slot(k).and_then(|at| f64_at(rf, at)).filter(|v| v.is_finite());
            let made = feature_pattern(
                &n.name,
                circular,
                patterns::seed_list(rf, a, b),
                &patterns::pattern_lines(rf, a, b),
                (int, real),
                |p| index.get(&p).and_then(|&k| nodes.get(k)).map(|m| m.name.clone()),
                &feats,
            );
            feats.extend(made);
        } else if cls.as_deref() == Some("moMirrorStock_c") {
            // A mirrored copy of an external part: the part keeps the source body in its
            // `StockBodies` stream (the importer hands it over as stored body 0); the
            // mirror plane is found by the importer by comparing it with the final body.
            feats.push(json!({"type": "stored_body", "name": n.name, "body": 0, "stock_mirror": true}));
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if cls.as_deref() == Some("moBaseBody_c") {
            // An imported body: the part keeps it, as imported, in its `LocalBodies` stream
            // under the node's name; the importer hands it over as a stored body.
            feats.push(json!({"type": "stored_body", "name": n.name, "body": 0, "local_body": n.name}));
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
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

/// `p` reflected in the plane `(origin, unit normal)`.
fn reflect(p: V3, (o, n): (V3, V3)) -> V3 {
    let k = 2.0 * dot(sub(p, o), n);
    comb(p, 1.0, n, -k)
}

/// An extrusion reflected in a plane: its plane's origin and axes reflected and its depth
/// negated (the normal is the cross product of the axes, which a reflection turns over).
fn reflect_swept(f: &Swept, plane: (V3, V3)) -> Swept {
    let dir = |v: V3| comb(v, 1.0, plane.1, -2.0 * dot(v, plane.1));
    let jp = JsonPlane { origin: reflect(f.plane.origin, plane), x: dir(f.plane.x), y: dir(f.plane.y) };
    Swept { plane: jp, distance: -f.distance, ..*f }
}

/// A mirror's plane, an edge's references reflected back and the extrusions they name.
type Reflected = ((V3, V3), Vec<Rep>, HashMap<u32, Swept>);

/// Fake ids of reflected extrusions (feature ids are far below).
const REFLECTED: u32 = 0x8000_0000;

/// An edge of a mirror's copy (every reference through the same mirror, or to an extrusion):
/// the same edge problem reflected back, where the copy's faces are the seed faces and the
/// other faces are reflected extrusions under fake ids; its midpoint is reflected again.
fn mirrored_edge(reps: &[Rep], swept: &HashMap<u32, Swept>, planes: &HashMap<u32, (V3, V3)>) -> Option<Reflected> {
    let m = reps.iter().find_map(|r| r.via)?;
    let plane = *planes.get(&m)?;
    let mut sw = swept.clone();
    let mut out = Vec::with_capacity(reps.len());
    for r in reps {
        match r.via {
            Some(v) if v == m => out.push(Rep { via: None, ..*r }),
            Some(_) => return None,
            None => {
                let f = swept.get(&r.fid)?;
                sw.insert(r.fid | REFLECTED, reflect_swept(f, plane));
                out.push(Rep { fid: r.fid | REFLECTED, ..*r });
            }
        }
    }
    Some((plane, out, sw))
}

/// Whether a feature of that name was read (not a sketch, not unsupported).
fn emitted(feats: &[Value], name: &str) -> bool {
    feats.iter().any(|f| {
        f.get("name").and_then(Value::as_str) == Some(name)
            && !matches!(f.get("type").and_then(Value::as_str), Some("sketch" | "unsupported") | None)
    })
}

/// The archive index of each node ([`patterns::object_indices`]). A node that declares its
/// class is also the object right after that class, whose index the other nodes of the class
/// write as their class tag (SG90's `Furo4` declares `moHoleWzd_c`, `Furo1` is `tag 709`:
/// `Furo4` is object 710, the index its linear pattern names).
fn node_indices(d: &[u8], nodes: &[Node], span: &impl Fn(&Node) -> (usize, usize)) -> HashMap<u32, usize> {
    let spans: Vec<(usize, usize, u32)> = nodes
        .iter()
        .map(|m| {
            let (a, b) = span(m);
            (a, b, m.id)
        })
        .collect();
    let mut index = patterns::object_indices(d, &spans);
    for (k, m) in nodes.iter().enumerate() {
        let Some(cls) = m.cls.as_deref().filter(|_| m.tag.is_none()) else { continue };
        let mut tags = nodes.iter().filter(|o| o.tag.is_some() && o.cls.as_deref() == Some(cls)).filter_map(|o| o.tag);
        let Some(t) = tags.next() else { continue };
        if tags.all(|u| u == t) {
            let p = u32::from(t) + 1;
            match index.get(&p) {
                Some(&other) if other != k => {
                    index.remove(&p);
                }
                _ => {
                    index.insert(p, k);
                }
            }
        }
    }
    index
}

/// A linear or circular feature pattern (`moLPattern_c`, `moCirPattern_c`) as tree features.
/// Parameters: a linear pattern's instance counts are `D1` and `D2` (integers) and its spacings
/// `D3` and `D4` (m), along the first and second written line; a circular pattern's count is
/// `D1`, its step angle `D2` and its total angle `D3` (rad), about the written line; `D4` is
/// the count of a second circular direction. The seeds are the features the seed list names.
/// Anything not read gives one feature marked `unresolved`.
fn feature_pattern(
    name: &str,
    circular: bool,
    seeds: Option<patterns::Seeds>,
    lines: &[(V3, V3)],
    (int, real): (impl Fn(&str) -> Option<u32>, impl Fn(&str) -> Option<f64>),
    node_name: impl Fn(u32) -> Option<String>,
    feats: &[Value],
) -> Vec<Value> {
    let kind = if circular { "pattern_circular" } else { "pattern_linear" };
    let fail = |why: &str| vec![json!({"type": kind, "name": name, "unresolved": true, "why": why})];
    let Some(seeds) = seeds else { return fail("no seed list") };
    if seeds.ptrs.is_empty() {
        return fail(if seeds.bodies > 0 { "a pattern of bodies" } else { "no seed feature" });
    }
    let mut names: Vec<String> = Vec::new();
    for &p in &seeds.ptrs {
        let Some(s) = node_name(p) else { return fail("a seed feature was not found") };
        if !emitted(feats, &s) {
            return fail("a seed feature was not read");
        }
        if !names.contains(&s) {
            names.push(s);
        }
    }
    let mut out = Vec::new();
    if circular {
        let (Some(count), Some((origin, dir))) = (int("D1"), lines.first()) else {
            return fail("no count or axis");
        };
        if int("D4").is_some_and(|c| c > 1) {
            return fail("a second circular direction");
        }
        let angle = match (real("D2"), real("D3")) {
            (Some(step), _) if (step * f64::from(count) - TAU).abs() < 1e-6 => 360.0,
            (Some(step), _) => step * f64::from(count.saturating_sub(1)) * 360.0 / TAU,
            (None, Some(total)) => total * 360.0 / TAU,
            (None, None) => 360.0,
        };
        if count > 1 {
            out.push(json!({"type": kind, "name": name, "features": names, "count": count,
                "axis": {"origin": json_v3(*origin), "direction": json_v3(*dir)}, "angle": round_to(angle, 9)}));
        }
    } else {
        let dirs = [(int("D1"), real("D3"), lines.first()), (int("D2"), real("D4"), lines.get(1))];
        let mut first: Option<String> = None;
        for (k, (count, spacing, line)) in dirs.into_iter().enumerate() {
            let Some(count) = count else { return fail("no instance count") };
            if count < 2 {
                continue;
            }
            let (Some(spacing), Some((_, dir))) = (spacing, line) else { return fail("no spacing or direction") };
            let mut fs = names.clone();
            let nm = match &first {
                // The second direction repeats the seeds and the first direction's copies.
                Some(f) => {
                    fs.push(f.clone());
                    format!("{name}_{}", k + 1)
                }
                None => name.to_string(),
            };
            out.push(json!({"type": kind, "name": nm, "features": fs, "count": count,
                "direction": json_v3(*dir), "spacing": mm(spacing)}));
            first = Some(nm);
        }
    }
    out
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
                if s("type") == "extrude"
                    && f.get("distance").is_none_or(Value::is_null)
                    && s("end_condition") != "to_face"
                {
                    return Err(format!("{name}: end condition {} is not supported", s("sw_end_kind")));
                }
                if s("type") == "revolve" && f.get("axis").is_none_or(Value::is_null) {
                    return Err(format!("{name}: no axis line in its sketch"));
                }
            }
            "stored_body" => solids += 1,
            "hole" | "mirror" if f.get("unresolved").is_some() => {
                return Err(format!("{name}: the {} was not resolved", s("type")));
            }
            "pattern_linear" | "pattern_circular" if f.get("unresolved").is_some() => {
                return Err(format!("{name}: the pattern was not resolved ({})", s("why")));
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

#[cfg(test)]
mod direction_tests {
    use super::*;

    #[test]
    fn written_direction_decides_the_sign() {
        // Sketch normal +z: an extrusion along +z is written as -y.
        assert_eq!(direction_agrees([0.0, 0.0, 1.0], 9.0, [0.0, -1.0, 0.0]), Some(true));
        assert_eq!(direction_agrees([0.0, 0.0, 1.0], 9.0, [0.0, 1.0, 0.0]), Some(false));
        assert_eq!(direction_agrees([0.0, 0.0, 1.0], -9.0, [0.0, 1.0, 0.0]), Some(true));
        // Other axes are written as they are.
        assert_eq!(direction_agrees([1.0, 0.0, 0.0], 2.0, [-1.0, 0.0, 0.0]), Some(false));
        assert_eq!(direction_agrees([0.0, -1.0, 0.0], -2.0, [0.0, 1.0, 0.0]), Some(true));
        // A tilted sketch, or a vector along another axis, says nothing.
        assert_eq!(direction_agrees([0.8, 0.0, 0.6], 2.0, [1.0, 0.0, 0.0]), None);
        assert_eq!(direction_agrees([1.0, 0.0, 0.0], 2.0, [0.0, 1.0, 0.0]), None);
    }

    #[test]
    fn direction_vector_follows_the_marker() {
        let mut d = vec![7u8; 3];
        d.extend([0, 5, 0, 2, 0, 2, 0, 2, 9]);
        for c in [0.0f64, 1.0, 0.0] {
            d.extend(c.to_le_bytes());
        }
        assert_eq!(extrude_direction(&d, 0, d.len()), Some([0.0, 1.0, 0.0]));
        assert_eq!(extrude_direction(&d, 0, 8), None);
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used, clippy::indexing_slicing)]
mod end_face_tests {
    use super::*;

    fn pt(x: f64, y: f64) -> Option<Pt> {
        Some(Pt { x, y, ext: false, id: 0, alone: 0 })
    }

    /// A 20 x 10 mm rectangle (sketch keys 2..5) extruded 5 mm along +z.
    fn plate() -> (HashMap<u32, Swept>, Vec<Geometry>) {
        let line = |a: u32, s: usize, e: usize| Seg {
            kind: SegKind::Line,
            id: a,
            a,
            s,
            e,
            c: 0,
            dir: 0,
            construction: false,
            centerline: false,
            external: false,
        };
        let g = Geometry {
            pts: vec![pt(0.0, 0.0), pt(0.02, 0.0), pt(0.02, 0.01), pt(0.0, 0.01)],
            segs: vec![line(2, 0, 1), line(3, 1, 2), line(4, 2, 3), line(5, 3, 0)],
            ellipse_axes: Vec::new(),
        };
        let plane = JsonPlane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let f = Swept { sketch: 0, plane, distance: 5.0, symmetric: false, through: false };
        (HashMap::from([(32, f)]), vec![g])
    }

    #[test]
    fn the_end_face_is_the_first_reference_of_the_single_face_object() {
        let word = [0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54];
        let node = |kind: u8| {
            let mut d = vec![0u8; 30];
            d.extend([0, kind, 0, 0, 0x65, 0x23, 0x1d, 0]);
            d.extend(word);
            d.extend(word);
            d.extend([0, 0, 0x3e, 0x82, 0, 0, 0x3b, 0x82, 0x3d, 2, 32, 0, 0, 0, 0x2e, 0x6f, 0xb8, 0x54, 1, 0, 0, 0]);
            d.extend([0u8; 20]);
            d
        };
        let d = node(2);
        let r = end_face_ref(&d, 0, d.len()).unwrap();
        assert_eq!((r.tag, r.fid, r.idx), (0x23e, 32, 1));
        // The header may say 3 there too (the object right behind its class declaration); a node
        // without the pair names no face.
        let d = node(3);
        assert_eq!(end_face_ref(&d, 0, d.len()).map(|r| r.idx), Some(1));
        assert!(end_face_ref(&d, 0, 40).is_none());
    }

    #[test]
    fn a_closed_edge_is_a_circle_cut_by_the_plane_across_it() {
        let (mut swept, mut sketches) = plate();
        let seg = |kind, a, s, e, c| Seg {
            kind,
            id: a,
            a,
            s,
            e,
            c,
            dir: 1,
            construction: false,
            centerline: false,
            external: false,
        };
        // A hole of r 1 mm drilled into the plate's side wall x = 20 (side 3), its centre at
        // y 5, z 2.5: a circle (key 7) on a sketch whose normal is +x.
        sketches.push(Geometry {
            pts: vec![pt(0.005, 0.0025), pt(0.006, 0.0025)],
            segs: vec![seg(SegKind::Arc, 7, 1, 1, 0)],
            ellipse_axes: Vec::new(),
        });
        let plane = JsonPlane { origin: [20.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
        swept.insert(33, Swept { sketch: 1, plane, distance: -10.0, symmetric: false, through: false });
        // A revolve about the y axis of the Front plane: line 4 at x = 5 mm sweeps a cylinder.
        sketches.push(Geometry {
            pts: vec![pt(0.005, 0.0), pt(0.005, 0.01)],
            segs: vec![seg(SegKind::Line, 4, 0, 1, 0)],
            ellipse_axes: Vec::new(),
        });
        let on = Swept {
            sketch: 2,
            plane: JsonPlane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] },
            distance: 0.0,
            symmetric: false,
            through: false,
        };
        let revolved = HashMap::from([(
            34,
            Revolved {
                on,
                axis: [
                    Pt { x: 0.0, y: 0.0, ext: false, id: 0, alone: 0 },
                    Pt { x: 0.0, y: 0.01, ext: false, id: 0, alone: 0 },
                ],
            },
        )]);
        let cap_tags = HashSet::from([9u16]);
        let rounds = HashMap::new();
        let m = Model { swept: &swept, rounds: &rounds, revolved: &revolved, sketches: &sketches, cap_tags: &cap_tags };
        let rep = |tag, fid, idx| Rep { tag, fid, idx, ext: false, ext2: false, via: None };
        let mut work = 0;
        // `[hole, wall, wall, wall]`: the rim, opposite the circle's start.
        let reps = [rep(1, 33, 7), rep(1, 32, 3), rep(1, 32, 3), rep(1, 32, 3)];
        assert_eq!(closed_edge(&reps, &m, &mut work).unwrap(), Some([20.0, 4.0, 2.5]));
        // Either face may come first.
        let reps = [rep(1, 32, 3), rep(1, 33, 7), rep(1, 33, 7), rep(1, 33, 7)];
        assert_eq!(closed_edge(&reps, &m, &mut work).unwrap(), Some([20.0, 4.0, 2.5]));
        // The cylinder cut by the plate's side y = 0 (side 2): the circle of r 5 in that plane.
        let reps = [rep(1, 34, 4), rep(1, 32, 2), rep(1, 32, 2), rep(1, 32, 2)];
        assert_eq!(closed_edge(&reps, &m, &mut work).unwrap(), Some([-5.0, 0.0, 0.0]));
        // A record whose vertex faces differ is not a closed edge.
        let reps = [rep(1, 33, 7), rep(1, 32, 3), rep(9, 32, 1), rep(1, 32, 3)];
        assert_eq!(closed_edge(&reps, &m, &mut work).unwrap(), None);
    }

    #[test]
    fn an_edge_ends_where_its_vertex_planes_cut_it() {
        let (mut swept, mut sketches) = plate();
        // A cut drawn on the plane y = 0 (x along x, y along z) whose line (key 7) runs at z = 3:
        // its wall is the plane z = 3.
        let line = Seg {
            kind: SegKind::Line,
            id: 7,
            a: 7,
            s: 0,
            e: 1,
            c: 0,
            dir: 0,
            construction: false,
            centerline: false,
            external: false,
        };
        sketches.push(Geometry {
            pts: vec![pt(-0.01, 0.003), pt(0.03, 0.003)],
            segs: vec![line],
            ellipse_axes: Vec::new(),
        });
        let plane = JsonPlane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0] };
        swept.insert(33, Swept { sketch: 1, plane, distance: -500.0, symmetric: false, through: true });
        let cap_tags = HashSet::from([9u16]);
        let rep = |tag, fid, idx| Rep { tag, fid, idx, ext: false, ext2: false, via: None };
        let mut work = 0;
        // The corner of sides 3 and 4 at (20, 10), from the plate's far cap (z = 5) down to the
        // cut's wall (z = 3): its middle is at z = 4, not at the plate's mid-depth.
        let reps = [rep(1, 32, 3), rep(1, 32, 4), rep(9, 32, 1), rep(1, 33, 7)];
        let m = midpoint(&reps, &swept, &HashMap::new(), &sketches, &cap_tags, &mut work).unwrap();
        assert_eq!(m, Some([20.0, 10.0, 4.0]));
        // Between the plate's own caps it stays at mid-depth.
        let reps = [rep(1, 32, 3), rep(1, 32, 4), rep(9, 32, 1), rep(9, 32, 0)];
        let m = midpoint(&reps, &swept, &HashMap::new(), &sketches, &cap_tags, &mut work).unwrap();
        assert_eq!(m, Some([20.0, 10.0, 2.5]));
    }

    #[test]
    fn an_edge_of_a_mirrored_copy_is_solved_reflected_back() {
        let (swept, _) = plate();
        // The plate reflected in x = 30: its corners land at x 40..60 and its far cap stays at
        // z = 5.
        let plane = ([30.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let r = reflect_swept(swept.get(&32).unwrap(), plane);
        assert_eq!(lift(&r, 20.0, 10.0, cap_height(&r, 1)), [40.0, 10.0, 5.0]);
        assert_eq!(lift(&r, 0.0, 0.0, cap_height(&r, 0)), [60.0, 0.0, 0.0]);
        // A face of Mirror1's copy (seed face 3) and a face of the plate: the copy's face is the
        // seed's, the plate's is the reflected plate's.
        let rep = |idx, via| Rep { tag: 9, fid: 32, idx, ext: false, ext2: false, via };
        let planes = HashMap::from([(40, plane)]);
        let (p, reps, sw) = mirrored_edge(&[rep(3, Some(40)), rep(1, None)], &swept, &planes).unwrap();
        assert_eq!(p, plane);
        assert_eq!((reps[0].fid, reps[0].via, reps[1].fid), (32, None, 32 | REFLECTED));
        assert!(sw.contains_key(&(32 | REFLECTED)));
        assert_eq!(reflect([35.0, 1.0, 2.0], plane), [25.0, 1.0, 2.0]);
        // Through a mirror whose plane is not known: nothing.
        assert!(mirrored_edge(&[rep(3, Some(41))], &swept, &planes).is_none());
    }

    #[test]
    fn a_face_of_a_mirrored_copy_nests_the_seed_reference() {
        let word = [0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54];
        let mut d = vec![0u8; 4];
        d.extend([1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        d.extend(word);
        d.extend(word);
        d.extend([0, 0]);
        // Mirror1 (id 59), then Cut-Extrude2 (id 55) face 3, then a cap of Boss-Extrude1 (id 30).
        d.extend([0x8a, 0x80, 0, 0, 0x8c, 0x80, 0x8e, 0, 59, 0, 0, 0, 0x0c, 0x97, 0xfe, 0x52]);
        d.extend([0x94, 0x80, 0, 0, 0x8c, 0x80, 0x8e, 0, 55, 0, 0, 0, 0xda, 0x96, 0xfe, 0x52, 3, 0, 0, 0]);
        d.extend([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
        d.extend([0xa4, 0x80, 0, 0, 0x8c, 0x80, 0x8e, 0, 30, 0, 0, 0, 0x1e, 0x96, 0xfe, 0x52, 0, 0, 0, 0]);
        d.extend([0u8; 12]);
        let recs = edge_records_to(&d, 0, d.len(), &HashSet::from([59])).unwrap();
        let reps = &recs[0].2;
        assert_eq!((reps[0].fid, reps[0].idx, reps[0].via), (55, 3, Some(59)));
        assert_eq!((reps[1].fid, reps[1].via), (30, None));
        // Unless the mirror is known, the record is not read so.
        assert!(edge_records_to(&d, 0, d.len(), &HashSet::new()).unwrap().iter().all(|r| r.2.len() != 2));
    }

    #[test]
    fn a_cap_or_a_side_of_an_extruded_profile_is_picked_with_its_outward_normal() {
        let (swept, sketches) = plate();
        let rep = |idx| Rep { tag: 9, fid: 32, idx, ext: false, ext2: false, via: None };
        let none = HashSet::new();
        // Face 1 (no side has that key): the far cap; face 0: the near one.
        let (p, n, _) = face_pick(&rep(1), &swept, &sketches, &none).unwrap();
        assert_eq!((p[2], n), (5.0, [0.0, 0.0, 1.0]));
        let (p, n, _) = face_pick(&rep(0), &swept, &sketches, &none).unwrap();
        assert_eq!((p[2], n), (0.0, [0.0, 0.0, -1.0]));
        // A side by its sketch key: the wall x = 20 faces +x, the wall y = 0 faces -y.
        let (p, n, region) = face_pick(&rep(3), &swept, &sketches, &none).unwrap();
        assert_eq!((p, n), ([20.0, 5.0, 2.5], [1.0, 0.0, 0.0]));
        assert!(on_face(&region, &sketches, [20.0, 7.0, 4.0]));
        assert!(!on_face(&region, &sketches, [20.0, 11.0, 4.0]));
        assert!(!on_face(&region, &sketches, [20.0, 7.0, 6.0]));
        let (_, n, _) = face_pick(&rep(2), &swept, &sketches, &none).unwrap();
        assert_eq!(n, [0.0, -1.0, 0.0]);
        // A cap's region is the profile; a key that is neither a cap nor a side picks nothing.
        let (_, _, region) = face_pick(&rep(1), &swept, &sketches, &none).unwrap();
        assert!(on_face(&region, &sketches, [10.0, 5.0, 5.0]));
        assert!(!on_face(&region, &sketches, [25.0, 5.0, 5.0]));
        assert!(face_pick(&rep(9), &swept, &sketches, &none).is_none());
    }

    #[test]
    fn an_edge_between_two_planes_is_found_without_a_shared_vertex() {
        // The wall x = 20 against the far cap z = 5, between the walls y = 0 and y = 10: the
        // middle of the 10 mm edge. (A record names such faces after the earlier feature when a
        // later one is coplanar with them.)
        let (swept, sketches) = plate();
        let caps = HashSet::from([9u16]);
        let side = |idx| Rep { tag: 4, fid: 32, idx, ext: false, ext2: false, via: None };
        let cap = Rep { tag: 9, fid: 32, idx: 1, ext: false, ext2: false, via: None };
        let faces: Vec<_> = [side(3), cap, side(2), side(4)]
            .iter()
            .map(|r| face_of(r, &swept, &HashMap::new(), &sketches, &caps))
            .collect();
        assert_eq!(plane_edge(&faces, &sketches), Some([20.0, 5.0, 5.0]));
        // Parallel faces make no edge.
        let faces: Vec<_> = [side(3), side(5), side(2), side(4)]
            .iter()
            .map(|r| face_of(r, &swept, &HashMap::new(), &sketches, &caps))
            .collect();
        assert_eq!(plane_edge(&faces, &sketches), None);
    }

    /// A line reference (Dual V-Wheel's Axis1, the z axis from z = 5.115 mm down): length,
    /// start and unit direction after the marker, 8 or 16 zero bytes on.
    #[test]
    fn a_line_reference_gives_the_axis() {
        for gap in [8usize, 16] {
            let mut d = vec![7u8; 5];
            d.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0x70, 0x17, 0, 0]);
            d.extend(vec![0u8; gap]);
            for v in [0.01023, 0.0, 0.0, 0.005115, 0.0, 0.0, -1.0] {
                d.extend(f64::to_le_bytes(v));
            }
            d.extend([0u8; 8]);
            let (o, dir) = line_ref(&d, 0, d.len()).unwrap();
            assert_eq!(o, [0.0, 0.0, 5.115]);
            assert_eq!(dir, [0.0, 0.0, -1.0]);
        }
        assert_eq!(line_ref(&[0u8; 100], 0, 100), None);
    }

    /// A point reference (V-Slot Rail_PIVOT's up-to-vertex end): the point after the marker and
    /// eight zero bytes, in mm.
    #[test]
    fn a_point_reference_gives_the_vertex() {
        let mut d = vec![7u8; 5];
        d.extend([0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0xf8, 0x2a, 0, 0]);
        d.extend([0u8; 8]);
        for v in [-0.0475, 0.02, 0.0] {
            d.extend(f64::to_le_bytes(v));
        }
        assert_eq!(point_ref(&d, 0, d.len()), Some([-47.5, 20.0, 0.0]));
        assert_eq!(point_ref(&d[..30], 0, 30), None);
    }

    /// Node names inside the header of the node before them: a reused sketch's `Sketch3<3>` and
    /// an attribute go; a hole wizard's nested sketch stays.
    #[test]
    fn nested_names_do_not_cut_a_feature_short() {
        let node = |start: usize, post: usize, cls: &str, name: &str| Node {
            start,
            cls: Some(cls.into()),
            tag: None,
            name: name.into(),
            flags: 0,
            id: 0,
            post,
            head_len: 0,
        };
        let mut nodes = vec![
            node(100, 300, "moExtrusion_c", "Boss-Extrude2"),
            node(160, 300, "moProfileFeature_c", "Sketch3<3>"),
            node(400, 600, "moProfileFeature_c", "Sketch4"),
            node(450, 600, "moAttribute_c", "Assoc1"),
            node(700, 900, "moHoleWzd_c", "Hole1"),
            node(760, 900, "moProfileFeature_c", "Sketch5"),
        ];
        drop_nested(&mut nodes);
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["Boss-Extrude2", "Sketch4", "Hole1", "Sketch5"]);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod pattern_tests {
    use super::*;

    fn seeds(ptrs: &[u32]) -> Option<patterns::Seeds> {
        Some(patterns::Seeds { ptrs: ptrs.to_vec(), bodies: 0 })
    }

    #[test]
    fn a_two_way_linear_pattern_repeats_the_first_direction_too() {
        let feats = [json!({"type": "extrude", "name": "Cut-Extrude1"})];
        let ints = |k: &str| match k {
            "D1" | "D2" => Some(2),
            _ => None,
        };
        let reals = |k: &str| match k {
            "D3" | "D4" => Some(0.02),
            _ => None,
        };
        let lines = [([0.0, 36.5, 1.6], [0.0, -1.0, 0.0]), ([3.5, 0.0, 1.6], [1.0, 0.0, 0.0])];
        let names = |p: u32| (p == 0x33d).then(|| "Cut-Extrude1".to_string());
        let out = feature_pattern("LPattern1", false, seeds(&[0x33d]), &lines, (ints, reals), names, &feats);
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0]["features"], json!(["Cut-Extrude1"]));
        assert_eq!(out[0]["direction"], json!([0.0, -1.0, 0.0]));
        assert_eq!(out[1]["name"], "LPattern1_2");
        assert_eq!(out[1]["features"], json!(["Cut-Extrude1", "LPattern1"]));
        assert_eq!(out[1]["spacing"], json!(20.0));
        // A seed the reader did not make is not patterned.
        let none = |_: u32| None;
        let out = feature_pattern("LPattern1", false, seeds(&[0x33d]), &lines, (ints, reals), none, &feats);
        assert_eq!(out[0]["unresolved"], json!(true));
    }

    #[test]
    fn a_node_declaring_its_class_is_the_object_after_it() {
        let node = |start: usize, cls: &str, tag: Option<u16>, id: u32| Node {
            start,
            cls: Some(cls.to_string()),
            tag,
            name: format!("n{id}"),
            flags: 0,
            id,
            post: start,
            head_len: 0,
        };
        // Furo4 declares moHoleWzd_c; Furo1 and Furo2 name it as class 709.
        let nodes = vec![
            node(0, "moHoleWzd_c", None, 188),
            node(10, "moHoleWzd_c", Some(709), 167),
            node(20, "moHoleWzd_c", Some(709), 174),
            node(30, "moLPattern_c", None, 266),
        ];
        let d = vec![0u8; 40];
        let span = |n: &Node| (n.start, n.start + 10);
        let index = node_indices(&d, &nodes, &span);
        assert_eq!(index.get(&710), Some(&0));
        // A class no other node names gives nothing.
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn a_selection_runs_on_while_records_follow_each_other() {
        // Records 162 bytes apart, each 14 bytes after the previous one's end, then one far off.
        let recs = vec![(361, 509, vec![]), (523, 669, vec![]), (683, 830, vec![]), (1773, 1900, vec![])];
        assert_eq!(selection(recs).len(), 3);
        // Short records keep the 0xa0 rule.
        assert_eq!(selection(vec![(0, 60, vec![]), (150, 210, vec![]), (400, 460, vec![])]).len(), 2);
    }

    #[test]
    fn a_circular_step_angle_becomes_the_total() {
        let feats = [json!({"type": "extrude", "name": "Air Hole"})];
        let names = |_: u32| Some("Air Hole".to_string());
        let lines = [([0.0, 2.8, 0.0], [0.0, 1.0, 0.0])];
        // Three at 120 degrees: a full turn; four at 30 degrees: 90 degrees from first to last.
        for (count, step, want) in [(3, TAU / 3.0, 360.0), (4, TAU / 12.0, 90.0)] {
            let ints = move |k: &str| (k == "D1").then_some(count);
            let reals = move |k: &str| (k == "D2").then_some(step);
            let out = feature_pattern("P", true, seeds(&[7]), &lines, (ints, reals), names, &feats);
            assert!((out[0]["angle"].as_f64().unwrap() - want).abs() < 1e-6, "{out:?}");
            assert_eq!(out[0]["axis"]["origin"], json!([0.0, 2.8, 0.0]));
        }
        // A pattern of bodies is not read.
        let body = Some(patterns::Seeds { ptrs: vec![], bodies: 1 });
        let out = feature_pattern("P", true, body, &lines, (|_: &str| Some(3), |_: &str| None), names, &feats);
        assert_eq!(out[0]["why"], "a pattern of bodies");
    }
}
