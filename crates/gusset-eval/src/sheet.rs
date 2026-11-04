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
    /// The bends: a contour flange's in order along its developed length (`x`), then
    /// those of edge flanges in feature order.
    pub bends: Vec<FlatBend>,
    /// For a tab: where its outline lies, so edge flanges can be bent off its lines.
    pub base: Option<TabBase>,
    /// For a contour flange: its straights, so edge flanges can be bent off their sides.
    pub contour: Option<ContourBase>,
    /// Features other than sheet-metal ones that changed the body since (cuts, holes,
    /// moves...): the pattern does not show what they did.
    pub changed_by: Vec<String>,
}

/// The sketch a tab was made from and the side its material is on.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBase {
    pub sketch: String,
    pub flip: bool,
}

/// Where a contour flange's straights lie: in its sketch, across `z` along the sketch
/// normal (the flat pattern's `y = z - z[0]`).
#[derive(Debug, Clone, PartialEq)]
pub struct ContourBase {
    pub sketch: String,
    pub straights: Vec<Straight>,
    pub z: [f64; 2],
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
        bends,
        base: None,
        // The sketch and the width's span are filled in by the caller.
        contour: Some(ContourBase { sketch: String::new(), straights, z: [0.0, w] }),
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
        if let SheetOp::EdgeFlange { body, line, length, angle, bend_radius, k_factor } = &d.op {
            let v = |e: &Expr, dim| eval_expr(name, e, dim, params, units);
            let spec = EdgeSpec {
                length: v(length, Dim::LENGTH)?,
                angle: v(angle, Dim::ANGLE)?,
                r: v(bend_radius, Dim::LENGTH)?,
                k: v(k_factor, Dim::NONE)?,
                down: d.flip,
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
                for r in &kp.regions {
                    outlines.push(sample(&r.outer));
                    outlines.extend(r.holes.iter().map(sample));
                }
                let base = Some(TabBase { sketch: profile.sketch.clone(), flip: d.flip });
                (
                    res,
                    None,
                    FlatPattern {
                        thickness: t,
                        outlines,
                        bends: Vec::new(),
                        base,
                        contour: None,
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

/// The evaluated values of an edge flange.
pub(crate) struct EdgeSpec {
    pub(crate) length: f64,
    pub(crate) angle: f64,
    pub(crate) r: f64,
    pub(crate) k: f64,
    pub(crate) down: bool,
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
        let site = match (&flat.base, &flat.contour) {
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
