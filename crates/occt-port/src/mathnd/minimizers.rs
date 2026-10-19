//! `math_Powell` (`math_Powell.cxx`), `math_FRPR` (`math_FRPR.cxx`) and `math_BFGS` (`math_BFGS.cxx`), with their
//! file-local `DirFunctionBis` / `DirFunctionTer` / `DirFunction`, `MinimizeDirection`, `ComputeInitScale` and
//! `ComputeMinMaxScale`.
#![allow(clippy::float_cmp)]

use super::{
    BfgsState, BfgsTest, FrprState, FrprTest, MultipleVarFunction, MultipleVarFunctionWithGradient, PowellState,
    PowellTest, fmt, vector_dump,
};
use crate::math1d::{
    BracketMinimum, BrentMinimum, Function, FunctionWithDerivative, P_CONFUSION, PRECISION_INFINITE, cmax, cmin,
};
use crate::mathlin::{MathError, MathStatus, Matrix, Vector};

/// `DirFunctionBis` / `DirFunctionTer`: the function along a direction.
struct PlainLine<'a, F: ?Sized + MultipleVarFunction> {
    p0: Vector,
    dir: Vector,
    p: Vector,
    f: &'a mut F,
}

impl<'a, F: ?Sized + MultipleVarFunction> PlainLine<'a, F> {
    fn new(f: &'a mut F, n: i32) -> Result<Self, MathError> {
        Ok(Self { p0: Vector::new(1, n)?, dir: Vector::new(1, n)?, p: Vector::new(1, n)?, f })
    }
    fn initialize(&mut self, p0: &Vector, dir: &Vector) -> Result<(), MathError> {
        self.p0.initialized(p0)?;
        self.dir.initialized(dir)
    }
    fn eval(&mut self, x: f64) -> Result<Option<f64>, MathError> {
        self.p.initialized(&self.dir)?;
        self.p.multiply(x);
        self.p.add(&self.p0)?;
        Ok(self.f.value(&self.p))
    }
}

impl<F: ?Sized + MultipleVarFunction> Function for PlainLine<'_, F> {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.eval(x).ok().flatten()
    }
}

/// `MinimizeDirection` of `math_Powell.cxx` / `math_FRPR.cxx`.
fn minimize_plain<F: ?Sized + MultipleVarFunction>(
    p: &mut Vector,
    dir: &mut Vector,
    result: &mut f64,
    f: &mut PlainLine<'_, F>,
) -> Result<bool, MathError> {
    f.initialize(p, dir)?;
    let bracket = BracketMinimum::with_function(f, 0.0, 1.0);
    if bracket.is_done() {
        let (ax, xx, bx) = bracket.values()?;
        let mut sol = BrentMinimum::new(1.0e-10, 100, 1.0e-12);
        sol.perform(f, ax, xx, bx);
        if sol.is_done() {
            let scale = sol.location()?;
            *result = sol.minimum()?;
            dir.multiply(scale);
            p.add(dir)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn sqr(a: f64) -> f64 {
    a * a
}

// ---------------------------------------------------------------------------------------------------------------
// math_Powell

struct DefaultPowellTest;
impl PowellTest for DefaultPowellTest {
    fn is_solution_reached(&mut self, s: &PowellState<'_>, _: &mut dyn MultipleVarFunction) -> bool {
        s.is_solution_reached_default()
    }
}

/// `math_Powell`.
#[derive(Debug, Clone)]
pub struct Powell {
    location: Vector,
    minimum: f64,
    location_error: f64,
    previous_minimum: f64,
    x_tol: f64,
    eps_z: f64,
    done: bool,
    iter: i32,
    status: MathStatus,
    directions: Matrix,
    state: i32,
    itermax: i32,
}

impl Powell {
    /// `math_Powell(F, Tolerance, NbIterations = 200, ZEPS = 1.0e-12)`.
    pub fn new(f: &dyn MultipleVarFunction, tolerance: f64, nb_iterations: i32, zeps: f64) -> Result<Self, MathError> {
        let n = f.nb_variables();
        Ok(Self {
            location: Vector::new(1, n)?,
            minimum: f64::MAX,
            location_error: f64::MAX,
            previous_minimum: f64::MAX,
            x_tol: tolerance,
            eps_z: zeps,
            done: false,
            iter: 0,
            status: MathStatus::NotBracketed,
            directions: Matrix::new(1, n, 1, n)?,
            state: 0,
            itermax: nb_iterations,
        })
    }

    /// `Perform(F, StartingPoint, StartingDirections)`.
    pub fn perform(&mut self, f: &mut dyn MultipleVarFunction, start: &Vector, dirs: &Matrix) -> Result<(), MathError> {
        self.perform_with_test(f, start, dirs, &mut DefaultPowellTest)
    }

    /// `Perform` of a sub-class redefining `IsSolutionReached`.
    pub fn perform_with_test(
        &mut self,
        f: &mut dyn MultipleVarFunction,
        start: &Vector,
        dirs: &Matrix,
        test: &mut dyn PowellTest,
    ) -> Result<(), MathError> {
        self.done = false;
        let n = self.location.length();
        let mut pt = Vector::new(1, n)?;
        let mut ptt = Vector::new(1, n)?;
        let mut xit = Vector::new(1, n)?;
        let mut f_dir = PlainLine::new(f, n)?;
        let mut fptt = 0.0_f64;

        self.location.initialized(start)?;
        self.directions.initialized(dirs)?;
        pt.initialized(&self.location)?; // saves the initial point

        self.iter = 1;
        while self.iter <= self.itermax {
            if let Some(v) = f_dir.f.value(&self.location) {
                self.previous_minimum = v;
            }
            let mut ibig = 0;
            let mut del = 0.0_f64;
            for i in 1..=n {
                for j in 1..=n {
                    xit.set_value(j, self.directions.value(j, i)?)?;
                }
                if let Some(v) = f_dir.f.value(&self.location) {
                    fptt = v;
                }
                let is_good = minimize_plain(&mut self.location, &mut xit, &mut self.minimum, &mut f_dir)?;

                if !is_good {
                    self.done = false;
                    self.status = MathStatus::DirectionSearchError;
                    return Ok(());
                }

                if (fptt - self.minimum).abs() > del {
                    del = (fptt - self.minimum).abs();
                    ibig = i;
                }
            }

            let state = PowellState {
                location: &self.location,
                minimum: self.minimum,
                previous_minimum: self.previous_minimum,
                location_error: self.location_error,
                x_tol: self.x_tol,
                eps_z: self.eps_z,
            };
            if test.is_solution_reached(&state, &mut *f_dir.f) {
                // termination criterion
                self.state = f_dir.f.state_number();
                self.done = true;
                self.status = MathStatus::Ok;
                return Ok(());
            }

            if self.iter == self.itermax {
                self.done = false;
                self.status = MathStatus::TooManyIterations;
                return Ok(());
            }

            // ptt = 2.0 * TheLocation - pt
            ptt.initialized(&self.location.multiplied(2.0).subtracted(&pt)?)?;
            xit.initialized(&self.location.subtracted(&pt)?)?;
            pt.initialized(&self.location)?;

            // value of the function at the extrapolated point
            if let Some(v) = f_dir.f.value(&ptt) {
                fptt = v;
            }

            if fptt < self.previous_minimum {
                let t = 2.0
                    * (self.previous_minimum - 2.0 * self.minimum + fptt)
                    * sqr(self.previous_minimum - self.minimum - del)
                    - del * sqr(self.previous_minimum - fptt);
                if t < 0.0 {
                    // minimisation along the direction
                    let is_good = minimize_plain(&mut self.location, &mut xit, &mut self.minimum, &mut f_dir)?;
                    if !is_good {
                        self.done = false;
                        self.status = MathStatus::FunctionError;
                        return Ok(());
                    }

                    for j in 1..=n {
                        self.directions.set_value(j, ibig, xit.value(j)?)?;
                    }
                }
            }
            self.iter += 1;
        }
        Ok(())
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    /// `Location()`.
    pub fn location(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.location)
    }
    /// `Location(math_Vector&)`.
    pub fn location_into(&self, o: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        o.initialized(&self.location)
    }
    /// `Minimum`.
    pub fn minimum(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.minimum)
    }
    /// `NbIterations`.
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.iter)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_Powell resolution:");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Location Vector = {}\n", vector_dump(&self.location)));
            s.push_str(&format!(" Minimum value = {}\n", fmt(self.minimum)));
            s.push_str(&format!(" Number of iterations = {}\n", self.iter));
        } else {
            s.push_str(&format!(" Status = not Done because {}\n", self.status as i32));
        }
        s
    }
}

// ---------------------------------------------------------------------------------------------------------------
// math_FRPR

struct DefaultFrprTest;
impl FrprTest for DefaultFrprTest {
    fn is_solution_reached(&mut self, s: &FrprState<'_>, _: &mut dyn MultipleVarFunctionWithGradient) -> bool {
        s.is_solution_reached_default()
    }
}

/// `math_FRPR`.
#[derive(Debug, Clone)]
pub struct Frpr {
    location: Vector,
    gradient: Vector,
    minimum: f64,
    previous_minimum: f64,
    x_tol: f64,
    eps_z: f64,
    done: bool,
    iter: i32,
    state: i32,
    status: MathStatus,
    itermax: i32,
}

impl Frpr {
    /// `math_FRPR(F, Tolerance, NbIterations = 200, ZEPS = 1.0e-12)`.
    pub fn new(
        f: &dyn MultipleVarFunctionWithGradient,
        tolerance: f64,
        nb_iterations: i32,
        zeps: f64,
    ) -> Result<Self, MathError> {
        let n = f.nb_variables();
        Ok(Self {
            location: Vector::new(1, n)?,
            gradient: Vector::new(1, n)?,
            minimum: 0.0,
            previous_minimum: 0.0,
            x_tol: tolerance,
            eps_z: zeps,
            done: false,
            iter: 0,
            state: 0,
            status: MathStatus::NotBracketed,
            itermax: nb_iterations,
        })
    }

    /// `Perform(F, StartingPoint)`.
    pub fn perform(&mut self, f: &mut dyn MultipleVarFunctionWithGradient, start: &Vector) -> Result<(), MathError> {
        self.perform_with_test(f, start, &mut DefaultFrprTest)
    }

    /// `Perform` of a sub-class redefining `IsSolutionReached`.
    pub fn perform_with_test(
        &mut self,
        f: &mut dyn MultipleVarFunctionWithGradient,
        start: &Vector,
        test: &mut dyn FrprTest,
    ) -> Result<(), MathError> {
        let n = self.location.length();
        let mut g = Vector::new(1, n)?;
        let mut h = Vector::new(1, n)?;
        let mut f_dir = PlainLine::new(f, n)?;

        self.location.initialized(start)?;
        match f_dir.f.values(&self.location, &mut self.gradient) {
            Some(v) => self.previous_minimum = v,
            None => {
                self.done = false;
                self.status = MathStatus::FunctionError;
                return Ok(());
            }
        }

        g.initialized(&self.gradient.opposite())?;
        h.initialized(&g)?;
        self.gradient.initialized(&g)?;

        self.iter = 1;
        while self.iter <= self.itermax {
            let is_good = minimize_plain(&mut self.location, &mut self.gradient, &mut self.minimum, &mut f_dir)?;
            let state = FrprState {
                location: &self.location,
                gradient: &self.gradient,
                minimum: self.minimum,
                previous_minimum: self.previous_minimum,
                x_tol: self.x_tol,
                eps_z: self.eps_z,
            };
            if test.is_solution_reached(&state, &mut *f_dir.f) {
                self.done = true;
                self.state = f_dir.f.state_number();
                self.status = MathStatus::Ok;
                return Ok(());
            }
            if !is_good {
                self.done = false;
                self.status = MathStatus::DirectionSearchError;
                return Ok(());
            }
            match f_dir.f.values(&self.location, &mut self.gradient) {
                Some(v) => self.previous_minimum = v,
                None => {
                    self.done = false;
                    self.status = MathStatus::FunctionError;
                    return Ok(());
                }
            }

            let mut dgg = 0.0_f64;
            let mut gg = 0.0_f64;
            for j in 1..=n {
                gg += g.value(j)? * g.value(j)?;
                // for Polak-Ribiere
                dgg += (self.gradient.value(j)? + g.value(j)?) * self.gradient.value(j)?;
            }

            if gg == 0.0 {
                // unlikely: if the gradient is exactly 0 we are already done
                self.done = false;
                self.status = MathStatus::FunctionError;
                return Ok(());
            }

            let gam = dgg / gg;
            g.initialized(&self.gradient.opposite())?;
            let s = g.added(&h.multiplied(gam))?;
            self.gradient.initialized(&s)?;
            h.initialized(&self.gradient)?;
            self.iter += 1;
        }
        self.done = false;
        self.status = MathStatus::TooManyIterations;
        Ok(())
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    /// `Location()`.
    pub fn location(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.location)
    }
    /// `Location(math_Vector&)`.
    pub fn location_into(&self, o: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        o.initialized(&self.location)
    }
    /// `Gradient()`.
    pub fn gradient(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.gradient)
    }
    /// `Gradient(math_Vector&)`.
    pub fn gradient_into(&self, o: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        o.initialized(&self.gradient)
    }
    /// `Minimum`.
    pub fn minimum(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.minimum)
    }
    /// `NbIterations`.
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.iter)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_FRPR ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Location Vector = {}\n", vector_dump(&self.location)));
            s.push_str(&format!(" Minimum value = {}\n", fmt(self.minimum)));
            s.push_str(&format!(" Number of iterations = {}\n", self.iter));
        } else {
            s.push_str(&format!(" Status = not Done because {}\n", self.status as i32));
        }
        s
    }
}

// ---------------------------------------------------------------------------------------------------------------
// math_BFGS

/// `DirFunction` of `math_BFGS.cxx`: target function of the 1D problem, point and direction known.
struct GradLine<'a> {
    p0: Vector,
    dir: Vector,
    p: Vector,
    g: Vector,
    f: &'a mut dyn MultipleVarFunctionWithGradient,
}

impl<'a> GradLine<'a> {
    fn new(f: &'a mut dyn MultipleVarFunctionWithGradient, n: i32) -> Result<Self, MathError> {
        Ok(Self { p0: Vector::new(1, n)?, dir: Vector::new(1, n)?, p: Vector::new(1, n)?, g: Vector::new(1, n)?, f })
    }
    fn initialize(&mut self, p0: &Vector, dir: &Vector) -> Result<(), MathError> {
        self.p0.initialized(p0)?;
        self.dir.initialized(dir)
    }
    fn point(&mut self, x: f64) -> Result<(), MathError> {
        self.p.initialized(&self.dir)?;
        self.p.multiply(x);
        self.p.add(&self.p0)
    }
    fn eval_value(&mut self, x: f64) -> Result<Option<f64>, MathError> {
        self.point(x)?;
        Ok(self.f.value(&self.p))
    }
    fn eval_values(&mut self, x: f64) -> Result<Option<(f64, f64)>, MathError> {
        self.point(x)?;
        match self.f.values(&self.p, &mut self.g) {
            Some(v) => Ok(Some((v, self.g.dot(&self.dir)?))),
            None => Ok(None),
        }
    }
}

impl Function for GradLine<'_> {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.eval_value(x).ok().flatten()
    }
}

impl FunctionWithDerivative for GradLine<'_> {
    fn derivative(&mut self, x: f64) -> Option<f64> {
        self.eval_values(x).ok().flatten().map(|v| v.1)
    }
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        self.eval_values(x).ok().flatten()
    }
}

/// `ComputeInitScale`.
fn compute_init_scale(f0: f64, dir: &Vector, gr: &Vector, scale: &mut f64) -> Result<bool, MathError> {
    let dy1 = gr.dot(dir)?;
    if dy1.abs() < f64::MIN_POSITIVE {
        return Ok(false);
    }

    let a_hnr1 = dir.norm2();
    let alfa = 0.7 * (-f0) / dy1;
    *scale = 0.015 / a_hnr1.sqrt();
    if *scale > alfa {
        *scale = alfa;
    }
    Ok(true)
}

/// `ComputeMinMaxScale`: false if the point is out of bounds.
fn compute_min_max_scale(
    point: &Vector,
    dir: &Vector,
    left: &Vector,
    right: &Vector,
    min_scale: &mut f64,
    max_scale: &mut f64,
) -> Result<bool, MathError> {
    for idx in 1..=left.upper() {
        let a_left = left.value(idx)? - point.value(idx)?;
        let a_right = right.value(idx)? - point.value(idx)?;
        if dir.value(idx)?.abs() > f64::MIN_POSITIVE {
            // PConfusion gets off a little from the bounds to prevent a possible refusal in Value
            let a_l_scale = (a_left + P_CONFUSION) / dir.value(idx)?;
            let a_r_scale = (a_right - P_CONFUSION) / dir.value(idx)?;
            if a_left.abs() < P_CONFUSION {
                // the point is on the left border
                *max_scale = cmin(*max_scale, cmax(0.0, a_r_scale));
                *min_scale = cmax(*min_scale, cmin(0.0, a_r_scale));
            } else if a_right.abs() < P_CONFUSION {
                // the point is on the right border
                *max_scale = cmin(*max_scale, cmax(0.0, a_l_scale));
                *min_scale = cmax(*min_scale, cmin(0.0, a_l_scale));
            } else if a_left * a_right < 0.0 {
                // the point is inside the allowed range
                *max_scale = cmin(*max_scale, cmax(a_l_scale, a_r_scale));
                *min_scale = cmax(*min_scale, cmin(a_l_scale, a_r_scale));
            } else {
                // the point is out of bounds
                return Ok(false);
            }
        } else {
            // the direction is parallel to the border: check that the point is not out of bounds
            if a_left > P_CONFUSION || a_right < -P_CONFUSION {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// `MinimizeDirection` of `math_BFGS.cxx`.
#[allow(clippy::too_many_arguments)]
fn minimize_direction(
    p: &mut Vector,
    f0: f64,
    gr: &mut Vector,
    dir: &mut Vector,
    result: &mut f64,
    f: &mut GradLine<'_>,
    is_bounds: bool,
    left: &Vector,
    right: &Vector,
) -> Result<bool, MathError> {
    let mut lambda = 0.0_f64;
    if !compute_init_scale(f0, dir, gr, &mut lambda)? {
        return Ok(false);
    }

    // by default the scaling range is unlimited
    let mut a_min_lambda = -PRECISION_INFINITE;
    let mut a_max_lambda = PRECISION_INFINITE;
    if is_bounds {
        // limit the scaling range taking into account the bounds
        if !compute_min_max_scale(p, dir, left, right, &mut a_min_lambda, &mut a_max_lambda)? {
            return Ok(false);
        }

        if a_min_lambda > -P_CONFUSION && a_max_lambda < P_CONFUSION {
            // the point is on the border and the direction shows outside: go along the border
            for idx in 1..=left.upper() {
                if ((p.value(idx)? - right.value(idx)?).abs() < P_CONFUSION && dir.value(idx)? > 0.0)
                    || ((p.value(idx)? - left.value(idx)?).abs() < P_CONFUSION && dir.value(idx)? < 0.0)
                {
                    dir.set_value(idx, 0.0)?;
                }
            }

            // re-compute scale values with the new direction
            if !compute_init_scale(f0, dir, gr, &mut lambda)? {
                return Ok(false);
            }
            if !compute_min_max_scale(p, dir, left, right, &mut a_min_lambda, &mut a_max_lambda)? {
                return Ok(false);
            }
        }
        lambda = cmin(lambda, a_max_lambda);
        lambda = cmax(lambda, a_min_lambda);
    }

    f.initialize(p, dir)?;
    let Some(f1) = f.eval_value(lambda)? else {
        return Ok(false);
    };

    let mut bracket = BracketMinimum::new(0.0, lambda);
    if is_bounds {
        bracket.set_limits(a_min_lambda, a_max_lambda);
    }
    bracket.set_fa(f0);
    bracket.set_fb(f1);
    bracket.perform(f);
    if bracket.is_done() {
        // find the minimum inside the bracket
        let (ax, xx, bx) = bracket.values()?;
        let (_fax, fxx, _fbx) = bracket.function_values()?;

        let niter = 100;
        let tol = 1.0e-03;
        let mut sol = BrentMinimum::with_fbx(tol, fxx, niter, 1.0e-08);
        sol.perform(f, ax, xx, bx);
        if sol.is_done() {
            let scale = sol.location()?;
            *result = sol.minimum()?;
            dir.multiply(scale);
            p.add(dir)?;
            return Ok(true);
        }
    } else if is_bounds {
        // the bracket definition failed: with bounds, set the current point to the intersection with the bounds
        let Some(a_f_min) = f.eval_value(a_min_lambda)? else {
            return Ok(false);
        };
        let Some(a_f_max) = f.eval_value(a_max_lambda)? else {
            return Ok(false);
        };
        let a_best_lambda;
        if a_f_min < a_f_max {
            a_best_lambda = a_min_lambda;
            *result = a_f_min;
        } else {
            a_best_lambda = a_max_lambda;
            *result = a_f_max;
        }
        dir.multiply(a_best_lambda);
        p.add(dir)?;
        return Ok(true);
    }
    Ok(false)
}

struct DefaultBfgsTest;
impl BfgsTest for DefaultBfgsTest {
    fn is_solution_reached(&mut self, s: &BfgsState<'_>, _: &mut dyn MultipleVarFunctionWithGradient) -> bool {
        s.is_solution_reached_default()
    }
}

/// `math_BFGS`.
#[derive(Debug, Clone)]
pub struct Bfgs {
    status: MathStatus,
    location: Vector,
    gradient: Vector,
    previous_minimum: f64,
    minimum: f64,
    x_tol: f64,
    eps_z: f64,
    nbiter: i32,
    is_bounds_defined: bool,
    left: Vector,
    right: Vector,
    done: bool,
    itermax: i32,
}

impl Bfgs {
    /// `math_BFGS(NbVariables, Tolerance = 1.0e-8, NbIterations = 200, ZEPS = 1.0e-12)`.
    pub fn new(n: i32, tolerance: f64, nb_iterations: i32, zeps: f64) -> Result<Self, MathError> {
        Ok(Self {
            status: MathStatus::Ok,
            location: Vector::new(1, n)?,
            gradient: Vector::new(1, n)?,
            previous_minimum: 0.0,
            minimum: 0.0,
            x_tol: tolerance,
            eps_z: zeps,
            nbiter: 0,
            is_bounds_defined: false,
            left: Vector::filled(1, n, 0.0)?,
            right: Vector::filled(1, n, 0.0)?,
            done: false,
            itermax: nb_iterations,
        })
    }

    /// `SetBoundary`.
    pub fn set_boundary(&mut self, left: &Vector, right: &Vector) -> Result<(), MathError> {
        if left.length() != self.left.length() || right.length() != self.right.length() {
            return Err(MathError::Dimension);
        }
        self.left.initialized(left)?;
        self.right.initialized(right)?;
        self.is_bounds_defined = true;
        Ok(())
    }

    /// `Perform(F, StartingPoint)`.
    pub fn perform(&mut self, f: &mut dyn MultipleVarFunctionWithGradient, start: &Vector) -> Result<(), MathError> {
        self.perform_with_test(f, start, &mut DefaultBfgsTest)
    }

    /// `Perform` of a sub-class redefining `IsSolutionReached`.
    pub fn perform_with_test(
        &mut self,
        f: &mut dyn MultipleVarFunctionWithGradient,
        start: &Vector,
        test: &mut dyn BfgsTest,
    ) -> Result<(), MathError> {
        let n = self.location.length();
        let mut xi = Vector::new(1, n)?;
        let mut dg = Vector::new(1, n)?;
        let mut hdg = Vector::new(1, n)?;
        let mut hessin = Matrix::new(1, n, 1, n)?;
        hessin.init(0.0);

        let mut f_dir = GradLine::new(f, n)?;

        self.location.initialized(start)?;
        match f_dir.f.values(&self.location, &mut self.gradient) {
            Some(v) => self.previous_minimum = v,
            None => {
                self.done = false;
                self.status = MathStatus::FunctionError;
                return Ok(());
            }
        }
        for i in 1..=n {
            hessin.set_value(i, i, 1.0)?;
            xi.set_value(i, -self.gradient.value(i)?)?;
        }

        self.nbiter = 1;
        while self.nbiter <= self.itermax {
            self.minimum = self.previous_minimum;
            let f0 = self.minimum;
            let is_good = minimize_direction(
                &mut self.location,
                f0,
                &mut self.gradient,
                &mut xi,
                &mut self.minimum,
                &mut f_dir,
                self.is_bounds_defined,
                &self.left,
                &self.right,
            )?;

            let state = BfgsState {
                location: &self.location,
                gradient: &self.gradient,
                previous_minimum: self.previous_minimum,
                minimum: self.minimum,
                x_tol: self.x_tol,
                eps_z: self.eps_z,
                nb_iter: self.nbiter,
            };
            if test.is_solution_reached(&state, &mut *f_dir.f) {
                self.done = true;
                self.status = MathStatus::Ok;
                return Ok(());
            }

            if !is_good {
                self.done = false;
                self.status = MathStatus::DirectionSearchError;
                return Ok(());
            }
            self.previous_minimum = self.minimum;

            dg.initialized(&self.gradient)?;

            match f_dir.f.values(&self.location, &mut self.gradient) {
                Some(v) => self.minimum = v,
                None => {
                    self.done = false;
                    self.status = MathStatus::FunctionError;
                    return Ok(());
                }
            }

            for i in 1..=n {
                dg.set_value(i, self.gradient.value(i)? - dg.value(i)?)?;
            }

            for i in 1..=n {
                hdg.set_value(i, 0.0)?;
                for j in 1..=n {
                    hdg.set_value(i, hdg.value(i)? + hessin.value(i, j)? * dg.value(j)?)?;
                }
            }

            let mut fac = 0.0_f64;
            let mut fae = 0.0_f64;
            for i in 1..=n {
                fac += dg.value(i)? * xi.value(i)?;
                fae += dg.value(i)? * hdg.value(i)?;
            }
            fac = 1.0 / fac;
            let fad = 1.0 / fae;

            for i in 1..=n {
                dg.set_value(i, fac * xi.value(i)? - fad * hdg.value(i)?)?;
            }

            for i in 1..=n {
                for j in 1..=n {
                    let upd = fac * xi.value(i)? * xi.value(j)? - fad * hdg.value(i)? * hdg.value(j)?
                        + fae * dg.value(i)? * dg.value(j)?;
                    hessin.set_value(i, j, hessin.value(i, j)? + upd)?;
                }
            }

            for i in 1..=n {
                xi.set_value(i, 0.0)?;
                for j in 1..=n {
                    xi.set_value(i, xi.value(i)? - hessin.value(i, j)? * self.gradient.value(j)?)?;
                }
            }
            self.nbiter += 1;
        }
        self.done = false;
        self.status = MathStatus::TooManyIterations;
        Ok(())
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    /// `Location()`.
    pub fn location(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.location)
    }
    /// `Location(math_Vector&)`.
    pub fn location_into(&self, o: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        o.initialized(&self.location)
    }
    /// `Gradient()`.
    pub fn gradient(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.gradient)
    }
    /// `Gradient(math_Vector&)`.
    pub fn gradient_into(&self, o: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        o.initialized(&self.gradient)
    }
    /// `Minimum`.
    pub fn minimum(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.minimum)
    }
    /// `NbIterations`.
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.nbiter)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_BFGS resolution: ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Location Vector = {}\n", vector_dump(&self.location)));
            s.push_str(&format!(" Minimum value = {}\n", fmt(self.minimum)));
            s.push_str(&format!(" Number of iterations = {}\n", self.nbiter));
        } else {
            s.push_str(&format!(" Status = not Done because {}\n", self.status as i32));
        }
        s
    }
}
