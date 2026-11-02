//! `GeomAdaptor_Curve`: the adaptor over a `Geom_Curve` (elementary curves evaluated by `ElCLib`, Bezier and B-spline curves
//! by a `BSplCLib_Cache`, offset curves through a nested adaptor of the basis curve).
//!
//! Translated from `GeomAdaptor_Curve.hxx/.cxx` (OCCT 8.0.1). The C++ `std::variant` of curve-specific data is the private
//! enum `CurveData`; the mutable `BSplCLib_Cache` handles are `Mutex<Option<Cache>>`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use super::Result;
use super::utils;
use crate::adaptor2d::{AdaptorError, GeomAbsCurveType, GeomAbsShape};
use crate::adaptor3d::Curve3dAdaptor;
use crate::bsplclib::flat::locate_core;
use crate::bsplclib_cache::{Cache, flat_bezier_knots, intervals};
use crate::elclib;
use crate::geom::{
    BSplineCurve, BezierCurve, Circle, Curve as GeomCurve, Ellipse, Hyperbola, Line, OffsetCurve, Parabola, ResD1,
    ResD2, ResD3, TrimmedCurve, confusion, pconfusion,
};
use crate::gp::{Circ, Dir, Elips, Hypr, Lin, Parab, Pnt, Vec};
use std::sync::{Arc, Mutex};

/// `std::max(a, b)` / `std::min(a, b)` on doubles.
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `PosTol = Precision::PConfusion() / 2`.
fn pos_tol() -> f64 {
    pconfusion() / 2.0
}

fn bspl(e: crate::bsplclib::BSplError) -> AdaptorError {
    AdaptorError::Geom3d(crate::geom::GeomError::BSpl(e))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn rd1<T: Copy>(a: &[T], i: i32) -> Result<T> {
    usize::try_from(i - 1).ok().and_then(|k| a.get(k)).copied().ok_or(AdaptorError::OutOfRange)
}

/// Offset curve data of the adaptor (`GeomAdaptor_Curve::OffsetData`).
#[derive(Debug, Clone)]
struct OffsetData {
    basis: Arc<Curve>,
    offset: f64,
    direction: Dir,
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
    Lin(Lin),
    Circ(Circ),
    Elips(Elips),
    Hypr(Hypr),
    Parab(Parab),
    Offset(OffsetData),
    Bezier(BezierData),
    BSpline(BSplineData),
}

/// `GeomAdaptor_Curve`.
#[derive(Debug)]
pub struct Curve {
    curve: Option<Arc<dyn GeomCurve>>,
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
    /// `GeomAdaptor_Curve()`: no curve, type `OtherCurve`, range 0, 0.
    pub fn new() -> Self {
        Self { curve: None, type_curve: GeomAbsCurveType::OtherCurve, first: 0.0, last: 0.0, data: CurveData::None }
    }

    /// `GeomAdaptor_Curve(C)`: `Load(C)`.
    pub fn with_curve(c: Arc<dyn GeomCurve>) -> Result<Self> {
        let mut a = Self::new();
        a.load(c)?;
        Ok(a)
    }

    /// `GeomAdaptor_Curve(C, UFirst, ULast)`: `Load(C, UFirst, ULast)`.
    pub fn with_range(c: Arc<dyn GeomCurve>, u_first: f64, u_last: f64) -> Result<Self> {
        let mut a = Self::new();
        a.load_range(c, u_first, u_last)?;
        Ok(a)
    }

    /// `GeomAdaptor_Curve(C)` with a possibly null handle: a null handle is `Err(NullObject)`.
    pub fn with_curve_opt(c: Option<Arc<dyn GeomCurve>>) -> Result<Self> {
        let mut a = Self::new();
        a.load_opt(c)?;
        Ok(a)
    }

    /// `GeomAdaptor_Curve(C, UFirst, ULast)` with a possibly null handle.
    pub fn with_range_opt(c: Option<Arc<dyn GeomCurve>>, u_first: f64, u_last: f64) -> Result<Self> {
        let mut a = Self::new();
        a.load_range_opt(c, u_first, u_last)?;
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

    /// `Load(C)` with a possibly null handle (`Standard_NullObject`).
    pub fn load_opt(&mut self, c: Option<Arc<dyn GeomCurve>>) -> Result<()> {
        let c = c.ok_or(AdaptorError::NullObject)?;
        self.load(c)
    }

    /// `Load(C, UFirst, ULast)` with a possibly null handle.
    pub fn load_range_opt(&mut self, c: Option<Arc<dyn GeomCurve>>, u_first: f64, u_last: f64) -> Result<()> {
        let c = c.ok_or(AdaptorError::NullObject)?;
        self.load_range(c, u_first, u_last)
    }

    /// `Load(C)`: the whole parameter range of the curve.
    pub fn load(&mut self, c: Arc<dyn GeomCurve>) -> Result<()> {
        let (f, l) = (c.first_parameter(), c.last_parameter());
        self.load_impl(c, f, l)
    }

    /// `Load(C, UFirst, ULast)`: `ConstructionError` if `UFirst > ULast + Precision::Confusion()`.
    pub fn load_range(&mut self, c: Arc<dyn GeomCurve>, u_first: f64, u_last: f64) -> Result<()> {
        if u_first > u_last + confusion() {
            return Err(AdaptorError::Construction);
        }
        self.load_impl(c, u_first, u_last)
    }

    /// `Curve`.
    pub fn curve(&self) -> Option<&Arc<dyn GeomCurve>> {
        self.curve.as_ref()
    }

    fn cur(&self) -> Result<&Arc<dyn GeomCurve>> {
        self.curve.as_ref().ok_or(AdaptorError::NullObject)
    }

    // GeomAdaptor_Curve::load
    fn load_impl(&mut self, c: Arc<dyn GeomCurve>, u_first: f64, u_last: f64) -> Result<()> {
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
                self.data = CurveData::Circ(x.circ());
            } else if let Some(x) = any.downcast_ref::<Line>() {
                self.type_curve = GeomAbsCurveType::Line;
                self.data = CurveData::Lin(x.lin());
            } else if let Some(x) = any.downcast_ref::<Ellipse>() {
                self.type_curve = GeomAbsCurveType::Ellipse;
                self.data = CurveData::Elips(x.elips());
            } else if let Some(x) = any.downcast_ref::<Parabola>() {
                self.type_curve = GeomAbsCurveType::Parabola;
                self.data = CurveData::Parab(x.parab());
            } else if let Some(x) = any.downcast_ref::<Hyperbola>() {
                self.type_curve = GeomAbsCurveType::Hyperbola;
                self.data = CurveData::Hypr(x.hypr());
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
                // a nested adaptor for the whole basis curve and the offset data
                let basis = Curve::with_curve(x.basis_curve())?;
                self.data = CurveData::Offset(OffsetData {
                    basis: Arc::new(basis),
                    offset: x.offset(),
                    direction: x.direction(),
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

    fn bspline_data(&self) -> Result<&Arc<BSplineCurve>> {
        match &self.data {
            CurveData::BSpline(d) => Ok(&d.curve),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }

    // GeomAdaptor_Curve::LocalContinuity
    fn local_continuity(&self, u1: f64, u2: f64) -> Result<GeomAbsShape> {
        let b = self.bspline_data()?;
        let nb = b.nb_knots();
        let tk = b.knots();
        let tm = b.multiplicities();
        let periodic = b.is_periodic();
        let (uf, ul) =
            if periodic { (tk.first().copied().unwrap_or(0.0), tk.last().copied().unwrap_or(1.0)) } else { (0.0, 1.0) };
        let (mut index1, new_first) = locate_core(tk, u1, periodic, 1, nb, uf, ul).map_err(bspl)?;
        let (mut index2, new_last) = locate_core(tk, u2, periodic, 1, nb, uf, ul).map_err(bspl)?;
        if (new_first - rd1(tk, index1 + 1)?).abs() < pconfusion() && index1 < nb {
            index1 += 1;
        }
        if (new_last - rd1(tk, index2)?).abs() < pconfusion() {
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
            let mut m = rd1(tm, index1 + 1)?;
            for i in (index1 + 1)..=index2 {
                if rd1(tm, i)? > m {
                    m = rd1(tm, i)?;
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

    // GeomAdaptor_Curve::RebuildCache
    fn rebuild_cache(&self, parameter: f64) -> Result<()> {
        match &self.data {
            CurveData::Bezier(d) => {
                let b = &d.curve;
                let deg = b.degree();
                let flat = flat_bezier_knots(deg).map_err(bspl)?;
                let mut g = lock(&d.cache);
                if g.is_none() {
                    *g = Some(Cache::new_3d(deg, b.is_periodic(), &flat, b.weights()).map_err(bspl)?);
                }
                if let Some(c) = g.as_mut() {
                    c.build_cache_3d(parameter, &flat, b.poles(), b.weights()).map_err(bspl)?;
                }
            }
            CurveData::BSpline(d) => {
                let b = &d.curve;
                let mut g = lock(&d.cache);
                if g.is_none() {
                    *g =
                        Some(Cache::new_3d(b.degree(), b.is_periodic(), b.knot_sequence(), b.weights()).map_err(bspl)?);
                }
                if let Some(c) = g.as_mut() {
                    c.build_cache_3d(parameter, b.knot_sequence(), b.poles(), b.weights()).map_err(bspl)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // GeomAdaptor_Curve::IsBoundary: the span of the first or the last parameter of a B-spline
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

    fn offset_data(&self) -> Result<&OffsetData> {
        match &self.data {
            CurveData::Offset(d) => Ok(d),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }

    fn offset_geom(&self) -> Result<&OffsetCurve> {
        self.cur()?.as_any().downcast_ref::<OffsetCurve>().ok_or(AdaptorError::NoSuchObject)
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

    /// `Some(nb + 1)` when `Intervals` replaces the array it is given by one of exactly that size (`BSplCLib::Intervals` does
    /// so; the other paths write into the array as it is), else `None`.
    pub fn intervals_replace_array(&self, s: GeomAbsShape) -> Result<Option<usize>> {
        if self.type_curve != GeomAbsCurveType::BSplineCurve {
            return Ok(None);
        }
        match self.spline_intervals(s, false)? {
            None => Ok(None),
            Some((n, _)) => Ok(Some(usize::try_from(n + 1).map_err(|_| AdaptorError::OutOfRange)?)),
        }
    }

    fn spline_intervals(&self, s: GeomAbsShape, want: bool) -> Result<Option<(i32, Option<std::vec::Vec<f64>>)>> {
        let b = self.bspline_data()?;
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

    /// The breaks of the basis curve of an offset curve that are strictly inside the range of the adaptor
    /// (`NbIntervals` and `Intervals` of an offset curve): (the basis adaptor, its shape, its intervals).
    fn offset_inner_breaks(&self, s: GeomAbsShape) -> Result<std::vec::Vec<f64>> {
        let base_s = Self::offset_base_shape(s)?;
        let off = self.offset_geom()?;
        let c = Curve::with_range(off.basis_curve(), self.first, self.last)?;
        let nb = c.nb_intervals(base_s)?;
        let mut inner = std::vec::Vec::new();
        if nb > 1 {
            let mut r = vec![0.0; usize::try_from(nb).map_err(|_| AdaptorError::OutOfRange)? + 1];
            c.intervals(&mut r, base_s)?;
            for i_int in 1..=nb {
                let v = rd1(&r, i_int)?;
                if v > self.first && v < self.last {
                    inner.push(v);
                }
            }
        }
        Ok(inner)
    }

    /// `ShallowCopy` with the concrete type.
    pub fn shallow_copied(&self) -> Result<Curve> {
        let mut copy = Curve::new();
        copy.curve = self.curve.clone();
        copy.type_curve = self.type_curve;
        copy.first = self.first;
        copy.last = self.last;
        copy.data = match &self.data {
            CurveData::Offset(d) => CurveData::Offset(OffsetData {
                basis: Arc::new(d.basis.shallow_copied()?),
                offset: d.offset,
                direction: d.direction,
                has_eval_rep: d.has_eval_rep,
            }),
            CurveData::BSpline(d) => CurveData::BSpline(BSplineData {
                curve: d.curve.clone(),
                cache: Mutex::new(None),
                has_eval_rep: d.has_eval_rep,
            }),
            CurveData::Bezier(d) => {
                // the cache is not copied: it is rebuilt on demand
                CurveData::Bezier(BezierData {
                    curve: d.curve.clone(),
                    cache: Mutex::new(None),
                    has_eval_rep: d.has_eval_rep,
                })
            }
            CurveData::None => CurveData::None,
            CurveData::Lin(x) => CurveData::Lin(*x),
            CurveData::Circ(x) => CurveData::Circ(*x),
            CurveData::Elips(x) => CurveData::Elips(*x),
            CurveData::Hypr(x) => CurveData::Hypr(*x),
            CurveData::Parab(x) => CurveData::Parab(*x),
        };
        Ok(copy)
    }

    /// `Trim` with the concrete type: `new GeomAdaptor_Curve(myCurve, First, Last)`.
    pub fn trimmed(&self, first: f64, last: f64, _tol: f64) -> Result<Curve> {
        let c = self.cur()?.clone();
        Curve::with_range(c, first, last)
    }
}

impl Curve3dAdaptor for Curve {
    fn shallow_copy(&self) -> Result<Arc<dyn Curve3dAdaptor>> {
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
            let off = self.offset_geom()?;
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
            Ok(match self.spline_intervals(s, false)? {
                None => 1,
                Some((n, _)) => n,
            })
        } else if self.type_curve == GeomAbsCurveType::OffsetCurve {
            let inner = self.offset_inner_breaks(s)?;
            Ok(1 + i32::try_from(inner.len()).map_err(|_| AdaptorError::OutOfRange)?)
        } else {
            Ok(1)
        }
    }

    fn intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        fn put(t: &mut [f64], k: usize, v: f64) -> Result<()> {
            *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = v;
            Ok(())
        }
        if self.type_curve == GeomAbsCurveType::BSplineCurve {
            match self.spline_intervals(s, true)? {
                None => {
                    put(t, 0, self.first)?;
                    put(t, 1, self.last)?;
                }
                Some((_, v)) => {
                    for (k, x) in v.ok_or(AdaptorError::OutOfRange)?.into_iter().enumerate() {
                        put(t, k, x)?;
                    }
                }
            }
        } else if self.type_curve == GeomAbsCurveType::OffsetCurve {
            let inner = self.offset_inner_breaks(s)?;
            // `T(++myNbIntervals) = ...`: the absolute index of an array that starts at 1
            for (k, x) in inner.iter().enumerate() {
                put(t, k + 1, *x)?;
            }
            put(t, 0, self.first)?;
            put(t, inner.len() + 1, self.last)?;
        } else {
            put(t, 0, self.first)?;
            put(t, 1, self.last)?;
        }
        Ok(())
    }

    fn trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn Curve3dAdaptor>> {
        Ok(Arc::new(self.trimmed(first, last, tol)?))
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

    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        match &self.data {
            CurveData::Lin(x) => Ok(elclib::value_lin(u, x)),
            CurveData::Circ(x) => Ok(elclib::value_circ(u, x)),
            CurveData::Elips(x) => Ok(elclib::value_elips(u, x)),
            CurveData::Hypr(x) => Ok(elclib::value_hypr(u, x)),
            CurveData::Parab(x) => Ok(elclib::value_parab(u, x)),
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d0(u)?);
                }
                self.with_cache(u, |c| c.d0_3d(u))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d0(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(d.curve.local_d0(u, start, finish)?)
                } else {
                    self.with_cache(u, |c| c.d0_3d(u))
                }
            }
            CurveData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d0(u)?);
                }
                utils::offset_d0(u, d.basis.as_ref(), &d.direction, d.offset)?.ok_or(AdaptorError::UndefinedValue)
            }
            CurveData::None => Ok(self.cur()?.eval_d0(u)?),
        }
    }

    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let r = |(point, d1): (Pnt, Vec)| ResD1 { point, d1 };
        match &self.data {
            CurveData::Lin(x) => Ok(r(elclib::d1_lin(u, x))),
            CurveData::Circ(x) => Ok(r(elclib::d1_circ(u, x))),
            CurveData::Elips(x) => Ok(r(elclib::d1_elips(u, x))),
            CurveData::Hypr(x) => Ok(r(elclib::d1_hypr(u, x))),
            CurveData::Parab(x) => Ok(r(elclib::d1_parab(u, x))),
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d1(u)?);
                }
                Ok(r(self.with_cache(u, |c| c.d1_3d(u))?))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d1(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(r(d.curve.local_d1(u, start, finish)?))
                } else {
                    Ok(r(self.with_cache(u, |c| c.d1_3d(u))?))
                }
            }
            CurveData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d1(u)?);
                }
                utils::offset_d1(u, d.basis.as_ref(), &d.direction, d.offset)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_d1(u)?),
        }
    }

    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let r = |(point, d1, d2): (Pnt, Vec, Vec)| ResD2 { point, d1, d2 };
        match &self.data {
            CurveData::Lin(x) => {
                let (point, d1) = elclib::d1_lin(u, x);
                Ok(ResD2 { point, d1, d2: Vec::new(0.0, 0.0, 0.0) })
            }
            CurveData::Circ(x) => Ok(r(elclib::d2_circ(u, x))),
            CurveData::Elips(x) => Ok(r(elclib::d2_elips(u, x))),
            CurveData::Hypr(x) => Ok(r(elclib::d2_hypr(u, x))),
            CurveData::Parab(x) => Ok(r(elclib::d2_parab(u, x))),
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d2(u)?);
                }
                Ok(r(self.with_cache(u, |c| c.d2_3d(u))?))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d2(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(r(d.curve.local_d2(u, start, finish)?))
                } else {
                    Ok(r(self.with_cache(u, |c| c.d2_3d(u))?))
                }
            }
            CurveData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d2(u)?);
                }
                utils::offset_d2(u, d.basis.as_ref(), &d.direction, d.offset)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_d2(u)?),
        }
    }

    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let r = |(point, d1, d2, d3): (Pnt, Vec, Vec, Vec)| ResD3 { point, d1, d2, d3 };
        let zero = Vec::new(0.0, 0.0, 0.0);
        match &self.data {
            CurveData::Lin(x) => {
                let (point, d1) = elclib::d1_lin(u, x);
                Ok(ResD3 { point, d1, d2: zero, d3: zero })
            }
            CurveData::Circ(x) => Ok(r(elclib::d3_circ(u, x))),
            CurveData::Elips(x) => Ok(r(elclib::d3_elips(u, x))),
            CurveData::Hypr(x) => Ok(r(elclib::d3_hypr(u, x))),
            CurveData::Parab(x) => {
                let (point, d1, d2) = elclib::d2_parab(u, x);
                Ok(ResD3 { point, d1, d2, d3: zero })
            }
            CurveData::Bezier(_) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d3(u)?);
                }
                Ok(r(self.with_cache(u, |c| c.d3_3d(u))?))
            }
            CurveData::BSpline(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d3(u)?);
                }
                if let Some((start, finish)) = self.is_boundary(u) {
                    Ok(r(d.curve.local_d3(u, start, finish)?))
                } else {
                    Ok(r(self.with_cache(u, |c| c.d3_3d(u))?))
                }
            }
            CurveData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_d3(u)?);
                }
                utils::offset_d3(u, d.basis.as_ref(), &d.direction, d.offset)?.ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_d3(u)?),
        }
    }

    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec> {
        match &self.data {
            CurveData::Lin(x) => Ok(elclib::dn_lin(u, x, n)),
            CurveData::Circ(x) => Ok(elclib::dn_circ(u, x, n)),
            CurveData::Elips(x) => Ok(elclib::dn_elips(u, x, n)),
            CurveData::Hypr(x) => Ok(elclib::dn_hypr(u, x, n)),
            CurveData::Parab(x) => Ok(elclib::dn_parab(u, x, n)),
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
            CurveData::Offset(d) => {
                if self.has_eval_rep() {
                    return Ok(self.cur()?.eval_dn(u, n)?);
                }
                utils::offset_dn(u, d.basis.as_ref(), &d.direction, d.offset, n)?
                    .ok_or(AdaptorError::UndefinedDerivative)
            }
            CurveData::None => Ok(self.cur()?.eval_dn(u, n)?),
        }
    }

    fn resolution(&self, r3d: f64) -> Result<f64> {
        match &self.data {
            CurveData::Lin(_) => Ok(r3d),
            CurveData::Circ(x) => {
                let r = x.radius();
                if r > r3d / 2.0 { Ok(2.0 * libm::asin(r3d / (2.0 * r))) } else { Ok(2.0 * std::f64::consts::PI) }
            }
            CurveData::Elips(x) => Ok(r3d / x.major_radius()),
            CurveData::Bezier(d) => Ok((*d.curve).clone().resolution(r3d)),
            CurveData::BSpline(d) => Ok((*d.curve).clone().resolution(r3d)),
            _ => Ok(r3d * 0.01), // Precision::Parametric(R3D)
        }
    }

    fn get_type(&self) -> Result<GeomAbsCurveType> {
        Ok(self.type_curve)
    }

    fn line(&self) -> Result<Lin> {
        match &self.data {
            CurveData::Lin(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn circle(&self) -> Result<Circ> {
        match &self.data {
            CurveData::Circ(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn ellipse(&self) -> Result<Elips> {
        match &self.data {
            CurveData::Elips(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn hyperbola(&self) -> Result<Hypr> {
        match &self.data {
            CurveData::Hypr(x) => Ok(*x),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn parabola(&self) -> Result<Parab> {
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
        match &self.data {
            CurveData::Bezier(d) => Ok(Some(d.curve.clone())),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn bspline(&self) -> Result<Option<Arc<BSplineCurve>>> {
        match &self.data {
            CurveData::BSpline(d) => Ok(Some(d.curve.clone())),
            _ => Err(AdaptorError::NoSuchObject),
        }
    }
    fn offset_curve(&self) -> Result<Option<Arc<OffsetCurve>>> {
        if self.type_curve != GeomAbsCurveType::OffsetCurve {
            return Err(AdaptorError::NoSuchObject);
        }
        let off = self.offset_geom()?;
        let d = self.offset_data()?;
        let _ = d;
        Ok(Some(Arc::new(off.clone())))
    }
}
