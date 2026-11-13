//! Joints (`Assemble`, class `8DFDD543`) and the placements they leave (section 24 of
//! `docs/formats/fusion-design-stream.md`).
//!
//! A joint makes no geometry: it moves an occurrence so that a frame on it lands on a frame of
//! another occurrence. The file stores, for every occurrence path (`549FBB80`), where the path
//! stood after each timeline feature that moved it, so a joint's result need not be solved. It
//! is read from there, and only after the joint's own record was checked against it:
//!
//! * the record's first origin frame, flipped (half a turn about its x axis) when the flip byte
//!   is set, then moved back by the typed offsets and angle, is the frame the joint's snapshot
//!   (`7F5A0426`) stores for the first side;
//! * the first side's stored placement after the joint carries that frame onto the second
//!   side's frame.
//!
//! A joint that fails either check, or whose records are not laid out as described, is reported
//! as unverified and places nothing.

use super::occpattern::value;
use super::stream::{Design, ref_end, refs_at, u32_at};
use super::{component_bodies, component_name};
use crate::fusion::{Placed, Placement, rigid_at};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

const JOINT: &str = "8DFDD543";
const ORIGIN_SNAP: &str = "7B6C3C2D";
const ORIGIN_NAMED: &str = "02E8AA49";
const OCCURRENCE: &str = "CE2913AA";
const COMPONENT: &str = "E03784ED";
const PATH: &str = "549FBB80";
const SNAPSHOT: &str = "7F5A0426";

/// Deepest occurrence path read.
const MAX_PATH: usize = 32;
/// Most timeline steps one path record may list.
const MAX_STEPS: usize = 4096;
/// Most joints and path records read from one stream.
const MAX_JOINTS: usize = 10_000;
const MAX_PATHS: usize = 100_000;
/// How far into a snapshot record its two sides may start (bytes).
const SNAPSHOT_REACH: usize = 8192;
/// Two placements are the same within these (rotation entries; millimetres).
const TOL_TURN: f64 = 1e-6;
const TOL_MM: f64 = 1e-5;

/// One side of a joint as its snapshot stores it: the occurrence path inside `comp` (empty for
/// `comp` itself) and the side's frame in the coordinates of the path's last occurrence, the
/// joint's offsets already applied (`None`: the origin).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Side {
    pub frame: Option<Placement>,
    pub path: Vec<u64>,
    pub comp: u64,
}

/// A joint record.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Joint {
    pub id: u64,
    /// The first side's z axis points against the second side's.
    pub flip: bool,
    /// The two joint origins' frames as the record repeats them (`None`: the origin).
    pub origins: [Option<Placement>; 2],
    /// `alignAngle` (radians) and `alignOffsetX/Y/Z` (millimetres).
    pub angle: f64,
    pub offset: [f64; 3],
    pub sides: [Side; 2],
}

/// An occurrence path from the root component and where it stood after each timeline feature
/// that the record lists (`None`: the feature did not move it).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OccPath {
    pub path: Vec<u64>,
    pub steps: Vec<(u64, Option<Placement>)>,
}

/// A reference to an object of `class` at `at`, with the zero byte that follows it in a list:
/// the object and the offset after both.
fn listed(d: &Design, rec: &[u8], at: usize, class: &str) -> Option<(u64, usize)> {
    let (q, e) = ref_end(d, rec, at)?;
    (d.is(q, class) && rec.get(e) == Some(&0)).then_some((q, e + 1))
}

/// `u32 n`, `n` occurrences, then the component they are counted from: the path, the component
/// and the offset after them.
fn path_at(d: &Design, rec: &[u8], at: usize) -> Option<(Vec<u64>, u64, usize)> {
    let n = u32_at(rec, at)? as usize;
    if n > MAX_PATH {
        return None;
    }
    let mut pos = at + 4;
    let mut path = Vec::with_capacity(n);
    for _ in 0..n {
        let (q, e) = listed(d, rec, pos, OCCURRENCE)?;
        path.push(q);
        pos = e;
    }
    let (comp, e) = listed(d, rec, pos, COMPONENT)?;
    Some((path, comp, e))
}

/// An optional matrix: a byte (0: a 4x4 matrix follows, 1: none), then the matrix.
fn optional_matrix(rec: &[u8], at: usize) -> Option<(Option<Placement>, usize)> {
    match rec.get(at)? {
        0 => Some((Some(rigid_at(rec, at + 1)?), at + 129)),
        1 => Some((None, at + 1)),
        _ => None,
    }
}

fn sides_at(d: &Design, rec: &[u8], at: usize) -> Option<[Side; 2]> {
    let mut pos = at;
    let mut side = || {
        let (frame, e) = optional_matrix(rec, pos)?;
        let (path, comp, e) = path_at(d, rec, e)?;
        pos = e;
        Some(Side { frame, path, comp })
    };
    Some([side()?, side()?])
}

/// The two sides of a snapshot record: they follow a `u32 2` behind the list of every
/// occurrence involved (whose length varies with the class version, so the start is searched).
fn snapshot_sides(d: &Design, rec: &[u8]) -> Option<[Side; 2]> {
    (0..rec.len().min(SNAPSHOT_REACH)).filter(|k| u32_at(rec, *k) == Some(2)).find_map(|k| sides_at(d, rec, k + 4))
}

fn joint(d: &Design, id: u64) -> Result<Joint, String> {
    let rec = d.rec(id);
    let refs = refs_at(d, rec, 0);
    let is_origin = |q: u64| d.is(q, ORIGIN_SNAP) || d.is(q, ORIGIN_NAMED);
    let Some(&(first, _)) = refs.first().filter(|r| is_origin(r.1)) else {
        return Err("its record does not start with a joint origin".into());
    };
    let flip = match first.checked_sub(3).and_then(|k| rec.get(k)) {
        Some(0) => false,
        Some(1) => true,
        _ => return Err("its flip byte is not 0 or 1".into()),
    };
    let mut origins = [None, None];
    let mut pos = first;
    for slot in &mut origins {
        let side = ref_end(d, rec, pos).filter(|r| is_origin(r.0) && rec.get(r.1) == Some(&0));
        let Some((_, e)) = side else { return Err("it does not name two joint origins".into()) };
        let Some((m, e)) = optional_matrix(rec, e + 1) else {
            return Err("a joint origin's frame is not readable".into());
        };
        *slot = m;
        pos = e;
    }
    let cm = |name: &str| value(d, id, name).map(|v| v * 10.0);
    let (Some(angle), Some(x), Some(y), Some(z)) =
        (value(d, id, "alignAngle"), cm("alignOffsetX"), cm("alignOffsetY"), cm("alignOffsetZ"))
    else {
        return Err("it has no single alignAngle and alignOffsetX/Y/Z".into());
    };
    let Some(snapshot) = refs.iter().map(|r| r.1).find(|q| d.is(*q, SNAPSHOT)) else {
        return Err("it has no snapshot record".into());
    };
    let Some(sides) = snapshot_sides(d, d.rec(snapshot)) else {
        return Err("its snapshot names an occurrence outside this design or is not laid out as known".into());
    };
    Ok(Joint { id, flip, origins, angle, offset: [x, y, z], sides })
}

fn occ_path(d: &Design, id: u64) -> Option<OccPath> {
    let rec = d.rec(id);
    let (first, _) = refs_at(d, rec, 0).into_iter().find(|r| d.is(r.1, OCCURRENCE))?;
    let (path, _, mut pos) = path_at(d, rec, first.checked_sub(4)?)?;
    let count = u32_at(rec, pos + 4)? as usize;
    if count > MAX_STEPS {
        return None;
    }
    pos += 8;
    let mut steps = Vec::with_capacity(count);
    for _ in 0..count {
        if rec.get(pos..pos + 2) != Some(&[1, 0]) {
            return None;
        }
        let (q, e) = ref_end(d, rec, pos + 2).filter(|r| rec.get(r.1) == Some(&0))?;
        let (m, e) = optional_matrix(rec, e + 1)?;
        steps.push((q, m));
        pos = e;
    }
    Some(OccPath { path, steps })
}

/// Where a path stood once `joint` was computed: the last placement up to its step (the
/// origin for a path nothing had moved yet).
fn after(p: &OccPath, joint: u64) -> Placement {
    let mut last = Placement::IDENTITY;
    for (q, m) in &p.steps {
        if let Some(m) = m {
            last = *m;
        }
        if *q == joint {
            break;
        }
    }
    last
}

fn same(a: &Placement, b: &Placement) -> bool {
    a.r.iter().flatten().zip(b.r.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL_TURN)
        && a.t.iter().zip(&b.t).all(|(x, y)| (x - y).abs() <= TOL_MM)
}

/// The component an occurrence instantiates: the first one its record names.
fn occ_component(d: &Design, occ: u64) -> Option<u64> {
    refs_at(d, d.rec(occ), 0).into_iter().map(|r| r.1).find(|q| d.is(*q, COMPONENT))
}

/// The frame the snapshot must hold for the first side: the origin's frame, flipped, then
/// moved back by the offsets and the angle (`origin * flip * (offset * turn)^-1`).
fn first_frame(j: &Joint) -> Placement {
    let (s, c) = (libm::sin(j.angle), libm::cos(j.angle));
    let aligned = Placement { r: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]], t: j.offset };
    let flip = Placement { r: [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]], t: [0.0; 3] };
    let [origin, _] = j.origins;
    let origin = origin.unwrap_or(Placement::IDENTITY);
    let origin = if j.flip { origin.then(&flip) } else { origin };
    origin.then(&aligned.inverse())
}

/// Checks one joint against the stored placements, see the module's notes.
fn verify(d: &Design, j: &Joint, paths: &HashMap<&[u64], &OccPath>) -> Result<(), String> {
    let [one, two] = &j.sides;
    let (f1, f2) = (one.frame.unwrap_or(Placement::IDENTITY), two.frame.unwrap_or(Placement::IDENTITY));
    if !same(&first_frame(j), &f1) {
        return Err("its origin, flip, offsets and angle do not give the first side's stored frame".into());
    }
    let [_, origin2] = j.origins;
    if !same(&origin2.unwrap_or(Placement::IDENTITY), &f2) {
        return Err("its second origin is not the second side's stored frame".into());
    }
    if one.comp != two.comp {
        return Err("its sides are counted from different components".into());
    }
    // The paths are counted from a component: every occurrence path of that component leads
    // to them (none for the root component).
    let mut prefixes: Vec<&[u64]> =
        paths.keys().copied().filter(|p| p.last().and_then(|o| occ_component(d, *o)) == Some(one.comp)).collect();
    if prefixes.is_empty() {
        prefixes.push(&[]);
    }
    for prefix in prefixes {
        let world = |side: &Side| -> Result<Placement, String> {
            let full = [prefix, side.path.as_slice()].concat();
            if full.is_empty() {
                return Ok(Placement::IDENTITY);
            }
            paths
                .get(full.as_slice())
                .map(|p| after(p, j.id))
                .ok_or_else(|| "a side's path has no placement record".into())
        };
        if !same(&world(one)?.then(&f1), &world(two)?.then(&f2)) {
            return Err("the stored placement after it does not bring its two frames together".into());
        }
    }
    Ok(())
}

/// Where a design's joints left an occurrence.
#[derive(Debug, Clone, PartialEq)]
pub struct JointPlacement {
    /// The occurrences from the root component down to this one (record ids).
    pub path: Vec<u64>,
    /// The component the occurrence instantiates: its name and its body blobs
    /// (`BREP.<id>.smb`; none for an occurrence of an inserted design).
    pub name: String,
    pub blobs: Vec<String>,
    /// The occurrence in the design (millimetres), all its parents' placements included.
    pub placement: Placement,
}

/// What a design's joints say, see [`super::joint_placements`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Joints {
    /// Joints whose record agrees with the stored placements.
    pub verified: usize,
    /// The others: why each could not be checked.
    pub unverified: Vec<String>,
    /// The final placement of every occurrence path of the design, when every joint is verified
    /// and every path record was read (else empty).
    pub placements: Vec<JointPlacement>,
}

pub(super) fn read(d: &Design) -> Joints {
    let mut out = Joints::default();
    let ids: Vec<u64> = d.ids.iter().copied().filter(|i| d.is(*i, JOINT)).take(MAX_JOINTS).collect();
    if ids.is_empty() {
        return out;
    }
    let records: Vec<u64> = d.ids.iter().copied().filter(|i| d.is(*i, PATH)).take(MAX_PATHS).collect();
    let read: Vec<Option<OccPath>> = records.iter().map(|i| occ_path(d, *i)).collect();
    // A record that names occurrences but is not laid out as known: nothing is placed.
    let unread = records
        .iter()
        .zip(&read)
        .any(|(i, p)| p.is_none() && refs_at(d, d.rec(*i), 0).iter().any(|r| d.is(r.1, OCCURRENCE)));
    let paths: HashMap<&[u64], &OccPath> = read.iter().flatten().map(|p| (p.path.as_slice(), p)).collect();
    for id in ids {
        match joint(d, id).and_then(|j| verify(d, &j, &paths)) {
            Ok(()) => out.verified += 1,
            Err(why) => out.unverified.push(why),
        }
    }
    if !out.unverified.is_empty() || unread {
        if unread {
            out.unverified.push("an occurrence path's placement record is not laid out as known".into());
        }
        return out;
    }
    out.placements = final_placements(d, &read);
    out
}

/// The last placement each path record stores (the origin for a path nothing moved).
fn final_placements(d: &Design, read: &[Option<OccPath>]) -> Vec<JointPlacement> {
    let mut out = Vec::new();
    for p in read.iter().flatten() {
        // An occurrence of an inserted design names no component of this design.
        let component = p.path.last().and_then(|o| occ_component(d, *o));
        let placement = p.steps.iter().rev().find_map(|s| s.1).unwrap_or(Placement::IDENTITY);
        out.push(JointPlacement {
            path: p.path.clone(),
            name: component.and_then(|c| component_name(d, c)).unwrap_or_else(|| "Component".into()),
            blobs: component.map(|c| component_bodies(d, c)).unwrap_or_default(),
            placement,
        });
    }
    out
}

/// Where the occurrence path records of a design put every occurrence, joints or not (see
/// [`super::occurrence_placements`]): empty when a record that names occurrences is not laid
/// out as known.
pub(super) fn paths(d: &Design) -> Vec<JointPlacement> {
    let records: Vec<u64> = d.ids.iter().copied().filter(|i| d.is(*i, PATH)).take(MAX_PATHS).collect();
    let read: Vec<Option<OccPath>> = records.iter().map(|i| occ_path(d, *i)).collect();
    let unread = records
        .iter()
        .zip(&read)
        .any(|(i, p)| p.is_none() && refs_at(d, d.rec(*i), 0).iter().any(|r| d.is(r.1, OCCURRENCE)));
    if unread { Vec::new() } else { final_placements(d, &read) }
}

/// Where each component stood once each timeline feature that moved occurrences (a Position, a
/// joint) was computed, by (component, feature), for the components of one occurrence path
/// (none where a path record is not read).
pub(super) fn positions(d: &Design) -> std::collections::HashMap<(u64, u64), Placement> {
    let records: Vec<u64> = d.ids.iter().copied().filter(|i| d.is(*i, PATH)).take(MAX_PATHS).collect();
    let read: Vec<Option<OccPath>> = records.iter().map(|i| occ_path(d, *i)).collect();
    let mut out = std::collections::HashMap::new();
    let unread = records
        .iter()
        .zip(&read)
        .any(|(i, p)| p.is_none() && refs_at(d, d.rec(*i), 0).iter().any(|r| d.is(r.1, OCCURRENCE)));
    if unread {
        return out;
    }
    let comps: Vec<Option<u64>> =
        read.iter().flatten().map(|p| p.path.last().and_then(|o| occ_component(d, *o))).collect();
    for (p, c) in read.iter().flatten().zip(&comps) {
        let Some(c) = *c else { continue };
        if comps.iter().filter(|x| **x == Some(c)).count() != 1 {
            continue;
        }
        for (q, m) in &p.steps {
            if m.is_some() {
                out.insert((c, *q), after(p, *q));
            }
        }
    }
    out
}

/// See [`super::place_by_joints`].
pub(super) fn place(bodies: &mut Vec<Placed>, joints: &Joints) {
    let mut by_blob: BTreeMap<&str, Vec<(&str, Placement)>> = BTreeMap::new();
    for p in &joints.placements {
        for b in &p.blobs {
            if let Some(id) = b.strip_prefix("BREP.").and_then(|b| b.strip_suffix(".smb")) {
                by_blob.entry(id).or_default().push((p.name.as_str(), p.placement));
            }
        }
    }
    // The blobs whose placements differ from the occurrence paths'. Never fewer placements
    // than were read: a board's listed parts are placed without occurrences of their own.
    by_blob.retain(|id, want| {
        let have: Vec<&Placed> = bodies.iter().filter(|p| p.brep == *id).collect();
        let agree = have.len() == want.len() && want.iter().all(|w| have.iter().any(|h| same(&h.placement, &w.1)));
        let unplaced = have.is_empty() && want.iter().all(|w| same(&w.1, &Placement::IDENTITY));
        !(agree || unplaced || want.len() < have.len())
    });
    if by_blob.is_empty() {
        return;
    }
    let placed = |id: &str, want: &[(&str, Placement)]| -> Vec<Placed> {
        want.iter()
            .enumerate()
            .map(|(k, (name, placement))| Placed {
                brep: id.to_string(),
                occurrence: format!("{name}:{}", k + 1),
                placement: *placement,
            })
            .collect()
    };
    // In place: a blob's new placements stand where its first old one stood.
    let mut out: Vec<Placed> = Vec::with_capacity(bodies.len());
    let mut done: Vec<&str> = Vec::new();
    for p in bodies.iter() {
        match by_blob.get_key_value(p.brep.as_str()) {
            None => out.push(p.clone()),
            Some((id, want)) if !done.contains(id) => {
                done.push(id);
                out.extend(placed(id, want));
            }
            Some(_) => {}
        }
    }
    for (id, want) in &by_blob {
        if !done.contains(id) {
            out.extend(placed(id, want));
        }
    }
    *bodies = out;
}

/// See the call in [`super::built`]: a design inserted into the root component stands where
/// its occurrence's path record has it, not where the Component Insert first put it.
pub(super) fn place_inserts(tree: &mut Value, joints: &Joints) {
    let Some(feats) = tree.get_mut("features").and_then(Value::as_array_mut) else { return };
    for f in feats {
        if f.get("type").and_then(Value::as_str) != Some("component_insert") {
            continue;
        }
        let Some(occ) = f.get("_occurrence").and_then(Value::as_u64) else { continue };
        let Some(p) = joints.placements.iter().find(|p| p.path == [occ]) else { continue };
        let (r, t) = (p.placement.r, p.placement.t);
        if let Some(o) = f.as_object_mut() {
            o.insert("placement".into(), json!({"r": r, "t": t}));
        }
    }
}
