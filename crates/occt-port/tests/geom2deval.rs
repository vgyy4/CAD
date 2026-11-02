//! Differential judge for OCCT's package `Geom2dEval` (OCCT 8.0.1): replays `tests/geom2deval.txt`, printed by
//! `oracle/geom2deval.cpp` against the real library. See `docs/api-geom2deval.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::geom2d::{BoundedCurve2d, Curve2d, Geom2dError};
use occt_port::geom2deval::{
    AhtBezierCurve, ArchimedeanSpiralCurve, CircleInvoluteCurve, LogarithmicSpiralCurve, SineWaveCurve, TBezierCurve,
};
use occt_port::gp::{Ax2d, Dir2d, Pnt2d, Trsf2d, Vec2d};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("geom2deval.txt");
const GOLDEN_LINES: usize = 474;
const MIN_DISTINCT: usize = 3;
const TYPES: [&str; 6] = ["archimedean", "involute", "logarithmic", "sine_wave", "aht_bezier", "t_bezier"];
const KINDS: [&str; 4] = ["ctor", "eval", "props", "unsupported"];

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
    fn b(&mut self) -> bool {
        self.i() != 0
    }
    fn f(&mut self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.next(), 16).unwrap())
    }
    fn p2(&mut self) -> Pnt2d {
        Pnt2d::new(self.f(), self.f())
    }
    fn finish(&self) {
        assert_eq!(self.p, self.w.len(), "unread input tokens");
    }
    fn ax(&mut self) -> Ax2d {
        let p = self.p2();
        let (dx, dy) = (self.f(), self.f());
        Ax2d::new(p, Dir2d::new(dx, dy).unwrap())
    }
    fn poles(&mut self) -> Vec<Pnt2d> {
        let n = self.i();
        (0..n).map(|_| self.p2()).collect()
    }
}

/// One of the six curves.
enum Spec {
    Arch(ArchimedeanSpiralCurve),
    Invol(CircleInvoluteCurve),
    Log(LogarithmicSpiralCurve),
    Sine(SineWaveCurve),
    Aht(AhtBezierCurve),
    Tb(TBezierCurve),
}
impl Spec {
    fn curve(&self) -> &dyn Curve2d {
        match self {
            Spec::Arch(c) => c,
            Spec::Invol(c) => c,
            Spec::Log(c) => c,
            Spec::Sine(c) => c,
            Spec::Aht(c) => c,
            Spec::Tb(c) => c,
        }
    }
    fn bounded(&self) -> Option<&dyn BoundedCurve2d> {
        match self {
            Spec::Aht(c) => Some(c),
            Spec::Tb(c) => Some(c),
            _ => None,
        }
    }
    /// The evaluation range of the oracle.
    fn range(&self) -> (f64, f64) {
        match self {
            Spec::Arch(_) => (0.0, 6.0),
            Spec::Invol(_) => (0.0, 5.0),
            Spec::Log(_) => (-2.0, 3.0),
            Spec::Sine(_) => (-3.0, 3.0),
            Spec::Aht(_) => (0.0, 1.0),
            Spec::Tb(c) => (0.0, std::f64::consts::PI / c.alpha()),
        }
    }
}

fn read_spec(rd: &mut Rd, ty: usize) -> Result<Spec, Geom2dError> {
    let t = rd.i();
    assert_eq!(t as usize, ty);
    Ok(match ty {
        0 => {
            let ax = rd.ax();
            let (r0, rate) = (rd.f(), rd.f());
            Spec::Arch(ArchimedeanSpiralCurve::new(ax, r0, rate)?)
        }
        1 => {
            let ax = rd.ax();
            Spec::Invol(CircleInvoluteCurve::new(ax, rd.f())?)
        }
        2 => {
            let ax = rd.ax();
            let (s, e) = (rd.f(), rd.f());
            Spec::Log(LogarithmicSpiralCurve::new(ax, s, e)?)
        }
        3 => {
            let ax = rd.ax();
            let (a, o, p) = (rd.f(), rd.f(), rd.f());
            Spec::Sine(SineWaveCurve::new(ax, a, o, p)?)
        }
        4 => {
            let (deg, alpha, beta) = (rd.i(), rd.f(), rd.f());
            let poles = rd.poles();
            if rd.b() {
                let w: Vec<f64> = (0..poles.len()).map(|_| rd.f()).collect();
                Spec::Aht(AhtBezierCurve::with_weights(&poles, &w, deg, alpha, beta)?)
            } else {
                Spec::Aht(AhtBezierCurve::new(&poles, deg, alpha, beta)?)
            }
        }
        _ => {
            let alpha = rd.f();
            let poles = rd.poles();
            if rd.b() {
                let w: Vec<f64> = (0..poles.len()).map(|_| rd.f()).collect();
                Spec::Tb(TBezierCurve::with_weights(&poles, &w, alpha)?)
            } else {
                Spec::Tb(TBezierCurve::new(&poles, alpha)?)
            }
        }
    })
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
    fn b(&mut self, v: bool) {
        self.i(v.into());
    }
    fn p2(&mut self, p: &Pnt2d) {
        self.f(p.x());
        self.f(p.y());
    }
    fn v2(&mut self, v: &Vec2d) {
        self.f(v.x());
        self.f(v.y());
    }
}

fn info(o: &mut Out, s: &Spec) -> Result<(), Geom2dError> {
    match s {
        Spec::Aht(c) => {
            o.i(c.nb_poles().into());
            o.i(c.alg_degree().into());
            o.f(c.alpha());
            o.f(c.beta());
            o.b(c.is_rational());
        }
        Spec::Tb(c) => {
            o.i(c.nb_poles().into());
            o.i(c.order().into());
            o.f(c.alpha());
            o.b(c.is_rational());
        }
        _ => return Err(Geom2dError::Domain),
    }
    Ok(())
}

fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, Geom2dError> {
    let ty = TYPES.iter().position(|t| family.starts_with(t)).unwrap();
    let kind = &family[TYPES[ty].len() + 1..];
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let mut o = Out::default();
    let s = read_spec(rd, ty)?;
    let c = s.curve();
    let (u0, u1) = s.range();
    match kind {
        "ctor" => {
            rd.finish();
            o.f(c.first_parameter());
            o.f(c.last_parameter());
            match &s {
                Spec::Aht(_) | Spec::Tb(_) => info(&mut o, &s)?,
                _ => o.p2(&c.eval_d0(u0 + 0.5)?),
            }
        }
        "eval" => {
            let (op, n, sel) = (rd.i(), rd.i(), rd.i());
            rd.finish();
            let u = match sel {
                0 => u0,
                1 => u1,
                2 => u0 + (u1 - u0) * 0.3125,
                3 => u0 + (u1 - u0) * 0.75,
                _ => u0 + (u1 - u0) * 0.5625,
            };
            o.f(u);
            match op {
                0 | 5 => o.p2(&c.eval_d0(u)?),
                1 | 6 => {
                    let r = c.eval_d1(u)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                }
                2 | 7 => {
                    let r = c.eval_d2(u)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                    o.v2(&r.d2);
                }
                3 | 8 => {
                    let r = c.eval_d3(u)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                    o.v2(&r.d2);
                    o.v2(&r.d3);
                }
                _ => o.v2(&c.eval_dn(u, n)?),
            }
        }
        "props" => {
            let (op, n) = (rd.i(), rd.i());
            rd.finish();
            match op {
                0 => {
                    o.f(c.first_parameter());
                    o.f(c.last_parameter());
                }
                1 => {
                    o.b(c.is_closed());
                    o.b(c.is_periodic());
                }
                2 => o.i(c.continuity() as i64),
                3 => o.b(c.is_cn(n)?),
                4 => {
                    let cp = c.copy_curve();
                    o.i(0);
                    o.i(1);
                    o.p2(&cp.eval_d0(u0 + (u1 - u0) * 0.4375)?);
                }
                5 => {
                    let b = s.bounded().ok_or(Geom2dError::Domain)?;
                    o.p2(&b.start_point());
                    o.p2(&b.end_point());
                }
                6 => info(&mut o, &s)?,
                7 => {
                    let poles = match &s {
                        Spec::Aht(c) => c.poles(),
                        Spec::Tb(c) => c.poles(),
                        _ => return Err(Geom2dError::Domain),
                    };
                    for p in poles {
                        o.p2(p);
                    }
                }
                8 => {
                    let (rational, w) = match &s {
                        Spec::Aht(c) => (c.is_rational(), c.weights()),
                        Spec::Tb(c) => (c.is_rational(), c.weights()),
                        _ => return Err(Geom2dError::Domain),
                    };
                    if !rational {
                        return Err(Geom2dError::Domain);
                    }
                    for v in w {
                        o.f(*v);
                    }
                }
                _ => o.p2(&c.eval_d0(u0)?),
            }
        }
        "unsupported" => {
            let j = rd.i();
            rd.finish();
            let mut cp = c.copy_curve();
            match j {
                0 => cp.try_reverse()?,
                1 => o.f(c.try_reversed_parameter(0.5)?),
                _ => {
                    let mut t = Trsf2d::new();
                    t.set_translation(&Vec2d::new(1.0, 2.0));
                    cp.try_transform(&t)?;
                }
            }
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

fn compare(t: &mut Tally, family: &str, line: &str, got: &Result<Vec<Tok>, Geom2dError>, want: &[&str]) {
    if want == ["throws"] {
        assert!(got.is_err(), "{family}: C++ throws but Rust returned Ok\n{line}");
        return;
    }
    let got = match got {
        Ok(g) => g,
        Err(e) => panic!("{family}: Rust returned Err({e:?}) but C++ did not throw\n{line}"),
    };
    assert_eq!(got.len(), want.len(), "{family}: output length\n{line}");
    let scale = got
        .iter()
        .zip(want)
        .filter_map(|(g, w)| match g {
            Tok::F(_) => Some(f64::from_bits(u64::from_str_radix(w, 16).unwrap()).abs()),
            Tok::I(_) => None,
        })
        .filter(|v| v.is_finite())
        .fold(0.0, f64::max);
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
                // sin, cos, exp, sinh and cosh: 8 ulps of the largest value on the line
                let ulp = scale.max(f64::MIN_POSITIVE) * f64::EPSILON;
                assert!(
                    v.to_bits() == e.to_bits() || (v - e).abs() <= 8.0 * ulp,
                    "{family} output {k}: {v:e} != {e:e} (scale {scale:e})\n{line}"
                );
            }
        }
    }
}

#[test]
fn geom2deval_matches_occt() {
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
    let mut want_fams: Vec<String> = TYPES.iter().flat_map(|t| KINDS.iter().map(move |k| format!("{t}_{k}"))).collect();
    want_fams.sort_unstable();
    assert_eq!(counts.keys().cloned().collect::<Vec<_>>(), want_fams);
    for name in &want_fams {
        assert!(distinct[name].len() >= MIN_DISTINCT, "{name} needs {MIN_DISTINCT} distinct inputs");
    }
    assert!(throws >= 1, "the oracle must exercise `throws`");
    println!(
        "geom2deval: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
