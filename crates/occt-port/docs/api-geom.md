# `geom`: Rust API contract

This contract covers OCCT 8.0.1's 3D geometry package `Geom` (`cpp/Geom/`, toolkit TKG3d) and the parts of `GeomEval` that it uses
(`GeomEval_RepCurveDesc.hxx`, `GeomEval_RepSurfaceDesc.hxx`, `GeomEval_RepUtils.pxx`); `cpp/GeomAbs/` gives the enumerations. The classes are:

* `Geom_Geometry` (`Mirror` about a point, an axis and a plane, `Rotate`, `Scale`, `Translate` x2, `Transform`, `Mirrored` x3, `Rotated`, `Scaled`,
  `Translated` x2, `Transformed`, `Copy`), `Geom_Point` / `Geom_CartesianPoint`, `Geom_Vector` / `Geom_Direction` / `Geom_VectorWithMagnitude`
  (every arithmetic method, `Cross`, `Crossed`, `CrossCross`, `CrossCrossed`, `Angle`, `AngleWithRef`, `Dot`, `DotCross`), `Geom_AxisPlacement` /
  `Geom_Axis1Placement` / `Geom_Axis2Placement`, `Geom_Transformation` (every method);
* `Geom_Curve` (`Reverse`, `Reversed`, `ReversedParameter`, `TransformedParameter`, `ParametricTransformation`, `IsClosed`, `IsPeriodic`, `Period`,
  `Continuity`, `IsCN`, `EvalD0`..`EvalDN`, `Value`), `Geom_Line`, `Geom_Conic` with `Geom_Circle`, `Geom_Ellipse`, `Geom_Hyperbola`, `Geom_Parabola`,
  `Geom_BoundedCurve` with `Geom_BezierCurve`, `Geom_BSplineCurve` (every editing method: degree, multiplicities, knots, segment, periodicity,
  origin, poles, weights, `MovePoint`, `MovePointAndTangent`, `LocalD*`, `LocateU`, `Resolution`, `IsEqual`) and `Geom_TrimmedCurve`, `Geom_OffsetCurve`;
* `Geom_Surface` (`UReverse`, `VReverse`, `UReversed`, `VReversed`, the parameter variants, `TransformParameters`, `ParametricTransformation`, `Bounds`,
  `IsUClosed`, `IsVClosed`, `IsUPeriodic`, `UPeriod`, `IsVPeriodic`, `VPeriod`, `UIso`, `VIso`, `Continuity`, `IsCNu`, `IsCNv`, `EvalD0`..`EvalDN`, `Value`),
  `Geom_ElementarySurface` with `Geom_Plane`, `Geom_CylindricalSurface`, `Geom_ConicalSurface`, `Geom_SphericalSurface`, `Geom_ToroidalSurface`,
  `Geom_SweptSurface` with `Geom_SurfaceOfLinearExtrusion` and `Geom_SurfaceOfRevolution`, `Geom_BoundedSurface` with `Geom_BezierSurface` and
  `Geom_BSplineSurface` (every editing method) and `Geom_RectangularTrimmedSurface`, `Geom_OffsetSurface` (the basis surface detection and unwrapping,
  the C0 check, `Surface`, `UOsculatingSurface`, `VOsculatingSurface` and the osculating-surface fallback at singular points of the evaluation);
* `GeomEval_RepCurveDesc` and `GeomEval_RepSurfaceDesc` (`Full`, `DerivBounded`, `Mapped`, `Map1d`, `Domain1d`, `Map2d`, `Domain2d`) and `GeomEval_RepUtils`
  (`CloneCurveDesc`, `CloneSurfaceDesc`, `TryEvalCurveD0`..`DN`, `TryEvalSurfaceD0`..`DN`, the `...Mapped` variants, `ValidateCurveDesc`, `ValidateSurfaceDesc`),
  used by Bezier, B-spline, offset curves, Bezier and B-spline surfaces, extrusion and revolution surfaces and offset surfaces through
  `SetEvalRepresentation`. The other classes of `GeomEval` (helix, sine wave, T-Bezier, AHT-Bezier, ellipsoid, hyperboloid, paraboloid, helicoid, ...)
  are not in scope: a representation curve or surface is any `Curve` / `Surface` of this module. In the offset surface the representation is created by
  the C++ constructor (`makeFullSurfaceRep`), which is part of the judged behaviour (`has_eval_representation` is printed).

The module is `occt_port::geom` (add `pub mod geom;` to `src/lib.rs`; `src/geom/` may have private submodules). Tests: `tests/geom.rs` with the golden
file `tests/geom.txt` printed by `oracle/geom.cpp` (real OCCT 8.0.1). Nothing that exists in `src/` changes. The module reuses `gp` (`Pnt`, `Vec`, `Dir`, `Ax1`, `Ax2`,
`Ax3`, `Trsf`, `GTrsf2d`, `TrsfForm`, `Lin`, `Circ`, `Elips`, `Hypr`, `Parab`, `Pln`, `Cylinder`, `Cone`, `Sphere`, `Torus`), `elclib` and `elslib` (the value / derivative
functions of lines, conics and elementary surfaces), `bsplclib` and `bsplslib` (all the B-spline algorithms), `plib`, `convert`, `geom2d` (`BSplKnotDistribution`
is re-exported) and `mathlin` (`MathError`): they are complete where they are used and must be reused, not copied. See "Helpers the translation will need" below.
Because the Geom code uses the name `Vec` for the vector of `gp`, the listing writes it `gp::Vec`.

## Rust design (the second OCCT class hierarchy: the same pattern as `geom2d`, whose conventions are followed as closely as the 3D classes allow)

* **One struct per concrete class** (`CartesianPoint`, `Direction`, `VectorWithMagnitude`, `Axis1Placement`, `Axis2Placement`, `Line`, `Circle`, `Ellipse`,
  `Hyperbola`, `Parabola`, `BezierCurve`, `BSplineCurve`, `TrimmedCurve`, `OffsetCurve`, `Plane`, `CylindricalSurface`, `ConicalSurface`, `SphericalSurface`,
  `ToroidalSurface`, `SurfaceOfLinearExtrusion`, `SurfaceOfRevolution`, `BezierSurface`, `BSplineSurface`, `RectangularTrimmedSurface`, `OffsetSurface`;
  `Transformation` is a plain struct, it is not a `Geom_Geometry`). No `Geom_` prefix.
* **The virtual methods of the abstract classes become object-safe traits** implemented by the concrete structs. The trait hierarchy mirrors the class
  hierarchy: `Geometry` (`Geom_Geometry`), `Point: Geometry`, `Vector: Geometry`, `AxisPlacement: Geometry`, `Curve: Geometry` (`Geom_Curve`),
  `BoundedCurve: Curve`, `Conic: Curve`, `Surface: Geometry` (`Geom_Surface`), `ElementarySurface: Surface`, `SweptSurface: Surface`, `BoundedSurface: Surface`.
  **The C++ default bodies are trait default methods** wherever the C++ base class implements the method (`Geom_Geometry::Mirror` ... `Translated`;
  `Geom_Curve::Reversed` / `TransformedParameter` / `ParametricTransformation` / `Period` / `Value`; `Geom_Surface::UReversed` / `VReversed` /
  `TransformParameters` / `ParametricTransformation` / `UPeriod` / `VPeriod` / `Value`; `Geom_Point::Distance`; `Geom_Vector::Reverse` / `Angle` / `Dot`
  ...; `Geom_Conic::SetAxis` ... `YAxis`, `Reverse`; `Geom_ElementarySurface::SetAxis` ... `Position`, `UReverse`, `VReverse`); a concrete class overrides the default
  exactly where the C++ class overrides it (`Line` and `Parabola` override `TransformedParameter` and `ParametricTransformation`; `TrimmedCurve` and `OffsetCurve` forward
  them to the basis curve; `TrimmedCurve` and `OffsetCurve` override `Period`; the swept, trimmed and offset surfaces forward `TransformParameters` and
  `ParametricTransformation` to the basis). The protected data of a base class that the default bodies need is reached through required accessor methods
  (`Conic::pos` / `pos_mut`, `ElementarySurface::pos` / `pos_mut`, `Vector::vec` / `set_vec_raw`).
* **Handles.** An OCCT `occ::handle<Geom_Curve>` or `occ::handle<Geom_Surface>` held by another object (`TrimmedCurve`, `OffsetCurve`, `SurfaceOfLinearExtrusion`,
  `SurfaceOfRevolution`, `RectangularTrimmedSurface`, `OffsetSurface` and the representation of an evaluation descriptor) is `Arc<dyn Curve>` / `Arc<dyn Surface>`.
  An object that is handed to a constructor is moved in (the test builds the basis with `Arc::from(boxed)`). **Copy semantics follow OCCT:** `copy_geometry` /
  `copy_curve` / `copy_surface` is OCCT's `Copy()`: deep where the C++ `Copy()` is deep (`TrimmedCurve::Copy` re-creates the trimmed curve over a *copy* of the
  basis; `OffsetCurve::Copy` and `OffsetSurface::Copy` (the copy constructors) copy the basis and clone the evaluation representation; `BezierCurve`,
  `BSplineCurve`, `BezierSurface`, `BSplineSurface` copy the arrays and clone the representation descriptor *and its representation* (`CloneCurveDesc`,
  `CloneSurfaceDesc`); the swept surfaces copy their basis curve; `RectangularTrimmedSurface::Copy` copies the basis surface). The constructors that call
  `C->Copy()` / `S->Copy()` in the C++ code do the same. Because an `Arc` is immutable the aliasing of a C++ handle (two objects sharing one basis, one of
  them modified through the shared handle) is not observable and is not tested.
* **Mutating methods take `&mut self`**; the `...ed` / `Copy` methods take `&self` and return a new object. Stored values are passed by value (`Pnt`, `Dir`,
  `Ax1`, `Ax2`, `Ax3`, `Lin`, `Circ`, ... are `Copy`), arguments that are only read by reference, as in `docs/api-gp3d.md`. C++ out-parameters become returned
  tuples in argument order. Indices are `i32` and 1-based (as in OCCT); a surface's pole array is `&[Vec<Pnt>]`, one entry per U index (so `poles[i - 1][j - 1]` is
  `Pole(i, j)`); a ragged array or an array of a wrong length is an `Err`.
* **Downcasting.** `Geometry` has `as_any` / `as_any_mut` (the test reads the state of an object of unknown type through them, as `IsKind` / `DownCast` do in
  the oracle). Every concrete struct also implements `Debug + Clone`.
* **Names.** `snake_case` of the C++ method; an overload gets its own name when the argument types or the results differ (`mirror_point` / `mirror_axis1` /
  `mirror_axis2`, `translate` / `translate_points`, `insert_pole_after` / `insert_pole_after_weight`, `set_pole` / `set_pole_weight`, `increase_multiplicity` /
  `increase_multiplicity_range`, `set_knot` / `set_knot_mult`, `set_origin` / `set_origin_param`, `set_trim` / `set_trim_param`, `u_iso` / `u_iso_check`, ...);
  constructors: see the listing. The `EvalD0`..`EvalDN` virtuals are `eval_d0`..`eval_dn`; the `D0`..`DN` convenience wrappers of `Geom_Curve` and `Geom_Surface`
  are not separate methods (the test uses `eval_*` and `value`).

## Conventions

* **Errors.** `geom::GeomError` (derive `Debug, Clone, PartialEq, Eq`; `From<gp::ConstructionError>`, `From<BSplError>`, `From<MathError>`, `From<PLibError>`)
  and `type Result<T> = std::result::Result<T, GeomError>`:

  | variant | stands for |
  |---|---|
  | `Construction` | `Standard_ConstructionError` raised by OCCT |
  | `OutOfRange` | `Standard_OutOfRange` raised by OCCT, and a Raise_if range check compiled out |
  | `Domain` | `Standard_DomainError` (raised, or a Raise_if compiled out) |
  | `NoSuchObject` | `Standard_NoSuchObject` (`Period` / `UPeriod` / `VPeriod` of a non periodic object, `SetOrigin` of a non periodic B-spline) |
  | `Range` | `Standard_RangeError` (`IsCN` / `IsCNu` / `IsCNv` with `N < 0`: compiled out) |
  | `UndefinedValue` / `UndefinedDerivative` | `Geom_UndefinedValue` / `Geom_UndefinedDerivative` (offset objects at a singular point, `DN` with an order that is not defined, ...) |
  | `NotImplemented` | `Standard_NotImplemented` (`SurfaceOfRevolution::reference_plane` always raises it) |
  | `Program` | `Standard_ProgramError` (`ValidateCurveDesc`, `ValidateSurfaceDesc`) |
  | `Dimension` | `Standard_DimensionError` (an array of a wrong size) |
  | `Argument` | a wrong shape of a Rust argument, or an unchecked input of OCCT 8.0.1 as built (below) |
  | `Gp(..)`, `BSpl(..)`, `Math(..)`, `PLib(..)` | errors of the functions used inside |

  The test only checks `Err` against the oracle's `throws` / `unchecked`; any variant is accepted. **Not judged but required:** nothing may panic (the lints forbid
  indexing and `unwrap`): an index outside an array, a negative count, a short slice must return `Err`.
* **`throws` and `unchecked`.** `throws` lines are exceptions OCCT really raises: explicit `throw` statements of `Geom_*.cxx` (invalid constructor arguments of
  the conics and elementary surfaces, `SetTrim`, `SetKnot`, `SetOrigin`, `Segment`, `InsertKnots`, `RemoveKnot`, `MovePoint`, `InsertPoleAfter` ..., the
  Bezier and B-spline surface editing methods, `EvalD0` .. `EvalDN` of an offset object at a singular point, `DN` with an order that is not defined, offset on a
  C0 curve or surface, `ValidateCurveDesc`), the `gp` constructors that the oracle itself calls (`gp_Dir(0,0,0)`, `gp_Circ` with a negative radius, a frame whose
  normal and x direction are parallel, ...) and the `Standard_OutOfRange` of the inline `gp_Trsf::Value`. **`unchecked`** lines are inputs for which a
  `Standard_*_Raise_if` check is compiled out of the OCCT libraries (`No_Exception`; this was verified with probes: `Geom_Direction(0,0,0)` returns NaN, `Period()` of a
  line returns `4e100`, the array accessors read outside the array). The oracle does not execute such a call; the Rust API must return `Err` for the same condition.
  The complete list:
  * `Direction::new` / `set_coord` with a vector of length `<= gp::resolution()`; `set_x` / `set_y` / `set_z` when the resulting vector has such a length;
    `Direction::cross` / `crossed` when the cross product with the other vector has a length `<= gp::resolution()` (parallel vectors), `cross_cross` /
    `cross_crossed` when the double cross product has such a length; `Vector::angle` / `angle_with_ref` when one of the vectors has a magnitude
    `<= gp::resolution()` (the check of `gp_Vec::Angle` is compiled out); `VectorWithMagnitude::divide` / `divided` with `|s| <= gp::resolution()`; `normalize` /
    `normalized` of a vector of magnitude `<= gp::resolution()`;
  * `Transformation::value` with a row outside `1..=3` or a column outside `1..=4`; `invert` / `inverted` and `power` / `powered` with `n < 0` when the form is not
    `Identity`, `Translation` or `PntMirror` and `|scale_factor| <= gp::resolution()`; a recipe of `Trsf::set_values` with `|det| < gp::resolution()` or of
    `Trsf::set_scale` with `|s| <= gp::resolution()` (the checks of `gp_Trsf` are compiled out), in any operation that applies a transformation;
  * `Curve::period` when the curve is not periodic (for `TrimmedCurve` and `OffsetCurve`: when the basis curve is not periodic; a trimmed curve over a periodic basis
    answers `Ok` even if the trim is not a whole period); `Surface::u_period` / `v_period` when the surface is not periodic in that direction; `is_cn(n)` with `n < 0`
    for `BSplineCurve`, `TrimmedCurve`, `OffsetCurve`; `is_cn_u(n)` / `is_cn_v(n)` with `n < 0` for `BSplineSurface`, `RectangularTrimmedSurface`, `OffsetSurface`,
    `SurfaceOfLinearExtrusion`, `SurfaceOfRevolution`;
  * `Hyperbola::eccentricity`, `parameter`, `asymptote1/2`, `directrix1/2` when `major_radius <= gp::resolution()`; `Ellipse::directrix1/2` when the eccentricity
    is `<= gp::resolution()` (a circle);
  * `BSplineCurve`: `increase_multiplicity(i, m)` with `i` outside `1..=nb_knots`; `increase_multiplicity_range(i1, i2, m)` and `increment_multiplicity(i1, i2, m)`
    with `i1 < 1`, `i2 > nb_knots` or `i1 > i2`; `knot(i)` / `multiplicity(i)` with `i` outside `1..=nb_knots`; `pole(i)` / `weight(i)` with `i` outside
    `1..=nb_poles`; `local_value` / `local_d0..d3` / `local_dn(u, k1, k2)` with `k1 < 1`, `k2 > nb_knots` or `k1 >= k2`;
  * `BSplineSurface`: `u_knot(i)` / `u_multiplicity(i)` and `increase_u_multiplicity(i, m)` with `i` outside `1..=nb_u_knots` (V likewise);
    `increase_u_multiplicity_range` and `increment_u_multiplicity` with `i1 < 1`, `i2 > nb_u_knots` or `i1 > i2` (V likewise); `pole(i, j)` / `weight(i, j)` with
    `i` outside `1..=nb_u_poles` or `j` outside `1..=nb_v_poles`; `local_value` / `local_d0..d3` / `local_dn` with `k1 < 1`, `k2 > nb_knots` or `k1 >= k2` in U or in V;
  * `BezierSurface::pole(i, j)` / `weight(i, j)` outside the pole array.
  Everything else that OCCT 8.0.1 does not check and that the oracle executes (for example `Segment` with `u2 < u1` on a Bezier curve, `LocateU` outside the range,
  `move_point_and_tangent` with any conditions, a frame direction change that makes the position degenerate: the oracle drops such a case when the result is not finite)
  is deterministic and is reproduced as OCCT computes it.
* **Arithmetic.** Operations are done in the order of the C++ source, with no fused multiply-add and no reassociation. `sin`, `cos`, `acos`, `atan2`, `exp`, `log`,
  `pow` are `libm::` (through `elclib`, `elslib`, `gp`). The families marked EXACT below only use `+ - * /`, `sqrt`, `abs`, `floor` / `ceil` and comparisons (no C library
  function): they are compared bit for bit, signed zeros included. All the others depend on the C library and are compared within 8 ulps of the largest absolute value
  of the line (at least 1; zero tolerance if that value is infinite); the oracle keeps only cases whose outputs stay within 2 ulps when every C-library result
  (`sin cos tan sinh cosh asin acos atan atan2 exp log pow`) is moved by an ulp in 24 patterns. Integers, booleans and the number of values are always compared exactly.
  `Epsilon(x)` of OCCT is the distance from `x` to the next representable `f64` away from zero (`std::nextafter`; `Epsilon(0.0)` is the smallest positive subnormal);
  `Precision::Infinite()` is `2e100`, `Precision::Confusion()` is `1e-7`, `Precision::PConfusion()` is `Confusion() / 100.0` (compute it that way: the literal `1e-9` may
  differ in the last bit), `Precision::Computational()` is `f64::EPSILON`, `Precision::Angular()` is `1e-12`; `Precision::IsInfinite(u)` is `|u| >= 2e100`.
* **Enumerations.** `geom::GeomAbsShape` is a re-export of `plib::GeomAbsShape` (`C0 G1 C1 G2 C2 C3 Cn`, discriminants 0..6) and `geom::BSplKnotDistribution` of
  `geom2d::BSplKnotDistribution` (`NonUniform = 0, Uniform = 1, QuasiUniform = 2, PiecewiseBezier = 3`); `gp::TrsfForm` already exists.
* **Result records.** `ResD1` / `ResD2` / `ResD3` (curves: `point`, `d1`, `d2`, `d3`) and `SurfResD1` / `SurfResD2` / `SurfResD3` (surfaces: `point`, `d1u`, `d1v`, `d2u`,
  `d2v`, `d2uv`, `d3u`, `d3v`, `d3uuv`, `d3uvv`), public fields, `Debug + Clone + Copy + PartialEq`.

## Signature listing

This is the complete public surface the test uses. The bodies are the translation's job; the C++ class and method of each item is the snake_case name (the table after the code
gives the constructors and the traits). Every concrete struct implements `Geometry` (`as_any`, `as_any_mut`, `transform`, `copy_geometry`) and derives `Debug` and `Clone`;
the others traits as in the table. `Geometry::mirror_point` ... `translated_points` have default bodies (the C++ `Geom_Geometry` code: build the `Trsf`, call `transform`; the
`...ed` forms `copy_geometry` first). The `as_any` methods are trivial. The `/* private */` fields are the translation's choice.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeomError {
    Construction,
    OutOfRange,
    Domain,
    NoSuchObject,
    Range,
    UndefinedValue,
    UndefinedDerivative,
    NotImplemented,
    Program,
    Dimension,
    Argument,
    Gp(gp::ConstructionError),
    BSpl(BSplError),
    Math(MathError),
    PLib(PLibError),
}
pub type Result<T> = std::result::Result<T, GeomError>;

pub use crate::plib::GeomAbsShape;
pub use crate::geom2d::BSplKnotDistribution;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD1 { pub point: Pnt, pub d1: gp::Vec }
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD2 { pub point: Pnt, pub d1: gp::Vec, pub d2: gp::Vec }
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResD3 { pub point: Pnt, pub d1: gp::Vec, pub d2: gp::Vec, pub d3: gp::Vec }
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfResD1 { pub point: Pnt, pub d1u: gp::Vec, pub d1v: gp::Vec }
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfResD2 { pub point: Pnt, pub d1u: gp::Vec, pub d1v: gp::Vec, pub d2u: gp::Vec, pub d2v: gp::Vec, pub d2uv: gp::Vec }
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfResD3 {
    pub point: Pnt, pub d1u: gp::Vec, pub d1v: gp::Vec, pub d2u: gp::Vec, pub d2v: gp::Vec, pub d2uv: gp::Vec,
    pub d3u: gp::Vec, pub d3v: gp::Vec, pub d3uuv: gp::Vec, pub d3uvv: gp::Vec,
}

pub trait Geometry: Any + std::fmt::Debug {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn transform(&mut self, t: &Trsf);
    fn copy_geometry(&self) -> Box<dyn Geometry>;
    fn mirror_point(&mut self, p: &Pnt);
    fn mirror_axis1(&mut self, a: &Ax1);
    fn mirror_axis2(&mut self, a: &Ax2);
    fn rotate(&mut self, a: &Ax1, ang: f64);
    fn scale(&mut self, p: &Pnt, s: f64);
    fn translate(&mut self, v: &gp::Vec);
    fn translate_points(&mut self, p1: &Pnt, p2: &Pnt);
    fn mirrored_point(&self, p: &Pnt) -> Box<dyn Geometry>;
    fn mirrored_axis1(&self, a: &Ax1) -> Box<dyn Geometry>;
    fn mirrored_axis2(&self, a: &Ax2) -> Box<dyn Geometry>;
    fn rotated(&self, a: &Ax1, ang: f64) -> Box<dyn Geometry>;
    fn scaled(&self, p: &Pnt, s: f64) -> Box<dyn Geometry>;
    fn transformed(&self, t: &Trsf) -> Box<dyn Geometry>;
    fn translated(&self, v: &gp::Vec) -> Box<dyn Geometry>;
    fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> Box<dyn Geometry>;
}

pub trait Point: Geometry {
    fn coord(&self) -> (f64, f64, f64);
    fn pnt(&self) -> Pnt;
    fn x(&self) -> f64;
    fn y(&self) -> f64;
    fn z(&self) -> f64;
    fn distance(&self, other: &dyn Point) -> f64;
    fn square_distance(&self, other: &dyn Point) -> f64;
}
#[derive(Debug, Clone)]
pub struct CartesianPoint { /* private */ }
impl CartesianPoint {
    pub fn from_pnt(p: Pnt) -> Self;
    pub fn new(x: f64, y: f64, z: f64) -> Self;
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64);
    pub fn set_pnt(&mut self, p: Pnt);
    pub fn set_x(&mut self, x: f64);
    pub fn set_y(&mut self, y: f64);
    pub fn set_z(&mut self, z: f64);
}

pub trait Vector: Geometry {
    fn vec(&self) -> gp::Vec;
    fn set_vec_raw(&mut self, v: gp::Vec);
    fn copy_vector(&self) -> Box<dyn Vector>;
    fn magnitude(&self) -> f64;
    fn square_magnitude(&self) -> f64;
    fn cross(&mut self, other: &dyn Vector) -> Result<()>;
    fn crossed(&self, other: &dyn Vector) -> Result<Box<dyn Vector>>;
    fn cross_cross(&mut self, v1: &dyn Vector, v2: &dyn Vector) -> Result<()>;
    fn cross_crossed(&self, v1: &dyn Vector, v2: &dyn Vector) -> Result<Box<dyn Vector>>;
    fn reverse(&mut self);
    fn reversed(&self) -> Box<dyn Vector>;
    fn angle(&self, other: &dyn Vector) -> Result<f64>;
    fn angle_with_ref(&self, other: &dyn Vector, vref: &dyn Vector) -> Result<f64>;
    fn coord(&self) -> (f64, f64, f64);
    fn x(&self) -> f64;
    fn y(&self) -> f64;
    fn z(&self) -> f64;
    fn dot(&self, other: &dyn Vector) -> f64;
    fn dot_cross(&self, v1: &dyn Vector, v2: &dyn Vector) -> f64;
}
#[derive(Debug, Clone)]
pub struct Direction { /* private */ }
impl Direction {
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self>;
    pub fn from_dir(d: Dir) -> Self;
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) -> Result<()>;
    pub fn set_dir(&mut self, d: Dir);
    pub fn set_x(&mut self, x: f64) -> Result<()>;
    pub fn set_y(&mut self, y: f64) -> Result<()>;
    pub fn set_z(&mut self, z: f64) -> Result<()>;
    pub fn dir(&self) -> Dir;
}
#[derive(Debug, Clone)]
pub struct VectorWithMagnitude { /* private */ }
impl VectorWithMagnitude {
    pub fn from_vec(v: gp::Vec) -> Self;
    pub fn new(x: f64, y: f64, z: f64) -> Self;
    pub fn from_points(p1: Pnt, p2: Pnt) -> Self;
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64);
    pub fn set_vec(&mut self, v: gp::Vec);
    pub fn set_x(&mut self, x: f64);
    pub fn set_y(&mut self, y: f64);
    pub fn set_z(&mut self, z: f64);
    pub fn add(&mut self, other: &dyn Vector);
    pub fn added(&self, other: &dyn Vector) -> VectorWithMagnitude;
    pub fn subtract(&mut self, other: &dyn Vector);
    pub fn subtracted(&self, other: &dyn Vector) -> VectorWithMagnitude;
    pub fn multiply(&mut self, s: f64);
    pub fn multiplied(&self, s: f64) -> VectorWithMagnitude;
    pub fn divide(&mut self, s: f64) -> Result<()>;
    pub fn divided(&self, s: f64) -> Result<VectorWithMagnitude>;
    pub fn normalize(&mut self) -> Result<()>;
    pub fn normalized(&self) -> Result<VectorWithMagnitude>;
}

pub trait AxisPlacement: Geometry {
    fn axis(&self) -> Ax1;
    fn set_axis(&mut self, a: Ax1) -> Result<()>;
    fn set_direction(&mut self, v: Dir) -> Result<()>;
    fn set_location(&mut self, p: Pnt);
    fn angle(&self, other: &dyn AxisPlacement) -> f64;
    fn direction(&self) -> Dir;
    fn location(&self) -> Pnt;
}
#[derive(Debug, Clone)]
pub struct Axis1Placement { /* private */ }
impl Axis1Placement {
    pub fn new(a: Ax1) -> Self;
    pub fn from_point_direction(p: Pnt, v: Dir) -> Self;
    pub fn ax1(&self) -> Ax1;
    pub fn reverse(&mut self);
    pub fn reversed(&self) -> Axis1Placement;
}
#[derive(Debug, Clone)]
pub struct Axis2Placement { /* private */ }
impl Axis2Placement {
    pub fn new(a: Ax2) -> Self;
    pub fn from_point_directions(p: Pnt, n: Dir, vx: Dir) -> Result<Self>;
    pub fn set_ax2(&mut self, a: Ax2);
    pub fn set_x_direction(&mut self, vx: Dir) -> Result<()>;
    pub fn set_y_direction(&mut self, vy: Dir) -> Result<()>;
    pub fn ax2(&self) -> Result<Ax2>;
    pub fn x_direction(&self) -> Dir;
    pub fn y_direction(&self) -> Dir;
}

#[derive(Debug, Clone)]
pub struct Transformation { /* private */ }
impl Transformation {
    pub fn new() -> Self;
    pub fn from_trsf(t: &Trsf) -> Self;
    pub fn set_mirror_point(&mut self, p: &Pnt);
    pub fn set_mirror_axis1(&mut self, a: &Ax1);
    pub fn set_mirror_axis2(&mut self, a: &Ax2);
    pub fn set_rotation(&mut self, a: &Ax1, ang: f64) -> Result<()>;
    pub fn set_scale(&mut self, p: &Pnt, s: f64) -> Result<()>;
    pub fn set_transformation_between(&mut self, from: &Ax3, to: &Ax3);
    pub fn set_transformation(&mut self, to: &Ax3);
    pub fn set_translation(&mut self, v: &gp::Vec);
    pub fn set_translation_points(&mut self, p1: &Pnt, p2: &Pnt);
    pub fn set_trsf(&mut self, t: &Trsf);
    pub fn is_negative(&self) -> bool;
    pub fn form(&self) -> TrsfForm;
    pub fn scale_factor(&self) -> f64;
    pub fn trsf(&self) -> Trsf;
    pub fn value(&self, row: i32, col: i32) -> Result<f64>;
    pub fn invert(&mut self) -> Result<()>;
    pub fn inverted(&self) -> Result<Transformation>;
    pub fn multiplied(&self, other: &Transformation) -> Transformation;
    pub fn multiply(&mut self, other: &Transformation);
    pub fn power(&mut self, n: i32) -> Result<()>;
    pub fn powered(&self, n: i32) -> Result<Transformation>;
    pub fn pre_multiply(&mut self, other: &Transformation);
    pub fn transforms(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64);
    pub fn copy(&self) -> Transformation;
}
impl Default for Transformation;

pub trait Curve: Geometry {
    fn copy_curve(&self) -> Box<dyn Curve>;
    fn reverse(&mut self);
    fn reversed_parameter(&self, u: f64) -> f64;
    fn transformed_parameter(&self, u: f64, t: &Trsf) -> f64;
    fn parametric_transformation(&self, t: &Trsf) -> f64;
    fn reversed(&self) -> Box<dyn Curve>;
    fn first_parameter(&self) -> f64;
    fn last_parameter(&self) -> f64;
    fn is_closed(&self) -> bool;
    fn is_periodic(&self) -> bool;
    fn period(&self) -> Result<f64>;
    fn continuity(&self) -> GeomAbsShape;
    fn is_cn(&self, n: i32) -> Result<bool>;
    fn eval_d0(&self, u: f64) -> Result<Pnt>;
    fn eval_d1(&self, u: f64) -> Result<ResD1>;
    fn eval_d2(&self, u: f64) -> Result<ResD2>;
    fn eval_d3(&self, u: f64) -> Result<ResD3>;
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec>;
    fn value(&self, u: f64) -> Result<Pnt>;
}
pub trait BoundedCurve: Curve {
    fn start_point(&self) -> Pnt;
    fn end_point(&self) -> Pnt;
}
pub trait Conic: Curve {
    fn pos(&self) -> &Ax2;
    fn pos_mut(&mut self) -> &mut Ax2;
    fn eccentricity(&self) -> Result<f64>;
    fn set_axis(&mut self, a: Ax1) -> Result<()>;
    fn set_location(&mut self, p: Pnt);
    fn set_position(&mut self, a: Ax2);
    fn axis(&self) -> Ax1;
    fn location(&self) -> Pnt;
    fn position(&self) -> &Ax2;
    fn x_axis(&self) -> Ax1;
    fn y_axis(&self) -> Ax1;
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalRepKind { Full = 0, DerivBounded = 1, Mapped = 2 }
#[derive(Debug, Clone)]
pub struct EvalRepDesc {
    pub kind: EvalRepKind,
    pub representation: Option<Arc<dyn Curve>>,
    pub max_deriv_order: usize,
    pub domain: Option<(f64, f64)>,
    pub scale: f64,
    pub offset: f64,
}
#[derive(Debug, Clone)]
pub struct SurfaceEvalRepDesc {
    pub kind: EvalRepKind,
    pub representation: Option<Arc<dyn Surface>>,
    pub max_deriv_order: usize,
    pub domain: Option<(f64, f64, f64, f64)>,
    pub scale_u: f64,
    pub offset_u: f64,
    pub scale_v: f64,
    pub offset_v: f64,
    pub swap_uv: bool,
}

#[derive(Debug, Clone)]
pub struct Line { /* private */ }
impl Line {
    pub fn from_ax1(a: Ax1) -> Self;
    pub fn from_lin(l: Lin) -> Self;
    pub fn new(p: Pnt, v: Dir) -> Self;
    pub fn set_lin(&mut self, l: Lin);
    pub fn set_direction(&mut self, v: Dir);
    pub fn set_location(&mut self, p: Pnt);
    pub fn set_position(&mut self, a: Ax1);
    pub fn lin(&self) -> Lin;
    pub fn position(&self) -> &Ax1;
}
#[derive(Debug, Clone)]
pub struct Circle { /* private */ }
impl Circle {
    pub fn from_circ(c: Circ) -> Self;
    pub fn new(a: Ax2, radius: f64) -> Result<Self>;
    pub fn set_circ(&mut self, c: Circ);
    pub fn set_radius(&mut self, r: f64) -> Result<()>;
    pub fn circ(&self) -> Circ;
    pub fn radius(&self) -> f64;
}
#[derive(Debug, Clone)]
pub struct Ellipse { /* private */ }
impl Ellipse {
    pub fn from_elips(e: Elips) -> Self;
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self>;
    pub fn set_elips(&mut self, e: Elips);
    pub fn set_major_radius(&mut self, r: f64) -> Result<()>;
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()>;
    pub fn elips(&self) -> Elips;
    pub fn directrix1(&self) -> Result<Ax1>;
    pub fn directrix2(&self) -> Result<Ax1>;
    pub fn focal(&self) -> f64;
    pub fn focus1(&self) -> Pnt;
    pub fn focus2(&self) -> Pnt;
    pub fn major_radius(&self) -> f64;
    pub fn minor_radius(&self) -> f64;
    pub fn parameter(&self) -> f64;
}
#[derive(Debug, Clone)]
pub struct Hyperbola { /* private */ }
impl Hyperbola {
    pub fn from_hypr(h: Hypr) -> Self;
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self>;
    pub fn set_hypr(&mut self, h: Hypr);
    pub fn set_major_radius(&mut self, r: f64) -> Result<()>;
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()>;
    pub fn hypr(&self) -> Hypr;
    pub fn asymptote1(&self) -> Result<Ax1>;
    pub fn asymptote2(&self) -> Result<Ax1>;
    pub fn conjugate_branch1(&self) -> Result<Hypr>;
    pub fn conjugate_branch2(&self) -> Result<Hypr>;
    pub fn directrix1(&self) -> Result<Ax1>;
    pub fn directrix2(&self) -> Result<Ax1>;
    pub fn focal(&self) -> f64;
    pub fn focus1(&self) -> Pnt;
    pub fn focus2(&self) -> Pnt;
    pub fn major_radius(&self) -> f64;
    pub fn minor_radius(&self) -> f64;
    pub fn other_branch(&self) -> Result<Hypr>;
    pub fn parameter(&self) -> Result<f64>;
}
#[derive(Debug, Clone)]
pub struct Parabola { /* private */ }
impl Parabola {
    pub fn from_parab(p: Parab) -> Self;
    pub fn new(a: Ax2, focal: f64) -> Result<Self>;
    pub fn from_directrix_focus(d: Ax1, f: Pnt) -> Result<Self>;
    pub fn set_focal(&mut self, f: f64) -> Result<()>;
    pub fn set_parab(&mut self, p: Parab);
    pub fn parab(&self) -> Parab;
    pub fn directrix(&self) -> Ax1;
    pub fn focus(&self) -> Pnt;
    pub fn focal(&self) -> f64;
    pub fn parameter(&self) -> f64;
}

#[derive(Debug, Clone)]
pub struct BezierCurve { /* private */ }
impl BezierCurve {
    pub fn new(poles: &[Pnt]) -> Result<Self>;
    pub fn with_weights(poles: &[Pnt], weights: &[f64]) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&EvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<EvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn increase(&mut self, degree: i32) -> Result<()>;
    pub fn insert_pole_after(&mut self, index: i32, p: Pnt) -> Result<()>;
    pub fn insert_pole_after_weight(&mut self, index: i32, p: Pnt, weight: f64) -> Result<()>;
    pub fn insert_pole_before(&mut self, index: i32, p: Pnt) -> Result<()>;
    pub fn insert_pole_before_weight(&mut self, index: i32, p: Pnt, weight: f64) -> Result<()>;
    pub fn remove_pole(&mut self, index: i32) -> Result<()>;
    pub fn segment(&mut self, u1: f64, u2: f64);
    pub fn set_pole(&mut self, index: i32, p: Pnt) -> Result<()>;
    pub fn set_pole_weight(&mut self, index: i32, p: Pnt, weight: f64) -> Result<()>;
    pub fn set_weight(&mut self, index: i32, weight: f64) -> Result<()>;
    pub fn is_rational(&self) -> bool;
    pub fn degree(&self) -> i32;
    pub fn nb_poles(&self) -> i32;
    pub fn pole(&self, index: i32) -> Result<Pnt>;
    pub fn poles(&self) -> &[Pnt];
    pub fn weight(&self, index: i32) -> Result<f64>;
    pub fn weights(&self) -> Option<&[f64]>;
    pub fn knots(&self) -> Vec<f64>;
    pub fn multiplicities(&self) -> Vec<i32>;
    pub fn knot_sequence(&self) -> Vec<f64>;
    pub fn max_degree() -> i32;
    pub fn resolution(&mut self, tolerance_3d: f64) -> f64;
}
#[derive(Debug, Clone)]
pub struct BSplineCurve { /* private */ }
impl BSplineCurve {
    pub fn new(poles: &[Pnt], knots: &[f64], mults: &[i32], degree: i32, periodic: bool) -> Result<Self>;
    pub fn with_weights(poles: &[Pnt], weights: &[f64], knots: &[f64], mults: &[i32], degree: i32, periodic: bool) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&EvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<EvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn increase_degree(&mut self, degree: i32) -> Result<()>;
    pub fn increase_multiplicity(&mut self, index: i32, m: i32) -> Result<()>;
    pub fn increase_multiplicity_range(&mut self, i1: i32, i2: i32, m: i32) -> Result<()>;
    pub fn increment_multiplicity(&mut self, i1: i32, i2: i32, step: i32) -> Result<()>;
    pub fn insert_knot(&mut self, u: f64, m: i32, parametric_tolerance: f64) -> Result<()>;
    pub fn insert_knots(&mut self, knots: &[f64], mults: &[i32], parametric_tolerance: f64, add: bool) -> Result<()>;
    pub fn remove_knot(&mut self, index: i32, m: i32, tolerance: f64) -> Result<bool>;
    pub fn segment(&mut self, u1: f64, u2: f64, tolerance: f64) -> Result<()>;
    pub fn set_knot(&mut self, index: i32, k: f64) -> Result<()>;
    pub fn set_knots(&mut self, k: &[f64]) -> Result<()>;
    pub fn set_knot_mult(&mut self, index: i32, k: f64, m: i32) -> Result<()>;
    pub fn periodic_normalization(&self, u: f64) -> f64;
    pub fn set_periodic(&mut self);
    pub fn set_origin(&mut self, index: i32) -> Result<()>;
    pub fn set_origin_param(&mut self, u: f64, tol: f64) -> Result<()>;
    pub fn set_not_periodic(&mut self);
    pub fn set_pole(&mut self, index: i32, p: Pnt) -> Result<()>;
    pub fn set_pole_weight(&mut self, index: i32, p: Pnt, weight: f64) -> Result<()>;
    pub fn set_weight(&mut self, index: i32, weight: f64) -> Result<()>;
    pub fn move_point(&mut self, u: f64, p: Pnt, index1: i32, index2: i32) -> Result<(i32, i32)>;
    pub fn move_point_and_tangent(&mut self, u: f64, p: Pnt, tangent: gp::Vec, tolerance: f64, starting: i32, ending: i32) -> Result<i32>;
    pub fn is_g1(&self, tf: f64, tl: f64, ang_tol: f64) -> Result<bool>;
    pub fn is_rational(&self) -> bool;
    pub fn degree(&self) -> i32;
    pub fn local_value(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<Pnt>;
    pub fn local_d0(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<Pnt>;
    pub fn local_d1(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<(Pnt, gp::Vec)>;
    pub fn local_d2(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<(Pnt, gp::Vec, gp::Vec)>;
    pub fn local_d3(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<(Pnt, gp::Vec, gp::Vec, gp::Vec)>;
    pub fn local_dn(&self, u: f64, from_k1: i32, to_k2: i32, n: i32) -> Result<gp::Vec>;
    pub fn first_u_knot_index(&self) -> i32;
    pub fn last_u_knot_index(&self) -> i32;
    pub fn knot(&self, index: i32) -> Result<f64>;
    pub fn knots(&self) -> &[f64];
    pub fn knot_sequence(&self) -> &[f64];
    pub fn knot_distribution(&self) -> BSplKnotDistribution;
    pub fn locate_u(&self, u: f64, parametric_tolerance: f64, with_knot_repetition: bool) -> (i32, i32);
    pub fn multiplicity(&self, index: i32) -> Result<i32>;
    pub fn multiplicities(&self) -> &[i32];
    pub fn nb_knots(&self) -> i32;
    pub fn nb_poles(&self) -> i32;
    pub fn pole(&self, index: i32) -> Result<Pnt>;
    pub fn poles(&self) -> &[Pnt];
    pub fn weight(&self, index: i32) -> Result<f64>;
    pub fn weights(&self) -> Option<&[f64]>;
    pub fn max_degree() -> i32;
    pub fn resolution(&mut self, tolerance_3d: f64) -> f64;
    pub fn is_equal(&self, other: &BSplineCurve, preci: f64) -> bool;
}
#[derive(Debug, Clone)]
pub struct TrimmedCurve { /* private */ }
impl TrimmedCurve {
    pub fn new(basis: Arc<dyn Curve>, u1: f64, u2: f64, sense: bool, adjust_periodic: bool) -> Result<Self>;
    pub fn set_trim(&mut self, u1: f64, u2: f64, sense: bool, adjust_periodic: bool) -> Result<()>;
    pub fn basis_curve(&self) -> Arc<dyn Curve>;
}
#[derive(Debug, Clone)]
pub struct OffsetCurve { /* private */ }
impl OffsetCurve {
    pub fn new(basis: Arc<dyn Curve>, offset: f64, v: Dir, not_check_c0: bool) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&EvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<EvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn set_basis_curve(&mut self, c: Arc<dyn Curve>, not_check_c0: bool) -> Result<()>;
    pub fn set_direction(&mut self, v: Dir);
    pub fn set_offset_value(&mut self, d: f64);
    pub fn basis_curve(&self) -> Arc<dyn Curve>;
    pub fn direction(&self) -> Dir;
    pub fn offset(&self) -> f64;
    pub fn get_basis_curve_continuity(&self) -> GeomAbsShape;
}

pub trait Surface: Geometry {
    fn copy_surface(&self) -> Box<dyn Surface>;
    fn u_reverse(&mut self);
    fn u_reversed(&self) -> Box<dyn Surface>;
    fn u_reversed_parameter(&self, u: f64) -> f64;
    fn v_reverse(&mut self);
    fn v_reversed(&self) -> Box<dyn Surface>;
    fn v_reversed_parameter(&self, v: f64) -> f64;
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64);
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d;
    fn bounds(&self) -> (f64, f64, f64, f64);
    fn is_u_closed(&self) -> bool;
    fn is_v_closed(&self) -> bool;
    fn is_u_periodic(&self) -> bool;
    fn u_period(&self) -> Result<f64>;
    fn is_v_periodic(&self) -> bool;
    fn v_period(&self) -> Result<f64>;
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>>;
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>>;
    fn continuity(&self) -> GeomAbsShape;
    fn is_cn_u(&self, n: i32) -> Result<bool>;
    fn is_cn_v(&self, n: i32) -> Result<bool>;
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt>;
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1>;
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2>;
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3>;
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec>;
    fn value(&self, u: f64, v: f64) -> Result<Pnt>;
}
pub trait ElementarySurface: Surface {
    fn pos(&self) -> &Ax3;
    fn pos_mut(&mut self) -> &mut Ax3;
    fn set_axis(&mut self, a: Ax1) -> Result<()>;
    fn set_location(&mut self, p: Pnt);
    fn set_position(&mut self, a: Ax3);
    fn axis(&self) -> Ax1;
    fn location(&self) -> Pnt;
    fn position(&self) -> &Ax3;
}
pub trait SweptSurface: Surface {
    fn direction(&self) -> Dir;
    fn basis_curve(&self) -> Arc<dyn Curve>;
}
pub trait BoundedSurface: Surface {}

#[derive(Debug, Clone)]
pub struct Plane { /* private */ }
impl Plane {
    pub fn from_ax3(a: Ax3) -> Self;
    pub fn from_pln(p: Pln) -> Self;
    pub fn from_point_normal(p: Pnt, v: Dir) -> Self;
    pub fn from_coefficients(a: f64, b: f64, c: f64, d: f64) -> Result<Self>;
    pub fn set_pln(&mut self, p: Pln);
    pub fn pln(&self) -> Pln;
    pub fn coefficients(&self) -> (f64, f64, f64, f64);
}
#[derive(Debug, Clone)]
pub struct CylindricalSurface { /* private */ }
impl CylindricalSurface {
    pub fn new(a: Ax3, radius: f64) -> Result<Self>;
    pub fn from_cylinder(c: Cylinder) -> Self;
    pub fn set_cylinder(&mut self, c: Cylinder);
    pub fn set_radius(&mut self, r: f64) -> Result<()>;
    pub fn cylinder(&self) -> Cylinder;
    pub fn coefficients(&self) -> [f64; 10];
    pub fn radius(&self) -> f64;
}
#[derive(Debug, Clone)]
pub struct ConicalSurface { /* private */ }
impl ConicalSurface {
    pub fn new(a: Ax3, ang: f64, radius: f64) -> Result<Self>;
    pub fn from_cone(c: Cone) -> Self;
    pub fn set_cone(&mut self, c: Cone);
    pub fn set_radius(&mut self, r: f64) -> Result<()>;
    pub fn set_semi_angle(&mut self, ang: f64) -> Result<()>;
    pub fn cone(&self) -> Cone;
    pub fn apex(&self) -> Pnt;
    pub fn coefficients(&self) -> [f64; 10];
    pub fn ref_radius(&self) -> f64;
    pub fn semi_angle(&self) -> f64;
}
#[derive(Debug, Clone)]
pub struct SphericalSurface { /* private */ }
impl SphericalSurface {
    pub fn new(a: Ax3, radius: f64) -> Result<Self>;
    pub fn from_sphere(s: Sphere) -> Self;
    pub fn set_radius(&mut self, r: f64) -> Result<()>;
    pub fn set_sphere(&mut self, s: Sphere);
    pub fn sphere(&self) -> Sphere;
    pub fn area(&self) -> f64;
    pub fn coefficients(&self) -> [f64; 10];
    pub fn radius(&self) -> f64;
    pub fn volume(&self) -> f64;
}
#[derive(Debug, Clone)]
pub struct ToroidalSurface { /* private */ }
impl ToroidalSurface {
    pub fn new(a: Ax3, major: f64, minor: f64) -> Result<Self>;
    pub fn from_torus(t: Torus) -> Self;
    pub fn set_major_radius(&mut self, r: f64) -> Result<()>;
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()>;
    pub fn set_torus(&mut self, t: Torus);
    pub fn torus(&self) -> Torus;
    pub fn area(&self) -> f64;
    pub fn coefficients(&self) -> [f64; 35];
    pub fn major_radius(&self) -> f64;
    pub fn minor_radius(&self) -> f64;
    pub fn volume(&self) -> f64;
}
#[derive(Debug, Clone)]
pub struct SurfaceOfLinearExtrusion { /* private */ }
impl SurfaceOfLinearExtrusion {
    pub fn new(c: Arc<dyn Curve>, v: Dir) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&SurfaceEvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<SurfaceEvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn set_direction(&mut self, v: Dir);
    pub fn set_basis_curve(&mut self, c: Arc<dyn Curve>);
}
#[derive(Debug, Clone)]
pub struct SurfaceOfRevolution { /* private */ }
impl SurfaceOfRevolution {
    pub fn new(c: Arc<dyn Curve>, a: Ax1) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&SurfaceEvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<SurfaceEvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn set_axis(&mut self, a: Ax1);
    pub fn set_direction(&mut self, v: Dir);
    pub fn set_basis_curve(&mut self, c: Arc<dyn Curve>);
    pub fn set_location(&mut self, p: Pnt);
    pub fn axis(&self) -> Ax1;
    pub fn location(&self) -> Pnt;
    pub fn reference_plane(&self) -> Result<Ax2>;
}

#[derive(Debug, Clone)]
pub struct BezierSurface { /* private */ }
impl BezierSurface {
    pub fn new(poles: &[Vec<Pnt>]) -> Result<Self>;
    pub fn with_weights(poles: &[Vec<Pnt>], weights: &[Vec<f64>]) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&SurfaceEvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<SurfaceEvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn exchange_uv(&mut self);
    pub fn increase(&mut self, u_deg: i32, v_deg: i32) -> Result<()>;
    pub fn insert_pole_col_after(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn insert_pole_col_after_weights(&mut self, vindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn insert_pole_col_before(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn insert_pole_col_before_weights(&mut self, vindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn insert_pole_row_after(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn insert_pole_row_after_weights(&mut self, uindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn insert_pole_row_before(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn insert_pole_row_before_weights(&mut self, uindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn remove_pole_col(&mut self, vindex: i32) -> Result<()>;
    pub fn remove_pole_row(&mut self, uindex: i32) -> Result<()>;
    pub fn segment(&mut self, u1: f64, u2: f64, v1: f64, v2: f64);
    pub fn set_pole(&mut self, uindex: i32, vindex: i32, p: Pnt) -> Result<()>;
    pub fn set_pole_weight(&mut self, uindex: i32, vindex: i32, p: Pnt, weight: f64) -> Result<()>;
    pub fn set_pole_col(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn set_pole_col_weights(&mut self, vindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn set_pole_row(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn set_pole_row_weights(&mut self, uindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn set_weight(&mut self, uindex: i32, vindex: i32, weight: f64) -> Result<()>;
    pub fn set_weight_col(&mut self, vindex: i32, weights: &[f64]) -> Result<()>;
    pub fn set_weight_row(&mut self, uindex: i32, weights: &[f64]) -> Result<()>;
    pub fn nb_u_poles(&self) -> i32;
    pub fn nb_v_poles(&self) -> i32;
    pub fn pole(&self, uindex: i32, vindex: i32) -> Result<Pnt>;
    pub fn poles(&self) -> &[Vec<Pnt>];
    pub fn u_degree(&self) -> i32;
    pub fn v_degree(&self) -> i32;
    pub fn weight(&self, uindex: i32, vindex: i32) -> Result<f64>;
    pub fn weights(&self) -> Option<&[Vec<f64>]>;
    pub fn is_u_rational(&self) -> bool;
    pub fn is_v_rational(&self) -> bool;
    pub fn max_degree() -> i32;
    pub fn resolution(&mut self, tolerance_3d: f64) -> (f64, f64);
    pub fn u_knots(&self) -> Vec<f64>;
    pub fn v_knots(&self) -> Vec<f64>;
    pub fn u_multiplicities(&self) -> Vec<i32>;
    pub fn v_multiplicities(&self) -> Vec<i32>;
    pub fn u_knot_sequence(&self) -> Vec<f64>;
    pub fn v_knot_sequence(&self) -> Vec<f64>;
}
#[derive(Debug, Clone)]
pub struct BSplineSurface { /* private */ }
impl BSplineSurface {
    pub fn new(poles: &[Vec<Pnt>], uknots: &[f64], vknots: &[f64], umults: &[i32], vmults: &[i32], udegree: i32, vdegree: i32, uperiodic: bool, vperiodic: bool) -> Result<Self>;
    pub fn with_weights(poles: &[Vec<Pnt>], weights: &[Vec<f64>], uknots: &[f64], vknots: &[f64], umults: &[i32], vmults: &[i32], udegree: i32, vdegree: i32, uperiodic: bool, vperiodic: bool) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&SurfaceEvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<SurfaceEvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn exchange_uv(&mut self);
    pub fn set_u_periodic(&mut self);
    pub fn set_v_periodic(&mut self);
    pub fn periodic_normalization(&self, u: f64, v: f64) -> (f64, f64);
    pub fn set_u_origin(&mut self, index: i32) -> Result<()>;
    pub fn set_v_origin(&mut self, index: i32) -> Result<()>;
    pub fn set_u_not_periodic(&mut self);
    pub fn set_v_not_periodic(&mut self);
    pub fn increase_degree(&mut self, u_degree: i32, v_degree: i32) -> Result<()>;
    pub fn insert_u_knots(&mut self, knots: &[f64], mults: &[i32], parametric_tolerance: f64, add: bool) -> Result<()>;
    pub fn insert_v_knots(&mut self, knots: &[f64], mults: &[i32], parametric_tolerance: f64, add: bool) -> Result<()>;
    pub fn remove_u_knot(&mut self, index: i32, m: i32, tolerance: f64) -> Result<bool>;
    pub fn remove_v_knot(&mut self, index: i32, m: i32, tolerance: f64) -> Result<bool>;
    pub fn increase_u_multiplicity(&mut self, uindex: i32, m: i32) -> Result<()>;
    pub fn increase_u_multiplicity_range(&mut self, from_i1: i32, to_i2: i32, m: i32) -> Result<()>;
    pub fn increment_u_multiplicity(&mut self, from_i1: i32, to_i2: i32, step: i32) -> Result<()>;
    pub fn increase_v_multiplicity(&mut self, vindex: i32, m: i32) -> Result<()>;
    pub fn increase_v_multiplicity_range(&mut self, from_i1: i32, to_i2: i32, m: i32) -> Result<()>;
    pub fn increment_v_multiplicity(&mut self, from_i1: i32, to_i2: i32, step: i32) -> Result<()>;
    pub fn insert_u_knot(&mut self, u: f64, m: i32, parametric_tolerance: f64, add: bool) -> Result<()>;
    pub fn insert_v_knot(&mut self, v: f64, m: i32, parametric_tolerance: f64, add: bool) -> Result<()>;
    pub fn segment(&mut self, u1: f64, u2: f64, v1: f64, v2: f64, u_tolerance: f64, v_tolerance: f64) -> Result<()>;
    pub fn check_and_segment(&mut self, u1: f64, u2: f64, v1: f64, v2: f64, u_tolerance: f64, v_tolerance: f64) -> Result<()>;
    pub fn set_u_knot(&mut self, uindex: i32, k: f64) -> Result<()>;
    pub fn set_u_knots(&mut self, uk: &[f64]) -> Result<()>;
    pub fn set_u_knot_mult(&mut self, uindex: i32, k: f64, m: i32) -> Result<()>;
    pub fn set_v_knot(&mut self, vindex: i32, k: f64) -> Result<()>;
    pub fn set_v_knots(&mut self, vk: &[f64]) -> Result<()>;
    pub fn set_v_knot_mult(&mut self, vindex: i32, k: f64, m: i32) -> Result<()>;
    pub fn locate_u(&self, u: f64, parametric_tolerance: f64, with_knot_repetition: bool) -> (i32, i32);
    pub fn locate_v(&self, v: f64, parametric_tolerance: f64, with_knot_repetition: bool) -> (i32, i32);
    pub fn set_pole(&mut self, uindex: i32, vindex: i32, p: Pnt) -> Result<()>;
    pub fn set_pole_weight(&mut self, uindex: i32, vindex: i32, p: Pnt, weight: f64) -> Result<()>;
    pub fn set_pole_col(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn set_pole_col_weights(&mut self, vindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn set_pole_row(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()>;
    pub fn set_pole_row_weights(&mut self, uindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()>;
    pub fn set_weight(&mut self, uindex: i32, vindex: i32, weight: f64) -> Result<()>;
    pub fn set_weight_col(&mut self, vindex: i32, weights: &[f64]) -> Result<()>;
    pub fn set_weight_row(&mut self, uindex: i32, weights: &[f64]) -> Result<()>;
    pub fn move_point(&mut self, u: f64, v: f64, p: Pnt, uindex1: i32, uindex2: i32, vindex1: i32, vindex2: i32) -> Result<(i32, i32, i32, i32)>;
    pub fn is_u_rational(&self) -> bool;
    pub fn is_v_rational(&self) -> bool;
    pub fn first_u_knot_index(&self) -> i32;
    pub fn first_v_knot_index(&self) -> i32;
    pub fn last_u_knot_index(&self) -> i32;
    pub fn last_v_knot_index(&self) -> i32;
    pub fn nb_u_knots(&self) -> i32;
    pub fn nb_u_poles(&self) -> i32;
    pub fn nb_v_knots(&self) -> i32;
    pub fn nb_v_poles(&self) -> i32;
    pub fn pole(&self, uindex: i32, vindex: i32) -> Result<Pnt>;
    pub fn poles(&self) -> &[Vec<Pnt>];
    pub fn u_degree(&self) -> i32;
    pub fn v_degree(&self) -> i32;
    pub fn u_knot(&self, uindex: i32) -> Result<f64>;
    pub fn v_knot(&self, vindex: i32) -> Result<f64>;
    pub fn u_knot_distribution(&self) -> BSplKnotDistribution;
    pub fn v_knot_distribution(&self) -> BSplKnotDistribution;
    pub fn u_knots(&self) -> &[f64];
    pub fn v_knots(&self) -> &[f64];
    pub fn u_knot_sequence(&self) -> &[f64];
    pub fn v_knot_sequence(&self) -> &[f64];
    pub fn u_multiplicity(&self, uindex: i32) -> Result<i32>;
    pub fn v_multiplicity(&self, vindex: i32) -> Result<i32>;
    pub fn u_multiplicities(&self) -> &[i32];
    pub fn v_multiplicities(&self) -> &[i32];
    pub fn weight(&self, uindex: i32, vindex: i32) -> Result<f64>;
    pub fn weights(&self) -> Option<&[Vec<f64>]>;
    pub fn local_d0(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32) -> Result<Pnt>;
    pub fn local_d1(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32) -> Result<(Pnt, gp::Vec, gp::Vec)>;
    pub fn local_d2(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32) -> Result<(Pnt, gp::Vec, gp::Vec, gp::Vec, gp::Vec, gp::Vec)>;
    pub fn local_d3(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32) -> Result<SurfResD3>;
    pub fn local_dn(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32, nu: i32, nv: i32) -> Result<gp::Vec>;
    pub fn local_value(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32) -> Result<Pnt>;
    pub fn u_iso_check(&self, u: f64, check_rational: bool) -> Result<BSplineCurve>;
    pub fn v_iso_check(&self, v: f64, check_rational: bool) -> Result<BSplineCurve>;
    pub fn max_degree() -> i32;
    pub fn resolution(&mut self, tolerance_3d: f64) -> (f64, f64);
}
#[derive(Debug, Clone)]
pub struct RectangularTrimmedSurface { /* private */ }
impl RectangularTrimmedSurface {
    pub fn new(s: Arc<dyn Surface>, u1: f64, u2: f64, v1: f64, v2: f64, usense: bool, vsense: bool) -> Result<Self>;
    pub fn new_param(s: Arc<dyn Surface>, param1: f64, param2: f64, utrim: bool, sense: bool) -> Result<Self>;
    pub fn set_trim(&mut self, u1: f64, u2: f64, v1: f64, v2: f64, usense: bool, vsense: bool) -> Result<()>;
    pub fn set_trim_param(&mut self, param1: f64, param2: f64, utrim: bool, sense: bool) -> Result<()>;
    pub fn basis_surface(&self) -> Arc<dyn Surface>;
}
#[derive(Debug, Clone)]
pub struct OffsetSurface { /* private */ }
impl OffsetSurface {
    pub fn new(s: Arc<dyn Surface>, offset: f64, not_check_c0: bool) -> Result<Self>;
    pub fn has_eval_representation(&self) -> bool;
    pub fn eval_representation(&self) -> Option<&SurfaceEvalRepDesc>;
    pub fn set_eval_representation(&mut self, d: Option<SurfaceEvalRepDesc>) -> Result<()>;
    pub fn clear_eval_representation(&mut self);
    pub fn set_basis_surface(&mut self, s: Arc<dyn Surface>, not_check_c0: bool) -> Result<()>;
    pub fn set_offset_value(&mut self, d: f64);
    pub fn offset(&self) -> f64;
    pub fn basis_surface(&self) -> Arc<dyn Surface>;
    pub fn surface(&self) -> Option<Arc<dyn Surface>>;
    pub fn u_osculating_surface(&self, u: f64, v: f64) -> Option<(bool, BSplineSurface)>;
    pub fn v_osculating_surface(&self, u: f64, v: f64) -> Option<(bool, BSplineSurface)>;
    pub fn get_basis_surf_continuity(&self) -> GeomAbsShape;
}
```

| item | C++ and traits |
|---|---|
| `CartesianPoint` : `Geometry`, `Point` | `Geom_CartesianPoint` (`from_pnt` = `Geom_CartesianPoint(gp_Pnt)`, `new(x, y, z)`) |
| `Direction` : `Geometry`, `Vector` | `Geom_Direction`; `new(x, y, z)` is the `(X, Y, Z)` constructor (normalizes), `from_dir` the `gp_Dir` one; `crossed` / `cross_crossed` of a direction return a `Direction` |
| `VectorWithMagnitude` : `Geometry`, `Vector` | `Geom_VectorWithMagnitude`; `from_vec`, `new(x, y, z)`, `from_points(p1, p2)` are its three constructors; `added(&other)` is `this + Other`, `subtracted` is `this - Other`; `crossed` / `cross_crossed` return a `VectorWithMagnitude` |
| `Axis1Placement` : `Geometry`, `AxisPlacement` | `Geom_Axis1Placement` (`new(Ax1)`, `from_point_direction(P, V)`) |
| `Axis2Placement` : `Geometry`, `AxisPlacement` | `Geom_Axis2Placement` (`new(Ax2)`, `from_point_directions(P, N, Vx)` is the three argument constructor, which builds a `gp_Ax2(P, N, Vx)`: an error when `N` and `Vx` are parallel) |
| `Transformation` | `Geom_Transformation` (`new()` is the identity, `from_trsf`); `set_transformation(to)` / `set_transformation_between(from, to)` take `gp::Ax3` |
| `Line` : `Geometry`, `Curve` | `Geom_Line` (`from_ax1`, `from_lin`, `new(P, V)`); no `distance` method exists in 3D |
| `Circle`, `Ellipse`, `Hyperbola`, `Parabola` : `Geometry`, `Curve`, `Conic` | `Geom_Circle` (`from_circ`, `new(Ax2, r)`), `Geom_Ellipse` (`from_elips`, `new(Ax2, a, b)`), `Geom_Hyperbola` (`from_hypr`, `new(Ax2, a, b)`), `Geom_Parabola` (`from_parab`, `new(Ax2, f)`, `from_directrix_focus(Ax1, Pnt)`); there is no sense flag in 3D |
| `BezierCurve`, `BSplineCurve`, `TrimmedCurve` : `Geometry`, `Curve`, `BoundedCurve` | `Geom_BezierCurve`, `Geom_BSplineCurve` (the constructor with weights has `CheckRational = true`), `Geom_TrimmedCurve` (the constructor's default arguments are explicit in `new`) |
| `OffsetCurve` : `Geometry`, `Curve` | `Geom_OffsetCurve(C, Offset, V, isNotCheckC0)` |
| `Plane`, `CylindricalSurface`, `ConicalSurface`, `SphericalSurface`, `ToroidalSurface` : `Geometry`, `Surface`, `ElementarySurface` | `Geom_Plane` (`from_ax3`, `from_pln`, `from_point_normal`, `from_coefficients`), `Geom_CylindricalSurface` (`new(Ax3, r)`, `from_cylinder`), `Geom_ConicalSurface` (`new(Ax3, angle, r)`, `from_cone`), `Geom_SphericalSurface` (`new(Ax3, r)`, `from_sphere`), `Geom_ToroidalSurface` (`new(Ax3, R, r)`, `from_torus`) |
| `SurfaceOfLinearExtrusion`, `SurfaceOfRevolution` : `Geometry`, `Surface`, `SweptSurface` | `Geom_SurfaceOfLinearExtrusion(C, V)`, `Geom_SurfaceOfRevolution(C, Ax1)` |
| `BezierSurface`, `BSplineSurface`, `RectangularTrimmedSurface` : `Geometry`, `Surface`, `BoundedSurface` | `Geom_BezierSurface`, `Geom_BSplineSurface`, `Geom_RectangularTrimmedSurface` (`new` = the constructor with the four parameters and two senses, `new_param` = the one with `Param1`, `Param2`, `UTrim`, `Sense`) |
| `OffsetSurface` : `Geometry`, `Surface` | `Geom_OffsetSurface(S, Offset, isNotCheckC0)`; `surface()` is `Surface()` |

### Behaviour that is easy to miss (each is exercised by the golden file)

* **Geometry.** `Mirror(Ax2)` is the mirror about the plane through the location of the `Ax2` with its main direction (`Trsf::set_mirror(&Ax2)`); `Translate(P1, P2)` translates by
  `gp_Vec(P1, P2)`. A `Direction` is transformed with `gp_Dir::Transform` (renormalized, reversed by a negative scale, untouched by a translation); a
  `VectorWithMagnitude` with `gp_Vec::Transform` (translations do nothing, scales multiply); a `CartesianPoint` with `gp_Pnt::Transform`; an `Axis1Placement` /
  `Axis2Placement` with `gp_Ax1::Transform` / `gp_Ax2::Transform`. The `...ed` copies are `Copy()` followed by the in-place method. `Geometry::copy_geometry` of a `Direction` is
  `Geom_Direction(gp_Dir(gpVec))`.
* **Vectors.** `Vector::reversed` is `Copy()` followed by `Reverse()` (the result has the type of the object). `Direction::cross` etc. normalize the result through `gp_Dir`;
  `VectorWithMagnitude::cross` etc. are the plain `gp_Vec` operations. `Vector::angle` is `gp_Vec::Angle` (always in `[0, pi]`), `angle_with_ref` is `gp_Vec::AngleWithRef`.
* **Axis placements.** `Geom_Axis2Placement` does not hold a `gp_Ax2`: it stores the axis (location and main direction) and the two directions `vxdir` and `vydir` separately, and only the following
  methods keep them consistent. `Axis2Placement::new(Ax2)` / `set_ax2` copy the three directions of the `Ax2`; `from_point_directions(P, N, Vx)` stores `N`, `vxdir = N.CrossCrossed(Vx, N)`,
  `vydir = N.Crossed(vxdir)` (parallel `N`, `Vx` is an unchecked case of OCCT 8.0.1: the oracle drops it, return `Err`); `set_direction(V)` (`AxisPlacement`) sets the axis direction, then `vxdir = V.CrossCrossed(vxdir, V)`,
  `vydir = V.Crossed(vxdir)`; `set_x_direction(Vx)`: `vxdir = axis.Direction().CrossCrossed(Vx, axis.Direction())`, `vydir = axis.Direction().Crossed(vxdir)`; `set_y_direction(Vy)`: `vxdir = Vy.Crossed(axis.Direction())`,
  `vydir = axis.Direction().Crossed(vxdir)` (these are *not* the `gp_Ax2` setters; a direction parallel to the axis is unchecked: `Err`). `AxisPlacement::set_axis` and `set_location` of either class only replace the
  axis / the location (the x and y directions of an `Axis2Placement` are left as they are, so after `set_axis` they need not be orthogonal to the new direction). `ax2()` builds `gp_Ax2(location, direction, vxdir)`
  and therefore returns `Err(Construction)` when `vxdir` is parallel to the direction (it returns a `Result`; the oracle prints the state through it). `transform` transforms the location, `vxdir` and `vydir`
  with `Pnt::transformed` / `Dir::transform` and sets the axis direction to `vxdir.Crossed(vydir)`. `copy_geometry` copies the three directions. For an `Axis1Placement` `set_direction` replaces the direction, `reverse`
  reverses it; `angle` is `gp_Ax1::Angle` of the axes.
* **Conics.** `Conic::reverse` (shared C++ body) is `pos.SetDirection(-pos.Direction())`: `gp_Ax2::SetDirection` recomputes the x direction as the double cross product and the y direction as `N x X`.
  `reversed_parameter` is `2 pi - u` for the circle and the ellipse, `-u` for the others; `first_parameter` / `last_parameter` are `0` / `2 pi` and `-/+ Precision::Infinite()`;
  `is_closed` and `is_periodic` are true for the circle and the ellipse only. `transform` multiplies the radii by `|scale_factor|` and transforms the position (`Ax2::transform`).
  `Line` and `Parabola` override `transformed_parameter` (`u` if infinite, else `u * |scale|`) and `parametric_transformation` (`|scale|`). `set_axis(Ax1)` is `gp_Ax2::SetAxis`.
* **Bezier curves.** parameters are `0 .. 1`; `reversed_parameter(u) = 1 - u`; `is_closed` is a stored flag (set by every pole change as the C++ does: `init`, `set_pole` of the first or
  last pole, `segment`); `init` (called by `increase`, `insert_pole_*`, `remove_pole` and the constructors) and `transform` clear the evaluation representation; `segment`, `set_pole`, `set_weight`, `reverse` do not.
  `insert_pole_after(i, p)` is `insert_pole_after_weight(i, p, 1.0)`. All the range and weight checks of the Bezier curve are explicit `throw`s (an `Err`, never `unchecked`).
  `resolution(tol)` caches `MaxDerivInv` and every pole change invalidates it.
* **B-spline curves.** `reversed_parameter(u) = first + last - u`; `is_closed` compares the end points with `Precision::Computational()` (squared distance); `continuity` is the
  stored `mySmooth` computed by `updateKnots` (note the `KnotAnalysis` quirks: `BSplCLib::KnotForm` compares the *absolute values* of the knots, so knots that straddle zero are
  `NonUniform`; a degree 1 curve with end multiplicity 2 and simple inner knots is `PiecewiseBezier`, not `QuasiUniform`); `set_knot` checks the neighbours with `Epsilon`; `segment`
  of a periodic curve first inserts the end knots, then `set_origin` and `set_not_periodic`; `set_origin_param(u, tol)` (the overload `SetOrigin(double, double)`) normalizes `u`
  into the period (re-parametrizing the curve by `delta` when `u` is further than `tol` from the period), inserts a knot at `u` when no knot is within `tol`, then calls
  `set_origin(index)`; `move_point_and_tangent` unperiodizes a periodic curve first (even when it then reports an error). **Unlike the 2D curve the 3D B-spline curve clears its
  evaluation representation** in `increase_degree`, `insert_knots`, `remove_knot`, `segment`, `set_knot*`, `set_periodic`, `set_origin*`, `set_not_periodic`, `set_pole*`, `set_weight`,
  `move_point`, `move_point_and_tangent` and `reverse` (see the `ClearEvalRepresentation` calls of `Geom_BSplineCurve.cxx`); `transform` does not. `is_equal(other, preci)`
  compares degree, periodicity, rationality, the knots, multiplicities, poles and weights (poles by `Pnt::distance <= preci`, the others by differences `<= preci`; see `Geom_BSplineCurve::IsEqual`).
  The copy constructor clones the representation. There is no pole insertion or removal on the 3D B-spline curve.
* **Trimmed curves.** The constructor replaces a trimmed basis by a copy of *its* basis; `set_trim` on a periodic basis keeps the parameters as they are or adjusts them with
  `ElCLib::AdjustPeriodic` (`adjust_periodic`), on a non periodic basis sorts them (flipping `sense`) and raises `Construction` when `u1 == u2` or a parameter lies further than
  `Precision::PConfusion()` outside the basis range; a reversed sense reverses the curve. `is_periodic` / `is_closed` use `std::remainder` of the trimmed length by the basis period.
* **Offset curves.** `set_basis_curve` clears the evaluation representation, unwraps trimmed and offset bases (`offset += basis.offset`), records the basis continuity
  (`get_basis_curve_continuity`), raises `Construction` ("Offset on C0 curve") for a C0 B-spline that is not `IsG1(first, last, Precision::Angular())` unless `not_check_c0`, and
  rewraps in a `TrimmedCurve` over a copy of the unwrapped basis. The evaluation functions follow `Geom_OffsetCurveUtils.pxx` (the offset is along `basis'(u) x direction`, the singular-point
  handling with the step `1e-7`, the factor `1e-3` ...); a singular point that cannot be handled raises `UndefinedValue` (`EvalD0`) or `UndefinedDerivative` (`EvalD1`..`EvalDN`).
  `reverse` (reverses the basis curve and negates the offset value; the direction is kept), `transform` (`offset *= |scale|`, the direction is transformed), `set_direction`,
  `set_offset_value` and `set_basis_curve` clear the evaluation representation.
* **Surfaces.** `ElementarySurface::u_reverse` is `Ax3::y_reverse` and `v_reverse` is `Ax3::z_reverse` of the position (a left-handed frame results); `Plane` overrides them with `x_reverse` / `y_reverse`; `ConicalSurface::v_reverse`
  additionally negates the semi-angle. The reversed parameters: `2 pi - u` and `-v` for the cylinder, cone and sphere, `2 pi - u` and `2 pi - v` for the torus, `-u` and `-v` for the plane; the swept surfaces: the
  revolution reverses its direction in `u_reverse` and its basis curve in `v_reverse` (`2 pi - u`, `basis.reversed_parameter(v)`), the extrusion the basis curve in `u_reverse` (`basis.reversed_parameter(u)`) and its direction in
  `v_reverse` (`-v`), and both clear the evaluation representation. `transform` of an elementary surface multiplies the radii by `|scale_factor|` and transforms the position.
  `Surface::transform_parameters(u, v, t)` and `Surface::parametric_transformation(t)` are the defaults of `Geom_Surface` unless a class overrides them as the listing and the golden file show (`surface_xform`).
  `UIso` / `VIso` of the elementary surfaces are a `Line` or `Circle` (the iso of the sphere in V is a circle, of the cone a line or a circle, of the torus circles), of the swept surfaces a transformed copy of the
  basis curve or a line, of the Bezier / B-spline surfaces a Bezier / B-spline curve. Bounds, periodicity and singular points (poles, apex, `v = +-pi/2`) are those of the C++ classes. The coefficient methods are the `gp` ones.
* **Bezier and B-spline surfaces.** The pole array is indexed `(UIndex, VIndex)`; `exchange_uv` transposes everything (degrees, knots, rationality flags, `u`/`v` periodicity);
  `u_reverse` / `v_reverse` reverse the poles, weights, knots and multiplicities in that direction. The weights are shared by both directions: a surface is `u_rational` /
  `v_rational` when the weights are not constant along that direction. `segment` of a periodic surface first unperiodizes, `check_and_segment` additionally clips the parameters to
  the surface bounds. The B-spline surface clears its evaluation representation in `exchange_uv`, `increase_degree`, `set_u_knot(s)`, `set_v_knot(s)`, `set_weight`, `set_weight_col`, `set_weight_row` and the private `segment`; the other editing methods
  (`set_pole*`, the periodicity setters, `insert_*`, `remove_*`, `increase_*_multiplicity`, `move_point`) do not, as in `Geom_BSplineSurface.cxx`. The Bezier surface clears it in `init` (`increase`, `insert_*`, `remove_*`, `segment`, the constructors), in `transform`, `u_reverse` and `v_reverse`.
* **Rectangular trimmed surfaces.** The constructor (both forms) unwraps a trimmed basis (a copy of *its* basis is used), checks `u1 == u2`, `v1 == v2` and the parameters outside a
  non periodic basis (`Construction`), and adjusts the parameters of a periodic direction into the period (`ElCLib::AdjustPeriodic`); `bounds()` is the trimmed rectangle.
* **Offset surfaces.** The constructor unwraps a trimmed or offset basis (adding the offsets), checks that the basis is at least C1 (`Construction`, "Offset with no C1 Surface", for a C0
  B-spline or Bezier basis that is not G1 in both directions, unless `not_check_c0`), sets `get_basis_surf_continuity`, builds the osculating surface (`Geom_OsculatingSurface`, for B-spline and Bezier bases) and creates the
  *full evaluation representation* of `Surface()` (so `has_eval_representation()` is true for every offset surface whose `Surface()` is not null; `set_offset_value`, `set_basis_surface`, `transform`, `u_reverse`
  and `v_reverse` recompute or clear it). The evaluation functions follow `Geom_OffsetSurfaceUtils.pxx`; at points where the normal is not defined (poles of a sphere, the apex of a cone, points of a revolution surface on the
  axis, collapsed Bezier rows) the osculating surface (the surface `u_osculating_surface` / `v_osculating_surface` returns, a presence flag, then `(opposite, surface)`) is used; when it cannot be used
  `UndefinedValue` / `UndefinedDerivative` is raised. `surface()` (`Geom_OffsetSurface::Surface`) returns `None` for a null result: the basis itself when the offset is 0; for a plane the translated plane; for a
  cylinder, sphere, cone and torus an analytic surface of the adjusted radius (a negative radius flips the x axis, or x and z for the sphere; a radius below `Precision::Confusion()` and a self-intersecting torus give
  `None`; the cone keeps a non-negative radius and moves its apex), re-trimmed with the bounds of a trimmed basis; for every other basis (swept, Bezier, B-spline, offset) `None`. The
  `OffsetSurface::u_iso` / `v_iso` approximation through `AdvApprox` is outside the provided sources and is not judged.

## Evaluation representations (`GeomEval_RepCurveDesc`, `GeomEval_RepSurfaceDesc`, `GeomEval_RepUtils`)

`BezierCurve`, `BSplineCurve`, `OffsetCurve` (curve descriptors) and `BezierSurface`, `BSplineSurface`, `SurfaceOfLinearExtrusion`, `SurfaceOfRevolution`, `OffsetSurface` (surface descriptors)
can carry an evaluation representation: a curve or surface that `eval_d0` .. `eval_dn` use instead of their own algorithm when the descriptor can answer. The C++ descriptor classes are
the structs `EvalRepDesc` and `SurfaceEvalRepDesc`:

```text
EvalRepDesc        { kind: Full | DerivBounded | Mapped, representation: Option<Arc<dyn Curve>>, max_deriv_order: usize,
                     domain: Option<(f64, f64)>, scale: f64, offset: f64 }               // Map1d { Scale, Offset }, Domain1d { First, Last }
SurfaceEvalRepDesc { kind, representation: Option<Arc<dyn Surface>>, max_deriv_order: usize,
                     domain: Option<(f64, f64, f64, f64)>,                               // Domain2d { UFirst, ULast, VFirst, VLast }
                     scale_u, offset_u, scale_v, offset_v: f64, swap_uv: bool }          // Map2d { ScaleU, OffsetU, ScaleV, OffsetV, SwapUV }
```

(the `Full` / `DerivBounded` descriptors have `scale 1`, `offset 0`, no domain, no swap). The functions of `GeomEval_RepUtils` are public in the module `geom::rep_utils`
(the objects use them; they are tested through the objects): `clone_curve_desc` / `clone_surface_desc` (deep: the representation is copied with `copy_curve` / `copy_surface`; a
descriptor without representation clones to `None`), `try_eval_curve_d0/d1/d2/d3/dn` and `try_eval_surface_d0/d1/d2/d3/dn` (`None` = C++ `false`: no descriptor, order above
`max_deriv_order`, `n < 1` for curves, or the mapped parameter outside the domain) and the `..._mapped` variants (derivatives are multiplied by the powers of the scales and, for a swapped
map, the U and V derivatives exchanged, exactly as the C++ does), `validate_curve_desc` / `validate_surface_desc(desc, owner) -> Result<()>`. `Map1d::IsValid` is `|scale| > Precision::Confusion()`,
`Map2d::IsValid` needs both scales; `Domain1d::Contains(u)` is `first - Confusion <= u <= last + Confusion` (the 2D domain likewise in both directions).

`set_eval_representation(Some(desc))` calls the validation: `Program` error when `representation` is `None` or the kind is `Mapped` with an invalid map (the third C++ check, "descriptor chain":
the representation is the owner itself, cannot happen with an `Arc` and is not tested). `set_eval_representation(None)` clears. The descriptor does not copy the representation when it is set;
`copy_geometry` (`CloneCurveDesc`) does. `eval_representation()` returns the descriptor, `has_eval_representation()` is `eval_representation().is_some()`.

## Helpers the translation will need

`src/bsplclib` and `src/bsplslib` implement the 3D (`Pnt`) forms and the dimension-generic forms of the B-spline algorithms (`d0`..`dn`, `build_cache`, `insert_knots`, `remove_knot`,
`increase_degree`, `unperiodize`, `prepare_insert_knots`, `prepare_unperiodize`, `knot_analysis`, `max_knot_mult`, `locate_parameter*`, `move_point`, `move_point_and_tangent`, `resolution`, `reverse_*`,
the surface `iso`, `d*_local`, `rational_derivative`, ...); `src/elclib` and `src/elslib` have the value / derivative / iso functions of the conics and elementary surfaces; `src/plib` the
polynomial helpers (`GeomAbsShape`, `coefficients_poles_3d`, `trimming_3d`, `set_poles_3d`, `get_poles_3d`); `src/convert` the conversions used by the offset objects. They are complete where they
are used and must be reused, not copied. What is missing in them (for example the segmenting and knot-removal drivers that `Geom_BSplineSurface` calls on `BSplSLib`, `BSplCLib::KnotForm` /
`MultForm`, `BSplCLib::FlatIndex`) must be added there (public, documented in the API document of that module), never copied into `geom`; the existing functions' APIs do not change.
`geom2d` has the 2D versions of the curve classes and of the representation machinery (`rep_utils`): the structure of `geom` follows it. The classes `Geom_OsculatingSurface`
(`Geom_OsculatingSurface.cxx/.pxx`), `Geom_OffsetCurveUtils.pxx`, `Geom_OffsetSurfaceUtils.pxx`, `Geom_ExtrusionUtils.pxx` and `Geom_RevolutionUtils.pxx` are part of the translation (private
submodules of `geom`).

# The reference file

## Record format

`tests/geom.txt` has one record per line: `<family> <inputs...> | <outputs...>`. Floats are 16-hex-digit `f64` bit patterns, integers and booleans decimal. **Every input is on the left of
` | `**, in the order of the family's schema; a schema is a list of words:

| word | tokens | meaning |
|---|---|---|
| `f` | 1 | a float |
| `i` | 1 | an integer |
| `tr` | 1 + (parameters of the kind) | a transformation recipe: kind (integer), then the parameters of the kind |
| `crv` | variable | a curve recipe (recursive for trimmed and offset curves) |
| `srf` | variable | a surface recipe (recursive for swept, trimmed and offset surfaces) |
| `ops` | variable | a sequence of operations on the object: `n`, then per operation `code`, `na`, `na` floats |

The oracle aborts if the tokens it prints differ from the schema; `tests/geom.rs` has a reader method per word (`Rd::f`, `i`, `tr`, `crv`, `srf`, `ops`) and `Rd::finish` asserts that every
input word was read. There is no text token. A line whose operations print nothing ends with ` |`. The schemas of the families:

| family group | schema |
|---|---|
| `cpoint_*` | `i f f f ops` (constructor, x, y, z) |
| `direction_*`, `vecmag_*` | `i i f f f f f f ops` (kind, constructor, x y z x2 y2 z2) |
| `axis1_*` | `i f f f f f f ops` (constructor, location, direction); `axis2_*`: `i f f f f f f f f f ops` (constructor, location, normal, x direction) |
| `trsf_*` | `tr ops` |
| curve families | `crv ops`; with a second curve `crv crv ops` (`bspline_is_equal`, `offset_set`, `evalrep_curve_*`) |
| surface families | `srf ops`; with a curve `srf crv ops` (`extrusion_ops`, `revolution_ops`); with a second surface `srf srf ops` (`offsurf_set`, `evalrep_surf_*`) |

The outputs are the values the operations return, in order. If one of the operations raises an exception the whole output is `throws`; if it is one of the unchecked inputs listed above the whole
output is `unchecked` (the operation is not executed, and neither are the following ones). The Rust replay must return `Err` at that operation (or earlier); it is compared only as "Err or not".
The states of the planned lines are collapsed: a curve or surface line prints the state of the object once, at the end (the planner removes the earlier state operations).

### `tr` (transformation recipe)

Kind and parameters `p0 ..`: `0` identity (no parameters); `1` mirror about the point `(p0, p1, p2)` (`SetMirror(gp_Pnt)`); `2` mirror about the axis through `(p0, p1, p2)` with direction `(p3, p4, p5)`
(`SetMirror(gp_Ax1)`); `3` mirror about the plane through `(p0, p1, p2)` with normal `(p3, p4, p5)` (`SetMirror(gp_Ax2(P, N))`); `4` rotation about the axis `(p0 .. p5)` by `p6` (`SetRotation`); `5` scale
about `(p0, p1, p2)` by `p3` (`SetScale`; negative factors occur; zero occurs only in the `trsf_*` families); `6` `SetTransformation(to)` with the frame `(p0 .. p9)`; `7` `SetTransformation(from, to)`
with the frames `(p0 .. p9)` and `(p10 .. p19)`; `8` translation by the vector `(p0, p1, p2)`; `9` translation from `(p0, p1, p2)` to `(p3, p4, p5)`; `10` `gp_Trsf::SetValues(p0 .. p11)` (row-major
`a11 a12 a13 a14 a21 ...`; a matrix with `|det| < gp::resolution()` is unchecked in `gp`, a non orthogonal one is accepted by OCCT and gives form `CompoundTrsf`); `11` `SetDisplacement(from, to)` with
the two frames. A **frame** is ten floats `(ox oy oz nx ny nz xx xy xz flip)`: the `gp_Ax3(P, N, Vx)` built from the point, the raw main direction and the raw x direction (both are normalized by `Dir::new`, `Vx`
need not be orthogonal to `N`), reversed in Y (`y_reverse`: a left-handed frame) when `flip` is not 0. The directions given as raw triples are normalized by `Dir::new`. The recipe is applied with
`Geom_Transformation::Set*` in the transformation families (kinds 10 and 11 through `set_trsf`) and as a `gp_Trsf` (`Trsf::set_*`) to `transform`, `transformed_parameter`, ... .

### `crv` (curve recipe)

```text
crv := kind                                                           (0 line 1 circle 2 ellipse 3 hyperbola 4 parabola 5 bezier 6 bspline 7 trimmed 8 offset)
kind 0 (line):  ctor  ox oy oz  dx dy dz                              ctor 0 Line::from_ax1, 1 from_lin, 2 new(P, V)
kind 1..3:      ctor  ox oy oz  nx ny nz  xx xy xz  r | a b           the Ax2(P, N, Vx) of the conic; ctor 0 the gp_ object (Circ, Elips, Hypr), 1 the (Ax2, radii) constructor
kind 4:         ctor  <9 floats>  [f]                                 ctor 0 Parab, 1 (Ax2, f): the frame o n x then f; ctor 2 (directrix Ax1, focus): o d focus (nine floats, no f)
kind 5 (Bezier):  n  n*(x y z)  rat  [nw  nw*w]                       rat = 1: the constructor with the weights (nw floats; nw may differ from n: an invalid variant)
kind 6 (B-spline): degree periodic  nk  nk*knot  nm  nm*mult  np  np*(x y z)  rat  [nw  nw*w]
kind 7 (trimmed):  u1 u2 sense adjust  crv                            basis curve = the nested crv
kind 8 (offset):   offset notcheck  dx dy dz  crv                     (dx, dy, dz) the offset direction (raw)
```

A constructor that raises makes the whole line `throws` (the gp object is built first).

### `srf` (surface recipe)

```text
srf := kind                    (0 plane 1 cylinder 2 cone 3 sphere 4 torus 5 bezier 6 bspline 7 linear extrusion 8 revolution 9 rectangular trimmed 10 offset)
kind 0:  ctor  <frame>                  ctor 0 Plane::from_ax3, 1 from_pln(Pln::new(frame)); ctor 2: o n (six floats) from_point_normal; ctor 3: A B C D from_coefficients
kind 1:  ctor  <frame>  r               ctor 0 CylindricalSurface::new(frame, r), 1 from_cylinder(Cylinder::new(frame, r))
kind 2:  ctor  <frame>  angle r         kind 3: ctor <frame> r        kind 4: ctor <frame> R r      (likewise)
kind 5 (Bezier):   nu nv  nu*nv*(x y z)  rat  [nw  nw*w]                  the poles row by row (U index major); the weights likewise (a wrong nw: the missing weights are 0)
kind 6 (B-spline): udeg vdeg uper vper  nku nku*uknot  nmu nmu*umult  nkv nkv*vknot  nmv nmv*vmult  nu nv  nu*nv*(x y z)  rat  [nw  nw*w]
kind 7 (extrusion): dx dy dz  crv            kind 8 (revolution): ox oy oz dx dy dz  crv
kind 9 (trimmed):   ctor  (ctor 0: u1 u2 v1 v2 usense vsense | ctor 1: param1 param2 utrim sense)  srf
kind 10 (offset):   offset notcheck  srf
```

### `state` (the printed state of an object, first token = type)

`0` line: location, direction (`Position()`). `1` circle: `Ax2` (location, direction, x direction, y direction), radius. `2` ellipse, `3` hyperbola: `Ax2`, major, minor. `4` parabola: `Ax2`, focal.
`5` Bezier curve: `n`, poles, rational, `n` weights **only if rational**, closed, has-representation. `6` B-spline curve: degree, periodic, rational, nb knots, nb poles, knots, multiplicities, poles, weights
**only if rational**, closed, knot distribution, continuity, has-representation. `7` trimmed curve: first, last, periodic, closed, then the state of the basis. `8` offset curve: offset, direction, continuity,
basis continuity, has-representation, then the state of the basis. `10` Cartesian point: x y z. `11` direction: x y z. `12` vector with magnitude: x y z. `13` axis 1 placement: location, direction.
`14` axis 2 placement: `Ax2`. `20` plane: `Ax3` (as `Ax2` plus `direct`). `21` cylinder: `Ax3`, radius. `22` cone: `Ax3`, semi-angle, reference radius. `23` sphere: `Ax3`, radius. `24` torus: `Ax3`, major, minor.
`25` extrusion: direction, has-representation, then the state of the basis curve. `26` revolution: location, direction, has-representation, then the basis curve. `27` Bezier surface: `nu`, `nv`, the poles
(U index major), u-rational, v-rational, `nu * nv` weights **only if rational**, u-closed, v-closed, has-representation. `28` B-spline surface: u degree, v degree, u periodic, v periodic, u rational, v rational, nb u knots, nb v knots, nb u poles,
nb v poles, u knots, v knots, u multiplicities, v multiplicities, poles, weights **only if rational**, u closed, v closed, u distribution, v distribution, continuity, has-representation. `29` rectangular trimmed surface:
`u1 u2 v1 v2` (the bounds), then the state of the basis surface. `30` offset surface: offset, basis continuity, has-representation, then the state of the basis. (Points, vectors and directions are printed with three
floats, an `Ax1` with 6, an `Ax2` with 12, an array as its length followed by its values, a 2D array as its numbers of rows and columns followed by the values row by row.) A state that cannot be
printed because the object is none of these (for example the surface returned by `OffsetSurface::surface` when it is another class) makes the oracle drop the case.

### Operations

Arguments are floats; integers are `lround`ed. P = point (3 floats), V = vector (3 floats), D = raw direction (3 floats, normalized by `Dir::new`), `ax1` = 6 floats `(x y z dx dy dz)`, `ax2` = 9 floats `(x y z nx ny nz xx xy xz)`
(`Ax2::new(P, N, Vx)`), `ax2n` = 6 floats `(x y z nx ny nz)` (`Ax2::from_direction(P, N)`), `frame` = 10 floats (see `tr`), `other` = 4 floats `(kind, x, y, z)` with kind 0 a `Direction` and 1 a
`VectorWithMagnitude`. The codes are the values of the `enum`s of `oracle/geom.cpp` (the names are the oracle's; `tests/geom.rs` has the same constants). B = bool, I = int, F = float, "state" = the state of
the object or of the result (above).

*Any geometry (`geom_common`; codes 1, 4, 21 .. 37)*

| code | operation | arguments | output |
|---|---|---|---|
| 1 | state | | state |
| 4 | `transform(T)` | `tr` | |
| 21 | `copy_geometry`, then `transform(T)` of the original | `tr` | state of the copy |
| 22 / 23 / 24 | `mirror_point` / `mirror_axis1` / `mirror_axis2` | P / ax1 / ax2n | |
| 25 / 26 | `rotate(ax1, a)` / `scale(P, s)` | ax1 a / x y z s | |
| 27 / 28 | `translate(V)` / `translate_points(P1, P2)` | V / P1 P2 | |
| 29 .. 35 | `mirrored_point`, `mirrored_axis1`, `mirrored_axis2`, `rotated`, `scaled`, `translated`, `translated_points` (same arguments as 22 .. 28) | | state of the result |
| 36 | `transformed(T)` | `tr` | state of the result |
| 37 | replace the object by its copy (`copy_geometry` / `copy_curve` / `copy_surface`) | | |

*Cartesian point (`P_*`, 40 .. 51)*: 40 `set_coord(x, y, z)`, 41 `set_pnt`, 42 `set_x`, 43 `set_y`, 44 `set_z`; 45 `coord` (3 F), 46 `pnt` (P), 47 `x`, 48 `y`, 49 `z` (F); 50 `distance`, 51 `square_distance` to a Cartesian point
`(x, y, z, ctor)` (F). Spec: `i f f f` = constructor (0 `from_pnt`, 1 `new`), x, y, z.

*Vectors (`V_*`, 50 .. 89)*: spec `i i f f f f f f` = kind (0 direction, 1 vector with magnitude), constructor (direction: 0 `new(x, y, z)`, 1 `from_dir(Dir::new(x, y, z))`; vector: 0 `from_vec`, 1 `new(x, y, z)`, 2
`from_points((x, y, z), (x2, y2, z2))`), then `x y z x2 y2 z2`. 50 `set_coord(x, y, z)`, 51 `set_x`, 52 `set_y`, 53 `set_z`, 54 `set_dir` / `set_vec(x, y, z)`, 55 `reverse`, 56 `reversed` (state), 57 `angle(other)` (F), 58
`angle_with_ref(other, ref)` (F; `other` at index 0, `ref` at index 4), 59 `coord` (3 F), 60 `magnitude`, 61 `square_magnitude`, 62 `x`, 63 `y`, 64 `z` (F), 65 `cross(other)`, 66 `crossed(other)` (state), 67
`cross_cross(v1, v2)`, 68 `cross_crossed(v1, v2)` (state), 69 `dot(other)` (F), 70 `dot_cross(v1, v2)` (F), 71 `vec` (V), 72 `dir` (direction only, D); vector with magnitude only: 80 `add(other)`, 81 `added` (state), 82
`subtract`, 83 `subtracted` (state), 84 `multiply(s)`, 85 `multiplied` (state), 86 `divide(s)`, 87 `divided` (state), 88 `normalize`, 89 `normalized` (state).

*Axis placements (`X_*`, `X2_*`)*: spec `i f f f f f f` (axis 1: constructor 0 `new(Ax1)`, 1 `from_point_direction`; location, direction) and `i f f f f f f f f f` (axis 2: constructor 0 `new(Ax2)`, 1 `from_point_directions`;
location, normal, x direction). 110 `reverse` (axis 1), 111 `reversed` (axis 1, state), 112 `set_axis(ax1)`, 113 `set_direction(D)`, 114 `set_location(P)`, 115 `angle(Axis1Placement::new(ax1))` (F), 116 `axis` (ax1: P D), 117
`direction` (D), 118 `location` (P), 119 `ax1` (axis 1, ax1); axis 2 only: 125 `set_ax2(ax2)`, 126 `set_x_direction(D)`, 127 `set_y_direction(D)`, 128 `ax2()?` (the `Ax2`: P D D D; an `Err` when the stored x direction is parallel to the direction), 129 `x_direction`, 130 `y_direction` (D).

*Transformation (own numbering `T_*`, family spec `tr ops`; `state` = form, is_negative, scale_factor, then `value(r, c)` for r = 1..3, c = 1..4)*: 1 state; 2 `set_*` with a recipe (`tr`); 3 `invert`; 4 `inverted` (state); 5
`multiply(T)` (recipe object); 6 `multiplied(T)` (state); 7 `pre_multiply(T)`; 8 `power(n)`; 9 `powered(n)` (state); 10 `transforms(x, y, z)` (3 F); 11 `value(r, c)` (F); 12 replace by `copy()`; 13 replace by
`Transformation::from_trsf(recipe)`; 14 `set_trsf(recipe)`.

*Curves (the object is built from the first `crv`; a second `crv` is the argument of codes 210, 230 and 240)*

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
| 100 .. 105 | line: `set_lin(Lin::from_axis(ax1))`, `set_direction(D)`, `set_location(P)`, `set_position(ax1)`, `lin` (ax1: the position of the `Lin`), `position` (ax1) | | |
| 110, 111, 112 | conic: `set_axis(ax1)`, `set_location(P)`, `set_position(ax2)` | | |
| 113 .. 118 | conic: `axis` (ax1), `location` (P), `position` (the `Ax2`), `x_axis` (ax1), `y_axis` (ax1), `eccentricity` (F) | | |
| 120, 121, 122 | circle: `set_radius(r)`, `set_circ(ax2 r)` (10 args), `circ` (radius, then the `Ax2`) | | |
| 123, 124 | ellipse / hyperbola: `set_major_radius(r)`, `set_minor_radius(r)` | r | |
| 125, 126 | ellipse: `set_elips(ax2 a b)` (11 args), `elips` (major, minor, `Ax2`) | | |
| 127 .. 134 | ellipse / hyperbola (129 and 134 also parabola): `directrix1` (ax1), `directrix2` (ax1), `focal` (F), `focus1` (P), `focus2` (P), `major_radius`, `minor_radius`, `parameter` (F) | | |
| 135 .. 139 | hyperbola: `asymptote1`, `asymptote2` (ax1), `conjugate_branch1`, `conjugate_branch2`, `other_branch` (major, minor, `Ax2`) | | |
| 140, 141 | hyperbola: `set_hypr(ax2 a b)` (11 args), `hypr` (major, minor, `Ax2`) | | |
| 142 .. 146 | parabola: `set_focal(f)`, `set_parab(ax2 f)` (10 args), `parab` (focal, `Ax2`), `directrix` (ax1), `focus` (P) | | |
| 150, 151 | `start_point`, `end_point` (Bezier, B-spline, trimmed) | | P |
| 160 | `increase(d)` / `increase_degree(d)` | d | |
| 161, 162 | Bezier `insert_pole_after_weight(i, P, w)` / `insert_pole_before_weight` | i x y z w | |
| 211, 212 | Bezier `insert_pole_after(i, P)` / `insert_pole_before` | i x y z | |
| 163 | Bezier `remove_pole(i)` | i | |
| 164 | `segment(u1, u2)` (Bezier) / `segment(u1, u2, tol)` (B-spline) | u1 u2 tol | |
| 165, 166, 167 | `set_pole(i, P)`, `set_pole_weight(i, P, w)`, `set_weight(i, w)` | i x y z / i x y z w / i w | |
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
| 209 | `set_origin_param(u, tol)` | u tol | |
| 193 | `move_point(u, P, i1, i2)` | u x y z i1 i2 | I I |
| 194 | `move_point_and_tangent(u, P, tangent, tol, sc, ec)` | u x y z tx ty tz tol sc ec | I |
| 195 | `is_g1(tf, tl, angular_tol)` | | B |
| 196 | `locate_u(u, tol, with_repetition)` | u tol rep | I I |
| 197 .. 201 | `local_value`, `local_d0`, `local_d1`, `local_d2`, `local_d3` | u k1 k2 | P; P; P V; P V V; P V V V |
| 202 | `local_dn(u, k1, k2, n)` | u k1 k2 n | V |
| 203, 204 | `knot(i)` (F), `multiplicity(i)` (I) | i | |
| 205, 206, 207, 208 | `knot_distribution`, `first_u_knot_index`, `last_u_knot_index`, `nb_knots` | | I |
| 210 | `is_equal(second curve, preci)` | preci | B |
| 220 | trimmed: `set_trim(u1, u2, sense, adjust_periodic)` | | |
| 221 | trimmed or offset: state of `basis_curve()` | | state |
| 230 | offset: `set_basis_curve(build(second crv), not_check_c0)` | notcheck | |
| 231, 232 | offset: `set_offset_value(d)`, `offset` (F) | | |
| 233, 234, 235 | offset: `set_direction(D)`, `get_basis_curve_continuity` (I), `direction` (D) | | |
| 240 | `set_eval_representation(desc)` over the second `crv` | kind max_deriv has_domain first last scale offset null_representation | |
| 241, 242, 243 | `clear_eval_representation`, `has_eval_representation` (B), kind of the representation (I, -1 if none) | | |

*Surfaces (`SU_*` etc.; the object is built from the first `srf`; a second `srf` is the argument of 395 and 410, a `crv` the argument of 383)*

| code | operation | arguments | output |
|---|---|---|---|
| 300, 301, 302 | `u_reverse`, `u_reversed` (state), `u_reversed_parameter(u)` (F) | u | |
| 303, 304, 305 | `v_reverse`, `v_reversed` (state), `v_reversed_parameter(v)` (F) | v | |
| 306 | `transform_parameters(u, v, T)` | u v `tr` | F F |
| 307 | `parametric_transformation(T)` | `tr` | `GTrsf2d`: form (I), is_negative (B), `value(r, c)` for r = 1..2, c = 1..3 (F) |
| 308 | `bounds` | | F F F F |
| 309 .. 312 | `is_u_closed`, `is_v_closed`, `is_u_periodic`, `is_v_periodic` | | B |
| 313, 314 | `u_period`, `v_period` | | F |
| 315, 316 | `u_iso(u)`, `v_iso(v)` | u / v | state of the curve |
| 317 | `continuity` | | I |
| 318, 319 | `is_cn_u(n)`, `is_cn_v(n)` | n | B |
| 320 .. 323 | `eval_d0` .. `eval_d3` | u v | P; P V V; P V V V V V; P and the nine derivatives `d1u d1v d2u d2v d2uv d3u d3v d3uuv d3uvv` |
| 324 | `eval_dn(u, v, nu, nv)` | u v nu nv | V |
| 325 | `value(u, v)` | u v | P |
| 330 .. 335 | elementary: `set_axis(ax1)`, `set_location(P)`, `set_position(frame)`, `axis` (ax1), `location` (P), `position` (`Ax3`: P D D D B) | | |
| 340, 341, 342 | plane: `set_pln(Pln::new(frame))`, `pln` (`Ax3`), `coefficients` (A B C D) | | |
| 345 .. 349 | cylinder: `set_cylinder(frame r)`, `set_radius(r)`, `cylinder` (radius, `Ax3`), `coefficients` (10 F), `radius` | | |
| 350 .. 357 | cone: `set_cone(frame angle r)`, `set_radius(r)`, `set_semi_angle(a)`, `cone` (semi-angle, ref radius, `Ax3`), `apex` (P), `coefficients` (10 F), `ref_radius`, `semi_angle` | | |
| 360 .. 366 | sphere: `set_sphere(frame r)`, `set_radius(r)`, `sphere` (radius, `Ax3`), `area`, `volume`, `coefficients` (10 F), `radius` | | |
| 370 .. 378 | torus: `set_torus(frame R r)`, `set_major_radius(R)`, `set_minor_radius(r)`, `torus` (major, minor, `Ax3`), `area`, `volume`, `coefficients` (35 F, with its length), `major_radius`, `minor_radius` | | |
| 380, 381 | swept: `direction` (D), `basis_curve` (state) | | |
| 382, 383 | swept: `set_direction(D)`, `set_basis_curve(build(second crv))` | | |
| 384 .. 388 | revolution: `set_axis(ax1)`, `set_location(P)`, `axis` (ax1), `location` (P), `reference_plane` (always `throws`) | | |
| 390, 391, 392 | rectangular trimmed: `set_trim(u1, u2, v1, v2, usense, vsense)`, `set_trim_param(p1, p2, utrim, sense)`, state of `basis_surface()` | | |
| 395 .. 402 | offset: `set_basis_surface(build(second srf), not_check_c0)`, `set_offset_value(d)`, `offset` (F), state of `basis_surface()`, `get_basis_surf_continuity` (I), `surface()` (B present; if true the state of the surface), `u_osculating_surface(u, v)`, `v_osculating_surface(u, v)` (B; if true: B opposite, then the state of the B-spline surface) | | |
| 410 | `set_eval_representation(desc)` over the second `srf` | kind max_deriv has_domain u1 u2 v1 v2 scale_u offset_u scale_v offset_v swap null_representation | |
| 411, 412, 413 | `clear_eval_representation`, `has_eval_representation` (B), kind (I, -1 if none) | | |
| 420 | Bezier: `exchange_uv` | | |
| 421 | Bezier: `increase(udeg, vdeg)` | udeg vdeg | |
| 422 .. 425 | Bezier: `insert_pole_col_after`, `insert_pole_col_before`, `insert_pole_row_after`, `insert_pole_row_before` (with weights when the flag is 1) | idx, n, n points, flag, [n weights] | |
| 426, 427 | Bezier: `remove_pole_col(v)`, `remove_pole_row(u)` | idx | |
| 428 | Bezier: `segment(u1, u2, v1, v2)` | u1 u2 v1 v2 | |
| 429, 430 | Bezier: `set_pole(i, j, P)`, `set_pole_weight(i, j, P, w)` | i j x y z [w] | |
| 431, 432 | Bezier: `set_pole_col(v, ..)`, `set_pole_row(u, ..)` (with weights when the flag is 1) | idx, n, n points, flag, [n weights] | |
| 433, 434, 435 | Bezier: `set_weight(i, j, w)`, `set_weight_col(v, ..)`, `set_weight_row(u, ..)` | i j w / idx, n, n weights | |
| 436 | Bezier and B-spline: `resolution(tol)` | tol | F F |
| 437 | `max_degree` (Bezier; 518 B-spline) | | I |
| 440, 441 | Bezier and B-spline: `pole(i, j)` (P), `weight(i, j)` (F) | i j | |
| 442, 443 | `poles` (rows, columns, values), `weights` (B, then rows, columns, values if some) | | |
| 444 .. 449 | `u_knots`, `v_knots`, `u_multiplicities`, `v_multiplicities`, `u_knot_sequence`, `v_knot_sequence` (length and values) | | |
| 450, 451 | `nb_u_poles` and `u_degree` (I I), `nb_v_poles` and `v_degree` (I I) | | |
| 460 .. 467 | B-spline: `exchange_uv`, `set_u_periodic`, `set_v_periodic`, `periodic_normalization(u, v)` (F F), `set_u_origin(i)`, `set_v_origin(i)`, `set_u_not_periodic`, `set_v_not_periodic` | | |
| 468 | `increase_degree(udeg, vdeg)` | | |
| 469, 470 | `insert_u_knots` / `insert_v_knots(knots, mults, tol, add)` | tol add n, n knots, n, n multiplicities | |
| 471, 472 | `remove_u_knot` / `remove_v_knot(i, m, tol)` | i m tol | B |
| 473, 474, 475 | `increase_u_multiplicity(i, m)`, `increase_u_multiplicity_range(i1, i2, m)`, `increment_u_multiplicity(i1, i2, step)` | | |
| 476, 477, 478 | the same for V | | |
| 479, 480 | `insert_u_knot` / `insert_v_knot(u, m, tol, add)` | u m tol add | |
| 481, 482 | `segment` / `check_and_segment(u1, u2, v1, v2, utol, vtol)` | | |
| 483, 485, 484 | `set_u_knot(i, k)`, `set_u_knot_mult(i, k, m)`, `set_u_knots(n, knots)` | | |
| 486, 488, 487 | the same for V | | |
| 489, 490 | `locate_u` / `locate_v(u, tol, with_repetition)` | | I I |
| 491 .. 496 | `set_pole(i, j, P)`, `set_pole_weight(i, j, P, w)`, `set_pole_col(v, ..)`, `set_pole_col_weights`, `set_pole_row`, `set_pole_row_weights` | idx, n, n points, [n weights] | |
| 497, 498, 499 | `set_weight(i, j, w)`, `set_weight_col(v, ..)`, `set_weight_row(u, ..)` | | |
| 500 | `move_point(u, v, P, ui1, ui2, vi1, vi2)` | u v x y z ui1 ui2 vi1 vi2 | I I I I |
| 501 .. 504 | `first_u_knot_index`, `first_v_knot_index`, `last_u_knot_index`, `last_v_knot_index` | | I |
| 505 .. 508 | `u_knot(i)` (F), `v_knot(i)` (F), `u_multiplicity(i)` (I), `v_multiplicity(i)` (I) | i | |
| 509 .. 514 | `local_value`, `local_d0`, `local_d1`, `local_d2`, `local_d3`, `local_dn` | u v uk1 uk2 vk1 vk2 [nu nv] | P; P; P V V; P and 5 V; P and 9 V; V |
| 515, 516 | `u_iso_check(u, check_rational)`, `v_iso_check(v, check_rational)` | u / v, flag | state of the B-spline curve |
| 518 | `max_degree` | | I |

## Families

101 families, 1616 lines in `tests/geom.txt` (16 per family), 299 `throws` lines, 64 `unchecked` lines. `EXACT` = compared bit for bit, `ulps` = within 8 ulps.

| family | schema | lines | throws | unchecked | compare | covers |
|---|---|---|---|---|---|---|
| `cpoint_ops` | `i f f f ops` | 16 | 0 | 0 | EXACT | Cartesian point: both constructors, setters, accessors, distances |
| `cpoint_xform` | `i f f f ops` | 16 | 0 | 0 | ulps | Cartesian point under Mirror (point, axis, plane) / Rotate / Scale / Translate / Transform, the `...ed` copies and Copy |
| `direction_ops` | `i i f f f f f f ops` | 16 | 0 | 2 | ulps | direction: constructors, setters (zero length unchecked), Reverse(d), Angle, Dot, accessors |
| `direction_cross` | `i i f f f f f f ops` | 16 | 0 | 5 | ulps | direction: Cross, Crossed, CrossCross, CrossCrossed, DotCross, AngleWithRef (parallel operands are unchecked) |
| `direction_xform` | `i i f f f f f f ops` | 16 | 0 | 0 | ulps | direction under the geometry operations |
| `vecmag_ops` | `i i f f f f f f ops` | 16 | 0 | 0 | ulps | vector with magnitude: three constructors, setters, accessors, Angle, Dot, Reverse(d) |
| `vecmag_arith` | `i i f f f f f f ops` | 16 | 0 | 2 | EXACT | Add / Added, Subtract / Subtracted, Multiply / Multiplied, Divide / Divided (zero unchecked), Normalize / Normalized (zero unchecked) |
| `vecmag_cross` | `i i f f f f f f ops` | 16 | 0 | 3 | ulps | vector with magnitude: Cross, Crossed, CrossCross, CrossCrossed, DotCross, AngleWithRef, zero vectors |
| `vecmag_xform` | `i i f f f f f f ops` | 16 | 0 | 0 | ulps | vector with magnitude under the geometry operations |
| `axis1_ops` | `i f f f f f f ops` | 16 | 0 | 0 | ulps | Geom_Axis1Placement: constructors, Reverse(d), SetAxis, SetDirection, SetLocation, Angle, accessors, geometry operations |
| `axis2_ops` | `i f f f f f f f f f ops` | 16 | 0 | 0 | ulps | Geom_Axis2Placement: constructors, SetAx2, SetAxis, SetDirection, SetXDirection, SetYDirection, Angle, accessors, geometry operations |
| `trsf_set` | `tr ops` | 16 | 0 | 0 | ulps | Geom_Transformation: every Set* (mirror point / axis / plane, rotation, scale, SetTransformation x2, translation x2, SetTrsf, displacement and SetValues through SetTrsf), constructors, Copy, Value, Transforms |
| `trsf_invert` | `tr ops` | 16 | 0 | 1 | ulps | Invert / Inverted, including scale 0 (unchecked) |
| `trsf_multiply` | `tr ops` | 16 | 0 | 0 | ulps | Multiply, Multiplied, PreMultiply |
| `trsf_power` | `tr ops` | 16 | 0 | 4 | ulps | Power / Powered, negative and zero exponents, scale 0 (unchecked) |
| `curve_params` | `crv ops` | 16 | 0 | 2 | ulps | FirstParameter, LastParameter, IsClosed, IsPeriodic, Period, Continuity, IsCN of every curve kind (trimmed and offset included) |
| `curve_d01` | `crv ops` | 16 | 0 | 0 | ulps | EvalD0 / EvalD1 / Value on every kind, parameters inside, on the ends, outside and across periods |
| `curve_d23` | `crv ops` | 16 | 0 | 0 | ulps | EvalD2 / EvalD3 |
| `curve_dn` | `crv ops` | 16 | 3 | 0 | ulps | EvalDN with N from -1 to 6 (N < 1 throws) |
| `curve_reverse` | `crv ops` | 16 | 4 | 0 | ulps | Reverse, Reversed, ReversedParameter, evaluation after reversing |
| `curve_xform` | `crv ops` | 16 | 2 | 0 | ulps | Transform with all recipes (negative scale, mirrors), TransformedParameter, ParametricTransformation |
| `curve_geom_ops` | `crv ops` | 16 | 4 | 0 | ulps | Mirror / Rotate / Scale / Translate / Transform / Copy and the copies on curves (deep copies of trimmed and offset curves) |
| `line_ops` | `crv ops` | 16 | 0 | 3 | ulps | Geom_Line: constructors, SetLin / SetDirection / SetLocation / SetPosition, Lin, Position, derivatives, parameters |
| `circle_ops` | `crv ops` | 16 | 1 | 0 | ulps | Geom_Circle: SetRadius (negative throws), SetCirc, Circ, SetAxis / SetPosition / SetLocation, evaluation, Reverse |
| `ellipse_ops` | `crv ops` | 16 | 3 | 1 | ulps | Geom_Ellipse: SetMajorRadius / SetMinorRadius / SetElips, Elips, directrices, foci, Focal, Parameter, conic axes |
| `hyperbola_ops` | `crv ops` | 16 | 0 | 0 | ulps | Geom_Hyperbola: setters, asymptotes, conjugate and other branches, directrices, foci, conic axes |
| `parabola_ops` | `crv ops` | 16 | 0 | 0 | ulps | Geom_Parabola: SetFocal, SetParab, Parab, Directrix, Focus, Focal, Parameter, conic axes |
| `conic_build` | `crv ops` | 16 | 2 | 0 | ulps | all four conics: every constructor, invalid radii, state and accessors |
| `bezier_build` | `crv ops` | 16 | 5 | 0 | EXACT | Bezier constructors (invalid sizes and weights), all accessors, Knots / Multiplicities / KnotSequence, MaxDegree |
| `bezier_poles` | `crv ops` | 16 | 5 | 0 | EXACT | InsertPoleAfter / Before (with and without a weight, up to the maximum degree), RemovePole |
| `bezier_increase_segment` | `crv ops` | 16 | 3 | 0 | EXACT | Increase (equal, lower and above the maximum degree), Segment (reversed, equal, outside parameters) |
| `bezier_set_pole` | `crv ops` | 16 | 5 | 0 | EXACT | SetPole x2, SetWeight, Resolution and its cache invalidation |
| `bezier_eval` | `crv ops` | 16 | 0 | 0 | EXACT | D0..D3, DN beyond the degree, StartPoint / EndPoint; rational, polynomial, cusps |
| `bspline_build` | `crv ops` | 16 | 8 | 6 | EXACT | B-spline constructors (invalid degree, sizes, knots, weights), every accessor, knot distribution |
| `bspline_eval` | `crv ops` | 16 | 0 | 0 | EXACT | D0..DN, periodic and non periodic, rational and polynomial, multiplicities up to degree + 1, parameters outside |
| `bspline_increase` | `crv ops` | 16 | 1 | 0 | EXACT | IncreaseDegree, IncreaseMultiplicity x2, IncrementMultiplicity |
| `bspline_insert_knots` | `crv ops` | 16 | 3 | 0 | EXACT | InsertKnot, InsertKnots (Add true / false, existing and new knots, tolerances) |
| `bspline_remove_knot` | `crv ops` | 16 | 11 | 0 | EXACT | RemoveKnot (after inserting, exact and tolerant) |
| `bspline_segment` | `crv ops` | 16 | 5 | 0 | EXACT | Segment (periodic, reversed, outside, tolerance) |
| `bspline_set_knot` | `crv ops` | 16 | 8 | 0 | EXACT | SetKnot, SetKnot with multiplicity, SetKnots |
| `bspline_periodic` | `crv ops` | 16 | 4 | 0 | EXACT | SetPeriodic, SetNotPeriodic, SetOrigin(index), SetOrigin(U, Tol), PeriodicNormalization |
| `bspline_set_pole` | `crv ops` | 16 | 5 | 0 | EXACT | SetPole x2, SetWeight, Resolution and its cache invalidation |
| `bspline_move_point` | `crv ops` | 16 | 1 | 0 | EXACT | MovePoint |
| `bspline_move_tangent` | `crv ops` | 16 | 0 | 0 | ulps | MovePointAndTangent |
| `bspline_locate_local` | `crv ops` | 16 | 0 | 3 | EXACT | LocateU (with and without knot repetition), LocalValue, LocalD0..D3, LocalDN |
| `bspline_is_g1` | `crv ops` | 16 | 0 | 0 | ulps | IsG1, IsCN, Continuity |
| `bspline_is_equal` | `crv crv ops` | 16 | 0 | 0 | EXACT | IsEqual against equal, slightly different and different curves |
| `trimmed_build` | `crv ops` | 16 | 0 | 0 | ulps | Geom_TrimmedCurve constructor (trimmed basis, reversed, equal and out of range parameters), accessors, evaluation |
| `trimmed_set_trim` | `crv ops` | 16 | 4 | 0 | ulps | SetTrim |
| `trimmed_periodic` | `crv ops` | 16 | 1 | 0 | ulps | trims across the period: IsClosed, IsPeriodic, Period, Reverse |
| `offset_build` | `crv ops` | 16 | 3 | 1 | ulps | Geom_OffsetCurve constructor: basis unwrapping, C0 check, accessors |
| `offset_eval` | `crv ops` | 16 | 4 | 0 | ulps | D0..DN of offsets (zero and negative distance) |
| `offset_singular` | `crv ops` | 16 | 8 | 0 | ulps | offsets of curves with cusps and zero derivatives (undefined values and derivatives) |
| `offset_set` | `crv crv ops` | 16 | 0 | 0 | ulps | SetBasisCurve, SetOffsetValue, SetDirection, Reverse, Transform |
| `evalrep_curve_basic` | `crv crv ops` | 16 | 3 | 0 | ulps | Full and DerivBounded descriptors on Bezier, B-spline and offset curves (orders above the limit fall back) |
| `evalrep_curve_mapped` | `crv crv ops` | 16 | 6 | 0 | ulps | Mapped descriptors (domain, scale, offset) |
| `evalrep_curve_invalid` | `crv crv ops` | 16 | 16 | 0 | ulps | null representation, invalid map: ProgramError |
| `evalrep_curve_copy` | `crv crv ops` | 16 | 2 | 0 | ulps | Copy / clone, and which modifiers clear the descriptor |
| `surface_params` | `srf ops` | 16 | 0 | 4 | ulps | Bounds, IsUClosed, IsVClosed, IsUPeriodic, IsVPeriodic, UPeriod, VPeriod, Continuity, IsCNu, IsCNv of every surface kind |
| `surface_d01` | `srf ops` | 16 | 0 | 0 | ulps | EvalD0 / EvalD1 / Value on every kind, parameters inside, on the ends, outside, at poles and the apex |
| `surface_d2` | `srf ops` | 16 | 0 | 0 | ulps | EvalD2 |
| `surface_d3dn` | `srf ops` | 16 | 3 | 0 | ulps | EvalD3, EvalDN (orders from -1 to 3) |
| `surface_reverse` | `srf ops` | 16 | 0 | 0 | ulps | UReverse, VReverse, UReversed, VReversed, UReversedParameter, VReversedParameter, evaluation after reversing |
| `surface_xform` | `srf ops` | 16 | 1 | 0 | ulps | Transform with all recipes, TransformParameters, ParametricTransformation (GTrsf2d) |
| `surface_geom_ops` | `srf ops` | 16 | 0 | 0 | ulps | Mirror / Rotate / Scale / Translate / Transform / Copy and the copies on surfaces |
| `surface_iso` | `srf ops` | 16 | 0 | 0 | ulps | UIso, VIso of every surface kind, UIso / VIso with CheckRational of B-spline surfaces |
| `plane_ops` | `srf ops` | 16 | 0 | 0 | ulps | Geom_Plane: SetPln, Pln, Coefficients, SetAxis / SetPosition / SetLocation, evaluation |
| `cylinder_ops` | `srf ops` | 16 | 4 | 0 | ulps | Geom_CylindricalSurface: SetCylinder, SetRadius, Cylinder, Coefficients, Radius, elementary setters |
| `cone_ops` | `srf ops` | 16 | 1 | 0 | ulps | Geom_ConicalSurface: SetCone, SetRadius, SetSemiAngle, Cone, Apex, Coefficients, RefRadius, SemiAngle |
| `sphere_ops` | `srf ops` | 16 | 0 | 0 | ulps | Geom_SphericalSurface: SetSphere, SetRadius, Sphere, Area, Volume, Coefficients |
| `torus_ops` | `srf ops` | 16 | 2 | 0 | ulps | Geom_ToroidalSurface: SetTorus, SetMajorRadius, SetMinorRadius, Torus, Area, Volume, Coefficients |
| `elementary_build` | `srf ops` | 16 | 4 | 0 | ulps | all five elementary surfaces: every constructor, invalid radii and angles, state and accessors |
| `swept_build` | `srf ops` | 16 | 7 | 1 | ulps | Geom_SurfaceOfLinearExtrusion and Geom_SurfaceOfRevolution: constructors, Direction, BasisCurve, Axis, Location, ReferencePlane (not implemented) |
| `extrusion_ops` | `srf crv ops` | 16 | 0 | 0 | ulps | SetDirection, SetBasisCurve, reversal, evaluation of extrusions |
| `revolution_ops` | `srf crv ops` | 16 | 5 | 0 | ulps | SetAxis, SetDirection, SetLocation, SetBasisCurve, evaluation of revolutions |
| `revolution_singular` | `srf ops` | 16 | 2 | 0 | ulps | revolutions of curves that touch the axis: derivatives at the singular points |
| `bezsurf_build` | `srf ops` | 16 | 4 | 7 | EXACT | Bezier surface constructors (invalid sizes and weights), all accessors, knots and multiplicities, MaxDegree |
| `bezsurf_eval` | `srf ops` | 16 | 3 | 0 | EXACT | D0..DN, UIso, VIso of rational and polynomial surfaces |
| `bezsurf_increase_segment` | `srf ops` | 16 | 4 | 0 | EXACT | Increase, Segment |
| `bezsurf_insert_remove` | `srf ops` | 16 | 13 | 0 | EXACT | InsertPoleColAfter / Before, InsertPoleRowAfter / Before (with and without weights), RemovePoleCol, RemovePoleRow |
| `bezsurf_set_pole` | `srf ops` | 16 | 6 | 0 | EXACT | SetPole x2, SetPoleCol, SetPoleRow, SetWeight, SetWeightCol, SetWeightRow, ExchangeUV, Resolution |
| `bsplsurf_build` | `srf ops` | 16 | 8 | 5 | EXACT | B-spline surface constructors (invalid degree, sizes, knots, weights), every accessor, knot distribution |
| `bsplsurf_eval` | `srf ops` | 16 | 4 | 0 | EXACT | D0..DN of u/v periodic and non periodic, rational and polynomial surfaces, multiplicities up to degree + 1, parameters outside |
| `bsplsurf_increase` | `srf ops` | 16 | 5 | 1 | EXACT | IncreaseDegree, IncreaseUMultiplicity x2, IncrementUMultiplicity (and V) |
| `bsplsurf_knots` | `srf ops` | 16 | 9 | 0 | EXACT | InsertUKnot, InsertUKnots, RemoveUKnot (and V) |
| `bsplsurf_segment` | `srf ops` | 16 | 12 | 0 | EXACT | Segment, CheckAndSegment (periodic, reversed, outside, tolerances) |
| `bsplsurf_set_knot` | `srf ops` | 16 | 8 | 0 | EXACT | SetUKnot, SetUKnot with multiplicity, SetUKnots (and V) |
| `bsplsurf_periodic` | `srf ops` | 16 | 10 | 0 | EXACT | SetUPeriodic, SetVPeriodic, SetUNotPeriodic, SetVNotPeriodic, SetUOrigin, SetVOrigin, PeriodicNormalization, ExchangeUV |
| `bsplsurf_set_pole` | `srf ops` | 16 | 4 | 0 | EXACT | SetPole x2, SetPoleCol x2, SetPoleRow x2, SetWeight, SetWeightCol, SetWeightRow |
| `bsplsurf_move_point` | `srf ops` | 16 | 5 | 0 | EXACT | MovePoint |
| `bsplsurf_iso_local` | `srf ops` | 16 | 0 | 4 | EXACT | LocateU, LocateV, UIso / VIso with CheckRational, Resolution, LocalValue, LocalD0..D3, LocalDN |
| `rts_build` | `srf ops` | 16 | 3 | 4 | ulps | Geom_RectangularTrimmedSurface constructors (trimmed basis, reversed, equal and out of range parameters), accessors |
| `rts_set_trim` | `srf ops` | 16 | 7 | 0 | ulps | SetTrim (both forms), evaluation |
| `offsurf_build` | `srf ops` | 16 | 4 | 5 | ulps | Geom_OffsetSurface constructor: basis unwrapping, C1 check, accessors, evaluation |
| `offsurf_singular` | `srf ops` | 16 | 4 | 0 | ulps | offsets of spheres at the poles, cones at the apex, revolutions on the axis, collapsed Bezier rows (osculating-surface fallback, undefined derivatives) |
| `offsurf_osculating` | `srf ops` | 16 | 5 | 0 | ulps | UOsculatingSurface, VOsculatingSurface |
| `offsurf_set` | `srf srf ops` | 16 | 1 | 0 | ulps | SetBasisSurface, SetOffsetValue, reversal, Transform |
| `evalrep_surf_basic` | `srf srf ops` | 16 | 2 | 0 | ulps | Full and DerivBounded descriptors on Bezier, B-spline, extrusion, revolution and offset surfaces |
| `evalrep_surf_mapped` | `srf srf ops` | 16 | 1 | 0 | ulps | Mapped descriptors (domain, scales, offsets, swapped parameters) |
| `evalrep_surf_invalid` | `srf srf ops` | 16 | 16 | 0 | ulps | null representation, invalid map: ProgramError |
| `evalrep_surf_copy` | `srf srf ops` | 16 | 2 | 0 | ulps | Copy / clone, and which modifiers clear the descriptor |


## Coverage and what is left out

Every public method of the classes in scope is called by at least one family (see the table above and the operation codes). The following are deliberately not judged:

* `DumpJson` (all classes): no Rust API.
* The `Geom_Curve::D0` .. `D3` / `DN` and `Geom_Surface::D0` .. `D3` / `DN` inline wrappers and the deprecated array-filling accessors (`Poles(NCollection_Array1&)`, `Weights(array&)`, `Knots(array&)`,
  `KnotSequence(array&)`, `Multiplicities(array&)`, `UKnots(array&)` ..., `WeightsArray()`): the same data is judged through `eval_*`, `poles`, `weights`, `knots`, ...;
* `Geom_UndefinedDerivative`, `Geom_UndefinedValue` as classes (they are the exceptions behind `GeomError::UndefinedDerivative` / `UndefinedValue`);
* the aliasing of a handle (`set_basis_curve` / `set_basis_surface` keep the caller's handle; two objects sharing one basis and one of them being modified): an `Arc` cannot be modified through the
  shared handle; likewise the "descriptor chain" check of `ValidateCurveDesc` / `ValidateSurfaceDesc`;
* the other classes of `GeomEval` (helix, sine wave, T-Bezier, AHT-Bezier, ellipsoid, hyperboloid, paraboloid, hyperbolic paraboloid, helicoid): not in scope; a representation curve in the test is
  a line, circle, Bezier or B-spline curve, a representation surface a plane, cylinder, sphere, Bezier or B-spline surface. `OffsetSurface::surface` may return one of those classes in C++
  (e.g. a `GeomEval` offset of an analytic surface); the oracle drops such a case instead of printing a state, so only the classes of this module occur;
* `Geom_OffsetSurface::UIso` / `VIso` (an approximation through `AdvApprox`, which is not among the provided sources): the oracle never calls them on offset surfaces; `Geom_OffsetSurface` is judged through its evaluation, `surface()`, the osculating surfaces and the editing methods;
* argument shapes that OCCT cannot receive in Rust (null handles, arrays with a lower bound other than 1): not representable;
* inputs for which OCCT 8.0.1 as built reads or writes outside an array and the result is not reproducible (the oracle drops the case, see the next section) other than the ones listed as `unchecked`;
  a periodic B-spline with fewer poles than its degree plus one is never generated (it makes several OCCT operations read outside their arrays), nor an inner multiplicity above the degree;
* a Bezier or B-spline surface of more than 5 x 5 poles (the golden file is limited to 1.5 MB); the algorithms are the same for any size.

## How the file is made (the uninitialised-read check, the C library, determinism)

`oracle/geom.cpp` is a single C++17 program (build: `mkdir -p oracle-bin && c++ -std=c++17 -O2 oracle/geom.cpp -I$OCCT/include/opencascade -L$OCCT/lib -lTKG3d -lTKG2d -lTKMath -lTKernel -ldl
-o oracle-bin/geom && LD_LIBRARY_PATH=$OCCT/lib oracle-bin/geom > tests/geom.txt`, with `OCCT=.../deps/occt-install`; `TKG2d` is needed because `libTKG3d` uses `Geom2d` classes; two runs give the same bytes; with a first
argument `oracle-bin/geom point,vec,axis,trsf,curve,conic,bezier,bspline,trim,offset,erep,surface,elementary,swept,bezsurf,bsplsurf,rts,offsurf,serep` runs a group, with a second one a family; the
environment variables `GEOM_CAP` (lines per family, 16), `GEOM_MAXLEN` (longest line, 1500 bytes), `GEOM_DROPLOG`, `GEOM_WHY`, `GEOM_SCHEMAS`, `GEOM_CHECK_EXACT` are for development). Each line is a pseudo-random
case from a fixed xorshift generator plus the edge cases listed in the families: all four conics with their constructors and invalid arguments (parabolas from a directrix and a focus); Bezier curves of 2 .. 26
poles and surfaces up to 5 x 5, rational and polynomial; B-spline curves and surfaces of degree 1 .. 6, periodic (in U, in V, in both) and not, rational and polynomial, inner multiplicities 1 .. degree, end
multiplicities up to degree + 1 (and below it: a non periodic object that is not clamped), uniform, quasi-uniform and piecewise-Bezier knots, cusps made of repeated poles and collapsed pole rows; trimmed
curves and surfaces across the period, reversed, equal and out of range; offsets of zero, negative and huge distance, over lines, conics, trimmed and offset curves, C0 B-splines, and over planes, spheres (the
poles), cones (the apex), tori, revolutions that touch their axis, Bezier and B-spline surfaces; transformations with negative scale, mirrors (point, axis, plane), left-handed frames and exact Pythagorean
rotations; descriptors of all three kinds with domains, scales, swapped parameters and negative scale. The sequence of operations of a line is planned on a scratch object in a child process (the operations
may corrupt the heap when OCCT's missing checks are hit, and building an offset surface over some B-spline surfaces may take very long: such a case is dropped after 3 seconds) and then replayed. To keep the golden file
under 1.5 MB the planner prints the state of the object once per line (the last state operation), no line is longer than 1500 bytes (`GEOM_MAXLEN`; the lines of the transformation families may be longer), and
the surfaces are small.

Every call is executed six times with the stack and the heap filled differently (pseudo-random words, all 0, all 1; `mallopt(M_PERTURB)`): a case whose printed results differ between the runs (an uninitialised
read in OCCT), that is not finite (a NaN or an infinity would be printed), or whose child process crashes, is dropped. The families that call the C library are in addition re-run 24 times with every
`sin cos tan sinh cosh asin acos atan atan2 exp log pow` result moved by 0 or 1 ulp (an interposed `libm`), and a case whose outputs move by more than 2 ulps is dropped: the golden values are therefore
stable under a different C library, which is what the 8 ulp tolerance relies on. The `EXACT` families were checked to have no case that changes under that perturbation (`GEOM_CHECK_EXACT=1` runs the whole
oracle with the perturbation and lists the families that change; none of the `EXACT` families does) and call none of those functions.

## Using the judge

`cargo test --release --test geom` once `pub mod geom;` exists (the test does not compile before: its only error is the unresolved import of `occt_port::geom`; against the signature listing above it compiles and every call
panics with `not yet implemented`). `tests/geom.rs` must not be edited: it asserts the family list, the golden line count, no duplicate line and at least 16 distinct lines per family; the `BEGIN GENERATED` block holds
the lists that are regenerated from `tests/geom.txt`. The other judges (`gp3d`, `geom2d`, `bsplslib`, ...) are untouched and keep passing.

## Removed judge line (review, 2026-10-26)
`curve_reverse` on a degree-2 unclamped B-spline (mults [2,2,1], 2 poles) was removed: after
`Reverse` its domain is one point and the line evaluates outside it, so OCCT reads an
unwritten knot slot and `Poles(-1)`; the printed values are uninitialised memory.
