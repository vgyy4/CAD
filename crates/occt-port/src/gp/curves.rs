//! gp_Lin, gp_Circ, gp_Elips, gp_Hypr, gp_Parab (only what ElCLib needs).
use super::{Ax1, Ax2, ConstructionError};
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lin(pub(crate) Ax1);
impl Lin {
    // gp_Lin::gp_Lin(Ax1)
    pub fn from_axis(a: Ax1) -> Self {
        Self(a)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circ {
    pub(crate) a: Ax2,
    pub(crate) r: f64,
}
impl Circ {
    // gp_Circ::gp_Circ
    pub fn new(a: Ax2, r: f64) -> Result<Self, ConstructionError> {
        if r < 0.0 { Err(ConstructionError) } else { Ok(Self { a, r }) }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elips {
    pub(crate) a: Ax2,
    pub(crate) major: f64,
    pub(crate) minor: f64,
}
impl Elips {
    // gp_Elips::gp_Elips
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < minor { Err(ConstructionError) } else { Ok(Self { a, major, minor }) }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hypr {
    pub(crate) a: Ax2,
    pub(crate) major: f64,
    pub(crate) minor: f64,
}
impl Hypr {
    // gp_Hypr::gp_Hypr
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if major < 0.0 || minor < 0.0 { Err(ConstructionError) } else { Ok(Self { a, major, minor }) }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parab {
    pub(crate) a: Ax2,
    pub(crate) f: f64,
}
impl Parab {
    // gp_Parab::gp_Parab
    pub fn new(a: Ax2, f: f64) -> Result<Self, ConstructionError> {
        if f < 0.0 { Err(ConstructionError) } else { Ok(Self { a, f }) }
    }
}
