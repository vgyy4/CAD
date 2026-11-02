//! `AdvApp2Var_Iso`: an iso-parametric curve of the constraint frame.

use super::{Arr2, Error, Result};
use crate::adaptor3d::GeomAbsIsoType as IsoType;

/// `AdvApp2Var_Iso`
#[derive(Debug, Clone, PartialEq)]
pub struct Iso {
    ty: IsoType,
    const_par: f64,
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    position: i32,
    extrem_order: i32,
    deriv_order: i32,
    nb_coeff: i32,
    appr_is_done: bool,
    has_result: bool,
    max_errors: Option<Arr2<f64>>,
    moy_errors: Option<Arr2<f64>>,
}

impl Iso {
    /// `AdvApp2Var_Iso::AdvApp2Var_Iso()`
    pub fn new_0() -> Self {
        Self {
            ty: IsoType::IsoU,
            const_par: 0.5,
            u0: 0.0,
            u1: 1.0,
            v0: 0.0,
            v1: 1.0,
            position: 0,
            extrem_order: 2,
            deriv_order: 2,
            nb_coeff: 0,
            appr_is_done: false,
            has_result: false,
            max_errors: None,
            moy_errors: None,
        }
    }

    /// `AdvApp2Var_Iso(type, iu, iv)`: delegates with `cte = 0.5`, the domain `[0,1]^2`, `pos = 0`.
    pub fn new_3(ty: IsoType, iu: i32, iv: i32) -> Self {
        Self::new_9(ty, 0.5, 0.0, 1.0, 0.0, 1.0, 0, iu, iv)
    }

    /// `AdvApp2Var_Iso(type, cte, Ufirst, Ulast, Vfirst, Vlast, pos, iu, iv)`
    #[allow(clippy::too_many_arguments)]
    pub fn new_9(
        ty: IsoType,
        cte: f64,
        ufirst: f64,
        ulast: f64,
        vfirst: f64,
        vlast: f64,
        pos: i32,
        iu: i32,
        iv: i32,
    ) -> Self {
        // The orders follow the iso direction: for an IsoU, iu is the derivative order.
        let (extrem_order, deriv_order) = if ty == IsoType::IsoU { (iv, iu) } else { (iu, iv) };
        Self {
            ty,
            const_par: cte,
            u0: ufirst,
            u1: ulast,
            v0: vfirst,
            v1: vlast,
            position: pos,
            extrem_order,
            deriv_order,
            nb_coeff: 0,
            appr_is_done: false,
            has_result: false,
            max_errors: None,
            moy_errors: None,
        }
    }

    /// `IsApproximated()`
    pub fn is_approximated(&self) -> bool {
        self.appr_is_done
    }

    /// `HasResult()`
    pub fn has_result(&self) -> bool {
        self.has_result
    }

    /// `ChangeDomain(a, b)`: the parameter of the iso direction.
    pub fn change_domain_2(&mut self, a: f64, b: f64) {
        if self.ty == IsoType::IsoU {
            self.v0 = a;
            self.v1 = b;
        } else {
            self.u0 = a;
            self.u1 = b;
        }
    }

    /// `ChangeDomain(a, b, c, d)`
    pub fn change_domain_4(&mut self, a: f64, b: f64, c: f64, d: f64) {
        self.u0 = a;
        self.u1 = b;
        self.v0 = c;
        self.v1 = d;
    }

    /// `SetConstante(newcte)`
    pub fn set_constante(&mut self, newcte: f64) {
        self.const_par = newcte;
    }

    /// `SetPosition(newpos)`
    pub fn set_position(&mut self, newpos: i32) {
        self.position = newpos;
    }

    /// `ResetApprox()`
    pub fn reset_approx(&mut self) {
        self.appr_is_done = false;
        self.has_result = false;
    }

    /// `OverwriteApprox()`
    pub fn overwrite_approx(&mut self) {
        if self.has_result {
            self.appr_is_done = true;
        }
    }

    /// `Type()`
    pub fn type_(&self) -> IsoType {
        self.ty
    }

    /// `Constante()`
    pub fn constante(&self) -> f64 {
        self.const_par
    }

    /// `T0()`
    pub fn t0(&self) -> f64 {
        if self.ty == IsoType::IsoU { self.v0 } else { self.u0 }
    }

    /// `T1()`
    pub fn t1(&self) -> f64 {
        if self.ty == IsoType::IsoU { self.v1 } else { self.u1 }
    }

    /// `U0()`
    pub fn u0(&self) -> f64 {
        self.u0
    }

    /// `U1()`
    pub fn u1(&self) -> f64 {
        self.u1
    }

    /// `V0()`
    pub fn v0(&self) -> f64 {
        self.v0
    }

    /// `V1()`
    pub fn v1(&self) -> f64 {
        self.v1
    }

    /// `UOrder()`
    pub fn u_order(&self) -> i32 {
        if self.type_() == IsoType::IsoU { self.deriv_order } else { self.extrem_order }
    }

    /// `VOrder()`
    pub fn v_order(&self) -> i32 {
        if self.type_() == IsoType::IsoV { self.deriv_order } else { self.extrem_order }
    }

    /// `Position()`
    pub fn position(&self) -> i32 {
        self.position
    }

    /// `NbCoeff()`
    pub fn nb_coeff(&self) -> i32 {
        self.nb_coeff
    }

    /// `MaxErrors()->Value(row, col)`: a null handle (never computed) is an error here.
    pub(crate) fn max_error(&self, row: i32, col: i32) -> Result<f64> {
        self.max_errors.as_ref().ok_or(Error::NotComputed)?.get(row, col).copied()
    }

    /// `MoyErrors()->Value(row, col)`
    pub(crate) fn moy_error(&self, row: i32, col: i32) -> Result<f64> {
        self.moy_errors.as_ref().ok_or(Error::NotComputed)?.get(row, col).copied()
    }
}
