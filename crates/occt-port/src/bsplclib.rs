//! The 3D curve evaluation part of OCCT's `BSplCLib`, and the flat-array functions that `BSplSLib` needs
//! (`Eval`/`Bohm` on raw buffers, local knots, knot location, B-spline basis, interpolation, knot insertion and
//! removal, degree elevation, unperiodizing).
#![allow(clippy::float_cmp)]
use crate::gp::{self, Pnt, Vec};
use crate::mathlin::MathError;
use crate::plib::PLibError;

pub(crate) mod basis;
pub(crate) mod flat;
pub(crate) mod knotops;
pub(crate) mod util;

pub use basis::{build_schoenberg_points, eval_bspline_basis, interpolate};
pub use flat::{
    bohm_buffer, build_knots, eval_buffer, knot_sequence, knot_sequence_length, locate_parameter_flat,
    locate_parameter_range, max_degree, max_unit_weights_size, no_mults, pole_index, unit_weights_data,
};
pub use knotops::{
    increase_degree, increase_degree_count_knots, insert_knots, prepare_insert_knots, prepare_unperiodize, remove_knot,
    unperiodize,
};

/// The error type of `BSplCLib` and `BSplSLib`, standing for the exceptions of OCCT and for the checks that OCCT
/// 8.0.1 as built does not make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BSplError {
    /// `Standard_ConstructionError` raised by OCCT (`function_multiply` when the function reports an error).
    Construction,
    /// `Standard_OutOfRange` raised by OCCT (`interpolate` on a singular system), and every call that OCCT 8.0.1 as
    /// built does not check and that would fail (an access outside an array, a degree above the maximum, ...).
    OutOfRange,
    /// Any other `Standard_Failure`.
    Failure,
    /// A wrong shape of a Rust argument (slices that are too short, sizes that do not match, ...).
    Argument,
    /// An error of `mathlin` used inside.
    Math(MathError),
    /// An error of `plib` used inside.
    PLib(PLibError),
}

impl From<MathError> for BSplError {
    fn from(e: MathError) -> Self {
        BSplError::Math(e)
    }
}

impl From<PLibError> for BSplError {
    fn from(e: PLibError) -> Self {
        BSplError::PLib(e)
    }
}

/// A B-spline curve with OCCT's pole, knot, and multiplicity data.
#[derive(Debug)]
pub struct Curve<'a> {
    pub degree: i32,
    pub periodic: bool,
    pub poles: &'a [Pnt],
    pub weights: Option<&'a [f64]>,
    pub knots: &'a [f64],
    pub mults: &'a [i32],
}

// Slice reads and writes model the C++ pointer walks; valid OCCT input stays in bounds.
fn r<T: Copy>(s: &[T], i: usize, d: T) -> T {
    s.get(i).copied().unwrap_or(d)
}
fn w<T>(s: &mut [T], i: usize, v: T) {
    if let Some(x) = s.get_mut(i) {
        *x = v;
    }
}
// Epsilon(x) of OCCT
fn eps(x: f64) -> f64 {
    util::epsilon(x)
}

// BSplCLib::LocateParameter
pub fn locate_parameter(degree: i32, knots: &[f64], mults: &[i32], u: f64, periodic: bool) -> (i32, f64) {
    let n = mults.len();
    let mut first = 1i32;
    let mut sum = r(mults, 0, 0);
    while sum <= degree && (first as usize) < n {
        first += 1;
        sum += r(mults, first.saturating_sub(1) as usize, 0);
    }
    let mut last = n as i32;
    sum = r(mults, last.saturating_sub(1) as usize, 0);
    while sum <= degree && last > 0 {
        last -= 1;
        sum += r(mults, last.saturating_sub(1) as usize, 0);
    }
    let (first, last) = if periodic { (1, n as i32) } else { (first, last) };
    let mut index = 0i32;
    let mut new_u = u;
    if index < first || index > last {
        let uf = r(knots, first.saturating_sub(1) as usize, 0.);
        let ul = r(knots, last.saturating_sub(1) as usize, 0.);
        if periodic && (new_u < uf || new_u > ul) {
            let period = ul - uf;
            if period >= eps(ul) {
                new_u = uf.max(new_u + period * ((uf - new_u) / period).ceil());
            }
        }
        let mut pos = if r(knots, 0, 0.) > new_u {
            0
        } else if r(knots, knots.len().saturating_sub(1), 0.) < new_u {
            knots.len() as i32 + 1
        } else {
            let mut lo = 0usize;
            let mut hi = knots.len().saturating_sub(1);
            while hi.saturating_sub(lo) > 1 {
                let mid = (hi + lo) / 2;
                if r(knots, mid, 0.) < new_u {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            lo as i32 + 1
        };
        // Hunt uses zero-based storage while OCCT exposes one-based knot numbers.
        let last1 = last - 1;
        let e = eps(r(knots, knots.len().saturating_sub(1), 0.).abs().min(u.abs()));
        if pos < knots.len() as i32 && (new_u - r(knots, pos.max(0) as usize, 0.)).abs() <= e {
            pos += 1;
        }
        if pos < first {
            pos = first;
        }
        if pos > last1 {
            pos = last1;
        }
        if pos != last1 {
            let mut k1 = r(knots, pos.max(1).saturating_sub(1) as usize, 0.);
            let mut k2 = r(knots, pos.max(1) as usize, 0.);
            let mut val = (k2 - k1).abs();
            while val <= e {
                pos += 1;
                if pos >= knots.len() as i32 {
                    break;
                }
                k1 = k2;
                k2 = r(knots, pos.max(1) as usize, 0.);
                val = (k2 - k1).abs();
            }
        }
        index = pos;
    }
    (index, new_u)
}

// BSplCLib::PrepareEval_T, BuildKnots, PoleIndex, and BSplCLib_BuildEval
fn prepared(u: f64, c: &Curve<'_>) -> (f64, std::vec::Vec<f64>, std::vec::Vec<f64>, usize, bool) {
    let degree = c.degree.max(0) as usize;
    let (kindex, u) = locate_parameter(c.degree, c.knots, c.mults, u, c.periodic);
    let mut local_knots = vec![0.; degree * 2 + 2];
    let mut ilow = kindex;
    let mut mlow = 0;
    let mut iupp = kindex + 1;
    let mut mupp = 0;
    let mut loffset = 0.;
    let mut uoffset = 0.;
    let mut getlow = true;
    let mut getupp = true;
    let kupper = c.knots.len() as i32;
    let mupper = c.mults.len() as i32;
    let dknot = if c.periodic { r(c.knots, kupper.saturating_sub(1) as usize, 0.) - r(c.knots, 0, 0.) } else { 0. };
    if c.periodic && iupp > mupper {
        iupp = 1;
        uoffset = dknot;
    }
    for i in 0..degree {
        if getlow {
            mlow += 1;
            if mlow > r(c.mults, ilow.saturating_sub(1).max(0) as usize, 0) {
                mlow = 1;
                ilow -= 1;
                getlow = ilow >= 1;
                if c.periodic && !getlow {
                    ilow = mupper - 1;
                    loffset = dknot;
                    getlow = true;
                }
            }
            if getlow {
                w(&mut local_knots, degree - 1 - i, r(c.knots, ilow.saturating_sub(1).max(0) as usize, 0.) - loffset);
            }
        }
        if getupp {
            mupp += 1;
            if mupp > r(c.mults, iupp.saturating_sub(1).max(0) as usize, 0) {
                mupp = 1;
                iupp += 1;
                getupp = iupp <= mupper;
                if c.periodic && !getupp {
                    iupp = 2;
                    uoffset = dknot;
                    getupp = true;
                }
            }
            if getupp {
                w(&mut local_knots, degree + i, r(c.knots, iupp.saturating_sub(1).max(0) as usize, 0.) + uoffset);
            }
        }
    }
    let mut pindex = 0i32;
    for i in 0..kindex {
        pindex += r(c.mults, i.max(0) as usize, 0);
    }
    pindex -= if c.periodic { r(c.mults, 0, 0) } else { c.degree + 1 };
    let pcount = c.poles.len();
    let rational = c.weights.is_some_and(|weights| {
        let start = pindex.max(0) as usize;
        let count = weights.len();
        count > 0 && (0..degree).any(|i| r(weights, (start + i) % count, 0.) != r(weights, (start + i + 1) % count, 0.))
    });
    let dim = if rational { 4 } else { 3 };
    let mut poles = vec![0.; (degree + 1) * dim];
    for i in 0..=degree {
        let pi = (pindex + i as i32).rem_euclid(pcount.max(1) as i32) as usize;
        let point = r(c.poles, pi, Pnt::new(0., 0., 0.));
        let p = point.xyz();
        if rational {
            let weight = r(c.weights.unwrap_or(&[]), pi, 0.);
            w(&mut poles, i * dim, p.x() * weight);
            w(&mut poles, i * dim + 1, p.y() * weight);
            w(&mut poles, i * dim + 2, p.z() * weight);
            w(&mut poles, i * dim + 3, weight);
        } else {
            w(&mut poles, i * dim, p.x());
            w(&mut poles, i * dim + 1, p.y());
            w(&mut poles, i * dim + 2, p.z());
        }
    }
    (u, local_knots, poles, dim, rational)
}

// BSplCLib_D0, BSplCLib_D1, BSplCLib_D2, BSplCLib_D3, and BSplCLib_DN evaluation path
fn evaluate(u: f64, n: usize, c: &Curve<'_>) -> (std::vec::Vec<f64>, usize, bool) {
    let (u, knots, mut poles, dim, rational) = prepared(u, c);
    if n == 0 {
        let degree = c.degree.max(0) as usize;
        let mut dms = degree + 1;
        for step in -1i32..(degree as i32 - 1) {
            dms -= 1;
            let mut dpi = degree as i32 - 1;
            let mut sti = step;
            for i in 0..dms {
                dpi += 1;
                sti += 1;
                let x = (r(&knots, dpi.max(0) as usize, 0.) - u)
                    / (r(&knots, dpi.max(0) as usize, 0.) - r(&knots, sti.max(0) as usize, 0.));
                let y = 1. - x;
                for k in 0..dim {
                    let ix = i * dim + k;
                    let a = r(&poles, ix, 0.) * x;
                    w(&mut poles, ix, a);
                    let b = y * r(&poles, ix + dim, 0.);
                    let current = r(&poles, ix, 0.);
                    w(&mut poles, ix, current + b);
                }
            }
        }
    } else {
        bohm(u, c.degree.max(0) as usize, n, &knots, dim, &mut poles);
    }
    if rational && n > 0 { rational_derivative(c.degree.max(0) as usize, n, &poles) } else { (poles, dim, rational) }
}

// BSplCLib::Bohm
fn bohm(u: f64, degree: usize, n: usize, knots: &[f64], dim: usize, poles: &mut [f64]) {
    let min = n.min(degree);
    let mut ddmi = (degree << 1) + 1;
    for i in 0..degree {
        ddmi -= 1;
        let mut jdmi = ddmi;
        let mut pole = (degree * dim) as isize;
        let mut tbis = pole - dim as isize;
        for j in (i..degree).rev() {
            jdmi -= 1;
            let coef =
                if r(knots, jdmi, 0.) == r(knots, j, 0.) { 0. } else { 1. / (r(knots, jdmi, 0.) - r(knots, j, 0.)) };
            for k in 0..dim {
                let ix = (pole + k as isize) as usize;
                let ix2 = (tbis + k as isize) as usize;
                let value = r(poles, ix, 0.) - r(poles, ix2, 0.);
                w(poles, ix, value);
                w(poles, ix, r(poles, ix, 0.) * coef);
            }
            pole -= dim as isize;
            tbis -= dim as isize;
        }
    }
    for i in 0..degree {
        let coef = u - r(knots, i, 0.);
        let mut pole = (i * dim) as isize;
        let mut tbis = pole + dim as isize;
        for _ in (0..=i).rev() {
            for k in 0..dim {
                let ix = (pole + k as isize) as usize;
                let ix2 = (tbis + k as isize) as usize;
                w(poles, ix, r(poles, ix, 0.) + coef * r(poles, ix2, 0.));
            }
            pole -= dim as isize;
            tbis -= dim as isize;
        }
    }
    let mut coef = degree as f64;
    let mut dmi = degree;
    for i in 1..=min {
        for k in 0..dim {
            let ix = i * dim + k;
            w(poles, ix, r(poles, ix, 0.) * coef);
        }
        dmi -= 1;
        coef *= dmi as f64;
    }
}

// PLib::RationalDerivative
fn rational_derivative(degree: usize, request: usize, poles: &[f64]) -> (std::vec::Vec<f64>, usize, bool) {
    let mindeg = request.min(degree);
    let mut binom = vec![1.; request + 1];
    let mut out = vec![0.; (request + 1) * 3];
    let inverse = 1. / r(poles, 3, 0.);
    let mut other = 0;
    for ii in 0..=mindeg {
        let mut idx = ii * 3;
        let mut idx1 = ii.saturating_sub(1) * 3;
        for k in 0..3 {
            w(&mut out, idx, r(poles, other, 0.));
            idx += 1;
            if k < 2 {
                other += 1;
            }
        }
        idx -= 3;
        for jj in (0..ii).rev() {
            let factor = r(&binom, jj, 0.) * r(poles, (ii - jj) * 4 + 3, 0.);
            for _ in 0..3 {
                let v = r(&out, idx, 0.) - factor * r(&out, idx1, 0.);
                w(&mut out, idx, v);
                idx += 1;
                idx1 += 1;
            }
            idx -= 3;
            idx1 = idx1.saturating_sub(6);
        }
        for jj in (1..=ii).rev() {
            let v = r(&binom, jj, 0.) + r(&binom, jj - 1, 0.);
            w(&mut binom, jj, v);
        }
        for _ in 0..3 {
            let value = r(&out, idx, 0.) * inverse;
            w(&mut out, idx, value);
            idx += 1;
        }
        if ii < mindeg {
            other += 2;
        }
    }
    for ii in mindeg + 1..=request {
        let mut idx = ii * 3;
        let mut idx1 = (ii - 1) * 3;
        for _ in 0..3 {
            w(&mut out, idx, 0.);
            idx += 1;
        }
        idx -= 3;
        for jj in (ii - mindeg..ii).rev() {
            let factor = r(&binom, jj, 0.) * r(poles, (ii - jj) * 4 + 3, 0.);
            for _ in 0..3 {
                let value = r(&out, idx, 0.) - factor * r(&out, idx1, 0.);
                w(&mut out, idx, value);
                idx += 1;
                idx1 += 1;
            }
            idx -= 3;
            idx1 -= 6;
        }
        for jj in (1..=ii).rev() {
            let value = r(&binom, jj, 0.) + r(&binom, jj - 1, 0.);
            w(&mut binom, jj, value);
        }
        for _ in 0..3 {
            let value = r(&out, idx, 0.) * inverse;
            w(&mut out, idx, value);
            idx += 1;
        }
    }
    (out, 3, true)
}

// BSplCLib_CurveTraits::CoordsToPointDirect
fn p(v: &[f64], i: usize) -> Pnt {
    gp::point(gp::xyz(r(v, i, 0.), r(v, i + 1, 0.), r(v, i + 2, 0.)))
}
// BSplCLib_CurveTraits::CoordsToVectorDirect
fn v(vv: &[f64], i: usize) -> Vec {
    gp::vector(gp::xyz(r(vv, i, 0.), r(vv, i + 1, 0.), r(vv, i + 2, 0.)))
}
// BSplCLib::D0
pub fn d0(u: f64, c: &Curve<'_>) -> Pnt {
    let (a, dim, rat) = evaluate(u, 0, c);
    if rat {
        let w0 = r(&a, 3, 1.);
        gp::point(gp::xyz(r(&a, 0, 0.) / w0, r(&a, 1, 0.) / w0, r(&a, 2, 0.) / w0))
    } else {
        let _ = dim;
        p(&a, 0)
    }
}
// BSplCLib::D1
pub fn d1(u: f64, c: &Curve<'_>) -> (Pnt, Vec) {
    let (a, _, _) = evaluate(u, 1, c);
    (p(&a, 0), v(&a, 3))
}
// BSplCLib::D2
pub fn d2(u: f64, c: &Curve<'_>) -> (Pnt, Vec, Vec) {
    let (a, _, rat) = evaluate(u, 2, c);
    (p(&a, 0), v(&a, 3), if !rat && c.degree < 2 { gp::vector(gp::xyz(0., 0., 0.)) } else { v(&a, 6) })
}
// BSplCLib::D3
pub fn d3(u: f64, c: &Curve<'_>) -> (Pnt, Vec, Vec, Vec) {
    let (a, _, rat) = evaluate(u, 3, c);
    (
        p(&a, 0),
        v(&a, 3),
        if !rat && c.degree < 2 { gp::vector(gp::xyz(0., 0., 0.)) } else { v(&a, 6) },
        if !rat && c.degree < 3 { gp::vector(gp::xyz(0., 0., 0.)) } else { v(&a, 9) },
    )
}
// BSplCLib::DN
pub fn dn(u: f64, n: i32, c: &Curve<'_>) -> Vec {
    let (a, _, rat) = evaluate(u, n.max(0) as usize, c);
    if !rat && n > c.degree { gp::vector(gp::xyz(0., 0., 0.)) } else { v(&a, n.max(0) as usize * 3) }
}
