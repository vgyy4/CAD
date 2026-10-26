//! The 2D (`gp_Pnt2d`) forms of `BSplCLib` that `Geom2d_BezierCurve` and `Geom2d_BSplineCurve` use: the evaluation
//! at a span index (`D0`..`DN`), `Reverse`, `Resolution`, `MovePoint`, `MovePointAndTangent`, `BuildCache`,
//! `KnotAnalysis` (with `KnotForm`, `MultForm`, `MaxKnotMult`), `FlatIndex`, and the `InsertKnots`,
//! `RemoveKnot`, `IncreaseDegree`, `Unperiodize` and `Trimming` templates of `BSplCLib_CurveComputation.pxx`
//! (poles and weights become flat homogeneous arrays, the flat functions do the work).
//!
//! Arrays have lower bound 1 as in OCCT; an access outside an array, which OCCT 8.0.1 does not check, is
//! [`BSplError::OutOfRange`].
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use super::BSplError;
use super::flat::{
    bohm_in_place, build_knots, eval_in_place, first_u_knot_index, last_u_knot_index, locate_core,
    locate_parameter_opt, nb_poles, pole_index,
};
use super::knotops;
use super::util::{R, count, epsilon, filled, ix, len_ix, rd, rd1, to_i32, wr, wr1, zeros};
use crate::gp::{Mat2d, Pnt2d, Vec2d};
use crate::plib;

/// `GeomAbs_BSplKnotDistribution`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BSplKnotDistribution {
    /// `GeomAbs_NonUniform`.
    NonUniform = 0,
    /// `GeomAbs_Uniform`.
    Uniform = 1,
    /// `GeomAbs_QuasiUniform`.
    QuasiUniform = 2,
    /// `GeomAbs_PiecewiseBezier`.
    PiecewiseBezier = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnotDistribution {
    NonUniform,
    Uniform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultDistribution {
    NonConstant,
    Constant,
    QuasiConstant,
}

fn gpe(_: crate::gp::ConstructionError) -> BSplError {
    BSplError::OutOfRange
}

fn map_plib(e: plib::PLibError) -> BSplError {
    match e {
        plib::PLibError::OutOfRange => BSplError::OutOfRange,
        other => BSplError::PLib(other),
    }
}

/// `BSplCLib::FlatIndex`.
pub fn flat_index(degree: i32, index: i32, mults: &[i32], periodic: bool) -> R<i32> {
    let mut idx = ix(index);
    let m_lower = 1isize;
    for i in m_lower + 1..=ix(index) {
        idx += ix(rd1(mults, i)?) - 1;
    }
    if periodic {
        idx += ix(degree);
    } else {
        idx += ix(rd1(mults, m_lower)?) - 1;
    }
    to_i32(idx)
}

/// `BSplCLib::MaxKnotMult`.
pub fn max_knot_mult(mults: &[i32], from_k1: i32, to_k2: i32) -> R<i32> {
    let mut max_mult = rd1(mults, ix(from_k1))?;
    let mut i = ix(from_k1) + 1;
    while i <= ix(to_k2) {
        max_mult = max_mult.max(rd1(mults, i)?);
        i += 1;
    }
    Ok(max_mult)
}

// BSplCLib::KnotForm
fn knot_form(knots: &[f64], from_k1: i32, to_k2: i32) -> R<KnotDistribution> {
    if ix(from_k1) + 1 > len_ix(knots) {
        return Ok(KnotDistribution::Uniform);
    }
    let mut ui = rd1(knots, ix(from_k1))?.abs();
    let mut uj = rd1(knots, ix(from_k1) + 1)?.abs();
    let mut du0 = (uj - ui).abs();
    let mut eps = epsilon(ui) + epsilon(uj) + epsilon(du0);
    let mut i = ix(from_k1) + 1;
    while i < ix(to_k2) {
        ui = rd1(knots, i)?.abs();
        uj = rd1(knots, i + 1)?.abs();
        let du1 = (uj - ui).abs();
        if (du1 - du0).abs() > eps {
            return Ok(KnotDistribution::NonUniform);
        }
        du0 = du1;
        eps = epsilon(ui) + epsilon(uj) + epsilon(du0);
        i += 1;
    }
    Ok(KnotDistribution::Uniform)
}

// BSplCLib::MultForm
fn mult_form(mults: &[i32], from_k1: i32, to_k2: i32) -> R<MultDistribution> {
    let (first, last) = if from_k1 <= to_k2 { (ix(from_k1), ix(to_k2)) } else { (ix(to_k2), ix(from_k1)) };
    if first + 1 > len_ix(mults) {
        return Ok(MultDistribution::Constant);
    }
    let first_mult = rd1(mults, first)?;
    let mut form = MultDistribution::Constant;
    let mut mult = rd1(mults, first + 1)?;
    let mut i = first + 1;
    while i <= last && form != MultDistribution::NonConstant {
        if i == first + 1 {
            if mult != first_mult {
                form = MultDistribution::QuasiConstant;
            }
        } else if i == last {
            if form == MultDistribution::QuasiConstant {
                if first_mult != rd1(mults, i)? {
                    form = MultDistribution::NonConstant;
                }
            } else if mult != rd1(mults, i)? {
                form = MultDistribution::NonConstant;
            }
        } else {
            if mult != rd1(mults, i)? {
                form = MultDistribution::NonConstant;
            }
            mult = rd1(mults, i)?;
        }
        i += 1;
    }
    Ok(form)
}

/// `BSplCLib::KnotAnalysis`: the knot distribution and the greatest multiplicity of an inner knot.
pub fn knot_analysis(degree: i32, periodic: bool, knots: &[f64], mults: &[i32]) -> R<(BSplKnotDistribution, i32)> {
    let mut knot_form_result = BSplKnotDistribution::NonUniform;
    let nk = to_i32(len_ix(knots))?;
    let nm = to_i32(len_ix(mults))?;
    let k_set = knot_form(knots, 1, nk)?;
    if k_set == KnotDistribution::Uniform {
        let m_set = mult_form(mults, 1, nm)?;
        match m_set {
            MultDistribution::NonConstant => {}
            MultDistribution::Constant => {
                if nk == 2 {
                    knot_form_result = BSplKnotDistribution::PiecewiseBezier;
                } else if rd1(mults, 1)? == 1 {
                    knot_form_result = BSplKnotDistribution::Uniform;
                }
            }
            MultDistribution::QuasiConstant => {
                if rd1(mults, 1)? == degree + 1 {
                    let m = rd1(mults, 2)?;
                    if m == degree {
                        knot_form_result = BSplKnotDistribution::PiecewiseBezier;
                    } else if m == 1 {
                        knot_form_result = BSplKnotDistribution::QuasiUniform;
                    }
                }
            }
        }
    }
    let first_km = if periodic { 1 } else { first_u_knot_index(degree, mults)? };
    let last_km = if periodic { nk } else { last_u_knot_index(degree, mults)? };
    let mut max_knot_mult = 0;
    if last_km - first_km != 1 {
        let mut i = ix(first_km) + 1;
        while i < ix(last_km) {
            let multi = rd1(mults, i)?;
            max_knot_mult = max_knot_mult.max(multi);
            i += 1;
        }
    }
    Ok((knot_form_result, max_knot_mult))
}

/// `BSplCLib::IsRational(Weights, I1, I2)`.
pub fn is_rational(weights: &[f64], i1: i32, i2: i32) -> R<bool> {
    let l = len_ix(weights);
    if l == 0 {
        return Err(BSplError::OutOfRange);
    }
    let f = 1isize;
    let i3 = ix(i2) - f;
    let mut i = ix(i1) - f;
    while i < i3 {
        if rd(weights, i % l)? != rd(weights, (i + 1) % l)? {
            return Ok(true);
        }
        i += 1;
    }
    Ok(false)
}

// ---------------------------------------------------------------------------------------------
// Reverse

// BSplCLib_Reverse(Array, L) on any array
fn reverse_with<T>(a: &mut [T], l: i32) -> R<()> {
    let len = len_ix(a);
    if len == 0 {
        return Err(BSplError::OutOfRange);
    }
    let a_l = 1 + (ix(l) - 1) % len;
    if a_l < 1 || a_l > len {
        return Err(BSplError::OutOfRange);
    }
    let split = a_l as usize;
    let (head, tail) = a.split_at_mut(split);
    head.reverse();
    tail.reverse();
    Ok(())
}

/// `BSplCLib::Reverse(Poles, L)` for 2D poles.
pub fn reverse_poles(poles: &mut [Pnt2d], l: i32) -> R<()> {
    reverse_with(poles, l)
}

/// `BSplCLib::Reverse(Weights, L)`.
pub fn reverse_weights(weights: &mut [f64], l: i32) -> R<()> {
    reverse_with(weights, l)
}

/// `BSplCLib::Reverse(Mults)`.
pub fn reverse_mults(mults: &mut [i32]) {
    mults.reverse();
}

/// `BSplCLib::Reverse(Knots)`.
pub fn reverse_knots(knots: &mut [f64]) -> R<()> {
    let mut first = 1isize;
    let mut last = len_ix(knots);
    if last < 1 {
        return Err(BSplError::OutOfRange);
    }
    let mut kfirst = rd1(knots, first)?;
    let mut klast = rd1(knots, last)?;
    let mut tfirst = kfirst;
    let mut tlast = klast;
    first += 1;
    last -= 1;
    while first <= last {
        tfirst += klast - rd1(knots, last)?;
        tlast -= rd1(knots, first)? - kfirst;
        kfirst = rd1(knots, first)?;
        klast = rd1(knots, last)?;
        wr1(knots, first, tfirst)?;
        wr1(knots, last, tlast)?;
        first += 1;
        last -= 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Evaluation

struct Prepared {
    u: f64,
    knots: Vec<f64>,
    poles: Vec<f64>,
    dim: i32,
    rational: bool,
}

// BSplCLib_BuildEval (Dimension 2)
fn build_eval(degree: i32, index: i32, poles: &[Pnt2d], weights: Option<&[f64]>) -> R<Vec<f64>> {
    let p_upper = len_ix(poles);
    let mut ip = ix(index);
    let stride = if weights.is_some() { 3 } else { 2 };
    let mut out = zeros(count(degree + 1)? * stride)?;
    let mut pole = 0isize;
    for _ in 0..=degree {
        ip += 1;
        if ip > p_upper {
            ip = 1;
        }
        let p = rd1(poles, ip)?;
        match weights {
            None => {
                wr(&mut out, pole, p.x() + 0.0)?;
                wr(&mut out, pole + 1, p.y() + 0.0)?;
                pole += 2;
            }
            Some(w) => {
                let wi = rd1(w, ip)?;
                wr(&mut out, pole + 2, wi)?;
                wr(&mut out, pole, p.x() * wi)?;
                wr(&mut out, pole + 1, p.y() * wi)?;
                pole += 3;
            }
        }
    }
    Ok(out)
}

// PrepareEval_T
fn prepare_eval(
    u: f64,
    index: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: Option<&[i32]>,
) -> R<Prepared> {
    if degree < 1 || degree > super::max_degree() {
        return Err(BSplError::OutOfRange);
    }
    let (index, u) = locate_parameter_opt(degree, knots, mults, u, periodic, index)?;
    let local_knots = build_knots(degree, index, periodic, knots, mults)?;
    let index = match mults {
        None => index - (1 + degree),
        Some(m) => pole_index(degree, index, periodic, m)?,
    };
    let mut rational = weights.is_some();
    if let (true, Some(w)) = (rational, weights) {
        let w_lower = 1 + index;
        rational = is_rational(w, w_lower, w_lower + degree)?;
    }
    let (dim, local_poles) = if rational {
        (3, build_eval(degree, index, poles, weights)?)
    } else {
        (2, build_eval(degree, index, poles, None)?)
    };
    Ok(Prepared { u, knots: local_knots, poles: local_poles, dim, rational })
}

fn pnt(v: &[f64], i: isize) -> R<Pnt2d> {
    Ok(Pnt2d::new(rd(v, i)?, rd(v, i + 1)?))
}
fn vec(v: &[f64], i: isize) -> R<Vec2d> {
    Ok(Vec2d::new(rd(v, i)?, rd(v, i + 1)?))
}

/// `BSplCLib::D0(U, Index, Degree, Periodic, Poles, Weights, Knots, Mults, P)`; `mults == None` is `NoMults()`
/// (flat knots).
pub fn d0(
    u: f64,
    index: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: Option<&[i32]>,
) -> R<Pnt2d> {
    let mut pr = prepare_eval(u, index, degree, periodic, poles, weights, knots, mults)?;
    eval_in_place(pr.u, degree, &pr.knots, pr.dim, &mut pr.poles)?;
    if pr.rational {
        let w = rd(&pr.poles, 2)?;
        Ok(Pnt2d::new(rd(&pr.poles, 0)? / w, rd(&pr.poles, 1)? / w))
    } else {
        pnt(&pr.poles, 0)
    }
}

// The derivative values of order 0..n in dimension 2 (after Bohm and, for a rational curve, RationalDerivative).
fn derivatives(
    n: i32,
    u: f64,
    index: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: Option<&[i32]>,
) -> R<(Vec<f64>, bool)> {
    let mut pr = prepare_eval(u, index, degree, periodic, poles, weights, knots, mults)?;
    bohm_in_place(pr.u, degree, n, &pr.knots, pr.dim, &mut pr.poles)?;
    if pr.rational {
        let ders = plib::rational_derivative(degree, n, 2, &pr.poles, true).map_err(map_plib)?;
        Ok((ders, true))
    } else {
        Ok((pr.poles, false))
    }
}

/// `BSplCLib::D1`.
pub fn d1(
    u: f64,
    index: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: Option<&[i32]>,
) -> R<(Pnt2d, Vec2d)> {
    let (r, _) = derivatives(1, u, index, degree, periodic, poles, weights, knots, mults)?;
    Ok((pnt(&r, 0)?, vec(&r, 2)?))
}

/// `BSplCLib::D2`.
pub fn d2(
    u: f64,
    index: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: Option<&[i32]>,
) -> R<(Pnt2d, Vec2d, Vec2d)> {
    let (r, rational) = derivatives(2, u, index, degree, periodic, poles, weights, knots, mults)?;
    let v2 = if !rational && degree < 2 { Vec2d::new(0.0, 0.0) } else { vec(&r, 4)? };
    Ok((pnt(&r, 0)?, vec(&r, 2)?, v2))
}

/// `BSplCLib::D3`.
pub fn d3(
    u: f64,
    index: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: Option<&[i32]>,
) -> R<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
    let (r, rational) = derivatives(3, u, index, degree, periodic, poles, weights, knots, mults)?;
    let v2 = if !rational && degree < 2 { Vec2d::new(0.0, 0.0) } else { vec(&r, 4)? };
    let v3 = if !rational && degree < 3 { Vec2d::new(0.0, 0.0) } else { vec(&r, 6)? };
    Ok((pnt(&r, 0)?, vec(&r, 2)?, v2, v3))
}

/// `BSplCLib::DN`.
pub fn dn(
    u: f64,
    n: i32,
    index: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: Option<&[i32]>,
) -> R<Vec2d> {
    let mut pr = prepare_eval(u, index, degree, periodic, poles, weights, knots, mults)?;
    bohm_in_place(pr.u, degree, n, &pr.knots, pr.dim, &mut pr.poles)?;
    if pr.rational {
        let v = plib::rational_derivative(degree, n, 2, &pr.poles, false).map_err(map_plib)?;
        vec(&v, 0)
    } else if n > degree {
        Ok(Vec2d::new(0.0, 0.0))
    } else {
        vec(&pr.poles, ix(n) * 2)
    }
}

// ---------------------------------------------------------------------------------------------
// Resolution

/// `BSplCLib::Resolution(Poles, Weights, NumPoles, FlatKnots, Degree, Tolerance3D, UTolerance)` in 2D.
pub fn resolution(
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    num_poles: i32,
    flat_knots: &[f64],
    degree: i32,
    tolerance_3d: f64,
) -> R<f64> {
    let deg1 = ix(degree) + 1;
    let deg2 = (ix(degree) << 1) + 1;
    let np = ix(num_poles);
    if np < 1 {
        return Err(BSplError::OutOfRange);
    }
    let fk = |i: isize| rd(flat_knots, i);
    let pa = |i: isize| -> R<f64> {
        // PA[2 * k + c]
        let k = i >> 1;
        let c = i & 1;
        let p = poles.get(usize::try_from(k).map_err(|_| BSplError::OutOfRange)?).ok_or(BSplError::OutOfRange)?;
        Ok(if c == 0 { p.x() } else { p.y() })
    };
    let mut max_derivative = 0.0f64;
    let num_poles_k = len_ix(flat_knots) - deg1;
    if let Some(wg) = weights {
        let mut min_weights = rd(wg, 0)?;
        for ii in 1..np {
            let w = rd(wg, ii)?;
            if w < min_weights {
                min_weights = w;
            }
        }
        for ii in 1..num_poles_k {
            let ii_index = ii % np;
            let ii_in_dim = ii_index << 1;
            let ii_minus = (ii - 1) % np;
            let ii_mi_dim = ii_minus << 1;
            let pa_ii_in_dim_0 = pa(ii_in_dim)?;
            let pa_ii_in_dim_1 = pa(ii_in_dim + 1)?;
            let pa_ii_mi_dim_0 = pa(ii_mi_dim)?;
            let pa_ii_mi_dim_1 = pa(ii_mi_dim + 1)?;
            let wg_ii_index = rd(wg, ii_index)?;
            let wg_ii_minus = rd(wg, ii_minus)?;
            let mut inverse = fk(ii + ix(degree))? - fk(ii)?;
            inverse = 1.0e0 / inverse;
            let mut lower = ii - deg1;
            if lower < 0 {
                lower = 0;
            }
            let mut upper = deg2 + ii;
            if upper > num_poles_k {
                upper = num_poles_k;
            }
            let mut jj = lower;
            while jj < upper {
                let jj_index = (jj % np) << 1;
                let mut value = 0.0e0;
                let mut factor =
                    ((pa(jj_index)? - pa_ii_in_dim_0) * wg_ii_index) - ((pa(jj_index)? - pa_ii_mi_dim_0) * wg_ii_minus);
                if factor < 0.0 {
                    factor = -factor;
                }
                value += factor;
                factor = ((pa(jj_index + 1)? - pa_ii_in_dim_1) * wg_ii_index)
                    - ((pa(jj_index + 1)? - pa_ii_mi_dim_1) * wg_ii_minus);
                if factor < 0.0 {
                    factor = -factor;
                }
                value += factor;
                value *= inverse;
                if max_derivative < value {
                    max_derivative = value;
                }
                jj += 1;
            }
        }
        max_derivative /= min_weights;
    } else {
        for ii in 1..num_poles_k {
            let mut ii_index = (ii % np) << 1;
            let mut ii_minus = ((ii - 1) % np) << 1;
            let mut inverse = fk(ii + ix(degree))? - fk(ii)?;
            inverse = 1.0e0 / inverse;
            let mut value = 0.0e0;
            let mut factor = pa(ii_index)? - pa(ii_minus)?;
            if factor < 0.0 {
                factor = -factor;
            }
            value += factor;
            ii_index += 1;
            ii_minus += 1;
            factor = pa(ii_index)? - pa(ii_minus)?;
            if factor < 0.0 {
                factor = -factor;
            }
            value += factor;
            value *= inverse;
            if max_derivative < value {
                max_derivative = value;
            }
        }
    }
    max_derivative *= f64::from(degree);
    let real_small = f64::MIN_POSITIVE;
    Ok(if max_derivative > real_small { tolerance_3d / max_derivative } else { tolerance_3d / real_small })
}

// ---------------------------------------------------------------------------------------------
// MovePoint, MovePointAndTangent

/// `BSplCLib::MovePoint` in 2D: the first and last modified pole (0, 0 when the basis cannot be evaluated) and the
/// new poles.
pub fn move_point(
    u: f64,
    displ: &Vec2d,
    index1: i32,
    index2: i32,
    degree: i32,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    flat_knots: &[f64],
) -> R<(i32, i32, Vec<Pnt2d>)> {
    let mut basis = crate::mathlin::Matrix::filled(1, 1, 1, degree + 1, 0.0)?;
    let (error_code, first_non_zero) = super::eval_bspline_basis(0, degree + 1, flat_knots, u, false, &mut basis)?;
    if error_code != 0 {
        return Ok((0, 0, poles.to_vec()));
    }
    let b = |i: i32| -> R<f64> { Ok(basis.value(1, i)?) };
    let mut first_index = first_non_zero;
    let mut last_index = first_non_zero + degree;
    if first_index < index1 {
        first_index = index1;
    }
    if last_index > index2 {
        last_index = index2;
    }
    let mut max_value = 0.0;
    let mut kk1 = 0;
    let mut i = first_index - first_non_zero + 1;
    while i <= last_index - first_non_zero + 1 {
        if b(i)? > max_value {
            kk1 = i + first_non_zero - 1;
            max_value = b(i)?;
        }
        i += 1;
    }
    let mut kk2 = kk1;
    // With kk1 = 0 (no basis function above zero in the clipped range) OCCT reads the basis matrix outside its
    // bounds: the zeroed buffer after the matrix, or an unspecified word before it; both are read as 0 here.
    let next = {
        let i = kk1 - first_non_zero + 2;
        if i >= 1 && i <= degree + 1 { b(i)? } else { 0.0 }
    };
    if (kk1 + 1) <= last_index && (next - max_value).abs() < 1.0e-10 {
        kk2 = kk1 + 1;
    }
    let mut d1 = 0.0;
    let mut d2 = 0.0;
    for i in 1..=degree + 1 {
        let ii = i + first_non_zero - 1;
        let h_n;
        if let Some(w) = weights {
            h_n = rd1(w, ix(ii))? * b(i)?;
            d2 += h_n;
        } else {
            h_n = b(i)?;
        }
        if ii >= first_index && ii <= last_index {
            let dval = if ii < kk1 {
                f64::from(kk1 - ii)
            } else if ii > kk2 {
                f64::from(ii - kk2)
            } else {
                0.0
            };
            d1 += 1. / (dval + 1.) * h_n;
        }
    }
    let coef = if weights.is_some() { d2 / d1 } else { 1. / d1 };
    let mut new_poles = Vec::with_capacity(poles.len());
    for (k, p) in poles.iter().enumerate() {
        let i = to_i32(k as isize + 1)?;
        if i >= first_index && i <= last_index {
            let dval = if i < kk1 {
                f64::from(kk1 - i)
            } else if i > kk2 {
                f64::from(i - kk2)
            } else {
                0.0
            };
            let scaled = displ.multiplied(coef / (dval + 1.));
            new_poles.push(p.translated(&scaled));
        } else {
            new_poles.push(*p);
        }
    }
    Ok((first_index, last_index, new_poles))
}

// The extrapolation-free branch of BSplCLib::Eval(Parameter, PeriodicFlag, DerivativeRequest, ExtrapMode, Degree,
// FlatKnots, ArrayDimension = 1, Poles, [Weights,] Results [, WeightsResults]) with ExtrapMode = Degree, which
// never extrapolates. Returns the values of the orders 0..=derivative_request (and the weights' when given).
fn eval_scalar(
    parameter: f64,
    derivative_request: i32,
    degree: i32,
    flat_knots: &[f64],
    poles: &[f64],
    weights: Option<&[f64]>,
) -> R<(Vec<f64>, Vec<f64>)> {
    let order = degree + 1;
    let mut local_request = derivative_request;
    if local_request >= order {
        local_request = degree;
    }
    let modulus = len_ix(flat_knots) - ix(degree);
    if modulus < 1 {
        return Err(BSplError::OutOfRange);
    }
    let mut basis = crate::mathlin::Matrix::filled(1, local_request + 1, 1, order, 0.0)?;
    let (error_code, first_non_zero) =
        super::eval_bspline_basis(local_request, order, flat_knots, parameter, false, &mut basis)?;
    if error_code != 0 {
        return Err(BSplError::OutOfRange);
    }
    let mut presult = zeros(count(local_request + 1)?)?;
    let mut wresult = zeros(count(local_request + 1)?)?;
    for (index, ii) in (0isize..).zip(1..=local_request + 1) {
        let mut index1 = ix(first_non_zero);
        wr(&mut presult, index, 0.0)?;
        wr(&mut wresult, index, 0.0)?;
        for jj in 1..=order {
            let b = basis.value(ii, jj)?;
            match weights {
                Some(w) => {
                    let cur = rd(&presult, index)?;
                    wr(&mut presult, index, cur + rd1(poles, index1)? * rd1(w, index1)? * b)?;
                    let curw = rd(&wresult, index)?;
                    wr(&mut wresult, index, curw + rd1(w, index1)? * b)?;
                }
                None => {
                    let cur = rd(&presult, index)?;
                    wr(&mut presult, index, cur + rd1(poles, index1)? * b)?;
                }
            }
            index1 %= modulus;
            index1 += 1;
        }
    }
    Ok((presult, wresult))
}

/// `BSplCLib::MovePointAndTangent` in 2D: the error status (0, 1 or 2) and the new poles (the poles of the input
/// when the status is not 0).
pub fn move_point_and_tangent(
    u: f64,
    delta: &Vec2d,
    delta_derivatives: &Vec2d,
    tolerance: f64,
    degree: i32,
    starting_condition: i32,
    ending_condition: i32,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    flat_knots: &[f64],
) -> R<(i32, Vec<Pnt2d>)> {
    let order = degree + 1;
    let num_knots = to_i32(len_ix(flat_knots))?;
    let num_poles = num_knots - order;
    let conditions = starting_condition + ending_condition + 4;
    if poles.len() != count(num_poles.max(0))? && poles.len() != count(num_poles.max(0))? {
        return Err(BSplError::OutOfRange);
    }
    if !(starting_condition >= -1
        && starting_condition <= degree
        && ending_condition >= -1
        && ending_condition <= degree
        && conditions <= num_poles)
    {
        return Ok((2, poles.to_vec()));
    }
    let start_index = 1 + degree;
    let end_index = num_knots - degree;
    let mut cond = true;
    if starting_condition == -1 {
        cond = cond && (rd1(flat_knots, ix(start_index))? <= u);
    } else {
        cond = cond && (rd1(flat_knots, ix(start_index))? + tolerance < u);
    }
    if ending_condition == -1 {
        cond = cond && (rd1(flat_knots, ix(end_index))? >= u);
    } else {
        cond = cond && (rd1(flat_knots, ix(end_index))? - tolerance > u);
    }
    if !cond {
        return Ok((1, poles.to_vec()));
    }
    let schoenberg_points = super::build_schoenberg_points(degree, flat_knots, num_poles)?;
    let sp = |i: i32| rd1(&schoenberg_points, ix(i));
    let start_index = starting_condition + 2;
    let end_index = num_poles - ending_condition - 1;
    let (index, _new_parameter) = locate_core(&schoenberg_points, u, false, start_index, end_index, 0.0, 1.0)?;
    #[allow(clippy::if_same_then_else)] // the branches of the C++ chain, kept in order
    let other_index = if index == start_index {
        index + 1
    } else if index == end_index {
        index - 1
    } else if u - rd1(flat_knots, ix(index))? < rd1(flat_knots, ix(index) + 1)? - u {
        index - 1
    } else {
        index + 1
    };
    let type_ = 3;
    let start_num_poles = starting_condition + 2;
    let end_num_poles = num_poles - ending_condition - 1;
    let start_value = if start_num_poles == 1 {
        let mut sv = sp(num_poles)? - sp(1)?;
        sv = sp(1)? - sv;
        sv
    } else {
        sp(start_num_poles - 1)?
    };
    let end_value = if end_num_poles == num_poles {
        let mut ev = sp(num_poles)? - sp(1)?;
        ev += sp(num_poles)?;
        ev
    } else {
        sp(end_num_poles + 1)?
    };
    let np = count(num_poles)?;
    let mut first_function = zeros(np)?;
    let mut second_function = zeros(np)?;
    for ii in 1..start_num_poles {
        wr1(&mut first_function, ix(ii), 0.0)?;
        wr1(&mut second_function, ix(ii), 0.0)?;
    }
    for ii in end_num_poles + 1..=num_poles {
        wr1(&mut first_function, ix(ii), 0.0)?;
        wr1(&mut second_function, ix(ii), 0.0)?;
    }
    let power = |value: f64| -> f64 {
        let mut r = 1.0e0;
        for _ in 0..type_ {
            r *= value;
        }
        r
    };
    let mut divide = 1.0e0 / (sp(index)? - start_value);
    for ii in start_num_poles..=index {
        let mut value = sp(ii)? - start_value;
        value *= divide;
        wr1(&mut first_function, ix(ii), power(value))?;
    }
    divide = 1.0e0 / (end_value - sp(index)?);
    for ii in index..=end_num_poles {
        let mut value = end_value - sp(ii)?;
        value *= divide;
        wr1(&mut first_function, ix(ii), power(value))?;
    }
    divide = 1.0e0 / (sp(other_index)? - start_value);
    for ii in start_num_poles..=other_index {
        let mut value = sp(ii)? - start_value;
        value *= divide;
        wr1(&mut second_function, ix(ii), power(value))?;
    }
    divide = 1.0e0 / (end_value - sp(other_index)?);
    for ii in other_index..=end_num_poles {
        let mut value = end_value - sp(ii)?;
        value *= divide;
        wr1(&mut second_function, ix(ii), power(value))?;
    }
    // results[function][0 = value, 1 = derivative]
    let mut results = [[0.0f64; 2]; 2];
    if let Some(w) = weights {
        for (k, f) in [&first_function, &second_function].into_iter().enumerate() {
            let (pr, wr_) = eval_scalar(u, 1, degree, flat_knots, f, Some(w))?;
            let rd_ = plib::rational_derivatives(1, 1, &pr, &wr_).map_err(map_plib)?;
            if let Some(slot) = results.get_mut(k) {
                *slot = [rd(&rd_, 0)?, rd(&rd_, 1)?];
            }
        }
    } else {
        for (k, f) in [&first_function, &second_function].into_iter().enumerate() {
            let (pr, _) = eval_scalar(u, 1, degree, flat_knots, f, None)?;
            if let Some(slot) = results.get_mut(k) {
                *slot = [rd(&pr, 0)?, rd(&pr, 1)?];
            }
        }
    }
    let mut a_matrix = Mat2d::new();
    for (ii, row) in results.iter().enumerate() {
        for (jj, v) in row.iter().enumerate() {
            a_matrix.set_value(ii as i32 + 1, jj as i32 + 1, *v).map_err(gpe)?;
        }
    }
    a_matrix.invert().map_err(gpe)?;
    let delta_array = [delta.x(), delta.y()];
    let derivatives_array = [delta_derivatives.x(), delta_derivatives.y()];
    let mut a_vec = [0.0f64; 2];
    let mut b_vec = [0.0f64; 2];
    for ii in 0..2 {
        let d = delta_array.get(ii).copied().ok_or(BSplError::OutOfRange)?;
        let dd = derivatives_array.get(ii).copied().ok_or(BSplError::OutOfRange)?;
        if let Some(a) = a_vec.get_mut(ii) {
            *a = a_matrix.value(1, 1).map_err(gpe)? * d + a_matrix.value(2, 1).map_err(gpe)? * dd;
        }
        if let Some(b) = b_vec.get_mut(ii) {
            *b = a_matrix.value(1, 2).map_err(gpe)? * d + a_matrix.value(2, 2).map_err(gpe)? * dd;
        }
    }
    let mut new_poles = Vec::with_capacity(poles.len());
    for (ii, p) in poles.iter().enumerate() {
        let f1 = rd(&first_function, ii as isize)?;
        let f2 = rd(&second_function, ii as isize)?;
        let mut c = [p.x(), p.y()];
        for (jj, slot) in c.iter_mut().enumerate() {
            *slot += f1 * a_vec.get(jj).copied().ok_or(BSplError::OutOfRange)?;
            *slot += f2 * b_vec.get(jj).copied().ok_or(BSplError::OutOfRange)?;
        }
        new_poles.push(Pnt2d::new(c[0], c[1]));
    }
    Ok((0, new_poles))
}

// ---------------------------------------------------------------------------------------------
// BuildCache

/// `BSplCLib::BuildCache(U, SpanDomain, Periodic, Degree, FlatKnots, Poles, Weights, CachePoles, CacheWeights)` in
/// 2D: the cache poles and, when `weights` is given, the cache weights.
pub fn build_cache(
    u: f64,
    span_domain: f64,
    periodic: bool,
    degree: i32,
    flat_knots: &[f64],
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
) -> R<(Vec<Pnt2d>, Option<Vec<f64>>)> {
    let mut pr = prepare_eval(u, 0, degree, periodic, poles, weights, flat_knots, None)?;
    bohm_in_place(pr.u, degree, degree, &pr.knots, pr.dim, &mut pr.poles)?;
    let mut local_value = 1.0e0;
    let mut local_index = 0isize;
    let mut cache_poles = Vec::new();
    let mut cache_weights = weights.map(|_| Vec::new());
    if pr.rational {
        for ii in 1..=degree + 1 {
            cache_poles.push(Pnt2d::new(
                rd(&pr.poles, local_index)? * local_value,
                rd(&pr.poles, local_index + 1)? * local_value,
            ));
            local_index += 3;
            local_value *= span_domain / f64::from(ii);
        }
        local_index = 2;
        local_value = 1.0e0;
        for ii in 1..=degree + 1 {
            if let Some(cw) = cache_weights.as_mut() {
                cw.push(rd(&pr.poles, local_index)? * local_value);
            }
            local_index += 3;
            local_value *= span_domain / f64::from(ii);
        }
    } else {
        for ii in 1..=degree + 1 {
            cache_poles.push(Pnt2d::new(
                rd(&pr.poles, local_index)? * local_value,
                rd(&pr.poles, local_index + 1)? * local_value,
            ));
            local_index += 2;
            local_value *= span_domain / f64::from(ii);
        }
        if let Some(cw) = cache_weights.as_mut() {
            for ii in 1..=degree + 1 {
                cw.push(if ii == 1 { 1.0e0 } else { 0.0e0 });
            }
        }
    }
    Ok((cache_poles, cache_weights))
}

// ---------------------------------------------------------------------------------------------
// The flat-array templates with 2D poles and optional weights

/// Poles and weights of a curve.
pub type PolesWeights = (Vec<Pnt2d>, Option<Vec<f64>>);

/// The poles (and weights), knots and multiplicities of a curve.
pub type Curve2dOut = (PolesWeights, Vec<f64>, Vec<i32>);

fn flat_poles(poles: &[Pnt2d], weights: Option<&[f64]>) -> R<(i32, Vec<f64>)> {
    Ok(match weights {
        Some(w) => (3, plib::set_poles_2d_weighted(poles, w).map_err(map_plib)?),
        None => (2, plib::set_poles_2d(poles).map_err(map_plib)?),
    })
}

fn unflat_poles(dim: i32, flat: &[f64], rational: bool) -> R<PolesWeights> {
    let n = to_i32(len_ix(flat) / ix(dim))?;
    if rational {
        let (p, w) = plib::get_poles_2d_weighted(flat, n).map_err(map_plib)?;
        Ok((p, Some(w)))
    } else {
        Ok((plib::get_poles_2d(flat, n).map_err(map_plib)?, None))
    }
}

/// `BSplCLib::InsertKnots` in 2D (`BSplCLib_InsertKnots`): the new poles (and weights), knots and multiplicities.
pub fn insert_knots(
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: &[i32],
    add_knots: &[f64],
    add_mults: Option<&[i32]>,
    epsilon: f64,
    add: bool,
) -> R<(PolesWeights, Vec<f64>, Vec<i32>)> {
    let (dim, flat) = flat_poles(poles, weights)?;
    let (np, nk, nm) =
        knotops::insert_knots(degree, periodic, dim, &flat, knots, mults, add_knots, add_mults, epsilon, add)?;
    Ok((unflat_poles(dim, &np, weights.is_some())?, nk, nm))
}

/// `BSplCLib::PrepareInsertKnots`: the numbers of poles and knots after the insertion (`None` is `false`).
pub fn prepare_insert_knots(
    degree: i32,
    periodic: bool,
    knots: &[f64],
    mults: &[i32],
    add_knots: &[f64],
    add_mults: Option<&[i32]>,
    tolerance: f64,
    add: bool,
) -> R<Option<(i32, i32)>> {
    knotops::prepare_insert_knots(degree, periodic, knots, mults, add_knots, add_mults, tolerance, add)
}

/// `BSplCLib::RemoveKnot` in 2D (`BSplCLib_RemoveKnot`); `None` is `false`.
pub fn remove_knot(
    index: i32,
    mult: i32,
    degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: &[i32],
    tolerance: f64,
) -> R<Option<Curve2dOut>> {
    let (dim, flat) = flat_poles(poles, weights)?;
    match knotops::remove_knot(index, mult, degree, periodic, dim, &flat, knots, mults, tolerance)? {
        None => Ok(None),
        Some((np, nk, nm)) => Ok(Some((unflat_poles(dim, &np, weights.is_some())?, nk, nm))),
    }
}

/// `BSplCLib::IncreaseDegree` in 2D (`BSplCLib_IncreaseDegree`).
pub fn increase_degree(
    degree: i32,
    new_degree: i32,
    periodic: bool,
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
    knots: &[f64],
    mults: &[i32],
) -> R<(PolesWeights, Vec<f64>, Vec<i32>)> {
    let (dim, flat) = flat_poles(poles, weights)?;
    let (np, nk, nm) = knotops::increase_degree(degree, new_degree, periodic, dim, &flat, knots, mults)?;
    Ok((unflat_poles(dim, &np, weights.is_some())?, nk, nm))
}

/// `BSplCLib::IncreaseDegreeCountKnots`.
pub fn increase_degree_count_knots(degree: i32, new_degree: i32, periodic: bool, mults: &[i32]) -> R<i32> {
    knotops::increase_degree_count_knots(degree, new_degree, periodic, mults)
}

/// `BSplCLib::Unperiodize` in 2D (`BSplCLib_Unperiodize`): the new multiplicities, knots, poles (and weights).
pub fn unperiodize(
    degree: i32,
    mults: &[i32],
    knots: &[f64],
    poles: &[Pnt2d],
    weights: Option<&[f64]>,
) -> R<(Vec<i32>, Vec<f64>, PolesWeights)> {
    let (dim, flat) = flat_poles(poles, weights)?;
    let (nm, nk, np) = knotops::unperiodize(degree, dim, mults, knots, &flat)?;
    Ok((nm, nk, unflat_poles(dim, &np, weights.is_some())?))
}

/// `BSplCLib::PrepareUnperiodize`.
pub fn prepare_unperiodize(degree: i32, mults: &[i32]) -> R<(i32, i32)> {
    knotops::prepare_unperiodize(degree, mults)
}

/// `BSplCLib::NbPoles`.
pub fn nb_poles_of(degree: i32, periodic: bool, mults: &[i32]) -> R<i32> {
    nb_poles(degree, periodic, mults)
}

/// A `filled` helper for callers that need `n` copies.
pub fn unit_weights(n: usize) -> R<Vec<f64>> {
    filled(n, 1.0)
}
