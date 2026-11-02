//! `Adaptor3d_HVertex`: a vertex of the parametric domain of a surface (a 2D point with an orientation and a resolution).
//!
//! Translated from `Adaptor3d_HVertex.hxx/.cxx` (OCCT 8.0.1).
use super::Result;
use crate::adaptor2d::Curve2dAdaptor;
use crate::elclib;
use crate::geom2d::confusion;
use crate::gp::Pnt2d;
use crate::topabs::Orientation;

/// `Adaptor3d_HVertex`.
#[derive(Debug, Clone)]
pub struct HVertex {
    pnt: Pnt2d,
    tol: f64,
    ori: Orientation,
}

impl Default for HVertex {
    /// `Adaptor3d_HVertex()`: the origin, resolution 0 (the orientation is not initialised in C++; here `Forward`).
    fn default() -> Self {
        Self { pnt: Pnt2d::new(0.0, 0.0), tol: 0.0, ori: Orientation::Forward }
    }
}

impl HVertex {
    /// `Adaptor3d_HVertex(P, Or, Resolution)`.
    pub fn new(p: Pnt2d, or: Orientation, resolution: f64) -> Self {
        Self { pnt: p, tol: resolution, ori: or }
    }

    /// `Value`.
    pub fn value(&self) -> Pnt2d {
        self.pnt
    }

    /// `Parameter`: the parameter of the point on the line of the curve (`ElCLib::Parameter(C->Line(), P)`).
    pub fn parameter(&self, c: &dyn Curve2dAdaptor) -> Result<f64> {
        Ok(elclib::parameter_lin2d(&c.line()?, &self.pnt))
    }

    /// `Resolution` (the curve is not used).
    pub fn resolution(&self, _c: &dyn Curve2dAdaptor) -> f64 {
        self.tol
    }

    /// `Orientation`.
    pub fn orientation(&self) -> Orientation {
        self.ori
    }

    /// `IsSame`: the points are within `Precision::Confusion()`.
    pub fn is_same(&self, other: &HVertex) -> bool {
        self.pnt.distance(&other.value()) <= confusion()
    }
}
