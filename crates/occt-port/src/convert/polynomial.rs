//! `Convert_CompPolynomialToPoles`: the poles of the B-spline curve that interpolates a sequence of polynomial curves.

use super::{ConvertError, R, at, len_of};
use crate::bsplclib;
use crate::plib::{self, Array2};

/// The B-spline curve (poles, knots, multiplicities) equivalent to a sequence of polynomial curves, computed by
/// interpolation at the Schoenberg points.
#[derive(Debug, Clone, PartialEq)]
pub struct CompPolynomialToPoles {
    #[allow(dead_code)]
    flat_knots: Vec<f64>,
    knots: Vec<f64>,
    mults: Vec<i32>,
    poles: Array2<f64>,
    degree: i32,
    done: bool,
}

/// `a * b` as a count, `Err(Argument)` on overflow.
fn product(factors: &[i64]) -> R<i64> {
    factors.iter().try_fold(1i64, |acc, f| acc.checked_mul(*f)).ok_or(ConvertError::Argument)
}

/// The degree of the first `num_curves` curves: the largest `num_coeff[..] - 1` (and 0); `Err(Argument)` where
/// OCCT would read outside the array or use a curve without coefficients.
fn max_degree_of(num_curves: i32, num_coeff_per_curve: &[i32]) -> R<i32> {
    let n = len_of(num_curves)?;
    let first = num_coeff_per_curve.get(..n).ok_or(ConvertError::Argument)?;
    let mut degree = 0;
    for c in first {
        if *c < 1 {
            return Err(ConvertError::Argument);
        }
        degree = degree.max(*c - 1);
    }
    Ok(degree)
}

/// The inputs OCCT 8.0.1 does not check and that would be read outside their arrays.
fn check_sizes(
    num_curves: i32,
    max_degree: i32,
    degree: i32,
    dimension: i32,
    coefficients: &[f64],
    polynomial_intervals: usize,
    true_intervals: usize,
) -> R<()> {
    let coef_len = product(&[i64::from(num_curves), i64::from(dimension), i64::from(max_degree.max(degree)) + 1])?;
    if i64::try_from(coefficients.len()).map_err(|_| ConvertError::Argument)? < coef_len
        || polynomial_intervals < len_of(num_curves)?
        || true_intervals < len_of(num_curves)? + 1
    {
        return Err(ConvertError::Argument);
    }
    Ok(())
}

impl CompPolynomialToPoles {
    /// `Convert_CompPolynomialToPoles(NumCurves, Continuity, Dimension, MaxDegree, NumCoeffPerCurve, Coefficients,
    /// PolynomialIntervals, TrueIntervals)`: one continuity for all the inner knots.
    ///
    /// `Err(Construction)` for `num_curves <= 0`, `continuity < 0`, `max_degree <= 0`, `dimension <= 0`, for a
    /// continuity above the degree with several curves, and for a singular interpolation; `Err(Argument)` for slices
    /// that are too short (OCCT 8.0.1 reads outside them).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        num_curves: i32,
        continuity: i32,
        dimension: i32,
        max_degree: i32,
        num_coeff_per_curve: &[i32],
        coefficients: &[f64],
        polynomial_intervals: &[[f64; 2]],
        true_intervals: &[f64],
    ) -> R<Self> {
        if num_curves <= 0 || continuity < 0 || max_degree <= 0 || dimension <= 0 {
            return Err(ConvertError::Construction);
        }
        let degree = max_degree_of(num_curves, num_coeff_per_curve)?;
        if continuity > degree && num_curves > 1 {
            return Err(ConvertError::Construction);
        }
        check_sizes(
            num_curves,
            max_degree,
            degree,
            dimension,
            coefficients,
            polynomial_intervals.len(),
            true_intervals.len(),
        )?;
        let n = len_of(num_curves)?;
        let knots = true_intervals.get(..=n).ok_or(ConvertError::Argument)?.to_vec();
        let multiplicities = degree - continuity;
        let mut mults = vec![multiplicities; n + 1];
        if let Some(m) = mults.first_mut() {
            *m = degree + 1;
        }
        if let Some(m) = mults.last_mut() {
            *m = degree + 1;
        }
        Self::perform(
            degree,
            knots,
            mults,
            num_curves,
            max_degree,
            dimension,
            num_coeff_per_curve,
            coefficients,
            polynomial_intervals,
            true_intervals,
        )
    }

    /// `Convert_CompPolynomialToPoles(NumCurves, Dimension, MaxDegree, Continuity, NumCoeffPerCurve, Coefficients,
    /// PolynomialIntervals, TrueIntervals)`: `continuity[k - 1]` is the continuity at the inner knot `k`
    /// (`k = 2..=num_curves`); errors as `new`.
    #[allow(clippy::too_many_arguments)]
    pub fn with_continuities(
        num_curves: i32,
        dimension: i32,
        max_degree: i32,
        continuity: &[i32],
        num_coeff_per_curve: &[i32],
        coefficients: &[f64],
        polynomial_intervals: &[[f64; 2]],
        true_intervals: &[f64],
    ) -> R<Self> {
        if num_curves <= 0 || max_degree <= 0 || dimension <= 0 {
            return Err(ConvertError::Construction);
        }
        let degree = max_degree_of(num_curves, num_coeff_per_curve)?;
        check_sizes(
            num_curves,
            max_degree,
            degree,
            dimension,
            coefficients,
            polynomial_intervals.len(),
            true_intervals.len(),
        )?;
        let n = len_of(num_curves)?;
        let knots = true_intervals.get(..=n).ok_or(ConvertError::Argument)?.to_vec();
        let mut mults = vec![0; n + 1];
        for ii in 2..=i64::from(num_curves) {
            let c = at(continuity, ii)?;
            if c > degree {
                return Err(ConvertError::Construction);
            }
            super::put(&mut mults, ii, degree - c)?;
        }
        if let Some(m) = mults.first_mut() {
            *m = degree + 1;
        }
        if let Some(m) = mults.last_mut() {
            *m = degree + 1;
        }
        Self::perform(
            degree,
            knots,
            mults,
            num_curves,
            max_degree,
            dimension,
            num_coeff_per_curve,
            coefficients,
            polynomial_intervals,
            true_intervals,
        )
    }

    /// `Convert_CompPolynomialToPoles(Dimension, MaxDegree, Degree, Coefficients, PolynomialIntervals,
    /// TrueIntervals)`: a single polynomial curve of degree `degree`; `polynomial_intervals` has its two bounds.
    pub fn single(
        dimension: i32,
        max_degree: i32,
        degree: i32,
        coefficients: &[f64],
        polynomial_intervals: &[f64],
        true_intervals: &[f64],
    ) -> R<Self> {
        if max_degree <= 0 || dimension <= 0 || polynomial_intervals.len() != 2 {
            return Err(ConvertError::Construction);
        }
        let (Some(p0), Some(p1)) = (polynomial_intervals.first(), polynomial_intervals.last()) else {
            return Err(ConvertError::Construction);
        };
        let (Some(t0), Some(t1)) = (true_intervals.first(), true_intervals.get(1)) else {
            return Err(ConvertError::Argument);
        };
        let num_coeff = [degree.checked_add(1).ok_or(ConvertError::Argument)?];
        Self::perform(
            degree,
            vec![*t0, *t1],
            vec![degree + 1; 2],
            1,
            max_degree,
            dimension,
            &num_coeff,
            coefficients,
            &[[*p0, *p1]],
            true_intervals,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn perform(
        degree: i32,
        knots: Vec<f64>,
        mults: Vec<i32>,
        num_curves: i32,
        max_degree: i32,
        dimension: i32,
        num_coeff_per_curve: &[i32],
        coefficients: &[f64],
        polynomial_intervals: &[[f64; 2]],
        true_intervals: &[f64],
    ) -> R<Self> {
        let mut num_flat_knots = i64::from(degree) * 2 + 2;
        let inner = mults.get(1..mults.len().saturating_sub(1)).unwrap_or(&[]);
        for m in inner {
            num_flat_knots += i64::from(*m);
        }
        let num_poles = num_flat_knots - i64::from(degree) - 1;
        if num_poles < 1 {
            return Err(ConvertError::Argument);
        }
        let flat_knots = bsplclib::knot_sequence(&knots, &mults, degree, false)?;
        if i64::try_from(flat_knots.len()).ok() != Some(num_flat_knots) {
            return Err(ConvertError::Argument);
        }
        let num_poles_i = i32::try_from(num_poles).map_err(|_| ConvertError::Argument)?;
        let parameters = bsplclib::build_schoenberg_points(degree, &flat_knots, num_poles_i)?;
        let dim = len_of(dimension)?;
        let mut poles = vec![0.0; len_of(num_poles_i)? * dim];
        let mut contact = Vec::with_capacity(parameters.len());
        let mut index = 2i64;
        let mut t_index = 2i64;
        let mut p_index = 1i64;
        let stride = product(&[i64::from(dimension), i64::from(max_degree.max(degree)) + 1])?;
        for (ii, u) in parameters.iter().enumerate() {
            contact.push(0);
            while *u >= at(true_intervals, t_index)? && index <= i64::from(num_curves) {
                index += 1;
                t_index += 1;
                p_index += 1;
            }
            // normalized value so that it fits the original intervals for the polynomial definition of the curves
            let t_prev = at(true_intervals, t_index - 1)?;
            let mut normalized_value = *u - t_prev;
            normalized_value /= at(true_intervals, t_index)? - t_prev;
            let [lower, upper] = at(polynomial_intervals, p_index)?;
            normalized_value = (1.0 - normalized_value) * lower + normalized_value * upper;
            let coeff_index = product(&[index - 2, stride])? + 1;
            let from = usize::try_from(coeff_index - 1).map_err(|_| ConvertError::Argument)?;
            let coefficient_array = coefficients.get(from..).ok_or(ConvertError::Argument)?;
            let deg = at(num_coeff_per_curve, i64::from(1) + index - 2)? - 1;
            let value = plib::no_derivative_eval_polynomial(
                normalized_value,
                deg,
                dimension,
                deg * dimension,
                coefficient_array,
            )?;
            let start = ii * dim;
            let slot = poles.get_mut(start..start + dim).ok_or(ConvertError::OutOfRange)?;
            if value.len() != dim {
                return Err(ConvertError::OutOfRange);
            }
            slot.copy_from_slice(&value);
        }
        // interpolation at schoenberg points should yield the desired result
        let (inversion_problem, solved) =
            bsplclib::interpolate(degree, &flat_knots, &parameters, &contact, dimension, &poles)?;
        if inversion_problem != 0 {
            return Err(ConvertError::Construction);
        }
        let poles = Array2::new(len_of(num_poles_i)?, dim, solved)?;
        Ok(Self { flat_knots, knots, mults, poles, degree, done: true })
    }

    /// Whether the conversion is done (always, after a constructor succeeded).
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The degree of the B-spline curve (0 if not done).
    pub fn degree(&self) -> i32 {
        if self.done { self.degree } else { 0 }
    }

    /// The number of poles (0 if not done).
    pub fn nb_poles(&self) -> i32 {
        if self.done { i32::try_from(self.poles.rows()).unwrap_or(i32::MAX) } else { 0 }
    }

    /// The poles: `nb_poles` rows of `dimension` columns.
    pub fn poles(&self) -> &Array2<f64> {
        &self.poles
    }

    /// The number of knots (0 if not done).
    pub fn nb_knots(&self) -> i32 {
        if self.done { i32::try_from(self.knots.len()).unwrap_or(i32::MAX) } else { 0 }
    }

    /// The knots.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The multiplicities of the knots.
    pub fn multiplicities(&self) -> &[i32] {
        &self.mults
    }
}
