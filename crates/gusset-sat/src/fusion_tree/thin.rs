//! Thin extrudes (section 33 of `docs/formats/fusion-design-stream.md`): an extrude with a
//! `Side1WallThickness` parameter extrudes a wall along an open chain of sketch curves (no
//! profile record: the record selects the curves, `5A1BF548` -> `E2CEFD18`, as a sweep path
//! does). The tree has no thin extrude: the wall's outline becomes a sketch of its own (the chain
//! offset to both sides of the wall, closed by two caps) and the extrude extrudes that region.

use super::Ctx;
use super::feature::{extrude_raw, pget};
use super::geom::{V3, dot, mm, round6, world};
use super::naming::FaceG;
use super::sketch;
use super::stream::{param, ref_ids, refs_at};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

type P2 = [f64; 2];

/// The parameter holder class (`D91D429C`) and the parameter class.
const HOLDER: &str = "D91D429C";

/// The planar faces of a thin extrude's wall (component coordinates, cm). Its faces are named
/// by generator: `10 + k` the two sides along the k-th selected curve, `10 + n` the cap across
/// the chain's start, `10 + n + 1` the one across its end (5981 and 6299, chains of three
/// curves). The extrude is two operations; of a curve's two sides the one on the chain itself
/// carries both (5981: its edges lie on the chain's points), the other only the first. The sides
/// along arcs are not modelled.
pub(super) struct Caps {
    n: usize,
    start: (V3, V3),
    end: (V3, V3),
    /// Per selected curve that is a line: a point of it and its left normal.
    sides: Vec<Option<(V3, V3)>>,
    /// The offset (cm, along the left normal) of the side off the chain.
    off: f64,
}

impl Caps {
    pub(super) fn face(&self, g: &str, on_chain: bool) -> Option<FaceG> {
        let k = g.parse::<usize>().ok()?.checked_sub(10)?;
        let (p, n) = if k < self.n {
            let (p, n) = (*self.sides.get(k)?)?;
            let t = if on_chain { 0.0 } else { self.off };
            ([p[0] + t * n[0], p[1] + t * n[1], p[2] + t * n[2]], n)
        } else {
            match k - self.n {
                0 => self.start,
                1 => self.end,
                _ => return None,
            }
        };
        Some(FaceG::Plane { n, p, cap: true, mid: p })
    }
}

/// How near two ends must be to meet (mm): the sketch's points are written to 6 decimals.
const CLOSE: f64 = 1e-5;

/// One curve of the chain in travel order.
#[derive(Clone, Copy, Debug)]
enum Piece {
    Line {
        a: P2,
        b: P2,
    },
    /// Centre, radius, start and end angle of the CCW arc, and whether the chain runs it CCW.
    Arc {
        c: P2,
        r: f64,
        a0: f64,
        a1: f64,
        ccw: bool,
    },
}

impl Piece {
    fn ends(&self) -> (P2, P2) {
        match *self {
            Piece::Line { a, b } => (a, b),
            Piece::Arc { c, r, a0, a1, ccw } => {
                let p = |t: f64| [c[0] + r * libm::cos(t), c[1] + r * libm::sin(t)];
                if ccw { (p(a0), p(a1)) } else { (p(a1), p(a0)) }
            }
        }
    }

    /// Unit travel directions at the start and the end.
    fn dirs(&self) -> (P2, P2) {
        match *self {
            Piece::Line { a, b } => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let l = libm::hypot(d[0], d[1]);
                let u = [d[0] / l, d[1] / l];
                (u, u)
            }
            Piece::Arc { a0, a1, ccw, .. } => {
                // CCW at angle t runs along (-sin t, cos t).
                let t = |a: f64, s: f64| [-s * libm::sin(a), s * libm::cos(a)];
                if ccw { (t(a0, 1.0), t(a1, 1.0)) } else { (t(a1, -1.0), t(a0, -1.0)) }
            }
        }
    }

    /// The curve moved `s` to the left of the travel direction.
    fn offset(&self, s: f64) -> Option<Piece> {
        match *self {
            Piece::Line { a, b } => {
                let (u, _) = self.dirs();
                let n = [-u[1], u[0]];
                Some(Piece::Line { a: [a[0] + s * n[0], a[1] + s * n[1]], b: [b[0] + s * n[0], b[1] + s * n[1]] })
            }
            Piece::Arc { c, r, a0, a1, ccw } => {
                // Left of a CCW run is the centre.
                let r2 = if ccw { r - s } else { r + s };
                (r2 > 1e-9).then_some(Piece::Arc { c, r: r2, a0, a1, ccw })
            }
        }
    }

    /// The point halfway along the curve.
    fn middle(&self) -> P2 {
        match *self {
            Piece::Line { a, b } => [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
            Piece::Arc { c, r, a0, a1, .. } => {
                let m = (a0 + a1) / 2.0;
                [c[0] + r * libm::cos(m), c[1] + r * libm::sin(m)]
            }
        }
    }
}

/// The selected curves of a sketch (in selection order), as a chain where each curve starts where the last ended
/// and runs on in the same direction (no corner: an offset of a corner would need a join).
fn chain(curves: &[Piece]) -> Option<Vec<Piece>> {
    let close = |p: P2, q: P2| libm::hypot(p[0] - q[0], p[1] - q[1]) < CLOSE;
    let flip = |p: Piece| match p {
        Piece::Line { a, b } => Piece::Line { a: b, b: a },
        Piece::Arc { c, r, a0, a1, ccw } => Piece::Arc { c, r, a0, a1, ccw: !ccw },
    };
    // The chain runs the way its first curve does (a line from its start to its end, an arc
    // counter-clockwise about the sketch's normal).
    let mut left: Vec<Piece> = curves.to_vec();
    if left.is_empty() {
        return None;
    }
    let mut out = vec![left.remove(0)];
    while !left.is_empty() {
        let (s, _) = out.first()?.ends();
        let (_, e) = out.last()?.ends();
        let k = left.iter().position(|p| {
            let (a, b) = p.ends();
            close(a, e) || close(b, e) || close(a, s) || close(b, s)
        })?;
        let p = left.remove(k);
        let (a, b) = p.ends();
        if close(a, e) {
            out.push(p);
        } else if close(b, e) {
            out.push(flip(p));
        } else if close(b, s) {
            out.insert(0, p);
        } else {
            out.insert(0, flip(p));
        }
    }
    let (s, _) = out.first()?.ends();
    let (_, e) = out.last()?.ends();
    if close(s, e) {
        return None;
    }
    for w in out.windows(2) {
        let (Some(p), Some(q)) = (w.first(), w.get(1)) else { return None };
        let (_, du) = p.dirs();
        let (dv, _) = q.dirs();
        // A corner only between two lines (their offsets meet where they cross); not a turn back.
        let tangent = (du[0] * dv[0] + du[1] * dv[1] - 1.0).abs() <= 1e-6;
        let lines = matches!((p, q), (Piece::Line { .. }, Piece::Line { .. }));
        if !tangent && !(lines && (du[0] * dv[1] - du[1] * dv[0]).abs() > 1e-6) {
            return None;
        }
    }
    Some(out)
}

/// The offsets of the chain's curves, `s` to its left, each corner between two lines closed where
/// the offset lines cross.
fn offsets(ch: &[Piece], s: f64) -> Option<Vec<Piece>> {
    let mut a: Vec<Piece> = ch.iter().map(|p| p.offset(s)).collect::<Option<_>>()?;
    for k in 1..a.len() {
        let (Some(Piece::Line { a: p0, b: p1 }), Some(Piece::Line { a: q0, b: q1 })) =
            (a.get(k - 1).copied(), a.get(k).copied())
        else {
            continue;
        };
        let (u, v) = ([p1[0] - p0[0], p1[1] - p0[1]], [q1[0] - q0[0], q1[1] - q0[1]]);
        let den = u[0] * v[1] - u[1] * v[0];
        if den.abs() < 1e-12 * libm::hypot(u[0], u[1]) * libm::hypot(v[0], v[1]) {
            continue;
        }
        let t = ((q0[0] - p0[0]) * v[1] - (q0[1] - p0[1]) * v[0]) / den;
        let x = [p0[0] + t * u[0], p0[1] + t * u[1]];
        if let Some(Piece::Line { b, .. }) = a.get_mut(k - 1) {
            *b = x;
        }
        if let Some(Piece::Line { a: q, .. }) = a.get_mut(k) {
            *q = x;
        }
    }
    Some(a)
}

/// The outline of the wall between the offsets `lo` < `hi` of the chain (to its left): its
/// points (mm), curves (tree sketch JSON) and a point inside it.
fn outline(ch: &[Piece], lo: f64, hi: f64) -> Option<(Vec<Value>, Vec<Value>, P2)> {
    let a = offsets(ch, lo)?;
    let b = offsets(ch, hi)?;
    let mut pts: Vec<P2> = Vec::new();
    let id = |p: P2, pts: &mut Vec<P2>| -> String {
        let k = pts.iter().position(|q| libm::hypot(p[0] - q[0], p[1] - q[1]) < CLOSE).unwrap_or_else(|| {
            pts.push(p);
            pts.len() - 1
        });
        format!("w{k}")
    };
    let mut curves: Vec<Value> = Vec::new();
    for (k, p) in a.iter().chain(b.iter()).enumerate() {
        let cid = format!("t{k}");
        match *p {
            Piece::Line { a, b } => {
                let (s, e) = (id(a, &mut pts), id(b, &mut pts));
                curves.push(json!({"id": cid, "type": "line", "start": s, "end": e}));
            }
            Piece::Arc { c, r, a0, a1, .. } => {
                let at = |t: f64| [c[0] + r * libm::cos(t), c[1] + r * libm::sin(t)];
                let (cc, s, e) = (id(c, &mut pts), id(at(a0), &mut pts), id(at(a1), &mut pts));
                curves.push(json!({"id": cid, "type": "arc", "center": cc, "start": s, "end": e}));
            }
        }
    }
    let (sa, _) = a.first()?.ends();
    let (_, ea) = a.last()?.ends();
    let (sb, _) = b.first()?.ends();
    let (_, eb) = b.last()?.ends();
    for (k, (p, q)) in [(sa, sb), (ea, eb)].into_iter().enumerate() {
        let (s, e) = (id(p, &mut pts), id(q, &mut pts));
        curves.push(json!({"id": format!("t{}", a.len() + b.len() + k), "type": "line", "start": s, "end": e}));
    }
    let mid = ch.first()?.offset((lo + hi) / 2.0)?.middle();
    let points = pts
        .iter()
        .enumerate()
        .map(|(k, p)| json!({"id": format!("w{k}"), "x": round6(p[0]), "y": round6(p[1])}))
        .collect();
    Some((points, curves, mid))
}

/// A sketch feature's points (mm) by id and its curves as pieces, for the curve ids given.
fn pieces_of(sk: &Value, ids: &[String]) -> Option<Vec<Piece>> {
    let pts: HashMap<&str, P2> = sk
        .get("points")?
        .as_array()?
        .iter()
        .filter_map(|p| Some((p.get("id")?.as_str()?, [p.get("x")?.as_f64()?, p.get("y")?.as_f64()?])))
        .collect();
    let mut out = Vec::new();
    for want in ids {
        let c = sk.get("curves")?.as_array()?.iter().find(|c| c.get("id").and_then(Value::as_str) == Some(want))?;
        let pt = |k: &str| c.get(k).and_then(Value::as_str).and_then(|i| pts.get(i)).copied();
        match c.get("type")?.as_str()? {
            "line" => out.push(Piece::Line { a: pt("start")?, b: pt("end")? }),
            "arc" => {
                let (cc, s, e) = (pt("center")?, pt("start")?, pt("end")?);
                let r = libm::hypot(s[0] - cc[0], s[1] - cc[1]);
                if (libm::hypot(e[0] - cc[0], e[1] - cc[1]) - r).abs() > 1e-6 {
                    return None;
                }
                let a0 = libm::atan2(s[1] - cc[1], s[0] - cc[0]);
                let mut a1 = libm::atan2(e[1] - cc[1], e[0] - cc[0]);
                while a1 <= a0 {
                    a1 += 2.0 * std::f64::consts::PI;
                }
                out.push(Piece::Arc { c: cc, r, a0, a1, ccw: true });
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The wall's side and thickness: the `u8 1, u32 location` just before the reference to the
/// `Side1WallThickness` holder in the extrude's record.
fn wall(cx: &Ctx, fid: u64) -> Option<(u32, f64)> {
    let d = cx.d;
    let rec = d.rec(fid);
    for (at, q) in refs_at(d, rec, 0) {
        if !d.is(q, HOLDER) {
            continue;
        }
        let Some(p) = ref_ids(d, d.rec(q)).into_iter().filter(|x| d.is(*x, super::PARAM)).find_map(|x| param(d, x))
        else {
            continue;
        };
        if p.name.as_deref() != Some("Side1WallThickness") {
            continue;
        }
        let flag = rec.get(at.checked_sub(5)?).copied()?;
        let loc = u32::from_le_bytes(rec.get(at.checked_sub(4)?..at)?.try_into().ok()?);
        return (flag == 1).then_some((loc, p.value?));
    }
    None
}

/// Rewrites a thin extrude `j` (read by `extrude_json`) as an extrude of the wall's outline,
/// pushing the outline's sketch to `feats`; leaves `j` with a reason where it cannot.
pub(super) fn thin_extrude(
    cx: &mut Ctx,
    fid: u64,
    j: &mut Map<String, Value>,
    feats: &mut Vec<Value>,
    sketch_names: &HashMap<u64, String>,
    nm: &str,
) {
    let d = cx.d;
    let Some(e) = extrude_raw(d, fid) else { return };
    if pget(&e.params, "Side1WallThickness").is_none() {
        return;
    }
    let fail = |j: &mut Map<String, Value>, why: &str| {
        j.insert("profiles".into(), Value::Null);
        j.insert("profile_note".into(), format!("a thin extrude: {why}").into());
    };
    if e.sketch.is_some() || pget(&e.params, "Side2WallThickness").is_some() {
        return fail(j, "of a closed profile or with a second wall");
    }
    let Some((loc, t)) = wall(cx, fid) else { return fail(j, "the wall's side is not read") };
    // The wall's offsets to the left of the chain (mm), by its location.
    let w = mm(t);
    // 5981's wall (2) lies to the left of its chain: its sides meet at the chain's own points
    // and 0.6 mm outside its arc (the stored edges of Extrude3's faces).
    let (lo, hi) = match loc {
        2 => (0.0, w),
        // Location 1 (6126, 6299) would be the right side; no rebuild confirms it yet.
        1 => return fail(j, "wall location 1 (the right of its chain?) is not confirmed by a rebuild yet"),
        _ => return fail(j, &format!("wall location {loc} is not established")),
    };
    // The curves, as a sweep path names them.
    let mut sk: Option<u64> = None;
    let mut ids: Vec<String> = Vec::new();
    for q in ref_ids(d, d.rec(fid)).into_iter().filter(|q| d.is(*q, "5A1BF548")) {
        for c in ref_ids(d, d.rec(q)).into_iter().filter(|c| d.is(*c, "E2CEFD18")) {
            let r = d.rec(c);
            let word = |o: usize| r.get(o..o + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
            let at = r.len().saturating_sub(16);
            let (Some(s), Some(prim)) = (word(at), word(at + 8)) else { return fail(j, "a curve is not read") };
            if sk.is_some_and(|x| x != s) {
                return fail(j, "its curves are in several sketches");
            }
            sk = Some(s);
            let skx = cx.sketch(s);
            match sketch::curve_by_primary(d, &skx, prim) {
                Some(cid) if !ids.contains(&format!("c{cid}")) => ids.push(format!("c{cid}")),
                Some(_) => {}
                None => return fail(j, "a curve is not in its sketch"),
            }
        }
    }
    let Some(sid) = sk else { return fail(j, "it names no curves") };
    let Some(name) = sketch_names.get(&sid) else { return fail(j, "its sketch is not read") };
    let Some(sketch_feat) = feats
        .iter()
        .rfind(|f| {
            f.get("type").and_then(Value::as_str) == Some("sketch")
                && f.get("name").and_then(Value::as_str) == Some(name)
        })
        .cloned()
    else {
        return fail(j, "its sketch is not in the tree");
    };
    let Some(ch) = pieces_of(&sketch_feat, &ids).and_then(|p| chain(&p)) else {
        return fail(j, "its curves are not one open chain (corners only between lines)");
    };
    let Some((points, curves, mid)) = outline(&ch, lo, hi) else { return fail(j, "the wall does not fit its arcs") };
    // The direction: the sketch's normal, or against it.
    let plane = cx.sketch(sid).plane;
    if let (Some(pl), Some(v)) = (plane, e.vec) {
        let along: V3 = pl.z;
        if dot(along, v).abs() < 1e-6 {
            return fail(j, "its direction lies in its sketch plane");
        }
        if dot(along, v) < 0.0
            && let Some(dist) = j.get("distance").and_then(Value::as_f64)
        {
            j.insert("distance".into(), (-dist).into());
        }
    }
    if let (Some(pl), Some(first), Some(last)) = (plane, ch.first(), ch.last()) {
        let cap = |p: P2, u: P2| -> (V3, V3) {
            let at = world(&pl, [p[0] / 10.0, p[1] / 10.0], 0.0);
            let o = world(&pl, [0.0, 0.0], 0.0);
            let d = world(&pl, [u[0], u[1]], 0.0);
            (at, [d[0] - o[0], d[1] - o[1], d[2] - o[2]])
        };
        let ((s0, _), (_, e1)) = (first.ends(), last.ends());
        let ((u0, _), (_, u1)) = (first.dirs(), last.dirs());
        // The sides by selection order (the chain may have put a curve the other way round).
        let sel = pieces_of(&sketch_feat, &ids).unwrap_or_default();
        let sides = sel
            .iter()
            .map(|p| match *p {
                Piece::Line { a, b } => {
                    // The left of the chain's run: find the chain's own piece on this line.
                    let along = ch.iter().find_map(|q| match *q {
                        Piece::Line { a: qa, b: qb }
                            if (libm::hypot(qa[0] - a[0], qa[1] - a[1]) < CLOSE
                                && libm::hypot(qb[0] - b[0], qb[1] - b[1]) < CLOSE)
                                || (libm::hypot(qa[0] - b[0], qa[1] - b[1]) < CLOSE
                                    && libm::hypot(qb[0] - a[0], qb[1] - a[1]) < CLOSE) =>
                        {
                            Some(q.dirs().0)
                        }
                        _ => None,
                    })?;
                    let (pt, dir) = cap(a, along);
                    // Left normal of the run, about the sketch's normal.
                    let z = [pl.z[0], pl.z[1], pl.z[2]];
                    let n =
                        [z[1] * dir[2] - z[2] * dir[1], z[2] * dir[0] - z[0] * dir[2], z[0] * dir[1] - z[1] * dir[0]];
                    Some((pt, n))
                }
                Piece::Arc { .. } => None,
            })
            .collect();
        let off = if lo != 0.0 { lo } else { hi } / 10.0;
        cx.thin_caps.insert(fid, Caps { n: ids.len(), start: cap(s0, u0), end: cap(e1, u1), sides, off });
    }
    let copy = format!("{nm} wall");
    let mut s = Map::new();
    for (k, v) in sketch_feat.as_object().into_iter().flatten() {
        if matches!(
            k.as_str(),
            "type" | "plane" | "plane_note" | "source_id" | "_component" | "component" | "in_timeline"
        ) {
            s.insert(k.clone(), v.clone());
        }
    }
    s.insert("name".into(), copy.clone().into());
    s.insert("points".into(), points.into());
    s.insert("curves".into(), curves.into());
    feats.push(Value::Object(s));
    j.insert("sketch".into(), copy.into());
    j.insert("sketch_source_id".into(), sid.into());
    j.insert("profiles".into(), json!([{"point": [round6(mid[0]), round6(mid[1])]}]));
    j.remove("profile_note");
    j.insert(
        "profile_from".into(),
        format!("the outline of a thin extrude's wall ({} mm, location {loc}) along its curves", round6(w)).into(),
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn a_hairpin_gets_a_closed_wall_around_it() {
        // 5981's Extrude3: two parallel lines joined by a half circle of radius 1.4 mm.
        let c = [3.0, -5.4];
        let (s, e) = ([2.521172, -6.71557], [3.478828, -4.08443]);
        let a0 = libm::atan2(s[1] - c[1], s[0] - c[0]);
        let a1 = a0 + std::f64::consts::PI;
        let r = libm::hypot(s[0] - c[0], s[1] - c[1]);
        let curves = [
            Piece::Line { a: s, b: [-10.634525, -1.927288] },
            Piece::Line { a: [-9.676868, 0.703852], b: e },
            Piece::Arc { c, r, a0, a1, ccw: true },
        ];
        let ch = chain(&curves).unwrap();
        assert_eq!(ch.len(), 3);
        let (pts, cs, mid) = outline(&ch, -0.3, 0.3).unwrap();
        // Two offsets of three curves and two caps; the offsets meet where the curves met.
        assert_eq!(cs.len(), 8);
        assert_eq!(pts.len(), 2 * 4 + 1);
        // A wall centred on the chain has its inside point on the chain.
        assert!(libm::hypot(mid[0] - ch[0].middle()[0], mid[1] - ch[0].middle()[1]) < 1e-12);
    }

    #[test]
    fn a_corner_of_lines_is_mitred() {
        let curves = [Piece::Line { a: [0.0, 0.0], b: [1.0, 0.0] }, Piece::Line { a: [1.0, 0.0], b: [1.0, 1.0] }];
        let ch = chain(&curves).unwrap();
        // Left of a run along +x then +y is inside the corner: the offsets meet at (0.9, 0.1).
        let a = offsets(&ch, 0.1).unwrap();
        let (_, e) = a[0].ends();
        let (s, _) = a[1].ends();
        assert!(libm::hypot(e[0] - 0.9, e[1] - 0.1) < 1e-12 && libm::hypot(s[0] - 0.9, s[1] - 0.1) < 1e-12);
        // A corner with an arc is not offset.
        let arc = Piece::Arc { c: [1.0, 1.0], r: 1.0, a0: -std::f64::consts::FRAC_PI_2, a1: 0.0, ccw: true };
        assert!(
            chain(&[Piece::Line { a: [0.0, 0.0], b: [1.0, 0.0] }, Piece::Line { a: [1.0, 0.0], b: [2.0, 1.0] }, arc])
                .is_none()
        );
    }
}
