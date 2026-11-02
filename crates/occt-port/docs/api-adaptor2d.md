# `adaptor2d`: Rust API contract

Module `occt_port::adaptor2d` translates the package `Adaptor2d` of OCCT 8.0.1 (`Adaptor2d_Curve2d`, `Adaptor2d_Line2d`,
`Adaptor2d_OffsetCurve`). It reuses `gp` (2D value types), `elclib::{line2d_value, line2d_d1}`, `geom2d::{BezierCurve,
BSplineCurve, ResD1..3}` and `plib::GeomAbsShape` (re-exported). Two existing items changed visibility only:
`geom2d::offset` is `pub(crate)` and its `calculate_d0..d3` (`Geom2d_OffsetCurveUtils`) are `pub(crate)`.

## Types
- `GeomAbsCurveType {Line=0, Circle, Ellipse, Hyperbola, Parabola, BezierCurve, BSplineCurve, OffsetCurve, OtherCurve}` (C++ order, `as i32`).
- `AdaptorError {NotImplemented, NoSuchObject, OutOfRange, TypeMismatch, Construction, NullObject}` (`From<gp::ConstructionError>`);
  `Result<T>`. `NullObject` is a method of an `OffsetCurve` without a curve (a null handle in C++).
- `trait Curve2dAdaptor: Debug + Send + Sync` is `Adaptor2d_Curve2d`; adaptor handles are `Arc<dyn Curve2dAdaptor>`. Every method is `&self`
  and returns `Result`. Defaults are the base-class bodies: all `Err(NotImplemented)` except `nb_samples() == Ok(20)` and the wrappers
  `eval_d0..eval_dn` that forward to `d0..dn`. Out-parameters are returned: `d1 -> (Pnt2d, Vec2d)`, `d2 -> (p, v1, v2)`, `d3 -> (p, v1, v2, v3)`;
  `eval_d1/2/3 -> geom2d::ResD1/2/3`. `intervals(&self, t: &mut [f64], s)` writes `t[0] = T(T.Lower())...`; a slice that is too short is
  `Err(OutOfRange)` (undefined behaviour in C++, not judged). Methods: `shallow_copy, first_parameter, last_parameter, continuity, nb_intervals,
  intervals, trim, is_closed, is_periodic, period, value, d0, d1, d2, d3, dn, resolution, get_type, line, circle, ellipse, hyperbola,
  parabola, degree, is_rational, nb_poles, nb_knots, nb_samples, bezier (Arc<BezierCurve>), bspline (Arc<BSplineCurve>)`.
- `Line2d` (`Adaptor2d_Line2d`): `new()` (parameters 0, 0), `with_point_dir(p, d, u_first, u_last)`, `load(&Lin2d)` (parameters `+-Precision::Infinite()`),
  `load_range(&Lin2d, f, l)`. Period and all conic/curve accessors are `NoSuchObject`; `dn(n <= 0)` is `OutOfRange`.
- `OffsetCurve` (`Adaptor2d_OffsetCurve`): `new()`, `with_curve(c)` (range 0, 0), `with_offset(c, off) -> Result` (range of the curve), `with_range(c, off, wf, wl)`,
  `load_curve(c)` (offset reset to 0), `load_offset(off) -> Result` (range of the curve), `load_range(off, wf, wl)`, `curve()`, `offset()`, plus the concrete-typed
  `shallow_copied() -> Result<OffsetCurve>` and `trimmed(first, last) -> OffsetCurve` behind the trait's `shallow_copy`/`trim`. `dn` is `NotImplemented`.

## Behaviour notes
- The OCCT build has the `Raise_if` checks compiled out (the condition is not evaluated): `OffsetCurve::bezier/bspline` forward to the adapted curve whatever
  the offset and type (C++ source: `Standard_NoSuchObject_Raise_if(...)`). Explicit `throw`s are kept (`continuity` of a C0 curve is `TypeMismatch`).
- Library-side inline `gp` checks are also off, so a zero tangent in `OffsetCurve::line` gives a NaN direction in C++; Rust returns `Err(Construction)`. The oracle keeps the tangent nonzero.
- `OffsetCurve::nb_intervals/intervals` read `T(first)`/`T(last)` of the adapted curve's breaks until they pass the window: the window must lie inside the breaks
  (else out-of-range reads, `Err(OutOfRange)` here, undefined in C++; not judged).
- `OffsetCurve::resolution(r) = 1e-7 / r` (`Precision::PConfusion(R)`); `nb_samples` follows `nbPoints` (20, Bezier `3 + NbPoles`, BSpline `NbKnots * Degree`, at most 300; `get_type` is called twice for a non-Bezier curve).

## Judge
`oracle/adaptor2d.cpp` (609 lines, 24 families) drives the real library with a closed set of test adaptors "TC": a polynomial curve with printed type, continuity, breaks,
closed/periodic flags, conic data, Bezier data. Every call TC receives is counted and hashed (method id and argument bits), so the call sequence of the offset adaptor is compared.
Deterministic twice, under `SMASH`/`MALLOC_PERTURB_` and ASAN/UBSAN. Values: all bit-identical.
