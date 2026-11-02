//! OCCT's package `Geom2dGridEval`: the grid evaluators of the 2D curves (`Geom2dGridEval_Line`, `_Circle`, `_Ellipse`,
//! `_Hyperbola`, `_Parabola`, `_BezierCurve`, `_BSplineCurve`, `_OffsetCurve`, `_OtherCurve`) and the dispatcher
//! `Geom2dGridEval_Curve` that picks the evaluator of a curve (a `std::variant` in C++, the private enum `Evaluator` here).
//!
//! Translated from `Geom2dGridEval*.hxx/.cxx` (OCCT 8.0.1). Every evaluator implements [`GridEvaluator`]; the parameter array
//! `NCollection_Array1<double>` is a slice (its lower bound does not matter) and the 1-based result arrays are `Vec`s.
// The comparisons `x == y` and the exact formulas are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::adaptor2d::{AdaptorError, Curve2dAdaptor, GeomAbsCurveType, Result};
use crate::bsplclib::BSplError;
use crate::bsplclib::curve2d;
use crate::bsplclib::flat::locate_parameter_flat;
use crate::bsplclib_cache::Cache;
use crate::geom2d::offset::{self, Derivatives};
use crate::geom2d::{self, Curve2d, Geom2dError, TrimmedCurve};
use crate::gp::{self, Circ2d, Dir2d, Elips2d, Hypr2d, Lin2d, Parab2d, Pnt2d, Vec2d};
use std::sync::Arc;

/// `Geom2dGridEval::CurveD1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveD1 {
    /// The point.
    pub point: Pnt2d,
    /// The first derivative.
    pub d1: Vec2d,
}

/// `Geom2dGridEval::CurveD2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveD2 {
    /// The point.
    pub point: Pnt2d,
    /// The first derivative.
    pub d1: Vec2d,
    /// The second derivative.
    pub d2: Vec2d,
}

/// `Geom2dGridEval::CurveD3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveD3 {
    /// The point.
    pub point: Pnt2d,
    /// The first derivative.
    pub d1: Vec2d,
    /// The second derivative.
    pub d2: Vec2d,
    /// The third derivative.
    pub d3: Vec2d,
}

/// The five `Evaluate*` methods every grid evaluator has. An empty vector is the empty `NCollection_Array1` (null
/// geometry, no parameters, `n < 1` for `evaluate_grid_dn`).
pub trait GridEvaluator {
    /// `EvaluateGrid`.
    fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>>;
    /// `EvaluateGridD1`.
    fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>>;
    /// `EvaluateGridD2`.
    fn evaluate_grid_d2(&self, params: &[f64]) -> Result<Vec<CurveD2>>;
    /// `EvaluateGridD3`.
    fn evaluate_grid_d3(&self, params: &[f64]) -> Result<Vec<CurveD3>>;
    /// `EvaluateGridDN`: the derivative of order `n`.
    fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>>;
}

fn zero() -> Vec2d {
    Vec2d::new(0.0, 0.0)
}

fn bspl(e: BSplError) -> AdaptorError {
    AdaptorError::Geom(Geom2dError::from(e))
}

// ---------------------------------------------------------------------------------------------
// Elementary curves
// ---------------------------------------------------------------------------------------------

/// The point-by-point evaluation of an elementary curve: the grid methods are the same loops for all of them.
trait Pointwise {
    fn present(&self) -> bool;
    /// The point and the derivatives of order 1..3 at `u`.
    fn all(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)>;
    /// The derivative of order `n >= 1` at `u`.
    fn nth(&self, u: f64, n: i32) -> Result<Vec2d>;
}

macro_rules! grid_pointwise {
    ($t:ty) => {
        impl GridEvaluator for $t {
            fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>> {
                if !self.present() || params.is_empty() {
                    return Ok(Vec::new());
                }
                params.iter().map(|&u| Ok(self.all(u)?.0)).collect()
            }
            fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>> {
                if !self.present() || params.is_empty() {
                    return Ok(Vec::new());
                }
                params
                    .iter()
                    .map(|&u| {
                        let (point, d1, _, _) = self.all(u)?;
                        Ok(CurveD1 { point, d1 })
                    })
                    .collect()
            }
            fn evaluate_grid_d2(&self, params: &[f64]) -> Result<Vec<CurveD2>> {
                if !self.present() || params.is_empty() {
                    return Ok(Vec::new());
                }
                params
                    .iter()
                    .map(|&u| {
                        let (point, d1, d2, _) = self.all(u)?;
                        Ok(CurveD2 { point, d1, d2 })
                    })
                    .collect()
            }
            fn evaluate_grid_d3(&self, params: &[f64]) -> Result<Vec<CurveD3>> {
                if !self.present() || params.is_empty() {
                    return Ok(Vec::new());
                }
                params
                    .iter()
                    .map(|&u| {
                        let (point, d1, d2, d3) = self.all(u)?;
                        Ok(CurveD3 { point, d1, d2, d3 })
                    })
                    .collect()
            }
            fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>> {
                if !self.present() || params.is_empty() || n < 1 {
                    return Ok(Vec::new());
                }
                params.iter().map(|&u| self.nth(u, n)).collect()
            }
        }
    };
}

/// `Geom2dGridEval_Line`.
#[derive(Debug, Clone)]
pub struct Line {
    geom: Option<geom2d::Line>,
}

impl Line {
    /// The evaluator of a line (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::Line>) -> Self {
        Self { geom: geom.cloned() }
    }
}

impl Pointwise for Line {
    fn present(&self) -> bool {
        self.geom.is_some()
    }
    fn all(&self, t: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let lin: Lin2d = self.geom.as_ref().ok_or(AdaptorError::NullObject)?.lin2d();
        let (loc_x, loc_y) = (lin.location().x(), lin.location().y());
        let (dir_x, dir_y) = (lin.direction().x(), lin.direction().y());
        Ok((Pnt2d::new(loc_x + t * dir_x, loc_y + t * dir_y), Vec2d::new(dir_x, dir_y), zero(), zero()))
    }
    fn nth(&self, _u: f64, n: i32) -> Result<Vec2d> {
        let lin: Lin2d = self.geom.as_ref().ok_or(AdaptorError::NullObject)?.lin2d();
        Ok(if n == 1 { Vec2d::new(lin.direction().x(), lin.direction().y()) } else { zero() })
    }
}
grid_pointwise!(Line);

/// `Geom2dGridEval_Circle`.
#[derive(Debug, Clone)]
pub struct Circle {
    geom: Option<geom2d::Circle>,
}

impl Circle {
    /// The evaluator of a circle (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::Circle>) -> Self {
        Self { geom: geom.cloned() }
    }
    fn circ(&self) -> Result<Circ2d> {
        Ok(self.geom.as_ref().ok_or(AdaptorError::NullObject)?.circ2d())
    }
}

impl Pointwise for Circle {
    fn present(&self) -> bool {
        self.geom.is_some()
    }
    fn all(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let circ = self.circ()?;
        let (cx, cy) = (circ.location().x(), circ.location().y());
        let (xx, xy) = (circ.position().x_direction().x(), circ.position().x_direction().y());
        let (yx, yy) = (circ.position().y_direction().x(), circ.position().y_direction().y());
        let radius = circ.radius();
        let cos_u = libm::cos(u);
        let sin_u = libm::sin(u);
        Ok((
            Pnt2d::new(cx + radius * (cos_u * xx + sin_u * yx), cy + radius * (cos_u * xy + sin_u * yy)),
            Vec2d::new(radius * (-sin_u * xx + cos_u * yx), radius * (-sin_u * xy + cos_u * yy)),
            Vec2d::new(radius * (-cos_u * xx - sin_u * yx), radius * (-cos_u * xy - sin_u * yy)),
            Vec2d::new(radius * (sin_u * xx - cos_u * yx), radius * (sin_u * xy - cos_u * yy)),
        ))
    }
    fn nth(&self, u: f64, n: i32) -> Result<Vec2d> {
        let circ = self.circ()?;
        let (xx, xy) = (circ.position().x_direction().x(), circ.position().x_direction().y());
        let (yx, yy) = (circ.position().y_direction().x(), circ.position().y_direction().y());
        let radius = circ.radius();
        let cos_u = libm::cos(u);
        let sin_u = libm::sin(u);
        // the derivatives are cyclic with period 4
        let (cx, cy) = match (n - 1) % 4 {
            0 => (-sin_u, cos_u),
            1 => (-cos_u, -sin_u),
            2 => (sin_u, -cos_u),
            _ => (cos_u, sin_u),
        };
        Ok(Vec2d::new(radius * (cx * xx + cy * yx), radius * (cx * xy + cy * yy)))
    }
}
grid_pointwise!(Circle);

/// `Geom2dGridEval_Ellipse`.
#[derive(Debug, Clone)]
pub struct Ellipse {
    geom: Option<geom2d::Ellipse>,
}

impl Ellipse {
    /// The evaluator of an ellipse (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::Ellipse>) -> Self {
        Self { geom: geom.cloned() }
    }
    fn elips(&self) -> Result<Elips2d> {
        Ok(self.geom.as_ref().ok_or(AdaptorError::NullObject)?.elips2d())
    }
}

impl Pointwise for Ellipse {
    fn present(&self) -> bool {
        self.geom.is_some()
    }
    fn all(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let e = self.elips()?;
        let (cx, cy) = (e.location().x(), e.location().y());
        let (x_dir, y_dir) = (*e.x_axis().direction(), *e.y_axis().direction());
        let (xx, xy, yx, yy) = (x_dir.x(), x_dir.y(), y_dir.x(), y_dir.y());
        let (maj, min) = (e.major_radius(), e.minor_radius());
        let cos_u = libm::cos(u);
        let sin_u = libm::sin(u);
        Ok((
            Pnt2d::new(cx + maj * cos_u * xx + min * sin_u * yx, cy + maj * cos_u * xy + min * sin_u * yy),
            Vec2d::new(-maj * sin_u * xx + min * cos_u * yx, -maj * sin_u * xy + min * cos_u * yy),
            Vec2d::new(-maj * cos_u * xx - min * sin_u * yx, -maj * cos_u * xy - min * sin_u * yy),
            Vec2d::new(maj * sin_u * xx - min * cos_u * yx, maj * sin_u * xy - min * cos_u * yy),
        ))
    }
    fn nth(&self, u: f64, n: i32) -> Result<Vec2d> {
        let e = self.elips()?;
        let (x_dir, y_dir) = (*e.x_axis().direction(), *e.y_axis().direction());
        let (xx, xy, yx, yy) = (x_dir.x(), x_dir.y(), y_dir.x(), y_dir.y());
        let (maj, min) = (e.major_radius(), e.minor_radius());
        let cos_u = libm::cos(u);
        let sin_u = libm::sin(u);
        let (c_maj, c_min) = match (n - 1) % 4 {
            0 => (-sin_u, cos_u),
            1 => (-cos_u, -sin_u),
            2 => (sin_u, -cos_u),
            _ => (cos_u, sin_u),
        };
        Ok(Vec2d::new(maj * c_maj * xx + min * c_min * yx, maj * c_maj * xy + min * c_min * yy))
    }
}
grid_pointwise!(Ellipse);

/// `Geom2dGridEval_Hyperbola`.
#[derive(Debug, Clone)]
pub struct Hyperbola {
    geom: Option<geom2d::Hyperbola>,
}

impl Hyperbola {
    /// The evaluator of a hyperbola (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::Hyperbola>) -> Self {
        Self { geom: geom.cloned() }
    }
    fn hypr(&self) -> Result<Hypr2d> {
        Ok(self.geom.as_ref().ok_or(AdaptorError::NullObject)?.hypr2d())
    }
}

impl Pointwise for Hyperbola {
    fn present(&self) -> bool {
        self.geom.is_some()
    }
    fn all(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let h = self.hypr()?;
        let (cx, cy) = (h.location().x(), h.location().y());
        let (x_dir, y_dir) = (*h.x_axis().direction(), *h.y_axis().direction());
        let (xx, xy, yx, yy) = (x_dir.x(), x_dir.y(), y_dir.x(), y_dir.y());
        let (maj, min) = (h.major_radius(), h.minor_radius());
        let cosh_u = libm::cosh(u);
        let sinh_u = libm::sinh(u);
        Ok((
            Pnt2d::new(cx + maj * cosh_u * xx + min * sinh_u * yx, cy + maj * cosh_u * xy + min * sinh_u * yy),
            Vec2d::new(maj * sinh_u * xx + min * cosh_u * yx, maj * sinh_u * xy + min * cosh_u * yy),
            Vec2d::new(maj * cosh_u * xx + min * sinh_u * yx, maj * cosh_u * xy + min * sinh_u * yy),
            Vec2d::new(maj * sinh_u * xx + min * cosh_u * yx, maj * sinh_u * xy + min * cosh_u * yy),
        ))
    }
    fn nth(&self, u: f64, n: i32) -> Result<Vec2d> {
        let h = self.hypr()?;
        let (x_dir, y_dir) = (*h.x_axis().direction(), *h.y_axis().direction());
        let (xx, xy, yx, yy) = (x_dir.x(), x_dir.y(), y_dir.x(), y_dir.y());
        let (maj, min) = (h.major_radius(), h.minor_radius());
        let cosh_u = libm::cosh(u);
        let sinh_u = libm::sinh(u);
        Ok(if n % 2 == 1 {
            Vec2d::new(maj * sinh_u * xx + min * cosh_u * yx, maj * sinh_u * xy + min * cosh_u * yy)
        } else {
            Vec2d::new(maj * cosh_u * xx + min * sinh_u * yx, maj * cosh_u * xy + min * sinh_u * yy)
        })
    }
}
grid_pointwise!(Hyperbola);

/// `Geom2dGridEval_Parabola`.
#[derive(Debug, Clone)]
pub struct Parabola {
    geom: Option<geom2d::Parabola>,
}

impl Parabola {
    /// The evaluator of a parabola (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::Parabola>) -> Self {
        Self { geom: geom.cloned() }
    }
    // the location, the coordinates of the X direction (the mirror axis) and of the Y direction, the focal distance
    fn frame(&self) -> Result<(f64, f64, f64, f64, f64, f64, f64)> {
        let p: Parab2d = self.geom.as_ref().ok_or(AdaptorError::NullObject)?.parab2d();
        let x_dir = *p.mirror_axis().direction();
        let y_dir = Dir2d::new(-x_dir.y(), x_dir.x())?;
        Ok((p.location().x(), p.location().y(), x_dir.x(), x_dir.y(), y_dir.x(), y_dir.y(), p.focal()))
    }
}

impl Pointwise for Parabola {
    fn present(&self) -> bool {
        self.geom.is_some()
    }
    fn all(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let (cx, cy, xx, xy, yx, yy, focal) = self.frame()?;
        let coeff = 1.0 / (4.0 * focal);
        let coeff2 = 1.0 / (2.0 * focal);
        let u2_term = u * u * coeff;
        let d1_term = u * coeff2;
        Ok((
            Pnt2d::new(cx + u2_term * xx + u * yx, cy + u2_term * xy + u * yy),
            Vec2d::new(d1_term * xx + yx, d1_term * xy + yy),
            Vec2d::new(coeff2 * xx, coeff2 * xy),
            Vec2d::new(0.0, 0.0),
        ))
    }
    fn nth(&self, u: f64, n: i32) -> Result<Vec2d> {
        let (_, _, xx, xy, yx, yy, focal) = self.frame()?;
        let coeff2 = 1.0 / (2.0 * focal);
        Ok(match n {
            1 => {
                let d1_term = u * coeff2;
                Vec2d::new(d1_term * xx + yx, d1_term * xy + yy)
            }
            2 => Vec2d::new(coeff2 * xx, coeff2 * xy),
            _ => zero(),
        })
    }
}
grid_pointwise!(Parabola);

// ---------------------------------------------------------------------------------------------
// Bezier and B-spline curves
// ---------------------------------------------------------------------------------------------

/// `Geom2dGridEval_BezierCurve`.
#[derive(Debug, Clone)]
pub struct BezierCurve {
    geom: Option<geom2d::BezierCurve>,
}

impl BezierCurve {
    /// The evaluator of a Bezier curve (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::BezierCurve>) -> Self {
        Self { geom: geom.cloned() }
    }

    // `CreateBezierCache2d`: the cache built at the parameter 0.5
    fn cache(g: &geom2d::BezierCurve) -> Result<Cache> {
        let flat = g.knot_sequence();
        let mut cache = Cache::new_2d(g.degree(), false, &flat, g.weights()).map_err(bspl)?;
        cache.build_cache_2d(0.5, &flat, g.poles(), g.weights()).map_err(bspl)?;
        Ok(cache)
    }

    // the common part of the four point grids: nothing for a null geometry or no parameters, else `f` at each parameter
    fn grid<T>(&self, params: &[f64], f: impl Fn(&Cache, f64) -> std::result::Result<T, BSplError>) -> Result<Vec<T>> {
        let Some(g) = self.geom.as_ref() else {
            return Ok(Vec::new());
        };
        if params.is_empty() {
            return Ok(Vec::new());
        }
        let cache = Self::cache(g)?;
        params.iter().map(|&u| f(&cache, u).map_err(bspl)).collect()
    }
}

impl GridEvaluator for BezierCurve {
    fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>> {
        self.grid(params, |c, u| c.d0_2d(u))
    }
    fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>> {
        self.grid(params, |c, u| c.d1_2d(u).map(|(point, d1)| CurveD1 { point, d1 }))
    }
    fn evaluate_grid_d2(&self, params: &[f64]) -> Result<Vec<CurveD2>> {
        self.grid(params, |c, u| c.d2_2d(u).map(|(point, d1, d2)| CurveD2 { point, d1, d2 }))
    }
    fn evaluate_grid_d3(&self, params: &[f64]) -> Result<Vec<CurveD3>> {
        self.grid(params, |c, u| c.d3_2d(u).map(|(point, d1, d2, d3)| CurveD3 { point, d1, d2, d3 }))
    }
    fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>> {
        let Some(g) = self.geom.as_ref() else {
            return Ok(Vec::new());
        };
        if params.is_empty() || n < 1 {
            return Ok(Vec::new());
        }
        let degree = g.degree();
        // the derivatives of an order above the degree are zero
        if n > degree {
            return Ok(vec![zero(); params.len()]);
        }
        let flat = g.knot_sequence();
        params
            .iter()
            .map(|&u| curve2d::dn(u, n, 0, degree, false, g.poles(), g.weights(), &flat, None).map_err(bspl))
            .collect()
    }
}

/// The data of a B-spline curve the evaluators need (`CurveData`).
struct SplineData<'a> {
    flat_knots: &'a [f64],
    poles: &'a [Pnt2d],
    weights: Option<&'a [f64]>,
    knots: &'a [f64],
    mults: &'a [i32],
    degree: i32,
    periodic: bool,
}

// the number of the span of the flat knots of `u` and the (periodically adjusted) parameter
fn locate_span(u: f64, d: &SplineData<'_>) -> Result<(i32, f64)> {
    let (span, adjusted) = locate_parameter_flat(d.degree, d.flat_knots, u, d.periodic, 0).map_err(bspl)?;
    Ok((span, adjusted))
}

// the number of the consecutive parameters from `start` that lie in the span `target` (at least 1)
fn count_span_size(params: &[f64], d: &SplineData<'_>, start: usize, target: i32) -> Result<usize> {
    let mut count = 1;
    for &u in params.iter().skip(start + 1) {
        if locate_span(u, d)?.0 == target {
            count += 1;
        } else {
            break;
        }
    }
    Ok(count)
}

// the cache is used for a span with at least this many consecutive parameters
const CACHE_THRESHOLD: usize = 4;

// `evaluateGridCached`
fn evaluate_grid_cached<T>(
    d: &SplineData<'_>,
    params: &[f64],
    cache_eval: impl Fn(&Cache, f64) -> std::result::Result<T, BSplError>,
    direct_eval: impl Fn(&SplineData<'_>, f64, i32) -> std::result::Result<T, BSplError>,
) -> Result<Vec<T>> {
    let mut results = Vec::with_capacity(params.len());
    let mut cache = Cache::new_2d(d.degree, d.periodic, d.flat_knots, d.weights).map_err(bspl)?;
    let mut prev_span = -1;
    let mut span_start = 0.0;
    let mut span_len = 1.0;
    let mut use_cache = false;
    for (i, &param) in params.iter().enumerate() {
        let (span, adj_param) = locate_span(param, d)?;
        // update the span information when the span changes
        if span != prev_span {
            prev_span = span;
            let span_size = count_span_size(params, d, i, span)?;
            let at = |k: i32| -> Result<f64> {
                usize::try_from(k - 1).ok().and_then(|k| d.flat_knots.get(k)).copied().ok_or(AdaptorError::OutOfRange)
            };
            span_start = at(span)?;
            span_len = at(span + 1)? - span_start;
            use_cache = span_size >= CACHE_THRESHOLD;
            if use_cache {
                cache.build_cache_2d(adj_param, d.flat_knots, d.poles, d.weights).map_err(bspl)?;
            }
        }
        results.push(if use_cache {
            // the curve cache uses the local parameter range [0, 1]
            let local = if span_len > 0.0 { (adj_param - span_start) / span_len } else { 0.0 };
            cache_eval(&cache, local).map_err(bspl)?
        } else {
            direct_eval(d, adj_param, span).map_err(bspl)?
        });
    }
    Ok(results)
}

/// `Geom2dGridEval_BSplineCurve`.
#[derive(Debug, Clone)]
pub struct BSplineCurve {
    geom: Option<geom2d::BSplineCurve>,
}

impl BSplineCurve {
    /// The evaluator of a B-spline curve (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::BSplineCurve>) -> Self {
        Self { geom: geom.cloned() }
    }

    fn with_data<T>(&self, params: &[f64], f: impl FnOnce(&SplineData<'_>) -> Result<Vec<T>>) -> Result<Vec<T>> {
        let Some(g) = self.geom.as_ref() else {
            return Ok(Vec::new());
        };
        if params.is_empty() {
            return Ok(Vec::new());
        }
        f(&Self::data(g))
    }

    fn data(g: &geom2d::BSplineCurve) -> SplineData<'_> {
        SplineData {
            flat_knots: g.knot_sequence(),
            poles: g.poles(),
            weights: g.weights(),
            knots: g.knots(),
            mults: g.multiplicities(),
            degree: g.degree(),
            periodic: g.is_periodic(),
        }
    }
}

impl GridEvaluator for BSplineCurve {
    fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>> {
        self.with_data(params, |d| {
            evaluate_grid_cached(
                d,
                params,
                |c, u| c.d0_local_2d(u),
                |d, u, span| curve2d::d0(u, span, d.degree, d.periodic, d.poles, d.weights, d.flat_knots, None),
            )
        })
    }
    fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>> {
        self.with_data(params, |d| {
            evaluate_grid_cached(
                d,
                params,
                |c, u| c.d1_local_2d(u).map(|(point, d1)| CurveD1 { point, d1 }),
                |d, u, span| {
                    curve2d::d1(u, span, d.degree, d.periodic, d.poles, d.weights, d.flat_knots, None)
                        .map(|(point, d1)| CurveD1 { point, d1 })
                },
            )
        })
    }
    fn evaluate_grid_d2(&self, params: &[f64]) -> Result<Vec<CurveD2>> {
        self.with_data(params, |d| {
            evaluate_grid_cached(
                d,
                params,
                |c, u| c.d2_local_2d(u).map(|(point, d1, d2)| CurveD2 { point, d1, d2 }),
                |d, u, span| {
                    curve2d::d2(u, span, d.degree, d.periodic, d.poles, d.weights, d.flat_knots, None)
                        .map(|(point, d1, d2)| CurveD2 { point, d1, d2 })
                },
            )
        })
    }
    fn evaluate_grid_d3(&self, params: &[f64]) -> Result<Vec<CurveD3>> {
        self.with_data(params, |d| {
            evaluate_grid_cached(
                d,
                params,
                |c, u| c.d3_local_2d(u).map(|(point, d1, d2, d3)| CurveD3 { point, d1, d2, d3 }),
                |d, u, span| {
                    curve2d::d3(u, span, d.degree, d.periodic, d.poles, d.weights, d.flat_knots, None)
                        .map(|(point, d1, d2, d3)| CurveD3 { point, d1, d2, d3 })
                },
            )
        })
    }
    fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>> {
        if n < 1 {
            return Ok(Vec::new());
        }
        self.with_data(params, |d| {
            // the derivatives of an order above the degree are zero
            if n > d.degree {
                return Ok(vec![zero(); params.len()]);
            }
            // direct evaluation (no cache); the span is the one of the flat knots, but `DN` gets the knots and the
            // multiplicities, as in the C++
            params
                .iter()
                .map(|&u| {
                    let (span, adj) = locate_span(u, d)?;
                    curve2d::dn(adj, n, span, d.degree, d.periodic, d.poles, d.weights, d.knots, Some(d.mults))
                        .map_err(bspl)
                })
                .collect()
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Offset curves
// ---------------------------------------------------------------------------------------------

/// `Geom2dGridEval_OffsetCurve`.
#[derive(Debug, Clone)]
pub struct OffsetCurve {
    offset: f64,
    basis: Option<Arc<dyn Curve2d>>,
}

impl OffsetCurve {
    /// The evaluator of an offset curve (`None`: a null handle).
    pub fn new(geom: Option<&geom2d::OffsetCurve>) -> Self {
        match geom {
            Some(g) => Self { offset: g.offset(), basis: Some(g.basis_curve()) },
            None => Self { offset: 0.0, basis: None },
        }
    }

    // the basis curve unless the geometry is null or there are no parameters
    fn basis_for(&self, params: &[f64]) -> Option<&Arc<dyn Curve2d>> {
        if params.is_empty() { None } else { self.basis.as_ref() }
    }
}

impl GridEvaluator for OffsetCurve {
    fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>> {
        let Some(basis) = self.basis_for(params) else {
            return Ok(Vec::new());
        };
        // the offset point needs the first derivative of the basis curve
        let basis_d1 = Curve::from_curve(Some(basis.clone())).evaluate_grid_d1(params)?;
        if basis_d1.is_empty() {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(params.len());
        for b in &basis_d1 {
            let mut p = b.point;
            if !offset::calculate_d0(&mut p, &b.d1, self.offset) {
                return Ok(Vec::new());
            }
            result.push(p);
        }
        Ok(result)
    }

    fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>> {
        let Some(basis) = self.basis_for(params) else {
            return Ok(Vec::new());
        };
        let basis_d2 = Curve::from_curve(Some(basis.clone())).evaluate_grid_d2(params)?;
        if basis_d2.is_empty() {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(params.len());
        for b in &basis_d2 {
            let (mut p, mut d1) = (b.point, b.d1);
            if !offset::calculate_d1(&mut p, &mut d1, &b.d2, self.offset) {
                return Ok(Vec::new());
            }
            result.push(CurveD1 { point: p, d1 });
        }
        Ok(result)
    }

    fn evaluate_grid_d2(&self, params: &[f64]) -> Result<Vec<CurveD2>> {
        let Some(basis) = self.basis_for(params) else {
            return Ok(Vec::new());
        };
        let basis_d3 = Curve::from_curve(Some(basis.clone())).evaluate_grid_d3(params)?;
        if basis_d3.is_empty() {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(params.len());
        for (b, &u) in basis_d3.iter().zip(params) {
            let mut p = b.point;
            let (mut d1, mut d2, mut d3) = (b.d1, b.d2, b.d3);
            // check for a direction change at a singular point
            let mut is_direction_change = false;
            if d1.square_magnitude() <= gp::resolution() {
                let mut dummy_d4 = zero();
                let mut ders = Derivatives { d1: &mut d1, d2: &mut d2, d3: &mut d3, d4: &mut dummy_d4 };
                if !offset::adjust_derivative(basis.as_ref(), 3, u, &mut ders, &mut is_direction_change)? {
                    return Ok(Vec::new());
                }
            }
            if !offset::calculate_d2(&mut p, &mut d1, &mut d2, &d3, is_direction_change, self.offset) {
                return Ok(Vec::new());
            }
            result.push(CurveD2 { point: p, d1, d2 });
        }
        Ok(result)
    }

    fn evaluate_grid_d3(&self, params: &[f64]) -> Result<Vec<CurveD3>> {
        let Some(basis) = self.basis_for(params) else {
            return Ok(Vec::new());
        };
        // the offset D3 needs the basis D3 and D4
        let basis_d3 = Curve::from_curve(Some(basis.clone())).evaluate_grid_d3(params)?;
        if basis_d3.is_empty() {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(params.len());
        for (b, &u) in basis_d3.iter().zip(params) {
            let mut p = b.point;
            let (mut d1, mut d2, mut d3) = (b.d1, b.d2, b.d3);
            let mut d4 = basis.eval_dn(u, 4)?;
            let mut is_direction_change = false;
            if d1.square_magnitude() <= gp::resolution() {
                let mut ders = Derivatives { d1: &mut d1, d2: &mut d2, d3: &mut d3, d4: &mut d4 };
                if !offset::adjust_derivative(basis.as_ref(), 4, u, &mut ders, &mut is_direction_change)? {
                    return Ok(Vec::new());
                }
            }
            if !offset::calculate_d3(&mut p, &mut d1, &mut d2, &mut d3, &d4, is_direction_change, self.offset) {
                return Ok(Vec::new());
            }
            result.push(CurveD3 { point: p, d1, d2, d3 });
        }
        Ok(result)
    }

    fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>> {
        let Some(basis) = self.basis_for(params) else {
            return Ok(Vec::new());
        };
        if n < 1 {
            return Ok(Vec::new());
        }
        // the optimised grid evaluators serve the orders 1 to 3
        match n {
            1 => Ok(self.evaluate_grid_d1(params)?.iter().map(|r| r.d1).collect()),
            2 => Ok(self.evaluate_grid_d2(params)?.iter().map(|r| r.d2).collect()),
            3 => Ok(self.evaluate_grid_d3(params)?.iter().map(|r| r.d3).collect()),
            // above 3: the derivative of the basis curve
            _ => Curve::from_curve(Some(basis.clone())).evaluate_grid_dn(params, n),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Other curves
// ---------------------------------------------------------------------------------------------

/// `Geom2dGridEval_OtherCurve`: the fallback over an adaptor (held by reference, as in C++).
#[derive(Debug, Clone, Copy)]
pub struct OtherCurve<'a> {
    curve: &'a dyn Curve2dAdaptor,
}

impl<'a> OtherCurve<'a> {
    /// The evaluator over the adaptor.
    pub fn new(curve: &'a dyn Curve2dAdaptor) -> Self {
        Self { curve }
    }
}

impl GridEvaluator for OtherCurve<'_> {
    fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>> {
        params.iter().map(|&u| self.curve.eval_d0(u)).collect()
    }
    fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>> {
        params
            .iter()
            .map(|&u| {
                let r = self.curve.eval_d1(u)?;
                Ok(CurveD1 { point: r.point, d1: r.d1 })
            })
            .collect()
    }
    fn evaluate_grid_d2(&self, params: &[f64]) -> Result<Vec<CurveD2>> {
        params
            .iter()
            .map(|&u| {
                let r = self.curve.eval_d2(u)?;
                Ok(CurveD2 { point: r.point, d1: r.d1, d2: r.d2 })
            })
            .collect()
    }
    fn evaluate_grid_d3(&self, params: &[f64]) -> Result<Vec<CurveD3>> {
        params
            .iter()
            .map(|&u| {
                let r = self.curve.eval_d3(u)?;
                Ok(CurveD3 { point: r.point, d1: r.d1, d2: r.d2, d3: r.d3 })
            })
            .collect()
    }
    fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>> {
        if params.is_empty() || n < 1 {
            return Ok(Vec::new());
        }
        match n {
            1 => Ok(self.evaluate_grid_d1(params)?.iter().map(|r| r.d1).collect()),
            2 => Ok(self.evaluate_grid_d2(params)?.iter().map(|r| r.d2).collect()),
            3 => Ok(self.evaluate_grid_d3(params)?.iter().map(|r| r.d3).collect()),
            _ => params.iter().map(|&u| self.curve.eval_dn(u, n)).collect(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The dispatcher
// ---------------------------------------------------------------------------------------------

/// The evaluator variant of `Geom2dGridEval_Curve` (`EvaluatorVariant`); `None` is `std::monostate`.
#[derive(Debug)]
enum Evaluator<'a> {
    None,
    Line(Line),
    Circle(Circle),
    Ellipse(Ellipse),
    Hyperbola(Hyperbola),
    Parabola(Parabola),
    Bezier(BezierCurve),
    BSpline(BSplineCurve),
    Offset(OffsetCurve),
    Other(OtherCurve<'a>),
}

/// `Geom2dGridEval_Curve`: the evaluator that fits the curve.
#[derive(Debug)]
pub struct Curve<'a> {
    evaluator: Evaluator<'a>,
    curve_type: GeomAbsCurveType,
}

impl Curve<'static> {
    /// `Geom2dGridEval_Curve(occ::handle<Geom2d_Curve>)` (`None`: a null handle, an empty evaluator of type
    /// `OtherCurve`). A chain of `TrimmedCurve`s is replaced by its basis curve.
    pub fn from_curve(curve: Option<Arc<dyn Curve2d>>) -> Self {
        let (evaluator, curve_type) = Self::initialize_curve(curve);
        Self { evaluator, curve_type }
    }
}

impl<'a> Curve<'a> {
    /// `Geom2dGridEval_Curve(const Adaptor2d_Curve2d&)`: a `Geom2dAdaptor_Curve` is replaced by its curve, any other adaptor is
    /// evaluated through `OtherCurve` and has the type `GetType()` (which is the error of the adaptor if it has none).
    pub fn from_adaptor(curve: &'a dyn Curve2dAdaptor) -> Result<Self> {
        if curve.is_geom2d_adaptor() {
            let (evaluator, curve_type) = Curve::initialize_curve(curve.geom2d_curve());
            return Ok(Self { evaluator, curve_type });
        }
        // for another adaptor, the reference is used by the evaluator
        let curve_type = curve.get_type()?;
        Ok(Self { evaluator: Evaluator::Other(OtherCurve::new(curve)), curve_type })
    }

    /// `GetType`.
    pub fn curve_type(&self) -> GeomAbsCurveType {
        self.curve_type
    }

    fn initialize_curve(curve: Option<Arc<dyn Curve2d>>) -> (Evaluator<'a>, GeomAbsCurveType) {
        let Some(mut basis) = curve else {
            return (Evaluator::None, GeomAbsCurveType::OtherCurve);
        };
        // extract the basis curve from the nested TrimmedCurve wrappers
        while let Some(basis_of_trimmed) = basis.as_any().downcast_ref::<TrimmedCurve>().map(TrimmedCurve::basis_curve)
        {
            basis = basis_of_trimmed;
        }
        let any = basis.as_any();
        if let Some(g) = any.downcast_ref::<geom2d::Line>() {
            (Evaluator::Line(Line::new(Some(g))), GeomAbsCurveType::Line)
        } else if let Some(g) = any.downcast_ref::<geom2d::Circle>() {
            (Evaluator::Circle(Circle::new(Some(g))), GeomAbsCurveType::Circle)
        } else if let Some(g) = any.downcast_ref::<geom2d::Ellipse>() {
            (Evaluator::Ellipse(Ellipse::new(Some(g))), GeomAbsCurveType::Ellipse)
        } else if let Some(g) = any.downcast_ref::<geom2d::Hyperbola>() {
            (Evaluator::Hyperbola(Hyperbola::new(Some(g))), GeomAbsCurveType::Hyperbola)
        } else if let Some(g) = any.downcast_ref::<geom2d::Parabola>() {
            (Evaluator::Parabola(Parabola::new(Some(g))), GeomAbsCurveType::Parabola)
        } else if let Some(g) = any.downcast_ref::<geom2d::BezierCurve>() {
            (Evaluator::Bezier(BezierCurve::new(Some(g))), GeomAbsCurveType::BezierCurve)
        } else if let Some(g) = any.downcast_ref::<geom2d::BSplineCurve>() {
            (Evaluator::BSpline(BSplineCurve::new(Some(g))), GeomAbsCurveType::BSplineCurve)
        } else if let Some(g) = any.downcast_ref::<geom2d::OffsetCurve>() {
            (Evaluator::Offset(OffsetCurve::new(Some(g))), GeomAbsCurveType::OffsetCurve)
        } else {
            // an unknown curve type: uninitialised
            (Evaluator::None, GeomAbsCurveType::OtherCurve)
        }
    }
}

macro_rules! dispatch {
    ($self:expr, $method:ident, $($arg:expr),*) => {
        match &$self.evaluator {
            Evaluator::None => Ok(Vec::new()),
            Evaluator::Line(e) => e.$method($($arg),*),
            Evaluator::Circle(e) => e.$method($($arg),*),
            Evaluator::Ellipse(e) => e.$method($($arg),*),
            Evaluator::Hyperbola(e) => e.$method($($arg),*),
            Evaluator::Parabola(e) => e.$method($($arg),*),
            Evaluator::Bezier(e) => e.$method($($arg),*),
            Evaluator::BSpline(e) => e.$method($($arg),*),
            Evaluator::Offset(e) => e.$method($($arg),*),
            Evaluator::Other(e) => e.$method($($arg),*),
        }
    };
}

impl GridEvaluator for Curve<'_> {
    fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>> {
        dispatch!(self, evaluate_grid, params)
    }
    fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>> {
        dispatch!(self, evaluate_grid_d1, params)
    }
    fn evaluate_grid_d2(&self, params: &[f64]) -> Result<Vec<CurveD2>> {
        dispatch!(self, evaluate_grid_d2, params)
    }
    fn evaluate_grid_d3(&self, params: &[f64]) -> Result<Vec<CurveD3>> {
        dispatch!(self, evaluate_grid_d3, params)
    }
    fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>> {
        dispatch!(self, evaluate_grid_dn, params, n)
    }
}
