//! 2.5D milling (ADR 0040): pocket and profile toolpaths for a flat end mill from planar
//! regions (sketch coordinates, mm), and G-code for them.
//!
//! Regions are offset with `cavalier_contours`, which keeps arcs exact: a pocket is the
//! region shrunk by the tool radius, then again by the stepover until nothing is left
//! (islands are kept clear); a profile is the boundary moved out (or in) by the tool radius.
//! Each ring is cut at every depth level: a rapid to its start at the safe height, a plunge,
//! the ring, a retract. The G-code is metric and absolute (G21 G90 G17), with G2/G3 arcs given
//! by I/J centre offsets.

use cavalier_contours::polyline::{PlineSource, PlineSourceMut, PlineVertex, Polyline};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};
use gusset_kernel::{Loop2, Region2, Seg2};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum CamError {
    /// A parameter out of range (named in the message).
    Invalid(String),
    /// The tool does not fit (nothing to cut).
    NothingToCut(String),
    TooLarge(String),
}

impl std::fmt::Display for CamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CamError::Invalid(m) | CamError::NothingToCut(m) | CamError::TooLarge(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for CamError {}

/// What to cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operation {
    /// Clear the region down to the depth; `stepover` is a fraction of the tool diameter.
    Pocket { stepover: f64 },
    /// Cut along the region's boundary: outside it (a part cut free) or inside it (a hole).
    Profile { outside: bool },
}

/// A flat end mill and how to drive it (mm, mm/min, rev/min).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    pub tool_diameter: f64,
    /// Z of the stock top (the sketch plane is z = 0).
    pub top: f64,
    /// Total depth below the top (positive).
    pub depth: f64,
    /// Depth per pass.
    pub stepdown: f64,
    /// Z for rapid moves, above the top.
    pub safe: f64,
    pub feed: f64,
    pub plunge: f64,
    pub rpm: f64,
}

/// One machine move, to the end point given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    Rapid([f64; 3]),
    /// A straight cut; `plunge` when it goes down at the plunge feed.
    Line {
        to: [f64; 3],
        plunge: bool,
    },
    /// An arc in the XY plane at constant Z about `center`.
    Arc {
        to: [f64; 3],
        center: [f64; 2],
        ccw: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Toolpath {
    pub moves: Vec<Move>,
    /// Rings cut per depth level.
    pub rings: usize,
    pub levels: Vec<f64>,
}

/// Lengths (mm) and an estimated time (min) of a toolpath.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stats {
    pub cut: f64,
    pub plunge: f64,
    pub rapid: f64,
    pub minutes: f64,
}

/// Most rings one pocket may have, and most moves one toolpath may hold.
const MAX_RINGS: usize = 10_000;
const MAX_MOVES: usize = 5_000_000;
/// Rapid feed assumed for time estimates (mm/min).
const RAPID: f64 = 5000.0;

fn bulge_of(sweep: f64) -> f64 {
    libm::tan(sweep / 4.0)
}

/// A closed loop as a polyline (arcs exact, Bézier segments sampled).
fn polyline(l: &Loop2) -> Polyline<f64> {
    let mut p = Polyline::new_closed();
    for e in &l.edges {
        match e.seg {
            Seg2::Line { a, .. } => p.add(a[0], a[1], 0.0),
            Seg2::Arc { center, start, end, ccw } => {
                let a0 = libm::atan2(start[1] - center[1], start[0] - center[0]);
                let a1 = libm::atan2(end[1] - center[1], end[0] - center[0]);
                let mut sweep = a1 - a0;
                if ccw && sweep <= 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                if !ccw && sweep >= 0.0 {
                    sweep -= std::f64::consts::TAU;
                }
                p.add(start[0], start[1], bulge_of(sweep));
            }
            Seg2::Circle { center, radius } => {
                p.add(center[0] + radius, center[1], 1.0);
                p.add(center[0] - radius, center[1], 1.0);
            }
            Seg2::Bezier { p: c } => {
                for i in 0..16 {
                    let u = f64::from(i) / 16.0;
                    let v = 1.0 - u;
                    let w = [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u];
                    let x = w.iter().zip(c.iter()).map(|(w, q)| w * q[0]).sum();
                    let y = w.iter().zip(c.iter()).map(|(w, q)| w * q[1]).sum();
                    p.add(x, y, 0.0);
                }
            }
        }
    }
    p
}

/// The regions as one shape: outer loops counter-clockwise, holes clockwise.
fn shape(regions: &[Region2]) -> Shape<f64> {
    let mut plines = Vec::new();
    for r in regions {
        let mut o = polyline(&r.outer);
        if o.area() < 0.0 {
            o.invert_direction_mut();
        }
        plines.push(o);
        for h in &r.holes {
            let mut hp = polyline(h);
            if hp.area() > 0.0 {
                hp.invert_direction_mut();
            }
            plines.push(hp);
        }
    }
    Shape::from_plines(plines)
}

fn plines_of(s: &Shape<f64>) -> Vec<Polyline<f64>> {
    s.ccw_plines.iter().chain(&s.cw_plines).map(|p| p.polyline.clone()).filter(|p| p.vertex_count() >= 2).collect()
}

fn check(s: &Setup) -> Result<(), CamError> {
    let bad = |m: &str| Err(CamError::Invalid(m.to_string()));
    let pos = |v: f64| v.is_finite() && v > 0.0;
    if !pos(s.tool_diameter) {
        return bad("the tool diameter must be a positive length");
    }
    if !pos(s.depth) || !pos(s.stepdown) {
        return bad("depth and stepdown must be positive lengths");
    }
    if s.depth / s.stepdown > 1000.0 {
        return bad("more than 1000 depth passes: use a larger stepdown");
    }
    if !(s.top.is_finite() && s.safe.is_finite() && s.safe > s.top) {
        return bad("the safe height must be above the stock top");
    }
    if !pos(s.feed) || !pos(s.plunge) || !(s.rpm.is_finite() && s.rpm >= 0.0) {
        return bad("feeds must be positive (and the spindle speed not negative)");
    }
    Ok(())
}

/// The depth levels from the first pass down to the final depth.
fn levels(s: &Setup) -> Vec<f64> {
    let n = (s.depth / s.stepdown).ceil().max(1.0) as usize;
    (1..=n).map(|i| s.top - (s.depth * i as f64 / n as f64)).collect()
}

/// The rings to cut (at every level), in cutting order.
fn rings(regions: &[Region2], op: Operation, s: &Setup) -> Result<Vec<Polyline<f64>>, CamError> {
    let opts = ShapeOffsetOptions::default();
    let base = shape(regions);
    let r = s.tool_diameter / 2.0;
    let out = match op {
        Operation::Profile { outside: true } => {
            // Grown by the tool radius: the outlines (holes are not cut).
            let grown = base.parallel_offset(-r, &opts);
            grown.ccw_plines.iter().map(|p| p.polyline.clone()).collect()
        }
        Operation::Profile { outside: false } => plines_of(&base.parallel_offset(r, &opts)),
        Operation::Pocket { stepover } => {
            if !(stepover > 0.0 && stepover <= 1.0) {
                return Err(CamError::Invalid("the stepover must be above 0 and at most 1 (of the diameter)".into()));
            }
            let mut all: Vec<Vec<Polyline<f64>>> = Vec::new();
            let mut cur = base.parallel_offset(r, &opts);
            loop {
                let ring = plines_of(&cur);
                if ring.is_empty() {
                    break;
                }
                all.push(ring);
                if all.len() > MAX_RINGS {
                    return Err(CamError::TooLarge("more than 10000 pocket rings: use a larger stepover".into()));
                }
                cur = cur.parallel_offset(stepover * s.tool_diameter, &opts);
            }
            // From the middle out: the last rings are the walls, cut last.
            all.into_iter().rev().flatten().collect()
        }
    };
    if out.is_empty() {
        return Err(CamError::NothingToCut(format!(
            "a {} mm tool does not fit the region",
            (s.tool_diameter * 1000.0).round() / 1000.0
        )));
    }
    Ok(out)
}

/// Computes the toolpath of `op` on `regions`.
pub fn toolpath(regions: &[Region2], op: Operation, s: &Setup) -> Result<Toolpath, CamError> {
    check(s)?;
    let rings = rings(regions, op, s)?;
    let levels = levels(s);
    let mut moves = Vec::new();
    for z in &levels {
        for p in &rings {
            let Some(first) = p.get(0) else { continue };
            moves.push(Move::Rapid([first.x, first.y, s.safe]));
            moves.push(Move::Line { to: [first.x, first.y, *z], plunge: true });
            let n = p.vertex_count();
            for i in 0..n {
                let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
                moves.push(segment(a, b, *z));
            }
            moves.push(Move::Rapid([first.x, first.y, s.safe]));
            if moves.len() > MAX_MOVES {
                return Err(CamError::TooLarge("the toolpath has more than 5M moves".into()));
            }
        }
    }
    Ok(Toolpath { moves, rings: rings.len(), levels })
}

/// The move along the polyline segment from `a` to `b` (an arc when `a` has a bulge).
fn segment(a: PlineVertex<f64>, b: PlineVertex<f64>, z: f64) -> Move {
    if a.bulge.abs() < 1e-12 {
        return Move::Line { to: [b.x, b.y, z], plunge: false };
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let m = [(a.x + b.x) / 2.0, (a.y + b.y) / 2.0];
    // Centre: the chord's midpoint moved along its left normal by d (1 - k²) / 4k.
    let f = (1.0 - a.bulge * a.bulge) / (4.0 * a.bulge);
    Move::Arc { to: [b.x, b.y, z], center: [m[0] - dy * f, m[1] + dx * f], ccw: a.bulge > 0.0 }
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    libm::sqrt((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2))
}

impl Toolpath {
    /// Lengths and time, starting from the safe height above the first move.
    pub fn stats(&self, s: &Setup) -> Stats {
        let mut st = Stats::default();
        let mut at: Option<[f64; 3]> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid(to) => {
                    st.rapid += at.map_or(0.0, |a| dist(a, to));
                    at = Some(to);
                }
                Move::Line { to, plunge } => {
                    let l = at.map_or(0.0, |a| dist(a, to));
                    if plunge {
                        st.plunge += l
                    } else {
                        st.cut += l
                    }
                    at = Some(to);
                }
                Move::Arc { to, center, ccw } => {
                    if let Some(a) = at {
                        let r = libm::hypot(a[0] - center[0], a[1] - center[1]);
                        let a0 = libm::atan2(a[1] - center[1], a[0] - center[0]);
                        let a1 = libm::atan2(to[1] - center[1], to[0] - center[0]);
                        let mut sw = a1 - a0;
                        if ccw && sw <= 0.0 {
                            sw += std::f64::consts::TAU;
                        }
                        if !ccw && sw >= 0.0 {
                            sw -= std::f64::consts::TAU;
                        }
                        st.cut += r * sw.abs();
                    }
                    at = Some(to);
                }
            }
        }
        st.minutes = st.cut / s.feed + st.plunge / s.plunge + st.rapid / RAPID;
        st
    }

    /// The toolpath as G-code (metric, absolute, XY plane), with a short header.
    pub fn gcode(&self, s: &Setup, title: &str) -> String {
        let n = |v: f64| {
            let r = (v * 10_000.0).round() / 10_000.0;
            let t = format!("{r:.4}");
            let t = t.trim_end_matches('0').trim_end_matches('.').to_string();
            if t == "-0" { "0".to_string() } else { t }
        };
        let clean: String = title
            .chars()
            .filter(|c| c.is_ascii() && !c.is_ascii_control() && *c != '(' && *c != ')')
            .take(60)
            .collect();
        let mut g = String::new();
        let _ = writeln!(g, "({clean})");
        let _ = writeln!(g, "(Gusset 2.5D toolpath: flat end mill D{} mm)", n(s.tool_diameter));
        g.push_str("G21 G90 G17 G94\n");
        let _ = writeln!(g, "G0 Z{}", n(s.safe));
        if s.rpm > 0.0 {
            let _ = writeln!(g, "S{} M3", n(s.rpm));
        }
        let mut at: Option<[f64; 3]> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid(to) => {
                    // Up first, then across: never a diagonal rapid through the stock.
                    if at.is_some_and(|a| a[2] < to[2]) {
                        let _ = writeln!(g, "G0 Z{}", n(to[2]));
                        let _ = writeln!(g, "G0 X{} Y{}", n(to[0]), n(to[1]));
                    } else {
                        let _ = writeln!(g, "G0 X{} Y{}", n(to[0]), n(to[1]));
                        let _ = writeln!(g, "G0 Z{}", n(to[2]));
                    }
                    at = Some(to);
                }
                Move::Line { to, plunge } => {
                    let f = if plunge { s.plunge } else { s.feed };
                    if plunge {
                        let _ = writeln!(g, "G1 Z{} F{}", n(to[2]), n(f));
                    } else {
                        let _ = writeln!(g, "G1 X{} Y{} Z{} F{}", n(to[0]), n(to[1]), n(to[2]), n(f));
                    }
                    at = Some(to);
                }
                Move::Arc { to, center, ccw } => {
                    let a = at.unwrap_or(to);
                    let _ = writeln!(
                        g,
                        "{} X{} Y{} I{} J{} F{}",
                        if ccw { "G3" } else { "G2" },
                        n(to[0]),
                        n(to[1]),
                        n(center[0] - a[0]),
                        n(center[1] - a[1]),
                        n(s.feed)
                    );
                    at = Some(to);
                }
            }
        }
        let _ = writeln!(g, "G0 Z{}", n(s.safe));
        if s.rpm > 0.0 {
            g.push_str("M5\n");
        }
        g.push_str("M30\n");
        g
    }
}

#[cfg(test)]
mod tests;
