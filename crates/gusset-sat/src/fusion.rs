//! Component placements in a Fusion 360 design stream (`Design1/BulkStream.dat`, newer
//! files `FusionDesignSegmentType1/`), read clean-room from sample files (ADR 0039
//! amendments of 2026-11-04 and 2026-11-08).
//!
//! What the samples show. Names are UTF-16LE strings; ids are UUIDs, written as UTF-16LE text
//! (a trailing `$` marks some) and, in the newer layout, as class tags in front of records
//! (`24 00 00 00` and 36 ASCII characters). Matrices are rigid 4×4 (16 little-endian doubles,
//! row-major, last row 0 0 0 1, centimetres). Each component lists its bodies as
//! `BREP.<guid>.smb` (the `Breps.BlobParts` entry holding the body, in the component's own
//! coordinates).
//!
//! * Older layout, per component a *block*: the name, the body, then either its listed
//!   instances (matrix, 4-byte length, `<component>:<instance>`) or `CreateComponent` and,
//!   about 1 to 2.5 KB later, one matrix: the component's placement in design coordinates.
//!   Listed matrices are relative to the last block placed that way (an electronics board);
//!   a `CreateComponent` matrix is trusted only in designs that list instances. Bodies with
//!   neither stay put (the root component's).
//! * Newer layout, per use of a component an *occurrence record* naming its parent
//!   occurrence, its own id and its component; the matrix is in a transform record that names
//!   its owner by record number or by id. A body is placed once per occurrence of its
//!   component, the parents' placements composed outermost first.
//! * References to other designs (`.f3z`) show as transform records keyed by the reference's
//!   `neutronRole` UUID, with one matrix per instance ([`Design::xrefs`]).
//!
//! All reading is bounded and total: noise gives no placements, never a panic.

use std::sync::atomic::{AtomicBool, Ordering};

/// A rigid placement in millimetres: `p' = R p + t` (column vectors), `r` by rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub r: [[f64; 3]; 3],
    pub t: [f64; 3],
}

/// A body's placement and the name of the occurrence that places it.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// The body's blob id (`BREP.<id>.smb`).
    pub brep: String,
    pub occurrence: String,
    pub placement: Placement,
}

/// Most occurrences read from one stream.
const MAX_OCCURRENCES: usize = 10_000;
/// Largest translation accepted (cm): beyond it the numbers are not a placement.
const MAX_OFFSET_CM: f64 = 1e7;

/// The UTF-16LE runs of printable ASCII (at least 3 characters) with their byte offsets.
fn utf16_runs(b: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        let start = i;
        let mut s = String::new();
        while let (Some(&lo), Some(&hi)) = (b.get(i), b.get(i + 1)) {
            if hi == 0 && (0x20..0x7f).contains(&lo) {
                s.push(char::from(lo));
                i += 2;
            } else {
                break;
            }
        }
        if s.len() >= 3 {
            out.push((start, s));
        }
        if i == start {
            i += 1;
        }
    }
    out
}

fn f64_at(b: &[u8], at: usize) -> Option<f64> {
    let s: [u8; 8] = b.get(at..at + 8)?.try_into().ok()?;
    Some(f64::from_le_bytes(s))
}

/// A rigid matrix starting at `at`: finite, orthonormal rows, determinant +1, last row
/// 0 0 0 1, a translation within reach.
pub(crate) fn rigid_at(b: &[u8], at: usize) -> Option<Placement> {
    // The last element (1.0) first: most offsets fail here, cheaply.
    if f64_at(b, at + 120).map(f64::to_bits) != Some(1.0f64.to_bits()) {
        return None;
    }
    let v: Vec<f64> = (0..16).map(|k| f64_at(b, at + 8 * k)).collect::<Option<_>>()?;
    let g = |i: usize| v.get(i).copied().unwrap_or(f64::NAN);
    // The stored markers are exact: compare bit patterns (-0.0 counts as 0).
    let is = |x: f64, want: f64| x.to_bits() == want.to_bits() || (want == 0.0 && x.to_bits() == (-0.0f64).to_bits());
    if !(is(g(12), 0.0) && is(g(13), 0.0) && is(g(14), 0.0) && is(g(15), 1.0)) || !v.iter().all(|x| x.is_finite()) {
        return None;
    }
    let r = [[g(0), g(1), g(2)], [g(4), g(5), g(6)], [g(8), g(9), g(10)]];
    let dot = |a: [f64; 3], c: [f64; 3]| a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
    let [a, c, d] = r;
    let unit = [a, c, d].iter().all(|row| (dot(*row, *row) - 1.0).abs() < 1e-6);
    let orth = dot(a, c).abs() < 1e-6 && dot(a, d).abs() < 1e-6 && dot(c, d).abs() < 1e-6;
    let det =
        a[0] * (c[1] * d[2] - c[2] * d[1]) - a[1] * (c[0] * d[2] - c[2] * d[0]) + a[2] * (c[0] * d[1] - c[1] * d[0]);
    let t = [g(3), g(7), g(11)];
    if !(unit && orth && (det - 1.0).abs() < 1e-6 && t.iter().all(|x| x.abs() <= MAX_OFFSET_CM)) {
        return None;
    }
    crate::cover::field(b, at, 128, "placement.matrix");
    Some(Placement { r, t: t.map(|x| x * 10.0) })
}

/// The id in `BREP.<id>.smb` (not `.smbh`, which holds history).
fn brep_id(s: &str) -> Option<String> {
    let rest = s.strip_prefix("BREP.")?;
    let (id, tail) = rest.split_once('.')?;
    let ok = tail.starts_with("smb") && !tail.starts_with("smbh") && !tail.starts_with("smbB");
    (ok && !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .then(|| id.to_string())
}

/// How the block rules placed a body.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockKind {
    /// Among the listed instances of a component (`name:instance`, each with its matrix).
    Listed,
    /// By the matrix that follows the component's `CreateComponent`.
    World,
}

fn is_uuid_text(s: &str) -> bool {
    s.len() >= 36
        && s.get(8..9) == Some("-")
        && s.get(13..14) == Some("-")
        && s.get(..8).is_some_and(|h| h.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The component name of the body at run `k`: the nearest earlier text that follows two ids.
fn block_name(runs: &[(usize, String)], k: usize) -> Option<&str> {
    let lo = k.saturating_sub(48);
    (lo..k).rev().find_map(|j| {
        let (_, s) = runs.get(j)?;
        let two_ids = j >= 2
            && runs.get(j - 1).is_some_and(|r| is_uuid_text(&r.1))
            && runs.get(j - 2).is_some_and(|r| is_uuid_text(&r.1));
        (two_ids && !is_uuid_text(s) && s.len() >= 3 && !s.starts_with("Prism") && !s.contains('$'))
            .then_some(s.as_str())
    })
}

/// Bodies placed by the older layout's blocks (module doc): a component's name, its body, then
/// either the listed instances (`matrix`, `name:instance`, ...) or `CreateComponent` followed by
/// the matrix of the component's first occurrence in design coordinates. Listed instances are
/// relative to the component they were created in, which is the last block placed by a
/// `CreateComponent` matrix. Bodies with neither stay in the root.
fn block_placements(bulk: &[u8], runs: &[(usize, String)]) -> (Vec<(Placed, BlockKind)>, Option<Placement>) {
    // Bodies, with their run index.
    let bodies: Vec<(usize, usize, String)> = runs
        .iter()
        .enumerate()
        .flat_map(|(k, (o, s))| {
            s.match_indices("BREP.").filter_map(move |(i, _)| brep_id(s.get(i..)?).map(|id| (k, o + 2 * i, id)))
        })
        .collect();
    // Listed instances: a name with a colon right after its matrix (4 bytes: the text's length).
    let mut listed: Vec<(usize, String, Placement)> = Vec::new();
    let mut creates: Vec<usize> = Vec::new();
    for (o, s) in runs {
        if s.starts_with("CreateComponent") {
            creates.push(*o);
        } else if s.contains(':') && !is_uuid_text(s) && s.len() < 80 && !s.starts_with("Prism") {
            let m = o.checked_sub(4 + 128).and_then(|at| rigid_at(bulk, at));
            if let Some(m) = m {
                listed.push((*o, s.clone(), m));
            }
        }
        if listed.len() > MAX_OCCURRENCES {
            break;
        }
    }
    let mut out = Vec::new();
    let mut context = Placement::IDENTITY;
    let mut first_context = None;
    for (i, (k, at, id)) in bodies.iter().enumerate() {
        let end = bodies.get(i + 1).map_or(usize::MAX, |b| b.1);
        let first = listed.partition_point(|l| l.0 < *at);
        let mine: Vec<&(usize, String, Placement)> = listed.iter().skip(first).take_while(|l| l.0 < end).collect();
        if !mine.is_empty() {
            first_context.get_or_insert(context);
            for (_, name, m) in mine {
                out.push((
                    Placed { brep: id.clone(), occurrence: name.clone(), placement: context.then(m) },
                    BlockKind::Listed,
                ));
            }
            continue;
        }
        let c = creates.partition_point(|c| c < at);
        let Some(&cc) = creates.get(c).filter(|&&cc| cc < end) else { continue };
        let reach = (cc + 2600).min(end);
        let m = (cc..reach).find_map(|p| rigid_at(bulk, p));
        if let Some(m) = m {
            context = m;
            let name = block_name(runs, *k).unwrap_or("Component");
            out.push((Placed { brep: id.clone(), occurrence: format!("{name}:1"), placement: m }, BlockKind::World));
        }
    }
    (out, first_context)
}

// ---------------------------------------------------------------------------------------
// Occurrence records (the newer stream layout) and external references

impl Placement {
    /// The placement that leaves points where they are.
    pub const IDENTITY: Placement = Placement { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    /// The placement that undoes `self`.
    #[must_use]
    pub fn inverse(&self) -> Placement {
        let r = [0, 1, 2].map(|i| [0, 1, 2].map(|j| self.r.get(j).and_then(|row| row.get(i)).copied().unwrap_or(0.0)));
        let t = r.map(|row| -row.iter().zip(&self.t).map(|(a, b)| a * b).sum::<f64>());
        Placement { r, t }
    }

    /// `self` applied after `inner`: `p' = self(inner(p))`. Placing a component inside a
    /// component composes the parent's placement with the child's, parent outermost.
    #[must_use]
    pub fn then(&self, inner: &Placement) -> Placement {
        let at = |i: usize, j: usize| self.r.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0.0);
        let inn = |i: usize, j: usize| inner.r.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0.0);
        let mut r = [[0.0; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| at(i, k) * inn(k, j)).sum();
            }
        }
        let mut t = self.t;
        for (i, v) in t.iter_mut().enumerate() {
            *v += (0..3).map(|k| at(i, k) * inner.t.get(k).copied().unwrap_or(0.0)).sum::<f64>();
        }
        Placement { r, t }
    }
}

/// An external reference (xref) of a design: the instances of the referenced design, each
/// the placement of that design's origin in this design (millimetres).
#[derive(Debug, Clone, PartialEq)]
pub struct Xref {
    /// The reference's key, a lowercase UUID; the archive's `DesignDescription.json` names it
    /// `neutronRole` and ties it to the referenced design file.
    pub role: String,
    pub instances: Vec<Placement>,
    /// For each instance, where this design put the referenced design's context component (the
    /// board of an electronics design, [`Design::context`]) in its own coordinates, when it
    /// moved it away from where that design has it.
    pub nested: Vec<Option<Placement>>,
    /// Where this design put each component of the referenced design (its own world
    /// placement, mm), in the order the referenced design lists them; see
    /// [`component_parts`]. Empty when none were read.
    pub parts: Vec<Placement>,
}

/// Class identifiers: each record of the newer layout starts with its class as a
/// length-prefixed (`24 00 00 00`) ASCII UUID.
const CLASS_OCCURRENCE: &str = "FF415D89-A1DB-44AC-B824-E9A772B86ED4";
const CLASS_COMPONENT: &str = "F9CB8717-EB86-47F2-AAA7-39E142C57A09";
/// A transform that names its owner by record number just before it.
const CLASS_TRANSFORM_OF: &str = "2E3AC990-483D-4A55-8662-C064B976A957";
/// A transform that names its owner by a UUID string near it.
const CLASS_TRANSFORM: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
/// A record of this class repeats a reference's matrix and, a few bytes after it, stores the
/// placement of a component inside the referenced design (in the referencing design's
/// coordinates) when the referencing design moved that component.
const CLASS_TRANSFORM_ALSO: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";
/// How far after a reference's matrix that repeat may start (bytes).
const REPEAT_REACH: usize = 3000;
/// Most records of one class read from a stream.
const MAX_RECORDS: usize = 200_000;
/// Most instances of one reference, and how far apart (bytes) the matrices of a list are.
const MAX_INSTANCES: usize = 4096;
const INSTANCE_GAP: usize = 160;
/// Deepest occurrence nesting followed.
const MAX_DEPTH: usize = 32;

/// Offsets of the records of `class` (the start of the length prefix).
fn class_records(b: &[u8], class: &str) -> Vec<usize> {
    let mut needle = vec![0x24u8, 0, 0, 0];
    needle.extend_from_slice(class.as_bytes());
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = b.get(from..).and_then(|rest| rest.iter().position(|&c| c == 0x24)) {
        let at = from + rel;
        if b.get(at..at + needle.len()) == Some(needle.as_slice()) {
            out.push(at);
            if out.len() >= MAX_RECORDS {
                break;
            }
            from = at + needle.len();
        } else {
            from = at + 1;
        }
    }
    out
}

/// A UUID written as UTF-16LE text at `at`: `(lowercase id, end offset, ends in '$')`. The
/// samples mark some ids with a trailing `$`.
fn uuid16_at(b: &[u8], at: usize) -> Option<(String, usize, bool)> {
    let mut id = String::with_capacity(36);
    for k in 0..36 {
        let lo = *b.get(at + 2 * k)?;
        if *b.get(at + 2 * k + 1)? != 0 {
            return None;
        }
        let ok = if matches!(k, 8 | 13 | 18 | 23) { lo == b'-' } else { lo.is_ascii_hexdigit() };
        if !ok {
            return None;
        }
        id.push(char::from(lo.to_ascii_lowercase()));
    }
    let end = at + 72;
    let dollar = b.get(end) == Some(&b'$') && b.get(end + 1) == Some(&0);
    Some((id, if dollar { end + 2 } else { end }, dollar))
}

/// The UUID strings in `b[lo..hi]`: `(start, end, id, ends in '$')`, at most `max`.
fn uuid_strings(b: &[u8], lo: usize, hi: usize, max: usize) -> Vec<(usize, usize, String, bool)> {
    let mut out = Vec::new();
    let mut i = lo;
    let hi = hi.min(b.len());
    while i + 72 <= hi && out.len() < max {
        if b.get(i + 1) == Some(&0)
            && b.get(i).is_some_and(u8::is_ascii_hexdigit)
            && let Some((id, end, dollar)) = uuid16_at(b, i)
        {
            crate::cover::field(b, i, end - i, "placement.uuid");
            out.push((i, end, id, dollar));
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let s: [u8; 8] = b.get(at..at + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(s))
}

/// The record number a record carries after its class: a 3-digit text (length-prefixed)
/// and the number as a little-endian u64.
fn record_number(b: &[u8], at: usize) -> Option<u64> {
    (at + 40..at + 100).find_map(|i| {
        let head = b.get(i..i + 7)?;
        let (len, digits) = head.split_at(4);
        (len == [3, 0, 0, 0] && digits.iter().all(u8::is_ascii_digit)).then(|| u64_at(b, i + 7)).flatten()
    })
}

/// An occurrence: one use of a component in a component.
struct OccRecord {
    parent: Option<String>,
    own: String,
    comp: String,
    number: Option<u64>,
}

fn occurrence_records(b: &[u8]) -> Vec<OccRecord> {
    let mut out = Vec::new();
    for at in class_records(b, CLASS_OCCURRENCE) {
        // Ids marked `$`, then the component's id (unmarked): `parent$ own$ design$ comp` for an
        // occurrence in a component, `own$ design$ comp` for one in the root.
        let ids = uuid_strings(b, at + 40, at + 700, 5);
        let Some(k) = ids.iter().position(|s| !s.3) else { continue };
        let (parent, own) = match k {
            3 => (ids.first().map(|s| s.2.clone()), ids.get(1).map(|s| s.2.clone())),
            2 => (None, ids.first().map(|s| s.2.clone())),
            _ => continue,
        };
        let (Some(own), Some(comp)) = (own, ids.get(k).map(|s| s.2.clone())) else { continue };
        out.push(OccRecord { parent, own, comp, number: record_number(b, at) });
    }
    out
}

/// Whether the stream has occurrence records this reader places bodies by (class tags in the
/// BulkStream). Streams without them carry their classes in the MetaStream only; see
/// [`crate::fusion_tree::occurrence_placements`].
pub(crate) fn has_occurrence_records(b: &[u8]) -> bool {
    !occurrence_records(b).is_empty()
}

/// A component's record: its id and name.
struct CompRecord {
    at: usize,
    id: String,
    name: Option<String>,
}

fn component_records(b: &[u8]) -> Vec<CompRecord> {
    let runs_in = |lo: usize, hi: usize| utf16_runs(b.get(lo..hi.min(b.len())).unwrap_or_default());
    let mut out = Vec::new();
    for at in class_records(b, CLASS_COMPONENT) {
        let Some(id) = uuid_strings(b, at + 40, at + 200, 1).into_iter().next().map(|s| s.2) else { continue };
        // The name: the first text after the ids that is not itself an id.
        let name = runs_in(at + 40, at + 500)
            .into_iter()
            .map(|(_, s)| s)
            .find(|s| !(s.len() >= 36 && s.get(8..9) == Some("-") && s.get(13..14) == Some("-")));
        out.push(CompRecord { at, id, name });
    }
    out
}

/// Rigid matrices found in `b`, in stream order, keyed by what names their owner.
struct Transforms {
    /// Owner record number (just before the matrix) → matrices.
    by_number: std::collections::HashMap<u64, Vec<Placement>>,
    /// Owner UUID (the string before the matrix) → lists of matrices, one list per record.
    by_id: std::collections::HashMap<String, Vec<Vec<Placement>>>,
    /// The same shape as `by_id`: the placement stored after each matrix, see
    /// [`CLASS_TRANSFORM_ALSO`].
    nested: std::collections::HashMap<String, Vec<Vec<Option<Placement>>>>,
    /// Component placements per reference key, see [`component_parts`].
    parts: std::collections::HashMap<String, Vec<Placement>>,
    /// Keys of records that name a reference but carry no matrix (a reference left at the
    /// origin).
    /// References named without a matrix (at the origin), with where the design moved their
    /// context component when it did (a moved-board record repeating the identity).
    bare: std::collections::HashMap<String, Option<Placement>>,
}

/// The matrix a transform record carries: a few bytes after its class.
fn matrix_in(b: &[u8], at: usize) -> Option<(usize, Placement)> {
    (at + 40..=at + 46).find_map(|p| rigid_at(b, p).map(|m| (p, m)))
}

fn same_placement(a: &Placement, b: &Placement) -> bool {
    let d = |x: f64, y: f64| (x - y).abs() < 1e-9;
    a.r.iter().flatten().zip(b.r.iter().flatten()).all(|(x, y)| d(*x, *y))
        && a.t.iter().zip(&b.t).all(|(x, y)| d(*x, *y))
}

/// The placement stored after a repeat of `x` in a record of class [`CLASS_TRANSFORM_ALSO`]
/// that starts within [`REPEAT_REACH`] bytes after `after`: the matrix 8 to 24 bytes after it.
fn nested_after(b: &[u8], repeats: &[usize], after: usize, x: &Placement) -> Option<Placement> {
    let k = repeats.partition_point(|a| *a < after);
    repeats.iter().skip(k).take_while(|a| **a < after + REPEAT_REACH).take(8).find_map(|a| {
        // The repeat's matrix is not at a fixed distance from the class: take the first.
        let (mp, m) = (*a + 40..*a + 110).find_map(|p| rigid_at(b, p).map(|m| (p, m)))?;
        if !same_placement(&m, x) {
            return None;
        }
        (mp + 128 + 8..=mp + 128 + 24).find_map(|p| rigid_at(b, p))
    })
}

/// [`nested_after`] for the 2025 layout, whose records carry no class tags: a repeat of `x`
/// anywhere within [`REPEAT_REACH`] bytes after `after`, and the matrix 8 to 24 bytes after
/// that repeat (HDMI cable: both of its references, the pins' and the USB-C connector's).
fn nested_loose(b: &[u8], after: usize, x: &Placement) -> Option<Placement> {
    let end = after.saturating_add(REPEAT_REACH).min(b.len());
    (after..end)
        .filter(|p| rigid_at(b, *p).is_some_and(|m| same_placement(&m, x)))
        .take(8)
        .find_map(|p| (p + 128 + 8..=p + 128 + 24).find_map(|q| rigid_at(b, q)))
        .filter(|m| !same_placement(m, x))
}

/// The placement the design stores for a reference whose matrix `x` ends at `after`.
/// `budget`: bytes the loose scan may still look at in this stream; each scan spends what
/// it reads, so many references cannot make the walk quadratic (security review S2).
fn nested_for(b: &[u8], repeats: &[usize], after: usize, x: &Placement, budget: &mut usize) -> Option<Placement> {
    nested_after(b, repeats, after, x).or_else(|| {
        if *budget < REPEAT_REACH {
            return None;
        }
        *budget -= REPEAT_REACH;
        nested_loose(b, after, x)
    })
}

/// Bytes all loose scans of one design stream may read together: 500 scans' worth (a few
/// seconds at worst; the samples need a handful).
const LOOSE_SCAN_BUDGET: usize = 500 * REPEAT_REACH;

/// Most components of one referenced design read from the placements a design stores for them.
const MAX_PARTS: usize = 256;
/// Largest gap (bytes) between two records of a run of component placements.
const PART_GAP: usize = 700;

/// The placements a design stores for the components of a referenced design (the Pico case): a
/// run of class [`CLASS_TRANSFORM_ALSO`] records starting within [`REPEAT_REACH`] bytes after the
/// reference. A record of the run holds two matrices, the second 8 to 24 bytes after the first
/// (a joint's origin, then the component's world placement). The run repeats the last
/// component's pair many times and has empty records between: the distinct second matrices in
/// order of first appearance are the components', in the order the referenced design lists them.
fn component_parts(b: &[u8], repeats: &[usize], after: usize) -> Vec<Placement> {
    let k = repeats.partition_point(|a| *a < after);
    let mut out: Vec<Placement> = Vec::new();
    let mut prev = match repeats.get(k) {
        Some(a) if *a < after.saturating_add(REPEAT_REACH) => *a,
        _ => return out,
    };
    for a in repeats.iter().skip(k).take(MAX_INSTANCES) {
        if a.saturating_sub(prev) > PART_GAP {
            break;
        }
        prev = *a;
        let Some(p) = (*a + 40..*a + 110).find(|p| rigid_at(b, *p).is_some()) else { continue };
        let Some(m) = (p + 128 + 8..=p + 128 + 24).find_map(|q| rigid_at(b, q)) else { continue };
        if !out.iter().any(|o| same_placement(o, &m)) {
            out.push(m);
            if out.len() >= MAX_PARTS {
                break;
            }
        }
    }
    out
}

/// A component name of the 2025 layouts: the design's names carry a version, `PicoW v1`.
fn versioned_name(s: &str) -> bool {
    let Some((head, tail)) = s.rsplit_once(" v") else { return false };
    !head.is_empty() && s.len() <= 100 && !tail.is_empty() && tail.bytes().all(|c| c.is_ascii_digit())
}

/// The bodies of each component of a design that has no occurrence records (the Pico W v2
/// case, stream version 310). The components are the version-suffixed names after the design's
/// own (the first); a body blob right after a component's name is that component's, any other
/// blob belongs to the last component (its record is written after the bodies it took over).
/// Verified against one sample (three components, all within 0.001 mm).
fn component_groups(runs: &[(usize, String)], refs: &[(usize, String)]) -> Vec<(String, Vec<String>)> {
    let names: Vec<&(usize, String)> = runs.iter().filter(|(_, s)| versioned_name(s)).collect();
    let Some((_, comps)) = names.split_first() else { return Vec::new() };
    if comps.is_empty() || comps.len() > MAX_PARTS {
        return Vec::new();
    }
    let mut groups: Vec<(String, Vec<String>)> = comps.iter().map(|(_, s)| (s.clone(), Vec::new())).collect();
    let mut prev = 0usize;
    for (at, id) in refs {
        // The last component name between the previous blob and this one.
        let k = comps.partition_point(|(o, _)| o < at);
        let named = k.checked_sub(1).filter(|k| comps.get(*k).is_some_and(|(o, _)| *o >= prev));
        let g = named.unwrap_or(groups.len() - 1);
        if let Some((_, v)) = groups.get_mut(g) {
            v.push(id.clone());
        }
        prev = *at;
    }
    groups
}

/// For a reference at the origin: the first record of class [`CLASS_TRANSFORM_ALSO`] within
/// [`REPEAT_REACH`] after `after` whose first matrix is not the identity holds the moved
/// context component's placement itself (the Buck case: no repeat of the reference's matrix
/// comes first).
fn moved_after_bare(b: &[u8], repeats: &[usize], after: usize) -> Option<Placement> {
    let k = repeats.partition_point(|a| *a < after);
    repeats.iter().skip(k).take_while(|a| **a < after + REPEAT_REACH).take(8).find_map(|a| {
        let m = (*a + 40..*a + 110).find_map(|p| rigid_at(b, p))?;
        (!same_placement(&m, &Placement::IDENTITY)).then_some(m)
    })
}

fn transforms(b: &[u8]) -> Transforms {
    let mut loose_budget = LOOSE_SCAN_BUDGET;
    let repeats = class_records(b, CLASS_TRANSFORM_ALSO);
    let mut by_number: std::collections::HashMap<u64, Vec<Placement>> = std::collections::HashMap::new();
    for at in class_records(b, CLASS_TRANSFORM_OF) {
        let Some((_, m)) = matrix_in(b, at) else { continue };
        if at >= 9
            && b.get(at - 9) == Some(&1)
            && let Some(n) = u64_at(b, at - 8)
        {
            by_number.entry(n).or_default().push(m);
        }
    }
    let mut by_id: std::collections::HashMap<String, Vec<Vec<Placement>>> = std::collections::HashMap::new();
    let mut nested: std::collections::HashMap<String, Vec<Vec<Option<Placement>>>> = std::collections::HashMap::new();
    let mut bare: std::collections::HashMap<String, Option<Placement>> = std::collections::HashMap::new();
    let mut parts: std::collections::HashMap<String, Vec<Placement>> = std::collections::HashMap::new();
    for at in class_records(b, CLASS_TRANSFORM) {
        // (a) The owner's id ends shortly before the record, the matrix is in it.
        if let Some((mp, m)) = matrix_in(b, at) {
            let near = uuid_strings(b, at.saturating_sub(300), at, 16)
                .into_iter()
                .rfind(|s| at.saturating_sub(s.1) <= 60 && s.1 <= at);
            if let Some((_, _, id, _)) = near {
                nested.entry(id.clone()).or_default().push(vec![nested_for(
                    b,
                    &repeats,
                    mp + 128,
                    &m,
                    &mut loose_budget,
                )]);
                let found = component_parts(b, &repeats, mp + 128);
                if !found.is_empty() {
                    parts.entry(id.clone()).or_insert(found);
                }
                by_id.entry(id).or_default().push(vec![m]);
            }
            // (a') or the id just after the matrix (a reference used once).
            let end = mp + 128;
            if let Some((_, _, id, _)) =
                uuid_strings(b, end, end + 24 + 72, 1).into_iter().next().filter(|s| s.0 <= end + 24)
            {
                nested.entry(id.clone()).or_default().push(vec![nested_for(
                    b,
                    &repeats,
                    mp + 128,
                    &m,
                    &mut loose_budget,
                )]);
                let found = component_parts(b, &repeats, mp + 128);
                if !found.is_empty() {
                    parts.entry(id.clone()).or_insert(found);
                }
                by_id.entry(id).or_default().push(vec![m]);
            }
            continue;
        }
        // (b) The id follows the class (after a length prefix), then the matrix and, for a
        // reference used several times, the matrices of the other instances.
        let Some((_, end, mut id, _)) = uuid_strings(b, at + 44, at + 44 + 80, 1).into_iter().next() else { continue };
        let (end, urn) = past_urn(b, end);
        // The key with its urn is how the archive's design description names the reference.
        id.push_str(&urn);
        let first = (end..=end + 32).find_map(|p| rigid_at(b, p).map(|m| (p, m)));
        let Some((mut pos, m)) = first else {
            // At the origin; the board inside may still have been moved (the Buck case).
            let over =
                nested_after(b, &repeats, end, &Placement::IDENTITY).or_else(|| moved_after_bare(b, &repeats, end));
            let slot = bare.entry(id).or_insert(None);
            if slot.is_none() {
                *slot = over;
            }
            continue;
        };
        let mut over = vec![nested_for(b, &repeats, pos + 128, &m, &mut loose_budget)];
        let mut list = vec![m];
        while list.len() < MAX_INSTANCES {
            pos += 128;
            let next = (pos..=pos + INSTANCE_GAP).find_map(|p| rigid_at(b, p).map(|m| (p, m)));
            let Some((p, m)) = next else { break };
            pos = p;
            over.push(nested_for(b, &repeats, pos + 128, &m, &mut loose_budget));
            list.push(m);
        }
        nested.entry(id.clone()).or_default().push(over);
        by_id.entry(id).or_default().push(list);
    }
    Transforms { by_number, by_id, nested, parts, bare }
}

/// Longest `_urn:...` suffix of a reference key skipped (UTF-16 characters).
const MAX_URN_CHARS: usize = 512;

/// After a reference key, the end of the `_urn:<document>?version=<n>` text a design may append
/// to it (3957 Crickit HAT: the jointed 2x20 header, whose transform records name it so), and
/// that text in lowercase; `end` and nothing when there is none.
fn past_urn(b: &[u8], end: usize) -> (usize, String) {
    let starts =
        "_urn:".bytes().enumerate().all(|(k, c)| b.get(end + 2 * k) == Some(&c) && b.get(end + 2 * k + 1) == Some(&0));
    if !starts {
        return (end, String::new());
    }
    let text: String = (0..MAX_URN_CHARS)
        .map_while(|k| match (b.get(end + 2 * k), b.get(end + 2 * k + 1)) {
            (Some(c), Some(0)) if (0x21..0x7f).contains(c) => Some(char::from(c.to_ascii_lowercase())),
            _ => None,
        })
        .collect();
    (end + 2 * text.len(), text)
}

/// What a design stream says about placements.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Design {
    /// Placed bodies: one entry per body and occurrence of its component, in the design's
    /// coordinates (nesting composed). Bodies of the root component are left out.
    pub bodies: Vec<Placed>,
    /// External references with their instances in this design.
    pub xrefs: Vec<Xref>,
    /// In a design that lists instances (an electronics board), the placement of the component
    /// they are relative to, as this design has it.
    pub context: Option<Placement>,
    /// Body blob id (`BREP.<id>.smb`) to the id of the history blob (`BREP.<id>.smbh`) written
    /// just before it, see [`history_pairs`].
    pub history: std::collections::HashMap<String, String>,
    /// Body blob id (`BREP.<id>.smb`) to the colour of each of its bodies, from the
    /// `BASEFEAT_PM_BODY_REF` records of its feature, see [`body_ref_colours`].
    pub body_colours: std::collections::HashMap<String, Vec<Option<u32>>>,
    /// In a design without occurrence records: each component's name and body blob ids, in the
    /// order the stream lists the components (see [`component_groups`]).
    pub groups: Vec<(String, Vec<String>)>,
}

/// Components, occurrences and references of a design stream.
pub fn read_design(bulk: &[u8]) -> Design {
    read_design_cancellable(bulk, &AtomicBool::new(false)).unwrap_or_default()
}

/// [`read_design`] that stops (`None`) once `cancel` is set. The work is linear in the stream:
/// every lookup per body or occurrence goes through an index built once.
pub fn read_design_cancellable(bulk: &[u8], cancel: &AtomicBool) -> Option<Design> {
    let stop = || cancel.load(Ordering::Relaxed);
    let runs = utf16_runs(bulk);
    if stop() {
        return None;
    }
    let comps = component_records(bulk);
    let occs = occurrence_records(bulk);
    if stop() {
        return None;
    }
    let tf = transforms(bulk);
    if stop() {
        return None;
    }
    // One entry per occurrence id: the parent from any of its records, the transform from the
    // first record that has one.
    #[derive(Default)]
    struct Occ {
        parent: Option<String>,
        comp: String,
        local: Option<Placement>,
    }
    let mut by_own: std::collections::HashMap<&str, Occ> = std::collections::HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for o in &occs {
        let e = by_own.entry(o.own.as_str()).or_insert_with(|| {
            order.push(o.own.as_str());
            Occ { comp: o.comp.clone(), ..Occ::default() }
        });
        if e.parent.is_none() {
            e.parent.clone_from(&o.parent);
        }
        if e.local.is_none() {
            // The owner number of a transform is the record before the occurrence's.
            e.local = o
                .number
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| tf.by_number.get(&n))
                .and_then(|v| v.first())
                .copied();
        }
    }
    // Transforms keyed by the occurrence's id: the last record that has one.
    for (own, e) in &mut by_own {
        if e.local.is_none() {
            e.local = tf.by_id.get(*own).and_then(|v| v.last()).and_then(|l| l.first()).copied();
        }
    }
    // The placement of an occurrence in the design, and whether any placement in its chain was
    // read (an occurrence with none anywhere above it sits at the origin, which is also what a
    // body without occurrences gets).
    let world = |own: &str| -> (Placement, bool) {
        let mut chain = Vec::new();
        let mut cur = Some(own);
        let mut explicit = false;
        while let Some(id) = cur {
            if chain.len() >= MAX_DEPTH {
                break;
            }
            let Some(e) = by_own.get(id) else { break };
            explicit |= e.local.is_some();
            chain.push(e.local.unwrap_or(Placement::IDENTITY));
            cur = e.parent.as_deref();
        }
        // Outermost first.
        (chain.iter().rev().fold(Placement::IDENTITY, |acc, l| acc.then(l)), explicit)
    };
    let (mut blocks, context) = block_placements(bulk, &runs);
    let mut out = Design::default();
    let listed: std::collections::HashSet<String> =
        blocks.iter().filter(|(_, k)| *k == BlockKind::Listed).map(|(p, _)| p.brep.clone()).collect();
    // A matrix after `CreateComponent` places a component only where listed instances follow
    // (an electronics board: its matrix is where the board sits and the parts are relative to
    // it); elsewhere it can be any move in the design's history.
    if !blocks.iter().any(|(_, k)| *k == BlockKind::Listed) {
        blocks.clear();
    }
    out.context = context.filter(|_| !blocks.is_empty());
    // Components with an occurrence placed by a matrix of its own or of an ancestor.
    let explicit: std::collections::HashSet<&str> =
        order.iter().filter(|own| world(own).1).filter_map(|own| by_own.get(*own)).map(|e| e.comp.as_str()).collect();
    if !occs.is_empty() {
        // The component of each body: the last component record before it.
        let refs = brep_refs_in(&runs);
        // Instance numbers per component, in the order the stream lists the occurrences.
        let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut owners: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
        for own in &order {
            if let Some(e) = by_own.get(*own) {
                owners.entry(e.comp.as_str()).or_default().push(own);
            }
        }
        let ordinal: std::collections::HashMap<&str, usize> = order
            .iter()
            .filter_map(|own| {
                let comp = by_own.get(*own)?.comp.as_str();
                let n = seen.entry(comp).or_insert(0);
                *n += 1;
                Some((*own, *n))
            })
            .collect();
        for (n, (at, id)) in refs.iter().enumerate() {
            if out.bodies.len() >= MAX_OCCURRENCES {
                break;
            }
            if n % 256 == 0 && stop() {
                return None;
            }
            let k = comps.partition_point(|c| c.at < *at);
            let Some(comp) = k.checked_sub(1).and_then(|k| comps.get(k)) else { continue };
            // Listed instances are relative to a component the occurrence records do not name.
            if listed.contains(id) || !explicit.contains(comp.id.as_str()) {
                continue;
            }
            for own in owners.get(comp.id.as_str()).into_iter().flatten() {
                if out.bodies.len() >= MAX_OCCURRENCES {
                    break;
                }
                let label = comp.name.as_deref().unwrap_or("Component");
                out.bodies.push(Placed {
                    brep: id.clone(),
                    occurrence: format!("{label}:{}", ordinal.get(*own).copied().unwrap_or(1)),
                    placement: world(own).0,
                });
            }
        }
    }
    // The block rules: listed instances replace what the records said about their body; the
    // others only place bodies the records left alone.
    out.bodies.retain(|q| !listed.contains(&q.brep));
    let mut have: std::collections::HashSet<String> = out.bodies.iter().map(|q| q.brep.clone()).collect();
    for (p, kind) in blocks {
        if kind == BlockKind::Listed || !have.contains(&p.brep) {
            have.insert(p.brep.clone());
            out.bodies.push(p);
        }
    }
    // References: for each key, the record with the most instances (the last of equals).
    let mut keys: Vec<&String> = tf.by_id.keys().collect();
    keys.sort();
    for key in keys {
        if by_own.contains_key(key.as_str()) {
            continue;
        }
        let best = tf.by_id.get(key).and_then(|lists| {
            let most = lists.iter().map(Vec::len).max()?;
            let k = lists.iter().rposition(|l| l.len() == most)?;
            Some((lists.get(k)?, tf.nested.get(key).and_then(|n| n.get(k))))
        });
        if let Some((list, over)) = best {
            let nested = (0..list.len()).map(|i| over.and_then(|o| o.get(i)).copied().flatten()).collect();
            let parts = tf.parts.get(key).cloned().unwrap_or_default();
            out.xrefs.push(Xref { role: key.clone(), instances: list.clone(), nested, parts });
        }
    }
    // A reference named without a matrix sits at the origin.
    let mut bare: Vec<(&String, &Option<Placement>)> =
        tf.bare.iter().filter(|(k, _)| !tf.by_id.contains_key(*k) && !by_own.contains_key(k.as_str())).collect();
    bare.sort_by(|a, b| a.0.cmp(b.0));
    for (key, over) in bare.into_iter().take(MAX_RECORDS) {
        out.xrefs.push(Xref {
            role: key.clone(),
            instances: vec![Placement::IDENTITY],
            nested: vec![*over],
            parts: Vec::new(),
        });
    }
    if occs.is_empty() {
        out.groups = component_groups(&runs, &brep_refs_in(&runs));
    }
    out.history = history_pairs(&runs);
    let by_history = body_ref_colours(bulk, &runs);
    for (body, history) in &out.history {
        if let Some(c) = by_history.get(history) {
            out.body_colours.insert(body.clone(), c.clone());
        }
    }
    Some(out)
}

/// Colours (ADR 0039 amendment "Fusion colours"). A body's faces carry a material id (a
/// `NEUTRON_Material_attrib_def` attribute, [`crate::Sat::material`]); the design stream
/// names each material id as UTF-16LE text with the id of its appearance asset a little
/// after it ([`material_assets`]); the asset's protein package holds `InstanceProperties.bin`
/// whose record for the asset has its sRGB colour as four doubles ([`instance_colours`]).
/// Most material or asset ids read from one stream or package.
const MAX_COLOUR_IDS: usize = 100_000;
/// How far (bytes) after a material id its asset id is looked for.
const ASSET_REACH: usize = 300;
/// Offset from the start of an asset's id in `InstanceProperties.bin` to its colour doubles.
const COLOUR_OFFSET: usize = 203;
/// Length of a UUID's text.
const UUID_LEN: usize = 36;

fn is_upper_uuid(b: &[u8]) -> bool {
    b.len() == UUID_LEN
        && b.iter().enumerate().all(|(i, &c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'A'..=b'F').contains(&c),
        })
}

/// The material id (lowercase UUID) to appearance asset id (uppercase UUID, without its
/// trailing `$`) pairs a design stream holds: an asset id is taken for the material id
/// before it when it follows within [`ASSET_REACH`] bytes; the first one found wins.
pub fn material_assets(bulk: &[u8]) -> std::collections::HashMap<String, String> {
    let mut mats: Vec<(usize, &str)> = Vec::new();
    let mut assets: Vec<(usize, &str)> = Vec::new();
    let runs = utf16_runs(bulk);
    for (off, run) in &runs {
        let off = *off;
        let b = run.as_bytes();
        for i in 0..b.len().saturating_sub(UUID_LEN - 1) {
            if b.get(i + 8) != Some(&b'-') {
                continue;
            }
            let Some(w) = run.get(i..i + UUID_LEN) else { continue };
            if crate::parse::is_uuid(w) {
                mats.push((off + 2 * i, w));
            } else if b.get(i + UUID_LEN) == Some(&b'$') && is_upper_uuid(w.as_bytes()) {
                assets.push((off + 2 * i, w));
            }
        }
        if mats.len() + assets.len() > MAX_COLOUR_IDS {
            break;
        }
    }
    let mut out = std::collections::HashMap::new();
    let mut from = 0usize;
    for (at, id) in mats {
        // Both lists ascend: the first asset after this material only moves forward.
        while assets.get(from).is_some_and(|(a, _)| *a <= at) {
            from += 1;
        }
        if let Some((a, asset)) = assets.get(from)
            && a - at <= UUID_LEN * 2 + ASSET_REACH
            && out.len() < MAX_COLOUR_IDS
        {
            out.entry(id.to_string()).or_insert_with(|| (*asset).to_string());
        }
    }
    out
}

/// The sRGB colour (`0xRRGGBB`) of every asset record in a protein package's
/// `InstanceProperties.bin`. A record starts with its asset id in ASCII (an uppercase UUID
/// not followed by `_` or a letter or digit: `<id>_Post2015` is another record); four
/// little-endian doubles (r, g, b in [0, 1], then alpha 1.0) lie [`COLOUR_OFFSET`] bytes
/// after it, else the first such quadruple before the next record is taken.
pub fn instance_colours(bin: &[u8]) -> std::collections::HashMap<String, u32> {
    let mut starts: Vec<usize> = Vec::new();
    for i in 0..bin.len().saturating_sub(UUID_LEN - 1) {
        if bin.get(i + 8) != Some(&b'-') {
            continue;
        }
        let Some(w) = bin.get(i..i + UUID_LEN) else { continue };
        let next = bin.get(i + UUID_LEN).copied();
        if is_upper_uuid(w) && !next.is_some_and(|c| c == b'_' || c.is_ascii_alphanumeric()) {
            starts.push(i);
            if starts.len() >= MAX_COLOUR_IDS {
                break;
            }
        }
    }
    let colour_at = |at: usize| -> Option<u32> {
        let mut c = [0f64; 4];
        for (k, v) in c.iter_mut().enumerate() {
            *v = f64_at(bin, at + 8 * k)?;
        }
        let ok = c.iter().take(3).all(|v| (0.0..=1.0).contains(v)) && c[3].to_bits() == 1.0f64.to_bits();
        // Nearest of 256 steps: `v` is in [0, 1].
        ok.then(|| c.iter().take(3).fold(0u32, |acc, v| (acc << 8) | (v * 255.0).round() as u32))
    };
    let mut out = std::collections::HashMap::new();
    for (k, &st) in starts.iter().enumerate() {
        let Some(id) = bin.get(st..st + UUID_LEN).and_then(|b| std::str::from_utf8(b).ok()) else { continue };
        if out.contains_key(id) {
            continue;
        }
        let end = starts.get(k + 1).copied().unwrap_or(bin.len());
        let found =
            colour_at(st + COLOUR_OFFSET).or_else(|| (st + UUID_LEN..end.saturating_sub(31)).find_map(colour_at));
        if let Some(c) = found {
            out.insert(id.to_string(), c);
        }
    }
    out
}

/// Body colours of imported bodies (ADR 0039 amendment "Fusion colours, third pass"). A feature
/// that holds many bodies without material attributes (an imported electronics model) lists
/// them in `BASEFEAT_PM_BODY_REF` records of the design stream, one per body after the first:
/// `14 00 00 00 BASEFEAT_PM_BODY_REF`, `12 00 00 00 PassiveRefMetaType`, `01` and the record
/// number, a class id (`D3937028-...`, 36 ASCII characters), `00 00 02 00 00 00` and the
/// body's 1-based index within the feature, and later the colour property id
/// (`702b9cd2-...`), six zero bytes and the colour as three little-endian `f32` and `1.0`.
const BODY_REF_NAME: &[u8] = b"BASEFEAT_PM_BODY_REF";
const PASSIVE_REF: &[u8] = b"\x12\0\0\0PassiveRefMetaType";
const REF_CLASS: &[u8] = b"D3937028-C20C-4E65-B010-94AD418A5C20";
const COLOUR_PROPERTY: &[u8] = b"702b9cd2-537c-429e-8cc4-22beeeb98c37";
/// How much of the stream after a record's name is searched for its fields.
const REF_WINDOW: usize = 4096;
/// How far (bytes) before a history blob's name the feature's pointers to a record stand.
const REF_REACH: usize = 4096;
/// Most body references read from a stream.
const MAX_BODY_REFS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BodyRef {
    at: usize,
    recno: Option<u32>,
    index: u32,
    colour: Option<u32>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn find_in(b: &[u8], pat: &[u8]) -> Option<usize> {
    let first = *pat.first()?;
    let mut from = 0;
    while let Some(i) = b.get(from..)?.iter().position(|&c| c == first) {
        let at = from + i;
        if b.get(at..at + pat.len()) == Some(pat) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// The record whose name starts at `at`; `end` is where the next one starts.
fn body_ref_at(b: &[u8], at: usize, end: usize) -> BodyRef {
    let win = b.get(at..end.min(at + REF_WINDOW)).unwrap_or_default();
    let name_end = BODY_REF_NAME.len();
    let recno = win
        .get(name_end..name_end + PASSIVE_REF.len() + 1 + 4)
        .filter(|w| w.starts_with(PASSIVE_REF) && w.get(PASSIVE_REF.len()) == Some(&1))
        .and_then(|w| u32_at(w, PASSIVE_REF.len() + 1));
    let index = find_in(win, REF_CLASS)
        .map(|c| c + REF_CLASS.len())
        .filter(|&c| win.get(c..c + 6) == Some(&[0, 0, 2, 0, 0, 0]))
        .and_then(|c| u32_at(win, c + 6))
        .unwrap_or(0);
    let colour = find_in(win, COLOUR_PROPERTY).and_then(|c| {
        let at = c + COLOUR_PROPERTY.len();
        if win.get(at..at + 6)? != [0u8; 6] {
            return None;
        }
        let f = |k: usize| u32_at(win, at + 6 + 4 * k).map(f32::from_bits);
        let [r, g, bl, a] = [f(0)?, f(1)?, f(2)?, f(3)?];
        let ok = |v: f32| (0.0..=1.0).contains(&v);
        if a.to_bits() != 1.0f32.to_bits() || !ok(r) || !ok(g) || !ok(bl) {
            return None;
        }
        let byte = |v: f32| (v * 255.0).round() as u32;
        Some(byte(r) << 16 | byte(g) << 8 | byte(bl))
    });
    BodyRef { at, recno, index, colour }
}

/// Runs of consecutive references with the indices 1, 2, ..., n. A feature's first record
/// (the one with a second index 1 right after it) stands alone and is not a group: its colour
/// reads black in every sample and it names no body, so groups of one are left out too.
fn ref_groups(refs: &[BodyRef]) -> Vec<&[BodyRef]> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..=refs.len() {
        let next = refs.get(i);
        if let (Some(s), Some(r)) = (start, next)
            && usize::try_from(r.index).is_ok_and(|x| x == i - s + 1)
        {
            continue;
        }
        if let Some(g) = start.take().and_then(|s| refs.get(s..i)).filter(|g| g.len() >= 2) {
            out.push(g);
        }
        if next.is_some_and(|r| r.index == 1) {
            start = Some(i);
        }
    }
    out
}

/// Colours of the bodies of each history blob (`BREP.<id>.smbh` id) whose feature lists
/// `BASEFEAT_PM_BODY_REF` records. A feature record points to each reference by record number
/// (`01` and four bytes) a little before it names its history blob; every reference of a group
/// must lead to the same blob, else the group is left out.
fn body_ref_colours(bulk: &[u8], runs: &[(usize, String)]) -> std::collections::HashMap<String, Vec<Option<u32>>> {
    let mut starts = Vec::new();
    let mut from = 0;
    while starts.len() < MAX_BODY_REFS
        && let Some(i) = find_in(bulk.get(from..).unwrap_or_default(), BODY_REF_NAME)
    {
        starts.push(from + i);
        from += i + BODY_REF_NAME.len();
    }
    let refs: Vec<BodyRef> = starts
        .iter()
        .enumerate()
        .map(|(k, &at)| body_ref_at(bulk, at, starts.get(k + 1).copied().unwrap_or(bulk.len())))
        .collect();
    let groups = ref_groups(&refs);
    if groups.is_empty() {
        return Default::default();
    }
    // Every BREP name with its offset; true for a history blob.
    let mut names: Vec<(usize, String, bool)> = Vec::new();
    for (o, s) in runs {
        for (i, _) in s.match_indices("BREP.") {
            let Some(t) = s.get(i..) else { continue };
            let at = o + 2 * i;
            if let Some(h) = history_id(t) {
                names.push((at, h, true));
            } else if let Some(b) = brep_id(t) {
                names.push((at, b, false));
            }
        }
    }
    names.sort_by_key(|n| n.0);
    // Per wanted record number, the nearest name after a pointer to it (before the records).
    let first = refs.iter().map(|r| r.at).min().unwrap_or(0);
    let mut want: std::collections::HashMap<u32, Option<(usize, usize)>> =
        groups.iter().flat_map(|g| g.iter()).filter_map(|r| r.recno).map(|n| (n, None)).collect();
    for (i, &c) in bulk.iter().enumerate().take(first) {
        if c != 1 {
            continue;
        }
        let Some(slot) = u32_at(bulk, i + 1).and_then(|n| want.get_mut(&n)) else { continue };
        let k = names.partition_point(|n| n.0 <= i);
        if let Some(n) = names.get(k).filter(|n| n.0 - i <= REF_REACH)
            && slot.is_none_or(|(d, _)| n.0 - i < d)
        {
            *slot = Some((n.0 - i, k));
        }
    }
    let mut out = std::collections::HashMap::new();
    for g in groups {
        let mut blob: Option<usize> = None;
        let linked = g.iter().all(|r| {
            let k = r.recno.and_then(|n| want.get(&n).copied().flatten()).map(|(_, k)| k);
            let same = k.is_some() && blob.is_none_or(|b| Some(b) == k);
            blob = blob.or(k);
            same
        });
        let Some((_, id, true)) = blob.filter(|_| linked).and_then(|k| names.get(k)) else { continue };
        out.entry(id.clone()).or_insert_with(|| g.iter().map(|r| r.colour).collect());
    }
    out
}

/// The id in `BREP.<id>.smbh`: the history blob of a body.
fn history_id(s: &str) -> Option<String> {
    let rest = s.strip_prefix("BREP.")?;
    let (id, tail) = rest.split_once('.')?;
    (tail.starts_with("smbh")
        && !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
    .then(|| id.to_string())
}

/// How far (bytes) before a body's name its history blob's name may stand.
const HISTORY_REACH: usize = 16 * 1024;

/// Which history blob (`BREP.<id>.smbh`) belongs to which body blob (`BREP.<id>.smb`). Fusion
/// writes a body's record as the name of its history blob and, a few KB later, the name of the
/// body itself; a body is paired with the nearest history name before it when no other body
/// name stands between, at most [`HISTORY_REACH`] bytes away. Bodies with none are left out.
fn history_pairs(runs: &[(usize, String)]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut last: Option<(usize, String)> = None;
    for (o, s) in runs {
        for (i, _) in s.match_indices("BREP.") {
            let Some(t) = s.get(i..) else { continue };
            let at = o + 2 * i;
            if let Some(h) = history_id(t) {
                last = Some((at, h));
            } else if let Some(b) = brep_id(t)
                && let Some((hat, h)) = last.take()
                && at - hat <= HISTORY_REACH
                && out.len() < MAX_RECORDS
            {
                out.entry(b).or_insert(h);
            }
        }
    }
    out
}

/// Every `BREP.<id>.smb` in the text runs with its offset.
fn brep_refs_in(runs: &[(usize, String)]) -> Vec<(usize, String)> {
    runs.iter()
        .flat_map(|(o, s)| {
            s.match_indices("BREP.").filter_map(move |(i, _)| brep_id(s.get(i..)?).map(|id| (o + 2 * i, id)))
        })
        .collect()
}

/// The placed bodies of a design stream (see [`read_design`]).
pub fn placements(bulk: &[u8]) -> Vec<Placed> {
    read_design(bulk).bodies
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn u16s(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    fn matrix(t: [f64; 3], quarter_turn: bool) -> Vec<u8> {
        // Rows (x', y', z'): a quarter turn about Z maps x to y.
        let (a, b) = if quarter_turn { ([0.0, -1.0], [1.0, 0.0]) } else { ([1.0, 0.0], [0.0, 1.0]) };
        let m = [a[0], a[1], 0.0, t[0], b[0], b[1], 0.0, t[1], 0.0, 0.0, 1.0, t[2], 0.0, 0.0, 0.0, 1.0];
        m.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn bodies_take_the_occurrence_after_them() {
        let mut b = vec![7u8; 40];
        b.extend(u16s("BREP.aa11.smb"));
        b.extend([0u8; 90]);
        b.extend(u16s("BREP.bb22.smbh"));
        b.extend(u16s("BREP.bb22.smb"));
        b.extend([1u8; 33]);
        b.extend(matrix([0.8, 2.0, 0.157], true));
        b.extend(7u32.to_le_bytes());
        b.extend(u16s("BTN:SW1"));
        b.extend([0u8; 30]);
        b.extend(u16s("CreateComponent!"));
        b.extend(u16s("BREP.cc33.smb"));
        let p = placements(&b);
        // aa11 has a body after it before the occurrence: the root's. bb22 is placed.
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].brep, "bb22");
        assert_eq!(p[0].occurrence, "BTN:SW1");
        let t = p[0].placement.t;
        assert!((t[0] - 8.0).abs() + (t[1] - 20.0).abs() + (t[2] - 1.57).abs() < 1e-12, "{t:?}");
        assert!(p[0].placement.r[0].iter().zip([0.0, -1.0, 0.0]).all(|(a, b)| (a - b).abs() < 1e-15));
    }

    fn class(id: &str) -> Vec<u8> {
        let mut v = vec![0x24, 0, 0, 0];
        v.extend_from_slice(id.as_bytes());
        v
    }

    fn uuid16(n: u8, dollar: bool) -> Vec<u8> {
        let id = format!("{n:02x}000000-0000-4000-8000-000000000000");
        u16s(&format!("{id}{}", if dollar { "$" } else { "" }))
    }

    fn id(n: u8) -> String {
        format!("{n:02x}000000-0000-4000-8000-000000000000")
    }

    /// A component record: class, id, filler, name.
    fn comp(n: u8, name: &str) -> Vec<u8> {
        let mut v = class(CLASS_COMPONENT);
        v.extend(uuid16(n, false));
        v.extend([0u8; 12]);
        v.extend(u16s(name));
        v.extend([0u8; 30]);
        v
    }

    /// An occurrence record numbered `number`: ids marked `$` then the component's.
    fn occurrence(number: u64, parent: Option<u8>, own: u8, comp: u8) -> Vec<u8> {
        let mut v = class(CLASS_OCCURRENCE);
        v.extend([0u8; 9]);
        v.extend([3, 0, 0, 0]);
        v.extend(b"416");
        v.extend(number.to_le_bytes());
        v.extend([0u8; 6]);
        if let Some(p) = parent {
            v.extend(uuid16(p, true));
        }
        v.extend(uuid16(own, true));
        v.extend(uuid16(0xee, true));
        v.extend(uuid16(comp, false));
        v.extend(uuid16(0xee, true));
        v.extend([0u8; 20]);
        v
    }

    /// A transform that names its owner by record number (cm in the matrix).
    fn transform_of(owner: u64, t: [f64; 3], quarter_turn: bool) -> Vec<u8> {
        let mut v = vec![1];
        v.extend(owner.to_le_bytes());
        v.extend(class(CLASS_TRANSFORM_OF));
        v.extend([0u8; 3]);
        v.extend(matrix(t, quarter_turn));
        v.extend([0u8; 12]);
        v
    }

    fn near(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn placements_compose_and_then_applies_the_parent_last() {
        let quarter = Placement { r: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], t: [10.0, 0.0, 0.0] };
        let inner = Placement { r: Placement::IDENTITY.r, t: [0.0, 5.0, 0.0] };
        let w = quarter.then(&inner);
        // The point (0, 5, 0) turned a quarter turn is (-5, 0, 0), then moved by 10 in x.
        assert!(near(w.t, [5.0, 0.0, 0.0]), "{:?}", w.t);
        assert!(near(Placement::IDENTITY.then(&w).t, w.t));
        assert!(near(w.then(&Placement::IDENTITY).t, w.t));
    }

    #[test]
    fn nested_occurrences_compose_and_a_component_used_twice_gives_two_bodies() {
        let mut b = vec![9u8; 16];
        // Root component, an assembly, a part inside it.
        b.extend(comp(1, "Root"));
        b.extend(comp(2, "Asm"));
        b.extend(comp(3, "Part"));
        b.extend(u16s("BREP.abcd.smb"));
        b.extend([0u8; 40]);
        // Asm:1 in the root (moved 1 cm along x, turned), Part:1 and Part:2 inside it.
        b.extend(occurrence(100, None, 0x10, 2));
        b.extend(occurrence(110, Some(0x10), 0x11, 3));
        b.extend(occurrence(120, Some(0x10), 0x12, 3));
        b.extend(transform_of(99, [1.0, 0.0, 0.0], true));
        b.extend(transform_of(109, [0.0, 0.5, 0.0], false));
        b.extend(transform_of(119, [0.0, 0.0, 0.2], false));
        let d = read_design(&b);
        assert_eq!(d.bodies.len(), 2, "{:?}", d.bodies);
        assert!(d.bodies.iter().all(|p| p.brep == "abcd"));
        let [first, second] = [&d.bodies[0], &d.bodies[1]];
        assert_eq!(first.occurrence, "Part:1");
        assert_eq!(second.occurrence, "Part:2");
        // Part:1 at (0, 5, 0) mm in Asm, Asm turned a quarter and moved to x = 10 mm.
        assert!(near(first.placement.t, [5.0, 0.0, 0.0]), "{:?}", first.placement.t);
        assert!(near(second.placement.t, [10.0, 0.0, 2.0]), "{:?}", second.placement.t);
        assert!(first.placement.r[0].iter().zip([0.0, -1.0, 0.0]).all(|(a, b)| (a - b).abs() < 1e-12));
    }

    #[test]
    fn a_body_with_no_occurrence_of_its_component_stays_in_the_root() {
        let mut b = Vec::new();
        b.extend(comp(1, "Root"));
        b.extend(u16s("BREP.aaaa.smb"));
        b.extend(comp(2, "Part"));
        b.extend(u16s("BREP.bbbb.smb"));
        b.extend(occurrence(10, None, 0x20, 2));
        b.extend(transform_of(9, [0.0, 0.0, 0.1], false));
        let d = read_design(&b);
        assert_eq!(d.bodies.len(), 1);
        assert_eq!(d.bodies[0].brep, "bbbb");
        assert!(near(d.bodies[0].placement.t, [0.0, 0.0, 1.0]));
    }

    /// A reference record: class, length prefix, the key, a few filler bytes, matrices.
    fn reference(key: u8, ms: &[[f64; 3]]) -> Vec<u8> {
        let mut v = class(CLASS_TRANSFORM);
        v.extend([0x24, 0, 0, 0]);
        v.extend(uuid16(key, false));
        v.extend([0u8, 0x15, 0, 0, 0, 0, 0]);
        for (i, t) in ms.iter().enumerate() {
            v.extend(matrix(*t, false));
            v.extend(vec![0u8; if i % 2 == 0 { 12 } else { 100 }]);
        }
        v.extend([7u8; 300]); // no more matrices
        v
    }

    #[test]
    fn a_reference_lists_its_instances_and_the_richest_record_wins() {
        let mut b = Vec::new();
        b.extend(reference(5, &[[0.0, 0.0, 2.4]]));
        b.extend(reference(5, &[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]]));
        b.extend(reference(5, &[[9.0, 0.0, 0.0]]));
        b.extend(reference(6, &[[0.0, -1.27, -0.443]]));
        let d = read_design(&b);
        assert_eq!(d.xrefs.len(), 2, "{:?}", d.xrefs);
        let five = d.xrefs.iter().find(|x| x.role == id(5)).unwrap();
        let xs: Vec<f64> = five.instances.iter().map(|p| p.t[0]).collect();
        assert_eq!(xs.len(), 3);
        assert!((xs[0] - 10.0).abs() < 1e-9 && (xs[2] - 30.0).abs() < 1e-9, "{xs:?}");
        let six = d.xrefs.iter().find(|x| x.role == id(6)).unwrap();
        assert!(near(six.instances[0].t, [0.0, -12.7, -4.43]));
    }

    #[test]
    fn listed_instances_follow_the_block_that_placed_the_parent() {
        // Block "Board": body, CreateComponent, then the board's matrix in design coordinates.
        // Block "Part": body, two listed instances (matrix, length, name), relative to the board.
        let mut b = vec![5u8; 8];
        b.extend(uuid16(1, false));
        b.extend([0u8; 4]);
        b.extend(uuid16(2, false));
        b.extend([0u8; 4]);
        b.extend(u16s("Board"));
        b.extend([0u8; 4]);
        b.extend(u16s("BREP.b0a1.smb"));
        b.extend([0u8; 30]);
        b.extend([0u8; 4]);
        b.extend(u16s("CreateComponent"));
        b.extend([0u8; 200]);
        b.extend(matrix([10.0, 20.0, 0.0], true));
        b.extend([0u8; 50]);
        b.extend(uuid16(3, false));
        b.extend([0u8; 4]);
        b.extend(uuid16(4, false));
        b.extend([0u8; 4]);
        b.extend(u16s("Part"));
        b.extend([0u8; 4]);
        b.extend(u16s("BREP.c0b2.smb"));
        b.extend([0u8; 30]);
        for (n, x) in [("Part:R1", 1.0), ("Part:R2", 2.0)] {
            b.extend(matrix([x, 0.0, 0.0], false));
            b.extend(((n.len() * 2) as u32).to_le_bytes());
            b.extend(u16s(n));
            b.extend([0u8; 40]);
        }
        b.extend(u16s("CreateComponent"));
        let d = read_design(&b);
        assert_eq!(d.bodies.len(), 3, "{:?}", d.bodies);
        assert_eq!(d.bodies[0].occurrence, "Board:1");
        assert!(near(d.bodies[0].placement.t, [100.0, 200.0, 0.0]));
        // The board is turned a quarter: a part 10 mm along its x lands 10 mm along y.
        assert_eq!(d.bodies[1].occurrence, "Part:R1");
        assert!(near(d.bodies[1].placement.t, [100.0, 210.0, 0.0]), "{:?}", d.bodies[1].placement.t);
        assert!(near(d.bodies[2].placement.t, [100.0, 220.0, 0.0]));
    }

    #[test]
    fn inverse_undoes_a_placement() {
        let p = Placement { r: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], t: [3.0, 4.0, 5.0] };
        let back = p.then(&p.inverse());
        assert!(near(back.t, [0.0; 3]));
        assert!(
            back.r.iter().flatten().zip(Placement::IDENTITY.r.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12)
        );
    }

    #[test]
    fn a_reference_moved_by_the_referencing_design_reports_where_it_put_the_inner_component() {
        let mut b = Vec::new();
        // The reference record: matrix, then the role (8 bytes later).
        b.extend(class(CLASS_TRANSFORM));
        b.extend([0u8; 2]);
        b.extend(matrix([0.0, 0.0, 1.5], false));
        b.extend([0u8; 8]);
        b.extend(uuid16(7, false));
        b.extend([5u8; 200]);
        // A repeat of it, and 14 bytes after the matrix the other placement.
        b.extend(class(CLASS_TRANSFORM_ALSO));
        b.extend([0u8; 3]);
        b.extend(matrix([0.0, 0.0, 1.5], false));
        b.extend([1u8; 14]);
        b.extend(matrix([-1.25, 0.0, 0.32], false));
        b.extend([5u8; 100]);
        let d = read_design(&b);
        let x = d.xrefs.iter().find(|x| x.role == id(7)).unwrap();
        assert!(near(x.instances[0].t, [0.0, 0.0, 15.0]));
        assert!(near(x.nested[0].unwrap().t, [-12.5, 0.0, 3.2]), "{:?}", x.nested);
    }

    #[test]
    fn a_reference_key_with_a_document_urn_keeps_it_and_takes_the_last_stored_placement() {
        // The 3957 layout: the key is followed by `_urn:...?version=2`, then the matrix. A
        // record without a matrix, the one written at insert time and the one after the joint
        // moved the occurrence: the last is where the design has it.
        let key = |b: &mut Vec<u8>| {
            b.extend(class(CLASS_TRANSFORM));
            b.extend([0u8; 4]);
            b.extend(uuid16(7, false));
            b.extend(u16s("_urn:adsk.wipprod:fs.file:vf.AbC_-d?version=2"));
            b.extend([0u8; 6]);
        };
        let mut b = Vec::new();
        key(&mut b);
        b.extend([5u8; 200]);
        key(&mut b);
        b.extend(matrix([0.0, -0.854, 0.13381], false));
        b.extend([5u8; 200]);
        key(&mut b);
        b.extend(matrix([0.835, 5.123, -0.093], true));
        b.extend([5u8; 100]);
        let d = read_design(&b);
        let role = format!("{}_urn:adsk.wipprod:fs.file:vf.abc_-d?version=2", id(7));
        let x = d.xrefs.iter().find(|x| x.role == role).unwrap();
        assert_eq!(x.instances.len(), 1);
        assert!(near(x.instances[0].t, [8.35, 51.23, -0.93]), "{:?}", x.instances);
        // Text that is not a urn is not skipped: no matrix is found and the key is bare.
        let mut c = Vec::new();
        c.extend(class(CLASS_TRANSFORM));
        c.extend([0u8; 4]);
        c.extend(uuid16(8, false));
        c.extend(u16s("_xyz:abcdefghijklmnop"));
        c.extend([0u8; 6]);
        c.extend(matrix([0.835, 5.123, -0.093], true));
        c.extend([5u8; 100]);
        let d = read_design(&c);
        assert!(near(d.xrefs.iter().find(|x| x.role == id(8)).unwrap().instances[0].t, [0.0; 3]));
    }

    #[test]
    fn a_reference_at_the_origin_with_a_moved_board_reports_the_board() {
        // The Buck case: the reference names its role without a matrix; the next moved-board
        // record holds the board's placement itself.
        let mut b = Vec::new();
        b.extend(class(CLASS_TRANSFORM));
        b.extend([0u8; 6]);
        b.extend(uuid16(9, false));
        b.extend([5u8; 120]);
        b.extend(class(CLASS_TRANSFORM_ALSO));
        b.extend([0u8; 48]);
        b.extend(matrix([-1.4605, -0.9525, 0.42], false));
        b.extend([5u8; 100]);
        let d = read_design(&b);
        let x = d.xrefs.iter().find(|x| x.role == id(9)).unwrap();
        assert!(near(x.instances[0].t, [0.0; 3]));
        assert!(near(x.nested[0].unwrap().t, [-14.605, -9.525, 4.2]), "{:?}", x.nested);
    }

    #[test]
    fn a_classless_repeat_of_a_references_matrix_gives_the_moved_placement() {
        // The 2025 layout (HDMI cable): the repeat has no class tag, the placement the root
        // stored for the design stands 14 bytes after the repeat's matrix.
        let mut b = Vec::new();
        b.extend(class(CLASS_TRANSFORM));
        b.extend([0u8; 2]);
        b.extend(matrix([0.55, 1.5464, 1.9], true));
        b.extend([0u8; 8]);
        b.extend(uuid16(7, false));
        b.extend([5u8; 300]);
        b.extend(matrix([0.55, 1.5464, 1.9], true));
        b.extend([1u8; 14]);
        b.extend(matrix([0.0, 0.77, 0.427], true));
        b.extend([5u8; 100]);
        let d = read_design(&b);
        let x = d.xrefs.iter().find(|x| x.role == id(7)).unwrap();
        assert!(near(x.instances[0].t, [5.5, 15.464, 19.0]));
        assert!(near(x.nested[0].unwrap().t, [0.0, 7.7, 4.27]), "{:?}", x.nested);
        // A repeat with no placement after it, or too far away, gives nothing.
        let mut c = b.clone();
        let cut = c.len() - 100 - 128 - 14;
        c.truncate(cut);
        let d = read_design(&c);
        assert!(d.xrefs.iter().find(|x| x.role == id(7)).unwrap().nested[0].is_none());
        let mut far = b.clone();
        for _ in 0..REPEAT_REACH {
            far.insert(128 + 40 + 2 + 8 + 72 + 20, 9);
        }
        let d = read_design(&far);
        assert!(d.xrefs.iter().find(|x| x.role == id(7)).unwrap().nested[0].is_none());
        // Hostile: one reference followed by thousands of repeats stays bounded and total.
        let mut many = Vec::new();
        many.extend(class(CLASS_TRANSFORM));
        many.extend([0u8; 2]);
        for _ in 0..4000 {
            many.extend(matrix([0.1, 0.2, 0.3], true));
            many.extend([1u8; 14]);
        }
        many.extend(uuid16(7, false));
        let _ = read_design(&many);
    }

    #[test]
    fn a_run_of_component_placements_after_a_reference_is_read_in_order_without_repeats() {
        // The Pico case: records of two matrices (a joint origin, the component's world
        // placement 14 bytes after it), the last pair repeated, empty records between.
        let pair = |j: [f64; 3], w: [f64; 3]| {
            let mut v = class(CLASS_TRANSFORM_ALSO);
            v.extend([0u8; 34]);
            v.extend(matrix(j, false));
            v.extend([1u8; 14]);
            v.extend(matrix(w, false));
            v.extend([5u8; 40]);
            v
        };
        let empty = || {
            let mut v = class(CLASS_TRANSFORM_ALSO);
            v.extend([2u8; 20]);
            v
        };
        let mut b = Vec::new();
        b.extend(class(CLASS_TRANSFORM));
        b.extend([0u8; 2]);
        b.extend(matrix([0.0, -2.5, 0.7], true));
        b.extend([0u8; 8]);
        b.extend(uuid16(7, false));
        b.extend([5u8; 200]);
        b.extend(pair([0.355, -1.51, 0.86], [0.3551, -1.5595, 0.4593]));
        b.extend(pair([0.0, -2.5, 0.7], [0.0, -2.55, 0.3]));
        for _ in 0..5 {
            b.extend(pair([1.05, 2.6, 0.86], [1.0501, 2.5505, 0.4593]));
            b.extend(empty());
        }
        let d = read_design(&b);
        let x = d.xrefs.iter().find(|x| x.role == id(7)).unwrap();
        let ts: Vec<[f64; 3]> = x.parts.iter().map(|p| p.t).collect();
        assert_eq!(ts.len(), 3, "{ts:?}");
        assert!(
            near(ts[0], [3.551, -15.595, 4.593])
                && near(ts[1], [0.0, -25.5, 3.0])
                && near(ts[2], [10.501, 25.505, 4.593])
        );
        // A run that starts too far from the reference is not its own.
        let mut far = b.clone();
        for _ in 0..REPEAT_REACH {
            far.insert(128 + 40 + 2 + 8 + 72 + 20, 9);
        }
        let d = read_design(&far);
        assert!(d.xrefs.iter().find(|x| x.role == id(7)).unwrap().parts.is_empty());
    }

    #[test]
    fn bodies_follow_the_component_names_of_a_design_without_occurrence_records() {
        let name = |s: &str| {
            let mut v = vec![7u8; 5];
            v.extend(u16s(s));
            v
        };
        let blob = |n: &str| {
            let mut v = vec![7u8; 5];
            v.extend(u16s(&format!("BREP.{n}.smb")));
            v
        };
        let mut b = Vec::new();
        for part in [
            name("Board v2"),
            name("Button v3"),
            blob("aa11"),
            name("Logo v15"),
            blob("bb22"),
            name("PART_A"),
            blob("cc33"),
            blob("dd44"),
            name("Main v1"),
        ] {
            b.extend(part);
        }
        let d = read_design(&b);
        assert_eq!(
            d.groups,
            vec![
                ("Button v3".to_string(), vec!["aa11".to_string()]),
                ("Logo v15".to_string(), vec!["bb22".to_string()]),
                ("Main v1".to_string(), vec!["cc33".to_string(), "dd44".to_string()]),
            ]
        );
        // Names without bodies, names alone and noise stay total.
        let mut many = Vec::new();
        for k in 0..2000 {
            many.extend(name(&format!("N{k} v{k}")));
        }
        // More components than a reference can place are not read.
        assert!(read_design(&many).groups.is_empty());
        assert!((1..MAX_PARTS).contains(&read_design(&many[..many.len() / 10]).groups.len()));
        assert!(read_design(&name("Only v1")).groups.is_empty());
        for n in [1usize, 77, 4001] {
            let noise: Vec<u8> = (0..n).map(|i| (i * 31 % 241) as u8).collect();
            let _ = read_design(&noise);
        }
    }

    #[test]
    fn a_mirror_or_a_scale_is_not_a_placement_and_noise_is_harmless() {
        let mut m = matrix([1.0, 2.0, 3.0], false);
        m[80..88].copy_from_slice(&(-1.0f64).to_le_bytes()); // z row flipped: a mirror
        assert!(rigid_at(&m, 0).is_none());
        let mut s = matrix([1.0, 2.0, 3.0], false);
        s[0..8].copy_from_slice(&2.0f64.to_le_bytes());
        assert!(rigid_at(&s, 0).is_none());
        for n in [0usize, 1, 7, 127, 128, 1000] {
            let noise: Vec<u8> = (0..n).map(|i| (i * 37 % 251) as u8).collect();
            let _ = placements(&noise);
            let _ = read_design(&noise);
        }
        // Records cut short, repeated and nested in themselves.
        let mut cut = Vec::new();
        for _ in 0..3 {
            cut.extend(occurrence(1, Some(1), 1, 1)); // its own parent
            cut.extend(comp(1, "A"));
            cut.extend(reference(1, &[[1.0, 2.0, 3.0]]));
            cut.extend(class(CLASS_OCCURRENCE));
            cut.extend(class(CLASS_TRANSFORM_OF));
        }
        for n in (0..cut.len()).step_by(37) {
            let _ = read_design(&cut[..n]);
        }
        let _ = read_design(&cut);
    }

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn a_material_id_maps_to_the_asset_id_after_it() {
        let mat = "617919e9-cd5c-4110-97a6-83c6bd6b90e3";
        let other = "0bf2cf32-4921-49f1-b71a-f1fd22dcd453";
        let asset = "D95E1000-A6FE-E3FA-3088-CB94EF89F5FE";
        let mut b = vec![1u8, 2, 3, 0xff];
        b.extend(utf16(mat));
        b.extend([0x10, 0x20, 0, 0x81]);
        b.extend(utf16(&format!("{asset}$")));
        b.extend(vec![0xee; 40]);
        // Too far from its asset: no pair.
        b.extend(utf16(other));
        b.extend(vec![0x81; 400]);
        b.extend(utf16(&format!("{asset}$")));
        let m = material_assets(&b);
        assert_eq!(m.get(mat).map(String::as_str), Some(asset));
        assert!(!m.contains_key(other), "{m:?}");
        for n in [0usize, 1, 5, 77, 400] {
            let noise: Vec<u8> = (0..n).map(|i| (i * 37 % 251) as u8).collect();
            let _ = material_assets(&noise);
            let _ = instance_colours(&noise);
        }
        for n in (0..b.len()).step_by(13) {
            let _ = material_assets(&b[..n]);
        }
    }

    fn record(id: &str, at: usize, c: [f64; 4]) -> Vec<u8> {
        let mut r = id.as_bytes().to_vec();
        r.resize(at, 0xff);
        for v in c {
            r.extend(v.to_le_bytes());
        }
        r.extend([0x7f; 9]);
        r
    }

    #[test]
    fn instance_properties_give_each_assets_srgb_colour() {
        let a = "D95E1000-A6FE-E3FA-3088-CB94EF89F5FE";
        let b = "11111111-A6FE-E3FA-3088-CB94EF89F5FE";
        let c = "22222222-A6FE-E3FA-3088-CB94EF89F5FE";
        let mut bin = vec![9u8; 5];
        // `<id>_Post2015` is another record, not a start.
        bin.extend(record(&format!("{a}_Post2015"), 203, [0.0, 0.0, 0.0, 1.0]));
        bin.extend(record(a, 203, [212.0 / 255.0, 171.0 / 255.0, 33.0 / 255.0, 1.0]));
        // Not at the usual offset: found by scanning on within the record.
        bin.extend(record(b, 211, [1.0, 1.0, 1.0, 1.0]));
        // Alpha not 1: no colour.
        bin.extend(record(c, 203, [0.5, 0.5, 0.5, 0.5]));
        let m = instance_colours(&bin);
        assert_eq!(m.get(a), Some(&0x00d4_ab21), "{m:?}");
        assert_eq!(m.get(b), Some(&0x00ff_ffff));
        assert!(!m.contains_key(c));
        for n in (0..bin.len()).step_by(11) {
            let _ = instance_colours(&bin[..n]);
        }
    }

    fn id32(n: u32) -> String {
        format!("{n:08x}-0000-4000-8000-000000000000")
    }

    /// `units` components, each with one body, one occurrence in the root and its transform,
    /// padded to `pad` bytes of noise between units.
    fn big_stream(units: u32, pad: usize) -> Vec<u8> {
        let mut b = vec![9u8; 16];
        for n in 0..units {
            let mut c = class(CLASS_COMPONENT);
            c.extend(u16s(&id32(n * 4)));
            c.extend([0u8; 12]);
            c.extend(u16s("Part"));
            c.extend([0u8; 30]);
            b.extend(c);
            b.extend(u16s(&format!("BREP.{n:08x}.smb")));
            b.extend([0u8; 40]);
            let mut o = class(CLASS_OCCURRENCE);
            o.extend([0u8; 9]);
            o.extend([3, 0, 0, 0]);
            o.extend(b"416");
            o.extend((u64::from(n) * 10 + 10).to_le_bytes());
            o.extend([0u8; 6]);
            o.extend(u16s(&format!("{}$", id32(n * 4 + 1))));
            o.extend(u16s(&format!("{}$", id32(n * 4 + 2))));
            o.extend(u16s(&id32(n * 4)));
            o.extend([0u8; 20]);
            b.extend(o);
            b.extend(transform_of(u64::from(n) * 10 + 9, [0.0, 0.0, 0.1], false));
            b.extend(vec![0xA5u8; pad]);
        }
        b
    }

    #[test]
    fn read_design_is_linear_in_the_stream_size() {
        let (small, large) = (big_stream(2500, 0), big_stream(10_000, 0));
        let t = std::time::Instant::now();
        let a = read_design(&small);
        let t_small = t.elapsed();
        let t = std::time::Instant::now();
        let d = read_design(&large);
        let t_large = t.elapsed();
        assert_eq!(a.bodies.len(), 2500);
        assert_eq!(d.bodies.len(), 10_000);
        // Four times the stream: about four times the time (a quadratic read gives sixteen).
        assert!(t_large < t_small * 12 + std::time::Duration::from_millis(50), "{t_small:?} -> {t_large:?}");
        assert!(t_large < std::time::Duration::from_secs(5), "{t_large:?}");
    }

    #[test]
    fn a_large_padded_stream_reads_in_bounded_time() {
        let big = big_stream(10_000, 700);
        assert!(big.len() > 8 << 20, "{}", big.len());
        let t = std::time::Instant::now();
        let d = read_design(&big);
        assert_eq!(d.bodies.len(), 10_000);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn many_bodies_of_one_component_and_one_occurrence_list_stay_capped() {
        let mut b = comp(1, "Part");
        for n in 0..30_000u32 {
            b.extend(u16s(&format!("BREP.{n:08x}.smb")));
        }
        for n in 0..300u64 {
            b.extend(occurrence(100 + n * 10, None, 0x40, 1));
            b.extend(transform_of(99 + n * 10, [0.0, 0.0, 0.1], false));
        }
        let t = std::time::Instant::now();
        let d = read_design(&b);
        assert!(d.bodies.len() <= MAX_OCCURRENCES);
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn a_set_cancel_flag_stops_the_read() {
        let b = big_stream(100, 0);
        assert!(read_design_cancellable(&b, &AtomicBool::new(true)).is_none());
        assert!(read_design_cancellable(&b, &AtomicBool::new(false)).is_some());
    }
    #[test]
    fn a_body_is_paired_with_the_history_blob_written_just_before_it() {
        let mut b = vec![0u8; 8];
        b.extend(u16s("BREP.aaaa0001.smbh"));
        b.extend(vec![0x81; 1800]);
        b.extend(u16s("BREP.bbbb0001.smb"));
        // A body with no history before it, and one whose history is too far away.
        b.extend(vec![0x81; 100]);
        b.extend(u16s("BREP.bbbb0002.smb"));
        b.extend([0x81; 20]);
        b.extend(u16s("BREP.aaaa0003.smbh"));
        b.extend(vec![0x81; HISTORY_REACH + 10]);
        b.extend(u16s("BREP.bbbb0003.smb"));
        // Two bodies after one history name: only the first is its.
        b.extend(u16s("BREP.aaaa0004.smbh"));
        b.extend([0x81; 20]);
        b.extend(u16s("BREP.bbbb0004.smb"));
        b.extend([0x81; 20]);
        b.extend(u16s("BREP.bbbb0005.smb"));
        let d = read_design(&b);
        let h = &d.history;
        assert_eq!(h.get("bbbb0001").map(String::as_str), Some("aaaa0001"), "{h:?}");
        assert_eq!(h.get("bbbb0004").map(String::as_str), Some("aaaa0004"));
        assert!(!h.contains_key("bbbb0002") && !h.contains_key("bbbb0003") && !h.contains_key("bbbb0005"), "{h:?}");
        // Noise and cut streams are harmless.
        for n in [0usize, 1, 5, 77, 400] {
            let noise: Vec<u8> = (0..n).map(|i| (i * 37 % 251) as u8).collect();
            assert!(read_design(&noise).history.is_empty());
        }
        for n in (0..b.len()).step_by(97) {
            let _ = read_design(&b[..n]);
        }
    }

    /// One synthetic `BASEFEAT_PM_BODY_REF` record: name, type, record number, class id, index
    /// and (when given) the colour property with its colour.
    fn body_ref(recno: u32, index: u32, rgb: Option<[f32; 3]>) -> Vec<u8> {
        let mut r = b"BASEFEAT_PM_BODY_REF".to_vec();
        r.extend(b"\x12\0\0\0PassiveRefMetaType\x01");
        r.extend(recno.to_le_bytes());
        r.extend([0; 7]);
        r.extend(REF_CLASS);
        r.extend([0, 0, 2, 0, 0, 0]);
        r.extend(index.to_le_bytes());
        r.extend([7; 30]);
        if let Some([a, b, c]) = rgb {
            r.extend(COLOUR_PROPERTY);
            r.extend([0; 6]);
            for v in [a, b, c, 1.0] {
                r.extend(v.to_le_bytes());
            }
        }
        r.extend([9; 20]);
        r
    }

    /// A feature record that points to the given records and names a history blob and its body.
    fn feature(recnos: &[u32], history: &str, body: &str) -> Vec<u8> {
        let mut b = vec![0x55u8; 6];
        for n in recnos {
            b.push(1);
            b.extend(n.to_le_bytes());
            b.extend([0; 3]);
        }
        b.extend(u16s(&format!("BREP.{history}.smbh")));
        b.extend([0x81; 40]);
        b.extend(u16s(&format!("BREP.{body}.smb")));
        b
    }

    #[test]
    fn body_references_colour_the_bodies_of_the_blob_their_feature_names() {
        let (red, green) = ([1.0, 0.0, 0.0], [0.0, 0.5, 0.0]);
        // A feature with a record of its own (index 1 twice: not a group), then one with a
        // group of three and one whose references point to a blob the next feature names.
        let mut b = feature(&[100], "aaaa0001", "bbbb0001");
        b.extend(feature(&[200, 201, 202], "aaaa0002", "bbbb0002"));
        b.extend(feature(&[300, 301], "aaaa0003", "bbbb0003"));
        b.extend([0x42; 50]);
        for r in [
            body_ref(100, 1, Some([0.0; 3])),
            body_ref(200, 1, Some(red)),
            body_ref(201, 2, Some(green)),
            body_ref(202, 3, None),
            body_ref(300, 1, Some(red)),
            body_ref(301, 2, Some(red)),
        ] {
            b.extend(r);
        }
        let d = read_design(&b);
        assert_eq!(
            d.body_colours.get("bbbb0002"),
            Some(&vec![Some(0x00ff_0000), Some(0x0000_8000), None]),
            "{:?}",
            d.body_colours
        );
        assert_eq!(d.body_colours.get("bbbb0003"), Some(&vec![Some(0x00ff_0000); 2]));
        assert!(!d.body_colours.contains_key("bbbb0001"), "a single record is not a group");
    }

    #[test]
    fn a_group_must_agree_on_its_blob_and_run_from_index_one() {
        let red = Some([1.0, 0.0, 0.0]);
        // The second reference is pointed to by a feature that names another blob.
        let mut b = feature(&[10], "aaaa0001", "bbbb0001");
        b.extend(feature(&[11], "aaaa0002", "bbbb0002"));
        for r in [body_ref(10, 1, red), body_ref(11, 2, red)] {
            b.extend(r);
        }
        assert!(read_design(&b).body_colours.is_empty());
        // A gap in the indices ends a group; one that does not start at 1 is not one.
        let refs: Vec<BodyRef> =
            [1, 2, 4, 5, 1, 2, 3, 7].iter().map(|&index| BodyRef { at: 0, recno: None, index, colour: None }).collect();
        let sizes: Vec<usize> = ref_groups(&refs).iter().map(|g| g.len()).collect();
        assert_eq!(sizes, vec![2, 3]);
        // A pointer too far before the blob's name does not link.
        let mut far = vec![1u8];
        far.extend(10u32.to_le_bytes());
        far.extend(vec![0x81; REF_REACH + 10]);
        far.extend(u16s("BREP.aaaa0001.smbh"));
        far.extend(u16s("BREP.bbbb0001.smb"));
        for r in [body_ref(10, 1, red), body_ref(11, 2, red)] {
            far.extend(r);
        }
        assert!(read_design(&far).body_colours.is_empty());
    }

    #[test]
    fn a_colour_needs_its_property_zero_padding_and_unit_alpha_in_range() {
        let at = |mut r: Vec<u8>, f: &dyn Fn(&mut Vec<u8>)| {
            f(&mut r);
            body_ref_at(&r, 0, r.len()).colour
        };
        let ok = body_ref(1, 1, Some([0.5, 0.25, 1.0]));
        assert_eq!(body_ref_at(&ok, 0, ok.len()).colour, Some(0x0080_40ff));
        assert_eq!(body_ref_at(&ok, 0, ok.len()).index, 1);
        assert_eq!(body_ref_at(&ok, 0, ok.len()).recno, Some(1));
        let p = find_in(&ok, COLOUR_PROPERTY).unwrap() + COLOUR_PROPERTY.len();
        assert_eq!(at(ok.clone(), &|r| r[p] = 1), None, "padding");
        assert_eq!(
            at(ok.clone(), &|r| r[p + 6 + 12..p + 6 + 16].copy_from_slice(&0.5f32.to_le_bytes())),
            None,
            "alpha"
        );
        assert_eq!(at(ok.clone(), &|r| r[p + 6..p + 10].copy_from_slice(&2.0f32.to_le_bytes())), None, "range");
        assert_eq!(at(ok.clone(), &|r| r[p + 6..p + 10].copy_from_slice(&f32::NAN.to_le_bytes())), None, "nan");
        assert_eq!(at(ok, &|r| r.truncate(p + 8)), None, "cut");
    }

    #[test]
    fn body_reference_noise_and_cut_streams_are_harmless() {
        let mut b = feature(&[200, 201], "aaaa0002", "bbbb0002");
        for r in [body_ref(200, 1, Some([1.0, 0.0, 0.0])), body_ref(201, 2, Some([0.0, 1.0, 0.0]))] {
            b.extend(r);
        }
        for n in 0..b.len() {
            let _ = read_design(&b[..n]);
        }
        let mut x = 12345u32;
        for _ in 0..50 {
            let noise: Vec<u8> = (0..400)
                .map(|_| {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (x >> 24) as u8
                })
                .collect();
            assert!(read_design(&noise).body_colours.is_empty());
        }
        // Many references with no feature at all: bounded and empty.
        let many: Vec<u8> = (0..2000u32).flat_map(|i| body_ref(i, i % 7 + 1, None)).collect();
        assert!(read_design(&many).body_colours.is_empty());
    }
}
