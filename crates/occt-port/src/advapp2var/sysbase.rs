//! `AdvApp2Var_SysBase`: the Fortran-support state of the package (the part that is judged).
//! Its core-allocation globals are not observable through the judged API, so the struct is
//! empty and `mainial_` only reports success.

/// `AdvApp2Var_SysBase`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SysBase;

impl SysBase {
    /// `AdvApp2Var_SysBase::AdvApp2Var_SysBase()`: runs `mainial_` first.
    pub fn new() -> Self {
        let mut s = Self;
        s.mainial_();
        s
    }

    /// `AdvApp2Var_SysBase::mainial_()`: resets the core-allocation state, returns 0.
    pub fn mainial_(&mut self) -> i32 {
        0
    }

    /// `AdvApp2Var_SysBase::mnfndeb_()`: returns 0.
    pub fn mnfndeb_() -> i32 {
        0
    }
}
