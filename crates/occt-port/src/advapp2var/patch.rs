//! `AdvApp2Var_Patch`: one rectangular patch of the network, with its error estimates.

use super::{Arr1, Arr2, Error, Framework, Result, fmax, hmax};

/// `AdvApp2Var_Patch`
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
    ord_in_u: i32,
    ord_in_v: i32,
    nb_coeff_in_u: i32,
    nb_coeff_in_v: i32,
    appr_is_done: bool,
    has_result: bool,
    max_errors: Option<Arr1<f64>>,
    moy_errors: Option<Arr1<f64>>,
    iso_errors: Option<Arr2<f64>>,
    cut_sense: i32,
    disc_is_done: bool,
    crit_value: f64,
}

impl Patch {
    /// `AdvApp2Var_Patch::AdvApp2Var_Patch()`
    pub fn new_0() -> Self {
        Self {
            u0: 0.0,
            u1: 1.0,
            v0: 0.0,
            v1: 1.0,
            ord_in_u: 0,
            ord_in_v: 0,
            nb_coeff_in_u: 0,
            nb_coeff_in_v: 0,
            appr_is_done: false,
            has_result: false,
            max_errors: None,
            moy_errors: None,
            iso_errors: None,
            cut_sense: 0,
            disc_is_done: false,
            crit_value: 0.0,
        }
    }

    /// `AdvApp2Var_Patch::AdvApp2Var_Patch(U0, U1, V0, V1, iu, iv)`
    pub fn new_6(u0: f64, u1: f64, v0: f64, v1: f64, iu: i32, iv: i32) -> Self {
        Self { u0, u1, v0, v1, ord_in_u: iu, ord_in_v: iv, ..Self::new_0() }
    }

    /// `IsDiscretised()`
    pub fn is_discretised(&self) -> bool {
        self.disc_is_done
    }

    /// `IsApproximated()`
    pub fn is_approximated(&self) -> bool {
        self.appr_is_done
    }

    /// `HasResult()`
    pub fn has_result(&self) -> bool {
        self.has_result
    }

    /// `AddErrors(Constraints)`: the error estimates of the patch from the frame of its four
    /// borders and corner nodes. Fails where the C++ raises (a missing iso or node of the
    /// frame) and where it dereferences a handle that was never computed.
    pub fn add_errors(&mut self, constraints: &Framework) -> Result<()> {
        let nbsesp: i32 = 1;
        let (u0, u1, v0, v1) = (self.u0, self.u1, self.v0, self.v1);
        let (ou, ov) = (self.ord_in_u, self.ord_in_v);
        let hmax_v = hmax(ov)?;
        let hmax_u = hmax(ou)?;
        for iesp in 1..=nbsesp {
            let mut err_u = 0.0_f64;
            for iv in 1..=ov + 1 {
                let error = constraints.iso_v(u0, u1, v0)?.max_error(iesp, iv)?;
                err_u = fmax(err_u, error);
                let error = constraints.iso_v(u0, u1, v1)?.max_error(iesp, iv)?;
                err_u = fmax(err_u, error);
            }
            let mut err_v = 0.0_f64;
            for iu in 1..=ou + 1 {
                let error = constraints.iso_u(u0, v0, v1)?.max_error(iesp, iu)?;
                err_v = fmax(err_v, error);
                let error = constraints.iso_u(u1, v0, v1)?.max_error(iesp, iu)?;
                err_v = fmax(err_v, error);
            }
            let max_errors = self.max_errors.as_mut().ok_or(Error::NotComputed)?;
            *max_errors.get_mut(iesp)? += err_u * hmax_v + err_v * hmax_u;

            let mut err_u = 0.0_f64;
            for iv in 1..=ov + 1 {
                let error = constraints.iso_v(u0, u1, v0)?.moy_error(iesp, iv)?;
                err_u = fmax(err_u, error);
                let error = constraints.iso_v(u0, u1, v1)?.moy_error(iesp, iv)?;
                err_u = fmax(err_u, error);
            }
            let mut err_v = 0.0_f64;
            for iu in 1..=ou + 1 {
                let error = constraints.iso_u(u0, v0, v1)?.moy_error(iesp, iu)?;
                err_v = fmax(err_v, error);
                let error = constraints.iso_u(u1, v0, v1)?.moy_error(iesp, iu)?;
                err_v = fmax(err_v, error);
            }
            let moy_errors = self.moy_errors.as_mut().ok_or(Error::NotComputed)?;
            let mut error = *moy_errors.get(iesp)?;
            error *= error;
            error += err_u * hmax_v * err_u * hmax_v + err_v * hmax_u * err_v * hmax_u;
            moy_errors.set(iesp, error.sqrt())?;

            // HERISO(1..NBSESP, 1..4) with the four borders of the patch
            let mut heriso = Arr2::new(1, nbsesp, 1, 4, 0.0_f64);
            heriso.set(iesp, 1, constraints.iso_v(u0, u1, v0)?.max_error(iesp, 1)?)?;
            heriso.set(iesp, 2, constraints.iso_v(u0, u1, v1)?.max_error(iesp, 1)?)?;
            heriso.set(iesp, 3, constraints.iso_u(u0, v0, v1)?.max_error(iesp, 1)?)?;
            heriso.set(iesp, 4, constraints.iso_u(u1, v0, v1)?.max_error(iesp, 1)?)?;

            let (mut emax1, mut emax2, mut emax3, mut emax4) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
            for iu in 0..=ou {
                for iv in 0..=ov {
                    let error = constraints.node(u0, v0)?.error(iu, iv)?;
                    emax1 = fmax(emax1, error);
                    let error = constraints.node(u1, v0)?.error(iu, iv)?;
                    emax2 = fmax(emax2, error);
                    let error = constraints.node(u0, v1)?.error(iu, iv)?;
                    emax3 = fmax(emax3, error);
                    let error = constraints.node(u1, v1)?.error(iu, iv)?;
                    emax4 = fmax(emax4, error);
                }
            }
            let err1 = fmax(emax1, emax2);
            let err2 = fmax(emax3, emax4);
            let err3 = fmax(emax1, emax3);
            let err4 = fmax(emax2, emax4);
            if constraints.iso_v(u0, u1, v0)?.position() == 0 {
                let c = heriso.get_mut(iesp, 1)?;
                *c += err1 * hmax_u;
            }
            if constraints.iso_v(u0, u1, v1)?.position() == 0 {
                let c = heriso.get_mut(iesp, 2)?;
                *c += err2 * hmax_u;
            }
            if constraints.iso_u(u0, v0, v1)?.position() == 0 {
                let c = heriso.get_mut(iesp, 3)?;
                *c += err3 * hmax_v;
            }
            if constraints.iso_u(u1, v0, v1)?.position() == 0 {
                let c = heriso.get_mut(iesp, 4)?;
                *c += err4 * hmax_v;
            }
            self.iso_errors = Some(heriso);
        }
        Ok(())
    }

    /// `ChangeDomain(a, b, c, d)`
    pub fn change_domain(&mut self, a: f64, b: f64, c: f64, d: f64) {
        self.u0 = a;
        self.u1 = b;
        self.v0 = c;
        self.v1 = d;
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
        self.ord_in_u
    }

    /// `VOrder()`
    pub fn v_order(&self) -> i32 {
        self.ord_in_v
    }

    /// `CutSense()`
    pub fn cut_sense_0(&self) -> i32 {
        self.cut_sense
    }

    /// `NbCoeffInU()`
    pub fn nb_coeff_in_u(&self) -> i32 {
        self.nb_coeff_in_u
    }

    /// `NbCoeffInV()`
    pub fn nb_coeff_in_v(&self) -> i32 {
        self.nb_coeff_in_v
    }

    /// `ChangeNbCoeff(NbCoeffU, NbCoeffV)`: only grows the counts.
    pub fn change_nb_coeff(&mut self, nb_coeff_u: i32, nb_coeff_v: i32) {
        if self.nb_coeff_in_u < nb_coeff_u {
            self.nb_coeff_in_u = nb_coeff_u;
        }
        if self.nb_coeff_in_v < nb_coeff_v {
            self.nb_coeff_in_v = nb_coeff_v;
        }
    }

    /// `CritValue()`
    pub fn crit_value(&self) -> f64 {
        self.crit_value
    }

    /// `SetCritValue(dist)`
    pub fn set_crit_value(&mut self, dist: f64) {
        self.crit_value = dist;
    }
}
