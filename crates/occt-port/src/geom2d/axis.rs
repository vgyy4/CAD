//! `Geom2d_AxisPlacement`.
use super::{Geometry2d, geometry_any};
use crate::gp::{Ax2d, Dir2d, Pnt2d, Trsf2d};

/// `Geom2d_AxisPlacement`.
#[derive(Debug, Clone)]
pub struct AxisPlacement {
    axis: Ax2d,
}

impl AxisPlacement {
    /// `Geom2d_AxisPlacement(gp_Ax2d)`.
    pub fn new(a: Ax2d) -> Self {
        AxisPlacement { axis: a }
    }
    /// `Geom2d_AxisPlacement(P, V)`.
    pub fn from_point_direction(p: Pnt2d, v: Dir2d) -> Self {
        AxisPlacement { axis: Ax2d::new(p, v) }
    }
    /// `Reverse`.
    pub fn reverse(&mut self) {
        self.axis.reverse();
    }
    /// `Reversed`.
    pub fn reversed(&self) -> AxisPlacement {
        let mut a = self.axis;
        a.reverse();
        AxisPlacement::new(a)
    }
    /// `SetAxis`.
    pub fn set_axis(&mut self, a: Ax2d) {
        self.axis = a;
    }
    /// `SetDirection`.
    pub fn set_direction(&mut self, v: Dir2d) {
        self.axis.set_direction(v);
    }
    /// `SetLocation`.
    pub fn set_location(&mut self, p: Pnt2d) {
        self.axis.set_location(p);
    }
    /// `Angle`.
    pub fn angle(&self, other: &AxisPlacement) -> f64 {
        self.axis.angle(&other.ax2d())
    }
    /// `Ax2d`.
    pub fn ax2d(&self) -> Ax2d {
        self.axis
    }
    /// `Direction`.
    pub fn direction(&self) -> Dir2d {
        *self.axis.direction()
    }
    /// `Location`.
    pub fn location(&self) -> Pnt2d {
        *self.axis.location()
    }
}

impl Geometry2d for AxisPlacement {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.axis.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(AxisPlacement::new(self.axis))
    }
}
