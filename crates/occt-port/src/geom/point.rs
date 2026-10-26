//! `Geom_CartesianPoint`.
use super::{Geometry, Point, geometry_any};
use crate::gp::{Pnt, Trsf};

/// `Geom_CartesianPoint`.
#[derive(Debug, Clone)]
pub struct CartesianPoint {
    gp_pnt: Pnt,
}

impl CartesianPoint {
    /// `Geom_CartesianPoint(gp_Pnt)`.
    pub fn from_pnt(p: Pnt) -> Self {
        CartesianPoint { gp_pnt: p }
    }
    /// `Geom_CartesianPoint(X, Y, Z)`.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        CartesianPoint { gp_pnt: Pnt::new(x, y, z) }
    }
    /// `SetCoord`.
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) {
        self.gp_pnt.set_coord(x, y, z);
    }
    /// `SetPnt`.
    pub fn set_pnt(&mut self, p: Pnt) {
        self.gp_pnt = p;
    }
    /// `SetX`.
    pub fn set_x(&mut self, x: f64) {
        self.gp_pnt.set_x(x);
    }
    /// `SetY`.
    pub fn set_y(&mut self, y: f64) {
        self.gp_pnt.set_y(y);
    }
    /// `SetZ`.
    pub fn set_z(&mut self, z: f64) {
        self.gp_pnt.set_z(z);
    }
}

impl Geometry for CartesianPoint {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.gp_pnt.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(CartesianPoint::from_pnt(self.gp_pnt))
    }
}

impl Point for CartesianPoint {
    fn coord(&self) -> (f64, f64, f64) {
        self.gp_pnt.coords()
    }
    fn pnt(&self) -> Pnt {
        self.gp_pnt
    }
    fn x(&self) -> f64 {
        self.gp_pnt.x()
    }
    fn y(&self) -> f64 {
        self.gp_pnt.y()
    }
    fn z(&self) -> f64 {
        self.gp_pnt.z()
    }
}
