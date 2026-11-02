//! `GProp_VelGProps`: the mass properties of a volume of revolution portion (a cylinder, a cone, a sphere or a torus
//! part) of unit density.

use std::ops::{Deref, DerefMut};

use super::frame::{Rows3, finish_inertia, passage, sym};
use super::sel::{cos, frame, set_centre, sin};
use super::{Error, GProps};
use crate::gp::{Cone, Cylinder, Pnt, Sphere, Torus};

/// `GProp_VelGProps`: the properties of a volume, derived from [`GProps`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VelGProps {
    base: GProps,
}

impl Deref for VelGProps {
    type Target = GProps;
    fn deref(&self) -> &GProps {
        &self.base
    }
}

impl DerefMut for VelGProps {
    fn deref_mut(&mut self) -> &mut GProps {
        &mut self.base
    }
}

impl VelGProps {
    /// `GProp_VelGProps::GProp_VelGProps()`.
    pub fn new_0() -> Self {
        Self { base: GProps::new_0() }
    }

    /// `GProp_VelGProps::GProp_VelGProps(S, Alpha1, Alpha2, Z1, Z2, VLocation)`: a cylinder part.
    pub fn new_6_cylinder(
        s: &Cylinder,
        alpha1: f64,
        alpha2: f64,
        z1: f64,
        z2: f64,
        v_location: &Pnt,
    ) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(v_location);
        this.try_cylinder(s, alpha1, alpha2, z1, z2)?;
        Ok(this)
    }

    /// `GProp_VelGProps::GProp_VelGProps(S, Alpha1, Alpha2, Z1, Z2, VLocation)`: a cone part.
    pub fn new_6_cone(s: &Cone, alpha1: f64, alpha2: f64, z1: f64, z2: f64, v_location: &Pnt) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(v_location);
        this.try_cone(s, alpha1, alpha2, z1, z2)?;
        Ok(this)
    }

    /// `GProp_VelGProps::GProp_VelGProps(S, Teta1, Teta2, Alpha1, Alpha2, VLocation)`: a sphere part.
    pub fn new_6_sphere(
        s: &Sphere,
        teta1: f64,
        teta2: f64,
        alpha1: f64,
        alpha2: f64,
        v_location: &Pnt,
    ) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(v_location);
        this.try_sphere(s, teta1, teta2, alpha1, alpha2)?;
        Ok(this)
    }

    /// `GProp_VelGProps::GProp_VelGProps(S, Teta1, Teta2, Alpha1, Alpha2, VLocation)`: a torus part.
    pub fn new_6_torus(
        s: &Torus,
        teta1: f64,
        teta2: f64,
        alpha1: f64,
        alpha2: f64,
        v_location: &Pnt,
    ) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(v_location);
        this.try_torus(s, teta1, teta2, alpha1, alpha2)?;
        Ok(this)
    }

    /// `GProp_VelGProps::SetLocation`.
    pub fn set_location(&mut self, v_location: &Pnt) {
        self.base.loc = *v_location;
    }

    /// `GProp_VelGProps::Perform(S, Alpha1, Alpha2, Z1, Z2)` for a cylinder.
    pub fn perform_cylinder(&mut self, s: &Cylinder, alpha1: f64, alpha2: f64, z1: f64, z2: f64) {
        let _ = self.try_cylinder(s, alpha1, alpha2, z1, z2);
    }

    /// `GProp_VelGProps::Perform(S, Alpha1, Alpha2, Z1, Z2)` for a cone.
    pub fn perform_cone(&mut self, s: &Cone, alpha1: f64, alpha2: f64, z1: f64, z2: f64) {
        let _ = self.try_cone(s, alpha1, alpha2, z1, z2);
    }

    /// `GProp_VelGProps::Perform(S, Teta1, Teta2, Alpha1, Alpha2)` for a sphere.
    pub fn perform_sphere(&mut self, s: &Sphere, teta1: f64, teta2: f64, alpha1: f64, alpha2: f64) {
        let _ = self.try_sphere(s, teta1, teta2, alpha1, alpha2);
    }

    /// `GProp_VelGProps::Perform(S, Teta1, Teta2, Alpha1, Alpha2)` for a torus.
    pub fn perform_torus(&mut self, s: &Torus, teta1: f64, teta2: f64, alpha1: f64, alpha2: f64) {
        let _ = self.try_torus(s, teta1, teta2, alpha1, alpha2);
    }

    fn try_cylinder(&mut self, s: &Cylinder, alpha1: f64, alpha2: f64, z1: f64, z2: f64) -> Result<(), Error> {
        let (origin, pass) = frame(s.location(), s.position());
        let [[xa1, xa2, xa3], [ya1, ya2, ya3], [za1, za2, za3]] = pass;
        let (x0, y0, z0) = origin;
        let rayon = s.radius();
        self.base.dim = rayon * rayon * (z2 - z1) / 2.0;
        let sa2 = sin(alpha2);
        let sa1 = sin(alpha1);
        let ca2 = cos(alpha2);
        let ca1 = cos(alpha1);
        let dsin = sa2 - sa1;
        let dcos = ca1 - ca2;
        let coef = rayon / (alpha2 - alpha1);
        self.base.g.set_coord(
            x0 + (coef * (xa1 * dsin + xa2 * dcos)) + (xa3 * (z2 + z1) / 2.0),
            y0 + (coef * (ya1 * dsin + ya2 * dcos)) + (ya3 * (z2 + z1) / 2.0),
            z0 + (coef * (za1 * dsin + za2 * dcos)) + (za3 * (z2 + z1) / 2.0),
        );
        let dim = self.base.dim;
        let icn2 = dim / 2.0 * (alpha2 - alpha1 + sa2 * ca2 - sa1 * ca1);
        let isn2 = dim / 2.0 * (alpha2 - alpha1 - sa2 * ca2 + sa1 * ca1);
        let iz2 = dim * (alpha2 - alpha1) * (z2 * z2 + z1 * z2 + z1 * z1);
        let icnsn = dim * (ca2 * ca2 - ca1 * ca1) / 2.0;
        let icnz = dim * (z2 + z1) / 2.0 * dsin;
        let isnz = dim * (z2 + z1) / 2.0 * dcos;
        self.base.dim = (alpha2 - alpha1) * dim;
        let dm = sym(
            rayon * rayon * isn2 + iz2,
            rayon * rayon * icn2 + iz2,
            rayon * rayon * self.base.dim,
            -rayon * rayon * icnsn,
            -rayon * icnz,
            -rayon * isnz,
        );
        finish_inertia(&mut self.base, &pass, &dm, 1.0)
    }

    fn try_cone(&mut self, s: &Cone, alpha1: f64, alpha2: f64, z1: f64, z2: f64) -> Result<(), Error> {
        let (origin, pass) = frame(s.location(), s.position());
        let t = s.semi_angle();
        let cnt = cos(t);
        let snt = sin(t);
        let r = s.ref_radius();
        let sn2 = sin(alpha2);
        let sn1 = sin(alpha1);
        let cn2 = cos(alpha2);
        let cn1 = cos(alpha1);
        let zz = (z2 - z1) * (z2 - z1) * cnt * snt;
        let auxi1 = 2.0 * r + (z2 + z1) * snt;
        self.base.dim = zz * (alpha2 - alpha1) * auxi1 / 2.0;
        let r1 = r + z1 * snt;
        let r2 = r + z2 * snt;
        let coef0 = r1 * r1 + r1 * r2 + r2 * r2;
        let iz = cnt * (r * (z2 + z1) + 2.0 * snt * (z1 * z1 + z1 * z2 + z2 * z2) / 3.0) / auxi1;
        let ix = coef0 * (sn2 - sn1) / (alpha2 - alpha1) / auxi1;
        let iy = coef0 * (cn1 - cn2) / (alpha2 - alpha1) / auxi1;
        set_centre(&mut self.base, origin, &pass, (ix, iy, iz));
        let ir2 = zz * (r2 * r2 * r2 + r2 * r2 * r1 + r1 * r1 * r2 + r1 * r1 * r1) / 4.0;
        let icn2 = ir2 * (alpha2 - alpha1 + cn2 * sn2 - cn1 * sn1) / 2.0;
        let isn2 = ir2 * (alpha2 - alpha1 + cn2 * sn2 - cn1 * sn1) / 2.0;
        let iz2 = zz
            * cnt
            * cnt
            * (alpha2 - alpha1)
            * (z1 * z1 * (r / 3.0 + z1 * snt / 4.0)
                + z2 * z2 * (r / 3.0 + z2 * snt / 4.0)
                + z1 * z2 * (r / 3.0 + z1 * snt / 4.0 + z2 * snt / 4.0));
        let icnsn = ir2 * (cn2 * cn2 - cn1 * cn1);
        let icnz = (z1 + z2) * zz * coef0 * (sn2 - sn1) / 3.0;
        let isnz = (z1 + z2) * zz * coef0 * (cn1 - cn2) / 3.0;
        let dm = sym(isn2 + iz2, icn2 + iz2, ir2 * (alpha2 - alpha1), -icnsn, -icnz, -isnz);
        finish_inertia(&mut self.base, &pass, &dm, 1.0)
    }

    fn try_sphere(&mut self, s: &Sphere, teta1: f64, teta2: f64, alpha1: f64, alpha2: f64) -> Result<(), Error> {
        let (origin, pass) = frame(s.location(), s.position());
        let r = s.radius();
        let cnt1 = cos(teta1);
        let snt1 = sin(teta1);
        let cnt2 = cos(teta2);
        let snt2 = sin(teta2);
        let cnf1 = cos(alpha1);
        let snf1 = sin(alpha1);
        let cnf2 = cos(alpha2);
        let snf2 = sin(alpha2);
        self.base.dim = (teta2 - teta1) * r * r * r * (snf2 - snf1) / 3.0;
        let ix =
            r * (snt2 - snt1) / (teta2 - teta1) * (alpha2 - alpha1 + snf2 * cnf2 - snf1 * cnf1) / (snf2 - snf1) / 2.0;
        let iy =
            r * (cnt1 - cnt2) / (teta2 - teta1) * (alpha2 - alpha1 + snf2 * cnf2 - snf1 * cnf1) / (snf2 - snf1) / 2.0;
        let iz = r * (snf2 + snf1) / 2.0;
        set_centre(&mut self.base, origin, &pass, (ix, iy, iz));
        let ir2 = (cnf2 * snf2 * (cnf2 + 1.0) - cnf1 * snf1 * (cnf1 + 1.0) + alpha2 - alpha1) / 9.0;
        let icn2 = (teta2 - teta1 + cnt2 * snt2 - cnt1 * snt1) * ir2 / 2.0;
        let isn2 = (teta2 - teta1 - cnt2 * snt2 + cnt1 * snt1) * ir2 / 2.0;
        let icnsn = (snt2 * snt2 - snt1 * snt1) * ir2 / 2.0;
        let iz2 = (teta2 - teta1) * (snf2 * snf2 * snf2 - snf1 * snf1 * snf1) / 9.0;
        let icnz = (snt2 - snt1) * (cnf1 * cnf1 * cnf1 - cnf2 * cnf2 * cnf2) / 9.0;
        let isnz = (cnt1 - cnt2) * (cnf1 * cnf1 * cnf1 - cnf2 * cnf2 * cnf2) / 9.0;
        let dm = sym(isn2 + iz2, icn2 + iz2, ir2 * (teta2 - teta1), -icnsn, -icnz, -isnz);
        finish_inertia(&mut self.base, &pass, &dm, r * r * r * r * r)
    }

    fn try_torus(&mut self, s: &Torus, teta1: f64, teta2: f64, alpha1: f64, alpha2: f64) -> Result<(), Error> {
        let x_axis = s.x_axis();
        let y_axis = s.y_axis();
        let axis = s.axis();
        let origin = s.location().xyz().coords();
        let pass: Rows3 = passage(x_axis.direction(), y_axis.direction(), axis.direction());
        let mut rmax = s.major_radius();
        let rmin = s.minor_radius();
        let cnt1 = cos(teta1);
        let snt1 = sin(teta1);
        // The C++ code takes the cosine and the sine of `Alpha2` for `Cnt2` and `Snt2` (kept as is).
        let cnt2 = cos(alpha2);
        let snt2 = sin(alpha2);
        let cnf1 = cos(alpha1);
        let snf1 = sin(alpha1);
        let cnf2 = cos(alpha2);
        let snf2 = sin(alpha2);
        self.base.dim = rmax * rmin * rmin * (teta2 - teta1) * (alpha2 - alpha1) / 2.0;
        let ix = (snt2 - snt1) / (teta2 - teta1) * (rmin * (snf2 - snf1) / (alpha2 - alpha1) + rmax);
        let iy = (cnt1 - cnt2) / (teta2 - teta1) * (rmin * (snf2 - snf1) / (alpha2 - alpha1) + rmax);
        let iz = rmin * (cnf1 - cnf2) / (alpha2 - alpha1);
        set_centre(&mut self.base, origin, &pass, (ix, iy, iz));
        let ir2 = rmax * rmax
            + rmin * rmin / 2.0
            + 2.0 * rmax * rmin * (snf2 - snf1)
            + rmin * rmin / 2.0 * (snf2 * cnf2 - snf1 * cnf1);
        let icn2 = ir2 * (teta2 - teta1 + snt2 * cnt2 - snt1 * cnt1) / 2.0;
        let isn2 = ir2 * (teta2 - teta1 - snt2 * cnt2 + snt1 * cnt1) / 2.0;
        let icnsn = ir2 * (snt2 * snt2 - snt1 * snt1) / 2.0;
        let iz2 = (teta2 - teta1) * rmin * rmin * (alpha2 - alpha1 - snf2 * cnf2 + snf1 * cnf1) / 2.0;
        let icnz = rmin * (snt2 - snt1) * (cnf1 - cnf2) * (rmax + rmin * (cnf1 + cnf2) / 2.0);
        let isnz = rmin * (cnt2 - cnt1) * (cnf1 - cnf2) * (rmax + rmin * (cnf1 + cnf2) / 2.0);
        let dm = sym(isn2 + iz2, icn2 + iz2, ir2 * (teta2 - teta1), -icnsn, -icnz, -isnz);
        rmax = rmax * rmin * rmin / 2.0;
        finish_inertia(&mut self.base, &pass, &dm, rmax)
    }
}
