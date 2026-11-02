//! OCCT's `GProp` package (toolkit TKMath): the mass properties of point sets, curves and elementary surfaces
//! and volumes of revolution (`GProp_GProps`, `GProp_PGProps`, `GProp_CelGProps`, `GProp_SelGProps`,
//! `GProp_VelGProps`), the principal properties (`GProp_PrincipalProps`) and the point-set equation
//! (`GProp_PEquation`).
//!
//! The C++ class hierarchy (`GProp_PGProps`, `GProp_CelGProps`, ... derive from `GProp_GProps`) is a struct per
//! class holding the base struct, with `Deref`/`DerefMut` giving the base methods. `Standard_Failure` is an
//! [`Error`]. The inertia matrices are `gp::Mat` with the same operation order as the C++ code.
#![allow(clippy::float_cmp)]

mod cel;
mod frame;
mod gprops;
mod pequation;
mod pgprops;
mod principal;
mod sel;
mod vel;

pub use cel::CelGProps;
pub use gprops::GProps;
pub use pequation::PEquation;
pub use pgprops::PGProps;
pub use principal::PrincipalProps;
pub use sel::SelGProps;
pub use vel::VelGProps;

use crate::gp::{Mat, Pnt};

/// `Standard_Failure` (domain, dimension, no-such-object and the `gp` construction errors).
pub use crate::gp::ConstructionError as Error;

/// `GProp_ValueType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Mass,
    CenterMassX,
    CenterMassY,
    CenterMassZ,
    InertiaXX,
    InertiaYY,
    InertiaZZ,
    InertiaXY,
    InertiaXZ,
    InertiaYZ,
    Unknown,
}

/// `GProp_PEquation::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    None,
    Point,
    Line,
    Plane,
    Space,
}

/// `GProp::HOperator`: the inertia operator of a point mass `mass` at `g`, taken about `q`.
pub fn h_operator(g: &Pnt, q: &Pnt, mass: f64) -> Mat {
    let qg = g.xyz().subtracted(q.xyz());
    let ixx = qg.y() * qg.y() + qg.z() * qg.z();
    let iyy = qg.x() * qg.x() + qg.z() * qg.z();
    let izz = qg.y() * qg.y() + qg.x() * qg.x();
    let ixy = -qg.x() * qg.y();
    let iyz = -qg.y() * qg.z();
    let ixz = -qg.x() * qg.z();
    let mut op = Mat::from_columns(
        crate::gp::XYZ::new(ixx, ixy, ixz),
        crate::gp::XYZ::new(ixy, iyy, iyz),
        crate::gp::XYZ::new(ixz, iyz, izz),
    );
    op.multiply_scalar(mass);
    op
}
