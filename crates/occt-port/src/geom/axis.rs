//! `Geom_Axis1Placement` and `Geom_Axis2Placement`.
use super::{AxisPlacement, GeomError, Geometry, Result, geometry_any};
use crate::gp::{Ax1, Ax2, Dir, Pnt, Trsf};

/// `Geom_Axis1Placement`.
#[derive(Debug, Clone)]
pub struct Axis1Placement {
    axis: Ax1,
}

impl Axis1Placement {
    /// `Geom_Axis1Placement(gp_Ax1)`.
    pub fn new(a: Ax1) -> Self {
        Axis1Placement { axis: a }
    }
    /// `Geom_Axis1Placement(P, V)`.
    pub fn from_point_direction(p: Pnt, v: Dir) -> Self {
        Axis1Placement { axis: Ax1::new(p, v) }
    }
    /// `Ax1`.
    pub fn ax1(&self) -> Ax1 {
        self.axis
    }
    /// `Reverse`.
    pub fn reverse(&mut self) {
        self.axis.reverse();
    }
    /// `Reversed`.
    pub fn reversed(&self) -> Axis1Placement {
        let mut a1 = self.axis;
        a1.reverse();
        Axis1Placement { axis: a1 }
    }
}

impl Geometry for Axis1Placement {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.axis.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(Axis1Placement { axis: self.axis })
    }
}

impl AxisPlacement for Axis1Placement {
    fn ax1_ref(&self) -> &Ax1 {
        &self.axis
    }
    fn ax1_mut(&mut self) -> &mut Ax1 {
        &mut self.axis
    }
    fn set_direction(&mut self, v: Dir) -> Result<()> {
        self.axis.set_direction(v);
        Ok(())
    }
}

/// `Geom_Axis2Placement`: the axis (location and main direction) and the x and y directions are stored separately.
#[derive(Debug, Clone)]
pub struct Axis2Placement {
    axis: Ax1,
    vxdir: Dir,
    vydir: Dir,
}

impl Axis2Placement {
    /// `Geom_Axis2Placement(gp_Ax2)`.
    pub fn new(a: Ax2) -> Self {
        Axis2Placement { vxdir: *a.x_direction(), vydir: *a.y_direction(), axis: a.axis() }
    }
    /// `Geom_Axis2Placement(P, N, Vx)`: `N` and `Vx` parallel is an unchecked case of OCCT 8.0.1 (an error here).
    pub fn from_point_directions(p: Pnt, n: Dir, vx: Dir) -> Result<Self> {
        let axis = Ax1::new(p, n);
        let vxdir = n.cross_crossed(&vx, &n)?;
        let vydir = n.crossed(&vxdir)?;
        Ok(Axis2Placement { axis, vxdir, vydir })
    }
    /// `SetAx2`.
    pub fn set_ax2(&mut self, a: Ax2) {
        self.vxdir = *a.x_direction();
        self.vydir = *a.y_direction();
        self.axis = a.axis();
    }
    /// `SetXDirection`.
    pub fn set_x_direction(&mut self, vx: Dir) -> Result<()> {
        let d = *self.axis.direction();
        self.vxdir = d.cross_crossed(&vx, &d)?;
        self.vydir = d.crossed(&self.vxdir)?;
        Ok(())
    }
    /// `SetYDirection`.
    pub fn set_y_direction(&mut self, vy: Dir) -> Result<()> {
        let d = *self.axis.direction();
        self.vxdir = vy.crossed(&d)?;
        self.vydir = d.crossed(&self.vxdir)?;
        Ok(())
    }
    /// `Ax2`: `gp_Ax2(location, direction, vxdir)` (an error when `vxdir` is parallel to the direction).
    pub fn ax2(&self) -> Result<Ax2> {
        Ok(Ax2::new(*self.axis.location(), *self.axis.direction(), &self.vxdir)?)
    }
    /// `XDirection`.
    pub fn x_direction(&self) -> Dir {
        self.vxdir
    }
    /// `YDirection`.
    pub fn y_direction(&self) -> Dir {
        self.vydir
    }
}

impl Geometry for Axis2Placement {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        let loc = self.axis.location().transformed(t);
        self.axis.set_location(loc);
        let (Ok(vx), Ok(vy)) = (self.vxdir.transformed(t), self.vydir.transformed(t)) else {
            return;
        };
        self.vxdir = vx;
        self.vydir = vy;
        if let Ok(d) = vx.crossed(&vy) {
            self.axis.set_direction(d);
        }
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(Axis2Placement { axis: self.axis, vxdir: self.vxdir, vydir: self.vydir })
    }
}

impl AxisPlacement for Axis2Placement {
    fn ax1_ref(&self) -> &Ax1 {
        &self.axis
    }
    fn ax1_mut(&mut self) -> &mut Ax1 {
        &mut self.axis
    }
    fn set_direction(&mut self, v: Dir) -> Result<()> {
        self.axis.set_direction(v);
        self.vxdir = v.cross_crossed(&self.vxdir, &v).map_err(|_| GeomError::Argument)?;
        self.vydir = v.crossed(&self.vxdir)?;
        Ok(())
    }
}
