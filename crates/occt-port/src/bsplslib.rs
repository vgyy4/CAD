//! OCCT's `BSplSLib` package: evaluation, surface modifications, caches (`BSplSLib_Cache`), `Resolution`,
//! `Interpolate`, `FunctionMultiply`, `UnitWeights` and `MovePoint` of B-spline surfaces.
//!
//! The functions follow the C++ closely (same operation order, bit-identical results). OCCT's arrays have lower
//! bound 1: `Array2<T>` has `rows()` = U (first index) and `cols()` = V (second index); knots and multiplicities
//! are slices with `knots[k - 1]` the OCCT `Knots(k)`; every index argument keeps its 1-based meaning. Checks that
//! OCCT 8.0.1 as built has compiled out, and that would have failed, are `Err(BSplError::OutOfRange)`.

mod cache;
mod eval;
mod misc;
mod ops;

pub use crate::bsplclib::BSplError;
pub use cache::{
    Cache, build_cache, build_cache_array, cache_d0, cache_d1, cache_d2, coefs_d0, coefs_d1, coefs_d2,
    poles_coefficients, poles_coefficients_nw,
};
pub use eval::{D3Result, Surface, d0, d1, d2, d3, dn, homogeneous_d0, homogeneous_d1, rational_derivative};
pub use misc::{
    MovedPoint, function_multiply, interpolate, interpolate_weighted, move_point, resolution, unit_weights,
};
pub use ops::{
    KnotResult, get_poles, get_poles_weighted, increase_degree, insert_knots, is_rational, iso, remove_knot,
    reverse_poles, reverse_weights, set_poles, set_poles_weighted, unperiodize,
};
