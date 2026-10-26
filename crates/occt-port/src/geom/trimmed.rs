//! `Geom_TrimmedCurve`.
use super::{BoundedCurve, Curve, GeomAbsShape, GeomError, Geometry, ResD1, ResD2, ResD3, Result};
use super::{computational, geometry_any, pconfusion};
use crate::elclib;
use crate::gp::{self, Pnt, Trsf};
use std::sync::Arc;

/// A mutable access to the curve behind a handle (`Arc<dyn Curve>` is immutable: a shared curve is copied first,
/// the aliasing of a C++ handle is not observable).
pub(crate) fn make_unique(a: &mut Arc<dyn Curve>) -> Option<&mut dyn Curve> {
    if Arc::get_mut(a).is_none() {
        *a = Arc::from(a.copy_curve());
    }
    Arc::get_mut(a)
}

/// `Geom_TrimmedCurve`.
#[derive(Debug, Clone)]
pub struct TrimmedCurve {
    basis_curve: Arc<dyn Curve>,
    u_trim1: f64,
    u_trim2: f64,
}

impl TrimmedCurve {
    /// `Geom_TrimmedCurve(C, U1, U2, Sense, theAdjustPeriodic)`; the basis curve is copied (a trimmed basis is replaced
    /// by a copy of its own basis).
    pub fn new(basis: Arc<dyn Curve>, u1: f64, u2: f64, sense: bool, adjust_periodic: bool) -> Result<Self> {
        let basis_curve: Arc<dyn Curve> = match basis.as_any().downcast_ref::<TrimmedCurve>() {
            Some(t) => Arc::from(t.basis_curve.copy_curve()),
            None => Arc::from(basis.copy_curve()),
        };
        let mut tc = TrimmedCurve { basis_curve, u_trim1: u1, u_trim2: u2 };
        tc.set_trim(u1, u2, sense, adjust_periodic)?;
        Ok(tc)
    }

    /// `SetTrim`.
    pub fn set_trim(&mut self, u1: f64, u2: f64, sense: bool, adjust_periodic: bool) -> Result<()> {
        if u1 == u2 {
            return Err(GeomError::Construction);
        }
        let udeb = self.basis_curve.first_parameter();
        let ufin = self.basis_curve.last_parameter();
        let same_sense;
        if self.basis_curve.is_periodic() {
            same_sense = sense;
            self.u_trim1 = u1;
            self.u_trim2 = u2;
            if adjust_periodic {
                let (a, b) = elclib::adjust_periodic(
                    udeb,
                    ufin,
                    ((self.u_trim2 - self.u_trim1).abs() / 2.0).min(pconfusion()),
                    self.u_trim1,
                    self.u_trim2,
                );
                self.u_trim1 = a;
                self.u_trim2 = b;
            }
        } else {
            if u1 < u2 {
                same_sense = sense;
                self.u_trim1 = u1;
                self.u_trim2 = u2;
            } else {
                same_sense = !sense;
                self.u_trim1 = u2;
                self.u_trim2 = u1;
            }
            if (udeb - self.u_trim1 > pconfusion()) || (self.u_trim2 - ufin > pconfusion()) {
                return Err(GeomError::Construction);
            }
        }
        if !same_sense {
            // (the exception of the `SetTrim` inside `Reverse` propagates from `SetTrim`)
            self.try_reverse()?;
        }
        Ok(())
    }

    // `Reverse` with its exception.
    fn try_reverse(&mut self) -> Result<()> {
        let u1 = self.basis_curve.reversed_parameter(self.u_trim2);
        let u2 = self.basis_curve.reversed_parameter(self.u_trim1);
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.reverse();
        }
        self.set_trim(u1, u2, true, false)
    }

    /// `BasisCurve`.
    pub fn basis_curve(&self) -> Arc<dyn Curve> {
        self.basis_curve.clone()
    }

    fn periodic_length(&self) -> Option<(f64, f64)> {
        if self.basis_curve.is_periodic()
            && let Ok(period) = self.basis_curve.period()
        {
            return Some((period, self.last_parameter() - self.first_parameter()));
        }
        None
    }
}

impl Geometry for TrimmedCurve {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.transform(t);
        }
        let u1 = self.basis_curve.transformed_parameter(self.u_trim1, t);
        let u2 = self.basis_curve.transformed_parameter(self.u_trim2, t);
        // SetTrim(U1, U2, true, false) cannot fail for a transformed trim of a valid curve; a failure leaves the trim as is.
        let _ = self.set_trim(u1, u2, true, false);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        match self.copy_trimmed() {
            Some(c) => Box::new(c),
            None => Box::new(self.clone()),
        }
    }
}

impl TrimmedCurve {
    // Copy(): new TrimmedCurve(basisCurve, uTrim1, uTrim2) (Sense and theAdjustPeriodic default to true)
    fn copy_trimmed(&self) -> Option<TrimmedCurve> {
        TrimmedCurve::new(self.basis_curve.clone(), self.u_trim1, self.u_trim2, true, true).ok()
    }
}

impl Curve for TrimmedCurve {
    fn copy_curve(&self) -> Box<dyn Curve> {
        match self.copy_trimmed() {
            Some(c) => Box::new(c),
            None => Box::new(self.clone()),
        }
    }
    fn reverse(&mut self) {
        let _ = self.try_reverse();
    }
    fn reversed_parameter(&self, u: f64) -> f64 {
        self.basis_curve.reversed_parameter(u)
    }
    fn transformed_parameter(&self, u: f64, t: &Trsf) -> f64 {
        self.basis_curve.transformed_parameter(u, t)
    }
    fn parametric_transformation(&self, t: &Trsf) -> f64 {
        self.basis_curve.parametric_transformation(t)
    }
    fn first_parameter(&self) -> f64 {
        self.u_trim1
    }
    fn last_parameter(&self) -> f64 {
        self.u_trim2
    }
    fn is_closed(&self) -> bool {
        if let Some((period, length)) = self.periodic_length()
            && length > pconfusion()
            && libm::remainder(length, period).abs() <= pconfusion()
        {
            return true;
        }
        match (self.value(self.first_parameter()), self.value(self.last_parameter())) {
            (Ok(a), Ok(b)) => a.square_distance(&b) <= computational(),
            _ => false,
        }
    }
    fn is_periodic(&self) -> bool {
        if let Some((period, length)) = self.periodic_length()
            && length > pconfusion()
            && libm::remainder(length, period).abs() <= pconfusion()
        {
            return true;
        }
        false
    }
    fn period(&self) -> Result<f64> {
        self.basis_curve.period()
    }
    fn continuity(&self) -> GeomAbsShape {
        self.basis_curve.continuity()
    }
    fn is_cn(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_curve.is_cn(n)
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        self.basis_curve.eval_d0(u)
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        self.basis_curve.eval_d1(u)
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        self.basis_curve.eval_d2(u)
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        self.basis_curve.eval_d3(u)
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec> {
        self.basis_curve.eval_dn(u, n)
    }
}

impl BoundedCurve for TrimmedCurve {
    fn start_point(&self) -> Pnt {
        self.basis_curve.value(self.u_trim1).unwrap_or_else(|_| Pnt::origin())
    }
    fn end_point(&self) -> Pnt {
        self.basis_curve.value(self.u_trim2).unwrap_or_else(|_| Pnt::origin())
    }
}
