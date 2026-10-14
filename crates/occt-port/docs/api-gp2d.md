# `gp` 2D Rust API contract

This contract covers OCCT 8.0.1's 2D `gp` value algebra: `gp_XY`, `gp_Pnt2d`, `gp_Vec2d`,
`gp_Dir2d`, `gp_Ax2d`, `gp_Ax22d`, `gp_Mat2d`, `gp_Trsf2d`, `gp_GTrsf2d`, `gp_Lin2d`,
`gp_Circ2d`, `gp_Elips2d`, `gp_Hypr2d`, `gp_Parab2d` and the 2D `gp::` constants. Everything is in
`occt_port::gp` and sits beside the 3D translation (`docs/api-gp3d.md`), whose conventions it
follows; nothing in the 3D API changes. The tests are `tests/gp2d.rs` with the golden file
`tests/gp2d.txt` printed by `oracle/gp2d.cpp` (real OCCT).

## Conventions

* Rust types: `gp::XY`, `Pnt2d`, `Vec2d`, `Dir2d`, `Ax2d`, `Ax22d`, `Mat2d`, `Trsf2d`, `GTrsf2d`,
  `Lin2d`, `Circ2d`, `Elips2d`, `Hypr2d`, `Parab2d`. All derive `Debug, Clone, Copy, PartialEq`
  (the tests copy values freely). C++ names become `snake_case` methods. Scalars are `f64`, indices
  and exponents `i32`, flags `bool`. Out-parameters become returned tuples in argument order.
* `Foo::new(...)` is the main constructor. `Default` is the C++ default constructor (listed per
  type). The `...ed` C++ methods return a copy; the others mutate `&mut self`.
* **Argument passing.** An object that is stored (constructor arguments, setters) is passed by value
  (all types are `Copy`): `Ax2d::new(Pnt2d, Dir2d)`, `set_location(Pnt2d)`, `set_xy(XY)`,
  `set_x_axis(Ax2d)`, `set_axis(Ax22d)`, `from_xy(XY)`, ... Every other object argument is `&T`.
* **Errors.** The public zero-sized `gp::ConstructionError` (existing, see the 3D contract) stands
  for every OCCT failure. Fallible operations return `Result<_, ConstructionError>`; no panic is part
  of the contract (indexing a matrix with a bad index returns `Err`). This covers
  * exceptions OCCT really raises: out-of-range indices of `XY`, `Pnt2d`, `Vec2d`, `Dir2d`, the
    value/set-value/change-value accessors of `Mat2d`, `Trsf2d`, `GTrsf2d`; zero-norm normalization
    (`XY`, `Vec2d`, `Dir2d`); `Vec2d` angles/relations with a zero vector (`gp_VectorWithNullMagnitude`);
    negative radii / focal lengths (`Circ2d`, `Elips2d`, `Hypr2d`, `Parab2d` constructors and setters);
    the hyperbola / ellipse domain checks (`Hypr2d::eccentricity/parameter/asymptote*/directrix*`,
    `Elips2d::directrix*`); `GTrsf2d::trsf2d()` (and everything calling it) when the transformation
    is `Other` or not orthogonal;
  * checks that OCCT 8.0.1 *as built* has compiled out (the numbers it returns there are
    unspecified; the oracle prints `unchecked` and the Rust API must return `Err` for the same
    condition): `Mat2d::column/row/set_col/set_row` with an index outside 1..=2;
    `Mat2d::invert` when `|det| <= gp::resolution()` and `Mat2d::power(n)` with `n < 0` for such a
    matrix; `Trsf2d::invert` (and `power(n)` with `n < 0`) when the form is not `Identity`,
    `Translation` or `PntMirror` and `|scale_factor| <= gp::resolution()`;
    `Trsf2d::set_values` when `|a11*a22 - a21*a12| < gp::resolution()` (strict);
    `Lin2d::from_coefficients(a, b, c)` when `a*a + b*b <= gp::resolution()`;
    `GTrsf2d::invert/power(n<0)` for the same conditions of the underlying matrix (form `Other`:
    the `Mat2d` condition; otherwise `trsf2d()` first, then the `Trsf2d` condition).
  * Division by a (nearly) zero scalar is **not** checked by OCCT and the Rust `divide/divided` of
    `XY`, `Vec2d`, `Mat2d` do not return `Result`; the test never calls them with
    `|s| <= gp::resolution()` (the oracle prints `unchecked` and the test returns early, exactly as the
    3D `xyz_divide`).
* Exact comparisons (`==`, `!=`, `<`) are kept as written in OCCT.
* `gp::resolution()` and `TrsfForm` (with its `#[repr(i32)]` discriminants) already exist.

## Package constants and the direction enumeration

`gp::origin2d() -> Pnt2d`; `gp::dx2d()`, `gp::dy2d() -> Dir2d`; `gp::ox2d()`, `gp::oy2d() -> Ax2d`
(axes through the origin along X and Y).

`gp::Direction2d` is the four-value enum `X`, `Y`, `NegativeX`, `NegativeY` (`gp_Dir2d::D`).

## `XY`

`XY::new(x,y)`, `XY::zero()`, `Default` (zero). Accessors `x()`, `y()`, `coords() -> (f64,f64)`,
`coord(index) -> Result<f64>` (1-based). Setters `set_coord(x,y)`, `set_coord_at(index,v) -> Result`,
`set_x`, `set_y`, `change_coord(index) -> Result<&mut f64>`. `modulus`, `square_modulus`,
`is_equal(&XY,tol)` (strict `<` per coordinate, as OCCT), `dot(&XY)`, `crossed(&XY) -> f64` (the
scalar `x1*y2 - y1*x2`), `cross_magnitude`, `cross_square_magnitude`.
Mutators and copies: `add/added(&XY)`, `subtract/subtracted(&XY)`, `divide/divided(f64)`,
`multiply(f64)/multiplied(f64)`, `multiply_xy(&XY)/multiplied_xy(&XY)` (componentwise),
`multiply_mat(&Mat2d)/multiplied_mat(&Mat2d)`, `reverse/reversed`,
`normalize() -> Result` / `normalized() -> Result` (error when `modulus <= gp::resolution()`).
The four `SetLinearForm` overloads have one name each: `set_linear_form_2(a1,&X1,a2,&X2)`
(= a1·X1 + a2·X2), `set_linear_form_2_plus(a1,&X1,a2,&X2,&X3)` (+ X3),
`set_linear_form_1_plus(a1,&X1,&X2)` (= a1·X1 + X2), `set_linear_form_sum(&X1,&X2)`.

## `Pnt2d`

`Pnt2d::new(x,y)`, `from_xy(XY)`, `origin()`, `Default` (origin). `x()`, `y()`, `coords()`,
`coord(i) -> Result<f64>`, `xy() -> &XY`, `set_coord(x,y)`, `set_coord_at(i,v) -> Result`,
`set_x`, `set_y`, `set_xy(XY)`, `change_coord() -> &mut XY`, `is_equal(&Pnt2d,tol)` (distance
`<= tol`), `distance`, `square_distance`.
Geometry: `mirror/mirrored` about `&Pnt2d` or `&Ax2d` (one generic method over the public trait
`Pnt2dMirror`, implemented for both; same pattern as `PntMirror`), `rotate/rotated(&Pnt2d, angle)`,
`scale/scaled(&Pnt2d, factor)`, `transform/transformed(&Trsf2d)`, `translate/translated(&Vec2d)`,
`translate_points/translated_points(&Pnt2d,&Pnt2d)`.

## `Vec2d`

`Vec2d::new(x,y)`, `from_xy(XY)`, `from_dir(&Dir2d)`, `between(&Pnt2d,&Pnt2d)`, `zero()`, `Default`
(zero). Coordinate accessors and setters as for `Pnt2d` (including `xy() -> &XY`, `set_xy(XY)`).
Queries: `is_equal(&Vec2d, linear_tol, angular_tol) -> Result<bool>`,
`is_normal/is_opposite/is_parallel(&Vec2d, angular_tol) -> Result<bool>`,
`angle(&Vec2d) -> Result<f64>` (error when either magnitude is `<= gp::resolution()`), `magnitude`,
`square_magnitude`, `dot`, `crossed(&Vec2d) -> f64`, `cross_magnitude`, `cross_square_magnitude`,
`get_normal() -> Vec2d` (= `(y, -x)`).
Arithmetic: `add/added`, `subtract/subtracted`, `multiply/multiplied(f64)`, `divide/divided(f64)`,
`normalize/normalized -> Result`, `reverse/reversed`, `set_linear_form_2/_2_plus/_1_plus/_sum` as for `XY`.
Geometry: `mirror/mirrored` about `&Vec2d` or `&Ax2d` (trait `Vec2dMirror`; a zero mirror vector
leaves the vector unchanged), `rotate/rotated(angle)`, `scale/scaled(f64)`,
`transform/transformed(&Trsf2d)`. `gp_Vec2d` has no translate method.

## `Dir2d`

`Dir2d::new(x,y) -> Result` (normalizes; an input whose squared norm is within `1 ± resolution` is
kept as given; error when the squared norm is `<= resolution²`), `from_xy(XY) -> Result`,
`from_vec(&Vec2d) -> Result`, `from_direction(Direction2d)` (exact `(±1,0)`/`(0,±1)`), `Default` is
`(1,0)`. `x()`, `y()`, `coords()`, `coord(i) -> Result<f64>`, `xy() -> &XY`. Setters that renormalize
are fallible: `set_coord(x,y) -> Result`, `set_coord_at(i,v) -> Result`, `set_x(f64) -> Result`,
`set_y(f64) -> Result`, `set_xy(XY) -> Result`.
Queries: `is_equal/is_normal/is_opposite/is_parallel(&Dir2d, angular_tol) -> bool`,
`angle(&Dir2d) -> f64` (signed, never fails), `crossed(&Dir2d) -> f64`, `dot`.
`reverse/reversed`, `mirror/mirrored` about `&Dir2d` or `&Ax2d` (trait `Dir2dMirror`),
`rotate/rotated(angle)`, `transform/transformed(&Trsf2d)`.

## `Ax2d`

`Ax2d::new(Pnt2d, Dir2d)`, `from_standard_direction(Direction2d)` (the explicit one-argument
constructor, location at the origin), `from_point_standard(Pnt2d, Direction2d)` (the C++
`gp_Ax2d(P, gp_Dir2d::D)` overload), `Default` (origin, direction `(1,0)`). `location() -> &Pnt2d`,
`direction() -> &Dir2d`, `set_location(Pnt2d)`, `set_direction(Dir2d)`,
`is_coaxial(&Ax2d, angular_tol, linear_tol)`, `is_normal/is_opposite/is_parallel(&Ax2d, angular_tol)`,
`angle(&Ax2d) -> f64`, `reverse/reversed`, `mirror/mirrored` about `&Pnt2d` or `&Ax2d`
(trait `Ax2dMirror`), `rotate/rotated(&Pnt2d, angle)`, `scale/scaled(&Pnt2d, f)`,
`transform/transformed(&Trsf2d)`, `translate/translated(&Vec2d)`,
`translate_points/translated_points(&Pnt2d,&Pnt2d)`.

## `Ax22d`

A right- or left-handed frame (location, X direction, Y direction). `Default` is the origin with
X `(1,0)` and Y `(0,1)`. The three constructors are separate names:
`Ax22d::from_dirs(Pnt2d, Dir2d vx, Dir2d vy)` (`gp_Ax22d(P, Vx, Vy)`: Y is `(-vx.y, vx.x)` when
`vx.crossed(vy) >= 0`, else `(vx.y, -vx.x)`), `Ax22d::from_dir(Pnt2d, Dir2d, is_sense: bool)`
(`gp_Ax22d(P, V, sense)`), `Ax22d::from_ax2d(Ax2d, is_sense: bool)` (`gp_Ax22d(A, sense)`).
`location() -> &Pnt2d`, `x_direction()`, `y_direction() -> &Dir2d`, `x_axis()`, `y_axis() -> Ax2d`.
Setters `set_axis(Ax22d)`, `set_x_axis(Ax2d)`, `set_y_axis(Ax2d)`, `set_location(Pnt2d)`,
`set_x_direction(Dir2d)`, `set_y_direction(Dir2d)` (the handedness is kept, as in OCCT; the setters
renormalize through `Dir2d` and cannot fail on unit input). `mirror/mirrored` about `&Pnt2d` or
`&Ax2d` (trait `Ax22dMirror`), `rotate/rotated(&Pnt2d, angle)`, `scale/scaled(&Pnt2d, f)`
(a negative factor reverses both directions), `transform/transformed(&Trsf2d)`,
`translate/translated(&Vec2d)`, `translate_points/translated_points(&Pnt2d,&Pnt2d)`.

## `Mat2d`

`Mat2d::new()` (the zero matrix, as C++ `gp_Mat2d()`), `Default` (zero),
`from_columns(XY col1, XY col2)`. `set_col/set_row(index,&XY) -> Result`, `set_cols/set_rows(&XY,&XY)`,
`set_diagonal(x1,x2)`, `set_identity()`, `set_rotation(angle)`, `set_scale(f)`,
`set_value(row,col,v) -> Result`. Queries `column/row(index) -> Result<XY>`, `diagonal() -> XY`,
`value(row,col) -> Result<f64>`, `change_value(row,col) -> Result<&mut f64>`, `determinant`,
`is_singular` (`|det| <= gp::resolution()`). Arithmetic: `add/added`, `subtract/subtracted`,
`divide/divided(f64)`, `multiply_scalar/multiplied_scalar(f64)`, `multiply_mat/multiplied_mat(&Mat2d)`,
`pre_multiply(&Mat2d)`, `invert/inverted -> Result`, `power/powered(i32) -> Result`,
`transpose/transposed`. Errors as listed in *Conventions* (bad indices; singular inversion; the
`unchecked` cases of `column/row/set_col/set_row/invert/power`).

## `Trsf2d`

`Trsf2d::new()` is the identity (`Default` too). The C++ `gp_Trsf2d(const gp_Trsf&)` constructor is
`Trsf2d::from_trsf(&Trsf)`: it copies the scale factor, the form, the translation `(x, y)` and the upper-left 2×2 block
of `h_vectorial_part()` of the 3D transformation (even when that block is not orthogonal).
Setters: `set_mirror(&Pnt2d | &Ax2d)` (generic over the public trait `Trsf2dMirror`),
`set_rotation(&Pnt2d, angle)`, `set_scale(&Pnt2d, f)` (any factor, zero included),
`set_transformation(&Ax2d to)`, `set_transformation_between(&Ax2d from, &Ax2d to)`,
`set_translation(&Vec2d)`, `set_translation_points(&Pnt2d,&Pnt2d)`, `set_translation_part(&Vec2d)`,
`set_scale_factor(f)` (no check), `set_values([a11,a12,a13,a21,a22,a23]) -> Result` (the
`Orthogonalize` step is protected and not part of the API).
Queries: `is_negative`, `form() -> TrsfForm`, `scale_factor`, `translation_part() -> &XY`,
`vectorial_part() -> Mat2d`, `h_vectorial_part() -> &Mat2d`, `rotation_part() -> f64`,
`value(row,col) -> Result<f64>` (rows 1..=2, columns 1..=3).
Operations: `invert/inverted -> Result`, `multiplied(&Trsf2d) -> Trsf2d`, `multiply(&Trsf2d)`,
`pre_multiply(&Trsf2d)`, `power/powered(i32) -> Result`, `transforms_xy(&self, &mut XY)`,
`transforms_coords(x,y) -> (f64,f64)`. `multiply`, `pre_multiply` follow every shape branch of
OCCT (and the resulting `form`, `scale_factor`, matrix and translation are all checked).

## `GTrsf2d`

`GTrsf2d::new()` (identity, `Default`), `from_trsf2d(&Trsf2d)`, `from_parts(Mat2d, XY)` (form `Other`).
Setters `set_affinity(&Ax2d, ratio)`, `set_value(row,col,v) -> Result`, `set_translation_part(&XY)`,
`set_trsf2d(&Trsf2d)`, `set_vectorial_part(&Mat2d)`. Queries `is_negative`, `is_singular`, `form`,
`translation_part() -> &XY`, `vectorial_part() -> &Mat2d` (the raw matrix: the hidden scale factor
is visible through `value`), `value(row,col) -> Result<f64>` (rows 1..=2, columns 1..=3; the scale
factor multiplies the matrix part unless the form is `Other`), `trsf2d() -> Result<Trsf2d>` (error when
the form is `Other` or the matrix is not orthogonal within `2·1e-12`/`1e-12`).
Operations `invert/inverted -> Result`, `multiplied(&GTrsf2d) -> Result<GTrsf2d>`,
`multiply(&GTrsf2d) -> Result`, `pre_multiply(&GTrsf2d) -> Result`, `power/powered(i32) -> Result`,
`transforms_xy(&self, &mut XY)`, `transformed(&XY) -> XY`, `transforms_coords(x,y) -> (f64,f64)`.

## `Lin2d`

`Lin2d::new(Pnt2d, Dir2d)`, `from_ax2d(Ax2d)`, `from_coefficients(a,b,c) -> Result` (line `a·x + b·y + c = 0`),
`Default` (the default `Ax2d`). `location() -> &Pnt2d`, `direction() -> &Dir2d`, `position() -> &Ax2d`,
`set_location(Pnt2d)`, `set_direction(Dir2d)`, `set_position(Ax2d)`, `coefficients() -> (a,b,c)`,
`reverse/reversed`, `angle(&Lin2d) -> f64`, `contains(&Pnt2d, linear_tol) -> bool`,
`distance(&T)` and `square_distance(&T)` generic over the public trait `Lin2dDistance` implemented for
`Pnt2d` and `Lin2d` (parallel lines: the distance of the other location; otherwise 0), `normal(&Pnt2d) -> Lin2d`.
Geometry as for `Ax2d`: `mirror/mirrored` (trait `Lin2dMirror`, `&Pnt2d`/`&Ax2d`),
`rotate/rotated(&Pnt2d, angle)`, `scale/scaled`, `transform/transformed`, `translate/translated(&Vec2d)`,
`translate_points/translated_points`.

## `Circ2d`, `Elips2d`, `Hypr2d`, `Parab2d`

All four share: `location() -> &Pnt2d`, `axis()` (and, for `Circ2d`, `position()`) `-> &Ax22d`,
`x_axis()`, `y_axis() -> Ax2d`, `is_direct()`, `reverse/reversed` (flips the Y direction),
`set_location(Pnt2d)`, `set_axis(Ax22d)`, `coefficients() -> (a,b,c,d,e,f)`, geometry
`mirror/mirrored` (`&Pnt2d`/`&Ax2d`, traits `Circ2dMirror`, `Elips2dMirror`, `Hypr2dMirror`, `Parab2dMirror`),
`rotate/rotated(&Pnt2d, angle)`, `scale/scaled(&Pnt2d, f)` (radii take the absolute value),
`transform/transformed(&Trsf2d)` (radii are multiplied by the scale factor, absolute value),
`translate/translated(&Vec2d)`, `translate_points/translated_points(&Pnt2d,&Pnt2d)`.

* `Circ2d`: `new(Ax2d, radius, is_sense: bool) -> Result`, `from_ax22d(Ax22d, radius) -> Result`
  (error when `radius < 0`), `Default` (radius `f64::MAX`, i.e. OCCT `RealLast()`), `radius()`,
  `set_radius(f) -> Result`, `set_x_axis(Ax2d)`, `set_y_axis(Ax2d)`, `area`, `length`,
  `contains(&Pnt2d, tol)`, `distance(&Pnt2d)`, `square_distance(&Pnt2d)`.
* `Elips2d`: `new(Ax2d, major, minor, is_sense) -> Result`, `from_ax22d(Ax22d, major, minor) -> Result`
  (error when `minor < 0` or `major < minor`), `Default` (major `f64::MAX`, minor `gp::resolution()`),
  `major_radius`, `minor_radius`, `set_major_radius(f) -> Result`, `set_minor_radius(f) -> Result`,
  `set_x_axis(Ax2d)`, `set_y_axis(Ax2d)`, `area`, `eccentricity`, `focal`, `parameter`, `focus1`, `focus2 -> Pnt2d`,
  `directrix1/directrix2 -> Result<Ax2d>` (error when `eccentricity <= gp::resolution()`).
* `Hypr2d`: `new(Ax2d, major, minor, is_sense) -> Result`, `from_ax22d(Ax22d, major, minor) -> Result`
  (error when either radius is negative), `Default` (both `f64::MAX`), `major_radius`, `minor_radius`,
  `set_major_radius(f) -> Result`, `set_minor_radius(f) -> Result`, `set_x_axis`, `set_y_axis`,
  `asymptote1/asymptote2 -> Result<Ax2d>`, `directrix1/directrix2 -> Result<Ax2d>`,
  `eccentricity -> Result<f64>`, `parameter -> Result<f64>` (errors when `major <= gp::resolution()`),
  `conjugate_branch1/conjugate_branch2/other_branch -> Hypr2d`, `focal`, `focus1`, `focus2 -> Pnt2d`.
* `Parab2d`: `new(Ax2d mirror_axis, focal, is_sense) -> Result`, `from_ax22d(Ax22d, focal) -> Result`
  (error when `focal < 0`), `from_directrix_focus(&Ax2d directrix, &Pnt2d focus, is_sense) -> Parab2d`
  (the third C++ constructor, no failure), `Default` (focal `f64::MAX`), `focal`, `parameter`
  (`2·focal`), `set_focal(f) -> Result`, `set_mirror_axis(Ax2d)`, `mirror_axis() -> Ax2d`,
  `focus() -> Pnt2d`, `directrix() -> Ax2d`.

## OCCT behaviors the golden file pins down (reproduce them, do not "fix" them)

* **Self-multiplication of `gp_Mat2d`.** `gp_Mat2d::Multiply(const gp_Mat2d&)` writes `myMat[0][1]` before it
  reads `theOther.myMat[0][1]` for the `[1][1]` entry. `gp_Mat2d::Power`, `gp_Trsf2d::Power` (the `Rotation`
  and the compound branch) and `gp_GTrsf2d::Power` square a temporary with `Tempmatrix.Multiply(Tempmatrix)`,
  where both operands are the same object, so the entry `[1][1]` of the squared matrix is
  `m10 * m01' + m11 * m11` with `m01' = m00 * m01 + m01 * m11` (the already overwritten `m01`) instead of
  `m10 * m01 + m11 * m11`. The results of `power`/`powered` with an exponent whose binary expansion needs a
  squaring (|n| >= 3) therefore differ from the mathematically correct power, and the Rust translation has to
  compute exactly this (an internal "square in place like OCCT" helper). The ordinary `multiply_mat(&other)`
  with a distinct operand is unaffected.
* **`gp_Lin2d` translations.** `translated_points(p1, p2)` goes through `gp_Vec2d(p1, p2)` (`p2 - p1` first),
  whereas the in-place `translate_points(p1, p2)` adds `p2` and then subtracts `p1`; the two differ in the last bit.
* **Which methods use the hidden scale.** `Trsf2d::value` multiplies the matrix part by the scale factor,
  `GTrsf2d::value` only when the form is not `Other`, `Trsf2d::vectorial_part` multiplies only the diagonal for
  the `Scale`/`PntMirror` forms; `h_vectorial_part` is the raw matrix.
* The 2D forms follow `gp_Trsf2d::Multiply/PreMultiply` branch by branch (the test compares `form`,
  `scale_factor`, the raw matrix and the translation after every product over 121 form/kind combinations).

## Header-by-header coverage

* `gp.hxx`: the 2D constants `Origin2d`, `DX2d`, `DY2d`, `OX2d`, `OY2d` (family `constants2d`).
* `gp_XY.hxx`, `gp_Pnt2d.hxx`, `gp_Vec2d.hxx`, `gp_Dir2d.hxx`, `gp_Ax2d.hxx`, `gp_Ax22d.hxx`,
  `gp_Mat2d.hxx`, `gp_Trsf2d.hxx`, `gp_GTrsf2d.hxx`, `gp_Lin2d.hxx`, `gp_Circ2d.hxx`,
  `gp_Elips2d.hxx`, `gp_Hypr2d.hxx`, `gp_Parab2d.hxx`: every public member is exercised by at least one
  family of the oracle (see the `FAMILIES` list in `tests/gp2d.rs`). The C++ operators map to the named
  methods; `DumpJson`, assignment and the protected `Trsf2d::Orthogonalize` are not part of the API.
* Overload naming summary (each name once): `Ax2d::{new, from_point_standard, from_standard_direction}`,
  `Dir2d::{new, from_xy, from_vec, from_direction}`, `Ax22d::{from_dirs, from_dir, from_ax2d}`,
  `Circ2d::{new, from_ax22d}` (same for `Elips2d`, `Hypr2d`, `Parab2d`; plus `Parab2d::from_directrix_focus`),
  `Trsf2d::{set_transformation, set_transformation_between, set_translation, set_translation_points}`,
  `XY::{multiply, multiply_xy, multiply_mat}`, `Mat2d::{multiply_scalar, multiply_mat}`.
