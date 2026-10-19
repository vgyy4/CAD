//! `math_BrentMinimum`: Brent's method for the minimum of a function of one variable.
#![allow(clippy::float_cmp)]

use super::{Function, REAL_LAST, fmt_g};
use crate::mathlin::MathError;

const CGOLD: f64 = 0.3819660;

/// The protected state of `math_BrentMinimum` that a sub-class sees in `IsSolutionReached`.
#[derive(Clone, Copy, Debug)]
pub struct BrentState {
    /// `a`.
    pub a: f64,
    /// `b`.
    pub b: f64,
    /// `x`.
    pub x: f64,
    /// `fx`.
    pub fx: f64,
    /// `fv`.
    pub fv: f64,
    /// `fw`.
    pub fw: f64,
    /// `XTol`.
    pub x_tol: f64,
    /// `EPSZ`.
    pub eps_z: f64,
}

impl BrentState {
    /// The inline `math_BrentMinimum::IsSolutionReached`.
    pub fn is_solution_reached_default(&self) -> bool {
        let two_tol = 2.0 * (self.x_tol * self.x.abs() + self.eps_z);
        (self.x <= (two_tol + self.a)) && (self.x >= (self.b - two_tol))
    }
}

/// A redefinition of the virtual `IsSolutionReached`.
pub trait BrentSolutionTest {
    /// Called where the C++ calls `IsSolutionReached(F)`, with the current state.
    fn is_solution_reached(&mut self, state: &BrentState, f: &mut dyn Function) -> bool;
}

struct DefaultTest;

impl BrentSolutionTest for DefaultTest {
    fn is_solution_reached(&mut self, state: &BrentState, _f: &mut dyn Function) -> bool {
        state.is_solution_reached_default()
    }
}

/// `math_BrentMinimum`.
#[derive(Debug, Clone)]
pub struct BrentMinimum {
    st: BrentState,
    done: bool,
    iter: i32,
    itermax: i32,
    my_f: bool,
}

impl BrentMinimum {
    /// `(TolX, NbIterations = 100, ZEPS = 1.0e-12)`.
    pub fn new(tol_x: f64, nb_iterations: i32, zeps: f64) -> Self {
        Self {
            st: BrentState { a: 0.0, b: 0.0, x: 0.0, fx: 0.0, fv: 0.0, fw: 0.0, x_tol: tol_x, eps_z: zeps },
            done: false,
            iter: 0,
            itermax: nb_iterations,
            my_f: false,
        }
    }

    /// `(TolX, Fbx, NbIterations = 100, ZEPS = 1.0e-12)`: `F(Bx)` is not computed.
    pub fn with_fbx(tol_x: f64, fbx: f64, nb_iterations: i32, zeps: f64) -> Self {
        let mut s = Self::new(tol_x, nb_iterations, zeps);
        s.st.fx = fbx;
        s.my_f = true;
        s
    }

    /// `Perform` with the default `IsSolutionReached`.
    pub fn perform(&mut self, f: &mut dyn Function, ax: f64, bx: f64, cx: f64) {
        self.perform_with_test(f, ax, bx, cx, &mut DefaultTest);
    }

    /// `Perform` of a sub-class that redefines `IsSolutionReached`.
    pub fn perform_with_test(
        &mut self,
        f: &mut dyn Function,
        ax: f64,
        bx: f64,
        cx: f64,
        test: &mut dyn BrentSolutionTest,
    ) {
        let (mut fu, mut p, mut q, mut r);
        let (mut u, mut v, mut w);
        let mut e = 0.0_f64;
        let mut d = REAL_LAST;

        self.st.a = if ax < cx { ax } else { cx };
        self.st.b = if ax > cx { ax } else { cx };
        self.st.x = bx;
        w = bx;
        v = bx;
        if !self.my_f {
            match f.value(self.st.x) {
                Some(val) => self.st.fx = val,
                None => return,
            }
        }
        self.st.fv = self.st.fx;
        self.st.fw = self.st.fx;
        self.iter = 1;
        while self.iter <= self.itermax {
            let xm = 0.5 * (self.st.a + self.st.b);
            let tol1 = self.st.x_tol * self.st.x.abs() + self.st.eps_z;
            let tol2 = 2.0 * tol1;
            if test.is_solution_reached(&self.st, f) {
                self.done = true;
                return;
            }
            if e.abs() > tol1 {
                r = (self.st.x - w) * (self.st.fx - self.st.fv);
                q = (self.st.x - v) * (self.st.fx - self.st.fw);
                p = (self.st.x - v) * q - (self.st.x - w) * r;
                q = 2.0 * (q - r);
                if q > 0.0 {
                    p = -p;
                }
                q = q.abs();
                let etemp = e;
                e = d;
                if p.abs() >= (0.5 * q * etemp).abs()
                    || p <= q * (self.st.a - self.st.x)
                    || p >= q * (self.st.b - self.st.x)
                {
                    e = if self.st.x >= xm { self.st.a - self.st.x } else { self.st.b - self.st.x };
                    d = CGOLD * e;
                } else {
                    d = p / q;
                    u = self.st.x + d;
                    if u - self.st.a < tol2 || self.st.b - u < tol2 {
                        d = tol1.copysign(xm - self.st.x);
                    }
                }
            } else {
                e = if self.st.x >= xm { self.st.a - self.st.x } else { self.st.b - self.st.x };
                d = CGOLD * e;
            }
            u = if d.abs() >= tol1 { self.st.x + d } else { self.st.x + tol1.copysign(d) };
            match f.value(u) {
                Some(val) => fu = val,
                None => return,
            }
            if fu <= self.st.fx {
                if u >= self.st.x {
                    self.st.a = self.st.x;
                } else {
                    self.st.b = self.st.x;
                }
                v = w;
                w = self.st.x;
                self.st.x = u;
                self.st.fv = self.st.fw;
                self.st.fw = self.st.fx;
                self.st.fx = fu;
            } else {
                if u < self.st.x {
                    self.st.a = u;
                } else {
                    self.st.b = u;
                }
                if fu <= self.st.fw || w == self.st.x {
                    v = w;
                    w = u;
                    self.st.fv = self.st.fw;
                    self.st.fw = fu;
                } else if fu <= self.st.fv || v == self.st.x || v == w {
                    v = u;
                    self.st.fv = fu;
                }
            }
            self.iter = self.iter.wrapping_add(1);
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
    /// `Location`.
    pub fn location(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.st.x)
    }
    /// `Minimum`.
    pub fn minimum(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.st.fx)
    }
    /// `NbIterations`.
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.iter)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_BrentMinimum ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Location value = {}\n", fmt_g(self.st.x)));
            s.push_str(&format!(" Minimum value = {}\n", fmt_g(self.st.fx)));
            s.push_str(&format!(" Number of iterations = {}\n", self.iter));
        } else {
            s.push_str(" Status = not Done \n");
        }
        s
    }
}
