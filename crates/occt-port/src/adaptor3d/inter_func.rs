//! `Adaptor3d_InterFunc`: the function `C(t).X - FixVal` (or `.Y`) of a 2D curve adaptor, whose roots give the
//! intersections of the curve with an iso line of a surface.
//!
//! Translated from `Adaptor3d_InterFunc.hxx/.cxx` (OCCT 8.0.1). The math callbacks return `None` where the C++ callback
//! would throw (an adaptor that fails).
use super::Result;
use crate::adaptor2d::{AdaptorError, Curve2dAdaptor};
use crate::math1d::{Function, FunctionWithDerivative};
use std::sync::Arc;

/// `Adaptor3d_InterFunc`.
#[derive(Debug, Clone)]
pub struct InterFunc {
    curve: Arc<dyn Curve2dAdaptor>,
    fix_val: f64,
    fix: i32,
}

impl InterFunc {
    /// `Adaptor3d_InterFunc(C, FixVal, Fix)`: `Fix` is 1 (the X coordinate) or 2 (the Y coordinate), else `Err(Construction)`.
    pub fn new(c: Arc<dyn Curve2dAdaptor>, fix_val: f64, fix: i32) -> Result<Self> {
        if fix != 1 && fix != 2 {
            return Err(AdaptorError::Construction);
        }
        Ok(Self { curve: c, fix_val, fix })
    }
}

impl Function for InterFunc {
    fn value(&mut self, x: f64) -> Option<f64> {
        let c = self.curve.d0(x).ok()?;
        Some(if self.fix == 1 { c.x() - self.fix_val } else { c.y() - self.fix_val })
    }
}

impl FunctionWithDerivative for InterFunc {
    fn derivative(&mut self, x: f64) -> Option<f64> {
        self.values(x).map(|r| r.1)
    }

    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        let (c, dc) = self.curve.d1(x).ok()?;
        Some(if self.fix == 1 { (c.x() - self.fix_val, dc.x()) } else { (c.y() - self.fix_val, dc.y()) })
    }
}
