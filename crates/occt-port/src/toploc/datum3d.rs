//! `TopLoc_Datum3D`.
use crate::gp::{Trsf, TrsfForm};
use crate::poly::fmt::{ostream_g, setw};

/// `TopLoc_Datum3D`: a transformation shared by reference (`Rc<Datum3D>`); identity is `Rc::ptr_eq`.
#[derive(Debug, Clone, Default)]
pub struct Datum3D {
    trsf: Trsf,
}

impl Datum3D {
    /// `TopLoc_Datum3D()`: the identity.
    pub fn new() -> Self {
        Self::default()
    }
    /// `TopLoc_Datum3D(T)`.
    pub fn from_trsf(t: &Trsf) -> Self {
        Self { trsf: *t }
    }
    /// `Transformation()`.
    pub fn transformation(&self) -> &Trsf {
        &self.trsf
    }
    /// `Trsf()`.
    pub fn trsf(&self) -> &Trsf {
        &self.trsf
    }
    /// `Form()`.
    pub fn form(&self) -> TrsfForm {
        self.trsf.form()
    }
    /// `ShallowDump` (the address is written `<ptr>`).
    pub fn shallow_dump(&self) -> String {
        let mut s = String::from(" TopLoc_Datum3D <ptr>\n");
        for i in 1..=3 {
            s.push_str("  ( ");
            for j in 1..=4 {
                let v = self.trsf.value(i, j).unwrap_or(0.0);
                s.push_str(&setw(&ostream_g(v, 6), 10));
                s.push(if j < 4 { ',' } else { ')' });
            }
            s.push('\n');
        }
        s.push('\n');
        s
    }
}
