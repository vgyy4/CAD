//! `CSLib_NormalPolyDef`: the polynomial in `cos` and `sin` whose roots separate the sign changes of
//! the normal in `CSLib::Normal` with derivatives.
use crate::{
    math1d::{Function, FunctionWithDerivative},
    mathlin::MathError,
    plib,
};

/// `RealSmall()`.
const REAL_SMALL: f64 = f64::MIN_POSITIVE;

/// `CSLib_NormalPolyDef`.
#[derive(Debug, Clone)]
pub struct NormalPolyDef {
    k0: i32,
    /// `myTABli(0..=k0)`.
    ta_bli: Vec<f64>,
}

impl NormalPolyDef {
    /// `CSLib_NormalPolyDef(theK0, theLi)`; `li[k]` has the C++ index `li_lower + k`.
    pub fn new(k0: i32, li_lower: i32, li: &[f64]) -> Result<Self, MathError> {
        if k0 < 0 || li_lower > 0 {
            return Err(MathError::Range);
        }
        let mut ta_bli = Vec::new();
        for i in 0..=k0 {
            let offset = usize::try_from(i - li_lower).map_err(|_| MathError::Range)?;
            ta_bli.push(*li.get(offset).ok_or(MathError::Range)?);
        }
        Ok(Self { k0, ta_bli })
    }

    fn coefficient(&self, i: i32) -> Result<f64, MathError> {
        let idx = usize::try_from(i).map_err(|_| MathError::Range)?;
        self.ta_bli.get(idx).copied().ok_or(MathError::Range)
    }

    fn eval_value(&self, x: f64) -> Result<f64, MathError> {
        let mut f = 0.0;
        let cos = libm::cos(x);
        let sin = libm::sin(x);
        if cos.abs() <= REAL_SMALL || sin.abs() <= REAL_SMALL {
            return Ok(f);
        }
        for i in 0..=self.k0 {
            f += plib::bin(self.k0, i).map_err(|_| MathError::Range)?
                * libm::pow(cos, f64::from(i))
                * libm::pow(sin, f64::from(self.k0 - i))
                * self.coefficient(i)?;
        }
        Ok(f)
    }

    fn eval_derivative(&self, x: f64) -> Result<f64, MathError> {
        let mut d = 0.0;
        let cos = libm::cos(x);
        let sin = libm::sin(x);
        if cos.abs() <= REAL_SMALL || sin.abs() <= REAL_SMALL {
            return Ok(d);
        }
        for i in 0..=self.k0 {
            d += plib::bin(self.k0, i).map_err(|_| MathError::Range)?
                * libm::pow(cos, f64::from(i - 1))
                * libm::pow(sin, f64::from(self.k0 - i - 1))
                * (f64::from(self.k0) * cos * cos - f64::from(i))
                * self.coefficient(i)?;
        }
        Ok(d)
    }

    fn eval_values(&self, x: f64) -> Result<(f64, f64), MathError> {
        let mut f = 0.0;
        let mut d = 0.0;
        let cos = libm::cos(x);
        let sin = libm::sin(x);
        if cos.abs() <= REAL_SMALL || sin.abs() <= REAL_SMALL {
            return Ok((f, d));
        }
        for i in 0..=self.k0 {
            let bin_coeff = plib::bin(self.k0, i).map_err(|_| MathError::Range)?;
            let li_coeff = self.coefficient(i)?;
            f += bin_coeff * libm::pow(cos, f64::from(i)) * libm::pow(sin, f64::from(self.k0 - i)) * li_coeff;
            d += bin_coeff
                * libm::pow(cos, f64::from(i - 1))
                * libm::pow(sin, f64::from(self.k0 - i - 1))
                * (f64::from(self.k0) * cos * cos - f64::from(i))
                * li_coeff;
        }
        Ok((f, d))
    }
}

impl Function for NormalPolyDef {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.eval_value(x).ok()
    }
}

impl FunctionWithDerivative for NormalPolyDef {
    fn derivative(&mut self, x: f64) -> Option<f64> {
        self.eval_derivative(x).ok()
    }
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        self.eval_values(x).ok()
    }
}
