//! Differential judge for OCCT's header-only `MathSys` package (OCCT 8.0.1): replays `tests/mathsys.txt`, printed by
//! `oracle/mathsys.cpp` against the real library. See `docs/api-mathsys.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::gp::{Pnt, Vec as Vec3};
use occt_port::mathlin::{MathError, Matrix, Vector};
use occt_port::mathnd::{FunctionSet, FunctionSetWithDerivatives};
use occt_port::mathsys::{self as ms, LmConfig, NewtonBoundsN, NewtonOptions, NewtonResultN};
use occt_port::mathutils::{Config, Status, VectorResult};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("mathsys.txt");
const GOLDEN_LINES: usize = 260;
const MIN_DISTINCT: usize = 4;
const FAMILIES: &[&str] = &[
    "clamp_nd",
    "levenberg_marquardt",
    "levenberg_marquardt_bounded",
    "newton",
    "newton_bounded",
    "newton_scalar_tol",
    "options_validity",
    "solve2d",
    "solve2d_symmetric",
    "solve3d",
    "solve3x3",
    "solve4d",
    "solve4x4",
    "solve_curve_surface_extrema_3d",
    "solve_surface_surface_extrema_4d",
    "solve_symmetric_2x2_svd",
    "types",
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
    fn arr<const N: usize>(&mut self) -> [f64; N] {
        let mut a = [0.0; N];
        for v in &mut a {
            *v = self.f();
        }
        a
    }
    fn opts(&mut self) -> NewtonOptions {
        let (tol, xtol, ftol, mi) = (self.f(), self.f(), self.f(), self.i());
        NewtonOptions {
            base: Config {
                tolerance: tol,
                x_tolerance: xtol,
                f_tolerance: ftol,
                max_iterations: mi,
                ..Config::default()
            },
            max_step_ratio: self.f(),
            enable_line_search: self.i() != 0,
            allow_soft_bounds: self.i() != 0,
            soft_bounds_extension: self.f(),
        }
    }
    fn bounds<const N: usize>(&mut self) -> NewtonBoundsN<N> {
        let mut b = NewtonBoundsN::<N>::default();
        for i in 0..N {
            b.min[i] = self.f();
            b.max[i] = self.f();
        }
        b.has_bounds = self.i() != 0;
        b
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
    fn vector_result(&mut self, r: &VectorResult, t: &Tsys) {
        self.i(r.status as i64);
        self.i(r.nb_iterations as i64);
        self.b(r.solution.is_some());
        if let Some(v) = &r.solution {
            self.vec(v);
        }
        self.b(r.value.is_some());
        if let Some(v) = r.value {
            self.f(v);
        }
        self.b(r.gradient.is_some());
        if let Some(v) = &r.gradient {
            self.vec(v);
        }
        self.b(r.jacobian.is_some());
        if let Some(m) = &r.jacobian {
            self.mat(m);
        }
        self.i(t.vcalls.into());
        self.i(t.dcalls.into());
        self.i(t.vscalls.into());
        self.u(t.hash);
    }
    fn newton_n<const N: usize>(&mut self, r: &NewtonResultN<N>, calls: i32, hash: u64) {
        self.i(r.status as i64);
        for v in r.x {
            self.f(v);
        }
        self.i(r.nb_iterations as i64);
        self.f(r.residual_norm);
        self.f(r.step_norm);
        self.i(calls.into());
        self.u(hash);
    }
}

// ---- callbacks (identical to the oracle's) -----------------------------------------------------------------
fn mixh(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x100000001b3)
}
const H0: u64 = 0xcbf29ce484222325;

struct Tsys {
    n: i32,
    m: i32,
    c: Matrix,
    b: Vector,
    c3: f64,
    fail_hi: f64,
    fail_lo: f64,
    vcalls: i32,
    dcalls: i32,
    vscalls: i32,
    hash: u64,
}
impl Tsys {
    fn read(rd: &mut Rd) -> Self {
        let (n, m, c3, fail_hi, fail_lo) = (rd.i(), rd.i(), rd.f(), rd.f(), rd.f());
        let (c, b) = (rd.mat(), rd.vec());
        Self { n, m, c, b, c3, fail_hi, fail_lo, vcalls: 0, dcalls: 0, vscalls: 0, hash: H0 }
    }
    fn mixx(&mut self, x: &Vector, salt: u64) {
        self.hash = mixh(self.hash, salt);
        for i in x.lower()..=x.upper() {
            self.hash = mixh(self.hash, x.value(i).unwrap().to_bits());
        }
    }
    fn fill(&self, x: &Vector, mut f: Option<&mut Vector>, mut d: Option<&mut Matrix>) {
        for k in 0..self.m {
            if let Some(f) = f.as_deref_mut() {
                let mut s = 0.0;
                for j in 0..self.n {
                    s += self.c.value(self.c.lower_row() + k, self.c.lower_col() + j).unwrap()
                        * x.value(x.lower() + j).unwrap();
                }
                let xc = x.value(x.lower() + k % self.n).unwrap();
                f.set_value(f.lower() + k, s + self.c3 * xc * xc * xc - self.b.value(self.b.lower() + k).unwrap())
                    .unwrap();
            }
            if let Some(d) = d.as_deref_mut() {
                for j in 0..self.n {
                    let xj = x.value(x.lower() + j).unwrap();
                    let cj = self.c.value(self.c.lower_row() + k, self.c.lower_col() + j).unwrap();
                    let extra = if j == k % self.n { 3.0 * self.c3 * xj * xj } else { 0.0 };
                    d.set_value(d.lower_row() + k, d.lower_col() + j, cj + extra).unwrap();
                }
            }
        }
    }
}
impl FunctionSet for Tsys {
    fn nb_variables(&self) -> i32 {
        self.n
    }
    fn nb_equations(&self) -> i32 {
        self.m
    }
    fn value(&mut self, x: &Vector, f: &mut Vector) -> bool {
        self.vcalls += 1;
        self.mixx(x, 1);
        if x.value(x.lower()).unwrap() > self.fail_hi {
            return false;
        }
        self.fill(x, Some(f), None);
        true
    }
}
impl FunctionSetWithDerivatives for Tsys {
    fn derivatives(&mut self, x: &Vector, d: &mut Matrix) -> bool {
        self.dcalls += 1;
        self.mixx(x, 2);
        let x0 = x.value(x.lower()).unwrap();
        if x0 > self.fail_hi || x0 < self.fail_lo {
            return false;
        }
        self.fill(x, None, Some(d));
        true
    }
    fn values(&mut self, x: &Vector, f: &mut Vector, d: &mut Matrix) -> bool {
        self.vscalls += 1;
        self.mixx(x, 3);
        let x0 = x.value(x.lower()).unwrap();
        if x0 > self.fail_hi || x0 < self.fail_lo {
            return false;
        }
        self.fill(x, Some(f), Some(d));
        true
    }
}

struct Tn<const N: usize> {
    a: [[f64; N]; N],
    c: [f64; N],
    b: [f64; N],
    failx: f64,
    calls: i32,
    hash: u64,
}
impl<const N: usize> Tn<N> {
    fn read(rd: &mut Rd) -> Self {
        let mut a = [[0.0; N]; N];
        for row in &mut a {
            *row = rd.arr::<N>();
        }
        let (c, b, failx) = (rd.arr::<N>(), rd.arr::<N>(), rd.f());
        Self { a, c, b, failx, calls: 0, hash: H0 }
    }
    fn eval_n(&mut self, x: &[f64; N], f: &mut [f64; N], j: &mut [[f64; N]; N]) -> bool {
        self.calls += 1;
        for v in x {
            self.hash = mixh(self.hash, v.to_bits());
        }
        if x[0] > self.failx {
            return false;
        }
        for k in 0..N {
            let mut s = 0.0;
            for (a, xv) in self.a[k].iter().zip(x) {
                s += a * xv;
            }
            f[k] = s + self.c[k] * x[k] * x[k] - self.b[k];
            for (jj, (jv, a)) in j[k].iter_mut().zip(&self.a[k]).enumerate() {
                *jv = a + if jj == k { 2.0 * self.c[k] * x[k] } else { 0.0 };
            }
        }
        true
    }
}
impl ms::Function2D for Tn<2> {
    fn eval(&mut self, x: f64, y: f64, f: &mut [f64; 2], j: &mut [[f64; 2]; 2]) -> bool {
        self.eval_n(&[x, y], f, j)
    }
}
impl ms::Function3D for Tn<3> {
    fn eval(&mut self, x: f64, y: f64, z: f64, f: &mut [f64; 3], j: &mut [[f64; 3]; 3]) -> bool {
        self.eval_n(&[x, y, z], f, j)
    }
}
impl ms::Function4D for Tn<4> {
    fn eval(&mut self, x0: f64, x1: f64, x2: f64, x3: f64, f: &mut [f64; 4], j: &mut [[f64; 4]; 4]) -> bool {
        self.eval_n(&[x0, x1, x2, x3], f, j)
    }
}

struct Tsym {
    h11: f64,
    h12: f64,
    h22: f64,
    c: f64,
    b1: f64,
    b2: f64,
    failx: f64,
    calls: i32,
    hash: u64,
}
impl ms::SymmetricFunction2D for Tsym {
    fn value(&mut self, u: f64, v: f64) -> Option<(f64, f64)> {
        self.calls += 1;
        self.hash = mixh(self.hash, u.to_bits() ^ 0x11);
        self.hash = mixh(self.hash, v.to_bits());
        if u > self.failx {
            return None;
        }
        Some((
            self.h11 * u + self.h12 * v + self.c * u * u * u - self.b1,
            self.h12 * u + self.h22 * v + self.c * v * v * v - self.b2,
        ))
    }
    fn value_and_jacobian(&mut self, u: f64, v: f64) -> Option<(f64, f64, f64, f64, f64)> {
        self.calls += 1;
        self.hash = mixh(self.hash, u.to_bits() ^ 0x22);
        self.hash = mixh(self.hash, v.to_bits());
        if u > self.failx {
            return None;
        }
        Some((
            self.h11 * u + self.h12 * v + self.c * u * u * u - self.b1,
            self.h12 * u + self.h22 * v + self.c * v * v * v - self.b2,
            self.h11 + 3.0 * self.c * u * u,
            self.h12,
            self.h22 + 3.0 * self.c * v * v,
        ))
    }
}

struct Tcurve {
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
}
impl ms::CurveEvaluator for Tcurve {
    fn d2(&self, t: f64) -> (Pnt, Vec3, Vec3) {
        let (p, a, b, c) = (&self.p, &self.a, &self.b, &self.c);
        (
            Pnt::new(
                p[0] + t * a[0] + t * t * b[0] + t * t * t * c[0],
                p[1] + t * a[1] + t * t * b[1] + t * t * t * c[1],
                p[2] + t * a[2] + t * t * b[2] + t * t * t * c[2],
            ),
            Vec3::new(
                a[0] + 2.0 * t * b[0] + 3.0 * t * t * c[0],
                a[1] + 2.0 * t * b[1] + 3.0 * t * t * c[1],
                a[2] + 2.0 * t * b[2] + 3.0 * t * t * c[2],
            ),
            Vec3::new(2.0 * b[0] + 6.0 * t * c[0], 2.0 * b[1] + 6.0 * t * c[1], 2.0 * b[2] + 6.0 * t * c[2]),
        )
    }
}
struct Tsurf {
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    d: [f64; 3],
    e: [f64; 3],
}
impl Tsurf {
    fn read(rd: &mut Rd) -> Self {
        Self { p: rd.arr(), a: rd.arr(), b: rd.arr(), c: rd.arr(), d: rd.arr(), e: rd.arr() }
    }
}
impl ms::SurfaceEvaluator for Tsurf {
    fn d2(&self, u: f64, v: f64) -> (Pnt, Vec3, Vec3, Vec3, Vec3, Vec3) {
        let mut q = [0.0; 3];
        let mut du = [0.0; 3];
        let mut dv = [0.0; 3];
        let mut duu = [0.0; 3];
        let mut dvv = [0.0; 3];
        let mut duv = [0.0; 3];
        for i in 0..3 {
            q[i] =
                self.p[i] + u * self.a[i] + v * self.b[i] + u * u * self.c[i] + v * v * self.d[i] + u * v * self.e[i];
            du[i] = self.a[i] + 2.0 * u * self.c[i] + v * self.e[i];
            dv[i] = self.b[i] + 2.0 * v * self.d[i] + u * self.e[i];
            duu[i] = 2.0 * self.c[i];
            dvv[i] = 2.0 * self.d[i];
            duv[i] = self.e[i];
        }
        (
            Pnt::new(q[0], q[1], q[2]),
            Vec3::new(du[0], du[1], du[2]),
            Vec3::new(dv[0], dv[1], dv[2]),
            Vec3::new(duu[0], duu[1], duu[2]),
            Vec3::new(dvv[0], dvv[1], dvv[2]),
            Vec3::new(duv[0], duv[1], duv[2]),
        )
    }
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

/// Replays one record; `Err` stands for a C++ exception (`throws`).
fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, MathError> {
    let mut rd = Rd::new(words);
    let mut o = Out::default();
    match family {
        "newton" => {
            let mut t = Tsys::read(&mut rd);
            let (x0, tolx, tol_f, mi) = (rd.vec(), rd.vec(), rd.f(), rd.i());
            rd.finish();
            let r = ms::newton(&mut t, &x0, &tolx, tol_f, mi as usize)?;
            o.vector_result(&r, &t);
        }
        "newton_scalar_tol" => {
            let mut t = Tsys::read(&mut rd);
            let (x0, tolx, tol_f, mi) = (rd.vec(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let r = ms::newton_scalar_tol(&mut t, &x0, tolx, tol_f, mi as usize)?;
            o.vector_result(&r, &t);
        }
        "newton_bounded" => {
            let mut t = Tsys::read(&mut rd);
            let (x0, inf, sup, tolx, tol_f, mi) = (rd.vec(), rd.vec(), rd.vec(), rd.vec(), rd.f(), rd.i());
            rd.finish();
            let r = ms::newton_bounded(&mut t, &x0, &inf, &sup, &tolx, tol_f, mi as usize)?;
            o.vector_result(&r, &t);
        }
        "levenberg_marquardt" | "levenberg_marquardt_bounded" => {
            let mut t = Tsys::read(&mut rd);
            let x0 = rd.vec();
            let bounds = if family == "levenberg_marquardt_bounded" { Some((rd.vec(), rd.vec())) } else { None };
            let (tol, xtol, mi) = (rd.f(), rd.f(), rd.i());
            let mut c = LmConfig::new(tol, mi);
            c.base.x_tolerance = xtol;
            c.lambda_init = rd.f();
            c.lambda_increase = rd.f();
            c.lambda_decrease = rd.f();
            c.lambda_max = rd.f();
            c.lambda_min = rd.f();
            rd.finish();
            let r = match &bounds {
                Some((inf, sup)) => ms::levenberg_marquardt_bounded(&mut t, &x0, inf, sup, &c)?,
                None => ms::levenberg_marquardt(&mut t, &x0, &c)?,
            };
            o.vector_result(&r, &t);
        }
        "solve2d" => {
            let mut t = Tn::<2>::read(&mut rd);
            let (x0, bd, op) = (rd.arr::<2>(), rd.bounds::<2>(), rd.opts());
            rd.finish();
            let r = ms::solve2d(&mut t, &x0, &bd, &op);
            o.newton_n(&r, t.calls, t.hash);
        }
        "solve3d" => {
            let mut t = Tn::<3>::read(&mut rd);
            let (x0, bd, op) = (rd.arr::<3>(), rd.bounds::<3>(), rd.opts());
            rd.finish();
            let r = ms::solve3d(&mut t, &x0, &bd, &op);
            o.newton_n(&r, t.calls, t.hash);
        }
        "solve4d" => {
            let mut t = Tn::<4>::read(&mut rd);
            let (x0, bd, op) = (rd.arr::<4>(), rd.bounds::<4>(), rd.opts());
            rd.finish();
            let r = ms::solve4d(&mut t, &x0, &bd, &op);
            o.newton_n(&r, t.calls, t.hash);
        }
        "solve2d_symmetric" => {
            let mut t = Tsym {
                h11: rd.f(),
                h12: rd.f(),
                h22: rd.f(),
                c: rd.f(),
                b1: rd.f(),
                b2: rd.f(),
                failx: rd.f(),
                calls: 0,
                hash: H0,
            };
            let (x0, bd, op) = (rd.arr::<2>(), rd.bounds::<2>(), rd.opts());
            rd.finish();
            let r = ms::solve2d_symmetric(&mut t, &x0, &bd, &op);
            o.newton_n(&r, t.calls, t.hash);
        }
        "solve_curve_surface_extrema_3d" => {
            let curve = Tcurve { p: rd.arr(), a: rd.arr(), b: rd.arr(), c: rd.arr() };
            let surf = Tsurf::read(&mut rd);
            let (x0, bd, op) = (rd.arr::<3>(), rd.bounds::<3>(), rd.opts());
            rd.finish();
            let r = ms::solve_curve_surface_extrema_3d(&curve, &surf, &x0, &bd, &op);
            o.newton_n(&r, 0, 0);
        }
        "solve_surface_surface_extrema_4d" => {
            let s1 = Tsurf::read(&mut rd);
            let s2 = Tsurf::read(&mut rd);
            let (x0, bd, op) = (rd.arr::<4>(), rd.bounds::<4>(), rd.opts());
            rd.finish();
            let r = ms::solve_surface_surface_extrema_4d(&s1, &s2, &x0, &bd, &op);
            o.newton_n(&r, 0, 0);
        }
        "solve_symmetric_2x2_svd" => {
            let (j11, j12, j22, f1, f2, tol) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            for t in [tol, 1.0e-15] {
                let r = ms::solve_symmetric_2x2_svd(j11, j12, j22, f1, f2, t);
                o.b(r.is_some());
                let (du, dv) = r.unwrap_or((7.0, 7.0));
                o.f(du);
                o.f(dv);
            }
        }
        "solve3x3" => {
            let mut j = [[0.0; 3]; 3];
            for row in &mut j {
                *row = rd.arr::<3>();
            }
            let f = rd.arr::<3>();
            rd.finish();
            let r = ms::solve3x3(&j, &f);
            o.b(r.is_some());
            for v in r.unwrap_or([7.0; 3]) {
                o.f(v);
            }
        }
        "solve4x4" => {
            let mut j = [[0.0; 4]; 4];
            for row in &mut j {
                *row = rd.arr::<4>();
            }
            let f = rd.arr::<4>();
            rd.finish();
            let r = ms::solve4x4(&j, &f);
            o.b(r.is_some());
            for v in r.unwrap_or([7.0; 4]) {
                o.f(v);
            }
        }
        "clamp_nd" => {
            let x = rd.arr::<4>();
            let (b2, b3, b4) = (rd.bounds::<2>(), rd.bounds::<3>(), rd.bounds::<4>());
            let (soft, ext) = (rd.i() != 0, rd.f());
            rd.finish();
            let mut a2 = [x[0], x[1]];
            ms::clamp_2d(&mut a2, &b2, soft, ext);
            let mut a3 = [x[0], x[1], x[2]];
            ms::clamp_3d(&mut a3, &b3, soft, ext);
            let mut a4 = x;
            ms::clamp_4d(&mut a4, &b4, soft, ext);
            for v in a2.into_iter().chain(a3).chain(a4) {
                o.f(v);
            }
            o.f(ms::max_domain_size_2d(&b2));
            o.f(ms::max_domain_size_3d(&b3));
            o.f(ms::max_domain_size_4d(&b4));
            o.b(ms::is_bounds_valid_2d(&b2));
            o.b(ms::is_bounds_valid_3d(&b3));
            o.b(ms::is_bounds_valid_4d(&b4));
        }
        "options_validity" => {
            let op = rd.opts();
            let (b2, b3, b4) = (rd.bounds::<2>(), rd.bounds::<3>(), rd.bounds::<4>());
            rd.finish();
            o.b(ms::is_options_valid(&op));
            o.b(ms::is_options_valid_3d(&op));
            o.b(ms::is_options_valid_4d(&op));
            o.b(ms::is_bounds_valid_2d(&b2));
            o.b(ms::is_bounds_valid_3d(&b3));
            o.b(ms::is_bounds_valid_4d(&b4));
            o.f(ms::max_domain_size_2d(&b2));
            o.f(ms::max_domain_size_3d(&b3));
            o.f(ms::max_domain_size_4d(&b4));
            o.f(ms::THE_SINGULAR_DET_TOL);
            o.f(ms::THE_CRITICAL_GRAD_SQ);
            o.i(ms::THE_LINE_SEARCH_MAX.into());
            o.f(ms::THE_ARMIJO_C1);
        }
        "types" => {
            let j = rd.i();
            rd.finish();
            let opt = NewtonOptions::default();
            let c = LmConfig::default();
            let c2 = LmConfig::new(1e-6, 5 + j);
            let mut r2 = NewtonResultN::<2>::default();
            let r3 = NewtonResultN::<3>::default();
            let r4 = NewtonResultN::<4>::default();
            let (b2, b3, b4) =
                (NewtonBoundsN::<2>::default(), NewtonBoundsN::<3>::default(), NewtonBoundsN::<4>::default());
            o.f(opt.base.tolerance);
            o.f(opt.base.x_tolerance);
            o.f(opt.base.f_tolerance);
            o.i(opt.base.max_iterations.into());
            o.f(opt.base.step_min);
            o.f(opt.max_step_ratio);
            o.b(opt.enable_line_search);
            o.b(opt.allow_soft_bounds);
            o.f(opt.soft_bounds_extension);
            o.f(c.base.tolerance);
            o.f(c.base.x_tolerance);
            o.f(c.base.f_tolerance);
            o.i(c.base.max_iterations.into());
            o.f(c.base.step_min);
            o.f(c.lambda_init);
            o.f(c.lambda_increase);
            o.f(c.lambda_decrease);
            o.f(c.lambda_max);
            o.f(c.lambda_min);
            o.f(c2.base.tolerance);
            o.f(c2.base.x_tolerance);
            o.f(c2.base.f_tolerance);
            o.i(c2.base.max_iterations.into());
            o.f(c2.lambda_init);
            o.i(r2.status as i64);
            o.i(r2.nb_iterations as i64);
            o.f(r2.residual_norm);
            o.f(r2.step_norm);
            o.f(r2.x[0]);
            o.f(r2.x[1]);
            o.b(r2.is_done());
            o.i(r3.status as i64);
            o.f(r3.x[2]);
            o.i(r4.status as i64);
            o.f(r4.x[3]);
            o.f(b2.min[1]);
            o.f(b2.max[1]);
            o.b(b2.has_bounds);
            o.f(b3.min[2]);
            o.b(b3.has_bounds);
            o.f(b4.max[3]);
            o.b(b4.has_bounds);
            r2.status = status(if j % 2 == 1 { 0 } else { 3 });
            o.b(r2.is_done());
        }
        _ => panic!("unknown family {family}"),
    }
    Ok(o.0)
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
fn mathsys_matches_occt() {
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
        "mathsys: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
