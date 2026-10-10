//! Tiny 2D vector helpers. Plain scalar arithmetic only (no FMA), `libm` for
//! transcendental functions, so results are bit-identical across platforms.

pub(crate) type V2 = [f64; 2];

pub(crate) fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

pub(crate) fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}

pub(crate) fn scale(a: V2, s: f64) -> V2 {
    [a[0] * s, a[1] * s]
}

pub(crate) fn dot(a: V2, b: V2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

pub(crate) fn cross(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

pub(crate) fn norm(a: V2) -> f64 {
    libm::hypot(a[0], a[1])
}

pub(crate) fn dist(a: V2, b: V2) -> f64 {
    norm(sub(a, b))
}

pub(crate) fn angle(a: V2) -> f64 {
    libm::atan2(a[1], a[0])
}

/// `true` unless `a > b`; in particular `true` when either value is NaN.
pub(crate) fn not_gt(a: f64, b: f64) -> bool {
    a.partial_cmp(&b) != Some(core::cmp::Ordering::Greater)
}

/// Normalises an angle into `[0, 2π)`.
pub(crate) fn wrap_2pi(a: f64) -> f64 {
    let tau = core::f64::consts::TAU;
    let mut r = a - tau * libm::floor(a / tau);
    if !(0.0..tau).contains(&r) {
        r = 0.0;
    }
    r
}
