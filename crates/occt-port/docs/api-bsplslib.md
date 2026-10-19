# `bsplslib` (and the BSplCLib functions it needs): Rust API contract

This contract covers all of OCCT 8.0.1's `BSplSLib` (`cpp/BSplSLib/`): the static functions of `BSplSLib`
(`BSplSLib.cxx`, `BSplSLib_BzSyntaxes.cxx`, `BSplSLib.lxx`), the class `BSplSLib_Cache`, the evaluator callback of
`FunctionMultiply` (`BSplSLib_EvaluatorFunction`), plus 18 `BSplCLib` functions (and four constants) that `BSplSLib` calls and that
`src/bsplclib.rs` does not have. Tests: `tests/bsplslib.rs` with the golden file `tests/bsplslib.txt` printed by
`oracle/bsplslib.cpp` (real OCCT 8.0.1). The new module is `occt_port::bsplslib` (add `pub mod bsplslib;` to
`src/lib.rs`); the `BSplCLib` additions go into the existing `occt_port::bsplclib`. Nothing that exists in `src/`
changes (`bsplclib::Curve`, `d0..dn`, `locate_parameter` stay; the private helper names `bohm`, `evaluate`,
`prepared`, `rational_derivative`, `p`, `v`, `r`, `w`, `eps` of `src/bsplclib.rs` are taken: the new public names
below avoid them). The package uses `occt_port::gp` (`Pnt`, `Vec`), `occt_port::plib` (`Array2`, `eval_polynomial`,
`no_derivative_eval_polynomial`, `bin`: PLib is complete and must be used, not copied) and `occt_port::mathlin`
(`Matrix`, for `eval_bspline_basis`).

## Conventions

* **Arrays and bounds.** Every OCCT array has lower bound 1 (rows and columns of 2D arrays, knots, multiplicities,
  the flat arrays of the BSplCLib functions). A slice is indexed from 0 and "element `k` of the OCCT array" is
  `slice[k - 1]`; **every index argument (`UIndex`, `Index`, `FromK1`, ...) keeps the OCCT, 1-based, meaning** (they
  depend on the lower bound: the translation must behave as if `Lower() == 1`). `NCollection_Array2<T>` is
  `plib::Array2<T>`, row-major, `rows()` = OCCT `ColLength()` (the U / first index), `cols()` = `RowLength()` (the V /
  second index). `NCollection_Array1<gp_Pnt>` is `Vec<Pnt>`, `NCollection_Array1<double>` `Vec<f64>`, `...<int>`
  `Vec<i32>`; `math_Matrix` is `mathlin::Matrix`.
* **Optional arrays.** `const NCollection_Array2<double>* Weights` (`NULL` = polynomial) is `Option<&Array2<f64>>`;
  `const NCollection_Array1<int>* Mults` (`BSplCLib::NoMults()`, i.e. `NULL`, means the knot array is the *flat*
  knot sequence) is `Option<&[i32]>`. Returned weights / multiplicities are `Option` exactly when the C++ fills
  them.
* **Out-parameters** become returned values in the C++ argument order. An array that the caller allocates for the
  result in C++ is returned as a freshly allocated `Vec` / `Array2` of the size OCCT's contract gives (stated
  per function); sizes are never inputs.
* **Scalars.** Reals are `f64`; every OCCT `int` is `i32` (indices, degrees, counts); bools are `bool`. Lengths
  of slices are `usize`.
* **Arithmetic.** Every family of the golden file is in the `EXACT` list of `tests/bsplslib.rs`: the results are
  compared bit for bit (signed zeros included). The operations must be done in the same order as in the C++ source,
  with no fused multiply-add and no reassociation (`BSplCLib::Eval`, for example, multiplies and then adds).
  `Epsilon(x)` of OCCT is the distance from `x` to the next representable `f64` away from zero (`std::nextafter`), not
  `f64::EPSILON * |x|`.
* **Errors.** `bsplclib::BSplError` (derive `Debug, Clone, PartialEq, Eq`; `impl From<MathError>` and
  `impl From<PLibError>`; `bsplslib` re-exports it):

  | variant | stands for |
  |---|---|
  | `Construction` | `Standard_ConstructionError` raised by OCCT (`function_multiply` when the function reports an error) |
  | `OutOfRange` | `Standard_OutOfRange` raised by OCCT (`bsplclib::interpolate` on a singular system), and every call that OCCT 8.0.1 *as built* does not check and that would fail (below) |
  | `Failure` | any other `Standard_Failure` |
  | `Argument` | wrong shape of a Rust argument (slices that are too short, sizes that do not match, ...) |
  | `Math(MathError)`, `PLib(PLibError)` | errors of the functions used inside |

  The test only checks `Err` against the oracle's `throws` / `unchecked`; any variant is accepted, but use the one
  of the table. **Not judged but required:** nothing may panic (the lints forbid indexing and `unwrap`): a slice that
  is shorter than the C++ reads, a negative count, a zero dimension that the golden file does not contain must
  return `Err`.

### Where the golden file says `throws` and where `unchecked`

* `throws` (the line was produced by a caught `Standard_Failure`): `BSplCLib::Interpolate` / `BSplSLib::Interpolate` on
  a singular or inconsistent system (`throw Standard_OutOfRange("BSplCLib::Interpolate")`, a real `throw`; the
  "inversion problem" output is therefore always 0 when the call returns), and `BSplSLib::FunctionMultiply` when the
  callback reports an error (`throw Standard_ConstructionError()`).
* `unchecked`: everything else that is invalid. `Standard_OutOfRange_Raise_if`, `Standard_RangeError`, the
  `NCollection_Array` and `math_Matrix` range checks are compiled out of the OCCT libraries, and the code has no
  explicit test. This is verified, for example `BSplSLib::D0` with degree 26 or 30 returns numbers (and overflows
  the `BSplSLib_DataContainer` on the stack). The oracle does not call such functions when they would read or
  write outside an array, dereference `NULL` or read uninitialised memory, and prints `unchecked`. The conditions
  are listed per function below ("**Err**"); a condition that is not listed has no golden line, but a Rust
  implementation that can detect an out-of-bounds access must return `Err` instead of panicking.
* A few inputs for which OCCT *runs* but violates its own contract are simply not in the golden file (an
  `InsertKnots` whose `PrepareInsertKnots` size is wrong for repeated knots when the multiplicities are not added;
  `MovePoint` when the displacement is spread over poles whose basis functions are all zero (division by zero)).
  The oracle also ran every call with two differently dirtied stacks and under valgrind: no golden line reads
  uninitialised memory or outside its arrays.

### What "valid" means for the structures

A B-spline direction is `(degree, periodic, knots, mults)`, with the numbers of poles given by
`BSplCLib::NbPoles`: non-periodic `sum(mults) - degree - 1`, periodic `sum(mults[..n-1])`. Degrees of a surface are
`1..=25` (`BSplCLib::MaxDegree()`); a degree outside that range is **Err** for every evaluation function
(`d0`, ..., `build_cache`, cache functions, `Cache::new`, ...). A direction with `Some(mults)` has `n` knots and `n`
multiplicities; with `None` the knots are the flat sequence (`BSplCLib::KnotSequence` of the same structure, which for a
periodic direction has the extra knots in front and behind). The pole array of a surface has
`rows = number of poles in U`, `cols = number of poles in V`; `weights`, if present, has the same shape. The
evaluation functions do not validate these relations beyond what they need to avoid reading outside arrays
(then **Err**).

## `Surface` and the evaluation functions

```rust
pub struct Surface<'a> {
    pub u_degree: i32, pub v_degree: i32,
    pub u_rational: bool, pub v_rational: bool,       // URat, VRat
    pub u_periodic: bool, pub v_periodic: bool,       // UPer, VPer
    pub poles: &'a Array2<Pnt>,
    pub weights: Option<&'a Array2<f64>>,
    pub u_knots: &'a [f64], pub v_knots: &'a [f64],   // flat knots when the mults are None
    pub u_mults: Option<&'a [i32]>, pub v_mults: Option<&'a [i32]>,
}
```
(`#[derive(Debug)]`; the struct fields are the C++ arguments `Poles, Weights, UKnots, VKnots, UMults, VMults,
UDegree, VDegree, URat, VRat, UPer, VPer`.) The index arguments `u_index`, `v_index` are the C++ `UIndex`, `VIndex`:
when the value lies inside `1..=knots.len()` it is used as the knot span index and `U` is *not* adjusted (no periodic
reduction); otherwise (typically 0) OCCT locates the parameter with `BSplCLib::LocateParameter`. The tests pass 0, the
located index, and other usable indices. All functions below return `Result<_, BSplError>`.

| Rust | C++ | result |
|---|---|---|
| `d0(u, v, u_index, v_index, &Surface)` | `D0` | `Pnt` |
| `d1(...)` | `D1` | `(P, Vu, Vv)` |
| `d2(...)` | `D2` | `(P, Vu, Vv, Vuu, Vvv, Vuv)` (the C++ order) |
| `d3(...)` | `D3` | `(P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)` |
| `dn(u, v, nu, nv, u_index, v_index, &Surface)` | `DN` | `Vec` (the `gp::Vec`) |
| `homogeneous_d0(...)` | `HomogeneousD0` | `(W, P)` (the C++ `(double& W, gp_Pnt& P)`) |
| `homogeneous_d1(...)` | `HomogeneousD1` | `(N, Nu, Nv, D, Du, Dv)` |

Common arguments `(u, v, u_index, v_index, surface)`. Families `d0 d1 d2 d3 dn homogeneous_d0 homogeneous_d1`
(prefixed `bsplslib_`); the inputs are `f f i i surf` (`dn`: `f f i i i i surf`, i.e. `u v nu nv u_index v_index`).
**Err:** a degree outside `1..=25`; `u_rational || v_rational` without weights (OCCT dereferences `NULL`); `dn`
with `nu < 0 || nv < 0`. Notes: a surface that is rational by flag but whose weights are locally equal is evaluated
as polynomial (`PrepareEval`); `d2`/`d3`/`dn` return zero vectors for derivative orders above the degree of a
polynomial surface; weights present with both flags false are ignored. The word `surf` of the golden file carries
`udeg vdeg urat vrat uper vper poles weights uknots vknots umults vmults`.

* `rational_derivative(u_degree, v_degree, n, m, ders: &[f64], all: bool) -> Result<Vec<f64>>` = `RationalDerivative`
  (family `rational_derivative`, inputs `i i i i fs i`). `ders` holds the homogeneous derivatives, `(u_degree + 1)`
  lines of `(v_degree + 1) * 4` values; only the lines/columns up to `min(n, u_degree)` and `min(m, v_degree)` are
  read. Result: `all` -> `3 * (n + 1) * (m + 1)` values; otherwise the 3 values of the derivative `(n, m)`.
  **Err:** `n < 0`, `m < 0`, a degree `< 0`, `ders` shorter than `min(n,u_degree) * (v_degree + 1) * 4 +
  min(m,v_degree) * 4 + 4`, or `ders[3] == 0`.
* `iso(param, is_u, poles, weights, knots, mults, degree, periodic, want_weights) -> Result<(Vec<Pnt>, Option<Vec<f64>>)>`
  = `Iso` (family `iso`, inputs `f i p3a2 oarr2 fs ois i i i`). `knots`/`mults` belong to the direction of `param`
  (U for `is_u`, which then walks along the rows). Returns one pole per element of the *other* direction (`cols` poles
  when `is_u`, `rows` when not) and the weights `Some` iff `want_weights` (the C++ `CWeights != NULL`; non-rational input
  gives all ones). The input is rational iff `weights` is `Some` (no local test). **Err:** degree < 1; `weights`
  is `Some` and `!want_weights` (OCCT dereferences `NULL`).
* `reverse_poles(poles, last, u_direction) -> Result<Array2<Pnt>>` and `reverse_weights(weights, last, u_direction) ->
  Result<Array2<f64>>` = the two `Reverse` overloads (families `reverse_poles` / `reverse_weights`, inputs `p3a2 i i`
  / `arr2 i i`). **Err:** `1 + (last - 1) % len < 1` with C++ `%` (`len` = rows for `u_direction`, else columns).
* `is_rational(weights, i1, i2, j1, j2, epsilon) -> Result<bool>` = `IsRational` (family `is_rational`, inputs
  `arr2 i i i i f`), reproducing its odd indexing (it compares neighbours along the first index only and uses
  `Epsilon(Weights(I1, I2))` when `epsilon <= 0`). **Err:** `epsilon <= 0` and `(i1, i2)` outside the array; or the
  loops run (`i1 < i2 && j1 < j2`) and `i1 < 1 || j1 < 1`.
* `set_poles(poles, u_direction) -> Result<Vec<f64>>` (`SetPoles` without weights: `3 * rows * cols` values),
  `set_poles_weighted(poles, weights, u_direction) -> Result<Vec<f64>>` (`4 * rows * cols`, homogeneous),
  `get_poles(fp, rows, cols, u_direction) -> Result<Array2<Pnt>>` (`GetPoles`), `get_poles_weighted(fp, rows, cols,
  u_direction) -> Result<(Array2<Pnt>, Array2<f64>)>` (divides by the weight `FP(l + 3)`). `u_direction` selects row-major
  (`true`) or column-major (`false`) order of the flat array. Families `set_poles`, `set_poles_weighted` (inputs
  `p3a2 i`, `p3a2 arr2 i`), `get_poles`, `get_poles_weighted` (inputs `i i fs i`). **Err:** weights smaller than the poles;
  `rows < 1 || cols < 1`; `fp` shorter than `rows * cols * 3` (`* 4`); a zero weight in `get_poles_weighted`.

## Knot operations

`struct KnotResult { pub poles: Array2<Pnt>, pub weights: Option<Array2<f64>>, pub knots: Vec<f64>, pub mults: Vec<i32> }`
(`Debug`). In all four functions the surface is processed as a curve whose poles are whole rows (`u_direction`) or
columns of the pole array (`SetPoles` / `GetPoles` / the BSplCLib function / `GetPoles`); `weights` is `Some` iff the
input is rational and the result then carries weights. The new arrays have the sizes the BSplCLib contract gives, and
the other direction keeps its size.

* `insert_knots(u_direction, degree, periodic, poles, weights, knots, mults, add_knots, add_mults: Option<&[i32]>,
  epsilon, add) -> Result<KnotResult>` = `InsertKnots` (family `insert_knots`, inputs `i i i p3a2 oarr2 fs is fs ois f
  i`). New pole count: `bsplclib::prepare_insert_knots`. **Err:** invalid structure, `add_knots` empty, `add_mults`
  of another length, poles not matching the multiplicities, `prepare_insert_knots` says `None`.
* `remove_knot(u_direction, index, mult, degree, periodic, poles, weights, knots, mults, tolerance) ->
  Result<Option<KnotResult>>` = `RemoveKnot` (family `remove_knot`, inputs `i i i i i p3a2 oarr2 fs is f`). `None` is the C++
  `false` (index outside the removable range, or the knot cannot be removed within the tolerance). New sizes: knots
  `n` (`mult > 0`) or `n - 1`; pole count minus `mults[index] - mult` (for a periodic surface and `index == 1` the
  last knot's multiplicity). **Err:** structure/poles invalid; for an index in range: `mult < 0` or
  `mult >= mults[index]`.
* `increase_degree(u_direction, degree, new_degree, periodic, poles, weights, knots, mults) -> Result<KnotResult>` =
  `IncreaseDegree` (family `increase_degree`, inputs `i i i i p3a2 oarr2 fs is`). Knot count:
  `bsplclib::increase_degree_count_knots`; pole count: `BSplCLib::NbPoles(new_degree, periodic, new_mults)` (the
  translation may compute it with a first pass). **Err:** structure/poles invalid, `new_degree < degree`,
  `new_degree > 25`, non-periodic with no span.
* `unperiodize(u_direction, degree, mults, knots, poles, weights) -> Result<KnotResult>` = `Unperiodize` (family
  `unperiodize`, inputs `i i is fs p3a2 oarr2`). Sizes: `bsplclib::prepare_unperiodize`. **Err:** structure/poles invalid
  (the structure must be periodic-valid).

## Caches (Taylor expansions of a span)

* `build_cache(u, v, u_span_domain, v_span_domain, u_periodic, v_periodic, u_degree, v_degree, u_index, v_index,
  u_flat_knots, v_flat_knots, poles, weights) -> Result<(Array2<Pnt>, Option<Array2<f64>>)>` = the `BuildCache`
  overload with `CachePoles` / `CacheWeights` (family `build_cache`, inputs `f f f f i i i i i i fs fs p3a2 oarr2`).
  Result arrays have `max(u_degree, v_degree) + 1` rows and `min(...) + 1` columns; the weights are `Some` iff the
  input weights are (a surface that is locally polynomial gets weights `1, 0, 0, ...`).
* `build_cache_array(...same arguments...) -> Result<Array2<f64>>` = the overload with `theCacheArray` (family
  `build_cache_array`, same inputs). The array has `max + 1` rows and `(min + 1) * 3` columns (`* 4` with weights);
  each pole is `x y z [w]`. **Err** for both: a degree outside `1..=25`.
* `cache_d0(u, v, u_degree, v_degree, u_cache_parameter, v_cache_parameter, u_span_length, v_span_length, poles,
  weights) -> Result<Pnt>`, `cache_d1(...) -> Result<(Pnt, Vec, Vec)>` (`P, VecU, VecV`), `cache_d2(...) ->
  Result<(Pnt, Vec, Vec, Vec, Vec, Vec)>` (`P, VecU, VecV, VecUU, VecUV, VecVV`, the C++ order of `CacheD2`).
  Families `cache_d0 cache_d1 cache_d2`, inputs `f f i i f f f f p3a2 oarr2`. `poles` (and `weights`) are cache coefficient
  arrays: `max(u_degree, v_degree) + 1` rows by `min(...) + 1` columns. **Err:** a degree outside `1..=25`,
  `poles.rows() * poles.cols()` (or the weights') different from that product, a zero span length.
* `coefs_d0(u, v, poles, weights)`, `coefs_d1`, `coefs_d2` = `CoefsD0/1/2` (`CacheD*` with `u_degree = cols - 1`,
  `v_degree = rows - 1`, parameters `0, 0` and span lengths `1, 1`; same result types as `cache_d*`). Families `coefs_d0
  coefs_d1 coefs_d2`, inputs `f f p3a2 oarr2`. **Err:** `rows < 2 || cols < 2`, weights of another shape.
* `poles_coefficients(poles, weights) -> Result<(Array2<Pnt>, Option<Array2<f64>>)>` = the `PolesCoefficients`
  overload with weights (the `BSplSLib_BzSyntaxes.cxx` one), `poles_coefficients_nw(poles) -> Result<Array2<Pnt>>` = the
  inline overload without weights (families `poles_coefficients`, `poles_coefficients_nw`, inputs `p3a2 oarr2` /
  `p3a2`). Result arrays have the shape of `poles`. **Err:** `rows < 2 || cols < 2`, weights of another shape.

### `Cache` (`BSplSLib_Cache`)

```rust
pub struct Cache { /* private */ }   // Debug
impl Cache {
    pub fn new(u_degree: i32, u_periodic: bool, u_flat_knots: &[f64], v_degree: i32, v_periodic: bool,
               v_flat_knots: &[f64], weights: Option<&Array2<f64>>) -> Result<Cache, BSplError>;
    pub fn is_cache_valid(&self, u: f64, v: f64) -> bool;
    pub fn build_cache(&mut self, u: f64, v: f64, u_flat_knots: &[f64], v_flat_knots: &[f64],
                       poles: &Array2<Pnt>, weights: Option<&Array2<f64>>) -> Result<(), BSplError>;
    pub fn d0(&self, u: f64, v: f64) -> Result<Pnt, BSplError>;
    pub fn d1(&self, u: f64, v: f64) -> Result<(Pnt, Vec, Vec), BSplError>;
    pub fn d2(&self, u: f64, v: f64) -> Result<(Pnt, Vec, Vec, Vec, Vec, Vec), BSplError>; // P, TangentU, TangentV, CurvatureU, CurvatureV, CurvatureUV
    pub fn d0_local(&self, local_u: f64, local_v: f64) -> Result<Pnt, BSplError>;
    pub fn d1_local(&self, ...) ; pub fn d2_local(&self, ...);                              // as d1 / d2
}
```
`new` = the constructor (rational iff `weights` is `Some`; the weights themselves are not stored). **Err:** a degree
outside `1..=25`. `is_cache_valid` on a cache that was never built is defined (the fresh `SpanStart = SpanLength = SpanIndex
= 0`) and is in the golden file. The evaluation functions on a cache that was never built read uninitialised
memory in OCCT: **Err** (golden `unchecked`). Families `cache_class_valid`, `cache_class_d0`, `cache_class_d1`,
`cache_class_d2`, `cache_class_d0_local`, `cache_class_d1_local`, `cache_class_d2_local`; inputs `surf i f f f f` = the
surface (flat knots, `uknots`/`vknots` are the flat knot arrays, mults `None`; only degrees, periodic flags, poles, weights and
knots are used), a flag "build the cache first" (with `build_cache(u0, v0, ...)`), then `u0 v0 u v`; for the
`*_local` families `u v` are the local parameters.

## `resolution`, `interpolate`, `function_multiply`, `unit_weights`, `move_point`

* `resolution(&Surface, tolerance_3d) -> Result<(f64, f64)>` = `Resolution` (`(UTolerance, VTolerance)`; zeros when
  the estimated derivative bound is zero). Family `resolution`, inputs `surf f`. The multiplicities must be `Some`
  (`Resolution` takes non-optional multiplicities; flat knots are **Err**). **Err:** degree outside `1..=25`;
  rational flag without weights.
* `interpolate(u_degree, v_degree, u_flat_knots, v_flat_knots, u_parameters, v_parameters, poles) -> Result<(i32,
  Array2<Pnt>)>` and `interpolate_weighted(..., poles, weights) -> Result<(i32, Array2<Pnt>, Array2<f64>)>` = the two
  `Interpolate` overloads (families `interpolate`, `interpolate_weighted`, inputs `i i fs fs fs fs p3a2 [arr2]`). The
  input arrays hold the data points (and weights) to interpolate; the result holds the interpolating poles (and
  weights); the `i32` is the "inversion problem" (always 0, see `throws` above). **Err/throws:** a singular system
  (`throws`); `Err` (`unchecked`): degree < 1, empty parameters, poles not `u_parameters.len()` by
  `v_parameters.len()`, flat knots not `parameters.len() + degree + 1` long, or a parameter whose basis functions
  start so far from its row that `BuildBSpMatrix` would write outside the band matrix.
* `function_multiply(function: &dyn Fn(i32, f64, f64) -> (f64, i32), u_degree, v_degree, u_knots, v_knots, u_mults:
  Option<&[i32]>, v_mults, poles, weights: Option<&Array2<f64>>, u_flat_knots, v_flat_knots, u_new_degree,
  v_new_degree) -> Result<(Array2<Pnt>, Array2<f64>, i32)>` = `FunctionMultiply`, returning `(NewNumerator,
  NewDenominator, theStatus)` with `u_flat_knots.len() - u_new_degree - 1` rows and `v_flat_knots.len() -
  v_new_degree - 1` columns (the C++ `ConstructionError` for arrays of other sizes cannot happen: sizes are not inputs).
  `function(derivative_request, u, v)` = `Evaluate` returning `(result, error_code)`; the derivative request is
  always 0. A non-zero error code is `Err(Construction)` (`throws`). Family `function_multiply`, inputs `evf i i fs fs ois
  ois p3a2 oarr2 fs fs i i` (`evf`: `kind c0 c1 c2 c3`, see the test). **Err:** no weights (OCCT dereferences `NULL`),
  a degree < 1, empty result, the interpolation matrix unusable as in `interpolate`.
* `unit_weights(nb_u_poles, nb_v_poles) -> Result<Array2<f64>>` = `UnitWeights` (family `unit_weights`, inputs `i i`; the
  test prints the shape and `first, last, sum, min, max`; `nb_u_poles * nb_v_poles` around
  `bsplclib::max_unit_weights_size()` = 2049 are covered). **Err:** a size < 1.
* `move_point(u, v, displ: &Vec, u_index1, u_index2, v_index1, v_index2, u_degree, v_degree, rational, poles, weights:
  &Array2<f64>, u_flat_knots, v_flat_knots) -> Result<MovedPoint>` = `MovePoint`, with
  `struct MovedPoint { pub u_first: i32, pub u_last: i32, pub v_first: i32, pub v_last: i32, pub poles: Option<Array2<Pnt>> }`
  (`Debug`); `poles` is `None` (and the four indices 0) iff `EvalBsplineBasis` reports an error, the C++ "no change"
  case. Family `move_point`, inputs `f f vec i i i i i i i p3a2 arr2 fs fs`. `weights` is read only if `rational`.
  **Err:** a degree outside `1..=25`; flat knots not `poles.rows() + u_degree + 1` (resp. `cols`) long; `rational`
  with weights of another shape than `poles`.

## BSplCLib functions needed by BSplSLib (`occt_port::bsplclib`, families `bsplclib_*`)

All return `Result<_, BSplError>`. Raw buffers (`Eval`, `Bohm`, `Interpolate`) are in/out in C++; the Rust function takes the
input buffer by reference and returns the whole modified buffer (same length), because callers use the other entries
after the call.

| Rust | C++ (`BSplCLib::`) | inputs of the golden family | result |
|---|---|---|---|
| `eval_buffer(u, degree, knots, dimension, poles)` | `Eval(U, Degree, Knots&, Dimension, Poles&)` | `f i fs i fs` | `Vec<f64>`, the buffer (`(degree + 1) * dimension` values) |
| `bohm_buffer(u, degree, n, knots, dimension, poles)` | `Bohm(U, Degree, N, Knots&, Dimension, Poles&)` | `f i i fs i fs` | the buffer |
| `build_knots(degree, index, periodic, knots, mults: Option<&[i32]>)` | `BuildKnots` | `i i i fs ois` | `Vec<f64>` of `2 * degree` local knots |
| `pole_index(degree, index, periodic, mults)` | `PoleIndex` | `i i i is` | `i32` |
| `locate_parameter_flat(degree, knots, u, periodic, knot_index)` | `LocateParameter(Degree, Knots, NULL mults, U, Periodic, KnotIndex&, NewU&)` | `i fs f i i` | `(index, new_u)` |
| `locate_parameter_range(degree, knots, u, periodic, from_k1, to_k2)` | `LocateParameter(Degree, Knots, U, IsPeriodic, FromK1, ToK2, KnotIndex&, NewU&)` | `i fs f i i i` | `(index, new_u)` |
| `knot_sequence_length(mults, degree, periodic)` | `KnotSequenceLength` | `is i i` | `i32` |
| `knot_sequence(knots, mults, degree, periodic)` | `KnotSequence(Knots, Mults, Degree, Periodic, KnotSeq&)` | `fs is i i` | `Vec<f64>` of `knot_sequence_length` values |
| `eval_bspline_basis(derivative_request, order, flat_knots, parameter, is_periodic, basis: &mut Matrix)` | `EvalBsplineBasis(..., isPeriodic)` | `i i fs f i i i i i` | `(error_code, first_nonzero_bspline_index)` |
| `build_schoenberg_points(degree, flat_knots, nb_parameters: i32)` | `BuildSchoenbergPoints` | `i fs i` | `Vec<f64>` |
| `interpolate(degree, flat_knots, parameters, contact_order, dimension, poles)` | the 7-argument `Interpolate` | `i fs fs is i fs` | `(inversion_problem, Vec<f64>)`, the poles buffer (`parameters.len() * dimension`) |
| `prepare_insert_knots(degree, periodic, knots, mults, add_knots, add_mults, tolerance, add)` | `PrepareInsertKnots` | `i i fs is fs ois f i` | `Option<(nb_poles, nb_knots)>`, `None` = the C++ `false` |
| `insert_knots(degree, periodic, dimension, poles, knots, mults, add_knots, add_mults, tolerance, add)` | `InsertKnots` (flat poles) | `i i i fs fs is fs ois f i` | `(new_poles, new_knots, new_mults)` |
| `remove_knot(index, mult, degree, periodic, dimension, poles, knots, mults, tolerance)` | `RemoveKnot` | `i i i i i fs fs is f` | `Option<(new_poles, new_knots, new_mults)>` |
| `increase_degree_count_knots(degree, new_degree, periodic, mults)` | `IncreaseDegreeCountKnots` | `i i i is` | `i32` |
| `increase_degree(degree, new_degree, periodic, dimension, poles, knots, mults)` | `IncreaseDegree` | `i i i i fs fs is` | `(new_poles, new_knots, new_mults)` |
| `prepare_unperiodize(degree, mults)` | `PrepareUnperiodize` | `i is` | `(nb_knots, nb_poles)` |
| `unperiodize(degree, dimension, mults, knots, poles)` | `Unperiodize` | `i i is fs fs` | `(new_mults, new_knots, new_poles)` |
| `max_degree()`, `max_unit_weights_size()`, `unit_weights_data()`, `no_mults()` | `MaxDegree`, `MaxUnitWeightsSize`, `UnitWeightsData`, `NoMults` | none (input-less family `constants`) | `25`, `2049`, `&'static [f64]` of 2049 ones, `None::<&[i32]>` |

Poles of the flat functions are `dimension` values per pole, pole after pole. The families are named
`bsplclib_<rust name>` (`bsplclib_constants` for the last row). Notes and **Err** conditions:

* `eval_buffer`: **Err** if `degree < 0`, `dimension < 1`, `knots.len() < 2 * degree` (or empty), `poles.len() < (degree +
  1) * dimension`. Degree 0 is a no-op. The golden file has dimensions 1..9, 12, 16 and 20 (the C++ switch has unrolled
  branches for 1..4).
* `bohm_buffer`: the same, plus `n < 0`. **Note** C++ `Bohm` with `Dimension == 1` divides by the knot difference where the other
  dimensions multiply by its reciprocal; equal knots give a zero coefficient. The golden file has dimensions
  1..9, 12, 16 and 20.
* `build_knots`: with `mults == None`, **Err** unless `degree <= index` and `index + degree <= knots.len()`; with mults,
  **Err** unless `1 <= index <= n` (`n - 1` for a non-periodic structure) and `mults.len() == knots.len()`; also **Err**
  (golden `unchecked`) when the C++ would leave one of the `2 * degree` knots unwritten (an index too close to an end
  of a non-periodic structure). `degree < 1` is **Err**.
* `pole_index`: **Err** if `index > mults.len()` or `mults` is empty.
* `locate_parameter_flat`: `knot_index` is the in/out `KnotIndex` on entry: when it lies in
  `[1 + degree, knots.len() - degree]` the C++ returns it unchanged with `new_u = u`; otherwise the parameter is located
  (`first = 1 + degree`, `last = knots.len() - degree`, periodic period `[knots[first], knots[last]]`). **Err:**
  `degree < 1` or `knots.len() < 2 * degree + 2`.
* `locate_parameter_range`: **Err:** `degree < 0`, `knots.len() < degree + 1`, `from_k1` or `to_k2` outside `1..=n`, or
  `from_k1 == to_k2`. A periodic structure uses `knots[1 + degree]` and `knots[n - degree]` as the period.
* `knot_sequence_length`: **Err** on empty `mults`. `knot_sequence`: **Err** unless `knots.len() == mults.len() > 0`,
  `degree >= 1`, and for a periodic structure `mults[0] == mults[last]` and `degree + 1 - mults[0] >= 0`.
* `eval_bspline_basis`: the basis matrix `basis` (a `mathlin::Matrix`, any bounds) is in/out; on return its entries
  are as OCCT leaves them (entries it does not touch keep their value, also after error code 2); error code 1: bounds
  that do not fit (`LowerCol != 1 || UpperCol < order || LowerRow != 1 || UpperRow <= min(derivative_request, order -
  1)`), code 2: a knot interval shorter than `gp::resolution()`. `first_nonzero_bspline_index` is 0 for code 1.
  **Err:** `order < 1`, `derivative_request < 0`, `flat_knots.len() < 2 * order`, an invalid matrix.
* `build_schoenberg_points`: **Err** if `degree < 1`, `nb_parameters < 1` or `flat_knots.len() < degree +
  nb_parameters`.
* `interpolate`: `throws` (the C++ `Standard_OutOfRange`) when `BuildBSpMatrix` / `FactorBandedMatrix` /
  `SolveBandedSystem` report an error (a pivot below `RealSmall()`, a knot interval shorter than the resolution);
  **Err** (`unchecked`) when `degree < 1`, `dimension < 1`, `contact_order.len() != parameters.len()`, a contact
  order outside `0..=degree`, `poles` shorter than `parameters.len() * dimension`, `flat_knots.len() != parameters.len() +
  degree + 1`, or a row whose first non-zero basis function is so far from the diagonal that the band is exceeded.
  The helpers `BuildBSpMatrix`, `FactorBandedMatrix`, `SolveBandedSystem` are checked through this family.
* `prepare_insert_knots`: **Err** for an invalid structure (see above and: strictly increasing knots, multiplicities
  `1..=degree + 1`), empty `add_knots`, `add_mults` of another length.
* `insert_knots`: **Err** for the same, a pole array that is not `nb_poles * dimension` long, or when
  `prepare_insert_knots` is `None`. The result sizes are the ones `prepare_insert_knots` returns.
* `remove_knot`, `increase_degree`, `unperiodize`: see the surface-level descriptions (same sizes, same **Err**).
  `increase_degree_count_knots`: **Err** for `degree < 1`, `new_degree < degree`, empty mults, an invalid non-periodic
  structure. `prepare_unperiodize`: **Err** unless the multiplicities are those of a valid periodic structure
  (`1..=degree`, first equal to last, at least two knots). The golden file for `increase_degree` contains periodic
  structures for which OCCT leaves zero poles at the start of the result: the translation must reproduce them.
* **Internal functions** (not judged on their own, needed by the above): `FirstUKnotIndex`, `LastUKnotIndex`, `Hunt`,
  `NbPoles`, `BuildBoor`, `BoorScheme`, `AntiBoorScheme`, `BoorIndex`, `GetPole`, `Copy` (static of `BSplCLib.cxx`),
  `BuildBSpMatrix`, `FactorBandedMatrix`, `SolveBandedSystem`, `ElCLib::InPeriod`.
  Only these `BSplCLib` overloads are used by `BSplSLib`; the rest of `BSplCLib` (trimming, reparametrization, the
  other `Eval`, `MovePointAndTangent`, `Resolution`, `Intervals`, ...) is not part of this job.

## Overloads and naming

C++ overloads got their own Rust names: `Reverse` -> `reverse_poles` / `reverse_weights`; `SetPoles` / `GetPoles` ->
`set_poles`, `set_poles_weighted`, `get_poles`, `get_poles_weighted`; `BuildCache` -> `build_cache` (with
`CachePoles` / `CacheWeights`) / `build_cache_array`; `PolesCoefficients` -> `poles_coefficients` / `poles_coefficients_nw`;
`Interpolate` -> `interpolate` / `interpolate_weighted`; `BSplSLib_Cache::D0 / D1 / D2` and `D0Local / D1Local / D2Local`
are methods of `Cache`, `BSplSLib::CacheD0..D2` are the free functions `cache_d0..cache_d2`; `BSplCLib::Eval` and `Bohm`
are `eval_buffer`, `bohm_buffer`; the three `LocateParameter` overloads are `locate_parameter` (existing, with
mults), `locate_parameter_flat`, `locate_parameter_range`; `Interpolate` of BSplCLib has the 7-argument form only;
`KnotSequence` the 5-argument form only (the 4-argument form is `degree = 0` of it and is not called by BSplSLib).
