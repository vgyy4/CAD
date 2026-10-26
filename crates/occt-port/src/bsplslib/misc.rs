//! `BSplSLib::Resolution`, `Interpolate`, `FunctionMultiply`, `UnitWeights` and `MovePoint`.
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use super::eval::{Surface, homogeneous_d0};
use crate::bsplclib::BSplError;
use crate::bsplclib::flat::{knot_sequence, knot_sequence_length};
use crate::bsplclib::util::{R, filled, ix, rd, rd1, to_i32, wr, zeros};
use crate::bsplclib::{
    build_schoenberg_points, eval_bspline_basis_lenient as eval_bspline_basis, interpolate as curve_interpolate,
};
use crate::gp::{Pnt, Vec as Vec3};
use crate::mathlin::Matrix;
use crate::plib::Array2;

fn item<T: Copy>(a: &Array2<T>, row: isize, col: isize) -> R<T> {
    let r = usize::try_from(row - 1).map_err(|_| BSplError::OutOfRange)?;
    let c = usize::try_from(col - 1).map_err(|_| BSplError::OutOfRange)?;
    a.get(r, c).copied().ok_or(BSplError::OutOfRange)
}

fn array2<T>(rows: usize, cols: usize, data: Vec<T>) -> R<Array2<T>> {
    Ok(Array2::new(rows, cols, data)?)
}

// ---------------------------------------------------------------------------------------------
// Resolution

/// `x % n + 1` of the C++ (`n` the length of a pole direction).
fn wrap(i: isize, n: isize) -> R<isize> {
    if n <= 0 {
        return Err(BSplError::OutOfRange);
    }
    Ok(i % n + 1)
}

/// `BSplSLib::Resolution`: the parametric tolerances `(UTolerance, VTolerance)` that correspond to `tolerance_3d`
/// (zeros when the estimated derivative bound is zero). The multiplicities of `s` must be `Some`.
pub fn resolution(s: &Surface<'_>, tolerance_3d: f64) -> R<(f64, f64)> {
    if !(1..=25).contains(&s.u_degree) || !(1..=25).contains(&s.v_degree) {
        return Err(BSplError::OutOfRange);
    }
    if (s.u_rational || s.v_rational) && s.weights.is_none() {
        return Err(BSplError::OutOfRange);
    }
    let (ud, vd) = (ix(s.u_degree), ix(s.v_degree));
    let u_mults = s.u_mults.ok_or(BSplError::OutOfRange)?;
    let v_mults = s.v_mults.ok_or(BSplError::OutOfRange)?;
    let mut max_derivative = [0.0e0f64; 2];

    // the flat knots
    let flat_u = knot_sequence(s.u_knots, u_mults, s.u_degree, s.u_periodic)?;
    let flat_v = knot_sequence(s.v_knots, v_mults, s.v_degree, s.v_periodic)?;
    debug_assert_eq!(flat_u.len() as i32, knot_sequence_length(u_mults, s.u_degree, s.u_periodic)?);
    let p_row_length = s.poles.cols() as isize;
    let p_col_length = s.poles.rows() as isize;
    let mut min_weights = 0.0e0;
    if s.u_rational || s.v_rational {
        let size = (p_row_length * p_col_length) as usize;
        let wg = s.weights.ok_or(BSplError::OutOfRange)?.as_slice();
        if wg.len() < size || size == 0 {
            return Err(BSplError::OutOfRange);
        }
        min_weights = rd(wg, 0)?;
        for ii in 1..size as isize {
            let min = rd(wg, ii)?;
            if min_weights > min {
                min_weights = min;
            }
        }
    }
    let ud1 = ud + 1;
    let vd1 = vd + 1;
    let num_poles = [flat_u.len() as isize - ud1, flat_v.len() as isize - vd1];
    let poles_length = [p_col_length, p_row_length];
    let pole = |i: isize, j: isize| -> R<Pnt> { item(s.poles, i, j) };
    let weight = |i: isize, j: isize| -> R<f64> { item(s.weights.ok_or(BSplError::OutOfRange)?, i, j) };

    if s.u_rational {
        let ud2 = ud << 1;
        let vd2 = vd << 1;
        for ii in 2..=num_poles[0] {
            let ii_index = wrap(ii - 1, poles_length[0])?;
            let ii_minus = wrap(ii - 2, poles_length[0])?;
            let mut inverse = rd1(&flat_u, ii + ud)? - rd1(&flat_u, ii)?;
            inverse = 1.0e0 / inverse;
            let lower0 = (ii - ud1).max(1);
            let upper0 = (ii + ud2 + 1).min(num_poles[0]);
            for jj in 1..=num_poles[1] {
                let jj_index = wrap(jj - 1, poles_length[1])?;
                let lower1 = (jj - vd1).max(1);
                let upper1 = (jj + vd2 + 1).min(num_poles[1]);
                let pij = pole(ii_index, jj_index)?;
                let wij = weight(ii_index, jj_index)?;
                let pmj = pole(ii_minus, jj_index)?;
                let wmj = weight(ii_minus, jj_index)?;
                for pp in lower0..=upper0 {
                    let pp_index = wrap(pp - 1, poles_length[0])?;
                    for qq in lower1..=upper1 {
                        let mut value = 0.0e0;
                        let qq_index = wrap(qq - 1, poles_length[1])?;
                        let ppq = pole(pp_index, qq_index)?;
                        for (c_pq, c_ij, c_mj) in
                            [(ppq.x(), pij.x(), pmj.x()), (ppq.y(), pij.y(), pmj.y()), (ppq.z(), pij.z(), pmj.z())]
                        {
                            let mut factor = (c_pq - c_ij) * wij;
                            factor -= (c_pq - c_mj) * wmj;
                            if factor < 0.0 {
                                factor = -factor;
                            }
                            value += factor;
                        }
                        value *= inverse;
                        if max_derivative[0] < value {
                            max_derivative[0] = value;
                        }
                    }
                }
            }
        }
        max_derivative[0] /= min_weights;
    } else {
        for ii in 2..=num_poles[0] {
            let ii_index = wrap(ii - 1, poles_length[0])?;
            let ii_minus = wrap(ii - 2, poles_length[0])?;
            let mut inverse = rd1(&flat_u, ii + ud)? - rd1(&flat_u, ii)?;
            inverse = 1.0e0 / inverse;
            for jj in 1..=num_poles[1] {
                let jj_index = wrap(jj - 1, poles_length[1])?;
                let mut value = 0.0e0;
                let pij = pole(ii_index, jj_index)?;
                let pmj = pole(ii_minus, jj_index)?;
                for (a, b) in [(pij.x(), pmj.x()), (pij.y(), pmj.y()), (pij.z(), pmj.z())] {
                    let mut factor = a - b;
                    if factor < 0.0 {
                        factor = -factor;
                    }
                    value += factor;
                }
                value *= inverse;
                if max_derivative[0] < value {
                    max_derivative[0] = value;
                }
            }
        }
    }
    max_derivative[0] *= s.u_degree as f64;

    if s.v_rational {
        let ud2 = ud << 1;
        let vd2 = vd << 1;
        for ii in 2..=num_poles[1] {
            let ii_index = wrap(ii - 1, poles_length[1])?;
            let ii_minus = wrap(ii - 2, poles_length[1])?;
            let mut inverse = rd1(&flat_v, ii + vd)? - rd1(&flat_v, ii)?;
            inverse = 1.0e0 / inverse;
            let lower0 = (ii - vd1).max(1);
            let upper0 = (ii + vd2 + 1).min(num_poles[1]);
            for jj in 1..=num_poles[0] {
                let jj_index = wrap(jj - 1, poles_length[0])?;
                let lower1 = (jj - ud1).max(1);
                let upper1 = (jj + ud2 + 1).min(num_poles[0]);
                let pji = pole(jj_index, ii_index)?;
                let wji = weight(jj_index, ii_index)?;
                let pjm = pole(jj_index, ii_minus)?;
                let wjm = weight(jj_index, ii_minus)?;
                for pp in lower1..=upper1 {
                    let pp_index = wrap(pp - 1, poles_length[1])?;
                    for qq in lower0..=upper0 {
                        let mut value = 0.0e0;
                        let qq_index = wrap(qq - 1, poles_length[0])?;
                        let pqp = pole(qq_index, pp_index)?;
                        for (c_qp, c_ji, c_jm) in
                            [(pqp.x(), pji.x(), pjm.x()), (pqp.y(), pji.y(), pjm.y()), (pqp.z(), pji.z(), pjm.z())]
                        {
                            let mut factor = (c_qp - c_ji) * wji;
                            factor -= (c_qp - c_jm) * wjm;
                            if factor < 0.0 {
                                factor = -factor;
                            }
                            value += factor;
                        }
                        value *= inverse;
                        if max_derivative[1] < value {
                            max_derivative[1] = value;
                        }
                    }
                }
            }
        }
        max_derivative[1] /= min_weights;
    } else {
        for ii in 2..=num_poles[1] {
            let ii_index = wrap(ii - 1, poles_length[1])?;
            let ii_minus = wrap(ii - 2, poles_length[1])?;
            let mut inverse = rd1(&flat_v, ii + vd)? - rd1(&flat_v, ii)?;
            inverse = 1.0e0 / inverse;
            for jj in 1..=num_poles[0] {
                let jj_index = wrap(jj - 1, poles_length[0])?;
                let mut value = 0.0e0;
                let pji = pole(jj_index, ii_index)?;
                let pjm = pole(jj_index, ii_minus)?;
                for (a, b) in [(pji.x(), pjm.x()), (pji.y(), pjm.y()), (pji.z(), pjm.z())] {
                    let mut factor = a - b;
                    if factor < 0.0 {
                        factor = -factor;
                    }
                    value += factor;
                }
                value *= inverse;
                if max_derivative[1] < value {
                    max_derivative[1] = value;
                }
            }
        }
    }
    max_derivative[1] *= s.v_degree as f64;
    max_derivative[0] *= std::f64::consts::SQRT_2;
    max_derivative[1] *= std::f64::consts::SQRT_2;
    if max_derivative[0] != 0.0 && max_derivative[1] != 0.0 {
        Ok((tolerance_3d / max_derivative[0], tolerance_3d / max_derivative[1]))
    } else {
        Ok((0.0, 0.0))
    }
}

// ---------------------------------------------------------------------------------------------
// Interpolate

/// The two `BSplSLib::Interpolate` overloads: `dim_per` is 3 (poles) or 4 (poles and weights).
fn interpolate_core(
    u_degree: i32,
    v_degree: i32,
    u_flat_knots: &[f64],
    v_flat_knots: &[f64],
    u_parameters: &[f64],
    v_parameters: &[f64],
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
) -> R<(i32, Array2<Pnt>, Option<Array2<f64>>)> {
    let u_length = u_parameters.len();
    let v_length = v_parameters.len();
    if u_degree < 1
        || v_degree < 1
        || u_length == 0
        || v_length == 0
        || poles.rows() != u_length
        || poles.cols() != v_length
        || weights.is_some_and(|w| w.rows() != u_length || w.cols() != v_length)
        || u_flat_knots.len() != u_length + u_degree as usize + 1
        || v_flat_knots.len() != v_length + v_degree as usize + 1
    {
        return Err(BSplError::OutOfRange);
    }
    let per = if weights.is_some() { 4usize } else { 3 };
    let (ul, vl) = (u_length as isize, v_length as isize);
    let per_i = per as isize;

    // extraction of iso u: Points is VLength x (per * ULength)
    let dimension = per_i * ul;
    let mut points = zeros((v_length * per * u_length).max(1))?;
    for ii in 1..=vl {
        for jj in 1..=ul {
            let ll = per_i * (jj - 1) + 1;
            let p = item(poles, jj, ii)?;
            let at = (ii - 1) * dimension + (ll - 1);
            wr(&mut points, at, p.x())?;
            wr(&mut points, at + 1, p.y())?;
            wr(&mut points, at + 2, p.z())?;
            if let Some(w) = weights {
                wr(&mut points, at + 3, item(w, jj, ii)?)?;
            }
        }
    }

    // interpolation of iso u
    let contact_v = filled(v_length, 0i32)?;
    let (inversion, points) =
        curve_interpolate(v_degree, v_flat_knots, v_parameters, &contact_v, to_i32(dimension)?, &points)?;
    if inversion != 0 {
        return Ok((inversion, poles.clone(), weights.cloned()));
    }

    // extraction of iso v: IsoPoles is ULength x (per * VLength)
    let dimension2 = per_i * vl;
    let mut iso_poles = zeros((u_length * per * v_length).max(1))?;
    for ii in 1..=ul {
        let kk = per_i * (ii - 1) + 1;
        for jj in 1..=vl {
            let ll = per_i * (jj - 1) + 1;
            for c in 0..per_i {
                let value = rd(&points, (jj - 1) * dimension + (kk - 1) + c)?;
                wr(&mut iso_poles, (ii - 1) * dimension2 + (ll - 1) + c, value)?;
            }
        }
    }
    // interpolation of iso v
    let contact_u = filled(u_length, 0i32)?;
    let (inversion, iso_poles) =
        curve_interpolate(u_degree, u_flat_knots, u_parameters, &contact_u, to_i32(dimension2)?, &iso_poles)?;

    // return results
    let mut new_poles = Vec::with_capacity(u_length * v_length);
    let mut new_weights = Vec::with_capacity(u_length * v_length);
    for ii in 1..=ul {
        for jj in 1..=vl {
            let ll = per_i * (jj - 1) + 1;
            let at = (ii - 1) * dimension2 + (ll - 1);
            new_poles.push(Pnt::new(rd(&iso_poles, at)?, rd(&iso_poles, at + 1)?, rd(&iso_poles, at + 2)?));
            if weights.is_some() {
                new_weights.push(rd(&iso_poles, at + 3)?);
            }
        }
    }
    let new_weights = if weights.is_some() { Some(array2(u_length, v_length, new_weights)?) } else { None };
    Ok((inversion, array2(u_length, v_length, new_poles)?, new_weights))
}

/// `BSplSLib::Interpolate` without weights: the poles that interpolate the data points `poles` (`u_parameters.len()`
/// by `v_parameters.len()`) at the parameters; the `i32` is the inversion problem (0 when the call returns, a
/// singular system is an error).
pub fn interpolate(
    u_degree: i32,
    v_degree: i32,
    u_flat_knots: &[f64],
    v_flat_knots: &[f64],
    u_parameters: &[f64],
    v_parameters: &[f64],
    poles: &Array2<Pnt>,
) -> R<(i32, Array2<Pnt>)> {
    let (code, p, _) =
        interpolate_core(u_degree, v_degree, u_flat_knots, v_flat_knots, u_parameters, v_parameters, poles, None)?;
    Ok((code, p))
}

/// `BSplSLib::Interpolate` with weights (homogeneous interpolation of poles and weights).
pub fn interpolate_weighted(
    u_degree: i32,
    v_degree: i32,
    u_flat_knots: &[f64],
    v_flat_knots: &[f64],
    u_parameters: &[f64],
    v_parameters: &[f64],
    poles: &Array2<Pnt>,
    weights: &Array2<f64>,
) -> R<(i32, Array2<Pnt>, Array2<f64>)> {
    let (code, p, w) = interpolate_core(
        u_degree,
        v_degree,
        u_flat_knots,
        v_flat_knots,
        u_parameters,
        v_parameters,
        poles,
        Some(weights),
    )?;
    Ok((code, p, w.ok_or(BSplError::OutOfRange)?))
}

// ---------------------------------------------------------------------------------------------
// FunctionMultiply

/// `BSplSLib::FunctionMultiply`: the B-spline `(numerator, denominator)` of the product of the surface and the
/// function, interpolated in the new flat knots and degrees; `function(derivative_request, u, v)` returns the
/// value and the error code (a non-zero code is `Err(Construction)`). The `i32` is `theStatus`.
pub fn function_multiply(
    function: &dyn Fn(i32, f64, f64) -> (f64, i32),
    u_degree: i32,
    v_degree: i32,
    u_knots: &[f64],
    v_knots: &[f64],
    u_mults: Option<&[i32]>,
    v_mults: Option<&[i32]>,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
    u_flat_knots: &[f64],
    v_flat_knots: &[f64],
    u_new_degree: i32,
    v_new_degree: i32,
) -> R<(Array2<Pnt>, Array2<f64>, i32)> {
    let weights_ref = weights.ok_or(BSplError::OutOfRange)?;
    if u_new_degree < 1 || v_new_degree < 1 {
        return Err(BSplError::OutOfRange);
    }
    let num_u = u_flat_knots.len() as i64 - i64::from(u_new_degree) - 1;
    let num_v = v_flat_knots.len() as i64 - i64::from(v_new_degree) - 1;
    if num_u < 1 || num_v < 1 {
        return Err(BSplError::OutOfRange);
    }
    let (num_u, num_v) = (num_u as usize, num_v as usize);
    let u_parameters = build_schoenberg_points(u_new_degree, u_flat_knots, to_i32(num_u as isize)?)?;
    let v_parameters = build_schoenberg_points(v_new_degree, v_flat_knots, to_i32(num_v as isize)?)?;

    let surface = Surface {
        u_degree,
        v_degree,
        u_rational: true,
        v_rational: true,
        u_periodic: false,
        v_periodic: false,
        poles,
        weights: Some(weights_ref),
        u_knots,
        v_knots,
        u_mults,
        v_mults,
    };
    let mut numerator = Vec::with_capacity(num_u * num_v);
    let mut denominator = Vec::with_capacity(num_u * num_v);
    for ii in 0..num_u {
        for jj in 0..num_v {
            let u = *u_parameters.get(ii).ok_or(BSplError::OutOfRange)?;
            let v = *v_parameters.get(jj).ok_or(BSplError::OutOfRange)?;
            let (w, p) = homogeneous_d0(u, v, 0, 0, &surface)?;
            let (result, error_code) = function(0, u, v);
            if error_code != 0 {
                return Err(BSplError::Construction);
            }
            numerator.push(Pnt::new(p.x() * result, p.y() * result, p.z() * result));
            denominator.push(w * result);
        }
    }
    let numerator = array2(num_u, num_v, numerator)?;
    let denominator = array2(num_u, num_v, denominator)?;
    let (status, numerator, denominator) = interpolate_weighted(
        u_new_degree,
        v_new_degree,
        u_flat_knots,
        v_flat_knots,
        &u_parameters,
        &v_parameters,
        &numerator,
        &denominator,
    )?;
    Ok((numerator, denominator, status))
}

// ---------------------------------------------------------------------------------------------
// UnitWeights

/// `BSplSLib::UnitWeights`: a `nb_u_poles` x `nb_v_poles` array of ones.
pub fn unit_weights(nb_u_poles: i32, nb_v_poles: i32) -> R<Array2<f64>> {
    if nb_u_poles < 1 || nb_v_poles < 1 {
        return Err(BSplError::OutOfRange);
    }
    let (rows, cols) = (nb_u_poles as usize, nb_v_poles as usize);
    let total = rows.checked_mul(cols).ok_or(BSplError::OutOfRange)?;
    array2(rows, cols, filled(total, 1.0)?)
}

// ---------------------------------------------------------------------------------------------
// MovePoint

/// The result of `move_point`.
#[derive(Debug)]
pub struct MovedPoint {
    /// `UFirstIndex`.
    pub u_first: i32,
    /// `ULastIndex`.
    pub u_last: i32,
    /// `VFirstIndex`.
    pub v_first: i32,
    /// `VLastIndex`.
    pub v_last: i32,
    /// `NewPoles`; `None` (with the four indices 0) when the B-spline basis could not be computed (no change).
    pub poles: Option<Array2<Pnt>>,
}

/// The index and value of the predominant basis function (`ukk1`) and the symmetric one (`ukk2`).
fn predominant(basis: &Matrix, first_non_zero: i32, first_index: i32, last_index: i32) -> R<(i32, i32)> {
    let mut max_value = 0.0e0;
    let mut kk1 = 0i32;
    let from =
        i32::try_from(i64::from(first_index) - i64::from(first_non_zero) + 1).map_err(|_| BSplError::OutOfRange)?;
    let to = i32::try_from(i64::from(last_index) - i64::from(first_non_zero) + 1).map_err(|_| BSplError::OutOfRange)?;
    for i in from..=to {
        let value = basis.value(1, i)?;
        if value > max_value {
            kk1 = i + first_non_zero - 1;
            max_value = value;
        }
    }
    // find a kk2 if symmetry. With no predominant function (an empty range, `kk1 == 0`) the C++ reads the matrix just
    // outside its row (no range check): the value only matters when poles are moved, and then no pole is in range.
    let mut kk2 = kk1;
    if kk1 < last_index {
        let neighbour = basis.value(1, kk1 - first_non_zero + 2).ok();
        if neighbour.is_some_and(|value| (value - max_value).abs() < 1.0e-10) {
            kk2 = kk1 + 1;
        }
    }
    Ok((kk1, kk2))
}

// `Weights(ii, jj)` as OCCT reads it: the weights of a periodic direction are read past the end of the array (the
// columns beyond the last wrap onto the following row, rows beyond the last are memory after the array, which the
// golden file shows as zero).
fn weight_word(weights: &Array2<f64>, ii: isize, jj: isize) -> R<f64> {
    if ii > weights.rows() as isize && ii >= 1 && jj >= 1 && jj <= weights.cols() as isize {
        return Ok(0.0);
    }
    item(weights, ii, jj)
}

/// `BSplSLib::MovePoint`: the new poles which allow the point of parameters (`u`, `v`) to reach its position
/// plus `displ`; `weights` is read only if `rational`.
pub fn move_point(
    u: f64,
    v: f64,
    displ: &Vec3,
    u_index1: i32,
    u_index2: i32,
    v_index1: i32,
    v_index2: i32,
    u_degree: i32,
    v_degree: i32,
    rational: bool,
    poles: &Array2<Pnt>,
    weights: &Array2<f64>,
    u_flat_knots: &[f64],
    v_flat_knots: &[f64],
) -> R<MovedPoint> {
    if !(1..=25).contains(&u_degree)
        || !(1..=25).contains(&v_degree)
        // (the flat knots of a periodic direction are shorter than poles + degree + 1)
        || (rational && (weights.rows() != poles.rows() || weights.cols() != poles.cols()))
    {
        return Err(BSplError::OutOfRange);
    }
    // calculate the UBSplineBasis in the parameter U
    let mut u_basis = Matrix::new(1, 1, 1, u_degree + 1)?;
    let (error1, u_first_non_zero) = eval_bspline_basis(0, u_degree + 1, u_flat_knots, u, false, &mut u_basis)?;
    // calculate the VBSplineBasis in the parameter V
    let mut v_basis = Matrix::new(1, 1, 1, v_degree + 1)?;
    let (error2, v_first_non_zero) = eval_bspline_basis(0, v_degree + 1, v_flat_knots, v, false, &mut v_basis)?;
    if error1 != 0 || error2 != 0 {
        return Ok(MovedPoint { u_first: 0, u_last: 0, v_first: 0, v_last: 0, poles: None });
    }

    // find the span which is predominant for parameter U
    let u_first_index = u_first_non_zero.max(u_index1);
    let u_last_index = (u_first_non_zero + u_degree).min(u_index2);
    let (ukk1, ukk2) = predominant(&u_basis, u_first_non_zero, u_first_index, u_last_index)?;

    // find the span which is predominant for parameter V
    let v_first_index = v_first_non_zero.max(v_index1);
    let v_last_index = (v_first_non_zero + v_degree).min(v_index2);
    let (vkk1, vkk2) = predominant(&v_basis, v_first_non_zero, v_first_index, v_last_index)?;

    // compute the vector of displacement
    let mut d1 = 0.0e0;
    let mut d2 = 0.0e0;
    for i in 1..=u_degree + 1 {
        let ii = i + u_first_non_zero - 1;
        let dval_u = if ii < ukk1 {
            f64::from(ukk1 - ii)
        } else if ii > ukk2 {
            f64::from(ii - ukk2)
        } else {
            0.0
        };
        for j in 1..=v_degree + 1 {
            let jj = j + v_first_non_zero - 1;
            let hn;
            if rational {
                hn = weight_word(weights, ix(ii), ix(jj))? * u_basis.value(1, i)? * v_basis.value(1, j)?;
                d2 += hn;
            } else {
                hn = u_basis.value(1, i)? * v_basis.value(1, j)?;
            }
            if ii >= u_first_index && ii <= u_last_index && jj >= v_first_index && jj <= v_last_index {
                let dval_v = if jj < vkk1 {
                    f64::from(vkk1 - jj)
                } else if jj > vkk2 {
                    f64::from(jj - vkk2)
                } else {
                    0.0
                };
                d1 += 1.0 / (dval_u + dval_v + 1.0) * hn;
            }
        }
    }
    let coef = if rational { d2 / d1 } else { 1.0 / d1 };

    // compute the new poles
    let (rows, cols) = (poles.rows(), poles.cols());
    let mut new_poles = Vec::with_capacity(rows * cols);
    for i in 1..=rows as i32 {
        let dval_u = if i < ukk1 {
            f64::from(ukk1 - i)
        } else if i > ukk2 {
            f64::from(i - ukk2)
        } else {
            0.0
        };
        for j in 1..=cols as i32 {
            let p = item(poles, ix(i), ix(j))?;
            if i >= u_first_index && i <= u_last_index && j >= v_first_index && j <= v_last_index {
                let dval_v = if j < vkk1 {
                    f64::from(vkk1 - j)
                } else if j > vkk2 {
                    f64::from(j - vkk2)
                } else {
                    0.0
                };
                let factor = coef / (dval_u + dval_v + 1.0);
                new_poles.push(Pnt::new(
                    p.x() + displ.x() * factor,
                    p.y() + displ.y() * factor,
                    p.z() + displ.z() * factor,
                ));
            } else {
                new_poles.push(p);
            }
        }
    }
    Ok(MovedPoint {
        u_first: u_first_index,
        u_last: u_last_index,
        v_first: v_first_index,
        v_last: v_last_index,
        poles: Some(array2(rows, cols, new_poles)?),
    })
}
