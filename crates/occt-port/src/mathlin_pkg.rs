//! OCCT's header-only `MathLin` package: LU (Gauss), Crout, SVD, Householder QR, Jacobi,
//! least squares and the QL eigenvalue search for tridiagonal matrices.
//!
//! Translated from `MathLin_*.hxx` (OCCT 8.0.1). C++ exceptions (range errors) become `Err(MathError)`;
//! the indices are OCCT's absolute indices, read through the checked accessors of `mathlin::Matrix`.
// The comparisons `x == y` and `x + y == x` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::mathlin::recipes;
use crate::mathlin::{IntegerVector, MathError, Matrix, Vector};

pub use crate::mathinteg::Status;

type R<T> = Result<T, MathError>;

/// `MathUtils::THE_ZERO_TOL`.
const ZERO_TOL: f64 = 1.0e-15;

fn is_zero(x: f64) -> bool {
    x.abs() < ZERO_TOL
}

/// `std::max(a, b)`.
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `std::min(a, b)`.
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `MathLin::LUResult`.
#[derive(Debug, Clone)]
pub struct LuResult {
    pub status: Status,
    pub lu: Option<Matrix>,
    pub pivot: Option<IntegerVector>,
    pub determinant: Option<f64>,
    pub sign: i32,
}
impl Default for LuResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, lu: None, pivot: None, determinant: None, sign: 1 }
    }
}

/// `MathUtils::LinearResult`.
#[derive(Debug, Clone)]
pub struct LinearResult {
    pub status: Status,
    pub solution: Option<Vector>,
    pub determinant: Option<f64>,
}
impl Default for LinearResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, solution: None, determinant: None }
    }
}

/// `MathUtils::LinearMultipleResult`.
#[derive(Debug, Clone)]
pub struct LinearMultipleResult {
    pub status: Status,
    pub solutions: Option<Matrix>,
    pub determinant: Option<f64>,
}
impl Default for LinearMultipleResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, solutions: None, determinant: None }
    }
}

/// `MathUtils::InverseResult`.
#[derive(Debug, Clone)]
pub struct InverseResult {
    pub status: Status,
    pub inverse: Option<Matrix>,
    pub determinant: Option<f64>,
}
impl Default for InverseResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, inverse: None, determinant: None }
    }
}

/// `MathLin::CroutResult`.
#[derive(Debug, Clone)]
pub struct CroutResult {
    pub status: Status,
    pub l: Option<Matrix>,
    pub d: Option<Vector>,
    pub inverse: Option<Matrix>,
    pub determinant: Option<f64>,
}
impl Default for CroutResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, l: None, d: None, inverse: None, determinant: None }
    }
}

/// `MathLin::SVDResult`.
#[derive(Debug, Clone)]
pub struct SvdResult {
    pub status: Status,
    pub u: Option<Matrix>,
    pub singular_values: Option<Vector>,
    pub v: Option<Matrix>,
    pub rank: i32,
}
impl Default for SvdResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, u: None, singular_values: None, v: None, rank: 0 }
    }
}

/// `MathLin::QRResult`.
#[derive(Debug, Clone)]
pub struct QrResult {
    pub status: Status,
    pub q: Option<Matrix>,
    pub r: Option<Matrix>,
    pub rank: i32,
}
impl Default for QrResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, q: None, r: None, rank: 0 }
    }
}

/// `MathUtils::EigenResult` (the result of the Jacobi solvers).
#[derive(Debug, Clone)]
pub struct EigenResult {
    pub status: Status,
    pub nb_iterations: usize,
    pub eigen_values: Option<Vector>,
    pub eigen_vectors: Option<Matrix>,
}
impl Default for EigenResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, nb_iterations: 0, eigen_values: None, eigen_vectors: None }
    }
}

/// `MathLin::LeastSquaresMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeastSquaresMethod {
    NormalEquations = 0,
    Qr = 1,
    Svd = 2,
}

/// `MathLin::LeastSquaresResult`.
#[derive(Debug, Clone)]
pub struct LeastSquaresResult {
    pub status: Status,
    pub solution: Option<Vector>,
    pub residual: Option<f64>,
    pub residual_sq: Option<f64>,
    pub rank: i32,
}
impl Default for LeastSquaresResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, solution: None, residual: None, residual_sq: None, rank: 0 }
    }
}

/// `MathLin::EigenResult` of `MathLin_EigenSearch.hxx`.
#[derive(Debug, Clone)]
pub struct TridiagonalEigenResult {
    pub status: Status,
    pub eigen_values: Option<Vector>,
    pub eigen_vectors: Option<Matrix>,
    pub dimension: i32,
}
impl Default for TridiagonalEigenResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, eigen_values: None, eigen_vectors: None, dimension: 0 }
    }
}

// ---------------------------------------------------------------------------------------------
// MathLin_Gauss.hxx
// ---------------------------------------------------------------------------------------------

/// The elimination of `MathLin::LU` on the working copy; returns the status.
fn lu_core(
    lu: &mut Matrix,
    pivot: &mut IntegerVector,
    sign: &mut i32,
    determinant: &mut Option<f64>,
    min_pivot: f64,
) -> R<Status> {
    let (rl, ru, cl, cu) = (lu.lower_row(), lu.upper_row(), lu.lower_col(), lu.upper_col());
    let mut scale = Vector::new(rl, ru)?;
    for i in rl..=ru {
        let mut mx = 0.0;
        for j in cl..=cu {
            let ab = lu.value(i, j)?.abs();
            if ab > mx {
                mx = ab;
            }
        }
        if mx < min_pivot {
            return Ok(Status::NumericalError);
        }
        scale.set_value(i, 1.0 / mx)?;
    }
    for k in rl..=ru {
        let mut max_scaled = 0.0;
        let mut pivot_row = k;
        for i in k..=ru {
            let scaled = scale.value(i)? * lu.value(i, k)?.abs();
            if scaled > max_scaled {
                max_scaled = scaled;
                pivot_row = i;
            }
        }
        if pivot_row != k {
            for j in cl..=cu {
                let a = lu.value(pivot_row, j)?;
                let b = lu.value(k, j)?;
                lu.set_value(pivot_row, j, b)?;
                lu.set_value(k, j, a)?;
            }
            *sign = -*sign;
            let a = scale.value(pivot_row)?;
            let b = scale.value(k)?;
            scale.set_value(pivot_row, b)?;
            scale.set_value(k, a)?;
        }
        pivot.set_value(k, pivot_row)?;
        if lu.value(k, k)?.abs() < min_pivot {
            return Ok(Status::NumericalError);
        }
        for i in k + 1..=ru {
            let q = lu.value(i, k)? / lu.value(k, k)?;
            lu.set_value(i, k, q)?;
            let factor = lu.value(i, k)?;
            for j in k + 1..=cu {
                let v = lu.value(i, j)? - factor * lu.value(k, j)?;
                lu.set_value(i, j, v)?;
            }
        }
    }
    let mut det = f64::from(*sign);
    for i in rl..=ru {
        det *= lu.value(i, i)?;
    }
    *determinant = Some(det);
    Ok(Status::Ok)
}

/// `MathLin::LU`.
pub fn lu(a: &Matrix, min_pivot: f64) -> R<LuResult> {
    let mut res = LuResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    if ru - rl + 1 != cu - cl + 1 {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let mut work = a.clone();
    let mut pivot = IntegerVector::new(rl, ru)?;
    let mut sign = 1;
    let mut det = None;
    let status = lu_core(&mut work, &mut pivot, &mut sign, &mut det, min_pivot);
    res.lu = Some(work);
    res.pivot = Some(pivot);
    res.sign = sign;
    res.determinant = det;
    res.status = status?;
    Ok(res)
}

/// The substitution phase shared by `Solve`, `SolveMultiple` and `Invert` (on a vector with the row bounds).
fn lu_substitute(lu: &Matrix, pivot: &IntegerVector, x: &mut Vector, rl: i32, ru: i32) -> R<()> {
    let mut first_non_zero = 0;
    for i in rl..=ru {
        let pivot_idx = pivot.value(i)?;
        let mut sum = x.value(pivot_idx)?;
        let xi = x.value(i)?;
        x.set_value(pivot_idx, xi)?;
        if first_non_zero != 0 {
            for j in first_non_zero..i {
                sum -= lu.value(i, j)? * x.value(j)?;
            }
        } else if !is_zero(sum) {
            first_non_zero = i;
        }
        x.set_value(i, sum)?;
    }
    for i in (rl..=ru).rev() {
        let mut sum = x.value(i)?;
        for j in i + 1..=ru {
            sum -= lu.value(i, j)? * x.value(j)?;
        }
        x.set_value(i, sum / lu.value(i, i)?)?;
    }
    Ok(())
}

/// `MathLin::Solve`.
pub fn solve(a: &Matrix, b: &Vector, min_pivot: f64) -> R<LinearResult> {
    let mut res = LinearResult::default();
    let lu_res = lu(a, min_pivot)?;
    if lu_res.status != Status::Ok {
        res.status = lu_res.status;
        return Ok(res);
    }
    let (rl, ru) = (a.lower_row(), a.upper_row());
    if b.lower() != rl || b.upper() != ru {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let (Some(lum), Some(pivot)) = (&lu_res.lu, &lu_res.pivot) else {
        return Ok(res);
    };
    let mut x = b.clone();
    lu_substitute(lum, pivot, &mut x, rl, ru)?;
    res.status = Status::Ok;
    res.solution = Some(x);
    res.determinant = lu_res.determinant;
    Ok(res)
}

/// `MathLin::SolveMultiple`.
pub fn solve_multiple(a: &Matrix, b: &Matrix, min_pivot: f64) -> R<LinearMultipleResult> {
    let mut res = LinearMultipleResult::default();
    let lu_res = lu(a, min_pivot)?;
    if lu_res.status != Status::Ok {
        res.status = lu_res.status;
        return Ok(res);
    }
    let (rl, ru) = (a.lower_row(), a.upper_row());
    if b.lower_row() != rl || b.upper_row() != ru {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let (Some(lum), Some(pivot)) = (&lu_res.lu, &lu_res.pivot) else {
        return Ok(res);
    };
    let mut x = Matrix::new(rl, ru, b.lower_col(), b.upper_col())?;
    for col in b.lower_col()..=b.upper_col() {
        let mut work = Vector::new(rl, ru)?;
        for i in rl..=ru {
            work.set_value(i, b.value(i, col)?)?;
        }
        lu_substitute(lum, pivot, &mut work, rl, ru)?;
        for i in rl..=ru {
            x.set_value(i, col, work.value(i)?)?;
        }
    }
    res.status = Status::Ok;
    res.determinant = lu_res.determinant;
    res.solutions = Some(x);
    Ok(res)
}

/// `MathLin::Determinant`.
pub fn determinant(a: &Matrix, min_pivot: f64) -> R<LinearResult> {
    let mut res = LinearResult::default();
    let lu_res = lu(a, min_pivot)?;
    if lu_res.status != Status::Ok {
        res.status = lu_res.status;
        res.determinant = Some(0.0);
        return Ok(res);
    }
    res.status = Status::Ok;
    res.determinant = lu_res.determinant;
    Ok(res)
}

/// `MathLin::Invert`.
pub fn invert(a: &Matrix, min_pivot: f64) -> R<InverseResult> {
    let mut res = InverseResult::default();
    let lu_res = lu(a, min_pivot)?;
    if lu_res.status != Status::Ok {
        res.status = lu_res.status;
        return Ok(res);
    }
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let (Some(lum), Some(pivot)) = (&lu_res.lu, &lu_res.pivot) else {
        return Ok(res);
    };
    let mut inv = Matrix::filled(rl, ru, cl, cu, 0.0)?;
    let mut col_vec = Vector::new(rl, ru)?;
    for col in cl..=cu {
        for i in rl..=ru {
            col_vec.set_value(i, 0.0)?;
        }
        col_vec.set_value(col - cl + rl, 1.0)?;
        lu_substitute(lum, pivot, &mut col_vec, rl, ru)?;
        for i in rl..=ru {
            inv.set_value(i, col, col_vec.value(i)?)?;
        }
    }
    res.status = Status::Ok;
    res.inverse = Some(inv);
    res.determinant = lu_res.determinant;
    Ok(res)
}

// ---------------------------------------------------------------------------------------------
// MathLin_Crout.hxx
// ---------------------------------------------------------------------------------------------

/// `MathLin::Crout`.
pub fn crout(a: &Matrix, min_pivot: f64) -> R<CroutResult> {
    let mut res = CroutResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let n = ru - rl + 1;
    if n != cu - cl + 1 {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let mut l = Matrix::filled(1, n, 1, n, 0.0)?;
    let mut diag = Vector::new(1, n)?;
    let mut det = 1.0;
    for i in 1..=n {
        for j in 1..i {
            let mut scale = 0.0;
            for k in 1..j {
                scale += l.value(i, k)? * l.value(j, k)? * diag.value(k)?;
            }
            let v = (a.value(i + rl - 1, j + cl - 1)? - scale) / diag.value(j)?;
            l.set_value(i, j, v)?;
        }
        let mut scale = 0.0;
        for k in 1..i {
            scale += l.value(i, k)? * l.value(i, k)? * diag.value(k)?;
        }
        diag.set_value(i, a.value(i + rl - 1, i + cl - 1)? - scale)?;
        det *= diag.value(i)?;
        if diag.value(i)?.abs() <= min_pivot {
            res.status = Status::Singular;
            return Ok(res);
        }
        l.set_value(i, i, 1.0)?;
    }
    l.set_value(1, 1, 1.0 / l.value(1, 1)?)?;
    for i in 2..=n {
        for k in 1..i {
            let mut scale = 0.0;
            for j in k..i {
                scale += l.value(i, j)? * l.value(j, k)?;
            }
            l.set_value(i, k, -scale / l.value(i, i)?)?;
        }
        l.set_value(i, i, 1.0 / l.value(i, i)?)?;
    }
    let mut inv = Matrix::filled(1, n, 1, n, 0.0)?;
    for j in 1..=n {
        let mut scale = l.value(j, j)? * l.value(j, j)? / diag.value(j)?;
        for k in j + 1..=n {
            scale += l.value(k, j)? * l.value(k, j)? / diag.value(k)?;
        }
        inv.set_value(j, j, scale)?;
        for i in j + 1..=n {
            let mut s = l.value(i, j)? * l.value(i, i)? / diag.value(i)?;
            for k in i + 1..=n {
                s += l.value(k, j)? * l.value(k, i)? / diag.value(k)?;
            }
            inv.set_value(i, j, s)?;
        }
    }
    let mut out_l = Matrix::filled(rl, ru, cl, cu, 0.0)?;
    for i in 1..=n {
        for j in 1..=i {
            out_l.set_value(i + rl - 1, j + cl - 1, l.value(i, j)?)?;
        }
    }
    let mut out_d = Vector::new(rl, ru)?;
    for i in 1..=n {
        out_d.set_value(i + rl - 1, diag.value(i)?)?;
    }
    let mut out_inv = Matrix::filled(rl, ru, cl, cu, 0.0)?;
    for i in 1..=n {
        for j in 1..=i {
            out_inv.set_value(i + rl - 1, j + cl - 1, inv.value(i, j)?)?;
        }
        for j in i + 1..=n {
            out_inv.set_value(i + rl - 1, j + cl - 1, inv.value(j, i)?)?;
        }
    }
    res.l = Some(out_l);
    res.d = Some(out_d);
    res.inverse = Some(out_inv);
    res.determinant = Some(det);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::SolveCrout`.
pub fn solve_crout(a: &Matrix, b: &Vector, min_pivot: f64) -> R<LinearResult> {
    let mut res = LinearResult::default();
    let cr = crout(a, min_pivot)?;
    if cr.status != Status::Ok {
        res.status = cr.status;
        return Ok(res);
    }
    let (rl, ru) = (a.lower_row(), a.upper_row());
    let n = ru - rl + 1;
    if b.length() != n {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let Some(inv) = &cr.inverse else {
        return Ok(res);
    };
    let bl = b.lower();
    let mut sol = Vector::filled(rl, ru, 0.0)?;
    for i in 1..=n {
        let mut sum = inv.value(i + rl - 1, rl)? * b.value(bl)?;
        for j in 2..=i {
            sum += inv.value(i + rl - 1, j + rl - 1)? * b.value(j + bl - 1)?;
        }
        for j in i + 1..=n {
            sum += inv.value(j + rl - 1, i + rl - 1)? * b.value(j + bl - 1)?;
        }
        sol.set_value(i + rl - 1, sum)?;
    }
    res.solution = Some(sol);
    res.determinant = cr.determinant;
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::InvertCrout`.
pub fn invert_crout(a: &Matrix, min_pivot: f64) -> R<InverseResult> {
    let mut res = InverseResult::default();
    let cr = crout(a, min_pivot)?;
    if cr.status != Status::Ok {
        res.status = cr.status;
        return Ok(res);
    }
    res.status = Status::Ok;
    res.inverse = cr.inverse;
    res.determinant = cr.determinant;
    Ok(res)
}

// ---------------------------------------------------------------------------------------------
// MathLin_SVD.hxx
// ---------------------------------------------------------------------------------------------

/// `MathLin::SVD`.
pub fn svd(a: &Matrix, tolerance: f64) -> R<SvdResult> {
    let mut res = SvdResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let m = ru - rl + 1;
    let n = cu - cl + 1;
    let u_rows = m.max(n);
    let mut u = Matrix::filled(1, u_rows, 1, n, 0.0)?;
    for i in rl..=ru {
        for j in cl..=cu {
            u.set_value(i - rl + 1, j - cl + 1, a.value(i, j)?)?;
        }
    }
    let (status, u, w, v) = recipes::svd_decompose(u)?;
    if status != 0 {
        res.status = Status::NumericalError;
        return Ok(res);
    }
    let mut max_sv = 0.0;
    for i in 1..=n {
        max_sv = cmax(max_sv, w.value(i)?);
    }
    let threshold = tolerance * max_sv;
    res.rank = 0;
    for i in 1..=n {
        if w.value(i)? > threshold {
            res.rank += 1;
        }
    }
    let mut out_u = Matrix::new(rl, rl + m - 1, cl, cl + n - 1)?;
    for i in 1..=m {
        for j in 1..=n {
            out_u.set_value(rl + i - 1, cl + j - 1, u.value(i, j)?)?;
        }
    }
    let mut out_w = Vector::new(cl, cl + n - 1)?;
    for i in 1..=n {
        out_w.set_value(cl + i - 1, w.value(i)?)?;
    }
    let mut out_v = Matrix::new(cl, cl + n - 1, cl, cl + n - 1)?;
    for i in 1..=n {
        for j in 1..=n {
            out_v.set_value(cl + i - 1, cl + j - 1, v.value(i, j)?)?;
        }
    }
    res.u = Some(out_u);
    res.singular_values = Some(out_w);
    res.v = Some(out_v);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::SolveSVD`.
pub fn solve_svd(a: &Matrix, b: &Vector, tolerance: f64) -> R<LinearResult> {
    let mut res = LinearResult::default();
    let sv = svd(a, tolerance)?;
    if sv.status != Status::Ok {
        res.status = sv.status;
        return Ok(res);
    }
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let m = ru - rl + 1;
    if b.length() != m {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let bl = b.lower();
    let (Some(u), Some(w), Some(v)) = (&sv.u, &sv.singular_values, &sv.v) else {
        return Ok(res);
    };
    let mut max_sv = 0.0;
    for i in cl..=cu {
        max_sv = cmax(max_sv, w.value(i)?);
    }
    let w_min = tolerance * max_sv;
    let mut tmp = Vector::filled(cl, cu, 0.0)?;
    for j in cl..=cu {
        let mut sum = 0.0;
        for i in rl..=ru {
            sum += u.value(i, j)? * b.value(bl + (i - rl))?;
        }
        if w.value(j)? > w_min {
            tmp.set_value(j, sum / w.value(j)?)?;
        }
    }
    let mut sol = Vector::filled(cl, cu, 0.0)?;
    for i in cl..=cu {
        let mut sum = 0.0;
        for j in cl..=cu {
            sum += v.value(i, j)? * tmp.value(j)?;
        }
        sol.set_value(i, sum)?;
    }
    res.solution = Some(sol);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::PseudoInverse`.
pub fn pseudo_inverse(a: &Matrix, tolerance: f64) -> R<InverseResult> {
    let mut res = InverseResult::default();
    let sv = svd(a, tolerance)?;
    if sv.status != Status::Ok {
        res.status = sv.status;
        return Ok(res);
    }
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let (Some(u), Some(w), Some(v)) = (&sv.u, &sv.singular_values, &sv.v) else {
        return Ok(res);
    };
    let mut max_sv = 0.0;
    for i in cl..=cu {
        max_sv = cmax(max_sv, w.value(i)?);
    }
    let w_min = tolerance * max_sv;
    let mut inv = Matrix::filled(cl, cu, rl, ru, 0.0)?;
    for i in cl..=cu {
        for j in rl..=ru {
            let mut sum = 0.0;
            for k in cl..=cu {
                if w.value(k)? > w_min {
                    sum += v.value(i, k)? * u.value(j, k)? / w.value(k)?;
                }
            }
            inv.set_value(i, j, sum)?;
        }
    }
    res.inverse = Some(inv);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::ConditionNumber` (default tolerance of `SVD`).
pub fn condition_number(a: &Matrix) -> R<f64> {
    let sv = svd(a, 1.0e-15)?;
    let Some(w) = (if sv.status == Status::Ok { sv.singular_values.as_ref() } else { None }) else {
        return Ok(f64::INFINITY);
    };
    let (lower, upper) = (w.lower(), w.upper());
    let mut max_sv = 0.0;
    let mut min_sv = f64::MAX;
    for i in lower..=upper {
        let wi = w.value(i)?;
        if wi > 0.0 {
            max_sv = cmax(max_sv, wi);
            min_sv = cmin(min_sv, wi);
        }
    }
    if min_sv <= 0.0 || max_sv <= 0.0 {
        return Ok(f64::INFINITY);
    }
    Ok(max_sv / min_sv)
}

/// `MathLin::NumericalRank`.
pub fn numerical_rank(a: &Matrix, tolerance: f64) -> R<i32> {
    let sv = svd(a, tolerance)?;
    Ok(if sv.status == Status::Ok { sv.rank } else { 0 })
}

// ---------------------------------------------------------------------------------------------
// MathLin_Householder.hxx
// ---------------------------------------------------------------------------------------------

/// `MathLin::QR`.
pub fn qr(a: &Matrix, tolerance: f64) -> R<QrResult> {
    let mut res = QrResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let m = ru - rl + 1;
    let n = cu - cl + 1;
    if m < n {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let mut r = Matrix::new(rl, ru, cl, cu)?;
    for i in rl..=ru {
        for j in cl..=cu {
            r.set_value(i, j, a.value(i, j)?)?;
        }
    }
    let mut q = Matrix::filled(rl, ru, rl, ru, 0.0)?;
    for i in rl..=ru {
        q.set_value(i, i, 1.0)?;
    }
    let mut v = Vector::new(rl, ru)?;
    res.rank = 0;
    for j in cl..=cu {
        let j_offset = j - cl;
        let mut norm = 0.0;
        for i in rl + j_offset..=ru {
            let x = r.value(i, j)?;
            norm += x * x;
        }
        let norm = norm.sqrt();
        if norm < tolerance {
            continue;
        }
        res.rank += 1;
        let alpha = r.value(rl + j_offset, j)?;
        let beta = if alpha >= 0.0 { -norm } else { norm };
        for i in rl..rl + j_offset {
            v.set_value(i, 0.0)?;
        }
        v.set_value(rl + j_offset, alpha - beta)?;
        for i in rl + j_offset + 1..=ru {
            v.set_value(i, r.value(i, j)?)?;
        }
        let mut v_norm_sq = 0.0;
        for i in rl + j_offset..=ru {
            let x = v.value(i)?;
            v_norm_sq += x * x;
        }
        if v_norm_sq < ZERO_TOL {
            continue;
        }
        let tau = 2.0 / v_norm_sq;
        for k in j..=cu {
            let mut v_dot_rk = 0.0;
            for i in rl + j_offset..=ru {
                v_dot_rk += v.value(i)? * r.value(i, k)?;
            }
            for i in rl + j_offset..=ru {
                let x = r.value(i, k)? - tau * v_dot_rk * v.value(i)?;
                r.set_value(i, k, x)?;
            }
        }
        for k in rl..=ru {
            let mut qk_v = 0.0;
            for i in rl + j_offset..=ru {
                qk_v += q.value(k, i)? * v.value(i)?;
            }
            for i in rl + j_offset..=ru {
                let x = q.value(k, i)? - tau * qk_v * v.value(i)?;
                q.set_value(k, i, x)?;
            }
        }
    }
    for i in rl..=ru {
        let mut j = cl;
        while j < cl + (i - rl) && j <= cu {
            r.set_value(i, j, 0.0)?;
            j += 1;
        }
    }
    res.q = Some(q);
    res.r = Some(r);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::SolveQR`.
pub fn solve_qr(a: &Matrix, b: &Vector, tolerance: f64) -> R<LinearResult> {
    let mut res = LinearResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let m = ru - rl + 1;
    if b.length() != m {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let qr_res = qr(a, tolerance)?;
    if qr_res.status != Status::Ok {
        res.status = qr_res.status;
        return Ok(res);
    }
    let (Some(q), Some(r)) = (&qr_res.q, &qr_res.r) else {
        return Ok(res);
    };
    let mut c = Vector::filled(rl, ru, 0.0)?;
    for i in rl..=ru {
        let mut sum = 0.0;
        for k in rl..=ru {
            sum += q.value(k, i)? * b.value(b.lower() + k - rl)?;
        }
        c.set_value(i, sum)?;
    }
    let mut sol = Vector::filled(cl, cu, 0.0)?;
    for i in (cl..=cu).rev() {
        let i_offset = i - cl;
        let diag = r.value(rl + i_offset, i)?;
        if diag.abs() < tolerance {
            res.status = Status::Singular;
            res.solution = Some(sol);
            return Ok(res);
        }
        let mut sum = c.value(rl + i_offset)?;
        for k in i + 1..=cu {
            sum -= r.value(rl + i_offset, k)? * sol.value(k)?;
        }
        sol.set_value(i, sum / diag)?;
    }
    res.solution = Some(sol);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::SolveQRMultiple`.
pub fn solve_qr_multiple(a: &Matrix, b: &Matrix, tolerance: f64) -> R<LinearMultipleResult> {
    let mut res = LinearMultipleResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    if b.row_number() != a.row_number() {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let qr_res = qr(a, tolerance)?;
    if qr_res.status != Status::Ok {
        res.status = qr_res.status;
        return Ok(res);
    }
    let (Some(q), Some(r)) = (&qr_res.q, &qr_res.r) else {
        return Ok(res);
    };
    let mut x = Matrix::filled(cl, cu, b.lower_col(), b.upper_col(), 0.0)?;
    for j in b.lower_col()..=b.upper_col() {
        let mut c = Vector::filled(rl, ru, 0.0)?;
        for i in rl..=ru {
            let mut sum = 0.0;
            for k in rl..=ru {
                let b_row = b.lower_row() + (k - rl);
                sum += q.value(k, i)? * b.value(b_row, j)?;
            }
            c.set_value(i, sum)?;
        }
        for i in (cl..=cu).rev() {
            let i_offset = i - cl;
            let r_row = rl + i_offset;
            let diag = r.value(r_row, i)?;
            if diag.abs() < tolerance {
                res.status = Status::Singular;
                return Ok(res);
            }
            let mut sum = c.value(r_row)?;
            for k in i + 1..=cu {
                sum -= r.value(r_row, k)? * x.value(k, j)?;
            }
            x.set_value(i, j, sum / diag)?;
        }
    }
    res.solutions = Some(x);
    res.status = Status::Ok;
    Ok(res)
}

// ---------------------------------------------------------------------------------------------
// MathLin_Jacobi.hxx
// ---------------------------------------------------------------------------------------------

/// `MathLin::Jacobi`.
pub fn jacobi(a: &Matrix, sort_descending: bool) -> R<EigenResult> {
    let mut res = EigenResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let n = ru - rl + 1;
    if cu - cl + 1 != n {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let mut work = Matrix::new(1, n, 1, n)?;
    for i in rl..=ru {
        for j in cl..=cu {
            work.set_value(i - rl + 1, j - cl + 1, a.value(i, j)?)?;
        }
    }
    let jr = recipes::jacobi(work)?;
    if jr.status != 0 {
        res.status = Status::NotConverged;
        return Ok(res);
    }
    let mut vals = jr.values;
    let mut vecs = jr.vectors;
    res.nb_iterations = usize::try_from(jr.rotations).unwrap_or(0);
    if sort_descending && n > 1 {
        for i in 1..n {
            let mut max_idx = i;
            let mut max_val = vals.value(i)?;
            for j in i + 1..=n {
                if vals.value(j)? > max_val {
                    max_val = vals.value(j)?;
                    max_idx = j;
                }
            }
            if max_idx != i {
                let (x, y) = (vals.value(i)?, vals.value(max_idx)?);
                vals.set_value(i, y)?;
                vals.set_value(max_idx, x)?;
                for k in 1..=n {
                    let (x, y) = (vecs.value(k, i)?, vecs.value(k, max_idx)?);
                    vecs.set_value(k, i, y)?;
                    vecs.set_value(k, max_idx, x)?;
                }
            }
        }
    }
    let mut out_vals = Vector::new(rl, ru)?;
    for i in 1..=n {
        out_vals.set_value(rl + i - 1, vals.value(i)?)?;
    }
    let mut out_vecs = Matrix::new(rl, ru, cl, cu)?;
    for i in 1..=n {
        for j in 1..=n {
            out_vecs.set_value(rl + i - 1, cl + j - 1, vecs.value(i, j)?)?;
        }
    }
    res.eigen_values = Some(out_vals);
    res.eigen_vectors = Some(out_vecs);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::EigenValues`.
pub fn eigen_values(a: &Matrix, sort_descending: bool) -> R<EigenResult> {
    jacobi(a, sort_descending)
}

/// `MathLin::SpectralDecomposition`.
pub fn spectral_decomposition(a: &Matrix) -> R<EigenResult> {
    jacobi(a, false)
}

/// `MathLin::MatrixPower`.
pub fn matrix_power(a: &Matrix, power: f64) -> R<Option<Matrix>> {
    let eigen = jacobi(a, false)?;
    if eigen.status != Status::Ok {
        return Ok(None);
    }
    let (Some(d), Some(v)) = (&eigen.eigen_values, &eigen.eigen_vectors) else {
        return Ok(None);
    };
    let (lower, upper) = (d.lower(), d.upper());
    let mut dp = Vector::new(lower, upper)?;
    for i in lower..=upper {
        let di = d.value(i)?;
        if di < 0.0 && power != libm::floor(power) {
            return Ok(None);
        }
        let mut x = if di >= 0.0 { libm::pow(di, power) } else { libm::pow(-di, power) };
        dp.set_value(i, x)?;
        if di < 0.0 && (power as i32) % 2 != 0 {
            x = -x;
            dp.set_value(i, x)?;
        }
    }
    let mut out = Matrix::filled(lower, upper, lower, upper, 0.0)?;
    for i in lower..=upper {
        for j in lower..=upper {
            let mut sum = 0.0;
            for k in lower..=upper {
                sum += v.value(i, k)? * dp.value(k)? * v.value(j, k)?;
            }
            out.set_value(i, j, sum)?;
        }
    }
    Ok(Some(out))
}

/// `MathLin::MatrixSqrt`.
pub fn matrix_sqrt(a: &Matrix) -> R<Option<Matrix>> {
    matrix_power(a, 0.5)
}

/// `MathLin::MatrixInvSqrt`.
pub fn matrix_inv_sqrt(a: &Matrix) -> R<Option<Matrix>> {
    matrix_power(a, -0.5)
}

// ---------------------------------------------------------------------------------------------
// MathLin_LeastSquares.hxx
// ---------------------------------------------------------------------------------------------

/// `||A x - b||` bookkeeping shared by the least-squares solvers.
fn residual(a: &Matrix, b: &Vector, x: &Vector) -> R<f64> {
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let mut residual_sq = 0.0;
    for i in rl..=ru {
        let mut axi = 0.0;
        for j in cl..=cu {
            axi += a.value(i, j)? * x.value(j)?;
        }
        let ri = axi - b.value(b.lower() + i - rl)?;
        residual_sq += ri * ri;
    }
    Ok(residual_sq)
}

/// `AtA` and `Atb` of the normal equations (with `lambda` added to the diagonal when `lambda` is `Some`).
fn normal_equations(a: &Matrix, b: &Vector, lambda: Option<f64>) -> R<(Matrix, Vector)> {
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let mut ata = Matrix::filled(cl, cu, cl, cu, 0.0)?;
    let mut atb = Vector::filled(cl, cu, 0.0)?;
    for i in cl..=cu {
        for j in cl..=cu {
            let mut sum = 0.0;
            for k in rl..=ru {
                sum += a.value(k, i)? * a.value(k, j)?;
            }
            ata.set_value(i, j, sum)?;
        }
        if let Some(l) = lambda {
            let d = ata.value(i, i)? + l;
            ata.set_value(i, i, d)?;
        }
    }
    for i in cl..=cu {
        let mut sum = 0.0;
        for k in rl..=ru {
            sum += a.value(k, i)? * b.value(b.lower() + k - rl)?;
        }
        atb.set_value(i, sum)?;
    }
    Ok((ata, atb))
}

/// `MathLin::LeastSquares`.
pub fn least_squares(a: &Matrix, b: &Vector, method: LeastSquaresMethod, tolerance: f64) -> R<LeastSquaresResult> {
    let mut res = LeastSquaresResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let m = ru - rl + 1;
    let n = cu - cl + 1;
    if b.length() != m {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let lin = match method {
        LeastSquaresMethod::NormalEquations => {
            let (ata, atb) = normal_equations(a, b, None)?;
            let lin = solve(&ata, &atb, tolerance)?;
            res.rank = if lin.status == Status::Ok { n } else { 0 };
            lin
        }
        LeastSquaresMethod::Qr => {
            let lin = solve_qr(a, b, tolerance)?;
            if lin.status == Status::Ok {
                res.rank = n;
            }
            lin
        }
        LeastSquaresMethod::Svd => {
            let lin = solve_svd(a, b, tolerance)?;
            if lin.status == Status::Ok {
                let sv = svd(a, tolerance)?;
                res.rank = if sv.status == Status::Ok { sv.rank } else { 0 };
            }
            lin
        }
    };
    if lin.status != Status::Ok {
        res.status = lin.status;
        return Ok(res);
    }
    let Some(x) = lin.solution else {
        return Ok(res);
    };
    let residual_sq = residual(a, b, &x)?;
    res.solution = Some(x);
    res.residual_sq = Some(residual_sq);
    res.residual = Some(residual_sq.sqrt());
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::WeightedLeastSquares`.
pub fn weighted_least_squares(
    a: &Matrix,
    b: &Vector,
    w: &Vector,
    method: LeastSquaresMethod,
    tolerance: f64,
) -> R<LeastSquaresResult> {
    let mut res = LeastSquaresResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let m = ru - rl + 1;
    if b.length() != m || w.length() != m {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    for i in w.lower()..=w.upper() {
        if w.value(i)? <= 0.0 {
            res.status = Status::InvalidInput;
            return Ok(res);
        }
    }
    let mut wa = Matrix::new(rl, ru, cl, cu)?;
    let mut wb = Vector::new(b.lower(), b.upper())?;
    for i in rl..=ru {
        let sqrt_w = w.value(w.lower() + i - rl)?.sqrt();
        for j in cl..=cu {
            wa.set_value(i, j, sqrt_w * a.value(i, j)?)?;
        }
        wb.set_value(b.lower() + i - rl, sqrt_w * b.value(b.lower() + i - rl)?)?;
    }
    least_squares(&wa, &wb, method, tolerance)
}

/// `MathLin::RegularizedLeastSquares`.
pub fn regularized_least_squares(a: &Matrix, b: &Vector, lambda: f64, tolerance: f64) -> R<LeastSquaresResult> {
    let mut res = LeastSquaresResult::default();
    let (rl, ru, cl, cu) = (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col());
    let m = ru - rl + 1;
    let n = cu - cl + 1;
    if b.length() != m {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    if lambda < 0.0 {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    let (ata, atb) = normal_equations(a, b, Some(lambda))?;
    let lin = solve(&ata, &atb, tolerance)?;
    if lin.status != Status::Ok {
        res.status = lin.status;
        return Ok(res);
    }
    let Some(x) = lin.solution else {
        return Ok(res);
    };
    res.rank = n;
    let residual_sq = residual(a, b, &x)?;
    res.solution = Some(x);
    res.residual_sq = Some(residual_sq);
    res.residual = Some(residual_sq.sqrt());
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::OptimalRegularization`.
pub fn optimal_regularization(a: &Matrix, b: &Vector, lambda_min: f64, lambda_max: f64, nb_points: i32) -> R<f64> {
    let mut best_lambda = lambda_min;
    let mut best_score = f64::MAX;
    let log_min = libm::log10(lambda_min);
    let log_max = libm::log10(lambda_max);
    let log_step = (log_max - log_min) / f64::from(nb_points - 1);
    for k in 0..nb_points {
        let lambda = libm::pow(10.0, log_min + f64::from(k) * log_step);
        let r = regularized_least_squares(a, b, lambda, 1.0e-15)?;
        if r.status == Status::Ok
            && let Some(score) = r.residual_sq
            && score < best_score
        {
            best_score = score;
            best_lambda = lambda;
        }
    }
    Ok(best_lambda)
}

// ---------------------------------------------------------------------------------------------
// MathLin_EigenSearch.hxx
// ---------------------------------------------------------------------------------------------

fn hypot(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

/// `Internal::FindSubmatrixEnd`.
fn find_submatrix_end(diag: &Vector, subdiag: &Vector, start: i32, n: i32) -> R<i32> {
    let lower = diag.lower();
    let mut end = start;
    while end < n {
        let diag_sum = diag.value(end + lower - 1)?.abs() + diag.value(end + 1 + lower - 1)?.abs();
        if subdiag.value(end + lower - 1)?.abs() + diag_sum == diag_sum {
            break;
        }
        end += 1;
    }
    Ok(end)
}

/// `Internal::ComputeWilkinsonShift`.
fn wilkinson_shift(diag: &Vector, subdiag: &Vector, start: i32, end: i32) -> R<f64> {
    let lower = diag.lower();
    let mut shift = (diag.value(start + 1 + lower - 1)? - diag.value(start + lower - 1)?)
        / (2.0 * subdiag.value(start + lower - 1)?);
    let radius = hypot(1.0, shift);
    if shift < 0.0 {
        shift = diag.value(end + lower - 1)? - diag.value(start + lower - 1)?
            + subdiag.value(start + lower - 1)? / (shift - radius);
    } else {
        shift = diag.value(end + lower - 1)? - diag.value(start + lower - 1)?
            + subdiag.value(start + lower - 1)? / (shift + radius);
    }
    Ok(shift)
}

/// `Internal::PerformQLStep` (always succeeds).
fn ql_step(
    diag: &mut Vector,
    subdiag: &mut Vector,
    vec: &mut Matrix,
    start: i32,
    end: i32,
    shift_in: f64,
    n: i32,
) -> R<()> {
    let ld = diag.lower();
    let lv = vec.lower_row();
    let mut sine = 1.0;
    let mut cosine = 1.0;
    let mut prev_diag = 0.0;
    let mut shift = shift_in;
    let mut radius = 0.0;
    let mut row_idx = end - 1;
    while row_idx >= start {
        let temp_val = sine * subdiag.value(row_idx + ld - 1)?;
        let subdiag_temp = cosine * subdiag.value(row_idx + ld - 1)?;
        radius = hypot(temp_val, shift);
        subdiag.set_value(row_idx + 1 + ld - 1, radius)?;
        if radius == 0.0 {
            let x = diag.value(row_idx + 1 + ld - 1)? - prev_diag;
            diag.set_value(row_idx + 1 + ld - 1, x)?;
            subdiag.set_value(end + ld - 1, 0.0)?;
            break;
        }
        sine = temp_val / radius;
        cosine = shift / radius;
        shift = diag.value(row_idx + 1 + ld - 1)? - prev_diag;
        let radius_temp = (diag.value(row_idx + ld - 1)? - shift) * sine + 2.0 * cosine * subdiag_temp;
        prev_diag = sine * radius_temp;
        diag.set_value(row_idx + 1 + ld - 1, shift + prev_diag)?;
        shift = cosine * radius_temp - subdiag_temp;
        for vec_idx in 1..=n {
            let temp_vec = vec.value(vec_idx + lv - 1, row_idx + 1 + lv - 1)?;
            let a = sine * vec.value(vec_idx + lv - 1, row_idx + lv - 1)? + cosine * temp_vec;
            vec.set_value(vec_idx + lv - 1, row_idx + 1 + lv - 1, a)?;
            let b = cosine * vec.value(vec_idx + lv - 1, row_idx + lv - 1)? - sine * temp_vec;
            vec.set_value(vec_idx + lv - 1, row_idx + lv - 1, b)?;
        }
        row_idx -= 1;
    }
    if radius == 0.0 && row_idx >= 1 {
        return Ok(());
    }
    let x = diag.value(start + ld - 1)? - prev_diag;
    diag.set_value(start + ld - 1, x)?;
    subdiag.set_value(start + ld - 1, shift)?;
    subdiag.set_value(end + ld - 1, 0.0)?;
    Ok(())
}

/// `MathLin::EigenTridiagonal`.
pub fn eigen_tridiagonal(diagonal: &Vector, subdiagonal: &Vector, max_iterations: i32) -> R<TridiagonalEigenResult> {
    let mut res = TridiagonalEigenResult::default();
    let n = diagonal.length();
    if subdiagonal.length() != n {
        res.status = Status::InvalidInput;
        return Ok(res);
    }
    res.dimension = n;
    let mut diag = Vector::new(1, n)?;
    let mut subdiag = Vector::new(1, n)?;
    for i in 1..=n {
        diag.set_value(i, diagonal.value(diagonal.lower() + i - 1)?)?;
    }
    for i in 2..=n {
        subdiag.set_value(i - 1, subdiagonal.value(subdiagonal.lower() + i - 1)?)?;
    }
    subdiag.set_value(n, 0.0)?;
    let mut eigen_vec = Matrix::filled(1, n, 1, n, 0.0)?;
    for i in 1..=n {
        eigen_vec.set_value(i, i, 1.0)?;
    }
    if n == 1 {
        res.eigen_values = Some(diag);
        res.eigen_vectors = Some(eigen_vec);
        res.status = Status::Ok;
        return Ok(res);
    }
    for start in 1..=n {
        let mut iter_count = 0;
        loop {
            let end = find_submatrix_end(&diag, &subdiag, start, n)?;
            if end != start {
                let it = iter_count;
                iter_count += 1;
                if it >= max_iterations {
                    res.status = Status::MaxIterations;
                    return Ok(res);
                }
                let shift = wilkinson_shift(&diag, &subdiag, start, end)?;
                ql_step(&mut diag, &mut subdiag, &mut eigen_vec, start, end, shift, n)?;
            }
            if end == start {
                break;
            }
        }
    }
    res.eigen_values = Some(diag);
    res.eigen_vectors = Some(eigen_vec);
    res.status = Status::Ok;
    Ok(res)
}

/// `MathLin::GetEigenVector`; without eigenvectors C++ returns an uninitialised `math_Vector(1, 1)`.
pub fn get_eigen_vector(result: &TridiagonalEigenResult, index: i32) -> R<Vector> {
    let Some(vecs) = &result.eigen_vectors else {
        return Vector::new(1, 1);
    };
    let n = vecs.row_number();
    let (lr, lc) = (vecs.lower_row(), vecs.lower_col());
    let mut v = Vector::new(1, n)?;
    for i in 1..=n {
        v.set_value(i, vecs.value(i + lr - 1, index + lc - 1)?)?;
    }
    Ok(v)
}
