//! The parts of OCCT's `BSplCLib` that the curve adaptors need and that `bsplclib` does not have: the class
//! `BSplCLib_Cache` with its `BSplCLib_CacheParams` (a Taylor expansion of one span of a Bezier or B-spline curve, 2D or
//! 3D), `BSplCLib::Intervals` and `BSplCLib::FlatBezierKnots`. The span-index overload of `BSplCLib::BuildCache` is
//! `bsplclib::curve2d::build_cache_span` / `bsplclib::curve3d::build_cache_span`.
//!
//! Translated from `BSplCLib_Cache.hxx/.cxx`, `BSplCLib_CacheParams.hxx` and `BSplCLib.cxx` (OCCT 8.0.1). Arrays are
//! slices; the C++ arrays all have lower bound 1 (flat knots, knots, multiplicities), and an item that C++ would read
//! outside its array (undefined behaviour in the unchecked build) is an `Err`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::bsplclib::BSplError;
use crate::bsplclib::curve2d::build_cache_span as build_cache_span_2d;
use crate::bsplclib::curve3d::build_cache_span as build_cache_span_3d;
use crate::bsplclib::flat::{first_u_knot_index, last_u_knot_index, locate_core, locate_parameter_flat};
use crate::bsplclib::util::epsilon;
use crate::gp::{Pnt, Pnt2d, Vec as Vec3, Vec2d};
use crate::plib;

type R<T> = Result<T, BSplError>;

fn map_plib(e: plib::PLibError) -> BSplError {
    BSplError::PLib(e)
}

/// Reads the 1-based item `i` of `a`.
fn rd1<T: Copy>(a: &[T], i: i32) -> R<T> {
    usize::try_from(i - 1).ok().and_then(|k| a.get(k)).copied().ok_or(BSplError::OutOfRange)
}

/// `BSplCLib::FlatBezierKnots(Degree)` as an array: `degree + 1` zeros then `degree + 1` ones (`1 <= degree <= 25`).
pub fn flat_bezier_knots(degree: i32) -> R<Vec<f64>> {
    if !(1..=25).contains(&degree) {
        return Err(BSplError::OutOfRange);
    }
    let n = usize::try_from(degree + 1).map_err(|_| BSplError::OutOfRange)?;
    let mut v = vec![0.0; n];
    v.extend(std::iter::repeat_n(1.0, n));
    Ok(v)
}

/// `BSplCLib::Intervals`: the number of parameter intervals of `[first, last]` where a spline of the given `knots`,
/// `mults` and `degree` is `continuity` times continuously differentiable, and (when `want_intervals`) their bounds
/// (`number + 1` values, the first being `first` and the last `last`).
#[allow(clippy::too_many_arguments)]
pub fn intervals(
    knots: &[f64],
    mults: &[i32],
    degree: i32,
    periodic: bool,
    continuity: i32,
    first: f64,
    last: f64,
    tolerance: f64,
    want_intervals: bool,
) -> R<(i32, Option<Vec<f64>>)> {
    // remove all knots with multiplicity less or equal than (degree - continuity) except first and last
    let nb_knots = i32::try_from(knots.len()).map_err(|_| BSplError::OutOfRange)?;
    let first_index = if periodic { 1 } else { first_u_knot_index(degree, mults)? };
    let last_index = if periodic { nb_knots } else { last_u_knot_index(degree, mults)? };
    let mut new_knots = Vec::new();
    for index in first_index..=last_index {
        if rd1(mults, index)? > degree - continuity || index == first_index || index == last_index {
            new_knots.push(rd1(knots, index)?);
        }
    }
    let nb_new = i32::try_from(new_knots.len()).map_err(|_| BSplError::OutOfRange)?;

    // the range boundaries
    let mut cur_first = first;
    let mut cur_last = last;
    let mut period = 0.0;
    let mut first_period = 0;
    let mut last_period = 0;
    // move boundaries into period
    if periodic {
        let lower = *knots.first().ok_or(BSplError::OutOfRange)?;
        let upper = *knots.last().ok_or(BSplError::OutOfRange)?;
        period = upper - lower;
        while cur_first < lower {
            cur_first += period;
            first_period -= 1;
        }
        while cur_last < lower {
            cur_last += period;
            last_period -= 1;
        }
        while cur_first >= upper {
            cur_first -= period;
            first_period += 1;
        }
        while cur_last >= upper {
            cur_last -= period;
            last_period += 1;
        }
    }
    // locate the left and nearest knot for the boundaries (the version that does not need multiplicities)
    let (mut index1, _) = locate_core(&new_knots, cur_first, false, 1, nb_new, 0.0, 1.0)?;
    let (mut index2, _) = locate_core(&new_knots, cur_last, false, 1, nb_new, 0.0, 1.0)?;
    // the beginning of the range coincides with the next knot
    if index1 < nb_new && (rd1(&new_knots, index1 + 1)? - cur_first).abs() < tolerance {
        index1 += 1;
    }
    // the ending of the range coincides with the current knot
    if nb_new != 0 && (rd1(&new_knots, index2)? - cur_last).abs() < tolerance {
        index2 -= 1;
    }
    let nb_intervals = index2 - index1 + 1 + (last_period - first_period) * (nb_new - 1);
    if !want_intervals {
        return Ok((nb_intervals, None));
    }
    // fill the interval array
    let len = usize::try_from(nb_intervals + 1).map_err(|_| BSplError::OutOfRange)?;
    let mut out = vec![0.0; len];
    let mut put = |k: usize, v: f64| -> R<()> {
        *out.get_mut(k).ok_or(BSplError::OutOfRange)? = v;
        Ok(())
    };
    if periodic && last_period != first_period {
        let mut idx = 0usize;
        // part from the beginning of the range to the end of the first period
        for i in index1..nb_new {
            put(idx, rd1(&new_knots, i)? + f64::from(first_period) * period)?;
            idx += 1;
        }
        // full periods
        for period_num in (first_period + 1)..last_period {
            for i in 1..nb_new {
                put(idx, rd1(&new_knots, i)? + f64::from(period_num) * period)?;
                idx += 1;
            }
        }
        // part from the beginning of the last period to the end of the range
        for i in 1..=index2 {
            put(idx, rd1(&new_knots, i)? + f64::from(last_period) * period)?;
            idx += 1;
        }
    } else {
        for (idx, i) in (index1..=index2).enumerate() {
            put(idx, rd1(&new_knots, i)? + f64::from(first_period) * period)?;
        }
    }
    // the beginning of the range does not coincide with the knot at index1 in general
    put(0, first)?;
    // the ending of the range was not written at all
    put(len - 1, last)?;
    Ok((nb_intervals, Some(out)))
}

/// `BSplCLib_CacheParams`: the span bookkeeping of a cache.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheParams {
    /// Degree of the spline.
    pub degree: i32,
    /// Whether the spline is periodic.
    pub is_periodic: bool,
    /// First parameter of the knot sequence.
    pub first_parameter: f64,
    /// Last parameter of the knot sequence.
    pub last_parameter: f64,
    /// Minimal index of a span.
    pub span_index_min: i32,
    /// Maximal index of a span.
    pub span_index_max: i32,
    /// Parameter of the first point of the span.
    pub span_start: f64,
    /// Length of the span.
    pub span_length: f64,
    /// Index of the span.
    pub span_index: i32,
}

impl CacheParams {
    /// `BSplCLib_CacheParams(Degree, Periodic, FlatKnots)` (flat knots with lower bound 1).
    pub fn new(degree: i32, periodic: bool, flat_knots: &[f64]) -> R<Self> {
        let n = i32::try_from(flat_knots.len()).map_err(|_| BSplError::OutOfRange)?;
        Ok(Self {
            degree,
            is_periodic: periodic,
            first_parameter: rd1(flat_knots, 1 + degree)?,
            last_parameter: rd1(flat_knots, n - degree)?,
            span_index_min: 1 + degree,
            span_index_max: n - degree - 1,
            span_start: 0.0,
            span_length: 0.0,
            span_index: 0,
        })
    }

    /// `PeriodicNormalization`: the parameter moved into the knot range of a periodic spline.
    pub fn periodic_normalization(&self, parameter: f64) -> f64 {
        if self.is_periodic {
            let period = self.last_parameter - self.first_parameter;
            if parameter < self.first_parameter {
                let scale = ((self.first_parameter - parameter) / period).trunc();
                return parameter + period * (scale + 1.0);
            }
            if parameter > self.last_parameter {
                let scale = ((parameter - self.last_parameter) / period).trunc();
                return parameter - period * (scale + 1.0);
            }
        }
        parameter
    }

    /// `IsCacheValid`: whether the parameter lies in the cached span.
    pub fn is_cache_valid(&self, parameter: f64) -> bool {
        let new_param = self.periodic_normalization(parameter);
        let delta = new_param - self.span_start;
        if (delta < 0.0 && self.span_index != self.span_index_min)
            || (delta >= self.span_length && self.span_index != self.span_index_max)
        {
            return false;
        }
        if self.span_index == self.span_index_max {
            return true;
        }
        // from BSplCLib::LocateParameter() check hitting of the next knot within double floating point precision
        let eps = epsilon(cmin(self.last_parameter.abs(), new_param.abs()));
        let delta_to_next = (delta - self.span_length).abs();
        delta_to_next > eps // the next knot should be used instead
    }

    /// `LocateParameter`: finds the span of `*parameter` (which receives the parameter moved into the span range).
    pub fn locate_parameter(&mut self, parameter: &mut f64, flat_knots: &[f64]) -> R<()> {
        self.span_index = 0;
        let (index, new_u) =
            locate_parameter_flat(self.degree, flat_knots, *parameter, self.is_periodic, self.span_index)?;
        self.span_index = index;
        *parameter = new_u;
        self.span_start = rd1(flat_knots, self.span_index)?;
        self.span_length = rd1(flat_knots, self.span_index + 1)? - self.span_start;
        Ok(())
    }
}

/// `std::min(a, b)`.
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `BSplCLib_Cache`: the Taylor expansion of one span of a spline, for fast evaluation. A cache is made for 2D or 3D
/// poles ([`new_2d`](Self::new_2d), [`new_3d`](Self::new_3d)) and used with the matching methods.
#[derive(Debug, Clone)]
pub struct Cache {
    is_rational: bool,
    params: CacheParams,
    row_length: usize,
    buffer: [f64; 128],
}

impl Cache {
    fn new(degree: i32, periodic: bool, flat_knots: &[f64], weights: Option<&[f64]>, dim: usize) -> R<Self> {
        let is_rational = weights.is_some();
        Ok(Self {
            is_rational,
            params: CacheParams::new(degree, periodic, flat_knots)?,
            row_length: if is_rational { dim + 1 } else { dim },
            buffer: [0.0; 128],
        })
    }

    /// The constructor for a 2D curve (`weights` given: rational).
    pub fn new_2d(degree: i32, periodic: bool, flat_knots: &[f64], weights: Option<&[f64]>) -> R<Self> {
        Self::new(degree, periodic, flat_knots, weights, 2)
    }

    /// The constructor for a 3D curve (`weights` given: rational).
    pub fn new_3d(degree: i32, periodic: bool, flat_knots: &[f64], weights: Option<&[f64]>) -> R<Self> {
        Self::new(degree, periodic, flat_knots, weights, 3)
    }

    /// `IsCacheValid`.
    pub fn is_cache_valid(&self, parameter: f64) -> bool {
        self.params.is_cache_valid(parameter)
    }

    /// The parameters of the cached span.
    pub fn params(&self) -> &CacheParams {
        &self.params
    }

    fn store(&mut self, cache: &[f64]) -> R<()> {
        for (k, v) in cache.iter().enumerate() {
            *self.buffer.get_mut(k).ok_or(BSplError::OutOfRange)? = *v;
        }
        Ok(())
    }

    /// `BuildCache` for a 2D curve: the cache of the span of `parameter`.
    pub fn build_cache_2d(
        &mut self,
        parameter: f64,
        flat_knots: &[f64],
        poles: &[Pnt2d],
        weights: Option<&[f64]>,
    ) -> R<()> {
        let mut new_param = self.params.periodic_normalization(parameter);
        self.params.locate_parameter(&mut new_param, flat_knots)?;
        let cache = build_cache_span_2d(
            self.params.span_start,
            self.params.span_length,
            self.params.is_periodic,
            self.params.degree,
            self.params.span_index,
            flat_knots,
            poles,
            weights,
        )?;
        self.store(&cache)
    }

    /// `BuildCache` for a 3D curve.
    pub fn build_cache_3d(
        &mut self,
        parameter: f64,
        flat_knots: &[f64],
        poles: &[Pnt],
        weights: Option<&[f64]>,
    ) -> R<()> {
        let mut new_param = self.params.periodic_normalization(parameter);
        self.params.locate_parameter(&mut new_param, flat_knots)?;
        let cache = build_cache_span_3d(
            self.params.span_start,
            self.params.span_length,
            self.params.is_periodic,
            self.params.degree,
            self.params.span_index,
            flat_knots,
            poles,
            weights,
        )?;
        self.store(&cache)
    }

    fn local_param(&self, parameter: f64) -> f64 {
        let local = self.params.periodic_normalization(parameter);
        (local - self.params.span_start) / self.params.span_length
    }

    /// `calculateDerivativeLocal`: the point and the derivatives up to `derivative` (`derivative + 1` groups of
    /// `row_length` values, or `row_length - 1` for a rational cache) as a flat array of 16 items.
    fn derivatives_local(&self, local_param: f64, derivative: i32) -> R<[f64; 16]> {
        let dimension = self.row_length;
        let dim_i = i32::try_from(dimension).map_err(|_| BSplError::OutOfRange)?;
        let mut out = [0.0; 16];
        let mut pnt_deriv = [0.0; 16];
        let mut deriv = derivative;
        if !self.is_rational && self.params.degree < derivative {
            deriv = self.params.degree;
        }
        let coeffs = self.buffer.get(..).ok_or(BSplError::OutOfRange)?;
        let evaluated =
            plib::eval_polynomial(local_param, deriv, self.params.degree, dim_i, coeffs).map_err(map_plib)?;
        for (k, v) in evaluated
            .iter()
            .take((usize::try_from(deriv).map_err(|_| BSplError::OutOfRange)? + 1) * dimension)
            .enumerate()
        {
            *pnt_deriv.get_mut(k).ok_or(BSplError::OutOfRange)? = *v;
        }
        // unnormalize the derivatives: they are computed normalized (division by the span length at each order)
        let mut factor = 1.0;
        for d in 1..=usize::try_from(deriv).map_err(|_| BSplError::OutOfRange)? {
            factor /= self.params.span_length;
            for ind in 0..dimension {
                let cell = pnt_deriv.get_mut(dimension * d + ind).ok_or(BSplError::OutOfRange)?;
                *cell *= factor;
            }
        }
        if self.is_rational {
            let rd = plib::rational_derivative(deriv, deriv, dim_i - 1, &pnt_deriv, true).map_err(map_plib)?;
            for (k, v) in rd.iter().enumerate() {
                *out.get_mut(k).ok_or(BSplError::OutOfRange)? = *v;
            }
        } else {
            out = pnt_deriv;
        }
        Ok(out)
    }

    /// The step between derivative groups in [`derivatives_local`](Self::derivatives_local).
    fn dim(&self) -> usize {
        if self.is_rational { self.row_length - 1 } else { self.row_length }
    }

    fn point_local(&self, local_param: f64) -> R<[f64; 4]> {
        let dim_i = i32::try_from(self.row_length).map_err(|_| BSplError::OutOfRange)?;
        let res = plib::no_derivative_eval_polynomial(
            local_param,
            self.params.degree,
            dim_i,
            self.params.degree * dim_i,
            &self.buffer,
        )
        .map_err(map_plib)?;
        let mut p = [0.0; 4];
        for (k, v) in res.iter().take(4).enumerate() {
            *p.get_mut(k).ok_or(BSplError::OutOfRange)? = *v;
        }
        Ok(p)
    }

    fn at(a: &[f64; 16], i: usize) -> R<f64> {
        a.get(i).copied().ok_or(BSplError::OutOfRange)
    }

    /// `D0Local` (2D): the point at the span-local parameter `(u - SpanStart) / SpanLength`.
    pub fn d0_local_2d(&self, local_param: f64) -> R<Pnt2d> {
        let p = self.point_local(local_param)?;
        let mut pt = Pnt2d::new(p[0], p[1]);
        if self.is_rational {
            pt = Pnt2d::new(pt.x() / p[2], pt.y() / p[2]);
        }
        Ok(pt)
    }

    /// `D0Local` (3D).
    pub fn d0_local_3d(&self, local_param: f64) -> R<Pnt> {
        let p = self.point_local(local_param)?;
        let mut pt = Pnt::new(p[0], p[1], p[2]);
        if self.is_rational {
            pt = Pnt::new(pt.x() / p[3], pt.y() / p[3], pt.z() / p[3]);
        }
        Ok(pt)
    }

    /// `D1Local` (2D).
    pub fn d1_local_2d(&self, local_param: f64) -> R<(Pnt2d, Vec2d)> {
        let a = self.derivatives_local(local_param, 1)?;
        let d = self.dim();
        Ok((Pnt2d::new(Self::at(&a, 0)?, Self::at(&a, 1)?), Vec2d::new(Self::at(&a, d)?, Self::at(&a, d + 1)?)))
    }

    /// `D2Local` (2D).
    pub fn d2_local_2d(&self, local_param: f64) -> R<(Pnt2d, Vec2d, Vec2d)> {
        let a = self.derivatives_local(local_param, 2)?;
        let d = self.dim();
        let s = d << 1;
        Ok((
            Pnt2d::new(Self::at(&a, 0)?, Self::at(&a, 1)?),
            Vec2d::new(Self::at(&a, d)?, Self::at(&a, d + 1)?),
            Vec2d::new(Self::at(&a, s)?, Self::at(&a, s + 1)?),
        ))
    }

    /// `D3Local` (2D).
    pub fn d3_local_2d(&self, local_param: f64) -> R<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let a = self.derivatives_local(local_param, 3)?;
        let d = self.dim();
        let s2 = d << 1;
        let s3 = s2 + d;
        Ok((
            Pnt2d::new(Self::at(&a, 0)?, Self::at(&a, 1)?),
            Vec2d::new(Self::at(&a, d)?, Self::at(&a, d + 1)?),
            Vec2d::new(Self::at(&a, s2)?, Self::at(&a, s2 + 1)?),
            Vec2d::new(Self::at(&a, s3)?, Self::at(&a, s3 + 1)?),
        ))
    }

    /// `D1Local` (3D).
    pub fn d1_local_3d(&self, local_param: f64) -> R<(Pnt, Vec3)> {
        let a = self.derivatives_local(local_param, 1)?;
        let d = self.dim();
        Ok((
            Pnt::new(Self::at(&a, 0)?, Self::at(&a, 1)?, Self::at(&a, 2)?),
            Vec3::new(Self::at(&a, d)?, Self::at(&a, d + 1)?, Self::at(&a, d + 2)?),
        ))
    }

    /// `D2Local` (3D).
    pub fn d2_local_3d(&self, local_param: f64) -> R<(Pnt, Vec3, Vec3)> {
        let a = self.derivatives_local(local_param, 2)?;
        let d = self.dim();
        let s = d << 1;
        Ok((
            Pnt::new(Self::at(&a, 0)?, Self::at(&a, 1)?, Self::at(&a, 2)?),
            Vec3::new(Self::at(&a, d)?, Self::at(&a, d + 1)?, Self::at(&a, d + 2)?),
            Vec3::new(Self::at(&a, s)?, Self::at(&a, s + 1)?, Self::at(&a, s + 2)?),
        ))
    }

    /// `D3Local` (3D).
    pub fn d3_local_3d(&self, local_param: f64) -> R<(Pnt, Vec3, Vec3, Vec3)> {
        let a = self.derivatives_local(local_param, 3)?;
        let d = self.dim();
        let s2 = d << 1;
        let s3 = s2 + d;
        Ok((
            Pnt::new(Self::at(&a, 0)?, Self::at(&a, 1)?, Self::at(&a, 2)?),
            Vec3::new(Self::at(&a, d)?, Self::at(&a, d + 1)?, Self::at(&a, d + 2)?),
            Vec3::new(Self::at(&a, s2)?, Self::at(&a, s2 + 1)?, Self::at(&a, s2 + 2)?),
            Vec3::new(Self::at(&a, s3)?, Self::at(&a, s3 + 1)?, Self::at(&a, s3 + 2)?),
        ))
    }

    /// `D0` (2D) at the parameter `u`.
    pub fn d0_2d(&self, u: f64) -> R<Pnt2d> {
        self.d0_local_2d(self.local_param(u))
    }
    /// `D1` (2D).
    pub fn d1_2d(&self, u: f64) -> R<(Pnt2d, Vec2d)> {
        self.d1_local_2d(self.local_param(u))
    }
    /// `D2` (2D).
    pub fn d2_2d(&self, u: f64) -> R<(Pnt2d, Vec2d, Vec2d)> {
        self.d2_local_2d(self.local_param(u))
    }
    /// `D3` (2D).
    pub fn d3_2d(&self, u: f64) -> R<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        self.d3_local_2d(self.local_param(u))
    }
    /// `D0` (3D).
    pub fn d0_3d(&self, u: f64) -> R<Pnt> {
        self.d0_local_3d(self.local_param(u))
    }
    /// `D1` (3D).
    pub fn d1_3d(&self, u: f64) -> R<(Pnt, Vec3)> {
        self.d1_local_3d(self.local_param(u))
    }
    /// `D2` (3D).
    pub fn d2_3d(&self, u: f64) -> R<(Pnt, Vec3, Vec3)> {
        self.d2_local_3d(self.local_param(u))
    }
    /// `D3` (3D).
    pub fn d3_3d(&self, u: f64) -> R<(Pnt, Vec3, Vec3, Vec3)> {
        self.d3_local_3d(self.local_param(u))
    }
}
