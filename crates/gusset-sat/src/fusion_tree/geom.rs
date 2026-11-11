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
    split_params_ex(poly, others, false)
}

/// Parameters on `p1..p2` of the ends of `q1..q2` when the two segments lie on one line.
fn seg_on(p1: P2, p2: P2, q1: P2, q2: P2) -> Vec<f64> {
    let (rx, ry) = (p2[0] - p1[0], p2[1] - p1[1]);
    let l2 = rx * rx + ry * ry;
    if l2 < 1e-24 {
        return Vec::new();
    }
    let l = libm::sqrt(l2);
    let mut out = Vec::new();
    for q in [q1, q2] {
        let off = ((q[0] - p1[0]) * ry - (q[1] - p1[1]) * rx) / l;
        if off.abs() > 1e-9 {
            return Vec::new();
        }
        let t = ((q[0] - p1[0]) * rx + (q[1] - p1[1]) * ry) / l2;
        if (-1e-9..=1.0 + 1e-9).contains(&t) {
            out.push(t);
        }
    }
    out
}

/// [`split_params`]; with `collinear` the ends of a straight piece lying on a straight piece
/// of `poly` cut it too.
pub(super) fn split_params_ex(poly: &[P2], others: &[&[P2]], collinear: bool) -> Vec<f64> {
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
                let (l0, l1) = (l.get(k).copied().unwrap_or(0.0), l.get(k + 1).copied().unwrap_or(0.0));
                if let Some(t) = seg_x(*a, *b, *c, *dd) {
                    out.push((l0 + t * (l1 - l0)) / tot);
                } else if collinear && poly.len() == 2 && o.len() == 2 {
                    for t in seg_on(*a, *b, *c, *dd) {
                        out.push((l0 + t * (l1 - l0)) / tot);
                    }
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
    piece_ex(poly, closed, others, a, b, false)
}

/// [`piece`]; with `collinear` the ends of a line lying on a line cut it too.
pub(super) fn piece_ex(
    poly: &[P2],
    closed: bool,
    others: &[&[P2]],
    a: u32,
    b: u32,
    collinear: bool,
) -> Option<Vec<P2>> {
    if a == 0 || a > b {
        return None;
    }
    let (a, b) = (a as usize, b as usize);
    let fs: Vec<f64> = split_params_ex(poly, others, collinear)
        .into_iter()
        .filter(|f| (*f > 1e-6 && *f < 1.0 - 1e-6) || closed)
        .collect();
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
        // The pieces are numbered by where they end: cuts in (0, 1] (a cut at the start counts
        // as 1), piece k ends at the k-th cut and starts at the one before it, piece 1 at the
        // last cut of the turn before. So the piece that holds the curve's start is piece 1
        // (a circle cut at top and bottom, starting on the right: the right half is piece 1;
        // BQ25185 Extrude2's slots) and, for a cut exactly at the start, piece 1 runs from the
        // start to the next cut.
        let mut c: Vec<f64> = fs.iter().map(|f| if *f == 0.0 { 1.0 } else { *f }).collect();
        c.sort_by(f64::total_cmp);
        let f1 = *c.get(a - 1)?;
        let f0 = if a == 1 { *c.get(b - 1)? - 1.0 } else { *c.get(a - 2)? };
        if f0 < 0.0 {
            let mut v = sub_poly(poly, f0 + 1.0, 1.0);
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

/// Distance from `p` to the polyline `q`.
fn poly_dist(p: P2, q: &[P2]) -> f64 {
    q.windows(2)
        .filter_map(|w| if let [a, b] = w { Some(seg_dist(p, *a, *b)) } else { None })
        .fold(f64::INFINITY, f64::min)
}

/// `p` lies wholly on `q`: its vertices and the middles of its segments are on `q`. Curved
/// polylines are compared within the sag of their chords.
fn covered_by(p: &[P2], q: &[P2], scale: f64) -> bool {
    if p.len() < 2 || q.len() < 2 {
        return false;
    }
    let sag = |v: &[P2]| -> f64 {
        if v.len() == 2 {
            return 0.0;
        }
        v.windows(2).filter_map(|w| if let [a, b] = w { Some(dist(*a, *b)) } else { None }).fold(0.0, f64::max) * 0.05
    };
    let tol = 1e-7 * scale + 1e-9 + sag(p).max(sag(q));
    p.iter().all(|a| poly_dist(*a, q) <= tol)
        && p.windows(2).all(|w| {
            let [a, b] = w else { return true };
            poly_dist([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], q) <= tol
        })
}

/// The polylines of a loop without those that lie wholly on another polyline of the loop: a
/// profile loop lists every sketch curve on its boundary, so where two curves lie on each
/// other (a rectangle drawn on a rectangle's side) an edge is listed twice. Of two polylines
/// that cover each other the first stays. `None` when nothing is dropped.
pub(super) fn drop_covered(polys: &[Vec<P2>]) -> Option<Vec<Vec<P2>>> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in polys.iter().flatten() {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let scale = dist(lo, hi).max(1e-6);
    let mut keep = vec![true; polys.len()];
    for (i, p) in polys.iter().enumerate() {
        for (j, q) in polys.iter().enumerate() {
            if i == j || !keep.get(j).copied().unwrap_or(false) || !covered_by(p, q, scale) {
                continue;
            }
            // Equal polylines: the later one goes.
            if covered_by(q, p, scale) && i < j {
                continue;
            }
            if let Some(k) = keep.get_mut(i) {
                *k = false;
            }
            break;
        }
    }
    if keep.iter().all(|k| *k) {
        return None;
    }
    Some(polys.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| p.clone()).collect())
}

/// A loop that walks its outline, a curve into the region, a closed curve there (a hole) and
/// the same curve back, or a closed curve that touches the outline: the outline's ring and the
/// rings of the closed curves, which must lie inside it. Polylines that lie on each other count once; open polylines with an end no
/// other open polyline meets are the curves walked both ways.
pub(super) fn bridged(polys: &[Vec<P2>]) -> Option<(Vec<P2>, Vec<Vec<P2>>)> {
    let polys = drop_covered(polys).unwrap_or_else(|| polys.to_vec());
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in polys.iter().flatten() {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let tol = 1e-4 * dist(lo, hi).max(1e-6);
    let is_closed = |p: &Vec<P2>| match (p.first(), p.last()) {
        (Some(a), Some(b)) => p.len() > 2 && dist(*a, *b) < tol,
        _ => false,
    };
    let (closed, mut open): (Vec<Vec<P2>>, Vec<Vec<P2>>) = polys.into_iter().partition(is_closed);
    if closed.is_empty() || open.is_empty() {
        return None;
    }
    let mut bridges: Vec<Vec<P2>> = Vec::new();
    loop {
        let ends: Vec<(usize, P2)> = open
            .iter()
            .enumerate()
            .flat_map(|(i, p)| [p.first(), p.last()].into_iter().flatten().map(move |e| (i, *e)))
            .collect();
        let spur = open.iter().enumerate().position(|(i, p)| {
            [p.first(), p.last()]
                .into_iter()
                .flatten()
                .any(|e| !ends.iter().any(|(j, e2)| *j != i && dist(*e, *e2) < tol))
        });
        match spur {
            Some(i) => bridges.push(open.remove(i)),
            None => break,
        }
    }
    let outer = chain(&open)?;
    let mut ring = outer.clone();
    ring.push(*outer.first()?);
    // Each closed curve hangs on a walked-twice curve or touches the outline (a circle
    // tangent to an edge), and lies inside the outline.
    let mut rings = Vec::new();
    for c in closed {
        let step =
            c.windows(2).filter_map(|w| if let [a, b] = w { Some(dist(*a, *b)) } else { None }).fold(0.0, f64::max);
        let near = 20.0 * tol + 0.05 * step;
        let hung =
            bridges.iter().any(|b| [b.first(), b.last()].into_iter().flatten().any(|e| poly_dist(*e, &c) < near));
        let (mut touch, mut far) = (f64::INFINITY, (0.0, *c.first()?));
        for v in &c {
            let dv = poly_dist(*v, &ring);
            touch = touch.min(dv);
            if dv > far.0 {
                far = (dv, *v);
            }
        }
        if !(hung || touch < near) || !inside(far.1, &outer) {
            return None;
        }
        let mut r = c;
        r.pop();
        rings.push(r);
    }
    Some((outer, rings))
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
    // A cut of a curved polyline lies on a chord, off the curve by up to the chord's sag.
    let sag = polys
        .iter()
        .filter(|p| p.len() > 2)
        .flat_map(|p| p.windows(2))
        .filter_map(|w| if let [a, b] = w { Some(dist(*a, *b)) } else { None })
        .fold(0.0, f64::max)
        * 0.05;
    let join = 1e-4 * scale + sag;
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
        if bd > join {
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
    if dist(*ring.first()?, *ring.last()?) > join {
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

/// Whether the segments `a`-`b` and `c`-`d` meet (touching counts).
fn segs_meet(a: P2, b: P2, c: P2, d: P2) -> bool {
    let o = |p: P2, q: P2, r: P2| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    if (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) {
        return true;
    }
    let on = |p: P2, q: P2, r: P2, v: f64| {
        v == 0.0 && r[0] >= p[0].min(q[0]) && r[0] <= p[0].max(q[0]) && r[1] >= p[1].min(q[1]) && r[1] <= p[1].max(q[1])
    };
    on(c, d, a, d1) || on(c, d, b, d2) || on(a, b, c, d3) || on(a, b, d, d4)
}

/// One point in every face the curves `others` cut the region (`outer` less `holes`) into; the
/// region's own interior point alone when none of them runs through it. A sketch region as
/// Fusion records it can be the union of several faces of the sketch (a disc whose record has
/// no hole although a smaller circle lies in it; a square whose corner a circle crosses): a
/// pick in each face names the whole region. Found on a grid: two grid points lie in one face
/// when a path of grid steps joins them that crosses neither the region's edges nor a curve, so
/// a face narrower than a grid step can be missed (the rebuild check then refuses the result).
pub(super) fn face_points(outer: &[P2], holes: &[&[P2]], others: &[&[P2]]) -> Option<Vec<P2>> {
    const N: usize = 96;
    let x0 = outer.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let x1 = outer.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let y0 = outer.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let y1 = outer.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let size = (x1 - x0).max(y1 - y0);
    if size.is_nan() || size <= 0.0 {
        return None;
    }
    let mut edges: Vec<(P2, P2)> = Vec::new();
    for r in std::iter::once(&outer).chain(holes.iter()) {
        for (i, a) in r.iter().enumerate() {
            if let Some(b) = r.get((i + 1) % r.len()) {
                edges.push((*a, *b));
            }
        }
    }
    // The pieces of the other curves that run through the region: off its edges, inside it.
    let tol = 1e-6 * size;
    let within = |p: P2| {
        inside(p, outer) && !holes.iter().any(|h| inside(p, h)) && edges.iter().all(|(a, b)| seg_dist(p, *a, *b) > tol)
    };
    // A curve cuts the region when a point of it lies inside, off the region's edges (looked for
    // at 16 steps along each of its chords). Its chords near the region are walls then.
    let mut cuts: Vec<(P2, P2)> = Vec::new();
    let mut cut = false;
    for c in others {
        for w in c.windows(2) {
            let [a, b] = w else { continue };
            if dist(*a, *b) == 0.0
                || a[0].max(b[0]) < x0
                || a[0].min(b[0]) > x1
                || a[1].max(b[1]) < y0
                || a[1].min(b[1]) > y1
            {
                continue;
            }
            cuts.push((*a, *b));
            cut = cut
                || (0..=16).any(|k| {
                    let t = f64::from(k) / 16.0;
                    within([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])])
                });
        }
    }
    if !cut {
        return interior_point(outer, holes).map(|p| vec![p]);
    }
    let walls: Vec<(P2, P2)> = edges.iter().chain(cuts.iter()).copied().collect();
    // Grid points off the lines of an axis-aligned sketch.
    let at = |i: usize, j: usize| {
        [x0 + (x1 - x0) * (i as f64 + 0.37) / N as f64, y0 + (y1 - y0) * (j as f64 + 0.41) / N as f64]
    };
    let ok: Vec<bool> = (0..N * N)
        .map(|k| {
            let p = at(k / N, k % N);
            inside(p, outer) && !holes.iter().any(|h| inside(p, h))
        })
        .collect();
    let mut parent: Vec<usize> = (0..N * N).collect();
    fn root(parent: &mut [usize], mut k: usize) -> usize {
        while let Some(&p) = parent.get(k) {
            if p == k {
                break;
            }
            k = p;
        }
        k
    }
    let blocked = |p: P2, q: P2| {
        let (lx, hx, ly, hy) = (p[0].min(q[0]), p[0].max(q[0]), p[1].min(q[1]), p[1].max(q[1]));
        walls.iter().any(|(a, b)| {
            a[0].max(b[0]) >= lx
                && a[0].min(b[0]) <= hx
                && a[1].max(b[1]) >= ly
                && a[1].min(b[1]) <= hy
                && segs_meet(p, q, *a, *b)
        })
    };
    for i in 0..N {
        for j in 0..N {
            let k = i * N + j;
            if ok.get(k) != Some(&true) {
                continue;
            }
            for (i2, j2) in [(i + 1, j), (i, j + 1)] {
                let k2 = i2 * N + j2;
                if i2 < N && j2 < N && ok.get(k2) == Some(&true) && !blocked(at(i, j), at(i2, j2)) {
                    let (a, b) = (root(&mut parent, k), root(&mut parent, k2));
                    if a != b
                        && let Some(slot) = parent.get_mut(a)
                    {
                        *slot = b;
                    }
                }
            }
        }
    }
    // Per face, the grid point farthest from every wall.
    let mut best: Vec<(usize, f64, P2)> = Vec::new();
    for k in 0..N * N {
        if ok.get(k) != Some(&true) {
            continue;
        }
        let p = at(k / N, k % N);
        let r = root(&mut parent, k);
        let dmin = walls.iter().map(|(a, b)| seg_dist(p, *a, *b)).fold(f64::INFINITY, f64::min);
        match best.iter_mut().find(|b| b.0 == r) {
            Some(b) if dmin > b.1 => (b.1, b.2) = (dmin, p),
            Some(_) => {}
            None => best.push((r, dmin, p)),
        }
    }
    if best.is_empty() {
        return None;
    }
    Some(best.into_iter().map(|b| b.2).collect())
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
