//! Sketches (points, lines, arcs, placement) and the profile records that say which sketch
//! regions an extrude uses.

use super::geom::{self, P2, Plane, V3};
use super::stream::{
    self, Design, dbl, dbl3, parse_header, ref_end, ref_ids, ref_lists, refs_at, sections, wstrings_all,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

pub(super) const SKETCH_FEAT: &str = "8DA771B7";
pub(super) const SKETCH: &str = "44A64366";
const SKETCH_BASE: &str = "37AD519C";
const SK_XFORM: &str = "F47A46FB";
const POINT: &str = "C2CEDAE7";
const LINE: &str = "DCA267ED";
const ARC: &str = "F0130424";

#[derive(Debug, Clone)]
pub(super) struct Line {
    pub origin: V3,
    pub vector: V3,
    pub end_ref: Option<u64>,
    pub start_ref: Option<u64>,
}

#[derive(Debug, Clone)]
pub(super) struct Arc {
    pub center: V3,
    pub normal: V3,
    pub xdir: V3,
    pub radius: f64,
    pub a0: f64,
    pub a1: f64,
    pub refs: Vec<u64>,
}

#[derive(Debug, Default)]
pub(super) struct Sketch {
    pub id: u64,
    pub plane: Option<Plane>,
    /// The plane was not stored: the component's XY plane is assumed.
    pub identity_assumed: bool,
    pub points: BTreeMap<u64, V3>,
    pub lines: BTreeMap<u64, Line>,
    pub arcs: BTreeMap<u64, Arc>,
    pub other: BTreeMap<u64, String>,
}

/// `(class prefix, body offset, end)` of each section of record `i`.
fn section_bodies<'a>(d: &'a Design, i: u64) -> Vec<(Option<&'a str>, Option<usize>, usize)> {
    let rec = d.rec(i);
    let Some(secs) = sections(rec) else { return Vec::new() };
    let mut out = Vec::new();
    for (k, (o, tag)) in secs.iter().enumerate() {
        let end = secs.get(k + 1).map_or(rec.len(), |s| s.0);
        let body = if k == 0 { parse_header(rec).map(|h| h.body) } else { Some(o + 7 + 8 + 4) };
        out.push((d.tag_class(*tag), body, end));
    }
    out
}

fn find_section(d: &Design, i: u64, prefix: &str) -> Option<(usize, usize)> {
    section_bodies(d, i)
        .into_iter()
        .find_map(|(c, body, end)| if c.is_some_and(|c| c.starts_with(prefix)) { body.map(|b| (b, end)) } else { None })
}

fn has_section(d: &Design, i: u64, prefix: &str) -> bool {
    section_bodies(d, i).iter().any(|(c, _, _)| c.is_some_and(|c| c.starts_with(prefix)))
}

#[allow(clippy::float_cmp)]
fn zero(x: f64) -> bool {
    x == 0.0
}

fn point(d: &Design, i: u64) -> Option<V3> {
    let (b, _) = find_section(d, i, POINT)?;
    let rec = d.rec(i);
    // The point's curve list, a few bytes, then x y z (cm, sketch space): 9 bytes in most class
    // versions, 10 in one older typed-reference version; the offset whose values are sane wins.
    let (_, after) = ref_end(d, rec, b)?;
    for gap in [9usize, 10, 8, 11] {
        if let Some(v) = dbl3(rec, after + gap)
            && zero(v[2])
            && v.iter().all(|x| zero(*x) || (1e-200 < x.abs() && x.abs() < 1e5))
        {
            return Some(v);
        }
    }
    None
}

fn line(d: &Design, i: u64) -> Option<Line> {
    let (b, e) = find_section(d, i, LINE)?;
    let rec = d.rec(i);
    let v = dbl::<9>(rec, b)?;
    let slice = rec.get(..e)?;
    let pts: Vec<u64> =
        refs_at(d, slice, b + 72).into_iter().map(|p| p.1).filter(|q| has_section(d, *q, POINT)).take(2).collect();
    Some(Line {
        origin: [v[0], v[1], v[2]],
        vector: [v[3], v[4], v[5]],
        end_ref: pts.first().copied(),
        start_ref: pts.get(1).copied(),
    })
}

fn arc(d: &Design, i: u64) -> Option<Arc> {
    let (b, e) = find_section(d, i, ARC)?;
    let rec = d.rec(i);
    let v = dbl::<12>(rec, b)?;
    let slice = rec.get(..e)?;
    let refs: Vec<u64> =
        refs_at(d, slice, b + 96).into_iter().map(|p| p.1).filter(|q| has_section(d, *q, POINT)).take(3).collect();
    Some(Arc {
        center: [v[0], v[1], v[2]],
        normal: [v[3], v[4], v[5]],
        xdir: [v[6], v[7], v[8]],
        radius: v[9],
        a0: v[10],
        a1: v[11],
        refs,
    })
}

/// A sketch placement: 16 doubles, a row-major 4x4 whose columns are the sketch x axis, y axis,
/// normal and origin (cm); or the short form without a matrix (the component's XY plane).
fn xform(d: &Design, i: u64) -> Option<(Plane, bool)> {
    let rec = d.rec(i);
    let hi = rec.len().saturating_sub(127).min(64);
    for o in 15..hi {
        let Some(m) = dbl::<16>(rec, o) else { continue };
        if m[12].abs() > 1e-9 || m[13].abs() > 1e-9 || m[14].abs() > 1e-9 || (m[15] - 1.0).abs() > 1e-9 {
            continue;
        }
        let cols = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
        if cols.iter().all(|c| (geom::dot(*c, *c) - 1.0).abs() < 1e-6) {
            return Some((Plane { x: cols[0], y: cols[1], z: cols[2], origin: [m[3], m[7], m[11]] }, false));
        }
    }
    if rec.len() < 100 && rec.get(21..23) == Some(&[1, 0]) {
        return Some((Plane { x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], origin: [0.0; 3] }, true));
    }
    None
}

pub(super) fn read_sketch(d: &Design, sid: u64) -> Sketch {
    let mut out = Sketch { id: sid, ..Sketch::default() };
    let Some((b, e)) = find_section(d, sid, SKETCH_BASE) else { return out };
    let rec = d.rec(sid);
    if let Some((xf, _)) = ref_end(d, rec, b)
        && d.is(xf, SK_XFORM)
        && let Some((pl, assumed)) = xform(d, xf)
    {
        out.plane = Some(pl);
        out.identity_assumed = assumed;
    }
    let mut ents = std::collections::BTreeSet::new();
    if let Some(slice) = rec.get(..e) {
        for (_, ids) in ref_lists(d, slice, b) {
            ents.extend(ids);
        }
    }
    for q in ents {
        if has_section(d, q, POINT) {
            if let Some(p) = point(d, q) {
                out.points.insert(q, p);
            }
        } else if has_section(d, q, LINE) {
            if let Some(l) = line(d, q) {
                out.lines.insert(q, l);
            }
        } else if has_section(d, q, ARC) {
            if let Some(a) = arc(d, q) {
                out.arcs.insert(q, a);
            }
        } else {
            out.other.insert(q, d.cls(q).to_string());
        }
    }
    out
}

// ----- profiles ----------------------------------------------------------------------------

/// One curve of a profile loop: the curve's `(primary, secondary)` ids, and piece `a` of `b`.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub primary: u64,
    pub secondary: u64,
    pub a: u32,
    pub b: u32,
}

#[derive(Debug, Clone)]
pub(super) struct Loop {
    pub flag: u8,
    pub curves: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub(super) struct Region {
    pub loops: Vec<Loop>,
    pub children: Vec<Region>,
}

fn parse_region(rec: &[u8], mut o: usize, depth: usize) -> Option<(Region, usize)> {
    if depth > 32 {
        return None;
    }
    let nl = stream::u32_at(rec, o)?;
    o += 4;
    if nl >= 10_000 || (nl == 0 && depth > 0) {
        return None;
    }
    let mut loops = Vec::new();
    for _ in 0..nl {
        let n = stream::u32_at(rec, o)? as usize;
        o += 4;
        if n == 0 || n >= 100_000 {
            return None;
        }
        let mut curves = Vec::new();
        for _ in 0..n {
            let t = stream::u32_at(rec, o)?;
            let p = stream::u64_at(rec, o + 4)?;
            let s = stream::u64_at(rec, o + 12)?;
            let a = stream::u32_at(rec, o + 24)?;
            let b = stream::u32_at(rec, o + 28)?;
            stream::u32_at(rec, o + 20)?;
            rec.get(o + 32..o + 40)?;
            if t > 16 || p > 1 << 40 {
                return None;
            }
            curves.push(Entry { primary: p, secondary: s, a, b });
            o += 40;
        }
        let flag = *rec.get(o)?;
        o += 1;
        loops.push(Loop { flag, curves });
    }
    if depth > 0 {
        return Some((Region { loops, children: Vec::new() }, o));
    }
    let nc = stream::u32_at(rec, o)? as usize;
    o += 4;
    if nc > 10_000 {
        return None;
    }
    let mut children = Vec::new();
    for _ in 0..nc {
        let (k, o2) = parse_region(rec, o, depth + 1)?;
        children.push(k);
        o = o2;
    }
    Some((Region { loops, children }, o))
}

/// The regions of a `0D57BD2F` profile-geometry record, if it parses exactly up to the next
/// section (or the record's end).
pub(super) fn parse_profiles(d: &Design, i: u64) -> Option<Vec<Region>> {
    let rec = d.rec(i);
    let b = parse_header(rec)?.body;
    let hi = (b + 16).min(rec.len());
    for j in b.saturating_sub(4).max(15)..hi {
        let Some((_, after)) = ref_end(d, rec, j) else { continue };
        for skip in [1usize, 0, 2] {
            let mut o = after + skip;
            let Some(n) = stream::u32_at(rec, o) else { continue };
            o += 4;
            if n == 0 || n >= 100_000 {
                continue;
            }
            let mut regs = Vec::new();
            let mut ok = true;
            for _ in 0..n {
                match parse_region(rec, o, 0) {
                    Some((g, o2)) => {
                        regs.push(g);
                        o = o2;
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            let sec = rec
                .get(o..o + 7)
                .is_some_and(|s| s.starts_with(&[3, 0, 0, 0]) && s.iter().skip(4).all(u8::is_ascii_digit));
            if sec || o == rec.len() {
                return Some(regs);
            }
        }
    }
    None
}

// ----- regions -----------------------------------------------------------------------------

/// A sketch curve as used by one profile loop.
#[derive(Debug)]
pub(super) struct Seg {
    pub id: usize,
    pub curve: u64,
    pub poly: Vec<P2>,
    pub flag: u8,
    pub loopkey: usize,
}

/// A region a feature uses: an interior point and the loops' curve pieces.
#[derive(Debug)]
pub(super) struct RegionEnt {
    pub point: P2,
    pub segments: Vec<Rc<Seg>>,
    pub naming_segments: Option<Vec<Rc<Seg>>>,
}

fn crv_key(d: &Design, q: u64) -> Option<(u64, u64)> {
    let h = parse_header(d.rec(q))?;
    let v = |k: &str| h.attrs.get(k).map(|b| b.iter().rev().fold(0u64, |a, c| a.wrapping_shl(8) | u64::from(*c)));
    Some((v("crv_primary_id")?, v("crv_secondary_id")?))
}

/// The object id of the line or arc of sketch `s` whose `crv_primary_id` is `primary`.
pub(super) fn curve_by_primary(d: &Design, s: &Sketch, primary: u64) -> Option<u64> {
    s.lines.keys().chain(s.arcs.keys()).copied().find(|q| crv_key(d, *q).is_some_and(|k| k.0 == primary))
}

/// Sketch curve `q` as a polyline from its start to its end (cm).
fn curve_polyline(d: &Design, s: &Sketch, q: u64) -> Option<Vec<P2>> {
    if let Some(l) = s.lines.get(&q) {
        let a = l.origin;
        let b = geom::add(l.origin, l.vector);
        return Some(vec![[a[0], a[1]], [b[0], b[1]]]);
    }
    if let Some(a) = s.arcs.get(&q) {
        let (n, xd) = (a.normal, a.xdir);
        let yd = geom::cross(n, xd);
        let m = ((a.a1 - a.a0).abs() / (2.0 * std::f64::consts::PI) * 64.0) as usize;
        let m = m.clamp(8, 4096);
        let mut out = Vec::with_capacity(m + 1);
        for k in 0..=m {
            let t = a.a0 + (a.a1 - a.a0) * k as f64 / m as f64;
            let (c, s_) = (libm::cos(t), libm::sin(t));
            out.push([
                a.center[0] + a.radius * (c * xd[0] + s_ * yd[0]),
                a.center[1] + a.radius * (c * xd[1] + s_ * yd[1]),
            ]);
        }
        return Some(out);
    }
    // Other curve classes (splines): a chord through the point references, in record order.
    let mut uniq: Vec<P2> = Vec::new();
    for (_, r) in refs_at(d, d.rec(q), 0) {
        if let Some(p) = s.points.get(&r) {
            let p = [p[0], p[1]];
            if !uniq.contains(&p) {
                uniq.push(p);
            }
        }
    }
    (uniq.len() >= 2).then_some(uniq)
}

/// Computes the regions a feature's profile names. Shared state: a counter for segment and
/// loop ids, and the design-wide sets of curves that bound some profile.
#[derive(Default)]
pub(super) struct RegionCache {
    next_id: usize,
    used: Option<HashMap<u64, HashSet<(u64, u64)>>>,
    pub done: HashMap<u64, Result<Rc<Vec<RegionEnt>>, String>>,
}

fn sketch_of_profile(d: &Design, q: u64) -> Option<u64> {
    for w in wstrings_all(d.rec(q)) {
        if let Ok(n) = w.2.parse::<u64>()
            && d.has(n)
            && d.is(n, SKETCH)
        {
            return Some(n);
        }
    }
    None
}

impl RegionCache {
    fn used_curves(&mut self, d: &Design) -> &HashMap<u64, HashSet<(u64, u64)>> {
        self.used.get_or_insert_with(|| {
            let mut cache: HashMap<u64, HashSet<(u64, u64)>> = HashMap::new();
            for &i in &d.ids {
                if !d.is(i, "0D57BD2F") {
                    continue;
                }
                let own: Vec<u64> =
                    refs_at(d, d.rec(i), 0).into_iter().take(2).map(|p| p.1).filter(|q| d.is(*q, "4BD53E5A")).collect();
                let Some(sk) = own.first().and_then(|q| sketch_of_profile(d, *q)) else { continue };
                for g in parse_profiles(d, i).unwrap_or_default() {
                    for l in &g.loops {
                        for c in &l.curves {
                            cache.entry(sk).or_default().insert((c.primary, c.secondary));
                        }
                    }
                }
            }
            cache
        })
    }

    /// One interior point per profile region the feature `fid` uses, in sketch `s`.
    pub(super) fn regions(&mut self, d: &Design, fid: u64, s: &Sketch) -> Result<Rc<Vec<RegionEnt>>, String> {
        if let Some(r) = self.done.get(&fid) {
            return r.clone();
        }
        let r = self.compute(d, fid, s).map(Rc::new);
        self.done.insert(fid, r.clone());
        r
    }

    fn compute(&mut self, d: &Design, fid: u64, s: &Sketch) -> Result<Vec<RegionEnt>, String> {
        let profs: Vec<u64> = ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "4BD53E5A")).collect();
        if profs.is_empty() {
            return Err("no profile record".into());
        }
        // The curves of the sketch by their (primary, secondary) ids; a later curve with the
        // same key replaces an earlier one in place.
        let mut keymap: HashMap<(u64, u64), u64> = HashMap::new();
        let mut key_order: Vec<(u64, u64)> = Vec::new();
        for q in s.lines.keys().chain(s.arcs.keys()).chain(s.other.keys()) {
            if let Some(k) = crv_key(d, *q)
                && keymap.insert(k, *q).is_none()
            {
                key_order.push(k);
            }
        }
        let curve_ids: Vec<u64> = key_order.iter().filter_map(|k| keymap.get(k).copied()).collect();
        let mut allpolys: Option<Vec<(u64, Option<Vec<P2>>)>> = None;
        let mut used: Option<HashSet<u64>> = None;
        let mut pts = Vec::new();
        for &i in &d.ids {
            if !d.is(i, "0D57BD2F") {
                continue;
            }
            let own: Vec<u64> = refs_at(d, d.rec(i), 0).into_iter().take(2).map(|p| p.1).collect();
            if !profs.iter().any(|p| own.contains(p)) {
                continue;
            }
            let regs = parse_profiles(d, i).ok_or("0D57BD2F layout")?;
            for g in &regs {
                let mut naming = None;
                let parts: Vec<&Region> = if g.children.is_empty() {
                    vec![g]
                } else {
                    naming = Some(if g.loops.is_empty() {
                        Vec::new()
                    } else {
                        self.process(d, s, g, &keymap, &curve_ids, &mut allpolys, &mut used)?.1
                    });
                    g.children.iter().collect()
                };
                for (k, part) in parts.into_iter().enumerate() {
                    let (rings, segments) = self.process(d, s, part, &keymap, &curve_ids, &mut allpolys, &mut used)?;
                    let outer: Vec<&Vec<P2>> = rings.iter().filter(|(f, _)| *f == 1).map(|(_, r)| r).collect();
                    let holes: Vec<&[P2]> = rings.iter().filter(|(f, _)| *f != 1).map(|(_, r)| r.as_slice()).collect();
                    let [outer] = outer.as_slice() else { return Err(format!("outer loops {}", outer.len())) };
                    let point = geom::interior_point(outer, &holes).ok_or("no interior point")?;
                    let naming_segments = naming.as_ref().map(|n| if k == 0 { n.clone() } else { Vec::new() });
                    pts.push(RegionEnt { point, segments, naming_segments });
                }
            }
        }
        if pts.is_empty() {
            return Err("no 0D57BD2F for profile".into());
        }
        Ok(pts)
    }

    /// The closed rings of a region's loops and the curve pieces they are made of.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn process(
        &mut self,
        d: &Design,
        s: &Sketch,
        g: &Region,
        keymap: &HashMap<(u64, u64), u64>,
        curve_ids: &[u64],
        allpolys: &mut Option<Vec<(u64, Option<Vec<P2>>)>>,
        used: &mut Option<HashSet<u64>>,
    ) -> Result<(Vec<(u8, Vec<P2>)>, Vec<Rc<Seg>>), String> {
        let mut rings = Vec::new();
        let mut segs = Vec::new();
        for l in &g.loops {
            self.next_id += 1;
            let loopkey = self.next_id;
            let mut full: Vec<(u64, &Entry, Vec<P2>)> = Vec::new();
            for c in &l.curves {
                let q = keymap.get(&(c.primary, c.secondary)).copied().ok_or("curve unknown")?;
                let pl = curve_polyline(d, s, q).ok_or_else(|| format!("curve {}", d.cls(q).get(..8).unwrap_or("")))?;
                full.push((q, c, pl));
            }
            let n = full.len();
            let mut polys: Vec<Vec<P2>> = Vec::new();
            for (k, (q, c, pl)) in full.iter().enumerate() {
                let mut pl = pl.clone();
                if c.b > 1 {
                    // A piece of a curve cut by other sketch curves: piece a of b.
                    let all = allpolys
                        .get_or_insert_with(|| curve_ids.iter().map(|kk| (*kk, curve_polyline(d, s, *kk))).collect());
                    let used_set = used.get_or_insert_with(|| {
                        let keys = self.used_curves(d).get(&s.id).cloned().unwrap_or_default();
                        keymap.iter().filter(|(k, _)| keys.contains(k)).map(|(_, q)| *q).collect()
                    });
                    let closed = match (pl.first(), pl.last()) {
                        (Some(a), Some(b)) => geom::dist(*a, *b) < 1e-9,
                        _ => false,
                    };
                    let first: Vec<&[P2]> = all
                        .iter()
                        .filter(|(kk, v)| kk != q && v.is_some() && used_set.contains(kk))
                        .filter_map(|(_, v)| v.as_deref())
                        .collect();
                    let mut pc = geom::piece(&pl, closed, &first, c.a, c.b);
                    if pc.is_none() && !closed {
                        let prev = if n > 1 { full.get((k + n - 1) % n).map(|f| f.2.as_slice()) } else { None };
                        let next = if n > 1 { full.get((k + 1) % n).map(|f| f.2.as_slice()) } else { None };
                        pc = geom::trim_by_neighbours(&pl, prev, next, c.a, c.b);
                    }
                    if pc.is_none() {
                        let everything: Vec<&[P2]> =
                            all.iter().filter(|(kk, _)| kk != q).filter_map(|(_, v)| v.as_deref()).collect();
                        pc = geom::piece(&pl, closed, &everything, c.a, c.b);
                    }
                    pl = pc.ok_or_else(|| format!("curve piece {}/{} not reproduced", c.a, c.b))?;
                }
                polys.push(pl.clone());
                self.next_id += 1;
                segs.push(Rc::new(Seg { id: self.next_id, curve: *q, poly: pl, flag: l.flag, loopkey }));
            }
            let mut ring = geom::chain(&polys);
            if ring.is_none() {
                // The record can leave out a curve of the loop (a corner arc that a duplicate
                // arc of the sketch shadows): a sketch curve that joins exactly the open ends
                // completes it.
                let all = allpolys
                    .get_or_insert_with(|| curve_ids.iter().map(|kk| (*kk, curve_polyline(d, s, *kk))).collect());
                let in_loop: Vec<u64> = full.iter().map(|f| f.0).collect();
                for (q, pl) in geom::close_gaps(&mut polys, all, &in_loop) {
                    self.next_id += 1;
                    segs.push(Rc::new(Seg { id: self.next_id, curve: q, poly: pl, flag: l.flag, loopkey }));
                }
                ring = geom::chain(&polys);
            }
            rings.push((l.flag, ring.ok_or("loop does not close")?));
        }
        Ok((rings, segs))
    }
}
