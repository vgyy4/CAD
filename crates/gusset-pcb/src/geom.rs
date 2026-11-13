//! Plane geometry shared by the outputs: arcs through three points, pad outlines and the
//! board outline as closed loops.

use crate::board::{Board, Pad, PadShape, Pt, Shape};

/// Centre and radius of the circle through three points (`None` when they are collinear).
pub fn circle_through(a: Pt, b: Pt, c: Pt) -> Option<(Pt, f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: Pt| p[0] * p[0] + p[1] * p[1];
    let ux = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let uy = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    let r = libm::hypot(a[0] - ux, a[1] - uy);
    (r.is_finite() && r > 0.0).then_some(([ux, uy], r))
}

/// Whether the arc start → mid → end turns counter-clockwise in the coordinates given
/// (mathematical orientation: positive for X right, Y up).
pub fn arc_ccw(start: Pt, mid: Pt, end: Pt) -> bool {
    (mid[0] - start[0]) * (end[1] - mid[1]) - (mid[1] - start[1]) * (end[0] - mid[0]) > 0.0
}

/// The arc's sweep (radians, positive = counter-clockwise in the given coordinates).
fn sweep(c: Pt, start: Pt, mid: Pt, end: Pt) -> f64 {
    let ang = |p: Pt| libm::atan2(p[1] - c[1], p[0] - c[0]);
    let tau = std::f64::consts::TAU;
    let (a0, a2) = (ang(start), ang(end));
    let ccw = arc_ccw(start, mid, end);
    let mut s = a2 - a0;
    if ccw {
        while s <= 0.0 {
            s += tau;
        }
    } else {
        while s >= 0.0 {
            s -= tau;
        }
    }
    // A closed arc (start = end) sweeps a full turn.
    if libm::hypot(start[0] - end[0], start[1] - end[1]) < 1e-9 {
        s = if ccw { tau } else { -tau };
    }
    s
}

/// The length of the arc start → mid → end (a straight line when the points are collinear).
pub fn arc_length(start: Pt, mid: Pt, end: Pt) -> f64 {
    match circle_through(start, mid, end) {
        Some((c, r)) => r * sweep(c, start, mid, end).abs(),
        None => libm::hypot(end[0] - start[0], end[1] - start[1]),
    }
}

/// Points along an arc (start and end included), with chords at most `tol` from it.
pub fn arc_points(start: Pt, mid: Pt, end: Pt, tol: f64) -> Vec<Pt> {
    let Some((c, r)) = circle_through(start, mid, end) else { return vec![start, end] };
    let s = sweep(c, start, mid, end);
    let step = 2.0 * libm::acos(1.0 - (tol / r).min(1.0));
    let n = if step > 1e-6 { ((s.abs() / step).ceil() as usize).clamp(2, 720) } else { 720 };
    let a0 = libm::atan2(start[1] - c[1], start[0] - c[0]);
    let mut out: Vec<Pt> = (0..=n)
        .map(|i| {
            let a = a0 + s * (i as f64) / (n as f64);
            [c[0] + r * libm::cos(a), c[1] + r * libm::sin(a)]
        })
        .collect();
    if let Some(l) = out.last_mut() {
        *l = end;
    }
    if let Some(f) = out.first_mut() {
        *f = start;
    }
    out
}

/// A KiCad 5 arc (centre, start point, sweep in degrees) as start, mid and end points.
pub fn arc_from_center(c: Pt, start: Pt, angle_deg: f64) -> Option<(Pt, Pt, Pt)> {
    if !angle_deg.is_finite() || angle_deg.abs() < 1e-9 {
        return None;
    }
    let rot = |a: f64| {
        let (s, co) = (libm::sin(a.to_radians()), libm::cos(a.to_radians()));
        let (dx, dy) = (start[0] - c[0], start[1] - c[1]);
        [c[0] + dx * co - dy * s, c[1] + dx * s + dy * co]
    };
    Some((start, rot(angle_deg / 2.0), rot(angle_deg)))
}

/// One piece of a closed contour: a line or an arc to the next point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Piece {
    Line { a: Pt, b: Pt },
    Arc { start: Pt, mid: Pt, end: Pt },
}

impl Piece {
    pub fn start(&self) -> Pt {
        match *self {
            Piece::Line { a, .. } => a,
            Piece::Arc { start, .. } => start,
        }
    }
    pub fn end(&self) -> Pt {
        match *self {
            Piece::Line { b, .. } => b,
            Piece::Arc { end, .. } => end,
        }
    }
    fn reversed(self) -> Piece {
        match self {
            Piece::Line { a, b } => Piece::Line { a: b, b: a },
            Piece::Arc { start, mid, end } => Piece::Arc { start: end, mid, end: start },
        }
    }
}

/// A closed loop of the board outline.
#[derive(Debug, Clone, PartialEq)]
pub enum Contour {
    /// A full circle.
    Circle { center: Pt, radius: f64 },
    /// Pieces end to end; the last ends where the first starts.
    Pieces(Vec<Piece>),
}

impl Contour {
    /// The loop as a polygon (arcs as chords within `tol`).
    pub fn polygon(&self, tol: f64) -> Vec<Pt> {
        match self {
            Contour::Circle { center, radius } => {
                let s = [center[0] + radius, center[1]];
                let m = [center[0] - radius, center[1]];
                let mut v = arc_points(s, [center[0], center[1] + radius], m, tol);
                v.pop();
                let mut w = arc_points(m, [center[0], center[1] - radius], s, tol);
                w.pop();
                v.extend(w);
                v
            }
            Contour::Pieces(p) => {
                let mut v = Vec::new();
                for x in p {
                    match *x {
                        Piece::Line { a, .. } => v.push(a),
                        Piece::Arc { start, mid, end } => {
                            let mut pts = arc_points(start, mid, end, tol);
                            pts.pop();
                            v.extend(pts);
                        }
                    }
                }
                v
            }
        }
    }
}

/// Signed area of a polygon (positive = counter-clockwise in the given coordinates).
pub fn area(p: &[Pt]) -> f64 {
    let n = p.len();
    let mut s = 0.0;
    for i in 0..n {
        let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
        s += a[0] * b[1] - b[0] * a[1];
    }
    s / 2.0
}

/// Whether `q` is inside the polygon (even-odd rule).
pub fn inside(p: &[Pt], q: Pt) -> bool {
    let n = p.len();
    let mut c = false;
    for i in 0..n {
        let (Some(a), Some(b)) = (p.get(i), p.get((i + n - 1) % n)) else { continue };
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

/// The board outline chained into closed loops, and the ends of pieces that did not close
/// (a gap in `Edge.Cuts`). Points closer than `tol` are joined.
pub fn outline_loops(board: &Board, tol: f64) -> (Vec<Contour>, Vec<Pt>) {
    let mut loops = Vec::new();
    let mut open: Vec<Piece> = Vec::new();
    for g in board.outline() {
        match &g.shape {
            Shape::Circle { center, radius } if *radius > 0.0 => {
                loops.push(Contour::Circle { center: *center, radius: *radius });
            }
            Shape::Circle { .. } => {}
            Shape::Line { a, b } => open.push(Piece::Line { a: *a, b: *b }),
            Shape::Arc { start, mid, end } => open.push(Piece::Arc { start: *start, mid: *mid, end: *end }),
            Shape::Poly { pts } => {
                let n = pts.len();
                let v: Vec<Piece> = (0..n)
                    .filter_map(|i| Some(Piece::Line { a: *pts.get(i)?, b: *pts.get((i + 1) % n)? }))
                    .filter(|p| dist(p.start(), p.end()) > tol)
                    .collect();
                if v.len() >= 2 {
                    loops.push(Contour::Pieces(v));
                }
            }
        }
    }
    open.retain(|p| dist(p.start(), p.end()) > tol || matches!(p, Piece::Arc { .. }));
    let mut gaps = Vec::new();
    // Greedy chaining: O(n²), outlines have at most a few thousand pieces.
    while let Some(first) = open.pop() {
        let mut chain = vec![first];
        loop {
            let head = chain.first().map_or([0.0; 2], Piece::start);
            let tail = chain.last().map_or([0.0; 2], Piece::end);
            if chain.len() > 1 && dist(head, tail) <= tol {
                break;
            }
            let next = open.iter().position(|p| dist(p.start(), tail) <= tol || dist(p.end(), tail) <= tol);
            match next {
                Some(i) => {
                    let p = open.swap_remove(i);
                    chain.push(if dist(p.start(), tail) <= tol { p } else { p.reversed() });
                }
                None => break,
            }
        }
        let head = chain.first().map_or([0.0; 2], Piece::start);
        let tail = chain.last().map_or([0.0; 2], Piece::end);
        let closed = dist(head, tail) <= tol && (chain.len() > 1 || matches!(chain.first(), Some(Piece::Arc { .. })));
        if closed {
            // Snap each piece's start to the previous end so the loop is exactly closed.
            let n = chain.len();
            for i in 0..n {
                let prev_end = chain.get((i + n - 1) % n).map(Piece::end);
                if let (Some(e), Some(p)) = (prev_end, chain.get_mut(i)) {
                    match p {
                        Piece::Line { a, .. } => *a = e,
                        Piece::Arc { start, .. } => *start = e,
                    }
                }
            }
            loops.push(Contour::Pieces(chain));
        } else {
            gaps.push(head);
            gaps.push(tail);
        }
    }
    (loops, gaps)
}

fn dist(a: Pt, b: Pt) -> f64 {
    libm::hypot(a[0] - b[0], a[1] - b[1])
}

/// A pad's outline on the board as a closed contour, grown by `grow` on every side
/// (negative shrinks). Circles come back as [`Contour::Circle`].
pub fn pad_contour(pad: &Pad, grow: f64) -> Option<Contour> {
    let [w, h] = pad.size;
    let (w, h) = (w + 2.0 * grow, h + 2.0 * grow);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let place = crate::board::Place { at: pad.at, angle: pad.angle };
    let line_loop = |pts: &[Pt]| {
        let n = pts.len();
        let v: Vec<Piece> = (0..n)
            .filter_map(|i| Some(Piece::Line { a: place.apply(*pts.get(i)?), b: place.apply(*pts.get((i + 1) % n)?) }))
            .collect();
        Contour::Pieces(v)
    };
    let rounded = |r: f64| rounded_rect(w, h, r, &place);
    Some(match pad.shape {
        PadShape::Circle => Contour::Circle { center: pad.at, radius: w / 2.0 },
        PadShape::Custom { anchor_circle: true } => Contour::Circle { center: pad.at, radius: w / 2.0 },
        PadShape::Rect | PadShape::Custom { .. } => {
            let (x, y) = (w / 2.0, h / 2.0);
            line_loop(&[[-x, -y], [x, -y], [x, y], [-x, y]])
        }
        PadShape::Oval => rounded(w.min(h) / 2.0),
        PadShape::RoundRect { ratio } => {
            let r = (pad.size[0].min(pad.size[1]) * ratio + grow).max(0.0);
            rounded(r)
        }
        PadShape::Trapezoid { delta } => {
            // `rect_delta` dy widens the left side, dx the bottom (KiCad's convention).
            let (x, y) = (w / 2.0, h / 2.0);
            let (dx, dy) = (delta[0] / 2.0, delta[1] / 2.0);
            line_loop(&[[-x - dx, -y + dy], [x + dx, -y - dy], [x - dx, y + dy], [-x + dx, y - dy]])
        }
    })
}

/// A rectangle `w` × `h` with corner radius `r`, placed.
fn rounded_rect(w: f64, h: f64, r: f64, place: &crate::board::Place) -> Contour {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let (x, y) = (w / 2.0, h / 2.0);
    let p = |q: Pt| place.apply(q);
    if r < 1e-9 {
        let pts = [[-x, -y], [x, -y], [x, y], [-x, y]];
        return Contour::Pieces(
            (0..4).filter_map(|i| Some(Piece::Line { a: p(*pts.get(i)?), b: p(*pts.get((i + 1) % 4)?) })).collect(),
        );
    }
    let k = r * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
    let mut v = Vec::new();
    // Corners in order: (-x,-y) → (x,-y) → (x,y) → (-x,y).
    let corners = [
        ([x - r, -y], [x - k, -y + k], [x, -y + r]),
        ([x, y - r], [x - k, y - k], [x - r, y]),
        ([-x + r, y], [-x + k, y - k], [-x, y - r]),
        ([-x, -y + r], [-x + k, -y + k], [-x + r, -y]),
    ];
    let mut last = [-x + r, -y];
    for (s, m, e) in corners {
        if dist(last, s) > 1e-9 {
            v.push(Piece::Line { a: p(last), b: p(s) });
        }
        v.push(Piece::Arc { start: p(s), mid: p(m), end: p(e) });
        last = e;
    }
    Contour::Pieces(v)
}

/// Whether a placed angle is a multiple of 90° (pads at such angles use plain apertures).
pub fn right_angle(a: f64) -> Option<u8> {
    let q = a / 90.0;
    let r = q.round();
    ((q - r).abs() < 1e-6).then(|| (r.rem_euclid(4.0)) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_through_three_points() {
        let (c, r) = circle_through([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]).unwrap_or(([9.0, 9.0], 0.0));
        assert!(c[0].abs() < 1e-12 && c[1].abs() < 1e-12 && (r - 1.0).abs() < 1e-12);
        assert!(circle_through([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]).is_none());
        assert!(arc_ccw([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]));
    }

    #[test]
    fn arc_points_stay_on_the_arc() {
        let v = arc_points([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], 0.001);
        assert!(v.len() > 10);
        assert!(v.iter().all(|p| (libm::hypot(p[0], p[1]) - 1.0).abs() < 1e-9 && p[1] >= -1e-12));
    }

    #[test]
    fn polygon_area_and_inside() {
        let sq = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        assert!((area(&sq) - 4.0).abs() < 1e-12);
        assert!(inside(&sq, [1.0, 1.0]) && !inside(&sq, [3.0, 1.0]));
        assert_eq!(right_angle(270.0), Some(3));
        assert_eq!(right_angle(-90.0), Some(3));
        assert_eq!(right_angle(45.0), None);
    }
}
