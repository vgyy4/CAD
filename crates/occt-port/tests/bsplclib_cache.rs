//! Differential judge for `BSplCLib_Cache`, `BSplCLib_CacheParams`, the span-index `BSplCLib::BuildCache`,
//! `BSplCLib::Intervals` and `BSplCLib::FlatBezierKnots` (OCCT 8.0.1): replays `tests/bsplclib_cache.txt`, printed by
//! `oracle/bsplclib_cache.cpp` against the real library. See `docs/api-bsplclib_cache.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::bsplclib::BSplError;
use occt_port::bsplclib::{curve2d, curve3d};
use occt_port::bsplclib_cache::{Cache, CacheParams, flat_bezier_knots, intervals};
use occt_port::gp::{Pnt, Pnt2d};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("bsplclib_cache.txt");
const GOLDEN_LINES: usize = 261;
const MIN_DISTINCT: usize = 25;
const FAMILIES: &[&str] = &["build_cache", "cache", "cache_params", "flat_bezier_knots", "intervals"];

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
    fn da(&mut self) -> Vec<f64> {
        let n = self.i();
        (0..n).map(|_| self.f()).collect()
    }
    fn ja(&mut self) -> Vec<i32> {
        let n = self.i();
        (0..n).map(|_| self.i()).collect()
    }
    fn pa(&mut self) -> Vec<Pnt2d> {
        let n = self.i();
        (0..n).map(|_| Pnt2d::new(self.f(), self.f())).collect()
    }
    fn qa(&mut self) -> Vec<Pnt> {
        let n = self.i();
        (0..n).map(|_| Pnt::new(self.f(), self.f(), self.f())).collect()
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
    fn b(&mut self, v: bool) {
        self.i(v.into());
    }
}

fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, BSplError> {
    let mut rd = Rd::new(words);
    let rd = &mut rd;
    let mut o = Out::default();
    match family {
        "flat_bezier_knots" => {
            let d = rd.i();
            rd.finish();
            for v in flat_bezier_knots(d)? {
                o.f(v);
            }
        }
        "intervals" => {
            let (degree, periodic, cont, f, l, tol) = (rd.i(), rd.b(), rd.i(), rd.f(), rd.f(), rd.f());
            let (knots, mults) = (rd.da(), rd.ja());
            rd.finish();
            let (n0, _) = intervals(&knots, &mults, degree, periodic, cont, f, l, tol, false)?;
            let (n1, out) = intervals(&knots, &mults, degree, periodic, cont, f, l, tol, true)?;
            let out = out.unwrap();
            o.i(n0.into());
            o.i(n1.into());
            o.i(out.len() as i64);
            for v in out {
                o.f(v);
            }
        }
        "cache_params" => {
            let (degree, periodic, flat) = (rd.i(), rd.b(), rd.da());
            let mut us = Vec::new();
            while rd.p < rd.w.len() {
                us.push(rd.f());
            }
            let mut p = CacheParams::new(degree, periodic, &flat)?;
            o.f(p.first_parameter);
            o.f(p.last_parameter);
            o.i(p.span_index_min.into());
            o.i(p.span_index_max.into());
            o.f(p.span_start);
            o.f(p.span_length);
            o.i(p.span_index.into());
            for u in us {
                o.f(p.periodic_normalization(u));
                o.b(p.is_cache_valid(u));
                let mut nu = u;
                p.locate_parameter(&mut nu, &flat)?;
                o.f(nu);
                o.i(p.span_index.into());
                o.f(p.span_start);
                o.f(p.span_length);
                o.b(p.is_cache_valid(u));
            }
        }
        "build_cache" => {
            let (dim3, wmode, degree, periodic, nu, span_len, span_index) =
                (rd.b(), rd.i(), rd.i(), rd.b(), rd.f(), rd.f(), rd.i());
            let flat = rd.da();
            let has_w = wmode != 0;
            let cache = if dim3 {
                let poles = rd.qa();
                let w = has_w.then(|| rd.da());
                rd.finish();
                curve3d::build_cache_span(nu, span_len, periodic, degree, span_index, &flat, &poles, w.as_deref())?
            } else {
                let poles = rd.pa();
                let w = has_w.then(|| rd.da());
                rd.finish();
                curve2d::build_cache_span(nu, span_len, periodic, degree, span_index, &flat, &poles, w.as_deref())?
            };
            for v in cache {
                o.f(v);
            }
        }
        "cache" => {
            let (dim3, _has_w0, degree, periodic) = (rd.b(), rd.i(), rd.i(), rd.b());
            let flat = rd.da();
            let (p2, p3) = if dim3 { (Vec::new(), rd.qa()) } else { (rd.pa(), Vec::new()) };
            let has_w = rd.b();
            let w = has_w.then(|| rd.da());
            let nus = rd.i();
            let us: Vec<f64> = (0..nus).map(|_| rd.f()).collect();
            rd.finish();
            let u0 = us[0];
            let wp = w.as_deref();
            if dim3 {
                let mut c = Cache::new_3d(degree, periodic, &flat, wp)?;
                c.build_cache_3d(u0, &flat, &p3, wp)?;
                for &u in &us {
                    o.b(c.is_cache_valid(u));
                    let pr = |o: &mut Out, p: &Pnt| {
                        o.f(p.x());
                        o.f(p.y());
                        o.f(p.z());
                    };
                    let vr = |o: &mut Out, v: &occt_port::gp::Vec| {
                        o.f(v.x());
                        o.f(v.y());
                        o.f(v.z());
                    };
                    pr(&mut o, &c.d0_3d(u)?);
                    let (p, v1) = c.d1_3d(u)?;
                    pr(&mut o, &p);
                    vr(&mut o, &v1);
                    vr(&mut o, &c.d2_3d(u)?.2);
                    vr(&mut o, &c.d3_3d(u)?.3);
                    let lp = 0.375 * (u - u0) + 0.5;
                    pr(&mut o, &c.d0_local_3d(lp)?);
                    vr(&mut o, &c.d1_local_3d(lp)?.1);
                    vr(&mut o, &c.d2_local_3d(lp)?.2);
                    vr(&mut o, &c.d3_local_3d(lp)?.3);
                }
            } else {
                let mut c = Cache::new_2d(degree, periodic, &flat, wp)?;
                c.build_cache_2d(u0, &flat, &p2, wp)?;
                for &u in &us {
                    o.b(c.is_cache_valid(u));
                    let pr = |o: &mut Out, p: &Pnt2d| {
                        o.f(p.x());
                        o.f(p.y());
                    };
                    let vr = |o: &mut Out, v: &occt_port::gp::Vec2d| {
                        o.f(v.x());
                        o.f(v.y());
                    };
                    pr(&mut o, &c.d0_2d(u)?);
                    let (p, v1) = c.d1_2d(u)?;
                    pr(&mut o, &p);
                    vr(&mut o, &v1);
                    vr(&mut o, &c.d2_2d(u)?.2);
                    vr(&mut o, &c.d3_2d(u)?.3);
                    let lp = 0.375 * (u - u0) + 0.5;
                    pr(&mut o, &c.d0_local_2d(lp)?);
                    vr(&mut o, &c.d1_local_2d(lp)?.1);
                    vr(&mut o, &c.d2_local_2d(lp)?.2);
                    vr(&mut o, &c.d3_local_2d(lp)?.3);
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

fn compare(t: &mut Tally, family: &str, line: &str, got: &Result<Vec<Tok>, BSplError>, want: &[&str]) {
    if want == ["throws"] {
        assert!(got.is_err(), "{family}: C++ throws but Rust returned Ok\n{line}");
        return;
    }
    let got = match got {
        Ok(g) => g,
        Err(e) => panic!("{family}: Rust returned Err({e:?}) but C++ did not throw\n{line}"),
    };
    assert_eq!(got.len(), want.len(), "{family}: output length\n{line}");
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
                assert_eq!(v.to_bits(), e.to_bits(), "{family} output {k}: {v:e} != {e:e}\n{line}");
            }
        }
    }
}

#[test]
fn bsplclib_cache_matches_occt() {
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
        let got = run(family, &words);
        compare(&mut tally, family, line, &got, &want);
        lines += 1;
    }
    assert_eq!(lines, GOLDEN_LINES);
    let mut want_fams = FAMILIES.to_vec();
    want_fams.sort_unstable();
    assert_eq!(counts.keys().map(String::as_str).collect::<Vec<_>>(), want_fams);
    for name in FAMILIES {
        assert!(distinct[*name].len() >= MIN_DISTINCT.min(counts[*name]), "{name} needs distinct inputs");
    }
    println!(
        "bsplclib_cache: {} families, {lines} lines, {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
