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

/// The transform of copy `j` of one pattern.
enum Step {
    Shift([f64; 2]),
    Turn { center: [f64; 2], angle: f64 },
}

impl Step {
    fn apply(&self, p: [f64; 2], j: u32) -> [f64; 2] {
        let k = f64::from(j);
        match self {
            Step::Shift(d) => [p[0] + k * d[0], p[1] + k * d[1]],
            Step::Turn { center, angle } => rotate(p, *center, k * angle),
        }
    }
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
        let step = match &pat.kind {
            SkPatternKind::Linear { dx, dy } => {
                let dx = crate::eval_expr(name, dx, Dim::LENGTH, params, units)?;
                let dy = crate::eval_expr(name, dy, Dim::LENGTH, params, units)?;
                if libm::hypot(dx, dy) < 1e-9 {
                    return Err(sketch_err(name, pat, "the spacing is zero, so the copies would overlap".into())
                        .with_hint("give dx and/or dy a non-zero length"));
                }
                Step::Shift([dx, dy])
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
                Step::Turn { center, angle }
            }
        };
        for j in 1..count {
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
                        out.points.push((id.clone(), step.apply(pos, j)));
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
