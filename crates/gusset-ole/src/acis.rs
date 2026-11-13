//! A topology-level reader of ShapeManager binary files, for the Inventor feature history
//! (clean room, from sample bytes; the token layout is the one `gusset-sat` reads for `.smb`).
//!
//! A file is a header record `asmheader`, then records: a run of tokens ended by `0x11`. A token
//! is a tag byte and its value: `0x04` an int, `0x06` a double, `0x0c` a pointer (record
//! number), `0x13` a position, `0x14` a vector (3 doubles), strings (`0x07`, `0x0d`, `0x0e`:
//! one length byte) and the record type words (`0x0d`/`0x0e`, joined by `-`). The reader keeps
//! per record its type, pointers, ints, doubles, positions and vectors, and answers the
//! questions the feature reader asks: the loops of a body (as sampled polylines) and, for the
//! result body, the faces that carry a feature's blend tag with their neighbours.
//!
//! Input is untrusted: records, tokens, chains and walks are all bounded.

use std::collections::{BTreeMap, HashMap};

/// Most records one file may have.
const MAX_RECORDS: usize = 4_000_000;
/// Most tokens one file may have.
const MAX_TOKENS: usize = 40_000_000;
/// Samples per ellipse edge.
const ELLIPSE_SEGMENTS: usize = 48;
/// Longest chain of loops of one face followed.
const MAX_FACE_LOOPS: usize = 64;

pub(crate) type V3 = [f64; 3];

#[derive(Debug, Default, Clone)]
pub(crate) struct Rec {
    pub(crate) ty: String,
    /// The last `-` separated word of the type.
    last: String,
    pub(crate) refs: Vec<i64>,
    pub(crate) ints: Vec<i64>,
    pub(crate) dbls: Vec<f64>,
    pub(crate) pos: Vec<V3>,
    pub(crate) vec: Vec<V3>,
    /// Flag tokens (0x0a, 0x0b) in order: the senses (`forward`/`reversed`) and booleans.
    pub(crate) flags: Vec<u8>,
}

fn size_of(tag: u8) -> Option<usize> {
    Some(match tag {
        0x04 | 0x0c | 0x15 | 0x05 | 0x03 => 4,
        0x06 | 0x16 => 8,
        0x13 | 0x14 => 24,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 | 0x12 => 0,
        _ => return None,
    })
}

fn le_f64(b: &[u8]) -> f64 {
    let mut a = [0u8; 8];
    for (x, y) in a.iter_mut().zip(b) {
        *x = *y;
    }
    f64::from_le_bytes(a)
}

fn le_int(b: &[u8]) -> i64 {
    if b.len() >= 8 {
        let mut a = [0u8; 8];
        for (x, y) in a.iter_mut().zip(b) {
            *x = *y;
        }
        i64::from_le_bytes(a)
    } else {
        let mut a = [0u8; 4];
        for (x, y) in a.iter_mut().zip(b) {
            *x = *y;
        }
        i64::from(i32::from_le_bytes(a))
    }
}

fn vec3(b: &[u8]) -> V3 {
    [
        le_f64(b.get(..8).unwrap_or_default()),
        le_f64(b.get(8..16).unwrap_or_default()),
        le_f64(b.get(16..24).unwrap_or_default()),
    ]
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Splits `b` into records.
fn records(b: &[u8]) -> Result<Vec<Rec>, String> {
    let h = find(b, b"\x0d\x09asmheader").ok_or("the ShapeManager data has no header")?;
    // Files of the 64-bit layout write pointers and ints as 8 bytes.
    let wide = b.starts_with(b"ASM BinaryFile8");
    let width = |tag: u8| match tag {
        0x0c | 0x04 | 0x15 if wide => Some(8),
        t => size_of(t),
    };
    let mut i = h;
    let mut out: Vec<Rec> = Vec::new();
    let mut cur = Rec::default();
    let mut names: Vec<String> = Vec::new();
    let mut prev_name: Option<Vec<u8>> = None;
    // The first string of the record (the name of a custom attribute).
    let mut text: Option<Vec<u8>> = None;
    let mut ntok = 0usize;
    while i < b.len() {
        let t = *b.get(i).ok_or("cut short")?;
        i += 1;
        ntok += 1;
        if ntok > MAX_TOKENS {
            return Err("the ShapeManager data has too many tokens".into());
        }
        match t {
            0x07 | 0x0d | 0x0e => {
                let l = usize::from(*b.get(i).ok_or("cut short")?);
                let s = b.get(i + 1..i + 1 + l).ok_or("cut short")?;
                i += 1 + l;
                if s.starts_with(b"End-of-ASM-data")
                    || (t == 0x0d && s == b"data" && prev_name.as_deref() == Some(b"ASM"))
                {
                    return Ok(out);
                }
                if t != 0x07 && !s.is_empty() {
                    names.push(String::from_utf8_lossy(s).into_owned());
                }
                if t == 0x07 && text.is_none() {
                    text = Some(s.to_vec());
                }
                prev_name = (t == 0x0e).then(|| s.to_vec());
            }
            0x08 => {
                let l = b.get(i..i + 2).map_or(0, |x| le_int(x) as usize & 0xffff);
                i += 2 + l;
                prev_name = None;
            }
            0x09 => {
                let l = b.get(i..i + 4).map_or(0, |x| le_int(x) as usize & 0xffff_ffff);
                i = i.saturating_add(4).saturating_add(l);
                prev_name = None;
            }
            0x0a | 0x0b => {
                cur.flags.push(t);
                prev_name = None;
            }
            _ => {
                let n = width(t).ok_or("unknown token")?;
                let v = b.get(i..i + n).ok_or("cut short")?;
                i += n;
                prev_name = None;
                match t {
                    0x0c => cur.refs.push(le_int(v)),
                    0x04 => cur.ints.push(le_int(v)),
                    0x06 => cur.dbls.push(le_f64(v)),
                    0x13 => cur.pos.push(vec3(v)),
                    0x14 => cur.vec.push(vec3(v)),
                    _ => {}
                }
                if t == 0x11 {
                    let mut r = std::mem::take(&mut cur);
                    r.ty = names.join("-");
                    custom_attribute(&mut r, text.take().as_deref());
                    r.last = r.ty.rsplit('-').next().unwrap_or_default().to_string();
                    names.clear();
                    out.push(r);
                    if out.len() > MAX_RECORDS {
                        return Err("the ShapeManager data has too many records".into());
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Newer files (ShapeManager attribute version 44) write Inventor's naming attributes as
/// `ATTRIB_CUSTOM` records named by a string (`INV_NMX_BLEND_TAG`) instead of their own record
/// types (`NMx_blend_tag-...-attrib`), with one more int (1) before the version: `-1, 1, 44, 1,
/// feature id, -1, ...` against `-1, 43, 1, feature id, -1, ...`. Such a record is given the older
/// type name in front and loses that int, so the readers below see one layout. Only the names
/// read here are mapped.
fn custom_attribute(r: &mut Rec, text: Option<&[u8]>) {
    const MAP: [(&[u8], &str); 2] =
        [(b"INV_NMX_BLEND_TAG", "NMx_blend_tag"), (b"INV_NMX_EDGE_BLEND_TAG", "NMx_edge_blend_tag")];
    if !r.ty.starts_with("ATTRIB_CUSTOM") {
        return;
    }
    let Some(&(_, name)) = MAP.iter().find(|(k, _)| Some(*k) == text) else { return };
    if r.ints.get(1) == Some(&1) {
        r.ints.remove(1);
    }
    r.ty = format!("{name}-{}", r.ty);
}

pub(crate) struct Acis {
    pub(crate) recs: Vec<Rec>,
    /// Body tag (first int) -> record.
    bodies: HashMap<i64, usize>,
}

pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(crate) fn dot(a: V3, b: V3) -> f64 {
    // Summed from +0.0 (the prototype's `sum`), so a sum of negative zeros is +0.0: the sign of
    // a zero decides which side of -pi/pi an angle falls on.
    0.0 + a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(crate) fn dist(a: V3, b: V3) -> f64 {
    let d = sub(a, b);
    libm::sqrt(dot(d, d))
}
fn unit(a: V3) -> V3 {
    let l = libm::sqrt(dot(a, a));
    if l > 0.0 { [a[0] / l, a[1] / l, a[2] / l] } else { a }
}

impl Acis {
    /// Parses a ShapeManager binary file (`ASM BinaryFile...`).
    pub(crate) fn parse(blob: &[u8]) -> Result<Acis, String> {
        let recs = records(blob)?;
        let mut bodies = HashMap::new();
        for (k, r) in recs.iter().enumerate() {
            if r.last == "body"
                && !r.ty.ends_with("_body")
                && let Some(&t) = r.ints.first()
            {
                bodies.insert(t, k);
            }
        }
        Ok(Acis { recs, bodies })
    }

    fn rec(&self, k: i64) -> Option<&Rec> {
        usize::try_from(k).ok().and_then(|k| self.recs.get(k))
    }

    /// Record `k` is of the entity type `name` (`face`, not `NMx_face`).
    pub(crate) fn is(&self, k: i64, name: &str) -> bool {
        self.rec(k).is_some_and(|r| r.last == name && !r.ty.ends_with(&format!("_{name}")))
    }

    fn ends(&self, k: i64, suffix: &str) -> bool {
        self.rec(k).is_some_and(|r| r.ty.ends_with(suffix))
    }

    pub(crate) fn point_of_vertex(&self, v: i64) -> Option<V3> {
        let r = self.rec(v)?;
        r.refs.iter().find(|&&p| self.is(p, "point")).and_then(|&p| self.rec(p)?.pos.first().copied())
    }

    fn ellipse_eval(&self, c: i64, t: f64) -> Option<V3> {
        let r = self.rec(c)?;
        let cen = *r.pos.first()?;
        let n = unit(*r.vec.first()?);
        let maj = *r.vec.get(1)?;
        let ratio = r.dbls.first().copied().unwrap_or(1.0);
        let mn = cross(n, maj);
        Some(add(cen, add(mul(maj, libm::cos(t)), mul(mn, ratio * libm::sin(t)))))
    }

    /// Points along the curved edges of face `f` that run round the axis `ax` through `cen` at
    /// one radius and height (the arcs of a cone or cylinder face), in file units.
    fn arc_samples(&self, f: i64, cen: V3, ax: V3) -> Vec<V3> {
        let u = unit(ax);
        let mut out = Vec::new();
        for c in self.coedges(f) {
            let Some(e) = self.rec(c).and_then(|r| r.refs.iter().copied().find(|&y| self.is(y, "edge"))) else {
                continue;
            };
            let Some(pts) = self.edge_samples(e).filter(|p| p.len() > 2) else { continue };
            let rh: Vec<(f64, f64)> = pts.iter().map(|&p| Self::rh(cen, u, p)).collect();
            let Some(&(r0, h0)) = rh.first() else { continue };
            if rh.iter().all(|&(r, h)| (r - r0).abs() < 1e-6 && (h - h0).abs() < 1e-6) {
                out.extend(pts);
            }
        }
        out
    }

    /// An edge from its start vertex to its end vertex (straight edges: the two ends).
    fn edge_samples(&self, e: i64) -> Option<Vec<V3>> {
        let r = self.rec(e)?;
        let mut verts = r.refs.iter().filter(|&&x| self.is(x, "vertex"));
        let v0 = verts.next();
        let v1 = verts.next();
        let p0 = v0.and_then(|&v| self.point_of_vertex(v));
        let p1 = if v1.is_some() { v1.and_then(|&v| self.point_of_vertex(v)) } else { p0 };
        let curve = r.refs.iter().copied().find(|&x| self.ends(x, "curve"));
        let ends = || Some(vec![p0?, p1?]);
        let (Some(c), true) = (curve, r.dbls.len() >= 2) else { return ends() };
        let (t0, t1) = (*r.dbls.first()?, *r.dbls.get(1)?);
        if self.ends(c, "straight-curve") {
            return ends();
        }
        if !self.ends(c, "ellipse-curve") {
            return None;
        }
        let at = |t: f64| self.ellipse_eval(c, t);
        let q = at(t0);
        let fwd = matches!((q, p0), (Some(q), Some(p)) if dist(q, p) < 1e-6 * 1.0_f64.max((t1 - t0).abs()));
        if !fwd
            && let (Some(q2), Some(p)) = (at(-t0), p0)
            && dist(q2, p) < 1e-6
        {
            return (0..=ELLIPSE_SEGMENTS)
                .map(|k| at(-(t0 + (t1 - t0) * k as f64 / ELLIPSE_SEGMENTS as f64)))
                .collect();
        }
        (0..=ELLIPSE_SEGMENTS).map(|k| at(t0 + (t1 - t0) * k as f64 / ELLIPSE_SEGMENTS as f64)).collect()
    }

    /// Every loop (face loop or wire) of the body with this tag, as points (file units).
    /// `None`: no such body, or an edge that cannot be sampled.
    pub(crate) fn loops_of_body(&self, tag: i64) -> Option<Vec<Vec<V3>>> {
        let b = *self.bodies.get(&tag)?;
        let mut seen = vec![false; self.recs.len()];
        let mut st = vec![b];
        if let Some(s) = seen.get_mut(b) {
            *s = true;
        }
        while let Some(x) = st.pop() {
            for &p in &self.recs.get(x)?.refs {
                let Some(pu) = usize::try_from(p).ok().filter(|&u| u < self.recs.len()) else { continue };
                if seen.get(pu) == Some(&false) && !self.is(p, "body") && !self.recs.get(pu)?.ty.contains("attrib") {
                    if let Some(s) = seen.get_mut(pu) {
                        *s = true;
                    }
                    st.push(pu);
                }
            }
        }
        let mut own: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
        for (k, r) in self.recs.iter().enumerate() {
            if !seen.get(k).copied().unwrap_or(false) || r.last != "coedge" || r.ty.ends_with("_coedge") {
                continue;
            }
            let owner = r.refs.iter().copied().find(|&x| self.is(x, "loop") || self.is(x, "wire")).unwrap_or(-1);
            own.entry(owner).or_default().push(k);
        }
        let mut loops = Vec::new();
        for cs in own.values() {
            let start = *cs.first()?;
            let mut cur = start;
            let mut order = vec![cur];
            loop {
                let nxt = self.recs.get(cur)?.refs.get(3).copied().unwrap_or(-1);
                let Some(nu) = usize::try_from(nxt).ok().filter(|u| cs.binary_search(u).is_ok()) else { break };
                if nu == start || order.len() > cs.len() {
                    break;
                }
                cur = nu;
                order.push(cur);
            }
            let mut segs: Vec<Vec<V3>> = Vec::new();
            for ce in order {
                let Some(edge) = self.recs.get(ce)?.refs.iter().copied().find(|&x| self.is(x, "edge")) else {
                    continue;
                };
                segs.push(self.edge_samples(edge)?);
            }
            let mut pts: Vec<V3> = Vec::new();
            let first_len = segs.first().map_or(0, Vec::len);
            for mut sm in segs {
                if pts.is_empty() {
                    pts = sm;
                    continue;
                }
                let (s0, s1) = (*sm.first()?, *sm.last()?);
                if pts.len() == first_len {
                    let (p0, pl) = (*pts.first()?, *pts.last()?);
                    if dist(p0, s0).min(dist(p0, s1)) < dist(pl, s0).min(dist(pl, s1)) {
                        pts.reverse();
                    }
                }
                if dist(*pts.last()?, s0) > dist(*pts.last()?, s1) {
                    sm.reverse();
                }
                pts.extend(sm.into_iter().skip(1));
            }
            loops.push(pts);
        }
        Some(loops)
    }
}

// --- the result body: blend faces -------------------------------------------------------------

/// A full round as read from its cylinder face (file units).
#[derive(Debug, Clone, Copy)]
pub(crate) struct FullRound {
    /// The crest of the half cylinder: where it touches the middle face it replaced.
    pub(crate) point: V3,
    /// The outward normal of that middle face.
    pub(crate) normal: V3,
    /// A slot's end (the supports face the axis) rather than a tab's.
    pub(crate) concave: bool,
    /// A point on the cylinder axis, the axis direction, the direction from the axis to the
    /// crest, the radius, and the axial extent of the cylinder face about the point.
    pub(crate) axis: V3,
    pub(crate) dir: V3,
    pub(crate) out: V3,
    pub(crate) radius: f64,
    pub(crate) span: (f64, f64),
}

/// A surface as far as blend analysis needs it.
#[derive(Debug, Clone)]
pub(crate) enum Surf {
    Plane { p: V3, n: V3 },
    Cyl { c: V3, ax: V3, r: f64 },
    Cone,
    Torus { c: V3, ax: V3, major: f64, minor: f64 },
    Sphere,
    Other(String),
    None,
}

impl Acis {
    fn surf_rec(&self, f: i64) -> Option<usize> {
        self.rec(f)?
            .refs
            .iter()
            .copied()
            .find(|&y| self.ends(y, "surface") || self.is_spline_surface(y))
            .and_then(|y| usize::try_from(y).ok())
    }

    /// A spline surface record: its type name carries the whole description of the surface
    /// (`spline-surface-rb_blend_spl_sur-cone-...`), so it does not end in `surface`.
    fn is_spline_surface(&self, k: i64) -> bool {
        self.rec(k).is_some_and(|r| r.ty.starts_with("spline-surface-"))
    }

    pub(crate) fn kind(&self, f: i64) -> Surf {
        let Some(s) = self.surf_rec(f) else { return Surf::None };
        let Some(mut r) = self.recs.get(s) else { return Surf::None };
        // A `spline-surface-ref` is a pointer to the spline surface record that describes it.
        if r.ty == "spline-surface-ref"
            && let Some(q) = r.refs.iter().filter_map(|&y| self.rec(y)).find(|q| q.ty.starts_with("spline-surface-"))
        {
            r = q;
        }
        let t = r.ty.as_str();
        let (p0, v0) = (r.pos.first().copied(), r.vec.first().copied());
        if t.ends_with("plane-surface") {
            return match (p0, v0) {
                (Some(p), Some(n)) => Surf::Plane { p, n: unit(n) },
                _ => Surf::None,
            };
        }
        if t.ends_with("cone-surface") {
            if r.dbls.len() >= 3
                && r.dbls.get(1).is_some_and(|d| d.abs() < 1e-12)
                && let (Some(c), Some(a), Some(m)) = (p0, v0, r.vec.get(1))
            {
                return Surf::Cyl { c, ax: unit(a), r: libm::sqrt(dot(*m, *m)) };
            }
            return Surf::Cone;
        }
        if t.ends_with("torus-surface") {
            return match (r.dbls.first(), r.dbls.get(1), p0, v0) {
                (Some(&a), Some(&b), Some(c), Some(ax)) => Surf::Torus { c, ax: unit(ax), major: a, minor: b.abs() },
                _ => Surf::None,
            };
        }
        if t == "sphere-surface" {
            return Surf::Sphere;
        }
        if let Some(rest) = t.strip_prefix("spline-surface-") {
            // The kind of spline surface: `srf_srf_v_bl_spl_sur` (a vertex blend, the patch where
            // blends meet), `rb_blend_spl_sur` (a rolling ball blend), ...
            return Surf::Other(rest.split('-').next().unwrap_or(rest).to_string());
        }
        Surf::Other(t.to_string())
    }

    fn coedges(&self, f: i64) -> Vec<i64> {
        let Some(r) = self.rec(f) else { return Vec::new() };
        let mut loops: Vec<i64> = r.refs.iter().copied().filter(|&y| self.is(y, "loop")).collect();
        while !loops.is_empty() && loops.len() < MAX_FACE_LOOPS {
            let Some(last) = loops.last().and_then(|&l| self.rec(l)) else { break };
            let nx = last.refs.iter().take(3).copied().find(|&y| self.is(y, "loop") && !loops.contains(&y));
            match nx {
                Some(n) => loops.push(n),
                None => break,
            }
        }
        let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut out = Vec::new();
        for l in loops {
            let Some(lr) = self.rec(l) else { continue };
            let mut st: Vec<i64> = lr.refs.iter().copied().filter(|&y| self.is(y, "coedge")).collect();
            while let Some(c) = st.pop() {
                if !seen.insert(c) {
                    continue;
                }
                out.push(c);
                if let Some(cr) = self.rec(c) {
                    st.extend(cr.refs.iter().skip(2).take(2).copied().filter(|&y| self.is(y, "coedge")));
                }
            }
        }
        out
    }

    /// The faces across each coedge of a face, with the edge between them.
    pub(crate) fn neighbours(&self, f: i64) -> Vec<(i64, Option<i64>)> {
        let mut out = Vec::new();
        for c in self.coedges(f) {
            let Some(p) = self.rec(c).and_then(|r| r.refs.get(4).copied()).filter(|&p| self.is(p, "coedge")) else {
                continue;
            };
            let Some(l) = self.rec(p).and_then(|r| r.refs.iter().copied().find(|&y| self.is(y, "loop"))) else {
                continue;
            };
            if let Some(g) = self.rec(l).and_then(|r| r.refs.iter().copied().find(|&y| self.is(y, "face"))) {
                let e = self.rec(c).and_then(|r| r.refs.iter().copied().find(|&y| self.is(y, "edge")));
                out.push((g, e));
            }
        }
        out
    }

    /// The points of an edge's vertices.
    fn edge_points(&self, e: i64) -> Vec<V3> {
        let Some(r) = self.rec(e) else { return Vec::new() };
        r.refs.iter().filter(|&&v| self.is(v, "vertex")).filter_map(|&v| self.point_of_vertex(v)).collect()
    }

    /// A point on the axis and the unit direction of a face of revolution (cone, cylinder or
    /// torus surface).
    fn rev_axis(&self, f: i64) -> Option<(V3, V3)> {
        let r = self.recs.get(self.surf_rec(f)?)?;
        (r.ty.ends_with("cone-surface") || r.ty.ends_with("torus-surface"))
            .then(|| Some((*r.pos.first()?, unit(*r.vec.first()?))))
            .flatten()
    }

    pub(crate) fn verts(&self, f: i64) -> Vec<V3> {
        let mut pts = Vec::new();
        for c in self.coedges(f) {
            let Some(cr) = self.rec(c) else { continue };
            for &e in cr.refs.iter().filter(|&&e| self.is(e, "edge")) {
                for &v in self.rec(e).map(|r| r.refs.as_slice()).unwrap_or_default() {
                    if self.is(v, "vertex")
                        && let Some(p) = self.point_of_vertex(v)
                    {
                        pts.push(p);
                    }
                }
            }
        }
        pts
    }

    /// A full round: the cylinder face `f` is tangent to two parallel planar faces (apart by
    /// twice its radius, on opposite sides of its axis) and replaces the face between them.
    /// Returns a point on that middle face (the crest of the half cylinder, at the middle of
    /// its extent along the axis) and the face's outward normal, in file units. `None` when
    /// `f` is not such a blend.
    pub(crate) fn full_round(&self, f: i64) -> Option<FullRound> {
        let Surf::Cyl { c, ax, r } = self.kind(f) else { return None };
        let u = unit(ax);
        let mut sup: Vec<(i64, V3, f64, f64)> = Vec::new();
        for (g, _) in self.neighbours(f) {
            if sup.iter().any(|x| x.0 == g) {
                continue;
            }
            if let Surf::Plane { p, n } = self.kind(g)
                && dot(n, u).abs() < 1e-6
                && (dot(sub(p, c), n).abs() - r).abs() < 1e-6
            {
                // The outward normal: a reversed face (first flag) faces against its surface.
                let sgn = if self.rec(g).and_then(|r| r.flags.first()) == Some(&0x0a) { -1.0 } else { 1.0 };
                sup.push((g, n, dot(sub(p, c), n), sgn));
            }
        }
        let [a, b] = sup.as_slice() else { return None };
        // Parallel, with the axis between them.
        if (dot(a.1, b.1).abs() - 1.0).abs() > 1e-6 || (dot(a.1, b.1) * a.2 * b.2) >= 0.0 {
            return None;
        }
        let m = unit(cross(u, a.1));
        // The body lies behind the crest: the supports' vertices are mostly on its side.
        let vs: Vec<f64> = [a.0, b.0].iter().flat_map(|&g| self.verts(g)).map(|v| dot(sub(v, c), m)).collect();
        let hi = vs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let lo = vs.iter().copied().fold(f64::INFINITY, f64::min);
        if !hi.is_finite() || !lo.is_finite() || (hi + lo).abs() < 1e-6 {
            return None;
        }
        let out = if hi.abs() > lo.abs() { mul(m, -1.0) } else { m };
        let ts: Vec<f64> = self.verts(f).iter().map(|&v| dot(sub(v, c), u)).collect();
        let t = (ts.iter().copied().fold(f64::INFINITY, f64::min)
            + ts.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            / 2.0;
        if !t.is_finite() {
            return None;
        }
        // A convex round (a tab's end) has supports whose outward normals point away from the
        // axis; a concave one (a slot's end) has them pointing at it, and the middle face it
        // replaced then faces the other way than the crest does.
        let away = (a.3 * a.2 > 0.0, b.3 * b.2 > 0.0);
        if away.0 != away.1 {
            return None;
        }
        let normal = if away.0 { out } else { mul(out, -1.0) };
        let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Some(FullRound {
            point: add(c, add(mul(u, t), mul(out, r))),
            normal,
            concave: !away.0,
            axis: c,
            dir: u,
            out,
            radius: r,
            span: (lo, hi),
        })
    }

    /// The planar faces (point, unit normal; file units) tagged `NMx_Brep_Feature_tag` by
    /// the feature with id `fid`: what a thicken made.
    pub(crate) fn feature_planes(&self, fid: i64) -> Vec<(V3, V3)> {
        let mut out = Vec::new();
        for r in &self.recs {
            if r.ty.starts_with("NMx_Brep_Feature_tag")
                && r.ints.get(3) == Some(&fid)
                && let Some(&f) = r.refs.iter().find(|&&y| self.is(y, "face"))
                && let Surf::Plane { p, n } = self.kind(f)
            {
                out.push((p, n));
            }
        }
        out
    }

    /// A planar face of a chamfer that meets at least two other faces of the same feature and has
    /// three corners: the patch where chamfers of a straight and a curved edge meet at a vertex.
    /// It replaces no edge of its own (the faces around it carry the edges).
    pub(crate) fn is_chamfer_corner_patch(&self, f: i64, feature_faces: &[i64]) -> bool {
        if !matches!(self.kind(f), Surf::Plane { .. }) {
            return false;
        }
        let mut corners: Vec<V3> = Vec::new();
        for v in self.verts(f) {
            if corners.iter().all(|c| dist(*c, v) > 1e-7) {
                corners.push(v);
            }
        }
        let mut near: Vec<i64> = self.neighbours(f).into_iter().map(|x| x.0).filter(|g| *g != f).collect();
        near.sort_unstable();
        near.dedup();
        corners.len() == 3 && near.iter().filter(|g| feature_faces.contains(g)).count() >= 2
    }

    /// Faces that carry an `NMx_blend_tag` (or `NMx_edge_blend_tag`, which the chamfers of
    /// some circle edges use) attribute whose 4th int is `fid`.
    pub(crate) fn blend_faces(&self, fid: i64) -> Vec<i64> {
        let mut out = Vec::new();
        for r in &self.recs {
            if (r.ty.starts_with("NMx_blend_tag") || r.ty.starts_with("NMx_edge_blend_tag"))
                && r.ints.get(3) == Some(&fid)
                && let Some(&f) = r.refs.iter().find(|&&y| self.is(y, "face"))
            {
                out.push(f);
            }
        }
        out
    }
}

fn line_of_planes(p1: V3, n1: V3, p2: V3, n2: V3, near: V3) -> Option<(V3, V3)> {
    let u = cross(n1, n2);
    let l = libm::sqrt(dot(u, u));
    if l < 1e-6 {
        return None;
    }
    let u = mul(u, 1.0 / l);
    let (d1, d2) = (dot(n1, p1), dot(n2, p2));
    let g = dot(n1, n2);
    let det = 1.0 - g * g;
    let (r1, r2) = (d1 - dot(n1, near), d2 - dot(n2, near));
    let a = (r1 - g * r2) / det;
    let b = (r2 - g * r1) / det;
    Some((add(near, add(mul(n1, a), mul(n2, b))), u))
}

/// Where a plane meets a cylinder whose axis is parallel to it: a line along the axis; of the
/// two, the one nearest `near`.
fn line_of_plane_cyl(p: V3, n: V3, c: V3, ax: V3, r: f64, near: V3) -> Option<(V3, V3)> {
    let (n, u) = (unit(n), unit(ax));
    if dot(n, u).abs() > 1e-6 {
        return None;
    }
    let d = dot(sub(c, p), n);
    if d.abs() > r + 1e-9 {
        return None;
    }
    let h = libm::sqrt((r * r - d * d).max(0.0));
    let foot = sub(c, mul(n, d));
    let w = unit(cross(n, u));
    let (a, b) = (add(foot, mul(w, h)), sub(foot, mul(w, h)));
    Some((if dist(a, near) <= dist(b, near) { a } else { b }, u))
}

fn round_to(x: f64, digits: i32) -> f64 {
    let s = libm::pow(10.0, f64::from(digits));
    libm::round(x * s) / s
}

/// Where Gusset puts the seam of a circle with this normal (found by experiment): the midpoint
/// of a full circle is centre - R * ref.
fn ref_dir(n: V3) -> V3 {
    let n = unit(n);
    let x = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    unit(sub(x, mul(n, dot(x, n))))
}

/// The edge a blend face replaced, as far as a pick on it needs: a straight edge's direction,
/// or a circle (a full one has its midpoint at the seam).
#[derive(Debug, Clone, Copy)]
pub(crate) enum EdgeInfo {
    Line { dir: V3 },
    Circle { cen: V3, ax: V3, r: f64 },
}

/// The vertices are two points on opposite sides of the axis: the arc between them is half a
/// circle, but which half is not known from them.
fn is_half_circle(cen: V3, ax: V3, vs: &[V3]) -> bool {
    let u = unit(ax);
    let mut dirs: Vec<V3> = Vec::new();
    for &v in vs {
        let q = sub(v, cen);
        let q = sub(q, mul(u, dot(q, u)));
        if libm::sqrt(dot(q, q)) > 1e-9 && dirs.iter().all(|p| dist(*p, unit(q)) > 1e-6) {
            dirs.push(unit(q));
        }
    }
    let [a, b] = dirs.as_slice() else { return false };
    dot(*a, *b) < -1.0 + 1e-9
}

/// The midpoint of the arc the vertices `vs` bound on a circle; of a whole circle (fewer than
/// two distinct vertex angles) the seam, where the model has the midpoint of a full circle.
fn circ_mid(cen: V3, ax: V3, rad: f64, vs: &[V3]) -> V3 {
    let rf = ref_dir(ax);
    let oth = cross(unit(ax), rf);
    let tau = 2.0 * std::f64::consts::PI;
    let mut angs: Vec<f64> = Vec::new();
    for &v in vs {
        let q = sub(v, cen);
        let q = sub(q, mul(ax, dot(q, ax)));
        if libm::sqrt(dot(q, q)) > 1e-9 {
            // atan2 gives -pi and +pi for one angle: into [0, tau) before comparing.
            let a = libm::atan2(dot(q, oth), dot(q, rf));
            let a = libm::fmod(a, tau);
            angs.push(if a < 0.0 { a + tau } else { a });
        }
    }
    angs.sort_by(f64::total_cmp);
    let mut ded: Vec<f64> = Vec::new();
    for a in angs {
        if ded.last().is_none_or(|&l| a - l > 1e-6) {
            ded.push(a);
        }
    }
    if ded.len() > 1
        && let (Some(&f), Some(&l)) = (ded.first(), ded.last())
        && f + tau - l < 1e-6
    {
        ded.pop();
    }
    let angs = ded;
    if angs.len() < 2 {
        return sub(cen, mul(rf, rad));
    }
    // The arc is the complement of the largest gap between vertex angles.
    let n = angs.len();
    let gaps: Vec<f64> = (0..n)
        .map(|i| {
            let a = angs.get((i + 1) % n).copied().unwrap_or(0.0) - angs.get(i).copied().unwrap_or(0.0);
            // Python's float modulo: fmod, then into [0, tau).
            let r = libm::fmod(a, tau);
            if r < 0.0 { r + tau } else { r }
        })
        .collect();
    let mut best = 0usize;
    for (j, g) in gaps.iter().enumerate() {
        if *g > gaps.get(best).copied().unwrap_or(0.0) {
            best = j;
        }
    }
    let a0 = angs.get((best + 1) % n).copied().unwrap_or(0.0);
    let span = tau - gaps.get(best).copied().unwrap_or(0.0);
    let am = a0 + span / 2.0;
    add(cen, add(mul(rf, rad * libm::cos(am)), mul(oth, rad * libm::sin(am))))
}

/// A curve in a plane: a line `n . x = d` or a circle.
#[derive(Debug, Clone, Copy)]
enum Curve2 {
    Line { n: (f64, f64), d: f64 },
    Circ { c: (f64, f64), r: f64 },
}

/// The intersections of two plane curves, nearest to `o` first.
fn inter2(s1: Curve2, s2: Curve2, o: (f64, f64)) -> Vec<(f64, f64)> {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    match (s1, s2) {
        (Curve2::Line { n: n1, d: d1 }, Curve2::Line { n: n2, d: d2 }) => {
            let det = n1.0 * n2.1 - n1.1 * n2.0;
            if det.abs() < 1e-9 {
                return Vec::new();
            }
            pts.push(((d1 * n2.1 - d2 * n1.1) / det, (n1.0 * d2 - n2.0 * d1) / det));
        }
        (Curve2::Line { n, d }, Curve2::Circ { c, r }) | (Curve2::Circ { c, r }, Curve2::Line { n, d }) => {
            let h = n.0 * c.0 + n.1 * c.1 - d;
            if h.abs() > r + 1e-9 {
                return Vec::new();
            }
            let t = libm::sqrt((r * r - h * h).max(0.0));
            let foot = (c.0 - n.0 * h, c.1 - n.1 * h);
            let tg = (-n.1, n.0);
            pts.push((foot.0 + tg.0 * t, foot.1 + tg.1 * t));
            pts.push((foot.0 - tg.0 * t, foot.1 - tg.1 * t));
        }
        (Curve2::Circ { c: c1, r: r1 }, Curve2::Circ { c: c2, r: r2 }) => {
            let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
            let dd = libm::hypot(dx, dy);
            if dd < 1e-9 || dd > r1 + r2 + 1e-9 || dd < (r1 - r2).abs() - 1e-9 {
                return Vec::new();
            }
            let a = (r1 * r1 - r2 * r2 + dd * dd) / (2.0 * dd);
            let h = libm::sqrt((r1 * r1 - a * a).max(0.0));
            let (mx, my) = (c1.0 + dx * a / dd, c1.1 + dy * a / dd);
            pts.push((mx - dy * h / dd, my + dx * h / dd));
            pts.push((mx + dy * h / dd, my - dx * h / dd));
        }
    }
    pts.sort_by(|p, q| libm::hypot(p.0 - o.0, p.1 - o.1).total_cmp(&libm::hypot(q.0 - o.0, q.1 - o.1)));
    pts
}

/// Two unit vectors perpendicular to `u` and to each other.
fn basis(u: V3) -> (V3, V3) {
    let a = if u[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let e1 = unit(cross(u, a));
    (e1, cross(u, e1))
}

/// The straight edge (a point on it and its direction) a cylindrical blend replaced: the two
/// support faces beside the blend (planes parallel to its axis, cylinders parallel to it) are
/// tangent to the blend circle; the edge is where they meet in the cross-section.
fn cyl_edge(nb: &[(i64, Surf)], c: V3, ax: V3, rad: f64) -> Result<(V3, V3), String> {
    let u = unit(ax);
    let (e1, e2) = basis(u);
    let p2 = |q: V3| (dot(sub(q, c), e1), dot(sub(q, c), e2));
    let mut sup: Vec<(i64, Curve2)> = Vec::new();
    for (g, s) in nb {
        if sup.iter().any(|x| x.0 == *g) {
            continue;
        }
        match s {
            Surf::Plane { p, n } => {
                if dot(*n, u).abs() > 1e-6 || (dot(sub(*p, c), *n).abs() - rad).abs() >= 1e-6 {
                    continue;
                }
                let n2 = (dot(*n, e1), dot(*n, e2));
                let q = p2(*p);
                sup.push((*g, Curve2::Line { n: n2, d: n2.0 * q.0 + n2.1 * q.1 }));
            }
            Surf::Cyl { c: cp, ax: n, r } => {
                if (dot(*n, u).abs() - 1.0).abs() > 1e-6 {
                    continue;
                }
                let c2 = p2(*cp);
                let dist = libm::hypot(c2.0, c2.1);
                let tangent = ((dist - (r + rad)).abs()).min((dist - (r - rad).abs()).abs()) < 1e-6;
                if tangent && dist > 1e-9 {
                    sup.push((*g, Curve2::Circ { c: c2, r: *r }));
                }
            }
            _ => {}
        }
    }
    let [a, b] = sup.as_slice() else {
        return Err(format!("cylinder blend with {} usable supports", sup.len()));
    };
    let q = *inter2(a.1, b.1, (0.0, 0.0)).first().ok_or("supports do not meet")?;
    Ok((add(c, add(mul(e1, q.0), mul(e2, q.1))), u))
}

/// A cylindrical blend that rolled between a plane and a convex cylinder that do not meet (a
/// middle face between them was consumed entirely, as in a full round, but not parallel to
/// either): file units.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BridgedRound {
    /// The blend's axis point, direction and radius.
    pub(crate) axis: V3,
    pub(crate) dir: V3,
    pub(crate) radius: f64,
    /// The direction from the blend axis to its contact line on the plane (into the material
    /// beyond the plane), and to the axis of the cylinder it touches.
    pub(crate) to_plane: V3,
    pub(crate) to_cyl: V3,
    /// The axial extent of the blend face about `axis`.
    pub(crate) span: (f64, f64),
}

impl Acis {
    /// Reads a cylinder blend whose two supports (a plane parallel to the axis and a cylinder
    /// about a parallel axis, touching the blend externally) have no common edge.
    pub(crate) fn bridged_round(&self, f: i64) -> Option<BridgedRound> {
        let Surf::Cyl { c, ax, r: rad } = self.kind(f) else { return None };
        let u = unit(ax);
        let nb: Vec<(i64, Surf)> = self.neighbours(f).into_iter().map(|(g, _)| (g, self.kind(g))).collect();
        let mut plane: Option<(i64, V3)> = None;
        let mut cyl: Option<(i64, V3, f64)> = None;
        for (g, s) in &nb {
            match s {
                Surf::Plane { p, n } if dot(*n, u).abs() < 1e-6 && (dot(sub(*p, c), *n).abs() - rad).abs() < 1e-6 => {
                    if plane.is_some_and(|x| x.0 != *g) {
                        return None;
                    }
                    let side = if dot(sub(*p, c), *n) > 0.0 { 1.0 } else { -1.0 };
                    plane = Some((*g, mul(*n, side)));
                }
                Surf::Cyl { c: cp, ax: n, r } if (dot(*n, u).abs() - 1.0).abs() < 1e-6 => {
                    let q = sub(*cp, c);
                    let q = sub(q, mul(u, dot(q, u)));
                    let d = libm::sqrt(dot(q, q));
                    if (d - (r + rad)).abs() < 1e-6 {
                        if cyl.is_some_and(|x| x.0 != *g) {
                            return None;
                        }
                        cyl = Some((*g, q, *r));
                    }
                }
                _ => {}
            }
        }
        let ((_, to_plane), (_, q, cr)) = (plane?, cyl?);
        // They must really be disjoint: the plane is further from the cylinder's axis than its radius.
        let gap = rad - dot(q, to_plane);
        if gap.abs() <= cr + 1e-6 {
            return None;
        }
        let ts: Vec<f64> = self.verts(f).iter().map(|&v| dot(sub(v, c), u)).collect();
        let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !lo.is_finite() || !hi.is_finite() || hi - lo < 1e-9 {
            return None;
        }
        Some(BridgedRound { axis: c, dir: u, radius: rad, to_plane, to_cyl: unit(q), span: (lo, hi) })
    }
}

impl Acis {
    /// (distance from the axis, height along it) of a point.
    fn rh(o: V3, ax: V3, p: V3) -> (f64, f64) {
        let q = sub(p, o);
        let h = dot(q, ax);
        (libm::sqrt((dot(q, q) - h * h).max(0.0)), h)
    }

    fn profile_curve(&self, g: i64, o: V3, ax: V3, kind: &Surf) -> Option<Curve2> {
        let coaxial = |a: V3| (dot(a, ax).abs() - 1.0).abs() <= 1e-6;
        match kind {
            Surf::Plane { p, n } => coaxial(*n).then(|| Curve2::Line { n: (0.0, 1.0), d: Self::rh(o, ax, *p).1 }),
            Surf::Cyl { c, ax: u, r } => {
                (coaxial(*u) && Self::rh(o, ax, *c).0 <= 1e-6).then_some(Curve2::Line { n: (1.0, 0.0), d: *r })
            }
            Surf::Torus { c, ax: u, major, minor } => {
                coaxial(*u).then(|| Curve2::Circ { c: (*major, Self::rh(o, ax, *c).1), r: *minor })
            }
            Surf::Cone => {
                let mut pts: Vec<(f64, f64)> = Vec::new();
                for p in self.verts(g) {
                    let c = Self::rh(o, ax, p);
                    if pts.iter().all(|q| libm::hypot(c.0 - q.0, c.1 - q.1) > 1e-6) {
                        pts.push(c);
                    }
                }
                let a = *pts.first()?;
                let mut b = *pts.get(1)?;
                for &q in pts.iter().skip(1) {
                    if libm::hypot(q.0 - a.0, q.1 - a.1) > libm::hypot(b.0 - a.0, b.1 - a.1) {
                        b = q;
                    }
                }
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let l = libm::hypot(dx, dy);
                let n = (-dy / l, dx / l);
                if pts.iter().any(|q| (n.0 * (q.0 - a.0) + n.1 * (q.1 - a.1)).abs() > 1e-6) {
                    return None;
                }
                Some(Curve2::Line { n, d: n.0 * a.0 + n.1 * a.1 })
            }
            _ => None,
        }
    }

    /// The circle a blend face replaced, from its two long-side neighbours (coaxial surfaces of
    /// revolution, or planes across the axis), whatever the blend face's own surface is:
    /// (centre, axis, radius).
    fn circle_edge(&self, f: i64) -> Result<(V3, V3, f64), String> {
        let nb: Vec<(i64, i64, Surf)> = self
            .neighbours(f)
            .into_iter()
            .filter_map(|(g, e)| (g != f).then_some((g, e?)))
            .map(|(g, e)| (g, e, self.kind(g)))
            .collect();
        let (o, ax) = std::iter::once(f)
            .chain(nb.iter().map(|x| x.0))
            .find_map(|g| self.rev_axis(g))
            .ok_or("no axis of revolution")?;
        let vs = self.verts(f);
        if vs.is_empty() {
            return Err("no vertices".into());
        }
        let k = 1.0 / vs.len() as f64;
        let mean = vs.iter().fold((0.0, 0.0), |a, &v| {
            let c = Self::rh(o, ax, v);
            (a.0 + c.0, a.1 + c.1)
        });
        let mean = (mean.0 * k, mean.1 * k);
        let mut sup: Vec<(i64, Curve2)> = Vec::new();
        for (g, e, kd) in &nb {
            let cs: Vec<(f64, f64)> = self.edge_points(*e).into_iter().map(|p| Self::rh(o, ax, p)).collect();
            let Some(c0) = cs.first() else { continue };
            // An end edge, not a long side.
            if cs.iter().any(|c| (c.0 - c0.0).abs() > 1e-5 || (c.1 - c0.1).abs() > 1e-5) {
                continue;
            }
            if matches!(kd, Surf::None) {
                continue;
            }
            let Some(pc) = self.profile_curve(*g, o, ax, kd) else { continue };
            match sup.iter_mut().find(|x| x.0 == *g) {
                Some(x) => x.1 = pc,
                None => sup.push((*g, pc)),
            }
        }
        let [a, b] = sup.as_slice() else {
            return Err(format!("{} coaxial long-side neighbours", sup.len()));
        };
        let pts = inter2(a.1, b.1, mean);
        let (rho, h) = *pts.first().ok_or("supports do not meet in the half-plane")?;
        if rho < 1e-9 {
            return Err("edge on the axis".into());
        }
        Ok((add(o, mul(ax, h)), ax, rho))
    }
}

impl Acis {
    /// The midpoint (file units) of the edge the blend face `f` replaced, with the edge's
    /// tangent information, or why not.
    pub(crate) fn edge_mid(&self, f: i64) -> Result<(V3, EdgeInfo), String> {
        self.edge_mid_of(f)
    }

    fn edge_mid_of(&self, f: i64) -> Result<(V3, EdgeInfo), String> {
        let kind = self.kind(f);
        let nb: Vec<(i64, Surf)> = self.neighbours(f).into_iter().map(|(g, _)| (g, self.kind(g))).collect();
        let mut planes: Vec<(i64, V3, V3)> = Vec::new();
        for (g, s) in &nb {
            if let Surf::Plane { p, n } = s {
                match planes.iter_mut().find(|x| x.0 == *g) {
                    Some(x) => *x = (*g, *p, *n),
                    None => planes.push((*g, *p, *n)),
                }
            }
        }
        let vs = self.verts(f);
        if vs.is_empty() {
            return Err("no vertices".into());
        }
        let circle = |cen: V3, ax: V3, r: f64| {
            // A half circle has its two ends diametrically opposite: which half is not in the
            // vertices, so the face's own arcs say.
            let mut pts = vs.clone();
            if is_half_circle(cen, ax, &vs) {
                pts.extend(self.arc_samples(f, cen, ax));
            }
            (circ_mid(cen, ax, r, &pts), EdgeInfo::Circle { cen, ax, r })
        };
        let (p, u);
        match &kind {
            Surf::Cyl { c, ax, r } => (p, u) = cyl_edge(&nb, *c, *ax, *r)?,
            Surf::Plane { n: nc, .. } => {
                let k = 1.0 / vs.len() as f64;
                let cen = mul(vs.iter().fold([0.0; 3], |a, &v| add(a, v)), k);
                let mut best: Option<(f64, (V3, V3))> = None;
                let mut consider = |l: (V3, V3)| {
                    let dd = dist(l.0, cen);
                    if dot(l.1, *nc).abs() <= 1e-6 && best.as_ref().is_none_or(|x| dd < x.0) {
                        best = Some((dd, l));
                    }
                };
                for (i, a) in planes.iter().enumerate() {
                    for b in planes.iter().skip(i + 1) {
                        if let Some(l) = line_of_planes(a.1, a.2, b.1, b.2, cen) {
                            consider(l);
                        }
                    }
                    // A chamfer between a plane and a cylinder about an axis parallel to it.
                    for (_, s) in &nb {
                        if let Surf::Cyl { c, ax, r } = s
                            && let Some(l) = line_of_plane_cyl(a.1, a.2, *c, *ax, *r, cen)
                        {
                            consider(l);
                        }
                    }
                }
                (p, u) = best.ok_or("no support pair")?.1;
            }
            other => {
                // A circle edge: from the coaxial supports, else (torus, cone) from the corner.
                let ce = self.circle_edge(f);
                if let Ok((cen, ax, r)) = ce {
                    return Ok(circle(cen, ax, r));
                }
                let why = ce.err().unwrap_or_default();
                let (major, what) = match other {
                    Surf::Torus { major, .. } => (Some(*major), "torus"),
                    Surf::Cone => (None, "cone"),
                    Surf::Sphere => return Err("blend surface sphere-surface".into()),
                    Surf::Other(t) => return Err(format!("blend surface {t} ({why})")),
                    _ => return Err(format!("blend surface None ({why})")),
                };
                let (cen, ax, r) = self.corner_circle(f, &nb, &vs, major, what)?;
                return Ok(circle(cen, ax, r));
            }
        }
        let ts: Vec<f64> = vs.iter().map(|&v| dot(sub(v, p), u)).collect();
        let mut lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
        let mut hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mut ends: Vec<f64> = Vec::new();
        for (_, s) in &nb {
            if let Surf::Plane { p: pp, n } = s
                && dot(*n, u).abs() > 1e-3
            {
                ends.push(dot(sub(*pp, p), *n) / dot(u, *n));
            }
        }
        let span = hi - lo;
        let mid = (lo + hi) / 2.0;
        let closest = |c: Vec<f64>, to: f64| {
            let mut best: Option<f64> = None;
            for t in c {
                if best.is_none_or(|b| (t - to).abs() < (b - to).abs()) {
                    best = Some(t);
                }
            }
            best
        };
        let near_lo: Vec<f64> =
            ends.iter().copied().filter(|t| (t - lo).abs() < 0.5 * span + 1e-9 && *t <= mid).collect();
        let near_hi: Vec<f64> =
            ends.iter().copied().filter(|t| (t - hi).abs() < 0.5 * span + 1e-9 && *t >= mid).collect();
        if let Some(t) = closest(near_lo, lo) {
            lo = t;
        }
        if let Some(t) = closest(near_hi, hi) {
            hi = t;
        }
        Ok((add(p, mul(u, (lo + hi) / 2.0)), EdgeInfo::Line { dir: u }))
    }

    /// The circle of a torus or cone blend at an end face: (centre, axis, radius).
    fn corner_circle(
        &self,
        f: i64,
        nb: &[(i64, Surf)],
        vs: &[V3],
        torus_major: Option<f64>,
        what: &str,
    ) -> Result<(V3, V3, f64), String> {
        let miss = || format!("{what} blend: corner circle not found");
        let s = self.surf_rec(f).and_then(|s| self.recs.get(s)).ok_or_else(miss)?;
        let o = *s.pos.first().ok_or_else(miss)?;
        let ax = unit(*s.vec.first().ok_or_else(miss)?);
        let rh: Vec<(f64, f64)> = vs.iter().map(|&v| Self::rh(o, ax, v)).collect();
        let axial = |n: V3| (dot(n, ax).abs() - 1.0).abs() < 1e-6;
        let pls: Vec<V3> = nb
            .iter()
            .filter_map(|(_, s)| match s {
                Surf::Plane { p, n } if axial(*n) => Some(*p),
                _ => None,
            })
            .collect();
        let cyl: Vec<f64> = nb
            .iter()
            .filter_map(|(_, s)| match s {
                Surf::Cyl { ax: a, r, .. } if axial(*a) => Some(*r),
                _ => None,
            })
            .collect();
        let uniq = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v.dedup();
            v
        };
        let mut hp: Option<f64> = None;
        let mut rc: Option<f64> = None;
        if let Some(p) = pls.first() {
            hp = Some(dot(sub(*p, o), ax));
        } else if let Some(major) = torus_major {
            hp = rh.iter().find(|(rr, _)| (rr - major).abs() < 1e-6).map(|x| x.1);
        }
        if let Some(h0) = hp {
            let others =
                uniq(rh.iter().filter(|(_, h)| (h - h0).abs() > 1e-6).map(|(rr, _)| round_to(*rr, 7)).collect());
            if let [one] = others.as_slice() {
                rc = Some(*one);
            }
        }
        if rc.is_none()
            && let Some(&r0) = cyl.first()
        {
            rc = Some(r0);
            if hp.is_none() {
                let hs =
                    uniq(rh.iter().filter(|(rr, _)| (rr - r0).abs() > 1e-6).map(|(_, h)| round_to(*h, 7)).collect());
                if let [one] = hs.as_slice() {
                    hp = Some(*one);
                }
            }
        }
        let (Some(hp), Some(rc)) = (hp, rc) else { return Err(miss()) };
        Ok((add(o, mul(ax, hp)), ax, rc))
    }
}

/// Two points `eps` (file units) either side of `m` on the edge, for a pick's `path`: the
/// importer then takes the model edge that holds the midpoint and both points, whatever its
/// extent at that point of the history.
pub(crate) fn local_path(m: V3, info: &EdgeInfo, eps: f64) -> Option<[V3; 2]> {
    match *info {
        EdgeInfo::Line { dir } => Some([sub(m, mul(dir, eps)), add(m, mul(dir, eps))]),
        EdgeInfo::Circle { cen, ax, r } => {
            let q = sub(m, cen);
            let q = sub(q, mul(ax, dot(q, ax)));
            if libm::sqrt(dot(q, q)) < 1e-9 {
                return None;
            }
            let a = unit(q);
            let b = cross(unit(ax), a);
            let d = eps / r;
            let pt = |t: f64| add(cen, mul(add(mul(a, libm::cos(t)), mul(b, libm::sin(t))), r));
            Some([pt(-d), pt(d)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_naming_attributes_read_as_the_older_record_types() {
        let tok = |t: u8, v: &[u8]| {
            let mut x = vec![t];
            if matches!(t, 0x07 | 0x0d | 0x0e) {
                x.push(v.len() as u8);
            }
            x.extend(v);
            x
        };
        let int = |v: i32| tok(0x04, &v.to_le_bytes());
        let attr = |name: &[u8]| {
            let mut b = tok(0x0e, b"ATTRIB_CUSTOM");
            b.extend(tok(0x0d, b"attrib"));
            b.extend(tok(0x0c, &(-1i32).to_le_bytes()));
            b.extend(int(-1));
            b.extend(tok(0x0c, &4i32.to_le_bytes()));
            b.extend(tok(0x07, name));
            for v in [1, 44, 1, 209, -1, 1, 1, 24] {
                b.extend(int(v));
            }
            b.push(0x11);
            b
        };
        let mut b = tok(0x0d, b"asmheader");
        b.push(0x11);
        b.extend(attr(b"INV_NMX_BLEND_TAG"));
        b.extend(attr(b"INV_NMX_GEN_TAG"));
        let r = records(&b).unwrap();
        assert!(r[1].ty.starts_with("NMx_blend_tag-"), "{}", r[1].ty);
        assert_eq!(r[1].ints, [-1, 44, 1, 209, -1, 1, 1, 24]);
        assert_eq!(r[1].refs, [-1, 4]);
        // A name not read here keeps its record as it is.
        assert_eq!(r[2].ty, "ATTRIB_CUSTOM-attrib");
        assert_eq!(r[2].ints, [-1, 1, 44, 1, 209, -1, 1, 1, 24]);
    }

    #[test]
    fn an_arc_across_minus_pi_and_pi_keeps_its_midpoint() {
        // Two vertices straddle the +-pi seam of atan2 (the reference axis, a hair to each side)
        // and one is a quarter turn away: the arc between them is the short way round.
        let ax = [0.0, 0.0, 1.0];
        let rf = ref_dir(ax);
        let oth = cross(ax, rf);
        let v0 = [rf[0] * 2.0, rf[1] * 2.0 - 1e-12, 0.0];
        let v1 = [oth[0] * 2.0, oth[1] * 2.0, 0.0];
        let v2 = [rf[0] * 2.0, rf[1] * 2.0 + 1e-12, 0.0];
        let m = circ_mid([0.0; 3], ax, 2.0, &[v0, v1, v2]);
        let want = unit(add(rf, oth));
        assert!(dist(m, mul(want, 2.0)) < 1e-6, "{m:?}");
        // A single vertex is a whole circle: the seam.
        assert!(dist(circ_mid([0.0; 3], ax, 2.0, &[v0]), mul(rf, -2.0)) < 1e-12);
    }

    #[test]
    fn a_plane_meets_a_parallel_cylinder_in_a_line_nearest_the_point() {
        // The plane y = -0.5 and the cylinder of radius 2 about the z axis meet at x = +-sqrt(3.75).
        let (p, u) =
            line_of_plane_cyl([0.0, -0.5, 0.0], [0.0, -1.0, 0.0], [0.0; 3], [0.0, 0.0, 1.0], 2.0, [-2.0, -0.4, 1.0])
                .unwrap();
        assert!(dist(p, [-libm::sqrt(3.75), -0.5, 0.0]) < 1e-9, "{p:?}");
        assert!(dot(u, [0.0, 0.0, 1.0]).abs() > 0.999);
        // An axis across the plane, or a cylinder the plane misses, meet in no line.
        assert!(
            line_of_plane_cyl([0.0, -0.5, 0.0], [0.0, -1.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0], 2.0, [0.0; 3]).is_none()
        );
        assert!(
            line_of_plane_cyl([0.0, -3.0, 0.0], [0.0, -1.0, 0.0], [0.0; 3], [0.0, 0.0, 1.0], 2.0, [0.0; 3]).is_none()
        );
    }

    #[test]
    fn the_midpoint_of_a_half_circle_comes_from_its_arc_not_its_two_ends() {
        let ax = [0.0, 0.0, 1.0];
        let ends = [[2.0, 0.0, 0.0], [-2.0, 0.0, 0.0]];
        assert!(is_half_circle([0.0; 3], ax, &ends));
        assert!(!is_half_circle([0.0; 3], ax, &[ends[0], [0.0, 2.0, 0.0]]));
        // Points along the upper half give its crest, those of the lower half the opposite one.
        let arc = |s: f64| {
            (0..=8).map(move |k| {
                let t = std::f64::consts::PI * f64::from(k) / 8.0;
                [2.0 * libm::cos(t), s * 2.0 * libm::sin(t), 0.0]
            })
        };
        let up: Vec<V3> = arc(1.0).collect();
        let down: Vec<V3> = arc(-1.0).collect();
        assert!(dist(circ_mid([0.0; 3], ax, 2.0, &up), [0.0, 2.0, 0.0]) < 1e-9);
        assert!(dist(circ_mid([0.0; 3], ax, 2.0, &down), [0.0, -2.0, 0.0]) < 1e-9);
    }

    #[test]
    fn a_cylinder_blend_between_a_plane_and_a_cylinder() {
        // A blend of radius 1 about an axis along z, centred at (2, sqrt 5): tangent to the plane
        // x = 1 (distance 1) and outside the cylinder of radius 2 about the origin (distance 3).
        // The edge it replaced is where the two meet nearest the blend: (1, sqrt 3).
        let nb = [
            (1, Surf::Plane { p: [1.0, 0.0, 0.0], n: [1.0, 0.0, 0.0] }),
            (2, Surf::Cyl { c: [0.0; 3], ax: [0.0, 0.0, 1.0], r: 2.0 }),
        ];
        let c = [2.0, libm::sqrt(5.0), 0.5];
        let (p, u) = cyl_edge(&nb, c, [0.0, 0.0, 1.0], 1.0).unwrap();
        assert!(dist(p, [1.0, libm::sqrt(3.0), 0.5]) < 1e-9, "{p:?}");
        assert!(dot(u, [0.0, 0.0, 1.0]).abs() > 0.999);
        // A third face beside it does not matter; a missing support does.
        assert!(cyl_edge(&nb[..1], c, [0.0, 0.0, 1.0], 1.0).unwrap_err().contains("1 usable supports"));
    }
}
