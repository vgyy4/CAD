//! OCCT's `PLib` package: the static functions of `PLib` (polynomial evaluation, Lagrange and cubic Hermite
//! interpolation, Hermite polynomials, power basis <-> Bezier poles, trimming, rational derivatives, binomials,
//! Jacobi parameters, constraint orders, polynomial length), `PLib_JacobiPolynomial` and `PLib_HermitJacobi`.
//!
//! OCCT's arrays become slices (flat, group by group, power 0 first) and `Vec`s, the out-parameters returned tuples;
//! the arithmetic is done in the order of the C++ source. Checks that the OCCT libraries as built do not make
//! (`No_Exception`) become [`PLibError::OutOfRange`] where the C++ would read or write outside an array.
#![allow(clippy::float_cmp)]

mod hermit;
mod jacobi;
mod jacobi_coeffs;
mod jacobi_data;

pub use hermit::HermitJacobi;
pub use jacobi::JacobiPolynomial;

use crate::gp::{Pnt, Pnt2d, XYZ};
use crate::mathlin::{self, Gauss, MathError, Matrix, Vector};

/// The error type standing for the exceptions of `PLib` (and the checks the OCCT build leaves out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PLibError {
    /// `Standard_ConstructionError` raised by OCCT.
    Construction,
    /// `Standard_Failure` raised by OCCT.
    Failure,
    /// A check that OCCT 8.0.1 as built does not make, and that would have failed (out-of-range access, division by
    /// zero, unspecified result).
    OutOfRange,
    /// A wrong shape of a Rust argument.
    Argument,
    /// An error of `mathlin` used inside.
    Math(MathError),
}

impl From<MathError> for PLibError {
    fn from(e: MathError) -> Self {
        PLibError::Math(e)
    }
}

/// `GeomAbs_Shape`; the discriminants are the C++ enumerator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum GeomAbsShape {
    /// `GeomAbs_C0`.
    C0 = 0,
    /// `GeomAbs_G1`.
    G1 = 1,
    /// `GeomAbs_C1`.
    C1 = 2,
    /// `GeomAbs_G2`.
    G2 = 3,
    /// `GeomAbs_C2`.
    C2 = 4,
    /// `GeomAbs_C3`.
    C3 = 5,
    /// `GeomAbs_CN`.
    Cn = 6,
}

/// `NCollection_Array2<T>`: a row-major array with 0-based indices.
#[derive(Debug, Clone, PartialEq)]
pub struct Array2<T> {
    rows: usize,
    cols: usize,
    data: Vec<T>,
}

impl<T> Array2<T> {
    /// A `rows` x `cols` array from its items row by row; `Err(Argument)` unless `data.len() == rows * cols`.
    pub fn new(rows: usize, cols: usize, data: Vec<T>) -> Result<Self, PLibError> {
        if rows.checked_mul(cols) == Some(data.len()) {
            Ok(Self { rows, cols, data })
        } else {
            Err(PLibError::Argument)
        }
    }
    /// The number of rows (`ColLength()` of OCCT).
    pub fn rows(&self) -> usize {
        self.rows
    }
    /// The number of columns (`RowLength()` of OCCT).
    pub fn cols(&self) -> usize {
        self.cols
    }
    /// The item at (`row`, `col`).
    pub fn get(&self, row: usize, col: usize) -> Option<&T> {
        if row < self.rows && col < self.cols { self.data.get(row * self.cols + col) } else { None }
    }
    /// The items row by row.
    pub fn as_slice(&self) -> &[T] {
        &self.data
    }
    /// The items row by row, by value.
    pub fn into_vec(self) -> Vec<T> {
        self.data
    }
}

// ---------------------------------------------------------------------------------------------
// Small helpers. Index arithmetic follows the C++ (`int`, may go below zero between uses), so indices are `isize`
// and every access is checked.

fn ix(v: i32) -> isize {
    v as isize
}

fn len_ix<T>(s: &[T]) -> isize {
    s.len() as isize
}

fn rd<T: Copy>(s: &[T], i: isize) -> Result<T, PLibError> {
    usize::try_from(i).ok().and_then(|k| s.get(k)).copied().ok_or(PLibError::OutOfRange)
}

fn wr<T>(s: &mut [T], i: isize, v: T) -> Result<(), PLibError> {
    let slot = usize::try_from(i).ok().and_then(|k| s.get_mut(k)).ok_or(PLibError::OutOfRange)?;
    *slot = v;
    Ok(())
}

/// `s[i] = s[i] + v`
fn add_to(s: &mut [f64], i: isize, v: f64) -> Result<(), PLibError> {
    let x = rd(s, i)?;
    wr(s, i, x + v)
}

/// `s[i] = s[i] - v`
fn sub_from(s: &mut [f64], i: isize, v: f64) -> Result<(), PLibError> {
    let x = rd(s, i)?;
    wr(s, i, x - v)
}

/// `s[i] = s[i] * v`
fn mul_by(s: &mut [f64], i: isize, v: f64) -> Result<(), PLibError> {
    let x = rd(s, i)?;
    wr(s, i, x * v)
}

/// `s[i] = s[i] / v`
fn div_by(s: &mut [f64], i: isize, v: f64) -> Result<(), PLibError> {
    let x = rd(s, i)?;
    wr(s, i, x / v)
}

fn zeros(n: usize) -> Result<Vec<f64>, PLibError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| PLibError::OutOfRange)?;
    v.resize(n, 0.0);
    Ok(v)
}

/// `a * b` for two non-negative counts.
fn mul(a: i32, b: i32) -> Result<usize, PLibError> {
    if a < 0 || b < 0 {
        return Err(PLibError::OutOfRange);
    }
    usize::try_from(i64::from(a) * i64::from(b)).map_err(|_| PLibError::OutOfRange)
}

/// A non-negative count.
fn count(a: i32) -> Result<usize, PLibError> {
    usize::try_from(a).map_err(|_| PLibError::OutOfRange)
}

fn to_i32(a: usize) -> Result<i32, PLibError> {
    i32::try_from(a).map_err(|_| PLibError::OutOfRange)
}

fn need(slice_len: usize, wanted: usize) -> Result<(), PLibError> {
    if slice_len >= wanted { Ok(()) } else { Err(PLibError::OutOfRange) }
}

// ---------------------------------------------------------------------------------------------
// Poles <-> flat arrays

/// `PLib::SetPoles(Array1<gp_Pnt>, FP)`: `x y z` per pole.
pub fn set_poles_3d(poles: &[Pnt]) -> Result<Vec<f64>, PLibError> {
    let mut fp = Vec::with_capacity(poles.len() * 3);
    for p in poles {
        fp.push(p.x());
        fp.push(p.y());
        fp.push(p.z());
    }
    Ok(fp)
}

/// `PLib::SetPoles(Poles, Weights, FP)`: `x*w y*w z*w w` per pole.
pub fn set_poles_3d_weighted(poles: &[Pnt], weights: &[f64]) -> Result<Vec<f64>, PLibError> {
    need(weights.len(), poles.len()).map_err(|_| PLibError::Argument)?;
    let mut fp = Vec::with_capacity(poles.len() * 4);
    for (p, w) in poles.iter().zip(weights) {
        let w = *w;
        fp.push(p.x() * w);
        fp.push(p.y() * w);
        fp.push(p.z() * w);
        fp.push(w);
    }
    Ok(fp)
}

/// `PLib::SetPoles(Array1<gp_Pnt2d>, FP)`: `x y` per pole.
pub fn set_poles_2d(poles: &[Pnt2d]) -> Result<Vec<f64>, PLibError> {
    let mut fp = Vec::with_capacity(poles.len() * 2);
    for p in poles {
        fp.push(p.x());
        fp.push(p.y());
    }
    Ok(fp)
}

/// `PLib::SetPoles(Poles, Weights, FP)` in 2D: `x*w y*w w` per pole.
pub fn set_poles_2d_weighted(poles: &[Pnt2d], weights: &[f64]) -> Result<Vec<f64>, PLibError> {
    need(weights.len(), poles.len()).map_err(|_| PLibError::Argument)?;
    let mut fp = Vec::with_capacity(poles.len() * 3);
    for (p, w) in poles.iter().zip(weights) {
        let w = *w;
        fp.push(p.x() * w);
        fp.push(p.y() * w);
        fp.push(w);
    }
    Ok(fp)
}

/// The poles of a flat array of `dim`-groups (`dim` = 2 or 3 as `PLib::GetPoles` without weights).
fn flat_to_pnts(fp: &[f64]) -> Vec<Pnt> {
    fp.chunks_exact(3).filter_map(|c| if let [x, y, z] = *c { Some(Pnt::new(x, y, z)) } else { None }).collect()
}

fn flat_to_pnts2d(fp: &[f64]) -> Vec<Pnt2d> {
    fp.chunks_exact(2).filter_map(|c| if let [x, y] = *c { Some(Pnt2d::new(x, y)) } else { None }).collect()
}

/// `PLib::GetPoles(FP, Poles)`: `nb_poles` poles from `x y z` groups.
pub fn get_poles_3d(fp: &[f64], nb_poles: i32) -> Result<Vec<Pnt>, PLibError> {
    if nb_poles < 1 {
        return Err(PLibError::OutOfRange);
    }
    need(fp.len(), mul(nb_poles, 3)?)?;
    Ok(flat_to_pnts(fp.get(..mul(nb_poles, 3)?).ok_or(PLibError::OutOfRange)?))
}

/// `PLib::GetPoles(FP, Poles, Weights)`: groups `x*w y*w z*w w`; the poles are divided by the weight.
pub fn get_poles_3d_weighted(fp: &[f64], nb_poles: i32) -> Result<(Vec<Pnt>, Vec<f64>), PLibError> {
    if nb_poles < 1 {
        return Err(PLibError::OutOfRange);
    }
    need(fp.len(), mul(nb_poles, 4)?)?;
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for c in fp.chunks_exact(4).take(count(nb_poles)?) {
        if let [x, y, z, w] = *c {
            if w == 0.0 {
                return Err(PLibError::OutOfRange);
            }
            weights.push(w);
            poles.push(Pnt::new(x / w, y / w, z / w));
        }
    }
    Ok((poles, weights))
}

/// `PLib::GetPoles(FP, Poles)` in 2D.
pub fn get_poles_2d(fp: &[f64], nb_poles: i32) -> Result<Vec<Pnt2d>, PLibError> {
    if nb_poles < 1 {
        return Err(PLibError::OutOfRange);
    }
    need(fp.len(), mul(nb_poles, 2)?)?;
    Ok(flat_to_pnts2d(fp.get(..mul(nb_poles, 2)?).ok_or(PLibError::OutOfRange)?))
}

/// `PLib::GetPoles(FP, Poles, Weights)` in 2D: groups `x*w y*w w`.
pub fn get_poles_2d_weighted(fp: &[f64], nb_poles: i32) -> Result<(Vec<Pnt2d>, Vec<f64>), PLibError> {
    if nb_poles < 1 {
        return Err(PLibError::OutOfRange);
    }
    need(fp.len(), mul(nb_poles, 3)?)?;
    let mut poles = Vec::new();
    let mut weights = Vec::new();
    for c in fp.chunks_exact(3).take(count(nb_poles)?) {
        if let [x, y, w] = *c {
            if w == 0.0 {
                return Err(PLibError::OutOfRange);
            }
            weights.push(w);
            poles.push(Pnt2d::new(x / w, y / w));
        }
    }
    Ok((poles, weights))
}

// ---------------------------------------------------------------------------------------------
// Binomials

/// `PLib::Bin(N, P)`: the binomial coefficient for `0 <= P <= N <= 25` (`BSplCLib::MaxDegree()`), as the table of
/// OCCT holds it (exact integers).
pub fn bin(n: i32, p: i32) -> Result<f64, PLibError> {
    if !(0..=25).contains(&n) || p < 0 || p > n {
        return Err(PLibError::OutOfRange);
    }
    // C(n, k) = C(n, k - 1) * (n - k + 1) / k stays exact in integers (the same values as Pascal's triangle).
    let mut value: u64 = 1;
    for k in 1..=u64::from(p.unsigned_abs()) {
        value = value * (u64::from(n.unsigned_abs()) - k + 1) / k;
    }
    Ok(value as f64)
}

// ---------------------------------------------------------------------------------------------
// Rational derivatives

/// `PLib::RationalDerivative`: the derivatives of `x(t) / w(t)` up to order `derivative_request` from the
/// derivatives of the homogeneous function (`dimension` components of `x`, then `w`, per order, orders above
/// `degree` taken as 0). With `all` the result is the `dimension * (derivative_request + 1)` values of all orders,
/// without it only the `dimension` values of the last order.
pub fn rational_derivative(
    degree: i32,
    derivative_request: i32,
    dimension: i32,
    ders: &[f64],
    all: bool,
) -> Result<Vec<f64>, PLibError> {
    if degree < 0 || derivative_request < 0 || dimension < 1 {
        return Err(PLibError::OutOfRange);
    }
    let min_deg_requ = derivative_request.min(degree);
    need(ders.len(), mul(dimension + 1, min_deg_requ + 1)?)?;
    if rd(ders, ix(dimension))? == 0.0 {
        return Err(PLibError::OutOfRange);
    }
    let de_request1 = derivative_request + 1;
    let dimension1 = dimension + 1;
    let dimension2 = dimension << 1;
    let mut binomial = vec![1.0_f64; count(de_request1)?];
    // `RationalArray` is the result itself with `All`, a temporary of all the orders otherwise
    let mut rational = zeros(mul(dimension, de_request1)?)?;

    let inverse = 1.0e0 / rd(ders, ix(dimension))?;
    let mut index: isize = 0;
    let mut index2: isize = -ix(dimension2);
    let mut other_index: isize = 0;

    for ii in 0..=min_deg_requ {
        index2 += ix(dimension);
        let mut index1 = index2;
        for _ in 0..dimension {
            wr(&mut rational, index, rd(ders, other_index)?)?;
            index += 1;
            other_index += 1;
        }
        index -= ix(dimension);
        other_index += 1;
        for jj in (0..ii).rev() {
            let factor = rd(&binomial, ix(jj))? * rd(ders, ix((ii - jj) * dimension1 + dimension))?;
            for _ in 0..dimension {
                let v = factor * rd(&rational, index1)?;
                sub_from(&mut rational, index, v)?;
                index += 1;
                index1 += 1;
            }
            index -= ix(dimension);
            index1 -= ix(dimension2);
        }
        for jj in (1..=ii).rev() {
            let v = rd(&binomial, ix(jj - 1))?;
            add_to(&mut binomial, ix(jj), v)?;
        }
        for _ in 0..dimension {
            mul_by(&mut rational, index, inverse)?;
            index += 1;
        }
    }

    for ii in (min_deg_requ + 1)..=derivative_request {
        index2 += ix(dimension);
        let mut index1 = index2;
        for _ in 0..dimension {
            wr(&mut rational, index, 0.0e0)?;
            index += 1;
        }
        index -= ix(dimension);
        for jj in ((ii - min_deg_requ)..ii).rev() {
            let factor = rd(&binomial, ix(jj))? * rd(ders, ix((ii - jj) * dimension1 + dimension))?;
            for _ in 0..dimension {
                let v = factor * rd(&rational, index1)?;
                sub_from(&mut rational, index, v)?;
                index += 1;
                index1 += 1;
            }
            index -= ix(dimension);
            index1 -= ix(dimension2);
        }
        for jj in (1..=ii).rev() {
            let v = rd(&binomial, ix(jj - 1))?;
            add_to(&mut binomial, ix(jj), v)?;
        }
        for _ in 0..dimension {
            mul_by(&mut rational, index, inverse)?;
            index += 1;
        }
    }

    if all {
        Ok(rational)
    } else {
        let start = mul(dimension, derivative_request)?;
        let tail = rational.get(start..start + count(dimension)?).ok_or(PLibError::OutOfRange)?;
        Ok(tail.to_vec())
    }
}

/// `PLib::RationalDerivatives`: the derivatives of `x / w` up to order `derivative_request` from the derivatives
/// of `x` (`dimension` values per order) and of the weight; `dimension * (derivative_request + 1)` values.
pub fn rational_derivatives(
    derivative_request: i32,
    dimension: i32,
    poles_derivatives: &[f64],
    weights_derivatives: &[f64],
) -> Result<Vec<f64>, PLibError> {
    if derivative_request < 0 || dimension < 1 {
        return Err(PLibError::OutOfRange);
    }
    let de_request1 = derivative_request + 1;
    need(poles_derivatives.len(), mul(dimension, de_request1)?)?;
    need(weights_derivatives.len(), count(de_request1)?)?;
    if rd(weights_derivatives, 0)? == 0.0 {
        return Err(PLibError::OutOfRange);
    }
    let dimension2 = dimension << 1;
    let mut binomial = vec![1.0_f64; count(de_request1)?];
    let mut rational = zeros(mul(dimension, de_request1)?)?;
    let inverse = 1.0e0 / rd(weights_derivatives, 0)?;
    let mut index: isize = 0;
    let mut index2: isize = -ix(dimension2);

    for ii in 0..de_request1 {
        index2 += ix(dimension);
        let mut index1 = index2;
        for _ in 0..dimension {
            wr(&mut rational, index, rd(poles_derivatives, index)?)?;
            index += 1;
        }
        index -= ix(dimension);
        for jj in (0..ii).rev() {
            let factor = rd(&binomial, ix(jj))? * rd(weights_derivatives, ix(ii - jj))?;
            for _ in 0..dimension {
                let v = factor * rd(&rational, index1)?;
                sub_from(&mut rational, index, v)?;
                index += 1;
                index1 += 1;
            }
            index -= ix(dimension);
            index1 -= ix(dimension2);
        }
        for jj in (1..=ii).rev() {
            let v = rd(&binomial, ix(jj - 1))?;
            add_to(&mut binomial, ix(jj), v)?;
        }
        for _ in 0..dimension {
            mul_by(&mut rational, index, inverse)?;
            index += 1;
        }
    }
    Ok(rational)
}

// ---------------------------------------------------------------------------------------------
// Polynomial evaluation

/// `eval_poly0` / `eval_poly0_runtime`: the value only. `top` is the offset of the group of the highest power.
fn eval_poly0(coeffs: &[f64], top: isize, degree: i32, par: f64, dim: i32) -> Result<Vec<f64>, PLibError> {
    let mut res = zeros(count(dim)?)?;
    for i in 0..ix(dim) {
        wr(&mut res, i, rd(coeffs, top + i)?)?;
    }
    let mut base = top;
    for _ in 0..degree {
        base -= ix(dim);
        for i in 0..ix(dim) {
            let v = rd(&res, i)? * par + rd(coeffs, base + i)?;
            wr(&mut res, i, v)?;
        }
    }
    Ok(res)
}

/// `eval_poly1` / `eval_poly1_runtime`: value and first derivative.
fn eval_poly1(coeffs: &[f64], top: isize, degree: i32, par: f64, dim: i32) -> Result<Vec<f64>, PLibError> {
    let d = ix(dim);
    let mut res = zeros(mul(dim, 2)?)?;
    for i in 0..d {
        wr(&mut res, i, rd(coeffs, top + i)?)?;
        wr(&mut res, d + i, 0.0)?;
    }
    let mut base = top;
    for _ in 0..degree {
        base -= d;
        for i in 0..d {
            let val = rd(&res, i)?;
            let d1 = rd(&res, d + i)? * par + val;
            wr(&mut res, d + i, d1)?;
            wr(&mut res, i, val * par + rd(coeffs, base + i)?)?;
        }
    }
    Ok(res)
}

/// `eval_poly2` / `eval_poly2_runtime`: value, first and second derivative.
fn eval_poly2(coeffs: &[f64], top: isize, degree: i32, par: f64, dim: i32) -> Result<Vec<f64>, PLibError> {
    let d = ix(dim);
    let mut res = zeros(mul(dim, 3)?)?;
    for i in 0..d {
        wr(&mut res, i, rd(coeffs, top + i)?)?;
        wr(&mut res, d + i, 0.0)?;
        wr(&mut res, 2 * d + i, 0.0)?;
    }
    let mut base = top;
    for _ in 0..degree {
        base -= d;
        for i in 0..d {
            let d1 = rd(&res, d + i)?;
            let val = rd(&res, i)?;
            let d2 = rd(&res, 2 * d + i)? * par + d1 * 2.0;
            wr(&mut res, 2 * d + i, d2)?;
            wr(&mut res, d + i, d1 * par + val)?;
            wr(&mut res, i, val * par + rd(coeffs, base + i)?)?;
        }
    }
    Ok(res)
}

/// `PLib::EvalPolynomial`: the value and the derivatives up to `derivative_order` of the polynomial of the
/// `degree + 1` groups of `dimension` coefficients (power 0 first) at `u`; derivative `d` is at
/// `res[d * dimension ..]`.
pub fn eval_polynomial(
    u: f64,
    derivative_order: i32,
    degree: i32,
    dimension: i32,
    coeffs: &[f64],
) -> Result<Vec<f64>, PLibError> {
    if derivative_order < 0 || degree < 0 || dimension < 1 {
        return Err(PLibError::OutOfRange);
    }
    need(coeffs.len(), mul(degree + 1, dimension)?)?;
    let top = ix(degree) * ix(dimension);
    match derivative_order {
        1 => eval_poly1(coeffs, top, degree, u, dimension),
        2 => eval_poly2(coeffs, top, degree, u, dimension),
        _ => {
            // General case (DerivativeRequest = 0 or > 2)
            let dim = ix(dimension);
            let res_size = mul(derivative_order + 1, dimension)?;
            let mut res = zeros(res_size)?;
            let mut coeff_at = top;
            for _ in 0..=degree {
                let mut ptr = ix_len(res_size) - dim;
                // Calculating derivatives of the polynomial
                for deriv in (1..=derivative_order).rev() {
                    let original = ptr - dim;
                    for ind in 0..dim {
                        let v = rd(&res, ptr + ind)? * u + rd(&res, original + ind)? * f64::from(deriv);
                        wr(&mut res, ptr + ind, v)?;
                    }
                    ptr = original;
                }
                // Calculating the value of the polynomial
                for ind in 0..dim {
                    let v = rd(&res, ptr + ind)? * u + rd(coeffs, coeff_at + ind)?;
                    wr(&mut res, ptr + ind, v)?;
                }
                coeff_at -= dim;
            }
            Ok(res)
        }
    }
}

fn ix_len(n: usize) -> isize {
    n as isize
}

/// `PLib::NoDerivativeEvalPolynomial`: the value (`dimension` values) of the polynomial whose top group starts at
/// `coeffs[degree_dimension]`.
pub fn no_derivative_eval_polynomial(
    u: f64,
    degree: i32,
    dimension: i32,
    degree_dimension: i32,
    coeffs: &[f64],
) -> Result<Vec<f64>, PLibError> {
    if degree < 0 || dimension < 1 || degree_dimension < 0 {
        return Err(PLibError::OutOfRange);
    }
    if i64::from(degree_dimension) < i64::from(degree) * i64::from(dimension) {
        return Err(PLibError::OutOfRange);
    }
    if i64::from(degree_dimension) + i64::from(dimension) > len_ix(coeffs) as i64 {
        return Err(PLibError::OutOfRange);
    }
    eval_poly0(coeffs, ix(degree_dimension), degree, u, dimension)
}

/// `PLib::EvalPoly2Var`: the partial derivative of order (`u_derivative`, `v_derivative`) of
/// `sum c_ij u^i v^j` at (`u`, `v`); the coefficient of `u^i v^j` is the group `coeffs[(i * (v_degree + 1) + j) *
/// dimension ..]`.
#[allow(clippy::too_many_arguments)]
pub fn eval_poly2var(
    u: f64,
    v: f64,
    u_derivative: i32,
    v_derivative: i32,
    u_degree: i32,
    v_degree: i32,
    dimension: i32,
    coeffs: &[f64],
) -> Result<Vec<f64>, PLibError> {
    if u_derivative < 0 || v_derivative < 0 || u_degree < 0 || v_degree < 0 || dimension < 1 {
        return Err(PLibError::OutOfRange);
    }
    let udim = to_i32(mul(v_degree + 1, dimension)?)?;
    let index = mul(udim, u_derivative)?;
    let curve = eval_polynomial(u, u_derivative, u_degree, udim, coeffs)?;
    let rest = curve.get(index..).ok_or(PLibError::OutOfRange)?;
    let point = eval_polynomial(v, v_derivative, v_degree, dimension, rest)?;
    let start = mul(dimension, v_derivative)?;
    let end = start + count(dimension)?;
    Ok(point.get(start..end).ok_or(PLibError::OutOfRange)?.to_vec())
}

/// `PLib::EvalLagrange`: the Lagrange polynomial through `degree + 1` points (group by group in `values`, with the
/// `parameters`) and its derivatives up to `derivative_order` (at most `degree`). The code is `1` (and the result
/// empty) when two parameters of a divided difference differ by less than `RealSmall()`.
pub fn eval_lagrange(
    parameter: f64,
    derivative_order: i32,
    degree: i32,
    dimension: i32,
    values: &[f64],
    parameters: &[f64],
) -> Result<(i32, Vec<f64>), PLibError> {
    if derivative_order < 0 || degree < 0 || dimension < 1 {
        return Err(PLibError::OutOfRange);
    }
    let total = mul(degree + 1, dimension)?;
    need(values.len(), total)?;
    need(parameters.len(), count(degree + 1)?)?;
    let local_request = derivative_order.min(degree);
    let dim = ix(dimension);
    let mut dd: Vec<f64> = values.get(..total).ok_or(PLibError::OutOfRange)?.to_vec();
    let mut res = zeros(mul(local_request + 1, dimension)?)?;

    //  Build the divided differences array
    for ii in (0..=degree).rev() {
        for jj in ((degree - ii + 1)..=degree).rev() {
            let index = ix(jj) * dim;
            let index1 = index - dim;
            for kk in 0..dim {
                let v = rd(&dd, index1 + kk)?;
                sub_from(&mut dd, index + kk, v)?;
            }
            let mut difference = rd(parameters, ix(jj))? - rd(parameters, ix(jj - degree - 1 + ii))?;
            if difference.abs() < f64::MIN_POSITIVE {
                return Ok((1, Vec::new()));
            }
            difference = 1.0e0 / difference;
            for kk in 0..dim {
                mul_by(&mut dd, index + kk, difference)?;
            }
        }
    }

    // Evaluate the divided difference array polynomial
    let index = ix(degree) * dim;
    for kk in 0..dim {
        wr(&mut res, kk, rd(&dd, index + kk)?)?;
    }
    for ii in dim..(ix(local_request + 1) * dim) {
        wr(&mut res, ii, 0.0e0)?;
    }
    for ii in (1..=degree).rev() {
        let difference = parameter - rd(parameters, ix(ii - 1))?;
        for jj in (1..=local_request).rev() {
            let index = ix(jj) * dim;
            let index1 = index - dim;
            for kk in 0..dim {
                mul_by(&mut res, index + kk, difference)?;
                let v = rd(&res, index1 + kk)? * f64::from(jj);
                add_to(&mut res, index + kk, v)?;
            }
        }
        let index = ix(ii - 1) * dim;
        for kk in 0..dim {
            mul_by(&mut res, kk, difference)?;
            add_to(&mut res, kk, rd(&dd, index + kk)?)?;
        }
    }
    Ok((0, res))
}

/// `PLib::EvalCubicHermite`: the cubic through two points (`values`: first then last) with the given derivatives
/// there, and its derivatives up to `derivative_order` (at most 3). The code is always 0.
pub fn eval_cubic_hermite(
    parameter: f64,
    derivative_order: i32,
    dimension: i32,
    values: &[f64],
    derivatives: &[f64],
    parameters: &[f64],
) -> Result<(i32, Vec<f64>), PLibError> {
    if derivative_order < 0 || dimension < 1 {
        return Err(PLibError::OutOfRange);
    }
    let degree: i32 = 3;
    let two_dim = mul(dimension, 2)?;
    need(values.len(), two_dim)?;
    need(derivatives.len(), two_dim)?;
    need(parameters.len(), 2)?;
    let first_last = [rd(parameters, 0)?, rd(parameters, 1)?];
    if first_last[0] == first_last[1] {
        return Err(PLibError::OutOfRange);
    }
    let local_request = derivative_order.min(degree);
    let dim = ix(dimension);
    let parameters_array = [first_last[0], first_last[0], first_last[1], first_last[1]];
    let mut dd = zeros(mul(degree + 1, dimension)?)?;
    let mut res = zeros(mul(local_request + 1, dimension)?)?;

    let mut inverse = first_last[1] - first_last[0];
    inverse = 1.0e0 / inverse;

    for ii in 0..dim {
        let jj = dim + ii;
        let kk = 2 * dim + ii;
        let pp = 3 * dim + ii;
        wr(&mut dd, ii, rd(values, ii)?)?;
        wr(&mut dd, kk, inverse * (rd(values, jj)? - rd(values, ii)?))?;
        wr(&mut dd, jj, rd(derivatives, ii)?)?;
        wr(&mut dd, pp, rd(derivatives, jj)?)?;
    }

    for ii in 1..=degree {
        for jj in ((ii + 1)..=degree).rev() {
            let index = ix(jj) * dim;
            let index1 = index - dim;
            for kk in 0..dim {
                let v = rd(&dd, index1 + kk)?;
                sub_from(&mut dd, index + kk, v)?;
            }
            for kk in 0..dim {
                mul_by(&mut dd, index + kk, inverse)?;
            }
        }
    }

    let index = ix(degree) * dim;
    for kk in 0..dim {
        wr(&mut res, kk, rd(&dd, index + kk)?)?;
    }
    for ii in dim..(ix(local_request + 1) * dim) {
        wr(&mut res, ii, 0.0e0)?;
    }
    for ii in (1..=degree).rev() {
        let difference = parameter - rd(&parameters_array, ix(ii - 1))?;
        for jj in (1..=local_request).rev() {
            let index = ix(jj) * dim;
            let index1 = index - dim;
            for kk in 0..dim {
                mul_by(&mut res, index + kk, difference)?;
                let v = rd(&res, index1 + kk)? * f64::from(jj);
                add_to(&mut res, index + kk, v)?;
            }
        }
        let index = ix(ii - 1) * dim;
        for kk in 0..dim {
            mul_by(&mut res, kk, difference)?;
            add_to(&mut res, kk, rd(&dd, index + kk)?)?;
        }
    }
    Ok((0, res))
}

// ---------------------------------------------------------------------------------------------
// Hermite polynomials

/// `PLib::HermiteCoefficients`: the coefficients (lowest power first, one row each) of the Hermite polynomials
/// of the orders `first_order`, `last_order` on `[first_parameter, last_parameter]`, written into the
/// `n x n` (`n = first_order + last_order + 2`, bounds `1..=n`) `matrix`. `Ok(false)` (matrix untouched) for
/// invalid orders or parameters or a singular system.
pub fn hermite_coefficients(
    first_parameter: f64,
    last_parameter: f64,
    first_order: i32,
    last_order: i32,
    matrix: &mut Matrix,
) -> Result<bool, PLibError> {
    // The C++ builds math_Vector(1, NbCoeff) first, which crashes for a negative length.
    let nb_coeff = first_order.checked_add(last_order).and_then(|s| s.checked_add(2)).ok_or(PLibError::OutOfRange)?;
    if nb_coeff < 0 {
        return Err(PLibError::OutOfRange);
    }

    // Test de validites
    if first_order < 0 || last_order < 0 {
        return Ok(false);
    }
    let d1 = first_parameter.abs();
    let mut d2 = last_parameter.abs();
    if d1 > 100.0 || d2 > 100.0 {
        return Ok(false);
    }
    d2 += d1;
    if d2 < 0.01 {
        return Ok(false);
    }
    if (last_parameter - first_parameter).abs() / d2 < 0.01 {
        return Ok(false);
    }
    // A guard against absurd sizes (the system is nb_coeff squared).
    if nb_coeff > 1024 {
        return Err(PLibError::OutOfRange);
    }

    let ordre = [first_order + 1, last_order + 1];
    let mut iof = 0;
    let mut t_borne = first_parameter;
    let mut coeff = Vector::new(1, nb_coeff)?;
    let mut b = Vector::filled(1, nb_coeff, 0.0)?;
    let mut mat = Matrix::filled(1, nb_coeff, 1, nb_coeff, 0.0)?;

    // Calcul de la matrice a inverser (MAT)
    for cote in 0..=1 {
        coeff.init(1.0);
        for pp in 1..=rd(&ordre, cote)? {
            let ii = pp + iof;
            let mut prod = 1.0_f64;
            for jj in pp..=nb_coeff {
                //        tout se passe dans les 3 lignes suivantes
                mat.set_value(ii, jj, coeff.value(jj)? * prod)?;
                coeff.set_value(jj, coeff.value(jj)? * f64::from(jj - pp))?;
                prod *= t_borne;
            }
        }
        t_borne = last_parameter;
        iof = rd(&ordre, 0)?;
    }

    // resolution du systemes
    let resol_coeff = Gauss::new(&mat, 1.0e-10)?;
    if !resol_coeff.is_done() {
        return Ok(false);
    }

    for ii in 1..=nb_coeff {
        b.set_value(ii, 1.0)?;
        coeff = resol_coeff.solve(&b)?;
        matrix.set_row(ii, &coeff)?;
        b.set_value(ii, 0.0)?;
    }
    Ok(true)
}

fn pattern_at(pattern: &[[f64; 6]; 3], irow: i32, icol: i32) -> Result<f64, PLibError> {
    pattern.get(count(irow)?).and_then(|r| r.get(count(icol).ok()?)).copied().ok_or(PLibError::OutOfRange)
}

/// `PLib::HermiteInterpolate`: the coefficients (`coef[dimension * p + i - 1]` is the one of `x^p` of component
/// `i`) of the polynomial of degree `first_order + last_order + 1` with the derivatives `0..=first_order` at
/// `first_parameter` (`first_constr(i, j)`: row `i - 1`, column `j`) and `0..=last_order` at `last_parameter`.
/// The orders are in `-1..=2` (`-1`: no constraint at that end, not both). A singular system is
/// `Err(OutOfRange)` (the C++ then solves with a failed decomposition).
pub fn hermite_interpolate(
    dimension: i32,
    first_parameter: f64,
    last_parameter: f64,
    first_order: i32,
    last_order: i32,
    first_constr: &Array2<f64>,
    last_constr: &Array2<f64>,
) -> Result<Vec<f64>, PLibError> {
    const PATTERN: [[f64; 6]; 3] =
        [[1.0, 1.0, 1.0, 1.0, 1.0, 1.0], [0.0, 1.0, 2.0, 3.0, 4.0, 5.0], [0.0, 0.0, 2.0, 6.0, 12.0, 20.0]];
    if !(-1..=2).contains(&first_order) || !(-1..=2).contains(&last_order) || (first_order == -1 && last_order == -1) {
        return Err(PLibError::OutOfRange);
    }
    if dimension < 0 {
        return Err(PLibError::OutOfRange);
    }
    let dim_rows = count(dimension)?;
    if first_order >= 0 && (first_constr.rows() < dim_rows || first_constr.cols() < count(first_order + 1)?) {
        return Err(PLibError::OutOfRange);
    }
    if last_order >= 0 && (last_constr.rows() < dim_rows || last_constr.cols() < count(last_order + 1)?) {
        return Err(PLibError::OutOfRange);
    }
    let m = first_order + last_order + 1;
    let mut a = Matrix::new(0, m, 0, m)?;
    //  The initialisation of the matrix A
    for irow in 0..=first_order {
        let mut first_val = 1.0_f64;
        for icol in 0..=m {
            let pattern = pattern_at(&PATTERN, irow, icol)?;
            a.set_value(irow, icol, pattern * first_val)?;
            if irow <= icol {
                first_val *= first_parameter;
            }
        }
    }
    for irow in 0..=last_order {
        let mut last_val = 1.0_f64;
        for icol in 0..=m {
            let pattern = pattern_at(&PATTERN, irow, icol)?;
            a.set_value(irow + first_order + 1, icol, pattern * last_val)?;
            if irow <= icol {
                last_val *= last_parameter;
            }
        }
    }

    let equations = Gauss::new(&a, 1.0e-20)?;
    let mut coefficients = zeros(mul(dimension, m + 1)?)?;
    for idim in 1..=dimension {
        if !equations.is_done() {
            return Err(PLibError::OutOfRange);
        }
        let mut b = Vector::new(0, m)?;
        for icol in 0..=first_order {
            let v = *first_constr.get(count(idim - 1)?, count(icol)?).ok_or(PLibError::OutOfRange)?;
            b.set_value(icol, v)?;
        }
        for icol in 0..=last_order {
            let v = *last_constr.get(count(idim - 1)?, count(icol)?).ok_or(PLibError::OutOfRange)?;
            b.set_value(first_order + 1 + icol, v)?;
        }
        //  The solving of equations system A * X = B. Then B = X
        let b = equations.solve_in_place(b)?;
        //  the filling of the Coefficients
        for icol in 0..=m {
            wr(&mut coefficients, ix(dimension) * ix(icol) + ix(idim) - 1, b.value(icol)?)?;
        }
    }
    Ok(coefficients)
}

// ---------------------------------------------------------------------------------------------
// Power basis -> Bezier poles

/// `PLib::CoefficientsPoles(dim, ...)`: the `n = coefs.len() / dim` groups of power-basis coefficients of a
/// polynomial of degree `n - 1` become the `n` poles of the equal Bezier curve; with `wcoefs` the same for the
/// weights, and the poles are divided by the weights at the end.
pub fn coefficients_poles_dim(
    dim: i32,
    coefs: &[f64],
    wcoefs: Option<&[f64]>,
) -> Result<(Vec<f64>, Option<Vec<f64>>), PLibError> {
    if dim < 1 || !coefs.len().is_multiple_of(count(dim)?) {
        return Err(PLibError::OutOfRange);
    }
    let reflen = to_i32(coefs.len() / count(dim)?)?;
    if reflen < 1 {
        return Err(PLibError::OutOfRange);
    }
    let rat = wcoefs.is_some();
    if let Some(w) = wcoefs
        && w.len() != count(reflen)?
    {
        return Err(PLibError::OutOfRange);
    }
    let mut poles = coefs.to_vec();
    let mut weights: Vec<f64> = wcoefs.map(<[f64]>::to_vec).unwrap_or_default();
    let d = ix(dim);

    // Les Extremites: the first and the last group are the coefficients themselves (the arrays start as copies).

    for i in 2..reflen {
        let cnp = bin(reflen - 1, i - 1)?;
        if rat {
            wr(&mut weights, ix(i - 1), rd(wcoefs.unwrap_or_default(), ix(i - 1))? / cnp)?;
        }
        for j in 0..d {
            wr(&mut poles, d * ix(i - 1) + j, rd(coefs, d * ix(i - 1) + j)? / cnp)?;
        }
    }

    for i in 1..reflen {
        for j in (i..reflen).rev() {
            if rat {
                let v = rd(&weights, ix(j - 1))?;
                add_to(&mut weights, ix(j), v)?;
            }
            for k in 0..d {
                let v = rd(&poles, d * ix(j - 1) + k)?;
                add_to(&mut poles, d * ix(j) + k, v)?;
            }
        }
    }
    if rat {
        for i in 1..=reflen {
            for j in 0..d {
                div_by(&mut poles, d * ix(i - 1) + j, rd(&weights, ix(i - 1))?)?;
            }
        }
    }
    Ok((poles, rat.then_some(weights)))
}

/// `PLib::CoefficientsPoles(Array1<gp_Pnt>, ...)`.
pub fn coefficients_poles_3d(coefs: &[Pnt], wcoefs: Option<&[f64]>) -> Result<(Vec<Pnt>, Option<Vec<f64>>), PLibError> {
    let temp_c = set_poles_3d(coefs)?;
    let (temp_p, weights) = coefficients_poles_dim(3, &temp_c, wcoefs)?;
    Ok((flat_to_pnts(&temp_p), weights))
}

/// `PLib::CoefficientsPoles(Array1<gp_Pnt2d>, ...)`.
pub fn coefficients_poles_2d(
    coefs: &[Pnt2d],
    wcoefs: Option<&[f64]>,
) -> Result<(Vec<Pnt2d>, Option<Vec<f64>>), PLibError> {
    let temp_c = set_poles_2d(coefs)?;
    let (temp_p, weights) = coefficients_poles_dim(2, &temp_c, wcoefs)?;
    Ok((flat_to_pnts2d(&temp_p), weights))
}

/// `PLib::CoefficientsPoles(Array1<double>, ...)`.
pub fn coefficients_poles_1d(coefs: &[f64], wcoefs: Option<&[f64]>) -> Result<(Vec<f64>, Option<Vec<f64>>), PLibError> {
    coefficients_poles_dim(1, coefs, wcoefs)
}

/// `PLib::CoefficientsPoles(Array2<gp_Pnt>, ...)`: the same along each row (the second variable), then along each
/// column (the first variable).
pub fn coefficients_poles_surface(
    coefs: &Array2<Pnt>,
    wcoefs: Option<&Array2<f64>>,
) -> Result<(Array2<Pnt>, Option<Array2<f64>>), PLibError> {
    let rows = to_i32(coefs.rows())?;
    let cols = to_i32(coefs.cols())?;
    if rows < 1 || cols < 1 {
        return Err(PLibError::OutOfRange);
    }
    let rat = wcoefs.is_some();
    if let Some(w) = wcoefs
        && (w.rows() != coefs.rows() || w.cols() != coefs.cols())
    {
        return Err(PLibError::OutOfRange);
    }
    let at = |r: i32, c: i32| ix(r) * ix(cols) + ix(c);
    let mut poles: Vec<Pnt> = coefs.as_slice().to_vec();
    let wsrc: &[f64] = wcoefs.map(Array2::as_slice).unwrap_or_default();
    let mut weights: Vec<f64> = wsrc.to_vec();
    let csrc = coefs.as_slice();

    for npoleu in 0..rows {
        wr(&mut poles, at(npoleu, 0), rd(csrc, at(npoleu, 0))?)?;
        if rat {
            wr(&mut weights, at(npoleu, 0), rd(wsrc, at(npoleu, 0))?)?;
        }
        for col in 1..(cols - 1) {
            let cnp = bin(cols - 1, col)?;
            let mut temp: XYZ = *rd(csrc, at(npoleu, col))?.xyz();
            temp.divide(cnp);
            let mut p = rd(&poles, at(npoleu, col))?;
            p.set_xyz(temp);
            wr(&mut poles, at(npoleu, col), p)?;
            if rat {
                wr(&mut weights, at(npoleu, col), rd(wsrc, at(npoleu, col))? / cnp)?;
            }
        }
        wr(&mut poles, at(npoleu, cols - 1), rd(csrc, at(npoleu, cols - 1))?)?;
        if rat {
            wr(&mut weights, at(npoleu, cols - 1), rd(wsrc, at(npoleu, cols - 1))?)?;
        }
        for i1 in 1..cols {
            for i2 in (i1..cols).rev() {
                let mut temp = XYZ::zero();
                temp.set_linear_form_sum(rd(&poles, at(npoleu, i2))?.xyz(), rd(&poles, at(npoleu, i2 - 1))?.xyz());
                let mut p = rd(&poles, at(npoleu, i2))?;
                p.set_xyz(temp);
                wr(&mut poles, at(npoleu, i2), p)?;
                if rat {
                    let v = rd(&weights, at(npoleu, i2 - 1))?;
                    add_to(&mut weights, at(npoleu, i2), v)?;
                }
            }
        }
    }

    for npolev in 0..cols {
        for row in 1..(rows - 1) {
            let cnp = bin(rows - 1, row)?;
            let mut temp: XYZ = *rd(&poles, at(row, npolev))?.xyz();
            temp.divide(cnp);
            let mut p = rd(&poles, at(row, npolev))?;
            p.set_xyz(temp);
            wr(&mut poles, at(row, npolev), p)?;
            if rat {
                div_by(&mut weights, at(row, npolev), cnp)?;
            }
        }
        for i1 in 1..rows {
            for i2 in (i1..rows).rev() {
                let mut temp = XYZ::zero();
                temp.set_linear_form_sum(rd(&poles, at(i2, npolev))?.xyz(), rd(&poles, at(i2 - 1, npolev))?.xyz());
                let mut p = rd(&poles, at(i2, npolev))?;
                p.set_xyz(temp);
                wr(&mut poles, at(i2, npolev), p)?;
                if rat {
                    let v = rd(&weights, at(i2 - 1, npolev))?;
                    add_to(&mut weights, at(i2, npolev), v)?;
                }
            }
        }
    }
    if rat {
        for row in 0..rows {
            for col in 0..cols {
                let w = rd(&weights, at(row, col))?;
                let mut temp: XYZ = *rd(&poles, at(row, col))?.xyz();
                temp.divide(w);
                let mut p = rd(&poles, at(row, col))?;
                p.set_xyz(temp);
                wr(&mut poles, at(row, col), p)?;
            }
        }
    }
    let new_weights = if rat { Some(Array2::new(coefs.rows(), coefs.cols(), weights)?) } else { None };
    Ok((Array2::new(coefs.rows(), coefs.cols(), poles)?, new_weights))
}

// ---------------------------------------------------------------------------------------------
// Trimming

/// `PLib::Trimming(U1, U2, dim, Coefs, WCoefs)`: the coefficients (groups of `dim`, power 0 first, weights
/// optional) of `P(U1 + (U2 - U1) v)` for the polynomial `P(u)` given, by Horner synthetic division.
pub fn trimming_dim(
    u1: f64,
    u2: f64,
    dim: i32,
    coefs: &[f64],
    wcoefs: Option<&[f64]>,
) -> Result<(Vec<f64>, Option<Vec<f64>>), PLibError> {
    if dim < 1 || !coefs.len().is_multiple_of(count(dim)?) {
        return Err(PLibError::OutOfRange);
    }
    // principe :
    // on fait le changement de variable v = (u-U1) / (U2-U1)
    // on exprime u = f(v) que l'on remplace dans l'expression polynomiale
    // decomposee sous la forme du schema iteratif de horner.
    let lsp = u2 - u1;
    let d = ix(dim);
    let mut c = coefs.to_vec();
    let mut w: Vec<f64> = wcoefs.map(<[f64]>::to_vec).unwrap_or_default();
    let upc = len_ix(coefs) - d;
    let mut len = len_ix(coefs) / d;
    let rat = wcoefs.is_some();
    let mut upw: isize = 0;
    let mut indw: isize = 0;

    if rat {
        if len != len_ix(&w) {
            return Err(PLibError::Failure);
        }
        upw = len_ix(&w) - 1;
    }
    len -= 1;

    for i in 1..=len {
        let mut indc = upc - d * (i - 1);
        if rat {
            indw = upw - i + 1;
        }
        // calcul du coefficient de degre le plus faible a l'iteration i
        for j in 0..d {
            let v = u1 * rd(&c, indc + j)?;
            add_to(&mut c, indc - d + j, v)?;
        }
        if rat {
            let v = u1 * rd(&w, indw)?;
            add_to(&mut w, indw - 1, v)?;
        }

        // calcul des coefficients intermediaires :
        while indc < upc {
            indc += d;
            for k in 0..d {
                let v = u1 * rd(&c, indc + k)? + lsp * rd(&c, indc - d + k)?;
                wr(&mut c, indc - d + k, v)?;
            }
            if rat {
                indw += 1;
                let v = u1 * rd(&w, indw)? + lsp * rd(&w, indw - 1)?;
                wr(&mut w, indw - 1, v)?;
            }
        }

        // calcul du coefficient de degre le plus eleve :
        for j in 0..d {
            mul_by(&mut c, upc + j, lsp)?;
        }
        if rat {
            mul_by(&mut w, upw, lsp)?;
        }
    }
    Ok((c, rat.then_some(w)))
}

/// `PLib::Trimming` for `gp_Pnt` coefficients.
pub fn trimming_3d(
    u1: f64,
    u2: f64,
    coefs: &[Pnt],
    wcoefs: Option<&[f64]>,
) -> Result<(Vec<Pnt>, Option<Vec<f64>>), PLibError> {
    let temp = set_poles_3d(coefs)?;
    let (c, w) = trimming_dim(u1, u2, 3, &temp, wcoefs)?;
    Ok((flat_to_pnts(&c), w))
}

/// `PLib::Trimming` for `gp_Pnt2d` coefficients.
pub fn trimming_2d(
    u1: f64,
    u2: f64,
    coefs: &[Pnt2d],
    wcoefs: Option<&[f64]>,
) -> Result<(Vec<Pnt2d>, Option<Vec<f64>>), PLibError> {
    let temp = set_poles_2d(coefs)?;
    let (c, w) = trimming_dim(u1, u2, 2, &temp, wcoefs)?;
    Ok((flat_to_pnts2d(&c), w))
}

/// `PLib::Trimming` for scalar coefficients.
pub fn trimming_1d(
    u1: f64,
    u2: f64,
    coefs: &[f64],
    wcoefs: Option<&[f64]>,
) -> Result<(Vec<f64>, Option<Vec<f64>>), PLibError> {
    trimming_dim(u1, u2, 1, coefs, wcoefs)
}

/// The shape check of the weights of `PLib::UTrimming` / `VTrimming` (OCCT would read outside the weight array).
fn check_same_shape(coefs: &Array2<Pnt>, wcoefs: Option<&Array2<f64>>) -> Result<(), PLibError> {
    match wcoefs {
        Some(w) if w.rows() != coefs.rows() || w.cols() != coefs.cols() => Err(PLibError::OutOfRange),
        _ => Ok(()),
    }
}

/// `PLib::UTrimming`: trims every column (the polynomial in the first index, the rows).
pub fn u_trimming(
    u1: f64,
    u2: f64,
    coefs: &Array2<Pnt>,
    wcoefs: Option<&Array2<f64>>,
) -> Result<(Array2<Pnt>, Option<Array2<f64>>), PLibError> {
    check_same_shape(coefs, wcoefs)?;
    let (rows, cols) = (coefs.rows(), coefs.cols());
    let mut out: Vec<Pnt> = coefs.as_slice().to_vec();
    let mut out_w: Vec<f64> = wcoefs.map(|w| w.as_slice().to_vec()).unwrap_or_default();
    for icol in 0..cols {
        let mut temp = Vec::with_capacity(rows);
        let mut temw = Vec::with_capacity(rows);
        for irow in 0..rows {
            temp.push(*coefs.get(irow, icol).ok_or(PLibError::OutOfRange)?);
            if let Some(w) = wcoefs {
                temw.push(*w.get(irow, icol).ok_or(PLibError::OutOfRange)?);
            }
        }
        let (t, tw) = trimming_3d(u1, u2, &temp, wcoefs.map(|_| temw.as_slice()))?;
        for (irow, p) in t.into_iter().enumerate() {
            wr(&mut out, ix_len(irow * cols + icol), p)?;
        }
        if let Some(tw) = tw {
            for (irow, v) in tw.into_iter().enumerate() {
                wr(&mut out_w, ix_len(irow * cols + icol), v)?;
            }
        }
    }
    let new_w = if wcoefs.is_some() { Some(Array2::new(rows, cols, out_w)?) } else { None };
    Ok((Array2::new(rows, cols, out)?, new_w))
}

/// `PLib::VTrimming`: trims every row (the polynomial in the second index, the columns).
pub fn v_trimming(
    v1: f64,
    v2: f64,
    coefs: &Array2<Pnt>,
    wcoefs: Option<&Array2<f64>>,
) -> Result<(Array2<Pnt>, Option<Array2<f64>>), PLibError> {
    check_same_shape(coefs, wcoefs)?;
    let (rows, cols) = (coefs.rows(), coefs.cols());
    let mut out: Vec<Pnt> = Vec::with_capacity(rows * cols);
    let mut out_w: Vec<f64> = Vec::new();
    for irow in 0..rows {
        let mut temp = Vec::with_capacity(cols);
        let mut temw = Vec::with_capacity(cols);
        for icol in 0..cols {
            temp.push(*coefs.get(irow, icol).ok_or(PLibError::OutOfRange)?);
            if let Some(w) = wcoefs {
                temw.push(*w.get(irow, icol).ok_or(PLibError::OutOfRange)?);
            }
        }
        let (t, tw) = trimming_3d(v1, v2, &temp, wcoefs.map(|_| temw.as_slice()))?;
        out.extend(t);
        if let Some(tw) = tw {
            out_w.extend(tw);
        }
    }
    let new_w = if wcoefs.is_some() { Some(Array2::new(rows, cols, out_w)?) } else { None };
    Ok((Array2::new(rows, cols, out)?, new_w))
}

// ---------------------------------------------------------------------------------------------
// Constraint orders, Jacobi parameters, length

/// `PLib::JacobiParameters`: `(nb_gauss_points, work_degree)` for a constraint order, a maximal degree and a
/// discretisation code (`-5..=-1`, `1..=4`).
pub fn jacobi_parameters(constraint_order: GeomAbsShape, max_degree: i32, code: i32) -> Result<(i32, i32), PLibError> {
    const NDEG8: i32 = 8;
    const NDEG10: i32 = 10;
    const NDEG15: i32 = 15;
    const NDEG20: i32 = 20;
    const NDEG25: i32 = 25;
    const NDEG30: i32 = 30;
    const NDEG40: i32 = 40;
    const NDEG50: i32 = 50;
    const NDEG61: i32 = 61;

    let niv_constr = match constraint_order {
        GeomAbsShape::C0 => 0,
        GeomAbsShape::C1 => 1,
        GeomAbsShape::C2 => 2,
        _ => return Err(PLibError::Construction),
    };
    if max_degree < 2 * niv_constr + 1 {
        return Err(PLibError::Construction);
    }

    let work_degree =
        if code >= 1 { max_degree.checked_add(9) } else { max_degree.checked_add(6) }.ok_or(PLibError::Construction)?;

    //---> Nbre mini de points necessaires.
    let ipmin = if work_degree < NDEG8 {
        NDEG8
    } else if work_degree < NDEG10 {
        NDEG10
    } else if work_degree < NDEG15 {
        NDEG15
    } else if work_degree < NDEG20 {
        NDEG20
    } else if work_degree < NDEG25 {
        NDEG25
    } else if work_degree < NDEG30 {
        NDEG30
    } else if work_degree < NDEG40 {
        NDEG40
    } else if work_degree < NDEG50 {
        NDEG50
    } else if work_degree < NDEG61 {
        NDEG61
    } else {
        return Err(PLibError::Construction);
    };
    // ---> Nbre de points voulus.
    let iwant = match code {
        -5 => NDEG8,
        -4 => NDEG10,
        -3 => NDEG15,
        -2 => NDEG20,
        -1 => NDEG25,
        1 => NDEG30,
        2 => NDEG40,
        3 => NDEG50,
        4 => NDEG61,
        _ => return Err(PLibError::Construction),
    };
    Ok((ipmin.max(iwant), work_degree))
}

/// `PLib::NivConstr`: `C0 -> 0`, `C1 -> 1`, `C2 -> 2`.
pub fn niv_constr(constraint_order: GeomAbsShape) -> Result<i32, PLibError> {
    match constraint_order {
        GeomAbsShape::C0 => Ok(0),
        GeomAbsShape::C1 => Ok(1),
        GeomAbsShape::C2 => Ok(2),
        _ => Err(PLibError::Construction),
    }
}

/// `PLib::ConstraintOrder`: `0 -> C0`, `1 -> C1`, `2 -> C2`.
pub fn constraint_order(niv_constr: i32) -> Result<GeomAbsShape, PLibError> {
    match niv_constr {
        0 => Ok(GeomAbsShape::C0),
        1 => Ok(GeomAbsShape::C1),
        2 => Ok(GeomAbsShape::C2),
        _ => Err(PLibError::Construction),
    }
}

/// The checks shared by the two `PLib::EvalLength`.
fn check_length_args(degree: i32, dimension: i32, coeffs: &[f64]) -> Result<(), PLibError> {
    if degree < 0 || dimension < 1 {
        return Err(PLibError::OutOfRange);
    }
    need(coeffs.len(), mul(degree + 1, dimension)?)
}

/// `PLib::EvalLength` without tolerance (`eval_length_unchecked` after the argument checks).
fn eval_length_unchecked(
    degree: i32,
    dimension: i32,
    coeffs: &[f64],
    u1: f64,
    u2: f64,
    gauss_points: &Vector,
    gauss_weights: &Vector,
) -> Result<f64, PLibError> {
    let nb_gauss_points = 4 * ((degree / 4) + 1).min(10);
    let c1 = (u2 + u1) / 2.;
    let c2 = (u2 - u1) / 2.;

    //-----------------------------------------------------------
    //****** Integration - Boucle sur les intervalles de GAUSS **
    //-----------------------------------------------------------
    let mut sum = 0.0_f64;
    for j in 1..=(nb_gauss_points / 2) {
        // Integration en tenant compte de la symetrie
        let tran = c2 * gauss_points.value(j)?;
        let x1 = c1 + tran;
        let x2 = c1 - tran;

        //****** Derivation sur la dimension de l'espace **
        let degdim = ix(degree) * ix(dimension);
        let mut der1 = 0.0_f64;
        let mut der2 = 0.0_f64;
        for idim in 0..ix(dimension) {
            let mut d1 = f64::from(degree) * rd(coeffs, idim + degdim)?;
            let mut d2 = d1;
            for i in (1..degree).rev() {
                let dd = f64::from(i) * rd(coeffs, idim + ix(i) * ix(dimension))?;
                d1 = d1 * x1 + dd;
                d2 = d2 * x2 + dd;
            }
            der1 += d1 * d1;
            der2 += d2 * d2;
        }

        //****** Integration **
        sum += gauss_weights.value(j)? * c2 * (der1.sqrt() + der2.sqrt());
    }
    Ok(sum)
}

fn length_tables(degree: i32) -> Result<(Vector, Vector), PLibError> {
    let nb_gauss_points = 4 * ((degree / 4) + 1).min(10);
    Ok((mathlin::gauss_points(nb_gauss_points, 1)?, mathlin::gauss_weights(nb_gauss_points, 1)?))
}

/// `PLib::EvalLength` (without tolerance): the length `integral |P'(u)| du` over `[u1, u2]` of the polynomial
/// curve of `dimension` components, by Gauss integration.
pub fn eval_length(degree: i32, dimension: i32, coeffs: &[f64], u1: f64, u2: f64) -> Result<f64, PLibError> {
    check_length_args(degree, dimension, coeffs)?;
    let (points, weights) = length_tables(degree)?;
    eval_length_unchecked(degree, dimension, coeffs, u1, u2, &points, &weights)
}

/// `PLib::EvalLength` with `Tol`: `(length, error)`; the sub-intervals are halved until the change is `<= tol` or
/// 13 iterations were done.
pub fn eval_length_tol(
    degree: i32,
    dimension: i32,
    coeffs: &[f64],
    u1: f64,
    u2: f64,
    tol: f64,
) -> Result<(f64, f64), PLibError> {
    check_length_args(degree, dimension, coeffs)?;
    let (points, weights) = length_tables(degree)?;
    let mut nb_sub_int: i32 = 1; // Current number of subintervals
    let max_nb_iter = 13; // Max number of iterations
    let mut nb_iter = 1; // Current number of iterations

    let mut length = eval_length_unchecked(degree, dimension, coeffs, u1, u2, &points, &weights)?;
    let mut error;
    loop {
        let old_len = length;
        length = 0.;
        nb_sub_int *= 2;
        let du = (u2 - u1) / f64::from(nb_sub_int);
        for i in 1..=nb_sub_int {
            let len_i = eval_length_unchecked(
                degree,
                dimension,
                coeffs,
                u1 + f64::from(i - 1) * du,
                u1 + f64::from(i) * du,
                &points,
                &weights,
            )?;
            length += len_i;
        }
        nb_iter += 1;
        error = (old_len - length).abs();
        if !(error > tol && nb_iter <= max_nb_iter) {
            break;
        }
    }
    Ok((length, error))
}
