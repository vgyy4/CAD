//! `GProp_SelGProps`: the mass properties of a surface of revolution portion (a cylinder, a cone, a sphere or a
//! torus part) of unit surface density.

use std::ops::{Deref, DerefMut};

use super::frame::{Rows3, finish_inertia, passage, sym};
use super::{Error, GProps};
use crate::gp::{Ax3, Cone, Cylinder, Pnt, Sphere, Torus};

/// `GProp_SelGProps`: the properties of a surface, derived from [`GProps`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelGProps {
    base: GProps,
}

impl Deref for SelGProps {
    type Target = GProps;
    fn deref(&self) -> &GProps {
        &self.base
    }
}

impl DerefMut for SelGProps {
    fn deref_mut(&mut self) -> &mut GProps {
        &mut self.base
    }
}

/// `std::sin` (libm).
pub(crate) fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// `std::cos` (libm).
pub(crate) fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// The centre of mass `origin + ix * X + iy * Y + iz * Z` of the C++ code, where `X`, `Y`, `Z` are the axes of the
/// local frame `pass` (the rows of `pass` are the coordinates of the axes, the first column is `X`).
pub(crate) fn set_centre(base: &mut GProps, origin: (f64, f64, f64), pass: &Rows3, c: (f64, f64, f64)) {
    let [[xa1, xa2, xa3], [ya1, ya2, ya3], [za1, za2, za3]] = *pass;
    let (x0, y0, z0) = origin;
    let (ix, iy, iz) = c;
    base.g.set_coord(
        x0 + ix * xa1 + iy * xa2 + iz * xa3,
        y0 + ix * ya1 + iy * ya2 + iz * ya3,
        z0 + ix * za1 + iy * za2 + iz * za3,
    );
}

/// The local frame of a `gp_Ax3` (its location and the passage matrix of its axes).
pub(crate) fn frame(loc: &Pnt, pos: &Ax3) -> ((f64, f64, f64), Rows3) {
    (loc.xyz().coords(), passage(pos.x_direction(), pos.y_direction(), pos.direction()))
}

impl SelGProps {
    /// `GProp_SelGProps::GProp_SelGProps()`.
    pub fn new_0() -> Self {
        Self { base: GProps::new_0() }
    }

    /// `GProp_SelGProps::GProp_SelGProps(S, Alpha1, Alpha2, Z1, Z2, SLocation)`: a cylinder part.
    pub fn new_6_cylinder(
        s: &Cylinder,
        alpha1: f64,
        alpha2: f64,
        z1: f64,
        z2: f64,
        s_location: &Pnt,
    ) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(s_location);
        this.try_cylinder(s, alpha1, alpha2, z1, z2)?;
        Ok(this)
    }

    /// `GProp_SelGProps::GProp_SelGProps(S, Alpha1, Alpha2, Z1, Z2, SLocation)`: a cone part.
    pub fn new_6_cone(s: &Cone, alpha1: f64, alpha2: f64, z1: f64, z2: f64, s_location: &Pnt) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(s_location);
        this.try_cone(s, alpha1, alpha2, z1, z2)?;
        Ok(this)
    }

    /// `GProp_SelGProps::GProp_SelGProps(S, Teta1, Teta2, Alpha1, Alpha2, SLocation)`: a sphere part.
    pub fn new_6_sphere(
        s: &Sphere,
        teta1: f64,
        teta2: f64,
        alpha1: f64,
        alpha2: f64,
        s_location: &Pnt,
    ) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(s_location);
        this.try_sphere(s, teta1, teta2, alpha1, alpha2)?;
        Ok(this)
    }

    /// `GProp_SelGProps::GProp_SelGProps(S, Teta1, Teta2, Alpha1, Alpha2, SLocation)`: a torus part.
    pub fn new_6_torus(
        s: &Torus,
        teta1: f64,
        teta2: f64,
        alpha1: f64,
        alpha2: f64,
        s_location: &Pnt,
    ) -> Result<Self, Error> {
        let mut this = Self::new_0();
        this.set_location(s_location);
        this.try_torus(s, teta1, teta2, alpha1, alpha2)?;
        Ok(this)
    }

    /// `GProp_SelGProps::SetLocation`.
    pub fn set_location(&mut self, s_location: &Pnt) {
        self.base.loc = *s_location;
    }

    /// `GProp_SelGProps::Perform(S, Alpha1, Alpha2, Z1, Z2)` for a cylinder.
    pub fn perform_cylinder(&mut self, s: &Cylinder, alpha1: f64, alpha2: f64, z1: f64, z2: f64) {
        let _ = self.try_cylinder(s, alpha1, alpha2, z1, z2);
    }

    /// `GProp_SelGProps::Perform(S, Alpha1, Alpha2, Z1, Z2)` for a cone.
    pub fn perform_cone(&mut self, s: &Cone, alpha1: f64, alpha2: f64, z1: f64, z2: f64) {
        let _ = self.try_cone(s, alpha1, alpha2, z1, z2);
    }

    /// `GProp_SelGProps::Perform(S, Teta1, Teta2, Alpha1, Alpha2)` for a sphere.
    pub fn perform_sphere(&mut self, s: &Sphere, teta1: f64, teta2: f64, alpha1: f64, alpha2: f64) {
        let _ = self.try_sphere(s, teta1, teta2, alpha1, alpha2);
    }

    /// `GProp_SelGProps::Perform(S, Teta1, Teta2, Alpha1, Alpha2)` for a torus.
    pub fn perform_torus(&mut self, s: &Torus, teta1: f64, teta2: f64, alpha1: f64, alpha2: f64) {
        let _ = self.try_torus(s, teta1, teta2, alpha1, alpha2);
    }

    fn try_cylinder(&mut self, s: &Cylinder, alpha1: f64, alpha2: f64, z1: f64, z2: f64) -> Result<(), Error> {
        let (origin, pass) = frame(s.location(), s.position());
        let [[xa1, xa2, xa3], [ya1, ya2, ya3], [za1, za2, za3]] = pass;
        let (x0, y0, z0) = origin;
        let mut r = s.radius();
        self.base.dim = r * (z2 - z1) * (alpha2 - alpha1);
        let sa2 = sin(alpha2);
        let sa1 = sin(alpha1);
        let ca2 = cos(alpha2);
        let ca1 = cos(alpha1);
        let ix = r * (sa2 - sa1) / (alpha2 - alpha1);
        let iy = r * (ca1 - ca2) / (alpha2 - alpha1);
        self.base.g.set_coord(
            x0 + ix * xa1 + iy * xa2 + xa3 * (z2 + z1) / 2.0,
            y0 + ix * ya1 + iy * ya2 + ya3 * (z2 + z1) / 2.0,
            z0 + ix * za1 + iy * za2 + za3 * (z2 + z1) / 2.0,
        );
        let icn2 = r * r * (alpha2 - alpha1 + sa2 * ca2 - sa1 * ca1) / 2.0;
        let isn2 = r * r * (alpha2 - alpha1 - sa2 * ca2 + sa1 * ca1) / 2.0;
        let iz2 = (alpha2 - alpha1) * (z2 * z2 + z2 * z1 + z1 * z1) / 3.0;
        let icnsn = r * r * (sa2 * sa2 - sa1 * sa1) / 2.0;
        let icnz = (z2 + z1) * (sa2 - sa1) / 2.0;
        let isnz = (z2 + z1) * (ca1 - ca2) / 2.0;
        let dm = sym(isn2 + iz2, icn2 + iz2, alpha2 - alpha1, -icnsn, -icnz, -isnz);
        r *= z2 - z1;
        finish_inertia(&mut self.base, &pass, &dm, r)
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
        let auxi1 = r + (z2 + z1) * snt / 2.0;
        let auxi2 = (z2 * z2 + z1 * z2 + z1 * z1) / 3.0;
        self.base.dim = (alpha2 - alpha1) * cnt * (z2 - z1) * auxi1;
        let mut ix = (r * r + r * (z2 + z1) * snt + snt * auxi2) / auxi1;
        let iy = ix * (cn1 - cn2) / (alpha2 - alpha1);
        ix = ix * (sn2 - sn1) / (alpha2 - alpha1);
        let iz = cnt * (r * (z2 + z1) / 2.0 + snt * auxi2) / auxi1;
        set_centre(&mut self.base, origin, &pass, (ix, iy, iz));
        let r1 = r + z1 * snt;
        let r2 = r + z2 * snt;
        let zz = (z2 - z1) * cnt;
        let ir2 = zz * snt * (r1 * r1 * r1 + r1 * r1 * r2 + r1 * r2 * r2 + r2 * r2 * r2) / 4.0;
        let icn2 = ir2 * (alpha2 - alpha1 + cn2 * sn2 - cn1 * sn1) / 2.0;
        let isn2 = ir2 * (alpha2 - alpha1 + cn2 * sn2 - cn1 * sn1) / 2.0;
        let iz2 = zz
            * cnt
            * cnt
            * (z2 - z1)
            * (alpha2 - alpha1)
            * (r * auxi2 + snt * (z2 * z2 * z2 + z2 * z2 * z1 + z2 * z1 * z1 + z1 * z1 * z1))
            / 4.0;
        let icnsn = ir2 * (cn2 * cn2 - cn1 * cn1);
        let icnz = cnt * snt * zz * (r * (z1 + z2) / 2.0 + auxi2) * (sn2 - sn1);
        let isnz = cnt * snt * zz * (r * (z1 + z2) / 2.0 + auxi2) * (cn1 - cn2);
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
        self.base.dim = r * r * (teta2 - teta1) * (snf2 - snf1);
        let ix =
            r * (snt2 - snt1) / (teta2 - teta1) * (alpha2 - alpha1 + snf2 * cnf2 - snf1 * cnf1) / (snf2 - snf1) / 2.0;
        let iy =
            r * (cnt1 - cnt2) / (teta2 - teta1) * (alpha2 - alpha1 + snf2 * cnf2 - snf1 * cnf1) / (snf2 - snf1) / 2.0;
        let iz = r * (snf2 + snf1) / 2.0;
        set_centre(&mut self.base, origin, &pass, (ix, iy, iz));
        let ir2 = (cnf2 * snf2 * (cnf2 + 1.0) - cnf1 * snf1 * (cnf1 + 1.0) + alpha2 - alpha1) / 3.0;
        let icn2 = (teta2 - teta1 + cnt2 * snt2 - cnt1 * snt1) * ir2 / 2.0;
        let isn2 = (teta2 - teta1 - cnt2 * snt2 + cnt1 * snt1) * ir2 / 2.0;
        let icnsn = (snt2 * snt2 - snt1 * snt1) * ir2 / 2.0;
        let iz2 = (teta2 - teta1) * (snf2 * snf2 * snf2 - snf1 * snf1 * snf1) / 3.0;
        let icnz = (snt2 - snt1) * (cnf1 * cnf1 * cnf1 - cnf2 * cnf2 * cnf2) / 3.0;
        let isnz = (cnt1 - cnt2) * (cnf1 * cnf1 * cnf1 - cnf2 * cnf2 * cnf2) / 3.0;
        let dm = sym(isn2 + iz2, icn2 + iz2, ir2 * (teta2 - teta1), -icnsn, -icnz, -isnz);
        finish_inertia(&mut self.base, &pass, &dm, r * r * r * r)
    }

    fn try_torus(&mut self, s: &Torus, teta1: f64, teta2: f64, alpha1: f64, alpha2: f64) -> Result<(), Error> {
        let x_axis = s.x_axis();
        let y_axis = s.y_axis();
        let axis = s.axis();
        let origin = s.location().xyz().coords();
        let pass = passage(x_axis.direction(), y_axis.direction(), axis.direction());
        let mut rmax = s.major_radius();
        let rmin = s.minor_radius();
        let cnt1 = cos(teta1);
        let snt1 = sin(teta1);
        let cnt2 = cos(teta2);
        let snt2 = sin(teta2);
        let cnf1 = cos(alpha1);
        let snf1 = sin(alpha1);
        let cnf2 = cos(alpha2);
        let snf2 = sin(alpha2);
        self.base.dim = rmax * rmin * (teta2 - teta1) * (alpha2 - alpha1);
        let ix = (snt2 - snt1) / (teta2 - teta1) * (rmin * (snf2 - snf1) / (alpha2 - alpha1) + rmax);
        let iy = (cnt1 - cnt2) / (teta2 - teta1) * (rmin * (snf2 - snf1) / (alpha2 - alpha1) + rmax);
        let iz = rmin * (cnf1 - cnf2) / (alpha2 - alpha1);
        set_centre(&mut self.base, origin, &pass, (ix, iy, iz));
        let ir2 = rmax * rmax + 2.0 * rmax * rmin * (snf2 - snf1) + rmin * rmin / 2.0 * (snf2 * cnf2 - snf1 * cnf1);
        let icn2 = ir2 * (teta2 - teta1 + snt2 * cnt2 - snt1 * cnt1) / 2.0;
        let isn2 = ir2 * (teta2 - teta1 - snt2 * cnt2 + snt1 * cnt1) / 2.0;
        let icnsn = ir2 * (snt2 * snt2 - snt1 * snt1) / 2.0;
        let iz2 = (teta2 - teta1) * rmin * rmin * (alpha2 - alpha1 - snf2 * cnf2 + snf1 * cnf1) / 2.0;
        let icnz = rmin * (snt2 - snt1) * (cnf1 - cnf2) * (rmax + rmin * (cnf1 + cnf2) / 2.0);
        let isnz = rmin * (cnt2 - cnt1) * (cnf1 - cnf2) * (rmax + rmin * (cnf1 + cnf2) / 2.0);
        let dm = sym(isn2 + iz2, icn2 + iz2, ir2 * (teta2 - teta1), -icnsn, -icnz, -isnz);
        rmax *= rmin;
        finish_inertia(&mut self.base, &pass, &dm, rmax)
    }
}
