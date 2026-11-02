# `geom2dgrideval`: Rust API contract

Module `occt_port::geom2dgrideval` translates the package `Geom2dGridEval` of OCCT 8.0.1: the grid evaluators of the 2D curves
(`Geom2dGridEval_Line/Circle/Ellipse/Hyperbola/Parabola/BezierCurve/BSplineCurve/OffsetCurve/OtherCurve` and the dispatcher
`Geom2dGridEval_Curve`). It reuses `geom2d` (curves, `OffsetCurve` utilities), `adaptor2d` (trait `Curve2dAdaptor`, `AdaptorError`),
`geom2dadaptor` (a `Geom2dAdaptor_Curve` is recognised by the dispatcher), `bsplclib::curve2d` (`d0..d3`, `dn`), `bsplclib::flat::locate_parameter_flat`
and `bsplclib_cache::Cache`.

## Changes to existing modules (all additive or visibility)
- `adaptor2d`: new provided method `Curve2dAdaptor::is_geom2d_adaptor() -> bool` (default `false`): `Geom2dGridEval_Curve` tests `IsKind(STANDARD_TYPE(Geom2dAdaptor_Curve))`, which
  also holds for an adaptor that was never loaded (then the evaluator is empty). `geom2dadaptor::Curve` returns `true`.
- `geom2d::offset`: `adjust_derivative` and `Derivatives` (`Geom2d_OffsetCurveUtils::AdjustDerivative`, taking a `Geom2d_Curve`) are `pub(crate)` with `pub(crate)` fields.

## API
```rust
pub struct CurveD1 { pub point: Pnt2d, pub d1: Vec2d }                      // Geom2dGridEval::CurveD1 / CurveD2 / CurveD3
pub struct CurveD2 { pub point, pub d1, pub d2 }   pub struct CurveD3 { pub point, pub d1, pub d2, pub d3 }
pub trait GridEvaluator {                                                   // the five Evaluate* methods every evaluator has
    fn evaluate_grid(&self, params: &[f64]) -> Result<Vec<Pnt2d>>;           // EvaluateGrid
    fn evaluate_grid_d1(&self, params: &[f64]) -> Result<Vec<CurveD1>>;     // EvaluateGridD1 / D2 / D3
    fn evaluate_grid_d2(..) / evaluate_grid_d3(..);
    fn evaluate_grid_dn(&self, params: &[f64], n: i32) -> Result<Vec<Vec2d>>;
}
Line::new(Option<&geom2d::Line>), Circle::new(Option<&geom2d::Circle>), Ellipse::new, Hyperbola::new, Parabola::new,
BezierCurve::new(Option<&geom2d::BezierCurve>), BSplineCurve::new(Option<&geom2d::BSplineCurve>), OffsetCurve::new(Option<&geom2d::OffsetCurve>)
OtherCurve::new(&'a dyn Curve2dAdaptor)
Curve::from_curve(Option<Arc<dyn Curve2d>>) -> Curve<'static>              // Geom2dGridEval_Curve(handle<Geom2d_Curve>)
Curve::from_adaptor(&'a dyn Curve2dAdaptor) -> Result<Curve<'a>>            // Geom2dGridEval_Curve(Adaptor2d_Curve2d&)
Curve::curve_type() -> GeomAbsCurveType                                     // GetType
```
`Result` is `adaptor2d::Result` (`AdaptorError`). An empty parameter slice is `NCollection_Array1::IsEmpty` (any lower bound is the same slice); the result arrays are 1-based `Vec`s.
A null handle is `None`. The evaluators of the elementary curves, `BezierCurve` and `BSplineCurve` hold a copy of the geometry (the Rust curves are immutable behind an `Arc`, so a handle copy has
no observable difference); `Geometry()` is not translated.

## Behaviour notes
- Every evaluator returns an empty vector for a null geometry, no parameters and (`evaluate_grid_dn`) `n < 1`. The dispatcher's evaluator is empty for a curve of an unknown type (for example a
  `Geom2dEval` curve), a null handle, an adaptor that is a `Geom2dAdaptor_Curve` without a curve, and its type is then `OtherCurve`.
- The dispatcher unwraps any chain of `TrimmedCurve`s (the trimming is ignored: the grid is evaluated on the basis curve), a `Geom2dAdaptor_Curve` is replaced by its curve, any other adaptor goes to `OtherCurve`
  (calls `eval_d0..eval_dn` of the adaptor; `evaluate_grid_dn` for 1..3 reuses the D1..D3 grids).
- Elementary curves evaluate the formulas of the C++ in the same operand order (`libm` for `sin cos sinh cosh`); the parabola's `Y` direction is `Dir2d::new(-x, y)` (normalising, as `gp_Dir2d`).
  The circle and ellipse derivatives of order `n` cycle with period 4, the hyperbola's with period 2, the parabola's are zero from the third.
- `BezierCurve` builds a `Cache` at 0.5 (`BuildCache(0.5, ...)`) and evaluates `Cache::d0_2d..d3_2d` at the parameters (any parameter, also outside [0, 1]); derivatives of order `n > degree` are zero, else `bsplclib::curve2d::dn` with span 0.
- `BSplineCurve` locates the span in the flat knots for every parameter and uses the `Cache` of the span when 4 or more consecutive parameters share it (local parameter `(u - start) / length`), otherwise
  `bsplclib::curve2d::d0..d3` with the span; derivatives of order `n > degree` are zero, else `bsplclib::curve2d::dn` with **the span of the flat knots together with the knots and multiplicities** (as C++:
  the index is kept when it lies in the knot range, so the polynomial piece can be the wrong one). In C++ this reads past the multiplicities when that span index equals the number of knots (undefined
  behaviour); the Rust function returns `Err(OutOfRange)`, and the judge leaves such parameters out of derivative grids of non-periodic B-splines.
- `OffsetCurve` evaluates the basis grid through the dispatcher (`D0` from the basis `D1`, `D1` from `D2`, `D2` and `D3` from `D3`; `D3` also calls `EvalDN(4)` of the basis) and `Geom2d_OffsetCurveUtils::Calculate*`; when the
  basis derivative vanishes `AdjustDerivative` replaces the derivatives. A basis grid that is empty or a `Calculate*` that fails makes the whole result empty. `evaluate_grid_dn`: 1..3 reuse the grids (unlike
  `Geom2dAdaptor_Curve`, no aliasing of variables), above 3 it is the basis grid DN.
- The library is built without `Raise_if` checks: nothing else is checked, and an out-of-range input that C++ would read through (undefined behaviour) is `Err` in Rust and kept out of the judge.

## Judge
`oracle/geom2dgrideval.cpp` (944 lines, 17 families: `line_grid, circle_grid, ellipse_grid, hyperbola_grid, parabola_grid, bezier_grid, bspline_grid, offset_grid, other_grid, curve_geom, curve_adaptor, curve_other,
curve_type, dn_sweep, bspline_cache, bezier_outside, offset_singular`) builds the curves of the real library from printed data (lines, conics, Bezier, polynomial and rational B-splines, periodic ones, offset and trimmed curves,
nested ones, `Adaptor2d_Line2d`, `Adaptor2d_OffsetCurve`, `Geom2dAdaptor_Curve`) and compares every grid method on sorted, dense, unsorted, clustered and empty parameter arrays. Bit-identical comparisons
except the families that go through `sin`/`cos`/`sinh`/`cosh` (8 ulps of the largest value on the line).
