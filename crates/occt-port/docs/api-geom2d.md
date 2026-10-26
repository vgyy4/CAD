# `geom2d`: Rust API contract

This contract covers OCCT 8.0.1's 2D geometry package `Geom2d` (`cpp/Geom2d/`, toolkit TKG2d) and the parts of `Geom2dEval` that
it uses (`Geom2dEval_RepCurveDesc.hxx`, `Geom2dEval_RepUtils.pxx`); `cpp/GeomAbs/` gives the enumerations. The classes are:

* `Geom2d_Geometry` (`Mirror`, `Rotate`, `Scale`, `Translate`, `Transform`, `Mirrored`, `Rotated`, `Scaled`, `Translated`,
  `Transformed`, `Copy`), `Geom2d_Point` / `Geom2d_CartesianPoint`, `Geom2d_Vector` / `Geom2d_Direction` /
  `Geom2d_VectorWithMagnitude`, `Geom2d_AxisPlacement`, `Geom2d_Transformation` (every method);
* `Geom2d_Curve` (`Reverse`, `Reversed`, `ReversedParameter`, `TransformedParameter`, `ParametricTransformation`, `IsClosed`,
  `IsPeriodic`, `Period`, `Continuity`, `IsCN`, `D0`..`D3`, `DN`, `Value`, the `EvalD0`..`EvalDN` virtuals), `Geom2d_Line`,
  `Geom2d_Conic` with `Geom2d_Circle`, `Geom2d_Ellipse`, `Geom2d_Hyperbola`, `Geom2d_Parabola`, `Geom2d_BoundedCurve` with
  `Geom2d_BezierCurve`, `Geom2d_BSplineCurve` and `Geom2d_TrimmedCurve`, `Geom2d_OffsetCurve`;
* `Geom2dEval_RepCurveDesc` (`Full`, `DerivBounded`, `Mapped`, `Map1d`, `Domain1d`) and `Geom2dEval_RepUtils` (`CloneCurveDesc`,
  `TryEvalCurveD0`..`DN`, the `...Mapped` variants, `ValidateCurveDesc`), used by Bezier, B-spline and offset curves through
  `SetEvalRepresentation`. The other classes of `Geom2dEval` (spirals, sine wave, involute, T-Bezier) are not in scope: a
  representation curve is any `Curve2d` of this module.

The module is `occt_port::geom2d` (add `pub mod geom2d;` to `src/lib.rs`; `src/geom2d/` may have private submodules). Tests:
`tests/geom2d.rs` with the golden file `tests/geom2d.txt` printed by `oracle/geom2d.cpp` (real OCCT 8.0.1). Nothing that exists in
`src/` changes. The module reuses `gp` (2D: `Pnt2d`, `Vec2d`, `Dir2d`, `Ax2d`, `Ax22d`, `Trsf2d`, `Lin2d`, `Circ2d`, `Elips2d`,
`Hypr2d`, `Parab2d`, `TrsfForm`), `elclib` (the 2D functions `line2d_*`, `circle2d_*`, ...), `bsplclib`, `plib` (`GeomAbsShape`,
`coefficients_poles_2d`, `trimming_2d`, `set_poles_2d`, `get_poles_2d`) and `mathlin` (`MathError`): they are complete where they are
used and must be reused, not copied. See "Helpers the translation will need" below for what is missing.

## Rust design (the first OCCT class hierarchy; the same pattern is to be used for the other `Geom*` packages)

* **One struct per concrete class** (`CartesianPoint`, `Direction`, `VectorWithMagnitude`, `AxisPlacement`, `Line`, `Circle`,
  `Ellipse`, `Hyperbola`, `Parabola`, `BezierCurve`, `BSplineCurve`, `TrimmedCurve`, `OffsetCurve`; `Transformation` is a plain
  struct, it is not a `Geom2d_Geometry`). No `Geom2d_` prefix.
* **The virtual methods of the abstract classes become object-safe traits** implemented by the concrete structs. A trait
  hierarchy mirrors the class hierarchy: `Geometry2d` (`Geom2d_Geometry`), `Point2d: Geometry2d` (`Geom2d_Point`),
  `Vector2d: Geometry2d` (`Geom2d_Vector`), `Curve2d: Geometry2d` (`Geom2d_Curve`), `BoundedCurve2d: Curve2d`
  (`Geom2d_BoundedCurve`), `Conic2d: Curve2d` (`Geom2d_Conic`). **The C++ default bodies are trait default methods** wherever the C++
  base class implements the method (`Geom2d_Geometry::Mirror` ... `Translated`, `Geom2d_Curve::Reversed` / `TransformedParameter` /
  `ParametricTransformation` / `Period` / `Value`, `Geom2d_Point::Distance`, `Geom2d_Vector::Reverse` / `Angle` / `Coord` / `Dot`,
  `Geom2d_Conic::SetAxis` ... `Position`); a concrete class overrides the default exactly where the C++ class overrides it
  (`Line` and `Parabola` override `TransformedParameter` and `ParametricTransformation`; `TrimmedCurve` and `OffsetCurve`
  forward them to the basis curve; `TrimmedCurve` overrides `Period`). The protected data of a base class that the default bodies
  need is reached through required accessor methods (`Conic2d::pos` / `pos_mut`, `Vector2d::vec2d` / `set_vec2d_raw`).
* **Handles.** An OCCT `occ::handle<Geom2d_Curve>` held by another curve (`TrimmedCurve` / `OffsetCurve` basis, the representation
  of an evaluation descriptor) is `Arc<dyn Curve2d>`. A curve that is handed to a constructor is moved in (the test builds the basis
  with `Arc::from(boxed_curve)`). **Copy semantics follow OCCT:** `copy_geometry` / `copy_curve` is OCCT's `Copy()`:
  `TrimmedCurve::Copy` re-creates the trimmed curve over a *copy* of the basis (deep); `OffsetCurve::Copy` (the copy constructor)
  copies the basis curve and clones the evaluation representation (deep); `BezierCurve` / `BSplineCurve` copy the arrays and clone the
  representation descriptor *and its representation curve* (`CloneCurveDesc`); a `Direction` copy is
  `Direction::from_dir2d(Dir2d::from_vec(&v))` (OCCT's `new Geom2d_Direction(gpVec2d)` renormalizes). The constructors
  `TrimmedCurve::new` copy their basis (`C->Copy()`; a trimmed basis is replaced by a copy of *its* basis); `OffsetCurve::set_basis_curve`
  does not copy the basis (OCCT keeps the handle), it unwraps trimmed and offset bases exactly as the C++ loop does (adding the
  offsets, re-wrapping in a `TrimmedCurve` over a copy). Because an `Arc<dyn Curve2d>` is immutable the aliasing of the C++ handle is
  not observable and is not tested.
* **Mutating methods take `&mut self`**; the `...ed` / `Copy` methods take `&self` and return a new object. Stored values are
  passed by value (`Pnt2d`, `Dir2d`, `Ax2d`, `Ax22d`, `Circ2d`, ... are `Copy`), the others by reference, as in `docs/api-gp2d.md`.
  C++ out-parameters become returned tuples in argument order. Indices are `i32` and 1-based (as in OCCT).
* **Downcasting.** `Geometry2d` has `as_any` / `as_any_mut` (the test reads the state of a curve of unknown type through them, as
  `IsKind` / `DownCast` do in the oracle). Every concrete struct also implements `Debug + Clone`.
* **Names.** `snake_case` of the C++ method; an overload gets its own name when the argument types differ (`mirror_point` /
  `mirror_axis`, `translate` / `translate_points`, `set_pole` / `set_pole_weight`, `increase_multiplicity` /
  `increase_multiplicity_range`, `set_knot` / `set_knot_mult`, `local_value`, ...); constructors: see the listing. The `EvalD0`..`EvalDN`
  virtuals are `eval_d0`..`eval_dn`; the `D0`..`DN` convenience wrappers of `Geom2d_Curve` are not separate methods (the test uses
  `eval_*` and `value`).

## Conventions

* **Errors.** `geom2d::Geom2dError` (derive `Debug, Clone, PartialEq, Eq`; `From<gp::ConstructionError>`, `From<BSplError>`,
  `From<MathError>`, `From<PLibError>`) and `type Result<T> = std::result::Result<T, Geom2dError>`:

  | variant | stands for |
  |---|---|
  | `Construction` | `Standard_ConstructionError` raised by OCCT |
  | `OutOfRange` | `Standard_OutOfRange` raised by OCCT, and a Raise_if range check compiled out |
  | `Domain` | `Standard_DomainError` (raised, or a Raise_if compiled out) |
  | `NoSuchObject` | `Standard_NoSuchObject` (`Period` of a non periodic curve, `SetOrigin` of a non periodic B-spline) |
  | `Range` | `Standard_RangeError` (`IsCN` with `N < 0`: compiled out) |
  | `UndefinedValue` / `UndefinedDerivative` | `Geom2d_UndefinedValue` / `Geom2d_UndefinedDerivative` (offset curves at a singular point, `DN` with `N < 1`, ...) |
  | `Program` | `Standard_ProgramError` (`ValidateCurveDesc`) |
  | `Dimension` | `Standard_DimensionError` |
  | `Argument` | a wrong shape of a Rust argument, or an unchecked input of OCCT 8.0.1 as built (below) |
  | `Gp(..)`, `BSpl(..)`, `Math(..)`, `PLib(..)` | errors of the functions used inside |

  The test only checks `Err` against the oracle's `throws` / `unchecked`; any variant is accepted. **Not judged but required:**
  nothing may panic (the lints forbid indexing and `unwrap`): an index outside an array, a negative count, a short slice must
  return `Err`.
* **`throws` and `unchecked`.** `throws` lines are exceptions OCCT really raises: explicit `throw` statements of `Geom2d_*.cxx`
  (invalid constructor arguments, `SetTrim`, `SetKnot`, `SetOrigin`, `Segment`, `InsertKnots`, `RemoveKnot`, `MovePoint`,
  `InsertPoleAfter` ..., `EvalD0` .. `EvalDN` of an offset curve at a singular point, `DN` with `N < 1`, offset on a C0 curve,
  `ValidateCurveDesc`), the `gp` constructors that the oracle itself calls (`gp_Dir2d(0,0)`, `gp_Circ2d` with a negative radius ...),
  and `gp_Vec2d::Angle` with a null vector. **`unchecked`** lines are inputs for which a `Standard_*_Raise_if` check is compiled
  out of the OCCT libraries (`No_Exception`; this was verified with probes: `Geom2d_Direction(0,0)` returns NaN, `Period()` of a line
  returns `4e100`, the array accessors read outside the array). The oracle does not execute such a call; the Rust API must return
  `Err` for the same condition. The complete list:
  * `Direction::new` / `set_coord` with a vector of length `<= gp::resolution()`; `set_x` / `set_y` when the resulting vector has
    such a length; `VectorWithMagnitude::divide` / `divided` with `|s| <= gp::resolution()`; `normalize` / `normalized` of a vector of
    magnitude `<= gp::resolution()`;
  * `Transformation::value` with a row outside `1..=2` or a column outside `1..=3`; `invert` / `inverted` and `power` /
    `powered` with `n < 0` when the form is not `Identity`, `Translation` or `PntMirror` and `|scale_factor| <= gp::resolution()`
    (`Trsf2d::set_values` of a recipe with `|det| < gp::resolution()` is the same kind of case inside `gp`);
  * `Curve2d::period` when the curve is not periodic (for `TrimmedCurve` and `OffsetCurve`: when the basis curve is not periodic;
    note that a trimmed curve over a periodic basis answers `Ok` even if the trim is not a whole period);
    `is_cn(n)` with `n < 0` for `BSplineCurve`, `TrimmedCurve`, `OffsetCurve`;
  * `Hyperbola::eccentricity`, `parameter`, `asymptote1/2`, `directrix1/2` when `major_radius <= gp::resolution()`;
    `Ellipse::directrix1/2` when the eccentricity is `<= gp::resolution()` (a circle);
  * `BezierCurve`: `increase(d)` when `d != degree` and (`d < degree` or `d > max_degree()`); `insert_pole_after(i)` with `i`
    outside `0..=n`, `insert_pole_before(i)` with `i - 1` outside `0..=n`, a weight `<= gp::resolution()`, or `n >= max_degree()`
    (25); `remove_pole(i)` with `n <= 2` or `i` outside `1..=n`; `set_pole` / `set_pole_weight` / `set_weight` / `pole` / `weight`
    with `i` outside `1..=n`, and a weight `<= gp::resolution()`;
  * `BSplineCurve`: `increase_multiplicity(i, m)` and `set_knot_mult(i, k, m)` with `i` outside `1..=nb_knots`;
    `increase_multiplicity_range(i1, i2, m)` and `increment_multiplicity(i1, i2, m)` with `i1 < 1`, `i2 > nb_knots` or `i1 > i2`;
    `knot(i)` / `multiplicity(i)` with `i` outside `1..=nb_knots`; `pole(i)` / `weight(i)` with `i` outside `1..=nb_poles`;
    `local_value` / `local_d0..d3` / `local_dn(u, k1, k2)` with `k1 < 1`, `k2 > nb_knots` or `k1 >= k2`.
  Everything else that OCCT 8.0.1 does not check and that the oracle executes (for example `Segment` with `u2 < u1` on a Bezier curve,
  `LocateU` outside the range, `move_point_and_tangent` with any conditions) is deterministic and is reproduced as OCCT computes it.
* **Arithmetic.** Operations are done in the order of the C++ source, with no fused multiply-add and no reassociation. `sin`, `cos`,
  `acos`, `atan2`, `exp`, `log`, `pow` are `libm::` (through `elclib`, `gp`). The families marked EXACT below only use `+ - * /`,
  `sqrt`, `abs`, `floor` / `ceil` and comparisons (no C library function): they are compared bit for bit, signed zeros
  included. All the others depend on the C library and are compared within 8 ulps of the largest absolute value of the line (at
  least 1; zero tolerance if that value is infinite); the oracle keeps only cases whose outputs stay within 2 ulps when every
  C-library result (`sin cos tan sinh cosh asin acos atan atan2 exp log pow`) is moved by an ulp in 24 patterns. Integers,
  booleans and the number of values are always compared exactly. `Epsilon(x)` of OCCT is the distance from `x` to the next
  representable `f64` away from zero (`std::nextafter`; `Epsilon(0.0)` is the smallest positive subnormal); `Precision::Infinite()` is `2e100`, `Precision::Confusion()` is `1e-7`,
  `Precision::PConfusion()` is `Confusion() / 100.0` (compute it that way: the literal `1e-9` may differ in the last bit),
  `Precision::Computational()` is `f64::EPSILON`, `Precision::Angular()` is `1e-12`; `Precision::IsInfinite(u)` is `|u| >= 2e100`.
* **Enumerations.** `geom2d::GeomAbsShape` is a re-export of the existing `plib::GeomAbsShape` (`C0 G1 C1 G2 C2 C3 Cn`, discriminants
  0..6 as `GeomAbs_Shape`). `geom2d::BSplKnotDistribution` is new: `NonUniform = 0, Uniform = 1, QuasiUniform = 2, PiecewiseBezier = 3`
  (`#[repr(i32)]`). `gp::TrsfForm` already exists.
* **Result records.** `ResD1 { point, d1 }`, `ResD2 { point, d1, d2 }`, `ResD3 { point, d1, d2, d3 }` (`Geom2d_Curve::ResD1` ...),
  public fields, `Debug + Clone + Copy + PartialEq`.

## Signature listing

This is the complete public surface the test uses. The bodies are the translation's job; the C++ class and method of each item is
the snake_case name (listed in the table after the code). Every concrete struct implements `Geometry2d` (`as_any`, `as_any_mut`,
`transform`, `copy_geometry`) and derives `Debug` and `Clone`; the curves implement `Curve2d` (and `BoundedCurve2d` / `Conic2d` as
below). `Geometry2d::mirror_point` ... `translated_points` have default bodies (the C++ `Geom2d_Geometry` code: build the `Trsf2d`, call
`transform`; the `...ed` forms `copy_geometry` first). The `as_any` methods are trivial.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Geom2dError {
    Construction,
    OutOfRange,
    Domain,
    NoSuchObject,
    Range,
    UndefinedValue,
    UndefinedDerivative,
    Program,
    Dimension,
    Argument,
    Gp(gp::ConstructionError),
    BSpl(BSplError),
    Math(MathError),
    PLib(PLibError),
}
pub type Result<T> = std::result::Result<T, Geom2dError>;

pub use crate::plib::GeomAbsShape;
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BSplKnotDistribution {
    NonUniform = 0,
    Uniform = 1,
    QuasiUniform = 2,
    PiecewiseBezier = 3,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD1 {
    pub point: Pnt2d,
    pub d1: Vec2d,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD2 {
    pub point: Pnt2d,
    pub d1: Vec2d,
    pub d2: Vec2d,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD3 {
    pub point: Pnt2d,
    pub d1: Vec2d,
    pub d2: Vec2d,
    pub d3: Vec2d,
}

pub trait Geometry2d: Any + std::fmt::Debug {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn transform(&mut self, t: &Trsf2d);
    fn copy_geometry(&self) -> Box<dyn Geometry2d>;
    fn mirror_point(&mut self, p: &Pnt2d) ;
    fn mirror_axis(&mut self, a: &Ax2d) ;
    fn rotate(&mut self, p: &Pnt2d, ang: f64) ;
    fn scale(&mut self, p: &Pnt2d, s: f64) ;
    fn translate(&mut self, v: &Vec2d) ;
    fn translate_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) ;
    fn mirrored_point(&self, p: &Pnt2d) -> Box<dyn Geometry2d> ;
    fn mirrored_axis(&self, a: &Ax2d) -> Box<dyn Geometry2d> ;
    fn rotated(&self, p: &Pnt2d, ang: f64) -> Box<dyn Geometry2d> ;
    fn scaled(&self, p: &Pnt2d, s: f64) -> Box<dyn Geometry2d> ;
    fn transformed(&self, t: &Trsf2d) -> Box<dyn Geometry2d> ;
    fn translated(&self, v: &Vec2d) -> Box<dyn Geometry2d> ;
    fn translated_points(&self, p1: &Pnt2d, p2: &Pnt2d) -> Box<dyn Geometry2d> ;
}

pub trait Point2d: Geometry2d {
    fn coord(&self) -> (f64, f64);
    fn pnt2d(&self) -> Pnt2d;
    fn x(&self) -> f64;
    fn y(&self) -> f64;
    fn distance(&self, other: &dyn Point2d) -> f64 ;
    fn square_distance(&self, other: &dyn Point2d) -> f64 ;
}
#[derive(Debug, Clone)]
pub struct CartesianPoint {
    // private fields
}
impl CartesianPoint {
    pub fn from_pnt2d(p: Pnt2d) -> Self ;
    pub fn new(x: f64, y: f64) -> Self ;
    pub fn set_coord(&mut self, x: f64, y: f64) ;
    pub fn set_pnt2d(&mut self, p: Pnt2d) ;
    pub fn set_x(&mut self, x: f64) ;
    pub fn set_y(&mut self, y: f64) ;
}

pub trait Vector2d: Geometry2d {
    fn vec2d(&self) -> Vec2d;
    fn set_vec2d_raw(&mut self, v: Vec2d);
    fn copy_vector(&self) -> Box<dyn Vector2d>;
    fn magnitude(&self) -> f64;
    fn square_magnitude(&self) -> f64;
    fn crossed(&self, other: &dyn Vector2d) -> f64;
    fn reverse(&mut self) ;
    fn reversed(&self) -> Box<dyn Vector2d> ;
    fn angle(&self, other: &dyn Vector2d) -> Result<f64> ;
    fn coord(&self) -> (f64, f64) ;
    fn x(&self) -> f64 ;
    fn y(&self) -> f64 ;
    fn dot(&self, other: &dyn Vector2d) -> f64 ;
}
#[derive(Debug, Clone)]
pub struct Direction {
    // private fields
}
impl Direction {
    pub fn new(x: f64, y: f64) -> Result<Self> ;
    pub fn from_dir2d(d: Dir2d) -> Self ;
    pub fn set_coord(&mut self, x: f64, y: f64) -> Result<()> ;
    pub fn set_dir2d(&mut self, d: Dir2d) ;
    pub fn set_x(&mut self, x: f64) -> Result<()> ;
    pub fn set_y(&mut self, y: f64) -> Result<()> ;
    pub fn dir2d(&self) -> Dir2d ;
}
#[derive(Debug, Clone)]
pub struct VectorWithMagnitude {
    // private fields
}
impl VectorWithMagnitude {
    pub fn from_vec2d(v: Vec2d) -> Self ;
    pub fn new(x: f64, y: f64) -> Self ;
    pub fn from_points(p1: Pnt2d, p2: Pnt2d) -> Self ;
    pub fn set_coord(&mut self, x: f64, y: f64) ;
    pub fn set_vec2d(&mut self, v: Vec2d) ;
    pub fn set_x(&mut self, x: f64) ;
    pub fn set_y(&mut self, y: f64) ;
    pub fn add(&mut self, other: &dyn Vector2d) ;
    pub fn added(&self, other: &dyn Vector2d) -> VectorWithMagnitude ;
    pub fn subtract(&mut self, other: &dyn Vector2d) ;
    pub fn subtracted(&self, other: &dyn Vector2d) -> VectorWithMagnitude ;
    pub fn multiply(&mut self, s: f64) ;
    pub fn multiplied(&self, s: f64) -> VectorWithMagnitude ;
    pub fn divide(&mut self, s: f64) -> Result<()> ;
    pub fn divided(&self, s: f64) -> Result<VectorWithMagnitude> ;
    pub fn normalize(&mut self) -> Result<()> ;
    pub fn normalized(&self) -> Result<VectorWithMagnitude> ;
}
#[derive(Debug, Clone)]
pub struct AxisPlacement {
    // private fields
}
impl AxisPlacement {
    pub fn new(a: Ax2d) -> Self ;
    pub fn from_point_direction(p: Pnt2d, v: Dir2d) -> Self ;
    pub fn reverse(&mut self) ;
    pub fn reversed(&self) -> AxisPlacement ;
    pub fn set_axis(&mut self, a: Ax2d) ;
    pub fn set_direction(&mut self, v: Dir2d) ;
    pub fn set_location(&mut self, p: Pnt2d) ;
    pub fn angle(&self, other: &AxisPlacement) -> f64 ;
    pub fn ax2d(&self) -> Ax2d ;
    pub fn direction(&self) -> Dir2d ;
    pub fn location(&self) -> Pnt2d ;
}

#[derive(Debug, Clone)]
pub struct Transformation {
    // private fields
}
impl Transformation {
    pub fn new() -> Self ;
    pub fn from_trsf2d(t: &Trsf2d) -> Self ;
    pub fn set_mirror_point(&mut self, p: &Pnt2d) ;
    pub fn set_mirror_axis(&mut self, a: &Ax2d) ;
    pub fn set_rotation(&mut self, p: &Pnt2d, ang: f64) ;
    pub fn set_scale(&mut self, p: &Pnt2d, s: f64) ;
    pub fn set_transformation_between(&mut self, from: &Ax2d, to: &Ax2d) ;
    pub fn set_transformation(&mut self, to: &Ax2d) ;
    pub fn set_translation(&mut self, v: &Vec2d) ;
    pub fn set_translation_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) ;
    pub fn set_trsf2d(&mut self, t: &Trsf2d) ;
    pub fn is_negative(&self) -> bool ;
    pub fn form(&self) -> TrsfForm ;
    pub fn scale_factor(&self) -> f64 ;
    pub fn trsf2d(&self) -> Trsf2d ;
    pub fn value(&self, row: i32, col: i32) -> Result<f64> ;
    pub fn invert(&mut self) -> Result<()> ;
    pub fn inverted(&self) -> Result<Transformation> ;
    pub fn multiplied(&self, other: &Transformation) -> Transformation ;
    pub fn multiply(&mut self, other: &Transformation) ;
    pub fn power(&mut self, n: i32) -> Result<()> ;
    pub fn powered(&self, n: i32) -> Result<Transformation> ;
    pub fn pre_multiply(&mut self, other: &Transformation) ;
    pub fn transforms(&self, x: f64, y: f64) -> (f64, f64) ;
    pub fn copy(&self) -> Transformation ;
}
impl Default for Transformation;
impl std::ops::Mul<&Transformation> for &Transformation { type Output = Transformation; }
impl std::ops::MulAssign<&Transformation> for Transformation {}

pub trait Curve2d: Geometry2d {
    fn copy_curve(&self) -> Box<dyn Curve2d>;
    fn reverse(&mut self);
    fn reversed_parameter(&self, u: f64) -> f64;
    fn transformed_parameter(&self, u: f64, t: &Trsf2d) -> f64 ;
    fn parametric_transformation(&self, t: &Trsf2d) -> f64 ;
    fn reversed(&self) -> Box<dyn Curve2d> ;
    fn first_parameter(&self) -> f64;
    fn last_parameter(&self) -> f64;
    fn is_closed(&self) -> bool;
    fn is_periodic(&self) -> bool;
    fn period(&self) -> Result<f64> ;
    fn continuity(&self) -> GeomAbsShape;
    fn is_cn(&self, n: i32) -> Result<bool>;
    fn eval_d0(&self, u: f64) -> Result<Pnt2d>;
    fn eval_d1(&self, u: f64) -> Result<ResD1>;
    fn eval_d2(&self, u: f64) -> Result<ResD2>;
    fn eval_d3(&self, u: f64) -> Result<ResD3>;
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d>;
    fn value(&self, u: f64) -> Result<Pnt2d> ;
}
pub trait BoundedCurve2d: Curve2d {
    fn start_point(&self) -> Pnt2d;
    fn end_point(&self) -> Pnt2d;
}
pub trait Conic2d: Curve2d {
    fn pos(&self) -> &Ax22d;
    fn pos_mut(&mut self) -> &mut Ax22d;
    fn eccentricity(&self) -> Result<f64>;
    fn set_axis(&mut self, a: Ax22d) ;
    fn set_x_axis(&mut self, a: Ax2d) ;
    fn set_y_axis(&mut self, a: Ax2d) ;
    fn set_location(&mut self, p: Pnt2d) ;
    fn x_axis(&self) -> Ax2d ;
    fn y_axis(&self) -> Ax2d ;
    fn location(&self) -> Pnt2d ;
    fn position(&self) -> &Ax22d ;
}

/// An evaluation representation (Geom2dEval_RepCurveDesc::Full, ::DerivBounded or ::Mapped).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalRepKind {
    Full = 0,
    DerivBounded = 1,
    Mapped = 2,
}
#[derive(Debug, Clone)]
pub struct EvalRepDesc {
    pub kind: EvalRepKind,
    pub representation: Option<Arc<dyn Curve2d>>,
    pub max_deriv_order: usize,
    pub domain: Option<(f64, f64)>,
    pub scale: f64,
    pub offset: f64,
}

#[derive(Debug, Clone)]
pub struct Line {
    // private fields
}
impl Line {
    pub fn from_ax2d(a: Ax2d) -> Self ;
    pub fn from_lin2d(l: Lin2d) -> Self ;
    pub fn new(p: Pnt2d, v: Dir2d) -> Self ;
    pub fn set_lin2d(&mut self, l: Lin2d) ;
    pub fn set_direction(&mut self, v: Dir2d) ;
    pub fn direction(&self) -> &Dir2d ;
    pub fn set_location(&mut self, p: Pnt2d) ;
    pub fn location(&self) -> &Pnt2d ;
    pub fn set_position(&mut self, a: Ax2d) ;
    pub fn position(&self) -> &Ax2d ;
    pub fn lin2d(&self) -> Lin2d ;
    pub fn distance(&self, p: &Pnt2d) -> f64 ;
}

#[derive(Debug, Clone)]
pub struct Circle {
    // private fields
}
impl Circle {
    pub fn from_circ2d(c: Circ2d) -> Self ;
    pub fn new(a: Ax2d, radius: f64, sense: bool) -> Result<Self> ;
    pub fn from_ax22d(a: Ax22d, radius: f64) -> Result<Self> ;
    pub fn set_circ2d(&mut self, c: Circ2d) ;
    pub fn set_radius(&mut self, r: f64) -> Result<()> ;
    pub fn circ2d(&self) -> Circ2d ;
    pub fn radius(&self) -> f64 ;
}

#[derive(Debug, Clone)]
pub struct Ellipse {
    // private fields
}
impl Ellipse {
    pub fn from_elips2d(e: Elips2d) -> Self ;
    pub fn new(major_axis: Ax2d, major: f64, minor: f64, sense: bool) -> Result<Self> ;
    pub fn from_ax22d(a: Ax22d, major: f64, minor: f64) -> Result<Self> ;
    pub fn set_elips2d(&mut self, e: Elips2d) ;
    pub fn set_major_radius(&mut self, r: f64) -> Result<()> ;
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()> ;
    pub fn elips2d(&self) -> Elips2d ;
    pub fn directrix1(&self) -> Result<Ax2d> ;
    pub fn directrix2(&self) -> Result<Ax2d> ;
    pub fn focal(&self) -> f64 ;
    pub fn focus1(&self) -> Pnt2d ;
    pub fn focus2(&self) -> Pnt2d ;
    pub fn major_radius(&self) -> f64 ;
    pub fn minor_radius(&self) -> f64 ;
    pub fn parameter(&self) -> f64 ;
}

#[derive(Debug, Clone)]
pub struct Hyperbola {
    // private fields
}
impl Hyperbola {
    pub fn from_hypr2d(h: Hypr2d) -> Self ;
    pub fn new(major_axis: Ax2d, major: f64, minor: f64, sense: bool) -> Result<Self> ;
    pub fn from_ax22d(a: Ax22d, major: f64, minor: f64) -> Result<Self> ;
    pub fn set_hypr2d(&mut self, h: Hypr2d) ;
    pub fn set_major_radius(&mut self, r: f64) -> Result<()> ;
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()> ;
    pub fn hypr2d(&self) -> Hypr2d ;
    pub fn asymptote1(&self) -> Result<Ax2d> ;
    pub fn asymptote2(&self) -> Result<Ax2d> ;
    pub fn conjugate_branch1(&self) -> Hypr2d ;
    pub fn conjugate_branch2(&self) -> Hypr2d ;
    pub fn directrix1(&self) -> Result<Ax2d> ;
    pub fn directrix2(&self) -> Result<Ax2d> ;
    pub fn focal(&self) -> f64 ;
    pub fn focus1(&self) -> Pnt2d ;
    pub fn focus2(&self) -> Pnt2d ;
    pub fn major_radius(&self) -> f64 ;
    pub fn minor_radius(&self) -> f64 ;
    pub fn other_branch(&self) -> Hypr2d ;
    pub fn parameter(&self) -> Result<f64> ;
}

#[derive(Debug, Clone)]
pub struct Parabola {
    // private fields
}
impl Parabola {
    pub fn from_parab2d(p: Parab2d) -> Self ;
    pub fn new(mirror_axis: Ax2d, focal: f64, sense: bool) -> Result<Self> ;
    pub fn from_ax22d(a: Ax22d, focal: f64) -> Result<Self> ;
    pub fn from_directrix_focus(d: Ax2d, f: Pnt2d) -> Self ;
    pub fn set_focal(&mut self, f: f64) -> Result<()> ;
    pub fn set_parab2d(&mut self, p: Parab2d) ;
    pub fn parab2d(&self) -> Parab2d ;
    pub fn directrix(&self) -> Ax2d ;
    pub fn focus(&self) -> Pnt2d ;
    pub fn focal(&self) -> f64 ;
    pub fn parameter(&self) -> f64 ;
}

#[derive(Debug, Clone)]
pub struct BezierCurve {
    // private fields
}
impl BezierCurve {
    pub fn new(poles: &[Pnt2d]) -> Result<Self> ;
    pub fn with_weights(poles: &[Pnt2d], weights: &[f64]) -> Result<Self> ;
    pub fn has_eval_representation(&self) -> bool ;
    pub fn eval_representation(&self) -> Option<&EvalRepDesc> ;
    pub fn set_eval_representation(&mut self, d: Option<EvalRepDesc>) -> Result<()> ;
    pub fn clear_eval_representation(&mut self) ;
    pub fn increase(&mut self, degree: i32) -> Result<()> ;
    pub fn insert_pole_after(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> ;
    pub fn insert_pole_before(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> ;
    pub fn remove_pole(&mut self, index: i32) -> Result<()> ;
    pub fn segment(&mut self, u1: f64, u2: f64) ;
    pub fn set_pole(&mut self, index: i32, p: Pnt2d) -> Result<()> ;
    pub fn set_pole_weight(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> ;
    pub fn set_weight(&mut self, index: i32, weight: f64) -> Result<()> ;
    pub fn is_rational(&self) -> bool ;
    pub fn degree(&self) -> i32 ;
    pub fn nb_poles(&self) -> i32 ;
    pub fn pole(&self, index: i32) -> Result<Pnt2d> ;
    pub fn poles(&self) -> &[Pnt2d] ;
    pub fn weight(&self, index: i32) -> Result<f64> ;
    pub fn weights(&self) -> Option<&[f64]> ;
    pub fn knots(&self) -> Vec<f64> ;
    pub fn multiplicities(&self) -> Vec<i32> ;
    pub fn knot_sequence(&self) -> Vec<f64> ;
    pub fn max_degree() -> i32 ;
    pub fn resolution(&mut self, tolerance_uv: f64) -> f64 ;
}

#[derive(Debug, Clone)]
pub struct BSplineCurve {
    // private fields
}
impl BSplineCurve {
    pub fn new(poles: &[Pnt2d], knots: &[f64], mults: &[i32], degree: i32, periodic: bool) -> Result<Self> ;
    pub fn with_weights(
        poles: &[Pnt2d],
        weights: &[f64],
        knots: &[f64],
        mults: &[i32],
        degree: i32,
        periodic: bool,
    ) -> Result<Self> ;
    pub fn has_eval_representation(&self) -> bool ;
    pub fn eval_representation(&self) -> Option<&EvalRepDesc> ;
    pub fn set_eval_representation(&mut self, d: Option<EvalRepDesc>) -> Result<()> ;
    pub fn clear_eval_representation(&mut self) ;
    pub fn increase_degree(&mut self, degree: i32) -> Result<()> ;
    pub fn increase_multiplicity(&mut self, index: i32, m: i32) -> Result<()> ;
    pub fn increase_multiplicity_range(&mut self, i1: i32, i2: i32, m: i32) -> Result<()> ;
    pub fn increment_multiplicity(&mut self, i1: i32, i2: i32, m: i32) -> Result<()> ;
    pub fn insert_knot(&mut self, u: f64, m: i32, parametric_tolerance: f64) -> Result<()> ;
    pub fn insert_knots(&mut self, knots: &[f64], mults: &[i32], parametric_tolerance: f64, add: bool) -> Result<()> ;
    pub fn remove_knot(&mut self, index: i32, m: i32, tolerance: f64) -> Result<bool> ;
    pub fn insert_pole_after(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> ;
    pub fn insert_pole_before(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> ;
    pub fn remove_pole(&mut self, index: i32) -> Result<()> ;
    pub fn segment(&mut self, u1: f64, u2: f64, tolerance: f64) -> Result<()> ;
    pub fn set_knot(&mut self, index: i32, k: f64) -> Result<()> ;
    pub fn set_knot_mult(&mut self, index: i32, k: f64, m: i32) -> Result<()> ;
    pub fn set_knots(&mut self, k: &[f64]) -> Result<()> ;
    pub fn periodic_normalization(&self, u: f64) -> f64 ;
    pub fn set_periodic(&mut self) ;
    pub fn set_origin(&mut self, index: i32) -> Result<()> ;
    pub fn set_not_periodic(&mut self) ;
    pub fn set_pole(&mut self, index: i32, p: Pnt2d) -> Result<()> ;
    pub fn set_pole_weight(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> ;
    pub fn set_weight(&mut self, index: i32, weight: f64) -> Result<()> ;
    pub fn move_point(&mut self, u: f64, p: Pnt2d, index1: i32, index2: i32) -> Result<(i32, i32)> ;
    pub fn move_point_and_tangent(
        &mut self,
        u: f64,
        p: Pnt2d,
        tangent: Vec2d,
        tolerance: f64,
        starting_condition: i32,
        ending_condition: i32,
    ) -> Result<i32> ;
    pub fn is_g1(&self, tf: f64, tl: f64, ang_tol: f64) -> Result<bool> ;
    pub fn is_rational(&self) -> bool ;
    pub fn degree(&self) -> i32 ;
    pub fn local_value(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<Pnt2d> ;
    pub fn local_d0(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<Pnt2d> ;
    pub fn local_d1(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<(Pnt2d, Vec2d)> ;
    pub fn local_d2(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<(Pnt2d, Vec2d, Vec2d)> ;
    pub fn local_d3(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> ;
    pub fn local_dn(&self, u: f64, from_k1: i32, to_k2: i32, n: i32) -> Result<Vec2d> ;
    pub fn first_u_knot_index(&self) -> i32 ;
    pub fn knot(&self, index: i32) -> Result<f64> ;
    pub fn knots(&self) -> &[f64] ;
    pub fn knot_sequence(&self) -> &[f64] ;
    pub fn knot_distribution(&self) -> BSplKnotDistribution ;
    pub fn last_u_knot_index(&self) -> i32 ;
    pub fn locate_u(&self, u: f64, parametric_tolerance: f64, with_knot_repetition: bool) -> (i32, i32) ;
    pub fn multiplicity(&self, index: i32) -> Result<i32> ;
    pub fn multiplicities(&self) -> &[i32] ;
    pub fn nb_knots(&self) -> i32 ;
    pub fn nb_poles(&self) -> i32 ;
    pub fn pole(&self, index: i32) -> Result<Pnt2d> ;
    pub fn poles(&self) -> &[Pnt2d] ;
    pub fn weight(&self, index: i32) -> Result<f64> ;
    pub fn weights(&self) -> Option<&[f64]> ;
    pub fn max_degree() -> i32 ;
    pub fn resolution(&mut self, tolerance_uv: f64) -> f64 ;
}

#[derive(Debug, Clone)]
pub struct TrimmedCurve {
    // private fields
}
impl TrimmedCurve {
    pub fn new(basis: Arc<dyn Curve2d>, u1: f64, u2: f64, sense: bool, adjust_periodic: bool) -> Result<Self> ;
    pub fn set_trim(&mut self, u1: f64, u2: f64, sense: bool, adjust_periodic: bool) -> Result<()> ;
    pub fn basis_curve(&self) -> Arc<dyn Curve2d> ;
}

#[derive(Debug, Clone)]
pub struct OffsetCurve {
    // private fields
}
impl OffsetCurve {
    pub fn new(basis: Arc<dyn Curve2d>, offset: f64, not_check_c0: bool) -> Result<Self> ;
    pub fn has_eval_representation(&self) -> bool ;
    pub fn eval_representation(&self) -> Option<&EvalRepDesc> ;
    pub fn set_eval_representation(&mut self, d: Option<EvalRepDesc>) -> Result<()> ;
    pub fn clear_eval_representation(&mut self) ;
    pub fn set_basis_curve(&mut self, c: Arc<dyn Curve2d>, not_check_c0: bool) -> Result<()> ;
    pub fn set_offset_value(&mut self, d: f64) ;
    pub fn basis_curve(&self) -> Arc<dyn Curve2d> ;
    pub fn offset(&self) -> f64 ;
    pub fn get_basis_curve_continuity(&self) -> GeomAbsShape ;
}

```

| item | C++ |
|---|---|
| `CartesianPoint` : `Geometry2d`, `Point2d` | `Geom2d_CartesianPoint` (`from_pnt2d` = `Geom2d_CartesianPoint(gp_Pnt2d)`, `new(x, y)`) |
| `Direction` : `Geometry2d`, `Vector2d` | `Geom2d_Direction`; `new(x, y)` is the `(X, Y)` constructor (normalizes), `from_dir2d` the `gp_Dir2d` one |
| `VectorWithMagnitude` : `Geometry2d`, `Vector2d` | `Geom2d_VectorWithMagnitude`; `from_vec2d`, `new(x, y)`, `from_points(p1, p2)` are its three constructors; `added(&other)` is `Other + this` (C++ `Temp = Other; Temp.Add(gpVec2d)`), `subtracted` is `this - Other` |
| `AxisPlacement` : `Geometry2d` | `Geom2d_AxisPlacement` (`new(Ax2d)`, `from_point_direction(P, V)`) |
| `Transformation` | `Geom2d_Transformation`; `new()` = default constructor (identity), `from_trsf2d`; `Mul` is `operator*`, `MulAssign` is `operator*=`; `multiplied(&o)` = `this * o`, `pre_multiply(&o)` = `this = o * this`, `power(n)` = `gp_Trsf2d::Power`, `copy()` = `Copy()` |
| `Line` : `Curve2d` | `Geom2d_Line`; constructors `from_ax2d(Ax2d)`, `from_lin2d(Lin2d)`, `new(P, V)` |
| `Circle`, `Ellipse`, `Hyperbola`, `Parabola` : `Curve2d`, `Conic2d` | the conics. Constructors: `from_circ2d` / `from_elips2d` / `from_hypr2d` / `from_parab2d` take the gp object, `new(axis, radius..., sense)` is the `(gp_Ax2d, ..., Sense)` one, `from_ax22d` the `gp_Ax22d` one, `Parabola::from_directrix_focus(D, F)` the `(gp_Ax2d D, gp_Pnt2d F)` one |
| `BezierCurve`, `BSplineCurve`, `TrimmedCurve` : `Curve2d`, `BoundedCurve2d` | the bounded curves |
| `OffsetCurve` : `Curve2d` | `Geom2d_OffsetCurve` |
| `EvalRepDesc`, `EvalRepKind` | `Geom2dEval_RepCurveDesc::Base` with its three kinds, see below |

Where a C++ method returns a `gp` object by reference (`Geom2d_Line::Direction()`, `Location()`, `Position()`) the Rust method returns a
reference; the others return a copy. `BezierCurve::knots` / `multiplicities` / `knot_sequence` return `Vec`s (OCCT builds them from static
tables: `{0, 1}`, `{n, n}` with `n` the number of poles, `FlatBezierKnots(degree)`); `BSplineCurve` returns slices of its stored arrays. `weights()` is
`None` for a polynomial curve (`Geom2d_BezierCurve::Weights()` returns `BSplCLib::NoWeights()`), `weight(i)` is `1.0` then.

### Behaviour that is easy to miss (each is exercised by the golden file)

* **Constructors and setters.** `Circle` / `Ellipse` / `Hyperbola` / `Parabola` constructors raise `Construction` for a negative radius or
  focal length, `Ellipse` also for `major < minor`; `Hyperbola::set_major_radius` / `set_minor_radius` and `Ellipse::set_major_radius` /
  `set_minor_radius` check as the C++ does (`set_major_radius(r)` of an ellipse only checks `r >= minor`). `Direction::from_dir2d` and the
  `gp`-object constructors store the gp object as it is. `BezierCurve::new` raises `Construction` for fewer than 2 or more than 26 poles, for
  weights of a different count, or a weight `<= gp::resolution()`; equal weights (within `gp::resolution()` of their neighbour: the C++
  `Rational` helper) give a polynomial curve. `BSplineCurve` constructors run `CheckCurveData` (degree in `1..=max_degree()`, at least 2 poles,
  knots and multiplicities of the same length, strictly increasing knots (`k[i+1] - k[i] > Epsilon(|k[i]|)`), `nb_poles = BSplCLib::NbPoles`) and raise
  `Construction` otherwise; the end multiplicities of a periodic curve must be equal (not checked by OCCT, never generated).
* **Conics.** `Conic2d::reverse` (shared C++ body) replaces the y direction of the position by its opposite. `reversed_parameter` is
  `2 pi - u` for the circle and the ellipse, `-u` for the others; `first_parameter` / `last_parameter` are `0` / `2 pi` and
  `-/+ Precision::Infinite()`; `is_closed` and `is_periodic` are true for circle and ellipse only. `transform` multiplies the radii by
  `|scale_factor|` and transforms the position. `Line` and `Parabola` override `transformed_parameter` (`u` if infinite, else
  `u * |scale|`) and `parametric_transformation` (`|scale|`).
* **Bezier.** parameters are `0 .. 1`; `reversed_parameter(u) = 1 - u`; `is_closed` is a stored flag (set by every pole change as the C++ does:
  `init`, `set_pole` of the first or last pole, `segment`); every operation that rebuilds the curve through `init` (`increase`,
  `insert_pole_*`, `remove_pole`) and `transform` clear the evaluation representation; `segment`, `set_pole`, `set_weight`, `reverse` do not.
  `resolution(tol)` caches `MaxDerivInv` and every pole change invalidates it (the test queries it before and after a `set_pole`).
* **B-spline.** `reversed_parameter(u) = first + last - u`; `is_closed` compares the end points with `Precision::Computational()` (squared
  distance); `continuity` is the stored `mySmooth` computed by `updateKnots` (note the `KnotAnalysis` quirks: `BSplCLib::KnotForm` compares the
  *absolute values* of the knots, so knots that straddle zero are `NonUniform`; a degree 1 curve with end multiplicity 2 and simple inner knots is
  `PiecewiseBezier`, not `QuasiUniform`); `insert_pole_after/before` and `remove_pole` raise `Construction` for `NonUniform` and `PiecewiseBezier` curves;
  `set_knot` checks the neighbours with `Epsilon`; `segment` of a periodic curve first inserts the end knots, then `set_origin` and `set_not_periodic`;
  `move_point_and_tangent` unperiodizes a periodic curve first (even when it then reports an error). `BSplineCurve` never clears its
  evaluation representation (the C++ does not: only `BezierCurve::transform` / `init` and the `OffsetCurve` modifiers do), and its copy constructor
  clones it.
* **Trimmed curves.** The constructor replaces a trimmed basis by a copy of *its* basis; `set_trim` on a periodic basis keeps the parameters as they are
  or adjusts them with `ElCLib::AdjustPeriodic` (`adjust_periodic`), on a non periodic basis sorts them (flipping `sense`) and raises `Construction` when
  `u1 == u2` or a parameter lies further than `Precision::PConfusion()` outside the basis range; a reversed sense reverses the curve (`Reverse` goes through
  `set_trim(.., true, false)`). `is_periodic` / `is_closed` use `std::remainder` of the trimmed length by the basis period.
* **Offset curves.** `set_basis_curve` clears the evaluation representation, unwraps trimmed and offset bases (`offset += basis.offset`), records the
  basis continuity (`get_basis_curve_continuity`), raises `Construction` ("Offset on C0 curve") for a C0 B-spline that is not `IsG1(first, last,
  Precision::Angular())` unless `not_check_c0`, and rewraps in a `TrimmedCurve` over a copy of the unwrapped basis. The evaluation functions follow
  `Geom2d_OffsetCurveUtils.pxx` (`CalculateD0`..`D3`, `AdjustDerivative`, the singular-point handling with the step `1e-7`, the factor `1e-3` ...); a
  singular point that cannot be handled raises `UndefinedValue` (`EvalD0`) or `UndefinedDerivative` (`EvalD1`..`EvalDN`, and `EvalDN` with `N < 1`).
  `set_basis_curve` *adds* the offsets of an unwrapped offset basis to the current `offset` (the constructor starts from its argument, a later call
  from the stored value). `reverse` (basis reversed, `offset = -offset`), `transform` (`offset *= |scale|`) and `set_offset_value` clear the evaluation
  representation.

## Evaluation representations (`Geom2dEval_RepCurveDesc`, `Geom2dEval_RepUtils`)

`BezierCurve`, `BSplineCurve` and `OffsetCurve` can carry an evaluation representation: a curve (any `Curve2d`) that `eval_d0` ..
`eval_dn` use instead of their own algorithm when the descriptor can answer. The three C++ descriptor classes are one Rust struct:

```text
EvalRepDesc { kind: Full | DerivBounded | Mapped, representation: Option<Arc<dyn Curve2d>>,
              max_deriv_order: usize,          // DerivBounded and Mapped (Full ignores it)
              domain: Option<(f64, f64)>,      // Mapped only: Domain1d { First, Last }; None = the whole domain
              scale: f64, offset: f64 }        // Mapped only: Map1d { Scale, Offset } (Full / DerivBounded: 1.0, 0.0)
```

The functions of `Geom2dEval_RepUtils` are public in the module `geom2d::rep_utils` (the curves use them; they are tested through the
curves): `clone_curve_desc(&Option<EvalRepDesc>) -> Option<EvalRepDesc>` (deep: the representation curve is copied with `copy_curve`; a descriptor
without representation clones to `None`), `try_eval_curve_d0/d1/d2/d3/dn(desc, u[, n]) -> Option<...>` (`None` = C++ `false`: no descriptor, order above
`max_deriv_order`, `n < 1`, or the mapped parameter outside the domain) and the `..._mapped` variants (derivatives are multiplied by `scale^k`,
`dn` by `scale` multiplied `n` times in a loop, exactly as the C++ does), `validate_curve_desc(desc, owner) -> Result<()>`. `Map1d::IsValid` is
`|scale| > Precision::Confusion()` and `Domain1d::Contains(u)` is `first - Confusion <= u <= last + Confusion` (both use `Precision::Confusion()`).

`set_eval_representation(Some(desc))` calls `validate_curve_desc`: `Program` error when `representation` is `None` or the kind is `Mapped` with an
invalid map (the third C++ check, "descriptor chain": the representation is the owner itself, cannot happen with `Arc<dyn Curve2d>` and is not tested).
`set_eval_representation(None)` clears. The descriptor does not copy the representation when it is set; `copy_geometry` / `copy_curve`
(`CloneCurveDesc`) does. `eval_representation()` returns the descriptor, `has_eval_representation()` is `eval_representation().is_some()`.

## Helpers the translation will need

`src/bsplclib` and `src/plib` implement the 3D (`Pnt`) forms and the dimension-generic flat forms; the Geom2d classes call the 2D forms of
`BSplCLib` (`D0`..`D3`, `DN`, `Reverse`, `Resolution`, `BuildCache`, `MovePoint`, `MovePointAndTangent`, `InsertKnots`, `RemoveKnot`,
`IncreaseDegree`, `Unperiodize`, `PrepareInsertKnots`, `PrepareUnperiodize`, `IncreaseDegreeCountKnots`, `KnotAnalysis`, `KnotForm`, `MultForm`,
`MaxKnotMult`, `Hunt`, `FlatIndex`, `LocateParameter` with `FromK1`/`ToK2`, `NbPoles`, `KnotSequence`, `NoWeights`, `UnitWeights`, `FlatBezierKnots`) and of
`PLib` (`Trimming`, `CoefficientsPoles` for `Pnt2d`; `plib::trimming_2d` and `coefficients_poles_2d` exist). Whatever of this is missing in `bsplclib`
(for example `knot_analysis`, `knot_form`, `mult_form`, `max_knot_mult`, `hunt`, `flat_index`, `resolution`, `move_point`, `move_point_and_tangent`, the 2D `reverse`) must be
added to `bsplclib` (public, documented in `docs/api-bsplclib.md` if it exists), never copied into `geom2d`; the existing functions' APIs do not change.
`elclib` already has everything the conics and `TrimmedCurve` need (the 2D value / derivative functions, `adjust_periodic`); `mathlin` provides the
solvers `MovePoint*` use. In `bsplclib` today only `hunt` exists, and it is `pub(crate)`. There is no cache object in `Geom2d_BSplineCurve` / `Geom2d_BezierCurve` of this OCCT version (the cache of the 3D classes
is not in the 2D ones; `Geom2d_BezierCurve::Segment` calls `BSplCLib::BuildCache`, covered by `bezier_segment`).

# The reference file

## Record format

`tests/geom2d.txt` has one record per line: `<family> <inputs...> | <outputs...>`. Floats are 16-hex-digit `f64` bit patterns, integers and
booleans decimal. **Every input is on the left of ` | `**, in the order of the family's schema; a schema is a list of words:

| word | tokens | meaning |
|---|---|---|
| `f` | 1 | a float |
| `i` | 1 | an integer |
| `tr` | 9 | a transformation recipe: kind (integer), then 8 floats (unused ones are 0) |
| `crv` | variable | a curve recipe (recursive for trimmed and offset curves) |
| `ops` | variable | a sequence of operations on the object: `n`, then per operation `code`, `na`, `na` floats |

The oracle aborts if the tokens it prints differ from the schema; `tests/geom2d.rs` has a reader method per word (`Rd::f`, `i`, `tr`,
`crv`, `ops`) and `Rd::finish` asserts that every input word was read. There is no text token (the escapes of `oracle/poly.cpp` are not needed). A line whose
operations print nothing ends with ` |`.

The outputs are the values the operations return, in order. If one of the operations raises an exception the whole output is `throws`;
if it is one of the unchecked inputs listed above the whole output is `unchecked` (the operation is not executed, and neither are the following ones).
The Rust replay must return `Err` at that operation (or earlier); it is compared only as "Err or not".

### `tr` (transformation recipe)

Kind and parameters `p0 .. p7`: `0` identity (the default `Geom2d_Transformation` / `gp_Trsf2d`); `1` mirror about the point `(p0, p1)` (`SetMirror(gp_Pnt2d)`);
`2` mirror about the axis `(p0, p1)`, direction `(p2, p3)` (`SetMirror(gp_Ax2d)`); `3` rotation about `(p0, p1)` by `p2` (`SetRotation`); `4` scale about
`(p0, p1)` by `p2` (`SetScale`; negative factors occur; zero occurs only in the `trsf_*` families); `5` `SetTransformation(gp_Ax2d)` with the axis `(p0 p1 p2 p3)`;
`6` `SetTransformation(from, to)` with the axes `(p0..p3)` and `(p4..p7)`; `7` translation by the vector `(p0, p1)`; `8` translation from `(p0, p1)` to `(p2, p3)`; `9` `gp_Trsf2d::SetValues(p0 .. p5)`
(row-major `a11 a12 a13 a21 a22 a23`; `Geom2d_Transformation::SetTrsf2d` then stores it; a matrix with `|det| < gp::resolution()` is unchecked in `gp`, a non orthogonal one is accepted by OCCT
and gives form `Other`). The directions given as `(dx, dy)` are raw and normalized by `Dir2d::new`. The recipe is applied with `Geom2d_Transformation::Set*` in the transformation families and
as a `gp_Trsf2d` (`Trsf2d::set_*`) to `transform`, `transformed_parameter`, ... .

### `crv` (curve recipe)

```text
crv := kind                                                           (0 line 1 circle 2 ellipse 3 hyperbola 4 parabola 5 bezier 6 bspline 7 trimmed 8 offset)
kind 0..4: ctor ox oy dx dy sense  <body>                             ctor: constructor variant (below); (ox, oy) origin; (dx, dy) raw direction; sense 0/1
    line: (empty)      circle: r      ellipse, hyperbola: a b (major, minor)      parabola: f   [fx fy when ctor = 3]
kind 5 (Bezier):  n  n*(x y)  rat  [nw  nw*w]                         rat = 1: the constructor with the weights (nw floats; nw may differ from n: an invalid variant)
kind 6 (B-spline): degree periodic  nk  nk*knot  nm  nm*mult  np  np*(x y)  rat  [nw  nw*w]
kind 7 (trimmed):  u1 u2 sense adjust  crv                            basis curve = the nested crv
kind 8 (offset):   offset notcheck  crv
```

Constructor variants (`ctor`): line `0` `Line::from_ax2d`, `1` `from_lin2d`, `2` `new(P, V)`; circle `0` `from_circ2d(Circ2d::new(ax, r, sense))`, `1` `Circle::new(ax, r,
sense)`, `2` `from_ax22d(Ax22d::from_ax2d(ax, sense), r)`; ellipse and hyperbola likewise (`from_elips2d` / `from_hypr2d`, `new`, `from_ax22d`); parabola `0` `from_parab2d`, `1` `new`, `2`
`from_ax22d`, `3` `from_directrix_focus(ax, (fx, fy))` (the sense is then the positive one). A constructor that raises makes the whole line `throws` (the gp object is built first).

### `state` (the printed state of an object, first token = type)

`0` line: location, direction. `1` circle: location, x direction, y direction, radius. `2` ellipse, `3` hyperbola: position (3 pairs), major, minor. `4` parabola: position, focal.
`5` Bezier: `n`, poles, rational, `n` weights, closed, has-representation. `6` B-spline: degree, periodic, rational, nb knots, nb poles, knots, multiplicities, poles, weights, closed,
knot distribution, continuity, has-representation. `7` trimmed: first, last, periodic, closed, then the state of the basis. `8` offset: offset, continuity, basis continuity,
has-representation, then the state of the basis. `10` Cartesian point: x y. `11` direction: x y. `12` vector with magnitude: x y. `13` axis placement: location, direction.
(Points, vectors and directions are printed with two floats, an `Ax2d` with 4, an `Ax22d` with 6, an array as its length followed by its values.)

### Operations

Arguments are floats; integers are `lround`ed. P = point (2 floats), V = vector (2 floats), `ax` = 4 floats `(x y dx dy)`.

*Any geometry (`geom_common`)*

| code | operation | arguments | output |
|---|---|---|---|
| 1 | state | | state |
| 4 | `transform(T)` | `tr` | |
| 21 | copy, then `transform(T)` of the original | `tr` | state of the copy |
| 22 / 23 | `mirror_point` / `mirror_axis` | P / ax | |
| 24 / 25 | `rotate(P, a)` / `scale(P, s)` | x y a / x y s | |
| 26 / 27 | `translate(V)` / `translate_points(P1, P2)` | x y / x1 y1 x2 y2 | |
| 28 .. 33 | `mirrored_point`, `mirrored_axis`, `rotated`, `scaled`, `translated`, `translated_points` (same arguments as 22 .. 27) | | state of the result |
| 34 | `transformed(T)` | `tr` | state of the result |
| 35 | replace the object by its copy | | |

*Cartesian point (40..49)*: 40 `set_coord(x, y)`, 41 `set_pnt2d(x, y)`, 42 `set_x`, 43 `set_y`; 44 `coord` (P), 45 `pnt2d` (P), 46 `x`, 47 `y` (F); 48 `distance`, 49
`square_distance` to a Cartesian point `(x, y, ctor)` (F). Spec: `i f f` = constructor (0 `from_pnt2d`, 1 `new`), x, y.

*Vectors (50..79)*: spec `i i f f f f` = kind (0 direction, 1 vector with magnitude), constructor (direction: 0 `new(x, y)`, 1 `from_dir2d(Dir2d::new(x, y))`; vector: 0 `from_vec2d`, 1 `new(x, y)`, 2
`from_points((x, y), (x2, y2))`), then `x y x2 y2`. An "other" operand is `(kind, x, y)`. 50 `set_coord(x, y)`, 51 `set_x`, 52 `set_y`, 53 `set_dir2d` / `set_vec2d(x, y)`, 54 `reverse`, 55 `reversed` (state), 56
`angle(other)` (F), 57 `coord` (P), 58 `magnitude`, 59 `square_magnitude`, 60 `x`, 61 `y` (F), 62 `crossed(other)`, 63 `dot(other)` (F), 64 `vec2d` (V), 65 `dir2d` (direction only, P); vector with
magnitude only: 70 `add(other)`, 71 `added` (state), 72 `subtract`, 73 `subtracted` (state), 74 `multiply(s)`, 75 `multiplied` (state), 76 `divide(s)`, 77 `divided` (state), 78 `normalize`, 79 `normalized` (state).

*Axis placement (80..88)*: spec `i f f f f` = constructor (0 `new(Ax2d)`, 1 `from_point_direction`), `ox oy dx dy`. 80 `reverse`, 81 `reversed` (state), 82 `set_axis(ax)`, 83 `set_direction(dx, dy)`, 84
`set_location(P)`, 85 `angle(AxisPlacement::new(ax))` (F), 86 `ax2d` (ax), 87 `direction` (2 floats), 88 `location` (P).

*Transformation (own numbering, family spec `tr ops`; `state` = form, is_negative, scale_factor, then `value(r, c)` for r = 1..2, c = 1..3)*: 1 state; 2 `set_*` with a recipe (`tr`); 3
`invert`; 4 `inverted` (state); 5 `multiply(T)` (recipe object); 6 `multiplied(T)` (state); 7 `pre_multiply(T)`; 8 `power(n)`; 9 `powered(n)` (state); 10 `transforms(x, y)` (P); 11 `value(r, c)` (F); 12 replace by
`copy()`; 13 replace by `Transformation::from_trsf2d(recipe)`; 14 `set_trsf2d(recipe)`; 15 `&t * &T` (state); 16 `t *= &T`.

*Curves (the object is built from the first `crv`; a second `crv` is the argument of codes 230 and 240)*

| code | operation | arguments | output |
|---|---|---|---|
| 2 | `reverse` | | |
| 3 | `reversed_parameter(u)` | u | F |
| 5 | `transformed_parameter(u, T)` | u, `tr` | F |
| 6 | `parametric_transformation(T)` | `tr` | F |
| 7, 8, 9, 10 | `eval_d0`, `eval_d1`, `eval_d2`, `eval_d3` | u | P; P V; P V V; P V V V |
| 11 | `eval_dn(u, n)` | u n | V |
| 12 | `value(u)` | u | P |
| 13, 14 | `first_parameter`, `last_parameter` | | F |
| 15, 16 | `is_closed`, `is_periodic` | | B |
| 17 | `period` | | F |
| 18 | `continuity` | | I |
| 19 | `is_cn(n)` | n | B |
| 20 | `reversed` | | state of the copy |
| 100 .. 108 | line: `set_lin2d(ax)`, `set_direction(dx dy)`, `set_location(P)`, `set_position(ax)`, `lin2d` (P, 2 floats), `distance(P)` (F), `direction` (2 floats), `location` (P), `position` (ax) | | |
| 110 .. 114 | conic: `set_axis(ax, sense)` (5 args), `set_axis(Ax22d::from_dirs(P, dir, dir))` (6 args: `x y xdx xdy ydx ydy`), `set_x_axis(ax)`, `set_y_axis(ax)`, `set_location(P)` | | |
| 115 .. 119 | conic: `x_axis` (ax), `y_axis` (ax), `eccentricity` (F), `location` (P), `position` (6 floats) | | |
| 120, 121, 122 | circle: `set_radius(r)`, `set_circ2d(ax sense r)` (6 args), `circ2d` (radius, position) | | |
| 123, 124 | ellipse / hyperbola: `set_major_radius(r)`, `set_minor_radius(r)` | r | |
| 125, 126 | ellipse: `set_elips2d(ax sense a b)` (7 args), `elips2d` (major, minor, position) | | |
| 127 .. 134 | ellipse / hyperbola (129 and 134 also parabola): `directrix1` (ax), `directrix2` (ax), `focal` (F), `focus1` (P), `focus2` (P), `major_radius`, `minor_radius`, `parameter` (F) | | |
| 135 .. 139 | hyperbola: `asymptote1`, `asymptote2` (ax), `conjugate_branch1`, `conjugate_branch2`, `other_branch` (major, minor, position) | | |
| 140, 141 | hyperbola: `set_hypr2d(ax sense a b)`, `hypr2d` (major, minor, position) | | |
| 142 .. 146 | parabola: `set_focal(f)`, `set_parab2d(ax sense f)`, `parab2d` (focal, position), `directrix` (ax), `focus` (P) | | |
| 150, 151 | `start_point`, `end_point` (Bezier, B-spline, trimmed) | | P |
| 160 | `increase(d)` / `increase_degree(d)` | d | |
| 161, 162 | `insert_pole_after(i, P, w)` / `insert_pole_before` | i x y w | |
| 163 | `remove_pole(i)` | i | |
| 164 | `segment(u1, u2)` (Bezier) / `segment(u1, u2, tol)` (B-spline) | u1 u2 tol | |
| 165, 166, 167 | `set_pole(i, P)`, `set_pole_weight(i, P, w)`, `set_weight(i, w)` | i x y [w] / i w | |
| 168 | `resolution(tol)` | tol | F |
| 169, 170 | `pole(i)` (P), `weight(i)` (F) | i | |
| 171, 172, 173 | `nb_poles`, `degree` (I), `is_rational` (B) | | |
| 174, 175 | `poles` (length, points), `weights` (B `weights().is_some()`, then length and values if some) | | |
| 176, 177, 178 | `knots`, `multiplicities`, `knot_sequence` (length and values) | | |
| 179 | `max_degree` | | I |
| 180 | `increase_multiplicity(i, m)` | i m | |
| 181, 182 | `increase_multiplicity_range(i1, i2, m)`, `increment_multiplicity(i1, i2, m)` | i1 i2 m | |
| 183 | `insert_knot(u, m, tol)` | u m tol | |
| 184 | `insert_knots(knots, mults, tol, add)` | tol add n then n knots then n multiplicities | |
| 185 | `remove_knot(i, m, tol)` | i m tol | B |
| 186, 187 | `set_knot(i, k)`, `set_knot_mult(i, k, m)` | i k [m] | |
| 188 | `set_knots` | n then n knots | |
| 189 | `periodic_normalization(u)` | u | F |
| 190, 191, 192 | `set_periodic`, `set_origin(i)`, `set_not_periodic` | | |
| 193 | `move_point(u, P, i1, i2)` | u x y i1 i2 | I I |
| 194 | `move_point_and_tangent(u, P, tangent, tol, sc, ec)` | u x y tx ty tol sc ec | I |
| 195 | `is_g1(tf, tl, angular_tol)` | | B |
| 196 | `locate_u(u, tol, with_repetition)` | u tol rep | I I |
| 197 .. 201 | `local_value`, `local_d0`, `local_d1`, `local_d2`, `local_d3` | u k1 k2 | P; P; P V; P V V; P V V V |
| 202 | `local_dn(u, k1, k2, n)` | u k1 k2 n | V |
| 203, 204 | `knot(i)` (F), `multiplicity(i)` (I) | i | |
| 205, 206, 207, 208 | `knot_distribution`, `first_u_knot_index`, `last_u_knot_index`, `nb_knots` | | I |
| 220 | trimmed: `set_trim(u1, u2, sense, adjust_periodic)` | | |
| 221 | trimmed or offset: state of `basis_curve()` | | state |
| 230 | offset: `set_basis_curve(build(second crv), not_check_c0)` | notcheck | |
| 231, 232 | offset: `set_offset_value(d)`, `offset` (F) | | |
| 234 | offset: `get_basis_curve_continuity` | | I |
| 240 | `set_eval_representation(desc)` over the second `crv` | kind max_deriv has_domain first last scale offset null_representation | |
| 241, 242, 243 | `clear_eval_representation`, `has_eval_representation` (B), kind of the representation (I, -1 if none) | | |
(233 is unused.) The curves with a second recipe (`crv crv ops`) are `offset_set` and the `evalrep_*` families; for `kind` of 240: 0 `Full`, 1 `DerivBounded`, 2 `Mapped`.

## Families

Each family has 18 distinct lines (the oracle stops there to keep the golden file under 1.5 MB; the test requires at least 16, no duplicate lines, the exact family list and the
exact line count). `th` / `un` are the numbers of `throws` / `unchecked` lines. The `EXACT` families are compared bit for bit, the others within 8 ulps.

| family | schema | lines | th | un | comparison | covers |
|---|---|---|---|---|---|---|
| `axis_ops` | `i f f f f ops` | 18 | 0 | 0 | ulps | axis placement: constructors, Reverse(d), setters, Angle, accessors |
| `axis_xform` | `i f f f f ops` | 18 | 0 | 0 | ulps | axis placement under the geometry operations |
| `bezier_build` | `crv ops` | 18 | 1 | 4 | EXACT | Bezier constructors (invalid sizes and weights), all accessors, Knots / Multiplicities / KnotSequence, MaxDegree |
| `bezier_eval` | `crv ops` | 18 | 0 | 0 | EXACT | D0..D3, DN beyond the degree, StartPoint / EndPoint; rational, polynomial, cusps |
| `bezier_increase` | `crv ops` | 18 | 0 | 6 | EXACT | Increase (equal, lower and above the maximum degree) |
| `bezier_insert_pole` | `crv ops` | 18 | 0 | 5 | EXACT | InsertPoleAfter / Before, up to the maximum degree |
| `bezier_remove_pole` | `crv ops` | 18 | 0 | 11 | EXACT | RemovePole |
| `bezier_resolution` | `crv ops` | 18 | 0 | 0 | EXACT | Resolution and its cache invalidation |
| `bezier_segment` | `crv ops` | 18 | 0 | 0 | EXACT | Segment (reversed, equal, outside parameters; rational and polynomial) |
| `bezier_set_pole` | `crv ops` | 18 | 0 | 5 | EXACT | SetPole x2, SetWeight, weights that make the curve polynomial again |
| `bspline_build` | `crv ops` | 18 | 3 | 3 | EXACT | B-spline constructors (invalid degree, sizes, knots, weights), every accessor, knot distribution |
| `bspline_eval` | `crv ops` | 18 | 0 | 0 | EXACT | D0..DN, periodic and non periodic, rational and polynomial, multiplicities up to degree + 1, parameters outside |
| `bspline_increase_degree` | `crv ops` | 18 | 3 | 0 | EXACT | IncreaseDegree |
| `bspline_increase_mult` | `crv ops` | 18 | 0 | 3 | EXACT | IncreaseMultiplicity x2, IncrementMultiplicity |
| `bspline_insert_knot` | `crv ops` | 18 | 1 | 0 | EXACT | InsertKnot (existing and new knots, tolerances) |
| `bspline_insert_knots` | `crv ops` | 18 | 0 | 0 | EXACT | InsertKnots (Add true / false) |
| `bspline_insert_pole` | `crv ops` | 18 | 4 | 0 | EXACT | InsertPoleAfter / Before (uniform and non uniform knots) |
| `bspline_is_g1` | `crv ops` | 18 | 0 | 0 | ulps | IsG1, IsCN, Continuity |
| `bspline_local` | `crv ops` | 18 | 0 | 4 | EXACT | LocalValue, LocalD0..D3, LocalDN |
| `bspline_locate` | `crv ops` | 18 | 0 | 0 | EXACT | LocateU (with and without knot repetition) |
| `bspline_move_point` | `crv ops` | 18 | 4 | 0 | EXACT | MovePoint |
| `bspline_move_tangent` | `crv ops` | 18 | 0 | 0 | ulps | MovePointAndTangent |
| `bspline_periodic` | `crv ops` | 18 | 6 | 0 | EXACT | SetPeriodic, SetNotPeriodic, SetOrigin, PeriodicNormalization |
| `bspline_remove_knot` | `crv ops` | 18 | 1 | 0 | EXACT | RemoveKnot (after inserting, exact and tolerant) |
| `bspline_remove_pole` | `crv ops` | 18 | 8 | 0 | EXACT | RemovePole |
| `bspline_resolution` | `crv ops` | 18 | 0 | 0 | EXACT | Resolution and its cache invalidation |
| `bspline_segment` | `crv ops` | 18 | 8 | 0 | EXACT | Segment (periodic, reversed, outside, tolerance) |
| `bspline_set_knot` | `crv ops` | 18 | 9 | 3 | EXACT | SetKnot, SetKnot with multiplicity, SetKnots |
| `bspline_set_pole` | `crv ops` | 18 | 6 | 0 | EXACT | SetPole x2, SetWeight |
| `circle_ops` | `crv ops` | 18 | 2 | 0 | ulps | Geom2d_Circle: SetRadius (negative throws), SetCirc2d, Circ2d, evaluation, Reverse |
| `conic_axes` | `crv ops` | 18 | 0 | 0 | ulps | Geom2d_Conic: SetAxis (both forms), SetXAxis, SetYAxis, SetLocation, XAxis, YAxis, Reverse |
| `conic_build` | `crv ops` | 18 | 5 | 0 | ulps | all four conics: every constructor, invalid radii, state and accessors |
| `curve_copy` | `crv ops` | 18 | 3 | 0 | ulps | Copy independence: transform the original after the copy, Reversed, swap to the copy |
| `curve_d0` | `crv ops` | 18 | 0 | 0 | ulps | EvalD0 / Value on every kind, parameters inside, on the ends, outside and across periods |
| `curve_d1` | `crv ops` | 18 | 0 | 0 | ulps | EvalD1 |
| `curve_d2` | `crv ops` | 18 | 1 | 0 | ulps | EvalD2 |
| `curve_d3` | `crv ops` | 18 | 0 | 0 | ulps | EvalD3 |
| `curve_dn` | `crv ops` | 18 | 6 | 0 | ulps | EvalDN with N from -1 to 6 (N < 1 throws) |
| `curve_geom_ops` | `crv ops` | 18 | 1 | 0 | ulps | Mirror / Rotate / Scale / Translate / Transform and the copies on curves |
| `curve_params` | `crv ops` | 18 | 1 | 4 | ulps | FirstParameter, LastParameter, IsClosed, IsPeriodic, Period, Continuity, IsCN of every curve kind (trimmed and offset included) |
| `curve_reverse` | `crv ops` | 18 | 2 | 0 | ulps | Reverse, Reversed, ReversedParameter, evaluation after reversing |
| `curve_xform` | `crv ops` | 18 | 4 | 0 | ulps | Transform with all recipes (negative scale, mirrors), TransformedParameter, ParametricTransformation |
| `direction_ops` | `i i f f f f ops` | 18 | 0 | 5 | ulps | direction: constructors, setters (zero length unchecked), Reverse, Angle, Crossed, Dot, accessors |
| `direction_xform` | `i i f f f f ops` | 18 | 0 | 0 | ulps | direction under the geometry operations |
| `ellipse_ops` | `crv ops` | 18 | 4 | 1 | ulps | Geom2d_Ellipse: SetMajorRadius / SetMinorRadius / SetElips2d, Elips2d, directrices, foci, Focal, Parameter |
| `evalrep_bounded` | `crv crv ops` | 18 | 0 | 0 | ulps | DerivBounded descriptors (orders above the limit fall back) |
| `evalrep_copy` | `crv crv ops` | 18 | 2 | 0 | ulps | Copy / clone, and which modifiers clear the descriptor |
| `evalrep_full` | `crv crv ops` | 18 | 0 | 0 | ulps | Full descriptors |
| `evalrep_invalid` | `crv crv ops` | 18 | 15 | 0 | ulps | null representation, invalid map: ProgramError |
| `evalrep_mapped` | `crv crv ops` | 18 | 1 | 0 | ulps | Mapped descriptors (domain, scale, offset) |
| `hyperbola_ops` | `crv ops` | 18 | 1 | 0 | ulps | Geom2d_Hyperbola: setters, asymptotes, conjugate and other branches, directrices, foci |
| `line_ops` | `crv ops` | 18 | 0 | 2 | ulps | Geom2d_Line: constructors, SetLin2d / SetDirection / SetLocation / SetPosition, Lin2d, Distance, derivatives |
| `offset_build` | `crv ops` | 18 | 5 | 0 | ulps | Geom2d_OffsetCurve constructor: basis unwrapping, C0 check, accessors |
| `offset_eval` | `crv ops` | 18 | 1 | 0 | ulps | D0..DN of offsets (zero and negative distance) |
| `offset_set` | `crv crv ops` | 18 | 3 | 0 | ulps | SetBasisCurve, SetOffsetValue, Reverse, Transform |
| `offset_singular` | `crv ops` | 18 | 10 | 0 | ulps | offsets of curves with cusps and zero derivatives (AdjustDerivative paths, undefined values) |
| `parabola_ops` | `crv ops` | 18 | 1 | 0 | ulps | Geom2d_Parabola: SetFocal, SetParab2d, Parab2d, Directrix, Focus, Focal, Parameter |
| `point_ops` | `i f f ops` | 18 | 0 | 0 | EXACT | Cartesian point: both constructors, setters, accessors, distances |
| `point_xform` | `i f f ops` | 18 | 0 | 0 | ulps | Cartesian point under Mirror / Rotate / Scale / Translate / Transform and the `...ed` copies, Copy |
| `trimmed_build` | `crv ops` | 18 | 3 | 3 | ulps | Geom2d_TrimmedCurve constructor (trimmed basis, reversed, equal and out of range parameters), accessors |
| `trimmed_eval` | `crv ops` | 18 | 0 | 0 | ulps | evaluation, ReversedParameter, TransformedParameter, StartPoint / EndPoint |
| `trimmed_periodic` | `crv ops` | 18 | 0 | 0 | ulps | trims across the period: IsClosed, IsPeriodic, Period, Reverse |
| `trimmed_set_trim` | `crv ops` | 18 | 1 | 0 | ulps | SetTrim |
| `trsf_invert` | `tr ops` | 18 | 0 | 5 | EXACT | Invert / Inverted, including scale 0 (unchecked) |
| `trsf_misc` | `tr ops` | 18 | 0 | 0 | ulps | Copy, Value in and out of range, constructor from gp_Trsf2d, SetTrsf2d, Transforms |
| `trsf_multiply` | `tr ops` | 18 | 0 | 0 | EXACT | Multiply, Multiplied, PreMultiply, operator* and operator*= |
| `trsf_power` | `tr ops` | 18 | 0 | 5 | EXACT | Power / Powered, negative and zero exponents, scale 0 (unchecked) |
| `trsf_set` | `tr ops` | 18 | 0 | 0 | ulps | Geom2d_Transformation: every Set* (mirror, rotation, scale, SetTransformation x2, translation x2, SetTrsf2d), constructors, state, Transforms, Value |
| `vecmag_arith` | `i i f f f f ops` | 18 | 0 | 1 | EXACT | Add / Added, Subtract / Subtracted, Multiply, Divide (zero unchecked), Normalize (zero unchecked) |
| `vecmag_ops` | `i i f f f f ops` | 18 | 0 | 0 | ulps | vector with magnitude: three constructors, setters, accessors, Angle, Crossed, Dot |
| `vecmag_xform` | `i i f f f f ops` | 18 | 0 | 0 | ulps | vector under the geometry operations |

Totals: 71 families, 1278 lines (1184913 bytes), 126 `throws` lines, 70 `unchecked` lines.

## Coverage and what is left out

Every public method of the classes in scope is called by at least one family (see the table above and the operation codes). The following are deliberately not judged:

* `DumpJson` (all classes): no Rust API.
* The `Geom2d_Curve::D0` .. `D3` / `DN` inline wrappers and the deprecated array-filling accessors (`Poles(NCollection_Array1&)`, `Weights(array&)`, `Knots(array&)`,
  `KnotSequence(array&)`, `Multiplicities(array&)`, `WeightsArray()`): the same data is judged through `eval_*`, `poles`, `weights`, `knots`, ...;
* `Geom2d_UndefinedDerivative`, `Geom2d_UndefinedValue` as classes (they are the exceptions behind `Geom2dError::UndefinedDerivative` / `UndefinedValue`);
* the aliasing of a handle (`OffsetCurve::set_basis_curve` keeps the caller's handle; two curves sharing one basis and one of them being modified): an
  `Arc<dyn Curve2d>` cannot be modified through the shared handle; likewise the "descriptor chain" check of `ValidateCurveDesc`;
* the other classes of `Geom2dEval` (`AHTBezierCurve`, `ArchimedeanSpiralCurve`, `CircleInvoluteCurve`, `LogarithmicSpiralCurve`, `SineWaveCurve`, `TBezierCurve`): not in scope;
  a representation curve in the test is a line, circle, Bezier or B-spline curve;
* argument shapes that OCCT cannot receive in Rust (null handles, arrays with a lower bound other than 1): not representable;
* inputs for which OCCT 8.0.1 as built reads or writes outside an array and the result is not reproducible (the oracle drops the case, see the next section) other than the
  ones listed as `unchecked`.

## How the file is made (the uninitialised-read check, the C library, determinism)

`oracle/geom2d.cpp` is a single C++17 program (build: `mkdir -p oracle-bin && c++ -std=c++17 -O2 oracle/geom2d.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKG2d -lTKMath
-lTKernel -ldl -o oracle-bin/geom2d && LD_LIBRARY_PATH=$OCCT/lib oracle-bin/geom2d > tests/geom2d.txt`, with
`OCCT=.../deps/occt-install`; two runs give the same bytes; with a first argument `oracle-bin/geom2d point,vec,axis,trsf,curve,conic,bezier,bspline,trim,offset,erep` runs a
group, with a second one a family). Each line is a pseudo-random case from a fixed xorshift generator plus the edge cases listed in the families (all four conics with their
constructors and invalid arguments; Bezier curves of 2 .. 26 poles, rational and polynomial; B-splines of degree 1 .. 9, periodic and not, rational and polynomial, inner multiplicities 1 .. degree, end multiplicities up to degree + 1 (and below it: a non periodic curve that is not clamped),
uniform, quasi-uniform and piecewise-Bezier knots, cusps made of repeated poles (an inner multiplicity above the degree and a periodic curve with fewer poles than degree + 1 are not generated: OCCT's `NbPoles` refuses the first, the second makes several operations read outside their arrays); trimmed curves across the period, reversed, equal and out of range;
offsets of zero, negative and huge distance, over lines, conics, trimmed and offset curves and C0 B-splines; transformations with negative scale, mirrors and exact Pythagorean rotations; descriptors of all
three kinds with domains and negative scale). The sequence of operations of a line is planned on a scratch object in a child process (the operations may corrupt the heap when
OCCT's missing checks are hit) and then replayed.

Every call is executed six times with the stack and the heap filled differently (pseudo-random words, all 0, all 1; `mallopt(M_PERTURB)`): a case whose printed results differ between the runs (an
uninitialised read in OCCT), that is not finite (a NaN or an infinity would be printed), or whose child process crashes, is dropped. The families that call the C library are in addition re-run
24 times with every `sin cos tan sinh cosh asin acos atan atan2 exp log pow` result moved by 0 or 1 ulp (an interposed `libm`), and a case whose outputs move by more than 2 ulps is dropped:
the golden values are therefore stable under a different C library, which is what the 8 ulp tolerance relies on. The `EXACT` families were checked to have no case that changes under that
perturbation and to call none of those functions.

## Using the judge

`cargo test --release --test geom2d` once `pub mod geom2d;` exists (the test does not compile before: its only error is the unresolved import of `occt_port::geom2d`; against the signature
listing above it compiles and every call panics with `not yet implemented`). `tests/geom2d.rs` must not be edited: it asserts the family list, the golden line count, no duplicate line and at
least 16 distinct lines per family; the `BEGIN GENERATED` block holds the lists that are regenerated from `tests/geom2d.txt`.
