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

pub(crate) struct Acis {
    pub(crate) recs: Vec<Rec>,
    /// Body tag (first int) -> record.
    bodies: HashMap<i64, usize>,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
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

/// A surface as far as blend analysis needs it.
#[derive(Debug, Clone)]
pub(crate) enum Surf {
    Plane { p: V3, n: V3 },
    Cyl { c: V3, ax: V3, r: f64 },
    Cone,
    Torus { major: f64, minor: f64 },
    Sphere,
    Other(String),
    None,
}

impl Acis {
    fn surf_rec(&self, f: i64) -> Option<usize> {
        self.rec(f)?.refs.iter().copied().find(|&y| self.ends(y, "surface")).and_then(|y| usize::try_from(y).ok())
    }

    pub(crate) fn kind(&self, f: i64) -> Surf {
        let Some(s) = self.surf_rec(f) else { return Surf::None };
        let Some(r) = self.recs.get(s) else { return Surf::None };
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
            return match (r.dbls.first(), r.dbls.get(1)) {
                (Some(&a), Some(&b)) => Surf::Torus { major: a, minor: b.abs() },
                _ => Surf::None,
            };
        }
        if t == "sphere-surface" {
            return Surf::Sphere;
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

    /// The faces across each coedge of a face.
    pub(crate) fn neighbours(&self, f: i64) -> Vec<i64> {
        let mut out = Vec::new();
        for c in self.coedges(f) {
            let Some(p) = self.rec(c).and_then(|r| r.refs.get(4).copied()).filter(|&p| self.is(p, "coedge")) else {
                continue;
            };
            let Some(l) = self.rec(p).and_then(|r| r.refs.iter().copied().find(|&y| self.is(y, "loop"))) else {
                continue;
            };
            if let Some(g) = self.rec(l).and_then(|r| r.refs.iter().copied().find(|&y| self.is(y, "face"))) {
                out.push(g);
            }
        }
        out
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

    /// Faces that carry an `NMx_blend_tag` attribute whose 4th int is `fid`.
    pub(crate) fn blend_faces(&self, fid: i64) -> Vec<i64> {
        let mut out = Vec::new();
        for r in &self.recs {
            if r.ty.starts_with("NMx_blend_tag")
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

fn circ_mid(cen: V3, ax: V3, rad: f64, vs: &[V3]) -> V3 {
    let rf = ref_dir(ax);
    let oth = cross(unit(ax), rf);
    let mut angs: Vec<f64> = Vec::new();
    for &v in vs {
        let q = sub(v, cen);
        let q = sub(q, mul(ax, dot(q, ax)));
        if libm::sqrt(dot(q, q)) > 1e-9 {
            angs.push(round_to(libm::atan2(dot(q, oth), dot(q, rf)), 9));
        }
    }
    angs.sort_by(f64::total_cmp);
    angs.dedup();
    if angs.len() < 2 {
        return sub(cen, mul(rf, rad));
    }
    let tau = 2.0 * std::f64::consts::PI;
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

impl Acis {
    /// The midpoint (file units) of the edge the blend face `f` replaced, or why not.
    pub(crate) fn edge_mid(&self, f: i64) -> Result<V3, String> {
        let kind = self.kind(f);
        let nb: Vec<(i64, Surf)> = self.neighbours(f).into_iter().map(|g| (g, self.kind(g))).collect();
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
        let (p, u);
        match &kind {
            Surf::Cyl { c, ax, r } => {
                let sup: Vec<(V3, V3)> = planes
                    .iter()
                    .filter(|(_, pp, n)| dot(*n, *ax).abs() < 1e-6 && (dot(sub(*c, *pp), *n).abs() - r).abs() < 1e-6)
                    .map(|(_, pp, n)| (*pp, *n))
                    .collect();
                let [a, b] = sup.as_slice() else {
                    return Err(format!("cylinder blend with {} planar supports", sup.len()));
                };
                (p, u) = line_of_planes(a.0, a.1, b.0, b.1, *c).ok_or("parallel supports")?;
            }
            Surf::Plane { n: nc, .. } => {
                let k = 1.0 / vs.len() as f64;
                let cen = mul(vs.iter().fold([0.0; 3], |a, &v| add(a, v)), k);
                let mut best: Option<(f64, (V3, V3))> = None;
                for (i, a) in planes.iter().enumerate() {
                    for b in planes.iter().skip(i + 1) {
                        let Some(l) = line_of_planes(a.1, a.2, b.1, b.2, cen) else { continue };
                        if dot(l.1, *nc).abs() > 1e-6 {
                            continue;
                        }
                        let dd = dist(l.0, cen);
                        if best.as_ref().is_none_or(|x| dd < x.0) {
                            best = Some((dd, l));
                        }
                    }
                }
                (p, u) = best.ok_or("no support pair")?.1;
            }
            Surf::Torus { major, .. } => return self.corner_circle(f, &nb, &vs, Some(*major), "torus"),
            Surf::Cone => return self.corner_circle(f, &nb, &vs, None, "cone"),
            Surf::Sphere => return Err("blend surface sphere-surface".into()),
            Surf::Other(t) => return Err(format!("blend surface {t}")),
            Surf::None => return Err("blend surface None".into()),
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
        Ok(add(p, mul(u, (lo + hi) / 2.0)))
    }

    /// The edge of a torus or cone blend (a circle at an end face): its midpoint.
    fn corner_circle(
        &self,
        f: i64,
        nb: &[(i64, Surf)],
        vs: &[V3],
        torus_major: Option<f64>,
        what: &str,
    ) -> Result<V3, String> {
        let miss = || format!("{what} blend: corner circle not found");
        let s = self.surf_rec(f).and_then(|s| self.recs.get(s)).ok_or_else(miss)?;
        let o = *s.pos.first().ok_or_else(miss)?;
        let ax = unit(*s.vec.first().ok_or_else(miss)?);
        let rh: Vec<(f64, f64)> = vs
            .iter()
            .map(|&v| {
                let q = sub(v, o);
                let h = dot(q, ax);
                (libm::sqrt((dot(q, q) - h * h).max(0.0)), h)
            })
            .collect();
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
        Ok(circ_mid(add(o, mul(ax, hp)), ax, rc, vs))
    }
}
