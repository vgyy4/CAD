//! Differential judge for OCCT's package `Geom2dHash` (OCCT 8.0.1): replays `tests/geom2dhash.txt`, printed by
//! `oracle/geom2dhash.cpp` against the real library. See `docs/api-geom2dhash.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::adaptor2d::AdaptorError;
use occt_port::geom2d::{
    BSplineCurve, BezierCurve, Circle, Curve2d, Ellipse, Hyperbola, Line, OffsetCurve, Parabola, TrimmedCurve,
};
use occt_port::geom2deval::{ArchimedeanSpiralCurve, SineWaveCurve};
use occt_port::geom2dhash as h;
use occt_port::gp::{Ax2d, Ax22d, Dir2d, Pnt2d};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

const DATA: &str = include_str!("geom2dhash.txt");
const GOLDEN_LINES: usize = 750;
const MIN_DISTINCT: usize = 16;
const FAMILIES: &[&str] = &[
    "axis_eq",
    "axis_hash",
    "bezier_eq",
    "bezier_hash",
    "bspline_eq",
    "bspline_hash",
    "circle_eq",
    "circle_hash",
    "cstring",
    "curve_eq",
    "curve_hash",
    "dir_eq",
    "dir_hash",
    "ellipse_eq",
    "ellipse_hash",
    "fnv",
    "hash_int",
    "hash_int64",
    "hyperbola_eq",
    "hyperbola_hash",
    "line_eq",
    "line_hash",
    "murmur",
    "offset_eq",
    "offset_hash",
    "parabola_eq",
    "parabola_hash",
    "point_eq",
    "point_hash",
    "trimmed_eq",
    "trimmed_hash",
];

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // the float variant is unused: every value of this judge is an integer
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
            13 | 14 => {
                let p = self.p2();
                let d = Dir2d::new(self.f(), self.f())?;
                let ax = Ax2d::new(p, d);
                let (a, b) = (self.f(), self.f());
                if kind == 13 {
                    Ok(Some(Arc::new(ArchimedeanSpiralCurve::new(ax, a, b)?)))
                } else {
                    let ph = self.f();
                    Ok(Some(Arc::new(SineWaveCurve::new(ax, a, b, ph)?)))
                }
            }
            _ => {
                self.p -= 1;
                Ok(Some(self.base()?.0))
            }
        }
    }
    /// The hasher tolerances: the index (0: the default constructor) and the two values.
    fn tol(&mut self) -> (i32, f64, f64) {
        (self.i(), self.f(), self.f())
    }
    fn u64(&mut self) -> u64 {
        self.next().parse().unwrap()
    }
}

/// The hashers of one curve class, constructed from the printed tolerances.
trait Hasher<T> {
    fn make(tol: (i32, f64, f64)) -> Self;
    fn hash(&self, a: &T) -> u64;
    fn same(&self, a: &T, b: &T) -> bool;
}
macro_rules! impl_hasher {
    ($H:ty, $T:ty) => {
        impl Hasher<$T> for $H {
            fn make(tol: (i32, f64, f64)) -> Self {
                if tol.0 == 0 { <$H>::default() } else { <$H>::new(tol.1, tol.2) }
            }
            fn hash(&self, a: &$T) -> u64 {
                <$H>::hash(self, a)
            }
            fn same(&self, a: &$T, b: &$T) -> bool {
                <$H>::equal(self, a, b)
            }
        }
    };
}
impl_hasher!(h::PointHasher, Pnt2d);
impl_hasher!(h::DirectionHasher, Dir2d);
impl_hasher!(h::AxisPlacement, Ax22d);
impl_hasher!(h::LineHasher, Line);
impl_hasher!(h::CircleHasher, Circle);
impl_hasher!(h::EllipseHasher, Ellipse);
impl_hasher!(h::HyperbolaHasher, Hyperbola);
impl_hasher!(h::ParabolaHasher, Parabola);
impl_hasher!(h::BezierCurveHasher, BezierCurve);
impl_hasher!(h::BSplineCurveHasher, BSplineCurve);
impl_hasher!(h::TrimmedCurveHasher, TrimmedCurve);
impl_hasher!(h::OffsetCurveHasher, OffsetCurve);

fn down<T: 'static>(c: &Crv) -> Result<&T, AdaptorError> {
    c.as_ref().and_then(|c| c.as_any().downcast_ref::<T>()).ok_or(AdaptorError::NullObject)
}

fn curve_hasher(tol: (i32, f64, f64)) -> h::CurveHasher {
    if tol.0 == 0 { h::CurveHasher::default() } else { h::CurveHasher::new(tol.1, tol.2) }
}

/// A hash family (`eq == false`) or an equality family of the hasher `H` of the class `T`.
fn typed<T: 'static, H: Hasher<T>>(rd: &mut Rd<'_>, eq: bool) -> Result<Vec<Tok>, AdaptorError> {
    let tol = rd.tol();
    let a = rd.curve()?;
    if eq {
        let b = rd.curve()?;
        rd.finish();
        Ok(vec![Tok::I(H::make(tol).same(down::<T>(&a)?, down::<T>(&b)?).into())])
    } else {
        rd.finish();
        Ok(vec![Tok::I(H::make(tol).hash(down::<T>(&a)?).into())])
    }
}

fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, AdaptorError> {
    // a record whose curve the oracle's constructors refused (kind -2, before the tolerances); `hash_int*` have the value -2
    if words.first() == Some(&"-2") && !family.starts_with("hash_") {
        return Err(AdaptorError::Construction);
    }
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let bytes = |rd: &mut Rd<'_>| -> Vec<u8> {
        let n = rd.i();
        (0..n).map(|_| u8::try_from(rd.i()).unwrap()).collect()
    };
    let out = |v: u64| Ok(vec![Tok::I(v.into())]);
    match family {
        "murmur" => {
            let b = bytes(rd);
            let seed = rd.u64();
            rd.finish();
            out(h::murmur_hash64a(&b, seed))
        }
        "fnv" => {
            let b = bytes(rd);
            let seed = rd.u64();
            rd.finish();
            out(h::fnv_hash64a(&b, seed))
        }
        "cstring" => {
            let b = bytes(rd);
            rd.finish();
            out(h::cstring_hash(&b))
        }
        "hash_int" => {
            let v = rd.i();
            rd.finish();
            out(h::hash_i32(v))
        }
        "hash_int64" => {
            let v: i64 = rd.next().parse().unwrap();
            rd.finish();
            out(h::hash_i64(v))
        }
        "point_hash" | "point_eq" => {
            let tol = rd.tol();
            let (p, q) = (rd.p2(), (family == "point_eq").then(|| rd.p2()));
            rd.finish();
            let hs = <h::PointHasher as Hasher<Pnt2d>>::make(tol);
            match q {
                Some(q) => Ok(vec![Tok::I(hs.same(&p, &q).into())]),
                None => out(hs.hash(&p)),
            }
        }
        "dir_hash" | "dir_eq" => {
            let tol = rd.tol();
            let a = Dir2d::new(rd.f(), rd.f())?;
            let b = if family == "dir_eq" { Some(Dir2d::new(rd.f(), rd.f())?) } else { None };
            rd.finish();
            let hs = <h::DirectionHasher as Hasher<Dir2d>>::make(tol);
            match b {
                Some(b) => Ok(vec![Tok::I(hs.same(&a, &b).into())]),
                None => out(hs.hash(&a)),
            }
        }
        "axis_hash" | "axis_eq" => {
            let tol = rd.tol();
            let axis = |rd: &mut Rd<'_>| -> Result<Ax22d, AdaptorError> {
                let p = rd.p2();
                let d = Dir2d::new(rd.f(), rd.f())?;
                let sense = rd.b();
                Ok(Ax22d::from_ax2d(Ax2d::new(p, d), sense))
            };
            let a = axis(rd)?;
            let b = if family == "axis_eq" { Some(axis(rd)?) } else { None };
            rd.finish();
            let hs = <h::AxisPlacement as Hasher<Ax22d>>::make(tol);
            match b {
                Some(b) => Ok(vec![Tok::I(hs.same(&a, &b).into())]),
                None => out(hs.hash(&a)),
            }
        }
        "line_hash" | "line_eq" => typed::<Line, h::LineHasher>(rd, family.ends_with("_eq")),
        "circle_hash" | "circle_eq" => typed::<Circle, h::CircleHasher>(rd, family.ends_with("_eq")),
        "ellipse_hash" | "ellipse_eq" => typed::<Ellipse, h::EllipseHasher>(rd, family.ends_with("_eq")),
        "hyperbola_hash" | "hyperbola_eq" => typed::<Hyperbola, h::HyperbolaHasher>(rd, family.ends_with("_eq")),
        "parabola_hash" | "parabola_eq" => typed::<Parabola, h::ParabolaHasher>(rd, family.ends_with("_eq")),
        "bezier_hash" | "bezier_eq" => typed::<BezierCurve, h::BezierCurveHasher>(rd, family.ends_with("_eq")),
        "bspline_hash" | "bspline_eq" => typed::<BSplineCurve, h::BSplineCurveHasher>(rd, family.ends_with("_eq")),
        "trimmed_hash" | "trimmed_eq" => typed::<TrimmedCurve, h::TrimmedCurveHasher>(rd, family.ends_with("_eq")),
        "offset_hash" | "offset_eq" => typed::<OffsetCurve, h::OffsetCurveHasher>(rd, family.ends_with("_eq")),
        "curve_hash" => {
            let tol = rd.tol();
            let c = rd.curve()?;
            rd.finish();
            out(curve_hasher(tol).hash(c.as_ref()))
        }
        "curve_eq" => {
            let tol = rd.tol();
            let v = rd.i();
            let (a, b): (Crv, Crv) = match v {
                6 => {
                    // the null handle and the index of the record
                    let _ = (rd.i(), rd.i());
                    (None, None)
                }
                5 => {
                    let _ = rd.curve()?;
                    let b = rd.curve()?;
                    (None, b)
                }
                0 => {
                    let a = rd.curve()?;
                    (a.clone(), a)
                }
                7 => (rd.curve()?, None),
                _ => (rd.curve()?, rd.curve()?),
            };
            rd.finish();
            Ok(vec![Tok::I(curve_hasher(tol).equal(a.as_ref(), b.as_ref()).into())])
        }
        _ => panic!("unknown family {family}"),
    }
}

/// Every value of this judge is an integer (hashes, booleans): all comparisons are exact.
const ULP_FAMILIES: &[&str] = &[];

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
fn geom2dhash_matches_occt() {
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
        "geom2dhash: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
