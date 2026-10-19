# `math1d`: Rust API contract

This contract covers the one-dimensional solvers of OCCT 8.0.1's `math` package (`cpp/math/`): the callback interfaces
`math_Function` and `math_FunctionWithDerivative`, and the classes `math_FunctionSample`, `math_FunctionRoots`,
`math_FunctionRoot`, `math_FunctionAllRoots`, `math_BracketedRoot`, `math_NewtonFunctionRoot`, `math_BracketMinimum`,
`math_BrentMinimum`, `math_TrigonometricFunctionRoots` and `math_TrigonometricEquationFunction`. It is the module
`occt_port::math1d` (add `pub mod math1d;` to `src/lib.rs`; `src/` may use private submodules). The tests are
`tests/math1d.rs` with the golden file `tests/math1d.txt` printed by `oracle/math1d.cpp` (real OCCT 8.0.1). Nothing that
exists in `src/` changes. The module reuses `occt_port::mathlin` (`MathError`; `Vector`, `Matrix`, `Gauss`, ... for
`math_FunctionSetRoot`; `DirectPolynomialRoots` for the trigonometric roots) and must not copy it.

**A private dependency that is not in this list.** `math_FunctionRoot` is implemented in C++ on `math_FunctionSetRoot`
(`cpp/math/math_FunctionSetRoot.cxx`, 1452 lines) with one variable and one equation (the file's class
`math_MyFunctionSetWithDerivatives`). `math_FunctionSetRoot` itself (and `math_FunctionSet`,
`math_FunctionSetWithDerivatives`, `math_NewtonFunctionSetRoot`) is not part of this contract: translate what
`math_FunctionRoot` needs of it (the `Perform` with and without bounds, the one-variable case, built on `mathlin`) as
private code of the module. The golden file exercises it only through `function_root` / `function_root_bounded`.

## Conventions

* **Scalars.** Reals are `f64`, every OCCT `int` is `i32`, booleans are `bool`. `RealFirst()` / `RealLast()` are
  `-f64::MAX` / `f64::MAX`. `Precision::PConfusion()` is `1.0e-7 * 0.01` (evaluated in `f64`, not the literal `1e-9`),
  `Precision::Infinite()` is `2.0e100`. `Epsilon(x)` (`Standard_Real.hxx`) is `nextafter(x, f64::MAX) - x` for `x >= 0` and
  `x - nextafter(x, -f64::MAX)` for `x < 0`. Indices are 1-based, as in OCCT, and the `i32` index arguments are not
  converted.
* **Arithmetic.** All families but the trigonometric ones are compared bit for bit (`EXACT` in `tests/math1d.rs`): the
  operations must be done in the order of the C++ source, with no fused multiply-add and no reassociation, `sqrt` is
  `f64::sqrt`, `std::abs` / `fabs` is `f64::abs`, `std::copysign` is `f64::copysign`, `std::trunc` is `f64::trunc`.
  The trigonometric families (`trig_*`) allow 8 ulps of the largest absolute value on the line (at least 1): use the
  `libm` crate (`libm::asin`, `libm::acos`, `libm::atan`, `libm::sin`, `libm::cos`; the clippy configuration forbids
  `f64::sin` etc.), as `mathlin` does.
* **Out-parameters** become returned tuples in the C++ argument order. A constructor that cannot throw returns `Self`.
  The result accessors of the solver classes are inline in the OCCT headers and check `IsDone()` themselves; they return
  `Result<_, MathError>` (below). `IsDone()` and the other accessors that cannot raise return their value directly.
* **Overloads.** Rust has none; every C++ overload has its own name (table below). C++ default arguments do not exist
  in Rust: the golden file passes every argument explicitly, and this document lists the C++ defaults
  (`NbIterations = 100`, `ZEPS = 1.0e-12`, `EpsX = EpsF = EpsNull = K = 0.0`).
* **No struct keeps a reference to a user function.** The function is borrowed for the duration of the constructor /
  `perform` call only, so the test can read the call counters of its function object after the call.
* **`dump`.** `Dump(Standard_OStream&)` becomes `fn dump(&self) -> String` with exactly the text OCCT writes
  (`std::ostream` default formatting: a `double` is written like `printf("%g")`, i.e. 6 significant digits, no trailing
  zeros, exponent form `1e-05` / `1.5e+10` when the decimal exponent is below -4 or at least 6, `-0` for negative zero;
  an `int` in decimal; the strings and newlines of the C++ source). The golden file stores the text with spaces as `_`
  and newlines as `\n`, one token (`*_dump` families). The `math_*` classes' `operator<<` is the same text (not
  exported).

## Callbacks: `Function` and `FunctionWithDerivative`

```rust
pub trait Function {
    /// math_Function::Value: Some(f(x)), or None when the C++ callback returns false.
    fn value(&mut self, x: f64) -> Option<f64>;
    /// math_Function::GetStateNumber (default: 0).
    fn state_number(&mut self) -> i32 {
        0
    }
}

pub trait FunctionWithDerivative: Function {
    /// math_FunctionWithDerivative::Derivative.
    fn derivative(&mut self, x: f64) -> Option<f64>;
    /// math_FunctionWithDerivative::Values: Some((f(x), f'(x))).
    fn values(&mut self, x: f64) -> Option<(f64, f64)>;
}
```

Both traits are object safe; solvers take `&mut dyn Function` / `&mut dyn FunctionWithDerivative`.
`None` is C++ `return false` **and leaves the C++ out-parameters untouched**: where OCCT ignores the returned flag, the
variable that was passed keeps its previous value, and the translation must do the same (`if let Some(v) = f.value(x) { fb = v }`).
The test functions never write an out-parameter when they fail. Where OCCT would read a variable that was never written
(uninitialised memory) the case is not in the golden file (the oracle detects it by running every case four times with
differently, randomly filled stack and heap and keeping only reproducible results).
`GetStateNumber` is a method of `Function` (C++: of `math_Function`), the algorithms call it exactly where the C++ does.
The call trace is part of the behaviour: every golden line prints the number of `value`, `derivative`, `values` and
`state_number` calls and a hash of the whole sequence (call kind and argument), so the Rust solver must call the
function in the same order, with the same arguments, as OCCT.

`TrigonometricEquationFunction::new(a, b, c, d, e)` is the one library-provided implementation (below).

## Errors

The error type is `occt_port::mathlin::MathError` (no new type):

| variant | stands for |
|---|---|
| `NotDone` | `StdFail_NotDone`: a result accessor of a solver that is not done (`throws` in the golden file), or `BracketMinimum::values` / `function_values` before a successful `perform` (`unchecked`: OCCT 8.0.1 as built has that check compiled out) |
| `Range` | an index outside `1..=count` (`Standard_OutOfRange`; `throws`), or `FunctionSample::get_parameter` with an index outside `1..=nb_points` (`unchecked`) |
| `InfiniteSolutions` | `StdFail_InfiniteSolutions` of `TrigonometricFunctionRoots::nb_solutions` / `value` (`throws`) |
| `Domain` | `Standard_NumericError` in `FunctionAllRoots::new`, and the sample with fewer than 2 points (both `unchecked`, see below) |

The test only checks `Err` against the oracle's `throws` / `unchecked`; the variant is for the caller. Not judged but
required: nothing may panic (the lints forbid indexing and `unwrap`); a state the C++ would read out of range is an
error, never garbage.

## Types and functions (`occt_port::math1d::`)

### `FunctionSample` (`math_FunctionSample`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `FunctionSample::new` | `a: f64, b: f64, n: i32` | `Self` | constructor (no check, any `n`) |
| `bounds` | `&self` | `(f64, f64)` | `Bounds` |
| `nb_points` | `&self` | `i32` | `NbPoints` |
| `get_parameter` | `&self, index: i32` | `Result<f64, MathError>` | `GetParameter` |

* `get_parameter(index)` is `(((n - index) as f64) * a + ((index - 1) as f64) * b) / ((n - 1) as f64)`, the integer
  differences done in `i32` first. `Err(Range)` when `index <= 0` or `index > n`, and `Err(Domain)` when `n < 2` (the C++
  has no check at all and divides by `n - 1`): the golden file marks all of these `unchecked`.
* `FunctionAllRoots` takes the sample by reference and uses the same private formula for its own calls: it does not call
  the checked `get_parameter` where the index is valid by construction.

### `FunctionRoots` (`math_FunctionRoots`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `FunctionRoots::new` | `f: &mut dyn FunctionWithDerivative, a: f64, b: f64, nb_sample: i32, eps_x: f64, eps_f: f64, eps_null: f64, k: f64` | `Self` | constructor (defaults `0.0`) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `is_all_null` | `&self` | `Result<bool, MathError>` | `IsAllNull` |
| `nb_solutions` | `&self` | `Result<i32, MathError>` | `NbSolutions` |
| `value` | `&self, n: i32` | `Result<f64, MathError>` | `Value` |
| `state_number` | `&self, n: i32` | `Result<i32, MathError>` | `StateNumber` |
| `dump` | `&self` | `String` | `Dump` |

* The constructor runs the `MATH_FUNCTIONROOTS_NEWCODE` branch of `math_FunctionRoots.cxx` (the only one compiled; the
  `OLDCODE` branch is dead). It never raises: when a sample point has no value (`Value` returns false for one of the
  `2 * max(nb_sample, 10)` samples) the object is simply not done (`is_done() == false`). It uses
  `BracketedRoot` (the one of this module) for the derivative zero search.
* `is_all_null`, `nb_solutions`, `value`, `state_number` return `Err(NotDone)` when not done and `Err(Range)` for an index
  outside `1..=nb_solutions` (`throws`).

### `FunctionAllRoots` (`math_FunctionAllRoots`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `FunctionAllRoots::new` | `f: &mut dyn FunctionWithDerivative, s: &FunctionSample, eps_x: f64, eps_f: f64, eps_nul: f64` | `Result<Self, MathError>` | constructor |
| `is_done` | `&self` | `bool` | `IsDone` |
| `nb_intervals` | `&self` | `Result<i32, MathError>` | `NbIntervals` |
| `get_interval` | `&self, index: i32` | `Result<(f64, f64), MathError>` | `GetInterval` |
| `get_interval_state` | `&self, index: i32` | `Result<(i32, i32), MathError>` | `GetIntervalState` (`IFirst`, `ILast`) |
| `nb_points` | `&self` | `Result<i32, MathError>` | `NbPoints` |
| `get_point` | `&self, index: i32` | `Result<f64, MathError>` | `GetPoint` |
| `get_point_state` | `&self, index: i32` | `Result<i32, MathError>` | `GetPointState` |
| `dump` | `&self` | `String` | `Dump` |

* `new` is `Err(Domain)` when `s.nb_points() < 2`, and where the C++ `Standard_NumericError_Raise_if` conditions of
  `math_FunctionAllRoots.cxx` hold (an inner `FunctionRoots` that is not done, is all null, or has no solution where one is
  expected, exactly as the source says). **OCCT 8.0.1 as built has these checks compiled out** (the library has no
  reference to `Standard_NumericError`): it just goes on, reading empty sequences, and crashes in most of the cases. The
  golden file therefore says `unchecked` for them, never `throws`; the oracle decides by compiling the same source file
  with the checks on. The Rust constructor implements the checks and returns `Err`.
* A constructed object is always done (`is_done() == true`); the accessors still check, `Err(Range)` for an index outside
  the sequence (`throws`).
* The sample's parameters are used with the formula of `get_parameter`, a descending sample (`a > b`) is allowed.

### `BracketedRoot` (`math_BracketedRoot`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `BracketedRoot::new` | `f: &mut dyn Function, bound1: f64, bound2: f64, tolerance: f64, nb_iterations: i32, zeps: f64` | `Self` | constructor (defaults `100`, `1.0e-12`) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `root` / `value` | `&self` | `Result<f64, MathError>` | `Root` / `Value` (the value of the function at the root) |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `dump` | `&self` | `String` | `Dump` |

The two first evaluations ignore the returned flag (the golden file has no case in which they fail, since the variable
would be uninitialised); later failed evaluations leave `TheError` unchanged.

### `FunctionRoot` (`math_FunctionRoot`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `FunctionRoot::new` | `f: &mut dyn FunctionWithDerivative, guess: f64, tolerance: f64, nb_iterations: i32` | `Result<Self, MathError>` | first constructor (default `100`) |
| `FunctionRoot::new_bounded` | `f, guess, tolerance, a: f64, b: f64, nb_iterations: i32` | `Result<Self, MathError>` | second constructor (bounds `A`, `B`) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `root` / `derivative` / `value` | `&self` | `Result<f64, MathError>` | `Root` / `Derivative` / `Value` |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `dump` | `&self` | `String` | `Dump` |

The constructors return `Result` because the `mathlin` containers they use are fallible; they are never `Err` in the
golden file. `F.GetStateNumber()` is called once when the solve is done, then `F.Value(root)` for `value`.

### `NewtonFunctionRoot` (`math_NewtonFunctionRoot`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `NewtonFunctionRoot::new` | `f: &mut dyn FunctionWithDerivative, guess: f64, eps_x: f64, eps_f: f64, nb_iterations: i32` | `Self` | (F, Guess, EpsX, EpsF, NbIterations = 100); bounds `RealFirst()`, `RealLast()` |
| `NewtonFunctionRoot::new_bounded` | `f, guess, eps_x, eps_f, a: f64, b: f64, nb_iterations: i32` | `Self` | (F, Guess, EpsX, EpsF, A, B, NbIterations = 100) |
| `NewtonFunctionRoot::without_function` | `a: f64, b: f64, eps_x: f64, eps_f: f64, nb_iterations: i32` | `Self` | (A, B, EpsX, EpsF, NbIterations = 100): "used in a sub-class", does not solve |
| `perform` | `&mut self, f: &mut dyn FunctionWithDerivative, guess: f64` | `()` | `Perform` (public; may be called repeatedly with other functions: the state of the previous call is kept as the C++ does) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `root` / `derivative` / `value` | `&self` | `Result<f64, MathError>` | `Root` / `Derivative` / `Value` |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `dump` | `&self` | `String` | `Dump` |

### `BracketMinimum` (`math_BracketMinimum`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `BracketMinimum::new` | `a: f64, b: f64` | `Self` | `math_BracketMinimum(A, B)`: does not run |
| `BracketMinimum::with_function` | `f: &mut dyn Function, a, b` | `Self` | `(F, A, B)`: constructs and runs `Perform` |
| `BracketMinimum::with_function_fa` | `f, a, b, fa: f64` | `Self` | `(F, A, B, FA)` |
| `BracketMinimum::with_function_fa_fb` | `f, a, b, fa, fb: f64` | `Self` | `(F, A, B, FA, FB)` |
| `set_limits` | `&mut self, left: f64, right: f64` | `()` | `SetLimits` |
| `set_fa` / `set_fb` | `&mut self, value: f64` | `()` | `SetFA` / `SetFB` |
| `perform` | `&mut self, f: &mut dyn Function` | `()` | `Perform` (may be called again: it continues from the changed `Ax`, `Bx`, ... exactly as the C++) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `values` | `&self` | `Result<(f64, f64, f64), MathError>` | `Values` (`A`, `B`, `C`) |
| `function_values` | `&self` | `Result<(f64, f64, f64), MathError>` | `FunctionValues` (`FA`, `FB`, `FC`) |
| `dump` | `&self` | `String` | `Dump` |

`values` / `function_values` are `Err(NotDone)` when not done; OCCT's own check is compiled out of the library, the golden
file says `unchecked`. Without `set_limits` the limits are `-Precision::Infinite()` and `Precision::Infinite()` (`2e100`) and
`myIsLimited` is false.

### `BrentMinimum` (`math_BrentMinimum`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `BrentMinimum::new` | `tol_x: f64, nb_iterations: i32, zeps: f64` | `Self` | `(TolX, NbIterations = 100, ZEPS = 1.0e-12)` |
| `BrentMinimum::with_fbx` | `tol_x: f64, fbx: f64, nb_iterations: i32, zeps: f64` | `Self` | `(TolX, Fbx, NbIterations = 100, ZEPS = 1.0e-12)`: `F(Bx)` is not computed |
| `perform` | `&mut self, f: &mut dyn Function, ax: f64, bx: f64, cx: f64` | `()` | `Perform` with the default `IsSolutionReached` |
| `perform_with_test` | `&mut self, f: &mut dyn Function, ax, bx, cx, test: &mut dyn BrentSolutionTest` | `()` | `Perform` of a sub-class that redefines the virtual `IsSolutionReached` |
| `is_done` | `&self` | `bool` | `IsDone` |
| `location` / `minimum` | `&self` | `Result<f64, MathError>` | `Location` / `Minimum` |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `dump` | `&self` | `String` | `Dump` |

```rust
/// The protected state of math_BrentMinimum that a sub-class sees in IsSolutionReached.
#[derive(Clone, Copy, Debug)]
pub struct BrentState {
    pub a: f64, pub b: f64, pub x: f64, pub fx: f64, pub fv: f64, pub fw: f64,
    pub x_tol: f64,   // XTol
    pub eps_z: f64,   // EPSZ
}
impl BrentState {
    /// The inline math_BrentMinimum::IsSolutionReached: with TwoTol = 2 (XTol |x| + EPSZ),
    /// x <= TwoTol + a && x >= b - TwoTol.
    pub fn is_solution_reached_default(&self) -> bool;
}
pub trait BrentSolutionTest {
    /// Called at the point of the loop where the C++ calls IsSolutionReached(F), with the current state.
    fn is_solution_reached(&mut self, state: &BrentState, f: &mut dyn Function) -> bool;
}
```

`perform` is `perform_with_test` with a test that returns `state.is_solution_reached_default()`. A second `perform` on the
same object keeps `Done` (it is only set at the end of a successful or failing run) and, for `with_fbx`, keeps `fx` (the
C++ evaluates `F(Bx)` only when the object was not given `Fbx`); the golden file has such cases (`brent_minimum_twice`).

### `TrigonometricFunctionRoots` (`math_TrigonometricFunctionRoots`)

Roots of `A cos^2 x + 2 B cos x sin x + C cos x + D sin x + E` in `[inf_bound, sup_bound]`.

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `TrigonometricFunctionRoots::new` | `a, b, c, d, e, inf_bound, sup_bound: f64` | `Self` | 7-argument constructor |
| `TrigonometricFunctionRoots::new_cde` | `c, d, e, inf_bound, sup_bound` | `Self` | 5-argument constructor `(C, D, E, InfBound, SupBound)` (`A = B = 0`) |
| `TrigonometricFunctionRoots::new_de` | `d, e, inf_bound, sup_bound` | `Self` | 4-argument constructor `(D, E, InfBound, SupBound)` (`A = B = C = 0`) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `infinite_roots` | `&self` | `bool` | `InfiniteRoots` |
| `nb_solutions` | `&self` | `Result<i32, MathError>` | `NbSolutions` |
| `value` | `&self, index: i32` | `Result<f64, MathError>` | `Value` |
| `dump` | `&self` | `String` | `Dump` |

`nb_solutions` / `value`: `Err(InfiniteSolutions)` when `infinite_roots()`, then `Err(NotDone)` when not done, then
`Err(Range)` when `index < 1` or `index > nb_solutions` (`throws`). The protected `Perform` is the constructors' body and is
not exported. The root finder uses `NewtonFunctionRoot` (this module) with a private `TrigonometricEquationFunction`, and
`mathlin::DirectPolynomialRoots`. `M_PI` is `std::f64::consts::PI`.

### `TrigonometricEquationFunction` (`math_TrigonometricEquationFunction`)

`TrigonometricEquationFunction::new(a, b, c, d, e) -> Self`; it implements `Function` (`value`; `state_number` is the default
0) and `FunctionWithDerivative` (`derivative`, `values`) for `F(x) = A cos^2 x + 2 B cos x sin x + C cos x + D sin x + E`,
with the three different evaluation orders of the C++ header (`Value`, `Derivative`, `Values` do not share code, and their
results may differ in the last bit). All three always return `Some`. It is `Clone, Debug`.

## What the golden file contains

`tests/math1d.txt`: 5220 lines in 35 families, 1.37 MB (see `FAMILIES` in `tests/math1d.rs`). Every family has at least 40
distinct lines; no line is repeated; no family is input-less. The `EXACT` list is every family except the six
`trig_*` ones. Families are in groups per class: the construction family (`function_roots`, ...) prints
`IsDone`, the call counts and trace hash of the function, and every result of a finished solve; the `*_access` family
prints one accessor call (so that accessors of solvers that are not done and invalid indices are covered); the `*_dump`
family prints the `Dump` text. `test_function` replays the judge's own test functions (Rust vs C++) and has no API of
`math1d` to test beyond the two traits.

| family (lines) | Rust calls |
|---|---|
| `test_function` (220) | the judge's `Tf` only (both traits) |
| `function_sample` (160) | `FunctionSample::new`, `bounds`, `nb_points`, `get_parameter` |
| `function_roots` (260), `_access` (120), `_dump` (60) | `FunctionRoots::new` and its accessors, one accessor per `_access` line |
| `function_all_roots` (220), `_access` (120), `_dump` (60) | `FunctionAllRoots::new` (with a `FunctionSample`) and its accessors |
| `bracketed_root` (240), `_access` (90), `_dump` (40) | `BracketedRoot::new` and accessors |
| `function_root` (240), `function_root_bounded` (240), `_access` (100), `_dump` (40) | `FunctionRoot::new` / `new_bounded` and accessors (the `_access`/`_dump` families carry a `bounded` flag) |
| `newton_root` (240), `newton_root_bounded` (240), `newton_root_perform` (160), `_access` (90), `_dump` (40) | `NewtonFunctionRoot::new` / `new_bounded` / `without_function` + `perform` once or twice |
| `bracket_minimum` (240), `bracket_minimum_staged` (260), `_access` (80), `_dump` (40) | `BracketMinimum::with_function*` (variant 0..2) / `new` + `set_limits`, `set_fa`, `set_fb`, `perform` once or twice |
| `brent_minimum` (260), `_twice` (120), `_custom` (120), `_access` (80), `_dump` (40) | `BrentMinimum::new` / `with_fbx` (variant 0/1), `perform`, `perform_with_test` |
| `trig_roots` (400), `trig_roots_cde` (160), `trig_roots_de` (120), `_access` (120), `_dump` (40) | the three `TrigonometricFunctionRoots` constructors and accessors |
| `trig_equation_function` (160) | `TrigonometricEquationFunction` `value`, `derivative`, `values`, `state_number` |

The inputs cover: roots at the interval ends, at samples and between them, roots closer together than the sample step
(pairs 1e-4 to 1e-2 apart), double and triple roots, no root, functions that are zero (or constant) on a sub-interval,
a pole inside the interval, reversed and empty intervals, `NbSample` -30..=100, tolerances `0`, `1e-300`, `1e-15` up to
`10`, `K` shifts, iteration limits `0`, `1`, `2`, ..., `100`, functions scaled by 1e-150..1e150 and intervals up to
`1e8`, minima at the ends, flat minima and V-shaped minima, a Newton start at a stationary point and on a flat part,
bounds reversed or equal, `Perform` called twice with different functions, `SetLimits` / `SetFA` / `SetFB`, every
constructor of every class, and the trigonometric equations with every coefficient combination (all zero, constant,
`sin`, `cos`, `cos^2`, `sin cos`, the analytic `A = E = 0` cases, tangent roots at `|E/D| = 1`, tiny coefficients
below OCCT's `1.5e-12`, roots at `0`, `pi`, `2 pi`, bounds `+-f64::MAX`, empty and reversed ranges).

Cases are **not** in the golden file when they depend on uninitialised memory (a callback failed where OCCT ignores the
flag and uses a variable it never wrote), when a callback would see or return a non-finite number, when a case runs
away (a monotone function for `BracketMinimum` without limits), and, for the trigonometric families, when moving every
`sin`/`cos`/`asin`/`acos`/`atan`/`atan2`/`tan`/`pow` result of the C library by one ulp changes the output by more than
3 ulps (cases at multiple roots where the result is determined by rounding noise).

### Where the golden file says `throws` and where `unchecked`

* `throws`: the `StdFail_NotDone` / `StdFail_InfiniteSolutions` checks and the `Standard_OutOfRange` checks of the
  accessors that are **inline in the OCCT headers** (`*.lxx`, `NCollection_Sequence::Value`,
  `NCollection_Array1::operator()`), which are compiled into the caller, so they exist whenever the caller does not
  define `No_Exception`: `function_roots_access`, `function_all_roots_access`, `bracketed_root_access`,
  `function_root_access`, `newton_root_access`, `brent_minimum_access`, `trig_roots_access`.
* `unchecked`: checks of the OCCT sources that the shipped libraries have compiled out (`No_Standard_OutOfRange`,
  `No_StdFail_NotDone`, no `Standard_NumericError`): `function_sample` (`get_parameter` with an index outside `1..=n`, or
  `n < 2`: for an index in range of an `n >= 2` sample the oracle does call it and prints the value; for the others it does
  not call it, since the result is a division by `n - 1`, possibly 0/0), `function_all_roots*` (sample of fewer
  than 2 points and the `Standard_NumericError` cases, decided by compiling `math_FunctionAllRoots.cxx` into the oracle
  with the checks on), `bracket_minimum_access` (`values` / `function_values` before a successful run).

### Regenerating the golden file

```
mkdir -p oracle-bin && c++ -std=c++17 -O2 oracle/math1d.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKMath -lTKernel \
  -o oracle-bin/math1d && LD_LIBRARY_PATH=$OCCT/lib oracle-bin/math1d > tests/math1d.txt
```

The output is deterministic (fixed xorshift seed, 4 differently poisoned runs per case). The oracle, built from the
repository root, includes `cpp/math/math_FunctionAllRoots.cxx` (as `math_FunctionAllRootsChecked`, to classify the
`Standard_NumericError` cases) and interposes the C library's `sin cos tan asin acos atan atan2 pow sincos` (to re-run the
trigonometric cases with every result moved by an ulp); both are glibc/x86-64 specific and only matter for generating
the file, not for the test. The file was generated with glibc 2.39 on x86-64.
