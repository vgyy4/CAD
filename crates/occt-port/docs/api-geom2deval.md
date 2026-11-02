# `geom2deval`: Rust API contract

Module `occt_port::geom2deval` translates the classes of OCCT 8.0.1's `Geom2dEval` that the `geom2d` contract left out (`Geom2dEval_RepCurveDesc`/`RepUtils`
are in `geom2d`): `ArchimedeanSpiralCurve`, `CircleInvoluteCurve`, `LogarithmicSpiralCurve`, `SineWaveCurve`, `AhtBezierCurve`, `TBezierCurve` (Rust names
without the `Geom2dEval_` prefix and with `Aht` for `AHT`). Each implements `geom2d::{Geometry2d, Curve2d}`; the two Bezier curves also `BoundedCurve2d`.

## Changes to `geom2d` (additive)
`Geom2dError::NotImplemented` (`Standard_NotImplemented`); provided trait methods `Geometry2d::try_transform`, `Curve2d::try_reverse` and
`Curve2d::try_reversed_parameter`, which default to the infallible methods and are overridden by these curves to return `Err(NotImplemented)`. Their
`transform`/`reverse`/`reversed_parameter` cannot report an error (the traits return `()`/`f64`): they do nothing / return `u` and are documented as such.

## Curves
| struct | constructor (`Err(Construction)` when) | parameter range |
|---|---|---|
| `ArchimedeanSpiralCurve::new(Ax2d, initial_radius, growth_rate)` | `initial_radius < 0`, `growth_rate <= 0` | `[0, Precision::Infinite]` |
| `CircleInvoluteCurve::new(Ax2d, radius)` | `radius <= 0` | `[0, inf]` |
| `LogarithmicSpiralCurve::new(Ax2d, scale, growth_exponent)` | `scale <= 0`, `growth_exponent <= 0` | `[-inf, inf]` |
| `SineWaveCurve::new(Ax2d, amplitude, omega, phase)` | `amplitude <= 0`, `omega <= 0` | `[-inf, inf]` |
| `AhtBezierCurve::new(&[Pnt2d], alg_degree, alpha, beta)` / `with_weights(poles, weights, ..)` | negative degree/alpha/beta, pole count != `alg_degree + 1 + 2 [alpha > 0] + 2 [beta > 0]`, weights count != poles or a weight <= 0 | `[0, 1]` |
| `TBezierCurve::new(&[Pnt2d], alpha)` / `with_weights(poles, weights, alpha)` | pole count even or < 3, `alpha <= 0`, weights as above | `[0, pi / alpha]` |

Accessors: `position()`, `initial_radius()`, `growth_rate()`, `radius()`, `scale()`, `growth_exponent()`, `amplitude()`, `omega()`, `phase()`; Bezier curves `poles() -> &[Pnt2d]`,
`weights() -> &[f64]` (empty when not rational), `alpha()`, `beta()`, `alg_degree()`, `order()`, `nb_poles()`, `is_rational()`. `Curve2d`: `is_periodic = false`, `continuity = Cn`,
`is_cn = true`, `is_closed` false for the unbounded curves and `|start - end| <= Precision::Confusion()` for the Bezier curves; `eval_dn(n < 1)` is `Err(Range)`.

## Behaviour notes
- The formulas use `gp_XY` arithmetic (`O + r cos(t) X + r sin(t) Y`, ...) with one operation per component, reproduced with a private `Xy`; `sin`, `cos`, `exp`, `sinh`, `cosh` are `libm`.
- `eval_dn` for the circle involute, logarithmic spiral, AHT- and T-Bezier curves delegates to `eval_d1..3` for `n <= 3`; the T-Bezier rational case uses the recurrence for `n > 3`
  (basis derivatives `evalBasis`/`evalBasisDeriv`), the AHT rational case a Leibniz recurrence.

## Judge
`oracle/geom2deval.cpp`: 474 lines, 24 families (`archimedean`, `involute`, `logarithmic`, `sine_wave`, `aht_bezier`, `t_bezier`, each with `_ctor`, `_eval`, `_props`, `_unsupported`).
Values go through `sin`/`cos`/`exp`/`sinh`/`cosh`: compared to 8 ulps of the largest value on the line (1735 of 1766 values are bit-identical). `_unsupported` checks that
reversal, reversed parameter and transformation throw.
