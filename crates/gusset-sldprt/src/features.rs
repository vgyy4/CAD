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
const SKIPPED: [&str; 43] = [
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
    // Round sw6: a configuration table (its configurations' values; the part's own are in its
    // features), weldment cut lists and their folders, cosmetic weld beads (graphics only) and
    // the folder of reference points.
    "moNativeConfigurationTableFeature_c",
    "moCosmeticWeldCutList_c",
    "moCosmeticWeldSubFolder_c",
    "moCosmeticWeldBead_c",
    "moRefPointFtrFolder_c",
    // Round sw10: a 3D sketch, curves only (500 Hex Hub's, which no feature uses; a sweep along
    // one finds no path and is refused).
    "mo3DProfileFeature_c",
    // Round sw12: a split line divides faces along projected curves and leaves the solid as it
    // is (the V-Slot extrusions' fixture lines); a feature that picks a face or edge it made
    // finds none and is refused.
    "moPLine_c",
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
/// The low 12 bits of a feature node's flags word that mark a cut (a boss has `0x140`). Newer
/// files add bit 17 (`0x2_01ca`); older ones write `0x1ca` alone (the French, Russian and Spanish
/// parts whose cuts are not named "Cut-...": 46 cuts and 44 localized ones over the corpora, and
/// no boss). Bit 2 may be set too (`0x2_01ce`, round sw7): the zsCAD parts' extrudes named
/// "拉伸" that cut their imported bodies (5bq's two remove exactly the 3200 mm3 where they
/// overlap the bodies); no boss has it.
const CUT_FLAGS: u32 = 0x1ca;
/// The bits of the flags word compared with [`CUT_FLAGS`].
const CUT_MASK: u32 = 0xffb;
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
    /// The end of the node's name (where its flags word starts, 4 bytes on).
    name_end: usize,
    /// Suppressed in the configuration read (round sw7, [`mark_suppressed`]).
    suppressed: bool,
    /// The archive index of the node's first child (an extrude's: its sketch), when the header
    /// refers to it rather than writing it there.
    child: Option<u16>,
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
        out.push(Node {
            start,
            cls,
            tag,
            name,
            flags,
            id,
            post,
            head_len: k - (e + 12).min(k),
            name_end: e,
            suppressed: false,
            child: child_ref(d, e),
        });
        if out.len() > MAX_NODES {
            return Err(format!("more than {MAX_NODES} feature nodes"));
        }
    }
    Ok(out)
}

/// A contour of a sketch (round sw7): a region of the sketch, as SolidWorks lists them for the
/// sketch (`listed`) and for a feature that extrudes only some of them (a contour chooser).
#[derive(Debug, Clone)]
struct Contour {
    at: usize,
    /// The sketch's node id (its segment references name it).
    fid: u32,
    listed: bool,
    /// The region's bounding box in model space: centre and diagonal (m).
    bbox: [f64; 4],
    /// The keys of the sketch segments around it.
    segs: Vec<u32>,
}

/// The declaration of the contour class.
const ONE_CONTOUR_DECL: &[u8] = b"\xff\xff\x01\x00\x0e\x00moOneContour_c";

/// The contours in the stream (`moOneContour_c`, by declaration or class word). Each starts
/// with its class word and a `u16` count of chains; 26 bytes on, the first chain writes the
/// sentinel pair, then references to the sketch's segments (`XX 8X 00 00 XX 8X ?? ??`, the sketch's
/// node id, a stamp, the segment's key), then a box object `01 00 TT 8X 01 00 00 00` with the
/// region's box centre and diagonal in model space (Power Supply base's four r 1.5 circles:
/// their centres and 3 sqrt 2 mm). A contour of the sketch's own list follows its `u32` key
/// (100000 on) right before its class word, or before the list's count before the declaration;
/// a chooser's contours do not. Contours whose references name no node (the sketch's own list in
/// some files writes longer references) are not returned.
fn contours(d: &[u8], ids: &HashSet<u32>) -> Vec<Contour> {
    let mut pair = [0u8; 16];
    pair[..8].copy_from_slice(&SENTINEL);
    pair[8..].copy_from_slice(&SENTINEL);
    let key = |at: usize| u32_at(d, at).is_some_and(|k| (100_000..200_000).contains(&k));
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(s) = find(d, &pair, i, d.len()) {
        i = s + 16;
        let decl = rfind(d, ONE_CONTOUR_DECL, s.saturating_sub(60), s);
        let Some(e) = decl.or_else(|| s.checked_sub(26)) else { continue };
        if decl.is_none() && !(d.get(e + 1).is_some_and(|b| b & 0x80 != 0) && u16_at(d, e + 2).is_some_and(|m| m > 0)) {
            continue;
        }
        // The box: the first box object after the chain with a plausible box.
        let mut bx = None;
        let mut k = s + 16;
        let to = (s + 6000).min(d.len());
        while let Some(q) = (k..to.saturating_sub(40)).find(|&q| {
            d.get(q..q + 2) == Some(&[1, 0][..])
                && d.get(q + 3).is_some_and(|b| b & 0x80 != 0)
                && d.get(q + 4..q + 8) == Some(&[1, 0, 0, 0][..])
        }) {
            k = q + 1;
            if let Some(v) = f64s_at::<4>(d, q + 8)
                && v.iter().all(|x| x.is_finite() && x.abs() < 10.0)
                && v[3] > 0.0
            {
                bx = Some((q, v));
                break;
            }
        }
        let Some((q, bbox)) = bx else { continue };
        // The sketch the chain's references name (one only).
        let mut fid = None;
        let mut mixed = false;
        let mut segs: Vec<u32> = Vec::new();
        let mut r = s + 16;
        while r + 20 <= q {
            let w = d.get(r..r + 8).unwrap_or_default();
            let refers = matches!(w, [_, h1, 0, 0, _, h2, _, _] if h1 & 0x80 != 0 && h2 & 0x80 != 0);
            if refers && let Some(f) = u32_at(d, r + 8).filter(|f| ids.contains(f)) {
                mixed |= fid.is_some_and(|x| x != f);
                fid = Some(f);
                // Key 0 names the sketch itself; a key is written once per chain end.
                if let Some(a) = u32_at(d, r + 16).filter(|&a| a != 0 && !segs.contains(&a) && segs.len() < 4096) {
                    segs.push(a);
                }
                r += 20;
                continue;
            }
            r += 1;
        }
        i = q + 40;
        let (Some(fid), false) = (fid, mixed) else { continue };
        let listed = key(e.saturating_sub(4)) || key(e.saturating_sub(6));
        out.push(Contour { at: e, fid, listed, bbox, segs });
    }
    out
}

/// The profiles of an extrude or revolve that uses only some contours of its sketch (round sw7):
/// the chooser's contours of sketch `fid` in `[lo, hi)` (the feature's span and the stretch since
/// the previous one: a chooser is written in the sketch's span before the feature, or in the
/// feature's own span after a nested sketch), whose box centres lie on the sketch's plane. Each
/// becomes a profile pick by its box (centre and diagonal, mm); a chooser that names as many
/// contours as the sketch lists is all of them (`None`).
fn chosen_profiles(
    cs: &[Contour],
    used: &mut HashSet<usize>,
    (fid, g, plane): (u32, &Geometry, &JsonPlane),
    lo: usize,
    hi: usize,
) -> Option<Value> {
    let n = cross(plane.x, plane.y);
    let on_plane = |c: &Contour| {
        let p = [c.bbox[0] * 1000.0, c.bbox[1] * 1000.0, c.bbox[2] * 1000.0];
        dot(sub(p, plane.origin), n).abs() < 1e-3
    };
    let chosen: Vec<&Contour> =
        cs.iter().filter(|c| !c.listed && c.fid == fid && (lo..hi).contains(&c.at) && !used.contains(&c.at)).collect();
    if chosen.is_empty() {
        return None;
    }
    used.extend(chosen.iter().map(|c| c.at));
    let listed = cs.iter().filter(|c| c.listed && c.fid == fid).count();
    // The chooser must fit the sketch as read: its segments drawn, its box within theirs (a
    // sketch whose stored contours belong to other dimensions keeps its whole profile).
    let fits = |c: &Contour| {
        let keys = &c.segs;
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for &k in keys {
            let Some(s) = g.segs.iter().find(|s| s.a == k && !s.construction) else { return false };
            for (u, v) in seg_uv(g, s).unwrap_or_default() {
                lo = [lo[0].min(u), lo[1].min(v)];
                hi = [hi[0].max(u), hi[1].max(v)];
            }
        }
        let span = libm::hypot(hi[0] - lo[0], hi[1] - lo[1]);
        span.is_finite() && c.bbox[3] <= span * (1.0 + 1e-6) + 1e-9
    };
    if (listed > 0 && chosen.len() >= listed) || !chosen.iter().all(|c| on_plane(c) && fits(c)) {
        return None;
    }
    let mm = |v: f64| round_to(v * 1000.0, 9);
    Some(Value::Array(
        chosen.iter().map(|c| json!({"bbox": [mm(c.bbox[0]), mm(c.bbox[1]), mm(c.bbox[2]), mm(c.bbox[3])]})).collect(),
    ))
}

/// The first child of a node, by archive index (round sw7): after the name, the flags, the id
/// and zeros (and the empty string newer files write), a `u16` count of children, each an index
/// or a nested object. An extrude's child is its sketch (back_shoulder_LT's Boss-Extrude2 refers
/// to 0x22c, before Cut-Extrude1's 0x3d2: it extrudes Sketch2, not the later Sketch3).
fn child_ref(d: &[u8], e: usize) -> Option<u16> {
    let c0 = if d.get(e + 16..e + 19) == Some(&UTF16_MARK[..]) { e + 26 } else { e + 22 };
    let n = u16_at(d, c0)?;
    let r = u16_at(d, c0 + 2)?;
    ((1..=8).contains(&n) && r != 0 && r & 0x8000 == 0).then_some(r)
}

/// The sketch an extrude or revolve uses (round sw7). Normally the last sketch read; a child
/// index already seen names the same sketch; an index below one seen names the one sketch not yet
/// used between the sketches of the nearest indices seen (gear box side1's Boss-Extrude5 uses
/// Sketch5, read before the Sketch10 that Cut-Extrude3 used).
fn feature_sketch(
    child: Option<u16>,
    last: &Option<String>,
    order: &[String],
    seen: &[(u16, String)],
    used: &HashSet<String>,
) -> Option<String> {
    let Some(r) = child else { return last.clone() };
    if let Some((_, s)) = seen.iter().find(|(k, _)| *k == r) {
        return Some(s.clone());
    }
    let pos = |name: &str| order.iter().position(|s| s == name);
    let Some(hi) = seen.iter().filter(|(k, _)| *k > r).min_by_key(|(k, _)| *k).and_then(|(_, s)| pos(s)) else {
        return last.clone();
    };
    let lo = seen.iter().filter(|(k, _)| *k < r).max_by_key(|(k, _)| *k).and_then(|(_, s)| pos(s));
    let from = lo.map_or(0, |p| p + 1);
    let free: Vec<&String> = order.get(from..hi).unwrap_or_default().iter().filter(|s| !used.contains(*s)).collect();
    match free.as_slice() {
        [one] => Some((*one).clone()),
        _ => last.clone(),
    }
}

/// The first node block (either version) in `d[from..to]`.
fn node_block(d: &[u8], from: usize, to: usize) -> Option<usize> {
    let hay = d.get(from..to.min(d.len()))?;
    hay.windows(NODE_BLOCK.len()).position(|w| w == NODE_BLOCK || w == NODE_BLOCK3).map(|i| from + i)
}

/// Round sw7: marks the nodes suppressed in the configuration whose stream is read. The `u32` 47
/// bytes before a node's own 18-byte block is 1 for a suppressed feature and 0 otherwise
/// (8020-1010 X 12's Cut-Extrude1: 1 in the eleven configurations whose body has no cut, 0 in
/// Config-12, whose body has it; 200mm 250mm Top Platform's Platform Pivot and Cut-Extrude8, whose
/// removed volume is exactly what the rebuild had too little of). A node that nests others (a cut
/// that reuses its sketch, a revolve with its cosmetic thread) shares its first block with them
/// and owns the block after theirs. The word is read only where the block follows the empty
/// string and the word after it is 0; anything else counts as not suppressed.
fn mark_suppressed(d: &[u8], nodes: &mut [Node]) {
    let keys: Vec<(usize, usize)> = nodes.iter().map(|n| (n.post, n.start)).collect();
    for n in nodes.iter_mut() {
        let nested = keys.iter().filter(|&&(p, s)| p == n.post && s > n.start).count();
        // The node's own block lies before the next node that does not nest in it.
        let to = keys.iter().filter(|&&(p, s)| p != n.post && s > n.start).map(|&(_, s)| s).min().unwrap_or(d.len());
        let mut at = n.name_end;
        let mut own = None;
        for _ in 0..=nested {
            own = node_block(d, at, to);
            match own {
                Some(k) => at = k + 1,
                None => break,
            }
        }
        let Some(k) = own.filter(|&k| k >= n.name_end + 47) else { continue };
        let word = u32_at(d, k - 47);
        let after = u32_at(d, k - 43);
        n.suppressed = word == Some(1) && after == Some(0) && d.get(k - 4..k - 1) == Some(&UTF16_MARK[..]);
    }
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

/// Whether a variable-radius fillet's radii (`D0`, `D01`, ...: one per vertex) are all one
/// positive value: then it is a constant fillet (OeMga's bottom/middle/top: eight of 1 mm),
/// which Gusset rebuilds; Gusset has no variable fillet for the others (round sw9).
fn constant_radius(ps: &[f64]) -> bool {
    ps.first().is_some_and(|r| *r > 0.0 && ps.iter().all(|v| (v - r).abs() <= 1e-12 * r))
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

/// The value (m) of the dimension `dim` of `feature` that an equation drives: the configuration
/// stream writes the equation (a CString `"D1@Boss-Extrude1"= "rod_length_xaxis"`) and then,
/// within a few hundred bytes, the dimension's own parameter object, named `dim` and laid out
/// as in the feature stream (ThreadedRod's Boss-Extrude1: 369.52 mm; the feature node keeps no
/// `D1`).
fn equation_dimension(c: &[u8], feature: &str, dim: &str) -> Option<f64> {
    let key: Vec<u8> = format!("\"{dim}@{feature}\"").encode_utf16().flat_map(u16::to_le_bytes).collect();
    let want: Vec<u8> = dim.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut from = 0;
    while let Some(i) = find(c, &key, from, c.len()) {
        from = i + 1;
        // The equation is the CString that starts with the key and assigns it.
        let Some(s) = i.checked_sub(4).filter(|&s| c.get(s..s + 3) == Some(&UTF16_MARK[..])) else { continue };
        let n = usize::from(*c.get(s + 3)?);
        let text = utf16(c.get(s + 4..s + 4 + 2 * n)?);
        if !text.contains('=') {
            continue;
        }
        let mut at = s + 4 + 2 * n;
        let end = (at + 1000).min(c.len());
        while let Some(j) = find(c, &UTF16_MARK, at, end) {
            at = j + 1;
            let len = usize::from(*c.get(j + 3)?);
            if 2 * len != want.len() || c.get(j + 4..j + 4 + 2 * len) != Some(&want[..]) {
                continue;
            }
            let q = j + 4 + 2 * len;
            let head = c.get(q..q + 16)?;
            let head_ok = head.get(..4) == Some(&[0, 0, 0, 0][..])
                && head.get(5..7) == Some(&[0, 0][..])
                && matches!(head.get(7), Some(0x40 | 0xc0))
                && head.get(8..16) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0][..]);
            let vend = if c.get(q + 16..q + 20) == Some(&PARAM_CSTRING[..]) { q + 22 } else { q + 18 };
            if !head_ok || c.get(vend - 2..vend) != Some(&[0, 0][..]) {
                continue;
            }
            return f64_at(c, vend).filter(|v| v.is_finite() && *v != 0.0 && v.abs() < 1e3);
        }
    }
    None
}

#[derive(Debug, Clone, Copy)]
struct EndSpec {
    cls: Option<u16>,
    x: u32,
    kind: u32,
    flag: u32,
    /// Byte 12 of the head (`u32` 0/1).
    b12: u8,
    /// Byte 8 of the head (`u32` 0/1): a cut that keeps what lies inside its profile and removes
    /// the rest of the sweep ("flip side to cut"): BK12_nut's Cut-Extrude1, a circle of 22 mm
    /// through the hexagon, trims its corners; SlotMount-Attachment, the Key Fob parts' Top Cut.
    flip: bool,
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
            // Bytes 8 and 12 are `u32` flags 0/1: the flipped side of a cut and the reversed
            // direction ([`EndSpec`]).
            && w.iter().enumerate().skip(5).all(|(k, &c)| c == 0 || (matches!(k, 8 | 12) && c == 1));
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
        let flip = *need(d.get(p + 8), "an end condition")? == 1;
        out.push(EndSpec { cls: Some(t & 0x7fff), x, kind, flag, b12, flip });
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
    let flip = *need(d.get(p + 8), "an end condition")? == 1;
    Ok(Some(EndSpec { cls: None, x, kind, flag, b12, flip }))
}

/// The end condition of an extrude whose direction is given by a reference (a
/// `moDirectionSpec_c` holding a `moPlaneRef_w`: the sweep runs along the plane's normal).
/// The direction object takes the place of the end spec's leading `u16 0`, so the usual fields
/// (`u32 X, 10 x 00, u32 type, u32 flag`) follow the plane reference, after its closing
/// `u32 3` (Bottom-Plate's Cut-Extrude3: up to a surface; V-Slot Rail_PIVOT's Cut-Extrude5: up
/// to a vertex). The direction itself is not read: on the samples it is the sketch normal.
fn endspec_directed(d: &[u8], a: usize, b: usize) -> Option<EndSpec> {
    let i = find(d, b"\x11\x00moDirectionSpec_c", a, b)?;
    find(d, b"moPlaneRef_w", i, (i + 64).min(b))?;
    let mut p = i + 19;
    while p + 26 <= b.min(i + 1200) {
        let w = d.get(p..p + 26)?;
        let ok = w.get(0..4) == Some(&[3, 0, 0, 0][..])
            && matches!(w.get(4), Some(0 | 1))
            && w.get(5..8) == Some(&[0, 0, 0][..])
            && w.iter().enumerate().take(18).skip(8).all(|(k, &c)| c == 0 || (matches!(k, 8 | 12) && c == 1))
            && w.get(18..20).is_some_and(|t| t.first().is_some_and(|&t| t <= 9) && t.get(1) == Some(&0))
            && w.get(20..22) == Some(&[0, 0][..])
            && matches!(w.get(22), Some(0 | 1))
            && w.get(23..26) == Some(&[0, 0, 0][..]);
        if ok {
            // Laid out as [`endspec_candidates`] reads it, the `u32 3` in place of the tag.
            return Some(EndSpec {
                cls: None,
                x: u32_at(d, p + 4)?,
                kind: u32_at(d, p + 18)?,
                flag: u32_at(d, p + 22)?,
                b12: *d.get(p + 12)?,
                flip: *d.get(p + 8)? == 1,
            });
        }
        p += 1;
    }
    None
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
    // (adpator, a chunk-container part of an intermediate version) do not, and some version 1
    // headers do too (the Hecatron OpenBuilds parts: Aluminum Spacer's Sketch1). A payload never
    // starts with 8 zero bytes (a point starts with its type 0x1e/0x1f, a segment with two
    // different point indices, a circle with its point twice and the word 1), so the bytes say
    // which.
    let after = |q: usize| if d.get(q..q + 8).is_some_and(|w| w.iter().all(|&b| b == 0)) { q + 8 } else { q };
    let ext = u16_at(d, h + 35)?;
    if ext == 0 {
        let p = h + 40;
        if d.get(p..p + 16) == Some(&ONE[..]) {
            return Some((Some(after(p + 16)), false));
        }
        // A projected point without an object chain marks itself with a byte (1 or 3) where the
        // 0.0 would be (round sw12: the 126p rods' groove circles and mid_elbow's hole are
        // centred on one; the search below found the next entity's payload, so the centre took
        // its start point's place).
        let marked = d.get(p..p + 8).is_some_and(|w| {
            w.first() == Some(&0) && w.get(1).is_some_and(|&b| b != 0) && w.iter().skip(2).all(|&b| b == 0)
        });
        // A converted segment marks itself the same way (Back_sup_low's base line, byte 4): its
        // payload is two point indices, the word 1 or 0 and -1.0.
        let q = after(p + 16);
        let point = matches!(u16_at(d, q), Some(0x1e | 0x1f));
        let segment = u16_at(d, q + 4).is_some_and(|w| w <= 1) && f64_at(d, q + 8).is_some_and(|m| is(m, -1.0));
        if marked && d.get(p + 8..p + 16) == ONE.get(8..) && (point || segment) {
            return Some((Some(q), true));
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

/// Whether a side of an extrude of sketch `g` (on `jp`) would lie on a side of an earlier
/// extrude along the same axis: a line on the plane of one of its lines, or an arc on the
/// cylinder of one of its arcs.
fn continues_sides<'a>(
    g: &Geometry,
    jp: JsonPlane,
    (dist, symmetric): (f64, bool),
    mut earlier: impl Iterator<Item = &'a Swept>,
    sketches: &[Geometry],
) -> bool {
    let n = cross(jp.x, jp.y);
    let model = |q: Pt| -> V3 { comb(comb(jp.origin, 1.0, jp.x, q.x * 1000.0), 1.0, jp.y, q.y * 1000.0) };
    let len = |v: V3| dot(v, v).sqrt();
    let ours: Vec<(&Seg, V3, V3, Option<V3>)> = g
        .segs
        .iter()
        .filter(|s| !s.construction && !s.centerline && s.kind != SegKind::Ellipse)
        .filter_map(|s| {
            let c = (s.kind == SegKind::Arc).then(|| point(&g.pts, s.c).map(model)).flatten();
            Some((s, model(point(&g.pts, s.s)?), model(point(&g.pts, s.e)?), c))
        })
        .collect();
    earlier.any(|f| {
        let nf = normal(f);
        if len(cross(nf, n)) > 1e-9 {
            return false;
        }
        // The two must meet along the axis (heights along `nf` from `f`'s sketch plane).
        let span = |o: f64, d: f64, sym: bool| {
            if sym { (o - d.abs() / 2.0, o + d.abs() / 2.0) } else { (o + d.min(0.0), o + d.max(0.0)) }
        };
        let (f0, f1) = if f.through { (f64::NEG_INFINITY, f64::INFINITY) } else { span(0.0, f.distance, f.symmetric) };
        let (g0, g1) = span(dot(sub(jp.origin, f.plane.origin), nf), dist * dot(n, nf), symmetric);
        if g0 > f1 + 1e-6 || f0 > g1 + 1e-6 {
            return false;
        }
        let Some(gf) = sketches.get(f.sketch) else { return false };
        gf.segs.iter().filter(|t| !t.construction && !t.centerline).any(|t| {
            let at = |i: usize| point(&gf.pts, i).map(|q| lift(f, q.x * 1000.0, q.y * 1000.0, 0.0));
            let (Some(a), Some(b)) = (at(t.s), at(t.e)) else { return false };
            ours.iter().any(|(s, p, q, c)| match (t.kind, s.kind) {
                (SegKind::Line, SegKind::Line) => {
                    let m = cross(sub(b, a), nf);
                    let lm = len(m);
                    lm > 1e-9 && (dot(sub(*p, a), m) / lm).abs() < 1e-6 && (dot(sub(*q, a), m) / lm).abs() < 1e-6
                }
                (SegKind::Arc, SegKind::Arc) => {
                    let (Some(cf), Some(c)) = (at(t.c), c) else { return false };
                    len(cross(sub(*c, cf), nf)) < 1e-6 && (len(sub(*p, *c)) - len(sub(a, cf))).abs() < 1e-6
                }
                _ => false,
            })
        })
    })
}

/// An extrude's draft angle (rad): its `D3`, an angle of whole half degrees under 90 whose
/// dimension is placed (the two `f64` of its display position, 31 bytes after the value, are not
/// both zero: Toolbox washers keep an unused 1 degree `D3` and `D4` with no position).
fn extrude_draft(d: &[u8], slots: &[(String, usize)]) -> Option<f64> {
    let &(_, at) = slots.iter().find(|(k, _)| k == "D3")?;
    let v = f64_at(d, at)?;
    let half_degrees = v.to_degrees() * 2.0;
    let placed = f64s_at::<2>(d, at + 31).is_some_and(|w| w.iter().any(|x| x.is_finite() && *x != 0.0));
    (v > 1e-6 && v < TAU / 4.0 && (half_degrees - half_degrees.round()).abs() < 1e-6 && placed).then_some(v)
}

/// The centre of the first box object of a span (`01 00 TT 8X 01 00 00 00`, then the centre and
/// the diagonal, m, as a contour's box; `moBBoxCenterData_c`), in mm.
fn revolve_box_centre(d: &[u8], a: usize, b: usize) -> Option<V3> {
    (a..b.min(d.len()).saturating_sub(40)).find_map(|q| {
        let head = d.get(q..q + 8)?;
        if head.get(..2) != Some(&[1, 0][..])
            || head.get(3).is_none_or(|b| b & 0x80 == 0)
            || head.get(4..) != Some(&[1, 0, 0, 0][..])
        {
            return None;
        }
        let v = f64s_at::<4>(d, q + 8)?;
        // Older files write -1 for the diagonal (8020's T-nut).
        (v.iter().all(|x| x.is_finite() && x.abs() < 10.0) && (v[3] > 0.0 || is(v[3], -1.0)))
            .then(|| [v[0], v[1], v[2]].map(mm))
    })
}

/// The axis pieces that close a revolve's open profile: SolidWorks closes a profile whose
/// loose ends lie on the axis line along the axis (Tommy Bar: a bar and its ball end, drawn
/// from the axis and back to it, with a line from the ball's rim to the axis between them).
/// Pairs of point indices, consecutive along the axis; empty when fewer than two loose ends lie
/// on the axis.
fn axis_closure(g: &Geometry, k: usize) -> Vec<(usize, usize)> {
    let Some(ax) = g.segs.get(k) else { return Vec::new() };
    let (Some(a), Some(b)) = (point(&g.pts, ax.s), point(&g.pts, ax.e)) else { return Vec::new() };
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = libm::hypot(dx, dy);
    if len < 1e-9 {
        return Vec::new();
    }
    let ends: Vec<(usize, Pt)> = g
        .segs
        .iter()
        .filter(|s| !s.construction && !s.centerline && s.s != s.e)
        .flat_map(|s| [s.s, s.e])
        .filter_map(|i| point(&g.pts, i).map(|p| (i, p)))
        .collect();
    let same = |p: &Pt, q: &Pt| libm::hypot(p.x - q.x, p.y - q.y) < 1e-9;
    let mut loose: Vec<(f64, usize)> = Vec::new();
    for (n, (i, p)) in ends.iter().enumerate() {
        let first = ends.iter().position(|(_, q)| same(p, q)) == Some(n);
        let count = ends.iter().filter(|(_, q)| same(p, q)).count();
        let off = (dx * (p.y - a.y) - dy * (p.x - a.x)) / len;
        if first && count % 2 == 1 && off.abs() < 1e-9 {
            loose.push((((p.x - a.x) * dx + (p.y - a.y) * dy) / len, *i));
        }
    }
    if loose.len() < 2 {
        return Vec::new();
    }
    loose.sort_by(|x, y| x.0.total_cmp(&y.0));
    loose
        .windows(2)
        .filter_map(|w| match w {
            [(_, p), (_, q)] => Some((*p, *q)),
            _ => None,
        })
        .collect()
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
    // A shared piece whose ends would both be left open without it is no wall between two
    // loops but a line drawn twice in one loop (shaft_stage_1's keyway: a side line and a
    // second line over half of it): one copy of it stays. Lines that lead nowhere (a projected
    // edge running past a profile) are pruned first, so they keep no end closed.
    let shared = |key: &Pair| owners.get(key).is_some_and(|o| o.len() > 1);
    let mut edges: Vec<Pair> = Vec::new();
    let mut known: std::collections::HashSet<Pair> = std::collections::HashSet::new();
    for (c, piece, orig) in &pieces {
        let e = match (piece, orig) {
            (Some((u, v)), Some(_)) => pair(u, v),
            _ if profile(c) && matches!(c.get("type").and_then(Value::as_str), Some("line" | "arc")) => {
                let (Some(u), Some(v)) = (str_of(c, "start"), str_of(c, "end")) else { continue };
                (u, v)
            }
            _ => continue,
        };
        if known.insert(e.clone()) {
            edges.push(e);
        }
    }
    // Prune leaves: a queue of ends met once.
    let mut alive = vec![true; edges.len()];
    let mut at_vertex: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, (u, v)) in edges.iter().enumerate() {
        at_vertex.entry(u).or_default().push(i);
        if u != v {
            at_vertex.entry(v).or_default().push(i);
        }
    }
    let mut deg: HashMap<&str, usize> = at_vertex.iter().map(|(k, l)| (*k, l.len())).collect();
    let mut queue: Vec<&str> = deg.iter().filter(|(_, d)| **d == 1).map(|(k, _)| *k).collect();
    while let Some(x) = queue.pop() {
        let Some(list) = at_vertex.get(x) else { continue };
        for &i in list {
            let (Some(a), Some((u, v))) = (alive.get_mut(i), edges.get(i)) else { continue };
            if !*a {
                continue;
            }
            *a = false;
            for y in [u.as_str(), v.as_str()] {
                if let Some(d) = deg.get_mut(y) {
                    *d = d.saturating_sub(1);
                    if *d == 1 {
                        queue.push(y);
                    }
                }
            }
        }
    }
    let mut degree: HashMap<String, usize> = HashMap::new();
    for ((u, v), _) in edges.iter().zip(&alive).filter(|(e, a)| **a && !shared(e)) {
        *degree.entry(u.clone()).or_default() += 1;
        *degree.entry(v.clone()).or_default() += 1;
    }
    // Restored along runs of shared pieces from an open end: each piece from an open end to
    // another open end or to a point only shared pieces meet (the run goes on from there).
    let mut restore: std::collections::HashSet<Pair> = std::collections::HashSet::new();
    let runs: Vec<&Pair> = edges.iter().zip(&alive).filter(|(e, a)| **a && shared(e)).map(|(e, _)| e).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (u, v) in &runs {
            let d = |e: &String| degree.get(e).copied().unwrap_or(0);
            let open = |e: &String| d(e) % 2 == 1;
            if (open(u) && (open(v) || d(v) == 0) || open(v) && d(u) == 0) && restore.insert(pair(u, v)) {
                for e in [u, v] {
                    *degree.entry(e.clone()).or_default() += 1;
                }
                changed = true;
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
        if (shared(&key) && !restore.contains(&key)) || !seen.insert(key.clone()) {
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
    /// A face of a pattern's copy (`via` the pattern): the copy's number, 1 the first after the seed.
    copy: u32,
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
    Ok(edge_records_to(d, a, b, &HashSet::new(), &HashSet::new())?.into_iter().map(|(s, _, r)| (s, r)).collect())
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
    patterns: &HashSet<u32>,
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
            if (mirrors.contains(&outer) || patterns.contains(&outer))
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
            // A face of a pattern's copy: the seed's side words, then the copy's number.
            let mut copy = 0;
            if via.is_some_and(|v| patterns.contains(&v)) {
                match u32_at(d, p) {
                    Some(c @ 1..=4096) if extra == 2 => {
                        copy = c;
                        p += 4;
                    }
                    _ => break,
                }
            }
            reps.push(Rep { tag: t & 0x7fff, fid, idx, ext: extra == 1 && !last, ext2, via, copy });
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
///
/// Records that name a face of the feature itself (`own`, its node id) describe the faces it
/// made (each blend face with its neighbours), not picked edges; newer files write them before
/// the picks (Power Supply Unit's Fillet1: four such records, then its four corner edges). The
/// run starts at the first record without one, where there is such a record, else as before.
fn selection(recs: Vec<(usize, usize, Vec<Rep>)>, own: u32) -> Vec<(usize, Vec<Rep>)> {
    let mentions_own = |reps: &[Rep]| reps.iter().any(|r| r.fid == own);
    let from = recs.iter().position(|(_, _, reps)| !mentions_own(reps)).filter(|&k| k > 0).unwrap_or(0);
    let mut run: Vec<(usize, Vec<Rep>)> = Vec::new();
    let mut last_end = 0usize;
    for (start, end, reps) in recs.into_iter().skip(from) {
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

/// The point a feature names by a point reference (`moEndPointRef_w`, an end of an edge, or
/// `moPointRef_w`): `c7 cf ff ff c7 cf ff ff 00 00 00 00`, a `u32` version word, then the point
/// in model coordinates (m). Words 9000 and 11000 (r3 iso, V-Slot Rail_PIVOT) put eight zero
/// bytes before the point; 6000 (V-Slot Actuator End Mount) none. Returns it in mm.
fn point_ref(d: &[u8], a: usize, b: usize) -> Option<V3> {
    const MARK: [u8; 12] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0];
    // The last one: a direction given by a plane reference writes the same marker before it
    // (V-Slot Rail_PIVOT's Cut-Extrude5).
    let mut from = a;
    let mut last = None;
    while let Some(i) = find(d, &MARK, from, b) {
        from = i + 1;
        let at = match u32_at(d, i + 12) {
            Some(6000) => i + 16,
            Some(9000 | 11000) => i + 24,
            _ => continue,
        };
        if let Some(p) = f64s_at::<3>(d, at).filter(|p| p.iter().all(|c| c.is_finite() && c.abs() < 100.0)) {
            last = Some(p.map(mm));
        }
    }
    last
}

/// The plane a feature names by a plane reference (`moPlaneRef_w`: a reference plane or a
/// planar face) when it has no face reference: the last `c7 cf ff ff c7 cf ff ff 00 00 00 00`
/// with a `u32` version word in the span, then (after eight zero bytes for words 9000 and
/// 11000) four extents, the plane's origin and its unit normal, in model coordinates (m).
/// end_cap's Enlèv. mat.-Extru.1 ends on the Top plane (origin 0, normal +y). Returns the
/// origin in mm.
fn plane_ref(d: &[u8], a: usize, b: usize) -> Option<(V3, V3)> {
    const MARK: [u8; 12] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0];
    let mut from = a;
    let mut last = None;
    while let Some(i) = find(d, &MARK, from, b) {
        from = i + 1;
        let at = match u32_at(d, i + 12) {
            Some(6000) => i + 16,
            Some(9000 | 11000) if f64_at(d, i + 16) == Some(0.0) => i + 24,
            _ => continue,
        };
        let (Some(o), Some(n)) = (f64s_at::<3>(d, at + 32), f64s_at::<3>(d, at + 56)) else { continue };
        let finite = o.iter().chain(&n).all(|c| c.is_finite() && c.abs() < 100.0);
        if finite && (libm::sqrt(dot(n, n)) - 1.0).abs() < 1e-9 {
            last = Some((o.map(mm), n.map(|c| round_to(c, 12) + 0.0)));
        }
    }
    last
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
        let (m, path) = revolve_circle(reps, revolved, sketches, None)?;
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
///
/// `ends`: the planes of the record's two vertex faces, when they are faces of other features
/// that cut the circle into pieces (round sw12: the nut's flats cut its rims in two). The edge
/// is then the piece that runs from the first plane to the second with the first face on its
/// left (as for two meeting points of extrude sides: the cross of the outward normals points
/// along the run), the shortest such run; its midpoint and its own path.
fn revolve_circle(
    reps: &[Rep],
    revolved: &HashMap<u32, Revolved>,
    sketches: &[Geometry],
    ends: Option<[(V3, V3); 2]>,
) -> Option<(V3, Vec<V3>)> {
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
    let Some([(o1, n1), (o2, n2)]) = ends else {
        let path = (0..=steps).map(|k| at(TAU * f64::from(k) / f64::from(steps))).collect();
        return Some((at(TAU / 2.0), path));
    };
    // The run's sense about the axis: the outward normals at the profile point.
    let p = lift(&rv.on, v.x * 1000.0, v.y * 1000.0, 0.0);
    let (na, nb) = (side_out(&rv.on, g, &sa, p)?, side_out(&rv.on, g, &sb, p)?);
    let turn = dot(cross(na, nb), e2);
    if turn.abs() < 1e-9 {
        return None;
    }
    let sense = turn.signum();
    // Where a plane cuts the circle: cos(t) a + sin(t) b = k.
    let cuts = |o: V3, n: V3| -> Vec<f64> {
        let (a, b, k) = (dot(r0v, n), dot(e2, n), dot(sub(o, c), n));
        let r = libm::hypot(a, b);
        if r < 1e-9 || k.abs() > r {
            return Vec::new();
        }
        let (phi, d) = (libm::atan2(b, a), libm::acos(k / r));
        vec![phi - d, phi + d]
    };
    let mut best: Option<(f64, f64)> = None;
    for t1 in cuts(o1, n1) {
        for t2 in cuts(o2, n2) {
            let span = (sense * (t2 - t1)).rem_euclid(TAU);
            if span > 1e-9 && best.is_none_or(|(_, s)| span < s) {
                best = Some((t1, span));
            }
        }
    }
    let (t1, span) = best?;
    let path = (0..=steps).map(|k| at(t1 + sense * span * f64::from(k) / f64::from(steps))).collect();
    Some((at(t1 + sense * span / 2.0), path))
}

/// A solid feature an edge can name: its sketch, plane and signed depth (mm).
#[derive(Debug, Clone, Copy)]
struct Swept {
    sketch: usize,
    plane: JsonPlane,
    distance: f64,
    symmetric: bool,
    through: bool,
    /// A cut: its sides face into the profile.
    cut: bool,
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
    Some(Rep {
        tag: t & 0x7fff,
        fid: u32_at(d, c + 8)?,
        idx: u32_at(d, c + 16)?,
        ext: false,
        ext2: false,
        via: None,
        copy: 0,
    })
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
    /// A curved side (an arc's cylinder): whether it covers the sweep is left to the kernel.
    Curved,
}

/// Whether the point `q` (mm) of the face's plane lies on the face, up to 0.01 mm.
fn on_face(region: &Region, sketches: &[Geometry], q: V3) -> bool {
    let (Region::Cap(f) | Region::Wall(f, ..)) = region else { return true };
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
        Region::Curved => true,
    }
}

fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 < 1e-24 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
    libm::hypot(p.0 - (a.0 + t * dx), p.1 - (a.1 + t * dy))
}

/// The face reference of a draft's neutral plane: the first reference after the
/// `moPlaneRef_w` class name and the two sentinels that close its record header.
fn neutral_face_ref(span: &[u8]) -> Option<Rep> {
    let at = span.windows(12).position(|w| w == b"moPlaneRef_w")?;
    let tail = span.get(at..(at + 200).min(span.len()))?;
    let s = tail.windows(16).position(|w| w.get(..8) == Some(&SENTINEL[..]) && w.get(8..) == Some(&SENTINEL[..]))?;
    let p = at + s + 18;
    let t = u16_at(span, p)?;
    if t & 0x8000 == 0 || u16_at(span, p + 2)? != 0 {
        return None;
    }
    Some(Rep {
        tag: t & 0x7fff,
        fid: u32_at(span, p + 8)?,
        idx: u32_at(span, p + 16)?,
        ext: false,
        ext2: false,
        via: None,
        copy: 0,
    })
}

/// The points (mm) a draft span ends with: `u32 n`, then `n` points (`f64` x, y, z in metres),
/// then up to 16 zero bytes. One lies on each face the draft tapers, pattern copies included.
fn draft_points(span: &[u8]) -> Vec<V3> {
    for tail in 0..=16usize {
        let Some(end) = span.len().checked_sub(tail) else { break };
        if span.get(end..).is_some_and(|t| t.iter().any(|&c| c != 0)) {
            break;
        }
        for n in 1..=512usize {
            let Some(k) = end.checked_sub(4 + 24 * n) else { break };
            if u32_at(span, k).map(|v| v as usize) != Some(n) {
                continue;
            }
            let pts: Vec<V3> = (0..n)
                .filter_map(|i| {
                    let q = k + 4 + 24 * i;
                    Some([f64_at(span, q)?, f64_at(span, q + 8)?, f64_at(span, q + 16)?])
                })
                .collect();
            if pts.len() == n && pts.iter().flatten().all(|v| v.is_finite() && v.abs() < 10.0) {
                return pts.into_iter().map(|p| p.map(|v| round_to(v * 1000.0, 6) + 0.0)).collect();
            }
        }
    }
    Vec::new()
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
            .fold(None::<Side>, |b, x| if b.is_none_or(|b| x.2 > b.2) { Some(x) } else { b });
        let (u, v) = match longest {
            Some((mid, out, len)) => {
                let e = 1e-6 + 0.01 * len;
                (mid.0 - out.0 * e, mid.1 - out.1 * e)
            }
            // A profile of arcs only (RobotCap's disc): a point just inside the middle of its
            // largest arc.
            None => arc_inside(g)?,
        };
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
    // The side an arc sweeps (housing's Boss-Extrude4 ends on Boss-Extrude2's cylinder): the
    // middle of the arc at mid-height, its normal out of the material.
    if s.kind == SegKind::Arc {
        let pts = seg_uv(g, s)?;
        let &(u, v) = pts.get(pts.len() / 2)?;
        let p = lift(f, u * 1000.0, v * 1000.0, f.distance / 2.0);
        return Some((p, unit(side_out(f, g, s, p)?), Region::Curved));
    }
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

/// The plane of a cap (a point on it, mm, and its outward normal), without a point inside the
/// face: for an end face [`face_pick`] cannot place.
fn cap_plane(r: &Rep, swept: &HashMap<u32, Swept>, sketches: &[Geometry], cap_tags: &HashSet<u16>) -> Option<(V3, V3)> {
    let f = swept.get(&r.fid).filter(|f| !f.through && f.distance.abs() > 1e-9)?;
    let g = sketches.get(f.sketch)?;
    let cap = cap_tags.contains(&r.tag) || !g.segs.iter().any(|s| s.a == r.idx && !s.construction);
    if !cap || r.idx > 1 {
        return None;
    }
    let sweep = if f.distance > 0.0 { 1.0 } else { -1.0 };
    let o = if r.idx == 1 { sweep } else { -sweep };
    Some((lift(f, 0.0, 0.0, cap_height(f, r.idx)), normal(f).map(|c| round_to(c * o, 12) + 0.0)))
}

/// The plane of a straight side of an extruded feature, through ones too (a point on it, mm,
/// and a unit normal of either sense): for an end face [`face_pick`] cannot place.
fn side_plane(r: &Rep, swept: &HashMap<u32, Swept>, sketches: &[Geometry]) -> Option<(V3, V3)> {
    let f = swept.get(&r.fid)?;
    let g = sketches.get(f.sketch)?;
    let mut c = g.segs.iter().filter(|s| s.a == r.idx && !s.construction);
    let (Some(s), None) = (c.next(), c.next()) else { return None };
    if s.kind != SegKind::Line {
        return None;
    }
    let (p0, p1) = (point(&g.pts, s.s)?, point(&g.pts, s.e)?);
    let o = lift(f, p0.x * 1000.0, p0.y * 1000.0, 0.0);
    let q = lift(f, p1.x * 1000.0, p1.y * 1000.0, 0.0);
    let c = cross(sub(q, o), normal(f));
    let l = libm::sqrt(dot(c, c));
    (l > 1e-9).then(|| (o, c.map(|v| v / l)))
}

/// A point of a profile's inside next to the middle of its largest arc (m), when the profile
/// has one; either side of the arc may be the inside.
fn arc_inside(g: &Geometry) -> Option<(f64, f64)> {
    type Uv = (f64, f64);
    // Radius, the arc's middle sample, its outward direction.
    let mut best: Option<(f64, Uv, Uv)> = None;
    for s in g.segs.iter().filter(|s| !s.construction && s.kind == SegKind::Arc) {
        let (Some(c), Some(p)) = (point(&g.pts, s.c), point(&g.pts, s.s)) else { continue };
        let r = libm::hypot(p.x - c.x, p.y - c.y);
        let Some(pts) = seg_uv(g, s) else { continue };
        let Some(&m) = pts.get(pts.len() / 2) else { continue };
        if r > 1e-9 && best.is_none_or(|b| r > b.0) {
            best = Some((r, m, ((m.0 - c.x) / r, (m.1 - c.y) / r)));
        }
    }
    let (r, m, out) = best?;
    let e = 1e-6 + 0.01 * r;
    [-1.0, 1.0].into_iter().map(|k| (m.0 + k * out.0 * e, m.1 + k * out.1 * e)).find(|&(u, v)| inside(g, u, v))
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

/// A point `(u, v)` of a sketch (m) on the segment `s`, to 1e-7 m: on a line, or at the arc's
/// radius within its sweep.
fn on_seg(g: &Geometry, s: &Seg, u: f64, v: f64) -> bool {
    let Some(pts) = seg_uv(g, s) else { return false };
    if s.kind == SegKind::Line {
        return matches!(pts.as_slice(), [a, b] if dist_to_segment((u, v), *a, *b) < 1e-7);
    }
    let (Some(c), Some(p)) = (point(&g.pts, s.c), point(&g.pts, s.s)) else { return false };
    let r = libm::hypot(p.x - c.x, p.y - c.y);
    let near = pts.windows(2).any(|w| matches!(w, [a, b] if dist_to_segment((u, v), *a, *b) < 0.01 * r));
    (libm::hypot(u - c.x, v - c.y) - r).abs() < 1e-7 && near
}

/// The sketch coordinates (m) of a model point (mm) in a feature's sketch plane.
fn uv_of(f: &Swept, p: V3) -> (f64, f64) {
    let d = sub(p, f.plane.origin);
    (dot(d, f.plane.x) / 1000.0, dot(d, f.plane.y) / 1000.0)
}

/// Where two profile curves of parallel extrusions meet without crossing: the ends of each that
/// lie on the other (model points in the plane of the first's sketch, mm).
fn touches(a: (&Swept, &Geometry, &Seg), b: (&Swept, &Geometry, &Seg)) -> Vec<V3> {
    let mut out = Vec::new();
    for ((f, g, s), (of, og, os)) in [(a, b), (b, a)] {
        for i in [s.s, s.e] {
            let Some(p) = point(&g.pts, i) else { continue };
            let p3 = lift(f, p.x * 1000.0, p.y * 1000.0, 0.0);
            let (u, v) = uv_of(of, p3);
            if on_seg(og, os, u, v) {
                // Onto the first one's sketch plane, along the extrusion.
                let n = normal(a.0);
                let h = dot(sub(p3, a.0.plane.origin), n);
                out.push(comb(p3, 1.0, n, -h));
            }
        }
    }
    out
}

/// The outward normal of a side face at a model point on it (mm): out of the profile for a
/// boss, into it for a cut. In the sketch plane, unit.
fn side_out(f: &Swept, g: &Geometry, s: &Seg, at: V3) -> Option<V3> {
    let (u, v) = uv_of(f, at);
    let n2 = match s.kind {
        SegKind::Line => {
            let (p, q) = (point(&g.pts, s.s)?, point(&g.pts, s.e)?);
            let l = libm::hypot(q.x - p.x, q.y - p.y);
            (l > 1e-12).then(|| ((q.y - p.y) / l, -(q.x - p.x) / l))?
        }
        SegKind::Arc => {
            let c = point(&g.pts, s.c)?;
            let l = libm::hypot(u - c.x, v - c.y);
            (l > 1e-12).then(|| ((u - c.x) / l, (v - c.y) / l))?
        }
        SegKind::Ellipse => return None,
    };
    let e = 1e-6;
    let into = inside(g, u + n2.0 * e, v + n2.1 * e);
    let sign = if into != f.cut { -1.0 } else { 1.0 };
    let (x, y) = (f.plane.x, f.plane.y);
    Some([0, 1, 2].map(|i| sign * (n2.0 * x.get(i).copied().unwrap_or(0.0) + n2.1 * y.get(i).copied().unwrap_or(0.0))))
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
            let mut cands: Vec<V3> = if ida == idb {
                let mut common: Vec<usize> = [sa.s, sa.e].into_iter().filter(|v| *v == sb.s || *v == sb.e).collect();
                common.sort_unstable();
                common.dedup();
                common
                    .iter()
                    .filter_map(|i| point(&gf.pts, *i))
                    .map(|p| lift(f, p.x * 1000.0, p.y * 1000.0, 0.0))
                    .collect()
            } else {
                if (dot(normal(f), normal(g)).abs() - 1.0).abs() > 1e-9 {
                    return Ok(None);
                }
                let (Some(pa), Some(pb)) = (poly(f, gf, sa, work)?, poly(g, gg, sb, work)?) else { return Ok(None) };
                let ts = cross_params(&pa, &pb, normal(f), work)?;
                let mut v: Vec<V3> =
                    ts.iter().filter_map(|t| point_at(&pa, *t)).map(|at| snap(at, f, gf, sa)).collect();
                // A curve that ends on the other (a line ending on an arc: Biyel Kolu's Radyus3)
                // need not cross its polyline.
                if v.is_empty() {
                    v = touches((f, gf, sa), (g, gg, sb));
                }
                v
            };
            let mut uniq: Vec<V3> = Vec::new();
            for p in cands.drain(..) {
                if uniq.iter().all(|q| libm::sqrt(dot(sub(p, *q), sub(p, *q))) > 1e-6) {
                    uniq.push(p);
                }
            }
            let p0 = match uniq.as_slice() {
                [p] => *p,
                // Two meeting points (two arcs of a lens, an arc across a line): the record's
                // order says which, the first face on the left of the edge run from the first
                // vertex face to the second (cross of the outward normals along the run).
                [p, q] => {
                    let dir = normal(f);
                    let h = |x: &Face| {
                        face_plane(x, sketches).and_then(|(o, nv)| {
                            let den = dot(dir, nv);
                            (den.abs() > 1e-6).then(|| dot(sub(o, *p), nv) / den)
                        })
                    };
                    let (Some(h0), Some(h1)) = (fc.as_ref().and_then(h), fd.as_ref().and_then(h)) else {
                        return Ok(None);
                    };
                    let run = (h1 - h0).signum();
                    let turn = |at: V3| -> Option<f64> {
                        let (na, nb) = (side_out(f, gf, sa, at)?, side_out(g, gg, sb, at)?);
                        Some(dot(cross(na, nb), dir) * run)
                    };
                    match (turn(*p), turn(*q)) {
                        (Some(a), Some(b)) if a > 1e-9 && b < -1e-9 => *p,
                        (Some(a), Some(b)) if b > 1e-9 && a < -1e-9 => *q,
                        _ => return Ok(None),
                    }
                }
                _ => return Ok(None),
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

/// What [`face_sel`] needs to name a face by the feature that made it.
struct Named<'a> {
    nodes: &'a [Node],
    swept: &'a HashMap<u32, Swept>,
    revolved: &'a HashMap<u32, Revolved>,
    /// Fillets and chamfers written as such (their faces are blends of their edges).
    blends: &'a HashSet<u32>,
    sketches: &'a [Geometry],
    /// Index into `sketches` -> the sketch's name.
    sketch_names: &'a HashMap<usize, String>,
    feats: &'a [Value],
    cap_tags: &'a HashSet<u16>,
}

/// The tree's description of a face an edge record names (`EdgePick.faces` of the tree
/// import): a side of an extrude or revolve by the sketch curves the key `idx` drew (lines
/// the sketch cleaning split keep their curve id with a suffix), an extrude's cap with its
/// plane where it is known, or the face a fillet or chamfer made from its `idx`-th edge.
/// `None` for faces of other features and mirror copies.
fn face_sel(r: &Rep, m: &Named) -> Option<Value> {
    if let Some(v) = r.via {
        // A face of a pattern's copy: the seed's face, through the pattern and copy (no plane:
        // the seed's is not the copy's).
        if r.copy == 0 {
            return None;
        }
        let pattern = &m.nodes.iter().find(|q| q.id == v)?.name;
        let mut o = face_sel(&Rep { via: None, copy: 0, ..*r }, m)?;
        let map = o.as_object_mut()?;
        map.remove("point");
        map.remove("normal");
        map.insert("pattern".into(), json!(pattern));
        map.insert("copy".into(), json!(r.copy));
        return Some(o);
    }
    let name = &m.nodes.iter().find(|q| q.id == r.fid)?.name;
    if m.blends.contains(&r.fid) {
        return Some(json!({"feature": name, "kind": "blend", "index": r.idx}));
    }
    let f = m.swept.get(&r.fid).or_else(|| m.revolved.get(&r.fid).map(|v| &v.on))?;
    let g = m.sketches.get(f.sketch)?;
    let keyed: Vec<usize> =
        g.segs.iter().enumerate().filter(|(_, s)| s.a == r.idx && !s.construction).map(|(k, _)| k).collect();
    let extruded = m.swept.contains_key(&r.fid);
    if extruded && (m.cap_tags.contains(&r.tag) || (keyed.is_empty() && r.idx <= 1)) {
        let mut o = json!({"feature": name, "kind": "cap"});
        if let Some((p, n)) = cap_plane(r, m.swept, m.sketches, m.cap_tags)
            && let Some(map) = o.as_object_mut()
        {
            map.insert("point".into(), json_v3(p));
            map.insert("normal".into(), json_v3(n));
        }
        return Some(o);
    }
    let sketch = m.sketch_names.get(&f.sketch)?;
    let sk = m.feats.iter().find(|v| {
        v.get("type").and_then(Value::as_str) == Some("sketch") && v.get("name").and_then(Value::as_str) == Some(sketch)
    })?;
    let curves: Vec<&str> = sk
        .get("curves")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|c| c.get("id").and_then(Value::as_str))
        .filter(|id| {
            keyed.iter().any(|k| {
                let base = format!("c{k}");
                *id == base || id.strip_prefix(&base).is_some_and(|rest| rest.starts_with('_'))
            })
        })
        .collect();
    if curves.is_empty() {
        return None;
    }
    let mut o = json!({"feature": name, "kind": "side", "sketch": sketch, "curves": curves});
    if extruded
        && let Some((p, n)) = side_plane(r, m.swept, m.sketches)
        && let Some(map) = o.as_object_mut()
    {
        map.insert("point".into(), json_v3(p.map(|v| round_to(v, 6) + 0.0)));
        map.insert("normal".into(), json_v3(n.map(|v| round_to(v, 12) + 0.0)));
    }
    Some(o)
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
        // The drill point: `Drill Angle` where the hole writes one (180 degrees is a flat
        // bottom: GripperbaseNEW's 2 mm holes), else the usual 118 degrees.
        let tip = get(&["Drill Angle"])
            .map(f64::to_degrees)
            .filter(|a| (30.0..=180.0 + 1e-9).contains(a))
            .map_or(118.0, |a| round_to(a.min(180.0), 9));
        o.insert("point_angle".into(), json!(tip));
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
    mark_suppressed(rf, &mut nodes);
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
    // X = 1 in solid features (ties: the first seen); with none, with X = 0 (SlotMount-Attachment:
    // the boss declares the class, the one cut refers to it with X = 0).
    let mut counts: Vec<(u16, usize)> = Vec::new();
    for want_x in [1, 0] {
        for n in &nodes {
            if is_in(&n.cls, &EXTRUDE) || is_in(&n.cls, &REVOLVE) {
                let (a, b) = span(n);
                for c in endspec_candidates(rf, a, b)? {
                    let Some(cls) = c.cls.filter(|&k| k < 0x3000 && c.x == want_x) else { continue };
                    match counts.iter_mut().find(|(k, _)| *k == cls) {
                        Some((_, v)) => *v += 1,
                        None => counts.push((cls, 1)),
                    }
                }
            }
        }
        if !counts.is_empty() {
            break;
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
    // The imported bodies so far: a cut reaches all of them (round sw7, the zsCAD parts).
    let mut imported: Vec<String> = Vec::new();
    let mut swept: HashMap<u32, Swept> = HashMap::new();
    let mut rounds: HashMap<u32, Vec<Option<Round>>> = HashMap::new();
    let mut revolved: HashMap<u32, Revolved> = HashMap::new();
    // Fillets and chamfers written as such: later edge records name their faces (`face_sel`).
    let mut blend_ids: HashSet<u32> = HashSet::new();
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
    // Each pattern of one direction read: the motion from one copy to the next.
    let mut pattern_steps: HashMap<u32, Step> = HashMap::new();
    // Node id -> name of each sketch read (lofts name their sections by id).
    let mut sketch_ids: HashMap<u32, String> = HashMap::new();
    // Node id -> name of each helix read, and the helix (round sw10): sweeps run along them.
    let mut helix_ids: HashMap<u32, String> = HashMap::new();
    let mut helices: HashMap<String, Option<Helix>> = HashMap::new();
    // The sketches read, in order, and the child indices of the features that used them.
    let mut sketch_order: Vec<String> = Vec::new();
    let mut sketch_seen: Vec<(u16, String)> = Vec::new();
    let mut sketch_used: HashSet<String> = HashSet::new();
    // The contours the stream lists and chooses (round sw7), and those taken by a feature.
    let all_contours = contours(rf, &nodes.iter().map(|n| n.id).collect());
    let mut used_contours: HashSet<usize> = HashSet::new();
    // Where the stretch of the next extrude or revolve begins (the end of the previous one).
    let mut feat_lo = 0usize;
    let mut chosen = |last_sketch: &Option<String>,
                      sketch_ids: &HashMap<u32, String>,
                      sketch_by_name: &HashMap<String, (usize, JsonPlane)>,
                      sketches: &[Geometry],
                      feat_lo: &mut usize,
                      b: usize|
     -> Value {
        let lo = std::mem::replace(feat_lo, b);
        let pick = (|| {
            let sk = last_sketch.as_ref()?;
            let fid = sketch_ids.iter().find(|(_, v)| *v == sk).map(|(k, _)| *k)?;
            let (i, plane) = sketch_by_name.get(sk)?;
            chosen_profiles(&all_contours, &mut used_contours, (fid, sketches.get(*i)?, plane), lo, b)
        })();
        pick.unwrap_or_else(|| json!("all"))
    };
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
        // A suppressed feature adds nothing; a suppressed sketch still takes its place among the
        // configuration stream's sketches below.
        if n.suppressed && cls.as_deref() != Some("moProfileFeature_c") {
            continue;
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
            if n.suppressed {
                continue;
            }
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
            sketch_ids.insert(n.id, n.name.clone());
            last_sketch = Some(n.name.clone());
            sketch_order.push(n.name.clone());
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
                || n.flags & CUT_MASK == CUT_FLAGS
                || n.name.to_lowercase().starts_with("cut")
            {
                "cut"
            } else {
                "join"
            }
        };
        if is_in(&cls, &EXTRUDE) {
            let picked = feature_sketch(n.child, &last_sketch, &sketch_order, &sketch_seen, &sketch_used);
            if let (Some(r), Some(sk)) = (n.child, &picked) {
                sketch_seen.push((r, sk.clone()));
                sketch_used.insert(sk.clone());
            }
            let last_sketch = &picked;
            let dec = endspec_decl(rf, a, b, b"moEndSpec_c")?;
            let mut cands: Vec<EndSpec> =
                dec.into_iter().chain(endspec_candidates(rf, a, b)?.into_iter().filter(in_es)).collect();
            if cands.is_empty() {
                cands.extend(endspec_directed(rf, a, b));
            }
            let es = cands.first().copied();
            let mut ps = params(rf, a, b, nt)?;
            let op = op_for("moCut_c", solid);
            let typ = es.map(|e| e.kind);
            // A depth an equation drives is kept with the equation in the configuration stream.
            if ps.is_empty()
                && matches!(typ, Some(0 | 6))
                && let Some(v) = equation_dimension(c0, &n.name, "D1")
            {
                ps.push(v);
            }
            // A cut of the flipped side keeps what its profile sweeps: through everything that is
            // the common part with the sweep (the rebuild check says whether the part lay wholly
            // on the swept side); a blind one would need the slab as well and is left unsupported.
            let flip = op == "cut" && es.is_some_and(|e| e.flip);
            let end = match typ {
                _ if flip && !matches!(typ, Some(1 | 2 | 9)) => "flip_side".to_string(),
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
            // (point, normal, offset, reverse, extend the face's surface: ADR 0061)
            let mut up_to: Option<(V3, V3, f64, bool, bool)> = None;
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
                    if t == 4 && matches!(region, Region::Curved) {
                        // A curved face: the tangent plane does not say which way the sweep runs;
                        // the written direction does, or the default one (a boss along its sketch
                        // normal, a cut against it, byte 12 reversing either).
                        let dir = extrude_direction(rf, a, b).and_then(|v| direction_agrees(n, 1.0, v)).map_or_else(
                            || {
                                let d = if op == "cut" { -1.0 } else { 1.0 };
                                if es.is_some_and(|e| e.b12 == 1) { -d } else { d }
                            },
                            |agree| if agree { 1.0 } else { -1.0 },
                        );
                        // SolidWorks extends the surface where it does not cover the profile.
                        up_to = Some((pt, nf, 0.0, dir < 0.0, true));
                    } else if t == 4 && den.abs() > 1e-9 {
                        let dir = extrude_direction(rf, a, b)
                            .and_then(|v| direction_agrees(n, 1.0, v))
                            .map_or(s_face.signum(), |agree| if agree { 1.0 } else { -1.0 });
                        if s_face * dir > 1e-9 && covered(dir) {
                            up_to = Some((pt, nf, 0.0, dir < 0.0, false));
                            end_s = parallel.then_some(s_face);
                        } else if s_face * dir > 1e-9 && parallel {
                            // The face is smaller than the profile: SolidWorks extends its surface.
                            distance = Some(round_to(s_face, 6) + 0.0);
                            end_s = distance;
                        } else if s_face * dir > 1e-9 {
                            // A tilted face smaller than the profile: up to its extended plane.
                            up_to = Some((pt, nf, 0.0, dir < 0.0, true));
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
                                up_to = Some((pt, nf, round_to((s_end - s_face) * dir, 6) + 0.0, dir < 0.0, false));
                            } else {
                                distance = end_s;
                            }
                        }
                    }
                }
                // A cap the reader cannot place a point on (its profile does not read as closed:
                // Back_sup_RT_upp's Boss-Extrude1), or the flat wall of a cut through all
                // (z_motor_mount), still has its plane: a parallel one gives the distance, as an
                // end on a face smaller than the profile does.
                let end = match end_face_ref(rf, a, b) {
                    Some(r) if face_pick(&r, &swept, &sketches, &cap_tags).is_none() => {
                        cap_plane(&r, &swept, &sketches, &cap_tags)
                            .or_else(|| side_plane(&r, &swept, &sketches).filter(|_| t == 4))
                    }
                    Some(_) => None,
                    // A reference plane (or a face named as a plane) instead of a face.
                    None => plane_ref(rf, a, b).filter(|_| t == 4),
                };
                if let Some(&(_, plane)) = sketch_by_name.get(sk)
                    && let Some((pt, nf)) = end
                {
                    let n = cross(plane.x, plane.y);
                    let den = dot(n, nf);
                    let s_face = dot(sub(pt, plane.origin), nf) / den;
                    if (den.abs() - 1.0).abs() < 1e-9 {
                        let s = match (t, ps.first()) {
                            (4, _) => Some(s_face),
                            (_, Some(off)) => Some(s_face - mm(*off) / den),
                            _ => None,
                        };
                        if let Some(s) = s.filter(|s| s.abs() > 1e-9) {
                            distance = Some(round_to(s, 6) + 0.0);
                            end_s = distance;
                        }
                    }
                }
            }
            // A blind cut with a further length (`D<n+1>` after the `n` dimensions of its own
            // sketch) starts that far from its sketch plane. With an end spec of X 1 the start
            // lies ahead along the cut direction (MicroGearMotor Cut-Extrude2: D1 4.25 mm, offset
            // 0.8 mm, the void spans z -5.05 .. -0.8 from the Front plane); with X 0 it lies
            // behind. `D2` is the second direction's length instead (below; round sw12:
            // gt2_pulley 2's Cut-Extrude5, 10 mm each way; idler_single_v1's bore, 10 mm one way
            // and 1 mm the other, which round sw2 read as a start 1 mm behind: the same solid).
            let draft_angle = extrude_draft(rf, &param_slots(rf, a, b, nt)?);
            let start_offset = named_params(rf, a, b, nt)?
                .iter()
                .find(|(k, _)| {
                    k.strip_prefix('D').is_some_and(|n| n != "1" && n != "2" && n.bytes().all(|c| c.is_ascii_digit()))
                        && (draft_angle.is_none() || k != "D3")
                })
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
            js.insert("profiles".into(), chosen(last_sketch, &sketch_ids, &sketch_by_name, &sketches, &mut feat_lo, b));
            js.insert("operation".into(), json!(if flip { "intersect" } else { op }));
            // A cut reaches every body (SolidWorks' default scope): with several imported bodies
            // it names them all (5bq's two extrudes cut all four of its bodies).
            let mut bodies: Vec<&String> = first_solid.iter().chain(&imported).collect();
            bodies.dedup();
            let target = match op {
                "new" => Value::Null,
                "cut" if !flip && bodies.len() > 1 => json!(bodies),
                _ => json!(first_solid),
            };
            js.insert("target".into(), target);
            js.insert("sw_end_kind".into(), json!(end));
            if flip && !through_all {
                distance = None;
                up_to = None;
            }
            if let Some((pt, nf, off, reverse, extend)) = up_to {
                js.insert("end_condition".into(), json!("to_face"));
                js.insert("face".into(), json!({"point": json_v3(pt), "normal": json_v3(nf)}));
                if off.abs() > 1e-9 {
                    js.insert("offset".into(), json!(round_to(off, 6) + 0.0));
                }
                if reverse {
                    js.insert("reverse".into(), json!(true));
                }
                if extend {
                    js.insert("extend".into(), json!(true));
                }
                // A face parallel to the sketch: its plane, should the face no longer cover the
                // sweep when rebuilt (a hole cut through it since: 3_way_cube_connector's
                // counterbores end 3.5 mm above the bottom face their through holes pierce).
                if let Some(e) = end_s {
                    js.insert("plane_distance".into(), json!(e));
                }
            } else {
                js.insert("distance".into(), json!(distance));
                // A boss of end type 2 stops at the next face it meets (round sw12: mid_elbow's
                // pins, written through all, ran 360 mm out of a 35 mm part); the reach through
                // all stays as the fallback where that does not build.
                if op != "cut"
                    && typ == Some(2)
                    && let Some(d) = distance
                {
                    js.insert("end_condition".into(), json!("to_next"));
                    if d < 0.0 {
                        js.insert("reverse".into(), json!(true));
                    }
                }
                // A blind extrude with a second length `D2` runs that far to the other side of its
                // sketch too (SolidWorks' "Direction 2": the pulleys' hubs, 2 mm one way and 7 mm
                // the other).
                if typ == Some(0)
                    && !symmetric
                    && let Some(d) = distance.filter(|d| d.abs() > 1e-9)
                    && let named = named_params(rf, a, b, nt)?
                    && named.first().is_some_and(|(k, _)| k == "D1")
                    && let Some(d2) = named.iter().find(|(k, _)| k == "D2").map(|(_, v)| mm(*v)).filter(|v| *v > 1e-9)
                {
                    js.insert("distance2".into(), json!(round_to(-d.signum() * d2, 9) + 0.0));
                }
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
                swept.insert(
                    n.id,
                    Swept {
                        sketch: i,
                        plane,
                        distance: dist,
                        symmetric,
                        through: through_all,
                        cut: op == "cut" && !flip,
                    },
                );
            }
            // Draft while extruding: the side faces lean by the angle, the sketch plane the
            // neutral plane, pulled the way the feature runs into the part (a symmetric one: against
            // the sketch normal, Retainer's through cut from its top face).
            let draft = match (draft_angle, last_sketch.as_ref().and_then(|sk| sketch_by_name.get(sk))) {
                (Some(angle), Some(&(i, jp))) if !flip => {
                    let normal = cross(jp.x, jp.y);
                    let reach = end_s.or(distance).filter(|d| d.abs() > 1e-9);
                    let sign = if symmetric { -1.0 } else { reach.map_or(0.0, f64::signum) };
                    let depth = if through_all || symmetric { 0.01 } else { reach.map_or(0.0, |d| d.abs() / 2.0) };
                    let dir = normal.map(|c| c * sign);
                    let points: Vec<Value> = sketches
                        .get(i)
                        .map(|g| {
                            g.segs
                                .iter()
                                .filter(|s| !s.construction && !s.centerline)
                                .filter_map(|s| {
                                    let uv = seg_uv(g, s)?;
                                    // A line's midpoint, an arc's middle sample.
                                    let (u, v) = match uv.as_slice() {
                                        [(u0, v0), (u1, v1)] => (f64::midpoint(*u0, *u1), f64::midpoint(*v0, *v1)),
                                        _ => *uv.get(uv.len() / 2)?,
                                    };
                                    let p = comb(comb(jp.origin, 1.0, jp.x, u * 1000.0), 1.0, jp.y, v * 1000.0);
                                    Some(json!({"point": json_v3(comb(p, 1.0, dir, depth).map(|c| round_to(c, 9) + 0.0))}))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    (sign != 0.0 && !points.is_empty()).then(|| {
                        json!({"type": "draft", "name": format!("{}-Draft", n.name), "angle": round_to(angle.to_degrees(), 9),
                            "faces": points, "plane": {"origin": json_v3(jp.origin), "normal": json_v3(dir)}})
                    })
                }
                _ => None,
            };
            // The extrude keeps its own side faces (no merge with coplanar faces of the body it
            // joins), so the draft leans those only (Link 3's end piece on the arm's sides).
            if draft.is_some()
                && let Some(&(i, jp)) = last_sketch.as_ref().and_then(|sk| sketch_by_name.get(sk))
                && sketches.get(i).is_some_and(|g| {
                    continues_sides(
                        g,
                        jp,
                        (end_s.or(distance).unwrap_or(0.0), symmetric),
                        swept.iter().filter(|(k, _)| **k != n.id).map(|(_, f)| f),
                        &sketches,
                    )
                })
            {
                js.insert("refine".into(), json!(false));
            }
            feats.push(Value::Object(js));
            feats.extend(draft);
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if is_in(&cls, &REVOLVE) {
            let picked = feature_sketch(n.child, &last_sketch, &sketch_order, &sketch_seen, &sketch_used);
            if let (Some(r), Some(sk)) = (n.child, &picked) {
                sketch_seen.push((r, sk.clone()));
                sketch_used.insert(sk.clone());
            }
            let last_sketch = &picked;
            let ps = params(rf, a, b, nt)?;
            let ang = ps.iter().find(|v| v.abs() <= TAU + 1e-9);
            let op = op_for("moRevCut_c", solid);
            let mut js = Map::new();
            js.insert("type".into(), json!("revolve"));
            js.insert("name".into(), json!(n.name));
            js.insert("sketch".into(), json!(last_sketch));
            js.insert("profiles".into(), chosen(last_sketch, &sketch_ids, &sketch_by_name, &sketches, &mut feat_lo, b));
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
                    // Centerlines before construction lines (Linear Bearing Inner: a centerline
                    // across the profile, a construction line at its end, then the axis).
                    let one_sided_of = |centerline: bool| {
                        (0..g.segs.len()).find(|&k| {
                            g.segs.get(k).is_some_and(|s| {
                                line(s)
                                    && s.id != 0xffff_ffff
                                    && (s.centerline || (!centerline && s.construction))
                                    && one_sided(k)
                            })
                        })
                    };
                    one_sided_of(true)
                        .or_else(|| one_sided_of(false))
                        .or_else(|| {
                            (0..g.segs.len())
                                .find(|&k| g.segs.get(k).is_some_and(|s| line(s) && on_sketch_axis(s)) && one_sided(k))
                        })
                        .or(first)
                };
                // A full revolve writes its body's box (centre and diagonal, model space); the
                // centre lies on the axis. Where the line chosen above misses it, the line through
                // it with the profile on one side is the axis (126p-door-stop-roller: a profile
                // edge off the sketch's axes; 126p-rosette: a lone line).
                let full = ang.is_none_or(|v| (v.abs() - TAU).abs() < 1e-9);
                let through = |k: usize, c: V3| -> bool {
                    let (Some(s), Some(&(_, jp))) =
                        (g.segs.get(k).filter(|s| line(s)), sketch_by_name.get(last_sketch.as_deref().unwrap_or("")))
                    else {
                        return false;
                    };
                    let model =
                        |q: Pt| -> V3 { comb(comb(jp.origin, 1.0, jp.x, q.x * 1000.0), 1.0, jp.y, q.y * 1000.0) };
                    let (Some(p), Some(q)) = (point(&g.pts, s.s), point(&g.pts, s.e)) else { return false };
                    let (p, q) = (model(p), model(q));
                    let u = sub(q, p);
                    let len = dot(u, u).sqrt();
                    let w = cross(sub(c, p), u);
                    len > 1e-9 && dot(w, w).sqrt() / len < 1e-5
                };
                let axis = match (named, revolve_box_centre(rf, a, b)) {
                    (None, Some(c)) if full && !axis.is_some_and(|k| through(k, c)) => (0..g.segs.len())
                        .find(|&k| g.segs.get(k).is_some_and(|s| s.id != 0xffff_ffff) && through(k, c) && one_sided(k))
                        .or(axis),
                    _ => axis,
                };
                let axis_json = match (axis, named) {
                    (Some(k), _) => json!({"line": format!("c{k}")}),
                    (None, Some((o, dir))) => json!({"origin": json_v3(round3(o)), "direction": json_v3(dir)}),
                    (None, None) => Value::Null,
                };
                js.insert("axis".into(), axis_json);
                // An open profile whose loose ends lie on the axis is closed along it.
                let closing = axis.map(|k| axis_closure(g, k)).unwrap_or_default();
                if !closing.is_empty()
                    && let Some(cs) = feats
                        .iter_mut()
                        .rev()
                        .find(|f| f.get("type") == Some(&json!("sketch")) && f.get("name") == Some(&json!(last_sketch)))
                        .and_then(|f| f.get_mut("curves"))
                        .and_then(Value::as_array_mut)
                {
                    for (j, (p, q)) in closing.into_iter().enumerate() {
                        cs.push(json!({"id": format!("c{}", g.segs.len() + j), "type": "line",
                            "start": format!("p{p}"), "end": format!("p{q}")}));
                    }
                }
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
                        let on =
                            Swept { sketch: i, plane, distance: 0.0, symmetric: false, through: false, cut: false };
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
        } else if matches!(cls.as_deref(), Some("Fillet_c" | "Chamfer_c"))
            || (cls.as_deref() == Some("VarFillet_c") && constant_radius(&params(rf, a, b, nt)?))
        {
            let fillet = cls.as_deref() != Some("Chamfer_c");
            let ps = params(rf, a, b, nt)?;
            let mirror_ids: HashSet<u32> = mirror_planes.keys().copied().collect();
            let pattern_ids: HashSet<u32> = pattern_steps.keys().copied().collect();
            let sel = selection(edge_records_to(rf, a, b, &mirror_ids, &pattern_ids)?, n.id);
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
            let sketch_names: HashMap<usize, String> =
                sketch_by_name.iter().map(|(k, (i, _))| (*i, k.clone())).collect();
            let names = Named {
                nodes: &nodes,
                swept: &swept,
                revolved: &revolved,
                blends: &blend_ids,
                sketches: &sketches,
                sketch_names: &sketch_names,
                feats: &feats,
                cap_tags: &cap_tags,
            };
            // Edges named by their faces alone (no midpoint found).
            let mut by_faces = 0usize;
            for (k, (_, reps)) in sel.iter().enumerate() {
                let number = k + 1;
                // The planes of vertex faces of other features that cut the circle.
                let ends = match reps.as_slice() {
                    [r0, _, r2, r3, ..] if r2.fid != r0.fid && r3.fid != r0.fid => {
                        let plane = |r: &Rep| {
                            face_of(r, &swept, &rounds, &sketches, &cap_tags).and_then(|f| face_plane(&f, &sketches))
                        };
                        plane(r2).zip(plane(r3)).map(|(a, b)| [a, b])
                    }
                    _ => None,
                };
                if let Some((m, path)) = revolve_circle(reps, &revolved, &sketches, ends) {
                    mids.push(Some(m));
                    made.push(None);
                    edges.push(json!({"midpoint": m, "path": path, "selection": number}));
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
                    None if reps.iter().any(|r| r.via.is_some_and(|v| pattern_steps.contains_key(&v))) => {
                        match patterned_edge(reps, &swept, &pattern_steps) {
                            Some((step, k, reps, swept)) => {
                                let model = Model {
                                    swept: &swept,
                                    rounds: &rounds,
                                    revolved: &revolved,
                                    sketches: &sketches,
                                    cap_tags: &cap_tags,
                                };
                                let seed = match midpoint(&reps, &swept, &rounds, &sketches, &cap_tags, &mut work)? {
                                    Some(m) => Some(m),
                                    None => closed_edge(&reps, &model, &mut work)?,
                                };
                                seed.map(|m| step_point(step, k, m).map(|c| round_to(c, 6) + 0.0))
                            }
                            None => None,
                        }
                    }
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
                // The edge's faces and end faces, for the importer to find it by name where
                // the midpoint is off the model or not known (round sw9).
                let faces = match reps {
                    [a, b, rest @ ..] => face_sel(a, &names).zip(face_sel(b, &names)).map(|(a, b)| {
                        let ends: Vec<Value> =
                            rest.iter().take(2).map(|r| face_sel(r, &names).unwrap_or(Value::Null)).collect();
                        (vec![a, b], ends)
                    }),
                    _ => None,
                };
                let pick = |mid: Option<V3>| {
                    let mut o = Map::new();
                    if let Some(m) = mid {
                        o.insert("midpoint".into(), json!(m));
                    }
                    if let Some((f, e)) = &faces {
                        o.insert("faces".into(), json!(f));
                        o.insert("ends".into(), json!(e));
                    }
                    o.insert("selection".into(), json!(number));
                    Value::Object(o)
                };
                // A record of faces the features read made whose edge was not placed may be a
                // superseded selection (the idler arms' keys 15-19): named by its faces, it is
                // optional, and left out where the model has no such edge.
                let superseded = reps.iter().all(|r| {
                    swept.contains_key(&r.fid) || rounds.contains_key(&r.fid) || revolved.contains_key(&r.fid)
                });
                match m {
                    Some(m) => edges.push(pick(Some(m))),
                    None if faces.is_some() => {
                        by_faces += 1;
                        let mut p = pick(None);
                        if superseded && let Some(o) = p.as_object_mut() {
                            o.insert("optional".into(), json!(true));
                        }
                        edges.push(p);
                    }
                    // References to faces the sketch no longer has are superseded selections.
                    None if superseded => {
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
                && by_faces == 0
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
                && by_faces == 0
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
            blend_ids.insert(n.id);
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
            if let [one] = made.as_slice()
                && let Some(step) = pattern_step(one)
            {
                pattern_steps.insert(n.id, step);
            }
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
            imported.push(n.name.clone());
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if matches!(cls.as_deref(), Some("moBlend_c" | "moBlendCut_c")) {
            // A loft: its sections are profile components naming their sketches by node id.
            let sections = named_sketches(rf, a, b, &sketch_ids);
            let op = if !solid {
                "new"
            } else if cls.as_deref() == Some("moBlendCut_c") {
                "cut"
            } else {
                "join"
            };
            let mut js = Map::new();
            js.insert("type".into(), json!("loft"));
            js.insert("name".into(), json!(n.name));
            js.insert("sketches".into(), json!(sections));
            js.insert("operation".into(), json!(op));
            if op != "new" {
                js.insert("target".into(), json!(first_solid));
            }
            if sections.len() < 2 {
                js.insert("unresolved".into(), json!(true));
            }
            // A circle section starts where the loft's connector meets it: the sections twist
            // against each other as SolidWorks joins them.
            let mut aligned = sections.len() >= 2;
            for sk in &sections {
                let split = feats
                    .iter_mut()
                    .find(|f| {
                        f.get("type").and_then(Value::as_str) == Some("sketch")
                            && f.get("name").and_then(Value::as_str) == Some(sk.as_str())
                    })
                    .is_some_and(|f| split_circle_at_connector(f, rf, a, b));
                aligned &= split;
            }
            if aligned {
                js.insert("aligned".into(), json!(true));
            }
            feats.push(Value::Object(js));
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if matches!(cls.as_deref(), Some("moSweep_c" | "moSweepCut_c")) {
            // A sweep: its profile sketch, then its path sketch, named as a loft's sections are.
            let named = named_sketches(rf, a, b, &sketch_ids);
            let op = if !solid {
                "new"
            } else if cls.as_deref() == Some("moSweepCut_c") {
                "cut"
            } else {
                "join"
            };
            let mut js = Map::new();
            js.insert("type".into(), json!("sweep"));
            js.insert("name".into(), json!(n.name));
            // A circular profile (SolidWorks 2016 on) has no sketch: its diameter is the
            // dimension written before the `<MOD-DIAM>` text, the path is the one sketch named.
            let circle = || -> Option<(Value, String)> {
                let [path] = named.as_slice() else { return None };
                let at = find(rf, &MOD_DIAM, a, b)?;
                let dia = named_params(rf, a, at, nt).ok()?.last().map(|(_, v)| mm(*v)).filter(|v| *v > 0.0)?;
                let sk = feats.iter().find(|f| {
                    f.get("type").and_then(Value::as_str) == Some("sketch")
                        && f.get("name").and_then(Value::as_str) == Some(path.as_str())
                })?;
                Some((circle_at_path_start(sk, dia, &format!("{}_Profile", n.name))?, path.clone()))
            };
            // A path that is a helix: the sweep names the helix node as it names its sketches.
            let along = named_sketches(rf, a, b, &helix_ids).into_iter().next();
            match (named.as_slice(), along) {
                ([profile, path, ..], _) => {
                    js.insert("sketch".into(), json!(profile));
                    js.insert("path".into(), json!(path));
                }
                ([profile], Some(hn)) => {
                    let h = helices.get(&hn).cloned().flatten();
                    match h.and_then(|h| helix_sweep(&mut feats, profile, &h)) {
                        Some(hx) => {
                            js.insert("sketch".into(), json!(profile));
                            js.insert("helix".into(), hx);
                        }
                        None => {
                            js.insert("unresolved".into(), json!(true));
                        }
                    }
                }
                _ => match circle() {
                    Some((profile, path)) => {
                        js.insert("sketch".into(), profile.get("name").cloned().unwrap_or(Value::Null));
                        js.insert("path".into(), json!(path));
                        feats.push(profile);
                    }
                    None => {
                        js.insert("unresolved".into(), json!(true));
                    }
                },
            }
            js.insert("operation".into(), json!(op));
            if op != "new" {
                js.insert("target".into(), json!(first_solid));
            }
            feats.push(Value::Object(js));
            solid = true;
            if first_solid.as_deref().is_none_or(str::is_empty) {
                first_solid = Some(n.name.clone());
            }
        } else if cls.as_deref() == Some("moHelix_c") {
            // A helix is a curve: it adds nothing to the part until a sweep runs along it.
            // Its base sketch, a circle, is its child as an extrude's sketch is.
            let base = feature_sketch(n.child, &last_sketch, &sketch_order, &sketch_seen, &sketch_used);
            if let (Some(r), Some(sk)) = (n.child, &base) {
                sketch_seen.push((r, sk.clone()));
                sketch_used.insert(sk.clone());
            }
            let sk = base.as_ref().and_then(|nm| {
                feats.iter().find(|f| {
                    f.get("type").and_then(Value::as_str) == Some("sketch")
                        && f.get("name").and_then(Value::as_str) == Some(nm.as_str())
                })
            });
            let h = sk.and_then(|sk| helix_of(sk, helix_record(rf, a, b)?));
            helix_ids.insert(n.id, n.name.clone());
            helices.insert(n.name.clone(), h);
        } else if cls.as_deref() == Some("moShell_c") {
            let ps = params(rf, a, b, nt)?;
            let recs = edge_records(rf, a, b)?;
            let pick =
                recs.last().and_then(|(_, reps)| reps.first()).and_then(|r| face_pick(r, &swept, &sketches, &cap_tags));
            let mut js = Map::new();
            js.insert("type".into(), json!("shell"));
            js.insert("name".into(), json!(n.name));
            js.insert("thickness".into(), json!(ps.first().map(|v| mm(*v))));
            match pick {
                Some((pt, nrm, _)) => {
                    js.insert("faces".into(), json!([{"point": json_v3(pt), "normal": json_v3(nrm)}]));
                }
                None => {
                    js.insert("faces".into(), json!([]));
                    js.insert("unresolved".into(), json!(true));
                }
            }
            feats.push(Value::Object(js));
        } else if cls.is_none() || is_in(&cls, &SKIPPED) {
            continue;
        } else if cls.as_deref() == Some("moDraft_c") {
            // A draft (round sw11): the angle `D1`, the neutral plane the face its `moPlaneRef_w`
            // names, and a point on each drafted face in the list that ends the span.
            let ps = params(rf, a, b, nt)?;
            let span = rf.get(a..b).unwrap_or(&[]);
            let neutral = neutral_face_ref(span)
                .and_then(|r| face_pick(&r, &swept, &sketches, &cap_tags))
                .map(|(o, n, _)| (o, n))
                .or_else(|| plane_ref(rf, a, b));
            let points = draft_points(span);
            let mut js = Map::new();
            js.insert("type".into(), json!("draft"));
            js.insert("name".into(), json!(n.name));
            js.insert("angle".into(), json!(ps.first().map(|v| round_to(v.to_degrees(), 9))));
            js.insert("faces".into(), json!(points.iter().map(|p| json!({"point": json_v3(*p)})).collect::<Vec<_>>()));
            match neutral {
                Some((o, nrm)) if !points.is_empty() => {
                    js.insert("plane".into(), json!({"origin": json_v3(o), "normal": json_v3(nrm)}));
                }
                _ => {
                    js.insert("unresolved".into(), json!(true));
                }
            }
            feats.push(Value::Object(js));
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

/// The sketches a loft or a sweep names, in order of first mention. Each is a profile
/// component naming its sketch by node id: the id follows 8 zero bytes (or four and the word
/// `50 46 00 00` in newer files) and is followed by a time stamp (shaft_stage_1's Loft1:
/// `... 00 00 00 00 50 46 00 00 2d 00 00 00 c3 10 34 69`, Sketch4; Part3's:
/// `00 x 8 22 00 00 00 19 c4 42 5f`, Sketch2).
fn named_sketches(d: &[u8], a: usize, b: usize, sketch_ids: &HashMap<u32, String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in a.saturating_add(8)..b.saturating_sub(8) {
        let (Some(id), Some(stamp)) = (u32_at(d, i), u32_at(d, i + 4)) else { break };
        let Some(name) = sketch_ids.get(&id) else { continue };
        if !(0x3000_0000..0x8000_0000).contains(&stamp) {
            continue;
        }
        let (Some(p0), Some(p1)) = (u32_at(d, i - 8), u32_at(d, i - 4)) else { continue };
        if p0 == 0 && (p1 == 0 || p1 == 0x4650) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
}

/// A loft section that is one circle, split into two half arcs at the loft's connector point
/// on it: three doubles (m) in the loft's span that lie on the circle as placed in its sketch
/// plane (shaft_stage_3's Loft1: (9.81, -11.34, 305) on the 15 mm circle at z = 305 and
/// (12.76, -2.50, 330) on the 13 mm one, a twist of 38 degrees). Other sketches are left alone.
fn split_circle_at_connector(sk: &mut Value, d: &[u8], a: usize, b: usize) -> bool {
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    let Some(pl) = sk.get("plane") else { return false };
    let (Some(o), Some(x), Some(y)) = (v3(pl.get("origin")), v3(pl.get("x_axis")), v3(pl.get("y_axis"))) else {
        return false;
    };
    let Some(curves) = sk.get("curves").and_then(Value::as_array) else { return false };
    let profile: Vec<&Value> = curves.iter().filter(|c| c.get("construction").is_none()).collect();
    let [circle] = profile.as_slice() else { return false };
    if circle.get("type").and_then(Value::as_str) != Some("circle") {
        return false;
    }
    let (Some(cid), Some(r), Some(id)) = (
        circle.get("center").and_then(Value::as_str).map(str::to_owned),
        circle.get("radius").and_then(Value::as_f64),
        circle.get("id").and_then(Value::as_str).map(str::to_owned),
    ) else {
        return false;
    };
    let Some(points) = sk.get("points").and_then(Value::as_array) else { return false };
    let Some(c) = points.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(cid.as_str())) else {
        return false;
    };
    let (Some(cu), Some(cv)) = (c.get("x").and_then(Value::as_f64), c.get("y").and_then(Value::as_f64)) else {
        return false;
    };
    let n = cross(x, y);
    let tol = 1e-6 * (1.0 + r);
    let mut hit = None;
    for i in a..b.saturating_sub(23) {
        let Some(p) = f64s_at::<3>(d, i) else { break };
        if !p.iter().all(|v| v.is_finite() && v.abs() < 1e3) {
            continue;
        }
        let q = sub(p.map(mm), o);
        let (u, v) = (dot(q, x), dot(q, y));
        if dot(q, n).abs() < tol && (libm::hypot(u - cu, v - cv) - r).abs() < tol && r > 1e-6 {
            hit = Some((u, v));
            break;
        }
    }
    let Some((u, v)) = hit else { return false };
    let used: Vec<u64> = points
        .iter()
        .chain(curves.iter())
        .filter_map(|p| p.get("id").and_then(Value::as_str))
        .filter_map(|s| s.get(1..).and_then(|t| t.parse().ok()))
        .collect();
    let next = used.iter().max().map_or(0, |m| m + 1);
    let (pa, pb, ca, cb) = (format!("p{next}"), format!("p{}", next + 1), format!("c{next}"), format!("c{}", next + 1));
    let r6 = |t: f64| round_to(t, 6) + 0.0;
    let new_pts =
        [json!({"id": pa, "x": r6(u), "y": r6(v)}), json!({"id": pb, "x": r6(2.0 * cu - u), "y": r6(2.0 * cv - v)})];
    let arcs = [
        json!({"type": "arc", "id": ca, "center": cid, "start": pa, "end": pb}),
        json!({"type": "arc", "id": cb, "center": cid, "start": pb, "end": pa}),
    ];
    if let Some(Value::Array(ps)) = sk.get_mut("points") {
        ps.extend(new_pts);
    }
    if let Some(Value::Array(cs)) = sk.get_mut("curves") {
        cs.retain(|c| c.get("id").and_then(Value::as_str) != Some(id.as_str()));
        cs.extend(arcs);
    }
    true
}

/// `<MOD-DIAM>` in UTF-16: the display text of a sweep's circular-profile diameter.
const MOD_DIAM: [u8; 20] = *b"<\0M\0O\0D\0-\0D\0I\0A\0M\0>\0";

/// A sketch of one circle of diameter `dia` (mm) square to the start of the open path that the
/// sketch `path` (tree JSON) draws: the profile of a circular-profile sweep. The start is an
/// end of the path (a point one curve meets), the circle's plane is square to the path there.
fn circle_at_path_start(path: &Value, dia: f64, name: &str) -> Option<Value> {
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    let pl = path.get("plane")?;
    let (o, x, y) = (v3(pl.get("origin"))?, v3(pl.get("x_axis"))?, v3(pl.get("y_axis"))?);
    let pts: HashMap<&str, (f64, f64)> = path
        .get("points")?
        .as_array()?
        .iter()
        .filter_map(|p| Some((p.get("id")?.as_str()?, (p.get("x")?.as_f64()?, p.get("y")?.as_f64()?))))
        .collect();
    let curves: Vec<&Value> = path
        .get("curves")?
        .as_array()?
        .iter()
        .filter(|c| {
            c.get("construction").is_none() && matches!(c.get("type").and_then(Value::as_str), Some("line" | "arc"))
        })
        .collect();
    fn end<'a>(c: &'a Value, k: &str) -> Option<&'a str> {
        c.get(k).and_then(Value::as_str)
    }
    let mut degree: HashMap<&str, usize> = HashMap::new();
    for c in &curves {
        for k in ["start", "end"] {
            *degree.entry(end(c, k)?).or_default() += 1;
        }
    }
    // The first curve with an open end, and the tangent into the path there.
    let (at, t) = curves.iter().find_map(|c| {
        let (s, e) = (end(c, "start")?, end(c, "end")?);
        let first = if degree.get(s) == Some(&1) {
            s
        } else if degree.get(e) == Some(&1) {
            e
        } else {
            return None;
        };
        let p = *pts.get(first)?;
        let t = if c.get("type").and_then(Value::as_str) == Some("line") {
            let q = *pts.get(if first == s { e } else { s })?;
            (q.0 - p.0, q.1 - p.1)
        } else {
            // Arcs run counter-clockwise from start to end.
            let m = *pts.get(c.get("center")?.as_str()?)?;
            let (dx, dy) = (p.0 - m.0, p.1 - m.1);
            if first == s { (-dy, dx) } else { (dy, -dx) }
        };
        Some((p, t))
    })?;
    let len = libm::hypot(t.0, t.1);
    if len < 1e-12 {
        return None;
    }
    let along = comb(x, t.0 / len, y, t.1 / len);
    let origin = comb(comb(o, 1.0, x, at.0), 1.0, y, at.1);
    let nx = cross(x, y);
    let ny = cross(along, nx);
    let r6 = |v: V3| v.map(|c| round_to(c, 12) + 0.0);
    Some(json!({
        "type": "sketch", "name": name,
        "plane": {"origin": r6(origin), "x_axis": r6(nx), "y_axis": r6(ny)},
        "points": [{"id": "p0", "x": 0.0, "y": 0.0}],
        "curves": [{"id": "c0", "type": "circle", "center": "p0", "radius": round_to(dia / 2.0, 9)}],
    }))
}

/// A helix (`moHelix_c`, round sw10) as placed in the part: about the normal of its sketch
/// plane through the centre of the sketch's circle (mm).
#[derive(Debug, Clone)]
struct Helix {
    centre: V3,
    /// Unit normal of the base sketch's plane (its x axis cross its y axis).
    normal: V3,
    radius: f64,
    clockwise: bool,
    reversed: bool,
    height: f64,
    pitch: f64,
}

/// The helix record of a `moHelix_c` span: `06 00 00 00`, six zero bytes, then the start angle
/// (rad), the clockwise and reverse-direction flags (`u32`, 0 or 1), the height and the pitch
/// (m) and the revolutions; the height is the pitch times the revolutions (all 15 helices of
/// the corpus). Returns (clockwise, reversed, height mm, pitch mm).
fn helix_record(d: &[u8], a: usize, b: usize) -> Option<(bool, bool, f64, f64)> {
    let mut at = a;
    while let Some(i) = find(d, &[6, 0, 0, 0, 0, 0, 0, 0, 0, 0], at, b) {
        at = i + 1;
        let o = i + 10;
        let (Some(angle), Some(cw), Some(rev)) = (f64_at(d, o), u32_at(d, o + 8), u32_at(d, o + 12)) else {
            continue;
        };
        let (Some(h), Some(p), Some(n)) = (f64_at(d, o + 16), f64_at(d, o + 24), f64_at(d, o + 32)) else {
            continue;
        };
        let pos = |v: f64| v.is_finite() && v > 0.0;
        let flags = cw < 2 && rev < 2;
        if angle.is_finite()
            && angle.abs() <= 2.0 * TAU
            && flags
            && pos(h)
            && pos(p)
            && pos(n)
            && (h - p * n).abs() <= 1e-9 * h
        {
            return Some((cw == 1, rev == 1, mm(h), mm(p)));
        }
    }
    None
}

/// The helix on the base sketch `sk` (tree JSON) with the record's flags and sizes: the sketch's
/// circles (all one circle, drawn once or more) give its centre and radius.
fn helix_of(sk: &Value, (clockwise, reversed, height, pitch): (bool, bool, f64, f64)) -> Option<Helix> {
    let (o, x, y) = sketch_frame(sk)?;
    let pts = sketch_points(sk)?;
    let mut circle: Option<((f64, f64), f64)> = None;
    for c in sk.get("curves")?.as_array()? {
        if c.get("type").and_then(Value::as_str) != Some("circle") {
            continue;
        }
        let at = *pts.get(c.get("center")?.as_str()?)?;
        let r = c.get("radius")?.as_f64()?;
        match circle {
            None => circle = Some((at, r)),
            Some((p, q)) if (p.0 - at.0).abs() < 1e-9 && (p.1 - at.1).abs() < 1e-9 && (q - r).abs() < 1e-9 => {}
            Some(_) => return None,
        }
    }
    let ((u, v), radius) = circle.filter(|(_, r)| *r > 0.0)?;
    Some(Helix {
        centre: comb(comb(o, 1.0, x, u), 1.0, y, v),
        normal: cross(x, y),
        radius,
        clockwise,
        reversed,
        height,
        pitch,
    })
}

/// A sketch's plane (origin, x axis, y axis) in its tree JSON.
fn sketch_frame(sk: &Value) -> Option<(V3, V3, V3)> {
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    let pl = sk.get("plane")?;
    Some((v3(pl.get("origin"))?, v3(pl.get("x_axis"))?, v3(pl.get("y_axis"))?))
}

/// A sketch's points by id in its tree JSON.
fn sketch_points(sk: &Value) -> Option<HashMap<&str, (f64, f64)>> {
    Some(
        sk.get("points")?
            .as_array()?
            .iter()
            .filter_map(|p| Some((p.get("id")?.as_str()?, (p.get("x")?.as_f64()?, p.get("y")?.as_f64()?))))
            .collect(),
    )
}

/// The tree's `helix` of a sweep of the sketch `profile` along `h`. The helix climbs from its
/// base plane along the plane's normal when reversed, against it when not, turning clockwise
/// (seen with the normal towards the viewer) or not (the Shaft Couplers' helix: reversed,
/// clockwise, its sweep profile on the helix past its end, 6 to 15.2 mm up and turning from
/// 135 to 99 degrees; the gears' RH helix: clockwise, not reversed). The profile runs the helix
/// from the end it lies at (within a turn), the tree's helix from the profile. The tree's
/// helix climbs a world axis through the origin; one that climbs the other way runs along a
/// construction line added to the profile sketch on the axis.
fn helix_sweep(feats: &mut [Value], profile: &str, h: &Helix) -> Option<Value> {
    let up = if h.reversed { h.normal } else { h.normal.map(|c| -c) };
    let left_handed = h.clockwise == h.reversed;
    let sk = feats.iter_mut().find(|f| {
        f.get("type").and_then(Value::as_str) == Some("sketch")
            && f.get("name").and_then(Value::as_str) == Some(profile)
    })?;
    let (o, x, y) = sketch_frame(sk)?;
    // The profile's level on the axis: the middle of its curves' points.
    let level = {
        let pts = sketch_points(sk)?;
        let mut used: Vec<&str> = Vec::new();
        for c in sk.get("curves")?.as_array()? {
            if c.get("construction").is_some() {
                continue;
            }
            for k in ["start", "end", "center"] {
                if let Some(id) = c.get(k).and_then(Value::as_str) {
                    used.push(id);
                }
            }
        }
        let lv: Vec<f64> = used
            .iter()
            .filter_map(|id| pts.get(id))
            .map(|&(u, v)| dot(sub(comb(comb(o, 1.0, x, u), 1.0, y, v), h.centre), up))
            .collect();
        let lo = lv.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = lv.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (lo + hi) / 2.0
    };
    let climb = if level.abs() <= h.pitch {
        up
    } else if (level - h.height).abs() <= h.pitch {
        up.map(|c| -c)
    } else {
        return None;
    };
    let k = climb.iter().position(|c| (c.abs() - 1.0).abs() < 1e-9)?;
    if h.centre.iter().enumerate().any(|(i, c)| i != k && c.abs() > 1e-6) {
        return None;
    }
    let mut hx = json!({
        "origin": json_v3(h.centre.map(|c| c + 0.0)),
        "axis": json_v3(climb.map(|c| c.round() + 0.0)),
        "radius": h.radius,
        "pitch": round_to(h.pitch, 9),
        "height": round_to(h.height, 9),
        "left_handed": left_handed,
    });
    if climb.get(k).is_some_and(|c| *c > 0.0) {
        return Some(hx);
    }
    let n = cross(x, y);
    let mut ends = Vec::new();
    for p in [h.centre, comb(h.centre, 1.0, climb, 10.0)] {
        let q = sub(p, o);
        if dot(q, n).abs() > 1e-6 {
            return None;
        }
        ends.push((round_to(dot(q, x), 9) + 0.0, round_to(dot(q, y), 9) + 0.0));
    }
    let [(u0, v0), (u1, v1)] = ends.as_slice() else { return None };
    sk.get_mut("points")?.as_array_mut()?.extend([
        json!({"id": "helix_axis_start", "x": u0, "y": v0, "construction": true}),
        json!({"id": "helix_axis_end", "x": u1, "y": v1, "construction": true}),
    ]);
    sk.get_mut("curves")?.as_array_mut()?.push(json!({
        "id": "helix_axis", "type": "line", "start": "helix_axis_start", "end": "helix_axis_end", "construction": true,
    }));
    hx.as_object_mut()?.insert("line".into(), json!("helix_axis"));
    Some(hx)
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

/// The motion from one copy of a pattern to the next: a turn about an axis (a point on it and
/// its unit direction, mm; the angle in radians, right-handed about the direction) or a shift
/// (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Turn(V3, V3, f64),
    Shift(V3),
}

/// The step of a pattern of one direction as the tree writes it: a circular pattern's angle
/// spread over its copies (all the way round: count steps), a linear one's spacing.
fn pattern_step(f: &Value) -> Option<Step> {
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    let count = f.get("count")?.as_f64().filter(|c| *c >= 2.0)?;
    match f.get("type")?.as_str()? {
        "pattern_circular" => {
            let axis = f.get("axis")?;
            let (o, d) = (v3(axis.get("origin"))?, v3(axis.get("direction"))?);
            let len = libm::sqrt(dot(d, d));
            let total = f.get("angle")?.as_f64()?.to_radians();
            let step = if (total - TAU).abs() < 1e-9 { total / count } else { total / (count - 1.0) };
            (len > 1e-12).then(|| Step::Turn(o, d.map(|c| c / len), step))
        }
        "pattern_linear" => {
            let d = v3(f.get("direction"))?;
            let len = libm::sqrt(dot(d, d));
            let sp = f.get("spacing")?.as_f64()?;
            (len > 1e-12).then(|| Step::Shift(d.map(|c| c * sp / len)))
        }
        _ => None,
    }
}

/// A direction moved `k` steps (a shift leaves it).
fn step_dir(s: Step, k: f64, v: V3) -> V3 {
    match s {
        Step::Turn(_, u, a) => {
            // Rodrigues: v cos + (u x v) sin + u (u . v)(1 - cos).
            let (sn, cs) = (libm::sin(a * k), libm::cos(a * k));
            let w = cross(u, v);
            let d = dot(u, v) * (1.0 - cs);
            [v[0] * cs + w[0] * sn + u[0] * d, v[1] * cs + w[1] * sn + u[1] * d, v[2] * cs + w[2] * sn + u[2] * d]
        }
        Step::Shift(_) => v,
    }
}

/// A point moved `k` steps.
fn step_point(s: Step, k: f64, p: V3) -> V3 {
    match s {
        Step::Turn(o, _, _) => comb(o, 1.0, step_dir(s, k, sub(p, o)), 1.0),
        Step::Shift(t) => comb(p, 1.0, t, k),
    }
}

/// A pattern's step, the copy's number, an edge's references moved back and the extrusions they name.
type Patterned = (Step, f64, Vec<Rep>, HashMap<u32, Swept>);

/// Fake ids of extrusions moved back from a pattern's copy (feature ids are far below).
const MOVED: u32 = 0x4000_0000;

/// An edge of a pattern's copy (every copy reference through the same pattern and copy, the
/// other references to extrusions): the seed's edge problem, with the other faces moved back
/// to the seed under fake ids; its midpoint is moved forward again by `k` steps.
fn patterned_edge(reps: &[Rep], swept: &HashMap<u32, Swept>, steps: &HashMap<u32, Step>) -> Option<Patterned> {
    let first = reps.iter().find(|r| r.via.is_some())?;
    let (m, copy) = (first.via?, first.copy);
    let step = *steps.get(&m)?;
    if copy == 0 {
        return None;
    }
    let k = f64::from(copy);
    let mut sw = swept.clone();
    let mut out = Vec::with_capacity(reps.len());
    for r in reps {
        match r.via {
            Some(v) if v == m && r.copy == copy => out.push(Rep { via: None, copy: 0, ..*r }),
            Some(_) => return None,
            None => {
                let f = swept.get(&r.fid)?;
                let jp = JsonPlane {
                    origin: step_point(step, -k, f.plane.origin),
                    x: step_dir(step, -k, f.plane.x),
                    y: step_dir(step, -k, f.plane.y),
                };
                sw.insert(r.fid | MOVED, Swept { plane: jp, ..*f });
                out.push(Rep { fid: r.fid | MOVED, ..*r });
            }
        }
    }
    Some((step, k, out, sw))
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
            "loft" => {
                solids += 1;
                if f.get("unresolved").is_some() {
                    return Err(format!("{name}: the loft sections were not found"));
                }
            }
            "sweep" => {
                solids += 1;
                if f.get("unresolved").is_some() {
                    return Err(format!("{name}: the sweep's profile and path were not found"));
                }
            }
            "draft" if f.get("unresolved").is_some() => {
                return Err(format!("{name}: the draft's neutral plane or faces were not found"));
            }
            "hole" | "mirror" | "shell" if f.get("unresolved").is_some() => {
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
        let f = Swept { sketch: 0, plane, distance: 5.0, symmetric: false, through: false, cut: false };
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
    fn a_cap_of_a_profile_of_arcs_is_picked_inside_its_largest_arc() {
        // A disc of r 10 mm (a full circle, key 2) extruded 5 mm: its far cap, picked just
        // inside the circle opposite its start.
        let circle = Seg {
            kind: SegKind::Arc,
            id: 2,
            a: 2,
            s: 1,
            e: 1,
            c: 0,
            dir: 1,
            construction: false,
            centerline: false,
            external: false,
        };
        let g = Geometry { pts: vec![pt(0.0, 0.0), pt(0.01, 0.0)], segs: vec![circle], ellipse_axes: Vec::new() };
        let plane = JsonPlane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let swept = HashMap::from([(
            40,
            Swept { sketch: 0, plane, distance: 5.0, symmetric: false, through: false, cut: false },
        )]);
        let r = Rep { tag: 9, fid: 40, idx: 1, ext: false, ext2: false, via: None, copy: 0 };
        let (p, n, _) = face_pick(&r, &swept, &[g], &HashSet::from([9u16])).unwrap();
        assert_eq!(n, [0.0, 0.0, 1.0]);
        assert!((p[0] + 9.899).abs() < 1e-6 && p[1].abs() < 1e-6 && (p[2] - 5.0).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn a_projected_point_without_a_chain_is_read_where_it_is() {
        // 40 header bytes (the reference word at 35 is 0), then 0.0 and 1.0, then the point.
        let mut d = vec![0u8; 40];
        d.extend(ONE);
        d.extend([0x1e, 0]);
        assert_eq!(payload(&d, 0), Some((Some(56), false)));
        // A projected point writes a 1 byte into the 0.0: still its own payload, marked external.
        let mut e = vec![0u8; 40];
        e.extend([0, 3, 0, 0, 0, 0, 0, 0]);
        e.extend(ONE.get(8..).unwrap());
        e.extend([0x1e, 0]);
        // A later entity's 0.0/1.0 pair must not be taken instead.
        e.extend([0u8; 40]);
        e.extend(ONE);
        assert_eq!(payload(&e, 0), Some((Some(56), true)));
    }

    #[test]
    fn a_converted_segment_without_a_chain_is_read_where_it_is() {
        // Back_sup_low's base line: byte 4 in the 0.0, eight zero bytes, then the segment
        // (points 0 and 1, the word 1, -1.0); the next entity's pair must not be taken.
        let mut e = vec![0u8; 40];
        e.extend([0, 4, 0, 0, 0, 0, 0, 0]);
        e.extend(ONE.get(8..).unwrap());
        e.extend([0u8; 8]);
        e.extend([0, 0, 1, 0, 1, 0, 0, 0]);
        e.extend((-1.0f64).to_le_bytes());
        e.extend([0u8; 40]);
        e.extend(ONE);
        assert_eq!(payload(&e, 0), Some((Some(64), true)));
    }

    #[test]
    fn an_open_revolve_profile_is_closed_along_its_axis() {
        // Tommy Bar: the axis (0,0)-(290,0); a bar from the axis up and along to (275,10), a
        // line back down to (275,0) and a ball arc from (275,10) to (277,0).
        let seg = |kind, s, e, c, construction| Seg {
            kind,
            id: 1,
            a: 1,
            s,
            e,
            c,
            dir: -1,
            construction,
            centerline: construction,
            external: false,
        };
        let g = Geometry {
            pts: vec![
                pt(0.0, 0.0),
                pt(0.29, 0.0),
                pt(0.0, 0.006),
                pt(0.275, 0.01),
                pt(0.275, 0.0),
                pt(0.251, 0.0),
                pt(0.277, 0.0),
            ],
            segs: vec![
                seg(SegKind::Line, 0, 1, 0, true),
                seg(SegKind::Line, 0, 2, 0, false),
                seg(SegKind::Line, 2, 3, 0, false),
                seg(SegKind::Line, 3, 4, 0, false),
                seg(SegKind::Arc, 6, 3, 5, false),
            ],
            ellipse_axes: Vec::new(),
        };
        assert_eq!(axis_closure(&g, 0), vec![(0, 4), (4, 6)]);
        // A closed profile touching the axis has no loose end there.
        let mut closed = g.clone();
        closed.segs.remove(3);
        closed.segs.push(seg(SegKind::Line, 6, 0, 0, false));
        assert!(axis_closure(&closed, 0).is_empty());
    }

    #[test]
    fn a_box_object_gives_the_revolve_axis_point() {
        let mut d = vec![0u8; 3];
        d.extend([1, 0, 0xb7, 0x81, 1, 0, 0, 0]);
        for v in [-0.0032, -0.0088, 0.0, 0.0468] {
            d.extend(f64::to_le_bytes(v));
        }
        d.extend([0u8; 8]);
        let c = revolve_box_centre(&d, 0, d.len()).unwrap();
        assert!((c[0] + 3.2).abs() < 1e-9 && (c[1] + 8.8).abs() < 1e-9, "{c:?}");
        // Older files write -1 for the diagonal; other values are no box.
        d[35..43].copy_from_slice(&(-1.0f64).to_le_bytes());
        assert!(revolve_box_centre(&d, 0, d.len()).is_some());
        d[35..43].copy_from_slice(&(-2.0f64).to_le_bytes());
        assert_eq!(revolve_box_centre(&d, 0, d.len()), None);
    }

    #[test]
    fn an_extrude_draft_is_a_placed_half_degree_d3() {
        // The value at 0, its display position 31 bytes on.
        let slot = |v: f64, pos: f64| {
            let mut d = f64::to_le_bytes(v).to_vec();
            d.resize(31, 0);
            d.extend(f64::to_le_bytes(pos));
            d.extend([0u8; 8]);
            d
        };
        let names = vec![("D3".to_string(), 0usize)];
        let deg45 = TAU / 8.0;
        assert_eq!(extrude_draft(&slot(deg45, 0.012), &names), Some(deg45));
        // Unplaced (Toolbox washers), not whole half degrees (a start offset in metres), or
        // another parameter: no draft.
        assert_eq!(extrude_draft(&slot(deg45, 0.0), &names), None);
        assert_eq!(extrude_draft(&slot(0.0008, 0.012), &names), None);
        assert_eq!(extrude_draft(&slot(deg45, 0.012), &[("D4".to_string(), 0)]), None);
    }

    #[test]
    fn a_revolved_rim_cut_by_two_flats_is_the_piece_its_vertex_faces_bound() {
        // A ring about the y axis of the Front plane: r 11 to 21 mm, y 0 to 6 mm; the rim
        // between the outer wall (key 2) and the top (key 3) is the circle r 21 at y 6.
        let seg = |a, s, e| Seg {
            kind: SegKind::Line,
            id: a,
            a,
            s,
            e,
            c: 0,
            dir: 1,
            construction: false,
            centerline: false,
            external: false,
        };
        let sketches = vec![Geometry {
            pts: vec![pt(0.011, 0.0), pt(0.021, 0.0), pt(0.021, 0.006), pt(0.011, 0.006)],
            segs: vec![seg(1, 0, 1), seg(2, 1, 2), seg(3, 2, 3), seg(4, 3, 0)],
            ellipse_axes: Vec::new(),
        }];
        let on = Swept {
            sketch: 0,
            plane: JsonPlane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] },
            distance: 0.0,
            symmetric: false,
            through: false,
            cut: false,
        };
        let axis =
            [Pt { x: 0.0, y: 0.0, ext: false, id: 0, alone: 0 }, Pt { x: 0.0, y: 0.01, ext: false, id: 0, alone: 0 }];
        let revolved = HashMap::from([(34, Revolved { on, axis })]);
        let rep = |fid, idx| Rep { tag: 1, fid, idx, ext: false, ext2: false, via: None, copy: 0 };
        let reps = [rep(34, 2), rep(34, 3), rep(55, 2), rep(55, 6)];
        // Uncut: a point opposite the profile, and the whole circle.
        let (m, path) = revolve_circle(&reps, &revolved, &sketches, None).unwrap();
        assert_eq!(m, [-21.0, 6.0, 0.0]);
        assert_eq!(path.len(), 201);
        // Flats at x = -15 and x = 15 leave two arcs, |x| < 15; the vertex faces' order picks one.
        let (left, right) = (([-15.0, 0.0, 0.0], [-1.0, 0.0, 0.0]), ([15.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        let (m, path) = revolve_circle(&reps, &revolved, &sketches, Some([left, right])).unwrap();
        assert_eq!(m, [0.0, 6.0, -21.0]);
        let ends = (path.first().copied().unwrap(), path.last().copied().unwrap());
        assert!((ends.0[0] + 15.0).abs() < 1e-6 && (ends.1[0] - 15.0).abs() < 1e-6, "{ends:?}");
        let (m, _) = revolve_circle(&reps, &revolved, &sketches, Some([right, left])).unwrap();
        assert_eq!(m, [0.0, 6.0, 21.0]);
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
        swept.insert(33, Swept { sketch: 1, plane, distance: -10.0, symmetric: false, through: false, cut: false });
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
            cut: false,
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
        let rep = |tag, fid, idx| Rep { tag, fid, idx, ext: false, ext2: false, via: None, copy: 0 };
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
        swept.insert(33, Swept { sketch: 1, plane, distance: -500.0, symmetric: false, through: true, cut: false });
        let cap_tags = HashSet::from([9u16]);
        let rep = |tag, fid, idx| Rep { tag, fid, idx, ext: false, ext2: false, via: None, copy: 0 };
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
        let rep = |idx, via| Rep { tag: 9, fid: 32, idx, ext: false, ext2: false, via, copy: 0 };
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
        let recs = edge_records_to(&d, 0, d.len(), &HashSet::from([59]), &HashSet::new()).unwrap();
        let reps = &recs[0].2;
        assert_eq!((reps[0].fid, reps[0].idx, reps[0].via), (55, 3, Some(59)));
        assert_eq!((reps[1].fid, reps[1].via), (30, None));
        // Unless the mirror is known, the record is not read so.
        assert!(
            edge_records_to(&d, 0, d.len(), &HashSet::new(), &HashSet::new()).unwrap().iter().all(|r| r.2.len() != 2)
        );
    }

    #[test]
    fn a_face_of_a_pattern_copy_carries_the_copy_number() {
        let word = [0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54];
        let mut d = vec![0u8; 4];
        d.extend([1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        d.extend(word);
        d.extend(word);
        d.extend([0, 0]);
        // A cap of Boss-Extrude1 (id 30), then CirPattern1 (id 62) copy 5 of Cut-Extrude1 (id 58) face 2.
        d.extend([0x94, 0x81, 0, 0, 0x91, 0x81, 0x93, 1, 30, 0, 0, 0, 0x0d, 0x4c, 0x1e, 0x52, 0, 0, 0, 0, 2, 0, 0, 0]);
        d.extend([0xa7, 0x84, 0, 0, 0x91, 0x81, 0x93, 1, 62, 0, 0, 0, 0x4c, 0x2d, 0x2a, 0x52]);
        d.extend([0xfb, 0x83, 0, 0, 0x91, 0x81, 0x93, 1, 58, 0, 0, 0, 0x37, 0x2d, 0x2a, 0x52, 2, 0, 0, 0]);
        d.extend([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 5, 0, 0, 0]);
        d.extend([0u8; 12]);
        let recs = edge_records_to(&d, 0, d.len(), &HashSet::new(), &HashSet::from([62])).unwrap();
        let reps = &recs[0].2;
        assert_eq!((reps[0].fid, reps[0].via, reps[0].copy), (30, None, 0));
        assert_eq!((reps[1].fid, reps[1].idx, reps[1].via, reps[1].copy), (58, 2, Some(62), 5));
        // Unless the pattern is known, the record is not read so.
        let plain = edge_records_to(&d, 0, d.len(), &HashSet::new(), &HashSet::new()).unwrap();
        assert!(plain.iter().flat_map(|r| &r.2).all(|r| r.via.is_none() && r.copy == 0 && r.fid != 58));
    }

    #[test]
    fn a_pattern_step_moves_points_by_the_copy_number() {
        let turn = pattern_step(&json!({"type": "pattern_circular", "count": 6, "angle": 360.0,
            "axis": {"origin": [0.0, 0.0, 0.0], "direction": [0.0, 0.0, 2.0]}}))
        .unwrap();
        let p = step_point(turn, 3.0, [1.0, 0.0, 5.0]);
        assert!((p[0] + 1.0).abs() < 1e-12 && p[1].abs() < 1e-12 && (p[2] - 5.0).abs() < 1e-12);
        let q = step_point(turn, 1.0, [1.0, 0.0, 0.0]);
        assert!((q[0] - 0.5).abs() < 1e-12 && (q[1] - libm::sqrt(3.0) / 2.0).abs() < 1e-12);
        // A partial circle spreads its angle over count - 1 steps.
        let part = pattern_step(&json!({"type": "pattern_circular", "count": 3, "angle": 90.0,
            "axis": {"origin": [0.0, 0.0, 0.0], "direction": [0.0, 0.0, 1.0]}}));
        assert!(matches!(part, Some(Step::Turn(_, _, a)) if (a - TAU / 8.0).abs() < 1e-12));
        let shift =
            pattern_step(&json!({"type": "pattern_linear", "count": 4, "direction": [2.0, 0.0, 0.0], "spacing": 5.0}))
                .unwrap();
        assert_eq!(step_point(shift, 2.0, [1.0, 1.0, 1.0]), [11.0, 1.0, 1.0]);
        assert_eq!(step_dir(shift, 2.0, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_draft_ends_with_a_point_on_each_face() {
        let mut d = vec![9u8; 7];
        d.extend(2u32.to_le_bytes());
        for v in [0.001f64, 0.002, 0.003, -0.004, 0.0, 0.0125] {
            d.extend(v.to_le_bytes());
        }
        d.extend([0u8; 8]);
        assert_eq!(draft_points(&d), vec![[1.0, 2.0, 3.0], [-4.0, 0.0, 12.5]]);
        assert!(draft_points(&[1u8; 40]).is_empty());
    }

    #[test]
    fn a_cap_or_a_side_of_an_extruded_profile_is_picked_with_its_outward_normal() {
        let (swept, sketches) = plate();
        let rep = |idx| Rep { tag: 9, fid: 32, idx, ext: false, ext2: false, via: None, copy: 0 };
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
        let side = |idx| Rep { tag: 4, fid: 32, idx, ext: false, ext2: false, via: None, copy: 0 };
        let cap = Rep { tag: 9, fid: 32, idx: 1, ext: false, ext2: false, via: None, copy: 0 };
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
        // Version word 6000 (V-Slot Actuator End Mount's moPointRef_w): no zero bytes.
        let mut d = vec![0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0x70, 0x17, 0, 0];
        for v in [0.087, 0.0035, -0.0019] {
            d.extend(f64::to_le_bytes(v));
        }
        assert_eq!(point_ref(&d, 0, d.len()), Some([87.0, 3.5, -1.9]));
    }

    /// A line of one extrusion ending on a circle of another (Biyel Kolu's Radyus3) meets it
    /// there although its polyline need not cross the circle's.
    #[test]
    fn a_line_ending_on_a_circle_touches_it() {
        let pt = |x: f64, y: f64| Some(Pt { x, y, ext: false, id: 0, alone: 0 });
        let seg = |kind, s, e, c| Seg {
            kind,
            id: 0,
            a: 1,
            s,
            e,
            c,
            dir: 1,
            construction: false,
            centerline: false,
            external: false,
        };
        let line = Geometry {
            pts: vec![pt(-0.02, 0.0), pt(-0.01, 0.08)],
            segs: vec![seg(SegKind::Line, 0, 1, 0)],
            ..Default::default()
        };
        // The circle about (0, 0.08 + sqrt(0.019^2 - 0.01^2)) of radius 19 mm passes through the
        // line's end (-0.01, 0.08).
        let cy = 0.08 + libm::sqrt(0.019 * 0.019 - 0.0001);
        let circle = Geometry {
            pts: vec![pt(0.0, cy), pt(0.019, cy)],
            segs: vec![seg(SegKind::Arc, 1, 1, 0)],
            ..Default::default()
        };
        let plane = JsonPlane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let f = Swept { sketch: 0, plane, distance: 12.0, symmetric: false, through: false, cut: false };
        let g = Swept { sketch: 1, plane, distance: 24.0, symmetric: false, through: false, cut: false };
        let t = touches((&f, &line, line.segs.first().unwrap()), (&g, &circle, circle.segs.first().unwrap()));
        assert_eq!(t.len(), 1, "{t:?}");
        let p = t.first().unwrap();
        assert!((p[0] + 10.0).abs() < 1e-9 && (p[1] - 80.0).abs() < 1e-9 && p[2].abs() < 1e-9, "{p:?}");
    }

    /// A plane reference (end_cap's up-to-surface end on the Top plane): four extents, the
    /// origin and the normal after the marker.
    #[test]
    fn a_plane_reference_gives_its_plane() {
        let mut d = vec![0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0x70, 0x17, 0, 0];
        for v in [0.011, -0.011, 0.011, -0.011, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            d.extend(f64::to_le_bytes(v));
        }
        assert_eq!(plane_ref(&d, 0, d.len()), Some(([0.0; 3], [0.0, 1.0, 0.0])));
        let mut d = vec![0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0, 0, 0, 0, 0x28, 0x23, 0, 0];
        for v in [0.0, -1.0, 1.0, 1.0, -1.0, 0.0975, 0.012, -0.001, 1.0, 0.0, 0.0] {
            d.extend(f64::to_le_bytes(v));
        }
        assert_eq!(plane_ref(&d, 0, d.len()), Some(([97.5, 12.0, -1.0], [1.0, 0.0, 0.0])));
        assert_eq!(plane_ref(&d[..40], 0, 40), None);
    }

    /// End specs: byte 8 flips the side of a cut (BK12_nut); a direction given by a plane
    /// reference puts the fields after it (Bottom-Plate's Cut-Extrude3: up to a surface).
    #[test]
    fn end_specs_of_flipped_cuts_and_referenced_directions() {
        let mut d = vec![0x97, 0x81, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
        let e = endspec_candidates(&d, 0, d.len()).unwrap();
        let [e] = e.as_slice() else { panic!("{e:?}") };
        assert!(e.flip && e.kind == 1 && e.flag == 0 && e.x == 1 && e.b12 == 0);
        d.clear();
        d.extend(b"\xf0\x87\xff\xff\x01\x00\x11\x00moDirectionSpec_c\x01\x00\x00\x00\x01\x00");
        d.extend(b"\xff\xff\x01\x00\x0c\x00moPlaneRef_w");
        d.extend([0u8; 40]);
        d.extend([3, 0, 0, 0, 1, 0, 0, 0]);
        d.extend([0u8; 10]);
        d.extend([4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let e = endspec_directed(&d, 0, d.len()).unwrap();
        assert!(!e.flip && e.kind == 4 && e.flag == 0 && e.x == 1 && e.b12 == 0);
    }

    /// Node names inside the header of the node before them: a reused sketch's `Sketch3<3>` and
    /// an attribute go; a hole wizard's nested sketch stays.
    #[test]
    fn an_extrude_takes_the_sketch_its_child_index_names() {
        let order: Vec<String> = ["Sketch1", "Sketch2", "Sketch3"].map(String::from).to_vec();
        let last = Some("Sketch3".to_string());
        let mut seen = vec![(0xd8u16, "Sketch1".to_string()), (0x3d2, "Sketch3".to_string())];
        let mut used: HashSet<String> = seen.iter().map(|(_, s)| s.clone()).collect();
        // Between Sketch1's index and Sketch3's: the one sketch not yet used.
        assert_eq!(feature_sketch(Some(0x22c), &last, &order, &seen, &used).as_deref(), Some("Sketch2"));
        // An index seen names its sketch again; a larger one, or none, the last sketch.
        assert_eq!(feature_sketch(Some(0xd8), &last, &order, &seen, &used).as_deref(), Some("Sketch1"));
        assert_eq!(feature_sketch(Some(0x500), &last, &order, &seen, &used).as_deref(), Some("Sketch3"));
        assert_eq!(feature_sketch(None, &last, &order, &seen, &used).as_deref(), Some("Sketch3"));
        // Two free sketches in between: the last sketch, as before.
        seen.retain(|(_, s)| s != "Sketch1");
        used.remove("Sketch1");
        assert_eq!(feature_sketch(Some(0x22c), &last, &order, &seen, &used).as_deref(), Some("Sketch3"));
    }

    #[test]
    fn suppressed_features_are_marked_by_their_own_block() {
        // Writes a node block at `k` with the suppression word `w` 47 bytes before it.
        let put = |d: &mut Vec<u8>, k: usize, w: u32| {
            d[k - 47..k - 43].copy_from_slice(&w.to_le_bytes());
            d[k - 4..k - 1].copy_from_slice(&UTF16_MARK);
            d[k..k + 18].copy_from_slice(&NODE_BLOCK);
        };
        let mut d = vec![0u8; 1000];
        put(&mut d, 100, 1); // a suppressed cut (name ends at 20)
        put(&mut d, 300, 1); // a cosmetic thread nested in a revolve, suppressed
        put(&mut d, 500, 0); // the revolve's own block, after the thread's
        put(&mut d, 800, 7); // not a suppression word
        let node = |start: usize, name_end: usize, k: usize| Node {
            start,
            cls: None,
            tag: None,
            name: String::new(),
            flags: 0,
            id: 0,
            post: k + 56,
            head_len: 0,
            name_end,
            suppressed: false,
            child: None,
        };
        let mut nodes = vec![node(0, 20, 100), node(200, 210, 300), node(220, 230, 300), node(700, 720, 800)];
        mark_suppressed(&d, &mut nodes);
        assert_eq!(nodes.iter().map(|n| n.suppressed).collect::<Vec<_>>(), [true, false, true, false]);
    }

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
            name_end: 0,
            suppressed: false,
            child: None,
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
            name_end: 0,
            suppressed: false,
            child: None,
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
        assert_eq!(selection(recs, 9).len(), 3);
        // Short records keep the 0xa0 rule.
        assert_eq!(selection(vec![(0, 60, vec![]), (150, 210, vec![]), (400, 460, vec![])], 9).len(), 2);
        // Records naming the feature's own faces (id 9) come before its picks and are passed
        // over; with nothing else, they are what there is.
        let rep = |fid| Rep { tag: 1, fid, idx: 1, ext: false, ext2: false, via: None, copy: 0 };
        let own = || vec![rep(9), rep(5), rep(5), rep(5), rep(5)];
        let pick = || vec![rep(5), rep(5), rep(5), rep(5)];
        let recs =
            vec![(0, 150, own()), (300, 450, own()), (600, 730, pick()), (736, 866, pick()), (2000, 2130, pick())];
        let sel = selection(recs, 9);
        assert_eq!(sel.iter().map(|(at, _)| *at).collect::<Vec<_>>(), [600, 736]);
        assert_eq!(selection(vec![(0, 150, own()), (300, 450, own())], 9).len(), 1);
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

    #[test]
    fn a_circular_sweep_profile_is_square_to_the_path_start() {
        // An L path on the Front plane: a line from (183, 0) to (10, 0), a quarter arc about
        // (10, -10) down to (0, -10), then a line to (0, -110).
        let path = json!({"type": "sketch", "name": "P",
            "plane": {"origin": [0.0, 0.0, 0.0], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]},
            "points": [{"id": "p0", "x": 10.0, "y": -10.0}, {"id": "p1", "x": 183.0, "y": 0.0},
                       {"id": "p3", "x": 10.0, "y": 0.0}, {"id": "p4", "x": 0.0, "y": -110.0},
                       {"id": "p5", "x": 0.0, "y": -10.0}],
            "curves": [{"id": "c0", "type": "line", "start": "p3", "end": "p1"},
                       {"id": "c1", "type": "line", "start": "p5", "end": "p4"},
                       {"id": "c2", "type": "arc", "center": "p0", "start": "p3", "end": "p5"}]});
        let sk = circle_at_path_start(&path, 7.0, "S_Profile").unwrap();
        assert_eq!(sk["plane"]["origin"], json!([183.0, 0.0, 0.0]));
        let (x, y) = (&sk["plane"]["x_axis"], &sk["plane"]["y_axis"]);
        let v = |a: &Value| [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()];
        // The plane's normal runs along the path, into it (-x).
        let n = cross(v(x), v(y));
        assert!((n[0] + 1.0).abs() < 1e-12 && n[1].abs() < 1e-12 && n[2].abs() < 1e-12, "{n:?}");
        assert_eq!(sk["curves"][0]["radius"], json!(3.5));
        // A closed path has no start.
        let mut closed = path.clone();
        closed["curves"][0]["end"] = json!("p4");
        assert!(circle_at_path_start(&closed, 7.0, "S").is_none());
    }

    #[test]
    fn a_helix_record_gives_its_flags_height_and_pitch() {
        // The Shaft Couplers' record: start 135 degrees, clockwise, reversed, 9.2 mm of 5.1 turns.
        let mut d = vec![0xaa; 7];
        d.extend([6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        d.extend((0.75 * core::f64::consts::PI).to_le_bytes());
        d.extend([1, 0, 0, 0, 1, 0, 0, 0]);
        let pitch = 0.0092 / 5.1;
        for v in [0.0092, pitch, 5.1] {
            d.extend(f64::to_le_bytes(v));
        }
        let (cw, rev, h, p) = helix_record(&d, 0, d.len()).unwrap();
        assert!(cw && rev);
        assert!((h - 9.2).abs() < 1e-9 && (p - pitch * 1000.0).abs() < 1e-6, "{h} {p}");
        // A height that is not pitch times turns is no record.
        let at = d.len() - 8;
        d.splice(at.., 5.2f64.to_le_bytes());
        assert!(helix_record(&d, 0, d.len()).is_none());
    }

    #[test]
    fn a_helix_sweep_runs_from_the_end_its_profile_lies_at() {
        // The Shaft Couplers: the helix on a circle of 11.25 mm at z = 6, climbing 9.2 mm up
        // (reversed: along its plane's normal), clockwise; the slot profile on the Right plane
        // at z = 15.1 .. 15.4, at the helix's top end. The tree's helix climbs down from the
        // profile, along a line added to its sketch; a clockwise turn up the normal is left-handed.
        let base = json!({"type": "sketch", "name": "B",
            "plane": {"origin": [0.0, 0.0, 6.0], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]},
            "points": [{"id": "p0", "x": 0.0, "y": 0.0}],
            "curves": [{"id": "c0", "type": "circle", "center": "p0", "radius": 11.25}]});
        let h = helix_of(&base, (true, true, 9.2, 9.2 / 5.1)).unwrap();
        let profile = json!({"type": "sketch", "name": "P",
            "plane": {"origin": [0.0, 0.0, 0.0], "x_axis": [0.0, 0.0, -1.0], "y_axis": [0.0, 1.0, 0.0]},
            "points": [{"id": "p0", "x": -15.1, "y": 12.5}, {"id": "p1", "x": -15.4, "y": 12.5},
                       {"id": "p2", "x": -15.4, "y": 1.1}, {"id": "p3", "x": -15.1, "y": 1.1}],
            "curves": [{"id": "c0", "type": "line", "start": "p0", "end": "p1"},
                       {"id": "c1", "type": "line", "start": "p1", "end": "p2"},
                       {"id": "c2", "type": "line", "start": "p2", "end": "p3"},
                       {"id": "c3", "type": "line", "start": "p3", "end": "p0"}]});
        let mut feats = vec![base, profile.clone()];
        let hx = helix_sweep(&mut feats, "P", &h).unwrap();
        assert_eq!(hx["axis"], json!([0.0, 0.0, -1.0]));
        assert_eq!(hx["left_handed"], json!(true));
        assert_eq!(hx["line"], json!("helix_axis"));
        let line = feats[1]["curves"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(line["construction"], json!(true));
        // The line runs from the axis at z = 6 down the axis (sketch x is -z).
        assert_eq!(feats[1]["points"][4]["x"], json!(-6.0));
        assert_eq!(feats[1]["points"][5]["x"], json!(4.0));
        // At the helix's start the profile climbs it up the world axis, with no line.
        let mut low = profile.clone();
        for p in low["points"].as_array_mut().unwrap() {
            let x = p["x"].as_f64().unwrap();
            p["x"] = json!(x + 9.2);
        }
        let mut feats = vec![low];
        let hx = helix_sweep(&mut feats, "P", &h).unwrap();
        assert_eq!(hx["axis"], json!([0.0, 0.0, 1.0]));
        assert!(hx.get("line").is_none());
        // Not reversed, the same helix climbs down from z = 6 and turns the other way round it.
        let h2 = Helix { reversed: false, ..h.clone() };
        let mut feats = vec![profile.clone()];
        assert!(helix_sweep(&mut feats, "P", &h2).is_none(), "the profile is at neither end");
        // A profile half way up lies at neither end.
        let mut mid = profile;
        for p in mid["points"].as_array_mut().unwrap() {
            let x = p["x"].as_f64().unwrap();
            p["x"] = json!(x + 4.6);
        }
        assert!(helix_sweep(&mut [mid], "P", &h).is_none());
    }

    #[test]
    fn lofts_and_sweeps_name_their_sketches_by_id() {
        let comp = |pf: bool, id: u32| {
            let mut v = vec![0x2b, 0x80, 2, 0];
            v.extend([0xff; 16]);
            v.extend([0; 8]);
            if pf {
                // Newer files: more zero bytes and the word `50 46 00 00`.
                v.extend([0; 8]);
                v.extend(0x4650u32.to_le_bytes());
            }
            v.extend(id.to_le_bytes());
            v.extend(0x5f42_c419u32.to_le_bytes());
            v.extend([0; 8]);
            v.extend(0x65u32.to_le_bytes());
            v
        };
        let ids: HashMap<u32, String> = [(45, "Sketch4".into()), (49, "Sketch5".into())].into();
        for pf in [false, true] {
            let mut d = vec![1u8; 10];
            // A solid-body component: its id is no sketch.
            d.extend(comp(pf, 0x180fb));
            d.extend(comp(pf, 49));
            d.extend(comp(pf, 45));
            d.extend(comp(pf, 49));
            // An id without a time stamp after it is not a section.
            d.extend([0, 0, 0, 0, 0, 0, 0, 0, 45, 0, 0, 0, 1, 0, 0, 0]);
            assert_eq!(named_sketches(&d, 0, d.len(), &ids), ["Sketch5", "Sketch4"]);
            assert_eq!(named_sketches(&d, 0, 40, &ids), Vec::<String>::new());
        }
    }
}
