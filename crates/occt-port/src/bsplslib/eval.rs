//! The evaluation functions of `BSplSLib`: `PrepareEval`, `D0` .. `D3`, `DN`, `HomogeneousD0/D1` and
//! `RationalDerivative`.
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use crate::bsplclib::BSplError;
use crate::bsplclib::flat::{bohm_in_place, build_knots, eval_in_place, locate_parameter_opt, pole_index};
use crate::bsplclib::util::{R, epsilon, ix, len_ix, mul, rd, to_i32, wr, zeros};
use crate::gp::{Pnt, Vec as Vec3};
use crate::plib::{self, Array2};

/// A B-spline surface with OCCT's arguments of the evaluation functions (`Poles, Weights, UKnots, VKnots, UMults,
/// VMults, UDegree, VDegree, URat, VRat, UPer, VPer`). Knots are flat knots when the multiplicities are `None`.
#[derive(Debug)]
pub struct Surface<'a> {
    /// `UDegree`.
    pub u_degree: i32,
    /// `VDegree`.
    pub v_degree: i32,
    /// `URat`.
    pub u_rational: bool,
    /// `VRat`.
    pub v_rational: bool,
    /// `UPer`.
    pub u_periodic: bool,
    /// `VPer`.
    pub v_periodic: bool,
    /// The poles: `rows` in U, `cols` in V.
    pub poles: &'a Array2<Pnt>,
    /// The weights (`NULL` is `None`).
    pub weights: Option<&'a Array2<f64>>,
    /// `UKnots`.
    pub u_knots: &'a [f64],
    /// `VKnots`.
    pub v_knots: &'a [f64],
    /// `UMults`.
    pub u_mults: Option<&'a [i32]>,
    /// `VMults`.
    pub v_mults: Option<&'a [i32]>,
}

/// `BSplSLib_DataContainer::poles`: `4 * (MaxDegree + 1)^2` values.
const POLES_BUFFER: usize = 4 * 26 * 26;

/// What `PrepareEval` leaves in the data container.
pub(crate) struct Prepared {
    /// `true` when the first direction is U (`UDegree <= VDegree`)
    pub(crate) ufirst: bool,
    pub(crate) u1: f64,
    pub(crate) u2: f64,
    pub(crate) d1: i32,
    pub(crate) d2: i32,
    pub(crate) rational: bool,
    pub(crate) knots1: Vec<f64>,
    pub(crate) knots2: Vec<f64>,
    pub(crate) poles: Vec<f64>,
}

fn check_surface(s: &Surface<'_>) -> R<()> {
    if !(1..=25).contains(&s.u_degree) || !(1..=25).contains(&s.v_degree) {
        return Err(BSplError::OutOfRange);
    }
    if (s.u_rational || s.v_rational) && s.weights.is_none() {
        return Err(BSplError::OutOfRange);
    }
    Ok(())
}

fn pole_at(poles: &Array2<Pnt>, ip: isize, jp: isize) -> R<Pnt> {
    let (r, c) = (
        usize::try_from(ip - 1).map_err(|_| BSplError::OutOfRange)?,
        usize::try_from(jp - 1).map_err(|_| BSplError::OutOfRange)?,
    );
    poles.get(r, c).copied().ok_or(BSplError::OutOfRange)
}

fn weight_at(weights: Option<&Array2<f64>>, ip: isize, jp: isize) -> R<f64> {
    let weights = weights.ok_or(BSplError::OutOfRange)?;
    let (r, c) = (
        usize::try_from(ip - 1).map_err(|_| BSplError::OutOfRange)?,
        usize::try_from(jp - 1).map_err(|_| BSplError::OutOfRange)?,
    );
    weights.get(r, c).copied().ok_or(BSplError::OutOfRange)
}

/// `PrepareEval`: the local knots, the local poles (multiplied by the weights when locally rational) and the
/// parameters, the first direction being the one of smaller degree.
pub(crate) fn prepare_eval(u: f64, v: f64, uindex: i32, vindex: i32, s: &Surface<'_>) -> R<Prepared> {
    check_surface(s)?;
    let (ud, vd) = (s.u_degree, s.v_degree);
    let mut rational = s.u_rational || s.v_rational;
    let nu = to_i32(len_ix(s.u_knots))?;
    let nv = to_i32(len_ix(s.v_knots))?;

    // compute the indices
    let (uidx, upar) = if uindex < 1 || uindex > nu {
        locate_parameter_opt(ud, s.u_knots, s.u_mults, u, s.u_periodic, uindex)?
    } else {
        (uindex, u)
    };
    let (vidx, vpar) = if vindex < 1 || vindex > nv {
        locate_parameter_opt(vd, s.v_knots, s.v_mults, v, s.v_periodic, vindex)?
    } else {
        (vindex, v)
    };

    // get the knots
    let ufirst = ud <= vd;
    let ku = build_knots(ud, uidx, s.u_periodic, s.u_knots, s.u_mults)?;
    let kv = build_knots(vd, vidx, s.v_periodic, s.v_knots, s.v_mults)?;

    let up = match s.u_mults {
        None => ix(uidx) - (1 + ix(ud)),
        Some(m) => ix(pole_index(ud, uidx, s.u_periodic, m)?),
    };
    let vp = match s.v_mults {
        None => ix(vidx) - (1 + ix(vd)),
        Some(m) => ix(pole_index(vd, vidx, s.v_periodic, m)?),
    };

    // get the poles
    let rows = len_ix_us(s.poles.rows());
    let cols = len_ix_us(s.poles.cols());
    let mut buffer = zeros(POLES_BUFFER)?;

    // verify if locally non rational
    if rational {
        rational = false;
        let mut ip = 1 + up;
        let mut jp = 1 + vp;
        if ip < 1 {
            ip = rows;
        }
        if jp < 1 {
            jp = cols;
        }
        let w = weight_at(s.weights, ip, jp)?;
        let eps = epsilon(w);
        let mut i = 0;
        while i <= ud && !rational {
            jp = 1 + vp;
            if jp < 1 {
                jp = cols;
            }
            let mut j = 0;
            while j <= vd && !rational {
                let mut dw = weight_at(s.weights, ip, jp)? - w;
                if dw < 0.0 {
                    dw = -dw;
                }
                rational = dw > eps;
                jp += 1;
                if jp > cols {
                    jp = 1;
                }
                j += 1;
            }
            ip += 1;
            if ip > rows {
                ip = 1;
            }
            i += 1;
        }
    }

    // copy the poles
    let mut at = 0isize;
    let mut put = |p: &Pnt, weight: Option<f64>| -> R<()> {
        match weight {
            Some(w) => {
                wr(&mut buffer, at + 3, w)?;
                wr(&mut buffer, at, p.x() * w)?;
                wr(&mut buffer, at + 1, p.y() * w)?;
                wr(&mut buffer, at + 2, p.z() * w)?;
                at += 4;
            }
            None => {
                wr(&mut buffer, at, p.x())?;
                wr(&mut buffer, at + 1, p.y())?;
                wr(&mut buffer, at + 2, p.z())?;
                at += 3;
            }
        }
        Ok(())
    };
    if ufirst {
        let mut ip = 1 + up;
        if ip < 1 {
            ip = rows;
        }
        for _ in 0..=ud {
            let mut jp = 1 + vp;
            if jp < 1 {
                jp = cols;
            }
            for _ in 0..=vd {
                let p = pole_at(s.poles, ip, jp)?;
                if rational {
                    let w = weight_at(s.weights, ip, jp)?;
                    put(&p, Some(w))?;
                } else {
                    put(&p, None)?;
                }
                jp += 1;
                if jp > cols {
                    jp = 1;
                }
            }
            ip += 1;
            if ip > rows {
                ip = 1;
            }
        }
    } else {
        let mut jp = 1 + vp;
        if jp < 1 {
            jp = cols;
        }
        for _ in 0..=vd {
            let mut ip = 1 + up;
            if ip < 1 {
                ip = rows;
            }
            if !rational && ip > rows {
                ip = 1;
            }
            for _ in 0..=ud {
                let p = pole_at(s.poles, ip, jp)?;
                if rational {
                    let w = weight_at(s.weights, ip, jp)?;
                    put(&p, Some(w))?;
                } else {
                    put(&p, None)?;
                }
                ip += 1;
                if ip > rows {
                    ip = 1;
                }
            }
            jp += 1;
            if jp > cols {
                jp = 1;
            }
        }
    }

    Ok(if ufirst {
        Prepared { ufirst, u1: upar, u2: vpar, d1: ud, d2: vd, rational, knots1: ku, knots2: kv, poles: buffer }
    } else {
        Prepared { ufirst, u1: vpar, u2: upar, d1: vd, d2: ud, rational, knots1: kv, knots2: ku, poles: buffer }
    })
}

fn len_ix_us(n: usize) -> isize {
    n as isize
}

/// `buffer[offset ..]` as a mutable slice.
pub(crate) fn tail(buffer: &mut [f64], offset: isize) -> R<&mut [f64]> {
    usize::try_from(offset).ok().and_then(|k| buffer.get_mut(k..)).ok_or(BSplError::OutOfRange)
}

fn vec_at(buffer: &[f64], offset: isize) -> R<Vec3> {
    Ok(Vec3::new(rd(buffer, offset)?, rd(buffer, offset + 1)?, rd(buffer, offset + 2)?))
}

fn pnt_at(buffer: &[f64], offset: isize) -> R<Pnt> {
    Ok(Pnt::new(rd(buffer, offset)?, rd(buffer, offset + 1)?, rd(buffer, offset + 2)?))
}

/// `BSplSLib_zero`.
fn zero_vec() -> Vec3 {
    Vec3::new(0.0, 0.0, 0.0)
}

/// A derivative of the result array, or the null derivative of `BSplSLib_zero` (`None`).
fn vec_or_zero(buffer: &[f64], offset: Option<isize>) -> R<Vec3> {
    match offset {
        Some(offset) => vec_at(buffer, offset),
        None => Ok(zero_vec()),
    }
}

// ---------------------------------------------------------------------------------------------
// RationalDerivative

/// `BSplSLib::RationalDerivative(UDeg, VDeg, N, M, HDerivatives, RDerivatives, All)`: the rational derivatives from
/// the derivatives of the homogeneous numerator and the denominator; all `3 * (N + 1) * (M + 1)` values, or (not
/// `all`) the 3 values of the derivative `(N, M)`.
pub(crate) fn rational_derivative_core(u_deg: i32, v_deg: i32, n: i32, m: i32, h: &[f64], all: bool) -> R<Vec<f64>> {
    let (u_deg, v_deg, n, m) = (ix(u_deg), ix(v_deg), ix(n), ix(m));
    let m1 = m + 1;
    let n1 = n + 1;
    let m3 = (m1 << 1) + m1;
    let m4 = (v_deg + 1) << 2;
    let mut r = zeros(mul(n1 * m1, 3)?)?;
    let mut store_w = zeros(mul(n1, m1)?)?;

    let denominator = 1.0e0 / rd(h, 3)?;
    let mut index_u = 0isize;
    let mut index_u1 = 0isize;
    let min_n = u_deg.min(n);
    let min_m = v_deg.min(m);
    let min_n1 = min_n + 1;
    let min_m1 = min_m + 1;
    let mut ii_m1 = -m1;

    for _ in 0..min_n1 {
        ii_m1 += m1;
        let mut index_v = index_u;
        let mut index_v1 = index_u1;
        let mut index_w = ii_m1;
        for _ in 0..min_m1 {
            for _ in 0..3 {
                wr(&mut r, index_v, rd(h, index_v1)?)?;
                index_v += 1;
                index_v1 += 1;
            }
            wr(&mut store_w, index_w, rd(h, index_v1)?)?;
            index_w += 1;
            index_v1 += 1;
        }
        for _ in min_m1..m1 {
            for _ in 0..3 {
                wr(&mut r, index_v, 0.0)?;
                index_v += 1;
            }
            wr(&mut store_w, index_w, 0.0)?;
            index_w += 1;
        }
        index_u1 += m4;
        index_u += m3;
    }
    let mut index_v = min_n1 * m3;
    let mut index_w = min_n1 * m1;
    for _ in min_n1..n1 {
        for _ in 0..m1 {
            for _ in 0..3 {
                wr(&mut r, index_v, 0.0)?;
                index_v += 1;
            }
            wr(&mut store_w, index_w, 0.0)?;
            index_w += 1;
        }
    }

    // Calculation
    ii_m1 = -m1;
    let mut ii_m3 = -m3;
    for ii in 0..=n {
        ii_m1 += m1;
        ii_m3 += m3;
        let mut index1 = ii_m3 - 3;
        let mut jj_m1 = ii_m1;
        for jj in 0..=m {
            jj_m1 += 1;
            let mut pp_m1 = -m1;
            let mut pp_m3 = -m3;
            index1 += 3;
            for pp in 0..ii {
                pp_m1 += m1;
                pp_m3 += m3;
                let mut index = pp_m3;
                let mut index2 = jj_m1 - pp_m1;
                let pip = plib::bin(to_i32(ii)?, to_i32(pp)?)?;
                for qq in 0..=jj {
                    index2 -= 1;
                    let pjq = pip * plib::bin(to_i32(jj)?, to_i32(qq)?)? * rd(&store_w, index2)?;
                    for _ in 0..3 {
                        let value = rd(&r, index1)? - pjq * rd(&r, index)?;
                        wr(&mut r, index1, value)?;
                        index += 1;
                        index1 += 1;
                    }
                    index1 -= 3;
                }
            }
            let mut index = ii_m3;
            let mut index2 = jj + 1;
            let pii = plib::bin(to_i32(ii)?, to_i32(ii)?)?;
            for qq in 0..jj {
                index2 -= 1;
                let pjq = pii * plib::bin(to_i32(jj)?, to_i32(qq)?)? * rd(&store_w, index2)?;
                for _ in 0..3 {
                    let value = rd(&r, index1)? - pjq * rd(&r, index)?;
                    wr(&mut r, index1, value)?;
                    index += 1;
                    index1 += 1;
                }
                index1 -= 3;
            }
            for _ in 0..3 {
                let value = rd(&r, index1)? * denominator;
                wr(&mut r, index1, value)?;
                index1 += 1;
            }
            index1 -= 3;
        }
    }
    if all {
        Ok(r)
    } else {
        let index = (n * m1 + m) * 3;
        Ok(vec![rd(&r, index)?, rd(&r, index + 1)?, rd(&r, index + 2)?])
    }
}

/// `BSplSLib::RationalDerivative`: `ders` holds the homogeneous derivatives, `u_degree + 1` lines of
/// `(v_degree + 1) * 4` values; only the lines and columns up to `min(n, u_degree)` and `min(m, v_degree)` are
/// read. With `all` the result has `3 * (n + 1) * (m + 1)` values, otherwise the 3 values of the derivative
/// `(n, m)`.
pub fn rational_derivative(u_degree: i32, v_degree: i32, n: i32, m: i32, ders: &[f64], all: bool) -> R<Vec<f64>> {
    if n < 0 || m < 0 || u_degree < 0 || v_degree < 0 {
        return Err(BSplError::OutOfRange);
    }
    let need = i64::from(n.min(u_degree)) * (i64::from(v_degree) + 1) * 4 + i64::from(m.min(v_degree)) * 4 + 4;
    if (ders.len() as i64) < need || rd(ders, 3)? == 0.0 {
        return Err(BSplError::OutOfRange);
    }
    rational_derivative_core(u_degree, v_degree, n, m, ders, all)
}

// ---------------------------------------------------------------------------------------------
// D0 .. DN

/// `BSplSLib::HomogeneousD0`: the weight and the point (the homogeneous numerator when the surface is locally
/// rational).
pub fn homogeneous_d0(u: f64, v: f64, u_index: i32, v_index: i32, s: &Surface<'_>) -> R<(f64, Pnt)> {
    let mut pr = prepare_eval(u, v, u_index, v_index, s)?;
    let dim = if pr.rational { 4 } else { 3 };
    eval_in_place(pr.u1, pr.d1, &pr.knots1, dim * (pr.d2 + 1), &mut pr.poles)?;
    eval_in_place(pr.u2, pr.d2, &pr.knots2, dim, &mut pr.poles)?;
    let w = if pr.rational { rd(&pr.poles, 3)? } else { 1.0e0 };
    Ok((w, pnt_at(&pr.poles, 0)?))
}

/// `BSplSLib::D0`.
pub fn d0(u: f64, v: f64, u_index: i32, v_index: i32, s: &Surface<'_>) -> R<Pnt> {
    let (w, p) = homogeneous_d0(u, v, u_index, v_index, s)?;
    Ok(Pnt::new(p.x() / w, p.y() / w, p.z() / w))
}

/// The first phase of `D1`/`D2`/`D3`: the derivatives in the first direction, then in the second direction for the
/// lines of the poles that the derivative order needs.
fn bohm_rows(pr: &mut Prepared, dim: i32, dim2: i32, orders: &[(i32, bool)]) -> R<()> {
    let (u1, d1, u2, d2) = (pr.u1, pr.d1, pr.u2, pr.d2);
    let first_order = orders.first().map_or(0, |(o, _)| *o);
    bohm_in_place(u1, d1, first_order, &pr.knots1, dim2, &mut pr.poles)?;
    for (row, (order, full)) in orders.iter().enumerate() {
        let offset = ix(dim2) * row as isize;
        let buffer = tail(&mut pr.poles, offset)?;
        if *full {
            bohm_in_place(u2, d2, *order, &pr.knots2, dim, buffer)?;
        } else {
            eval_in_place(u2, d2, &pr.knots2, dim, buffer)?;
        }
    }
    Ok(())
}

/// `BSplSLib::D1`.
pub fn d1(u: f64, v: f64, u_index: i32, v_index: i32, s: &Surface<'_>) -> R<(Pnt, Vec3, Vec3)> {
    let mut pr = prepare_eval(u, v, u_index, v_index, s)?;
    let (dim, dim2) = if pr.rational { (4, (pr.d2 + 1) << 2) } else { (3, 3 * (pr.d2 + 1)) };
    // Bohm(u1, d1, 1, knots1, dim2, poles); Bohm(u2, d2, 1, knots2, dim, poles); Eval(u2, d2, knots2, dim, poles + dim2)
    bohm_rows(&mut pr, dim, dim2, &[(1, true), (0, false)])?;
    let (result, vu, vv): (Vec<f64>, isize, isize);
    if pr.rational {
        result = rational_derivative_core(pr.d1, pr.d2, 1, 1, &pr.poles, true)?;
        (vu, vv) = if pr.ufirst { (6, 3) } else { (3, 6) };
    } else {
        (vu, vv) = if pr.ufirst { (ix(dim2), 3) } else { (3, ix(dim2)) };
        result = pr.poles;
    }
    Ok((pnt_at(&result, 0)?, vec_at(&result, vu)?, vec_at(&result, vv)?))
}

/// `BSplSLib::HomogeneousD1`: the numerator `N`, its derivatives `Nu`, `Nv`, and the denominator `D` with `Du`,
/// `Dv` (1, 0, 0 for a locally polynomial surface).
pub fn homogeneous_d1(
    u: f64,
    v: f64,
    u_index: i32,
    v_index: i32,
    s: &Surface<'_>,
) -> R<(Pnt, Vec3, Vec3, f64, f64, f64)> {
    let mut pr = prepare_eval(u, v, u_index, v_index, s)?;
    let dim = if pr.rational { 4 } else { 3 };
    let dim2 = dim * (pr.d2 + 1);
    bohm_rows(&mut pr, dim, dim2, &[(1, true), (0, false)])?;
    let res_vu = if pr.ufirst { ix(dim2) } else { ix(dim) };
    let res_vv = if pr.ufirst { ix(dim) } else { ix(dim2) };
    let n = pnt_at(&pr.poles, 0)?;
    let nu = vec_at(&pr.poles, res_vu)?;
    let nv = vec_at(&pr.poles, res_vv)?;
    let (mut d, mut du, mut dv) = (1.0e0, 0.0e0, 0.0e0);
    if pr.rational {
        d = rd(&pr.poles, 3)?;
        du = rd(&pr.poles, res_vu + 3)?;
        dv = rd(&pr.poles, res_vv + 3)?;
    }
    Ok((n, nu, nv, d, du, dv))
}

/// `BSplSLib::D2`: `(P, Vu, Vv, Vuu, Vvv, Vuv)`.
pub fn d2(u: f64, v: f64, u_index: i32, v_index: i32, s: &Surface<'_>) -> R<(Pnt, Vec3, Vec3, Vec3, Vec3, Vec3)> {
    let mut pr = prepare_eval(u, v, u_index, v_index, s)?;
    let (dim, dim2) = if pr.rational { (4, (pr.d2 + 1) << 2) } else { (3, 3 * (pr.d2 + 1)) };
    // Bohm(u1, d1, 2, ...); Bohm(u2, d2, 2, poles); Bohm(u2, d2, 1, poles + dim2); if d1 > 1: Eval(poles + 2 * dim2)
    let mut orders = vec![(2, true), (1, true)];
    if pr.d1 > 1 {
        orders.push((0, false));
    }
    bohm_rows(&mut pr, dim, dim2, &orders)?;
    let (ud, vd) = (s.u_degree, s.v_degree);
    let d2i = ix(pr.d2);
    let dim2i = ix(dim2);
    let (src, p, vu, vv, vuu, vvv, vuv);
    if pr.rational {
        src = rational_derivative_core(pr.d1, pr.d2, 2, 2, &pr.poles, true)?;
        p = 0;
        (vu, vv, vuu, vvv) =
            if pr.ufirst { (Some(9), Some(3), Some(18), Some(6)) } else { (Some(3), Some(9), Some(6), Some(18)) };
        vuv = Some(12);
    } else {
        p = 0;
        vuv = Some(d2i * 3 + 6);
        if pr.ufirst {
            vu = Some(dim2i);
            vv = Some(3);
            vuu = if ud <= 1 { None } else { Some(dim2i << 1) };
            vvv = if vd <= 1 { None } else { Some(6) };
        } else {
            vu = Some(3);
            vv = Some(dim2i);
            vuu = if ud <= 1 { None } else { Some(6) };
            vvv = if vd <= 1 { None } else { Some(dim2i << 1) };
        }
        src = pr.poles;
    }
    Ok((
        pnt_at(&src, p)?,
        vec_or_zero(&src, vu)?,
        vec_or_zero(&src, vv)?,
        vec_or_zero(&src, vuu)?,
        vec_or_zero(&src, vvv)?,
        vec_or_zero(&src, vuv)?,
    ))
}

/// The result of `D3`: `(P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)`.
pub type D3Result = (Pnt, Vec3, Vec3, Vec3, Vec3, Vec3, Vec3, Vec3, Vec3, Vec3);

/// `BSplSLib::D3`: `(P, Vu, Vv, Vuu, Vvv, Vuv, Vuuu, Vvvv, Vuuv, Vuvv)`.
pub fn d3(u: f64, v: f64, u_index: i32, v_index: i32, s: &Surface<'_>) -> R<D3Result> {
    let mut pr = prepare_eval(u, v, u_index, v_index, s)?;
    let (dim, dim2) = if pr.rational { (4, (pr.d2 + 1) << 2) } else { (3, 3 * (pr.d2 + 1)) };
    // Bohm(u1, d1, 3, ...); Bohm(u2, d2, 3, poles); Bohm(.., 2, poles + dim2); d1 > 1: Bohm(.., 1, poles + 2 dim2);
    // d1 > 2: Eval(poles + 3 dim2)
    let mut orders = vec![(3, true), (2, true)];
    if pr.d1 > 1 {
        orders.push((1, true));
    }
    if pr.d1 > 2 {
        orders.push((0, false));
    }
    bohm_rows(&mut pr, dim, dim2, &orders)?;
    let (ud, vd) = (s.u_degree, s.v_degree);
    let d2i = ix(pr.d2);
    let dim2i = ix(dim2);
    let src;
    let (vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv);
    if pr.rational {
        src = rational_derivative_core(pr.d1, pr.d2, 3, 3, &pr.poles, true)?;
        if pr.ufirst {
            (vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) =
                (Some(12), Some(3), Some(24), Some(6), Some(15), Some(36), Some(9), Some(27), Some(18));
        } else {
            (vu, vv, vuu, vvv, vuv, vuuu, vvvv, vuuv, vuvv) =
                (Some(3), Some(12), Some(6), Some(24), Some(15), Some(9), Some(36), Some(18), Some(27));
        }
    } else {
        vuv = Some(d2i * 3 + 6);
        if pr.ufirst {
            vu = Some(dim2i);
            vv = Some(3);
            (vuu, vuuv) = if ud <= 1 { (None, None) } else { (Some(dim2i << 1), Some((dim2i << 1) + 3)) };
            (vvv, vuvv) = if vd <= 1 { (None, None) } else { (Some(6), Some(dim2i + 6)) };
            vuuu = if ud <= 2 { None } else { Some((dim2i << 1) + dim2i) };
            vvvv = if vd <= 2 { None } else { Some(9) };
        } else {
            vu = Some(3);
            vv = Some(dim2i);
            (vuu, vuuv) = if ud <= 1 { (None, None) } else { (Some(6), Some(dim2i + 6)) };
            (vvv, vuvv) = if vd <= 1 { (None, None) } else { (Some(dim2i << 1), Some((dim2i << 1) + 3)) };
            vuuu = if ud <= 2 { None } else { Some(9) };
            vvvv = if vd <= 2 { None } else { Some((dim2i << 1) + dim2i) };
        }
        src = pr.poles;
    }
    Ok((
        pnt_at(&src, 0)?,
        vec_or_zero(&src, vu)?,
        vec_or_zero(&src, vv)?,
        vec_or_zero(&src, vuu)?,
        vec_or_zero(&src, vvv)?,
        vec_or_zero(&src, vuv)?,
        vec_or_zero(&src, vuuu)?,
        vec_or_zero(&src, vvvv)?,
        vec_or_zero(&src, vuuv)?,
        vec_or_zero(&src, vuvv)?,
    ))
}

/// `BSplSLib::DN`: the derivative of order (`nu`, `nv`).
pub fn dn(u: f64, v: f64, nu: i32, nv: i32, u_index: i32, v_index: i32, s: &Surface<'_>) -> R<Vec3> {
    if nu < 0 || nv < 0 {
        return Err(BSplError::OutOfRange);
    }
    let mut pr = prepare_eval(u, v, u_index, v_index, s)?;
    let dim = if pr.rational { 4 } else { 3 };

    if !pr.rational && (nu > s.u_degree || nv > s.v_degree) {
        return Ok(zero_vec());
    }

    let n1 = if pr.ufirst { nu } else { nv };
    let n2 = if pr.ufirst { nv } else { nu };

    let (u1, d1, u2, d2) = (pr.u1, pr.d1, pr.u2, pr.d2);
    bohm_in_place(u1, d1, n1, &pr.knots1, dim * (d2 + 1), &mut pr.poles)?;
    for k in 0..=n1.min(d1) {
        let offset = ix(k) * ix(dim) * (ix(d2) + 1);
        let buffer = tail(&mut pr.poles, offset)?;
        bohm_in_place(u2, d2, n2, &pr.knots2, dim, buffer)?;
    }

    if pr.rational {
        let result = rational_derivative_core(d1, d2, n1, n2, &pr.poles, false)?;
        vec_at(&result, 0)
    } else {
        vec_at(&pr.poles, (ix(n1) * (ix(d2) + 1) + ix(n2)) * ix(dim))
    }
}
