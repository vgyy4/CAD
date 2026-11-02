//! `GProp_GProps`: the mass properties of a system of point masses about a system location.

use super::principal::PrincipalProps;
use super::{Error, h_operator};
use crate::gp::{self, Ax1, Mat, Pnt, XYZ};
use crate::mathlin::{Jacobi, Matrix};

/// `GProp_GProps`: centre of mass `g` relative to the system location `loc`, mass `dim`, inertia `inertia`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GProps {
    pub(crate) g: Pnt,
    pub(crate) loc: Pnt,
    pub(crate) dim: f64,
    pub(crate) inertia: Mat,
}

impl Default for GProps {
    fn default() -> Self {
        Self::new_0()
    }
}

impl GProps {
    /// `GProp_GProps::GProp_GProps()`: the system location is the origin.
    pub fn new_0() -> Self {
        Self { g: gp::origin(), loc: gp::origin(), dim: 0.0, inertia: Mat::from_values([0.0; 9]) }
    }

    /// `GProp_GProps::GProp_GProps(SystemLocation)`.
    pub fn new_1(system_location: &Pnt) -> Self {
        Self { g: gp::origin(), loc: *system_location, dim: 0.0, inertia: Mat::from_values([0.0; 9]) }
    }

    /// `GProp_GProps::Add`: adds `item` with density `density` (the density must be positive).
    pub fn add(&mut self, item: &GProps, density: f64) -> Result<(), Error> {
        if density <= gp::resolution() {
            return Err(Error);
        }
        if self.loc.distance(&item.loc) <= gp::resolution() {
            let mut gxyz: XYZ = item.g.xyz().multiplied(item.dim * density);
            let scaled = self.g.xyz().multiplied(self.dim);
            self.g.set_xyz(scaled);
            gxyz.add(self.g.xyz());
            self.dim += item.dim * density;
            if self.dim.abs() >= 1.0e-20 {
                gxyz.divide(self.dim);
                self.g.set_xyz(gxyz);
            } else {
                self.g.set_coord(0.0, 0.0, 0.0);
            }
            self.inertia = self.inertia.added(&item.inertia.multiplied_scalar(density));
        } else {
            let itemloc: XYZ = self.loc.xyz().subtracted(item.loc.xyz());
            let itemg: XYZ = item.loc.xyz().added(item.g.xyz());
            let mut gxyz: XYZ = item.g.xyz().subtracted(&itemloc);
            gxyz = gxyz.multiplied(item.dim * density);
            let scaled = self.g.xyz().multiplied(self.dim);
            self.g.set_xyz(scaled);
            gxyz.add(self.g.xyz());
            self.dim += item.dim * density;
            if self.dim.abs() >= 1.0e-20 {
                gxyz.divide(self.dim);
                self.g.set_xyz(gxyz);
            } else {
                self.g.set_coord(0.0, 0.0, 0.0);
            }
            let mut item_inertia: Mat = item.inertia;
            if item.g.xyz().modulus() > gp::resolution() {
                let h = h_operator(&Pnt::from_xyz(itemg), &item.loc, item.dim);
                item_inertia = item_inertia.subtracted(&h);
            }
            let h = h_operator(&Pnt::from_xyz(itemg), &self.loc, item.dim);
            item_inertia = item_inertia.added(&h);
            self.inertia = self.inertia.added(&item_inertia.multiplied_scalar(density));
        }
        Ok(())
    }

    /// `GProp_GProps::Mass`.
    pub fn mass(&self) -> f64 {
        self.dim
    }

    /// `GProp_GProps::CentreOfMass`.
    pub fn centre_of_mass(&self) -> Pnt {
        Pnt::from_xyz(self.loc.xyz().added(self.g.xyz()))
    }

    /// `GProp_GProps::MatrixOfInertia`: the inertia about the origin of the world frame.
    pub fn matrix_of_inertia(&self) -> Mat {
        let h = h_operator(&self.g, &gp::origin(), self.dim);
        self.inertia.subtracted(&h)
    }

    /// `GProp_GProps::StaticMoments`: `(Ix, Iy, Iz)`.
    pub fn static_moments(&self) -> (f64, f64, f64) {
        let g = self.loc.xyz().added(self.g.xyz());
        (g.x() * self.dim, g.y() * self.dim, g.z() * self.dim)
    }

    /// `GProp_GProps::MomentOfInertia(A)`.
    pub fn moment_of_inertia(&self, a: &Ax1) -> f64 {
        let d = a.direction().xyz();
        if self.loc.distance(a.location()) <= gp::resolution() {
            d.dot(&d.multiplied_mat(&self.inertia))
        } else {
            let mut axis_inertia = self.matrix_of_inertia();
            let at = Pnt::from_xyz(self.loc.xyz().added(self.g.xyz()));
            let h = h_operator(&at, a.location(), self.dim);
            axis_inertia = axis_inertia.added(&h);
            d.dot(&d.multiplied_mat(&axis_inertia))
        }
    }

    /// `GProp_GProps::RadiusOfGyration(A)`.
    pub fn radius_of_gyration(&self, a: &Ax1) -> f64 {
        (self.moment_of_inertia(a) / self.dim).sqrt()
    }

    /// `GProp_GProps::PrincipalProperties`: the principal moments, radii and axes of inertia.
    pub fn principal_properties(&self) -> Result<PrincipalProps, Error> {
        let axis_inertia = self.matrix_of_inertia();
        let mut diag = Matrix::new(1, 3, 1, 3).map_err(|_| Error)?;
        for i in 1..=3 {
            for j in 1..=3 {
                diag.set_value(i, j, axis_inertia.at(i, j)).map_err(|_| Error)?;
            }
        }
        let jac = Jacobi::new(&diag).map_err(|_| Error)?;
        let ixx = jac.value(1).map_err(|_| Error)?;
        let iyy = jac.value(2).map_err(|_| Error)?;
        let izz = jac.value(3).map_err(|_| Error)?;
        let vecs = jac.vectors().map_err(|_| Error)?;
        let column = |c: i32| -> Result<gp::Vec, Error> {
            Ok(gp::Vec::new(
                vecs.value(1, c).map_err(|_| Error)?,
                vecs.value(2, c).map_err(|_| Error)?,
                vecs.value(3, c).map_err(|_| Error)?,
            ))
        };
        let vxx = column(1)?;
        let vyy = column(2)?;
        let vzz = column(3)?;
        let (mut rxx, mut ryy, mut rzz) = (0.0, 0.0, 0.0);
        if 0.0 != self.dim {
            rxx = (ixx / self.dim).abs().sqrt();
            ryy = (iyy / self.dim).abs().sqrt();
            rzz = (izz / self.dim).abs().sqrt();
        }
        Ok(PrincipalProps::from_parts([ixx, iyy, izz], [rxx, ryy, rzz], [vxx, vyy, vzz]))
    }
}
