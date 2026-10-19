//! The caches of `BSplSLib`: `BuildCache`, `CacheD0/D1/D2`, `CoefsD0/D1/D2`, `PolesCoefficients` and the class
//! `BSplSLib_Cache` (with `BSplCLib_CacheParams`).
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use super::eval::{Prepared, Surface, prepare_eval, rational_derivative_core, tail};
use crate::bsplclib::BSplError;
use crate::bsplclib::flat::{bohm_in_place, locate_parameter_opt};
use crate::bsplclib::util::{R, epsilon, filled, ix, len_ix, rd, to_i32, wr, zeros};
use crate::gp::{Pnt, Vec as Vec3};
use crate::plib::{self, Array2};

/// A cache of a surface (the type of the three-value result of the cache evaluations).
type Vec3s = (Pnt, Vec3, Vec3);
type Vec3s2 = (Pnt, Vec3, Vec3, Vec3, Vec3, Vec3);

fn check_degrees(u_degree: i32, v_degree: i32) -> R<()> {
    if !(1..=25).contains(&u_degree) || !(1..=25).contains(&v_degree) {
        return Err(BSplError::OutOfRange);
    }
    Ok(())
}

/// The values of the points, pole after pole.
fn flat_points(poles: &Array2<Pnt>) -> Vec<f64> {
    poles.as_slice().iter().flat_map(|p| [p.x(), p.y(), p.z()]).collect()
}

fn array2<T>(rows: isize, cols: isize, data: Vec<T>) -> R<Array2<T>> {
    let rows = usize::try_from(rows).map_err(|_| BSplError::OutOfRange)?;
    let cols = usize::try_from(cols).map_err(|_| BSplError::OutOfRange)?;
    Ok(Array2::new(rows, cols, data)?)
}

fn surface_for_cache<'a>(
    u_periodic: bool,
    v_periodic: bool,
    u_degree: i32,
    v_degree: i32,
    u_flat_knots: &'a [f64],
    v_flat_knots: &'a [f64],
    poles: &'a Array2<Pnt>,
    weights: Option<&'a Array2<f64>>,
) -> Surface<'a> {
    Surface {
        u_degree,
        v_degree,
        u_rational: weights.is_some(),
        v_rational: weights.is_some(),
        u_periodic,
        v_periodic,
        poles,
        weights,
        u_knots: u_flat_knots,
        v_knots: v_flat_knots,
        u_mults: None,
        v_mults: None,
    }
}

/// The Taylor derivatives of the prepared poles: `Bohm(u1, d1, d1, knots1, dim * (d2 + 1), poles)` and, for every
/// line, `Bohm(u2, d2, d2, knots2, dim, line)`.
fn cache_derivatives(pr: &mut Prepared, dimension: i32) -> R<()> {
    let (u1, d1, u2, d2) = (pr.u1, pr.d1, pr.u2, pr.d2);
    let d2p1 = d2 + 1;
    bohm_in_place(u1, d1, d1, &pr.knots1, dimension * d2p1, &mut pr.poles)?;
    for kk in 0..=d1 {
        let offset = ix(kk) * ix(dimension) * ix(d2p1);
        let buffer = tail(&mut pr.poles, offset)?;
        bohm_in_place(u2, d2, d2, &pr.knots2, dimension, buffer)?;
    }
    Ok(())
}

/// `BSplSLib::BuildCache` with `CachePoles` and `CacheWeights`: the Taylor expansion of the span normalised on
/// `[0, 1]` (homogeneous when rational). The arrays have `max(u_degree, v_degree) + 1` rows and
/// `min(u_degree, v_degree) + 1` columns; the weights are `Some` iff `weights` is.
pub fn build_cache(
    u: f64,
    v: f64,
    u_span_domain: f64,
    v_span_domain: f64,
    u_periodic: bool,
    v_periodic: bool,
    u_degree: i32,
    v_degree: i32,
    u_index: i32,
    v_index: i32,
    u_flat_knots: &[f64],
    v_flat_knots: &[f64],
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
) -> R<(Array2<Pnt>, Option<Array2<f64>>)> {
    check_degrees(u_degree, v_degree)?;
    let surface =
        surface_for_cache(u_periodic, v_periodic, u_degree, v_degree, u_flat_knots, v_flat_knots, poles, weights);
    let mut pr = prepare_eval(u, v, u_index, v_index, &surface)?;
    let d1 = ix(pr.d1);
    let d2 = ix(pr.d2);
    let d2p1 = d2 + 1;
    let rows = d2 + 1;
    let cols = d1 + 1;
    let mut cache_poles = filled(crate::bsplclib::util::mul(rows, cols)?, Pnt::new(0.0, 0.0, 0.0))?;
    let mut cache_weights = zeros(crate::bsplclib::util::mul(rows, cols)?)?;
    let dim = if pr.rational { 4 } else { 3 };
    cache_derivatives(&mut pr, dim)?;
    let (min_degree_domain, max_degree_domain) =
        if pr.ufirst { (u_span_domain, v_span_domain) } else { (v_span_domain, u_span_domain) };
    let mut factor0 = 1.0e0;
    for ii in 0..=d2 {
        let iii = ii + 1;
        let mut factor1 = 1.0e0;
        for jj in 0..=d1 {
            let jjj = jj + 1;
            let index = (jj * d2p1 + ii) * ix(dim);
            let f = factor0 * factor1;
            let slot = (iii - 1) * cols + (jjj - 1);
            wr(
                &mut cache_poles,
                slot,
                Pnt::new(f * rd(&pr.poles, index)?, f * rd(&pr.poles, index + 1)?, f * rd(&pr.poles, index + 2)?),
            )?;
            if pr.rational {
                wr(&mut cache_weights, slot, f * rd(&pr.poles, index + 3)?)?;
            }
            factor1 *= min_degree_domain / (jjj as f64);
        }
        factor0 *= max_degree_domain / (iii as f64);
    }
    if !pr.rational && weights.is_some() {
        // the surface is locally polynomial but constructed with weights: the weight polynomial is constant
        for slot in cache_weights.iter_mut() {
            *slot = 0.0e0;
        }
        wr(&mut cache_weights, 0, 1.0e0)?;
    }
    let cache_weights = if weights.is_some() { Some(array2(rows, cols, cache_weights)?) } else { None };
    Ok((array2(rows, cols, cache_poles)?, cache_weights))
}

/// `BSplSLib::BuildCache` with `theCacheArray`: `max + 1` rows and `(min + 1) * 3` columns (`* 4` with weights),
/// each pole `x y z [w]`.
pub fn build_cache_array(
    u: f64,
    v: f64,
    u_span_domain: f64,
    v_span_domain: f64,
    u_periodic: bool,
    v_periodic: bool,
    u_degree: i32,
    v_degree: i32,
    u_index: i32,
    v_index: i32,
    u_flat_knots: &[f64],
    v_flat_knots: &[f64],
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
) -> R<Array2<f64>> {
    check_degrees(u_degree, v_degree)?;
    let surface =
        surface_for_cache(u_periodic, v_periodic, u_degree, v_degree, u_flat_knots, v_flat_knots, poles, weights);
    let mut pr = prepare_eval(u, v, u_index, v_index, &surface)?;
    let on_param = weights.is_some();
    let d1 = ix(pr.d1);
    let d2 = ix(pr.d2);
    let d2p1 = d2 + 1;
    let dimension = if pr.rational { 4isize } else { 3 };
    // helps to store weights when PrepareEval did not found that the surface is locally polynomial
    let cache_shift = if on_param && !pr.rational { dimension + 1 } else { dimension };

    // domains[0] corresponds to variable with minimal degree, domains[1] to variable with maximal degree
    let domains = if pr.ufirst { [u_span_domain, v_span_domain] } else { [v_span_domain, u_span_domain] };

    cache_derivatives(&mut pr, to_i32(dimension)?)?;

    let cols = (d1 + 1) * cache_shift;
    let mut cache = zeros(crate::bsplclib::util::mul(d2 + 1, cols)?)?;
    let mut at = 0isize;
    // factors[0] corresponds to variable with minimal degree, factors[1] to variable with maximal degree
    let mut factor_max = 1.0;
    for row in 0..=d2 {
        let mut factor_min = 1.0;
        for col in 0..=d1 {
            let coefficients = (col * d2p1 + row) * dimension;
            let coeff = factor_min * factor_max;
            for i in 0..dimension {
                wr(&mut cache, at + i, rd(&pr.poles, coefficients + i)? * coeff)?;
            }
            at += cache_shift;
            factor_min *= domains[0] / ((col + 1) as f64);
        }
        factor_max *= domains[1] / ((row + 1) as f64);
    }

    // Fill the weights for the surface which is not locally polynomial
    if cache_shift > dimension {
        let mut at = cache_shift - 1;
        for _row in 0..=d2 {
            for _col in 0..=d1 {
                wr(&mut cache, at, 0.0)?;
                at += cache_shift;
            }
        }
        wr(&mut cache, cache_shift - 1, 1.0)?;
    }
    array2(d2 + 1, cols, cache)
}

// ---------------------------------------------------------------------------------------------
// Evaluation of a cache

/// The sizes of the cache arrays: `(min_degree, max_degree)` and a check of the shapes.
fn check_cache_shapes(
    u_degree: i32,
    v_degree: i32,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
    u_span_length: f64,
    v_span_length: f64,
) -> R<()> {
    check_degrees(u_degree, v_degree)?;
    let product = (ix(u_degree.max(v_degree)) + 1) * (ix(u_degree.min(v_degree)) + 1);
    if len_ix(poles.as_slice()) != product
        || weights.is_some_and(|w| len_ix(w.as_slice()) != product)
        || u_span_length == 0.0
        || v_span_length == 0.0
    {
        return Err(BSplError::OutOfRange);
    }
    Ok(())
}

/// `BSplSLib::CacheD0`.
pub fn cache_d0(
    u: f64,
    v: f64,
    u_degree: i32,
    v_degree: i32,
    u_cache_parameter: f64,
    v_cache_parameter: f64,
    u_span_length: f64,
    v_span_length: f64,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
) -> R<Pnt> {
    check_cache_shapes(u_degree, v_degree, poles, weights, u_span_length, v_span_length)?;
    // the CacheParameter is where the cache polynomial was evaluated in homogeneous form; the SpanLenght is the
    // normalizing factor so that the polynomial is between 0 and 1
    let parr = flat_points(poles);
    let (min_degree, max_degree, np0, np1, mut dimension);
    if u_degree <= v_degree {
        min_degree = u_degree;
        max_degree = v_degree;
        np1 = (u - u_cache_parameter) / u_span_length;
        np0 = (v - v_cache_parameter) / v_span_length;
        dimension = 3 * (u_degree + 1);
    } else {
        min_degree = v_degree;
        max_degree = u_degree;
        np0 = (u - u_cache_parameter) / u_span_length;
        np1 = (v - v_cache_parameter) / v_span_length;
        dimension = 3 * (v_degree + 1);
    }
    let locpoles = plib::no_derivative_eval_polynomial(np0, max_degree, dimension, max_degree * dimension, &parr)?;
    let point = plib::no_derivative_eval_polynomial(np1, min_degree, 3, 3 * min_degree, &locpoles)?;
    let (mut x, mut y, mut z) = (rd(&point, 0)?, rd(&point, 1)?, rd(&point, 2)?);
    if let Some(weights) = weights {
        dimension = min_degree + 1;
        let locw = plib::no_derivative_eval_polynomial(
            np0,
            max_degree,
            dimension,
            max_degree * dimension,
            weights.as_slice(),
        )?;
        let inverse = plib::no_derivative_eval_polynomial(np1, min_degree, 1, min_degree, &locw)?;
        let inverse = 1.0e0 / rd(&inverse, 0)?;
        x *= inverse;
        y *= inverse;
        z *= inverse;
    }
    Ok(Pnt::new(x, y, z))
}

/// Copies `values` into `target` at `offset`.
fn put(target: &mut [f64], offset: isize, values: &[f64]) -> R<()> {
    for (k, value) in values.iter().enumerate() {
        wr(target, offset + k as isize, *value)?;
    }
    Ok(())
}

fn at(buffer: &[f64], offset: isize) -> R<[f64; 3]> {
    Ok([rd(buffer, offset)?, rd(buffer, offset + 1)?, rd(buffer, offset + 2)?])
}

fn scaled(c: [f64; 3], factor: f64) -> Vec3 {
    Vec3::new(factor * c[0], factor * c[1], factor * c[2])
}

/// `BSplSLib::CacheD1`.
pub fn cache_d1(
    u: f64,
    v: f64,
    u_degree: i32,
    v_degree: i32,
    u_cache_parameter: f64,
    v_cache_parameter: f64,
    u_span_length: f64,
    v_span_length: f64,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
) -> R<Vec3s> {
    check_cache_shapes(u_degree, v_degree, poles, weights, u_span_length, v_span_length)?;
    let parr = flat_points(poles);
    // local_poles_array[2][2][3] and local_poles_and_weights_array[2][2][4], zero (they are all used by the rational
    // derivative)
    let mut lpa = zeros(12)?;
    let (min_degree, max_degree, inverse_min, inverse_max, np0, np1, mut dimension, min_is_u);
    if u_degree <= v_degree {
        min_degree = u_degree;
        max_degree = v_degree;
        inverse_min = 1.0e0 / u_span_length;
        inverse_max = 1.0e0 / v_span_length;
        np0 = (v - v_cache_parameter) * inverse_max;
        np1 = (u - u_cache_parameter) * inverse_min;
        dimension = 3 * (u_degree + 1);
        min_is_u = true;
    } else {
        min_degree = v_degree;
        max_degree = u_degree;
        inverse_min = 1.0e0 / v_span_length;
        inverse_max = 1.0e0 / u_span_length;
        np0 = (u - u_cache_parameter) * inverse_max;
        np1 = (v - v_cache_parameter) * inverse_min;
        dimension = 3 * (v_degree + 1);
        min_is_u = false;
    }

    let mut locpoles = plib::eval_polynomial(np0, 1, max_degree, dimension, &parr)?;
    let first = plib::eval_polynomial(np1, 1, min_degree, 3, tail_of(&locpoles, 0)?)?;
    put(&mut lpa, 0, &first)?;
    let second =
        plib::no_derivative_eval_polynomial(np1, min_degree, 3, 3 * min_degree, tail_of(&locpoles, ix(dimension))?)?;
    put(&mut lpa, 6, &second)?;

    if let Some(weights) = weights {
        dimension = min_degree + 1;
        let warr = weights.as_slice();
        let loc = plib::eval_polynomial(np0, 1, max_degree, dimension, warr)?;
        // only the first 2 * dimension values of the buffer are overwritten
        put(&mut locpoles, 0, &loc)?;
        let mut lw = zeros(4)?;
        let w0 = plib::eval_polynomial(np1, 1, min_degree, 1, tail_of(&locpoles, 0)?)?;
        put(&mut lw, 0, &w0)?;
        let w1 =
            plib::no_derivative_eval_polynomial(np1, min_degree, 1, min_degree, tail_of(&locpoles, ix(dimension))?)?;
        put(&mut lw, 2, &w1)?;
        let mut lpwa = zeros(16)?;
        for ab in 0..4isize {
            put(&mut lpwa, ab * 4, &at(&lpa, ab * 3)?)?;
            wr(&mut lpwa, ab * 4 + 3, rd(&lw, ab)?)?;
        }
        lpa = rational_derivative_core(1, 1, 1, 1, &lpwa, true)?;
    }

    let point = Pnt::new(rd(&lpa, 0)?, rd(&lpa, 1)?, rd(&lpa, 2)?);
    let vec_min = scaled(at(&lpa, 3)?, inverse_min);
    let vec_max = scaled(at(&lpa, 6)?, inverse_max);
    Ok(if min_is_u { (point, vec_min, vec_max) } else { (point, vec_max, vec_min) })
}

fn tail_of(buffer: &[f64], offset: isize) -> R<&[f64]> {
    usize::try_from(offset).ok().and_then(|k| buffer.get(k..)).ok_or(BSplError::OutOfRange)
}

/// `BSplSLib::CacheD2`: `(P, VecU, VecV, VecUU, VecUV, VecVV)`.
pub fn cache_d2(
    u: f64,
    v: f64,
    u_degree: i32,
    v_degree: i32,
    u_cache_parameter: f64,
    v_cache_parameter: f64,
    u_span_length: f64,
    v_span_length: f64,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
) -> R<Vec3s2> {
    check_cache_shapes(u_degree, v_degree, poles, weights, u_span_length, v_span_length)?;
    let parr = flat_points(poles);
    // local_poles_array[3][3][3], zero in case the min and max degree are less than 2
    let mut lpa = zeros(27)?;
    let (min_degree, max_degree, inverse_min, inverse_max, np0, np1, mut dimension, min_is_u);
    if u_degree <= v_degree {
        min_degree = u_degree;
        max_degree = v_degree;
        inverse_min = 1.0e0 / u_span_length;
        inverse_max = 1.0e0 / v_span_length;
        np0 = (v - v_cache_parameter) * inverse_max;
        np1 = (u - u_cache_parameter) * inverse_min;
        dimension = 3 * (u_degree + 1);
        min_is_u = true;
    } else {
        min_degree = v_degree;
        max_degree = u_degree;
        inverse_min = 1.0e0 / v_span_length;
        inverse_max = 1.0e0 / u_span_length;
        np0 = (u - u_cache_parameter) * inverse_max;
        np1 = (v - v_cache_parameter) * inverse_min;
        dimension = 3 * (v_degree + 1);
        min_is_u = false;
    }

    let mut locpoles = zeros(mul3(dimension)?)?;
    // initialize in case min or max degree are less than 2
    let min_ind_max = max_degree.min(2);
    let min_ind_min = min_degree.min(2);
    // (the rows from min_ind_max on are zero already)

    let first = plib::eval_polynomial(np0, min_ind_max, max_degree, dimension, &parr)?;
    put(&mut locpoles, 0, &first)?;

    put(&mut lpa, 0, &plib::eval_polynomial(np1, min_ind_min, min_degree, 3, tail_of(&locpoles, 0)?)?)?;
    put(&mut lpa, 9, &plib::eval_polynomial(np1, 1, min_degree, 3, tail_of(&locpoles, ix(dimension))?)?)?;
    put(
        &mut lpa,
        18,
        &plib::no_derivative_eval_polynomial(
            np1,
            min_degree,
            3,
            3 * min_degree,
            tail_of(&locpoles, 2 * ix(dimension))?,
        )?,
    )?;

    if let Some(weights) = weights {
        dimension = min_degree + 1;
        let warr = weights.as_slice();
        let loc = plib::eval_polynomial(np0, min_ind_max, max_degree, dimension, warr)?;
        put(&mut locpoles, 0, &loc)?;
        let mut lw = zeros(9)?;
        put(&mut lw, 0, &plib::eval_polynomial(np1, min_ind_min, min_degree, 1, tail_of(&locpoles, 0)?)?)?;
        put(&mut lw, 3, &plib::eval_polynomial(np1, 1, min_degree, 1, tail_of(&locpoles, ix(dimension))?)?)?;
        put(
            &mut lw,
            6,
            &plib::no_derivative_eval_polynomial(
                np1,
                min_degree,
                1,
                min_degree,
                tail_of(&locpoles, 2 * ix(dimension))?,
            )?,
        )?;
        let mut lpwa = zeros(36)?;
        for ab in 0..9isize {
            put(&mut lpwa, ab * 4, &at(&lpa, ab * 3)?)?;
            wr(&mut lpwa, ab * 4 + 3, rd(&lw, ab)?)?;
        }
        lpa = rational_derivative_core(2, 2, 2, 2, &lpwa, true)?;
    }

    let minmin = inverse_min * inverse_min;
    let minmax = inverse_min * inverse_max;
    let maxmax = inverse_max * inverse_max;

    let point = Pnt::new(rd(&lpa, 0)?, rd(&lpa, 1)?, rd(&lpa, 2)?);
    let vec_min = scaled(at(&lpa, 3)?, inverse_min);
    let vec_max = scaled(at(&lpa, 9)?, inverse_max);
    let vec_min_min = scaled(at(&lpa, 6)?, minmin);
    let vec_min_max = scaled(at(&lpa, 12)?, minmax);
    let vec_max_max = scaled(at(&lpa, 18)?, maxmax);
    Ok(if min_is_u {
        (point, vec_min, vec_max, vec_min_min, vec_min_max, vec_max_max)
    } else {
        (point, vec_max, vec_min, vec_max_max, vec_min_max, vec_min_min)
    })
}

fn mul3(dimension: i32) -> R<usize> {
    crate::bsplclib::util::mul(3, ix(dimension))
}

fn check_coefs(poles: &Array2<Pnt>, weights: Option<&Array2<f64>>) -> R<(i32, i32)> {
    if poles.rows() < 2
        || poles.cols() < 2
        || weights.is_some_and(|w| w.rows() != poles.rows() || w.cols() != poles.cols())
    {
        return Err(BSplError::OutOfRange);
    }
    Ok((to_i32(poles.cols() as isize - 1)?, to_i32(poles.rows() as isize - 1)?))
}

/// `BSplSLib::CoefsD0`.
pub fn coefs_d0(u: f64, v: f64, poles: &Array2<Pnt>, weights: Option<&Array2<f64>>) -> R<Pnt> {
    let (ud, vd) = check_coefs(poles, weights)?;
    cache_d0(u, v, ud, vd, 0.0, 0.0, 1.0, 1.0, poles, weights)
}

/// `BSplSLib::CoefsD1`.
pub fn coefs_d1(u: f64, v: f64, poles: &Array2<Pnt>, weights: Option<&Array2<f64>>) -> R<Vec3s> {
    let (ud, vd) = check_coefs(poles, weights)?;
    cache_d1(u, v, ud, vd, 0.0, 0.0, 1.0, 1.0, poles, weights)
}

/// `BSplSLib::CoefsD2`.
pub fn coefs_d2(u: f64, v: f64, poles: &Array2<Pnt>, weights: Option<&Array2<f64>>) -> R<Vec3s2> {
    let (ud, vd) = check_coefs(poles, weights)?;
    cache_d2(u, v, ud, vd, 0.0, 0.0, 1.0, 1.0, poles, weights)
}

/// `BSplSLib::PolesCoefficients` with weights: the power basis coefficients of the Bezier surface.
pub fn poles_coefficients(poles: &Array2<Pnt>, weights: Option<&Array2<f64>>) -> R<(Array2<Pnt>, Option<Array2<f64>>)> {
    check_coefs(poles, weights)?;
    let uclas = poles.rows();
    let vclas = poles.cols();
    let flat = |n: usize| -> R<Vec<f64>> {
        let mut knots = zeros(2 * n)?;
        for i in 0..n {
            wr(&mut knots, (i + n) as isize, 1.0)?;
        }
        Ok(knots)
    };
    let biduflatknots = flat(uclas)?;
    let bidvflatknots = flat(vclas)?;
    let (ud, vd) = (to_i32(uclas as isize - 1)?, to_i32(vclas as isize - 1)?);
    if uclas > vclas {
        build_cache(0.0, 0.0, 1.0, 1.0, false, false, ud, vd, 0, 0, &biduflatknots, &bidvflatknots, poles, weights)
    } else {
        // BuilCache exige que les resultats soient formates en [MaxCoeff,MinCoeff]
        let (cpoles, cweights) = build_cache(
            0.0,
            0.0,
            1.0,
            1.0,
            false,
            false,
            ud,
            vd,
            0,
            0,
            &biduflatknots,
            &bidvflatknots,
            poles,
            weights,
        )?;
        let mut out_poles = Vec::with_capacity(uclas * vclas);
        let mut out_weights = Vec::with_capacity(uclas * vclas);
        for ii in 0..uclas {
            for jj in 0..vclas {
                out_poles.push(*cpoles.get(jj, ii).ok_or(BSplError::OutOfRange)?);
                if let Some(w) = &cweights {
                    out_weights.push(*w.get(jj, ii).ok_or(BSplError::OutOfRange)?);
                }
            }
        }
        let new_weights = if cweights.is_some() { Some(Array2::new(uclas, vclas, out_weights)?) } else { None };
        Ok((Array2::new(uclas, vclas, out_poles)?, new_weights))
    }
}

/// `BSplSLib::PolesCoefficients` without weights.
pub fn poles_coefficients_nw(poles: &Array2<Pnt>) -> R<Array2<Pnt>> {
    Ok(poles_coefficients(poles, None)?.0)
}

// ---------------------------------------------------------------------------------------------
// BSplSLib_Cache

/// `BSplCLib_CacheParams`: the parameterization of one direction and the data of the current span.
#[derive(Debug)]
struct CacheParams {
    degree: i32,
    is_periodic: bool,
    first_parameter: f64,
    last_parameter: f64,
    span_index_min: i32,
    span_index_max: i32,
    span_start: f64,
    span_length: f64,
    span_index: i32,
}

impl CacheParams {
    fn new(degree: i32, periodic: bool, flat_knots: &[f64]) -> R<Self> {
        let n = len_ix(flat_knots);
        Ok(Self {
            degree,
            is_periodic: periodic,
            first_parameter: rd(flat_knots, ix(degree))?,
            last_parameter: rd(flat_knots, n - 1 - ix(degree))?,
            span_index_min: 1 + degree,
            span_index_max: to_i32(n - ix(degree) - 1)?,
            span_start: 0.0,
            span_length: 0.0,
            span_index: 0,
        })
    }

    /// Normalizes the parameter for periodic B-splines.
    fn periodic_normalization(&self, parameter: f64) -> f64 {
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

    /// Verifies validity of the cache using flat parameter of the point.
    fn is_cache_valid(&self, parameter: f64) -> bool {
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
        let eps = epsilon(self.last_parameter.abs().min(new_param.abs()));
        let delta_to_next = (delta - self.span_length).abs();
        delta_to_next > eps // next knot should be used instead
    }

    /// Computes the span of the parameter.
    fn locate_parameter(&mut self, parameter: f64, flat_knots: &[f64]) -> R<()> {
        self.span_index = 0;
        let (index, _new_u) =
            locate_parameter_opt(self.degree, flat_knots, None, parameter, self.is_periodic, self.span_index)?;
        self.span_index = index;
        self.span_start = rd(flat_knots, ix(index) - 1)?;
        self.span_length = rd(flat_knots, ix(index))? - self.span_start;
        Ok(())
    }
}

/// `BSplSLib_Cache`: the Taylor expansion of one span of a surface, to evaluate it fast on that span.
#[derive(Debug)]
pub struct Cache {
    is_rational: bool,
    params_u: CacheParams,
    params_v: CacheParams,
    poles_weights: Array2<f64>,
    built: bool,
}

impl Cache {
    /// The constructor: the cache is rational iff `weights` is `Some`. `Err` for a degree outside `1..=25`.
    pub fn new(
        u_degree: i32,
        u_periodic: bool,
        u_flat_knots: &[f64],
        v_degree: i32,
        v_periodic: bool,
        v_flat_knots: &[f64],
        weights: Option<&Array2<f64>>,
    ) -> Result<Cache, BSplError> {
        check_degrees(u_degree, v_degree)?;
        let is_rational = weights.is_some();
        let params_u = CacheParams::new(u_degree, u_periodic, u_flat_knots)?;
        let params_v = CacheParams::new(v_degree, v_periodic, v_flat_knots)?;
        let min_degree = u_degree.min(v_degree);
        let max_degree = u_degree.max(v_degree);
        let columns = (if is_rational { 4 } else { 3 }) * (ix(min_degree) + 1);
        let rows = ix(max_degree) + 1;
        let data = zeros(crate::bsplclib::util::mul(rows, columns)?)?;
        Ok(Cache { is_rational, params_u, params_v, poles_weights: array2(rows, columns, data)?, built: false })
    }

    /// `IsCacheValid`: true when the parameters belong to the span of the cache.
    pub fn is_cache_valid(&self, u: f64, v: f64) -> bool {
        self.params_u.is_cache_valid(u) && self.params_v.is_cache_valid(v)
    }

    /// `BuildCache`: the Taylor expansion of the span of the parameters.
    pub fn build_cache(
        &mut self,
        u: f64,
        v: f64,
        u_flat_knots: &[f64],
        v_flat_knots: &[f64],
        poles: &Array2<Pnt>,
        weights: Option<&Array2<f64>>,
    ) -> Result<(), BSplError> {
        // Normalize the parameters for periodical B-splines
        let new_u = self.params_u.periodic_normalization(u);
        let new_v = self.params_v.periodic_normalization(v);
        self.params_u.locate_parameter(new_u, u_flat_knots)?;
        self.params_v.locate_parameter(new_v, v_flat_knots)?;

        // BSplSLib uses different convention for span parameters than BSplCLib (Start is in the middle of the span
        // and length is half-span), thus we need to amend them here
        let span_length_u = 0.5 * self.params_u.span_length;
        let span_start_u = self.params_u.span_start + span_length_u;
        let span_length_v = 0.5 * self.params_v.span_length;
        let span_start_v = self.params_v.span_start + span_length_v;

        let array = build_cache_array(
            span_start_u,
            span_start_v,
            span_length_u,
            span_length_v,
            self.params_u.is_periodic,
            self.params_v.is_periodic,
            self.params_u.degree,
            self.params_v.degree,
            self.params_u.span_index,
            self.params_v.span_index,
            u_flat_knots,
            v_flat_knots,
            poles,
            weights,
        )?;
        if array.rows() != self.poles_weights.rows() || array.cols() != self.poles_weights.cols() {
            return Err(BSplError::OutOfRange);
        }
        self.poles_weights = array;
        self.built = true;
        Ok(())
    }

    fn ready(&self) -> R<()> {
        if self.built { Ok(()) } else { Err(BSplError::OutOfRange) }
    }

    /// Local parameters for `D0` (no derivative scaling needed).
    fn local_d0(&self, u: f64, v: f64) -> (f64, f64) {
        let new_u = self.params_u.periodic_normalization(u);
        let new_v = self.params_v.periodic_normalization(v);
        let span_length_u = 0.5 * self.params_u.span_length;
        let span_start_u = self.params_u.span_start + span_length_u;
        let span_length_v = 0.5 * self.params_v.span_length;
        let span_start_v = self.params_v.span_start + span_length_v;
        ((new_u - span_start_u) / span_length_u, (new_v - span_start_v) / span_length_v)
    }

    /// Local parameters and the inverse span lengths for derivative evaluation.
    fn local_params(&self, u: f64, v: f64) -> (f64, f64, f64, f64) {
        let new_u = self.params_u.periodic_normalization(u);
        let new_v = self.params_v.periodic_normalization(v);
        let span_length_u = 0.5 * self.params_u.span_length;
        let span_start_u = self.params_u.span_start + span_length_u;
        let span_length_v = 0.5 * self.params_v.span_length;
        let span_start_v = self.params_v.span_start + span_length_v;
        let inv_u = 1.0 / span_length_u;
        let inv_v = 1.0 / span_length_v;
        ((new_u - span_start_u) * inv_u, (new_v - span_start_v) * inv_v, inv_u, inv_v)
    }

    /// `D0`.
    pub fn d0(&self, u: f64, v: f64) -> Result<Pnt, BSplError> {
        self.ready()?;
        let (local_u, local_v) = self.local_d0(u, v);
        self.d0_local(local_u, local_v)
    }

    /// `D0Local`.
    pub fn d0_local(&self, local_u: f64, local_v: f64) -> Result<Pnt, BSplError> {
        self.ready()?;
        let parr = self.poles_weights.as_slice();
        let dimension = if self.is_rational { 4 } else { 3 };
        let cache_cols = to_i32(self.poles_weights.cols() as isize)?;
        let is_max_u = self.params_u.degree > self.params_v.degree;
        let (min_degree, max_degree) =
            (self.params_u.degree.min(self.params_v.degree), self.params_u.degree.max(self.params_v.degree));
        let (min_param, max_param) = if is_max_u { (local_v, local_u) } else { (local_u, local_v) };
        // Calculate intermediate value of cached polynomial along variable with maximal degree
        let transient =
            plib::no_derivative_eval_polynomial(max_param, max_degree, cache_cols, max_degree * cache_cols, parr)?;
        // Calculate total value along variable with minimal degree
        let point =
            plib::no_derivative_eval_polynomial(min_param, min_degree, dimension, dimension * min_degree, &transient)?;
        let (x, y, z) = (rd(&point, 0)?, rd(&point, 1)?, rd(&point, 2)?);
        if self.is_rational {
            let w = rd(&point, 3)?;
            Ok(Pnt::new(x / w, y / w, z / w))
        } else {
            Ok(Pnt::new(x, y, z))
        }
    }

    /// `EvaluatePolynomials`: the polynomials and their derivatives up to (`u_deriv_max`, `v_deriv_max`).
    fn evaluate_polynomials(
        &self,
        local_u: f64,
        local_v: f64,
        u_deriv_max: i32,
        v_deriv_max: i32,
        result: &mut [f64],
    ) -> R<()> {
        let parr = self.poles_weights.as_slice();
        let dimension = if self.is_rational { 4 } else { 3 };
        let cache_cols = to_i32(self.poles_weights.cols() as isize)?;

        let is_max_u = self.params_u.degree > self.params_v.degree;
        let (min_param, max_param) = if is_max_u { (local_v, local_u) } else { (local_u, local_v) };
        let (max_degree, min_degree) = if is_max_u {
            (self.params_u.degree, self.params_v.degree)
        } else {
            (self.params_v.degree, self.params_u.degree)
        };

        // Determine derivatives to calculate along each direction
        let max_deriv = if is_max_u { u_deriv_max } else { v_deriv_max };
        let min_deriv = if is_max_u { v_deriv_max } else { u_deriv_max };

        // Stride between rows in the result array (corresponds to one step in MaxParam direction)
        let row_stride = ix(min_deriv + 1) * ix(dimension);

        // Calculate intermediate values and derivatives of bivariate polynomial along variable with maximal degree
        let transient = plib::eval_polynomial(max_param, max_deriv, max_degree, cache_cols, parr)?;

        // Block 0: derivatives along variable with minimal degree for D0_max: (0,0), (0,1)...(0, min_deriv)
        put(result, 0, &plib::eval_polynomial(min_param, min_deriv, min_degree, dimension, &transient)?)?;

        if max_deriv > 0 {
            // Block 1: D1_max, at the start of the second row
            let deriv = if self.is_rational { min_deriv } else { min_deriv.min(1) };
            put(
                result,
                row_stride,
                &plib::eval_polynomial(min_param, deriv, min_degree, dimension, tail_of(&transient, ix(cache_cols))?)?,
            )?;

            if max_deriv > 1 {
                // Block 2: D2_max, at the start of the third row
                let deriv2 = if self.is_rational { min_deriv } else { 0 };
                let coefficients = tail_of(&transient, ix(cache_cols) << 1)?;
                let values = if deriv2 == 0 {
                    plib::no_derivative_eval_polynomial(
                        min_param,
                        min_degree,
                        dimension,
                        min_degree * dimension,
                        coefficients,
                    )?
                } else {
                    plib::eval_polynomial(min_param, deriv2, min_degree, dimension, coefficients)?
                };
                put(result, row_stride << 1, &values)?;
            }
        }

        if self.is_rational {
            // RationalDerivative is not safe for in-place operation: use a separate temporary storage
            let temp = if is_max_u {
                rational_derivative_core(u_deriv_max, v_deriv_max, u_deriv_max, v_deriv_max, result, true)?
            } else {
                // V is the max degree: the result array is V-major, swap U/V arguments
                rational_derivative_core(v_deriv_max, u_deriv_max, v_deriv_max, u_deriv_max, result, true)?
            };
            for (i, value) in temp.iter().enumerate() {
                wr(result, i as isize, *value)?;
            }
        }
        Ok(())
    }

    /// `D1`.
    pub fn d1(&self, u: f64, v: f64) -> Result<(Pnt, Vec3, Vec3), BSplError> {
        self.ready()?;
        let (local_u, local_v, inv_u, inv_v) = self.local_params(u, v);
        let (point, tangent_u, tangent_v) = self.d1_values(local_u, local_v)?;
        Ok((point, scale(&tangent_u, inv_u), scale(&tangent_v, inv_v)))
    }

    /// `D1Local`.
    pub fn d1_local(&self, local_u: f64, local_v: f64) -> Result<(Pnt, Vec3, Vec3), BSplError> {
        self.ready()?;
        let (point, tangent_u, tangent_v) = self.d1_values(local_u, local_v)?;
        // direct division for better numerical stability with very small span lengths
        let span_length_u = 0.5 * self.params_u.span_length;
        let span_length_v = 0.5 * self.params_v.span_length;
        Ok((point, divide(&tangent_u, span_length_u), divide(&tangent_v, span_length_v)))
    }

    fn d1_values(&self, local_u: f64, local_v: f64) -> R<(Pnt, Vec3, Vec3)> {
        let mut deriv = zeros(16)?;
        self.evaluate_polynomials(local_u, local_v, 1, 1, &mut deriv)?;
        let dim = 3isize;
        let point = Pnt::new(rd(&deriv, 0)?, rd(&deriv, 1)?, rd(&deriv, 2)?);
        let first = vec_of(&deriv, dim)?;
        let second = vec_of(&deriv, dim << 1)?;
        // Tangents are stored after the point coordinates; the order depends on which parameter has higher degree
        Ok(if self.params_u.degree > self.params_v.degree { (point, second, first) } else { (point, first, second) })
    }

    /// `D2`: `(P, TangentU, TangentV, CurvatureU, CurvatureV, CurvatureUV)`.
    pub fn d2(&self, u: f64, v: f64) -> Result<(Pnt, Vec3, Vec3, Vec3, Vec3, Vec3), BSplError> {
        self.ready()?;
        let (local_u, local_v, inv_u, inv_v) = self.local_params(u, v);
        let (point, tu, tv, cu, cv, cuv) = self.d2_values(local_u, local_v)?;
        Ok((
            point,
            scale(&tu, inv_u),
            scale(&tv, inv_v),
            scale(&cu, inv_u * inv_u),
            scale(&cv, inv_v * inv_v),
            scale(&cuv, inv_u * inv_v),
        ))
    }

    /// `D2Local`.
    pub fn d2_local(&self, local_u: f64, local_v: f64) -> Result<(Pnt, Vec3, Vec3, Vec3, Vec3, Vec3), BSplError> {
        self.ready()?;
        let (point, tu, tv, cu, cv, cuv) = self.d2_values(local_u, local_v)?;
        let span_length_u = 0.5 * self.params_u.span_length;
        let span_length_v = 0.5 * self.params_v.span_length;
        let span_length_u2 = span_length_u * span_length_u;
        let span_length_v2 = span_length_v * span_length_v;
        Ok((
            point,
            divide(&tu, span_length_u),
            divide(&tv, span_length_v),
            divide(&cu, span_length_u2),
            divide(&cv, span_length_v2),
            divide(&cuv, span_length_u * span_length_v),
        ))
    }

    fn d2_values(&self, local_u: f64, local_v: f64) -> R<(Pnt, Vec3, Vec3, Vec3, Vec3, Vec3)> {
        let mut deriv = zeros(36)?;
        self.evaluate_polynomials(local_u, local_v, 2, 2, &mut deriv)?;
        let dim = 3isize;
        let (shift, shift2, shift3, shift4, shift6) = (dim, dim << 1, (dim << 1) + dim, (dim << 2), 6 * dim);
        let point = Pnt::new(rd(&deriv, 0)?, rd(&deriv, 1)?, rd(&deriv, 2)?);
        let a = vec_of(&deriv, shift)?;
        let b = vec_of(&deriv, shift2)?;
        let c = vec_of(&deriv, shift3)?;
        let d = vec_of(&deriv, shift4)?;
        let e = vec_of(&deriv, shift6)?;
        // If Max=U: [0]=P, [Dim]=DV, [2Dim]=DVV, [3Dim]=DU, [4Dim]=DUV, [6Dim]=DUU
        // If Max=V: [0]=P, [Dim]=DU, [2Dim]=DUU, [3Dim]=DV, [4Dim]=DUV, [6Dim]=DVV
        Ok(if self.params_u.degree > self.params_v.degree {
            // tangent U, tangent V, curvature U, curvature V, curvature UV
            (point, c, a, e, b, d)
        } else {
            (point, a, c, b, e, d)
        })
    }
}

fn vec_of(buffer: &[f64], offset: isize) -> R<Vec3> {
    Ok(Vec3::new(rd(buffer, offset)?, rd(buffer, offset + 1)?, rd(buffer, offset + 2)?))
}

/// `gp_Vec::Multiply`.
fn scale(v: &Vec3, factor: f64) -> Vec3 {
    Vec3::new(v.x() * factor, v.y() * factor, v.z() * factor)
}

/// `gp_Vec::Divide`.
fn divide(v: &Vec3, divisor: f64) -> Vec3 {
    Vec3::new(v.x() / divisor, v.y() / divisor, v.z() / divisor)
}
