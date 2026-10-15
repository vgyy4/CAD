# `elslib` and the rest of `elclib`: Rust API contract

This contract covers all of OCCT 8.0.1's `ElSLib` (`cpp/ElSLib/`) and the part of `ElCLib` (`cpp/ElCLib/`) that
`src/elclib.rs` did not have: the 3D `Value`, `D1` and `D2` overloads, every overload taking an `Ax1`/`Ax2`/`Ax2d`/`Ax22d`
(`LineValue`, `CircleD1`, `ParabolaParameter`, ...), every 2D function, and the `To3d` family. The tests are
`tests/elslib.rs` with the golden file `tests/elslib.txt` printed by `oracle/elslib.cpp` (real OCCT). The `gp` types
come from `occt_port::gp` (`docs/api-gp3d.md`, `docs/api-gp2d.md`, `docs/api-gpcs.md`); nothing in them changes.
`src/elclib.rs` keeps every item it has (`d1_lin`, `d3_*`, `d2_parab`, `dn_*`, `parameter_*`, `in_period`,
`adjust_periodic`), and `tests/elclib.rs` stays unchanged and passing; the new items are added beside them. The one
behavior change to an existing item is listed under *OCCT behaviors the golden file pins down* (hyperbola `DN`).

## Conventions

* New module `occt_port::elslib` (add `pub mod elslib;` to `src/lib.rs`); the new `elclib` functions go into the existing
  `occt_port::elclib`. C++ names become `snake_case`. Scalars are `f64`, derivative orders `i32`.
* **Rust has no overloading**, so every C++ overload has its own name:
  * ElSLib, `Ax3` level (the "static" functions that need no surface object): `<surface>_<op>` with surface in
    `plane`, `cylinder`, `cone`, `sphere`, `torus` and op in `value`, `d0`, `d1`, `d2`, `d3`, `dn`, `parameters`, `uiso`,
    `viso` (`ElSLib::CylinderD1` is `elslib::cylinder_d1`). The argument order is the C++ one: `(u, v, pos, shape parameters..)`
    for the evaluators, `(pos, shape parameters.., p)` for `parameters` and `(pos, shape parameters.., u|v)` for the isos;
    the shape parameters are `radius` (cylinder, sphere), `radius, semi_angle` (cone, **radius first**, as in C++) and
    `major_radius, minor_radius` (torus).
  * ElSLib, `gp` object overloads: `<op>_<key>` with key in `pln`, `cylinder`, `cone`, `sphere`, `torus`
    (`ElSLib::D1(u, v, gp_Cylinder, ...)` is `elslib::d1_cylinder(u, v, &cylinder)`). They call the `Ax3`-level function
    with the object's `Position()` and radii (for the cone: `RefRadius()` and `SemiAngle()`).
  * ElCLib, `gp` object overloads: `<op>_<key>` with key in `lin`, `circ`, `elips`, `hypr`, `parab` (3D) and `lin2d`,
    `circ2d`, `elips2d`, `hypr2d`, `parab2d` (2D): `value_circ`, `d1_circ`, `d2_circ`, `d3_circ`, `dn_circ`, `parameter_circ`,
    `value_circ2d`, ... (the names `d1_lin`, `d3_circ`, `d3_elips`, `d3_hypr`, `d2_parab`, `dn_*`, `parameter_*` already exist).
  * ElCLib, position-based overloads (`ElCLib::CircleD1(U, Pos, Radius, P, V1)`): `<curve>_<op>` with curve in `line`,
    `circle`, `ellipse`, `hyperbola`, `parabola`, and `line2d`, `circle2d`, `ellipse2d`, `hyperbola2d`, `parabola2d` for the
    2D ones, and op in `value`, `d1`, `d2`, `d3`, `dn`, `parameter`: `circle_d1(u, &Ax2, radius)`, `circle2d_d1(u, &Ax22d, radius)`.
    `Parameter` of a circle or parabola takes no radius (C++ has none), of an ellipse or hyperbola both radii.
  * `To3d`: one name per overload, `to3d_pnt`, `to3d_vec`, `to3d_dir`, `to3d_ax2d` (the `gp_Ax2d` overload, returns `Ax1`),
    `to3d_ax22d` (the `gp_Ax22d` overload, returns `Ax2`), `to3d_lin`, `to3d_circ`, `to3d_elips`, `to3d_hypr`, `to3d_parab`;
    all take the `Ax2` first.
* **Out-parameters become returned tuples in the C++ argument order.** `D1` of a surface returns `(P, Vu, Vv)`, `D2`
  `(P, Vu, Vv, Vuu, Vvv, Vuv)`, `D3` `(P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)`, `Parameters` returns `(u, v)`; curve
  functions return `(P, V1)`, `(P, V1, V2)`, `(P, V1, V2, V3)`. 3D points and vectors are `Pnt` and `Vec`, 2D ones `Pnt2d`
  and `Vec2d`. Object arguments are `&T`. `D0` returns the point.
* **Errors.** A function returns `Result<_, gp::ConstructionError>` only where the table says so; every other function
  is infallible, in particular all ElSLib functions and all 3D ElCLib functions (the existing 3D `parameter_*` return `f64`). The failures are
  * the 2D `Parameter` of a circle and of an ellipse (family `elclib_circ2d_parameter` / `elclib_elips2d_parameter`, see below);
  * the `To3d` functions that build a `Dir`, `Ax1`, `Ax2` or a curve (`gp_Dir(Vec)`, `gp_Ax2(P, N, Vx)`, the curve
    constructors). They cannot fail for the inputs of the test, but their Rust constructors return `Result`.
* The golden file contains no NaN or infinity (the oracle aborts on one), so the Rust code never has to reproduce them.
* Exact comparisons (`==`, `<`, `<=`) are kept as written in OCCT, as in the other ports.

## The functions

`(exists)` marks items that `src/elclib.rs` already has; their signatures are as in the table
(`c` is the curve object, `pos` the frame of the position-based overload).

### elslib
| Rust (`occt_port::elslib::`) | arguments | returns | C++ |
|---|---|---|---|
| `plane_value` | `u: f64, v: f64, pos: &Ax3` | `Pnt` | `ElSLib::PlaneValue` |
| `plane_d0` | `u: f64, v: f64, pos: &Ax3` | `Pnt` | `ElSLib::PlaneD0` |
| `plane_d1` | `u: f64, v: f64, pos: &Ax3` | `(Pnt, Vec, Vec)` | `ElSLib::PlaneD1 (P, Vu, Vv)` |
| `plane_dn` | `u: f64, v: f64, pos: &Ax3, nu: i32, nv: i32` | `Vec` | `ElSLib::PlaneDN` |
| `plane_parameters` | `pos: &Ax3, p: &Pnt` | `(f64, f64)` | `ElSLib::PlaneParameters (U, V)` |
| `plane_uiso` | `pos: &Ax3, u: f64` | `Lin` | `ElSLib::PlaneUIso` |
| `plane_viso` | `pos: &Ax3, v: f64` | `Lin` | `ElSLib::PlaneVIso` |
| `value_pln` | `u: f64, v: f64, s: &Pln` | `Pnt` | `ElSLib::Value` |
| `d0_pln` | `u: f64, v: f64, s: &Pln` | `Pnt` | `ElSLib::D0` |
| `d1_pln` | `u: f64, v: f64, s: &Pln` | `(Pnt, Vec, Vec)` | `ElSLib::D1` |
| `dn_pln` | `u: f64, v: f64, s: &Pln, nu: i32, nv: i32` | `Vec` | `ElSLib::DN` |
| `parameters_pln` | `s: &Pln, p: &Pnt` | `(f64, f64)` | `ElSLib::Parameters` |
| `cylinder_value` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `Pnt` | `ElSLib::CylinderValue` |
| `cylinder_d0` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `Pnt` | `ElSLib::CylinderD0` |
| `cylinder_d1` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `(Pnt, Vec, Vec)` | `ElSLib::CylinderD1 (P, Vu, Vv)` |
| `cylinder_d2` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::CylinderD2 (P, Vu, Vv, Vuu, Vvv, Vuv)` |
| `cylinder_d3` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::CylinderD3 (P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)` |
| `cylinder_dn` | `u: f64, v: f64, pos: &Ax3, radius: f64, nu: i32, nv: i32` | `Vec` | `ElSLib::CylinderDN` |
| `cylinder_parameters` | `pos: &Ax3, radius: f64, p: &Pnt` | `(f64, f64)` | `ElSLib::CylinderParameters (U, V)` |
| `cylinder_uiso` | `pos: &Ax3, radius: f64, u: f64` | `Lin` | `ElSLib::CylinderUIso` |
| `cylinder_viso` | `pos: &Ax3, radius: f64, v: f64` | `Circ` | `ElSLib::CylinderVIso` |
| `value_cylinder` | `u: f64, v: f64, s: &Cylinder` | `Pnt` | `ElSLib::Value` |
| `d0_cylinder` | `u: f64, v: f64, s: &Cylinder` | `Pnt` | `ElSLib::D0` |
| `d1_cylinder` | `u: f64, v: f64, s: &Cylinder` | `(Pnt, Vec, Vec)` | `ElSLib::D1` |
| `d2_cylinder` | `u: f64, v: f64, s: &Cylinder` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D2` |
| `d3_cylinder` | `u: f64, v: f64, s: &Cylinder` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D3` |
| `dn_cylinder` | `u: f64, v: f64, s: &Cylinder, nu: i32, nv: i32` | `Vec` | `ElSLib::DN` |
| `parameters_cylinder` | `s: &Cylinder, p: &Pnt` | `(f64, f64)` | `ElSLib::Parameters` |
| `cone_value` | `u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64` | `Pnt` | `ElSLib::ConeValue` |
| `cone_d0` | `u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64` | `Pnt` | `ElSLib::ConeD0` |
| `cone_d1` | `u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64` | `(Pnt, Vec, Vec)` | `ElSLib::ConeD1 (P, Vu, Vv)` |
| `cone_d2` | `u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::ConeD2 (P, Vu, Vv, Vuu, Vvv, Vuv)` |
| `cone_d3` | `u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::ConeD3 (P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)` |
| `cone_dn` | `u: f64, v: f64, pos: &Ax3, radius: f64, semi_angle: f64, nu: i32, nv: i32` | `Vec` | `ElSLib::ConeDN` |
| `cone_parameters` | `pos: &Ax3, radius: f64, semi_angle: f64, p: &Pnt` | `(f64, f64)` | `ElSLib::ConeParameters (U, V)` |
| `cone_uiso` | `pos: &Ax3, radius: f64, semi_angle: f64, u: f64` | `Lin` | `ElSLib::ConeUIso` |
| `cone_viso` | `pos: &Ax3, radius: f64, semi_angle: f64, v: f64` | `Circ` | `ElSLib::ConeVIso` |
| `value_cone` | `u: f64, v: f64, s: &Cone` | `Pnt` | `ElSLib::Value` |
| `d0_cone` | `u: f64, v: f64, s: &Cone` | `Pnt` | `ElSLib::D0` |
| `d1_cone` | `u: f64, v: f64, s: &Cone` | `(Pnt, Vec, Vec)` | `ElSLib::D1` |
| `d2_cone` | `u: f64, v: f64, s: &Cone` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D2` |
| `d3_cone` | `u: f64, v: f64, s: &Cone` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D3` |
| `dn_cone` | `u: f64, v: f64, s: &Cone, nu: i32, nv: i32` | `Vec` | `ElSLib::DN` |
| `parameters_cone` | `s: &Cone, p: &Pnt` | `(f64, f64)` | `ElSLib::Parameters` |
| `sphere_value` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `Pnt` | `ElSLib::SphereValue` |
| `sphere_d0` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `Pnt` | `ElSLib::SphereD0` |
| `sphere_d1` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `(Pnt, Vec, Vec)` | `ElSLib::SphereD1 (P, Vu, Vv)` |
| `sphere_d2` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::SphereD2 (P, Vu, Vv, Vuu, Vvv, Vuv)` |
| `sphere_d3` | `u: f64, v: f64, pos: &Ax3, radius: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::SphereD3 (P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)` |
| `sphere_dn` | `u: f64, v: f64, pos: &Ax3, radius: f64, nu: i32, nv: i32` | `Vec` | `ElSLib::SphereDN` |
| `sphere_parameters` | `pos: &Ax3, radius: f64, p: &Pnt` | `(f64, f64)` | `ElSLib::SphereParameters (U, V)` |
| `sphere_uiso` | `pos: &Ax3, radius: f64, u: f64` | `Circ` | `ElSLib::SphereUIso` |
| `sphere_viso` | `pos: &Ax3, radius: f64, v: f64` | `Circ` | `ElSLib::SphereVIso` |
| `value_sphere` | `u: f64, v: f64, s: &Sphere` | `Pnt` | `ElSLib::Value` |
| `d0_sphere` | `u: f64, v: f64, s: &Sphere` | `Pnt` | `ElSLib::D0` |
| `d1_sphere` | `u: f64, v: f64, s: &Sphere` | `(Pnt, Vec, Vec)` | `ElSLib::D1` |
| `d2_sphere` | `u: f64, v: f64, s: &Sphere` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D2` |
| `d3_sphere` | `u: f64, v: f64, s: &Sphere` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D3` |
| `dn_sphere` | `u: f64, v: f64, s: &Sphere, nu: i32, nv: i32` | `Vec` | `ElSLib::DN` |
| `parameters_sphere` | `s: &Sphere, p: &Pnt` | `(f64, f64)` | `ElSLib::Parameters` |
| `torus_value` | `u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64` | `Pnt` | `ElSLib::TorusValue` |
| `torus_d0` | `u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64` | `Pnt` | `ElSLib::TorusD0` |
| `torus_d1` | `u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64` | `(Pnt, Vec, Vec)` | `ElSLib::TorusD1 (P, Vu, Vv)` |
| `torus_d2` | `u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::TorusD2 (P, Vu, Vv, Vuu, Vvv, Vuv)` |
| `torus_d3` | `u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::TorusD3 (P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)` |
| `torus_dn` | `u: f64, v: f64, pos: &Ax3, major_radius: f64, minor_radius: f64, nu: i32, nv: i32` | `Vec` | `ElSLib::TorusDN` |
| `torus_parameters` | `pos: &Ax3, major_radius: f64, minor_radius: f64, p: &Pnt` | `(f64, f64)` | `ElSLib::TorusParameters (U, V)` |
| `torus_uiso` | `pos: &Ax3, major_radius: f64, minor_radius: f64, u: f64` | `Circ` | `ElSLib::TorusUIso` |
| `torus_viso` | `pos: &Ax3, major_radius: f64, minor_radius: f64, v: f64` | `Circ` | `ElSLib::TorusVIso` |
| `value_torus` | `u: f64, v: f64, s: &Torus` | `Pnt` | `ElSLib::Value` |
| `d0_torus` | `u: f64, v: f64, s: &Torus` | `Pnt` | `ElSLib::D0` |
| `d1_torus` | `u: f64, v: f64, s: &Torus` | `(Pnt, Vec, Vec)` | `ElSLib::D1` |
| `d2_torus` | `u: f64, v: f64, s: &Torus` | `(Pnt, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D2` |
| `d3_torus` | `u: f64, v: f64, s: &Torus` | `(Pnt, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec, Vec)` | `ElSLib::D3` |
| `dn_torus` | `u: f64, v: f64, s: &Torus, nu: i32, nv: i32` | `Vec` | `ElSLib::DN` |
| `parameters_torus` | `s: &Torus, p: &Pnt` | `(f64, f64)` | `ElSLib::Parameters` |
### elclib
| Rust (`occt_port::elclib::`) | arguments | returns | C++ |
|---|---|---|---|
| `value_lin` | `u: f64, c: &Lin` | `Pnt` | `ElCLib::Value` |
| `line_value` | `u: f64, pos: &Ax1` | `Pnt` | `ElCLib::LineValue` |
| `d1_lin` (exists) | `u: f64, c: &Lin` | `(Pnt, Vec)` | `ElCLib::D1` |
| `line_d1` | `u: f64, pos: &Ax1` | `(Pnt, Vec)` | `ElCLib::LineD1` |
| `dn_lin` (exists) | `u: f64, c: &Lin, n: i32` | `Vec` | `ElCLib::DN` |
| `line_dn` | `u: f64, pos: &Ax1, n: i32` | `Vec` | `ElCLib::LineDN` |
| `parameter_lin` (exists) | `c: &Lin, p: &Pnt` | `f64` | `ElCLib::Parameter` |
| `line_parameter` | `pos: &Ax1, p: &Pnt` | `f64` | `ElCLib::LineParameter` |
| `value_circ` | `u: f64, c: &Circ` | `Pnt` | `ElCLib::Value` |
| `circle_value` | `u: f64, pos: &Ax2, radius: f64` | `Pnt` | `ElCLib::CircleValue` |
| `d1_circ` | `u: f64, c: &Circ` | `(Pnt, Vec)` | `ElCLib::D1` |
| `circle_d1` | `u: f64, pos: &Ax2, radius: f64` | `(Pnt, Vec)` | `ElCLib::CircleD1` |
| `d2_circ` | `u: f64, c: &Circ` | `(Pnt, Vec, Vec)` | `ElCLib::D2` |
| `circle_d2` | `u: f64, pos: &Ax2, radius: f64` | `(Pnt, Vec, Vec)` | `ElCLib::CircleD2` |
| `d3_circ` (exists) | `u: f64, c: &Circ` | `(Pnt, Vec, Vec, Vec)` | `ElCLib::D3` |
| `circle_d3` | `u: f64, pos: &Ax2, radius: f64` | `(Pnt, Vec, Vec, Vec)` | `ElCLib::CircleD3` |
| `dn_circ` (exists) | `u: f64, c: &Circ, n: i32` | `Vec` | `ElCLib::DN` |
| `circle_dn` | `u: f64, pos: &Ax2, radius: f64, n: i32` | `Vec` | `ElCLib::CircleDN` |
| `parameter_circ` (exists) | `c: &Circ, p: &Pnt` | `f64` | `ElCLib::Parameter` |
| `circle_parameter` | `pos: &Ax2, p: &Pnt` | `f64` | `ElCLib::CircleParameter` |
| `value_elips` | `u: f64, c: &Elips` | `Pnt` | `ElCLib::Value` |
| `ellipse_value` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `Pnt` | `ElCLib::EllipseValue` |
| `d1_elips` | `u: f64, c: &Elips` | `(Pnt, Vec)` | `ElCLib::D1` |
| `ellipse_d1` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `(Pnt, Vec)` | `ElCLib::EllipseD1` |
| `d2_elips` | `u: f64, c: &Elips` | `(Pnt, Vec, Vec)` | `ElCLib::D2` |
| `ellipse_d2` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `(Pnt, Vec, Vec)` | `ElCLib::EllipseD2` |
| `d3_elips` (exists) | `u: f64, c: &Elips` | `(Pnt, Vec, Vec, Vec)` | `ElCLib::D3` |
| `ellipse_d3` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `(Pnt, Vec, Vec, Vec)` | `ElCLib::EllipseD3` |
| `dn_elips` (exists) | `u: f64, c: &Elips, n: i32` | `Vec` | `ElCLib::DN` |
| `ellipse_dn` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64, n: i32` | `Vec` | `ElCLib::EllipseDN` |
| `parameter_elips` (exists) | `c: &Elips, p: &Pnt` | `f64` | `ElCLib::Parameter` |
| `ellipse_parameter` | `pos: &Ax2, major_radius: f64, minor_radius: f64, p: &Pnt` | `f64` | `ElCLib::EllipseParameter` |
| `value_hypr` | `u: f64, c: &Hypr` | `Pnt` | `ElCLib::Value` |
| `hyperbola_value` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `Pnt` | `ElCLib::HyperbolaValue` |
| `d1_hypr` | `u: f64, c: &Hypr` | `(Pnt, Vec)` | `ElCLib::D1` |
| `hyperbola_d1` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `(Pnt, Vec)` | `ElCLib::HyperbolaD1` |
| `d2_hypr` | `u: f64, c: &Hypr` | `(Pnt, Vec, Vec)` | `ElCLib::D2` |
| `hyperbola_d2` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `(Pnt, Vec, Vec)` | `ElCLib::HyperbolaD2` |
| `d3_hypr` (exists) | `u: f64, c: &Hypr` | `(Pnt, Vec, Vec, Vec)` | `ElCLib::D3` |
| `hyperbola_d3` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64` | `(Pnt, Vec, Vec, Vec)` | `ElCLib::HyperbolaD3` |
| `dn_hypr` (exists) | `u: f64, c: &Hypr, n: i32` | `Vec` | `ElCLib::DN` |
| `hyperbola_dn` | `u: f64, pos: &Ax2, major_radius: f64, minor_radius: f64, n: i32` | `Vec` | `ElCLib::HyperbolaDN` |
| `parameter_hypr` (exists) | `c: &Hypr, p: &Pnt` | `f64` | `ElCLib::Parameter` |
| `hyperbola_parameter` | `pos: &Ax2, major_radius: f64, minor_radius: f64, p: &Pnt` | `f64` | `ElCLib::HyperbolaParameter` |
| `value_parab` | `u: f64, c: &Parab` | `Pnt` | `ElCLib::Value` |
| `parabola_value` | `u: f64, pos: &Ax2, focal: f64` | `Pnt` | `ElCLib::ParabolaValue` |
| `d1_parab` | `u: f64, c: &Parab` | `(Pnt, Vec)` | `ElCLib::D1` |
| `parabola_d1` | `u: f64, pos: &Ax2, focal: f64` | `(Pnt, Vec)` | `ElCLib::ParabolaD1` |
| `d2_parab` (exists) | `u: f64, c: &Parab` | `(Pnt, Vec, Vec)` | `ElCLib::D2` |
| `parabola_d2` | `u: f64, pos: &Ax2, focal: f64` | `(Pnt, Vec, Vec)` | `ElCLib::ParabolaD2` |
| `dn_parab` (exists) | `u: f64, c: &Parab, n: i32` | `Vec` | `ElCLib::DN` |
| `parabola_dn` | `u: f64, pos: &Ax2, focal: f64, n: i32` | `Vec` | `ElCLib::ParabolaDN` |
| `parameter_parab` (exists) | `c: &Parab, p: &Pnt` | `f64` | `ElCLib::Parameter` |
| `parabola_parameter` | `pos: &Ax2, p: &Pnt` | `f64` | `ElCLib::ParabolaParameter` |
| `value_lin2d` | `u: f64, c: &Lin2d` | `Pnt2d` | `ElCLib::Value` |
| `line2d_value` | `u: f64, pos: &Ax2d` | `Pnt2d` | `ElCLib::LineValue` |
| `d1_lin2d` | `u: f64, c: &Lin2d` | `(Pnt2d, Vec2d)` | `ElCLib::D1` |
| `line2d_d1` | `u: f64, pos: &Ax2d` | `(Pnt2d, Vec2d)` | `ElCLib::LineD1` |
| `dn_lin2d` | `u: f64, c: &Lin2d, n: i32` | `Vec2d` | `ElCLib::DN` |
| `line2d_dn` | `u: f64, pos: &Ax2d, n: i32` | `Vec2d` | `ElCLib::LineDN` |
| `parameter_lin2d` | `c: &Lin2d, p: &Pnt2d` | `f64` | `ElCLib::Parameter` |
| `line2d_parameter` | `pos: &Ax2d, p: &Pnt2d` | `f64` | `ElCLib::LineParameter` |
| `value_circ2d` | `u: f64, c: &Circ2d` | `Pnt2d` | `ElCLib::Value` |
| `circle2d_value` | `u: f64, pos: &Ax22d, radius: f64` | `Pnt2d` | `ElCLib::CircleValue` |
| `d1_circ2d` | `u: f64, c: &Circ2d` | `(Pnt2d, Vec2d)` | `ElCLib::D1` |
| `circle2d_d1` | `u: f64, pos: &Ax22d, radius: f64` | `(Pnt2d, Vec2d)` | `ElCLib::CircleD1` |
| `d2_circ2d` | `u: f64, c: &Circ2d` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::D2` |
| `circle2d_d2` | `u: f64, pos: &Ax22d, radius: f64` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::CircleD2` |
| `d3_circ2d` | `u: f64, c: &Circ2d` | `(Pnt2d, Vec2d, Vec2d, Vec2d)` | `ElCLib::D3` |
| `circle2d_d3` | `u: f64, pos: &Ax22d, radius: f64` | `(Pnt2d, Vec2d, Vec2d, Vec2d)` | `ElCLib::CircleD3` |
| `dn_circ2d` | `u: f64, c: &Circ2d, n: i32` | `Vec2d` | `ElCLib::DN` |
| `circle2d_dn` | `u: f64, pos: &Ax22d, radius: f64, n: i32` | `Vec2d` | `ElCLib::CircleDN` |
| `parameter_circ2d` | `c: &Circ2d, p: &Pnt2d` | `Result<f64, gp::ConstructionError>` | `ElCLib::Parameter` |
| `circle2d_parameter` | `pos: &Ax22d, p: &Pnt2d` | `Result<f64, gp::ConstructionError>` | `ElCLib::CircleParameter` |
| `value_elips2d` | `u: f64, c: &Elips2d` | `Pnt2d` | `ElCLib::Value` |
| `ellipse2d_value` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `Pnt2d` | `ElCLib::EllipseValue` |
| `d1_elips2d` | `u: f64, c: &Elips2d` | `(Pnt2d, Vec2d)` | `ElCLib::D1` |
| `ellipse2d_d1` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `(Pnt2d, Vec2d)` | `ElCLib::EllipseD1` |
| `d2_elips2d` | `u: f64, c: &Elips2d` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::D2` |
| `ellipse2d_d2` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::EllipseD2` |
| `d3_elips2d` | `u: f64, c: &Elips2d` | `(Pnt2d, Vec2d, Vec2d, Vec2d)` | `ElCLib::D3` |
| `ellipse2d_d3` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `(Pnt2d, Vec2d, Vec2d, Vec2d)` | `ElCLib::EllipseD3` |
| `dn_elips2d` | `u: f64, c: &Elips2d, n: i32` | `Vec2d` | `ElCLib::DN` |
| `ellipse2d_dn` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64, n: i32` | `Vec2d` | `ElCLib::EllipseDN` |
| `parameter_elips2d` | `c: &Elips2d, p: &Pnt2d` | `Result<f64, gp::ConstructionError>` | `ElCLib::Parameter` |
| `ellipse2d_parameter` | `pos: &Ax22d, major_radius: f64, minor_radius: f64, p: &Pnt2d` | `Result<f64, gp::ConstructionError>` | `ElCLib::EllipseParameter` |
| `value_hypr2d` | `u: f64, c: &Hypr2d` | `Pnt2d` | `ElCLib::Value` |
| `hyperbola2d_value` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `Pnt2d` | `ElCLib::HyperbolaValue` |
| `d1_hypr2d` | `u: f64, c: &Hypr2d` | `(Pnt2d, Vec2d)` | `ElCLib::D1` |
| `hyperbola2d_d1` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `(Pnt2d, Vec2d)` | `ElCLib::HyperbolaD1` |
| `d2_hypr2d` | `u: f64, c: &Hypr2d` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::D2` |
| `hyperbola2d_d2` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::HyperbolaD2` |
| `d3_hypr2d` | `u: f64, c: &Hypr2d` | `(Pnt2d, Vec2d, Vec2d, Vec2d)` | `ElCLib::D3` |
| `hyperbola2d_d3` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64` | `(Pnt2d, Vec2d, Vec2d, Vec2d)` | `ElCLib::HyperbolaD3` |
| `dn_hypr2d` | `u: f64, c: &Hypr2d, n: i32` | `Vec2d` | `ElCLib::DN` |
| `hyperbola2d_dn` | `u: f64, pos: &Ax22d, major_radius: f64, minor_radius: f64, n: i32` | `Vec2d` | `ElCLib::HyperbolaDN` |
| `parameter_hypr2d` | `c: &Hypr2d, p: &Pnt2d` | `f64` | `ElCLib::Parameter` |
| `hyperbola2d_parameter` | `pos: &Ax22d, major_radius: f64, minor_radius: f64, p: &Pnt2d` | `f64` | `ElCLib::HyperbolaParameter` |
| `value_parab2d` | `u: f64, c: &Parab2d` | `Pnt2d` | `ElCLib::Value` |
| `parabola2d_value` | `u: f64, pos: &Ax22d, focal: f64` | `Pnt2d` | `ElCLib::ParabolaValue` |
| `d1_parab2d` | `u: f64, c: &Parab2d` | `(Pnt2d, Vec2d)` | `ElCLib::D1` |
| `parabola2d_d1` | `u: f64, pos: &Ax22d, focal: f64` | `(Pnt2d, Vec2d)` | `ElCLib::ParabolaD1` |
| `d2_parab2d` | `u: f64, c: &Parab2d` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::D2` |
| `parabola2d_d2` | `u: f64, pos: &Ax22d, focal: f64` | `(Pnt2d, Vec2d, Vec2d)` | `ElCLib::ParabolaD2` |
| `dn_parab2d` | `u: f64, c: &Parab2d, n: i32` | `Vec2d` | `ElCLib::DN` |
| `parabola2d_dn` | `u: f64, pos: &Ax22d, focal: f64, n: i32` | `Vec2d` | `ElCLib::ParabolaDN` |
| `parameter_parab2d` | `c: &Parab2d, p: &Pnt2d` | `f64` | `ElCLib::Parameter` |
| `parabola2d_parameter` | `pos: &Ax22d, p: &Pnt2d` | `f64` | `ElCLib::ParabolaParameter` |
| `to3d_pnt` | `pos: &Ax2, x: &Pnt2d` | `Pnt` | `ElCLib::To3d(Ax2, Pnt2d)` |
| `to3d_vec` | `pos: &Ax2, x: &Vec2d` | `Vec` | `ElCLib::To3d(Ax2, Vec2d)` |
| `to3d_dir` | `pos: &Ax2, x: &Dir2d` | `Result<Dir, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Dir2d)` |
| `to3d_ax2d` | `pos: &Ax2, x: &Ax2d` | `Result<Ax1, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Ax2d)` |
| `to3d_ax22d` | `pos: &Ax2, x: &Ax22d` | `Result<Ax2, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Ax22d)` |
| `to3d_lin` | `pos: &Ax2, x: &Lin2d` | `Result<Lin, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Lin2d)` |
| `to3d_circ` | `pos: &Ax2, x: &Circ2d` | `Result<Circ, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Circ2d)` |
| `to3d_elips` | `pos: &Ax2, x: &Elips2d` | `Result<Elips, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Elips2d)` |
| `to3d_hypr` | `pos: &Ax2, x: &Hypr2d` | `Result<Hypr, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Hypr2d)` |
| `to3d_parab` | `pos: &Ax2, x: &Parab2d` | `Result<Parab, gp::ConstructionError>` | `ElCLib::To3d(Ax2, Parab2d)` |

## OCCT behaviors the golden file pins down (reproduce them, do not "fix" them)

* **`unchecked` (family `elclib_circ2d_parameter`).** `ElCLib::Parameter(gp_Circ2d, P)` and `CircleParameter(gp_Ax22d, P)`
  convert the vector from the location to `P` to a `gp_Dir2d`; OCCT 8.0.1 as built has the zero-norm check of that conversion
  compiled out, the result is NaN. The oracle prints `unchecked` when `sqrt(vx*vx + vy*vy) <= gp::resolution()` for
  `(vx, vy) = P - location` (so also when the squares underflow: the test has `P = location + (1e-200, 0)` at the origin); the
  Rust functions `parameter_circ2d` and `circle2d_parameter` must return `Err` for that condition (`Dir2d::from_vec` does).
* **`throws` (family `elclib_elips2d_parameter`).** `ElCLib::EllipseParameter(gp_Ax22d, ..)` calls `gp_Vec2d::Angle`, which
  really raises `gp_VectorWithNullMagnitude` when the magnitude (`sqrt(x*x + y*y)`) of `Om` is `<= gp::resolution()`: the point
  is the location, or so close to it that the squares underflow. `parameter_elips2d` and `ellipse2d_parameter` return `Err`.
* **No exception, no `unchecked` in the 3D parameter functions.** The tiny offset line of the 3D families is `1e-100`, whose
  squares do not underflow; the cases where OCCT's `gp_Vec::AngleWithRef` would see a null vector are not in the file because the
  existing 3D `parameter_elips` returns a plain `f64`.
* **Hyperbola `DN` of a negative odd order is the zero vector.** `HyperbolaDN` (3D and 2D) tests `IsOdd(N)` then `IsEven(N)`; OCCT
  defines them as `N % 2 == 1` and `N % 2 == 0` (C truncation), so for `N = -1, -3, ..` neither holds and `Xc = Yc = 0`. The
  existing `elclib::dn_hypr` uses `n % 2 != 0` for odd and returns the wrong vector there (found by the family `elclib_hypr_dn`);
  it must be changed to `n % 2 == 1`. For the other conics the orders are matched with `(N + 2) % 4 == 0`, ... in the C++ order, and
  `ParabolaDN` returns zero outside `1..=2`; `LineDN` is the direction for `N == 1` only. The tests use `N` from `-2` to `8` and
  random orders from `-3` to `9`.
* **Iso curves with a negative radius.** `ElSLib.cxx` is compiled with `No_Exception`; the `Radius < 0` check of the inline
  `gp_Circ` constructor is therefore absent and the iso functions store the negative radius as it is: `SphereUIso` (radius),
  `TorusUIso` (minor radius) and `CylinderVIso` (radius) return a circle whose `radius()` is the negative input (the golden file
  has lines for each). The Rust functions return a plain `Circ` and must build it without `Circ::new` (the fields are
  `pub(crate)`). `ConeVIso`, `SphereVIso` and `TorusVIso` never produce a negative radius: they reverse the frame and negate
  (`ConeVIso`, `TorusVIso`: both `XReverse` and `YReverse` of the `Ax3` and then `Ax2()`; `SphereVIso`: `SetDirection(-Direction)`),
  exactly as in the C++ source. `TorusUIso` and `SphereUIso` build the frame from `cos(u) * XDirection + sin(u) * YDirection`
  as a `gp_Dir` (normalized).
* **Clamp to zero in the torus functions.** `TorusValue` and `TorusDN` (the C++ source of the other torus functions is authoritative) set the values with `|A| <= 10 * (major + minor) * f64::EPSILON` are set to
  `0.0` (the `OCC620` modifications); the test parameters (`u = pi/2`, `pi`, ...) hit them.
* **`normalizeAngle`** (anonymous helper of `ElSLib.cxx`, same as the one in `ElCLib.cxx` and `src/elclib.rs`): angles
  below `-f64::EPSILON` get `2 pi` added, angles above `2 pi (1 + gp::resolution())` get it subtracted, remaining negatives
  become `0`; the closing seam `2 pi` is kept. It is visible through every `*_parameters` / `parameter_*` function (the test has points
  with `y = -1e-20` and `y = -1e-15` next to the seam).
* **Special points of `Parameters`.** Cylinder/cone/sphere/torus take their local coordinates through
  `Trsf::set_transformation(&Ax3)` and `Pnt::transformed` (also for indirect frames, which the test uses). Cone: `U = 0` when
  `|x|` and `|y|` are both `< gp::resolution()` (apex and axis), `atan2(-y, -x)` when `-radius > z * tan(semi_angle)` (wrong side of
  the apex). Sphere: on the axis (`sqrt(x*x + y*y) < gp::resolution()`) `U = 0`, `V = +-pi/2` by the sign of `z` (`z > 0`, else
  `-pi/2`, the origin included). Torus: `MajorRadius < MinorRadius` adds `pi` to `U` when `|D2| < |D1|` (the two squared distances to the core circle points `+-major (cos u, sin u)` minus `minor^2`);
  `V = 0` when the distance to the core circle is `<= gp::resolution()`, else the signed angle `dx.AngleWithRef(dP, dx ^ DZ)`
  (`gp_Dir`) normalized. Plane: the local `x`, `y`.
* **Parameters outside the periods and ranges.** The `u`, `v` of the tests include `0`, `pi/2`, `pi`, `3 pi/2`, `2 pi`, `-pi/2`,
  `5 pi`, `-3 pi`, `+-1e-17`, `2 pi + 1e-9`, `100.5`, `-77.25`, `pi/2 + 1e-8` and `-pi/2 - 1e-16`; nothing is reduced.
  The cone has semi-angles `0`, `pi/2`, `-pi/2`, `1e-9` (no validity check in the `Ax3`-level functions), the torus has
  `minor = 0`, `minor = major`, `minor > major`, `major = 0` and negative radii, the sphere/cylinder/cone radius `0` and negative
  values. The `gp` object families only use legal objects (`radius >= 0`; the cone semi-angle `1e-300` and `+-(pi/2 - 1ulp)` replace
  `0` and `+-pi/2`).
* **`DN` of surfaces.** `Nu`, `Nv` are `0..=4` (all 25 pairs), and `(-1,2)`, `(2,-1)`, `(0,0)`, `(6,0)`, `(0,6)`, `(7,3)`, `(3,7)`, `(-3,-3)`:
  the sphere and torus return the zero vector for `Nu + Nv < 1 || Nu < 0 || Nv < 0`; the plane for orders other than
  `(1,0)` and `(0,1)`; the cylinder unless `Nv == 0` or `(Nu, Nv) == (0, 1)`; the cone for `Nv > 1`. `IsOdd` / `IsEven` are the `%` versions above.
* **Parabola degeneration.** For `|focal| <= gp::resolution()` the parabola is a line (`Value`, `D1`, `D2`, `DN`): the test has
  `focal = 0`, `gp::resolution()` and the next double above it (with small `u` so that `u*u/(4*focal)` stays finite).
* **`To3d`.** The point is `x * XDirection + y * YDirection + location` (`SetLinearForm`), vectors and directions are
  `x * XDirection + y * YDirection` (the direction normalized as a `gp_Dir`); the `Ax22d` overload builds `gp_Ax2(P, VX x VY, VX)`
  so an indirect 2D frame gives a 3D frame whose main direction is reversed (the test has direct and indirect 2D frames); the curve overloads
  call the 3D constructors with the converted axis and the radii.

## Test structure

* Every family prints all its inputs on the left of ` | ` (no fixed inputs), declares a schema, and has 33
  distinct lines (`MIN_DISTINCT` is 16). No family is input-less. The oracle aborts when the number of printed input tokens
  differs from the schema. Floats are exact 16-hex-digit bit patterns, ints decimal; directions are printed raw (un-normalized);
  `ax3` is `pnt N X flag` (flag 1: `XReverse`, 2: `YReverse` of the frame, i.e. indirect frames); `ax22d` is `pnt X Y` built with
  `gp_Ax22d(P, Vx, Vy)`.
* Comparison: families in `EXACT` (only `+ - * /` and `sqrt`: every plane family, `elslib_cylinder_viso`, the line and parabola
  families of `elclib`, `To3d`) are compared bit for bit, signed zeros included; all others within 8 ulps of the largest absolute
  value on the line (the translation uses `libm`, OCCT the C library). `throws` and `unchecked` lines require `Err`.
* The families (all with 33 lines): `elclib_circ2d_dn`, `elclib_circ2d_eval`, `elclib_circ2d_parameter`, `elclib_circ_dn`, `elclib_circ_eval`, `elclib_circ_parameter`, `elclib_elips2d_dn`, `elclib_elips2d_eval`, `elclib_elips2d_parameter`, `elclib_elips_dn`, `elclib_elips_eval`, `elclib_elips_parameter`, `elclib_hypr2d_dn`, `elclib_hypr2d_eval`, `elclib_hypr2d_parameter`, `elclib_hypr_dn`, `elclib_hypr_eval`, `elclib_hypr_parameter`, `elclib_lin2d_dn`, `elclib_lin2d_eval`, `elclib_lin2d_parameter`, `elclib_lin_dn`, `elclib_lin_eval`, `elclib_lin_parameter`, `elclib_parab2d_dn`, `elclib_parab2d_eval`, `elclib_parab2d_parameter`, `elclib_parab_dn`, `elclib_parab_eval`, `elclib_parab_parameter`, `elclib_to3d_ax22d`, `elclib_to3d_ax2d`, `elclib_to3d_circ`, `elclib_to3d_dir`, `elclib_to3d_elips`, `elclib_to3d_hypr`, `elclib_to3d_lin`, `elclib_to3d_parab`, `elclib_to3d_pnt`, `elclib_to3d_vec`, `elslib_cone_d1`, `elslib_cone_d2`, `elslib_cone_d3`, `elslib_cone_dn`, `elslib_cone_object`, `elslib_cone_parameters`, `elslib_cone_uiso`, `elslib_cone_value`, `elslib_cone_viso`, `elslib_cylinder_d1`, `elslib_cylinder_d2`, `elslib_cylinder_d3`, `elslib_cylinder_dn`, `elslib_cylinder_object`, `elslib_cylinder_parameters`, `elslib_cylinder_uiso`, `elslib_cylinder_value`, `elslib_cylinder_viso`, `elslib_pln_d1`, `elslib_pln_dn`, `elslib_pln_object`, `elslib_pln_parameters`, `elslib_pln_uiso`, `elslib_pln_value`, `elslib_pln_viso`, `elslib_sphere_d1`, `elslib_sphere_d2`, `elslib_sphere_d3`, `elslib_sphere_dn`, `elslib_sphere_object`, `elslib_sphere_parameters`, `elslib_sphere_uiso`, `elslib_sphere_value`, `elslib_sphere_viso`, `elslib_torus_d1`, `elslib_torus_d2`, `elslib_torus_d3`, `elslib_torus_dn`, `elslib_torus_object`, `elslib_torus_parameters`, `elslib_torus_uiso`, `elslib_torus_value`, `elslib_torus_viso`.
* Coverage of the headers: `ElSLib.hxx` (every `Value`, `DN`, `D0`, `D1`, `D2`, `D3`, `Parameters` overload over the five `gp`
  surfaces, every `*Value`, `*DN`, `*D0`, `*D1`, `*D2`, `*D3`, `*Parameters`, `*UIso`, `*VIso` of the `Ax3` level; the `gp` object
  families check all object overloads on one line); `ElCLib.hxx` (3D and 2D `Value`, `D1`, `D2`, `D3`, `DN`, `Parameter` for the
  five curves in the object form and in the position-based form, and the ten `To3d` overloads). `InPeriod` and `AdjustPeriodic`
  are covered by the existing `tests/elclib.rs`. Excluded: the anonymous-namespace `normalizeAngle` (seen through the parameters).
