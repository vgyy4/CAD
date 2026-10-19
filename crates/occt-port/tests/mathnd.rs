//! Numerical reference test for OCCT 8.0.1's multi-variable solvers and numerical integration of the `math` package:
//! the callback traits of `math_MultipleVarFunction*` and `math_FunctionSet*`, `math_FunctionSetRoot`,
//! `math_NewtonFunctionSetRoot`, `math_NewtonMinimum`, `math_BFGS`, `math_FRPR`, `math_Powell`, `math_Uzawa`,
//! `math_BissecNewton`, `math_GaussSingleIntegration`, `math_GaussMultipleIntegration`, `math_GaussSetIntegration` and
//! `math_KronrodSingleIntegration`.
//!
//! `mathnd.txt` is printed by `oracle/mathnd.cpp` (real OCCT). Each line is `<family> <inputs...> | <outputs...>`: f64
//! values are 16-hex-digit bit patterns and ints (and bools) are decimal. Every input of a call is on the left of
//! ` | `; this test reads them back with the `Rd` reader methods of the same name as the oracle's schema words
//! (`f i v iv m tf mv fs`, see `SCHEMAS`: every reader call is checked against the family's schema) and replays the
//! call on the Rust API of docs/api-mathnd.md. The right side is either the expected values, `throws` (OCCT raised)
//! or `unchecked` (a check that OCCT 8.0.1 as built has compiled out would have failed and the numbers are
//! unspecified); on both the Rust API must return an error.
//!
//! The solvers call user functions. Those are the closed set of test functions described at the top of
//! `oracle/mathnd.cpp` (`Tf`: one variable, as in `oracle/math1d.cpp`; `Mv`: a function of several variables built
//! from scalar terms; `Fs`: a system of equations built from the same terms), implemented here with the same
//! arithmetic (+ - * / floor and fabs only), so both sides see the same bits. Besides the numbers, every construction
//! line prints the number of callback calls and a hash of the call trace (with the bounds and every element of each
//! argument vector): the Rust solver must make the same calls in the same order.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::math1d::{Function, FunctionWithDerivative};
use occt_port::mathlin::{IntegerVector, MathError, MathStatus, Matrix, Vector};
use occt_port::mathnd::{
    Bfgs, BfgsState, BfgsTest, BissecNewton, BissecNewtonState, BissecNewtonTest, Frpr, FrprState, FrprTest,
    FunctionSet, FunctionSetRoot, FunctionSetRootState, FunctionSetRootTest, FunctionSetWithDerivatives,
    GaussMultipleIntegration, GaussSetIntegration, GaussSingleIntegration, KronrodSingleIntegration,
    MultipleVarFunction, MultipleVarFunctionWithGradient, MultipleVarFunctionWithHessian, NewtonFunctionSetRoot,
    NewtonFunctionSetRootState, NewtonFunctionSetRootTest, NewtonMinimum, NewtonMinimumState, NewtonMinimumTest,
    Powell, PowellState, PowellTest, Uzawa,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for results that are not computed with `+ - * /` and `sqrt` only, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1). Only the Kronrod families need it (`GKRule`
/// calls `pow`): their cases were kept only when a 1-ulp change of the C library's `pow` (and `sin`, `cos`, ...) moves
/// the result by at most 3 ulps.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once. There are none here.
const INPUT_LESS: &[&str] = &[];

/// Families whose results use only + - * / sqrt, floor and abs: compared bit for bit (signed zeros included).
/// Everything except the Kronrod families. The `test_*` families check the judge's own test functions.
const EXACT: &[&str] = &[
    "bfgs",
    "bfgs_access",
    "bfgs_bounded",
    "bfgs_custom",
    "bfgs_dump",
    "bissec_newton",
    "bissec_newton_access",
    "bissec_newton_custom",
    "bissec_newton_dump",
    "frpr",
    "frpr_access",
    "frpr_custom",
    "frpr_dump",
    "function_set_root",
    "function_set_root_access",
    "function_set_root_bounded",
    "function_set_root_custom",
    "function_set_root_dump",
    "function_set_root_staged",
    "gauss_multiple",
    "gauss_multiple_access",
    "gauss_multiple_dump",
    "gauss_set",
    "gauss_set_access",
    "gauss_set_dump",
    "gauss_single",
    "gauss_single_access",
    "gauss_single_dump",
    "gauss_single_tol",
    "newton_minimum",
    "newton_minimum_access",
    "newton_minimum_bounded",
    "newton_minimum_custom",
    "newton_minimum_dump",
    "newton_set_root",
    "newton_set_root_access",
    "newton_set_root_bounded",
    "newton_set_root_custom",
    "newton_set_root_dump",
    "newton_set_root_staged",
    "powell",
    "powell_access",
    "powell_custom",
    "powell_dump",
    "test_fs",
    "test_mv",
    "test_tf",
    "uzawa",
    "uzawa_access",
    "uzawa_dump",
    "uzawa_ineq",
];

/// Every family the oracle must print, with its schema (the words of its inputs, in order). A family that disappears
/// from mathnd.txt fails; every reader call is checked against the schema.
const SCHEMAS: &[(&str, &str)] = &[
    ("bfgs", "mv f i f v"),
    ("bfgs_access", "mv f i f v i v v i i i"),
    ("bfgs_bounded", "mv f i f v i v v i v"),
    ("bfgs_custom", "mv f i f v i f"),
    ("bfgs_dump", "mv f i f v"),
    ("bissec_newton", "f tf f f i"),
    ("bissec_newton_access", "f tf f f i i"),
    ("bissec_newton_custom", "f tf f f i i f"),
    ("bissec_newton_dump", "f tf f f i"),
    ("frpr", "mv f i f v i v"),
    ("frpr_access", "mv f i f v i i i"),
    ("frpr_custom", "mv f i f v i f"),
    ("frpr_dump", "mv f i f v"),
    ("function_set_root", "fs v i v i"),
    ("function_set_root_access", "fs v i v i v v i i i i i"),
    ("function_set_root_bounded", "fs v i v v v i"),
    ("function_set_root_custom", "fs v i v i v v i f"),
    ("function_set_root_dump", "fs v i v i"),
    ("function_set_root_staged", "fs i v i v v v i i v"),
    ("gauss_multiple", "mv v v iv"),
    ("gauss_multiple_access", "mv v v iv"),
    ("gauss_multiple_dump", "mv v v iv"),
    ("gauss_set", "fs v v iv"),
    ("gauss_set_access", "fs v v iv"),
    ("gauss_set_dump", "fs v v iv"),
    ("gauss_single", "tf f f i"),
    ("gauss_single_access", "i tf f f i f"),
    ("gauss_single_dump", "i tf f f i f"),
    ("gauss_single_tol", "tf f f i f"),
    ("kronrod", "tf f f i"),
    ("kronrod_access", "tf i f f i f i i"),
    ("kronrod_gk_rule", "tf f f v v v v"),
    ("kronrod_perform", "tf i f f i f i i f f i f i"),
    ("kronrod_tol", "tf f f i f i"),
    ("newton_minimum", "mv f i f i v"),
    ("newton_minimum_access", "mv f i f i v i v v i i i"),
    ("newton_minimum_bounded", "mv f i f i v i v v i v"),
    ("newton_minimum_custom", "mv f i f i v i f"),
    ("newton_minimum_dump", "mv f i f i v"),
    ("newton_set_root", "fs v f i v"),
    ("newton_set_root_access", "fs v f i v i i i i"),
    ("newton_set_root_bounded", "fs v f i v v v"),
    ("newton_set_root_custom", "fs v f i v i f"),
    ("newton_set_root_dump", "fs v f i v"),
    ("newton_set_root_staged", "fs f i v i v v v i v"),
    ("powell", "mv f i f v m i v"),
    ("powell_access", "mv f i f v m i i i"),
    ("powell_custom", "mv f i f v m i f"),
    ("powell_dump", "mv f i f v m"),
    ("test_fs", "fs v"),
    ("test_mv", "mv v"),
    ("test_tf", "tf f"),
    ("uzawa", "m v v f f i"),
    ("uzawa_access", "m v v i i f f i i"),
    ("uzawa_dump", "m v v i i f f i"),
    ("uzawa_ineq", "m v v i i f f i"),
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 2600;

// ---------------------------------------------------------------------------------
// The deterministic stand-ins for sin, cos and exp (the same as in oracle/math1d.cpp and oracle/mathnd.cpp)

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

const K_POLY: i32 = 0;
const K_TRIG: i32 = 1;
const K_RATIONAL: i32 = 2;
const K_MULTIPLE: i32 = 3;
const K_FLAT: i32 = 4;
const K_EXP: i32 = 5;
const K_ABS: i32 = 6;

/// `kind nparams p.. fail_lo fail_hi fail_mode state_mode`.
struct Tf {
    kind: i32,
    p: Vec<f64>,
    fail_lo: f64,
    fail_hi: f64,
    fail_mode: i32,
    state_mode: i32,
    tables: Tables,
    nv: i32,
    nd: i32,
    nw: i32,
    ns: i32,
    trace: Trace,
}

impl Tf {
    fn eval(&self, x: f64) -> Option<(f64, f64)> {
        let p = &self.p;
        let t = &self.tables;
        match self.kind {
            K_POLY => {
                let n = p.len();
                let mut v = p[n - 1];
                for i in (0..n - 1).rev() {
                    v = v * x + p[i];
                }
                let mut d = (n - 1) as f64 * p[n - 1];
                for i in (1..n - 1).rev() {
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
                Some((p[0] * pm * (x - p[3]), p[0] * (f64::from(m) * pm1 * (x - p[3]) + pm)))
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
    fn fails(&self, x: f64, bit: i32) -> bool {
        (self.fail_mode & bit) != 0 && x >= self.fail_lo && x <= self.fail_hi
    }
    fn begin_call(&mut self, tag: u8, x: f64, code: i64) {
        assert!(x.is_finite(), "the solver called the function with a non-finite argument");
        self.trace.tagx(tag, x);
        self.trace.last = code;
        self.trace.calls += 1;
    }
}

impl Function for Tf {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.nv += 1;
        self.begin_call(b'V', x, 1);
        if self.fails(x, 1) {
            return None;
        }
        self.eval(x).map(|(v, _)| v)
    }
    fn state_number(&mut self) -> i32 {
        self.ns += 1;
        self.trace.tag(b'S');
        self.trace.state(self.state_mode)
    }
}

impl FunctionWithDerivative for Tf {
    fn derivative(&mut self, x: f64) -> Option<f64> {
        self.nd += 1;
        self.begin_call(b'D', x, 2);
        if self.fails(x, 2) {
            return None;
        }
        self.eval(x).map(|(_, d)| d)
    }
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        self.nw += 1;
        self.begin_call(b'W', x, 3);
        if self.fails(x, 4) {
            return None;
        }
        self.eval(x)
    }
}

// ---------------------------------------------------------------------------------
// The functions of several variables (the same as in oracle/mathnd.cpp)

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

/// The common part of `Mv` and `Fs`: `n`, the terms, the failure box and the state mode.
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

/// A system: equation k is term k.
struct Fs {
    s: Spec,
    nv: i32,
    nd: i32,
    nw: i32,
    ns: i32,
}

impl Fs {
    fn neq(&self) -> usize {
        self.s.terms.len()
    }
    fn f_values(&self, x: &[f64]) -> Vec<f64> {
        self.s.terms.iter().map(|g| g.value(self.s.n, x, &self.s.tables)).collect()
    }
    fn jacobian(&self, x: &[f64]) -> Vec<f64> {
        self.s.terms.iter().flat_map(|g| g.grad(self.s.n, x, &self.s.tables)).collect()
    }
}

impl FunctionSet for Fs {
    fn nb_variables(&self) -> i32 {
        self.s.n as i32
    }
    fn nb_equations(&self) -> i32 {
        self.neq() as i32
    }
    fn value(&mut self, x: &Vector, f: &mut Vector) -> bool {
        self.nv += 1;
        let xs = self.s.begin(b'V', x, 1);
        assert_eq!(f.length() as usize, self.neq(), "value vector of another length");
        if self.s.fails(&xs, 1) {
            return false;
        }
        put_vector(f, &self.f_values(&xs));
        true
    }
    fn state_number(&mut self) -> i32 {
        self.ns += 1;
        self.s.trace.tag(b'S');
        self.s.trace.state(self.s.state_mode)
    }
}

impl FunctionSetWithDerivatives for Fs {
    fn derivatives(&mut self, x: &Vector, d: &mut Matrix) -> bool {
        self.nd += 1;
        let xs = self.s.begin(b'D', x, 2);
        let (neq, n) = (self.neq(), self.s.n);
        assert_eq!((d.row_number() as usize, d.col_number() as usize), (neq, n), "Jacobian of another shape");
        if self.s.fails(&xs, 2) {
            return false;
        }
        put_matrix(d, neq, n, &self.jacobian(&xs));
        true
    }
    fn values(&mut self, x: &Vector, f: &mut Vector, d: &mut Matrix) -> bool {
        self.nw += 1;
        let xs = self.s.begin(b'W', x, 3);
        let (neq, n) = (self.neq(), self.s.n);
        assert_eq!(f.length() as usize, neq, "value vector of another length");
        assert_eq!((d.row_number() as usize, d.col_number() as usize), (neq, n), "Jacobian of another shape");
        if self.s.fails(&xs, 4) {
            return false;
        }
        put_vector(f, &self.f_values(&xs));
        put_matrix(d, neq, n, &self.jacobian(&xs));
        true
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
    fn counts_tf(&mut self, f: &Tf) {
        for c in [f.nv, f.nd, f.nw, f.ns] {
            self.i(c);
        }
        self.0.push(Tok::H(f.trace.hash));
    }
    fn counts_mv(&mut self, f: &Mv) {
        for c in [f.nv, f.ng, f.nw, f.nh, f.ns] {
            self.i(c);
        }
        self.0.push(Tok::H(f.s.trace.hash));
    }
    fn counts_fs(&mut self, f: &Fs) {
        for c in [f.nv, f.nd, f.nw, f.ns] {
            self.i(c);
        }
        self.0.push(Tok::H(f.s.trace.hash));
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
    /// An integer vector: lower length k...
    fn iv(&mut self) -> IntegerVector {
        self.expect("iv");
        let lower = self.raw_i();
        let n = self.raw_n();
        let x: Vec<i32> = (0..n).map(|_| self.raw_i()).collect();
        IntegerVector::from_slice(lower, &x).unwrap()
    }
    /// A matrix: lower_row rows lower_col cols a_11 a_12 ...
    fn m(&mut self) -> Matrix {
        self.expect("m");
        let (lr, rows, lc, cols) = (self.raw_i(), self.raw_i(), self.raw_i(), self.raw_i());
        let a: Vec<f64> = (0..rows * cols).map(|_| self.raw_f()).collect();
        Matrix::from_slice(lr, lc, rows, cols, &a).unwrap()
    }
    /// A one-variable test function: kind, parameter count, parameters, fail_lo, fail_hi, fail_mode, state_mode.
    fn tf(&mut self) -> Tf {
        self.expect("tf");
        let kind = self.raw_i();
        let n = self.raw_n();
        let p: Vec<f64> = (0..n).map(|_| self.raw_f()).collect();
        let (fail_lo, fail_hi) = (self.raw_f(), self.raw_f());
        let (fail_mode, state_mode) = (self.raw_i(), self.raw_i());
        Tf {
            kind,
            p,
            fail_lo,
            fail_hi,
            fail_mode,
            state_mode,
            tables: Tables::new(),
            nv: 0,
            nd: 0,
            nw: 0,
            ns: 0,
            trace: Trace::new(),
        }
    }
    fn terms(&mut self, count: usize) -> Vec<Term> {
        (0..count)
            .map(|_| {
                let kind = self.raw_i();
                let np = self.raw_n();
                Term { kind, p: (0..np).map(|_| self.raw_f()).collect() }
            })
            .collect()
    }
    fn spec_tail(&mut self, n: usize, mode: i32, terms: Vec<Term>) -> Spec {
        let fail_mode = self.raw_i();
        let lo: Vec<f64> = (0..n).map(|_| self.raw_f()).collect();
        let hi: Vec<f64> = (0..n).map(|_| self.raw_f()).collect();
        let state_mode = self.raw_i();
        Spec { n, mode, terms, fail_mode, lo, hi, state_mode, tables: Tables::new(), trace: Trace::new() }
    }
    /// A function of several variables: n mode nterms terms.. fail_mode lo.. hi.. state_mode.
    fn mv(&mut self) -> Mv {
        self.expect("mv");
        let n = self.raw_n();
        let mode = self.raw_i();
        let count = self.raw_n();
        let terms = self.terms(count);
        Mv { s: self.spec_tail(n, mode, terms), nv: 0, ng: 0, nw: 0, nh: 0, ns: 0 }
    }
    /// A system: n neq equations.. fail_mode lo.. hi.. state_mode.
    fn fs(&mut self) -> Fs {
        self.expect("fs");
        let n = self.raw_n();
        let count = self.raw_n();
        let terms = self.terms(count);
        Fs { s: self.spec_tail(n, 0, terms), nv: 0, nd: 0, nw: 0, ns: 0 }
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
        assert_eq!(self.next, self.schema.len(), "unread schema words");
    }
}

// ---------------------------------------------------------------------------------
// The solution tests of the `*_custom` families (the sub-classes of the oracle)

fn sum_sq(v: &Vector) -> f64 {
    let mut s = 0.0;
    for x in v.as_slice() {
        s += x * x;
    }
    s
}

fn first(v: &Vector) -> f64 {
    v.value(v.lower()).unwrap()
}

fn first_m(m: &Matrix) -> f64 {
    m.value(m.lower_row(), m.lower_col()).unwrap()
}

/// `FSRCustom`.
struct FsrCustom {
    mode: i32,
    thr: f64,
}

impl FunctionSetRootTest for FsrCustom {
    fn is_solution_reached(&mut self, st: &FunctionSetRootState<'_>, f: &mut dyn FunctionSetWithDerivatives) -> bool {
        let def = st.is_solution_reached_default();
        match self.mode {
            1 => def || sum_sq(st.delta) <= self.thr,
            2 => {
                // the function is called whatever the default test says
                let mut fv = Vector::new(1, f.nb_equations()).unwrap();
                let ok = f.value(st.sol, &mut fv);
                let s = if ok { sum_sq(&fv) } else { 0.0 };
                def || (ok && s <= self.thr)
            }
            3 => false,
            _ => def && first_m(st.df).abs() + first(st.tol) >= self.thr,
        }
    }
}

/// `NSRCustom`.
struct NsrCustom {
    mode: i32,
    thr: f64,
}

impl NewtonFunctionSetRootTest for NsrCustom {
    fn is_solution_reached(
        &mut self,
        st: &NewtonFunctionSetRootState<'_>,
        f: &mut dyn FunctionSetWithDerivatives,
    ) -> bool {
        let def = st.is_solution_reached_default();
        match self.mode {
            1 => def || sum_sq(st.f_values) <= self.thr,
            2 => {
                let mut fv = Vector::new(1, f.nb_equations()).unwrap();
                let ok = f.value(st.sol, &mut fv);
                def || (ok && fv.value(1).unwrap().abs() <= self.thr)
            }
            3 => false,
            _ => {
                def && first_m(st.jacobian).abs() + first(st.tol_x) + st.tol_f >= self.thr
                    && first(st.delta_x).abs() < 1e300
            }
        }
    }
}

/// `NMCustom`.
struct NmCustom {
    mode: i32,
    thr: f64,
}

impl NewtonMinimumTest for NmCustom {
    fn is_converged(&mut self, st: &NewtonMinimumState<'_>) -> bool {
        let def = st.is_converged_default();
        match self.mode {
            1 => def || (st.minimum - st.previous_minimum).abs() <= self.thr,
            2 => def && st.nb_iter >= 3,
            3 => false,
            _ => {
                (def && sum_sq(st.gradient) <= self.thr)
                    || (st.min_eigen_value > 1e300
                        && st.convex
                        && st.hessian.value(1, 1).unwrap() + st.x_tol + st.c_tol > 0.0)
            }
        }
    }
}

/// `BFGSCustom`.
struct BfgsCustom {
    mode: i32,
    thr: f64,
}

impl BfgsTest for BfgsCustom {
    fn is_solution_reached(&mut self, st: &BfgsState<'_>, f: &mut dyn MultipleVarFunctionWithGradient) -> bool {
        let def = st.is_solution_reached_default();
        match self.mode {
            1 => def || (st.minimum - st.previous_minimum).abs() <= self.thr,
            2 => {
                let v = f.value(st.location);
                def || v.is_some_and(|v| v <= self.thr)
            }
            3 => false,
            _ => def && (sum_sq(st.gradient) <= self.thr || st.nb_iter >= 3) && st.x_tol + st.eps_z >= 0.0,
        }
    }
}

/// `FRPRCustom`.
struct FrprCustom {
    mode: i32,
    thr: f64,
}

impl FrprTest for FrprCustom {
    fn is_solution_reached(&mut self, st: &FrprState<'_>, f: &mut dyn MultipleVarFunctionWithGradient) -> bool {
        let def = st.is_solution_reached_default();
        match self.mode {
            1 => def || (st.minimum - st.previous_minimum).abs() <= self.thr,
            2 => {
                let v = f.value(st.location);
                def || v.is_some_and(|v| v <= self.thr)
            }
            3 => false,
            _ => def && sum_sq(st.gradient) <= self.thr + st.x_tol + st.eps_z,
        }
    }
}

/// `PowellCustom`.
struct PowellCustom {
    mode: i32,
    thr: f64,
}

impl PowellTest for PowellCustom {
    fn is_solution_reached(&mut self, st: &PowellState<'_>, f: &mut dyn MultipleVarFunction) -> bool {
        let def = st.is_solution_reached_default();
        match self.mode {
            1 => def || (st.minimum - st.previous_minimum).abs() <= self.thr,
            2 => {
                let v = f.value(st.location);
                def || v.is_some_and(|v| v <= self.thr)
            }
            3 => false,
            _ => def && first(st.location).abs() + st.x_tol + st.eps_z >= self.thr && st.location_error > 0.0,
        }
    }
}

/// `BNCustom`.
struct BnCustom {
    mode: i32,
    thr: f64,
}

impl BissecNewtonTest for BnCustom {
    fn is_solution_reached(&mut self, st: &BissecNewtonState, f: &mut dyn FunctionWithDerivative) -> bool {
        let def = st.is_solution_reached_default();
        match self.mode {
            1 => def || st.f.abs() <= self.thr,
            2 => {
                let v = f.value(st.x);
                def || v.is_some_and(|v| v.abs() <= self.thr)
            }
            3 => false,
            _ => def && st.df.abs() + st.x_tol >= self.thr,
        }
    }
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn status_code(s: MathStatus) -> i32 {
    match s {
        MathStatus::Ok => 0,
        MathStatus::TooManyIterations => 1,
        MathStatus::FunctionError => 2,
        MathStatus::DirectionSearchError => 3,
        MathStatus::NotBracketed => 4,
    }
}

/// A vector `lower..lower+length-1` of zeros, and a matrix of zeros (the oracle's output arguments).
fn zeros(lower: i32, length: i32) -> Vector {
    Vector::new(lower, lower + length - 1).unwrap()
}

fn zeros_m(lower: i32, rows: i32, cols: i32) -> Matrix {
    Matrix::new(lower, lower + rows - 1, lower, lower + cols - 1).unwrap()
}

fn out_fsr(s: &FunctionSetRoot, f: &Fs, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    g.b(s.is_divergent());
    g.counts_fs(f);
    if s.is_done() {
        g.v(s.root()?);
        g.i(s.nb_iterations()?);
        g.i(s.state_number()?);
        g.m(s.derivative()?);
    }
    Ok(())
}

fn out_nsr(s: &NewtonFunctionSetRoot, f: &Fs, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    g.counts_fs(f);
    if s.is_done() {
        g.v(s.root()?);
        g.m(s.derivative()?);
        g.v(s.function_set_errors()?);
        g.i(s.nb_iterations()?);
    }
    Ok(())
}

fn out_nm(s: &NewtonMinimum, f: &Mv, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    g.i(status_code(s.get_status()));
    g.counts_mv(f);
    if s.is_done() {
        g.v(s.location()?);
        g.f(s.minimum()?);
        g.v(s.gradient()?);
        g.i(s.nb_iterations()?);
    }
    Ok(())
}

fn out_bfgs(s: &Bfgs, f: &Mv, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    g.counts_mv(f);
    if s.is_done() {
        g.v(s.location()?);
        g.f(s.minimum()?);
        g.v(s.gradient()?);
        g.i(s.nb_iterations()?);
    }
    Ok(())
}

fn out_frpr(s: &Frpr, f: &Mv, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    g.counts_mv(f);
    if s.is_done() {
        g.v(s.location()?);
        g.f(s.minimum()?);
        g.v(s.gradient()?);
        g.i(s.nb_iterations()?);
    }
    Ok(())
}

fn out_powell(s: &Powell, f: &Mv, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    g.counts_mv(f);
    if s.is_done() {
        g.v(s.location()?);
        g.f(s.minimum()?);
        g.i(s.nb_iterations()?);
    }
    Ok(())
}

/// The first `Cont.RowNumber()` elements of `InitialError` (OCCT writes no others).
fn initial_error_head(u: &Uzawa, nlig: i32, g: &mut Got) -> Result<(), Thrown> {
    let e = u.initial_error()?;
    g.i(e.lower());
    g.i(e.length());
    for i in 1..=nlig.min(e.length()) {
        g.f(e.value(i)?);
    }
    Ok(())
}

fn out_uzawa(u: &Uzawa, nci: i32, nlig: i32, g: &mut Got) -> Result<(), Thrown> {
    g.b(u.is_done());
    if u.is_done() {
        g.v(u.value()?);
        initial_error_head(u, nlig, g)?;
        g.v(u.error()?);
        g.i(u.nb_iterations()?);
        if nci == 0 {
            g.m(u.inverse_cont()?);
        }
    }
    g.v(&u.duale());
    Ok(())
}

fn out_bissec(s: &BissecNewton, f: &Tf, g: &mut Got) -> Result<(), Thrown> {
    g.b(s.is_done());
    g.counts_tf(f);
    if s.is_done() {
        g.f(s.root()?);
        g.f(s.derivative()?);
        g.f(s.value()?);
    }
    Ok(())
}

fn out_kronrod(k: &KronrodSingleIntegration, f: &Tf, g: &mut Got) -> Result<(), Thrown> {
    g.b(k.is_done());
    g.counts_tf(f);
    if k.is_done() {
        g.f(k.value()?);
        g.f(k.error_reached()?);
        g.f(k.absolut_error()?);
        g.i(k.order_reached()?);
        g.i(k.nb_iter_reached()?);
    }
    Ok(())
}

/// The inputs shared by the minimiser families: function, tolerance, iteration limit.
fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        // ---- the test functions themselves ----
        "test_tf" => {
            let mut f = r.tf();
            let x = r.f();
            let v = f.value(x);
            g.b(v.is_some());
            if let Some(v) = v {
                g.f(v);
            }
            let d = f.derivative(x);
            g.b(d.is_some());
            if let Some(d) = d {
                g.f(d);
            }
            let w = f.values(x);
            g.b(w.is_some());
            if let Some((v, d)) = w {
                g.f(v);
                g.f(d);
            }
            let st = f.state_number();
            g.i(st);
            g.counts_tf(&f);
        }
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
        "test_fs" => {
            let mut f = r.fs();
            let x = r.v();
            let (n, neq) = (f.nb_variables(), f.nb_equations());
            let mut fv = zeros(1, neq);
            let ok = f.value(&x, &mut fv);
            g.b(ok);
            if ok {
                g.v(&fv);
            }
            let mut d = zeros_m(1, neq, n);
            let ok = f.derivatives(&x, &mut d);
            g.b(ok);
            if ok {
                g.m(&d);
            }
            let mut f2 = zeros(1, neq);
            let mut d2 = zeros_m(1, neq, n);
            let ok = f.values(&x, &mut f2, &mut d2);
            g.b(ok);
            if ok {
                g.v(&f2);
                g.m(&d2);
            }
            let st = f.state_number();
            g.i(st);
            g.i(f.nb_variables());
            g.i(f.nb_equations());
            g.counts_fs(&f);
        }
        // ---- math_FunctionSetRoot ----
        "function_set_root" | "function_set_root_dump" => {
            let mut f = r.fs();
            let (tol, nbit, start, stop) = (r.v(), r.i(), r.v(), r.i() != 0);
            let mut s = FunctionSetRoot::new(&f, &tol, nbit)?;
            s.perform(&mut f, &start, stop)?;
            if name == "function_set_root" {
                out_fsr(&s, &f, g)?;
            } else {
                g.dump(&s.dump());
            }
        }
        "function_set_root_bounded" => {
            let mut f = r.fs();
            let (tol, nbit, start, inf, sup, stop) = (r.v(), r.i(), r.v(), r.v(), r.v(), r.i() != 0);
            let mut s = FunctionSetRoot::new(&f, &tol, nbit)?;
            s.perform_bounded(&mut f, &start, &inf, &sup, stop)?;
            out_fsr(&s, &f, g)?;
        }
        "function_set_root_staged" => {
            let mut f = r.fs();
            let (nbit, tol, bounded, start, inf, sup) = (r.i(), r.v(), r.i() != 0, r.v(), r.v(), r.v());
            let (stop, twice, start2) = (r.i() != 0, r.i(), r.v());
            let mut s = FunctionSetRoot::without_tolerance(&f, nbit)?;
            s.set_tolerance(&tol)?;
            for k in 0..=twice {
                let x0 = if k == 0 { &start } else { &start2 };
                if bounded {
                    s.perform_bounded(&mut f, x0, &inf, &sup, stop)?;
                } else {
                    s.perform(&mut f, x0, stop)?;
                }
            }
            out_fsr(&s, &f, g)?;
        }
        "function_set_root_custom" => {
            let mut f = r.fs();
            let (tol, nbit, start, bounded, inf, sup) = (r.v(), r.i(), r.v(), r.i() != 0, r.v(), r.v());
            let mut test = FsrCustom { mode: r.i(), thr: r.f() };
            let mut s = FunctionSetRoot::new(&f, &tol, nbit)?;
            if bounded {
                s.perform_bounded_with_test(&mut f, &start, &inf, &sup, false, &mut test)?;
            } else {
                s.perform_with_test(&mut f, &start, false, &mut test)?;
            }
            out_fsr(&s, &f, g)?;
        }
        "function_set_root_access" => {
            let mut f = r.fs();
            let (tol, nbit, start, bounded, inf, sup) = (r.v(), r.i(), r.v(), r.i() != 0, r.v(), r.v());
            let (stop, q, olow, orows, olen) = (r.i() != 0, r.i(), r.i(), r.i(), r.i());
            let mut s = FunctionSetRoot::new(&f, &tol, nbit)?;
            if bounded {
                s.perform_bounded(&mut f, &start, &inf, &sup, stop)?;
            } else {
                s.perform(&mut f, &start, stop)?;
            }
            let mut out = zeros(olow, olen);
            let mut outm = zeros_m(olow, orows, olen);
            match q {
                0 => g.v(s.root()?),
                1 => {
                    s.root_into(&mut out)?;
                    g.v(&out);
                }
                2 => g.m(s.derivative()?),
                3 => {
                    s.derivative_into(&mut outm)?;
                    g.m(&outm);
                }
                4 => g.v(s.function_set_errors()?),
                5 => {
                    s.function_set_errors_into(&mut out)?;
                    g.v(&out);
                }
                6 => g.i(s.nb_iterations()?),
                7 => g.i(s.state_number()?),
                _ => g.b(s.is_divergent()),
            }
        }
        // ---- math_NewtonFunctionSetRoot ----
        "newton_set_root" | "newton_set_root_dump" => {
            let mut f = r.fs();
            let (xtol, ftol, nbit, start) = (r.v(), r.f(), r.i(), r.v());
            let mut s = NewtonFunctionSetRoot::new(&f, &xtol, ftol, nbit)?;
            s.perform(&mut f, &start)?;
            if name == "newton_set_root" {
                out_nsr(&s, &f, g)?;
            } else {
                g.dump(&s.dump());
            }
        }
        "newton_set_root_bounded" => {
            let mut f = r.fs();
            let (xtol, ftol, nbit, start, inf, sup) = (r.v(), r.f(), r.i(), r.v(), r.v(), r.v());
            let mut s = NewtonFunctionSetRoot::new(&f, &xtol, ftol, nbit)?;
            s.perform_bounded(&mut f, &start, &inf, &sup)?;
            out_nsr(&s, &f, g)?;
        }
        "newton_set_root_staged" => {
            let mut f = r.fs();
            let (ftol, nbit, xtol, bounded, start, inf, sup) = (r.f(), r.i(), r.v(), r.i() != 0, r.v(), r.v(), r.v());
            let (twice, start2) = (r.i(), r.v());
            let mut s = NewtonFunctionSetRoot::without_tolerance(&f, ftol, nbit)?;
            s.set_tolerance(&xtol)?;
            for k in 0..=twice {
                let x0 = if k == 0 { &start } else { &start2 };
                if bounded {
                    s.perform_bounded(&mut f, x0, &inf, &sup)?;
                } else {
                    s.perform(&mut f, x0)?;
                }
            }
            out_nsr(&s, &f, g)?;
        }
        "newton_set_root_custom" => {
            let mut f = r.fs();
            let (xtol, ftol, nbit, start) = (r.v(), r.f(), r.i(), r.v());
            let mut test = NsrCustom { mode: r.i(), thr: r.f() };
            let mut s = NewtonFunctionSetRoot::new(&f, &xtol, ftol, nbit)?;
            s.perform_with_test(&mut f, &start, &mut test)?;
            out_nsr(&s, &f, g)?;
        }
        "newton_set_root_access" => {
            let mut f = r.fs();
            let (xtol, ftol, nbit, start) = (r.v(), r.f(), r.i(), r.v());
            let (q, olow, orows, olen) = (r.i(), r.i(), r.i(), r.i());
            let mut s = NewtonFunctionSetRoot::new(&f, &xtol, ftol, nbit)?;
            s.perform(&mut f, &start)?;
            let mut out = zeros(olow, olen);
            let mut outm = zeros_m(olow, orows, olen);
            match q {
                0 => g.v(s.root()?),
                1 => {
                    s.root_into(&mut out)?;
                    g.v(&out);
                }
                2 => g.m(s.derivative()?),
                3 => {
                    s.derivative_into(&mut outm)?;
                    g.m(&outm);
                }
                4 => g.v(s.function_set_errors()?),
                5 => {
                    s.function_set_errors_into(&mut out)?;
                    g.v(&out);
                }
                _ => g.i(s.nb_iterations()?),
            }
        }
        // ---- math_NewtonMinimum ----
        "newton_minimum" | "newton_minimum_dump" => {
            let mut f = r.mv();
            let (tol, nbit, conv, sing, start) = (r.f(), r.i(), r.f(), r.i() != 0, r.v());
            let mut s = NewtonMinimum::new(&f, tol, nbit, conv, sing)?;
            s.perform(&mut f, &start)?;
            if name == "newton_minimum" {
                out_nm(&s, &f, g)?;
            } else {
                g.dump(&s.dump()?);
            }
        }
        "newton_minimum_bounded" => {
            let mut f = r.mv();
            let (tol, nbit, conv, sing, start) = (r.f(), r.i(), r.f(), r.i() != 0, r.v());
            let (setb, left, right, twice, start2) = (r.i() != 0, r.v(), r.v(), r.i() != 0, r.v());
            let mut s = NewtonMinimum::new(&f, tol, nbit, conv, sing)?;
            if setb {
                s.set_boundary(&left, &right)?;
            }
            s.perform(&mut f, &start)?;
            if twice {
                s.perform(&mut f, &start2)?;
            }
            out_nm(&s, &f, g)?;
        }
        "newton_minimum_custom" => {
            let mut f = r.mv();
            let (tol, nbit, conv, sing, start) = (r.f(), r.i(), r.f(), r.i() != 0, r.v());
            let mut test = NmCustom { mode: r.i(), thr: r.f() };
            let mut s = NewtonMinimum::new(&f, tol, nbit, conv, sing)?;
            s.perform_with_test(&mut f, &start, &mut test)?;
            out_nm(&s, &f, g)?;
        }
        "newton_minimum_access" => {
            let mut f = r.mv();
            let (tol, nbit, conv, sing, start) = (r.f(), r.i(), r.f(), r.i() != 0, r.v());
            let (setb, left, right) = (r.i() != 0, r.v(), r.v());
            let (q, olow, olen) = (r.i(), r.i(), r.i());
            let mut s = NewtonMinimum::new(&f, tol, nbit, conv, sing)?;
            if setb {
                s.set_boundary(&left, &right)?;
            }
            s.perform(&mut f, &start)?;
            let mut out = zeros(olow, olen);
            match q {
                0 => g.v(s.location()?),
                1 => {
                    s.location_into(&mut out)?;
                    g.v(&out);
                }
                2 => g.f(s.minimum()?),
                3 => g.v(s.gradient()?),
                4 => {
                    s.gradient_into(&mut out)?;
                    g.v(&out);
                }
                5 => g.i(s.nb_iterations()?),
                _ => g.i(status_code(s.get_status())),
            }
        }
        // ---- math_BFGS ----
        "bfgs" | "bfgs_dump" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start) = (r.f(), r.i(), r.f(), r.v());
            let mut s = Bfgs::new(f.nb_variables(), tol, nbit, zeps)?;
            s.perform(&mut f, &start)?;
            if name == "bfgs" {
                out_bfgs(&s, &f, g)?;
            } else {
                g.dump(&s.dump());
            }
        }
        "bfgs_bounded" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start) = (r.f(), r.i(), r.f(), r.v());
            let (setb, left, right, twice, start2) = (r.i() != 0, r.v(), r.v(), r.i() != 0, r.v());
            let mut s = Bfgs::new(f.nb_variables(), tol, nbit, zeps)?;
            if setb {
                s.set_boundary(&left, &right)?;
            }
            s.perform(&mut f, &start)?;
            if twice {
                s.perform(&mut f, &start2)?;
            }
            out_bfgs(&s, &f, g)?;
        }
        "bfgs_custom" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start) = (r.f(), r.i(), r.f(), r.v());
            let mut test = BfgsCustom { mode: r.i(), thr: r.f() };
            let mut s = Bfgs::new(f.nb_variables(), tol, nbit, zeps)?;
            s.perform_with_test(&mut f, &start, &mut test)?;
            out_bfgs(&s, &f, g)?;
        }
        "bfgs_access" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start) = (r.f(), r.i(), r.f(), r.v());
            let (setb, left, right) = (r.i() != 0, r.v(), r.v());
            let (q, olow, olen) = (r.i(), r.i(), r.i());
            let mut s = Bfgs::new(f.nb_variables(), tol, nbit, zeps)?;
            if setb {
                s.set_boundary(&left, &right)?;
            }
            s.perform(&mut f, &start)?;
            let mut out = zeros(olow, olen);
            match q {
                0 => g.v(s.location()?),
                1 => {
                    s.location_into(&mut out)?;
                    g.v(&out);
                }
                2 => g.f(s.minimum()?),
                3 => g.v(s.gradient()?),
                4 => {
                    s.gradient_into(&mut out)?;
                    g.v(&out);
                }
                _ => g.i(s.nb_iterations()?),
            }
        }
        // ---- math_FRPR ----
        "frpr" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start, twice, start2) = (r.f(), r.i(), r.f(), r.v(), r.i() != 0, r.v());
            let mut s = Frpr::new(&f, tol, nbit, zeps)?;
            s.perform(&mut f, &start)?;
            if twice {
                s.perform(&mut f, &start2)?;
            }
            out_frpr(&s, &f, g)?;
        }
        "frpr_dump" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start) = (r.f(), r.i(), r.f(), r.v());
            let mut s = Frpr::new(&f, tol, nbit, zeps)?;
            s.perform(&mut f, &start)?;
            g.dump(&s.dump());
        }
        "frpr_custom" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start) = (r.f(), r.i(), r.f(), r.v());
            let mut test = FrprCustom { mode: r.i(), thr: r.f() };
            let mut s = Frpr::new(&f, tol, nbit, zeps)?;
            s.perform_with_test(&mut f, &start, &mut test)?;
            out_frpr(&s, &f, g)?;
        }
        "frpr_access" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start) = (r.f(), r.i(), r.f(), r.v());
            let (q, olow, olen) = (r.i(), r.i(), r.i());
            let mut s = Frpr::new(&f, tol, nbit, zeps)?;
            s.perform(&mut f, &start)?;
            let mut out = zeros(olow, olen);
            match q {
                0 => g.v(s.location()?),
                1 => {
                    s.location_into(&mut out)?;
                    g.v(&out);
                }
                2 => g.f(s.minimum()?),
                3 => g.v(s.gradient()?),
                4 => {
                    s.gradient_into(&mut out)?;
                    g.v(&out);
                }
                _ => g.i(s.nb_iterations()?),
            }
        }
        // ---- math_Powell ----
        "powell" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start, dirs) = (r.f(), r.i(), r.f(), r.v(), r.m());
            let (twice, start2) = (r.i() != 0, r.v());
            let mut s = Powell::new(&f, tol, nbit, zeps)?;
            s.perform(&mut f, &start, &dirs)?;
            if twice {
                s.perform(&mut f, &start2, &dirs)?;
            }
            out_powell(&s, &f, g)?;
        }
        "powell_dump" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start, dirs) = (r.f(), r.i(), r.f(), r.v(), r.m());
            let mut s = Powell::new(&f, tol, nbit, zeps)?;
            s.perform(&mut f, &start, &dirs)?;
            g.dump(&s.dump());
        }
        "powell_custom" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start, dirs) = (r.f(), r.i(), r.f(), r.v(), r.m());
            let mut test = PowellCustom { mode: r.i(), thr: r.f() };
            let mut s = Powell::new(&f, tol, nbit, zeps)?;
            s.perform_with_test(&mut f, &start, &dirs, &mut test)?;
            out_powell(&s, &f, g)?;
        }
        "powell_access" => {
            let mut f = r.mv();
            let (tol, nbit, zeps, start, dirs) = (r.f(), r.i(), r.f(), r.v(), r.m());
            let (q, olow, olen) = (r.i(), r.i(), r.i());
            let mut s = Powell::new(&f, tol, nbit, zeps)?;
            s.perform(&mut f, &start, &dirs)?;
            let mut out = zeros(olow, olen);
            match q {
                0 => g.v(s.location()?),
                1 => {
                    s.location_into(&mut out)?;
                    g.v(&out);
                }
                2 => g.f(s.minimum()?),
                _ => g.i(s.nb_iterations()?),
            }
        }
        // ---- math_Uzawa ----
        "uzawa" => {
            let (cont, secont, start) = (r.m(), r.v(), r.v());
            let (elx, elc, nbit) = (r.f(), r.f(), r.i());
            let u = Uzawa::new(&cont, &secont, &start, elx, elc, nbit)?;
            out_uzawa(&u, 0, cont.row_number(), g)?;
        }
        "uzawa_ineq" | "uzawa_access" | "uzawa_dump" => {
            let (cont, secont, start) = (r.m(), r.v(), r.v());
            let (nce, nci, elx, elc, nbit) = (r.i(), r.i(), r.f(), r.f(), r.i());
            let q = if name == "uzawa_access" { r.i() } else { 0 };
            let u = Uzawa::with_constraints(&cont, &secont, &start, nce, nci, elx, elc, nbit)?;
            let nlig = cont.row_number();
            match name {
                "uzawa_ineq" => out_uzawa(&u, nci, nlig, g)?,
                "uzawa_dump" => g.dump(&u.dump()),
                _ => match q {
                    0 => g.v(u.value()?),
                    1 => initial_error_head(&u, nlig, g)?,
                    2 => g.v(u.error()?),
                    3 => g.i(u.nb_iterations()?),
                    4 => g.m(u.inverse_cont()?),
                    5 => g.v(&u.duale()),
                    _ => g.b(u.is_done()),
                },
            }
        }
        // ---- math_BissecNewton ----
        "bissec_newton" | "bissec_newton_access" | "bissec_newton_dump" => {
            let xtol = r.f();
            let mut f = r.tf();
            let (b1, b2, nbit) = (r.f(), r.f(), r.i());
            let mut s = BissecNewton::new(xtol);
            s.perform(&mut f, b1, b2, nbit);
            match name {
                "bissec_newton" => out_bissec(&s, &f, g)?,
                "bissec_newton_dump" => g.dump(&s.dump()),
                _ => match r.i() {
                    0 => g.f(s.root()?),
                    1 => g.f(s.derivative()?),
                    _ => g.f(s.value()?),
                },
            }
        }
        "bissec_newton_custom" => {
            let xtol = r.f();
            let mut f = r.tf();
            let (b1, b2, nbit) = (r.f(), r.f(), r.i());
            let mut test = BnCustom { mode: r.i(), thr: r.f() };
            let mut s = BissecNewton::new(xtol);
            s.perform_with_test(&mut f, b1, b2, nbit, &mut test);
            out_bissec(&s, &f, g)?;
        }
        // ---- math_GaussSingleIntegration ----
        "gauss_single" => {
            let mut f = r.tf();
            let (a, b, order) = (r.f(), r.f(), r.i());
            let gi = GaussSingleIntegration::with_order(&mut f, a, b, order);
            g.b(gi.is_done());
            g.counts_tf(&f);
            if gi.is_done() {
                g.f(gi.value()?);
            }
        }
        "gauss_single_tol" => {
            let mut f = r.tf();
            let (a, b, order, tol) = (r.f(), r.f(), r.i(), r.f());
            let gi = GaussSingleIntegration::with_tolerance(&mut f, a, b, order, tol);
            g.b(gi.is_done());
            g.counts_tf(&f);
            if gi.is_done() {
                g.f(gi.value()?);
            }
        }
        "gauss_single_access" | "gauss_single_dump" => {
            let variant = r.i();
            let mut f = r.tf();
            let (a, b, order, tol) = (r.f(), r.f(), r.i(), r.f());
            let gi = match variant {
                0 => GaussSingleIntegration::new(),
                1 => GaussSingleIntegration::with_order(&mut f, a, b, order),
                _ => GaussSingleIntegration::with_tolerance(&mut f, a, b, order, tol),
            };
            if name == "gauss_single_access" {
                g.f(gi.value()?);
            } else {
                g.dump(&gi.dump());
            }
        }
        // ---- math_GaussMultipleIntegration ----
        "gauss_multiple" | "gauss_multiple_access" | "gauss_multiple_dump" => {
            let mut f = r.mv();
            let (lower, upper, order) = (r.v(), r.v(), r.iv());
            let gm = GaussMultipleIntegration::new(&mut f, &lower, &upper, &order);
            match name {
                "gauss_multiple" => {
                    g.b(gm.is_done());
                    g.counts_mv(&f);
                    if gm.is_done() {
                        g.f(gm.value()?);
                    }
                }
                "gauss_multiple_access" => g.f(gm.value()?),
                _ => g.dump(&gm.dump()),
            }
        }
        // ---- math_GaussSetIntegration ----
        "gauss_set" | "gauss_set_access" | "gauss_set_dump" => {
            let mut f = r.fs();
            let (lower, upper, order) = (r.v(), r.v(), r.iv());
            let gs = GaussSetIntegration::new(&mut f, &lower, &upper, &order)?;
            match name {
                "gauss_set" => {
                    g.b(gs.is_done());
                    g.counts_fs(&f);
                    if gs.is_done() {
                        g.v(gs.value()?);
                    }
                }
                "gauss_set_access" => g.v(gs.value()?),
                _ => g.dump(&gs.dump()),
            }
        }
        // ---- math_KronrodSingleIntegration ----
        "kronrod" => {
            let mut f = r.tf();
            let (a, b, n) = (r.f(), r.f(), r.i());
            let k = KronrodSingleIntegration::with_points(&mut f, a, b, n);
            out_kronrod(&k, &f, g)?;
        }
        "kronrod_tol" => {
            let mut f = r.tf();
            let (a, b, n, tol, maxit) = (r.f(), r.f(), r.i(), r.f(), r.i());
            let k = KronrodSingleIntegration::with_tolerance(&mut f, a, b, n, tol, maxit);
            out_kronrod(&k, &f, g)?;
        }
        "kronrod_perform" => {
            let mut f = r.tf();
            let (v1, a, b, n, tol, maxit) = (r.i(), r.f(), r.f(), r.i(), r.f(), r.i());
            let (v2, a2, b2, n2, tol2, maxit2) = (r.i(), r.f(), r.f(), r.i(), r.f(), r.i());
            let mut k = KronrodSingleIntegration::new();
            if v1 == 0 {
                k.perform(&mut f, a, b, n);
            } else {
                k.perform_with_tolerance(&mut f, a, b, n, tol, maxit);
            }
            if v2 == 0 {
                k.perform(&mut f, a2, b2, n2);
            } else if v2 == 1 {
                k.perform_with_tolerance(&mut f, a2, b2, n2, tol2, maxit2);
            }
            out_kronrod(&k, &f, g)?;
        }
        "kronrod_access" => {
            let mut f = r.tf();
            let (variant, a, b, n, tol, maxit, q) = (r.i(), r.f(), r.f(), r.i(), r.f(), r.i(), r.i());
            let k = match variant {
                0 => KronrodSingleIntegration::new(),
                1 => KronrodSingleIntegration::with_points(&mut f, a, b, n),
                _ => KronrodSingleIntegration::with_tolerance(&mut f, a, b, n, tol, maxit),
            };
            match q {
                0 => g.f(k.value()?),
                1 => g.f(k.error_reached()?),
                2 => g.f(k.absolut_error()?),
                3 => g.i(k.order_reached()?),
                4 => g.i(k.nb_iter_reached()?),
                _ => g.b(k.is_done()),
            }
        }
        "kronrod_gk_rule" => {
            let mut f = r.tf();
            let (a, b, gp, gw, kp, kw) = (r.f(), r.f(), r.v(), r.v(), r.v(), r.v());
            let res = KronrodSingleIntegration::gk_rule(&mut f, a, b, &gp, &gw, &kp, &kw);
            g.b(res.is_some());
            g.counts_tf(&f);
            if let Some((v, e)) = res {
                g.f(v);
                g.f(e);
            }
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
fn mathnd_matches_occt() {
    let text = include_str!("mathnd.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in mathnd.txt:\n{line}");
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
