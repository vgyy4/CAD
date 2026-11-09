//! Small vector helpers (centimetres, as the streams store them) and 2D polyline operations
//! used to cut sketch curves into the pieces a profile names.

pub(super) type V3 = [f64; 3];
pub(super) type P2 = [f64; 2];

pub(super) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(super) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(super) fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(super) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(super) fn norm(a: V3) -> f64 {
    libm::sqrt(dot(a, a))
}
pub(super) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(super) fn unit(v: V3) -> Option<V3> {
    let n = norm(v);
    (n > 1e-15).then(|| mul(v, 1.0 / n))
}
pub(super) fn dist(a: P2, b: P2) -> f64 {
    libm::hypot(a[0] - b[0], a[1] - b[1])
}

/// A sketch's placement: axes and origin in the component's coordinates (cm).
#[derive(Debug, Clone, Copy)]
pub(super) struct Plane {
    pub x: V3,
    pub y: V3,
    pub z: V3,
    pub origin: V3,
}

/// Sketch point `(x, y)` at offset `t` along the sketch normal, in the component's coordinates.
pub(super) fn world(pl: &Plane, p: P2, t: f64) -> V3 {
    let o = pl.origin;
    [
        o[0] + p[0] * pl.x[0] + p[1] * pl.y[0] + t * pl.z[0],
        o[1] + p[0] * pl.x[1] + p[1] * pl.y[1] + t * pl.z[1],
        o[2] + p[0] * pl.x[2] + p[1] * pl.y[2] + t * pl.z[2],
    ]
}

/// Python-style `round(v, 6)` (half away from zero; only exact ties differ).
pub(super) fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}
pub(super) fn round9(v: f64) -> f64 {
    (v * 1e9).round() / 1e9
}

/// Millimetres from the stream's centimetres.
pub(super) fn mm(v: f64) -> f64 {
    round6(v * 10.0)
}

fn cumulative(poly: &[P2]) -> Vec<f64> {
    let mut l = vec![0.0];
    for w in poly.windows(2) {
        if let [a, b] = w {
            l.push(l.last().copied().unwrap_or(0.0) + dist(*a, *b));
        }
    }
    l
}

/// Parameter on `p1..p2` where it meets segment `q1..q2` (touching counts), if it does.
fn seg_x(p1: P2, p2: P2, q1: P2, q2: P2) -> Option<f64> {
    let (rx, ry) = (p2[0] - p1[0], p2[1] - p1[1]);
    let (sx, sy) = (q2[0] - q1[0], q2[1] - q1[1]);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-15 {
        return None;
    }
    let (qpx, qpy) = (q1[0] - p1[0], q1[1] - p1[1]);
    let t = (qpx * sy - qpy * sx) / den;
    let u = (qpx * ry - qpy * rx) / den;
    let e = 1e-9;
    ((-e..=1.0 + e).contains(&t) && (-e..=1.0 + e).contains(&u)).then_some(t)
}

/// Arc-length fractions (0..1) where `poly` is crossed or touched by any of the other polylines.
pub(super) fn split_params(poly: &[P2], others: &[&[P2]]) -> Vec<f64> {
    let l = cumulative(poly);
    let tot = match l.last().copied() {
        Some(t) if t != 0.0 => t,
        _ => 1.0,
    };
    let mut out = Vec::new();
    for o in others {
        for (k, w) in poly.windows(2).enumerate() {
            let [a, b] = w else { continue };
            for w2 in o.windows(2) {
                let [c, dd] = w2 else { continue };
                if let Some(t) = seg_x(*a, *b, *c, *dd) {
                    let (l0, l1) = (l.get(k).copied().unwrap_or(0.0), l.get(k + 1).copied().unwrap_or(0.0));
                    out.push((l0 + t * (l1 - l0)) / tot);
                }
            }
        }
    }
    out.sort_by(f64::total_cmp);
    let mut res: Vec<f64> = Vec::new();
    for f in out {
        if res.last().is_none_or(|p| f - p > 1e-6) {
            res.push(f);
        }
    }
    res
}

/// The part of `poly` between arc-length fractions `f0` and `f1`.
pub(super) fn sub_poly(poly: &[P2], f0: f64, f1: f64) -> Vec<P2> {
    let l = cumulative(poly);
    let tot = match l.last().copied() {
        Some(t) if t != 0.0 => t,
        _ => 1.0,
    };
    let at = |f: f64| -> P2 {
        let x = f * tot;
        for (k, w) in poly.windows(2).enumerate() {
            let [a, b] = w else { continue };
            let (l0, l1) = (l.get(k).copied().unwrap_or(0.0), l.get(k + 1).copied().unwrap_or(0.0));
            if l1 >= x - 1e-15 {
                let seg = l1 - l0;
                let t = if seg == 0.0 { 0.0 } else { (x - l0) / seg };
                return [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
            }
        }
        poly.last().copied().unwrap_or([0.0, 0.0])
    };
    let mut pts = vec![at(f0)];
    for (p, li) in poly.iter().zip(&l) {
        if f0 * tot < *li && *li < f1 * tot {
            pts.push(*p);
        }
    }
    pts.push(at(f1));
    pts
}

/// Piece `a` (1-based) of `b` of a curve cut at its crossings with the other curves; `None`
/// when the crossings do not give `b` pieces.
pub(super) fn piece(poly: &[P2], closed: bool, others: &[&[P2]], a: u32, b: u32) -> Option<Vec<P2>> {
    if a == 0 || a > b {
        return None;
    }
    let (a, b) = (a as usize, b as usize);
    let fs: Vec<f64> =
        split_params(poly, others).into_iter().filter(|f| (*f > 1e-6 && *f < 1.0 - 1e-6) || closed).collect();
    if closed {
        let mut fs: Vec<f64> =
            fs.into_iter().map(|f| round9(f.rem_euclid(1.0))).map(|f| if f >= 1.0 { 0.0 } else { f }).collect();
        fs.sort_by(f64::total_cmp);
        fs.dedup_by(|x, y| x == y);
        if fs.len() != b {
            return None;
        }
        if b == 1 {
            return Some(poly.to_vec());
        }
        let f0 = *fs.get(a - 1)?;
        let f1 = *fs.get(a % b)?;
        if f1 <= f0 {
            let mut v = sub_poly(poly, f0, 1.0);
            v.extend(sub_poly(poly, 0.0, f1).into_iter().skip(1));
            return Some(v);
        }
        return Some(sub_poly(poly, f0, f1));
    }
    let mut cuts = vec![0.0];
    cuts.extend(fs);
    cuts.push(1.0);
    if cuts.len() - 1 != b {
        return None;
    }
    Some(sub_poly(poly, *cuts.get(a - 1)?, *cuts.get(a)?))
}

/// The piece of `pl` between its meeting points with the loop neighbours (piece `a` of `b`).
pub(super) fn trim_by_neighbours(
    pl: &[P2],
    prev: Option<&[P2]>,
    next: Option<&[P2]>,
    a: u32,
    b: u32,
) -> Option<Vec<P2>> {
    let fp = prev.map(|p| split_params(pl, &[p])).unwrap_or_default();
    let fn_ = next.map(|p| split_params(pl, &[p])).unwrap_or_default();
    let mut cand: Vec<f64> = fp.iter().chain(&fn_).map(|f| round9(*f)).collect();
    cand.sort_by(f64::total_cmp);
    cand.dedup_by(|x, y| x == y);
    if a == 1 {
        let inner = cand.iter().find(|f| **f > 1e-6)?;
        return Some(sub_poly(pl, 0.0, *inner));
    }
    if a == b {
        let inner = cand.iter().rev().find(|f| **f < 1.0 - 1e-6)?;
        return Some(sub_poly(pl, *inner, 1.0));
    }
    let mut best: Option<(f64, f64)> = None;
    for f0 in &fp {
        for f1 in &fn_ {
            let (lo, hi) = (f0.min(*f1), f0.max(*f1));
            if hi - lo > 1e-6 && best.is_none_or(|(bl, bh)| hi - lo < bh - bl) {
                best = Some((lo, hi));
            }
        }
    }
    let (lo, hi) = best?;
    Some(sub_poly(pl, lo, hi))
}

/// Joins polylines end to end (either direction, nearest end first) into one closed ring.
/// Sketch curves that join the open ends of a loop's polylines: `polys` gains each curve of
/// `cands` (not already `skip`ped) both of whose ends sit on open ends, until none does.
/// Returns the curves added with their polylines (oriented as stored).
pub(super) fn close_gaps(
    polys: &mut Vec<Vec<P2>>,
    cands: &[(u64, Option<Vec<P2>>)],
    skip: &[u64],
) -> Vec<(u64, Vec<P2>)> {
    let mut added: Vec<(u64, Vec<P2>)> = Vec::new();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in polys.iter().flatten() {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let tol = 1e-4 * dist(lo, hi).max(1e-6);
    for _ in 0..8 {
        // Ends no other polyline end meets.
        let ends: Vec<P2> = polys.iter().flat_map(|p| [p.first(), p.last()]).flatten().copied().collect();
        let open: Vec<P2> = ends
            .iter()
            .enumerate()
            .filter(|(i, e)| !ends.iter().enumerate().any(|(j, e2)| j != *i && dist(**e, *e2) < tol))
            .map(|(_, e)| *e)
            .collect();
        if open.len() < 2 {
            break;
        }
        let near = |p: P2| open.iter().any(|o| dist(*o, p) < tol);
        let Some((q, pl)) = cands.iter().find_map(|(q, v)| {
            let v = v.as_ref()?;
            let (a, b) = (*v.first()?, *v.last()?);
            (!skip.contains(q) && !added.iter().any(|x| x.0 == *q) && dist(a, b) > tol && near(a) && near(b))
                .then(|| (*q, v.clone()))
        }) else {
            break;
        };
        polys.push(pl.clone());
        added.push((q, pl));
    }
    added
}

pub(super) fn chain(polys: &[Vec<P2>]) -> Option<Vec<P2>> {
    if polys.is_empty() {
        return None;
    }
    let all: Vec<P2> = polys.iter().flatten().copied().collect();
    let minx = all.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let miny = all.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let maxx = all.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let maxy = all.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let scale = dist([minx, miny], [maxx, maxy]).max(1e-6);
    let tol = 1e-6 * scale + 1e-7;
    if let [r] = polys {
        let (f, l) = (*r.first()?, *r.last()?);
        let d = dist(f, l);
        return (d < tol * 10.0 || (r.len() > 2 && d < 1e-3 * scale)).then(|| r.clone());
    }
    let mut rest: Vec<Vec<P2>> = polys.to_vec();
    let mut ring = rest.remove(0);
    let mut flipped = false;
    while !rest.is_empty() {
        let end = *ring.last()?;
        // (distance, index, reversed): the smallest tuple wins, as the first-listed on ties.
        let mut best: Option<(f64, usize, bool)> = None;
        for rev in [false, true] {
            for (k, p) in rest.iter().enumerate() {
                let e = if rev { *p.last()? } else { *p.first()? };
                let c = (dist(end, e), k, rev);
                if best.is_none_or(|b| c.0 < b.0 || (c.0 == b.0 && (c.1, c.2) < (b.1, b.2))) {
                    best = Some(c);
                }
            }
        }
        let (bd, bk, brev) = best?;
        if bd > 1e-4 * scale {
            if !flipped {
                ring.reverse();
                flipped = true;
                continue;
            }
            return None;
        }
        let mut p = rest.remove(bk);
        if brev {
            p.reverse();
        }
        ring.extend(p.into_iter().skip(1));
    }
    if dist(*ring.first()?, *ring.last()?) > 1e-4 * scale {
        return None;
    }
    ring.pop();
    Some(ring)
}

/// Even-odd point in polygon test.
pub(super) fn inside(pt: P2, ring: &[P2]) -> bool {
    let [x, y] = pt;
    let mut c = false;
    for (i, p1) in ring.iter().enumerate() {
        let Some(p2) = ring.get((i + 1) % ring.len()) else { continue };
        let (x1, y1, x2, y2) = (p1[0], p1[1], p2[0], p2[1]);
        if (y1 > y) != (y2 > y) && x < (x2 - x1) * (y - y1) / (y2 - y1) + x1 {
            c = !c;
        }
    }
    c
}

fn seg_dist(p: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx * dx + dy * dy;
    let t = if l == 0.0 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l).clamp(0.0, 1.0) };
    libm::hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)
}

/// A point inside `outer` and outside every hole, as far from all edges as a grid search finds.
pub(super) fn interior_point(outer: &[P2], holes: &[&[P2]]) -> Option<P2> {
    let x0 = outer.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let x1 = outer.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let y0 = outer.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let y1 = outer.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let mut edges: Vec<(P2, P2)> = Vec::new();
    for r in std::iter::once(&outer).chain(holes.iter()) {
        for (i, a) in r.iter().enumerate() {
            if let Some(b) = r.get((i + 1) % r.len()) {
                edges.push((*a, *b));
            }
        }
    }
    let mut best: Option<(f64, P2)> = None;
    for n in [24usize, 96] {
        for i in 1..n {
            for j in 1..n {
                let p = [x0 + (x1 - x0) * i as f64 / n as f64, y0 + (y1 - y0) * j as f64 / n as f64];
                if !inside(p, outer) || holes.iter().any(|h| inside(p, h)) {
                    continue;
                }
                let dmin = edges.iter().map(|(a, b)| seg_dist(p, *a, *b)).fold(f64::INFINITY, f64::min);
                if best.is_none_or(|b| dmin > b.0) {
                    best = Some((dmin, p));
                }
            }
        }
        if best.is_some_and(|b| b.0 > 1e-4 * (x1 - x0).max(y1 - y0)) {
            break;
        }
    }
    best.map(|b| b.1)
}
