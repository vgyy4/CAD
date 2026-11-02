//! `GProp_PGProps`: the mass properties of a set of points (each point is a mass, optionally with a density).

use std::ops::{Deref, DerefMut};

use super::{Error, GProps};
use crate::gp::{self, Mat, Pnt, XYZ};
use crate::plib::Array2;

/// `GProp_PGProps`: the properties of a point set, derived from [`GProps`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PGProps {
    base: GProps,
}

impl Deref for PGProps {
    type Target = GProps;
    fn deref(&self) -> &GProps {
        &self.base
    }
}

impl DerefMut for PGProps {
    fn deref_mut(&mut self) -> &mut GProps {
        &mut self.base
    }
}

/// The inertia of a point of unit mass at `(x, y, z)` about the origin (`aMp` of the C++ code).
fn point_inertia(x: f64, y: f64, z: f64) -> Mat {
    Mat::from_columns(
        XYZ::new(y * y + z * z, -x * y, -x * z),
        XYZ::new(-x * y, x * x + z * z, -y * z),
        XYZ::new(-x * z, -y * z, x * x + y * y),
    )
}

/// The `ColLength`/`RowLength` check of the two-dimensional arrays (`Standard_DimensionError`).
fn array2_len_check(rows: usize, cols: usize, other_rows: usize, other_cols: usize) -> Result<(), Error> {
    if rows == other_rows && cols == other_cols { Ok(()) } else { Err(Error) }
}

impl PGProps {
    /// `GProp_PGProps::GProp_PGProps()`.
    pub fn new_0() -> Self {
        Self { base: GProps::new_0() }
    }

    /// `GProp_PGProps::GProp_PGProps(thePnts)`: unit masses at the points.
    pub fn new_1_arr_pnt(pnts: &[Pnt]) -> Self {
        let mut this = Self::new_0();
        for p in pnts {
            this.add_point_1(p);
        }
        this
    }

    /// `GProp_PGProps::GProp_PGProps(thePnts)` for a two-dimensional array (column by column).
    pub fn new_1_arr2_pnt(pnts: &Array2<Pnt>) -> Self {
        let mut this = Self::new_0();
        for c in 0..pnts.cols() {
            for r in 0..pnts.rows() {
                if let Some(p) = pnts.get(r, c) {
                    this.add_point_1(p);
                }
            }
        }
        this
    }

    /// `GProp_PGProps::GProp_PGProps(thePnts, theDensity)`: the arrays must have the same length.
    pub fn new_2_arr_pnt(pnts: &[Pnt], density: &[f64]) -> Result<Self, Error> {
        if pnts.len() != density.len() {
            return Err(Error);
        }
        let mut this = Self::new_0();
        for (p, d) in pnts.iter().zip(density) {
            this.add_point_2(p, *d)?;
        }
        Ok(this)
    }

    /// `GProp_PGProps::GProp_PGProps(thePnts, theDensity)` for two-dimensional arrays.
    pub fn new_2_arr2_pnt(pnts: &Array2<Pnt>, density: &Array2<f64>) -> Result<Self, Error> {
        array2_len_check(pnts.rows(), pnts.cols(), density.rows(), density.cols())?;
        let mut this = Self::new_0();
        for c in 0..pnts.cols() {
            for r in 0..pnts.rows() {
                let p = pnts.get(r, c).ok_or(Error)?;
                let d = density.get(r, c).ok_or(Error)?;
                this.add_point_2(p, *d)?;
            }
        }
        Ok(this)
    }

    /// `GProp_PGProps::AddPoint(thePnt)`: adds a point of unit mass.
    pub fn add_point_1(&mut self, pnt: &Pnt) {
        let (ax, ay, az) = pnt.coords();
        let mp = point_inertia(ax, ay, az);
        let g = &mut self.base.g;
        if self.base.dim == 0.0 {
            self.base.dim = 1.0;
            *g = *pnt;
            self.base.inertia = mp;
        } else {
            let new_mass = self.base.dim + 1.0;
            let new_x = (g.x() * self.base.dim + ax) / new_mass;
            let new_y = (g.y() * self.base.dim + ay) / new_mass;
            let new_z = (g.z() * self.base.dim + az) / new_mass;
            g.set_coord(new_x, new_y, new_z);
            self.base.dim = new_mass;
            self.base.inertia = self.base.inertia.added(&mp);
        }
    }

    /// `GProp_PGProps::AddPoint(thePnt, theDensity)`: the density must be positive.
    pub fn add_point_2(&mut self, pnt: &Pnt, density: f64) -> Result<(), Error> {
        if density <= gp::resolution() {
            return Err(Error);
        }
        let (ax, ay, az) = pnt.coords();
        let mp = point_inertia(ax, ay, az);
        let g = &mut self.base.g;
        if self.base.dim == 0.0 {
            self.base.dim = density;
            *g = *pnt;
            self.base.inertia = mp.multiplied_scalar(density);
        } else {
            let new_mass = self.base.dim + density;
            let new_x = (g.x() * self.base.dim + ax * density) / new_mass;
            let new_y = (g.y() * self.base.dim + ay * density) / new_mass;
            let new_z = (g.z() * self.base.dim + az * density) / new_mass;
            g.set_coord(new_x, new_y, new_z);
            self.base.dim = new_mass;
            self.base.inertia = self.base.inertia.added(&mp.multiplied_scalar(density));
        }
        Ok(())
    }

    /// `GProp_PGProps::Barycentre(thePnts)`: the arithmetic mean of the points.
    pub fn barycentre_1_arr_pnt(pnts: &[Pnt]) -> Pnt {
        let mut sum = XYZ::zero();
        if let Some((first, rest)) = pnts.split_first() {
            sum = *first.xyz();
            for p in rest {
                sum.add(p.xyz());
            }
            sum.divide(pnts.len() as f64);
        }
        Pnt::from_xyz(sum)
    }

    /// `GProp_PGProps::Barycentre(thePnts)` for a two-dimensional array.
    pub fn barycentre_1_arr2_pnt(pnts: &Array2<Pnt>) -> Pnt {
        let mut sum = XYZ::new(0.0, 0.0, 0.0);
        for c in 0..pnts.cols() {
            for r in 0..pnts.rows() {
                if let Some(p) = pnts.get(r, c) {
                    sum.add(p.xyz());
                }
            }
        }
        sum.divide((pnts.rows() * pnts.cols()) as f64);
        Pnt::from_xyz(sum)
    }

    /// `GProp_PGProps::Barycentre(thePnts, theDensity, theMass, theG)`: returns `(theMass, theG)`.
    pub fn barycentre_4_arr_pnt(pnts: &[Pnt], density: &[f64]) -> Result<(f64, Pnt), Error> {
        if pnts.len() != density.len() {
            return Err(Error);
        }
        let (first_pnt, first_density) = (pnts.first().ok_or(Error)?, density.first().ok_or(Error)?);
        let mut mass = *first_density;
        let mut sum = first_pnt.xyz().multiplied(mass);
        for (p, d) in pnts.iter().zip(density).skip(1) {
            mass += *d;
            sum.add(&p.xyz().multiplied(*d));
        }
        sum.divide(mass);
        Ok((mass, Pnt::from_xyz(sum)))
    }

    /// `GProp_PGProps::Barycentre(thePnts, theDensity, theMass, theG)` for two-dimensional arrays.
    pub fn barycentre_4_arr2_pnt(pnts: &Array2<Pnt>, density: &Array2<f64>) -> Result<(f64, Pnt), Error> {
        array2_len_check(pnts.rows(), pnts.cols(), density.rows(), density.cols())?;
        let mut mass = 0.0;
        let mut sum = XYZ::new(0.0, 0.0, 0.0);
        for c in 0..pnts.cols() {
            for r in 0..pnts.rows() {
                let p = pnts.get(r, c).ok_or(Error)?;
                let d = *density.get(r, c).ok_or(Error)?;
                mass += d;
                sum.add(&p.xyz().multiplied(d));
            }
        }
        sum.divide(mass);
        Ok((mass, Pnt::from_xyz(sum)))
    }
}
