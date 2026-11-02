//! OCCT's header-only `MathPoly` package: closed-form solvers for linear to quartic equations and a
//! Laguerre-based solver for general real polynomials (with quintic, sextic and octic helpers).
//!
//! Translated from `MathPoly_*.hxx` (OCCT 8.0.1). The C++ uses `std::complex<double>`; [`Complex`] reproduces its
//! arithmetic as compiled by GCC (`__muldc3` inline, libgcc's `__divdc3`, glibc's `csqrt`/`cabs`).
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::mathutils::{self, PolyResult, Status};

// ---------------------------------------------------------------------------------------------
// std::complex<double>
// ---------------------------------------------------------------------------------------------

/// `std::complex<double>`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    /// `std::abs` (`cabs`, i.e. `hypot`).
    pub fn abs(self) -> f64 {
        hypot(self.re, self.im)
    }
    /// `std::conj`.
    pub fn conj(self) -> Self {
        Self { re: self.re, im: -self.im }
    }
    /// `std::sqrt` (glibc `csqrt`, finite arguments).
    pub fn sqrt(self) -> Self {
        csqrt(self)
    }
    /// `complex * double`.
    pub fn scale(self, d: f64) -> Self {
        Self { re: self.re * d, im: self.im * d }
    }
}
impl core::ops::Add for Complex {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self { re: self.re + o.re, im: self.im + o.im }
    }
}
impl core::ops::Sub for Complex {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self { re: self.re - o.re, im: self.im - o.im }
    }
}
impl core::ops::SubAssign for Complex {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}
impl core::ops::Mul for Complex {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }
}
impl core::ops::Div for Complex {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        divdc3(self.re, self.im, o.re, o.im)
    }
}

/// glibc's `hypot` (2.35 and later, with a fused multiply-add: the x86-64 build dispatches on the CPU).
fn hypot(x: f64, y: f64) -> f64 {
    const SCALE: f64 = 2.409919865102884e-181; // 0x1p-600
    const LARGE_VAL: f64 = 6.703903964971299e153; // 0x1p+511
    const TINY_VAL: f64 = 1.4916681462400413e-154; // 0x1p-511
    const EPS: f64 = 5.551115123125783e-17; // 0x1p-54
    fn kernel(ax: f64, ay: f64) -> f64 {
        let (t1, t2);
        let mut h = libm::sqrt(ax * ax + ay * ay);
        if h <= 2.0 * ay {
            let delta = h - ay;
            t1 = ax * (2.0 * delta - ax);
            t2 = (delta - 2.0 * (ax - ay)) * delta;
        } else {
            let delta = h - ax;
            t1 = 2.0 * delta * (ax - 2.0 * ay);
            t2 = (4.0 * delta - ay) * ay + delta * delta;
        }
        h -= (t1 + t2) / (2.0 * h);
        h
    }
    if !x.is_finite() || !y.is_finite() {
        if x.is_infinite() || y.is_infinite() {
            return f64::INFINITY;
        }
        return x + y;
    }
    let (x, y) = (x.abs(), y.abs());
    let ax = if x < y { y } else { x };
    let ay = if x < y { x } else { y };
    if ax > LARGE_VAL {
        if ay <= ax * EPS {
            return ax + ay;
        }
        return kernel(ax * SCALE, ay * SCALE) / SCALE;
    }
    if ay < TINY_VAL {
        if ax >= ay / EPS {
            return ax + ay;
        }
        return kernel(ax / SCALE, ay / SCALE) * SCALE;
    }
    if ay <= ax * EPS {
        return ax + ay;
    }
    kernel(ax, ay)
}

/// libgcc's `__divdc3` (GCC 12 and later, double precision).
fn divdc3(mut a: f64, mut b: f64, mut c: f64, mut d: f64) -> Complex {
    const RBIG: f64 = f64::MAX / 2.0;
    const RMIN: f64 = f64::MIN_POSITIVE;
    const RMIN2: f64 = f64::EPSILON;
    const RMINSCAL: f64 = 1.0 / f64::EPSILON;
    const RMAX2: f64 = RBIG * RMIN2;
    let (x, y);
    if c.abs() < d.abs() {
        if d.abs() > RBIG {
            a *= 0.5;
            b *= 0.5;
            c *= 0.5;
            d *= 0.5;
        }
        if d.abs() < RMIN2
            || (a.abs() < RMIN && b.abs() < RMAX2 && d.abs() < RMAX2)
            || (b.abs() < RMIN && a.abs() < RMAX2 && d.abs() < RMAX2)
        {
            a *= RMINSCAL;
            b *= RMINSCAL;
            c *= RMINSCAL;
            d *= RMINSCAL;
        }
        let ratio = c / d;
        let denom = (c * ratio) + d;
        if ratio.abs() > RMIN {
            x = ((a * ratio) + b) / denom;
            y = ((b * ratio) - a) / denom;
        } else {
            x = ((c * (a / d)) + b) / denom;
            y = ((c * (b / d)) - a) / denom;
        }
    } else {
        if c.abs() > RBIG {
            a *= 0.5;
            b *= 0.5;
            c *= 0.5;
            d *= 0.5;
        }
        if c.abs() < RMIN2
            || (a.abs() < RMIN && b.abs() < RMAX2 && c.abs() < RMAX2)
            || (b.abs() < RMIN && a.abs() < RMAX2 && c.abs() < RMAX2)
        {
            a *= RMINSCAL;
            b *= RMINSCAL;
            c *= RMINSCAL;
            d *= RMINSCAL;
        }
        let ratio = d / c;
        let denom = (d * ratio) + c;
        if ratio.abs() > RMIN {
            x = ((b * ratio) + a) / denom;
            y = (b - (a * ratio)) / denom;
        } else {
            x = (a + (d * (b / c))) / denom;
            y = (b - (d * (a / c))) / denom;
        }
    }
    if x.is_nan() && y.is_nan() && c == 0.0 && d == 0.0 && (!a.is_nan() || !b.is_nan()) {
        let inf = f64::INFINITY.copysign(c);
        return Complex { re: inf * a, im: inf * b };
    }
    Complex { re: x, im: y }
}

/// glibc's `csqrt` for finite arguments.
fn csqrt(x: Complex) -> Complex {
    let (mut re, mut im) = (x.re, x.im);
    if !re.is_finite() || !im.is_finite() {
        return Complex { re: f64::NAN, im: f64::NAN };
    }
    if im == 0.0 {
        return if re < 0.0 {
            Complex { re: 0.0, im: libm::sqrt(-re).copysign(im) }
        } else {
            Complex { re: libm::sqrt(re).abs(), im: 0.0_f64.copysign(im) }
        };
    }
    if re == 0.0 {
        let r = if im.abs() >= 2.0 * f64::MIN_POSITIVE {
            libm::sqrt(0.5 * im.abs())
        } else {
            0.5 * libm::sqrt(2.0 * im.abs())
        };
        return Complex { re: r, im: r.copysign(im) };
    }
    let mut scale: i32 = 0;
    if re.abs() > f64::MAX / 4.0 {
        scale = 1;
        re = libm::scalbn(re, -2);
        im = libm::scalbn(im, -2);
    } else if im.abs() > f64::MAX / 4.0 {
        scale = 1;
        re = if re.abs() >= 4.0 * f64::MIN_POSITIVE { libm::scalbn(re, -2) } else { 0.0 };
        im = libm::scalbn(im, -2);
    } else if re.abs() < 2.0 * f64::MIN_POSITIVE && im.abs() < 2.0 * f64::MIN_POSITIVE {
        scale = -((53 + 1) / 2);
        re = libm::scalbn(re, -2 * scale);
        im = libm::scalbn(im, -2 * scale);
    }
    let d = hypot(re, im);
    let (mut r, mut s);
    if re > 0.0 {
        r = libm::sqrt(0.5 * (d + re));
        if scale == 1 && im.abs() < 1.0 {
            s = im / r;
            r = libm::scalbn(r, scale);
            scale = 0;
        } else {
            s = 0.5 * (im / r);
        }
    } else {
        s = libm::sqrt(0.5 * (d - re));
        if scale == 1 && im.abs() < 1.0 {
            r = (im / s).abs();
            s = libm::scalbn(s, scale);
            scale = 0;
        } else {
            r = (0.5 * (im / s)).abs();
        }
    }
    if scale != 0 {
        r = libm::scalbn(r, scale);
        s = libm::scalbn(s, scale);
    }
    Complex { re: r, im: s.copysign(im) }
}

// ---------------------------------------------------------------------------------------------
// MathPoly_Quadratic.hxx
// ---------------------------------------------------------------------------------------------

/// `std::max({a, b, c, ...})`.
fn max_of(values: &[f64]) -> f64 {
    let mut m = values.first().copied().unwrap_or(0.0);
    for &v in values.iter().skip(1) {
        if m < v {
            m = v;
        }
    }
    m
}

fn set_root(r: &mut PolyResult, i: usize, v: f64) {
    if let Some(slot) = r.roots.get_mut(i) {
        *slot = v;
    }
}

fn root(r: &PolyResult, i: usize) -> f64 {
    r.roots.get(i).copied().unwrap_or(0.0)
}

/// `MathPoly::Linear`.
pub fn linear(a: f64, b: f64) -> PolyResult {
    let mut result = PolyResult::default();
    if mathutils::is_zero(a, mathutils::THE_ZERO_TOL) {
        if mathutils::is_zero(b, mathutils::THE_ZERO_TOL) {
            result.status = Status::InfiniteSolutions;
        } else {
            result.status = Status::NoSolution;
        }
    } else {
        result.status = Status::Ok;
        result.nb_roots = 1;
        set_root(&mut result, 0, -b / a);
    }
    result
}

/// `MathPoly::Quadratic`.
pub fn quadratic(a: f64, b: f64, c: f64) -> PolyResult {
    let mut result = PolyResult::default();
    if mathutils::is_zero(a, mathutils::THE_ZERO_TOL) {
        return linear(b, c);
    }
    let scale = max_of(&[a.abs(), b.abs(), c.abs()]);
    if scale < mathutils::THE_ZERO_TOL {
        result.status = Status::InfiniteSolutions;
        return result;
    }
    let (a, b, c) = (a / scale, b / scale, c / scale);
    let disc = b * b - 4.0 * a * c;
    let disc_tol = mathutils::THE_ZERO_TOL * (b * b + (4.0 * a * c).abs());
    if disc < -disc_tol {
        result.status = Status::Ok;
        result.nb_roots = 0;
        return result;
    }
    if disc.abs() <= disc_tol {
        result.status = Status::Ok;
        result.nb_roots = 1;
        set_root(&mut result, 0, -b / (2.0 * a));
        return result;
    }
    let sqrt_disc = disc.sqrt();
    let q = -0.5 * (b + mathutils::sign_transfer(sqrt_disc, b));
    result.status = Status::Ok;
    result.nb_roots = 2;
    set_root(&mut result, 0, q / a);
    set_root(&mut result, 1, c / q);
    if root(&result, 0) > root(&result, 1) {
        result.roots.swap(0, 1);
    }
    result
}

// ---------------------------------------------------------------------------------------------
// MathPoly_Cubic.hxx
// ---------------------------------------------------------------------------------------------

/// `MathPoly::Cubic`.
pub fn cubic(a_in: f64, b_in: f64, c_in: f64, d_in: f64) -> PolyResult {
    let mut result = PolyResult::default();
    if mathutils::is_zero(a_in, mathutils::THE_ZERO_TOL) {
        return quadratic(b_in, c_in, d_in);
    }
    let scale = max_of(&[a_in.abs(), b_in.abs(), c_in.abs(), d_in.abs()]);
    if scale < mathutils::THE_ZERO_TOL {
        result.status = Status::InfiniteSolutions;
        return result;
    }
    let p = b_in / a_in;
    let q = c_in / a_in;
    let r = d_in / a_in;
    let p3 = p / 3.0;
    let p3_sq = p3 * p3;
    let a = q - 3.0 * p3_sq;
    let b = r - p3 * q + 2.0 * p3_sq * p3;
    let half_b = b / 2.0;
    let third_a = a / 3.0;
    let disc = half_b * half_b + third_a * third_a * third_a;
    let disc_tol = mathutils::THE_ZERO_TOL * max_of(&[half_b * half_b, (third_a * third_a * third_a).abs()]);
    let coeffs = [d_in, c_in, b_in, a_in];
    if disc > disc_tol {
        let sqrt_disc = disc.sqrt();
        let u = mathutils::cube_root(-half_b + sqrt_disc);
        let v = mathutils::cube_root(-half_b - sqrt_disc);
        result.status = Status::Ok;
        result.nb_roots = 1;
        set_root(&mut result, 0, u + v - p3);
        let refined = mathutils::refine_poly_root(&coeffs, 3, root(&result, 0), 5);
        set_root(&mut result, 0, refined);
    } else if disc < -disc_tol {
        let r_val = (-third_a * third_a * third_a).sqrt();
        let cos_arg = mathutils::clamp(-half_b / r_val, -1.0, 1.0);
        let theta = libm::acos(cos_arg);
        let two_sqrt_neg_a3 = 2.0 * (-third_a).sqrt();
        result.status = Status::Ok;
        result.nb_roots = 3;
        set_root(&mut result, 0, two_sqrt_neg_a3 * libm::cos(theta / 3.0) - p3);
        set_root(&mut result, 1, two_sqrt_neg_a3 * libm::cos((theta + 2.0 * mathutils::THE_PI) / 3.0) - p3);
        set_root(&mut result, 2, two_sqrt_neg_a3 * libm::cos((theta + 4.0 * mathutils::THE_PI) / 3.0) - p3);
        for i in 0..3 {
            let refined = mathutils::refine_poly_root(&coeffs, 3, root(&result, i), 5);
            set_root(&mut result, i, refined);
        }
        if let Some(head) = result.roots.get_mut(..3) {
            mathutils::sort_roots(head);
        }
    } else {
        let u = mathutils::cube_root(-half_b);
        result.status = Status::Ok;
        if mathutils::is_zero(u, mathutils::THE_ZERO_TOL) {
            result.nb_roots = 1;
            set_root(&mut result, 0, -p3);
        } else {
            result.nb_roots = 2;
            set_root(&mut result, 0, 2.0 * u - p3);
            set_root(&mut result, 1, -u - p3);
            let r0 = mathutils::refine_poly_root(&coeffs, 3, root(&result, 0), 5);
            set_root(&mut result, 0, r0);
            let r1 = mathutils::refine_poly_root(&coeffs, 3, root(&result, 1), 5);
            set_root(&mut result, 1, r1);
            if root(&result, 0) > root(&result, 1) {
                result.roots.swap(0, 1);
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------------------------
// MathPoly_Quartic.hxx
// ---------------------------------------------------------------------------------------------

/// `aResult.Roots[aResult.NbRoots++] = v`.
fn push_root(r: &mut PolyResult, v: f64) {
    set_root(r, r.nb_roots, v);
    r.nb_roots += 1;
}

/// The final refine / sort / deduplicate of `Quartic`.
fn finish_quartic(result: &mut PolyResult, coeffs: &[f64; 5]) {
    for i in 0..result.nb_roots {
        let refined = mathutils::refine_poly_root(coeffs, 4, root(result, i), 5);
        set_root(result, i, refined);
    }
    let n = result.nb_roots;
    if let Some(head) = result.roots.get_mut(..n) {
        mathutils::sort_roots(head);
        result.nb_roots = mathutils::remove_duplicate_roots(head, 1.0e-10);
    }
}

/// `MathPoly::Quartic`.
pub fn quartic(a_in: f64, b_in: f64, c_in: f64, d_in: f64, e_in: f64) -> PolyResult {
    let mut result = PolyResult::default();
    if mathutils::is_zero(a_in, mathutils::THE_ZERO_TOL) {
        return cubic(b_in, c_in, d_in, e_in);
    }
    let scale = max_of(&[a_in.abs(), b_in.abs(), c_in.abs(), d_in.abs(), e_in.abs()]);
    if scale < mathutils::THE_ZERO_TOL {
        result.status = Status::InfiniteSolutions;
        return result;
    }
    let a = b_in / a_in;
    let b = c_in / a_in;
    let c = d_in / a_in;
    let d = e_in / a_in;
    let shift = a / 4.0;
    let a2 = a * a;
    let a3 = a2 * a;
    let a4 = a2 * a2;
    let p = b - 3.0 * a2 / 8.0;
    let q = c - a * b / 2.0 + a3 / 8.0;
    let r = d - a * c / 4.0 + a2 * b / 16.0 - 3.0 * a4 / 256.0;
    let coeffs = [e_in, d_in, c_in, b_in, a_in];
    if mathutils::is_zero(q, mathutils::THE_ZERO_TOL) {
        let quad = quadratic(1.0, p, r);
        if !quad.is_done() {
            result.status = quad.status;
            return result;
        }
        result.status = Status::Ok;
        result.nb_roots = 0;
        for i in 0..quad.nb_roots {
            let u = root(&quad, i);
            if u >= -mathutils::THE_ZERO_TOL {
                if u <= mathutils::THE_ZERO_TOL {
                    push_root(&mut result, -shift);
                } else {
                    let sqrt_u = u.sqrt();
                    push_root(&mut result, sqrt_u - shift);
                    push_root(&mut result, -sqrt_u - shift);
                }
            }
        }
        finish_quartic(&mut result, &coeffs);
        return result;
    }
    let cubic_result = cubic(1.0, 2.0 * p, p * p - 4.0 * r, -q * q);
    if !cubic_result.is_done() || cubic_result.nb_roots == 0 {
        result.status = Status::NumericalError;
        return result;
    }
    let mut z = root(&cubic_result, cubic_result.nb_roots - 1);
    if z < -mathutils::THE_ZERO_TOL {
        for i in 0..cubic_result.nb_roots {
            if root(&cubic_result, i) >= -mathutils::THE_ZERO_TOL {
                z = cmax(0.0, root(&cubic_result, i));
                break;
            }
        }
    }
    z = cmax(0.0, z);
    let s = z.sqrt();
    if mathutils::is_zero(s, mathutils::THE_ZERO_TOL) {
        let uv = quadratic(1.0, p, r);
        let (u, v) = if !uv.is_done() || uv.nb_roots < 2 { (p / 2.0, p / 2.0) } else { (root(&uv, 0), root(&uv, 1)) };
        result.status = Status::Ok;
        result.nb_roots = 0;
        for w in [u, v] {
            if w <= mathutils::THE_ZERO_TOL {
                let sqrt = cmax(0.0, -w).sqrt();
                push_root(&mut result, sqrt - shift);
                if sqrt > mathutils::THE_ZERO_TOL {
                    push_root(&mut result, -sqrt - shift);
                }
            }
        }
    } else {
        let half_p_plus_z = (p + z) / 2.0;
        let q_over_2s = q / (2.0 * s);
        let u = half_p_plus_z - q_over_2s;
        let v = half_p_plus_z + q_over_2s;
        let quad1 = quadratic(1.0, s, u);
        let quad2 = quadratic(1.0, -s, v);
        result.status = Status::Ok;
        result.nb_roots = 0;
        if quad1.is_done() {
            for i in 0..quad1.nb_roots {
                push_root(&mut result, root(&quad1, i) - shift);
            }
        }
        if quad2.is_done() {
            for i in 0..quad2.nb_roots {
                push_root(&mut result, root(&quad2, i) - shift);
            }
        }
    }
    finish_quartic(&mut result, &coeffs);
    result
}

/// `std::max(a, b)`.
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

// ---------------------------------------------------------------------------------------------
// MathPoly_Laguerre.hxx
// ---------------------------------------------------------------------------------------------

/// `MathPoly::THE_MAX_POLY_DEGREE`.
pub const THE_MAX_POLY_DEGREE: usize = 20;

/// `MathPoly::GeneralPolyResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneralPolyResult {
    pub status: Status,
    pub roots: [f64; THE_MAX_POLY_DEGREE],
    pub complex_roots: [Complex; THE_MAX_POLY_DEGREE],
    pub nb_roots: usize,
    pub nb_complex_roots: usize,
}
impl Default for GeneralPolyResult {
    fn default() -> Self {
        Self {
            status: Status::NotConverged,
            roots: [0.0; THE_MAX_POLY_DEGREE],
            complex_roots: [Complex::default(); THE_MAX_POLY_DEGREE],
            nb_roots: 0,
            nb_complex_roots: 0,
        }
    }
}
impl GeneralPolyResult {
    /// `IsDone()` (and the C++ `explicit operator bool`).
    pub fn is_done(&self) -> bool {
        self.status == Status::Ok
    }
}

fn co(c: &[f64], i: i32) -> f64 {
    usize::try_from(i).ok().and_then(|k| c.get(k)).copied().unwrap_or(0.0)
}

/// `detail::EvaluatePolynomialWithDerivatives`: `(P, P', P'')`.
pub fn evaluate_polynomial_with_derivatives(coeffs: &[f64], degree: i32, x: Complex) -> (Complex, Complex, Complex) {
    let mut p = Complex::new(co(coeffs, degree), 0.0);
    let mut dp = Complex::new(0.0, 0.0);
    let mut d2p = Complex::new(0.0, 0.0);
    for i in (0..degree).rev() {
        d2p = d2p * x + dp;
        dp = dp * x + p;
        p = p * x + Complex::new(co(coeffs, i), 0.0);
    }
    d2p = d2p.scale(2.0);
    (p, dp, d2p)
}

/// `detail::LaguerreIteration`.
pub fn laguerre_iteration(coeffs: &[f64], degree: i32, x0: Complex, tol: f64, max_iter: i32) -> Complex {
    let mut x = x0;
    let n = f64::from(degree);
    for _ in 0..max_iter {
        let (p, dp, d2p) = evaluate_polynomial_with_derivatives(coeffs, degree, x);
        let abs_p = p.abs();
        if abs_p < tol {
            return x;
        }
        let g = dp / p;
        let h = g * g - d2p / p;
        let sq = (h.scale(n) - g * g).scale(n - 1.0).sqrt();
        let denom1 = g + sq;
        let denom2 = g - sq;
        let denom = if denom1.abs() > denom2.abs() { denom1 } else { denom2 };
        let delta = if denom.abs() < mathutils::THE_ZERO_TOL {
            if dp.abs() < mathutils::THE_ZERO_TOL { Complex::new(1.0 + x.abs(), 0.0) } else { p / dp }
        } else {
            Complex::new(n, 0.0) / denom
        };
        x -= delta;
        if delta.abs() < tol * (1.0 + x.abs()) {
            return x;
        }
    }
    x
}

/// `detail::DeflateReal`: divides by `(x - root)`; `degree` is decremented.
pub fn deflate_real(coeffs: &mut [f64], degree: &mut i32, root: f64) {
    let mut carry = co(coeffs, *degree);
    for i in (0..*degree).rev() {
        let temp = co(coeffs, i);
        if let Some(slot) = usize::try_from(i).ok().and_then(|k| coeffs.get_mut(k)) {
            *slot = carry;
        }
        carry = temp + carry * root;
    }
    *degree -= 1;
}

/// `detail::DeflateComplex`: divides by the quadratic with the roots `root` and its conjugate (`degree >= 2`).
pub fn deflate_complex(coeffs: &mut [f64], degree: &mut i32, root: Complex) {
    let re = root.re;
    let im = root.im;
    let b = -2.0 * re;
    let c = re * re + im * im;
    let mut quotient = [0.0_f64; THE_MAX_POLY_DEGREE + 1];
    let deg = *degree;
    let set = |q: &mut [f64; THE_MAX_POLY_DEGREE + 1], i: i32, v: f64| {
        if let Some(slot) = usize::try_from(i).ok().and_then(|k| q.get_mut(k)) {
            *slot = v;
        }
    };
    let get = |q: &[f64; THE_MAX_POLY_DEGREE + 1], i: i32| {
        usize::try_from(i).ok().and_then(|k| q.get(k)).copied().unwrap_or(0.0)
    };
    set(&mut quotient, deg - 2, co(coeffs, deg));
    if deg >= 3 {
        let v = co(coeffs, deg - 1) - b * get(&quotient, deg - 2);
        set(&mut quotient, deg - 3, v);
    }
    let mut i = deg - 4;
    while i >= 0 {
        let v = co(coeffs, i + 2) - b * get(&quotient, i + 1) - c * get(&quotient, i + 2);
        set(&mut quotient, i, v);
        i -= 1;
    }
    for i in 0..=(deg - 2) {
        if let Some(slot) = usize::try_from(i).ok().and_then(|k| coeffs.get_mut(k)) {
            *slot = get(&quotient, i);
        }
    }
    *degree -= 2;
}

/// `detail::RefineRealRoot`.
pub fn refine_real_root(orig_coeffs: &[f64], orig_degree: i32, root: f64) -> f64 {
    const MAX_ITER: i32 = 10;
    const TOL: f64 = 1.0e-14;
    let mut x = root;
    for _ in 0..MAX_ITER {
        let mut p = co(orig_coeffs, orig_degree);
        let mut dp = 0.0;
        for i in (0..orig_degree).rev() {
            dp = dp * x + p;
            p = p * x + co(orig_coeffs, i);
        }
        if dp.abs() < mathutils::THE_ZERO_TOL {
            break;
        }
        let delta = p / dp;
        x -= delta;
        if delta.abs() < TOL * (1.0 + x.abs()) {
            break;
        }
    }
    x
}

/// `MathPoly::Laguerre` (default `tol` `1e-12`): all roots of the real polynomial with ascending coefficients.
pub fn laguerre(coeffs: &[f64], degree: i32, tol: f64) -> GeneralPolyResult {
    let mut result = GeneralPolyResult::default();
    if degree < 1 || degree > THE_MAX_POLY_DEGREE as i32 {
        result.status = Status::InvalidInput;
        return result;
    }
    if co(coeffs, degree).abs() < mathutils::THE_ZERO_TOL {
        result.status = Status::InvalidInput;
        return result;
    }
    let mut work = [0.0_f64; THE_MAX_POLY_DEGREE + 1];
    let mut orig = [0.0_f64; THE_MAX_POLY_DEGREE + 1];
    for i in 0..=degree {
        if let Ok(k) = usize::try_from(i) {
            if let Some(w) = work.get_mut(k) {
                *w = co(coeffs, i);
            }
            if let Some(o) = orig.get_mut(k) {
                *o = co(coeffs, i);
            }
        }
    }
    let mut deg = degree;
    let mut start_idx: usize = 0;
    let start_points =
        [Complex::new(0.0, 0.1), Complex::new(1.0, 0.5), Complex::new(-0.5, 0.3), Complex::new(0.5, -0.3)];
    while deg > 0 {
        let x0 = start_points.get(start_idx % 4).copied().unwrap_or_default();
        start_idx += 1;
        let root = laguerre_iteration(&work, deg, x0, tol, 100);
        let imag_part = root.im.abs();
        let real_part = root.re.abs();
        let scale = cmax(1.0, real_part);
        if imag_part < tol * scale {
            let mut real_root = root.re;
            real_root = refine_real_root(&orig, degree, real_root);
            if let Some(slot) = result.roots.get_mut(result.nb_roots) {
                *slot = real_root;
            }
            result.nb_roots += 1;
            deflate_real(&mut work, &mut deg, real_root);
        } else {
            if let Some(slot) = result.complex_roots.get_mut(result.nb_complex_roots) {
                *slot = root;
            }
            result.nb_complex_roots += 1;
            if let Some(slot) = result.complex_roots.get_mut(result.nb_complex_roots) {
                *slot = root.conj();
            }
            result.nb_complex_roots += 1;
            deflate_complex(&mut work, &mut deg, root);
        }
    }
    let n = result.nb_roots;
    if let Some(head) = result.roots.get_mut(..n) {
        head.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    }
    if result.nb_roots > 1 {
        let mut new_count = 1;
        for i in 1..result.nb_roots {
            let (Some(&ri), Some(&last)) = (result.roots.get(i), result.roots.get(new_count - 1)) else {
                break;
            };
            if (ri - last).abs() > tol {
                if let Some(slot) = result.roots.get_mut(new_count) {
                    *slot = ri;
                }
                new_count += 1;
            }
        }
        result.nb_roots = new_count;
    }
    result.status = Status::Ok;
    result
}

/// `MathPoly::LaguerreN` (default `tol` `1e-12`): `coeffs.len()` coefficients.
pub fn laguerre_n(coeffs: &[f64], tol: f64) -> GeneralPolyResult {
    if coeffs.len() < 2 {
        return GeneralPolyResult { status: Status::InvalidInput, ..GeneralPolyResult::default() };
    }
    laguerre(coeffs, i32::try_from(coeffs.len() - 1).unwrap_or(i32::MAX), tol)
}

/// Copies the first four real roots of a general result into a `PolyResult` (as `Quintic`/`Sextic` do).
fn first_four(gen_result: &GeneralPolyResult) -> PolyResult {
    let mut result = PolyResult { status: Status::Ok, nb_roots: gen_result.nb_roots.min(4), ..PolyResult::default() };
    for i in 0..result.nb_roots {
        set_root(&mut result, i, gen_result.roots.get(i).copied().unwrap_or(0.0));
    }
    result
}

/// `MathPoly::Sextic`.
pub fn sextic(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64) -> PolyResult {
    let mut result = PolyResult::default();
    if mathutils::is_zero(a, mathutils::THE_ZERO_TOL) {
        let coeffs = [g, f, e, d, c, b];
        let gen_result = laguerre(&coeffs, 5, 1.0e-12);
        if !gen_result.is_done() {
            result.status = gen_result.status;
            return result;
        }
        return first_four(&gen_result);
    }
    let coeffs = [g, f, e, d, c, b, a];
    let gen_result = laguerre(&coeffs, 6, 1.0e-12);
    if !gen_result.is_done() {
        result.status = gen_result.status;
        return result;
    }
    first_four(&gen_result)
}

/// `MathPoly::Quintic`.
pub fn quintic(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> PolyResult {
    let mut result = PolyResult::default();
    if mathutils::is_zero(a, mathutils::THE_ZERO_TOL) {
        return quartic(b, c, d, e, f);
    }
    let coeffs = [f, e, d, c, b, a];
    let gen_result = laguerre(&coeffs, 5, 1.0e-12);
    if !gen_result.is_done() {
        result.status = gen_result.status;
        return result;
    }
    first_four(&gen_result)
}

/// `MathPoly::Octic`.
pub fn octic(coeffs: &[f64; 9]) -> GeneralPolyResult {
    laguerre(coeffs, 8, 1.0e-12)
}
