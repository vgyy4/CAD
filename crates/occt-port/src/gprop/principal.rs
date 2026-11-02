//! `GProp_PrincipalProps`: the principal moments, radii of gyration and axes of inertia of a system of masses.

use crate::gp::Vec as GpVec;

/// `GProp_PrincipalProps`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrincipalProps {
    i1: f64,
    i2: f64,
    i3: f64,
    r1: f64,
    r2: f64,
    r3: f64,
    v1: GpVec,
    v2: GpVec,
    v3: GpVec,
}

impl Default for PrincipalProps {
    fn default() -> Self {
        Self::new()
    }
}

impl PrincipalProps {
    /// `GProp_PrincipalProps::GProp_PrincipalProps()`: all moments and radii are `RealLast()`.
    pub fn new() -> Self {
        let last = f64::MAX;
        Self {
            i1: last,
            i2: last,
            i3: last,
            r1: last,
            r2: last,
            r3: last,
            v1: GpVec::new(1.0, 0.0, 0.0),
            v2: GpVec::new(0.0, 1.0, 0.0),
            v3: GpVec::new(0.0, 0.0, 1.0),
        }
    }

    /// The private constructor used by `GProp_GProps::PrincipalProperties` (the centre of gravity is not kept:
    /// the C++ class has no accessor for it).
    pub(crate) fn from_parts(moments: [f64; 3], radii: [f64; 3], axes: [GpVec; 3]) -> Self {
        let [i1, i2, i3] = moments;
        let [r1, r2, r3] = radii;
        let [v1, v2, v3] = axes;
        Self { i1, i2, i3, r1, r2, r3, v1, v2, v3 }
    }

    /// `GProp_PrincipalProps::HasSymmetryAxis()`.
    pub fn has_symmetry_axis_0(&self) -> bool {
        let a_rel_tol = 1.0e-10;
        let eps1 = self.i1.abs() * a_rel_tol;
        let eps2 = self.i2.abs() * a_rel_tol;
        (self.i1 - self.i2).abs() <= eps1 || (self.i1 - self.i3).abs() <= eps1 || (self.i2 - self.i3).abs() <= eps2
    }

    /// `GProp_PrincipalProps::HasSymmetryAxis(aTol)`.
    pub fn has_symmetry_axis_1(&self, a_tol: f64) -> bool {
        let eps1 = (self.i1 * a_tol).abs() + crate::math1d::epsilon(self.i1).abs();
        let eps2 = (self.i2 * a_tol).abs() + crate::math1d::epsilon(self.i2).abs();
        (self.i1 - self.i2).abs() <= eps1 || (self.i1 - self.i3).abs() <= eps1 || (self.i2 - self.i3).abs() <= eps2
    }

    /// `GProp_PrincipalProps::HasSymmetryPoint()`.
    pub fn has_symmetry_point_0(&self) -> bool {
        let a_rel_tol = 1.0e-10;
        let eps1 = self.i1.abs() * a_rel_tol;
        (self.i1 - self.i2).abs() <= eps1 && (self.i1 - self.i3).abs() <= eps1
    }

    /// `GProp_PrincipalProps::HasSymmetryPoint(aTol)`.
    pub fn has_symmetry_point_1(&self, a_tol: f64) -> bool {
        let eps1 = (self.i1 * a_tol).abs() + crate::math1d::epsilon(self.i1).abs();
        (self.i1 - self.i2).abs() <= eps1 && (self.i1 - self.i3).abs() <= eps1
    }

    /// `GProp_PrincipalProps::Moments`: `(Ixx, Iyy, Izz)`.
    pub fn moments(&self) -> (f64, f64, f64) {
        (self.i1, self.i2, self.i3)
    }

    /// `GProp_PrincipalProps::FirstAxisOfInertia`.
    pub fn first_axis_of_inertia(&self) -> GpVec {
        self.v1
    }

    /// `GProp_PrincipalProps::SecondAxisOfInertia`.
    pub fn second_axis_of_inertia(&self) -> GpVec {
        self.v2
    }

    /// `GProp_PrincipalProps::ThirdAxisOfInertia`.
    pub fn third_axis_of_inertia(&self) -> GpVec {
        self.v3
    }

    /// `GProp_PrincipalProps::RadiusOfGyration`: `(Rxx, Ryy, Rzz)`.
    pub fn radius_of_gyration(&self) -> (f64, f64, f64) {
        (self.r1, self.r2, self.r3)
    }
}
