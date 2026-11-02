//! OCCT's `MathUtils` package: scalar/vector helpers, configuration and result types, convergence tests,
//! polynomial utilities, domains, a random generator, numerical differentiation, bracketing, line searches,
//! function objects and Gauss/Kronrod point helpers.
//!
//! Translated from `MathUtils_*.hxx` and `MathUtils_Gauss*.cxx` (OCCT 8.0.1). C++ exceptions (range errors)
//! become `Err(MathError)`; indices are OCCT's absolute indices read through the checked accessors of
//! `mathlin::Vector`/`Matrix`. Inside the function objects (which never throw for well-formed input) out-of-range
//! accesses read as `0.0`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::mathlin::{self, MathError, Matrix, Vector};
use crate::mathlin_pkg;

pub use crate::mathinteg::Status;

type R<T> = Result<T, MathError>;

// ---------------------------------------------------------------------------------------------
// MathUtils_Core.hxx
// ---------------------------------------------------------------------------------------------

pub const THE_EPSILON: f64 = f64::EPSILON;
pub const THE_ZERO_TOL: f64 = 1.0e-15;
pub const THE_PI: f64 = core::f64::consts::PI;
pub const THE_2PI: f64 = core::f64::consts::TAU;
pub const THE_GOLDEN_RATIO: f64 = 1.618033988749895;
pub const THE_GOLDEN_SECTION: f64 = 0.381966011250105;

/// `std::max(a, b)`.
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `std::min(a, b)`.
fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `MathUtils::Clamp`.
pub fn clamp(v: f64, lower: f64, upper: f64) -> f64 {
    if v < lower {
        lower
    } else if v > upper {
        upper
    } else {
        v
    }
}

/// `MathUtils::IsZero` (default tolerance `THE_ZERO_TOL`).
pub fn is_zero(v: f64, tol: f64) -> bool {
    v.abs() < tol
}

/// `MathUtils::IsEqual` (default tolerance `THE_ZERO_TOL`).
pub fn is_equal(a: f64, b: f64, tol: f64) -> bool {
    let diff = (a - b).abs();
    let scale = cmax(cmax(1.0, a.abs()), b.abs());
    diff < tol * scale
}

/// `MathUtils::SafeDiv` (default `0.0`).
pub fn safe_div(n: f64, d: f64, default: f64) -> f64 {
    if is_zero(d, THE_ZERO_TOL) { default } else { n / d }
}

/// `MathUtils::Sign`.
pub fn sign(v: f64) -> i32 {
    if v > THE_ZERO_TOL {
        1
    } else if v < -THE_ZERO_TOL {
        -1
    } else {
        0
    }
}

/// `MathUtils::SignTransfer`.
pub fn sign_transfer(a: f64, b: f64) -> f64 {
    if b >= 0.0 { a.abs() } else { -a.abs() }
}

/// `MathUtils::Sqr`.
pub fn sqr(v: f64) -> f64 {
    v * v
}

/// `MathUtils::Cube`.
pub fn cube(v: f64) -> f64 {
    v * v * v
}

/// `MathUtils::CubeRoot`.
pub fn cube_root(v: f64) -> f64 {
    if v >= 0.0 { libm::cbrt(v) } else { -libm::cbrt(-v) }
}

/// `MathUtils::IsFinite`.
pub fn is_finite(v: f64) -> bool {
    v.is_finite()
}

/// `MathUtils::ComputeScaleFactor`.
pub fn compute_scale_factor(coeffs: &[f64]) -> f64 {
    let mut max_abs = 0.0;
    for c in coeffs {
        max_abs = cmax(max_abs, c.abs());
    }
    if !(THE_ZERO_TOL..=1.0e15).contains(&max_abs) {
        let (_, exp) = libm::frexp(max_abs);
        return libm::ldexp(1.0, -exp + 1);
    }
    1.0
}

/// `MathUtils::DotProduct` (indexes `b` with the range of `a`).
pub fn dot_product(a: &Vector, b: &Vector) -> R<f64> {
    let mut sum = 0.0;
    for i in a.lower()..=a.upper() {
        sum += a.value(i)? * b.value(i)?;
    }
    Ok(sum)
}

/// `MathUtils::VectorNorm`.
pub fn vector_norm(v: &Vector) -> f64 {
    let mut sum = 0.0;
    for &x in v.as_slice() {
        sum += x * x;
    }
    sum.sqrt()
}

/// `MathUtils::VectorInfNorm`.
pub fn vector_inf_norm(v: &Vector) -> f64 {
    let mut m = 0.0;
    for &x in v.as_slice() {
        m = cmax(m, x.abs());
    }
    m
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Config.hxx
// ---------------------------------------------------------------------------------------------

pub const THE_NEWTON_FTOL_SQ: f64 = 1.0e-32;
pub const THE_NEWTON_STEP_TOL_FACTOR: f64 = 1.0e-16;
pub const THE_NEWTON_MAX_ITER: usize = 100;
pub const THE_NEWTON2D_SINGULAR_DET: f64 = 1.0e-25;
pub const THE_NEWTON2D_CRITICAL_GRAD_SQ: f64 = 1.0e-60;
pub const THE_NEWTON2D_MAX_STEP_RATIO: f64 = 0.5;
pub const THE_NEWTON2D_DOMAIN_EXT: f64 = 1.0e-4;
pub const THE_NEWTON2D_STAGNATION_RATIO: f64 = 0.999;
pub const THE_NEWTON2D_STAGNATION_COUNT: i32 = 3;
pub const THE_NEWTON2D_LINE_SEARCH_MAX: i32 = 8;
pub const THE_NEWTON2D_SKIP_LINESEARCH_SQ: f64 = 0.01;
pub const THE_NEWTON2D_STAGNATION_RELAX: f64 = 10.0;
pub const THE_NEWTON2D_BACKTRACK_TRIGGER: f64 = 1.5;
pub const THE_NEWTON2D_BACKTRACK_ACCEPT: f64 = 1.2;
pub const THE_NEWTON2D_MAXITER_RELAX: f64 = 10.0;
pub const THE_HESSIAN_DEGENERACY_REL: f64 = 1.0e-8;
pub const THE_HESSIAN_DEGENERACY_ABS: f64 = 1.0e-20;
pub const THE_ARMIJO_C1: f64 = 1.0e-4;

/// `MathUtils::Config`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    pub max_iterations: i32,
    pub tolerance: f64,
    pub x_tolerance: f64,
    pub f_tolerance: f64,
    pub step_min: f64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1.0e-10,
            x_tolerance: 1.0e-10,
            f_tolerance: 1.0e-10,
            step_min: f64::EPSILON,
        }
    }
}
impl Config {
    /// `Config(theTolerance, theMaxIter)`.
    pub fn new(tolerance: f64, max_iterations: i32) -> Self {
        Self { max_iterations, tolerance, x_tolerance: tolerance, f_tolerance: tolerance, step_min: f64::EPSILON }
    }
}

/// `MathUtils::BoundedConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundedConfig {
    pub base: Config,
    pub lower_bound: f64,
    pub upper_bound: f64,
}
impl Default for BoundedConfig {
    fn default() -> Self {
        Self { base: Config::default(), lower_bound: -f64::MAX, upper_bound: f64::MAX }
    }
}
impl BoundedConfig {
    /// `BoundedConfig(theLower, theUpper, theTolerance, theMaxIter)`.
    pub fn new(lower: f64, upper: f64, tolerance: f64, max_iterations: i32) -> Self {
        Self { base: Config::new(tolerance, max_iterations), lower_bound: lower, upper_bound: upper }
    }
}

/// `MathUtils::NDimConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NDimConfig {
    pub base: Config,
    pub use_bounds: bool,
}
impl NDimConfig {
    /// `NDimConfig(theTolerance, theMaxIter, theUseBounds)`.
    pub fn new(tolerance: f64, max_iterations: i32, use_bounds: bool) -> Self {
        Self { base: Config::new(tolerance, max_iterations), use_bounds }
    }
}

/// `MathUtils::IntegConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegConfig {
    pub initial_order: i32,
    pub max_order: i32,
    pub max_iterations: i32,
    pub tolerance: f64,
}
impl Default for IntegConfig {
    fn default() -> Self {
        Self { initial_order: 15, max_order: 61, max_iterations: 100, tolerance: 1.0e-10 }
    }
}
impl IntegConfig {
    /// `IntegConfig(theTolerance, theMaxIter)`.
    pub fn new(tolerance: f64, max_iterations: i32) -> Self {
        Self { max_iterations, tolerance, ..Self::default() }
    }
}

/// `MathUtils::LinConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinConfig {
    pub singularity_tolerance: f64,
    pub use_pivoting: bool,
}
impl Default for LinConfig {
    fn default() -> Self {
        Self { singularity_tolerance: 1.0e-15, use_pivoting: true }
    }
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Types.hxx
// ---------------------------------------------------------------------------------------------

macro_rules! result_struct {
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $f:ident : $t:ty = $d:expr),* $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            pub status: Status,
            $($(#[$fm])* pub $f: $t,)*
        }
        impl Default for $name {
            fn default() -> Self {
                Self { status: Status::NotConverged, $($f: $d,)* }
            }
        }
        impl $name {
            /// `IsDone()` (and the C++ `explicit operator bool`).
            pub fn is_done(&self) -> bool {
                self.status == Status::Ok
            }
        }
    };
}

result_struct!(
    /// `MathUtils::ScalarResult`.
    ScalarResult {
        nb_iterations: usize = 0,
        root: Option<f64> = None,
        value: Option<f64> = None,
        derivative: Option<f64> = None,
    }
);
result_struct!(
    /// `MathUtils::PolyResult`.
    PolyResult {
        nb_roots: usize = 0,
        roots: [f64; 4] = [0.0; 4],
    }
);
impl PolyResult {
    /// `operator[]`.
    pub fn index(&self, i: usize) -> f64 {
        self.roots.get(i).copied().unwrap_or(0.0)
    }
}
result_struct!(
    /// `MathUtils::VectorResult`.
    VectorResult {
        nb_iterations: usize = 0,
        solution: Option<Vector> = None,
        value: Option<f64> = None,
        gradient: Option<Vector> = None,
        jacobian: Option<Matrix> = None,
    }
);
result_struct!(
    /// `MathUtils::LinearResult`.
    LinearResult {
        solution: Option<Vector> = None,
        determinant: Option<f64> = None,
    }
);
result_struct!(
    /// `MathUtils::LinearMultipleResult`.
    LinearMultipleResult {
        solutions: Option<Matrix> = None,
        determinant: Option<f64> = None,
    }
);
result_struct!(
    /// `MathUtils::EigenResult`.
    EigenResult {
        nb_iterations: usize = 0,
        eigen_values: Option<Vector> = None,
        eigen_vectors: Option<Matrix> = None,
    }
);
result_struct!(
    /// `MathUtils::DecompResult`.
    DecompResult {
        l: Option<Matrix> = None,
        u: Option<Matrix> = None,
        d: Option<Vector> = None,
        determinant: Option<f64> = None,
    }
);
result_struct!(
    /// `MathUtils::IntegResult`.
    IntegResult {
        nb_iterations: usize = 0,
        nb_points: usize = 0,
        value: Option<f64> = None,
        absolute_error: Option<f64> = None,
        relative_error: Option<f64> = None,
    }
);
result_struct!(
    /// `MathUtils::InverseResult`.
    InverseResult {
        inverse: Option<Matrix> = None,
        determinant: Option<f64> = None,
    }
);

// ---------------------------------------------------------------------------------------------
// MathUtils_Convergence.hxx
// ---------------------------------------------------------------------------------------------

/// `MathUtils::IsXConverged`.
pub fn is_x_converged(x_old: f64, x_new: f64, tol: f64) -> bool {
    let diff = (x_new - x_old).abs();
    let scale = cmax(1.0, x_new.abs());
    diff < tol * scale
}

/// `MathUtils::IsFConverged`.
pub fn is_f_converged(f: f64, tol: f64) -> bool {
    f.abs() < tol
}

/// `MathUtils::IsConverged`.
pub fn is_converged(x_old: f64, x_new: f64, f: f64, c: &Config) -> bool {
    is_x_converged(x_old, x_new, c.x_tolerance) || is_f_converged(f, c.f_tolerance)
}

/// `MathUtils::IsMinConverged`.
pub fn is_min_converged(x_old: f64, x_new: f64, f_old: f64, f_new: f64, c: &Config) -> bool {
    if is_x_converged(x_old, x_new, c.x_tolerance) {
        return true;
    }
    let f_diff = (f_new - f_old).abs();
    let f_scale = cmax(1.0, f_new.abs());
    f_diff < c.f_tolerance * f_scale
}

/// `MathUtils::IsVectorConverged` (indexes `new` with the range of `old`).
pub fn is_vector_converged(old: &Vector, new: &Vector, tol: f64) -> R<bool> {
    let mut max_diff = 0.0;
    let mut max_scale = 1.0;
    for i in old.lower()..=old.upper() {
        max_diff = cmax(max_diff, (new.value(i)? - old.value(i)?).abs());
        max_scale = cmax(max_scale, new.value(i)?.abs());
    }
    Ok(max_diff < tol * max_scale)
}

/// `MathUtils::IsGradientConverged`.
pub fn is_gradient_converged(g: &Vector, tol: f64) -> bool {
    let mut norm_sq = 0.0;
    for &x in g.as_slice() {
        norm_sq += sqr(x);
    }
    norm_sq.sqrt() < tol
}

/// `MathUtils::InfinityNorm`.
pub fn infinity_norm(v: &Vector) -> f64 {
    let mut m = 0.0;
    for &x in v.as_slice() {
        m = cmax(m, x.abs());
    }
    m
}

/// `MathUtils::EuclideanNorm`.
pub fn euclidean_norm(v: &Vector) -> f64 {
    let mut s = 0.0;
    for &x in v.as_slice() {
        s += sqr(x);
    }
    s.sqrt()
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Poly.hxx
// ---------------------------------------------------------------------------------------------

fn at(c: &[f64], i: i32) -> f64 {
    usize::try_from(i).ok().and_then(|k| c.get(k)).copied().unwrap_or(0.0)
}

/// `MathUtils::EvalPoly` (ascending coefficients).
pub fn eval_poly(coeffs: &[f64], degree: i32, x: f64) -> f64 {
    let mut result = at(coeffs, degree);
    for i in (0..degree).rev() {
        result = result * x + at(coeffs, i);
    }
    result
}

/// `MathUtils::EvalPolyDeriv`: `(value, derivative)`.
pub fn eval_poly_deriv(coeffs: &[f64], degree: i32, x: f64) -> (f64, f64) {
    let mut value = at(coeffs, degree);
    let mut deriv = 0.0;
    for i in (0..degree).rev() {
        deriv = deriv * x + value;
        value = value * x + at(coeffs, i);
    }
    (value, deriv)
}

/// `MathUtils::EvalPolyDesc` (descending coefficients).
pub fn eval_poly_desc(coeffs: &[f64], degree: i32, x: f64) -> f64 {
    let mut result = at(coeffs, 0);
    for i in 1..=degree {
        result = result * x + at(coeffs, i);
    }
    result
}

/// `MathUtils::RefinePolyRoot` (default `max_iter` 5).
pub fn refine_poly_root(coeffs: &[f64], degree: i32, root: f64, max_iter: i32) -> f64 {
    let mut x = root;
    for _ in 0..max_iter {
        let (f, df) = eval_poly_deriv(coeffs, degree, x);
        if is_zero(df, THE_ZERO_TOL) {
            break;
        }
        let dx = f / df;
        x -= dx;
        if dx.abs() < THE_EPSILON * cmax(1.0, x.abs()) {
            break;
        }
    }
    x
}

/// `MathUtils::RefinePolyRootDesc` (default `max_iter` 5; `degree <= 4`).
pub fn refine_poly_root_desc(coeffs: &[f64], degree: i32, root: f64, max_iter: i32) -> f64 {
    let mut asc = [0.0_f64; 5];
    for i in 0..=degree {
        if let Some(slot) = usize::try_from(i).ok().and_then(|k| asc.get_mut(k)) {
            *slot = at(coeffs, degree - i);
        }
    }
    refine_poly_root(&asc, degree, root, max_iter)
}

/// `MathUtils::SortRoots` (insertion sort; the count is `roots.len()`).
pub fn sort_roots(roots: &mut [f64]) {
    for i in 1..roots.len() {
        let mut j = i;
        while j > 0 && roots.get(j - 1).zip(roots.get(j)).is_some_and(|(a, b)| a > b) {
            roots.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// `MathUtils::RemoveDuplicateRoots` (default `tol` `1e-10`); returns the new count.
pub fn remove_duplicate_roots(roots: &mut [f64], tol: f64) -> usize {
    let count = roots.len();
    if count <= 1 {
        return count;
    }
    let mut new_count = 1;
    for i in 1..count {
        let (Some(&ri), Some(&last)) = (roots.get(i), roots.get(new_count - 1)) else {
            break;
        };
        if (ri - last).abs() > tol {
            if let Some(slot) = roots.get_mut(new_count) {
                *slot = ri;
            }
            new_count += 1;
        }
    }
    new_count
}

/// `MathUtils::DepressCubic`: `(p, q, shift)`.
pub fn depress_cubic(b: f64, c: f64, d: f64) -> (f64, f64, f64) {
    let shift = b / 3.0;
    let b2 = b * b;
    let p = c - b2 / 3.0;
    let q = d - b * c / 3.0 + 2.0 * b2 * b / 27.0;
    (p, q, shift)
}

/// `MathUtils::DepressQuartic`: `(p, q, r, shift)`.
pub fn depress_quartic(b: f64, c: f64, d: f64, e: f64) -> (f64, f64, f64, f64) {
    let shift = b / 4.0;
    let b2 = b * b;
    let b3 = b2 * b;
    let b4 = b2 * b2;
    let p = c - 3.0 * b2 / 8.0;
    let q = d - b * c / 2.0 + b3 / 8.0;
    let r = e - b * d / 4.0 + b2 * c / 16.0 - 3.0 * b4 / 256.0;
    (p, q, r, shift)
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Domain.hxx
// ---------------------------------------------------------------------------------------------

/// `MathUtils::Domain1D`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Domain1D {
    pub min: f64,
    pub max: f64,
}
impl Domain1D {
    pub fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }
    pub fn length(&self) -> f64 {
        self.max - self.min
    }
    pub fn mid(&self) -> f64 {
        (self.min + self.max) * 0.5
    }
    /// `Contains(theU, theTol)` (default tolerance `0.0`).
    pub fn contains(&self, u: f64, tol: f64) -> bool {
        u >= self.min - tol && u <= self.max + tol
    }
    pub fn clamp(&self, u: f64) -> f64 {
        if u < self.min {
            self.min
        } else if u > self.max {
            self.max
        } else {
            u
        }
    }
    /// `IsLarge(theThreshold)` (default `1000.0`).
    pub fn is_large(&self, threshold: f64) -> bool {
        self.length() > threshold
    }
    /// `IsFullPeriod(thePeriod, theTol)` (default tolerance `1e-10`).
    pub fn is_full_period(&self, period: f64, tol: f64) -> bool {
        self.length() >= period - tol
    }
    pub fn lerp(&self, t: f64) -> f64 {
        self.min + t * (self.max - self.min)
    }
    pub fn normalize(&self, u: f64) -> f64 {
        let len = self.length();
        if len > 0.0 { (u - self.min) / len } else { 0.5 }
    }
    /// `IsFinite(theInfLimit)` (default `1e100`).
    pub fn is_finite(&self, inf_limit: f64) -> bool {
        self.min > -inf_limit && self.max < inf_limit
    }
    /// `IsEqual(theOther, theTol)` (default tolerance `1e-10`).
    pub fn is_equal(&self, other: &Self, tol: f64) -> bool {
        (self.min - other.min).abs() <= tol && (self.max - other.max).abs() <= tol
    }
}

/// `MathUtils::Domain2D`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Domain2D {
    pub umin: f64,
    pub umax: f64,
    pub vmin: f64,
    pub vmax: f64,
}
impl Domain2D {
    pub fn new(umin: f64, umax: f64, vmin: f64, vmax: f64) -> Self {
        Self { umin, umax, vmin, vmax }
    }
    /// `Domain2D(theUDomain, theVDomain)`.
    pub fn from_domains(u: &Domain1D, v: &Domain1D) -> Self {
        Self { umin: u.min, umax: u.max, vmin: v.min, vmax: v.max }
    }
    pub fn u(&self) -> Domain1D {
        Domain1D { min: self.umin, max: self.umax }
    }
    pub fn v(&self) -> Domain1D {
        Domain1D { min: self.vmin, max: self.vmax }
    }
    pub fn u_length(&self) -> f64 {
        self.umax - self.umin
    }
    pub fn v_length(&self) -> f64 {
        self.vmax - self.vmin
    }
    pub fn u_mid(&self) -> f64 {
        (self.umin + self.umax) * 0.5
    }
    pub fn v_mid(&self) -> f64 {
        (self.vmin + self.vmax) * 0.5
    }
    /// `Contains(theU, theV, theTol)` (default tolerance `0.0`).
    pub fn contains(&self, u: f64, v: f64, tol: f64) -> bool {
        u >= self.umin - tol && u <= self.umax + tol && v >= self.vmin - tol && v <= self.vmax + tol
    }
    /// `Clamp(theU, theV)`: the clamped `(u, v)`.
    pub fn clamp(&self, u: f64, v: f64) -> (f64, f64) {
        let cu = if u < self.umin {
            self.umin
        } else if u > self.umax {
            self.umax
        } else {
            u
        };
        let cv = if v < self.vmin {
            self.vmin
        } else if v > self.vmax {
            self.vmax
        } else {
            v
        };
        (cu, cv)
    }
    /// `IsLarge(theThreshold)` (default `1000.0`).
    pub fn is_large(&self, threshold: f64) -> bool {
        self.u_length() > threshold && self.v_length() > threshold
    }
    pub fn is_u_full_period(&self, period: f64, tol: f64) -> bool {
        self.u_length() >= period - tol
    }
    pub fn is_v_full_period(&self, period: f64, tol: f64) -> bool {
        self.v_length() >= period - tol
    }
    /// `IsFinite(theInfLimit)` (default `1e100`).
    pub fn is_finite(&self, inf_limit: f64) -> bool {
        self.umin > -inf_limit && self.umax < inf_limit && self.vmin > -inf_limit && self.vmax < inf_limit
    }
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Random.hxx
// ---------------------------------------------------------------------------------------------

/// `MathUtils::RandomGenerator` (xoshiro256** seeded with splitmix64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomGenerator {
    state: [u64; 4],
}
impl Default for RandomGenerator {
    fn default() -> Self {
        Self::new(1)
    }
}
impl RandomGenerator {
    pub fn new(seed: u64) -> Self {
        let mut g = Self { state: [0; 4] };
        g.set_seed(seed);
        g
    }
    fn splitmix64(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    pub fn set_seed(&mut self, seed: u64) {
        let mut s = seed;
        for slot in &mut self.state {
            *slot = Self::splitmix64(&mut s);
        }
    }
    pub fn next_int(&mut self) -> u64 {
        let [s0, s1, s2, s3] = self.state;
        let result = s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s1 << 17;
        let s2 = s2 ^ s0;
        let s3 = s3 ^ s1;
        let s1 = s1 ^ s2;
        let s0 = s0 ^ s3;
        let s2 = s2 ^ t;
        let s3 = s3.rotate_left(45);
        self.state = [s0, s1, s2, s3];
        result
    }
    pub fn next_real(&mut self) -> f64 {
        (self.next_int() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
}

// ---------------------------------------------------------------------------------------------
// Callback traits
// ---------------------------------------------------------------------------------------------

/// A function `x -> y` (C++ `bool Value(double, double&)`; `None` is `false`).
pub trait ScalarFunction {
    fn value(&mut self, x: f64) -> Option<f64>;
}
/// C++ `bool Values(double, double&, double&)`: `(y, dy)`.
pub trait ScalarFunctionWithDerivative: ScalarFunction {
    fn values(&mut self, x: f64) -> Option<(f64, f64)>;
}
/// A function of a vector (C++ `bool Value(const math_Vector&, double&)`).
pub trait VectorFunction {
    fn value(&mut self, x: &Vector) -> Option<f64>;
}
/// A function of a vector with a gradient.
pub trait VectorFunctionWithGradient: VectorFunction {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool;
    /// `Values(x, y, g)`: the value, then the gradient.
    fn values(&mut self, x: &Vector, y: &mut f64, g: &mut Vector) -> bool {
        match self.value(x) {
            Some(v) => {
                *y = v;
                self.gradient(x, g)
            }
            None => false,
        }
    }
}
/// A system of equations (C++ `NbEquations()` and `bool Value(const math_Vector&, math_Vector&)`).
pub trait SystemFunction {
    fn nb_equations(&self) -> i32;
    fn value(&mut self, x: &Vector, f: &mut Vector) -> bool;
}

impl<T: ScalarFunction + ?Sized> ScalarFunction for Box<T> {
    fn value(&mut self, x: f64) -> Option<f64> {
        (**self).value(x)
    }
}
impl<T: VectorFunction + ?Sized> VectorFunction for Box<T> {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        (**self).value(x)
    }
}

/// `out = f(x)` with the C++ out-parameter convention (`out` untouched on failure).
fn sval<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, out: &mut f64) -> bool {
    match f.value(x) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

/// `out = f(x)` for a vector function.
fn vval<F: VectorFunction + ?Sized>(f: &mut F, x: &Vector, out: &mut f64) -> bool {
    match f.value(x) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Deriv.hxx
// ---------------------------------------------------------------------------------------------

/// `MathUtils::CentralDifference` (default step `1e-8`).
pub fn central_difference<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, step: f64) -> Option<f64> {
    let (mut f_plus, mut f_minus) = (0.0, 0.0);
    if !sval(f, x + step, &mut f_plus) {
        return None;
    }
    if !sval(f, x - step, &mut f_minus) {
        return None;
    }
    Some((f_plus - f_minus) / (2.0 * step))
}

/// `MathUtils::ForwardDifference` (default step `1e-8`).
pub fn forward_difference<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, fx: f64, step: f64) -> Option<f64> {
    let mut f_plus = 0.0;
    if !sval(f, x + step, &mut f_plus) {
        return None;
    }
    Some((f_plus - fx) / step)
}

/// `MathUtils::NumericalGradient` (default step `1e-8`).
pub fn numerical_gradient<F: VectorFunction + ?Sized>(
    f: &mut F,
    x: &mut Vector,
    grad: &mut Vector,
    step: f64,
) -> R<bool> {
    let (lower, upper) = (x.lower(), x.upper());
    for i in lower..=upper {
        let xi = x.value(i)?;
        let (mut f_plus, mut f_minus) = (0.0, 0.0);
        x.set_value(i, xi + step)?;
        if !vval(f, x, &mut f_plus) {
            x.set_value(i, xi)?;
            return Ok(false);
        }
        x.set_value(i, xi - step)?;
        if !vval(f, x, &mut f_minus) {
            x.set_value(i, xi)?;
            return Ok(false);
        }
        x.set_value(i, xi)?;
        grad.set_value(i, (f_plus - f_minus) / (2.0 * step))?;
    }
    Ok(true)
}

/// `MathUtils::NumericalGradientAdaptive` (default relative step `1e-8`).
pub fn numerical_gradient_adaptive<F: VectorFunction + ?Sized>(
    f: &mut F,
    x: &mut Vector,
    grad: &mut Vector,
    rel_step: f64,
) -> R<bool> {
    let (lower, upper) = (x.lower(), x.upper());
    for i in lower..=upper {
        let xi = x.value(i)?;
        let step = rel_step * cmax(1.0, xi.abs());
        let (mut f_plus, mut f_minus) = (0.0, 0.0);
        x.set_value(i, xi + step)?;
        if !vval(f, x, &mut f_plus) {
            x.set_value(i, xi)?;
            return Ok(false);
        }
        x.set_value(i, xi - step)?;
        if !vval(f, x, &mut f_minus) {
            x.set_value(i, xi)?;
            return Ok(false);
        }
        x.set_value(i, xi)?;
        grad.set_value(i, (f_plus - f_minus) / (2.0 * step))?;
    }
    Ok(true)
}

/// `MathUtils::NumericalJacobian` (default step `1e-8`); `jac` is `rows x cols` with 1-based indices.
pub fn numerical_jacobian<F: SystemFunction + ?Sized>(
    f: &mut F,
    x: &mut Vector,
    jac: &mut Matrix,
    step: f64,
) -> R<bool> {
    let nb_rows = jac.row_number();
    let nb_cols = jac.col_number();
    let mut f_plus = Vector::new(1, nb_rows)?;
    let mut f_minus = Vector::new(1, nb_rows)?;
    for j in 1..=nb_cols {
        let idx = x.lower() + j - 1;
        let xj = x.value(idx)?;
        x.set_value(idx, xj + step)?;
        if !f.value(x, &mut f_plus) {
            x.set_value(idx, xj)?;
            return Ok(false);
        }
        x.set_value(idx, xj - step)?;
        if !f.value(x, &mut f_minus) {
            x.set_value(idx, xj)?;
            return Ok(false);
        }
        x.set_value(idx, xj)?;
        for i in 1..=nb_rows {
            jac.set_value(i, j, (f_plus.value(i)? - f_minus.value(i)?) / (2.0 * step))?;
        }
    }
    Ok(true)
}

/// `MathUtils::NumericalHessian` (default step `1e-5`); `hess` is `n x n` with 1-based indices.
pub fn numerical_hessian<F: VectorFunction + ?Sized>(
    f: &mut F,
    x: &mut Vector,
    hess: &mut Matrix,
    step: f64,
) -> R<bool> {
    let (lower, upper) = (x.lower(), x.upper());
    let mut fx = 0.0;
    if !vval(f, x, &mut fx) {
        return Ok(false);
    }
    for i in lower..=upper {
        let xi = x.value(i)?;
        let (mut f_plus, mut f_minus) = (0.0, 0.0);
        x.set_value(i, xi + step)?;
        if !vval(f, x, &mut f_plus) {
            x.set_value(i, xi)?;
            return Ok(false);
        }
        x.set_value(i, xi - step)?;
        if !vval(f, x, &mut f_minus) {
            x.set_value(i, xi)?;
            return Ok(false);
        }
        x.set_value(i, xi)?;
        let mat_idx = i - lower + 1;
        hess.set_value(mat_idx, mat_idx, (f_plus - 2.0 * fx + f_minus) / (step * step))?;
    }
    for i in lower..=upper {
        for j in i + 1..=upper {
            let xi = x.value(i)?;
            let xj = x.value(j)?;
            let (mut fpp, mut fpm, mut fmp, mut fmm) = (0.0, 0.0, 0.0, 0.0);
            x.set_value(i, xi + step)?;
            x.set_value(j, xj + step)?;
            if !vval(f, x, &mut fpp) {
                x.set_value(i, xi)?;
                x.set_value(j, xj)?;
                return Ok(false);
            }
            x.set_value(j, xj - step)?;
            if !vval(f, x, &mut fpm) {
                x.set_value(i, xi)?;
                x.set_value(j, xj)?;
                return Ok(false);
            }
            x.set_value(i, xi - step)?;
            if !vval(f, x, &mut fmm) {
                x.set_value(i, xi)?;
                x.set_value(j, xj)?;
                return Ok(false);
            }
            x.set_value(j, xj + step)?;
            if !vval(f, x, &mut fmp) {
                x.set_value(i, xi)?;
                x.set_value(j, xj)?;
                return Ok(false);
            }
            x.set_value(i, xi)?;
            x.set_value(j, xj)?;
            let mat_i = i - lower + 1;
            let mat_j = j - lower + 1;
            let hij = (fpp - fpm - fmp + fmm) / (4.0 * step * step);
            hess.set_value(mat_i, mat_j, hij)?;
            hess.set_value(mat_j, mat_i, hij)?;
        }
    }
    Ok(true)
}

/// `MathUtils::SecondDerivative` (default step `1e-5`).
pub fn second_derivative<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, fx: f64, step: f64) -> Option<f64> {
    let (mut f_plus, mut f_minus) = (0.0, 0.0);
    if !sval(f, x + step, &mut f_plus) {
        return None;
    }
    if !sval(f, x - step, &mut f_minus) {
        return None;
    }
    Some((f_plus - 2.0 * fx + f_minus) / (step * step))
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Bracket.hxx
// ---------------------------------------------------------------------------------------------

/// `MathUtils::BracketResult`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BracketResult {
    pub is_valid: bool,
    pub a: f64,
    pub b: f64,
    pub fa: f64,
    pub fb: f64,
}

/// `MathUtils::BracketRoot` (default `max_iter` 50).
pub fn bracket_root<F: ScalarFunction + ?Sized>(f: &mut F, a: f64, b: f64, max_iter: i32) -> BracketResult {
    let mut r = BracketResult { a, b, ..BracketResult::default() };
    if !sval(f, r.a, &mut r.fa) {
        return r;
    }
    if !sval(f, r.b, &mut r.fb) {
        return r;
    }
    for _ in 0..max_iter {
        if r.fa * r.fb < 0.0 {
            r.is_valid = true;
            if r.a > r.b {
                core::mem::swap(&mut r.a, &mut r.b);
                core::mem::swap(&mut r.fa, &mut r.fb);
            }
            return r;
        }
        if r.fa.abs() < r.fb.abs() {
            r.a += THE_GOLDEN_RATIO * (r.a - r.b);
            if !sval(f, r.a, &mut r.fa) {
                return r;
            }
        } else {
            r.b += THE_GOLDEN_RATIO * (r.b - r.a);
            if !sval(f, r.b, &mut r.fb) {
                return r;
            }
        }
    }
    r
}

/// `MathUtils::MinBracketResult`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MinBracketResult {
    pub is_valid: bool,
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub fa: f64,
    pub fb: f64,
    pub fc: f64,
}

/// `MathUtils::MinBracketOptions`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinBracketOptions {
    pub max_iterations: i32,
    pub use_limits: bool,
    pub left_limit: f64,
    pub right_limit: f64,
    pub has_fa: bool,
    pub has_fb: bool,
    pub fa: f64,
    pub fb: f64,
}
impl Default for MinBracketOptions {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            use_limits: false,
            left_limit: 0.0,
            right_limit: 0.0,
            has_fa: false,
            has_fb: false,
            fa: 0.0,
            fb: 0.0,
        }
    }
}

/// `detail::Limited`.
fn limited(value: f64, o: &MinBracketOptions) -> f64 {
    if !o.use_limits {
        return value;
    }
    cmax(o.left_limit, cmin(o.right_limit, value))
}

/// `detail::LimitAndMayBeSwap`.
fn limit_and_maybe_swap<F: ScalarFunction + ?Sized>(
    f: &mut F,
    o: &MinBracketOptions,
    a: f64,
    b: &mut f64,
    fb: &mut f64,
    c: &mut f64,
    fc: &mut f64,
) -> bool {
    *c = limited(*c, o);
    if (*b - *c).abs() < THE_ZERO_TOL {
        return false;
    }
    if !sval(f, *c, fc) {
        return false;
    }
    if (a - *b) * (*b - *c) < 0.0 {
        core::mem::swap(b, c);
        core::mem::swap(fb, fc);
    }
    true
}

/// `MathUtils::BracketMinimum` with options.
pub fn bracket_minimum<F: ScalarFunction + ?Sized>(
    f: &mut F,
    a: f64,
    b: f64,
    o: &MinBracketOptions,
) -> MinBracketResult {
    let mut r = MinBracketResult::default();
    if o.max_iterations < 1 {
        return r;
    }
    if o.use_limits && o.left_limit > o.right_limit {
        return r;
    }
    r.a = limited(a, o);
    r.b = limited(b, o);
    if (r.a - r.b).abs() < THE_ZERO_TOL {
        return r;
    }
    let use_fa = o.has_fa && (!o.use_limits || (r.a - a).abs() < THE_ZERO_TOL);
    let use_fb = o.has_fb && (!o.use_limits || (r.b - b).abs() < THE_ZERO_TOL);
    if use_fa {
        r.fa = o.fa;
    } else if !sval(f, r.a, &mut r.fa) {
        return r;
    }
    if use_fb {
        r.fb = o.fb;
    } else if !sval(f, r.b, &mut r.fb) {
        return r;
    }
    if r.fb > r.fa {
        core::mem::swap(&mut r.a, &mut r.b);
        core::mem::swap(&mut r.fa, &mut r.fb);
    }
    r.c = r.b + THE_GOLDEN_RATIO * (r.b - r.a);
    if o.use_limits {
        if !limit_and_maybe_swap(f, o, r.a, &mut r.b, &mut r.fb, &mut r.c, &mut r.fc) {
            return r;
        }
    } else if !sval(f, r.c, &mut r.fc) {
        return r;
    }
    let mut iter = 0;
    while iter < o.max_iterations && r.fb >= r.fc {
        let rr = (r.b - r.a) * (r.fb - r.fc);
        let q = (r.b - r.c) * (r.fb - r.fa);
        let denom = 2.0 * sign_transfer(cmax((q - rr).abs(), THE_ZERO_TOL), q - rr);
        let mut u = r.b - ((r.b - r.c) * q - (r.b - r.a) * rr) / denom;
        let mut u_lim = r.b + 100.0 * (r.c - r.b);
        if o.use_limits {
            u_lim = limited(u_lim, o);
        }
        let mut fu = 0.0;
        if (r.b - u) * (u - r.c) > 0.0 {
            if !sval(f, u, &mut fu) {
                return r;
            }
            if fu < r.fc {
                r.a = r.b;
                r.b = u;
                r.fa = r.fb;
                r.fb = fu;
                r.is_valid = true;
                return r;
            } else if fu > r.fb {
                r.c = u;
                r.fc = fu;
                r.is_valid = true;
                return r;
            }
            u = r.c + THE_GOLDEN_RATIO * (r.c - r.b);
            if o.use_limits {
                if !limit_and_maybe_swap(f, o, r.b, &mut r.c, &mut r.fc, &mut u, &mut fu) {
                    return r;
                }
            } else if !sval(f, u, &mut fu) {
                return r;
            }
        } else if (r.c - u) * (u - u_lim) > 0.0 {
            if o.use_limits {
                if !limit_and_maybe_swap(f, o, r.b, &mut r.c, &mut r.fc, &mut u, &mut fu) {
                    return r;
                }
            } else if !sval(f, u, &mut fu) {
                return r;
            }
            if fu < r.fc {
                r.b = r.c;
                r.c = u;
                u = r.c + THE_GOLDEN_RATIO * (r.c - r.b);
                r.fb = r.fc;
                r.fc = fu;
                if o.use_limits {
                    if !limit_and_maybe_swap(f, o, r.b, &mut r.c, &mut r.fc, &mut u, &mut fu) {
                        return r;
                    }
                } else if !sval(f, u, &mut fu) {
                    return r;
                }
            }
        } else if (u - u_lim) * (u_lim - r.c) >= 0.0 {
            u = u_lim;
            if o.use_limits {
                if !limit_and_maybe_swap(f, o, r.b, &mut r.c, &mut r.fc, &mut u, &mut fu) {
                    return r;
                }
            } else if !sval(f, u, &mut fu) {
                return r;
            }
        } else {
            u = r.c + THE_GOLDEN_RATIO * (r.c - r.b);
            if o.use_limits {
                if !limit_and_maybe_swap(f, o, r.b, &mut r.c, &mut r.fc, &mut u, &mut fu) {
                    return r;
                }
            } else if !sval(f, u, &mut fu) {
                return r;
            }
        }
        r.a = r.b;
        r.b = r.c;
        r.c = u;
        r.fa = r.fb;
        r.fb = r.fc;
        r.fc = fu;
        iter += 1;
    }
    r.is_valid = r.fb < r.fa && r.fb < r.fc;
    if r.is_valid && r.a > r.c {
        core::mem::swap(&mut r.a, &mut r.c);
        core::mem::swap(&mut r.fa, &mut r.fc);
    }
    if r.is_valid && !(r.a < r.b && r.b < r.c) {
        r.is_valid = false;
    }
    r
}

/// `MathUtils::BracketMinimum(theFunc, theA, theB, int theMaxIter)`.
pub fn bracket_minimum_iter<F: ScalarFunction + ?Sized>(f: &mut F, a: f64, b: f64, max_iter: i32) -> MinBracketResult {
    let o = MinBracketOptions { max_iterations: max_iter, ..MinBracketOptions::default() };
    bracket_minimum(f, a, b, &o)
}

// ---------------------------------------------------------------------------------------------
// MathUtils_LineSearch.hxx
// ---------------------------------------------------------------------------------------------

/// `MathUtils::LineSearchResult`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LineSearchResult {
    pub is_valid: bool,
    pub alpha: f64,
    pub f_new: f64,
    pub nb_evals: i32,
}

/// `MathUtils::ArmijoBacktrack` (defaults `1.0, 1e-4, 0.5, 50`).
#[allow(clippy::too_many_arguments)]
pub fn armijo_backtrack<F: VectorFunction + ?Sized>(
    f: &mut F,
    x: &Vector,
    dir: &Vector,
    grad: &Vector,
    fx: f64,
    alpha_init: f64,
    c1: f64,
    rho: f64,
    max_iter: i32,
) -> R<LineSearchResult> {
    let mut r = LineSearchResult { alpha: alpha_init, nb_evals: 0, ..LineSearchResult::default() };
    let (lower, upper) = (x.lower(), x.upper());
    let mut dir_deriv = 0.0;
    for i in lower..=upper {
        dir_deriv += grad.value(i)? * dir.value(i)?;
    }
    if dir_deriv >= 0.0 {
        r.is_valid = false;
        return Ok(r);
    }
    let mut x_new = Vector::new(lower, upper)?;
    for _ in 0..max_iter {
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + r.alpha * dir.value(i)?)?;
        }
        let mut f_new = 0.0;
        if !vval(f, &x_new, &mut f_new) {
            r.alpha *= rho;
            r.nb_evals += 1;
            continue;
        }
        r.nb_evals += 1;
        if f_new <= fx + c1 * r.alpha * dir_deriv {
            r.is_valid = true;
            r.f_new = f_new;
            return Ok(r);
        }
        r.alpha *= rho;
        if r.alpha < THE_EPSILON {
            break;
        }
    }
    r.is_valid = false;
    Ok(r)
}

/// `MathUtils::WolfeSearch` (defaults `1.0, 1e-4, 0.9, 20`).
#[allow(clippy::too_many_arguments)]
pub fn wolfe_search<F: VectorFunctionWithGradient + ?Sized>(
    f: &mut F,
    x: &Vector,
    dir: &Vector,
    grad: &Vector,
    fx: f64,
    alpha_init: f64,
    c1: f64,
    c2: f64,
    max_iter: i32,
) -> R<LineSearchResult> {
    let mut r = LineSearchResult { nb_evals: 0, ..LineSearchResult::default() };
    let (lower, upper) = (x.lower(), x.upper());
    let mut phi0_prime = 0.0;
    for i in lower..=upper {
        phi0_prime += grad.value(i)? * dir.value(i)?;
    }
    if phi0_prime >= 0.0 {
        r.is_valid = false;
        return Ok(r);
    }
    let mut x_new = Vector::new(lower, upper)?;
    let mut grad_new = Vector::new(lower, upper)?;
    let mut alpha_lo = 0.0;
    let mut alpha_hi = alpha_init * 2.0;
    let mut alpha = alpha_init;
    let mut phi_lo = fx;
    for k in 0..max_iter {
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + alpha * dir.value(i)?)?;
        }
        let mut phi = 0.0;
        if !vval(f, &x_new, &mut phi) {
            alpha_hi = alpha;
            alpha = 0.5 * (alpha_lo + alpha_hi);
            r.nb_evals += 1;
            continue;
        }
        r.nb_evals += 1;
        if phi > fx + c1 * alpha * phi0_prime || (k > 0 && phi >= phi_lo) {
            alpha_hi = alpha;
            break;
        }
        if !f.gradient(&x_new, &mut grad_new) {
            r.is_valid = false;
            return Ok(r);
        }
        let mut phi_prime = 0.0;
        for i in lower..=upper {
            phi_prime += grad_new.value(i)? * dir.value(i)?;
        }
        if phi_prime.abs() <= -c2 * phi0_prime {
            r.is_valid = true;
            r.alpha = alpha;
            r.f_new = phi;
            return Ok(r);
        }
        if phi_prime >= 0.0 {
            alpha_hi = alpha_lo;
            alpha_lo = alpha;
            phi_lo = phi;
            break;
        }
        alpha_lo = alpha;
        phi_lo = phi;
        alpha = 0.5 * (alpha + alpha_hi);
    }
    for _ in 0..max_iter {
        alpha = 0.5 * (alpha_lo + alpha_hi);
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + alpha * dir.value(i)?)?;
        }
        let mut phi = 0.0;
        if !vval(f, &x_new, &mut phi) {
            alpha_hi = alpha;
            r.nb_evals += 1;
            continue;
        }
        r.nb_evals += 1;
        if phi > fx + c1 * alpha * phi0_prime || phi >= phi_lo {
            alpha_hi = alpha;
        } else {
            if !f.gradient(&x_new, &mut grad_new) {
                break;
            }
            let mut phi_prime = 0.0;
            for i in lower..=upper {
                phi_prime += grad_new.value(i)? * dir.value(i)?;
            }
            if phi_prime.abs() <= -c2 * phi0_prime {
                r.is_valid = true;
                r.alpha = alpha;
                r.f_new = phi;
                return Ok(r);
            }
            if phi_prime * (alpha_hi - alpha_lo) >= 0.0 {
                alpha_hi = alpha_lo;
            }
            alpha_lo = alpha;
            phi_lo = phi;
        }
        if (alpha_hi - alpha_lo).abs() < THE_EPSILON {
            break;
        }
    }
    r.is_valid = true;
    r.alpha = alpha;
    r.f_new = phi_lo;
    Ok(r)
}

/// The shared Brent step of `ExactLineSearch` and `BrentAlongCoordinate`: updates the bracket and the best points
/// after the new point `u` with value `fu` was evaluated.
#[derive(Debug, Clone, Copy)]
struct BrentState {
    a: f64,
    b: f64,
    x: f64,
    w: f64,
    v: f64,
    fx: f64,
    fw: f64,
    fv: f64,
    d: f64,
    e: f64,
}
impl BrentState {
    /// The part of the iteration before the evaluation: the new abscissa `u`.
    fn next_point(&mut self, tol1: f64, tol2: f64, xm: f64) -> f64 {
        let mut use_parabolic = false;
        if self.e.abs() > tol1 {
            let r = (self.x - self.w) * (self.fx - self.fv);
            let mut q = (self.x - self.v) * (self.fx - self.fw);
            let mut p = (self.x - self.v) * q - (self.x - self.w) * r;
            q = 2.0 * (q - r);
            if q > 0.0 {
                p = -p;
            } else {
                q = -q;
            }
            let e_tmp = self.e;
            self.e = self.d;
            if p.abs() < (0.5 * q * e_tmp).abs() && p > q * (self.a - self.x) && p < q * (self.b - self.x) {
                self.d = p / q;
                let u_try = self.x + self.d;
                if (u_try - self.a) < tol2 || (self.b - u_try) < tol2 {
                    self.d = sign_transfer(tol1, xm - self.x);
                }
                use_parabolic = true;
            }
        }
        if !use_parabolic {
            self.e = if self.x < xm { self.b - self.x } else { self.a - self.x };
            self.d = THE_GOLDEN_SECTION * self.e;
        }
        if self.d.abs() >= tol1 { self.x + self.d } else { self.x + sign_transfer(tol1, self.d) }
    }
    /// The part of the iteration after the evaluation.
    fn accept(&mut self, u: f64, fu: f64) {
        if fu <= self.fx {
            if u < self.x {
                self.b = self.x;
            } else {
                self.a = self.x;
            }
            self.v = self.w;
            self.w = self.x;
            self.x = u;
            self.fv = self.fw;
            self.fw = self.fx;
            self.fx = fu;
        } else {
            if u < self.x {
                self.a = u;
            } else {
                self.b = u;
            }
            if fu <= self.fw || self.w == self.x {
                self.v = self.w;
                self.w = u;
                self.fv = self.fw;
                self.fw = fu;
            } else if fu <= self.fv || self.v == self.x || self.v == self.w {
                self.v = u;
                self.fv = fu;
            }
        }
    }
}

/// `MathUtils::ExactLineSearch` (defaults `10.0, 1e-6, 100`).
pub fn exact_line_search<F: VectorFunction + ?Sized>(
    f: &mut F,
    x: &Vector,
    dir: &Vector,
    alpha_max: f64,
    tolerance: f64,
    max_iter: i32,
) -> R<LineSearchResult> {
    let mut r = LineSearchResult { nb_evals: 0, ..LineSearchResult::default() };
    let (lower, upper) = (x.lower(), x.upper());
    let mut x_new = Vector::new(lower, upper)?;
    // aEvalPhi
    let mut eval_phi = |f: &mut F, alpha: f64, phi: &mut f64, nb: &mut i32| -> R<bool> {
        for i in lower..=upper {
            x_new.set_value(i, x.value(i)? + alpha * dir.value(i)?)?;
        }
        *nb += 1;
        Ok(vval(f, &x_new, phi))
    };
    let mut fx0 = 0.0;
    if !eval_phi(f, 0.0, &mut fx0, &mut r.nb_evals)? {
        r.is_valid = false;
        return Ok(r);
    }
    let mut s =
        BrentState { a: -alpha_max, b: alpha_max, x: 0.0, w: 0.0, v: 0.0, fx: fx0, fw: fx0, fv: fx0, d: 0.0, e: 0.0 };
    for _ in 0..max_iter {
        let xm = 0.5 * (s.a + s.b);
        let tol1 = tolerance * s.x.abs() + THE_ZERO_TOL / 10.0;
        let tol2 = 2.0 * tol1;
        if (s.x - xm).abs() <= (tol2 - 0.5 * (s.b - s.a)) {
            r.is_valid = true;
            r.alpha = s.x;
            r.f_new = s.fx;
            return Ok(r);
        }
        let u = s.next_point(tol1, tol2, xm);
        let mut fu = 0.0;
        if !eval_phi(f, u, &mut fu, &mut r.nb_evals)? {
            r.is_valid = false;
            r.alpha = s.x;
            r.f_new = s.fx;
            return Ok(r);
        }
        s.accept(u, fu);
    }
    r.is_valid = true;
    r.alpha = s.x;
    r.f_new = s.fx;
    Ok(r)
}

/// `MathUtils::QuadraticInterpolation`.
pub fn quadratic_interpolation(phi0: f64, phi0_prime: f64, alpha1: f64, phi1: f64) -> f64 {
    let num = phi0_prime * alpha1 * alpha1;
    let denom = 2.0 * (phi1 - phi0 - phi0_prime * alpha1);
    if denom.abs() < THE_ZERO_TOL {
        return 0.5 * alpha1;
    }
    let mut alpha_new = -num / denom;
    if alpha_new < 0.1 * alpha1 {
        alpha_new = 0.1 * alpha1;
    } else if alpha_new > 0.9 * alpha1 {
        alpha_new = 0.5 * alpha1;
    }
    alpha_new
}

/// `MathUtils::BrentAlongCoordinate`.
#[allow(clippy::too_many_arguments)]
pub fn brent_along_coordinate<F: VectorFunction + ?Sized>(
    f: &mut F,
    point: &mut Vector,
    dim_idx: i32,
    lo_bound: f64,
    up_bound: f64,
    fx_io: &mut f64,
    tolerance: f64,
    max_iter: i32,
    eval_count: &mut i32,
) -> R<bool> {
    let orig = point.value(dim_idx)?;
    let mut s = BrentState {
        a: lo_bound,
        b: up_bound,
        x: orig,
        w: orig,
        v: orig,
        fx: *fx_io,
        fw: *fx_io,
        fv: *fx_io,
        d: 0.0,
        e: 0.0,
    };
    for _ in 0..max_iter {
        let xm = 0.5 * (s.a + s.b);
        let tol1 = tolerance * s.x.abs() + THE_ZERO_TOL / 10.0;
        let tol2 = 2.0 * tol1;
        if (s.x - xm).abs() <= (tol2 - 0.5 * (s.b - s.a)) {
            break;
        }
        let u = s.next_point(tol1, tol2, xm);
        point.set_value(dim_idx, u)?;
        let mut fu = 0.0;
        if !vval(f, point, &mut fu) {
            point.set_value(dim_idx, orig)?;
            return Ok(false);
        }
        *eval_count += 1;
        s.accept(u, fu);
    }
    if s.fx < *fx_io {
        point.set_value(dim_idx, s.x)?;
        *fx_io = s.fx;
        return Ok(true);
    }
    point.set_value(dim_idx, orig)?;
    Ok(false)
}

// ---------------------------------------------------------------------------------------------
// MathUtils_FunctorScalar.hxx
// ---------------------------------------------------------------------------------------------

/// `MathUtils::ScalarLambda`.
#[derive(Debug, Clone, Copy)]
pub struct ScalarLambda<L> {
    lambda: L,
}
impl<L: FnMut(f64) -> Option<f64>> ScalarFunction for ScalarLambda<L> {
    fn value(&mut self, x: f64) -> Option<f64> {
        (self.lambda)(x)
    }
}
/// `MathUtils::MakeScalar`.
pub fn make_scalar<L: FnMut(f64) -> Option<f64>>(lambda: L) -> ScalarLambda<L> {
    ScalarLambda { lambda }
}

/// `MathUtils::ScalarLambdaWithDerivative`.
#[derive(Debug, Clone, Copy)]
pub struct ScalarLambdaWithDerivative<L> {
    lambda: L,
}
impl<L: FnMut(f64) -> Option<(f64, f64)>> ScalarFunction for ScalarLambdaWithDerivative<L> {
    fn value(&mut self, x: f64) -> Option<f64> {
        (self.lambda)(x).map(|(y, _)| y)
    }
}
impl<L: FnMut(f64) -> Option<(f64, f64)>> ScalarFunctionWithDerivative for ScalarLambdaWithDerivative<L> {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        (self.lambda)(x)
    }
}
/// `MathUtils::MakeScalarWithDerivative`.
pub fn make_scalar_with_derivative<L: FnMut(f64) -> Option<(f64, f64)>>(lambda: L) -> ScalarLambdaWithDerivative<L> {
    ScalarLambdaWithDerivative { lambda }
}

/// `MathUtils::Polynomial` (ascending coefficients; `myCoeffs` keeps its bounds).
#[derive(Debug, Clone, PartialEq)]
pub struct Polynomial {
    coeffs: Vector,
}
impl Polynomial {
    pub fn new(coeffs: &Vector) -> Self {
        Self { coeffs: coeffs.clone() }
    }
    /// `Polynomial({c0, c1, ...})`: coefficients with the lower bound 0.
    pub fn from_slice(coeffs: &[f64]) -> R<Self> {
        Ok(Self { coeffs: Vector::from_slice(0, coeffs)? })
    }
    pub fn degree(&self) -> i32 {
        if self.coeffs.length() <= 0 { 0 } else { self.coeffs.length() - 1 }
    }
    /// `Coefficient(theIndex)`: relative to the lower bound, `0.0` when out of range.
    pub fn coefficient(&self, index: i32) -> f64 {
        let idx = self.coeffs.lower() + index;
        if idx >= self.coeffs.lower() && idx <= self.coeffs.upper() {
            self.coeffs.value(idx).unwrap_or(0.0)
        } else {
            0.0
        }
    }
    fn c(&self, i: i32) -> f64 {
        self.coeffs.value(i).unwrap_or(0.0)
    }
}
impl ScalarFunction for Polynomial {
    fn value(&mut self, x: f64) -> Option<f64> {
        if self.coeffs.length() <= 0 {
            return Some(0.0);
        }
        let last = self.coeffs.upper();
        let mut y = self.c(last);
        for i in (self.coeffs.lower()..last).rev() {
            y = y * x + self.c(i);
        }
        Some(y)
    }
}
impl ScalarFunctionWithDerivative for Polynomial {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        if self.coeffs.length() <= 0 {
            return Some((0.0, 0.0));
        }
        let n = self.coeffs.length();
        if n == 1 {
            return Some((self.c(self.coeffs.lower()), 0.0));
        }
        let lower = self.coeffs.lower();
        let last = self.coeffs.upper();
        let mut y = self.c(last);
        let mut dy = 0.0;
        for i in (lower..last).rev() {
            dy = dy * x + y;
            y = y * x + self.c(i);
        }
        Some((y, dy))
    }
}

/// `MathUtils::Rational`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rational {
    num: Vector,
    denom: Vector,
}
impl Rational {
    pub fn new(num: &Vector, denom: &Vector) -> Self {
        Self { num: num.clone(), denom: denom.clone() }
    }
}
fn horner(v: &Vector, x: f64) -> f64 {
    let mut r = 0.0;
    if v.length() > 0 {
        let last = v.upper();
        r = v.value(last).unwrap_or(0.0);
        for i in (v.lower()..last).rev() {
            r = r * x + v.value(i).unwrap_or(0.0);
        }
    }
    r
}
impl ScalarFunction for Rational {
    fn value(&mut self, x: f64) -> Option<f64> {
        let num = horner(&self.num, x);
        let denom = horner(&self.denom, x);
        if denom.abs() < 1e-15 {
            return None;
        }
        Some(num / denom)
    }
}

/// `MathUtils::Composite` (`outer(inner(x))`).
#[derive(Debug, Clone, Copy)]
pub struct Composite<O, I> {
    outer: O,
    inner: I,
}
impl<O: ScalarFunction, I: ScalarFunction> ScalarFunction for Composite<O, I> {
    fn value(&mut self, x: f64) -> Option<f64> {
        let inner = self.inner.value(x)?;
        self.outer.value(inner)
    }
}
/// `MathUtils::MakeComposite`.
pub fn make_composite<O: ScalarFunction, I: ScalarFunction>(outer: O, inner: I) -> Composite<O, I> {
    Composite { outer, inner }
}

macro_rules! binary_functor {
    ($(#[$m:meta])* $name:ident, $make:ident, |$a:ident, $b:ident| $body:expr) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy)]
        pub struct $name<F, G> {
            f: F,
            g: G,
        }
        impl<F: ScalarFunction, G: ScalarFunction> ScalarFunction for $name<F, G> {
            fn value(&mut self, x: f64) -> Option<f64> {
                let $a = self.f.value(x)?;
                let $b = self.g.value(x)?;
                $body
            }
        }
        /// The `Make...` helper.
        pub fn $make<F: ScalarFunction, G: ScalarFunction>(f: F, g: G) -> $name<F, G> {
            $name { f, g }
        }
    };
}
binary_functor!(
    /// `MathUtils::Sum`.
    Sum, make_sum, |a, b| Some(a + b)
);
binary_functor!(
    /// `MathUtils::Difference`.
    Difference, make_difference, |a, b| Some(a - b)
);
binary_functor!(
    /// `MathUtils::Product`.
    Product, make_product, |a, b| Some(a * b)
);
binary_functor!(
    /// `MathUtils::Quotient`.
    Quotient, make_quotient, |a, b| if b.abs() < 1e-15 { None } else { Some(a / b) }
);

/// `MathUtils::Scaled`.
#[derive(Debug, Clone, Copy)]
pub struct Scaled<F> {
    f: F,
    scale: f64,
}
impl<F: ScalarFunction> ScalarFunction for Scaled<F> {
    fn value(&mut self, x: f64) -> Option<f64> {
        let v = self.f.value(x)?;
        Some(self.scale * v)
    }
}
/// `MathUtils::MakeScaled`.
pub fn make_scaled<F: ScalarFunction>(f: F, scale: f64) -> Scaled<F> {
    Scaled { f, scale }
}

/// `MathUtils::Shifted`.
#[derive(Debug, Clone, Copy)]
pub struct Shifted<F> {
    f: F,
    shift: f64,
}
impl<F: ScalarFunction> ScalarFunction for Shifted<F> {
    fn value(&mut self, x: f64) -> Option<f64> {
        let v = self.f.value(x)?;
        Some(v + self.shift)
    }
}
/// `MathUtils::MakeShifted`.
pub fn make_shifted<F: ScalarFunction>(f: F, shift: f64) -> Shifted<F> {
    Shifted { f, shift }
}

/// `MathUtils::Negated`.
#[derive(Debug, Clone, Copy)]
pub struct Negated<F> {
    f: F,
}
impl<F: ScalarFunction> ScalarFunction for Negated<F> {
    fn value(&mut self, x: f64) -> Option<f64> {
        let v = self.f.value(x)?;
        Some(-v)
    }
}
/// `MathUtils::MakeNegated`.
pub fn make_negated<F: ScalarFunction>(f: F) -> Negated<F> {
    Negated { f }
}

/// `MathUtils::Constant`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constant {
    value: f64,
}
impl Constant {
    pub fn new(value: f64) -> Self {
        Self { value }
    }
}
impl ScalarFunction for Constant {
    fn value(&mut self, _x: f64) -> Option<f64> {
        Some(self.value)
    }
}
impl ScalarFunctionWithDerivative for Constant {
    fn values(&mut self, _x: f64) -> Option<(f64, f64)> {
        Some((self.value, 0.0))
    }
}

/// `MathUtils::Linear`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Linear {
    slope: f64,
    intercept: f64,
}
impl Linear {
    pub fn new(slope: f64, intercept: f64) -> Self {
        Self { slope, intercept }
    }
}
impl ScalarFunction for Linear {
    fn value(&mut self, x: f64) -> Option<f64> {
        Some(self.slope * x + self.intercept)
    }
}
impl ScalarFunctionWithDerivative for Linear {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        Some((self.slope * x + self.intercept, self.slope))
    }
}

/// `MathUtils::Sine`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sine {
    amplitude: f64,
    frequency: f64,
    phase: f64,
    offset: f64,
}
impl Default for Sine {
    fn default() -> Self {
        Self::new(1.0, 1.0, 0.0, 0.0)
    }
}
impl Sine {
    pub fn new(amplitude: f64, frequency: f64, phase: f64, offset: f64) -> Self {
        Self { amplitude, frequency, phase, offset }
    }
}
impl ScalarFunction for Sine {
    fn value(&mut self, x: f64) -> Option<f64> {
        Some(self.amplitude * libm::sin(self.frequency * x + self.phase) + self.offset)
    }
}
impl ScalarFunctionWithDerivative for Sine {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        let arg = self.frequency * x + self.phase;
        Some((self.amplitude * libm::sin(arg) + self.offset, self.amplitude * self.frequency * libm::cos(arg)))
    }
}

/// `MathUtils::Cosine`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cosine {
    amplitude: f64,
    frequency: f64,
    phase: f64,
    offset: f64,
}
impl Default for Cosine {
    fn default() -> Self {
        Self::new(1.0, 1.0, 0.0, 0.0)
    }
}
impl Cosine {
    pub fn new(amplitude: f64, frequency: f64, phase: f64, offset: f64) -> Self {
        Self { amplitude, frequency, phase, offset }
    }
}
impl ScalarFunction for Cosine {
    fn value(&mut self, x: f64) -> Option<f64> {
        Some(self.amplitude * libm::cos(self.frequency * x + self.phase) + self.offset)
    }
}
impl ScalarFunctionWithDerivative for Cosine {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        let arg = self.frequency * x + self.phase;
        Some((self.amplitude * libm::cos(arg) + self.offset, -self.amplitude * self.frequency * libm::sin(arg)))
    }
}

/// `MathUtils::Exponential`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exponential {
    scale: f64,
    rate: f64,
    offset: f64,
}
impl Default for Exponential {
    fn default() -> Self {
        Self::new(1.0, 1.0, 0.0)
    }
}
impl Exponential {
    pub fn new(scale: f64, rate: f64, offset: f64) -> Self {
        Self { scale, rate, offset }
    }
}
impl ScalarFunction for Exponential {
    fn value(&mut self, x: f64) -> Option<f64> {
        Some(self.scale * libm::exp(self.rate * x) + self.offset)
    }
}
impl ScalarFunctionWithDerivative for Exponential {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        let e = libm::exp(self.rate * x);
        Some((self.scale * e + self.offset, self.scale * self.rate * e))
    }
}

/// `MathUtils::Power`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Power {
    exponent: f64,
    scale: f64,
    offset: f64,
}
impl Power {
    /// `Power(theExponent, theScale, theOffset)` (defaults `1.0`, `0.0` for the last two).
    pub fn new(exponent: f64, scale: f64, offset: f64) -> Self {
        Self { exponent, scale, offset }
    }
}
impl ScalarFunction for Power {
    fn value(&mut self, x: f64) -> Option<f64> {
        if x < 0.0 && self.exponent != libm::floor(self.exponent) {
            return None;
        }
        Some(self.scale * libm::pow(x, self.exponent) + self.offset)
    }
}
impl ScalarFunctionWithDerivative for Power {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        if x < 0.0 && self.exponent != libm::floor(self.exponent) {
            return None;
        }
        let p = libm::pow(x, self.exponent);
        let y = self.scale * p + self.offset;
        let dy = if x.abs() < 1e-15 {
            if self.exponent == 1.0 { self.scale } else { 0.0 }
        } else {
            self.scale * self.exponent * p / x
        };
        Some((y, dy))
    }
}

/// `MathUtils::Gaussian`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gaussian {
    amplitude: f64,
    mean: f64,
    sigma: f64,
}
impl Default for Gaussian {
    fn default() -> Self {
        Self::new(1.0, 0.0, 1.0)
    }
}
impl Gaussian {
    pub fn new(amplitude: f64, mean: f64, sigma: f64) -> Self {
        Self { amplitude, mean, sigma }
    }
}
impl ScalarFunction for Gaussian {
    fn value(&mut self, x: f64) -> Option<f64> {
        if self.sigma.abs() < 1e-15 {
            return None;
        }
        let z = (x - self.mean) / self.sigma;
        Some(self.amplitude * libm::exp(-0.5 * z * z))
    }
}
impl ScalarFunctionWithDerivative for Gaussian {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        if self.sigma.abs() < 1e-15 {
            return None;
        }
        let z = (x - self.mean) / self.sigma;
        let e = libm::exp(-0.5 * z * z);
        Some((self.amplitude * e, -self.amplitude * z * e / self.sigma))
    }
}

// ---------------------------------------------------------------------------------------------
// MathUtils_FunctorVector.hxx
// ---------------------------------------------------------------------------------------------

fn vget(v: &Vector, i: i32) -> f64 {
    v.value(i).unwrap_or(0.0)
}
fn vput(v: &mut Vector, i: i32, x: f64) {
    let _ = v.set_value(i, x);
}
fn mget(m: &Matrix, i: i32, j: i32) -> f64 {
    m.value(i, j).unwrap_or(0.0)
}

/// `MathUtils::VectorLambda`.
#[derive(Debug, Clone, Copy)]
pub struct VectorLambda<L> {
    lambda: L,
}
impl<L: FnMut(&Vector) -> Option<f64>> VectorFunction for VectorLambda<L> {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        (self.lambda)(x)
    }
}
/// `MathUtils::MakeVector`.
pub fn make_vector<L: FnMut(&Vector) -> Option<f64>>(lambda: L) -> VectorLambda<L> {
    VectorLambda { lambda }
}

/// `MathUtils::VectorLambdaWithGradient`.
#[derive(Debug, Clone, Copy)]
pub struct VectorLambdaWithGradient<V, G> {
    value_lambda: V,
    grad_lambda: G,
}
impl<V: FnMut(&Vector) -> Option<f64>, G> VectorFunction for VectorLambdaWithGradient<V, G> {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        (self.value_lambda)(x)
    }
}
impl<V: FnMut(&Vector) -> Option<f64>, G: FnMut(&Vector, &mut Vector) -> bool> VectorFunctionWithGradient
    for VectorLambdaWithGradient<V, G>
{
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        (self.grad_lambda)(x, g)
    }
}
/// `MathUtils::MakeVectorWithGradient`.
pub fn make_vector_with_gradient<V, G>(value_lambda: V, grad_lambda: G) -> VectorLambdaWithGradient<V, G>
where
    V: FnMut(&Vector) -> Option<f64>,
    G: FnMut(&Vector, &mut Vector) -> bool,
{
    VectorLambdaWithGradient { value_lambda, grad_lambda }
}

/// `MathUtils::SystemLambda`.
#[derive(Debug, Clone, Copy)]
pub struct SystemLambda<L> {
    lambda: L,
    nb_equations: i32,
}
impl<L: FnMut(&Vector, &mut Vector) -> bool> SystemFunction for SystemLambda<L> {
    fn nb_equations(&self) -> i32 {
        self.nb_equations
    }
    fn value(&mut self, x: &Vector, f: &mut Vector) -> bool {
        (self.lambda)(x, f)
    }
}
/// `MathUtils::MakeSystem`.
pub fn make_system<L: FnMut(&Vector, &mut Vector) -> bool>(lambda: L, nb_equations: i32) -> SystemLambda<L> {
    SystemLambda { lambda, nb_equations }
}

/// `MathUtils::QuadraticForm`: `x^T A x + b^T x + c`.
#[derive(Debug, Clone, PartialEq)]
pub struct QuadraticForm {
    a: Matrix,
    b: Vector,
    c: f64,
}
impl QuadraticForm {
    pub fn new(a: &Matrix, b: &Vector, c: f64) -> Self {
        Self { a: a.clone(), b: b.clone(), c }
    }
}
impl VectorFunction for QuadraticForm {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let mut y = self.c;
        for i in x.lower()..=x.upper() {
            for j in x.lower()..=x.upper() {
                y += vget(x, i) * mget(&self.a, i, j) * vget(x, j);
            }
        }
        for i in x.lower()..=x.upper() {
            y += vget(&self.b, i) * vget(x, i);
        }
        Some(y)
    }
}
impl VectorFunctionWithGradient for QuadraticForm {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        for i in x.lower()..=x.upper() {
            vput(g, i, vget(&self.b, i));
            for j in x.lower()..=x.upper() {
                let v = vget(g, i) + (mget(&self.a, i, j) + mget(&self.a, j, i)) * vget(x, j);
                vput(g, i, v);
            }
        }
        true
    }
}

/// `MathUtils::Rosenbrock`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rosenbrock {
    a: f64,
    b: f64,
}
impl Default for Rosenbrock {
    fn default() -> Self {
        Self::new(1.0, 100.0)
    }
}
impl Rosenbrock {
    pub fn new(a: f64, b: f64) -> Self {
        Self { a, b }
    }
}
impl VectorFunction for Rosenbrock {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let t1 = self.a - xx;
        let t2 = y - xx * xx;
        Some(t1 * t1 + self.b * t2 * t2)
    }
}
impl VectorFunctionWithGradient for Rosenbrock {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let t2 = y - xx * xx;
        vput(g, g.lower(), -2.0 * (self.a - xx) - 4.0 * self.b * xx * t2);
        vput(g, g.lower() + 1, 2.0 * self.b * t2);
        true
    }
}

/// `MathUtils::Sphere`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sphere;
impl VectorFunction for Sphere {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let mut y = 0.0;
        for i in x.lower()..=x.upper() {
            y += vget(x, i) * vget(x, i);
        }
        Some(y)
    }
}
impl VectorFunctionWithGradient for Sphere {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        for i in x.lower()..=x.upper() {
            vput(g, i, 2.0 * vget(x, i));
        }
        true
    }
}

/// `MathUtils::Booth`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Booth;
impl VectorFunction for Booth {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let t1 = xx + 2.0 * y - 7.0;
        let t2 = 2.0 * xx + y - 5.0;
        Some(t1 * t1 + t2 * t2)
    }
}
impl VectorFunctionWithGradient for Booth {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let t1 = xx + 2.0 * y - 7.0;
        let t2 = 2.0 * xx + y - 5.0;
        vput(g, g.lower(), 2.0 * t1 + 4.0 * t2);
        vput(g, g.lower() + 1, 4.0 * t1 + 2.0 * t2);
        true
    }
}

/// `MathUtils::Beale`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Beale;
impl VectorFunction for Beale {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let t1 = 1.5 - xx + xx * y;
        let t2 = 2.25 - xx + xx * y * y;
        let t3 = 2.625 - xx + xx * y * y * y;
        Some(t1 * t1 + t2 * t2 + t3 * t3)
    }
}
impl VectorFunctionWithGradient for Beale {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let y2 = y * y;
        let y3 = y2 * y;
        let t1 = 1.5 - xx + xx * y;
        let t2 = 2.25 - xx + xx * y2;
        let t3 = 2.625 - xx + xx * y3;
        vput(g, g.lower(), 2.0 * ((y - 1.0) * t1 + (y2 - 1.0) * t2 + (y3 - 1.0) * t3));
        vput(g, g.lower() + 1, 2.0 * xx * (t1 + 2.0 * y * t2 + 3.0 * y2 * t3));
        true
    }
}

/// `MathUtils::Himmelblau`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Himmelblau;
impl VectorFunction for Himmelblau {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let t1 = xx * xx + y - 11.0;
        let t2 = xx + y * y - 7.0;
        Some(t1 * t1 + t2 * t2)
    }
}
impl VectorFunctionWithGradient for Himmelblau {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        let xx = vget(x, x.lower());
        let y = vget(x, x.lower() + 1);
        let t1 = xx * xx + y - 11.0;
        let t2 = xx + y * y - 7.0;
        vput(g, g.lower(), 4.0 * xx * t1 + 2.0 * t2);
        vput(g, g.lower() + 1, 2.0 * t1 + 4.0 * y * t2);
        true
    }
}

const TWO_PI_LITERAL: f64 = 2.0 * core::f64::consts::PI;

/// `MathUtils::Rastrigin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rastrigin {
    a: f64,
}
impl Default for Rastrigin {
    fn default() -> Self {
        Self::new(10.0)
    }
}
impl Rastrigin {
    pub fn new(a: f64) -> Self {
        Self { a }
    }
}
impl VectorFunction for Rastrigin {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let n = x.upper() - x.lower() + 1;
        let mut y = self.a * f64::from(n);
        for i in x.lower()..=x.upper() {
            y += vget(x, i) * vget(x, i) - self.a * libm::cos(TWO_PI_LITERAL * vget(x, i));
        }
        Some(y)
    }
}
impl VectorFunctionWithGradient for Rastrigin {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        for i in x.lower()..=x.upper() {
            vput(g, i, 2.0 * vget(x, i) + self.a * TWO_PI_LITERAL * libm::sin(TWO_PI_LITERAL * vget(x, i)));
        }
        true
    }
}

/// `MathUtils::Ackley` (value only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ackley {
    a: f64,
    b: f64,
    c: f64,
}
impl Default for Ackley {
    fn default() -> Self {
        Self::new(20.0, 0.2, TWO_PI_LITERAL)
    }
}
impl Ackley {
    pub fn new(a: f64, b: f64, c: f64) -> Self {
        Self { a, b, c }
    }
}
impl VectorFunction for Ackley {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        const E: f64 = core::f64::consts::E;
        let n = x.upper() - x.lower() + 1;
        let mut sum_sq = 0.0;
        let mut sum_cos = 0.0;
        for i in x.lower()..=x.upper() {
            sum_sq += vget(x, i) * vget(x, i);
            sum_cos += libm::cos(self.c * vget(x, i));
        }
        let nf = f64::from(n);
        Some(-self.a * libm::exp(-self.b * (sum_sq / nf).sqrt()) - libm::exp(sum_cos / nf) + self.a + E)
    }
}

/// `MathUtils::LinearResidual`: `||A x - b||^2`.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearResidual {
    a: Matrix,
    b: Vector,
}
impl LinearResidual {
    pub fn new(a: &Matrix, b: &Vector) -> Self {
        Self { a: a.clone(), b: b.clone() }
    }
}
impl VectorFunction for LinearResidual {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        let mut y = 0.0;
        for i in self.a.lower_row()..=self.a.upper_row() {
            let mut residual = -vget(&self.b, i);
            for j in self.a.lower_col()..=self.a.upper_col() {
                residual += mget(&self.a, i, j) * vget(x, j);
            }
            y += residual * residual;
        }
        Some(y)
    }
}
impl VectorFunctionWithGradient for LinearResidual {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        let m = self.a.upper_row() - self.a.lower_row() + 1;
        let Ok(mut residual) = Vector::new(1, m) else {
            return false;
        };
        let lr = self.a.lower_row();
        for i in lr..=self.a.upper_row() {
            vput(&mut residual, i - lr + 1, -vget(&self.b, i));
            for j in self.a.lower_col()..=self.a.upper_col() {
                let v = vget(&residual, i - lr + 1) + mget(&self.a, i, j) * vget(x, j);
                vput(&mut residual, i - lr + 1, v);
            }
        }
        for j in self.a.lower_col()..=self.a.upper_col() {
            vput(g, j, 0.0);
            for i in lr..=self.a.upper_row() {
                let v = vget(g, j) + 2.0 * mget(&self.a, i, j) * vget(&residual, i - lr + 1);
                vput(g, j, v);
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------------------------
// MathUtils_Gauss.cxx / MathUtils_GaussKronrodWeights.cxx
// ---------------------------------------------------------------------------------------------

/// Copies `values` into `target` position by position (the vector keeps its own bounds).
fn copy_into(target: &mut Vector, values: &Vector) -> R<()> {
    for k in 0..values.length() {
        target.set_value(target.lower() + k, values.value(values.lower() + k)?)?;
    }
    Ok(())
}

/// `ComputeGaussLegendre` (anonymous namespace of `MathUtils_Gauss.cxx`).
fn compute_gauss_legendre(order: i32, points: &mut Vector, weights: &mut Vector) -> R<bool> {
    if order < 1 || points.length() != order || weights.length() != order {
        return Ok(false);
    }
    let mut diag = Vector::new(1, order)?;
    let mut sub_diag = Vector::new(1, order)?;
    for i in 1..=order {
        diag.set_value(i, 0.0)?;
        sub_diag.set_value(i, 0.0)?;
        if i > 1 {
            let sqr_im1 = (i - 1) * (i - 1);
            sub_diag.set_value(i, (f64::from(sqr_im1) / (4.0 * f64::from(sqr_im1) - 1.0)).sqrt())?;
        }
    }
    let eigen = mathlin_pkg::eigen_tridiagonal(&diag, &sub_diag, 30)?;
    let (Status::Ok, Some(values), Some(vecs)) = (eigen.status, &eigen.eigen_values, &eigen.eigen_vectors) else {
        return Ok(false);
    };
    let (vec_lower_row, vec_lower_col, val_lower) = (vecs.lower_row(), vecs.lower_col(), values.lower());
    let mut pairs: Vec<(f64, f64)> = Vec::new();
    for i in 1..=order {
        let e = vecs.value(vec_lower_row, vec_lower_col + i - 1)?;
        let weight = 2.0 * e * e;
        pairs.push((values.value(val_lower + i - 1)?, weight));
    }
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
    let (point_lower, weight_lower) = (points.lower(), weights.lower());
    for (k, (value, weight)) in pairs.iter().enumerate() {
        let i = k as i32;
        points.set_value(point_lower + i, *value)?;
        weights.set_value(weight_lower + i, *weight)?;
    }
    Ok(true)
}

/// `MathUtils::GetGaussPointsAndWeights`.
pub fn get_gauss_points_and_weights(order: i32, points: &mut Vector, weights: &mut Vector) -> bool {
    if order < 1 || points.length() != order || weights.length() != order {
        return false;
    }
    if order <= mathlin::gauss_points_max() {
        return match mathlin::ordered_gauss_points_and_weights(order, 1) {
            Ok(Some((p, w))) => copy_into(points, &p).is_ok() && copy_into(weights, &w).is_ok(),
            _ => false,
        };
    }
    compute_gauss_legendre(order, points, weights).unwrap_or(false)
}

/// `MathUtils::GetOrderedGaussPointsAndWeights`.
pub fn get_ordered_gauss_points_and_weights(nb_gauss: i32, points: &mut Vector, weights: &mut Vector) -> bool {
    if nb_gauss < 1 || points.length() != nb_gauss || weights.length() != nb_gauss {
        return false;
    }
    get_gauss_points_and_weights(nb_gauss, points, weights)
}

/// `MathUtils::GetKronrodPointsAndWeights` (`math::KronrodPointsAndWeights`).
pub fn get_kronrod_points_and_weights(nb_kronrod: i32, points: &mut Vector, weights: &mut Vector) -> bool {
    if nb_kronrod < 3 || nb_kronrod % 2 == 0 || points.length() != nb_kronrod || weights.length() != nb_kronrod {
        return false;
    }
    match mathlin::kronrod_points_and_weights(nb_kronrod, 1) {
        Some((p, w)) => copy_into(points, &p).is_ok() && copy_into(weights, &w).is_ok(),
        None => false,
    }
}
