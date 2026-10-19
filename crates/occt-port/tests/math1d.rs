//! Numerical reference test for OCCT 8.0.1's one-dimensional solvers of the `math` package: `math_Function` and
//! `math_FunctionWithDerivative` (the callback traits), `math_FunctionSample`, `math_FunctionRoots`,
//! `math_FunctionRoot`, `math_FunctionAllRoots`, `math_BracketedRoot`, `math_NewtonFunctionRoot`,
//! `math_BracketMinimum`, `math_BrentMinimum`, `math_TrigonometricFunctionRoots` and
//! `math_TrigonometricEquationFunction`.
//!
//! `math1d.txt` is printed by `oracle/math1d.cpp` (real OCCT). Each line is `<family> <inputs...> | <outputs...>`: f64
//! values are 16-hex-digit bit patterns and ints (and bools) are decimal. Every input of a call is on the left of
//! ` | `; this test reads them back with the `Rd` reader methods of the same name as the oracle's schema words
//! (`f i tf`) and replays the call on the Rust API of docs/api-math1d.md. The right side is either the expected
//! values, `throws` (OCCT raised) or `unchecked` (a check that OCCT 8.0.1 as built has compiled out would have failed
//! and the numbers are unspecified); on both the Rust API must return an error.
//!
//! The solvers call user functions. Those are the closed set of test functions described at the top of
//! `oracle/math1d.cpp` (kind, parameters, failure interval, state mode), implemented by `Tf` below with the same
//! arithmetic (+ - * / floor and fabs only), so both sides see the same bits. Besides the numbers, every line prints
//! the number of callback calls and a hash of the call trace: the Rust solver must make the same calls in the same
//! order.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::math1d::{
    BracketMinimum, BracketedRoot, BrentMinimum, BrentSolutionTest, BrentState, Function, FunctionAllRoots,
    FunctionRoot, FunctionRoots, FunctionSample, FunctionWithDerivative, NewtonFunctionRoot,
    TrigonometricEquationFunction, TrigonometricFunctionRoots,
};
use occt_port::mathlin::MathError;
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for results that are not computed with `+ - * /` and `sqrt` only, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1). Only the trigonometric families need it
/// (they call `asin`, `acos`, `atan`, `sin`, `cos` and, through the polynomial roots, `pow`): their cases were kept only
/// when a 1-ulp change of those C library functions moves the result by at most 3 ulps.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once. There are none here.
const INPUT_LESS: &[&str] = &[];

/// Families whose results use only + - * / sqrt, floor, abs and copysign: compared bit for bit (signed zeros included).
/// Everything except the trigonometric families. `test_function` checks the judge's own test functions.
const EXACT: &[&str] = &[
    "bracket_minimum",
    "bracket_minimum_access",
    "bracket_minimum_dump",
    "bracket_minimum_staged",
    "bracketed_root",
    "bracketed_root_access",
    "bracketed_root_dump",
    "brent_minimum",
    "brent_minimum_access",
    "brent_minimum_custom",
    "brent_minimum_dump",
    "brent_minimum_twice",
    "function_all_roots",
    "function_all_roots_access",
    "function_all_roots_dump",
    "function_root",
    "function_root_access",
    "function_root_bounded",
    "function_root_dump",
    "function_roots",
    "function_roots_access",
    "function_roots_dump",
    "function_sample",
    "newton_root",
    "newton_root_access",
    "newton_root_bounded",
    "newton_root_dump",
    "newton_root_perform",
    "test_function",
];

/// Every family the oracle must print (a family that disappears from math1d.txt fails).
const FAMILIES: &[&str] = &[
    "bracket_minimum",
    "bracket_minimum_access",
    "bracket_minimum_dump",
    "bracket_minimum_staged",
    "bracketed_root",
    "bracketed_root_access",
    "bracketed_root_dump",
    "brent_minimum",
    "brent_minimum_access",
    "brent_minimum_custom",
    "brent_minimum_dump",
    "brent_minimum_twice",
    "function_all_roots",
    "function_all_roots_access",
    "function_all_roots_dump",
    "function_root",
    "function_root_access",
    "function_root_bounded",
    "function_root_dump",
    "function_roots",
    "function_roots_access",
    "function_roots_dump",
    "function_sample",
    "newton_root",
    "newton_root_access",
    "newton_root_bounded",
    "newton_root_dump",
    "newton_root_perform",
    "test_function",
    "trig_equation_function",
    "trig_roots",
    "trig_roots_access",
    "trig_roots_cde",
    "trig_roots_de",
    "trig_roots_dump",
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 5220;

// ---------------------------------------------------------------------------------
// The test functions (the same as in oracle/math1d.cpp)

const K_POLY: i32 = 0;
const K_TRIG: i32 = 1;
const K_RATIONAL: i32 = 2;
const K_MULTIPLE: i32 = 3;
const K_FLAT: i32 = 4;
const K_EXP: i32 = 5;
const K_ABS: i32 = 6;

const FNV0: u64 = 0xcbf2_9ce4_8422_2325;
const FNVP: u64 = 0x0000_0100_0000_01b3;

/// The deterministic stand-ins for sin, cos and exp: built from the same tables as the oracle's, with + - * / and floor.
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

/// A test function as printed in the golden file: `kind nparams p.. fail_lo fail_hi fail_mode state_mode`.
#[derive(Clone, Debug)]
struct Spec {
    kind: i32,
    p: Vec<f64>,
    fail_lo: f64,
    fail_hi: f64,
    fail_mode: i32,
    state_mode: i32,
}

/// The function object handed to the solvers: counts the calls and hashes the call trace.
struct Tf {
    spec: Spec,
    tables: Tables,
    nv: i32,
    nd: i32,
    nw: i32,
    ns: i32,
    trace: u64,
    calls: i64,
    last: i64,
}

impl Tf {
    fn new(spec: Spec) -> Self {
        Tf { spec, tables: Tables::new(), nv: 0, nd: 0, nw: 0, ns: 0, trace: FNV0, calls: 0, last: 0 }
    }

    /// Value and derivative; `None` when the function has no value there (the pole of the rational function).
    fn eval(&self, x: f64) -> Option<(f64, f64)> {
        let p = &self.spec.p;
        let t = &self.tables;
        match self.spec.kind {
            K_POLY => {
                let n = p.len();
                let mut v = p[n - 1];
                for i in (0..=n - 2).rev() {
                    v = v * x + p[i];
                }
                let mut d = (n - 1) as f64 * p[n - 1];
                for i in (1..=n - 2).rev() {
                    d = d * x + i as f64 * p[i];
                }
                Some((v, d))
            }
            K_TRIG => {
                let (mut v, mut d) = (0.0, 0.0);
                for term in p.chunks_exact(3) {
                    let arg = term[1] * x + term[2];
                    v += term[0] * t.sinp(arg);
                    d += (term[0] * term[1]) * t.cosp(arg);
                }
                Some((v, d))
            }
            K_RATIONAL => {
                let e = x - p[3];
                if e.abs() < 1e-100 {
                    return None;
                }
                Some((p[0] + p[1] / e + p[2] * x, p[2] - (p[1] / e) / e))
            }
            K_MULTIPLE => {
                let m = p[2] as i32;
                let u = x - p[1];
                let mut pm1 = 1.0;
                for _ in 0..m - 1 {
                    pm1 *= u;
                }
                let pm = pm1 * u;
                let v = p[0] * pm * (x - p[3]);
                let d = p[0] * (f64::from(m) * pm1 * (x - p[3]) + pm);
                Some((v, d))
            }
            K_FLAT => {
                let pw = p[4] as i32;
                let c = p[5];
                let power = |t: f64| match pw {
                    1 => t,
                    2 => t * t,
                    _ => t * t * t,
                };
                let slope = |t: f64| match pw {
                    1 => 1.0,
                    2 => 2.0 * t,
                    _ => 3.0 * (t * t),
                };
                if x < p[0] {
                    let t = p[0] - x;
                    Some((c + p[2] * power(t), -(p[2] * slope(t))))
                } else if x > p[1] {
                    let t = x - p[1];
                    Some((c + p[3] * power(t), p[3] * slope(t)))
                } else {
                    Some((c, 0.0))
                }
            }
            K_EXP => {
                let e = t.expp(p[0] * x);
                Some((e - p[1], p[0] * e))
            }
            K_ABS => {
                let u = x - p[1];
                let d = if u > 0.0 {
                    p[0]
                } else if u < 0.0 {
                    -p[0]
                } else {
                    0.0
                };
                Some((p[0] * u.abs() + p[2], d))
            }
            other => panic!("unknown test function kind {other}"),
        }
    }

    fn tag(&mut self, c: u8) {
        self.trace ^= u64::from(c);
        self.trace = self.trace.wrapping_mul(FNVP);
    }
    fn tagx(&mut self, c: u8, x: f64) {
        self.tag(c);
        for byte in x.to_bits().to_le_bytes() {
            self.trace ^= u64::from(byte);
            self.trace = self.trace.wrapping_mul(FNVP);
        }
    }
    fn fails(&self, x: f64, bit: i32) -> bool {
        (self.spec.fail_mode & bit) != 0 && x >= self.spec.fail_lo && x <= self.spec.fail_hi
    }
    fn begin_call(&mut self, x: f64) {
        assert!(x.is_finite(), "the solver called the function with a non-finite argument");
        self.calls += 1;
    }
}

impl Function for Tf {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.begin_call(x);
        self.nv += 1;
        self.tagx(b'V', x);
        self.last = 1;
        if self.fails(x, 1) {
            return None;
        }
        self.eval(x).map(|(v, _)| v)
    }
    fn state_number(&mut self) -> i32 {
        self.ns += 1;
        self.tag(b'S');
        match self.spec.state_mode {
            0 => 0,
            1 => self.calls as i32,
            _ => (4 * self.calls + self.last) as i32,
        }
    }
}

impl FunctionWithDerivative for Tf {
    fn derivative(&mut self, x: f64) -> Option<f64> {
        self.begin_call(x);
        self.nd += 1;
        self.tagx(b'D', x);
        self.last = 2;
        if self.fails(x, 2) {
            return None;
        }
        self.eval(x).map(|(_, d)| d)
    }
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        self.begin_call(x);
        self.nw += 1;
        self.tagx(b'W', x);
        self.last = 3;
        if self.fails(x, 4) {
            return None;
        }
        self.eval(x)
    }
}

// ---------------------------------------------------------------------------------
// Results of one replayed call

/// The Rust API returned an error (or the call is known to be invalid).
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
    /// A text (a `Dump()`), spaces as `_` and newlines as `\n`.
    S(String),
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
    /// The call counts and the trace hash of a test function.
    fn counts(&mut self, tf: &Tf) {
        self.i(tf.nv);
        self.i(tf.nd);
        self.i(tf.nw);
        self.i(tf.ns);
        self.0.push(Tok::H(tf.trace));
    }
    /// A `Dump()` text: newlines as `\n`, spaces as `_`.
    fn dump(&mut self, text: &str) {
        self.0.push(Tok::S(text.replace('\n', "\\n").replace(' ', "_")));
    }
}

// ---------------------------------------------------------------------------------
// Reading the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

struct Rd<'a> {
    words: &'a [&'a str],
    at: usize,
}

impl<'a> Rd<'a> {
    fn word(&mut self) -> &'a str {
        let w = *self.words.get(self.at).unwrap_or_else(|| panic!("input word {} missing", self.at));
        self.at += 1;
        w
    }
    /// A scalar.
    fn f(&mut self) -> f64 {
        bits(self.word())
    }
    /// An integer.
    fn i(&mut self) -> i32 {
        self.word().parse().expect("integer input")
    }
    /// A test function: kind, parameter count, parameters, fail_lo, fail_hi, fail_mode, state_mode.
    fn tf(&mut self) -> Tf {
        let kind = self.i();
        let n: usize = self.word().parse().expect("parameter count");
        let p: Vec<f64> = (0..n).map(|_| self.f()).collect();
        let (fail_lo, fail_hi) = (self.f(), self.f());
        let (fail_mode, state_mode) = (self.i(), self.i());
        Tf::new(Spec { kind, p, fail_lo, fail_hi, fail_mode, state_mode })
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn out_function_roots(r: &FunctionRoots, g: &mut Got) -> Result<(), Thrown> {
    g.b(r.is_all_null()?);
    let n = r.nb_solutions()?;
    g.i(n);
    for j in 1..=n {
        g.f(r.value(j)?);
        g.i(r.state_number(j)?);
    }
    Ok(())
}

fn out_newton(r: &NewtonFunctionRoot, g: &mut Got) -> Result<(), Thrown> {
    g.b(r.is_done());
    if r.is_done() {
        g.f(r.root()?);
        g.f(r.derivative()?);
        g.f(r.value()?);
        g.i(r.nb_iterations()?);
    }
    Ok(())
}

fn out_bracket_minimum(m: &BracketMinimum, g: &mut Got) -> Result<(), Thrown> {
    g.b(m.is_done());
    if m.is_done() {
        let (a, b, c) = m.values()?;
        let (fa, fb, fc) = m.function_values()?;
        for v in [a, b, c, fa, fb, fc] {
            g.f(v);
        }
    }
    Ok(())
}

fn out_brent(m: &BrentMinimum, g: &mut Got) -> Result<(), Thrown> {
    g.b(m.is_done());
    if m.is_done() {
        g.f(m.location()?);
        g.f(m.minimum()?);
        g.i(m.nb_iterations()?);
    }
    Ok(())
}

fn out_trig(r: &TrigonometricFunctionRoots, g: &mut Got) -> Result<(), Thrown> {
    g.b(r.is_done());
    g.b(r.infinite_roots());
    if r.is_done() && !r.infinite_roots() {
        let n = r.nb_solutions()?;
        g.i(n);
        for j in 1..=n {
            g.f(r.value(j)?);
        }
    }
    Ok(())
}

/// The `IsSolutionReached` redefinitions of `brent_minimum_custom` (see `BrentCustom` in the oracle).
struct BrentCustom {
    mode: i32,
    thr: f64,
}

impl BrentSolutionTest for BrentCustom {
    fn is_solution_reached(&mut self, state: &BrentState, f: &mut dyn Function) -> bool {
        match self.mode {
            1 => state.is_solution_reached_default() || state.fx <= self.thr,
            2 => {
                // the function is called whatever the default test says
                let t = f.value(0.5 * (state.a + state.b));
                state.is_solution_reached_default() || t.is_some_and(|t| t < state.fx - self.thr)
            }
            _ => state.is_solution_reached_default() && state.fx.abs() <= self.thr,
        }
    }
}

fn new_brent(variant: i32, tolx: f64, fbx: f64, nbit: i32, zeps: f64) -> BrentMinimum {
    if variant == 0 { BrentMinimum::new(tolx, nbit, zeps) } else { BrentMinimum::with_fbx(tolx, fbx, nbit, zeps) }
}

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        // ---- the test functions themselves ----
        "test_function" => {
            let mut tf = r.tf();
            let x = r.f();
            let v = tf.value(x);
            g.b(v.is_some());
            if let Some(v) = v {
                g.f(v);
            }
            let d = tf.derivative(x);
            g.b(d.is_some());
            if let Some(d) = d {
                g.f(d);
            }
            let w = tf.values(x);
            g.b(w.is_some());
            if let Some((v, d)) = w {
                g.f(v);
                g.f(d);
            }
            let state = tf.state_number();
            g.i(state);
            g.counts(&tf);
        }
        // ---- math_FunctionSample ----
        "function_sample" => {
            let (a, b, n, index) = (r.f(), r.f(), r.i(), r.i());
            let s = FunctionSample::new(a, b, n);
            let (ra, rb) = s.bounds();
            g.f(ra);
            g.f(rb);
            g.i(s.nb_points());
            g.f(s.get_parameter(index)?);
        }
        // ---- math_FunctionRoots ----
        "function_roots" | "function_roots_access" | "function_roots_dump" => {
            let mut tf = r.tf();
            let (a, b, nb) = (r.f(), r.f(), r.i());
            let (ex, ef, en, k) = (r.f(), r.f(), r.f(), r.f());
            let (q, index) = if name == "function_roots_access" { (r.i(), r.i()) } else { (0, 0) };
            let roots = FunctionRoots::new(&mut tf, a, b, nb, ex, ef, en, k);
            match name {
                "function_roots" => {
                    g.b(roots.is_done());
                    g.counts(&tf);
                    if roots.is_done() {
                        out_function_roots(&roots, g)?;
                    }
                }
                "function_roots_access" => match q {
                    0 => g.b(roots.is_all_null()?),
                    1 => g.i(roots.nb_solutions()?),
                    2 => g.f(roots.value(index)?),
                    _ => g.i(roots.state_number(index)?),
                },
                _ => g.dump(&roots.dump()),
            }
        }
        // ---- math_FunctionAllRoots ----
        "function_all_roots" | "function_all_roots_access" | "function_all_roots_dump" => {
            let mut tf = r.tf();
            let (a, b, n) = (r.f(), r.f(), r.i());
            let (ex, ef, en) = (r.f(), r.f(), r.f());
            let (q, index) = if name == "function_all_roots_access" { (r.i(), r.i()) } else { (0, 0) };
            let sample = FunctionSample::new(a, b, n);
            let roots = FunctionAllRoots::new(&mut tf, &sample, ex, ef, en)?;
            match name {
                "function_all_roots" => {
                    g.b(roots.is_done());
                    g.counts(&tf);
                    let ni = roots.nb_intervals()?;
                    g.i(ni);
                    for j in 1..=ni {
                        let (ia, ib) = roots.get_interval(j)?;
                        let (i1, i2) = roots.get_interval_state(j)?;
                        g.f(ia);
                        g.f(ib);
                        g.i(i1);
                        g.i(i2);
                    }
                    let np = roots.nb_points()?;
                    g.i(np);
                    for j in 1..=np {
                        g.f(roots.get_point(j)?);
                        g.i(roots.get_point_state(j)?);
                    }
                }
                "function_all_roots_access" => match q {
                    0 => g.i(roots.nb_intervals()?),
                    1 => {
                        let (ia, ib) = roots.get_interval(index)?;
                        g.f(ia);
                        g.f(ib);
                    }
                    2 => {
                        let (i1, i2) = roots.get_interval_state(index)?;
                        g.i(i1);
                        g.i(i2);
                    }
                    3 => g.i(roots.nb_points()?),
                    4 => g.f(roots.get_point(index)?),
                    _ => g.i(roots.get_point_state(index)?),
                },
                _ => g.dump(&roots.dump()),
            }
        }
        // ---- math_BracketedRoot ----
        "bracketed_root" | "bracketed_root_access" | "bracketed_root_dump" => {
            let mut tf = r.tf();
            let (b1, b2, tol, nbit, zeps) = (r.f(), r.f(), r.f(), r.i(), r.f());
            let q = if name == "bracketed_root_access" { r.i() } else { 0 };
            let root = BracketedRoot::new(&mut tf, b1, b2, tol, nbit, zeps);
            match name {
                "bracketed_root" => {
                    g.b(root.is_done());
                    g.counts(&tf);
                    if root.is_done() {
                        g.f(root.root()?);
                        g.f(root.value()?);
                        g.i(root.nb_iterations()?);
                    }
                }
                "bracketed_root_access" => match q {
                    0 => g.f(root.root()?),
                    1 => g.f(root.value()?),
                    _ => g.i(root.nb_iterations()?),
                },
                _ => g.dump(&root.dump()),
            }
        }
        // ---- math_FunctionRoot ----
        "function_root" | "function_root_bounded" => {
            let mut tf = r.tf();
            let (guess, tol) = (r.f(), r.f());
            let root = if name == "function_root" {
                let nbit = r.i();
                FunctionRoot::new(&mut tf, guess, tol, nbit)?
            } else {
                let (a, b, nbit) = (r.f(), r.f(), r.i());
                FunctionRoot::new_bounded(&mut tf, guess, tol, a, b, nbit)?
            };
            g.b(root.is_done());
            g.counts(&tf);
            if root.is_done() {
                g.f(root.root()?);
                g.f(root.derivative()?);
                g.f(root.value()?);
                g.i(root.nb_iterations()?);
            }
        }
        "function_root_access" | "function_root_dump" => {
            let mut tf = r.tf();
            let bounded = r.i();
            let (guess, tol, a, b, nbit) = (r.f(), r.f(), r.f(), r.f(), r.i());
            let q = if name == "function_root_access" { r.i() } else { 0 };
            let root = if bounded == 0 {
                FunctionRoot::new(&mut tf, guess, tol, nbit)?
            } else {
                FunctionRoot::new_bounded(&mut tf, guess, tol, a, b, nbit)?
            };
            if name == "function_root_dump" {
                g.dump(&root.dump());
            } else {
                match q {
                    0 => g.f(root.root()?),
                    1 => g.f(root.derivative()?),
                    2 => g.f(root.value()?),
                    _ => g.i(root.nb_iterations()?),
                }
            }
        }
        // ---- math_NewtonFunctionRoot ----
        "newton_root" | "newton_root_bounded" => {
            let mut tf = r.tf();
            let (guess, ex, ef) = (r.f(), r.f(), r.f());
            let root = if name == "newton_root" {
                let nbit = r.i();
                NewtonFunctionRoot::new(&mut tf, guess, ex, ef, nbit)
            } else {
                let (a, b, nbit) = (r.f(), r.f(), r.i());
                NewtonFunctionRoot::new_bounded(&mut tf, guess, ex, ef, a, b, nbit)
            };
            out_newton(&root, g)?;
            g.counts(&tf);
        }
        "newton_root_perform" => {
            let (a, b, ex, ef, nbit) = (r.f(), r.f(), r.f(), r.f(), r.i());
            let (mut tf1, guess1) = (r.tf(), r.f());
            let (mut tf2, guess2) = (r.tf(), r.f());
            let twice = r.i();
            let mut root = NewtonFunctionRoot::without_function(a, b, ex, ef, nbit);
            g.b(root.is_done());
            root.perform(&mut tf1, guess1);
            out_newton(&root, g)?;
            g.counts(&tf1);
            if twice != 0 {
                root.perform(&mut tf2, guess2);
                out_newton(&root, g)?;
                g.counts(&tf2);
            }
        }
        "newton_root_access" | "newton_root_dump" => {
            let mut tf = r.tf();
            let bounded = r.i();
            let (guess, ex, ef, a, b, nbit) = (r.f(), r.f(), r.f(), r.f(), r.f(), r.i());
            let q = if name == "newton_root_access" { r.i() } else { 0 };
            let root = if bounded == 0 {
                NewtonFunctionRoot::new(&mut tf, guess, ex, ef, nbit)
            } else {
                NewtonFunctionRoot::new_bounded(&mut tf, guess, ex, ef, a, b, nbit)
            };
            if name == "newton_root_dump" {
                g.dump(&root.dump());
            } else {
                match q {
                    0 => g.f(root.root()?),
                    1 => g.f(root.derivative()?),
                    2 => g.f(root.value()?),
                    _ => g.i(root.nb_iterations()?),
                }
            }
        }
        // ---- math_BracketMinimum ----
        "bracket_minimum" => {
            let variant = r.i();
            let mut tf = r.tf();
            let (a, b, fa, fb) = (r.f(), r.f(), r.f(), r.f());
            let m = match variant {
                0 => BracketMinimum::with_function(&mut tf, a, b),
                1 => BracketMinimum::with_function_fa(&mut tf, a, b, fa),
                _ => BracketMinimum::with_function_fa_fb(&mut tf, a, b, fa, fb),
            };
            out_bracket_minimum(&m, g)?;
            g.counts(&tf);
        }
        "bracket_minimum_staged" | "bracket_minimum_access" | "bracket_minimum_dump" => {
            let mut tf = r.tf();
            let (a, b) = (r.f(), r.f());
            let limited = r.i();
            let (left, right) = (r.f(), r.f());
            let (set_fa, fa) = (r.i(), r.f());
            let (set_fb, fb) = (r.i(), r.f());
            let performs = r.i();
            let q = if name == "bracket_minimum_access" { r.i() } else { 0 };
            let mut m = BracketMinimum::new(a, b);
            if name == "bracket_minimum_staged" {
                g.b(m.is_done());
            }
            if limited != 0 {
                m.set_limits(left, right);
            }
            if set_fa != 0 {
                m.set_fa(fa);
            }
            if set_fb != 0 {
                m.set_fb(fb);
            }
            for _ in 0..performs {
                m.perform(&mut tf);
                if name == "bracket_minimum_staged" {
                    out_bracket_minimum(&m, g)?;
                }
            }
            match name {
                "bracket_minimum_staged" => g.counts(&tf),
                "bracket_minimum_access" => {
                    // OCCT's StdFail_NotDone checks of Values / FunctionValues are compiled out (`unchecked`)
                    let (x, y, z) = if q == 0 { m.values()? } else { m.function_values()? };
                    g.f(x);
                    g.f(y);
                    g.f(z);
                }
                _ => g.dump(&m.dump()),
            }
        }
        // ---- math_BrentMinimum ----
        "brent_minimum" | "brent_minimum_access" | "brent_minimum_dump" => {
            let (variant, tolx, fbx, nbit, zeps) = (r.i(), r.f(), r.f(), r.i(), r.f());
            let mut tf = r.tf();
            let (ax, bx, cx) = (r.f(), r.f(), r.f());
            let q = if name == "brent_minimum_access" { r.i() } else { 0 };
            let mut m = new_brent(variant, tolx, fbx, nbit, zeps);
            m.perform(&mut tf, ax, bx, cx);
            match name {
                "brent_minimum" => {
                    out_brent(&m, g)?;
                    g.counts(&tf);
                }
                "brent_minimum_access" => match q {
                    0 => g.f(m.location()?),
                    1 => g.f(m.minimum()?),
                    _ => g.i(m.nb_iterations()?),
                },
                _ => g.dump(&m.dump()),
            }
        }
        "brent_minimum_twice" => {
            let (variant, tolx, fbx, nbit, zeps) = (r.i(), r.f(), r.f(), r.i(), r.f());
            let mut tf1 = r.tf();
            let (ax1, bx1, cx1) = (r.f(), r.f(), r.f());
            let mut tf2 = r.tf();
            let (ax2, bx2, cx2) = (r.f(), r.f(), r.f());
            let mut m = new_brent(variant, tolx, fbx, nbit, zeps);
            m.perform(&mut tf1, ax1, bx1, cx1);
            out_brent(&m, g)?;
            g.counts(&tf1);
            m.perform(&mut tf2, ax2, bx2, cx2);
            out_brent(&m, g)?;
            g.counts(&tf2);
        }
        "brent_minimum_custom" => {
            let (variant, tolx, fbx, nbit, zeps) = (r.i(), r.f(), r.f(), r.i(), r.f());
            let mut tf = r.tf();
            let (ax, bx, cx) = (r.f(), r.f(), r.f());
            let (mode, thr) = (r.i(), r.f());
            let mut m = new_brent(variant, tolx, fbx, nbit, zeps);
            m.perform_with_test(&mut tf, ax, bx, cx, &mut BrentCustom { mode, thr });
            out_brent(&m, g)?;
            g.counts(&tf);
        }
        // ---- math_TrigonometricFunctionRoots ----
        "trig_roots" | "trig_roots_access" | "trig_roots_dump" => {
            let (a, b, c, d, e, inf, sup) = (r.f(), r.f(), r.f(), r.f(), r.f(), r.f(), r.f());
            let (q, index) = if name == "trig_roots_access" { (r.i(), r.i()) } else { (0, 0) };
            let roots = TrigonometricFunctionRoots::new(a, b, c, d, e, inf, sup);
            match name {
                "trig_roots" => out_trig(&roots, g)?,
                "trig_roots_access" => {
                    if q == 0 {
                        g.i(roots.nb_solutions()?);
                    } else {
                        g.f(roots.value(index)?);
                    }
                }
                _ => g.dump(&roots.dump()),
            }
        }
        "trig_roots_cde" => {
            let (c, d, e, inf, sup) = (r.f(), r.f(), r.f(), r.f(), r.f());
            out_trig(&TrigonometricFunctionRoots::new_cde(c, d, e, inf, sup), g)?;
        }
        "trig_roots_de" => {
            let (d, e, inf, sup) = (r.f(), r.f(), r.f(), r.f());
            out_trig(&TrigonometricFunctionRoots::new_de(d, e, inf, sup), g)?;
        }
        "trig_equation_function" => {
            let (a, b, c, d, e, x) = (r.f(), r.f(), r.f(), r.f(), r.f(), r.f());
            let mut t = TrigonometricEquationFunction::new(a, b, c, d, e);
            let v = t.value(x);
            g.b(v.is_some());
            g.f(v.expect("a value"));
            let dv = t.derivative(x);
            g.b(dv.is_some());
            g.f(dv.expect("a derivative"));
            let w = t.values(x);
            g.b(w.is_some());
            let (v2, d2) = w.expect("values");
            g.f(v2);
            g.f(d2);
            g.i(t.state_number());
        }
        _ => panic!("unknown oracle family {name}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Comparison

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
            Tok::S(g) => assert_eq!(w, g, "value {index} (text)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn math1d_matches_occt() {
    let text = include_str!("math1d.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in math1d.txt:\n{line}");
        let (head, tail) = line.split_once(" | ").unwrap_or_else(|| panic!("line without ' | ': {line}"));
        let head: Vec<&str> = head.split_whitespace().collect();
        let want: Vec<&str> = tail.split_whitespace().collect();
        let (name, inputs) = head.split_first().expect("family name");
        let name: &str = name;
        let entry = stats.entry(name).or_default();
        entry.0 += 1;
        entry.1 += usize::from(!inputs.is_empty());

        let mut reader = Rd { words: inputs, at: 0 };
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
    let expected: BTreeSet<&str> = FAMILIES.iter().copied().collect();
    assert_eq!(present, expected, "family list changed");
    for name in EXACT {
        assert!(expected.contains(name), "EXACT names an unknown family {name}");
    }

    assert_eq!(lines, text.lines().count());
    assert_eq!(lines, GOLDEN_LINES, "golden line count changed");
    println!("{lines} lines, {} values, {} bit-identical, worst {} ulps", tally.values, tally.exact, tally.worst);
}
