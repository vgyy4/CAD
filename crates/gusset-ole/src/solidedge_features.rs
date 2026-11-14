//! A Solid Edge part's ordered feature history as a Gusset feature tree
//! (`gusset-feature-tree` v2), read from the part's object store `PSMcluster0`. Clean room,
//! from sample bytes (`docs/formats/solidedge-feature-tree.md`).
//!
//! The store is a run of objects, each `QQ`, a 16-bit kind and a 32-bit serial (serials rise
//! through the store). A feature is an object whose first associative function is a
//! `JM<Name>Function`: a type signature (`%>r%<r...%n`), the class name and a list of slots,
//! each a reference (or list of references) to other objects by serial. Slot 1 is the
//! previous feature, which gives the history order. What is read here:
//!
//! * extruded protrusions and cutouts: profiles (slot 2), extent kind (slot 3: 13 finite,
//!   14 through all, 16 through next, 72 to a key point), side (slot 4: 1 against the profile plane's normal, 2 along it,
//!   3 symmetric, 6 both ways), depth (slot 6: a value object or a dimension relation);
//! * revolved protrusions and cutouts: profiles, axis (slot 6: a line), angle (slot 7);
//! * each profile: its plane (an absolute frame in the plane object) and its 2D curves
//!   (lines, circles, arcs and B-splines, in metres in the plane's frame); an open profile of a
//!   cutout closed on the side it cuts;
//! * profiles copied by a smart pattern (the seed's curves on the plane the copy stores) or a
//!   mirror (the seed's profile reflected);
//! * sweeps of one cross section along one path, lofts through their cross sections;
//! * rounds of a constant radius and equal chamfers: the size and the edges, each picked on
//!   the curve the part stored for it in the feature's edge set (lines, circles, arcs,
//!   ellipses, B-splines).
//!
//! Anything else (thin walls, holes, other patterns...) is an `unsupported` feature: the
//! caller rebuilds the tree and keeps it only when it gives the part's own body.
//!
//! Input is untrusted: every count, list and chain is bounded, and an object that is not
//! shaped as expected gives an `unsupported` feature or an error, never a panic.

use crate::{Limits, open};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Metres to millimetres.
const MM: f64 = 1000.0;
/// Most objects read from one store.
const MAX_OBJECTS: usize = 4_000_000;
/// Most features of one part.
const MAX_FEATURES: usize = 2000;
/// Most slots of one function, and references in one slot list.
const MAX_SLOTS: u32 = 256;
const MAX_LIST: u32 = 100_000;
/// Most members of one profile.
const MAX_MEMBERS: u32 = 20_000;
/// Longest type signature and class name read.
const MAX_SIG: u32 = 4096;
const MAX_NAME: u32 = 128;
/// Largest object kind and serial taken for an object start.
const MAX_KIND: u16 = 4096;
const MAX_SERIAL: u32 = 1 << 28;
/// A through-all extent, in millimetres.
const THROUGH: f64 = 1e4;
/// Profile points closer than this (mm) are one point.
const POINT_EPS: f64 = 1e-6;

type V3 = [f64; 3];

fn u16_at(b: &[u8], k: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(k..k.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(b: &[u8], k: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(k..k.checked_add(4)?)?.try_into().ok()?))
}

fn f64_at(b: &[u8], k: usize) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(k..k.checked_add(8)?)?.try_into().ok()?))
}

fn v3_at(b: &[u8], k: usize) -> Option<V3> {
    Some([f64_at(b, k)?, f64_at(b, k + 8)?, f64_at(b, k + 16)?])
}

fn rnd(v: f64) -> f64 {
    libm::round(v * 1e9) / 1e9 + 0.0
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: V3) -> Option<V3> {
    let l = libm::sqrt(dot(a, a));
    (l.is_finite() && l > 1e-12).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

fn finite3(a: V3) -> bool {
    a.iter().all(|v| v.is_finite() && v.abs() < 1e6)
}

fn arr_mm(v: V3) -> Value {
    json!([rnd(v[0] * MM), rnd(v[1] * MM), rnd(v[2] * MM)])
}

fn arr(v: V3) -> Value {
    json!([rnd(v[0]), rnd(v[1]), rnd(v[2])])
}

/// Sets a key of a JSON object.
fn put(o: &mut Value, key: &str, v: Value) {
    if let Some(m) = o.as_object_mut() {
        m.insert(key.to_string(), v);
    }
}

/// An associative function: its class, its slots (each the serials it refers to) and where
/// its own data starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Func {
    pub(crate) class: String,
    pub(crate) slots: Vec<Vec<u32>>,
    pub(crate) tail: usize,
}

impl Func {
    fn slot(&self, i: usize) -> Option<u32> {
        self.slots.get(i).and_then(|s| s.first().copied()).filter(|&v| v != 0)
    }
    fn list(&self, i: usize) -> &[u32] {
        self.slots.get(i).map_or(&[], Vec::as_slice)
    }
}

fn is_sig(c: u8) -> bool {
    matches!(c, b'%' | b'<' | b'>' | b'#' | b'r' | b'n')
}

/// The first associative function of an object: `u32 n`, a type signature of `n` (or `n - 1`)
/// characters, `u32 m`, the class name (`m - 1` characters and a NUL), `u32` slot count, then
/// the slots: `0x10, flags, serial, class id(16)` or `0x80000010, count[, flags, count x
/// (serial, class id(16))]`.
pub(crate) fn parse_func(b: &[u8]) -> Option<Func> {
    let mut i = 4;
    while let Some(p) = b.get(i..).and_then(|t| t.iter().position(|&c| c == b'%')) {
        let at = i + p;
        i = at + 1;
        if let Some(f) = func_at(b, at) {
            return Some(f);
        }
    }
    None
}

/// The function whose type signature starts at `at`, if one does.
fn func_at(b: &[u8], at: usize) -> Option<Func> {
    let n = u32_at(b, at.checked_sub(4)?)?;
    if n == 0 || n > MAX_SIG {
        return None;
    }
    let run = b.get(at..)?.iter().take_while(|&&c| is_sig(c)).count();
    if run + 1 < n as usize || run > n as usize {
        return None;
    }
    let mut q = at + run;
    if b.get(q) == Some(&0) {
        q += 1;
    }
    let m = u32_at(b, q)?;
    if !(3..=MAX_NAME).contains(&m) {
        return None;
    }
    let m = m as usize;
    let name = b.get(q + 4..q + 3 + m)?;
    if b.get(q + 3 + m) != Some(&0)
        || !name.first().is_some_and(u8::is_ascii_alphabetic)
        || !name.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
    {
        return None;
    }
    let class = String::from_utf8_lossy(name).into_owned();
    let mut q = q + 4 + m;
    let count = u32_at(b, q)?;
    if count > MAX_SLOTS {
        return None;
    }
    q += 4;
    let mut slots = Vec::new();
    for _ in 0..count {
        match u32_at(b, q)? {
            0x10 => {
                slots.push(vec![u32_at(b, q + 8)?]);
                q += 28;
            }
            0x8000_0010 => {
                let k = u32_at(b, q + 4)?;
                q += 8;
                if k > MAX_LIST {
                    return None;
                }
                let mut v = Vec::new();
                if k > 0 {
                    q += 4;
                    for _ in 0..k {
                        v.push(u32_at(b, q)?);
                        q += 20;
                    }
                }
                slots.push(v);
            }
            _ => return None,
        }
    }
    Some(Func { class, slots, tail: q })
}

/// The object store: each object's kind and bytes by serial, and the class names by kind.
pub(crate) struct Store {
    data: Vec<u8>,
    objs: BTreeMap<u32, (u16, usize, usize)>,
    kinds: HashMap<u16, String>,
    /// The sketches' curves (`SKETCH.P_B`) by the serial of the profile member they stand for.
    pub(crate) sketch: HashMap<u32, Edge3>,
}

impl Store {
    /// Segments `PSMcluster0` into objects (`QQ`, kind, serial; serials rise).
    pub(crate) fn new(data: Vec<u8>, typetable: &[u8]) -> Result<Store, String> {
        // Every `QQ` with a plausible kind is a candidate; the objects are the longest run of
        // candidates whose serials rise (a `QQ` inside a payload breaks the rise).
        let mut cand: Vec<(u32, u16, usize)> = Vec::new();
        let mut i = 0;
        while let Some(p) = data.get(i..).and_then(|t| t.windows(2).position(|w| w == b"QQ")) {
            let at = i + p;
            i = at + 2;
            let (Some(kind), Some(serial)) = (u16_at(&data, at + 2), u32_at(&data, at + 4)) else { break };
            if kind < MAX_KIND && serial > 0 && serial < MAX_SERIAL {
                if cand.len() >= 2 * MAX_OBJECTS {
                    return Err("the part has too many objects".into());
                }
                cand.push((serial, kind, at));
            }
        }
        let starts = rising(&cand);
        if starts.is_empty() {
            return Err("the part's object store is empty".into());
        }
        let mut objs = BTreeMap::new();
        for (k, &(serial, kind, at)) in starts.iter().enumerate() {
            let end = starts.get(k + 1).map_or(data.len(), |n| n.2);
            objs.insert(serial, (kind, at, end));
        }
        Ok(Store { data, objs, kinds: type_names(typetable), sketch: HashMap::new() })
    }

    pub(crate) fn obj(&self, serial: u32) -> Option<&[u8]> {
        let &(_, a, e) = self.objs.get(&serial)?;
        self.data.get(a..e)
    }

    fn kind_name(&self, serial: u32) -> &str {
        self.objs.get(&serial).and_then(|o| self.kinds.get(&o.0)).map_or("", String::as_str)
    }

    fn func(&self, serial: u32) -> Option<Func> {
        parse_func(self.obj(serial)?)
    }
}

/// The longest subsequence of `cand` (in store order) whose serials rise strictly.
fn rising(cand: &[(u32, u16, usize)]) -> Vec<(u32, u16, usize)> {
    // tails[k]: index of the smallest last serial of a rising run of length k + 1.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; cand.len()];
    for (i, c) in cand.iter().enumerate() {
        let k = tails.partition_point(|&t| cand.get(t).is_some_and(|x| x.0 < c.0));
        if let Some(p) = prev.get_mut(i) {
            *p = k.checked_sub(1).and_then(|j| tails.get(j).copied());
        }
        if let Some(t) = tails.get_mut(k) {
            *t = i;
        } else {
            tails.push(i);
        }
    }
    let mut out = Vec::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        if let Some(c) = cand.get(i) {
            out.push(*c);
        }
        at = prev.get(i).copied().flatten();
    }
    out.reverse();
    out
}

/// `PSMtypetable`: records `class id(16), u32 index, 01 00 ff ff, u32 count, u16 n, name(n)`.
fn type_names(t: &[u8]) -> HashMap<u16, String> {
    let mut out = HashMap::new();
    let mut i = 4;
    while let Some(p) = t.get(i..).and_then(|s| s.windows(4).position(|w| w == [1, 0, 0xff, 0xff])) {
        let at = i + p;
        i = at + 4;
        let (Some(idx), Some(n)) = (u32_at(t, at - 4), u16_at(t, at + 8)) else { continue };
        let Some(name) = t.get(at + 10..at + 10 + usize::from(n).saturating_sub(1)) else { continue };
        if let Ok(idx) = u16::try_from(idx) {
            out.insert(idx, String::from_utf8_lossy(name).into_owned());
        }
        if out.len() > 100_000 {
            break;
        }
    }
    out
}

/// A sketch curve of `SKETCH.P_B` (metres, absolute).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Edge3 {
    Line(V3, V3),
    /// Centre, normal, radius and, for an arc, its start and end, counter-clockwise about the
    /// normal from start to end.
    Circle(V3, V3, f64, Option<(V3, V3)>),
}

impl Edge3 {
    /// The curve in a plane's frame, if it lies in the plane.
    pub(crate) fn flat(&self, o: V3, n: V3, x: V3) -> Option<Curve2> {
        let y = cross(n, x);
        let on = |p: V3| {
            let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            (dot(d, n).abs() < 1e-7).then_some([dot(d, x), dot(d, y)])
        };
        match *self {
            Edge3::Line(a, b) => Some(Curve2::Line(on(a)?, on(b)?)),
            Edge3::Circle(c, cn, r, ends) => {
                let c2 = on(c)?;
                let k = dot(cn, n);
                if (k.abs() - 1.0).abs() > 1e-7 {
                    return None;
                }
                let Some((a, b)) = ends else { return Some(Curve2::Circle(c2, r)) };
                let (a, b) = (on(a)?, on(b)?);
                let ang = |p: [f64; 2]| libm::atan2(p[1] - c2[1], p[0] - c2[0]);
                Some(if k > 0.0 { Curve2::Arc(c2, r, ang(a), ang(b)) } else { Curve2::Arc(c2, r, ang(b), ang(a)) })
            }
        }
    }
}

/// The curves of `SKETCH.P_B` that stand for profile members: edges with the attribute
/// `SolidEdge/ProfileEntityInfo` (integers `[1, 0, member, 0, profile]`).
pub(crate) fn sketch_edges(pb: &[u8]) -> HashMap<u32, Edge3> {
    let mut out = HashMap::new();
    let Ok(g) = gusset_xt::decode(pb, &gusset_xt::Limits::default()) else { return out };
    let pt = |v: u32| g.vec3(g.ptr(v, "point"), "pvec");
    for &idx in g.nodes.keys() {
        if g.type_name(idx) != Some("ATTRIBUTE") {
            continue;
        }
        let def = g.ptr(idx, "definition");
        if g.string(g.ptr(def, "identifier"), "string") != "SolidEdge/ProfileEntityInfo" {
            continue;
        }
        let ints: Vec<i64> = g
            .list(idx, "fields")
            .iter()
            .filter_map(|v| if let gusset_xt::Val::Ptr(p) = v { Some(*p) } else { None })
            .flat_map(|p| g.list(p, "values").iter())
            .filter_map(|v| if let gusset_xt::Val::Int(i) = v { Some(*i) } else { None })
            .collect();
        let Some(m) = ints.get(2).and_then(|&m| u32::try_from(m).ok()) else { continue };
        let e = g.ptr(idx, "owner");
        if g.type_name(e) != Some("EDGE") {
            continue;
        }
        // The head fin's vertex ends the edge, the other fin's starts it (as in a body).
        let head = g.ptr(e, "fin");
        let end = pt(g.ptr(head, "vertex"));
        let start = pt(g.ptr(g.ptr(head, "other"), "vertex"));
        let cu = g.ptr(e, "curve");
        let forward = g.ch(cu, "sense") != Some(b'-');
        let edge = match g.type_name(cu) {
            Some("LINE") => match (start, end) {
                (Some(a), Some(b)) => Edge3::Line(a, b),
                _ => continue,
            },
            Some("CIRCLE") => {
                let (Some(c), Some(n), Some(r)) = (g.vec3(cu, "centre"), g.vec3(cu, "normal"), g.real(cu, "radius"))
                else {
                    continue;
                };
                let Some(n) = unit(n) else { continue };
                if !(r.is_finite() && r > 0.0 && finite3(c)) {
                    continue;
                }
                let ends = match (start, end) {
                    (Some(a), Some(b)) if forward => Some((a, b)),
                    (Some(a), Some(b)) => Some((b, a)),
                    _ => None,
                };
                Edge3::Circle(c, n, r, ends)
            }
            _ => continue,
        };
        out.insert(m, edge);
        if out.len() > 1_000_000 {
            break;
        }
    }
    out
}

/// A plane object: `u32, u8 14, origin, normal, x direction` (metres, absolute).
pub(crate) fn plane(b: &[u8]) -> Option<(V3, V3, V3)> {
    if b.get(12) != Some(&0x0e) {
        return None;
    }
    let (o, n, x) = (v3_at(b, 13)?, v3_at(b, 37)?, v3_at(b, 61)?);
    if !(finite3(o) && finite3(n) && finite3(x)) {
        return None;
    }
    let n = unit(n)?;
    let x = unit(x)?;
    (dot(n, x).abs() < 1e-6).then_some((o, n, x))
}

/// A line object (a revolve axis): `u32, u8 3, point, direction` (metres, absolute).
pub(crate) fn axis_line(b: &[u8]) -> Option<(V3, V3)> {
    if b.get(12) != Some(&0x03) {
        return None;
    }
    let (p, d) = (v3_at(b, 13)?, v3_at(b, 37)?);
    (finite3(p) && finite3(d)).then_some(())?;
    Some((p, unit(d)?))
}

/// The value of a scalar: a small value object (`f64` at +8) or a dimension relation, where
/// `u32 1, u32 k` near +0x88 is followed later by `k` again and the value.
pub(crate) fn value(b: &[u8]) -> Option<f64> {
    let v = if b.len() <= 64 {
        f64_at(b, 8)?
    } else {
        let start =
            (0x80..0xa0).step_by(4).find(|&o| u32_at(b, o) == Some(1) && u32_at(b, o + 4).is_some_and(|k| k > 0))?;
        let k = u32_at(b, start + 4)?;
        let at = (start + 8..start + 0x60).find(|&o| u32_at(b, o) == Some(k))?;
        f64_at(b, at + 4)?
    };
    (v.is_finite() && v.abs() < 1e6).then_some(v)
}

/// A key point extent (`JMLinearKeyPointFunction`): after its slots, `u32 1`, the reach (metres)
/// and its unit direction.
pub(crate) fn key_point_reach(b: &[u8]) -> Option<(f64, V3)> {
    let f = parse_func(b)?;
    if f.class != "JMLinearKeyPointFunction" || u32_at(b, f.tail)? != 1 {
        return None;
    }
    let d = f64_at(b, f.tail + 4)?;
    let v = v3_at(b, f.tail + 12)?;
    (d.is_finite() && d.abs() < 1e6 && finite3(v)).then_some(())?;
    Some((d, unit(v)?))
}

/// A curve of a round's or chamfer's edge set, as the part stored it when the feature was
/// made (metres, absolute).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SetCurve {
    /// Point, unit direction and the parameter range (the edge runs from `p + t0 d` to `p + t1 d`).
    Line(V3, V3, f64, f64),
    /// Centre, unit normal, major axis (its length is the radius), minor to major ratio and,
    /// for an arc, its angle range, counter-clockwise about the normal from the major axis.
    Conic(V3, V3, V3, f64, Option<(f64, f64)>),
    /// A polynomial B-spline: order, poles, knots (each repeated by its multiplicity).
    Spline(usize, Vec<V3>, Vec<f64>),
}

/// The class id that opens an edge set's curve list ends in these bytes.
const SET_LIST: [u8; 10] = [0xcd, 0x11, 0x8b, 0xa4, 0x08, 0x00, 0x36, 0x24, 0xfc, 0x01];
/// The class ids of the stored curves end in these bytes.
const SET_CURVE: [u8; 11] = [0x3a, 0xce, 0x11, 0x9a, 0x5e, 0x08, 0x00, 0x36, 0x9b, 0x41, 0x02];

/// The edges a round or chamfer works on, from the edge set it refers to (a
/// `JMTransmigrateAssocFunction` naming them by `SEGetRoundableEdgeSeq`): after the names,
/// `ff`, a class id ending in [`SET_LIST`], `u32 1, u32 n, u32 0, u8 2`, then `n` curves, each a
/// class id ending in [`SET_CURVE`], a type byte and its numbers (`f64`), then a few words:
/// type 3 a line (point, direction, t0, t1), 15 a circle or ellipse (centre, normal, major
/// axis, ratio), 16 an arc of one (the same and the angles from, to), 27 a B-spline
/// (`u32 n`, `n` poles, `u32 w` weights (0 here), `u32 k`, `k` knots, `f64`, `u8 0`, `u8 order`
/// and 3 bytes).
pub(crate) fn edge_set(b: &[u8]) -> Result<Vec<SetCurve>, String> {
    let at = b
        .windows(17)
        .position(|w| w.first() == Some(&0xff) && w.get(7..) == Some(&SET_LIST[..]))
        .ok_or("the edge set lists no curves")?;
    let mut q = at + 17;
    let n = u32_at(b, q + 4).ok_or("the edge set is cut short")?;
    if u32_at(b, q) != Some(1) || u32_at(b, q + 8) != Some(0) || b.get(q + 12) != Some(&2) {
        return Err("the edge set's curve list is not readable".into());
    }
    if n == 0 || n > MAX_MEMBERS {
        return Err("the edge set has no curves or too many".into());
    }
    q += 13;
    let mut out = Vec::new();
    for k in 0..n {
        if b.get(q + 5..q + 16) != Some(&SET_CURVE[..]) {
            return Err("an edge of the set is not readable".into());
        }
        let t = *b.get(q + 16).ok_or("the edge set is cut short")?;
        let (c, end) = set_curve(b, t, q + 17)?;
        out.push(c);
        q = end;
        // A few words follow each curve (zero, or a count and a zero after a spline): the next
        // curve is the next class id within reach.
        if k + 1 < n {
            q = (q..=q + 64)
                .find(|&o| b.get(o + 5..o + 16) == Some(&SET_CURVE[..]))
                .ok_or("an edge of the set is not readable")?;
        }
    }
    Ok(out)
}

/// One stored curve of type `t` whose numbers start at `at` (see [`edge_set`]) and where it
/// ends.
fn set_curve(b: &[u8], t: u8, at: usize) -> Result<(SetCurve, usize), String> {
    let f = |i: usize| f64_at(b, at + 8 * i).filter(|v| v.is_finite() && v.abs() < 1e6);
    let v3 = |i: usize| Some([f(i)?, f(i + 1)?, f(i + 2)?]);
    let (c, len) = match t {
        27 => return set_spline(b, at),
        3 => {
            let (p, d, t0, t1) = (v3(0), v3(3).and_then(unit), f(6), f(7));
            let (Some(p), Some(d), Some(t0), Some(t1)) = (p, d, t0, t1) else {
                return Err("an edge line of the set is not readable".into());
            };
            (SetCurve::Line(p, d, t0, t1), 8)
        }
        15 | 16 => {
            let (Some(c), Some(nrm), Some(m), Some(r)) = (v3(0), v3(3).and_then(unit), v3(6), f(9)) else {
                return Err("an edge circle of the set is not readable".into());
            };
            let range = if t == 16 {
                let (Some(a0), Some(a1)) = (f(10), f(11)) else {
                    return Err("an edge arc of the set is not readable".into());
                };
                Some((a0, a1))
            } else {
                None
            };
            (SetCurve::Conic(c, nrm, m, r, range), if t == 16 { 12 } else { 10 })
        }
        _ => return Err(format!("an edge of the set is a curve of type {t} (not read)")),
    };
    Ok((c, at + 8 * len))
}

/// A B-spline of an edge set from `at` (see [`edge_set`]) and where it ends.
fn set_spline(b: &[u8], at: usize) -> Result<(SetCurve, usize), String> {
    let bad = || "an edge spline of the set is not readable".to_string();
    let n = u32_at(b, at).ok_or_else(bad)? as usize;
    if !(2..=MAX_POLES as usize).contains(&n) {
        return Err(bad());
    }
    let mut q = at + 4;
    let mut poles = Vec::with_capacity(n);
    for _ in 0..n {
        let p = v3_at(b, q).filter(|p| finite3(*p) && p.iter().all(|v| v.abs() < 1e6)).ok_or_else(bad)?;
        poles.push(p);
        q += 24;
    }
    if u32_at(b, q).ok_or_else(bad)? != 0 {
        return Err("an edge of the set is a rational spline (not read)".into());
    }
    let k = u32_at(b, q + 4).ok_or_else(bad)? as usize;
    q += 8;
    if k > n + 26 {
        return Err(bad());
    }
    let mut knots = Vec::with_capacity(k);
    for _ in 0..k {
        let t = f64_at(b, q).filter(|t| t.is_finite()).ok_or_else(bad)?;
        if knots.last().is_some_and(|&l| t < l) {
            return Err(bad());
        }
        knots.push(t);
        q += 8;
    }
    let order = usize::from(*b.get(q + 9).ok_or_else(bad)?);
    if b.get(q + 8) != Some(&0) || !(2..=26).contains(&order) || k != n + order {
        return Err(bad());
    }
    Ok((SetCurve::Spline(order, poles, knots), q + 13))
}

/// A plane face of a face set (`JMGetSmoothFaceSeq` names, then the faces as they were when
/// the feature was made): its plane (origin, unit normal, x direction; metres, absolute) and
/// its boundary loops.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SetFace {
    pub(crate) origin: V3,
    pub(crate) normal: V3,
    pub(crate) x: V3,
    pub(crate) loops: Vec<Vec<SetCurve>>,
}

/// Most boundary loops of one face.
const MAX_LOOPS: u32 = 10_000;

/// The faces of a face set: laid out as an edge set's list ([`edge_set`]), each entry a class
/// id ending in [`SET_CURVE`], type 14 (a plane: origin, normal, x direction), `u32` loop
/// count and per loop `u32 m, u8 2, u8 0` and its `m` curves (a type byte and its numbers,
/// as an edge set's), then a few words to the next face.
pub(crate) fn face_set(b: &[u8]) -> Result<Vec<SetFace>, String> {
    let at = b
        .windows(17)
        .position(|w| w.first() == Some(&0xff) && w.get(7..) == Some(&SET_LIST[..]))
        .ok_or("the face set lists no faces")?;
    let mut q = at + 17;
    let n = u32_at(b, q + 4).ok_or("the face set is cut short")?;
    if u32_at(b, q) != Some(1) || u32_at(b, q + 8) != Some(0) || b.get(q + 12) != Some(&2) {
        return Err("the face set's list is not readable".into());
    }
    if n == 0 || n > MAX_MEMBERS {
        return Err("the face set has no faces or too many".into());
    }
    q += 13;
    let mut out = Vec::new();
    for k in 0..n {
        if b.get(q + 5..q + 16) != Some(&SET_CURVE[..]) {
            return Err("a face of the set is not readable".into());
        }
        match b.get(q + 16) {
            Some(14) => {}
            Some(t) => return Err(format!("a face of the set lies on a surface of type {t} (not read)")),
            None => return Err("the face set is cut short".into()),
        }
        let at = q + 17;
        let v3 = |i: usize| v3_at(b, at + 8 * i).filter(|v| finite3(*v) && v.iter().all(|c| c.abs() < 1e6));
        let (Some(origin), Some(normal), Some(x)) = (v3(0), v3(3).and_then(unit), v3(6).and_then(unit)) else {
            return Err("a face plane of the set is not readable".into());
        };
        let nl = u32_at(b, at + 72).ok_or("the face set is cut short")?;
        if nl == 0 || nl > MAX_LOOPS {
            return Err("a face of the set has no boundary or too many loops".into());
        }
        q = at + 76;
        let mut loops = Vec::new();
        for _ in 0..nl {
            let m = u32_at(b, q).ok_or("the face set is cut short")?;
            if m == 0 || m > MAX_MEMBERS || b.get(q + 4..q + 6) != Some(&[2, 0][..]) {
                return Err("a face boundary of the set is not readable".into());
            }
            q += 6;
            let mut curves = Vec::new();
            for _ in 0..m {
                let t = *b.get(q).ok_or("the face set is cut short")?;
                let (c, end) = set_curve(b, t, q + 1)?;
                curves.push(c);
                q = end;
            }
            loops.push(curves);
        }
        out.push(SetFace { origin, normal, x, loops });
        if k + 1 < n {
            q = (q..=q + 64)
                .find(|&o| b.get(o + 5..o + 16) == Some(&SET_CURVE[..]))
                .ok_or("a face of the set is not readable")?;
        }
    }
    Ok(out)
}

/// Points along a stored curve (metres): a line's ends, 64 steps along anything else.
fn curve_points(c: &SetCurve) -> Result<Vec<V3>, String> {
    const STEPS: usize = 64;
    let at = |p: V3, d: V3, s: f64| [p[0] + d[0] * s, p[1] + d[1] * s, p[2] + d[2] * s];
    let steps = |lo: f64, hi: f64, p: &dyn Fn(f64) -> Option<V3>| -> Result<Vec<V3>, String> {
        (0..=STEPS)
            .map(|i| {
                p(lo + (hi - lo) * i as f64 / STEPS as f64).ok_or_else(|| "a boundary curve is not readable".into())
            })
            .collect()
    };
    match c {
        SetCurve::Line(p, d, t0, t1) => Ok(vec![at(*p, *d, *t0), at(*p, *d, *t1)]),
        SetCurve::Conic(cn, n, major, ratio, range) => {
            let u = unit(*major).ok_or("a boundary circle has no radius")?;
            let (r, v) = (dot(*major, *major).sqrt(), cross(*n, u));
            let (lo, hi) = turn(*range);
            let (cn, ratio) = (*cn, *ratio);
            steps(lo, hi, &|t: f64| {
                let (cs, sn) = (r * libm::cos(t), r * ratio * libm::sin(t));
                Some([cn[0] + cs * u[0] + sn * v[0], cn[1] + cs * u[1] + sn * v[1], cn[2] + cs * u[2] + sn * v[2]])
            })
        }
        SetCurve::Spline(order, poles, knots) => {
            let (lo, hi) = (
                *knots.get(order - 1).ok_or("a spline is empty")?,
                *knots.get(poles.len()).ok_or("a spline is empty")?,
            );
            steps(lo, hi, &|t| spline_at(*order, poles, knots, t))
        }
    }
}

/// Points well inside a stored plane face (metres): on scan lines across it, the middle of
/// each stretch inside its boundary (even-odd over all its loops).
pub(crate) fn face_inside_points(f: &SetFace) -> Result<Vec<V3>, String> {
    const LINES: usize = 8;
    let y = cross(f.normal, f.x);
    let mut segs: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for c in f.loops.iter().flatten() {
        let pts: Vec<[f64; 2]> = curve_points(c)?
            .iter()
            .map(|p| {
                let d = [p[0] - f.origin[0], p[1] - f.origin[1], p[2] - f.origin[2]];
                [dot(d, f.x), dot(d, y)]
            })
            .collect();
        segs.extend(pts.windows(2).filter_map(|w| if let [a, b] = w { Some((*a, *b)) } else { None }));
    }
    let (lo, hi) = segs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (a, b)| (lo.min(a[1]).min(b[1]), hi.max(a[1]).max(b[1])));
    if !(hi > lo && (hi - lo).is_finite()) {
        return Err("a face of the set has no area".into());
    }
    let mut out = Vec::new();
    for k in 0..LINES {
        // Off the simple fractions, so that a scan line rarely runs through a corner.
        let v = lo + (hi - lo) * (k as f64 + 0.5 + 0.0123) / LINES as f64;
        let mut us: Vec<f64> = segs
            .iter()
            .filter(|(a, b)| (a[1] <= v) != (b[1] <= v))
            .map(|(a, b)| a[0] + (b[0] - a[0]) * (v - a[1]) / (b[1] - a[1]))
            .collect();
        us.sort_by(f64::total_cmp);
        for w in us.chunks_exact(2) {
            if let [u0, u1] = w
                && u1 - u0 > 1e-6
            {
                let u = (u0 + u1) / 2.0;
                out.push([
                    f.origin[0] + u * f.x[0] + v * y[0],
                    f.origin[1] + u * f.x[1] + v * y[1],
                    f.origin[2] + u * f.x[2] + v * y[2],
                ]);
            }
        }
    }
    if out.is_empty() {
        return Err("a face of the set has no inside".into());
    }
    Ok(out)
}

/// The tree's pick of a stored plane face: points inside it and its plane (mm).
pub(crate) fn face_pick(f: &SetFace) -> Result<Value, String> {
    let mm = |p: V3| json!([rnd(p[0] * MM), rnd(p[1] * MM), rnd(p[2] * MM)]);
    let points: Vec<Value> = face_inside_points(f)?.into_iter().map(mm).collect();
    let first = points.first().cloned().ok_or("a face of the set has no inside")?;
    Ok(json!({"point": first, "points": points,
              "surface": {"type": "plane", "origin": mm(f.origin), "normal": arr(f.normal)}}))
}

/// A point of a B-spline (order, poles, flat knots) at `t` (de Boor).
fn spline_at(order: usize, poles: &[V3], knots: &[f64], t: f64) -> Option<V3> {
    let n = poles.len();
    let (lo, hi) = (*knots.get(order - 1)?, *knots.get(n)?);
    let t = t.clamp(lo, hi);
    // The span: knots[s] <= t < knots[s + 1], order - 1 <= s < n.
    let s = (order - 1..n).rev().find(|&i| knots.get(i).is_some_and(|&k| k <= t))?;
    let mut d: Vec<V3> = poles.get(s + 1 - order..=s)?.to_vec();
    for r in 1..order {
        for j in (r..order).rev() {
            let i = s + 1 - order + j;
            let (a, b) = (*knots.get(i)?, *knots.get(i + order - r)?);
            let w = if b > a { (t - a) / (b - a) } else { 0.0 };
            let (p, q) = (*d.get(j - 1)?, *d.get(j)?);
            *d.get_mut(j)? = [p[0] + w * (q[0] - p[0]), p[1] + w * (q[1] - p[1]), p[2] + w * (q[2] - p[2])];
        }
    }
    d.last().copied()
}

/// An arc's angles from and to, the second past the first; a whole turn without them.
fn turn(range: Option<(f64, f64)>) -> (f64, f64) {
    match range {
        None => (0.0, 2.0 * core::f64::consts::PI),
        Some((a0, a1)) => (a0, if a1 <= a0 { a1 + 2.0 * core::f64::consts::PI } else { a1 }),
    }
}

/// The pick of an edge along a curve `p(t)`, `t` from `lo` to `hi`: the point at the middle
/// and the whole edge as a polyline within 2 um of it (mm), the importer matching the model's
/// edges that lie along it.
fn along(p: &dyn Fn(f64) -> Option<V3>, lo: f64, hi: f64) -> Result<Value, String> {
    if !(hi > lo && (hi - lo).is_finite()) {
        return Err("an edge of the set is empty".into());
    }
    let mm = |p: V3| json!([rnd(p[0] * MM), rnd(p[1] * MM), rnd(p[2] * MM)]);
    let ev = |t: f64| p(t).ok_or("an edge of the set is not readable");
    let mut n = 16;
    let pts = loop {
        let pts: Vec<V3> = (0..=n).map(|i| ev(lo + (hi - lo) * i as f64 / n as f64)).collect::<Result<_, _>>()?;
        let mut worst = 0.0f64;
        for (i, w) in pts.windows(2).enumerate() {
            let m = ev(lo + (hi - lo) * (i as f64 + 0.5) / n as f64)?;
            if let [a, b] = w {
                let c = [(a[0] + b[0]) / 2.0 - m[0], (a[1] + b[1]) / 2.0 - m[1], (a[2] + b[2]) / 2.0 - m[2]];
                worst = worst.max(dot(c, c).sqrt());
            }
        }
        if worst * MM < 0.002 || n >= 256 {
            break pts;
        }
        n *= 2;
    };
    let path: Vec<Value> = pts.iter().map(|p| mm(*p)).collect();
    Ok(json!({"midpoint": mm(ev((lo + hi) / 2.0)?), "path": path}))
}

/// The tree's pick of a stored edge: its middle and a short piece of it around the middle
/// (mm), so the importer finds the model's edge that holds that piece whatever its extent.
pub(crate) fn edge_pick(c: &SetCurve) -> Result<Value, String> {
    // Half the length of the piece (mm).
    const PIECE: f64 = 0.1;
    let at = |p: V3, d: V3, s: f64| [p[0] + d[0] * s, p[1] + d[1] * s, p[2] + d[2] * s];
    let mm = |p: V3| json!([rnd(p[0] * MM), rnd(p[1] * MM), rnd(p[2] * MM)]);
    if let SetCurve::Spline(order, poles, knots) = c {
        let (lo, hi) = (
            *knots.get(order - 1).ok_or("an edge spline is empty")?,
            *knots.get(poles.len()).ok_or("an edge spline is empty")?,
        );
        return along(&|t| spline_at(*order, poles, knots, t), lo, hi);
    }
    // An ellipse (or an arc of one): along it as well.
    if let SetCurve::Conic(cn, n, major, ratio, range) = *c
        && (ratio - 1.0).abs() > 1e-9
    {
        let u = unit(major).ok_or("an edge ellipse of the set has no axis")?;
        let (r, v) = (dot(major, major).sqrt(), cross(n, u));
        let (lo, hi) = turn(range);
        if !(ratio > 1e-6 && ratio < 1e6) {
            return Err("an edge ellipse of the set is flat".into());
        }
        let p = |t: f64| {
            let (cs, sn) = (r * libm::cos(t), r * ratio * libm::sin(t));
            Some([cn[0] + cs * u[0] + sn * v[0], cn[1] + cs * u[1] + sn * v[1], cn[2] + cs * u[2] + sn * v[2]])
        };
        return along(&p, lo, hi);
    }
    let (mid, a, b) = match *c {
        SetCurve::Line(p, d, t0, t1) => {
            let len = (t1 - t0) * MM;
            if !(len.is_finite() && len > 1e-6) {
                return Err("an edge of the set has no length".into());
            }
            let m = at(p, d, (t0 + t1) / 2.0);
            let h = PIECE.min(len / 4.0) / MM;
            (m, at(m, d, -h), at(m, d, h))
        }
        SetCurve::Conic(cn, n, major, _, range) => {
            let r = dot(major, major).sqrt();
            let u = unit(major).ok_or("an edge circle of the set has no radius")?;
            let v = cross(n, u);
            // Any point of a whole circle will do.
            let (a0, a1) = turn(range);
            let sweep = a1 - a0;
            if !(sweep > 1e-9 && sweep <= 2.0 * core::f64::consts::PI + 1e-9 && r > 1e-12) {
                return Err("an edge arc of the set is empty".into());
            }
            let p = |t: f64| {
                let (cs, sn) = (libm::cos(t), libm::sin(t));
                [
                    cn[0] + r * (cs * u[0] + sn * v[0]),
                    cn[1] + r * (cs * u[1] + sn * v[1]),
                    cn[2] + r * (cs * u[2] + sn * v[2]),
                ]
            };
            let m = (a0 + a1) / 2.0;
            let h = (PIECE / MM / r).min(sweep / 4.0);
            (p(m), p(m - h), p(m + h))
        }
        SetCurve::Spline(..) => return Err("an edge spline of the set".into()),
    };
    Ok(json!({"midpoint": mm(mid), "path": [mm(a), mm(b)]}))
}

/// A 2D curve of a profile (metres, in the profile plane's frame).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Curve2 {
    Line([f64; 2], [f64; 2]),
    Circle([f64; 2], f64),
    /// Centre, radius, start and end angles, counter-clockwise from start to end.
    Arc([f64; 2], f64, f64, f64),
    /// A polynomial B-spline: degree, poles, knots (each repeated by its multiplicity).
    Spline(u32, Vec<[f64; 2]>, Vec<f64>),
}

/// Most poles of one spline.
const MAX_POLES: u32 = 10_000;

/// A B-spline member: `u16 flags, u32 kind, u32 flags, u32 n, u32 0, n poles (x, y), u8 order,
/// u32 k, k multiplicities, k knots`, then `u32 0` and more. Kind 1 is drawn by its poles,
/// kind 2 through points: then the rest holds the points' parameters (`u8 1, u8 1, u32 m, m
/// f64`) and their order; the curve is the same poles and knots (no weights).
pub(crate) fn spline(b: &[u8]) -> Option<Curve2> {
    if !matches!(u32_at(b, 10)?, 1 | 2) || !matches!(u32_at(b, 14)?, 1 | 0x8000_0001) {
        return None;
    }
    let n = u32_at(b, 18)?;
    if !(2..=MAX_POLES).contains(&n) {
        return None;
    }
    let mut q = 26;
    let mut poles = Vec::new();
    for _ in 0..n {
        let p = [f64_at(b, q)?, f64_at(b, q + 8)?];
        if !p.iter().all(|v| v.is_finite() && v.abs() < 1e6) {
            return None;
        }
        poles.push(p);
        q += 16;
    }
    let order = u32::from(*b.get(q)?);
    let k = u32_at(b, q + 1)?;
    q += 5;
    if !(2..=26).contains(&order) || !(2..=MAX_POLES + 26).contains(&k) {
        return None;
    }
    let mut mults = Vec::new();
    for _ in 0..k {
        mults.push(u32_at(b, q)?);
        q += 4;
    }
    let mut knots = Vec::new();
    for &m in &mults {
        let t = f64_at(b, q)?;
        q += 8;
        if !t.is_finite() || m == 0 || m > order || knots.last().is_some_and(|&l| t < l) {
            return None;
        }
        knots.extend(std::iter::repeat_n(t, m as usize));
    }
    if knots.len() != (n + order) as usize || u32_at(b, q)? != 0 || b.len() < q + 20 {
        return None;
    }
    Some(Curve2::Spline(order - 1, poles, knots))
}

/// A profile member: a line (`u16 0, x1, y1, x2, y2`), a circle (`u16 0, cx, cy, r, 0.0,
/// 1.0, u8`) or an arc (`u16 0, cx, cy, r, u8 ccw, a0, a1`); `None` for anything else.
pub(crate) fn curve(b: &[u8]) -> Option<Curve2> {
    // u8 0, u8 flags (0 or 8 seen on curves).
    if *b.get(8)? != 0 || !matches!(*b.get(9)?, 0 | 8) {
        return None;
    }
    let ok = |v: f64| v.is_finite() && v.abs() < 1e6;
    match b.len() {
        46 => {
            let (x1, y1, x2, y2) = (f64_at(b, 10)?, f64_at(b, 18)?, f64_at(b, 26)?, f64_at(b, 34)?);
            [x1, y1, x2, y2].into_iter().all(ok).then_some(Curve2::Line([x1, y1], [x2, y2]))
        }
        55 => {
            let (cx, cy, r) = (f64_at(b, 10)?, f64_at(b, 18)?, f64_at(b, 26)?);
            if !([cx, cy].into_iter().all(ok) && r.is_finite() && r > 0.0 && r < 1e6) {
                return None;
            }
            if f64_at(b, 34)?.to_bits() == 0 && f64_at(b, 42)?.to_bits() == 1f64.to_bits() {
                return Some(Curve2::Circle([cx, cy], r));
            }
            let ccw = *b.get(34)?;
            let (a0, a1) = (f64_at(b, 35)?, f64_at(b, 43)?);
            if ccw > 1 || !(a0.is_finite() && a1.is_finite() && a0.abs() < 20.0 && a1.abs() < 20.0) {
                return None;
            }
            Some(if ccw == 1 { Curve2::Arc([cx, cy], r, a0, a1) } else { Curve2::Arc([cx, cy], r, a1, a0) })
        }
        _ => spline(b),
    }
}

/// A profile member that is not a curve (a constraint, key point, dimension, group...), by
/// its class name: the store's kinds of curves do not name curve classes, those of these do.
fn not_a_curve(class: &str) -> bool {
    let c = class.strip_prefix("CLSID_").unwrap_or(class);
    if c.contains("Dim") || c.contains("KeyPoint") || c.contains("PointOn") {
        return true;
    }
    c.ends_with("2d")
        && !["JLine2d", "JArc2d", "JCircle2d", "JEllipArc2d", "JEllipse2d", "JBspCurve2d", "JPoint2d"].contains(&c)
}

/// A profile: its plane and curves.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Profile {
    pub(crate) origin: V3,
    pub(crate) normal: V3,
    pub(crate) x: V3,
    pub(crate) curves: Vec<Curve2>,
    /// Turned over (a mirror's copy): the curves run the other way round.
    pub(crate) turned: bool,
    /// For each curve, whether it was drawn from its end to its start.
    pub(crate) backwards: Vec<bool>,
}

impl Profile {
    fn y(&self) -> V3 {
        cross(self.normal, self.x)
    }
    fn same_plane(&self, o: &Profile) -> bool {
        dot(self.normal, o.normal) > 1.0 - 1e-9
            && dot(self.x, o.x) > 1.0 - 1e-9
            && self.origin.iter().zip(o.origin).all(|(a, b)| (a - b).abs() < 1e-9)
    }
}

/// Reads a feature's profile (a `JProfileRecomputeEx` attribute set): its plane (slot 0) and
/// the curves of its `Profile2d`.
fn profile(st: &Store, serial: u32) -> Result<Profile, String> {
    let f = st.func(serial).ok_or("the profile is not readable")?;
    if f.class == "JMDummyComputeFunction" {
        return copied_profile(st, serial, &f);
    }
    if f.class == "JMTransmigrateAssocFunction" {
        return edge_profile(st.obj(serial).ok_or("the profile is missing")?);
    }
    if f.class != "JProfileRecomputeEx" {
        return Err(format!("the profile is a {} (not a sketch profile)", label(&f.class)));
    }
    let pl = f.slot(0).and_then(|p| st.obj(p)).and_then(plane).ok_or("the profile's plane is not readable")?;
    let b = st.obj(serial).ok_or("the profile is missing")?;
    let tag: Vec<u8> = "Profile2d".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let at = b.windows(tag.len()).position(|w| w == tag.as_slice()).ok_or("the profile has no 2D profile")?;
    let p2 = at.checked_sub(5).and_then(|k| u32_at(b, k)).ok_or("the 2D profile is not readable")?;
    let pb = st.obj(p2).ok_or("the 2D profile is missing")?;
    let n = u32_at(pb, 8).ok_or("the 2D profile is cut short")?;
    if n > MAX_MEMBERS {
        return Err("the profile has too many members".into());
    }
    let (curves, backwards) = members(st, pb, 12, n, Some(pl))?;
    Ok(Profile { origin: pl.0, normal: pl.1, x: pl.2, curves, turned: false, backwards })
}

/// The curves of a profile's `n` members, listed (`u32` serials) from `at` in `b`. A member kept
/// only in `SKETCH.P_B` is brought into `plane`, which a copied profile does not have.
fn members(st: &Store, b: &[u8], at: usize, n: u32, plane: Option<(V3, V3, V3)>) -> Result<Curves, String> {
    if n > MAX_MEMBERS {
        return Err("the profile has too many members".into());
    }
    let mut curves = Vec::new();
    let mut backwards = Vec::new();
    for k in 0..n as usize {
        let m = u32_at(b, at + 4 * k).ok_or("the 2D profile is cut short")?;
        // No member (a slot left empty: 0, 0xffffffff or 0xfffffffd).
        if m == 0 || m >= 0xffff_fff0 || not_a_curve(st.kind_name(m)) {
            continue;
        }
        let Some(mb) = st.obj(m) else {
            // A curve of a sketch: in the sketches' Parasolid partition.
            let e = st.sketch.get(&m).ok_or("a profile curve is not stored in the part")?;
            let pl = plane.ok_or("a copied profile's curve is kept only in the sketches")?;
            curves.push(e.flat(pl.0, pl.1, pl.2).ok_or("a sketch curve does not lie in the profile plane")?);
            backwards.push(false);
            continue;
        };
        // A dimension or relation listed with the curves (not shaped as a curve: a curve's
        // byte at +8 is 0).
        if mb.get(8) != Some(&0) {
            continue;
        }
        match curve(mb) {
            Some(c) => {
                // An arc drawn clockwise (u8 0 at +34) is kept counter-clockwise, from its end.
                backwards.push(matches!(c, Curve2::Arc(..)) && mb.get(34) == Some(&0));
                curves.push(c);
            }
            None if mb.len() == 46 || mb.len() == 55 => {
                return Err("a profile curve is not readable".into());
            }
            None => return Err("a profile curve of a kind not read (ellipse or spline)".into()),
        }
    }
    if curves.is_empty() {
        return Err("the profile has no curves".into());
    }
    Ok((curves, backwards))
}

/// A profile's curves and, for each, whether it was drawn from its end to its start.
type Curves = (Vec<Curve2>, Vec<bool>);

/// A profile made of edges of the model (an edge set, see [`edge_set`]): one whole circle,
/// on the plane through its centre square to its normal, x along its major axis.
fn edge_profile(b: &[u8]) -> Result<Profile, String> {
    match edge_set(b)?.as_slice() {
        [SetCurve::Conic(c, n, m, ratio, None)] if (ratio - 1.0).abs() < 1e-9 => {
            let r = dot(*m, *m).sqrt();
            let x = unit(*m).ok_or("an edge circle of the set has no radius")?;
            let curves = vec![Curve2::Circle([0.0, 0.0], r)];
            Ok(Profile { origin: *c, normal: *n, x, curves, turned: false, backwards: vec![false] })
        }
        _ => Err("a profile of model edges other than one circle".into()),
    }
}

/// The bytes after a copied profile's frame: five zero bytes, `01 01 00 00 00` and a class id.
const COPY_FRAME_END: [u8; 26] = [
    0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0x40, 0x11, 0x7d, 0xba, 0x44, 0x76, 0x1b, 0x10, 0xac, 0x07, 0x08, 0x00, 0x36, 0x01,
    0xb1, 0x4e,
];

/// A profile copied by a smart pattern or a mirror (`JMDummyComputeFunction`, slot 1 the
/// pattern or mirror): `u32 n` at +12 and the `n` member serials of the seed profile's curves
/// (2D, in the seed's frame), then the curves again. A pattern's copy then stores its plane laid
/// out as a plane object's: origin, normal and x direction (9 `f64`, metres, absolute), followed
/// by [`COPY_FRAME_END`]. A mirror's copy is the seed profile reflected in the mirror's plane
/// (its slot 2; the seed feature is slot 3): the reflected origin, normal and x direction, the
/// curves turned over (y to -y) so the sketch keeps the reflected normal.
fn copied_profile(st: &Store, serial: u32, f: &Func) -> Result<Profile, String> {
    let from = f.slot(1).and_then(|p| st.func(p)).ok_or("the copied profile's pattern is missing")?;
    let b = st.obj(serial).ok_or("the profile is missing")?;
    let n = u32_at(b, 12).ok_or("the copied profile is cut short")?;
    // The seed feature's profile plane, for members kept only in the sketches.
    let seed_feature = |slot: usize| {
        let feat = st.func(from.slot(slot)?)?;
        let prof = st.func(*feat.list(2).first()?)?;
        (prof.class == "JProfileRecomputeEx").then_some(())?;
        st.obj(prof.slot(0)?).and_then(plane)
    };
    match from.class.as_str() {
        "JMSmartPatternFunction" => {
            let (curves, backwards) = members(st, b, 16, n, seed_feature(4))?;
            let (origin, normal, x) = copy_frame(b).ok_or("the copied profile's plane is not readable")?;
            Ok(Profile { origin, normal, x, curves, turned: false, backwards })
        }
        "JMMirrorCopyFunction" => {
            let (mo, mn, _) =
                from.slot(2).and_then(|p| st.obj(p)).and_then(plane).ok_or("the mirror plane is not readable")?;
            let (so, sn, sx) = seed_feature(3).ok_or("the mirrored profile's plane is not readable")?;
            let (curves, backwards) = members(st, b, 16, n, Some((so, sn, sx)))?;
            let flip = |v: V3| {
                let k = 2.0 * dot(v, mn);
                [v[0] - k * mn[0], v[1] - k * mn[1], v[2] - k * mn[2]]
            };
            let d = [so[0] - mo[0], so[1] - mo[1], so[2] - mo[2]];
            let fd = flip(d);
            let origin = [mo[0] + fd[0], mo[1] + fd[1], mo[2] + fd[2]];
            // Turning an arc over swaps its ends.
            let backwards = curves.iter().zip(backwards).map(|(c, b)| b != matches!(c, Curve2::Arc(..))).collect();
            let curves = curves.iter().map(turned_over).collect();
            Ok(Profile { origin, normal: flip(sn), x: flip(sx), curves, turned: true, backwards })
        }
        other => Err(format!("the profile is copied by a {} (not read)", label(other))),
    }
}

/// A profile curve reflected in its x axis (y to -y): counter-clockwise arcs stay so.
fn turned_over(c: &Curve2) -> Curve2 {
    let f = |p: [f64; 2]| [p[0], -p[1]];
    match c {
        Curve2::Line(a, b) => Curve2::Line(f(*a), f(*b)),
        Curve2::Circle(c, r) => Curve2::Circle(f(*c), *r),
        Curve2::Arc(c, r, a0, a1) => Curve2::Arc(f(*c), *r, -a1, -a0),
        Curve2::Spline(d, poles, knots) => Curve2::Spline(*d, poles.iter().map(|p| f(*p)).collect(), knots.clone()),
    }
}

/// A copied profile's plane: origin, normal and x direction (see [`copied_profile`]).
pub(crate) fn copy_frame(b: &[u8]) -> Option<(V3, V3, V3)> {
    let at = b.windows(COPY_FRAME_END.len()).position(|w| w == COPY_FRAME_END)?.checked_sub(72)?;
    let (o, n, x) = (v3_at(b, at)?, v3_at(b, at + 24)?, v3_at(b, at + 48)?);
    if !(finite3(o) && o.iter().all(|v| v.abs() < 1e6)) {
        return None;
    }
    let unit_len = |v: V3| (dot(v, v) - 1.0).abs() < 1e-9;
    (unit_len(n) && unit_len(x) && dot(n, x).abs() < 1e-9).then_some((o, n, x))
}

/// A sketch of profiles on one plane, as a tree feature.
fn sketch_json(name: &str, profiles: &[Profile], axis: Option<([f64; 2], [f64; 2])>) -> Result<Value, String> {
    let first = profiles.first().ok_or("no profile")?;
    if profiles.iter().any(|p| !p.same_plane(first)) {
        return Err("the profiles lie on different planes".into());
    }
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let mut pid = |p: [f64; 2]| {
        let p = [p[0] * MM, p[1] * MM];
        if let Some(i) = pts.iter().position(|q| (q[0] - p[0]).abs() < POINT_EPS && (q[1] - p[1]).abs() < POINT_EPS) {
            return format!("p{i}");
        }
        pts.push(p);
        format!("p{}", pts.len() - 1)
    };
    let mut curves = Vec::new();
    for (k, c) in profiles.iter().flat_map(|p| p.curves.iter()).enumerate() {
        curves.push(match c {
            &Curve2::Line(a, b) => json!({"id": format!("c{k}"), "type": "line", "start": pid(a), "end": pid(b)}),
            &Curve2::Circle(c, r) => {
                json!({"id": format!("c{k}"), "type": "circle", "center": pid(c), "radius": rnd(r * MM)})
            }
            Curve2::Spline(degree, poles, knots) => {
                let poles: Vec<String> = poles.iter().map(|&p| pid(p)).collect();
                let knots: Vec<f64> = knots.iter().map(|&t| rnd(t)).collect();
                json!({"id": format!("c{k}"), "type": "bspline", "degree": degree, "poles": poles, "knots": knots})
            }
            &Curve2::Arc(c, r, a0, a1) => {
                let at = |a: f64| [c[0] + r * libm::cos(a), c[1] + r * libm::sin(a)];
                json!({"id": format!("c{k}"), "type": "arc", "center": pid(c), "start": pid(at(a0)), "end": pid(at(a1))})
            }
        });
    }
    // A revolve axis off the sketch's origin and axes: a construction line along it.
    if let Some((a, b)) = axis {
        let (a, b) = (pid(a), pid(b));
        curves.push(json!({"id": "axis", "type": "line", "start": a, "end": b, "construction": true}));
    }
    let points: Vec<Value> =
        pts.iter().enumerate().map(|(i, p)| json!({"id": format!("p{i}"), "x": rnd(p[0]), "y": rnd(p[1])})).collect();
    Ok(json!({"type": "sketch", "name": name,
              "plane": {"origin": arr_mm(first.origin), "x_axis": arr(first.x), "y_axis": arr(first.y())},
              "points": points, "curves": curves}))
}

/// A revolve axis that lies in the profile plane but is not one of the importer's sketch axes
/// (through the foot of the world origin on the plane, along x or y): its ends in the plane
/// (metres), one metre apart along the axis, for a construction line.
pub(crate) fn axis_in_plane(p: &Profile, (a, d): (V3, V3)) -> Option<([f64; 2], [f64; 2])> {
    let w = [a[0] - p.origin[0], a[1] - p.origin[1], a[2] - p.origin[2]];
    let line = axis_2d(p, (a, d))?;
    // The foot of the world origin on the plane, seen from the axis point.
    let h = dot(p.origin, p.normal);
    let c = cross([h * p.normal[0] - a[0], h * p.normal[1] - a[1], h * p.normal[2] - a[2]], d);
    let through_origin = dot(c, c).sqrt() < 1e-9;
    let along_axis = (dot(d, p.x).abs() - 1.0).abs() < 1e-9 || (dot(d, p.y()).abs() - 1.0).abs() < 1e-9;
    if through_origin && along_axis {
        return None;
    }
    let _ = w;
    Some(line)
}

/// A revolve axis in the profile plane's frame: a point of it and the point a unit further
/// along it. `None` when it does not lie in the plane.
fn axis_2d(p: &Profile, (a, d): (V3, V3)) -> Option<([f64; 2], [f64; 2])> {
    let w = [a[0] - p.origin[0], a[1] - p.origin[1], a[2] - p.origin[2]];
    if dot(w, p.normal).abs() > 1e-9 || dot(d, p.normal).abs() > 1e-9 {
        return None;
    }
    let y = p.y();
    let s = [dot(w, p.x), dot(w, y)];
    Some((s, [s[0] + dot(d, p.x), s[1] + dot(d, y)]))
}

/// How far an open profile's ends reach (metres) past its ends' middle: past any part.
const OPEN_REACH: f64 = 10.0;

/// A point of a 2D curve and a direction there.
type Ray = ([f64; 2], [f64; 2]);

/// The ends of a profile curve as kept: its start and end, each with the direction leading
/// away from the curve there.
fn curve_ends(c: &Curve2) -> Option<(Ray, Ray)> {
    let dir = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
        let l = (dx * dx + dy * dy).sqrt();
        (l > 1e-12).then(|| [dx / l, dy / l])
    };
    match c {
        Curve2::Line(a, b) => Some(((*a, dir(*a, *b)?), (*b, dir(*b, *a)?))),
        Curve2::Arc(c, r, a0, a1) => {
            let at = |t: f64| [c[0] + r * libm::cos(t), c[1] + r * libm::sin(t)];
            // Counter-clockwise: leaving the start backwards, the end forwards.
            let (s0, c0, s1, c1) = (libm::sin(*a0), libm::cos(*a0), libm::sin(*a1), libm::cos(*a1));
            Some(((at(*a0), [s0, -c0]), (at(*a1), [-s1, c1])))
        }
        Curve2::Spline(_, poles, _) => {
            let (p0, p1) = (*poles.first()?, *poles.get(1)?);
            let (q0, q1) = (*poles.last()?, *poles.get(poles.len().checked_sub(2)?)?);
            Some(((p0, dir(p0, p1)?), (q0, dir(q0, q1)?)))
        }
        Curve2::Circle(..) => None,
    }
}

/// An end of an open chain: point, outward direction, whether the curve was drawn from it.
type End = ([f64; 2], [f64; 2], bool);

/// The two free ends of an open profile (one chain), the chain as drawn running from the
/// first to the second; `None` when the profile is closed.
fn free_ends(curves: &[Curve2], backwards: &[bool]) -> Result<Option<(End, End)>, String> {
    let same = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
    let mut ends: Vec<End> = Vec::new();
    for (k, c) in curves.iter().enumerate() {
        if let Some((a, b)) = curve_ends(c) {
            let back = backwards.get(k).copied().unwrap_or(false);
            ends.push((a.0, a.1, !back));
            ends.push((b.0, b.1, back));
        }
    }
    let free: Vec<&End> = ends.iter().filter(|e| ends.iter().filter(|f| same(e.0, f.0)).count() % 2 == 1).collect();
    match free.as_slice() {
        [] => Ok(None),
        [x, y] if !x.2 && y.2 => Ok(Some((**y, **x))),
        [x, y] => Ok(Some((**x, **y))),
        _ => Err("an open profile with more than two ends".into()),
    }
}

/// Points along a 2D profile curve: a line's ends, 64 steps along anything else.
fn curve2_points(c: &Curve2) -> Vec<[f64; 2]> {
    const STEPS: usize = 64;
    match c {
        Curve2::Line(a, b) => vec![*a, *b],
        Curve2::Circle(cn, r) => (0..=STEPS)
            .map(|i| {
                let t = 2.0 * core::f64::consts::PI * i as f64 / STEPS as f64;
                [cn[0] + r * libm::cos(t), cn[1] + r * libm::sin(t)]
            })
            .collect(),
        Curve2::Arc(cn, r, a0, a1) => {
            let a1 = if a1 <= a0 { a1 + 2.0 * core::f64::consts::PI } else { *a1 };
            (0..=STEPS)
                .map(|i| {
                    let t = a0 + (a1 - a0) * i as f64 / STEPS as f64;
                    [cn[0] + r * libm::cos(t), cn[1] + r * libm::sin(t)]
                })
                .collect()
        }
        Curve2::Spline(degree, poles, knots) => {
            let order = *degree as usize + 1;
            let poles3: Vec<V3> = poles.iter().map(|p| [p[0], p[1], 0.0]).collect();
            let (Some(&lo), Some(&hi)) = (knots.get(order - 1), knots.get(poles.len())) else { return Vec::new() };
            (0..=STEPS)
                .filter_map(|i| spline_at(order, &poles3, knots, lo + (hi - lo) * i as f64 / STEPS as f64))
                .map(|p| [p[0], p[1]])
                .collect()
        }
    }
}

/// Whether `p` lies inside the region the curves bound (even-odd).
fn inside(curves: &[Curve2], p: [f64; 2]) -> bool {
    let mut odd = false;
    for c in curves {
        for w in curve2_points(c).windows(2) {
            if let [a, b] = w
                && (a[1] <= p[1]) != (b[1] <= p[1])
                && p[0] < a[0] + (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1])
            {
                odd = !odd;
            }
        }
    }
    odd
}

/// How far from a revolve axis (metres) a profile end still lies on it.
const ON_AXIS: f64 = 1e-7;

/// An open profile of a revolve, closed against its axis (`p`, `q` two points of it in the
/// profile's frame): a chain whose two ends lie on the axis is closed along it; a cutout's chain
/// with one end on it runs on from its other end, along its direction, to a box
/// [`OPEN_REACH`] round it on the profile's side of the axis, and is closed round that box to
/// the end on the axis on the side slot 5 names (1 left of the chain as drawn, 2 right).
pub(crate) fn close_on_axis(
    curves: &mut Vec<Curve2>,
    backwards: &[bool],
    (p, q): ([f64; 2], [f64; 2]),
    side: Option<i64>,
) -> Result<(), String> {
    let Some((a, b)) = free_ends(curves, backwards)? else { return Ok(()) };
    let u = [q[0] - p[0], q[1] - p[1]];
    let ul = libm::hypot(u[0], u[1]);
    if ul < 1e-12 {
        return Err("the revolve axis has no direction".into());
    }
    let u = [u[0] / ul, u[1] / ul];
    // Coordinates along the axis and off it.
    let along = |x: [f64; 2]| (x[0] - p[0]) * u[0] + (x[1] - p[1]) * u[1];
    let off = |x: [f64; 2]| (x[1] - p[1]) * u[0] - (x[0] - p[0]) * u[1];
    match (off(a.0).abs() < ON_AXIS, off(b.0).abs() < ON_AXIS) {
        (true, true) => {
            curves.push(Curve2::Line(b.0, a.0));
            return Ok(());
        }
        (false, false) => return Err("an open revolve profile with no end on its axis".into()),
        _ => {}
    }
    let side = match side {
        Some(1) => 1.0,
        Some(2) => -1.0,
        other => return Err(format!("an open profile's side {other:?}")),
    };
    // `e` the free end off the axis, `o` the one on it; `t` the chain's way at `e` as drawn.
    let (e, o, t) = if off(a.0).abs() < ON_AXIS { (b, a, b.1) } else { (a, b, [-a.1[0], -a.1[1]]) };
    let sv = off(e.0).signum();
    let from = |s: f64, v: f64| [p[0] + s * u[0] - v * sv * u[1], p[1] + s * u[1] + v * sv * u[0]];
    let (se, ve) = (along(e.0), off(e.0) * sv);
    let r = OPEN_REACH + se.abs().max(ve).max(along(o.0).abs());
    // The box: along the axis -r..r, off it 0..r (on the end's side); `e` lies inside it.
    let (ds, dv) = (e.1[0] * u[0] + e.1[1] * u[1], (e.1[1] * u[0] - e.1[0] * u[1]) * sv);
    let hit = [(ds, r - se), (-ds, r + se), (dv, r - ve), (-dv, ve)]
        .iter()
        .filter(|(d, room)| *d > 1e-12 && *room >= 0.0)
        .map(|(d, room)| room / d)
        .fold(f64::INFINITY, f64::min);
    if !hit.is_finite() {
        return Err("an open revolve profile that runs nowhere".into());
    }
    let f = [se + hit * ds, ve + hit * dv];
    // Perimeter positions, counter-clockwise from (-r, 0): the axis, then the far sides.
    let tol = 1e-9 * r;
    let pos = |s: f64, v: f64| {
        if v.abs() < tol {
            s + r
        } else if (s - r).abs() < tol {
            2.0 * r + v
        } else if (v - r).abs() < tol {
            4.0 * r + (r - s)
        } else {
            6.0 * r + (r - v)
        }
    };
    let corners = [(0.0, (-r, 0.0)), (2.0 * r, (r, 0.0)), (4.0 * r, (r, r)), (6.0 * r, (-r, r))];
    let (pf, po) = (pos(f[0], f[1]), pos(along(o.0), 0.0));
    let ring = |ccw: bool| -> Vec<Curve2> {
        let mut out = vec![Curve2::Line(e.0, from(f[0], f[1]))];
        let mut cs: Vec<(f64, (f64, f64))> = corners
            .iter()
            .copied()
            .map(|(k, c)| {
                let d = if ccw { k - pf } else { pf - k };
                (d.rem_euclid(8.0 * r), c)
            })
            .filter(|(d, _)| {
                *d > 0.0 && *d < if ccw { (po - pf).rem_euclid(8.0 * r) } else { (pf - po).rem_euclid(8.0 * r) }
            })
            .collect();
        cs.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut last = from(f[0], f[1]);
        for (_, (s, v)) in cs {
            let n = from(s, v);
            out.push(Curve2::Line(last, n));
            last = n;
        }
        out.push(Curve2::Line(last, o.0));
        out
    };
    // A point just beside the chain near `e` (a step in from it), on the side slot 5 names.
    let tl = libm::hypot(t[0], t[1]).max(1e-300);
    let t = [t[0] / tl, t[1] / tl];
    let eps = 1e-6;
    let probe = [e.0[0] - e.1[0] * eps - side * t[1] * eps, e.0[1] - e.1[1] * eps + side * t[0] * eps];
    for ccw in [true, false] {
        let mut all = curves.clone();
        all.extend(ring(ccw));
        if inside(&all, probe) {
            *curves = all;
            return Ok(());
        }
    }
    Err("an open revolve profile's side is not found".into())
}

/// An open profile of a cutout (one chain with two free ends) closed on the side the cutout
/// removes (slot 5: 1 left of the chain as drawn, 2 right): its ends run on along their
/// directions to a circle [`OPEN_REACH`] round the middle of its ends, and the arc of that
/// circle on that side closes it. A closed profile is left as it is.
pub(crate) fn close_open(curves: &mut Vec<Curve2>, backwards: &[bool], side: Option<i64>) -> Result<(), String> {
    let Some((a, b)) = free_ends(curves, backwards)? else { return Ok(()) };
    let c = [(a.0[0] + b.0[0]) / 2.0, (a.0[1] + b.0[1]) / 2.0];
    let half = libm::hypot(a.0[0] - c[0], a.0[1] - c[1]);
    if half < 1e-9 {
        return Err("an open profile whose ends meet".into());
    }
    let r = OPEN_REACH + half;
    // Where the ray from an end meets the circle (the end lies inside it).
    let out = |(p, d): (&[f64; 2], &[f64; 2])| {
        let w = [p[0] - c[0], p[1] - c[1]];
        let bb = d[0] * w[0] + d[1] * w[1];
        let t = -bb + (bb * bb - (w[0] * w[0] + w[1] * w[1] - r * r)).sqrt();
        [p[0] + t * d[0], p[1] + t * d[1]]
    };
    let (fa, fb) = (out((&a.0, &a.1)), out((&b.0, &b.1)));
    let ang = |p: [f64; 2]| libm::atan2(p[1] - c[1], p[0] - c[0]);
    // Left: on from b, counter-clockwise round to a; right: on from a round to b.
    let (p, fp, fq, q) = match side {
        Some(1) => (b.0, fb, fa, a.0),
        Some(2) => (a.0, fa, fb, b.0),
        other => return Err(format!("an open profile's side {other:?}")),
    };
    curves.push(Curve2::Line(p, fp));
    curves.push(Curve2::Arc(c, r, ang(fp), ang(fq)));
    curves.push(Curve2::Line(fq, q));
    Ok(())
}

/// The feature kinds read, by class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Protrusion,
    Cutout,
    RevolvedProtrusion,
    RevolvedCutout,
    Round,
    Chamfer,
    SweptProtrusion,
    SweptCutout,
    LoftedProtrusion,
    LoftedCutout,
    ThinWall,
    /// A cosmetic thread: no geometry.
    Thread,
    /// The start of an older part's history, before its first feature.
    BaseBody,
    Other,
}

fn kind(class: &str) -> Kind {
    match class {
        "JMProtrusionFunction" => Kind::Protrusion,
        "JMCutoutFunction" => Kind::Cutout,
        "JMRevolvedProtrusionFunction" => Kind::RevolvedProtrusion,
        "JMRevolvedCutoutFunction" => Kind::RevolvedCutout,
        "JMRoundFunction" => Kind::Round,
        "JMMultiSweepProtrusionFunction" => Kind::SweptProtrusion,
        "JMMultiSweepCutoutFunction" => Kind::SweptCutout,
        "JMLoftedProtrusionFunction" => Kind::LoftedProtrusion,
        "JMLoftedCutoutFunction" => Kind::LoftedCutout,
        "JMChamferFunction" => Kind::Chamfer,
        "JMThreadFunction" => Kind::Thread,
        "JMThinwallAssocFunctionNew" => Kind::ThinWall,
        "JMBodyFeatureAssocFunc" => Kind::BaseBody,
        _ => Kind::Other,
    }
}

/// Functions that are bookkeeping, not features of the history.
fn bookkeeping(class: &str) -> bool {
    // Features are `JM*Function` and a few `JM*AssocFunc*` (thin wall, base body, feature
    // group, wrap, extruded surface...).
    let feature = class.ends_with("Function") || (class.contains("AssocFunc") && !class.contains("Property"));
    !class.starts_with("JM")
        || !feature
        || class.contains("Transmigrate")
        || class.contains("Dummy")
        || class.contains("KeyPoint")
}

/// The features of the history in order: the longest chain of previous-feature links.
pub(crate) fn history(st: &Store) -> Result<Vec<(u32, Func)>, String> {
    let mut feats: HashMap<u32, Func> = HashMap::new();
    for &serial in st.objs.keys() {
        let Some(b) = st.obj(serial) else { continue };
        // A function names its class in plain ASCII; skip objects without one cheaply.
        if !b.windows(2).any(|w| w == b"JM") {
            continue;
        }
        let Some(f) = parse_func(b) else { continue };
        if bookkeeping(&f.class) {
            continue;
        }
        feats.insert(serial, f);
        if feats.len() > 4 * MAX_FEATURES {
            return Err("the part has too many features".into());
        }
    }
    if feats.is_empty() {
        return Err(String::new());
    }
    let prev: HashMap<u32, u32> = feats.iter().map(|(&k, f)| (k, f.slot(1).unwrap_or(0))).collect();
    let used: HashSet<u32> = prev.values().copied().collect();
    let mut tails: Vec<u32> = feats.keys().copied().filter(|k| !used.contains(k)).collect();
    tails.sort_unstable();
    let mut best: Vec<u32> = Vec::new();
    for t in tails {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut x = t;
        while feats.contains_key(&x) && seen.insert(x) && chain.len() <= MAX_FEATURES {
            chain.push(x);
            x = prev.get(&x).copied().unwrap_or(0);
        }
        if chain.len() > best.len() {
            best = chain;
        }
    }
    if best.len() > MAX_FEATURES {
        return Err("the part has too many features".into());
    }
    best.reverse();
    Ok(best.into_iter().filter_map(|k| feats.remove(&k).map(|f| (k, f))).collect())
}

fn unsupported(name: &str, what: &str, why: &str) -> Value {
    json!({"type": "unsupported", "name": name, "original_type": what, "reason": why})
}

/// The label of a feature class (`JMRoundFunction` -> `Round`).
fn label(class: &str) -> &str {
    let c = class.strip_prefix("JM").unwrap_or(class);
    ["AssocFunctionNew", "AssocCFunction", "AssocFunction", "AssocFunc", "Function"]
        .iter()
        .find_map(|s| c.strip_suffix(s))
        .unwrap_or(c)
}

struct State {
    out: Vec<Value>,
    first_body: Option<String>,
    omitted: Vec<Value>,
}

/// One extrude or revolve.
fn solid(st: &Store, s: &mut State, f: &Func, name: &str, k: Kind) -> Result<(), String> {
    let profiles: Vec<Profile> =
        f.list(2).iter().filter(|&&p| p != 0).map(|&p| profile(st, p)).collect::<Result<_, _>>()?;
    if profiles.is_empty() {
        return Err("the feature has no profile".into());
    }
    let small = |i: usize| f.slot(i).and_then(|v| st.obj(v)).and_then(value);
    // Kinds are stored as small whole numbers in value objects.
    let code = |i: usize| small(i).filter(|v| v.fract() == 0.0 && v.abs() < 1e6).map(|v| v as i64);
    let extent = code(3).ok_or("the extent is not readable")?;
    let side = code(4).ok_or("the side is not readable")?;
    let mut profiles = profiles;
    if k == Kind::Cutout
        && let [one] = profiles.as_mut_slice()
    {
        // Turned over, the chain's left is the seed's right.
        let side = code(5).map(|s| if one.turned { 3 - s } else { s });
        close_open(&mut one.curves, &one.backwards, side)?;
    }
    let sketch = format!("{name}_Sketch");
    let revolve = matches!(k, Kind::RevolvedProtrusion | Kind::RevolvedCutout);
    let axis = if revolve {
        Some(f.slot(6).and_then(|a| st.obj(a)).and_then(axis_line).ok_or("the revolve axis is not readable")?)
    } else {
        None
    };
    if revolve
        && let (Some(a), [one]) = (axis, profiles.as_mut_slice())
        && let Some(line2) = axis_2d(one, a)
    {
        let side = if k == Kind::RevolvedCutout { code(5).map(|s| if one.turned { 3 - s } else { s }) } else { None };
        close_on_axis(&mut one.curves, &one.backwards, line2, side)?;
    }
    let line = axis.and_then(|a| profiles.first().and_then(|p| axis_in_plane(p, a)));
    let sk = sketch_json(&sketch, &profiles, line)?;
    let cut = matches!(k, Kind::Cutout | Kind::RevolvedCutout);
    let op = if cut {
        "cut"
    } else if s.first_body.is_none() {
        "new"
    } else {
        "join"
    };
    let mut j = json!({"type": if revolve {"revolve"} else {"extrude"}, "name": name, "sketch": sketch,
                       "profiles": "all", "operation": op});
    if op != "new" {
        let fb = s.first_body.clone().ok_or("a cut before any body")?;
        put(&mut j, "target", json!(fb));
    }
    if revolve {
        let (p, d) = axis.ok_or("the revolve axis is not readable")?;
        let angle = match extent {
            13 => small(7).ok_or("the revolve angle is not readable")?.to_degrees(),
            // A full turn.
            17 => 360.0,
            e => return Err(format!("revolve extent {e}")),
        };
        if !(angle.is_finite() && angle.abs() > 1e-9 && angle.abs() <= 360.0 + 1e-9) {
            return Err("the revolve angle is out of range".into());
        }
        let angle = if side == 1 { -angle } else { angle };
        if line.is_some() {
            put(&mut j, "axis", json!({"line": "axis"}));
        } else {
            put(&mut j, "axis", json!({"origin": arr_mm(p), "direction": arr(d)}));
        }
        put(&mut j, "angle", json!(rnd(angle)));
    } else {
        let (dist, symmetric) = match extent {
            13 => {
                let d = small(6).ok_or("the depth is not readable")? * MM;
                if !(d.is_finite() && d.abs() > 1e-9) {
                    return Err("the depth is zero".into());
                }
                (d.abs(), false)
            }
            // Through all; through the next face (16) is swept as far, the rebuild check
            // tells whether the next face was the last one.
            14 | 16 => (THROUGH, false),
            // Up to a key point: the reach the part stores with it, signed along the normal.
            72 => {
                let kp = f.slot(6).ok_or("the key point is missing")?;
                let d = st.obj(kp).and_then(key_point_reach).ok_or("the key point's reach is not readable")?;
                let along = dot(d.1, profiles.first().map_or([0.0; 3], |p| p.normal));
                if (along.abs() - 1.0).abs() > 1e-6 || d.0.abs() < 1e-12 {
                    return Err("the key point's reach is not along the profile normal".into());
                }
                (d.0 * along * MM, false)
            }
            e => return Err(format!("extent {e} (not finite, through or to a key point)")),
        };
        let (dist, symmetric) = match side {
            // Symmetric up to a key point: the reach each way (the pattern copies of such a
            // cut store twice the reach as their symmetric depth).
            3 if extent == 72 => (2.0 * dist.abs(), true),
            _ if extent == 72 => (dist, symmetric),
            1 => (-dist, symmetric),
            2 => (dist, symmetric),
            3 => (dist, true),
            6 if extent != 13 => (2.0 * THROUGH, true),
            other => return Err(format!("side {other}")),
        };
        put(&mut j, "distance", json!(rnd(dist)));
        put(&mut j, "symmetric", json!(symmetric));
        // A second extent (slots 12 to 14, laid out like 3, 4 and 6): the other side.
        if let Some(e2) = code(12) {
            let reach = match e2 {
                13 => small(14).ok_or("the second depth is not readable")?.abs() * MM,
                14 => THROUGH,
                e => return Err(format!("second extent {e} (not finite or through all)")),
            };
            let d2 = match code(13) {
                Some(1) => -reach,
                Some(2) => reach,
                other => return Err(format!("second side {other:?}")),
            };
            if symmetric || d2 * dist >= 0.0 || d2.abs() < 1e-9 {
                return Err("the second extent does not lie on the other side of the profile plane".into());
            }
            put(&mut j, "distance2", json!(rnd(d2)));
        }
        // A taper (slot 17, a list): `173`, the angle (radians), 4 widening away from the
        // profile, 5 narrowing.
        if let [kind, angle, way] = f.list(17) {
            let v = |x: u32| st.obj(x).and_then(value);
            let angle = v(*angle).ok_or("the taper angle is not readable")?.to_degrees();
            let sign = match (v(*kind), v(*way)) {
                (Some(173.0), Some(4.0)) => 1.0,
                (Some(173.0), Some(5.0)) => -1.0,
                other => return Err(format!("taper kind {other:?} (not read)")),
            };
            if !(angle.is_finite() && angle.abs() < 89.0) {
                return Err("the taper angle is out of range".into());
            }
            if code(12).is_some() || symmetric {
                return Err("a taper of an extrude to both sides (not read)".into());
            }
            put(&mut j, "taper_deg", json!(rnd(sign * angle)));
        } else if f.list(17).iter().any(|&x| x != 0) {
            return Err("the taper is not readable".into());
        }
        if extent == 14 {
            put(&mut j, "through_all", json!(true));
        }
        if extent == 16 {
            put(&mut j, "through_next", json!(true));
        }
    }
    s.out.push(sk);
    s.out.push(j);
    if s.first_body.is_none() {
        s.first_body = Some(name.to_string());
    }
    Ok(())
}

/// A sweep (`JMMultiSweep*Function`): slot 2 the path's profiles, slot 3 the cross
/// sections'. One cross section swept along one path is read.
fn sweep(st: &Store, s: &mut State, f: &Func, name: &str, k: Kind) -> Result<(), String> {
    let read = |slot: usize| -> Result<Vec<Profile>, String> {
        f.list(slot).iter().filter(|&&p| p != 0).map(|&p| profile(st, p)).collect()
    };
    let (paths, sections) = (read(2)?, read(3)?);
    if paths.is_empty() || sections.len() != 1 {
        return Err(format!("a sweep of {} cross sections along {} paths", sections.len(), paths.len()));
    }
    let cut = k == Kind::SweptCutout;
    let op = if cut {
        "cut"
    } else if s.first_body.is_none() {
        "new"
    } else {
        "join"
    };
    let (path, section) = (format!("{name}_Path"), format!("{name}_Sketch"));
    let pj = sketch_json(&path, &paths, None)?;
    let sj = sketch_json(&section, &sections, None)?;
    let mut j = json!({"type": "sweep", "name": name, "sketch": section, "path": path, "operation": op});
    if op != "new" {
        let fb = s.first_body.clone().ok_or("a cut before any body")?;
        put(&mut j, "target", json!(fb));
    }
    s.out.push(pj);
    s.out.push(sj);
    s.out.push(j);
    if s.first_body.is_none() {
        s.first_body = Some(name.to_string());
    }
    Ok(())
}

/// A loft (`JMLofted*Function`): slot 2 the cross sections in order, each a sketch of its own.
/// The ends' conditions (slots 11 to 16) are not read: the rebuild check tells.
fn loft(st: &Store, s: &mut State, f: &Func, name: &str, k: Kind) -> Result<(), String> {
    let sections: Vec<Profile> =
        f.list(2).iter().filter(|&&p| p != 0).map(|&p| profile(st, p)).collect::<Result<_, _>>()?;
    if sections.len() < 2 {
        return Err(format!("a loft of {} cross sections", sections.len()));
    }
    let op = if k == Kind::LoftedCutout {
        "cut"
    } else if s.first_body.is_none() {
        "new"
    } else {
        "join"
    };
    let mut names = Vec::new();
    for (i, p) in sections.iter().enumerate() {
        let n = format!("{name}_Section{}", i + 1);
        s.out.push(sketch_json(&n, std::slice::from_ref(p), None)?);
        names.push(n);
    }
    let mut j = json!({"type": "loft", "name": name, "sketches": names, "operation": op});
    if op != "new" {
        let fb = s.first_body.clone().ok_or("a cut before any body")?;
        put(&mut j, "target", json!(fb));
    }
    s.out.push(j);
    if s.first_body.is_none() {
        s.first_body = Some(name.to_string());
    }
    Ok(())
}

/// A round (slot 2: the edge set, 3: kind, 1 constant radius; 4: the radius) or an equal
/// chamfer (slot 2: kind, 41 equal setbacks; 4: the edge set; 5: the setback).
fn blend(st: &Store, s: &mut State, f: &Func, name: &str, k: Kind) -> Result<(), String> {
    if s.first_body.is_none() {
        return Err("a blend before any body".into());
    }
    let small = |i: usize| f.slot(i).and_then(|v| st.obj(v)).and_then(value);
    let code = |i: usize| small(i).filter(|v| v.fract() == 0.0 && v.abs() < 1e6).map(|v| v as i64);
    let round = k == Kind::Round;
    let (set, size) = if round {
        match code(3) {
            Some(1) => {}
            other => return Err(format!("round kind {other:?} (not a constant radius)")),
        }
        (2, 4)
    } else {
        match code(2) {
            Some(41) if f.slot(3).is_none() => {}
            other => return Err(format!("chamfer kind {other:?} (not equal setbacks)")),
        }
        (4, 5)
    };
    let size = small(size).map(|v| v * MM).filter(|v| *v > 1e-9).ok_or("the size is not readable")?;
    let set = f.slot(set).and_then(|v| st.obj(v)).ok_or("the edge set is missing")?;
    let picks: Vec<Value> = edge_set(set)?.iter().map(edge_pick).collect::<Result<_, _>>()?;
    let j = if round {
        json!({"type": "fillet", "name": name, "edges": picks, "radius": rnd(size)})
    } else {
        json!({"type": "chamfer", "name": name, "edges": picks, "distance": rnd(size)})
    };
    s.out.push(j);
    Ok(())
}

/// A thin wall (`JMThinwallAssocFunctionNew`): slot 2 the side (4 the wall inside the body),
/// 3 the thickness, 4 the face set of the faces it opens.
fn thin_wall(st: &Store, s: &mut State, f: &Func, name: &str) -> Result<(), String> {
    let target = s.first_body.clone().ok_or("a thin wall before any body")?;
    let small = |i: usize| f.slot(i).and_then(|v| st.obj(v)).and_then(value);
    let side = match small(2) {
        Some(4.0) => "inside",
        other => return Err(format!("thin wall side {other:?} (not read)")),
    };
    let t = small(3).map(|v| v * MM).filter(|v| *v > 1e-9).ok_or("the thickness is not readable")?;
    let mut j = json!({"type": "shell", "name": name, "thickness": rnd(t), "side": side});
    match f.slot(4) {
        None => put(&mut j, "body", json!(target)),
        Some(set) => {
            let set = st.obj(set).ok_or("the open faces' set is missing")?;
            let picks: Vec<Value> = face_set(set)?.iter().map(face_pick).collect::<Result<_, _>>()?;
            put(&mut j, "faces", json!(picks));
        }
    }
    s.out.push(j);
    Ok(())
}

/// Reads a Solid Edge part's ordered feature history as a feature tree.
///
/// # Errors
/// Why there is no tree: `Err("")` when the part keeps no ordered history (a synchronous
/// part), else what could not be read.
pub fn feature_tree(bytes: &[u8], file_name: &str, limits: &Limits) -> Result<Value, String> {
    let mut warnings = Vec::new();
    let f = open(bytes, limits, &mut warnings).map_err(|e| e.to_string())?;
    let find = |n: &str| (0..f.streams.len()).find(|&i| f.streams.get(i).is_some_and(|e| e.name == n));
    let ci = find("PSMcluster0").ok_or("the part has no object store")?;
    let data = f.read(ci, limits).map_err(|e| e.to_string())?;
    let tt = match find("PSMtypetable") {
        Some(i) => f.read(i, limits).map_err(|e| e.to_string())?,
        None => Vec::new(),
    };
    let mut st = Store::new(data, &tt)?;
    if let Some(i) = find("SKETCH.P_B") {
        st.sketch = sketch_edges(&f.read(i, limits).map_err(|e| e.to_string())?);
    }
    let feats = history(&st)?;
    let mut s = State { out: Vec::new(), first_body: None, omitted: Vec::new() };
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut solids = 0;
    for (_, func) in &feats {
        let what = label(&func.class);
        let n = counts.entry(what).or_insert(0);
        *n += 1;
        let name = format!("{what}_{n}");
        let k = kind(&func.class);
        match k {
            Kind::Protrusion | Kind::Cutout | Kind::RevolvedProtrusion | Kind::RevolvedCutout => {
                let before = s.out.len();
                if let Err(why) = solid(&st, &mut s, func, &name, k) {
                    s.out.truncate(before);
                    s.out.push(unsupported(&name, what, &why));
                } else {
                    solids += 1;
                }
            }
            Kind::SweptProtrusion | Kind::SweptCutout | Kind::LoftedProtrusion | Kind::LoftedCutout => {
                let before = s.out.len();
                let made = if matches!(k, Kind::SweptProtrusion | Kind::SweptCutout) {
                    sweep(&st, &mut s, func, &name, k)
                } else {
                    loft(&st, &mut s, func, &name, k)
                };
                if let Err(why) = made {
                    s.out.truncate(before);
                    s.out.push(unsupported(&name, what, &why));
                } else {
                    solids += 1;
                }
            }
            Kind::Round | Kind::Chamfer => {
                if let Err(why) = blend(&st, &mut s, func, &name, k) {
                    s.out.push(unsupported(&name, what, &why));
                }
            }
            Kind::ThinWall => {
                if let Err(why) = thin_wall(&st, &mut s, func, &name) {
                    s.out.push(unsupported(&name, what, &why));
                }
            }
            Kind::Thread => s.omitted.push(json!({"name": name, "why": "cosmetic thread"})),
            // Left out: if it held geometry, the rebuild check finds the body short.
            Kind::BaseBody if s.out.is_empty() => s.omitted.push(json!({"name": name, "why": "base body feature"})),
            Kind::Other | Kind::BaseBody => s.out.push(unsupported(&name, what, "feature kind not read yet")),
        }
    }
    if solids == 0 && s.out.iter().all(|f| f.get("type").and_then(Value::as_str) == Some("unsupported")) {
        // Only bookkeeping of a synchronous part, or nothing read at all.
        if feats
            .iter()
            .all(|(_, f)| matches!(label(&f.class), "Hole" | "Thread" | "UserDefinedPattern" | "PartOwnerTrans"))
        {
            return Err(String::new());
        }
    }
    let file = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    Ok(json!({"format": "gusset-feature-tree", "version": 2,
              "source": {"app": "solidedge", "file": file, "omitted": s.omitted},
              "features": s.out}))
}
