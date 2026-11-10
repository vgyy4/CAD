//! Topological naming: operation ids, face and edge recipes, the faces an extrude makes and the
//! edge midpoints a fillet or chamfer picks (rules derived from the sample bodies, see the
//! layout notes, sections 11 and 12).

use super::Ctx;
use super::acis;
use super::geom::{self, P2, Plane, V3, add, cross, dot, mul, norm, sub, unit, world};
use super::sketch::{Seg, Sketch};
use super::stream::{Design, i32_at, u32_at, u64_at};
use std::collections::HashMap;
use std::rc::Rc;

/// Operation id to the feature that owns it (`5D89B935` records). Two layouts: `u32 n, n x
/// (u64 op, u64 feature)` (older) or `u32 n, n x (u64 feature, u32 k, k x u64 op)` (newer).
pub(super) fn op_map(d: &Design) -> HashMap<u64, u64> {
    let mut out = HashMap::new();
    for &i in &d.ids {
        if !d.is(i, "5D89B935") {
            continue;
        }
        let r = d.rec(i);
        for j in 15..r.len().saturating_sub(4).min(60) {
            let n = u32_at(r, j).unwrap_or(0) as usize;
            if n == 0 || n >= 100_000 {
                continue;
            }
            if let Some((res, known)) = newer_ops(d, r, j, n)
                && !res.is_empty()
                && known * 2 >= n
            {
                out.extend(res);
                break;
            }
            if j + 4 + 16 * n <= r.len() {
                let pairs: Vec<(u64, u64)> =
                    (0..n).filter_map(|k| Some((u64_at(r, j + 4 + 16 * k)?, u64_at(r, j + 12 + 16 * k)?))).collect();
                if pairs.len() == n && pairs.iter().all(|(o, f)| d.has(*f) && *o < 1 << 32) {
                    for (o, f) in pairs {
                        out.insert(o, f);
                    }
                    break;
                }
            }
        }
    }
    out
}

fn newer_ops(d: &Design, r: &[u8], j: usize, n: usize) -> Option<(HashMap<u64, u64>, usize)> {
    let mut p = j + 4;
    let mut res = HashMap::new();
    let mut known = 0;
    for _ in 0..n {
        let f = u64_at(r, p)?;
        let k = u32_at(r, p + 8)? as usize;
        p += 12;
        if k > 10_000 || f > 1 << 40 {
            return None;
        }
        let mut ops = Vec::with_capacity(k);
        for _ in 0..k {
            let o = u64_at(r, p)?;
            p += 8;
            if o > 1 << 32 {
                return None;
            }
            ops.push(o);
        }
        if d.has(f) {
            known += 1;
            for o in ops {
                res.insert(o, f);
            }
        }
    }
    Some((res, known))
}

/// A face named by its generators: each entry is the generator text and the operation ids (the
/// creating one first).
pub(super) type FaceName = Vec<(String, Vec<i32>)>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum RecipeKind {
    BoundedFace,
    Edge,
    Face,
    Body,
    Vertex,
}

/// A recipe record (`7ACC2A03`): its kind and the faces it names.
pub(super) fn recipe(d: &Design, i: u64) -> (Option<RecipeKind>, Option<Vec<FaceName>>) {
    let r = d.rec(i);
    let has = |n: &[u8]| r.windows(n.len()).any(|w| w == n);
    let kind = [
        (&b"bounded_face_recipe_data"[..], RecipeKind::BoundedFace),
        (b"edge_recipe_data", RecipeKind::Edge),
        (b"face_recipe_data", RecipeKind::Face),
        (b"body_recipe_data", RecipeKind::Body),
        (b"vertex_recipe_data", RecipeKind::Vertex),
    ]
    .into_iter()
    .find(|(n, _)| has(n))
    .map(|(_, k)| k);
    for start in 15..40 {
        if let Some(faces) = recipe_faces(r, start) {
            return (kind, Some(faces));
        }
    }
    (kind, None)
}

fn generator(r: &[u8], p: usize) -> Option<(String, usize)> {
    let l = u32_at(r, p)? as usize;
    if l == 0 || l >= 12 {
        return None;
    }
    let s = r.get(p + 4..p + 4 + l)?;
    let g = std::str::from_utf8(s).ok()?;
    let digits = g.strip_prefix('-').unwrap_or(g);
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((g.to_string(), p + 4 + l))
}

fn recipe_faces(r: &[u8], start: usize) -> Option<Vec<FaceName>> {
    let mut p = start;
    let a = u32_at(r, p)?;
    let nf = u32_at(r, p + 8)? as usize;
    p += 12;
    if a != 1 || nf == 0 || nf >= 1000 {
        return None;
    }
    let mut faces = Vec::new();
    for _ in 0..nf {
        let k = u32_at(r, p)? as usize;
        p += 4;
        if k == 0 || k >= 100 {
            return None;
        }
        let mut ents = Vec::new();
        for _ in 0..k {
            let (g, q) = generator(r, p)?;
            p = q;
            if u32_at(r, p)? > 100_000 {
                return None;
            }
            p += 4;
            let n = u32_at(r, p)? as usize;
            p += 4;
            if n > 1000 {
                return None;
            }
            let mut ops = Vec::with_capacity(n);
            for _ in 0..n {
                ops.push(i32_at(r, p)?);
                p += 4;
            }
            if u32_at(r, p)? != 0 {
                return None;
            }
            p += 4;
            ents.push((g, ops));
        }
        faces.push(ents);
    }
    Some(faces)
}

// ----- extrude geometry --------------------------------------------------------------------

/// What `extrude_geom` knows of an extrude: sketch placement, its profile curve pieces in
/// profile-record order, and the cap offsets along the normal (`None` when the extent is not
/// a distance).
#[derive(Debug)]
pub(super) struct ExtGeom {
    pub pl: Plane,
    pub sketch: Rc<Sketch>,
    pub segs: Vec<Rc<Seg>>,
    pub t0: Option<f64>,
    pub t1: Option<f64>,
}

/// Generator text to (profile curve piece index in `ExtGeom::segs`, traversed forward).
pub(super) type GenMap = HashMap<String, (usize, bool)>;

fn start_of(s: &Seg, fw: bool) -> Option<P2> {
    if fw { s.poly.first() } else { s.poly.last() }.copied()
}

/// A loop's pieces in walk order, each with whether it is walked forward; counter-clockwise
/// (or clockwise) about the sketch normal.
fn loop_ring(segs: &[(usize, &Rc<Seg>)], ccw: bool) -> Option<Vec<(usize, bool)>> {
    let (i0, s0) = segs.first()?;
    let mut out = vec![(*i0, true)];
    let mut used = vec![false; segs.len()];
    *used.first_mut()? = true;
    let mut cur = *s0.poly.last()?;
    let mut count = 1;
    while count < segs.len() {
        let mut found = false;
        for (k, (idx, sg)) in segs.iter().enumerate() {
            if *used.get(k)? {
                continue;
            }
            if geom::dist(*sg.poly.first()?, cur) < 1e-5 {
                out.push((*idx, true));
                cur = *sg.poly.last()?;
            } else if geom::dist(*sg.poly.last()?, cur) < 1e-5 {
                out.push((*idx, false));
                cur = *sg.poly.first()?;
            } else {
                continue;
            }
            *used.get_mut(k)? = true;
            count += 1;
            found = true;
            break;
        }
        if !found {
            return None;
        }
    }
    let by_idx: HashMap<usize, &Rc<Seg>> = segs.iter().map(|(i, s)| (*i, *s)).collect();
    let mut pts: Vec<P2> = Vec::new();
    for (idx, fw) in &out {
        let s = by_idx.get(idx)?;
        if *fw {
            pts.extend(s.poly.iter().copied());
        } else {
            pts.extend(s.poly.iter().rev().copied());
        }
    }
    let mut area = 0.0;
    for (i, p) in pts.iter().enumerate() {
        let q = pts.get((i + 1) % pts.len())?;
        area += p[0] * q[1] - q[0] * p[1];
    }
    area /= 2.0;
    if (area > 0.0) != ccw {
        let first = out.first().copied()?;
        let mut rev = vec![(first.0, !first.1)];
        rev.extend(out.iter().skip(1).rev().map(|(x, f)| (*x, !*f)));
        out = rev;
    }
    Some(out)
}

/// Generator names of the side faces of an extrude of these profile pieces: "3", "4", ... one per
/// profile curve, loops in profile-record order, each loop walked counter-clockwise about the
/// sketch normal (holes clockwise), starting with the curve that, in that walk, starts at the
/// start point of the loop's highest-record-id curve.
pub(super) fn gen_map(segs: &[Rc<Seg>]) -> Option<GenMap> {
    type Loop<'a> = (usize, Vec<(usize, &'a Rc<Seg>)>);
    let mut loops: Vec<Loop> = Vec::new();
    for (i, sg) in segs.iter().enumerate() {
        match loops.last_mut() {
            Some((key, v)) if *key == sg.loopkey => v.push((i, sg)),
            _ => loops.push((sg.loopkey, vec![(i, sg)])),
        }
    }
    let mut out = GenMap::new();
    let mut g = 3usize;
    for (_, ls) in &loops {
        let first = ls.first()?.1;
        let ring = loop_ring(ls, first.flag == 1)?;
        // The first maximum, as the walk lists them.
        let mut mx = ring.first().copied()?;
        for &(i, f) in &ring {
            if segs.get(i)?.curve > segs.get(mx.0)?.curve {
                mx = (i, f);
            }
        }
        let anchor = *segs.get(mx.0)?.poly.first()?;
        let k = ring
            .iter()
            .position(|(i, f)| segs.get(*i).and_then(|s| start_of(s, *f)).is_some_and(|p| geom::dist(p, anchor) < 1e-6))
            .unwrap_or(0);
        let rot: Vec<(usize, bool)> = ring.iter().skip(k).chain(ring.iter().take(k)).copied().collect();
        for (i, f) in rot {
            out.insert(g.to_string(), (i, f));
            g += 1;
        }
    }
    Some(out)
}

/// Face geometry of a modelled face.
#[derive(Debug, Clone)]
pub(super) enum FaceG {
    /// `mid`: a point of the face itself (a cap's origin projection, a side's chord middle at mid
    /// height); `p` is any point of its plane.
    Plane {
        n: V3,
        p: V3,
        cap: bool,
        mid: V3,
    },
    Cyl {
        axis: V3,
        feature: u64,
        seg: Rc<Seg>,
        pl: Plane,
    },
}

fn on_sweep(g: &ExtGeom, sg: &Seg, pts: &[V3], tol: f64) -> bool {
    let pl = &g.pl;
    let loc: Vec<V3> = pts
        .iter()
        .map(|p| {
            let w = sub(*p, pl.origin);
            [dot(w, pl.x), dot(w, pl.y), dot(w, pl.z)]
        })
        .collect();
    if let (Some(t0), Some(t1)) = (g.t0, g.t1) {
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        if loc.iter().any(|v| v[2] < lo - 1e-5 || v[2] > hi + 1e-5) {
            return false;
        }
    }
    let q = sg.curve;
    if g.sketch.lines.contains_key(&q) {
        let (Some(a), Some(b)) = (sg.poly.first(), sg.poly.last()) else { return false };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = libm::hypot(dx, dy);
        if l < 1e-12 {
            return false;
        }
        return loc.iter().all(|v| ((v[0] - a[0]) * dy - (v[1] - a[1]) * dx).abs() / l < tol);
    }
    if let Some(a) = g.sketch.arcs.get(&q) {
        return loc.iter().all(|v| (libm::hypot(v[0] - a.center[0], v[1] - a.center[1]) - a.radius).abs() < tol);
    }
    false
}

/// The line where two planes meet: a point on it and its unit direction.
pub(super) fn plane_plane(n0: V3, p0: V3, n1: V3, p1: V3) -> Option<(V3, V3)> {
    let u = cross(n0, n1);
    if norm(u) < 1e-9 {
        return None;
    }
    let u = unit(u)?;
    let (d0, d1) = (dot(n0, p0), dot(n1, p1));
    let c = cross(n1, u);
    let e = cross(u, n0);
    let den = dot(n0, c);
    if den.abs() < 1e-12 {
        return None;
    }
    Some((mul(add(mul(c, d0), mul(e, d1)), 1.0 / den), u))
}

impl Ctx<'_> {
    pub(super) fn tagged(&mut self) -> &[acis::TaggedFace] {
        let files = self.acis;
        self.tagged.get_or_insert_with(|| {
            let mut out = Vec::new();
            for (n, b) in files {
                if n.ends_with(".smb")
                    && n.contains("Active")
                    && let Some(f) = acis::tagged_faces(b)
                {
                    out.extend(f);
                }
            }
            out
        })
    }

    pub(super) fn edge_sets(&mut self) -> &[Vec<acis::Edge>] {
        let files = self.acis;
        self.edge_sets.get_or_insert_with(|| {
            let mut out = Vec::new();
            for (n, b) in files {
                if (n.ends_with(".smb") || n.ends_with(".smbh"))
                    && let Some(s) = acis::body_edges(b)
                {
                    out.extend(s);
                }
            }
            out
        })
    }

    /// The profile curve whose swept surface carries the ACIS face tagged `g`, when exactly one does.
    fn truth(&mut self, g: &ExtGeom, gen_text: &str) -> Option<u64> {
        let mut cands: Vec<u64> = Vec::new();
        for f in self.tagged() {
            if f.tag != gen_text || f.verts.len() < 2 {
                continue;
            }
            let hit: Vec<u64> = g.segs.iter().filter(|sg| on_sweep(g, sg, &f.verts, 2e-5)).map(|sg| sg.curve).collect();
            if let [one] = hit.as_slice()
                && !cands.contains(one)
            {
                cands.push(*one);
            }
        }
        if let [one] = cands.as_slice() { Some(*one) } else { None }
    }

    /// Geometry of a face named (generator, operation ids). A planar face of an extrude that a
    /// Shell then changed (the name lists the extrude's operation, then the shell's) is the
    /// shell's inner wall: the extrude's plane moved inward by the wall thickness, inward being
    /// the side of the middle of the extrude's profile (the profile vertices' mean at mid
    /// height). The shell's op names the face on both its sides in the samples (6124: Fillet2's
    /// edges end on the outer wall `[321]` and the inner wall `[321, 324]`).
    pub(super) fn face(&mut self, g: &str, ops: &[i32]) -> Option<FaceG> {
        let (first, rest) = ops.split_first()?;
        if rest.is_empty() {
            return self.face_of_extrude(g, ops);
        }
        // Other later operations leave the face where the extrude put it (as before).
        let mut wall = 0.0;
        let mut shelled = false;
        for o in rest {
            let Some(fid) = u64::try_from(*o).ok().and_then(|o| self.opmap.get(&o).copied()) else { continue };
            if self.d.is(fid, "E8063467") {
                shelled = true;
                wall += super::feature::param_values(self.d, fid, &["innerThickness"]).first().copied()?;
            }
        }
        if !shelled {
            return self.face_of_extrude(g, ops);
        }
        let base = self.face_of_extrude(g, std::slice::from_ref(first))?;
        let FaceG::Plane { n, p, cap, mid } = base else { return None };
        let efid = *self.opmap.get(&u64::try_from(*first).ok()?)?;
        let eg = self.ext_geom(efid)?;
        let (t0, t1) = (eg.t0?, eg.t1?);
        let pts: Vec<P2> = eg.segs.iter().flat_map(|sg| sg.poly.iter().copied()).collect();
        if pts.is_empty() {
            return None;
        }
        let k = pts.len() as f64;
        let mean = [pts.iter().map(|q| q[0]).sum::<f64>() / k, pts.iter().map(|q| q[1]).sum::<f64>() / k];
        let c = world(&eg.pl, mean, (t0 + t1) / 2.0);
        let side = dot(n, sub(c, p));
        if side.abs() < 1e-9 {
            return None;
        }
        let q = add(p, mul(n, wall * side.signum()));
        Some(FaceG::Plane { n, p: q, cap, mid: add(mid, mul(n, wall * side.signum())) })
    }

    /// A planar face of an extrude (or its shelled wall) as the draft reads it: a point of the
    /// face and the normal that points away from the middle of the extrude's profile at mid
    /// height (outward for a convex profile; the cap normals follow the extrusion direction).
    pub(super) fn face_pick(&mut self, g: &str, ops: &[i32]) -> Option<(V3, V3)> {
        let FaceG::Plane { n, mid, cap, .. } = self.face(g, ops)? else { return None };
        let efid = *self.opmap.get(&u64::try_from(*ops.first()?).ok()?)?;
        let eg = self.ext_geom(efid)?;
        let (t0, t1) = (eg.t0?, eg.t1?);
        if cap {
            let up = if t1 >= t0 { 1.0 } else { -1.0 };
            let s = if g == "2" { up } else { -up };
            return Some((mid, mul(n, s)));
        }
        let pts: Vec<P2> = eg.segs.iter().flat_map(|sg| sg.poly.iter().copied()).collect();
        if pts.is_empty() {
            return None;
        }
        let k = pts.len() as f64;
        let mean = [pts.iter().map(|q| q[0]).sum::<f64>() / k, pts.iter().map(|q| q[1]).sum::<f64>() / k];
        let c = world(&eg.pl, mean, (t0 + t1) / 2.0);
        let side = dot(n, sub(mid, c));
        if side.abs() < 1e-9 {
            return None;
        }
        Some((mid, mul(n, side.signum())))
    }

    /// Geometry of a face named (generator, operation ids), when it is an extrude's cap or side.
    fn face_of_extrude(&mut self, g: &str, ops: &[i32]) -> Option<FaceG> {
        let op = u64::try_from(*ops.first()?).ok()?;
        let fid = *self.opmap.get(&op)?;
        if !self.d.is(fid, super::EXTRUDE) {
            return None;
        }
        let eg = self.ext_geom(fid)?;
        let pl = eg.pl;
        let z = pl.z;
        if g == "1" || g == "2" {
            let t = if g == "1" { eg.t0 } else { eg.t1 }?;
            let p = world(&pl, [0.0, 0.0], t);
            return Some(FaceG::Plane { n: z, p, cap: true, mid: p });
        }
        let sg: Rc<Seg> = match self.truth(&eg, g) {
            Some(c) => eg.segs.iter().find(|x| x.curve == c)?.clone(),
            None => {
                if eg.segs.iter().any(|x| x.flag != 1) {
                    return None;
                }
                let m = self.gen_map(fid)?;
                let (i, _) = m.get(g)?;
                eg.segs.get(*i)?.clone()
            }
        };
        let q = sg.curve;
        if eg.sketch.lines.contains_key(&q) {
            let (a, b) = (*sg.poly.first()?, *sg.poly.last()?);
            let pa = world(&pl, a, 0.0);
            let pb = world(&pl, b, 0.0);
            let n = unit(cross(z, sub(pb, pa)))?;
            let mid = world(&pl, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], (eg.t0? + eg.t1?) / 2.0);
            return Some(FaceG::Plane { n, p: pa, cap: false, mid });
        }
        if eg.sketch.arcs.contains_key(&q) {
            return Some(FaceG::Cyl { axis: z, feature: fid, seg: sg.clone(), pl });
        }
        None
    }

    /// The midpoint of the edge between `faces[0]` and `faces[1]`, bounded by the others: and
    /// whether it is an arc.
    pub(super) fn edge_midpoint(&mut self, faces: &[FaceName]) -> Option<(V3, bool)> {
        let fs: Vec<Option<FaceG>> = faces.iter().map(|f| f.first().and_then(|(g, ops)| self.face(g, ops))).collect();
        let (a, b) = (fs.first()?.clone()?, fs.get(1)?.clone()?);
        if let (FaceG::Plane { n: n0, p: p0, .. }, FaceG::Plane { n: n1, p: p1, .. }) = (&a, &b) {
            let (pp, u) = plane_plane(*n0, *p0, *n1, *p1)?;
            let mut ts = Vec::new();
            for f in fs.iter().skip(2).flatten() {
                if let FaceG::Plane { n, p, .. } = f
                    && dot(*n, u).abs() > 1e-9
                {
                    ts.push(dot(*n, sub(*p, pp)) / dot(*n, u));
                }
            }
            if ts.len() >= 2 {
                let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if hi - lo > 1e-7 {
                    return Some((add(pp, mul(u, (lo + hi) / 2.0)), false));
                }
            }
            return None;
        }
        // A cap meeting an arc side: the middle of the arc piece at the cap.
        for (cap, cyl) in [(&a, &b), (&b, &a)] {
            let (FaceG::Plane { n, p, cap: true, .. }, FaceG::Cyl { axis, feature, seg, pl, .. }) = (cap, cyl) else {
                continue;
            };
            if (dot(*n, *axis).abs() - 1.0).abs() > 1e-9 {
                continue;
            }
            let poly = &seg.poly;
            let mut ls = vec![0.0];
            for w in poly.windows(2) {
                if let [p0, p1] = w {
                    ls.push(ls.last().copied().unwrap_or(0.0) + geom::dist(*p0, *p1));
                }
            }
            let half = ls.last().copied().unwrap_or(0.0) / 2.0;
            let mut m2 = None;
            for (k, w) in poly.windows(2).enumerate() {
                let [p0, p1] = w else { continue };
                let (l0, l1) = (ls.get(k).copied()?, ls.get(k + 1).copied()?);
                if l1 >= half {
                    let t = (half - l0) / (l1 - l0).max(1e-15);
                    m2 = Some([p0[0] + t * (p1[0] - p0[0]), p0[1] + t * (p1[1] - p0[1])]);
                    break;
                }
            }
            let mut m2 = m2?;
            let eg = self.ext_geom(*feature)?;
            let arc = eg.sketch.arcs.get(&seg.curve)?;
            let (c, r) = (arc.center, arc.radius);
            let v = [m2[0] - c[0], m2[1] - c[1]];
            let l = libm::hypot(v[0], v[1]);
            if l > 0.0 {
                m2 = [c[0] + v[0] / l * r, c[1] + v[1] / l * r];
            }
            let tcap = dot(*n, sub(*p, pl.origin)) / dot(*n, pl.z);
            return Some((world(pl, m2, tcap), true));
        }
        None
    }
}
