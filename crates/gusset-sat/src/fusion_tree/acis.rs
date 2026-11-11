//! A minimal reader for the binary ACIS blobs (`BREP.*.smb`, `.smbh`) of a Fusion file: what the
//! edge naming needs to check against the stored bodies, edges' end points per body and the
//! faces that carry a generator tag. Bounded, total: a blob that does not tokenize is skipped.

use super::geom::V3;
use std::collections::{HashMap, HashSet};

/// Most records one blob may hold.
const MAX_RECORDS: usize = 4_000_000;

#[derive(Debug, Default)]
struct ARec {
    ty: String,
    ptrs: Vec<i64>,
    /// Non-empty `0x07` strings.
    strs: Vec<String>,
    /// The first position (`0x13`) token.
    pos: Option<V3>,
    /// Integer (`0x04`) tokens, and for each string of `strs` how many came before it.
    ints: Vec<i64>,
    str_at: Vec<usize>,
    /// Double (`0x06`) tokens and position/vector (`0x13`, `0x14`) tokens.
    nums: Vec<f64>,
    vecs: Vec<V3>,
}

fn find(b: &[u8], pat: &[u8]) -> Option<usize> {
    b.windows(pat.len()).position(|w| w == pat)
}

fn size_of(t: u8, wide: bool) -> Option<usize> {
    Some(match t {
        0x04 | 0x0c | 0x15 => {
            if wide {
                8
            } else {
                4
            }
        }
        0x05 | 0x03 => 4,
        0x06 | 0x16 => 8,
        0x13 | 0x14 => 24,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 => 0,
        _ => return None,
    })
}

fn records(b: &[u8]) -> Option<Vec<ARec>> {
    let h = find(b, b"\x0d\x09asmheader")?;
    let wide = b.starts_with(b"ASM BinaryFile8") || b.starts_with(b"ACIS BinaryFile8");
    let mut i = h;
    let mut cur = ARec::default();
    let mut names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    while i < b.len() {
        let t = *b.get(i)?;
        i += 1;
        match t {
            0x07 | 0x0d | 0x0e => {
                let l = usize::from(*b.get(i)?);
                let s = b.get(i + 1..i + 1 + l)?;
                i += 1 + l;
                if !s.is_empty() {
                    let text: String = s.iter().map(|&c| char::from(c)).collect();
                    if t == 0x07 {
                        cur.str_at.push(cur.ints.len());
                        cur.strs.push(text);
                    } else {
                        names.push(text);
                    }
                }
            }
            0x08 => {
                let l = usize::from(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?));
                i += 2 + l;
            }
            0x09 | 0x12 => {
                let l = u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize;
                i = i.checked_add(4 + l)?;
            }
            _ => {
                let n = size_of(t, wide)?;
                let v = b.get(i..i + n)?;
                i += n;
                match t {
                    0x0c => {
                        let p = if n == 4 {
                            i64::from(i32::from_le_bytes(v.try_into().ok()?))
                        } else {
                            i64::from_le_bytes(v.try_into().ok()?)
                        };
                        cur.ptrs.push(p);
                    }
                    0x04 => {
                        let p = if n == 4 {
                            i64::from(i32::from_le_bytes(v.try_into().ok()?))
                        } else {
                            i64::from_le_bytes(v.try_into().ok()?)
                        };
                        cur.ints.push(p);
                    }
                    0x06 => cur.nums.push(f64::from_le_bytes(v.try_into().ok()?)),
                    0x13 | 0x14 => {
                        let f =
                            |k: usize| v.get(k * 8..k * 8 + 8).and_then(|s| s.try_into().ok()).map(f64::from_le_bytes);
                        let p = [f(0)?, f(1)?, f(2)?];
                        if t == 0x13 && cur.pos.is_none() {
                            cur.pos = Some(p);
                        }
                        cur.vecs.push(p);
                    }
                    _ => {}
                }
                if t == 0x11 {
                    cur.ty = names.join("-");
                    names.clear();
                    out.push(std::mem::take(&mut cur));
                    if out.len() > MAX_RECORDS {
                        return None;
                    }
                }
            }
        }
    }
    Some(out)
}

fn is_body(t: &str) -> bool {
    t.ends_with("body") && !t.ends_with("_body")
}

/// The records reachable from body record `k` through pointers, without crossing into other
/// bodies, attributes or transforms.
fn closure(recs: &[ARec], k: usize) -> HashSet<usize> {
    let mut seen = HashSet::from([k]);
    let mut st = vec![k];
    while let Some(x) = st.pop() {
        let Some(r) = recs.get(x) else { continue };
        for &p in &r.ptrs {
            let Ok(p) = usize::try_from(p) else { continue };
            let Some(pr) = recs.get(p) else { continue };
            if !seen.contains(&p) && !is_body(&pr.ty) && !pr.ty.contains("attrib") && !pr.ty.contains("transform") {
                seen.insert(p);
                st.push(p);
            }
        }
    }
    seen
}

fn vertex_points(recs: &[ARec]) -> HashMap<usize, V3> {
    let pos: HashMap<usize, V3> = recs
        .iter()
        .enumerate()
        .filter(|(_, r)| r.ty.ends_with("point"))
        .filter_map(|(k, r)| Some((k, r.pos?)))
        .collect();
    let mut vpt = HashMap::new();
    for (k, r) in recs.iter().enumerate() {
        if r.ty.ends_with("vertex")
            && let Some(p) = r.ptrs.iter().find_map(|p| usize::try_from(*p).ok().and_then(|p| pos.get(&p)))
        {
            vpt.insert(k, *p);
        }
    }
    vpt
}

/// An edge of a stored body: its end points (cm).
pub(super) type Edge = (V3, V3);

/// Per body of the blob: its edges' end points.
pub(super) fn body_edges(b: &[u8]) -> Option<Vec<Vec<Edge>>> {
    let recs = records(b)?;
    let vpt = vertex_points(&recs);
    let mut out = Vec::new();
    for (k, r) in recs.iter().enumerate() {
        if !is_body(&r.ty) {
            continue;
        }
        let mut edges = Vec::new();
        for e in closure(&recs, k) {
            let Some(er) = recs.get(e) else { continue };
            if er.ty.ends_with("edge") && !er.ty.ends_with("coedge") {
                let vs: Vec<V3> = er
                    .ptrs
                    .iter()
                    .filter_map(|p| usize::try_from(*p).ok().and_then(|p| vpt.get(&p)).copied())
                    .collect();
                if let [a, b, ..] = vs.as_slice() {
                    edges.push((*a, *b));
                }
            }
        }
        if !edges.is_empty() {
            out.push(edges);
        }
    }
    Some(out)
}

/// A face carrying a generator tag (`generic_tag_attrib_def`): the tag text and the vertex
/// positions of its edges.
#[derive(Debug)]
pub(super) struct TaggedFace {
    pub tag: String,
    /// The face's whole name (generators and their operation ids), where the attribute has one.
    pub name: Option<Name>,
    pub verts: Vec<V3>,
}

pub(super) fn tagged_faces(b: &[u8]) -> Option<Vec<TaggedFace>> {
    let recs = records(b)?;
    let vpt = vertex_points(&recs);
    let mut tag: HashMap<usize, (String, Option<Name>)> = HashMap::new();
    for r in &recs {
        if !r.ty.contains("attrib") || r.strs.first().map(String::as_str) != Some("generic_tag_attrib_def") {
            continue;
        }
        let owner = r
            .ptrs
            .iter()
            .filter_map(|p| usize::try_from(*p).ok())
            .find(|p| recs.get(*p).is_some_and(|o| !o.ty.contains("attrib")));
        if let (Some(o), Some(t)) = (owner, r.strs.last())
            && recs.get(o).is_some_and(|o| o.ty.ends_with("face"))
        {
            tag.insert(o, (t.clone(), tag_name(r)));
        }
    }
    let mut ce_loop: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, r) in recs.iter().enumerate() {
        if r.ty.ends_with("coedge") {
            for p in &r.ptrs {
                if let Ok(p) = usize::try_from(*p)
                    && recs.get(p).is_some_and(|x| x.ty.ends_with("loop"))
                {
                    ce_loop.entry(p).or_default().push(k);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (k, r) in recs.iter().enumerate() {
        if !is_body(&r.ty) {
            continue;
        }
        let seen = closure(&recs, k);
        let mut faces: Vec<usize> = seen.iter().copied().filter(|f| tag.contains_key(f)).collect();
        faces.sort_unstable();
        for f in faces {
            let Some(fr) = recs.get(f) else { continue };
            let mut verts: Vec<V3> = Vec::new();
            for p in &fr.ptrs {
                let Ok(l) = usize::try_from(*p) else { continue };
                if !recs.get(l).is_some_and(|x| x.ty.ends_with("loop")) {
                    continue;
                }
                for ce in ce_loop.get(&l).map(Vec::as_slice).unwrap_or(&[]) {
                    let Some(cr) = recs.get(*ce) else { continue };
                    for p in &cr.ptrs {
                        let Ok(e) = usize::try_from(*p) else { continue };
                        let Some(er) = recs.get(e) else { continue };
                        if er.ty.ends_with("edge") && !er.ty.ends_with("coedge") {
                            for q in &er.ptrs {
                                if let Some(v) = usize::try_from(*q).ok().and_then(|q| vpt.get(&q))
                                    && !verts.contains(v)
                                {
                                    verts.push(*v);
                                }
                            }
                        }
                    }
                }
            }
            if let Some((t, name)) = tag.get(&f) {
                out.push(TaggedFace { tag: t.clone(), name: name.clone(), verts });
            }
        }
    }
    Some(out)
}

/// A face name as the design stream writes it: entries of (generator, operation ids).
pub(super) type Name = Vec<(String, Vec<i32>)>;

/// The name a `generic_tag_attrib_def` attribute gives its owner: after the attribute's second
/// name string comes the entry count, then per entry the length of the generator text, the
/// text, a number, the count of operation ids, the ids and `0` (the layout of a face name in a
/// recipe record).
fn tag_name(r: &ARec) -> Option<Name> {
    let k = r.strs.iter().rposition(|x| x.starts_with("generic_tag_attrib_def"))?;
    if k == 0 {
        return None;
    }
    let count = usize::try_from(*r.ints.get(*r.str_at.get(k)?)?).ok().filter(|c| *c <= 256)?;
    let mut out = Name::new();
    for j in 0..count {
        let g = r.strs.get(k + 1 + j)?;
        let at = *r.str_at.get(k + 1 + j)?;
        // The text's length stands before it.
        if usize::try_from(*r.ints.get(at.checked_sub(1)?)?).ok() != Some(g.len()) {
            return None;
        }
        let n = usize::try_from(*r.ints.get(at + 1)?).ok().filter(|n| *n <= 4096)?;
        let ops: Option<Vec<i32>> =
            (0..n).map(|i| r.ints.get(at + 2 + i).and_then(|v| i32::try_from(*v).ok())).collect();
        out.push((g.clone(), ops?));
    }
    (!out.is_empty()).then_some(out)
}

/// An edge of a stored body between two named faces: the faces' names, the names of the other
/// faces at its two ends, its ends and its middle (cm).
#[derive(Debug)]
pub(super) struct NamedEdge {
    pub faces: [Name; 2],
    pub around: Vec<Name>,
    pub a: V3,
    pub b: V3,
    pub mid: V3,
    /// Points along the edge from `a` to `b` (both included).
    pub path: Vec<V3>,
}

/// A named face of a stored body that lies on a plane: the plane's point and normal (cm).
#[derive(Debug)]
pub(super) struct NamedPlane {
    pub name: Name,
    pub point: V3,
    pub normal: V3,
}

/// The face names of a blob: record index of the face to its name.
fn face_names(recs: &[ARec]) -> HashMap<usize, Name> {
    let mut name: HashMap<usize, Name> = HashMap::new();
    for r in recs {
        if !r.ty.contains("attrib") || r.strs.first().map(String::as_str) != Some("generic_tag_attrib_def") {
            continue;
        }
        let owner = r
            .ptrs
            .iter()
            .filter_map(|p| usize::try_from(*p).ok())
            .find(|p| recs.get(*p).is_some_and(|o| !o.ty.contains("attrib")));
        if let (Some(o), Some(n)) = (owner, tag_name(r))
            && recs.get(o).is_some_and(|x| x.ty.ends_with("face"))
        {
            name.insert(o, n);
        }
    }
    name
}

/// The planar named faces of the blob's bodies.
pub(super) fn named_planes(b: &[u8]) -> Option<Vec<NamedPlane>> {
    let recs = records(b)?;
    let names = face_names(&recs);
    let mut keys: Vec<&usize> = names.keys().collect();
    keys.sort_unstable();
    let mut out = Vec::new();
    for f in keys {
        let (Some(fr), Some(name)) = (recs.get(*f), names.get(f)) else { continue };
        let surface = fr
            .ptrs
            .iter()
            .filter_map(|p| usize::try_from(*p).ok())
            .filter_map(|p| recs.get(p))
            .find(|x| x.ty.ends_with("plane-surface"));
        if let Some(sr) = surface
            && let (Some(point), Some(normal)) = (sr.vecs.first(), sr.vecs.get(1))
        {
            out.push(NamedPlane { name: name.clone(), point: *point, normal: *normal });
        }
    }
    Some(out)
}

fn curve_point(c: &ARec, t: f64) -> Option<V3> {
    if c.ty.ends_with("straight-curve") {
        let (p, v) = (c.vecs.first()?, c.vecs.get(1)?);
        return Some([p[0] + t * v[0], p[1] + t * v[1], p[2] + t * v[2]]);
    }
    if c.ty.ends_with("ellipse-curve") {
        let (p, n, m) = (c.vecs.first()?, c.vecs.get(1)?, c.vecs.get(2)?);
        let ratio = *c.nums.first()?;
        let w = [n[1] * m[2] - n[2] * m[1], n[2] * m[0] - n[0] * m[2], n[0] * m[1] - n[1] * m[0]];
        let (co, si) = (libm::cos(t), libm::sin(t) * ratio);
        return Some([p[0] + co * m[0] + si * w[0], p[1] + co * m[1] + si * w[1], p[2] + co * m[2] + si * w[2]]);
    }
    None
}

/// The edges of the blob's bodies that lie between two faces carrying a name. An edge whose
/// curve is not a line or an ellipse, or whose stored parameters do not give its end vertices,
/// is left out (its middle is not known).
pub(super) fn named_edges(b: &[u8]) -> Option<Vec<NamedEdge>> {
    let recs = records(b)?;
    let vpt = vertex_points(&recs);
    let at = |p: &i64| usize::try_from(*p).ok().filter(|p| *p < recs.len());
    let kind = |p: usize, end: &str| recs.get(p).is_some_and(|x| x.ty.ends_with(end));
    let name = face_names(&recs);
    let mut loop_face: HashMap<usize, usize> = HashMap::new();
    for (k, r) in recs.iter().enumerate() {
        if r.ty.ends_with("loop")
            && let Some(f) = r.ptrs.iter().filter_map(at).find(|p| kind(*p, "face"))
        {
            loop_face.insert(k, f);
        }
    }
    let is_edge = |p: usize| kind(p, "edge") && !kind(p, "coedge");
    let mut edge_faces: HashMap<usize, Vec<usize>> = HashMap::new();
    for r in recs.iter().filter(|r| r.ty.ends_with("coedge")) {
        let e = r.ptrs.iter().filter_map(at).find(|p| is_edge(*p));
        let f = r.ptrs.iter().filter_map(at).find(|p| kind(*p, "loop")).and_then(|l| loop_face.get(&l));
        if let (Some(e), Some(f)) = (e, f) {
            let v = edge_faces.entry(e).or_default();
            if !v.contains(f) {
                v.push(*f);
            }
        }
    }
    // The faces at each vertex.
    let mut vertex_faces: HashMap<usize, Vec<usize>> = HashMap::new();
    for (e, fs) in &edge_faces {
        let Some(er) = recs.get(*e) else { continue };
        for v in er.ptrs.iter().filter_map(at).filter(|p| vpt.contains_key(p)) {
            let l = vertex_faces.entry(v).or_default();
            for f in fs {
                if !l.contains(f) {
                    l.push(*f);
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut keys: Vec<&usize> = edge_faces.keys().collect();
    keys.sort_unstable();
    for e in keys {
        let (Some(er), Some([f0, f1])) = (recs.get(*e), edge_faces.get(e).map(Vec::as_slice)) else { continue };
        let (Some(n0), Some(n1)) = (name.get(f0), name.get(f1)) else { continue };
        let vs: Vec<usize> = er.ptrs.iter().filter_map(at).filter(|p| vpt.contains_key(p)).collect();
        let [va, vb] = vs.as_slice() else { continue };
        let (Some(a), Some(b)) = (vpt.get(va).copied(), vpt.get(vb).copied()) else { continue };
        let (Some(t0), Some(t1)) = (er.nums.first().copied(), er.nums.get(1).copied()) else { continue };
        let Some(curve) = er.ptrs.iter().filter_map(at).find(|p| kind(*p, "curve")).and_then(|c| recs.get(c)) else {
            continue;
        };
        // The edge runs along its curve or against it (the parameters are then negated).
        let near = |p: V3, q: V3| (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs() < 1e-6;
        const STEPS: u32 = 16;
        let path = [1.0, -1.0].into_iter().find_map(|s| {
            let (p0, p1) = (curve_point(curve, s * t0)?, curve_point(curve, s * t1)?);
            if !(near(p0, a) && near(p1, b)) {
                return None;
            }
            (0..=STEPS).map(|k| curve_point(curve, s * (t0 + (t1 - t0) * f64::from(k) / f64::from(STEPS)))).collect()
        });
        let Some(path): Option<Vec<V3>> = path else { continue };
        let Some(mid) = path.get(path.len() / 2).copied() else { continue };
        let mut around = Vec::new();
        for v in [va, vb] {
            for f in vertex_faces.get(v).map(Vec::as_slice).unwrap_or(&[]) {
                if f != f0
                    && f != f1
                    && let Some(n) = name.get(f)
                    && !around.contains(n)
                {
                    around.push(n.clone());
                }
            }
        }
        out.push(NamedEdge { faces: [n0.clone(), n1.clone()], around, a, b, mid, path });
    }
    Some(out)
}

/// What one body of a blob is, as far as the PCB features need it: its lumps, the box of its
/// vertices, its cylinders and whether it has any other curved surface.
#[derive(Debug, Clone)]
pub(super) struct BodyShape {
    pub(super) lumps: usize,
    /// The box of the body's vertices (cm): `None` for a body without vertices.
    pub(super) vertex_box: Option<(V3, V3)>,
    /// Root point, unit axis and radius (cm) of every cylindrical surface.
    pub(super) cylinders: Vec<(V3, V3, f64)>,
    /// A surface that is neither a plane nor a cylinder.
    pub(super) other_surfaces: bool,
    /// The body record names a transform: its geometry is not in the blob's own frame.
    pub(super) transformed: bool,
}

impl BodyShape {
    /// All vertices in one plane of constant x, y or z: a sheet, not a solid.
    pub(super) fn flat(&self) -> bool {
        self.vertex_box.is_none_or(|(lo, hi)| lo.iter().zip(hi).any(|(a, b)| (b - a).abs() < 1e-9))
    }
}

/// A transform record that moves nothing: unit rows, no translation, scale 1 (as doubles or as
/// vectors).
fn identity(t: &ARec) -> bool {
    const ROWS: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    let flat: Vec<f64> = t.vecs.iter().flatten().copied().collect();
    let as_doubles = t.nums.get(..12) == Some(&ROWS[..]) && t.nums.get(12) == Some(&1.0);
    let as_vectors = flat.get(..12) == Some(&ROWS[..]) && t.nums.first() == Some(&1.0);
    as_doubles || as_vectors
}

/// Every body of the blob, in file order (none left out).
pub(super) fn body_shapes(b: &[u8]) -> Option<Vec<BodyShape>> {
    let recs = records(b)?;
    let vpt = vertex_points(&recs);
    let mut out = Vec::new();
    for (k, r) in recs.iter().enumerate() {
        if !is_body(&r.ty) {
            continue;
        }
        let mut shape = BodyShape {
            lumps: 0,
            vertex_box: None,
            cylinders: Vec::new(),
            other_surfaces: false,
            transformed: r.ptrs.iter().any(|p| {
                usize::try_from(*p)
                    .ok()
                    .and_then(|p| recs.get(p))
                    .is_some_and(|x| x.ty.contains("transform") && !identity(x))
            }),
        };
        let mut members: Vec<usize> = closure(&recs, k).into_iter().collect();
        members.sort_unstable();
        for m in members {
            let Some(x) = recs.get(m) else { continue };
            if x.ty.ends_with("lump") {
                shape.lumps += 1;
            } else if let Some(p) = vpt.get(&m) {
                shape.vertex_box = Some(shape.vertex_box.map_or((*p, *p), |(lo, hi)| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
                        [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
                    )
                }));
            } else if x.ty.ends_with("cone-surface") {
                // Root, axis, major radius vector; ratio, sine and cosine of the half angle.
                match (x.vecs.as_slice(), x.nums.as_slice()) {
                    ([root, axis, major, ..], [ratio, sine, ..])
                        if sine.abs() < 1e-12 && (ratio - 1.0).abs() < 1e-12 =>
                    {
                        let n = super::geom::norm(*axis);
                        let radius = super::geom::norm(*major);
                        if n > 0.0 {
                            shape.cylinders.push((*root, [axis[0] / n, axis[1] / n, axis[2] / n], radius));
                        }
                    }
                    _ => shape.other_surfaces = true,
                }
            } else if x.ty.ends_with("-surface") && !x.ty.ends_with("plane-surface") {
                shape.other_surfaces = true;
            }
        }
        out.push(shape);
    }
    Some(out)
}
