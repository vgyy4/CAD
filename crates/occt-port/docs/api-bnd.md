# `bnd` and `cslib`: Rust API contract

This contract covers the bounding volumes of OCCT 8.0.1's `Bnd` package (`cpp/Bnd/`): `Bnd_Box`, `Bnd_Box2d`,
`Bnd_Range`, `Bnd_Sphere`, `Bnd_OBB` and `Bnd_BoundSortBox`, and the surface-normal helpers of `CSLib` (`cpp/CSLib/`):
`CSLib::Normal` (all four overloads), `CSLib::DNNUV` (both), `CSLib::DNNormal`, `CSLib_Class2d` and
`CSLib_NormalPolyDef`. They are the modules `occt_port::bnd` and `occt_port::cslib` (add `pub mod bnd;` and
`pub mod cslib;` to `src/lib.rs`; `src/` may use private submodules, e.g. `src/bnd/{box3d,box2d,range,sphere,obb,bvh,sortbox}.rs`).
The tests are `tests/bnd.rs` with the golden file `tests/bnd.txt` printed by `oracle/bnd.cpp` (real OCCT 8.0.1).

Nothing that exists in `src/` changes its API. The modules reuse `occt_port::gp` (3D and 2D: `Pnt`, `Dir`, `Vec`,
`XYZ`, `XY`, `Pnt2d`, `Dir2d`, `Lin`, `Lin2d`, `Pln`, `Trsf`, `Trsf2d`, `TrsfForm`, `Ax3`, `gp::resolution()`),
`occt_port::math1d` (`Function`, `FunctionWithDerivative`, `FunctionRoots` for `CSLib::Normal` with derivatives),
`occt_port::plib` (`bin` for `PLib::Bin`) and `occt_port::mathlin::MathError`, and must not copy them.

**Sources.** `cpp/Bnd/`, `cpp/CSLib/`, the `gp` and `PLib` sources of the earlier packages, and `cpp/BVH/`: the header-only
BVH templates (`BVH_BoxSet`, `BVH_LinearBuilder`, `BVH_RadixSorter`, `BVH_Traverse`, ...) that `Bnd_OBB::ReBuild` uses for
`theIsOptimal == true`. They are copied from the OCCT 8.0.1 installation; the part of them that matters is described in
"The OBB builder" below. `Bnd_Box::DumpJson`, `InitFromJson`, `Bnd_Range::DumpJson`, `Bnd_OBB::DumpJson` (`Standard_Dump` JSON
text) and the templates `Bnd_B2` / `Bnd_B3` (`Bnd_B2d`, `Bnd_B3f`, ...), `Bnd_Tools` and the `Bnd_Array1Of*` typedefs are **not** in
this contract (see "Not covered").

## Conventions

* **Scalars.** Reals are `f64`, every OCCT `int` is `i32`, booleans are `bool`. `RealLast()` / `RealFirst()` are
  `f64::MAX` / `-f64::MAX`, `RealSmall()` is `f64::MIN_POSITIVE`, `RealEpsilon()` and `Epsilon(1.0)` are `f64::EPSILON`,
  `gp::Resolution()` is `gp::resolution()` (`f64::MIN_POSITIVE`), `Precision::Confusion()` is `1.0e-7`,
  `Precision::SquareConfusion()` is `1.0e-7 * 1.0e-7`, `Precision::PConfusion()` is `1.0e-7 * 0.01` (evaluated in `f64`). The
  "infinite" limit of an open box side is the literal `1.0e100` (`THE_BND_PRECISION_INFINITE`). `std::min(a, b)` is
  `if b < a { b } else { a }` and `std::max(a, b)` is `if a < b { b } else { a }` (the order matters for signed zeros and NaN);
  `std::abs` is `f64::abs`, `std::floor` / `std::ceil` are `f64::floor` / `f64::ceil`, `sqrt` is `f64::sqrt`.
  `IsEqual(a, b)` of `Standard_Real.hxx` is `(a - b).abs() < f64::MIN_POSITIVE`.
* **Arithmetic.** All families but two are compared bit for bit (`EXACT` in `tests/bnd.rs`, signed zeros included): the
  operations must be done in the order of the C++ source, with no fused multiply-add and no reassociation. The two others,
  `cslib_normal_order` and `cslib_normal_poly_def`, call `sin`, `cos` and `pow` (through `CSLib_NormalPolyDef` and
  `math_FunctionRoots`): use `libm::sin`, `libm::cos`, `libm::pow` (the clippy configuration forbids `f64::sin` etc.); they
  allow 8 ulps of the largest absolute value on the line (at least 1; zero tolerance if that value is infinite). The oracle
  prints only cases that stay within 2 ulps when every `sin` / `cos` / `pow` result is moved by an ulp in 24 pseudo-random
  patterns, and the throwaway port used to validate the judge was within 2 ulps of glibc on every line.
* **Out-parameters** become returned values (a tuple in C++ argument order). C++ functions that return a flag and write
  an out-parameter only on `true` (`GetMin`, `GetBounds`, `GetIntermediatePoint`) return an `Option`. `std::optional` results
  stay `Option`. Results the C++ returns as a reference to a list (`Compare`) are returned as a `Vec<i32>`.
* **Overloads.** Rust has none; every C++ overload has its own name (tables below). C++ default arguments do not exist in
  Rust: the golden file passes every argument explicitly, and the tables list the C++ defaults.
* **Types.** Every type derives `Debug` and `Clone` (`BndRange` also `PartialEq`, the C++ `operator==`). `new()` is the C++ default constructor. No type keeps
  a reference to a user object.
* **Errors.** `Result<_, occt_port::mathlin::MathError>` where the table says so. The test only checks `Err` against the
  oracle's `throws` / `unchecked`; the variant is for the caller (`Construction` for a void box, `Last < First`, a failed `gp`
  construction; `Range` for an index or array bound; `Domain` for a duplicate or non-positive count; `NotDone` for a
  missing polygon or array). Nothing may panic: the lints forbid indexing and `unwrap`; a state the C++ would read out of range is an error, never garbage.
* **`throws` and `unchecked`.** OCCT 8.0.1 was built **without** exception checks: the `Standard_*_Raise_if` macros inside the
  libraries are empty, while an explicit `throw` (e.g. `Bnd_Box::Get` of a void box, `Bnd_Range(2, 1)`) is real. The macros
  inside header inlines that the oracle itself instantiates (e.g. the range checks of `NCollection_Array1::Value` in the
  oracle's own code, the zero-norm check of `gp_Dir`) are real too, but the same inline code inside `libTKMath` is not. A
  golden line says `throws` when the call really raised; it says `unchecked` when the check is compiled out so that the C++ would read
  outside an array, write outside it, store a box twice or divide by a null vector: the result is unspecified, the oracle does
  not call the library with such arguments (or discards the non-finite result) and prints `unchecked`. **In both cases the Rust API returns
  `Err`.** The conditions are listed per function below. Where a `gp` normalization is involved the rule is the one of `gp`:
  `Dir::from_vec` / `Dir::from_xyz` / `Vec::normalized` / `XYZ::normalized` fail exactly when OCCT's checked code would raise,
  and a result that the library computes as NaN (null vector divided by its zero norm) is `Err`.
* **Dump texts.** `Dump()` of `Bnd_Box` and `Bnd_Box2d` writes on `std::cout`; `dump(&self) -> String` returns that text.
  `Box3D : ` (`Box2d : `) then `Void`, or `Whole`, or for each limit `\n Xmin : <v>` .. (`Xmin Xmax Ymin Ymax Zmin Zmax`; 2D: `Xmin Xmax Ymin Ymax`)
  where `<v>` is `Infinite` for an open side and else the raw stored limit (no gap added), then `\n Gap : <gap>` and a final `\n`. A
  `double` is written as `std::ostream` does by default: `printf("%g")`, i.e. 6 significant digits, no trailing zeros, exponent form
  `1e-05` / `1.5e+10` when the decimal exponent is below -4 or at least 6, `-0` for negative zero. (A void box dumps `Void`
  even when it has open sides; a whole box dumps `Whole` even when it has a gap.) The golden file stores the text with spaces as `_` and newlines as `\n`, one token.

## The `Bnd_Box` state

`Bnd_Box` has the fields `Xmin Xmax Ymin Ymax Zmin Zmax` (`f64`), `Gap` (`f64`) and `Flags` (`i32`) with
`VoidMask = 0x01`, `XminMask = 0x02`, `XmaxMask = 0x04`, `YminMask = 0x08`, `YmaxMask = 0x10`, `ZminMask = 0x20`,
`ZmaxMask = 0x40`, `WholeMask = 0x7e`. `Bnd_Box2d` has `Xmin Xmax Ymin Ymax Gap Flags` with `VoidMask = 1`, `XminMask = 2`,
`XmaxMask = 4`, `YminMask = 8`, `YmaxMask = 16`, `WholeMask = 0x1e`. The default object is void (`Flags = VoidMask`, limits
`RealLast` / `-RealLast`). A void box may have open sides, a box may be "whole" through `SetWhole` (limits untouched) or by
opening all six sides, and `FinitePart` / `Transformed` / `IsOut` treat these states as the C++ source does. The golden file
builds every box from a recipe (below) through the public API only, and prints the whole observable state
(`void whole open open-sides gap limits square-extent has-finite-part`), so the private fields are checked through the
public accessors.

## `BndBox` (`Bnd_Box`)

| Rust | C++ and notes |
|---|---|
| `new() -> Self` | `Bnd_Box()`: the void box (`Xmin = Ymin = Zmin = RealLast`, `Xmax = Ymax = Zmax = -RealLast`, gap 0, flags `VoidMask`) |
| `from_corners(min: &Pnt, max: &Pnt) -> Self` | `Bnd_Box(const gp_Pnt& theMin, const gp_Pnt& theMax)`: stores the corners as given (they may be inverted), gap 0, flags 0, so the box is **not** void |
| `set_whole(&mut self)` | `SetWhole`: flags := `WholeMask` (so `VoidMask` is cleared); limits and gap are kept |
| `set_void(&mut self)` | `SetVoid`: limits to `RealLast` / `-RealLast`, gap 0, flags := `VoidMask` (clears the open sides) |
| `set_point(&mut self, p: &Pnt)` | `Set(const gp_Pnt&)`: `SetVoid()` then `Add(P)` |
| `set_point_direction(&mut self, p: &Pnt, d: &Dir)` | `Set(const gp_Pnt&, const gp_Dir&)`: `SetVoid()` then `Add(P, D)` |
| `update(&mut self, xmin: f64, ymin: f64, zmin: f64, xmax: f64, ymax: f64, zmax: f64)` | `Update(aXmin, aYmin, aZmin, aXmax, aYmax, aZmax)`: argument order x y z of the minimum, then x y z of the maximum, as in C++. A void box takes the six values (and loses `VoidMask`); otherwise per axis `min`/`max` (the open sides are ignored in 3D) |
| `update_point(&mut self, x: f64, y: f64, z: f64)` | `Update(X, Y, Z)` |
| `gap(&self) -> f64` | `GetGap` |
| `set_gap(&mut self, tol: f64)` | `SetGap`: stores `abs(tol)` |
| `enlarge(&mut self, tol: f64)` | `Enlarge`: `gap := max(gap, abs(tol))` |
| `get(&self) -> Result<[f64; 6], MathError>` | `Get(Xmin, Ymin, Zmin, Xmax, Ymax, Zmax)`: the values of `get_x_min() .. get_z_max()` in this order (so **not** the order of `limits()`). `Err` when the box is void (`Standard_ConstructionError`, `throws`) |
| `limits(&self) -> BoxLimits` | `Get()` returning `Limits` (never fails, also for a void box): `xmin, xmax, ymin, ymax, zmin, zmax` are `GetXMin() .. GetZMax()` |
| `get_x_min(&self) -> f64` | `GetXMin`: `-1e100` when `Xmin` is open, otherwise `Xmin - Gap` |
| `get_x_max(&self) -> f64` | `GetXMax`: `1e100` when open, otherwise `Xmax + Gap` |
| `get_y_min(&self) -> f64` | `GetYMin` |
| `get_y_max(&self) -> f64` | `GetYMax` |
| `get_z_min(&self) -> f64` | `GetZMin` |
| `get_z_max(&self) -> f64` | `GetZMax` |
| `corner_min(&self) -> Result<Pnt, MathError>` | `CornerMin`: `(open ? -1e100 : Xmin - Gap, ...)`. `Err` when void (`throws`) |
| `corner_max(&self) -> Result<Pnt, MathError>` | `CornerMax`: `(open ? 1e100 : Xmax + Gap, ...)`. `Err` when void (`throws`) |
| `center(&self) -> Option<Pnt>` | `Center` (`std::optional<gp_Pnt>`): `None` when void, else `0.5 * (GetXMin() + GetXMax())`, ... |
| `open_xmin(&mut self)` | `OpenXmin`: `Flags |= XminMask` |
| `open_xmax(&mut self)` | `OpenXmax` |
| `open_ymin(&mut self)` | `OpenYmin` |
| `open_ymax(&mut self)` | `OpenYmax` |
| `open_zmin(&mut self)` | `OpenZmin` |
| `open_zmax(&mut self)` | `OpenZmax` |
| `is_open(&self) -> bool` | `IsOpen`: `(Flags & WholeMask) != 0` |
| `is_open_xmin(&self) -> bool` | `IsOpenXmin` |
| `is_open_xmax(&self) -> bool` | `IsOpenXmax` |
| `is_open_ymin(&self) -> bool` | `IsOpenYmin` |
| `is_open_ymax(&self) -> bool` | `IsOpenYmax` |
| `is_open_zmin(&self) -> bool` | `IsOpenZmin` |
| `is_open_zmax(&self) -> bool` | `IsOpenZmax` |
| `is_whole(&self) -> bool` | `IsWhole`: `(Flags & WholeMask) == WholeMask` |
| `is_void(&self) -> bool` | `IsVoid`: `(Flags & VoidMask) != 0` (a void box can still have open sides) |
| `is_x_thin(&self, tol: f64) -> bool` | `IsXThin(tol)` |
| `is_y_thin(&self, tol: f64) -> bool` | `IsYThin(tol)` |
| `is_z_thin(&self, tol: f64) -> bool` | `IsZThin(tol)` |
| `is_thin(&self, tol: f64) -> bool` | `IsThin(tol)` |
| `transformed(&self, t: &Trsf) -> BndBox` | `Transformed(const gp_Trsf&)`: void stays void; `gp_Identity` copies; `gp_Translation` shifts the finite part; otherwise the 8 corners are transformed and added, the gap is copied, and the open sides are added as transformed direction vectors |
| `add_box(&mut self, other: &BndBox)` | `Add(const Bnd_Box&)` |
| `add_point(&mut self, p: &Pnt)` | `Add(const gp_Pnt&)`: `Update(X, Y, Z)` |
| `add_point_direction(&mut self, p: &Pnt, d: &Dir)` | `Add(const gp_Pnt&, const gp_Dir&)`: `Add(P)` then `Add(D)` |
| `add_direction(&mut self, d: &Dir)` | `Add(const gp_Dir&)`: opens the sides whose direction component is beyond `+-RealEpsilon()` |
| `is_out_point(&self, p: &Pnt) -> bool` | `IsOut(const gp_Pnt&)` |
| `is_out_line(&self, l: &Lin) -> bool` | `IsOut(const gp_Lin&)` |
| `is_out_plane(&self, p: &Pln) -> bool` | `IsOut(const gp_Pln&)` (uses `Pln::coefficients`) |
| `is_out_box(&self, other: &BndBox) -> bool` | `IsOut(const Bnd_Box&)` |
| `is_out_box_transformed(&self, other: &BndBox, t: &Trsf) -> bool` | `IsOut(const Bnd_Box& Other, const gp_Trsf& T)`: `IsOut(Other.Transformed(T))` |
| `is_out_transformed(&self, t1: &Trsf, other: &BndBox, t2: &Trsf) -> bool` | `IsOut(const gp_Trsf& T1, const Bnd_Box& Other, const gp_Trsf& T2)`: `Transformed(T1).IsOut(Other.Transformed(T2))` |
| `is_out_segment(&self, p1: &Pnt, p2: &Pnt, d: &Dir) -> bool` | `IsOut(const gp_Pnt& P1, const gp_Pnt& P2, const gp_Dir& D)` (the segment test with the anonymous `IsSegmentOut`; `eps = RealSmall()`) |
| `contains(&self, p: &Pnt) -> bool` | `Contains(P)`: `!IsOut(P)` |
| `intersects(&self, other: &BndBox) -> bool` | `Intersects(Other)`: `!IsOut(Other)` |
| `distance(&self, other: &BndBox) -> f64` | `Distance(const Bnd_Box&)`: 0 when either box is void |
| `dump(&self) -> String` | `Dump()` (C++ prints on `std::cout`; here the text is returned): see "Dump texts" |
| `square_extent(&self) -> f64` | `SquareExtent` (0 when void) |
| `finite_part(&self) -> BndBox` | `FinitePart`: a void box when `!has_finite_part()`, else a box made by `Update(6)` of the limits with the same gap (no open sides) |
| `has_finite_part(&self) -> bool` | `HasFinitePart`: `!IsVoid() && Xmax >= Xmin` |

* `Bnd_Box::Limits` is `BoxLimits { xmin, xmax, ymin, ymax, zmin, zmax }`.
* Types used: `gp::Pnt`, `gp::Dir`, `gp::Lin`, `gp::Pln`, `gp::Trsf`, `gp::TrsfForm`. `Transformed` compares `t.form()` with
  `TrsfForm::Identity` / `TrsfForm::Translation` and reads `t.translation_part()`; `Dir::transform` of a unit axis direction
  cannot fail for a `Trsf` (it is a rotation, mirror or uniform scale), so a failure there may be ignored.

## `BndBox2d` (`Bnd_Box2d`)

| Rust | C++ and notes |
|---|---|
| `new() -> Self` | `Bnd_Box2d()`: void (limits `RealLast`/`-RealLast`, gap 0, flags `VoidMask`) |
| `set_whole(&mut self)` | `SetWhole` |
| `set_void(&mut self)` | `SetVoid` |
| `set_point(&mut self, p: &Pnt2d)` | `Set(const gp_Pnt2d&)`: `Flags = VoidMask; Gap = 0;` (the limits are not reset) then `Add(P)` |
| `set_point_direction(&mut self, p: &Pnt2d, d: &Dir2d)` | `Set(const gp_Pnt2d&, const gp_Dir2d&)`: `Flags = VoidMask; Gap = 0;` then `Add(P, D)` |
| `update(&mut self, xmin: f64, ymin: f64, xmax: f64, ymax: f64)` | `Update(aXmin, aYmin, aXmax, aYmax)`: **in 2D the open sides are respected** (an open side is not updated) |
| `update_point(&mut self, x: f64, y: f64)` | `Update(X, Y)`: same remark |
| `gap(&self) -> f64` | `GetGap` |
| `set_gap(&mut self, tol: f64)` | `SetGap` (`abs`) |
| `enlarge(&mut self, tol: f64)` | `Enlarge` |
| `get(&self) -> Result<[f64; 4], MathError>` | `Get(Xmin, Ymin, Xmax, Ymax)`: the values of `get_x_min(), get_y_min(), get_x_max(), get_y_max()` in this order. `Err` when void (`throws`) |
| `limits(&self) -> BoxLimits2d` | `Get()` returning `Limits`: `xmin, xmax, ymin, ymax` |
| `get_x_min(&self) -> f64` | `GetXMin` |
| `get_x_max(&self) -> f64` | `GetXMax` |
| `get_y_min(&self) -> f64` | `GetYMin` |
| `get_y_max(&self) -> f64` | `GetYMax` |
| `center(&self) -> Option<Pnt2d>` | `Center` (`std::optional<gp_Pnt2d>`) |
| `open_xmin(&mut self)` | `OpenXmin` |
| `open_xmax(&mut self)` | `OpenXmax` |
| `open_ymin(&mut self)` | `OpenYmin` |
| `open_ymax(&mut self)` | `OpenYmax` |
| `is_open_xmin(&self) -> bool` | `IsOpenXmin` |
| `is_open_xmax(&self) -> bool` | `IsOpenXmax` |
| `is_open_ymin(&self) -> bool` | `IsOpenYmin` |
| `is_open_ymax(&self) -> bool` | `IsOpenYmax` |
| `is_whole(&self) -> bool` | `IsWhole`: `(Flags & WholeMask) == WholeMask` (`WholeMask = 0x1e`) |
| `is_void(&self) -> bool` | `IsVoid` |
| `transformed(&self, t: &Trsf2d) -> BndBox2d` | `Transformed(const gp_Trsf2d&)`: see the source (note the order: a copy first; identity / translation shift the closed sides only; otherwise the open directions are transformed and added to a void box, then the vertices of the closed corners, then `Gap` is copied) |
| `add_box(&mut self, other: &BndBox2d)` | `Add(const Bnd_Box2d&)` (the order of the cases differs from `Bnd_Box::Add`: whole, other void, other whole, this void, general; the gap is the maximum only in the general case) |
| `add_point(&mut self, p: &Pnt2d)` | `Add(const gp_Pnt2d&)`: `Update(X, Y)` |
| `add_point_direction(&mut self, p: &Pnt2d, d: &Dir2d)` | `Add(const gp_Pnt2d&, const gp_Dir2d&)` |
| `add_direction(&mut self, d: &Dir2d)` | `Add(const gp_Dir2d&)` |
| `is_out_point(&self, p: &Pnt2d) -> bool` | `IsOut(const gp_Pnt2d&)` |
| `is_out_line(&self, l: &Lin2d) -> bool` | `IsOut(const gp_Lin2d&)` |
| `is_out_segment(&self, p0: &Pnt2d, p1: &Pnt2d) -> bool` | `IsOut(const gp_Pnt2d& theP0, const gp_Pnt2d& theP1)` |
| `is_out_box(&self, other: &BndBox2d) -> bool` | `IsOut(const Bnd_Box2d&)` |
| `is_out_box_transformed(&self, other: &BndBox2d, t: &Trsf2d) -> bool` | `IsOut(const Bnd_Box2d&, const gp_Trsf2d&)` (inline in the header) |
| `is_out_transformed(&self, t1: &Trsf2d, other: &BndBox2d, t2: &Trsf2d) -> bool` | `IsOut(const gp_Trsf2d&, const Bnd_Box2d&, const gp_Trsf2d&)` (inline) |
| `contains(&self, p: &Pnt2d) -> bool` | `Contains` |
| `intersects(&self, other: &BndBox2d) -> bool` | `Intersects` |
| `distance(&self, other: &BndBox2d) -> f64` | `Distance` (0 when either box is void) |
| `dump(&self) -> String` | `Dump()`: see "Dump texts" |
| `square_extent(&self) -> f64` | `SquareExtent` |

* `Bnd_Box2d::Limits` is `BoxLimits2d { xmin, xmax, ymin, ymax }`. `Bnd_Box2d` has no constructor from corners, no `IsThin`,
  no `FinitePart`, no `CornerMin` / `CornerMax`. The transformation recipes of the golden file avoid `sin` / `cos`.

## `BndRange` (`Bnd_Range`)

| Rust | C++ and notes |
|---|---|
| `new() -> Self` | `Bnd_Range()`: `myFirst = 0, myLast = -1` (void) |
| `from_bounds(min: f64, max: f64) -> Result<Self, MathError>` | `Bnd_Range(theMin, theMax)`: `Err` when `theMax < theMin` (`Standard_ConstructionError("Last < First")`, `throws`) |
| `common(&mut self, other: &BndRange)` | `Common`: intersection; a void argument makes this void |
| `union(&mut self, other: &BndRange) -> bool` | `Union` (`[[nodiscard]] bool`): false (and no change) when either is void or the ranges are disjoint |
| `split(&self, val: f64, period: f64) -> Vec<BndRange>` | `Split(theVal, theList, thePeriod)` (the list is returned; C++ default `thePeriod = 0.0`); uses `is_intersected` and `Bnd_Range(a, b)` constructions of the C++ order. The golden file keeps `|first - last| / period <= 400` so the list stays short |
| `is_intersected(&self, val: f64, period: f64) -> IntersectStatus` | `IsIntersected(theVal, thePeriod = 0.0)`: `IsEqual(a, b)` is `abs(a - b) < RealSmall()`; the quotients go through `floor` and an `i32` conversion (the golden file stays far from overflow) |
| `add_parameter(&mut self, p: f64)` | `Add(double)` |
| `add_range(&mut self, other: &BndRange)` | `Add(const Bnd_Range&)` |
| `get_min(&self) -> Option<f64>` | `GetMin(double&)` -> `Some(v)` / `None` (false) |
| `get_max(&self) -> Option<f64>` | `GetMax(double&)` |
| `get_bounds(&self) -> Option<(f64, f64)>` | `GetBounds(double&, double&)` |
| `get(&self) -> Option<(f64, f64)>` | `Get()` (`std::optional<Bounds>`) as `(min, max)` |
| `get_intermediate_point(&self, lambda: f64) -> Option<f64>` | `GetIntermediatePoint(theLambda, double&)`: `first + lambda * (last - first)` |
| `center(&self) -> Option<f64>` | `Center` (optional) |
| `delta(&self) -> f64` | `Delta`: `last - first` (-1 for a void range) |
| `is_void(&self) -> bool` | `IsVoid`: `last < first` |
| `set_void(&mut self)` | `SetVoid` |
| `enlarge(&mut self, delta: f64)` | `Enlarge`: no-op when void |
| `shifted(&self, val: f64) -> BndRange` | `Shifted`: `Bnd_Range(first + v, last + v)` (void stays void) |
| `shift(&mut self, val: f64)` | `Shift` |
| `trim_from(&mut self, lower: f64)` | `TrimFrom` |
| `trim_to(&mut self, upper: f64)` | `TrimTo` |
| `is_out_value(&self, v: f64) -> bool` | `IsOut(double)` |
| `is_out_range(&self, r: &BndRange) -> bool` | `IsOut(const Bnd_Range&)` |
| `contains(&self, v: f64) -> bool` | `Contains(double)` |
| `intersects(&self, r: &BndRange) -> bool` | `Intersects(const Bnd_Range&)` |
| `min(&self) -> Option<f64>` | `Min` (optional) |
| `max(&self) -> Option<f64>` | `Max` (optional) |

* `Bnd_Range::IntersectStatus` is `IntersectStatus { Out = 0, In = 1, Boundary = 2 }`.
* `Bnd_Range::Bounds` is returned as `(min, max)`. The `NCollection_List<Bnd_Range>` of `Split` is a `Vec`.

## `BndSphere` (`Bnd_Sphere`)

| Rust | C++ and notes |
|---|---|
| `new() -> Self` | `Bnd_Sphere()`: center (0, 0, 0), radius 0, not valid, `U = V = 0` |
| `with_center(center: &XYZ, radius: f64, u: i32, v: i32) -> Self` | `Bnd_Sphere(const gp_XYZ&, double theRad, int theU, int theV)`: `IsValid() == false` |
| `u(&self) -> i32` | `U` |
| `v(&self) -> i32` | `V` |
| `is_valid(&self) -> bool` | `IsValid` |
| `set_valid(&mut self, valid: bool)` | `SetValid` |
| `center(&self) -> &XYZ` | `Center` |
| `radius(&self) -> f64` | `Radius` |
| `distances(&self, p: &XYZ) -> (f64, f64)` | `Distances(theXYZ, theMin, theMax)` -> `(min, max)` |
| `square_distances(&self, p: &XYZ) -> (f64, f64)` | `SquareDistances(theXYZ, theMin, theMax)` -> `(min, max)` |
| `project(&self, node: &XYZ) -> (bool, XYZ, f64, bool)` | `Project(theNode, theProjNode, theDist, theInside)` -> `(returned bool, projection, distance, inside)` (always `true`, the center, ..., `true`) |
| `distance(&self, node: &XYZ) -> f64` | `Distance` |
| `square_distance(&self, node: &XYZ) -> f64` | `SquareDistance` |
| `add(&mut self, other: &BndSphere)` | `Add(const Bnd_Sphere&)`: a radius `< 0` means "not initialised" and takes the other sphere (including its `U`, `V` and validity) |
| `is_out_sphere(&self, other: &BndSphere) -> bool` | `IsOut(const Bnd_Sphere&)` |
| `is_out_point(&self, p: &XYZ, max_dist: f64) -> (bool, f64)` | `IsOut(const gp_XYZ&, double& theMaxDist)` -> `(out, new theMaxDist)`: the max distance is lowered only when the sphere is valid and not out |
| `square_extent(&self) -> f64` | `SquareExtent`: `4 * r * r` |

## `BndObb` (`Bnd_OBB`)

| Rust | C++ and notes |
|---|---|
| `new() -> Self` | `Bnd_OBB()`: half sizes -1, center and axes zero, not an AABox |
| `from_axes(center: &Pnt, x: &Dir, y: &Dir, z: &Dir, hx: f64, hy: f64, hz: f64) -> Self` | `Bnd_OBB(center, XDirection, YDirection, ZDirection, HXSize, HYSize, HZSize)`: stores them as given (negative sizes are accepted: the `Standard_ASSERT_VOID` checks are compiled out) |
| `from_box(b: &BndBox) -> Self` | `Bnd_OBB(const Bnd_Box&)`: a void box gives the void OBB; otherwise the axes are the coordinate axes, the sizes `0.5 * (max - min)` and the center `0.5 * (max + min)` of `Get(..)` (so an open box gives the `1e100` limits), `IsAABox() == true` |
| `rebuild(&mut self, points: &[Pnt], tolerances: Option<&[f64]>, optimal: bool) -> Result<(), MathError>` | `ReBuild(theListOfPoints, theListOfTolerances, theIsOptimal)`: the points as a slice (the lower index of the C++ array is irrelevant), the tolerances `None` (C++ `nullptr`) or a slice with as many values as points, `optimal` = `theIsOptimal`. `Err` for an empty point list (the C++ crashes: not in the golden file) and for a tolerance slice of another length. One point: `ProcessOnePoint` + `Enlarge(tol.First())`; two points: the special branch; otherwise `OBBTool` (see "The OBB builder") |
| `set_center(&mut self, c: &Pnt)` | `SetCenter` |
| `set_x_component(&mut self, d: &Dir, hx: f64)` | `SetXComponent(dir, size)` |
| `set_y_component(&mut self, d: &Dir, hy: f64)` | `SetYComponent` |
| `set_z_component(&mut self, d: &Dir, hz: f64)` | `SetZComponent` |
| `position(&self) -> Result<Ax3, MathError>` | `Position()` -> `gp_Ax3(myCenter, ZDirection(), XDirection())` (the `gp_XYZ` -> `gp_Dir` conversions normalize). `Err` when a conversion or the `Ax3` construction fails (void/default OBB: zero axes; `throws`) |
| `center(&self) -> &XYZ` | `Center` |
| `x_direction(&self) -> &XYZ` | `XDirection` |
| `y_direction(&self) -> &XYZ` | `YDirection` |
| `z_direction(&self) -> &XYZ` | `ZDirection` |
| `x_hsize(&self) -> f64` | `XHSize` |
| `y_hsize(&self) -> f64` | `YHSize` |
| `z_hsize(&self) -> f64` | `ZHSize` |
| `half_sizes(&self) -> (f64, f64, f64)` | `GetHalfSizes()` as `(X, Y, Z)` |
| `is_void(&self) -> bool` | `IsVoid`: any half size `< 0` |
| `set_void(&mut self)` | `SetVoid`: sizes -1, center and axes zero, not an AABox |
| `set_aabox(&mut self, flag: bool)` | `SetAABox` |
| `is_aabox(&self) -> bool` | `IsAABox` |
| `enlarge(&mut self, gap: f64)` | `Enlarge(theGapAdd)`: adds `abs(gap)` to each half size (also for a void OBB) |
| `vertices(&self) -> Option<[Pnt; 8]>` | `GetVertex(gp_Pnt[8])`: `None` when void, else the 8 vertices in the order `---, +--, -+-, ++-, --+, +-+, -++, +++` (sign of x, y, z), each computed `((center +- hx * ax) +- hy * ay) +- hz * az` from the left |
| `square_extent(&self) -> f64` | `SquareExtent` |
| `is_out_obb(&self, other: &BndObb) -> bool` | `IsOut(const Bnd_OBB&)` |
| `is_out_point(&self, p: &Pnt) -> bool` | `IsOut(const gp_Pnt&)` |
| `contains(&self, p: &Pnt) -> bool` | `Contains` |
| `intersects(&self, other: &BndObb) -> bool` | `Intersects` |
| `is_completely_inside(&self, other: &BndObb) -> bool` | `IsCompletelyInside` |
| `add_obb(&mut self, other: &BndObb)` | `Add(const Bnd_OBB&)`: 16 vertices -> `ReBuild` (non-optimal, no tolerances) |
| `add_point(&mut self, p: &Pnt)` | `Add(const gp_Pnt&)`: 8 vertices + the point -> `ReBuild` |

`Bnd_OBB` has the fields `myCenter` (`XYZ`), `myAxes[3]` (`XYZ`), `myHDims[3]` and `myIsAABox`. `Bnd_OBB::HalfSizes` is returned as a tuple.
There is no `Dump` in 8.0.1 (only `DumpJson`, not covered).

### The OBB builder (`Bnd_OBB::ReBuild`)

`ReBuild` with one point, two points (the branch of `Bnd_OBB.cxx` with `aDPm < Precision::Confusion()` included) or more (the
`OBBTool` of `Bnd_OBB.cxx`: DiTO) must be translated line by line. Notes for the general case:

* `theIsOptimal == false` projects all points (`Project` loops over the points in index order with strict `<` / `>`, so among equal
  projections the **first** point wins) and uses the standard DiTO triangles. `theIsOptimal == true` tries all triangles of the 14
  extreme points and takes the extreme points from the **BVH** of the points. For distinct projections both give the same minima; for **ties**
  (several points with the same extreme projection and different other coordinates: lattices, cube corners, symmetric sets) the point
  the BVH selector returns depends on the order of the BVH, and the golden file has such sets (`obb_rebuild_optimal_special`,
  4 of its 34 lines fail with a first-wins scan in input order, and none of the 22 lines of `obb_rebuild_optimal`). The golden file has no tolerance-dependent tie.
* **The point BVH** (`cpp/BVH/`), for `theIsOptimal`: `BVH_BoxSet<double, 3, gp_XYZ>` with `BVH_LinearBuilder<double, 3>(30)` (leaf size 30,
  `BVH_Constants_MaxTreeDepth` 32).
  1. Element `i` is the point `P_i` with the box `[P_i - tol_i, P_i + tol_i]` per axis, `tol_i = tolerances[i]` or `Precision::Confusion()` when there is no tolerance array.
  2. Scene box: the union of the element boxes (per axis `min` of the minima, `max` of the maxima).
  3. `BVH_RadixSorter`: per axis `size = max(1.0e-5, scene_max - scene_min)` (`BVH::THE_NODE_MIN_SIZE`), `rev = 1024.0 / size`; for each element the center
     `c = (box_min + box_max) * 0.5`, `voxel = clamp(IntFloor((c - scene_min) * rev), 0, 1023)` with `IntFloor(v) = (int)v - ((int)v > v)`;
     the 30-bit Morton code interleaves the bits (`x` bit `k` -> bit `3k`, `y` -> `3k + 1`, `z` -> `3k + 2`, `BVH::EncodeMortonCode`); the elements are sorted by
     ascending code (OCCT sorts with an MSB-first `std::partition` radix sort, which is not stable; a stable sort by code is enough for the golden file).
  4. `BVH_LinearBuilder::emitHierachy` builds the binary tree over the sorted elements: a range of more than 30 elements is split at the first element
     whose Morton bit `digit` (starting at 29, decreasing; at digit -1 the middle) is set (`lowerBound`; if the split is at an end the next digit is tried), nodes are
     numbered in depth-first order (the inner node first, then the left subtree, then the right one), a leaf holds the element range `[start, final)`.
     `UpdateBounds` gives every node the union of its children's / elements' boxes.
  5. `OBB_ExtremePointsSelector` is a `BVH_Traverse` with a `Bnd_Range` metric (`Select` of `BVH_Traverse.lxx`): it keeps `myPrmMin/Max` (initially `RealLast` / `RealFirst`)
     and the points of the extreme projections on the axis, accepts leaves element by element in tree order (a root leaf is accepted without `RejectNode`), rejects a
     node when none of the projections of its 8 box corners extends the range, orders the two children with `IsMetricBetter` and skips stacked nodes with `RejectMetric`; the exact code is in
     `Bnd_OBB.cxx` (class `OBB_ExtremePointsSelector`) and `BVH_Traverse.lxx`. A fresh selector is used for every projection.
  A Rust translation of exactly these steps reproduces every line of the golden file bit for bit (checked with a throwaway port).
* With `theListOfTolerances` the tolerance of point `i` is `tolerances[i]` (same length as the points), in `BuildBox` (`aDx - aTol`, ...) and, for `theIsOptimal`, in the BVH boxes.

## `BndBoundSortBox` (`Bnd_BoundSortBox`)

| Rust | C++ and notes |
|---|---|
| `new() -> Self` | `Bnd_BoundSortBox()` |
| `initialize_boxes(&mut self, boxes: &[BndBox], lower: i32)` | `Initialize(const occ::handle<NCollection_HArray1<Bnd_Box>>&)`: `boxes[k]` has the C++ index `lower + k`; the enclosing box is the union of the non-void boxes |
| `initialize_enclosing_boxes(&mut self, enclosing: &BndBox, boxes: &[BndBox], lower: i32)` | `Initialize(const Bnd_Box& theEnclosingBox, const occ::handle<NCollection_HArray1<Bnd_Box>>&)` |
| `initialize_enclosing_count(&mut self, enclosing: &BndBox, nb_boxes: i32) -> Result<(), MathError>` | `Initialize(const Bnd_Box& theEnclosingBox, const int theNbBoxes)`: creates `theNbBoxes` void boxes with indices `1..=n`. `Err` for `nb_boxes <= 0` (`Standard_NullValue_Raise_if` is compiled out: `unchecked`; a negative count crashes the C++) |
| `add(&mut self, b: &BndBox, index: i32) -> Result<(), MathError>` | `Add(theBox, theIndex)`. `Err` when no array exists, the index is outside the array (`unchecked`: the C++ writes outside it), the index already holds a non-void box (`Standard_MultiplyDefined`, compiled out: `unchecked`), or the enclosing box is void while `b` is not (`CornerMin` raises: `throws`). A void `b` is ignored (`Ok`) after the checks of the index |
| `compare_box(&mut self, b: &BndBox) -> Vec<i32>` | `Compare(const Bnd_Box&)` -> the indices, in the order of `myLastResult` |
| `compare_plane(&mut self, p: &Pln) -> Vec<i32>` | `Compare(const gp_Pln&)` -> the indices of the boxes (in index order) that are not out of the plane |

* The voxel resolution is `8` for up to 100 boxes, `16` up to 1000, `32` up to 10000, `64` up to 40000, else `128`
  (`getBnd_VoxelGridResolution`, from the box count of the array; for `initialize_enclosing_count` the given count). A box whose smallest voxel span times 4 exceeds the resolution is a "large box"
  (`myLargeBoxes`), else it is entered into the three slice tables; `Compare` returns the large boxes that are not out first (in insertion order), then
  the boxes found through the slices in the order of the Z slices (each box once). Study `Bnd_BoundSortBox.cxx`: the order of the returned list is part of the golden file.
* `static_cast<int>(double)` in `getBoundingVoxels` is undefined behaviour when the value does not fit: the golden file only has cases in which every
  value is below 1e9 in magnitude, so Rust's saturating `as i32` is safe.
* `Compare` on an object that was never initialized, and `Add` before an `Initialize` of the count form, are not in the golden file (the C++ crashes); `add` returns `Err`.

## `cslib`

`cslib::NormalStatus` (`CSLib_NormalStatus`) and `cslib::DerivativeStatus` (`CSLib_DerivativeStatus`) are the enums of the headers with the C++
numbering (the golden file prints it: `cslib_enum_values`); `cslib::Class2dResult` is `CSLib_Class2d::Result` (`Inside = 1`, `Outside = -1`, `Uncertain = 0`).
`NormalOfOrder { status, normal, order_u, order_v }` carries the out-parameters of the derivative-order `Normal`. The free functions:

| Rust | C++ and notes |
|---|---|
| `normal_sin_tol(d1u: &GpVec, d1v: &GpVec, sin_tol: f64) -> Result<(DerivativeStatus, Option<Dir>), MathError>` | `CSLib::Normal(D1U, D1V, SinTol, theStatus, theNormal)` -> `(status, normal)`; `normal` is `Some` exactly when the C++ writes `theNormal` (status `Done`). `Err` where the C++ builds a `gp_Dir` from a null vector (`unchecked`; with the Rust `gp` API: `Dir::from_vec` fails) |
| `normal_second_order( d1u: &GpVec, d1v: &GpVec, d2u: &GpVec, d2v: &GpVec, d2uv: &GpVec, sin_tol: f64, ) -> Result<(bool, NormalStatus, Option<Dir>), MathError>` | `CSLib::Normal(D1U, D1V, D2U, D2V, D2UV, SinTol, Done, Status, Normal)` -> `(done, status, normal)`; `normal` is `Some` for the statuses `D1NuIsNull`, `D1NvIsNull`, `D1NuIsParallelD1Nv` |
| `normal_mag_tol(d1u: &GpVec, d1v: &GpVec, mag_tol: f64) -> Result<(NormalStatus, Option<Dir>), MathError>` | `CSLib::Normal(D1U, D1V, MagTol, Status, Normal)` -> `(status, normal)`; `normal` is `Some` for `Defined` |
| `normal_of_order( max_order: i32, der: &VecArray2, mag_tol: f64, u: f64, v: f64, umin: f64, umax: f64, vmin: f64, vmax: f64, ) -> Result<NormalOfOrder, MathError>` | `CSLib::Normal(MaxOrder, DerNUV, MagTol, U, V, Umin, Umax, Vmin, Vmax, Status, Normal, OrderU, OrderV)`. `Err` when `max_order >= 0` and the array does not cover rows `0..=max_order` and columns `0..=max_order` (`unchecked`: the C++ reads outside), and where a normalization of a null vector occurs. For `max_order < 0` the array is not read. Uses `math1d::FunctionRoots` and `NormalPolyDef` |
| `dn_nuv(nu: i32, nv: i32, der: &VecArray2) -> Result<GpVec, MathError>` | `CSLib::DNNUV(Nu, Nv, DerSurf)`. For `nu >= 0 && nv >= 0` the array must cover rows `0..=nu+1` and columns `0..=nv+1`, else `Err` (`unchecked`); for negative `nu` or `nv` the loops do not run and the result is the zero vector |
| `dn_nuv_two(nu: i32, nv: i32, der1: &VecArray2, der2: &VecArray2) -> Result<GpVec, MathError>` | `CSLib::DNNUV(Nu, Nv, DerSurf1, DerSurf2)`: `der1` must cover rows `1..=nu+1` x columns `0..=nv`, `der2` rows `0..=nu` x columns `1..=nv+1` (when `nu >= 0 && nv >= 0`), else `Err` |
| `dn_normal(nu: i32, nv: i32, der: &VecArray2, iduref: i32, idvref: i32) -> Result<GpVec, MathError>` | `CSLib::DNNormal(Nu, Nv, DerNUV, Iduref = 0, Idvref = 0)` (the C++ defaults are not available in Rust). The array must cover rows `iduref..=nu+iduref` and columns `idvref..=nv+idvref`, else `Err` (`unchecked`); the first derivative `der(iduref, idvref)` must not be null (`Err`, `unchecked`). The golden file keeps `nu, nv >= 0`, `nu + nv + max(iduref) + max(idvref) <= 25` (the range of `PLib::Bin`) |

A `normal: Option<Dir>` is `Some` exactly when the C++ function assigns `theNormal` (the oracle detects this by calling the function with two different initial values); a status without a normal
leaves the argument untouched.

### `VecArray2` (`NCollection_Array2<gp_Vec>`)

| Rust | C++ and notes |
|---|---|
| `new(row_lower: i32, col_lower: i32, rows: usize, cols: usize, values: Vec<GpVec>) -> Result<Self, MathError>` | `NCollection_Array2<gp_Vec>(rowLower, rowUpper, colLower, colUpper)` filled from `values` (row-major, `rows * cols` of them); `Err` if `rows` or `cols` is 0 or the count is wrong |
| `row_lower(&self) -> i32` | `LowerRow` |
| `row_upper(&self) -> i32` | `UpperRow` |
| `col_lower(&self) -> i32` | `LowerCol` |
| `col_upper(&self) -> i32` | `UpperCol` |
| `value(&self, row: i32, col: i32) -> Result<&GpVec, MathError>` | `Value(row, col)`: `Err(Range)` outside the bounds |

### `Class2d` (`CSLib_Class2d`)

| Rust | C++ and notes |
|---|---|
| `new() -> Self` | `CSLib_Class2d()`: no polygon (`myPointsCount == 0`) |
| `from_points(points: &[Pnt2d], tol_u: f64, tol_v: f64, umin: f64, vmin: f64, umax: f64, vmax: f64) -> Self` | the three constructors `CSLib_Class2d(NCollection_Array1 / NCollection_Sequence / NCollection_DynamicArray<gp_Pnt2d>, theTolU, theTolV, theUMin, theVMin, theUMax, theVMax)`: they all use the points by position (`Lower()` is only an offset), so one Rust function serves the three; the golden file feeds the same polygon through all three. Fewer than 3 points or `umax <= umin` or `vmax <= vmin`: no polygon is stored (`myPointsCount = 0`, the tolerances stay 0) but the bounds are kept |
| `si_dans(&self, p: &Pnt2d) -> Class2dResult` | `SiDans` |
| `si_dans_on_mode(&self, p: &Pnt2d, tol: f64) -> Class2dResult` | `SiDans_OnMode` |
| `internal_si_dans(&self, x: f64, y: f64) -> Result<bool, MathError>` | `internalSiDans(X, Y)` (private in C++; the oracle makes it reachable): the arguments are in the **normalized** coordinates. `Err` when no polygon is stored (the arrays are empty: `unchecked`) |
| `internal_si_dans_ou_on(&self, x: f64, y: f64) -> Result<Class2dResult, MathError>` | `internalSiDansOuOn(X, Y)` (private in C++): same remarks |

The result enum values are those of the header; `SiDans` returns `Uncertain` when no polygon is stored.

### `NormalPolyDef` (`CSLib_NormalPolyDef`)

| Rust | C++ and notes |
|---|---|
| `new(k0: i32, li_lower: i32, li: &[f64]) -> Result<Self, MathError>` | `CSLib_NormalPolyDef(theK0, theLi)`: copies `theLi(i)` for `i = 0..=theK0`. `theLi` is the slice with the C++ lower index `li_lower`; `Err` unless `k0 >= 0`, `li_lower <= 0` and the slice reaches index `k0` (the C++ reads outside the array: `unchecked`) |
| `fn value(&mut self, x: f64) -> Option<f64>` | `Value`: always `Some`; `F := 0` first, returns early when `abs(cos x) <= RealSmall()` or `abs(sin x) <= RealSmall()`; use `libm::cos`, `libm::sin`, `libm::pow` |
| `fn derivative(&mut self, x: f64) -> Option<f64>` | `Derivative`: same structure |
| `fn values(&mut self, x: f64) -> Option<(f64, f64)>` | `Values` |

`NormalPolyDef` implements `math1d::Function` and `math1d::FunctionWithDerivative` (all three callbacks always return `Some`), so `normal_of_order` hands it to `math1d::FunctionRoots::new(&mut poly, inf, sup, 200, 1.0e-5, Precision::Confusion(), Precision::Confusion(), 0.0)`.

## Errors at a glance

| call | `Err` |
|---|---|
| `BndBox::get`, `corner_min`, `corner_max`, `BndBox2d::get` | the box is void (`throws`) |
| `BndRange::from_bounds` | `max < min` (`throws`) |
| `BndObb::position` | a zero axis (void / default OBB), or an `Ax3` that cannot be built (`throws`) |
| `BndObb::rebuild` | empty `points`, or a tolerance slice of another length (not in the golden file) |
| `BndBoundSortBox::initialize_enclosing_count` | `nb_boxes <= 0` (`unchecked`) |
| `BndBoundSortBox::add` | the enclosing box is void and the box is not (`throws`); index outside the array or already holding a non-void box (`unchecked`) |
| `cslib::normal_*`, `dn_normal` | a normalization of a null vector (`unchecked`) |
| `cslib::normal_of_order`, `dn_nuv`, `dn_nuv_two`, `dn_normal` | an array that does not cover the indices listed in the table (`unchecked`) |
| `Class2d::internal_si_dans`, `internal_si_dans_ou_on` | no polygon stored (`unchecked`) |
| `NormalPolyDef::new` | `li` does not reach index `k0` or its lower index is positive (`unchecked`) |

## The golden file and the judge

`tests/bnd.txt` has 3532 lines in 102 families (at least 16 distinct lines in every family with inputs; two families have no input and are
printed once: `sphere_default`, `cslib_enum_values`), about 1.4 MB. `oracle/bnd.cpp` prints it from the real OCCT; build and run with

```
OCCT=<scratch>/deps/occt-install
mkdir -p oracle-bin && c++ -std=c++17 -O2 oracle/bnd.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKMath -lTKernel -o oracle-bin/bnd
LD_LIBRARY_PATH=$OCCT/lib oracle-bin/bnd > tests/bnd.txt
```

Every case is run four times with differently filled stack and heap and kept only when the four runs agree (none of the cases
reads uninitialised memory in this file); cases with a non-finite result are dropped (an overflow of an exact expression to
`+-inf` is kept, e.g. `SquareExtent` of a whole box).

**Record format.** `<family> <inputs...> | <outputs...>`; floats are 16-hex-digit bit patterns, ints and bools decimal; the output
is `throws`, `unchecked` or the values the `run` function of `tests/bnd.rs` pushes in the same order as the oracle prints them.
The schema words (the reader method of the same name in the judge) are:

| word | tokens | meaning |
|---|---|---|
| `f` `i` | 1 | a float / an int |
| `xyz` `pnt` `vec` | 3 | three floats |
| `dir` | 3 | a **raw** direction: `Dir::new(x, y, z)` normalizes it (the oracle does the same with `gp_Dir`) |
| `xy` `pnt2d` `dir2d` | 2 | the 2D counterparts (`dir2d` raw) |
| `bx` | 9 | `Bnd_Box` recipe `kind xmin ymin zmin xmax ymax zmax gap flags`: kind 0 `Bnd_Box()` (numbers unused); 1 `Bnd_Box(Pnt(xmin,ymin,zmin), Pnt(xmax,ymax,zmax))` (may be inverted); 2 `Update(xmin,ymin,zmin,xmax,ymax,zmax)` on a void box; 3 `SetWhole()` (numbers unused); 4 `Update(xmin,ymin,zmin)` (a point). Then `SetGap(gap)`, then `OpenXmin OpenXmax OpenYmin OpenYmax OpenZmin OpenZmax` for the set bits 0..5 of `flags` |
| `bx2` | 7 | `Bnd_Box2d` recipe `kind xmin ymin xmax ymax gap flags`: kind 0 default; 1 `Update(xmin,ymin,xmax,ymax)`; 2 `SetWhole()`; 3 `Update(xmin,ymin)`; 4 `Set(Pnt2d(xmin,ymin))`. Then `SetGap`, then the 4 open bits |
| `trsf` | var. | a `gp_Trsf` recipe: `kind` then 2 `pnt f` (`SetScale`); 3 `xyz` (`SetTranslation`); 4 `pnt dir` (`SetMirror(Ax1)`); 5 `pnt` (`SetMirror(Pnt)`); 6 `pnt dir` (`SetMirror(Ax2::from_direction)`); 7 `x y z w` (`SetRotation(Quaternion)`); 8 `x y z w xyz f` (rotation, then `SetTranslationPart`, `SetScaleFactor`); 9 nothing (identity). No kind uses `sin` / `cos` |
| `trsf2d` | var. | a `gp_Trsf2d` recipe, the kinds of the gp2d judge: 2 `pnt2d f` (`SetScale`); 3 `xy` (`SetTranslation`); 4 `pnt2d pnt2d` (`SetTranslation(p, q)`); 5 `pnt2d` (`SetMirror`); 6 `pnt2d dir2d` (`SetMirror(Ax2d)`); 7 `pnt2d dir2d` (`SetTransformation(Ax2d)`); 8 `pnt2d dir2d pnt2d dir2d` (`SetTransformation(from, to)`); 9 `pnt2d dir2d f xy` (mirror, `SetScaleFactor`, `SetTranslationPart`); 10 six floats (`SetValues`); 12 identity |
| `rg` | 3 | `Bnd_Range` recipe `kind a b`: 0 `Bnd_Range()`; 1 `Bnd_Range(a, b)` (the line is `throws` when `b < a`); 2 `Add(a)`, `Add(b)` on a void range; 3 `Add(a)` |
| `sph` | 7 | `cx cy cz radius u v valid`: `Bnd_Sphere(XYZ, r, u, v)` then `SetValid(valid)` |
| `pts` `tols` | var. | `pts` is `lo n x1 y1 z1 .. xn yn zn` (an `NCollection_Array1<gp_Pnt>` with lower index `lo`); `tols` is `flag [t1 .. tn]` (1: an array of `n` doubles with the same bounds) |
| `obb` | var. | `kind payload aabox`: 0 `Bnd_OBB()`; 1 `pnt dir dir dir f f f` (constructor with raw directions); 2 `bx` (`Bnd_OBB(Bnd_Box)`); 3 `pts tols optimal` (`ReBuild` on a default OBB); then `aabox` is -1 (nothing) or `SetAABox(aabox)` |
| `bsb` | var. | `mode lo n` then: mode 0 `n` x `bx` (`Initialize(array)` with lower index `lo`); 1 `bx` then `n` x `bx` (`Initialize(enclosing, array)`); 2 `bx m` then `m` x `index bx` (`Initialize(enclosing, n)` then `Add(box, index)` in this order); 3 a generator (`Initialize(array)`); 4 `bx` + generator (`Initialize(enclosing, array)`); 5 `bx` + generator (`Initialize(enclosing, n)` then `Add(box_i, i + 1)`). A generator is `seed n lo hi size gmode` (below) |
| `a2` | var. | `rlo clo nr nc` then `nr * nc` vectors row-major: `NCollection_Array2<gp_Vec>(rlo, rlo+nr-1, clo, clo+nc-1)` |
| `c2d` | var. | `kind` (0 default constructor) then for kind > 0: `lo` (kind 1 only) `n` `n` points `tolu tolv umin vmin umax vmax`; kind 1 `NCollection_Array1`, 2 `NCollection_Sequence`, 3 `NCollection_DynamicArray` |
| `la` | var. | `lo n v1 .. vn`: an `NCollection_Array1<double>` with lower index `lo` |

**Box generator** (the generated box sets of `bsb` modes 3-5), identically in `oracle/bnd.cpp` and `tests/bnd.rs`:
`state = 0x9E3779B97F4A7C15 ^ (seed as u64).wrapping_mul(0xD1B54A32D192ED03)` (1 if 0); `r(lo, hi)`: `state ^= state << 13; state ^= state >> 7; state ^= state << 17;
lo + (hi - lo) * ((state >> 11) as f64) / 9007199254740992.0`. For `i in 0..n`: `x, y, z = r(lo, hi)` (in this order), `sx, sy, sz = r(0, size)`; the box is void when
`gmode == 1 && i % 5 == 4`, otherwise `Update(x, y, z, x + sx, y + sy, z + sz)`, with `SetGap(0.1 * sx)` when `gmode == 1 && i % 7 == 3`.
The generated sets reach 150, 1200, 10500 and 40100 boxes (the resolutions 16, 32, 64, 128).

**Families** (lines):

* `Bnd_Box`: `box_add_box` (48), `box_add_dir` (34), `box_add_pnt` (34), `box_add_pnt_dir` (34), `box_corners` (34), `box_distance` (34), `box_dump` (34), `box_finite_part` (34), `box_gap` (34), `box_get` (34), `box_is_out_box` (58), `box_is_out_box_trsf` (34), `box_is_out_lin` (58), `box_is_out_pln` (48), `box_is_out_pnt` (48), `box_is_out_segment` (58), `box_is_out_trsf_box_trsf` (34), `box_modify` (34), `box_set_pnt` (34), `box_set_pnt_dir` (34), `box_state` (34), `box_thin` (34), `box_transformed` (48), `box_update3` (34), `box_update6` (34)
* `Bnd_Box2d`: `box2d_add_box` (48), `box2d_add_dir` (34), `box2d_add_pnt` (34), `box2d_add_pnt_dir` (34), `box2d_distance` (34), `box2d_dump` (34), `box2d_gap` (34), `box2d_get` (34), `box2d_is_out_box` (58), `box2d_is_out_box_trsf` (34), `box2d_is_out_lin` (58), `box2d_is_out_pnt` (48), `box2d_is_out_segment` (58), `box2d_is_out_trsf_box_trsf` (34), `box2d_modify` (34), `box2d_set_pnt` (34), `box2d_set_pnt_dir` (34), `box2d_state` (34), `box2d_transformed` (48), `box2d_update2` (34), `box2d_update4` (34)
* `Bnd_Range`: `range_add_param` (28), `range_add_range` (28), `range_common` (28), `range_enlarge` (28), `range_intermediate` (28), `range_is_intersected` (42), `range_is_out` (28), `range_is_out_range` (28), `range_set_void` (18), `range_shift` (28), `range_split` (42), `range_state` (28), `range_trim` (28), `range_union` (28)
* `Bnd_Sphere`: `sphere_add` (42), `sphere_default` (1), `sphere_distances` (28), `sphere_is_out_pnt` (42), `sphere_is_out_sphere` (28), `sphere_set_valid` (18), `sphere_state` (28)
* `Bnd_OBB`: `obb_add_obb` (22), `obb_add_pnt` (22), `obb_completely_inside` (22), `obb_ctor` (22), `obb_enlarge` (22), `obb_from_box` (22), `obb_is_out_obb` (38), `obb_is_out_pnt` (34), `obb_position` (22), `obb_rebuild` (34), `obb_rebuild_optimal` (22), `obb_rebuild_optimal_special` (34), `obb_rebuild_special` (38), `obb_setters` (22), `obb_state` (22), `obb_vertices` (22), `obb_void_aabox` (18)
* `Bnd_BoundSortBox`: `bsb_add` (22), `bsb_compare_box` (32), `bsb_compare_box_gen` (32), `bsb_compare_plane` (30), `bsb_compare_plane_gen` (16)
* `CSLib`: `class2d_default` (18), `class2d_internal` (36), `class2d_si_dans` (36), `class2d_si_dans_on_mode` (36), `cslib_dnnormal` (42), `cslib_dnnuv` (42), `cslib_dnnuv_two` (42), `cslib_enum_values` (1), `cslib_normal_mag_tol` (42), `cslib_normal_order` (110), `cslib_normal_poly_def` (42), `cslib_normal_second_order` (70), `cslib_normal_sin_tol` (42)

What the families check: each mutating method is replayed on recipe-built boxes and the **whole state** is printed (`void whole open`, the six open
flags, the gap, the six limits, `SquareExtent`, `HasFinitePart`); the `IsOut` families print the booleans of the overloads (and of `Contains` / `Intersects`);
`Dump` families the text; `range_*` the state after each operation, `Split` lists, `IsIntersected` codes; `obb_*` the full state (flags, center, three axes, half
sizes twice, `SquareExtent`), the vertices and `Position`; `bsb_*` the lists of `Compare` for three queries per line (the lists are limited to 400 entries); `cslib_*` the statuses, the
`written` flag and the normal, the orders, the vectors; `class2d_*` the integer results (`1`, `-1`, `0`) of four query points per line, including points on
vertices, on edges, a few ulps off, outside the bounds and on the bounds, three container kinds, empty polygons and tolerances 0 / tiny / large.
Edge cases: void, whole and open boxes (each side, five sides, all six through the flags), gaps (0, positive, negative -> `abs`), flat and point boxes, inverted corners, `1e10` and `1e99` limits;
translation / identity / general transformations for `Transformed` and the two `IsOut` forms; lines and planes along the axes, through corners and faces; segments with
every zero-component pattern of the direction; ranges that are void, flat, reversed (`throws`), shifted by periods; spheres with negative radius (uninitialised), coincident centres;
OBB point sets that are random, rotated, axis-aligned, on a sphere, collinear, coplanar (tilted and `z = const`), a single point, all equal, two points (also closer than `Precision::Confusion()`), cube corners, a 3x3x3 grid, an octahedron,
a tetrahedron, a rectangle, a line plus a point, centrally symmetric, lattices and equally spaced points, with and without tolerances, with `optimal` on and off, and up to 90 points for the BVH;
singular normals with every status code of the three simple overloads (`Done`, `D1uIsNull`, `D1vIsNull`, `D1IsNull`, `D1uIsParallelD1v`; `D1NuIsNull`, `D1NvIsNull`, `D1NIsNull`, `D1NuNvRatioIsNull`, `D1NvNuRatioIsNull`, `D1NuIsParallelD1Nv`,
`InfinityOfSolutions`; `Singular`, `Defined`) and of the derivative-order overload (orders found at 0 to 4, parallel derivatives that need the root search of `NormalPolyDef`, all-zero derivatives, `MaxOrder = -1`, the four boundary cases of `(U, V)` against the bounds).
`D1uD1vRatioIsNull` and `D1vD1uRatioIsNull` of `CSLib_DerivativeStatus` are never produced by `CSLib::Normal` in 8.0.1.

**Not covered**: `DumpJson` / `InitFromJson`; `Bnd_B2*`, `Bnd_B3*` templates; `Bnd_Tools`; `Bnd_OBB::Dump` and `Bnd_BoundSortBox::Dump` do not exist in 8.0.1; copying and assignment (the Rust types derive `Clone`);
`Bnd_BoundSortBox::Compare` before an `Initialize` (plane form crashes in C++); `Bnd_OBB::ReBuild` of an empty array (crashes in C++); the unchecked UB casts of `getBoundingVoxels` for huge values;
`Bnd_BoundSortBox::Initialize(enclosing, n)` with `n < 0` (crashes in C++).
