//! Tiny 2D vector helpers. Plain scalar arithmetic only (no FMA), `libm` for
//! transcendental functions, so results are bit-identical across platforms.

pub(crate) type V2 = [f64; 2];

pub(crate) fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

pub(crate) fn norm(a: V2) -> f64 {
    libm::hypot(a[0], a[1])
}

pub(crate) fn dist(a: V2, b: V2) -> f64 {
    norm(sub(a, b))
}
