//! OCCT's `math` package, one-dimensional solvers: the callback interfaces `math_Function` and
//! `math_FunctionWithDerivative`, `math_FunctionSample`, `math_FunctionRoots`, `math_FunctionRoot`,
//! `math_FunctionAllRoots`, `math_BracketedRoot`, `math_NewtonFunctionRoot`, `math_BracketMinimum`,
//! `math_BrentMinimum`, `math_TrigonometricFunctionRoots` and `math_TrigonometricEquationFunction`.
//!
//! The operations are done in the order of the C++ sources (no fused multiply-add, no reassociation) and
//! the callbacks are called in the same order with the same arguments, so results and call traces agree
//! with OCCT bit for bit. A callback that returns `None` is C++ `return false` and leaves the C++
//! out-parameters untouched.

mod all_roots;
mod bracket_minimum;
mod bracketed_root;
mod brent_minimum;
mod function_root;
mod function_roots;
mod function_set_root;
mod newton_root;
mod trig_roots;

pub use all_roots::FunctionAllRoots;
pub use bracket_minimum::BracketMinimum;
pub use bracketed_root::BracketedRoot;
pub use brent_minimum::{BrentMinimum, BrentSolutionTest, BrentState};
pub use function_root::FunctionRoot;
pub use function_roots::FunctionRoots;
pub use newton_root::NewtonFunctionRoot;
pub use trig_roots::{TrigonometricEquationFunction, TrigonometricFunctionRoots};

use super::mathlin::MathError;

/// `math_Function`: a function of one variable.
pub trait Function {
    /// `math_Function::Value`: `Some(f(x))`, or `None` when the C++ callback returns false.
    fn value(&mut self, x: f64) -> Option<f64>;
    /// `math_Function::GetStateNumber` (default: 0).
    fn state_number(&mut self) -> i32 {
        0
    }
}

/// `math_FunctionWithDerivative`.
pub trait FunctionWithDerivative: Function {
    /// `math_FunctionWithDerivative::Derivative`.
    fn derivative(&mut self, x: f64) -> Option<f64>;
    /// `math_FunctionWithDerivative::Values`: `Some((f(x), f'(x)))`.
    fn values(&mut self, x: f64) -> Option<(f64, f64)>;
}

/// `RealFirst()`.
pub(crate) const REAL_FIRST: f64 = -f64::MAX;
/// `RealLast()`.
pub(crate) const REAL_LAST: f64 = f64::MAX;
/// `Precision::PConfusion()` (evaluated in `f64`).
pub(crate) const P_CONFUSION: f64 = 1.0e-7 * 0.01;
/// `Precision::Infinite()`.
pub(crate) const PRECISION_INFINITE: f64 = 2.0e100;
/// `Precision::SquareConfusion()`.
pub(crate) const SQUARE_CONFUSION: f64 = 1.0e-7 * 1.0e-7;

/// `Precision::IsInfinite`.
pub(crate) fn is_infinite(r: f64) -> bool {
    r.abs() >= 0.5 * PRECISION_INFINITE
}

/// `Epsilon(x)` of `Standard_Real.hxx`.
pub(crate) fn epsilon(x: f64) -> f64 {
    if x >= 0.0 { libm::nextafter(x, f64::MAX) - x } else { x - libm::nextafter(x, -f64::MAX) }
}

/// `std::min(a, b)`: `(b < a) ? b : a`.
pub(crate) fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`: `(a < b) ? b : a`.
pub(crate) fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// A `double` written to a `std::ostream` with the default format (`printf("%g")`).
pub(crate) fn fmt_g(x: f64) -> String {
    if x.is_nan() {
        return if x.is_sign_negative() { "-nan".into() } else { "nan".into() };
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let e = format!("{x:.5e}");
    let Some((mant, exp)) = e.split_once('e') else { return e };
    let exp: i32 = exp.parse().unwrap_or(0);
    let strip = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
    };
    if !(-4..6).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", strip(mant), sign, exp.abs())
    } else {
        let prec = usize::try_from(5 - exp).unwrap_or(0);
        strip(&format!("{x:.prec$}"))
    }
}

/// `math_FunctionSample`: `n` equidistant parameters between `a` and `b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FunctionSample {
    a: f64,
    b: f64,
    n: i32,
}

impl FunctionSample {
    /// Constructor (no check, any `n`).
    pub fn new(a: f64, b: f64, n: i32) -> Self {
        Self { a, b, n }
    }
    /// `Bounds`.
    pub fn bounds(&self) -> (f64, f64) {
        (self.a, self.b)
    }
    /// `NbPoints`.
    pub fn nb_points(&self) -> i32 {
        self.n
    }
    /// `GetParameter`. The C++ has no check and divides by `n - 1`: an index outside `1..=n` is
    /// `Err(Range)` and a sample of fewer than 2 points is `Err(Domain)`.
    pub fn get_parameter(&self, index: i32) -> Result<f64, MathError> {
        if index <= 0 || index > self.n {
            return Err(MathError::Range);
        }
        if self.n < 2 {
            return Err(MathError::Domain);
        }
        Ok(self.parameter(index))
    }
    /// The formula of `GetParameter` without checks.
    pub(crate) fn parameter(&self, index: i32) -> f64 {
        ((f64::from(self.n.wrapping_sub(index))) * self.a + f64::from(index.wrapping_sub(1)) * self.b)
            / f64::from(self.n.wrapping_sub(1))
    }
}

/// The item `index` (1-based) of a sequence; `Err(Range)` outside (`NCollection_Sequence::Value`).
pub(crate) fn seq_get<T: Copy>(v: &[T], index: i32) -> Result<T, MathError> {
    usize::try_from(i64::from(index) - 1).ok().and_then(|k| v.get(k)).copied().ok_or(MathError::Range)
}

/// The item `index` (1-based), or the default when outside (the caller has checked the range).
pub(crate) fn seq_at<T: Copy + Default>(v: &[T], index: i32) -> T {
    seq_get(v, index).unwrap_or_default()
}
