//! Sketches (points, lines, arcs, placement) and the profile records that say which sketch
//! regions an extrude uses.

use super::geom::{self, P2, Plane, V3};
use super::stream::{
    self, Design, dbl, dbl3, parse_header, ref_end, ref_ids, ref_lists, refs_at, sections, wstrings_all,
};
use crate::cover;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

pub(super) const SKETCH_FEAT: &str = "8DA771B7";
pub(super) const SKETCH: &str = "44A64366";
const SKETCH_BASE: &str = "37AD519C";
const SK_XFORM: &str = "F47A46FB";
const POINT: &str = "C2CEDAE7";
const LINE: &str = "DCA267ED";
const ARC: &str = "F0130424";
const SPLINE: &str = "D82E012F";

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

/// A sketch spline: the stored B-spline curve (control points in cm, sketch space).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Spline {
    pub degree: usize,
    pub knots: Vec<f64>,
    /// One per control point, or empty (all 1).
    pub weights: Vec<f64>,
    pub poles: Vec<V3>,
    /// The sketch points at the first and at the last control point.
    pub start_ref: u64,
    pub end_ref: u64,
}

impl Spline {
    /// The point at parameter `t` (de Boor; rational when the curve has weights).
    pub(super) fn at(&self, t: f64) -> Option<P2> {
        let (p, n) = (self.degree, self.poles.len());
        let lo = *self.knots.get(p)?;
        let hi = *self.knots.get(n)?;
        let t = t.clamp(lo, hi);
        // The span k with knots[k] <= t < knots[k + 1], p <= k < n.
        let mut k = p;
        while k + 1 < n && self.knots.get(k + 1).is_some_and(|u| *u <= t) {
            k += 1;
        }
        let mut dpts: Vec<[f64; 3]> = Vec::with_capacity(p + 1);
        for j in 0..=p {
            let i = k + j - p;
            let q = self.poles.get(i)?;
            let w = self.weights.get(i).copied().unwrap_or(1.0);
            dpts.push([q[0] * w, q[1] * w, w]);
        }
        for r in 1..=p {
            for j in (r..=p).rev() {
                let (a, b) = (*self.knots.get(k + j - p)?, *self.knots.get(k + 1 + j - r)?);
                let alpha = if b > a { (t - a) / (b - a) } else { 0.0 };
                let (prev, cur) = (*dpts.get(j - 1)?, *dpts.get(j)?);
                *dpts.get_mut(j)? = [
                    (1.0 - alpha) * prev[0] + alpha * cur[0],
                    (1.0 - alpha) * prev[1] + alpha * cur[1],
                    (1.0 - alpha) * prev[2] + alpha * cur[2],
                ];
            }
        }
        let v = dpts.get(p)?;
        Some([v[0] / v[2], v[1] / v[2]])
    }

    /// The curve as a polyline from its first to its last control point: 24 chords per knot span.
    pub(super) fn polyline(&self) -> Option<Vec<P2>> {
        const PER_SPAN: usize = 24;
        let (first, last) = (self.poles.first()?, self.poles.last()?);
        let mut out = vec![[first[0], first[1]]];
        for k in self.degree..self.poles.len() {
            let (a, b) = (*self.knots.get(k)?, *self.knots.get(k + 1)?);
            if b <= a {
                continue;
            }
            for i in 1..=PER_SPAN {
                out.push(self.at(a + (b - a) * i as f64 / PER_SPAN as f64)?);
            }
        }
        // The curve is clamped: it ends at its last control point exactly.
        *out.last_mut()? = [last[0], last[1]];
        Some(out)
    }
}

/// One array of a spline record: its count, a capacity not below it, the element size 8, then
/// `count * width` doubles.
pub(super) fn spline_array(rec: &[u8], o: usize, width: usize) -> Option<(Vec<f64>, usize)> {
    let n = stream::u32_at(rec, o)? as usize;
    let cap = stream::u32_at(rec, o + 4)? as usize;
    if stream::u32_at(rec, o + 8)? != 8 || cap < n || n > 4096 {
        return None;
    }
    let mut out = Vec::with_capacity(n * width);
    for k in 0..n * width {
        let b: [u8; 8] = rec.get(o + 12 + 8 * k..o + 20 + 8 * k)?.try_into().ok()?;
        let v = f64::from_le_bytes(b);
        if !v.is_finite() {
            return None;
        }
        out.push(v);
    }
    Some((out, o + 12 + 8 * n * width))
}

/// The B-spline of a sketch spline record. After the references to the curve's two end points:
/// one or two zero bytes (by class version), `01`, a byte that is 1 for a closed curve, `u32`
/// degree, a tolerance double, then the knots, the weights (none, or one per control point) and
/// the control points (x, y, z; z is 0). Read only when everything agrees: knots = control points +
/// degree + 1, non-decreasing and clamped at both ends, positive weights, and the first and last
/// control points at the two referenced sketch points (which is what ties the stored curve to the
/// sketch: a curve stored in another space or trimmed to a part of its range would not end there).
fn spline(d: &Design, i: u64, points: &BTreeMap<u64, V3>) -> Option<Spline> {
    let rec = d.rec(i);
    let ends: Vec<(usize, u64)> =
        refs_at(d, rec, 0).into_iter().filter(|p| has_section(d, p.1, POINT)).take(2).collect();
    let [(_, r0), (at1, r1)] = ends.as_slice() else { return None };
    let (_, after) = ref_end(d, rec, *at1)?;
    for gap in [1usize, 2] {
        if rec.get(after..after + gap).is_none_or(|z| z.iter().any(|x| *x != 0)) || rec.get(after + gap) != Some(&1) {
            continue;
        }
        let o = after + gap;
        let closed = match rec.get(o + 1) {
            Some(0) => false,
            Some(1) => true,
            _ => continue,
        };
        let Some(degree) = stream::u32_at(rec, o + 2).map(|v| v as usize).filter(|v| (1..=11).contains(v)) else {
            continue;
        };
        let Some([tol]) = dbl::<1>(rec, o + 6) else { continue };
        if !(tol > 0.0 && tol < 1e-3) {
            continue;
        }
        let head = o;
        let Some((knots, o)) = spline_array(rec, o + 14, 1) else { continue };
        let weights_at = o;
        let Some((weights, o)) = spline_array(rec, o, 1) else { continue };
        let poles_at = o;
        let Some((flat, poles_end)) = spline_array(rec, o, 3) else { continue };
        let poles: Vec<V3> = flat.chunks_exact(3).filter_map(|c| <[f64; 3]>::try_from(c).ok()).collect();
        let n = poles.len();
        let clamped =
            |part: Option<&[f64]>| part.is_some_and(|p| p.windows(2).all(|w| matches!(w, [a, b] if same(*a, *b))));
        if n <= degree
            || knots.len() != n + degree + 1
            || !(weights.is_empty() || weights.len() == n)
            || weights.iter().any(|w| *w <= 0.0)
            || knots.windows(2).any(|w| matches!(w, [a, b] if b < a))
            || !clamped(knots.get(..=degree))
            || !clamped(knots.get(n..))
            || knots.first() >= knots.last()
            || poles.iter().any(|p| p[2].abs() > 1e-9)
        {
            continue;
        }
        let weights = if weights.iter().all(|w| same(*w, 1.0)) { Vec::new() } else { weights };
        let (first, last) = (*poles.first()?, *poles.last()?);
        let at = |r: &u64, p: V3| points.get(r).is_some_and(|q| geom::dist([q[0], q[1]], [p[0], p[1]]) < 1e-7);
        // The record names the end point first, as a line does; a closed curve names one point twice.
        let (start_ref, end_ref) = if at(r1, first) && at(r0, last) {
            (*r1, *r0)
        } else if at(r0, first) && at(r1, last) {
            (*r0, *r1)
        } else {
            continue;
        };
        if closed != (start_ref == end_ref) {
            continue;
        }
        cover::skip(rec, after, gap, "spline.pad");
        cover::field(rec, head, 2, "spline.flag-closed");
        cover::field(rec, head + 2, 4, "spline.degree");
        cover::field(rec, head + 6, 8, "spline.tolerance");
        cover::field(rec, head + 14, weights_at - head - 14, "spline.knots");
        cover::field(rec, weights_at, poles_at - weights_at, "spline.weights");
        cover::field(rec, poles_at, poles_end - poles_at, "spline.poles");
        return Some(Spline { degree, knots, weights, poles, start_ref, end_ref });
    }
    None
}

#[allow(clippy::float_cmp)]
fn same(a: f64, b: f64) -> bool {
    a == b
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
    pub splines: BTreeMap<u64, Spline>,
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
            && v[2].abs() < 1e-9
            && v.iter().all(|x| zero(*x) || (1e-200 < x.abs() && x.abs() < 1e5))
        {
            // A point on the sketch plane can carry rounding noise in z (-1e-17 after a
            // transform): it is on the plane.
            cover::field(rec, after + gap, 24, "point.xyz");
            return Some([v[0], v[1], 0.0]);
        }
    }
    None
}

/// Whether a sketch curve is a construction curve (never a profile curve). The flag is a byte
/// after the curve's last point reference; where it is depends on the record layout, and only
/// the layouts whose flag was checked against the profiles are read (every flagged curve of the
/// sample set lies outside all profiles of its sketch: 24 + 49 lines of the 303/307-byte
/// layout, 72 of the 396-byte one, one arc of the 471-byte one):
/// line 303/307 bytes: 7 after the second point reference, 396: 2, 276/300: 1 (the diagonals
/// and centre crosses of a pin-header part); arc 471: 2 after the third; line 447: 2 (four
/// flagged lines of the 150-design set, none in a profile; they are a board outline that the
/// profile's circles cross uncut).
/// Another layout reads as "not construction" (no sample has a flagged curve there).
fn construction(d: &Design, rec: &[u8], e_from: usize, line: bool) -> bool {
    let need = if line { 2 } else { 3 };
    let pts: Vec<usize> =
        refs_at(d, rec, e_from).into_iter().filter(|p| has_section(d, p.1, POINT)).map(|p| p.0).take(need).collect();
    let Some(at) = (pts.len() == need).then(|| pts.last().copied()).flatten() else { return false };
    let Some((_, end)) = ref_end(d, rec, at) else { return false };
    // The 447-byte line layout is the 396-byte one with a byte more; a record of that length
    // with other content after the reference (a `01` followed by a count) is not read as flagged.
    if line && rec.len() == 447 && rec.get(end + 3..end + 6) != Some(&[0, 0, 0]) {
        return false;
    }
    let off = construction_flag_offset(line, rec.len());
    if let Some(off) = off {
        cover::field(rec, end + off, 1, "curve.construction");
    }
    off.is_some_and(|off| rec.get(end + off) == Some(&1))
}

/// Bytes between the end of the last point reference and the construction flag, by record layout.
pub(super) fn construction_flag_offset(line: bool, record_len: usize) -> Option<usize> {
    match (line, record_len) {
        (true, 276 | 300) => Some(1),
        (true, 303 | 307) => Some(7),
        (true, 396 | 447) | (false, 471) => Some(2),
        _ => None,
    }
}

fn line(d: &Design, i: u64) -> Option<Line> {
    let (b, e) = find_section(d, i, LINE)?;
    let rec = d.rec(i);
    let v = dbl::<9>(rec, b)?;
    let slice = rec.get(..e)?;
    cover::field(rec, b, 48, "line.origin-vector");
    cover::skip(rec, b + 48, 24, "line.unread-doubles");
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
    cover::field(rec, b, 96, "arc.center-normal-xdir-radius-angles");
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
            cover::field(rec, o, 128, "sketch.transform");
            return Some((Plane { x: cols[0], y: cols[1], z: cols[2], origin: [m[3], m[7], m[11]] }, false));
        }
    }
    if rec.len() < 100 && rec.get(21..23) == Some(&[1, 0]) {
        cover::field(rec, 21, 2, "sketch.transform-identity");
        return Some((Plane { x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], origin: [0.0; 3] }, true));
    }
    None
}

pub(super) fn read_sketch(d: &Design, sid: u64) -> Sketch {
    let mut out = Sketch { id: sid, ..Sketch::default() };
    let Some((b, e)) = find_section(d, sid, SKETCH_BASE) else { return out };
    let rec = d.rec(sid);
    if let Some((xf, xe)) = ref_end(d, rec, b)
        && d.is(xf, SK_XFORM)
        && let Some((pl, assumed)) = xform(d, xf)
    {
        cover::field(rec, b, xe - b, "sketch.transform-ref");
        out.plane = Some(pl);
        out.identity_assumed = assumed;
    }
    let mut ents = std::collections::BTreeSet::new();
    if let Some(slice) = rec.get(..e) {
        for (_, ids) in ref_lists(d, slice, b) {
            ents.extend(ids);
        }
    }
    for &q in &ents {
        if has_section(d, q, POINT)
            && let Some(p) = point(d, q)
        {
            out.points.insert(q, p);
        }
    }
    // A flagged curve that a profile loop of the sketch names is a profile curve all the same
    // (11 of 534 flagged 396-byte lines of the three adafruit sets: 2895's Extrude4, 3010's
    // board outline); read lazily, only for sketches with a flagged curve.
    let mut named: Option<HashSet<(u64, u64)>> = None;
    let mut kept = |q: u64, flagged: bool| {
        !flagged || crv_key(d, q).is_some_and(|k| named.get_or_insert_with(|| profile_curve_keys(d, sid)).contains(&k))
    };
    for q in ents {
        if has_section(d, q, POINT) {
        } else if d.is(q, SPLINE)
            && let Some(sp) = spline(d, q, &out.points)
        {
            out.splines.insert(q, sp);
        } else if has_section(d, q, LINE) {
            if let Some(l) = line(d, q)
                && let Some((b, _)) = find_section(d, q, LINE)
                && kept(q, construction(d, d.rec(q), b + 72, true))
            {
                out.lines.insert(q, l);
            }
        } else if has_section(d, q, ARC) {
            if let Some(a) = arc(d, q)
                && let Some((b, _)) = find_section(d, q, ARC)
                && kept(q, construction(d, d.rec(q), b + 96, false))
            {
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
    cover::field(rec, o, 4, "region.loop-count");
    o += 4;
    if nl >= 10_000 || (nl == 0 && depth > 0) {
        return None;
    }
    let mut loops = Vec::new();
    for _ in 0..nl {
        let n = stream::u32_at(rec, o)? as usize;
        cover::field(rec, o, 4, "loop.curve-count");
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
            // (Newer streams use 64-bit hashes as curve ids, so the id itself has no bound.)
            if t > 16 {
                return None;
            }
            curves.push(Entry { primary: p, secondary: s, a, b });
            cover::field(rec, o, 4, "loop.curve-type");
            cover::field(rec, o + 4, 16, "loop.curve-ids");
            cover::skip(rec, o + 20, 4, "loop.curve-unread-u32");
            cover::field(rec, o + 24, 8, "loop.curve-piece");
            cover::skip(rec, o + 32, 8, "loop.curve-unread-8");
            o += 40;
        }
        let flag = *rec.get(o)?;
        cover::field(rec, o, 1, "loop.flag");
        o += 1;
        loops.push(Loop { flag, curves });
    }
    if depth > 0 {
        return Some((Region { loops, children: Vec::new() }, o));
    }
    let nc = stream::u32_at(rec, o)? as usize;
    cover::field(rec, o, 4, "region.child-count");
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
            let undo = cover::mark();
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
                cover::rollback(undo);
                continue;
            }
            let sec = rec
                .get(o..o + 7)
                .is_some_and(|s| s.starts_with(&[3, 0, 0, 0]) && s.iter().skip(4).all(u8::is_ascii_digit));
            if sec || o == rec.len() {
                cover::field(rec, j, after - j, "profile.sketch-ref");
                cover::skip(rec, after, skip, "profile.pad");
                cover::field(rec, after + skip, 4, "profile.region-count");
                return Some(regs);
            }
            cover::rollback(undo);
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
    /// Points in the further faces of the sketch the region is made of, where other curves of
    /// the sketch cut it (see [`geom::face_points`]).
    pub more_points: Vec<P2>,
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
    if let Some(sp) = s.splines.get(&q) {
        return sp.polyline();
    }
    // Other curve classes (splines of an unread layout): a chord through the point references, in
    // record order.
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

/// A profile loop made only of curves the sketch no longer holds. The profile records are not
/// always rewritten when a sketch is edited: 5360's connector, cut down from four pins to three,
/// still lists the fourth pin's circles (no record of the design carries their ids), and 2756's
/// still lists a deleted circle as a hole and as the island inside it. Such a hole is gone, and
/// so is a region whose outline it was.
fn gone_loop(l: &Loop, keymap: &HashMap<(u64, u64), u64>) -> bool {
    !l.curves.is_empty() && l.curves.iter().all(|c| !keymap.contains_key(&(c.primary, c.secondary)))
}

/// The `(primary, secondary)` ids of the curves that the profile loops of sketch `sid` name.
fn profile_curve_keys(d: &Design, sid: u64) -> HashSet<(u64, u64)> {
    let mut out = HashSet::new();
    for &i in &d.ids {
        if !d.is(i, "0D57BD2F") {
            continue;
        }
        let own: Vec<u64> =
            refs_at(d, d.rec(i), 0).into_iter().take(2).map(|p| p.1).filter(|q| d.is(*q, "4BD53E5A")).collect();
        if own.first().and_then(|q| sketch_of_profile(d, *q)) != Some(sid) {
            continue;
        }
        fn walk(g: &Region, out: &mut HashSet<(u64, u64)>) {
            for l in &g.loops {
                out.extend(l.curves.iter().map(|c| (c.primary, c.secondary)));
            }
            for c in &g.children {
                walk(c, out);
            }
        }
        for g in parse_profiles(d, i).unwrap_or_default() {
            walk(&g, &mut out);
        }
    }
    out
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
        for q in s.lines.keys().chain(s.arcs.keys()).chain(s.splines.keys()).chain(s.other.keys()) {
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
            let gone = |l: &Loop| gone_loop(l, &keymap);
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
                let mut first = true;
                for part in parts {
                    // A region whose outline is made only of curves the sketch no longer holds
                    // is gone (see `process`).
                    if part.loops.iter().any(|l| l.flag == 1 && gone(l)) {
                        continue;
                    }
                    let (rings, segments) = self.process(d, s, part, &keymap, &curve_ids, &mut allpolys, &mut used)?;
                    let outer: Vec<&Vec<P2>> = rings.iter().filter(|(f, _)| *f == 1).map(|(_, r)| r).collect();
                    let holes: Vec<&[P2]> = rings.iter().filter(|(f, _)| *f != 1).map(|(_, r)| r.as_slice()).collect();
                    let [outer] = outer.as_slice() else { return Err(format!("outer loops {}", outer.len())) };
                    // The sketch's other curves (whole, the loops' own included: a curve can run
                    // on through the region past the piece that bounds it).
                    let all = allpolys
                        .get_or_insert_with(|| curve_ids.iter().map(|kk| (*kk, curve_polyline(d, s, *kk))).collect());
                    let others: Vec<&[P2]> = all
                        .iter()
                        .filter(|(q, _)| !s.other.contains_key(q))
                        .filter_map(|(_, v)| v.as_deref())
                        .collect();
                    let mut points = geom::face_points(outer, &holes, &others).ok_or("no interior point")?.into_iter();
                    let point = points.next().ok_or("no interior point")?;
                    let naming_segments = naming.as_ref().map(|n| if first { n.clone() } else { Vec::new() });
                    first = false;
                    pts.push(RegionEnt { point, more_points: points.collect(), segments, naming_segments });
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
            if gone_loop(l, keymap) {
                continue;
            }
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
            let mut missing: Option<String> = None;
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
                    // The curve is cut by the other curves of its loop; the ends of a line
                    // that lies on it cut it too (646 of 647 open and 175 of 178 closed pieces
                    // of the sample set have exactly `b` pieces this way).
                    let own: Vec<&[P2]> = full.iter().filter(|f| f.0 != *q).map(|f| f.2.as_slice()).collect();
                    let mut pc = geom::piece_ex(&pl, closed, &own, c.a, c.b, true);
                    if pc.is_none() {
                        pc = geom::piece(&pl, closed, &first, c.a, c.b);
                    }
                    if pc.is_none() {
                        let everything: Vec<&[P2]> =
                            all.iter().filter(|(kk, _)| kk != q).filter_map(|(_, v)| v.as_deref()).collect();
                        pc = geom::piece(&pl, closed, &everything, c.a, c.b);
                    }
                    if pc.is_none() && !closed {
                        let prev = if n > 1 { full.get((k + n - 1) % n).map(|f| f.2.as_slice()) } else { None };
                        let next = if n > 1 { full.get((k + 1) % n).map(|f| f.2.as_slice()) } else { None };
                        pc = geom::trim_by_neighbours(&pl, prev, next, c.a, c.b);
                    }
                    match pc {
                        Some(p) => pl = p,
                        None => {
                            // Left out for now: the loop may close without it (see below).
                            missing.get_or_insert_with(|| format!("curve piece {}/{} not reproduced", c.a, c.b));
                            continue;
                        }
                    }
                }
                polys.push(pl.clone());
                self.next_id += 1;
                segs.push(Rc::new(Seg { id: self.next_id, curve: *q, poly: pl, flag: l.flag, loopkey }));
            }
            let mut ring = geom::chain(&polys);
            if ring.as_deref().is_some_and(geom::revisits)
                && let Some(fewer) = geom::drop_covered(&polys)
                && let Some(r2) = geom::chain(&fewer)
                && !geom::revisits(&r2)
            {
                // A loop that lists every edge twice can chain by walking each edge out and back
                // (3027's slot: two arcs and two lines, each drawn twice) or round twice (its
                // circle drawn twice), passing its points twice: once each, the edges make a
                // simple ring.
                ring = Some(r2);
                polys = fewer;
            }
            if ring.is_none()
                && let Some(fewer) = geom::drop_covered(&polys)
            {
                // Curves that lie on each other are all listed: each edge counts once.
                ring = geom::chain(&fewer);
                if ring.is_some() {
                    polys = fewer;
                }
            }
            let mut islands: Vec<Vec<P2>> = Vec::new();
            if ring.is_none()
                && l.flag == 1
                && let Some((outer, inner)) = geom::bridged(&polys)
            {
                // A hole joined to the outline by a curve is walked in the same loop.
                ring = Some(outer);
                islands = inner;
            }
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
            // A piece that could not be cut out of its curve is only left out when the loop's
            // other curves close without it. (512's switch pocket lists the outline's bottom edge,
            // piece 2 of 3, beside the four sides of a rectangle that closes on its own; nothing
            // in the sketch cuts that edge.)
            if ring.is_none()
                && let Some(why) = missing
            {
                return Err(why);
            }
            rings.push((l.flag, ring.ok_or("loop does not close")?));
            rings.extend(islands.into_iter().map(|r| (0, r)));
        }
        Ok((rings, segs))
    }
}
