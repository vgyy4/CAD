//! Differential judge for OCCT's header-only `MathLin` package (OCCT 8.0.1): replays `tests/mathlin_pkg.txt`,
//! printed by `oracle/mathlin_pkg.cpp` against the real library. See `docs/api-mathlin_pkg.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]
use occt_port::mathlin::{IntegerVector, MathError, Matrix, Vector};
use occt_port::mathlin_pkg::{self as ml, LeastSquaresMethod, Status};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const DATA: &str = include_str!("mathlin_pkg.txt");
const GOLDEN_LINES: usize = 448;
const MIN_DISTINCT: usize = 16;
const MAX_ULPS: u64 = 8;
const FAMILIES: &[&str] = &[
    "condition_number",
    "crout",
    "determinant",
    "eigen_tridiagonal",
    "eigen_values",
    "get_eigen_vector",
    "invert",
    "invert_crout",
    "jacobi",
    "least_squares",
    "lu",
    "matrix_inv_sqrt",
    "matrix_power",
    "matrix_sqrt",
    "numerical_rank",
    "optimal_regularization",
    "pseudo_inverse",
    "qr",
    "regularized_least_squares",
    "solve",
    "solve_crout",
    "solve_multiple",
    "solve_qr",
    "solve_qr_multiple",
    "solve_svd",
    "spectral_decomposition",
    "svd",
    "weighted_least_squares",
];
/// Families that call `pow`/`log10`: compared within `MAX_ULPS` of the largest value on the line (all others bit for bit).
const LIBM: &[&str] = &["matrix_power", "matrix_sqrt", "matrix_inv_sqrt", "optimal_regularization"];

#[derive(Debug, Clone, Copy)]
enum Tok {
    I(i64),
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
    fn mat(&mut self) -> Matrix {
        let (lr, ur, lc, uc) = (self.i(), self.i(), self.i(), self.i());
        let (rows, cols) = (ur - lr + 1, uc - lc + 1);
        let vals: Vec<f64> = (0..rows * cols).map(|_| self.f()).collect();
        Matrix::from_slice(lr, lc, rows, cols, &vals).unwrap()
    }
    fn vec(&mut self) -> Vector {
        let (l, u) = (self.i(), self.i());
        let vals: Vec<f64> = (0..u - l + 1).map(|_| self.f()).collect();
        Vector::from_slice(l, &vals).unwrap()
    }
    fn finish(&self) {
        assert_eq!(self.p, self.w.len(), "unread input tokens");
    }
}

#[derive(Default)]
struct Out(Vec<Tok>);
impl Out {
    fn i(&mut self, v: i64) {
        self.0.push(Tok::I(v));
    }
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn st(&mut self, s: Status) {
        self.i(s as i64);
    }
    fn mat(&mut self, m: &Matrix) {
        self.i(m.lower_row().into());
        self.i(m.upper_row().into());
        self.i(m.lower_col().into());
        self.i(m.upper_col().into());
        for r in m.lower_row()..=m.upper_row() {
            for c in m.lower_col()..=m.upper_col() {
                self.f(m.value(r, c).unwrap());
            }
        }
    }
    fn vec(&mut self, v: &Vector) {
        self.i(v.lower().into());
        self.i(v.upper().into());
        for k in v.lower()..=v.upper() {
            self.f(v.value(k).unwrap());
        }
    }
    fn ivec(&mut self, v: &IntegerVector) {
        self.i(v.lower().into());
        self.i(v.upper().into());
        for k in v.lower()..=v.upper() {
            self.i(v.value(k).unwrap().into());
        }
    }
    fn om(&mut self, m: &Option<Matrix>) {
        self.i(m.is_some().into());
        if let Some(m) = m {
            self.mat(m);
        }
    }
    fn ov(&mut self, v: &Option<Vector>) {
        self.i(v.is_some().into());
        if let Some(v) = v {
            self.vec(v);
        }
    }
    fn od(&mut self, d: &Option<f64>) {
        self.i(d.is_some().into());
        if let Some(d) = d {
            self.f(*d);
        }
    }
    fn linear(&mut self, r: &ml::LinearResult) {
        self.st(r.status);
        self.ov(&r.solution);
        self.od(&r.determinant);
    }
    fn multiple(&mut self, r: &ml::LinearMultipleResult) {
        self.st(r.status);
        self.om(&r.solutions);
        self.od(&r.determinant);
    }
    fn inverse(&mut self, r: &ml::InverseResult) {
        self.st(r.status);
        self.om(&r.inverse);
        self.od(&r.determinant);
    }
    fn ls(&mut self, r: &ml::LeastSquaresResult) {
        self.st(r.status);
        self.i(r.rank.into());
        self.ov(&r.solution);
        self.od(&r.residual);
        self.od(&r.residual_sq);
    }
    fn eigen(&mut self, r: &ml::EigenResult) {
        self.st(r.status);
        self.i(r.nb_iterations as i64);
        self.ov(&r.eigen_values);
        self.om(&r.eigen_vectors);
    }
}

fn method(k: i32) -> LeastSquaresMethod {
    match k {
        0 => LeastSquaresMethod::NormalEquations,
        1 => LeastSquaresMethod::Qr,
        _ => LeastSquaresMethod::Svd,
    }
}

/// Replays one record; `Err` stands for a C++ exception (`throws`).
fn run(family: &str, words: &[&str]) -> Result<Vec<Tok>, MathError> {
    let mut rd = Rd::new(words);
    let mut o = Out::default();
    match family {
        "lu" => {
            let (a, mp) = (rd.mat(), rd.f());
            rd.finish();
            let r = ml::lu(&a, mp)?;
            o.st(r.status);
            o.i(r.sign.into());
            o.om(&r.lu);
            o.i(r.pivot.is_some().into());
            if let (Some(p), Status::Ok) = (&r.pivot, r.status) {
                o.ivec(p);
            }
            o.od(&r.determinant);
        }
        "solve" | "solve_crout" | "solve_qr" | "solve_svd" => {
            let (a, b, x) = (rd.mat(), rd.vec(), rd.f());
            rd.finish();
            let r = match family {
                "solve" => ml::solve(&a, &b, x)?,
                "solve_crout" => ml::solve_crout(&a, &b, x)?,
                "solve_qr" => ml::solve_qr(&a, &b, x)?,
                _ => ml::solve_svd(&a, &b, x)?,
            };
            if family == "solve_qr" || family == "solve_svd" {
                o.st(r.status);
                o.ov(&r.solution);
            } else {
                o.linear(&r);
            }
        }
        "solve_multiple" | "solve_qr_multiple" => {
            let (a, b, x) = (rd.mat(), rd.mat(), rd.f());
            rd.finish();
            let r = if family == "solve_multiple" {
                ml::solve_multiple(&a, &b, x)?
            } else {
                ml::solve_qr_multiple(&a, &b, x)?
            };
            if family == "solve_qr_multiple" {
                o.st(r.status);
                o.om(&r.solutions);
            } else {
                o.multiple(&r);
            }
        }
        "determinant" => {
            let (a, mp) = (rd.mat(), rd.f());
            rd.finish();
            o.linear(&ml::determinant(&a, mp)?);
        }
        "invert" | "invert_crout" | "pseudo_inverse" => {
            let (a, x) = (rd.mat(), rd.f());
            rd.finish();
            let r = match family {
                "invert" => ml::invert(&a, x)?,
                "invert_crout" => ml::invert_crout(&a, x)?,
                _ => ml::pseudo_inverse(&a, x)?,
            };
            if family == "pseudo_inverse" {
                o.st(r.status);
                o.om(&r.inverse);
            } else {
                o.inverse(&r);
            }
        }
        "crout" => {
            let (a, mp) = (rd.mat(), rd.f());
            rd.finish();
            let r = ml::crout(&a, mp)?;
            o.st(r.status);
            o.om(&r.l);
            o.ov(&r.d);
            o.om(&r.inverse);
            o.od(&r.determinant);
        }
        "svd" => {
            let (a, tol) = (rd.mat(), rd.f());
            rd.finish();
            let r = ml::svd(&a, tol)?;
            o.st(r.status);
            o.i(r.rank.into());
            o.om(&r.u);
            o.ov(&r.singular_values);
            o.om(&r.v);
        }
        "condition_number" => {
            let a = rd.mat();
            rd.finish();
            o.f(ml::condition_number(&a)?);
        }
        "numerical_rank" => {
            let (a, tol) = (rd.mat(), rd.f());
            rd.finish();
            o.i(ml::numerical_rank(&a, tol)?.into());
        }
        "qr" => {
            let (a, tol) = (rd.mat(), rd.f());
            rd.finish();
            let r = ml::qr(&a, tol)?;
            o.st(r.status);
            o.i(r.rank.into());
            o.om(&r.q);
            o.om(&r.r);
        }
        "jacobi" | "eigen_values" => {
            let (a, sort) = (rd.mat(), rd.i() != 0);
            rd.finish();
            o.eigen(&if family == "jacobi" { ml::jacobi(&a, sort)? } else { ml::eigen_values(&a, sort)? });
        }
        "spectral_decomposition" => {
            let a = rd.mat();
            rd.finish();
            o.eigen(&ml::spectral_decomposition(&a)?);
        }
        "matrix_power" => {
            let (a, p) = (rd.mat(), rd.f());
            rd.finish();
            o.om(&ml::matrix_power(&a, p)?);
        }
        "matrix_sqrt" | "matrix_inv_sqrt" => {
            let a = rd.mat();
            rd.finish();
            o.om(&if family == "matrix_sqrt" { ml::matrix_sqrt(&a)? } else { ml::matrix_inv_sqrt(&a)? });
        }
        "least_squares" => {
            let (a, b, k, tol) = (rd.mat(), rd.vec(), rd.i(), rd.f());
            rd.finish();
            o.ls(&ml::least_squares(&a, &b, method(k), tol)?);
        }
        "weighted_least_squares" => {
            let (a, b, w, k, tol) = (rd.mat(), rd.vec(), rd.vec(), rd.i(), rd.f());
            rd.finish();
            o.ls(&ml::weighted_least_squares(&a, &b, &w, method(k), tol)?);
        }
        "regularized_least_squares" => {
            let (a, b, lam, tol) = (rd.mat(), rd.vec(), rd.f(), rd.f());
            rd.finish();
            o.ls(&ml::regularized_least_squares(&a, &b, lam, tol)?);
        }
        "optimal_regularization" => {
            let (a, b, lo, hi, np) = (rd.mat(), rd.vec(), rd.f(), rd.f(), rd.i());
            rd.finish();
            o.f(ml::optimal_regularization(&a, &b, lo, hi, np)?);
        }
        "eigen_tridiagonal" => {
            let (d, s, mi) = (rd.vec(), rd.vec(), rd.i());
            rd.finish();
            let r = ml::eigen_tridiagonal(&d, &s, mi)?;
            o.st(r.status);
            o.i(r.dimension.into());
            o.ov(&r.eigen_values);
            o.om(&r.eigen_vectors);
        }
        "get_eigen_vector" => {
            let (d, s, mi, idx) = (rd.vec(), rd.vec(), rd.i(), rd.i());
            rd.finish();
            let r = ml::eigen_tridiagonal(&d, &s, mi)?;
            o.st(r.status);
            let v = ml::get_eigen_vector(&r, idx)?;
            o.i(v.lower().into());
            o.i(v.upper().into());
            if r.eigen_vectors.is_some() {
                for k in v.lower()..=v.upper() {
                    o.f(v.value(k).unwrap());
                }
            }
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
                scale = scale.max(f64::from_bits(u64::from_str_radix(w, 16).unwrap()).abs());
            }
        }
    }
    for (k, (g, w)) in got.iter().zip(want).enumerate() {
        match g {
            Tok::I(v) => assert_eq!(*v, w.parse::<i64>().unwrap(), "{family} output {k}\n{line}"),
            Tok::F(v) => {
                let e = f64::from_bits(u64::from_str_radix(w, 16).unwrap());
                t.values += 1;
                t.exact += usize::from(v.to_bits() == e.to_bits());
                if libm {
                    let tol = if scale.is_infinite() { 0.0 } else { MAX_ULPS as f64 * f64::EPSILON * scale };
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
fn mathlin_pkg_matches_occt() {
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut distinct = BTreeMap::<String, BTreeSet<String>>::new();
    let mut per_family = BTreeMap::<String, Tally>::new();
    let mut lines = 0;
    let mut throws = 0;
    for (n, line) in DATA.lines().enumerate() {
        let (left, right) = line.split_once(" | ").unwrap_or_else(|| panic!("line {} malformed", n + 1));
        let mut it = left.split_whitespace();
        let family = it.next().unwrap();
        let words: Vec<&str> = it.collect();
        assert!(seen.insert(line.to_owned()), "duplicate line {}", n + 1);
        *counts.entry(family.to_owned()).or_default() += 1;
        distinct.entry(family.to_owned()).or_default().insert(left.to_owned());
        let want: Vec<&str> = right.split_whitespace().collect();
        let got = run(family, &words);
        throws += usize::from(want == ["throws"]);
        compare(per_family.entry(family.to_owned()).or_default(), family, line, &got, &want);
        lines += 1;
    }
    assert_eq!(lines, GOLDEN_LINES);
    let mut want_fams = FAMILIES.to_vec();
    want_fams.sort_unstable();
    assert_eq!(counts.keys().map(String::as_str).collect::<Vec<_>>(), want_fams);
    for name in FAMILIES {
        assert!(distinct[*name].len() >= MIN_DISTINCT, "{name} needs {MIN_DISTINCT} distinct inputs");
    }
    assert!(throws >= 5, "the oracle must exercise `throws`");
    let (values, exact) = per_family.values().fold((0, 0), |a, t| (a.0 + t.values, a.1 + t.exact));
    println!(
        "mathlin_pkg: {} families, {lines} lines ({throws} throws), {exact}/{values} values bit-identical",
        counts.len()
    );
}
