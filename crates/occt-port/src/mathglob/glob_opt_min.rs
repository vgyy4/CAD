//! `math_GlobOptMin`: global minimum search by a Lipschitz-based scan with local refinement.
use crate::mathlin::{MathError, Matrix, Vector};
use crate::mathnd::{
    Bfgs, MultipleVarFunction, MultipleVarFunctionWithGradient, MultipleVarFunctionWithHessian, NewtonMinimum, Powell,
};

/// `Precision::Infinite()`.
const INFINITE: f64 = 2.0e100;
/// `Precision::PConfusion()`.
const PCONFUSION: f64 = 1.0e-7 * 0.01;
/// `Precision::Confusion()`.
const CONFUSION: f64 = 1.0e-7;

/// What the function can do: the answer of the `dynamic_cast`s of `math_GlobOptMin`.
pub enum GlobFunction<'a> {
    /// only a `math_MultipleVarFunction` (Powell only)
    Value(&'a mut dyn MultipleVarFunction),
    /// a `math_MultipleVarFunctionWithGradient` (BFGS, Powell)
    Gradient(&'a mut dyn MultipleVarFunctionWithGradient),
    /// a `math_MultipleVarFunctionWithHessian` (Newton, BFGS, Powell)
    Hessian(&'a mut dyn MultipleVarFunctionWithHessian),
}

impl core::fmt::Debug for GlobFunction<'_> {
    fn fmt(&self, fm: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let level = match self {
            Self::Value(_) => "Value",
            Self::Gradient(_) => "Gradient",
            Self::Hessian(_) => "Hessian",
        };
        fm.debug_tuple("GlobFunction").field(&level).finish()
    }
}

impl GlobFunction<'_> {
    /// `NbVariables` of the function.
    pub fn nb_variables(&self) -> i32 {
        match self {
            Self::Value(f) => f.nb_variables(),
            Self::Gradient(f) => f.nb_variables(),
            Self::Hessian(f) => f.nb_variables(),
        }
    }

    fn value(&mut self, x: &Vector) -> Option<f64> {
        match self {
            Self::Value(f) => f.value(x),
            Self::Gradient(f) => f.value(x),
            Self::Hessian(f) => f.value(x),
        }
    }

    fn as_value(&mut self) -> &mut dyn MultipleVarFunction {
        match self {
            Self::Value(f) => &mut **f,
            Self::Gradient(f) => &mut **f,
            Self::Hessian(f) => &mut **f,
        }
    }
}

/// `Vector(i)` read; the indices used here are always inside the vectors.
fn at(v: &Vector, i: i32) -> f64 {
    v.value(i).unwrap_or(0.0)
}

fn put(v: &mut Vector, i: i32, x: f64) {
    let _ = v.set_value(i, x);
}

/// `std::min(a, b)`.
fn min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`.
fn max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `Precision::IsInfinite`.
fn is_infinite(r: f64) -> bool {
    r.abs() >= INFINITE
}

/// The vector covers the absolute indices `1..=n`.
fn covers(v: &Vector, n: i32) -> bool {
    v.lower() <= 1 && v.upper() >= n
}

fn distance_to_border(x: &Vector, mn: &Vector, mx: &Vector) -> f64 {
    let mut dist = f64::MAX;
    for i in mn.lower()..=mn.upper() {
        let d1 = (at(x, i) - at(mn, i)).abs();
        let d2 = (at(x, i) - at(mx, i)).abs();
        dist = min(dist, min(d1, d2));
    }
    dist
}

/// `math_GlobOptMin`.
#[derive(Clone, Debug)]
pub struct GlobOptMin {
    n: i32,
    a: Vector,
    b: Vector,
    glob_a: Vector,
    glob_b: Vector,
    tol: f64,
    same_tol: f64,
    c: f64,
    init_c: f64,
    is_find_single_solution: bool,
    functional_minimal_value: f64,
    is_const_locked: bool,
    done: bool,
    y: Vec<f64>,
    sol_count: i32,
    z: f64,
    e1: f64,
    e2: f64,
    e3: f64,
    x: Vector,
    tmp: Vector,
    v: Vector,
    max_v: Vector,
    last_step: f64,
    min_cell_filter_sol: i32,
    cont: i32,
    f: f64,
}

impl GlobOptMin {
    /// `math_GlobOptMin(F, A, B, C, DiscretizationTol, SameTol)`.
    pub fn new(
        f: &mut GlobFunction<'_>,
        lower: &Vector,
        upper: &Vector,
        c: f64,
        discretization_tol: f64,
        same_tol: f64,
    ) -> Result<Self, MathError> {
        let n = f.nb_variables();
        let mut s = Self {
            n,
            a: Vector::new(1, n)?,
            b: Vector::new(1, n)?,
            glob_a: Vector::new(1, n)?,
            glob_b: Vector::new(1, n)?,
            tol: 0.0,
            same_tol: 0.0,
            c,
            init_c: c,
            is_find_single_solution: false,
            functional_minimal_value: -INFINITE,
            is_const_locked: false,
            done: false,
            y: Vec::new(),
            sol_count: 0,
            z: -1.0,
            e1: 0.0,
            e2: 0.0,
            e3: 0.0,
            x: Vector::new(1, n)?,
            tmp: Vector::new(1, n)?,
            v: Vector::new(1, n)?,
            max_v: Vector::new(1, n)?,
            last_step: 0.0,
            min_cell_filter_sol: 0,
            cont: 2,
            f: INFINITE,
        };
        if !covers(lower, n) || !covers(upper, n) {
            return Err(MathError::Dimension);
        }
        s.copy_borders(lower, upper, true);
        s.tol = discretization_tol;
        s.same_tol = same_tol;

        const MAX_SQUARE_SEARCH_SOL: i32 = 200;
        let mut sol_nb: i32 = 1;
        for _ in 0..n {
            sol_nb = sol_nb.saturating_mul(3);
        }
        s.min_cell_filter_sol = max(f64::from(sol_nb.saturating_mul(2)), f64::from(MAX_SQUARE_SEARCH_SOL)) as i32;
        s.compute_init_sol(f);
        s.done = false;
        Ok(s)
    }

    fn copy_borders(&mut self, a: &Vector, b: &Vector, global: bool) {
        for i in 1..=self.n {
            if global {
                put(&mut self.glob_a, i, at(a, i));
                put(&mut self.glob_b, i, at(b, i));
            }
            put(&mut self.a, i, at(a, i));
            put(&mut self.b, i, at(b, i));
        }
        for i in 1..=self.n {
            put(&mut self.max_v, i, (at(&self.b, i) - at(&self.a, i)) / 3.0);
        }
    }

    /// `SetGlobalParams`.
    pub fn set_global_params(
        &mut self,
        f: &mut GlobFunction<'_>,
        lower: &Vector,
        upper: &Vector,
        c: f64,
        discretization_tol: f64,
        same_tol: f64,
    ) -> Result<(), MathError> {
        if f.nb_variables() != self.n || !covers(lower, self.n) || !covers(upper, self.n) {
            return Err(MathError::Dimension);
        }
        self.c = c;
        self.init_c = c;
        self.z = -1.0;
        self.sol_count = 0;
        self.copy_borders(lower, upper, true);
        self.tol = discretization_tol;
        self.same_tol = same_tol;
        self.compute_init_sol(f);
        self.done = false;
        Ok(())
    }

    /// `SetLocalParams`.
    pub fn set_local_params(&mut self, a: &Vector, b: &Vector) -> Result<(), MathError> {
        if !covers(a, self.n) || !covers(b, self.n) {
            return Err(MathError::Dimension);
        }
        self.z = -1.0;
        self.copy_borders(a, b, false);
        self.done = false;
        Ok(())
    }

    /// `SetTol`.
    pub fn set_tol(&mut self, discretization_tol: f64, same_tol: f64) {
        self.tol = discretization_tol;
        self.same_tol = same_tol;
    }

    /// `GetTol`: the discretization and the same-point tolerance.
    pub fn get_tol(&self) -> (f64, f64) {
        (self.tol, self.same_tol)
    }

    /// `Perform(isFindSingleSolution)`.
    pub fn perform(&mut self, f: &mut GlobFunction<'_>, find_single_solution: bool) -> Result<(), MathError> {
        if f.nb_variables() != self.n {
            return Err(MathError::Dimension);
        }
        self.done = false;

        // Compute parameters range.
        let mut min_length = f64::MAX;
        let mut max_length = -f64::MAX;
        for i in 1..=self.n {
            let current_length = at(&self.b, i) - at(&self.a, i);
            if current_length < min_length {
                min_length = current_length;
            }
            if current_length > max_length {
                max_length = current_length;
            }

            put(&mut self.v, i, 0.0);
        }

        if min_length < PCONFUSION {
            return Ok(());
        }

        if !self.is_const_locked {
            // Compute initial value for myC.
            self.compute_initial_values(f);
        }

        self.e1 = min_length * self.tol;
        self.e2 = max_length * self.tol;

        self.is_find_single_solution = find_single_solution;
        if find_single_solution {
            // Run local optimization if current value better than optimal.
            self.e3 = 0.0;
        } else if self.c > 1.0 {
            self.e3 = -max_length * self.tol / 4.0;
        } else {
            self.e3 = -max_length * self.tol * self.c / 4.0;
        }

        // Search single solution and current solution in its neighborhood.
        if self.check_functional_stop_criteria() {
            self.done = true;
            return Ok(());
        }

        self.last_step = 0.0;
        self.compute_global_extremum(f, self.n);

        self.done = true;
        Ok(())
    }

    /// `Points(index, sol)`.
    pub fn points(&self, index: i32, out: &mut Vector) -> Result<(), MathError> {
        if index < 1 || index > self.sol_count || !covers(out, self.n) {
            return Err(MathError::Range);
        }
        for j in 1..=self.n {
            let k = usize::try_from((index - 1) * self.n + j - 1).map_err(|_| MathError::Range)?;
            let x = *self.y.get(k).ok_or(MathError::Range)?;
            out.set_value(j, x)?;
        }
        Ok(())
    }

    /// `SetContinuity`.
    pub fn set_continuity(&mut self, cont: i32) {
        self.cont = cont;
    }

    /// `GetContinuity`.
    pub fn get_continuity(&self) -> i32 {
        self.cont
    }

    /// `SetFunctionalMinimalValue`.
    pub fn set_functional_minimal_value(&mut self, v: f64) {
        self.functional_minimal_value = v;
    }

    /// `GetFunctionalMinimalValue`.
    pub fn get_functional_minimal_value(&self) -> f64 {
        self.functional_minimal_value
    }

    /// `SetLipConstState`.
    pub fn set_lip_const_state(&mut self, flag: bool) {
        self.is_const_locked = flag;
    }

    /// `GetLipConstState`.
    pub fn get_lip_const_state(&self) -> bool {
        self.is_const_locked
    }

    /// `isDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// `GetF`.
    pub fn get_f(&self) -> f64 {
        self.f
    }

    /// `NbExtrema`.
    pub fn nb_extrema(&self) -> i32 {
        self.sol_count
    }

    /// `computeLocalExtremum`: the start point is `src`, or `out` itself when `src` is `None` (the call of
    /// `ComputeInitSol` aliases its input and output point).
    fn compute_local_extremum(
        &self,
        f: &mut GlobFunction<'_>,
        src: Option<&Vector>,
        val: &mut f64,
        out: &mut Vector,
    ) -> bool {
        let n = self.n;

        // Newton method.
        if self.cont >= 2
            && let GlobFunction::Hessian(h) = f
        {
            let h: &mut dyn MultipleVarFunctionWithHessian = &mut **h;
            if let Ok(mut newton) = NewtonMinimum::new(&*h, CONFUSION, 40, 1.0e-6, true)
                && newton.set_boundary(&self.glob_a, &self.glob_b).is_ok()
            {
                let start = src.unwrap_or(&*out);
                let _ = newton.perform(h, start);
                if newton.is_done() {
                    let _ = newton.location_into(out);
                    if let Ok(m) = newton.minimum() {
                        *val = m;
                    }
                    if self.is_inside(out) {
                        return true;
                    }
                }
            }
        }

        // BFGS method used.
        if self.cont >= 1 {
            let g: Option<&mut dyn MultipleVarFunctionWithGradient> = match f {
                GlobFunction::Hessian(h) => Some(&mut **h),
                GlobFunction::Gradient(g) => Some(&mut **g),
                GlobFunction::Value(_) => None,
            };
            if let Some(g) = g
                && let Ok(mut bfgs) = Bfgs::new(g.nb_variables(), 1.0e-8, 200, 1.0e-12)
                && bfgs.set_boundary(&self.glob_a, &self.glob_b).is_ok()
            {
                let start = src.unwrap_or(&*out);
                let _ = bfgs.perform(g, start);
                if bfgs.is_done() {
                    let _ = bfgs.location_into(out);
                    if let Ok(m) = bfgs.minimum() {
                        *val = m;
                    }
                    if self.is_inside(out) {
                        return true;
                    }
                }
            }
        }

        // Powell method used.
        if let Ok(mut m) = Matrix::filled(1, n, 1, n, 0.0) {
            for i in 1..=n {
                let _ = m.set_value(i, i, 1.0);
            }

            let fv = f.as_value();
            if let Ok(mut powell) = Powell::new(&*fv, 1e-10, 200, 1.0e-12) {
                let start = src.unwrap_or(&*out);
                let _ = powell.perform(fv, start, &m);
                if powell.is_done() {
                    let _ = powell.location_into(out);
                    if let Ok(mn) = powell.minimum() {
                        *val = mn;
                    }
                    if self.is_inside(out) {
                        return true;
                    }
                }
            }
        }

        false
    }

    fn compute_initial_values(&mut self, f: &mut GlobFunction<'_>) {
        let min_lc = 0.01;
        let max_lc = 1000.;
        let min_eps = 0.1;
        let max_eps = 100.;
        let n = self.n;
        let mut curr_val = f64::MAX;

        // Lipschitz constant approximation.
        let mut lip_const = 0.0;
        let mut prev_val_diag = 0.0;
        let pnt_nb: i32 = 13;
        if let Some(v) = f.value(&self.a) {
            prev_val_diag = v;
        }
        let mut prev_val_proj = prev_val_diag;
        let Ok(diff) = self.b.subtracted(&self.a) else { return };
        let step = diff.norm() / f64::from(pnt_nb);
        let Ok(param_step) = diff.divided(f64::from(pnt_nb)) else { return };
        let Ok(mut curr_pnt) = Vector::new(1, n) else { return };
        for i in 1..=pnt_nb {
            if let Ok(p) = self.a.added(&param_step.multiplied(f64::from(i))) {
                let _ = curr_pnt.initialized(&p);
            }

            // Walk over diagonal.
            if let Some(v) = f.value(&curr_pnt) {
                curr_val = v;
            }
            lip_const = max((curr_val - prev_val_diag).abs(), lip_const);
            prev_val_diag = curr_val;

            // Walk over diag in projected space aPnt(1) = myA(1) = const.
            put(&mut curr_pnt, 1, at(&self.a, 1));
            if let Some(v) = f.value(&curr_pnt) {
                curr_val = v;
            }
            lip_const = max((curr_val - prev_val_proj).abs(), lip_const);
            prev_val_proj = curr_val;
        }

        self.c = self.init_c;
        lip_const *= f64::from(n).sqrt() / step;
        if lip_const < self.c * min_eps {
            self.c = max(lip_const * min_eps, min_lc);
        } else if lip_const > self.c * max_eps {
            self.c = min(self.c * max_eps, max_lc);
        }
    }

    fn compute_global_extremum(&mut self, f: &mut GlobFunction<'_>, j: i32) {
        let n = self.n;
        let mut d = f64::MAX;
        let mut val = f64::MAX;
        let Ok(mut step_best_point) = Vector::new(1, n) else { return };
        let mut is_inside;
        let mut is_reached = false;

        put(&mut self.x, j, at(&self.a, j) + self.e1);
        while !is_reached {
            if at(&self.x, j) > at(&self.b, j) {
                put(&mut self.x, j, at(&self.b, j));
                is_reached = true;
            }

            if self.check_functional_stop_criteria() {
                return; // Best possible value is obtained.
            }

            if j == 1 {
                is_inside = false;
                let prev_val = d;
                if let Some(v) = f.value(&self.x) {
                    d = v;
                }
                // Evtushenko estimation.
                let r1 = (d + self.z * self.c * self.last_step - self.f) * self.z;
                // Shubert / Piyavsky estimation.
                let r2 = ((d + prev_val - self.c * self.last_step) * 0.5 - self.f) * self.z;
                let r = min(r1, r2);
                if r > self.e3 {
                    let save_param = at(&self.x, 1);

                    // Piyavsky midpoint estimation.
                    let mut param = (2.0 * at(&self.x, 1) - at(&self.v, 1)) * 0.5 + (prev_val - d) * 0.5 / self.c;
                    if is_infinite(prev_val) {
                        param = at(&self.x, 1) - at(&self.v, 1) * 0.5; // Protection from upper dimension step.
                    }

                    put(&mut self.x, 1, param);
                    let mut a_val = 0.0;
                    if let Some(v) = f.value(&self.x) {
                        a_val = v;
                    }
                    put(&mut self.x, 1, save_param);

                    if (a_val < d && a_val < prev_val) || distance_to_border(&self.x, &self.a, &self.b) < self.e1 {
                        let mut tmp = self.tmp.clone();
                        is_inside = self.compute_local_extremum(f, Some(&self.x), &mut val, &mut tmp);
                        self.tmp = tmp;
                    }
                }
                let use_local = is_inside && val < d;
                let step_best_value = if use_local { val } else { d };
                let _ = step_best_point.initialized(if use_local { &self.tmp } else { &self.x });

                // Check point and value on the current step to be optimal.
                self.check_add_candidate(&step_best_point, step_best_value);

                if self.check_functional_stop_criteria() {
                    return; // Best possible value is obtained.
                }

                put(&mut self.v, 1, min(self.e2 + (self.f - d).abs() / self.c, at(&self.max_v, 1)));
                self.last_step = at(&self.v, 1);
            } else {
                put(&mut self.v, j, f64::MAX / 2.0);
                self.compute_global_extremum(f, j - 1);

                // Nullify steps on lower dimensions.
                for i in 1..j {
                    put(&mut self.v, i, 0.0);
                }
            }
            if j < n {
                let upper_dim_step = max(at(&self.v, j), self.e2);
                if at(&self.v, j + 1) > upper_dim_step {
                    if upper_dim_step > at(&self.max_v, j + 1) {
                        // Case of too big step.
                        put(&mut self.v, j + 1, at(&self.max_v, j + 1));
                    } else {
                        put(&mut self.v, j + 1, upper_dim_step);
                    }
                }
            }
            let xj = at(&self.x, j) + at(&self.v, j);
            put(&mut self.x, j, xj);
        }
    }

    fn is_inside(&self, pnt: &Vector) -> bool {
        for i in 1..=self.n {
            if at(pnt, i) < at(&self.glob_a, i) || at(pnt, i) > at(&self.glob_b, i) {
                return false;
            }
        }
        true
    }

    /// `isStored`: the cell filter of the C++ (used from `myMinCellFilterSol` solutions on) never answers `true`.
    fn is_stored(&self, pnt: &Vector) -> bool {
        let Ok(diff) = self.b.subtracted(&self.a) else { return false };
        let tol = diff.multiplied(self.same_tol);

        // C1 * n^2 = C2 * 3^dim * n
        if self.sol_count < self.min_cell_filter_sol {
            for i in 0..self.sol_count {
                let mut is_same = true;
                for j in 1..=self.n {
                    let Ok(k) = usize::try_from(i * self.n + j - 1) else { return false };
                    let yv = self.y.get(k).copied().unwrap_or(0.0);
                    if (at(pnt, j) - yv).abs() > at(&tol, j) {
                        is_same = false;
                        break;
                    }
                }
                if is_same {
                    return true;
                }
            }
        }
        false
    }

    fn check_functional_stop_criteria(&self) -> bool {
        // Search single solution and current solution in its neighborhood.
        self.is_find_single_solution && (self.f - self.functional_minimal_value).abs() < self.same_tol * 0.01
    }

    fn compute_init_sol(&mut self, f: &mut GlobFunction<'_>) {
        let mut val = 0.0;
        let Ok(mut pnt) = Vector::new(1, self.n) else { return };

        // Check functional value in midpoint. It is necessary since local optimization
        // algorithm may fail and return nothing. This is a protection from uninitialized
        // variables.
        if let Ok(sum) = self.glob_a.added(&self.glob_b) {
            let _ = pnt.initialized(&sum.multiplied(0.5));
        }
        if let Some(v) = f.value(&pnt) {
            val = v;
        }
        self.check_add_candidate(&pnt, val);

        // Run local optimization from lower corner, midpoint, and upper corner.
        for i in 1..=3 {
            if let Ok(diff) = self.b.subtracted(&self.a)
                && let Ok(p) = diff.multiplied(f64::from(i - 1)).divided(2.0)
                && let Ok(p) = self.a.added(&p)
            {
                let _ = pnt.initialized(&p);
            }

            if self.compute_local_extremum(f, None, &mut val, &mut pnt) {
                self.check_add_candidate(&pnt, val);
            }
        }
    }

    fn check_add_candidate(&mut self, pnt: &Vector, value: f64) {
        // Value in point is close to optimal value, and several optimal solutions are allowed.
        if (value - self.f).abs() < self.same_tol * 0.01 && !self.is_find_single_solution && !self.is_stored(pnt) {
            if (value - self.f) * self.z > 0.0 {
                self.f = value;
            }
            for j in 1..=self.n {
                self.y.push(at(pnt, j));
            }
            self.sol_count += 1;
        }

        // New best solution:
        // new point is out of (mySameTol * 0.01) surrounding or
        // new point is better than old and single point search.
        let delta = (value - self.f) * self.z;
        if delta > self.same_tol * 0.01 || (delta > 0.0 && self.is_find_single_solution) {
            self.f = value;
            self.y.clear();
            for j in 1..=self.n {
                self.y.push(at(pnt, j));
            }
            self.sol_count = 1;
        }
    }
}
