# `mathinteg`: Rust API contract

This contract covers every public declaration in `cpp/MathInteg/`: `MathInteg_Gauss.hxx`, `MathInteg_Kronrod.hxx`,
`MathInteg_DoubleExp.hxx`, `MathInteg_Multiple.hxx`, and `MathInteg_Set.hxx`. These are header-only algorithms from OCCT 8.0.1;
their supporting `MathUtils` types are included in the crate module. The module is `occt_port::mathinteg`. Existing `src/`
modules and APIs remain unchanged.

## Types and callbacks

`Status` matches the discriminants and names of `MathUtils::Status` (`Ok`, `NotConverged`, `MaxIterations`, `NumericalError`,
`InvalidInput`, `InfiniteSolutions`, `NoSolution`, `NotPositiveDefinite`, `Singular`, `NonDescentDirection`). `IntegConfig`
has `initial_order`, `max_order`, `max_iterations`, and `tolerance`, defaulting to `15, 61, 100, 1e-10`; `new(tolerance,
max_iterations)` sets tolerance and iteration count. `KronrodConfig` extends it with `nb_gauss_points` (default `7`) and
`adaptive` (default `true`). `DoubleExpConfig` extends it with `nb_levels` (default `6`) and `step_factor` (default `0.5`),
alongside the inherited tolerance and maximum iteration fields (defaults `1e-10, 100`).
`MultipleConfig` contains `max_order` (default `61`).

`IntegResult` exposes `status`, `value: Option<f64>`, `absolute_error: Option<f64>`, `relative_error: Option<f64>`,
`nb_points: usize`, `nb_iterations: usize`, and `is_done()`. `SetResult` exposes `status`, `values: Option<Vector>`,
`nb_equations: usize`, and `is_done()`. `MathError` is reused for invalid Rust dimensions/indices.

`Function` is object-safe: `fn value(&mut self, x: f64) -> Option<f64>`. `MultipleFunction` is object-safe:
`fn value(&mut self, x: &[f64]) -> Option<f64>`. `SetFunction` is object-safe: `fn nb_equations(&self) -> usize` and
`fn value(&mut self, x: &[f64]) -> Option<Vec<f64>>`. `None` represents C++ `Value` returning false. Algorithm functions
are generic over `?Sized` callbacks so trait objects work. Integration dimensions and vector bounds retain OCCT's 1-based
`mathlin::Vector` indexing when accepting vectors; returned multiple-integration values use the crate's `Vector`.

## Functions

The C++ overloads with different argument shapes have distinct Rust names:

```rust
pub fn gauss<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, order: i32) -> IntegResult;
pub fn gauss_adaptive<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, config: &IntegConfig) -> IntegResult;
pub fn gauss_composite<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, intervals: i32, order: i32) -> IntegResult;
pub fn kronrod_rule<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, gauss_points: i32) -> IntegResult;
pub fn kronrod<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, config: &KronrodConfig) -> IntegResult;
pub fn kronrod_auto<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, tolerance: f64, max_order: i32) -> IntegResult;
pub fn kronrod_semi_infinite<F: Function + ?Sized>(f: &mut F, lower: f64, config: &KronrodConfig) -> IntegResult;
pub fn kronrod_infinite<F: Function + ?Sized>(f: &mut F, config: &KronrodConfig) -> IntegResult;
pub fn tanh_sinh<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, config: &DoubleExpConfig) -> IntegResult;
pub fn exp_sinh<F: Function + ?Sized>(f: &mut F, lower: f64, config: &DoubleExpConfig) -> IntegResult;
pub fn sinh_sinh<F: Function + ?Sized>(f: &mut F, config: &DoubleExpConfig) -> IntegResult;
pub fn double_exponential<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, config: &DoubleExpConfig) -> IntegResult;
pub fn tanh_sinh_singular<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, tolerance: f64) -> IntegResult;
pub fn tanh_sinh_with_singularity<F: Function + ?Sized>(f: &mut F, lower: f64, upper: f64, singularity: f64, tolerance: f64) -> IntegResult;
pub fn gauss_multiple<F: MultipleFunction + ?Sized>(f: &mut F, lower: &Vector, upper: &Vector, orders: &[i32]) -> IntegResult;
pub fn gauss_multiple_with_config<F: MultipleFunction + ?Sized>(f: &mut F, lower: &Vector, upper: &Vector, orders: &[i32], config: &MultipleConfig) -> IntegResult;
pub fn gauss_multiple_uniform<F: MultipleFunction + ?Sized>(f: &mut F, lower: &Vector, upper: &Vector, order: i32) -> IntegResult;
pub fn gauss_set<F: SetFunction + ?Sized>(f: &mut F, lower: f64, upper: f64, order: i32) -> SetResult;
pub fn gauss_set_vectors<F: SetFunction + ?Sized>(f: &mut F, lower: &Vector, upper: &Vector, order: i32) -> SetResult;
```

The implementation is responsible for reproducing the OCCT 8.0.1 algorithms, evaluation order, status, optional result fields,
and callback invocation sequence. Inputs rejected by the C++ routines produce their documented status rather than a panic.
`GaussSet` clamps the order into `1..=61`; `Gauss` rejects orders below one; `GaussMultiple` follows its per-axis order
vector; and `GaussSet`'s vector overload reads the first element of each bound vector, as the C++ overload does.

## Judge

`tests/mathinteg.rs` replays `tests/mathinteg.txt`, emitted by `oracle/mathinteg.cpp` against OCCT 8.0.1. Each input is printed
before ` | ` and has a declared schema; floats are exact 16-digit f64 bit patterns, integers and booleans decimal. The oracle
and test use the same closed callback functions: polynomial, exponential, trigonometric, kinked, and failing callbacks. Each
call reports callback count and a 64-bit hash of argument bits in call order. The result status and every populated result field
are checked. Arithmetic-only families are bit-exact; libm-dependent functions are compared within eight ulps using the largest
absolute result on the record. The test requires at least 16 distinct input lines per family and 256 total lines.

Callback records use `kind = 0` for `x^param` (multiply repeatedly), `1` for `exp(-x*x)`, `2` for `sin(x)`, `3` for
`abs(x - 0.125)`, and `4` for the failure callback, which returns false when `-0.125 < x < 0.125`; otherwise kind 4 returns
`x*x + 1`. `gauss`, `gauss_composite`, `gauss_multiple`, `gauss_multiple_config`, `gauss_multiple_uniform`, `gauss_set`, and
`gauss_set_vectors` use only the arithmetic polynomial callback and form the `EXACT` list. The other families include libm
callbacks in their records.

`throws` and `unchecked` are not expected for these entry points: invalid orders, dimensions, callback failure, and convergence
limits are represented by status fields in `IntegResult` or `SetResult`, rather than C++ exceptions. The oracle executes the
documented invalid-order and callback-failure cases to check those statuses.

## Out of scope

None of the public declarations in these five headers is excluded. All are header-only and link using the requested `TKMath` and
`TKernel` toolkits. No class in this package is listed as untranslated: the translation can use the API above without an
additional OCCT package.

## Judge relaxation (libm)

For `tanh_sinh`, `exp_sinh`, `sinh_sinh`, `double_exponential`, `tanh_sinh_singular` and `tanh_sinh_with_singularity` the
callback abscissae come out of `sinh/cosh/tanh/exp`; glibc (oracle) and the `libm` crate differ by one ulp there, so the
argument-trace hash (last token) is not compared for these families. Call counts, statuses and values still are.
