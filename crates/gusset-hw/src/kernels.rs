//! Demonstration dispatched kernels for **display-only** work (viewport meshes, picking).
//!
//! # Determinism
//! These kernels are display-only (see [`crate::registry::Determinism`]). The FMA
//! levels (`Avx2Fma`, `Avx512`, `Neon`, `NeonDotprod`, `Sve`) round differently from
//! [`Dispatch::Scalar`]; the difference per output coordinate is bounded by
//! `8 * f32::EPSILON * (|m0*x| + |m1*y| + |m2*z| + |t|)` (tested). `bbox_f32` only
//! compares values and is bit-identical on every level. Model-defining code must call
//! the scalar level (or its own non-dispatched code), never [`crate::dispatch::active`].
//!
//! # How dispatch is made safe
//! Each SIMD variant is a safe `#[target_feature]` function (target_feature 1.1, Rust
//! 1.86+). Calling it from code without those features needs `unsafe`. The single call
//! site per variant re-checks the exact features with `is_*_feature_detected!`
//! immediately before the call, so soundness never depends on the caller passing a
//! correct [`Dispatch`] value. A requested level the CPU cannot run falls back to
//! scalar; the functions return the level that actually ran.

use crate::dispatch::Dispatch;

/// Row-major 3x4 affine transform: `out = M[.., 0..3] * p + M[.., 3]`.
pub type Affine3 = [[f32; 4]; 3];

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Minimum corner.
    pub min: [f32; 3],
    /// Maximum corner.
    pub max: [f32; 3],
}

/// Transform `points` in place with `m` using dispatch `level`. Returns the level that
/// actually ran (scalar if `level` is not runnable on this CPU).
pub fn transform_points_f32(level: Dispatch, m: &Affine3, points: &mut [[f32; 3]]) -> Dispatch {
    if run_transform(level, m, points) {
        level
    } else {
        transform_body::<false>(m, points);
        Dispatch::Scalar
    }
}

/// Bounding box of `points` (NaN coordinates are ignored; `None` if `points` is empty)
/// using dispatch `level`. Returns the box and the level that actually ran.
#[must_use]
pub fn bbox_f32(level: Dispatch, points: &[[f32; 3]]) -> (Option<Aabb>, Dispatch) {
    match run_bbox(level, points) {
        Some(b) => (b, level),
        None => (bbox_body(points), Dispatch::Scalar),
    }
}

#[inline(always)]
fn row<const FMA: bool>(r: [f32; 4], x: f32, y: f32, z: f32) -> f32 {
    let [a, b, c, d] = r;
    if FMA { a.mul_add(x, b.mul_add(y, c.mul_add(z, d))) } else { a * x + b * y + c * z + d }
}

#[inline(always)]
fn transform_body<const FMA: bool>(m: &Affine3, points: &mut [[f32; 3]]) {
    let [r0, r1, r2] = *m;
    for p in points.iter_mut() {
        let [x, y, z] = *p;
        *p = [row::<FMA>(r0, x, y, z), row::<FMA>(r1, x, y, z), row::<FMA>(r2, x, y, z)];
    }
}

#[inline(always)]
fn bbox_body(points: &[[f32; 3]]) -> Option<Aabb> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in points {
        for ((lo, hi), v) in min.iter_mut().zip(max.iter_mut()).zip(p) {
            *lo = lo.min(*v);
            *hi = hi.max(*v);
        }
    }
    if points.is_empty() { None } else { Some(Aabb { min, max }) }
}

/// `true` if `level` ran.
fn run_transform(level: Dispatch, m: &Affine3, points: &mut [[f32; 3]]) -> bool {
    match level {
        Dispatch::Scalar => {
            transform_body::<false>(m, points);
            true
        }
        #[cfg(target_arch = "x86_64")]
        Dispatch::Sse42 => x86::sse42::transform(m, points),
        #[cfg(target_arch = "x86_64")]
        Dispatch::Avx2Fma => x86::avx2fma::transform(m, points),
        #[cfg(target_arch = "x86_64")]
        Dispatch::Avx512 => x86::avx512::transform(m, points),
        #[cfg(target_arch = "aarch64")]
        Dispatch::Neon | Dispatch::NeonDotprod => {
            // NEON is part of the aarch64 baseline: no runtime check or `unsafe` needed.
            // dotprod does not help float kernels, so NeonDotprod reuses this code.
            transform_body::<true>(m, points);
            true
        }
        #[cfg(target_arch = "aarch64")]
        Dispatch::Sve => arm::sve::transform(m, points),
        #[allow(unreachable_patterns)]
        _ => false,
    }
}

/// `Some(result)` if `level` ran.
fn run_bbox(level: Dispatch, points: &[[f32; 3]]) -> Option<Option<Aabb>> {
    match level {
        Dispatch::Scalar => Some(bbox_body(points)),
        #[cfg(target_arch = "x86_64")]
        Dispatch::Sse42 => x86::sse42::bbox(points),
        #[cfg(target_arch = "x86_64")]
        Dispatch::Avx2Fma => x86::avx2fma::bbox(points),
        #[cfg(target_arch = "x86_64")]
        Dispatch::Avx512 => x86::avx512::bbox(points),
        #[cfg(target_arch = "aarch64")]
        Dispatch::Neon | Dispatch::NeonDotprod => Some(bbox_body(points)),
        #[cfg(target_arch = "aarch64")]
        Dispatch::Sve => arm::sve::bbox(points),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

/// Generates one target-feature variant: the `#[target_feature]` functions plus the
/// checked call sites (the only `unsafe` outside `crate::sys`).
macro_rules! variant {
    ($name:ident, $features:literal, $detect:ident, [$($feat:tt),+], fma = $fma:literal) => {
        #[allow(unsafe_code)]
        pub(super) mod $name {
            use super::super::{Aabb, Affine3, bbox_body, transform_body};

            #[target_feature(enable = $features)]
            fn transform_tf(m: &Affine3, points: &mut [[f32; 3]]) {
                transform_body::<$fma>(m, points);
            }

            #[target_feature(enable = $features)]
            fn bbox_tf(points: &[[f32; 3]]) -> Option<Aabb> {
                bbox_body(points)
            }

            fn available() -> bool {
                $(std::arch::$detect!($feat))&&+
            }

            pub(in crate::kernels) fn transform(m: &Affine3, points: &mut [[f32; 3]]) -> bool {
                if !available() {
                    return false;
                }
                // SAFETY: every feature `transform_tf` is compiled with was detected on
                // this CPU by `available()` just above.
                unsafe { transform_tf(m, points) };
                true
            }

            pub(in crate::kernels) fn bbox(points: &[[f32; 3]]) -> Option<Option<Aabb>> {
                if !available() {
                    return None;
                }
                // SAFETY: every feature `bbox_tf` is compiled with was detected on this
                // CPU by `available()` just above.
                Some(unsafe { bbox_tf(points) })
            }
        }
    };
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    variant!(sse42, "sse4.2,popcnt", is_x86_feature_detected, ["sse4.2", "popcnt"], fma = false);
    variant!(avx2fma, "avx,avx2,fma", is_x86_feature_detected, ["avx", "avx2", "fma"], fma = true);
    variant!(
        avx512,
        "avx2,fma,avx512f,avx512bw,avx512vl",
        is_x86_feature_detected,
        ["avx2", "fma", "avx512f", "avx512bw", "avx512vl"],
        fma = true
    );
}

#[cfg(target_arch = "aarch64")]
mod arm {
    variant!(sve, "sve", is_aarch64_feature_detected, ["sve"], fma = true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::CpuFeatures;
    use crate::dispatch::supported_levels;
    use crate::registry::{KERNEL_BBOX, KERNEL_TRANSFORM};

    /// Deterministic pseudo-random points (xorshift), including awkward magnitudes.
    fn points(n: usize) -> Vec<[f32; 3]> {
        let mut s: u32 = 0x9e37_79b9;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            // Map to [-1000, 1000].
            (f64::from(s) / f64::from(u32::MAX) * 2000.0 - 1000.0) as f32
        };
        (0..n).map(|_| [next(), next() * 1e-3, next() * 37.5]).collect()
    }

    const M: Affine3 = [[0.36, 0.48, -0.8, 12.5], [-0.8, 0.6, 0.0, -3.25], [0.48, 0.64, 0.6, 1e3]];

    #[test]
    fn registry_names_match_this_module() {
        assert!(KERNEL_TRANSFORM.starts_with("kernels::transform_points_f32 "));
        assert!(KERNEL_BBOX.starts_with("kernels::bbox_f32 "));
    }

    /// Runs `level` on `src` and checks it against the scalar reference. Returns the
    /// level that actually ran.
    fn check_against_scalar(level: Dispatch, src: &[[f32; 3]]) -> Dispatch {
        let mut reference = src.to_vec();
        assert_eq!(transform_points_f32(Dispatch::Scalar, &M, &mut reference), Dispatch::Scalar);
        let mut out = src.to_vec();
        let ran = transform_points_f32(level, &M, &mut out);
        for ((o, r), p) in out.iter().zip(&reference).zip(src) {
            for ((ov, rv), row) in o.iter().zip(r).zip(&M) {
                let [a, b, c, d] = *row;
                let mag = (a * p[0]).abs() + (b * p[1]).abs() + (c * p[2]).abs() + d.abs();
                let tol = 8.0 * f32::EPSILON * mag;
                assert!((ov - rv).abs() <= tol, "{level}: {ov} vs {rv} (tol {tol})");
            }
        }
        let (bb, bb_ran) = bbox_f32(level, src);
        assert_eq!(bb_ran, ran, "{level}: transform and bbox must pick the same level");
        let (rb, _) = bbox_f32(Dispatch::Scalar, src);
        let bits = |b: Option<Aabb>| b.map(|b| (b.min.map(f32::to_bits), b.max.map(f32::to_bits)));
        assert_eq!(bits(bb), bits(rb), "{level}: bbox must be bit-identical");
        ran
    }

    #[test]
    // CPU detection uses `cpuid` and OS queries, which Miri cannot run; the next test
    // covers the SIMD code under Miri.
    #[cfg_attr(miri, ignore)]
    fn every_supported_level_matches_scalar_within_tolerance() {
        let src = points(1027); // not a multiple of any vector width
        for &level in &supported_levels(&CpuFeatures::detect()) {
            assert_eq!(check_against_scalar(level, &src), level, "supported level must actually run");
        }
    }

    /// Every level either runs (its features are present) or falls back to scalar. Under
    /// Miri, `is_x86_feature_detected!` reports the compile-time target features, so CI
    /// runs this with `-C target-feature=...` to check each variant's `unsafe` call.
    #[test]
    fn every_level_runs_or_falls_back_to_scalar() {
        let src = points(if cfg!(miri) { 37 } else { 1027 });
        for &level in Dispatch::ALL {
            let ran = check_against_scalar(level, &src);
            assert!(ran == level || ran == Dispatch::Scalar, "{level} ran as {ran}");
        }
    }

    #[test]
    fn scalar_is_exact_on_simple_input() {
        let mut p = vec![[1.0, 2.0, 3.0]];
        let m: Affine3 = [[1.0, 0.0, 0.0, 10.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 1.0, -1.0]];
        let _ = transform_points_f32(Dispatch::Scalar, &m, &mut p);
        assert_eq!(p.first().map(|v| v.map(f32::to_bits)), Some([11.0_f32, 4.0, 2.0].map(f32::to_bits)));
    }

    #[test]
    fn unsupported_level_falls_back_to_scalar() {
        let foreign = if cfg!(target_arch = "x86_64") { Dispatch::Sve } else { Dispatch::Avx2Fma };
        let mut p = points(5);
        assert_eq!(transform_points_f32(foreign, &M, &mut p), Dispatch::Scalar);
        assert_eq!(bbox_f32(foreign, &p).1, Dispatch::Scalar);
    }

    #[test]
    fn bbox_edge_cases() {
        assert_eq!(bbox_f32(Dispatch::Scalar, &[]).0, None);
        let (b, _) = bbox_f32(Dispatch::Scalar, &[[1.0, f32::NAN, -2.0], [-1.0, 5.0, 3.0]]);
        let b = b.map(|b| (b.min.map(f32::to_bits), b.max.map(f32::to_bits)));
        assert_eq!(b, Some(([-1.0_f32, 5.0, -2.0].map(f32::to_bits), [1.0_f32, 5.0, 3.0].map(f32::to_bits))));
    }
}
