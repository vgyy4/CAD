//! `math_FunctionRoots`: all the real roots of `F - K` on an interval (the `MATH_FUNCTIONROOTS_NEWCODE` branch).
#![allow(clippy::float_cmp, clippy::manual_range_contains)]

use super::{BracketedRoot, Function, FunctionWithDerivative, fmt_g, seq_at, seq_get};
use crate::mathlin::MathError;

const ITMAX: i32 = 100;
const EPSEPS: f64 = 2e-14;

/// The derivative of a function with derivative, as a `math_Function` (`DerivFunction`).
struct DerivFunction<'a> {
    f: &'a mut dyn FunctionWithDerivative,
}

impl Function for DerivFunction<'_> {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.f.derivative(x)
    }
}

/// `AppendRoot`.
fn append_root(sol: &mut Vec<f64>, nb_state_sol: &mut Vec<i32>, x: f64, f: &mut dyn FunctionWithDerivative, d_x: f64) {
    let n = i32::try_from(sol.len()).unwrap_or(i32::MAX);
    if n == 0 {
        sol.push(x);
        let _ = f.value(x);
        nb_state_sol.push(f.state_number());
    } else {
        let mut i = 1;
        let mut pl = n + 1;
        while i <= n {
            let t = seq_at(sol, i);
            if t >= x {
                pl = i;
                i = n;
            }
            if (x - t).abs() <= d_x {
                pl = 0;
                i = n;
            }
            i += 1;
        }
        if pl > n {
            sol.push(x);
            let _ = f.value(x);
            nb_state_sol.push(f.state_number());
        } else if pl > 0 {
            let at = usize::try_from(pl - 1).unwrap_or(0);
            sol.insert(at, x);
            let _ = f.value(x);
            let state = f.state_number();
            nb_state_sol.insert(at, state);
        }
    }
}

/// `Solve`: Brent's method between `(x1, y1)` and `(x2, y2)`, finished by a Newton step.
#[allow(clippy::too_many_arguments)]
fn solve(
    f: &mut dyn FunctionWithDerivative,
    k: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    tol: f64,
    d_x: f64,
    sol: &mut Vec<f64>,
    nb_state_sol: &mut Vec<i32>,
) {
    let tols2 = 0.5 * tol;
    let (mut d, mut e) = (0.0_f64, 0.0_f64);
    let mut a = x1;
    let mut b = x2;
    let mut c = x2;
    let mut fa = y1;
    let mut fb = y2;
    let mut fc = y2;
    for _iter in 1..=ITMAX {
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = EPSEPS * b.abs() + tols2;
        let xm = 0.5 * (c - b);
        if xm.abs() < tol1 || fb == 0.0 {
            // one Newton iteration
            let mut xp = b;
            let mut yp = 0.0_f64;
            let mut dp = 0.0_f64;
            let mut itern = 5;
            loop {
                let mut ok = match f.values(xp) {
                    Some((y, dd)) => {
                        yp = y;
                        dp = dd;
                        true
                    }
                    None => false,
                };
                if ok {
                    ok = false;
                    if dp > 1e-10 || dp < -1e-10 {
                        xp -= (yp - k) / dp;
                    }
                    if xp <= x2 && xp >= x1 {
                        if let Some(v) = f.value(xp) {
                            yp = v;
                        }
                        yp -= k;
                        if yp.abs() < fb.abs() {
                            b = xp;
                            fb = yp;
                            ok = true;
                        }
                    }
                }
                if !ok {
                    break;
                }
                itern -= 1;
                if itern < 0 {
                    break;
                }
            }
            append_root(sol, nb_state_sol, b, f, d_x);
            return;
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (mut p, mut q);
            if a == c {
                p = xm * s;
                p += p;
                q = 1.0 - s;
            } else {
                q = fa / fc;
                let r = fb / fc;
                p = s * ((xm + xm) * q * (q - r) - (b - a) * (r - 1.0));
                q = (q - 1.0) * (r - 1.0) * (s - 1.0);
            }
            if p > 0.0 {
                q = -q;
            }
            p = p.abs();
            let min1 = 3.0 * xm * q - (tol1 * q).abs();
            let min2 = (e * q).abs();
            if (p + p) < (if min1 < min2 { min1 } else { min2 }) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        if d.abs() > tol1 {
            b += d;
        } else if xm >= 0.0 {
            b += tol1.abs();
        } else {
            b += -tol1.abs();
        }
        if let Some(v) = f.value(b) {
            fb = v;
        }
        fb -= k;
    }
}

/// `math_FunctionRoots`.
#[derive(Debug, Clone)]
pub struct FunctionRoots {
    done: bool,
    all_null: bool,
    sol: Vec<f64>,
    nb_state_sol: Vec<i32>,
}

impl FunctionRoots {
    /// The constructor (C++ defaults `eps_x = eps_f = eps_null = k = 0.0`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        f: &mut dyn FunctionWithDerivative,
        a: f64,
        b: f64,
        nb_sample: i32,
        eps_x_in: f64,
        eps_f: f64,
        eps_null: f64,
        k: f64,
    ) -> Self {
        let mut s = Self { done: true, all_null: false, sol: Vec::new(), nb_state_sol: Vec::new() };
        s.perform(f, a, b, nb_sample, eps_x_in, eps_f, eps_null, k);
        s
    }

    #[allow(clippy::too_many_arguments)]
    fn perform(
        &mut self,
        f: &mut dyn FunctionWithDerivative,
        a: f64,
        b: f64,
        nb_sample: i32,
        in_eps_x: f64,
        eps_f: f64,
        eps_null: f64,
        k: f64,
    ) {
        let mut x0 = a;
        let mut xn = b;
        let mut n = nb_sample;
        // low level checks
        if b < a {
            x0 = b;
            xn = a;
        }
        n = n.wrapping_mul(2);
        if n < 20 {
            n = 20;
        }
        // is EpsX too small (U + Nn * EpsX == U)?
        let mut eps_x = in_eps_x;
        let delta_u = x0.abs() + xn.abs();
        let n_eps_x = 0.0000000001 * delta_u;
        if eps_x < n_eps_x {
            eps_x = n_eps_x;
        }

        // look for an interval where F(xi) and F(xj) have different signs
        let mut x = x0;
        let dx = (xn - x0) / f64::from(n);
        let mut ptrval: Vec<f64> = Vec::new();
        let mut i = 0;
        while i <= n {
            if x > xn {
                x = xn;
            }
            if let Some(v) = f.value(x) {
                ptrval.push(v - k);
            }
            i += 1;
            x += dx;
        }
        // the function is not defined everywhere
        if i32::try_from(ptrval.len()).unwrap_or(i32::MAX) <= n {
            self.done = false;
            return;
        }
        let pv = |i: i32| -> f64 { seq_at(&ptrval, i + 1) };

        self.all_null = true;
        i = 0;
        while self.all_null && i <= n {
            if pv(i) > eps_null || pv(i) < -eps_null {
                self.all_null = false;
            }
            i += 1;
        }
        if self.all_null {
            // all the sample points are within the tolerance
            return;
        }

        // there are points out of the tolerance: detect the STRICT sign changes
        let tol = eps_x;
        i = 0;
        let mut ip1 = 1;
        x = x0;
        while i < n {
            let mut x2 = x + dx;
            if x2 > xn {
                x2 = xn;
            }
            if pv(i) < 0.0 {
                if pv(ip1) > 0.0 {
                    solve(f, k, x, pv(i), x2, pv(ip1), tol, n_eps_x, &mut self.sol, &mut self.nb_state_sol);
                }
            } else if pv(ip1) < 0.0 {
                solve(f, k, x, pv(i), x2, pv(ip1), tol, n_eps_x, &mut self.sol, &mut self.nb_state_sol);
            }
            i += 1;
            ip1 += 1;
            x += dx;
        }
        // the function vanishes on some Xi and is not null around Xi
        i = 0;
        while i <= n {
            if pv(i) == 0.0 {
                x = x0 + f64::from(i) * dx;
                if x > xn {
                    x = xn;
                }
                let mut u0 = dx * 0.5;
                let mut u1 = x + u0;
                u0 += x;
                if u0 < x0 {
                    u0 = x0;
                }
                if u0 > xn {
                    u0 = xn;
                }
                if u1 < x0 {
                    u1 = x0;
                }
                if u1 > xn {
                    u1 = xn;
                }

                let mut y0 = 0.0_f64;
                let mut y1 = 0.0_f64;
                if let Some(v) = f.value(u0) {
                    y0 = v;
                }
                y0 -= k;
                if let Some(v) = f.value(u1) {
                    y1 = v;
                }
                y1 -= k;
                if y0 * y1 < 0.0 {
                    solve(f, k, u0, y0, u1, y1, tol, n_eps_x, &mut self.sol, &mut self.nb_state_sol);
                } else if y0 != 0.0 || y1 != 0.0 {
                    append_root(&mut self.sol, &mut self.nb_state_sol, x, f, n_eps_x);
                }
            }
            i += 1;
        }
        // the end points
        if pv(0) <= eps_f && pv(0) >= -eps_f {
            append_root(&mut self.sol, &mut self.nb_state_sol, x0, f, n_eps_x);
        }
        if pv(n) <= eps_f && pv(n) >= -eps_f {
            append_root(&mut self.sol, &mut self.nb_state_sol, xn, f, n_eps_x);
        }

        // sample minima with f(x) > 0 and maxima with f(x) < 0: finer discretisation around them
        let mut ym = 0.0_f64;
        let mut dym = 0.0_f64;
        let majdx = 5.0 * dx;
        let mut im1 = 0;
        ip1 = 2;
        i = 1;
        let mut xm = x0 + dx;
        while i < n {
            let mut rediscr = false;
            if xm > xn {
                xm = xn;
            }
            if pv(i) > 0.0 {
                if pv(im1) > pv(i) && pv(ip1) > pv(i) {
                    // can the Ox axis be crossed? Estimate from Xim1
                    let mut xm1 = xm - dx;
                    if xm1 < x0 {
                        xm1 = x0;
                    }
                    if let Some((y, d)) = f.values(xm1) {
                        ym = y;
                        dym = d;
                    }
                    ym -= k;
                    if dym < -1e-10 || dym > 1e-10 {
                        let t = ym / dym;
                        if t < majdx && t > -majdx {
                            rediscr = true;
                        }
                    }
                    // estimate from Xip1
                    if !rediscr {
                        let mut xp1 = xm + dx;
                        if xp1 > xn {
                            xp1 = xn;
                        }
                        if let Some((y, d)) = f.values(xp1) {
                            ym = y;
                            dym = d;
                        }
                        ym -= k;
                        if dym < -1e-10 || dym > 1e-10 {
                            let t = ym / dym;
                            if t < majdx && t > -majdx {
                                rediscr = true;
                            }
                        }
                    }
                }
            } else if pv(i) < 0.0 && pv(im1) < pv(i) && pv(ip1) < pv(i) {
                // estimate from Xim1
                let mut xm1 = xm - dx;
                if xm1 < x0 {
                    xm1 = x0;
                }
                if let Some((y, d)) = f.values(xm1) {
                    ym = y;
                    dym = d;
                }
                ym -= k;
                if dym < -1e-10 || dym > 1e-10 {
                    let t = ym / dym;
                    if t < majdx && t > -majdx {
                        rediscr = true;
                    }
                }
                // (the C++ estimates from Xim1 a second time)
                if !rediscr {
                    xm1 = xm - dx;
                    if xm1 < x0 {
                        xm1 = x0;
                    }
                    if let Some((y, d)) = f.values(xm1) {
                        ym = y;
                        dym = d;
                    }
                    ym -= k;
                    if dym < -1e-10 || dym > 1e-10 {
                        let t = ym / dym;
                        if t < majdx && t > -majdx {
                            rediscr = true;
                        }
                    }
                }
            }
            if rediscr {
                self.refine_extremum(f, x0, xn, xm, dx, in_eps_x, eps_f, k, n_eps_x, tol, pv(im1), pv(ip1));
            }
            xm += dx;
            i += 1;
            im1 += 1;
            ip1 += 1;
        }
    }

    /// The block `if (Rediscr)`: search for the extremum between `xm - dx` and `xm + dx`.
    #[allow(clippy::too_many_arguments)]
    fn refine_extremum(
        &mut self,
        f: &mut dyn FunctionWithDerivative,
        x0_big: f64,
        xn: f64,
        xm: f64,
        dx: f64,
        in_eps_x: f64,
        eps_f: f64,
        k: f64,
        n_eps_x: f64,
        tol: f64,
        ptr_im1: f64,
        ptr_ip1: f64,
    ) {
        let mut x0 = xm - dx;
        let mut x3 = xm + dx;
        if x0 < x0_big {
            x0 = x0_big;
        }
        if x3 > xn {
            x3 = xn;
        }
        let (mut a_sol_x1, mut a_sol_x2) = (0.0_f64, 0.0_f64);
        let (mut a_val1, mut a_val2) = (0.0_f64, 0.0_f64);
        let (mut a_der1, mut a_der2) = (0.0_f64, 0.0_f64);
        let mut is_sol1 = false;
        let mut is_sol2 = false;
        // find the minimum of |F| between x0 and x3 by searching the zero of the derivative
        {
            let mut a_der_f = DerivFunction { f: &mut *f };
            let br = BracketedRoot::new(&mut a_der_f, x0, x3, in_eps_x, 100, 1.0e-12);
            if br.is_done() {
                a_sol_x1 = br.root().unwrap_or(0.0);
                if let Some(v) = f.value(a_sol_x1) {
                    a_val1 = v;
                }
                a_val1 = a_val1.abs();
                if a_val1 < eps_f {
                    is_sol1 = true;
                    a_der1 = br.value().unwrap_or(0.0);
                }
            }
        }

        // search for an extremum between x0 and x3 (x1 and x2 with x0 < x1 < x2 < x3)
        let r = 0.61803399;
        let c = 1.0 - r;
        let tol_cr = n_eps_x * 10.0;
        let mut f0 = ptr_im1;
        let mut f3 = ptr_ip1;
        let recherche_minimum = f0 > 0.0;

        let (mut x1, mut x2);
        if (x3 - xm).abs() > (x0 - xm).abs() {
            x1 = xm;
            x2 = xm + c * (x3 - xm);
        } else {
            x2 = xm;
            x1 = xm - c * (xm - x0);
        }
        let (mut f1, mut f2) = (0.0_f64, 0.0_f64);
        if let Some(v) = f.value(x1) {
            f1 = v;
        }
        f1 -= k;
        if let Some(v) = f.value(x2) {
            f2 = v;
        }
        f2 -= k;
        let tol_x = 0.001 * n_eps_x;
        while (x3 - x0).abs() > tol_cr * (x1.abs() + x2.abs()) && (x1 - x2).abs() > tol_x {
            if recherche_minimum {
                if f2 < f1 {
                    x0 = x1;
                    x1 = x2;
                    x2 = r * x1 + c * x3;
                    f0 = f1;
                    f1 = f2;
                    if let Some(v) = f.value(x2) {
                        f2 = v;
                    }
                    f2 -= k;
                } else {
                    x3 = x2;
                    x2 = x1;
                    x1 = r * x2 + c * x0;
                    f3 = f2;
                    f2 = f1;
                    if let Some(v) = f.value(x1) {
                        f1 = v;
                    }
                    f1 -= k;
                }
            } else if f2 > f1 {
                x0 = x1;
                x1 = x2;
                x2 = r * x1 + c * x3;
                f0 = f1;
                f1 = f2;
                if let Some(v) = f.value(x2) {
                    f2 = v;
                }
                f2 -= k;
            } else {
                x3 = x2;
                x2 = x1;
                x1 = r * x2 + c * x0;
                f3 = f2;
                f2 = f1;
                if let Some(v) = f.value(x1) {
                    f1 = v;
                }
                f1 -= k;
            }
            // not only extrema are searched: a root may be hit
            if f1 * f0 < 0.0 {
                solve(f, k, x0, f0, x1, f1, tol, n_eps_x, &mut self.sol, &mut self.nb_state_sol);
            }
            if f2 * f3 < 0.0 {
                solve(f, k, x2, f2, x3, f3, tol, n_eps_x, &mut self.sol, &mut self.nb_state_sol);
            }
        }
        if (recherche_minimum && f1 < f2) || (!recherche_minimum && f1 > f2) {
            // x1, f(x1) is the minimum
            if f1.abs() < eps_f {
                is_sol2 = true;
                a_sol_x2 = x1;
                a_val2 = f1.abs();
            }
        } else if f2.abs() < eps_f {
            // x2, f(x2) is the minimum
            is_sol2 = true;
            a_sol_x2 = x2;
            a_val2 = f2.abs();
        }
        // choose the best solution between aSolX1 and aSolX2
        if is_sol1 && is_sol2 {
            if a_val2 - a_val1 > eps_f {
                append_root(&mut self.sol, &mut self.nb_state_sol, a_sol_x1, f, n_eps_x);
            } else if a_val1 - a_val2 > eps_f {
                append_root(&mut self.sol, &mut self.nb_state_sol, a_sol_x2, f, n_eps_x);
            } else {
                a_der1 = a_der1.abs();
                if let Some(v) = f.derivative(a_sol_x2) {
                    a_der2 = v;
                }
                a_der2 = a_der2.abs();
                if a_der1 < a_der2 {
                    append_root(&mut self.sol, &mut self.nb_state_sol, a_sol_x1, f, n_eps_x);
                } else {
                    append_root(&mut self.sol, &mut self.nb_state_sol, a_sol_x2, f, n_eps_x);
                }
            }
        } else if is_sol1 {
            append_root(&mut self.sol, &mut self.nb_state_sol, a_sol_x1, f, n_eps_x);
        } else if is_sol2 {
            append_root(&mut self.sol, &mut self.nb_state_sol, a_sol_x2, f, n_eps_x);
        }
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    /// `IsAllNull`.
    pub fn is_all_null(&self) -> Result<bool, MathError> {
        self.check()?;
        Ok(self.all_null)
    }
    /// `NbSolutions`.
    pub fn nb_solutions(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(i32::try_from(self.sol.len()).unwrap_or(i32::MAX))
    }
    /// `Value`.
    pub fn value(&self, n: i32) -> Result<f64, MathError> {
        self.check()?;
        seq_get(&self.sol, n)
    }
    /// `StateNumber`.
    pub fn state_number(&self, n: i32) -> Result<i32, MathError> {
        self.check()?;
        seq_get(&self.nb_state_sol, n)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_FunctionRoots ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Number of solutions = {}\n", self.sol.len()));
            for (i, v) in self.sol.iter().enumerate() {
                s.push_str(&format!(" Solution Number {}= {}\n", i + 1, fmt_g(*v)));
            }
        } else {
            s.push_str(" Status = not Done \n");
        }
        s
    }
}
