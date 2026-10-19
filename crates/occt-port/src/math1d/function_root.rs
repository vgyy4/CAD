//! `math_FunctionRoot`: a root near a guess, computed by `math_FunctionSetRoot` on one variable.
#![allow(clippy::float_cmp)]

use super::function_set_root::{DirFunction, FunctionSetRoot, OneVarSet};
use super::{FunctionWithDerivative, REAL_FIRST, REAL_LAST, fmt_g};
use crate::mathlin::{MathError, Vector};

/// `math_FunctionRoot`.
#[derive(Debug, Clone)]
pub struct FunctionRoot {
    done: bool,
    the_root: f64,
    the_error: f64,
    the_derivative: f64,
    nb_iter: i32,
}

impl FunctionRoot {
    /// First constructor `(F, Guess, Tolerance, NbIterations = 100)`.
    pub fn new(
        f: &mut dyn FunctionWithDerivative,
        guess: f64,
        tolerance: f64,
        nb_iterations: i32,
    ) -> Result<Self, MathError> {
        Self::solve(f, guess, tolerance, REAL_FIRST, REAL_LAST, nb_iterations)
    }

    /// Second constructor `(F, Guess, Tolerance, A, B, NbIterations = 100)`.
    pub fn new_bounded(
        f: &mut dyn FunctionWithDerivative,
        guess: f64,
        tolerance: f64,
        a: f64,
        b: f64,
        nb_iterations: i32,
    ) -> Result<Self, MathError> {
        Self::solve(f, guess, tolerance, a, b, nb_iterations)
    }

    /// Without bounds `Perform(Ff, V)` runs with `InfBound = RealFirst()` and `SupBound = RealLast()`.
    fn solve(
        f: &mut dyn FunctionWithDerivative,
        guess: f64,
        tolerance: f64,
        a: f64,
        b: f64,
        nb_iterations: i32,
    ) -> Result<Self, MathError> {
        let mut v = Vector::new(1, 1)?;
        let mut tol = Vector::new(1, 1)?;
        let mut aa = Vector::new(1, 1)?;
        let mut bb = Vector::new(1, 1)?;
        v.set_value(1, guess)?;
        tol.set_value(1, tolerance)?;
        aa.set_value(1, a)?;
        bb.set_value(1, b)?;
        let mut sol = FunctionSetRoot::new(&tol, nb_iterations)?;
        {
            let mut f_dir = DirFunction::new(OneVarSet { ff: &mut *f })?;
            sol.perform(&mut f_dir, &v, &aa, &bb)?;
        }
        let mut r = Self { done: sol.is_done(), the_root: 0.0, the_error: 0.0, the_derivative: 0.0, nb_iter: 0 };
        if r.done {
            r.nb_iter = sol.nb_iterations()?;
            f.state_number();
            r.the_root = sol.root()?;
            r.the_derivative = sol.derivative()?;
            if let Some(val) = f.value(r.the_root) {
                r.the_error = val;
            }
        }
        Ok(r)
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    /// `Root`.
    pub fn root(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.the_root)
    }
    /// `Derivative`.
    pub fn derivative(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.the_derivative)
    }
    /// `Value`.
    pub fn value(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.the_error)
    }
    /// `NbIterations`.
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.nb_iter)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_FunctionRoot ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Number of iterations = {}\n", self.nb_iter));
            s.push_str(&format!(" The Root is: {}\n", fmt_g(self.the_root)));
            s.push_str(&format!("The value at the root is: {}\n", fmt_g(self.the_error)));
        } else {
            s.push_str(" Status = not Done \n");
        }
        s
    }
}
