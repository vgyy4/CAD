//! Differential judge for OCCT's header-only `MathOpt` package (OCCT 8.0.1): replays `tests/mathopt.txt`, printed by
//! `oracle/mathopt.cpp` against the real library. See `docs/api-mathopt.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::mathlin::{MathError, Matrix, Vector};
use occt_port::mathopt::{self as mo, *};
use occt_port::mathutils::{
    Config, ScalarFunction, ScalarResult, Status, VectorFunction, VectorFunctionWithGradient, VectorResult,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("mathopt.txt");
const GOLDEN_LINES: usize = 590;
const MIN_DISTINCT: usize = 8;
const FAMILIES: &[&str] = &[
    "bfgs",
    "bfgs_bounded",
    "bfgs_numerical",
    "brent",
    "brent_with_bracket",
    "differential_evolution",
    "frpr",
    "frpr_numerical",
    "global_minimum",
    "global_minimum_simple",
    "golden",
    "lbfgs",
    "multi_start",
    "newton",
    "newton_bounded",
    "newton_modified",
    "newton_numerical",
    "newton_numerical_hessian",
    "polish_coordinate_wise",
    "powell",
    "powell_with_directions",
    "pso",
    "pso_simple",
    "types",
    "uzawa",
    "uzawa_equality",
];

#[derive(Debug, Clone, Copy)]
enum Tok {
    I(i128),
    F(f64),
}

struct Rd<'a> {
    w: Vec<&'a str>,
    p: usize,
}
impl<'a> Rd<'a> {
    fn new(words: &[&'a str]) -> Self {
        Self { w: words.to_vec(), p: 0 }
    }
    fn next(&mut self) -> &'a str {
        let s = self.w[self.p];
        self.p += 1;
        s
    }
    fn i(&mut self) -> i32 {
        self.next().parse().unwrap()
    }
    fn u(&mut self) -> u64 {
        self.next().parse().unwrap()
    }
    fn b(&mut self) -> bool {
        self.i() != 0
    }
    fn f(&mut self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.next(), 16).unwrap())
    }
    fn mat(&mut self) -> Matrix {
        let (lr, ur, lc, uc) = (self.i(), self.i(), self.i(), self.i());
        let (rows, cols) = (ur - lr + 1, uc - lc + 1);
        let vals: Vec<f64> = (0..rows * cols).map(|_| self.f()).collect();
        Matrix::from_slice(lr, lc, rows, cols, &vals).unwrap()
    }
    fn vec(&mut self) -> Vector {
        let (l, u) = (self.i(), self.i());
        let vals: Vec<f64> = (0..u - l + 1).map(|_| self.f()).collect();
        Vector::from_slice(l, &vals).unwrap()
    }
    fn cfg(&mut self) -> Config {
        let (tolerance, x_tolerance, f_tolerance) = (self.f(), self.f(), self.f());
        let max_iterations = self.i();
        Config { max_iterations, tolerance, x_tolerance, f_tolerance, step_min: self.f() }
    }
    fn ncfg(&mut self) -> NewtonConfig {
        let base = self.cfg();
        NewtonConfig { base, regularization: self.f(), use_line_search: self.b() }
    }
    fn fcfg(&mut self) -> FrprConfig {
        let base = self.cfg();
        let formula = match self.i() {
            0 => ConjugateGradientFormula::FletcherReeves,
            1 => ConjugateGradientFormula::PolakRibiere,
            2 => ConjugateGradientFormula::HestenesStiefel,
            _ => ConjugateGradientFormula::DaiYuan,
        };
        FrprConfig { base, formula, restart_interval: self.i() }
    }
    fn pcfg(&mut self) -> PsoConfig {
        let base = occt_port::mathutils::NDimConfig { base: self.cfg(), use_bounds: self.b() };
        let nb_particles = self.i();
        let (omega, phi_personal, phi_global, velocity_clamp) = (self.f(), self.f(), self.f(), self.f());
        let seed = u32::try_from(self.u()).unwrap();
        let init_mode = match self.i() {
            0 => PsoInitMode::RandomOnly,
            1 => PsoInitMode::SeededOnly,
            _ => PsoInitMode::SeededPlusRandom,
        };
        let boundary_mode = match self.i() {
            0 => PsoBoundaryMode::Clamp,
            1 => PsoBoundaryMode::Reflect,
            _ => PsoBoundaryMode::Wrap,
        };
        let inertia_schedule =
            if self.i() == 0 { PsoInertiaSchedule::Constant } else { PsoInertiaSchedule::LinearDecay };
        let omega_min = self.f();
        let min_iterations = self.i();
        let has_target = self.b();
        let target = self.f();
        PsoConfig {
            base,
            nb_particles,
            omega,
            phi_personal,
            phi_global,
            velocity_clamp,
            seed,
            init_mode,
            boundary_mode,
            inertia_schedule,
            omega_min,
            min_iterations,
            target_value: has_target.then_some(target),
            no_improve_tol: self.f(),
            no_improve_iters: self.i(),
            restart_fraction: self.f(),
            max_restarts: self.i(),
            polish_budget_per_dim: self.i(),
        }
    }
    fn gcfg(&mut self) -> GlobalConfig {
        let base = occt_port::mathutils::NDimConfig { base: self.cfg(), use_bounds: self.b() };
        let strategy = match self.i() {
            0 => GlobalStrategy::Pso,
            1 => GlobalStrategy::MultiStart,
            2 => GlobalStrategy::PsoHybrid,
            _ => GlobalStrategy::DifferentialEvolution,
        };
        GlobalConfig {
            base,
            strategy,
            nb_population: self.i(),
            nb_starts: self.i(),
            mutation_scale: self.f(),
            crossover_prob: self.f(),
            seed: u32::try_from(self.u()).unwrap(),
            polish_budget_per_dim: self.i(),
        }
    }
    fn seeds(&mut self) -> Vec<PsoSeedParticle> {
        let count = self.i();
        (0..count)
            .map(|_| {
                let position = self.vec();
                let (has_value, value) = (self.b(), self.f());
                let (has_velocity, velocity) = (self.b(), self.vec());
                PsoSeedParticle {
                    position,
                    value: has_value.then_some(value),
                    velocity: has_velocity.then_some(velocity),
                }
            })
            .collect()
    }
    fn finish(&self) {
        assert_eq!(self.p, self.w.len(), "unread input tokens");
    }
}

#[derive(Default)]
struct Out(Vec<Tok>);
impl Out {
    fn i(&mut self, v: i64) {
        self.0.push(Tok::I(v.into()));
    }
    fn u(&mut self, v: u64) {
        self.0.push(Tok::I(v.into()));
    }
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn b(&mut self, v: bool) {
        self.i(v.into());
    }
    fn mat(&mut self, m: &Matrix) {
        self.i(m.lower_row().into());
        self.i(m.upper_row().into());
        self.i(m.lower_col().into());
        self.i(m.upper_col().into());
        for r in m.lower_row()..=m.upper_row() {
            for c in m.lower_col()..=m.upper_col() {
                self.f(m.value(r, c).unwrap());
            }
        }
    }
    fn vec(&mut self, v: &Vector) {
        self.i(v.lower().into());
        self.i(v.upper().into());
        for k in v.lower()..=v.upper() {
            self.f(v.value(k).unwrap());
        }
    }
    fn opt_f(&mut self, v: Option<f64>) {
        self.b(v.is_some());
        if let Some(v) = v {
            self.f(v);
        }
    }
    fn scalar_result(&mut self, r: &ScalarResult, t: &Ts) {
        self.i(r.status as i64);
        self.i(r.nb_iterations as i64);
        self.opt_f(r.root);
        self.opt_f(r.value);
        self.opt_f(r.derivative);
        self.i(t.calls.into());
        self.u(t.hash);
    }
    fn calls(&mut self, t: &Tv) {
        self.i(t.vcalls.into());
        self.i(t.gcalls.into());
        self.i(t.hcalls.into());
        self.u(t.hash);
    }
    fn vector_result(&mut self, r: &VectorResult, t: &Tv) {
        self.i(r.status as i64);
        self.i(r.nb_iterations as i64);
        self.b(r.solution.is_some());
        if let Some(v) = &r.solution {
            self.vec(v);
        }
        self.opt_f(r.value);
        self.b(r.gradient.is_some());
        if let Some(v) = &r.gradient {
            self.vec(v);
        }
        self.b(r.jacobian.is_some());
        if let Some(m) = &r.jacobian {
            self.mat(m);
        }
        self.calls(t);
    }
    fn stats(&mut self, s: &PsoStats) {
        self.i(s.nb_function_evals.into());
        self.i(s.nb_iterations.into());
        self.i(s.nb_boundary_corrections.into());
        self.i(s.nb_stagnation_events.into());
        self.i(s.nb_restarts.into());
        self.f(s.initial_best);
        self.f(s.final_best);
    }
    fn cfg(&mut self, c: &Config) {
        self.f(c.tolerance);
        self.f(c.x_tolerance);
        self.f(c.f_tolerance);
        self.i(c.max_iterations.into());
        self.f(c.step_min);
    }
    fn ncfg(&mut self, c: &NewtonConfig) {
        self.cfg(&c.base);
        self.f(c.regularization);
        self.b(c.use_line_search);
    }
    fn fcfg(&mut self, c: &FrprConfig) {
        self.cfg(&c.base);
        self.i(c.formula as i64);
        self.i(c.restart_interval.into());
    }
    fn pcfg(&mut self, c: &PsoConfig) {
        self.cfg(&c.base.base);
        self.b(c.base.use_bounds);
        self.i(c.nb_particles.into());
        self.f(c.omega);
        self.f(c.phi_personal);
        self.f(c.phi_global);
        self.f(c.velocity_clamp);
        self.u(c.seed.into());
        self.i(c.init_mode as i64);
        self.i(c.boundary_mode as i64);
        self.i(c.inertia_schedule as i64);
        self.f(c.omega_min);
        self.i(c.min_iterations.into());
        self.b(c.target_value.is_some());
        self.f(c.target_value.unwrap_or(0.0));
        self.f(c.no_improve_tol);
        self.i(c.no_improve_iters.into());
        self.f(c.restart_fraction);
        self.i(c.max_restarts.into());
        self.i(c.polish_budget_per_dim.into());
    }
    fn gcfg(&mut self, c: &GlobalConfig) {
        self.cfg(&c.base.base);
        self.b(c.base.use_bounds);
        self.i(c.strategy as i64);
        self.i(c.nb_population.into());
        self.i(c.nb_starts.into());
        self.f(c.mutation_scale);
        self.f(c.crossover_prob);
        self.u(c.seed.into());
        self.i(c.polish_budget_per_dim.into());
    }
}

// ---- callbacks (identical to the oracle's) -----------------------------------------------------------------
fn mixh(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x100000001b3)
}
const H0: u64 = 0xcbf29ce484222325;

struct Ts {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    fl: f64,
    fh: f64,
    calls: i32,
    hash: u64,
}
impl Ts {
    fn read(rd: &mut Rd) -> Self {
        let (a, b, c, d, fl, fh) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
        Self { a, b, c, d, fl, fh, calls: 0, hash: H0 }
    }
}
impl ScalarFunction for Ts {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.calls += 1;
        self.hash = mixh(self.hash, x.to_bits());
        if x >= self.fl && x <= self.fh {
            return None;
        }
        let t = x - self.a;
        let u = x - self.b;
        Some(t * t * (1.0 + self.c * u * u) + self.d)
    }
}

struct Tv {
    w: Vec<f64>,
    c: Vec<f64>,
    q: f64,
    k: f64,
    fail_v: f64,
    fail_g: f64,
    fail_h: f64,
    w_lo: f64,
    w_hi: f64,
    hmode: i32,
    vcalls: i32,
    gcalls: i32,
    hcalls: i32,
    hash: u64,
}
impl Tv {
    fn read(rd: &mut Rd) -> Self {
        let w = rd.vec().as_slice().to_vec();
        let c = rd.vec().as_slice().to_vec();
        let (q, k, fail_v, fail_g, fail_h, w_lo, w_hi) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
        let hmode = rd.i();
        Self { w, c, q, k, fail_v, fail_g, fail_h, w_lo, w_hi, hmode, vcalls: 0, gcalls: 0, hcalls: 0, hash: H0 }
    }
    fn mixx(&mut self, x: &Vector, salt: u64) {
        self.hash = mixh(self.hash, salt);
        for i in x.lower()..=x.upper() {
            self.hash = mixh(self.hash, x.value(i).unwrap().to_bits());
        }
    }
    fn wi(&self, i: i32) -> f64 {
        self.w[i as usize % self.w.len()]
    }
    fn ci(&self, i: i32) -> f64 {
        self.c[i as usize % self.c.len()]
    }
}
impl VectorFunction for Tv {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.vcalls += 1;
        self.mixx(x, 1);
        let xv = |i: i32| x.value(x.lower() + i).unwrap();
        let x0 = x.value(x.lower()).unwrap();
        if x0 > self.fail_v || (x0 >= self.w_lo && x0 <= self.w_hi) {
            return None;
        }
        let n = x.length();
        let mut s = 0.0;
        for i in 0..n {
            let d = xv(i) - self.ci(i);
            s += self.wi(i) * d * d;
        }
        for i in 0..n - 1 {
            let r = xv(i + 1) - xv(i) * xv(i);
            s += self.q * r * r;
        }
        for i in 0..n {
            let xi2 = xv(i) * xv(i);
            s += self.k * xi2 * xi2;
        }
        Some(s)
    }
}
impl VectorFunctionWithGradient for Tv {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        self.gcalls += 1;
        self.mixx(x, 2);
        if x.value(x.lower()).unwrap() > self.fail_g {
            return false;
        }
        let xv = |i: i32| x.value(x.lower() + i).unwrap();
        let gl = g.lower();
        let n = x.length();
        for i in 0..n {
            g.set_value(gl + i, 2.0 * self.wi(i) * (xv(i) - self.ci(i))).unwrap();
        }
        for i in 0..n - 1 {
            let xi = xv(i);
            let r = xv(i + 1) - xi * xi;
            let v = g.value(gl + i).unwrap() + 2.0 * self.q * r * (-2.0 * xi);
            g.set_value(gl + i, v).unwrap();
            let v = g.value(gl + i + 1).unwrap() + 2.0 * self.q * r;
            g.set_value(gl + i + 1, v).unwrap();
        }
        for i in 0..n {
            let xi = xv(i);
            let v = g.value(gl + i).unwrap() + 4.0 * self.k * xi * xi * xi;
            g.set_value(gl + i, v).unwrap();
        }
        true
    }
}
impl VectorFunctionWithHessian for Tv {
    fn hessian(&mut self, x: &Vector, h: &mut Matrix) -> bool {
        self.hcalls += 1;
        self.mixx(x, 3);
        if x.value(x.lower()).unwrap() > self.fail_h {
            return false;
        }
        let xv = |i: i32| x.value(x.lower() + i).unwrap();
        let n = x.length();
        let (r0, c0) = (h.lower_row(), h.lower_col());
        let add = |h: &mut Matrix, i: i32, j: i32, v: f64| {
            let cur = h.value(r0 + i, c0 + j).unwrap();
            h.set_value(r0 + i, c0 + j, cur + v).unwrap();
        };
        for i in 0..n {
            for j in 0..n {
                h.set_value(r0 + i, c0 + j, 0.0).unwrap();
            }
        }
        if self.hmode == 1 {
            return true;
        }
        for i in 0..n {
            add(h, i, i, 2.0 * self.wi(i));
        }
        for i in 0..n - 1 {
            let xi = xv(i);
            let r = xv(i + 1) - xi * xi;
            add(h, i, i, 2.0 * self.q * (4.0 * xi * xi - 2.0 * r));
            add(h, i, i + 1, 2.0 * self.q * (-2.0 * xi));
            add(h, i + 1, i, 2.0 * self.q * (-2.0 * xi));
            add(h, i + 1, i + 1, 2.0 * self.q);
        }
        for i in 0..n {
            let xi = xv(i);
            add(h, i, i, 12.0 * self.k * xi * xi);
        }
        if self.hmode == 2 {
            for i in 0..n {
                for j in 0..n {
                    let v = h.value(r0 + i, c0 + j).unwrap();
                    h.set_value(r0 + i, c0 + j, -v).unwrap();
                }
            }
        }
        true
    }
}

fn uzawa_out(o: &mut Out, r: &UzawaResult) {
    o.i(r.status as i64);
    o.i(r.nb_iterations.into());
    for v in [&r.solution, &r.dual, &r.error, &r.initial_error] {
        o.b(v.is_some());
        if let Some(v) = v {
            o.vec(v);
        }
    }
    o.b(r.inverse_ctc.is_some());
    if let Some(m) = &r.inverse_ctc {
        o.mat(m);
    }
    o.b(r.is_done());
}

fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, MathError> {
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let mut o = Out::default();
    match family {
        "brent" | "golden" => {
            let mut t = Ts::read(rd);
            let (lo, hi, c) = (rd.f(), rd.f(), rd.cfg());
            rd.finish();
            let r = if family == "brent" { mo::brent(&mut t, lo, hi, &c) } else { mo::golden(&mut t, lo, hi, &c) };
            o.scalar_result(&r, &t);
        }
        "brent_with_bracket" => {
            let mut t = Ts::read(rd);
            let (guess, step, c) = (rd.f(), rd.f(), rd.cfg());
            rd.finish();
            let r = mo::brent_with_bracket(&mut t, guess, step, &c);
            o.scalar_result(&r, &t);
        }
        "powell" => {
            let mut t = read_tv(rd);
            let (x0, c) = (rd.vec(), rd.cfg());
            rd.finish();
            let r = mo::powell(&mut t, &x0, &c)?;
            o.vector_result(&r, &t);
        }
        "powell_with_directions" => {
            let mut t = read_tv(rd);
            let (x0, dirs, c) = (rd.vec(), rd.mat(), rd.cfg());
            rd.finish();
            let r = mo::powell_with_directions(&mut t, &x0, &dirs, &c)?;
            o.vector_result(&r, &t);
        }
        "frpr" => {
            let mut t = read_tv(rd);
            let (x0, c) = (rd.vec(), rd.fcfg());
            rd.finish();
            let r = mo::frpr(&mut t, &x0, &c)?;
            o.vector_result(&r, &t);
        }
        "frpr_numerical" => {
            let mut t = read_tv(rd);
            let (x0, step, c) = (rd.vec(), rd.f(), rd.fcfg());
            rd.finish();
            let r = mo::frpr_numerical(&mut t, &x0, step, &c)?;
            o.vector_result(&r, &t);
        }
        "bfgs" => {
            let mut t = read_tv(rd);
            let (x0, c) = (rd.vec(), rd.cfg());
            rd.finish();
            let r = mo::bfgs(&mut t, &x0, &c)?;
            o.vector_result(&r, &t);
        }
        "bfgs_numerical" => {
            let mut t = read_tv(rd);
            let (x0, step, c) = (rd.vec(), rd.f(), rd.cfg());
            rd.finish();
            let r = mo::bfgs_numerical(&mut t, &x0, step, &c)?;
            o.vector_result(&r, &t);
        }
        "lbfgs" => {
            let mut t = read_tv(rd);
            let (x0, mem, c) = (rd.vec(), rd.i(), rd.cfg());
            rd.finish();
            let r = mo::lbfgs(&mut t, &x0, mem, &c)?;
            o.vector_result(&r, &t);
        }
        "bfgs_bounded" => {
            let mut t = read_tv(rd);
            let (x0, lo, hi, c) = (rd.vec(), rd.vec(), rd.vec(), rd.cfg());
            rd.finish();
            let r = mo::bfgs_bounded(&mut t, &x0, &lo, &hi, &c)?;
            o.vector_result(&r, &t);
        }
        "newton" | "newton_modified" => {
            let mut t = read_tv(rd);
            let (x0, c) = (rd.vec(), rd.ncfg());
            rd.finish();
            let r =
                if family == "newton" { mo::newton(&mut t, &x0, &c)? } else { mo::newton_modified(&mut t, &x0, &c)? };
            o.vector_result(&r, &t);
        }
        "newton_numerical_hessian" => {
            let mut t = read_tv(rd);
            let (x0, hstep, c) = (rd.vec(), rd.f(), rd.ncfg());
            rd.finish();
            let r = mo::newton_numerical_hessian(&mut t, &x0, hstep, &c)?;
            o.vector_result(&r, &t);
        }
        "newton_numerical" => {
            let mut t = read_tv(rd);
            let (x0, gstep, hstep, c) = (rd.vec(), rd.f(), rd.f(), rd.ncfg());
            rd.finish();
            let r = mo::newton_numerical(&mut t, &x0, gstep, hstep, &c)?;
            o.vector_result(&r, &t);
        }
        "newton_bounded" => {
            let mut t = read_tv(rd);
            let (x0, lo, hi, c) = (rd.vec(), rd.vec(), rd.vec(), rd.ncfg());
            rd.finish();
            let r = mo::newton_bounded(&mut t, &x0, &lo, &hi, &c)?;
            o.vector_result(&r, &t);
        }
        "uzawa" | "uzawa_equality" => {
            let (cont, sec, x0) = (rd.mat(), rd.vec(), rd.vec());
            let (nce, nci) = if family == "uzawa" { (rd.i(), rd.i()) } else { (0, 0) };
            let cfg = UzawaConfig { eps_lix: rd.f(), eps_lic: rd.f(), max_iterations: rd.i() };
            rd.finish();
            let r = if family == "uzawa" {
                mo::uzawa(&cont, &sec, &x0, nce, nci, &cfg)?
            } else {
                mo::uzawa_equality(&cont, &sec, &x0, &cfg)?
            };
            uzawa_out(&mut o, &r);
        }
        "pso" => {
            let mut t = read_tv(rd);
            let (lo, hi, c) = (rd.vec(), rd.vec(), rd.pcfg());
            let seeds = rd.seeds();
            let with_stats = rd.b();
            rd.finish();
            let mut st = PsoStats::default();
            let r = mo::pso(&mut t, &lo, &hi, &c, Some(&seeds), with_stats.then_some(&mut st))?;
            o.vector_result(&r, &t);
            o.stats(&st);
        }
        "pso_simple" => {
            let mut t = read_tv(rd);
            let (lo, hi, c) = (rd.vec(), rd.vec(), rd.pcfg());
            rd.finish();
            let r = mo::pso_simple(&mut t, &lo, &hi, &c)?;
            o.vector_result(&r, &t);
        }
        "polish_coordinate_wise" => {
            let mut t = read_tv(rd);
            let (mut pos, lo, hi, mut val, tol, budget) = (rd.vec(), rd.vec(), rd.vec(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let mut evals = -1;
            mo::polish_coordinate_wise(&mut t, &mut pos, &mut val, &lo, &hi, tol, budget, &mut evals)?;
            o.vec(&pos);
            o.f(val);
            o.i(evals.into());
            o.calls(&t);
        }
        "differential_evolution" | "multi_start" | "global_minimum_simple" => {
            let mut t = read_tv(rd);
            let (lo, hi, c) = (rd.vec(), rd.vec(), rd.gcfg());
            rd.finish();
            let r = match family {
                "differential_evolution" => mo::differential_evolution(&mut t, &lo, &hi, &c)?,
                "multi_start" => mo::multi_start(&mut t, &lo, &hi, &c)?,
                _ => mo::global_minimum(&mut t, &lo, &hi, &c)?,
            };
            o.vector_result(&r, &t);
        }
        "global_minimum" => {
            let mut t = read_tv(rd);
            let (lo, hi, c) = (rd.vec(), rd.vec(), rd.gcfg());
            let (with_pso, pc) = (rd.b(), rd.pcfg());
            let seeds = rd.seeds();
            let with_stats = rd.b();
            rd.finish();
            let mut st = PsoStats::default();
            let r = mo::global_minimum_with(
                &mut t,
                &lo,
                &hi,
                &c,
                with_pso.then_some(&pc),
                (!seeds.is_empty()).then_some(&seeds[..]),
                with_stats.then_some(&mut st),
            )?;
            o.vector_result(&r, &t);
            o.stats(&st);
        }
        "types" => {
            let kind = rd.i();
            rd.finish();
            types_case(&mut o, kind);
        }
        _ => panic!("unknown family {family}"),
    }
    Ok(o.0)
}

#[allow(non_snake_case)]
fn read_tv(rd: &mut Rd) -> Tv {
    Tv::read(rd)
}

fn types_case(o: &mut Out, kind: i32) {
    match kind {
        0 => o.fcfg(&FrprConfig::default()),
        1 => o.fcfg(&FrprConfig::new(1e-5, 100)),
        2 => o.fcfg(&FrprConfig::new(1e-5, 7)),
        3 => o.ncfg(&NewtonConfig::default()),
        4 => o.ncfg(&NewtonConfig::new(1e-3, 9)),
        5 => {
            let c = UzawaConfig::default();
            o.f(c.eps_lix);
            o.f(c.eps_lic);
            o.i(c.max_iterations.into());
        }
        6 => o.pcfg(&PsoConfig::default()),
        7 => o.pcfg(&PsoConfig::new(25, 100, 1e-8)),
        8 => o.pcfg(&PsoConfig::new(25, 77, 1e-3)),
        9 => o.gcfg(&GlobalConfig::default()),
        10 => o.gcfg(&GlobalConfig::new(GlobalStrategy::DifferentialEvolution, 200)),
        11 => o.gcfg(&GlobalConfig::new(GlobalStrategy::MultiStart, 300)),
        12 => {
            o.stats(&PsoStats::default());
            let a = PsoSeedParticle::new(Vector::filled(2, 3, 0.5).unwrap());
            let b = PsoSeedParticle::with_value(Vector::filled(2, 3, 1.5).unwrap(), 0.25);
            o.vec(&a.position);
            o.b(a.value.is_some());
            o.b(a.velocity.is_some());
            o.vec(&b.position);
            o.b(b.value.is_some());
            o.f(b.value.unwrap());
            o.b(b.velocity.is_some());
        }
        _ => {
            for v in [
                ConjugateGradientFormula::FletcherReeves as i64,
                ConjugateGradientFormula::PolakRibiere as i64,
                ConjugateGradientFormula::HestenesStiefel as i64,
                ConjugateGradientFormula::DaiYuan as i64,
                PsoInitMode::RandomOnly as i64,
                PsoInitMode::SeededOnly as i64,
                PsoInitMode::SeededPlusRandom as i64,
                PsoBoundaryMode::Clamp as i64,
                PsoBoundaryMode::Reflect as i64,
                PsoBoundaryMode::Wrap as i64,
                PsoInertiaSchedule::Constant as i64,
                PsoInertiaSchedule::LinearDecay as i64,
                GlobalStrategy::Pso as i64,
                GlobalStrategy::MultiStart as i64,
                GlobalStrategy::PsoHybrid as i64,
                GlobalStrategy::DifferentialEvolution as i64,
            ] {
                o.i(v);
            }
            let u = UzawaResult::default();
            o.i(u.status as i64);
            o.i(u.nb_iterations.into());
            o.b(u.is_done());
        }
    }
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
}

fn compare(t: &mut Tally, family: &str, line: &str, got: &Result<Vec<Tok>, MathError>, want: &[&str]) {
    if want == ["throws"] {
        assert!(got.is_err(), "{family}: C++ throws but Rust returned Ok\n{line}");
        return;
    }
    let got = match got {
        Ok(g) => g,
        Err(e) => panic!("{family}: Rust returned Err({e:?}) but C++ did not throw\n{line}"),
    };
    assert_eq!(got.len(), want.len(), "{family}: output length\n{line}");
    for (k, (g, w)) in got.iter().zip(want).enumerate() {
        match g {
            Tok::I(v) => assert_eq!(*v, w.parse::<i128>().unwrap(), "{family} output {k}\n{line}"),
            Tok::F(v) => {
                let e = f64::from_bits(u64::from_str_radix(w, 16).unwrap());
                t.values += 1;
                t.exact += usize::from(v.to_bits() == e.to_bits());
                if v.is_nan() && e.is_nan() {
                    continue;
                }
                assert_eq!(v.to_bits(), e.to_bits(), "{family} output {k}: {v:e} != {e:e}\n{line}");
            }
        }
    }
}

#[test]
fn mathopt_matches_occt() {
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut tally = Tally::default();
    let mut lines = 0;
    let mut throws = 0;
    for (n, line) in DATA.lines().enumerate() {
        let (left, right) = line.split_once(" |").unwrap_or_else(|| panic!("line {} malformed", n + 1));
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        let words: Vec<&str> = it.collect();
        assert!(seen.insert(line.to_owned()), "duplicate line {}", n + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let want: Vec<&str> = right.split_whitespace().collect();
        let got = run(family, &words);
        throws += usize::from(want == ["throws"]);
        compare(&mut tally, family, line, &got, &want);
        lines += 1;
    }
    assert_eq!(lines, GOLDEN_LINES);
    let mut want_fams = FAMILIES.to_vec();
    want_fams.sort_unstable();
    assert_eq!(counts.keys().map(String::as_str).collect::<Vec<_>>(), want_fams);
    for name in FAMILIES {
        assert!(distinct[*name].len() >= MIN_DISTINCT, "{name} needs {MIN_DISTINCT} distinct inputs");
    }
    assert!(throws >= 1, "the oracle must exercise `throws`");
    println!(
        "mathopt: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}

#[test]
fn status_values_are_distinct() {
    // The `Status` re-export is the one of `mathinteg`; the judge relies on its discriminants.
    assert_eq!(Status::Ok as i32, 0);
    assert_eq!(Status::InvalidInput as i32, 4);
    let _ = Status::Singular;
}
