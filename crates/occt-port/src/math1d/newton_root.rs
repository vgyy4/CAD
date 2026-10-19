//! `math_NewtonFunctionRoot`: Newton's method with bounds.
#![allow(clippy::float_cmp)]

use super::{FunctionWithDerivative, REAL_FIRST, REAL_LAST, fmt_g};
use crate::mathlin::MathError;

/// `math_NewtonFunctionRoot`.
#[derive(Debug, Clone)]
pub struct NewtonFunctionRoot {
    done: bool,
    x: f64,
    fx: f64,
    dfx: f64,
    it: i32,
    epsilon_x: f64,
    epsilon_f: f64,
    itermax: i32,
    binf: f64,
    bsup: f64,
}

impl NewtonFunctionRoot {
    fn blank(a: f64, b: f64, eps_x: f64, eps_f: f64, nb_iterations: i32) -> Self {
        Self {
            done: false,
            x: REAL_LAST,
            fx: REAL_LAST,
            dfx: 0.0,
            it: 0,
            epsilon_x: eps_x,
            epsilon_f: eps_f,
            itermax: nb_iterations,
            binf: a,
            bsup: b,
        }
    }

    /// `(F, Guess, EpsX, EpsF, NbIterations = 100)`: bounds `RealFirst()`, `RealLast()`.
    pub fn new(f: &mut dyn FunctionWithDerivative, guess: f64, eps_x: f64, eps_f: f64, nb_iterations: i32) -> Self {
        Self::new_bounded(f, guess, eps_x, eps_f, REAL_FIRST, REAL_LAST, nb_iterations)
    }

    /// `(F, Guess, EpsX, EpsF, A, B, NbIterations = 100)`.
    pub fn new_bounded(
        f: &mut dyn FunctionWithDerivative,
        guess: f64,
        eps_x: f64,
        eps_f: f64,
        a: f64,
        b: f64,
        nb_iterations: i32,
    ) -> Self {
        let mut s = Self::blank(a, b, eps_x, eps_f, nb_iterations);
        s.perform(f, guess);
        s
    }

    /// `(A, B, EpsX, EpsF, NbIterations = 100)`: does not solve.
    pub fn without_function(a: f64, b: f64, eps_x: f64, eps_f: f64, nb_iterations: i32) -> Self {
        Self::blank(a, b, eps_x, eps_f, nb_iterations)
    }

    /// `Perform`.
    pub fn perform(&mut self, f: &mut dyn FunctionWithDerivative, guess: f64) {
        let mut best_x = self.x;
        let mut best_fx = REAL_LAST;
        let (aa, bb) = if self.binf < self.bsup { (self.binf, self.bsup) } else { (self.bsup, self.binf) };

        let mut dx = REAL_LAST;
        self.fx = REAL_LAST;
        self.x = guess;
        self.it = 1;
        while self.it <= self.itermax && (dx.abs() > self.epsilon_x || self.fx.abs() > self.epsilon_f) {
            let ok = match f.values(self.x) {
                Some((v, d)) => {
                    self.fx = v;
                    self.dfx = d;
                    true
                }
                None => false,
            };

            let mut abs_fx = self.fx;
            if abs_fx < 0.0 {
                abs_fx = -abs_fx;
            }
            if abs_fx < best_fx {
                best_fx = abs_fx;
                best_x = self.x;
            }

            if ok {
                if self.dfx == 0.0 {
                    self.done = false;
                    self.it = self.itermax.wrapping_add(1);
                } else {
                    dx = self.fx / self.dfx;
                    self.x -= dx;
                    // limit the variation of X
                    if self.x <= aa {
                        self.x = aa;
                    }
                    if self.x >= bb {
                        self.x = bb;
                    }
                    self.it = self.it.wrapping_add(1);
                }
            } else {
                self.done = false;
                self.it = self.itermax.wrapping_add(1);
            }
        }
        self.x = best_x;

        self.done = self.it <= self.itermax;
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
        Ok(self.x)
    }
    /// `Derivative`.
    pub fn derivative(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.dfx)
    }
    /// `Value`.
    pub fn value(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.fx)
    }
    /// `NbIterations`.
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.it)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_NewtonFunctionRoot ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Location found = {}\n", fmt_g(self.x)));
            s.push_str(&format!(" function value at this minimum = {}\n", fmt_g(self.fx)));
            s.push_str(&format!(" Number of iterations = {}\n", self.it));
        } else {
            s.push_str("Status = not Done \n");
        }
        s
    }
}
