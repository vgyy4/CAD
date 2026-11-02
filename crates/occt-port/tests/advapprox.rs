//! Differential judge for OCCT's package `AdvApprox` (OCCT 8.0.1): replays `tests/advapprox.txt`, printed by
//! `oracle/advapprox.cpp` against the real library. See `docs/api-advapprox.md`.
//!
//! The function to approximate is the test evaluator `Tf` of the oracle: up to 8 scalar components (polynomial,
//! rational, sine, exponential, cubic hinge, square root, jump) defined by the records `def_func_exact` and
//! `def_func_trig`, which folds every call into a counter and a hash.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
#![allow(clippy::too_many_lines, clippy::type_complexity)]
use occt_port::advapprox::{
    AdvApproxError, ApproxAFunction, Cutting, DichoCutting, EvaluatorFunction, PrefAndRec, PrefCutting, SimpleApprox,
};
use occt_port::gp::{Pnt, Pnt2d};
use occt_port::plib::{Array2, GeomAbsShape, JacobiPolynomial};
use std::collections::{BTreeMap, BTreeSet, HashSet};

type AResult<T> = Result<T, AdvApproxError>;

const DATA: &str = include_str!("advapprox.txt");
const GOLDEN_LINES: usize = 616;
const MIN_DISTINCT: usize = 2;
const FAMILIES: &[&str] = &[
    "afunc_access",
    "afunc_bad",
    "afunc_cut_exact",
    "afunc_cut_trig",
    "afunc_dicho_exact",
    "afunc_dicho_trig",
    "afunc_dump",
    "afunc_err",
    "afunc_jump",
    "afunc_poles",
    "approximation_bad",
    "approximation_err",
    "approximation_exact",
    "approximation_trig",
    "cut_dicho",
    "cut_pref",
    "cut_prefrec",
    "def_func_exact",
    "def_func_trig",
    "simple_bad",
    "simple_err",
    "simple_exact",
    "simple_trig",
];
const SHAPES: [GeomAbsShape; 7] = [
    GeomAbsShape::C0,
    GeomAbsShape::G1,
    GeomAbsShape::C1,
    GeomAbsShape::G2,
    GeomAbsShape::C2,
    GeomAbsShape::C3,
    GeomAbsShape::Cn,
];
const H0: u64 = 0xcbf2_9ce4_8422_2325;

fn mixh(h: u64, x: u64) -> u64 {
    (h ^ x).wrapping_mul(0x0000_0100_0000_01b3)
}
fn fnv(s: &str) -> u64 {
    s.bytes().fold(H0, |h, c| mixh(h, u64::from(c)))
}

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
    /// `count` floats (a negative count reads none).
    fn fs(&mut self, count: i32) -> Vec<f64> {
        (0..count.max(0)).map(|_| self.f()).collect()
    }
    fn vec_f(&mut self) -> Vec<f64> {
        let n = self.i();
        self.fs(n)
    }
    fn more(&self) -> bool {
        self.p < self.w.len()
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
}

// ---- the test functions -----------------------------------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
struct Comp {
    kind: i32,
    p: [f64; 5],
}
#[derive(Debug, Clone)]
struct TfDef {
    comps: Vec<Comp>,
}

/// The series of the oracle (`msin`, `mcos`, `mexp`).
fn msin(x: f64) -> f64 {
    let (mut term, mut sum) = (x, x);
    for k in 1..30 {
        term = -term * x * x / f64::from((2 * k) * (2 * k + 1));
        sum += term;
    }
    sum
}
fn mcos(x: f64) -> f64 {
    let (mut term, mut sum) = (1.0, 1.0);
    for k in 1..30 {
        term = -term * x * x / f64::from((2 * k - 1) * (2 * k));
        sum += term;
    }
    sum
}
fn mexp(x: f64) -> f64 {
    let (mut term, mut sum) = (1.0, 1.0);
    for k in 1..60 {
        term = term * x / f64::from(k);
        sum += term;
    }
    sum
}

fn comp_value(c: &Comp, t: f64, d: i32) -> f64 {
    let p = &c.p;
    match c.kind {
        0 => match d {
            0 => (((p[4] * t + p[3]) * t + p[2]) * t + p[1]) * t + p[0],
            1 => ((4.0 * p[4] * t + 3.0 * p[3]) * t + 2.0 * p[2]) * t + p[1],
            _ => (12.0 * p[4] * t + 6.0 * p[3]) * t + 2.0 * p[2],
        },
        1 => {
            let s = t - p[1];
            let q = 1.0 + p[2] * s * s;
            let q1 = 2.0 * p[2] * s;
            match d {
                0 => p[0] / q,
                1 => -p[0] * q1 / (q * q),
                _ => p[0] * (2.0 * q1 * q1 / (q * q * q) - 2.0 * p[2] / (q * q)),
            }
        }
        2 => {
            let a = p[1] * t + p[2];
            match d {
                0 => p[3] + p[0] * msin(a),
                1 => p[0] * p[1] * mcos(a),
                _ => -p[0] * p[1] * p[1] * msin(a),
            }
        }
        3 => {
            let e = mexp(p[1] * t);
            match d {
                0 => p[0] * e,
                1 => p[0] * p[1] * e,
                _ => p[0] * p[1] * p[1] * e,
            }
        }
        4 => {
            let s = t - p[1];
            if s <= 0.0 || s.is_nan() {
                return 0.0;
            }
            match d {
                0 => p[0] * s * s * s,
                1 => 3.0 * p[0] * s * s,
                _ => 6.0 * p[0] * s,
            }
        }
        6 => {
            if d == 0 && t > p[1] {
                p[0]
            } else {
                0.0
            }
        }
        _ => {
            let r = ((t - p[1] + p[2]).abs() + p[3]).sqrt();
            match d {
                0 => p[0] * r,
                1 => p[0] / (2.0 * r),
                _ => -p[0] / (4.0 * r * r * r),
            }
        }
    }
}
/// The evaluator of the oracle (`TF`): errors above `err_above` (and below `err_below`) and the log.
#[derive(Debug)]
struct Tf {
    def: TfDef,
    err_above: f64,
    err_code: i32,
    err_below: f64,
    calls: i32,
    hash: u64,
}
impl Tf {
    fn new(def: &TfDef, err_above: f64, err_code: i32, err_below: f64) -> Self {
        Self { def: def.clone(), err_above, err_code, err_below, calls: 0, hash: H0 }
    }
    fn plain(def: &TfDef) -> Self {
        Self::new(def, 1.0e300, 5, 1.0e300)
    }
    fn log(&self, o: &mut Out) {
        o.i(self.calls.into());
        o.u(self.hash);
    }
}
impl EvaluatorFunction for Tf {
    fn evaluate(
        &mut self,
        dimension: i32,
        start_end: [f64; 2],
        parameter: f64,
        derivative_request: i32,
        result: &mut [f64],
    ) -> i32 {
        self.calls += 1;
        self.hash = mixh(self.hash, i64::from(dimension) as u64);
        self.hash = mixh(self.hash, start_end[0].to_bits());
        self.hash = mixh(self.hash, start_end[1].to_bits());
        self.hash = mixh(self.hash, parameter.to_bits());
        self.hash = mixh(self.hash, i64::from(derivative_request) as u64);
        let n = i32::try_from(self.def.comps.len()).unwrap();
        if dimension < 1 || dimension > n {
            return 9;
        }
        if !(0..=2).contains(&derivative_request) {
            return 3;
        }
        if parameter > self.err_above && parameter < self.err_below {
            return self.err_code;
        }
        for (c, r) in self.def.comps.iter().zip(result.iter_mut()).take(dimension as usize) {
            *r = comp_value(c, parameter, derivative_request);
        }
        0
    }
}

#[derive(Default)]
struct World {
    defs: BTreeMap<i32, TfDef>,
}
impl World {
    fn def(&self, f: i32) -> &TfDef {
        self.defs.get(&f).unwrap()
    }
}

// ---- cutting specifications and the inputs of ApproxAFunction -----------------------------------------------------------
struct CutSpec {
    kind: i32,
    a: Vec<f64>,
    b: Vec<f64>,
    weight: f64,
}
fn read_cut(rd: &mut Rd) -> CutSpec {
    let kind = rd.i();
    let (mut a, mut b, mut weight) = (Vec::new(), Vec::new(), 2.0);
    if kind == 1 {
        a = rd.vec_f();
    }
    if kind == 2 {
        a = rd.vec_f();
        b = rd.vec_f();
        weight = rd.f();
    }
    CutSpec { kind, a, b, weight }
}
fn make_cut(c: &CutSpec) -> AResult<Box<dyn Cutting>> {
    Ok(match c.kind {
        0 => Box::new(DichoCutting),
        1 => Box::new(PrefCutting::new(&c.a)),
        _ => Box::new(PrefAndRec::new(&c.a, &c.b, c.weight)?),
    })
}

struct AfIn {
    f: i32,
    n1: i32,
    n2: i32,
    n3: i32,
    t1: Vec<f64>,
    t2: Vec<f64>,
    t3: Vec<f64>,
    first: f64,
    last: f64,
    cont: usize,
    maxdeg: i32,
    maxseg: i32,
    err_above: f64,
    err_code: i32,
    cut: Option<CutSpec>,
}
/// `custom`: whether a cutting specification follows (`None`: the rest of the line decides).
fn read_af(rd: &mut Rd, custom: Option<bool>) -> AfIn {
    let (f, n1, n2, n3) = (rd.i(), rd.i(), rd.i(), rd.i());
    let t1 = rd.fs(n1);
    let t2 = rd.fs(n2);
    let t3 = rd.fs(n3);
    let (first, last) = (rd.f(), rd.f());
    let (cont, maxdeg, maxseg) = (rd.i() as usize, rd.i(), rd.i());
    let (err_above, err_code) = (rd.f(), rd.i());
    let custom = custom.unwrap_or_else(|| rd.more());
    let cut = if custom { Some(read_cut(rd)) } else { None };
    AfIn { f, n1, n2, n3, t1, t2, t3, first, last, cont, maxdeg, maxseg, err_above, err_code, cut }
}
fn build_af(world: &World, x: &AfIn) -> AResult<(ApproxAFunction, Tf)> {
    let mut tf = Tf::new(world.def(x.f), x.err_above, x.err_code, 1.0e300);
    let a = match &x.cut {
        None => ApproxAFunction::new(
            x.n1,
            x.n2,
            x.n3,
            &x.t1,
            &x.t2,
            &x.t3,
            x.first,
            x.last,
            SHAPES[x.cont],
            x.maxdeg,
            x.maxseg,
            &mut tf,
        )?,
        Some(c) => {
            let cut = make_cut(c)?;
            ApproxAFunction::with_cutting(
                x.n1,
                x.n2,
                x.n3,
                &x.t1,
                &x.t2,
                &x.t3,
                x.first,
                x.last,
                SHAPES[x.cont],
                x.maxdeg,
                x.maxseg,
                &mut tf,
                cut.as_ref(),
            )?
        }
    };
    Ok((a, tf))
}

fn pr_matrix(o: &mut Out, flat: &[f64], rows: usize, cols: usize, full: bool) {
    o.i(rows as i64);
    o.i(cols as i64);
    if !full {
        o.u(flat.iter().fold(H0, |h, x| mixh(h, x.to_bits())));
    }
    if full || rows <= 2 {
        for x in flat {
            o.f(*x);
        }
    } else {
        for x in &flat[..cols] {
            o.f(*x);
        }
        for j in 0..cols {
            o.f(flat[(rows - 1) * cols + j]);
        }
    }
}
fn pr_harr(o: &mut Out, h: Option<&[f64]>) {
    o.b(h.is_none());
    if let Some(h) = h {
        o.i(h.len() as i64);
        for x in h {
            o.f(*x);
        }
    }
}
fn pr_af(o: &mut Out, a: &ApproxAFunction, x: &AfIn, full: bool) -> AResult<()> {
    o.b(a.is_done());
    o.b(a.has_result());
    if !a.has_result() {
        return Ok(());
    }
    o.i(a.nb_poles()?.into());
    o.i(a.degree().into());
    o.i(a.nb_knots()?.into());
    for k in a.knots().unwrap() {
        o.f(*k);
    }
    for m in a.multiplicities().unwrap() {
        o.i((*m).into());
    }
    o.i(a.num_sub_spaces(1)?.into());
    o.i(a.num_sub_spaces(2)?.into());
    o.i(a.num_sub_spaces(3)?.into());
    if x.n1 > 0 {
        let p: &Array2<f64> = a.poles_1d().unwrap();
        pr_matrix(o, p.as_slice(), p.rows(), p.cols(), full);
        pr_harr(o, a.max_error_array(1)?);
        pr_harr(o, a.average_error_array(1)?);
    }
    if x.n2 > 0 {
        let p: &Array2<Pnt2d> = a.poles_2d().unwrap();
        let flat: Vec<f64> = p.as_slice().iter().flat_map(|q| [q.x(), q.y()]).collect();
        pr_matrix(o, &flat, p.rows(), p.cols() * 2, full);
        pr_harr(o, a.max_error_array(2)?);
        pr_harr(o, a.average_error_array(2)?);
    }
    if x.n3 > 0 {
        let p: &Array2<Pnt> = a.poles().unwrap();
        let flat: Vec<f64> = p.as_slice().iter().flat_map(|q| [q.x(), q.y(), q.z()]).collect();
        pr_matrix(o, &flat, p.rows(), p.cols() * 3, full);
        pr_harr(o, a.max_error_array(3)?);
        pr_harr(o, a.average_error_array(3)?);
    }
    let d = a.dump()?;
    o.i(d.len() as i64);
    o.u(fnv(&d));
    Ok(())
}

fn run(world: &mut World, family: &str, words: &[&str]) -> AResult<Vec<Tok>> {
    let mut rd = Rd::new(words);
    let mut o = Out::default();
    match family {
        "def_func_exact" | "def_func_trig" => {
            let f = rd.i();
            rd.finish();
            // the definition itself is data of the record: it is read from the golden line by the caller
            let def = world.def(f).clone();
            o.i(def.comps.len() as i64);
            for c in &def.comps {
                o.i(c.kind.into());
                for p in c.p {
                    o.f(p);
                }
            }
            let mut tf = Tf::plain(&def);
            let mut res = [0.0; 8];
            for t in [-0.7, 0.1, 0.9] {
                for d in 0..=2 {
                    let err = tf.evaluate(8, [-1.0, 1.0], t, d, &mut res);
                    o.i(err.into());
                    for x in res {
                        o.f(x);
                    }
                }
            }
            o.i(tf.evaluate(9, [0.0, 1.0], 0.5, 0, &mut res).into());
            o.i(tf.evaluate(3, [0.0, 1.0], 0.5, 3, &mut res).into());
            tf.log(&mut o);
        }
        "cut_dicho" => {
            let (a, b) = (rd.f(), rd.f());
            rd.finish();
            let (ok, v) = DichoCutting.value(a, b);
            o.b(ok);
            o.f(v);
        }
        "cut_pref" => {
            let pts = rd.vec_f();
            let (a, b) = (rd.f(), rd.f());
            rd.finish();
            let (ok, v) = PrefCutting::new(&pts).value(a, b);
            o.b(ok);
            o.f(v);
        }
        "cut_prefrec" => {
            let rec = rd.vec_f();
            let pref = rd.vec_f();
            let weight = rd.f();
            let (a, b) = (rd.f(), rd.f());
            rd.finish();
            let (ok, v) = PrefAndRec::new(&rec, &pref, weight)?.value(a, b);
            o.b(ok);
            o.f(v);
        }
        "simple_exact" | "simple_trig" | "simple_err" => {
            let (f, c, wd, ng, nss) = (rd.i(), rd.i() as usize, rd.i(), rd.i(), rd.i());
            let dims: Vec<i32> = (0..nss).map(|_| rd.i()).collect();
            let tols = rd.fs(nss);
            let (first, last, md, err_above, err_below, _j) = (rd.f(), rd.f(), rd.i(), rd.f(), rd.f(), rd.i());
            rd.finish();
            let niv = (c / 2) as i32;
            let total: i32 = dims.iter().sum();
            let mut tf = Tf::new(world.def(f), err_above, 5, err_below);
            {
                let jac = JacobiPolynomial::new(wd, SHAPES[c])?;
                let mut sa = SimpleApprox::new(total, nss, SHAPES[c], wd, ng, &jac, &mut tf)?;
                sa.perform(&dims, &tols, first, last, md)?;
                o.b(sa.is_done());
                if sa.is_done() {
                    o.i(sa.degree().into());
                    for x in sa.coefficients() {
                        o.f(*x);
                    }
                    for m in [sa.first_constr(), sa.last_constr()] {
                        for i in 0..total as usize {
                            for k in 0..=niv as usize {
                                o.f(*m.get(i, k).ok_or(AdvApproxError::OutOfRange)?);
                            }
                        }
                    }
                    let from = if ng % 2 == 1 { 0 } else { total as usize };
                    for x in sa.som_tab().iter().skip(from) {
                        o.f(*x);
                    }
                    for x in sa.dif_tab().iter().skip(from) {
                        o.f(*x);
                    }
                    for i in 1..=nss {
                        o.f(sa.max_error(i)?);
                        o.f(sa.average_error(i)?);
                    }
                    let d = sa.dump();
                    o.i(d.len() as i64);
                    o.u(fnv(&d));
                }
            }
            tf.log(&mut o);
        }
        "simple_bad" => {
            let (j, jc, c, wd, ng) = (rd.i(), rd.i() as usize, rd.i() as usize, rd.i(), rd.i());
            rd.finish();
            let mut tf = Tf::plain(world.def(j % 6));
            let jac = JacobiPolynomial::new(wd, SHAPES[jc])?;
            let sa = SimpleApprox::new(2, 1, SHAPES[c], wd, ng, &jac, &mut tf)?;
            o.b(true);
            o.i(sa.is_done().into());
        }
        "afunc_dicho_exact" | "afunc_dicho_trig" | "afunc_cut_exact" | "afunc_cut_trig" | "afunc_jump"
        | "afunc_err" | "afunc_bad" => {
            let x = read_af(
                &mut rd,
                match family {
                    "afunc_dicho_exact" | "afunc_dicho_trig" | "afunc_bad" => Some(false),
                    "afunc_cut_exact" | "afunc_cut_trig" => Some(true),
                    _ => None,
                },
            );
            rd.finish();
            let (a, tf) = build_af(world, &x)?;
            let full = family == "afunc_dicho_trig"
                || family == "afunc_cut_trig"
                || family == "afunc_err"
                || family == "afunc_bad";
            pr_af(&mut o, &a, &x, full)?;
            if family == "afunc_err" {
                o.i(a.nb_poles()?.into());
            }
            tf.log(&mut o);
        }
        "afunc_access" => {
            let x = read_af(&mut rd, Some(false));
            let (d, db, dc, idx) = (rd.i(), rd.i(), rd.i(), rd.i());
            rd.finish();
            let (a, tf) = build_af(world, &x)?;
            o.b(a.is_done());
            o.b(a.has_result());
            if a.has_result() {
                o.i(a.num_sub_spaces(d)?.into());
                let me = a.max_error_array(db)?;
                let ae = a.average_error_array(dc)?;
                pr_harr(&mut o, me);
                pr_harr(&mut o, ae);
                let count = |d: i32| match d {
                    1 => x.n1,
                    2 => x.n2,
                    _ => x.n3,
                };
                if me.is_some() && idx <= count(db) {
                    o.f(a.max_error(db, idx)?);
                }
                if ae.is_some() && idx <= count(dc) {
                    o.f(a.average_error(dc, idx)?);
                }
                o.i(a.nb_poles()?.into());
            }
            tf.log(&mut o);
        }
        "afunc_poles" => {
            let x = read_af(&mut rd, Some(false));
            let (d, idx, lower, upper) = (rd.i(), rd.i(), rd.i(), rd.i());
            rd.finish();
            let (a, tf) = build_af(world, &x)?;
            o.i(a.nb_poles()?.into());
            match d {
                1 => {
                    for v in a.poles_1d_at(idx, lower, upper)? {
                        o.f(v);
                    }
                }
                2 => {
                    for p in a.poles_2d_at(idx, lower, upper)? {
                        o.f(p.x());
                        o.f(p.y());
                    }
                }
                _ => {
                    for p in a.poles_at(idx, lower, upper)? {
                        o.f(p.x());
                        o.f(p.y());
                        o.f(p.z());
                    }
                }
            }
            tf.log(&mut o);
        }
        "afunc_dump" => {
            let x = read_af(&mut rd, Some(false));
            rd.finish();
            let (a, _tf) = build_af(world, &x)?;
            let d = if a.has_result() { a.dump()? } else { String::new() };
            o.i(d.len() as i64);
            o.u(fnv(&d));
        }
        "approximation_exact" | "approximation_trig" | "approximation_bad" | "approximation_err" => {
            let (f, total, nss) = (rd.i(), rd.i(), rd.i());
            let dims: Vec<i32> = (0..nss).map(|_| rd.i()).collect();
            let (first, last, ck) = (rd.f(), rd.f(), rd.i());
            let pts = if ck == 1 { rd.vec_f() } else { Vec::new() };
            let (co, nmax, maxseg) = (rd.i(), rd.i(), rd.i());
            let tols = rd.fs(nss);
            let (code, err_above) = (rd.i(), rd.f());
            rd.finish();
            let mut tf = Tf::new(world.def(f), err_above, 5, 1.0e300);
            let cut: Box<dyn Cutting> = if ck == 1 { Box::new(PrefCutting::new(&pts)) } else { Box::new(DichoCutting) };
            let r = ApproxAFunction::approximation(
                total,
                nss,
                &dims,
                first,
                last,
                &mut tf,
                cut.as_ref(),
                co,
                nmax,
                maxseg,
                &tols,
                code,
            )?;
            let ms = maxseg.max(1) as usize;
            o.i(r.error_code.into());
            o.i(r.num_curves.into());
            assert_eq!(r.num_coeff_per_curve.len(), ms);
            for v in &r.num_coeff_per_curve {
                o.i((*v).into());
            }
            for v in r.intervals.iter().chain(&r.error_max).chain(&r.average_error) {
                o.f(*v);
            }
            if family == "approximation_trig" {
                for v in &r.coefficients {
                    o.f(*v);
                }
            } else {
                o.i(r.coefficients.len() as i64);
                o.u(r.coefficients.iter().fold(H0, |h, x| mixh(h, x.to_bits())));
                let nsd = total.max(1) as i64;
                let used = (r.coefficients.len() as i64).min(i64::from(r.num_curves.max(0)) * nsd * i64::from(nmax));
                for v in r.coefficients.iter().take(used.min(24) as usize) {
                    o.f(*v);
                }
            }
            tf.log(&mut o);
        }
        _ => panic!("unknown family {family}"),
    }
    Ok(o.0)
}

/// Rebuilds the test functions from the golden records (the printed components are the data).
fn read_defs(world: &mut World) {
    for line in DATA.lines() {
        let (left, right) = line.split_once(" |").unwrap();
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        if family != "def_func_exact" && family != "def_func_trig" {
            continue;
        }
        let f: i32 = it.next().unwrap().parse().unwrap();
        let mut w = right.split_whitespace();
        let n: usize = w.next().unwrap().parse().unwrap();
        let mut comps = Vec::new();
        for _ in 0..n {
            let kind: i32 = w.next().unwrap().parse().unwrap();
            let mut p = [0.0; 5];
            for x in &mut p {
                *x = f64::from_bits(u64::from_str_radix(w.next().unwrap(), 16).unwrap());
            }
            comps.push(Comp { kind, p });
        }
        world.defs.insert(f, TfDef { comps });
    }
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
}

fn compare(t: &mut Tally, family: &str, line: &str, got: &AResult<Vec<Tok>>, want: &[&str]) {
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
fn advapprox_matches_occt() {
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut tally = Tally::default();
    let mut world = World::default();
    read_defs(&mut world);
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
        let got = run(&mut world, family, &words);
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
        "advapprox: {} families, {lines} lines ({throws} throws), {}/{} values bit-identical",
        counts.len(),
        tally.exact,
        tally.values
    );
}
