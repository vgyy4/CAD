//! `math_TrigonometricFunctionRoots` and `math_TrigonometricEquationFunction`.
#![allow(clippy::float_cmp)]

use super::{Function, FunctionWithDerivative, NewtonFunctionRoot, P_CONFUSION, REAL_FIRST, REAL_LAST, epsilon, fmt_g};
use crate::mathlin::{DirectPolynomialRoots, MathError};
use std::f64::consts::PI as M_PI;

/// `math_TrigonometricEquationFunction`: `A cos^2 x + 2 B cos x sin x + C cos x + D sin x + E`.
#[derive(Clone, Debug)]
pub struct TrigonometricEquationFunction {
    aa: f64,
    bb: f64,
    cc: f64,
    dd: f64,
    ee: f64,
}

impl TrigonometricEquationFunction {
    /// The constructor.
    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64) -> Self {
        Self { aa: a, bb: b, cc: c, dd: d, ee: e }
    }
}

impl Function for TrigonometricEquationFunction {
    fn value(&mut self, x: f64) -> Option<f64> {
        let (cn, sn) = (libm::cos(x), libm::sin(x));
        Some(cn * (self.aa * cn + (self.bb + self.bb) * sn + self.cc) + self.dd * sn + self.ee)
    }
}

impl FunctionWithDerivative for TrigonometricEquationFunction {
    fn derivative(&mut self, x: f64) -> Option<f64> {
        let (cn, sn) = (libm::cos(x), libm::sin(x));
        let mut d = -self.aa * cn * sn + self.bb * (cn * cn - sn * sn);
        d += d;
        d += -self.cc * sn + self.dd * cn;
        Some(d)
    }

    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        let (cn, sn) = (libm::cos(x), libm::sin(x));
        let aacn = self.aa * cn;
        let bbsn = self.bb * sn;

        let f = aacn * cn + bbsn * (cn + cn) + self.cc * cn + self.dd * sn + self.ee;
        let mut d = -aacn * sn + self.bb * (cn * cn - sn * sn);
        d += d;
        d += -self.cc * sn + self.dd * cn;
        Some((f, d))
    }
}

/// `math_TrigonometricFunctionRoots`: the roots of the equation in `[inf_bound, sup_bound]`.
#[derive(Debug, Clone)]
pub struct TrigonometricFunctionRoots {
    nb_sol: i32,
    sol: [f64; 4],
    infinite_status: bool,
    done: bool,
}

/// A 1-based array of 4 doubles (`NCollection_Array1<double>(1, 4)`) accessed without panics.
fn set4(a: &mut [f64; 4], i: i32, v: f64) {
    if let Some(slot) = usize::try_from(i - 1).ok().and_then(|k| a.get_mut(k)) {
        *slot = v;
    }
}

fn get4(a: &[f64; 4], i: i32) -> f64 {
    usize::try_from(i - 1).ok().and_then(|k| a.get(k)).copied().unwrap_or(0.0)
}

impl TrigonometricFunctionRoots {
    fn blank() -> Self {
        Self { nb_sol: -1, sol: [0.0; 4], infinite_status: false, done: false }
    }

    /// The 7-argument constructor.
    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, inf_bound: f64, sup_bound: f64) -> Self {
        let mut s = Self::blank();
        s.perform(a, b, c, d, e, inf_bound, sup_bound);
        s
    }

    /// The 5-argument constructor `(C, D, E, InfBound, SupBound)` (`A = B = 0`).
    pub fn new_cde(c: f64, d: f64, e: f64, inf_bound: f64, sup_bound: f64) -> Self {
        let mut s = Self::blank();
        s.perform(0.0, 0.0, c, d, e, inf_bound, sup_bound);
        s
    }

    /// The 4-argument constructor `(D, E, InfBound, SupBound)` (`A = B = C = 0`).
    pub fn new_de(d: f64, e: f64, inf_bound: f64, sup_bound: f64) -> Self {
        let mut s = Self::blank();
        s.perform(0.0, 0.0, 0.0, d, e, inf_bound, sup_bound);
        s
    }

    #[allow(clippy::too_many_arguments, clippy::cognitive_complexity, unused_assignments)]
    fn perform(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, inf_bound: f64, sup_bound: f64) {
        let mut nzer: i32 = 0;
        let nit = 10;
        let tol1 = 1.0e-15;
        let mut ko = [0.0_f64; 5];
        let mut zer = [0.0_f64; 4];
        self.infinite_status = false;
        self.done = true;

        let eps = 1.5e-12;

        let depi = M_PI + M_PI;
        let (my_borne_inf, mut delta, modv);
        if inf_bound <= REAL_FIRST && sup_bound >= REAL_LAST {
            my_borne_inf = 0.0;
            delta = depi;
            modv = 0.0;
        } else if sup_bound >= REAL_LAST {
            my_borne_inf = inf_bound;
            delta = depi;
            modv = my_borne_inf / depi;
        } else if inf_bound <= REAL_FIRST {
            my_borne_inf = sup_bound - depi;
            delta = depi;
            modv = my_borne_inf / depi;
        } else {
            my_borne_inf = inf_bound;
            delta = sup_bound - inf_bound;
            modv = inf_bound / depi;
            if (sup_bound - inf_bound) > depi {
                delta = depi;
            }
        }

        if a.abs() <= eps && b.abs() <= eps {
            if c.abs() <= eps {
                if d.abs() <= eps {
                    if e.abs() <= eps {
                        self.infinite_status = true; // infinitely many solutions
                        return;
                    } else {
                        self.nb_sol = 0;
                        return;
                    }
                } else {
                    // equation d*sin(x) + e = 0
                    self.nb_sol = 0;
                    let aa = -e / d;
                    if aa.abs() > 1.0 {
                        return;
                    }

                    set4(&mut zer, 1, libm::asin(aa));
                    let z1 = get4(&zer, 1);
                    set4(&mut zer, 2, M_PI - z1);
                    nzer = 2;
                    for i in 1..=nzer {
                        if get4(&zer, i) <= -eps {
                            let zi = get4(&zer, i);
                            set4(&mut zer, i, depi - zi.abs());
                        }
                        // the solutions are put between InfBound and SupBound
                        let zi = get4(&zer, i);
                        set4(&mut zer, i, zi + modv.trunc() * depi);
                        let x = get4(&zer, i) - my_borne_inf;
                        if (x > (-epsilon(delta))) && (x < delta + epsilon(delta)) {
                            self.nb_sol += 1;
                            let zi = get4(&zer, i);
                            set4(&mut self.sol, self.nb_sol, zi);
                        }
                    }
                }
                return;
            } else if d.abs() <= eps {
                // first degree equation c*cos(x) + e = 0
                self.nb_sol = 0;
                let aa = -e / c;
                if aa.abs() > 1.0 {
                    return;
                }
                set4(&mut zer, 1, libm::acos(aa));
                let z1 = get4(&zer, 1);
                set4(&mut zer, 2, -z1);
                nzer = 2;

                for i in 1..=nzer {
                    if get4(&zer, i) <= -eps {
                        let zi = get4(&zer, i);
                        set4(&mut zer, i, depi - zi.abs());
                    }
                    // the solutions are put between InfBound and SupBound
                    let zi = get4(&zer, i);
                    set4(&mut zer, i, zi + modv.trunc() * 2.0 * M_PI);
                    let x = get4(&zer, i) - my_borne_inf;
                    if (x >= (-epsilon(delta))) && (x <= delta + epsilon(delta)) {
                        self.nb_sol += 1;
                        let zi = get4(&zer, i);
                        set4(&mut self.sol, self.nb_sol, zi);
                    }
                }
                return;
            } else {
                // second degree equation
                let aa = e - c;
                let bb = 2.0 * d;
                let cc = e + c;

                let resol = DirectPolynomialRoots::new_quadratic(aa, bb, cc);
                if !resol.is_done() {
                    self.done = false;
                    return;
                } else if !resol.infinite_roots() {
                    nzer = resol.number_of_solutions().unwrap_or(0);
                    for i in 1..=nzer {
                        set4(&mut zer, i, resol.value(i).unwrap_or(0.0));
                    }
                } else {
                    self.infinite_status = true;
                    return;
                }
            }
        } else {
            // two additional analytical cases
            if a.abs() <= eps && e.abs() <= eps {
                if c.abs() <= eps {
                    // 2 * B * sin * cos + D * sin = 0
                    nzer = 2;
                    zer[0] = 0.0;
                    zer[1] = M_PI;

                    let aa = -d / (b * 2.0);
                    if aa.abs() <= 1.0 + P_CONFUSION {
                        nzer = 4;
                        if aa >= 1.0 {
                            zer[2] = 0.0;
                            zer[3] = 0.0;
                        } else if aa <= -1.0 {
                            zer[2] = M_PI;
                            zer[3] = M_PI;
                        } else {
                            zer[2] = libm::acos(aa);
                            zer[3] = depi - zer[2];
                        }
                    }
                    self.finish_analytic(&mut zer, nzer, my_borne_inf, modv, delta, eps, inf_bound, sup_bound);
                    return;
                }
                if d.abs() <= eps {
                    // 2 * B * sin * cos + C * cos = 0
                    nzer = 2;
                    zer[0] = M_PI / 2.0;
                    zer[1] = M_PI * 3.0 / 2.0;

                    let aa = -c / (b * 2.0);
                    if aa.abs() <= 1.0 + P_CONFUSION {
                        nzer = 4;
                        if aa >= 1.0 {
                            zer[2] = M_PI / 2.0;
                            zer[3] = M_PI / 2.0;
                        } else if aa <= -1.0 {
                            zer[2] = M_PI * 3.0 / 2.0;
                            zer[3] = M_PI * 3.0 / 2.0;
                        } else {
                            zer[2] = libm::asin(aa);
                            zer[3] = M_PI - zer[2];
                        }
                    }
                    self.finish_analytic(&mut zer, nzer, my_borne_inf, modv, delta, eps, inf_bound, sup_bound);
                    return;
                }
            }

            // fourth degree equation
            ko[0] = a - c + e;
            ko[1] = 2.0 * d - 4.0 * b;
            ko[2] = 2.0 * e - 2.0 * a;
            ko[3] = 4.0 * b + 2.0 * d;
            ko[4] = a + c + e;
            loop {
                let mut bko = false;
                let resol4 = DirectPolynomialRoots::new_quartic(ko[0], ko[1], ko[2], ko[3], ko[4]);
                if !resol4.is_done() {
                    self.done = false;
                    return;
                } else if !resol4.infinite_roots() {
                    nzer = resol4.number_of_solutions().unwrap_or(0);
                    for i in 1..=nzer {
                        set4(&mut zer, i, resol4.value(i).unwrap_or(0.0));
                    }
                } else {
                    self.infinite_status = true;
                    return;
                }
                loop {
                    let mut triok = true;
                    for i in 1..nzer {
                        if get4(&zer, i) > get4(&zer, i + 1) {
                            let t = get4(&zer, i);
                            let zn = get4(&zer, i + 1);
                            set4(&mut zer, i, zn);
                            set4(&mut zer, i + 1, t);
                            triok = false;
                        }
                    }
                    if triok {
                        break;
                    }
                }

                for i in 1..nzer {
                    if (get4(&zer, i + 1) - get4(&zer, i)).abs() < eps {
                        // a double root or a numerical error?
                        let qw = get4(&zer, i + 1);
                        let va = ko[3] + qw * (2.0 * ko[2] + qw * (3.0 * ko[1] + qw * (4.0 * ko[0])));
                        if va.abs() > eps {
                            bko = true;
                            break;
                        }
                    }
                }
                if bko {
                    // a small coefficient: divide
                    ko[0] *= 0.0001;
                    ko[1] *= 0.0001;
                    ko[2] *= 0.0001;
                    ko[3] *= 0.0001;
                    ko[4] *= 0.0001;
                }
                if !bko {
                    break;
                }
            }
        }

        // check the solutions against the bounds
        let supm_infs100 = (sup_bound - inf_bound) * 0.01;
        self.nb_sol = 0;
        let mut j = 0;
        for i in 1..=nzer {
            let mut teta = libm::atan(get4(&zer, i));
            teta += teta;
            if get4(&zer, i) <= -eps {
                teta = depi - teta.abs();
            }
            teta += modv.trunc() * depi;
            if teta - my_borne_inf < 0.0 {
                teta += depi;
            }

            let mut x = teta - my_borne_inf;
            if (x >= (-epsilon(delta))) && (x <= delta + epsilon(delta)) {
                x = teta;

                // Newton call
                let mut teta_newton = teta;
                let mut my_f = TrigonometricEquationFunction::new(a, b, c, d, e);
                let resol = NewtonFunctionRoot::new(&mut my_f, x, tol1, eps, nit);
                if resol.is_done() {
                    teta_newton = resol.root().unwrap_or(teta);
                }
                // Newton may converge very far from the initial solution
                let delta_newton = teta_newton - teta;
                if delta_newton > supm_infs100 || delta_newton < -supm_infs100 {
                    // ignored
                } else {
                    teta = teta_newton;
                }

                let mut flag4 = false;

                for k in 1..=self.nb_sol {
                    // the values are put in increasing order
                    if teta < get4(&self.sol, k) {
                        for l in k..=self.nb_sol {
                            j = self.nb_sol - l + k;
                            let sj = get4(&self.sol, j);
                            set4(&mut self.sol, j + 1, sj);
                        }
                        set4(&mut self.sol, k, teta);
                        self.nb_sol += 1;
                        flag4 = true;
                        break;
                    }
                }
                if !flag4 {
                    self.nb_sol += 1;
                    set4(&mut self.sol, self.nb_sol, teta);
                }
            }
        }
        // the particular case of PI
        if self.nb_sol < 4 {
            let start_index = self.nb_sol + 1;
            for sol_it in start_index..=4 {
                let teta = M_PI + modv.trunc() * 2.0 * M_PI;
                let x = teta - my_borne_inf;
                if (x >= (-epsilon(delta))) && (x <= delta + epsilon(delta)) && (a - c + e).abs() <= eps {
                    let mut flag4 = false;
                    for k in 1..=self.nb_sol {
                        j = k;
                        if teta < get4(&self.sol, k) {
                            flag4 = true;
                            break;
                        }
                        if sol_it == start_index && (teta - get4(&self.sol, k)).abs() <= eps {
                            return;
                        }
                    }

                    if !flag4 {
                        self.nb_sol += 1;
                        set4(&mut self.sol, self.nb_sol, teta);
                    } else {
                        for k in j..=self.nb_sol {
                            let i = self.nb_sol - k + j;
                            let si = get4(&self.sol, i);
                            set4(&mut self.sol, i + 1, si);
                        }
                        set4(&mut self.sol, j, teta);
                        self.nb_sol += 1;
                    }
                }
            }
        }
    }

    /// The common tail of the two analytic cases `2 B sin cos + D sin = 0` and `2 B sin cos + C cos = 0`.
    #[allow(clippy::too_many_arguments)]
    fn finish_analytic(
        &mut self,
        zer: &mut [f64; 4],
        nzer: i32,
        my_borne_inf: f64,
        modv: f64,
        delta: f64,
        eps: f64,
        inf_bound: f64,
        sup_bound: f64,
    ) {
        self.nb_sol = 0;
        let depi = M_PI + M_PI;
        for i in 1..=nzer {
            if get4(zer, i) <= my_borne_inf - eps {
                let zi = get4(zer, i);
                set4(zer, i, zi + depi);
            }
            // the solutions are put between InfBound and SupBound
            let zi = get4(zer, i);
            set4(zer, i, zi + modv.trunc() * 2.0 * M_PI);
            let x = get4(zer, i) - my_borne_inf;
            if (x >= (-P_CONFUSION)) && (x <= delta + P_CONFUSION) {
                if get4(zer, i) < inf_bound {
                    set4(zer, i, inf_bound);
                }
                if get4(zer, i) > sup_bound {
                    set4(zer, i, sup_bound);
                }
                self.nb_sol += 1;
                let zi = get4(zer, i);
                set4(&mut self.sol, self.nb_sol, zi);
            }
        }
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// `InfiniteRoots`.
    pub fn infinite_roots(&self) -> bool {
        self.infinite_status
    }

    /// `NbSolutions`.
    pub fn nb_solutions(&self) -> Result<i32, MathError> {
        if self.infinite_status {
            return Err(MathError::InfiniteSolutions);
        }
        if !self.done {
            return Err(MathError::NotDone);
        }
        Ok(self.nb_sol)
    }

    /// `Value`.
    pub fn value(&self, index: i32) -> Result<f64, MathError> {
        if self.infinite_status {
            return Err(MathError::InfiniteSolutions);
        }
        if !self.done {
            return Err(MathError::NotDone);
        }
        if index > self.nb_sol || index < 1 {
            return Err(MathError::Range);
        }
        usize::try_from(index - 1).ok().and_then(|k| self.sol.get(k)).copied().ok_or(MathError::Range)
    }

    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from(" math_TrigonometricFunctionRoots: \n");
        if !self.done {
            s.push_str("Not Done \n");
        } else if self.infinite_status {
            s.push_str(" There is an infinity of roots\n");
        } else {
            s.push_str(&format!(" Number of solutions = {}\n", self.nb_sol));
            for i in 1..=self.nb_sol {
                s.push_str(&format!(" Value number {}= {}\n", i, fmt_g(get4(&self.sol, i))));
            }
        }
        s
    }
}
