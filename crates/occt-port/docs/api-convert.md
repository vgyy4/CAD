# `convert`: Rust API contract

This contract covers all of OCCT 8.0.1's package `Convert` (`cpp/Convert/`, `TKMath`):

* the conic converters `Convert_ConicToBSplineCurve` with `Convert_CircleToBSplineCurve`, `Convert_EllipseToBSplineCurve`,
  `Convert_HyperbolaToBSplineCurve` and `Convert_ParabolaToBSplineCurve` (full and trimmed constructors, all eight
  `Convert_ParameterisationType`s), both overloads of `BuildCosAndSin` and `BuildPolynomialCosAndSin`
  (`Convert_PolynomialCosAndSin`), `Convert_CosAndSinEvalFunction` (an internal callback type: no Rust API);
* the elementary-surface converters `Convert_ElementarySurfaceToBSplineSurface` with `Convert_CylinderToBSplineSurface`,
  `Convert_ConeToBSplineSurface`, `Convert_SphereToBSplineSurface` and `Convert_TorusToBSplineSurface` (every constructor);
* `Convert_CompBezierCurvesToBSplineCurve` (and its template base `Convert_CompBezierCurvesToBSplineCurveBase`) and
  `Convert_CompBezierCurves2dToBSplineCurve2d`;
* `Convert_CompPolynomialToPoles` (three constructors) and `Convert_GridPolynomialToPoles` (two constructors).

The package has no `Convert_BSplineCurveToBezierCurve`-style helper. The module is `occt_port::convert` (add `pub mod convert;` to
`src/lib.rs`; `src/` may use private submodules). Tests: `tests/convert.rs` with the golden file `tests/convert.txt` printed by
`oracle/convert.cpp` (real OCCT 8.0.1). Nothing that exists in `src/` changes. The module reuses `gp` (2D: `Circ2d`, `Elips2d`,
`Hypr2d`, `Parab2d`, `Ax2d`, `Ax22d`, `Trsf2d`, `Pnt2d`, `Dir2d`; 3D: `Cylinder`, `Cone`, `Sphere`, `Torus`, `Ax3`, `Trsf`, `Pnt`),
`plib` (`Array2`, `no_derivative_eval_polynomial`, `eval_poly_2var`, the polynomial and Jacobi helpers it has), `bsplclib`
(`d0`, `interpolate`, `knot_sequence`, `build_schoenberg_points`, `trimming`, `increase_degree`: whatever `BSplCLib` function the
C++ calls; add the missing ones to `bsplclib`, never copy them) and `bsplslib` (`interpolate`): they are complete and must be used,
not copied.

## Conventions

* **Arrays.** Every OCCT array here has lower bound 1; `NCollection_Array1<T>` is `Vec<T>` / `&[T]`, `NCollection_Array2<T>` is
  `plib::Array2<T>` (row-major, `rows()` = OCCT `ColLength()` = the first index, `cols()` = `RowLength()`). The golden file
  prints every output array as `lower upper values...`, always `1 len ...` for the Rust slices. An input slice is indexed from 0:
  "element `k` of the OCCT array" is `slice[k - 1]`. Where OCCT takes an `NCollection_Array2<double>` of two columns
  (`PolynomialIntervals`) or an `NCollection_Array2<int>` of two columns (`NumCoeffPerSurface`) the Rust argument is a slice of
  rows, `&[[f64; 2]]` / `&[[i32; 2]]` (the C++ test `RowLength() != 2` therefore does not exist). The handle-array constructors
  of OCCT (`occ::handle<NCollection_HArray1<..>>`) and the `Poles(occ::handle<..>&)`-style accessors are the same operations on
  the same data and have no separate Rust API; null handles do not exist.
* **Scalars.** Reals are `f64`, OCCT `int` is `i32` (degrees, counts, indices, multiplicities), bools are `bool`.
* **Errors.** `convert::ConvertError` (derive `Debug, Clone, PartialEq, Eq`; `impl From<MathError>`, `impl From<gp::ConstructionError>`,
  `impl From<PLibError>`, `impl From<BSplError>`):

  | variant | stands for |
  |---|---|
  | `Construction` | `Standard_ConstructionError` raised by OCCT (`BuildCosAndSin`, `CompPolynomialToPoles` constructors) |
  | `Domain` | `Standard_DomainError` raised by OCCT (`CircleToBSplineCurve` trimmed, `GridPolynomialToPoles`) |
  | `OutOfRange` | `Standard_OutOfRange` (conic `pole/weight/knot/multiplicity`, a singular interpolation) |
  | `NotDone` | `StdFail_NotDone` (the accessors of a `GridPolynomialToPoles` that is not done) |
  | `Argument` | an input that OCCT 8.0.1 as built does not check and that is unspecified there (below), wrong shapes of Rust slices |
  | `Gp(..)`, `Math(..)`, `PLib(..)`, `BSpl(..)` | errors of the functions used inside |

  The test only checks `Err` against the oracle's `throws` / `unchecked`; any variant is accepted. **Not judged but required:**
  nothing may panic (the lints forbid indexing and `unwrap`): a slice that is shorter than the C++ reads, a negative count, a zero
  dimension must return `Err`.
* **Arithmetic.** Operations are done in the order of the C++ source, with no fused multiply-add and no reassociation. `sin`, `cos`,
  `tan`, `sinh`, `cosh`, `atan2` are `libm::`. The families `parabola`, `cpoly_*` and `gpoly_*` only use `+ - * / sqrt`
  and are compared bit for bit (`EXACT` in `tests/convert.rs`, signed zeros included); all others depend on the C library and are
  compared within 8 ulps of the largest absolute value of the line (at least 1; zero tolerance if that value is infinite). The
  oracle keeps only cases whose outputs stay within 2 ulps when every C-library result is moved by an ulp in 24 patterns.
  `Epsilon(x)` of OCCT is the distance from `x` to the next representable `f64` away from zero (`std::nextafter`), `Epsilon(0.0)`
  is the smallest positive subnormal. `Precision::PConfusion()` is `1e-7 / 100`.
* **`throws` and `unchecked`.** `throws` lines are exceptions OCCT really raises (explicit `throw` statements, and
  `Standard_Failure` of the `gp` constructors, `BSplCLib::Interpolate`). **`unchecked`** lines are inputs for which OCCT 8.0.1 *as
  built* has the check compiled out (`Standard_DomainError_Raise_if`, `Standard_OutOfRange_Raise_if`, `StdFail_NotDone_Raise_if`,
  the `NCollection` range checks are all compiled out: this was verified with probes) or that read or write outside an array;
  the oracle does not call OCCT for them and the Rust API must return `Err` for exactly these inputs (the conditions are listed
  per class). A shape of input that is unspecified and not in the golden file may return `Err` too.
* **Text tokens.** No family of this package prints a text, so the golden file has no text token. (The test still carries the
  `unchecked` marker of a not-done `GridPolynomialToPoles` as a text token; see `gpoly_*`.)

## Parametrisation type

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum ParameterisationType {
    TgtThetaOver2, TgtThetaOver2_1, TgtThetaOver2_2, TgtThetaOver2_3, TgtThetaOver2_4, QuasiAngular, RationalC1, Polynomial,
}
```

in the order of the C++ enum (the golden file prints their index 0..7).

## Conics: `ConicToBSplineCurve`

One concrete type holds the result of every conic converter (the C++ classes only differ by their constructors):

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ConicToBSplineCurve { /* degree, periodic, poles, weights, knots, mults */ }
```

| constructor | C++ | returns `Err` (`throws`) when | `Err` (`unchecked`) when |
|---|---|---|---|
| `circle(c: &Circ2d, p: ParameterisationType) -> Result<Self, ConvertError>` | `Convert_CircleToBSplineCurve(C, Parameterisation)` | `p` is `TgtThetaOver2_1` or `TgtThetaOver2_2` (the full circle does not fit in 1 or 2 spans: `Standard_ConstructionError` of `BuildCosAndSin`) | never |
| `circle_trimmed(c, u_first, u_last, p)` | `Convert_CircleToBSplineCurve(C, UFirst, ULast, Parameterisation)` | `delta = u_last - u_first` with `delta > 2*PI + PConfusion` or `delta <= 0` (`Standard_DomainError`, an explicit `throw`); `TgtThetaOver2_1` with `delta > 0.9999*PI`, `TgtThetaOver2_2` with `delta > 1.9999*PI` | never |
| `ellipse(e: &Elips2d, p)` | `Convert_EllipseToBSplineCurve(E, Parameterisation)` | as `circle` | never |
| `ellipse_trimmed(e, u_first, u_last, p)` | `Convert_EllipseToBSplineCurve(E, UFirst, ULast, Parameterisation)` | the `BuildCosAndSin` refusals of `circle_trimmed` | `delta > 2*PI + PConfusion` or `delta <= 0` (**not checked** by OCCT 8.0.1 as built, unlike the circle) |
| `hyperbola(h: &Hypr2d, u1, u2)` | `Convert_HyperbolaToBSplineCurve(H, U1, U2)` | never | `abs(u2 - u1) < Epsilon(0.0)`, i.e. `u1 == u2` |
| `parabola(p: &Parab2d, u1, u2)` | `Convert_ParabolaToBSplineCurve(Prb, U1, U2)` | never | `u1 == u2` |

The result is computed exactly as in `cpp/Convert/*.cxx`: the circle and ellipse call `build_cos_and_sin_periodic` (types
`TgtThetaOver2` and `RationalC1`, periodic result) or `build_cos_and_sin(p, 0, 2*PI)` (all other types, non-periodic result;
the full-circle `TgtThetaOver2_1/2` therefore refuse), the trimmed ones `build_cos_and_sin(p, u_first, u_last)` (non-periodic);
the poles are `R*cos`, `±R*sin` (the sign is that of `Ox.X*Oy.Y - Ox.Y*Oy.X`) transformed by `Trsf2d::set_transformation(
conic.x_axis(), gp::ox2d())`; hyperbola and parabola have degree 2, 3 poles, knots `[min, max]` with multiplicity 3, and the
weights `1, cosh((UL - UF)/2), 1` (hyperbola) or `1, 1, 1`. The `gp` conic objects are built by the caller (their constructors
return `Err` for negative radii, an ellipse with `major < minor`...: those are `throws` lines of the golden file, raised by `gp`).

Accessors (`&self`): `degree() -> i32`, `nb_poles() -> i32`, `nb_knots() -> i32`, `is_periodic() -> bool`, `poles() -> &[Pnt2d]`,
`weights() -> &[f64]`, `knots() -> &[f64]`, `multiplicities() -> &[i32]`, and the deprecated checked accessors
`pole(index: i32) -> Result<Pnt2d, ConvertError>`, `weight(index) -> Result<f64, _>`, `knot(index) -> Result<f64, _>`,
`multiplicity(index) -> Result<i32, _>`; `index` is 1-based and `Err(OutOfRange)` outside `1..=nb_poles` (`1..=nb_knots` for
knots and multiplicities): this really raises in OCCT (`throws` lines of `conic_index`). Note that `weights().len() ==
poles().len()` and `multiplicities().len() == knots().len()`.

### `BuildCosAndSin` and `BuildPolynomialCosAndSin`

`BuildCosAndSin` is `protected` in C++ (the oracle calls it through `#define protected public`); in Rust it is public because the
test calls it. It does not depend on the state of the converter, so the Rust functions are associated functions:

```rust
pub struct CosAndSin { pub cos_numerator: Vec<f64>, pub sin_numerator: Vec<f64>, pub denominator: Vec<f64>,
                       pub degree: i32, pub knots: Vec<f64>, pub mults: Vec<i32> }
impl ConicToBSplineCurve {
    /// BuildCosAndSin(Parameterisation, CosNumerator, SinNumerator, Denominator, Degree, Knots, Mults)
    pub fn build_cos_and_sin_periodic(p: ParameterisationType) -> Result<CosAndSin, ConvertError>;
    /// BuildCosAndSin(Parameterisation, UFirst, ULast, ...)
    pub fn build_cos_and_sin(p: ParameterisationType, u_first: f64, u_last: f64) -> Result<CosAndSin, ConvertError>;
}
pub fn build_polynomial_cos_and_sin(u_first: f64, u_last: f64, num_poles: i32) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), ConvertError>;
```

* `build_cos_and_sin_periodic`: `Err(Construction)` (`throws`, explicit) unless `p` is `TgtThetaOver2` or `RationalC1`. Only
  these two types exist for the periodic overload; the golden family `cos_sin_periodic` has all eight types (8 lines: the whole
  input domain, so it is exempt from the 16-line minimum). Each output array has `lower = 1`.
* `build_cos_and_sin`: `Err(Construction)` (`throws`) for `TgtThetaOver2_1` with `delta > 0.9999*PI` and `TgtThetaOver2_2` with
  `delta > 1.9999*PI` (`delta = u_last - u_first`); `Err` (`unchecked`) when `p == TgtThetaOver2` and
  `trunc(1.2*delta/PI) + 1 < 1` (empty arrays), and for `QuasiAngular`, `RationalC1`, `Polynomial` when `delta <= 0`. Other
  inputs, including `delta` larger than `2*PI` or negative with the fixed-span types, are computed (the numbers are meaningless
  but deterministic).
* `build_polynomial_cos_and_sin(u_first, u_last, num_poles)`: `Err` (`unchecked`) unless `num_poles == 8` (the control polygon is
  built for 8 poles) and `0 < u_last - u_first <= 2*PI + PConfusion`. It uses `Locate` (a bisection with `atan2` on the control
  polygon) and `BSplCLib::Trimming`. In the source `TPoles(1).SetCoord(1., 0.)` is the overload `SetCoord(double Xp, double Yp)`
  (both arguments are doubles), i.e. the point `(1, 0)`; `SetCoord(-0.199043, 1.871905)` is the point `(-0.199043, 1.871905)`.

## Elementary surfaces: `ElementarySurfaceToBSplineSurface`

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ElementarySurfaceToBSplineSurface { /* degrees, periodicity, poles, weights, knots, mults */ }
```

`PI/2` below is `M_PI / 2.0`; `nspans(d) = trunc(1.2*d/PI) as i32 + 1`; `Epsilon` and the `gp` types as above. The `gp` surface is built by
the caller. Constructors (all `-> Result<Self, ConvertError>` except the last of each group, which cannot fail and returns `Self`):

| constructor | C++ | `Err` (`unchecked`) when |
|---|---|---|
| `cylinder_trimmed(c: &Cylinder, u1, u2, v1, v2)` | `Convert_CylinderToBSplineSurface(Cyl, U1, U2, V1, V2)` | `abs(v2 - v1) <= abs(Epsilon(v1))` or `u2 - u1 > 2*PI` or `u2 - u1 < 0` |
| `cylinder(c: &Cylinder, v1, v2)` | `(Cyl, V1, V2)`, U periodic | `abs(v2 - v1) <= abs(Epsilon(v1))` |
| `cone_trimmed(c: &Cone, u1, u2, v1, v2)` | `Convert_ConeToBSplineSurface(C, U1, U2, V1, V2)` | as `cylinder_trimmed` |
| `cone(c: &Cone, v1, v2)` | `(C, V1, V2)` | as `cylinder` |
| `sphere_trimmed(s: &Sphere, u1, u2, v1, v2)` | `Convert_SphereToBSplineSurface(Sph, U1, U2, V1, V2)` | `u2 - u1 > 2*PI` or `< 0`, `v1 < -PI/2`, `v2 > PI/2`, or `nspans(v2 - v1) < 1` (`v2 < v1` by more than 2.6; a smaller negative `v2 - v1` is computed) |
| `sphere_param(s, param1, param2, u_trim: bool)` | `(Sph, Param1, Param2, UTrim)`: `u_trim` trims U to `[param1, param2]` (V full, U not periodic); otherwise U is full and periodic and `[param1, param2]` is the V range | `param2 - param1 > 2*PI` or `< 0`; with `u_trim == false` also `nspans(param2 - param1) > 2` (`param2 - param1 >= 5*PI/3` overflows OCCT's fixed arrays) |
| `sphere(s: &Sphere) -> Self` | `(Sph)` | never |
| `torus_trimmed(t: &Torus, u1, u2, v1, v2)` | `Convert_TorusToBSplineSurface(T, U1, U2, V1, V2)` | `u2 - u1` or `v2 - v1` outside `[0, 2*PI]` |
| `torus_param(t, param1, param2, u_trim)` | `(T, Param1, Param2, UTrim)`: `u_trim` trims U (V full and periodic); otherwise U full and periodic and `[param1, param2]` is the V range | `param2 - param1` outside `[0, 2*PI]` |
| `torus(t: &Torus) -> Self` | `(T)` | never |

Note the odd flags of OCCT, to be kept: `sphere_param`/`torus_param` set `is_u_periodic = !u_trim`; `torus_param` sets
`is_v_periodic = u_trim` (the sphere's V is never periodic). All constructors end with `Finalize()`: the arrays have exactly
`nb_u_poles x nb_v_poles` poles and weights, `nb_u_knots`/`nb_v_knots` knots and multiplicities. The poles are transformed with
`Trsf::set_transformation(position, gp::xoy())`.

Accessors (`&self`): `u_degree`, `v_degree`, `nb_u_poles`, `nb_v_poles`, `nb_u_knots`, `nb_v_knots` `-> i32`, `is_u_periodic`,
`is_v_periodic -> bool`, `poles() -> &Array2<Pnt>`, `weights() -> &Array2<f64>`, `u_knots`, `v_knots -> &[f64]`,
`u_multiplicities`, `v_multiplicities -> &[i32]`, and the deprecated scalar accessors `pole(u_index, v_index) -> Result<Pnt, _>`,
`weight(u_index, v_index) -> Result<f64, _>`, `u_knot(i)`, `v_knot(i) -> Result<f64, _>`, `u_multiplicity(i)`, `v_multiplicity(i)
-> Result<i32, _>`, 1-based; outside the ranges (`1..=nb_u_poles` etc.) they return `Err` (OCCT's check is compiled out: the
`surface_index` golden lines are `unchecked` there).

## `CompBezierCurvesToBSplineCurve` and `CompBezierCurves2dToBSplineCurve2d`

```rust
pub struct CompBezierCurvesToBSplineCurve { /* sequence of curves, results */ }          // Pnt
pub struct CompBezierCurves2dToBSplineCurve2d { /* ... */ }                              // Pnt2d
impl CompBezierCurvesToBSplineCurve {
    pub fn new(angular_tolerance: f64) -> Self;       // the C++ default is 1.0e-4 (also `Default`)
    pub fn add_curve(&mut self, poles: &[Pnt]) -> Result<(), ConvertError>;      // Err(Argument) for fewer than 2 poles (unspecified)
    pub fn perform(&mut self) -> Result<(), ConvertError>;
    pub fn degree(&self) -> i32;                       // 0 before perform / for no curve
    pub fn nb_poles(&self) -> i32;
    pub fn poles(&self) -> Vec<Pnt>;                   // Poles(thePoles): nb_poles() items
    pub fn nb_knots(&self) -> i32;
    pub fn knots_and_mults(&self) -> (Vec<f64>, Vec<i32>);                       // nb_knots() items each
}
```

(the 2D class is the same with `Pnt2d`.) `Perform` clears the previous result and recomputes it from all curves added so far, so
`add_curve` after `perform` followed by another `perform` is valid (the golden family does exactly that: it calls `perform` after
`split` curves and again after all of them). With no curve the result is empty (degree 0, no poles, no knots). The class is the C++
template `Convert_CompBezierCurvesToBSplineCurveBase<PointType, VecType>`: the junction test is `VecType::is_parallel` (an
`atan2`-based angle) with the angular tolerance, the 3D version has the extra test `aCurveKnVals(i-1) * lambda > 10 * Epsilon(aDet)`
that the 2D version lacks; curves of lower degree are raised with the Bezier syntax `BSplCLib::IncreaseDegree(NewDegree, Poles,
NoWeights, NewPoles, NoWeights)`, which is `bsplclib::increase_degree(degree, new_degree, false, dim, poles, [0, 1], [degree + 1,
degree + 1])` with `degree = poles.len() - 1` (`BSplCLib_IncreaseDegree_Bezier`: the Bezier knots `0, 1` with multiplicities `degree + 1`).
Curves of one pole (degree 0) are not in the golden file: `bsplclib::increase_degree` needs degree >= 1, so `add_curve` may return `Err` for them.

## `CompPolynomialToPoles`

```rust
pub struct CompPolynomialToPoles { /* flat knots (private), knots, mults, poles, degree, done */ }
impl CompPolynomialToPoles {
    /// Convert_CompPolynomialToPoles(NumCurves, Continuity, Dimension, MaxDegree, NumCoeffPerCurve, Coefficients,
    ///                               PolynomialIntervals, TrueIntervals)  (handle-array constructor, one continuity)
    pub fn new(num_curves: i32, continuity: i32, dimension: i32, max_degree: i32, num_coeff_per_curve: &[i32],
               coefficients: &[f64], polynomial_intervals: &[[f64; 2]], true_intervals: &[f64]) -> Result<Self, ConvertError>;
    /// Convert_CompPolynomialToPoles(NumCurves, Dimension, MaxDegree, Continuity (array), NumCoeffPerCurve, ...)
    pub fn with_continuities(num_curves: i32, dimension: i32, max_degree: i32, continuity: &[i32], num_coeff_per_curve: &[i32],
               coefficients: &[f64], polynomial_intervals: &[[f64; 2]], true_intervals: &[f64]) -> Result<Self, ConvertError>;
    /// Convert_CompPolynomialToPoles(Dimension, MaxDegree, Degree, Coefficients, PolynomialIntervals, TrueIntervals)  (one curve)
    pub fn single(dimension: i32, max_degree: i32, degree: i32, coefficients: &[f64], polynomial_intervals: &[f64],
               true_intervals: &[f64]) -> Result<Self, ConvertError>;
    pub fn is_done(&self) -> bool;          // true after a successful constructor
    pub fn degree(&self) -> i32;
    pub fn nb_poles(&self) -> i32;
    pub fn poles(&self) -> &Array2<f64>;    // nb_poles rows, `dimension` columns
    pub fn nb_knots(&self) -> i32;
    pub fn knots(&self) -> &[f64];
    pub fn multiplicities(&self) -> &[i32];
}
```

The coefficients of curve `k` (0-based) start at `k * dimension * (max(max_degree, degree) + 1)` (`degree` is the largest of
`num_coeff_per_curve[..] - 1` for `new`/`with_continuities`; `single` reads the first curve only: `dimension * (degree + 1)`
values), ordered by increasing power, `dimension` values per power (the `PLib::NoDerivativeEvalPolynomial` layout). Extra
trailing values are allowed and ignored.

* **Err (`throws`, explicit, checked before anything is read):** `new`: `num_curves <= 0`, `continuity < 0`, `max_degree <= 0`,
  `dimension <= 0`; `with_continuities`: `num_curves <= 0`, `max_degree <= 0`, `dimension <= 0`; `single`: `max_degree <= 0`,
  `dimension <= 0`, `polynomial_intervals.len() != 2`. Then, for `new` (after the degree is known) and `with_continuities` (per inner
  knot `ii = 2..=num_curves`, `continuity[ii-1]`): `continuity > degree` with `num_curves > 1` (`Standard_ConstructionError`); and a
  singular interpolation (`BSplCLib::Interpolate` raises `Standard_OutOfRange`; OCCT then reports `inversion_problem != 0` as
  `Standard_ConstructionError`: both are `throws`).
* **Err (`unchecked`):** `num_coeff_per_curve.len() < num_curves`, any of the first `num_curves` entries `< 1`,
  `true_intervals.len() < num_curves + 1` (`single`: `< 2`), `polynomial_intervals.len() < num_curves` (rows),
  `coefficients` shorter than `num_curves * dimension * (max(max_degree, degree) + 1)` (`single`: than `dimension *
  (degree + 1)`), `with_continuities` with `num_curves > 1` and `continuity.len() < num_curves`.
* The algorithm (see the C++): knots `true_intervals[..=num_curves]`, multiplicities `degree - continuity` for the inner knots and
  `degree + 1` at both ends (`single`: both `degree + 1`), the flat knot sequence by `BSplCLib::KnotSequence`, the Schoenberg points
  by `BSplCLib::BuildSchoenbergPoints`, the curve index by walking `true_intervals`, the normalised parameter mapped into
  `polynomial_intervals` row, evaluation by `PLib::NoDerivativeEvalPolynomial`, then `BSplCLib::Interpolate` (dimension > 1 version).
  A continuity equal to the degree gives multiplicity 0 (the inner knot is dropped from the flat knots): follow the C++.

## `GridPolynomialToPoles`

```rust
pub struct GridPolynomialToPoles { /* ... */ }
impl GridPolynomialToPoles {
    /// (MaxUDegree, MaxVDegree, NumCoeff, Coefficients, PolynomialUIntervals, PolynomialVIntervals): one patch
    pub fn single(max_u_degree: i32, max_v_degree: i32, num_coeff: &[i32], coefficients: &[f64], polynomial_u_intervals: &[f64],
                  polynomial_v_intervals: &[f64]) -> Result<Self, ConvertError>;
    /// (NbUSurfaces, NbVSurfaces, UContinuity, VContinuity, MaxUDegree, MaxVDegree, NumCoeffPerSurface, Coefficients,
    ///  PolynomialUIntervals, PolynomialVIntervals, TrueUIntervals, TrueVIntervals)
    pub fn multi(nb_u_surfaces: i32, nb_v_surfaces: i32, u_continuity: i32, v_continuity: i32, max_u_degree: i32, max_v_degree: i32,
                 num_coeff_per_surface: &[[i32; 2]], coefficients: &[f64], polynomial_u_intervals: &[f64],
                 polynomial_v_intervals: &[f64], true_u_intervals: &[f64], true_v_intervals: &[f64]) -> Result<Self, ConvertError>;
    pub fn is_done(&self) -> bool;
    pub fn nb_u_poles(&self) -> Result<i32, ConvertError>;      // the accessors below: Err(NotDone) unless is_done()
    pub fn nb_v_poles(&self) -> Result<i32, ConvertError>;
    pub fn poles(&self) -> Result<&Array2<Pnt>, ConvertError>;
    pub fn u_degree(&self) -> Result<i32, ConvertError>;
    pub fn v_degree(&self) -> Result<i32, ConvertError>;
    pub fn nb_u_knots(&self) -> Result<i32, ConvertError>;
    pub fn nb_v_knots(&self) -> Result<i32, ConvertError>;
    pub fn u_knots(&self) -> Result<&[f64], ConvertError>;
    pub fn v_knots(&self) -> Result<&[f64], ConvertError>;
    pub fn u_multiplicities(&self) -> Result<&[i32], ConvertError>;
    pub fn v_multiplicities(&self) -> Result<&[i32], ConvertError>;
}
```

* `single` ("Perform(0, 0, ...)" with the polynomial intervals as true intervals): `Err` (`throws`, `Standard_DomainError`)
  when `num_coeff.len() != 2`, `coefficients.len() != 3*(max_u_degree+1)*(max_v_degree+1)`, `num_coeff[0] - 1 > max_u_degree`
  or `num_coeff[1] - 1 > max_v_degree`; `Err` (`unchecked`) when `polynomial_u_intervals.len() < 2` or
  `polynomial_v_intervals.len() < 2`. Other lengths of the interval slices are not in the golden file (any `Err` or the C++ result
  is accepted).
* `multi`: with `real_u = max(max_u_degree, 2*u_continuity + 1)`, `real_v` likewise: `Err` (`throws`, `Standard_DomainError`)
  when `num_coeff_per_surface.len() != nb_u_surfaces * nb_v_surfaces`, `coefficients.len() != 3 * nb_u_surfaces * nb_v_surfaces *
  (real_u + 1) * (real_v + 1)`, the largest `num_coeff_per_surface[..][0] - 1 > real_u` or `[..][1] - 1 > real_v`; `Err`
  (`unchecked`) when an interval slice of the polynomial intervals has fewer than 2 values, or `u_continuity` is larger than the
  u degree with `nb_u_surfaces > 1` (negative multiplicities corrupt OCCT's memory), likewise for v. The slices of true intervals
  must have `nb_u_surfaces + 1` and `nb_v_surfaces + 1` values (other lengths are unspecified and not in the golden file). Patch `p`
  (0-based, u fastest: `p = u_index + (nb_u_surfaces) * v_index`) starts at coefficient `3 * (real_u + 1) * (real_v + 1) * p`;
  inside a patch the coefficient of `u^a v^b` (`a < num_coeff[p][0]`, `b < num_coeff[p][1]`) is the 3 values starting at
  `3 * (real_v + 1) * a + 3 * b`.
* A singular interpolation (`BSplSLib::Interpolate`) raises `Standard_OutOfRange` (`throws`); `is_done()` is therefore always `true`
  after a successful constructor in OCCT 8.0.1 (the golden file has no not-done line), and `inversion_problem != 0` is the only
  way to be not done: the Rust implementation returns `Err` from the constructor in that case too. The accessors of a not-done
  object would raise `StdFail_NotDone`, a check that is compiled out; the Rust accessors return `Err(NotDone)`.

## Families of the golden file

(`tests/convert.rs` has the `FAMILIES`, `EXACT`, `INPUT_LESS` and `SMALL_DOMAIN` lists and the line count.) Input words of the
schema, grammars:

| word | tokens |
|---|---|
| `f`, `i` | one float (16 hex digits of the `f64` bits) / one decimal integer |
| `iv`, `fv` | a count `n`, then `n` integers / floats |
| `ipairs` | a count `n`, then `2n` integers (rows of two) |
| `fr2` | `ox oy dx dy sense`: `Ax2d(Pnt2d(ox, oy), Dir2d(dx, dy))` (the direction is normalised by `Dir2d::new`) and the `is_sense` flag of the conic (0 or 1) |
| `fr3` | `ox oy oz nx ny nz vx vy vz indirect`: `Ax3(Pnt(o), Dir(n), Dir(v))` (normalised), then `y_reverse()` when `indirect` is 1 |
| `conic` | `kind` (0 circle, 1 ellipse, 2 hyperbola, 3 parabola) `fr2`, then circle `R trim pt [u1 u2]`, ellipse `major minor trim pt [u1 u2]`, hyperbola `major minor u1 u2`, parabola `focal u1 u2` (`trim` 1: the trimmed constructor, then `u1 u2` follow; `pt` is the index of the parametrisation) |
| `surf` | `kind` `fr3`, then by kind: 0 `R u1 u2 v1 v2` (cylinder_trimmed), 1 `R v1 v2` (cylinder), 2 `angle R u1 u2 v1 v2` (cone_trimmed), 3 `angle R v1 v2` (cone), 4 `R u1 u2 v1 v2` (sphere_trimmed), 5 `R p1 p2 utrim` (sphere_param), 6 `R` (sphere), 7 `major minor u1 u2 v1 v2` (torus_trimmed), 8 `major minor p1 p2 utrim` (torus_param), 9 `major minor` (torus); the cone is `Cone::new(ax3, angle, R)` |
| `bzc2`, `bzc3` | a count of curves, then per curve its number of poles and `2` / `3` floats per pole |

| family | schema | what it replays |
|---|---|---|
| `circle_full`, `circle_trim`, `ellipse_full`, `ellipse_trim`, `hyperbola`, `parabola` | `conic` | the constructor named by the kind; output `degree nb_poles nb_knots periodic`, poles (`1 n` then `x y` per pole), weights, knots, multiplicities |
| `conic_index` | `conic i i` | the conic, then accessor `0 pole, 1 weight, 2 knot, 3 multiplicity` at the 1-based index; `throws` for an index outside the range |
| `cos_sin_periodic` | `i` | `build_cos_and_sin_periodic(type)`; output: cos numerator, sin numerator, denominator (each `1 n values`), degree, knots, multiplicities |
| `cos_sin_trim` | `i f f` | `build_cos_and_sin(type, u1, u2)` |
| `poly_cos_sin` | `f f i` | `build_polynomial_cos_and_sin(u1, u2, num_poles)`; output: three arrays |
| `cylinder_trim`, `cylinder_full`, `cone_trim`, `cone_full`, `sphere_trim4`, `sphere_trim2`, `sphere_full`, `torus_trim4`, `torus_trim2`, `torus_full` | `surf` | the constructor of the kind; output `u_degree v_degree nb_u_poles nb_v_poles nb_u_knots nb_v_knots u_periodic v_periodic`, poles (`1 rows 1 cols`, then `x y z` row by row), weights (same shape), u knots, v knots, u multiplicities, v multiplicities |
| `surface_index` | `surf i i i` | the scalar accessor (`0 pole, 1 weight, 2 u_knot, 3 v_knot, 4 u_multiplicity, 5 v_multiplicity`) at the 1-based indices (`i1` then `i2` for pole and weight) |
| `bezier3d`, `bezier2d` | `f bzc3 i` / `f bzc2 i` | tolerance, the curves, `split`: the first `split` curves are added, `perform`, then the rest, `perform` again; output of each block: `degree nb_poles nb_knots`, all pole coordinates, knots, multiplicities |
| `cpoly_uniform` | `i i i i iv fv fv fv` | `new(n, continuity, dim, maxdeg, num_coeff, coefficients, polynomial_intervals (flat pairs), true_intervals)` |
| `cpoly_array` | `i i i iv iv fv fv fv` | `with_continuities(n, dim, maxdeg, continuity, ...)` |
| `cpoly_single` | `i i i fv fv fv` | `single(dim, maxdeg, degree, coefficients, polynomial_intervals, true_intervals)` |
| `gpoly_single` | `i i iv fv fv fv` | `GridPolynomialToPoles::single` |
| `gpoly_multi` | `i i i i i i ipairs fv fv fv fv fv` | `GridPolynomialToPoles::multi` |

Output of `cpoly_*`: `done degree nb_poles` and the pole array (`1 nb_poles 1 dim` then row by row), `nb_knots`, knots, multiplicities.
Output of `gpoly_*`: `done u_degree v_degree nb_u_poles nb_v_poles nb_u_knots nb_v_knots`, the poles (`1 rows 1 cols`, `x y z`), u knots,
v knots, u multiplicities, v multiplicities; a not-done object would print `0 unchecked` (the test then requires all accessors
to return `Err`; the golden file has no such line).
