//! OCCT's package `Geom2dEval`: the 2D evaluator curves `Geom2dEval_ArchimedeanSpiralCurve`, `CircleInvoluteCurve`,
//! `LogarithmicSpiralCurve`, `SineWaveCurve`, `AHTBezierCurve` and `TBezierCurve`.
//!
//! Translated from `Geom2dEval_*.hxx/.cxx` (OCCT 8.0.1). Every struct implements `geom2d::{Geometry2d, Curve2d}` (the two
//! Bezier curves also `BoundedCurve2d`). `Reverse`, `ReversedParameter` and `Transform` throw `Standard_NotImplemented` in
//! C++: the trait methods that cannot fail do nothing for these curves, and `try_reverse`, `try_reversed_parameter` and
//! `try_transform` return `Err(Geom2dError::NotImplemented)`. (`Geom2dEval_RepCurveDesc` and `RepUtils` are part of
//! `geom2d`.) The evaluation of the elementary functions goes through `libm`.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::geom2d::{
    BoundedCurve2d, Curve2d, Geom2dError, Geometry2d, PRECISION_INFINITE, ResD1, ResD2, ResD3, Result, confusion,
    geometry_any,
};
use crate::gp::{Ax2d, Pnt2d, Trsf2d, Vec2d};
use crate::plib::GeomAbsShape;
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

/// A `gp_XY` for the arithmetic of the formulas (componentwise, one operation per component).
#[derive(Debug, Clone, Copy, Default)]
struct Xy {
    x: f64,
    y: f64,
}
impl Xy {
    const ZERO: Xy = Xy { x: 0.0, y: 0.0 };
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    fn of(p: &Pnt2d) -> Self {
        Self { x: p.x(), y: p.y() }
    }
}
impl Add for Xy {
    type Output = Xy;
    fn add(self, o: Xy) -> Xy {
        Xy::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Xy {
    type Output = Xy;
    fn sub(self, o: Xy) -> Xy {
        Xy::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for Xy {
    type Output = Xy;
    fn mul(self, s: f64) -> Xy {
        Xy::new(self.x * s, self.y * s)
    }
}
impl Mul<Xy> for f64 {
    type Output = Xy;
    fn mul(self, v: Xy) -> Xy {
        Xy::new(self * v.x, self * v.y)
    }
}
impl Div<f64> for Xy {
    type Output = Xy;
    fn div(self, s: f64) -> Xy {
        Xy::new(self.x / s, self.y / s)
    }
}
impl AddAssign for Xy {
    fn add_assign(&mut self, o: Xy) {
        *self = *self + o;
    }
}
impl SubAssign for Xy {
    fn sub_assign(&mut self, o: Xy) {
        *self = *self - o;
    }
}
fn pnt(v: Xy) -> Pnt2d {
    Pnt2d::new(v.x, v.y)
}
fn vec(v: Xy) -> Vec2d {
    Vec2d::new(v.x, v.y)
}

/// `trigShiftByQuarterTurns`: `(sin, cos)` of `u + n * pi / 2` from those of `u`.
fn trig_shift(sin: f64, cos: f64, n: i32) -> (f64, f64) {
    match n & 3 {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

/// `powInt`: repeated multiplication.
fn pow_int(base: f64, exp: i32) -> f64 {
    let mut r = 1.0;
    for _ in 0..exp {
        r *= base;
    }
    r
}

/// The local frame `(origin, x direction, y direction)` of an axis.
fn frame(pos: &Ax2d) -> (Xy, Xy, Xy) {
    let o = Xy::of(pos.location());
    let xd = Xy::new(pos.direction().x(), pos.direction().y());
    (o, xd, Xy::new(-xd.y, xd.x))
}

fn check_n(n: i32) -> Result<()> {
    if n < 1 { Err(Geom2dError::Range) } else { Ok(()) }
}

/// The methods every evaluator curve has in the same form.
macro_rules! eval_curve_common {
    () => {
        fn copy_curve(&self) -> Box<dyn Curve2d> {
            Box::new(self.clone())
        }
        fn reverse(&mut self) {}
        fn reversed_parameter(&self, u: f64) -> f64 {
            u
        }
        fn try_reverse(&mut self) -> Result<()> {
            Err(Geom2dError::NotImplemented)
        }
        fn try_reversed_parameter(&self, _u: f64) -> Result<f64> {
            Err(Geom2dError::NotImplemented)
        }
        fn is_periodic(&self) -> bool {
            false
        }
        fn continuity(&self) -> GeomAbsShape {
            GeomAbsShape::Cn
        }
        fn is_cn(&self, _n: i32) -> Result<bool> {
            Ok(true)
        }
    };
}
macro_rules! eval_geometry_common {
    () => {
        geometry_any!();
        fn transform(&mut self, _t: &Trsf2d) {}
        fn try_transform(&mut self, _t: &Trsf2d) -> Result<()> {
            Err(Geom2dError::NotImplemented)
        }
        fn copy_geometry(&self) -> Box<dyn Geometry2d> {
            Box::new(self.clone())
        }
    };
}

// ---------------------------------------------------------------------------------------------
// Geom2dEval_ArchimedeanSpiralCurve
// ---------------------------------------------------------------------------------------------

/// `Geom2dEval_ArchimedeanSpiralCurve`: `r = r0 + b t` at the angle `t >= 0`.
#[derive(Debug, Clone)]
pub struct ArchimedeanSpiralCurve {
    position: Ax2d,
    initial_radius: f64,
    growth_rate: f64,
}

impl ArchimedeanSpiralCurve {
    /// The constructor; `Err(Construction)` if `initial_radius < 0` or `growth_rate <= 0`.
    pub fn new(position: Ax2d, initial_radius: f64, growth_rate: f64) -> Result<Self> {
        if initial_radius < 0.0 || growth_rate <= 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Self { position, initial_radius, growth_rate })
    }
    /// `Position`.
    pub fn position(&self) -> &Ax2d {
        &self.position
    }
    /// `InitialRadius`.
    pub fn initial_radius(&self) -> f64 {
        self.initial_radius
    }
    /// `GrowthRate`.
    pub fn growth_rate(&self) -> f64 {
        self.growth_rate
    }
}

impl Geometry2d for ArchimedeanSpiralCurve {
    eval_geometry_common!();
}

impl Curve2d for ArchimedeanSpiralCurve {
    eval_curve_common!();
    fn first_parameter(&self) -> f64 {
        0.0
    }
    fn last_parameter(&self) -> f64 {
        PRECISION_INFINITE
    }
    fn is_closed(&self) -> bool {
        false
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        let (o, xd, yd) = frame(&self.position);
        let r = self.initial_radius + self.growth_rate * u;
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(pnt(o + (r * cos_u) * xd + (r * sin_u) * yd))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (o, xd, yd) = frame(&self.position);
        let r = self.initial_radius + self.growth_rate * u;
        let b = self.growth_rate;
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(ResD1 {
            point: pnt(o + (r * cos_u) * xd + (r * sin_u) * yd),
            d1: vec((b * cos_u - r * sin_u) * xd + (b * sin_u + r * cos_u) * yd),
        })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (o, xd, yd) = frame(&self.position);
        let r = self.initial_radius + self.growth_rate * u;
        let b = self.growth_rate;
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(ResD2 {
            point: pnt(o + (r * cos_u) * xd + (r * sin_u) * yd),
            d1: vec((b * cos_u - r * sin_u) * xd + (b * sin_u + r * cos_u) * yd),
            d2: vec((-2.0 * b * sin_u - r * cos_u) * xd + (2.0 * b * cos_u - r * sin_u) * yd),
        })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (o, xd, yd) = frame(&self.position);
        let r = self.initial_radius + self.growth_rate * u;
        let b = self.growth_rate;
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(ResD3 {
            point: pnt(o + (r * cos_u) * xd + (r * sin_u) * yd),
            d1: vec((b * cos_u - r * sin_u) * xd + (b * sin_u + r * cos_u) * yd),
            d2: vec((-2.0 * b * sin_u - r * cos_u) * xd + (2.0 * b * cos_u - r * sin_u) * yd),
            d3: vec((-3.0 * b * cos_u + r * sin_u) * xd + (-3.0 * b * sin_u - r * cos_u) * yd),
        })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        check_n(n)?;
        // Leibniz: d^N[(a + b t) f] = (a + b t) f^(N) + N b f^(N-1)
        let (_, xd, yd) = frame(&self.position);
        let r = self.initial_radius + self.growth_rate * u;
        let b = self.growth_rate;
        let (sin_u, cos_u) = (libm::sin(u), libm::cos(u));
        let (sin_n, cos_n) = trig_shift(sin_u, cos_u, n);
        let (sin_nm1, cos_nm1) = trig_shift(sin_u, cos_u, n - 1);
        let cx = r * cos_n + f64::from(n) * b * cos_nm1;
        let cy = r * sin_n + f64::from(n) * b * sin_nm1;
        Ok(vec(cx * xd + cy * yd))
    }
}

// ---------------------------------------------------------------------------------------------
// Geom2dEval_CircleInvoluteCurve
// ---------------------------------------------------------------------------------------------

/// `Geom2dEval_CircleInvoluteCurve`.
#[derive(Debug, Clone)]
pub struct CircleInvoluteCurve {
    position: Ax2d,
    radius: f64,
}

impl CircleInvoluteCurve {
    /// The constructor; `Err(Construction)` if `radius <= 0`.
    pub fn new(position: Ax2d, radius: f64) -> Result<Self> {
        if radius <= 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Self { position, radius })
    }
    /// `Position`.
    pub fn position(&self) -> &Ax2d {
        &self.position
    }
    /// `Radius`.
    pub fn radius(&self) -> f64 {
        self.radius
    }
}

impl Geometry2d for CircleInvoluteCurve {
    eval_geometry_common!();
}

impl Curve2d for CircleInvoluteCurve {
    eval_curve_common!();
    fn first_parameter(&self) -> f64 {
        0.0
    }
    fn last_parameter(&self) -> f64 {
        PRECISION_INFINITE
    }
    fn is_closed(&self) -> bool {
        false
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        let (o, xd, yd) = frame(&self.position);
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        let r = self.radius;
        Ok(pnt(o + (r * (cos_u + u * sin_u)) * xd + (r * (sin_u - u * cos_u)) * yd))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (o, xd, yd) = frame(&self.position);
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        let r = self.radius;
        Ok(ResD1 {
            point: pnt(o + (r * (cos_u + u * sin_u)) * xd + (r * (sin_u - u * cos_u)) * yd),
            d1: vec((r * u * cos_u) * xd + (r * u * sin_u) * yd),
        })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (o, xd, yd) = frame(&self.position);
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        let r = self.radius;
        Ok(ResD2 {
            point: pnt(o + (r * (cos_u + u * sin_u)) * xd + (r * (sin_u - u * cos_u)) * yd),
            d1: vec((r * u * cos_u) * xd + (r * u * sin_u) * yd),
            d2: vec((r * (cos_u - u * sin_u)) * xd + (r * (sin_u + u * cos_u)) * yd),
        })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (o, xd, yd) = frame(&self.position);
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        let r = self.radius;
        Ok(ResD3 {
            point: pnt(o + (r * (cos_u + u * sin_u)) * xd + (r * (sin_u - u * cos_u)) * yd),
            d1: vec((r * u * cos_u) * xd + (r * u * sin_u) * yd),
            d2: vec((r * (cos_u - u * sin_u)) * xd + (r * (sin_u + u * cos_u)) * yd),
            d3: vec((r * (-2.0 * sin_u - u * cos_u)) * xd + (r * (2.0 * cos_u - u * sin_u)) * yd),
        })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        check_n(n)?;
        match n {
            1 => return Ok(self.eval_d1(u)?.d1),
            2 => return Ok(self.eval_d2(u)?.d2),
            3 => return Ok(self.eval_d3(u)?.d3),
            _ => {}
        }
        let (_, xd, yd) = frame(&self.position);
        let (sin_u, cos_u) = (libm::sin(u), libm::cos(u));
        let (sin_n, cos_n) = trig_shift(sin_u, cos_u, n);
        let (sin_nm1, cos_nm1) = trig_shift(sin_u, cos_u, n - 1);
        let fnn = cos_n + u * sin_n + f64::from(n) * sin_nm1;
        let gn = sin_n - u * cos_n - f64::from(n) * cos_nm1;
        Ok(vec((self.radius * fnn) * xd + (self.radius * gn) * yd))
    }
}

// ---------------------------------------------------------------------------------------------
// Geom2dEval_LogarithmicSpiralCurve
// ---------------------------------------------------------------------------------------------

/// `Geom2dEval_LogarithmicSpiralCurve`: `r = a e^(b t)`.
#[derive(Debug, Clone)]
pub struct LogarithmicSpiralCurve {
    position: Ax2d,
    scale: f64,
    growth_exponent: f64,
}

impl LogarithmicSpiralCurve {
    /// The constructor; `Err(Construction)` if `scale <= 0` or `growth_exponent <= 0`.
    pub fn new(position: Ax2d, scale: f64, growth_exponent: f64) -> Result<Self> {
        if scale <= 0.0 || growth_exponent <= 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Self { position, scale, growth_exponent })
    }
    /// `Position`.
    pub fn position(&self) -> &Ax2d {
        &self.position
    }
    /// `Scale`.
    pub fn scale(&self) -> f64 {
        self.scale
    }
    /// `GrowthExponent`.
    pub fn growth_exponent(&self) -> f64 {
        self.growth_exponent
    }
}

/// `powComplexInt`: `(re + i im)^exp` by squaring.
fn pow_complex_int(base_re: f64, base_im: f64, exp: i32) -> (f64, f64) {
    let (mut res_re, mut res_im) = (1.0, 0.0);
    let (mut pow_re, mut pow_im) = (base_re, base_im);
    let mut e = exp;
    while e > 0 {
        if (e & 1) != 0 {
            let tmp_re = res_re * pow_re - res_im * pow_im;
            let tmp_im = res_re * pow_im + res_im * pow_re;
            res_re = tmp_re;
            res_im = tmp_im;
        }
        let next_re = pow_re * pow_re - pow_im * pow_im;
        let next_im = 2.0 * pow_re * pow_im;
        pow_re = next_re;
        pow_im = next_im;
        e >>= 1;
    }
    (res_re, res_im)
}

impl Geometry2d for LogarithmicSpiralCurve {
    eval_geometry_common!();
}

impl Curve2d for LogarithmicSpiralCurve {
    eval_curve_common!();
    fn first_parameter(&self) -> f64 {
        -PRECISION_INFINITE
    }
    fn last_parameter(&self) -> f64 {
        PRECISION_INFINITE
    }
    fn is_closed(&self) -> bool {
        false
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        let (o, xd, yd) = frame(&self.position);
        let e = self.scale * libm::exp(self.growth_exponent * u);
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(pnt(o + (e * cos_u) * xd + (e * sin_u) * yd))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (o, xd, yd) = frame(&self.position);
        let e = self.scale * libm::exp(self.growth_exponent * u);
        let b = self.growth_exponent;
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(ResD1 {
            point: pnt(o + (e * cos_u) * xd + (e * sin_u) * yd),
            d1: vec((e * (b * cos_u - sin_u)) * xd + (e * (b * sin_u + cos_u)) * yd),
        })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (o, xd, yd) = frame(&self.position);
        let e = self.scale * libm::exp(self.growth_exponent * u);
        let b = self.growth_exponent;
        let b2 = b * b;
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(ResD2 {
            point: pnt(o + (e * cos_u) * xd + (e * sin_u) * yd),
            d1: vec((e * (b * cos_u - sin_u)) * xd + (e * (b * sin_u + cos_u)) * yd),
            d2: vec(
                (e * ((b2 - 1.0) * cos_u - 2.0 * b * sin_u)) * xd + (e * ((b2 - 1.0) * sin_u + 2.0 * b * cos_u)) * yd
            ),
        })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (o, xd, yd) = frame(&self.position);
        let e = self.scale * libm::exp(self.growth_exponent * u);
        let b = self.growth_exponent;
        let b2 = b * b;
        let b3 = b2 * b;
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        Ok(ResD3 {
            point: pnt(o + (e * cos_u) * xd + (e * sin_u) * yd),
            d1: vec((e * (b * cos_u - sin_u)) * xd + (e * (b * sin_u + cos_u)) * yd),
            d2: vec(
                (e * ((b2 - 1.0) * cos_u - 2.0 * b * sin_u)) * xd + (e * ((b2 - 1.0) * sin_u + 2.0 * b * cos_u)) * yd
            ),
            d3: vec((e * ((b3 - 3.0 * b) * cos_u - (3.0 * b2 - 1.0) * sin_u)) * xd
                + (e * ((b3 - 3.0 * b) * sin_u + (3.0 * b2 - 1.0) * cos_u)) * yd),
        })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        check_n(n)?;
        match n {
            1 => return Ok(self.eval_d1(u)?.d1),
            2 => return Ok(self.eval_d2(u)?.d2),
            3 => return Ok(self.eval_d3(u)?.d3),
            _ => {}
        }
        let (_, xd, yd) = frame(&self.position);
        let b = self.growth_exponent;
        let scale = self.scale * libm::exp(b * u);
        let (cos_u, sin_u) = (libm::cos(u), libm::sin(u));
        // d^N[a e^(bt) (cos t + i sin t)] = a e^(bt) (b + i)^N (cos t + i sin t)
        let (pow_re, pow_im) = pow_complex_int(b, 1.0, n);
        let cx = pow_re * cos_u - pow_im * sin_u;
        let cy = pow_re * sin_u + pow_im * cos_u;
        Ok(vec((scale * cx) * xd + (scale * cy) * yd))
    }
}

// ---------------------------------------------------------------------------------------------
// Geom2dEval_SineWaveCurve
// ---------------------------------------------------------------------------------------------

/// `Geom2dEval_SineWaveCurve`: `(t, A sin(omega t + phase))` in the local frame.
#[derive(Debug, Clone)]
pub struct SineWaveCurve {
    position: Ax2d,
    amplitude: f64,
    omega: f64,
    phase: f64,
}

impl SineWaveCurve {
    /// The constructor; `Err(Construction)` if `amplitude <= 0` or `omega <= 0`.
    pub fn new(position: Ax2d, amplitude: f64, omega: f64, phase: f64) -> Result<Self> {
        if amplitude <= 0.0 || omega <= 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Self { position, amplitude, omega, phase })
    }
    /// `Position`.
    pub fn position(&self) -> &Ax2d {
        &self.position
    }
    /// `Amplitude`.
    pub fn amplitude(&self) -> f64 {
        self.amplitude
    }
    /// `Omega`.
    pub fn omega(&self) -> f64 {
        self.omega
    }
    /// `Phase`.
    pub fn phase(&self) -> f64 {
        self.phase
    }
}

impl Geometry2d for SineWaveCurve {
    eval_geometry_common!();
}

impl Curve2d for SineWaveCurve {
    eval_curve_common!();
    fn first_parameter(&self) -> f64 {
        -PRECISION_INFINITE
    }
    fn last_parameter(&self) -> f64 {
        PRECISION_INFINITE
    }
    fn is_closed(&self) -> bool {
        false
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        let (o, xd, yd) = frame(&self.position);
        let arg = self.omega * u + self.phase;
        Ok(pnt(o + u * xd + (self.amplitude * libm::sin(arg)) * yd))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (o, xd, yd) = frame(&self.position);
        let arg = self.omega * u + self.phase;
        Ok(ResD1 {
            point: pnt(o + u * xd + (self.amplitude * libm::sin(arg)) * yd),
            d1: vec(xd + (self.amplitude * self.omega * libm::cos(arg)) * yd),
        })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (o, xd, yd) = frame(&self.position);
        let arg = self.omega * u + self.phase;
        let om2 = self.omega * self.omega;
        Ok(ResD2 {
            point: pnt(o + u * xd + (self.amplitude * libm::sin(arg)) * yd),
            d1: vec(xd + (self.amplitude * self.omega * libm::cos(arg)) * yd),
            d2: vec((self.amplitude * (-om2) * libm::sin(arg)) * yd),
        })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (o, xd, yd) = frame(&self.position);
        let arg = self.omega * u + self.phase;
        let om2 = self.omega * self.omega;
        let om3 = om2 * self.omega;
        Ok(ResD3 {
            point: pnt(o + u * xd + (self.amplitude * libm::sin(arg)) * yd),
            d1: vec(xd + (self.amplitude * self.omega * libm::cos(arg)) * yd),
            d2: vec((self.amplitude * (-om2) * libm::sin(arg)) * yd),
            d3: vec((self.amplitude * (-om3) * libm::cos(arg)) * yd),
        })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        check_n(n)?;
        let (_, xd, yd) = frame(&self.position);
        let arg = self.omega * u + self.phase;
        let (sin, cos) = (libm::sin(arg), libm::cos(arg));
        let om_n = pow_int(self.omega, n);
        let sin_n = match n & 3 {
            0 => sin,
            1 => cos,
            2 => -sin,
            _ => -cos,
        };
        let mut r = (self.amplitude * om_n * sin_n) * yd;
        if n == 1 {
            r += xd;
        }
        Ok(vec(r))
    }
}

// ---------------------------------------------------------------------------------------------
// Geom2dEval_TBezierCurve
// ---------------------------------------------------------------------------------------------

/// The numerators `N_k` and the weight sums `W_k` of the order `k` derivative.
#[derive(Debug, Clone, Copy, Default)]
struct BasisEval {
    n: [Xy; 4],
    w: [f64; 4],
}

fn trig_pair_nth_deriv(sin: f64, cos: f64, order_mod4: i32) -> (f64, f64) {
    match order_mod4 {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

/// `Geom2dEval_TBezierCurve`: a Bezier curve in the trigonometric basis `1, sin(k a t), cos(k a t)` on `[0, pi / a]`.
#[derive(Debug, Clone)]
pub struct TBezierCurve {
    poles: Vec<Pnt2d>,
    weights: Vec<f64>,
    alpha: f64,
    rational: bool,
}

impl TBezierCurve {
    /// The non-rational constructor; `Err(Construction)` unless the number of poles is odd and at least 3 and `alpha > 0`.
    pub fn new(poles: &[Pnt2d], alpha: f64) -> Result<Self> {
        let n = poles.len();
        if n < 3 || n.is_multiple_of(2) || alpha <= 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Self { poles: poles.to_vec(), weights: Vec::new(), alpha, rational: false })
    }

    /// The rational constructor; the weights must be as many as the poles and all positive.
    pub fn with_weights(poles: &[Pnt2d], weights: &[f64], alpha: f64) -> Result<Self> {
        let n = poles.len();
        if n < 3 || n.is_multiple_of(2) || alpha <= 0.0 || weights.len() != n || weights.iter().any(|&w| w <= 0.0) {
            return Err(Geom2dError::Construction);
        }
        Ok(Self { poles: poles.to_vec(), weights: weights.to_vec(), alpha, rational: true })
    }

    /// `Poles`.
    pub fn poles(&self) -> &[Pnt2d] {
        &self.poles
    }
    /// `Weights` (empty if the curve is not rational).
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
    /// `Alpha`.
    pub fn alpha(&self) -> f64 {
        self.alpha
    }
    /// `NbPoles`.
    pub fn nb_poles(&self) -> i32 {
        self.poles.len() as i32
    }
    /// `Order`.
    pub fn order(&self) -> i32 {
        (self.poles.len() as i32 - 1) / 2
    }
    /// `IsRational`.
    pub fn is_rational(&self) -> bool {
        self.rational
    }

    fn pole(&self, i: usize) -> Result<Xy> {
        self.poles.get(i).map(Xy::of).ok_or(Geom2dError::OutOfRange)
    }
    fn weight(&self, i: usize) -> Result<f64> {
        self.weights.get(i).copied().ok_or(Geom2dError::OutOfRange)
    }

    // evalCurveBasis<theMaxOrder>
    fn eval_curve_basis(&self, max_order: i32, rational: bool, u: f64) -> Result<BasisEval> {
        let mut data = BasisEval::default();
        let pole0 = self.pole(0)?;
        if !rational {
            data.n[0] += pole0;
        } else {
            let w0 = self.weight(0)?;
            data.n[0] += pole0 * w0;
            data.w[0] += w0;
        }
        let order = self.order();
        if order <= 0 {
            return Ok(data);
        }
        let theta = self.alpha * u;
        let (sin1, cos1) = (libm::sin(theta), libm::cos(theta));
        let (mut sin_k, mut cos_k) = (sin1, cos1);
        let mut freq = self.alpha;
        let mut idx = 1usize;
        for k in 1..=order {
            let (sin, cos) = (sin_k, cos_k);
            let freq2 = freq * freq;
            let freq3 = freq2 * freq;
            let pole_sin = self.pole(idx)?;
            let pole_cos = self.pole(idx + 1)?;
            let b0 = (sin, cos);
            let mut b = [b0, (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
            if max_order >= 1 {
                b[1] = (freq * cos, -freq * sin);
            }
            if max_order >= 2 {
                b[2] = (-freq2 * sin, -freq2 * cos);
            }
            if max_order >= 3 {
                b[3] = (-freq3 * cos, freq3 * sin);
            }
            for (d, bd) in b.iter().enumerate().take(max_order as usize + 1) {
                if !rational {
                    if let Some(nd) = data.n.get_mut(d) {
                        *nd += pole_sin * bd.0 + pole_cos * bd.1;
                    }
                } else {
                    let (w_sin, w_cos) = (self.weight(idx)?, self.weight(idx + 1)?);
                    if let Some(nd) = data.n.get_mut(d) {
                        *nd += pole_sin * (w_sin * bd.0) + pole_cos * (w_cos * bd.1);
                    }
                    if let Some(wd) = data.w.get_mut(d) {
                        *wd += w_sin * bd.0 + w_cos * bd.1;
                    }
                }
            }
            if k < order {
                let sin_next = sin_k * cos1 + cos_k * sin1;
                let cos_next = cos_k * cos1 - sin_k * sin1;
                sin_k = sin_next;
                cos_k = cos_next;
            }
            idx += 2;
            freq += self.alpha;
        }
        Ok(data)
    }

    // evalBasis
    fn eval_basis(&self, t: f64) -> Vec<f64> {
        let mut basis = vec![0.0; self.poles.len()];
        if let Some(b) = basis.first_mut() {
            *b = 1.0;
        }
        let order = self.order();
        if order <= 0 {
            return basis;
        }
        let theta = self.alpha * t;
        let (sin1, cos1) = (libm::sin(theta), libm::cos(theta));
        let (mut sin_k, mut cos_k) = (sin1, cos1);
        for k in 1..=order as usize {
            if let Some(b) = basis.get_mut(2 * k - 1) {
                *b = sin_k;
            }
            if let Some(b) = basis.get_mut(2 * k) {
                *b = cos_k;
            }
            if k < order as usize {
                let sin_next = sin_k * cos1 + cos_k * sin1;
                let cos_next = cos_k * cos1 - sin_k * sin1;
                sin_k = sin_next;
                cos_k = cos_next;
            }
        }
        basis
    }

    // evalBasisDeriv
    fn eval_basis_deriv(&self, t: f64, deriv_order: i32) -> Vec<f64> {
        let mut basis = vec![0.0; self.poles.len()];
        let order = self.order();
        if order <= 0 {
            return basis;
        }
        let order_mod4 = deriv_order & 3;
        let theta = self.alpha * t;
        let (sin1, cos1) = (libm::sin(theta), libm::cos(theta));
        let (mut sin_k, mut cos_k) = (sin1, cos1);
        let mut freq = self.alpha;
        for k in 1..=order as usize {
            let freq_pow = pow_int(freq, deriv_order);
            let (sd, cd) = trig_pair_nth_deriv(sin_k, cos_k, order_mod4);
            if let Some(b) = basis.get_mut(2 * k - 1) {
                *b = freq_pow * sd;
            }
            if let Some(b) = basis.get_mut(2 * k) {
                *b = freq_pow * cd;
            }
            freq += self.alpha;
            if k < order as usize {
                let sin_next = sin_k * cos1 + cos_k * sin1;
                let cos_next = cos_k * cos1 - sin_k * sin1;
                sin_k = sin_next;
                cos_k = cos_next;
            }
        }
        basis
    }
}

impl Geometry2d for TBezierCurve {
    eval_geometry_common!();
}

impl BoundedCurve2d for TBezierCurve {
    fn start_point(&self) -> Pnt2d {
        self.eval_d0(0.0).unwrap_or_else(|_| Pnt2d::new(0.0, 0.0))
    }
    fn end_point(&self) -> Pnt2d {
        self.eval_d0(std::f64::consts::PI / self.alpha).unwrap_or_else(|_| Pnt2d::new(0.0, 0.0))
    }
}

impl Curve2d for TBezierCurve {
    eval_curve_common!();
    fn first_parameter(&self) -> f64 {
        0.0
    }
    fn last_parameter(&self) -> f64 {
        std::f64::consts::PI / self.alpha
    }
    fn is_closed(&self) -> bool {
        self.start_point().distance(&self.end_point()) <= confusion()
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        let d = self.eval_curve_basis(0, self.rational, u)?;
        if !self.rational {
            return Ok(pnt(d.n[0]));
        }
        Ok(pnt(d.n[0] / d.w[0]))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let d = self.eval_curve_basis(1, self.rational, u)?;
        if !self.rational {
            return Ok(ResD1 { point: pnt(d.n[0]), d1: vec(d.n[1]) });
        }
        let c = d.n[0] / d.w[0];
        Ok(ResD1 { point: pnt(c), d1: vec((d.n[1] - d.w[1] * c) / d.w[0]) })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let d = self.eval_curve_basis(2, self.rational, u)?;
        if !self.rational {
            return Ok(ResD2 { point: pnt(d.n[0]), d1: vec(d.n[1]), d2: vec(d.n[2]) });
        }
        let c = d.n[0] / d.w[0];
        let cd1 = (d.n[1] - d.w[1] * c) / d.w[0];
        let cd2 = (d.n[2] - 2.0 * d.w[1] * cd1 - d.w[2] * c) / d.w[0];
        Ok(ResD2 { point: pnt(c), d1: vec(cd1), d2: vec(cd2) })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let d = self.eval_curve_basis(3, self.rational, u)?;
        if !self.rational {
            return Ok(ResD3 { point: pnt(d.n[0]), d1: vec(d.n[1]), d2: vec(d.n[2]), d3: vec(d.n[3]) });
        }
        let c = d.n[0] / d.w[0];
        let cd1 = (d.n[1] - d.w[1] * c) / d.w[0];
        let cd2 = (d.n[2] - 2.0 * d.w[1] * cd1 - d.w[2] * c) / d.w[0];
        let cd3 = (d.n[3] - 3.0 * d.w[2] * cd1 - 3.0 * d.w[1] * cd2 - d.w[3] * c) / d.w[0];
        Ok(ResD3 { point: pnt(c), d1: vec(cd1), d2: vec(cd2), d3: vec(cd3) })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        check_n(n)?;
        if !self.rational {
            let mut sum = Xy::ZERO;
            let order = self.order();
            let order_mod4 = n & 3;
            if order <= 0 {
                return Ok(vec(sum));
            }
            let theta = self.alpha * u;
            let (sin1, cos1) = (libm::sin(theta), libm::cos(theta));
            let (mut sin_k, mut cos_k) = (sin1, cos1);
            let mut freq = self.alpha;
            let mut idx = 1usize;
            for k in 1..=order {
                let freq_pow = pow_int(freq, n);
                let (sd, cd) = trig_pair_nth_deriv(sin_k, cos_k, order_mod4);
                sum += self.pole(idx)? * (freq_pow * sd);
                sum += self.pole(idx + 1)? * (freq_pow * cd);
                if k < order {
                    let sin_next = sin_k * cos1 + cos_k * sin1;
                    let cos_next = cos_k * cos1 - sin_k * sin1;
                    sin_k = sin_next;
                    cos_k = cos_next;
                }
                idx += 2;
                freq += self.alpha;
            }
            return Ok(vec(sum));
        }
        // rational curves: the low orders from the full evaluations
        match n {
            1 => return Ok(self.eval_d1(u)?.d1),
            2 => return Ok(self.eval_d2(u)?.d2),
            3 => return Ok(self.eval_d3(u)?.d3),
            _ => {}
        }
        // the recurrence C^(n) = (Cw^(n) - sum_{k=1..n} C(n,k) w^(k) C^(n-k)) / w
        let nu = usize::try_from(n).map_err(|_| Geom2dError::Range)?;
        let mut basis_derivs: Vec<Vec<f64>> = Vec::with_capacity(nu + 1);
        for d in 0..=n {
            basis_derivs.push(if d == 0 { self.eval_basis(u) } else { self.eval_basis_deriv(u, d) });
        }
        let mut w_derivs = vec![0.0; nu + 1];
        let mut cw_derivs = vec![Xy::ZERO; nu + 1];
        for (d, basis) in basis_derivs.iter().enumerate() {
            let mut wd = 0.0;
            let mut cwd = Xy::ZERO;
            for (i, bd) in basis.iter().enumerate() {
                let wi = self.weight(i)?;
                let pi = self.pole(i)?;
                cwd += (wi * bd) * pi;
                wd += wi * bd;
            }
            if let Some(x) = w_derivs.get_mut(d) {
                *x = wd;
            }
            if let Some(x) = cw_derivs.get_mut(d) {
                *x = cwd;
            }
        }
        let at = |v: &[f64], i: usize| v.get(i).copied().ok_or(Geom2dError::OutOfRange);
        let at_xy = |v: &[Xy], i: usize| v.get(i).copied().ok_or(Geom2dError::OutOfRange);
        let mut c_derivs = vec![Xy::ZERO; nu + 1];
        let w0 = at(&w_derivs, 0)?;
        if let Some(x) = c_derivs.get_mut(0) {
            *x = at_xy(&cw_derivs, 0)? / w0;
        }
        for d in 1..=nu {
            let mut sum = Xy::ZERO;
            let mut binom = 1.0;
            for k in 1..=d {
                binom *= (d - k + 1) as f64 / k as f64;
                sum += (binom * at(&w_derivs, k)?) * at_xy(&c_derivs, d - k)?;
            }
            if let Some(x) = c_derivs.get_mut(d) {
                *x = (at_xy(&cw_derivs, d)? - sum) / w0;
            }
        }
        Ok(vec(at_xy(&c_derivs, nu)?))
    }
}

// ---------------------------------------------------------------------------------------------
// Geom2dEval_AHTBezierCurve
// ---------------------------------------------------------------------------------------------

/// `PolyEvalData`: the numerators and weight sums of the derivatives of order 0..3.
type PolyEval = BasisEval;

/// `fallingFactorial(n, k) = n (n - 1) ... (n - k + 1)`.
fn falling_factorial(n: i32, k: i32) -> f64 {
    let mut r = 1.0;
    for i in 0..k {
        r *= f64::from(n - i);
    }
    r
}

/// `Geom2dEval_AHTBezierCurve`: a Bezier curve in the algebraic-hyperbolic-trigonometric basis
/// `1, t, ..., t^k, sinh(a t), cosh(a t), sin(b t), cos(b t)` on `[0, 1]` (the hyperbolic / trigonometric pairs only when
/// `alpha` / `beta` is positive).
#[derive(Debug, Clone)]
pub struct AhtBezierCurve {
    poles: Vec<Pnt2d>,
    weights: Vec<f64>,
    alg_degree: i32,
    alpha: f64,
    beta: f64,
    rational: bool,
}

impl AhtBezierCurve {
    fn basis_dimension(alg_degree: i32, alpha: f64, beta: f64) -> i32 {
        alg_degree + 1 + if alpha > 0.0 { 2 } else { 0 } + if beta > 0.0 { 2 } else { 0 }
    }

    fn check(poles: &[Pnt2d], alg_degree: i32, alpha: f64, beta: f64) -> Result<()> {
        if alg_degree < 0 || alpha < 0.0 || beta < 0.0 {
            return Err(Geom2dError::Construction);
        }
        if poles.len() as i32 != Self::basis_dimension(alg_degree, alpha, beta) {
            return Err(Geom2dError::Construction);
        }
        Ok(())
    }

    /// The non-rational constructor: `Err(Construction)` for a negative degree, `alpha` or `beta`, or a number of poles
    /// that is not the dimension of the basis.
    pub fn new(poles: &[Pnt2d], alg_degree: i32, alpha: f64, beta: f64) -> Result<Self> {
        Self::check(poles, alg_degree, alpha, beta)?;
        Ok(Self { poles: poles.to_vec(), weights: Vec::new(), alg_degree, alpha, beta, rational: false })
    }

    /// The rational constructor (as many positive weights as poles).
    pub fn with_weights(poles: &[Pnt2d], weights: &[f64], alg_degree: i32, alpha: f64, beta: f64) -> Result<Self> {
        Self::check(poles, alg_degree, alpha, beta)?;
        if weights.len() != poles.len() || weights.iter().any(|&w| w <= 0.0) {
            return Err(Geom2dError::Construction);
        }
        Ok(Self { poles: poles.to_vec(), weights: weights.to_vec(), alg_degree, alpha, beta, rational: true })
    }

    /// `Poles`.
    pub fn poles(&self) -> &[Pnt2d] {
        &self.poles
    }
    /// `Weights` (empty if the curve is not rational).
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
    /// `AlgDegree`.
    pub fn alg_degree(&self) -> i32 {
        self.alg_degree
    }
    /// `Alpha`.
    pub fn alpha(&self) -> f64 {
        self.alpha
    }
    /// `Beta`.
    pub fn beta(&self) -> f64 {
        self.beta
    }
    /// `NbPoles`.
    pub fn nb_poles(&self) -> i32 {
        self.poles.len() as i32
    }
    /// `IsRational`.
    pub fn is_rational(&self) -> bool {
        self.rational
    }

    fn pole(&self, i: usize) -> Result<Xy> {
        self.poles.get(i).map(Xy::of).ok_or(Geom2dError::OutOfRange)
    }
    fn weight(&self, i: usize) -> Result<f64> {
        self.weights.get(i).copied().ok_or(Geom2dError::OutOfRange)
    }

    // evalPolynomialNonRational<theMaxOrder> / evalPolynomialRational<theMaxOrder> (Horner with the derivatives)
    fn eval_polynomial(&self, max_order: i32, u: f64) -> Result<PolyEval> {
        let degree = usize::try_from(self.alg_degree).map_err(|_| Geom2dError::OutOfRange)?;
        let mut d = PolyEval::default();
        if !self.rational {
            d.n[0] = self.pole(degree)?;
            for k in (0..degree).rev() {
                if max_order >= 3 {
                    d.n[3] = d.n[3] * u + d.n[2] * 3.0;
                }
                if max_order >= 2 {
                    d.n[2] = d.n[2] * u + d.n[1] * 2.0;
                }
                if max_order >= 1 {
                    d.n[1] = d.n[1] * u + d.n[0];
                }
                d.n[0] = d.n[0] * u + self.pole(k)?;
            }
        } else {
            let w_lead = self.weight(degree)?;
            d.n[0] = self.pole(degree)? * w_lead;
            d.w[0] = w_lead;
            for k in (0..degree).rev() {
                if max_order >= 3 {
                    d.n[3] = d.n[3] * u + d.n[2] * 3.0;
                    d.w[3] = d.w[3] * u + d.w[2] * 3.0;
                }
                if max_order >= 2 {
                    d.n[2] = d.n[2] * u + d.n[1] * 2.0;
                    d.w[2] = d.w[2] * u + d.w[1] * 2.0;
                }
                if max_order >= 1 {
                    d.n[1] = d.n[1] * u + d.n[0];
                    d.w[1] = d.w[1] * u + d.w[0];
                }
                let wk = self.weight(k)?;
                d.n[0] = d.n[0] * u + self.pole(k)? * wk;
                d.w[0] = d.w[0] * u + wk;
            }
        }
        Ok(d)
    }

    // evalMixedAHT<theMaxOrder, theIsRational>
    fn eval_mixed(&self, max_order: i32, u: f64) -> Result<PolyEval> {
        let rational = self.rational;
        let mut d = PolyEval::default();
        let mut pole_idx = 0usize;
        let mut weight_idx = 0usize;
        let (mut pow, mut pow_m1, mut pow_m2, mut pow_m3) = (1.0, 0.0, 0.0, 0.0);
        for k in 0..=self.alg_degree {
            let pole = self.pole(pole_idx)?;
            pole_idx += 1;
            let b0 = pow;
            let mut b = [b0, 0.0, 0.0, 0.0];
            if max_order >= 1 {
                b[1] = if k >= 1 { f64::from(k) * pow_m1 } else { 0.0 };
            }
            if max_order >= 2 {
                b[2] = if k >= 2 { f64::from(k) * f64::from(k - 1) * pow_m2 } else { 0.0 };
            }
            if max_order >= 3 {
                b[3] = if k >= 3 { f64::from(k) * f64::from(k - 1) * f64::from(k - 2) * pow_m3 } else { 0.0 };
            }
            let wi = if rational {
                let w = self.weight(weight_idx)?;
                weight_idx += 1;
                Some(w)
            } else {
                None
            };
            for (i, bi) in b.iter().enumerate().take(max_order as usize + 1) {
                match wi {
                    Some(wi) => {
                        if let Some(n) = d.n.get_mut(i) {
                            *n += pole * (wi * bi);
                        }
                        if let Some(w) = d.w.get_mut(i) {
                            *w += wi * bi;
                        }
                    }
                    None => {
                        if let Some(n) = d.n.get_mut(i) {
                            *n += pole * *bi;
                        }
                    }
                }
            }
            pow_m3 = pow_m2;
            pow_m2 = pow_m1;
            pow_m1 = pow;
            pow *= u;
        }
        if self.alpha > 0.0 {
            let alpha_u = self.alpha * u;
            let (sinh, cosh) = (libm::sinh(alpha_u), libm::cosh(alpha_u));
            let alpha2 = self.alpha * self.alpha;
            let alpha3 = alpha2 * self.alpha;
            let pole_sinh = self.pole(pole_idx)?;
            pole_idx += 1;
            let pole_cosh = self.pole(pole_idx)?;
            pole_idx += 1;
            let (mut wi_sinh, mut wi_cosh) = (1.0, 1.0);
            if rational {
                wi_sinh = self.weight(weight_idx)?;
                weight_idx += 1;
                wi_cosh = self.weight(weight_idx)?;
                weight_idx += 1;
            }
            let mut bs = [(sinh, cosh), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
            if max_order >= 1 {
                bs[1] = (self.alpha * cosh, self.alpha * sinh);
            }
            if max_order >= 2 {
                bs[2] = (alpha2 * sinh, alpha2 * cosh);
            }
            if max_order >= 3 {
                bs[3] = (alpha3 * cosh, alpha3 * sinh);
            }
            for (i, bi) in bs.iter().enumerate().take(max_order as usize + 1) {
                if let Some(n) = d.n.get_mut(i) {
                    *n += pole_sinh * (wi_sinh * bi.0) + pole_cosh * (wi_cosh * bi.1);
                }
                if rational && let Some(w) = d.w.get_mut(i) {
                    *w += wi_sinh * bi.0 + wi_cosh * bi.1;
                }
            }
        }
        if self.beta > 0.0 {
            let beta_u = self.beta * u;
            let (sin, cos) = (libm::sin(beta_u), libm::cos(beta_u));
            let beta2 = self.beta * self.beta;
            let beta3 = beta2 * self.beta;
            let pole_sin = self.pole(pole_idx)?;
            pole_idx += 1;
            let pole_cos = self.pole(pole_idx)?;
            let (mut wi_sin, mut wi_cos) = (1.0, 1.0);
            if rational {
                wi_sin = self.weight(weight_idx)?;
                weight_idx += 1;
                wi_cos = self.weight(weight_idx)?;
            }
            let mut bs = [(sin, cos), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
            if max_order >= 1 {
                bs[1] = (self.beta * cos, -self.beta * sin);
            }
            if max_order >= 2 {
                bs[2] = (-beta2 * sin, -beta2 * cos);
            }
            if max_order >= 3 {
                bs[3] = (-beta3 * cos, beta3 * sin);
            }
            for (i, bi) in bs.iter().enumerate().take(max_order as usize + 1) {
                if let Some(n) = d.n.get_mut(i) {
                    *n += pole_sin * (wi_sin * bi.0) + pole_cos * (wi_cos * bi.1);
                }
                if rational && let Some(w) = d.w.get_mut(i) {
                    *w += wi_sin * bi.0 + wi_cos * bi.1;
                }
            }
        }
        Ok(d)
    }

    fn eval_data(&self, max_order: i32, u: f64) -> Result<PolyEval> {
        if self.alpha <= 0.0 && self.beta <= 0.0 {
            self.eval_polynomial(max_order, u)
        } else {
            self.eval_mixed(max_order, u)
        }
    }
}

impl Geometry2d for AhtBezierCurve {
    eval_geometry_common!();
}

impl BoundedCurve2d for AhtBezierCurve {
    fn start_point(&self) -> Pnt2d {
        self.eval_d0(0.0).unwrap_or_else(|_| Pnt2d::new(0.0, 0.0))
    }
    fn end_point(&self) -> Pnt2d {
        self.eval_d0(1.0).unwrap_or_else(|_| Pnt2d::new(0.0, 0.0))
    }
}

impl Curve2d for AhtBezierCurve {
    eval_curve_common!();
    fn first_parameter(&self) -> f64 {
        0.0
    }
    fn last_parameter(&self) -> f64 {
        1.0
    }
    fn is_closed(&self) -> bool {
        self.start_point().distance(&self.end_point()) <= confusion()
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        let d = self.eval_data(0, u)?;
        if !self.rational {
            return Ok(pnt(d.n[0]));
        }
        Ok(pnt(d.n[0] / d.w[0]))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let d = self.eval_data(1, u)?;
        if !self.rational {
            return Ok(ResD1 { point: pnt(d.n[0]), d1: vec(d.n[1]) });
        }
        let inv_w = 1.0 / d.w[0];
        Ok(ResD1 { point: pnt(d.n[0] * inv_w), d1: vec((d.n[1] * d.w[0] - d.n[0] * d.w[1]) * (inv_w * inv_w)) })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let d = self.eval_data(2, u)?;
        if !self.rational {
            return Ok(ResD2 { point: pnt(d.n[0]), d1: vec(d.n[1]), d2: vec(d.n[2]) });
        }
        let inv_w = 1.0 / d.w[0];
        let c = d.n[0] * inv_w;
        let cd1 = (d.n[1] * d.w[0] - d.n[0] * d.w[1]) * (inv_w * inv_w);
        Ok(ResD2 { point: pnt(c), d1: vec(cd1), d2: vec((d.n[2] - cd1 * (2.0 * d.w[1]) - c * d.w[2]) * inv_w) })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let d = self.eval_data(3, u)?;
        if !self.rational {
            return Ok(ResD3 { point: pnt(d.n[0]), d1: vec(d.n[1]), d2: vec(d.n[2]), d3: vec(d.n[3]) });
        }
        let inv_w = 1.0 / d.w[0];
        let c = d.n[0] * inv_w;
        let cd1 = (d.n[1] - c * d.w[1]) * inv_w;
        let cd2 = (d.n[2] - cd1 * (2.0 * d.w[1]) - c * d.w[2]) * inv_w;
        let cd3 = (d.n[3] - cd2 * (3.0 * d.w[1]) - cd1 * (3.0 * d.w[2]) - c * d.w[3]) * inv_w;
        Ok(ResD3 { point: pnt(c), d1: vec(cd1), d2: vec(cd2), d3: vec(cd3) })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        check_n(n)?;
        match n {
            1 => return Ok(self.eval_d1(u)?.d1),
            2 => return Ok(self.eval_d2(u)?.d2),
            3 => return Ok(self.eval_d3(u)?.d3),
            _ => {}
        }
        // the N-th derivative of the algebraic basis function t^k
        let alg_deriv = |k: i32, d: i32| -> f64 {
            if d <= k {
                if u == 0.0 {
                    if d == k { falling_factorial(k, d) } else { 0.0 }
                } else {
                    falling_factorial(k, d) * pow_int(u, k - d)
                }
            } else {
                0.0
            }
        };
        let trig = |sin: f64, cos: f64, d: i32| -> (f64, f64) {
            match d & 3 {
                0 => (sin, cos),
                1 => (cos, -sin),
                2 => (-sin, -cos),
                _ => (-cos, sin),
            }
        };
        if !self.rational {
            let mut pole_idx = 0usize;
            let mut sum = Xy::ZERO;
            for k in 0..=self.alg_degree {
                sum += self.pole(pole_idx)? * alg_deriv(k, n);
                pole_idx += 1;
            }
            if self.alpha > 0.0 {
                let alpha_u = self.alpha * u;
                let alpha_pow = pow_int(self.alpha, n);
                let (sinh, cosh) = (libm::sinh(alpha_u), libm::cosh(alpha_u));
                let even = n % 2 == 0;
                let sinh_n = alpha_pow * if even { sinh } else { cosh };
                let cosh_n = alpha_pow * if even { cosh } else { sinh };
                sum += self.pole(pole_idx)? * sinh_n;
                pole_idx += 1;
                sum += self.pole(pole_idx)? * cosh_n;
                pole_idx += 1;
            }
            if self.beta > 0.0 {
                let beta_u = self.beta * u;
                let beta_pow = pow_int(self.beta, n);
                let (sin_n, cos_n) = trig(libm::sin(beta_u), libm::cos(beta_u), n);
                sum += self.pole(pole_idx)? * (beta_pow * sin_n);
                pole_idx += 1;
                sum += self.pole(pole_idx)? * (beta_pow * cos_n);
            }
            return Ok(vec(sum));
        }
        let nu = usize::try_from(n).map_err(|_| Geom2dError::Range)?;
        let poly_count = (self.alg_degree + 1) as usize;
        let hyper_idx = poly_count;
        let trig_idx = hyper_idx + if self.alpha > 0.0 { 2 } else { 0 };
        let (mut pole_sinh, mut pole_cosh, mut pole_sin, mut pole_cos) = (Xy::ZERO, Xy::ZERO, Xy::ZERO, Xy::ZERO);
        let (mut wi_sinh, mut wi_cosh, mut wi_sin, mut wi_cos) = (0.0, 0.0, 0.0, 0.0);
        if self.alpha > 0.0 {
            pole_sinh = self.pole(hyper_idx)?;
            pole_cosh = self.pole(hyper_idx + 1)?;
            wi_sinh = self.weight(poly_count)?;
            wi_cosh = self.weight(poly_count + 1)?;
        }
        if self.beta > 0.0 {
            let trig_w_idx = poly_count + if self.alpha > 0.0 { 2 } else { 0 };
            pole_sin = self.pole(trig_idx)?;
            pole_cos = self.pole(trig_idx + 1)?;
            wi_sin = self.weight(trig_w_idx)?;
            wi_cos = self.weight(trig_w_idx + 1)?;
        }
        let alpha_u = self.alpha * u;
        let (sinh, cosh) = if self.alpha > 0.0 { (libm::sinh(alpha_u), libm::cosh(alpha_u)) } else { (0.0, 0.0) };
        let beta_u = self.beta * u;
        let (sin, cos) = if self.beta > 0.0 { (libm::sin(beta_u), libm::cos(beta_u)) } else { (0.0, 0.0) };
        let (mut alpha_pow, mut beta_pow) = (1.0, 1.0);
        let mut w_derivs = vec![0.0; nu + 1];
        let mut c_derivs = vec![Xy::ZERO; nu + 1];
        let mut inv_w0 = 0.0;
        for d in 0..=n {
            let mut nd = Xy::ZERO;
            let mut wd = 0.0;
            for k in 0..=self.alg_degree {
                let pole = self.pole(k as usize)?;
                let wi = self.weight(k as usize)?;
                let basis_d = alg_deriv(k, d);
                nd += pole * (wi * basis_d);
                wd += wi * basis_d;
            }
            if self.alpha > 0.0 {
                let even = d % 2 == 0;
                let dsinh = alpha_pow * if even { sinh } else { cosh };
                let dcosh = alpha_pow * if even { cosh } else { sinh };
                nd += pole_sinh * (wi_sinh * dsinh) + pole_cosh * (wi_cosh * dcosh);
                wd += wi_sinh * dsinh + wi_cosh * dcosh;
            }
            if self.beta > 0.0 {
                let (dsin, dcos) = trig(sin, cos, d);
                nd += pole_sin * (wi_sin * beta_pow * dsin) + pole_cos * (wi_cos * beta_pow * dcos);
                wd += wi_sin * beta_pow * dsin + wi_cos * beta_pow * dcos;
            }
            let du = d as usize;
            if let Some(x) = w_derivs.get_mut(du) {
                *x = wd;
            }
            if d == 0 {
                inv_w0 = 1.0 / wd;
                if let Some(x) = c_derivs.get_mut(0) {
                    *x = nd * inv_w0;
                }
            } else {
                let mut sum = nd;
                let mut binom = 1.0;
                for j in 1..=du {
                    binom = binom * (du - j + 1) as f64 / j as f64;
                    let cdj = c_derivs.get(du - j).copied().ok_or(Geom2dError::OutOfRange)?;
                    let wj = w_derivs.get(j).copied().ok_or(Geom2dError::OutOfRange)?;
                    sum -= cdj * (binom * wj);
                }
                if let Some(x) = c_derivs.get_mut(du) {
                    *x = sum * inv_w0;
                }
            }
            alpha_pow *= self.alpha;
            beta_pow *= self.beta;
        }
        Ok(vec(c_derivs.get(nu).copied().ok_or(Geom2dError::OutOfRange)?))
    }
}
