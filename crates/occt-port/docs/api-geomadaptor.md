# `geomadaptor`: Rust API contract

Module `occt_port::geomadaptor` translates the package `GeomAdaptor` of OCCT 8.0.1: `GeomAdaptor_Curve`, `GeomAdaptor_Surface`, `GeomAdaptor_TransformedCurve`,
`GeomAdaptor_TransformedSurface`, `GeomAdaptor_SurfaceOfRevolution`, `GeomAdaptor_SurfaceOfLinearExtrusion` and the static `GeomAdaptor::MakeCurve` / `MakeSurface`.
It implements the traits `adaptor3d::Curve3dAdaptor` and `adaptor3d::SurfaceAdaptor` and uses `geom` (the Geom curves and surfaces with their evaluation representations),
`elclib`, `elslib`, `bsplclib_cache::Cache` (curves), `bsplslib::cache::Cache` (surfaces), `bsplclib` (`Intervals`, `FlatBezierKnots`), `adaptor3d::CurveOnSurface`, `gp::Trsf`.

## Changes to existing modules (all additive)
- `geom::offsetsurf`: the functions of `Geom_OffsetSurfaceUtils` (`evaluate_d0 .. evaluate_dn` and their helpers) were written for a `&dyn Surface` basis; they are now generic
  over a small trait `OffsetBasis` (`eval_d1/2/3/n`, `bounds`, `is_u_periodic`, `is_v_periodic`), implemented for every `geom::Surface` (so `geom::OffsetSurface` is unchanged) and
  for `geomadaptor::Surface` (the C++ templates are instantiated with `GeomAdaptor_Surface`). They are `pub(crate)`.
- `geom::offset`: `calculate_d0 .. calculate_d3` of `Geom_OffsetCurveUtils` are `pub(crate)` (the adaptor drives them with its own basis adaptor).
- `adaptor3d::hsurface_tool`: unchanged (its stand-in for `GeomAdaptor_Curve` in `IsSurfG1` could now use `geomadaptor::Curve`).

## API
```rust
pub struct Curve;                                  // GeomAdaptor_Curve (implements Curve3dAdaptor)
Curve::new() / with_curve(c: Arc<dyn geom::Curve>)? / with_range(c, first, last)?
Curve::with_curve_opt(Option<..>)? / load_opt(Option<..>)? / load_range_opt(Option<..>, first, last)?   // a null handle is Err(NullObject)
Curve::load(c)? / load_range(c, first, last)? / reset() / curve() -> Option<&Arc<dyn geom::Curve>>
Curve::shallow_copied() -> Result<Curve> / trimmed(first, last, tol) -> Result<Curve>      // the concrete-type versions of ShallowCopy / Trim

pub struct Surface;                                // GeomAdaptor_Surface (implements SurfaceAdaptor)
Surface::new() / with_surface(s)? / with_range(s, u1, u2, v1, v2, tol_u, tol_v)?  (+ the `_opt` variants as above)
Surface::load(s)? / load_range(s, u1, u2, v1, v2, tol_u, tol_v)? / surface() / tolerance_u() / tolerance_v() / bounds()
Surface::shallow_copied() -> Result<Surface> / u_trimmed(first, last, tol) / v_trimmed(first, last, tol)

pub struct TransformedCurve;                       // GeomAdaptor_TransformedCurve
TransformedCurve::new() / with_curve(c, trsf) / with_range(c, first, last, trsf) / load(c) / load_range(c, first, last)
    load_curve_on_surface(Arc<adaptor3d::CurveOnSurface>) / set_trsf / trsf() / is_3d_curve() / is_curve_on_surface()
    curve() -> &Curve / change_curve() -> &mut Curve / curve_on_surface() -> Option<&Arc<CurveOnSurface>> / geom_curve()

pub struct TransformedSurface;                     // GeomAdaptor_TransformedSurface
TransformedSurface::new() / with_surface(s, trsf) / with_range(s, u1, u2, v1, v2, trsf, tol_u, tol_v)
    load(s, trsf) / load_range(.., trsf, tol_u, tol_v) / set_trsf / has_trsf() / trsf() / tolerance_u() / tolerance_v()
    adaptor_surface_original() -> &Surface / adaptor_surface_transformed() -> Result<Arc<Surface>>
    geom_surface_original() -> Option<&Arc<dyn geom::Surface>> / geom_surface_transformed() -> Result<Option<Arc<dyn geom::Surface>>>

pub struct SurfaceOfRevolution;                    // GeomAdaptor_SurfaceOfRevolution (a GeomAdaptor_Surface in C++)
SurfaceOfRevolution::new() / with_curve(c: Arc<dyn Curve3dAdaptor>)? / with_axis(c, axis: Ax1)?
    load_curve(c)? / load_axis(axis)? (Err(Construction) "Axe and meridian are confused") / axis() -> &Ax3

pub struct SurfaceOfLinearExtrusion;               // GeomAdaptor_SurfaceOfLinearExtrusion
SurfaceOfLinearExtrusion::new() / with_curve(c)? / with_direction(c, dir: Dir)? / load_curve(c)? / load_direction(dir)?

pub fn make_curve(c: &dyn Curve3dAdaptor) -> Result<Arc<dyn geom::Curve>>               // GeomAdaptor::MakeCurve
pub fn make_surface(s: &dyn SurfaceAdaptor, trim: bool) -> Result<Arc<dyn geom::Surface>> // GeomAdaptor::MakeSurface
```
Errors are `adaptor2d::AdaptorError`. Handles are `Arc<dyn ...>`. The caches of the Bezier and B-spline curves and surfaces are behind a `Mutex`.

## Behaviour notes
- The library is built without `Raise_if` checks (`Standard_NoSuchObject_Raise_if` and so on are not evaluated) but the explicit `throw` statements stay. The judge records what the
  library does: for example `Surface::u_period()` of a non-periodic surface answers `Geom_Surface::UPeriod()`, which is the difference of the bounds, and does not raise.
- `Load(curve)` of a `Geom_TrimmedCurve` loads its basis curve with the range of the trimmed curve, and `Curve()` then answers the basis curve (as in C++, `myCurve` is overwritten
  by the nested `Load`); likewise for `Geom_RectangularTrimmedSurface`.
- A curve or surface that carries an evaluation representation (`geom::EvalRepDesc` / `SurfaceEvalRepDesc`) is evaluated through its own `eval_*` methods, as `hasEvalRep` does.
- `GeomAbs_OtherCurve` / `OtherSurface` (a Geom class with no adaptor type) cannot happen with the classes of the `geom` module; `GeomEval` curves and surfaces do not exist yet.
- The cache of `Curve` / `Surface` is rebuilt on a miss. Evaluating in another order of derivatives on a fresh adaptor builds it on the first call, whichever it is.
- `GeomAdaptor_Surface::EvalD1` and following evaluate at `(u, v)` moved onto the first / last parameter when `|U - First| <= TolU` (`USide`), as in C++; the BSpline paths use `LocalD*` then.
- `SurfaceOfRevolution::load_axis` evaluates the type of the surface on a half-initialised object (`myAxeRev` is the identity `gp_Ax3` at that point), exactly as C++ does.
- `Surface`, `SurfaceOfRevolution` and `SurfaceOfLinearExtrusion` share the evaluation code of `GeomAdaptor_Surface`: the latter two hold a `Surface` base whose `mySurfaceType` / `mySurfaceData` are the revolution / extrusion data and whose range
  fields stay 0 (so the tolerance snapping of `EvalD1..DN` compares with 0, as C++ does).

## Library behaviours the judge records
- The OCCT libraries are built without the `Raise_if` checks, including those of inlined `gp` constructors: `Cylinder`/`Cone`/`Sphere`/`Torus` of the revolution and extrusion adaptors are built unchecked (`gp::*::new_unchecked`), and `Plane()` of an extrusion builds its frame with `Dir::from_vec_unchecked` / `Ax3::new_unchecked` (NaN directions, no error). These `pub(crate)` constructors are additions to `gp`.
- `BSplCLib::Intervals` replaces the array it is given by one of exactly `nb + 1` entries, so the oracle's later read of 8 entries throws when `nb + 1 < 8`. `Curve::intervals_replace_array(shape)` and `Surface::intervals_replace_array(v, shape)` report that size (`Some(nb+1)`) so the test can reproduce it; the trait method itself writes into the slice.
- `ULP_FAMILIES` (8 ulps of the line maximum): the eval, resolution and revolution/extrusion info families, whose values come through libm trig.

## Judge
`oracle/geomadaptor.cpp` (1973 lines of records, 33 families). The geometry is a closed set printed by the first records: 25 Geom curves (`def_gcurve`: line, circle, ellipse, hyperbola,
parabola, Bezier, rational Bezier, B-splines with all multiplicities and a periodic and a rational one, offset and trimmed curves, curves with evaluation representations), 33 Geom
surfaces (`def_gsurface`: the five elementary surfaces, Bezier and B-spline surfaces, revolution, extrusion, offset, rectangularly trimmed surfaces, evaluation representations), 3 2D curves
(`def_gcurve2d`, for the curves on surfaces). Families: `gac_load, gac_load_bad, gac_eval, gac_intervals, gac_info, gac_resolution, gac_trim` (GeomAdaptor_Curve), `gas_*` (the same for
GeomAdaptor_Surface), `gtc_eval, gtc_info, gtc_copy` and `gts_eval, gts_info, gts_trim` (the transformed adaptors, with seven transformations and five curves on surfaces),
`gsr_new, gsr_eval, gsr_info, gsr_trim` and `gse_new, gse_eval, gse_info, gse_trim` (revolution and extrusion adaptors over `GeomAdaptor_Curve` with computed axes: cylinder, plane, cone,
sphere, torus, ...), `make_curve`, `make_surface`. A call that may raise is recorded as `1 <results>` or `0`. Line coverage of the C++ sources by the oracle: 97 % (`GeomAdaptor`), 94.5 % (`Curve`),
93 % (`Surface`), 96 % / 98 % (transformed curve / surface), 98 % / 97 % (revolution / extrusion); the rest are the `GeomAbs_Other*` branches, which need classes of `GeomEval`.
Values are compared bit for bit, except the families whose results come through the trigonometric functions of the libm (8 ulps of the largest value of the line).
