# `geom2dadaptor`: Rust API contract

Module `occt_port::geom2dadaptor` translates the package `Geom2dAdaptor` of OCCT 8.0.1 (`Geom2dAdaptor_Curve`, `Geom2dAdaptor::MakeCurve`).
It reuses `adaptor2d` (trait `Curve2dAdaptor`), `geom2d` (curves, `ResD1..3`, `OffsetCurve` utilities), `elclib` (2D `line2d_*`, `circle2d_*`,
...), `bsplclib_cache` (`Cache`, `intervals`, `flat_bezier_knots`) and `bsplclib::flat::locate_core`.

## Changes to existing modules (all additive or visibility)
- `adaptor2d` (translated earlier in this queue, not yet reviewed): `Curve2dAdaptor::intervals(&self, t: &mut Vec<f64>, s)` (the B-spline adaptors `Resize` the C++ array,
  so the vector is replaced); `bezier()`/`bspline()` return `Result<Option<Arc<..>>>` (`None` is the null handle); new provided method `geom2d_curve() -> Option<Arc<dyn Curve2d>>`
  (the `dynamic_cast<const Geom2dAdaptor_Curve*>` of `MakeCurve`); `AdaptorError` gained `Domain`, `UndefinedValue`, `UndefinedDerivative`, `Geom(Geom2dError)` and is no longer `Copy`.
- `geom2d` (reviewed): `mod offset` is `pub(crate)` with `pub(crate)` `calculate_d0..d3` (already so since the `adaptor2d` job); no other change.

## API
```rust
pub struct Curve;                                    // Geom2dAdaptor_Curve; implements Curve2dAdaptor
Curve::new() / with_curve(c: Arc<dyn Curve2d>) / with_range(c, u_first, u_last)   // the constructors (the last two load)
Curve::load(c) / load_range(c, u_first, u_last)      // Load; load_range: ConstructionError if u_first > u_last + Precision::Confusion()
Curve::reset(), is_initialized(), curve() -> Option<&Arc<dyn Curve2d>>
pub fn make_curve(hc: &dyn Curve2dAdaptor) -> Result<Arc<dyn Curve2d>>   // Geom2dAdaptor::MakeCurve; OtherCurve / foreign offset adaptors: Err(Domain)
```
`load` of a `TrimmedCurve` loads its basis curve with the given range, as C++. `Load(null)` does not exist (an `Arc` cannot be null): the exception is `NullObject` for
calls on an adaptor without a curve. Types are found by `as_any().downcast_ref` (Line, Circle, Ellipse, Parabola, Hyperbola, BezierCurve, BSplineCurve, OffsetCurve; anything else is `OtherCurve`).
The Bezier/B-spline data holds a clone of the curve (`Arc<BezierCurve>`), because the Rust curves are immutable behind an `Arc` and a handle copy has no observable difference.

## Behaviour notes
- The elementary curves are evaluated by `ElCLib` (`Parabola::EvalD3` is `D2` plus a zero third derivative, a line's `D2`/`D3` are zero).
- Bezier and B-spline curves use `Cache` (rebuilt when missing or, for B-splines, not valid at `u`); at the first or last parameter of a B-spline the evaluation uses `LocalD*` of the span from `locate_u`. A curve with an
  evaluation representation forwards to the curve (`has_eval_rep`; not exercised by the judge: `Geom2dEval` is a separate package).
- Offset curves keep a nested `Curve` over the basis curve and evaluate with `Geom2d_OffsetCurveUtils::Evaluate*`, ported over the adaptor. `EvaluateDN(n = 3)` passes one variable as both
  the first and the second derivative, so its result is not the third derivative of the offset curve: reproduced (`offset_dn3`).
- `Resolution` of a circle uses `libm::asin`; Bezier/B-spline call `Resolution` on a copy of the curve.
- The library is built without `Raise_if` checks: the accessors `Line()`... are `std::get` of the data variant, which throws `std::bad_variant_access` for another type; the Rust result is `Err(NoSuchObject)`.
- A parameter range of the adaptor outside the parameter range of a B-spline reads outside the knots in C++ (unchecked); the judge keeps the evaluation ranges inside the curve.

## Judge
`oracle/geom2dadaptor.cpp` (912 lines, 14 families: `load, reset, continuity, nb_intervals, intervals, closed, nb_samples, resolution, info, conic, eval, eval_sweep, trim_copy, make_curve`) builds the
curves of the real library from printed data (lines, conics, Bezier, B-splines polynomial and rational, periodic ones, offset and trimmed curves) and compares every adaptor method. Values are bit-identical except
the families that go through `sin`/`cos`/`cosh`/`asin` (`eval`, `eval_sweep`, `resolution`, `trim_copy`, `make_curve`): 8 ulps of the largest value on the line (5376 of 5392 values are bit-identical).
