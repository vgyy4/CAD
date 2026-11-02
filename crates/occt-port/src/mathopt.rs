//! OCCT's header-only `MathOpt` package: scalar and multivariate minimisers (Brent, Golden, Powell, conjugate
//! gradients, BFGS, L-BFGS, Newton), the Uzawa constrained solver, and the global strategies (particle swarm,
//! differential evolution, multi-start).
//!
//! Translated from `MathOpt_*.hxx` (OCCT 8.0.1). The C++ templates over a `Function` type become generics over the
//! callback traits of `mathutils` (and [`VectorFunctionWithHessian`] for the Newton family). Vectors and matrices are
//! `mathlin::{Vector, Matrix}` with OCCT's absolute indices; `Err` appears exactly where the C++ code throws.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::mathlin::{MathError, Matrix, Vector};
use crate::mathlin_pkg;
use crate::mathutils::{
    self, Config, MinBracketOptions, NDimConfig, RandomGenerator, ScalarFunction, ScalarResult, Status, VectorFunction,
    VectorFunctionWithGradient, VectorResult,
};

type R<T> = Result<T, MathError>;

/// `std::max(a, b)`.
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `std::min(a, b)`.
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Sum of the squares of the items, in index order.
fn sum_sq(v: &Vector) -> R<f64> {
    let mut s = 0.0;
    for i in v.lower()..=v.upper() {
        s += mathutils::sqr(v.value(i)?);
    }
    Ok(s)
}

/// `dir(i) = -g(i)` over the range of `dir`.
fn set_neg(dir: &mut Vector, g: &Vector) -> R<()> {
    for i in dir.lower()..=dir.upper() {
        dir.set_value(i, -g.value(i)?)?;
    }
    Ok(())
}

/// The largest `|a(i) - b(i)|` over the range of `a`.
fn max_abs_diff(a: &Vector, b: &Vector) -> R<f64> {
    let mut m = 0.0;
    for i in a.lower()..=a.upper() {
        m = cmax(m, (a.value(i)? - b.value(i)?).abs());
    }
    Ok(m)
}

/// `a = b` of two `math_Vector`s: the lengths must agree, the items are copied by position.
fn assign(dst: &mut Vector, src: &Vector) -> R<()> {
    if dst.length() != src.length() {
        return Err(MathError::Dimension);
    }
    for k in 0..dst.length() {
        dst.set_value(dst.lower() + k, src.value(src.lower() + k)?)?;
    }
    Ok(())
}

/// `x = x + alpha * dir` over the range of `x`.
fn axpy(x: &mut Vector, alpha: f64, dir: &Vector) -> R<()> {
    for j in x.lower()..=x.upper() {
        x.set_value(j, x.value(j)? + alpha * dir.value(j)?)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Callbacks of the derivative-based solvers
// ---------------------------------------------------------------------------------------------

/// A function with a gradient and a Hessian (C++ `bool Hessian(const math_Vector&, math_Matrix&)`).
pub trait VectorFunctionWithHessian: VectorFunctionWithGradient {
    /// Fills `h` (n x n, the same lower bound in rows and columns as the caller's matrix) with the Hessian at `x`.
    fn hessian(&mut self, x: &Vector, h: &mut Matrix) -> bool;
}

/// Internal view of a function with a gradient whose gradient evaluation may itself throw (numerical wrappers).
trait GradFn: VectorFunction {
    fn grad(&mut self, x: &Vector, g: &mut Vector) -> R<bool>;
}

/// Internal view of a function with a gradient and a Hessian.
trait HessFn: GradFn {
    fn hess(&mut self, x: &Vector, h: &mut Matrix) -> R<bool>;
}

/// A user function with analytic derivatives.
struct Plain<'a, F: ?Sized>(&'a mut F);

impl<F: VectorFunction + ?Sized> VectorFunction for Plain<'_, F> {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.0.value(x)
    }
}
impl<F: VectorFunctionWithGradient + ?Sized> GradFn for Plain<'_, F> {
    fn grad(&mut self, x: &Vector, g: &mut Vector) -> R<bool> {
        Ok(self.0.gradient(x, g))
    }
}
impl<F: VectorFunctionWithHessian + ?Sized> HessFn for Plain<'_, F> {
    fn hess(&mut self, x: &Vector, h: &mut Matrix) -> R<bool> {
        Ok(self.0.hessian(x, h))
    }
}

/// `FuncWithGradient` of `BFGSNumerical`/`FRPRNumerical`: the gradient comes from `NumericalGradientAdaptive`.
struct NumGrad<'a, F: ?Sized> {
    f: &'a mut F,
    step: f64,
}
impl<F: VectorFunction + ?Sized> VectorFunction for NumGrad<'_, F> {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.f.value(x)
    }
}
impl<F: VectorFunction + ?Sized> GradFn for NumGrad<'_, F> {
    fn grad(&mut self, x: &Vector, g: &mut Vector) -> R<bool> {
        let mut xm = x.clone();
        mathutils::numerical_gradient_adaptive(self.f, &mut xm, g, self.step)
    }
}

/// `FuncWithHessian` of `NewtonNumericalHessian`: analytic gradient, `NumericalHessian`.
struct NumHess<'a, F: ?Sized> {
    f: &'a mut F,
    step: f64,
}
impl<F: VectorFunction + ?Sized> VectorFunction for NumHess<'_, F> {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.f.value(x)
    }
}
impl<F: VectorFunctionWithGradient + ?Sized> GradFn for NumHess<'_, F> {
    fn grad(&mut self, x: &Vector, g: &mut Vector) -> R<bool> {
        Ok(self.f.gradient(x, g))
    }
}
impl<F: VectorFunctionWithGradient + ?Sized> HessFn for NumHess<'_, F> {
    fn hess(&mut self, x: &Vector, h: &mut Matrix) -> R<bool> {
        let mut xm = x.clone();
        mathutils::numerical_hessian(self.f, &mut xm, h, self.step)
    }
}

/// `FuncWithDerivatives` of `NewtonNumerical`: numerical gradient and a Hessian by central differences of it.
struct NumBoth<'a, F: ?Sized> {
    f: &'a mut F,
    grad_step: f64,
    hess_step: f64,
}
impl<F: VectorFunction + ?Sized> VectorFunction for NumBoth<'_, F> {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.f.value(x)
    }
}
impl<F: VectorFunction + ?Sized> GradFn for NumBoth<'_, F> {
    fn grad(&mut self, x: &Vector, g: &mut Vector) -> R<bool> {
        let mut xm = x.clone();
        mathutils::numerical_gradient_adaptive(self.f, &mut xm, g, self.grad_step)
    }
}
impl<F: VectorFunction + ?Sized> HessFn for NumBoth<'_, F> {
    fn hess(&mut self, x: &Vector, h: &mut Matrix) -> R<bool> {
        let (lower, upper) = (x.lower(), x.upper());
        let mut xm = x.clone();
        let mut grad_plus = Vector::new(lower, upper)?;
        let mut grad_minus = Vector::new(lower, upper)?;
        for j in lower..=upper {
            let xj = xm.value(j)?;
            xm.set_value(j, xj + self.hess_step)?;
            if !mathutils::numerical_gradient_adaptive(self.f, &mut xm, &mut grad_plus, self.grad_step)? {
                xm.set_value(j, xj)?;
                return Ok(false);
            }
            xm.set_value(j, xj - self.hess_step)?;
            if !mathutils::numerical_gradient_adaptive(self.f, &mut xm, &mut grad_minus, self.grad_step)? {
                xm.set_value(j, xj)?;
                return Ok(false);
            }
            xm.set_value(j, xj)?;
            for i in lower..=upper {
                h.set_value(i, j, (grad_plus.value(i)? - grad_minus.value(i)?) / (2.0 * self.hess_step))?;
            }
        }
        for i in lower..=upper {
            for j in (i + 1)..=upper {
                let avg = 0.5 * (h.value(i, j)? + h.value(j, i)?);
                h.set_value(i, j, avg)?;
                h.set_value(j, i, avg)?;
            }
        }
        Ok(true)
    }
}

// ---------------------------------------------------------------------------------------------
// MathOpt_Brent.hxx
// ---------------------------------------------------------------------------------------------

/// `MathOpt::Brent`: Brent's minimiser on `[lower, upper]`.
pub fn brent<F: ScalarFunction + ?Sized>(f: &mut F, lower: f64, upper: f64, cfg: &Config) -> ScalarResult {
    let mut result = ScalarResult::default();
    let mut a = lower;
    let mut b = upper;
    let mut x = a + mathutils::THE_GOLDEN_SECTION * (b - a);
    let mut w = x;
    let mut v = x;
    let Some(mut fx) = f.value(x) else {
        result.status = Status::NumericalError;
        return result;
    };
    let mut fw = fx;
    let mut fv = fx;
    let mut d: f64 = 0.0;
    let mut e: f64 = 0.0;
    for iter in 0..cfg.max_iterations {
        let xm = 0.5 * (a + b);
        let tol1 = cfg.x_tolerance * x.abs() + mathutils::THE_ZERO_TOL / 10.0;
        let tol2 = 2.0 * tol1;
        result.nb_iterations = (iter + 1) as usize;
        if (x - xm).abs() <= (tol2 - 0.5 * (b - a)) {
            result.status = Status::Ok;
            result.root = Some(x);
            result.value = Some(fx);
            return result;
        }
        let mut use_parabolic = false;
        if e.abs() > tol1 {
            let r = (x - w) * (fx - fv);
            let mut q = (x - v) * (fx - fw);
            let mut p = (x - v) * q - (x - w) * r;
            q = 2.0 * (q - r);
            if q > 0.0 {
                p = -p;
            } else {
                q = -q;
            }
            let e_tmp = e;
            e = d;
            if p.abs() < (0.5 * q * e_tmp).abs() && p > q * (a - x) && p < q * (b - x) {
                d = p / q;
                let u_par = x + d;
                if (u_par - a) < tol2 || (b - u_par) < tol2 {
                    d = mathutils::sign_transfer(tol1, xm - x);
                }
                use_parabolic = true;
            }
        }
        if !use_parabolic {
            e = if x < xm { b - x } else { a - x };
            d = mathutils::THE_GOLDEN_SECTION * e;
        }
        let u = if d.abs() >= tol1 { x + d } else { x + mathutils::sign_transfer(tol1, d) };
        let Some(fu) = f.value(u) else {
            result.status = Status::NumericalError;
            result.root = Some(x);
            result.value = Some(fx);
            return result;
        };
        if fu <= fx {
            if u < x {
                b = x;
            } else {
                a = x;
            }
            v = w;
            w = x;
            x = u;
            fv = fw;
            fw = fx;
            fx = fu;
        } else {
            if u < x {
                a = u;
            } else {
                b = u;
            }
            if fu <= fw || w == x {
                v = w;
                w = u;
                fv = fw;
                fw = fu;
            } else if fu <= fv || v == x || v == w {
                v = u;
                fv = fu;
            }
        }
    }
    result.status = Status::MaxIterations;
    result.root = Some(x);
    result.value = Some(fx);
    result
}

/// `MathOpt::Golden`: golden-section search on `[lower, upper]`.
pub fn golden<F: ScalarFunction + ?Sized>(f: &mut F, lower: f64, upper: f64, cfg: &Config) -> ScalarResult {
    let mut result = ScalarResult::default();
    let r: f64 = 0.618033988749895;
    let c: f64 = 1.0 - r;
    let mut a = lower;
    let mut b = upper;
    let mut x1 = a + c * (b - a);
    let mut x2 = a + r * (b - a);
    let Some(mut f1) = f.value(x1) else {
        result.status = Status::NumericalError;
        return result;
    };
    let Some(mut f2) = f.value(x2) else {
        result.status = Status::NumericalError;
        return result;
    };
    for iter in 0..cfg.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        if (b - a) < cfg.x_tolerance * (x1.abs() + x2.abs()) {
            result.status = Status::Ok;
            if f1 < f2 {
                result.root = Some(x1);
                result.value = Some(f1);
            } else {
                result.root = Some(x2);
                result.value = Some(f2);
            }
            return result;
        }
        if f1 < f2 {
            b = x2;
            x2 = x1;
            f2 = f1;
            x1 = a + c * (b - a);
            match f.value(x1) {
                Some(v) => f1 = v,
                None => {
                    result.status = Status::NumericalError;
                    result.root = Some(x2);
                    result.value = Some(f2);
                    return result;
                }
            }
        } else {
            a = x1;
            x1 = x2;
            f1 = f2;
            x2 = a + r * (b - a);
            match f.value(x2) {
                Some(v) => f2 = v,
                None => {
                    result.status = Status::NumericalError;
                    result.root = Some(x1);
                    result.value = Some(f1);
                    return result;
                }
            }
        }
    }
    result.status = Status::MaxIterations;
    if f1 < f2 {
        result.root = Some(x1);
        result.value = Some(f1);
    } else {
        result.root = Some(x2);
        result.value = Some(f2);
    }
    result
}

/// `MathOpt::BrentWithBracket`: brackets a minimum from `guess` and `guess + step`, then runs [`brent`] on it.
pub fn brent_with_bracket<F: ScalarFunction + ?Sized>(f: &mut F, guess: f64, step: f64, cfg: &Config) -> ScalarResult {
    let mut result = ScalarResult::default();
    let bracket = mathutils::bracket_minimum(f, guess, guess + step, &MinBracketOptions::default());
    if !bracket.is_valid {
        result.status = Status::InvalidInput;
        return result;
    }
    brent(f, bracket.a, bracket.c, cfg)
}

// ---------------------------------------------------------------------------------------------
// MathOpt_Powell.hxx
// ---------------------------------------------------------------------------------------------

fn powell_core<F: VectorFunction + ?Sized>(
    f: &mut F,
    start: &Vector,
    mut dirs: Matrix,
    cfg: &Config,
) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let (lower, upper) = (start.lower(), start.upper());
    let n = upper - lower + 1;
    let mut x = start.clone();
    let Some(mut fx) = f.value(&x) else {
        result.status = Status::NumericalError;
        return Ok(result);
    };
    let mut dir = Vector::new(lower, upper)?;
    let mut ptt = Vector::new(lower, upper)?;
    let mut xit = Vector::new(lower, upper)?;
    for iter in 0..cfg.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        let fp = fx;
        let x_old = x.clone();
        let mut del = 0.0;
        let mut ibig = 0;
        for i in 1..=n {
            for j in lower..=upper {
                dir.set_value(j, dirs.value(i, j - lower + 1)?)?;
            }
            let fp_prev = fx;
            let line = mathutils::exact_line_search(f, &x, &dir, 10.0, cfg.x_tolerance, 100)?;
            if line.is_valid {
                axpy(&mut x, line.alpha, &dir)?;
                fx = line.f_new;
                let decrease = fp_prev - fx;
                if decrease > del {
                    del = decrease;
                    ibig = i;
                }
            }
        }
        if 2.0 * (fp - fx).abs() <= cfg.f_tolerance * (fp.abs() + fx.abs() + mathutils::THE_ZERO_TOL) {
            result.status = Status::Ok;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
        for j in lower..=upper {
            ptt.set_value(j, 2.0 * x.value(j)? - x_old.value(j)?)?;
            xit.set_value(j, x.value(j)? - x_old.value(j)?)?;
        }
        let Some(fptt) = f.value(&ptt) else {
            continue;
        };
        if fptt < fp {
            let t = 2.0 * (fp - 2.0 * fx + fptt) * mathutils::sqr(fp - fx - del) - del * mathutils::sqr(fp - fptt);
            if t < 0.0 {
                let line = mathutils::exact_line_search(f, &x, &xit, 10.0, cfg.x_tolerance, 100)?;
                if line.is_valid {
                    axpy(&mut x, line.alpha, &xit)?;
                    fx = line.f_new;
                    if ibig > 0 {
                        for j in 1..=n {
                            dirs.set_value(ibig, j, dirs.value(n, j)?)?;
                            dirs.set_value(n, j, xit.value(lower + j - 1)?)?;
                        }
                    }
                }
            }
        }
        let mut max_diff = 0.0;
        for j in lower..=upper {
            max_diff = cmax(max_diff, (x.value(j)? - x_old.value(j)?).abs());
        }
        if max_diff < cfg.x_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
    }
    result.status = Status::MaxIterations;
    result.solution = Some(x);
    result.value = Some(fx);
    Ok(result)
}

/// `MathOpt::Powell`: Powell's direction-set method from the coordinate directions.
pub fn powell<F: VectorFunction + ?Sized>(f: &mut F, start: &Vector, cfg: &Config) -> R<VectorResult> {
    let n = start.upper() - start.lower() + 1;
    let mut dirs = Matrix::filled(1, n, 1, n, 0.0)?;
    for i in 1..=n {
        dirs.set_value(i, i, 1.0)?;
    }
    powell_core(f, start, dirs, cfg)
}

/// `MathOpt::PowellWithDirections`: Powell's method from the rows of `initial_directions` (n x n, else `InvalidInput`).
pub fn powell_with_directions<F: VectorFunction + ?Sized>(
    f: &mut F,
    start: &Vector,
    initial_directions: &Matrix,
    cfg: &Config,
) -> R<VectorResult> {
    let n = start.upper() - start.lower() + 1;
    if initial_directions.row_number() != n || initial_directions.col_number() != n {
        return Ok(VectorResult { status: Status::InvalidInput, ..VectorResult::default() });
    }
    let mut dirs = Matrix::new(1, n, 1, n)?;
    for r in 0..n {
        for c in 0..n {
            dirs.set_value(
                1 + r,
                1 + c,
                initial_directions.value(initial_directions.lower_row() + r, initial_directions.lower_col() + c)?,
            )?;
        }
    }
    powell_core(f, start, dirs, cfg)
}

// ---------------------------------------------------------------------------------------------
// MathOpt_FRPR.hxx
// ---------------------------------------------------------------------------------------------

/// `MathOpt::ConjugateGradientFormula`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ConjugateGradientFormula {
    /// `beta = g_new^T g_new / g^T g`.
    FletcherReeves = 0,
    /// `beta = g_new^T (g_new - g) / g^T g`.
    PolakRibiere = 1,
    /// `beta = g_new^T (g_new - g) / d^T (g_new - g)`.
    HestenesStiefel = 2,
    /// `beta = g_new^T g_new / d^T (g_new - g)`.
    DaiYuan = 3,
}

/// `MathOpt::FRPRConfig` (derives from `Config` in C++).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrprConfig {
    /// The `Config` base.
    pub base: Config,
    /// Beta formula [`PolakRibiere`](ConjugateGradientFormula::PolakRibiere).
    pub formula: ConjugateGradientFormula,
    /// Restart every N iterations (0 = the dimension) [0].
    pub restart_interval: i32,
}
impl Default for FrprConfig {
    fn default() -> Self {
        Self { base: Config::default(), formula: ConjugateGradientFormula::PolakRibiere, restart_interval: 0 }
    }
}
impl FrprConfig {
    /// `FRPRConfig(theTolerance, theMaxIter)`; the C++ default for `max_iter` is 100.
    pub fn new(tolerance: f64, max_iter: i32) -> Self {
        Self { base: Config::new(tolerance, max_iter), ..Self::default() }
    }
}

fn frpr_core<G: GradFn + ?Sized>(f: &mut G, start: &Vector, cfg: &FrprConfig) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let (lower, upper) = (start.lower(), start.upper());
    let n = upper - lower + 1;
    let restart_interval = if cfg.restart_interval > 0 { cfg.restart_interval } else { n };
    let mut x = start.clone();
    let Some(mut fx) = f.value(&x) else {
        result.status = Status::NumericalError;
        return Ok(result);
    };
    let mut grad = Vector::new(lower, upper)?;
    if !f.grad(&x, &mut grad)? {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    let mut grad_norm_sq = sum_sq(&grad)?;
    if grad_norm_sq.sqrt() < cfg.base.f_tolerance {
        result.status = Status::Ok;
        result.solution = Some(x);
        result.value = Some(fx);
        result.gradient = Some(grad);
        return Ok(result);
    }
    let mut dir = Vector::new(lower, upper)?;
    set_neg(&mut dir, &grad)?;
    let mut x_new = Vector::new(lower, upper)?;
    let mut grad_new = Vector::new(lower, upper)?;
    let mut grad_diff = Vector::new(lower, upper)?;
    let mut restart_count = 0;
    for iter in 0..cfg.base.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        let mut line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
        if !line.is_valid || line.alpha < mathutils::THE_EPSILON {
            set_neg(&mut dir, &grad)?;
            line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
            if !line.is_valid {
                result.status = Status::NotConverged;
                result.solution = Some(x);
                result.value = Some(fx);
                result.gradient = Some(grad);
                return Ok(result);
            }
            restart_count = 0;
        }
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + line.alpha * dir.value(i)?)?;
        }
        let max_diff = max_abs_diff(&x_new, &x)?;
        if !f.grad(&x_new, &mut grad_new)? {
            result.status = Status::NumericalError;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
        let grad_new_norm_sq = sum_sq(&grad_new)?;
        if grad_new_norm_sq.sqrt() < cfg.base.f_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(line.f_new);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        if max_diff < cfg.base.x_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(line.f_new);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        for i in lower..=upper {
            grad_diff.set_value(i, grad_new.value(i)? - grad.value(i)?)?;
        }
        let mut beta = 0.0;
        restart_count += 1;
        if restart_count >= restart_interval {
            beta = 0.0;
            restart_count = 0;
        } else {
            match cfg.formula {
                ConjugateGradientFormula::FletcherReeves => {
                    if grad_norm_sq > mathutils::THE_ZERO_TOL {
                        beta = grad_new_norm_sq / grad_norm_sq;
                    }
                }
                ConjugateGradientFormula::PolakRibiere => {
                    let mut dot = 0.0;
                    for i in lower..=upper {
                        dot += grad_new.value(i)? * grad_diff.value(i)?;
                    }
                    if grad_norm_sq > mathutils::THE_ZERO_TOL {
                        beta = dot / grad_norm_sq;
                    }
                    if beta < 0.0 {
                        beta = 0.0;
                        restart_count = 0;
                    }
                }
                ConjugateGradientFormula::HestenesStiefel => {
                    let mut num = 0.0;
                    let mut den = 0.0;
                    for i in lower..=upper {
                        num += grad_new.value(i)? * grad_diff.value(i)?;
                        den += dir.value(i)? * grad_diff.value(i)?;
                    }
                    if den.abs() > mathutils::THE_ZERO_TOL {
                        beta = num / den;
                    }
                    if beta < 0.0 {
                        beta = 0.0;
                        restart_count = 0;
                    }
                }
                ConjugateGradientFormula::DaiYuan => {
                    let mut den = 0.0;
                    for i in lower..=upper {
                        den += dir.value(i)? * grad_diff.value(i)?;
                    }
                    if den.abs() > mathutils::THE_ZERO_TOL {
                        beta = grad_new_norm_sq / den;
                    }
                }
            }
        }
        for i in lower..=upper {
            dir.set_value(i, -grad_new.value(i)? + beta * dir.value(i)?)?;
        }
        let mut dir_deriv = 0.0;
        for i in lower..=upper {
            dir_deriv += grad_new.value(i)? * dir.value(i)?;
        }
        if dir_deriv >= 0.0 {
            set_neg(&mut dir, &grad_new)?;
            restart_count = 0;
        }
        x = x_new.clone();
        grad = grad_new.clone();
        grad_norm_sq = grad_new_norm_sq;
        fx = line.f_new;
    }
    result.status = Status::MaxIterations;
    result.solution = Some(x);
    result.value = Some(fx);
    result.gradient = Some(grad);
    Ok(result)
}

/// `MathOpt::FRPR`: Fletcher-Reeves / Polak-Ribiere conjugate gradients with an analytic gradient.
pub fn frpr<F: VectorFunctionWithGradient + ?Sized>(f: &mut F, start: &Vector, cfg: &FrprConfig) -> R<VectorResult> {
    frpr_core(&mut Plain(f), start, cfg)
}

/// `MathOpt::FRPRNumerical`: conjugate gradients with the gradient of `NumericalGradientAdaptive` (`grad_step` [1e-8]).
pub fn frpr_numerical<F: VectorFunction + ?Sized>(
    f: &mut F,
    start: &Vector,
    grad_step: f64,
    cfg: &FrprConfig,
) -> R<VectorResult> {
    frpr_core(&mut NumGrad { f, step: grad_step }, start, cfg)
}

// ---------------------------------------------------------------------------------------------
// MathOpt_BFGS.hxx
// ---------------------------------------------------------------------------------------------

/// The BFGS update of the inverse Hessian approximation `h` (1-based, n x n) with the step `s` and the gradient
/// difference `y` (both 1-based); nothing happens unless `s . y` is positive enough.
fn bfgs_update(h: &mut Matrix, s: &Vector, y: &Vector, n: i32) -> R<()> {
    let mut sy = 0.0;
    for i in 1..=n {
        sy += s.value(i)? * y.value(i)?;
    }
    if sy > mathutils::THE_ZERO_TOL {
        let rho = 1.0 / sy;
        let mut hy = Vector::filled(1, n, 0.0)?;
        for i in 1..=n {
            for j in 1..=n {
                hy.set_value(i, hy.value(i)? + h.value(i, j)? * y.value(j)?)?;
            }
        }
        let mut yhy = 0.0;
        for i in 1..=n {
            yhy += y.value(i)? * hy.value(i)?;
        }
        let factor = 1.0 + rho * yhy;
        for i in 1..=n {
            for j in 1..=n {
                let v = h.value(i, j)? - rho * (hy.value(i)? * s.value(j)? + s.value(i)? * hy.value(j)?)
                    + factor * rho * s.value(i)? * s.value(j)?;
                h.set_value(i, j, v)?;
            }
        }
    }
    Ok(())
}

/// `aDir = -H * grad` (`h` is 1-based, `grad` and `dir` start at `lower`).
fn bfgs_direction(h: &Matrix, grad: &Vector, dir: &mut Vector, n: i32, lower: i32) -> R<()> {
    for i in 1..=n {
        let mut sum = 0.0;
        for j in 1..=n {
            sum += h.value(i, j)? * grad.value(lower + j - 1)?;
        }
        dir.set_value(lower + i - 1, -sum)?;
    }
    Ok(())
}

fn identity(n: i32) -> R<Matrix> {
    let mut h = Matrix::filled(1, n, 1, n, 0.0)?;
    for i in 1..=n {
        h.set_value(i, i, 1.0)?;
    }
    Ok(h)
}

fn reset_identity(h: &mut Matrix, n: i32) -> R<()> {
    for i in 1..=n {
        for j in 1..=n {
            h.set_value(i, j, if i == j { 1.0 } else { 0.0 })?;
        }
    }
    Ok(())
}

fn bfgs_core<G: GradFn + ?Sized>(f: &mut G, start: &Vector, cfg: &Config) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let (lower, upper) = (start.lower(), start.upper());
    let n = upper - lower + 1;
    let mut x = start.clone();
    let Some(mut fx) = f.value(&x) else {
        result.status = Status::NumericalError;
        return Ok(result);
    };
    let mut grad = Vector::new(lower, upper)?;
    if !f.grad(&x, &mut grad)? {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    if sum_sq(&grad)?.sqrt() < cfg.f_tolerance {
        result.status = Status::Ok;
        result.solution = Some(x);
        result.value = Some(fx);
        result.gradient = Some(grad);
        return Ok(result);
    }
    let mut h = identity(n)?;
    let mut dir = Vector::new(lower, upper)?;
    let mut x_new = Vector::new(lower, upper)?;
    let mut grad_new = Vector::new(lower, upper)?;
    let mut s = Vector::new(1, n)?;
    let mut y = Vector::new(1, n)?;
    for iter in 0..cfg.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        bfgs_direction(&h, &grad, &mut dir, n, lower)?;
        let mut line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
        if !line.is_valid || line.alpha < mathutils::THE_EPSILON {
            set_neg(&mut dir, &grad)?;
            line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
            if !line.is_valid {
                result.status = Status::NotConverged;
                result.solution = Some(x);
                result.value = Some(fx);
                result.gradient = Some(grad);
                return Ok(result);
            }
            reset_identity(&mut h, n)?;
        }
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + line.alpha * dir.value(i)?)?;
        }
        for i in 1..=n {
            s.set_value(i, x_new.value(lower + i - 1)? - x.value(lower + i - 1)?)?;
        }
        if !f.grad(&x_new, &mut grad_new)? {
            result.status = Status::NumericalError;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
        if sum_sq(&grad_new)?.sqrt() < cfg.f_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(line.f_new);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        if max_abs_diff(&x_new, &x)? < cfg.x_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(line.f_new);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        for i in 1..=n {
            y.set_value(i, grad_new.value(lower + i - 1)? - grad.value(lower + i - 1)?)?;
        }
        bfgs_update(&mut h, &s, &y, n)?;
        x = x_new.clone();
        grad = grad_new.clone();
        fx = line.f_new;
    }
    result.status = Status::MaxIterations;
    result.solution = Some(x);
    result.value = Some(fx);
    result.gradient = Some(grad);
    Ok(result)
}

/// `MathOpt::BFGS`: BFGS with an analytic gradient.
pub fn bfgs<F: VectorFunctionWithGradient + ?Sized>(f: &mut F, start: &Vector, cfg: &Config) -> R<VectorResult> {
    bfgs_core(&mut Plain(f), start, cfg)
}

/// `MathOpt::BFGSNumerical`: BFGS with the gradient of `NumericalGradientAdaptive` (`grad_step` [1e-8]).
pub fn bfgs_numerical<F: VectorFunction + ?Sized>(
    f: &mut F,
    start: &Vector,
    grad_step: f64,
    cfg: &Config,
) -> R<VectorResult> {
    bfgs_core(&mut NumGrad { f, step: grad_step }, start, cfg)
}

/// `MathOpt::LBFGS`: limited-memory BFGS keeping `memory_size` [10] correction pairs. A `memory_size` of 0 or less
/// divides by zero in C++ (the process dies); here it is `Err(MathError::Construction)`.
pub fn lbfgs<F: VectorFunctionWithGradient + ?Sized>(
    f: &mut F,
    start: &Vector,
    memory_size: i32,
    cfg: &Config,
) -> R<VectorResult> {
    let mut f = Plain(f);
    let f = &mut f;
    let mut result = VectorResult::default();
    let (lower, upper) = (start.lower(), start.upper());
    let n = upper - lower + 1;
    let m = memory_size;
    let mut x = start.clone();
    let Some(mut fx) = f.value(&x) else {
        result.status = Status::NumericalError;
        return Ok(result);
    };
    let mut grad = Vector::new(lower, upper)?;
    if !f.grad(&x, &mut grad)? {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    if m <= 0 {
        return Err(MathError::Construction);
    }
    let mut s_vec = Vec::new();
    let mut y_vec = Vec::new();
    let mut rho_vec = Vector::filled(0, m - 1, 0.0)?;
    for _ in 0..m {
        s_vec.push(Vector::new(1, n)?);
        y_vec.push(Vector::new(1, n)?);
    }
    let mut head = 0;
    let mut count = 0;
    let mut dir = Vector::new(lower, upper)?;
    let mut x_new = Vector::new(lower, upper)?;
    let mut grad_new = Vector::new(lower, upper)?;
    let mut q = Vector::new(1, n)?;
    let mut alpha_vec = Vector::new(0, m - 1)?;
    let at = |v: &[Vector], i: i32| -> R<Vector> { v.get(i as usize).cloned().ok_or(MathError::Range) };
    for iter in 0..cfg.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        if sum_sq(&grad)?.sqrt() < cfg.f_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x);
            result.value = Some(fx);
            result.gradient = Some(grad);
            return Ok(result);
        }
        for i in 1..=n {
            q.set_value(i, grad.value(lower + i - 1)?)?;
        }
        for k in (0..count).rev() {
            let idx = (head + k) % m;
            alpha_vec.set_value(idx, rho_vec.value(idx)? * mathutils::dot_product(&at(&s_vec, idx)?, &q)?)?;
            let yv = at(&y_vec, idx)?;
            for i in 1..=n {
                q.set_value(i, q.value(i)? - alpha_vec.value(idx)? * yv.value(i)?)?;
            }
        }
        let mut gamma = 1.0;
        if count > 0 {
            let last = (head + count - 1) % m;
            let yl = at(&y_vec, last)?;
            let yy = mathutils::dot_product(&yl, &yl)?;
            if yy > mathutils::THE_ZERO_TOL {
                gamma = 1.0 / (rho_vec.value(last)? * yy);
            }
        }
        let mut r = Vector::new(1, n)?;
        for i in 1..=n {
            r.set_value(i, gamma * q.value(i)?)?;
        }
        for k in 0..count {
            let idx = (head + k) % m;
            let beta = rho_vec.value(idx)? * mathutils::dot_product(&at(&y_vec, idx)?, &r)?;
            let sv = at(&s_vec, idx)?;
            for i in 1..=n {
                r.set_value(i, r.value(i)? + (alpha_vec.value(idx)? - beta) * sv.value(i)?)?;
            }
        }
        for i in lower..=upper {
            dir.set_value(i, -r.value(i - lower + 1)?)?;
        }
        let mut line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
        if !line.is_valid {
            set_neg(&mut dir, &grad)?;
            line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
            if !line.is_valid {
                result.status = Status::NotConverged;
                result.solution = Some(x);
                result.value = Some(fx);
                return Ok(result);
            }
            count = 0;
        }
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + line.alpha * dir.value(i)?)?;
        }
        if max_abs_diff(&x_new, &x)? < cfg.x_tolerance {
            if !f.grad(&x_new, &mut grad_new)? {
                grad_new = grad.clone();
            }
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(line.f_new);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        if !f.grad(&x_new, &mut grad_new)? {
            result.status = Status::NumericalError;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
        let new_idx = (head + count) % m;
        {
            let (sv, yv) = (
                s_vec.get_mut(new_idx as usize).ok_or(MathError::Range)?,
                y_vec.get_mut(new_idx as usize).ok_or(MathError::Range)?,
            );
            for i in 1..=n {
                sv.set_value(i, x_new.value(lower + i - 1)? - x.value(lower + i - 1)?)?;
                yv.set_value(i, grad_new.value(lower + i - 1)? - grad.value(lower + i - 1)?)?;
            }
        }
        let sy = mathutils::dot_product(&at(&s_vec, new_idx)?, &at(&y_vec, new_idx)?)?;
        if sy > mathutils::THE_ZERO_TOL {
            rho_vec.set_value(new_idx, 1.0 / sy)?;
            if count < m {
                count += 1;
            } else {
                head = (head + 1) % m;
            }
        }
        x = x_new.clone();
        grad = grad_new.clone();
        fx = line.f_new;
    }
    result.status = Status::MaxIterations;
    result.solution = Some(x);
    result.value = Some(fx);
    result.gradient = Some(grad);
    Ok(result)
}

/// The bounds helpers shared by `BFGSBounded` and `NewtonBounded` (`theLowerBounds.Lower() + (i - lower)` indexes
/// both bound vectors, as in C++).
struct Bounds<'a> {
    lo: &'a Vector,
    hi: &'a Vector,
    lower: i32,
    upper: i32,
}
impl Bounds<'_> {
    fn clamp(&self, x: &mut Vector) -> R<()> {
        for i in self.lower..=self.upper {
            let b = self.lo.lower() + (i - self.lower);
            if x.value(i)? < self.lo.value(b)? {
                x.set_value(i, self.lo.value(b)?)?;
            }
            if x.value(i)? > self.hi.value(b)? {
                x.set_value(i, self.hi.value(b)?)?;
            }
        }
        Ok(())
    }
    fn project_gradient(&self, x: &Vector, grad: &mut Vector) -> R<()> {
        for i in self.lower..=self.upper {
            let b = self.lo.lower() + (i - self.lower);
            let tol = mathutils::THE_EPSILON * cmax(1.0, x.value(i)?.abs());
            if x.value(i)? - self.lo.value(b)? < tol && grad.value(i)? > 0.0 {
                grad.set_value(i, 0.0)?;
            }
            if self.hi.value(b)? - x.value(i)? < tol && grad.value(i)? < 0.0 {
                grad.set_value(i, 0.0)?;
            }
        }
        Ok(())
    }
    /// The largest step (at most 1, at least `THE_EPSILON`) keeping `x + alpha * dir` inside the bounds.
    fn alpha_max(&self, x: &Vector, dir: &Vector) -> R<f64> {
        let mut alpha_max = 1.0;
        for i in self.lower..=self.upper {
            let b = self.lo.lower() + (i - self.lower);
            if dir.value(i)? < -mathutils::THE_EPSILON {
                let max_step = (self.lo.value(b)? - x.value(i)?) / dir.value(i)?;
                alpha_max = cmin(alpha_max, max_step);
            } else if dir.value(i)? > mathutils::THE_EPSILON {
                let max_step = (self.hi.value(b)? - x.value(i)?) / dir.value(i)?;
                alpha_max = cmin(alpha_max, max_step);
            }
        }
        Ok(cmax(alpha_max, mathutils::THE_EPSILON))
    }
}

/// `MathOpt::BFGSBounded`: projected BFGS inside the box `[lower_bounds, upper_bounds]`.
pub fn bfgs_bounded<F: VectorFunctionWithGradient + ?Sized>(
    f: &mut F,
    start: &Vector,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &Config,
) -> R<VectorResult> {
    let mut f = Plain(f);
    let f = &mut f;
    let mut result = VectorResult::default();
    let (lower, upper) = (start.lower(), start.upper());
    let n = upper - lower + 1;
    if lower_bounds.length() != n || upper_bounds.length() != n {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let bounds = Bounds { lo: lower_bounds, hi: upper_bounds, lower, upper };
    let mut x = start.clone();
    bounds.clamp(&mut x)?;
    let Some(mut fx) = f.value(&x) else {
        result.status = Status::NumericalError;
        return Ok(result);
    };
    let mut grad = Vector::new(lower, upper)?;
    if !f.grad(&x, &mut grad)? {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    bounds.project_gradient(&x, &mut grad)?;
    if sum_sq(&grad)?.sqrt() < cfg.f_tolerance {
        result.status = Status::Ok;
        result.solution = Some(x);
        result.value = Some(fx);
        result.gradient = Some(grad);
        return Ok(result);
    }
    let mut h = identity(n)?;
    let mut dir = Vector::new(lower, upper)?;
    let mut x_new = Vector::new(lower, upper)?;
    let mut grad_new = Vector::new(lower, upper)?;
    let mut s = Vector::new(1, n)?;
    let mut y = Vector::new(1, n)?;
    for iter in 0..cfg.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        bfgs_direction(&h, &grad, &mut dir, n, lower)?;
        let mut alpha_max = bounds.alpha_max(&x, &dir)?;
        let mut line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, alpha_max, 1.0e-4, 0.5, 50)?;
        if !line.is_valid || line.alpha < mathutils::THE_EPSILON {
            set_neg(&mut dir, &grad)?;
            alpha_max = bounds.alpha_max(&x, &dir)?;
            line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, alpha_max, 1.0e-4, 0.5, 50)?;
            if !line.is_valid {
                result.status = Status::NotConverged;
                result.solution = Some(x);
                result.value = Some(fx);
                result.gradient = Some(grad);
                return Ok(result);
            }
            reset_identity(&mut h, n)?;
        }
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + line.alpha * dir.value(i)?)?;
        }
        bounds.clamp(&mut x_new)?;
        for i in 1..=n {
            s.set_value(i, x_new.value(lower + i - 1)? - x.value(lower + i - 1)?)?;
        }
        let Some(fx_new) = f.value(&x_new) else {
            result.status = Status::NumericalError;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        };
        if !f.grad(&x_new, &mut grad_new)? {
            result.status = Status::NumericalError;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
        bounds.project_gradient(&x_new, &mut grad_new)?;
        if sum_sq(&grad_new)?.sqrt() < cfg.f_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(fx_new);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        if max_abs_diff(&x_new, &x)? < cfg.x_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(fx_new);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        for i in 1..=n {
            y.set_value(i, grad_new.value(lower + i - 1)? - grad.value(lower + i - 1)?)?;
        }
        bfgs_update(&mut h, &s, &y, n)?;
        x = x_new.clone();
        grad = grad_new.clone();
        fx = fx_new;
    }
    result.status = Status::MaxIterations;
    result.solution = Some(x);
    result.value = Some(fx);
    result.gradient = Some(grad);
    Ok(result)
}

// ---------------------------------------------------------------------------------------------
// MathOpt_Newton.hxx
// ---------------------------------------------------------------------------------------------

/// `MathOpt::NewtonConfig` (derives from `Config` in C++).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewtonConfig {
    /// The `Config` base.
    pub base: Config,
    /// Diagonal regularisation tried when the Hessian solve fails [1e-8].
    pub regularization: f64,
    /// Whether to use the Armijo line search [true].
    pub use_line_search: bool,
}
impl Default for NewtonConfig {
    fn default() -> Self {
        Self { base: Config::default(), regularization: 1.0e-8, use_line_search: true }
    }
}
impl NewtonConfig {
    /// `NewtonConfig(theTolerance, theMaxIter)`; the C++ default for `max_iter` is 100.
    pub fn new(tolerance: f64, max_iter: i32) -> Self {
        Self { base: Config::new(tolerance, max_iter), ..Self::default() }
    }
}

/// The Newton direction at `x`: the solution of `H d = -g` (with growing diagonal regularisation when the solve
/// fails), turned into the steepest descent direction when it is not a descent direction or no solve succeeds.
fn newton_direction(hessian: &Matrix, grad: &Vector, dir: &mut Vector, regularization: f64) -> R<()> {
    let (lower, upper) = (grad.lower(), grad.upper());
    let mut neg_grad = Vector::new(lower, upper)?;
    set_neg(&mut neg_grad, grad)?;
    let mut lin = mathlin_pkg::solve(hessian, &neg_grad, 1.0e-20)?;
    if lin.status != Status::Ok {
        let mut lambda = regularization;
        let mut solved = false;
        let mut k = 0;
        while k < 10 && !solved {
            let mut reg = hessian.clone();
            for i in lower..=upper {
                reg.set_value(i, i, reg.value(i, i)? + lambda)?;
            }
            lin = mathlin_pkg::solve(&reg, &neg_grad, 1.0e-20)?;
            if lin.status == Status::Ok {
                solved = true;
            } else {
                lambda *= 10.0;
            }
            k += 1;
        }
        if !solved {
            // `goto perform_line_search` with the steepest descent direction
            return set_neg(dir, grad);
        }
    }
    let solution = lin.solution.as_ref().ok_or(MathError::NotDone)?;
    assign(dir, solution)?;
    let mut dir_deriv = 0.0;
    for i in lower..=upper {
        dir_deriv += grad.value(i)? * dir.value(i)?;
    }
    if dir_deriv >= 0.0 {
        set_neg(dir, grad)?;
    }
    Ok(())
}

fn newton_core<G: HessFn + ?Sized>(f: &mut G, start: &Vector, cfg: &NewtonConfig) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let (lower, upper) = (start.lower(), start.upper());
    let mut x = start.clone();
    let Some(mut fx) = f.value(&x) else {
        result.status = Status::NumericalError;
        return Ok(result);
    };
    let mut grad = Vector::new(lower, upper)?;
    if !f.grad(&x, &mut grad)? {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    if sum_sq(&grad)?.sqrt() < cfg.base.f_tolerance {
        result.status = Status::Ok;
        result.solution = Some(x);
        result.value = Some(fx);
        result.gradient = Some(grad);
        return Ok(result);
    }
    let mut dir = Vector::new(lower, upper)?;
    let mut x_new = Vector::new(lower, upper)?;
    let mut grad_new = Vector::new(lower, upper)?;
    let mut hessian = Matrix::new(lower, upper, lower, upper)?;
    for iter in 0..cfg.base.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        if !f.hess(&x, &mut hessian)? {
            result.status = Status::NumericalError;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
        newton_direction(&hessian, &grad, &mut dir, cfg.regularization)?;
        if cfg.use_line_search {
            let mut line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
            if !line.is_valid {
                set_neg(&mut dir, &grad)?;
                line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, 1.0, 1.0e-4, 0.5, 50)?;
                if !line.is_valid {
                    result.status = Status::NotConverged;
                    result.solution = Some(x);
                    result.value = Some(fx);
                    result.gradient = Some(grad);
                    return Ok(result);
                }
            }
            for i in lower..=upper {
                x_new.set_value(i, x.value(i)? + line.alpha * dir.value(i)?)?;
            }
            fx = line.f_new;
        } else {
            for i in lower..=upper {
                x_new.set_value(i, x.value(i)? + dir.value(i)?)?;
            }
            match f.value(&x_new) {
                Some(v) => fx = v,
                None => {
                    result.status = Status::NumericalError;
                    result.solution = Some(x);
                    return Ok(result);
                }
            }
        }
        let max_diff = max_abs_diff(&x_new, &x)?;
        if !f.grad(&x_new, &mut grad_new)? {
            result.status = Status::NumericalError;
            result.solution = Some(x_new);
            result.value = Some(fx);
            return Ok(result);
        }
        if sum_sq(&grad_new)?.sqrt() < cfg.base.f_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(fx);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        if max_diff < cfg.base.x_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(fx);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        x = x_new.clone();
        grad = grad_new.clone();
    }
    result.status = Status::MaxIterations;
    result.solution = Some(x);
    result.value = Some(fx);
    result.gradient = Some(grad);
    Ok(result)
}

/// `MathOpt::Newton`: Newton's method with an analytic gradient and Hessian.
pub fn newton<F: VectorFunctionWithHessian + ?Sized>(f: &mut F, start: &Vector, cfg: &NewtonConfig) -> R<VectorResult> {
    newton_core(&mut Plain(f), start, cfg)
}

/// `MathOpt::NewtonModified`: the same as [`newton`].
pub fn newton_modified<F: VectorFunctionWithHessian + ?Sized>(
    f: &mut F,
    start: &Vector,
    cfg: &NewtonConfig,
) -> R<VectorResult> {
    newton(f, start, cfg)
}

/// `MathOpt::NewtonNumericalHessian`: analytic gradient, Hessian by `NumericalHessian` (`hess_step` [1e-6]). That
/// function indexes the Hessian from 1, so a start vector whose lower bound is not 1 gives `Err` (C++ throws).
pub fn newton_numerical_hessian<F: VectorFunctionWithGradient + ?Sized>(
    f: &mut F,
    start: &Vector,
    hess_step: f64,
    cfg: &NewtonConfig,
) -> R<VectorResult> {
    newton_core(&mut NumHess { f, step: hess_step }, start, cfg)
}

/// `MathOpt::NewtonNumerical`: numerical gradient (`grad_step` [1e-8]) and Hessian (`hess_step` [1e-6]).
pub fn newton_numerical<F: VectorFunction + ?Sized>(
    f: &mut F,
    start: &Vector,
    grad_step: f64,
    hess_step: f64,
    cfg: &NewtonConfig,
) -> R<VectorResult> {
    newton_core(&mut NumBoth { f, grad_step, hess_step }, start, cfg)
}

/// `MathOpt::NewtonBounded`: projected Newton inside the box `[lower_bounds, upper_bounds]`.
pub fn newton_bounded<F: VectorFunctionWithHessian + ?Sized>(
    f: &mut F,
    start: &Vector,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &NewtonConfig,
) -> R<VectorResult> {
    let mut f = Plain(f);
    let f = &mut f;
    let mut result = VectorResult::default();
    let (lower, upper) = (start.lower(), start.upper());
    let n = upper - lower + 1;
    if lower_bounds.length() != n || upper_bounds.length() != n {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let bounds = Bounds { lo: lower_bounds, hi: upper_bounds, lower, upper };
    let mut x = start.clone();
    bounds.clamp(&mut x)?;
    let Some(mut fx) = f.value(&x) else {
        result.status = Status::NumericalError;
        return Ok(result);
    };
    let mut grad = Vector::new(lower, upper)?;
    if !f.grad(&x, &mut grad)? {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    bounds.project_gradient(&x, &mut grad)?;
    if sum_sq(&grad)?.sqrt() < cfg.base.f_tolerance {
        result.status = Status::Ok;
        result.solution = Some(x);
        result.value = Some(fx);
        result.gradient = Some(grad);
        return Ok(result);
    }
    let mut dir = Vector::new(lower, upper)?;
    let mut x_new = Vector::new(lower, upper)?;
    let mut grad_new = Vector::new(lower, upper)?;
    let mut hessian = Matrix::new(lower, upper, lower, upper)?;
    for iter in 0..cfg.base.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        if !f.hess(&x, &mut hessian)? {
            result.status = Status::NumericalError;
            result.solution = Some(x);
            result.value = Some(fx);
            return Ok(result);
        }
        newton_direction(&hessian, &grad, &mut dir, cfg.regularization)?;
        if cfg.use_line_search {
            let mut alpha_max = bounds.alpha_max(&x, &dir)?;
            let mut line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, alpha_max, 1.0e-4, 0.5, 50)?;
            if !line.is_valid {
                set_neg(&mut dir, &grad)?;
                alpha_max = bounds.alpha_max(&x, &dir)?;
                line = mathutils::armijo_backtrack(f, &x, &dir, &grad, fx, alpha_max, 1.0e-4, 0.5, 50)?;
                if !line.is_valid {
                    result.status = Status::NotConverged;
                    result.solution = Some(x);
                    result.value = Some(fx);
                    result.gradient = Some(grad);
                    return Ok(result);
                }
            }
            for i in lower..=upper {
                x_new.set_value(i, x.value(i)? + line.alpha * dir.value(i)?)?;
            }
        } else {
            for i in lower..=upper {
                x_new.set_value(i, x.value(i)? + dir.value(i)?)?;
            }
        }
        bounds.clamp(&mut x_new)?;
        match f.value(&x_new) {
            Some(v) => fx = v,
            None => {
                result.status = Status::NumericalError;
                result.solution = Some(x);
                return Ok(result);
            }
        }
        let max_diff = max_abs_diff(&x_new, &x)?;
        if !f.grad(&x_new, &mut grad_new)? {
            result.status = Status::NumericalError;
            result.solution = Some(x_new);
            result.value = Some(fx);
            return Ok(result);
        }
        bounds.project_gradient(&x_new, &mut grad_new)?;
        if sum_sq(&grad_new)?.sqrt() < cfg.base.f_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(fx);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        if max_diff < cfg.base.x_tolerance {
            result.status = Status::Ok;
            result.solution = Some(x_new);
            result.value = Some(fx);
            result.gradient = Some(grad_new);
            return Ok(result);
        }
        x = x_new.clone();
        grad = grad_new.clone();
    }
    result.status = Status::MaxIterations;
    result.solution = Some(x);
    result.value = Some(fx);
    result.gradient = Some(grad);
    Ok(result)
}

// ---------------------------------------------------------------------------------------------
// MathOpt_Uzawa.hxx
// ---------------------------------------------------------------------------------------------

/// `MathOpt::UzawaResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct UzawaResult {
    /// Outcome [`Status::NotConverged`].
    pub status: Status,
    /// The solution vector X (1-based).
    pub solution: Option<Vector>,
    /// The dual (Lagrange) variables.
    pub dual: Option<Vector>,
    /// `X - X0`.
    pub error: Option<Vector>,
    /// `C * X0 - S`.
    pub initial_error: Option<Vector>,
    /// `(C * C^T)^-1`, set by the equality-only branch.
    pub inverse_ctc: Option<Matrix>,
    /// Iterations used.
    pub nb_iterations: i32,
}
impl Default for UzawaResult {
    fn default() -> Self {
        Self {
            status: Status::NotConverged,
            solution: None,
            dual: None,
            error: None,
            initial_error: None,
            inverse_ctc: None,
            nb_iterations: 0,
        }
    }
}
impl UzawaResult {
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
}

/// `MathOpt::UzawaConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UzawaConfig {
    /// Tolerance for X convergence [1e-6].
    pub eps_lix: f64,
    /// Tolerance for dual variable convergence [1e-6].
    pub eps_lic: f64,
    /// Maximum iterations [500].
    pub max_iterations: i32,
}
impl Default for UzawaConfig {
    fn default() -> Self {
        Self { eps_lix: 1.0e-6, eps_lic: 1.0e-6, max_iterations: 500 }
    }
}

/// `MathOpt::Uzawa`: minimises `|X - X0|` subject to `C X = S` (the first `nce` rows) and `C X <= S` (the last `nci`).
pub fn uzawa(cont: &Matrix, secont: &Vector, start: &Vector, nce: i32, nci: i32, cfg: &UzawaConfig) -> R<UzawaResult> {
    let mut result = UzawaResult::default();
    let nlig = cont.row_number();
    let ncol = cont.col_number();
    if secont.length() != nlig || (nce + nci) != nlig || start.length() != ncol {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let row_lower = cont.lower_row();
    let col_lower = cont.lower_col();
    let sec_lower = secont.lower();
    let x_lower = start.lower();
    let mut errinit = Vector::new(1, nlig)?;
    for i in 1..=nlig {
        let mut sum = 0.0;
        for j in 1..=ncol {
            sum += cont.value(i + row_lower - 1, j + col_lower - 1)? * start.value(j + x_lower - 1)?;
        }
        errinit.set_value(i, sum - secont.value(i + sec_lower - 1)?)?;
    }
    result.initial_error = Some(errinit.clone());
    let mut vardua = Vector::new(1, nlig)?;
    let mut erruza = Vector::filled(1, ncol, 0.0)?;
    let mut ctc_inv = Matrix::filled(1, nlig, 1, nlig, 0.0)?;
    if nci == 0 {
        result.nb_iterations = 1;
        for i in 1..=nlig {
            for j in 1..=i {
                let mut sum = 0.0;
                for k in 1..=ncol {
                    sum += cont.value(i + row_lower - 1, k + col_lower - 1)?
                        * cont.value(j + row_lower - 1, k + col_lower - 1)?;
                }
                ctc_inv.set_value(i, j, sum)?;
                if i != j {
                    ctc_inv.set_value(j, i, sum)?;
                }
            }
        }
        let inv = mathlin_pkg::invert_crout(&ctc_inv, 1.0e-20)?;
        if inv.status != Status::Ok {
            result.status = Status::Singular;
            return Ok(result);
        }
        ctc_inv = inv.inverse.ok_or(MathError::NotDone)?;
        for i in 1..=nlig {
            let mut sum = 0.0;
            for j in 1..=nlig {
                sum += ctc_inv.value(i, j)? * errinit.value(j)?;
            }
            vardua.set_value(i, sum)?;
        }
        for i in 1..=ncol {
            let mut sum = 0.0;
            for j in 1..=nlig {
                sum -= cont.value(j + row_lower - 1, i + col_lower - 1)? * vardua.value(j)?;
            }
            erruza.set_value(i, sum)?;
        }
        let mut resul = Vector::new(1, ncol)?;
        for i in 1..=ncol {
            resul.set_value(i, start.value(i + x_lower - 1)? + erruza.value(i)?)?;
        }
        result.solution = Some(resul);
        result.dual = Some(vardua);
        result.error = Some(erruza);
        result.inverse_ctc = Some(ctc_inv);
        result.status = Status::Ok;
        return Ok(result);
    }
    for i in 1..=nlig {
        vardua.set_value(i, if i <= nce { 0.0 } else { 1.0 })?;
    }
    let mut normat = 0.0;
    for i in 1..=nlig {
        let mut normli = 0.0;
        for j in 1..=ncol {
            let val = cont.value(i + row_lower - 1, j + col_lower - 1)?;
            normli += val * val;
        }
        normat += normli;
    }
    let rho = 1.0 / (2.0_f64.sqrt() * normat);
    for iter in 1..=cfg.max_iterations {
        result.nb_iterations = iter;
        let mut xmax = 0.0;
        for i in 1..=ncol {
            let xprev = erruza.value(i)?;
            let mut sum = 0.0;
            for j in 1..=nlig {
                sum -= cont.value(j + row_lower - 1, i + col_lower - 1)? * vardua.value(j)?;
            }
            erruza.set_value(i, sum)?;
            if iter > 1 {
                xmax = cmax(xmax, (erruza.value(i)? - xprev).abs());
            }
        }
        let mut err_max = 0.0;
        for i in 1..=nlig {
            let mut err = errinit.value(i)?;
            for j in 1..=ncol {
                err += cont.value(i + row_lower - 1, j + col_lower - 1)? * erruza.value(j)?;
            }
            let err1 = if i <= nce {
                vardua.set_value(i, vardua.value(i)? + rho * err)?;
                (rho * err).abs()
            } else {
                let xmu_prev = vardua.value(i)?;
                vardua.set_value(i, cmax(0.0, vardua.value(i)? + rho * err))?;
                (vardua.value(i)? - xmu_prev).abs()
            };
            err_max = cmax(err_max, err1);
        }
        if iter > 1 && xmax <= cfg.eps_lix {
            if err_max <= cfg.eps_lic {
                let mut resul = Vector::new(1, ncol)?;
                for i in 1..=ncol {
                    resul.set_value(i, start.value(i + x_lower - 1)? + erruza.value(i)?)?;
                }
                result.solution = Some(resul);
                result.dual = Some(vardua);
                result.error = Some(erruza);
                result.status = Status::Ok;
                return Ok(result);
            }
            result.status = Status::NotConverged;
            return Ok(result);
        }
    }
    result.status = Status::MaxIterations;
    Ok(result)
}

/// `MathOpt::UzawaEquality`: [`uzawa`] with every row an equality.
pub fn uzawa_equality(cont: &Matrix, secont: &Vector, start: &Vector, cfg: &UzawaConfig) -> R<UzawaResult> {
    uzawa(cont, secont, start, cont.row_number(), 0, cfg)
}

// ---------------------------------------------------------------------------------------------
// MathOpt_PSO.hxx
// ---------------------------------------------------------------------------------------------

/// `MathOpt::PSOInitMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PsoInitMode {
    /// All particles randomly initialised.
    RandomOnly = 0,
    /// Initialise from the seed particles only.
    SeededOnly = 1,
    /// Seed particles, the rest random.
    SeededPlusRandom = 2,
}

/// `MathOpt::PSOBoundaryMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PsoBoundaryMode {
    /// Clamp to the bound and reverse the velocity with a -0.5 damping.
    Clamp = 0,
    /// Reflect the coordinate and damp the velocity by -0.5.
    Reflect = 1,
    /// Periodic wrap into the bounds.
    Wrap = 2,
}

/// `MathOpt::PSOInertiaSchedule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum PsoInertiaSchedule {
    /// Fixed omega.
    Constant = 0,
    /// Linear decay from omega to omega_min over the iterations.
    LinearDecay = 1,
}

/// `MathOpt::PSOSeedParticle`.
#[derive(Debug, Clone, PartialEq)]
pub struct PsoSeedParticle {
    /// Initial position (clamped to the bounds).
    pub position: Vector,
    /// Known function value (`None`: it is evaluated).
    pub value: Option<f64>,
    /// Initial velocity (`None`: a bounded random one).
    pub velocity: Option<Vector>,
}
impl PsoSeedParticle {
    /// `PSOSeedParticle(thePos)`.
    pub fn new(position: Vector) -> Self {
        Self { position, value: None, velocity: None }
    }
    /// `PSOSeedParticle(thePos, theValue)`.
    pub fn with_value(position: Vector, value: f64) -> Self {
        Self { position, value: Some(value), velocity: None }
    }
}

/// `MathOpt::PSOStats`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PsoStats {
    /// Total function evaluations.
    pub nb_function_evals: i32,
    /// Iterations performed.
    pub nb_iterations: i32,
    /// Boundary corrections applied.
    pub nb_boundary_corrections: i32,
    /// Times stagnation was detected.
    pub nb_stagnation_events: i32,
    /// Restarts performed.
    pub nb_restarts: i32,
    /// Best value after initialisation [`f64::MAX`].
    pub initial_best: f64,
    /// Best value at termination [`f64::MAX`].
    pub final_best: f64,
}
impl Default for PsoStats {
    fn default() -> Self {
        Self {
            nb_function_evals: 0,
            nb_iterations: 0,
            nb_boundary_corrections: 0,
            nb_stagnation_events: 0,
            nb_restarts: 0,
            initial_best: f64::MAX,
            final_best: f64::MAX,
        }
    }
}

/// `MathOpt::PSOConfig` (derives from `NDimConfig` in C++).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PsoConfig {
    /// The `NDimConfig` base (`new(1e-8, 100, true)` by default).
    pub base: NDimConfig,
    /// Number of particles [40].
    pub nb_particles: i32,
    /// Inertia weight [0.7].
    pub omega: f64,
    /// Personal best attraction [1.5].
    pub phi_personal: f64,
    /// Global best attraction [1.5].
    pub phi_global: f64,
    /// Maximum velocity as a fraction of the search space [0.5].
    pub velocity_clamp: f64,
    /// Random seed [6].
    pub seed: u32,
    /// Initialisation mode [`RandomOnly`](PsoInitMode::RandomOnly).
    pub init_mode: PsoInitMode,
    /// Boundary mode [`Clamp`](PsoBoundaryMode::Clamp).
    pub boundary_mode: PsoBoundaryMode,
    /// Inertia schedule [`Constant`](PsoInertiaSchedule::Constant).
    pub inertia_schedule: PsoInertiaSchedule,
    /// Minimum inertia for the linear decay [0.4].
    pub omega_min: f64,
    /// Minimum iterations before convergence [0].
    pub min_iterations: i32,
    /// Early stop if the best value is at most this [`None`].
    pub target_value: Option<f64>,
    /// Stagnation tolerance (0: use the tolerance) [0].
    pub no_improve_tol: f64,
    /// Stagnation iteration threshold [10].
    pub no_improve_iters: i32,
    /// Fraction of particles to reinitialise at a stagnation (0: no restarts) [0].
    pub restart_fraction: f64,
    /// Maximum restarts (0: unlimited when the fraction is positive) [0].
    pub max_restarts: i32,
    /// Maximum polishing evaluations per dimension (0: no polishing) [50].
    pub polish_budget_per_dim: i32,
}
impl Default for PsoConfig {
    fn default() -> Self {
        Self {
            base: NDimConfig::new(1.0e-8, 100, true),
            nb_particles: 40,
            omega: 0.7,
            phi_personal: 1.5,
            phi_global: 1.5,
            velocity_clamp: 0.5,
            seed: 6,
            init_mode: PsoInitMode::RandomOnly,
            boundary_mode: PsoBoundaryMode::Clamp,
            inertia_schedule: PsoInertiaSchedule::Constant,
            omega_min: 0.4,
            min_iterations: 0,
            target_value: None,
            no_improve_tol: 0.0,
            no_improve_iters: 10,
            restart_fraction: 0.0,
            max_restarts: 0,
            polish_budget_per_dim: 50,
        }
    }
}
impl PsoConfig {
    /// `PSOConfig(theNbParticles, theMaxIter, theTolerance)`; the C++ defaults are 100 and 1e-8.
    pub fn new(nb_particles: i32, max_iter: i32, tolerance: f64) -> Self {
        Self { base: NDimConfig::new(tolerance, max_iter, true), nb_particles, ..Self::default() }
    }
}

/// `MathOpt::PolishCoordinateWise`: Brent minimisations along each coordinate (two passes when there are several),
/// within a budget of `max_polish_evals` evaluations; `eval_count` receives the evaluations used.
#[allow(clippy::too_many_arguments)]
pub fn polish_coordinate_wise<F: VectorFunction + ?Sized>(
    f: &mut F,
    position: &mut Vector,
    value: &mut f64,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    tolerance: f64,
    max_polish_evals: i32,
    eval_count: &mut i32,
) -> R<()> {
    *eval_count = 0;
    let (lower, upper) = (position.lower(), position.upper());
    let nb_dims = upper - lower + 1;
    let nb_passes = if nb_dims > 1 { 2 } else { 1 };
    let per_coord_limit = std::cmp::max(1, max_polish_evals / (nb_dims * nb_passes));
    for dim in lower..=upper {
        if *eval_count >= max_polish_evals {
            break;
        }
        let max_iter = std::cmp::min(per_coord_limit, max_polish_evals - *eval_count);
        mathutils::brent_along_coordinate(
            f,
            position,
            dim,
            lower_bounds.value(dim)?,
            upper_bounds.value(dim)?,
            value,
            tolerance,
            max_iter,
            eval_count,
        )?;
    }
    if *eval_count < max_polish_evals && nb_dims > 1 {
        for dim in lower..=upper {
            if *eval_count >= max_polish_evals {
                break;
            }
            let max_iter = std::cmp::min(per_coord_limit, max_polish_evals - *eval_count);
            mathutils::brent_along_coordinate(
                f,
                position,
                dim,
                lower_bounds.value(dim)?,
                upper_bounds.value(dim)?,
                value,
                tolerance,
                max_iter,
                eval_count,
            )?;
        }
    }
    Ok(())
}

/// One swarm member.
struct Particle {
    position: Vector,
    velocity: Vector,
    best_position: Vector,
    best_value: f64,
    current_value: f64,
}

/// `theFunc.Value(...)` with the failure value `DBL_MAX`, counting the evaluation.
fn eval_or_max<F: VectorFunction + ?Sized>(f: &mut F, x: &Vector, stats: &mut PsoStats) -> f64 {
    stats.nb_function_evals += 1;
    f.value(x).unwrap_or(f64::MAX)
}

/// `MathOpt::PSO` (the full overload): particle swarm optimisation in the box `[lower_bounds, upper_bounds]`, with
/// optional seed particles and statistics. The result vector is polished with [`polish_coordinate_wise`].
pub fn pso<F: VectorFunction + ?Sized>(
    f: &mut F,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &PsoConfig,
    seeds: Option<&[PsoSeedParticle]>,
    stats: Option<&mut PsoStats>,
) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let (lower, upper) = (lower_bounds.lower(), lower_bounds.upper());
    let nb_dims = upper - lower + 1;
    if upper_bounds.length() != nb_dims {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    for dim in lower..=upper {
        if lower_bounds.value(dim)? >= upper_bounds.value(dim)? {
            result.status = Status::InvalidInput;
            return Ok(result);
        }
    }
    let nb_particles = cfg.nb_particles;
    if nb_particles <= 0 {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let mut local = PsoStats::default();
    let mut swarm = Vec::new();
    for _ in 0..nb_particles {
        swarm.push(Particle {
            position: Vector::new(lower, upper)?,
            velocity: Vector::new(lower, upper)?,
            best_position: Vector::new(lower, upper)?,
            best_value: f64::MAX,
            current_value: f64::MAX,
        });
    }
    let mut rng = RandomGenerator::new(u64::from(cfg.seed));
    let mut vel_max = Vector::new(lower, upper)?;
    let mut range = Vector::new(lower, upper)?;
    for dim in lower..=upper {
        range.set_value(dim, upper_bounds.value(dim)? - lower_bounds.value(dim)?)?;
        vel_max.set_value(dim, cfg.velocity_clamp * range.value(dim)?)?;
    }
    let mut global_best = Vector::new(lower, upper)?;
    let mut global_best_value = f64::MAX;
    let seed_list = seeds.unwrap_or(&[]);
    let nb_seeds = seed_list.len();
    let mut seeded = 0usize;
    for seed in seed_list {
        if seed.position.lower() != lower || seed.position.upper() != upper {
            result.status = Status::InvalidInput;
            return Ok(result);
        }
        if let Some(v) = &seed.velocity
            && (v.lower() != lower || v.upper() != upper)
        {
            result.status = Status::InvalidInput;
            return Ok(result);
        }
    }
    if cfg.init_mode == PsoInitMode::SeededOnly && nb_seeds == 0 {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    if nb_seeds > 0 && matches!(cfg.init_mode, PsoInitMode::SeededPlusRandom | PsoInitMode::SeededOnly) {
        let nb_particles_u = nb_particles as usize;
        let nb_direct = std::cmp::min(nb_seeds, nb_particles_u);
        for (seed_idx, seed) in seed_list.iter().take(nb_direct).enumerate() {
            let particle = swarm.get_mut(seed_idx).ok_or(MathError::Range)?;
            let mut was_clamped = false;
            for dim in lower..=upper {
                let clamped =
                    mathutils::clamp(seed.position.value(dim)?, lower_bounds.value(dim)?, upper_bounds.value(dim)?);
                if clamped != seed.position.value(dim)? {
                    was_clamped = true;
                }
                particle.position.set_value(dim, clamped)?;
            }
            if let Some(seed_vel) = &seed.velocity {
                for dim in lower..=upper {
                    particle.velocity.set_value(
                        dim,
                        mathutils::clamp(seed_vel.value(dim)?, -vel_max.value(dim)?, vel_max.value(dim)?),
                    )?;
                }
            } else {
                for dim in lower..=upper {
                    particle.velocity.set_value(dim, (2.0 * rng.next_real() - 1.0) * vel_max.value(dim)?)?;
                }
            }
            if let (Some(v), false) = (seed.value, was_clamped) {
                particle.current_value = v;
            } else {
                particle.current_value = eval_or_max(f, &particle.position, &mut local);
            }
            particle.best_position = particle.position.clone();
            particle.best_value = particle.current_value;
            if particle.best_value < global_best_value {
                global_best_value = particle.best_value;
                global_best = particle.best_position.clone();
            }
        }
        seeded = nb_direct;
        if cfg.init_mode == PsoInitMode::SeededOnly && seeded < nb_particles_u {
            for part_idx in seeded..nb_particles_u {
                let particle = swarm.get_mut(part_idx).ok_or(MathError::Range)?;
                let src = seed_list.get(part_idx % seeded).ok_or(MathError::Range)?;
                for dim in lower..=upper {
                    let jitter = (2.0 * rng.next_real() - 1.0) * 0.1 * range.value(dim)?;
                    particle.position.set_value(
                        dim,
                        mathutils::clamp(
                            src.position.value(dim)? + jitter,
                            lower_bounds.value(dim)?,
                            upper_bounds.value(dim)?,
                        ),
                    )?;
                    particle.velocity.set_value(dim, (2.0 * rng.next_real() - 1.0) * vel_max.value(dim)?)?;
                }
                particle.current_value = eval_or_max(f, &particle.position, &mut local);
                particle.best_position = particle.position.clone();
                particle.best_value = particle.current_value;
                if particle.best_value < global_best_value {
                    global_best_value = particle.best_value;
                    global_best = particle.best_position.clone();
                }
            }
            seeded = nb_particles_u;
        }
    }
    for part_idx in seeded..nb_particles as usize {
        let particle = swarm.get_mut(part_idx).ok_or(MathError::Range)?;
        for dim in lower..=upper {
            particle.position.set_value(dim, lower_bounds.value(dim)? + rng.next_real() * range.value(dim)?)?;
            particle.velocity.set_value(dim, (2.0 * rng.next_real() - 1.0) * vel_max.value(dim)?)?;
        }
        particle.current_value = eval_or_max(f, &particle.position, &mut local);
        particle.best_position = particle.position.clone();
        particle.best_value = particle.current_value;
        if particle.best_value < global_best_value {
            global_best_value = particle.best_value;
            global_best = particle.best_position.clone();
        }
    }
    local.initial_best = global_best_value;

    let mut prev_best = global_best_value;
    let mut stagnation_count = 0;
    let stagn_tol = if cfg.no_improve_tol > 0.0 { cfg.no_improve_tol } else { cfg.base.base.tolerance };
    for iter in 0..cfg.base.base.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        let mut omega = cfg.omega;
        if cfg.inertia_schedule == PsoInertiaSchedule::LinearDecay {
            omega = cfg.omega - (cfg.omega - cfg.omega_min) * f64::from(iter) / f64::from(cfg.base.base.max_iterations);
        }
        for particle in &mut swarm {
            for dim in lower..=upper {
                let rand1 = rng.next_real();
                let rand2 = rng.next_real();
                let mut vnew = omega * particle.velocity.value(dim)?
                    + cfg.phi_personal * rand1 * (particle.best_position.value(dim)? - particle.position.value(dim)?)
                    + cfg.phi_global * rand2 * (global_best.value(dim)? - particle.position.value(dim)?);
                vnew = mathutils::clamp(vnew, -vel_max.value(dim)?, vel_max.value(dim)?);
                particle.velocity.set_value(dim, vnew)?;
            }
            for dim in lower..=upper {
                let mut xnew = particle.position.value(dim)? + particle.velocity.value(dim)?;
                let (lo, up) = (lower_bounds.value(dim)?, upper_bounds.value(dim)?);
                if xnew < lo || xnew > up {
                    local.nb_boundary_corrections += 1;
                    match cfg.boundary_mode {
                        PsoBoundaryMode::Clamp => {
                            xnew = mathutils::clamp(xnew, lo, up);
                            particle.velocity.set_value(dim, -0.5 * particle.velocity.value(dim)?)?;
                        }
                        PsoBoundaryMode::Reflect => {
                            if xnew < lo {
                                xnew = 2.0 * lo - xnew;
                            } else {
                                xnew = 2.0 * up - xnew;
                            }
                            xnew = mathutils::clamp(xnew, lo, up);
                            particle.velocity.set_value(dim, -0.5 * particle.velocity.value(dim)?)?;
                        }
                        PsoBoundaryMode::Wrap => {
                            let range_size = range.value(dim)?;
                            xnew -= lo;
                            xnew %= range_size; // std::fmod
                            if xnew < 0.0 {
                                xnew += range_size;
                            }
                            xnew += lo;
                        }
                    }
                }
                particle.position.set_value(dim, xnew)?;
            }
            particle.current_value = eval_or_max(f, &particle.position, &mut local);
            if particle.current_value < particle.best_value {
                particle.best_value = particle.current_value;
                particle.best_position = particle.position.clone();
            }
            if particle.best_value < global_best_value {
                global_best_value = particle.best_value;
                global_best = particle.best_position.clone();
            }
        }
        if let Some(target) = cfg.target_value
            && global_best_value <= target
        {
            break;
        }
        if iter >= cfg.min_iterations {
            if (global_best_value - prev_best).abs() < stagn_tol * (1.0 + global_best_value.abs()) {
                stagnation_count += 1;
                if stagnation_count >= cfg.no_improve_iters {
                    local.nb_stagnation_events += 1;
                    if cfg.restart_fraction > 0.0 && (cfg.max_restarts == 0 || local.nb_restarts < cfg.max_restarts) {
                        local.nb_restarts += 1;
                        stagnation_count = 0;
                        let mut nb_restart = (cfg.restart_fraction * f64::from(nb_particles)).ceil() as i32;
                        nb_restart = std::cmp::min(nb_restart, nb_particles - 1);
                        let mut best_idx = 0usize;
                        for find_idx in 1..swarm.len() {
                            let a = swarm.get(find_idx).ok_or(MathError::Range)?.best_value;
                            let b = swarm.get(best_idx).ok_or(MathError::Range)?.best_value;
                            if a < b {
                                best_idx = find_idx;
                            }
                        }
                        let mut worst: Vec<usize> = (0..swarm.len()).filter(|&k| k != best_idx).collect();
                        let len = worst.len();
                        for outer in 0..len.saturating_sub(1) {
                            for inner in (outer + 1)..len {
                                let bo = swarm
                                    .get(*worst.get(outer).ok_or(MathError::Range)?)
                                    .ok_or(MathError::Range)?
                                    .best_value;
                                let bi = swarm
                                    .get(*worst.get(inner).ok_or(MathError::Range)?)
                                    .ok_or(MathError::Range)?
                                    .best_value;
                                if bo < bi {
                                    worst.swap(outer, inner);
                                }
                            }
                        }
                        let nb_to_restart = usize::try_from(nb_restart).unwrap_or(0).min(len);
                        for rest_idx in 0..nb_to_restart {
                            let idx = *worst.get(rest_idx).ok_or(MathError::Range)?;
                            let part = swarm.get_mut(idx).ok_or(MathError::Range)?;
                            for dim in lower..=upper {
                                part.position
                                    .set_value(dim, lower_bounds.value(dim)? + rng.next_real() * range.value(dim)?)?;
                                part.velocity.set_value(dim, (2.0 * rng.next_real() - 1.0) * vel_max.value(dim)?)?;
                            }
                            part.current_value = eval_or_max(f, &part.position, &mut local);
                            part.best_position = part.position.clone();
                            part.best_value = part.current_value;
                            if part.best_value < global_best_value {
                                global_best_value = part.best_value;
                                global_best = part.best_position.clone();
                            }
                        }
                    } else {
                        break;
                    }
                }
            } else {
                stagnation_count = 0;
            }
        }
        prev_best = global_best_value;
    }
    let mut polish_evals = 0;
    if cfg.polish_budget_per_dim > 0 {
        polish_coordinate_wise(
            f,
            &mut global_best,
            &mut global_best_value,
            lower_bounds,
            upper_bounds,
            cfg.base.base.tolerance,
            cfg.polish_budget_per_dim * nb_dims,
            &mut polish_evals,
        )?;
        local.nb_function_evals += polish_evals;
    }
    local.nb_iterations = result.nb_iterations as i32;
    local.final_best = global_best_value;
    if let Some(s) = stats {
        *s = local;
    }
    result.status = Status::Ok;
    result.solution = Some(global_best);
    result.value = Some(global_best_value);
    Ok(result)
}

/// `MathOpt::PSO` (the overload without seeds and statistics).
pub fn pso_simple<F: VectorFunction + ?Sized>(
    f: &mut F,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &PsoConfig,
) -> R<VectorResult> {
    pso(f, lower_bounds, upper_bounds, cfg, None, None)
}

// ---------------------------------------------------------------------------------------------
// MathOpt_GlobOpt.hxx
// ---------------------------------------------------------------------------------------------

/// `MathOpt::GlobalStrategy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum GlobalStrategy {
    /// Particle swarm only.
    Pso = 0,
    /// Several Powell runs from random starts.
    MultiStart = 1,
    /// Particle swarm followed by a Powell refinement.
    PsoHybrid = 2,
    /// Differential evolution.
    DifferentialEvolution = 3,
}

/// `MathOpt::GlobalConfig` (derives from `NDimConfig` in C++).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalConfig {
    /// The `NDimConfig` base (`new(1e-8, 200, true)` by default).
    pub base: NDimConfig,
    /// Algorithm [`PsoHybrid`](GlobalStrategy::PsoHybrid).
    pub strategy: GlobalStrategy,
    /// Population / swarm size [40].
    pub nb_population: i32,
    /// Number of random starts for multi-start [10].
    pub nb_starts: i32,
    /// Mutation scale for differential evolution [0.8].
    pub mutation_scale: f64,
    /// Crossover probability for differential evolution [0.9].
    pub crossover_prob: f64,
    /// Random seed [6].
    pub seed: u32,
    /// Maximum polishing evaluations per dimension [50].
    pub polish_budget_per_dim: i32,
}
impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            base: NDimConfig::new(1.0e-8, 200, true),
            strategy: GlobalStrategy::PsoHybrid,
            nb_population: 40,
            nb_starts: 10,
            mutation_scale: 0.8,
            crossover_prob: 0.9,
            seed: 6,
            polish_budget_per_dim: 50,
        }
    }
}
impl GlobalConfig {
    /// `GlobalConfig(theStrategy, theMaxIter)`; the C++ default for `max_iter` is 200.
    pub fn new(strategy: GlobalStrategy, max_iter: i32) -> Self {
        Self { base: NDimConfig::new(1.0e-8, max_iter, true), strategy, ..Self::default() }
    }
}

/// `MathOpt::DifferentialEvolution`: DE/rand/1 with binomial crossover, then a coordinate-wise polish.
pub fn differential_evolution<F: VectorFunction + ?Sized>(
    f: &mut F,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &GlobalConfig,
) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let (lower, upper) = (lower_bounds.lower(), lower_bounds.upper());
    let nb_dims = upper - lower + 1;
    if upper_bounds.length() != nb_dims {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let nb_pop = cfg.nb_population;
    if nb_pop < 4 {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let mut_scale = cfg.mutation_scale;
    let cross_prob = cfg.crossover_prob;
    let mut rng = RandomGenerator::new(u64::from(cfg.seed));
    let mut population: Vec<Vector> = Vec::new();
    let mut fitness = Vector::new(0, nb_pop - 1)?;
    for member in 0..nb_pop {
        let mut v = Vector::new(lower, upper)?;
        for dim in lower..=upper {
            let rand_val = rng.next_real();
            v.set_value(
                dim,
                lower_bounds.value(dim)? + rand_val * (upper_bounds.value(dim)? - lower_bounds.value(dim)?),
            )?;
        }
        let fit = f.value(&v).unwrap_or(f64::MAX);
        population.push(v);
        fitness.set_value(member, fit)?;
    }
    let mut best_idx = 0;
    let mut best_value = fitness.value(0)?;
    for member in 1..nb_pop {
        if fitness.value(member)? < best_value {
            best_value = fitness.value(member)?;
            best_idx = member;
        }
    }
    let mut trial = Vector::new(lower, upper)?;
    let pop = |p: &[Vector], i: i32| -> R<Vector> { p.get(i as usize).cloned().ok_or(MathError::Range) };
    for iter in 0..cfg.base.base.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        for member in 0..nb_pop {
            let mut idx_a;
            loop {
                idx_a = (rng.next_real() * f64::from(nb_pop)) as i32;
                if idx_a != member {
                    break;
                }
            }
            let mut idx_b;
            loop {
                idx_b = (rng.next_real() * f64::from(nb_pop)) as i32;
                if !(idx_b == member || idx_b == idx_a) {
                    break;
                }
            }
            let mut idx_c;
            loop {
                idx_c = (rng.next_real() * f64::from(nb_pop)) as i32;
                if !(idx_c == member || idx_c == idx_a || idx_c == idx_b) {
                    break;
                }
            }
            let j_rand = lower + (rng.next_real() * f64::from(nb_dims)) as i32;
            let (pa, pb, pc, pm) = (
                pop(&population, idx_a)?,
                pop(&population, idx_b)?,
                pop(&population, idx_c)?,
                pop(&population, member)?,
            );
            for dim in lower..=upper {
                if rng.next_real() < cross_prob || dim == j_rand {
                    let mut_val = pa.value(dim)? + mut_scale * (pb.value(dim)? - pc.value(dim)?);
                    trial.set_value(
                        dim,
                        mathutils::clamp(mut_val, lower_bounds.value(dim)?, upper_bounds.value(dim)?),
                    )?;
                } else {
                    trial.set_value(dim, pm.value(dim)?)?;
                }
            }
            let trial_fitness = f.value(&trial).unwrap_or(f64::MAX);
            if trial_fitness <= fitness.value(member)? {
                *population.get_mut(member as usize).ok_or(MathError::Range)? = trial.clone();
                fitness.set_value(member, trial_fitness)?;
                if trial_fitness < best_value {
                    best_value = trial_fitness;
                    best_idx = member;
                }
            }
        }
        let mut max_diff = 0.0;
        for member in 0..nb_pop {
            max_diff = cmax(max_diff, (fitness.value(member)? - best_value).abs());
        }
        if max_diff < cfg.base.base.tolerance {
            break;
        }
    }
    let mut polished = pop(&population, best_idx)?;
    let mut polished_value = best_value;
    if cfg.polish_budget_per_dim > 0 {
        let mut polish_evals = 0;
        polish_coordinate_wise(
            f,
            &mut polished,
            &mut polished_value,
            lower_bounds,
            upper_bounds,
            cfg.base.base.tolerance,
            cfg.polish_budget_per_dim * nb_dims,
            &mut polish_evals,
        )?;
    }
    result.status = Status::Ok;
    result.solution = Some(polished);
    result.value = Some(polished_value);
    Ok(result)
}

/// `MathOpt::MultiStart`: Powell runs from `nb_starts` random starting points. `nb_starts == 0` divides by zero in
/// C++ (the process dies); here it is `Err(MathError::Argument)`.
pub fn multi_start<F: VectorFunction + ?Sized>(
    f: &mut F,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &GlobalConfig,
) -> R<VectorResult> {
    let mut result = VectorResult::default();
    let (lower, upper) = (lower_bounds.lower(), lower_bounds.upper());
    let nb_dims = upper - lower + 1;
    if upper_bounds.length() != nb_dims {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let mut rng = RandomGenerator::new(u64::from(cfg.seed));
    let mut best_solution = Vector::new(lower, upper)?;
    let mut best_value = f64::MAX;
    let mut found = false;
    let tol = cfg.base.base.tolerance;
    let mut powell_config = Config { tolerance: tol, x_tolerance: tol, f_tolerance: tol, ..Config::default() };
    powell_config.max_iterations =
        cfg.base.base.max_iterations.checked_div(cfg.nb_starts).ok_or(MathError::Argument)?;
    if powell_config.max_iterations < 10 {
        powell_config.max_iterations = 10;
    }
    for _ in 0..cfg.nb_starts {
        let mut start = Vector::new(lower, upper)?;
        for dim in lower..=upper {
            let rand_val = rng.next_real();
            start.set_value(
                dim,
                lower_bounds.value(dim)? + rand_val * (upper_bounds.value(dim)? - lower_bounds.value(dim)?),
            )?;
        }
        let local = powell(f, &start, &powell_config)?;
        if local.is_done()
            && local.value.is_some()
            && let Some(solution) = &local.solution
        {
            let mut sol = solution.clone();
            for dim in lower..=upper {
                sol.set_value(
                    dim,
                    mathutils::clamp(sol.value(dim)?, lower_bounds.value(dim)?, upper_bounds.value(dim)?),
                )?;
            }
            if let Some(value) = f.value(&sol)
                && value < best_value
            {
                best_value = value;
                best_solution = sol;
                found = true;
            }
        } else if let Some(value) = f.value(&start)
            && value < best_value
        {
            best_value = value;
            best_solution = start;
            found = true;
        }
    }
    if !found {
        result.status = Status::NumericalError;
        return Ok(result);
    }
    result.status = Status::Ok;
    result.solution = Some(best_solution);
    result.value = Some(best_value);
    Ok(result)
}

/// The swarm configuration `GlobalMinimum` derives from a `GlobalConfig` when the caller gives none.
fn derived_pso_config(cfg: &GlobalConfig, max_iterations: i32, tolerance: f64) -> PsoConfig {
    let d = PsoConfig::default();
    PsoConfig {
        base: NDimConfig { base: Config { max_iterations, tolerance, ..d.base.base }, ..d.base },
        nb_particles: cfg.nb_population,
        seed: cfg.seed,
        polish_budget_per_dim: cfg.polish_budget_per_dim,
        ..d
    }
}

/// `MathOpt::GlobalMinimum` (the overload without a PSO configuration, seeds and statistics).
pub fn global_minimum<F: VectorFunction + ?Sized>(
    f: &mut F,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &GlobalConfig,
) -> R<VectorResult> {
    global_minimum_with(f, lower_bounds, upper_bounds, cfg, None, None, None)
}

/// `MathOpt::GlobalMinimum` (the full overload): dispatches on `cfg.strategy`; `pso_config`, `seeds` and `stats` are
/// used by the two swarm strategies.
pub fn global_minimum_with<F: VectorFunction + ?Sized>(
    f: &mut F,
    lower_bounds: &Vector,
    upper_bounds: &Vector,
    cfg: &GlobalConfig,
    pso_config: Option<&PsoConfig>,
    seeds: Option<&[PsoSeedParticle]>,
    stats: Option<&mut PsoStats>,
) -> R<VectorResult> {
    match cfg.strategy {
        GlobalStrategy::Pso => {
            let pc = match pso_config {
                Some(c) => *c,
                None => derived_pso_config(cfg, cfg.base.base.max_iterations, cfg.base.base.tolerance),
            };
            pso(f, lower_bounds, upper_bounds, &pc, seeds, stats)
        }
        GlobalStrategy::MultiStart => multi_start(f, lower_bounds, upper_bounds, cfg),
        GlobalStrategy::PsoHybrid => {
            let pc = match pso_config {
                Some(c) => *c,
                // half the iterations and a relaxed tolerance for the swarm phase
                None => derived_pso_config(cfg, cfg.base.base.max_iterations / 2, cfg.base.base.tolerance * 10.0),
            };
            let pso_result = pso(f, lower_bounds, upper_bounds, &pc, seeds, stats)?;
            let Some(pso_solution) = pso_result.solution.as_ref().filter(|_| pso_result.is_done()) else {
                return Ok(pso_result);
            };
            let tol = cfg.base.base.tolerance;
            let powell_config = Config {
                tolerance: tol,
                x_tolerance: tol,
                f_tolerance: tol,
                max_iterations: cfg.base.base.max_iterations / 2,
                ..Config::default()
            };
            let mut local = powell(f, pso_solution, &powell_config)?;
            if local.is_done()
                && let (Some(local_value), Some(pso_value)) = (local.value, pso_result.value)
                && local_value < pso_value
            {
                let (lower, upper) = (lower_bounds.lower(), lower_bounds.upper());
                let mut sol = local.solution.clone().ok_or(MathError::NotDone)?;
                for dim in lower..=upper {
                    sol.set_value(
                        dim,
                        mathutils::clamp(sol.value(dim)?, lower_bounds.value(dim)?, upper_bounds.value(dim)?),
                    )?;
                }
                local.solution = Some(sol.clone());
                if let Some(value) = f.value(&sol) {
                    local.value = Some(value);
                }
                return Ok(local);
            }
            Ok(pso_result)
        }
        GlobalStrategy::DifferentialEvolution => differential_evolution(f, lower_bounds, upper_bounds, cfg),
    }
}
