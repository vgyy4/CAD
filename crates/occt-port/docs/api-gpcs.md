# `gp` 3D curves and elementary surfaces: Rust API contract

This contract covers OCCT 8.0.1's `gp_Lin`, `gp_Circ`, `gp_Elips`, `gp_Hypr`, `gp_Parab` (3D curves)
and `gp_Pln`, `gp_Cylinder`, `gp_Cone`, `gp_Sphere`, `gp_Torus` (elementary surfaces). Everything is in
`occt_port::gp` and sits beside the 3D and 2D translations (`docs/api-gp3d.md`, `docs/api-gp2d.md`),
whose conventions it follows; nothing in them changes. The tests are `tests/gpcs.rs` with the golden
file `tests/gpcs.txt` printed by `oracle/gpcs.cpp` (real OCCT). The first part of
`src/gp/curves.rs` (`Lin::from_axis`, `Circ::new`, `Elips::new`, `Hypr::new`, `Parab::new` and the
`pub(crate)` fields used by `elclib.rs`) exists already and must stay source compatible; the rest of this
document extends those types.

## Conventions

* Rust types: `gp::Lin`, `Circ`, `Elips`, `Hypr`, `Parab`, `Pln`, `Cylinder`, `Cone`, `Sphere`,
  `Torus`. All derive `Debug, Clone, Copy, PartialEq` (the tests copy values freely). C++ names become
  `snake_case` methods; the C++ `...ed` methods return a copy, the others mutate `&mut self`. Scalars are
  `f64`, flags `bool`. Out-parameters become returned tuples in argument order (`coefficients()`).
* `Foo::new(...)` is the main constructor (the first one of the C++ header that takes the full
  state). `Default` is the C++ default constructor (values listed per type). Every other C++ constructor
  overload has its own name (listed per type and in the summary at the end).
* **Argument passing.** An object that is stored (constructor arguments, setters) is passed by value (all
  types are `Copy`): `Lin::new(Pnt, Dir)`, `Circ::new(Ax2, f64)`, `set_location(Pnt)`, `set_position(Ax1 | Ax2 |
  Ax3)`. Every other object argument is `&T`.
* **Frames.** The curves carry a `gp::Ax2` (right handed) and the surfaces a `gp::Ax3` (direct or
  indirect). `position()` returns `&Ax2` / `&Ax3` / (for `Lin`) `&Ax1`; `location() -> &Pnt`;
  `axis() -> Ax1` (by value, as `Ax2::axis`); `x_axis()`, `y_axis() -> Ax1` (location and the X or Y
  direction of the frame). `Lin` additionally has `direction() -> &Dir`.
* **Errors.** The public zero-sized `gp::ConstructionError` stands for every OCCT failure. Fallible
  operations return `Result<_, ConstructionError>`; no panic is part of the contract. This covers
  * exceptions OCCT really raises (the header-inline checks of this build): negative radii and focal
    lengths in constructors and setters; the ellipse / hyperbola / torus / cone parameter checks listed per type;
    `Hypr::eccentricity/parameter/asymptote*/directrix*` with a zero major radius; `Elips::directrix*` with
    `eccentricity <= gp::resolution()`; the derived hyperbolas of a negative-minor-radius hyperbola; `Lin::normal`
    when the point is on the line; `Parab::from_directrix_focus` when the focus is on the directrix;
  * one check that OCCT 8.0.1 *as built* does not perform (the zero-norm test of the `gp_Dir` built inside the library's
    `gp_Pln(A, B, C, D)`: the reference binary returns a frame full of NaN/inf instead of raising, so the numbers are
    unspecified; the oracle prints `unchecked`, and the Rust API must return `Err` for the same condition): `Pln::from_coefficients(a, b, c, d)` when `sqrt(a*a + b*b + c*c) <= gp::resolution()`
    (the squares underflow for components around 1e-200, so the test includes such an input).
  The oracle prints `throws` where OCCT caught an exception and `unchecked` otherwise; both mean `Err`.
* Setters that call `Ax2::set_axis` / `Ax3::set_axis` keep their `Result` (those functions return one) and so
  does every `set_axis` below, though the test never feeds an input that fails.
* Exact comparisons (`==`, `<`, `<=`) are kept as written in OCCT.
* Mirroring is generic over a public trait per type (the `PntMirror` pattern): `mirror(&mut self, &M)` /
  `mirrored(&self, &M)` where `M` is `Pnt` (point symmetry), `Ax1` (axial symmetry) or `Ax2` (plane symmetry,
  the plane is normal to the direction of the `Ax2`). The traits are `LinMirror`, `CircMirror`, `ElipsMirror`,
  `HyprMirror`, `ParabMirror`, `PlnMirror`, `CylinderMirror`, `ConeMirror`, `SphereMirror`, `TorusMirror`; each
  is implemented for `Pnt`, `Ax1` and `Ax2` and exported from `gp` with its type.
* The geometric operations of all ten types have the same signatures:
  `rotate/rotated(&Ax1, angle)`, `scale/scaled(&Pnt, factor)`, `transform/transformed(&Trsf)`,
  `translate/translated(&Vec)`, `translate_points/translated_points(&Pnt, &Pnt)`.
  On the curves and surfaces with radii, `scale` and `transform` multiply the radii by the factor
  (resp. `Trsf::scale_factor`) and keep the absolute value (the in-place `Torus` versions differ in the sign of zero,
  see the pinned behaviors below). A scale factor of `0` is legal (the radii become zero, nothing raises).

## Distances through traits

`Lin::distance` / `square_distance` and `Pln::distance` / `signed_distance` / `square_distance` are
overloaded in C++ over the argument type (same arity, different argument types, different results), so each is
one generic method over a small public trait: `LinDistance` (implemented for `Pnt` and `Lin`) and `PlnDistance`
(implemented for `Pnt`, `Lin` and `Pln`). Semantics are those of the C++ overloads, see below.

## `Lin`

`Lin::from_axis(Ax1)` (the C++ `gp_Lin(const gp_Ax1&)`, existing), `Lin::new(Pnt, Dir)` (the C++ `gp_Lin(P, V)`),
`Default` (the default `Ax1`: origin, direction +Z). `location() -> &Pnt`, `direction() -> &Dir`,
`position() -> &Ax1`, `set_location(Pnt)`, `set_direction(Dir)`, `set_position(Ax1)`, `reverse/reversed`,
`angle(&Lin) -> f64` (the angle between the directions, in `[0, pi]`), `contains(&Pnt, linear_tol) -> bool`
(`distance(P) <= tol`), `distance(&Pnt | &Lin)`, `square_distance(&Pnt | &Lin)`, `normal(&Pnt) -> Result<Lin>`
(the line through the point that is normal to this line and meets it; `Err` when the point is on the line or at
its location). Geometry as listed in *Conventions*.

* `distance(&Pnt)` is the modulus of `(P - location) x direction`; `square_distance(&Pnt)` is **not** its square
  but the square magnitude of `gp_Vec(P - location).Cross(direction)` computed through the vector class (the two
  differ in the last bits, the test pins both).
* `distance(&Lin)`: when the two directions are parallel within `gp::resolution()` (`Ax1::is_parallel`) it is
  `other.distance(&self.location())`, otherwise the absolute scalar product of the vector between the locations
  with the normalized cross product of the directions. `square_distance(&Lin)` is the square of that distance.
* `translated_points(P1, P2)` goes through `Vec::between(P1, P2)` (`P2 - P1` first) whereas the in-place
  `translate_points` adds `P2` to the location and subtracts `P1`; the two differ in the last bit.

## `Circ`

`Circ::new(Ax2, radius) -> Result` (existing; error when `radius < 0`), `Default` (radius `f64::MAX`, i.e. OCCT
`RealLast()`, standard frame). `radius()`, `area()` (`pi r r`), `length()` (`2 pi r`), `location`, `position`,
`axis`, `x_axis`, `y_axis`, `set_location(Pnt)`, `set_position(Ax2)`, `set_axis(Ax1) -> Result`,
`set_radius(f64) -> Result` (error when `< 0`; `-0.0` is accepted), `distance(&Pnt)`, `square_distance(&Pnt)`
(`(sqrt(x*x + y*y) - r)^2 + z*z` with `x, y, z` the coordinates of `gp_Vec(location, P)` on the frame),
`contains(&Pnt, linear_tol)` (`distance <= tol`).

## `Elips`

`Elips::new(Ax2, major, minor) -> Result` (existing; error when `minor < 0` or `major < minor`), `Default`
(major `f64::MAX`, minor `gp::resolution()`). `major_radius`, `minor_radius`, `area` (`pi major minor`), `focal`
(`2 sqrt(major^2 - minor^2)`), `eccentricity` (`0` when `major == 0`), `parameter` (`minor^2 / major`, `0`
when `major == 0`), `focus1/focus2() -> Pnt` (`location +- sqrt(major^2 - minor^2) * XDirection`),
`directrix1/directrix2() -> Result<Ax1>` (error when `eccentricity <= gp::resolution()`; the location is
`location +- XDirection * major / e`, the direction `YDirection`), `location`, `position`, `axis`, `x_axis`, `y_axis`,
`set_location(Pnt)`, `set_position(Ax2)`, `set_axis(Ax1) -> Result`, `set_major_radius(f64) -> Result` (error when
`major < current minor`), `set_minor_radius(f64) -> Result` (error when `minor < 0` or `current major < minor`).

## `Hypr`

`Hypr::new(Ax2, major, minor) -> Result` (existing; error when either radius is negative), `Default`
(major `f64::MAX`, minor `-f64::MAX`, i.e. OCCT `RealFirst()`, standard frame). `major_radius`, `minor_radius`,
`focal` (`2 sqrt(major^2 + minor^2)`), `focus1/focus2() -> Pnt`, `location`, `position`, `axis`, `x_axis`, `y_axis`,
`set_position(Ax2)`, `set_axis(Ax1) -> Result`, `set_major_radius(f64) -> Result` and `set_minor_radius(f64) -> Result`
(error when negative; `-0.0` is accepted), `set_location(Pnt)` (**not** a plain location change: OCCT rebuilds the
frame with `gp_Ax2(P, Direction, XDirection)`, which renormalizes the directions; the test pins the resulting bits),
`eccentricity() -> Result<f64>` and `parameter() -> Result<f64>` (`sqrt(major^2 + minor^2) / major`, resp. `minor^2 / major`;
error when `major <= gp::resolution()`), `asymptote1/asymptote2() -> Result<Ax1>` (through the location, direction
`XDirection +- (minor / major) * YDirection` normalized; error when `major <= gp::resolution()`),
`directrix1/directrix2() -> Result<Ax1>` (location `location +- XDirection * major / e`, direction `YDirection`; error when
`major <= gp::resolution()`), `conjugate_branch1/conjugate_branch2/other_branch() -> Result<Hypr>` (the conjugate branches
swap the radii and use frame `(location, Direction, +-YDirection)`; `other_branch` negates the X direction;
the result goes through `Hypr::new`, so these fail for the negative minor radius of `Hypr::default()`).

## `Parab`

`Parab::new(Ax2, focal) -> Result` (existing; error when `focal < 0`), `Parab::from_directrix_focus(&Ax1, &Pnt) ->
Result<Parab>` (the C++ `gp_Parab(D, F)`: focal = half the distance of the focus to the directrix; the frame location
is the focus minus `focal` along the normal; errors, because the underlying `Lin::normal` or a zero cross product
raises, when the focus is on the directrix line), `Default` (focal `f64::MAX`). `focal`, `parameter` (`2 focal`),
`focus() -> Pnt` (`location + focal * XDirection`), `directrix() -> Ax1` (location `location - focal * XDirection`,
direction `YDirection`), `location`, `position`, `axis`, `x_axis`, `y_axis`, `set_location(Pnt)`, `set_position(Ax2)`,
`set_axis(Ax1) -> Result`, `set_focal(f64) -> Result` (error when `< 0`; `-0.0` accepted).

## `Pln`

`Pln::new(Ax3)` (the C++ `gp_Pln(const gp_Ax3&)`), `Pln::from_point_normal(Pnt, Dir)` (the C++ `gp_Pln(P, V)`: the frame
gets an X direction chosen by comparing the absolute values of the components of `V`, exactly as OCCT does),
`Pln::from_coefficients(a, b, c, d) -> Result` (the C++ `gp_Pln(A, B, C, D)`, plane `a x + b y + c z + d = 0`; `Err` in the
`unchecked` case of *Conventions*), `Default` (the default `Ax3`). `coefficients() -> (a, b, c, d)` (the normal, negated when the
frame is indirect, and `-(a lx + b ly + c lz)`), `location`, `position`, `axis`, `x_axis`, `y_axis`, `direct() -> bool`,
`set_location(Pnt)`, `set_position(Ax3)`, `set_axis(Ax1) -> Result`, `u_reverse()` (reverses the X direction),
`v_reverse()` (reverses the Y direction).
`signed_distance`, `distance`, `square_distance` over `&Pnt | &Lin | &Pln`:
* point: `signed_distance` is the scalar product of `Direction` with `P - location` (written out with the three products),
  `distance` its absolute value, `square_distance` the square of `distance`;
* line: `0` unless the line direction is normal to the plane normal within `gp::resolution()` (`Dir::is_normal`), then the
  signed distance of the line location;
* plane: `0` unless the normals are parallel within `gp::resolution()` (`Dir::is_parallel`), then the signed distance of
  the other plane's location.
`contains(&Pnt, linear_tol) -> bool` (`distance <= tol`), `contains_line(&Lin, linear_tol, angular_tol) -> bool` (the line location
is on the plane within `linear_tol` and the line direction is normal to the plane normal within `angular_tol`).

## `Cylinder`

`Cylinder::new(Ax3, radius) -> Result` (error when `radius < 0`), `Default` (radius `f64::MAX`). `radius`, `location`, `position`,
`axis`, `x_axis`, `y_axis`, `direct`, `set_location(Pnt)`, `set_position(Ax3)`, `set_axis(Ax1) -> Result`,
`set_radius(f64) -> Result`, `u_reverse()` (reverses the **Y** direction), `v_reverse()` (reverses the **Z** direction),
`coefficients() -> (a1, a2, a3, b1, b2, b3, c1, c2, c3, d)` (the implicit equation, computed through the `Trsf::set_transformation(&Ax3)`
of the frame: `a1 = t11^2 + t21^2`, ..., `d = t14^2 + t24^2 - r^2`).

## `Cone`

`Cone::new(Ax3, semi_angle, radius) -> Result` (C++ argument order: frame, angle, radius; error when `radius < 0`, or `|semi_angle| <=
gp::resolution()`, or `pi/2 - |semi_angle| <= gp::resolution()`), `Default` (radius `f64::MAX`, semi-angle `pi/4`).
`ref_radius() -> f64`, `semi_angle() -> f64`, `apex() -> Pnt` (`location - direction * ref_radius / tan(semi_angle)`), `location`,
`position`, `axis`, `x_axis`, `y_axis`, `direct`, `set_location(Pnt)`, `set_position(Ax3)`, `set_axis(Ax1) -> Result`,
`set_radius(f64) -> Result` (error when `< 0`), `set_semi_angle(f64) -> Result` (error when `|angle| <= resolution` or
`pi/2 - |angle| <= resolution`), `u_reverse()` (Y direction), `v_reverse()` (Z direction **and** the semi-angle changes sign),
`coefficients() -> (a1, a2, a3, b1, b2, b3, c1, c2, c3, d)` (as the cylinder with the third row scaled by `tan(semi_angle)`).

## `Sphere`

`Sphere::new(Ax3, radius) -> Result` (error when `radius < 0`), `Default` (radius `f64::MAX`). `radius`, `area` (`4 pi r r`), `volume`
(`4 pi r r r / 3`), `location`, `position`, `x_axis`, `y_axis`, `direct`, `set_location(Pnt)`, `set_position(Ax3)`,
`set_radius(f64) -> Result`, `u_reverse()` (Y direction), `v_reverse()` (Z direction), `coefficients() -> (a1, ..., d)` (10 values, as the
cylinder with the third row included). `gp_Sphere` has no `Axis`/`SetAxis`.

## `Torus`

`Torus::new(Ax3, major, minor) -> Result` (error when either radius is negative; **`minor >= major` is accepted**, zeros are accepted),
`Default` (major `f64::MAX`, minor `gp::resolution()`). `major_radius`, `minor_radius`, `area` (`4 pi pi minor major`), `volume`
(`(pi minor minor) * (2 pi major)`), `location`, `position`, `axis`, `x_axis`, `y_axis`, `direct`, `set_location(Pnt)`,
`set_position(Ax3)`, `set_axis(Ax1) -> Result`, `set_major_radius(f64) -> Result` (error when `major - current minor <= gp::resolution()`),
`set_minor_radius(f64) -> Result` (error when `minor < 0` or `current major - minor <= gp::resolution()`), `u_reverse()` (Y), `v_reverse()` (Z),
`coefficients() -> [f64; 35]` (the C++ array overload; the dimension check of the array version cannot occur).

## OCCT behaviors the golden file pins down (reproduce them, do not "fix" them)

* `Lin::square_distance(&Pnt)` is not `distance^2` (see `Lin`); `Lin::translated_points` differs from `translate_points`
  in the last bit (all other types use `Ax2`/`Ax3::translate_points` in both forms).
* `Hypr::set_location` rebuilds the frame through `Ax2::new` (see `Hypr`); the other types only move the location.
* `Torus`: the copy forms `scaled` / `transformed` multiply each radius by the factor and then take the absolute value
  only `if radius < 0`, whereas the in-place `scale` / `transform` multiply by the absolute value of the factor. They agree
  except in the sign of a zero: a zero radius times a negative factor stays `-0.0` in the copy forms and is `+0.0` in the
  in-place forms (the test compares bits, and its torus inputs include a zero minor radius with negative factors). The other
  types use the copy-form rule in both forms.
* `Cone::v_reverse` negates the semi-angle; `Cylinder`, `Cone`, `Sphere`, `Torus` reverse U with `Ax3::y_reverse` and V with
  `Ax3::z_reverse`, while `Pln::u_reverse` / `v_reverse` use `x_reverse` / `y_reverse`.
* The negative-minor-radius default `Hypr` makes its derived branches fail (family `hypr_default_branches`).
* `Torus::new` accepts `minor >= major` and zeros; `set_major_radius(m)` requires `m - minor > resolution` and `set_minor_radius(m)`
  requires `m >= 0` and `major - m > resolution`, so setting a radius equal to the other one fails.
* `Pln::from_point_normal` picks the X direction from the absolute values of the components of the normal, with the tie
  rules of the C++ branches (`|B| <= |A| && |B| <= |C|` first, then `|A| <= |B| && |A| <= |C|`, else the third branch; inside a
  branch `>` decides); tie inputs are in the test.
* `Pln` distances to lines and planes use the trigonometric relation tests with tolerance `gp::resolution()`
  (`Dir::is_normal` / `Dir::is_parallel`); a line that is not exactly normal to the plane normal has distance `0`.
* In `Cone`, a semi-angle one ulp below `pi/2` is legal and so is a tiny one (`1e-300`); `0`, `-0.0`, `pi/2`, `-pi/2`, values
  beyond and `1e-310` (below the resolution) are not.
* `Parab::from_directrix_focus` goes through `Lin::normal` (which raises when the focus is on the directrix, through
  `Dir::cross_crossed`, or at its location, through `Dir::new`).
* `Pln::from_coefficients` with a (numerically) zero normal returns unspecified numbers in OCCT 8.0.1 as built; the contract is `Err`.

## Header-by-header coverage

* `gp_Lin.hxx`, `gp_Circ.hxx`, `gp_Elips.hxx`, `gp_Hypr.hxx`, `gp_Parab.hxx`, `gp_Pln.hxx`, `gp_Cylinder.hxx`, `gp_Cone.hxx`,
  `gp_Sphere.hxx`, `gp_Torus.hxx`: every public member is exercised by at least one family of the oracle (see the `FAMILIES`
  list in `tests/gpcs.rs`). Excluded: `DumpJson`, assignment and the array-dimension check of `Torus::Coefficients`.
* Overload naming summary (each name once): `Lin::{from_axis, new}`, `Pln::{new, from_point_normal, from_coefficients}`,
  `Parab::{new, from_directrix_focus}`, `Lin::{distance, square_distance}` over `LinDistance` (`Pnt`, `Lin`),
  `Pln::{distance, signed_distance, square_distance}` over `PlnDistance` (`Pnt`, `Lin`, `Pln`), `Pln::{contains, contains_line}`
  (`contains` takes a point and one tolerance, `contains_line` a line and two tolerances), `mirror/mirrored` over the per-type
  `...Mirror` traits (`Pnt`, `Ax1`, `Ax2`), `translate/translated` (vector) and `translate_points/translated_points`
  (two points), `Torus::coefficients` (array overload only).
