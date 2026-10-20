//! `TopLoc_ItemLocation`.
use super::Datum3D;
use crate::gp::Trsf;
use crate::mathlin::MathError;
use std::rc::Rc;

/// `TopLoc_ItemLocation`: a datum, a power and a transformation (`datum^power`, or the cumulative transformation of
/// the list node that holds the item).
#[derive(Debug, Clone)]
pub struct ItemLocation {
    datum: Rc<Datum3D>,
    power: i32,
    pub(crate) trsf: Trsf,
}

impl ItemLocation {
    /// `TopLoc_ItemLocation(D, P)`.
    pub fn new(datum: Rc<Datum3D>, power: i32) -> Result<Self, MathError> {
        let trsf = datum.transformation().powered(power).map_err(|_| MathError::Construction)?;
        Ok(Self { datum, power, trsf })
    }
    /// The datum.
    pub fn datum(&self) -> &Rc<Datum3D> {
        &self.datum
    }
    /// The power.
    pub fn power(&self) -> i32 {
        self.power
    }
    /// The transformation (cumulative once the item is in a list).
    pub fn transformation(&self) -> &Trsf {
        &self.trsf
    }
}
