//! A Fusion 360 design's feature timeline as a Gusset feature tree (ADR 0039 amendment of
//! 2026-11-09). Ported from the clean-room Python prototype (`scripts/re/fusion/`), which was
//! derived from sample files only; the byte layouts are in `docs/formats/fusion-design-stream.md`.
//!
//! [`feature_tree`] reads the design stream (class table, object records, sketches, profiles,
//! extrudes, blends, holes, patterns, mirrors) and the stored ACIS bodies (to name edges and to
//! measure through-all extents) and returns the `gusset-feature-tree` JSON, or an `Err` naming
//! the first feature it cannot say. Whether the tree is right is decided by the caller, which
//! rebuilds it and compares with the stored bodies.
//!
//! Everything is bounded: caps on records, sizes and depths, no panics on damaged input.

// The reading compares decoded doubles for exact zero or equality (stored flags, repeated
// values), as the prototype does.
#![allow(clippy::float_cmp)]

mod acis;
mod bend;
mod bodyops;
mod combine;
mod coverage;
mod feature;
mod geom;
mod insert;
mod joint;
mod json;
mod loft;
mod mirrorcomp;
mod naming;
mod occpattern;
mod patdir;
mod pcb;
mod sketch;
mod stream;
mod surface;
mod text;
mod thin;
mod thread;

use feature::feature_name;
use sketch::{RegionCache, Sketch};
use std::collections::HashMap;
use std::rc::Rc;
use stream::{Design, ref_ids, ref_lists, wstrings_all};

pub use coverage::{Coverage, Guess, ParamHit, RecordCover, Span, SpanKind, coverage};

const EXTRUDE: &str = "DD405BC2";
const SWEEP: &str = "FCBB1707";
const PARAM_HOLDER: &str = "D91D429C";
const PARAM: &str = "7A6A3D31";

/// Most bytes of design stream (meta plus bulk) one tree is read from.
const MAX_STREAM_BYTES: usize = 512 << 20;

/// State of one design's reading: the stream, the stored ACIS blobs and everything computed once.
struct Ctx<'a> {
    d: &'a Design<'a>,
    /// The `.smb` / `.smbh` blobs of the design (name, bytes).
    acis: &'a [(&'a str, &'a [u8])],
    opmap: HashMap<u64, u64>,
    sketches: HashMap<u64, Rc<Sketch>>,
    rc: RegionCache,
    exts: HashMap<u64, Option<Rc<naming::ExtGeom>>>,
    gmaps: HashMap<u64, Option<Rc<naming::GenMap>>>,
    /// Whether an extrude's generator numbering agrees with its stored faces, by (extrude, op).
    gconfirmed: HashMap<(u64, i32), bool>,
    tagged: Option<Vec<acis::TaggedFace>>,
    edge_sets: Option<Vec<Vec<acis::Edge>>>,
    named_edges: Option<Vec<acis::NamedEdge>>,
    named_planes: Option<Vec<acis::NamedPlane>>,
    named_faces: Option<Vec<acis::NamedFace>>,
    /// The operation ids of the blend whose edges are being read: a stored body that holds a
    /// face of one is a state after the blend, whose edges are not the ones it takes.
    blend_ops: Vec<i32>,
    /// The end caps of each thin extrude read so far (section 33).
    thin_caps: HashMap<u64, thin::Caps>,
    /// The faces offsets of faces read so far moved, by name, and how far (cm, out of the part).
    moved: Vec<(String, Vec<i32>, f64)>,
}

impl<'a> Ctx<'a> {
    fn new(d: &'a Design<'a>, acis: &'a [(&'a str, &'a [u8])]) -> Ctx<'a> {
        Ctx {
            d,
            acis,
            opmap: naming::op_map(d),
            sketches: HashMap::new(),
            rc: RegionCache::default(),
            exts: HashMap::new(),
            gmaps: HashMap::new(),
            gconfirmed: HashMap::new(),
            tagged: None,
            edge_sets: None,
            named_edges: None,
            named_planes: None,
            named_faces: None,
            blend_ops: Vec::new(),
            thin_caps: HashMap::new(),
            moved: Vec::new(),
        }
    }
}

/// One design stream of a Fusion file: `<dir>/MetaStream.dat` and `<dir>/BulkStream.dat`.
fn pick_stream<'a>(files: &'a [(String, Vec<u8>)]) -> Result<(&'a [u8], &'a [u8]), String> {
    let mut found: Vec<(&'a [u8], &'a [u8])> = Vec::new();
    for (n, bulk) in files {
        let Some(dir) = n.strip_suffix("BulkStream.dat") else { continue };
        if !dir.contains("[Active]") || !(dir.contains("Design1/") || dir.contains("FusionDesignSegmentType1/")) {
            continue;
        }
        let meta_name = format!("{dir}MetaStream.dat");
        let Some((_, meta)) = files.iter().find(|(m, _)| *m == meta_name) else { continue };
        if !found.iter().any(|(_, b)| *b == bulk.as_slice()) {
            found.push((meta.as_slice(), bulk.as_slice()));
        }
    }
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err("the file has no design stream".into()),
        _ => Err("the file has several design streams".into()),
    }
}

/// Gives the entries of the design an insert names, from the keys the insert carries.
pub type InsertResolver<'r> = dyn FnMut(&[String]) -> Option<Vec<(String, Vec<u8>)>> + 'r;

/// The feature tree of the design in `files` (entry names and contents of a Fusion `.f3d`:
/// the design stream pair and the `Breps.BlobParts` blobs), as `gusset-feature-tree` JSON, with
/// each component's features where the file's occurrences put it. A Component Insert is an
/// error here; see [`feature_tree_with_inserts`].
///
/// # Errors
/// A reason naming the first feature or structure that cannot be read.
pub fn feature_tree(files: &[(String, Vec<u8>)], file_name: &str) -> Result<serde_json::Value, String> {
    feature_tree_with_inserts(files, file_name, &mut |_| None)
}

/// [`feature_tree`] for a design that inserts other designs (Component Insert, an archive's
/// referenced designs): `resolve` gets the keys that name an inserted design (lowercase, the
/// UUID texts of the occurrence) and returns the entries of that design, or `None`. The inserted
/// design's own tree stands in the insert's place, moved by the insert's placement, its names
/// made unique with the insert's name; a design without a readable timeline is inserted as the
/// bodies its `.smb` blobs store. Anything the resolver cannot give stays an error.
///
/// # Errors
/// As [`feature_tree`].
pub fn feature_tree_with_inserts(
    files: &[(String, Vec<u8>)],
    file_name: &str,
    resolve: &mut InsertResolver,
) -> Result<serde_json::Value, String> {
    let mut tree = built(files, file_name, resolve, 0)?;
    json::finish(&mut tree);
    Ok(tree)
}

/// The checked tree of one design with its inserts merged and its components placed, before the
/// stored bodies are numbered.
fn built(
    files: &[(String, Vec<u8>)],
    file_name: &str,
    resolve: &mut InsertResolver,
    depth: usize,
) -> Result<serde_json::Value, String> {
    let mut tree = feature_tree_unchecked(files, file_name)?;
    let (meta, bulk) = pick_stream(files)?;
    // Joints make no geometry; they move occurrences, and the stream stores where to.
    let joints = joint_placements(meta, bulk);
    joint::place_inserts(&mut tree, &joints);
    insert::splice(&mut tree, resolve, depth)?;
    json::check(&tree)?;
    // The features are in each component's own coordinates; the file's bodies stand where the
    // occurrences of the components put them.
    let mut placed = crate::fusion::read_design(bulk).bodies;
    place_by_joints(&mut placed, &occurrence_placements(meta, bulk));
    add_pattern_copies(&mut placed, &pattern_copies(meta, bulk));
    place_by_joints(&mut placed, &joints);
    let then = Design::new(meta, bulk).map(|d| joint::positions(&d)).unwrap_or_default();
    json::place(&mut tree, &placed, &then)?;
    Ok(tree)
}

/// [`feature_tree`] with every component's features in the component's own coordinates, as the
/// stream stores them (and as the prototype writes them); the features carry `_component`.
///
/// # Errors
/// As [`feature_tree`].
pub fn feature_tree_local(files: &[(String, Vec<u8>)], file_name: &str) -> Result<serde_json::Value, String> {
    let tree = feature_tree_unchecked(files, file_name)?;
    json::check(&tree)?;
    Ok(tree)
}

/// [`feature_tree_local`] without the check that every feature was read in full: the features
/// the reader could not read stay in the tree with a reason (for diagnosing a design).
///
/// # Errors
/// As [`feature_tree`], except for the unread features.
#[doc(hidden)]
pub fn feature_tree_unchecked(files: &[(String, Vec<u8>)], file_name: &str) -> Result<serde_json::Value, String> {
    let (meta, bulk) = pick_stream(files)?;
    if meta.len().saturating_add(bulk.len()) > MAX_STREAM_BYTES {
        return Err("the design stream is too large".into());
    }
    let d = Design::new(meta, bulk).ok_or("the design stream cannot be framed")?;
    let acis: Vec<(&str, &[u8])> = files
        .iter()
        .filter(|(n, _)| n.ends_with(".smb") || n.ends_with(".smbh"))
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let mut cx = Ctx::new(&d, &acis);
    let mut tree = json::design_json(&mut cx);
    if let Some(src) = tree.get_mut("source").and_then(serde_json::Value::as_object_mut) {
        src.insert("file".into(), file_name.into());
        src.insert("design".into(), serde_json::Value::Null);
    }
    Ok(tree)
}

/// Body, feature and component relations of one design stream.
struct Graph {
    creator: HashMap<u64, u64>,
    comp_of_tl: HashMap<u64, u64>,
    tl_of_3a6d: HashMap<u64, u64>,
}

impl Graph {
    fn new(d: &Design) -> Graph {
        let mut g = Graph { creator: HashMap::new(), comp_of_tl: HashMap::new(), tl_of_3a6d: HashMap::new() };
        for &i in &d.ids {
            if d.is(i, "D26351F0") {
                for q in ref_ids(d, d.rec(i)) {
                    // `40DCC3F4` (a blob-name holder) leads one link of the sample set (NeoKey
                    // Trinkey's 0805-NO Base Feature) ahead of its creator.
                    if d.is(q, "D3937028") || d.is(q, "C3423E04") || d.is(q, "2C54F6D6") || d.is(q, "40DCC3F4") {
                        continue;
                    }
                    g.creator.entry(i).or_insert(q);
                }
            } else if d.is(i, "E03784ED") {
                // A component's own timeline is the first it names: the root also names the
                // timeline of the component being edited (6501, 6522), whose features are not its.
                if let Some(q) = ref_ids(d, d.rec(i)).into_iter().find(|q| d.is(*q, "C3423E04")) {
                    g.comp_of_tl.entry(q).or_insert(i);
                }
            } else if d.is(i, "C3423E04") {
                for q in ref_ids(d, d.rec(i)) {
                    if d.is(q, "3A6D1E62") {
                        g.tl_of_3a6d.entry(q).or_insert(i);
                    }
                }
            }
        }
        g
    }

    /// The component record a feature belongs to.
    fn component(&self, d: &Design, fid: u64) -> Option<u64> {
        for q in ref_ids(d, d.rec(fid)).into_iter().rev() {
            if d.is(q, "3A6D1E62")
                && let Some(tl) = self.tl_of_3a6d.get(&q)
            {
                return self.comp_of_tl.get(tl).copied();
            }
        }
        None
    }

    /// `D26351F0` body links a pattern copies: the ones its body selections (`9716F783`, reached
    /// through the pattern's `EA0E146E` / `2CA5A1CD` / `8DB90DAE` children) name. The links
    /// under the direction edge (`F2A7590D`) and under the pattern itself are not inputs.
    fn selected_bodies(&self, d: &Design, fid: u64) -> Vec<u64> {
        let mut seen = std::collections::HashSet::from([fid]);
        let mut front = vec![fid];
        let mut out: Vec<u64> = Vec::new();
        for _ in 0..4 {
            let mut next = Vec::new();
            for x in front {
                for q in ref_ids(d, d.rec(x)) {
                    if !seen.insert(q) {
                        continue;
                    }
                    if d.is(q, "9716F783") {
                        for l in ref_ids(d, d.rec(q)) {
                            if d.is(l, "D26351F0") && !out.contains(&l) {
                                out.push(l);
                            }
                        }
                    } else if ["EA0E146E", "2CA5A1CD", "8DB90DAE"].iter().any(|c| d.is(q, c)) {
                        next.push(q);
                    }
                }
            }
            front = next;
        }
        out
    }

    /// `D26351F0` body links reachable from a feature through its own child records.
    fn body_links(&self, d: &Design, fid: u64, depth: usize) -> Vec<u64> {
        let mut seen = std::collections::HashSet::from([fid]);
        let mut front = vec![fid];
        let mut out: Vec<u64> = Vec::new();
        for _ in 0..depth {
            let mut next = Vec::new();
            for x in front {
                for q in ref_ids(d, d.rec(x)) {
                    if !seen.insert(q) {
                        continue;
                    }
                    if d.is(q, "D26351F0") {
                        if !out.contains(&q) {
                            out.push(q);
                        }
                    } else if [
                        "9716F783", "2CA5A1CD", "5A1BF548", "5662F619", "0897AF07", "F2A7590D", "8DB90DAE", "EA0E146E",
                    ]
                    .iter()
                    .any(|c| d.is(q, c))
                    {
                        next.push(q);
                    }
                }
            }
            front = next;
        }
        out
    }

    /// The features a pattern or mirror copies: `6CCF41D5` lists under its `5A1BF548` input records.
    fn inputs(&self, d: &Design, fid: u64) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        for q in ref_ids(d, d.rec(fid)) {
            if !d.is(q, "5A1BF548") {
                continue;
            }
            for q2 in ref_ids(d, d.rec(q)) {
                if !d.is(q2, "6CCF41D5") {
                    continue;
                }
                for q3 in ref_ids(d, d.rec(q2)) {
                    if q3 != q
                        && !d.is(q3, "5A1BF548")
                        && !d.is(q3, sketch::SKETCH_FEAT)
                        && feature_name(d, q3).0.is_some()
                        && !out.contains(&q3)
                    {
                        out.push(q3);
                    }
                }
            }
        }
        out
    }
}

/// Classes whose records carry a feature name in the samples' timelines.
const FEATURE_PREFIXES: [&str; 36] = [
    "78B9DEFC",
    "DD405BC2",
    "8DA771B7",
    "A07D5F17",
    "F757D611",
    "F4163A07",
    "D1728651",
    "AA8B3D40",
    "D087EFE5",
    "428B5C6E",
    "D869265F",
    "AE12AD3F",
    "64E40C8E",
    "E8063467",
    "E8816367",
    "D7015B81",
    "8DFDD543",
    "02E8AA49",
    "FCBB1707",
    "2A94257F",
    "1339A425",
    "D45FD6C4",
    "8EE00B00",
    "4A782808",
    "B6678572",
    "D3DC9613",
    "E3849A15",
    "1C037A07",
    "21B67709",
    "11F1A5CE",
    "B0EEF272",
    "0EE4A83E",
    "54F9ACE8",
    pcb::BOX,
    pcb::LAYER,
    pcb::COPPER,
];

/// A custom feature (`ElectronicsPackageGenerator`): a group of ordinary features it made.
pub(super) const CUSTOM_FEATURE: &str = "827B3935";

/// Feature ids in timeline order (the reference lists of the `2F4C1849` records), then the
/// features of designs whose history is not captured, in compute order (the lists of `B65B5682`).
fn timeline(d: &Design) -> (Vec<u64>, Vec<u64>) {
    let mut out: Vec<u64> = Vec::new();
    for &i in &d.order {
        if d.is(i, "2F4C1849") {
            for (_, ids) in ref_lists(d, d.rec(i), 0) {
                for q in ids {
                    if !out.contains(&q) {
                        out.push(q);
                        // A custom feature (ElectronicsPackageGenerator) stands in the timeline
                        // for the features it made: its first reference list, in order (section 30).
                        if d.is(q, CUSTOM_FEATURE)
                            && let Some((_, members)) = ref_lists(d, d.rec(q), 0).into_iter().next()
                        {
                            for m in members {
                                if m != q && !out.contains(&m) {
                                    out.push(m);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut extra: Vec<u64> = Vec::new();
    for &i in &d.order {
        if d.is(i, "B65B5682") {
            for (_, ids) in ref_lists(d, d.rec(i), 0) {
                for q in ids {
                    if out.contains(&q) || extra.contains(&q) {
                        continue;
                    }
                    if FEATURE_PREFIXES.iter().any(|p| d.is(q, p)) && feature_name(d, q).0.is_some() {
                        extra.push(q);
                    }
                }
            }
        }
    }
    (out, extra)
}

pub use joint::{JointPlacement, Joints};
pub use occpattern::PatternCopies;

/// The occurrences that the rectangular patterns of components of one design stream made, each
/// relative to the original occurrence ([`PatternCopies`]); patterns whose stored copies are not
/// exactly their parameters' lattice are left out (the tree reader then refuses them).
pub fn pattern_copies(meta: &[u8], bulk: &[u8]) -> Vec<PatternCopies> {
    if meta.len().saturating_add(bulk.len()) > MAX_STREAM_BYTES {
        return Vec::new();
    }
    let Some(d) = Design::new(meta, bulk) else { return Vec::new() };
    d.ids
        .iter()
        .filter(|i| d.is(**i, "AE12AD3F") || d.is(**i, "E8816367"))
        .filter_map(|i| occpattern::copies_of(&d, *i).ok())
        .collect()
}

/// What the joints of one design stream say: how many agree with the placements the stream
/// stores for the occurrences they move, why the others could not be checked, and, when all
/// agree, where every occurrence of the design stands ([`Joints`]). Nothing for a stream
/// without joints.
pub fn joint_placements(meta: &[u8], bulk: &[u8]) -> Joints {
    if meta.len().saturating_add(bulk.len()) > MAX_STREAM_BYTES {
        return Joints::default();
    }
    Design::new(meta, bulk).map(|d| joint::read(&d)).unwrap_or_default()
}

/// Where the occurrence path records (`549FBB80`) of a design stream put its occurrences, for a
/// stream whose occurrences the bodies reader ([`crate::fusion::read_design`]) does not find:
/// one whose BulkStream carries no class tags, so that its occurrences (`CE2913AA`) are known by
/// the MetaStream's class table only. The reader places such a design's components by the
/// blocks of the older layout, which can be a matrix of something else (section 45 of
/// `docs/formats/fusion-design-stream.md`). Empty for any other stream, or when a path record is
/// not laid out as known. Apply with [`place_by_joints`].
pub fn occurrence_placements(meta: &[u8], bulk: &[u8]) -> Joints {
    if meta.len().saturating_add(bulk.len()) > MAX_STREAM_BYTES || crate::fusion::has_occurrence_records(bulk) {
        return Joints::default();
    }
    let placements = Design::new(meta, bulk).map(|d| joint::paths(&d)).unwrap_or_default();
    Joints { placements, ..Joints::default() }
}

/// Puts the bodies of a design where its occurrence path records have them, when
/// [`joint_placements`] could check every joint of the design against those records: a body the
/// occurrence records place elsewhere (where it stood before a joint moved it) or not at all
/// gets the placements of its component's occurrence paths. Nothing changes otherwise.
pub fn place_by_joints(bodies: &mut Vec<crate::fusion::Placed>, joints: &Joints) {
    joint::place(bodies, joints);
}

/// Adds to `bodies` the occurrences the patterns of components made: every body placed by an
/// occurrence of a repeated component gets one more placement per copy (the occurrence's
/// placement, then the copy's).
pub fn add_pattern_copies(bodies: &mut Vec<crate::fusion::Placed>, sets: &[PatternCopies]) {
    let originals = bodies.clone();
    for set in sets {
        let mine = |brep: &str| {
            set.blobs.iter().any(|b| b.strip_prefix("BREP.").and_then(|b| b.strip_suffix(".smb")) == Some(brep))
        };
        for p in originals.iter().filter(|p| mine(&p.brep)) {
            for (k, c) in set.copies.iter().enumerate() {
                bodies.push(crate::fusion::Placed {
                    brep: p.brep.clone(),
                    occurrence: format!("{} [{}]", p.occurrence, k + 2),
                    placement: p.placement.then(c),
                });
            }
        }
    }
}

/// The user-visible name of a component record.
fn component_name(d: &Design, c: u64) -> Option<String> {
    wstrings_all(d.rec(c))
        .into_iter()
        .map(|w| w.2)
        .rfind(|w| !stream::is_uuid_text(w) && !w.starts_with("PrismMaterial"))
}

/// The body blobs (`BREP.<id>.smb`) a component lists.
fn component_bodies(d: &Design, c: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for q in ref_ids(d, d.rec(c)) {
        if d.is(q, "CD57BC48") {
            for q2 in ref_ids(d, d.rec(q)) {
                for w in wstrings_all(d.rec(q2)) {
                    if w.2.starts_with("BREP.") && w.2.ends_with(".smb") && !out.contains(&w.2) {
                        out.push(w.2);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
