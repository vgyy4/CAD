//! `Convert_ConicToBSplineCurve` with `Convert_CircleToBSplineCurve`, `Convert_EllipseToBSplineCurve`,
//! `Convert_HyperbolaToBSplineCurve` and `Convert_ParabolaToBSplineCurve`.

use super::cos_sin::CosAndSin;
use super::{ConvertError, PCONFUSION, ParameterisationType, R, at, cmax, cmin};
use crate::gp::{self, Ax2d, Circ2d, Elips2d, Hypr2d, Parab2d, Pnt2d, Trsf2d};
use std::f64::consts::PI;

/// The result of every conic converter: a B-spline curve (degree, periodicity, poles, weights, knots and
/// multiplicities). The C++ classes only differ by their constructors.
#[derive(Debug, Clone, PartialEq)]
pub struct ConicToBSplineCurve {
    degree: i32,
    periodic: bool,
    poles: Vec<Pnt2d>,
    weights: Vec<f64>,
    knots: Vec<f64>,
    mults: Vec<i32>,
}

/// The side of the conic: `1` for a direct frame (`Ox.X * Oy.Y - Ox.Y * Oy.X > 0`), `-1` otherwise.
fn frame_sign(x_axis: &Ax2d, y_axis: &Ax2d) -> f64 {
    let ox = x_axis.direction();
    let oy = y_axis.direction();
    if ox.x() * oy.y() - ox.y() * oy.x() > 0.0 { 1.0 } else { -1.0 }
}

/// `(UFirst, ULast)` check of `Convert_CircleToBSplineCurve` (a `Standard_DomainError_Raise_if` for the ellipse).
fn check_arc(u_first: f64, u_last: f64) -> R<f64> {
    let delta = u_last - u_first;
    if delta > (2.0 * PI + PCONFUSION) || delta <= 0.0 {
        return Err(ConvertError::Domain);
    }
    Ok(delta)
}

impl ConicToBSplineCurve {
    /// The poles of a circle or an ellipse from its cos / sin representation: `(a * cos, side * b * sin)` in the
    /// frame of the conic.
    fn from_cos_and_sin(cs: CosAndSin, periodic: bool, x_axis: &Ax2d, y_axis: &Ax2d, a: f64, b: f64) -> R<Self> {
        let mut trsf = Trsf2d::new();
        trsf.set_transformation_between(x_axis, &gp::ox2d());
        let value = if frame_sign(x_axis, y_axis) > 0.0 { b } else { -b };
        let mut poles = Vec::with_capacity(cs.cos_numerator.len());
        for (c, s) in cs.cos_numerator.iter().zip(&cs.sin_numerator) {
            let mut p = Pnt2d::new(a * *c, value * *s);
            p.transform(&trsf);
            poles.push(p);
        }
        Ok(Self { degree: cs.degree, periodic, poles, weights: cs.denominator, knots: cs.knots, mults: cs.mults })
    }

    fn full(a: f64, b: f64, x_axis: &Ax2d, y_axis: &Ax2d, p: ParameterisationType) -> R<Self> {
        let (periodic, cs) = if p != ParameterisationType::TgtThetaOver2 && p != ParameterisationType::RationalC1 {
            // In case BuildCosAndSin does not know how to manage the periodicity => trim on 0, 2*PI
            (false, Self::build_cos_and_sin(p, 0.0, 2.0 * PI)?)
        } else {
            (true, Self::build_cos_and_sin_periodic(p)?)
        };
        Self::from_cos_and_sin(cs, periodic, x_axis, y_axis, a, b)
    }

    /// `Convert_CircleToBSplineCurve(C, Parameterisation)`: the full circle (periodic for `TgtThetaOver2` and
    /// `RationalC1`, trimmed on `[0, 2*PI]` otherwise).
    pub fn circle(c: &Circ2d, p: ParameterisationType) -> R<Self> {
        let r = c.radius();
        Self::full(r, r, &c.x_axis(), &c.y_axis(), p)
    }

    /// `Convert_CircleToBSplineCurve(C, UFirst, ULast, Parameterisation)`.
    pub fn circle_trimmed(c: &Circ2d, u_first: f64, u_last: f64, p: ParameterisationType) -> R<Self> {
        check_arc(u_first, u_last)?;
        let r = c.radius();
        let cs = Self::build_cos_and_sin(p, u_first, u_last)?;
        Self::from_cos_and_sin(cs, false, &c.x_axis(), &c.y_axis(), r, r)
    }

    /// `Convert_EllipseToBSplineCurve(E, Parameterisation)`.
    pub fn ellipse(e: &Elips2d, p: ParameterisationType) -> R<Self> {
        Self::full(e.major_radius(), e.minor_radius(), &e.x_axis(), &e.y_axis(), p)
    }

    /// `Convert_EllipseToBSplineCurve(E, UFirst, ULast, Parameterisation)`. The arc check of OCCT 8.0.1 is compiled
    /// out: an arc that is not in `(0, 2*PI + PConfusion]` is `Err(Domain)` here.
    pub fn ellipse_trimmed(e: &Elips2d, u_first: f64, u_last: f64, p: ParameterisationType) -> R<Self> {
        check_arc(u_first, u_last)?;
        let cs = Self::build_cos_and_sin(p, u_first, u_last)?;
        Self::from_cos_and_sin(cs, false, &e.x_axis(), &e.y_axis(), e.major_radius(), e.minor_radius())
    }

    /// `Convert_HyperbolaToBSplineCurve(H, U1, U2)`. `Err(Argument)` for `U1 == U2` (unchecked in OCCT 8.0.1).
    pub fn hyperbola(h: &Hypr2d, u1: f64, u2: f64) -> R<Self> {
        if (u2 - u1).abs() < f64::from_bits(1) {
            return Err(ConvertError::Argument);
        }
        let uf = cmin(u1, u2);
        let ul = cmax(u1, u2);
        let big_r = h.major_radius();
        let r = h.minor_radius();
        let ox = h.axis().x_direction();
        let oy = h.axis().y_direction();
        let s = if ox.x() * oy.y() - ox.y() * oy.x() > 0.0 { 1.0 } else { -1.0 };
        // the 2nd pole is at the intersection of 2 tangents to the curve at points P(UF), P(UL);
        // its weight is cosh((UL-UF)/2)
        let weights = vec![1.0, libm::cosh((ul - uf) / 2.0), 1.0];
        let delta = libm::sinh(ul - uf);
        let x = big_r * (libm::sinh(ul) - libm::sinh(uf)) / delta;
        let y = s * r * (libm::cosh(ul) - libm::cosh(uf)) / delta;
        let mut poles = vec![
            Pnt2d::new(big_r * libm::cosh(uf), s * r * libm::sinh(uf)),
            Pnt2d::new(x, y),
            Pnt2d::new(big_r * libm::cosh(ul), s * r * libm::sinh(ul)),
        ];
        let mut trsf = Trsf2d::new();
        trsf.set_transformation_between(&h.axis().x_axis(), &gp::ox2d());
        for p in &mut poles {
            p.transform(&trsf);
        }
        Ok(Self { degree: 2, periodic: false, poles, weights, knots: vec![uf, ul], mults: vec![3, 3] })
    }

    /// `Convert_ParabolaToBSplineCurve(Prb, U1, U2)`. `Err(Argument)` for `U1 == U2` (unchecked in OCCT 8.0.1).
    pub fn parabola(prb: &Parab2d, u1: f64, u2: f64) -> R<Self> {
        if (u2 - u1).abs() < f64::from_bits(1) {
            return Err(ConvertError::Argument);
        }
        let uf = cmin(u1, u2);
        let ul = cmax(u1, u2);
        let p = prb.parameter();
        let ox = prb.axis().x_direction();
        let oy = prb.axis().y_direction();
        let s = if ox.x() * oy.y() - ox.y() * oy.x() > 0.0 { 1.0 } else { -1.0 };
        let mut poles = vec![
            Pnt2d::new((uf * uf) / (2.0 * p), s * uf),
            Pnt2d::new((uf * ul) / (2.0 * p), s * (uf + ul) / 2.0),
            Pnt2d::new((ul * ul) / (2.0 * p), s * ul),
        ];
        let mut trsf = Trsf2d::new();
        trsf.set_transformation_between(&prb.axis().x_axis(), &gp::ox2d());
        for q in &mut poles {
            q.transform(&trsf);
        }
        Ok(Self {
            degree: 2,
            periodic: false,
            poles,
            weights: vec![1.0, 1.0, 1.0],
            knots: vec![uf, ul],
            mults: vec![3, 3],
        })
    }

    /// The degree of the curve.
    pub fn degree(&self) -> i32 {
        self.degree
    }

    /// The number of poles.
    pub fn nb_poles(&self) -> i32 {
        i32::try_from(self.poles.len()).unwrap_or(i32::MAX)
    }

    /// The number of knots.
    pub fn nb_knots(&self) -> i32 {
        i32::try_from(self.knots.len()).unwrap_or(i32::MAX)
    }

    /// Whether the curve is periodic.
    pub fn is_periodic(&self) -> bool {
        self.periodic
    }

    /// The poles.
    pub fn poles(&self) -> &[Pnt2d] {
        &self.poles
    }

    /// The weights (one per pole).
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// The knots.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The multiplicities (one per knot).
    pub fn multiplicities(&self) -> &[i32] {
        &self.mults
    }

    /// `Pole(Index)`, 1-based; `Err(OutOfRange)` outside `1..=nb_poles`.
    pub fn pole(&self, index: i32) -> R<Pnt2d> {
        at(&self.poles, i64::from(index))
    }

    /// `Weight(Index)`, 1-based; `Err(OutOfRange)` outside `1..=nb_poles`.
    pub fn weight(&self, index: i32) -> R<f64> {
        at(&self.poles, i64::from(index))?;
        at(&self.weights, i64::from(index))
    }

    /// `Knot(Index)`, 1-based; `Err(OutOfRange)` outside `1..=nb_knots`.
    pub fn knot(&self, index: i32) -> R<f64> {
        at(&self.knots, i64::from(index))
    }

    /// `Multiplicity(Index)`, 1-based; `Err(OutOfRange)` outside `1..=nb_knots`.
    pub fn multiplicity(&self, index: i32) -> R<i32> {
        at(&self.knots, i64::from(index))?;
        at(&self.mults, i64::from(index))
    }
}
