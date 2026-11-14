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
//!   14 through all), side (slot 4: 1 against the profile plane's normal, 2 along it,
//!   3 symmetric, 6 both ways), depth (slot 6: a value object or a dimension relation);
//! * revolved protrusions and cutouts: profiles, axis (slot 6: a line), angle (slot 7);
//! * each profile: its plane (an absolute frame in the plane object) and its 2D curves
//!   (lines, circles and arcs, in metres in the plane's frame).
//!
//! Anything else (rounds, chamfers, holes, patterns, sweeps, lofts...) is an `unsupported`
//! feature: the caller rebuilds the tree and keeps it only when it gives the part's own body.
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
/// Largest serial step between two consecutive objects (a `QQ` inside a payload whose
/// "serial" jumps further is not an object start).
const MAX_SERIAL_STEP: u32 = 5000;
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
        let mut starts: Vec<(u32, u16, usize)> = Vec::new();
        let mut last = 0u32;
        let mut i = 0;
        while let Some(p) = data.get(i..).and_then(|t| t.windows(2).position(|w| w == b"QQ")) {
            let at = i + p;
            i = at + 1;
            let (Some(kind), Some(serial)) = (u16_at(&data, at + 2), u32_at(&data, at + 4)) else { break };
            if serial > last && serial - last < MAX_SERIAL_STEP {
                if starts.len() >= MAX_OBJECTS {
                    return Err("the part has too many objects".into());
                }
                starts.push((serial, kind, at));
                last = serial;
                i = at + 8;
            }
        }
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
        // The head fin's vertex ends the edge, the other fin's starts it.
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

/// A 2D curve of a profile (metres, in the profile plane's frame).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Curve2 {
    Line([f64; 2], [f64; 2]),
    Circle([f64; 2], f64),
    /// Centre, radius, start and end angles, counter-clockwise from start to end.
    Arc([f64; 2], f64, f64, f64),
}

/// A profile member: a line (`u16 0, x1, y1, x2, y2`), a circle (`u16 0, cx, cy, r, 0.0,
/// 1.0, u8`) or an arc (`u16 0, cx, cy, r, u8 ccw, a0, a1`); `None` for anything else.
pub(crate) fn curve(b: &[u8]) -> Option<Curve2> {
    if u16_at(b, 8)? != 0 {
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
        _ => None,
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
struct Profile {
    origin: V3,
    normal: V3,
    x: V3,
    curves: Vec<Curve2>,
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
    let mut curves = Vec::new();
    for k in 0..n as usize {
        let m = u32_at(pb, 12 + 4 * k).ok_or("the 2D profile is cut short")?;
        if not_a_curve(st.kind_name(m)) {
            continue;
        }
        let Some(mb) = st.obj(m) else {
            // A curve of a sketch: in the sketches' Parasolid partition.
            let e = st.sketch.get(&m).ok_or("a profile curve is not stored in the part")?;
            curves.push(e.flat(pl.0, pl.1, pl.2).ok_or("a sketch curve does not lie in the profile plane")?);
            continue;
        };
        match curve(mb) {
            Some(c) => curves.push(c),
            None if mb.len() == 46 || mb.len() == 55 => {
                return Err("a profile curve is not readable".into());
            }
            None => return Err("a profile curve of a kind not read (ellipse or spline)".into()),
        }
    }
    if curves.is_empty() {
        return Err("the profile has no curves".into());
    }
    Ok(Profile { origin: pl.0, normal: pl.1, x: pl.2, curves })
}

/// A sketch of profiles on one plane, as a tree feature.
fn sketch_json(name: &str, profiles: &[Profile]) -> Result<Value, String> {
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
        curves.push(match *c {
            Curve2::Line(a, b) => json!({"id": format!("c{k}"), "type": "line", "start": pid(a), "end": pid(b)}),
            Curve2::Circle(c, r) => {
                json!({"id": format!("c{k}"), "type": "circle", "center": pid(c), "radius": rnd(r * MM)})
            }
            Curve2::Arc(c, r, a0, a1) => {
                let at = |a: f64| [c[0] + r * libm::cos(a), c[1] + r * libm::sin(a)];
                json!({"id": format!("c{k}"), "type": "arc", "center": pid(c), "start": pid(at(a0)), "end": pid(at(a1))})
            }
        });
    }
    let points: Vec<Value> =
        pts.iter().enumerate().map(|(i, p)| json!({"id": format!("p{i}"), "x": rnd(p[0]), "y": rnd(p[1])})).collect();
    Ok(json!({"type": "sketch", "name": name,
              "plane": {"origin": arr_mm(first.origin), "x_axis": arr(first.x), "y_axis": arr(first.y())},
              "points": points, "curves": curves}))
}

/// The feature kinds read, by class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Protrusion,
    Cutout,
    RevolvedProtrusion,
    RevolvedCutout,
    /// A cosmetic thread: no geometry.
    Thread,
    Other,
}

fn kind(class: &str) -> Kind {
    match class {
        "JMProtrusionFunction" => Kind::Protrusion,
        "JMCutoutFunction" => Kind::Cutout,
        "JMRevolvedProtrusionFunction" => Kind::RevolvedProtrusion,
        "JMRevolvedCutoutFunction" => Kind::RevolvedCutout,
        "JMThreadFunction" => Kind::Thread,
        _ => Kind::Other,
    }
}

/// Functions that are bookkeeping, not features of the history.
fn bookkeeping(class: &str) -> bool {
    !class.starts_with("JM")
        || !class.ends_with("Function")
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
    class.strip_prefix("JM").and_then(|c| c.strip_suffix("Function")).unwrap_or(class)
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
    let sketch = format!("{name}_Sketch");
    let sk = sketch_json(&sketch, &profiles)?;
    let cut = matches!(k, Kind::Cutout | Kind::RevolvedCutout);
    let op = if cut {
        "cut"
    } else if s.first_body.is_none() {
        "new"
    } else {
        "join"
    };
    let revolve = matches!(k, Kind::RevolvedProtrusion | Kind::RevolvedCutout);
    let mut j = json!({"type": if revolve {"revolve"} else {"extrude"}, "name": name, "sketch": sketch,
                       "profiles": "all", "operation": op});
    if op != "new" {
        let fb = s.first_body.clone().ok_or("a cut before any body")?;
        put(&mut j, "target", json!(fb));
    }
    if revolve {
        let (p, d) = f.slot(6).and_then(|a| st.obj(a)).and_then(axis_line).ok_or("the revolve axis is not readable")?;
        let angle = if extent == 13 {
            small(7).ok_or("the revolve angle is not readable")?.to_degrees()
        } else {
            return Err(format!("revolve extent {extent}"));
        };
        if !(angle.is_finite() && angle.abs() > 1e-9 && angle.abs() <= 360.0 + 1e-9) {
            return Err("the revolve angle is out of range".into());
        }
        let angle = if side == 1 { -angle } else { angle };
        put(&mut j, "axis", json!({"origin": arr_mm(p), "direction": arr(d)}));
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
            14 => (THROUGH, false),
            e => return Err(format!("extent {e} (not finite or through all)")),
        };
        let (dist, symmetric) = match side {
            1 => (-dist, symmetric),
            2 => (dist, symmetric),
            3 => (dist, true),
            6 if extent == 14 => (2.0 * THROUGH, true),
            other => return Err(format!("side {other}")),
        };
        put(&mut j, "distance", json!(rnd(dist)));
        put(&mut j, "symmetric", json!(symmetric));
        if extent == 14 {
            put(&mut j, "through_all", json!(true));
        }
    }
    s.out.push(sk);
    s.out.push(j);
    if s.first_body.is_none() {
        s.first_body = Some(name.to_string());
    }
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
            Kind::Thread => s.omitted.push(json!({"name": name, "why": "cosmetic thread"})),
            Kind::Other => s.out.push(unsupported(&name, what, "feature kind not read yet")),
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
