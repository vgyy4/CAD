//! Numerical reference test for OCCT 8.0.1's global optimizers of the `math` package: `math_GlobOptMin`, `math_PSO`,
//! `math_PSOParticlesPool` and `math_BullardGenerator`.
//!
//! `mathglob.txt` is printed by `oracle/mathglob.cpp` (real OCCT). Each line is `<family> <inputs...> | <outputs...>`:
//! f64 values are 16-hex-digit bit patterns and ints (and bools) are decimal. Every input of a call is on the left of
//! ` | `; this test reads them back with the `Rd` reader methods of the same name as the oracle's schema words
//! (`f i v mv pool`, see `SCHEMAS`: every reader call is checked against the family's schema) and replays the call on
//! the Rust API of docs/api-mathglob.md. The right side is either the expected values or `unchecked` (a check that
//! OCCT 8.0.1 as built has compiled out would have failed, or the call would touch memory outside a container, and
//! the numbers are unspecified); on `unchecked` the Rust API must return an error.
//!
//! The optimizers call user functions. Those are the closed set of test functions described at the top of
//! `oracle/mathglob.cpp` (`Mv`: a function of several variables built from scalar terms), implemented here with the
//! same arithmetic (+ - * / floor and fabs only), so both sides see the same bits. `math_GlobOptMin` asks its function
//! with `dynamic_cast` what it can do: the `level` word of a line says whether the optimizer gets the function as a
//! plain value function (0), with gradient (1) or with Hessian (2). Besides the numbers, every construction line prints
//! the number of callback calls and a hash of the call trace (with the bounds and every element of each argument
//! vector): the Rust optimizers must make the same calls in the same order.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::mathglob::{BullardGenerator, GlobFunction, GlobOptMin, Pso, PsoParticlesPool};
use occt_port::mathlin::{MathError, Matrix, Vector};
use occt_port::mathnd::{MultipleVarFunction, MultipleVarFunctionWithGradient, MultipleVarFunctionWithHessian};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for results that are not computed with `+ - * /` and `sqrt` only. No family needs it:
/// everything here is compared bit for bit (see `EXACT`).
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once. There are none here.
const INPUT_LESS: &[&str] = &[];

/// Families whose results use only + - * / sqrt, floor and abs: compared bit for bit (signed zeros included). All of
/// them: neither `GlobOptMin`, `Pso` nor the local optimizers call a libm function (`pow(3, N)` is exact).
const EXACT: &[&str] = &[
    "bullard",
    "glob_access",
    "glob_global",
    "glob_new",
    "glob_perform",
    "glob_staged",
    "pso",
    "pso_pool",
    "pso_pool_access",
    "test_mv",
];

/// Every family the oracle must print, with its schema (the words of its inputs, in order). A family that disappears
/// from mathglob.txt fails; every reader call is checked against the schema.
const SCHEMAS: &[(&str, &str)] = &[
    ("bullard", "i i i i"),
    ("glob_access", "mv i v v f f f i i v f f i"),
    ("glob_global", "mv i v v f f f i f i i mv i v v f f f i"),
    ("glob_new", "mv i v v f f f"),
    ("glob_perform", "mv i v v f f f i f i f f i"),
    ("glob_staged", "mv i v v f f f i f i i v v i i"),
    ("pso", "mv v v v i i v i v"),
    ("pso_pool", "mv v v v i i pool i i v"),
    ("pso_pool_access", "pool i"),
    ("test_mv", "mv v"),
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 820;

// ---------------------------------------------------------------------------------
// The deterministic stand-ins for sin, cos and exp (the same as in oracle/mathnd.cpp)

const FNV0: u64 = 0xcbf2_9ce4_8422_2325;
const FNVP: u64 = 0x0000_0100_0000_01b3;

struct Tables {
    two_pi: f64,
    inv_two_pi: f64,
    sc: [f64; 16],
    cc: [f64; 16],
    ec: [f64; 13],
}

impl Tables {
    fn new() -> Self {
        let two_pi = 2.0 * std::f64::consts::PI;
        let inv_two_pi = 1.0 / two_pi;
        let mut sc = [0.0; 16];
        let mut cc = [0.0; 16];
        let mut ec = [0.0; 13];
        let mut f = 1.0_f64;
        for (j, s) in sc.iter_mut().enumerate() {
            *s = (if j % 2 == 1 { -1.0 } else { 1.0 }) / f;
            f *= (2 * j + 2) as f64 * (2 * j + 3) as f64;
        }
        f = 1.0;
        for (j, c) in cc.iter_mut().enumerate() {
            *c = (if j % 2 == 1 { -1.0 } else { 1.0 }) / f;
            f *= (2 * j + 1) as f64 * (2 * j + 2) as f64;
        }
        f = 1.0;
        for (j, e) in ec.iter_mut().enumerate() {
            *e = 1.0 / f;
            f *= (j + 1) as f64;
        }
        Tables { two_pi, inv_two_pi, sc, cc, ec }
    }
    fn reduce(&self, x: f64) -> f64 {
        let k = (x * self.inv_two_pi + 0.5).floor();
        x - k * self.two_pi
    }
    fn sinp(&self, x: f64) -> f64 {
        let r = self.reduce(x);
        let r2 = r * r;
        let mut s = self.sc[15];
        for j in (0..=14).rev() {
            s = s * r2 + self.sc[j];
        }
        r * s
    }
    fn cosp(&self, x: f64) -> f64 {
        let r = self.reduce(x);
        let r2 = r * r;
        let mut s = self.cc[15];
        for j in (0..=14).rev() {
            s = s * r2 + self.cc[j];
        }
        s
    }
    fn expp(&self, x: f64) -> f64 {
        let z = x / 1024.0;
        let mut e = self.ec[12];
        for j in (0..=11).rev() {
            e = e * z + self.ec[j];
        }
        for _ in 0..10 {
            e *= e;
        }
        e
    }
}

/// The trace hash and the call counter shared by the test functions.
struct Trace {
    hash: u64,
    calls: i64,
    last: i64,
}

impl Trace {
    fn new() -> Self {
        Trace { hash: FNV0, calls: 0, last: 0 }
    }
    fn byte(&mut self, b: u8) {
        self.hash ^= u64::from(b);
        self.hash = self.hash.wrapping_mul(FNVP);
    }
    fn tag(&mut self, c: u8) {
        self.byte(c);
    }
    #[allow(dead_code)] // Kept from the shared trace format; no family logs a value here.
    fn tagx(&mut self, c: u8, x: f64) {
        self.tag(c);
        for b in x.to_bits().to_le_bytes() {
            self.byte(b);
        }
    }
    /// A vector argument: the tag, the lower bound and the length (4 bytes each, little endian), every element.
    fn tagv(&mut self, c: u8, x: &Vector) {
        self.tag(c);
        for b in x.lower().to_le_bytes() {
            self.byte(b);
        }
        for b in x.length().to_le_bytes() {
            self.byte(b);
        }
        for v in x.as_slice() {
            for b in v.to_bits().to_le_bytes() {
                self.byte(b);
            }
        }
    }
    fn state(&self, mode: i32) -> i32 {
        match mode {
            0 => 0,
            1 => self.calls as i32,
            _ => (4 * self.calls + self.last) as i32,
        }
    }
}

// ---------------------------------------------------------------------------------
// The one-variable test functions (the same as in oracle/math1d.cpp)

// ---------------------------------------------------------------------------------
// The functions of several variables (the same as in oracle/mathglob.cpp)

const G_QUAD: i32 = 0;
const G_ROSEN: i32 = 1;
const G_POLY: i32 = 2;
const G_TRIG: i32 = 3;
const G_KINK: i32 = 4;
const G_EXP: i32 = 5;

/// A scalar term `gkind nparams p..` of `m` variables.
#[derive(Clone, Debug)]
struct Term {
    kind: i32,
    p: Vec<f64>,
}

fn pw(x: f64, e: i32) -> f64 {
    let mut r = 1.0;
    for _ in 0..e {
        r *= x;
    }
    r
}

/// One term of a polynomial differentiated `d[k]` times in `x_k`; `None` when it vanishes.
fn poly_term(q: &[f64], m: usize, x: &[f64], d: &[i32]) -> Option<f64> {
    let mut term = q[0];
    for k in 0..m {
        let e = q[1 + k] as i32;
        if e < d[k] {
            return None;
        }
        if d[k] == 1 {
            term *= f64::from(e);
        } else if d[k] == 2 {
            term *= f64::from(e * (e - 1));
        }
        term *= pw(x[k], e - d[k]);
    }
    Some(term)
}

impl Term {
    fn linear(&self, m: usize) -> bool {
        self.p.len() == 1 + m
    }
    fn value(&self, m: usize, x: &[f64], t: &Tables) -> f64 {
        let p = &self.p;
        match self.kind {
            G_QUAD => {
                let mut v = p[0];
                if self.linear(m) {
                    for i in 0..m {
                        v += x[i] * p[1 + i];
                    }
                    return v;
                }
                for i in 0..m {
                    let mut r = 0.0;
                    for j in 0..m {
                        r += p[1 + m + i * m + j] * x[j];
                    }
                    v += x[i] * (p[1 + i] + 0.5 * r);
                }
                v
            }
            G_ROSEN => {
                let (a, b) = (p[0], p[1]);
                if m == 1 {
                    let u = a - x[0];
                    return u * u;
                }
                let mut v = 0.0;
                for i in 0..m - 1 {
                    let u = x[i + 1] - x[i] * x[i];
                    let w = a - x[i];
                    v += b * (u * u) + w * w;
                }
                v
            }
            G_POLY => {
                let nt = p[0] as usize;
                let mut v = 0.0;
                for k in 0..nt {
                    let q = &p[1 + k * (1 + m)..];
                    let mut term = q[0];
                    for j in 0..m {
                        term *= pw(x[j], q[1 + j] as i32);
                    }
                    v += term;
                }
                v
            }
            G_TRIG => {
                let mut v = p[0];
                for j in 0..m {
                    let q = &p[1 + 4 * j..];
                    let arg = q[1] * x[j] + q[2];
                    v += q[0] * t.sinp(arg) + q[3] * (x[j] * x[j]);
                }
                v
            }
            G_KINK => {
                let mut v = p[0];
                for j in 0..m {
                    v += p[1 + 2 * j] * (x[j] - p[2 + 2 * j]).abs();
                }
                v
            }
            G_EXP => {
                let mut s = 0.0;
                for j in 0..m {
                    s += p[2 + j] * x[j];
                }
                p[0] + p[1] * t.expp(s)
            }
            other => panic!("unknown term kind {other}"),
        }
    }
    fn grad(&self, m: usize, x: &[f64], t: &Tables) -> Vec<f64> {
        let p = &self.p;
        let mut gr = vec![0.0; m];
        match self.kind {
            G_QUAD => {
                if self.linear(m) {
                    gr.copy_from_slice(&p[1..=m]);
                    return gr;
                }
                for i in 0..m {
                    let mut r = 0.0;
                    for j in 0..m {
                        r += (p[1 + m + i * m + j] + p[1 + m + j * m + i]) * x[j];
                    }
                    gr[i] = p[1 + i] + 0.5 * r;
                }
            }
            G_ROSEN => {
                let (a, b) = (p[0], p[1]);
                if m == 1 {
                    gr[0] = -2.0 * (a - x[0]);
                    return gr;
                }
                for i in 0..m - 1 {
                    let u = x[i + 1] - x[i] * x[i];
                    let w = a - x[i];
                    gr[i] += -4.0 * b * u * x[i] - 2.0 * w;
                    gr[i + 1] += 2.0 * b * u;
                }
            }
            G_POLY => {
                let nt = p[0] as usize;
                #[allow(clippy::needless_range_loop)] // `j` also builds the derivative order `d`.
                for j in 0..m {
                    let d: Vec<i32> = (0..m).map(|k| i32::from(k == j)).collect();
                    let mut s = 0.0;
                    for k in 0..nt {
                        if let Some(v) = poly_term(&p[1 + k * (1 + m)..], m, x, &d) {
                            s += v;
                        }
                    }
                    gr[j] = s;
                }
            }
            G_TRIG => {
                for j in 0..m {
                    let q = &p[1 + 4 * j..];
                    let arg = q[1] * x[j] + q[2];
                    gr[j] = (q[0] * q[1]) * t.cosp(arg) + 2.0 * q[3] * x[j];
                }
            }
            G_KINK => {
                for j in 0..m {
                    let u = x[j] - p[2 + 2 * j];
                    let a = p[1 + 2 * j];
                    gr[j] = if u > 0.0 {
                        a
                    } else if u < 0.0 {
                        -a
                    } else {
                        0.0
                    };
                }
            }
            G_EXP => {
                let mut s = 0.0;
                for j in 0..m {
                    s += p[2 + j] * x[j];
                }
                let e = t.expp(s);
                for j in 0..m {
                    gr[j] = (p[1] * p[2 + j]) * e;
                }
            }
            other => panic!("unknown term kind {other}"),
        }
        gr
    }
    /// The Hessian, row major.
    fn hess(&self, m: usize, x: &[f64], t: &Tables) -> Vec<f64> {
        let p = &self.p;
        let mut hs = vec![0.0; m * m];
        match self.kind {
            G_QUAD => {
                if self.linear(m) {
                    return hs;
                }
                for i in 0..m {
                    for j in 0..m {
                        hs[i * m + j] = 0.5 * (p[1 + m + i * m + j] + p[1 + m + j * m + i]);
                    }
                }
            }
            G_ROSEN => {
                let b = p[1];
                if m == 1 {
                    hs[0] = 2.0;
                    return hs;
                }
                for i in 0..m - 1 {
                    let u = x[i + 1] - x[i] * x[i];
                    hs[i * m + i] += 8.0 * b * (x[i] * x[i]) - 4.0 * b * u + 2.0;
                    hs[i * m + i + 1] += -4.0 * b * x[i];
                    hs[(i + 1) * m + i] += -4.0 * b * x[i];
                    hs[(i + 1) * m + i + 1] += 2.0 * b;
                }
            }
            G_POLY => {
                let nt = p[0] as usize;
                for i in 0..m {
                    for j in 0..m {
                        let mut d = vec![0; m];
                        d[i] += 1;
                        d[j] += 1;
                        let mut s = 0.0;
                        for k in 0..nt {
                            if let Some(v) = poly_term(&p[1 + k * (1 + m)..], m, x, &d) {
                                s += v;
                            }
                        }
                        hs[i * m + j] = s;
                    }
                }
            }
            G_TRIG => {
                for j in 0..m {
                    let q = &p[1 + 4 * j..];
                    let arg = q[1] * x[j] + q[2];
                    hs[j * m + j] = -((q[0] * q[1]) * q[1]) * t.sinp(arg) + 2.0 * q[3];
                }
            }
            G_KINK => {}
            G_EXP => {
                let mut s = 0.0;
                for j in 0..m {
                    s += p[2 + j] * x[j];
                }
                let e = t.expp(s);
                for i in 0..m {
                    for j in 0..m {
                        hs[i * m + j] = ((p[1] * p[2 + i]) * p[2 + j]) * e;
                    }
                }
            }
            other => panic!("unknown term kind {other}"),
        }
        hs
    }
}

/// The state of `Mv`: `n`, the terms, the failure box and the state mode.
struct Spec {
    n: usize,
    mode: i32,
    terms: Vec<Term>,
    fail_mode: i32,
    lo: Vec<f64>,
    hi: Vec<f64>,
    state_mode: i32,
    tables: Tables,
    trace: Trace,
}

impl Spec {
    /// Starts a call: the trace, the counter, the elements of `x` (which must have `n` elements).
    fn begin(&mut self, tag: u8, x: &Vector, code: i64) -> Vec<f64> {
        self.trace.tagv(tag, x);
        self.trace.last = code;
        self.trace.calls += 1;
        assert_eq!(x.length() as usize, self.n, "the solver passed an argument of another length");
        let xs = x.as_slice().to_vec();
        assert!(xs.iter().all(|v| v.is_finite()), "the solver called the function with a non-finite argument");
        xs
    }
    fn fails(&self, x: &[f64], bit: i32) -> bool {
        (self.fail_mode & bit) != 0 && (0..self.n).all(|j| x[j] >= self.lo[j] && x[j] <= self.hi[j])
    }
    fn mv_value(&self, x: &[f64]) -> f64 {
        let mut f = 0.0;
        for g in &self.terms {
            let v = g.value(self.n, x, &self.tables);
            f += if self.mode == 1 { v * v } else { v };
        }
        f
    }
    fn mv_grad(&self, x: &[f64]) -> Vec<f64> {
        let n = self.n;
        let mut gs = vec![0.0; n];
        for g in &self.terms {
            let gr = g.grad(n, x, &self.tables);
            if self.mode == 1 {
                let v = g.value(n, x, &self.tables);
                for j in 0..n {
                    gs[j] += (2.0 * v) * gr[j];
                }
            } else {
                for j in 0..n {
                    gs[j] += gr[j];
                }
            }
        }
        gs
    }
    fn mv_hess(&self, x: &[f64]) -> Vec<f64> {
        let n = self.n;
        let mut hs = vec![0.0; n * n];
        for g in &self.terms {
            let h = g.hess(n, x, &self.tables);
            if self.mode == 1 {
                let v = g.value(n, x, &self.tables);
                let gr = g.grad(n, x, &self.tables);
                for i in 0..n {
                    for j in 0..n {
                        hs[i * n + j] += 2.0 * (gr[i] * gr[j] + v * h[i * n + j]);
                    }
                }
            } else {
                for j in 0..n * n {
                    hs[j] += h[j];
                }
            }
        }
        hs
    }
}

fn put_vector(out: &mut Vector, values: &[f64]) {
    assert_eq!(out.length() as usize, values.len(), "the solver passed an output vector of another length");
    for (k, v) in values.iter().enumerate() {
        out.set_value(out.lower() + k as i32, *v).unwrap();
    }
}

fn put_matrix(out: &mut Matrix, rows: usize, cols: usize, values: &[f64]) {
    assert_eq!((out.row_number() as usize, out.col_number() as usize), (rows, cols), "output matrix of another shape");
    for i in 0..rows {
        for j in 0..cols {
            out.set_value(out.lower_row() + i as i32, out.lower_col() + j as i32, values[i * cols + j]).unwrap();
        }
    }
}

/// A function of several variables: the sum of its terms (mode 0) or of their squares (mode 1).
struct Mv {
    s: Spec,
    nv: i32,
    ng: i32,
    nw: i32,
    nh: i32,
    ns: i32,
}

impl MultipleVarFunction for Mv {
    fn nb_variables(&self) -> i32 {
        self.s.n as i32
    }
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.nv += 1;
        let xs = self.s.begin(b'V', x, 1);
        if self.s.fails(&xs, 1) {
            return None;
        }
        Some(self.s.mv_value(&xs))
    }
    fn state_number(&mut self) -> i32 {
        self.ns += 1;
        self.s.trace.tag(b'S');
        self.s.trace.state(self.s.state_mode)
    }
}

impl MultipleVarFunctionWithGradient for Mv {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        self.ng += 1;
        let xs = self.s.begin(b'G', x, 2);
        assert_eq!(g.length() as usize, self.s.n, "gradient vector of another length");
        if self.s.fails(&xs, 2) {
            return false;
        }
        put_vector(g, &self.s.mv_grad(&xs));
        true
    }
    fn values(&mut self, x: &Vector, g: &mut Vector) -> Option<f64> {
        self.nw += 1;
        let xs = self.s.begin(b'W', x, 3);
        assert_eq!(g.length() as usize, self.s.n, "gradient vector of another length");
        if self.s.fails(&xs, 4) {
            return None;
        }
        let v = self.s.mv_value(&xs);
        put_vector(g, &self.s.mv_grad(&xs));
        Some(v)
    }
}

impl MultipleVarFunctionWithHessian for Mv {
    fn values_with_hessian(&mut self, x: &Vector, g: &mut Vector, h: &mut Matrix) -> Option<f64> {
        self.nh += 1;
        let xs = self.s.begin(b'H', x, 4);
        let n = self.s.n;
        assert_eq!(g.length() as usize, n, "gradient vector of another length");
        assert_eq!((h.row_number() as usize, h.col_number() as usize), (n, n), "Hessian of another shape");
        if self.s.fails(&xs, 8) {
            return None;
        }
        let v = self.s.mv_value(&xs);
        put_vector(g, &self.s.mv_grad(&xs));
        put_matrix(h, n, n, &self.s.mv_hess(&xs));
        Some(v)
    }
}

// ---------------------------------------------------------------------------------
// The test function at the three interface levels. `GlobOptMin` takes what the C++ `dynamic_cast` would find.

struct AsValue<'a>(&'a mut Mv);

impl MultipleVarFunction for AsValue<'_> {
    fn nb_variables(&self) -> i32 {
        self.0.nb_variables()
    }
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.0.value(x)
    }
    fn state_number(&mut self) -> i32 {
        self.0.state_number()
    }
}

struct AsGradient<'a>(&'a mut Mv);

impl MultipleVarFunction for AsGradient<'_> {
    fn nb_variables(&self) -> i32 {
        self.0.nb_variables()
    }
    fn value(&mut self, x: &Vector) -> Option<f64> {
        self.0.value(x)
    }
    fn state_number(&mut self) -> i32 {
        self.0.state_number()
    }
}

impl MultipleVarFunctionWithGradient for AsGradient<'_> {
    fn gradient(&mut self, x: &Vector, g: &mut Vector) -> bool {
        self.0.gradient(x, g)
    }
    fn values(&mut self, x: &Vector, g: &mut Vector) -> Option<f64> {
        self.0.values(x, g)
    }
}

/// Calls `body` with the function presented at `level` (0 value, 1 gradient, 2 Hessian).
fn at_level<R>(level: i32, f: &mut Mv, body: impl FnOnce(&mut GlobFunction<'_>) -> R) -> R {
    match level {
        0 => {
            let mut w = AsValue(f);
            body(&mut GlobFunction::Value(&mut w))
        }
        1 => {
            let mut w = AsGradient(f);
            body(&mut GlobFunction::Gradient(&mut w))
        }
        _ => body(&mut GlobFunction::Hessian(f)),
    }
}

// ---------------------------------------------------------------------------------
// Results of one replayed call

/// The Rust API returned an error.
#[derive(Debug)]
struct Thrown;

impl From<MathError> for Thrown {
    fn from(_: MathError) -> Self {
        Thrown
    }
}

#[derive(Clone, Debug)]
enum Tok {
    F(f64),
    B(bool),
    I(i32),
    /// A 64-bit hash, printed as 16 hex digits.
    H(u64),
    /// An unsigned decimal (a random integer).
    U(u32),
}

/// Collects the values produced by the Rust side, in the order the oracle printed them.
#[derive(Default)]
struct Got(Vec<Tok>);

impl Got {
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn b(&mut self, v: bool) {
        self.0.push(Tok::B(v));
    }
    fn i(&mut self, v: i32) {
        self.0.push(Tok::I(v));
    }
    fn u(&mut self, v: u32) {
        self.0.push(Tok::U(v));
    }
    fn h(&mut self, v: u64) {
        self.0.push(Tok::H(v));
    }
    /// A vector: lower bound, length, elements.
    fn v(&mut self, v: &Vector) {
        self.i(v.lower());
        self.i(v.length());
        for x in v.as_slice() {
            self.f(*x);
        }
    }
    /// A matrix: lower row, row count, lower column, column count, elements row by row.
    fn m(&mut self, m: &Matrix) {
        self.i(m.lower_row());
        self.i(m.row_number());
        self.i(m.lower_col());
        self.i(m.col_number());
        for r in m.lower_row()..=m.upper_row() {
            for c in m.lower_col()..=m.upper_col() {
                self.f(m.value(r, c).unwrap());
            }
        }
    }
    fn counts_mv(&mut self, f: &Mv) {
        for c in [f.nv, f.ng, f.nw, f.nh, f.ns] {
            self.i(c);
        }
        self.h(f.s.trace.hash);
    }
}

// ---------------------------------------------------------------------------------
// Reading the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

/// A particle pool as the oracle prints it: count, dimension, and per particle position, velocity, best position
/// (dimension floats each), distance, best distance.
struct PoolIn {
    count: i32,
    dim: i32,
    data: Vec<Vec<f64>>,
}

impl PoolIn {
    fn build(&self) -> Result<PsoParticlesPool, MathError> {
        let mut pool = PsoParticlesPool::new(self.count, self.dim)?;
        let d = self.dim as usize;
        for (i, q) in self.data.iter().enumerate() {
            let p = pool.get_particle_mut(i as i32 + 1)?;
            p.position.copy_from_slice(&q[0..d]);
            p.velocity.copy_from_slice(&q[d..2 * d]);
            p.best_position.copy_from_slice(&q[2 * d..3 * d]);
            p.distance = q[3 * d];
            p.best_distance = q[3 * d + 1];
        }
        Ok(pool)
    }
}

/// Reads the inputs of a line; every method checks that it is the next word of the family's schema.
struct Rd<'a> {
    words: &'a [&'a str],
    at: usize,
    schema: Vec<&'static str>,
    next: usize,
}

impl<'a> Rd<'a> {
    fn new(name: &str, words: &'a [&'a str]) -> Self {
        let schema = SCHEMAS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("unknown oracle family {name}"))
            .1
            .split_whitespace()
            .collect();
        Rd { words, at: 0, schema, next: 0 }
    }
    fn expect(&mut self, word: &str) {
        let want = self.schema.get(self.next).copied().unwrap_or("<end>");
        assert_eq!(want, word, "reader call {} does not match the schema {:?}", self.next, self.schema);
        self.next += 1;
    }
    fn raw(&mut self) -> &'a str {
        let w = *self.words.get(self.at).unwrap_or_else(|| panic!("input word {} missing", self.at));
        self.at += 1;
        w
    }
    fn raw_f(&mut self) -> f64 {
        bits(self.raw())
    }
    fn raw_i(&mut self) -> i32 {
        self.raw().parse().expect("integer input")
    }
    fn raw_n(&mut self) -> usize {
        self.raw().parse().expect("count input")
    }
    /// A scalar.
    fn f(&mut self) -> f64 {
        self.expect("f");
        self.raw_f()
    }
    /// An integer (or a bool).
    fn i(&mut self) -> i32 {
        self.expect("i");
        self.raw_i()
    }
    /// A vector: lower length x...
    fn v(&mut self) -> Vector {
        self.expect("v");
        let lower = self.raw_i();
        let n = self.raw_n();
        let x: Vec<f64> = (0..n).map(|_| self.raw_f()).collect();
        Vector::from_slice(lower, &x).unwrap()
    }
    /// A particle pool: count dim, then 3 * dim + 2 floats per particle.
    fn pool(&mut self) -> PoolIn {
        self.expect("pool");
        let count = self.raw_i();
        let dim = self.raw_i();
        let data = (0..count).map(|_| (0..3 * dim + 2).map(|_| self.raw_f()).collect::<Vec<f64>>()).collect();
        PoolIn { count, dim, data }
    }
    /// A function of several variables: n mode nterms terms.. fail_mode lo.. hi.. state_mode.
    fn mv(&mut self) -> Mv {
        self.expect("mv");
        let n = self.raw_n();
        let mode = self.raw_i();
        let count = self.raw_n();
        let terms: Vec<Term> = (0..count)
            .map(|_| {
                let kind = self.raw_i();
                let np = self.raw_n();
                Term { kind, p: (0..np).map(|_| self.raw_f()).collect() }
            })
            .collect();
        let fail_mode = self.raw_i();
        let lo: Vec<f64> = (0..n).map(|_| self.raw_f()).collect();
        let hi: Vec<f64> = (0..n).map(|_| self.raw_f()).collect();
        let state_mode = self.raw_i();
        let s = Spec { n, mode, terms, fail_mode, lo, hi, state_mode, tables: Tables::new(), trace: Trace::new() };
        Mv { s, nv: 0, ng: 0, nw: 0, nh: 0, ns: 0 }
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
        assert_eq!(self.next, self.schema.len(), "unread schema words");
    }
}

// ---------------------------------------------------------------------------------
// Outputs

fn zeros(lower: i32, length: i32) -> Vector {
    Vector::new(lower, lower + length - 1).unwrap()
}

fn zeros_m(lower: i32, rows: i32, cols: i32) -> Matrix {
    Matrix::new(lower, lower + rows - 1, lower, lower + cols - 1).unwrap()
}

fn fold_f(hash: &mut u64, v: f64) {
    for b in v.to_bits().to_le_bytes() {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(FNVP);
    }
}

/// The state of a `GlobOptMin` as `out_glob` of the oracle prints it.
fn out_glob(s: &GlobOptMin, n: i32, f: &Mv, brief: bool, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    let nb = s.nb_extrema();
    g.i(nb);
    g.f(s.get_f());
    if !brief {
        let (dt, st) = s.get_tol();
        g.f(dt);
        g.f(st);
        g.i(s.get_continuity());
        g.f(s.get_functional_minimal_value());
        g.b(s.get_lip_const_state());
    }
    let mut p = zeros(1, n);
    let mut hash = FNV0;
    for i in 1..=nb {
        s.points(i, &mut p)?;
        for x in p.as_slice() {
            fold_f(&mut hash, *x);
        }
        if !brief && i <= 3 {
            g.v(&p);
        }
    }
    g.h(hash);
    g.counts_mv(f);
    Ok(())
}

fn out_particle(p: &occt_port::mathglob::PsoParticle, g: &mut Got) {
    for x in p.position.iter().chain(&p.velocity).chain(&p.best_position) {
        g.f(*x);
    }
    g.f(p.distance);
    g.f(p.best_distance);
}

fn out_pool(pool: &PsoParticlesPool, g: &mut Got) -> Result<(), Thrown> {
    for i in 1..=pool.nb_particles() {
        out_particle(pool.get_particle(i)?, g);
    }
    g.i(pool.get_best_particle());
    g.i(pool.get_worst_particle());
    Ok(())
}

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        "test_mv" => {
            let mut f = r.mv();
            let x = r.v();
            let n = f.nb_variables();
            let v = f.value(&x);
            g.b(v.is_some());
            if let Some(v) = v {
                g.f(v);
            }
            let mut gr = zeros(1, n);
            let ok = f.gradient(&x, &mut gr);
            g.b(ok);
            if ok {
                g.v(&gr);
            }
            let mut g2 = zeros(1, n);
            let w = f.values(&x, &mut g2);
            g.b(w.is_some());
            if let Some(v) = w {
                g.f(v);
                g.v(&g2);
            }
            let mut g3 = zeros(1, n);
            let mut h = zeros_m(1, n, n);
            let w = f.values_with_hessian(&x, &mut g3, &mut h);
            g.b(w.is_some());
            if let Some(v) = w {
                g.f(v);
                g.v(&g3);
                g.m(&h);
            }
            let st = f.state_number();
            g.i(st);
            g.i(f.nb_variables());
            g.counts_mv(&f);
        }
        "bullard" => {
            let (seed, n1, seed2, n2) = (r.i(), r.i(), r.i(), r.i());
            let mut rng = BullardGenerator::new(seed as u32);
            for _ in 0..n1 {
                g.u(rng.next_int());
            }
            for _ in 0..n1 {
                g.f(rng.next_real());
            }
            rng.set_seed(seed2 as u32);
            for _ in 0..n2 {
                g.f(rng.next_real());
            }
            for _ in 0..n2 {
                g.u(rng.next_int());
            }
            let mut hash = FNV0;
            for _ in 0..2000 {
                fold_f(&mut hash, rng.next_real());
            }
            g.h(hash);
            for _ in 0..2000 {
                for b in rng.next_int().to_le_bytes() {
                    hash ^= u64::from(b);
                    hash = hash.wrapping_mul(FNVP);
                }
            }
            g.h(hash);
            g.u(BullardGenerator::default().next_int());
        }
        "glob_new" => {
            let mut f = r.mv();
            let level = r.i();
            let (a, b) = (r.v(), r.v());
            let (c, dt, st) = (r.f(), r.f(), r.f());
            let n = f.nb_variables();
            let s = at_level(level, &mut f, |gf| GlobOptMin::new(gf, &a, &b, c, dt, st))?;
            out_glob(&s, n, &f, false, g)?;
        }
        "glob_perform" => {
            let mut f = r.mv();
            let level = r.i();
            let (a, b) = (r.v(), r.v());
            let (c, dt, st) = (r.f(), r.f(), r.f());
            let (cont, fmin, lip) = (r.i(), r.f(), r.i());
            let (dt2, st2, single) = (r.f(), r.f(), r.i());
            let n = f.nb_variables();
            let mut s = at_level(level, &mut f, |gf| GlobOptMin::new(gf, &a, &b, c, dt, st))?;
            s.set_continuity(cont);
            s.set_functional_minimal_value(fmin);
            s.set_lip_const_state(lip != 0);
            s.set_tol(dt2, st2);
            out_glob(&s, n, &f, true, g)?;
            at_level(level, &mut f, |gf| s.perform(gf, single != 0))?;
            out_glob(&s, n, &f, false, g)?;
        }
        "glob_staged" => {
            let mut f = r.mv();
            let level = r.i();
            let (a, b) = (r.v(), r.v());
            let (c, dt, st) = (r.f(), r.f(), r.f());
            let (cont, fmin, lip, single) = (r.i(), r.f(), r.i(), r.i());
            let (la, lb) = (r.v(), r.v());
            let (single2, cont2) = (r.i(), r.i());
            let n = f.nb_variables();
            let mut s = at_level(level, &mut f, |gf| GlobOptMin::new(gf, &a, &b, c, dt, st))?;
            s.set_continuity(cont);
            s.set_functional_minimal_value(fmin);
            s.set_lip_const_state(lip != 0);
            at_level(level, &mut f, |gf| s.perform(gf, single != 0))?;
            out_glob(&s, n, &f, false, g)?;
            s.set_local_params(&la, &lb)?;
            s.set_continuity(cont2);
            at_level(level, &mut f, |gf| s.perform(gf, single2 != 0))?;
            out_glob(&s, n, &f, false, g)?;
        }
        "glob_global" => {
            let mut f = r.mv();
            let level = r.i();
            let (a, b) = (r.v(), r.v());
            let (c, dt, st) = (r.f(), r.f(), r.f());
            let (cont, fmin, lip, single) = (r.i(), r.f(), r.i(), r.i());
            let mut f2 = r.mv();
            let level2 = r.i();
            let (a2, b2) = (r.v(), r.v());
            let (c2, dt2, st2) = (r.f(), r.f(), r.f());
            let single2 = r.i();
            let n = f.nb_variables();
            let mut s = at_level(level, &mut f, |gf| GlobOptMin::new(gf, &a, &b, c, dt, st))?;
            s.set_continuity(cont);
            s.set_functional_minimal_value(fmin);
            s.set_lip_const_state(lip != 0);
            at_level(level, &mut f, |gf| s.perform(gf, single != 0))?;
            out_glob(&s, n, &f, false, g)?;
            at_level(level2, &mut f2, |gf| s.set_global_params(gf, &a2, &b2, c2, dt2, st2))?;
            out_glob(&s, n, &f2, true, g)?;
            at_level(level2, &mut f2, |gf| s.perform(gf, single2 != 0))?;
            out_glob(&s, n, &f2, false, g)?;
        }
        "glob_access" => {
            let mut f = r.mv();
            let level = r.i();
            let (a, b) = (r.v(), r.v());
            let (c, dt, st) = (r.f(), r.f(), r.f());
            let (pm, idx) = (r.i(), r.i());
            let out = r.v();
            let (dt2, st2, cont) = (r.f(), r.f(), r.i());
            let n = f.nb_variables();
            let mut s = at_level(level, &mut f, |gf| GlobOptMin::new(gf, &a, &b, c, dt, st))?;
            if pm >= 0 {
                at_level(level, &mut f, |gf| s.perform(gf, pm != 0))?;
            }
            s.set_tol(dt2, st2);
            s.set_continuity(cont);
            let mut o = out;
            s.points(idx, &mut o)?;
            g.v(&o);
            out_glob(&s, n, &f, false, g)?;
        }
        "pso" => {
            let mut f = r.mv();
            let (lo, hi, st) = (r.v(), r.v(), r.v());
            let (nbpart, nbiter) = (r.i(), r.i());
            let (st2, nbiter2) = (r.v(), r.i());
            let mut o = r.v();
            let mut pso = Pso::new(&f, &lo, &hi, &st, nbpart, nbiter)?;
            let val = pso.perform(&mut f, &st2, &mut o, nbiter2)?;
            g.f(val);
            g.v(&o);
            g.counts_mv(&f);
        }
        "pso_pool" => {
            let mut f = r.mv();
            let (lo, hi, st) = (r.v(), r.v(), r.v());
            let (nbpart, nbiter) = (r.i(), r.i());
            let pl = r.pool();
            let (nbp, nbiter2) = (r.i(), r.i());
            let mut o = r.v();
            let mut pso = Pso::new(&f, &lo, &hi, &st, nbpart, nbiter)?;
            let mut pool = pl.build()?;
            let val = pso.perform_with_pool(&mut f, &mut pool, nbp, &mut o, nbiter2)?;
            g.f(val);
            g.v(&o);
            g.counts_mv(&f);
            out_pool(&pool, g)?;
        }
        "pso_pool_access" => {
            let pl = r.pool();
            let idx = r.i();
            let pool = pl.build()?;
            out_pool(&pool, g)?;
            out_particle(pool.get_particle(idx)?, g);
        }
        other => panic!("family {other} has no replay"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------

fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let n = x.to_bits() as i64;
        if n < 0 { i64::MIN - n } else { n }
    };
    key(a).abs_diff(key(b))
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
    worst: u64,
}

/// Compares the expected words of a line with the replayed values. Floats are compared bit
/// for bit for `EXACT` families and within `MAX_ULPS` of the line's largest magnitude
/// otherwise (zero tolerance when that magnitude is infinite); bools, ints, hashes and texts always exactly.
fn compare(tally: &mut Tally, line: &str, name: &str, want: &[&str], got: &[Tok]) {
    assert_eq!(want.len(), got.len(), "value count differs\n{line}");
    let exact = EXACT.contains(&name);
    let mut scale = 1.0_f64;
    for (w, t) in want.iter().zip(got) {
        if let Tok::F(_) = t {
            scale = scale.max(bits(w).abs());
        }
    }
    let tol = if scale.is_finite() { MAX_ULPS * f64::EPSILON * scale } else { 0.0 };
    for (index, (w, t)) in want.iter().zip(got).enumerate() {
        match t {
            Tok::F(g) => {
                let (w, g) = (bits(w), *g);
                let d = ulps(w, g);
                let ok =
                    if exact { w.to_bits() == g.to_bits() } else { (w - g).abs() <= tol || w.to_bits() == g.to_bits() };
                assert!(ok, "value {index}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
                tally.values += 1;
                tally.exact += usize::from(d == 0);
                tally.worst = tally.worst.max(d);
            }
            Tok::B(g) => assert_eq!(*w, if *g { "1" } else { "0" }, "value {index} (bool)\n{line}"),
            Tok::I(g) => assert_eq!(w.parse::<i32>().expect("integer output"), *g, "value {index} (int)\n{line}"),
            Tok::H(g) => {
                assert_eq!(
                    u64::from_str_radix(w, 16).expect("hash output"),
                    *g,
                    "value {index} (call trace hash)\n{line}"
                )
            }
            Tok::U(g) => assert_eq!(w.parse::<u32>().expect("unsigned output"), *g, "value {index} (unsigned)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------

#[test]
fn mathglob_matches_occt() {
    let text = include_str!("mathglob.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in mathglob.txt:\n{line}");
        let (head, tail) = line.split_once(" | ").unwrap_or_else(|| panic!("line without ' | ': {line}"));
        let head: Vec<&str> = head.split_whitespace().collect();
        let want: Vec<&str> = tail.split_whitespace().collect();
        let (name, inputs) = head.split_first().expect("family name");
        let name: &str = name;
        let entry = stats.entry(name).or_default();
        entry.0 += 1;
        entry.1 += usize::from(!inputs.is_empty());

        let mut reader = Rd::new(name, inputs);
        let mut got = Got::default();
        let outcome = run(name, &mut reader, &mut got);
        let marker = matches!(want.first(), Some(&"throws") | Some(&"unchecked"));
        match outcome {
            Err(Thrown) => assert!(marker, "Rust API returned an error but OCCT returned values\n{line}"),
            Ok(()) => {
                assert!(!marker, "Rust API returned values but OCCT {}\n{line}", want[0]);
                reader.finish();
                compare(&mut tally, line, name, &want, &got.0);
            }
        }
    }

    // Input coverage: no duplicates (checked above), every family with inputs has at least
    // MIN_DISTINCT distinct lines, and only the listed families are input-less (printed once).
    for (name, (count, with_inputs)) in &stats {
        if INPUT_LESS.contains(name) {
            assert_eq!((*count, *with_inputs), (1, 0), "input-less family {name} must be printed exactly once");
        } else {
            assert_eq!(*with_inputs, *count, "family {name} lost its inputs");
            assert!(*count >= MIN_DISTINCT, "family {name} has only {count} distinct lines");
        }
    }
    for name in INPUT_LESS {
        assert!(stats.contains_key(name), "input-less family {name} missing");
    }
    // Every family is still covered, and every EXACT entry names a real family.
    let present: BTreeSet<&str> = stats.keys().copied().collect();
    let expected: BTreeSet<&str> = SCHEMAS.iter().map(|(n, _)| *n).collect();
    assert_eq!(present, expected, "family list changed");
    for name in EXACT {
        assert!(expected.contains(name), "EXACT names an unknown family {name}");
    }

    assert_eq!(lines, text.lines().count());
    assert_eq!(lines, GOLDEN_LINES, "golden line count changed");
    println!("{lines} lines, {} values, {} bit-identical, worst {} ulps", tally.values, tally.exact, tally.worst);
}
