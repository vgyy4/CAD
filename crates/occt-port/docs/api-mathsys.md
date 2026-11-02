# `mathsys`: Rust API contract

Module `occt_port::mathsys` translates the header-only package `MathSys` of OCCT 8.0.1 (`cpp/MathSys/`: Newton,
LevenbergMarquardt, Newton2D/3D/4D, NewtonTypes). It uses `mathnd::FunctionSetWithDerivatives` as the callback of the
generic solvers, `mathlin_pkg::solve` for the linear systems, `mathutils::{Config, VectorResult, Status}` and `gp::{Pnt, Vec}`;
nothing existing changes. C++ default arguments: `max_iter` 100 for the Newton functions, `LMConfig()` for the LM ones,
`NewtonOptions()` for the fixed-size ones; Rust passes everything.

## Generic solvers (Result is `Err` where C++ throws a range error, e.g. a tolerance vector with another lower bound)

```rust
pub fn newton<F: FunctionSetWithDerivatives + ?Sized>(f: &mut F, start: &Vector, tol_x: &Vector, tol_f: f64, max_iter: usize) -> Result<VectorResult, MathError>;
pub fn newton_scalar_tol<F: ...>(f: &mut F, start: &Vector, tol_x: f64, tol_f: f64, max_iter: usize) -> Result<VectorResult, MathError>; // the double-tolerance overload
pub fn newton_bounded<F: ...>(f: &mut F, start: &Vector, inf_bound: &Vector, sup_bound: &Vector, tol_x: &Vector, tol_f: f64, max_iter: usize) -> Result<VectorResult, MathError>;
pub struct LmConfig { pub base: Config, pub lambda_init: f64 /*1e-3*/, pub lambda_increase: f64 /*10*/, pub lambda_decrease: f64 /*0.1*/,
                      pub lambda_max: f64 /*1e10*/, pub lambda_min: f64 /*1e-12*/ }   // Default; LmConfig::new(tolerance, max_iterations)
pub fn levenberg_marquardt<F: ...>(f: &mut F, start: &Vector, config: &LmConfig) -> Result<VectorResult, MathError>;
pub fn levenberg_marquardt_bounded<F: ...>(f: &mut F, start: &Vector, inf_bound: &Vector, sup_bound: &Vector, config: &LmConfig) -> Result<VectorResult, MathError>;
```
`VectorResult` is `mathutils::VectorResult` (`solution`, `jacobian` for Newton; `solution`, `value` = chi squared, `gradient` for LM).
The status/iteration/optional-field conventions are exactly C++'s (see the sources). `Config::new(tolerance, max_iterations)` sets all
three tolerances, `LmConfig::new` does the same through `base`.

## Fixed-size solvers

```rust
pub struct NewtonResultN<const N: usize> { pub status: Status, pub x: [f64; N], pub nb_iterations: usize, pub residual_norm: f64, pub step_norm: f64 } // Default (NotConverged, zeros) + is_done()
pub struct NewtonBoundsN<const N: usize> { pub min: [f64; N], pub max: [f64; N], pub has_bounds: bool }                       // Default (zeros, true)
pub struct NewtonOptions { pub base: Config, pub max_step_ratio: f64 /*0.5*/, pub enable_line_search: bool /*true*/,
                           pub allow_soft_bounds: bool /*false*/, pub soft_bounds_extension: f64 /*1e-4*/ }               // Default: base = {tolerance 1e-10, x_tolerance 1e-16, f_tolerance 1e-10, max_iterations 100, step_min EPSILON}
pub trait Function2D { fn eval(&mut self, x: f64, y: f64, f: &mut [f64; 2], j: &mut [[f64; 2]; 2]) -> bool; }   // + blanket impl for FnMut closures
pub trait Function3D { fn eval(&mut self, x: f64, y: f64, z: f64, f: &mut [f64; 3], j: &mut [[f64; 3]; 3]) -> bool; }
pub trait Function4D { fn eval(&mut self, x0: f64, x1: f64, x2: f64, x3: f64, f: &mut [f64; 4], j: &mut [[f64; 4]; 4]) -> bool; }
pub trait SymmetricFunction2D { fn value(&mut self, u: f64, v: f64) -> Option<(f64, f64)>;
                                fn value_and_jacobian(&mut self, u: f64, v: f64) -> Option<(f64, f64, f64, f64, f64)>; } // (f1, f2) / (f1, f2, j11, j12, j22)
pub fn solve2d<F: Function2D + ?Sized>(f: &mut F, x0: &[f64; 2], bounds: &NewtonBoundsN<2>, options: &NewtonOptions) -> NewtonResultN<2>;
pub fn solve2d_symmetric<F: SymmetricFunction2D + ?Sized>(f: &mut F, x0: &[f64; 2], bounds: &NewtonBoundsN<2>, options: &NewtonOptions) -> NewtonResultN<2>;
pub fn solve3d<F: Function3D + ?Sized>(...) -> NewtonResultN<3>;   pub fn solve4d<F: Function4D + ?Sized>(...) -> NewtonResultN<4>;
pub trait CurveEvaluator { fn d2(&self, t: f64) -> (Pnt, Vec, Vec); }                                   // (P, D1, D2)
pub trait SurfaceEvaluator { fn d2(&self, u: f64, v: f64) -> (Pnt, Vec, Vec, Vec, Vec, Vec); }           // (P, D1U, D1V, D2UU, D2VV, D2UV)
pub fn solve_curve_surface_extrema_3d<C, S>(curve: &C, surface: &S, x0: &[f64; 3], bounds: &NewtonBoundsN<3>, options: &NewtonOptions) -> NewtonResultN<3>;
pub fn solve_surface_surface_extrema_4d<S1, S2>(s1: &S1, s2: &S2, x0: &[f64; 4], bounds: &NewtonBoundsN<4>, options: &NewtonOptions) -> NewtonResultN<4>;
```

## Helpers of namespace `detail` (public in the header)

```rust
pub const THE_SINGULAR_DET_TOL: f64 = 1e-25; THE_CRITICAL_GRAD_SQ: f64 = 1e-60; THE_LINE_SEARCH_MAX: i32 = 8; THE_ARMIJO_C1: f64 = 1e-4;
pub fn is_options_valid(&NewtonOptions) -> bool;  is_options_valid_3d;  is_options_valid_4d;          // identical bodies
pub fn is_bounds_valid_2d(&NewtonBoundsN<2>) -> bool;  _3d;  _4d;
pub fn max_domain_size_2d(&NewtonBoundsN<2>) -> f64;  _3d;  _4d;                                         // 1.0 without bounds
pub fn clamp_2d(x: &mut [f64; 2], b: &NewtonBoundsN<2>, use_soft_bounds: bool, soft_ext_ratio: f64);  clamp_3d;  clamp_4d;
pub fn solve_symmetric_2x2_svd(j11, j12, j22, f1, f2, tol /*1e-15*/) -> Option<(f64, f64)>;             // None: C++ false, du/dv untouched
pub fn solve3x3(j: &[[f64; 3]; 3], f: &[f64; 3]) -> Option<[f64; 3]>;                                   // None when |det| < 1e-30
pub fn solve4x4(j: &[[f64; 4]; 4], f: &[f64; 4]) -> Option<[f64; 4]>;                                   // partial pivoting; None when a pivot < 1e-30
```
The generic-width `Clamp`/`IsBoundsValid`/`MaxDomainSize` of the three dimensions are separate public functions.

## Judge

`oracle/mathsys.cpp` -> `tests/mathsys.txt` (`build-oracle.sh`), `tests/mathsys.rs`: 17 families. Function sets are
`F_k(x) = sum_j C_kj x_j + c3 x_(k mod n)^3 - b_k` (any `m x n`, with failure thresholds on `x(lower)` for `Value` and for `Derivatives`/`Values`);
the fixed-size systems are `A p + c p^2 - b` and, for the symmetric solver, the gradient of a quartic potential; the extrema solvers use a
cubic curve and quadratic surface patches given by printed vectors. All callbacks count their calls and hash their argument bits. Everything
is compared bit for bit (no libm). Run with random stack/heap fill and under AddressSanitizer: identical output.
