//! Differential judge for OCCT's `MathUtils` package (OCCT 8.0.1): replays `tests/mathutils.txt`, printed by
//! `oracle/mathutils.cpp` against the real library. See `docs/api-mathutils.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::mathlin::{MathError, Matrix, Vector};
use occt_port::mathutils::{
    self as mu, ScalarFunction, Status, SystemFunction, VectorFunction, VectorFunctionWithGradient,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("mathutils.txt");
const GOLDEN_LINES: usize = 658;
const MIN_DISTINCT: usize = 10;
const MAX_ULPS: u64 = 8;
const FAMILIES: &[&str] = &[
    "armijo_backtrack",
    "bracket_minimum",
    "bracket_minimum_iter",
    "bracket_root",
    "brent_along_coordinate",
    "central_difference",
    "compute_scale_factor",
    "config",
    "converged_scalar",
    "converged_vector",
    "core_scalar",
    "cube_root",
    "depress_cubic_quartic",
    "domain1d",
    "domain2d",
    "eval_poly",
    "eval_poly_deriv",
    "exact_line_search",
    "forward_difference",
    "gauss_points_weights",
    "kronrod_points_weights",
    "numerical_gradient",
    "numerical_gradient_adaptive",
    "numerical_hessian",
    "numerical_jacobian",
    "ordered_gauss_points_weights",
    "quadratic_interpolation",
    "random_generator",
    "refine_poly_root",
    "remove_duplicate_roots",
    "result_types",
    "scalar_functor",
    "scalar_functor_libm",
    "second_derivative",
    "sort_roots",
    "vector_functor",
    "vector_functor_libm",
    "vector_lambda",
    "vector_norms",
    "wolfe_search",
];
/// Families computed with libm functions: compared within `MAX_ULPS` of the largest value on the line.
const LIBM: &[&str] = &["cube_root", "scalar_functor_libm", "vector_functor_libm"];

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
    fn calls(&mut self, calls: i32, hash: u64) {
        self.i(calls.into());
        self.u(hash);
    }
}

// ---- callbacks (identical to the oracle's) ----------------------------------------------------------------
fn mixh(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x100000001b3)
}
const H0: u64 = 0xcbf29ce484222325;

#[derive(Clone, Copy)]
struct Ts {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    flo: f64,
    fhi: f64,
    calls: i32,
    hash: u64,
}
impl Ts {
    fn read(rd: &mut Rd) -> Self {
        Self { a: rd.f(), b: rd.f(), c: rd.f(), d: rd.f(), flo: rd.f(), fhi: rd.f(), calls: 0, hash: H0 }
    }
    fn fresh(&self) -> Self {
        Self { calls: 0, hash: H0, ..*self }
    }
}
impl ScalarFunction for Ts {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.calls += 1;
        self.hash = mixh(self.hash, x.to_bits());
        if x > self.flo && x < self.fhi {
            return None;
        }
        Some(((self.d * x + self.c) * x + self.b) * x + self.a)
    }
}

struct Tv {
    kind: i32,
    pa: f64,
    pb: f64,
    failx: f64,
    a: Matrix,
    bv: Vector,
    c: f64,
    calls: i32,
    gcalls: i32,
    hash: u64,
}
impl Tv {
    fn read(rd: &mut Rd) -> Self {
        let (kind, pa, pb, failx) = (rd.i(), rd.f(), rd.f(), rd.f());
        let (a, bv, c) = (rd.mat(), rd.vec(), rd.f());
        Self { kind, pa, pb, failx, a, bv, c, calls: 0, gcalls: 0, hash: H0 }
    }
    fn mixx(&mut self, x: &Vector, salt: u64) {
        self.hash = mixh(self.hash, salt);
        for i in x.lower()..=x.upper() {
            self.hash = mixh(self.hash, x.value(i).unwrap().to_bits());
        }
    }
}
impl VectorFunction for Tv {
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.calls += 1;
        self.mixx(x, 1);
        if x.value(x.lower()).unwrap() > self.failx {
            return None;
        }
        match self.kind {
            0 => mu::Sphere.value(x),
            1 => mu::Rosenbrock::new(self.pa, self.pb).value(x),
            2 => mu::Booth.value(x),
            3 => mu::QuadraticForm::new(&self.a, &self.bv, self.c).value(x),
            _ => mu::Himmelblau.value(x),
        }
    }
}
impl VectorFunctionWithGradient for Tv {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        self.gcalls += 1;
        self.mixx(x, 2);
        match self.kind {
            0 => mu::Sphere.gradient(x, g),
            1 => mu::Rosenbrock::new(self.pa, self.pb).gradient(x, g),
            2 => mu::Booth.gradient(x, g),
            3 => mu::QuadraticForm::new(&self.a, &self.bv, self.c).gradient(x, g),
            _ => mu::Himmelblau.gradient(x, g),
        }
    }
}

struct Tsys {
    m: i32,
    failx: f64,
    calls: i32,
    hash: u64,
}
impl SystemFunction for Tsys {
    fn nb_equations(&self) -> i32 {
        self.m
    }
    fn value(&mut self, x: &Vector, f: &mut Vector) -> bool {
        self.calls += 1;
        for i in x.lower()..=x.upper() {
            self.hash = mixh(self.hash, x.value(i).unwrap().to_bits());
        }
        if x.value(x.lower()).unwrap() > self.failx {
            return false;
        }
        let mut s = 0.0;
        for i in x.lower()..=x.upper() {
            let v = x.value(i).unwrap();
            s += v * v;
        }
        let n = x.length();
        for i in 1..=self.m {
            f.set_value(i, f64::from(i) * s + x.value(x.lower() + (i - 1) % n).unwrap()).unwrap();
        }
        true
    }
}

fn slice(v: &Vector) -> Vec<f64> {
    v.as_slice().to_vec()
}

/// Replays one record; `Err` stands for a C++ exception (`throws`).
fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, MathError> {
    let mut rd = Rd::new(words);
    let mut o = Out::default();
    match family {
        "core_scalar" => {
            let (v1, v2, v3, tol) = (rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            o.f(mu::clamp(v1, v2, v3));
            o.b(mu::is_zero(v1, mu::THE_ZERO_TOL));
            o.b(mu::is_zero(v1, tol));
            o.b(mu::is_equal(v1, v2, mu::THE_ZERO_TOL));
            o.b(mu::is_equal(v1, v2, tol));
            o.f(mu::safe_div(v1, v2, 0.0));
            o.f(mu::safe_div(v1, v2, v3));
            o.i(mu::sign(v1).into());
            o.f(mu::sign_transfer(v1, v2));
            o.f(mu::sqr(v1));
            o.f(mu::cube(v1));
            o.b(mu::is_finite(v1));
            for c in [
                mu::THE_EPSILON,
                mu::THE_ZERO_TOL,
                mu::THE_PI,
                mu::THE_2PI,
                mu::THE_GOLDEN_RATIO,
                mu::THE_GOLDEN_SECTION,
            ] {
                o.f(c);
            }
        }
        "cube_root" => {
            let v = rd.f();
            rd.finish();
            o.f(mu::cube_root(v));
        }
        "compute_scale_factor" => {
            let c = rd.vec();
            rd.finish();
            o.f(mu::compute_scale_factor(c.as_slice()));
        }
        "vector_norms" => {
            let (a, b) = (rd.vec(), rd.vec());
            rd.finish();
            o.f(mu::dot_product(&a, &b)?);
            o.f(mu::vector_norm(&a));
            o.f(mu::vector_inf_norm(&a));
            o.f(mu::vector_norm(&b));
            o.f(mu::vector_inf_norm(&b));
        }
        "converged_scalar" => {
            let (xo, xn, fo, fn_, tol, xt, ft) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let c = mu::Config { x_tolerance: xt, f_tolerance: ft, ..mu::Config::default() };
            o.b(mu::is_x_converged(xo, xn, tol));
            o.b(mu::is_f_converged(fn_, tol));
            o.b(mu::is_converged(xo, xn, fn_, &c));
            o.b(mu::is_min_converged(xo, xn, fo, fn_, &c));
        }
        "converged_vector" => {
            let (old, new, g, tol) = (rd.vec(), rd.vec(), rd.vec(), rd.f());
            rd.finish();
            o.b(mu::is_vector_converged(&old, &new, tol)?);
            o.b(mu::is_gradient_converged(&g, tol));
            o.f(mu::infinity_norm(&g));
            o.f(mu::euclidean_norm(&g));
        }
        "config" => {
            let (tol, it, lo, up, ub) = (rd.f(), rd.i(), rd.f(), rd.f(), rd.i() != 0);
            rd.finish();
            let base = |o: &mut Out, c: &mu::Config| {
                o.i(c.max_iterations.into());
                o.f(c.tolerance);
                o.f(c.x_tolerance);
                o.f(c.f_tolerance);
                o.f(c.step_min);
            };
            for c in [mu::Config::default(), mu::Config::new(tol, it), mu::Config::new(tol, 100)] {
                base(&mut o, &c);
            }
            let bc = |o: &mut Out, c: &mu::BoundedConfig| {
                base(o, &c.base);
                o.f(c.lower_bound);
                o.f(c.upper_bound);
            };
            for c in [
                mu::BoundedConfig::default(),
                mu::BoundedConfig::new(lo, up, tol, it),
                mu::BoundedConfig::new(lo, up, 1e-10, 100),
            ] {
                bc(&mut o, &c);
            }
            for c in [mu::NDimConfig::default(), mu::NDimConfig::new(tol, it, ub), mu::NDimConfig::new(tol, 100, false)]
            {
                base(&mut o, &c.base);
                o.b(c.use_bounds);
            }
            for c in [mu::IntegConfig::default(), mu::IntegConfig::new(tol, it)] {
                o.i(c.initial_order.into());
                o.i(c.max_order.into());
                o.i(c.max_iterations.into());
                o.f(c.tolerance);
            }
            let lc = mu::LinConfig::default();
            o.f(lc.singularity_tolerance);
            o.b(lc.use_pivoting);
            o.f(mu::THE_NEWTON_FTOL_SQ);
            o.f(mu::THE_NEWTON_STEP_TOL_FACTOR);
            o.i(mu::THE_NEWTON_MAX_ITER as i64);
            o.f(mu::THE_NEWTON2D_SINGULAR_DET);
            o.f(mu::THE_NEWTON2D_CRITICAL_GRAD_SQ);
            o.f(mu::THE_NEWTON2D_MAX_STEP_RATIO);
            o.f(mu::THE_NEWTON2D_DOMAIN_EXT);
            o.f(mu::THE_NEWTON2D_STAGNATION_RATIO);
            o.i(mu::THE_NEWTON2D_STAGNATION_COUNT.into());
            o.i(mu::THE_NEWTON2D_LINE_SEARCH_MAX.into());
            o.f(mu::THE_NEWTON2D_SKIP_LINESEARCH_SQ);
            o.f(mu::THE_NEWTON2D_STAGNATION_RELAX);
            o.f(mu::THE_NEWTON2D_BACKTRACK_TRIGGER);
            o.f(mu::THE_NEWTON2D_BACKTRACK_ACCEPT);
            o.f(mu::THE_NEWTON2D_MAXITER_RELAX);
            o.f(mu::THE_HESSIAN_DEGENERACY_REL);
            o.f(mu::THE_HESSIAN_DEGENERACY_ABS);
            o.f(mu::THE_ARMIJO_C1);
        }
        "result_types" => {
            let j = rd.i();
            rd.finish();
            let st = status(j);
            let sr = mu::ScalarResult::default();
            let pr = mu::PolyResult::default();
            let vr = mu::VectorResult::default();
            let lr = mu::LinearResult::default();
            let lm = mu::LinearMultipleResult::default();
            let er = mu::EigenResult::default();
            let dr = mu::DecompResult::default();
            let ir = mu::IntegResult::default();
            let nr = mu::InverseResult::default();
            o.i(sr.status as i64);
            o.i(sr.nb_iterations as i64);
            o.b(sr.root.is_some());
            o.b(sr.value.is_some());
            o.b(sr.derivative.is_some());
            o.b(sr.is_done());
            o.i(pr.status as i64);
            o.i(pr.nb_roots as i64);
            for i in 0..4 {
                o.f(pr.index(i));
            }
            o.b(pr.is_done());
            o.i(vr.status as i64);
            o.i(vr.nb_iterations as i64);
            o.b(vr.solution.is_some());
            o.b(vr.value.is_some());
            o.b(vr.gradient.is_some());
            o.b(vr.jacobian.is_some());
            o.b(vr.is_done());
            o.i(lr.status as i64);
            o.b(lr.solution.is_some());
            o.b(lr.determinant.is_some());
            o.b(lr.is_done());
            o.i(lm.status as i64);
            o.b(lm.solutions.is_some());
            o.b(lm.determinant.is_some());
            o.b(lm.is_done());
            o.i(er.status as i64);
            o.i(er.nb_iterations as i64);
            o.b(er.eigen_values.is_some());
            o.b(er.eigen_vectors.is_some());
            o.b(er.is_done());
            o.i(dr.status as i64);
            o.b(dr.l.is_some());
            o.b(dr.u.is_some());
            o.b(dr.d.is_some());
            o.b(dr.determinant.is_some());
            o.b(dr.is_done());
            o.i(ir.status as i64);
            o.i(ir.nb_iterations as i64);
            o.i(ir.nb_points as i64);
            o.b(ir.value.is_some());
            o.b(ir.absolute_error.is_some());
            o.b(ir.relative_error.is_some());
            o.b(ir.is_done());
            o.i(nr.status as i64);
            o.b(nr.inverse.is_some());
            o.b(nr.determinant.is_some());
            o.b(nr.is_done());
            let sr = mu::ScalarResult { status: st, ..sr };
            let pr = mu::PolyResult { status: st, ..pr };
            let vr = mu::VectorResult { status: st, ..vr };
            let lr = mu::LinearResult { status: st, ..lr };
            let lm = mu::LinearMultipleResult { status: st, ..lm };
            let er = mu::EigenResult { status: st, ..er };
            let dr = mu::DecompResult { status: st, ..dr };
            let ir = mu::IntegResult { status: st, ..ir };
            let nr = mu::InverseResult { status: st, ..nr };
            for d in [
                sr.is_done(),
                pr.is_done(),
                vr.is_done(),
                lr.is_done(),
                lm.is_done(),
                er.is_done(),
                dr.is_done(),
                ir.is_done(),
                nr.is_done(),
            ] {
                o.b(d);
            }
            o.b(sr.is_done());
            o.b(pr.is_done());
            o.b(ir.is_done());
        }
        "random_generator" => {
            let (s1, s2) = (rd.u(), rd.u());
            rd.finish();
            let mut g = mu::RandomGenerator::new(s1);
            for _ in 0..6 {
                o.u(g.next_int());
            }
            for _ in 0..6 {
                o.f(g.next_real());
            }
            g.set_seed(s2);
            for _ in 0..3 {
                o.u(g.next_int());
            }
            o.f(g.next_real());
            let mut d = mu::RandomGenerator::default();
            o.u(d.next_int());
            o.u(d.next_int());
        }
        "eval_poly" => {
            let (c, x) = (slice(&rd.vec()), rd.f());
            rd.finish();
            let d = c.len() as i32 - 1;
            o.f(mu::eval_poly(&c, d, x));
            o.f(mu::eval_poly_desc(&c, d, x));
        }
        "eval_poly_deriv" => {
            let (c, x) = (slice(&rd.vec()), rd.f());
            rd.finish();
            let (v, dv) = mu::eval_poly_deriv(&c, c.len() as i32 - 1, x);
            o.f(v);
            o.f(dv);
        }
        "refine_poly_root" => {
            let (c, r, mi) = (slice(&rd.vec()), rd.f(), rd.i());
            rd.finish();
            let d = c.len() as i32 - 1;
            o.f(mu::refine_poly_root(&c, d, r, mi));
            o.f(mu::refine_poly_root_desc(&c, d, r, mi));
        }
        "sort_roots" => {
            let (n, rr) = (rd.i() as usize, rd.vec());
            rd.finish();
            let mut r = slice(&rr)[..n].to_vec();
            mu::sort_roots(&mut r);
            for v in r {
                o.f(v);
            }
        }
        "remove_duplicate_roots" => {
            let (n, rr, tol) = (rd.i() as usize, rd.vec(), rd.f());
            rd.finish();
            let mut r = slice(&rr)[..n].to_vec();
            let k = mu::remove_duplicate_roots(&mut r, tol);
            o.i(k as i64);
            for v in &r[..k] {
                o.f(*v);
            }
            let k2 = mu::remove_duplicate_roots(&mut r[..k], 1e-10);
            o.i(k2 as i64);
        }
        "depress_cubic_quartic" => {
            let (b, c, d, e) = (rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let (p, q, s) = mu::depress_cubic(b, c, d);
            o.f(p);
            o.f(q);
            o.f(s);
            let (p, q, r, s) = mu::depress_quartic(b, c, d, e);
            o.f(p);
            o.f(q);
            o.f(r);
            o.f(s);
        }
        "domain1d" => {
            let (mn, mx, u, tol, thr, per, mn2, mx2, lim) =
                (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let dflt = mu::Domain1D::default();
            let d = mu::Domain1D::new(mn, mx);
            let other = mu::Domain1D::new(mn2, mx2);
            o.f(dflt.min);
            o.f(dflt.max);
            o.f(d.length());
            o.f(d.mid());
            o.b(d.contains(u, 0.0));
            o.b(d.contains(u, tol));
            o.f(d.clamp(u));
            o.b(d.is_large(1000.0));
            o.b(d.is_large(thr));
            o.b(d.is_full_period(per, 1e-10));
            o.b(d.is_full_period(per, tol));
            o.f(d.lerp(u));
            o.f(d.normalize(u));
            o.b(d.is_finite(lim));
            o.b(d.is_finite(thr));
            o.b(d.is_equal(&other, 1e-10));
            o.b(d.is_equal(&other, tol));
        }
        "domain2d" => {
            let (u0, u1, v0, v1, u, v, tol, thr, per) =
                (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let dflt = mu::Domain2D::default();
            let d = mu::Domain2D::new(u0, u1, v0, v1);
            let e = mu::Domain2D::from_domains(&mu::Domain1D::new(u0, u1), &mu::Domain1D::new(v0, v1));
            o.f(dflt.umin);
            o.f(dflt.vmax);
            o.f(d.u_length());
            o.f(d.v_length());
            o.f(d.u_mid());
            o.f(d.v_mid());
            o.b(d.contains(u, v, 0.0));
            o.b(d.contains(u, v, tol));
            let (cu, cv) = d.clamp(u, v);
            o.f(cu);
            o.f(cv);
            o.b(d.is_large(1000.0));
            o.b(d.is_large(thr));
            o.b(d.is_u_full_period(per, 1e-10));
            o.b(d.is_u_full_period(per, tol));
            o.b(d.is_v_full_period(per, 1e-10));
            o.b(d.is_v_full_period(per, tol));
            o.b(d.is_finite(1e100));
            o.b(d.is_finite(thr));
            o.f(d.u().min);
            o.f(d.u().max);
            o.f(d.v().min);
            o.f(d.v().max);
            o.f(e.umin);
            o.f(e.umax);
            o.f(e.vmin);
            o.f(e.vmax);
        }
        "central_difference" | "forward_difference" | "second_derivative" => {
            let mut t = Ts::read(&mut rd);
            let x = rd.f();
            let fx = if family == "central_difference" { 0.0 } else { rd.f() };
            let step = rd.f();
            rd.finish();
            let call = |t: &mut Ts, step: f64| match family {
                "central_difference" => mu::central_difference(t, x, step),
                "forward_difference" => mu::forward_difference(t, x, fx, step),
                _ => mu::second_derivative(t, x, fx, step),
            };
            let default_step = if family == "second_derivative" { 1e-5 } else { 1e-8 };
            let r = call(&mut t, step);
            o.b(r.is_some());
            o.f(r.unwrap_or(0.0));
            o.calls(t.calls, t.hash);
            let mut t2 = t.fresh();
            let r2 = call(&mut t2, default_step);
            o.b(r2.is_some());
            o.f(r2.unwrap_or(0.0));
        }
        "numerical_gradient" | "numerical_gradient_adaptive" => {
            let mut t = Tv::read(&mut rd);
            let (mut x, step) = (rd.vec(), rd.f());
            rd.finish();
            let mut g = Vector::filled(x.lower(), x.upper(), 0.0)?;
            let ok = if family == "numerical_gradient" {
                mu::numerical_gradient(&mut t, &mut x, &mut g, step)?
            } else {
                mu::numerical_gradient_adaptive(&mut t, &mut x, &mut g, step)?
            };
            o.b(ok);
            o.vec(&g);
            o.vec(&x);
            o.calls(t.calls, t.hash);
        }
        "numerical_jacobian" => {
            let (m, failx, mut x, step) = (rd.i(), rd.f(), rd.vec(), rd.f());
            rd.finish();
            let mut t = Tsys { m, failx, calls: 0, hash: H0 };
            let mut jac = Matrix::filled(1, m, 1, x.length(), 0.0)?;
            let ok = mu::numerical_jacobian(&mut t, &mut x, &mut jac, step)?;
            o.b(ok);
            o.mat(&jac);
            o.vec(&x);
            o.calls(t.calls, t.hash);
        }
        "numerical_hessian" => {
            let mut t = Tv::read(&mut rd);
            let (mut x, step) = (rd.vec(), rd.f());
            rd.finish();
            let n = x.length();
            let mut h = Matrix::filled(1, n, 1, n, 0.0)?;
            let ok = mu::numerical_hessian(&mut t, &mut x, &mut h, step)?;
            o.b(ok);
            o.mat(&h);
            o.vec(&x);
            o.calls(t.calls, t.hash);
        }
        "bracket_root" => {
            let mut t = Ts::read(&mut rd);
            let (a, b, mi) = (rd.f(), rd.f(), rd.i());
            rd.finish();
            let r = mu::bracket_root(&mut t, a, b, mi);
            o.b(r.is_valid);
            o.f(r.a);
            o.f(r.b);
            o.f(r.fa);
            o.f(r.fb);
            o.calls(t.calls, t.hash);
            let r2 = mu::bracket_root(&mut t, a, b, 50);
            o.b(r2.is_valid);
            o.f(r2.a);
            o.f(r2.b);
            o.calls(t.calls, t.hash);
        }
        "bracket_minimum" | "bracket_minimum_iter" => {
            let mut t = Ts::read(&mut rd);
            let (a, b) = (rd.f(), rd.f());
            let r = if family == "bracket_minimum" {
                let opt = mu::MinBracketOptions {
                    max_iterations: rd.i(),
                    use_limits: rd.i() != 0,
                    left_limit: rd.f(),
                    right_limit: rd.f(),
                    has_fa: rd.i() != 0,
                    has_fb: rd.i() != 0,
                    fa: rd.f(),
                    fb: rd.f(),
                };
                rd.finish();
                mu::bracket_minimum(&mut t, a, b, &opt)
            } else {
                let mi = rd.i();
                rd.finish();
                mu::bracket_minimum_iter(&mut t, a, b, mi)
            };
            o.b(r.is_valid);
            for v in [r.a, r.b, r.c, r.fa, r.fb, r.fc] {
                o.f(v);
            }
            o.calls(t.calls, t.hash);
        }
        "armijo_backtrack" | "wolfe_search" => {
            let mut t = Tv::read(&mut rd);
            let (x, dir, g, fx, ai, c1, c2, mi) =
                (rd.vec(), rd.vec(), rd.vec(), rd.f(), rd.f(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let search = |t: &mut Tv, ai: f64, c1: f64, c2: f64, mi: i32| -> Result<mu::LineSearchResult, MathError> {
                if family == "armijo_backtrack" {
                    mu::armijo_backtrack(t, &x, &dir, &g, fx, ai, c1, c2, mi)
                } else {
                    mu::wolfe_search(t, &x, &dir, &g, fx, ai, c1, c2, mi)
                }
            };
            let r = search(&mut t, ai, c1, c2, mi)?;
            o.b(r.is_valid);
            o.f(r.alpha);
            o.f(r.f_new);
            o.i(r.nb_evals.into());
            o.calls(t.calls, t.hash);
            if family == "wolfe_search" {
                o.i(t.gcalls.into());
            }
            let (d1, d2, dm) = if family == "armijo_backtrack" { (1e-4, 0.5, 50) } else { (1e-4, 0.9, 20) };
            let r2 = search(&mut t, 1.0, d1, d2, dm)?;
            o.b(r2.is_valid);
            o.f(r2.alpha);
            o.f(r2.f_new);
            o.i(r2.nb_evals.into());
            o.calls(t.calls, t.hash);
            if family == "wolfe_search" {
                o.i(t.gcalls.into());
            }
        }
        "exact_line_search" => {
            let mut t = Tv::read(&mut rd);
            let (x, dir, am, tol, mi) = (rd.vec(), rd.vec(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let r = mu::exact_line_search(&mut t, &x, &dir, am, tol, mi)?;
            o.b(r.is_valid);
            o.f(r.alpha);
            o.f(r.f_new);
            o.i(r.nb_evals.into());
            o.calls(t.calls, t.hash);
            let r2 = mu::exact_line_search(&mut t, &x, &dir, 10.0, 1e-6, 100)?;
            o.b(r2.is_valid);
            o.f(r2.alpha);
            o.f(r2.f_new);
            o.i(r2.nb_evals.into());
            o.calls(t.calls, t.hash);
        }
        "quadratic_interpolation" => {
            let (p0, dp, a1, p1) = (rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            o.f(mu::quadratic_interpolation(p0, dp, a1, p1));
        }
        "brent_along_coordinate" => {
            let mut t = Tv::read(&mut rd);
            let (mut p, idx, lo, up, mut fx, tol, mi, mut ec) =
                (rd.vec(), rd.i(), rd.f(), rd.f(), rd.f(), rd.f(), rd.i(), rd.i());
            rd.finish();
            let ok = mu::brent_along_coordinate(&mut t, &mut p, idx, lo, up, &mut fx, tol, mi, &mut ec)?;
            o.b(ok);
            o.vec(&p);
            o.f(fx);
            o.i(ec.into());
            o.calls(t.calls, t.hash);
        }
        "scalar_functor" => {
            let (kind, p, q, s, x, idx) = (rd.i(), rd.vec(), rd.vec(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let poly = || mu::Polynomial::new(&p);
            let (q0, q1) = (q.value(q.lower()).unwrap(), q.value(q.lower() + 1).unwrap_or(0.0));
            let lin = || mu::Linear::new(q0, q1);
            let (mut y, mut dy) = (3.0, 3.0);
            let ok;
            let (mut ok2, mut has_v) = (false, false);
            let put = |r: Option<f64>, y: &mut f64| {
                if let Some(v) = r {
                    *y = v;
                }
                r.is_some()
            };
            match kind {
                0 => {
                    let mut f = poly();
                    ok = put(f.value(x), &mut y);
                    if let Some((a, b)) = mu::ScalarFunctionWithDerivative::values(&mut f, x) {
                        (y, dy, ok2) = (a, b, true);
                    }
                    has_v = true;
                    o.i(f.degree().into());
                    o.f(f.coefficient(idx));
                }
                1 => ok = put(mu::Rational::new(&p, &q).value(x), &mut y),
                2 => {
                    let mut f = mu::Constant::new(s);
                    ok = put(f.value(x), &mut y);
                    if let Some((a, b)) = mu::ScalarFunctionWithDerivative::values(&mut f, x) {
                        (y, dy, ok2) = (a, b, true);
                    }
                    has_v = true;
                }
                3 => {
                    let mut f = lin();
                    ok = put(f.value(x), &mut y);
                    if let Some((a, b)) = mu::ScalarFunctionWithDerivative::values(&mut f, x) {
                        (y, dy, ok2) = (a, b, true);
                    }
                    has_v = true;
                }
                4 => ok = put(mu::make_composite(poly(), lin()).value(x), &mut y),
                5 => ok = put(mu::make_sum(poly(), lin()).value(x), &mut y),
                6 => ok = put(mu::make_difference(poly(), lin()).value(x), &mut y),
                7 => ok = put(mu::make_product(poly(), lin()).value(x), &mut y),
                8 => ok = put(mu::make_quotient(poly(), lin()).value(x), &mut y),
                9 => ok = put(mu::make_scaled(poly(), s).value(x), &mut y),
                10 => ok = put(mu::make_shifted(poly(), s).value(x), &mut y),
                11 => ok = put(mu::make_negated(poly()).value(x), &mut y),
                12 => {
                    let mut f = mu::make_scalar(|t: f64| if t < 0.0 { None } else { Some(s * t * t + q0) });
                    ok = put(f.value(x), &mut y);
                }
                _ => {
                    let mut f = mu::make_scalar_with_derivative(|t: f64| {
                        if t < 0.0 { None } else { Some((s * t * t + q0, 2.0 * s * t)) }
                    });
                    ok = put(f.value(x), &mut y);
                    has_v = true;
                    let (mut y2, mut d2) = (5.0, 5.0);
                    if let Some((a, b)) = mu::ScalarFunctionWithDerivative::values(&mut f, x) {
                        (y2, d2, ok2) = (a, b, true);
                    }
                    dy = d2;
                    o.f(y2);
                }
            }
            o.b(ok);
            o.f(y);
            o.b(has_v);
            if has_v {
                o.b(ok2);
                o.f(dy);
            }
        }
        "scalar_functor_libm" => {
            let (kind, p1, p2, p3, p4, x, _half) = (rd.i(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let (mut y, mut dy) = (3.0, 3.0);
            let (mut ok, mut ok2) = (false, false);
            macro_rules! both {
                ($f:expr) => {{
                    let mut f = $f;
                    if let Some(v) = f.value(x) {
                        y = v;
                        ok = true;
                    }
                    if let Some((a, b)) = mu::ScalarFunctionWithDerivative::values(&mut f, x) {
                        (y, dy, ok2) = (a, b, true);
                    }
                }};
            }
            match kind {
                0 => both!(mu::Sine::new(p1, p2, p3, p4)),
                1 => both!(mu::Cosine::new(p1, p2, p3, p4)),
                2 => both!(mu::Exponential::new(p1, p2, p3)),
                3 => both!(mu::Power::new(p1, p2, p3)),
                _ => both!(mu::Gaussian::new(p1, p2, p3)),
            }
            o.b(ok);
            o.f(y);
            o.b(ok2);
            o.f(dy);
            let (mut a, mut b) = (0.0, 0.0);
            a = mu::Sine::default().value(x).unwrap_or(a);
            b = mu::Cosine::default().value(x).unwrap_or(b);
            o.f(a);
            o.f(b);
            a = mu::Exponential::default().value(x).unwrap_or(a);
            b = mu::Gaussian::default().value(x).unwrap_or(b);
            o.f(a);
            o.f(b);
        }
        "vector_functor" => {
            let (kind, pa, pb, a, b, c, x) = (rd.i(), rd.f(), rd.f(), rd.mat(), rd.vec(), rd.f(), rd.vec());
            rd.finish();
            let l = x.lower();
            let mut g = Vector::filled(l, x.upper(), 0.0)?;
            let mut g2 = Vector::filled(l, x.upper(), 0.0)?;
            let (mut y, mut y2) = (3.0, 4.0);
            macro_rules! all {
                ($f:expr) => {{
                    let mut f = $f;
                    let ok = if let Some(v) = f.value(&x) {
                        y = v;
                        true
                    } else {
                        false
                    };
                    let ok2 = f.gradient(&x, &mut g);
                    let ok3 = f.values(&x, &mut y2, &mut g2);
                    (ok, ok2, ok3)
                }};
            }
            let (ok, ok2, ok3) = match kind {
                0 => all!(mu::QuadraticForm::new(&a, &b, c)),
                1 => all!(mu::Rosenbrock::new(pa, pb)),
                2 => all!(mu::Sphere),
                3 => all!(mu::Booth),
                4 => all!(mu::Beale),
                5 => all!(mu::Himmelblau),
                _ => all!(mu::LinearResidual::new(&a, &b)),
            };
            o.b(ok);
            o.f(y);
            o.b(ok2);
            o.vec(&g);
            o.b(ok3);
            o.f(y2);
            o.vec(&g2);
        }
        "vector_functor_libm" => {
            let (kind, pa, pb, pc, x) = (rd.i(), rd.f(), rd.f(), rd.f(), rd.vec());
            rd.finish();
            let mut g = Vector::filled(x.lower(), x.upper(), 0.0)?;
            let mut y = 3.0;
            let (mut ok, mut ok2) = (false, false);
            let mut yd = 0.0;
            if kind == 0 {
                let mut f = mu::Rastrigin::new(pa);
                if let Some(v) = f.value(&x) {
                    y = v;
                    ok = true;
                }
                ok2 = f.gradient(&x, &mut g);
                yd = mu::Rastrigin::default().value(&x).unwrap_or(yd);
            } else {
                let mut f = mu::Ackley::new(pa, pb, pc);
                if let Some(v) = f.value(&x) {
                    y = v;
                    ok = true;
                }
                yd = mu::Ackley::default().value(&x).unwrap_or(yd);
            }
            o.f(yd);
            o.b(ok);
            o.f(y);
            o.b(ok2);
            o.vec(&g);
        }
        "vector_lambda" => {
            let (m, k, x) = (rd.i(), rd.vec(), rd.vec());
            rd.finish();
            let (l, u) = (x.lower(), x.upper());
            let val = |t: &Vector| -> Option<f64> {
                if t.value(t.lower()).unwrap() < -1.0 {
                    return None;
                }
                let mut y = 0.0;
                for i in t.lower()..=t.upper() {
                    y += k.value(i).unwrap() * t.value(i).unwrap() * t.value(i).unwrap();
                }
                Some(y)
            };
            let grad = |t: &Vector, g: &mut Vector| -> bool {
                for i in t.lower()..=t.upper() {
                    g.set_value(i, 2.0 * k.value(i).unwrap() * t.value(i).unwrap()).unwrap();
                }
                true
            };
            let mut vl = mu::make_vector(val);
            let mut vg = mu::make_vector_with_gradient(val, grad);
            let (mut y, mut y2, mut y3) = (3.0, 3.0, 3.0);
            let mut g = Vector::filled(l, u, 0.0)?;
            let mut g2 = Vector::filled(l, u, 0.0)?;
            let ok = vl.value(&x).map(|v| y = v).is_some();
            let ok2 = vg.value(&x).map(|v| y2 = v).is_some();
            let ok3 = vg.gradient(&x, &mut g);
            let ok4 = vg.values(&x, &mut y3, &mut g2);
            o.b(ok);
            o.f(y);
            o.b(ok2);
            o.f(y2);
            o.b(ok3);
            o.vec(&g);
            o.b(ok4);
            o.f(y3);
            o.vec(&g2);
            let k0 = k.value(k.lower()).unwrap();
            let mut sys = mu::make_system(
                |t: &Vector, f: &mut Vector| {
                    let t0 = t.value(t.lower()).unwrap();
                    if t0 < -1.0 {
                        return false;
                    }
                    for i in 1..=m {
                        f.set_value(i, f64::from(i) * t0 + k0).unwrap();
                    }
                    true
                },
                m,
            );
            let mut f = Vector::filled(1, m, 0.0)?;
            let ok5 = sys.value(&x, &mut f);
            o.i(sys.nb_equations().into());
            o.b(ok5);
            o.vec(&f);
        }
        "gauss_points_weights" | "ordered_gauss_points_weights" | "kronrod_points_weights" => {
            let (order, np, nw, lp, lw) = (rd.i(), rd.i(), rd.i(), rd.i(), rd.i());
            rd.finish();
            let mut p = Vector::filled(lp, lp + np - 1, 0.0)?;
            let mut w = Vector::filled(lw, lw + nw - 1, 0.0)?;
            let ok = match family {
                "gauss_points_weights" => mu::get_gauss_points_and_weights(order, &mut p, &mut w),
                "ordered_gauss_points_weights" => mu::get_ordered_gauss_points_and_weights(order, &mut p, &mut w),
                _ => mu::get_kronrod_points_and_weights(order, &mut p, &mut w),
            };
            o.b(ok);
            o.vec(&p);
            o.vec(&w);
        }
        _ => panic!("unknown family {family}"),
    }
    Ok(o.0)
}

fn status(k: i32) -> Status {
    [
        Status::Ok,
        Status::NotConverged,
        Status::MaxIterations,
        Status::NumericalError,
        Status::InvalidInput,
        Status::InfiniteSolutions,
        Status::NoSolution,
        Status::NotPositiveDefinite,
        Status::Singular,
        Status::NonDescentDirection,
    ][k as usize]
}

fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let q = x.to_bits() as i64;
        if q < 0 { i64::MIN - q } else { q }
    };
    key(a).abs_diff(key(b))
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
    let libm = LIBM.contains(&family);
    let mut scale = 1.0_f64;
    if libm {
        for (g, w) in got.iter().zip(want) {
            if let Tok::F(_) = g {
                let v = f64::from_bits(u64::from_str_radix(w, 16).unwrap()).abs();
                if v.is_finite() {
                    scale = scale.max(v);
                }
            }
        }
    }
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
                if libm {
                    let tol = MAX_ULPS as f64 * f64::EPSILON * scale;
                    assert!(
                        ulps(*v, e) <= MAX_ULPS || (v - e).abs() <= tol,
                        "{family} output {k}: {v:e} != {e:e}\n{line}"
                    );
                } else {
                    assert_eq!(v.to_bits(), e.to_bits(), "{family} output {k}: {v:e} != {e:e}\n{line}");
                }
            }
        }
    }
}

#[test]
fn mathutils_matches_occt() {
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
    assert!(throws >= 2, "the oracle must exercise `throws`");
    println!(
        "mathutils: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
