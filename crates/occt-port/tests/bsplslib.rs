//! Numerical reference test for OCCT 8.0.1's `BSplSLib` package (every public function of `BSplSLib`, the class
//! `BSplSLib_Cache`, the `FunctionMultiply` callback) and for the `BSplCLib` functions that it calls and that
//! `src/bsplclib.rs` did not have.
//!
//! `bsplslib.txt` is printed by `oracle/bsplslib.cpp` (real OCCT). Each line is
//! `<family> <inputs...> | <outputs...>`: f64 values are 16-hex-digit bit patterns and ints (and bools) are decimal.
//! Every input of a call is on the left of ` | `; this test reads them back with the `Rd` reader methods of the same
//! name as the oracle's schema words (`f i fs is arr2 p3a2 ois oarr2 vec evf surf`) and replays the call on the Rust
//! API of docs/api-bsplslib.md. The right side is either the expected values, `throws` (OCCT raised) or `unchecked`
//! (a check that OCCT 8.0.1 as built has compiled out would have failed and the numbers are unspecified); on both the
//! Rust API must return an error.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic, clippy::float_cmp)]

use occt_port::bsplclib::{self, BSplError};
use occt_port::bsplslib::{self, Cache, Surface};
use occt_port::gp::{Pnt, Vec as Vec3};
use occt_port::mathlin::{MathError, Matrix};
use occt_port::plib::{Array2, PLibError};
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ---------------------------------------------------------------------------------
// Test configuration

/// Largest allowed difference for results that are not computed with `+ - * /` and `sqrt` only, in units of
/// f64::EPSILON times the largest absolute value on the line (at least 1). BSplSLib needs none of that: the list of
/// `EXACT` families below is every family.
const MAX_ULPS: f64 = 8.0;

/// Every family with inputs must have at least this many distinct lines.
const MIN_DISTINCT: usize = 16;

/// Families that really take no input: they are printed exactly once.
const INPUT_LESS: &[&str] = &["bsplclib_constants"];

/// Families whose results use only + - * / and sqrt: compared bit for bit (signed zeros included).
const EXACT: &[&str] = &[
    "bsplclib_bohm_buffer",
    "bsplclib_build_knots",
    "bsplclib_build_schoenberg_points",
    "bsplclib_constants",
    "bsplclib_eval_bspline_basis",
    "bsplclib_eval_buffer",
    "bsplclib_increase_degree",
    "bsplclib_increase_degree_count_knots",
    "bsplclib_insert_knots",
    "bsplclib_interpolate",
    "bsplclib_knot_sequence",
    "bsplclib_knot_sequence_length",
    "bsplclib_locate_parameter_flat",
    "bsplclib_locate_parameter_range",
    "bsplclib_pole_index",
    "bsplclib_prepare_insert_knots",
    "bsplclib_prepare_unperiodize",
    "bsplclib_remove_knot",
    "bsplclib_unperiodize",
    "bsplslib_build_cache",
    "bsplslib_build_cache_array",
    "bsplslib_cache_class_d0",
    "bsplslib_cache_class_d0_local",
    "bsplslib_cache_class_d1",
    "bsplslib_cache_class_d1_local",
    "bsplslib_cache_class_d2",
    "bsplslib_cache_class_d2_local",
    "bsplslib_cache_class_valid",
    "bsplslib_cache_d0",
    "bsplslib_cache_d1",
    "bsplslib_cache_d2",
    "bsplslib_coefs_d0",
    "bsplslib_coefs_d1",
    "bsplslib_coefs_d2",
    "bsplslib_d0",
    "bsplslib_d1",
    "bsplslib_d2",
    "bsplslib_d3",
    "bsplslib_dn",
    "bsplslib_function_multiply",
    "bsplslib_get_poles",
    "bsplslib_get_poles_weighted",
    "bsplslib_homogeneous_d0",
    "bsplslib_homogeneous_d1",
    "bsplslib_increase_degree",
    "bsplslib_insert_knots",
    "bsplslib_interpolate",
    "bsplslib_interpolate_weighted",
    "bsplslib_is_rational",
    "bsplslib_iso",
    "bsplslib_move_point",
    "bsplslib_poles_coefficients",
    "bsplslib_poles_coefficients_nw",
    "bsplslib_rational_derivative",
    "bsplslib_remove_knot",
    "bsplslib_resolution",
    "bsplslib_reverse_poles",
    "bsplslib_reverse_weights",
    "bsplslib_set_poles",
    "bsplslib_set_poles_weighted",
    "bsplslib_unit_weights",
    "bsplslib_unperiodize",
];

/// Every family the oracle must print (a family that disappears from bsplslib.txt fails).
const FAMILIES: &[&str] = &[
    "bsplclib_bohm_buffer",
    "bsplclib_build_knots",
    "bsplclib_build_schoenberg_points",
    "bsplclib_constants",
    "bsplclib_eval_bspline_basis",
    "bsplclib_eval_buffer",
    "bsplclib_increase_degree",
    "bsplclib_increase_degree_count_knots",
    "bsplclib_insert_knots",
    "bsplclib_interpolate",
    "bsplclib_knot_sequence",
    "bsplclib_knot_sequence_length",
    "bsplclib_locate_parameter_flat",
    "bsplclib_locate_parameter_range",
    "bsplclib_pole_index",
    "bsplclib_prepare_insert_knots",
    "bsplclib_prepare_unperiodize",
    "bsplclib_remove_knot",
    "bsplclib_unperiodize",
    "bsplslib_build_cache",
    "bsplslib_build_cache_array",
    "bsplslib_cache_class_d0",
    "bsplslib_cache_class_d0_local",
    "bsplslib_cache_class_d1",
    "bsplslib_cache_class_d1_local",
    "bsplslib_cache_class_d2",
    "bsplslib_cache_class_d2_local",
    "bsplslib_cache_class_valid",
    "bsplslib_cache_d0",
    "bsplslib_cache_d1",
    "bsplslib_cache_d2",
    "bsplslib_coefs_d0",
    "bsplslib_coefs_d1",
    "bsplslib_coefs_d2",
    "bsplslib_d0",
    "bsplslib_d1",
    "bsplslib_d2",
    "bsplslib_d3",
    "bsplslib_dn",
    "bsplslib_function_multiply",
    "bsplslib_get_poles",
    "bsplslib_get_poles_weighted",
    "bsplslib_homogeneous_d0",
    "bsplslib_homogeneous_d1",
    "bsplslib_increase_degree",
    "bsplslib_insert_knots",
    "bsplslib_interpolate",
    "bsplslib_interpolate_weighted",
    "bsplslib_is_rational",
    "bsplslib_iso",
    "bsplslib_move_point",
    "bsplslib_poles_coefficients",
    "bsplslib_poles_coefficients_nw",
    "bsplslib_rational_derivative",
    "bsplslib_remove_knot",
    "bsplslib_resolution",
    "bsplslib_reverse_poles",
    "bsplslib_reverse_weights",
    "bsplslib_set_poles",
    "bsplslib_set_poles_weighted",
    "bsplslib_unit_weights",
    "bsplslib_unperiodize",
];

/// The number of lines of the golden file.
const GOLDEN_LINES: usize = 1295;

// ---------------------------------------------------------------------------------
// Results of one replayed call

/// The Rust API returned an error (or the call is known to be invalid).
#[derive(Debug)]
struct Thrown;

impl From<BSplError> for Thrown {
    fn from(_: BSplError) -> Self {
        Thrown
    }
}

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
    I(i64),
}

/// Collects the values produced by the Rust side, in the order the oracle printed them.
#[derive(Default)]
struct Got(Vec<Tok>);

impl Got {
    fn f(&mut self, v: f64) {
        self.0.push(Tok::F(v));
    }
    fn i(&mut self, v: i32) {
        self.0.push(Tok::I(i64::from(v)));
    }
    fn b(&mut self, v: bool) {
        self.0.push(Tok::I(i64::from(v)));
    }
    fn fs(&mut self, vs: &[f64]) {
        for v in vs {
            self.f(*v);
        }
    }
    fn is(&mut self, vs: &[i32]) {
        for v in vs {
            self.i(*v);
        }
    }
    fn p(&mut self, p: &Pnt) {
        self.fs(&[p.x(), p.y(), p.z()]);
    }
    fn v(&mut self, v: &Vec3) {
        self.fs(&[v.x(), v.y(), v.z()]);
    }
    fn p3s(&mut self, ps: &[Pnt]) {
        for p in ps {
            self.p(p);
        }
    }
    /// The shape of an array, as the oracle prints `(ColLength, RowLength)`.
    fn dims<T>(&mut self, a: &Array2<T>) {
        self.i(i32::try_from(a.rows()).expect("rows"));
        self.i(i32::try_from(a.cols()).expect("cols"));
    }
    /// The points of a 2D array (no shape).
    fn p3a2(&mut self, a: &Array2<Pnt>) {
        self.p3s(a.as_slice());
    }
    /// The values of a 2D array (no shape).
    fn a2(&mut self, a: &Array2<f64>) {
        self.fs(a.as_slice());
    }
}

// ---------------------------------------------------------------------------------
// Reading the inputs of a line

fn bits(w: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(w, 16).unwrap_or_else(|e| panic!("bad hex word {w}: {e}")))
}

/// The compound word `surf`: a surface exactly as the oracle printed it.
struct SurfData {
    u_degree: i32,
    v_degree: i32,
    u_rational: bool,
    v_rational: bool,
    u_periodic: bool,
    v_periodic: bool,
    poles: Array2<Pnt>,
    weights: Option<Array2<f64>>,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    u_mults: Option<Vec<i32>>,
    v_mults: Option<Vec<i32>>,
}

impl SurfData {
    fn surface(&self) -> Surface<'_> {
        Surface {
            u_degree: self.u_degree,
            v_degree: self.v_degree,
            u_rational: self.u_rational,
            v_rational: self.v_rational,
            u_periodic: self.u_periodic,
            v_periodic: self.v_periodic,
            poles: &self.poles,
            weights: self.weights.as_ref(),
            u_knots: &self.u_knots,
            v_knots: &self.v_knots,
            u_mults: self.u_mults.as_deref(),
            v_mults: self.v_mults.as_deref(),
        }
    }
}

/// The function of `FunctionMultiply`: `kind c0 c1 c2 c3`.
struct Evf {
    kind: i32,
    c: [f64; 4],
}

impl Evf {
    /// `Evaluate(derivative request, u, v) -> (result, error code)`; the arithmetic is the oracle's, operation by operation.
    fn call(&self, _request: i32, u: f64, v: f64) -> (f64, i32) {
        let c = &self.c;
        match self.kind {
            0 => (c[0], 0),
            1 => (c[0] + c[1] * u + c[2] * v, 0),
            2 => (c[0] + c[1] * u * v + c[2] * u * u + c[3] * v * v, 0),
            3 => (0.0, 1),
            _ => (c[0] + c[1] * u, i32::from(u > c[2])),
        }
    }
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
    fn count(&mut self) -> usize {
        self.word().parse().expect("count input")
    }
    /// A count followed by that many scalars.
    fn fs(&mut self) -> Vec<f64> {
        let n = self.count();
        (0..n).map(|_| self.f()).collect()
    }
    /// A count followed by that many ints.
    fn is(&mut self) -> Vec<i32> {
        let n = self.count();
        (0..n).map(|_| self.i()).collect()
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
    fn flag(&mut self) -> bool {
        match self.i() {
            0 => false,
            1 => true,
            other => panic!("bad option flag {other}"),
        }
    }
    /// A flag, and when it is 1 an int array: optional multiplicities (`NULL` = flat knots).
    fn ois(&mut self) -> Option<Vec<i32>> {
        if self.flag() { Some(self.is()) } else { None }
    }
    /// A flag, and when it is 1 a 2D scalar array: optional weights.
    fn oarr2(&mut self) -> Option<Array2<f64>> {
        if self.flag() { Some(self.arr2()) } else { None }
    }
    /// Three scalars: a `gp_Vec`.
    fn vec(&mut self) -> Vec3 {
        let (x, y, z) = (self.f(), self.f(), self.f());
        Vec3::new(x, y, z)
    }
    /// `kind c0 c1 c2 c3`.
    fn evf(&mut self) -> Evf {
        let kind = self.i();
        Evf { kind, c: [self.f(), self.f(), self.f(), self.f()] }
    }
    /// `udeg vdeg urat vrat uper vper poles weights uknots vknots umults vmults`.
    fn surf(&mut self) -> SurfData {
        let (u_degree, v_degree) = (self.i(), self.i());
        let (u_rational, v_rational, u_periodic, v_periodic) = (self.flag(), self.flag(), self.flag(), self.flag());
        let poles = self.p3a2();
        let weights = self.oarr2();
        let (u_knots, v_knots) = (self.fs(), self.fs());
        let (u_mults, v_mults) = (self.ois(), self.ois());
        SurfData {
            u_degree,
            v_degree,
            u_rational,
            v_rational,
            u_periodic,
            v_periodic,
            poles,
            weights,
            u_knots,
            v_knots,
            u_mults,
            v_mults,
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
        // ================= BSplCLib =================
        "bsplclib_eval_buffer" => {
            let (u, degree, knots, dim, poles) = (r.f(), r.i(), r.fs(), r.i(), r.fs());
            g.fs(&bsplclib::eval_buffer(u, degree, &knots, dim, &poles)?);
        }
        "bsplclib_bohm_buffer" => {
            let (u, degree, n, knots, dim, poles) = (r.f(), r.i(), r.i(), r.fs(), r.i(), r.fs());
            g.fs(&bsplclib::bohm_buffer(u, degree, n, &knots, dim, &poles)?);
        }
        "bsplclib_build_knots" => {
            let (degree, index, periodic, knots, mults) = (r.i(), r.i(), r.flag(), r.fs(), r.ois());
            g.fs(&bsplclib::build_knots(degree, index, periodic, &knots, mults.as_deref())?);
        }
        "bsplclib_pole_index" => {
            let (degree, index, periodic, mults) = (r.i(), r.i(), r.flag(), r.is());
            g.i(bsplclib::pole_index(degree, index, periodic, &mults)?);
        }
        "bsplclib_locate_parameter_flat" => {
            let (degree, knots, u, periodic, index) = (r.i(), r.fs(), r.f(), r.flag(), r.i());
            let (index, new_u) = bsplclib::locate_parameter_flat(degree, &knots, u, periodic, index)?;
            g.i(index);
            g.f(new_u);
        }
        "bsplclib_locate_parameter_range" => {
            let (degree, knots, u, periodic, from, to) = (r.i(), r.fs(), r.f(), r.flag(), r.i(), r.i());
            let (index, new_u) = bsplclib::locate_parameter_range(degree, &knots, u, periodic, from, to)?;
            g.i(index);
            g.f(new_u);
        }
        "bsplclib_knot_sequence_length" => {
            let (mults, degree, periodic) = (r.is(), r.i(), r.flag());
            g.i(bsplclib::knot_sequence_length(&mults, degree, periodic)?);
        }
        "bsplclib_knot_sequence" => {
            let (knots, mults, degree, periodic) = (r.fs(), r.is(), r.i(), r.flag());
            g.fs(&bsplclib::knot_sequence(&knots, &mults, degree, periodic)?);
        }
        "bsplclib_eval_bspline_basis" => {
            let (der, order, flat, u, periodic) = (r.i(), r.i(), r.fs(), r.f(), r.flag());
            let (lr, ur, lc, uc) = (r.i(), r.i(), r.i(), r.i());
            // the oracle fills the matrix with 0.25 before the call and prints every entry afterwards
            let mut m = Matrix::filled(lr, ur, lc, uc, 0.25)?;
            let (code, first) = bsplclib::eval_bspline_basis(der, order, &flat, u, periodic, &mut m)?;
            g.i(code);
            g.i(first);
            for row in lr..=ur {
                for col in lc..=uc {
                    g.f(m.value(row, col)?);
                }
            }
        }
        "bsplclib_build_schoenberg_points" => {
            let (degree, flat, nb) = (r.i(), r.fs(), r.i());
            g.fs(&bsplclib::build_schoenberg_points(degree, &flat, nb)?);
        }
        "bsplclib_interpolate" => {
            let (degree, flat, params, contact, dim, poles) = (r.i(), r.fs(), r.fs(), r.is(), r.i(), r.fs());
            let (inversion, poles) = bsplclib::interpolate(degree, &flat, &params, &contact, dim, &poles)?;
            g.i(inversion);
            g.fs(&poles);
        }
        "bsplclib_prepare_insert_knots" => {
            let (degree, periodic, knots, mults) = (r.i(), r.flag(), r.fs(), r.is());
            let (add_knots, add_mults, tol, add) = (r.fs(), r.ois(), r.f(), r.flag());
            let res: Option<(i32, i32)> = bsplclib::prepare_insert_knots(
                degree,
                periodic,
                &knots,
                &mults,
                &add_knots,
                add_mults.as_deref(),
                tol,
                add,
            )?;
            match res {
                None => g.i(0),
                Some((nb_poles, nb_knots)) => {
                    g.i(1);
                    g.i(nb_poles);
                    g.i(nb_knots);
                }
            }
        }
        "bsplclib_insert_knots" => {
            let (degree, periodic, dim, poles) = (r.i(), r.flag(), r.i(), r.fs());
            let (knots, mults, add_knots, add_mults, tol, add) = (r.fs(), r.is(), r.fs(), r.ois(), r.f(), r.flag());
            let (np, nk, nm) = bsplclib::insert_knots(
                degree,
                periodic,
                dim,
                &poles,
                &knots,
                &mults,
                &add_knots,
                add_mults.as_deref(),
                tol,
                add,
            )?;
            g.fs(&np);
            g.fs(&nk);
            g.is(&nm);
        }
        "bsplclib_remove_knot" => {
            let (index, mult, degree, periodic, dim) = (r.i(), r.i(), r.i(), r.flag(), r.i());
            let (poles, knots, mults, tol) = (r.fs(), r.fs(), r.is(), r.f());
            let res: Option<(Vec<f64>, Vec<f64>, Vec<i32>)> =
                bsplclib::remove_knot(index, mult, degree, periodic, dim, &poles, &knots, &mults, tol)?;
            match res {
                None => g.i(0),
                Some(t) => {
                    g.i(1);
                    g.fs(&t.0);
                    g.fs(&t.1);
                    g.is(&t.2);
                }
            }
        }
        "bsplclib_increase_degree_count_knots" => {
            let (degree, new_degree, periodic, mults) = (r.i(), r.i(), r.flag(), r.is());
            g.i(bsplclib::increase_degree_count_knots(degree, new_degree, periodic, &mults)?);
        }
        "bsplclib_increase_degree" => {
            let (degree, new_degree, periodic, dim) = (r.i(), r.i(), r.flag(), r.i());
            let (poles, knots, mults) = (r.fs(), r.fs(), r.is());
            let (np, nk, nm) = bsplclib::increase_degree(degree, new_degree, periodic, dim, &poles, &knots, &mults)?;
            g.fs(&np);
            g.fs(&nk);
            g.is(&nm);
        }
        "bsplclib_prepare_unperiodize" => {
            let (degree, mults) = (r.i(), r.is());
            let (nb_knots, nb_poles) = bsplclib::prepare_unperiodize(degree, &mults)?;
            g.i(nb_knots);
            g.i(nb_poles);
        }
        "bsplclib_unperiodize" => {
            let (degree, dim, mults, knots, poles) = (r.i(), r.i(), r.is(), r.fs(), r.fs());
            let (nm, nk, np) = bsplclib::unperiodize(degree, dim, &mults, &knots, &poles)?;
            g.is(&nm);
            g.fs(&nk);
            g.fs(&np);
        }
        "bsplclib_constants" => {
            g.i(bsplclib::max_degree());
            g.i(bsplclib::max_unit_weights_size());
            let data = bsplclib::unit_weights_data();
            assert_eq!(data.len(), 2049);
            g.f(data[0]);
            g.f(data[2048]);
            g.b(bsplclib::no_mults().is_none());
        }
        // ================= BSplSLib: evaluation =================
        "bsplslib_d0" => {
            let (u, v, ui, vi) = (r.f(), r.f(), r.i(), r.i());
            let sd = r.surf();
            g.p(&bsplslib::d0(u, v, ui, vi, &sd.surface())?);
        }
        "bsplslib_d1" => {
            let (u, v, ui, vi) = (r.f(), r.f(), r.i(), r.i());
            let sd = r.surf();
            let (p, vu, vv) = bsplslib::d1(u, v, ui, vi, &sd.surface())?;
            g.p(&p);
            g.v(&vu);
            g.v(&vv);
        }
        "bsplslib_d2" => {
            let (u, v, ui, vi) = (r.f(), r.f(), r.i(), r.i());
            let sd = r.surf();
            let (p, vu, vv, vuu, vvv, vuv) = bsplslib::d2(u, v, ui, vi, &sd.surface())?;
            g.p(&p);
            for x in [&vu, &vv, &vuu, &vvv, &vuv] {
                g.v(x);
            }
        }
        "bsplslib_d3" => {
            let (u, v, ui, vi) = (r.f(), r.f(), r.i(), r.i());
            let sd = r.surf();
            let (p, vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) = bsplslib::d3(u, v, ui, vi, &sd.surface())?;
            g.p(&p);
            for x in [&vu, &vv, &vuu, &vvv, &vuv, &vuuu, &vvvv, &vuuv, &vuvv] {
                g.v(x);
            }
        }
        "bsplslib_dn" => {
            let (u, v, nu, nv, ui, vi) = (r.f(), r.f(), r.i(), r.i(), r.i(), r.i());
            let sd = r.surf();
            g.v(&bsplslib::dn(u, v, nu, nv, ui, vi, &sd.surface())?);
        }
        "bsplslib_homogeneous_d0" => {
            let (u, v, ui, vi) = (r.f(), r.f(), r.i(), r.i());
            let sd = r.surf();
            let (w, p) = bsplslib::homogeneous_d0(u, v, ui, vi, &sd.surface())?;
            g.f(w);
            g.p(&p);
        }
        "bsplslib_homogeneous_d1" => {
            let (u, v, ui, vi) = (r.f(), r.f(), r.i(), r.i());
            let sd = r.surf();
            let (n, nu, nv, d, du, dv) = bsplslib::homogeneous_d1(u, v, ui, vi, &sd.surface())?;
            g.p(&n);
            g.v(&nu);
            g.v(&nv);
            g.f(d);
            g.f(du);
            g.f(dv);
        }
        "bsplslib_rational_derivative" => {
            let (ud, vd, n, m, ders, all) = (r.i(), r.i(), r.i(), r.i(), r.fs(), r.flag());
            g.fs(&bsplslib::rational_derivative(ud, vd, n, m, &ders, all)?);
        }
        "bsplslib_iso" => {
            let (param, is_u, poles, weights) = (r.f(), r.flag(), r.p3a2(), r.oarr2());
            let (knots, mults, degree, periodic, want) = (r.fs(), r.ois(), r.i(), r.flag(), r.flag());
            let (cp, cw) =
                bsplslib::iso(param, is_u, &poles, weights.as_ref(), &knots, mults.as_deref(), degree, periodic, want)?;
            g.p3s(&cp);
            if want {
                g.fs(&cw.expect("weights were requested"));
            }
        }
        "bsplslib_reverse_poles" => {
            let (poles, last, u_dir) = (r.p3a2(), r.i(), r.flag());
            let a = bsplslib::reverse_poles(&poles, last, u_dir)?;
            g.dims(&a);
            g.p3a2(&a);
        }
        "bsplslib_reverse_weights" => {
            let (w, last, u_dir) = (r.arr2(), r.i(), r.flag());
            let a = bsplslib::reverse_weights(&w, last, u_dir)?;
            g.dims(&a);
            g.a2(&a);
        }
        "bsplslib_is_rational" => {
            let (w, i1, i2, j1, j2, eps) = (r.arr2(), r.i(), r.i(), r.i(), r.i(), r.f());
            g.b(bsplslib::is_rational(&w, i1, i2, j1, j2, eps)?);
        }
        "bsplslib_set_poles" => {
            let (poles, u_dir) = (r.p3a2(), r.flag());
            g.fs(&bsplslib::set_poles(&poles, u_dir)?);
        }
        "bsplslib_set_poles_weighted" => {
            let (poles, weights, u_dir) = (r.p3a2(), r.arr2(), r.flag());
            g.fs(&bsplslib::set_poles_weighted(&poles, &weights, u_dir)?);
        }
        "bsplslib_get_poles" => {
            let (rows, cols, fp, u_dir) = (r.i(), r.i(), r.fs(), r.flag());
            let poles = bsplslib::get_poles(&fp, rows, cols, u_dir)?;
            g.dims(&poles);
            g.p3a2(&poles);
        }
        "bsplslib_get_poles_weighted" => {
            let (rows, cols, fp, u_dir) = (r.i(), r.i(), r.fs(), r.flag());
            let (poles, weights) = bsplslib::get_poles_weighted(&fp, rows, cols, u_dir)?;
            g.dims(&poles);
            g.p3a2(&poles);
            g.dims(&weights);
            g.a2(&weights);
        }
        // ================= BSplSLib: knot operations =================
        "bsplslib_insert_knots" => {
            let (u_dir, degree, periodic, poles, weights) = (r.flag(), r.i(), r.flag(), r.p3a2(), r.oarr2());
            let (knots, mults, add_knots, add_mults, eps, add) = (r.fs(), r.is(), r.fs(), r.ois(), r.f(), r.flag());
            let res = bsplslib::insert_knots(
                u_dir,
                degree,
                periodic,
                &poles,
                weights.as_ref(),
                &knots,
                &mults,
                &add_knots,
                add_mults.as_deref(),
                eps,
                add,
            )?;
            got_knot_result(g, &res);
        }
        "bsplslib_remove_knot" => {
            let (u_dir, index, mult, degree, periodic) = (r.flag(), r.i(), r.i(), r.i(), r.flag());
            let (poles, weights, knots, mults, tol) = (r.p3a2(), r.oarr2(), r.fs(), r.is(), r.f());
            let res: Option<bsplslib::KnotResult> = bsplslib::remove_knot(
                u_dir,
                index,
                mult,
                degree,
                periodic,
                &poles,
                weights.as_ref(),
                &knots,
                &mults,
                tol,
            )?;
            match res {
                None => g.i(0),
                Some(res) => {
                    g.i(1);
                    got_knot_result(g, &res);
                }
            }
        }
        "bsplslib_increase_degree" => {
            let (u_dir, degree, new_degree, periodic) = (r.flag(), r.i(), r.i(), r.flag());
            let (poles, weights, knots, mults) = (r.p3a2(), r.oarr2(), r.fs(), r.is());
            let res = bsplslib::increase_degree(
                u_dir,
                degree,
                new_degree,
                periodic,
                &poles,
                weights.as_ref(),
                &knots,
                &mults,
            )?;
            got_knot_result(g, &res);
        }
        "bsplslib_unperiodize" => {
            let (u_dir, degree, mults, knots, poles, weights) = (r.flag(), r.i(), r.is(), r.fs(), r.p3a2(), r.oarr2());
            let res = bsplslib::unperiodize(u_dir, degree, &mults, &knots, &poles, weights.as_ref())?;
            // the oracle prints the C++ output order: mults, knots, poles, weights
            g.is(&res.mults);
            g.fs(&res.knots);
            g.dims(&res.poles);
            g.p3a2(&res.poles);
            if let Some(w) = &res.weights {
                g.dims(w);
                g.a2(w);
            }
        }
        // ================= BSplSLib: caches =================
        "bsplslib_build_cache" => {
            let (u, v, us, vs) = (r.f(), r.f(), r.f(), r.f());
            let (up, vp, ud, vd, ui, vi) = (r.flag(), r.flag(), r.i(), r.i(), r.i(), r.i());
            let (uf, vf, poles, weights) = (r.fs(), r.fs(), r.p3a2(), r.oarr2());
            let (cp, cw) =
                bsplslib::build_cache(u, v, us, vs, up, vp, ud, vd, ui, vi, &uf, &vf, &poles, weights.as_ref())?;
            g.dims(&cp);
            g.p3a2(&cp);
            if let Some(w) = &cw {
                g.dims(w);
                g.a2(w);
            }
        }
        "bsplslib_build_cache_array" => {
            let (u, v, us, vs) = (r.f(), r.f(), r.f(), r.f());
            let (up, vp, ud, vd, ui, vi) = (r.flag(), r.flag(), r.i(), r.i(), r.i(), r.i());
            let (uf, vf, poles, weights) = (r.fs(), r.fs(), r.p3a2(), r.oarr2());
            let a =
                bsplslib::build_cache_array(u, v, us, vs, up, vp, ud, vd, ui, vi, &uf, &vf, &poles, weights.as_ref())?;
            g.dims(&a);
            g.a2(&a);
        }
        "bsplslib_cache_d0" => {
            let (u, v, ud, vd, ucp, vcp, us, vs) = (r.f(), r.f(), r.i(), r.i(), r.f(), r.f(), r.f(), r.f());
            let (poles, weights) = (r.p3a2(), r.oarr2());
            g.p(&bsplslib::cache_d0(u, v, ud, vd, ucp, vcp, us, vs, &poles, weights.as_ref())?);
        }
        "bsplslib_cache_d1" => {
            let (u, v, ud, vd, ucp, vcp, us, vs) = (r.f(), r.f(), r.i(), r.i(), r.f(), r.f(), r.f(), r.f());
            let (poles, weights) = (r.p3a2(), r.oarr2());
            let (p, a, b) = bsplslib::cache_d1(u, v, ud, vd, ucp, vcp, us, vs, &poles, weights.as_ref())?;
            g.p(&p);
            g.v(&a);
            g.v(&b);
        }
        "bsplslib_cache_d2" => {
            let (u, v, ud, vd, ucp, vcp, us, vs) = (r.f(), r.f(), r.i(), r.i(), r.f(), r.f(), r.f(), r.f());
            let (poles, weights) = (r.p3a2(), r.oarr2());
            let (p, a, b, c, d, e) = bsplslib::cache_d2(u, v, ud, vd, ucp, vcp, us, vs, &poles, weights.as_ref())?;
            g.p(&p);
            for x in [&a, &b, &c, &d, &e] {
                g.v(x);
            }
        }
        "bsplslib_coefs_d0" => {
            let (u, v, poles, weights) = (r.f(), r.f(), r.p3a2(), r.oarr2());
            g.p(&bsplslib::coefs_d0(u, v, &poles, weights.as_ref())?);
        }
        "bsplslib_coefs_d1" => {
            let (u, v, poles, weights) = (r.f(), r.f(), r.p3a2(), r.oarr2());
            let (p, a, b) = bsplslib::coefs_d1(u, v, &poles, weights.as_ref())?;
            g.p(&p);
            g.v(&a);
            g.v(&b);
        }
        "bsplslib_coefs_d2" => {
            let (u, v, poles, weights) = (r.f(), r.f(), r.p3a2(), r.oarr2());
            let (p, a, b, c, d, e) = bsplslib::coefs_d2(u, v, &poles, weights.as_ref())?;
            g.p(&p);
            for x in [&a, &b, &c, &d, &e] {
                g.v(x);
            }
        }
        "bsplslib_poles_coefficients" => {
            let (poles, weights) = (r.p3a2(), r.oarr2());
            let (cp, cw) = bsplslib::poles_coefficients(&poles, weights.as_ref())?;
            g.dims(&cp);
            g.p3a2(&cp);
            if let Some(w) = &cw {
                g.dims(w);
                g.a2(w);
            }
        }
        "bsplslib_poles_coefficients_nw" => {
            let poles = r.p3a2();
            let cp = bsplslib::poles_coefficients_nw(&poles)?;
            g.dims(&cp);
            g.p3a2(&cp);
        }
        "bsplslib_cache_class_valid" => {
            let sd = r.surf();
            let (build, u0, v0, u, v) = (r.flag(), r.f(), r.f(), r.f(), r.f());
            let c = cache_for(&sd, build, u0, v0)?;
            g.b(c.is_cache_valid(u, v));
        }
        "bsplslib_cache_class_d0" => {
            let sd = r.surf();
            let (build, u0, v0, u, v) = (r.flag(), r.f(), r.f(), r.f(), r.f());
            g.p(&cache_for(&sd, build, u0, v0)?.d0(u, v)?);
        }
        "bsplslib_cache_class_d1" => {
            let sd = r.surf();
            let (build, u0, v0, u, v) = (r.flag(), r.f(), r.f(), r.f(), r.f());
            let (p, a, b) = cache_for(&sd, build, u0, v0)?.d1(u, v)?;
            g.p(&p);
            g.v(&a);
            g.v(&b);
        }
        "bsplslib_cache_class_d2" => {
            let sd = r.surf();
            let (build, u0, v0, u, v) = (r.flag(), r.f(), r.f(), r.f(), r.f());
            let (p, a, b, c, d, e) = cache_for(&sd, build, u0, v0)?.d2(u, v)?;
            g.p(&p);
            for x in [&a, &b, &c, &d, &e] {
                g.v(x);
            }
        }
        "bsplslib_cache_class_d0_local" => {
            let sd = r.surf();
            let (build, u0, v0, u, v) = (r.flag(), r.f(), r.f(), r.f(), r.f());
            g.p(&cache_for(&sd, build, u0, v0)?.d0_local(u, v)?);
        }
        "bsplslib_cache_class_d1_local" => {
            let sd = r.surf();
            let (build, u0, v0, u, v) = (r.flag(), r.f(), r.f(), r.f(), r.f());
            let (p, a, b) = cache_for(&sd, build, u0, v0)?.d1_local(u, v)?;
            g.p(&p);
            g.v(&a);
            g.v(&b);
        }
        "bsplslib_cache_class_d2_local" => {
            let sd = r.surf();
            let (build, u0, v0, u, v) = (r.flag(), r.f(), r.f(), r.f(), r.f());
            let (p, a, b, c, d, e) = cache_for(&sd, build, u0, v0)?.d2_local(u, v)?;
            g.p(&p);
            for x in [&a, &b, &c, &d, &e] {
                g.v(x);
            }
        }
        // ================= BSplSLib: the rest =================
        "bsplslib_resolution" => {
            let sd = r.surf();
            let tol = r.f();
            let (ut, vt) = bsplslib::resolution(&sd.surface(), tol)?;
            g.f(ut);
            g.f(vt);
        }
        "bsplslib_interpolate" => {
            let (ud, vd, uf, vf, up, vp, poles) = (r.i(), r.i(), r.fs(), r.fs(), r.fs(), r.fs(), r.p3a2());
            let (code, p) = bsplslib::interpolate(ud, vd, &uf, &vf, &up, &vp, &poles)?;
            g.i(code);
            g.dims(&p);
            g.p3a2(&p);
        }
        "bsplslib_interpolate_weighted" => {
            let (ud, vd, uf, vf, up, vp) = (r.i(), r.i(), r.fs(), r.fs(), r.fs(), r.fs());
            let (poles, weights) = (r.p3a2(), r.arr2());
            let (code, p, w) = bsplslib::interpolate_weighted(ud, vd, &uf, &vf, &up, &vp, &poles, &weights)?;
            g.i(code);
            g.dims(&p);
            g.p3a2(&p);
            g.dims(&w);
            g.a2(&w);
        }
        "bsplslib_function_multiply" => {
            let (e, ud, vd, uk, vk, um, vm) = (r.evf(), r.i(), r.i(), r.fs(), r.fs(), r.ois(), r.ois());
            let (poles, weights, uf, vf, und, vnd) = (r.p3a2(), r.oarr2(), r.fs(), r.fs(), r.i(), r.i());
            let f = |request: i32, u: f64, v: f64| e.call(request, u, v);
            let (num, den, status) = bsplslib::function_multiply(
                &f,
                ud,
                vd,
                &uk,
                &vk,
                um.as_deref(),
                vm.as_deref(),
                &poles,
                weights.as_ref(),
                &uf,
                &vf,
                und,
                vnd,
            )?;
            g.i(status);
            g.dims(&num);
            g.p3a2(&num);
            g.dims(&den);
            g.a2(&den);
        }
        "bsplslib_unit_weights" => {
            let (nu, nv) = (r.i(), r.i());
            let w = bsplslib::unit_weights(nu, nv)?;
            g.dims(&w);
            let v = w.as_slice();
            g.f(v[0]);
            g.f(v[v.len() - 1]);
            g.f(v.iter().fold(0.0, |s, x| s + x));
            g.f(v.iter().fold(f64::INFINITY, |s, x| s.min(*x)));
            g.f(v.iter().fold(f64::NEG_INFINITY, |s, x| s.max(*x)));
        }
        "bsplslib_move_point" => {
            let (u, v, displ) = (r.f(), r.f(), r.vec());
            let (ui1, ui2, vi1, vi2, ud, vd, rational) = (r.i(), r.i(), r.i(), r.i(), r.i(), r.i(), r.flag());
            let (poles, weights, uf, vf) = (r.p3a2(), r.arr2(), r.fs(), r.fs());
            let m =
                bsplslib::move_point(u, v, &displ, ui1, ui2, vi1, vi2, ud, vd, rational, &poles, &weights, &uf, &vf)?;
            g.b(m.poles.is_some());
            g.i(m.u_first);
            g.i(m.u_last);
            g.i(m.v_first);
            g.i(m.v_last);
            if let Some(p) = &m.poles {
                g.p3a2(p);
            }
        }
        _ => panic!("unknown oracle family {name}"),
    }
    Ok(())
}

/// poles, weights, knots, mults of a knot operation (the order of the oracle's output)
fn got_knot_result(g: &mut Got, res: &bsplslib::KnotResult) {
    g.dims(&res.poles);
    g.p3a2(&res.poles);
    if let Some(w) = &res.weights {
        g.dims(w);
        g.a2(w);
    }
    g.fs(&res.knots);
    g.is(&res.mults);
}

/// A `BSplSLib_Cache` for the (flat knot) surface; built at (`u0`, `v0`) when `build`.
fn cache_for(sd: &SurfData, build: bool, u0: f64, v0: f64) -> Result<Cache, Thrown> {
    let mut c = Cache::new(
        sd.u_degree,
        sd.u_periodic,
        &sd.u_knots,
        sd.v_degree,
        sd.v_periodic,
        &sd.v_knots,
        sd.weights.as_ref(),
    )?;
    if build {
        c.build_cache(u0, v0, &sd.u_knots, &sd.v_knots, &sd.poles, sd.weights.as_ref())?;
    }
    Ok(c)
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
/// otherwise (zero tolerance when that magnitude is infinite); ints always exactly.
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
            Tok::I(g) => assert_eq!(w.parse::<i64>().expect("integer output"), g, "value {index} (int)\n{line}"),
        }
    }
}

// ---------------------------------------------------------------------------------

#[test]
fn bsplslib_matches_occt() {
    let text = include_str!("bsplslib.txt");
    let mut tally = Tally::default();
    let mut lines = 0usize;
    let mut seen: HashSet<&str> = HashSet::new();
    // family -> (number of lines, number of lines with at least one input)
    let mut stats: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for line in text.lines() {
        lines += 1;
        assert!(seen.insert(line), "duplicate line in bsplslib.txt:\n{line}");
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
