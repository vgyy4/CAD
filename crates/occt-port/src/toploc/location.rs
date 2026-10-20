//! `TopLoc_Location`.
use super::{Datum3D, ItemLocation, SListOfItemLocation};
use crate::gp::Trsf;
use crate::mathlin::MathError;
use std::rc::Rc;
use std::sync::OnceLock;

/// `TopLoc_Location`: a list of (datum, power) items; `Clone` shares the nodes.
#[derive(Debug, Clone, Default)]
pub struct Location {
    items: SListOfItemLocation,
}

impl Location {
    /// `TopLoc_Location()`: the identity.
    pub fn new() -> Self {
        Self::default()
    }
    /// `TopLoc_Location(const gp_Trsf&)`: a fresh datum with power 1.
    pub fn from_trsf(t: &Trsf) -> Result<Self, MathError> {
        Self::from_datum(Rc::new(Datum3D::from_trsf(t)))
    }
    /// `TopLoc_Location(const Handle(TopLoc_Datum3D)&)`.
    pub fn from_datum(d: Rc<Datum3D>) -> Result<Self, MathError> {
        let mut items = SListOfItemLocation::new();
        items.construct(ItemLocation::new(d, 1)?)?;
        Ok(Self { items })
    }
    /// `IsIdentity()`.
    pub fn is_identity(&self) -> bool {
        self.items.is_empty()
    }
    /// `Identity()`.
    pub fn identity(&mut self) {
        self.items.clear();
    }
    /// `Clear()`.
    pub fn clear(&mut self) {
        self.items.clear();
    }
    /// `FirstDatum()` (identity: `Err`).
    pub fn first_datum(&self) -> Result<&Rc<Datum3D>, MathError> {
        Ok(self.items.value()?.datum())
    }
    /// `FirstPower()` (identity: `Err`).
    pub fn first_power(&self) -> Result<i32, MathError> {
        Ok(self.items.value()?.power())
    }
    /// `NextLocation()`.
    pub fn next_location(&self) -> Location {
        Location { items: self.items.tail() }
    }
    /// `Transformation()`.
    pub fn transformation(&self) -> &Trsf {
        static IDENTITY: OnceLock<Trsf> = OnceLock::new();
        match self.items.value() {
            Ok(v) => v.transformation(),
            Err(_) => IDENTITY.get_or_init(Trsf::new),
        }
    }
    /// `operator gp_Trsf()`.
    pub fn to_trsf(&self) -> Trsf {
        *self.transformation()
    }
    /// `HashCode()`: 0 for the identity; equal locations hash equal.
    pub fn hash_code(&self) -> usize {
        if self.is_identity() {
            return 0;
        }
        let mut h: usize = 0x9e37_79b9_7f4a_7c15_u64 as usize;
        let mut it = self.items.clone();
        while let Ok(v) = it.value() {
            let a = Rc::as_ptr(v.datum()) as usize;
            h = (h ^ a).wrapping_mul(0x0100_0000_01b3).rotate_left(23);
            h = (h ^ (v.power() as u32 as usize)).wrapping_mul(0x0100_0000_01b3).rotate_left(23);
            it.to_tail();
        }
        h
    }
    /// `IsEqual`.
    pub fn is_equal(&self, other: &Location) -> bool {
        let (mut a, mut b) = (self.items.clone(), other.items.clone());
        while let (Ok(x), Ok(y)) = (a.value(), b.value()) {
            if !Rc::ptr_eq(x.datum(), y.datum()) || x.power() != y.power() {
                return false;
            }
            let (na, nb) = (a.tail(), b.tail());
            a = na;
            b = nb;
        }
        !a.more() && !b.more()
    }
    /// `IsDifferent`.
    pub fn is_different(&self, other: &Location) -> bool {
        !self.is_equal(other)
    }
    /// `Inverted()`.
    pub fn inverted(&self) -> Result<Location, MathError> {
        if self.is_identity() {
            return Ok(self.clone());
        }
        let mut result = Location::new();
        let mut items = self.items.clone();
        while let Ok(v) = items.value() {
            result.items.construct(ItemLocation::new(v.datum().clone(), -v.power())?)?;
            items.next();
        }
        Ok(result)
    }
    /// `Multiplied(Other)`.
    pub fn multiplied(&self, other: &Location) -> Result<Location, MathError> {
        if self.is_identity() {
            return Ok(other.clone());
        }
        if other.is_identity() {
            return Ok(self.clone());
        }
        let mut result = self.multiplied(&other.next_location())?;
        let mut p = other.first_power()?;
        if !result.is_identity() && Rc::ptr_eq(other.first_datum()?, result.first_datum()?) {
            p += result.first_power()?;
            result.items.to_tail();
        }
        if p != 0 {
            result.items.construct(ItemLocation::new(other.first_datum()?.clone(), p)?)?;
        }
        Ok(result)
    }
    /// `Divided(Other)`.
    pub fn divided(&self, other: &Location) -> Result<Location, MathError> {
        self.multiplied(&other.inverted()?)
    }
    /// `Predivided(Other)`.
    pub fn predivided(&self, other: &Location) -> Result<Location, MathError> {
        if other.is_identity() {
            return Ok(self.clone());
        }
        if self.is_identity() {
            return other.inverted();
        }
        if self == other {
            return Ok(Location::new());
        }
        other.inverted()?.multiplied(self)
    }
    /// `Powered(pwr)`.
    pub fn powered(&self, pwr: i32) -> Result<Location, MathError> {
        if self.is_identity() || pwr == 1 {
            return Ok(self.clone());
        }
        if pwr == 0 {
            return Ok(Location::new());
        }
        if self.items.tail().is_empty() {
            let mut result = Location::new();
            result
                .items
                .construct(ItemLocation::new(self.first_datum()?.clone(), self.first_power()?.wrapping_mul(pwr))?)?;
            return Ok(result);
        }
        if pwr > 0 { self.multiplied(&self.powered(pwr - 1)?) } else { self.inverted()?.powered(pwr.wrapping_neg()) }
    }
    /// `ShallowDump`.
    pub fn shallow_dump(&self) -> String {
        let mut s = String::from("TopLoc_Location : ");
        if self.items.is_empty() {
            s.push_str("Identity\n");
        }
        let mut items = self.items.clone();
        while let Ok(v) = items.value() {
            s.push('\n');
            s.push_str("       Exponent : ");
            s.push_str(&v.power().to_string());
            s.push('\n');
            s.push_str(&v.datum().shallow_dump());
            items.next();
        }
        s.push('\n');
        s
    }
}

impl PartialEq for Location {
    fn eq(&self, other: &Location) -> bool {
        self.is_equal(other)
    }
}
