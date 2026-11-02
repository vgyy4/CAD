//! OCCT's header-only `MathSys` package: Newton and Levenberg-Marquardt solvers for systems of nonlinear
//! equations, and the allocation-free 2D, 3D and 4D Newton solvers used by the extrema algorithms.
//!
//! Translated from `MathSys_*.hxx` (OCCT 8.0.1). The generic solvers work on `mathnd::FunctionSetWithDerivatives`
//! and `mathlin::{Vector, Matrix}` (OCCT's absolute indices; `Err` where C++ throws a range error); the fixed-size
//! solvers work on plain arrays.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::gp::{Pnt, Vec as Vec3};
use crate::mathlin::{MathError, Matrix, Vector};
use crate::mathlin_pkg;
use crate::mathnd::FunctionSetWithDerivatives;
use crate::mathutils::{self, Config, Status, VectorResult};

type R<T> = Result<T, MathError>;

/// `std::max(a, b)`.
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `std::min(a, b)`.
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::clamp(v, lo, hi)`.
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

// ---------------------------------------------------------------------------------------------
// MathSys_Newton.hxx
// ---------------------------------------------------------------------------------------------

/// The convergence tests and the update shared by `Newton` and `NewtonBounded`.
#[allow(clippy::too_many_arguments)]
fn newton_impl<F: FunctionSetWithDerivatives + ?Sized>(
    f: &mut F,
    start: &Vector,
    bounds: Option<(&Vector, &Vector)>,
    tol_x: &Vector,
    tol_f: f64,
    max_iter: usize,
) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let n = f.nb_variables();
    let m = f.nb_equations();
    let bounds_ok = bounds.is_none_or(|(inf, sup)| inf.length() == n && sup.length() == n);
    if n != m || start.length() != n || tol_x.length() != n || !bounds_ok {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let lower = start.lower();
    let upper = start.upper();
    let mut sol = start.clone();
    let mut fv = Vector::new(lower, upper)?;
    let mut jacobian = Matrix::new(lower, upper, lower, upper)?;
    if let Some((inf, sup)) = bounds {
        for i in lower..=upper {
            if sol.value(i)? < inf.value(i)? {
                sol.set_value(i, inf.value(i)?)?;
            }
            if sol.value(i)? > sup.value(i)? {
                sol.set_value(i, sup.value(i)?)?;
            }
        }
    }
    for iter in 0..max_iter {
        if !f.values(&sol, &mut fv, &mut jacobian) {
            result.status = Status::NumericalError;
            result.nb_iterations = iter;
            return Ok(result);
        }
        let mut neg_f = Vector::new(lower, upper)?;
        for i in lower..=upper {
            neg_f.set_value(i, -fv.value(i)?)?;
        }
        let lin = mathlin_pkg::solve(&jacobian, &neg_f, 1.0e-20)?;
        let (mathlin_pkg::Status::Ok, Some(delta_x)) = (lin.status, lin.solution) else {
            result.status = Status::Singular;
            result.nb_iterations = iter;
            return Ok(result);
        };
        let mut x_converged = true;
        for i in lower..=upper {
            if delta_x.value(i)?.abs() > tol_x.value(i)? {
                x_converged = false;
                break;
            }
        }
        for i in lower..=upper {
            sol.set_value(i, sol.value(i)? + delta_x.value(i)?)?;
            if let Some((inf, sup)) = bounds {
                if sol.value(i)? < inf.value(i)? {
                    sol.set_value(i, inf.value(i)?)?;
                }
                if sol.value(i)? > sup.value(i)? {
                    sol.set_value(i, sup.value(i)?)?;
                }
            }
        }
        if !f.value(&sol, &mut fv) {
            result.status = Status::NumericalError;
            result.nb_iterations = iter + 1;
            return Ok(result);
        }
        let mut f_converged = true;
        for i in lower..=upper {
            if fv.value(i)?.abs() > tol_f {
                f_converged = false;
                break;
            }
        }
        if x_converged && f_converged {
            result.status = Status::Ok;
            result.nb_iterations = iter + 1;
            result.solution = Some(sol);
            result.jacobian = Some(jacobian);
            return Ok(result);
        }
        result.nb_iterations = iter + 1;
    }
    result.status = Status::MaxIterations;
    result.solution = Some(sol);
    result.jacobian = Some(jacobian);
    Ok(result)
}

/// `MathSys::Newton` with a tolerance vector (default `max_iter` 100).
pub fn newton<F: FunctionSetWithDerivatives + ?Sized>(
    f: &mut F,
    start: &Vector,
    tol_x: &Vector,
    tol_f: f64,
    max_iter: usize,
) -> R<VectorResult> {
    newton_impl(f, start, None, tol_x, tol_f, max_iter)
}

/// `MathSys::NewtonBounded` (default `max_iter` 100).
pub fn newton_bounded<F: FunctionSetWithDerivatives + ?Sized>(
    f: &mut F,
    start: &Vector,
    inf_bound: &Vector,
    sup_bound: &Vector,
    tol_x: &Vector,
    tol_f: f64,
    max_iter: usize,
) -> R<VectorResult> {
    newton_impl(f, start, Some((inf_bound, sup_bound)), tol_x, tol_f, max_iter)
}

/// `MathSys::Newton` with one scalar tolerance (default `max_iter` 100).
pub fn newton_scalar_tol<F: FunctionSetWithDerivatives + ?Sized>(
    f: &mut F,
    start: &Vector,
    tol_x: f64,
    tol_f: f64,
    max_iter: usize,
) -> R<VectorResult> {
    let tol_x_vec = Vector::filled(start.lower(), start.upper(), tol_x)?;
    newton(f, start, &tol_x_vec, tol_f, max_iter)
}

// ---------------------------------------------------------------------------------------------
// MathSys_LevenbergMarquardt.hxx
// ---------------------------------------------------------------------------------------------

/// `MathSys::LMConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LmConfig {
    pub base: Config,
    pub lambda_init: f64,
    pub lambda_increase: f64,
    pub lambda_decrease: f64,
    pub lambda_max: f64,
    pub lambda_min: f64,
}
impl Default for LmConfig {
    fn default() -> Self {
        Self {
            base: Config::default(),
            lambda_init: 1.0e-3,
            lambda_increase: 10.0,
            lambda_decrease: 0.1,
            lambda_max: 1.0e10,
            lambda_min: 1.0e-12,
        }
    }
}
impl LmConfig {
    /// `LMConfig(theTolerance, theMaxIter)`.
    pub fn new(tolerance: f64, max_iterations: i32) -> Self {
        Self { base: Config::new(tolerance, max_iterations), ..Self::default() }
    }
}

fn lm_impl<F: FunctionSetWithDerivatives + ?Sized>(
    f: &mut F,
    start: &Vector,
    bounds: Option<(&Vector, &Vector)>,
    config: &LmConfig,
) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let nb_vars = f.nb_variables();
    let nb_eqs = f.nb_equations();
    let bounds_ok = bounds.is_none_or(|(inf, sup)| inf.length() == nb_vars && sup.length() == nb_vars);
    if start.length() != nb_vars || !bounds_ok {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let var_lower = start.lower();
    let var_upper = start.upper();
    let mut sol = start.clone();
    let mut fv = Vector::new(1, nb_eqs)?;
    let mut f_new = Vector::new(1, nb_eqs)?;
    let mut grad = Vector::new(var_lower, var_upper)?;
    let mut jac = Matrix::new(1, nb_eqs, var_lower, var_upper)?;
    let mut jtj = Matrix::new(var_lower, var_upper, var_lower, var_upper)?;
    let mut jtf = Vector::new(var_lower, var_upper)?;
    if let Some((inf, sup)) = bounds {
        for i in var_lower..=var_upper {
            sol.set_value(i, mathutils::clamp(sol.value(i)?, inf.value(i)?, sup.value(i)?))?;
        }
    }
    let mut lambda = config.lambda_init;
    if !f.value(&sol, &mut fv) {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    let mut chi2 = 0.0;
    for i in 1..=nb_eqs {
        chi2 += fv.value(i)? * fv.value(i)?;
    }
    for iter in 0..config.base.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        let mut f_converged = true;
        for i in 1..=nb_eqs {
            if fv.value(i)?.abs() > config.base.f_tolerance {
                f_converged = false;
                break;
            }
        }
        if f_converged {
            result.status = Status::Ok;
            result.solution = Some(sol);
            result.value = Some(chi2);
            return Ok(result);
        }
        if !f.derivatives(&sol, &mut jac) {
            result.status = Status::NumericalError;
            return Ok(result);
        }
        for i in var_lower..=var_upper {
            for j in var_lower..=var_upper {
                let mut sum = 0.0;
                for k in 1..=nb_eqs {
                    sum += jac.value(k, i)? * jac.value(k, j)?;
                }
                jtj.set_value(i, j, sum)?;
            }
        }
        for i in var_lower..=var_upper {
            let mut sum = 0.0;
            for k in 1..=nb_eqs {
                sum += jac.value(k, i)? * fv.value(k)?;
            }
            jtf.set_value(i, sum)?;
            grad.set_value(i, 2.0 * sum)?;
        }
        let mut grad_norm = 0.0;
        for i in var_lower..=var_upper {
            grad_norm += grad.value(i)? * grad.value(i)?;
        }
        grad_norm = grad_norm.sqrt();
        if grad_norm < config.base.tolerance {
            result.status = Status::Ok;
            result.solution = Some(sol);
            result.value = Some(chi2);
            result.gradient = Some(grad);
            return Ok(result);
        }
        let mut step_accepted = false;
        let mut lam_iter = 0;
        while lam_iter < 20 && !step_accepted {
            lam_iter += 1;
            let mut damped = jtj.clone();
            for i in var_lower..=var_upper {
                damped.set_value(i, i, damped.value(i, i)? + lambda)?;
            }
            let mut neg_jtf = Vector::new(var_lower, var_upper)?;
            for i in var_lower..=var_upper {
                neg_jtf.set_value(i, -jtf.value(i)?)?;
            }
            let lin = mathlin_pkg::solve(&damped, &neg_jtf, 1.0e-20)?;
            let (mathlin_pkg::Status::Ok, Some(dx)) = (lin.status, lin.solution) else {
                lambda *= config.lambda_increase;
                if lambda > config.lambda_max {
                    result.status = Status::Singular;
                    result.solution = Some(sol);
                    result.value = Some(chi2);
                    return Ok(result);
                }
                continue;
            };
            let delta_x = dx;
            let mut sol_new = Vector::new(var_lower, var_upper)?;
            for i in var_lower..=var_upper {
                let v = sol.value(i)? + delta_x.value(i)?;
                let v = match bounds {
                    Some((inf, sup)) => mathutils::clamp(v, inf.value(i)?, sup.value(i)?),
                    None => v,
                };
                sol_new.set_value(i, v)?;
            }
            if !f.value(&sol_new, &mut f_new) {
                lambda *= config.lambda_increase;
                if lambda > config.lambda_max {
                    result.status = Status::NumericalError;
                    result.solution = Some(sol);
                    result.value = Some(chi2);
                    return Ok(result);
                }
                continue;
            }
            let mut chi2_new = 0.0;
            for i in 1..=nb_eqs {
                chi2_new += f_new.value(i)? * f_new.value(i)?;
            }
            if chi2_new < chi2 {
                sol = sol_new;
                fv = f_new.clone();
                chi2 = chi2_new;
                lambda *= config.lambda_decrease;
                if lambda < config.lambda_min {
                    lambda = config.lambda_min;
                }
                step_accepted = true;
                let mut x_converged = true;
                for i in var_lower..=var_upper {
                    if delta_x.value(i)?.abs() > config.base.x_tolerance * (1.0 + sol.value(i)?.abs()) {
                        x_converged = false;
                        break;
                    }
                }
                if x_converged {
                    result.status = Status::Ok;
                    result.solution = Some(sol);
                    result.value = Some(chi2);
                    return Ok(result);
                }
            } else {
                lambda *= config.lambda_increase;
                if lambda > config.lambda_max {
                    result.status = Status::NotConverged;
                    result.solution = Some(sol);
                    result.value = Some(chi2);
                    return Ok(result);
                }
            }
        }
        if !step_accepted {
            result.status = Status::NotConverged;
            result.solution = Some(sol);
            result.value = Some(chi2);
            return Ok(result);
        }
    }
    result.status = Status::MaxIterations;
    result.solution = Some(sol);
    result.value = Some(chi2);
    Ok(result)
}

/// `MathSys::LevenbergMarquardt`.
pub fn levenberg_marquardt<F: FunctionSetWithDerivatives + ?Sized>(
    f: &mut F,
    start: &Vector,
    config: &LmConfig,
) -> R<VectorResult> {
    lm_impl(f, start, None, config)
}

/// `MathSys::LevenbergMarquardtBounded`.
pub fn levenberg_marquardt_bounded<F: FunctionSetWithDerivatives + ?Sized>(
    f: &mut F,
    start: &Vector,
    inf_bound: &Vector,
    sup_bound: &Vector,
    config: &LmConfig,
) -> R<VectorResult> {
    lm_impl(f, start, Some((inf_bound, sup_bound)), config)
}

// ---------------------------------------------------------------------------------------------
// MathSys_NewtonTypes.hxx
// ---------------------------------------------------------------------------------------------

/// `MathSys::NewtonResultN<N>`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewtonResultN<const N: usize> {
    pub status: Status,
    pub x: [f64; N],
    pub nb_iterations: usize,
    pub residual_norm: f64,
    pub step_norm: f64,
}
impl<const N: usize> Default for NewtonResultN<N> {
    fn default() -> Self {
        Self { status: Status::NotConverged, x: [0.0; N], nb_iterations: 0, residual_norm: 0.0, step_norm: 0.0 }
    }
}
impl<const N: usize> NewtonResultN<N> {
    /// `IsDone()` (and the C++ `explicit operator bool`).
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
}

/// `MathSys::NewtonBoundsN<N>` (`HasBounds` defaults to `true`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewtonBoundsN<const N: usize> {
    pub min: [f64; N],
    pub max: [f64; N],
    pub has_bounds: bool,
}
impl<const N: usize> Default for NewtonBoundsN<N> {
    fn default() -> Self {
        Self { min: [0.0; N], max: [0.0; N], has_bounds: true }
    }
}

/// `MathSys::NewtonOptions` (derives from `MathUtils::Config`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewtonOptions {
    pub base: Config,
    pub max_step_ratio: f64,
    pub enable_line_search: bool,
    pub allow_soft_bounds: bool,
    pub soft_bounds_extension: f64,
}
impl Default for NewtonOptions {
    fn default() -> Self {
        Self {
            base: Config {
                tolerance: 1.0e-10,
                x_tolerance: 1.0e-16,
                f_tolerance: 1.0e-10,
                max_iterations: 100,
                ..Config::default()
            },
            max_step_ratio: 0.5,
            enable_line_search: true,
            allow_soft_bounds: false,
            soft_bounds_extension: 1.0e-4,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// MathSys_Newton2D.hxx / 3D / 4D
// ---------------------------------------------------------------------------------------------

/// `detail::THE_SINGULAR_DET_TOL`.
pub const THE_SINGULAR_DET_TOL: f64 = 1.0e-25;
/// `detail::THE_CRITICAL_GRAD_SQ`.
pub const THE_CRITICAL_GRAD_SQ: f64 = 1.0e-60;
/// `detail::THE_LINE_SEARCH_MAX`.
pub const THE_LINE_SEARCH_MAX: i32 = 8;
/// `detail::THE_ARMIJO_C1`.
pub const THE_ARMIJO_C1: f64 = 1.0e-4;

/// `detail::IsOptionsValid` (also `IsOptionsValid3D` and `IsOptionsValid4D`, which are identical).
pub fn is_options_valid(o: &NewtonOptions) -> bool {
    o.base.f_tolerance > 0.0
        && o.base.x_tolerance > 0.0
        && o.base.max_iterations > 0
        && o.max_step_ratio > 0.0
        && o.soft_bounds_extension >= 0.0
}

/// `detail::IsOptionsValid3D`.
pub fn is_options_valid_3d(o: &NewtonOptions) -> bool {
    is_options_valid(o)
}

/// `detail::IsOptionsValid4D`.
pub fn is_options_valid_4d(o: &NewtonOptions) -> bool {
    is_options_valid(o)
}

/// `detail::IsBoundsValid2D`/`3D`/`4D`.
fn is_bounds_valid<const N: usize>(b: &NewtonBoundsN<N>) -> bool {
    if !b.has_bounds {
        return true;
    }
    b.min.iter().zip(&b.max).all(|(lo, hi)| lo <= hi)
}

/// `detail::IsBoundsValid2D`.
pub fn is_bounds_valid_2d(b: &NewtonBoundsN<2>) -> bool {
    is_bounds_valid(b)
}

/// `detail::IsBoundsValid3D`.
pub fn is_bounds_valid_3d(b: &NewtonBoundsN<3>) -> bool {
    is_bounds_valid(b)
}

/// `detail::IsBoundsValid4D`.
pub fn is_bounds_valid_4d(b: &NewtonBoundsN<4>) -> bool {
    is_bounds_valid(b)
}

/// `detail::MaxDomainSize2D`/`3D`/`4D`: `max(1, largest extent)` (`1` without bounds).
fn max_domain_size<const N: usize>(b: &NewtonBoundsN<N>) -> f64 {
    if !b.has_bounds {
        return 1.0;
    }
    // std::max(1.0, std::max(aD0, std::max(aD1, ...))): the innermost maximum is taken first
    let mut m: Option<f64> = None;
    for i in (0..N).rev() {
        let d = b.max.get(i).copied().unwrap_or(0.0) - b.min.get(i).copied().unwrap_or(0.0);
        m = Some(match m {
            Some(inner) => cmax(d, inner),
            None => d,
        });
    }
    cmax(1.0, m.unwrap_or(0.0))
}

/// `detail::MaxDomainSize2D`.
pub fn max_domain_size_2d(b: &NewtonBoundsN<2>) -> f64 {
    max_domain_size(b)
}

/// `detail::MaxDomainSize3D`.
pub fn max_domain_size_3d(b: &NewtonBoundsN<3>) -> f64 {
    max_domain_size(b)
}

/// `detail::MaxDomainSize4D`.
pub fn max_domain_size_4d(b: &NewtonBoundsN<4>) -> f64 {
    max_domain_size(b)
}

/// `detail::Clamp2D`/`3D`/`4D`.
fn clamp_n<const N: usize>(x: &mut [f64; N], b: &NewtonBoundsN<N>, use_soft_bounds: bool, soft_ext_ratio: f64) {
    if !b.has_bounds {
        return;
    }
    for i in 0..N {
        let (lo, hi) = (b.min.get(i).copied().unwrap_or(0.0), b.max.get(i).copied().unwrap_or(0.0));
        let ext = if use_soft_bounds { (hi - lo) * soft_ext_ratio } else { 0.0 };
        if let Some(slot) = x.get_mut(i) {
            *slot = clamp(*slot, lo - ext, hi + ext);
        }
    }
}

/// `detail::Clamp2D`.
pub fn clamp_2d(x: &mut [f64; 2], b: &NewtonBoundsN<2>, use_soft_bounds: bool, soft_ext_ratio: f64) {
    clamp_n(x, b, use_soft_bounds, soft_ext_ratio);
}

/// `detail::Clamp3D`.
pub fn clamp_3d(x: &mut [f64; 3], b: &NewtonBoundsN<3>, use_soft_bounds: bool, soft_ext_ratio: f64) {
    clamp_n(x, b, use_soft_bounds, soft_ext_ratio);
}

/// `detail::Clamp4D`.
pub fn clamp_4d(x: &mut [f64; 4], b: &NewtonBoundsN<4>, use_soft_bounds: bool, soft_ext_ratio: f64) {
    clamp_n(x, b, use_soft_bounds, soft_ext_ratio);
}

/// `detail::SolveSymmetric2x2SVD` (default `tol` `1e-15`): `Some((du, dv))`, or `None` (C++ `false`, outputs untouched).
pub fn solve_symmetric_2x2_svd(j11: f64, j12: f64, j22: f64, f1: f64, f2: f64, tol: f64) -> Option<(f64, f64)> {
    let trace = j11 + j22;
    let diff = (j11 - j22) * 0.5;
    let discrim = (diff * diff + j12 * j12).sqrt();
    let lambda1 = trace * 0.5 + discrim;
    let lambda2 = trace * 0.5 - discrim;
    let min_lambda = cmax(lambda1.abs(), lambda2.abs()) * tol;
    if lambda1.abs() < min_lambda && lambda2.abs() < min_lambda {
        return None;
    }
    let (v1x, v1y);
    if j12.abs() > diff.abs() {
        let len1 = (j12 * j12 + (lambda1 - j11) * (lambda1 - j11)).sqrt();
        if len1 < tol {
            return None;
        }
        v1x = j12 / len1;
        v1y = (lambda1 - j11) / len1;
    } else {
        let len1 = ((lambda1 - j22) * (lambda1 - j22) + j12 * j12).sqrt();
        if len1 < tol {
            return None;
        }
        v1x = (lambda1 - j22) / len1;
        v1y = j12 / len1;
    }
    let v2x = -v1y;
    let v2y = v1x;
    let b1 = v1x * (-f1) + v1y * (-f2);
    let b2 = v2x * (-f1) + v2y * (-f2);
    let x1 = if lambda1.abs() > min_lambda { b1 / lambda1 } else { 0.0 };
    let x2 = if lambda2.abs() > min_lambda { b2 / lambda2 } else { 0.0 };
    Some((v1x * x1 + v2x * x2, v1y * x1 + v2y * x2))
}

/// `detail::Solve3x3`: `Some(delta)`, or `None` when the determinant is below `1e-30` (outputs untouched).
pub fn solve3x3(j: &[[f64; 3]; 3], f: &[f64; 3]) -> Option<[f64; 3]> {
    let cof00 = j[1][1] * j[2][2] - j[1][2] * j[2][1];
    let cof01 = j[1][2] * j[2][0] - j[1][0] * j[2][2];
    let cof02 = j[1][0] * j[2][1] - j[1][1] * j[2][0];
    let det = j[0][0] * cof00 + j[0][1] * cof01 + j[0][2] * cof02;
    if det.abs() < 1.0e-30 {
        return None;
    }
    let inv_det = 1.0 / det;
    let cof10 = j[0][2] * j[2][1] - j[0][1] * j[2][2];
    let cof11 = j[0][0] * j[2][2] - j[0][2] * j[2][0];
    let cof12 = j[0][1] * j[2][0] - j[0][0] * j[2][1];
    let cof20 = j[0][1] * j[1][2] - j[0][2] * j[1][1];
    let cof21 = j[0][2] * j[1][0] - j[0][0] * j[1][2];
    let cof22 = j[0][0] * j[1][1] - j[0][1] * j[1][0];
    Some([
        -(cof00 * f[0] + cof10 * f[1] + cof20 * f[2]) * inv_det,
        -(cof01 * f[0] + cof11 * f[1] + cof21 * f[2]) * inv_det,
        -(cof02 * f[0] + cof12 * f[1] + cof22 * f[2]) * inv_det,
    ])
}

/// `A[i][c]` of the augmented 4x5 matrix of `solve4x4` (`0.0` outside).
fn aug(a: &[[f64; 5]; 4], i: usize, c: usize) -> f64 {
    a.get(i).and_then(|row| row.get(c)).copied().unwrap_or(0.0)
}

fn aug_set(a: &mut [[f64; 5]; 4], i: usize, c: usize, v: f64) {
    if let Some(slot) = a.get_mut(i).and_then(|row| row.get_mut(c)) {
        *slot = v;
    }
}

/// `detail::Solve4x4`: Gaussian elimination with partial pivoting; `None` when a pivot is below `1e-30`.
pub fn solve4x4(j: &[[f64; 4]; 4], f: &[f64; 4]) -> Option<[f64; 4]> {
    let mut a = [[0.0_f64; 5]; 4];
    for (i, (row, fi)) in j.iter().zip(f).enumerate() {
        for (c, v) in row.iter().enumerate() {
            aug_set(&mut a, i, c, *v);
        }
        aug_set(&mut a, i, 4, -*fi);
    }
    for k in 0..4 {
        let mut max_row = k;
        let mut max_val = aug(&a, k, k).abs();
        for i in k + 1..4 {
            let val = aug(&a, i, k).abs();
            if val > max_val {
                max_val = val;
                max_row = i;
            }
        }
        if max_val < 1.0e-30 {
            return None;
        }
        if max_row != k {
            for c in k..=4 {
                let t = aug(&a, k, c);
                let other = aug(&a, max_row, c);
                aug_set(&mut a, k, c, other);
                aug_set(&mut a, max_row, c, t);
            }
        }
        let inv_pivot = 1.0 / aug(&a, k, k);
        for i in k + 1..4 {
            let factor = aug(&a, i, k) * inv_pivot;
            for c in k + 1..=4 {
                let v = aug(&a, i, c) - factor * aug(&a, k, c);
                aug_set(&mut a, i, c, v);
            }
            aug_set(&mut a, i, k, 0.0);
        }
    }
    let mut delta = [0.0_f64; 4];
    for i in (0..4).rev() {
        let mut sum = aug(&a, i, 4);
        for c in i + 1..4 {
            sum -= aug(&a, i, c) * delta.get(c).copied().unwrap_or(0.0);
        }
        if let Some(slot) = delta.get_mut(i) {
            *slot = sum / aug(&a, i, i);
        }
    }
    Some(delta)
}

/// A function `F(x, y)` and its Jacobian for `solve2d` (C++ `bool operator()(double, double, double[2], double[2][2])`).
pub trait Function2D {
    fn eval(&mut self, x: f64, y: f64, f: &mut [f64; 2], j: &mut [[f64; 2]; 2]) -> bool;
}
impl<T: FnMut(f64, f64, &mut [f64; 2], &mut [[f64; 2]; 2]) -> bool> Function2D for T {
    fn eval(&mut self, x: f64, y: f64, f: &mut [f64; 2], j: &mut [[f64; 2]; 2]) -> bool {
        self(x, y, f, j)
    }
}

/// A function of three variables with its Jacobian for `solve3d`.
pub trait Function3D {
    fn eval(&mut self, x: f64, y: f64, z: f64, f: &mut [f64; 3], j: &mut [[f64; 3]; 3]) -> bool;
}
impl<T: FnMut(f64, f64, f64, &mut [f64; 3], &mut [[f64; 3]; 3]) -> bool> Function3D for T {
    fn eval(&mut self, x: f64, y: f64, z: f64, f: &mut [f64; 3], j: &mut [[f64; 3]; 3]) -> bool {
        self(x, y, z, f, j)
    }
}

/// A function of four variables with its Jacobian for `solve4d`.
pub trait Function4D {
    fn eval(&mut self, x0: f64, x1: f64, x2: f64, x3: f64, f: &mut [f64; 4], j: &mut [[f64; 4]; 4]) -> bool;
}
impl<T: FnMut(f64, f64, f64, f64, &mut [f64; 4], &mut [[f64; 4]; 4]) -> bool> Function4D for T {
    fn eval(&mut self, x0: f64, x1: f64, x2: f64, x3: f64, f: &mut [f64; 4], j: &mut [[f64; 4]; 4]) -> bool {
        self(x0, x1, x2, x3, f, j)
    }
}

/// A gradient-like system with a symmetric Jacobian for `solve2d_symmetric`.
pub trait SymmetricFunction2D {
    /// `Value(u, v, f1, f2)`: `(f1, f2)`.
    fn value(&mut self, u: f64, v: f64) -> Option<(f64, f64)>;
    /// `ValueAndJacobian(u, v, f1, f2, j11, j12, j22)`: `(f1, f2, j11, j12, j22)`.
    fn value_and_jacobian(&mut self, u: f64, v: f64) -> Option<(f64, f64, f64, f64, f64)>;
}

/// `MathSys::Solve2D`.
pub fn solve2d<F: Function2D + ?Sized>(
    f: &mut F,
    x0: &[f64; 2],
    bounds: &NewtonBoundsN<2>,
    options: &NewtonOptions,
) -> NewtonResultN<2> {
    let mut res = NewtonResultN::<2> { x: *x0, ..NewtonResultN::default() };
    if !is_options_valid(options) || !is_bounds_valid_2d(bounds) {
        res.status = Status::InvalidInput;
        return res;
    }
    clamp_2d(&mut res.x, bounds, false, 0.0);
    let tol_sq = options.base.f_tolerance * options.base.f_tolerance;
    let max_step = options.max_step_ratio * max_domain_size_2d(bounds);
    for iter in 0..options.base.max_iterations {
        res.nb_iterations = (iter + 1) as usize;
        let mut fv = [0.0; 2];
        let mut jm = [[0.0; 2]; 2];
        if !f.eval(res.x[0], res.x[1], &mut fv, &mut jm) {
            res.status = Status::NumericalError;
            return res;
        }
        let f_norm_sq = fv[0] * fv[0] + fv[1] * fv[1];
        res.residual_norm = f_norm_sq.sqrt();
        if f_norm_sq <= tol_sq {
            res.status = Status::Ok;
            return res;
        }
        let (mut du, mut dv);
        let det = jm[0][0] * jm[1][1] - jm[0][1] * jm[1][0];
        if det.abs() < THE_SINGULAR_DET_TOL {
            let grad_u = jm[0][0] * fv[0] + jm[1][0] * fv[1];
            let grad_v = jm[0][1] * fv[0] + jm[1][1] * fv[1];
            let grad_sq = grad_u * grad_u + grad_v * grad_v;
            if grad_sq < THE_CRITICAL_GRAD_SQ {
                res.status = Status::Singular;
                return res;
            }
            let alpha = cmin(1.0, (f_norm_sq / grad_sq).sqrt() * 0.1);
            du = -alpha * grad_u;
            dv = -alpha * grad_v;
        } else {
            let inv_det = 1.0 / det;
            du = (-fv[0] * jm[1][1] + fv[1] * jm[0][1]) * inv_det;
            dv = (-fv[1] * jm[0][0] + fv[0] * jm[1][0]) * inv_det;
        }
        let step_norm_sq = du * du + dv * dv;
        let step_norm = step_norm_sq.sqrt();
        if step_norm > max_step {
            let scale = max_step / step_norm;
            du *= scale;
            dv *= scale;
        }
        let mut new_x = [res.x[0] + du, res.x[1] + dv];
        clamp_2d(&mut new_x, bounds, false, 0.0);
        res.step_norm =
            ((new_x[0] - res.x[0]) * (new_x[0] - res.x[0]) + (new_x[1] - res.x[1]) * (new_x[1] - res.x[1])).sqrt();
        res.x = new_x;
        let scale_ref = cmax(1.0, cmax(res.x[0].abs(), res.x[1].abs()));
        if res.step_norm <= options.base.x_tolerance * scale_ref {
            let mut check_f = [0.0; 2];
            let mut check_j = [[0.0; 2]; 2];
            if !f.eval(res.x[0], res.x[1], &mut check_f, &mut check_j) {
                res.status = Status::NumericalError;
                return res;
            }
            res.residual_norm = (check_f[0] * check_f[0] + check_f[1] * check_f[1]).sqrt();
            res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
            return res;
        }
    }
    let mut fv = [0.0; 2];
    let mut jm = [[0.0; 2]; 2];
    if !f.eval(res.x[0], res.x[1], &mut fv, &mut jm) {
        res.status = Status::NumericalError;
        return res;
    }
    res.residual_norm = (fv[0] * fv[0] + fv[1] * fv[1]).sqrt();
    res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
    res
}

/// `MathSys::Solve2DSymmetric`.
pub fn solve2d_symmetric<F: SymmetricFunction2D + ?Sized>(
    f: &mut F,
    x0: &[f64; 2],
    bounds: &NewtonBoundsN<2>,
    options: &NewtonOptions,
) -> NewtonResultN<2> {
    let mut res = NewtonResultN::<2> { x: *x0, ..NewtonResultN::default() };
    if !is_options_valid(options) || !is_bounds_valid_2d(bounds) {
        res.status = Status::InvalidInput;
        return res;
    }
    clamp_2d(&mut res.x, bounds, options.allow_soft_bounds, options.soft_bounds_extension);
    let tol_sq = options.base.f_tolerance * options.base.f_tolerance;
    let max_step = options.max_step_ratio * max_domain_size_2d(bounds);
    for iter in 0..options.base.max_iterations {
        res.nb_iterations = (iter + 1) as usize;
        let Some((f1, f2, j11, j12, j22)) = f.value_and_jacobian(res.x[0], res.x[1]) else {
            res.status = Status::NumericalError;
            return res;
        };
        let f_norm_sq = f1 * f1 + f2 * f2;
        res.residual_norm = f_norm_sq.sqrt();
        if f_norm_sq <= tol_sq {
            res.status = Status::Ok;
            return res;
        }
        let (mut du, mut dv);
        let det = j11 * j22 - j12 * j12;
        if det.abs() < THE_SINGULAR_DET_TOL {
            if let Some((a, b)) = solve_symmetric_2x2_svd(j11, j12, j22, f1, f2, 1.0e-15) {
                du = a;
                dv = b;
            } else {
                let grad_u = j11 * f1 + j12 * f2;
                let grad_v = j12 * f1 + j22 * f2;
                let grad_sq = grad_u * grad_u + grad_v * grad_v;
                if grad_sq < THE_CRITICAL_GRAD_SQ {
                    res.status = Status::Singular;
                    return res;
                }
                let alpha = cmin(1.0, (f_norm_sq / grad_sq).sqrt() * 0.1);
                du = -alpha * grad_u;
                dv = -alpha * grad_v;
            }
        } else {
            let inv_det = 1.0 / det;
            du = (-f1 * j22 + f2 * j12) * inv_det;
            dv = (-f2 * j11 + f1 * j12) * inv_det;
        }
        let step_norm_sq = du * du + dv * dv;
        let step_norm = step_norm_sq.sqrt();
        if step_norm > max_step {
            let scale = max_step / step_norm;
            du *= scale;
            dv *= scale;
        }
        let grad_phi_u = j11 * f1 + j12 * f2;
        let grad_phi_v = j12 * f1 + j22 * f2;
        let dir_deriv = grad_phi_u * du + grad_phi_v * dv;
        let mut new_x = res.x;
        let mut new_residual_norm = -1.0;
        let soft_ext = if options.allow_soft_bounds { options.soft_bounds_extension } else { 0.0 };
        if options.enable_line_search {
            if dir_deriv >= 0.0 {
                res.status = Status::NonDescentDirection;
                return res;
            }
            let phi0 = 0.5 * f_norm_sq;
            let mut alpha = 1.0;
            let mut is_accepted = false;
            for _ in 0..THE_LINE_SEARCH_MAX {
                new_x[0] = res.x[0] + alpha * du;
                new_x[1] = res.x[1] + alpha * dv;
                clamp_2d(&mut new_x, bounds, options.allow_soft_bounds, soft_ext);
                let Some((try_f1, try_f2)) = f.value(new_x[0], new_x[1]) else {
                    alpha *= 0.5;
                    continue;
                };
                let try_f_norm_sq = try_f1 * try_f1 + try_f2 * try_f2;
                let try_phi = 0.5 * try_f_norm_sq;
                let armijo_bound = phi0 + THE_ARMIJO_C1 * alpha * dir_deriv;
                if try_phi <= armijo_bound {
                    is_accepted = true;
                    new_residual_norm = try_f_norm_sq.sqrt();
                    break;
                }
                alpha *= 0.5;
            }
            if !is_accepted {
                res.status = Status::NonDescentDirection;
                return res;
            }
        } else {
            new_x[0] += du;
            new_x[1] += dv;
            clamp_2d(&mut new_x, bounds, options.allow_soft_bounds, soft_ext);
        }
        res.step_norm =
            ((new_x[0] - res.x[0]) * (new_x[0] - res.x[0]) + (new_x[1] - res.x[1]) * (new_x[1] - res.x[1])).sqrt();
        res.x = new_x;
        if new_residual_norm >= 0.0 {
            res.residual_norm = new_residual_norm;
        }
        let scale_ref = cmax(1.0, cmax(res.x[0].abs(), res.x[1].abs()));
        if res.step_norm <= options.base.x_tolerance * scale_ref {
            let Some((c1, c2)) = f.value(res.x[0], res.x[1]) else {
                res.status = Status::NumericalError;
                return res;
            };
            res.residual_norm = (c1 * c1 + c2 * c2).sqrt();
            res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
            return res;
        }
    }
    let Some((f1, f2)) = f.value(res.x[0], res.x[1]) else {
        res.status = Status::NumericalError;
        return res;
    };
    res.residual_norm = (f1 * f1 + f2 * f2).sqrt();
    res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
    res
}

/// `MathSys::Solve3D`.
pub fn solve3d<F: Function3D + ?Sized>(
    f: &mut F,
    x0: &[f64; 3],
    bounds: &NewtonBoundsN<3>,
    options: &NewtonOptions,
) -> NewtonResultN<3> {
    let mut res = NewtonResultN::<3> { x: *x0, ..NewtonResultN::default() };
    if !is_options_valid_3d(options) || !is_bounds_valid_3d(bounds) {
        res.status = Status::InvalidInput;
        return res;
    }
    clamp_3d(&mut res.x, bounds, options.allow_soft_bounds, options.soft_bounds_extension);
    let tol_sq = options.base.f_tolerance * options.base.f_tolerance;
    let max_step = options.max_step_ratio * max_domain_size_3d(bounds);
    for iter in 0..options.base.max_iterations {
        res.nb_iterations = (iter + 1) as usize;
        let mut fv = [0.0; 3];
        let mut jm = [[0.0; 3]; 3];
        if !f.eval(res.x[0], res.x[1], res.x[2], &mut fv, &mut jm) {
            res.status = Status::NumericalError;
            return res;
        }
        let f_norm_sq = fv[0] * fv[0] + fv[1] * fv[1] + fv[2] * fv[2];
        res.residual_norm = f_norm_sq.sqrt();
        if f_norm_sq <= tol_sq {
            res.status = Status::Ok;
            return res;
        }
        let mut delta = match solve3x3(&jm, &fv) {
            Some(d) => d,
            None => {
                let grad_x = jm[0][0] * fv[0] + jm[1][0] * fv[1] + jm[2][0] * fv[2];
                let grad_y = jm[0][1] * fv[0] + jm[1][1] * fv[1] + jm[2][1] * fv[2];
                let grad_z = jm[0][2] * fv[0] + jm[1][2] * fv[1] + jm[2][2] * fv[2];
                let grad_sq = grad_x * grad_x + grad_y * grad_y + grad_z * grad_z;
                if grad_sq < 1.0e-60 {
                    res.status = Status::Singular;
                    return res;
                }
                let alpha = cmin(1.0, (f_norm_sq / grad_sq).sqrt() * 0.1);
                [-alpha * grad_x, -alpha * grad_y, -alpha * grad_z]
            }
        };
        let step_norm_sq = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
        let step_norm = step_norm_sq.sqrt();
        if step_norm > max_step {
            let scale = max_step / step_norm;
            delta[0] *= scale;
            delta[1] *= scale;
            delta[2] *= scale;
        }
        let mut new_x = [res.x[0] + delta[0], res.x[1] + delta[1], res.x[2] + delta[2]];
        clamp_3d(&mut new_x, bounds, options.allow_soft_bounds, options.soft_bounds_extension);
        res.step_norm = ((new_x[0] - res.x[0]) * (new_x[0] - res.x[0])
            + (new_x[1] - res.x[1]) * (new_x[1] - res.x[1])
            + (new_x[2] - res.x[2]) * (new_x[2] - res.x[2]))
            .sqrt();
        res.x = new_x;
        let scale_ref = cmax(1.0, cmax(res.x[0].abs(), cmax(res.x[1].abs(), res.x[2].abs())));
        if res.step_norm <= options.base.x_tolerance * scale_ref {
            let mut check_f = [0.0; 3];
            let mut check_j = [[0.0; 3]; 3];
            if !f.eval(res.x[0], res.x[1], res.x[2], &mut check_f, &mut check_j) {
                res.status = Status::NumericalError;
                return res;
            }
            res.residual_norm = (check_f[0] * check_f[0] + check_f[1] * check_f[1] + check_f[2] * check_f[2]).sqrt();
            res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
            return res;
        }
    }
    let mut fv = [0.0; 3];
    let mut jm = [[0.0; 3]; 3];
    if !f.eval(res.x[0], res.x[1], res.x[2], &mut fv, &mut jm) {
        res.status = Status::NumericalError;
        return res;
    }
    res.residual_norm = (fv[0] * fv[0] + fv[1] * fv[1] + fv[2] * fv[2]).sqrt();
    res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
    res
}

/// `MathSys::Solve4D`.
pub fn solve4d<F: Function4D + ?Sized>(
    f: &mut F,
    x0: &[f64; 4],
    bounds: &NewtonBoundsN<4>,
    options: &NewtonOptions,
) -> NewtonResultN<4> {
    let mut res = NewtonResultN::<4> { x: *x0, ..NewtonResultN::default() };
    if !is_options_valid_4d(options) || !is_bounds_valid_4d(bounds) {
        res.status = Status::InvalidInput;
        return res;
    }
    clamp_4d(&mut res.x, bounds, options.allow_soft_bounds, options.soft_bounds_extension);
    let tol_sq = options.base.f_tolerance * options.base.f_tolerance;
    let max_step = options.max_step_ratio * max_domain_size_4d(bounds);
    for iter in 0..options.base.max_iterations {
        res.nb_iterations = (iter + 1) as usize;
        let mut fv = [0.0; 4];
        let mut jm = [[0.0; 4]; 4];
        if !f.eval(res.x[0], res.x[1], res.x[2], res.x[3], &mut fv, &mut jm) {
            res.status = Status::NumericalError;
            return res;
        }
        let f_norm_sq = fv[0] * fv[0] + fv[1] * fv[1] + fv[2] * fv[2] + fv[3] * fv[3];
        res.residual_norm = f_norm_sq.sqrt();
        if f_norm_sq <= tol_sq {
            res.status = Status::Ok;
            return res;
        }
        let mut delta = match solve4x4(&jm, &fv) {
            Some(d) => d,
            None => {
                let mut grad = [0.0_f64; 4];
                for (c, g) in grad.iter_mut().enumerate() {
                    for (row, fr) in jm.iter().zip(&fv) {
                        *g += row.get(c).copied().unwrap_or(0.0) * *fr;
                    }
                }
                let grad_sq = grad[0] * grad[0] + grad[1] * grad[1] + grad[2] * grad[2] + grad[3] * grad[3];
                if grad_sq < 1.0e-60 {
                    res.status = Status::Singular;
                    return res;
                }
                let alpha = cmin(1.0, (f_norm_sq / grad_sq).sqrt() * 0.1);
                let mut d = [0.0_f64; 4];
                for (di, gi) in d.iter_mut().zip(&grad) {
                    *di = -alpha * *gi;
                }
                d
            }
        };
        let step_norm_sq = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2] + delta[3] * delta[3];
        let step_norm = step_norm_sq.sqrt();
        if step_norm > max_step {
            let scale = max_step / step_norm;
            for d in &mut delta {
                *d *= scale;
            }
        }
        let mut new_x = [res.x[0] + delta[0], res.x[1] + delta[1], res.x[2] + delta[2], res.x[3] + delta[3]];
        clamp_4d(&mut new_x, bounds, options.allow_soft_bounds, options.soft_bounds_extension);
        res.step_norm = ((new_x[0] - res.x[0]) * (new_x[0] - res.x[0])
            + (new_x[1] - res.x[1]) * (new_x[1] - res.x[1])
            + (new_x[2] - res.x[2]) * (new_x[2] - res.x[2])
            + (new_x[3] - res.x[3]) * (new_x[3] - res.x[3]))
            .sqrt();
        res.x = new_x;
        let scale_ref = cmax(1.0, cmax(res.x[0].abs(), cmax(res.x[1].abs(), cmax(res.x[2].abs(), res.x[3].abs()))));
        if res.step_norm <= options.base.x_tolerance * scale_ref {
            let mut check_f = [0.0; 4];
            let mut check_j = [[0.0; 4]; 4];
            if !f.eval(res.x[0], res.x[1], res.x[2], res.x[3], &mut check_f, &mut check_j) {
                res.status = Status::NumericalError;
                return res;
            }
            res.residual_norm =
                (check_f[0] * check_f[0] + check_f[1] * check_f[1] + check_f[2] * check_f[2] + check_f[3] * check_f[3])
                    .sqrt();
            res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
            return res;
        }
    }
    let mut fv = [0.0; 4];
    let mut jm = [[0.0; 4]; 4];
    if !f.eval(res.x[0], res.x[1], res.x[2], res.x[3], &mut fv, &mut jm) {
        res.status = Status::NumericalError;
        return res;
    }
    res.residual_norm = (fv[0] * fv[0] + fv[1] * fv[1] + fv[2] * fv[2] + fv[3] * fv[3]).sqrt();
    res.status = if res.residual_norm <= options.base.f_tolerance { Status::Ok } else { Status::MaxIterations };
    res
}

/// A curve evaluator with second derivative (C++ `D2(t, P, D1, D2)`): `(P, D1, D2)`.
pub trait CurveEvaluator {
    fn d2(&self, t: f64) -> (Pnt, Vec3, Vec3);
}

/// A surface evaluator with second derivatives (C++ `D2(u, v, P, D1U, D1V, D2UU, D2VV, D2UV)`):
/// `(P, D1U, D1V, D2UU, D2VV, D2UV)`.
pub trait SurfaceEvaluator {
    fn d2(&self, u: f64, v: f64) -> (Pnt, Vec3, Vec3, Vec3, Vec3, Vec3);
}

/// `MathSys::SolveCurveSurfaceExtrema3D`: the unknowns are `(t, u, v)`.
pub fn solve_curve_surface_extrema_3d<C: CurveEvaluator + ?Sized, S: SurfaceEvaluator + ?Sized>(
    curve: &C,
    surface: &S,
    x0: &[f64; 3],
    bounds: &NewtonBoundsN<3>,
    options: &NewtonOptions,
) -> NewtonResultN<3> {
    let mut func = |t: f64, u: f64, v: f64, f: &mut [f64; 3], j: &mut [[f64; 3]; 3]| -> bool {
        let (pt_c, d1c, d2c) = curve.d2(t);
        let (pt_s, d1u, d1v, d2uu, d2vv, d2uv) = surface.d2(u, v);
        let d = Vec3::between(&pt_s, &pt_c);
        f[0] = d.dot(&d1c);
        f[1] = -d.dot(&d1u);
        f[2] = -d.dot(&d1v);
        j[0][0] = d1c.dot(&d1c) + d.dot(&d2c);
        j[0][1] = -d1u.dot(&d1c);
        j[0][2] = -d1v.dot(&d1c);
        j[1][0] = j[0][1];
        j[1][1] = d1u.dot(&d1u) - d.dot(&d2uu);
        j[1][2] = d1v.dot(&d1u) - d.dot(&d2uv);
        j[2][0] = j[0][2];
        j[2][1] = j[1][2];
        j[2][2] = d1v.dot(&d1v) - d.dot(&d2vv);
        true
    };
    solve3d(&mut func, x0, bounds, options)
}

/// `MathSys::SolveSurfaceSurfaceExtrema4D`: the unknowns are `(u1, v1, u2, v2)`.
pub fn solve_surface_surface_extrema_4d<S1: SurfaceEvaluator + ?Sized, S2: SurfaceEvaluator + ?Sized>(
    surf1: &S1,
    surf2: &S2,
    x0: &[f64; 4],
    bounds: &NewtonBoundsN<4>,
    options: &NewtonOptions,
) -> NewtonResultN<4> {
    let mut func = |u1: f64, v1: f64, u2: f64, v2: f64, f: &mut [f64; 4], j: &mut [[f64; 4]; 4]| -> bool {
        let (pt1, d1u1, d1v1, d2uu1, d2vv1, d2uv1) = surf1.d2(u1, v1);
        let (pt2, d1u2, d1v2, d2uu2, d2vv2, d2uv2) = surf2.d2(u2, v2);
        let d = Vec3::between(&pt2, &pt1);
        f[0] = d.dot(&d1u1);
        f[1] = d.dot(&d1v1);
        f[2] = -d.dot(&d1u2);
        f[3] = -d.dot(&d1v2);
        j[0][0] = d1u1.dot(&d1u1) + d.dot(&d2uu1);
        j[0][1] = d1v1.dot(&d1u1) + d.dot(&d2uv1);
        j[1][0] = j[0][1];
        j[1][1] = d1v1.dot(&d1v1) + d.dot(&d2vv1);
        j[0][2] = -d1u2.dot(&d1u1);
        j[0][3] = -d1v2.dot(&d1u1);
        j[1][2] = -d1u2.dot(&d1v1);
        j[1][3] = -d1v2.dot(&d1v1);
        j[2][0] = j[0][2];
        j[2][1] = j[1][2];
        j[3][0] = j[0][3];
        j[3][1] = j[1][3];
        j[2][2] = d1u2.dot(&d1u2) - d.dot(&d2uu2);
        j[2][3] = d1v2.dot(&d1u2) - d.dot(&d2uv2);
        j[3][2] = j[2][3];
        j[3][3] = d1v2.dot(&d1v2) - d.dot(&d2vv2);
        true
    };
    solve4d(&mut func, x0, bounds, options)
}
