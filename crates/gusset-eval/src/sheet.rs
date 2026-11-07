//! Sheet metal (ADR 0037): contour flanges (sketch lines bent at every corner) and tabs
//! (flat plates from sketch regions), each with its flat pattern.
//!
//! A contour flange is one extrusion of its cross-section: the line chain on one side
//! (the sketch lines are the faces `flat(<line>)`), the same chain one thickness away on
//! the material side (`flat_back(<line>)`), concentric arcs at the corners whose inside
//! radius is `bend_radius` (`bend_in`/`bend_out(<line before the bend>)`), closed by the
//! two end faces (`start(<first line>)`, `end(<last line>)`). The flat pattern follows from
//! the same numbers: the straight lengths plus a bend allowance `|angle| (r + k t)` per
//! bend.

use super::*;
use gusset_doc::{PathRef, SheetDef, SheetOp, SheetPart};
use gusset_kernel::{ExtrudeSpec, Loop2, P2, Profile, ProfileEdge, Region2, Seg2};
use gusset_naming::sheet_tag;
use gusset_units::expr::Expr;

/// The developed (unbent) shape of a sheet-metal body, in its own plane (mm).
#[derive(Debug, Clone, PartialEq)]
pub struct FlatPattern {
    pub thickness: f64,
    /// Closed outlines (the first is the outer boundary; the rest are cut-outs), as
    /// polylines without the closing point repeated.
    pub outlines: Vec<Vec<[f64; 2]>>,
    /// Cut-outs that are full circles (their polyline is in `outlines`).
    pub circles: Vec<FlatCircle>,
    /// The bends: a contour flange's in order along its developed length (`x`), then
    /// those of edge flanges in feature order.
    pub bends: Vec<FlatBend>,
    /// For a tab: where its outline lies, so edge flanges can be bent off its lines.
    pub base: Option<TabBase>,
    /// For a contour flange: its straights, so edge flanges can be bent off their sides.
    pub contour: Option<ContourBase>,
    /// The far ends of edge flanges, so further flanges can be bent off them.
    pub flange_ends: Vec<FlangeEnd>,
    /// Features other than sheet-metal ones that changed the body since (cuts, holes,
    /// moves...): the pattern does not show what they did.
    pub changed_by: Vec<String>,
}

/// A circular cut-out of the flat pattern: `outlines[outline]` is its polyline.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatCircle {
    pub outline: usize,
    pub centre: [f64; 2],
    pub radius: f64,
}

/// The sketch a tab was made from and the side its material is on.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBase {
    pub sketch: String,
    pub flip: bool,
}

/// An edge flange's far edge: the line from `a` to `b` in the flat pattern (outward `out`),
/// and in space the frame of a flange bent off it and how far that one runs.
#[derive(Debug, Clone, PartialEq)]
pub struct FlangeEnd {
    /// The edge flange feature.
    pub flange: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub out: [f64; 2],
    pub frame: Frame,
    pub along: f64,
    /// The flange's own bend, for cuts across it.
    pub bend: FlangeBend,
}

/// An edge flange's bend: its profile's frame and run along the line, where the bend starts
/// in the flat pattern (the line from `a` to `b`, outward `out`), and in the section its
/// centre, the direction to its start on the outer face (`bend_out`), that face's radius,
/// its signed turn and its allowance.
#[derive(Debug, Clone, PartialEq)]
pub struct FlangeBend {
    pub frame: Frame,
    pub along: f64,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub out: [f64; 2],
    pub centre: [f64; 2],
    pub start: [f64; 2],
    pub radius: f64,
    pub turn: f64,
    pub allowance: f64,
    /// The flat length after the bend.
    pub length: f64,
    /// Bent off a tab's outline line (corners can be closed between such flanges).
    pub on_tab: bool,
}

/// Where a contour flange's straights lie: in its sketch, across `z` along the sketch
/// normal (the flat pattern's `y = z - z[0]`).
#[derive(Debug, Clone, PartialEq)]
pub struct ContourBase {
    pub sketch: String,
    pub straights: Vec<Straight>,
    pub z: [f64; 2],
    /// The bends between the straights, in order.
    pub bends: Vec<BendZone>,
}

/// A contour flange's bend: in the sketch, its centre, the direction from the centre to
/// where the bend starts on the sketch side, and its signed turn; in the flat pattern,
/// where its zone starts and how long it is. `line` is the straight before it, `outer` says
/// whether its sketch-side face is the bend's outside (`bend_out`).
#[derive(Debug, Clone, PartialEq)]
pub struct BendZone {
    pub line: String,
    pub centre: [f64; 2],
    pub start: [f64; 2],
    /// The sketch-side face's radius.
    pub radius: f64,
    pub turn: f64,
    pub x0: f64,
    pub allowance: f64,
    pub outer: bool,
}

/// One flat stretch of a contour flange: the sketch-side line from `a` to `b` (trimmed by
/// the bends), the direction into the material, and where it starts in the flat pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct Straight {
    pub line: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub inward: [f64; 2],
    pub x0: f64,
}

/// One bend of a flat pattern: the zone between the lines `start` and `end` straightens
/// the bend after `line`.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatBend {
    /// The sketch line the bend follows.
    pub line: String,
    /// Bend angle (rad), positive when the flange turns towards the material side.
    pub angle: f64,
    pub inner_radius: f64,
    pub k_factor: f64,
    /// Bend allowance: the developed length of the bend zone.
    pub allowance: f64,
    /// The bend zone's first and last line across the pattern.
    pub start: [[f64; 2]; 2],
    pub end: [[f64; 2]; 2],
}

impl FlatPattern {
    /// Width and height of the pattern's bounding box.
    pub fn size(&self) -> [f64; 2] {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in self.outlines.iter().flatten() {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        if lo[0] > hi[0] { [0.0, 0.0] } else { [hi[0] - lo[0], hi[1] - lo[1]] }
    }
}

/// Bend angles closer to 0 than this are collinear lines, closer to a half turn are folds.
const MIN_BEND: f64 = 1e-6;

fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn addv(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: P2, s: f64) -> P2 {
    [a[0] * s, a[1] * s]
}
fn len(a: P2) -> f64 {
    libm::hypot(a[0], a[1])
}

/// The cross-section of a contour flange and its flat pattern.
pub(crate) struct Section {
    pub(crate) edges: Vec<ProfileEdge>,
    pub(crate) flat: FlatPattern,
}

/// Builds the cross-section of lines `pts` (chain order, `ids` their sketch ids).
/// `side` is +1 for material left of the travel direction, -1 for right.
pub(crate) fn contour_section(
    pts: &[P2],
    ids: &[String],
    t: f64,
    r: f64,
    k: f64,
    side: f64,
    width: f64,
) -> Result<Section, String> {
    let n = ids.len();
    if n == 0 || pts.len() != n + 1 {
        return Err("a contour flange needs at least one line".into());
    }
    let name = |i: usize| ids.get(i).cloned().unwrap_or_default();
    let mut dirs = Vec::with_capacity(n);
    let mut lens = Vec::with_capacity(n);
    for (i, w) in pts.windows(2).enumerate() {
        let [a, b] = w else { continue };
        let v = sub(*b, *a);
        let l = len(v);
        if l < 1e-9 {
            return Err(format!("line {} has no length", name(i)));
        }
        dirs.push(mul(v, 1.0 / l));
        lens.push(l);
    }
    let normal = |d: P2| [-d[1], d[0]];
    // Per corner k (between line k-1 and k): signed turn, radius on the sketch side.
    let mut turns = vec![0.0; n];
    let mut ra = vec![0.0; n];
    let mut setback = vec![0.0; n + 1];
    for c in 1..n {
        let (Some(d0), Some(d1)) = (dirs.get(c - 1), dirs.get(c)) else { continue };
        let cr = d0[0] * d1[1] - d0[1] * d1[0];
        let dt = d0[0] * d1[0] + d0[1] * d1[1];
        let th = libm::atan2(cr, dt);
        if th.abs() < MIN_BEND {
            return Err(format!("lines {} and {} are in line; join them into one line", name(c - 1), name(c)));
        }
        if std::f64::consts::PI - th.abs() < 1e-3 {
            return Err(format!("the bend between {} and {} folds the sheet back onto itself", name(c - 1), name(c)));
        }
        // The material is inside the turn when it lies on the side the line turns to.
        let inside = (th > 0.0) == (side > 0.0);
        let rad = if inside { r + t } else { r };
        if let (Some(tc), Some(rc)) = (turns.get_mut(c), ra.get_mut(c)) {
            *tc = th;
            *rc = rad;
        }
        if let Some(sb) = setback.get_mut(c) {
            *sb = rad * libm::tan(th.abs() / 2.0);
        }
    }
    let mut straight = Vec::with_capacity(n);
    for i in 0..n {
        let s0 = setback.get(i).copied().unwrap_or(0.0);
        let s1 = setback.get(i + 1).copied().unwrap_or(0.0);
        let l = lens.get(i).copied().unwrap_or(0.0) - s0 - s1;
        if l < 1e-6 {
            return Err(format!(
                "line {} ({:.3} mm) is too short for its bends, which take {:.3} mm",
                name(i),
                lens.get(i).copied().unwrap_or(0.0),
                s0 + s1
            ));
        }
        straight.push(l);
    }
    // Side A (the sketch lines) forward, side B (offset to the material) backward.
    let off = |i: usize| mul(normal(dirs.get(i).copied().unwrap_or([1.0, 0.0])), side * t);
    let a0 = |i: usize| {
        addv(
            pts.get(i).copied().unwrap_or_default(),
            mul(dirs.get(i).copied().unwrap_or_default(), setback.get(i).copied().unwrap_or(0.0)),
        )
    };
    let a1 = |i: usize| {
        sub(
            pts.get(i + 1).copied().unwrap_or_default(),
            mul(dirs.get(i).copied().unwrap_or_default(), setback.get(i + 1).copied().unwrap_or(0.0)),
        )
    };
    let tag = |p: SheetPart, i: usize| sheet_tag(p, i);
    let mut edges: Vec<ProfileEdge> = Vec::new();
    // Bend corner `c`: centre on the inside of the turn of side A.
    let centre = |c: usize| {
        let th = turns.get(c).copied().unwrap_or(0.0);
        let d0 = dirs.get(c - 1).copied().unwrap_or_default();
        addv(a1(c - 1), mul(normal(d0), th.signum() * ra.get(c).copied().unwrap_or(0.0)))
    };
    let parts = |c: usize| {
        let th = turns.get(c).copied().unwrap_or(0.0);
        let inside = (th > 0.0) == (side > 0.0);
        // Side A is the inside of the bend when the material is outside the turn.
        if inside { (SheetPart::BendOut, SheetPart::BendIn) } else { (SheetPart::BendIn, SheetPart::BendOut) }
    };
    for i in 0..n {
        if i > 0 && ra.get(i).is_some_and(|r| *r > 1e-9) {
            let th = turns.get(i).copied().unwrap_or(0.0);
            let (pa, _) = parts(i);
            edges.push(ProfileEdge {
                seg: Seg2::Arc { center: centre(i), start: a1(i - 1), end: a0(i), ccw: th > 0.0 },
                tag: tag(pa, i - 1),
            });
        }
        edges.push(ProfileEdge { seg: Seg2::Line { a: a0(i), b: a1(i) }, tag: tag(SheetPart::Flat, i) });
    }
    let last = n - 1;
    edges.push(ProfileEdge {
        seg: Seg2::Line { a: a1(last), b: addv(a1(last), off(last)) },
        tag: tag(SheetPart::End, last),
    });
    for i in (0..n).rev() {
        let o = off(i);
        edges.push(ProfileEdge {
            seg: Seg2::Line { a: addv(a1(i), o), b: addv(a0(i), o) },
            tag: tag(SheetPart::FlatBack, i),
        });
        // A sharp inside corner (bend radius 0) has no arc.
        let rb = ra
            .get(i)
            .map_or(0.0, |r| r - turns.get(i).map_or(0.0, |th| if (*th > 0.0) == (side > 0.0) { t } else { -t }));
        if i > 0 && rb > 1e-9 {
            let th = turns.get(i).copied().unwrap_or(0.0);
            let (_, pb) = parts(i);
            edges.push(ProfileEdge {
                seg: Seg2::Arc {
                    center: centre(i),
                    start: addv(a0(i), o),
                    end: addv(a1(i - 1), off(i - 1)),
                    ccw: th <= 0.0,
                },
                tag: tag(pb, i - 1),
            });
        }
    }
    edges.push(ProfileEdge { seg: Seg2::Line { a: addv(a0(0), off(0)), b: a0(0) }, tag: tag(SheetPart::Start, 0) });
    // Developed length: straights plus bend allowances, the bends as zones across it.
    let w = width.abs();
    let mut x = 0.0;
    let mut bends = Vec::new();
    let mut straights = Vec::with_capacity(n);
    let mut zones = Vec::with_capacity(n);
    for i in 0..n {
        straights.push(Straight {
            line: name(i),
            a: a0(i),
            b: a1(i),
            inward: mul(normal(dirs.get(i).copied().unwrap_or([1.0, 0.0])), side),
            x0: x,
        });
        x += straight.get(i).copied().unwrap_or(0.0);
        if i + 1 < n {
            let th = turns.get(i + 1).copied().unwrap_or(0.0);
            let ba = th.abs() * (r + k * t);
            let cc = centre(i + 1);
            let sd = sub(a1(i), cc);
            zones.push(BendZone {
                line: name(i),
                centre: cc,
                start: mul(sd, 1.0 / len(sd).max(1e-300)),
                radius: len(sd),
                turn: th,
                x0: x,
                allowance: ba,
                outer: parts(i + 1).0 == SheetPart::BendOut,
            });
            bends.push(FlatBend {
                line: name(i),
                angle: th * side,
                inner_radius: r,
                k_factor: k,
                allowance: ba,
                start: [[x, 0.0], [x, w]],
                end: [[x + ba, 0.0], [x + ba, w]],
            });
            x += ba;
        }
    }
    let flat = FlatPattern {
        thickness: t,
        outlines: vec![vec![[0.0, 0.0], [x, 0.0], [x, w], [0.0, w]]],
        circles: Vec::new(),
        bends,
        base: None,
        // The sketch and the width's span are filled in by the caller.
        contour: Some(ContourBase { sketch: String::new(), straights, z: [0.0, w], bends: zones }),
        flange_ends: Vec::new(),
        changed_by: Vec::new(),
    };
    Ok(Section { edges, flat })
}

/// Signed area of a closed loop of lines and arcs (arcs by their chords plus segments).
fn loop_area(edges: &[ProfileEdge]) -> f64 {
    let mut a = 0.0;
    for e in edges {
        match e.seg {
            Seg2::Line { a: p, b: q } => a += p[0] * q[1] - q[0] * p[1],
            Seg2::Arc { center, start, end, ccw } => {
                a += start[0] * end[1] - end[0] * start[1];
                let r = len(sub(start, center));
                let a0 = libm::atan2(start[1] - center[1], start[0] - center[0]);
                let a1 = libm::atan2(end[1] - center[1], end[0] - center[0]);
                let mut sweep = a1 - a0;
                if ccw && sweep < 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                if !ccw && sweep > 0.0 {
                    sweep -= std::f64::consts::TAU;
                }
                a += r * r * (sweep - libm::sin(sweep));
            }
            _ => {}
        }
    }
    a / 2.0
}

fn reversed(edges: Vec<ProfileEdge>) -> Vec<ProfileEdge> {
    edges
        .into_iter()
        .rev()
        .map(|e| ProfileEdge {
            seg: match e.seg {
                Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
                Seg2::Arc { center, start, end, ccw } => Seg2::Arc { center, start: end, end: start, ccw: !ccw },
                s => s,
            },
            tag: e.tag,
        })
        .collect()
}

/// Chord tolerance of sampled arcs in flat outlines (mm).
const CHORD_TOL: f64 = 0.005;

/// Segments for an arc of `sweep` radians and radius `r` within [`CHORD_TOL`].
fn arc_steps(r: f64, sweep: f64) -> usize {
    let step = if r > CHORD_TOL { 2.0 * libm::acos(1.0 - CHORD_TOL / r) } else { sweep };
    ((sweep / step.max(1e-4)).ceil() as usize).clamp(1, 4096)
}

/// A loop sampled as a polyline (arcs and circles in steps of at most 5 degrees).
fn sample(l: &Loop2) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for e in &l.edges {
        match e.seg {
            Seg2::Line { a, .. } => out.push(a),
            Seg2::Arc { center, start, end, ccw } => {
                let r = len(sub(start, center));
                let a0 = libm::atan2(start[1] - center[1], start[0] - center[0]);
                let a1 = libm::atan2(end[1] - center[1], end[0] - center[0]);
                let mut sweep = a1 - a0;
                if ccw && sweep <= 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                if !ccw && sweep >= 0.0 {
                    sweep -= std::f64::consts::TAU;
                }
                let steps = arc_steps(r, sweep.abs());
                for s in 0..steps {
                    let a = a0 + sweep * (s as f64) / (steps as f64);
                    out.push([center[0] + r * libm::cos(a), center[1] + r * libm::sin(a)]);
                }
            }
            Seg2::Circle { center, radius } => {
                let steps = arc_steps(radius, std::f64::consts::TAU);
                for s in 0..steps {
                    let a = std::f64::consts::TAU * (s as f64) / (steps as f64);
                    out.push([center[0] + radius * libm::cos(a), center[1] + radius * libm::sin(a)]);
                }
            }
            Seg2::Bezier { p } => {
                for s in 0..8 {
                    let u = f64::from(s) / 8.0;
                    let v = 1.0 - u;
                    let w = [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u];
                    let mut q = [0.0, 0.0];
                    for (wi, pi) in w.iter().zip(p.iter()) {
                        q = addv(q, mul(*pi, *wi));
                    }
                    out.push(q);
                }
            }
        }
    }
    out
}

impl Evaluator {
    pub(crate) fn run_sheet(
        &self,
        s: &mut State,
        name: &str,
        d: &SheetDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        if let SheetOp::EdgeFlange { body, line, length, angle, bend_radius, k_factor, close, mitre } = &d.op {
            let v = |e: &Expr, dim| eval_expr(name, e, dim, params, units);
            let spec = EdgeSpec {
                length: v(length, Dim::LENGTH)?,
                angle: v(angle, Dim::ANGLE)?,
                r: v(bend_radius, Dim::LENGTH)?,
                k: v(k_factor, Dim::NONE)?,
                down: d.flip,
                close: *close,
                mitre: *mitre,
            };
            return self.run_edge_flange(s, name, body, line, &spec);
        }
        let Some(thickness) = &d.thickness else {
            return Err(EvalError::new(name, ErrorCode::Expression, "the thickness is missing"));
        };
        let t = eval_expr(name, thickness, Dim::LENGTH, params, units)?;
        if !(t > 1e-6 && t.is_finite()) {
            return Err(EvalError::new(name, ErrorCode::Expression, "the thickness must be a positive length")
                .with_hint("enter the sheet thickness, for example 2 mm"));
        }
        let (res, ctx_at, flat, sketch_name, regions) = match &d.op {
            SheetOp::Contour { curves, width, symmetric, bend_radius, k_factor } => {
                let PathRef::Sketch { sketch, curves } = curves else {
                    return Err(EvalError::new(name, ErrorCode::InvalidReference, "contour_flange takes sketch lines"));
                };
                let width = eval_expr(name, width, Dim::LENGTH, params, units)?;
                if width.abs() < 1e-6 || !width.is_finite() {
                    return Err(EvalError::new(name, ErrorCode::Expression, "the width is zero")
                        .with_hint("enter how deep the flange is along the sketch normal, for example 40 mm"));
                }
                let r = eval_expr(name, bend_radius, Dim::LENGTH, params, units)?;
                if !(r >= 0.0 && r.is_finite()) {
                    return Err(EvalError::new(name, ErrorCode::Expression, "the bend radius must not be negative"));
                }
                let kf = eval_expr(name, k_factor, Dim::NONE, params, units)?;
                if !(0.0..=1.0).contains(&kf) {
                    return Err(EvalError::new(name, ErrorCode::Expression, format!("k_factor {kf} is outside 0..1"))
                        .with_hint(
                            "the neutral fibre's place in the thickness: 0.33 to 0.5 is usual, 0.44 for steel",
                        ));
                }
                let (sv, _) = sketch_input(name, sketch, s)?;
                let chain = sketch::path_chain(name, sketch, &sv, curves)?;
                let mut lines: Vec<(P2, P2, String)> = Vec::new();
                for e in &chain.edges {
                    let id = gusset_naming::tag_entity(e.tag).unwrap_or_default();
                    match e.seg {
                        Seg2::Line { a, b } => lines.push((a, b, id)),
                        _ => {
                            return Err(EvalError::new(
                                name,
                                ErrorCode::InvalidReference,
                                format!("{id} is not a line; a contour flange bends straight lines only"),
                            )
                            .with_hint("draw the contour with lines; the bends are added at the corners"));
                        }
                    }
                }
                if lines.len() > gusset_doc::MAX_CONTOUR_LINES {
                    return Err(EvalError::new(name, ErrorCode::TooComplex, "too many lines in one contour flange"));
                }
                if let (Some(f), Some(l)) = (lines.first(), lines.last())
                    && lines.len() > 1
                    && len(sub(f.0, l.1)) < 1e-6
                {
                    return Err(EvalError::new(name, ErrorCode::InvalidReference, "the contour is closed")
                        .with_hint("leave the contour open (a closed one cannot unfold), or use a tab"));
                }
                // Travel from the first listed line (else the chain's own order).
                let first = curves.first().cloned();
                if lines.len() > 1 && first.as_ref().is_some_and(|f| lines.last().is_some_and(|l| &l.2 == f)) {
                    lines.reverse();
                    for l in &mut lines {
                        std::mem::swap(&mut l.0, &mut l.1);
                    }
                }
                let mut pts: Vec<P2> = lines.iter().map(|l| l.0).collect();
                if let Some(l) = lines.last() {
                    pts.push(l.1);
                }
                let ids: Vec<String> = lines.iter().map(|l| l.2.clone()).collect();
                let side = if d.flip { -1.0 } else { 1.0 };
                let sec = contour_section(&pts, &ids, t, r, kf, side, width).map_err(|m| {
                    let mut e = EvalError::new(name, ErrorCode::InvalidReference, m)
                        .with_hint("lengthen the line, or use a smaller bend radius or thickness");
                    e.reference = Some(sketch.clone());
                    e
                })?;
                let mut edges = sec.edges;
                if loop_area(&edges) < 0.0 {
                    edges = reversed(edges);
                }
                let profile = Profile {
                    frame: chain.frame,
                    regions: vec![Region2 { outer: Loop2 { edges }, holes: Vec::new() }],
                };
                let res = k
                    .extrude(&profile, &ExtrudeSpec { distance: width, symmetric: *symmetric })
                    .map_err(|ke| kernel_err(name, "contour flange", ke))?;
                let mut flat = sec.flat;
                if let Some(c) = &mut flat.contour {
                    c.sketch = sketch.clone();
                    c.z = if *symmetric {
                        [-width.abs() / 2.0, width.abs() / 2.0]
                    } else {
                        [width.min(0.0), width.max(0.0)]
                    };
                }
                (res, Some(ids), flat, sketch.clone(), Vec::new())
            }
            SheetOp::Tab { profile } => {
                let (solved, frame) = sketch_input(name, &profile.sketch, s)?;
                let (kp, regions) = sketch::kernel_profile(name, &profile.sketch, &solved, frame, &profile.regions)?;
                let distance = if d.flip { -t } else { t };
                let res = k
                    .extrude(&kp, &ExtrudeSpec { distance, symmetric: false })
                    .map_err(|ke| kernel_err(name, "tab", ke))?;
                let mut outlines = Vec::new();
                let mut circles = Vec::new();
                for r in &kp.regions {
                    outlines.push(sample(&r.outer));
                    for h in &r.holes {
                        if let [ProfileEdge { seg: Seg2::Circle { center, radius }, .. }] = h.edges.as_slice() {
                            circles.push(FlatCircle { outline: outlines.len(), centre: *center, radius: *radius });
                        }
                        outlines.push(sample(h));
                    }
                }
                let base = Some(TabBase { sketch: profile.sketch.clone(), flip: d.flip });
                (
                    res,
                    None,
                    FlatPattern {
                        thickness: t,
                        outlines,
                        circles,
                        bends: Vec::new(),
                        base,
                        contour: None,
                        flange_ends: Vec::new(),
                        changed_by: Vec::new(),
                    },
                    profile.sketch.clone(),
                    regions,
                )
            }
            SheetOp::EdgeFlange { .. } => return Ok(()),
        };
        let body_name = d.body_name.as_deref();
        match &ctx_at {
            Some(at) => {
                self.combine(s, name, &OpContext::Sheet { feature: name, at }, res, BodyOp::New, (None, body_name))?
            }
            None => {
                let ctx = OpContext::Sweep { feature: name, sketch: &sketch_name, regions: &regions };
                self.combine(s, name, &ctx, res, BodyOp::New, (None, body_name))?;
            }
        }
        if let Some(slot) = s.bodies.iter_mut().rev().find(|b| b.feature == name) {
            let b = Body {
                name: slot.name.clone(),
                feature: slot.feature.clone(),
                shape: slot.shape.clone(),
                names: slot.names.clone(),
                topology: slot.topology.clone(),
                holes: slot.holes.clone(),
                sheet: Some(Arc::new(flat)),
            };
            *slot = Arc::new(b);
        }
        Ok(())
    }
}

/// The gap between mitred walls (mm): welded shut they would be the butt joint.
const MITRE_GAP: f64 = 0.1;

/// The evaluated values of an edge flange.
pub(crate) struct EdgeSpec {
    pub(crate) length: f64,
    pub(crate) angle: f64,
    pub(crate) r: f64,
    pub(crate) k: f64,
    pub(crate) down: bool,
    pub(crate) close: bool,
    /// Closed corners meet on the diagonal.
    pub(crate) mitre: bool,
}

/// Inside test by the even-odd rule over all outlines.
fn inside(outlines: &[Vec<[f64; 2]>], p: P2) -> bool {
    let mut odd = false;
    for l in outlines {
        let n = l.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (l.get(i), l.get((i + 1) % n)) else { continue };
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
                odd = !odd;
            }
        }
    }
    odd
}

/// The cross-section of an edge flange in (u out of the tab, v along the sheet normal),
/// the tab's sheet occupying v in [0, t] and ending at u = 0.
pub(crate) fn edge_section(t: f64, e: &EdgeSpec) -> Vec<ProfileEdge> {
    let (r, th) = (e.r, e.angle);
    let c = [0.0, t + r];
    let rad = [libm::sin(th), -libm::cos(th)];
    let o1 = addv(c, mul(rad, r + t));
    let i1 = addv(c, mul(rad, r));
    let d = [libm::cos(th), libm::sin(th)];
    let (o2, i2) = (addv(o1, mul(d, e.length)), addv(i1, mul(d, e.length)));
    let tag = |p: SheetPart| sheet_tag(p, 0);
    let mut edges = vec![
        ProfileEdge {
            seg: Seg2::Arc { center: c, start: [0.0, 0.0], end: o1, ccw: true },
            tag: tag(SheetPart::BendOut),
        },
        ProfileEdge { seg: Seg2::Line { a: o1, b: o2 }, tag: tag(SheetPart::Flat) },
        ProfileEdge { seg: Seg2::Line { a: o2, b: i2 }, tag: tag(SheetPart::End) },
        ProfileEdge { seg: Seg2::Line { a: i2, b: i1 }, tag: tag(SheetPart::FlatBack) },
    ];
    if r > 1e-9 {
        edges.push(ProfileEdge {
            seg: Seg2::Arc { center: c, start: i1, end: [0.0, t], ccw: false },
            tag: tag(SheetPart::BendIn),
        });
    }
    edges.push(ProfileEdge { seg: Seg2::Line { a: [0.0, t], b: [0.0, 0.0] }, tag: tag(SheetPart::Start) });
    if e.down {
        // Mirrored across the sheet's mid-plane: the bend turns the other way.
        let m = |p: P2| [p[0], t - p[1]];
        edges = edges
            .into_iter()
            .map(|pe| ProfileEdge {
                seg: match pe.seg {
                    Seg2::Line { a, b } => Seg2::Line { a: m(a), b: m(b) },
                    Seg2::Arc { center, start, end, ccw } => {
                        Seg2::Arc { center: m(center), start: m(start), end: m(end), ccw: !ccw }
                    }
                    other => other,
                },
                tag: pe.tag,
            })
            .collect();
    }
    edges
}

/// Where an edge flange bends: the line from `a` to `b` in the flat pattern with the
/// outward direction `out` there, and the flange profile's frame and extrusion.
struct Site {
    a: P2,
    b: P2,
    out: P2,
    frame: Frame,
    along: f64,
}

type SiteError = (String, &'static str);

/// A tab's outline line: the flat pattern is the tab's sketch.
fn tab_site(s: &State, flat: &FlatPattern, base: &TabBase, line: &str, t: f64) -> Result<Site, SiteError> {
    let sv = s
        .solved
        .get(&base.sketch)
        .cloned()
        .ok_or_else(|| (format!("'{}' is not evaluated", base.sketch), "rebuild the model"))?;
    let (a, b) = sv.line_points.get(line).copied().ok_or_else(|| {
        (format!("'{line}' is not a line of {}", base.sketch), "name an outline line of the tab's sketch")
    })?;
    let ab = sub(b, a);
    let l = len(ab);
    if l < 1e-6 {
        return Err((format!("line {line} has no length"), "pick another line"));
    }
    // Outward: the side of the line that is outside the flat outline.
    let mid = mul(addv(a, b), 0.5);
    let perp = [-ab[1] / l, ab[0] / l];
    let probe = (l * 1e-3).min(1e-2);
    let (left, right) =
        (inside(&flat.outlines, addv(mid, mul(perp, probe))), inside(&flat.outlines, sub(mid, mul(perp, probe))));
    let out = match (left, right) {
        (false, true) => perp,
        (true, false) => mul(perp, -1.0),
        _ => {
            return Err((
                format!("line {line} is not on the outline (or a flange next to it covers it)"),
                "pick a line of the tab's outer boundary, not one inside it or already flanged",
            ));
        }
    };
    let fr = sv.frame;
    let n3 = fr.normal();
    let o3 = gusset_kernel::add(gusset_kernel::scale(fr.x, out[0]), gusset_kernel::scale(fr.y, out[1]));
    let mut origin = fr.to_world(a);
    if base.flip {
        origin = gusset_kernel::sub(origin, gusset_kernel::scale(n3, t));
    }
    let e3 = gusset_kernel::sub(fr.to_world(b), fr.to_world(a));
    let along = if gusset_kernel::dot(cross(o3, n3), e3) >= 0.0 { l } else { -l };
    Ok(Site { a, b, out, frame: Frame { origin, x: o3, y: n3 }, along })
}

/// A contour flange's side edge `<line>_start` (the low side along its sketch normal) or
/// `<line>_end`: the flange stands off that side of the straight.
fn contour_site(s: &State, flat: &FlatPattern, c: &ContourBase, line: &str) -> Result<Site, SiteError> {
    let hint = "name a contour line's side: l2_start (low side along the sketch normal) or l2_end";
    let (id, hi) = match (line.strip_suffix("_start"), line.strip_suffix("_end")) {
        (Some(id), _) => (id, false),
        (_, Some(id)) => (id, true),
        _ => return Err((format!("'{line}' names no side of a contour line"), hint)),
    };
    let i = c
        .straights
        .iter()
        .position(|st| st.line == id)
        .ok_or_else(|| (format!("'{id}' is not a line of the contour flange"), hint))?;
    let st = c.straights.get(i).ok_or_else(|| (format!("no line '{id}'"), hint))?;
    // Neighbouring flanges on the same side would meet inside the bend between them.
    let side = if hi { "_end" } else { "_start" };
    for j in [i.wrapping_sub(1), i + 1] {
        if let Some(n) = c.straights.get(j)
            && flat.bends.iter().any(|bd| bd.line == format!("{}{side}", n.line))
        {
            return Err((
                format!("{}{side} already has a flange: flanges on neighbouring sides would collide", n.line),
                "flange every other line on one side, or use the other side",
            ));
        }
    }
    if flat.bends.iter().any(|bd| bd.line == line) {
        return Err((format!("{line} already has a flange"), "pick another side"));
    }
    let sv = s.solved.get(&c.sketch).cloned().ok_or_else(|| (format!("'{}' is not evaluated", c.sketch), hint))?;
    let l = len(sub(st.b, st.a));
    let w = c.z[1] - c.z[0];
    let (y, out) = if hi { (w, [0.0, 1.0]) } else { (0.0, [0.0, -1.0]) };
    let fr = sv.frame;
    let n3 = fr.normal();
    let o3 = gusset_kernel::scale(n3, if hi { 1.0 } else { -1.0 });
    let y3 = gusset_kernel::add(gusset_kernel::scale(fr.x, st.inward[0]), gusset_kernel::scale(fr.y, st.inward[1]));
    let zs = if hi { c.z[1] } else { c.z[0] };
    let origin = gusset_kernel::add(fr.to_world(st.a), gusset_kernel::scale(n3, zs));
    let e3 = gusset_kernel::sub(fr.to_world(st.b), fr.to_world(st.a));
    let along = if gusset_kernel::dot(cross(o3, y3), e3) >= 0.0 { l } else { -l };
    Ok(Site { a: [st.x0, y], b: [st.x0 + l, y], out, frame: Frame { origin, x: o3, y: y3 }, along })
}

/// Inserts the flange `a`, `pa`, `pb`, `b` into the outline edge that holds the line from
/// `a` to `b` (the whole edge, or part of it); false when no edge holds it.
fn splice(outlines: &mut [Vec<[f64; 2]>], a: P2, b: P2, pa: P2, pb: P2) -> bool {
    let on = |p: P2, q: P2, x: P2| {
        let pq = sub(q, p);
        let l2 = pq[0] * pq[0] + pq[1] * pq[1];
        if l2 <= 0.0 {
            return None;
        }
        let u = ((x[0] - p[0]) * pq[0] + (x[1] - p[1]) * pq[1]) / l2;
        let d = len(sub(x, addv(p, mul(pq, u))));
        (d < 1e-6 && (-1e-9..=1.0 + 1e-9).contains(&u)).then_some(u)
    };
    for o in outlines.iter_mut() {
        let n = o.len();
        let hit = (0..n).find_map(|i| {
            let (p, q) = (*o.get(i)?, *o.get((i + 1) % n)?);
            let (ua, ub) = (on(p, q, a)?, on(p, q, b)?);
            let pts = if ua <= ub { [a, pa, pb, b] } else { [b, pb, pa, a] };
            let ins: Vec<[f64; 2]> =
                pts.into_iter().filter(|x| len(sub(*x, p)) > 1e-9 && len(sub(*x, q)) > 1e-9).collect();
            Some((i, ins))
        });
        if let Some((i, ins)) = hit {
            o.splice(i + 1..i + 1, ins);
            return true;
        }
    }
    false
}

impl Evaluator {
    fn run_edge_flange(
        &self,
        s: &mut State,
        name: &str,
        body: &str,
        line: &str,
        e: &EdgeSpec,
    ) -> Result<(), EvalError> {
        let bad = |m: String| EvalError::new(name, ErrorCode::InvalidReference, m);
        let expr = |m: &str| EvalError::new(name, ErrorCode::Expression, m.to_string());
        if !(e.length > 1e-6 && e.length.is_finite()) {
            return Err(expr("the flange length must be a positive length"));
        }
        if !(e.angle > MIN_BEND && e.angle <= std::f64::consts::PI + 1e-9) {
            return Err(expr("the bend angle must be between 0 and 180 deg").with_hint("90 deg for a right angle"));
        }
        // A hem (folded back over the tab) keeps the gap of its bend.
        if e.angle > std::f64::consts::PI - 1e-3 && e.r < 1e-6 {
            return Err(expr("a hem (a bend near 180 deg) needs a bend radius").with_hint("e.g. bend_radius: 0.5 mm"));
        }
        if !(e.r >= 0.0 && e.r.is_finite()) {
            return Err(expr("the bend radius must not be negative"));
        }
        if !(0.0..=1.0).contains(&e.k) {
            return Err(expr("k_factor must be between 0 and 1"));
        }
        let target = s.bodies.iter().find(|b| b.feature == body).cloned().ok_or_else(|| {
            EvalError::new(
                name,
                ErrorCode::NoTargetBody,
                format!("body '{body}' does not exist (did its feature fail?)"),
            )
        })?;
        let flat = target.sheet.clone().ok_or_else(|| bad(format!("'{body}' is not sheet metal")))?;
        if let Some(by) = flat.changed_by.first() {
            return Err(bad(format!("'{body}' was changed by {by} since it was made"))
                .with_hint("put edge flanges right after their tab, before cuts, holes or moves"));
        }
        let t = flat.thickness;
        let end = line.strip_suffix("_end").and_then(|f| flat.flange_ends.iter().find(|x| x.flange == f));
        let site = match (&flat.base, &flat.contour) {
            _ if end.is_some() => {
                let x = end.ok_or_else(|| bad(format!("no flange end '{line}'")))?;
                if flat.bends.iter().any(|bd| bd.line == line) {
                    return Err(bad(format!("{line} already has a flange")).with_hint("pick another edge"));
                }
                Site { a: x.a, b: x.b, out: x.out, frame: x.frame, along: x.along }
            }
            (Some(base), _) => tab_site(s, &flat, base, line, t).map_err(|(m, h)| bad(m).with_hint(h))?,
            (None, Some(c)) => contour_site(s, &flat, c, line).map_err(|(m, h)| bad(m).with_hint(h))?,
            (None, None) => {
                return Err(bad(format!("'{body}' is not a tab or a contour flange"))
                    .with_hint("edge flanges bend off a tab's outline or a contour flange's sides"));
            }
        };
        let (a, b, out2) = (site.a, site.b, site.out);
        let ab = sub(b, a);
        let mut edges = edge_section(t, e);
        if loop_area(&edges) < 0.0 {
            edges = reversed(edges);
        }
        let profile =
            Profile { frame: site.frame, regions: vec![Region2 { outer: Loop2 { edges }, holes: Vec::new() }] };
        let res = self
            .kernel
            .extrude(&profile, &ExtrudeSpec { distance: site.along, symmetric: false })
            .map_err(|ke| kernel_err(name, "edge flange", ke))?;
        // The flat pattern grows by the bend zone and the flange beyond the line.
        let ba = e.angle * (e.r + e.k * t);
        let reach = ba + e.length;
        let mut nf = (*flat).clone();
        let (pa, pb) = (addv(a, mul(out2, reach)), addv(b, mul(out2, reach)));
        // At a convex corner two flanges leave an open corner; at a concave one this flange
        // would run into the sheet already there (the tab or another flange).
        for f in [0.02, 0.5, 0.98] {
            let along = addv(a, mul(ab, f));
            for depth in [0.25, 0.75] {
                if inside(&flat.outlines, addv(along, mul(out2, reach * depth))) {
                    return Err(bad(format!("a flange on {line} would run into the sheet next to it"))
                        .with_hint("flange lines at outside corners only, or shorten the neighbouring flange"));
                }
            }
        }
        if !splice(&mut nf.outlines, a, b, pa, pb) {
            return Err(bad(format!("line {line} is not on {body}'s outline")));
        }
        // This flange's far edge, for a flange bent off it: its outer face's end in the
        // section, the new frame along the flange and across its thickness (towards the
        // inner face), the same run along the line.
        {
            let (r, th, len) = (e.r, e.angle, e.length);
            let c = [0.0, t + r];
            let rad = [libm::sin(th), -libm::cos(th)];
            let d = [libm::cos(th), libm::sin(th)];
            let o2 = addv(addv(c, mul(rad, r + t)), mul(d, len));
            let i2 = addv(addv(c, mul(rad, r)), mul(d, len));
            let m = |p: P2| if e.down { [p[0], t - p[1]] } else { p };
            let (o2, i2) = (m(o2), m(i2));
            let d = if e.down { [d[0], -d[1]] } else { d };
            let f = site.frame;
            let at = |q: P2| {
                gusset_kernel::add(
                    f.origin,
                    gusset_kernel::add(gusset_kernel::scale(f.x, q[0]), gusset_kernel::scale(f.y, q[1])),
                )
            };
            let dir = |v: P2| gusset_kernel::add(gusset_kernel::scale(f.x, v[0]), gusset_kernel::scale(f.y, v[1]));
            let x = dir(d);
            let y = dir(mul(sub(i2, o2), 1.0 / t));
            let e3 = gusset_kernel::scale(f.normal(), site.along.signum());
            let l = site.along.abs();
            let along = if gusset_kernel::dot(cross(x, y), e3) >= 0.0 { l } else { -l };
            let (centre, start) = if e.down { ([0.0, -r], [0.0, 1.0]) } else { ([0.0, t + r], [0.0, -1.0]) };
            let bend = FlangeBend {
                frame: f,
                along: site.along,
                a,
                b,
                out: out2,
                centre,
                start,
                radius: r + t,
                turn: if e.down { -th } else { th },
                allowance: ba,
                length: len,
                on_tab: flat.base.is_some() && end.is_none(),
            };
            nf.flange_ends.push(FlangeEnd {
                flange: name.to_string(),
                a: pa,
                b: pb,
                out: out2,
                frame: Frame { origin: at(o2), x, y },
                along,
                bend,
            });
        }
        nf.bends.push(FlatBend {
            line: line.to_string(),
            angle: if e.down { -e.angle } else { e.angle },
            inner_radius: e.r,
            k_factor: e.k,
            allowance: ba,
            start: [a, b],
            end: [addv(a, mul(out2, ba)), addv(b, mul(out2, ba))],
        });
        let at = [line.to_string()];
        self.combine(s, name, &OpContext::Sheet { feature: name, at: &at }, res, BodyOp::Add, (Some(body), None))?;
        // Walls bent past a right angle lean over a corner they share with a neighbouring
        // flange and run into it; left open, the solid would lose the overlap while the flat
        // pattern still draws both (dogfooding F9-5): refused, pointing at `close`.
        if !e.close && flat.base.is_some() && end.is_none() {
            let me = Wall {
                frame: site.frame,
                along: site.along,
                a,
                b,
                out: out2,
                r: e.r,
                len: e.length,
                down: e.down,
                ba,
                angle: e.angle,
                name: String::new(),
            };
            let right = std::f64::consts::FRAC_PI_2 + 1e-9;
            for fe in flat.flange_ends.iter().filter(|fe| fe.bend.on_tab) {
                let fb = &fe.bend;
                let other = Wall {
                    frame: fb.frame,
                    along: fb.along,
                    a: fb.a,
                    b: fb.b,
                    out: fb.out,
                    r: fb.radius - t,
                    len: fb.length,
                    down: fb.turn < 0.0,
                    ba: fb.allowance,
                    angle: fb.turn.abs(),
                    name: String::new(),
                };
                if other.down == me.down
                    && (me.angle > right || other.angle > right)
                    && me.corner_with(&other).is_some()
                {
                    return Err(bad(format!(
                        "{line}'s flange and {}'s lean over the corner they share and run into each other",
                        fe.flange
                    ))
                    .with_hint("close the corner (close: true or close: mitre), or bend 90 deg or less"));
                }
            }
        }
        if e.close {
            let me = Wall {
                frame: site.frame,
                along: site.along,
                a,
                b,
                out: out2,
                r: e.r,
                len: e.length,
                down: e.down,
                ba,
                angle: e.angle,
                name: format!("{line}_corner"),
            };
            if !(flat.base.is_some() && end.is_none()) {
                return Err(bad(format!("close joins flanges off a tab's outline; {line} is not one")));
            }
            if e.angle > MAX_CLOSE_ANGLE + 1e-9 {
                return Err(expr("close joins flanges bent up to 150 deg")
                    .with_hint("bend 150 deg or less, or leave the corner open"));
            }
            let mut closed = 0;
            for fe in flat.flange_ends.iter().filter(|fe| fe.bend.on_tab) {
                let fb = &fe.bend;
                let other = Wall {
                    frame: fb.frame,
                    along: fb.along,
                    a: fb.a,
                    b: fb.b,
                    out: fb.out,
                    r: fb.radius - t,
                    len: fb.length,
                    down: fb.turn < 0.0,
                    ba: fb.allowance,
                    angle: fb.turn.abs(),
                    name: format!("{}_corner", fe.flange),
                };
                if (other.angle - me.angle).abs() > 1e-9 || other.down != me.down {
                    continue;
                }
                let Some((p, me_start, other_start, square)) = me.corner_with(&other) else { continue };
                if !square || (me.angle - std::f64::consts::FRAC_PI_2).abs() > 1e-9 {
                    // Other angles: both walls run to the plane that mirrors one into the
                    // other (equal radii), 0.1 mm short of it for a mitre.
                    if (me.r - other.r).abs() > 1e-9 {
                        return Err(bad(format!(
                            "{line}: closing a corner at {:.4} deg needs equal bend radii",
                            me.angle.to_degrees()
                        ))
                        .with_hint("give both flanges the same bend radius"));
                    }
                    let gap = if e.mitre { MITRE_GAP } else { 0.0 };
                    let pairs = [(&me, me_start, &other, other_start), (&other, other_start, &me, me_start)];
                    let obtuse = me.angle > std::f64::consts::FRAC_PI_2 + 1e-9;
                    if obtuse {
                        // Past a right angle the walls lean over the tab and cross: each is
                        // cut away beyond the mirror plane, except where the other wall is.
                        let k = self.kernel.as_ref();
                        let kerr = |ke| kernel_err(name, "closed corner", ke);
                        let mut fulls = Vec::new();
                        for (w, ws, o, os) in pairs {
                            fulls.push(w.angled_end(k, t, ws, (o, os), gap, EndPart::Full).map_err(kerr)?);
                        }
                        for (i, (w, ws, o, os)) in pairs.into_iter().enumerate() {
                            let trimmed = w.angled_end(k, t, ws, (o, os), gap, EndPart::Trimmed).map_err(kerr)?;
                            let Some(keep) = fulls.get(1 - i) else { continue };
                            let rest =
                                k.boolean(gusset_kernel::BoolOp::Cut, &trimmed.shape, &keep.shape).map_err(kerr)?;
                            let at = [w.name.clone()];
                            self.combine(
                                s,
                                name,
                                &OpContext::Sheet { feature: name, at: &at },
                                inherit(&trimmed, rest),
                                BodyOp::Cut,
                                (Some(body), None),
                            )?;
                        }
                    }
                    for (w, ws, o, os) in pairs {
                        let res = w
                            .angled_end(self.kernel.as_ref(), t, ws, (o, os), gap, EndPart::Extension)
                            .map_err(|ke| kernel_err(name, "closed corner", ke))?;
                        let at = [w.name.clone()];
                        self.combine(
                            s,
                            name,
                            &OpContext::Sheet { feature: name, at: &at },
                            res,
                            BodyOp::Add,
                            (Some(body), None),
                        )?;
                        let (q1, q2, a1, a2) = w.angled_flap(p, ws, o, os, gap);
                        let (e1, e2) = (addv(q1, a1), addv(q2, a2));
                        if !splice(&mut nf.outlines, q1, q2, e1, e2) {
                            return Err(bad(format!("the corner at {line} cannot be closed in the flat pattern")));
                        }
                        // The end cut runs into the strip's end edge: the strip's corner
                        // point goes (the flap is negative there).
                        let u = w.beyond2(ws);
                        if a2[0] * u[0] + a2[1] * u[1] < -1e-9 {
                            for ol in nf.outlines.iter_mut() {
                                let n = ol.len();
                                let at_e2 = ol.iter().position(|x| len(sub(*x, e2)) < 1e-9);
                                let corner = at_e2.and_then(|i| {
                                    [(i + 1) % n, (i + n - 1) % n]
                                        .into_iter()
                                        .find(|j| ol.get(*j).is_some_and(|x| len(sub(*x, q2)) < 1e-9))
                                });
                                if let Some(j) = corner {
                                    ol.remove(j);
                                    break;
                                }
                            }
                        }
                    }
                    closed += 1;
                    continue;
                }
                // Overlap: the new flange's wall covers the corner and the other runs up to its
                // inside. Mitre: each runs to the diagonal through the corner's inner and outer
                // edges (`d` shifts it when the radii differ); the flat flap is as wide as the
                // wedge at the neutral layer.
                let walls = if e.mitre {
                    let rn = |w: &Wall| w.ba / std::f64::consts::FRAC_PI_2;
                    // Each wall stops half the gap short of the diagonal (measured across it).
                    let c = MITRE_GAP / std::f64::consts::SQRT_2;
                    let (d1, d2) = (other.r - me.r - c, me.r - other.r - c);
                    [(&me, me_start, rn(&me) + d1, Some(d1)), (&other, other_start, rn(&other) + d2, Some(d2))]
                } else {
                    [(&me, me_start, other.r + t, None), (&other, other_start, me.r, None)]
                };
                for (w, at_start, ext, mitre) in walls {
                    let res = match mitre {
                        None => self.kernel.extrude(
                            &w.extension(t, at_start),
                            &ExtrudeSpec { distance: w.reach(at_start, ext), symmetric: false },
                        ),
                        Some(d) => {
                            let (profile, distance) = w.mitre(t, at_start, d);
                            self.kernel.extrude(&profile, &ExtrudeSpec { distance, symmetric: false })
                        }
                    }
                    .map_err(|ke| kernel_err(name, "closed corner", ke))?;
                    let at = [w.name.clone()];
                    self.combine(
                        s,
                        name,
                        &OpContext::Sheet { feature: name, at: &at },
                        res,
                        BodyOp::Add,
                        (Some(body), None),
                    )?;
                    let (q1, q2, away) = w.flap(p, at_start, ext);
                    if !splice(&mut nf.outlines, q1, q2, addv(q1, away), addv(q2, away)) {
                        return Err(bad(format!("the corner at {line} cannot be closed in the flat pattern")));
                    }
                }
                closed += 1;
            }
            if closed == 0 {
                return Err(bad(format!("no 90 deg flange meets {line} at an outside corner to close"))
                    .with_hint("flange the neighbouring line first (same direction, 90 deg), then close"));
            }
        }
        if let Some(slot) = s.bodies.iter_mut().find(|x| x.feature == body) {
            let nb = Body {
                name: slot.name.clone(),
                feature: slot.feature.clone(),
                shape: slot.shape.clone(),
                names: slot.names.clone(),
                topology: slot.topology.clone(),
                holes: slot.holes.clone(),
                sheet: Some(Arc::new(nf)),
            };
            *slot = Arc::new(nb);
        }
        Ok(())
    }
}

/// A boolean's result named like its first input: that input's faces keep their origins, the
/// faces the other input made take the far cap's place.
fn inherit(first: &gusset_kernel::OpResult, res: gusset_kernel::OpResult) -> gusset_kernel::OpResult {
    use gusset_kernel::FaceOrigin;
    let face_origins = res
        .face_origins
        .iter()
        .map(|os| {
            os.iter()
                .flat_map(|o| match o {
                    FaceOrigin::Input { input: 0, face } => {
                        first.face_origins.get(*face as usize).cloned().unwrap_or_default()
                    }
                    _ => vec![FaceOrigin::Cap { end: gusset_kernel::CapEnd::End, region: 0 }],
                })
                .collect()
        })
        .collect();
    gusset_kernel::OpResult { shape: res.shape, face_origins }
}

/// The widest bend (rad) of flanges whose corners `close` joins.
const MAX_CLOSE_ANGLE: f64 = 150.0 * std::f64::consts::PI / 180.0;

/// What [`Wall::angled_end`] builds at a corner.
#[derive(Clone, Copy, PartialEq)]
enum EndPart {
    /// The wall's flat part past the line's end, up to the mirror plane (flanges up to 90 deg).
    Extension,
    /// The wall's whole flat part along the line and past its end, up to the mirror plane.
    Full,
    /// The wall's flat part along the line beyond the mirror plane: the part to cut away.
    Trimmed,
}

/// A 90 degree edge flange's wall, for closing corners: its profile frame and run along the
/// line, the line in the flat pattern (`a` to `b`, outward `out`), inside radius, flat
/// length, direction and bend allowance.
struct Wall {
    frame: Frame,
    along: f64,
    a: P2,
    b: P2,
    out: P2,
    r: f64,
    len: f64,
    down: bool,
    ba: f64,
    /// The bend angle (rad, 0 to a right angle).
    angle: f64,
    name: String,
}

impl Wall {
    /// The shared end `p` of this wall's line and `o`'s, if they meet at an outside corner
    /// (each line leaves `p` against the other's outward direction), and whether `p` is the
    /// start of each line.
    fn corner_with(&self, o: &Wall) -> Option<(P2, bool, bool, bool)> {
        let near = |x: P2, y: P2| len(sub(x, y)) < 1e-6;
        for (p, ms) in [(self.a, true), (self.b, false)] {
            for (q, os) in [(o.a, true), (o.b, false)] {
                if !near(p, q) {
                    continue;
                }
                let unit = |v: P2| mul(v, 1.0 / len(v).max(1e-300));
                let away_me = unit(sub(if ms { self.b } else { self.a }, p));
                let away_o = unit(sub(if os { o.b } else { o.a }, p));
                let dot = |x: P2, y: P2| x[0] * y[0] + x[1] * y[1];
                // An outside corner: each line leaves `p` against the other's outward side
                // (square when that is straight against it).
                if dot(away_o, self.out) < -1e-6 && dot(away_me, o.out) < -1e-6 {
                    let square = dot(away_o, self.out) < -0.99 && dot(away_me, o.out) < -0.99;
                    return Some((p, ms, os, square));
                }
            }
        }
        None
    }

    /// The wall's flat part (beyond the bend) as a profile at the line's start or end.
    fn extension(&self, t: f64, at_start: bool) -> Profile {
        let r = self.r;
        // The 90 degree section's flat part: outer (o) and inner (i) faces from the bend's end.
        let (o1, i1) = ([r + t, t + r], [r, t + r]);
        let (o2, i2) = (addv(o1, [0.0, self.len]), addv(i1, [0.0, self.len]));
        let m = |p: P2| if self.down { [p[0], t - p[1]] } else { p };
        let tag = |p: SheetPart| sheet_tag(p, 0);
        let mut edges = vec![
            ProfileEdge { seg: Seg2::Line { a: m(o1), b: m(o2) }, tag: tag(SheetPart::Flat) },
            ProfileEdge { seg: Seg2::Line { a: m(o2), b: m(i2) }, tag: tag(SheetPart::End) },
            ProfileEdge { seg: Seg2::Line { a: m(i2), b: m(i1) }, tag: tag(SheetPart::FlatBack) },
            ProfileEdge { seg: Seg2::Line { a: m(i1), b: m(o1) }, tag: tag(SheetPart::Start) },
        ];
        if loop_area(&edges) < 0.0 {
            edges = reversed(edges);
        }
        let mut frame = self.frame;
        if !at_start {
            frame.origin = gusset_kernel::add(frame.origin, gusset_kernel::scale(frame.normal(), self.along));
        }
        Profile { frame, regions: vec![Region2 { outer: Loop2 { edges }, holes: Vec::new() }] }
    }

    /// The wall's mitred end past the line's start or end: a profile across the wall's
    /// height (thickness offset `x` from the line, `s` past its end) bounded by the diagonal
    /// `s = x + d`, and the signed extrusion up the wall's flat length.
    fn mitre(&self, t: f64, at_start: bool, d: f64) -> (Profile, f64) {
        use gusset_kernel::{add as add3, cross, dot as dot3, scale};
        let r = self.r;
        let base =
            if at_start { self.frame.origin } else { add3(self.frame.origin, scale(self.frame.normal(), self.along)) };
        // The flat part starts at height t + r (mirrored when bent down).
        let (h0, up) = if self.down { (-r, -1.0) } else { (t + r, 1.0) };
        let beyond = scale(self.frame.normal(), self.reach(at_start, 1.0).signum());
        let frame = Frame { origin: add3(base, scale(self.frame.y, h0)), x: self.frame.x, y: beyond };
        let tag = |p: SheetPart| sheet_tag(p, 0);
        // Under the diagonal y = x + d; where it meets y = 0 past the inner face (radii
        // below the half gap) the profile is a triangle, not a clamped quad (review R7).
        let x0 = r.max(-d);
        let pts = [[x0, 0.0], [r + t, 0.0], [r + t, r + t + d], [x0, x0 + d]];
        let tags = [SheetPart::Start, SheetPart::Flat, SheetPart::End, SheetPart::FlatBack];
        let mut edges: Vec<ProfileEdge> = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .zip(tags)
            .filter(|((a, b), _)| len(sub(**b, **a)) > 1e-9)
            .map(|((a, b), p)| ProfileEdge { seg: Seg2::Line { a: *a, b: *b }, tag: tag(p) })
            .collect();
        if loop_area(&edges) < 0.0 {
            edges = reversed(edges);
        }
        let height = scale(self.frame.y, up);
        let distance = if dot3(cross(frame.x, frame.y), height) >= 0.0 { self.len } else { -self.len };
        (Profile { frame, regions: vec![Region2 { outer: Loop2 { edges }, holes: Vec::new() }] }, distance)
    }

    /// The wall's direction past its line's start or end, in space.
    fn beyond(&self, at_start: bool) -> gusset_kernel::V3 {
        gusset_kernel::scale(self.frame.normal(), self.reach(at_start, 1.0).signum())
    }

    /// The line's end at the corner, in space (the profile frame's origin there).
    fn corner_point(&self, at_start: bool) -> gusset_kernel::V3 {
        if at_start {
            self.frame.origin
        } else {
            gusset_kernel::add(self.frame.origin, gusset_kernel::scale(self.frame.normal(), self.along))
        }
    }

    /// For a corner at another angle: the wall's flat part carried well past the line's end
    /// and cut off at the plane that mirrors it into `o` (moved `gap / 2` towards this wall);
    /// see [`EndPart`] for what is built.
    fn angled_end(
        &self,
        k: &dyn gusset_kernel::Kernel,
        t: f64,
        at_start: bool,
        (o, o_start): (&Wall, bool),
        gap: f64,
        part: EndPart,
    ) -> gusset_kernel::KResult<gusset_kernel::OpResult> {
        use gusset_kernel::{add as add3, cross, norm, scale, sub as sub3};
        let (r, th) = (self.r, self.angle);
        let c = [0.0, t + r];
        let rad = [libm::sin(th), -libm::cos(th)];
        let d = [libm::cos(th), libm::sin(th)];
        let (o1, i1) = (addv(c, mul(rad, r + t)), addv(c, mul(rad, r)));
        let (o2, i2) = (addv(o1, mul(d, self.len)), addv(i1, mul(d, self.len)));
        let m = |p: P2| if self.down { [p[0], t - p[1]] } else { p };
        let tag = |p: SheetPart| sheet_tag(p, 0);
        let mut edges = vec![
            ProfileEdge { seg: Seg2::Line { a: m(o1), b: m(o2) }, tag: tag(SheetPart::Flat) },
            ProfileEdge { seg: Seg2::Line { a: m(o2), b: m(i2) }, tag: tag(SheetPart::End) },
            ProfileEdge { seg: Seg2::Line { a: m(i2), b: m(i1) }, tag: tag(SheetPart::FlatBack) },
            ProfileEdge { seg: Seg2::Line { a: m(i1), b: m(o1) }, tag: tag(SheetPart::Start) },
        ];
        if loop_area(&edges) < 0.0 {
            edges = reversed(edges);
        }
        let mut frame = self.frame;
        frame.origin = self.corner_point(at_start);
        // Past the end by as much as the wall's far side can need at this corner's angle.
        let (a, c) = self.corner_ratio(at_start, o, o_start);
        let reach = 2.0 * (r + t + self.len) * (c / (1.0 - a)).abs().max(1.0) + 1.0;
        let (frame, dist) = match part {
            EndPart::Extension => (frame, self.reach(at_start, reach)),
            EndPart::Trimmed => (frame, -self.reach(at_start, self.along.abs())),
            EndPart::Full => {
                let mut f = frame;
                f.origin = add3(f.origin, scale(self.beyond(at_start), reach));
                (f, -self.reach(at_start, reach + self.along.abs()))
            }
        };
        let prism = k.extrude(
            &Profile { frame, regions: vec![Region2 { outer: Loop2 { edges }, holes: Vec::new() }] },
            &ExtrudeSpec { distance: dist, symmetric: false },
        )?;
        // The half space past the mirror plane: a block on the plane, along its normal.
        let n = sub3(self.beyond(at_start), o.beyond(o_start));
        let n = scale(n, 1.0 / norm(n).max(1e-300));
        let up = self.frame.y;
        let side = cross(n, up);
        let origin = sub3(self.corner_point(at_start), scale(n, gap / 2.0));
        let big = 2.0 * reach;
        let sq = [[-big, -big], [big, -big], [big, big], [-big, big]];
        let block_edges: Vec<ProfileEdge> = sq
            .iter()
            .zip(sq.iter().cycle().skip(1))
            .map(|(a, b)| ProfileEdge { seg: Seg2::Line { a: *a, b: *b }, tag: 0 })
            .collect();
        let bframe = Frame { origin: add3(origin, [0.0; 3]), x: up, y: side };
        let bdist = if gusset_kernel::dot(bframe.normal(), n) >= 0.0 { big } else { -big };
        let block = k.extrude(
            &Profile {
                frame: bframe,
                regions: vec![Region2 { outer: Loop2 { edges: block_edges }, holes: Vec::new() }],
            },
            &ExtrudeSpec { distance: bdist, symmetric: false },
        )?;
        let op = if part == EndPart::Trimmed { gusset_kernel::BoolOp::Intersect } else { gusset_kernel::BoolOp::Cut };
        let cut = k.boolean(op, &prism.shape, &block.shape)?;
        Ok(inherit(&prism, cut))
    }

    /// In the flat pattern for a corner at another angle: the strip's side edge (from the
    /// bend's end to the flange's end) and how far past the corner the flap reaches at each
    /// end: the neutral layer's offset from the line, less the half gap across the plane.
    fn angled_flap(&self, p: P2, at_start: bool, o: &Wall, o_start: bool, gap: f64) -> (P2, P2, P2, P2) {
        let u = self.beyond2(at_start);
        let rn = self.ba / self.angle.max(1e-300);
        // Past the end by s where the mirror plane is: s (1 - a) = x c - gap/2 |bW - bO|, x the
        // neutral layer's distance out from the line.
        let (a, c) = self.corner_ratio(at_start, o, o_start);
        let span = len(sub(u, o.beyond2(o_start)));
        // Past a right angle the wall can end short of the line's end (a negative reach).
        let floor = if self.angle > std::f64::consts::FRAC_PI_2 + 1e-9 { f64::NEG_INFINITY } else { 0.0 };
        let reach = |x: f64| ((x * c - gap / 2.0 * span) / (1.0 - a).max(1e-9)).max(floor);
        let w0 = reach(rn * libm::sin(self.angle));
        let w1 = reach(rn * libm::sin(self.angle) + self.len * libm::cos(self.angle));
        (addv(p, mul(self.out, self.ba)), addv(p, mul(self.out, self.ba + self.len)), mul(u, w0), mul(u, w1))
    }

    /// The flat-pattern direction past the line's start or end.
    fn beyond2(&self, at_start: bool) -> P2 {
        let far = if at_start { self.b } else { self.a };
        let near = if at_start { self.a } else { self.b };
        let d = sub(near, far);
        mul(d, 1.0 / len(d).max(1e-300))
    }

    /// For a corner with `o`: how the other wall's direction past the corner splits into
    /// this one's (a) and its outward side (c), in the flat pattern.
    fn corner_ratio(&self, at_start: bool, o: &Wall, o_start: bool) -> (f64, f64) {
        let bo = o.beyond2(o_start);
        let dot = |x: P2, y: P2| x[0] * y[0] + x[1] * y[1];
        (dot(self.beyond2(at_start), bo), dot(self.out, bo))
    }

    /// The signed extrusion that carries the profile `ext` past the line's start or end.
    fn reach(&self, at_start: bool, ext: f64) -> f64 {
        let s = if self.along >= 0.0 { 1.0 } else { -1.0 };
        if at_start { -s * ext } else { s * ext }
    }

    /// In the flat pattern: the strip's side edge at the corner (from where the bend ends to
    /// the flange's end) and the way past the corner the flap reaches.
    fn flap(&self, p: P2, at_start: bool, ext: f64) -> (P2, P2, P2) {
        let far = if at_start { self.b } else { self.a };
        let d = sub(p, far);
        let away = mul(d, ext / len(d).max(1e-300));
        (addv(p, mul(self.out, self.ba)), addv(p, mul(self.out, self.ba + self.len)), away)
    }
}

/// How a sheet face maps to the flat pattern.
enum FlatMap<'a> {
    /// A tab's face on its sketch plane: the sketch's own coordinates.
    Tab(Frame),
    /// A contour flange straight's sketch-side face.
    Straight(&'a Straight, &'a ContourBase, Frame),
    /// An edge flange's outer face.
    Flange(&'a FlangeEnd),
    /// A contour flange bend's sketch-side face.
    Bend(&'a BendZone, &'a ContourBase, Frame),
    /// An edge flange bend's outer face.
    FlangeBend(&'a FlangeBend),
}

impl FlatMap<'_> {
    /// Whether the face is flat (a circle on it develops as a circle).
    fn is_flat(&self) -> bool {
        matches!(self, FlatMap::Tab(_) | FlatMap::Straight(..) | FlatMap::Flange(_))
    }

    /// The flat-pattern point of `p`, and how far `p` lies off the face's plane. On a bend
    /// face, `p` is first moved along the cut direction `dir` onto the neutral surface, so the
    /// flat size is the neutral arc length whichever side the loop came from.
    fn map(&self, p: gusset_kernel::V3, dir: Option<gusset_kernel::V3>) -> (P2, f64) {
        use gusset_kernel::{dot, scale, sub as sub3};
        match self {
            FlatMap::Tab(f) => {
                let d = sub3(p, f.origin);
                ([dot(d, f.x), dot(d, f.y)], dot(d, f.normal()).abs())
            }
            FlatMap::Straight(st, c, f) => {
                let a = f.to_world(st.a);
                let ab = sub3(f.to_world(st.b), a);
                let l = gusset_kernel::norm(ab).max(1e-300);
                let dir = scale(ab, 1.0 / l);
                let n = f.normal();
                let inward = gusset_kernel::add(scale(f.x, st.inward[0]), scale(f.y, st.inward[1]));
                let d = sub3(p, a);
                ([st.x0 + dot(d, dir), dot(sub3(p, f.origin), n) - c.z[0]], dot(d, inward).abs())
            }
            FlatMap::Bend(z, c, f) => {
                // Into the sketch plane, then the angle from the bend's start, as a share of
                // its turn, along its zone.
                let d = sub3(p, f.origin);
                let q = sub([dot(d, f.x), dot(d, f.y)], z.centre);
                let rad = len(q);
                let p = to_neutral(p, dir, f, z.centre, z.allowance / z.turn.abs().max(1e-300));
                let d = sub3(p, f.origin);
                let q = sub([dot(d, f.x), dot(d, f.y)], z.centre);
                let (cs, sn) = (z.start[0] * q[0] + z.start[1] * q[1], z.start[0] * q[1] - z.start[1] * q[0]);
                let phi = libm::atan2(sn, cs);
                let share = (phi / z.turn).clamp(-1e-6, 1.0 + 1e-6);
                ([z.x0 + share * z.allowance, dot(d, f.normal()) - c.z[0]], (rad - z.radius).abs())
            }
            FlatMap::FlangeBend(fb) => {
                let f = fb.frame;
                let o = sub3(p, f.origin);
                let off = (len(sub([dot(o, f.x), dot(o, f.y)], fb.centre)) - fb.radius).abs();
                let p = to_neutral(p, dir, &f, fb.centre, fb.allowance / fb.turn.abs().max(1e-300));
                let d = sub3(p, f.origin);
                let q = sub([dot(d, f.x), dot(d, f.y)], fb.centre);
                let (cs, sn) = (fb.start[0] * q[0] + fb.start[1] * q[1], fb.start[0] * q[1] - fb.start[1] * q[0]);
                let share = (libm::atan2(sn, cs) / fb.turn).clamp(-1e-6, 1.0 + 1e-6);
                let s = dot(d, scale(f.normal(), fb.along.signum()));
                let ab = sub(fb.b, fb.a);
                let u = mul(ab, 1.0 / len(ab).max(1e-300));
                (addv(addv(fb.a, mul(u, s)), mul(fb.out, share * fb.allowance)), off)
            }
            FlatMap::Flange(fe) => {
                let f = fe.frame;
                let e = scale(f.normal(), fe.along.signum());
                let d = sub3(p, f.origin);
                let (s, back) = (dot(d, e), -dot(d, f.x));
                let ab = sub(fe.b, fe.a);
                let u = mul(ab, 1.0 / len(ab).max(1e-300));
                (addv(addv(fe.a, mul(u, s)), mul(fe.out, -back)), dot(d, f.y).abs())
            }
        }
    }
}

/// `p` moved along `dir` onto the neutral surface (radius `rn` round `centre`, in the
/// sketch frame `f`) of a bend; unchanged without a direction or when the line misses it.
fn to_neutral(
    p: gusset_kernel::V3,
    dir: Option<gusset_kernel::V3>,
    f: &Frame,
    centre: P2,
    rn: f64,
) -> gusset_kernel::V3 {
    use gusset_kernel::{add, dot, scale, sub as sub3};
    let Some(d) = dir else { return p };
    let o = sub3(p, f.origin);
    let q = sub([dot(o, f.x), dot(o, f.y)], centre);
    let dq = [dot(d, f.x), dot(d, f.y)];
    let a = dq[0] * dq[0] + dq[1] * dq[1];
    if a < 1e-12 {
        return p;
    }
    let b = q[0] * dq[0] + q[1] * dq[1];
    let disc = b * b - a * (q[0] * q[0] + q[1] * q[1] - rn * rn);
    if disc < 0.0 {
        return p;
    }
    let sq = libm::sqrt(disc);
    let (t1, t2) = ((-b - sq) / a, (-b + sq) / a);
    add(p, scale(d, if t1.abs() <= t2.abs() { t1 } else { t2 }))
}

/// One cut-out developed into the flat pattern: its outline, and its centre and radius when
/// it is a full circle.
pub(crate) struct Cut {
    pub outline: Vec<[f64; 2]>,
    pub circle: Option<(P2, f64)>,
}

/// Joins polylines end to end into closed loops (ends within `tol`); `None` when they do not
/// close.
fn chain_loops(mut pieces: Vec<Vec<P2>>, tol: f64) -> Option<Vec<Vec<P2>>> {
    let near = |a: P2, b: P2| len(sub(a, b)) <= tol;
    let mut loops = Vec::new();
    while let Some(mut cur) = pieces.pop() {
        let start = *cur.first()?;
        loop {
            let end = *cur.last()?;
            if cur.len() > 2 && near(end, start) {
                cur.pop();
                break;
            }
            let i = pieces
                .iter()
                .position(|p| p.first().is_some_and(|q| near(*q, end)) || p.last().is_some_and(|q| near(*q, end)))?;
            let mut next = pieces.swap_remove(i);
            if !next.first().is_some_and(|q| near(*q, end)) {
                next.reverse();
            }
            cur.extend(next.into_iter().skip(1));
        }
        if cur.len() >= 3 {
            loops.push(cur);
        }
    }
    Some(loops)
}

impl Evaluator {
    /// The cut-outs a later feature made through the flat regions of sheet body `b`, as flat
    /// pattern outlines; `None` when the change is not that (it touches a bend or an edge,
    /// adds material, or moved the sheet): then the flat pattern is marked stale instead.
    pub(crate) fn unfold_cuts(
        &self,
        s: &State,
        b: &Body,
        flat: &FlatPattern,
        dir: Option<gusset_kernel::V3>,
        feature: &str,
    ) -> Option<Vec<Cut>> {
        use gusset_doc::{CapSide, FaceKey};
        #[derive(Clone, Copy, PartialEq)]
        enum Kind {
            Primary,
            Back,
            Other,
            New,
        }
        let names = &b.names.faces;
        let tab_frame = flat.base.as_ref().and_then(|t| s.solved.get(&t.sketch)).map(|sv| sv.frame);
        let contour_frame = flat.contour.as_ref().and_then(|c| s.solved.get(&c.sketch)).map(|sv| sv.frame);
        let mut maps: Vec<Option<FlatMap<'_>>> = Vec::with_capacity(names.len());
        let mut kinds = Vec::with_capacity(names.len());
        for n in names {
            let (kind, map) = match &n.key {
                FaceKey::Sheet { part: SheetPart::Flat, at } if n.feature == b.feature => {
                    let c = flat.contour.as_ref();
                    let m = c.zip(contour_frame).and_then(|(c, f)| {
                        c.straights.iter().find(|st| &st.line == at).map(|st| FlatMap::Straight(st, c, f))
                    });
                    (Kind::Primary, m)
                }
                FaceKey::Sheet { part: SheetPart::Flat, .. } => {
                    let m = flat.flange_ends.iter().find(|fe| fe.flange == n.feature).map(FlatMap::Flange);
                    (Kind::Primary, m)
                }
                FaceKey::Sheet { part: part @ (SheetPart::BendIn | SheetPart::BendOut), at }
                    if n.feature == b.feature =>
                {
                    let c = flat.contour.as_ref();
                    let zone = c.and_then(|c| c.bends.iter().find(|z| &z.line == at));
                    match (zone, c, contour_frame) {
                        (Some(z), Some(c), Some(f)) if z.outer == (*part == SheetPart::BendOut) => {
                            (Kind::Primary, Some(FlatMap::Bend(z, c, f)))
                        }
                        (Some(_), _, _) => (Kind::Back, None),
                        _ => (Kind::Other, None),
                    }
                }
                FaceKey::Sheet { part: part @ (SheetPart::BendIn | SheetPart::BendOut), .. }
                    if n.feature != b.feature =>
                {
                    match flat.flange_ends.iter().find(|fe| fe.flange == n.feature) {
                        Some(fe) if *part == SheetPart::BendOut => (Kind::Primary, Some(FlatMap::FlangeBend(&fe.bend))),
                        Some(_) => (Kind::Back, None),
                        None => (Kind::Other, None),
                    }
                }
                FaceKey::Sheet { part: SheetPart::FlatBack, .. } => (Kind::Back, None),
                FaceKey::Sheet { .. } => (Kind::Other, None),
                FaceKey::Cap { side: CapSide::Start, .. } if n.feature == b.feature && flat.base.is_some() => {
                    (Kind::Primary, tab_frame.map(FlatMap::Tab))
                }
                FaceKey::Cap { side: CapSide::End, .. } if n.feature == b.feature && flat.base.is_some() => {
                    (Kind::Back, None)
                }
                // Any other face of the sheet's own features (a flange's end caps, the
                // tab's sides): part of the sheet, not of the cut.
                _ if n.feature == b.feature || flat.flange_ends.iter().any(|fe| fe.flange == n.feature) => {
                    (Kind::Other, None)
                }
                // Only the cutting feature's own faces are new: earlier cuts are in the
                // pattern already (dogfooding F7-1, F7-2).
                _ if n.feature == feature => (Kind::New, None),
                _ => (Kind::Other, None),
            };
            kinds.push(kind);
            maps.push(map);
        }
        if !kinds.contains(&Kind::New) {
            return None;
        }
        // Every new face is a wall through the sheet: it meets a flat and its back, and no
        // other sheet face.
        let mut meets = vec![(false, false); kinds.len()];
        for e in &b.topology.edges {
            let fs: Vec<usize> = e.faces.iter().map(|f| *f as usize).collect();
            for (i, j) in [(0, 1), (1, 0)] {
                let (Some(&a), Some(&c)) = (fs.get(i), fs.get(j)) else { continue };
                if kinds.get(a) != Some(&Kind::New) {
                    continue;
                }
                match kinds.get(c) {
                    Some(Kind::Primary) => meets.get_mut(a).map(|m| m.0 = true).unwrap_or(()),
                    Some(Kind::Back) => meets.get_mut(a).map(|m| m.1 = true).unwrap_or(()),
                    Some(Kind::Other) => return None,
                    _ => {}
                }
            }
        }
        // A wall that meets only the back is a blind sliver where the cut ends inside the
        // sheet's thickness (a hole across a bend and its wall, stopped at the bend's start,
        // leaves one at the bend's thin edge, F8-7): it is not in the pattern, which is cut
        // where walls reach both sides.
        let through = |m: &(bool, bool)| m.0 && m.1;
        let sliver = |m: &(bool, bool)| !m.0 && m.1;
        let has_through = kinds.iter().zip(&meets).any(|(k, m)| *k == Kind::New && through(m));
        if kinds.iter().zip(&meets).any(|(k, m)| *k == Kind::New && !through(m) && !(has_through && sliver(m))) {
            return None;
        }
        // The loops: the edges between new faces and the flats on the sketch side, mapped.
        // Bend faces are mapped at the neutral surface along the cut direction; if that does
        // not close the loops (a cut slanted at a bend's edge), the sketch-side face is used.
        let mesh = self.kernel.tessellate(&b.shape, &gusset_kernel::TessellationParams::default()).ok()?;
        let tol = (flat.thickness * 1e-3).max(1e-4);
        let attempts: &[Option<gusset_kernel::V3>] = if dir.is_some() { &[dir, None] } else { &[None] };
        let mut found = None;
        'attempt: for dir in attempts {
            let mut pieces = Vec::new();
            let mut cuts: Vec<Cut> = Vec::new();
            for (ei, e) in b.topology.edges.iter().enumerate() {
                let fs: Vec<usize> = e.faces.iter().map(|f| *f as usize).collect();
                let primary = fs.iter().find(|f| kinds.get(**f) == Some(&Kind::Primary));
                let new = fs.iter().any(|f| kinds.get(*f) == Some(&Kind::New));
                let (Some(pf), true) = (primary, new) else { continue };
                let map = maps.get(*pf)?.as_ref()?;
                let poly = mesh.edges.get(ei)?;
                let mut pts = Vec::with_capacity(poly.len());
                for q in poly {
                    let (p2, off) = map.map(q.map(f64::from), *dir);
                    if off > tol * 10.0 {
                        return None; // the sheet moved: not a cut of it
                    }
                    pts.push(p2);
                }
                // A full circle on a flat face is carried as a circle.
                let circle = match (&e.circle, e.kind == gusset_kernel::CurveKind::Circle && e.vertices.len() == 1) {
                    (Some(c), true) if map.is_flat() => Some((map.map(c.center, None).0, c.radius)),
                    _ => None,
                };
                match circle {
                    Some(c) if pts.len() >= 4 => {
                        if pts.first().zip(pts.last()).is_some_and(|(a, z)| len(sub(*a, *z)) <= tol * 10.0) {
                            pts.pop();
                        }
                        cuts.push(Cut { outline: pts, circle: Some(c) });
                    }
                    _ => pieces.push(pts),
                }
            }
            if !pieces.is_empty() {
                let Some(loops) = chain_loops(pieces, tol * 10.0) else { continue 'attempt };
                cuts.extend(loops.into_iter().map(|outline| Cut { outline, circle: None }));
            }
            found = Some(cuts);
            break;
        }
        let mut cuts = found?;
        if cuts.is_empty() {
            return None;
        }
        // An earlier cut's outlines are found again (its faces are not the sheet's): keep
        // only those not already in the pattern.
        let key = |o: &[[f64; 2]]| {
            let n = o.len().max(1) as f64;
            let c = o.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
            let area = (0..o.len())
                .map(|i| {
                    let (p, q) =
                        (o.get(i).copied().unwrap_or_default(), o.get((i + 1) % o.len()).copied().unwrap_or_default());
                    p[0] * q[1] - q[0] * p[1]
                })
                .sum::<f64>()
                .abs()
                / 2.0;
            (c, area)
        };
        cuts.retain(|l| {
            let (c, a) = key(&l.outline);
            !flat.outlines.iter().skip(1).any(|o| {
                let (c2, a2) = key(o);
                len(sub(c, c2)) < tol * 10.0 && (a - a2).abs() <= 1e-3 * a.max(a2).max(1e-9)
            })
        });
        // Each cut-out lies inside the pattern.
        if cuts.iter().any(|l| {
            l.outline.iter().any(|p| !inside(&flat.outlines, *p) && !on_outline(&flat.outlines, *p, tol * 10.0))
        }) {
            return None;
        }
        Some(cuts)
    }
}

/// Whether `p` lies on one of the outlines (within `tol`).
fn on_outline(outlines: &[Vec<[f64; 2]>], p: P2, tol: f64) -> bool {
    outlines.iter().any(|o| {
        let n = o.len();
        (0..n).any(|i| {
            let (Some(a), Some(b)) = (o.get(i), o.get((i + 1) % n)) else { return false };
            let ab = sub(*b, *a);
            let l2 = ab[0] * ab[0] + ab[1] * ab[1];
            if l2 <= 0.0 {
                return len(sub(p, *a)) <= tol;
            }
            let u = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0);
            len(sub(p, addv(*a, mul(ab, u)))) <= tol
        })
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn an_l_bracket_develops_to_its_straights_plus_one_allowance() {
        // 50 x 30 L, material inside the corner (left of travel turning left).
        let pts = [[0.0, 0.0], [50.0, 0.0], [50.0, 30.0]];
        let ids = ["l1".to_string(), "l2".to_string()];
        let sec = contour_section(&pts, &ids, 2.0, 1.0, 0.5, 1.0, 40.0).unwrap_or_else(|e| panic!("{e}"));
        // Inside the turn: side A radius r + t = 3, set back 3 on each line.
        let ba = std::f64::consts::FRAC_PI_2 * (1.0 + 0.5 * 2.0);
        let b = sec.flat.bends.first().unwrap_or_else(|| panic!("a bend"));
        assert!((b.allowance - ba).abs() < 1e-12);
        assert!((b.start[0][0] - 47.0).abs() < 1e-12);
        let size = sec.flat.size();
        assert!((size[0] - (47.0 + 27.0 + ba)).abs() < 1e-9, "{size:?}");
        assert!((size[1] - 40.0).abs() < 1e-12);
        // Cross-section area = t x neutral length at k = 0.5.
        let area = loop_area(&sec.edges).abs();
        assert!((area - 2.0 * (47.0 + 27.0 + ba)).abs() < 1e-9, "{area}");
        // Flipped to the outside of the turn: side A is now the inside (radius r).
        let out = contour_section(&pts, &ids, 2.0, 1.0, 0.5, -1.0, 40.0).unwrap_or_else(|e| panic!("{e}"));
        assert!((out.flat.bends.first().map_or(0.0, |b| b.start[0][0]) - 49.0).abs() < 1e-12);
        assert!((loop_area(&out.edges).abs() - 2.0 * (49.0 + 29.0 + ba)).abs() < 1e-9);
    }

    #[test]
    fn short_lines_and_straight_corners_are_refused() {
        let ids = ["l1".to_string(), "l2".to_string()];
        let e = contour_section(&[[0.0, 0.0], [2.0, 0.0], [2.0, 30.0]], &ids, 2.0, 1.0, 0.44, 1.0, 10.0);
        assert!(e.is_err_and(|m| m.contains("too short")));
        let e = contour_section(&[[0.0, 0.0], [20.0, 0.0], [40.0, 0.0]], &ids, 2.0, 1.0, 0.44, 1.0, 10.0);
        assert!(e.is_err_and(|m| m.contains("in line")));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod eval_tests {
    use crate::Evaluator;
    use gusset_doc::Document;
    use gusset_kernel::mock::MockKernel;
    use std::sync::Arc;

    const SRC: &str = "gusset 1\nunits mm\n\nsketch S (plane: XY, next_id: 6) {\n  p1 = point(0, 0)\n  p2 = point(50, 0)\n  p3 = point(50, 30)\n  l4 = line(p1, p2)\n  l5 = line(p2, p3)\n}\n\n";

    #[test]
    fn a_contour_flange_names_its_faces_and_keeps_a_flat_pattern() {
        let src = format!(
            "{SRC}contour_flange Base (curves: S.curves(l4), width: 40 mm, thickness: 2 mm, bend_radius: 1 mm, k_factor: 0.5)\n"
        );
        let doc = Document::from_text(&src).unwrap();
        assert_eq!(doc.to_text(), src);
        let mut ev = Evaluator::new(Arc::new(MockKernel));
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        let b = r.body("Base").unwrap();
        let names: Vec<String> = b.names.faces.iter().map(ToString::to_string).collect();
        // The mock kernel has no arcs: one line, no bends (bends are tested with OCCT).
        for want in [
            "Base/flat(l4)",
            "Base/flat_back(l4)",
            "Base/start(l4)",
            "Base/end(l4)",
            "Base/cap(start)",
            "Base/cap(end)",
        ] {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
        let flat = b.sheet.as_ref().unwrap();
        assert!((flat.size()[0] - 50.0).abs() < 1e-9 && flat.bends.is_empty());
        let v = ev.kernel().mass_properties(&b.shape).unwrap().volume;
        assert!((v - 40.0 * 2.0 * 50.0).abs() < 1e-6 * v, "{v}");
        let bb = ev.kernel().bbox(&b.shape).unwrap();
        assert!((bb.max[1] - 2.0).abs() < 1e-9, "material left of the line (+y)");
        // A closed contour or an arc is refused.
        let closed = src
            .replace("next_id: 6", "next_id: 7")
            .replace("  l5 = line(p2, p3)\n", "  l5 = line(p2, p3)\n  l6 = line(p3, p1)\n")
            .replace("S.curves(l4)", "S.curves(l4, l5, l6)");
        let d = Document::from_text(&closed).unwrap();
        let errs = ev.evaluate(&d, None, &());
        assert!(errs.errors().iter().any(|e| e.message.contains("closed")), "{:?}", errs.errors());
    }

    #[test]
    fn edge_flanges_round_trip_and_need_sheet_metal() {
        let src = format!(
            "{SRC}contour_flange Base (curves: S.curves(l4), width: 40 mm, thickness: 2 mm, bend_radius: 1 mm, k_factor: 0.5)\n\n\
             edge_flange E (body: Base, line: l4, length: 10 mm, angle: 90 deg, bend_radius: 2 mm, k_factor: 0.44, flip: true)\n"
        );
        // A contour flange takes a side of one of its lines; a hole feature is no sheet.
        assert!(Document::from_text(&src).is_ok(), "{src}");
        let e = Document::from_text(&src.replace("body: Base", "body: S")).unwrap_err().to_string();
        assert!(e.contains("not a tab or a contour flange"), "{e}");
        let missing = src.replace(", length: 10 mm", "");
        assert!(Document::from_text(&missing).unwrap_err().to_string().contains("length"));
    }

    #[test]
    fn a_tab_is_a_flat_plate_with_its_outline() {
        let src = "gusset 1\nunits mm\n\nsketch S (plane: XY, next_id: 9) {\n  p1 = point(-10, 0)\n  p2 = point(10, 0)\n  p3 = point(10, 5)\n  p4 = point(-10, 5)\n  l5 = line(p1, p2)\n  l6 = line(p2, p3)\n  l7 = line(p3, p4)\n  l8 = line(p4, p1)\n}\n\ntab Plate (profile: S, thickness: 1.5 mm, flip: true)\n";
        let doc = Document::from_text(src).unwrap();
        let mut ev = Evaluator::new(Arc::new(MockKernel));
        let r = ev.evaluate(&doc, None, &());
        assert!(r.errors().is_empty(), "{:?}", r.errors());
        let flat = r.body("Plate").unwrap().sheet.as_ref().unwrap();
        assert!(flat.bends.is_empty());
        assert!((flat.size()[0] - 20.0).abs() < 1e-9);
        let bb = ev.kernel().bbox(&r.body("Plate").unwrap().shape).unwrap();
        assert!((bb.min[2] + 1.5).abs() < 1e-9, "flip puts the plate below the sketch");
    }
}
