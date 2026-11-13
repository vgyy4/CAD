//! An Inventor part's feature history as a Gusset feature tree (`gusset-feature-tree` v2),
//! read from the `PmDCSegment` records of the `.ipt` and, for fillets and chamfers, from the
//! ShapeManager result body. Clean room, from sample bytes (`docs/formats/inventor-feature-tree.md`).
//!
//! What is read: sketches (a mask-compressed 4x4 placement in cm, points, lines, circles and
//! arcs), extrudes, revolves, holes, fillets and chamfers, in history order. Fillet and chamfer
//! edges are not stored as geometry: the result body's blend faces carry the feature's id and
//! the original edge is rebuilt from the faces beside them (see `acis`).
//!
//! The reader does not judge the tree. It is only a candidate: the caller rebuilds it and
//! keeps it when it gives the part's own body. A hole carries `_fallback`, the same hole as
//! plain cuts, for the caller to try when the hole feature itself is refused (keys that start
//! with `_` are for the caller and must be removed before the tree is parsed).
//!
//! Input is untrusted: every count, list, chain and walk is bounded, and a record that is not
//! shaped as expected gives an `unsupported` feature or an error, never a panic.

use crate::acis::{Acis, BridgedRound, EdgeInfo, FullRound, Surf, V3, cross, dot, local_path};
use crate::inventor_dc::{Dc, FeatType, Feature, Role, f64_at, u16_at, u32_at};
use crate::inventor_profile::{P2, Poly, picks};
use crate::{Limits, inventor};
use serde_json::{Value, json};
use std::cell::OnceCell;
use std::collections::HashMap;

/// Centimetres to millimetres.
const CM: f64 = 10.0;
/// Most features read from one part.
const MAX_FEATURES: usize = 2000;
/// Most curves of one sketch.
const MAX_CURVES: usize = 20_000;
/// How far either side of a blend edge's midpoint its pick's path reaches, in file units (cm): 0.1 mm.
const PATH_EPS: f64 = 0.01;
/// A through-all extent, in millimetres.
const THROUGH: f64 = 1e4;

fn rnd(v: f64) -> f64 {
    rnd_to(v, 1e9)
}

fn rnd_to(v: f64, scale: f64) -> f64 {
    libm::round(v * scale) / scale + 0.0
}

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn unit3(a: V3) -> V3 {
    let l = libm::sqrt(dot(a, a));
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Sets a key of a JSON object.
fn put(o: &mut Value, key: &str, v: Value) {
    if let Some(m) = o.as_object_mut() {
        m.insert(key.to_string(), v);
    }
}

fn arr(v: V3) -> Value {
    json!([rnd(v[0]), rnd(v[1]), rnd(v[2])])
}

/// Origin (mm) and axes of a placement matrix.
fn frame(m: &[f64; 16]) -> (V3, V3, V3) {
    let g = |i: usize| m.get(i).copied().unwrap_or(0.0);
    ([g(3) * CM, g(7) * CM, g(11) * CM], [g(0), g(4), g(8)], [g(1), g(5), g(9)])
}

/// A feature's slots with one record's bytes at hand.
struct Slots(Vec<Option<usize>>);

impl Slots {
    fn get(&self, j: usize) -> Option<usize> {
        self.0.get(j).copied().flatten()
    }
    fn need(&self, j: usize) -> Result<usize, String> {
        self.get(j).ok_or_else(|| format!("slot {j} is empty"))
    }
}

/// u16 at the end of a record (operation, extent).
fn tail_u16(dc: &Dc, i: usize) -> Option<u16> {
    let r = dc.r(i);
    u16_at(r, r.len().checked_sub(2)?)
}

struct Sketch {
    name: String,
    m: [f64; 16],
    points: Vec<(usize, f64, f64, bool)>,
    /// (kind, id, [point records], radius, construction)
    curves: Vec<Curve>,
}

enum Curve {
    Line {
        id: usize,
        a: usize,
        b: usize,
        con: bool,
    },
    Arc {
        id: usize,
        c: usize,
        s: usize,
        e: usize,
        con: bool,
    },
    Circle {
        id: usize,
        c: usize,
        r: f64,
        con: bool,
    },
    /// Centre point, unit major direction, semi-axes (mm).
    Ellipse {
        id: usize,
        c: usize,
        major: [f64; 2],
        a: f64,
        b: f64,
        con: bool,
    },
}

impl Sketch {
    fn json(&self) -> Value {
        let (o, x, y) = frame(&self.m);
        let points: Vec<Value> = self
            .points
            .iter()
            .map(|&(i, u, v, con)| {
                let mut p = json!({"id": format!("p{i}"), "x": u, "y": v});
                if con {
                    put(&mut p, "construction", json!(true));
                }
                p
            })
            .collect();
        let curves: Vec<Value> = self
            .curves
            .iter()
            .map(|c| {
                let (mut j, con) = match *c {
                    Curve::Line { id, a, b, con } => {
                        (json!({"id": format!("l{id}"), "type": "line", "start": format!("p{a}"), "end": format!("p{b}")}), con)
                    }
                    Curve::Arc { id, c, s, e, con } => (
                        json!({"id": format!("a{id}"), "type": "arc", "center": format!("p{c}"), "start": format!("p{s}"), "end": format!("p{e}")}),
                        con,
                    ),
                    Curve::Circle { id, c, r, con } => {
                        (json!({"id": format!("c{id}"), "type": "circle", "center": format!("p{c}"), "radius": r}), con)
                    }
                    Curve::Ellipse { id, c, major, a, b, con } => (
                        json!({"id": format!("e{id}"), "type": "ellipse", "center": format!("p{c}"), "major": major, "a": a, "b": b}),
                        con,
                    ),
                };
                if con {
                    put(&mut j, "construction", json!(true));
                }
                j
            })
            .collect();
        json!({"type": "sketch", "name": self.name,
               "plane": {"origin": arr(o), "x_axis": arr(x), "y_axis": arr(y)},
               "points": points, "curves": curves})
    }

    /// The non-construction curves in the sketch plane (mm), for the profile picks.
    fn polys(&self) -> Vec<Poly> {
        let p: HashMap<usize, P2> = self.points.iter().map(|&(i, u, v, _)| (i, (u, v))).collect();
        let mut out = Vec::new();
        for c in &self.curves {
            match *c {
                Curve::Line { a, b, con: false, .. } => {
                    if let (Some(&a), Some(&b)) = (p.get(&a), p.get(&b)) {
                        out.push(Poly::Line(a, b));
                    }
                }
                Curve::Circle { c, r, con: false, .. } => {
                    if let Some(&c) = p.get(&c) {
                        out.push(Poly::Circle(c, r));
                    }
                }
                Curve::Arc { c, s, e, con: false, .. } => {
                    if let (Some(&c), Some(&s), Some(&e)) = (p.get(&c), p.get(&s), p.get(&e)) {
                        out.push(Poly::Arc { c, s, e });
                    }
                }
                Curve::Ellipse { c, major, a, b, con: false, .. } => {
                    if let Some(&c) = p.get(&c) {
                        out.push(Poly::Ellipse { c, major: (major[0], major[1]), a, b });
                    }
                }
                _ => {}
            }
        }
        out
    }
}

struct Reader<'a> {
    dc: Dc,
    bytes: &'a [u8],
    limits: &'a Limits,
    /// Records that refer to each record.
    back: OnceCell<Vec<Vec<usize>>>,
    /// Sketch contents by sketch record.
    owned: OnceCell<HashMap<usize, Vec<usize>>>,
    history: OnceCell<Option<Acis>>,
    result: OnceCell<Option<Acis>>,
}

impl Reader<'_> {
    fn back(&self) -> &Vec<Vec<usize>> {
        self.back.get_or_init(|| {
            let mut br = vec![Vec::new(); self.dc.n()];
            for i in 0..self.dc.n() {
                for (_, x) in self.dc.refs(i) {
                    if let Some(l) = br.get_mut(x) {
                        l.push(i);
                    }
                }
            }
            br
        })
    }

    fn owner(&self, i: usize) -> Option<usize> {
        super::inventor_dc::reference(u32_at(self.dc.r(i), self.dc.body(i) + 4)?, self.dc.n())
    }

    fn owned(&self, s: usize) -> &[usize] {
        let m = self.owned.get_or_init(|| {
            let mut m: HashMap<usize, Vec<usize>> = HashMap::new();
            for i in 0..self.dc.n() {
                if matches!(self.dc.role(i), Role::SkPoint | Role::SkLine | Role::SkCircle | Role::SkEllipse)
                    && let Some(o) = self.owner(i)
                {
                    m.entry(o).or_default().push(i);
                }
            }
            m
        });
        m.get(&s).map_or(&[], Vec::as_slice)
    }

    fn flags(&self, i: usize) -> u32 {
        u32_at(self.dc.r(i), self.dc.body(i)).unwrap_or(0)
    }

    fn history(&self) -> Option<&Acis> {
        self.history
            .get_or_init(|| {
                let hb = (0..self.dc.n()).find(|&i| self.dc.role(i) == Role::HistoryBlob)?;
                let r = self.dc.r(hb);
                let k = r.windows(14).position(|w| w == b"ASM BinaryFile")?;
                Acis::parse(r.get(k..)?).ok()
            })
            .as_ref()
    }

    fn result(&self) -> Option<&Acis> {
        self.result
            .get_or_init(|| {
                let e = inventor::extract_sab(self.bytes, self.limits).ok()?;
                Acis::parse(&e.sab).ok()
            })
            .as_ref()
    }

    fn sketch(&self, s: usize) -> Result<Sketch, String> {
        let r = self.dc.refs(s);
        let placement = r.iter().map(|&(_, x)| x).find(|&x| self.dc.role(x) == Role::SkPlacement);
        let m = match placement {
            Some(p) => self.dc.placement(p).ok_or("a sketch placement is not readable")?,
            None => [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        };
        let mut sk = Sketch {
            name: self.dc.name(s).unwrap_or_else(|| format!("Sketch{s}")),
            m,
            points: Vec::new(),
            curves: Vec::new(),
        };
        let mut ids: HashMap<usize, ()> = HashMap::new();
        let items = self.owned(s);
        for &i in items {
            if self.dc.role(i) != Role::SkPoint {
                continue;
            }
            let rec = self.dc.r(i);
            let b = self.dc.body(i);
            let (Some(u), Some(v)) = (f64_at(rec, b + 8), f64_at(rec, b + 16)) else {
                return Err("a sketch point is cut short".into());
            };
            ids.insert(i, ());
            sk.points.push((i, rnd(u * CM), rnd(v * CM), self.flags(i) & 0x40 != 0));
        }
        for &i in items {
            let role = self.dc.role(i);
            if !matches!(role, Role::SkLine | Role::SkCircle | Role::SkEllipse) {
                continue;
            }
            if sk.curves.len() >= MAX_CURVES {
                return Err("the sketch has too many curves".into());
            }
            let con = self.flags(i) & 0x80000 != 0;
            let list = self.dc.first_list(i, None).map(|x| x.0).unwrap_or_default();
            let known = |t: &Option<usize>| t.is_some_and(|t| ids.contains_key(&t));
            if role == Role::SkLine {
                if let (Some(Some(a)), Some(Some(b)), true) =
                    (list.first(), list.get(1), list.iter().take(2).all(known))
                {
                    sk.curves.push(Curve::Line { id: i, a: *a, b: *b, con });
                }
                continue;
            }
            if role == Role::SkEllipse {
                if let Some(c) = self.ellipse(i, &sk.points, con) {
                    sk.curves.push(c);
                }
                continue;
            }
            let rec = self.dc.r(i);
            let (Some(rad), Some(cen)) = (
                rec.len().checked_sub(9).and_then(|k| f64_at(rec, k)),
                rec.len()
                    .checked_sub(13)
                    .and_then(|k| u32_at(rec, k))
                    .and_then(|v| super::inventor_dc::reference(v, self.dc.n())),
            ) else {
                continue;
            };
            if !ids.contains_key(&cen) {
                continue;
            }
            if let (2, Some(Some(s0)), Some(Some(e0)), true) =
                (list.len(), list.first(), list.get(1), list.iter().all(known))
            {
                sk.curves.push(Curve::Arc { id: i, c: cen, s: *s0, e: *e0, con });
            } else {
                sk.curves.push(Curve::Circle { id: i, c: cen, r: rnd(rad * CM), con });
            }
        }
        Ok(sk)
    }

    /// An ellipse record: the tail holds `f64 s, a, b, u8` (cm; `s` = y component of the major
    /// axis direction, `a` >= `b`), and it refers to its centre and axis-end points. The centre
    /// is the referred point from which the others lie `a` / `b` away; the x sign of the
    /// direction comes from the axis-end point when there is one, else it is taken as positive.
    fn ellipse(&self, i: usize, points: &[(usize, f64, f64, bool)], con: bool) -> Option<Curve> {
        let rec = self.dc.r(i);
        let k = rec.len().checked_sub(25)?;
        let (s, a, b) = (f64_at(rec, k)?, f64_at(rec, k + 8)? * CM, f64_at(rec, k + 16)? * CM);
        if !(a > 0.0 && b > 0.0 && a >= b && s.abs() <= 1.0 + 1e-9) {
            return None;
        }
        let at = |id: usize| points.iter().find(|p| p.0 == id).map(|p| (p.1, p.2));
        let refd: Vec<(usize, (f64, f64))> = self
            .dc
            .refs(i)
            .into_iter()
            .filter(|&(_, x)| self.dc.role(x) == Role::SkPoint)
            .filter_map(|(_, x)| Some((x, at(x)?)))
            .collect();
        let near = |p: (f64, f64), q: (f64, f64), d: f64| (libm::hypot(p.0 - q.0, p.1 - q.1) - d).abs() < 1e-4;
        let centre = if let [only] = refd.as_slice() {
            *only
        } else {
            *refd
                .iter()
                .find(|&&(_, c)| refd.iter().all(|&(_, q)| near(q, c, 0.0) || near(q, c, a) || near(q, c, b)))?
        };
        let sy = s.clamp(-1.0, 1.0);
        let cx = libm::sqrt((1.0 - sy * sy).max(0.0));
        // The major-axis end, when it is among the points, fixes the x sign.
        let sx = refd
            .iter()
            .find(|&&(_, q)| near(q, centre.1, a) && ((q.1 - centre.1.1) / a - sy).abs() < 1e-4)
            .map_or(cx, |&(_, q)| if cx > 1e-9 { cx * ((q.0 - centre.1.0).signum()) } else { cx });
        Some(Curve::Ellipse { id: i, c: centre.0, major: [rnd(sx), rnd(sy)], a: rnd(a), b: rnd(b), con })
    }

    /// The sketch whose profile a loops list belongs to.
    fn profile_sketch(&self, lst: usize) -> Option<usize> {
        for &j in self.back().get(lst)? {
            if self.dc.class(j) == super::inventor_dc::G_PROFILE {
                for (_, x) in self.dc.refs(j) {
                    if self.dc.role(x) == Role::Sketch {
                        return Some(x);
                    }
                }
            }
        }
        None
    }

    /// The history-body tags of the loops in a loops list.
    fn loop_tags(&self, lst: usize) -> Option<Vec<i64>> {
        let (items, _) = self.dc.first_list(lst, None)?;
        let mut out = Vec::new();
        for it in items {
            let x = it.filter(|&x| self.dc.class(x) == super::inventor_dc::G_LOOP)?;
            let refs = self.dc.refs(x);
            let c = refs.iter().map(|&(_, y)| y).rfind(|&y| self.dc.class(y) == super::inventor_dc::G_LOOP_BODY)?;
            let hb = self.dc.refs(c).into_iter().find(|&(_, y)| self.dc.role(y) == Role::HistoryBlob)?.0;
            out.push(i64::from(u32_at(self.dc.r(c), hb + 4)?));
        }
        Some(out)
    }

    /// `"all"`, or one point per region the feature's loops surround.
    fn profiles(&self, sk: &Sketch, lst: usize) -> Value {
        let (Some(ac), Some(tags)) = (self.history(), self.loop_tags(lst)) else { return json!("all") };
        if tags.is_empty() {
            return json!("all");
        }
        let (o, x, y) = ([sk.m[3], sk.m[7], sk.m[11]], [sk.m[0], sk.m[4], sk.m[8]], [sk.m[1], sk.m[5], sk.m[9]]);
        let mut l2: Vec<Vec<P2>> = Vec::new();
        for t in tags {
            let Some(ls) = ac.loops_of_body(t) else { return json!("all") };
            for l in ls {
                l2.push(
                    l.into_iter()
                        .map(|p| {
                            let q = sub3(p, o);
                            (dot(q, x) * CM, dot(q, y) * CM)
                        })
                        .collect(),
                );
            }
        }
        if l2.is_empty() {
            return json!("all");
        }
        match picks(&sk.polys(), &l2) {
            Some(pk) if !pk.is_empty() => Value::Array(pk.into_iter().map(|(a, b)| json!({"point": [a, b]})).collect()),
            _ => json!("all"),
        }
    }

    /// How far an extrude that ends on a face (extent 4, to the next face, or 7, to a chosen
    /// face) reached, in mm along its sketch's normal: the face is named through the naming
    /// records, so the reach is measured instead on the faces the feature made in the result
    /// body (side faces and caps carry its id). All their corners must lie on one side of the
    /// sketch plane; the farthest is the reach. A later feature that trimmed every face back,
    /// or an end face that is not parallel to the sketch, gives a reach the rebuild check
    /// refuses.
    fn swept_distance(&self, sk: &Sketch, fid: i64) -> Result<f64, String> {
        let b = self.result().ok_or("the result body is not readable")?;
        let faces = b.sweep_faces(fid);
        if faces.is_empty() {
            return Err("no face of the feature is in the result body".into());
        }
        let (o, x, y) = frame(&sk.m);
        let n = unit3(cross(x, y));
        let (mut lo, mut hi) = (0.0f64, 0.0f64);
        for f in faces {
            for v in b.verts(f) {
                let t = dot(sub3([v[0] * CM, v[1] * CM, v[2] * CM], o), n);
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
        match (lo < -1e-6, hi > 1e-6) {
            (true, true) => Err("the feature's faces lie on both sides of its sketch".into()),
            (false, false) => Err("the feature's faces lie in its sketch plane".into()),
            (true, false) => Ok(lo),
            (false, true) => Ok(hi),
        }
    }

    fn slots(&self, f: &Feature) -> Result<Slots, String> {
        self.dc.slots(f.rec).map(Slots).ok_or_else(|| "the feature has no slot list".into())
    }

    fn param(&self, s: &Slots, j: usize) -> Result<f64, String> {
        let i = s.need(j)?;
        self.dc.param(i).map(|p| p.1).ok_or_else(|| format!("slot {j} is not a parameter"))
    }

    fn pval(&self, s: &Slots, j: usize) -> Result<f64, String> {
        self.dc.pval(&s.0, j).ok_or_else(|| format!("slot {j} is not a parameter"))
    }
}

/// What a hole's plain-cut form is made from (kept to remake it when a pattern moves the hole).
struct HoleInfo {
    nd: V3,
    diameter: f64,
    through: bool,
    depth: f64,
    point_angle: Option<f64>,
    cbore: Option<(f64, f64)>,
}

/// Everything decoded so far.
struct State {
    out: Vec<Value>,
    done: HashMap<usize, ()>,
    first_body: Option<String>,
    omitted: Vec<Value>,
    /// Feature record -> its name.
    names: HashMap<usize, String>,
    holes: HashMap<String, HoleInfo>,
}

fn solid(r: &Reader, st: &mut State, f: &Feature, name: &str, typ: FeatType) -> Result<(), String> {
    let s = r.slots(f)?;
    let lst = s.need(1)?;
    let sk = r.profile_sketch(lst).ok_or("no sketch found")?;
    let sketch = r.sketch(sk)?;
    let mut op = match s.get(0).and_then(|i| tail_u16(&r.dc, i)) {
        Some(2) => "cut",
        Some(1) => "new",
        Some(4) => "intersect",
        _ => "join",
    };
    if st.first_body.is_some() && op == "new" {
        op = "join";
    }
    if st.first_body.is_none() && op == "join" {
        op = "new";
    }
    let mut j = json!({"type": if typ == FeatType::Extrude {"extrude"} else {"revolve"}, "name": name,
                       "sketch": sketch.name, "profiles": r.profiles(&sketch, lst), "operation": op});
    if op != "new" {
        let fb = st.first_body.clone().ok_or("a cut or join before any body")?;
        put(&mut j, "target", json!(fb));
    }
    let ext = |i: Option<usize>| i.and_then(|i| tail_u16(&r.dc, i));
    if typ == FeatType::Extrude {
        let rev = s.get(3).and_then(|i| r.dc.r(i).last().copied()) == Some(1);
        let dist = r.param(&s, 4)? * CM;
        match ext(s.get(6)) {
            Some(2) => {
                put(&mut j, "distance", json!(rnd(dist)));
                put(&mut j, "symmetric", json!(true));
            }
            Some(1) => {
                put(&mut j, "distance", json!(rnd(if rev { -dist } else { dist })));
                put(&mut j, "symmetric", json!(false));
            }
            Some(5) => {
                put(&mut j, "distance", json!(rnd(if rev { -THROUGH } else { THROUGH })));
                put(&mut j, "symmetric", json!(false));
                put(&mut j, "through_all", json!(true));
            }
            Some(e @ (4 | 7)) => {
                let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
                let d = r.swept_distance(&sketch, fid).map_err(|w| format!("extrude extent {e}: {w}"))?;
                put(&mut j, "distance", json!(rnd(d)));
                put(&mut j, "symmetric", json!(false));
            }
            e => return Err(format!("extrude extent {e:?}")),
        }
    } else {
        let ax = s.need(2)?;
        let rec = r.dc.r(ax);
        let k = rec
            .get(r.dc.body(ax)..)
            .and_then(|t| t.windows(4).position(|w| w == [0x02, 0, 0, 0x30]))
            .map(|k| k + r.dc.body(ax))
            .ok_or("the revolve axis is not readable")?;
        let g = |o: usize| f64_at(rec, k + o).ok_or("the revolve axis is cut short");
        let p0 = [g(8)?, g(16)?, g(24)?];
        let v = [g(32)?, g(40)?, g(48)?];
        let ln = libm::sqrt(dot(v, v));
        put(
            &mut j,
            "axis",
            json!({"origin": [rnd(p0[0] * CM), rnd(p0[1] * CM), rnd(p0[2] * CM)],
                           "direction": [rnd(v[0] / ln), rnd(v[1] / ln), rnd(v[2] / ln)]}),
        );
        put(
            &mut j,
            "angle",
            match ext(s.get(3)) {
                Some(3) => json!(360.0),
                _ => json!(rnd(r.param(&s, 4)?.to_degrees())),
            },
        );
    }
    if !st.done.contains_key(&sk) {
        st.out.push(sketch.json());
        st.done.insert(sk, ());
    }
    st.out.push(j);
    if st.first_body.is_none() {
        st.first_body = Some(name.to_string());
    }
    Ok(())
}

fn hole(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let items = r.dc.first_list(s.need(7)?, None).map(|x| x.0).ok_or("the hole has no centre list")?;
    let m = r.dc.placement(s.need(8)?).ok_or("the hole placement is not readable")?;
    let (o, x, y) = frame(&m);
    let mut pos: Vec<V3> = Vec::new();
    for it in items.into_iter().flatten() {
        if r.dc.role(it) != Role::SkPoint {
            continue;
        }
        let rec = r.dc.r(it);
        let b = r.dc.body(it);
        let (Some(u), Some(v)) = (f64_at(rec, b + 8), f64_at(rec, b + 16)) else {
            return Err("a hole centre is cut short".into());
        };
        let at = |o: f64, x: f64, y: f64| rnd(o + (u * x + v * y) * CM);
        pos.push([at(o[0], x[0], y[0]), at(o[1], x[1], y[1]), at(o[2], x[2], y[2])]);
    }
    let n = cross(x, y);
    let nd: V3 = match s.get(16) {
        Some(d) => {
            let a = r.dc.skdir(d).ok_or("the hole direction is not readable")?;
            [a[2], a[3], a[4]]
        }
        None => n,
    };
    let t0 = s.need(0)?;
    let typ = u32_at(r.dc.r(t0), r.dc.body(t0)).ok_or("the hole type is not readable")?;
    let diameter = rnd(r.pval(&s, 1)? * CM);
    let mut j = json!({"type": "hole", "name": name,
        "face": {"point": arr(pos.first().copied().unwrap_or(o)), "normal": arr(nd)},
        "positions": pos.iter().map(|p| arr(*p)).collect::<Vec<_>>(), "diameter": diameter});
    let mut cb = None;
    if typ & 0x20000 != 0 {
        let (d, h) = (rnd(r.pval(&s, 3)? * CM), rnd(r.pval(&s, 4)? * CM));
        put(&mut j, "variant", json!("counterbore"));
        put(&mut j, "cbore_diameter", json!(d));
        put(&mut j, "cbore_depth", json!(h));
        cb = Some((d, h));
    }
    let through = s.get(9).and_then(|i| tail_u16(&r.dc, i)) == Some(5);
    let mut depth = 0.0;
    let mut point_angle = None;
    if through {
        put(&mut j, "through", json!(true));
    } else {
        depth = rnd(r.pval(&s, 2)? * CM);
        put(&mut j, "depth", json!(depth));
        if let Ok(pa) = r.pval(&s, 6)
            && pa != 0.0
        {
            let a = rnd_to(pa.to_degrees(), 1e6);
            put(&mut j, "point_angle", json!(a));
            point_angle = Some(a);
        }
    }
    let info = HoleInfo { nd, diameter, through, depth, point_angle, cbore: cb };
    if let Some(target) = &st.first_body
        && let Some(fb) = hole_fallback(name, &pos, &info, target)
    {
        put(&mut j, "_fallback", Value::Array(fb));
    }
    st.holes.insert(name.to_string(), info);
    st.out.push(j);
    Ok(())
}

/// The same hole as plain cuts (exact geometry, less editable): a through hole as a circle
/// sketch cut through everything, a blind one as one revolve cut per position (drill point
/// and counterbore included).
fn hole_fallback(name: &str, pos: &[V3], info: &HoleInfo, target: &str) -> Option<Vec<Value>> {
    let HoleInfo { nd, diameter, through, depth, point_angle, cbore } = *info;
    let ln = libm::sqrt(dot(nd, nd));
    if ln == 0.0 || !ln.is_finite() || pos.is_empty() {
        return None;
    }
    let n = unit3(nd);
    let a: V3 = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let x = unit3(cross(a, n));
    let y = cross(n, x);
    let r = diameter / 2.0;
    let mut out = Vec::new();
    if through {
        let o = *pos.first()?;
        let mut pts = Vec::new();
        let mut cur = Vec::new();
        for (k, p) in pos.iter().enumerate() {
            let q = sub3(*p, o);
            pts.push(json!({"id": format!("h{k}"), "x": rnd(dot(q, x)), "y": rnd(dot(q, y))}));
            cur.push(json!({"id": format!("hc{k}"), "type": "circle", "center": format!("h{k}"), "radius": r}));
        }
        let sk = format!("{name}_sketch");
        out.push(json!({"type": "sketch", "name": sk, "plane": {"origin": o, "x_axis": x, "y_axis": y}, "points": pts, "curves": cur}));
        out.push(json!({"type": "extrude", "name": name, "sketch": sk, "profiles": "all", "operation": "cut", "target": target,
                        "distance": -THROUGH, "symmetric": false, "from_hole": true}));
        return Some(out);
    }
    let tip = point_angle.map_or(0.0, |pa| r / libm::tan(pa.to_radians() / 2.0));
    let mut prof: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    match cbore {
        Some((d, cd)) => prof.extend([(d / 2.0, 0.0), (d / 2.0, -cd), (r, -cd)]),
        None => prof.push((r, 0.0)),
    }
    prof.extend([(r, -depth), (0.0, -depth - tip)]);
    for (k, p) in pos.iter().enumerate() {
        let sk = format!("{name}_{k}_sketch");
        let pts: Vec<Value> = prof
            .iter()
            .enumerate()
            .map(|(j, &(u, v))| json!({"id": format!("q{j}"), "x": rnd(u), "y": rnd(v)}))
            .collect();
        let cur: Vec<Value> = (0..prof.len())
            .map(|j| json!({"id": format!("e{j}"), "type": "line", "start": format!("q{j}"), "end": format!("q{}", (j + 1) % prof.len())}))
            .collect();
        out.push(json!({"type": "sketch", "name": sk, "plane": {"origin": p, "x_axis": x, "y_axis": n}, "points": pts, "curves": cur}));
        out.push(json!({"type": "revolve", "name": format!("{name}_{k}"), "sketch": sk, "profiles": "all", "operation": "cut",
                        "target": target, "axis": {"line": format!("e{}", prof.len() - 1)}, "angle": 360.0, "from_hole": name}));
    }
    Some(out)
}

/// A rectangular pattern, one direction. The record's references, in order: the patterned
/// features (part or pattern features; a second direction is a pattern of the originals and
/// the first pattern), `a244457b` the direction (three triples at the end: start, vector, end
/// point, cm), `bb1dd5df` the count (its last u32), the spacing parameter, and `90874d28` flags
/// (the last one: midplane). The pattern runs against the edge vector (confirmed on two
/// patterns by volume and hole centres).
fn pattern(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    const G_DIR: u32 = 0xa244_457b;
    const G_COUNT: u32 = 0xbb1d_d5df;
    const G_FLAGS: u32 = 0x9087_4d28;
    let refs = r.dc.refs(f.rec);
    let tgt: Vec<String> =
        refs.iter().filter(|&&(_, x)| x != f.rec).filter_map(|&(_, x)| st.names.get(&x).cloned()).collect();
    let count_of = |x: usize| {
        let b = r.dc.r(x);
        u32_at(b, b.len().checked_sub(4)?)
    };
    let dir = refs.iter().map(|x| x.1).find(|&x| r.dc.class(x) == G_DIR);
    let cnt =
        refs.iter().map(|x| x.1).find(|&x| r.dc.class(x) == G_COUNT && count_of(x).is_some_and(|c| 1 < c && c < 1000));
    let par = refs.iter().map(|x| x.1).find(|&x| r.dc.role(x) == Role::Param);
    let (true, Some(dir), Some(cnt), Some(par)) = (!tgt.is_empty(), dir, cnt, par) else {
        return Err("direction, count or spacing not found".into());
    };
    let rec = r.dc.r(dir);
    let n = rec.len();
    // Start point, vector, end point (3 doubles each), then `u8 1` and two references.
    let o = n.checked_sub(57).ok_or("the direction is cut short")?;
    let vec = [f64_at(rec, o), f64_at(rec, o + 8), f64_at(rec, o + 16)];
    let [Some(x), Some(y), Some(z)] = vec else { return Err("the direction is cut short".into()) };
    let vec: V3 = [x, y, z];
    let ln = libm::sqrt(dot(vec, vec));
    if ln == 0.0 || !ln.is_finite() {
        return Err("the direction has no length".into());
    }
    let count = count_of(cnt).ok_or("no count")?;
    let spacing = rnd(r.dc.param(par).ok_or("the spacing is not readable")?.1 * CM);
    let midplane = refs
        .iter()
        .rfind(|&&(_, x)| r.dc.class(x) == G_FLAGS)
        .is_some_and(|&(_, x)| r.dc.r(x).last().is_some_and(|&b| b != 0));
    let u = [rnd(-vec[0] / ln), rnd(-vec[1] / ln), rnd(-vec[2] / ln)];
    if midplane {
        // Gusset has no midplane pattern: move the original holes back by half the run.
        let sh: V3 = u.map(|c| -c * spacing * f64::from(count - 1) / 2.0);
        let holes_only = tgt.iter().all(|t| st.holes.contains_key(t));
        if !holes_only {
            return Err("a midplane pattern of a feature that is not a hole".into());
        }
        for t in &tgt {
            let Some(info) = st.holes.get(t) else { continue };
            let Some(o) = st.out.iter_mut().find(|o| o.get("name").and_then(Value::as_str) == Some(t.as_str())) else {
                continue;
            };
            let moved: Vec<V3> = o
                .get("positions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?]))
                .map(|p| [rnd(p[0] + sh[0]), rnd(p[1] + sh[1]), rnd(p[2] + sh[2])])
                .collect();
            let fp = o
                .pointer("/face/point")
                .and_then(Value::as_array)
                .and_then(|p| Some([p.first()?.as_f64()?, p.get(1)?.as_f64()?, p.get(2)?.as_f64()?]));
            put(o, "positions", Value::Array(moved.iter().map(|p| arr(*p)).collect()));
            if let (Some(fp), Some(face)) = (fp, o.get_mut("face")) {
                put(face, "point", arr([rnd(fp[0] + sh[0]), rnd(fp[1] + sh[1]), rnd(fp[2] + sh[2])]));
            }
            if o.get("_fallback").is_some() {
                let fb = st.first_body.as_deref().and_then(|tb| hole_fallback(t, &moved, info, tb));
                match fb {
                    Some(fb) => put(o, "_fallback", Value::Array(fb)),
                    None => {
                        if let Some(m) = o.as_object_mut() {
                            m.remove("_fallback");
                        }
                    }
                }
            }
        }
    }
    st.out.push(json!({"type": "pattern_linear", "name": name, "features": tgt, "direction": u, "spacing": spacing, "count": count}));
    Ok(())
}

/// A thicken of a side face of the base block: the same as a join extrude of that side's
/// outline (the base block's extent across the side), from the cap plane back by the thickness
/// (section F of the notes). The thickness is the parameter under the face reference (slot 0);
/// the cap is the thicken-tagged planar face lying exactly that far outside the first extrusion's
/// bounding box. The rebuild check confirms the reading.
fn thicken(r: &Reader, st: &mut State, f: &Feature, name: &str) -> Result<(), String> {
    let s = r.slots(f)?;
    let mut dist = None;
    for (_, x1) in r.dc.refs(s.need(0)?) {
        for (_, x2) in r.dc.refs(x1) {
            if r.dc.role(x2) == Role::Param {
                dist = r.dc.param(x2).map(|p| p.1 * CM);
            }
        }
    }
    let dist = dist.filter(|d| d.is_finite() && *d > 0.0).ok_or("the thickness is not readable")?;
    let b = r.result().ok_or("the result body is not readable")?;
    let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
    let num = |v: Option<&Value>| v.and_then(Value::as_f64);
    let v3 = |v: Option<&Value>| -> Option<V3> {
        let a = v?.as_array()?;
        Some([num(a.first())?, num(a.get(1))?, num(a.get(2))?])
    };
    let first = st
        .out
        .iter()
        .find(|o| {
            o.get("type").and_then(Value::as_str) == Some("extrude")
                && o.get("operation").and_then(Value::as_str) == Some("new")
        })
        .ok_or("no base extrusion")?;
    let sk = st
        .out
        .iter()
        .find(|o| o.get("type").and_then(Value::as_str) == Some("sketch") && o.get("name") == first.get("sketch"))
        .ok_or("the base sketch is not found")?;
    let (o, x, y) = (
        v3(sk.pointer("/plane/origin")).ok_or("sketch plane")?,
        v3(sk.pointer("/plane/x_axis")).ok_or("sketch plane")?,
        v3(sk.pointer("/plane/y_axis")).ok_or("sketch plane")?,
    );
    let n = cross(x, y);
    let len = num(first.get("distance")).ok_or("the base extent")?;
    let symmetric = first.get("symmetric").and_then(Value::as_bool) == Some(true);
    let target = first.get("name").and_then(Value::as_str).ok_or("the base name")?.to_string();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in sk.get("points").and_then(Value::as_array).into_iter().flatten() {
        let (Some(u), Some(v)) = (num(p.get("x")), num(p.get("y"))) else { return Err("a base sketch point".into()) };
        for k in [if symmetric { -len } else { 0.0 }, len] {
            for i in 0..3 {
                let c = o.get(i).copied().unwrap_or(0.0)
                    + u * x.get(i).copied().unwrap_or(0.0)
                    + v * y.get(i).copied().unwrap_or(0.0)
                    + k * n.get(i).copied().unwrap_or(0.0);
                if let (Some(l), Some(h)) = (lo.get_mut(i), hi.get_mut(i)) {
                    *l = l.min(c);
                    *h = h.max(c);
                }
            }
        }
    }
    let mut caps: Vec<(usize, f64, f64)> = Vec::new();
    for (p, nn) in b.feature_planes(fid) {
        for ax in 0..3 {
            let na = nn.get(ax).copied().unwrap_or(0.0);
            if (na.abs() - 1.0).abs() < 1e-6 {
                let sg = if na > 0.0 { 1.0 } else { -1.0 };
                let c = p.get(ax).copied().unwrap_or(0.0) * CM;
                let ext = if sg > 0.0 { hi.get(ax) } else { lo.get(ax) }.copied().unwrap_or(f64::NAN);
                if (sg * (c - ext) - dist).abs() < 1e-4 && !caps.iter().any(|q| q.0 == ax && (q.2 - c).abs() < 1e-6) {
                    caps.push((ax, sg, c));
                }
            }
        }
    }
    let [(ax, sg, c)] = caps.as_slice() else {
        return Err(format!("the cap face is not found ({} candidates)", caps.len()));
    };
    let (ax, sg, c) = (*ax, *sg, *c);
    let (j, k) = match ax {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let unit_of = |i: usize, s: f64| -> V3 {
        let mut v = [0.0; 3];
        if let Some(e) = v.get_mut(i) {
            *e = s;
        }
        v
    };
    let ej = unit_of(j, 1.0);
    let mut ek = unit_of(k, 1.0);
    if dot(cross(ej, ek), unit_of(ax, sg)) < 0.0 {
        ek = unit_of(k, -1.0);
    }
    let mut org = [0.0; 3];
    for (i, e) in org.iter_mut().enumerate() {
        *e = (lo.get(i).copied().unwrap_or(0.0) + hi.get(i).copied().unwrap_or(0.0)) / 2.0;
    }
    if let Some(e) = org.get_mut(ax) {
        *e = c;
    }
    let half = |i: usize| (hi.get(i).copied().unwrap_or(0.0) - lo.get(i).copied().unwrap_or(0.0)) / 2.0;
    let (hj, hk) = (half(j), half(k));
    let ps = [(-hj, -hk), (hj, -hk), (hj, hk), (-hj, hk)];
    let sname = format!("{name}_sketch");
    let points: Vec<Value> =
        ps.iter().enumerate().map(|(i, &(u, v))| json!({"id": format!("t{i}"), "x": rnd(u), "y": rnd(v)})).collect();
    let curves: Vec<Value> = (0..4)
        .map(|i| json!({"id": format!("tl{i}"), "type": "line", "start": format!("t{i}"), "end": format!("t{}", (i + 1) % 4)}))
        .collect();
    st.out.push(json!({"type": "sketch", "name": sname, "plane": {"origin": arr(org), "x_axis": ej, "y_axis": ek},
                       "points": points, "curves": curves}));
    st.out.push(json!({"type": "extrude", "name": name, "sketch": sname, "profiles": "all", "operation": "join",
                       "target": target, "distance": rnd(-dist), "symmetric": false}));
    Ok(())
}

fn unsupported(name: &str, what: &str, reason: Option<String>) -> Value {
    let mut v = json!({"type": "unsupported", "name": name, "original_type": what});
    if let Some(r) = reason {
        put(&mut v, "reason", json!(r));
    }
    v
}

/// The rounds of slot ends (concave full rounds: the kernel's full round does the convex ones).
/// The round fills the two corners between the slot's end wall and the half cylinder with
/// material, so each is a join of the two slivers, drawn in a sketch across the cylinder's axis
/// (x towards the crest, y across the slot) and extruded over the cylinder's extent.
fn slot_end_rounds(st: &mut State, name: &str, ends: &[FullRound]) -> Result<(), String> {
    let target = st.first_body.clone().ok_or("a slot end round before any body")?;
    let mut ends = ends.to_vec();
    ends.sort_by(|a, b| a.point.partial_cmp(&b.point).unwrap_or(std::cmp::Ordering::Equal));
    ends.dedup_by(|a, b| crate::acis::dist(a.point, b.point) < 1e-6);
    for (k, e) in ends.iter().enumerate() {
        let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
        let (lo, hi) = e.span;
        let top = crate::acis::add(e.axis, crate::acis::mul(e.dir, hi));
        let org = [top[0] * CM, top[1] * CM, top[2] * CM];
        let ex = unit3(e.out);
        let ey = cross(unit3(e.dir), ex);
        let r = e.radius * CM;
        let pt = |id: &str, x: f64, y: f64| json!({"id": id, "x": rnd(x), "y": rnd(y)});
        let points = vec![
            pt("o", 0.0, 0.0),
            pt("a1", 0.0, r),
            pt("b1", r, r),
            pt("c1", r, 0.0),
            pt("a2", 0.0, -r),
            pt("b2", r, -r),
            pt("c2", r, 0.0),
        ];
        let line = |id: &str, a: &str, b: &str| json!({"id": id, "type": "line", "start": a, "end": b});
        let curves = vec![
            line("l1", "a1", "b1"),
            line("l2", "b1", "c1"),
            json!({"id": "r1", "type": "arc", "center": "o", "start": "c1", "end": "a1"}),
            line("l3", "a2", "b2"),
            line("l4", "b2", "c2"),
            json!({"id": "r2", "type": "arc", "center": "o", "start": "a2", "end": "c2"}),
        ];
        let sname = format!("{nm}_sketch");
        st.out.push(json!({"type": "sketch", "name": sname, "plane": {"origin": org, "x_axis": ex, "y_axis": ey},
                           "points": points, "curves": curves}));
        let picks = [[rnd(0.8 * r), rnd(0.8 * r)], [rnd(0.8 * r), rnd(-0.8 * r)]];
        st.out.push(json!({"type": "extrude", "name": nm, "sketch": sname,
                           "profiles": [{"point": picks[0]}, {"point": picks[1]}], "operation": "join",
                           "target": target, "distance": rnd(-(hi - lo) * CM), "symmetric": false}));
    }
    Ok(())
}

/// A cylinder blend between a plane and a convex cylinder that do not meet (a consumed middle
/// face): the material it adds is the sliver between its arc and the surrounding body. It is
/// joined as a sketch of the arc and a circumscribed polyline (1.2 radii out) whose far side
/// lies in the existing body; only the whole-part rebuild check vouches for that.
fn bridged_rounds(st: &mut State, name: &str, rs: &[BridgedRound]) -> Result<(), String> {
    let target = st.first_body.clone().ok_or("a bridged round before any body")?;
    for (k, b) in rs.iter().enumerate() {
        let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
        let (lo, hi) = b.span;
        let top = crate::acis::add(b.axis, crate::acis::mul(b.dir, hi));
        let org = [top[0] * CM, top[1] * CM, top[2] * CM];
        let ex = unit3(b.to_plane);
        let ey = cross(unit3(b.dir), ex);
        let phi = libm::atan2(crate::acis::dot(b.to_cyl, ey), crate::acis::dot(b.to_cyl, ex));
        let (rad, out) = (b.radius * CM, 1.2 * b.radius * CM);
        let at = |a: f64, q: f64| (q * libm::cos(a), q * libm::sin(a));
        let ang = [0.0, phi / 3.0, 2.0 * phi / 3.0, phi];
        let mut points = vec![
            json!({"id": "o", "x": 0.0, "y": 0.0}),
            json!({"id": "p0", "x": rnd(rad), "y": 0.0}),
            json!({"id": "k", "x": rnd(at(phi, rad).0), "y": rnd(at(phi, rad).1)}),
        ];
        for (i, a) in ang.iter().enumerate() {
            let (x, y) = at(*a, out);
            points.push(json!({"id": format!("w{i}"), "x": rnd(x), "y": rnd(y)}));
        }
        let line = |id: &str, a: &str, b: &str| json!({"id": id, "type": "line", "start": a, "end": b});
        let (s0, s1) = if phi > 0.0 { ("p0", "k") } else { ("k", "p0") };
        let curves = vec![
            json!({"id": "arc", "type": "arc", "center": "o", "start": s0, "end": s1}),
            line("l0", "k", "w3"),
            line("l1", "w3", "w2"),
            line("l2", "w2", "w1"),
            line("l3", "w1", "w0"),
            line("l4", "w0", "p0"),
        ];
        let sname = format!("{nm}_sketch");
        st.out.push(json!({"type": "sketch", "name": sname, "plane": {"origin": org, "x_axis": ex, "y_axis": ey},
                           "points": points, "curves": curves}));
        let mid = at(phi / 2.0, (rad + out) / 2.0);
        st.out.push(json!({"type": "extrude", "name": nm, "sketch": sname,
                           "profiles": [{"point": [rnd(mid.0), rnd(mid.1)]}], "operation": "join",
                           "target": target, "distance": rnd(-(hi - lo) * CM), "symmetric": false}));
    }
    Ok(())
}

fn blend(r: &Reader, st: &mut State, f: &Feature, name: &str, typ: FeatType) -> Result<(), String> {
    let s = r.slots(f)?;
    let label = typ.label();
    let Some(b) = r.result() else {
        st.out.push(unsupported(name, &label, Some("the result body is not readable".into())));
        return Ok(());
    };
    let fid = i64::from(r.dc.seq(f.rec).ok_or("the feature has no id")?);
    let faces = b.blend_faces(fid);
    // A pick: the midpoint of the pre-fillet edge (mm) and, unless the edge is a whole circle,
    // two points 0.1 mm either side of it along the edge.
    // With the blend face's radius (mm; cylinders and tori), to tell the edge sets of a fillet
    // with several radii apart.
    let mut mids: Vec<(V3, Option<[V3; 2]>, Option<f64>)> = Vec::new();
    let mut why: Vec<String> = Vec::new();
    let mut radii: Vec<f64> = Vec::new();
    let mut rounds: Vec<(V3, V3)> = Vec::new();
    let mut slot_ends: Vec<FullRound> = Vec::new();
    let mut bridged: Vec<BridgedRound> = Vec::new();
    let chamfer_d = (typ == FeatType::Chamfer).then(|| r.pval(&s, 2).ok()).flatten();
    let mm = |v: V3| [rnd_to(v[0] * CM, 1e6), rnd_to(v[1] * CM, 1e6), rnd_to(v[2] * CM, 1e6)];
    for &face in &faces {
        let k = b.kind(face);
        // A corner patch; and a planar face with a fillet's tag is a leftover of an edge split
        // later (a fillet face is never planar).
        // A vertex blend (`srf_srf_v_bl_spl_sur`) is the patch where blends meet at a corner.
        let patch = matches!(&k, Surf::Other(t) if t.contains("_v_bl_"));
        if matches!(k, Surf::Sphere) || patch || (typ == FeatType::Fillet && matches!(k, Surf::Plane { .. })) {
            continue;
        }
        if typ == FeatType::Chamfer && b.is_chamfer_corner_patch(face, &faces) {
            continue;
        }
        if typ == FeatType::Fillet
            && let Some(fr) = b.full_round(face)
        {
            if fr.concave {
                slot_ends.push(fr);
            } else {
                rounds.push((mm(fr.point), fr.normal));
            }
            continue;
        }
        if typ == FeatType::Fillet
            && let Some(br) = b.bridged_round(face)
        {
            if bridged.iter().all(|x| crate::acis::dist(x.axis, br.axis) > 1e-6) {
                bridged.push(br);
            }
            continue;
        }
        match b.edge_mid(face) {
            Err(w) => why.push(w),
            Ok((m, info)) => {
                // A chamfer face lies within a few distances of the edge it replaced. A planar
                // face with the tag whose "edge" is further away is a leftover (an original
                // wall that kept the tag when a chamfer chain went round it), not a chamfer.
                if typ == FeatType::Chamfer
                    && matches!(k, Surf::Plane { .. })
                    && let (EdgeInfo::Line { dir }, Some(d)) = (&info, chamfer_d)
                    && b.verts(face).iter().any(|&v| {
                        let q = crate::acis::sub(v, m);
                        let off = crate::acis::sub(q, crate::acis::mul(*dir, crate::acis::dot(q, *dir)));
                        libm::sqrt(crate::acis::dot(off, off)) > 3.0 * d
                    })
                {
                    continue;
                }
                let rad = match k {
                    Surf::Cyl { r, .. } => Some(rnd_to(r * CM, 1e6)),
                    Surf::Torus { minor, .. } => Some(rnd_to(minor * CM, 1e6)),
                    _ => None,
                };
                radii.extend(rad);
                mids.push((mm(m), local_path(m, &info, PATH_EPS).map(|[a, b]| [mm(a), mm(b)]), rad));
            }
        }
    }
    if !bridged.is_empty() {
        if !mids.is_empty() || !why.is_empty() || !rounds.is_empty() || !slot_ends.is_empty() {
            why.push("a bridged round next to other fillet edges".into());
        } else {
            return bridged_rounds(st, name, &bridged);
        }
    }
    if !slot_ends.is_empty() {
        if !mids.is_empty() || !why.is_empty() || !rounds.is_empty() {
            why.push("a full round next to other fillet edges".into());
        } else {
            return slot_end_rounds(st, name, &slot_ends);
        }
    }
    if !rounds.is_empty() {
        if !mids.is_empty() || !why.is_empty() {
            why.push("a full round next to other fillet edges".into());
        } else {
            // One feature, several middle faces: one full-round fillet each (the first keeps
            // the feature's name).
            rounds.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            rounds.dedup_by(|a, b| crate::acis::dist(a.0, b.0) < 1e-6);
            for (k, (p, n)) in rounds.iter().enumerate() {
                let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
                let normal = [rnd(n[0]), rnd(n[1]), rnd(n[2])];
                st.out.push(json!({"type": "fillet", "name": nm, "full_round": {"point": p, "normal": normal}}));
            }
            return Ok(());
        }
    }
    if faces.is_empty() {
        why.push("no blend faces tagged with this feature in the result body".into());
    }
    if !why.is_empty() {
        why.sort();
        why.dedup();
        st.out.push(unsupported(name, &label, Some(why.join("; "))));
        return Ok(());
    }
    let mut uniq: Vec<(V3, Option<[V3; 2]>, Option<f64>)> = Vec::new();
    for (m, p, q) in mids {
        if uniq.iter().all(|u| crate::acis::dist(m, u.0) > 1e-6) {
            uniq.push((m, p, q));
        }
    }
    if uniq.is_empty() {
        st.out.push(unsupported(name, &label, Some("no edge of the blend faces".into())));
        return Ok(());
    }
    let pick = |(m, p, _): &(V3, Option<[V3; 2]>, Option<f64>)| match p {
        Some([a, b]) => json!({"midpoint": m, "path": [a, b]}),
        None => json!({"midpoint": m}),
    };
    let edges: Vec<Value> = uniq.iter().map(pick).collect();
    if typ == FeatType::Fillet {
        let mut rad = None;
        let (sets, _) = r.dc.first_list(s.need(0)?, None).ok_or("the fillet has no edge sets")?;
        'sets: for x in sets.iter().flatten() {
            for (_, y) in r.dc.refs(*x) {
                if r.dc.role(y) == Role::Param {
                    rad = r.dc.param(y).map(|p| p.1 * CM);
                    break;
                }
            }
            if rad.is_some_and(|v| v != 0.0) {
                break 'sets;
            }
        }
        radii.sort_by(f64::total_cmp);
        radii.dedup();
        if rad.is_none() && radii.len() == 1 {
            rad = radii.first().copied();
        }
        let Some(rad) = rad else {
            st.out.push(unsupported(name, "fillet", Some("the radius is not readable".into())));
            return Ok(());
        };
        if sets.len() > 1 || radii.iter().any(|q| (q - rad).abs() > 1e-6) {
            // Several edge sets: one fillet per radius, each edge going to the set whose radius
            // its blend face has (a face without a radius, or a radius no set has, refuses).
            let mut set_radii: Vec<f64> = Vec::new();
            for x in sets.iter().flatten() {
                let q = r.dc.refs(*x).into_iter().find(|&(_, y)| r.dc.role(y) == Role::Param);
                if let Some(v) = q.and_then(|(_, y)| r.dc.param(y)).map(|p| rnd_to(p.1 * CM, 1e6))
                    && v > 0.0
                    && set_radii.iter().all(|w| (w - v).abs() > 1e-6)
                {
                    set_radii.push(v);
                }
            }
            let face_radii: Vec<Option<f64>> = uniq.iter().map(|u| u.2).collect();
            let Some(groups) = radius_groups(&set_radii, &face_radii) else {
                st.out.push(unsupported(name, "fillet with several radii", None));
                return Ok(());
            };
            for (k, (rad, idx)) in groups.into_iter().enumerate() {
                let nm = if k == 0 { name.to_string() } else { format!("{name}_{}", k + 1) };
                let edges: Vec<Value> = idx.iter().filter_map(|&i| uniq.get(i)).map(pick).collect();
                st.out.push(json!({"type": "fillet", "name": nm, "edges": edges, "radius": rnd(rad)}));
            }
            return Ok(());
        }
        st.out.push(json!({"type": "fillet", "name": name, "edges": edges, "radius": rnd(rad)}));
    } else {
        let dist = r.pval(&s, 2)?;
        let mut ch = json!({"type": "chamfer", "name": name, "edges": edges, "distance": rnd(dist * CM)});
        match chamfer_kind(r, &s, dist, edges.len()) {
            Ok(None) => {}
            Ok(Some((angle, face))) => {
                put(&mut ch, "angle", json!(rnd(angle.to_degrees())));
                put(&mut ch, "face", json!({"point": face}));
            }
            Err(why) => {
                st.out.push(unsupported(name, "chamfer", Some(why)));
                return Ok(());
            }
        }
        st.out.push(ch);
    }
    Ok(())
}

/// The edges of a fillet with several edge sets, by radius: for each set radius (mm, in set
/// order) that some edge has, the indices of the edges whose blend face has it. `None` when an
/// edge has no radius (a spline blend) or one no set has.
pub(crate) fn radius_groups(set_radii: &[f64], face_radii: &[Option<f64>]) -> Option<Vec<(f64, Vec<usize>)>> {
    let mut groups: Vec<(f64, Vec<usize>)> = set_radii.iter().map(|&r| (r, Vec::new())).collect();
    for (i, q) in face_radii.iter().enumerate() {
        let q = (*q)?;
        groups.iter_mut().find(|g| (g.0 - q).abs() <= 1e-6)?.1.push(i);
    }
    groups.retain(|g| !g.1.is_empty());
    Some(groups)
}

/// The chamfer type (slot 4, `7daa0032`: `u16 2, u16 type, u32 0` at the end; 0 equal
/// distances, 1 distance and angle, confirmed on the M8 screw) and, for an unequal one, the
/// angle (slot 10, radians) and the point (mm) that names the face the distance runs along.
/// Slot 8 holds two records of the two offset lines of the first edge: start on the edge, end
/// `distance` away inside a support face; the first one is the face of the first distance.
/// Anything else (another type, one record not a line of that length, several edges: the pick
/// would apply to every edge) is refused.
fn chamfer_kind(r: &Reader, s: &Slots, dist: f64, edges: usize) -> Result<Option<(f64, V3)>, String> {
    let rec = r.dc.r(s.need(4)?);
    let n = rec.len();
    let (Some(two), Some(ty), Some(zero)) = (
        n.checked_sub(8).and_then(|k| u16_at(rec, k)),
        n.checked_sub(6).and_then(|k| u16_at(rec, k)),
        n.checked_sub(4).and_then(|k| u32_at(rec, k)),
    ) else {
        return Err("the chamfer type is not readable".into());
    };
    if two != 2 || zero != 0 {
        return Err("the chamfer type record has an unknown layout".into());
    }
    match ty {
        0 => return Ok(None),
        1 => {}
        t => return Err(format!("chamfer type {t} is not decoded")),
    }
    if edges != 1 {
        return Err("an unequal chamfer on several edges: the reference face of each edge is not read".into());
    }
    let angle = r.pval(s, 10)?;
    if !(angle > 0.0 && angle < std::f64::consts::FRAC_PI_2) {
        return Err("the chamfer angle is out of range".into());
    }
    let (items, _) = r.dc.first_list(s.need(8)?, None).ok_or("the chamfer has no reference faces")?;
    let first = items.first().copied().flatten().ok_or("the chamfer has no reference face")?;
    let rf = r.dc.r(first);
    let f = |k: usize| f64_at(rf, 22 + 8 * k).filter(|v| v.is_finite());
    let (Some(p0), Some(p1)) =
        (f(0).zip(f(1)).zip(f(2)).map(|((a, b), c)| [a, b, c]), f(3).zip(f(4)).zip(f(5)).map(|((a, b), c)| [a, b, c]))
    else {
        return Err("the reference face of the chamfer is not readable".into());
    };
    if u32_at(rf, 10) != Some(2) || (crate::acis::dist(p0, p1) - dist).abs() > 1e-6 {
        return Err("the first distance does not run along the first reference face".into());
    }
    Ok(Some((angle, [rnd_to(p1[0] * CM, 1e6), rnd_to(p1[1] * CM, 1e6), rnd_to(p1[2] * CM, 1e6)])))
}

/// The part's feature history as a tree (see the module notes).
///
/// # Errors
/// A reason: the file is not an Inventor part, has no feature segment, or its records do not
/// have the layout read here.
pub fn feature_tree(bytes: &[u8], file_name: &str, limits: &Limits) -> Result<Value, String> {
    let dc = Dc::open(bytes, limits)?;
    let feats = dc.ordered_features();
    if feats.is_empty() {
        return Err("the part has no features".into());
    }
    if feats.len() > MAX_FEATURES {
        return Err("the part has too many features".into());
    }
    let r = Reader {
        dc,
        bytes,
        limits,
        back: OnceCell::new(),
        owned: OnceCell::new(),
        history: OnceCell::new(),
        result: OnceCell::new(),
    };
    let names: HashMap<usize, String> =
        feats.iter().map(|f| (f.rec, f.name.clone().unwrap_or_else(|| format!("Feature{}", f.rec)))).collect();
    let mut st = State {
        out: Vec::new(),
        done: HashMap::new(),
        first_body: None,
        omitted: Vec::new(),
        names,
        holes: HashMap::new(),
    };
    for f in &feats {
        let name = f.name.clone().unwrap_or_else(|| format!("Feature{}", f.rec));
        let Some(typ) = f.typ else {
            st.out.push(unsupported(&name, "unknown", None));
            continue;
        };
        // Suppressed in the source (flag bit 2 of the header): not part of the result body.
        if r.dc.r(f.rec).get(10).is_some_and(|b| b & 0x04 != 0) {
            st.omitted.push(json!({"name": name, "why": format!("{} suppressed in the source", typ.label())}));
            continue;
        }
        let before = st.out.len();
        let res = match typ {
            FeatType::Extrude | FeatType::Revolve => solid(&r, &mut st, f, &name, typ),
            FeatType::Fillet | FeatType::Chamfer => blend(&r, &mut st, f, &name, typ),
            FeatType::Hole => hole(&r, &mut st, f, &name),
            FeatType::RectPattern => pattern(&r, &mut st, f, &name),
            FeatType::Thicken => thicken(&r, &mut st, f, &name),
            // A cosmetic thread changes no geometry.
            FeatType::Thread => {
                st.omitted.push(json!({"name": name, "why": "cosmetic thread"}));
                Ok(())
            }
            other => {
                st.out.push(unsupported(&name, &other.label(), None));
                Ok(())
            }
        };
        if let Err(why) = res {
            st.out.truncate(before);
            st.out.push(unsupported(&name, &typ.label(), Some(why)));
        }
    }
    let file = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    Ok(json!({"format": "gusset-feature-tree", "version": 2,
              "source": {"app": "inventor", "file": file, "omitted": st.omitted},
              "features": st.out}))
}
