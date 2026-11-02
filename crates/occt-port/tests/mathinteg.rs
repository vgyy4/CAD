//! Oracle replay for all public integration entry points in `cpp/MathInteg/`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::mathinteg::{
    self, DoubleExpConfig, Function, IntegConfig, IntegResult, KronrodConfig, MultipleFunction, SetFunction,
};
use occt_port::mathlin::Vector;
use std::collections::{BTreeMap, BTreeSet, HashSet};

const MAX_ULPS: f64 = 8.0;
const MIN_DISTINCT: usize = 16;
const GOLDEN_LINES: usize = 304;
// Families whose callback arguments come out of sinh/cosh/tanh/exp: glibc and the libm crate differ by an ulp there,
// so the argument-trace hash (last token) cannot be compared; call counts and values still are.
const HASH_RELAXED: &[&str] =
    &["tanh_sinh", "exp_sinh", "sinh_sinh", "double_exponential", "tanh_sinh_singular", "tanh_sinh_with_singularity"];
const EXACT: &[&str] = &[
    "gauss",
    "gauss_composite",
    "gauss_multiple",
    "gauss_multiple_config",
    "gauss_multiple_uniform",
    "gauss_set",
    "gauss_set_vectors",
];
const FAMILIES: &[&str] = &[
    "double_exponential",
    "exp_sinh",
    "gauss",
    "gauss_adaptive",
    "gauss_composite",
    "gauss_multiple",
    "gauss_multiple_config",
    "gauss_multiple_uniform",
    "gauss_set",
    "gauss_set_vectors",
    "kronrod",
    "kronrod_auto",
    "kronrod_infinite",
    "kronrod_rule",
    "kronrod_semi_infinite",
    "sinh_sinh",
    "tanh_sinh",
    "tanh_sinh_singular",
    "tanh_sinh_with_singularity",
];
const SCHEMA: &[&str] = &["i", "i", "i", "f", "f", "i", "f", "i", "f", "i"];

#[derive(Clone, Debug)]
enum Tok {
    F(f64),
    I(i128),
    B(bool),
}
#[derive(Default)]
struct Got(Vec<Tok>);
impl Got {
    fn i(&mut self, x: impl Into<i128>) {
        self.0.push(Tok::I(x.into()));
    }
    fn b(&mut self, x: bool) {
        self.0.push(Tok::B(x));
    }
    fn f(&mut self, x: f64) {
        self.0.push(Tok::F(x));
    }
}
struct Rd<'a> {
    w: &'a [&'a str],
    n: usize,
    schema: Vec<&'static str>,
}
impl<'a> Rd<'a> {
    fn new(w: &'a [&'a str]) -> Self {
        Self { w, n: 0, schema: Vec::new() }
    }
    fn word(&mut self, kind: &'static str) -> &'a str {
        self.schema.push(kind);
        let v = *self.w.get(self.n).expect("missing input");
        self.n += 1;
        v
    }
    fn i(&mut self) -> i32 {
        self.word("i").parse().expect("integer input")
    }
    fn f(&mut self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.word("f"), 16).expect("f64 input"))
    }
    fn finish(&self) {
        assert_eq!(self.n, self.w.len());
        assert_eq!(self.schema, SCHEMA);
    }
}
struct Trace {
    calls: u64,
    hash: u64,
}
impl Trace {
    fn new() -> Self {
        Self { calls: 0, hash: 0xcbf29ce484222325 }
    }
    fn add(&mut self, x: f64) {
        self.calls += 1;
        self.mix(x);
    }
    fn mix(&mut self, x: f64) {
        self.hash ^= x.to_bits();
        self.hash = self.hash.wrapping_mul(0x100000001b3);
    }
}
fn ipow(x: f64, n: i32) -> f64 {
    let mut y = 1.0;
    for _ in 0..n {
        y *= x;
    }
    y
}
fn eval(kind: i32, param: i32, x: f64) -> f64 {
    match kind {
        0 => ipow(x, param),
        1 => libm::exp(-x * x),
        2 => libm::sin(x),
        3 => (x - 0.125).abs(),
        _ => x * x + 1.0,
    }
}
struct Scalar {
    kind: i32,
    param: i32,
    trace: Trace,
}
impl Function for Scalar {
    fn value(&mut self, x: f64) -> Option<f64> {
        self.trace.add(x);
        if self.kind == 4 && x > -0.125 && x < 0.125 { None } else { Some(eval(self.kind, self.param, x)) }
    }
}
struct Multi {
    dim: usize,
    abs: bool,
    trace: Trace,
}
impl MultipleFunction for Multi {
    fn value(&mut self, x: &[f64]) -> Option<f64> {
        let mut s = 0.0;
        self.trace.calls += 1;
        for v in x.iter().take(self.dim) {
            self.trace.mix(*v);
            s += if self.abs { v.abs() } else { *v * *v };
        }
        Some(s)
    }
}
struct SetFn {
    n: usize,
    trace: Trace,
}
impl SetFunction for SetFn {
    fn nb_equations(&self) -> usize {
        self.n
    }
    fn value(&mut self, x: &[f64]) -> Option<Vec<f64>> {
        self.trace.add(*x.first()?);
        Some((1..=self.n).map(|j| ipow(*x.first().unwrap_or(&0.0), j as i32)).collect())
    }
}
fn expected_scalar(r: &IntegResult, g: &mut Got, trace: &Trace) {
    g.i(r.status as i32);
    g.b(r.value.is_some());
    if let Some(v) = r.value {
        g.f(v)
    }
    g.b(r.absolute_error.is_some());
    if let Some(v) = r.absolute_error {
        g.f(v)
    }
    g.b(r.relative_error.is_some());
    if let Some(v) = r.relative_error {
        g.f(v)
    }
    g.i(r.nb_points as i128);
    g.i(r.nb_iterations as i128);
    g.i(trace.calls as i128);
    g.i(trace.hash as i128);
}
fn vector(lower: i32, n: usize, v: f64) -> Vector {
    Vector::filled(lower, lower + n as i32 - 1, v).expect("valid test vector")
}
fn run(family: &str, words: &[&str]) -> Got {
    let mut rd = Rd::new(words);
    let kind = rd.i();
    let param = rd.i();
    let dim = rd.i() as usize;
    let a = rd.f();
    let b = rd.f();
    let order = rd.i();
    let tol = rd.f();
    let it = rd.i();
    let sing = rd.f();
    let extra = rd.i();
    rd.finish();
    let mut cb = Scalar { kind, param, trace: Trace::new() };
    let initial_order = order.max(1);
    let max_order = initial_order.saturating_mul(2).min(61).max(initial_order);
    let kc = KronrodConfig {
        nb_gauss_points: order.clamp(1, 15),
        initial_order,
        max_order,
        max_iterations: it,
        tolerance: tol,
        adaptive: extra == 0,
    };
    let ic = IntegConfig { initial_order, max_order, max_iterations: it, tolerance: tol };
    let dc =
        DoubleExpConfig { tolerance: tol, max_iterations: it, nb_levels: 1 + dim as i32, ..DoubleExpConfig::default() };
    let mut got = Got::default();
    if family == "gauss_multiple" || family == "gauss_multiple_uniform" || family == "gauss_multiple_config" {
        let ord = vec![order; dim];
        let lo = vector(1, dim, a);
        let hi = vector(1, dim, b);
        let mut cb = Multi { dim, abs: false, trace: Trace::new() };
        let r = if family == "gauss_multiple_config" {
            let mc = mathinteg::MultipleConfig { max_order: extra };
            mathinteg::gauss_multiple_with_config(&mut cb, &lo, &hi, &ord, &mc)
        } else if family == "gauss_multiple" {
            mathinteg::gauss_multiple(&mut cb, &lo, &hi, &ord)
        } else {
            mathinteg::gauss_multiple_uniform(&mut cb, &lo, &hi, order)
        };
        expected_scalar(&r, &mut got, &cb.trace);
    } else if family == "gauss_set" || family == "gauss_set_vectors" {
        let mut cb = SetFn { n: dim, trace: Trace::new() };
        let r = if family == "gauss_set" {
            mathinteg::gauss_set(&mut cb, a, b, order)
        } else {
            let lo = vector(0, 1, a);
            let hi = vector(0, 1, b);
            mathinteg::gauss_set_vectors(&mut cb, &lo, &hi, order)
        };
        got.i(r.status as i32);
        got.b(r.values.is_some());
        if let Some(v) = r.values {
            for x in v.as_slice() {
                got.f(*x)
            }
        }
        got.i(r.nb_equations as i128);
        got.i(cb.trace.calls as i128);
        got.i(cb.trace.hash as i128);
    } else {
        let r = match family {
            "gauss" => mathinteg::gauss(&mut cb, a, b, order),
            "gauss_adaptive" => mathinteg::gauss_adaptive(&mut cb, a, b, &ic),
            "gauss_composite" => mathinteg::gauss_composite(&mut cb, a, b, extra, order),
            "kronrod_rule" => mathinteg::kronrod_rule(&mut cb, a, b, order.max(1)),
            "kronrod" => mathinteg::kronrod(&mut cb, a, b, &kc),
            "kronrod_auto" => mathinteg::kronrod_auto(&mut cb, a, b, tol, extra),
            "kronrod_semi_infinite" => mathinteg::kronrod_semi_infinite(&mut cb, a.max(0.0), &kc),
            "kronrod_infinite" => mathinteg::kronrod_infinite(&mut cb, &kc),
            "tanh_sinh" => mathinteg::tanh_sinh(&mut cb, a, b, &dc),
            "exp_sinh" => mathinteg::exp_sinh(&mut cb, a.max(0.0), &dc),
            "sinh_sinh" => mathinteg::sinh_sinh(&mut cb, &dc),
            "double_exponential" => mathinteg::double_exponential(&mut cb, a, b, &dc),
            "tanh_sinh_singular" => mathinteg::tanh_sinh_singular(&mut cb, a, b, tol),
            "tanh_sinh_with_singularity" => mathinteg::tanh_sinh_with_singularity(&mut cb, a, b, sing, &dc),
            _ => panic!("unknown family {family}"),
        };
        expected_scalar(&r, &mut got, &cb.trace);
    }
    got
}
fn ulp_distance(a: f64, b: f64) -> u64 {
    fn ord(x: f64) -> i64 {
        let v = x.to_bits() as i64;
        if v < 0 { i64::MIN - v } else { v }
    }
    ord(a).abs_diff(ord(b))
}
fn compare(family: &str, got: &[Tok], want: &[&str]) {
    let mut max = 1.0_f64;
    for (i, t) in got.iter().enumerate() {
        if matches!(t, Tok::F(_))
            && let Ok(v) = u64::from_str_radix(want[i], 16)
        {
            max = max.max(f64::from_bits(v).abs());
        }
    }
    let exact = EXACT.contains(&family);
    let relax_hash = HASH_RELAXED.contains(&family);
    let eps = if max.is_infinite() { 0.0 } else { MAX_ULPS * f64::EPSILON * max };
    assert_eq!(got.len(), want.len(), "{family}: output length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        if relax_hash && i + 1 == got.len() {
            continue;
        }
        match g {
            Tok::I(v) => assert_eq!(*v, w.parse::<i128>().expect("expected integer"), "{family} output {i}"),
            Tok::B(v) => assert_eq!(*v, *w == "1", "{family} output {i}"),
            Tok::F(v) => {
                let e = f64::from_bits(u64::from_str_radix(w, 16).expect("expected float bits"));
                if exact {
                    assert_eq!(v.to_bits(), e.to_bits(), "{family} output {i}")
                } else {
                    assert!(ulp_distance(*v, e) <= 8 || (v - e).abs() <= eps, "{family} output {i}: {v:?} != {e:?}");
                }
            }
        }
    }
}

#[test]
fn mathinteg_matches_occt() {
    let mut lines = 0;
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    for (line_no, line) in include_str!("mathinteg.txt").lines().enumerate() {
        let (left, right) = line.split_once(" | ").unwrap_or_else(|| panic!("line {} malformed", line_no + 1));
        let mut it = left.split_whitespace();
        let family = it.next().expect("family");
        let words: Vec<_> = it.collect();
        let key = line.to_owned();
        assert!(seen.insert(key), "duplicate line {}", line_no + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let got = run(family, &words);
        let want: Vec<_> = right.split_whitespace().collect();
        compare(family, &got.0, &want);
        lines += 1;
    }
    assert_eq!(lines, GOLDEN_LINES);
    let got_families: Vec<_> = counts.keys().map(String::as_str).collect();
    let mut want = FAMILIES.to_vec();
    want.sort_unstable();
    assert_eq!(got_families, want);
    for name in FAMILIES {
        assert!(distinct.get(*name).map_or(0, BTreeSet::len) >= MIN_DISTINCT, "{name} needs 16 distinct inputs");
        assert_eq!(counts.get(*name), Some(&16), "{name} line count");
    }
}
