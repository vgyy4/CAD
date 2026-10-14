//! The 3D and 2D parts of OCCT's `gp` package (the 2D types are gp_XY, gp_Pnt2d, gp_Vec2d, gp_Dir2d,
//! gp_Ax2d, gp_Ax22d, gp_Mat2d, gp_Trsf2d, gp_GTrsf2d and the 2D conics). 3D: gp_XYZ, gp_Pnt, gp_Vec, gp_Dir, gp_Ax1, gp_Ax2, gp_Ax3,
//! gp_Mat, gp_Trsf, gp_GTrsf, gp_Quaternion (with NLerp/SLerp), the enums and the conics that
//! ElCLib needs. `gp::resolution()` is `f64::MIN_POSITIVE` (OCCT's `RealSmall()`).
//!
//! Checks that OCCT performs with exceptions are reported as `Err(ConstructionError)`. Checks
//! that OCCT 8.0.1 has compiled out (see docs/api-gp3d.md) are reported the same way, because
//! the numbers OCCT returns there are unspecified.
//!
//! Exact floating point comparisons (`==`, `!=`) are kept as written in OCCT.
#![allow(clippy::float_cmp)]

mod ax1;
mod ax2;
mod ax22d;
mod ax2d;
mod ax3;
mod curves;
mod curves2d;
mod dir;
mod dir2d;
mod gtrsf;
mod gtrsf2d;
mod lin2d;
mod mat;
mod mat2d;
mod pnt;
mod pnt2d;
mod quaternion;
mod trsf;
mod trsf2d;
mod vec;
mod vec2d;
mod xy;
mod xyz;

pub use ax1::{Ax1, Ax1Mirror};
pub use ax2::{Ax2, Ax2Coplanar, Ax2Mirror};
pub use ax2d::{Ax2d, Ax2dMirror};
pub use ax3::{Ax3, Ax3Coplanar, Ax3Mirror};
pub use ax22d::{Ax22d, Ax22dMirror};
pub use curves::{Circ, Elips, Hypr, Lin, Parab};
pub use curves2d::{Circ2d, Circ2dMirror, Elips2d, Elips2dMirror, Hypr2d, Hypr2dMirror, Parab2d, Parab2dMirror};
pub use dir::{Dir, DirMirror};
pub use dir2d::{Dir2d, Dir2dMirror};
pub use gtrsf::{Affinity, GTrsf};
pub use gtrsf2d::GTrsf2d;
pub use lin2d::{Lin2d, Lin2dDistance, Lin2dMirror};
pub use mat::Mat;
pub use mat2d::Mat2d;
pub use pnt::{Pnt, PntMirror};
pub use pnt2d::{Pnt2d, Pnt2dMirror};
pub use quaternion::{EulerSequence, Quaternion, QuaternionNLerp, QuaternionSLerp};
pub use trsf::{Trsf, TrsfForm, TrsfMirror};
pub use trsf2d::{Trsf2d, Trsf2dMirror};
pub use vec::{Vec, VecMirror};
pub use vec2d::{Vec2d, Vec2dMirror};
pub use xy::XY;
pub use xyz::XYZ;

/// OCCT's construction, domain, range and singular-matrix failures (`Standard_ConstructionError`,
/// `Standard_OutOfRange`, `gp_VectorWithNullMagnitude`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstructionError;

impl std::fmt::Display for ConstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("gp construction error")
    }
}

impl std::error::Error for ConstructionError {}

/// `Precision::Angular()`.
pub(crate) const ANGULAR: f64 = 1.0e-12;

/// gp::Resolution()
pub fn resolution() -> f64 {
    f64::MIN_POSITIVE
}

/// gp_Dir::D
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    X,
    Y,
    Z,
    NegativeX,
    NegativeY,
    NegativeZ,
}

/// gp_Dir2d::D
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction2d {
    X,
    Y,
    NegativeX,
    NegativeY,
}

// gp::Origin2d
pub fn origin2d() -> Pnt2d {
    Pnt2d::new(0., 0.)
}
// gp::DX2d
pub fn dx2d() -> Dir2d {
    Dir2d::from_direction(Direction2d::X)
}
// gp::DY2d
pub fn dy2d() -> Dir2d {
    Dir2d::from_direction(Direction2d::Y)
}
// gp::OX2d
pub fn ox2d() -> Ax2d {
    Ax2d::from_standard_direction(Direction2d::X)
}
// gp::OY2d
pub fn oy2d() -> Ax2d {
    Ax2d::from_standard_direction(Direction2d::Y)
}

// gp::Origin
pub fn origin() -> Pnt {
    Pnt::new(0., 0., 0.)
}
// gp::DX
pub fn dx() -> Dir {
    Dir::from_direction(Direction::X)
}
// gp::DY
pub fn dy() -> Dir {
    Dir::from_direction(Direction::Y)
}
// gp::DZ
pub fn dz() -> Dir {
    Dir::from_direction(Direction::Z)
}
// gp::OX
pub fn ox() -> Ax1 {
    Ax1::at(Pnt::new(0., 0., 0.), Direction::X)
}
// gp::OY
pub fn oy() -> Ax1 {
    Ax1::at(Pnt::new(0., 0., 0.), Direction::Y)
}
// gp::OZ
pub fn oz() -> Ax1 {
    Ax1::at(Pnt::new(0., 0., 0.), Direction::Z)
}
// gp::XOY
pub fn xoy() -> Ax2 {
    Ax2::from_standard_direction(Direction::Z)
}
// gp::ZOX
pub fn zox() -> Ax2 {
    Ax2::from_standard_direction(Direction::Y)
}
// gp::YOZ
pub fn yoz() -> Ax2 {
    Ax2::from_standard_direction(Direction::X)
}

// getPerpendicularXDir (gp_Ax2 / gp_Ax3)
pub(crate) fn perpendicular_x(d: Direction) -> Direction {
    match d {
        Direction::X | Direction::NegativeX => Direction::Y,
        Direction::Y | Direction::NegativeY => Direction::Z,
        _ => Direction::X,
    }
}
// getPerpendicularYDir (gp_Ax2 / gp_Ax3)
pub(crate) fn perpendicular_y(d: Direction) -> Direction {
    match d {
        Direction::Z => Direction::Y,
        Direction::NegativeZ => Direction::NegativeY,
        Direction::X => Direction::Z,
        Direction::NegativeX => Direction::NegativeZ,
        Direction::Y => Direction::X,
        Direction::NegativeY => Direction::NegativeX,
    }
}
// crossStandardDir (gp_Ax2 / gp_Ax3)
pub(crate) fn cross_standard_dir(a: Direction, b: Direction) -> Direction {
    use Direction::{NegativeX as NX, NegativeY as NY, NegativeZ as NZ, X, Y, Z};
    match a {
        X => match b {
            Y => Z,
            Z => NY,
            NY => NZ,
            NZ => Y,
            _ => Z,
        },
        Y => match b {
            Z => X,
            X => NZ,
            NX => Z,
            NZ => NX,
            _ => X,
        },
        Z => match b {
            X => Y,
            Y => NX,
            NX => NY,
            NY => X,
            _ => Y,
        },
        NX => match b {
            Y => NZ,
            Z => Y,
            NY => Z,
            NZ => NY,
            _ => NZ,
        },
        NY => match b {
            Z => NX,
            X => Z,
            NX => NZ,
            NZ => X,
            _ => NX,
        },
        NZ => match b {
            X => NY,
            Y => X,
            NX => Y,
            NY => NX,
            _ => NY,
        },
    }
}

pub(crate) fn xyz(x: f64, y: f64, z: f64) -> XYZ {
    XYZ::new(x, y, z)
}
pub(crate) fn point(x: XYZ) -> Pnt {
    Pnt::from_xyz(x)
}
pub(crate) fn vector(x: XYZ) -> Vec {
    Vec::from_xyz(x)
}
