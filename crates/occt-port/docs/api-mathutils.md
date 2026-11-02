# `mathutils`: Rust API contract

Module `occt_port::mathutils` translates the package `MathUtils` of OCCT 8.0.1 (`cpp/MathUtils/`): Core, Config,
Types, Convergence, Poly, Domain, Random, Deriv, Bracket, LineSearch, FunctorScalar, FunctorVector, Gauss and
GaussKronrodWeights. It reuses `mathlin::{Matrix, Vector, MathError}`, `mathinteg::Status` (re-exported as
`mathutils::Status`) and `mathlin_pkg::eigen_tridiagonal` (MathUtils' Gauss-Legendre for more than 61 points is
computed with `MathLin::EigenTridiagonal`) and `mathlin::{ordered_gauss_points_and_weights,
kronrod_points_and_weights}` for the tables. No existing API changes. `mathinteg` and `mathlin_pkg` keep their own
private copies of the result structs; `mathutils` holds the canonical ones.

C++ default arguments have no Rust counterpart: the Rust function takes every argument; the defaults are listed
(`[default]`) and the judge passes them explicitly. C++ `bool f(..., T& out)` becomes `Option<T>` when `out` is only
meaningful on success and a `bool`/`Result<bool, MathError>` with `&mut` arguments when the C++ output is partially
written on failure. Functions that index a `Vector`/`Matrix` with OCCT's absolute indices return `Result<_, MathError>`
(`Err` exactly where C++ throws `Standard_OutOfRange`/`Standard_DimensionMismatch`; this OCCT build has range checks on).

## Constants and Core

```rust
pub const THE_EPSILON: f64; THE_ZERO_TOL: f64 /*1e-15*/; THE_PI; THE_2PI; THE_GOLDEN_RATIO; THE_GOLDEN_SECTION;
pub const THE_NEWTON_FTOL_SQ, THE_NEWTON_STEP_TOL_FACTOR: f64; THE_NEWTON_MAX_ITER: usize;
pub const THE_NEWTON2D_SINGULAR_DET, ..._CRITICAL_GRAD_SQ, ..._MAX_STEP_RATIO, ..._DOMAIN_EXT, ..._STAGNATION_RATIO: f64;
pub const THE_NEWTON2D_STAGNATION_COUNT, THE_NEWTON2D_LINE_SEARCH_MAX: i32; // + _SKIP_LINESEARCH_SQ, _STAGNATION_RELAX,
// _BACKTRACK_TRIGGER, _BACKTRACK_ACCEPT, _MAXITER_RELAX: f64; THE_HESSIAN_DEGENERACY_REL/_ABS, THE_ARMIJO_C1: f64
pub fn clamp(v: f64, lower: f64, upper: f64) -> f64;
pub fn is_zero(v: f64, tol: f64) -> bool;                      // [THE_ZERO_TOL]
pub fn is_equal(a: f64, b: f64, tol: f64) -> bool;             // [THE_ZERO_TOL]
pub fn safe_div(n: f64, d: f64, default: f64) -> f64;          // [0.0]
pub fn sign(v: f64) -> i32;  pub fn sign_transfer(a: f64, b: f64) -> f64;  pub fn sqr(v: f64) -> f64;  pub fn cube(v: f64) -> f64;
pub fn cube_root(v: f64) -> f64;  // libm::cbrt
pub fn is_finite(v: f64) -> bool;
pub fn compute_scale_factor(coeffs: &[f64]) -> f64;            // count = coeffs.len()
pub fn dot_product(a: &Vector, b: &Vector) -> Result<f64, MathError>;  // indexes b with a's range
pub fn vector_norm(v: &Vector) -> f64;  pub fn vector_inf_norm(v: &Vector) -> f64;
```
`Config` fields: `max_iterations: i32 [100], tolerance, x_tolerance, f_tolerance: f64 [1e-10], step_min: f64 [EPSILON]`,
`Config::new(tolerance, max_iterations)` (all three tolerances = tolerance). `BoundedConfig { base: Config, lower_bound [-f64::MAX], upper_bound [f64::MAX] }` (C++ derives from Config), `new(lower, upper, tolerance, max_iterations)`. `NDimConfig { base: Config, use_bounds [false] }`, `new(tolerance, max_iterations, use_bounds)`. `IntegConfig { initial_order [15], max_order [61],
max_iterations [100], tolerance [1e-10] }`, `new(tolerance, max_iterations)`. `LinConfig { singularity_tolerance [1e-15],
use_pivoting [true] }`. All implement `Default` (+ `new`).

## Types

`ScalarResult { status [NotConverged], nb_iterations: usize, root, value, derivative: Option<f64> }`,
`PolyResult { status, nb_roots: usize, roots: [f64; 4] }` + `index(i)`, `VectorResult { status, nb_iterations, solution:
Option<Vector>, value: Option<f64>, gradient: Option<Vector>, jacobian: Option<Matrix> }`, `LinearResult { status, solution,
determinant }`, `LinearMultipleResult { status, solutions: Option<Matrix>, determinant }`, `EigenResult { status,
nb_iterations, eigen_values: Option<Vector>, eigen_vectors: Option<Matrix> }`, `DecompResult { status, l: Option<Matrix>,
u: Option<Matrix>, d: Option<Vector>, determinant }`, `IntegResult { status, nb_iterations, nb_points: usize, value,
absolute_error, relative_error }`, `InverseResult { status, inverse, determinant }`. All `Default` (status `NotConverged`),
all with `is_done() -> bool` (`status == Ok`). (C++ `explicit operator bool` is `is_done`.)

## Convergence

```rust
pub fn is_x_converged(x_old: f64, x_new: f64, tol: f64) -> bool;  pub fn is_f_converged(f: f64, tol: f64) -> bool;
pub fn is_converged(x_old: f64, x_new: f64, f: f64, c: &Config) -> bool;
pub fn is_min_converged(x_old: f64, x_new: f64, f_old: f64, f_new: f64, c: &Config) -> bool;
pub fn is_vector_converged(old: &Vector, new: &Vector, tol: f64) -> Result<bool, MathError>; // indexes new with old's range
pub fn is_gradient_converged(g: &Vector, tol: f64) -> bool;  pub fn infinity_norm(v: &Vector) -> f64;  pub fn euclidean_norm(v: &Vector) -> f64;
```

## Poly (coefficients are slices; `degree` is the C++ argument; the slice holds `degree + 1` items)

```rust
pub fn eval_poly(coeffs: &[f64], degree: i32, x: f64) -> f64;           // ascending
pub fn eval_poly_deriv(coeffs: &[f64], degree: i32, x: f64) -> (f64, f64); // (value, derivative)
pub fn eval_poly_desc(coeffs: &[f64], degree: i32, x: f64) -> f64;      // descending
pub fn refine_poly_root(coeffs: &[f64], degree: i32, root: f64, max_iter: i32) -> f64;       // [5]
pub fn refine_poly_root_desc(coeffs: &[f64], degree: i32, root: f64, max_iter: i32) -> f64;  // [5]; degree <= 4
pub fn sort_roots(roots: &mut [f64]);                                    // insertion sort, count = len
pub fn remove_duplicate_roots(roots: &mut [f64], tol: f64) -> usize;    // [1e-10]; compacts in place, returns the new count
pub fn depress_cubic(b: f64, c: f64, d: f64) -> (f64, f64, f64);        // (p, q, shift)
pub fn depress_quartic(b: f64, c: f64, d: f64, e: f64) -> (f64, f64, f64, f64); // (p, q, r, shift)
```

## Domain, Random

`Domain1D { min, max }` (`Default` 0,0; `new(min, max)`; `length()`, `mid()`, `contains(u, tol) [0.0]`, `clamp(u)`,
`is_large(threshold) [1000.0]`, `is_full_period(period, tol) [1e-10]`, `lerp(t)`, `normalize(u)`, `is_finite(limit) [1e100]`,
`is_equal(&other, tol) [1e-10]`). `Domain2D { umin, umax, vmin, vmax }` (`Default`; `new(umin, umax, vmin, vmax)`,
`from_domains(&Domain1D, &Domain1D)`, `u()`, `v()` -> Domain1D, `u_length()`, `v_length()`, `u_mid()`, `v_mid()`,
`contains(u, v, tol) [0.0]`, `clamp(u, v) -> (f64, f64)`, `is_large(threshold) [1000.0]`, `is_u_full_period(period, tol)`,
`is_v_full_period(period, tol)` `[1e-10]`, `is_finite(limit) [1e100]`).
`RandomGenerator`: `new(seed: u64) [seed 1 = Default]`, `set_seed(seed)`, `next_int() -> u64`, `next_real() -> f64`
(xoshiro256** with a splitmix64 seeding; bit-exact).

## Callback traits (object safe; `None` / `false` is C++ `return false`)

```rust
pub trait ScalarFunction { fn value(&mut self, x: f64) -> Option<f64>; }
pub trait ScalarFunctionWithDerivative: ScalarFunction { fn values(&mut self, x: f64) -> Option<(f64, f64)>; } // (y, dy)
pub trait VectorFunction { fn value(&mut self, x: &Vector) -> Option<f64>; }
pub trait VectorFunctionWithGradient: VectorFunction {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool;
    fn values(&mut self, x: &Vector, y: &mut f64, g: &mut Vector) -> bool; // default: value, then gradient
}
pub trait SystemFunction { fn nb_equations(&self) -> i32; fn value(&mut self, x: &Vector, f: &mut Vector) -> bool; }
```

## Deriv, Bracket, LineSearch (all generic over `F: Trait + ?Sized`)

```rust
pub fn central_difference<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, step: f64) -> Option<f64>;           // [1e-8]
pub fn forward_difference<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, fx: f64, step: f64) -> Option<f64>; // [1e-8]
pub fn second_derivative<F: ScalarFunction + ?Sized>(f: &mut F, x: f64, fx: f64, step: f64) -> Option<f64>;  // [1e-5]
// the next four restore `x` before returning and leave the output partially written on failure, as C++
pub fn numerical_gradient<F: VectorFunction + ?Sized>(f: &mut F, x: &mut Vector, grad: &mut Vector, step: f64) -> Result<bool, MathError>; // [1e-8]
pub fn numerical_gradient_adaptive<F: VectorFunction + ?Sized>(f: &mut F, x: &mut Vector, grad: &mut Vector, rel_step: f64) -> Result<bool, MathError>; // [1e-8]
pub fn numerical_jacobian<F: SystemFunction + ?Sized>(f: &mut F, x: &mut Vector, jac: &mut Matrix, step: f64) -> Result<bool, MathError>; // [1e-8]; jac is rows x cols (1-based)
pub fn numerical_hessian<F: VectorFunction + ?Sized>(f: &mut F, x: &mut Vector, hess: &mut Matrix, step: f64) -> Result<bool, MathError>; // [1e-5]; hess is n x n, 1-based

pub struct BracketResult { pub is_valid: bool, pub a: f64, pub b: f64, pub fa: f64, pub fb: f64 }             // Default
pub struct MinBracketResult { pub is_valid: bool, pub a, pub b, pub c, pub fa, pub fb, pub fc: f64 }          // Default
pub struct MinBracketOptions { pub max_iterations: i32 [50], pub use_limits: bool [false], pub left_limit: f64 [0.0],
    pub right_limit: f64 [0.0], pub has_fa: bool, pub has_fb: bool, pub fa: f64, pub fb: f64 }                 // Default
pub fn bracket_root<F: ScalarFunction + ?Sized>(f: &mut F, a: f64, b: f64, max_iter: i32) -> BracketResult;    // [50]
pub fn bracket_minimum<F: ScalarFunction + ?Sized>(f: &mut F, a: f64, b: f64, o: &MinBracketOptions) -> MinBracketResult;
pub fn bracket_minimum_iter<F: ScalarFunction + ?Sized>(f: &mut F, a: f64, b: f64, max_iter: i32) -> MinBracketResult; // C++ overload with int

pub struct LineSearchResult { pub is_valid: bool, pub alpha: f64, pub f_new: f64, pub nb_evals: i32 }          // Default
pub fn armijo_backtrack<F: VectorFunction + ?Sized>(f: &mut F, x: &Vector, dir: &Vector, grad: &Vector, fx: f64,
    alpha_init: f64, c1: f64, rho: f64, max_iter: i32) -> Result<LineSearchResult, MathError>;   // [1.0, 1e-4, 0.5, 50]
pub fn wolfe_search<F: VectorFunctionWithGradient + ?Sized>(f: &mut F, x: &Vector, dir: &Vector, grad: &Vector, fx: f64,
    alpha_init: f64, c1: f64, c2: f64, max_iter: i32) -> Result<LineSearchResult, MathError>;    // [1.0, 1e-4, 0.9, 20]
pub fn exact_line_search<F: VectorFunction + ?Sized>(f: &mut F, x: &Vector, dir: &Vector, alpha_max: f64, tol: f64,
    max_iter: i32) -> Result<LineSearchResult, MathError>;                                       // [10.0, 1e-6, 100]
pub fn quadratic_interpolation(phi0: f64, phi0_prime: f64, alpha1: f64, phi1: f64) -> f64;
pub fn brent_along_coordinate<F: VectorFunction + ?Sized>(f: &mut F, point: &mut Vector, dim_idx: i32, lo: f64, up: f64,
    fx: &mut f64, tol: f64, max_iter: i32, eval_count: &mut i32) -> Result<bool, MathError>;
```
The C++ `ArmijoBacktrack` does not set `FNew`/`Alpha` on failure beyond what the code shows; reproduce exactly (`alpha`
holds the last tried step). `wolfe_search` uses `gradient` of the callback.

## Functors (FunctorScalar, FunctorVector)

All functor structs implement the callback traits above; their `value`/`values`/`gradient` are the C++ methods.
Scalar: `Polynomial::new(&Vector)` (+ `from_slice(&[f64])`; `degree() -> i32`, `coefficient(i: i32) -> f64`), `Rational::new(&Vector, &Vector)`,
`Constant::new(v)`, `Linear::new(slope, intercept)`, `Sine::new(amplitude, frequency, phase, offset)` (`Default` 1,1,0,0),
`Cosine` (same), `Exponential::new(scale, rate, offset)` (`Default` 1,1,0), `Power::new(exponent, scale, offset)` (scale
`[1]`, offset `[0]`), `Gaussian::new(amplitude, mean, sigma)` (`Default` 1,0,1). Combinators `Composite<O, I>`, `Sum<F, G>`,
`Difference`, `Product`, `Quotient`, `Scaled<F>`, `Shifted<F>`, `Negated<F>` with `make_composite(outer, inner)`,
`make_sum`, `make_difference`, `make_product`, `make_quotient`, `make_scaled(f, s)`, `make_shifted(f, s)`, `make_negated(f)`.
Lambdas: `make_scalar(|x| -> Option<f64>)` -> `ScalarLambda`; `make_scalar_with_derivative(|x| -> Option<(f64, f64)>)` ->
`ScalarLambdaWithDerivative` (its `value` runs the lambda and drops the derivative; `values` returns both).
Vector: `QuadraticForm::new(&Matrix, &Vector, c)`, `Rosenbrock::new(a, b)` (`Default` 1, 100), `Sphere`, `Booth`, `Beale`,
`Himmelblau`, `Rastrigin::new(a)` (`Default` 10), `Ackley::new(a, b, c)` (`Default` 20, 0.2, 2*PI; value only),
`LinearResidual::new(&Matrix, &Vector)`; lambdas `make_vector(|x: &Vector| -> Option<f64>)`,
`make_vector_with_gradient(value, |x: &Vector, g: &mut Vector| -> bool)`, `make_system(|x: &Vector, f: &mut Vector| -> bool, nb_equations)`
(`SystemLambda`: `nb_equations()`, `value`). Out-of-range accesses inside functors (a `g` whose bounds do not cover `x`'s) are not
exercised by the judge.

## Gauss / Kronrod points

```rust
pub fn get_gauss_points_and_weights(order: i32, points: &mut Vector, weights: &mut Vector) -> bool;
pub fn get_ordered_gauss_points_and_weights(nb: i32, points: &mut Vector, weights: &mut Vector) -> bool;
pub fn get_kronrod_points_and_weights(nb: i32, points: &mut Vector, weights: &mut Vector) -> bool;
```
`false` when `order < 1` or a vector's length differs from the order (Kronrod: also even or `< 3`). Gauss: `order <=
61` from the tables (`math::OrderedGaussPointsAndWeights`), above that `ComputeGaussLegendre` (eigenvalues of the Jacobi
matrix with `mathlin_pkg::eigen_tridiagonal`, sorted by value). The vectors keep their own lower bounds (use equal lower bounds: with different ones C++ writes out of range).

## Judge

`oracle/mathutils.cpp` -> `tests/mathutils.txt` (`build-oracle.sh`), `tests/mathutils.rs`. Callbacks: a cubic
`((d x + c) x + b) x + a` that fails on a printed open interval (scalar), and Sphere/Rosenbrock/Booth/QuadraticForm/Himmelblau with a
failure threshold on `x(lower)` (vector) or an `m`-equation system (Jacobian); every call is counted and the argument bits are
hashed (the call sequence is compared). Everything is compared bit for bit except the libm families `cube_root`,
`scalar_functor_libm`, `vector_functor_libm` (8 ulps of the largest value on the line; NaN equals NaN). The oracle ran with
random stack/heap fill and under AddressSanitizer, and printed identical output. OCCT's point functions write the weights at the
lower bound of the *points* (out of range and unchecked when the two lower bounds differ), so the judge always gives both vectors the
same lower bound.
