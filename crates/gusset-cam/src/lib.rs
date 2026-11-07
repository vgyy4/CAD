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
    /// Entry: spiral down along the ring at this angle (degrees) instead of plunging; 0
    /// plunges straight down.
    pub ramp_deg: f64,
    /// Holding tabs left on a profile's last passes.
    pub tabs: Option<Tabs>,
    /// Climb milling (the material on the right of the tool's travel, with the spindle
    /// turning clockwise, M3); false for conventional milling.
    pub climb: bool,
}

/// Bridges of uncut stock that hold a part cut out by a profile: `count` evenly spaced
/// along the ring, `width` long and `height` high above the final depth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tabs {
    pub count: usize,
    pub width: f64,
    pub height: f64,
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
    /// A drilling job: written as canned cycles (the moves expand them, for statistics).
    pub drill: Option<Drill>,
}

/// Holes drilled straight down at `points` (in cutting order), pecking by `peck` if set.
#[derive(Debug, Clone, PartialEq)]
pub struct Drill {
    pub points: Vec<[f64; 2]>,
    pub peck: Option<f64>,
}

/// Most holes one drilling job may have.
const MAX_HOLES: usize = 20_000;

/// Drills `points` to the setup's depth (the stepdown is not used; `peck` breaks chips).
/// The holes are visited nearest first from the origin.
pub fn drilling(points: &[[f64; 2]], peck: Option<f64>, s: &Setup) -> Result<Toolpath, CamError> {
    check(s)?;
    if s.tabs.is_some() {
        return Err(CamError::Invalid("tabs are for profiles only".into()));
    }
    if points.is_empty() {
        return Err(CamError::NothingToCut("no holes to drill (the sketch has no circles)".into()));
    }
    if points.len() > MAX_HOLES {
        return Err(CamError::TooLarge("more than 20000 holes".into()));
    }
    if peck.is_some_and(|q| !(q.is_finite() && q > 0.0 && s.depth / q <= 1000.0)) {
        return Err(CamError::Invalid("the peck depth must be positive (at most 1000 pecks)".into()));
    }
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(CamError::Invalid("a hole position is not a number".into()));
    }
    // Nearest neighbour order (quadratic, bounded by the hole count above).
    let mut left: Vec<[f64; 2]> = points.to_vec();
    let mut order = Vec::with_capacity(left.len());
    let mut at = [0.0, 0.0];
    while !left.is_empty() {
        let (i, _) = left.iter().enumerate().fold((0, f64::INFINITY), |best, (i, p)| {
            let d = libm::hypot(p[0] - at[0], p[1] - at[1]);
            if d < best.1 { (i, d) } else { best }
        });
        let p = left.swap_remove(i);
        at = p;
        order.push(p);
    }
    let bottom = s.top - s.depth;
    let retract = retract_plane(s);
    let mut moves = Vec::new();
    for p in &order {
        moves.push(Move::Rapid([p[0], p[1], s.safe]));
        match peck {
            Some(q) => {
                let mut z = s.top;
                while z > bottom + 1e-9 {
                    z = (z - q).max(bottom);
                    moves.push(Move::Line { to: [p[0], p[1], z], plunge: true });
                    moves.push(Move::Rapid([p[0], p[1], retract]));
                }
            }
            None => moves.push(Move::Line { to: [p[0], p[1], bottom], plunge: true }),
        }
        moves.push(Move::Rapid([p[0], p[1], s.safe]));
        if moves.len() > MAX_MOVES {
            return Err(CamError::TooLarge("the toolpath has more than 5M moves".into()));
        }
    }
    Ok(Toolpath { moves, rings: 0, levels: vec![bottom], drill: Some(Drill { points: order, peck }) })
}

/// The canned cycles' retract plane R: a little above the stock top (at most 1 mm, at most
/// half way to the safe height), so the tool clears the top between pecks.
fn retract_plane(s: &Setup) -> f64 {
    s.top + 1.0_f64.min((s.safe - s.top) / 2.0)
}

/// Most helix turns one bore may have.
const MAX_TURNS: usize = 1000;

/// Bores holes `(centre, diameter)` by helical interpolation: the tool circles inside each hole
/// on a radius of half the difference of hole and tool, sinking by at most the stepdown (or the
/// ramp angle's pitch, if smaller) per turn, then cuts one flat circle at the bottom. Holes
/// narrower than the tool are refused; holes up to the tool's size are plunged; holes wider
/// than twice the tool need a pocket. The holes are visited nearest first from the origin.
pub fn helical_bore(holes: &[([f64; 2], f64)], s: &Setup) -> Result<Toolpath, CamError> {
    check(s)?;
    if s.tabs.is_some() {
        return Err(CamError::Invalid("tabs are for profiles only".into()));
    }
    if holes.is_empty() {
        return Err(CamError::NothingToCut("no holes to bore (the sketch has no circles)".into()));
    }
    if holes.len() > MAX_HOLES {
        return Err(CamError::TooLarge("more than 20000 holes".into()));
    }
    let tool = s.tool_diameter;
    for (c, d) in holes {
        if !(c[0].is_finite() && c[1].is_finite() && d.is_finite()) {
            return Err(CamError::Invalid("a hole position or diameter is not a number".into()));
        }
        if *d < tool {
            return Err(CamError::NothingToCut(format!(
                "the hole at ({}, {}) of diameter {d} mm is narrower than the {tool} mm tool",
                c[0], c[1]
            )));
        }
        if *d > 2.0 * tool {
            return Err(CamError::Invalid("use a pocket for holes wider than twice the tool".into()));
        }
    }
    let mut left: Vec<([f64; 2], f64)> = holes.to_vec();
    let mut order = Vec::with_capacity(left.len());
    let mut at = [0.0, 0.0];
    while !left.is_empty() {
        let (i, _) = left.iter().enumerate().fold((0, f64::INFINITY), |best, (i, h)| {
            let d = libm::hypot(h.0[0] - at[0], h.0[1] - at[1]);
            if d < best.1 { (i, d) } else { best }
        });
        let h = left.swap_remove(i);
        at = h.0;
        order.push(h);
    }
    let bottom = s.top - s.depth;
    let mut moves = Vec::new();
    for (c, d) in &order {
        let r = (d - tool) / 2.0;
        moves.push(Move::Rapid([c[0] + r, c[1], s.safe]));
        if *d <= tool * 1.0001 {
            moves.push(Move::Line { to: [c[0] + r, c[1], bottom], plunge: true });
            moves.push(Move::Rapid([c[0] + r, c[1], s.safe]));
            continue;
        }
        moves.push(Move::Line { to: [c[0] + r, c[1], s.top], plunge: true });
        let mut pitch = s.stepdown;
        if s.ramp_deg > 0.0 {
            pitch = pitch.min(std::f64::consts::TAU * r * libm::tan(s.ramp_deg.to_radians()));
        }
        let turns = (s.depth / pitch).ceil();
        if !(turns.is_finite() && turns <= MAX_TURNS as f64) {
            return Err(CamError::TooLarge("more than 1000 helix turns: use a larger stepdown or ramp angle".into()));
        }
        let turns = (turns as usize).max(1);
        let half_drop = s.depth / (2 * turns) as f64;
        let ccw = s.climb;
        let mut x = r; // current offset from the centre along x: +r or -r
        let mut half = |z: f64, x: &mut f64| {
            *x = -*x;
            moves.push(Move::Arc { to: [c[0] + *x, c[1], z], center: *c, ccw });
        };
        for k in 1..=(2 * turns) {
            let z = if k == 2 * turns { bottom } else { s.top - half_drop * k as f64 };
            half(z, &mut x);
        }
        half(bottom, &mut x);
        half(bottom, &mut x);
        moves.push(Move::Rapid([c[0] + x, c[1], s.safe]));
        if moves.len() > MAX_MOVES {
            return Err(CamError::TooLarge("the toolpath has more than 5M moves".into()));
        }
    }
    Ok(Toolpath { moves, rings: holes.len(), levels: vec![bottom], drill: None })
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
/// Largest length (mm), feed (mm/min) and spindle speed (rev/min) accepted.
const MAX_SIZE: f64 = 100_000.0;
const MAX_FEED: f64 = 1_000_000.0;
const MAX_RPM: f64 = 1_000_000.0;
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
                let n = bezier_steps(&c);
                for i in 0..n {
                    let u = f64::from(i) / f64::from(n);
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

/// Chords for a cubic Bézier within `CHORD_TOL` of the curve: a chord of a curve whose
/// second derivative is at most `M` (here 6 × the control polygon's largest second
/// difference) strays at most `M h² / 8` over a parameter step `h`.
fn bezier_steps(c: &[[f64; 2]; 4]) -> u32 {
    let dd = |a: [f64; 2], b: [f64; 2], d: [f64; 2]| libm::hypot(a[0] - 2.0 * b[0] + d[0], a[1] - 2.0 * b[1] + d[1]);
    let m = 6.0 * dd(c[0], c[1], c[2]).max(dd(c[1], c[2], c[3]));
    let n = libm::ceil(libm::sqrt(m / (8.0 * CHORD_TOL)));
    if n.is_finite() { n.clamp(4.0, 4096.0) as u32 } else { 4 }
}

/// Largest distance (mm) a sampled curve may stray from its chords.
const CHORD_TOL: f64 = 0.002;

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
    if let Some(t) = s.tabs
        && !((1..=100).contains(&t.count)
            && t.width.is_finite()
            && t.width > 0.0
            && t.height.is_finite()
            && t.height > 0.0
            && t.height < s.depth)
    {
        return bad("tabs need a count of 1 to 100, a positive width and a height below the depth");
    }
    if !(s.ramp_deg.is_finite() && (0.0..=45.0).contains(&s.ramp_deg)) {
        return bad("the ramp angle must be 0 (plunge) to 45 degrees");
    }
    if !pos(s.feed) || !pos(s.plunge) || !pos(s.rpm) {
        return bad("feeds and the spindle speed must be positive");
    }
    // Sizes a machine could hold: G-code numbers stay short and finite.
    let big = [s.tool_diameter, s.depth, s.top.abs(), s.safe.abs()].into_iter().any(|v| v > MAX_SIZE)
        || s.feed > MAX_FEED
        || s.plunge > MAX_FEED
        || s.rpm > MAX_RPM;
    if big {
        return bad("a size above 100 m, a feed above 1000000 mm/min or a speed above 1000000 rev/min");
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
            // Rings at most a radius apart leave nothing standing between them: wider steps
            // leave ridges in narrow arms and cusps in corners (review 2026-11-04).
            if !(stepover > 0.0 && stepover <= 0.5) {
                return Err(CamError::Invalid(
                    "the stepover must be above 0 and at most 0.5 (of the diameter): wider steps leave ridges".into(),
                ));
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
    // Each ring's direction: climb milling keeps the material on the right of the travel
    // (clockwise round material inside the ring, counter-clockwise round material outside).
    let mut out = out;
    for p in &mut out {
        let ccw = p.area() > 0.0;
        let material_inside = matches!(op, Operation::Profile { outside: true }) || !ccw;
        let want_ccw = material_inside != s.climb;
        if ccw != want_ccw {
            p.invert_direction_mut();
        }
    }
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
    if s.tabs.is_some() && !matches!(op, Operation::Profile { .. }) {
        return Err(CamError::Invalid("tabs are for profiles only".into()));
    }
    let rings = rings(regions, op, s)?;
    let levels = levels(s);
    let mut moves = Vec::new();
    let slope = libm::tan(s.ramp_deg.to_radians());
    let tab_top = s.tabs.map(|t| s.top - s.depth + t.height);
    for (li, z) in levels.iter().enumerate() {
        // Each ring is entered from the level above (the stock top for the first).
        let above = if li == 0 { s.top } else { levels.get(li - 1).copied().unwrap_or(s.top) };
        for p in &rings {
            let Some(first) = p.get(0) else { continue };
            let n = p.vertex_count();
            moves.push(Move::Rapid([first.x, first.y, s.safe]));
            let perimeter = p.path_length();
            // A helix never runs below the tabs' top (it would cut through them): on the
            // tabbed levels it stops there and the rest of the descent is a ramp along the
            // ring's first stretch, which is never on a tab (see `tabbed_ring`).
            let tabbed = matches!(op, Operation::Profile { .. }) && tab_top.is_some_and(|t| *z < t - 1e-9);
            let floor = match tab_top {
                Some(t) if tabbed => t,
                _ => *z,
            };
            let mut entry = *z;
            if slope > 0.0 && perimeter > 1e-9 && floor < above - 1e-9 {
                // A helix along the ring, as many turns as the angle needs, then one
                // full ring at the depth.
                moves.push(Move::Line { to: [first.x, first.y, above], plunge: true });
                let turns = ((above - floor) / (perimeter * slope)).ceil().max(1.0);
                if turns * n as f64 + moves.len() as f64 > MAX_MOVES as f64 {
                    return Err(CamError::TooLarge(
                        "the ramp needs more than 5M moves: use a steeper ramp or plunge".into(),
                    ));
                }
                let turns = turns as usize;
                let total = perimeter * turns as f64;
                let mut run = 0.0;
                for _ in 0..turns {
                    for i in 0..n {
                        let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
                        run += seg_len(a, b);
                        moves.push(segment(a, b, above - (above - floor) * (run / total).min(1.0)));
                    }
                }
                entry = floor;
            } else if tabbed && slope > 0.0 && perimeter > 1e-9 {
                // Already below the tabs' top: the ring is open at the level above.
                moves.push(Move::Line { to: [first.x, first.y, above], plunge: true });
                entry = above;
            } else {
                moves.push(Move::Line { to: [first.x, first.y, *z], plunge: true });
            }
            match (s.tabs, tab_top) {
                (Some(t), Some(top)) if tabbed => {
                    let tool = s.tool_diameter;
                    tabbed_ring(p, Heights { entry, z: *z, tab_top: top, slope }, &t, tool, &mut moves)?;
                }
                _ => {
                    for i in 0..n {
                        let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
                        moves.push(segment(a, b, *z));
                    }
                }
            }
            moves.push(Move::Rapid([first.x, first.y, s.safe]));
            if moves.len() > MAX_MOVES {
                return Err(CamError::TooLarge("the toolpath has more than 5M moves".into()));
            }
        }
    }
    Ok(Toolpath { moves, rings: rings.len(), levels, drill: None })
}

/// The part of the segment from `a` to `b` between length fractions `t0` and `t1`: its start
/// and the move to its end at height `z`.
fn piece(a: PlineVertex<f64>, b: PlineVertex<f64>, t0: f64, t1: f64, z: f64) -> ([f64; 2], Move) {
    if a.bulge.abs() < 1e-12 {
        let at = |t: f64| [a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t];
        let e = at(t1);
        return (at(t0), Move::Line { to: [e[0], e[1], z], plunge: false });
    }
    let Move::Arc { center, ccw, .. } = segment(a, b, z) else {
        return ([a.x, a.y], Move::Line { to: [b.x, b.y, z], plunge: false });
    };
    let r = libm::hypot(a.x - center[0], a.y - center[1]);
    let a0 = libm::atan2(a.y - center[1], a.x - center[0]);
    let sweep = 4.0 * libm::atan(a.bulge);
    let at = |t: f64| [center[0] + r * libm::cos(a0 + sweep * t), center[1] + r * libm::sin(a0 + sweep * t)];
    let e = at(t1);
    (at(t0), Move::Arc { to: [e[0], e[1], z], center, ccw })
}

/// The heights a tabbed ring is cut at.
#[derive(Debug, Clone, Copy)]
struct Heights {
    /// Height at the ring's start when `tabbed_ring` begins.
    entry: f64,
    /// The depth of the level.
    z: f64,
    /// Top of the tabs.
    tab_top: f64,
    /// Tangent of the ramp angle (0: plunge).
    slope: f64,
}

/// The tabs' spans along a ring of length `total`: evenly spaced, `width` plus the tool's
/// diameter long (the tool radius added on both sides leaves the stock `width` wide). They never
/// touch the ring's start or end.
fn tab_spans(total: f64, t: &Tabs, tool: f64) -> Vec<(f64, f64)> {
    let span = t.width + tool;
    (0..t.count)
        .map(|k| {
            let c = total * (k as f64 + 0.5) / t.count as f64;
            (c - span / 2.0, c + span / 2.0)
        })
        .collect()
}

/// One segment of a closed polyline: its ends and where it starts and how long it is along the ring.
type RingSeg = (PlineVertex<f64>, PlineVertex<f64>, f64, f64);

/// A closed polyline's segments with their start lengths along it.
struct Walk {
    segs: Vec<RingSeg>,
    total: f64,
}

impl Walk {
    fn new(p: &Polyline<f64>) -> Walk {
        let n = p.vertex_count();
        let mut segs = Vec::new();
        let mut run = 0.0;
        for i in 0..n {
            let (Some(a), Some(b)) = (p.get(i), p.get((i + 1) % n)) else { continue };
            let len = seg_len(a, b);
            if len > 0.0 {
                segs.push((a, b, run, len));
                run += len;
            }
        }
        Walk { segs, total: run }
    }

    /// The point `s` along the ring.
    fn xy(&self, s: f64) -> [f64; 2] {
        let hit = self.segs.iter().find(|(_, _, run, len)| s <= run + len).or(self.segs.last());
        hit.map_or([0.0, 0.0], |(a, b, run, len)| {
            let f = ((s - run) / len).clamp(0.0, 1.0);
            piece(*a, *b, f, f, 0.0).0
        })
    }

    /// Cuts from length `from` to `to` along the ring (backwards if `to < from`), the height
    /// changing evenly from `z_from` to `z_to`.
    fn cut(&self, from: f64, to: f64, z_from: f64, z_to: f64, moves: &mut Vec<Move>) {
        let span = (to - from).abs();
        if span < 1e-9 {
            return;
        }
        let z_at = |s: f64| z_from + (z_to - z_from) * ((s - from).abs() / span);
        if to > from {
            for (a, b, run, len) in &self.segs {
                let (lo, hi) = (from.max(*run), to.min(run + len));
                if hi - lo > 1e-9 {
                    moves.push(piece(*a, *b, (lo - run) / len, (hi - run) / len, z_at(hi)).1);
                }
            }
        } else {
            for (a, b, run, len) in self.segs.iter().rev() {
                let (lo, hi) = (to.max(*run), from.min(run + len));
                if hi - lo > 1e-9 {
                    // The segment walked backwards: from `b` to `a`, the bulge reversed.
                    let back = PlineVertex::new(b.x, b.y, -a.bulge);
                    moves.push(piece(back, *a, 1.0 - (hi - run) / len, 1.0 - (lo - run) / len, z_at(lo)).1);
                }
            }
        }
    }
}

/// One ring at depth `h.z` that rises to the tabs' top over each tab (tool radius added on
/// both sides, so the stock left is the tab's width). The tool is at `h.entry` at the ring's
/// start and ends at the start at depth `h.z`.
///
/// Going down (from the entry height, and after every tab) is a ramp at the ramp angle along
/// the stretch before the next tab (steeper if that is shorter), never over a tab; that stretch
/// is then cut again at depth, back and forth, so the whole ring outside the tabs is cut at
/// depth. Without a ramp the tool plunges.
fn tabbed_ring(p: &Polyline<f64>, h: Heights, t: &Tabs, tool: f64, moves: &mut Vec<Move>) -> Result<(), CamError> {
    let w = Walk::new(p);
    let total = w.total;
    if t.count == 0 || total <= 0.0 {
        return Ok(());
    }
    if (t.width + tool) * t.count as f64 >= total * 0.9 {
        return Err(CamError::Invalid("the tabs do not fit on the profile: fewer or narrower tabs".into()));
    }
    let tabs = tab_spans(total, t, tool);
    let at = |s: f64, z: f64| {
        let xy = w.xy(s);
        [xy[0], xy[1], z]
    };
    // Down from `from_z` at length `s0` to the depth; returns the length the tool is at, at depth.
    let descend = |s0: f64, from_z: f64, limit: f64, moves: &mut Vec<Move>| -> f64 {
        if from_z <= h.z + 1e-9 {
            return s0;
        }
        if h.slope <= 0.0 {
            moves.push(Move::Line { to: at(s0, h.z), plunge: true });
            return s0;
        }
        let end = (s0 + (from_z - h.z) / h.slope).min(limit);
        w.cut(s0, end, from_z, h.z, moves);
        w.cut(end, s0, h.z, h.z, moves);
        w.cut(s0, end, h.z, h.z, moves);
        end
    };
    let first_tab = tabs.first().map_or(total, |t| t.0);
    let mut pos = descend(0.0, h.entry, first_tab, moves);
    for (k, (a, b)) in tabs.iter().enumerate() {
        w.cut(pos, *a, h.z, h.z, moves);
        // Up onto the tab, along it, then down after it.
        moves.push(Move::Line { to: at(*a, h.tab_top), plunge: false });
        w.cut(*a, *b, h.tab_top, h.tab_top, moves);
        let next = tabs.get(k + 1).map_or(total, |t| t.0);
        pos = descend(*b, h.tab_top, next, moves);
    }
    w.cut(pos, total, h.z, h.z, moves);
    Ok(())
}

/// The length of the polyline segment from `a` to `b`.
fn seg_len(a: PlineVertex<f64>, b: PlineVertex<f64>) -> f64 {
    let d = libm::hypot(b.x - a.x, b.y - a.y);
    if a.bulge.abs() < 1e-12 {
        return d;
    }
    // Arc: sweep 4 atan(k), radius d (1 + k²) / 4|k|.
    let k = a.bulge.abs();
    let sweep = 4.0 * libm::atan(k);
    sweep * d * (1.0 + k * k) / (4.0 * k)
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
        let _ = writeln!(g, "(T1 D{} flat end mill)", n(s.tool_diameter));
        g.push_str("G21 G90 G17 G94\nG40 G49 G80 G54\n");
        put(&mut g, &format!("G0 Z{}", n(s.safe)));
        put(&mut g, &format!("S{} M3", n(s.rpm)));
        if let Some(d) = &self.drill {
            // Canned cycles: G81 drill, or G83 peck drill; R the retract plane, just above the top.
            let bottom = s.top - s.depth;
            let r = retract_plane(s);
            for (i, p) in d.points.iter().enumerate() {
                let xy = format!("X{} Y{}", n(p[0]), n(p[1]));
                if i > 0 {
                    put(&mut g, &xy);
                    continue;
                }
                put(&mut g, &format!("G0 {xy}"));
                match d.peck {
                    Some(q) => {
                        put(&mut g, &format!("G98 G83 {xy} Z{} R{} Q{} F{}", n(bottom), n(r), n(q), n(s.plunge)));
                    }
                    None => {
                        put(&mut g, &format!("G98 G81 {xy} Z{} R{} F{}", n(bottom), n(r), n(s.plunge)));
                    }
                }
            }
            g.push_str("G80\n");
            put(&mut g, &format!("G0 Z{}", n(s.safe)));
            g.push_str("M5\nM30\n");
            return g;
        }
        // Where the tool is, as far as the G-code says (the header lifted it to the safe height).
        let mut at: Option<[f64; 3]> = None;
        let mut z_now = Some(s.safe);
        let mut xy_now: Option<[f64; 2]> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid(to) => {
                    // Up first, then across: never a diagonal rapid through the stock. A
                    // coordinate the tool is already at is not written again.
                    let up = z_now.is_some_and(|z| z < to[2]);
                    let across = |g: &mut String, xy_now: &mut Option<[f64; 2]>| {
                        if !xy_now.is_some_and(|p| n(p[0]) == n(to[0]) && n(p[1]) == n(to[1])) {
                            put(g, &format!("G0 X{} Y{}", n(to[0]), n(to[1])));
                        }
                        *xy_now = Some([to[0], to[1]]);
                    };
                    if up {
                        put(&mut g, &format!("G0 Z{}", n(to[2])));
                        across(&mut g, &mut xy_now);
                    } else {
                        across(&mut g, &mut xy_now);
                        if z_now != Some(to[2]) {
                            put(&mut g, &format!("G0 Z{}", n(to[2])));
                        }
                    }
                    z_now = Some(to[2]);
                    at = Some(to);
                }
                Move::Line { to, plunge } => {
                    let f = if plunge { s.plunge } else { s.feed };
                    if plunge {
                        put(&mut g, &format!("G1 Z{} F{}", n(to[2]), n(f)));
                    } else {
                        put(&mut g, &format!("G1 X{} Y{} Z{} F{}", n(to[0]), n(to[1]), n(to[2]), n(f)));
                    }
                    at = Some(to);
                    z_now = Some(to[2]);
                    xy_now = Some([to[0], to[1]]);
                }
                Move::Arc { to, center, ccw } => {
                    let a = at.unwrap_or(to);
                    z_now = Some(to[2]);
                    xy_now = Some([to[0], to[1]]);
                    // An arc too short to state after rounding (the same end as its start, or
                    // a centre on it) is written as the straight cut it then is.
                    let same = n(to[0]) == n(a[0]) && n(to[1]) == n(a[1]);
                    if same || libm::hypot(center[0] - a[0], center[1] - a[1]) < 1e-3 {
                        put(&mut g, &format!("G1 X{} Y{} Z{} F{}", n(to[0]), n(to[1]), n(to[2]), n(s.feed)));
                        at = Some(to);
                        continue;
                    }
                    put(
                        &mut g,
                        &format!(
                            "{} X{} Y{} Z{} I{} J{} F{}",
                            if ccw { "G3" } else { "G2" },
                            n(to[0]),
                            n(to[1]),
                            n(to[2]),
                            n(center[0] - a[0]),
                            n(center[1] - a[1]),
                            n(s.feed)
                        ),
                    );
                    at = Some(to);
                }
            }
        }
        put(&mut g, &format!("G0 Z{}", n(s.safe)));
        g.push_str("M5\nM30\n");
        g
    }
}

/// Appends a line unless it repeats the one before.
fn put(g: &mut String, line: &str) {
    if g.strip_suffix('\n').and_then(|h| h.rsplit('\n').next()) == Some(line) {
        return;
    }
    g.push_str(line);
    g.push('\n');
}

#[cfg(test)]
mod tests;
