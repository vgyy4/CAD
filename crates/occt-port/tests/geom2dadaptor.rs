//! Differential judge for OCCT's package `Geom2dAdaptor` (OCCT 8.0.1): replays `tests/geom2dadaptor.txt`, printed by
//! `oracle/geom2dadaptor.cpp` against the real library. See `docs/api-geom2dadaptor.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::adaptor2d::{AdaptorError, Curve2dAdaptor, GeomAbsShape};
use occt_port::geom2d::{
    BSplineCurve, BezierCurve, Circle, Curve2d, Ellipse, Hyperbola, Line, OffsetCurve, Parabola, TrimmedCurve,
};
use occt_port::geom2dadaptor::{Curve, make_curve};
use occt_port::gp::{Ax2d, Ax22d, Circ2d, Dir2d, Elips2d, Hypr2d, Lin2d, Parab2d, Pnt2d, Vec2d};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

const DATA: &str = include_str!("geom2dadaptor.txt");
const GOLDEN_LINES: usize = 912;
const MIN_DISTINCT: usize = 9;
const FAMILIES: &[&str] = &[
    "closed",
    "conic",
    "continuity",
    "eval",
    "eval_sweep",
    "info",
    "intervals",
    "load",
    "make_curve",
    "nb_intervals",
    "nb_samples",
    "reset",
    "resolution",
    "trim_copy",
];
/// Families whose values come through `sin`, `cos`, `cosh`, `asin`...: 8 ulps of the largest value of the line.
const ULP_FAMILIES: &[&str] = &["eval", "eval_sweep", "resolution", "trim_copy", "make_curve"];
const SHAPES: [GeomAbsShape; 7] = [
    GeomAbsShape::C0,
    GeomAbsShape::G1,
    GeomAbsShape::C1,
    GeomAbsShape::G2,
    GeomAbsShape::C2,
    GeomAbsShape::C3,
    GeomAbsShape::Cn,
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
type Crv = Option<Arc<dyn Curve2d>>;
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
    /// A basis curve (kinds 0..6); returns it with its kind.
    fn base(&mut self) -> Result<(Arc<dyn Curve2d>, i32), AdaptorError> {
        let kind = self.i();
        let c: Arc<dyn Curve2d> = match kind {
            0..=4 => {
                let p = self.p2();
                let d = Dir2d::new(self.f(), self.f()).map_err(|_| AdaptorError::Construction)?;
                if kind == 0 {
                    Arc::new(Line::new(p, d))
                } else {
                    let ax = Ax2d::new(p, d);
                    let sense = self.b();
                    match kind {
                        1 => Arc::new(Circle::new(ax, self.f(), sense)?),
                        2 => {
                            let (a, b) = (self.f(), self.f());
                            Arc::new(Ellipse::new(ax, a, b, sense)?)
                        }
                        3 => {
                            let (a, b) = (self.f(), self.f());
                            Arc::new(Hyperbola::new(ax, a, b, sense)?)
                        }
                        _ => Arc::new(Parabola::new(ax, self.f(), sense)?),
                    }
                }
            }
            5 => {
                let n = self.i();
                let poles: Vec<Pnt2d> = (0..n).map(|_| self.p2()).collect();
                if self.b() {
                    let w: Vec<f64> = (0..n).map(|_| self.f()).collect();
                    Arc::new(BezierCurve::with_weights(&poles, &w)?)
                } else {
                    Arc::new(BezierCurve::new(&poles)?)
                }
            }
            _ => {
                let (degree, periodic) = (self.i(), self.b());
                let k = self.i();
                let knots: Vec<f64> = (0..k).map(|_| self.f()).collect();
                let k2 = self.i();
                let mults: Vec<i32> = (0..k2).map(|_| self.i()).collect();
                let np = self.i();
                let poles: Vec<Pnt2d> = (0..np).map(|_| self.p2()).collect();
                if self.b() {
                    let w: Vec<f64> = (0..np).map(|_| self.f()).collect();
                    Arc::new(BSplineCurve::with_weights(&poles, &w, &knots, &mults, degree, periodic)?)
                } else {
                    Arc::new(BSplineCurve::new(&poles, &knots, &mults, degree, periodic)?)
                }
            }
        };
        Ok((c, kind))
    }
    /// A top-level curve; `Ok(None)` when the oracle's constructors refused it (kind -2) and the case throws.
    fn curve(&mut self) -> Result<Crv, AdaptorError> {
        let kind = self.i();
        match kind {
            -2 => Ok(None),
            7 => {
                let off = self.f();
                let (b, _) = self.base()?;
                Ok(Some(Arc::new(OffsetCurve::new(b, off, false)?)))
            }
            8 => {
                let (u1, u2) = (self.f(), self.f());
                let (b, _) = self.base()?;
                Ok(Some(Arc::new(TrimmedCurve::new(b, u1, u2, true, true)?)))
            }
            _ => {
                self.p -= 1;
                Ok(Some(self.base()?.0))
            }
        }
    }
}

fn kind_of(c: Option<&Arc<dyn Curve2d>>) -> i64 {
    let Some(c) = c else {
        return -1;
    };
    let a = c.as_any();
    if a.is::<Line>() {
        0
    } else if a.is::<Circle>() {
        1
    } else if a.is::<Ellipse>() {
        2
    } else if a.is::<Hyperbola>() {
        3
    } else if a.is::<Parabola>() {
        4
    } else if a.is::<BezierCurve>() {
        5
    } else if a.is::<BSplineCurve>() {
        6
    } else if a.is::<OffsetCurve>() {
        7
    } else if a.is::<TrimmedCurve>() {
        8
    } else {
        9
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
    fn d2(&mut self, d: &Dir2d) {
        self.f(d.x());
        self.f(d.y());
    }
    fn a22(&mut self, a: &Ax22d) {
        self.p2(a.location());
        self.d2(a.x_direction());
        self.d2(a.y_direction());
    }
    fn lin(&mut self, l: &Lin2d) {
        self.p2(l.location());
        self.d2(l.direction());
    }
    fn circ(&mut self, c: &Circ2d) {
        self.a22(c.axis());
        self.f(c.radius());
    }
    fn elips(&mut self, e: &Elips2d) {
        self.a22(e.axis());
        self.f(e.major_radius());
        self.f(e.minor_radius());
    }
    fn hypr(&mut self, h: &Hypr2d) {
        self.a22(h.axis());
        self.f(h.major_radius());
        self.f(h.minor_radius());
    }
    fn parab(&mut self, p: &Parab2d) {
        self.a22(p.axis());
        self.f(p.focal());
    }
}

fn adaptor(c: &Crv, ranged: bool, u1: f64, u2: f64) -> Result<Curve, AdaptorError> {
    let c = c.clone().ok_or(AdaptorError::NullObject)?;
    if ranged { Curve::with_range(c, u1, u2) } else { Curve::with_curve(c) }
}

fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, AdaptorError> {
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let mut o = Out::default();
    let c = rd.curve()?;
    match family {
        "load" => {
            let (v, u1, u2) = (rd.i(), rd.f(), rd.f());
            rd.finish();
            let cc = c.ok_or(AdaptorError::NullObject)?;
            let mut a = Curve::new();
            match v {
                0 => a = Curve::with_curve(cc)?,
                1 => a = Curve::with_range(cc, u1, u2)?,
                2 => a.load(cc)?,
                3 | 5 => a.load_range(cc, u1, u2)?,
                _ => return Err(AdaptorError::NullObject),
            }
            o.b(a.is_initialized());
            o.i(a.get_type()? as i64);
            o.f(a.first_parameter()?);
            o.f(a.last_parameter()?);
            o.i(kind_of(a.curve()));
        }
        "reset" => {
            rd.finish();
            let mut a = adaptor(&c, false, 0.0, 0.0)?;
            a.reset();
            o.b(a.is_initialized());
            o.i(a.get_type()? as i64);
            o.f(a.first_parameter()?);
            o.f(a.last_parameter()?);
            o.i(kind_of(a.curve()));
        }
        "continuity" => {
            let (ranged, u1, u2) = (rd.b(), rd.f(), rd.f());
            rd.finish();
            o.i(adaptor(&c, ranged, u1, u2)?.continuity()? as i64);
        }
        "nb_intervals" => {
            let (si, ranged, u1, u2) = (rd.i(), rd.b(), rd.f(), rd.f());
            rd.finish();
            o.i(adaptor(&c, ranged, u1, u2)?.nb_intervals(SHAPES[si as usize])?.into());
        }
        "intervals" => {
            let (si, ranged, u1, u2) = (rd.i(), rd.b(), rd.f(), rd.f());
            rd.finish();
            let a = adaptor(&c, ranged, u1, u2)?;
            let n = a.nb_intervals(SHAPES[si as usize])?;
            let mut t = vec![-7.0; (n + 1) as usize];
            a.intervals(&mut t, SHAPES[si as usize])?;
            o.i(n.into());
            for v in t {
                o.f(v);
            }
        }
        "closed" => {
            rd.finish();
            let a = adaptor(&c, false, 0.0, 0.0)?;
            o.b(a.is_closed()?);
            o.b(a.is_periodic()?);
            o.f(a.period()?);
        }
        "nb_samples" => {
            rd.finish();
            o.i(adaptor(&c, false, 0.0, 0.0)?.nb_samples()?.into());
        }
        "resolution" => {
            let r = rd.f();
            rd.finish();
            o.f(adaptor(&c, false, 0.0, 0.0)?.resolution(r)?);
        }
        "info" => {
            let op = rd.i();
            rd.finish();
            let a = adaptor(&c, false, 0.0, 0.0)?;
            match op {
                0 => o.i(a.degree()?.into()),
                1 => o.b(a.is_rational()?),
                2 => o.i(a.nb_poles()?.into()),
                3 => o.i(a.nb_knots()?.into()),
                4 => {
                    let b = a.bezier()?;
                    o.b(b.is_none());
                    if let Some(b) = b {
                        o.i(b.nb_poles().into());
                        o.i(b.degree().into());
                    }
                }
                5 => {
                    let b = a.bspline()?;
                    o.b(b.is_none());
                    if let Some(b) = b {
                        o.i(b.nb_knots().into());
                        o.i(b.degree().into());
                    }
                }
                6 => o.lin(&a.line()?),
                _ => o.circ(&a.circle()?),
            }
        }
        "conic" => {
            let wrong = rd.i();
            rd.finish();
            let kind = kind_of(c.as_ref()) as i32;
            let a = adaptor(&c, false, 0.0, 0.0)?;
            let which = if wrong != 0 { (kind % 4) + 1 } else { kind };
            match which {
                1 => o.circ(&a.circle()?),
                2 => o.elips(&a.ellipse()?),
                3 => o.hypr(&a.hyperbola()?),
                _ => o.parab(&a.parabola()?),
            }
        }
        "eval" => {
            let (op, n, ranged, u1, u2, sel) = (rd.i(), rd.i(), rd.b(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let a = adaptor(&c, ranged, u1, u2)?;
            let (f, l) = (a.first_parameter()?, a.last_parameter()?);
            let fr = !(f.abs() > 1e50 || l.abs() > 1e50);
            let ua = match sel {
                0 => f,
                1 => l,
                2 => {
                    if fr {
                        f + (l - f) * 0.4375
                    } else {
                        0.4375
                    }
                }
                3 => {
                    if fr {
                        f + (l - f) * 0.8125
                    } else {
                        -0.8125
                    }
                }
                _ => {
                    if fr {
                        f + (l - f) * 0.0625
                    } else {
                        1.0625
                    }
                }
            };
            o.f(ua);
            match op {
                0 => o.p2(&a.value(ua)?),
                1 => o.p2(&a.d0(ua)?),
                2 | 11 => {
                    let (p, v1) = a.d1(ua)?;
                    o.p2(&p);
                    o.v2(&v1);
                }
                3 => {
                    let (p, v1, v2) = a.d2(ua)?;
                    o.p2(&p);
                    o.v2(&v1);
                    o.v2(&v2);
                }
                4 => {
                    let (p, v1, v2, v3) = a.d3(ua)?;
                    o.p2(&p);
                    o.v2(&v1);
                    o.v2(&v2);
                    o.v2(&v3);
                }
                5 => o.v2(&a.dn(ua, n)?),
                6 => o.p2(&a.eval_d0(ua)?),
                7 => {
                    let r = a.eval_d1(ua)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                }
                8 => {
                    let r = a.eval_d2(ua)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                    o.v2(&r.d2);
                }
                9 => {
                    let r = a.eval_d3(ua)?;
                    o.p2(&r.point);
                    o.v2(&r.d1);
                    o.v2(&r.d2);
                    o.v2(&r.d3);
                }
                _ => o.v2(&a.eval_dn(ua, n)?),
            }
        }
        "eval_sweep" => {
            let (ranged, u1, u2) = (rd.b(), rd.f(), rd.f());
            rd.finish();
            let a = adaptor(&c, ranged, u1, u2)?;
            let (f, l) = (a.first_parameter()?, a.last_parameter()?);
            for k in 0..=12 {
                let mut u = f + (l - f) * (f64::from(k) / 12.0);
                if k % 3 == 1 {
                    u = f + (l - f) * (f64::from(12 - k) / 12.0);
                }
                let (p, v1, v2, v3) = a.d3(u)?;
                o.f(u);
                o.p2(&p);
                o.v2(&v1);
                o.v2(&v2);
                o.v2(&v3);
            }
        }
        "trim_copy" => {
            let (op, f, l) = (rd.i(), rd.f(), rd.f());
            rd.finish();
            let a = adaptor(&c, op == 2, f, l)?;
            let h = if op == 0 { a.trim(f, l, 1e-7)? } else { a.shallow_copy()? };
            o.i(0);
            o.i(h.get_type()? as i64);
            o.f(h.first_parameter()?);
            o.f(h.last_parameter()?);
            o.i(kind_of(h.geom2d_curve().as_ref()));
            let mut u = 0.5 * (h.first_parameter()? + h.last_parameter()?);
            if h.first_parameter()?.abs() > 1e50 || h.last_parameter()?.abs() > 1e50 {
                u = 0.25;
            }
            o.p2(&h.value(u)?);
        }
        "make_curve" => {
            let (ranged, u1, u2) = (rd.b(), rd.f(), rd.f());
            rd.finish();
            let a = adaptor(&c, ranged, u1, u2)?;
            let m = make_curve(&a)?;
            o.i(kind_of(Some(&m)));
            let (f, l) = (m.first_parameter(), m.last_parameter());
            o.f(f);
            o.f(l);
            let u = if f.abs() > 1e50 || l.abs() > 1e50 { 0.25 } else { 0.5 * (f + l) };
            o.p2(&m.value(u)?);
            if kind_of(Some(&m)) == 8 {
                let t = m.as_any().downcast_ref::<TrimmedCurve>().unwrap();
                o.i(kind_of(Some(&t.basis_curve())));
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

fn compare(t: &mut Tally, family: &str, line: &str, got: &Result<Vec<Tok>, AdaptorError>, want: &[&str]) {
    if want == ["throws"] {
        assert!(got.is_err(), "{family}: C++ throws but Rust returned Ok\n{line}");
        return;
    }
    let got = match got {
        Ok(g) => g,
        Err(e) => panic!("{family}: Rust returned Err({e:?}) but C++ did not throw\n{line}"),
    };
    assert_eq!(got.len(), want.len(), "{family}: output length\n{line}");
    let tolerant = ULP_FAMILIES.contains(&family);
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
                if tolerant {
                    let ulp = scale.max(f64::MIN_POSITIVE) * f64::EPSILON;
                    assert!(
                        v.to_bits() == e.to_bits() || (v - e).abs() <= 8.0 * ulp,
                        "{family} output {k}: {v:e} != {e:e} (scale {scale:e})\n{line}"
                    );
                } else {
                    assert_eq!(v.to_bits(), e.to_bits(), "{family} output {k}: {v:e} != {e:e}\n{line}");
                }
            }
        }
    }
}

#[test]
fn geom2dadaptor_matches_occt() {
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
        "geom2dadaptor: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
