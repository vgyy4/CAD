# `mathroot`: Rust API contract

Module `occt_port::mathroot` translates the header-only package `MathRoot` of OCCT 8.0.1 (`cpp/MathRoot/`: Brent, Bisection,
Newton, Secant, Trig, Multiple, MultipleUtils, All). It uses the callback traits and result types of `mathutils`
(`ScalarFunction`, `ScalarFunctionWithDerivative`, `Config`, `ScalarResult`, `Status`) and `mathpoly::{quadratic, quartic}`;
nothing existing changes. C++ default arguments are listed in brackets; Rust passes everything.

```rust
// bracketing / open solvers (generic over F: ScalarFunction + ?Sized, or ScalarFunctionWithDerivative where noted)
pub fn brent(f, lower: f64, upper: f64, config: &Config) -> ScalarResult;
pub fn bisection(f, lower: f64, upper: f64, config: &Config) -> ScalarResult;
pub fn bisection_newton(f /*WithDerivative*/, lower: f64, upper: f64, config: &Config) -> ScalarResult;
pub fn secant(f, x0: f64, x1: f64, config: &Config) -> ScalarResult;
pub fn secant_auto(f, x0: f64, config: &Config) -> ScalarResult;
pub fn newton(f /*WithDerivative*/, guess: f64, config: &Config) -> ScalarResult;
pub fn newton_bounded(f /*WithDerivative*/, guess: f64, lower: f64, upper: f64, config: &Config) -> ScalarResult;
// trigonometric a cos^2 + 2 b cos sin + c cos + d sin + e = 0 in [inf_bound, sup_bound]
pub struct TrigResult { pub status: Status, pub roots: [f64; 4], pub nb_roots: i32, pub infinite_roots: bool } // Default + is_done()
pub fn trigonometric(a, b, c, d, e, inf_bound /*0*/, sup_bound /*2 pi*/, eps /*1.5e-12*/) -> TrigResult;
pub fn trigonometric_linear(d, e, inf_bound, sup_bound) -> TrigResult;       // a = b = c = 0
pub fn trigonometric_cde(c, d, e, inf_bound, sup_bound) -> TrigResult;       // a = b = 0
// all roots in an interval
pub struct MultipleConfig { pub nb_samples: i32 /*100*/, pub x_tolerance /*1e-10*/, pub f_tolerance /*1e-10*/,
    pub null_tolerance /*1e-12*/, pub max_iterations: i32 /*100*/, pub offset: f64 /*0*/ }        // Default
pub struct MultipleResult { pub status: Status, pub nb_iterations: usize, pub roots: Vec<f64>, pub values: Vec<f64>,
    pub is_all_null: bool }                                                       // Default + is_done() + nb_roots()
pub fn find_all_roots(f, lower, upper, config: &MultipleConfig) -> Result<MultipleResult, MathError>;
pub fn find_all_roots_nb_samples(f, lower, upper, nb_samples: i32) -> Result<MultipleResult, MathError>;   // the int overload
pub fn find_all_roots_with_derivative(f /*WithDerivative*/, lower, upper, config: &MultipleConfig) -> Result<MultipleResult, MathError>;
pub fn find_multiple_roots(f, a, b, nb_samples: i32, eps_x, eps_f, offset /*0*/) -> Result<MultipleResult, MathError>;
// null intervals
pub struct NullInterval { pub a: f64, pub b: f64, pub state: i32 }                // Default
pub struct AllRootsResult { pub status: Status, pub roots: Vec<f64>, pub root_states: Vec<i32>,
    pub null_intervals: Vec<NullInterval> }                                       // Default + is_done() + nb_roots() + nb_intervals()
pub fn find_all_roots_with_intervals(f, samples: &Vector, eps_x /*1e-10*/, eps_f /*1e-10*/, eps_nul /*1e-10*/) -> Result<AllRootsResult, MathError>;
pub fn find_all_roots_with_intervals_range(f, a, b, nb_samples: i32, eps_x, eps_f, eps_nul) -> Result<AllRootsResult, MathError>;
// helpers of MultipleUtils
pub fn sort_roots(result: &mut MultipleResult);                    // insertion sort of roots and values together
pub fn add_root(result: &mut MultipleResult, eps_x: f64, root: f64, value: f64);   // skips a root within eps_x of one already there
pub fn effective_x_tolerance(lower: f64, upper: f64, x_tolerance: f64) -> f64;
pub fn refine_bracketed_root(f /*WithDerivative*/, offset, x1, y1, x2, y2, tolerance, eps_x, result: &mut MultipleResult) -> bool;
```
`Result<_, MathError>` is `Err` where C++ throws (a `math_Vector` out of range; this OCCT build has range checks on). The judge
exercises no throwing case besides allocation errors. `FindAllRootsImpl` (a template with functors) is inlined into
`find_all_roots`; `FindAllRootsWithDerivativeImpl` is `find_all_roots_with_derivative`. The `Multiple*` functor structs of
MultipleUtils exist only to adapt the callbacks and have no Rust counterpart (the derivative of a function is a private
wrapper).

## Judge

`oracle/mathroot.cpp` -> `tests/mathroot.txt` (`build-oracle.sh`), `tests/mathroot.rs`: 20 families. The callback is one of four
closed functions (cubic, plateau `p0 max(0, |x - p1| - p2)`, `p0 (x - p1)^2 (x - p2)(x - p3)`, Chebyshev T5) given by printed
parameters, optionally failing on a printed open interval; both `Value` and `Values` are counted and their argument bits hashed
(the call sequence is compared). Everything is compared bit for bit except the `trigonometric*` families (`sin`, `cos`, `asin`,
`acos`, `atan`: 8 ulps of the largest value on the line). Run with random stack/heap fill and under AddressSanitizer: identical output.
