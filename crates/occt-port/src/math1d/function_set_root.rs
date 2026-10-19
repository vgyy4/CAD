//! The one-variable path of `math_FunctionSetRoot` (private): what `math_FunctionRoot` needs of it.
//!
//! `math_FunctionRoot` runs `math_FunctionSetRoot` on its class `math_MyFunctionSetWithDerivatives`, which has one
//! variable and one equation. Therefore:
//! * the branches of `SearchDirection` for more unknowns than equations (SVD) or fewer (`math_GaussLeastSquare`),
//!   and the sub-problem built for a partly constrained system, are unreachable and not translated;
//! * `math_FunctionSetRoot::Perform` is always called with `theStopOnDivergent = false`, so `myIsDivergent` has no
//!   effect and is not tracked;
//! * `math_MyFunctionSetWithDerivatives` does not override `GetStateNumber` (it returns 0 without calling the
//!   user's function), so the `State` of `math_FunctionSetRoot` is not kept.
#![allow(clippy::float_cmp)]

use super::{
    BrentMinimum, Function, FunctionWithDerivative, PRECISION_INFINITE, SQUARE_CONFUSION, cmax, cmin, epsilon,
    is_infinite,
};
use crate::mathlin::{Gauss, IntegerVector, MathError, Matrix, Svd, Vector};

/// `math_MyFunctionSetWithDerivatives`.
pub(super) struct OneVarSet<'a> {
    pub(super) ff: &'a mut dyn FunctionWithDerivative,
}

impl OneVarSet<'_> {
    /// `Value(X, F)`: `Ff->Value(X(1), F(1))`.
    fn value(&mut self, x: &Vector, fs: &mut Vector) -> Result<bool, MathError> {
        match self.ff.value(x.value(1)?) {
            Some(v) => {
                fs.set_value(1, v)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// `Values(X, F, D)`: `Ff->Values(X(1), F(1), D(1, 1))`.
    fn values(&mut self, x: &Vector, fs: &mut Vector, d: &mut Matrix) -> Result<bool, MathError> {
        match self.ff.values(x.value(1)?) {
            Some((v, dv)) => {
                fs.set_value(1, v)?;
                d.set_value(1, 1, dv)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

/// `MyDirFunction`: the function set along a direction (`Temp1..Temp4` are its own vectors).
pub(super) struct DirFunction<'a> {
    set: OneVarSet<'a>,
    p0: Vector,
    dir: Vector,
    p: Vector,
    fv: Vector,
}

impl<'a> DirFunction<'a> {
    pub(super) fn new(set: OneVarSet<'a>) -> Result<Self, MathError> {
        Ok(Self { set, p0: Vector::new(1, 1)?, dir: Vector::new(1, 1)?, p: Vector::new(1, 1)?, fv: Vector::new(1, 1)? })
    }

    /// `Initialize`.
    fn initialize(&mut self, p0: &Vector, dir: &Vector) {
        self.p0 = p0.clone();
        self.dir = dir.clone();
    }

    fn eval(&mut self, x: f64) -> Result<Option<f64>, MathError> {
        for i in self.p.lower()..=self.p.upper() {
            let p = self.dir.value(i)?;
            self.p.set_value(i, p * x + self.p0.value(i)?)?;
        }
        if self.set.value(&self.p, &mut self.fv)? {
            for i in self.fv.lower()..=self.fv.upper() {
                let a_val = self.fv.value(i)?;
                if a_val <= -1.0e+100 || a_val >= 1.0e+100 {
                    return Ok(None);
                }
            }
            return Ok(Some(0.5 * self.fv.norm2()));
        }
        Ok(None)
    }

    /// `Value(Sol, FF, DF, GH, F2, Gnr1)`.
    fn full_value(
        &mut self,
        sol: &Vector,
        ff: &mut Vector,
        df: &mut Matrix,
        gh: &mut Vector,
        f2: &mut f64,
        gnr1: &mut f64,
    ) -> Result<bool, MathError> {
        if self.set.values(sol, ff, df)? {
            for i in ff.lower()..=ff.upper() {
                let a_val = ff.value(i)?;
                if a_val < 0.0 {
                    if a_val <= -1.0e+100 {
                        return Ok(false);
                    }
                } else if a_val >= 1.0e+100 {
                    return Ok(false);
                }
            }
            *f2 = 0.5 * ff.norm2();
            gh.t_multiply_matrix_from(df, ff)?;
            for i in gh.lower()..=gh.upper() {
                if is_infinite(gh.value(i)?) {
                    return Ok(false);
                }
            }
            *gnr1 = gh.norm2();
            return Ok(true);
        }
        Ok(false)
    }
}

impl Function for DirFunction<'_> {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.eval(x).ok().flatten()
    }
}

/// `MinimizeDirection` from three points.
fn minimize_direction_3(
    p0: &Vector,
    p1: &Vector,
    p2: &Vector,
    f1: f64,
    delta: &mut Vector,
    tol: &Vector,
    f: &mut DirFunction,
) -> Result<bool, MathError> {
    // (1) one-dimensional parametric tolerance
    let mut tol1d = 2.1_f64;
    let eps = 1.0e-16;
    for ii in 1..=tol.length() {
        let invnorme = delta.value(ii)?.abs();
        if invnorme > eps {
            tol1d = cmin(tol1d, tol.value(ii)? / invnorme);
        }
    }
    if tol1d > 1.9 {
        return Ok(false); // not worth the effort
    }
    tol1d /= 3.0;

    *delta = p1.subtracted(p0)?;
    let mut invnorme = delta.norm();
    if invnorme <= eps {
        return Ok(false);
    }
    invnorme = 1.0 / invnorme;

    f.initialize(p1, delta);

    // (2) minimise
    let ax = -1.0;
    let bx = 0.0;
    let cx = p2.subtracted(p1)?.norm() * invnorme;
    if cx < 1.0e-2 {
        return Ok(false);
    }

    let mut sol = BrentMinimum::new(tol1d, 100, tol1d);
    sol.perform(f, ax, bx, cx);

    if sol.is_done() {
        let tsol = sol.location()?;
        if sol.minimum()? < f1 {
            delta.multiply(tsol);
            return Ok(true);
        }
    }
    Ok(false)
}

/// `MinimizeDirection` from two points and a derivative.
#[allow(clippy::too_many_arguments)]
fn minimize_direction_2(
    p: &Vector,
    dir: &mut Vector,
    p_value: f64,
    p_dir_value: f64,
    gradient: &Vector,
    d_gradient: &Vector,
    tol: &Vector,
    f: &mut DirFunction,
) -> Result<bool, MathError> {
    if is_infinite(p_value) || is_infinite(p_dir_value) {
        return Ok(false);
    }
    // (0) one-dimensional parametric tolerance
    let mut good = false;
    let eps = 1.0e-20;
    let mut tol1d = 1.1_f64;
    let mut result = p_value;
    for ii in 1..=tol.length() {
        let absdir = dir.value(ii)?.abs();
        if absdir > eps {
            tol1d = cmin(tol1d, tol.value(ii)? / absdir);
        }
    }
    if tol1d > 0.9 {
        return Ok(false);
    }

    // (1) a first quadratic interpolation
    let (mut ax, mut bx, mut cx);
    let mut tsol;

    let df1 = gradient.dot(dir)?;
    let df2 = d_gradient.dot(dir)?;

    if df1 < -eps && df2 > eps {
        // trough
        tsol = -df1 / (df2 - df1);
    } else {
        cx = p_value;
        bx = df1;
        ax = p_dir_value - (bx + cx);

        if ax.abs() <= eps {
            // linear case
            if bx.abs() >= eps {
                tsol = -cx / bx;
            } else {
                tsol = 0.0;
            }
        } else {
            // quadratic case
            let mut delta = bx * bx - 4.0 * ax * cx;
            if delta > 1.0e-9 {
                // there are roots: take the nearest to 0
                delta = delta.sqrt();
                tsol = -(bx + delta);
                let tsolbis = delta - bx;
                if tsolbis.abs() < tsol.abs() {
                    tsol = tsolbis;
                }
                tsol /= 2.0 * ax;
            } else {
                // no or hardly any root: take the extremum
                tsol = -(0.5 * bx) / ax;
            }
        }
    }

    if tsol.abs() >= 1.0 {
        return Ok(false); // useless result
    }

    f.initialize(p, dir);
    let mut fsol = 0.0_f64;
    if let Some(v) = f.value(tsol) {
        fsol = v;
    }

    if fsol < p_value {
        good = true;
        result = fsol;
    }

    // (2) if the progress is not enough, a proper search from the previous initial values
    if (fsol > 0.2 * p_value) && (tol1d < 0.5) {
        if tsol < 0.0 {
            ax = tsol;
            bx = 0.0;
            cx = 1.0;
        } else {
            ax = 0.0;
            bx = tsol;
            cx = 1.0;
        }

        let mut sol = BrentMinimum::new(tol1d, 100, tol1d);

        // base invocation
        sol.perform(f, ax, bx, cx);
        if sol.is_done() && sol.minimum()? <= result {
            tsol = sol.location()?;
            good = true;
            result = sol.minimum()?;

            // the objective function changes too fast: additional computations
            if gradient.norm2() > 1.0 / SQUARE_CONFUSION && tsol > ax && tsol < cx {
                // first and second part invocation
                sol.perform(f, ax, (ax + tsol) / 2.0, tsol);
                if sol.is_done() && sol.minimum()? <= result {
                    tsol = sol.location()?;
                    good = true;
                    result = sol.minimum()?;
                }

                sol.perform(f, tsol, (cx + tsol) / 2.0, cx);
                if sol.is_done() && sol.minimum()? <= result {
                    tsol = sol.location()?;
                    good = true;
                    result = sol.minimum()?;
                }
            }
        }
    }

    if good {
        // update Delta
        dir.multiply(tsol);
    }
    let _ = result;
    Ok(good)
}

/// `SearchDirection` without constraints (one unknown and one equation: the square case).
fn search_direction(
    df: &Matrix,
    gh: &Vector,
    ff: &Vector,
    change_direction: bool,
    inv_length_max: &Vector,
    direction: &mut Vector,
    dy: &mut f64,
) -> Result<(), MathError> {
    let ninc = df.col_number();
    let neq = df.row_number();
    let eps = 1.0e-32;
    let mut change_direction = change_direction;
    if !change_direction {
        if ninc == neq {
            for i in ff.lower()..=ff.upper() {
                direction.set_value(i, -ff.value(i)?)?;
            }
            let solut = Gauss::new(df, 1.0e-9)?;
            if solut.is_done() {
                *direction = solut.solve_in_place(direction.clone())?;
            } else {
                // singular directions have to be forgotten
                let solve_by_svd = Svd::new(df)?;
                if solve_by_svd.is_done() {
                    *direction = solve_by_svd.solve(&ff.t_multiplied(-1.0), 1.0e-6)?;
                } else {
                    change_direction = true;
                }
            }
        } else {
            // more unknowns than equations (SVD) or fewer (least squares): not reachable for one unknown
            return Err(MathError::Dimension);
        }
    }
    // too long directions are forbidden, to armour ill-conditioned cases
    let lower = direction.lower();
    let mut ratio = (direction.value(lower)? * inv_length_max.value(lower)?).abs();
    for i in (lower + 1)..=direction.upper() {
        ratio = cmax(ratio, (direction.value(i)? * inv_length_max.value(i)?).abs());
    }
    if ratio > 1.0 {
        direction.divide(ratio)?;
    }

    *dy = direction.dot(gh)?;
    if *dy >= -eps {
        // Newton does not descend: take the gradient
        change_direction = true;
    }
    if change_direction {
        for i in direction.lower()..=direction.upper() {
            direction.set_value(i, -gh.value(i)?)?;
        }
        *dy = -gh.norm2();
    }
    Ok(())
}

/// `SearchDirection` along a boundary, with the constraints (one unknown: either free or fixed).
#[allow(clippy::too_many_arguments)]
fn search_direction_constrained(
    df: &Matrix,
    gh: &Vector,
    ff: &Vector,
    constraints: &IntegerVector,
    change_direction: bool,
    inv_length_max: &Vector,
    direction: &mut Vector,
    dy: &mut f64,
) -> Result<(), MathError> {
    let ninc = df.col_number();
    let mut cons = 0;
    for i in 1..=ninc {
        if constraints.value(i)? != 0 {
            cons += 1;
        }
    }
    if cons == 0 {
        search_direction(df, gh, ff, change_direction, inv_length_max, direction, dy)
    } else if cons == ninc {
        // nothing more to do
        for i in direction.lower()..=direction.upper() {
            direction.set_value(i, 0.0)?;
        }
        *dy = 0.0;
        Ok(())
    } else {
        // a sub-problem with some of the unknowns: not reachable for one unknown
        Err(MathError::Dimension)
    }
}

/// `Bounds`: trims `sol` to the domain; returns `(Out, IsNewSol)`.
fn bounds(
    inf_bound: &Vector,
    sup_bound: &Vector,
    tol: &Vector,
    sol: &mut Vector,
    sol_save: &Vector,
    constraints: &mut IntegerVector,
    delta: &mut Vector,
) -> Result<(bool, bool), MathError> {
    let mut out = false;
    let ninc = sol.length();
    let mut monratio = 1.0_f64;

    let mut is_new_sol = true;

    // ratio of the trimming
    for i in 1..=ninc {
        constraints.set_value(i, 0)?;
        delta.set_value(i, sol.value(i)? - sol_save.value(i)?)?;
        if inf_bound.value(i)? == sup_bound.value(i)? {
            constraints.set_value(i, 1)?;
            out = true;
        } else if sol.value(i)? < inf_bound.value(i)? {
            constraints.set_value(i, 1)?;
            out = true;
            // Delta(i) is negative
            if -delta.value(i)? > tol.value(i)? {
                monratio = cmin(monratio, (inf_bound.value(i)? - sol_save.value(i)?) / delta.value(i)?);
            }
        } else if sol.value(i)? > sup_bound.value(i)? {
            constraints.set_value(i, 1)?;
            out = true;
            // Delta(i) is positive
            if delta.value(i)? > tol.value(i)? {
                monratio = cmin(monratio, (sup_bound.value(i)? - sol_save.value(i)?) / delta.value(i)?);
            }
        }
    }

    if out {
        // truncation and last trimming (numerical problems)
        if monratio == 0.0 {
            is_new_sol = false;
            *sol = sol_save.clone();
            delta.init(0.0);
        } else {
            delta.multiply(monratio);
            *sol = sol_save.added(delta)?;
            for i in 1..=ninc {
                if sol.value(i)? < inf_bound.value(i)? {
                    sol.set_value(i, inf_bound.value(i)?)?;
                    delta.set_value(i, sol.value(i)? - sol_save.value(i)?)?;
                } else if sol.value(i)? > sup_bound.value(i)? {
                    sol.set_value(i, sup_bound.value(i)?)?;
                    delta.set_value(i, sol.value(i)? - sol_save.value(i)?)?;
                }
            }
        }
    }
    Ok((out, is_new_sol))
}

/// `math_FunctionSetRoot` for one variable and one equation.
pub(super) struct FunctionSetRoot {
    delta: Vector,
    sol: Vector,
    df: Matrix,
    tol: Vector,
    done: bool,
    kount: i32,
    itermax: i32,
    sol_save: Vector,
    gh: Vector,
    dh: Vector,
    dh_save: Vector,
    ff: Vector,
    previous_solution: Vector,
    save: Vector,
}

impl FunctionSetRoot {
    /// `math_FunctionSetRoot(F, Tolerance, NbIterations)` (the one-variable set).
    pub(super) fn new(tolerance: &Vector, nb_iterations: i32) -> Result<Self, MathError> {
        let mut tol = Vector::new(1, 1)?;
        for i in 1..=tol.length() {
            tol.set_value(i, tolerance.value(i)?)?;
        }
        Ok(Self {
            delta: Vector::new(1, 1)?,
            sol: Vector::new(1, 1)?,
            df: Matrix::new(1, 1, 1, 1)?,
            tol,
            done: false,
            kount: 0,
            itermax: nb_iterations,
            sol_save: Vector::new(1, 1)?,
            gh: Vector::new(1, 1)?,
            dh: Vector::new(1, 1)?,
            dh_save: Vector::new(1, 1)?,
            ff: Vector::new(1, 1)?,
            previous_solution: Vector::new(1, 1)?,
            save: Vector::new(0, nb_iterations)?,
        })
    }

    pub(super) fn is_done(&self) -> bool {
        self.done
    }

    pub(super) fn nb_iterations(&self) -> Result<i32, MathError> {
        if self.done { Ok(self.kount) } else { Err(MathError::NotDone) }
    }

    pub(super) fn root(&self) -> Result<f64, MathError> {
        if self.done { self.sol.value(1) } else { Err(MathError::NotDone) }
    }

    pub(super) fn derivative(&self) -> Result<f64, MathError> {
        if self.done { self.df.value(1, 1) } else { Err(MathError::NotDone) }
    }

    /// `IsSolutionReached`.
    fn is_solution_reached(&self) -> Result<bool, MathError> {
        for i in 1..=self.sol.length() {
            if self.delta.value(i)?.abs() > self.tol.value(i)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `Delta(i) = PreviousSolution(i) - Sol(i)`.
    fn delta_from_previous(&mut self) -> Result<(), MathError> {
        for i in self.delta.lower()..=self.delta.upper() {
            self.delta.set_value(i, self.previous_solution.value(i)? - self.sol.value(i)?)?;
        }
        Ok(())
    }

    /// `Sol(i) = Sol(i) + Ambda * DH(i)`.
    fn step(&mut self, ambda: f64) -> Result<(), MathError> {
        for i in self.sol.lower()..=self.sol.upper() {
            self.sol.set_value(i, self.sol.value(i)? + ambda * self.dh.value(i)?)?;
        }
        Ok(())
    }

    /// `Perform(F, StartingPoint, InfBound, SupBound)`.
    #[allow(clippy::cognitive_complexity, unused_assignments)]
    pub(super) fn perform(
        &mut self,
        f_dir: &mut DirFunction,
        starting_point: &Vector,
        inf_bound: &Vector,
        sup_bound: &Vector,
    ) -> Result<(), MathError> {
        let ninc = 1;
        if starting_point.length() != ninc || inf_bound.length() != ninc || sup_bound.length() != ninc {
            return Err(MathError::Dimension);
        }

        let mut change_direction = false;
        let mut sort = false;
        let mut is_new_sol = false;
        const EPS_SQRT: f64 = 1.0e-16;
        const EPS: f64 = 1.0e-32;
        const EPS2: f64 = 1.0e-64;
        const PROGRES: f64 = 0.005;
        let (mut f2, mut gnr1) = (0.0_f64, 0.0_f64);
        let (mut dy, mut old_f);
        let (mut ambda, mut ambda2);
        let mut inv_length_max = Vector::new(1, ninc)?; // blocks the steps to 1/4 of the domain
        let mut a_constraints = IntegerVector::new(1, ninc)?; // on which boundary we are
        for i in 1..=ninc {
            let a_sup_bound = cmin(sup_bound.value(i)?, PRECISION_INFINITE);
            let an_inf_bound = cmax(inf_bound.value(i)?, -PRECISION_INFINITE);
            inv_length_max.set_value(i, 1.0 / cmax((a_sup_bound - an_inf_bound) / 4.0, 1.0e-9))?;
        }

        self.done = false;
        self.sol = starting_point.clone();
        self.kount = 0;

        // validity of the unknowns against the bounds: recentre on the bounds if not valid
        for i in 1..=ninc {
            if self.sol.value(i)? <= inf_bound.value(i)? {
                self.sol.set_value(i, inf_bound.value(i)?)?;
            } else if self.sol.value(i)? > sup_bound.value(i)? {
                self.sol.set_value(i, sup_bound.value(i)?)?;
            }
        }

        // first value of F and its gradient
        if !f_dir.full_value(&self.sol, &mut self.ff, &mut self.df, &mut self.gh, &mut f2, &mut gnr1)? {
            self.done = false;
            return Ok(());
        }
        ambda2 = gnr1;
        // rank 0 of Save is only used for the accelerator test at the end of the loop
        self.save.set_value(0, cmax(f2, EPS_SQRT))?;
        let a_tol_func = epsilon(f2);

        if f2 <= EPS || gnr1 <= EPS2 {
            self.done = true;
            return Ok(());
        }

        self.kount = 1;
        while self.kount <= self.itermax {
            let previous_minimum = f2;
            let oldgr = gnr1;
            self.previous_solution = self.sol.clone();
            self.sol_save = self.sol.clone();

            dy = 0.0;
            search_direction(&self.df, &self.gh, &self.ff, change_direction, &inv_length_max, &mut self.dh, &mut dy)?;
            if dy.abs() <= EPS {
                self.done = true;
                f_dir.set.value(&self.sol, &mut self.ff)?; // update F before the state number
                return Ok(());
            }
            if change_direction {
                ambda = ambda2 / dy.abs().sqrt();
                if ambda > 1.0 {
                    ambda = 1.0;
                }
            } else {
                ambda = 1.0;
                ambda2 = 0.5 * ambda / self.dh.norm();
            }

            self.step(ambda)?;
            let (s, n) = bounds(
                inf_bound,
                sup_bound,
                &self.tol,
                &mut self.sol,
                &self.sol_save,
                &mut a_constraints,
                &mut self.delta,
            )?;
            sort = s;
            is_new_sol = n;

            self.dh_save = self.gh.clone();
            if is_new_sol
                && !f_dir.full_value(&self.sol, &mut self.ff, &mut self.df, &mut self.gh, &mut f2, &mut gnr1)?
            {
                self.done = false;
                return Ok(());
            }

            if f2 <= EPS || gnr1 <= EPS2 {
                self.done = true;
                f_dir.set.value(&self.sol, &mut self.ff)?;
                return Ok(());
            }

            if sort || (f2 / previous_minimum > PROGRES) {
                dy = self.gh.dot(&self.dh)?;
                old_f = previous_minimum;
                let mut stop = false;
                let mut good;
                let mut descente_iter = 0;
                let mut sortbis;

                // standard processing without boundary handling
                if !sort {
                    // if we have not exited, try to progress forward
                    while (f2 / previous_minimum > PROGRES) && !stop {
                        if f2 < old_f && (dy < 0.0) {
                            // try to progress in this direction
                            descente_iter += 1;
                            self.sol_save = self.sol.clone();
                            old_f = f2;
                            self.step(ambda)?;
                            let (s, n) = bounds(
                                inf_bound,
                                sup_bound,
                                &self.tol,
                                &mut self.sol,
                                &self.sol_save,
                                &mut a_constraints,
                                &mut self.delta,
                            )?;
                            stop = s;
                            is_new_sol = n;
                            ambda *= 1.7;
                        } else {
                            if (f2 >= old_f) || (f2 >= previous_minimum) {
                                good = false;
                                if descente_iter == 0 {
                                    // the first step fails: interpolate and minimise if necessary
                                    descente_iter += 1;
                                    good = minimize_direction_2(
                                        &self.sol_save,
                                        &mut self.delta,
                                        old_f,
                                        f2,
                                        &self.dh_save,
                                        &self.gh,
                                        &self.tol,
                                        f_dir,
                                    )?;
                                } else if change_direction || (descente_iter > 1) || (old_f > previous_minimum) {
                                    // the progress was useful: minimise
                                    descente_iter += 1;
                                    good = minimize_direction_3(
                                        &self.previous_solution,
                                        &self.sol_save,
                                        &self.sol,
                                        old_f,
                                        &mut self.delta,
                                        &self.tol,
                                        f_dir,
                                    )?;
                                }
                                if !good {
                                    self.sol = self.sol_save.clone();
                                    f2 = old_f;
                                } else {
                                    self.sol = self.sol_save.added(&self.delta)?;
                                    let (s, n) = bounds(
                                        inf_bound,
                                        sup_bound,
                                        &self.tol,
                                        &mut self.sol,
                                        &self.sol_save,
                                        &mut a_constraints,
                                        &mut self.delta,
                                    )?;
                                    sort = s;
                                    is_new_sol = n;
                                }
                                sort = false; // the point on the boundary was rejected
                            }
                            stop = true; // and leave in any case
                        }
                        self.dh_save = self.gh.clone();
                        if is_new_sol
                            && !f_dir.full_value(
                                &self.sol,
                                &mut self.ff,
                                &mut self.df,
                                &mut self.gh,
                                &mut f2,
                                &mut gnr1,
                            )?
                        {
                            self.done = false;
                            return Ok(());
                        }
                        dy = self.gh.dot(&self.dh)?;
                        if dy.abs() <= EPS {
                            if f2 > old_f {
                                self.sol = self.sol_save.clone();
                            }
                            self.done = true;
                            f_dir.set.value(&self.sol, &mut self.ff)?;
                            return Ok(());
                        }
                        if descente_iter >= 100 {
                            stop = true;
                        }
                    }
                }
                // boundary processing
                if sort {
                    stop = f2 > 1.001 * old_f; // do not progress on the boundary
                    sortbis = sort;
                    descente_iter = 0;
                    while sortbis && ((f2 < old_f) || (descente_iter == 0)) && !stop {
                        descente_iter += 1;
                        // try to progress on the boundary
                        self.sol_save = self.sol.clone();
                        old_f = f2;
                        search_direction_constrained(
                            &self.df,
                            &self.gh,
                            &self.ff,
                            &a_constraints,
                            change_direction,
                            &inv_length_max,
                            &mut self.dh,
                            &mut dy,
                        )?;
                        if dy < -EPS {
                            // avoid useless computations and divisions by 0
                            if change_direction {
                                ambda = ambda2 / (-dy).sqrt();
                                if ambda > 1.0 {
                                    ambda = 1.0;
                                }
                            } else {
                                ambda = 1.0;
                                ambda2 = 0.5 * ambda / self.dh.norm();
                            }

                            self.step(ambda)?;
                            let (s, n) = bounds(
                                inf_bound,
                                sup_bound,
                                &self.tol,
                                &mut self.sol,
                                &self.sol_save,
                                &mut a_constraints,
                                &mut self.delta,
                            )?;
                            sortbis = s;
                            is_new_sol = n;

                            self.dh_save = self.gh.clone();
                            if is_new_sol
                                && !f_dir.full_value(
                                    &self.sol,
                                    &mut self.ff,
                                    &mut self.df,
                                    &mut self.gh,
                                    &mut f2,
                                    &mut gnr1,
                                )?
                            {
                                self.done = false;
                                return Ok(());
                            }
                            ambda2 = gnr1;
                        } else {
                            stop = true;
                        }

                        while (f2 / previous_minimum > PROGRES) && (f2 < old_f) && !stop {
                            descente_iter += 1;
                            if f2 < old_f && dy < 0.0 {
                                // try to progress in this direction
                                self.sol_save = self.sol.clone();
                                old_f = f2;
                                self.step(ambda)?;
                                let (s, n) = bounds(
                                    inf_bound,
                                    sup_bound,
                                    &self.tol,
                                    &mut self.sol,
                                    &self.sol_save,
                                    &mut a_constraints,
                                    &mut self.delta,
                                )?;
                                sortbis = s;
                                is_new_sol = n;
                            }
                            self.dh_save = self.gh.clone();
                            if is_new_sol
                                && !f_dir.full_value(
                                    &self.sol,
                                    &mut self.ff,
                                    &mut self.df,
                                    &mut self.gh,
                                    &mut f2,
                                    &mut gnr1,
                                )?
                            {
                                self.done = false;
                                return Ok(());
                            }
                            ambda2 = gnr1;
                            dy = self.gh.dot(&self.dh)?;
                            stop = (dy >= 0.0) || (descente_iter >= 10) || sortbis;
                        }
                        stop = (dy >= 0.0) || (descente_iter >= 10);
                    }
                    if ((f2 / previous_minimum > PROGRES) && (f2 >= old_f)) || (f2 >= previous_minimum) {
                        // minimise by Brent
                        good = minimize_direction_2(
                            &self.sol_save,
                            &mut self.delta,
                            old_f,
                            f2,
                            &self.dh_save,
                            &self.gh,
                            &self.tol,
                            f_dir,
                        )?;
                        if !good {
                            self.sol = self.sol_save.clone();
                            sort = false;
                        } else {
                            self.sol = self.sol_save.added(&self.delta)?;
                            let (s, n) = bounds(
                                inf_bound,
                                sup_bound,
                                &self.tol,
                                &mut self.sol,
                                &self.sol_save,
                                &mut a_constraints,
                                &mut self.delta,
                            )?;
                            sort = s;
                            is_new_sol = n;
                            if is_new_sol
                                && !f_dir.full_value(
                                    &self.sol,
                                    &mut self.ff,
                                    &mut self.df,
                                    &mut self.gh,
                                    &mut f2,
                                    &mut gnr1,
                                )?
                            {
                                self.done = false;
                                return Ok(());
                            }
                        }
                        dy = self.gh.dot(&self.dh)?;
                    }
                }
            }

            // stopping tests
            self.save.set_value(self.kount, f2)?;
            // is it the solution?
            let verif;
            if change_direction {
                verif = true; // gradient: avoid looping
            } else if self.kount > 1 {
                // accelerates the quasi-quadratic cases
                verif = self.save.value(self.kount - 1)? < 1.0e-4 * self.save.value(self.kount - 2)?;
            } else {
                verif = f2 < 1.0e-6 * self.save.value(0)?; // for the cases that already are solutions
            }
            if verif {
                self.delta_from_previous()?;
                if self.is_solution_reached()? {
                    if previous_minimum < f2 {
                        self.sol = self.sol_save.clone();
                    }
                    self.done = true;
                    f_dir.set.value(&self.sol, &mut self.ff)?;
                    return Ok(());
                }
            }
            // end of the solution test

            // analysis of the progression: compare the current and previous minima
            if (f2 - previous_minimum) <= a_tol_func {
                if self.kount > 5 {
                    // is the history good?
                    if f2 >= 0.95 * self.save.value(self.kount - 5)? {
                        if !change_direction {
                            change_direction = true;
                        } else {
                            self.done = true;
                            return Ok(()); // a gain below 5% leaves
                        }
                    } else {
                        change_direction = false; // if yes, restart
                    }
                } else {
                    change_direction = false; // no history, continue
                }
                // if the gradient does not decrease enough with Newton, try the gradient method unless f decreases
                if (gnr1 > 0.9 * oldgr) && (f2 > 0.5 * previous_minimum) {
                    change_direction = true;
                }

                // if the strategy is not changed, verify if not already done
                if !change_direction && !verif {
                    self.delta_from_previous()?;
                    if self.is_solution_reached()? {
                        self.done = true;
                        f_dir.set.value(&self.sol, &mut self.ff)?;
                        return Ok(());
                    }
                }
            } else {
                // regression
                if !change_direction {
                    // switch to the gradient
                    change_direction = true;
                    self.sol = self.previous_solution.clone();
                    if !f_dir.full_value(&self.sol, &mut self.ff, &mut self.df, &mut self.gh, &mut f2, &mut gnr1)? {
                        self.done = false;
                        return Ok(());
                    }
                } else {
                    return Ok(()); // no way out
                }
            }
            self.kount = self.kount.wrapping_add(1);
        }
        let _ = (sort, is_new_sol);
        Ok(())
    }
}
