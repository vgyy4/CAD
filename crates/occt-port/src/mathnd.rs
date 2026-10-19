//! OCCT multi-variable solvers and numerical integration.
use crate::math1d::{Function, FunctionWithDerivative, fmt_g};
use crate::mathlin::{IntegerVector, MathError, Matrix, Vector, gauss_points, gauss_points_max, gauss_weights};

pub trait MultipleVarFunction {
    fn nb_variables(&self) -> i32;
    fn value(&mut self, x: &Vector) -> Option<f64>;
    fn state_number(&mut self) -> i32 {
        0
    }
}
pub trait MultipleVarFunctionWithGradient: MultipleVarFunction {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool;
    fn values(&mut self, x: &Vector, g: &mut Vector) -> Option<f64>;
}
pub trait MultipleVarFunctionWithHessian: MultipleVarFunctionWithGradient {
    fn values_with_hessian(&mut self, x: &Vector, g: &mut Vector, h: &mut Matrix) -> Option<f64>;
}
pub trait FunctionSet {
    fn nb_variables(&self) -> i32;
    fn nb_equations(&self) -> i32;
    fn value(&mut self, x: &Vector, f: &mut Vector) -> bool;
    fn state_number(&mut self) -> i32 {
        0
    }
}
pub trait FunctionSetWithDerivatives: FunctionSet {
    fn derivatives(&mut self, x: &Vector, d: &mut Matrix) -> bool;
    fn values(&mut self, x: &Vector, f: &mut Vector, d: &mut Matrix) -> bool;
}

#[derive(Debug)]
pub struct FunctionSetRootState<'a> {
    pub delta: &'a Vector,
    pub sol: &'a Vector,
    pub df: &'a Matrix,
    pub tol: &'a Vector,
}
impl FunctionSetRootState<'_> {
    pub fn is_solution_reached_default(&self) -> bool {
        (1..=self.sol.length()).all(|i| match (self.delta.value(i), self.tol.value(i)) {
            (Ok(d), Ok(t)) => d.abs() <= t,
            _ => false,
        })
    }
}
pub trait FunctionSetRootTest {
    fn is_solution_reached(&mut self, state: &FunctionSetRootState<'_>, f: &mut dyn FunctionSetWithDerivatives)
    -> bool;
}

#[derive(Debug)]
pub struct NewtonFunctionSetRootState<'a> {
    pub tol_x: &'a Vector,
    pub tol_f: f64,
    pub sol: &'a Vector,
    pub delta_x: &'a Vector,
    pub f_values: &'a Vector,
    pub jacobian: &'a Matrix,
}
impl NewtonFunctionSetRootState<'_> {
    pub fn is_solution_reached_default(&self) -> bool {
        (1..=self.delta_x.length()).all(|i| {
            match (self.delta_x.value(i), self.tol_x.value(i), self.f_values.value(i)) {
                (Ok(d), Ok(t), Ok(v)) => d.abs() <= t && v.abs() <= self.tol_f,
                _ => false,
            }
        })
    }
}
pub trait NewtonFunctionSetRootTest {
    fn is_solution_reached(
        &mut self,
        state: &NewtonFunctionSetRootState<'_>,
        f: &mut dyn FunctionSetWithDerivatives,
    ) -> bool;
}

#[derive(Debug)]
pub struct NewtonMinimumState<'a> {
    pub step: &'a Vector,
    pub gradient: &'a Vector,
    pub hessian: &'a Matrix,
    pub previous_minimum: f64,
    pub minimum: f64,
    pub min_eigen_value: f64,
    pub x_tol: f64,
    pub c_tol: f64,
    pub nb_iter: i32,
    pub convex: bool,
}
impl NewtonMinimumState<'_> {
    pub fn is_converged_default(&self) -> bool {
        self.step.norm() <= self.x_tol
            || (self.minimum - self.previous_minimum).abs() <= self.x_tol * self.previous_minimum.abs()
    }
}
pub trait NewtonMinimumTest {
    fn is_converged(&mut self, state: &NewtonMinimumState<'_>) -> bool;
}

#[derive(Debug)]
pub struct BfgsState<'a> {
    pub location: &'a Vector,
    pub gradient: &'a Vector,
    pub previous_minimum: f64,
    pub minimum: f64,
    pub x_tol: f64,
    pub eps_z: f64,
    pub nb_iter: i32,
}
impl BfgsState<'_> {
    pub fn is_solution_reached_default(&self) -> bool {
        2.0 * (self.minimum - self.previous_minimum).abs()
            <= self.x_tol * (self.minimum.abs() + self.previous_minimum.abs() + self.eps_z)
    }
}
pub trait BfgsTest {
    fn is_solution_reached(&mut self, state: &BfgsState<'_>, f: &mut dyn MultipleVarFunctionWithGradient) -> bool;
}

#[derive(Debug)]
pub struct FrprState<'a> {
    pub location: &'a Vector,
    pub gradient: &'a Vector,
    pub minimum: f64,
    pub previous_minimum: f64,
    pub x_tol: f64,
    pub eps_z: f64,
}
impl FrprState<'_> {
    pub fn is_solution_reached_default(&self) -> bool {
        2.0 * (self.minimum - self.previous_minimum).abs()
            <= self.x_tol * (self.minimum.abs() + self.previous_minimum.abs() + self.eps_z)
    }
}
pub trait FrprTest {
    fn is_solution_reached(&mut self, state: &FrprState<'_>, f: &mut dyn MultipleVarFunctionWithGradient) -> bool;
}

#[derive(Debug)]
pub struct PowellState<'a> {
    pub location: &'a Vector,
    pub minimum: f64,
    pub previous_minimum: f64,
    pub location_error: f64,
    pub x_tol: f64,
    pub eps_z: f64,
}
impl PowellState<'_> {
    pub fn is_solution_reached_default(&self) -> bool {
        2.0 * (self.previous_minimum - self.minimum).abs()
            <= self.x_tol * (self.previous_minimum.abs() + self.minimum.abs() + self.eps_z)
    }
}
pub trait PowellTest {
    fn is_solution_reached(&mut self, state: &PowellState<'_>, f: &mut dyn MultipleVarFunction) -> bool;
}

struct DefaultNewtonSetTest;
impl NewtonFunctionSetRootTest for DefaultNewtonSetTest {
    fn is_solution_reached(
        &mut self,
        state: &NewtonFunctionSetRootState<'_>,
        _: &mut dyn FunctionSetWithDerivatives,
    ) -> bool {
        state.is_solution_reached_default()
    }
}

#[derive(Debug, Clone)]
pub struct NewtonFunctionSetRoot {
    tol_x: Vector,
    tol_f: f64,
    indx: IntegerVector,
    scratch: Vector,
    sol: Vector,
    delta_x: Vector,
    f_values: Vector,
    jacobian: Matrix,
    done: bool,
    state: i32,
    iter: i32,
    itermax: i32,
}
impl NewtonFunctionSetRoot {
    pub fn new(
        f: &dyn FunctionSetWithDerivatives,
        x_tolerance: &Vector,
        f_tolerance: f64,
        nb_iterations: i32,
    ) -> Result<Self, MathError> {
        let mut s = Self::without_tolerance(f, f_tolerance, nb_iterations)?;
        s.set_tolerance(x_tolerance)?;
        Ok(s)
    }
    pub fn without_tolerance(
        f: &dyn FunctionSetWithDerivatives,
        f_tolerance: f64,
        nb_iterations: i32,
    ) -> Result<Self, MathError> {
        let n = f.nb_variables();
        Ok(Self {
            tol_x: Vector::new(1, n)?,
            tol_f: f_tolerance,
            indx: IntegerVector::new(1, n)?,
            scratch: Vector::new(1, n)?,
            sol: Vector::new(1, n)?,
            delta_x: Vector::new(1, n)?,
            f_values: Vector::new(1, n)?,
            jacobian: Matrix::new(1, n, 1, n)?,
            done: false,
            state: 0,
            iter: 0,
            itermax: nb_iterations,
        })
    }
    pub fn set_tolerance(&mut self, t: &Vector) -> Result<(), MathError> {
        for i in 1..=self.tol_x.length() {
            self.tol_x.set_value(i, t.value(i)?)?;
        }
        Ok(())
    }
    pub fn perform(&mut self, f: &mut dyn FunctionSetWithDerivatives, start: &Vector) -> Result<(), MathError> {
        let lo = Vector::new(1, f.nb_variables())?;
        let hi = Vector::new(1, f.nb_variables())?;
        let mut lo = lo;
        let mut hi = hi;
        lo.init(-f64::MAX);
        hi.init(f64::MAX);
        self.perform_bounded(f, start, &lo, &hi)
    }
    pub fn perform_bounded(
        &mut self,
        f: &mut dyn FunctionSetWithDerivatives,
        start: &Vector,
        lo: &Vector,
        hi: &Vector,
    ) -> Result<(), MathError> {
        let mut t = DefaultNewtonSetTest;
        self.perform_bounded_with_test(f, start, lo, hi, &mut t)
    }
    pub fn perform_with_test(
        &mut self,
        f: &mut dyn FunctionSetWithDerivatives,
        start: &Vector,
        test: &mut dyn NewtonFunctionSetRootTest,
    ) -> Result<(), MathError> {
        let mut lo = Vector::new(1, f.nb_variables())?;
        let mut hi = Vector::new(1, f.nb_variables())?;
        lo.init(-f64::MAX);
        hi.init(f64::MAX);
        self.perform_bounded_with_test(f, start, &lo, &hi, test)
    }
    pub fn perform_bounded_with_test(
        &mut self,
        f: &mut dyn FunctionSetWithDerivatives,
        start: &Vector,
        lo: &Vector,
        hi: &Vector,
        test: &mut dyn NewtonFunctionSetRootTest,
    ) -> Result<(), MathError> {
        let n = f.nb_variables();
        if f.nb_equations() != n || start.length() != n || lo.length() != n || hi.length() != n {
            return Err(MathError::Dimension);
        }
        self.done = false;
        self.sol.initialized(start)?;
        if !f.values(&self.sol, &mut self.f_values, &mut self.jacobian) {
            return Ok(());
        }
        for it in 1..=self.itermax {
            self.iter = it;
            for k in 1..=self.delta_x.length() {
                self.delta_x.set_value(k, -self.f_values.value(k)?)?;
            }
            let (err, a, indx, _d, scratch) = crate::mathlin::recipes::lu_decompose_with_scratch(
                self.jacobian.clone(),
                self.indx.clone(),
                1.0,
                self.scratch.clone(),
                1.0e-30,
            )?;
            if err != 0 {
                return Ok(());
            }
            self.jacobian = a;
            self.indx = indx;
            self.scratch = scratch;
            self.delta_x = crate::mathlin::recipes::lu_solve(&self.jacobian, &self.indx, self.delta_x.clone())?;
            for i in 1..=self.sol.length() {
                self.sol.set_value(i, self.sol.value(i)? + self.delta_x.value(i)?)?;
                if self.sol.value(i)? <= lo.value(lo.lower() + i - 1)? {
                    self.sol.set_value(i, lo.value(lo.lower() + i - 1)?)?;
                }
                if self.sol.value(i)? >= hi.value(hi.lower() + i - 1)? {
                    self.sol.set_value(i, hi.value(hi.lower() + i - 1)?)?;
                }
            }
            if !f.values(&self.sol, &mut self.f_values, &mut self.jacobian) {
                return Ok(());
            }
            let st = NewtonFunctionSetRootState {
                tol_x: &self.tol_x,
                tol_f: self.tol_f,
                sol: &self.sol,
                delta_x: &self.delta_x,
                f_values: &self.f_values,
                jacobian: &self.jacobian,
            };
            if test.is_solution_reached(&st, f) {
                self.state = f.state_number();
                self.done = true;
                return Ok(());
            }
        }
        Ok(())
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    pub fn root(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.sol)
    }
    pub fn function_set_errors(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.delta_x)
    }
    pub fn derivative(&self) -> Result<&Matrix, MathError> {
        self.check()?;
        Ok(&self.jacobian)
    }
    pub fn root_into(&self, out: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        if out.length() != self.sol.length() {
            return Err(MathError::Dimension);
        };
        out.initialized(&self.sol)
    }
    pub fn function_set_errors_into(&self, out: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        if out.length() != self.delta_x.length() {
            return Err(MathError::Dimension);
        };
        out.initialized(&self.delta_x)
    }
    pub fn derivative_into(&self, out: &mut Matrix) -> Result<(), MathError> {
        self.check()?;
        if out.row_number() != self.jacobian.row_number() || out.col_number() != self.jacobian.col_number() {
            return Err(MathError::Dimension);
        };
        out.initialize_from(&self.jacobian)
    }
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.iter)
    }
    pub fn dump(&self) -> String {
        if self.done {
            format!(
                "math_NewtonFunctionSetRoot  Status = Done \n Vector solution = {}\n Value of the function at this solution = \n{}\n Number of iterations = {}\n",
                vector_dump(&self.sol),
                vector_dump(&self.f_values),
                self.iter
            )
        } else {
            "math_NewtonFunctionSetRoot Status = not Done \n".into()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Uzawa {
    resul: Vector,
    erruza: Vector,
    errinit: Vector,
    vardua: Vector,
    ctcinv: Matrix,
    nb_iter: i32,
    done: bool,
}
impl Uzawa {
    pub fn new(
        c: &Matrix,
        d: &Vector,
        x0: &Vector,
        eps_lix: f64,
        eps_lic: f64,
        nb_iterations: i32,
    ) -> Result<Self, MathError> {
        Self::with_constraints(c, d, x0, c.row_number(), 0, eps_lix, eps_lic, nb_iterations)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn with_constraints(
        c: &Matrix,
        d: &Vector,
        x0: &Vector,
        nce: i32,
        nci: i32,
        eps_lix: f64,
        eps_lic: f64,
        nb_iterations: i32,
    ) -> Result<Self, MathError> {
        let nr = c.row_number();
        let nc = c.col_number();
        if d.length() != nr || nce + nci != nr {
            return Err(MathError::Dimension);
        };
        if nr > nc {
            return Err(MathError::Range);
        }
        let mut s = Self {
            resul: Vector::new(1, nc)?,
            erruza: Vector::new(1, nc)?,
            errinit: Vector::new(1, nc)?,
            vardua: Vector::new(1, nr)?,
            ctcinv: Matrix::new(1, nr, 1, nr)?,
            nb_iter: 0,
            done: false,
        };
        for i in 1..=nr {
            let mut v = c.value(i, 1)? * x0.value(1)? - d.value(i)?;
            for j in 2..=nc {
                v += c.value(i, j)? * x0.value(j)?;
            }
            s.errinit.set_value(i, v)?;
        }
        if nci == 0 {
            s.nb_iter = 1;
            for i in 1..=nr {
                for j in 1..=i {
                    let mut v = c.value(i, 1)? * c.value(j, 1)?;
                    for k in 2..=nc {
                        v += c.value(i, k)? * c.value(j, k)?;
                    }
                    s.ctcinv.set_value(i, j, v)?;
                }
            }
            let cr = crate::mathlin::Crout::new(&s.ctcinv, 1.0e-20)?;
            if !cr.is_done() {
                return Err(MathError::Singular);
            }
            s.ctcinv = cr.inverse()?;
            for i in 1..=nr {
                let mut scale = s.ctcinv.value(i, 1)? * s.errinit.value(1)?;
                for j in 2..=i {
                    scale += s.ctcinv.value(i, j)? * s.errinit.value(j)?;
                }
                for j in i + 1..=nr {
                    scale += s.ctcinv.value(j, i)? * s.errinit.value(j)?;
                }
                s.vardua.set_value(i, scale)?;
            }
            for i in 1..=nc {
                let mut v = -c.value(1, i)? * s.vardua.value(1)?;
                for j in 2..=nr {
                    v -= c.value(j, i)? * s.vardua.value(j)?;
                }
                s.erruza.set_value(i, v)?;
            }
            s.resul.add_from(x0, &s.erruza)?;
            s.done = true;
            return Ok(s);
        }
        for i in 1..=nr {
            s.vardua.set_value(i, if i <= nce { 0.0 } else { 1.0 })?;
        }
        let mut normat = 0.0;
        for i in 1..=nr {
            let mut q = c.value(i, 1)? * c.value(i, 1)?;
            for j in 2..=nc {
                q += c.value(i, j)? * c.value(i, j)?;
            }
            normat += q;
        }
        let rho = (1.0 / libm::sqrt(2.0)) / normat;
        let mut xmax = 0.0;
        let mut err_max = 0.0;
        for it in 1..=nb_iterations {
            s.nb_iter = it;
            for i in 1..=nc {
                let xian = s.erruza.value(i)?;
                let mut v = -c.value(1, i)? * s.vardua.value(1)?;
                for j in 2..=nr {
                    v -= c.value(j, i)? * s.vardua.value(j)?;
                }
                s.erruza.set_value(i, v)?;
                if it > 1 {
                    if i == 1 {
                        xmax = (v - xian).abs();
                    }
                    xmax = if xmax < (v - xian).abs() { (v - xian).abs() } else { xmax };
                }
            }
            for i in 1..=nr {
                let mut err = c.value(i, 1)? * s.erruza.value(1)? + s.errinit.value(i)?;
                for j in 2..=nc {
                    err += c.value(i, j)? * s.erruza.value(j)?;
                }
                let err1 = if i <= nce {
                    s.vardua.set_value(i, s.vardua.value(i)? + rho * err)?;
                    (rho * err).abs()
                } else {
                    let old = s.vardua.value(i)?;
                    let z = old + rho * err;
                    s.vardua.set_value(i, if 0.0 < z { z } else { 0.0 })?;
                    (s.vardua.value(i)? - old).abs()
                };
                if i == 1 {
                    err_max = err1;
                }
                err_max = if err_max < err1 { err1 } else { err_max };
            }
            if it > 1 && xmax <= eps_lix {
                if err_max > eps_lic {
                    return Ok(s);
                }
                s.resul.add_from(x0, &s.erruza)?;
                s.done = true;
                return Ok(s);
            }
        }
        Ok(s)
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    pub fn value(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.resul)
    }
    pub fn initial_error(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.errinit)
    }
    pub fn error(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.erruza)
    }
    pub fn duale(&self) -> Vector {
        self.vardua.clone()
    }
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.nb_iter)
    }
    pub fn inverse_cont(&self) -> Result<&Matrix, MathError> {
        self.check()?;
        Ok(&self.ctcinv)
    }
    pub fn dump(&self) -> String {
        if self.done {
            format!(
                "math_Uzawa Status = Done \n Number of iterations = {}\n The solution vector is: {}\n",
                self.nb_iter,
                vector_dump(&self.resul)
            )
        } else {
            "math_Uzawa Status = not Done \n".into()
        }
    }
}

mod minimizers;
pub use minimizers::{Bfgs, Frpr, Powell};

struct DefaultNewtonMinimumTest;
impl NewtonMinimumTest for DefaultNewtonMinimumTest {
    fn is_converged(&mut self, s: &NewtonMinimumState<'_>) -> bool {
        s.is_converged_default()
    }
}
#[derive(Debug, Clone)]
pub struct NewtonMinimum {
    location: Vector,
    gradient: Vector,
    step: Vector,
    hessian: Matrix,
    previous_minimum: f64,
    minimum: f64,
    min_eigen_value: f64,
    x_tol: f64,
    c_tol: f64,
    nb_iter: i32,
    itermax: i32,
    with_singularity: bool,
    convex: bool,
    left: Vector,
    right: Vector,
    bounded: bool,
    done: bool,
    status: crate::mathlin::MathStatus,
}
impl NewtonMinimum {
    pub fn new(
        f: &dyn MultipleVarFunctionWithHessian,
        tolerance: f64,
        nb_iterations: i32,
        convexity: f64,
        with_singularity: bool,
    ) -> Result<Self, MathError> {
        let n = f.nb_variables();
        Ok(Self {
            location: Vector::new(1, n)?,
            gradient: Vector::new(1, n)?,
            step: Vector::filled(1, n, 10.0 * tolerance)?,
            hessian: Matrix::new(1, n, 1, n)?,
            previous_minimum: 0.0,
            minimum: 0.0,
            min_eigen_value: 0.0,
            x_tol: tolerance,
            c_tol: convexity,
            nb_iter: 0,
            itermax: nb_iterations,
            with_singularity,
            convex: true,
            left: Vector::new(1, n)?,
            right: Vector::new(1, n)?,
            bounded: false,
            done: false,
            status: crate::mathlin::MathStatus::NotBracketed,
        })
    }
    pub fn set_boundary(&mut self, l: &Vector, r: &Vector) -> Result<(), MathError> {
        if l.length() != self.location.length() || r.length() != self.location.length() {
            return Err(MathError::Dimension);
        }
        self.left.initialized(l)?;
        self.right.initialized(r)?;
        self.bounded = true;
        Ok(())
    }
    pub fn perform(&mut self, f: &mut dyn MultipleVarFunctionWithHessian, start: &Vector) -> Result<(), MathError> {
        let mut t = DefaultNewtonMinimumTest;
        self.perform_with_test(f, start, &mut t)
    }
    pub fn perform_with_test(
        &mut self,
        f: &mut dyn MultipleVarFunctionWithHessian,
        start: &Vector,
        test: &mut dyn NewtonMinimumTest,
    ) -> Result<(), MathError> {
        let n = self.location.length();
        if start.length() != n {
            return Err(MathError::Dimension);
        }
        let mut prev = Vector::new(1, n)?;
        prev.initialized(start)?;
        let mut next = Vector::new(1, n)?;
        let mut ok = true;
        let mut nb_conv = 0;
        self.done = true;
        self.status = crate::mathlin::MathStatus::Ok;
        self.nb_iter = 0;
        while ok && nb_conv < 2 {
            self.nb_iter += 1;
            let Some(v) = f.values_with_hessian(&prev, &mut self.gradient, &mut self.hessian) else {
                self.done = false;
                self.status = crate::mathlin::MathStatus::FunctionError;
                return Ok(());
            };
            let vprev = v;
            if self.nb_iter == 1 {
                self.previous_minimum = vprev;
                self.minimum = vprev;
            }
            let jac = crate::mathlin::Jacobi::new(&self.hessian)?;
            if !jac.is_done() {
                self.done = false;
                self.status = crate::mathlin::MathStatus::FunctionError;
                return Ok(());
            }
            let vals = jac.values()?;
            let mut min = vals.value(1)?;
            for i in 2..=vals.length() {
                let q = vals.value(i)?;
                if q < min {
                    min = q
                }
            }
            self.min_eigen_value = min;
            if min < self.c_tol {
                self.convex = false;
                if self.with_singularity && min.abs() > self.c_tol {
                    let delta = self.c_tol + 0.1 * min.abs() - min;
                    for i in 1..=n {
                        self.hessian.set_value(i, i, self.hessian.value(i, i)? + delta)?;
                    }
                } else {
                    self.done = false;
                    self.status = crate::mathlin::MathStatus::FunctionError;
                    return Ok(());
                }
            }
            let gauss = crate::mathlin::Gauss::new(&self.hessian, self.c_tol / 100.0)?;
            if !gauss.is_done() {
                self.done = false;
                self.status = crate::mathlin::MathStatus::DirectionSearchError;
                return Ok(());
            }
            self.step = gauss.solve(&self.gradient)?;
            if self.bounded {
                next.subtract_from(&prev, &self.step)?;
                let mut mult = f64::MAX;
                for i in 1..=n {
                    let absstep = self.step.value(i)?.abs();
                    if absstep < f64::MIN_POSITIVE {
                        continue;
                    }
                    if next.value(i)? < self.left.value(i)? {
                        let z = (prev.value(i)? - self.left.value(i)?).abs() / absstep;
                        if z < mult {
                            mult = z
                        }
                    }
                    if next.value(i)? > self.right.value(i)? {
                        let z = (prev.value(i)? - self.right.value(i)?).abs() / absstep;
                        if z < mult {
                            mult = z
                        }
                    }
                }
                #[allow(clippy::float_cmp)]
                let found = mult != f64::MAX;
                if found {
                    if mult > 1.0e-9 {
                        self.step.multiply(mult)
                    } else {
                        for i in 1..=n {
                            if (prev.value(i)? - self.right.value(i)?).abs() < 1.0e-9 && self.step.value(i)? < 0.0
                                || (prev.value(i)? - self.left.value(i)?).abs() < 1.0e-9 && self.step.value(i)? > 0.0
                            {
                                self.step.set_value(i, 0.0)?
                            }
                        }
                    }
                }
            }
            let mut viter;
            loop {
                next.subtract_from(&prev, &self.step)?;
                if let Some(v) = f.value(&next) {
                    viter = v;
                    break;
                }
                self.step.multiply(0.5)
            }
            self.minimum = viter;
            let st = NewtonMinimumState {
                step: &self.step,
                gradient: &self.gradient,
                hessian: &self.hessian,
                previous_minimum: self.previous_minimum,
                minimum: self.minimum,
                min_eigen_value: self.min_eigen_value,
                x_tol: self.x_tol,
                c_tol: self.c_tol,
                nb_iter: self.nb_iter,
                convex: self.convex,
            };
            if test.is_converged(&st) {
                nb_conv += 1
            } else {
                nb_conv = 0
            }
            let mut reductions = 0;
            self.minimum = self.previous_minimum;
            while viter > vprev && reductions < 10 {
                self.step.multiply(0.4);
                next.subtract_from(&prev, &self.step)?;
                if let Some(v) = f.value(&next) {
                    viter = v
                }
                reductions += 1;
            }
            if viter <= vprev {
                core::mem::swap(&mut prev, &mut next);
                self.previous_minimum = vprev;
                self.minimum = viter;
                ok = self.nb_iter < self.itermax;
                if !ok && nb_conv < 2 {
                    self.status = crate::mathlin::MathStatus::TooManyIterations
                }
            } else {
                ok = false;
                self.status = crate::mathlin::MathStatus::DirectionSearchError
            }
        }
        self.location.initialized(&prev)?;
        Ok(())
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    pub fn get_status(&self) -> crate::mathlin::MathStatus {
        self.status
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    pub fn location(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.location)
    }
    pub fn gradient(&self) -> Result<&Vector, MathError> {
        self.check()?;
        Ok(&self.gradient)
    }
    pub fn location_into(&self, o: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        if o.length() != self.location.length() {
            return Err(MathError::Dimension);
        }
        o.initialized(&self.location)
    }
    pub fn gradient_into(&self, o: &mut Vector) -> Result<(), MathError> {
        self.check()?;
        if o.length() != self.gradient.length() {
            return Err(MathError::Dimension);
        }
        o.initialized(&self.gradient)
    }
    pub fn minimum(&self) -> Result<f64, MathError> {
        self.check()?;
        Ok(self.minimum)
    }
    pub fn nb_iterations(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(self.nb_iter)
    }
    pub fn dump(&self) -> Result<String, MathError> {
        self.check()?;
        Ok(format!(
            "math_Newton Optimisation:  Done   ={}\n Status = {}\n Location Vector = {}\n Minimum value = {}\n Previous value = {}\n Number of iterations = {}\n Convexity = {}\n Eigen Value = {}\n",
            u8::from(self.done),
            self.status as i32,
            vector_dump(&self.location),
            fmt(self.minimum),
            fmt(self.previous_minimum),
            self.nb_iter,
            u8::from(self.convex),
            fmt(self.min_eigen_value)
        ))
    }
}

mod function_set_root;
pub use function_set_root::FunctionSetRoot;

fn fmt(x: f64) -> String {
    fmt_g(x)
}
fn vector_dump(v: &Vector) -> String {
    let mut s = format!("math_Vector of Length = {}\n", v.length());
    for i in v.lower()..=v.upper() {
        if let Ok(x) = v.value(i) {
            s.push_str(&format!("math_Vector({i}) = {}\n", fmt(x)));
        }
    }
    s
}

#[derive(Debug, Clone)]
pub struct GaussSingleIntegration {
    val: f64,
    done: bool,
}
impl Default for GaussSingleIntegration {
    fn default() -> Self {
        Self { val: 0.0, done: false }
    }
}
impl GaussSingleIntegration {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_order(f: &mut dyn Function, lower: f64, upper: f64, order: i32) -> Self {
        let mut s = Self::new();
        let order = gauss_points_max().min(order);
        s.perform(f, lower, upper, order);
        s
    }
    pub fn with_tolerance(f: &mut dyn Function, lower: f64, upper: f64, order: i32, tol: f64) -> Self {
        let mut s = Self::new();
        let order = gauss_points_max().min(order);
        s.perform(f, lower, upper, order);
        let mut len = s.val;
        let mut n_iter = 1;
        let mut intervals = 1;
        loop {
            let old = len;
            len = 0.0;
            intervals *= 2;
            let du = (upper - lower) / f64::from(intervals);
            for i in 1..=intervals {
                s.perform(f, lower + f64::from(i - 1) * du, lower + f64::from(i) * du, order);
                if !s.done {
                    return s;
                }
                len += s.val;
            }
            n_iter += 1;
            if !((old - len).abs() > tol && n_iter <= 13) {
                break;
            }
        }
        s.val = len;
        s
    }
    pub fn perform(&mut self, f: &mut dyn Function, lower: f64, upper: f64, order: i32) {
        self.done = false;
        // an empty `math_Vector(1, Order)` for `Order < 1`: no point is read (placeholders here)
        let (p, w) = if order < 1 {
            (Vector::new(1, 1), Vector::new(1, 1))
        } else {
            (gauss_points(order, 1), gauss_weights(order, 1))
        };
        let (Ok(p), Ok(w)) = (p, w) else { return };
        let xm = 0.5 * (upper + lower);
        let xr = 0.5 * (upper - lower);
        self.val = 0.0;
        let ind = order / 2;
        let ind1 = (order + 1) / 2;
        if ind1 > ind {
            // the C++ `F.Value(xm, Val)` leaves `Val` as it is when it fails
            let Some(v) = f.value(xm) else { return };
            let Ok(ww) = w.value(ind1) else { return };
            self.val = v * ww;
        }
        for j in 1..=ind {
            let (Ok(pp), Ok(ww)) = (p.value(j), w.value(j)) else { return };
            let dx = xr * pp;
            let Some(f1) = f.value(xm - dx) else { return };
            let Some(f2) = f.value(xm + dx) else { return };
            let ft = f1 + f2;
            self.val += ww * ft;
        }
        self.val *= xr;
        self.done = true;
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    pub fn value(&self) -> Result<f64, MathError> {
        if self.done { Ok(self.val) } else { Err(MathError::NotDone) }
    }
    pub fn dump(&self) -> String {
        if self.done {
            format!("math_GaussSingleIntegration  Status = Done \nIntegration Value = {}\n", fmt(self.val))
        } else {
            "math_GaussSingleIntegration Status = not Done \n".into()
        }
    }
}

#[derive(Debug, Clone)]
pub struct GaussSetIntegration {
    val: Vector,
    done: bool,
}
impl GaussSetIntegration {
    pub fn new(
        f: &mut dyn FunctionSet,
        lower: &Vector,
        upper: &Vector,
        order: &IntegerVector,
    ) -> Result<Self, MathError> {
        let ne = f.nb_equations();
        let nv = f.nb_variables();
        let mut s = Self { val: Vector::new(1, ne)?, done: false };
        if nv != 1 {
            return Err(MathError::Dimension);
        }
        let ord = order.value(order.lower())?;
        if ord > gauss_points_max() {
            return Err(MathError::Range);
        }
        let lo = lower.value(lower.lower())?;
        let hi = upper.value(upper.lower())?;
        let xm = 0.5 * (lo + hi);
        let xr = 0.5 * (hi - lo);
        // an empty `math_Vector(1, Ordre)` for `Ordre < 1`: no point is read (placeholders here)
        let (gp, gw) = if ord < 1 {
            (Vector::new(1, 1)?, Vector::new(1, 1)?)
        } else {
            (gauss_points(ord, 1)?, gauss_weights(ord, 1)?)
        };
        let mut x = Vector::new(1, 1)?;
        let mut a = Vector::new(1, ne)?;
        let mut b = Vector::new(1, ne)?;
        let ind = ord / 2;
        let ind1 = (ord + 1) / 2;
        if ind1 > ind {
            x.set_value(1, xm)?;
            if !f.value(&x, &mut s.val) {
                return Ok(s);
            }
            s.val.multiply(gw.value(ind1)?);
        } else {
            s.val.init(0.0)
        }
        for i in 1..=ind {
            x.set_value(1, xm + xr * gp.value(i)?)?;
            if !f.value(&x, &mut a) {
                return Ok(s);
            }
            x.set_value(1, xm - xr * gp.value(i)?)?;
            if !f.value(&x, &mut b) {
                return Ok(s);
            }
            a.add(&b)?;
            a.multiply(gw.value(i)?);
            s.val.add(&a)?;
        }
        s.val.multiply(xr);
        s.done = true;
        Ok(s)
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    pub fn value(&self) -> Result<&Vector, MathError> {
        if self.done { Ok(&self.val) } else { Err(MathError::NotDone) }
    }
    pub fn dump(&self) -> String {
        if self.done {
            format!("math_GaussSetIntegration  Status = Done \nIntegration Value = {}\n", vector_dump(&self.val))
        } else {
            "math_GaussSetIntegration Status = not Done \n".into()
        }
    }
}

#[derive(Debug, Clone)]
pub struct GaussMultipleIntegration {
    val: f64,
    done: bool,
}
impl GaussMultipleIntegration {
    pub fn new(f: &mut dyn MultipleVarFunction, lower: &Vector, upper: &Vector, order: &IntegerVector) -> Self {
        let n = f.nb_variables();
        let mut s = Self { val: 0.0, done: false };
        if n <= 0 {
            return s;
        }
        let (Ok(mut lo), Ok(mut hi), Ok(mut ord)) = (Vector::new(1, n), Vector::new(1, n), IntegerVector::new(1, n))
        else {
            return s;
        };
        let mut max = 0;
        for i in 1..=n {
            let (Ok(l), Ok(h), Ok(o)) = (
                lower.value(lower.lower() + i - 1),
                upper.value(upper.lower() + i - 1),
                order.value(order.lower() + i - 1),
            ) else {
                return s;
            };
            let o = gauss_points_max().min(o);
            if lo.set_value(i, l).is_err() || hi.set_value(i, h).is_err() || ord.set_value(i, o).is_err() {
                return s;
            }
            if o >= max {
                max = o
            }
        }
        for i in 1..=n {
            if let Ok(o) = ord.value(i) {
                if o <= 0 {
                    return s;
                }
            } else {
                return s;
            }
        }
        let mut xm = Vec::new();
        let mut xr = Vec::new();
        for i in 1..=n {
            let (Ok(l), Ok(h)) = (lo.value(i), hi.value(i)) else { return s };
            xm.push(0.5 * (l + h));
            xr.push(0.5 * (h - l));
        }
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for i in 1..=n {
            let Ok(o) = ord.value(i) else { return s };
            let (Ok(p), Ok(w)) = (gauss_points(o, 1), gauss_weights(o, 1)) else { return s };
            points.push(p);
            weights.push(w);
        }
        let Ok(mut x) = Vector::new(1, n) else { return s };
        // Recursive loop implementation below stores the selected weight indices directly.
        #[allow(clippy::too_many_arguments)]
        fn run(
            f: &mut dyn MultipleVarFunction,
            lev: i32,
            n: i32,
            x: &mut Vector,
            idx: &mut [i32],
            ord: &IntegerVector,
            xm: &[f64],
            xr: &[f64],
            p: &[Vector],
            w: &[Vector],
            sum: &mut f64,
        ) -> bool {
            if lev > n {
                let Some(v) = f.value(x) else { return false };
                let mut wt = 1.0;
                for j in 1..=n {
                    let (Some(wj), Some(idxj)) = (w.get((j - 1) as usize), idx.get((j - 1) as usize)) else {
                        return false;
                    };
                    let Ok(wv) = wj.value(*idxj) else { return false };
                    wt *= wv;
                }
                *sum += wt * v;
                return true;
            }
            let Ok(o) = ord.value(lev) else { return false };
            let mut ok = false;
            for k in 1..=o {
                let Some(pj) = p.get((lev - 1) as usize) else { return false };
                let Ok(node) = pj.value(k) else {
                    return false;
                };
                let (Some(mid), Some(radius)) = (xm.get((lev - 1) as usize), xr.get((lev - 1) as usize)) else {
                    return false;
                };
                if x.set_value(lev, *mid + *radius * node).is_err() {
                    return false;
                }
                let Some(slot) = idx.get_mut((lev - 1) as usize) else { return false };
                *slot = k;
                ok = run(f, lev + 1, n, x, idx, ord, xm, xr, p, w, sum);
            }
            ok
        }
        let mut idx = vec![1; n as usize];
        if !run(f, 1, n, &mut x, &mut idx, &ord, &xm, &xr, &points, &weights, &mut s.val) {
            return s;
        }
        for i in 1..=n {
            let (Ok(a), Ok(b)) = (lo.value(i), hi.value(i)) else { return s };
            s.val *= 0.5 * (b - a);
        }
        s.done = true;
        s
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    pub fn value(&self) -> Result<f64, MathError> {
        if self.done { Ok(self.val) } else { Err(MathError::NotDone) }
    }
    pub fn dump(&self) -> String {
        if self.done {
            format!("math_GaussMultipleIntegration  Status = Done \n Integration value = {}\n", fmt(self.val))
        } else {
            "math_GaussMultipleIntegration Status = not Done \n".into()
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BissecNewtonState {
    pub x_tol: f64,
    pub x: f64,
    pub dx: f64,
    pub f: f64,
    pub df: f64,
}
impl BissecNewtonState {
    pub fn is_solution_reached_default(&self) -> bool {
        self.dx.abs() <= self.x_tol
    }
}
pub trait BissecNewtonTest {
    fn is_solution_reached(&mut self, state: &BissecNewtonState, f: &mut dyn FunctionWithDerivative) -> bool;
}
struct DefaultBissecTest;
impl BissecNewtonTest for DefaultBissecTest {
    fn is_solution_reached(&mut self, s: &BissecNewtonState, _: &mut dyn FunctionWithDerivative) -> bool {
        s.is_solution_reached_default()
    }
}
#[derive(Debug, Clone)]
pub struct BissecNewton {
    x_tol: f64,
    x: f64,
    dx: f64,
    f: f64,
    df: f64,
    done: bool,
    status: crate::mathlin::MathStatus,
}
#[allow(clippy::float_cmp)]
impl BissecNewton {
    pub fn new(x_tolerance: f64) -> Self {
        Self {
            x_tol: x_tolerance,
            x: 0.0,
            dx: 0.0,
            f: 0.0,
            df: 0.0,
            done: false,
            status: crate::mathlin::MathStatus::NotBracketed,
        }
    }
    pub fn perform(&mut self, f: &mut dyn FunctionWithDerivative, bound1: f64, bound2: f64, nb_iterations: i32) {
        let mut t = DefaultBissecTest;
        self.perform_with_test(f, bound1, bound2, nb_iterations, &mut t)
    }
    pub fn perform_with_test(
        &mut self,
        fun: &mut dyn FunctionWithDerivative,
        bound1: f64,
        bound2: f64,
        nb_iterations: i32,
        test: &mut dyn BissecNewtonTest,
    ) {
        let Some((mut fl, df)) = fun.values(bound1) else {
            self.done = false;
            self.status = crate::mathlin::MathStatus::FunctionError;
            return;
        };
        self.df = df;
        let Some((fh, _)) = fun.values(bound2) else {
            self.done = false;
            self.status = crate::mathlin::MathStatus::FunctionError;
            return;
        };
        let ftol = f64::EPSILON;
        if fl * fh > ftol * ftol {
            self.done = false;
            self.status = crate::mathlin::MathStatus::NotBracketed;
            return;
        }
        let (mut xl, mut xh) =
            if fl < -ftol || (fl < ftol && fh < -ftol) { (bound1, bound2) } else { (bound2, bound1) };
        self.x = 0.5 * (bound1 + bound2);
        let mut dxold = (bound2 - bound1).abs();
        self.dx = dxold;
        let Some((fv, dfv)) = fun.values(self.x) else {
            self.done = false;
            self.status = crate::mathlin::MathStatus::FunctionError;
            return;
        };
        self.f = fv;
        self.df = dfv;
        for _ in 1..=nb_iterations {
            if (((self.x - xh) * self.df - self.f) * ((self.x - xl) * self.df - self.f) >= 0.0)
                || ((2.0 * self.f).abs() > (dxold * self.df).abs())
            {
                dxold = self.dx;
                self.dx = 0.5 * (xh - xl);
                self.x = xl + self.dx;
                if self.dx.abs() < self.x_tol {
                    self.status = crate::mathlin::MathStatus::Ok;
                    self.done = true;
                    return;
                }
            } else {
                dxold = self.dx;
                self.dx = self.f / self.df;
                let temp = self.x;
                self.x -= self.dx;
                if temp == self.x {
                    self.status = crate::mathlin::MathStatus::Ok;
                    self.done = true;
                    return;
                }
            }
            let st = BissecNewtonState { x_tol: self.x_tol, x: self.x, dx: self.dx, f: self.f, df: self.df };
            if test.is_solution_reached(&st, fun) {
                self.status = crate::mathlin::MathStatus::Ok;
                self.done = true;
                return;
            }
            let Some((v, d)) = fun.values(self.x) else {
                self.done = false;
                self.status = crate::mathlin::MathStatus::FunctionError;
                return;
            };
            self.f = v;
            self.df = d;
            if self.f < 0.0 {
                xl = self.x;
                fl = self.f
            } else if self.f > 0.0 {
                xh = self.x
            } else {
                self.status = crate::mathlin::MathStatus::Ok;
                self.done = true;
                return;
            }
        }
        let _ = fl;
        self.status = crate::mathlin::MathStatus::TooManyIterations;
        self.done = false;
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    pub fn get_status(&self) -> crate::mathlin::MathStatus {
        self.status
    }
    pub fn root(&self) -> Result<f64, MathError> {
        if self.done { Ok(self.x) } else { Err(MathError::NotDone) }
    }
    pub fn derivative(&self) -> Result<f64, MathError> {
        if self.done { Ok(self.df) } else { Err(MathError::NotDone) }
    }
    pub fn value(&self) -> Result<f64, MathError> {
        if self.done { Ok(self.f) } else { Err(MathError::NotDone) }
    }
    pub fn dump(&self) -> String {
        if self.done {
            format!(
                "math_BissecNewton  Status = Done \n The Root  is: {}\n The value at this Root is: {}\n",
                fmt(self.x),
                fmt(self.f)
            )
        } else {
            "math_BissecNewton  Status = not Done \n".into()
        }
    }
}

#[derive(Debug, Clone)]
pub struct KronrodSingleIntegration {
    done: bool,
    value: f64,
    error_reached: f64,
    absolute_error: f64,
    order_reached: i32,
    iterations: i32,
}
impl Default for KronrodSingleIntegration {
    fn default() -> Self {
        Self { done: false, value: 0.0, error_reached: 0.0, absolute_error: 0.0, order_reached: 0, iterations: 0 }
    }
}
impl KronrodSingleIntegration {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_points(f: &mut dyn Function, lower: f64, upper: f64, n: i32) -> Self {
        let mut s = Self::new();
        s.perform(f, lower, upper, n);
        s
    }
    pub fn with_tolerance(f: &mut dyn Function, lower: f64, upper: f64, n: i32, tol: f64, max_iter: i32) -> Self {
        let mut s = Self::new();
        s.perform_with_tolerance(f, lower, upper, n, tol, max_iter);
        s
    }
    pub fn gk_rule(
        f: &mut dyn Function,
        lower: f64,
        upper: f64,
        _gp: &Vector,
        gw: &Vector,
        kp: &Vector,
        kw: &Vector,
    ) -> Option<(f64, f64)> {
        let n = kp.length();
        let half = (n + 1) / 2;
        let mut gauss = 0.0;
        let mut value = 0.0;
        let mut f1 = Vector::new(1, half - 1).ok()?;
        let mut f2 = Vector::new(1, half - 1).ok()?;
        let xm = 0.5 * (upper + lower);
        let xr = 0.5 * (upper - lower);
        let mut i = 2;
        while i < half {
            let dx = xr * kp.value(i).ok()?;
            let v1 = f.value(xm + dx)?;
            let v2 = f.value(xm - dx)?;
            f1.set_value(i, v1).ok()?;
            f2.set_value(i, v2).ok()?;
            gauss += (v1 + v2) * gw.value(i / 2).ok()?;
            value += (v1 + v2) * kw.value(i).ok()?;
            i += 2;
        }
        let fc = f.value(xm)?;
        value += fc * kw.value(half).ok()?;
        if i == half {
            gauss += fc * gw.value(half / 2).ok()?;
        }
        i = 1;
        while i < half {
            let dx = xr * kp.value(i).ok()?;
            let v1 = f.value(xm + dx)?;
            let v2 = f.value(xm - dx)?;
            f1.set_value(i, v1).ok()?;
            f2.set_value(i, v2).ok()?;
            value += (v1 + v2) * kw.value(i).ok()?;
            i += 2;
        }
        let mean = 0.5 * value;
        let mut asc = (fc - mean).abs() * kw.value(half).ok()?;
        i = 1;
        while i < half {
            asc += kw.value(i).ok()? * (f1.value(i).ok()? - mean).abs()
                + kw.value(i).ok()? * (f2.value(i).ok()? - mean).abs();
            i += 1;
        }
        asc *= xr;
        value *= xr;
        gauss *= xr;
        let mut err = (value - gauss).abs();
        let mut scale = 1.0;
        if asc != 0.0 && err != 0.0 {
            scale = libm::pow(200.0 * err / asc, 1.5);
        }
        if scale < 1.0 {
            err = if err < asc * scale { err } else { asc * scale };
        }
        Some((value, err))
    }
    pub fn perform(&mut self, f: &mut dyn Function, lower: f64, upper: f64, n: i32) {
        self.iterations = 0;
        if n < 3 || upper - lower < 1.0e-9 {
            self.done = false;
            return;
        }
        self.order_reached = if n % 2 == 0 { n + 1 } else { n };
        let ng = self.order_reached / 2;
        let Some((kp, kw)) = crate::mathlin::kronrod_points_and_weights(self.order_reached, 1) else {
            self.done = false;
            return;
        };
        let Ok(Some((gp, gw))) = crate::mathlin::ordered_gauss_points_and_weights(ng, 1) else {
            self.done = false;
            return;
        };
        let Some((v, e)) = Self::gk_rule(f, lower, upper, &gp, &gw, &kp, &kw) else {
            self.done = false;
            return;
        };
        self.value = v;
        self.error_reached = e;
        self.absolute_error = e;
        self.done = true;
        self.iterations = 1;
    }
    pub fn perform_with_tolerance(
        &mut self,
        f: &mut dyn Function,
        lower: f64,
        upper: f64,
        n: i32,
        tol: f64,
        max_iter: i32,
    ) {
        self.iterations = 0;
        if n < 3 || tol <= 0.0 {
            self.done = false;
            return;
        }
        self.order_reached = if n % 2 == 0 { n + 1 } else { n };
        let ng = self.order_reached / 2;
        let Some((kp, kw)) = crate::mathlin::kronrod_points_and_weights(self.order_reached, 1) else {
            self.done = false;
            return;
        };
        let Ok(Some((gp, gw))) = crate::mathlin::ordered_gauss_points_and_weights(ng, 1) else {
            self.done = false;
            return;
        };
        let Some((value, error)) = Self::gk_rule(f, lower, upper, &gp, &gw, &kp, &kw) else {
            self.done = false;
            return;
        };
        self.value = value;
        self.error_reached = error;
        self.absolute_error = error;
        self.done = true;
        self.iterations += 1;
        let min_vol = libm::nextafter(1.0, f64::MAX) - 1.0;
        if self.value.abs() > min_vol {
            self.error_reached /= self.value.abs();
        }
        if self.error_reached <= tol || self.iterations >= max_iter {
            return;
        }
        #[derive(Clone, Copy)]
        struct Interval {
            a: f64,
            b: f64,
            err: f64,
            val: f64,
        }
        let mut intervals = vec![Interval { a: lower, b: upper, err: self.absolute_error, val: self.value }];
        let mut count = 0;
        while self.error_reached > tol && self.iterations < max_iter {
            let mut best = 0usize;
            let mut maxerr = 0.0;
            for (i, it) in intervals.iter().enumerate() {
                if it.err > maxerr {
                    maxerr = it.err;
                    best = i;
                }
            }
            let Some(old) = intervals.get(best).copied() else {
                self.done = false;
                return;
            };
            let c = 0.5 * (old.a + old.b);
            let Some((v1, e1)) = Self::gk_rule(f, old.a, c, &gp, &gw, &kp, &kw) else {
                self.done = false;
                return;
            };
            let Some((v2, e2)) = Self::gk_rule(f, c, old.b, &gp, &gw, &kp, &kw) else {
                self.done = false;
                return;
            };
            self.iterations += 1;
            let deltav = v1 + v2 - old.val;
            self.value += deltav;
            if deltav.abs() <= libm::nextafter(self.value.abs(), f64::MAX) - self.value.abs() {
                count += 1;
            }
            let deltae = e1 + e2 - old.err;
            self.absolute_error += deltae;
            if self.absolute_error <= libm::nextafter(self.value.abs(), f64::MAX) - self.value.abs() {
                count += 1;
            }
            self.error_reached =
                if self.value.abs() > min_vol { self.absolute_error / self.value.abs() } else { self.absolute_error };
            if count > 50 {
                return;
            }
            if let Some(slot) = intervals.get_mut(best) {
                *slot = Interval { a: old.a, b: c, err: e1, val: v1 };
            } else {
                self.done = false;
                return;
            }
            intervals.insert(best + 1, Interval { a: c, b: old.b, err: e2, val: v2 });
        }
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn checked(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    pub fn value(&self) -> Result<f64, MathError> {
        self.checked()?;
        Ok(self.value)
    }
    pub fn error_reached(&self) -> Result<f64, MathError> {
        self.checked()?;
        Ok(self.error_reached)
    }
    pub fn absolut_error(&self) -> Result<f64, MathError> {
        self.checked()?;
        Ok(self.absolute_error)
    }
    pub fn order_reached(&self) -> Result<i32, MathError> {
        self.checked()?;
        Ok(self.order_reached)
    }
    pub fn nb_iter_reached(&self) -> Result<i32, MathError> {
        self.checked()?;
        Ok(self.iterations)
    }
    pub fn dump(&self) -> String {
        if self.done {
            format!(
                "math_KronrodSingleIntegration  Status = Done \nIntegration Value = {}\nError Reached = {}\nAbsolute Error = {}\n",
                fmt(self.value),
                fmt(self.error_reached),
                fmt(self.absolute_error)
            )
        } else {
            "math_KronrodSingleIntegration  Status = not Done \n".into()
        }
    }
}
