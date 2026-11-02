//! Differential judge for OCCT's package `Geom2dGridEval` (OCCT 8.0.1): replays `tests/geom2dgrideval.txt`, printed by
//! `oracle/geom2dgrideval.cpp` against the real library. See `docs/api-geom2dgrideval.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::adaptor2d::{AdaptorError, Curve2dAdaptor, Line2d, OffsetCurve as OffsetAdaptor};
use occt_port::geom2d::{
    BSplineCurve, BezierCurve, Circle, Curve2d, Ellipse, Hyperbola, Line, OffsetCurve, Parabola, TrimmedCurve,
};
use occt_port::geom2dadaptor::Curve as GeomAdaptor;
use occt_port::geom2dgrideval as grid;
use occt_port::geom2dgrideval::GridEvaluator;
use occt_port::gp::{Ax2d, Dir2d, Pnt2d, Vec2d};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

const DATA: &str = include_str!("geom2dgrideval.txt");
const GOLDEN_LINES: usize = 944;
const MIN_DISTINCT: usize = 16;
const FAMILIES: &[&str] = &[
    "bezier_grid",
    "bezier_outside",
    "bspline_cache",
    "bspline_grid",
    "circle_grid",
    "curve_adaptor",
    "curve_geom",
    "curve_other",
    "curve_type",
    "dn_sweep",
    "ellipse_grid",
    "hyperbola_grid",
    "line_grid",
    "offset_grid",
    "offset_singular",
    "other_grid",
    "parabola_grid",
];
/// Families whose values come through `sin`, `cos`, `sinh`, `cosh`: 8 ulps of the largest value of the line.
const ULP_FAMILIES: &[&str] = &[
    "circle_grid",
    "ellipse_grid",
    "hyperbola_grid",
    "offset_grid",
    "other_grid",
    "curve_geom",
    "curve_adaptor",
    "curve_other",
    "curve_type",
    "dn_sweep",
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
    /// A curve of any kind, nested curves included: `Ok(None)` is the null handle (kind -1) and kind -2 (the oracle's
    /// constructors refused the curve; the case throws) is `Err`.
    fn curve(&mut self) -> Result<Crv, AdaptorError> {
        let kind = self.i();
        match kind {
            -1 => Ok(None),
            -2 => Err(AdaptorError::Construction),
            7 => {
                let off = self.f();
                let b = self.curve()?.ok_or(AdaptorError::NullObject)?;
                Ok(Some(Arc::new(OffsetCurve::new(b, off, false)?)))
            }
            8 => {
                let (u1, u2) = (self.f(), self.f());
                let b = self.curve()?.ok_or(AdaptorError::NullObject)?;
                Ok(Some(Arc::new(TrimmedCurve::new(b, u1, u2, true, true)?)))
            }
            _ => {
                self.p -= 1;
                Ok(Some(self.base()?.0))
            }
        }
    }
    /// The parameters: their number and the values.
    fn params(&mut self) -> Vec<f64> {
        let n = self.i();
        (0..n).map(|_| self.f()).collect()
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
    fn p2(&mut self, p: &Pnt2d) {
        self.f(p.x());
        self.f(p.y());
    }
    fn v2(&mut self, v: &Vec2d) {
        self.f(v.x());
        self.f(v.y());
    }
}

/// The operation `op` (0 D0, 1 D1, 2 D2, 3 D3, 4 DN of order `n`) of an evaluator on the parameters.
fn emit<E: GridEvaluator + ?Sized>(e: &E, op: i32, n: i32, ps: &[f64], o: &mut Out) -> Result<(), AdaptorError> {
    match op {
        0 => {
            let r = e.evaluate_grid(ps)?;
            o.i(r.len() as i64);
            for p in &r {
                o.p2(p);
            }
        }
        1 => {
            let r = e.evaluate_grid_d1(ps)?;
            o.i(r.len() as i64);
            for x in &r {
                o.p2(&x.point);
                o.v2(&x.d1);
            }
        }
        2 => {
            let r = e.evaluate_grid_d2(ps)?;
            o.i(r.len() as i64);
            for x in &r {
                o.p2(&x.point);
                o.v2(&x.d1);
                o.v2(&x.d2);
            }
        }
        3 => {
            let r = e.evaluate_grid_d3(ps)?;
            o.i(r.len() as i64);
            for x in &r {
                o.p2(&x.point);
                o.v2(&x.d1);
                o.v2(&x.d2);
                o.v2(&x.d3);
            }
        }
        _ => {
            let r = e.evaluate_grid_dn(ps, n)?;
            o.i(r.len() as i64);
            for v in &r {
                o.v2(v);
            }
        }
    }
    Ok(())
}

fn down<T: 'static>(c: &Crv) -> Option<&T> {
    c.as_ref().and_then(|c| c.as_any().downcast_ref::<T>())
}

fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, AdaptorError> {
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let mut o = Out::default();
    match family {
        "line_grid" | "circle_grid" | "ellipse_grid" | "hyperbola_grid" | "parabola_grid" | "bezier_grid"
        | "bspline_grid" | "bspline_cache" | "bezier_outside" | "offset_grid" | "offset_singular" => {
            let c = rd.curve()?;
            let ps = rd.params();
            let (op, n) = (rd.i(), rd.i());
            rd.finish();
            match family {
                "line_grid" => emit(&grid::Line::new(down::<Line>(&c)), op, n, &ps, &mut o)?,
                "circle_grid" => emit(&grid::Circle::new(down::<Circle>(&c)), op, n, &ps, &mut o)?,
                "ellipse_grid" => emit(&grid::Ellipse::new(down::<Ellipse>(&c)), op, n, &ps, &mut o)?,
                "hyperbola_grid" => emit(&grid::Hyperbola::new(down::<Hyperbola>(&c)), op, n, &ps, &mut o)?,
                "parabola_grid" => emit(&grid::Parabola::new(down::<Parabola>(&c)), op, n, &ps, &mut o)?,
                "bezier_grid" | "bezier_outside" => {
                    emit(&grid::BezierCurve::new(down::<BezierCurve>(&c)), op, n, &ps, &mut o)?;
                }
                "bspline_grid" | "bspline_cache" => {
                    emit(&grid::BSplineCurve::new(down::<BSplineCurve>(&c)), op, n, &ps, &mut o)?;
                }
                _ => emit(&grid::OffsetCurve::new(down::<OffsetCurve>(&c)), op, n, &ps, &mut o)?,
            }
        }
        "other_grid" | "curve_other" => {
            let v = rd.i();
            // 0 line adaptor, 1 offset adaptor (curve_other); 0 Geom2dAdaptor_Curve, 1 line, 2 offset (other_grid)
            let is_line = if family == "other_grid" { v == 1 } else { v == 0 };
            let adaptor: Arc<dyn Curve2dAdaptor> = if is_line {
                let p = Pnt2d::new(rd.f(), rd.f());
                let d = Dir2d::new(rd.f(), rd.f())?;
                let (u0, u1) = (rd.f(), rd.f());
                Arc::new(Line2d::with_point_dir(p, d, u0, u1))
            } else {
                let offset = (family == "curve_other" || v == 2).then(|| rd.f());
                let c = rd.curve()?.ok_or(AdaptorError::NullObject)?;
                let ga: Arc<dyn Curve2dAdaptor> = Arc::new(GeomAdaptor::with_curve(c)?);
                match offset {
                    Some(off) => Arc::new(OffsetAdaptor::with_offset(ga, off)?),
                    None => ga,
                }
            };
            let ps = rd.params();
            let (op, n) = (rd.i(), rd.i());
            rd.finish();
            if family == "other_grid" {
                emit(&grid::OtherCurve::new(adaptor.as_ref()), op, n, &ps, &mut o)?;
            } else {
                let e = grid::Curve::from_adaptor(adaptor.as_ref())?;
                o.i(e.curve_type() as i64);
                emit(&e, op, n, &ps, &mut o)?;
            }
        }
        "curve_geom" => {
            let c = rd.curve()?;
            let ps = rd.params();
            let (op, n) = (rd.i(), rd.i());
            rd.finish();
            let e = grid::Curve::from_curve(c);
            o.i(e.curve_type() as i64);
            emit(&e, op, n, &ps, &mut o)?;
        }
        "curve_adaptor" => {
            let v = rd.i();
            let a = if v == 2 {
                GeomAdaptor::new()
            } else {
                let c = rd.curve()?.ok_or(AdaptorError::NullObject)?;
                if v == 1 {
                    let (u1, u2) = (rd.f(), rd.f());
                    GeomAdaptor::with_range(c, u1, u2)?
                } else {
                    GeomAdaptor::with_curve(c)?
                }
            };
            let ps = rd.params();
            let (op, n) = (rd.i(), rd.i());
            rd.finish();
            let e = grid::Curve::from_adaptor(&a)?;
            o.i(e.curve_type() as i64);
            emit(&e, op, n, &ps, &mut o)?;
        }
        "curve_type" => {
            let c = rd.curve()?;
            let ps = rd.params();
            rd.finish();
            let e = grid::Curve::from_curve(c);
            o.i(e.curve_type() as i64);
            emit(&e, 0, 0, &ps, &mut o)?;
        }
        "dn_sweep" => {
            let c = rd.curve()?;
            let ps = rd.params();
            let n = rd.i();
            rd.finish();
            emit(&grid::Curve::from_curve(c), 4, n, &ps, &mut o)?;
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
fn geom2dgrideval_matches_occt() {
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
        "geom2dgrideval: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
