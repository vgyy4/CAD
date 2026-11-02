//! OCCT's package `Geom2dAdaptor`: the adaptor `Geom2dAdaptor_Curve` over a `Geom2d_Curve` (elementary curves evaluated by
//! `ElCLib`, Bezier and B-spline curves by a `BSplCLib_Cache`, offset curves through a nested adaptor of the basis curve) and
//! `Geom2dAdaptor::MakeCurve`.
//!
//! Translated from `Geom2dAdaptor.cxx` and `Geom2dAdaptor_Curve.hxx/.cxx` (OCCT 8.0.1). It implements
//! [`Curve2dAdaptor`](crate::adaptor2d::Curve2dAdaptor). The C++ `std::variant` of curve-specific data is the private enum
//! `CurveData`; the mutable `BSplCLib_Cache` handles are `Mutex<Option<Cache>>`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::adaptor2d::{AdaptorError, Curve2dAdaptor, GeomAbsCurveType, GeomAbsShape, Result};
use crate::bsplclib::flat::locate_core;
use crate::bsplclib_cache::{Cache, flat_bezier_knots, intervals};
use crate::elclib;
use crate::geom2d::{
    BSplineCurve, BezierCurve, Circle, Curve2d, Ellipse, Hyperbola, Line, OffsetCurve, Parabola, ResD1, ResD2, ResD3,
    TrimmedCurve, confusion, pconfusion,
};
use crate::gp::{self, Circ2d, Elips2d, Hypr2d, Lin2d, Parab2d, Pnt2d, Vec2d};
use std::sync::{Arc, Mutex};

/// `std::max(a, b)` / `std::min(a, b)` on doubles.
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn pos_tol() -> f64 {
    pconfusion() / 2.0
}

/// Offset curve data of the adaptor (`Geom2dAdaptor_Curve::OffsetData`).
#[derive(Debug, Clone)]
struct OffsetData {
    basis: Option<Arc<Curve>>,
    offset: f64,
    has_eval_rep: bool,
}

/// Bezier curve data (`BezierData`): the curve, its cache and whether it has an evaluation representation.
#[derive(Debug)]
struct BezierData {
    curve: Arc<BezierCurve>,
    cache: Mutex<Option<Cache>>,
    has_eval_rep: bool,
}

/// B-spline curve data (`BSplineData`).
#[derive(Debug)]
struct BSplineData {
    curve: Arc<BSplineCurve>,
    cache: Mutex<Option<Cache>>,
    has_eval_rep: bool,
}

/// The curve-specific data (`CurveDataVariant`).
#[derive(Debug)]
enum CurveData {
    None,
    Lin(Lin2d),
    Circ(Circ2d),
    Elips(Elips2d),
    Hypr(Hypr2d),
    Parab(Parab2d),
    Offset(OffsetData),
    Bezier(BezierData),
    BSpline(BSplineData),
}

/// `Geom2dAdaptor_Curve`.
#[derive(Debug)]
pub struct Curve {
    curve: Option<Arc<dyn Curve2d>>,
    type_curve: GeomAbsCurveType,
    first: f64,
    last: f64,
    data: CurveData,
}

impl Default for Curve {
    fn default() -> Self {
        Self::new()
    }
}

impl Curve {
    /// `Geom2dAdaptor_Curve()`: no curve, type `OtherCurve`, range 0, 0.
    pub fn new() -> Self {
        Self { curve: None, type_curve: GeomAbsCurveType::OtherCurve, first: 0.0, last: 0.0, data: CurveData::None }
    }

    /// `Geom2dAdaptor_Curve(C)`: `Load(C)`.
    pub fn with_curve(c: Arc<dyn Curve2d>) -> Result<Self> {
        let mut a = Self::new();
        a.load(c)?;
        Ok(a)
    }

    /// `Geom2dAdaptor_Curve(C, UFirst, ULast)`: `Load(C, UFirst, ULast)`.
    pub fn with_range(c: Arc<dyn Curve2d>, u_first: f64, u_last: f64) -> Result<Self> {
        let mut a = Self::new();
        a.first = u_first;
        a.last = u_last;
        a.load_range(c, u_first, u_last)?;
        Ok(a)
    }

    /// `Reset`.
    pub fn reset(&mut self) {
        self.type_curve = GeomAbsCurveType::OtherCurve;
        self.curve = None;
        self.data = CurveData::None;
        self.first = 0.0;
        self.last = 0.0;
    }

    /// `Load(C)`: the whole parameter range of the curve.
    pub fn load(&mut self, c: Arc<dyn Curve2d>) -> Result<()> {
        let (f, l) = (c.first_parameter(), c.last_parameter());
        self.load_impl(c, f, l)
    }

    /// `Load(C, UFirst, ULast)`: `ConstructionError` if `UFirst > ULast + Precision::Confusion()`.
    pub fn load_range(&mut self, c: Arc<dyn Curve2d>, u_first: f64, u_last: f64) -> Result<()> {
        if u_first > u_last + confusion() {
            return Err(AdaptorError::Construction);
        }
        self.load_impl(c, u_first, u_last)
    }

    /// `IsInitialized`.
    pub fn is_initialized(&self) -> bool {
        self.curve.is_some()
    }

    /// `Curve`.
    pub fn curve(&self) -> Option<&Arc<dyn Curve2d>> {
        self.curve.as_ref()
    }

    fn cur(&self) -> Result<&Arc<dyn Curve2d>> {
        self.curve.as_ref().ok_or(AdaptorError::NullObject)
    }

    // Geom2dAdaptor_Curve::load
    fn load_impl(&mut self, c: Arc<dyn Curve2d>, u_first: f64, u_last: f64) -> Result<()> {
        self.first = u_first;
        self.last = u_last;
        let same = self
            .curve
            .as_ref()
            .is_some_and(|m| std::ptr::eq(Arc::as_ptr(m).cast::<()>(), Arc::as_ptr(&c).cast::<()>()));
        if !same {
            self.curve = Some(c.clone());
            self.data = CurveData::None;
            let any = c.as_any();
            if let Some(t) = any.downcast_ref::<TrimmedCurve>() {
                let basis = t.basis_curve();
                return self.load_range(basis, u_first, u_last);
            } else if let Some(x) = any.downcast_ref::<Circle>() {
                self.type_curve = GeomAbsCurveType::Circle;
                self.data = CurveData::Circ(x.circ2d());
            } else if let Some(x) = any.downcast_ref::<Line>() {
                self.type_curve = GeomAbsCurveType::Line;
                self.data = CurveData::Lin(x.lin2d());
            } else if let Some(x) = any.downcast_ref::<Ellipse>() {
                self.type_curve = GeomAbsCurveType::Ellipse;
                self.data = CurveData::Elips(x.elips2d());
            } else if let Some(x) = any.downcast_ref::<Parabola>() {
                self.type_curve = GeomAbsCurveType::Parabola;
                self.data = CurveData::Parab(x.parab2d());
            } else if let Some(x) = any.downcast_ref::<Hyperbola>() {
                self.type_curve = GeomAbsCurveType::Hyperbola;
                self.data = CurveData::Hypr(x.hypr2d());
            } else if let Some(x) = any.downcast_ref::<BezierCurve>() {
                self.type_curve = GeomAbsCurveType::BezierCurve;
                self.data = CurveData::Bezier(BezierData {
                    has_eval_rep: x.eval_representation().is_some(),
                    curve: Arc::new(x.clone()),
                    cache: Mutex::new(None),
                });
            } else if let Some(x) = any.downcast_ref::<BSplineCurve>() {
                self.type_curve = GeomAbsCurveType::BSplineCurve;
                self.data = CurveData::BSpline(BSplineData {
                    has_eval_rep: x.eval_representation().is_some(),
                    curve: Arc::new(x.clone()),
                    cache: Mutex::new(None),
                });
            } else if let Some(x) = any.downcast_ref::<OffsetCurve>() {
                self.type_curve = GeomAbsCurveType::OffsetCurve;
                // a nested adaptor for the basis curve and the offset data
                let basis = Curve::with_range(x.basis_curve(), u_first, u_last)?;
                self.data = CurveData::Offset(OffsetData {
                    basis: Some(Arc::new(basis)),
                    offset: x.offset(),
                    has_eval_rep: x.eval_representation().is_some(),
                });
            } else {
                self.type_curve = GeomAbsCurveType::OtherCurve;
            }
        } else {
            // the same curve but potentially other parameters: invalidate the cache
            match &self.data {
                CurveData::BSpline(d) => *lock(&d.cache) = None,
                CurveData::Bezier(d) => *lock(&d.cache) = None,
                _ => {}
            }
        }
        Ok(())
    }

    fn has_eval_rep(&self) -> bool {
        match &self.data {
            CurveData::Bezier(d) => d.has_eval_rep,
            CurveData::BSpline(d) => d.has_eval_rep,
            CurveData::Offset(d) => d.has_eval_rep,
            _ => false,
        }
    }

    fn bspline(&self) -> Result<&Arc<BSplineCurve>> {
        match &self.data {
            CurveData::BSpline(d) => Ok(&d.curve),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }

    // Geom2dAdaptor_Curve::LocalContinuity
    fn local_continuity(&self, u1: f64, u2: f64) -> Result<GeomAbsShape> {
        let b = self.bspline()?;
        let nb = b.nb_knots();
        let tk = b.knots();
        let tm = b.multiplicities();
        let periodic = b.is_periodic();
        let (uf, ul) =
            if periodic { (tk.first().copied().unwrap_or(0.0), tk.last().copied().unwrap_or(1.0)) } else { (0.0, 1.0) };
        let (mut index1, new_first) = locate_core(tk, u1, periodic, 1, nb, uf, ul).map_err(bspl)?;
        let (mut index2, new_last) = locate_core(tk, u2, periodic, 1, nb, uf, ul).map_err(bspl)?;
        let at_k = |i: i32| -> Result<f64> { rd1(tk, i) };
        let at_m = |i: i32| -> Result<i32> { rd1(tm, i) };
        if (new_first - at_k(index1 + 1)?).abs() < pconfusion() && index1 < nb {
            index1 += 1;
        }
        if (new_last - at_k(index2)?).abs() < pconfusion() {
            index2 -= 1;
        }
        // beware of periodic curves
        if periodic && index1 == nb {
            index1 = 1;
        }
        let mult_max;
        if (index2 - index1 <= 0) && !periodic {
            mult_max = 100; // CN between 2 consecutive nodes
        } else {
            let mut m = at_m(index1 + 1)?;
            for i in (index1 + 1)..=index2 {
                if at_m(i)? > m {
                    m = at_m(i)?;
                }
            }
            mult_max = b.degree() - m;
        }
        Ok(match mult_max {
            ..=0 => GeomAbsShape::C0,
            1 => GeomAbsShape::C1,
            2 => GeomAbsShape::C2,
            3 => GeomAbsShape::C3,
            _ => GeomAbsShape::Cn,
        })
    }

    // Geom2dAdaptor_Curve::RebuildCache
    fn rebuild_cache(&self, parameter: f64) -> Result<()> {
        match &self.data {
            CurveData::Bezier(d) => {
                let b = &d.curve;
                let deg = b.degree();
                let flat = flat_bezier_knots(deg).map_err(bspl)?;
                let mut g = lock(&d.cache);
                if g.is_none() {
                    *g = Some(Cache::new_2d(deg, b.is_periodic(), &flat, b.weights()).map_err(bspl)?);
                }
                if let Some(c) = g.as_mut() {
                    c.build_cache_2d(parameter, &flat, b.poles(), b.weights()).map_err(bspl)?;
                }
            }
            CurveData::BSpline(d) => {
                let b = &d.curve;
                let mut g = lock(&d.cache);
                if g.is_none() {
                    *g =
                        Some(Cache::new_2d(b.degree(), b.is_periodic(), b.knot_sequence(), b.weights()).map_err(bspl)?);
                }
                if let Some(c) = g.as_mut() {
                    c.build_cache_2d(parameter, b.knot_sequence(), b.poles(), b.weights()).map_err(bspl)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // Geom2dAdaptor_Curve::IsBoundary: the span of the first or the last parameter of a B-spline
    fn is_boundary(&self, u: f64) -> Option<(i32, i32)> {
        let CurveData::BSpline(d) = &self.data else {
            return None;
        };
        if !(u == self.first || u == self.last) {
            return None;
        }
        let b = &d.curve;
        if u == self.first {
            let (mut start, mut finish) = b.locate_u(self.first, pos_tol(), false);
            if start < 1 {
                start = 1;
            }
            if start >= finish {
                finish = start + 1;
            }
            Some((start, finish))
        } else {
            let (mut start, mut finish) = b.locate_u(self.last, pos_tol(), false);
            if finish > b.nb_knots() {
                finish = b.nb_knots();
            }
            if start >= finish {
                start = finish - 1;
            }
            Some((start, finish))
        }
    }

    /// Runs `f` on the cache of a Bezier / B-spline curve, rebuilding it when it is missing or (B-spline) not valid at `u`.
    fn with_cache<T>(
        &self,
        u: f64,
        f: impl FnOnce(&Cache) -> std::result::Result<T, crate::bsplclib::BSplError>,
    ) -> Result<T> {
        let (cache, check_valid) = match &self.data {
            CurveData::Bezier(d) => (&d.cache, false),
            CurveData::BSpline(d) => (&d.cache, true),
            _ => return Err(AdaptorError::NoSuchObject),
        };
        let invalid = {
            let g = lock(cache);
            match g.as_ref() {
                None => true,
                Some(c) => check_valid && !c.is_cache_valid(u),
            }
        };
        if invalid {
            self.rebuild_cache(u)?;
        }
        let g = lock(cache);
        let c = g.as_ref().ok_or(AdaptorError::NoSuchObject)?;
        f(c).map_err(bspl)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn bspl(e: crate::bsplclib::BSplError) -> AdaptorError {
    AdaptorError::Geom(crate::geom2d::Geom2dError::from(e))
}

fn rd1<T: Copy>(a: &[T], i: i32) -> Result<T> {
    usize::try_from(i - 1).ok().and_then(|k| a.get(k)).copied().ok_or(AdaptorError::OutOfRange)
}

/// `Geom2d_OffsetCurveUtils::AdjustDerivative` over an adaptor: replaces the derivatives of a point where the first one vanishes.
fn adjust_derivative(
    curve: &Curve,
    max_derivative: i32,
    u: f64,
    d: &mut [Vec2d; 4],
    is_direction_change: &mut bool,
) -> Result<()> {
    let tol = gp::resolution();
    let min_step = 1e-7;
    let max_deriv_order = 3;
    *is_direction_change = false;
    let uinfium = curve.first;
    let usupremum = curve.last;
    let division_factor = 1.0e-3;
    let du = if usupremum >= f64::MAX || uinfium <= -f64::MAX { 0.0 } else { usupremum - uinfium };
    let delta = cmax(du * division_factor, min_step);
    // the derivative is approximated by a Taylor series
    let mut index = 1;
    let mut v;
    loop {
        index += 1;
        v = curve.eval_dn(u, index)?;
        if !(v.square_magnitude() <= tol && index < max_deriv_order) {
            break;
        }
    }
    let uu = if u - uinfium < delta { u + delta } else { u - delta };
    let p1 = curve.eval_d0(cmin(u, uu))?;
    let p2 = curve.eval_d0(cmax(u, uu))?;
    let v1 = Vec2d::between(&p1, &p2);
    *is_direction_change = v.dot(&v1) < 0.0;
    let sign = if *is_direction_change { -1.0 } else { 1.0 };
    d[0] = v.multiplied(sign);
    for i in 1..max_derivative {
        let dn = curve.eval_dn(u, index + i)?;
        if let Some(t) = d.get_mut((i) as usize) {
            *t = dn.multiplied(sign);
        }
    }
    Ok(())
}

// Geom2d_OffsetCurveUtils::EvaluateD0 .. EvaluateDN over the basis adaptor
fn offset_d0(u: f64, basis: &Curve, offset: f64) -> Result<Option<Pnt2d>> {
    let r = basis.eval_d1(u)?;
    let mut p = r.point;
    Ok(crate::geom2d::offset::calculate_d0(&mut p, &r.d1, offset).then_some(p))
}
fn offset_d1(u: f64, basis: &Curve, offset: f64) -> Result<Option<(Pnt2d, Vec2d)>> {
    let r = basis.eval_d2(u)?;
    let (mut p, mut d1) = (r.point, r.d1);
    Ok(crate::geom2d::offset::calculate_d1(&mut p, &mut d1, &r.d2, offset).then_some((p, d1)))
}
fn offset_d2(u: f64, basis: &Curve, offset: f64) -> Result<Option<(Pnt2d, Vec2d, Vec2d)>> {
    let r = basis.eval_d3(u)?;
    let (mut p, mut d1, mut d2, mut d3) = (r.point, r.d1, r.d2, r.d3);
    let mut dir_change = false;
    if d1.square_magnitude() <= gp::resolution() {
        let mut ders = [d1, d2, d3, Vec2d::new(0.0, 0.0)];
        adjust_derivative(basis, 3, u, &mut ders, &mut dir_change)?;
        [d1, d2, d3, _] = ders;
    }
    Ok(crate::geom2d::offset::calculate_d2(&mut p, &mut d1, &mut d2, &d3, dir_change, offset).then_some((p, d1, d2)))
}
fn offset_d3(u: f64, basis: &Curve, offset: f64) -> Result<Option<(Pnt2d, Vec2d, Vec2d, Vec2d)>> {
    let r = basis.eval_d3(u)?;
    let (mut p, mut d1, mut d2, mut d3) = (r.point, r.d1, r.d2, r.d3);
    let mut d4 = basis.eval_dn(u, 4)?;
    let mut dir_change = false;
    if d1.square_magnitude() <= gp::resolution() {
        let mut ders = [d1, d2, d3, d4];
        adjust_derivative(basis, 4, u, &mut ders, &mut dir_change)?;
        [d1, d2, d3, d4] = ders;
    }
    Ok(crate::geom2d::offset::calculate_d3(&mut p, &mut d1, &mut d2, &mut d3, &d4, dir_change, offset)
        .then_some((p, d1, d2, d3)))
}
/// `EvaluateDN` with `theN == 3` calls `EvaluateD3(theU, basis, offset, aPnt, aDummy, aDummy, theDN)`: the first and
/// the second derivative are the same C++ variable, so both `CalculateD3` inputs hold the last value written to it
/// (the second derivative) and the result is not the third derivative of the offset curve proper.
fn offset_dn3(u: f64, basis: &Curve, offset: f64) -> Result<Option<Vec2d>> {
    let r = basis.eval_d3(u)?;
    let mut p = r.point;
    let mut dd = r.d2; // aDummy = D1, then aDummy = D2
    let mut d3 = r.d3;
    let mut d4 = basis.eval_dn(u, 4)?;
    let mut dir_change = false;
    if dd.square_magnitude() <= gp::resolution() {
        let mut ders = [dd, dd, d3, d4];
        adjust_derivative(basis, 4, u, &mut ders, &mut dir_change)?;
        // the writes to the first and the second derivative go to the same variable: the second one stays
        [_, dd, d3, d4] = ders;
    }
    let (mut a, mut b) = (dd, dd);
    Ok(crate::geom2d::offset::calculate_d3(&mut p, &mut a, &mut b, &mut d3, &d4, dir_change, offset).then_some(d3))
}
fn offset_dn(u: f64, basis: &Curve, offset: f64, n: i32) -> Result<Option<Vec2d>> {
    Ok(match n {
        1 => offset_d1(u, basis, offset)?.map(|r| r.1),
        2 => offset_d2(u, basis, offset)?.map(|r| r.2),
        3 => offset_dn3(u, basis, offset)?,
        _ => Some(basis.eval_dn(u, n)?),
    })
}

impl Curve {
    fn offset_data(&self) -> Result<(&Curve, f64)> {
        match &self.data {
            CurveData::Offset(d) => Ok((d.basis.as_deref().ok_or(AdaptorError::NullObject)?, d.offset)),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }

    // NbIntervals / Intervals of an offset curve: the continuity asked of the basis curve
    fn offset_base_shape(s: GeomAbsShape) -> Result<GeomAbsShape> {
        match s {
            GeomAbsShape::G1 | GeomAbsShape::G2 => Err(AdaptorError::Domain),
            GeomAbsShape::C0 => Ok(GeomAbsShape::C1),
            GeomAbsShape::C1 => Ok(GeomAbsShape::C2),
            GeomAbsShape::C2 => Ok(GeomAbsShape::C3),
            _ => Ok(GeomAbsShape::Cn),
        }
    }

    // the continuity order of a B-spline for NbIntervals / Intervals
    fn bspline_cont(s: GeomAbsShape, degree: i32) -> Option<i32> {
        match s {
            GeomAbsShape::C1 => Some(1),
            GeomAbsShape::C2 => Some(2),
            GeomAbsShape::C3 => Some(3),
            GeomAbsShape::Cn => Some(degree),
            _ => None,
        }
    }

    fn spline_intervals(&self, s: GeomAbsShape, want: bool) -> Result<Option<(i32, Option<Vec<f64>>)>> {
        let b = self.bspline()?;
        if (!b.is_periodic() && (s as i32) <= (self.continuity()? as i32)) || s == GeomAbsShape::C0 {
            return Ok(None);
        }
        let degree = b.degree();
        let cont = Self::bspline_cont(s, degree).ok_or(AdaptorError::Domain)?;
        let eps = cmin(self.resolution(confusion())?, pconfusion());
        let r =
            intervals(b.knots(), b.multiplicities(), degree, b.is_periodic(), cont, self.first, self.last, eps, want)
                .map_err(bspl)?;
        Ok(Some(r))
    }
}

impl Curve2dAdaptor for Curve {
    fn shallow_copy(&self) -> Result<Arc<dyn Curve2dAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }

    fn first_parameter(&self) -> Result<f64> {
        Ok(self.first)
    }
    fn last_parameter(&self) -> Result<f64> {
        Ok(self.last)
    }

    fn continuity(&self) -> Result<GeomAbsShape> {
        if self.type_curve == GeomAbsCurveType::BSplineCurve {
            self.local_continuity(self.first, self.last)
        } else if self.type_curve == GeomAbsCurveType::OffsetCurve {
            let c = self.cur()?;
            let off = c.as_any().downcast_ref::<OffsetCurve>().ok_or(AdaptorError::NoSuchObject)?;
            match off.get_basis_curve_continuity() {
                GeomAbsShape::Cn => Ok(GeomAbsShape::Cn),
                GeomAbsShape::C3 => Ok(GeomAbsShape::C2),
                GeomAbsShape::C2 => Ok(GeomAbsShape::C1),
                GeomAbsShape::C1 => Ok(GeomAbsShape::C0),
                GeomAbsShape::G1 => Ok(GeomAbsShape::G1),
                GeomAbsShape::G2 => Ok(GeomAbsShape::G2),
                GeomAbsShape::C0 => Err(AdaptorError::NoSuchObject),
            }
        } else if self.type_curve == GeomAbsCurveType::OtherCurve {
            Err(AdaptorError::NoSuchObject)
        } else {
            Ok(GeomAbsShape::Cn)
        }
    }

    fn nb_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        if self.type_curve == GeomAbsCurveType::BSplineCurve {
            return Ok(match self.spline_intervals(s, false)? {
                None => 1,
                Some((n, _)) => n,
            });
        } else if self.type_curve == GeomAbsCurveType::OffsetCurve {
            let base_s = Self::offset_base_shape(s)?;
            let c = self.cur()?;
            let off = c.as_any().downcast_ref::<OffsetCurve>().ok_or(AdaptorError::NoSuchObject)?;
            let adaptor = Curve::with_range(off.basis_curve(), self.first, self.last)?;
            return adaptor.nb_intervals(base_s);
        }
        Ok(1)
    }

    fn intervals(&self, t: &mut Vec<f64>, s: GeomAbsShape) -> Result<()> {
        let put = |t: &mut Vec<f64>, k: usize, v: f64| -> Result<()> {
            *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = v;
            Ok(())
        };
        if self.type_curve == GeomAbsCurveType::BSplineCurve {
            match self.spline_intervals(s, true)? {
                None => {
                    put(t, 0, self.first)?;
                    put(t, 1, self.last)?;
                }
                Some((_, v)) => *t = v.ok_or(AdaptorError::OutOfRange)?,
            }
        } else if self.type_curve == GeomAbsCurveType::OffsetCurve {
            let base_s = Self::offset_base_shape(s)?;
            let c = self.cur()?;
            let off = c.as_any().downcast_ref::<OffsetCurve>().ok_or(AdaptorError::NoSuchObject)?;
            let adaptor = Curve::with_range(off.basis_curve(), self.first, self.last)?;
            let n = adaptor.nb_intervals(base_s)?;
            adaptor.intervals(t, base_s)?;
            put(t, 0, self.first)?;
            put(t, usize::try_from(n).map_err(|_| AdaptorError::OutOfRange)?, self.last)?;
        } else {
            put(t, 0, self.first)?;
            put(t, 1, self.last)?;
        }
        Ok(())
    }

    fn trim(&self, first: f64, last: f64, _tol: f64) -> Result<Arc<dyn Curve2dAdaptor>> {
        let c = self.cur()?.clone();
        Ok(Arc::new(Curve::with_range(c, first, last)?))
    }

    fn is_closed(&self) -> Result<bool> {
        // !Precision::IsPositiveInfinite(last) && !Precision::IsNegativeInfinite(first) (also true for a NaN)
        if (self.last < 2.0e100 || self.last.is_nan()) && (self.first > -2.0e100 || self.first.is_nan()) {
            let pd = self.value(self.first)?;
            let pf = self.value(self.last)?;
            Ok(pd.distance(&pf) <= confusion())
        } else {
            Ok(false)
        }
    }

    fn is_periodic(&self) -> Result<bool> {
        Ok(self.cur()?.is_periodic())
    }

    fn period(&self) -> Result<f64> {
        let c = self.cur()?;
        Ok(c.last_parameter() - c.first_parameter())
    }

    fn value(&self, u: f64) -> Result<Pnt2d> {
        self.d0(u)
    }

    fn d0(&self, u: f64) -> Result<Pnt2d> {
        self.eval_d0(u)
    }

    fn d1(&self, u: f64) -> Result<(Pnt2d, Vec2d)> {
        let r = self.eval_d1(u)?;
        Ok((r.point, r.d1))
    }

    fn d2(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d)> {
        let r = self.eval_d2(u)?;
        Ok((r.point, r.d1, r.d2))
    }

    fn d3(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let r = self.eval_d3(u)?;
        Ok((r.point, r.d1, r.d2, r.d3))
    }

    fn dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        self.eval_dn(u, n)
    }

    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        match &self.data {
            CurveData::Lin(x) => Ok(elclib::line2d_value(u, x.position())),
            CurveData::Circ(x) => Ok(elclib::circle2d_value(u, x.axis(), x.radius())),
            CurveData::Elips(x) => Ok(elclib::ellipse2d_value(u, x.axis(), x.major_radius(), x.minor_radius())),
            CurveData::Hypr(x) => Ok(elclib::hyperbola2d_value(u, x.axis(), x.major_radius(), x.minor_radius())),
            CurveData::Parab(x) => Ok(elclib::parabola2d_value(u, x.axis(), x.focal())),
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d0(u)?);
                }
                self.with_cache(u, |c| c.d0_2d(u))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d0(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(d.curve.local_d0(u, start, finish)?)
                } else {
                    self.with_cache(u, |c| c.d0_2d(u))
                }
            }
            CurveData::Offset(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d0(u)?);
                }
                let (basis, offset) = self.offset_data()?;
                offset_d0(u, basis, offset)?.ok_or(AdaptorError::UndefinedValue)
            }
            CurveData::None => Ok(self.cur()?.eval_d0(u)?),
        }
    }

    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let r = |(point, d1): (Pnt2d, Vec2d)| ResD1 { point, d1 };
        match &self.data {
            CurveData::Lin(x) => Ok(r(elclib::line2d_d1(u, x.position()))),
            CurveData::Circ(x) => Ok(r(elclib::circle2d_d1(u, x.axis(), x.radius()))),
            CurveData::Elips(x) => Ok(r(elclib::ellipse2d_d1(u, x.axis(), x.major_radius(), x.minor_radius()))),
            CurveData::Hypr(x) => Ok(r(elclib::hyperbola2d_d1(u, x.axis(), x.major_radius(), x.minor_radius()))),
            CurveData::Parab(x) => Ok(r(elclib::parabola2d_d1(u, x.axis(), x.focal()))),
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d1(u)?);
                }
                Ok(r(self.with_cache(u, |c| c.d1_2d(u))?))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d1(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(r(d.curve.local_d1(u, start, finish)?))
                } else {
                    Ok(r(self.with_cache(u, |c| c.d1_2d(u))?))
                }
            }
            CurveData::Offset(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d1(u)?);
                }
                let (basis, offset) = self.offset_data()?;
                offset_d1(u, basis, offset)?.map(r).ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_d1(u)?),
        }
    }

    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let r = |(point, d1, d2): (Pnt2d, Vec2d, Vec2d)| ResD2 { point, d1, d2 };
        match &self.data {
            CurveData::Lin(x) => {
                let (point, d1) = elclib::line2d_d1(u, x.position());
                Ok(ResD2 { point, d1, d2: Vec2d::new(0.0, 0.0) })
            }
            CurveData::Circ(x) => Ok(r(elclib::circle2d_d2(u, x.axis(), x.radius()))),
            CurveData::Elips(x) => Ok(r(elclib::ellipse2d_d2(u, x.axis(), x.major_radius(), x.minor_radius()))),
            CurveData::Hypr(x) => Ok(r(elclib::hyperbola2d_d2(u, x.axis(), x.major_radius(), x.minor_radius()))),
            CurveData::Parab(x) => Ok(r(elclib::parabola2d_d2(u, x.axis(), x.focal()))),
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d2(u)?);
                }
                Ok(r(self.with_cache(u, |c| c.d2_2d(u))?))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d2(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(r(d.curve.local_d2(u, start, finish)?))
                } else {
                    Ok(r(self.with_cache(u, |c| c.d2_2d(u))?))
                }
            }
            CurveData::Offset(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d2(u)?);
                }
                let (basis, offset) = self.offset_data()?;
                offset_d2(u, basis, offset)?.map(r).ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_d2(u)?),
        }
    }

    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let r = |(point, d1, d2, d3): (Pnt2d, Vec2d, Vec2d, Vec2d)| ResD3 { point, d1, d2, d3 };
        let zero = Vec2d::new(0.0, 0.0);
        match &self.data {
            CurveData::Lin(x) => {
                let (point, d1) = elclib::line2d_d1(u, x.position());
                Ok(ResD3 { point, d1, d2: zero, d3: zero })
            }
            CurveData::Circ(x) => Ok(r(elclib::circle2d_d3(u, x.axis(), x.radius()))),
            CurveData::Elips(x) => Ok(r(elclib::ellipse2d_d3(u, x.axis(), x.major_radius(), x.minor_radius()))),
            CurveData::Hypr(x) => Ok(r(elclib::hyperbola2d_d3(u, x.axis(), x.major_radius(), x.minor_radius()))),
            CurveData::Parab(x) => {
                let (point, d1, d2) = elclib::parabola2d_d2(u, x.axis(), x.focal());
                Ok(ResD3 { point, d1, d2, d3: zero })
            }
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d3(u)?);
                }
                Ok(r(self.with_cache(u, |c| c.d3_2d(u))?))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d3(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(r(d.curve.local_d3(u, start, finish)?))
                } else {
                    Ok(r(self.with_cache(u, |c| c.d3_2d(u))?))
                }
            }
            CurveData::Offset(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d3(u)?);
                }
                let (basis, offset) = self.offset_data()?;
                offset_d3(u, basis, offset)?.map(r).ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_d3(u)?),
        }
    }

    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        match &self.data {
            CurveData::Lin(x) => Ok(elclib::line2d_dn(u, x.position(), n)),
            CurveData::Circ(x) => Ok(elclib::circle2d_dn(u, x.axis(), x.radius(), n)),
            CurveData::Elips(x) => Ok(elclib::ellipse2d_dn(u, x.axis(), x.major_radius(), x.minor_radius(), n)),
            CurveData::Hypr(x) => Ok(elclib::hyperbola2d_dn(u, x.axis(), x.major_radius(), x.minor_radius(), n)),
            CurveData::Parab(x) => Ok(elclib::parabola2d_dn(u, x.axis(), x.focal(), n)),
            CurveData::Bezier(_) => Ok(self.cur()?.eval_dn(u, n)?),
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_dn(u, n)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(d.curve.local_dn(u, start, finish, n)?)
                } else {
                    Ok(self.cur()?.eval_dn(u, n)?)
                }
            }
            CurveData::Offset(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_dn(u, n)?);
                }
                let (basis, offset) = self.offset_data()?;
                offset_dn(u, basis, offset, n)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_dn(u, n)?),
        }
    }

    fn resolution(&self, ruv: f64) -> Result<f64> {
        match &self.data {
            CurveData::Lin(_) => Ok(ruv),
            CurveData::Circ(x) => {
                let r = x.radius();
                if r > ruv / 2.0 { Ok(2.0 * libm::asin(ruv / (2.0 * r))) } else { Ok(2.0 * std::f64::consts::PI) }
            }
            CurveData::Elips(x) => Ok(ruv / x.major_radius()),
            CurveData::Bezier(d) => Ok((*d.curve).clone().resolution(ruv)),
            CurveData::BSpline(d) => Ok((*d.curve).clone().resolution(ruv)),
            _ => Ok(ruv * 0.01), // Precision::Parametric(Ruv)
        }
    }

    fn get_type(&self) -> Result<GeomAbsCurveType> {
        Ok(self.type_curve)
    }

    fn line(&self) -> Result<Lin2d> {
        match &self.data {
            CurveData::Lin(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn circle(&self) -> Result<Circ2d> {
        match &self.data {
            CurveData::Circ(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn ellipse(&self) -> Result<Elips2d> {
        match &self.data {
            CurveData::Elips(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn hyperbola(&self) -> Result<Hypr2d> {
        match &self.data {
            CurveData::Hypr(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn parabola(&self) -> Result<Parab2d> {
        match &self.data {
            CurveData::Parab(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }

    fn degree(&self) -> Result<i32> {
        match &self.data {
            CurveData::Bezier(d) => Ok(d.curve.degree()),
            CurveData::BSpline(d) => Ok(d.curve.degree()),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn is_rational(&self) -> Result<bool> {
        match &self.data {
            CurveData::BSpline(d) => Ok(d.curve.is_rational()),
            CurveData::Bezier(d) => Ok(d.curve.is_rational()),
            _ => Ok(false),
        }
    }
    fn nb_poles(&self) -> Result<i32> {
        match &self.data {
            CurveData::Bezier(d) => Ok(d.curve.nb_poles()),
            CurveData::BSpline(d) => Ok(d.curve.nb_poles()),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn nb_knots(&self) -> Result<i32> {
        match &self.data {
            CurveData::BSpline(d) => Ok(d.curve.nb_knots()),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn bezier(&self) -> Result<Option<Arc<BezierCurve>>> {
        Ok(match &self.data {
            CurveData::Bezier(d) => Some(d.curve.clone()),
            _ => None,
        })
    }
    fn bspline(&self) -> Result<Option<Arc<BSplineCurve>>> {
        Ok(match &self.data {
            CurveData::BSpline(d) => Some(d.curve.clone()),
            _ => None,
        })
    }

    fn nb_samples(&self) -> Result<i32> {
        Ok(nb_points(self.cur()?.as_ref()))
    }

    fn is_geom2d_adaptor(&self) -> bool {
        true
    }
    fn geom2d_curve(&self) -> Option<Arc<dyn Curve2d>> {
        self.curve.clone()
    }
}

impl Curve {
    /// `ShallowCopy` with the concrete type.
    fn shallow_copied(&self) -> Result<Curve> {
        let mut copy = Curve::new();
        copy.curve = self.curve.clone();
        copy.type_curve = self.type_curve;
        copy.first = self.first;
        copy.last = self.last;
        copy.data = match &self.data {
            CurveData::Offset(d) => {
                let basis = match &d.basis {
                    Some(b) => Some(Arc::new(b.shallow_copied()?)),
                    None => None,
                };
                CurveData::Offset(OffsetData { basis, offset: d.offset, has_eval_rep: d.has_eval_rep })
            }
            CurveData::BSpline(d) => CurveData::BSpline(BSplineData {
                curve: d.curve.clone(),
                cache: Mutex::new(None),
                has_eval_rep: d.has_eval_rep,
            }),
            CurveData::Bezier(d) => CurveData::Bezier(BezierData {
                curve: d.curve.clone(),
                cache: Mutex::new(None),
                has_eval_rep: d.has_eval_rep,
            }),
            CurveData::None => CurveData::None,
            CurveData::Lin(x) => CurveData::Lin(*x),
            CurveData::Circ(x) => CurveData::Circ(*x),
            CurveData::Elips(x) => CurveData::Elips(*x),
            CurveData::Hypr(x) => CurveData::Hypr(*x),
            CurveData::Parab(x) => CurveData::Parab(*x),
        };
        Ok(copy)
    }
}

// the static nbPoints of Geom2dAdaptor_Curve.cxx
fn nb_points(c: &dyn Curve2d) -> i32 {
    let mut nbs = 20;
    let any = c.as_any();
    if any.downcast_ref::<Line>().is_some() {
        nbs = 2;
    } else if let Some(b) = any.downcast_ref::<BezierCurve>() {
        nbs = 3 + b.nb_poles();
    } else if let Some(b) = any.downcast_ref::<BSplineCurve>() {
        nbs = b.nb_knots();
        nbs *= b.degree();
        if nbs < 2 {
            nbs = 2;
        }
    } else if let Some(o) = any.downcast_ref::<OffsetCurve>() {
        let basis = o.basis_curve();
        return std::cmp::max(nbs, nb_points(basis.as_ref()));
    } else if let Some(t) = any.downcast_ref::<TrimmedCurve>() {
        let basis = t.basis_curve();
        return std::cmp::max(nbs, nb_points(basis.as_ref()));
    }
    if nbs > 300 {
        nbs = 300;
    }
    nbs
}

/// `Geom2dAdaptor::MakeCurve`: the 2D curve equivalent to an adaptor (trimmed to its range when needed). `OtherCurve`
/// adaptors and offset adaptors that are not a [`Curve`] are `Err(Domain)`.
pub fn make_curve(hc: &dyn Curve2dAdaptor) -> Result<Arc<dyn Curve2d>> {
    let mut c2d: Option<Arc<dyn Curve2d>> = match hc.get_type()? {
        GeomAbsCurveType::Line => Some(Arc::new(Line::from_lin2d(hc.line()?))),
        GeomAbsCurveType::Circle => Some(Arc::new(Circle::from_circ2d(hc.circle()?))),
        GeomAbsCurveType::Ellipse => Some(Arc::new(Ellipse::from_elips2d(hc.ellipse()?))),
        GeomAbsCurveType::Parabola => Some(Arc::new(Parabola::from_parab2d(hc.parabola()?))),
        GeomAbsCurveType::Hyperbola => Some(Arc::new(Hyperbola::from_hypr2d(hc.hyperbola()?))),
        GeomAbsCurveType::BezierCurve => hc.bezier()?.map(|b| b as Arc<dyn Curve2d>),
        GeomAbsCurveType::BSplineCurve => hc.bspline()?.map(|b| b as Arc<dyn Curve2d>),
        GeomAbsCurveType::OffsetCurve => match hc.geom2d_curve() {
            Some(c) => Some(c),
            None => return Err(AdaptorError::Domain),
        },
        _ => return Err(AdaptorError::Domain),
    };
    // trim the curve if necessary
    if let Some(c) = &c2d {
        let (hf, hl) = (hc.first_parameter()?, hc.last_parameter()?);
        if hf != c.first_parameter() || hl != c.last_parameter() {
            let trimmed = if c.is_periodic() || (hf >= c.first_parameter() && hl <= c.last_parameter()) {
                TrimmedCurve::new(c.clone(), hf, hl, true, true)?
            } else {
                let tf = cmax(hf, c.first_parameter());
                let tl = cmin(hl, c.last_parameter());
                TrimmedCurve::new(c.clone(), tf, tl, true, true)?
            };
            c2d = Some(Arc::new(trimmed));
        }
    }
    c2d.ok_or(AdaptorError::NullObject)
}
