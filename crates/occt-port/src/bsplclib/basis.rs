//! `BSplCLib::EvalBsplineBasis`, the banded interpolation matrix (`BuildBSpMatrix`, `FactorBandedMatrix`,
//! `SolveBandedSystem`), `BuildSchoenbergPoints` and `Interpolate`.
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use super::BSplError;
use super::flat::locate_parameter_range;
use super::util::{R, filled, ix, len_ix, rd, wr};
use crate::gp;
use crate::mathlin::Matrix;

/// The size of the basis buffer of `BSplCLib::BuildBSpMatrix` (`MaxDegree() + 1`).
const MAX_ORDER: isize = 26;

/// The body of `BSplCLib::EvalBsplineBasis` once the bounds of the basis matrix are known to fit; the matrix is the
/// row-major buffer `basis` with `ncols` columns (both bounds 1). Returns the error code (0 or 2) and the first
/// non-zero B-spline index.
fn eval_basis_core(
    derivative_request: i32,
    order: i32,
    flat_knots: &[f64],
    parameter: f64,
    is_periodic: bool,
    basis: &mut [f64],
    ncols: isize,
) -> R<(i32, i32)> {
    let order = ix(order);
    let local_request = if ix(derivative_request) >= order { order - 1 } else { ix(derivative_request) };
    let num_poles = len_ix(flat_knots) - order;
    let (ii, _new_param) = locate_parameter_range(
        super::util::to_i32(order - 1)?,
        flat_knots,
        parameter,
        is_periodic,
        super::util::to_i32(order)?,
        super::util::to_i32(num_poles + 1)?,
    )?;
    let first_non_zero = ii - super::util::to_i32(order)? + 1;
    // rebase to zero-based indexing into the knots
    let ii = ix(ii) - 1;
    let resolution = gp::resolution();

    wr(basis, 0, 1.0)?;
    for qq in 2..=order - local_request {
        wr(basis, qq - 1, 0.0)?;
        for pp in 1..qq {
            let k_lo = rd(flat_knots, ii - qq + pp + 1)?;
            let scale = rd(flat_knots, ii + pp)? - k_lo;
            if scale.abs() < resolution {
                return Ok((2, first_non_zero));
            }
            let factor = (parameter - k_lo) / scale;
            let saved = factor * rd(basis, pp - 1)?;
            let mut value = rd(basis, pp - 1)? * (1.0 - factor);
            value += rd(basis, qq - 1)?;
            wr(basis, pp - 1, value)?;
            wr(basis, qq - 1, saved)?;
        }
    }

    for qq in order - local_request + 1..=order {
        for pp in 1..qq {
            let value = rd(basis, pp - 1)?;
            wr(basis, (order - qq + 1) * ncols + (pp - 1), value)?;
        }
        wr(basis, qq - 1, 0.0)?;
        for ss in order - local_request + 1..=qq {
            wr(basis, (order - ss + 1) * ncols + (qq - 1), 0.0)?;
        }
        for pp in 1..qq {
            let k_lo = rd(flat_knots, ii - qq + pp + 1)?;
            let scale = rd(flat_knots, ii + pp)? - k_lo;
            if scale.abs() < resolution {
                return Ok((2, first_non_zero));
            }
            let inverse = 1.0 / scale;
            let factor = (parameter - k_lo) * inverse;
            let mut saved = factor * rd(basis, pp - 1)?;
            let mut value = rd(basis, pp - 1)? * (1.0 - factor);
            value += rd(basis, qq - 1)?;
            wr(basis, pp - 1, value)?;
            wr(basis, qq - 1, saved)?;
            let local_inverse = (qq - 1) as f64 * inverse;
            for ss in order - local_request + 1..=qq {
                let row = (order - ss + 1) * ncols;
                saved = local_inverse * rd(basis, row + pp - 1)?;
                let mut value = rd(basis, row + pp - 1)? * -local_inverse;
                value += rd(basis, row + qq - 1)?;
                wr(basis, row + pp - 1, value)?;
                wr(basis, row + qq - 1, saved)?;
            }
        }
    }
    Ok((0, first_non_zero))
}

/// `BSplCLib::EvalBsplineBasis(DerivativeRequest, Order, FlatKnots, Parameter, FirstNonZeroBsplineIndex,
/// BsplineBasis, isPeriodic)`: returns the error code (0, 1 for a matrix whose bounds do not fit, 2 for a knot
/// interval shorter than `gp::resolution()`) and the first non-zero B-spline index (0 for code 1). `basis` is
/// updated in place.
pub fn eval_bspline_basis(
    derivative_request: i32,
    order: i32,
    flat_knots: &[f64],
    parameter: f64,
    is_periodic: bool,
    basis: &mut Matrix,
) -> R<(i32, i32)> {
    if order < 1 || derivative_request < 0 || (flat_knots.len() as i64) < 2 * i64::from(order) {
        return Err(BSplError::OutOfRange);
    }
    let local_request = if derivative_request >= order { order - 1 } else { derivative_request };
    if basis.lower_col() != 1
        || basis.upper_col() < order
        || basis.lower_row() != 1
        || basis.upper_row() <= local_request
    {
        return Ok((1, 0));
    }
    let rows = basis.upper_row();
    let cols = basis.upper_col();
    let mut buffer = filled(ix(rows).max(0) as usize * ix(cols).max(0) as usize, 0.0)?;
    for r in 1..=rows {
        for c in 1..=cols {
            wr(&mut buffer, ix(r - 1) * ix(cols) + ix(c - 1), basis.value(r, c)?)?;
        }
    }
    let result = eval_basis_core(derivative_request, order, flat_knots, parameter, is_periodic, &mut buffer, ix(cols));
    // the entries OCCT touched keep their new values, also when an error is reported
    for r in 1..=rows {
        for c in 1..=cols {
            basis.set_value(r, c, rd(&buffer, ix(r - 1) * ix(cols) + ix(c - 1))?)?;
        }
    }
    result
}

/// `BSplCLib::BuildBSpMatrix`: the band matrix (`parameters.len()` rows, `2 * degree + 1` columns) of the B-spline
/// basis functions (or their derivatives of the given contact order) at the parameters, and the band widths.
pub(crate) fn build_bsp_matrix(
    parameters: &[f64],
    contact_order: &[i32],
    flat_knots: &[f64],
    degree: i32,
) -> R<(Matrix, i32, i32)> {
    let order = degree + 1;
    let band_width = 2 * degree + 1;
    let n = parameters.len();
    let mut matrix = Matrix::filled(1, super::util::to_i32(len_ix(parameters))?, 1, band_width, 0.0)?;
    let mut basis = filled((MAX_ORDER * MAX_ORDER) as usize, 0.0)?;
    for i in 1..=n {
        let contact = *contact_order.get(i - 1).ok_or(BSplError::OutOfRange)?;
        let parameter = *parameters.get(i - 1).ok_or(BSplError::OutOfRange)?;
        if contact < 0 || contact > degree || ix(order) > MAX_ORDER {
            return Err(BSplError::OutOfRange);
        }
        let (code, first_non_zero) =
            eval_basis_core(contact, order, flat_knots, parameter, false, &mut basis, MAX_ORDER)?;
        if code != 0 {
            return Err(BSplError::OutOfRange);
        }
        let index = ix(degree) + 1 + ix(first_non_zero) - ix(super::util::to_i32(ix_us(i))?);
        for j in 0..ix(order) {
            let value = rd(&basis, ix(contact) * MAX_ORDER + j)?;
            let column = index + j - 1;
            if column < 0 || column >= ix(band_width) {
                return Err(BSplError::OutOfRange);
            }
            matrix.set_value(super::util::to_i32(ix_us(i))?, super::util::to_i32(column + 1)?, value)?;
        }
    }
    Ok((matrix, degree, degree))
}

fn ix_us(i: usize) -> isize {
    i as isize
}

/// `BSplCLib::FactorBandedMatrix`: the LU factorisation of the band matrix in place; returns the error code and the
/// pivot index problem.
pub(crate) fn factor_banded_matrix(matrix: &mut Matrix, upper_band: i32, lower_band: i32) -> R<(i32, i32)> {
    let band_width = ix(upper_band) + ix(lower_band) + 1;
    let lb = ix(lower_band);
    let l_row = ix(matrix.lower_row());
    for i in l_row + 1..=ix(matrix.upper_row()) {
        let min_index = if lb - i + 2 >= 1 { lb - i + 2 } else { 1 };
        for j in min_index..=lb {
            let index = i - lb + j - 1;
            let pivot = matrix.value(super::util::to_i32(index)?, super::util::to_i32(lb + 1)?)?;
            if pivot.abs() <= gp::resolution() {
                return Ok((1, super::util::to_i32(index)?));
            }
            let inverse = -1.0 / pivot;
            let ri = super::util::to_i32(i)?;
            let rj = super::util::to_i32(j)?;
            let scaled = matrix.value(ri, rj)? * inverse;
            matrix.set_value(ri, rj, scaled)?;
            let max_index = band_width + index - i;
            for k in j + 1..=max_index {
                let rk = super::util::to_i32(k)?;
                let source = matrix.value(super::util::to_i32(index)?, super::util::to_i32(k + i - index)?)?;
                let value = matrix.value(ri, rk)? + scaled * source;
                matrix.set_value(ri, rk, value)?;
            }
        }
    }
    Ok((0, 0))
}

/// `BSplCLib::SolveBandedSystem(Matrix, UpperBandWidth, LowerBandWidth, ArrayDimension, Array)`: solves in place the
/// factored system for `array_dimension` right-hand sides per row; returns the error code.
pub(crate) fn solve_banded_system(
    matrix: &Matrix,
    upper_band: i32,
    lower_band: i32,
    array_dimension: i32,
    array: &mut [f64],
) -> R<i32> {
    if matrix.lower_col() != 1 || ix(matrix.upper_col()) != ix(upper_band) + ix(lower_band) + 1 {
        return Ok(1);
    }
    let lb = ix(lower_band);
    let ub = ix(upper_band);
    let dim = ix(array_dimension);
    let l_row = ix(matrix.lower_row());
    let u_row = ix(matrix.upper_row());
    let at = |row: isize, col: isize| -> R<f64> {
        Ok(matrix.value(super::util::to_i32(row)?, super::util::to_i32(col + 1)?)?)
    };
    for ii in l_row + 1..=u_row {
        let min_index = if ii - lb >= l_row { ii - lb } else { l_row };
        for jj in min_index..ii {
            let coeff = at(ii, jj - ii + lb)?;
            for kk in 0..dim {
                let value = rd(array, (ii - 1) * dim + kk)? + rd(array, (jj - 1) * dim + kk)? * coeff;
                wr(array, (ii - 1) * dim + kk, value)?;
            }
        }
    }
    let mut ii = u_row;
    while ii >= l_row {
        let max_index = if ii + ub <= u_row { ii + ub } else { u_row };
        let mut jj = max_index;
        while jj > ii {
            let coeff = at(ii, jj - ii + lb)?;
            for kk in 0..dim {
                let value = rd(array, (ii - 1) * dim + kk)? - rd(array, (jj - 1) * dim + kk)? * coeff;
                wr(array, (ii - 1) * dim + kk, value)?;
            }
            jj -= 1;
        }
        // Fixing a bug PRO18577 to avoid division by zero
        let divisor = at(ii, lb)?;
        if divisor.abs() <= 1.0e-16 {
            return Ok(1);
        }
        let inverse = 1.0 / divisor;
        for kk in 0..dim {
            let value = rd(array, (ii - 1) * dim + kk)? * inverse;
            wr(array, (ii - 1) * dim + kk, value)?;
        }
        ii -= 1;
    }
    Ok(0)
}

/// `BSplCLib::BuildSchoenbergPoints`: the `nb_parameters` Schoenberg abscissae of the flat knots.
pub fn build_schoenberg_points(degree: i32, flat_knots: &[f64], nb_parameters: i32) -> R<Vec<f64>> {
    if degree < 1 || nb_parameters < 1 || (flat_knots.len() as i64) < i64::from(degree) + i64::from(nb_parameters) {
        return Err(BSplError::OutOfRange);
    }
    let inverse = 1.0e0 / f64::from(degree);
    let mut parameters = Vec::new();
    for ii in 1..=ix(nb_parameters) {
        let mut value = 0.0e0;
        for jj in 1..=ix(degree) {
            value += rd(flat_knots, jj + ii - 1)?;
        }
        value *= inverse;
        parameters.push(value);
    }
    Ok(parameters)
}

/// `BSplCLib::Interpolate(Degree, FlatKnots, Parameters, ContactOrderArray, ArrayDimension, Poles,
/// InversionProblem)`: solves for the poles that interpolate the data points in `poles` (`dimension` values per
/// parameter); returns the inversion problem (always 0 when the call returns) and the whole poles buffer.
/// A singular system is `Err(OutOfRange)` (`Standard_OutOfRange("BSplCLib::Interpolate")`).
pub fn interpolate(
    degree: i32,
    flat_knots: &[f64],
    parameters: &[f64],
    contact_order: &[i32],
    dimension: i32,
    poles: &[f64],
) -> R<(i32, Vec<f64>)> {
    if degree < 1
        || dimension < 1
        || parameters.is_empty()
        || contact_order.len() != parameters.len()
        || contact_order.iter().any(|c| *c < 0 || *c > degree)
        || (poles.len() as i64) < parameters.len() as i64 * i64::from(dimension)
        || flat_knots.len() as i64 != parameters.len() as i64 + i64::from(degree) + 1
    {
        return Err(BSplError::OutOfRange);
    }
    let (mut matrix, upper_band, lower_band) = build_bsp_matrix(parameters, contact_order, flat_knots, degree)?;
    let (code, _pivot) = factor_banded_matrix(&mut matrix, upper_band, lower_band)?;
    if code != 0 {
        return Err(BSplError::OutOfRange);
    }
    let mut buffer = poles.to_vec();
    if solve_banded_system(&matrix, upper_band, lower_band, dimension, &mut buffer)? != 0 {
        return Err(BSplError::OutOfRange);
    }
    Ok((0, buffer))
}
