# `plib`: Rust API contract

This contract covers all of OCCT 8.0.1's `PLib` package (`cpp/PLib/`): the static functions of `PLib`
(polynomial evaluation, Lagrange and cubic Hermite interpolation, Hermite polynomials, poles <-> power-basis
coefficients, trimming, rational derivatives, binomials, Jacobi parameters, constraint orders, polynomial length),
`PLib_JacobiPolynomial` and `PLib_HermitJacobi`. It is the module `occt_port::plib` (add `pub mod plib;` to
`src/lib.rs`; `src/` may use private submodules). The tests are `tests/plib.rs` with the golden file `tests/plib.txt`
printed by `oracle/plib.cpp` (real OCCT 8.0.1). Nothing that exists in `src/` changes. The package uses
`occt_port::gp` (`Pnt`, `Pnt2d`) and `occt_port::mathlin` (`Matrix` of `hermite_coefficients`, `Gauss`,
`Vector::norm`, `gauss_points` / `gauss_weights`): the translation must use them, not copies. Note that `PLib.hxx` of
8.0.1 has no `PolesCoefficients` functions; the "both directions" of the old API are `coefficients_poles_*` (power basis
to Bezier poles) and the trimming functions.

## Conventions

* **Arrays.** Every OCCT array argument becomes a slice (`&[T]`), every array result a `Vec<T>`. OCCT's lower bounds
  (`NCollection_Array1<double>(1, n)`, `(0, n)`, ...) never influence a result of PLib (the code works with offsets from
  the lower bound), so a slice is indexed from 0 and "element `k` of the OCCT array" is `slice[k - lower]`. The only
  place where the lower bound is part of the contract is stated below (`hermite_interpolate`: the coefficient array
  has lower bound 0 and the constraint arrays have bounds `(1..=dimension, 0..=order)`).
  An in/out array (`Trimming`, `HermiteCoefficients`' matrix) is taken by value/reference and returned as described.
* **Flat layouts.** A polynomial of `Dimension` components is stored group by group: the coefficients of `u^k`
  are the `Dimension` values `coeffs[k * Dimension .. (k + 1) * Dimension]` (power `0` first). A list of points
  of dimension `Dimension` has point `k` at `values[k * Dimension ..]`. Derivative results are stored the same
  way, derivative `d` at `res[d * Dimension ..]`. Slices may be longer than needed (the extra values are not read);
  a shorter one is an error (below).
* **2D arrays.** `NCollection_Array2<T>` becomes `plib::Array2<T>`, a row-major array with 0-based indices:
  `Array2::new(rows: usize, cols: usize, data: Vec<T>) -> Result<Array2<T>, PLibError>` (`Err(Argument)` unless
  `data.len() == rows * cols`), `rows()`, `cols()`, `get(row, col) -> Option<&T>`, `as_slice() -> &[T]` (row-major),
  `into_vec()`. It derives `Debug, Clone, PartialEq`. OCCT row `r` / column `c` (with lower bounds `lr`, `lc`) is
  `(r - lr, c - lc)`. In OCCT's naming `ColLength()` is the number of rows and `RowLength()` the number of columns.
* **Optional weights.** `const NCollection_Array1<double>*` (`NULL` = no weights; `PLib::NoWeights()` and
  `NoWeights2()` return `NULL`) becomes `Option<&[f64]>` / `Option<&Array2<f64>>`; `PLib::NoWeights()` is `None`.
  Functions that return weights return `Option<Vec<f64>>` / `Option<Array2<f64>>`, `Some` exactly when the input weights
  are `Some`.
* **Scalars.** Reals are `f64`; every OCCT `int` is `i32` (orders, degrees, dimensions, counts that are parameters).
  Lengths of slices are `usize`.
* **Out-parameters** become returned tuples in the C++ argument order. A C++ function returning a status code and
  filling an array returns `(code, Vec)`.
* **Overloads.** Rust has none; each C++ overload has its own name, see the table.
* **Arithmetic.** All results are bit-compared with OCCT (every family of the golden file is in the `EXACT` list of
  `tests/plib.rs`: PLib uses only `+ - * /` and `sqrt`). The operations must be done in the same order as in the C++
  source, with no fused multiply-add and no reassociation. `sqrt` is `f64::sqrt`.
* **Errors.** `PLibError` (derive `Debug, Clone, PartialEq, Eq`; `impl From<MathError> for PLibError`):

  | variant | stands for |
  |---|---|
  | `Construction` | `Standard_ConstructionError` raised by OCCT (constructors of the two classes, `jacobi_parameters`, `niv_constr`, `constraint_order`, `points`) |
  | `Failure` | `Standard_Failure` raised by OCCT (`trimming_*` with a weight array of the wrong length) |
  | `OutOfRange` | a check that OCCT 8.0.1 *as built* has compiled out, or never had, and that would have failed: OCCT then returns unspecified numbers, reads or writes outside an array, or crashes. The oracle prints `unchecked`; the Rust API must return an error. |
  | `Argument` | wrong shape of a Rust argument (`Array2::new`, slices too short where the C++ would read outside the array, ...) |
  | `Math(MathError)` | an error of `mathlin` used inside |

  The test only checks `Err` against the oracle's `throws` / `unchecked`; the variant is for the caller. Where a
  condition below says "`OutOfRange`", any variant is accepted by the test, but use `OutOfRange` (or `Argument` for
  slices that are too short).
* **Not judged but required.** A slice shorter than the C++ reads, a negative count or a zero dimension that OCCT does not
  check must return `Err` and never panic (the lints forbid indexing and `unwrap`). A few inputs for which the C++
  result is well defined but accidental (an array length that is not a multiple of `dim`, `Dimension < 0` in
  `hermite_interpolate`) are errors too; they are not in the golden file.

### Where the golden file says `throws` and where `unchecked`

* `throws` (the line was produced by a caught `Standard_Failure`): only the explicit `throw` statements of the
  library. `jacobi_parameters`, `constraint_conversions` (`niv_constr` / `constraint_order`), the constructors of
  both classes (`jacobi_new`, `hermit_new` and every class family with a bad `(work_degree, shape)`),
  `jacobi_points` (bad number of Gauss points) and the `trimming_*` families (weights of the wrong length).
* `unchecked`: everything else that can be invalid, because `Standard_OutOfRange_Raise_if`, the
  `math_Vector` / `math_Matrix` range checks etc. are compiled out of the OCCT libraries (`No_Exception`) and `PLib`
  has no explicit test. Where the oracle can call the function safely it does, and would print `throws` if a check
  existed (`bin` near the table: the probe showed no exception). Where the call would read or write outside an
  array, divide an integer by zero or call `math_Gauss::Solve` after a failed elimination (undefined behaviour,
  crashes observed), the oracle does not call it and prints `unchecked`.

## Types

```rust
pub enum PLibError { Construction, Failure, OutOfRange, Argument, Math(MathError) }

/// GeomAbs_Shape; the discriminants are the C++ enumerator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum GeomAbsShape { C0 = 0, G1 = 1, C1 = 2, G2 = 3, C2 = 4, C3 = 5, Cn = 6 }

pub struct Array2<T> { /* row-major */ }
```

`GeomAbsShape` is `GeomAbs_Shape` (`C0 G1 C1 G2 C2 C3 CN`). The test converts the golden file's ordinals 0..=6 with a
`match` of its own, and writes `shape as i32` for an output.

## Functions of `PLib` (`occt_port::plib::`)

Notation: `dim` is the `Dimension`; "groups" are the groups of `dim` values described under *Flat layouts*.

### Evaluation

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `eval_polynomial` | `u: f64, derivative_order: i32, degree: i32, dimension: i32, coeffs: &[f64]` | `Result<Vec<f64>, PLibError>` | `PLib::EvalPolynomial` |
| `no_derivative_eval_polynomial` | `u: f64, degree: i32, dimension: i32, degree_dimension: i32, coeffs: &[f64]` | `Result<Vec<f64>, PLibError>` | `PLib::NoDerivativeEvalPolynomial` |
| `eval_poly2var` | `u: f64, v: f64, u_derivative: i32, v_derivative: i32, u_degree: i32, v_degree: i32, dimension: i32, coeffs: &[f64]` | `Result<Vec<f64>, PLibError>` | `PLib::EvalPoly2Var` |
| `eval_lagrange` | `u: f64, derivative_order: i32, degree: i32, dimension: i32, values: &[f64], parameters: &[f64]` | `Result<(i32, Vec<f64>), PLibError>` | `PLib::EvalLagrange` |
| `eval_cubic_hermite` | `u: f64, derivative_order: i32, dimension: i32, values: &[f64], derivatives: &[f64], parameters: &[f64]` | `Result<(i32, Vec<f64>), PLibError>` | `PLib::EvalCubicHermite` |

* `eval_polynomial`: `coeffs` holds the `degree + 1` groups of the polynomial `sum_k c_k u^k`. Returns the
  `(derivative_order + 1) * dim` values `value, 1st derivative, ..., derivative_order-th derivative` (derivative
  `d` at `res[d * dim ..]`; orders above `degree` are exactly `0.0`). The C++ code has special loops for orders 1 and
  2 (dimensions 1..=15 unrolled, larger dimensions at run time) and a general loop for the others; they must all give
  the bits of the C++ (the three Horner variants evaluate the same recurrences in different loop order, the order of
  the operations per component is what counts). `OutOfRange` when `derivative_order < 0`, `degree < 0`, `dimension < 1`
  or `coeffs.len() < (degree + 1) * dimension`.
* `no_derivative_eval_polynomial`: the value only (`dim` values). The coefficient groups are read from the offset
  `degree_dimension` backwards: the top group is `coeffs[degree_dimension .. degree_dimension + dim]`, the next
  `coeffs[degree_dimension - dim ..]`, ... down to `degree_dimension - degree * dim`. Normally
  `degree_dimension == degree * dimension`; other values (an array holding more groups than `degree + 1`) are fine as long as
  every group lies inside the slice. `OutOfRange` when `degree < 0`, `dimension < 1`,
  `degree_dimension < degree * dimension` or `degree_dimension + dimension > coeffs.len()`.
* `eval_poly2var`: polynomial `sum_ij c_ij u^i v^j` (`i <= u_degree`, `j <= v_degree`) with `dim` components. **The
  coefficient of `u^i v^j` is the group `coeffs[(i * (v_degree + 1) + j) * dim ..]`**, so `coeffs` has
  `(u_degree + 1) * (v_degree + 1) * dim` values (the comment in `PLib.hxx` lists the groups in the other order;
  the code and the golden file are authoritative: first `eval_polynomial` in `u` with `(v_degree + 1) * dim`
  components, then `eval_polynomial` in `v` on the selected derivative group). Returns the `dim` components of the
  partial derivative of order `u_derivative` in `u` and `v_derivative` in `v` at `(u, v)`. `OutOfRange` when an order,
  degree or `dimension` is negative / below 1, or `coeffs` is too short.
* `eval_lagrange`: interpolating polynomial through `degree + 1` points: `values` has the points group by group,
  `parameters` the `degree + 1` parameters (any order). Returns `(code, res)`: `code` is `0`, or `1` when two
  parameters that enter a divided difference differ by less than `f64::MIN_POSITIVE` (`RealSmall()`; in the code
  `abs(difference) < RealSmall()`); with code `1` `res` is unspecified (return an empty `Vec`; the test ignores it).
  With code `0`, `res` has `(min(derivative_order, degree) + 1) * dim` values: orders above `degree` are *not*
  part of the result (OCCT does not write them). `OutOfRange` when `derivative_order < 0`, `degree < 0`,
  `dimension < 1`, `values.len() < (degree + 1) * dim` or `parameters.len() < degree + 1`.
* `eval_cubic_hermite`: the cubic through two points: `values` = the first point then the last point (`2 * dim`),
  `derivatives` = the first derivative at the first parameter then at the last (`2 * dim`), `parameters` = `[first,
  last]`. Returns `(0, res)` with `(min(derivative_order, 3) + 1) * dim` values (the code is always 0; it is part of
  the C++ signature). `OutOfRange` when `derivative_order < 0`, `dimension < 1`, a slice is too short, or
  `parameters[0] == parameters[1]` (OCCT divides by zero without a check).

### Hermite polynomials

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `hermite_coefficients` | `first: f64, last: f64, first_order: i32, last_order: i32, matrix: &mut mathlin::Matrix` | `Result<bool, PLibError>` | `PLib::HermiteCoefficients` |
| `hermite_interpolate` | `dimension: i32, first: f64, last: f64, first_order: i32, last_order: i32, first_constr: &Array2<f64>, last_constr: &Array2<f64>` | `Result<Vec<f64>, PLibError>` | `PLib::HermiteInterpolate` |

* `hermite_coefficients`: with `n = first_order + last_order + 2` the `matrix` is `n x n` with bounds `1..=n` x `1..=n`
  (row `j` is written with `Matrix::set_row(j, &coeff)` exactly as in C++, so a matrix of another shape gives the
  `mathlin` error as `Math(..)`). Returns `Ok(false)` and leaves `matrix` untouched when
  `first_order < 0 || last_order < 0`, `|first| > 100 || |last| > 100`, `|first| + |last| < 0.01`,
  `|last - first| / (|first| + |last|) < 0.01`, or when `mathlin::Gauss::new(&mat, 1.0e-10)` is not done; otherwise
  fills the matrix (row `i` = the coefficients, lowest power first, of the Hermite polynomial `H0,i-1` for
  `i <= first_order + 1` and `H1,i-first_order-2` after that) and returns `Ok(true)`. The order of the tests is the C++
  one (orders, then magnitudes, then the elimination). `OutOfRange` when `n < 0` (OCCT builds a `math_Vector` of negative length
  first and crashes); the test uses a `1 x 1` matrix for `n < 1`.
* `hermite_interpolate`: finds the polynomial of degree `first_order + last_order + 1` with the given derivatives
  `0..=first_order` at `first` and `0..=last_order` at `last`. `first_constr(i, j)` (row `i - 1`, column `j` of the
  `Array2`) is the `j`-th derivative of component `i` (`1..=dimension`) at `first`; same for `last_constr`. The
  arrays may have more rows and columns than needed. Returns the `dimension * (first_order + last_order + 2)` coefficients
  `coef[dimension * p + (i - 1)]` = coefficient of `x^p` of component `i`. The C++ returns `false` when the elimination
  of the system matrix fails *after* it has called `math_Gauss::Solve` on the failed elimination (unspecified,
  crashes seen); the Rust function returns `Err(OutOfRange)` for a singular system (`mathlin::Gauss::new(&A, 1.0e-20)`
  not done) when `dimension >= 1`. For `dimension == 0` nothing is solved and the result is `Ok(vec![])` even if the
  system is singular. `OutOfRange` also when `first_order` or `last_order` is outside `-1..=2` (OCCT reads outside its
  `Pattern[3][6]`), both are `-1` (empty system), `dimension < 0`, or the constraint arrays have fewer than `dimension` rows or
  fewer than `order + 1` columns. The system matrix is `(0..=m) x (0..=m)`, `m = first_order + last_order + 1`, with the rows
  of `Pattern` times powers of the parameters exactly as in the C++ (see `PLib.cxx`); orders `-1` simply have no rows at that end.

### Poles and coefficients

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `set_poles_3d` | `poles: &[Pnt]` | `Result<Vec<f64>, PLibError>` | `SetPoles(Array1<gp_Pnt>, FP)` |
| `set_poles_3d_weighted` | `poles: &[Pnt], weights: &[f64]` | `Result<Vec<f64>, PLibError>` | `SetPoles(Poles, Weights, FP)` |
| `set_poles_2d` | `poles: &[Pnt2d]` | `Result<Vec<f64>, PLibError>` | `SetPoles(Array1<gp_Pnt2d>, FP)` |
| `set_poles_2d_weighted` | `poles: &[Pnt2d], weights: &[f64]` | `Result<Vec<f64>, PLibError>` | `SetPoles(Poles, Weights, FP)` |
| `get_poles_3d` | `fp: &[f64], nb_poles: i32` | `Result<Vec<Pnt>, PLibError>` | `GetPoles(FP, Poles)` |
| `get_poles_3d_weighted` | `fp: &[f64], nb_poles: i32` | `Result<(Vec<Pnt>, Vec<f64>), PLibError>` | `GetPoles(FP, Poles, Weights)` |
| `get_poles_2d` | `fp: &[f64], nb_poles: i32` | `Result<Vec<Pnt2d>, PLibError>` | `GetPoles(FP, Poles)` |
| `get_poles_2d_weighted` | `fp: &[f64], nb_poles: i32` | `Result<(Vec<Pnt2d>, Vec<f64>), PLibError>` | `GetPoles(FP, Poles, Weights)` |
| `bin` | `n: i32, p: i32` | `Result<f64, PLibError>` | `PLib::Bin` |
| `coefficients_poles_3d` | `coefs: &[Pnt], wcoefs: Option<&[f64]>` | `Result<(Vec<Pnt>, Option<Vec<f64>>), PLibError>` | `CoefficientsPoles(Array1<gp_Pnt>, ..)` |
| `coefficients_poles_2d` | `coefs: &[Pnt2d], wcoefs: Option<&[f64]>` | `Result<(Vec<Pnt2d>, Option<Vec<f64>>), PLibError>` | `CoefficientsPoles(Array1<gp_Pnt2d>, ..)` |
| `coefficients_poles_1d` | `coefs: &[f64], wcoefs: Option<&[f64]>` | `Result<(Vec<f64>, Option<Vec<f64>>), PLibError>` | `CoefficientsPoles(Array1<double>, ..)` |
| `coefficients_poles_dim` | `dim: i32, coefs: &[f64], wcoefs: Option<&[f64]>` | `Result<(Vec<f64>, Option<Vec<f64>>), PLibError>` | `CoefficientsPoles(dim, ..)` |
| `coefficients_poles_surface` | `coefs: &Array2<Pnt>, wcoefs: Option<&Array2<f64>>` | `Result<(Array2<Pnt>, Option<Array2<f64>>), PLibError>` | `CoefficientsPoles(Array2<gp_Pnt>, ..)` |

* `set_poles_*`: `FP` of OCCT is the returned `Vec`: per pole `x y z` (`x y`), or with weights `x*w y*w z*w w`
  (`x*w y*w w`) (the products as written in C++). `weights.len() >= poles.len()` is required (`Argument`).
  A zero weight is fine here.
* `get_poles_*`: `nb_poles` is the length of the OCCT output array. `fp` holds `3 * nb_poles` (2D: `2 *`) values, with
  weights `4 * nb_poles` (`3 *`) values per `x y z w` (`x y w`); poles are `x`, `y`, `z` (divided by `w`
  when weighted, the division as in C++). `OutOfRange` when `nb_poles < 1` or `fp` is too short, or (weighted) a weight is
  `0.0` (OCCT divides without a check). `fp` may be longer than needed.
* `bin(n, p)`: the binomial coefficient from the table for `n <= 25` (`BSplCLib::MaxDegree()`) as `f64`.
  `OutOfRange` unless `0 <= p <= n <= 25` (OCCT's check is compiled out and reads outside its table: golden-file lines
  `unchecked`).
* `coefficients_poles_*`: converts the `n` power-basis coefficient groups of a polynomial of degree `n - 1` into the
  `n` poles of the equal Bezier curve; with `wcoefs` the same for the weights, and the poles are divided by the weights
  at the end. The 3D and 2D versions use the `dim` version with 3 / 2 (their `SetPoles`/`GetPoles` round trip
  included; note the C++ also copies the coefficients into the temporary pole array first, which has no effect).
  `coefs.len() == n`; `dim` version: `n = coefs.len() / dim`, `coefs.len()` must be a multiple of `dim`, `dim >= 1`.
  Weights: `wcoefs.len() == n`. `OutOfRange` when `n >= 27` (the binomials `Bin(n - 1, ..)` leave the table), the weight
  array has another length, `dim < 1` or `coefs.len()` is not a multiple of `dim`. The returned weights have the length `n`.
* `coefficients_poles_surface`: the same in both directions of an `Array2` of points (rows are the first
  variable, columns the second; `Bin(cols - 1, ..)` along each row, then `Bin(rows - 1, ..)` along each column). `OutOfRange`
  when rows or columns `>= 27` or `< 1`, or the weights have another shape. The result has the shape of `coefs`.

### Trimming

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `trimming_3d` | `u1: f64, u2: f64, coefs: &[Pnt], wcoefs: Option<&[f64]>` | `Result<(Vec<Pnt>, Option<Vec<f64>>), PLibError>` | `Trimming(U1, U2, Array1<gp_Pnt>&, ..)` |
| `trimming_2d` | `u1: f64, u2: f64, coefs: &[Pnt2d], wcoefs: Option<&[f64]>` | same with `Pnt2d` | `Trimming(.., Array1<gp_Pnt2d>&, ..)` |
| `trimming_1d` | `u1: f64, u2: f64, coefs: &[f64], wcoefs: Option<&[f64]>` | `Result<(Vec<f64>, Option<Vec<f64>>), PLibError>` | `Trimming(.., Array1<double>&, ..)` |
| `trimming_dim` | `u1: f64, u2: f64, dim: i32, coefs: &[f64], wcoefs: Option<&[f64]>` | `Result<(Vec<f64>, Option<Vec<f64>>), PLibError>` | `Trimming(U1, U2, dim, ..)` |
| `u_trimming` | `u1: f64, u2: f64, coefs: &Array2<Pnt>, wcoefs: Option<&Array2<f64>>` | `Result<(Array2<Pnt>, Option<Array2<f64>>), PLibError>` | `PLib::UTrimming` |
| `v_trimming` | `v1: f64, v2: f64, coefs: &Array2<Pnt>, wcoefs: Option<&Array2<f64>>` | same | `PLib::VTrimming` |

* The C++ functions change the arrays in place; the Rust functions take the input and return the trimmed coefficients (and
  weights). The polynomial `P(u)` given by `coefs` (groups of `dim`, power 0 first) is replaced by the coefficients of
  `P(U1 + (U2 - U1) v)`, by Horner synthetic division with `lsp = U2 - U1` exactly as in C++ (`U1 == U2` gives the
  zero-length interval, `U2 < U1` is allowed).
* `Failure` when `wcoefs` is given and its length differs from `coefs.len() / dim` (C++: `Standard_Failure("PLib::Trimming :
  nbcoefs/dim != nbweights !!!")`, a real exception: golden lines `throws`). `OutOfRange` for `dim < 1` and for a `coefs.len()` that is not a
  multiple of `dim` (the C++ division by zero crashes for `dim == 0`).
* `u_trimming` trims every column (the polynomial in the first index, the rows) with `trimming_3d`; `v_trimming` every row
  (polynomial in the second index, the columns). `OutOfRange` when the weights have another shape than `coefs` (OCCT would
  read outside the weight array).

### Rational derivatives

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `rational_derivative` | `degree: i32, derivative_request: i32, dimension: i32, ders: &[f64], all: bool` | `Result<Vec<f64>, PLibError>` | `PLib::RationalDerivative` |
| `rational_derivatives` | `derivative_request: i32, dimension: i32, poles_derivatives: &[f64], weights_derivatives: &[f64]` | `Result<Vec<f64>, PLibError>` | `PLib::RationalDerivatives` |

* `rational_derivative`: derivatives of `x(t) / w(t)` up to order `N = derivative_request` from the derivatives of the
  homogeneous function. `ders` holds, for each order `0..=min(N, degree)`, the `dimension` components of `x` followed by the weight
  derivative `w` (`dimension + 1` values per order): `ders.len() >= (dimension + 1) * (min(N, degree) + 1)`; orders above
  `degree` are taken as 0. With `all` the result has `dimension * (N + 1)` values (derivative `d` of the quotient at
  `res[d * dimension ..]`), without it only the `dimension` values of order `N`. `OutOfRange` for `degree < 0`,
  `derivative_request < 0`, `dimension < 1`, `ders` too short, or `ders[dimension] == 0.0` (division by the value of the weight).
  `dimension == 3` has its own unrolled path in C++; the arithmetic is the same as the general path.
* `rational_derivatives`: `poles_derivatives` has `dimension * (N + 1)` values (derivative `d` of `x` at
  `[d * dimension ..]`), `weights_derivatives` has `N + 1`; result `dimension * (N + 1)` values like `all = true` above.
  OCCT allows the output to overwrite the input; the Rust function does not. `OutOfRange` for `N < 0`, `dimension < 1`, short
  arrays, `weights_derivatives[0] == 0.0`.

### Constraint orders, Jacobi parameters, length

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `jacobi_parameters` | `constraint_order: GeomAbsShape, max_degree: i32, code: i32` | `Result<(i32, i32), PLibError>` | `PLib::JacobiParameters` (-> `(nb_gauss_points, work_degree)`) |
| `niv_constr` | `constraint_order: GeomAbsShape` | `Result<i32, PLibError>` | `PLib::NivConstr` |
| `constraint_order` | `niv_constr: i32` | `Result<GeomAbsShape, PLibError>` | `PLib::ConstraintOrder` |
| `eval_length` | `degree: i32, dimension: i32, coeffs: &[f64], u1: f64, u2: f64` | `Result<f64, PLibError>` | `PLib::EvalLength` (without tolerance) |
| `eval_length_tol` | `degree: i32, dimension: i32, coeffs: &[f64], u1: f64, u2: f64, tol: f64` | `Result<(f64, f64), PLibError>` | `PLib::EvalLength` with `Tol` (-> `(length, error)`) |

* `niv_constr`: `C0 -> 0`, `C1 -> 1`, `C2 -> 2`, every other shape `Err(Construction)` (`Standard_ConstructionError("Invalid
  ConstraintOrder")`). `constraint_order`: `0, 1, 2` to `C0, C1, C2`, others `Err(Construction)`.
* `jacobi_parameters`: `Err(Construction)` when the shape is not `C0/C1/C2`, `max_degree < 2 * niv + 1`, `code` is not one of
  `-5..=-1, 1..=4`, or `work_degree` (= `max_degree + 9` for `code >= 1`, `max_degree + 6` otherwise) is `>= 61`. Otherwise
  `nb_gauss_points = max(IPMIN, IWANT)` where `IPMIN` is the smallest of `8, 10, 15, 20, 25, 30, 40, 50, 61` greater than
  `work_degree`, and `IWANT` is `8, 10, 15, 20, 25` for `code = -5..=-1` and `30, 40, 50, 61` for `code = 1..=4`.
* `eval_length`: the length `integral |P'(u)| du` of the polynomial curve with `dim` components over `[u1, u2]` by Gauss
  integration with `4 * min(degree / 4 + 1, 10)` points (`mathlin::gauss_points` / `gauss_weights`, summing the
  symmetric halves as in C++); `eval_length_tol` repeats it on `2, 4, 8, ...` equal sub-intervals until the change is `<= tol`
  or 13 iterations were done and returns `(length, error)` (`error` = the last `|OldLen - Length|`). `OutOfRange` for
  `degree < 0`, `dimension < 1` or `coeffs.len() < (degree + 1) * dimension`.

## `JacobiPolynomial` (`PLib_JacobiPolynomial`)

A plain struct (the OCCT class is not a `Standard_Transient` in 8.0.1) holding `work_degree`, `niv_constr` and `degree =
work_degree - 2 * (niv_constr + 1)`. `Clone, Debug`.

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `JacobiPolynomial::new` | `work_degree: i32, constraint_order: GeomAbsShape` | `Result<Self, PLibError>` | constructor |
| `work_degree` / `niv_constr` | `&self` | `i32` | `WorkDegree()` / `NivConstr()` |
| `points` | `&self, nb_gauss_points: i32` | `Result<Vec<f64>, PLibError>` | `Points` |
| `weights` | `&self, nb_gauss_points: i32` | `Result<Array2<f64>, PLibError>` | `Weights` |
| `max_value` | `&self, len: i32` | `Result<Vec<f64>, PLibError>` | `MaxValue` |
| `max_error` | `&self, dimension: i32, jac_coeff: &[f64], new_degree: i32` | `Result<f64, PLibError>` | `MaxError` |
| `reduce_degree` | `&self, dimension: i32, max_degree: i32, tol: f64, jac_coeff: &[f64]` | `Result<(i32, f64), PLibError>` | `ReduceDegree` (-> `(new_degree, max_error)`) |
| `average_error` | `&self, dimension: i32, jac_coeff: &[f64], new_degree: i32` | `Result<f64, PLibError>` | `AverageError` |
| `to_coefficients` | `&self, dimension: i32, degree: i32, jac_coeff: &[f64]` | `Result<Vec<f64>, PLibError>` | `ToCoefficients` |
| `d0` | `&self, u: f64` | `Vec<f64>` | `D0` |
| `d1` | `&self, u: f64` | `(Vec<f64>, Vec<f64>)` | `D1` |
| `d2` | `&self, u: f64` | `(Vec<f64>, Vec<f64>, Vec<f64>)` | `D2` |
| `d3` | `&self, u: f64` | `(Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>)` | `D3` |

* `new`: `Err(Construction)` when `constraint_order` is not `C0/C1/C2` (through `niv_constr`), or `degree < 0` ("WorkDegree
  too small for given ConstraintOrder"), or `degree > 30` ("Invalid Degree"). So `work_degree` is `2*(niv+1) ..= 30 + 2*(niv+1)`
  (`C0`: 2..=32, `C1`: 4..=34, `C2`: 6..=36).
* `points(n)`: the positive Gauss-Legendre points. `Err(Construction)` unless `n` is one of `8, 10, 15, 20, 25, 30, 40, 50, 61`
  and `n > degree` (note `degree`, not `work_degree`; `35`, mentioned in the C++ comment, is not accepted). Returns `n/2 + 1`
  values, the OCCT array `TabPoints(0..=n/2)`: element 0 is `0.0` for odd `n` and `-999.0` for even `n`; element `i` (`1..=n/2`)
  is the `i`-th smallest positive point, i.e. `mathlin::gauss_points(n, 1)` element `n/2 - i + 1`.
* `weights(n)`: `Array2` with `n/2 + 1` rows and `degree + 1` columns, the OCCT array `TabWeights(0..=n/2, 0..=degree)`:
  row `i >= 1` and column `j` hold the weight tabulated for point `i`, `j`-th Jacobi polynomial; row 0 holds the weights of
  the middle point for odd `n` (even columns from the second table, odd columns `0.0`), and `-999.0` everywhere for even `n`. All
  other entries are `0.0` if the tables do not fill them. The data are the tables of `PLib_JacobiPolynomial_Data.pxx`
  (`WeightsDB_C0/C1/C2`, `WeightsDB0_C0/C1/C2`) with the offsets computed in `Weights` (they must be translated exactly). `OutOfRange`
  unless `n` is one of the nine values above and `n > work_degree` (OCCT then reads the weights of another `n`).
* `max_value(len)`: the first `len` entries of `MaxValuesDB_C0/C1/C2` (lengths 57, 55, 53) for `niv_constr` 0, 1, 2: the OCCT array
  `TabMax(0..=len-1)`. `OutOfRange` unless `1 <= len <= 57 / 55 / 53`. (`max_error` and `reduce_degree` use `len = degree + 2`.)
* `max_error`: `jac_coeff` has `work_degree + 1` groups of `dimension` values (see *Jacobi coefficients* below).
  Returns `sqrt(sum_dim (sum_{k = max(2*(niv+1), new_degree+1) ..= work_degree} |c_k| * max_value[k - 2*(niv+1)])^2)` (with the sum
  order, the `mathlin::Vector` accumulation and `norm()` of C++). `OutOfRange` for `dimension < 1` or `jac_coeff.len() < (work_degree
  + 1) * dimension`.
* `reduce_degree`: `Ok((new_degree, max_error))`, the algorithm of C++ (search from `work_degree` down, then the fallback
  search for the last coefficient above `1e-9`). Same `OutOfRange` conditions as `max_error`.
* `average_error`: `sqrt(sum of c_k^2 / 2)` over `k = max(2*(niv+1)+1, new_degree+1) ..= degree` and all components
  (note: up to `degree`, not `work_degree`, as in C++). `OutOfRange` for `dimension < 1` or `jac_coeff.len() < (degree + 1) *
  dimension`.
* `to_coefficients(dimension, degree, jac_coeff)`: converts a polynomial with `degree + 1` groups from the Jacobi basis to the canonical
  basis with the transformation tables (`TransMatrix_C0/C1/C2` of the data file: triangular, `31 * 32 / 2 = 496` entries for the even and
  for the odd part); returns `(degree + 1) * dimension` values. It depends on `niv_constr` and `degree` only (not on
  `work_degree`). `OutOfRange` for `dimension < 1`, `degree < 0`, `degree > 61` (the tables end), or `jac_coeff.len() < (degree + 1) * dimension`.
* `d0(u)` .. `d3(u)`: the `degree + 1` values of the basis functions (and derivatives, `d1` returns `(values, first derivatives)`) at `u`, from the
  recurrence of C++ with the coefficient tables (`CofA`, `CofB`, `Denom`, `TNorm` of `PLib_JacobiPolynomial_Coeffs.pxx`, for `niv_constr` and
  `degree`). Any `u` is allowed. For `degree == 0` the basis is `[1.0 * TNorm[0]]`, for `degree >= 1` the first two functions are `1` and `(2*(niv+1) + 1) * u`
  (before normalisation), the rest by recurrence.

*Jacobi coefficients*: `jac_coeff` is `c_0 .. c_work_degree`, each a group of `dimension` values (see *Flat layouts*). The first
`2*(niv+1)` groups are the part `R(t)` in the canonical basis, the rest the coefficients of `Q(t)` in the Jacobi basis, as in the
class comment of `PLib_JacobiPolynomial.hxx`.

## `HermitJacobi` (`PLib_HermitJacobi`)

Holds a `JacobiPolynomial` (the same `work_degree`, constraint order). `Clone, Debug`. It has no `points`, `weights` or `max_value`.

| Rust | arguments | returns | C++ |
|---|---|---|---|
| `HermitJacobi::new` | `work_degree: i32, constraint_order: GeomAbsShape` | `Result<Self, PLibError>` | constructor (same errors as `JacobiPolynomial::new`) |
| `work_degree` / `niv_constr` | `&self` | `i32` | `WorkDegree()` / `NivConstr()` |
| `max_error` | `&self, dimension: i32, herm_jac_coeff: &[f64], new_degree: i32` | `Result<f64, PLibError>` | `MaxError` (the `JacobiPolynomial` one) |
| `reduce_degree` | `&self, dimension: i32, max_degree: i32, tol: f64, herm_jac_coeff: &[f64]` | `Result<(i32, f64), PLibError>` | `ReduceDegree` (the `JacobiPolynomial` one) |
| `average_error` | `&self, dimension: i32, herm_jac_coeff: &[f64], new_degree: i32` | `Result<f64, PLibError>` | `AverageError` (the `JacobiPolynomial` one) |
| `to_coefficients` | `&self, dimension: i32, degree: i32, herm_jac_coeff: &[f64]` | `Result<Vec<f64>, PLibError>` | `ToCoefficients` |
| `d0` .. `d3` | `&self, u: f64` | `Vec<f64>` / tuples of them like `JacobiPolynomial` | `D0 .. D3` |

* `to_coefficients`: with `h = 2 * niv_constr + 1` (`DegreeH`): the first `2*niv+2` groups are in the Hermite basis, converted with the
  matrix of `hermite_coefficients(-1.0, 1.0, niv, niv)` (computed once, `(2*niv+2)`-square); the rest are Jacobi coefficients. For `degree > h`
  the combined array is passed to `JacobiPolynomial::to_coefficients`, otherwise (`degree == h`) it is the result. Returns `(degree + 1) *
  dimension` values. `OutOfRange` when `degree < h` (OCCT writes beyond its auxiliary array), plus the conditions of `JacobiPolynomial::to_coefficients`
  (`herm_jac_coeff.len() < (degree + 1) * dimension`, `dimension < 1`, `degree > 61`).
* `d0(u)` .. `d3(u)` return `work_degree + 1` values per array: the first `h + 1` are the Hermite basis polynomials (rows of the same matrix,
  evaluated with `eval_polynomial` / `no_derivative_eval_polynomial`), the rest `W(t) * Jacobi_k(t)` with `W = (1 - t^2)^(niv+1)` and the derivative
  combinations (Leibniz) of C++. Any `u` is allowed.

## What the golden file contains

`tests/plib.txt`: 3140 lines in 54 families (one per Rust function / overload; see `FAMILIES` in `tests/plib.rs`), 1.40 MB. Every
family has at least 16 distinct lines (the smallest has 21); no line is repeated; no family is input-less. Every family is compared
bit for bit (`EXACT` lists all of them). The lines cover degrees 0, 1 and up to the maximum (Jacobi `degree` 0..=30, i.e. all `work_degree` the
constructors accept), derivative orders `0..=degree + 2`, parameters `0`, `+-1` and well outside `[-1, 1]`, dimensions `1..=8` and `14..=20` (the
run-time loop of `eval_polynomial` starts above 15), zero-length and reversed trimming intervals, weights down to `1e-6`, the constraint
orders `-1..=2` of `hermite_interpolate` (`-1` = no constraint at that end) and all seven `GeomAbs_Shape` values for the
constructors and conversions. The lines marked `throws` / `unchecked` are described above; the Rust API must return `Err` for all of them.
