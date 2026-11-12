//! The features of a board made from PCB data (2025 files; ADR 0039 round 15, section 23 of the
//! format notes): `BoxPrimitive` `F193B033`, and `PCBExtrude` in its two classes, `94B9F77E`
//! (drill, plated holes, soldermask) and `2BB91A84` (the copper of one side).
//!
//! * A box is its five parameters and a 4x4 placement of its corner: a rectangle sketch and an
//!   extrude, accepted only when the box that gives is the box of the body the file stores for it.
//! * A copper body and the plated-hole body come from the board file, which the design does not
//!   hold: they are the bodies the feature's state list stores (one tree body per lump).
//! * A soldermask is the board's outline extruded by the thickness its stored body shows (which
//!   the layer record must hold), then drilled.
//! * The drill is a cut through the stored drill tool's cylinders: the record's table lists the
//!   pad holes only (the vias are in the board file), so the holes are read off the tool the
//!   component stores, and every table entry must be one of them.

use super::acis::{self, BodyShape};
use super::feature::{params_of, pget};
use super::geom::{self, V3, mm, round9};
use super::stream::{f64_at, parse_header, ref_end, ref_ids, refs_at, u32_at, wstrings_all};
use super::{Ctx, Graph, component_bodies};
use crate::cover;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub(super) const BOX: &str = "F193B033";
pub(super) const LAYER: &str = "94B9F77E";
pub(super) const COPPER: &str = "2BB91A84";
pub(super) const COIL: &str = "DB2E4EDC";

/// Most holes one drill is read with (the importer's limit of profile picks).
const MAX_HOLES: usize = 256;

/// A body one of these features made: the history blob that stores it, its place among the
/// blob's bodies, how many bodies the blob holds, and what the body is.
struct Made {
    blob: String,
    index: usize,
    count: usize,
    shape: BodyShape,
}

/// One hole of a drill tool: centre and radius (cm) in the component's frame.
type Hole = (f64, f64, f64);

/// A drill tool: its holes and the z range it spans (cm).
#[derive(Debug, PartialEq)]
pub(super) struct Drill {
    pub(super) holes: Vec<Hole>,
    pub(super) z: (f64, f64),
}

/// The drill tools among the bodies of one blob: every body that is nothing but whole cylinders
/// along z, one per lump (a board can have several: 6250 stores the holes of its jack apart, on
/// a shorter tool).
pub(super) fn drills_of(shapes: &[BodyShape]) -> Vec<Drill> {
    let mut found: Vec<Drill> = Vec::new();
    for s in shapes {
        // Not `flat()`: the vertices of a tool of one hole are the two ends of its seam.
        if s.other_surfaces || s.transformed || s.lumps == 0 {
            continue;
        }
        let Some((lo, hi)) = s.vertex_box else { continue };
        let mut holes: Vec<Hole> = Vec::new();
        let mut upright = true;
        for (root, axis, r) in &s.cylinders {
            if axis[2].abs() < 1.0 - 1e-12 {
                upright = false;
                break;
            }
            let same =
                |h: &Hole| (h.0 - root[0]).abs() < 1e-9 && (h.1 - root[1]).abs() < 1e-9 && (h.2 - r).abs() < 1e-9;
            if !holes.iter().any(same) {
                holes.push((root[0], root[1], *r));
            }
        }
        // One cylinder per lump: a body with holes in it has more cylinders than lumps.
        if !upright || holes.len() != s.lumps || hi[2] - lo[2] < 1e-9 {
            continue;
        }
        found.push(Drill { holes, z: (lo[2], hi[2]) });
    }
    found
}

/// The sketch of a drill's circles (on the plane through the middle of the tool) and the cut that
/// runs the tool's length through `target`.
pub(super) fn drill_json(drill: &Drill, name: &str, sketch: &str, target: &str, fid: u64) -> Vec<Value> {
    let mid = (drill.z.0 + drill.z.1) / 2.0;
    let mut points = Vec::new();
    let mut curves = Vec::new();
    let mut picks = Vec::new();
    for (k, (x, y, r)) in drill.holes.iter().enumerate() {
        points.push(json!({"id": format!("p{k}"), "x": mm(*x), "y": mm(*y)}));
        curves.push(json!({"id": format!("c{k}"), "type": "circle", "center": format!("p{k}"), "radius": mm(*r)}));
        picks.push(json!({"point": [mm(*x), mm(*y)]}));
    }
    vec![
        json!({"type": "sketch", "name": sketch, "source_id": fid,
            "plane": {"origin": [0.0, 0.0, mm(mid)], "x_axis": [1.0, 0.0, 0.0], "y_axis": [0.0, 1.0, 0.0]},
            "plane_note": "through the middle of the stored drill tool",
            "points": points, "curves": curves}),
        json!({"type": "extrude", "name": name, "source_id": fid, "sketch": sketch, "profiles": picks,
            "distance": mm(drill.z.1 - drill.z.0), "symmetric": true, "operation": "cut", "target": target,
            "creates_bodies": 0,
            "holes_from": "the cylinders of the drill tool the component stores"}),
    ]
}

/// Column `k` of the upper three rows of a row-major 4x4.
fn column(m: &[f64; 16], k: usize) -> Option<V3> {
    Some([*m.get(k)?, *m.get(k + 4)?, *m.get(k + 8)?])
}

/// The rigid frame an origin snapshot stores (the first in its record; translation in mm).
fn snapshot_frame(rec: &[u8]) -> Option<crate::fusion::Placement> {
    (0..rec.len().saturating_sub(127)).find_map(|at| crate::fusion::rigid_at(rec, at))
}

/// `s` applied to the frame `m` (row-major 4x4, cm).
fn compose_cm(s: &crate::fusion::Placement, m: &[f64; 16]) -> [f64; 16] {
    let g = |i: usize, j: usize| m.get(4 * i + j).copied().unwrap_or(0.0);
    let r = |i: usize, j: usize| s.r.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0.0);
    let mut out = *m;
    for i in 0..3 {
        for j in 0..4 {
            let v: f64 = (0..3).map(|k| r(i, k) * g(k, j)).sum();
            let t = if j == 3 { s.t.get(i).copied().unwrap_or(0.0) / 10.0 } else { 0.0 };
            if let Some(slot) = out.get_mut(4 * i + j) {
                *slot = v + t;
            }
        }
    }
    out
}

/// The corner frame and the sizes of a box against the box of the body stored for it: the signed
/// height along the frame's z that reproduces the stored box, when one of the two does.
pub(super) fn box_height(m: &[f64; 16], size: V3, stored: (V3, V3)) -> Option<f64> {
    let (x, y, z, o) = (column(m, 0)?, column(m, 1)?, column(m, 2)?, column(m, 3)?);
    for s in [1.0, -1.0] {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for corner in 0..8u8 {
            let mut p = o;
            for (bit, axis, len) in [(1u8, x, size[0]), (2, y, size[1]), (4, z, s * size[2])] {
                if corner & bit != 0 {
                    p = geom::add(p, geom::mul(axis, len));
                }
            }
            for i in 0..3 {
                if let (Some(l), Some(h), Some(v)) = (lo.get_mut(i), hi.get_mut(i), p.get(i)) {
                    *l = l.min(*v);
                    *h = h.max(*v);
                }
            }
        }
        let near = |a: V3, b: V3| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-7);
        if near(lo, stored.0) && near(hi, stored.1) {
            return Some(s * size[2]);
        }
    }
    None
}

/// The kind of a `94B9F77E` record and the offset after it: the `u32` behind the record's
/// `C46D3EEB` reference and a zero byte (0 the drill, 1 the plated holes, 2 and 3 a soldermask);
/// a `u32` count follows it.
fn layer_kind(cx: &Ctx, rec: &[u8]) -> Option<(u32, usize)> {
    let d = cx.d;
    let (at, _) = refs_at(d, rec, 0).into_iter().find(|(_, q)| d.is(*q, "C46D3EEB"))?;
    let (_, end) = ref_end(d, rec, at)?;
    (rec.get(end) == Some(&0)).then_some(())?;
    let kind = u32_at(rec, end + 1)?;
    cover::skip(rec, end, 1, "layer.pad");
    cover::field(rec, end + 1, 4, "layer.kind");
    Some((kind, end + 5))
}

/// The drill record's table: `u32 n` and `n` entries of 40 bytes: a zero `u32`, x and y (cm) and
/// twenty zero bytes.
pub(super) fn hole_table(rec: &[u8], at: usize) -> Option<Vec<(f64, f64)>> {
    let n = usize::try_from(u32_at(rec, at)?).ok().filter(|n| *n <= 100_000)?;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let o = at + 4 + 40 * k;
        let (x, y) = (f64_at(rec, o + 4)?, f64_at(rec, o + 12)?);
        let zeros = rec.get(o + 20..o + 40).is_some_and(|z| z.iter().all(|b| *b == 0));
        if !(x.is_finite() && y.is_finite() && zeros) || u32_at(rec, o) != Some(0) {
            return None;
        }
        out.push((x, y));
    }
    cover::field(rec, at, 4, "drill.count");
    for k in 0..n {
        let o = at + 4 + 40 * k;
        cover::skip(rec, o, 4, "drill.zero");
        cover::field(rec, o + 4, 16, "drill.xy");
        cover::skip(rec, o + 20, 20, "drill.zeros");
    }
    Some(out)
}

impl Ctx<'_> {
    fn shapes(&self, blob: &str) -> Option<Vec<BodyShape>> {
        let (_, bytes) = self.acis.iter().find(|(n, _)| n.rsplit('/').next() == Some(blob))?;
        acis::body_shapes(bytes)
    }

    /// The bodies `fid` made, each found in the history blob its state list names: the list's
    /// bodies are the blob's solid bodies in order (a blob also holds flat sheet bodies, which
    /// no list names).
    fn made(&self, g: &Graph, fid: u64) -> Result<Vec<Made>, String> {
        let d = self.d;
        let mut out = Vec::new();
        let mut links: Vec<u64> = Vec::new();
        for link in ref_ids(d, d.rec(fid)) {
            if d.is(link, "D26351F0") && g.creator.get(&link) == Some(&fid) && !links.contains(&link) {
                links.push(link);
            }
        }
        for link in links {
            let dm = ref_ids(d, d.rec(link))
                .into_iter()
                .find(|q| d.is(*q, "D3937028"))
                .ok_or("a body link names no body")?;
            let mut found: Option<(String, Vec<u64>)> = None;
            for list in ref_ids(d, d.rec(dm)).into_iter().filter(|q| d.is(*q, "CD57BC48")) {
                let members = ref_ids(d, d.rec(list));
                let name = members.iter().find_map(|q| {
                    wstrings_all(d.rec(*q))
                        .into_iter()
                        .map(|w| w.2)
                        .find(|w| w.starts_with("BREP.") && w.ends_with(".smbh"))
                });
                if let Some(name) = name {
                    let mut bodies: Vec<u64> = Vec::new();
                    for q in members.into_iter().filter(|q| d.is(*q, "D3937028")) {
                        if !bodies.contains(&q) {
                            bodies.push(q);
                        }
                    }
                    found = Some((name, bodies));
                    break;
                }
            }
            let (blob, members) = found.ok_or("a body names no history blob")?;
            let at = members.iter().position(|q| *q == dm).ok_or("a body is not in its own state list")?;
            let shapes = self.shapes(&blob).ok_or_else(|| format!("the history blob {blob} cannot be read"))?;
            let solid: Vec<usize> = shapes.iter().enumerate().filter(|(_, s)| !s.flat()).map(|(k, _)| k).collect();
            if solid.len() != members.len() {
                return Err(format!(
                    "the history blob holds {} solid bodies, its state list {}",
                    solid.len(),
                    members.len()
                ));
            }
            let (Some(index), count) = (solid.get(at).copied(), shapes.len()) else {
                return Err("a body is not in its history blob".into());
            };
            let shape = shapes.into_iter().nth(index).ok_or("a body is not in its history blob")?;
            if shape.transformed {
                return Err("a stored body carries a transform".into());
            }
            out.push(Made { blob, index, count, shape });
        }
        Ok(out)
    }

    /// The drill tools the component of `fid` stores among its bodies.
    fn component_drills(&self, g: &Graph, fid: u64) -> Result<Vec<Drill>, String> {
        let Some(comp) = g.component(self.d, fid) else { return Ok(Vec::new()) };
        let mut found = Vec::new();
        for blob in component_bodies(self.d, comp) {
            let shapes = self.shapes(&blob).ok_or_else(|| format!("the body blob {blob} cannot be read"))?;
            found.extend(drills_of(&shapes));
        }
        if found.iter().any(|f| f.holes.len() > MAX_HOLES) {
            return Err(format!("a drill has more than {MAX_HOLES} holes"));
        }
        Ok(found)
    }

    /// `BoxPrimitive`: a rectangle sketch on the box's corner frame and an extrude.
    pub(super) fn box_primitive(&self, g: &Graph, fid: u64, name: &str) -> Result<Vec<Value>, String> {
        let d = self.d;
        let rec = d.rec(fid);
        let ps = params_of(d, fid);
        let val = |n: &str| pget(&ps, n).and_then(|p| p.value).ok_or_else(|| format!("the box has no {n}"));
        let size = [val("DistanceX")?, val("DistanceY")?, val("DistanceZ")?];
        if size.iter().take(2).any(|v| !(v.is_finite() && *v > 0.0)) {
            return Err("a size of the box is not positive".into());
        }
        // The height is signed: a box joined to or cut from a body stands on the side of its
        // frame's z the sign names (2354: -0.05 cm for a cut into a board, section 43).
        let dz = size[2];
        if !(dz.is_finite() && dz != 0.0) {
            return Err("the box has no height".into());
        }
        // The record's body: `01`, the operation (`u32`, the extrude's numbers: 1 join, 2 cut,
        // 4 new body), `01` and the first reference.
        let h = parse_header(rec).ok_or("the box's record is not recognised")?;
        cover::field(rec, h.body + 1, 4, "box.operation");
        let op = match (rec.get(h.body), u32_at(rec, h.body + 1)) {
            (Some(1), Some(1)) => "join",
            (Some(1), Some(2)) => "cut",
            (Some(1), Some(4)) => "new",
            _ => return Err("the box's operation is not a known value".into()),
        };
        // The 4x4 (row-major, cm) stands two bytes behind the last of the five parameter holders.
        // A plain reference ends in its own zero byte; a typed one (the class GUID spelled out,
        // the first reference to a class in the stream: 3000) does not, and is followed by
        // three bytes (section 43).
        let holders: Vec<usize> =
            refs_at(d, rec, 0).into_iter().filter(|(_, q)| d.is(*q, "D91D429C")).map(|(o, _)| o).take(5).collect();
        let (start, end) = holders
            .get(4)
            .and_then(|o| ref_end(d, rec, *o).map(|(_, e)| (*o, e)))
            .ok_or("the box does not name five parameters")?;
        let pad = if end - start > 10 { 3 } else { 2 };
        let at = end + pad;
        let mut m = [0.0f64; 16];
        for (k, v) in m.iter_mut().enumerate() {
            *v = f64_at(rec, at + 8 * k).filter(|v| v.is_finite()).ok_or("the box has no placement")?;
        }
        cover::skip(rec, end, pad, "box.pad");
        cover::field(rec, at, 128, "box.placement");
        // The corner frame is in the frame of the box's origin snapshot (`7B6C3C2D`, the first
        // one the record names): 3000's box stores (4, -18.85) mm with its x along +y, and its
        // snapshot a half turn about z, which puts it at (-4, 18.85) mm with its x along -y,
        // where the stored body and the faces Draft1 tapers have it (section 45).
        if let Some(s) = refs_at(d, rec, 0)
            .into_iter()
            .map(|(_, q)| q)
            .find(|q| d.is(*q, "7B6C3C2D"))
            .and_then(|q| snapshot_frame(d.rec(q)))
        {
            m = compose_cm(&s, &m);
        }
        let frame = (column(&m, 0), column(&m, 1), column(&m, 2), column(&m, 3));
        let (Some(x), Some(y), Some(z), Some(o)) = frame else { return Err("the box has no placement".into()) };
        let unit = |a: V3| (geom::norm(a) - 1.0).abs() < 1e-9;
        let right = geom::dot(geom::cross(x, y), z);
        if !(unit(x) && unit(y) && unit(z))
            || geom::dot(x, y).abs() > 1e-9
            || (right.abs() - 1.0).abs() > 1e-9
            || m.get(12..) != Some(&[0.0, 0.0, 0.0, 1.0][..])
        {
            return Err("the box's placement is not a rigid frame".into());
        }
        let (height, from) = if op == "new" {
            let made = self.made(g, fid)?;
            let [one] = made.as_slice() else { return Err(format!("the box makes {} bodies, not one", made.len())) };
            let stored = one.shape.vertex_box.ok_or("the box's stored body has no vertices")?;
            let height = box_height(&m, [size[0], size[1], dz.abs()], stored)
                .ok_or("the box of its parameters and placement is not the box of its stored body")?;
            (height, "the side of the corner frame on which the stored body lies")
        } else {
            (dz, "the sign of the box's height along the corner frame's z")
        };
        let sketch = format!("{name} profile");
        let dir = |v: V3| json!([round9(v[0]), round9(v[1]), round9(v[2])]);
        let (w, h) = (mm(size[0]), mm(size[1]));
        Ok(vec![
            json!({"type": "sketch", "name": sketch, "source_id": fid,
                "plane": {"origin": [mm(o[0]), mm(o[1]), mm(o[2])], "x_axis": dir(x), "y_axis": dir(y)},
                "plane_note": "the box's stored corner frame",
                "points": [{"id": "p0", "x": 0.0, "y": 0.0}, {"id": "p1", "x": w, "y": 0.0},
                    {"id": "p2", "x": w, "y": h}, {"id": "p3", "x": 0.0, "y": h}],
                "curves": [{"id": "c0", "type": "line", "start": "p0", "end": "p1"},
                    {"id": "c1", "type": "line", "start": "p1", "end": "p2"},
                    {"id": "c2", "type": "line", "start": "p2", "end": "p3"},
                    {"id": "c3", "type": "line", "start": "p3", "end": "p0"}]}),
            // The sketch normal is x cross y: the frame's z or its opposite.
            json!({"type": "extrude", "name": name, "source_id": fid, "sketch": sketch,
                "profiles": [{"point": [mm(size[0] / 2.0), mm(size[1] / 2.0)]}],
                "distance": mm(height * right), "symmetric": false, "operation": op, "target": null,
                "creates_bodies": u64::from(op == "new"), "direction_from": from}),
        ])
    }

    /// `CoilPrimitive` `DB2E4EDC`: a triangle swept along a helix (sections 29, 40, 46). The
    /// record's body: `u8 0`, the operation (`u32`, 4 a new body), `u32 1`, then the `Diameter`
    /// and `Height` holders, a `DA1231DE` entity, the coil's origin snapshot (`7B6C3C2D`), the
    /// `Revolutions` holder and a `u32` (the section's position), the `SectionSize` holder and a
    /// `u32` (the section's type), `TaperAngle`, `u32 1` and the body link. The coil climbs the
    /// snapshot's z from its origin and starts on its x (3722: the root's coil has no matrix and
    /// starts on +x; the Cap's a half turn about z and starts on -x, as their stored helices
    /// do). Both stored coils turn right-handed; the hand is not read (no left-handed sample).
    pub(super) fn coil_primitive(&self, g: &Graph, fid: u64, name: &str) -> Result<Vec<Value>, String> {
        let d = self.d;
        let rec = d.rec(fid);
        let ps = params_of(d, fid);
        let val = |n: &str| pget(&ps, n).and_then(|p| p.value).ok_or_else(|| format!("the coil has no {n}"));
        let (dia, height, size, turns) = (val("Diameter")?, val("Height")?, val("SectionSize")?, val("Revolutions")?);
        if ![dia, height, size, turns].iter().all(|v| v.is_finite() && *v > 0.0) || size >= dia / 2.0 {
            return Err("a size of the coil is not positive".into());
        }
        if val("TaperAngle")? != 0.0 {
            return Err("the coil is tapered".into());
        }
        if ps.iter().any(|(n, _)| n == "Pitch") {
            return Err("the coil has a pitch parameter, which is not read".into());
        }
        // Each holder by its parameter's name, and the `u32` behind the holder.
        let refs = refs_at(d, rec, 0);
        let after = |want: &str| -> Option<u32> {
            let (at, _) = refs.iter().find(|(_, q)| {
                d.is(*q, "D91D429C")
                    && ref_ids(d, d.rec(*q)).into_iter().any(|p| {
                        d.is(p, "7A6A3D31") && super::stream::param(d, p).and_then(|p| p.name).as_deref() == Some(want)
                    })
            })?;
            let (_, end) = ref_end(d, rec, *at)?;
            let skip = rec.get(end..)?.iter().take(3).take_while(|b| **b == 0).count();
            cover::field(rec, end + skip, 4, "coil.enum");
            u32_at(rec, end + skip)
        };
        let first = refs.iter().find(|(_, q)| d.is(*q, "D91D429C")).map(|(o, _)| *o).ok_or("the coil has no holder")?;
        cover::field(rec, first.saturating_sub(8), 4, "coil.operation");
        if u32_at(rec, first.saturating_sub(4)) != Some(1) || u32_at(rec, first.saturating_sub(8)) != Some(4) {
            return Err("the coil does not make a new body (join, cut and intersect are not read)".into());
        }
        // Position and type, as seen: 3 and 3 (the triangle outside the diameter, pointing out),
        // 1 and 4 (inside it, pointing in). The triangle is equilateral, `SectionSize` across
        // the radius, its side on the diameter.
        let (r, apex) = match (after("Revolutions"), after("SectionSize")) {
            (Some(3), Some(3)) => (dia / 2.0, dia / 2.0 + size),
            (Some(1), Some(4)) => (dia / 2.0, dia / 2.0 - size),
            (p, t) => return Err(format!("the coil's section ({p:?}, {t:?}) is not one seen")),
        };
        let half = size / 3f64.sqrt();
        let prof = [(r, -half), (r, half), (apex, 0.0)];
        let s =
            refs.iter().map(|(_, q)| *q).find(|q| d.is(*q, "7B6C3C2D")).ok_or("the coil names no origin snapshot")?;
        let (rot, t) = snapshot_frame(d.rec(s))
            .map_or(([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3]), |p| (p.r, p.t));
        let at = |i: usize, k: usize| rot.get(i).and_then(|row| row.get(k)).copied().unwrap_or(0.0);
        let col = |k: usize| -> V3 { [at(0, k), at(1, k), at(2, k)] };
        let (fx, fy, fz, o) = (col(0), col(1), col(2), t.map(|c| c / 10.0));
        // Where the history blob holds the coil's body (or a later state of it), that body has
        // the box of the coil's ends. 3722's state lists do not name their own bodies; the
        // importer's state checks then confirm the coil.
        let stored = match self.made(g, fid) {
            Ok(made) => match made.as_slice() {
                [one] => one.shape.vertex_box,
                _ => return Err(format!("the coil makes {} bodies, not one", made.len())),
            },
            Err(_) => None,
        };
        let mut blo = [f64::INFINITY; 3];
        let mut bhi = [f64::NEG_INFINITY; 3];
        let end = std::f64::consts::TAU * turns;
        for (a, h0) in [(0.0f64, 0.0), (end, height)] {
            for (pr, ph) in prof {
                let p = geom::add(
                    o,
                    geom::add(
                        geom::add(geom::mul(fx, pr * libm::cos(a)), geom::mul(fy, pr * libm::sin(a))),
                        geom::mul(fz, h0 + ph),
                    ),
                );
                for i in 0..3 {
                    if let (Some(l), Some(h), Some(v)) = (blo.get_mut(i), bhi.get_mut(i), p.get(i)) {
                        *l = l.min(*v);
                        *h = h.max(*v);
                    }
                }
            }
        }
        let sizes_agree = |(lo, hi): (V3, V3)| {
            (0..3).all(|i| {
                let span = |a: &V3, b: &V3| b.get(i).copied().unwrap_or(0.0) - a.get(i).copied().unwrap_or(0.0);
                (span(&blo, &bhi) - span(&lo, &hi)).abs() < 1e-6
            })
        };
        if stored.is_some_and(|b| !sizes_agree(b)) {
            return Err("the box of the coil's ends is not the box of its stored body".into());
        }
        let v3 = |v: V3| json!([round9(v[0]), round9(v[1]), round9(v[2])]);
        let sketch = format!("{name} section");
        let points: Vec<Value> = prof
            .iter()
            .enumerate()
            .map(|(j, (pr, ph))| json!({"id": format!("p{j}"), "x": mm(*pr), "y": mm(*ph)}))
            .collect();
        let curves: Vec<Value> = (0..3)
            .map(|j| json!({"id": format!("l{j}"), "type": "line", "start": format!("p{j}"), "end": format!("p{}", (j + 1) % 3)}))
            .collect();
        Ok(vec![
            json!({"type": "sketch", "name": sketch,
                "plane": {"origin": v3(o.map(mm)), "x_axis": v3(fx), "y_axis": v3(fz)},
                "plane_note": "the coil's axial section at its start", "points": points, "curves": curves}),
            json!({"type": "sweep", "name": name, "source_id": fid, "sketch": sketch, "operation": "new",
                "creates_bodies": 1,
                "helix": {"origin": v3(o.map(mm)), "axis": v3(fz), "x": v3(fx), "radius": mm(r),
                    "pitch": mm(height / turns), "height": mm(height)}}),
        ])
    }

    /// The copper bodies of one side: the bodies the file stores for the feature, a tree body per
    /// lump (`lump` of `lumps`), named apart from `used`.
    pub(super) fn copper(
        &self,
        g: &Graph,
        fid: u64,
        name: &str,
        used: &mut HashSet<String>,
    ) -> Result<Vec<Value>, String> {
        let made = self.made(g, fid)?;
        let total: usize = made.iter().map(|m| m.shape.lumps).sum();
        if made.iter().any(|m| m.shape.lumps == 0) || total == 0 {
            return Err("a body of the feature has no lump".into());
        }
        let mut out = Vec::with_capacity(total);
        for m in &made {
            for lump in 0..m.shape.lumps {
                let mut body = if out.is_empty() { name.to_string() } else { format!("{name} {}", out.len() + 1) };
                while !out.is_empty() && used.contains(&body) {
                    body.push('_');
                }
                used.insert(body.clone());
                out.push(json!({"type": "stored_body", "name": body, "source_id": fid, "brep": m.blob,
                    "index": m.index, "count": m.count, "lump": lump, "lumps": m.shape.lumps,
                    "creates_bodies": total}));
            }
        }
        Ok(out)
    }

    /// A `94B9F77E` PCBExtrude by its kind. `Ok(None)`: it makes nothing (a drill without holes).
    pub(super) fn layer(
        &self,
        g: &Graph,
        fid: u64,
        name: &str,
        names: &HashMap<u64, String>,
        feats: &[Value],
        used: &mut HashSet<String>,
    ) -> Result<Option<Vec<Value>>, String> {
        let d = self.d;
        let rec = d.rec(fid);
        let (kind, after) = layer_kind(self, rec).ok_or("the record's kind is not found")?;
        let refs = ref_ids(d, rec);
        let layer = refs.iter().copied().find(|q| d.is(*q, "369DDC0A")).ok_or("it names no layer data")?;
        let board = ref_ids(d, d.rec(layer))
            .into_iter()
            .find(|q| d.is(*q, super::EXTRUDE))
            .ok_or("its layer data names no board extrude")?;
        let board_name = names.get(&board).ok_or("the board's extrude is not in the tree")?;
        let mut fresh = |n: String| -> String {
            let mut n = n;
            while used.contains(&n) {
                n.push('_');
            }
            used.insert(n.clone());
            n
        };
        match kind {
            0 => {
                let table = hole_table(rec, after).ok_or("the drill's hole table is not read")?;
                let drills = self.component_drills(g, fid)?;
                for (x, y) in &table {
                    if !drills.iter().flat_map(|t| &t.holes).any(|h| (h.0 - x).abs() < 1e-7 && (h.1 - y).abs() < 1e-7) {
                        return Err("a hole of the drill's table is not a hole of a stored tool".into());
                    }
                }
                if drills.is_empty() {
                    return Ok(None);
                }
                let mut out = Vec::new();
                for (k, drill) in drills.iter().enumerate() {
                    let cut = if k == 0 { name.to_string() } else { fresh(format!("{name} tool {}", k + 1)) };
                    let sketch = fresh(format!("{cut} holes"));
                    out.extend(drill_json(drill, &cut, &sketch, board_name, fid));
                }
                Ok(Some(out))
            }
            1 => self.copper(g, fid, name, used).map(Some),
            2 | 3 => {
                let made = self.made(g, fid)?;
                let [one] = made.as_slice() else {
                    return Err(format!("the soldermask makes {} bodies, not one", made.len()));
                };
                let (lo, hi) = one.shape.vertex_box.ok_or("the soldermask's stored body has no vertices")?;
                // The outline is on the component's z = 0: the body lies on one side of it.
                let distance = match (lo[2].abs() < 1e-9, hi[2].abs() < 1e-9) {
                    (true, false) => hi[2],
                    (false, true) => lo[2],
                    _ => return Err("the soldermask's stored body does not start at the outline's plane".into()),
                };
                let lrec = d.rec(layer);
                let stored_thickness = (0..lrec.len().saturating_sub(7))
                    .any(|o| f64_at(lrec, o).is_some_and(|v| (v - distance.abs()).abs() < 1e-12));
                if !stored_thickness {
                    return Err("the layer data does not hold the thickness of the soldermask's stored body".into());
                }
                let entry = |ty: &str, key: &str, v: &Value| {
                    feats.iter().find(|f| f.get("type").and_then(Value::as_str) == Some(ty) && f.get(key) == Some(v))
                };
                let ext =
                    entry("extrude", "source_id", &board.into()).ok_or("the board's extrude is not in the tree")?;
                let outline = ext.get("sketch").and_then(|s| entry("sketch", "name", s));
                let (Some(Value::Object(outline)), Some(picks)) = (outline, ext.get("profiles")) else {
                    return Err("the board's outline sketch is not in the tree".into());
                };
                if ext.get("error").is_some() || ext.get("operation").and_then(Value::as_str) != Some("new") {
                    return Err("the board's extrude is not read".into());
                }
                let mut copy = outline.clone();
                copy.retain(|k, _| !k.starts_with('_') && k != "component" && k != "in_timeline");
                let sketch = fresh(format!("{name} outline"));
                copy.insert("name".into(), sketch.clone().into());
                copy.insert("copy_of".into(), ext.get("sketch").cloned().unwrap_or(Value::Null));
                let mut out = vec![
                    Value::Object(copy),
                    json!({"type": "extrude", "name": name, "source_id": fid, "sketch": sketch, "profiles": picks,
                        "distance": mm(distance), "symmetric": false, "operation": "new", "target": null,
                        "creates_bodies": 1,
                        "distance_from": "the thickness of the stored body, which the layer data holds"}),
                ];
                for (k, drill) in self.component_drills(g, fid)?.iter().enumerate() {
                    let cut = fresh(if k == 0 { format!("{name} drill") } else { format!("{name} drill {}", k + 1) });
                    let holes = fresh(format!("{cut} holes"));
                    out.extend(drill_json(drill, &cut, &holes, name, fid));
                }
                Ok(Some(out))
            }
            k => Err(format!("its kind {k} is not known")),
        }
    }
}
