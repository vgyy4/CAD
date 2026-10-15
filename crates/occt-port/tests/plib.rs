//! Numerical reference test for OCCT 8.0.1's `PLib` package (the static functions of `PLib`, `PLib_JacobiPolynomial`
//! and `PLib_HermitJacobi`).
//!
//! `plib.txt` is printed by `oracle/plib.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns and ints (and bools) are decimal.
//! Every input of a call is on the left of ` | `; this test reads them back with the `Rd` reader methods of the same
//! name as the oracle's schema words (`f i shape fs p3s p2s arr2 p3a2 ofs oarr2`) and replays the call on the Rust API of
//! docs/api-plib.md. The right side is either the expected values, `throws` (OCCT raised) or `unchecked` (a check that
//! OCCT 8.0.1 as built has compiled out would have failed and the numbers are unspecified); on both the Rust API must
//! return an error.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::gp::{Pnt, Pnt2d};
use occt_port::mathlin::{MathError, Matrix};
use occt_port::plib::{self, Array2, GeomAbsShape, HermitJacobi, JacobiPolynomial, PLibError};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for results that are not computed with `+ - * /` and `sqrt` only, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1). PLib needs none of that: the list of
/// `EXACT` families below is every family.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once. There are none here.
const INPUT_LESS: &[&str] = &[];

/// Families whose results use only + - * / and sqrt: compared bit for bit (signed zeros included).
const EXACT: &[&str] = &[
    "bin",
    "coefficients_poles_1d",
    "coefficients_poles_2d",
    "coefficients_poles_3d",
    "coefficients_poles_dim",
    "coefficients_poles_surface",
    "constraint_conversions",
    "eval_cubic_hermite",
    "eval_lagrange",
    "eval_length",
    "eval_length_tol",
    "eval_poly2var",
    "eval_polynomial",
    "get_poles_2d",
    "get_poles_2d_weighted",
    "get_poles_3d",
    "get_poles_3d_weighted",
    "hermit_average_error",
    "hermit_d0",
    "hermit_d1",
    "hermit_d2",
    "hermit_d3",
    "hermit_max_error",
    "hermit_new",
    "hermit_reduce_degree",
    "hermit_to_coefficients",
    "hermite_coefficients",
    "hermite_interpolate",
    "jacobi_average_error",
    "jacobi_d0",
    "jacobi_d1",
    "jacobi_d2",
    "jacobi_d3",
    "jacobi_max_error",
    "jacobi_max_value",
    "jacobi_new",
    "jacobi_parameters",
    "jacobi_points",
    "jacobi_reduce_degree",
    "jacobi_to_coefficients",
    "jacobi_weights",
    "no_derivative_eval_polynomial",
    "rational_derivative",
    "rational_derivatives",
    "set_poles_2d",
    "set_poles_2d_weighted",
    "set_poles_3d",
    "set_poles_3d_weighted",
    "trimming_1d",
    "trimming_2d",
    "trimming_3d",
    "trimming_dim",
    "u_trimming",
    "v_trimming",
];

/// Every family the oracle must print (a family that disappears from plib.txt fails).
const FAMILIES: &[&str] = &[
    "bin",
    "coefficients_poles_1d",
    "coefficients_poles_2d",
    "coefficients_poles_3d",
    "coefficients_poles_dim",
    "coefficients_poles_surface",
    "constraint_conversions",
    "eval_cubic_hermite",
    "eval_lagrange",
    "eval_length",
    "eval_length_tol",
    "eval_poly2var",
    "eval_polynomial",
    "get_poles_2d",
    "get_poles_2d_weighted",
    "get_poles_3d",
    "get_poles_3d_weighted",
    "hermit_average_error",
    "hermit_d0",
    "hermit_d1",
    "hermit_d2",
    "hermit_d3",
    "hermit_max_error",
    "hermit_new",
    "hermit_reduce_degree",
    "hermit_to_coefficients",
    "hermite_coefficients",
    "hermite_interpolate",
    "jacobi_average_error",
    "jacobi_d0",
    "jacobi_d1",
    "jacobi_d2",
    "jacobi_d3",
    "jacobi_max_error",
    "jacobi_max_value",
    "jacobi_new",
    "jacobi_parameters",
    "jacobi_points",
    "jacobi_reduce_degree",
    "jacobi_to_coefficients",
    "jacobi_weights",
    "no_derivative_eval_polynomial",
    "rational_derivative",
    "rational_derivatives",
    "set_poles_2d",
    "set_poles_2d_weighted",
    "set_poles_3d",
    "set_poles_3d_weighted",
    "trimming_1d",
    "trimming_2d",
    "trimming_3d",
    "trimming_dim",
    "u_trimming",
    "v_trimming",
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 3140;

// ---------------------------------------------------------------------------------
// Results of one replayed call

/// The Rust API returned an error (or the call is known to be invalid).
#[derive(Debug)]
struct Thrown;

impl From<PLibError> for Thrown {
    fn from(_: PLibError) -> Self {
        Thrown
    }
}

impl From<MathError> for Thrown {
    fn from(_: MathError) -> Self {
        Thrown
    }
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    F(f64),
    B(bool),
    I(i32),
}

/// Collects the values produced by the Rust side, in the order the oracle printed them.
#[derive(Default)]
struct Got(Vec<Tok>);

impl Got {
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn fs(&mut self, vs: &[f64]) {
        for v in vs {
            self.f(*v);
        }
    }
    fn b(&mut self, v: bool) {
        self.0.push(Tok::B(v));
    }
    fn i(&mut self, v: i32) {
        self.0.push(Tok::I(v));
    }
    fn p3s(&mut self, vs: &[Pnt]) {
        for p in vs {
            self.fs(&[p.x(), p.y(), p.z()]);
        }
    }
    fn p2s(&mut self, vs: &[Pnt2d]) {
        for p in vs {
            self.fs(&[p.x(), p.y()]);
        }
    }
}

// ---------------------------------------------------------------------------------
// Reading the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

struct Rd<'a> {
    words: &'a [&'a str],
    at: usize,
}

impl<'a> Rd<'a> {
    fn word(&mut self) -> &'a str {
        let w = *self.words.get(self.at).unwrap_or_else(|| panic!("input word {} missing", self.at));
        self.at += 1;
        w
    }
    /// A scalar.
    fn f(&mut self) -> f64 {
        bits(self.word())
    }
    /// An integer.
    fn i(&mut self) -> i32 {
        self.word().parse().expect("integer input")
    }
    /// A `GeomAbs_Shape` ordinal 0..=6.
    fn shape(&mut self) -> GeomAbsShape {
        match self.i() {
            0 => GeomAbsShape::C0,
            1 => GeomAbsShape::G1,
            2 => GeomAbsShape::C1,
            3 => GeomAbsShape::G2,
            4 => GeomAbsShape::C2,
            5 => GeomAbsShape::C3,
            6 => GeomAbsShape::Cn,
            other => panic!("bad shape ordinal {other}"),
        }
    }
    fn count(&mut self) -> usize {
        self.word().parse().expect("count input")
    }
    /// A count followed by that many scalars.
    fn fs(&mut self) -> Vec<f64> {
        let n = self.count();
        (0..n).map(|_| self.f()).collect()
    }
    /// A count followed by that many 3D points.
    fn p3s(&mut self) -> Vec<Pnt> {
        let n = self.count();
        (0..n)
            .map(|_| {
                let (x, y, z) = (self.f(), self.f(), self.f());
                Pnt::new(x, y, z)
            })
            .collect()
    }
    /// A count followed by that many 2D points.
    fn p2s(&mut self) -> Vec<Pnt2d> {
        let n = self.count();
        (0..n)
            .map(|_| {
                let (x, y) = (self.f(), self.f());
                Pnt2d::new(x, y)
            })
            .collect()
    }
    /// Rows, columns, then the scalars row by row.
    fn arr2(&mut self) -> Array2<f64> {
        let (rows, cols) = (self.count(), self.count());
        let data: Vec<f64> = (0..rows * cols).map(|_| self.f()).collect();
        Array2::new(rows, cols, data).expect("array shape")
    }
    /// Rows, columns, then the 3D points row by row.
    fn p3a2(&mut self) -> Array2<Pnt> {
        let (rows, cols) = (self.count(), self.count());
        let data: Vec<Pnt> = (0..rows * cols)
            .map(|_| {
                let (x, y, z) = (self.f(), self.f(), self.f());
                Pnt::new(x, y, z)
            })
            .collect();
        Array2::new(rows, cols, data).expect("array shape")
    }
    /// A flag, and when it is 1 a scalar array: the optional weights (`NULL` = `None`).
    fn ofs(&mut self) -> Option<Vec<f64>> {
        match self.i() {
            0 => None,
            1 => Some(self.fs()),
            other => panic!("bad option flag {other}"),
        }
    }
    /// A flag, and when it is 1 a 2D scalar array.
    fn oarr2(&mut self) -> Option<Array2<f64>> {
        match self.i() {
            0 => None,
            1 => Some(self.arr2()),
            other => panic!("bad option flag {other}"),
        }
    }
    fn finish(&self) {
        assert_eq!(self.at, self.words.len(), "unread inputs");
    }
}

// ---------------------------------------------------------------------------------
// Replaying one family

fn run(name: &str, r: &mut Rd, g: &mut Got) -> Result<(), Thrown> {
    match name {
        // ---- evaluation ----
        "eval_polynomial" => {
            let (u, order, degree, dim) = (r.f(), r.i(), r.i(), r.i());
            let coeffs = r.fs();
            g.fs(&plib::eval_polynomial(u, order, degree, dim, &coeffs)?);
        }
        "no_derivative_eval_polynomial" => {
            let (u, degree, dim, degree_dim) = (r.f(), r.i(), r.i(), r.i());
            let coeffs = r.fs();
            g.fs(&plib::no_derivative_eval_polynomial(u, degree, dim, degree_dim, &coeffs)?);
        }
        "eval_poly2var" => {
            let (u, v, u_order, v_order) = (r.f(), r.f(), r.i(), r.i());
            let (u_degree, v_degree, dim) = (r.i(), r.i(), r.i());
            let coeffs = r.fs();
            g.fs(&plib::eval_poly2var(u, v, u_order, v_order, u_degree, v_degree, dim, &coeffs)?);
        }
        "eval_lagrange" => {
            let (u, order, degree, dim) = (r.f(), r.i(), r.i(), r.i());
            let (values, parameters) = (r.fs(), r.fs());
            let (code, res) = plib::eval_lagrange(u, order, degree, dim, &values, &parameters)?;
            g.i(code);
            // with a failure code the results are unspecified (not written by OCCT)
            if code == 0 {
                g.fs(&res);
            }
        }
        "eval_cubic_hermite" => {
            let (u, order, dim) = (r.f(), r.i(), r.i());
            let (values, derivatives, parameters) = (r.fs(), r.fs(), r.fs());
            let (code, res) = plib::eval_cubic_hermite(u, order, dim, &values, &derivatives, &parameters)?;
            g.i(code);
            g.fs(&res);
        }
        // ---- Hermite polynomials ----
        "hermite_coefficients" => {
            let (first, last, first_order, last_order) = (r.f(), r.f(), r.i(), r.i());
            // the oracle passes an n x n matrix (1 x 1 when n < 1) and prints it after the call
            let n = (first_order + last_order + 2).max(1);
            let mut m = Matrix::filled(1, n, 1, n, 0.0)?;
            let ok = plib::hermite_coefficients(first, last, first_order, last_order, &mut m)?;
            g.b(ok);
            for row in 1..=n {
                for col in 1..=n {
                    g.f(m.value(row, col)?);
                }
            }
        }
        "hermite_interpolate" => {
            let (dim, first, last, first_order, last_order) = (r.i(), r.f(), r.f(), r.i(), r.i());
            let (first_constr, last_constr) = (r.arr2(), r.arr2());
            let coefs =
                plib::hermite_interpolate(dim, first, last, first_order, last_order, &first_constr, &last_constr)?;
            g.i(i32::try_from(coefs.len()).expect("length"));
            g.fs(&coefs);
        }
        // ---- poles <-> flat arrays ----
        "set_poles_3d" => {
            let poles = r.p3s();
            g.fs(&plib::set_poles_3d(&poles)?);
        }
        "set_poles_3d_weighted" => {
            let (poles, weights) = (r.p3s(), r.fs());
            g.fs(&plib::set_poles_3d_weighted(&poles, &weights)?);
        }
        "set_poles_2d" => {
            let poles = r.p2s();
            g.fs(&plib::set_poles_2d(&poles)?);
        }
        "set_poles_2d_weighted" => {
            let (poles, weights) = (r.p2s(), r.fs());
            g.fs(&plib::set_poles_2d_weighted(&poles, &weights)?);
        }
        "get_poles_3d" => {
            let (n, fp) = (r.i(), r.fs());
            g.p3s(&plib::get_poles_3d(&fp, n)?);
        }
        "get_poles_3d_weighted" => {
            let (n, fp) = (r.i(), r.fs());
            let (poles, weights) = plib::get_poles_3d_weighted(&fp, n)?;
            g.p3s(&poles);
            g.fs(&weights);
        }
        "get_poles_2d" => {
            let (n, fp) = (r.i(), r.fs());
            g.p2s(&plib::get_poles_2d(&fp, n)?);
        }
        "get_poles_2d_weighted" => {
            let (n, fp) = (r.i(), r.fs());
            let (poles, weights) = plib::get_poles_2d_weighted(&fp, n)?;
            g.p2s(&poles);
            g.fs(&weights);
        }
        "bin" => {
            let (n, p) = (r.i(), r.i());
            g.f(plib::bin(n, p)?);
        }
        // ---- power basis -> Bezier poles ----
        "coefficients_poles_3d" => {
            let (coefs, weights) = (r.p3s(), r.ofs());
            let (poles, w) = plib::coefficients_poles_3d(&coefs, weights.as_deref())?;
            g.p3s(&poles);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "coefficients_poles_2d" => {
            let (coefs, weights) = (r.p2s(), r.ofs());
            let (poles, w) = plib::coefficients_poles_2d(&coefs, weights.as_deref())?;
            g.p2s(&poles);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "coefficients_poles_1d" => {
            let (coefs, weights) = (r.fs(), r.ofs());
            let (poles, w) = plib::coefficients_poles_1d(&coefs, weights.as_deref())?;
            g.fs(&poles);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "coefficients_poles_dim" => {
            let dim = r.i();
            let (coefs, weights) = (r.fs(), r.ofs());
            let (poles, w) = plib::coefficients_poles_dim(dim, &coefs, weights.as_deref())?;
            g.fs(&poles);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "coefficients_poles_surface" => {
            let (coefs, weights) = (r.p3a2(), r.oarr2());
            let (poles, w) = plib::coefficients_poles_surface(&coefs, weights.as_ref())?;
            g.p3s(poles.as_slice());
            if let Some(w) = w {
                g.fs(w.as_slice());
            }
        }
        // ---- trimming ----
        "trimming_3d" => {
            let (u1, u2, coefs, weights) = (r.f(), r.f(), r.p3s(), r.ofs());
            let (c, w) = plib::trimming_3d(u1, u2, &coefs, weights.as_deref())?;
            g.p3s(&c);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "trimming_2d" => {
            let (u1, u2, coefs, weights) = (r.f(), r.f(), r.p2s(), r.ofs());
            let (c, w) = plib::trimming_2d(u1, u2, &coefs, weights.as_deref())?;
            g.p2s(&c);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "trimming_1d" => {
            let (u1, u2, coefs, weights) = (r.f(), r.f(), r.fs(), r.ofs());
            let (c, w) = plib::trimming_1d(u1, u2, &coefs, weights.as_deref())?;
            g.fs(&c);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "trimming_dim" => {
            let (u1, u2, dim) = (r.f(), r.f(), r.i());
            let (coefs, weights) = (r.fs(), r.ofs());
            let (c, w) = plib::trimming_dim(u1, u2, dim, &coefs, weights.as_deref())?;
            g.fs(&c);
            if let Some(w) = w {
                g.fs(&w);
            }
        }
        "u_trimming" => {
            let (u1, u2, coefs, weights) = (r.f(), r.f(), r.p3a2(), r.oarr2());
            let (c, w) = plib::u_trimming(u1, u2, &coefs, weights.as_ref())?;
            g.p3s(c.as_slice());
            if let Some(w) = w {
                g.fs(w.as_slice());
            }
        }
        "v_trimming" => {
            let (v1, v2, coefs, weights) = (r.f(), r.f(), r.p3a2(), r.oarr2());
            let (c, w) = plib::v_trimming(v1, v2, &coefs, weights.as_ref())?;
            g.p3s(c.as_slice());
            if let Some(w) = w {
                g.fs(w.as_slice());
            }
        }
        // ---- rational derivatives ----
        "rational_derivative" => {
            let (degree, request, dim, all) = (r.i(), r.i(), r.i(), r.i());
            let ders = r.fs();
            g.fs(&plib::rational_derivative(degree, request, dim, &ders, all != 0)?);
        }
        "rational_derivatives" => {
            let (request, dim) = (r.i(), r.i());
            let (poles_derivatives, weights_derivatives) = (r.fs(), r.fs());
            g.fs(&plib::rational_derivatives(request, dim, &poles_derivatives, &weights_derivatives)?);
        }
        // ---- constraint orders, Jacobi parameters, length ----
        "jacobi_parameters" => {
            let (shape, max_degree, code) = (r.shape(), r.i(), r.i());
            let (nb_gauss_points, work_degree) = plib::jacobi_parameters(shape, max_degree, code)?;
            g.i(nb_gauss_points);
            g.i(work_degree);
        }
        "constraint_conversions" => {
            let (shape, niv) = (r.shape(), r.i());
            // both conversions are called; either one failing makes the record `throws`
            let from_shape = plib::niv_constr(shape)?;
            let from_niv = plib::constraint_order(niv)?;
            g.i(from_shape);
            g.i(from_niv as i32);
        }
        "eval_length" => {
            let (degree, dim) = (r.i(), r.i());
            let (coeffs, u1, u2) = (r.fs(), r.f(), r.f());
            g.f(plib::eval_length(degree, dim, &coeffs, u1, u2)?);
        }
        "eval_length_tol" => {
            let (degree, dim) = (r.i(), r.i());
            let (coeffs, u1, u2, tol) = (r.fs(), r.f(), r.f(), r.f());
            let (length, error) = plib::eval_length_tol(degree, dim, &coeffs, u1, u2, tol)?;
            g.f(length);
            g.f(error);
        }

        // ---- JacobiPolynomial ----
        "jacobi_new" => {
            let (wd, shape) = (r.i(), r.shape());
            let j = JacobiPolynomial::new(wd, shape)?;
            g.i(j.work_degree());
            g.i(j.niv_constr());
        }
        "jacobi_max_error" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (c, new_degree) = (r.fs(), r.i());
            g.f(JacobiPolynomial::new(wd, shape)?.max_error(dim, &c, new_degree)?);
        }
        "jacobi_average_error" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (c, new_degree) = (r.fs(), r.i());
            g.f(JacobiPolynomial::new(wd, shape)?.average_error(dim, &c, new_degree)?);
        }
        "jacobi_reduce_degree" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (max_degree, tol, c) = (r.i(), r.f(), r.fs());
            let (new_degree, max_error) = JacobiPolynomial::new(wd, shape)?.reduce_degree(dim, max_degree, tol, &c)?;
            g.i(new_degree);
            g.f(max_error);
        }
        "jacobi_to_coefficients" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (degree, c) = (r.i(), r.fs());
            g.fs(&JacobiPolynomial::new(wd, shape)?.to_coefficients(dim, degree, &c)?);
        }
        "jacobi_d0" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            g.fs(&JacobiPolynomial::new(wd, shape)?.d0(u));
        }
        "jacobi_d1" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            let (v, d1) = JacobiPolynomial::new(wd, shape)?.d1(u);
            g.fs(&v);
            g.fs(&d1);
        }
        "jacobi_d2" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            let (v, d1, d2) = JacobiPolynomial::new(wd, shape)?.d2(u);
            g.fs(&v);
            g.fs(&d1);
            g.fs(&d2);
        }
        "jacobi_d3" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            let (v, d1, d2, d3) = JacobiPolynomial::new(wd, shape)?.d3(u);
            g.fs(&v);
            g.fs(&d1);
            g.fs(&d2);
            g.fs(&d3);
        }
        "jacobi_points" => {
            let (wd, shape, n) = (r.i(), r.shape(), r.i());
            g.fs(&JacobiPolynomial::new(wd, shape)?.points(n)?);
        }
        "jacobi_weights" => {
            let (wd, shape, n) = (r.i(), r.shape(), r.i());
            let w = JacobiPolynomial::new(wd, shape)?.weights(n)?;
            g.fs(w.as_slice());
        }
        "jacobi_max_value" => {
            let (wd, shape, len) = (r.i(), r.shape(), r.i());
            g.fs(&JacobiPolynomial::new(wd, shape)?.max_value(len)?);
        }

        // ---- HermitJacobi ----
        "hermit_new" => {
            let (wd, shape) = (r.i(), r.shape());
            let j = HermitJacobi::new(wd, shape)?;
            g.i(j.work_degree());
            g.i(j.niv_constr());
        }
        "hermit_max_error" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (c, new_degree) = (r.fs(), r.i());
            g.f(HermitJacobi::new(wd, shape)?.max_error(dim, &c, new_degree)?);
        }
        "hermit_average_error" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (c, new_degree) = (r.fs(), r.i());
            g.f(HermitJacobi::new(wd, shape)?.average_error(dim, &c, new_degree)?);
        }
        "hermit_reduce_degree" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (max_degree, tol, c) = (r.i(), r.f(), r.fs());
            let (new_degree, max_error) = HermitJacobi::new(wd, shape)?.reduce_degree(dim, max_degree, tol, &c)?;
            g.i(new_degree);
            g.f(max_error);
        }
        "hermit_to_coefficients" => {
            let (wd, shape, dim) = (r.i(), r.shape(), r.i());
            let (degree, c) = (r.i(), r.fs());
            g.fs(&HermitJacobi::new(wd, shape)?.to_coefficients(dim, degree, &c)?);
        }
        "hermit_d0" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            g.fs(&HermitJacobi::new(wd, shape)?.d0(u));
        }
        "hermit_d1" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            let (v, d1) = HermitJacobi::new(wd, shape)?.d1(u);
            g.fs(&v);
            g.fs(&d1);
        }
        "hermit_d2" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            let (v, d1, d2) = HermitJacobi::new(wd, shape)?.d2(u);
            g.fs(&v);
            g.fs(&d1);
            g.fs(&d2);
        }
        "hermit_d3" => {
            let (wd, shape, u) = (r.i(), r.shape(), r.f());
            let (v, d1, d2, d3) = HermitJacobi::new(wd, shape)?.d3(u);
            g.fs(&v);
            g.fs(&d1);
            g.fs(&d2);
            g.fs(&d3);
        }
        _ => panic!("unknown oracle family {name}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------
// Comparison

fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let n = x.to_bits() as i64;
        if n < 0 { i64::MIN - n } else { n }
    };
    key(a).abs_diff(key(b))
}

#[derive(Default)]
struct Tally {
    values: usize,
    exact: usize,
    worst: u64,
}

/// Compares the expected words of a line with the replayed values. Floats are compared bit
/// for bit for `EXACT` families and within `MAX_ULPS` of the line's largest magnitude
/// otherwise (zero tolerance when that magnitude is infinite); bools and ints always exactly.
fn compare(tally: &mut Tally, line: &str, name: &str, want: &[&str], got: &[Tok]) {
    assert_eq!(want.len(), got.len(), "value count differs\n{line}");
    let exact = EXACT.contains(&name);
    let mut scale = 1.0_f64;
    for (w, t) in want.iter().zip(got) {
        if let Tok::F(_) = t {
            scale = scale.max(bits(w).abs());
        }
    }
    let tol = if scale.is_finite() { MAX_ULPS * f64::EPSILON * scale } else { 0.0 };
    for (index, (w, t)) in want.iter().zip(got).enumerate() {
        match *t {
            Tok::F(g) => {
                let w = bits(w);
                let d = ulps(w, g);
                let ok =
                    if exact { w.to_bits() == g.to_bits() } else { (w - g).abs() <= tol || w.to_bits() == g.to_bits() };
                assert!(ok, "value {index}: want {w:e}, got {g:e} ({d} ulps)\n{line}");
                tally.values += 1;
                tally.exact += usize::from(d == 0);
                tally.worst = tally.worst.max(d);
            }
            Tok::B(g) => assert_eq!(*w, if g { "1" } else { "0" }, "value {index} (bool)\n{line}"),
            Tok::I(g) => assert_eq!(w.parse::<i32>().expect("integer output"), g, "value {index} (int)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn plib_matches_occt() {
    let text = include_str!("plib.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in plib.txt:\n{line}");
        let (head, tail) = line.split_once(" | ").unwrap_or_else(|| panic!("line without ' | ': {line}"));
        let head: Vec<&str> = head.split_whitespace().collect();
        let want: Vec<&str> = tail.split_whitespace().collect();
        let (name, inputs) = head.split_first().expect("family name");
        let name: &str = name;
        let entry = stats.entry(name).or_default();
        entry.0 += 1;
        entry.1 += usize::from(!inputs.is_empty());

        let mut reader = Rd { words: inputs, at: 0 };
        let mut got = Got::default();
        let outcome = run(name, &mut reader, &mut got);
        let marker = matches!(want.first(), Some(&"throws") | Some(&"unchecked"));
        match outcome {
            Err(Thrown) => assert!(marker, "Rust API returned an error but OCCT returned values\n{line}"),
            Ok(()) => {
                assert!(!marker, "Rust API returned values but OCCT {}\n{line}", want[0]);
                reader.finish();
                compare(&mut tally, line, name, &want, &got.0);
            }
        }
    }

    // Input coverage: no duplicates (checked above), every family with inputs has at least
    // MIN_DISTINCT distinct lines, and only the listed families are input-less (printed once).
    for (name, (count, with_inputs)) in &stats {
        if INPUT_LESS.contains(name) {
            assert_eq!((*count, *with_inputs), (1, 0), "input-less family {name} must be printed exactly once");
        } else {
            assert_eq!(*with_inputs, *count, "family {name} lost its inputs");
            assert!(*count >= MIN_DISTINCT, "family {name} has only {count} distinct lines");
        }
    }
    for name in INPUT_LESS {
        assert!(stats.contains_key(name), "input-less family {name} missing");
    }
    // Every family is still covered, and every EXACT entry names a real family.
    let present: BTreeSet<&str> = stats.keys().copied().collect();
    let expected: BTreeSet<&str> = FAMILIES.iter().copied().collect();
    assert_eq!(present, expected, "family list changed");
    for name in EXACT {
        assert!(expected.contains(name), "EXACT names an unknown family {name}");
    }

    assert_eq!(lines, text.lines().count());
    assert_eq!(lines, GOLDEN_LINES, "golden line count changed");
    println!("{lines} lines, {} values, {} bit-identical, worst {} ulps", tally.values, tally.exact, tally.worst);
}
