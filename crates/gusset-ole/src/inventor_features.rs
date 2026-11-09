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

use crate::acis::{Acis, Surf, V3, cross, dot, local_path};
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
    Line { id: usize, a: usize, b: usize, con: bool },
    Arc { id: usize, c: usize, s: usize, e: usize, con: bool },
    Circle { id: usize, c: usize, r: f64, con: bool },
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
                if matches!(self.dc.role(i), Role::SkPoint | Role::SkLine | Role::SkCircle)
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
            if !matches!(role, Role::SkLine | Role::SkCircle) {
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

fn unsupported(name: &str, what: &str, reason: Option<String>) -> Value {
    let mut v = json!({"type": "unsupported", "name": name, "original_type": what});
    if let Some(r) = reason {
        put(&mut v, "reason", json!(r));
    }
    v
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
    let mut mids: Vec<(V3, Option<[V3; 2]>)> = Vec::new();
    let mut why: Vec<String> = Vec::new();
    let mut radii: Vec<f64> = Vec::new();
    let mm = |v: V3| [rnd_to(v[0] * CM, 1e6), rnd_to(v[1] * CM, 1e6), rnd_to(v[2] * CM, 1e6)];
    for &face in &faces {
        let k = b.kind(face);
        // A corner patch; and a planar face with a fillet's tag is a leftover of an edge split
        // later (a fillet face is never planar).
        if matches!(k, Surf::Sphere) || (typ == FeatType::Fillet && matches!(k, Surf::Plane { .. })) {
            continue;
        }
        match b.edge_mid(face) {
            Err(w) => why.push(w),
            Ok((m, info)) => {
                mids.push((mm(m), local_path(m, &info, PATH_EPS).map(|[a, b]| [mm(a), mm(b)])));
                match k {
                    Surf::Cyl { r, .. } => radii.push(rnd_to(r * CM, 1e6)),
                    Surf::Torus { minor, .. } => radii.push(rnd_to(minor * CM, 1e6)),
                    _ => {}
                }
            }
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
    let mut uniq: Vec<(V3, Option<[V3; 2]>)> = Vec::new();
    for (m, p) in mids {
        if uniq.iter().all(|q| crate::acis::dist(m, q.0) > 1e-6) {
            uniq.push((m, p));
        }
    }
    if uniq.is_empty() {
        st.out.push(unsupported(name, &label, Some("no edge of the blend faces".into())));
        return Ok(());
    }
    let edges: Vec<Value> = uniq
        .iter()
        .map(|(m, p)| match p {
            Some([a, b]) => json!({"midpoint": m, "path": [a, b]}),
            None => json!({"midpoint": m}),
        })
        .collect();
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
            st.out.push(unsupported(name, "fillet with several radii", None));
            return Ok(());
        }
        st.out.push(json!({"type": "fillet", "name": name, "edges": edges, "radius": rnd(rad)}));
    } else {
        let dist = r.pval(&s, 2)?;
        st.out.push(json!({"type": "chamfer", "name": name, "edges": edges, "distance": rnd(dist * CM)}));
    }
    Ok(())
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
