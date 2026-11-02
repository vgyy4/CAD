//! `GProp_CelGProps`: the mass properties of a curve (a circle arc or a line segment) of unit linear density.

use std::ops::{Deref, DerefMut};

use super::frame::{finish_inertia, passage, sym};
use super::sel::{cos, sin};
use super::{Error, GProps};
use crate::elclib::line_value;
use crate::gp::{Ax1, Circ, Lin, Mat, Pnt, XYZ};

/// `GProp_CelGProps`: the properties of a curve, derived from [`GProps`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CelGProps {
    base: GProps,
}

impl Deref for CelGProps {
    type Target = GProps;
    fn deref(&self) -> &GProps {
        &self.base
    }
}

impl DerefMut for CelGProps {
    fn deref_mut(&mut self) -> &mut GProps {
        &mut self.base
    }
}

/// `(U2 * (U2 * (U2 * alfa1 + alfa2) + alfa3)) - (U1 * (U1 * (U1 * alfa1 + alfa2) + alfa3))`.
fn moment_integral(u1: f64, u2: f64, alfa1: f64, alfa2: f64, alfa3: f64) -> f64 {
    (u2 * (u2 * (u2 * alfa1 + alfa2) + alfa3)) - (u1 * (u1 * (u1 * alfa1 + alfa2) + alfa3))
}

impl CelGProps {
    /// `GProp_CelGProps::GProp_CelGProps()`.
    pub fn new_0() -> Self {
        Self { base: GProps::new_0() }
    }

    /// `GProp_CelGProps::GProp_CelGProps(C, CLocation)`: the whole circle.
    pub fn new_2(c: &Circ, c_location: &Pnt) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(c_location);
        this.try_perform_circ(c, 0.0, 2.0 * std::f64::consts::PI)?;
        Ok(this)
    }

    /// `GProp_CelGProps::GProp_CelGProps(C, U1, U2, CLocation)` for a circle arc.
    pub fn new_4_circ(c: &Circ, u1: f64, u2: f64, c_location: &Pnt) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(c_location);
        this.try_perform_circ(c, u1, u2)?;
        Ok(this)
    }

    /// `GProp_CelGProps::GProp_CelGProps(C, U1, U2, CLocation)` for a line segment.
    pub fn new_4_lin(c: &Lin, u1: f64, u2: f64, c_location: &Pnt) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(c_location);
        this.perform_lin(c, u1, u2);
        Ok(this)
    }

    /// `GProp_CelGProps::SetLocation`.
    pub fn set_location(&mut self, c_location: &Pnt) {
        self.base.loc = *c_location;
    }

    /// `GProp_CelGProps::Perform(C, U1, U2)` for a circle: the arc from `U1` to `U2`. The eigen decomposition
    /// cannot fail for a finite input; the state is left unchanged if it does.
    pub fn perform_circ(&mut self, c: &Circ, u1: f64, u2: f64) {
        let _ = self.try_perform_circ(c, u1, u2);
    }

    fn try_perform_circ(&mut self, c: &Circ, u1: f64, u2: f64) -> Result<(), Error> {
        let (x0, y0, z0) = c.location().xyz().coords();
        let x_axis = c.x_axis();
        let y_axis = c.y_axis();
        let axis = c.axis();
        let (xa1, ya1, _) = x_axis.direction().xyz().coords();
        let (xa2, ya2, _) = y_axis.direction().xyz().coords();
        let ray = c.radius();
        self.base.dim = ray * (u2 - u1).abs();
        let xloc = ray * (sin(u2) - sin(u1)) / (u2 - u1);
        let yloc = ray * (cos(u1) - cos(u2)) / (u2 - u1);
        self.base.g.set_coord(xloc * xa1 + yloc * xa2 + x0, xloc * ya1 + yloc * ya2 + y0, z0);

        let d11 = ray * ray * ray * (u2 / 2.0 - u1 / 2.0 - sin(2.0 * u2) / 4.0 + sin(2.0 * u1) / 4.0);
        let d22 = ray * ray * ray * (u2 / 2.0 - u1 / 2.0 + sin(2.0 * u2) / 4.0 - sin(2.0 * u1) / 4.0);
        let d33 = ray * ray * self.base.dim;
        let d12 = -ray * ray * ray * (cos(2.0 * u1) / 4.0 - cos(2.0 * u2) / 4.0);
        let dm = sym(d11, d22, d33, d12, 0.0, 0.0);
        let pass = passage(x_axis.direction(), y_axis.direction(), axis.direction());
        finish_inertia(&mut self.base, &pass, &dm, 1.0)
    }

    /// `GProp_CelGProps::Perform(C, U1, U2)` for a line segment.
    pub fn perform_lin(&mut self, c: &Lin, u1: f64, u2: f64) {
        let pos: &Ax1 = c.position();
        let p1 = line_value(u1, pos);
        self.base.dim = (u2 - u1).abs();
        let p2 = line_value(u2, pos);
        self.base.g.set_coord((p1.x() + p2.x()) / 2.0, (p1.y() + p2.y()) / 2.0, (p1.z() + p2.z()) / 2.0);
        let (vx, vy, vz) = pos.direction().xyz().coords();
        let (x0, y0, z0) = pos.location().xyz().coords();

        let mut alfa1 = (vz * vz + vy * vy) / 3.0;
        let mut alfa2 = vy * y0 + vz * z0;
        let mut alfa3 = y0 * y0 + z0 * z0;
        let ixx = moment_integral(u1, u2, alfa1, alfa2, alfa3);

        alfa1 = (vz * vz + vx * vx) / 3.0;
        alfa2 = vx * x0 + vz * z0;
        alfa3 = x0 * x0 + z0 * z0;
        let iyy = moment_integral(u1, u2, alfa1, alfa2, alfa3);

        alfa1 = (vy * vy + vx * vx) / 3.0;
        alfa2 = vy * y0 + vz * z0;
        alfa3 = y0 * y0 + z0 * z0;
        let izz = moment_integral(u1, u2, alfa1, alfa2, alfa3);

        alfa1 = (vy * vx) / 3.0;
        alfa2 = (vy * x0 + vx * y0) / 2.0;
        alfa3 = y0 * x0;
        let ixy = moment_integral(u1, u2, alfa1, alfa2, alfa3);

        alfa1 = (vz * vx) / 3.0;
        alfa2 = (vz * x0 + vx * z0) / 2.0;
        alfa3 = z0 * x0;
        let ixz = moment_integral(u1, u2, alfa1, alfa2, alfa3);

        alfa1 = (vy * vz) / 3.0;
        alfa2 = (vy * z0 + vz * y0) / 2.0;
        alfa3 = y0 * z0;
        let iyz = moment_integral(u1, u2, alfa1, alfa2, alfa3);

        self.base.inertia =
            Mat::from_columns(XYZ::new(ixx, -ixy, -ixz), XYZ::new(-ixy, iyy, -iyz), XYZ::new(-ixz, -iyz, izz));
    }
}
