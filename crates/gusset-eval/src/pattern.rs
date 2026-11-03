//! Sketch patterns (ADR 0016 §4): linear and circular copies of sketch curves.
//!
//! A pattern is a sketch item that names source curves, a count (an expression) and a
//! transform. The copies are *derived* after the sketch is solved: copy `j` of entity
//! `l5` is `l5_j`, with its own derived points `p3_j`, positioned from the solved source
//! points and the pattern's parameters. They take part in regions and extrusions like
//! ordinary curves but are not solver variables, so changing the count or the spacing is
//! an ordinary parameter edit.

use crate::error::{ErrorCode, EvalError};
use gusset_doc::{SkGeom, SkPattern, SkPatternKind, SketchDef};
use gusset_units::params::ParamValues;
use gusset_units::{Dim, LengthUnit};
use std::collections::{BTreeMap, BTreeSet};

/// Most copies of one source curve a pattern may make (the count includes the original).
pub(crate) const MAX_SKETCH_INSTANCES: u32 = 500;
/// Most derived curves all patterns of one sketch may make together.
pub(crate) const MAX_DERIVED_CURVES: usize = 20_000;

/// One derived curve.
#[derive(Debug, Clone)]
pub(crate) struct DerivedCurve {
    pub(crate) id: String,
    /// References derived points (`p3_2`) by id.
    pub(crate) geom: SkGeom,
    pub(crate) construction: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Derived {
    pub(crate) points: Vec<(String, [f64; 2])>,
    pub(crate) curves: Vec<DerivedCurve>,
}

/// Id of copy `j` of `id`.
pub(crate) fn copy_id(id: &str, j: u32) -> String {
    format!("{id}_{j}")
}

fn rotate(p: [f64; 2], c: [f64; 2], a: f64) -> [f64; 2] {
    let (s, co) = (libm::sin(a), libm::cos(a));
    let (x, y) = (p[0] - c[0], p[1] - c[1]);
    [c[0] + co * x - s * y, c[1] + s * x + co * y]
}

/// The transform of one copy.
#[derive(Clone, Copy)]
enum Step {
    Shift([f64; 2]),
    Turn { center: [f64; 2], angle: f64 },
}

impl Step {
    fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        match self {
            Step::Shift(d) => [p[0] + d[0], p[1] + d[1]],
            Step::Turn { center, angle } => rotate(p, *center, *angle),
        }
    }
}

/// A whole-number count in `2..=max`.
fn whole_count(
    name: &str,
    pat: &SkPattern,
    e: &gusset_units::expr::Expr,
    max: u32,
    params: &ParamValues,
    units: LengthUnit,
) -> Result<u32, EvalError> {
    let v = crate::eval_expr(name, e, Dim::NONE, params, units)?;
    if !v.is_finite() || (v - v.round()).abs() > 1e-6 {
        return Err(sketch_err(name, pat, format!("the count '{e}' is {v}, not a whole number"))
            .with_hint("the count of copies must be a whole number"));
    }
    let n = v.round();
    if !(2.0..=f64::from(max)).contains(&n) {
        return Err(sketch_err(name, pat, format!("a count must be between 2 and {max}, not {n}"))
            .with_hint("the count includes the original"));
    }
    Ok(n as u32)
}

fn sketch_err(name: &str, pat: &SkPattern, msg: String) -> EvalError {
    EvalError::new(name, ErrorCode::InvalidSketch, format!("pattern '{}': {msg}", pat.id))
}

/// Evaluates every pattern of `def` against the solved geometry.
pub(crate) fn derive(
    name: &str,
    def: &SketchDef,
    params: &ParamValues,
    units: LengthUnit,
    points: &BTreeMap<String, [f64; 2]>,
    radii: &BTreeMap<String, f64>,
) -> Result<Derived, EvalError> {
    let mut out = Derived::default();
    let mut have: BTreeSet<String> = BTreeSet::new();
    for pat in &def.patterns {
        let count = pattern_count(name, pat, params, units)?;
        // (copy number, transform) for every copy but the original.
        let copies: Vec<(u32, Step)> = match &pat.kind {
            SkPatternKind::Linear { dx, dy, second } => {
                let spacing = |dx: &gusset_units::expr::Expr,
                               dy: &gusset_units::expr::Expr|
                 -> Result<[f64; 2], EvalError> {
                    let d = [
                        crate::eval_expr(name, dx, Dim::LENGTH, params, units)?,
                        crate::eval_expr(name, dy, Dim::LENGTH, params, units)?,
                    ];
                    if libm::hypot(d[0], d[1]) < 1e-9 {
                        return Err(sketch_err(name, pat, "the spacing is zero, so the copies would overlap".into())
                            .with_hint("give dx and/or dy a non-zero length"));
                    }
                    Ok(d)
                };
                let d1 = spacing(dx, dy)?;
                match second {
                    None => {
                        (1..count).map(|j| (j, Step::Shift([f64::from(j) * d1[0], f64::from(j) * d1[1]]))).collect()
                    }
                    Some(g) => {
                        let grid = gusset_doc::MAX_GRID_COUNT;
                        if count > grid {
                            return Err(sketch_err(
                                name,
                                pat,
                                format!("a grid has at most {grid} copies per direction, not {count}"),
                            ));
                        }
                        let m = whole_count(name, pat, &g.count, grid, params, units)?;
                        if count * m > MAX_SKETCH_INSTANCES {
                            return Err(sketch_err(
                                name,
                                pat,
                                format!("the grid makes {} copies, more than {MAX_SKETCH_INSTANCES}", count * m),
                            )
                            .with_hint("lower the counts"));
                        }
                        let d2 = spacing(&g.dx, &g.dy)?;
                        let mut v = Vec::new();
                        for j2 in 0..m {
                            for j1 in 0..count {
                                if j1 == 0 && j2 == 0 {
                                    continue;
                                }
                                let (a, b) = (f64::from(j1), f64::from(j2));
                                v.push((j1 + 64 * j2, Step::Shift([a * d1[0] + b * d2[0], a * d1[1] + b * d2[1]])));
                            }
                        }
                        v
                    }
                }
            }
            SkPatternKind::Circular { center, angle } => {
                let center = points
                    .get(center)
                    .copied()
                    .ok_or_else(|| sketch_err(name, pat, format!("centre '{center}' is not a point of this sketch")))?;
                let total = crate::eval_expr(name, angle, Dim::ANGLE, params, units)?;
                if total.abs() < 1e-9 {
                    return Err(sketch_err(name, pat, "the angle is zero".into())
                        .with_hint("enter the total angle, for example 360 deg"));
                }
                // A full turn spaces copies evenly round the circle; a partial angle spans
                // from the first copy to the last (like the body pattern).
                let full = total.abs() >= std::f64::consts::TAU - 1e-9;
                let angle = if full { total / f64::from(count) } else { total / f64::from(count - 1) };
                (1..count).map(|j| (j, Step::Turn { center, angle: f64::from(j) * angle })).collect()
            }
        };
        for &(j, step) in &copies {
            for src in &pat.of {
                let Some(e) = def.entities.iter().find(|e| &e.id == src) else {
                    return Err(sketch_err(name, pat, format!("'{src}' is not an entity of this sketch")));
                };
                let mut point = |p: &str| -> Result<String, EvalError> {
                    let id = copy_id(p, j);
                    // Curves of one copy share their points; so do patterns using one source point.
                    if !have.contains(&id) {
                        let pos = points
                            .get(p)
                            .copied()
                            .ok_or_else(|| sketch_err(name, pat, format!("'{src}' refers to unknown point '{p}'")))?;
                        out.points.push((id.clone(), step.apply(pos)));
                        have.insert(id.clone());
                    }
                    Ok(id)
                };
                let geom = match &e.geom {
                    SkGeom::Point { .. } => {
                        return Err(sketch_err(
                            name,
                            pat,
                            format!("'{src}' is a point; patterns copy lines, circles, arcs and splines"),
                        ));
                    }
                    SkGeom::Line { a, b } => SkGeom::Line { a: point(a)?, b: point(b)? },
                    SkGeom::Circle { center, .. } => {
                        SkGeom::Circle { center: point(center)?, radius: radii.get(src).copied().unwrap_or(0.0) }
                    }
                    SkGeom::Arc { center, start, end } => {
                        SkGeom::Arc { center: point(center)?, start: point(start)?, end: point(end)? }
                    }
                    SkGeom::Spline { points: ps, closed } => {
                        let v = ps.iter().map(|p| point(p)).collect::<Result<Vec<_>, _>>()?;
                        SkGeom::Spline { points: v, closed: *closed }
                    }
                };
                out.curves.push(DerivedCurve { id: copy_id(src, j), geom, construction: e.construction });
                if out.curves.len() > MAX_DERIVED_CURVES {
                    return Err(sketch_err(
                        name,
                        pat,
                        format!("the patterns make more than {MAX_DERIVED_CURVES} curves"),
                    )
                    .with_hint("lower the counts"));
                }
            }
        }
    }
    Ok(out)
}

fn pattern_count(name: &str, pat: &SkPattern, params: &ParamValues, units: LengthUnit) -> Result<u32, EvalError> {
    let v = crate::eval_expr(name, &pat.count, Dim::NONE, params, units)?;
    if !v.is_finite() || (v - v.round()).abs() > 1e-6 {
        return Err(sketch_err(name, pat, format!("the count '{}' is {v}, not a whole number", pat.count))
            .with_hint("the count of copies must be a whole number"));
    }
    let n = v.round();
    if !(2.0..=f64::from(MAX_SKETCH_INSTANCES)).contains(&n) {
        return Err(sketch_err(
            name,
            pat,
            format!("a pattern needs between 2 and {MAX_SKETCH_INSTANCES} copies, not {n}"),
        )
        .with_hint("the count includes the original"));
    }
    Ok(n as u32)
}
