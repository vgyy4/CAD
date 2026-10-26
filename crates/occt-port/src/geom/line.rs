//! `Geom_Line`.
use super::{Curve, GeomAbsShape, GeomError, Geometry, PRECISION_INFINITE, ResD1, ResD2, ResD3, Result};
use super::{geometry_any, is_infinite};
use crate::elclib;
use crate::gp::{self, Ax1, Dir, Lin, Pnt, Trsf};

/// `Geom_Line`.
#[derive(Debug, Clone)]
pub struct Line {
    pos: Ax1,
}

impl Line {
    /// `Geom_Line(gp_Ax1)`.
    pub fn from_ax1(a: Ax1) -> Self {
        Line { pos: a }
    }
    /// `Geom_Line(gp_Lin)`.
    pub fn from_lin(l: Lin) -> Self {
        Line { pos: *l.position() }
    }
    /// `Geom_Line(P, V)`.
    pub fn new(p: Pnt, v: Dir) -> Self {
        Line { pos: Ax1::new(p, v) }
    }
    /// `SetLin`.
    pub fn set_lin(&mut self, l: Lin) {
        self.pos = *l.position();
    }
    /// `SetDirection`.
    pub fn set_direction(&mut self, v: Dir) {
        self.pos.set_direction(v);
    }
    /// `SetLocation`.
    pub fn set_location(&mut self, p: Pnt) {
        self.pos.set_location(p);
    }
    /// `SetPosition`.
    pub fn set_position(&mut self, a: Ax1) {
        self.pos = a;
    }
    /// `Lin`.
    pub fn lin(&self) -> Lin {
        Lin::from_axis(self.pos)
    }
    /// `Position`.
    pub fn position(&self) -> &Ax1 {
        &self.pos
    }
}

impl Geometry for Line {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(Line { pos: self.pos })
    }
}

impl Curve for Line {
    fn copy_curve(&self) -> Box<dyn Curve> {
        Box::new(Line { pos: self.pos })
    }
    fn reverse(&mut self) {
        self.pos.reverse();
    }
    fn reversed_parameter(&self, u: f64) -> f64 {
        -u
    }
    fn transformed_parameter(&self, u: f64, t: &Trsf) -> f64 {
        if is_infinite(u) {
            return u;
        }
        u * t.scale_factor().abs()
    }
    fn parametric_transformation(&self, t: &Trsf) -> f64 {
        t.scale_factor().abs()
    }
    fn first_parameter(&self) -> f64 {
        -PRECISION_INFINITE
    }
    fn last_parameter(&self) -> f64 {
        PRECISION_INFINITE
    }
    fn is_closed(&self) -> bool {
        false
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
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        Ok(elclib::line_value(u, &self.pos))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::line_d1(u, &self.pos);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1) = elclib::line_d1(u, &self.pos);
        Ok(ResD2 { point, d1, d2: gp::Vec::new(0.0, 0.0, 0.0) })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1) = elclib::line_d1(u, &self.pos);
        Ok(ResD3 { point, d1, d2: gp::Vec::new(0.0, 0.0, 0.0), d3: gp::Vec::new(0.0, 0.0, 0.0) })
    }
    fn eval_dn(&self, _u: f64, n: i32) -> Result<gp::Vec> {
        if n < 1 {
            return Err(GeomError::UndefinedDerivative);
        }
        if n == 1 { Ok(gp::Vec::from_dir(self.pos.direction())) } else { Ok(gp::Vec::new(0.0, 0.0, 0.0)) }
    }
}
