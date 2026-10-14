# `gp` 3D Rust API contract

This contract covers OCCT 8.0.1's 3D `gp` package. All items below are in
`occt_port::gp`; C++ names are mapped to Rust `UpperCamelCase` types and
`snake_case` methods. Constructors use `Type::new(...)`, and output-reference
arguments return tuples in their original argument order. Methods taking an
OCCT object by reference take `&Type` unless mutation of that object is part
of the operation. Mutating C++ methods mutate `&mut self`; `Fooed` methods
return a copy. Scalars are `f64`, integer indices/exponents are `i32`, and
boolean values are `bool`.

Fallible constructors/operations use `Result<T, ConstructionError>`. The
public zero-sized `gp::ConstructionError` represents OCCT construction,
domain, range, and singular-matrix failures. Specifically this includes
zero-magnitude XYZ/Vec/Dir normalization and direction construction, invalid
1-based indices, invalid axis placements, invalid scale parameters, and
inversion/power of a singular matrix. Quaternion normalization stabilizes a
zero quaternion to identity, matching OCCT. No panic is part of the API contract.

## Package constants

`gp::resolution() -> f64`; `gp::origin() -> Pnt`; `gp::dx/dy/dz() -> Dir`;
`gp::ox/oy/oz() -> Ax1`; `gp::xoy/zox/yoz() -> Ax2`.

`gp::Direction` is the six-value enum `X`, `Y`, `Z`, `NegativeX`, `NegativeY`,
`NegativeZ`, corresponding to `gp_Dir::D`.

## `XYZ`

`XYZ::new(x,y,z)`, `XYZ::zero()`, and `Default` (zero). Accessors `x/y/z() -> f64`,
`coord(index: i32) -> Result<f64, ConstructionError>`,
`coords() -> (f64,f64,f64)`, `modulus/square_modulus()`,
`cross_magnitude/cross_square_magnitude(&XYZ)`, `dot(&XYZ)`,
`dot_cross(&XYZ,&XYZ)`, `is_equal(&XYZ,tol) -> bool`, `get_data() -> [f64;3]`,
and `change_data() -> &mut [f64;3]` (mutable counterpart to C++ `ChangeData`).
Setters: `set_coord(x,y,z)`, `set_coord_at(index,value) -> Result`,
`set_x/y/z`, `change_coord(index) -> Result<&mut f64,...>`, `add/subtract/cross`,
`cross_cross(&XYZ,&XYZ)`, `divide(f64)`, `multiply(f64|&XYZ|&Mat)`,
`normalize() -> Result`, `reverse()`, `set_linear_form(...)`. Copy forms:
`added/subtracted/crossed/cross_crossed/divided/multiplied/normalized/reversed`.
The C++ `SetLinearForm` overloads have one Rust name each (Rust has no overloading):
`set_linear_form_2(a1,&X1,a2,&X2)` = a1·X1 + a2·X2,
`set_linear_form_2_plus(a1,&X1,a2,&X2,&X3)` = a1·X1 + a2·X2 + X3,
`set_linear_form_3(a1,&X1,a2,&X2,a3,&X3)`,
`set_linear_form_3_plus(a1,&X1,a2,&X2,a3,&X3,&X4)`,
`set_linear_form_1_plus(a1,&X1,&X2)` = a1·X1 + X2, and `set_linear_form_sum(&X1,&X2)`.
`add`, `cross` and `dot` are public and take `&self`/`&mut self` and `&XYZ`.

## `Pnt`

Constructors `new(x,y,z)`, `from_xyz(XYZ)`, `origin()`, and `Default` (origin).
Accessors/setters
mirror `XYZ`, including checked 1-based coordinate access. `xyz() -> &XYZ`,
`change_coord() -> &mut XYZ`,
`set_xyz(XYZ)`, `bary_center(alpha,&Pnt,beta)`,
`is_equal(&Pnt,tolerance)`, `distance/square_distance(&Pnt)`. Geometric methods:
`mirror/mirrored` about `&Pnt`, `&Ax1`, or `&Ax2`; `rotate/rotated(&Ax1,angle)`;
`scale/scaled(&Pnt,factor)`; `transform/transformed(&Trsf)`;
`translate/translated(&Vec)` and `translate_points/translated_points(&Pnt,&Pnt)`
(the same two names on `Ax1`, `Ax2` and `Ax3`).

## `Vec`

Constructors `new(x,y,z)`, `from_xyz(XYZ)`, `from_dir(&Dir)`,
`between(&Pnt,&Pnt)`, and `zero()`. Coordinate accessors/setters mirror `Pnt`,
including `xyz() -> &XYZ`.
Queries: `is_equal(&Vec,linear_tol,angular_tol)`,
`is_normal/is_opposite/is_parallel(&Vec,angular_tol) -> Result<bool, ConstructionError>`,
`angle(&Vec) -> Result<f64, ConstructionError>`,
`angle_with_ref(&Vec,&Vec) -> Result<f64, ConstructionError>`, `magnitude/square_magnitude`,
`cross_magnitude/cross_square_magnitude`, `dot`, `dot_cross`.
Arithmetic mutators and copy forms: `add/added`, `subtract/subtracted`,
`multiply/multiplied(f64)`, `divide/divided(f64)`, `cross/crossed`,
`cross_cross/cross_crossed`, `normalize/normalized`, `reverse/reversed`,
`set_linear_form_2/_2_plus/_3/_3_plus/_1_plus/_sum` as for `XYZ`, on `Vec`. Geometric
methods: `mirror/mirrored` about `&Vec`, `&Ax1`, `&Ax2`; `rotate/rotated`;
`scale/scaled`; `transform/transformed`. `gp_Vec` has no translate method.

## `Dir`

`new(x,y,z)`, `from_xyz(XYZ)`, `from_vec(&Vec)`, `from_direction(Direction)`,
`default_x/default_y/default_z()`; `Default` is the positive X direction.
`xyz() -> &XYZ`; coordinates and setters as for `XYZ`; all constructors and setters that
normalize are fallible. Queries `is_equal/is_normal/is_opposite/is_parallel`,
`angle/angle_with_ref`, `dot/dot_cross`. Operations `cross/crossed`,
`cross_cross/cross_crossed` return `Result` because a zero result cannot be
normalized, `reverse/reversed`, `mirror/mirrored` about
`&Dir`, `&Ax1`, `&Ax2`, `rotate/rotated`, and `transform/transformed`.

## `Ax1`, `Ax2`, `Ax3`

`Ax1::new(Pnt,Dir)`, `from_direction(Direction)`,
`at(Pnt,Direction)`; `Default` is the positive Z axis.
`location/direction`, `set_location/set_direction`,
`is_coaxial(&Ax1,angular_tol,linear_tol)`, `is_normal/is_opposite/is_parallel(&Ax1,angular_tol)`, `angle`, `reverse/reversed`,
`mirror/mirrored` about point/axis/plane, `rotate/rotated`, `scale/scaled`,
`transform/transformed`, and both translation overloads.

`Ax2::new(Pnt,Dir,&Dir)` (preserving the existing constructor signature),
`from_direction(Pnt,Dir)`, `from_standard_directions(Pnt,Direction,Direction)`,
`from_point_standard(Pnt,Direction)` (the C++ `gp_Ax2(P, gp_Dir::D)` overload, whose
signed zeros differ from `from_direction`), and `from_standard_direction(Direction)`;
`Default` is the reference frame. `axis`,
`location/direction/x_direction/y_direction`, `set_axis/set_direction/`
`set_location/set_x_direction/set_y_direction`, `angle`,
`is_coplanar(&Ax2|&Ax1,linear_tol,angular_tol)`, and geometric operations as
for `Ax1`. Protected helper routines in the C++ header are not part of the API.

`Ax3::new(Pnt,Dir,Dir)`, `from_ax2(&Ax2)`, `from_direction(Pnt,Dir)`,
`from_standard_directions(Pnt,Direction,Direction)`, `from_point_standard(Pnt,Direction)`,
and `from_standard_direction(Direction)`; `Default` is the reference frame.
all `Ax2` accessors and setters, `ax2()`, `direct()`, `x_reverse/y_reverse/z_reverse`,
`is_coplanar(&Ax3|&Ax1,linear_tol,angular_tol)`, and geometric operations as
for `Ax2`.

## `Mat`

Constructors `new()` (the zero matrix, as C++ `gp_Mat()`), `from_values([f64;9])`, and
`from_columns(XYZ,XYZ,XYZ)`. `set_col/set_row(index,&XYZ) -> Result`, `set_cols/set_rows`,
`set_cross(&XYZ)`, `set_dot(&XYZ)`, `set_diagonal(x,y,z)`, `set_identity`,
`set_rotation(&XYZ,angle) -> Result`, `set_scale(f64)`,
`set_value(row,col,value) -> Result`. Queries `column/row(index) -> Result<XYZ>`,
`diagonal`, `value(row,col) -> Result<f64>`, `change_value(row,col) -> Result<&mut f64>`,
`determinant`, `is_singular`. Arithmetic: `add/added`, `subtract/subtracted`,
`divide/divided`, `multiply_mat/multiplied_mat(&Mat)`,
`multiply_scalar/multiplied_scalar(f64)`, `pre_multiply`, `invert/inverted`,
`power/powered(i32)`, `transpose/transposed`. Fallible methods return
`ConstructionError` on invalid indices, or singularity where OCCT raises.

## `Trsf` and `GTrsf`

`Trsf::new()` is identity. Setters: `set_mirror(&Pnt|&Ax1|&Ax2)`,
`set_rotation(&Ax1,angle)` / `set_rotation_quaternion(&Quaternion)`,
`set_rotation_part(&Quaternion)`, `set_scale(&Pnt,f64)`,
`set_displacement(&Ax3,&Ax3)`, `set_transformation(&Ax3)` /
`set_transformation_between(&Ax3,&Ax3)` / `set_transformation_quaternion(&Quaternion,&Vec)`,
`set_translation(&Vec)` / `set_translation_points(&Pnt,&Pnt)`,
`set_translation_part(&Vec)`, `set_scale_factor(f64)`, `set_form(TrsfForm)`,
and `set_values([f64;12]) -> Result`. Queries: `is_negative`, `form`,
`scale_factor`, `translation_part`, `get_rotation() -> (bool,XYZ,f64)` (the C++
success flag and its two output parameters), and
`get_rotation_quaternion() -> Quaternion`, `vectorial_part/h_vectorial_part`,
`value(row,col) -> Result<f64>`. Operations `invert/inverted`,
`multiply/multiplied/pre_multiply`, `power/powered`, `transforms_xyz`,
`transforms_coords(x,y,z) -> (f64,f64,f64)`. Inverse, power, and invalid index
failures return `Result`. `Orthogonalize` is protected in the C++ header and is
not part of the public Rust API.

The public templated `GetMat4` adapter is represented as
`get_mat4() -> [[f64;4];4]`, with OCCT's zero-based 4×4 layout.

`GTrsf::new()`, `from_trsf(&Trsf)`, `from_parts(Mat,XYZ)`. Setters
`set_affinity(&Ax1|&Ax2,ratio)`, `set_value(row,col,value) -> Result`,
`set_vectorial_part(&Mat)`, `set_translation_part(&XYZ)`, `set_trsf(&Trsf)`,
`set_form()`, `set_mat4([[f64;4];4])`, and `get_mat4() -> [[f64;4];4]`.
Queries `is_negative/is_singular/form`,
`translation_part/vectorial_part`, `value(row,col) -> Result<f64>`,
`transforms_xyz`, `transforms_coords`, `trsf() -> Result<Trsf>`.
Operations `invert/inverted`, `multiply/multiplied/pre_multiply`, and
`power/powered`, fallible on singular matrices and invalid indices.

## Quaternion interpolation, enum types

`Quaternion::new()` (identity), `from_components(x,y,z,w)`, `from_vectors(&Vec,&Vec)`,
`from_vectors_with_help(&Vec,&Vec,&Vec)`,
`from_axis_angle(&Vec,angle) -> Result<Quaternion, ConstructionError>`, and
`from_matrix(&Mat)`. Methods include `is_equal`, `set_rotation(&Vec,&Vec)`,
`set_rotation_with_help(&Vec,&Vec,&Vec)`,
`set_vector_and_angle(&Vec,angle) -> Result<(), ConstructionError>`,
`vector_and_angle() -> (Vec,f64)`, `set_matrix/get_matrix`,
`set_euler_angles/get_euler_angles -> (f64,f64,f64)`,
`set_components(x,y,z,w)`, `set_quaternion(&Quaternion)`,
`x/y/z/w`, `set_ident`, `reverse/reversed`, `invert/inverted`,
`square_norm/norm`, `scale/scaled`, `stabilize_length`, `normalize/normalized`,
`negated`, `add/added`, `subtract/subtracted`, `multiply_quaternion/multiplied_quaternion(&Quaternion)`,
`scale/scaled(f64)`, `dot`, `rotation_angle`, and `rotate(&Vec) -> Vec`.

`QuaternionNLerp` and `QuaternionSLerp`: `new()` (default),
`between(&Quaternion,&Quaternion)` (endpoint constructor),
`interpolate(&self,t) -> Quaternion`, `init(&mut self,&Quaternion,&Quaternion)`,
`init_from_unit(&mut self,&Quaternion,&Quaternion)`, and associated
`interpolate_between(&Quaternion,&Quaternion,t) -> Quaternion`.
`EulerSequence` exposes `EulerAngles`, `YawPitchRoll`, and all 24 named
extrinsic/intrinsic variants: `ExtrinsicXYZ`, `ExtrinsicXZY`, `ExtrinsicYZX`,
`ExtrinsicYXZ`, `ExtrinsicZXY`, `ExtrinsicZYX`, `IntrinsicXYZ`, `IntrinsicXZY`,
`IntrinsicYZX`, `IntrinsicYXZ`, `IntrinsicZXY`, `IntrinsicZYX`, `ExtrinsicXYX`,
`ExtrinsicXZX`, `ExtrinsicYZY`, `ExtrinsicYXY`, `ExtrinsicZYZ`, `ExtrinsicZXZ`,
`IntrinsicXYX`, `IntrinsicXZX`, `IntrinsicYZY`, `IntrinsicYXY`, `IntrinsicZXZ`,
and `IntrinsicZYZ`. `TrsfForm` exposes
`Identity`, `Rotation`, `Translation`, `PntMirror`, `Ax1Mirror`, `Ax2Mirror`,
`Scale`, `CompoundTrsf`, and `Other`. `TrsfForm` uses `#[repr(i32)]` with
OCCT's discriminant ordering so values agree with the C++ enum.

## Header-by-header coverage checklist

This checklist follows the public declarations in `cpp/gp/` one class header
at a time. `✓` means the API contract and oracle/test dispatch cover the
public numerical API in that header. C++ stream/JSON diagnostics and
assignment operators are excluded because they are serialization/container
plumbing rather than gp algebra. C++ arithmetic operator overloads map to the
same named Rust arithmetic methods listed above; the oracle checks their
underlying operations through those methods. `XYZ` matrix multiplication is
covered in both copy and in-place forms, and `Pnt` transformation is covered
in both copy and in-place forms. `GetMat4`/`SetMat4` use fixed arrays.

- [x] `gp.hxx`: all 10 in-scope package constants (`Resolution`, `Origin`,
  `DX/DY/DZ`, `OX/OY/OZ`, `XOY/ZOX/YOZ`).
- [x] `gp_XYZ.hxx`: constructors; coordinate read/write/change/data access;
  equality; norm and dot/cross family; arithmetic mutators/copies; normalize,
  reverse; all six linear-form overloads.
- [x] `gp_Pnt.hxx`: constructors; coordinate access/change; barycenter,
  equality and distances; each mirror/rotate/scale/transform/translate
  mutator and copy overload.
- [x] `gp_Vec.hxx`: constructors; coordinate access; relation/angle queries;
  magnitude and dot/cross family; arithmetic mutators/copies; normalization,
  reverse, all linear forms; each mirror/rotate/scale/transform/translate
  overload.
- [x] `gp_Dir.hxx`: constructors/default directions; checked coordinate
  setters; relations/angles/dot-cross; cross and reverse mutators/copies;
  mirror/rotate/transform mutators and copies.
- [x] `gp_Ax1.hxx`: constructor/accessors/setters; coaxiality/relations/angle;
  reverse; each mirror/rotate/scale/transform/translate mutator and copy.
- [x] `gp_Ax2.hxx`: constructors; axis/frame accessors and setters; angle and
  coplanarity overloads; all mirror/rotate/scale/transform/translate mutators
  and copies. Protected helper routines are excluded.
- [x] `gp_Ax3.hxx`: constructors; accessors/setters; `Ax2`, directness and
  reverse queries; coplanarity overloads; all geometric mutators and copies.
- [x] `gp_Mat.hxx`: constructors; every setter/getter/indexed access; singularity
  and determinant; add/subtract/divide/multiply/pre-multiply; inverse, power,
  and transpose mutators/copies.
- [x] `gp_Trsf.hxx`: all setters and overloads; form/scale/translation/rotation
  queries; matrix/value access; inversion, multiply/pre-multiply, power,
  and coordinate transforms. `Orthogonalize` is protected and excluded.
- [x] `gp_GTrsf.hxx`: constructors; all setters including affinity and 4x4
  conversion; queries; inversion, multiplication/pre-multiplication, power,
  point transforms, and conversion to `Trsf`.
- [x] `gp_Quaternion.hxx`: constructors; equality; rotation/vector-angle/matrix/
  Euler setters and getters; component setters/getters; normalization, reverse,
  inverse, stabilize, scalar/quaternion arithmetic, dot, angle, vector multiply.
- [x] `gp_QuaternionNLerp.hxx` and `gp_QuaternionSLerp.hxx`: default and
  endpoint constructors, both init forms, instance interpolation and associated
  interpolation.
- [x] `gp_EulerSequence.hxx` and `gp_TrsfForm.hxx`: all enum variants and the
  `TrsfForm` OCCT discriminants.
- [x] `gp_Dir::D` constructors: all six directions, plus every standard
  direction overload of `Ax1`, `Ax2`, and `Ax3`.
- [x] `gp_Quaternion` Euler conversion: set and recover angles for all 26
  `EulerSequence` values.
- [x] Checked coordinate and matrix indices: invalid read, write, and mutable
  access paths return the documented error.
- [x] Zero magnitude error cases: vector angles and relation queries, matrix
  rotation axes, and quaternion axis-angle construction/setters return errors;
  quaternion zero normalization matches OCCT's identity fallback.
- [x] `gp_Trsf::GetMat4`: fixed-array 4×4 conversion is checked alongside the
  matching `gp_GTrsf` conversion.
- [x] Deterministic edge values: zero, near-zero, near-π, parallel, very large,
  and very small inputs are exercised by the oracle.

## Existing API compatibility

Existing `gp::XYZ`, `Pnt`, `Vec`, `Dir`, `Ax1`, and `Ax2` definitions and all
methods used by `tests/elclib.rs` and `tests/bsplclib.rs` remain source and
signature compatible. This contract does not include 2D classes or curves and
surfaces. Stream JSON methods and pointer aliases are outside the value-algebra
API. Templated `GetMat4/SetMat4` use fixed arrays rather than C++ stream or
template interfaces.
