//! Differential judge for OCCT's header-only `MathPoly` package (OCCT 8.0.1): replays `tests/mathpoly.txt`, printed by
//! `oracle/mathpoly.cpp` against the real library. See `docs/api-mathpoly.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::mathpoly::{self as mp, Complex};
use occt_port::mathutils::PolyResult;
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("mathpoly.txt");
const GOLDEN_LINES: usize = 288;
const MIN_DISTINCT: usize = 16;
const MAX_ULPS: u64 = 8;
const FAMILIES: &[&str] = &[
    "cubic",
    "cubic_edge",
    "deflate_complex",
    "deflate_real",
    "eval_polynomial_with_derivatives",
    "laguerre",
    "laguerre_iteration",
    "laguerre_n",
    "linear",
    "octic",
    "quadratic",
    "quadratic_edge",
    "quartic",
    "quartic_biquadratic",
    "quartic_edge",
    "quintic",
    "refine_real_root",
    "sextic",
];
/// Compared bit for bit (+ - * / sqrt only; `Complex` reproduces `__divdc3`, glibc `csqrt` and `hypot`). The others
/// call `acos`/`cos`/`cbrt` (`cubic`, `quartic`, and `quintic` when its leading coefficient is zero): 8 ulps.
const EXACT: &[&str] = &[
    "laguerre",
    "laguerre_n",
    "laguerre_iteration",
    "octic",
    "sextic",
    "linear",
    "quadratic",
    "quadratic_edge",
    "eval_polynomial_with_derivatives",
    "deflate_real",
    "deflate_complex",
    "refine_real_root",
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
    fn vec(&mut self) -> Vec<f64> {
        let (l, u) = (self.i(), self.i());
        assert_eq!(l, 0);
        (0..=u).map(|_| self.f()).collect()
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
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn c(&mut self, z: Complex) {
        self.f(z.re);
        self.f(z.im);
    }
    fn poly(&mut self, r: &PolyResult) {
        self.i(r.status as i64);
        self.i(r.nb_roots as i64);
        for v in r.roots {
            self.f(v);
        }
    }
    fn general(&mut self, r: &mp::GeneralPolyResult) {
        self.i(r.status as i64);
        self.i(r.nb_roots as i64);
        self.i(r.nb_complex_roots as i64);
        for v in r.roots {
            self.f(v);
        }
        for z in r.complex_roots {
            self.c(z);
        }
    }
}

fn run(family: &str, words: &[&str]) -> Vec<Tok> {
    let mut rd = Rd::new(words);
    let mut o = Out::default();
    match family {
        "linear" => {
            let (a, b) = (rd.f(), rd.f());
            rd.finish();
            o.poly(&mp::linear(a, b));
        }
        "quadratic" | "quadratic_edge" => {
            let (a, b, c) = (rd.f(), rd.f(), rd.f());
            rd.finish();
            o.poly(&mp::quadratic(a, b, c));
        }
        "cubic" | "cubic_edge" => {
            let (a, b, c, d) = (rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            o.poly(&mp::cubic(a, b, c, d));
        }
        "quartic" | "quartic_biquadratic" | "quartic_edge" => {
            let (a, b, c, d, e) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            o.poly(&mp::quartic(a, b, c, d, e));
        }
        "quintic" => {
            let (a, b, c, d, e, f) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            o.poly(&mp::quintic(a, b, c, d, e, f));
        }
        "sextic" => {
            let (a, b, c, d, e, f, g) = (rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f(), rd.f());
            rd.finish();
            o.poly(&mp::sextic(a, b, c, d, e, f, g));
        }
        "octic" => {
            let p = rd.vec();
            rd.finish();
            let mut c = [0.0; 9];
            c.copy_from_slice(&p);
            o.general(&mp::octic(&c));
        }
        "laguerre" => {
            let (p, tol, deg) = (rd.vec(), rd.f(), rd.i());
            rd.finish();
            o.general(&mp::laguerre(&p, deg, tol));
            o.general(&mp::laguerre(&p, deg, 1e-12));
        }
        "laguerre_n" => {
            let (p, tol) = (rd.vec(), rd.f());
            rd.finish();
            o.general(&mp::laguerre_n(&p, tol));
        }
        "eval_polynomial_with_derivatives" => {
            let (p, re, im) = (rd.vec(), rd.f(), rd.f());
            rd.finish();
            let (v, d, d2) = mp::evaluate_polynomial_with_derivatives(&p, p.len() as i32 - 1, Complex::new(re, im));
            o.c(v);
            o.c(d);
            o.c(d2);
        }
        "laguerre_iteration" => {
            let (p, re, im, tol, mi) = (rd.vec(), rd.f(), rd.f(), rd.f(), rd.i());
            rd.finish();
            o.c(mp::laguerre_iteration(&p, p.len() as i32 - 1, Complex::new(re, im), tol, mi));
        }
        "deflate_real" => {
            let (mut p, root) = (rd.vec(), rd.f());
            rd.finish();
            let mut d = p.len() as i32 - 1;
            mp::deflate_real(&mut p, &mut d, root);
            o.i(d.into());
            for v in &p[..p.len() - 1] {
                o.f(*v);
            }
        }
        "deflate_complex" => {
            let (mut p, re, im) = (rd.vec(), rd.f(), rd.f());
            rd.finish();
            let mut d = p.len() as i32 - 1;
            mp::deflate_complex(&mut p, &mut d, Complex::new(re, im));
            o.i(d.into());
            for v in &p[..p.len() - 2] {
                o.f(*v);
            }
        }
        "refine_real_root" => {
            let (p, guess) = (rd.vec(), rd.f());
            rd.finish();
            o.f(mp::refine_real_root(&p, p.len() as i32 - 1, guess));
        }
        _ => panic!("unknown family {family}"),
    }
    o.0
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

fn compare(t: &mut Tally, family: &str, line: &str, got: &[Tok], want: &[&str]) {
    assert_eq!(got.len(), want.len(), "{family}: output length\n{line}");
    let exact = EXACT.contains(&family)
        || (family == "quintic"
            && f64::from_bits(u64::from_str_radix(line.split_whitespace().nth(1).unwrap(), 16).unwrap()) != 0.0);
    let mut scale = 1.0_f64;
    for (g, w) in got.iter().zip(want) {
        if let Tok::F(_) = g {
            let v = f64::from_bits(u64::from_str_radix(w, 16).unwrap()).abs();
            if v.is_finite() {
                scale = scale.max(v);
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
                if exact {
                    assert_eq!(v.to_bits(), e.to_bits(), "{family} output {k}: {v:e} != {e:e}\n{line}");
                } else {
                    let tol = MAX_ULPS as f64 * f64::EPSILON * scale;
                    assert!(
                        ulps(*v, e) <= MAX_ULPS || (v - e).abs() <= tol,
                        "{family} output {k}: {v:e} != {e:e}\n{line}"
                    );
                }
            }
        }
    }
}

#[test]
fn mathpoly_matches_occt() {
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut tally = Tally::default();
    let mut lines = 0;
    for (n, line) in DATA.lines().enumerate() {
        let (left, right) = line.split_once(" |").unwrap_or_else(|| panic!("line {} malformed", n + 1));
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        let words: Vec<&str> = it.collect();
        assert!(seen.insert(line.to_owned()), "duplicate line {}", n + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let want: Vec<&str> = right.split_whitespace().collect();
        compare(&mut tally, family, line, &run(family, &words), &want);
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
        "mathpoly: {} families, {lines} lines, {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
