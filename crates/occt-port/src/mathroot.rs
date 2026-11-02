//! OCCT's header-only `MathRoot` package: bracketing and open root finders for one variable (Brent, bisection,
//! Newton, secant), all roots of a function in an interval, null intervals, and the trigonometric equation solver.
//!
//! Translated from `MathRoot_*.hxx` (OCCT 8.0.1), on the function-object traits of `mathutils`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::mathlin::{MathError, Vector};
use crate::mathpoly;
use crate::mathutils::{
    self, Config, ScalarFunction, ScalarFunctionWithDerivative, ScalarResult, Status, THE_2PI, THE_PI, THE_ZERO_TOL,
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

/// `out = f(x)` with the C++ out-parameter convention (`out` untouched on failure).
fn val<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, out: &mut f64) -> bool {
    match f.value(x) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

/// `f.Values(x, y, dy)` with out-parameters.
fn vals<F: ScalarFunctionWithDerivative + ?Sized>(f: &mut F, x: f64, y: &mut f64, dy: &mut f64) -> bool {
    match f.values(x) {
        Some((a, b)) => {
            *y = a;
            *dy = b;
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------------------------
// MathRoot_Brent.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::Brent`.
pub fn brent<F: ScalarFunction + ?Sized>(f: &mut F, lower: f64, upper: f64, config: &Config) -> ScalarResult {
    let mut result = ScalarResult::default();
    let mut a = lower;
    let mut b = upper;
    let mut fa = 0.0;
    let mut fb = 0.0;
    if !val(f, a, &mut fa) {
        result.status = Status::NumericalError;
        return result;
    }
    if !val(f, b, &mut fb) {
        result.status = Status::NumericalError;
        return result;
    }
    if fa * fb > 0.0 {
        result.status = Status::InvalidInput;
        return result;
    }
    if fa.abs() < fb.abs() {
        core::mem::swap(&mut a, &mut b);
        core::mem::swap(&mut fa, &mut fb);
    }
    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;
    for iter in 0..config.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        let tol = 2.0 * mathutils::THE_EPSILON * b.abs() + 0.5 * config.x_tolerance;
        let m = 0.5 * (c - b);
        if fb.abs() < config.f_tolerance || fb == 0.0 || m.abs() <= tol {
            result.status = Status::Ok;
            result.root = Some(b);
            result.value = Some(fb);
            return result;
        }
        let mut s;
        if (fa - fc).abs() > THE_ZERO_TOL && (fb - fc).abs() > THE_ZERO_TOL {
            s = a * fb * fc / ((fa - fb) * (fa - fc))
                + b * fa * fc / ((fb - fa) * (fb - fc))
                + c * fa * fb / ((fc - fa) * (fc - fb));
        } else {
            s = b - fb * (b - a) / (fb - fa);
        }
        let mut use_interp = false;
        let bound1 = (3.0 * a + b) / 4.0;
        if s > cmin(bound1, b) && s < cmax(bound1, b) && (s - b).abs() < e.abs() / 2.0 {
            use_interp = true;
        }
        if !use_interp {
            s = b + m;
            e = m;
            d = m;
        } else {
            e = d;
            d = s - b;
        }
        a = b;
        fa = fb;
        if d.abs() > tol {
            b = s;
        } else {
            b += if m > 0.0 { tol } else { -tol };
        }
        if !val(f, b, &mut fb) {
            result.status = Status::NumericalError;
            result.root = Some(b);
            return result;
        }
        if fb * fc > 0.0 {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        } else if fc.abs() < fb.abs() {
            a = b;
            fa = fb;
            core::mem::swap(&mut b, &mut c);
            core::mem::swap(&mut fb, &mut fc);
        }
    }
    result.status = Status::MaxIterations;
    result.root = Some(b);
    result.value = Some(fb);
    result
}

// ---------------------------------------------------------------------------------------------
// MathRoot_Bisection.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::Bisection`.
pub fn bisection<F: ScalarFunction + ?Sized>(f: &mut F, lower: f64, upper: f64, config: &Config) -> ScalarResult {
    let mut result = ScalarResult::default();
    let mut a = lower;
    let mut b = upper;
    let mut fa = 0.0;
    let mut fb = 0.0;
    if !val(f, a, &mut fa) {
        result.status = Status::NumericalError;
        return result;
    }
    if !val(f, b, &mut fb) {
        result.status = Status::NumericalError;
        return result;
    }
    if fa * fb > 0.0 {
        result.status = Status::InvalidInput;
        return result;
    }
    if a > b {
        core::mem::swap(&mut a, &mut b);
        core::mem::swap(&mut fa, &mut fb);
    }
    let is_neg_at_a = fa < 0.0;
    for iter in 0..config.max_iterations {
        let m = 0.5 * (a + b);
        let mut fm = 0.0;
        if !val(f, m, &mut fm) {
            result.status = Status::NumericalError;
            result.root = Some(m);
            result.nb_iterations = (iter + 1) as usize;
            return result;
        }
        result.nb_iterations = (iter + 1) as usize;
        if fm.abs() < config.f_tolerance {
            result.status = Status::Ok;
            result.root = Some(m);
            result.value = Some(fm);
            return result;
        }
        if (b - a) < config.x_tolerance * cmax(1.0, m.abs()) {
            result.status = Status::Ok;
            result.root = Some(m);
            result.value = Some(fm);
            return result;
        }
        let is_neg_at_m = fm < 0.0;
        if is_neg_at_m == is_neg_at_a {
            a = m;
            fa = fm;
        } else {
            b = m;
            fb = fm;
        }
    }
    let _ = (fa, fb);
    result.status = Status::MaxIterations;
    let root = 0.5 * (a + b);
    let mut value = 0.0;
    val(f, root, &mut value);
    result.root = Some(root);
    result.value = Some(value);
    result
}

/// `MathRoot::BisectionNewton`.
pub fn bisection_newton<F: ScalarFunctionWithDerivative + ?Sized>(
    f: &mut F,
    lower: f64,
    upper: f64,
    config: &Config,
) -> ScalarResult {
    let mut result = ScalarResult::default();
    let mut a = lower;
    let mut b = upper;
    let mut fa = 0.0;
    let mut fb = 0.0;
    let mut dummy = 0.0;
    if !vals(f, a, &mut fa, &mut dummy) {
        result.status = Status::NumericalError;
        return result;
    }
    if !vals(f, b, &mut fb, &mut dummy) {
        result.status = Status::NumericalError;
        return result;
    }
    if fa * fb > 0.0 {
        result.status = Status::InvalidInput;
        return result;
    }
    if a > b {
        core::mem::swap(&mut a, &mut b);
        core::mem::swap(&mut fa, &mut fb);
    }
    let is_neg_at_a = fa < 0.0;
    let mut x = 0.5 * (a + b);
    let mut fx = 0.0;
    let mut dfx = 0.0;
    for iter in 0..config.max_iterations {
        if !vals(f, x, &mut fx, &mut dfx) {
            result.status = Status::NumericalError;
            result.root = Some(x);
            result.nb_iterations = (iter + 1) as usize;
            return result;
        }
        result.nb_iterations = (iter + 1) as usize;
        if fx.abs() < config.f_tolerance {
            result.status = Status::Ok;
            result.root = Some(x);
            result.value = Some(fx);
            result.derivative = Some(dfx);
            return result;
        }
        if (b - a) < config.x_tolerance * cmax(1.0, x.abs()) {
            result.status = Status::Ok;
            result.root = Some(x);
            result.value = Some(fx);
            result.derivative = Some(dfx);
            return result;
        }
        let mut x_new = x;
        if !mathutils::is_zero(dfx, THE_ZERO_TOL) {
            x_new = x - fx / dfx;
        }
        if x_new <= a || x_new >= b {
            x_new = 0.5 * (a + b);
        }
        let is_neg_at_x = fx < 0.0;
        if is_neg_at_x == is_neg_at_a {
            a = x;
        } else {
            b = x;
        }
        x = x_new;
    }
    result.status = Status::MaxIterations;
    let (mut value, mut derivative) = (0.0, 0.0);
    vals(f, x, &mut value, &mut derivative);
    result.root = Some(x);
    result.value = Some(value);
    result.derivative = Some(derivative);
    result
}

// ---------------------------------------------------------------------------------------------
// MathRoot_Secant.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::Secant`.
pub fn secant<F: ScalarFunction + ?Sized>(f: &mut F, x0_in: f64, x1_in: f64, config: &Config) -> ScalarResult {
    let mut result = ScalarResult::default();
    let mut x0 = x0_in;
    let mut x1 = x1_in;
    let mut f0 = 0.0;
    let mut f1 = 0.0;
    if !val(f, x0, &mut f0) {
        result.status = Status::NumericalError;
        return result;
    }
    if !val(f, x1, &mut f1) {
        result.status = Status::NumericalError;
        return result;
    }
    for iter in 0..config.max_iterations {
        result.nb_iterations = (iter + 1) as usize;
        if mathutils::is_f_converged(f1, config.f_tolerance) {
            result.status = Status::Ok;
            result.root = Some(x1);
            result.value = Some(f1);
            return result;
        }
        let denom = f1 - f0;
        if mathutils::is_zero(denom, THE_ZERO_TOL) {
            result.status = Status::NumericalError;
            result.root = Some(x1);
            result.value = Some(f1);
            return result;
        }
        let x_new = x1 - f1 * (x1 - x0) / denom;
        if mathutils::is_x_converged(x1, x_new, config.x_tolerance) {
            let mut f_new = 0.0;
            val(f, x_new, &mut f_new);
            result.status = Status::Ok;
            result.root = Some(x_new);
            result.value = Some(f_new);
            return result;
        }
        x0 = x1;
        f0 = f1;
        x1 = x_new;
        if !val(f, x1, &mut f1) {
            result.status = Status::NumericalError;
            result.root = Some(x1);
            return result;
        }
    }
    result.status = Status::MaxIterations;
    result.root = Some(x1);
    result.value = Some(f1);
    result
}

/// `MathRoot::SecantAuto`.
pub fn secant_auto<F: ScalarFunction + ?Sized>(f: &mut F, x0: f64, config: &Config) -> ScalarResult {
    let delta = if x0.abs() > 1.0 { 0.01 * x0 } else { 0.01 };
    secant(f, x0, x0 + delta, config)
}

// ---------------------------------------------------------------------------------------------
// MathRoot_Newton.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::Newton`.
pub fn newton<F: ScalarFunctionWithDerivative + ?Sized>(f: &mut F, guess: f64, config: &Config) -> ScalarResult {
    let mut result = ScalarResult::default();
    let mut x = guess;
    let mut fx = 0.0;
    let mut dfx = 0.0;
    for iter in 0..config.max_iterations {
        let x_old = x;
        if !vals(f, x, &mut fx, &mut dfx) {
            result.status = Status::NumericalError;
            result.root = Some(x);
            result.nb_iterations = iter as usize;
            return result;
        }
        if mathutils::is_zero(dfx, THE_ZERO_TOL) {
            if !mathutils::is_f_converged(fx, config.f_tolerance) {
                result.status = Status::NumericalError;
                result.root = Some(x);
                result.value = Some(fx);
                result.derivative = Some(dfx);
                result.nb_iterations = iter as usize;
                return result;
            }
        } else {
            x -= fx / dfx;
        }
        result.nb_iterations = (iter + 1) as usize;
        if mathutils::is_converged(x_old, x, fx, config) {
            result.status = Status::Ok;
            result.root = Some(x);
            result.value = Some(fx);
            result.derivative = Some(dfx);
            return result;
        }
    }
    result.status = Status::MaxIterations;
    result.root = Some(x);
    result.value = Some(fx);
    result.derivative = Some(dfx);
    result
}

/// `MathRoot::NewtonBounded`.
pub fn newton_bounded<F: ScalarFunctionWithDerivative + ?Sized>(
    f: &mut F,
    guess: f64,
    lower: f64,
    upper: f64,
    config: &Config,
) -> ScalarResult {
    let mut result = ScalarResult::default();
    let mut x = mathutils::clamp(guess, lower, upper);
    let mut x_lo = lower;
    let mut x_hi = upper;
    let mut fx = 0.0;
    let mut dfx = 0.0;
    let mut f_lo = 0.0;
    let mut f_hi = 0.0;
    let mut dummy = 0.0;
    if !vals(f, x_lo, &mut f_lo, &mut dummy) {
        result.status = Status::NumericalError;
        return result;
    }
    if !vals(f, x_hi, &mut f_hi, &mut dummy) {
        result.status = Status::NumericalError;
        return result;
    }
    let bracketed = f_lo * f_hi < 0.0;
    for iter in 0..config.max_iterations {
        let x_old = x;
        if !vals(f, x, &mut fx, &mut dfx) {
            result.status = Status::NumericalError;
            result.root = Some(x);
            result.nb_iterations = iter as usize;
            return result;
        }
        result.nb_iterations = (iter + 1) as usize;
        if mathutils::is_f_converged(fx, config.f_tolerance) {
            result.status = Status::Ok;
            result.root = Some(x);
            result.value = Some(fx);
            result.derivative = Some(dfx);
            return result;
        }
        let mut x_new = x;
        if !mathutils::is_zero(dfx, THE_ZERO_TOL) {
            x_new = x - fx / dfx;
        }
        if x_new < x_lo || x_new > x_hi {
            if bracketed {
                x_new = 0.5 * (x_lo + x_hi);
            } else {
                x_new = mathutils::clamp(x_new, x_lo, x_hi);
            }
        }
        x = x_new;
        if bracketed {
            if fx * f_lo < 0.0 {
                x_hi = x_old;
                f_hi = fx;
            } else {
                x_lo = x_old;
                f_lo = fx;
            }
        }
        if mathutils::is_x_converged(x_old, x, config.x_tolerance) {
            vals(f, x, &mut fx, &mut dfx);
            result.status = Status::Ok;
            result.root = Some(x);
            result.value = Some(fx);
            result.derivative = Some(dfx);
            return result;
        }
    }
    let _ = f_hi;
    result.status = Status::MaxIterations;
    result.root = Some(x);
    result.value = Some(fx);
    result.derivative = Some(dfx);
    result
}

// ---------------------------------------------------------------------------------------------
// MathRoot_Trig.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::TrigResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrigResult {
    pub status: Status,
    pub roots: [f64; 4],
    pub nb_roots: i32,
    pub infinite_roots: bool,
}
impl Default for TrigResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, roots: [0.0; 4], nb_roots: 0, infinite_roots: false }
    }
}
impl TrigResult {
    /// `IsDone()` (and the C++ `explicit operator bool`).
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
}

fn zset(z: &mut [f64; 4], i: usize, v: f64) {
    if let Some(slot) = z.get_mut(i) {
        *slot = v;
    }
}

fn zget(z: &[f64; 4], i: usize) -> f64 {
    z.get(i).copied().unwrap_or(0.0)
}

/// `aResult.Roots[aResult.NbRoots++] = v`.
fn trig_push(r: &mut TrigResult, v: f64) {
    if let Some(slot) = usize::try_from(r.nb_roots).ok().and_then(|k| r.roots.get_mut(k)) {
        *slot = v;
    }
    r.nb_roots += 1;
}

fn trig_root(r: &TrigResult, k: i32) -> f64 {
    usize::try_from(k).ok().and_then(|k| r.roots.get(k)).copied().unwrap_or(0.0)
}

/// Inserts `teta` into the sorted roots (`aPos` search and shift of `Trigonometric`).
fn trig_insert_sorted(r: &mut TrigResult, teta: f64) {
    let mut pos = r.nb_roots;
    for k in 0..r.nb_roots {
        if teta < trig_root(r, k) {
            pos = k;
            break;
        }
    }
    let mut k = r.nb_roots;
    while k > pos {
        let v = trig_root(r, k - 1);
        if let Some(slot) = usize::try_from(k).ok().and_then(|i| r.roots.get_mut(i)) {
            *slot = v;
        }
        k -= 1;
    }
    if let Some(slot) = usize::try_from(pos).ok().and_then(|i| r.roots.get_mut(i)) {
        *slot = teta;
    }
    r.nb_roots += 1;
}

/// The Newton refinement lambda `aRefineRoot` of `Trigonometric`.
fn trig_refine_root(a: f64, b: f64, c: f64, d: f64, e: f64, x_in: f64) -> f64 {
    const MAX_ITER: i32 = 20;
    const TOL: f64 = 1.0e-14;
    let mut x = x_in;
    for _ in 0..MAX_ITER {
        let cos = libm::cos(x);
        let sin = libm::sin(x);
        let cos2 = cos * cos;
        let sin2 = sin * sin;
        let cs = cos * sin;
        let f = a * cos2 + 2.0 * b * cs + c * cos + d * sin + e;
        let df = -2.0 * a * cs + 2.0 * b * (cos2 - sin2) - c * sin + d * cos;
        if f.abs() < 1.0e-15 {
            break;
        }
        let mut delta;
        if df.abs() < 1.0e-10 * (f.abs() + 1.0) {
            let d2f = -2.0 * a * (cos2 - sin2) - 4.0 * b * cs - c * cos - d * sin;
            let denom = 2.0 * df * df - f * d2f;
            if denom.abs() < 1.0e-30 {
                break;
            }
            delta = 2.0 * f * df / denom;
        } else {
            delta = f / df;
        }
        const MAX_STEP: f64 = 0.5;
        if delta.abs() > MAX_STEP {
            delta = if delta > 0.0 { MAX_STEP } else { -MAX_STEP };
        }
        x -= delta;
        if delta.abs() < TOL {
            break;
        }
    }
    x
}

/// `MathRoot::Trigonometric` (defaults `inf_bound` 0, `sup_bound` 2 pi, `eps` 1.5e-12):
/// roots of `a cos^2 + 2 b cos sin + c cos + d sin + e = 0` in `[inf_bound, sup_bound]`.
#[allow(clippy::too_many_arguments)]
pub fn trigonometric(a: f64, b: f64, c: f64, d: f64, e: f64, inf_bound: f64, sup_bound: f64, eps: f64) -> TrigResult {
    let mut result = TrigResult { status: Status::Ok, ..TrigResult::default() };
    let my_borne_inf;
    let mut delta;
    let a_mod;
    let lowest = -f64::MAX;
    if inf_bound <= lowest / 2.0 && sup_bound >= f64::MAX / 2.0 {
        my_borne_inf = 0.0;
        delta = THE_2PI;
        a_mod = 0.0;
    } else if sup_bound >= f64::MAX / 2.0 {
        my_borne_inf = inf_bound;
        delta = THE_2PI;
        a_mod = my_borne_inf / THE_2PI;
    } else if inf_bound <= lowest / 2.0 {
        my_borne_inf = sup_bound - THE_2PI;
        delta = THE_2PI;
        a_mod = my_borne_inf / THE_2PI;
    } else {
        my_borne_inf = inf_bound;
        delta = sup_bound - inf_bound;
        a_mod = inf_bound / THE_2PI;
        if delta > THE_2PI {
            delta = THE_2PI;
        }
    }
    let mut zer = [0.0_f64; 4];
    #[allow(unused_assignments)]
    let mut n_zer: usize;
    let delta_eps = f64::EPSILON * delta.abs();
    let trunc_mod = libm::trunc(a_mod);
    if a.abs() <= eps && b.abs() <= eps {
        if c.abs() <= eps {
            if d.abs() <= eps {
                if e.abs() <= eps {
                    result.infinite_roots = true;
                    return result;
                }
                result.nb_roots = 0;
                return result;
            }
            let v = -e / d;
            if v.abs() > 1.0 {
                result.nb_roots = 0;
                return result;
            }
            zset(&mut zer, 0, libm::asin(v));
            let z0 = zget(&zer, 0);
            zset(&mut zer, 1, THE_PI - z0);
            n_zer = 2;
            for i in 0..n_zer {
                if zget(&zer, i) <= -eps {
                    let z = THE_2PI - zget(&zer, i).abs();
                    zset(&mut zer, i, z);
                }
                let z = zget(&zer, i) + trunc_mod * THE_2PI;
                zset(&mut zer, i, z);
                let x = zget(&zer, i) - my_borne_inf;
                if x >= -delta_eps && x <= delta + delta_eps {
                    trig_push(&mut result, zget(&zer, i));
                }
            }
            return result;
        } else if d.abs() <= eps {
            let v = -e / c;
            if v.abs() > 1.0 {
                result.nb_roots = 0;
                return result;
            }
            let principal = libm::acos(v);
            zset(&mut zer, 0, principal);
            zset(&mut zer, 1, -principal);
            n_zer = 2;
            for i in 0..n_zer {
                let mut angle = zget(&zer, i);
                let k = libm::floor((inf_bound - angle) / THE_2PI);
                angle += (k + 1.0) * THE_2PI;
                for period in 0..2 {
                    let test_angle = angle + f64::from(period) * THE_2PI;
                    if test_angle >= inf_bound - delta_eps && test_angle <= sup_bound + delta_eps {
                        let mut dup = false;
                        for k in 0..result.nb_roots {
                            if (test_angle - trig_root(&result, k)).abs() < eps {
                                dup = true;
                                break;
                            }
                        }
                        if !dup && result.nb_roots < 4 {
                            trig_push(&mut result, test_angle);
                        }
                    }
                }
            }
            return result;
        } else {
            let aa = e - c;
            let bb = 2.0 * d;
            let cc = e + c;
            let poly = mathpoly::quadratic(aa, bb, cc);
            if !poly.is_done() {
                result.status = poly.status;
                return result;
            }
            if poly.status == Status::InfiniteSolutions {
                result.infinite_roots = true;
                return result;
            }
            n_zer = poly.nb_roots;
            for i in 0..n_zer {
                zset(&mut zer, i, poly.roots.get(i).copied().unwrap_or(0.0));
            }
        }
    } else {
        if a.abs() <= eps && e.abs() <= eps {
            if c.abs() <= eps {
                zset(&mut zer, 0, 0.0);
                zset(&mut zer, 1, THE_PI);
                n_zer = 2;
                let v = -d / (b * 2.0);
                if v.abs() <= 1.0 + 1.0e-10 {
                    if v >= 1.0 {
                        zset(&mut zer, 2, 0.0);
                        zset(&mut zer, 3, 0.0);
                    } else if v <= -1.0 {
                        zset(&mut zer, 2, THE_PI);
                        zset(&mut zer, 3, THE_PI);
                    } else {
                        zset(&mut zer, 2, libm::acos(v));
                        let z2 = zget(&zer, 2);
                        zset(&mut zer, 3, THE_2PI - z2);
                    }
                    n_zer = 4;
                }
                trig_finish_special(
                    &mut result,
                    &mut zer,
                    n_zer,
                    my_borne_inf,
                    eps,
                    trunc_mod,
                    delta,
                    inf_bound,
                    sup_bound,
                );
                return result;
            }
            if d.abs() <= eps {
                zset(&mut zer, 0, THE_PI / 2.0);
                zset(&mut zer, 1, THE_PI * 3.0 / 2.0);
                n_zer = 2;
                let v = -c / (b * 2.0);
                if v.abs() <= 1.0 + 1.0e-10 {
                    if v >= 1.0 {
                        zset(&mut zer, 2, THE_PI / 2.0);
                        zset(&mut zer, 3, THE_PI / 2.0);
                    } else if v <= -1.0 {
                        zset(&mut zer, 2, THE_PI * 3.0 / 2.0);
                        zset(&mut zer, 3, THE_PI * 3.0 / 2.0);
                    } else {
                        zset(&mut zer, 2, libm::asin(v));
                        let z2 = zget(&zer, 2);
                        zset(&mut zer, 3, THE_PI - z2);
                    }
                    n_zer = 4;
                }
                trig_finish_special(
                    &mut result,
                    &mut zer,
                    n_zer,
                    my_borne_inf,
                    eps,
                    trunc_mod,
                    delta,
                    inf_bound,
                    sup_bound,
                );
                return result;
            }
        }
        let ko0 = a - c + e;
        let ko1 = 2.0 * d - 4.0 * b;
        let ko2 = 2.0 * e - 2.0 * a;
        let ko3 = 4.0 * b + 2.0 * d;
        let ko4 = a + c + e;
        let poly = mathpoly::quartic(ko0, ko1, ko2, ko3, ko4);
        if !poly.is_done() {
            if poly.status == Status::InfiniteSolutions {
                result.infinite_roots = true;
            } else {
                result.status = poly.status;
            }
            return result;
        }
        n_zer = poly.nb_roots.min(4);
        for i in 0..n_zer {
            zset(&mut zer, i, poly.roots.get(i).copied().unwrap_or(0.0));
        }
        if let Some(head) = zer.get_mut(..n_zer) {
            head.sort_by(|p, q| p.partial_cmp(q).unwrap_or(core::cmp::Ordering::Equal));
        }
    }
    for i in 0..n_zer {
        let mut teta = 2.0 * libm::atan(zget(&zer, i));
        if zget(&zer, i) <= -eps {
            teta = THE_2PI - teta.abs();
        }
        teta += trunc_mod * THE_2PI;
        if teta - my_borne_inf < 0.0 {
            teta += THE_2PI;
        }
        let x = teta - my_borne_inf;
        if x >= -delta_eps && x <= delta + delta_eps {
            let teta_refined = trig_refine_root(a, b, c, d, e, teta);
            let delta_newton = (teta_refined - teta).abs();
            let sup_m_inf_100 = (sup_bound - inf_bound) * 0.01;
            if delta_newton <= sup_m_inf_100 {
                teta = teta_refined;
            }
            let mut found = false;
            for k in 0..result.nb_roots {
                if (teta - trig_root(&result, k)).abs() < eps {
                    found = true;
                    break;
                }
            }
            if !found && result.nb_roots < 4 {
                trig_insert_sorted(&mut result, teta);
            }
        }
    }
    if result.nb_roots < 4 && (a - c + e).abs() <= eps {
        let teta = THE_PI + trunc_mod * THE_2PI;
        let x = teta - my_borne_inf;
        if x >= -delta_eps && x <= delta + delta_eps {
            let mut found = false;
            for k in 0..result.nb_roots {
                if (teta - trig_root(&result, k)).abs() <= eps {
                    found = true;
                    break;
                }
            }
            if !found {
                trig_insert_sorted(&mut result, teta);
            }
        }
    }
    result
}

/// The common tail of the two `a = e = 0` branches (`c = 0` and `d = 0`) of `Trigonometric`.
#[allow(clippy::too_many_arguments)]
fn trig_finish_special(
    result: &mut TrigResult,
    zer: &mut [f64; 4],
    n_zer: usize,
    my_borne_inf: f64,
    eps: f64,
    trunc_mod: f64,
    delta: f64,
    inf_bound: f64,
    sup_bound: f64,
) {
    for i in 0..n_zer {
        if zget(zer, i) <= my_borne_inf - eps {
            let z = zget(zer, i) + THE_2PI;
            zset(zer, i, z);
        }
        let z = zget(zer, i) + trunc_mod * THE_2PI;
        zset(zer, i, z);
        let x = zget(zer, i) - my_borne_inf;
        if x >= -1.0e-10 && x <= delta + 1.0e-10 {
            let z = cmax(inf_bound, cmin(sup_bound, zget(zer, i)));
            zset(zer, i, z);
            trig_push(result, z);
        }
    }
}

/// `MathRoot::TrigonometricLinear` (defaults `0`, `2 pi`): `d sin + e = 0`.
pub fn trigonometric_linear(d: f64, e: f64, inf_bound: f64, sup_bound: f64) -> TrigResult {
    trigonometric(0.0, 0.0, 0.0, d, e, inf_bound, sup_bound, 1.5e-12)
}

/// `MathRoot::TrigonometricCDE` (defaults `0`, `2 pi`): `c cos + d sin + e = 0`.
pub fn trigonometric_cde(c: f64, d: f64, e: f64, inf_bound: f64, sup_bound: f64) -> TrigResult {
    trigonometric(0.0, 0.0, c, d, e, inf_bound, sup_bound, 1.5e-12)
}

// ---------------------------------------------------------------------------------------------
// MathRoot_MultipleUtils.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::MultipleResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct MultipleResult {
    pub status: Status,
    pub nb_iterations: usize,
    pub roots: Vec<f64>,
    pub values: Vec<f64>,
    pub is_all_null: bool,
}
impl Default for MultipleResult {
    fn default() -> Self {
        Self {
            status: Status::NotConverged,
            nb_iterations: 0,
            roots: Vec::new(),
            values: Vec::new(),
            is_all_null: false,
        }
    }
}
impl MultipleResult {
    /// `IsDone()` (and the C++ `explicit operator bool`).
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
    /// `NbRoots()`.
    pub fn nb_roots(&self) -> i32 {
        self.roots.len() as i32
    }
}

/// `MathRoot::MultipleConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MultipleConfig {
    pub nb_samples: i32,
    pub x_tolerance: f64,
    pub f_tolerance: f64,
    pub null_tolerance: f64,
    pub max_iterations: i32,
    pub offset: f64,
}
impl Default for MultipleConfig {
    fn default() -> Self {
        Self {
            nb_samples: 100,
            x_tolerance: 1e-10,
            f_tolerance: 1e-10,
            null_tolerance: 1e-12,
            max_iterations: 100,
            offset: 0.0,
        }
    }
}

/// `MathRoot::SortRoots(MultipleResult&)`: insertion sort of the roots (and their values).
pub fn sort_roots(result: &mut MultipleResult) {
    for i in 1..result.roots.len() {
        let key_root = result.roots.get(i).copied().unwrap_or(0.0);
        let key_val = result.values.get(i).copied().unwrap_or(0.0);
        let mut j = i as isize - 1;
        while j >= 0 && result.roots.get(j as usize).copied().unwrap_or(0.0) > key_root {
            let (r, v) = (
                result.roots.get(j as usize).copied().unwrap_or(0.0),
                result.values.get(j as usize).copied().unwrap_or(0.0),
            );
            if let Some(slot) = result.roots.get_mut((j + 1) as usize) {
                *slot = r;
            }
            if let Some(slot) = result.values.get_mut((j + 1) as usize) {
                *slot = v;
            }
            j -= 1;
        }
        if let Some(slot) = result.roots.get_mut((j + 1) as usize) {
            *slot = key_root;
        }
        if let Some(slot) = result.values.get_mut((j + 1) as usize) {
            *slot = key_val;
        }
    }
}

/// `MathRoot::AddRoot`: appends the root unless one within `eps_x` is already there.
pub fn add_root(result: &mut MultipleResult, eps_x: f64, root: f64, value: f64) {
    for r in &result.roots {
        if (root - r).abs() < eps_x {
            return;
        }
    }
    result.roots.push(root);
    result.values.push(value);
}

/// `MathRoot::EffectiveXTolerance`.
pub fn effective_x_tolerance(lower: f64, upper: f64, x_tolerance: f64) -> f64 {
    let min_eps_x = 1.0e-10 * (lower.abs() + upper.abs());
    cmax(x_tolerance, min_eps_x)
}

/// `MathRoot::MultipleDerivativeValueWrapper`: the derivative of a function as a scalar function.
struct DerivativeValueWrapper<'a, F: ?Sized> {
    func: &'a mut F,
}
impl<F: ScalarFunctionWithDerivative + ?Sized> ScalarFunction for DerivativeValueWrapper<'_, F> {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.func.values(x).map(|(_, dy)| dy)
    }
}

/// `MathRoot::MultipleBrentValueWrapper`: `f(x) - offset`.
struct BrentValueWrapper<'a, F: ?Sized> {
    func: &'a mut F,
    offset: f64,
}
impl<F: ScalarFunction + ?Sized> ScalarFunction for BrentValueWrapper<'_, F> {
    fn value(&mut self, x: f64) -> Option<f64> {
        let y = self.func.value(x)?;
        Some(y - self.offset)
    }
}

/// `MathRoot::EvaluateShiftedValue`.
fn eval_shifted<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, offset: f64, out: &mut f64) -> bool {
    if !val(f, x, out) {
        return false;
    }
    *out -= offset;
    true
}

/// `MathRoot::EvaluateShiftedValues`.
fn eval_shifted_values<F: ScalarFunctionWithDerivative + ?Sized>(
    f: &mut F,
    x: f64,
    offset: f64,
    value: &mut f64,
    derivative: &mut f64,
) -> bool {
    if !vals(f, x, value, derivative) {
        return false;
    }
    *value -= offset;
    true
}

/// `MathRoot::RefineBracketedRoot`.
#[allow(clippy::too_many_arguments)]
pub fn refine_bracketed_root<F: ScalarFunctionWithDerivative + ?Sized>(
    f: &mut F,
    offset: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    tolerance: f64,
    eps_x: f64,
    result: &mut MultipleResult,
) -> bool {
    const MAX_ITERATIONS: i32 = 100;
    const EPS2: f64 = 2.0e-14;
    const DERIV_EPS: f64 = 1.0e-10;
    let tol2 = 0.5 * tolerance;
    let mut a = x1;
    let mut b = x2;
    let mut c = x2;
    let mut d = 0.0;
    let mut e = 0.0;
    let mut fa = y1;
    let mut fb = y2;
    let mut fc = y2;
    for iter in 1..=MAX_ITERATIONS {
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            let prev_a = a;
            let prev_fa = fa;
            a = b;
            b = c;
            c = prev_a;
            fa = fb;
            fb = fc;
            fc = prev_fa;
        }
        let tol1 = EPS2 * b.abs() + tol2;
        let xm = 0.5 * (c - b);
        if xm.abs() < tol1 || fb == 0.0 {
            let mut newton_x = b;
            for _ in 0..5 {
                let mut y = 0.0;
                let mut dfdx = 0.0;
                if !eval_shifted_values(f, newton_x, offset, &mut y, &mut dfdx) {
                    return false;
                }
                if dfdx.abs() <= DERIV_EPS {
                    break;
                }
                newton_x -= y / dfdx;
                if newton_x < x1 || newton_x > x2 {
                    break;
                }
                if !eval_shifted(f, newton_x, offset, &mut y) {
                    return false;
                }
                if y.abs() < fb.abs() {
                    b = newton_x;
                    fb = y;
                }
            }
            let mut root_value = 0.0;
            if !val(f, b, &mut root_value) {
                return false;
            }
            result.nb_iterations += iter as usize;
            add_root(result, eps_x, b, root_value);
            return true;
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let mut p;
            let mut q;
            let s = fb / fa;
            if a == c {
                p = 2.0 * xm * s;
                q = 1.0 - s;
            } else {
                q = fa / fc;
                let r = fb / fc;
                p = s * (2.0 * xm * q * (q - r) - (b - a) * (r - 1.0));
                q = (q - 1.0) * (r - 1.0) * (s - 1.0);
            }
            if p > 0.0 {
                q = -q;
            }
            p = p.abs();
            let min1 = 3.0 * xm * q - (tol1 * q).abs();
            let min2 = (e * q).abs();
            if 2.0 * p < cmin(min1, min2) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        if d.abs() > tol1 {
            b += d;
        } else {
            b += if xm >= 0.0 { tol1.abs() } else { -tol1.abs() };
        }
        if !eval_shifted(f, b, offset, &mut fb) {
            return false;
        }
    }
    result.nb_iterations += MAX_ITERATIONS as usize;
    true
}

fn vget(v: &Vector, i: i32) -> R<f64> {
    v.value(i)
}

/// `MathRoot::FindAllRootsWithDerivative`.
pub fn find_all_roots_with_derivative<F: ScalarFunctionWithDerivative + ?Sized>(
    f: &mut F,
    lower_in: f64,
    upper_in: f64,
    config: &MultipleConfig,
) -> R<MultipleResult> {
    let mut result = MultipleResult { status: Status::Ok, ..MultipleResult::default() };
    let lower = cmin(lower_in, upper_in);
    let upper = cmax(lower_in, upper_in);
    let nb_samples = i32::max(2 * config.nb_samples, 20);
    let dx = (upper - lower) / f64::from(nb_samples);
    let eps_x = effective_x_tolerance(lower, upper, config.x_tolerance);
    let raw_x_tolerance = config.x_tolerance;
    let major_dx = 5.0 * dx;
    let mut values = Vector::new(0, nb_samples)?;
    let mut x = lower;
    for i in 0..=nb_samples {
        if x > upper {
            x = upper;
        }
        let mut v = 0.0;
        let ok = eval_shifted(f, x, config.offset, &mut v);
        values.set_value(i, v)?;
        if !ok {
            result.status = Status::NumericalError;
            return Ok(result);
        }
        x += dx;
    }
    result.is_all_null = true;
    for i in 0..=nb_samples {
        let v = vget(&values, i)?;
        if v > config.null_tolerance || v < -config.null_tolerance {
            result.is_all_null = false;
            break;
        }
    }
    if result.is_all_null {
        return Ok(result);
    }
    let tolerance = eps_x;
    let mut x1 = lower;
    for i in 0..nb_samples {
        let mut x2 = x1 + dx;
        if x2 > upper {
            x2 = upper;
        }
        let (vi, vip1) = (vget(&values, i)?, vget(&values, i + 1)?);
        if ((vi < 0.0 && vip1 > 0.0) || (vi > 0.0 && vip1 < 0.0))
            && !refine_bracketed_root(f, config.offset, x1, vi, x2, vip1, tolerance, eps_x, &mut result)
        {
            result.status = Status::NumericalError;
            return Ok(result);
        }
        x1 += dx;
    }
    for i in 0..=nb_samples {
        if vget(&values, i)? != 0.0 {
            continue;
        }
        let zero_x = cmin(lower + f64::from(i) * dx, upper);
        let mut left_x = zero_x - 0.5 * dx;
        let mut right_x = zero_x + 0.5 * dx;
        if left_x < lower {
            left_x = lower;
        }
        if left_x > upper {
            left_x = upper;
        }
        if right_x < lower {
            right_x = lower;
        }
        if right_x > upper {
            right_x = upper;
        }
        let (mut left_y, mut right_y) = (0.0, 0.0);
        if !eval_shifted(f, left_x, config.offset, &mut left_y)
            || !eval_shifted(f, right_x, config.offset, &mut right_y)
        {
            result.status = Status::NumericalError;
            return Ok(result);
        }
        if left_y * right_y < 0.0 {
            if !refine_bracketed_root(f, config.offset, left_x, left_y, right_x, right_y, tolerance, eps_x, &mut result)
            {
                result.status = Status::NumericalError;
                return Ok(result);
            }
        } else if left_y != 0.0 || right_y != 0.0 {
            let mut root_value = 0.0;
            if !val(f, zero_x, &mut root_value) {
                result.status = Status::NumericalError;
                return Ok(result);
            }
            add_root(&mut result, eps_x, zero_x, root_value);
        }
    }
    let v0 = vget(&values, 0)?;
    if v0 <= config.f_tolerance && v0 >= -config.f_tolerance {
        let mut root_value = 0.0;
        if !val(f, lower, &mut root_value) {
            result.status = Status::NumericalError;
            return Ok(result);
        }
        add_root(&mut result, eps_x, lower, root_value);
    }
    let vn = vget(&values, nb_samples)?;
    if vn <= config.f_tolerance && vn >= -config.f_tolerance {
        let mut root_value = 0.0;
        if !val(f, upper, &mut root_value) {
            result.status = Status::NumericalError;
            return Ok(result);
        }
        add_root(&mut result, eps_x, upper, root_value);
    }
    let mut mid_x = lower + dx;
    for i in 1..nb_samples {
        if mid_x > upper {
            mid_x = upper;
        }
        let mut is_rediscretize = false;
        let vi = vget(&values, i)?;
        let (vm, vp) = (vget(&values, i - 1)?, vget(&values, i + 1)?);
        if (vi > 0.0 && vm > vi && vp > vi) || (vi < 0.0 && vm < vi && vp < vi) {
            let mut probe_x = cmax(lower, mid_x - dx);
            let (mut probe_y, mut probe_dy) = (0.0, 0.0);
            if !eval_shifted_values(f, probe_x, config.offset, &mut probe_y, &mut probe_dy) {
                result.status = Status::NumericalError;
                return Ok(result);
            }
            if probe_dy.abs() > 1.0e-10 {
                let step = probe_y / probe_dy;
                if step < major_dx && step > -major_dx {
                    is_rediscretize = true;
                }
            }
            if !is_rediscretize {
                probe_x = cmin(upper, mid_x + dx);
                if !eval_shifted_values(f, probe_x, config.offset, &mut probe_y, &mut probe_dy) {
                    result.status = Status::NumericalError;
                    return Ok(result);
                }
                if probe_dy.abs() > 1.0e-10 {
                    let step = probe_y / probe_dy;
                    if step < major_dx && step > -major_dx {
                        is_rediscretize = true;
                    }
                }
            }
        }
        if is_rediscretize {
            let mut x0 = cmax(lower, mid_x - dx);
            let mut x3 = cmin(upper, mid_x + dx);
            let mut root1 = 0.0;
            let mut root2 = 0.0;
            let mut val1 = 0.0;
            let mut val2 = 0.0;
            let mut der1 = 0.0;
            let mut der2 = 0.0;
            let mut has_root1 = false;
            let mut has_root2 = false;
            let derivative_config = Config {
                x_tolerance: raw_x_tolerance,
                f_tolerance: 0.0,
                max_iterations: config.max_iterations,
                ..Config::default()
            };
            let derivative_root = {
                let mut wrapper = DerivativeValueWrapper { func: &mut *f };
                brent(&mut wrapper, x0, x3, &derivative_config)
            };
            result.nb_iterations += derivative_root.nb_iterations;
            if derivative_root.is_done()
                && let Some(r1) = derivative_root.root
            {
                root1 = r1;
                let mut original_value = 0.0;
                if !val(f, root1, &mut original_value) {
                    result.status = Status::NumericalError;
                    return Ok(result);
                }
                val1 = (original_value - config.offset).abs();
                if val1 < config.f_tolerance {
                    has_root1 = true;
                    if !eval_shifted_values(f, root1, config.offset, &mut original_value, &mut der1) {
                        result.status = Status::NumericalError;
                        return Ok(result);
                    }
                }
            }
            let golden_inv_ratio = 1.0 / mathutils::THE_GOLDEN_RATIO;
            let golden_inv_comp = 1.0 - golden_inv_ratio;
            let tol_cr = eps_x * 10.0;
            let local_tol_x = 0.001 * eps_x;
            let mut f0 = vget(&values, i - 1)?;
            let mut f3 = vget(&values, i + 1)?;
            let is_search_minimum = f0 > 0.0;
            let mut x_probe1;
            let mut x_probe2;
            if (x3 - mid_x).abs() > (x0 - mid_x).abs() {
                x_probe1 = mid_x;
                x_probe2 = mid_x + golden_inv_comp * (x3 - mid_x);
            } else {
                x_probe2 = mid_x;
                x_probe1 = mid_x - golden_inv_comp * (mid_x - x0);
            }
            let (mut f1, mut f2) = (0.0, 0.0);
            if !eval_shifted(f, x_probe1, config.offset, &mut f1) || !eval_shifted(f, x_probe2, config.offset, &mut f2)
            {
                result.status = Status::NumericalError;
                return Ok(result);
            }
            while (x3 - x0).abs() > tol_cr * (x_probe1.abs() + x_probe2.abs())
                && (x_probe1 - x_probe2).abs() > local_tol_x
            {
                let go_right = if is_search_minimum { f2 < f1 } else { f2 > f1 };
                if go_right {
                    x0 = x_probe1;
                    x_probe1 = x_probe2;
                    x_probe2 = golden_inv_ratio * x_probe1 + golden_inv_comp * x3;
                    f0 = f1;
                    f1 = f2;
                    if !eval_shifted(f, x_probe2, config.offset, &mut f2) {
                        result.status = Status::NumericalError;
                        return Ok(result);
                    }
                } else {
                    x3 = x_probe2;
                    x_probe2 = x_probe1;
                    x_probe1 = golden_inv_ratio * x_probe2 + golden_inv_comp * x0;
                    f3 = f2;
                    f2 = f1;
                    if !eval_shifted(f, x_probe1, config.offset, &mut f1) {
                        result.status = Status::NumericalError;
                        return Ok(result);
                    }
                }
                if f1 * f0 < 0.0
                    && !refine_bracketed_root(f, config.offset, x0, f0, x_probe1, f1, tolerance, eps_x, &mut result)
                {
                    result.status = Status::NumericalError;
                    return Ok(result);
                }
                if f2 * f3 < 0.0
                    && !refine_bracketed_root(f, config.offset, x_probe2, f2, x3, f3, tolerance, eps_x, &mut result)
                {
                    result.status = Status::NumericalError;
                    return Ok(result);
                }
            }
            if (is_search_minimum && f1 < f2) || (!is_search_minimum && f1 > f2) {
                if f1.abs() < config.f_tolerance {
                    has_root2 = true;
                    root2 = x_probe1;
                    val2 = f1.abs();
                }
            } else if f2.abs() < config.f_tolerance {
                has_root2 = true;
                root2 = x_probe2;
                val2 = f2.abs();
            }
            let mut chosen: Option<f64> = None;
            if has_root1 && has_root2 {
                if val2 - val1 > config.f_tolerance {
                    chosen = Some(root1);
                } else if val1 - val2 > config.f_tolerance {
                    chosen = Some(root2);
                } else {
                    let mut shifted_value = 0.0;
                    if !eval_shifted_values(f, root2, config.offset, &mut shifted_value, &mut der2) {
                        result.status = Status::NumericalError;
                        return Ok(result);
                    }
                    chosen = Some(if der1.abs() < der2.abs() { root1 } else { root2 });
                }
            } else if has_root1 {
                chosen = Some(root1);
            } else if has_root2 {
                chosen = Some(root2);
            }
            if let Some(chosen_root) = chosen {
                let mut root_value = 0.0;
                if !val(f, chosen_root, &mut root_value) {
                    result.status = Status::NumericalError;
                    return Ok(result);
                }
                add_root(&mut result, eps_x, chosen_root, root_value);
            }
        }
        mid_x += dx;
    }
    sort_roots(&mut result);
    Ok(result)
}

// ---------------------------------------------------------------------------------------------
// MathRoot_Multiple.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::FindAllRoots` with a configuration (`FindAllRootsImpl` with the plain sampling functors).
pub fn find_all_roots<F: ScalarFunction + ?Sized>(
    f: &mut F,
    lower_in: f64,
    upper_in: f64,
    config: &MultipleConfig,
) -> R<MultipleResult> {
    let nb_samples = i32::max(2 * config.nb_samples, 20);
    let mut samples = Vector::new(0, nb_samples)?;
    let mut result = MultipleResult { status: Status::Ok, ..MultipleResult::default() };
    let lower = cmin(lower_in, upper_in);
    let upper = cmax(lower_in, upper_in);
    let dx = (upper - lower) / f64::from(nb_samples);
    let eps_x = effective_x_tolerance(lower, upper, config.x_tolerance);
    let mut x_values = Vector::new(0, nb_samples)?;
    for i in 0..=nb_samples {
        let mut x = lower + f64::from(i) * dx;
        if x > upper {
            x = upper;
        }
        x_values.set_value(i, x)?;
        let mut fv = 0.0;
        if !val(f, x, &mut fv) {
            result.status = Status::NumericalError;
            return Ok(result);
        }
        samples.set_value(i, fv - config.offset)?;
    }
    result.is_all_null = true;
    for i in 0..=nb_samples {
        if vget(&samples, i)?.abs() > config.null_tolerance {
            result.is_all_null = false;
            break;
        }
    }
    if result.is_all_null {
        return Ok(result);
    }
    for i in 0..nb_samples {
        let f0 = vget(&samples, i)?;
        let f1 = vget(&samples, i + 1)?;
        let x0 = vget(&x_values, i)?;
        let x1 = vget(&x_values, i + 1)?;
        if f0.abs() < config.f_tolerance {
            add_root(&mut result, eps_x, x0, f0 + config.offset);
            continue;
        }
        if f0 * f1 < 0.0 {
            let brent_config = Config {
                x_tolerance: eps_x,
                f_tolerance: config.f_tolerance,
                max_iterations: config.max_iterations,
                ..Config::default()
            };
            let brent_result = {
                let mut wrapper = BrentValueWrapper { func: &mut *f, offset: config.offset };
                brent(&mut wrapper, x0, x1, &brent_config)
            };
            result.nb_iterations += brent_result.nb_iterations;
            if brent_result.is_done()
                && let Some(root) = brent_result.root
            {
                let mut root_value = 0.0;
                val(f, root, &mut root_value);
                add_root(&mut result, eps_x, root, root_value);
            }
        }
    }
    let last = vget(&samples, nb_samples)?;
    if last.abs() < config.f_tolerance {
        add_root(&mut result, eps_x, vget(&x_values, nb_samples)?, last + config.offset);
    }
    sort_roots(&mut result);
    Ok(result)
}

/// `MathRoot::FindAllRoots(theFunc, theLower, theUpper, int theNbSamples)`.
pub fn find_all_roots_nb_samples<F: ScalarFunction + ?Sized>(
    f: &mut F,
    lower: f64,
    upper: f64,
    nb_samples: i32,
) -> R<MultipleResult> {
    let config = MultipleConfig { nb_samples, ..MultipleConfig::default() };
    find_all_roots(f, lower, upper, &config)
}

// ---------------------------------------------------------------------------------------------
// MathRoot_All.hxx
// ---------------------------------------------------------------------------------------------

/// `MathRoot::NullInterval`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NullInterval {
    pub a: f64,
    pub b: f64,
    pub state: i32,
}

/// `MathRoot::AllRootsResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct AllRootsResult {
    pub status: Status,
    pub roots: Vec<f64>,
    pub root_states: Vec<i32>,
    pub null_intervals: Vec<NullInterval>,
}
impl Default for AllRootsResult {
    fn default() -> Self {
        Self { status: Status::NotConverged, roots: Vec::new(), root_states: Vec::new(), null_intervals: Vec::new() }
    }
}
impl AllRootsResult {
    /// `IsDone()` (and the C++ `explicit operator bool`).
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
    pub fn nb_roots(&self) -> i32 {
        self.roots.len() as i32
    }
    pub fn nb_intervals(&self) -> i32 {
        self.null_intervals.len() as i32
    }
}

/// `MathRoot::FindMultipleRoots` (default `offset` 0).
pub fn find_multiple_roots<F: ScalarFunction + ?Sized>(
    f: &mut F,
    a: f64,
    b: f64,
    nb_samples: i32,
    eps_x: f64,
    eps_f: f64,
    offset: f64,
) -> R<MultipleResult> {
    let config =
        MultipleConfig { nb_samples, x_tolerance: eps_x, f_tolerance: eps_f, offset, ..MultipleConfig::default() };
    find_all_roots(f, a, b, &config)
}

fn append_roots(result: &mut AllRootsResult, found: &MultipleResult) {
    if found.is_done() {
        for r in &found.roots {
            result.roots.push(*r);
            result.root_states.push(0);
        }
    }
}

/// `MathRoot::FindAllRootsWithIntervals` over the given sample points (defaults `1e-10` for the three tolerances).
pub fn find_all_roots_with_intervals<F: ScalarFunction + ?Sized>(
    f: &mut F,
    samples: &Vector,
    eps_x: f64,
    eps_f: f64,
    eps_nul: f64,
) -> R<AllRootsResult> {
    let mut result = AllRootsResult::default();
    let nbp = samples.length();
    if nbp < 2 {
        result.status = Status::InvalidInput;
        return Ok(result);
    }
    let lower = samples.lower();
    let mut val_ = 0.0;
    let mut prev_val = 0.0;
    if !val(f, samples.value(lower)?, &mut prev_val) {
        result.status = Status::NotConverged;
        return Ok(result);
    }
    let mut prev_nul = prev_val.abs() <= eps_nul;
    let mut in_interval = false;
    let mut nul_start = false;
    let mut nul_end = false;
    let mut deb_nul = 0.0;
    let mut fin_nul;
    let mut val_sav = prev_val;
    let mut interval_starts: Vec<f64> = Vec::new();
    let mut interval_ends: Vec<f64> = Vec::new();
    for i in 1..nbp {
        if !val(f, samples.value(lower + i)?, &mut val_) {
            result.status = Status::NotConverged;
            return Ok(result);
        }
        let cur_nul = val_.abs() <= eps_nul;
        if !cur_nul {
            val_sav = val_;
        }
        if in_interval && !cur_nul {
            in_interval = false;
            interval_starts.push(deb_nul);
            let mut cst = if val_ > 0.0 { eps_nul } else { -eps_nul };
            let res = find_multiple_roots(
                f,
                samples.value(lower + i - 1)?,
                samples.value(lower + i)?,
                10,
                eps_x,
                eps_f,
                cst,
            )?;
            fin_nul = match res.roots.first() {
                Some(r) if res.is_done() => *r,
                _ => samples.value(lower + i - 1)?,
            };
            cst = -cst;
            let res2 = find_multiple_roots(
                f,
                samples.value(lower + i - 1)?,
                samples.value(lower + i)?,
                10,
                eps_x,
                eps_f,
                cst,
            )?;
            if res2.is_done()
                && let Some(r) = res2.roots.first()
                && *r < fin_nul
            {
                fin_nul = *r;
            }
            interval_ends.push(fin_nul);
        } else if !in_interval && prev_nul && cur_nul {
            in_interval = true;
            if i == 1 {
                deb_nul = samples.value(lower)?;
                nul_start = true;
            } else {
                let mut cst = if val_sav > 0.0 { eps_nul } else { -eps_nul };
                let res = find_multiple_roots(
                    f,
                    samples.value(lower + i - 2)?,
                    samples.value(lower + i - 1)?,
                    10,
                    eps_x,
                    eps_f,
                    cst,
                )?;
                deb_nul = match res.roots.last() {
                    Some(r) if res.is_done() => *r,
                    _ => samples.value(lower + i - 1)?,
                };
                cst = -cst;
                let res2 = find_multiple_roots(
                    f,
                    samples.value(lower + i - 2)?,
                    samples.value(lower + i - 1)?,
                    10,
                    eps_x,
                    eps_f,
                    cst,
                )?;
                if res2.is_done()
                    && let Some(r) = res2.roots.last()
                    && *r > deb_nul
                {
                    deb_nul = *r;
                }
            }
        }
        prev_nul = cur_nul;
    }
    if in_interval {
        interval_starts.push(deb_nul);
        fin_nul = samples.value(lower + nbp - 1)?;
        interval_ends.push(fin_nul);
        nul_end = true;
    }
    for (a, b) in interval_starts.iter().zip(&interval_ends) {
        result.null_intervals.push(NullInterval { a: *a, b: *b, state: 0 });
    }
    let sample_first = samples.value(lower)?;
    let sample_last = samples.value(lower + nbp - 1)?;
    if interval_starts.is_empty() {
        let res = find_multiple_roots(f, sample_first, sample_last, nbp, eps_x, eps_f, 0.0)?;
        append_roots(&mut result, &res);
    } else {
        // `std::max(3, static_cast<int>(std::abs((end - start) / (last - first)) * nbp))`
        let nb_points = |start: f64, end: f64| {
            i32::max(3, (((end - start) / (sample_last - sample_first)).abs() * f64::from(nbp)) as i32)
        };
        if !nul_start {
            let start = sample_first;
            let end = interval_starts.first().copied().unwrap_or(0.0);
            let res = find_multiple_roots(f, start, end, nb_points(start, end), eps_x, eps_f, 0.0)?;
            append_roots(&mut result, &res);
        }
        for k in 1..interval_starts.len() {
            let start = interval_ends.get(k - 1).copied().unwrap_or(0.0);
            let end = interval_starts.get(k).copied().unwrap_or(0.0);
            let res = find_multiple_roots(f, start, end, nb_points(start, end), eps_x, eps_f, 0.0)?;
            append_roots(&mut result, &res);
        }
        if !nul_end {
            let start = interval_ends.last().copied().unwrap_or(0.0);
            let end = sample_last;
            let res = find_multiple_roots(f, start, end, nb_points(start, end), eps_x, eps_f, 0.0)?;
            append_roots(&mut result, &res);
        }
    }
    result.status = Status::Ok;
    Ok(result)
}

/// `MathRoot::FindAllRootsWithIntervals(theFunc, theA, theB, theNbSamples, ...)`.
pub fn find_all_roots_with_intervals_range<F: ScalarFunction + ?Sized>(
    f: &mut F,
    a: f64,
    b: f64,
    nb_samples: i32,
    eps_x: f64,
    eps_f: f64,
    eps_nul: f64,
) -> R<AllRootsResult> {
    let mut samples = Vector::new(0, nb_samples - 1)?;
    let step = (b - a) / f64::from(nb_samples - 1);
    for i in 0..nb_samples {
        samples.set_value(i, a + f64::from(i) * step)?;
    }
    samples.set_value(nb_samples - 1, b)?;
    find_all_roots_with_intervals(f, &samples, eps_x, eps_f, eps_nul)
}
