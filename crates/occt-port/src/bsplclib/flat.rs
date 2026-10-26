//! The flat-array functions of `BSplCLib` that `BSplSLib` needs: `Eval` and `Bohm` on raw buffers, the local knots,
//! the pole index, the knot location and the flat knot sequences, plus the constants.
//!
//! Arrays have lower bound 1 as in OCCT (`knots[k - 1]` is `Knots(k)`); every index argument keeps the OCCT meaning.
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use super::BSplError;
use super::util::{R, count, epsilon, filled, ix, len_ix, rd, rd1, to_i32, wr, wr1};
use crate::elclib;

/// `BSplCLib::MaxDegree()`.
pub fn max_degree() -> i32 {
    25
}

/// `BSplCLib::MaxUnitWeightsSize()`.
pub fn max_unit_weights_size() -> i32 {
    2049
}

static UNIT_WEIGHTS: [f64; 2049] = [1.0; 2049];

/// `BSplCLib::UnitWeightsData()`: `max_unit_weights_size()` ones.
pub fn unit_weights_data() -> &'static [f64] {
    &UNIT_WEIGHTS
}

/// `BSplCLib::NoMults()`: the null multiplicities (the knots are flat knots).
pub fn no_mults() -> Option<&'static [i32]> {
    None
}

// ---------------------------------------------------------------------------------------------
// Eval and Bohm on raw buffers

/// `BSplCLib::Eval(U, Degree, Knots, Dimension, Poles)` in place.
pub(crate) fn eval_in_place(u: f64, degree: i32, knots: &[f64], dimension: i32, poles: &mut [f64]) -> R<()> {
    if degree < 0 || dimension < 1 {
        return Err(BSplError::OutOfRange);
    }
    let dim = ix(dimension);
    let dm1 = ix(degree) - 1;
    let mut dms = ix(degree) + 1;
    let mut step = -1isize;
    while step < dm1 {
        dms -= 1;
        let mut base = 0isize;
        let mut dpi = dm1;
        let mut sti = step;
        for _ in 0..dms {
            dpi += 1;
            sti += 1;
            let kd = rd(knots, dpi)?;
            let ks = rd(knots, sti)?;
            let x = (kd - u) / (kd - ks);
            let y = 1.0 - x;
            for k in 0..dim {
                let a = rd(poles, base + k)? * x;
                let b = rd(poles, base + k + dim)?;
                wr(poles, base + k, a + y * b)?;
            }
            base += dim;
        }
        step += 1;
    }
    Ok(())
}

/// `BSplCLib::Bohm(U, Degree, N, Knots, Dimension, Poles)` in place.
pub(crate) fn bohm_in_place(u: f64, degree: i32, n: i32, knots: &[f64], dimension: i32, poles: &mut [f64]) -> R<()> {
    if degree < 0 || dimension < 1 || n < 0 {
        return Err(BSplError::OutOfRange);
    }
    let dim = ix(dimension);
    let deg = ix(degree);
    let min = ix(n.min(degree));
    let degm1 = deg - 1;
    let psdd = deg * dim;
    let psdd_m_dim = psdd - dim;
    let mut ddmi = (deg << 1) + 1;
    // First phase, independent of U: the poles of the derivatives
    for i in 0..deg {
        ddmi -= 1;
        let mut pole = psdd;
        let mut tbis = psdd_m_dim;
        let mut jdmi = ddmi;
        let mut j = degm1;
        while j >= i {
            jdmi -= 1;
            let kj = rd(knots, jdmi)?;
            let kk = rd(knots, j)?;
            if dim == 1 {
                // Dimension 1 divides by the knot difference
                let value = rd(poles, pole)? - rd(poles, tbis)?;
                wr(poles, pole, if kj == kk { 0.0 } else { value / (kj - kk) })?;
            } else {
                let coef = if kj == kk { 0.0 } else { 1.0 / (kj - kk) };
                for k in 0..dim {
                    let value = rd(poles, pole + k)? - rd(poles, tbis + k)?;
                    wr(poles, pole + k, value * coef)?;
                }
            }
            pole -= dim;
            tbis -= dim;
            j -= 1;
        }
    }
    // Second phase, dependant of U
    let mut i_dim = -dim;
    for i in 0..deg {
        i_dim += dim;
        let mut pole = i_dim;
        let mut tbis = pole + dim;
        let coef = u - rd(knots, i)?;
        let mut j = i;
        while j >= 0 {
            for k in 0..dim {
                let value = rd(poles, pole + k)? + coef * rd(poles, tbis + k)?;
                wr(poles, pole + k, value)?;
            }
            pole -= dim;
            tbis -= dim;
            j -= 1;
        }
    }
    // multiply by the degrees
    let mut coef = deg as f64;
    let mut dmi = deg;
    let mut pole = dim;
    for _ in 1..=min {
        for k in 0..dim {
            let value = rd(poles, pole + k)? * coef;
            wr(poles, pole + k, value)?;
        }
        pole += dim;
        dmi -= 1;
        coef *= dmi as f64;
    }
    Ok(())
}

fn check_buffer(degree: i32, knots: &[f64], dimension: i32, poles: &[f64]) -> R<()> {
    if degree < 0 || dimension < 1 || knots.is_empty() {
        return Err(BSplError::OutOfRange);
    }
    let need_knots = 2 * i64::from(degree);
    let need_poles = (i64::from(degree) + 1) * i64::from(dimension);
    if (knots.len() as i64) < need_knots || (poles.len() as i64) < need_poles {
        return Err(BSplError::OutOfRange);
    }
    Ok(())
}

/// `BSplCLib::Eval(U, Degree, Knots, Dimension, Poles)`: the de Boor evaluation on the `degree + 1` poles of
/// `dimension` values in `poles`; returns the whole modified buffer (the point is its first `dimension` values).
pub fn eval_buffer(u: f64, degree: i32, knots: &[f64], dimension: i32, poles: &[f64]) -> R<Vec<f64>> {
    check_buffer(degree, knots, dimension, poles)?;
    let mut buffer = poles.to_vec();
    eval_in_place(u, degree, knots, dimension, &mut buffer)?;
    Ok(buffer)
}

/// `BSplCLib::Bohm(U, Degree, N, Knots, Dimension, Poles)`: the derivatives up to order `n` (entry `k` holds the
/// derivative of order `k`); returns the whole modified buffer.
pub fn bohm_buffer(u: f64, degree: i32, n: i32, knots: &[f64], dimension: i32, poles: &[f64]) -> R<Vec<f64>> {
    check_buffer(degree, knots, dimension, poles)?;
    if n < 0 {
        return Err(BSplError::OutOfRange);
    }
    let mut buffer = poles.to_vec();
    bohm_in_place(u, degree, n, knots, dimension, &mut buffer)?;
    Ok(buffer)
}

// ---------------------------------------------------------------------------------------------
// Local knots and pole indices

/// `BSplCLib::BuildKnots`: the `2 * degree` knots around the span `index` (1-based). With `mults == None` the knots
/// are flat knots.
pub fn build_knots(degree: i32, index: i32, periodic: bool, knots: &[f64], mults: Option<&[i32]>) -> R<Vec<f64>> {
    if degree < 1 {
        return Err(BSplError::OutOfRange);
    }
    let deg = ix(degree);
    let mut out: Vec<Option<f64>> = filled(count(degree)?.checked_mul(2).ok_or(BSplError::OutOfRange)?, None)?;
    match mults {
        None => {
            let mut j = ix(index) - deg;
            for i in 0..2 * deg {
                j += 1;
                wr(&mut out, i, Some(rd1(knots, j)?))?;
            }
        }
        Some(mults) => {
            let ku = len_ix(knots);
            let mu = len_ix(mults);
            if ku != mu {
                return Err(BSplError::OutOfRange);
            }
            let deg1 = deg - 1;
            let mut dknot = 0.0;
            let mut ilow = ix(index);
            let mut mlow = 0isize;
            let mut iupp = ilow + 1;
            let mut mupp = 0isize;
            let mut loffset = 0.0;
            let mut uoffset = 0.0;
            let mut getlow = true;
            let mut getupp = true;
            if periodic {
                dknot = rd1(knots, ku)? - rd1(knots, 1)?;
                if iupp > mu {
                    iupp = 2;
                    uoffset = dknot;
                }
            }
            // Find the knots around Index
            for i in 0..deg {
                if getlow {
                    mlow += 1;
                    if mlow > ix(rd1(mults, ilow)?) {
                        mlow = 1;
                        ilow -= 1;
                        getlow = ilow >= 1;
                        if periodic && !getlow {
                            ilow = mu - 1;
                            loffset = dknot;
                            getlow = true;
                        }
                    }
                    if getlow {
                        wr(&mut out, deg1 - i, Some(rd1(knots, ilow)? - loffset))?;
                    }
                }
                if getupp {
                    mupp += 1;
                    if mupp > ix(rd1(mults, iupp)?) {
                        mupp = 1;
                        iupp += 1;
                        getupp = iupp <= mu;
                        if periodic && !getupp {
                            iupp = 2;
                            uoffset = dknot;
                            getupp = true;
                        }
                    }
                    if getupp {
                        wr(&mut out, deg + i, Some(rd1(knots, iupp)? + uoffset))?;
                    }
                }
            }
        }
    }
    // A knot the C++ leaves unwritten would be read uninitialised
    out.into_iter().map(|k| k.ok_or(BSplError::OutOfRange)).collect()
}

/// `BSplCLib::PoleIndex`: the index (0-based, OCCT's `int` arithmetic) of the first pole of the span `index`.
pub fn pole_index(degree: i32, index: i32, periodic: bool, mults: &[i32]) -> R<i32> {
    let first = *mults.first().ok_or(BSplError::OutOfRange)?;
    let mut pindex = 0i64;
    for i in 1..=ix(index) {
        pindex += i64::from(rd1(mults, i)?);
    }
    if periodic {
        pindex -= i64::from(first);
    } else {
        pindex -= i64::from(degree) + 1;
    }
    i32::try_from(pindex).map_err(|_| BSplError::OutOfRange)
}

/// `BSplCLib::NbPoles` (0 for an invalid structure).
pub(crate) fn nb_poles(degree: i32, periodic: bool, mults: &[i32]) -> R<i32> {
    let mf = *mults.first().ok_or(BSplError::OutOfRange)?;
    let ml = *mults.last().ok_or(BSplError::OutOfRange)?;
    if mf <= 0 || ml <= 0 {
        return Ok(0);
    }
    let mut sigma = if periodic {
        if mf > degree || ml > degree || mf != ml {
            return Ok(0);
        }
        i64::from(mf)
    } else {
        let deg1 = i64::from(degree) + 1;
        if i64::from(mf) > deg1 || i64::from(ml) > deg1 {
            return Ok(0);
        }
        i64::from(mf) + i64::from(ml) - deg1
    };
    let n = len_ix(mults);
    for i in 1..n - 1 {
        let m = rd(mults, i)?;
        if m <= 0 || m > degree {
            return Ok(0);
        }
        sigma += i64::from(m);
    }
    i32::try_from(sigma).map_err(|_| BSplError::OutOfRange)
}

/// `BSplCLib::FirstUKnotIndex`.
pub(crate) fn first_u_knot_index(degree: i32, mults: &[i32]) -> R<i32> {
    let mut index = 1isize;
    let mut sigma = i64::from(rd1(mults, index)?);
    while sigma <= i64::from(degree) {
        index += 1;
        sigma += i64::from(rd1(mults, index)?);
    }
    to_i32(index)
}

/// `BSplCLib::LastUKnotIndex`.
pub(crate) fn last_u_knot_index(degree: i32, mults: &[i32]) -> R<i32> {
    let mut index = len_ix(mults);
    let mut sigma = i64::from(rd1(mults, index)?);
    while sigma <= i64::from(degree) {
        index -= 1;
        sigma += i64::from(rd1(mults, index)?);
    }
    to_i32(index)
}

// ---------------------------------------------------------------------------------------------
// Locating a parameter

/// `BSplCLib::Hunt`: the position of `x` in the sorted `array` (0 below it, `len + 1` above it).
pub(crate) fn hunt(array: &[f64], x: f64) -> R<i32> {
    let first = *array.first().ok_or(BSplError::OutOfRange)?;
    let last = *array.last().ok_or(BSplError::OutOfRange)?;
    if first > x {
        return Ok(0);
    }
    if last < x {
        return to_i32(len_ix(array) + 1);
    }
    let mut pos = 1isize;
    if array.len() <= 1 {
        return to_i32(pos);
    }
    let mut hi = len_ix(array);
    while hi - pos != 1 {
        let mid = (hi + pos) / 2;
        if rd1(array, mid)? < x {
            pos = mid;
        } else {
            hi = mid;
        }
    }
    to_i32(pos)
}

/// The general `BSplCLib::LocateParameter(Knots, U, IsPeriodic, FromK1, ToK2, KnotIndex, NewU, UFirst, ULast)`.
pub(crate) fn locate_core(
    knots: &[f64],
    u: f64,
    periodic: bool,
    from_k1: i32,
    to_k2: i32,
    u_first: f64,
    u_last: f64,
) -> R<(i32, f64)> {
    let (first, last) = if from_k1 < to_k2 { (from_k1, to_k2) } else { (to_k2, from_k1) };
    let last1 = last - 1;
    let mut new_u = u;
    if periodic && (new_u < u_first || new_u > u_last) {
        new_u = elclib::in_period(new_u, u_first, u_last);
    }
    let mut knot_index = hunt(knots, new_u)?;
    let upper = to_i32(len_ix(knots))?;
    let eps = epsilon(rd1(knots, ix(upper))?.abs().min(u.abs()));
    if knot_index < upper {
        let val = (new_u - rd1(knots, ix(knot_index) + 1)?).abs();
        // <= to be coherent with Segment where Eps corresponds to a bit of error.
        if val <= eps {
            knot_index += 1;
        }
    }
    if knot_index < first {
        knot_index = first;
    }
    if knot_index > last1 {
        knot_index = last1;
    }
    if knot_index != last1 {
        let mut k1 = rd1(knots, ix(knot_index))?;
        let mut k2 = rd1(knots, ix(knot_index) + 1)?;
        let mut val = (k2 - k1).abs();
        while val <= eps {
            knot_index += 1;
            if knot_index >= upper {
                break;
            }
            k1 = k2;
            k2 = rd1(knots, ix(knot_index) + 1)?;
            val = (k2 - k1).abs();
        }
    }
    Ok((knot_index, new_u))
}

/// `BSplCLib::LocateParameter(Degree, Knots, Mults*, U, Periodic, KnotIndex, NewU)` with the in/out `KnotIndex`
/// given on entry: a usable index is kept (and `NewU = U`), otherwise the parameter is located.
pub(crate) fn locate_parameter_opt(
    degree: i32,
    knots: &[f64],
    mults: Option<&[i32]>,
    u: f64,
    periodic: bool,
    knot_index: i32,
) -> R<(i32, f64)> {
    let n = to_i32(len_ix(knots))?;
    let (first, last) = match mults {
        Some(m) => {
            if periodic {
                (1, n)
            } else {
                (first_u_knot_index(degree, m)?, last_u_knot_index(degree, m)?)
            }
        }
        None => (degree.saturating_add(1), n.saturating_sub(degree)),
    };
    if knot_index < first || knot_index > last {
        locate_core(knots, u, periodic, first, last, rd1(knots, ix(first))?, rd1(knots, ix(last))?)
    } else {
        Ok((knot_index, u))
    }
}

/// `BSplCLib::LocateParameter(Degree, Knots, NULL, U, Periodic, KnotIndex, NewU)` for flat knots: `knot_index` is the
/// in/out `KnotIndex` on entry.
pub fn locate_parameter_flat(degree: i32, knots: &[f64], u: f64, periodic: bool, knot_index: i32) -> R<(i32, f64)> {
    if degree < 1 || (knots.len() as i64) < 2 * i64::from(degree) + 2 {
        return Err(BSplError::OutOfRange);
    }
    locate_parameter_opt(degree, knots, None, u, periodic, knot_index)
}

/// `BSplCLib::LocateParameter(Degree, Knots, U, Periodic, FromK1, ToK2, KnotIndex, NewU)`; equal bounds are unchecked
/// (`Err`) here, see [`locate_parameter_range_lenient`].
pub fn locate_parameter_range(
    degree: i32,
    knots: &[f64],
    u: f64,
    periodic: bool,
    from_k1: i32,
    to_k2: i32,
) -> R<(i32, f64)> {
    if from_k1 == to_k2 {
        return Err(BSplError::OutOfRange);
    }
    locate_parameter_range_lenient(degree, knots, u, periodic, from_k1, to_k2)
}

/// The same with equal bounds allowed (`EvalBsplineBasis` of a surface direction with a single parameter).
pub(crate) fn locate_parameter_range_lenient(
    degree: i32,
    knots: &[f64],
    u: f64,
    periodic: bool,
    from_k1: i32,
    to_k2: i32,
) -> R<(i32, f64)> {
    let n = len_ix(knots);
    if degree < 0 || n < ix(degree) + 1 || !(1..=n).contains(&ix(from_k1)) || !(1..=n).contains(&ix(to_k2)) {
        return Err(BSplError::OutOfRange);
    }
    if periodic {
        let u_first = rd1(knots, 1 + ix(degree))?;
        let u_last = rd1(knots, n - ix(degree))?;
        locate_core(knots, u, true, from_k1, to_k2, u_first, u_last)
    } else {
        locate_core(knots, u, false, from_k1, to_k2, 0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------------------------
// Flat knot sequences

/// `BSplCLib::KnotSequenceLength`.
pub fn knot_sequence_length(mults: &[i32], degree: i32, periodic: bool) -> R<i32> {
    let first = *mults.first().ok_or(BSplError::OutOfRange)?;
    let mut l: i64 = mults.iter().map(|m| i64::from(*m)).sum();
    if periodic {
        l += 2 * (i64::from(degree) + 1 - i64::from(first));
    }
    i32::try_from(l).map_err(|_| BSplError::OutOfRange)
}

/// `BSplCLib::KnotSequence(Knots, Mults, Degree, Periodic, KnotSeq)`: the flat knots (`knot_sequence_length` values).
pub fn knot_sequence(knots: &[f64], mults: &[i32], degree: i32, periodic: bool) -> R<Vec<f64>> {
    if knots.is_empty() || knots.len() != mults.len() || degree < 1 {
        return Err(BSplError::OutOfRange);
    }
    let first_mult = rd1(mults, 1)?;
    if periodic && (first_mult != rd1(mults, len_ix(mults))? || i64::from(degree) + 1 - i64::from(first_mult) < 0) {
        return Err(BSplError::OutOfRange);
    }
    let length = count(knot_sequence_length(mults, degree, periodic)?)?;
    let mut seq = filled(length, 0.0)?;
    let k_upper = len_ix(knots);
    let m1 = ix(degree) + 1 - ix(first_mult);
    let mut index = if periodic { m1 + 1 } else { 1 };
    for i in 1..=k_upper {
        let mult = rd1(mults, i)?;
        let k = rd1(knots, i)?;
        for _ in 1..=mult {
            wr1(&mut seq, index, k)?;
            index += 1;
        }
    }
    if periodic {
        let period = rd1(knots, k_upper)? - rd1(knots, 1)?;
        let mut m = 1isize;
        let mut j = k_upper - 1;
        let mut i = m1;
        while i >= 1 {
            wr1(&mut seq, i, rd1(knots, j)? - period)?;
            m += 1;
            if m > ix(rd1(mults, j)?) {
                j -= 1;
                m = 1;
            }
            i -= 1;
        }
        m = 1;
        j = 2;
        let mut i = index;
        while i <= len_ix(&seq) {
            wr1(&mut seq, i, rd1(knots, j)? + period)?;
            m += 1;
            if m > ix(rd1(mults, j)?) {
                j += 1;
                m = 1;
            }
            i += 1;
        }
    }
    Ok(seq)
}
