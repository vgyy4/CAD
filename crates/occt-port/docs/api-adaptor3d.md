# `adaptor3d`: Rust API contract

Module `occt_port::adaptor3d` translates the package `Adaptor3d` of OCCT 8.0.1: `Adaptor3d_Curve`, `Adaptor3d_Surface` (abstract), `Adaptor3d_IsoCurve`,
`Adaptor3d_CurveOnSurface`, `Adaptor3d_TopolTool`, `Adaptor3d_HVertex`, `Adaptor3d_HSurfaceTool`, `Adaptor3d_InterFunc`. It reuses `adaptor2d`
(`Curve2dAdaptor`, `Line2d`, `AdaptorError`, `GeomAbsCurveType`), `geom` (curves and surfaces, `ResD*`, `SurfResD*`), `elclib`, `elslib`, `math1d::FunctionRoots`
and `topabs` (the enumerations of `TopAbs`, translated as a prerequisite of this job).

## Changes to existing modules (all additive)
- `adaptor2d::AdaptorError` gained the variant `Geom3d(geom::GeomError)` (with `From`), because the 3D adaptors hand out `geom` curves and surfaces.
- The prerequisite package `topabs` is a separate job (`TopAbs_Orientation`, `TopAbs_State`, `TopAbs_ShapeEnum`, `TopAbs::*`).

## API
```rust
pub enum GeomAbsSurfaceType { Plane, Cylinder, Cone, Sphere, Torus, BezierSurface, BSplineSurface, SurfaceOfRevolution, SurfaceOfExtrusion, OffsetSurface, OtherSurface }
pub enum GeomAbsIsoType { IsoU, IsoV, NoneIso }                              // discriminants are the C++ values
pub trait Curve3dAdaptor: Debug + Send + Sync { ... }                        // Adaptor3d_Curve: every method defaults to Err(NotImplemented)
pub trait SurfaceAdaptor: Debug + Send + Sync { ... }                        // Adaptor3d_Surface
IsoCurve::new() / with_surface(S) / with_iso(S, iso, p)? / with_iso_range(S, iso, p, wf, wl)?   // the four constructors
IsoCurve::load_surface(S) / load_iso(iso, p)? / load_iso_range(iso, p, wf, wl)?                 // Load; iso(), parameter(), surface()
IsoCurve::shallow_copied() / trimmed(first, last, tol)                       // ShallowCopy / Trim with the concrete type (the traits return Arc<dyn Curve3dAdaptor>)
CurveOnSurface::new() / with_surface(S) / with_curve_surface(C, S)? ; load_surface(S)? / load_curve(C)? / load(C, S)? ; get_curve() / get_surface() / shallow_copied() / trimmed(..)
TopolTool::new() / with_surface(S)? ; initialize() (NotImplemented) / initialize_surface(S) / initialize_curve(C) ; init, more, value, next ; init_vertex_iterator, more_vertex, vertex, next_vertex ;
    classify, is_the_point_on, orientation_curve, orientation_vertex, identical, has_3d, tol_3d_curve, tol_3d_vertex, pnt ; compute_sample_points, nb_samples_u/v, nb_samples, u_parameters, v_parameters,
    sample_point, domain_is_infinite, edge, sample_pnts, bspl_sample_pnts, is_uniform_sampling, TopolTool::get_cone_apex_param(&Cone)
HVertex::new(p, orientation, resolution), value, parameter(C), resolution(C), orientation, is_same
adaptor3d::surface_tool::*(&Arc<dyn SurfaceAdaptor>, ..)                      // Adaptor3d_HSurfaceTool (all the static methods; is_surf_g1, nb_samples_u/v and the ranged variants)
InterFunc::new(C, fix_val, fix)?  (math1d::Function + FunctionWithDerivative)  // Adaptor3d_InterFunc
```
Handles are `Arc<dyn ...>`; a null handle returned by `Bezier`, `BSpline`, `OffsetCurve`, `BasisCurve`, `BasisSurface` is `None`; using it is `Err(NullObject)`. `Intervals`/`UIntervals`/`VIntervals` take `&mut [f64]`
(`t[0]` is `T(T.Lower())`; a write past the end is `Err(OutOfRange)`). `D0..DN` of the traits are the provided wrappers of `eval_d0..eval_dn`.

## Behaviour notes
- The library is built without `Raise_if` checks: `Standard_NoSuchObject_Raise_if`, `Standard_OutOfRange_Raise_if`... conditions are not even evaluated (so `CurveOnSurface::Bezier()` does not call `GetType` of the
  surface for its check), and `NCollection_Array1` has no range checks. The explicit `throw` statements are reproduced (`NoSuchObject` for a `NoneIso` iso curve, `NotImplemented` for `CurveOnSurface::EvalDN` above order 3, `DomainError`...).
- `IsoCurve::NbIntervals`/`Intervals` walk the intervals of the surface with unchecked loops; the judge keeps the iso range inside the intervals (as OCCT callers do).
- `CurveOnSurface::NbIntervals` caches its interval sequence (`myIntervals`, `myIntCont`, mutable in C++: a `Mutex` here) and uses `math_FunctionRoots` on `Adaptor3d_InterFunc` for every interval break of the surface.
- `EvalFirstLastSurf`/`LocatePart*`: for B-spline, revolution, extrusion and offset surfaces the evaluation at the first / last parameter of the curve uses the surface trimmed (`UTrim`/`VTrim`) to the span the curve starts / ends in
  (`myFirstSurf`, `myLastSurf`).
- `TopolTool`: `TopolTool(S)` leaves some members uninitialised in C++ (`myNbSamplesV`, `nbVtx`, `idVtx`); here they are initialised (-1 and 0), and the judge only reads members that C++ sets. `BSplSamplePnts` of C++ can leave
  entries of `myUPars`/`myVPars` uninitialised when it keeps fewer flagged samples than it announces (a surface with fewer than 4 samples in one direction); the judge uses surfaces and sample minima that avoid this (valgrind-clean).
- `HSurfaceTool::IsSurfG1` makes a `GeomAdaptor_Curve` of the basis curve of an offset curve (`GeomAdaptor` is a later package): only its type and its B-spline are used, which are derived from the `geom` curve here
  (a B-spline curve gives itself; any other type gives no B-spline, so the answer is `false`).

## Judge
`oracle/adaptor3d.cpp` (1941 lines, 38 families: `def_curve3d, def_surface, def_curve2d`; `iso_new, iso_params, iso_nb_intervals, iso_intervals, iso_eval, iso_type, iso_info, iso_line, iso_circle, iso_conic, iso_geom, iso_copy`;
`cos_new, cos_params, cos_eval, cos_info, cos_nb_intervals, cos_intervals, cos_copy`; `topol_init0, topol_restr, topol_vertices, topol_classify, topol_on, topol_misc, topol_samples, topol_samplepnts, topol_bspl_samplepnts, topol_cone_apex`;
`hst_forward, hst_g1, hst_nb_samples_u, hst_nb_samples_v`; `hvertex, interfunc`). The adaptors the classes work on are a closed set of test adaptors defined by the first records (12 curves, 32 surfaces of every type -- planes with
infinite ranges, analytic surfaces, Bezier, B-spline, revolution, extrusion, offset --, 31 2D curves) which answer from printed data and fold every call (method id and argument bits) into a counter and a 64-bit hash that is
printed after each record, so the sequence of calls each class makes is compared as well as its results. Line coverage of the C++ sources by the oracle is 90 % (IsoCurve), 91 % (CurveOnSurface), 92 % (TopolTool), 98 % (HSurfaceTool).
Values are bit-identical except the families whose results come through the trigonometric functions of the libm (8 ulps of the largest value of the line).
