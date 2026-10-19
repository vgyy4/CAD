//! `math_FunctionSetRoot` (`math_FunctionSetRoot.cxx`), with its file-local `MyDirFunction`,
//! `MinimizeDirection` (two overloads), `SearchDirection` (two overloads) and `Bounds`.
//!
//! The operations and the callback calls are in the order of the C++ source.
#![allow(clippy::float_cmp)]

use super::{FunctionSetRootState, FunctionSetRootTest, FunctionSetWithDerivatives, vector_dump};
use crate::math1d::{BrentMinimum, Function, PRECISION_INFINITE, SQUARE_CONFUSION, cmax, cmin, epsilon, is_infinite};
use crate::mathlin::{Gauss, GaussLeastSquare, IntegerVector, MathError, Matrix, Svd, Vector};

/// `MyDirFunction`: the function set along a direction. `Temp1..Temp4` of the solver are its own vectors.
struct DirFunction<'a> {
    p0: Vector,
    dir: Vector,
    p: Vector,
    fv: Vector,
    f: &'a mut dyn FunctionSetWithDerivatives,
}

impl<'a> DirFunction<'a> {
    fn new(f: &'a mut dyn FunctionSetWithDerivatives, n: i32, m: i32) -> Result<Self, MathError> {
        Ok(Self { p0: Vector::new(1, n)?, dir: Vector::new(1, n)?, p: Vector::new(1, n)?, fv: Vector::new(1, m)?, f })
    }

    /// `Initialize`.
    fn initialize(&mut self, p0: &Vector, dir: &Vector) -> Result<(), MathError> {
        self.p0.initialized(p0)?;
        self.dir.initialized(dir)
    }

    /// `Value(const double x, double& fval)`: `None` is `return false`.
    fn eval(&mut self, x: f64) -> Result<Option<f64>, MathError> {
        for i in self.p.lower()..=self.p.upper() {
            let p = self.dir.value(i)?;
            self.p.set_value(i, p * x + self.p0.value(i)?)?;
        }
        if self.f.value(&self.p, &mut self.fv) {
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
        if self.f.values(sol, ff, df) {
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
    f: &mut DirFunction<'_>,
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
        return Ok(false);
    }
    tol1d /= 3.0;

    let d = p1.subtracted(p0)?;
    delta.initialized(&d)?;
    let mut invnorme = delta.norm();
    if invnorme <= eps {
        return Ok(false);
    }
    invnorme = 1.0 / invnorme;

    f.initialize(p1, delta)?;

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
    f: &mut DirFunction<'_>,
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

    f.initialize(p, dir)?;
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
    let _ = result;

    if good {
        // update Delta
        dir.multiply(tsol);
    }
    Ok(good)
}

/// `SearchDirection` without constraints (`ChangeDirection` is passed by value in C++).
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
                let x = solut.solve_in_place(direction.clone())?;
                direction.initialized(&x)?;
            } else {
                // singular directions have to be "forgotten"
                let solve_by_svd = Svd::new(df)?;
                if solve_by_svd.is_done() {
                    let x = solve_by_svd.solve(&ff.t_multiplied(-1.0), 1.0e-6)?;
                    direction.initialized(&x)?;
                } else {
                    change_direction = true;
                }
            }
        } else if ninc > neq {
            let solut = Svd::new(df)?;
            if solut.is_done() {
                let x = solut.solve(&ff.t_multiplied(-1.0), 1.0e-6)?;
                direction.initialized(&x)?;
            } else {
                change_direction = true;
            }
        } else {
            // Calcul par GaussLeastSquare
            let solut = GaussLeastSquare::new(df, 1.0e-20)?;
            if solut.is_done() {
                let x = solut.solve(&ff.t_multiplied(-1.0))?;
                direction.initialized(&x)?;
            } else {
                change_direction = true;
            }
        }
    }
    // too long directions are forbidden, to armour the ill-conditioned cases
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

/// `SearchDirection` along a boundary.
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
    let neq = df.row_number();
    let mut cons = 0;

    for i in 1..=ninc {
        if constraints.value(i)? != 0 {
            cons += 1;
        }
    }

    if cons == 0 {
        search_direction(df, gh, ff, change_direction, inv_length_max, direction, dy)?;
    } else if cons == ninc {
        for i in direction.lower()..=direction.upper() {
            direction.set_value(i, 0.0)?;
        }
        *dy = 0.0;
    } else {
        // (1) general case: a sub-problem
        let mut df2 = Matrix::new(1, neq, 1, ninc - cons)?;
        let mut my_gh = Vector::new(1, ninc - cons)?;
        let mut my_direction = Vector::new(1, ninc - cons)?;
        let mut my_inv_length_max = Vector::new(1, ninc)?;

        let mut k = 1;
        for i in 1..=ninc {
            if constraints.value(i)? == 0 {
                my_gh.set_value(k, gh.value(i)?)?;
                my_inv_length_max.set_value(k, inv_length_max.value(i)?)?;
                my_direction.set_value(k, direction.value(i)?)?;
                for j in 1..=neq {
                    df2.set_value(j, k, df.value(j, i)?)?;
                }
                k += 1;
            }
        }
        // (2) solve it
        search_direction(&df2, &my_gh, ff, change_direction, &my_inv_length_max, &mut my_direction, dy)?;

        // (3) interpret it: rebuild Direction
        k = 1;
        for i in 1..=ninc {
            if constraints.value(i)? == 0 {
                if !change_direction {
                    direction.set_value(i, my_direction.value(k)?)?;
                } else {
                    direction.set_value(i, -gh.value(i)?)?;
                }
                k += 1;
            } else {
                direction.set_value(i, 0.0)?;
            }
        }
    }
    Ok(())
}

/// `Bounds`: trims `sol` to the domain; `delta` gets the distance between the final `sol` and `sol_save`.
/// Returns `Out`; `is_new_sol` is false if the final `sol` coincides with `sol_save`.
#[allow(clippy::too_many_arguments)]
fn bounds(
    inf_bound: &Vector,
    sup_bound: &Vector,
    tol: &Vector,
    sol: &mut Vector,
    sol_save: &Vector,
    constraints: &mut IntegerVector,
    delta: &mut Vector,
    is_new_sol: &mut bool,
) -> Result<bool, MathError> {
    let mut out = false;
    let ninc = sol.length();
    let mut monratio = 1.0_f64;

    *is_new_sol = true;

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
            *is_new_sol = false;
            sol.initialized(sol_save)?;
            delta.init(0.0);
        } else {
            delta.multiply(monratio);
            let s = sol_save.added(delta)?;
            sol.initialized(&s)?;
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
    Ok(out)
}

/// `Root = Sol` / `Err = Delta` in the library, where the checks are compiled out. Wrong lengths are `Dimension` errors
/// when the solver is done (the golden file's `unchecked`); when it is not done the golden file copies the common prefix.
fn copy_checked(done: bool, out: &mut Vector, from: &Vector) -> Result<(), MathError> {
    if out.length() == from.length() {
        return out.initialized(from);
    }
    if done {
        return Err(MathError::Dimension);
    }
    let first = out.lower();
    for k in 0..out.length().min(from.length()) {
        out.set_value(first + k, from.value(from.lower() + k)?)?;
    }
    Ok(())
}

struct DefaultTest;
impl FunctionSetRootTest for DefaultTest {
    fn is_solution_reached(&mut self, s: &FunctionSetRootState<'_>, _: &mut dyn FunctionSetWithDerivatives) -> bool {
        s.is_solution_reached_default()
    }
}

/// `math_FunctionSetRoot`.
#[derive(Debug, Clone)]
pub struct FunctionSetRoot {
    delta: Vector,
    sol: Vector,
    df: Matrix,
    tol: Vector,
    done: bool,
    kount: i32,
    state: i32,
    itermax: i32,
    inf_bound: Vector,
    sup_bound: Vector,
    sol_save: Vector,
    gh: Vector,
    dh: Vector,
    dh_save: Vector,
    ff: Vector,
    previous_solution: Vector,
    save: Vector,
    is_divergent: bool,
}

impl FunctionSetRoot {
    /// `math_FunctionSetRoot(F, Tolerance, NbIterations)`.
    pub fn new(f: &dyn FunctionSetWithDerivatives, tolerance: &Vector, nb_iterations: i32) -> Result<Self, MathError> {
        let mut s = Self::without_tolerance(f, nb_iterations)?;
        s.set_tolerance(tolerance)?;
        Ok(s)
    }

    /// `math_FunctionSetRoot(F, NbIterations)`: `set_tolerance` must be called.
    pub fn without_tolerance(f: &dyn FunctionSetWithDerivatives, nb_iterations: i32) -> Result<Self, MathError> {
        let n = f.nb_variables();
        let m = f.nb_equations();
        Ok(Self {
            delta: Vector::new(1, n)?,
            sol: Vector::new(1, n)?,
            df: Matrix::new(1, m, 1, n)?,
            tol: Vector::new(1, n)?,
            done: false,
            kount: 0,
            state: 0,
            itermax: nb_iterations,
            inf_bound: Vector::filled(1, n, -f64::MAX)?,
            sup_bound: Vector::filled(1, n, f64::MAX)?,
            sol_save: Vector::new(1, n)?,
            gh: Vector::new(1, n)?,
            dh: Vector::new(1, n)?,
            dh_save: Vector::new(1, n)?,
            ff: Vector::new(1, m)?,
            previous_solution: Vector::new(1, n)?,
            save: Vector::new(0, nb_iterations)?,
            is_divergent: false,
        })
    }

    /// `SetTolerance`.
    pub fn set_tolerance(&mut self, tolerance: &Vector) -> Result<(), MathError> {
        for i in 1..=self.tol.length() {
            self.tol.set_value(i, tolerance.value(i)?)?;
        }
        Ok(())
    }

    /// `Perform(F, StartingPoint, theStopOnDivergent)`.
    pub fn perform(
        &mut self,
        f: &mut dyn FunctionSetWithDerivatives,
        starting_point: &Vector,
        stop_on_divergent: bool,
    ) -> Result<(), MathError> {
        self.perform_with_test(f, starting_point, stop_on_divergent, &mut DefaultTest)
    }

    /// `Perform(F, StartingPoint, InfBound, SupBound, theStopOnDivergent)`.
    pub fn perform_bounded(
        &mut self,
        f: &mut dyn FunctionSetWithDerivatives,
        starting_point: &Vector,
        inf_bound: &Vector,
        sup_bound: &Vector,
        stop_on_divergent: bool,
    ) -> Result<(), MathError> {
        self.perform_bounded_with_test(f, starting_point, inf_bound, sup_bound, stop_on_divergent, &mut DefaultTest)
    }

    /// `Perform(F, StartingPoint, theStopOnDivergent)` with a redefined `IsSolutionReached`.
    pub fn perform_with_test(
        &mut self,
        f: &mut dyn FunctionSetWithDerivatives,
        starting_point: &Vector,
        stop_on_divergent: bool,
        test: &mut dyn FunctionSetRootTest,
    ) -> Result<(), MathError> {
        let (lo, hi) = (self.inf_bound.clone(), self.sup_bound.clone());
        self.perform_bounded_with_test(f, starting_point, &lo, &hi, stop_on_divergent, test)
    }

    fn update_divergent(&mut self, lo: &Vector, hi: &Vector) -> Result<(), MathError> {
        for i in 1..=self.sol.length() {
            self.is_divergent =
                self.is_divergent || self.sol.value(i)? < lo.value(i)? || self.sol.value(i)? > hi.value(i)?;
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

    #[allow(clippy::too_many_arguments)]
    fn call_bounds(
        &mut self,
        lo: &Vector,
        hi: &Vector,
        constraints: &mut IntegerVector,
        is_new_sol: &mut bool,
    ) -> Result<bool, MathError> {
        bounds(lo, hi, &self.tol, &mut self.sol, &self.sol_save, constraints, &mut self.delta, is_new_sol)
    }

    fn eval_full(&mut self, f_dir: &mut DirFunction<'_>, f2: &mut f64, gnr1: &mut f64) -> Result<bool, MathError> {
        f_dir.full_value(&self.sol, &mut self.ff, &mut self.df, &mut self.gh, f2, gnr1)
    }

    /// The failure exit of the first value of `F`: `Done = false; if (...) State = F.GetStateNumber()`.
    fn fail_exit(&mut self, f_dir: &mut DirFunction<'_>, stop: bool) {
        self.done = false;
        if !stop || !self.is_divergent {
            self.state = f_dir.f.state_number();
        }
    }

    /// The success exit: `Done = false; if (...) { Done = true; [F.Value(Sol, FF);] State = ...; }`.
    fn done_exit(&mut self, f_dir: &mut DirFunction<'_>, stop: bool, update_f: bool) {
        self.done = false;
        if !stop || !self.is_divergent {
            self.done = true;
            if update_f {
                f_dir.f.value(&self.sol, &mut self.ff);
            }
            self.state = f_dir.f.state_number();
        }
    }

    fn solution_reached(&mut self, f_dir: &mut DirFunction<'_>, test: &mut dyn FunctionSetRootTest) -> bool {
        let state = FunctionSetRootState { delta: &self.delta, sol: &self.sol, df: &self.df, tol: &self.tol };
        test.is_solution_reached(&state, &mut *f_dir.f)
    }

    fn delta_from_previous(&mut self) -> Result<(), MathError> {
        for i in self.delta.lower()..=self.delta.upper() {
            self.delta.set_value(i, self.previous_solution.value(i)? - self.sol.value(i)?)?;
        }
        Ok(())
    }

    /// `Perform(F, StartingPoint, InfBound, SupBound, theStopOnDivergent)` with a redefined `IsSolutionReached`.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity, clippy::too_many_arguments, unused_assignments)]
    pub fn perform_bounded_with_test(
        &mut self,
        f: &mut dyn FunctionSetWithDerivatives,
        starting_point: &Vector,
        the_inf_bound: &Vector,
        the_sup_bound: &Vector,
        stop_on_divergent: bool,
        test: &mut dyn FunctionSetRootTest,
    ) -> Result<(), MathError> {
        let ninc = f.nb_variables();
        let neq = f.nb_equations();

        if neq <= 0
            || starting_point.length() != ninc
            || the_inf_bound.length() != ninc
            || the_sup_bound.length() != ninc
        {
            return Err(MathError::Dimension);
        }

        let mut change_direction = false;
        let mut sort;
        let mut is_new_sol = false;
        const EPS_SQRT: f64 = 1.0e-16;
        const EPS: f64 = 1.0e-32;
        const EPS2: f64 = 1.0e-64;
        const PROGRES: f64 = 0.005;
        let (mut f2, mut gnr1) = (0.0_f64, 0.0_f64);
        let mut dy = 0.0_f64;
        let mut old_f;
        let (mut ambda, mut ambda2);
        let mut inv_length_max = Vector::new(1, ninc)?; // blocks the steps to 1/4 of the domain
        let mut a_constraints = IntegerVector::new(1, ninc)?; // on which boundary we are
        for i in 1..=ninc {
            let a_sup_bound = cmin(the_sup_bound.value(i)?, PRECISION_INFINITE);
            let an_inf_bound = cmax(the_inf_bound.value(i)?, -PRECISION_INFINITE);
            inv_length_max.set_value(i, 1.0 / cmax((a_sup_bound - an_inf_bound) / 4.0, 1.0e-9))?;
        }

        let mut f_dir = DirFunction::new(f, ninc, neq)?;

        self.done = false;
        self.sol.initialized(starting_point)?;
        self.kount = 0;

        self.is_divergent = false;
        self.update_divergent(the_inf_bound, the_sup_bound)?;
        if stop_on_divergent && self.is_divergent {
            return Ok(());
        }

        // validity of the unknowns against the bounds: recentre on the bounds if not valid
        for i in 1..=ninc {
            if self.sol.value(i)? <= the_inf_bound.value(i)? {
                self.sol.set_value(i, the_inf_bound.value(i)?)?;
            } else if self.sol.value(i)? > the_sup_bound.value(i)? {
                self.sol.set_value(i, the_sup_bound.value(i)?)?;
            }
        }

        // first value of F and its gradient
        if !self.eval_full(&mut f_dir, &mut f2, &mut gnr1)? {
            self.fail_exit(&mut f_dir, stop_on_divergent);
            return Ok(());
        }
        ambda2 = gnr1;
        // rank 0 of Save is only used for the accelerator test at the end of the loop
        self.save.set_value(0, cmax(f2, EPS_SQRT))?;
        let a_tol_func = epsilon(f2);

        if f2 <= EPS || gnr1 <= EPS2 {
            self.done_exit(&mut f_dir, stop_on_divergent, false);
            return Ok(());
        }
        // the C++ `Done = true` here has no `F.Value` update; `done_exit` with `false` matches it

        self.kount = 1;
        while self.kount <= self.itermax {
            let previous_minimum = f2;
            let oldgr = gnr1;
            self.previous_solution.initialized(&self.sol)?;
            self.sol_save.initialized(&self.sol)?;

            search_direction(&self.df, &self.gh, &self.ff, change_direction, &inv_length_max, &mut self.dh, &mut dy)?;
            if dy.abs() <= EPS {
                self.done_exit(&mut f_dir, stop_on_divergent, true);
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
            self.update_divergent(the_inf_bound, the_sup_bound)?;
            if stop_on_divergent && self.is_divergent {
                return Ok(());
            }
            sort = self.call_bounds(the_inf_bound, the_sup_bound, &mut a_constraints, &mut is_new_sol)?;

            self.dh_save.initialized(&self.gh)?;
            if is_new_sol && !self.eval_full(&mut f_dir, &mut f2, &mut gnr1)? {
                self.fail_exit(&mut f_dir, stop_on_divergent);
                return Ok(());
            }

            if f2 <= EPS || gnr1 <= EPS2 {
                self.done_exit(&mut f_dir, stop_on_divergent, true);
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
                            self.sol_save.initialized(&self.sol)?;
                            old_f = f2;
                            self.step(ambda)?;
                            self.update_divergent(the_inf_bound, the_sup_bound)?;
                            if stop_on_divergent && self.is_divergent {
                                return Ok(());
                            }
                            stop =
                                self.call_bounds(the_inf_bound, the_sup_bound, &mut a_constraints, &mut is_new_sol)?;
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
                                        &mut f_dir,
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
                                        &mut f_dir,
                                    )?;
                                }
                                if !good {
                                    self.sol.initialized(&self.sol_save)?;
                                    f2 = old_f;
                                } else {
                                    let s = self.sol_save.added(&self.delta)?;
                                    self.sol.initialized(&s)?;
                                    self.update_divergent(the_inf_bound, the_sup_bound)?;
                                    if stop_on_divergent && self.is_divergent {
                                        return Ok(());
                                    }
                                    let _ = self.call_bounds(
                                        the_inf_bound,
                                        the_sup_bound,
                                        &mut a_constraints,
                                        &mut is_new_sol,
                                    )?;
                                }
                                sort = false; // the point on the boundary was rejected
                            }
                            stop = true; // and leave in any case
                        }
                        self.dh_save.initialized(&self.gh)?;
                        if is_new_sol && !self.eval_full(&mut f_dir, &mut f2, &mut gnr1)? {
                            self.fail_exit(&mut f_dir, stop_on_divergent);
                            return Ok(());
                        }
                        dy = self.gh.dot(&self.dh)?;
                        if dy.abs() <= EPS {
                            if f2 > old_f {
                                self.sol.initialized(&self.sol_save)?;
                            }
                            self.done_exit(&mut f_dir, stop_on_divergent, true);
                            return Ok(());
                        }
                        if descente_iter >= 100 {
                            stop = true;
                        }
                    }
                }
                // processing of the boundaries
                if sort {
                    stop = f2 > 1.001 * old_f; // do not progress on the boundary
                    sortbis = sort;
                    descente_iter = 0;
                    while sortbis && ((f2 < old_f) || (descente_iter == 0)) && !stop {
                        descente_iter += 1;
                        // try to progress on the boundary
                        self.sol_save.initialized(&self.sol)?;
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
                            self.update_divergent(the_inf_bound, the_sup_bound)?;
                            if stop_on_divergent && self.is_divergent {
                                return Ok(());
                            }
                            sortbis =
                                self.call_bounds(the_inf_bound, the_sup_bound, &mut a_constraints, &mut is_new_sol)?;

                            self.dh_save.initialized(&self.gh)?;
                            if is_new_sol && !self.eval_full(&mut f_dir, &mut f2, &mut gnr1)? {
                                self.fail_exit(&mut f_dir, stop_on_divergent);
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
                                self.sol_save.initialized(&self.sol)?;
                                old_f = f2;
                                self.step(ambda)?;
                                self.update_divergent(the_inf_bound, the_sup_bound)?;
                                if stop_on_divergent && self.is_divergent {
                                    return Ok(());
                                }
                                sortbis = self.call_bounds(
                                    the_inf_bound,
                                    the_sup_bound,
                                    &mut a_constraints,
                                    &mut is_new_sol,
                                )?;
                            }
                            self.dh_save.initialized(&self.gh)?;
                            if is_new_sol && !self.eval_full(&mut f_dir, &mut f2, &mut gnr1)? {
                                self.fail_exit(&mut f_dir, stop_on_divergent);
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
                        descente_iter += 1;
                        good = minimize_direction_2(
                            &self.sol_save,
                            &mut self.delta,
                            old_f,
                            f2,
                            &self.dh_save,
                            &self.gh,
                            &self.tol,
                            &mut f_dir,
                        )?;
                        if !good {
                            self.sol.initialized(&self.sol_save)?;
                            sort = false;
                        } else {
                            let s = self.sol_save.added(&self.delta)?;
                            self.sol.initialized(&s)?;
                            self.update_divergent(the_inf_bound, the_sup_bound)?;
                            if stop_on_divergent && self.is_divergent {
                                return Ok(());
                            }
                            sort =
                                self.call_bounds(the_inf_bound, the_sup_bound, &mut a_constraints, &mut is_new_sol)?;
                            if is_new_sol && !self.eval_full(&mut f_dir, &mut f2, &mut gnr1)? {
                                self.fail_exit(&mut f_dir, stop_on_divergent);
                                return Ok(());
                            }
                        }
                        dy = self.gh.dot(&self.dh)?;
                    }
                }
                let _ = (descente_iter, sort);
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
                if self.solution_reached(&mut f_dir, test) {
                    if previous_minimum < f2 {
                        self.sol.initialized(&self.sol_save)?;
                    }
                    self.done_exit(&mut f_dir, stop_on_divergent, true);
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
                            self.done_exit(&mut f_dir, stop_on_divergent, false);
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
                    if self.solution_reached(&mut f_dir, test) {
                        self.done_exit(&mut f_dir, stop_on_divergent, true);
                        return Ok(());
                    }
                }
            } else {
                // regression
                if !change_direction {
                    // switch to the gradient
                    change_direction = true;
                    self.sol.initialized(&self.previous_solution)?;
                    if !self.eval_full(&mut f_dir, &mut f2, &mut gnr1)? {
                        self.fail_exit(&mut f_dir, stop_on_divergent);
                        return Ok(());
                    }
                } else {
                    if !stop_on_divergent || !self.is_divergent {
                        self.state = f_dir.f.state_number();
                    }
                    return Ok(()); // no way out
                }
            }
            self.kount += 1;
        }
        if !stop_on_divergent || !self.is_divergent {
            self.state = f_dir.f.state_number();
        }
        Ok(())
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `IsDivergent`.
    pub fn is_divergent(&self) -> bool {
        self.is_divergent
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    /// `NbIterations`.
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.kount)
    }
    /// `StateNumber`.
    pub fn state_number(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.state)
    }
    /// `Root()`.
    pub fn root(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.sol)
    }
    /// `Root(math_Vector&)`.
    pub fn root_into(&self, o: &mut Vector) -> Result<(), MathError> {
        // the library is built without `StdFail_NotDone_Raise_if`: no `NotDone` check here
        copy_checked(self.done, o, &self.sol)
    }
    /// `Derivative()`.
    pub fn derivative(&self) -> Result<&Matrix, MathError> {
        self.check()?;
        Ok(&self.df)
    }
    /// `Derivative(math_Matrix&)`.
    pub fn derivative_into(&self, o: &mut Matrix) -> Result<(), MathError> {
        self.check()?;
        if o.col_number() != self.sol.length() || o.row_number() != self.df.row_number() {
            return Err(MathError::Dimension);
        }
        o.initialize_from(&self.df)
    }
    /// `FunctionSetErrors()`.
    pub fn function_set_errors(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.delta)
    }
    /// `FunctionSetErrors(math_Vector&)`.
    pub fn function_set_errors_into(&self, o: &mut Vector) -> Result<(), MathError> {
        // the library is built without `StdFail_NotDone_Raise_if`: no `NotDone` check here
        copy_checked(self.done, o, &self.delta)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from(" math_FunctionSetRoot");
        if self.done {
            s.push_str(" Status = Done\n");
            s.push_str(&format!(" Location value = {}\n", vector_dump(&self.sol)));
            s.push_str(&format!(" Number of iterations = {}\n", self.kount));
        } else {
            s.push_str("Status = Not Done\n");
        }
        s
    }
}
