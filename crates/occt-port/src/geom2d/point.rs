//! `Geom2d_CartesianPoint`.
use super::{Geometry2d, Point2d, geometry_any};
use crate::gp::{Pnt2d, Trsf2d};

/// `Geom2d_CartesianPoint`.
#[derive(Debug, Clone)]
pub struct CartesianPoint {
    gp_pnt2d: Pnt2d,
}

impl CartesianPoint {
    /// `Geom2d_CartesianPoint(gp_Pnt2d)`.
    pub fn from_pnt2d(p: Pnt2d) -> Self {
        CartesianPoint { gp_pnt2d: p }
    }
    /// `Geom2d_CartesianPoint(X, Y)`.
    pub fn new(x: f64, y: f64) -> Self {
        CartesianPoint { gp_pnt2d: Pnt2d::new(x, y) }
    }
    /// `SetCoord`.
    pub fn set_coord(&mut self, x: f64, y: f64) {
        self.gp_pnt2d.set_coord(x, y);
    }
    /// `SetPnt2d`.
    pub fn set_pnt2d(&mut self, p: Pnt2d) {
        self.gp_pnt2d = p;
    }
    /// `SetX`.
    pub fn set_x(&mut self, x: f64) {
        self.gp_pnt2d.set_x(x);
    }
    /// `SetY`.
    pub fn set_y(&mut self, y: f64) {
        self.gp_pnt2d.set_y(y);
    }
}

impl Geometry2d for CartesianPoint {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.gp_pnt2d.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(CartesianPoint::from_pnt2d(self.gp_pnt2d))
    }
}

impl Point2d for CartesianPoint {
    fn coord(&self) -> (f64, f64) {
        self.gp_pnt2d.coords()
    }
    fn pnt2d(&self) -> Pnt2d {
        self.gp_pnt2d
    }
    fn x(&self) -> f64 {
        self.gp_pnt2d.x()
    }
    fn y(&self) -> f64 {
        self.gp_pnt2d.y()
    }
}
