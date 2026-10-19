//! `math_BracketedRoot`: Brent's method on a bracketed root (Numerical Recipes in C, p. 269).
#![allow(clippy::float_cmp)]

use super::{Function, fmt_g};
use crate::mathlin::MathError;

/// `math_BracketedRoot`.
#[derive(Debug, Clone)]
pub struct BracketedRoot {
    done: bool,
    the_root: f64,
    the_error: f64,
    nb_iter: i32,
}

impl BracketedRoot {
    /// The constructor (C++ defaults: `nb_iterations = 100`, `zeps = 1.0e-12`).
    pub fn new(f: &mut dyn Function, bound1: f64, bound2: f64, tolerance: f64, nb_iterations: i32, zeps: f64) -> Self {
        let mut s = Self { done: false, the_root: bound2, the_error: 0.0, nb_iter: 0 };
        s.run(f, bound1, tolerance, nb_iterations, zeps);
        s
    }

    fn run(&mut self, f: &mut dyn Function, bound1: f64, tolerance: f64, nb_iterations: i32, zeps: f64) {
        let mut fa = 0.0_f64;
        let mut fc;
        let (mut c, mut d, mut e) = (0.0_f64, 0.0_f64, 0.0_f64);
        let mut a = bound1;
        if let Some(v) = f.value(a) {
            fa = v;
        }
        if let Some(v) = f.value(self.the_root) {
            self.the_error = v;
        }
        if fa * self.the_error > 0.0 {
            self.done = false;
            return;
        }
        fc = self.the_error;
        self.nb_iter = 1;
        while self.nb_iter <= nb_iterations {
            if self.the_error * fc > 0.0 {
                c = a; // rename a TheRoot c and adjust bounding interval d
                fc = fa;
                d = self.the_root - a;
                e = d;
            }
            if fc.abs() < fa.abs() {
                a = self.the_root;
                self.the_root = c;
                c = a;
                fa = self.the_error;
                self.the_error = fc;
                fc = fa;
            }
            let tol1 = 2.0 * zeps * self.the_root.abs() + 0.5 * tolerance; // convergence check
            let xm = 0.5 * (c - self.the_root);
            if xm.abs() <= tol1 || self.the_error == 0.0 {
                self.done = true;
                return;
            }
            if e.abs() >= tol1 && fa.abs() > self.the_error.abs() {
                let s = self.the_error / fa; // attempt inverse quadratic interpolation
                let (mut p, mut q);
                if a == c {
                    p = 2.0 * xm * s;
                    q = 1.0 - s;
                } else {
                    q = fa / fc;
                    let r = self.the_error / fc;
                    p = s * (2.0 * xm * q * (q - r) - (self.the_root - a) * (r - 1.0));
                    q = (q - 1.0) * (r - 1.0) * (s - 1.0);
                }
                if p > 0.0 {
                    q = -q;
                }
                p = p.abs();
                let min1 = 3.0 * xm * q - (tol1 * q).abs();
                let min2 = (e * q).abs();
                if 2.0 * p < (if min1 < min2 { min1 } else { min2 }) {
                    e = d; // accept interpolation
                    d = p / q;
                } else {
                    d = xm; // interpolation failed, use bisection
                    e = d;
                }
            } else {
                // bounds decreasing too slowly, use bisection
                d = xm;
                e = d;
            }
            a = self.the_root; // move last best guess to a
            fa = self.the_error;
            if d.abs() > tol1 {
                self.the_root += d;
            } else {
                self.the_root += if xm > 0.0 { tol1.abs() } else { -tol1.abs() };
            }
            if let Some(v) = f.value(self.the_root) {
                self.the_error = v;
            }
            self.nb_iter = self.nb_iter.wrapping_add(1);
        }
        self.done = false;
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
    /// `Value`: the value of the function at the root.
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
        let mut s = String::from("math_BracketedRoot ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Number of iterations = {}\n", self.nb_iter));
            s.push_str(&format!(" The Root is: {}\n", fmt_g(self.the_root)));
            s.push_str(&format!(" The value at the root is: {}\n", fmt_g(self.the_error)));
        } else {
            s.push_str(" Status = not Done \n");
        }
        s
    }
}
