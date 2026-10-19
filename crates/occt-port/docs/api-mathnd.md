# `mathnd`: Rust API contract

This contract covers the multi-variable solvers and the numerical integration of OCCT 8.0.1's `math` package
(`cpp/math/`): the callback interfaces `math_MultipleVarFunction`, `math_MultipleVarFunctionWithGradient`,
`math_MultipleVarFunctionWithHessian`, `math_FunctionSet` and `math_FunctionSetWithDerivatives`; the solvers
`math_FunctionSetRoot`, `math_NewtonFunctionSetRoot`, `math_NewtonMinimum`, `math_BFGS`, `math_FRPR`, `math_Powell`,
`math_Uzawa` and `math_BissecNewton`; and `math_GaussSingleIntegration`, `math_GaussMultipleIntegration`,
`math_GaussSetIntegration` and `math_KronrodSingleIntegration`. (`math_GlobOptMin` and `math_PSO` are a later package.)
It is the module `occt_port::mathnd` (add `pub mod mathnd;` to `src/lib.rs`; `src/` may use private submodules). The
tests are `tests/mathnd.rs` with the golden file `tests/mathnd.txt` printed by `oracle/mathnd.cpp` (real OCCT 8.0.1).

Nothing that exists in `src/` changes its API. The module reuses `occt_port::mathlin` (`MathError`, `MathStatus`,
`Vector`, `IntegerVector`, `Matrix`, `Gauss`, `GaussLeastSquare`, `Svd`, `Crout`, `Jacobi`, `recipes::lu_decompose*`
/ `lu_solve`, `gauss_points`, `gauss_weights`, `gauss_points_max`, `ordered_gauss_points_and_weights`,
`kronrod_points_and_weights`) and `occt_port::math1d` (the traits `Function` and `FunctionWithDerivative`, and
`BracketMinimum`, `BrentMinimum` for the line searches of `FunctionSetRoot`, `Bfgs`, `Frpr` and `Powell`), and must
not copy them.

**`math1d`'s private `FunctionSetRoot`.** `src/math1d/function_set_root.rs` is a private port of the one-variable path
of `math_FunctionSetRoot`, used by `math1d::FunctionRoot`. The public `mathnd::FunctionSetRoot` of this package
replaces it: `math1d::FunctionRoot` should be rebuilt on `mathnd::FunctionSetRoot` (through a private adapter that
turns its `FunctionWithDerivative` into a one-equation `FunctionSetWithDerivatives`, as the C++ class
`math_MyFunctionSetWithDerivatives` does), and the private file removed. **`tests/math1d.rs` must keep passing
unchanged**, and so must every other existing test.

## Conventions

* **Scalars.** Reals are `f64`, every OCCT `int` is `i32`, booleans are `bool`. `RealFirst()` / `RealLast()` are
  `-f64::MAX` / `f64::MAX`, `RealSmall()` is `f64::MIN_POSITIVE`, `RealEpsilon()` is `f64::EPSILON`,
  `gp::Resolution()` is `f64::MIN_POSITIVE`. `Precision::PConfusion()` is `1.0e-7 * 0.01` (evaluated in `f64`),
  `Precision::Confusion()` is `1.0e-7`, `Precision::SquareConfusion()` is `1.0e-7 * 1.0e-7`,
  `Precision::Infinite()` is `2.0e100`, `Precision::IsInfinite(r)` is `r.abs() >= 0.5 * 2.0e100`. `Epsilon(x)` is the
  one of `docs/api-math1d.md`. `std::min(a, b)` is `if b < a { b } else { a }`, `std::max(a, b)` is
  `if a < b { b } else { a }` (the order matters for signed zeros and NaN).
* **Vectors and matrices** are `mathlin::Vector` / `IntegerVector` / `Matrix` with OCCT's index ranges, which are
  part of the results (the golden file prints the lower bound and the length of every vector, the lower row, row
  count, lower column and column count of every matrix). The solvers' own vectors have the ranges of the C++
  (`1..=n` nearly everywhere). Where the C++ copies a user vector into its own (`Sol = StartingPoint`,
  `TheLocation = StartingPoint`, `myLeft = theLeftBorder`, `Lowsav = Lower`, ...), the values are taken **by position**
  (`math_Vector::operator=` keeps the bounds of its left side): a starting point with the bounds `0..=n-1` gives the
  same result as one with `1..=n`. The golden file has such inputs. Where the C++ indexes a user vector directly
  (`theTolerance(i)`, `theInfBound(i)`, `Order(i)`, `StartingPoint(1)` of `Uzawa`, `Cont(i, 1)`, ...), the golden file
  always passes the lower bound 1.
* **Arithmetic.** All families but the Kronrod ones are compared bit for bit (`EXACT` in `tests/mathnd.rs`): the
  operations must be done in the order of the C++ source, with no fused multiply-add and no reassociation, `sqrt` is
  `f64::sqrt`, `std::abs` / `fabs` is `f64::abs`. The `mathlin` operations used by the C++ (`Norm`, `Norm2`, `*` (dot
  product), `TMultiply`, `Gauss`, `Svd`, ...) are `mathlin`'s, which are bit-identical to OCCT's. The Kronrod families
  (`kronrod*`) call `pow` (in `GKRule`): use `libm::pow`; they allow 8 ulps of the largest absolute value on the line
  (at least 1).
* **Out-parameters** become returned values (a tuple in C++ argument order). The C++ accessors that return
  `const math_Vector&` / `const math_Matrix&` return `Result<&Vector, MathError>` / `Result<&Matrix, MathError>`. The
  C++ overloads that copy a result into a vector or matrix the caller passes (`Root(math_Vector&)`,
  `Derivative(math_Matrix&)`, `Location(math_Vector&)`, ...) are `*_into(&self, out: &mut Vector) -> Result<(), MathError>`:
  they copy by position into `out`, keep `out`'s bounds, and fail where the C++ checks the dimensions (see each class).
* **Overloads and defaults.** Rust has none: every C++ overload has its own name (tables below). The golden file
  passes every argument explicitly; the C++ defaults are listed.
* **No struct keeps a reference to a user function.** A function is borrowed for the duration of the constructor or
  `perform` call only (constructors that take it only to read `NbVariables` / `NbEquations` take `&dyn ...`), so the
  test can read the call counters of its function object between calls.
* **`dump`.** `Dump(Standard_OStream&)` becomes `fn dump(&self) -> String` with exactly the text OCCT writes, with the
  conventions of `docs/api-math1d.md` (`std::ostream` default formatting of `double`: `printf("%g")`; `bool` as `1` /
  `0`; a `math_Vector` written with `operator<<` is its `Dump`: `math_Vector of Length = <n>\n` then
  `math_Vector(<i>) = <%g>\n` per element; note that `mathlin::Vector::dump()` writes the elements with Rust's `{}`,
  not `%g`, so it is not that text: use `math1d`'s `%g` formatting). The golden file stores the text with spaces as `_`
  and newlines as `\n`, one token (`*_dump` families).
* **Nothing may panic** (the lints forbid indexing and `unwrap`); a state in which the C++ would read or write out of
  range is an error or a "not done" result, never garbage.

## Callbacks

```rust
use occt_port::mathlin::{Matrix, Vector};

/// math_MultipleVarFunction: f(X) of NbVariables() variables.
pub trait MultipleVarFunction {
    /// NbVariables.
    fn nb_variables(&self) -> i32;
    /// Value: Some(f(x)), or None when the C++ callback returns false.
    fn value(&mut self, x: &Vector) -> Option<f64>;
    /// GetStateNumber (default: 0).
    fn state_number(&mut self) -> i32 {
        0
    }
}

/// math_MultipleVarFunctionWithGradient.
pub trait MultipleVarFunctionWithGradient: MultipleVarFunction {
    /// Gradient: writes the gradient into `g` (the solver's vector, with its bounds); false is C++ `return false`.
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool;
    /// Values(X, F, G): Some(f(x)) and the gradient written into `g`.
    fn values(&mut self, x: &Vector, g: &mut Vector) -> Option<f64>;
}

/// math_MultipleVarFunctionWithHessian.
pub trait MultipleVarFunctionWithHessian: MultipleVarFunctionWithGradient {
    /// Values(X, F, G, H): Some(f(x)), the gradient written into `g` and the Hessian into `h`.
    fn values_with_hessian(&mut self, x: &Vector, g: &mut Vector, h: &mut Matrix) -> Option<f64>;
}

/// math_FunctionSet: NbEquations() functions F_k(X) of NbVariables() variables.
pub trait FunctionSet {
    /// NbVariables.
    fn nb_variables(&self) -> i32;
    /// NbEquations.
    fn nb_equations(&self) -> i32;
    /// Value: writes F(x) into `f`; false is C++ `return false`.
    fn value(&mut self, x: &Vector, f: &mut Vector) -> bool;
    /// GetStateNumber (default: 0).
    fn state_number(&mut self) -> i32 {
        0
    }
}

/// math_FunctionSetWithDerivatives.
pub trait FunctionSetWithDerivatives: FunctionSet {
    /// Derivatives: writes the Jacobian (row k = equation k, column j = variable j) into `d`.
    fn derivatives(&mut self, x: &Vector, d: &mut Matrix) -> bool;
    /// Values: writes F(x) into `f` and the Jacobian into `d`.
    fn values(&mut self, x: &Vector, f: &mut Vector, d: &mut Matrix) -> bool;
}
```

All five traits are object safe; the solvers take `&mut dyn ...` (and `&dyn ...` in constructors). The one-variable
callbacks (`BissecNewton`, `GaussSingleIntegration`, `KronrodSingleIntegration`) are `math1d::Function` /
`math1d::FunctionWithDerivative`. As in C++, the solver passes its own output vectors and matrices, with its own
bounds, and **a failing callback leaves them as they were**: where OCCT ignores the returned flag, the values of the
previous call are used (the test functions never write anything when they fail). Scalar results come back as
`Option<f64>`; where OCCT ignores the flag of a scalar callback, the C++ variable keeps its previous value
(`if let Some(v) = f.value(x) { fval = v }`).

The test functions check the dimensions they are given (`x` of length `nb_variables()`, `g` of length `n`, `h` and the
Jacobian of the right shape) and panic otherwise: the solver must pass vectors and matrices of the dimensions the
C++ passes. **The call trace is part of the behaviour**: every construction line prints the number of calls of each
callback and of `state_number`, and a hash of the whole sequence (call kind, then `x.lower()`, `x.length()` and every
element of `x`), so the Rust solvers must call the functions in the same order, with the same arguments (same bits,
same bounds), as OCCT, and call `state_number` exactly where the C++ calls `GetStateNumber`.

### Solution tests (`IsSolutionReached`, `IsConverged`)

The C++ solvers call a virtual `IsSolutionReached` (`IsConverged` for `math_NewtonMinimum`) that a sub-class may
redefine (OCCT's own sub-classes do). In Rust a solver has `perform` (the default test) and `perform_with_test` (a
user test); the test gets a read-only view of the protected state the C++ sub-class sees, and, where the C++ passes
it, the function (the test may call it; the golden file has such tests). Each state struct has
`is_solution_reached_default` (`is_converged_default`), the C++ inline default; `perform` is `perform_with_test` with
a test that returns it.

```rust
pub struct FunctionSetRootState<'a> { pub delta: &'a Vector, pub sol: &'a Vector, pub df: &'a Matrix, pub tol: &'a Vector }
pub trait FunctionSetRootTest {
    fn is_solution_reached(&mut self, state: &FunctionSetRootState<'_>, f: &mut dyn FunctionSetWithDerivatives) -> bool;
}
pub struct NewtonFunctionSetRootState<'a> {
    pub tol_x: &'a Vector, pub tol_f: f64, pub sol: &'a Vector, pub delta_x: &'a Vector,
    pub f_values: &'a Vector, pub jacobian: &'a Matrix,
}
pub trait NewtonFunctionSetRootTest {
    fn is_solution_reached(&mut self, state: &NewtonFunctionSetRootState<'_>, f: &mut dyn FunctionSetWithDerivatives) -> bool;
}
pub struct NewtonMinimumState<'a> {
    pub step: &'a Vector, pub gradient: &'a Vector, pub hessian: &'a Matrix, pub previous_minimum: f64,
    pub minimum: f64, pub min_eigen_value: f64, pub x_tol: f64, pub c_tol: f64, pub nb_iter: i32, pub convex: bool,
}
pub trait NewtonMinimumTest {
    /// IsConverged is const and gets no function.
    fn is_converged(&mut self, state: &NewtonMinimumState<'_>) -> bool;
}
pub struct BfgsState<'a> {
    pub location: &'a Vector, pub gradient: &'a Vector, pub previous_minimum: f64, pub minimum: f64,
    pub x_tol: f64, pub eps_z: f64, pub nb_iter: i32,
}
pub trait BfgsTest {
    fn is_solution_reached(&mut self, state: &BfgsState<'_>, f: &mut dyn MultipleVarFunctionWithGradient) -> bool;
}
pub struct FrprState<'a> {
    pub location: &'a Vector, pub gradient: &'a Vector, pub minimum: f64, pub previous_minimum: f64,
    pub x_tol: f64, pub eps_z: f64,
}
pub trait FrprTest {
    fn is_solution_reached(&mut self, state: &FrprState<'_>, f: &mut dyn MultipleVarFunctionWithGradient) -> bool;
}
pub struct PowellState<'a> {
    pub location: &'a Vector, pub minimum: f64, pub previous_minimum: f64, pub location_error: f64,
    pub x_tol: f64, pub eps_z: f64,
}
pub trait PowellTest {
    fn is_solution_reached(&mut self, state: &PowellState<'_>, f: &mut dyn MultipleVarFunction) -> bool;
}
pub struct BissecNewtonState { pub x_tol: f64, pub x: f64, pub dx: f64, pub f: f64, pub df: f64 }
pub trait BissecNewtonTest {
    fn is_solution_reached(&mut self, state: &BissecNewtonState, f: &mut dyn FunctionWithDerivative) -> bool;
}
```

The fields are the C++ protected members at the moment of the call (`Delta`, `Sol`, `DF`, `Tol`; `TolX`, `TolF`,
`Sol`, `DeltaX`, `FValues`, `Jacobian`; `TheStep`, `TheGradient`, `TheHessian`, `PreviousMinimum`, `TheMinimum`,
`MinEigenValue`, `XTol`, `CTol`, `nbiter`, `Convex`; `TheLocation`, `TheGradient`, `PreviousMinimum`, `TheMinimum`,
`XTol`, `EPSZ`, `nbiter`; `TheLocation`, `TheGradient`, `TheMinimum`, `PreviousMinimum`, `XTol`, `EPSZ`;
`TheLocation`, `TheMinimum`, `PreviousMinimum`, `TheLocationError`, `XTol`, `EPSZ`; `XTol`, `x`, `dx`, `f`, `df`). All
structs derive `Debug` (the `BissecNewtonState` also `Clone, Copy`). The defaults:

| state | default |
|---|---|
| `FunctionSetRootState` | `abs(delta(i)) <= tol(i)` for every `i` in `1..=sol.length()` |
| `NewtonFunctionSetRootState` | `abs(delta_x(i)) <= tol_x(i) && abs(f_values(i)) <= tol_f` for every `i` of `delta_x` |
| `NewtonMinimumState::is_converged_default` | `step.norm() <= x_tol \|\| abs(minimum - previous_minimum) <= x_tol * abs(previous_minimum)` |
| `BfgsState`, `FrprState` | `2.0 * abs(minimum - previous_minimum) <= x_tol * (abs(minimum) + abs(previous_minimum) + eps_z)` |
| `PowellState` | `2.0 * abs(previous_minimum - minimum) <= x_tol * (abs(previous_minimum) + abs(minimum) + eps_z)` |
| `BissecNewtonState` | `abs(dx) <= x_tol` |

## Errors

The error type is `occt_port::mathlin::MathError` (no new type). The test only checks `Err` against the oracle's
`throws` / `unchecked`; the variant is for the caller:

| variant | stands for |
|---|---|
| `NotDone` | `StdFail_NotDone` of a result accessor of a solver that is not done (`throws`: the accessors are inline in the headers), and `NewtonMinimum::dump` of a solver that is not done (`unchecked`) |
| `Dimension` | the explicit `throw Standard_DimensionError` of `FunctionSetRoot::perform*` (`throws`), the dimension checks of the `*_into` accessors (`throws` where the check or the `operator=` it uses is inline, `unchecked` where it is in the library), `Uzawa`'s dimension check, `set_boundary` with bounds of a wrong length, and `GaussSetIntegration` with more than one variable (all `unchecked`) |
| `Range` | `GaussSetIntegration` with an order above `gauss_points_max()`, `Uzawa` with more rows than columns (`unchecked`) |
| `Singular` | `Uzawa`'s direct method with a singular `C C^T` (`unchecked`: `math_Crout::Inverse` of a decomposition that is not done) |

## Types and functions (`occt_port::mathnd::`)

### `FunctionSetRoot` (`math_FunctionSetRoot`)

Roots of a system of `NbEquations()` equations in `NbVariables()` unknowns (square, over- or under-determined: the
Newton direction uses `Gauss` (pivot `1.e-9`) then `Svd` when singular, `Svd` when there are more unknowns, and
`GaussLeastSquare` when there are more equations, as `SearchDirection` does), with a bounded line search
(`math1d::BrentMinimum` in `MinimizeDirection`, on the private `MyDirFunction`, which calls `F.Value` along a line).

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `FunctionSetRoot::new` | `f: &dyn FunctionSetWithDerivatives, tolerance: &Vector, nb_iterations: i32` | `Result<Self, MathError>` | `(F, Tolerance, NbIterations = 100)` |
| `FunctionSetRoot::without_tolerance` | `f: &dyn FunctionSetWithDerivatives, nb_iterations: i32` | `Result<Self, MathError>` | `(F, NbIterations = 100)`: the tolerance is unset until `set_tolerance` |
| `set_tolerance` | `&mut self, tolerance: &Vector` | `Result<(), MathError>` | `SetTolerance` (reads `tolerance(1..=n)`) |
| `perform` | `&mut self, f: &mut dyn FunctionSetWithDerivatives, starting_point: &Vector, stop_on_divergent: bool` | `Result<(), MathError>` | `Perform(F, StartingPoint, theStopOnDivergent = false)` |
| `perform_bounded` | `&mut self, f, starting_point, inf_bound: &Vector, sup_bound: &Vector, stop_on_divergent: bool` | `Result<(), MathError>` | `Perform(F, StartingPoint, InfBound, SupBound, theStopOnDivergent = false)` |
| `perform_with_test` | `&mut self, f, starting_point, stop_on_divergent, test: &mut dyn FunctionSetRootTest` | `Result<(), MathError>` | `Perform` of a sub-class redefining `IsSolutionReached` |
| `perform_bounded_with_test` | `&mut self, f, starting_point, inf_bound, sup_bound, stop_on_divergent, test` | `Result<(), MathError>` | the same with bounds |
| `is_done` / `is_divergent` | `&self` | `bool` | `IsDone` / `IsDivergent` |
| `nb_iterations` / `state_number` | `&self` | `Result<i32, MathError>` | `NbIterations` / `StateNumber` |
| `root` | `&self` | `Result<&Vector, MathError>` | `Root()` |
| `root_into` | `&self, out: &mut Vector` | `Result<(), MathError>` | `Root(math_Vector&)` |
| `derivative` | `&self` | `Result<&Matrix, MathError>` | `Derivative()` |
| `derivative_into` | `&self, out: &mut Matrix` | `Result<(), MathError>` | `Derivative(math_Matrix&)` |
| `function_set_errors` | `&self` | `Result<&Vector, MathError>` | `FunctionSetErrors()` (`Delta`) |
| `function_set_errors_into` | `&self, out: &mut Vector` | `Result<(), MathError>` | `FunctionSetErrors(math_Vector&)` |
| `dump` | `&self` | `String` | `Dump` |

* `perform` is `perform_bounded` with the object's own bounds, which are `RealFirst()` / `RealLast()` for every unknown
  (the C++ members `InfBound` / `SupBound`, never changed: a `perform_bounded` does not change them).
* `perform*` return `Err(Dimension)` when `nb_equations() <= 0` or a vector's length differs from `nb_variables()`:
  the C++ `throw Standard_DimensionError` (`throws` in the golden file). Otherwise they never fail in the golden file.
  Every other state (`Done`, `Kount`, `State`, `myIsDivergent`, ...) is reset or kept exactly as the C++ does; a second
  `perform` on the same object is in the golden file (`function_set_root_staged`). `Save` is the C++ vector
  `0..=nb_iterations`. In the C++, `State` is only assigned where `GetStateNumber` is called; a `stop_on_divergent`
  return keeps the previous one.
* The accessors are `Err(NotDone)` when not done (`throws`). `root_into` / `function_set_errors_into` are
  `Err(Dimension)` when `out.length() != nb_variables()` (`unchecked`: the check is in the library, compiled out);
  `derivative_into` is `Err(Dimension)` when `out.col_number() != nb_variables()` or `out.row_number() != nb_equations()`
  (`throws`: the inline check, and the inline `math_Matrix::operator=`).
* `function_set_errors` is `Delta`, which OCCT does not write on some paths (a start at the solution): such cases are
  not in the golden file (the main families do not print it; `function_set_root_access` has the reproducible ones).
* `without_tolerance` leaves `Tol` unspecified (C++: uninitialised; the golden file calls `set_tolerance` first).

### `NewtonFunctionSetRoot` (`math_NewtonFunctionSetRoot`)

Newton's method on a square system (`LU_Decompose` with the pivot `1.0e-30` and the object's `Scratch` vector:
`recipes::lu_decompose_with_scratch`, then `recipes::lu_solve`), clamped to the bounds after every step.

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `NewtonFunctionSetRoot::new` | `f: &dyn FunctionSetWithDerivatives, x_tolerance: &Vector, f_tolerance: f64, nb_iterations: i32` | `Result<Self, MathError>` | `(F, XTolerance, FTolerance, NbIterations = 100)` |
| `NewtonFunctionSetRoot::without_tolerance` | `f, f_tolerance: f64, nb_iterations: i32` | `Result<Self, MathError>` | `(F, FTolerance, NbIterations = 100)` (`TolX` unset until `set_tolerance`) |
| `set_tolerance` | `&mut self, x_tolerance: &Vector` | `Result<(), MathError>` | `SetTolerance` |
| `perform` | `&mut self, f: &mut dyn FunctionSetWithDerivatives, starting_point: &Vector` | `Result<(), MathError>` | `Perform(F, StartingPoint)`: bounds `RealFirst()` / `RealLast()` |
| `perform_bounded` | `&mut self, f, starting_point, inf_bound: &Vector, sup_bound: &Vector` | `Result<(), MathError>` | `Perform(F, StartingPoint, InfBound, SupBound)` |
| `perform_with_test` / `perform_bounded_with_test` | the same `+ test: &mut dyn NewtonFunctionSetRootTest` | `Result<(), MathError>` | a sub-class redefining `IsSolutionReached` |
| `is_done` | `&self` | `bool` | `IsDone` |
| `root`, `function_set_errors` (`DeltaX`) | `&self` | `Result<&Vector, MathError>` | `Root()`, `FunctionSetErrors()` |
| `derivative` (`Jacobian`) | `&self` | `Result<&Matrix, MathError>` | `Derivative()` |
| `root_into`, `function_set_errors_into` | `&self, out: &mut Vector` | `Result<(), MathError>` | `Root(math_Vector&)`, `FunctionSetErrors(math_Vector&)` |
| `derivative_into` | `&self, out: &mut Matrix` | `Result<(), MathError>` | `Derivative(math_Matrix&)` |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `dump` | `&self` | `String` | `Dump` |

* The accessors are inline: `Err(NotDone)` when not done, and the `*_into` ones `Err(Dimension)` when `out` has another
  length (another shape), all `throws` (the inline `operator=` checks the size).
* `StateNumber` is declared in the C++ header but defined nowhere in OCCT 8.0.1 (it cannot be called): it is not part
  of the API. The `State` is still obtained (`GetStateNumber` is called when the solution is reached).
* The C++ works only for square systems (`FValues` has `NbVariables()` elements) and copies the starting point without
  a length check: `perform*` return `Err(Dimension)` when `nb_equations() != nb_variables()` or a vector has another
  length (not in the golden file).

### `NewtonMinimum` (`math_NewtonMinimum`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `NewtonMinimum::new` | `f: &dyn MultipleVarFunctionWithHessian, tolerance: f64, nb_iterations: i32, convexity: f64, with_singularity: bool` | `Result<Self, MathError>` | `(F, Tolerance = Precision::Confusion(), NbIterations = 40, Convexity = 1.0e-6, WithSingularity = true)` |
| `set_boundary` | `&mut self, left: &Vector, right: &Vector` | `Result<(), MathError>` | `SetBoundary` |
| `perform` | `&mut self, f: &mut dyn MultipleVarFunctionWithHessian, starting_point: &Vector` | `Result<(), MathError>` | `Perform` |
| `perform_with_test` | `&mut self, f, starting_point, test: &mut dyn NewtonMinimumTest` | `Result<(), MathError>` | a sub-class redefining `IsConverged` |
| `is_done` | `&self` | `bool` | `IsDone` |
| `get_status` | `&self` | `MathStatus` | `GetStatus` |
| `location`, `gradient` | `&self` | `Result<&Vector, MathError>` | `Location()`, `Gradient()` |
| `location_into`, `gradient_into` | `&self, out: &mut Vector` | `Result<(), MathError>` | `Location(math_Vector&)`, `Gradient(math_Vector&)` |
| `minimum` | `&self` | `Result<f64, MathError>` | `Minimum` |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `dump` | `&self` | `Result<String, MathError>` | `Dump` |

* The Hessian's smallest eigenvalue is `mathlin::Jacobi`'s (`CalculVP.Values()(CalculVP.Values().Min())`), the step is
  `Gauss` with the pivot `CTol / 100`. `Done` is set to true at the start of `perform` and only set to false by the
  failure returns: a run that stops on `math_DirectionSearchError` after the loop is still done. `Convex` is never
  reset (a second `perform` keeps `false`). `F.Value` is retried with half the step until it succeeds (the golden
  file has no case that does not end); the reduction loop ignores its flag.
* `set_boundary` is `Err(Dimension)` when a bound's length differs from `nb_variables()` (`unchecked`: the C++ copies
  without a check); the bounds are copied by position.
* The accessors are inline: `Err(NotDone)` when not done, and `*_into` `Err(Dimension)` for another length (`throws`).
* `dump` writes `Location()`, `Minimum()`, `NbIterations()`, whose checks are compiled out of the library: `Err(NotDone)`
  when not done (`unchecked`; the location is not written then).
* `IsConvex` is declared in the header but defined nowhere in OCCT 8.0.1: not part of the API.

### `Bfgs` (`math_BFGS`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `Bfgs::new` | `nb_variables: i32, tolerance: f64, nb_iterations: i32, zeps: f64` | `Result<Self, MathError>` | `(NbVariables, Tolerance = 1.0e-8, NbIterations = 200, ZEPS = 1.0e-12)` |
| `set_boundary` | `&mut self, left: &Vector, right: &Vector` | `Result<(), MathError>` | `SetBoundary` (as `NewtonMinimum`'s) |
| `perform` | `&mut self, f: &mut dyn MultipleVarFunctionWithGradient, starting_point: &Vector` | `Result<(), MathError>` | `Perform` |
| `perform_with_test` | `&mut self, f, starting_point, test: &mut dyn BfgsTest` | `Result<(), MathError>` | a sub-class redefining `IsSolutionReached` |
| `is_done` | `&self` | `bool` | `IsDone` |
| `location`, `gradient` | `&self` | `Result<&Vector, MathError>` | `Location()`, `Gradient()` |
| `location_into`, `gradient_into` | `&self, out: &mut Vector` | `Result<(), MathError>` | `Location(math_Vector&)`, `Gradient(math_Vector&)` |
| `minimum` | `&self` | `Result<f64, MathError>` | `Minimum` |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `dump` | `&self` | `String` | `Dump` |

The line search (`MinimizeDirection`, `ComputeInitScale`, `ComputeMinMaxScale`) runs `math1d::BracketMinimum::new(0.0,
lambda)` with `set_limits` (when bounded), `set_fa`, `set_fb`, `perform`, then `math1d::BrentMinimum::with_fbx(1.e-03,
Fxx, 100, 1.e-08)` on the private `DirFunction` (a `math1d::FunctionWithDerivative` along the line: its `Value` calls
`F.Value`, its `Values` / `Derivative` call `F.Values`). Accessors as `NewtonMinimum`'s (`throws`).

### `Frpr` (`math_FRPR`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `Frpr::new` | `f: &dyn MultipleVarFunctionWithGradient, tolerance: f64, nb_iterations: i32, zeps: f64` | `Result<Self, MathError>` | `(F, Tolerance, NbIterations = 200, ZEPS = 1.0e-12)` |
| `perform` | `&mut self, f: &mut dyn MultipleVarFunctionWithGradient, starting_point: &Vector` | `Result<(), MathError>` | `Perform` |
| `perform_with_test` | `&mut self, f, starting_point, test: &mut dyn FrprTest` | `Result<(), MathError>` | a sub-class redefining `IsSolutionReached` |
| `is_done`, `location`, `location_into`, `gradient`, `gradient_into`, `minimum`, `nb_iterations`, `dump` | | | as `Bfgs` |

The line search is `math1d::BracketMinimum::with_function(F_Dir, 0.0, 1.0)` then `math1d::BrentMinimum::new(1.e-10,
100, 1.0e-12)` on the private `DirFunctionTer`. `gradient()` is the C++ `TheGradient`, which holds the last search
direction at the end (as in OCCT).

### `Powell` (`math_Powell`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `Powell::new` | `f: &dyn MultipleVarFunction, tolerance: f64, nb_iterations: i32, zeps: f64` | `Result<Self, MathError>` | `(F, Tolerance, NbIterations = 200, ZEPS = 1.0e-12)` |
| `perform` | `&mut self, f: &mut dyn MultipleVarFunction, starting_point: &Vector, starting_directions: &Matrix` | `Result<(), MathError>` | `Perform` |
| `perform_with_test` | `&mut self, f, starting_point, starting_directions, test: &mut dyn PowellTest` | `Result<(), MathError>` | a sub-class redefining `IsSolutionReached` |
| `is_done`, `location`, `location_into`, `minimum`, `nb_iterations`, `dump` | | | as `Bfgs` (no gradient) |

The line search is FRPR's (`math1d::BracketMinimum::with_function(F, 0.0, 1.0)`, `BrentMinimum::new(1.0e-10, 100,
1.0e-12)`) on the private `DirFunctionBis`. The directions are copied by position (`TheDirections =
StartingDirections`). OCCT ignores the flag of `F.Value(TheLocation, ...)` here. `TheDirections(j, ibig)` with
`ibig == 0` (no decrease along any direction) would be out of range in the C++: `perform` returns `Err(Range)` then
(not in the golden file).

### `Uzawa` (`math_Uzawa`)

Minimises `Norm(X - X0)` under `Cont X = Secont`, where the first `Nce` rows are equations and the last `Nci`
inequalities (`<`): the direct method (`math_Crout` on the lower triangle of `Cont Cont^T`) when `Nci == 0`, otherwise
Uzawa's iterations. There are no callbacks.

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `Uzawa::new` | `cont: &Matrix, secont: &Vector, starting_point: &Vector, eps_lix: f64, eps_lic: f64, nb_iterations: i32` | `Result<Self, MathError>` | `(Cont, Secont, StartingPoint, EpsLix = 1.0e-06, EpsLic = 1.0e-06, NbIterations = 500)`: `Nce = Cont.RowNumber()`, `Nci = 0` |
| `Uzawa::with_constraints` | `cont, secont, starting_point, nce: i32, nci: i32, eps_lix, eps_lic, nb_iterations` | `Result<Self, MathError>` | the 8-argument constructor |
| `is_done` | `&self` | `bool` | `IsDone` |
| `value`, `initial_error`, `error` | `&self` | `Result<&Vector, MathError>` | `Value` (`Resul`), `InitialError` (`Errinit`), `Error` (`Erruza`) |
| `duale` | `&self` | `Vector` | `Duale(V)` (`Vardua`, bounds `1..=Cont.RowNumber()`; no check in C++) |
| `nb_iterations` | `&self` | `Result<i32, MathError>` | `NbIterations` |
| `inverse_cont` | `&self` | `Result<&Matrix, MathError>` | `InverseCont` (`CTCinv`) |
| `dump` | `&self` | `String` | `Dump` |

* **The 4th argument of the 8-argument constructor is the number of equality constraints `Nce`, the 5th the number of
  inequalities `Nci`** (the order of `math_Uzawa.cxx`, which calls `Perform(..., Nce, Nci, ...)`; the header names
  them the other way round). Rows `1..=Nce` are equalities.
* The constructors are `Err(Dimension)` when `secont.length() != cont.row_number()` or `nce + nci != cont.row_number()`,
  `Err(Range)` when `cont.row_number() > cont.col_number()` (the C++ writes `Errinit(i)` for `i` up to the row count
  into a vector of `cont.col_number()` elements: out of range, with the check compiled out), and `Err(Singular)` when `nci == 0` and `math_Crout` (default pivot `1.0e-20`) of `C C^T` is not done: OCCT's checks
  are compiled out (`unchecked` in the golden file). `cont` and the vectors have the lower bound 1.
* The accessors are `Err(NotDone)` when not done (`throws`).
* `InitialError` has `Cont.ColNumber()` elements of which OCCT writes the first `Cont.RowNumber()` only; the golden
  file prints only those (the others are free, e.g. 0). `inverse_cont` after the iterative method (never written in
  C++) is not in the golden file. `Erruza` is read before it is written in the first iteration only where the C++
  does not use the value.

### `BissecNewton` (`math_BissecNewton`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `BissecNewton::new` | `x_tolerance: f64` | `Self` | constructor |
| `perform` | `&mut self, f: &mut dyn FunctionWithDerivative, bound1: f64, bound2: f64, nb_iterations: i32` | `()` | `Perform(F, Bound1, Bound2, NbIterations = 100)` |
| `perform_with_test` | `&mut self, f, bound1, bound2, nb_iterations, test: &mut dyn BissecNewtonTest` | `()` | a sub-class redefining `IsSolutionReached` |
| `is_done` | `&self` | `bool` | `IsDone` |
| `root` / `derivative` / `value` | `&self` | `Result<f64, MathError>` | `Root` (`x`) / `Derivative` (`df`) / `Value` (`f`) |
| `dump` | `&self` | `String` | `Dump` |

`f` / `df` keep their values when a `Values` call fails (`fl` / `fh` are locals of `Perform`; the golden file has no
case where a never-written local is used). `aFTol` is `RealEpsilon()`. Accessors: `Err(NotDone)` (`throws`).

### `GaussSingleIntegration` (`math_GaussSingleIntegration`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `GaussSingleIntegration::new` | | `Self` | default constructor (not done) |
| `GaussSingleIntegration::with_order` | `f: &mut dyn Function, lower: f64, upper: f64, order: i32` | `Self` | `(F, Lower, Upper, Order)` |
| `GaussSingleIntegration::with_tolerance` | `f, lower, upper, order, tol: f64` | `Self` | `(F, Lower, Upper, Order, Tol)` (interval halving, at most 13 passes) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `value` | `&self` | `Result<f64, MathError>` | `Value` (`Err(NotDone)`: `throws`) |
| `dump` | `&self` | `String` | `Dump` |

The order is `min(gauss_points_max(), order)`. An order of `0` or `-1` gives a done result `0.0 * xr` without calling
the function (the C++ loops do not run; the golden file has such cases); lower orders read before the weight table
in the C++ and are not in the golden file. `with_tolerance` does not check that the first pass is done, as the C++.

### `GaussMultipleIntegration` (`math_GaussMultipleIntegration`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `GaussMultipleIntegration::new` | `f: &mut dyn MultipleVarFunction, lower: &Vector, upper: &Vector, order: &IntegerVector` | `Self` | constructor |
| `is_done` | `&self` | `bool` | `IsDone` |
| `value` | `&self` | `Result<f64, MathError>` | `Value` (`Err(NotDone)`: `throws`) |
| `dump` | `&self` | `String` | `Dump` |

The orders are clamped to `gauss_points_max()`; the recursion (`recursive_iteration` of the private
`IntegrationFunction`) returns the flag of the **last** point of each loop only, and goes on after a failing call (whose
value is not added), as the C++. An order of 0 for a variable gives a result that is not done. `order` has the lower
bound 1 and `nb_variables()` elements, `lower` / `upper` are copied by position.

### `GaussSetIntegration` (`math_GaussSetIntegration`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `GaussSetIntegration::new` | `f: &mut dyn FunctionSet, lower: &Vector, upper: &Vector, order: &IntegerVector` | `Result<Self, MathError>` | constructor |
| `is_done` | `&self` | `bool` | `IsDone` |
| `value` | `&self` | `Result<&Vector, MathError>` | `Value` (bounds `1..=NbEquations()`; `Err(NotDone)`: `throws`) |
| `dump` | `&self` | `String` | `Dump` |

The C++ reads `Lower.Value(Lower.Lower())`, `Upper.Value(Upper.Lower())`, `Order.Value(Order.Lower())` (any bounds). Its
`Standard_NotImplemented_Raise_if(NbVar != 1 || Order > GaussPointsMax())` is compiled out of the library:
`Err(Dimension)` for `nb_variables() != 1` and `Err(Range)` for an order above `gauss_points_max()` (`unchecked`). An
order of `0` or `-1` gives a done result of zeros (`Val.Init(0)` then `Val *= Xr`).

### `KronrodSingleIntegration` (`math_KronrodSingleIntegration`)

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `KronrodSingleIntegration::new` | | `Self` | default constructor |
| `KronrodSingleIntegration::with_points` | `f: &mut dyn Function, lower: f64, upper: f64, nb_pnts: i32` | `Self` | `(F, Lower, Upper, NbPnts)` |
| `KronrodSingleIntegration::with_tolerance` | `f, lower, upper, nb_pnts, tolerance: f64, max_nb_iter: i32` | `Self` | `(F, Lower, Upper, NbPnts, Tolerance, MaxNbIter)` |
| `perform` | `&mut self, f: &mut dyn Function, lower, upper, nb_pnts` | `()` | `Perform` (4 arguments) |
| `perform_with_tolerance` | `&mut self, f, lower, upper, nb_pnts, tolerance, max_nb_iter` | `()` | `Perform` (6 arguments) |
| `is_done` | `&self` | `bool` | `IsDone` |
| `value`, `error_reached`, `absolut_error` | `&self` | `Result<f64, MathError>` | `Value`, `ErrorReached`, `AbsolutError` |
| `order_reached`, `nb_iter_reached` | `&self` | `Result<i32, MathError>` | `OrderReached`, `NbIterReached` |
| `KronrodSingleIntegration::gk_rule` | `f: &mut dyn Function, lower, upper, gauss_p: &Vector, gauss_w: &Vector, kronrod_p: &Vector, kronrod_w: &Vector` | `Option<(f64, f64)>` | static `GKRule`: `Some((value, error))`, `None` for `false` |

* The points are `mathlin::kronrod_points_and_weights` and `ordered_gauss_points_and_weights` (computed beyond the
  tables). The adaptive loop keeps the C++ sequences of intervals, errors and values (insertion after the interval of
  the largest error, the first one of equal errors) and the `count > 50` stop.
* The accessors are `Err(NotDone)` when not done (`throws`). The members keep their values between `perform` calls
  where the C++ does (e.g. `myNbPntsReached` of a failed call); the golden file has a second `perform` on the same
  object (`kronrod_perform`).
* `gk_rule` reads `kronrod_p.value(i)`, `kronrod_w.value(i)`, `gauss_w.value(i / 2)` for `i` from 1 (lower bound 1) and
  ignores `gauss_p`.

## What the golden file contains

`tests/mathnd.txt` (see `FAMILIES`, `GOLDEN_LINES` and `MIN_DISTINCT` in `tests/mathnd.rs`): 56 families, every one with
inputs, no line repeated. Families are in groups per class: the construction families print `IsDone` (and
`IsDivergent`, `GetStatus`), the call counts and trace hash of the function, and every result of a finished solve; the
`*_access` families print one accessor call (so that accessors of solvers that are not done, and the `*_into` overloads
with outputs of the wrong size, are covered); the `*_dump` families print the `Dump` text; the `*_custom` families
run `perform_with_test` with one of four tests (a relaxed test, a test that calls the function, a test that never
stops, a stricter test using the state fields). `test_tf`, `test_mv`, `test_fs` replay the judge's own test functions
(Rust vs C++) and have no API of `mathnd` to test beyond the traits.

| families | Rust calls |
|---|---|
| `test_tf`, `test_mv`, `test_fs` | the judge's `Tf`, `Mv`, `Fs` only |
| `function_set_root`, `_bounded`, `_staged`, `_custom`, `_access`, `_dump` | `FunctionSetRoot::new` / `without_tolerance` + `set_tolerance`, `perform` / `perform_bounded` (once or twice) / `perform*_with_test`, accessors |
| `newton_set_root`, `_bounded`, `_staged`, `_custom`, `_access`, `_dump` | `NewtonFunctionSetRoot::new` / `without_tolerance` + `set_tolerance`, `perform*`, accessors |
| `newton_minimum`, `_bounded`, `_custom`, `_access`, `_dump` | `NewtonMinimum::new`, `set_boundary`, `perform` (once or twice), `perform_with_test`, accessors |
| `bfgs`, `_bounded`, `_custom`, `_access`, `_dump` | `Bfgs::new`, `set_boundary`, `perform` (once or twice), `perform_with_test`, accessors |
| `frpr`, `_custom`, `_access`, `_dump` | `Frpr::new`, `perform` (once or twice), `perform_with_test`, accessors |
| `powell`, `_custom`, `_access`, `_dump` | `Powell::new`, `perform` (once or twice), `perform_with_test`, accessors |
| `uzawa`, `uzawa_ineq`, `_access`, `_dump` | `Uzawa::new`, `Uzawa::with_constraints`, accessors |
| `bissec_newton`, `_custom`, `_access`, `_dump` | `BissecNewton::new`, `perform`, `perform_with_test`, accessors |
| `gauss_single`, `_tol`, `_access`, `_dump` | `GaussSingleIntegration::new` / `with_order` / `with_tolerance` |
| `gauss_multiple`, `_access`, `_dump` | `GaussMultipleIntegration::new` |
| `gauss_set`, `_access`, `_dump` | `GaussSetIntegration::new` |
| `kronrod`, `_tol`, `_perform`, `_access`, `_gk_rule` | `KronrodSingleIntegration::new` / `with_points` / `with_tolerance`, `perform`, `perform_with_tolerance`, `gk_rule` |

The inputs cover dimensions 1 to 6; square, over- and under-determined systems; systems whose Jacobian is singular at
the root (every equation stationary there), rank deficient (two equal equations), linear, without a root; convex,
semidefinite, indefinite and ill-conditioned quadratics, Rosenbrock functions, sums of squares of polynomial and
trigonometric residuals, trigonometric functions, quartic polynomials, kinks (`|x - k|`), a quadratic plus an
exponential, linear functions (no minimum: on a bound only); starting points at the solution, near it (1e-12 to 2
away), far, at a stationary point, outside the bounds; tolerances `0`, `1e-300`, `1e-15` up to `1e3`; iteration limits
0, 1, 2, ..., 200; bounds that contain the solution, exclude it, are degenerate (`lower = upper` in a coordinate),
infinite (`+-f64::MAX`, `+-2e100`), or tight around the root; starting points with other lower bounds than 1; functions
that fail (`None` / `false`) in a box, for one or several callbacks; every state-number mode; Uzawa systems with
dependent rows, tiny coefficients, more rows than columns, only equalities, only inequalities, mixed; integration of
polynomials of degree 0 to 8, `exp`, trigonometric sums, kinks and flat parts, over intervals given reversed, empty or
tiny, with orders 1, 2, ..., 61, above 61 and 0, -1, and Kronrod point counts below 3, even, up to 123 and beyond.

Cases are **not** in the golden file when they depend on uninitialised memory (OCCT reads a variable that was never
written), when a callback would see or return a non-finite number, when a case runs away (more than 300000 callback
calls), and, for the Kronrod families, when moving every `pow` (and `sin`, `cos`, ...) result of the C library by one
ulp changes the output by more than 3 ulps.

### Where the golden file says `throws` and where `unchecked`

The OCCT 8.0.1 libraries are built without the `Standard_*_Raise_if` / `StdFail_*_Raise_if` checks (the library has
no reference to `StdFail_NotDone` or `Standard_NotImplemented`), but the accessors that are inline in the headers
(`*.lxx`, the `const math_Vector&` getters of `math_FunctionSetRoot.hxx`, `NCollection_Array1::CopyValues` used by an
inline `operator=`) are compiled into the caller and keep their checks. The oracle decides `throws` by catching the
exception the real call raises; it prints `unchecked` where the documented check is compiled out (it calls the
function when that is harmless, and sees no exception).

* `throws`: `perform*` of `FunctionSetRoot` with a dimension error (an explicit `throw`); the `StdFail_NotDone` checks of
  every result accessor (`*_access` families) and the inline `*_into` overloads with a wrong size
  (`NewtonFunctionSetRoot`, `NewtonMinimum`, `Bfgs`, `Frpr`, `Powell`, `FunctionSetRoot::derivative_into`).
* `unchecked`: `FunctionSetRoot::root_into` / `function_set_errors_into` with a wrong length (in the library);
  `set_boundary` of `NewtonMinimum` / `Bfgs` with a wrong length; `NewtonMinimum::dump` when not done; `Uzawa`'s
  dimension errors and singular `C C^T` (decided by running `math_Crout` in the oracle); `GaussSetIntegration` with two
  variables or an order above 61.

### Regenerating the golden file

```
mkdir -p oracle-bin && c++ -std=c++17 -O2 oracle/mathnd.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKMath -lTKernel \
  -o oracle-bin/mathnd && LD_LIBRARY_PATH=$OCCT/lib oracle-bin/mathnd > tests/mathnd.txt
```

The output is deterministic (fixed xorshift seeds, one per family; 4 differently poisoned runs per case). The oracle
interposes the C library's `sin cos tan asin acos atan atan2 pow sincos` (to re-run the Kronrod cases with every result
moved by an ulp); this is glibc/x86-64 specific and only matters for generating the file, not for the test. The file
was generated with glibc 2.39 on x86-64. `MATHND_SCHEMAS=1` prints every family's schema to stderr.
