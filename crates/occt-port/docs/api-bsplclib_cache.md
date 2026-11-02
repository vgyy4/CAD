# `bsplclib_cache`: Rust API contract

A prerequisite of the curve adaptors (`Geom2dAdaptor`, `GeomAdaptor`, ...): the parts of OCCT 8.0.1's `BSplCLib` that the
`bsplclib` translation lacks. Module `occt_port::bsplclib_cache` (new) plus one additive function in each of
`bsplclib::curve2d` and `bsplclib::curve3d`; nothing existing changes. Arrays are slices; the C++ arrays all have lower bound 1.
Errors are `bsplclib::BSplError` (`OutOfRange` for an item C++ would read outside its array, undefined behaviour in the unchecked build).

## Functions
```rust
pub fn flat_bezier_knots(degree: i32) -> Result<Vec<f64>, BSplError>;   // BSplCLib::FlatBezierKnots: degree+1 zeros, degree+1 ones; 1..=25
pub fn intervals(knots: &[f64], mults: &[i32], degree: i32, periodic: bool, continuity: i32, first: f64, last: f64,
    tolerance: f64, want_intervals: bool) -> Result<(i32, Option<Vec<f64>>), BSplError>;  // BSplCLib::Intervals: (count, bounds)
// bsplclib::curve2d / bsplclib::curve3d: BSplCLib::BuildCache(U, SpanDomain, Periodic, Degree, SpanIndex, FlatKnots, Poles, Weights, CacheArray)
pub fn build_cache_span(u: f64, span_domain: f64, periodic: bool, degree: i32, span_index: i32, flat_knots: &[f64],
    poles: &[Pnt2d] /* or &[Pnt] */, weights: Option<&[f64]>) -> Result<Vec<f64>, BSplError>;  // (degree+1) rows, row-major
```
`build_cache_span` rows have `dim` (2 or 3) items, plus one when `weights` is given: the weight coefficient of a rational spline, or
(all weights equal) a column that is 0 except the 1 of the first row. `intervals` returns, with `want_intervals`, `count + 1` bounds, the first
being `first` and the last `last`; `Resize` of C++'s output array is the returned `Vec`.

## `CacheParams` (`BSplCLib_CacheParams`)
Public fields `degree, is_periodic, first_parameter, last_parameter, span_index_min, span_index_max, span_start, span_length, span_index`;
`CacheParams::new(degree, periodic, flat_knots)`, `periodic_normalization(u)`, `is_cache_valid(u)`, `locate_parameter(&mut self, &mut u, flat_knots)`.

## `Cache` (`BSplCLib_Cache`)
`Cache::new_2d(degree, periodic, flat_knots, weights: Option<&[f64]>)` / `new_3d` (weights given: rational, even when all are 1);
`build_cache_2d(u, flat_knots, &[Pnt2d], weights)` / `build_cache_3d`; `is_cache_valid(u)`, `params()`; evaluation `d0_2d(u)`, `d1_2d -> (p, v1)`,
`d2_2d -> (p, v1, v2)`, `d3_2d -> (p, v1, v2, v3)` and the `d*_3d` forms; `d0_local_2d(local)` ... `d3_local_3d(local)` for the span-local parameter
`(u - SpanStart) / SpanLength`. Evaluating before `build_cache_*` is undefined in C++ (an uninitialised buffer); here the buffer is zero.

## Judge
`oracle/bsplclib_cache.cpp`: 261 lines, 5 families (`flat_bezier_knots`, `intervals`, `cache_params`, `build_cache`, `cache`) over Bezier, non-periodic and
periodic splines of degree 1..4, 2D and 3D, polynomial, rational and unit-weight (all equal) weights. All values are bit-identical. No `throws` line: the
checks of the unchecked build are not reproduced and the oracle only gives valid input.
