//! `math_BracketMinimum`: bracketing a minimum (golden section with parabolic extrapolation).
#![allow(clippy::float_cmp)]

use super::{Function, P_CONFUSION, PRECISION_INFINITE, fmt_g};
use crate::mathlin::MathError;

const GOLD: f64 = 1.618034;
const GLIMIT: f64 = 100.0;
const TINY: f64 = 1.0e-20;
/// Bracketing steps before `perform` gives up (not in OCCT).
const MAX_STEPS: u32 = 100_000;

/// `math_BracketMinimum`.
#[derive(Debug, Clone)]
pub struct BracketMinimum {
    done: bool,
    ax: f64,
    bx: f64,
    cx: f64,
    fax: f64,
    fbx: f64,
    fcx: f64,
    left: f64,
    right: f64,
    is_limited: bool,
    fa_set: bool,
    fb_set: bool,
}

/// `Limited`: the value within `[left, right]`.
fn limited(left: f64, right: f64, value: f64) -> f64 {
    if value < left {
        left
    } else if value > right {
        right
    } else {
        value
    }
}

/// `LimitAndMayBeSwap`.
#[allow(clippy::too_many_arguments)]
fn limit_and_may_be_swap(
    f: &mut dyn Function,
    left: f64,
    right: f64,
    a: f64,
    b: &mut f64,
    fb: &mut f64,
    c: &mut f64,
    fc: &mut f64,
) -> bool {
    *c = limited(left, right, *c);
    if (*b - *c).abs() < P_CONFUSION {
        return false;
    }
    match f.value(*c) {
        Some(v) => *fc = v,
        None => return false,
    }
    // check that B is between A and C
    if (a - *b) * (*b - *c) < 0.0 {
        // swap B and C
        std::mem::swap(b, c);
        std::mem::swap(fb, fc);
    }
    true
}

impl BracketMinimum {
    /// `math_BracketMinimum(A, B)`: does not run.
    pub fn new(a: f64, b: f64) -> Self {
        Self {
            done: false,
            ax: a,
            bx: b,
            cx: 0.0,
            fax: 0.0,
            fbx: 0.0,
            fcx: 0.0,
            left: -PRECISION_INFINITE,
            right: PRECISION_INFINITE,
            is_limited: false,
            fa_set: false,
            fb_set: false,
        }
    }

    /// `(F, A, B)`: constructs and runs `Perform`.
    pub fn with_function(f: &mut dyn Function, a: f64, b: f64) -> Self {
        let mut s = Self::new(a, b);
        s.perform(f);
        s
    }

    /// `(F, A, B, FA)`.
    pub fn with_function_fa(f: &mut dyn Function, a: f64, b: f64, fa: f64) -> Self {
        let mut s = Self::new(a, b);
        s.fax = fa;
        s.fa_set = true;
        s.perform(f);
        s
    }

    /// `(F, A, B, FA, FB)`.
    pub fn with_function_fa_fb(f: &mut dyn Function, a: f64, b: f64, fa: f64, fb: f64) -> Self {
        let mut s = Self::new(a, b);
        s.fax = fa;
        s.fbx = fb;
        s.fa_set = true;
        s.fb_set = true;
        s.perform(f);
        s
    }

    /// `SetLimits`.
    pub fn set_limits(&mut self, left: f64, right: f64) {
        self.left = left;
        self.right = right;
        self.is_limited = true;
    }

    /// `SetFA`.
    pub fn set_fa(&mut self, value: f64) {
        self.fax = value;
        self.fa_set = true;
    }

    /// `SetFB`.
    pub fn set_fb(&mut self, value: f64) {
        self.fbx = value;
        self.fb_set = true;
    }

    /// `Perform`.
    pub fn perform(&mut self, f: &mut dyn Function) {
        self.done = false;
        let lambda = GOLD;
        if !self.fa_set {
            match f.value(self.ax) {
                Some(v) => self.fax = v,
                None => return,
            }
        }
        if !self.fb_set {
            match f.value(self.bx) {
                Some(v) => self.fbx = v,
                None => return,
            }
        }
        if self.fbx > self.fax {
            std::mem::swap(&mut self.ax, &mut self.bx);
            std::mem::swap(&mut self.fbx, &mut self.fax);
        }

        // get next prob after (A, B)
        self.cx = self.bx + lambda * (self.bx - self.ax);
        if self.is_limited {
            if !limit_and_may_be_swap(
                f,
                self.left,
                self.right,
                self.ax,
                &mut self.bx,
                &mut self.fbx,
                &mut self.cx,
                &mut self.fcx,
            ) {
                return;
            }
        } else {
            match f.value(self.cx) {
                Some(v) => self.fcx = v,
                None => return,
            }
        }

        let mut steps = 0u32;
        while self.fbx > self.fcx {
            // Deviation from OCCT: its loop has no bound and spins forever when the
            // bracket stops moving (a function still falling at a limit, infinite
            // bounds). Each step grows the bracket geometrically, so a convergent case
            // never comes near the cap.
            steps += 1;
            if steps > MAX_STEPS {
                return;
            }
            let r = (self.bx - self.ax) * (self.fbx - self.fcx);
            let q = (self.bx - self.cx) * (self.fbx - self.fax);
            let qr = q - r;
            let m = if qr.abs() > TINY { qr.abs() } else { TINY };
            let sign = if qr > 0.0 { m.abs() } else { -m.abs() };
            let mut u = self.bx - ((self.bx - self.cx) * q - (self.bx - self.ax) * r) / (2.0 * sign);
            let mut ulim = self.bx + GLIMIT * (self.cx - self.bx);
            if self.is_limited {
                ulim = limited(self.left, self.right, ulim);
            }
            let mut fu = 0.0_f64;
            if (self.bx - u) * (u - self.cx) > 0.0 {
                // u is between B and C
                match f.value(u) {
                    Some(v) => fu = v,
                    None => return,
                }
                if fu < self.fcx {
                    // solution is found (B, u, c)
                    self.ax = self.bx;
                    self.bx = u;
                    self.fax = self.fbx;
                    self.fbx = fu;
                    self.done = true;
                    return;
                } else if fu > self.fbx {
                    // solution is found (A, B, u)
                    self.cx = u;
                    self.fcx = fu;
                    self.done = true;
                    return;
                }
                // get next prob after (B, C)
                u = self.cx + lambda * (self.cx - self.bx);
                if !self.next_probe(f, &mut u, &mut fu) {
                    return;
                }
            } else if (self.cx - u) * (u - ulim) > 0.0 {
                // u is beyond C but between C and limit
                match f.value(u) {
                    Some(v) => fu = v,
                    None => return,
                }
            } else if (u - ulim) * (ulim - self.cx) >= 0.0 {
                // u is beyond limit
                u = ulim;
                match f.value(u) {
                    Some(v) => fu = v,
                    None => return,
                }
            } else {
                // u tends to approach to the side of A, so reset it to the next prob after (B, C)
                u = self.cx + GOLD * (self.cx - self.bx);
                if !self.next_probe(f, &mut u, &mut fu) {
                    return;
                }
            }
            self.ax = self.bx;
            self.bx = self.cx;
            self.cx = u;
            self.fax = self.fbx;
            self.fbx = self.fcx;
            self.fcx = fu;
        }
        self.done = true;
    }

    /// The probe `u` after `(B, C)`: limited with `LimitAndMayBeSwap(F, Bx, Cx, FCx, u, fu)` or evaluated directly.
    fn next_probe(&mut self, f: &mut dyn Function, u: &mut f64, fu: &mut f64) -> bool {
        if self.is_limited {
            limit_and_may_be_swap(f, self.left, self.right, self.bx, &mut self.cx, &mut self.fcx, u, fu)
        } else {
            match f.value(*u) {
                Some(v) => {
                    *fu = v;
                    true
                }
                None => false,
            }
        }
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// `Values`: `(A, B, C)`. `Err(NotDone)` when not done (OCCT's own check is compiled out).
    pub fn values(&self) -> Result<(f64, f64, f64), MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        Ok((self.ax, self.bx, self.cx))
    }

    /// `FunctionValues`: `(FA, FB, FC)`.
    pub fn function_values(&self) -> Result<(f64, f64, f64), MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        Ok((self.fax, self.fbx, self.fcx))
    }

    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_BracketMinimum ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(" The bracketed triplet is: \n");
            s.push_str(&format!("{}, {}, {}\n", fmt_g(self.ax), fmt_g(self.bx), fmt_g(self.cx)));
            s.push_str(" The corresponding function values are: \n");
            s.push_str(&format!("{}, {}, {}\n", fmt_g(self.fax), fmt_g(self.fbx), fmt_g(self.fcx)));
        } else {
            s.push_str(" Status = not Done \n");
        }
        s
    }
}
