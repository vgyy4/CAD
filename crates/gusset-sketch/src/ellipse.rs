//! Ellipse geometry: an ellipse has a centre, the direction angle of its major (`a`) axis
//! and the semi-axes `a` and `b`. A point is `centre + R(angle) · (a cos t, b sin t)` with
//! `t` the eccentric anomaly. Ellipse arcs run counter-clockwise from `t0` to `t0 + span`.

use crate::geom2::{add, wrap_2pi};

/// The point at eccentric anomaly `t`.
pub fn point(center: [f64; 2], angle: f64, a: f64, b: f64, t: f64) -> [f64; 2] {
    let (x, y) = (a * libm::cos(t), b * libm::sin(t));
    let (c, s) = (libm::cos(angle), libm::sin(angle));
    add(center, [c * x - s * y, s * x + c * y])
}

/// The derivative of [`point`] with respect to `t`.
pub fn deriv(angle: f64, a: f64, b: f64, t: f64) -> [f64; 2] {
    let (x, y) = (-a * libm::sin(t), b * libm::cos(t));
    let (c, s) = (libm::cos(angle), libm::sin(angle));
    [c * x - s * y, s * x + c * y]
}

/// The eccentric anomaly in `[0, 2π)` of the curve point nearest to the direction of `p`
/// (exact for a point on the ellipse).
pub fn param(center: [f64; 2], angle: f64, a: f64, b: f64, p: [f64; 2]) -> f64 {
    let (d0, d1) = (p[0] - center[0], p[1] - center[1]);
    let (c, s) = (libm::cos(angle), libm::sin(angle));
    let (u, v) = (c * d0 + s * d1, -s * d0 + c * d1);
    wrap_2pi(libm::atan2(v / b, u / a))
}

/// First-order distance from `p` to the ellipse (signed: negative inside).
pub fn distance(center: [f64; 2], angle: f64, a: f64, b: f64, p: [f64; 2]) -> f64 {
    let (d0, d1) = (p[0] - center[0], p[1] - center[1]);
    let (c, s) = (libm::cos(angle), libm::sin(angle));
    let (u, v) = (c * d0 + s * d1, -s * d0 + c * d1);
    let f = (u / a) * (u / a) + (v / b) * (v / b) - 1.0;
    let g = 2.0 * libm::hypot(u / (a * a), v / (b * b));
    if g < 1e-300 { -a.min(b) } else { f / g }
}

/// Signed curvature (positive: counter-clockwise travel turns left) at `t`.
pub fn curvature(a: f64, b: f64, t: f64) -> f64 {
    let (s, c) = (libm::sin(t), libm::cos(t));
    let d = a * a * s * s + b * b * c * c;
    a * b / (d * libm::sqrt(d)).max(f64::MIN_POSITIVE)
}

/// The same ellipse with `a >= b`: swapping the axes turns the major direction by a
/// quarter turn and shifts every eccentric anomaly by `-π/2`. Returns `(angle, a, b,
/// shift)` where `t_new = t_old + shift`.
pub fn normalized(angle: f64, a: f64, b: f64) -> (f64, f64, f64, f64) {
    if b > a { (angle + core::f64::consts::FRAC_PI_2, b, a, -core::f64::consts::FRAC_PI_2) } else { (angle, a, b, 0.0) }
}

/// Half extents of the bounding box of the whole ellipse.
pub fn half_extents(angle: f64, a: f64, b: f64) -> [f64; 2] {
    let (c, s) = (libm::cos(angle), libm::sin(angle));
    [libm::hypot(a * c, b * s), libm::hypot(a * s, b * c)]
}

/// `t` brought into `[0, 2π)`.
pub fn wrap(t: f64) -> f64 {
    wrap_2pi(t)
}

/// The unit vector of the major axis.
pub fn major_dir(angle: f64) -> [f64; 2] {
    [libm::cos(angle), libm::sin(angle)]
}
