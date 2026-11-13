//! Topological naming: operation ids, face and edge recipes, the faces an extrude makes and the
//! edge midpoints a fillet or chamfer picks (rules derived from the sample bodies, see the
//! layout notes, sections 11 and 12).

use super::Ctx;
use super::acis;
use super::geom::{self, P2, Plane, V3, add, cross, dot, mul, norm, sub, unit, world};
use super::sketch::{Seg, Sketch};
use super::stream::{Design, i32_at, u32_at, u64_at};
use crate::cover;
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
            if let Some((res, known, end)) = newer_ops(d, r, j, n)
                && !res.is_empty()
                && known * 2 >= n
            {
                cover::field(r, j, end - j, "op-map.feature-ops");
                out.extend(res);
                break;
            }
            if j + 4 + 16 * n <= r.len() {
                let pairs: Vec<(u64, u64)> =
                    (0..n).filter_map(|k| Some((u64_at(r, j + 4 + 16 * k)?, u64_at(r, j + 12 + 16 * k)?))).collect();
                // The table keeps the operations of features the design no longer has (2232:
                // 1 of 45, 4400: 38 of 408): the ids rise strictly, and most features exist.
                let rising = pairs.windows(2).all(|w| matches!(w, [a, b] if a.0 < b.0));
                let known = pairs.iter().filter(|(_, f)| d.has(*f)).count();
                if pairs.len() == n
                    && rising
                    && known * 2 >= n
                    && pairs.iter().all(|(o, f)| *o < 1 << 32 && *f < 1 << 40)
                {
                    cover::field(r, j, 4 + 16 * n, "op-map.pairs");
                    out.extend(pairs.into_iter().filter(|(_, f)| d.has(*f)));
                    break;
                }
            }
        }
    }
    out
}

fn newer_ops(d: &Design, r: &[u8], j: usize, n: usize) -> Option<(HashMap<u64, u64>, usize, usize)> {
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
    Some((res, known, p))
}

/// An edge of the model: its middle, whether it is curved, and for a curved edge points along
/// it from end to end (cm).
pub(super) type EdgeAt = (V3, bool, Vec<V3>);

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
    .map(|(n, k)| {
        if let Some(at) = r.windows(n.len()).position(|w| w == n) {
            let len_at = at.checked_sub(4).filter(|o| u32_at(r, *o) == Some(n.len() as u32));
            let from = len_at.unwrap_or(at);
            cover::field(r, from, at + n.len() - from, "recipe.kind");
        }
        k
    });
    // A `71D91267` record wraps a recipe: the recipe's class name (as text) comes first, its
    // faces after it (324's draft selection).
    let wrapped = r.windows(9).position(|w| w == b"7ACC2A03-").map_or(40, |at| (at + 36).max(40));
    for start in (15..40).chain(wrapped..r.len().min(wrapped + 160)) {
        if let Some((faces, end)) = recipe_faces(r, start) {
            cover::field(r, start, end - start, "recipe.faces");
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

fn recipe_faces(r: &[u8], start: usize) -> Option<(Vec<FaceName>, usize)> {
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
    Some((faces, p))
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
    /// The side walls lean (a taper angle): they do not stand on the profile.
    pub tapered: bool,
    /// The curve pieces that bound the profile as a whole (those of one region only: a piece two
    /// regions share lies inside the extruded area).
    pub bounds: Vec<Rc<Seg>>,
}

impl ExtGeom {
    /// Whether a sketch point lies inside the profile's area (even-odd over `bounds`); `None`
    /// without a closed boundary.
    pub(super) fn in_profile(&self, q: P2) -> Option<bool> {
        if self.bounds.is_empty() {
            return None;
        }
        // Every end of a piece meets an end of another (or its own other end): closed loops.
        let ends: Vec<P2> =
            self.bounds.iter().flat_map(|sg| [sg.poly.first(), sg.poly.last()]).flatten().copied().collect();
        let close = |a: &P2, b: &P2| geom::dist(*a, *b) < 1e-6;
        if ends.iter().enumerate().any(|(i, a)| !ends.iter().enumerate().any(|(j, b)| i != j && close(a, b))) {
            return None;
        }
        let mut c = false;
        for sg in &self.bounds {
            for w in sg.poly.windows(2) {
                let [a, b] = w else { continue };
                if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
                    c = !c;
                }
            }
        }
        Some(c)
    }

    /// The side of a planar side face (its normal `n`, a point `mid` of it) away from the
    /// profile's area: the probes either side of the face, where one is inside and one outside.
    pub(super) fn away_from_profile(&self, n: V3, mid: V3) -> Option<f64> {
        let pl = &self.pl;
        let rel = sub(mid, pl.origin);
        let (q, m) = ([dot(rel, pl.x), dot(rel, pl.y)], [dot(n, pl.x), dot(n, pl.y)]);
        let len = (m[0] * m[0] + m[1] * m[1]).sqrt();
        if len < 1e-9 {
            return None;
        }
        let e = 1e-5 / len;
        let plus = self.in_profile([q[0] + m[0] * e, q[1] + m[1] * e])?;
        let minus = self.in_profile([q[0] - m[0] * e, q[1] - m[1] * e])?;
        (plus != minus).then_some(if plus { -1.0 } else { 1.0 })
    }
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
    /// The rolling-ball face a fillet puts on one concave edge of two planar sides: its radius and
    /// each side's point and normal pointing into the void (towards the fillet's axis).
    Fillet {
        r: f64,
        a: (V3, V3),
        b: (V3, V3),
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

/// Whether the points (flattened onto the sketch plane) all lie along the profile piece `sg`,
/// between its ends: on its polyline within the sag of an arc's chords.
fn spans(g: &ExtGeom, sg: &Seg, pts: &[V3]) -> bool {
    let pl = &g.pl;
    let sag = g.sketch.arcs.get(&sg.curve).map_or(0.0, |a| 0.002 * a.radius);
    pts.iter().all(|p| {
        let w = sub(*p, pl.origin);
        let q = [dot(w, pl.x), dot(w, pl.y)];
        sg.poly.windows(2).any(|s| matches!(s, [a, b] if geom::seg_dist(q, *a, *b) < 2e-5 + sag))
    })
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

/// The centre of the rolling ball of a fillet (radius `r`, sides `a`, `b` with their inward
/// normals) in the plane `w` (normal, point): the point `r` inside both sides.
fn fillet_centre(r: f64, a: (V3, V3), b: (V3, V3), w: (V3, V3)) -> Option<V3> {
    let (na, nb, nw) = (a.1, b.1, w.0);
    let det = dot(na, cross(nb, nw));
    if det.abs() < 1e-9 {
        return None;
    }
    let (ra, rb, rw) = (dot(na, a.0) + r, dot(nb, b.0) + r, dot(nw, w.1));
    let x = add(add(mul(cross(nb, nw), ra), mul(cross(nw, na), rb)), mul(cross(na, nb), rw));
    Some(mul(x, 1.0 / det))
}

/// Where a line `pp + t u` lying in one of the two planes `on` ends at a fillet face (radius
/// `r`, sides `a`, `b` with normals towards its axis) that rounds the corner of that plane with
/// another: `r` from the other side's plane.
pub(super) fn fillet_end(r: f64, a: (V3, V3), b: (V3, V3), on: [(V3, V3); 2], pp: V3, u: V3) -> Option<f64> {
    let lies = |q: &(V3, V3), n: V3, p: V3| {
        (dot(unit(q.1).unwrap_or(q.1), n).abs() - 1.0).abs() < 1e-9 && dot(n, sub(q.0, p)).abs() < 1e-7
    };
    let (op, on_) =
        [(a, b), (b, a)].into_iter().find_map(|(s, o)| on.iter().any(|(n, p)| lies(&s, *n, *p)).then_some(o))?;
    (dot(on_, u).abs() > 1e-9).then(|| (r - dot(on_, sub(pp, op))) / dot(on_, u))
}

/// Where the line `pp + t u` crosses the round of a fillet (its cylinder, within the quarter
/// between its two sides' tangent lines, 1e-7 cm): the parameters `t`.
pub(super) fn fillet_crossings(r: f64, a: (V3, V3), b: (V3, V3), pp: V3, u: V3) -> Vec<f64> {
    let (Some(na), Some(nb)) = (unit(a.1), unit(b.1)) else { return Vec::new() };
    let Some(axis) = unit(cross(na, nb)) else { return Vec::new() };
    let Some(c) = fillet_centre(r, a, b, (axis, [0.0; 3])) else { return Vec::new() };
    let across = |v: V3| sub(v, mul(axis, dot(v, axis)));
    let (w, v) = (across(sub(pp, c)), across(u));
    let (qa, qb, qc) = (dot(v, v), 2.0 * dot(w, v), dot(w, w) - r * r);
    let disc = qb * qb - 4.0 * qa * qc;
    if qa < 1e-18 || disc < 0.0 {
        return Vec::new();
    }
    let sq = disc.sqrt();
    [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)]
        .into_iter()
        .filter(|t| {
            // On the round: away from the axis against both sides' normals (which point at it).
            let q = add(w, mul(v, *t));
            dot(q, na) <= 1e-7 && dot(q, nb) <= 1e-7
        })
        .collect()
}

/// The midpoint of an edge that touches fillet faces. Either the arc where a fillet meets a
/// plane (the middle of the arc, at the point of the circle nearest the corner), or the straight
/// line where two planes meet between two fillets (the middle of the two tangent points).
pub(super) fn fillet_edge(fs: &[Option<FaceG>]) -> Option<(V3, bool)> {
    let first = [fs.first()?.as_ref()?, fs.get(1)?.as_ref()?];
    if let [FaceG::Fillet { r, a, b }, FaceG::Plane { n, p, .. }]
    | [FaceG::Plane { n, p, .. }, FaceG::Fillet { r, a, b }] = first
    {
        let c = fillet_centre(*r, *a, *b, (*n, *p))?;
        let bisector = unit(add(a.1, b.1))?;
        return Some((sub(c, mul(bisector, *r)), true));
    }
    let [FaceG::Plane { n: n0, p: p0, .. }, FaceG::Plane { n: n1, p: p1, .. }] = first else { return None };
    let fillets: Vec<&FaceG> = fs.iter().skip(2).flatten().filter(|f| matches!(f, FaceG::Fillet { .. })).collect();
    // The side is the plane both fillets round; the other is the wall they are cut in.
    for (w, s) in [((*n0, *p0), (*n1, *p1)), ((*n1, *p1), (*n0, *p0))] {
        let ends: Vec<V3> = fillets
            .iter()
            .filter_map(|f| {
                let FaceG::Fillet { r, a, b } = f else { return None };
                let side = [a, b]
                    .into_iter()
                    .find(|q| (dot(q.1, s.0).abs() - 1.0).abs() < 1e-6 && dot(q.1, sub(s.1, q.0)).abs() < 1e-6)?;
                let c = fillet_centre(*r, *a, *b, w)?;
                Some(sub(c, mul(side.1, *r)))
            })
            .collect();
        if let [e0, e1] = ends.as_slice() {
            return Some((mul(add(*e0, *e1), 0.5), false));
        }
    }
    None
}

/// The surface of a stored face (cm), as far as a pick names it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Surface {
    Plane {
        origin: V3,
        normal: V3,
    },
    Cylinder {
        origin: V3,
        axis: V3,
        radius: f64,
    },
    /// ACIS `cone-surface`: base centre, axis, base radius and the sine and cosine of the half
    /// angle (the radius at height `h` along the axis is `radius + h * sin / cos`).
    Cone {
        origin: V3,
        axis: V3,
        radius: f64,
        sin: f64,
        cos: f64,
    },
    /// Any other surface: the pick goes by the face's points alone.
    Other,
}

impl Surface {
    fn of(f: &acis::NamedFace) -> Option<Surface> {
        let v = |k: usize| f.vecs.get(k).copied();
        let n = |k: usize| f.nums.get(k).copied();
        Some(match f.kind.as_str() {
            // The face's sense (its first logical: `true` reversed) turns the normal outward;
            // the samples' faces are all forward.
            "plane" => {
                let n = unit(v(1)?)?;
                Surface::Plane { origin: v(0)?, normal: if f.flags.first() == Some(&true) { mul(n, -1.0) } else { n } }
            }
            "cone" => {
                // centre, axis, major axis; ratio, sine, cosine. An elliptic section is "other".
                let (c, a, m, ratio, s, co) = (v(0)?, unit(v(1)?)?, v(2)?, n(0)?, n(1)?, n(2)?);
                if (ratio - 1.0).abs() > 1e-9 {
                    return Some(Surface::Other);
                }
                if s.abs() < 1e-12 {
                    Surface::Cylinder { origin: c, axis: a, radius: norm(m) }
                } else {
                    Surface::Cone { origin: c, axis: a, radius: norm(m), sin: s, cos: co }
                }
            }
            _ => Surface::Other,
        })
    }

    fn same(&self, o: &Surface) -> bool {
        let close = |a: V3, b: V3| norm(sub(a, b)) < 1e-7;
        let on_axis = |o1: V3, a1: V3, o2: V3, a2: V3| {
            (dot(a1, a2).abs() - 1.0).abs() < 1e-9 && norm(cross(sub(o2, o1), a1)) < 1e-7
        };
        match (self, o) {
            (Surface::Plane { origin: o1, normal: n1 }, Surface::Plane { origin: o2, normal: n2 }) => {
                (dot(*n1, *n2).abs() - 1.0).abs() < 1e-9 && dot(*n1, sub(*o2, *o1)).abs() < 1e-7
            }
            (
                Surface::Cylinder { origin: o1, axis: a1, radius: r1 },
                Surface::Cylinder { origin: o2, axis: a2, radius: r2 },
            ) => on_axis(*o1, *a1, *o2, *a2) && (r1 - r2).abs() < 1e-7,
            (Surface::Cone { .. }, Surface::Cone { .. }) => {
                let (
                    Surface::Cone { origin: o1, axis: a1, radius: r1, sin: s1, cos: c1 },
                    Surface::Cone { origin: o2, axis: a2, radius: r2, sin: s2, cos: c2 },
                ) = (self, o)
                else {
                    return false;
                };
                close(*o1, *o2)
                    && close(*a1, *a2)
                    && (r1 - r2).abs() < 1e-7
                    && (s1 - s2).abs() < 1e-9
                    && (c1 - c2).abs() < 1e-9
            }
            (Surface::Other, Surface::Other) => true,
            _ => false,
        }
    }

    /// The tree's `surface` object (mm).
    pub(super) fn json(&self) -> serde_json::Value {
        use serde_json::json;
        let m = |p: V3| json!([geom::mm(p[0]), geom::mm(p[1]), geom::mm(p[2])]);
        let u = |p: V3| json!([geom::round9(p[0]), geom::round9(p[1]), geom::round9(p[2])]);
        match self {
            Surface::Plane { origin, normal } => json!({"type": "plane", "origin": m(*origin), "normal": u(*normal)}),
            Surface::Cylinder { origin, axis, radius } => {
                json!({"type": "cylinder", "origin": m(*origin), "axis": u(*axis), "radius": geom::mm(*radius)})
            }
            Surface::Cone { origin, axis, radius, sin, cos } => json!({"type": "cone", "origin": m(*origin),
                "axis": u(*axis), "radius": geom::mm(*radius), "sin": geom::round9(*sin), "cos": geom::round9(*cos)}),
            Surface::Other => json!({"type": "other"}),
        }
    }
}

/// At most 64 points, evenly spread over the list.
fn thin(points: Vec<V3>) -> Vec<V3> {
    let k = points.len();
    if k <= 64 {
        return points;
    }
    (0..64).filter_map(|i| points.get(i * k / 64).copied()).collect()
}

/// A face picked by the stored bodies: its surface and points of its edges (cm).
#[derive(Debug, Clone)]
pub(super) struct StoredPick {
    pub surface: Surface,
    pub points: Vec<V3>,
}

impl StoredPick {
    /// The tree's face pick: `point` (the first point), `points` and `surface` (mm).
    pub(super) fn json(&self) -> serde_json::Value {
        let m = |p: &V3| serde_json::json!([geom::mm(p[0]), geom::mm(p[1]), geom::mm(p[2])]);
        let pts: Vec<serde_json::Value> = self.points.iter().map(m).collect();
        serde_json::json!({"point": pts.first().cloned().unwrap_or_default(), "points": pts, "surface": self.surface.json()})
    }
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

    /// The edges of the stored bodies (every state the file keeps) that lie between two named
    /// faces.
    fn named_edges(&mut self) -> &[acis::NamedEdge] {
        let files = self.acis;
        self.named_edges.get_or_insert_with(|| {
            let mut out = Vec::new();
            for (n, b) in files {
                if (n.ends_with(".smb") || n.ends_with(".smbh"))
                    && let Some(s) = acis::named_edges(b)
                {
                    out.extend(s);
                }
            }
            out
        })
    }

    /// The plane of the face named (generator, operation ids), as the stored bodies hold it: a
    /// planar face whose name has the generator with these operations, alone or followed by
    /// operations that trim a face without moving it (extrudes, sweeps, revolves, holes, blends,
    /// combines). Every such stored face must lie on one plane.
    fn stored_plane(&mut self, g: &str, ops: &[i32]) -> Option<(V3, V3)> {
        const TRIMMING: [&str; 8] =
            [super::EXTRUDE, super::SWEEP, "E3849A15", "1C037A07", "A07D5F17", "F757D611", "2A94257F", "B6678572"];
        if ops.is_empty() {
            return None;
        }
        let files = self.acis;
        if self.named_planes.is_none() {
            let mut out = Vec::new();
            for (n, b) in files {
                if (n.ends_with(".smb") || n.ends_with(".smbh"))
                    && let Some(s) = acis::named_planes(b)
                {
                    out.extend(s);
                }
            }
            self.named_planes = Some(out);
        }
        let mut found: Option<(V3, V3)> = None;
        for f in self.named_planes.as_deref().unwrap_or(&[]) {
            let hit = f.name.iter().any(|(g2, ops2)| {
                g2 == g
                    && ops2.get(..ops.len()) == Some(ops)
                    && ops2.iter().skip(ops.len()).all(|o| {
                        u64::try_from(*o)
                            .ok()
                            .and_then(|o| self.opmap.get(&o))
                            .is_some_and(|fid| TRIMMING.iter().any(|c| self.d.is(*fid, c)))
                    })
            });
            if !hit {
                continue;
            }
            let n = unit(f.normal)?;
            match found {
                None => found = Some((f.point, n)),
                Some((p, m)) => {
                    if (dot(n, m).abs() - 1.0).abs() > 1e-9 || dot(m, sub(f.point, p)).abs() > 1e-7 {
                        return None;
                    }
                }
            }
        }
        found
    }

    /// The edge a recipe names, as a stored body holds it: an edge between two faces carrying
    /// exactly the names of the recipe's first two faces. Several such edges (the two faces
    /// meet more than once, or several stored states hold the edge) must agree on the middle,
    /// or exactly one of them must have the recipe's other faces at its ends. A body that holds
    /// faces of the blend being read is a state after it, and its edges are left out (3722's
    /// end body: Fusion's chamfer of the coil's start leaves a sliver of the start cap, whose
    /// slanted edge meets the base's cylinder face there; before the chamfer the cap meets the
    /// cylinder along the base of its triangle).
    fn stored_edge(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let (f0, f1) = (faces.first()?, faces.get(1)?);
        let ends: Vec<&FaceName> = faces.iter().skip(2).collect();
        let mut mids: Vec<(EdgeAt, bool)> = Vec::new();
        let blend = std::mem::take(&mut self.blend_ops);
        for e in self.named_edges() {
            if e.body_ops.iter().any(|o| blend.contains(o)) {
                continue;
            }
            let [a, b] = &e.faces;
            if !((a == f0 && b == f1) || (a == f1 && b == f0)) {
                continue;
            }
            let bounded = ends.iter().all(|n| e.around.contains(n));
            let chord = mul(add(e.a, e.b), 0.5);
            let arc = norm(sub(chord, e.mid)) > 1e-7;
            match mids.iter_mut().find(|m| norm(sub(m.0.0, e.mid)) < 1e-6) {
                Some(m) => m.1 |= bounded,
                // A curved edge keeps its points: where the model cuts a circle is not where
                // the file does, so its middle alone may name no edge of the model.
                None => mids.push(((e.mid, arc, if arc { e.path.clone() } else { Vec::new() }), bounded)),
            }
        }
        self.blend_ops = blend;
        if let [one] = mids.as_slice() {
            return Some(one.0.clone());
        }
        let fit: Vec<&(EdgeAt, bool)> = mids.iter().filter(|m| m.1).collect();
        if let [one] = fit.as_slice() { Some(one.0.clone()) } else { None }
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

    /// The profile piece whose swept surface carries the stored face `gen_text` of the operation
    /// `op` (a face whose name is read must have that generator under that operation; one with
    /// a tag only counts by the tag). An extrude of regions side by side lists a curve's pieces
    /// once per region, and several pieces of one line lie on one plane: of the pieces the
    /// face's vertices lie on, the ones that span them are taken, and pieces with the same ends
    /// count once. Exactly one piece must remain, over all such faces. A face that runs over
    /// several pieces of one straight line (regions side by side, swept as one) is the plane of
    /// any of them: the piece comes with the middle of the face's own vertices then.
    fn truth(&mut self, g: &ExtGeom, gen_text: &str, op: i32) -> Option<(usize, Option<V3>)> {
        let same = |i: usize, j: usize| {
            let (Some(a), Some(b)) = (g.segs.get(i), g.segs.get(j)) else { return false };
            let ends = |s: &Seg| Some((*s.poly.first()?, *s.poly.last()?));
            let (Some((a0, a1)), Some((b0, b1))) = (ends(a), ends(b)) else { return false };
            let near = |p: P2, q: P2| geom::dist(p, q) < 1e-6;
            a.curve == b.curve && ((near(a0, b0) && near(a1, b1)) || (near(a0, b1) && near(a1, b0)))
        };
        let mut cands: Vec<(usize, Option<V3>)> = Vec::new();
        for f in self.tagged() {
            if f.tag != gen_text || f.verts.len() < 2 {
                continue;
            }
            if f.name.as_ref().is_some_and(|n| !n.iter().any(|(g2, ops)| g2 == gen_text && ops.first() == Some(&op))) {
                continue;
            }
            let on: Vec<usize> = (0..g.segs.len())
                .filter(|i| g.segs.get(*i).is_some_and(|sg| on_sweep(g, sg, &f.verts, 2e-5)))
                .collect();
            let spanning: Vec<usize> =
                on.iter().copied().filter(|i| g.segs.get(*i).is_some_and(|sg| spans(g, sg, &f.verts))).collect();
            let mut hit: Vec<usize> = Vec::new();
            for i in if spanning.is_empty() { on } else { spanning } {
                if !hit.iter().any(|j| same(i, *j)) {
                    hit.push(i);
                }
            }
            let found = match hit.as_slice() {
                [one] => Some((*one, None)),
                [first, ..]
                    if hit.iter().all(|i| g.segs.get(*i).is_some_and(|sg| g.sketch.lines.contains_key(&sg.curve))) =>
                {
                    let k = f.verts.len() as f64;
                    let sum = f.verts.iter().fold([0.0; 3], |a, v| add(a, *v));
                    Some((*first, Some(mul(sum, 1.0 / k))))
                }
                _ => None,
            };
            // Two faces count once when they lead to the same piece, or to pieces of one line.
            if let Some((i, mid)) = found
                && !cands.iter().any(|(j, m)| same(i, *j) || (mid.is_some() && m.is_some()))
            {
                cands.push((i, mid));
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
        self.face_at(g, ops, 0)
    }

    /// [`Ctx::face`] without the stored planes: a face whose middle point is known.
    pub(super) fn face_with_middle(&mut self, g: &str, ops: &[i32]) -> Option<FaceG> {
        self.face_modelled(g, ops, 0)
    }

    /// The modelled face, else the plane a stored body has under that name (read as a face
    /// without a known middle: `cap`, so nothing takes it for an extrude's side).
    fn face_at(&mut self, g: &str, ops: &[i32], depth: usize) -> Option<FaceG> {
        if let Some(f) = self.face_modelled(g, ops, depth) {
            return Some(f);
        }
        if let Some((p, n)) = self.stored_plane(g, ops) {
            return Some(FaceG::Plane { n, p, cap: true, mid: p });
        }
        // Every stored face that carries exactly this name on one plane (the plane faces that
        // carry it among other entries disagree: 6299's thin cut `Extrude7 (2)`, whose side
        // `11` ends `Fillet4 (2)`'s corner edge).
        match self.stored_face(g, ops)?.surface {
            Surface::Plane { origin, normal } => {
                let n = unit(normal)?;
                Some(FaceG::Plane { n, p: origin, cap: true, mid: origin })
            }
            _ => None,
        }
    }

    fn face_modelled(&mut self, g: &str, ops: &[i32], depth: usize) -> Option<FaceG> {
        if let [op] = ops
            && let Some(fid) = u64::try_from(*op).ok().and_then(|o| self.opmap.get(&o).copied())
            && self.d.is(fid, "A07D5F17")
        {
            return self.face_of_fillet(g, fid, depth);
        }
        let (first, rest) = ops.split_first()?;
        if rest.is_empty() {
            return self.face_of_extrude(g, ops);
        }
        if let Some(f) = self.mirrored_face(g, ops, depth) {
            return Some(f);
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

    /// A face of a copy a mirror of features made, named (g, [op, mirror, copy]) (`D1728651`, then
    /// its copy `AD719C1A`): the face (g, [op]) reflected in the mirror plane. 6124's `Mirror2`
    /// copies the tab `Extrude9` across y = 0; `Fillet3` rounds the copy's corners and `Fillet4`
    /// runs along the copy's sides (`6 [374, 376, 377]`, where the tab's own is `6 [374]`).
    fn mirrored_face(&mut self, g: &str, ops: &[i32], depth: usize) -> Option<FaceG> {
        let [first, m, i] = ops else { return None };
        let mfid = self.op_feature(*m).filter(|q| self.d.is(*q, "D1728651"))?;
        if !self.op_feature(*i).is_some_and(|q| self.d.is(q, "AD719C1A")) {
            return None;
        }
        let f = self.face_modelled(g, std::slice::from_ref(first), depth)?;
        let (o, n) = super::json::mirror_plane(self, mfid)?;
        let n = unit(n)?;
        let pt = |p: V3| sub(p, mul(n, 2.0 * dot(n, sub(p, o))));
        let dir = |v: V3| sub(v, mul(n, 2.0 * dot(n, v)));
        match f {
            FaceG::Plane { n: fnormal, p, cap, mid } => {
                Some(FaceG::Plane { n: dir(fnormal), p: pt(p), cap, mid: pt(mid) })
            }
            FaceG::Fillet { r, a, b } => Some(FaceG::Fillet { r, a: (pt(a.0), dir(a.1)), b: (pt(b.0), dir(b.1)) }),
            // An arc side is held by its sketch and placement, which a reflection does not keep.
            FaceG::Cyl { .. } => None,
        }
    }

    /// The feature an operation id belongs to.
    fn op_feature(&self, op: i32) -> Option<u64> {
        u64::try_from(op).ok().and_then(|o| self.opmap.get(&o).copied())
    }

    /// The face a fillet puts on its `g`-th face (1-based): the faces are numbered in the order
    /// of its edge recipes, but an edge whose round lies on the round of an earlier edge (the
    /// same radius and axis: two corners of a side the rounds use up, 2315's posts, whose 4 mm
    /// ends are rounded by 2 mm, the stored faces show one face per end) takes no number of its
    /// own. Where a recipe's round cannot be worked out, the `g`-th recipe.
    fn face_of_fillet(&mut self, g: &str, fid: u64, depth: usize) -> Option<FaceG> {
        if depth >= 3 {
            return None;
        }
        let k: usize = g.parse().ok().filter(|k| *k >= 1)?;
        let axis = |f: &FaceG| -> Option<(f64, V3, V3)> {
            let FaceG::Fillet { r, a, b } = f else { return None };
            let dir = unit(cross(a.1, b.1))?;
            Some((*r, dir, fillet_centre(*r, *a, *b, (dir, [0.0; 3]))?))
        };
        let mut seen: Vec<(f64, V3, V3)> = Vec::new();
        let mut idx = 0;
        while seen.len() < k && idx < 512 {
            if self.blend_edge(fid, idx).is_none() {
                break;
            }
            let Some(f) = self.fillet_of_recipe(fid, idx, depth) else {
                return self.fillet_of_recipe(fid, k - 1, depth);
            };
            let Some(ax) = axis(&f) else { return self.fillet_of_recipe(fid, k - 1, depth) };
            let same = |o: &(f64, V3, V3)| {
                (o.0 - ax.0).abs() < 1e-9 && (dot(o.1, ax.1).abs() - 1.0).abs() < 1e-9 && norm(sub(o.2, ax.2)) < 1e-7
            };
            if !seen.iter().any(same) {
                seen.push(ax);
                if seen.len() == k {
                    return Some(f);
                }
            }
            idx += 1;
        }
        self.fillet_of_recipe(fid, k - 1, depth)
    }

    /// The round a fillet puts on its `idx`-th edge recipe (0-based): the two sides that edge
    /// joins must be planar faces, and the fillet a corner of the void they bound, rounded with
    /// the radius of the fillet's set.
    fn fillet_of_recipe(&mut self, fid: u64, idx: usize, depth: usize) -> Option<FaceG> {
        let (r, rid) = self.blend_edge(fid, idx)?;
        let (Some(RecipeKind::Edge), Some(faces)) = recipe(self.d, rid) else { return None };
        // A cap counts where the edge's two faces are of one extrude (a side and its cap meet
        // at a convex edge of the prism, the material inside both): 67's Fillet1 rounds the
        // four long edges of a box, between its sides and caps.
        let one_op = match (faces.first().and_then(|f| f.first()), faces.get(1).and_then(|f| f.first())) {
            (Some((_, o0)), Some((_, o1))) => o0.len() == 1 && o0 == o1,
            _ => false,
        };
        let side = |cx: &mut Self, f: &FaceName| -> Option<(V3, V3)> {
            let (g, ops) = f.first()?;
            if !matches!(cx.face_at(g, ops, depth + 1)?, FaceG::Plane { cap, .. } if !cap || one_op) {
                return None;
            }
            let (p, out) = cx.face_pick_at(g, ops, depth + 1)?;
            Some((p, mul(out, -1.0)))
        };
        let a = side(self, faces.first()?)?;
        let b = side(self, faces.get(1)?)?;
        // The faces' sides as read can be wrong for the edge (4632's Fillet6 rounds the corners
        // of a notch a cut made, the cut's side facing into the notch; a concave edge rounds
        // about an axis in the void): where the round as read is not on a stored round of the
        // fillet with its radius, the one way of turning the sides that is.
        let flip = |s: (V3, V3)| (s.0, mul(s.1, -1.0));
        let read = FaceG::Fillet { r, a, b };
        if !self.on_stored_round(fid, &read) {
            let others = [(flip(a), b), (a, flip(b)), (flip(a), flip(b))].map(|(a, b)| FaceG::Fillet { r, a, b });
            let fits: Vec<&FaceG> = others.iter().filter(|f| self.on_stored_round(fid, f)).collect();
            if let [one] = fits.as_slice() {
                return Some((*one).clone());
            }
        }
        Some(FaceG::Fillet { r, a, b })
    }

    /// Whether the round `f` lies on a stored round of the fillet `fid`: a cylinder of its
    /// radius (1e-7 cm) about its axis (1e-6 cm).
    fn on_stored_round(&mut self, fid: u64, f: &FaceG) -> bool {
        let FaceG::Fillet { r, a, b } = f else { return false };
        let Some(dir) = unit(cross(a.1, b.1)) else { return false };
        let Some(c) = fillet_centre(*r, *a, *b, (dir, [0.0; 3])) else { return false };
        let ops: Vec<i32> =
            self.opmap.iter().filter(|(_, q)| **q == fid).filter_map(|(o, _)| i32::try_from(*o).ok()).collect();
        self.named_faces().iter().filter(|nf| nf.name.first().is_some_and(|(_, o)| ops.iter().any(|k| o == &[*k]))).any(
            |nf| {
                let Some(Surface::Cylinder { origin, axis, radius }) = Surface::of(nf) else { return false };
                let Some(ax) = unit(axis) else { return false };
                let w = sub(c, origin);
                (radius - r).abs() < 1e-7
                    && (dot(ax, dir).abs() - 1.0).abs() < 1e-9
                    && norm(sub(w, mul(ax, dot(w, ax)))) < 1e-6
            },
        )
    }

    /// A planar face of an extrude (or its shelled wall) as the draft reads it: a point of the
    /// face and the normal that points away from the middle of the extrude's profile at mid
    /// height (outward for a convex profile; the cap normals follow the extrusion direction).
    pub(super) fn face_pick(&mut self, g: &str, ops: &[i32]) -> Option<(V3, V3)> {
        self.face_pick_at(g, ops, 0)
    }

    fn face_pick_at(&mut self, g: &str, ops: &[i32], depth: usize) -> Option<(V3, V3)> {
        // A mirrored copy's face: the original's pick, reflected.
        if let [first, m, i] = ops
            && let Some(mfid) = self.op_feature(*m).filter(|q| self.d.is(*q, "D1728651"))
            && self.op_feature(*i).is_some_and(|q| self.d.is(q, "AD719C1A"))
        {
            let (p, out) = self.face_pick_at(g, std::slice::from_ref(first), depth)?;
            let (o, n) = super::json::mirror_plane(self, mfid)?;
            let n = unit(n)?;
            return Some((sub(p, mul(n, 2.0 * dot(n, sub(p, o)))), sub(out, mul(n, 2.0 * dot(n, out)))));
        }
        let FaceG::Plane { n, mid, cap, .. } = self.face_modelled(g, ops, depth)? else { return None };
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
        // Away from the profile's own area where the probes tell (a profile of several regions
        // apart: 2315's posts), else away from the middle of its points.
        if let Some(s) = eg.away_from_profile(n, mid) {
            return Some((mid, mul(n, s)));
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

    /// Which way a revolve (operation `op`, `angle` radians, about an axis through `at`, cm)
    /// turned, from its stored faces: the unit axis direction about which a right-handed turn by
    /// `angle` from the start cap (`1 [op]`, or the end cap `2 [op]` turned back) sweeps every
    /// point of its faces. The axis's line comes from the faces that are not caps (a cylinder or
    /// cone about it, a plane across it). `None` where the faces do not tell one way alone.
    pub(super) fn revolve_turn(&mut self, op: i32, at: V3, angle: f64) -> Option<V3> {
        let faces: Vec<acis::NamedFace> = self
            .named_faces()
            .iter()
            .filter(|f| f.name.first().is_some_and(|(_, o)| o.as_slice() == [op]))
            .cloned()
            .collect();
        let gen_of = |f: &acis::NamedFace| f.name.first().map_or("", |(g, _)| g.as_str()).to_string();
        let mut axis: Option<V3> = None;
        for f in faces.iter().filter(|f| !matches!(gen_of(f).as_str(), "1" | "2")) {
            let a = match Surface::of(f)? {
                Surface::Cylinder { origin, axis, .. } | Surface::Cone { origin, axis, .. } => {
                    if norm(cross(sub(at, origin), unit(axis)?)) > 1e-6 {
                        return None;
                    }
                    axis
                }
                Surface::Plane { normal, .. } => normal,
                Surface::Other => continue,
            };
            let a = unit(a)?;
            match axis {
                None => axis = Some(a),
                Some(b) if (dot(a, b).abs() - 1.0).abs() < 1e-9 => {}
                Some(_) => return None,
            }
        }
        let axis = axis?;
        let radial = |q: V3| {
            let r = sub(q, at);
            sub(r, mul(axis, dot(r, axis)))
        };
        let points = |f: &acis::NamedFace| f.verts.iter().chain(&f.edge_pts).copied().collect::<Vec<V3>>();
        // The way a cap leaves the axis; `None` for a cap on both sides of it (2354's `Revolve2`:
        // a face the axis halves, which a half turn makes a whole cylinder of either way).
        let cap = |g: &str| {
            let ps: Vec<V3> = faces.iter().filter(|f| gen_of(f) == g).flat_map(points).map(radial).collect();
            let dir = unit(ps.iter().fold([0.0; 3], |acc, r| add(acc, *r)))?;
            ps.iter().all(|r| dot(*r, dir) > -1e-7).then_some(dir)
        };
        let (start, end) = (cap("1"), cap("2"));
        let all: Vec<V3> = faces.iter().flat_map(points).collect();
        let fits = |a: V3| -> Option<bool> {
            let s = match (start, end) {
                (Some(s), _) => s,
                // The end cap turned back by the angle (`end` is across the axis).
                (None, Some(e)) => add(mul(e, libm::cos(angle)), mul(cross(a, e), -libm::sin(angle))),
                (None, None) => return None,
            };
            Some(all.iter().all(|q| {
                let r = radial(*q);
                if norm(r) < 1e-7 {
                    return true;
                }
                let mut phi = libm::atan2(dot(a, cross(s, r)), dot(s, r));
                if phi < 0.0 {
                    phi += std::f64::consts::TAU;
                }
                phi <= angle + 1e-6 || phi >= std::f64::consts::TAU - 1e-6
            }))
        };
        let back = mul(axis, -1.0);
        match (fits(axis)?, fits(back)?) {
            (true, false) => Some(axis),
            (false, true) => Some(back),
            _ => None,
        }
    }

    fn named_faces(&mut self) -> &[acis::NamedFace] {
        let files = self.acis;
        self.named_faces.get_or_insert_with(|| {
            let mut out = Vec::new();
            for (n, b) in files {
                if (n.ends_with(".smb") || n.ends_with(".smbh"))
                    && let Some(s) = acis::named_faces(b)
                {
                    out.extend(s);
                }
            }
            out
        })
    }

    /// A face the modelled caps and planar sides do not give: an extrude's arc side (its
    /// cylinder, with the profile arc's points at mid height), else [`Ctx::stored_face`].
    pub(super) fn general_face(&mut self, g: &str, ops: &[i32]) -> Option<StoredPick> {
        if let Some(FaceG::Cyl { axis, feature, seg, pl }) = self.face_modelled(g, ops, 0) {
            let eg = self.ext_geom(feature)?;
            let arc = eg.sketch.arcs.get(&seg.curve)?;
            let origin = world(&pl, [arc.center[0], arc.center[1]], 0.0);
            let surface = Surface::Cylinder { origin, axis, radius: arc.radius };
            let points = match (eg.t0, eg.t1) {
                (Some(t0), Some(t1)) => seg.poly.iter().map(|q| world(&pl, *q, (t0 + t1) / 2.0)).collect(),
                // An extent that is not a distance: the points of the stored faces of that name
                // on this cylinder (a later offset may have moved others without renaming them).
                _ => self.stored_points(g, ops, &surface),
            };
            if points.is_empty() {
                return None;
            }
            return Some(StoredPick { surface, points });
        }
        self.stored_face(g, ops)
    }

    /// The edge points of the stored faces whose name starts with (g, ops) that lie on `surface`.
    fn stored_points(&mut self, g: &str, ops: &[i32], surface: &Surface) -> Vec<V3> {
        let want = (g.to_string(), ops.to_vec());
        let mut points: Vec<V3> = Vec::new();
        for f in self.named_faces().iter().filter(|f| f.name.first() == Some(&want)) {
            if *surface != Surface::Other && Surface::of(f).is_none_or(|s| !s.same(surface)) {
                continue;
            }
            for p in f.verts.iter().chain(&f.edge_pts) {
                if !points.iter().any(|q| norm(sub(*p, *q)) < 1e-7) {
                    points.push(*p);
                }
            }
        }
        thin(points)
    }

    /// The surfaces of the stored faces whose name starts with exactly (g, ops), one per state
    /// that holds such a face (a later feature that moves the face keeps its name: a thread).
    pub(super) fn stored_surfaces(&mut self, g: &str, ops: &[i32]) -> Vec<Surface> {
        let want = (g.to_string(), ops.to_vec());
        self.named_faces().iter().filter(|f| f.name.first() == Some(&want)).filter_map(Surface::of).collect()
    }

    /// A face named (generator, operation ids), of any surface, as the stored bodies hold it:
    /// every stored face whose name starts with exactly that entry must lie on one surface, and
    /// the pick is that surface with the points of those faces' edges (cm). A later feature
    /// that moves a face without renaming it (an offset) leaves two surfaces: no pick then.
    pub(super) fn stored_face(&mut self, g: &str, ops: &[i32]) -> Option<StoredPick> {
        let want = (g.to_string(), ops.to_vec());
        let hits: Vec<acis::NamedFace> =
            self.named_faces().iter().filter(|f| f.name.first() == Some(&want)).cloned().collect();
        let mut surface = Surface::of(hits.first()?)?;
        if hits.iter().any(|f| Surface::of(f).is_none_or(|s| !s.same(&surface))) {
            // States on different surfaces (a later offset moved the face and kept its name):
            // the points of all of them, the model's face at this feature holds its own.
            surface = Surface::Other;
        }
        let points = self.stored_points(g, ops, &surface);
        if points.is_empty() {
            return None;
        }
        Some(StoredPick { surface, points })
    }

    /// [`Ctx::stored_face`] of a face whose name several stored faces carry (an extrude's cap
    /// over two regions apart: 5998's `Extrude5`, two rails), narrowed by the faces round it
    /// that a bounded-face recipe names after it (`beside`): of the stored faces of that name,
    /// those that share an edge point with a stored face of every name beside it. `None` where
    /// that leaves none or does not narrow the faces down.
    pub(super) fn stored_face_beside(&mut self, g: &str, ops: &[i32], beside: &[FaceName]) -> Option<StoredPick> {
        let want = (g.to_string(), ops.to_vec());
        let all = self.named_faces();
        let hits: Vec<&acis::NamedFace> = all.iter().filter(|f| f.name.first() == Some(&want)).collect();
        let around: Vec<Vec<&acis::NamedFace>> = beside
            .iter()
            .filter_map(|n| n.first())
            .map(|w| all.iter().filter(|f| f.name.first() == Some(w)).collect())
            .collect();
        if around.is_empty() || around.iter().any(Vec::is_empty) {
            return None;
        }
        let pts = |f: &acis::NamedFace| f.verts.iter().chain(&f.edge_pts).copied().collect::<Vec<V3>>();
        let touches = |a: &acis::NamedFace, b: &acis::NamedFace| {
            let pb = pts(b);
            pts(a).iter().any(|p| pb.iter().any(|q| norm(sub(*p, *q)) < 1e-7))
        };
        let kept: Vec<&acis::NamedFace> =
            hits.iter().copied().filter(|f| around.iter().all(|fs| fs.iter().any(|b| touches(f, b)))).collect();
        if kept.is_empty() || kept.len() == hits.len() {
            return None;
        }
        let surface = Surface::of(kept.first()?)?;
        if kept.iter().any(|f| Surface::of(f).is_none_or(|s| !s.same(&surface))) {
            return None;
        }
        let mut points: Vec<V3> = Vec::new();
        for f in &kept {
            for p in pts(f) {
                if !points.iter().any(|q| norm(sub(p, *q)) < 1e-7) {
                    points.push(p);
                }
            }
        }
        (!points.is_empty()).then(|| StoredPick { surface, points: thin(points) })
    }

    /// Where the line `p + t u` touches the cylinder an extrude sweeps from the arc piece `seg`
    /// (`None` when it crosses it, misses it or runs along its axis).
    fn tangent_at(&mut self, feature: u64, seg: &Seg, pl: &Plane, p: V3, u: V3) -> Option<f64> {
        let eg = self.ext_geom(feature)?;
        let arc = eg.sketch.arcs.get(&seg.curve)?;
        let (c, r, a) = (world(pl, [arc.center[0], arc.center[1]], 0.0), arc.radius, pl.z);
        let w = sub(p, c);
        let wp = sub(w, mul(a, dot(w, a)));
        let up = sub(u, mul(a, dot(u, a)));
        let aa = dot(up, up);
        if aa < 1e-12 {
            return None;
        }
        let t = -dot(wp, up) / aa;
        let closest = norm(add(wp, mul(up, t)));
        ((closest - r).abs() < 1e-6).then_some(t)
    }

    /// Whether the generator numbering of an extrude whose profile has holes agrees with the
    /// stored faces: at least two of its side generators name a stored face whose piece is known
    /// (`truth`), and each of those is the piece the numbering gives. 6299's `Extrude1 (2)` (an
    /// outline of 16 pieces and a round hole): generators 3 to 6 are stored and all agree, and
    /// `Chamfer1 (2)` names generator 8.
    fn gen_map_confirmed(&mut self, fid: u64, eg: &ExtGeom, op: i32) -> bool {
        if let Some(c) = self.gconfirmed.get(&(fid, op)) {
            return *c;
        }
        let ok = self.gen_map(fid).is_some_and(|m| {
            let mut agree = 0;
            for (k, (i, _)) in m.iter() {
                match self.truth(eg, k, op) {
                    Some((t, _)) if t == *i => agree += 1,
                    Some(_) => return false,
                    None => {}
                }
            }
            agree >= 2
        });
        self.gconfirmed.insert((fid, op), ok);
        ok
    }

    /// Geometry of a face named (generator, operation ids), when it is an extrude's cap or side.
    fn face_of_extrude(&mut self, g: &str, ops: &[i32]) -> Option<FaceG> {
        let op = u64::try_from(*ops.first()?).ok()?;
        let fid = *self.opmap.get(&op)?;
        if !self.d.is(fid, super::EXTRUDE) {
            return None;
        }
        if let Some(caps) = self.thin_caps.get(&fid) {
            return caps.face(g, ops.len() > 1);
        }
        let eg = self.ext_geom(fid)?;
        let pl = eg.pl;
        let z = pl.z;
        if g == "1" || g == "2" {
            let t = if g == "1" { eg.t0 } else { eg.t1 }?;
            let p = world(&pl, [0.0, 0.0], t);
            return Some(FaceG::Plane { n: z, p, cap: true, mid: p });
        }
        if eg.tapered {
            // A leaning wall is not modelled: its edges come from the stored bodies.
            return None;
        }
        let mut face_mid: Option<V3> = None;
        let sg: Rc<Seg> = match self.truth(&eg, g, *ops.first()?) {
            Some((i, mid)) => {
                face_mid = mid;
                eg.segs.get(i)?.clone()
            }
            None => {
                // With holes the generator numbering is only taken where the stored faces confirm
                // it (see `gen_map_confirmed`).
                if eg.segs.iter().any(|x| x.flag != 1) && !self.gen_map_confirmed(fid, &eg, *ops.first()?) {
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
            let mid = match face_mid {
                Some(m) => m,
                None => world(&pl, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], (eg.t0? + eg.t1?) / 2.0),
            };
            return Some(FaceG::Plane { n, p: pa, cap: false, mid });
        }
        if eg.sketch.arcs.contains_key(&q) {
            return Some(FaceG::Cyl { axis: z, feature: fid, seg: sg.clone(), pl });
        }
        None
    }

    /// The midpoint of the edge between `faces[0]` and `faces[1]`, bounded by the others: and
    /// whether it is an arc.
    pub(super) fn edge_midpoint(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        self.modelled_edge(faces)
            .or_else(|| self.stored_edge(faces))
            .or_else(|| self.side_at_cap(faces))
            .or_else(|| self.rim_at_plane(faces))
            .or_else(|| self.rim_at_cone(faces))
            .or_else(|| self.edge_by_round_ends(faces))
            .or_else(|| self.rim_named_minus_one(faces))
            .or_else(|| self.mirrored_edge(faces))
            .or_else(|| self.line_on_cylinder(faces))
    }

    /// A straight edge where a plane along a cylinder's axis meets the cylinder, known only
    /// from the stored bodies, ended by faces that are not modelled (3722's Chamfer1: the end
    /// face of a coil joined to the outside of a revolve's cylinder, ended by the coil's helical
    /// faces; no stored state holds the joined body before the chamfer, and the stored end face
    /// is what the chamfer left of it). Of the two lines where the plane cuts the cylinder, the
    /// one on the side of the plane face's stored points. The edge is the piece of it a stored
    /// state of the plane face ends on (below); where none tells, a short piece about the stored
    /// points' middle, which the model's edge must hold whole.
    fn line_on_cylinder(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let (f0, f1) = (faces.first()?, faces.get(1)?);
        for (pf, cf) in [(f0, f1), (f1, f0)] {
            let (pg, pops) = pf.first()?;
            let (cg, cops) = cf.first()?;
            if self.face(cg, cops).is_some() {
                continue;
            }
            let Some(StoredPick { surface: Surface::Cylinder { origin, axis, radius }, .. }) =
                self.stored_face(cg, cops)
            else {
                continue;
            };
            let Some(StoredPick { surface: Surface::Plane { origin: p, normal: n }, points }) =
                self.stored_face(pg, pops)
            else {
                continue;
            };
            let axis = unit(axis)?;
            if dot(n, axis).abs() > 1e-9 || radius.is_nan() || radius <= 0.0 || points.is_empty() {
                continue;
            }
            let k = points.len() as f64;
            let mean = points.iter().fold([0.0; 3], |acc, q| add(acc, mul(*q, 1.0 / k)));
            // The axis's distance from the plane, and the plane's direction across the axis.
            let off = dot(n, sub(p, origin));
            let half = radius * radius - off * off;
            if half <= 1e-12 {
                continue;
            }
            let u = unit(cross(n, axis))?;
            let w = sub(mean, origin);
            let side = if dot(u, w) >= 0.0 { 1.0 } else { -1.0 };
            let along = dot(axis, w);
            let foot = add(origin, add(mul(n, off), mul(u, side * half.sqrt())));
            // Where the edge ends: a stored face of the plane's name, or of the name it had on
            // a combine's tool before the combine trimmed it, with two vertices on the line,
            // whose piece of the line holds the stored points' middle (3722: the coil's start
            // cap `1 [312]` alone, a triangle with its base on the line, before `Combine` 314
            // joined it; the joined cap only survives in the end body, cut down by the chamfer).
            let mut names = vec![pops.clone()];
            if let Some((last, first)) = pops.split_last()
                && !first.is_empty()
                && self.op_feature(*last).is_some_and(|q| self.d.is(q, "2A94257F"))
            {
                names.push(first.to_vec());
            }
            let mut pieces: Vec<(f64, f64)> = Vec::new();
            for f in self.named_faces() {
                let Some((g, ops)) = f.name.first() else { continue };
                if g != pg || !names.contains(ops) {
                    continue;
                }
                let on: Vec<f64> = f
                    .verts
                    .iter()
                    .filter(|q| norm(sub(sub(**q, foot), mul(axis, dot(axis, sub(**q, foot))))) < 1e-7)
                    .map(|q| dot(axis, sub(*q, foot)))
                    .collect();
                let lo = on.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = on.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if on.len() >= 2 && hi - lo > 1e-7 && lo <= along && along <= hi && !pieces.contains(&(lo, hi)) {
                    pieces.push((lo, hi));
                }
            }
            if let [(lo, hi)] = pieces.as_slice() {
                let (a, b) = (add(foot, mul(axis, *lo)), add(foot, mul(axis, *hi)));
                return Some((mul(add(a, b), 0.5), false, vec![a, b]));
            }
            let mid = add(foot, mul(axis, along));
            let step = mul(axis, 0.01);
            return Some((mid, false, vec![sub(mid, step), add(mid, step)]));
        }
        None
    }

    /// An edge of a copy a mirror of features made, where every face it names carries the
    /// operations of its own feature, the mirror's and the copy's (`AD719C1A`, one per copy):
    /// the same edge of the feature mirrored, reflected in the mirror plane. 770's Fillet2 rounds
    /// the end edges of tabs `Mirror1` copied (`[308, 313, 314]`), whose end faces the fillets
    /// take away, so no stored state holds the copies' edges.
    fn mirrored_edge(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let mut tail: Option<(i32, i32)> = None;
        let mut orig: Vec<FaceName> = Vec::new();
        for f in faces {
            let mut of = Vec::new();
            for (g, ops) in f {
                let [first, m, i] = ops.as_slice() else { return None };
                if tail.is_some_and(|t| t != (*m, *i)) {
                    return None;
                }
                tail = Some((*m, *i));
                of.push((g.clone(), vec![*first]));
            }
            orig.push(of);
        }
        let (m, i) = tail?;
        let mfid = self.op_feature(m).filter(|q| self.d.is(*q, "D1728651"))?;
        if !self.op_feature(i).is_some_and(|q| self.d.is(q, "AD719C1A")) {
            return None;
        }
        let (mid, arc, path) = self.edge_midpoint(&orig)?;
        let (o, n) = super::json::mirror_plane(self, mfid)?;
        let n = unit(n)?;
        let pt = |p: V3| sub(p, mul(n, 2.0 * dot(n, sub(p, o))));
        Some((pt(mid), arc, path.into_iter().map(pt).collect()))
    }

    /// A closed edge (no faces bound its ends) between a plane and a cylinder about an axis
    /// across it, where the cylinder is known only from the stored bodies (610's `Fillet3`: the
    /// rim of `Extrude2`'s end cap on the side it shares with `Extrude1`, a profile whose
    /// generator numbering is not confirmed): the whole circle where the plane cuts the
    /// cylinder. Where its seam lies is not known, so the edge goes as points all round it
    /// (the import matches the model's edge along them).
    fn rim_at_plane(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let [f0, f1] = faces else { return None };
        for (pf, cf) in [(f0, f1), (f1, f0)] {
            let (pg, pops) = pf.first()?;
            let (cg, cops) = cf.first()?;
            let (n, p) = match self.face(pg, pops) {
                Some(FaceG::Plane { n, p, .. }) => (n, p),
                _ => continue,
            };
            if self.face(cg, cops).is_some() {
                continue;
            }
            let Some(StoredPick { surface: Surface::Cylinder { origin, axis, radius }, .. }) =
                self.stored_face(cg, cops)
            else {
                continue;
            };
            if (dot(n, axis).abs() - 1.0).abs() > 1e-9 || radius.is_nan() || radius <= 0.0 {
                continue;
            }
            // The circle's centre: the axis where it meets the plane.
            let centre = add(origin, mul(axis, dot(n, sub(p, origin)) / dot(n, axis)));
            let u = unit(cross(axis, if axis[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }))?;
            let v = cross(axis, u);
            // Chords within 5e-4 cm (0.005 mm) of the circle.
            let step = libm::acos((1.0 - 5e-4 / radius).clamp(-1.0, 1.0)) * 2.0;
            let k = ((2.0 * std::f64::consts::PI / step.max(1e-3)).ceil() as usize).clamp(16, 256);
            let at = |t: f64| add(centre, add(mul(u, radius * libm::cos(t)), mul(v, radius * libm::sin(t))));
            let path: Vec<V3> = (0..=k).map(|i| at(2.0 * std::f64::consts::PI * i as f64 / k as f64)).collect();
            return Some((at(std::f64::consts::PI), true, path));
        }
        None
    }

    /// An edge named by generator `-1` of an extrude, a side of it, and its end cap `2` (6252's
    /// `Chamfer1 (2)`, adafruit-more 5981's `Chamfer2`): in both the stored state after the blend
    /// has the rim of the end cap `2` blended (6252: the chamfer face `1 [353]` runs from cap `2`
    /// at z 7.4 mm to the side at z 7.8 mm; no stored face carries `-1`), so the edge is taken as
    /// the one between the side and cap `2`.
    fn rim_named_minus_one(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let [a, b, end] = faces else { return None };
        let [(ge, ops)] = end.as_slice() else { return None };
        let minus = |f: &FaceName| matches!(f.as_slice(), [(g, o)] if g == "-1" && o == ops);
        let side = match (minus(a), minus(b)) {
            (true, false) => b,
            (false, true) => a,
            _ => return None,
        };
        if ge != "2" || ops.len() != 1 {
            return None;
        }
        // The side may be merged with another feature's face (5981: the post's side and the
        // wall's arc side): its entry of this extrude.
        let own = side.iter().find(|(g, o)| o == ops && g != "1" && g != "2" && !g.starts_with('-'))?;
        self.edge_midpoint(&[end.clone(), vec![own.clone()]])
    }

    /// The edges of a face picked whole (a bounded-face recipe in a blend: 5998's Chamfer2 takes
    /// the bottom cap of a box whose corners two blends cut), one per face round it. The list of
    /// faces round it is not in loop order, so each edge where the face meets a plane `B` round it
    /// runs along their common line between the nearest ends, on either side of a point of `B`
    /// (a modelled side's middle, else the mean of `B`'s stored points), that the other faces round
    /// it make (planes it crosses, fillets that leave the face or `B`). A fillet face round it
    /// meets the face along the fillet's arc.
    pub(super) fn face_loop_edges(&mut self, face: &FaceName, ring: &[FaceName]) -> Vec<Option<EdgeAt>> {
        // One face round it (5360's `Fillet - Contacts`: the end cap of a round pin): a closed
        // edge.
        if let [b] = ring {
            return vec![self.edge_midpoint(&[face.clone(), b.clone()])];
        }
        let Some((n0, p0)) = self.plane_of(face) else { return vec![None; ring.len()] };
        let gs: Vec<Option<FaceG>> = ring.iter().map(|f| f.first().and_then(|(g, ops)| self.face(g, ops))).collect();
        let mut out = Vec::new();
        for (k, b) in ring.iter().enumerate() {
            let e = match gs.get(k).cloned().flatten() {
                Some(fb @ FaceG::Fillet { .. }) => {
                    fillet_edge(&[Some(fb), Some(FaceG::Plane { n: n0, p: p0, cap: true, mid: p0 })])
                        .map(|(m, arc)| (m, arc, Vec::new()))
                }
                Some(FaceG::Plane { n: n1, p: p1, cap, mid }) => (|| {
                    let n1 = unit(n1)?;
                    let (pp, u) = plane_plane(n0, p0, n1, p1)?;
                    let inside = if cap {
                        let (g, ops) = b.first()?;
                        let pts = self.stored_face(g, ops)?.points;
                        if pts.is_empty() {
                            return None;
                        }
                        let k = pts.len() as f64;
                        pts.iter().fold([0.0; 3], |a, q| add(a, mul(*q, 1.0 / k)))
                    } else {
                        mid
                    };
                    let tm = dot(u, sub(inside, pp));
                    let mut ts = Vec::new();
                    for (j, o) in gs.iter().enumerate() {
                        if j == k {
                            continue;
                        }
                        match o {
                            Some(FaceG::Plane { n, p, .. }) if dot(*n, u).abs() > 1e-9 => {
                                ts.push(dot(*n, sub(*p, pp)) / dot(*n, u));
                            }
                            Some(FaceG::Fillet { r, a, b }) => {
                                if let Some(t) = fillet_end(*r, *a, *b, [(n0, p0), (n1, p1)], pp, u) {
                                    ts.push(t);
                                }
                            }
                            _ => {}
                        }
                    }
                    let lo = ts.iter().copied().filter(|t| *t < tm - 1e-9).fold(f64::NEG_INFINITY, f64::max);
                    let hi = ts.iter().copied().filter(|t| *t > tm + 1e-9).fold(f64::INFINITY, f64::min);
                    if !(lo.is_finite() && hi.is_finite()) {
                        return None;
                    }
                    Some((add(pp, mul(u, (lo + hi) / 2.0)), false, Vec::new()))
                })(),
                _ => None,
            };
            out.push(e);
        }
        out
    }

    /// A planar face, modelled or stored: its unit normal and a point.
    fn plane_of(&mut self, f: &FaceName) -> Option<(V3, V3)> {
        let (g, ops) = f.first()?;
        match self.face(g, ops) {
            Some(FaceG::Plane { n, p, .. }) => Some((unit(n)?, p)),
            _ => None,
        }
    }

    /// A cylindrical face the model does not give, as the stored bodies hold it: axis point,
    /// unit axis and radius (a fillet's round face where its sides are not modelled planes).
    fn stored_cylinder(&mut self, f: &FaceName) -> Option<(V3, V3, f64)> {
        let (g, ops) = f.first()?;
        if self.face(g, ops).is_some() {
            return None;
        }
        match self.stored_face(g, ops)?.surface {
            Surface::Cylinder { origin, axis, radius } if radius.is_finite() && radius > 0.0 => {
                Some((origin, unit(axis)?, radius))
            }
            _ => None,
        }
    }

    /// The edges round a pocket whose corners a fillet rounded, where the fillet's faces are
    /// known only as the stored cylinders (6252's Chamfer3: the rim of a notch in a wall whose
    /// corners Fillet rounds by 1 mm, the notch's sides a cut and a chamfer face). Two kinds:
    /// * a line where two planes meet, each end a plane or a stored cylinder about an axis
    ///   across the line that the line touches (`r` from its axis): it ends at the touching point;
    /// * an arc where a plane meets a stored cylinder about an axis across it, ended by two planes
    ///   the cylinder touches: the shorter arc between the touching points, given with its points.
    fn edge_by_round_ends(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let (f0, f1) = (faces.first()?, faces.get(1)?);
        let ends: Vec<&FaceName> = faces.iter().skip(2).collect();
        if ends.len() != 2 {
            return None;
        }
        let cyls: Vec<Option<(V3, V3, f64)>> = faces.iter().map(|f| self.stored_cylinder(f)).collect();
        if !cyls.iter().any(Option::is_some) {
            return None;
        }
        if let (Some((n0, p0)), Some((n1, p1))) = (self.plane_of(f0), self.plane_of(f1)) {
            let (pp, u) = plane_plane(n0, p0, n1, p1)?;
            let mut ts = Vec::new();
            for (k, e) in ends.iter().enumerate() {
                if let Some((o, a, r)) = cyls.get(k + 2).copied().flatten() {
                    if dot(a, u).abs() > 1e-6 {
                        return None;
                    }
                    let t = dot(u, sub(o, pp));
                    // The line touches the cylinder: its distance from the axis is the radius.
                    let w = sub(add(pp, mul(u, t)), o);
                    if (norm(sub(w, mul(a, dot(w, a)))) - r).abs() > 1e-6 {
                        return None;
                    }
                    ts.push(t);
                } else if let Some((e0, eops)) = e.first()
                    && let Some(FaceG::Fillet { r, a, b }) = self.face(e0, eops)
                {
                    ts.push(fillet_end(r, a, b, [(n0, p0), (n1, p1)], pp, u)?);
                } else {
                    let (n, p) = self.plane_of(e)?;
                    if dot(n, u).abs() < 1e-9 {
                        return None;
                    }
                    ts.push(dot(n, sub(p, pp)) / dot(n, u));
                }
            }
            let [t0, t1] = ts.as_slice() else { return None };
            if (t1 - t0).abs() < 1e-7 {
                return None;
            }
            return Some((add(pp, mul(u, (t0 + t1) / 2.0)), false, Vec::new()));
        }
        // A plane and a stored cylinder about an axis across it.
        let ((n, p), (o, a, r), round) = match (cyls.first().copied().flatten(), cyls.get(1).copied().flatten()) {
            (None, Some(c)) => (self.plane_of(f0)?, c, f1),
            (Some(c), None) => (self.plane_of(f1)?, c, f0),
            _ => return None,
        };
        if (dot(n, a).abs() - 1.0).abs() > 1e-9 {
            return None;
        }
        let c = add(o, mul(a, dot(n, sub(p, o)) / dot(n, a)));
        let mut touch = Vec::new();
        for e in &ends {
            let (m, q) = self.plane_of(e)?;
            let d = dot(m, sub(c, q));
            if dot(m, a).abs() > 1e-9 || (d.abs() - r).abs() > 1e-6 {
                return None;
            }
            touch.push(sub(c, mul(m, d)));
        }
        let [t0, t1] = touch.as_slice() else { return None };
        let (v0, v1) = (sub(*t0, c), sub(*t1, c));
        let mut bis = add(v0, v1);
        let mut ang = libm::acos((dot(v0, v1) / (r * r)).clamp(-1.0, 1.0));
        if norm(bis) < 1e-6 * r {
            // A half circle (the round end of a thin wall, 5998's Chamfer7): it bulges to the side
            // of the cylinder's stored points.
            let (g, ops) = round.first()?;
            let pts = self.stored_face(g, ops)?.points;
            let k = pts.len() as f64;
            let w = sub(pts.iter().fold([0.0; 3], |m, q| add(m, mul(*q, 1.0 / k))), c);
            let w = sub(w, mul(a, dot(w, a)));
            let w = sub(w, mul(v0, dot(w, v0) / (r * r)));
            if pts.is_empty() || norm(w) < 1e-3 * r {
                return None;
            }
            bis = w;
            ang = std::f64::consts::PI;
        }
        let mid = add(c, mul(unit(bis)?, r));
        let w = unit(cross(a, v0))?;
        // The turn from v0 towards the middle about the axis.
        let sgn = if dot(cross(v0, bis), a) >= 0.0 { 1.0 } else { -1.0 };
        let k = 16;
        let path: Vec<V3> = (0..=k)
            .map(|i| {
                let t = ang * f64::from(i) / f64::from(k);
                add(c, add(mul(v0, libm::cos(t)), mul(w, sgn * r * libm::sin(t))))
            })
            .collect();
        Some((mid, true, path))
    }

    /// A closed edge (no faces bound its ends) between a cylinder and a cone about the same axis,
    /// both as the stored bodies hold them (6118's Fillet2: the round side of an extrude, r 11.75
    /// mm, and the 45 degree cone of a revolve about the extrude's axis): the circle where the
    /// cone reaches the cylinder's radius, given as points all round it like [`Ctx::rim_at_plane`].
    fn rim_at_cone(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let [f0, f1] = faces else { return None };
        let (g0, ops0) = f0.first()?;
        let (g1, ops1) = f1.first()?;
        let s0 = self.stored_face(g0, ops0)?.surface;
        let s1 = self.stored_face(g1, ops1)?.surface;
        let (cyl, cone) = match (&s0, &s1) {
            (Surface::Cylinder { .. }, Surface::Cone { .. }) => (s0, s1),
            (Surface::Cone { .. }, Surface::Cylinder { .. }) => (s1, s0),
            _ => return None,
        };
        let (Surface::Cylinder { origin, axis, radius }, Surface::Cone { origin: co, axis: ca, radius: cr, sin, cos }) =
            (cyl, cone)
        else {
            return None;
        };
        let (axis, ca) = (unit(axis)?, unit(ca)?);
        let off = sub(co, origin);
        let across = sub(off, mul(axis, dot(off, axis)));
        if (dot(axis, ca).abs() - 1.0).abs() > 1e-9 || norm(across) > 1e-7 || sin.abs() < 1e-6 || cos.abs() < 1e-6 {
            return None;
        }
        if !(radius.is_finite() && radius > 0.0) {
            return None;
        }
        // The cone's radius at height h along its axis is `cr + h sin / cos`.
        let h = (radius - cr) * cos / sin;
        let centre = add(co, mul(ca, h));
        let u = unit(cross(ca, if ca[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }))?;
        let v = cross(ca, u);
        let step = libm::acos((1.0 - 5e-4 / radius).clamp(-1.0, 1.0)) * 2.0;
        let k = ((2.0 * std::f64::consts::PI / step.max(1e-3)).ceil() as usize).clamp(16, 256);
        let at = |t: f64| add(centre, add(mul(u, radius * libm::cos(t)), mul(v, radius * libm::sin(t))));
        let path: Vec<V3> = (0..=k).map(|i| at(2.0 * std::f64::consts::PI * i as f64 / k as f64)).collect();
        Some((at(std::f64::consts::PI), true, path))
    }

    /// An edge named by a straight side of an extrude and a cap of the same extrude, and by
    /// nothing that bounds it (6299's `Chamfer1 (2)`), where neither the model nor the stored
    /// bodies say more: the whole side at the cap, so its middle is the side's chord middle moved
    /// onto the cap. A model edge that is only part of it (the side cut by a later feature) does
    /// not match that middle, so the import stops there.
    fn side_at_cap(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let [f0, f1] = faces else { return None };
        let (g0, ops0) = f0.first()?;
        let (g1, ops1) = f1.first()?;
        if ops0 != ops1 || f0.len() != 1 || f1.len() != 1 {
            return None;
        }
        let is_cap = |g: &str| g == "1" || g == "2";
        let (side, cap) = match (is_cap(g0), is_cap(g1)) {
            (false, true) => ((g0, ops0), (g1, ops1)),
            (true, false) => ((g1, ops1), (g0, ops0)),
            _ => return None,
        };
        let (FaceG::Plane { n: sn, mid: sm, cap: false, .. }, FaceG::Plane { n: cn, p: cp, cap: true, .. }) =
            (self.face(side.0, side.1)?, self.face(cap.0, cap.1)?)
        else {
            return None;
        };
        if dot(cn, sn).abs() > 1e-6 {
            return None;
        }
        let t = dot(cn, sub(cp, sm));
        Some((add(sm, mul(cn, t)), false, Vec::new()))
    }

    fn modelled_edge(&mut self, faces: &[FaceName]) -> Option<EdgeAt> {
        let fs: Vec<Option<FaceG>> = faces.iter().map(|f| f.first().and_then(|(g, ops)| self.face(g, ops))).collect();
        let is_fillet = |f: &Option<FaceG>| matches!(f, Some(FaceG::Fillet { .. }));
        if fs.iter().any(is_fillet) {
            if let Some((m, arc)) = fillet_edge(&fs) {
                return Some((m, arc, Vec::new()));
            }
            // Two planes ended by a fillet and another face: the ends below.
            if fs.iter().take(2).any(is_fillet) {
                return None;
            }
        }
        let (a, b) = (fs.first()?.clone()?, fs.get(1)?.clone()?);
        if let (FaceG::Plane { n: n0, p: p0, .. }, FaceG::Plane { n: n1, p: p1, .. }) = (&a, &b) {
            let (pp, u) = plane_plane(*n0, *p0, *n1, *p1)?;
            let mut ts = Vec::new();
            let mut crossings: Vec<Vec<f64>> = Vec::new();
            for f in fs.iter().skip(2).flatten() {
                if let FaceG::Plane { n, p, .. } = f
                    && dot(*n, u).abs() > 1e-9
                {
                    ts.push(dot(*n, sub(*p, pp)) / dot(*n, u));
                }
                // An arc side the line touches (a straight side running into an arc of its
                // profile): the point of tangency.
                if let FaceG::Cyl { feature, seg, pl, .. } = f
                    && let Some(t) = self.tangent_at(*feature, seg, pl, pp, u)
                {
                    ts.push(t);
                }
                // A fillet face that rounds the corner of one of the edge's planes with another
                // plane: the line ends where the fillet leaves that plane, `r` from the other
                // (770: the bottom rim of a box between its side, rounded at the corners, and a
                // later join that interrupts it).
                if let FaceG::Fillet { r, a, b } = f {
                    if let Some(t) = fillet_end(*r, *a, *b, [(*n0, *p0), (*n1, *p1)], pp, u) {
                        ts.push(t);
                    } else {
                        crossings.push(fillet_crossings(*r, *a, *b, pp, u));
                    }
                }
            }
            // A fillet of two other planes the line runs into (6124's `Fillet4`: a tab's side
            // meets the box's corner round `Fillet1` made): where it enters the round, the
            // crossing nearest the edge's other ends.
            if !ts.is_empty() {
                let k = ts.len() as f64;
                let centre = ts.iter().sum::<f64>() / k;
                for c in &crossings {
                    if let Some(t) = c.iter().copied().min_by(|x, y| (x - centre).abs().total_cmp(&(y - centre).abs()))
                    {
                        ts.push(t);
                    }
                }
            }
            if ts.len() >= 2 {
                let lo = ts.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = ts.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if hi - lo > 1e-7 {
                    return Some((add(pp, mul(u, (lo + hi) / 2.0)), false, Vec::new()));
                }
            }
            // A side of an extrude standing on a perpendicular plane, bounded by that same
            // extrude's neighbouring sides (arcs among them): the edge is the whole segment at
            // the plane, so its middle is the side's chord middle moved onto the plane.
            for ((wn, wp), (sn, sm, sface)) in [(&a, &b), (&b, &a)].into_iter().filter_map(|(w, s)| {
                let (FaceG::Plane { n: wn, p: wp, .. }, FaceG::Plane { n: sn, mid, cap: false, .. }) = (w, s) else {
                    return None;
                };
                Some(((*wn, *wp), (*sn, *mid, if std::ptr::eq(s, &a) { 0 } else { 1 })))
            }) {
                let side_ops = &faces.get(sface)?.first()?.1;
                let bounded_by_own = faces
                    .iter()
                    .skip(2)
                    .all(|f| f.first().is_some_and(|(g, ops)| ops == side_ops && g != "1" && g != "2"));
                if bounded_by_own && faces.len() > 2 && dot(wn, sn).abs() < 1e-6 {
                    let t = dot(wn, sub(wp, sm));
                    return Some((add(sm, mul(wn, t)), false, Vec::new()));
                }
            }
            return None;
        }
        // A cap meeting an arc side: the middle of the arc piece at the cap. A side of another
        // extrude across the arc side's axis cuts the whole piece the same way, when the edge
        // ends on the arc's own neighbouring sides (5981: a slot cut into a wall).
        for (k, (cap, cyl)) in [(&a, &b), (&b, &a)].into_iter().enumerate() {
            let (FaceG::Plane { n, p, cap: is_cap, .. }, FaceG::Cyl { axis, feature, seg, pl, .. }) = (cap, cyl) else {
                continue;
            };
            if !is_cap {
                let cyl_ops = &faces.get(1 - k)?.first()?.1;
                let own_ends = faces.len() > 2
                    && faces
                        .iter()
                        .skip(2)
                        .all(|f| f.first().is_some_and(|(g, ops)| ops == cyl_ops && g != "1" && g != "2"));
                if !own_ends {
                    continue;
                }
            }
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
            let path = poly.iter().map(|q| world(pl, *q, tcap)).collect();
            return Some((world(pl, m2, tcap), true, path));
        }
        None
    }
}
