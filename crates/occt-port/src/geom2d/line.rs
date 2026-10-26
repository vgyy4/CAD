//! `Geom2d_Line`.
use super::{Curve2d, Geom2dError, GeomAbsShape, Geometry2d, PRECISION_INFINITE, ResD1, ResD2, ResD3, Result};
use super::{geometry_any, is_infinite};
use crate::elclib;
use crate::gp::{Ax2d, Dir2d, Lin2d, Pnt2d, Trsf2d, Vec2d};

/// `Geom2d_Line`.
#[derive(Debug, Clone)]
pub struct Line {
    pos: Ax2d,
}

impl Line {
    /// `Geom2d_Line(gp_Ax2d)`.
    pub fn from_ax2d(a: Ax2d) -> Self {
        Line { pos: a }
    }
    /// `Geom2d_Line(gp_Lin2d)`.
    pub fn from_lin2d(l: Lin2d) -> Self {
        Line { pos: *l.position() }
    }
    /// `Geom2d_Line(P, V)`.
    pub fn new(p: Pnt2d, v: Dir2d) -> Self {
        Line { pos: Ax2d::new(p, v) }
    }
    /// `SetLin2d`.
    pub fn set_lin2d(&mut self, l: Lin2d) {
        self.pos = *l.position();
    }
    /// `SetDirection`.
    pub fn set_direction(&mut self, v: Dir2d) {
        self.pos.set_direction(v);
    }
    /// `Direction`.
    pub fn direction(&self) -> &Dir2d {
        self.pos.direction()
    }
    /// `SetLocation`.
    pub fn set_location(&mut self, p: Pnt2d) {
        self.pos.set_location(p);
    }
    /// `Location`.
    pub fn location(&self) -> &Pnt2d {
        self.pos.location()
    }
    /// `SetPosition`.
    pub fn set_position(&mut self, a: Ax2d) {
        self.pos = a;
    }
    /// `Position`.
    pub fn position(&self) -> &Ax2d {
        &self.pos
    }
    /// `Lin2d`.
    pub fn lin2d(&self) -> Lin2d {
        Lin2d::from_ax2d(self.pos)
    }
    /// `Distance`.
    pub fn distance(&self, p: &Pnt2d) -> f64 {
        let l = Lin2d::from_ax2d(self.pos);
        l.distance(p)
    }
}

impl Geometry2d for Line {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(Line { pos: self.pos })
    }
}

impl Curve2d for Line {
    fn copy_curve(&self) -> Box<dyn Curve2d> {
        Box::new(Line { pos: self.pos })
    }
    fn reverse(&mut self) {
        self.pos.reverse();
    }
    fn reversed_parameter(&self, u: f64) -> f64 {
        -u
    }
    fn transformed_parameter(&self, u: f64, t: &Trsf2d) -> f64 {
        if is_infinite(u) {
            return u;
        }
        u * t.scale_factor().abs()
    }
    fn parametric_transformation(&self, t: &Trsf2d) -> f64 {
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
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        Ok(elclib::line2d_value(u, &self.pos))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::line2d_d1(u, &self.pos);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1) = elclib::line2d_d1(u, &self.pos);
        Ok(ResD2 { point, d1, d2: Vec2d::new(0.0, 0.0) })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1) = elclib::line2d_d1(u, &self.pos);
        Ok(ResD3 { point, d1, d2: Vec2d::new(0.0, 0.0), d3: Vec2d::new(0.0, 0.0) })
    }
    fn eval_dn(&self, _u: f64, n: i32) -> Result<Vec2d> {
        if n < 1 {
            return Err(Geom2dError::UndefinedDerivative);
        }
        if n == 1 { Ok(Vec2d::from_dir(self.pos.direction())) } else { Ok(Vec2d::new(0.0, 0.0)) }
    }
}
