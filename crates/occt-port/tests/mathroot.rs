//! Differential judge for OCCT's header-only `MathRoot` package (OCCT 8.0.1): replays `tests/mathroot.txt`, printed by
//! `oracle/mathroot.cpp` against the real library. See `docs/api-mathroot.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::mathlin::{MathError, Vector};
use occt_port::mathroot::{self as mr, MultipleConfig, MultipleResult};
use occt_port::mathutils::{Config, ScalarFunction, ScalarFunctionWithDerivative, ScalarResult, Status};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("mathroot.txt");
const GOLDEN_LINES: usize = 314;
const MIN_DISTINCT: usize = 10;
const MAX_ULPS: u64 = 8;
const FAMILIES: &[&str] = &[
    "bisection",
    "bisection_newton",
    "brent",
    "effective_x_tolerance",
    "find_all_roots",
    "find_all_roots_nb_samples",
    "find_all_roots_with_derivative",
    "find_all_roots_with_intervals",
    "find_all_roots_with_intervals_range",
    "find_multiple_roots",
    "newton",
    "newton_bounded",
    "refine_bracketed_root",
    "result_types",
    "secant",
    "secant_auto",
    "sort_roots_add_root",
    "trigonometric",
    "trigonometric_cde",
    "trigonometric_linear",
];
/// Families that call `sin`/`cos`/`asin`/`acos`/`atan` (and the libm-based quartic solver): 8 ulps of the largest value on
/// the line. Everything else is compared bit for bit.
const LIBM: &[&str] = &["trigonometric", "trigonometric_cde", "trigonometric_linear"];

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
    fn vec(&mut self) -> Vector {
        let (l, u) = (self.i(), self.i());
        let vals: Vec<f64> = (0..u - l + 1).map(|_| self.f()).collect();
        Vector::from_slice(l, &vals).unwrap()
    }
    fn cfg(&mut self) -> Config {
        Config { x_tolerance: self.f(), f_tolerance: self.f(), max_iterations: self.i(), ..Config::default() }
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
    fn opt(&mut self, v: Option<f64>) {
        self.b(v.is_some());
        if let Some(v) = v {
            self.f(v);
        }
    }
    fn calls(&mut self, t: &Ts) {
        self.i(t.calls.into());
        self.u(t.hash);
    }
    fn scalar(&mut self, r: &ScalarResult, t: &Ts) {
        self.i(r.status as i64);
        self.i(r.nb_iterations as i64);
        self.opt(r.root);
        self.opt(r.value);
        self.opt(r.derivative);
        self.calls(t);
    }
    fn multiple(&mut self, r: &MultipleResult, t: &Ts) {
        self.i(r.status as i64);
        self.i(r.nb_iterations as i64);
        self.b(r.is_all_null);
        self.i(r.nb_roots().into());
        for v in &r.roots {
            self.f(*v);
        }
        self.i(r.values.len() as i64);
        for v in &r.values {
            self.f(*v);
        }
        self.calls(t);
    }
    fn all(&mut self, r: &mr::AllRootsResult, t: &Ts) {
        self.i(r.status as i64);
        self.i(r.nb_roots().into());
        for v in &r.roots {
            self.f(*v);
        }
        self.i(r.root_states.len() as i64);
        for v in &r.root_states {
            self.i((*v).into());
        }
        self.i(r.nb_intervals().into());
        for n in &r.null_intervals {
            self.f(n.a);
            self.f(n.b);
            self.i(n.state.into());
        }
        self.calls(t);
    }
}

// ---- the callback (identical to the oracle's) --------------------------------------------------------------
fn mixh(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x100000001b3)
}
const H0: u64 = 0xcbf29ce484222325;

#[derive(Clone, Copy)]
struct Ts {
    kind: i32,
    p0: f64,
    p1: f64,
    p2: f64,
    p3: f64,
    flo: f64,
    fhi: f64,
    calls: i32,
    hash: u64,
}
impl Ts {
    fn read(rd: &mut Rd) -> Self {
        Self {
            kind: rd.i(),
            p0: rd.f(),
            p1: rd.f(),
            p2: rd.f(),
            p3: rd.f(),
            flo: rd.f(),
            fhi: rd.f(),
            calls: 0,
            hash: H0,
        }
    }
    fn eval(&self, x: f64) -> (f64, f64) {
        let (p0, p1, p2, p3) = (self.p0, self.p1, self.p2, self.p3);
        match self.kind {
            0 => (((p3 * x + p2) * x + p1) * x + p0, (3.0 * p3 * x + 2.0 * p2) * x + p1),
            1 => {
                let v = (x - p1).abs() - p2;
                let y = p0 * (if 0.0 < v { v } else { 0.0 });
                let dy = if v > 0.0 { p0 * (if x > p1 { 1.0 } else { -1.0 }) } else { 0.0 };
                (y, dy)
            }
            2 => {
                let (a, b, c) = (x - p1, x - p2, x - p3);
                (p0 * a * a * b * c, p0 * (2.0 * a * b * c + a * a * c + a * a * b))
            }
            _ => {
                let x2 = x * x;
                (p0 * (((16.0 * x2 - 20.0) * x2 + 5.0) * x), p0 * ((80.0 * x2 - 60.0) * x2 + 5.0))
            }
        }
    }
}
impl ScalarFunction for Ts {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.calls += 1;
        self.hash = mixh(self.hash, x.to_bits());
        if x > self.flo && x < self.fhi {
            return None;
        }
        Some(self.eval(x).0)
    }
}
impl ScalarFunctionWithDerivative for Ts {
    fn values(&mut self, x: f64) -> Option<(f64, f64)> {
        self.calls += 1;
        self.hash = mixh(self.hash, x.to_bits() ^ 0x5bd1e995);
        if x > self.flo && x < self.fhi {
            return None;
        }
        Some(self.eval(x))
    }
}

fn mc(rd: &mut Rd) -> MultipleConfig {
    MultipleConfig {
        nb_samples: rd.i(),
        x_tolerance: rd.f(),
        f_tolerance: rd.f(),
        null_tolerance: rd.f(),
        max_iterations: rd.i(),
        offset: rd.f(),
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
        "brent" | "bisection" | "bisection_newton" => {
            let mut t = Ts::read(&mut rd);
            let (a, b, c) = (rd.f(), rd.f(), rd.cfg());
            rd.finish();
            let r = match family {
                "brent" => mr::brent(&mut t, a, b, &c),
                "bisection" => mr::bisection(&mut t, a, b, &c),
                _ => mr::bisection_newton(&mut t, a, b, &c),
            };
            o.scalar(&r, &t);
        }
        "secant" => {
            let mut t = Ts::read(&mut rd);
            let (x0, x1, c) = (rd.f(), rd.f(), rd.cfg());
            rd.finish();
            let r = mr::secant(&mut t, x0, x1, &c);
            o.scalar(&r, &t);
        }
        "secant_auto" => {
            let mut t = Ts::read(&mut rd);
            let (x0, c) = (rd.f(), rd.cfg());
            rd.finish();
            let r = mr::secant_auto(&mut t, x0, &c);
            o.scalar(&r, &t);
        }
        "newton" => {
            let mut t = Ts::read(&mut rd);
            let (x0, c) = (rd.f(), rd.cfg());
            rd.finish();
            let r = mr::newton(&mut t, x0, &c);
            o.scalar(&r, &t);
        }
        "newton_bounded" => {
            let mut t = Ts::read(&mut rd);
            let (x0, lo, up, c) = (rd.f(), rd.f(), rd.f(), rd.cfg());
            rd.finish();
            let r = mr::newton_bounded(&mut t, x0, lo, up, &c);
            o.scalar(&r, &t);
        }
        "trigonometric" | "trigonometric_linear" | "trigonometric_cde" => {
            let r = match family {
                "trigonometric" => {
                    let (a, b, c, d, e, inf, sup, eps) =
                        (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
                    mr::trigonometric(a, b, c, d, e, inf, sup, eps)
                }
                "trigonometric_linear" => {
                    let (d, e, inf, sup) = (rd.f(), rd.f(), rd.f(), rd.f());
                    mr::trigonometric_linear(d, e, inf, sup)
                }
                _ => {
                    let (c, d, e, inf, sup) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
                    mr::trigonometric_cde(c, d, e, inf, sup)
                }
            };
            rd.finish();
            o.i(r.status as i64);
            o.i(r.nb_roots.into());
            o.b(r.infinite_roots);
            for v in r.roots {
                o.f(v);
            }
        }
        "find_all_roots" | "find_all_roots_with_derivative" => {
            let mut t = Ts::read(&mut rd);
            let (a, b, c) = (rd.f(), rd.f(), mc(&mut rd));
            rd.finish();
            let r = if family == "find_all_roots" {
                mr::find_all_roots(&mut t, a, b, &c)?
            } else {
                mr::find_all_roots_with_derivative(&mut t, a, b, &c)?
            };
            o.multiple(&r, &t);
        }
        "find_all_roots_nb_samples" => {
            let mut t = Ts::read(&mut rd);
            let (a, b, n) = (rd.f(), rd.f(), rd.i());
            rd.finish();
            let r = mr::find_all_roots_nb_samples(&mut t, a, b, n)?;
            o.multiple(&r, &t);
        }
        "find_multiple_roots" => {
            let mut t = Ts::read(&mut rd);
            let (a, b, ns, ex, ef, off) = (rd.f(), rd.f(), rd.i(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let r = mr::find_multiple_roots(&mut t, a, b, ns, ex, ef, off)?;
            o.multiple(&r, &t);
            let mut t2 = Ts { calls: 0, hash: H0, ..t };
            let r2 = mr::find_multiple_roots(&mut t2, a, b, ns, ex, ef, 0.0)?;
            o.multiple(&r2, &t2);
        }
        "find_all_roots_with_intervals" => {
            let mut t = Ts::read(&mut rd);
            let (s, ex, ef, en) = (rd.vec(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let r = mr::find_all_roots_with_intervals(&mut t, &s, ex, ef, en)?;
            o.all(&r, &t);
        }
        "find_all_roots_with_intervals_range" => {
            let mut t = Ts::read(&mut rd);
            let (a, b, n, ex, ef, en) = (rd.f(), rd.f(), rd.i(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let r = mr::find_all_roots_with_intervals_range(&mut t, a, b, n, ex, ef, en)?;
            o.all(&r, &t);
        }
        "sort_roots_add_root" => {
            let (n, iv, epsx) = (rd.i() as usize, rd.vec(), rd.f());
            rd.finish();
            let input: Vec<f64> = iv.as_slice()[..n].to_vec();
            let mut r = MultipleResult::default();
            for v in &input {
                mr::add_root(&mut r, epsx, *v, *v * 2.0 + 1.0);
            }
            o.i(r.nb_roots().into());
            for (a, b) in r.roots.iter().zip(&r.values) {
                o.f(*a);
                o.f(*b);
            }
            mr::sort_roots(&mut r);
            for (a, b) in r.roots.iter().zip(&r.values) {
                o.f(*a);
                o.f(*b);
            }
        }
        "effective_x_tolerance" => {
            let (lo, up, xt) = (rd.f(), rd.f(), rd.f());
            rd.finish();
            o.f(mr::effective_x_tolerance(lo, up, xt));
        }
        "refine_bracketed_root" => {
            let mut t = Ts::read(&mut rd);
            let (off, x1, y1, x2, y2, tol, epsx) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            let mut r = MultipleResult { nb_iterations: 7, ..MultipleResult::default() };
            let ok = mr::refine_bracketed_root(&mut t, off, x1, y1, x2, y2, tol, epsx, &mut r);
            o.b(ok);
            o.i(r.nb_iterations as i64);
            o.i(r.nb_roots().into());
            for (a, b) in r.roots.iter().zip(&r.values) {
                o.f(*a);
                o.f(*b);
            }
            o.calls(&t);
        }
        "result_types" => {
            let j = rd.i();
            rd.finish();
            let m = MultipleResult::default();
            let c = MultipleConfig::default();
            let n = mr::NullInterval::default();
            let a = mr::AllRootsResult::default();
            let tr = mr::TrigResult::default();
            o.i(m.status as i64);
            o.i(m.nb_iterations as i64);
            o.b(m.is_all_null);
            o.i(m.nb_roots().into());
            o.b(m.is_done());
            o.i(c.nb_samples.into());
            o.f(c.x_tolerance);
            o.f(c.f_tolerance);
            o.f(c.null_tolerance);
            o.i(c.max_iterations.into());
            o.f(c.offset);
            o.f(n.a);
            o.f(n.b);
            o.i(n.state.into());
            o.i(a.status as i64);
            o.i(a.nb_roots().into());
            o.i(a.nb_intervals().into());
            o.b(a.is_done());
            o.i(tr.status as i64);
            o.i(tr.nb_roots.into());
            o.b(tr.infinite_roots);
            o.b(tr.is_done());
            for v in tr.roots {
                o.f(v);
            }
            let st = status(j);
            o.b(MultipleResult { status: st, ..m }.is_done());
            o.b(mr::AllRootsResult { status: st, ..a }.is_done());
            o.b(mr::TrigResult { status: st, ..tr }.is_done());
        }
        _ => panic!("unknown family {family}"),
    }
    Ok(o.0)
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
fn mathroot_matches_occt() {
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
    println!(
        "mathroot: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
