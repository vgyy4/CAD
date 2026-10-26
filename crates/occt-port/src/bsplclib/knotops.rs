//! The knot operations of `BSplCLib` on flat pole arrays: `PrepareInsertKnots`/`InsertKnots`, `RemoveKnot`,
//! `IncreaseDegreeCountKnots`/`IncreaseDegree`, `PrepareUnperiodize`/`Unperiodize` and the Boor scheme helpers.
//!
//! Poles are flat (`dimension` values per pole, pole after pole). Every array has lower bound 1 as in OCCT; the
//! arrays of the C++ that alias a part of another (`NCollection_Array1(Item, Lower, Upper)`) are sub-slices.
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use super::BSplError;
use super::flat::{build_knots, first_u_knot_index, last_u_knot_index, locate_core, nb_poles, pole_index};
use super::util::{R, count, epsilon, filled, ix, len_ix, mul, rd, rd1, to_i32, wr, wr1, zeros};
use crate::gp;

/// The new poles, knots and multiplicities of a knot operation.
pub(crate) type CurveOut = (Vec<f64>, Vec<f64>, Vec<i32>);

// ---------------------------------------------------------------------------------------------
// Validation of the inputs (what the golden file contains)

/// A B-spline direction that OCCT can work with: degree >= 1, as many knots as multiplicities (at least two),
/// strictly increasing knots, multiplicities `1..=degree + 1`, at least two poles and a usable parameter range.
pub(crate) fn check_structure(degree: i32, periodic: bool, knots: &[f64], mults: &[i32]) -> R<()> {
    if degree < 1 || knots.len() != mults.len() || knots.len() < 2 {
        return Err(BSplError::OutOfRange);
    }
    if mults.iter().any(|m| *m < 1 || i64::from(*m) > i64::from(degree) + 1) {
        return Err(BSplError::OutOfRange);
    }
    if knots.windows(2).any(|w| !matches!((w.first(), w.get(1)), (Some(a), Some(b)) if b > a)) {
        return Err(BSplError::OutOfRange);
    }
    let np = nb_poles(degree, periodic, mults)?;
    if np < 2 {
        return Err(BSplError::OutOfRange);
    }
    if periodic {
        if np <= degree {
            return Err(BSplError::OutOfRange);
        }
    } else {
        let sum: i64 = mults.iter().map(|m| i64::from(*m)).sum();
        if sum < i64::from(degree) + 1 {
            return Err(BSplError::OutOfRange);
        }
        let first = first_u_knot_index(degree, mults)?;
        let last = last_u_knot_index(degree, mults)?;
        if first < 1 || ix(last) > len_ix(knots) || first > last {
            return Err(BSplError::OutOfRange);
        }
    }
    Ok(())
}

fn check_poles_len(degree: i32, periodic: bool, dimension: i32, mults: &[i32], poles: &[f64]) -> R<()> {
    if dimension < 1 {
        return Err(BSplError::OutOfRange);
    }
    let expected = i64::from(nb_poles(degree, periodic, mults)?) * i64::from(dimension);
    if poles.len() as i64 != expected {
        return Err(BSplError::OutOfRange);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The Boor scheme

/// `BSplCLib::BoorIndex`.
pub(crate) fn boor_index(index: isize, length: isize, depth: isize) -> isize {
    if index <= depth {
        return index;
    }
    if index <= length {
        return 2 * index - depth;
    }
    length + index - depth
}

/// `BSplCLib::BuildBoor`: `length + 1` poles of the view `poles` (lower bound 1, wrapping at its end) from the
/// pole `index` (0-based), spaced `2 * dimension` apart in `lp`.
pub(crate) fn build_boor(index: isize, length: isize, dimension: isize, poles: &[f64], lp: &mut [f64]) -> R<()> {
    let upper = len_ix(poles);
    let mut pole = 0isize;
    let mut ip = 1 + index * dimension;
    for _ in 0..length + 1 {
        for k in 0..dimension {
            wr(lp, pole + k, rd1(poles, ip)?)?;
            ip += 1;
            if ip > upper {
                ip = 1;
            }
        }
        pole += 2 * dimension;
    }
    Ok(())
}

/// `BSplCLib::BoorScheme`.
pub(crate) fn boor_scheme(
    u: f64,
    degree: isize,
    knots: &[f64],
    dimension: isize,
    poles: &mut [f64],
    depth: isize,
    length: isize,
) -> R<()> {
    let mut first_pole = -2 * dimension;
    for step in 0..depth {
        first_pole += dimension;
        let mut pole = first_pole;
        for i in step..length {
            pole += 2 * dimension;
            let kd = rd(knots, i + degree - step)?;
            let x = (kd - u) / (kd - rd(knots, i)?);
            let y = 1.0 - x;
            for k in 0..dimension {
                let value = x * rd(poles, pole + k - dimension)? + y * rd(poles, pole + k + dimension)?;
                wr(poles, pole + k, value)?;
            }
        }
    }
    Ok(())
}

/// `BSplCLib::AntiBoorScheme`.
pub(crate) fn anti_boor_scheme(
    u: f64,
    degree: isize,
    knots: &[f64],
    dimension: isize,
    poles: &mut [f64],
    depth: isize,
    length: isize,
    tolerance: f64,
) -> R<bool> {
    // do the Boor scheme reverted
    let mut first_pole = (depth - 1) * dimension;

    // Test the special case length = 1: only verification of the central point
    if length == 1 {
        let kd = rd(knots, degree)?;
        let x = (kd - u) / (kd - rd(knots, 0)?);
        let y = 1.0 - x;
        for k in 0..dimension {
            let z = x * rd(poles, first_pole + k)? + y * rd(poles, first_pole + k + 2 * dimension)?;
            if (z - rd(poles, first_pole + k + dimension)?).abs() > tolerance {
                return Ok(false);
            }
        }
        return Ok(true);
    }

    // General case: the steps of recursion
    let mut step = depth - 1;
    while step >= 0 {
        first_pole -= dimension;
        let mut pole = first_pole;

        // first step from left to right
        for i in step..length - 1 {
            pole += 2 * dimension;
            let kd = rd(knots, i + degree - step)?;
            let x = (kd - u) / (kd - rd(knots, i)?);
            let y = 1.0 - x;
            for k in 0..dimension {
                let value = (rd(poles, pole + k)? - x * rd(poles, pole + k - dimension)?) / y;
                wr(poles, pole + k + dimension, value)?;
            }
        }

        // second step from right to left
        pole += 4 * dimension;
        let half_length = (length - 1 + step) / 2;
        // only do half of the way from right to left otherwise it start degenerating because of overflows
        let mut i = length - 1;
        while i > half_length {
            pole -= 2 * dimension;
            // coefficient
            let kd = rd(knots, i + degree - step)?;
            let x = (kd - u) / (kd - rd(knots, i)?);
            let y = 1.0 - x;
            for k in 0..dimension {
                let z = (rd(poles, pole + k)? - y * rd(poles, pole + k + dimension)?) / x;
                if (z - rd(poles, pole + k - dimension)?).abs() > tolerance {
                    return Ok(false);
                }
                let mut value = rd(poles, pole + k - dimension)?;
                value += z;
                value /= 2.0;
                wr(poles, pole + k - dimension, value)?;
            }
            i -= 1;
        }
        step -= 1;
    }
    Ok(true)
}

/// `BSplCLib::GetPole`: `dimension` values of the Boor scheme pole `index` to `pole` at `position`, which wraps to 1
/// after the pole when it passes the end of `pole`.
pub(crate) fn get_pole(
    index: isize,
    length: isize,
    depth: isize,
    dimension: isize,
    lp: &[f64],
    position: &mut isize,
    pole: &mut [f64],
) -> R<()> {
    let base = boor_index(index, length, depth) * dimension;
    for k in 0..dimension {
        wr1(pole, *position, rd(lp, base + k)?)?;
        *position += 1;
    }
    if *position > len_ix(pole) {
        *position = 1;
    }
    Ok(())
}

/// The static `Copy` of `BSplCLib.cxx`: `nb_poles` values from `old` at `old_first` to `new` at `new_first`, both
/// wrapping, the positions are updated.
fn copy_poles(nb: isize, old_first: &mut isize, old: &[f64], new_first: &mut isize, new: &mut [f64]) -> R<()> {
    let old_len = len_ix(old);
    let new_len = len_ix(new);
    if old_len == 0 || new_len == 0 {
        return Err(BSplError::OutOfRange);
    }
    // reset the index in the range for periodicity
    *old_first = 1 + (*old_first - 1) % old_len;
    *new_first = 1 + (*new_first - 1) % new_len;
    for _ in 1..=nb {
        wr1(new, *new_first, rd1(old, *old_first)?)?;
        *old_first += 1;
        if *old_first > old_len {
            *old_first = 1;
        }
        *new_first += 1;
        if *new_first > new_len {
            *new_first = 1;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Knot insertion

/// The body of `BSplCLib::PrepareInsertKnots`; `None` is the C++ `false`.
fn prepare_insert_core(
    degree: i32,
    periodic: bool,
    knots: &[f64],
    mults: &[i32],
    add_knots: &[f64],
    add_mults: Option<&[i32]>,
    tolerance: f64,
    add: bool,
) -> R<Option<(i32, i32)>> {
    let add_flat = add_mults.is_none();
    let nk = len_ix(knots);
    let na = len_ix(add_knots);
    let (first, last) = if periodic {
        (1, nk)
    } else {
        (ix(first_u_knot_index(degree, mults)?), ix(last_u_knot_index(degree, mults)?))
    };
    let delta_k1 = rd1(knots, first)? - rd1(add_knots, 1)?;
    let delta_k2 = rd1(add_knots, na)? - rd1(knots, last)?;
    if delta_k1 > tolerance {
        return Ok(None);
    }
    if delta_k2 > tolerance {
        return Ok(None);
    }

    let mut sigma = 0i64;
    let mut nb_knots = 0i64;
    let mut k = 0isize;
    let mut ak = 1isize;

    if periodic && na > 1 {
        // for the case when segments was produced on full period only one knot was added in the end of curve
        if delta_k1.abs() <= gp::resolution() && delta_k2.abs() <= gp::resolution() {
            ak += 1;
        }
    }

    let mut last_knot_mult = i64::from(rd1(mults, nk)?);
    let mut old_au = rd1(add_knots, ak)?;

    while ak <= na {
        let au = rd1(add_knots, ak)?;
        if au < old_au {
            return Ok(None);
        }
        old_au = au;

        let eps = tolerance.max(epsilon(au));

        while k < nk && rd1(knots, k + 1)? - au <= eps {
            k += 1;
            nb_knots += 1;
            sigma += i64::from(rd1(mults, k)?);
        }

        let mut amult = if add_flat { 1i64 } else { i64::from(rd1(add_mults.unwrap_or(&[]), ak)?).max(0) };

        while ak < na && (au - rd1(add_knots, ak + 1)?).abs() <= eps {
            ak += 1;
            if add {
                if add_flat {
                    amult += 1;
                } else {
                    amult += i64::from(rd1(add_mults.unwrap_or(&[]), ak)?).max(0);
                }
            }
        }

        if (au - rd1(knots, k)?).abs() <= eps {
            // identic to existing knot
            let mult = i64::from(rd1(mults, k)?);
            if add {
                if mult + amult > i64::from(degree) {
                    amult = (i64::from(degree) - mult).max(0);
                }
                sigma += amult;
            } else if amult > mult {
                if amult > i64::from(degree) {
                    amult = i64::from(degree);
                }
                if k == nk && periodic {
                    last_knot_mult = amult.max(mult);
                    sigma += 2 * (last_knot_mult - mult);
                } else {
                    sigma += amult - mult;
                }
            }
        } else if amult > 0 {
            // not identic to existing knot
            if amult > i64::from(degree) {
                amult = i64::from(degree);
            }
            nb_knots += 1;
            sigma += amult;
        }

        ak += 1;
    }

    // count the last knots
    while k < nk {
        k += 1;
        nb_knots += 1;
        sigma += i64::from(rd1(mults, k)?);
    }

    // for periodic B-Spline the requirement is that multiplicities of the first and last knots must be equal
    let nb_poles = if periodic { sigma - last_knot_mult } else { sigma - i64::from(degree) - 1 };
    Ok(Some((
        i32::try_from(nb_poles).map_err(|_| BSplError::OutOfRange)?,
        i32::try_from(nb_knots).map_err(|_| BSplError::OutOfRange)?,
    )))
}

fn check_add(add_knots: &[f64], add_mults: Option<&[i32]>) -> R<()> {
    if add_knots.is_empty() || add_mults.is_some_and(|m| m.len() != add_knots.len()) {
        return Err(BSplError::OutOfRange);
    }
    Ok(())
}

/// `BSplCLib::PrepareInsertKnots`: the number of poles and of knots after the insertion; `None` is the C++
/// `false` (the knots to add are outside the usable range or not sorted).
pub fn prepare_insert_knots(
    degree: i32,
    periodic: bool,
    knots: &[f64],
    mults: &[i32],
    add_knots: &[f64],
    add_mults: Option<&[i32]>,
    tolerance: f64,
    add: bool,
) -> R<Option<(i32, i32)>> {
    check_structure(degree, periodic, knots, mults)?;
    check_add(add_knots, add_mults)?;
    prepare_insert_core(degree, periodic, knots, mults, add_knots, add_mults, tolerance, add)
}

/// The body of `BSplCLib::InsertKnots`; `new_poles`, `new_knots` and `new_mults` have the sizes of the result.
pub(crate) fn insert_knots_core(
    degree: i32,
    periodic: bool,
    dimension: i32,
    poles: &[f64],
    knots: &[f64],
    mults: &[i32],
    add_knots: &[f64],
    add_mults: Option<&[i32]>,
    new_poles: &mut [f64],
    new_knots: &mut [f64],
    new_mults: &mut [i32],
    tolerance: f64,
    add: bool,
) -> R<()> {
    let add_flat = add_mults.is_none();
    let deg = ix(degree);
    let dim = ix(dimension);
    let nk_up = len_ix(knots);
    let poles_up = len_ix(poles);

    // create local arrays
    let mut local_knots: Vec<f64>;
    let mut local_poles = zeros(mul(2 * deg + 1, dim)?)?;

    // loop on the knots to insert
    let mut curk = 0isize; // current position in Knots
    let mut curnk = 0isize; // current position in NewKnots
    let mut curp = 1isize; // current position in Poles
    let mut curnp = 1isize; // current position in NewPoles

    // index is the first pole of the current curve for insertion schema
    let mut index = if periodic { -ix(rd1(mults, 1)?) } else { -deg - 1 };

    // on Periodic curves the first knot and the last knot are inserted later
    let mut firstmult = 0isize;

    for kn in 1..=len_ix(add_knots) {
        let u = rd1(add_knots, kn)?;
        let eps = tolerance.max(epsilon(u));

        // find the position in the old knots and copy to the new knots
        while curk < nk_up && rd1(knots, curk + 1)? - u <= eps {
            curk += 1;
            curnk += 1;
            wr1(new_knots, curnk, rd1(knots, curk)?)?;
            let m = rd1(mults, curk)?;
            wr1(new_mults, curnk, m)?;
            index += ix(m);
        }

        // Slice the knots and the mults to the current size of the new curve
        let nup = curnk + nk_up - curk;

        // copy enough knots to compute the insertion schema
        let mut k = curk;
        let mut i = curnk;
        let mut mult = 0isize;
        while mult < deg && k < nk_up {
            k += 1;
            i += 1;
            wr1(new_knots, i, rd1(knots, k)?)?;
            let m = rd1(mults, k)?;
            wr1(new_mults, i, m)?;
            mult += ix(m);
        }

        // copy knots at the end for periodic curve
        if periodic {
            mult = 0;
            k = nk_up;
            i = nup;
            while mult < deg && i > curnk {
                wr1(new_knots, i, rd1(knots, k)?)?;
                let m = rd1(mults, k)?;
                wr1(new_mults, i, m)?;
                mult += ix(m);
                k -= 1;
                i -= 1;
            }
            let first = rd1(new_mults, 1)?;
            wr1(new_mults, nup, first)?;
        }

        // do the boor scheme on the new curve to insert the new knot
        let same_knot = (u - rd1(new_knots, curnk)?).abs() <= eps;

        let length = if same_knot { (deg - ix(rd1(new_mults, curnk)?)).max(0) } else { deg };

        let mut depth = if add_flat { 1 } else { deg.min(ix(rd1(add_mults.unwrap_or(&[]), kn)?)) };

        if same_knot {
            if add {
                let current = ix(rd1(new_mults, curnk)?);
                if current + depth > deg {
                    depth = deg - current;
                }
            } else {
                depth = (depth - ix(rd1(new_mults, curnk)?)).max(0);
            }

            if periodic {
                // on periodic curve the first and last knot are delayed to the end
                if curk == 1 || curk == nk_up {
                    if firstmult == 0 {
                        // do that only once
                        firstmult += depth;
                    }
                    depth = 0;
                }
            }
        }
        if depth <= 0 {
            continue;
        }

        let nup_len = usize::try_from(nup).map_err(|_| BSplError::OutOfRange)?;
        let nknots = new_knots.get(..nup_len).ok_or(BSplError::OutOfRange)?;
        let nmults = new_mults.get(..nup_len).ok_or(BSplError::OutOfRange)?;
        let built = build_knots(degree, to_i32(curnk)?, periodic, nknots, Some(nmults))?;
        local_knots = built;

        // copy the poles
        let mut need = 1 + (index + length + 1) * dim - curnp;
        need = need.min(poles_up - curp + 1);

        let mut p = curp;
        let mut np = curnp;
        copy_poles(need, &mut p, poles, &mut np, new_poles)?;
        curp += need;
        curnp += need;

        // slice the poles to the current number of poles in case of periodic
        let npoles_len = usize::try_from(curnp - 1).map_err(|_| BSplError::OutOfRange)?;
        {
            let npoles = new_poles.get(..npoles_len).ok_or(BSplError::OutOfRange)?;
            build_boor(index, length, dim, npoles, &mut local_poles)?;
        }
        boor_scheme(u, deg, &local_knots, dim, &mut local_poles, depth, length)?;

        // copy the new poles
        curnp += depth * dim; // number of poles is increased by depth
        let the_len = usize::try_from(curnp - 1).map_err(|_| BSplError::OutOfRange)?;
        let the_poles = new_poles.get_mut(..the_len).ok_or(BSplError::OutOfRange)?;
        let mut np = 1 + (index + 1) * dim;
        for i in 1..=length + depth {
            get_pole(i, length, depth, dim, &local_poles, &mut np, the_poles)?;
        }

        // insert the knot
        index += depth;
        if same_knot {
            let m = rd1(new_mults, curnk)?;
            wr1(new_mults, curnk, m + to_i32(depth)?)?;
        } else {
            curnk += 1;
            wr1(new_knots, curnk, u)?;
            wr1(new_mults, curnk, to_i32(depth)?)?;
        }
    }

    // copy the last poles and knots
    {
        let mut p = curp;
        copy_poles(poles_up - curp + 1, &mut p, poles, &mut curnp, new_poles)?;
    }

    while curk < nk_up {
        curk += 1;
        curnk += 1;
        wr1(new_knots, curnk, rd1(knots, curk)?)?;
        wr1(new_mults, curnk, rd1(mults, curk)?)?;
    }

    // process the first-last knot on periodic curves
    if firstmult > 0 {
        curnk = 1;
        let first_mult_now = ix(rd1(new_mults, curnk)?);
        if first_mult_now + firstmult > deg {
            firstmult = deg - first_mult_now;
        }
        if firstmult > 0 {
            let length = deg - first_mult_now;
            let depth = firstmult;

            local_knots = build_knots(degree, to_i32(curnk)?, periodic, new_knots, Some(new_mults))?;
            let npoles_len = usize::try_from(len_ix(new_poles) - depth * dim).map_err(|_| BSplError::OutOfRange)?;
            {
                let npoles = new_poles.get(..npoles_len).ok_or(BSplError::OutOfRange)?;
                build_boor(0, length, dim, npoles, &mut local_poles)?;
            }
            let u = rd1(new_knots, curnk)?;
            boor_scheme(u, deg, &local_knots, dim, &mut local_poles, depth, length)?;

            // copy the new poles but rotate them with depth
            let mut np = 1isize;
            for i in depth..length + depth {
                get_pole(i, length, depth, dim, &local_poles, &mut np, new_poles)?;
            }
            np = len_ix(new_poles) - depth * dim + 1;
            for i in 0..depth {
                get_pole(i, length, depth, dim, &local_poles, &mut np, new_poles)?;
            }

            let first = rd1(new_mults, 1)?;
            wr1(new_mults, 1, first + to_i32(depth)?)?;
            let last_index = len_ix(new_mults);
            let last = rd1(new_mults, last_index)?;
            wr1(new_mults, last_index, last + to_i32(depth)?)?;
        }
    }
    Ok(())
}

/// `BSplCLib::InsertKnots` on flat poles: returns the new poles, knots and multiplicities, with the sizes
/// `prepare_insert_knots` gives.
pub fn insert_knots(
    degree: i32,
    periodic: bool,
    dimension: i32,
    poles: &[f64],
    knots: &[f64],
    mults: &[i32],
    add_knots: &[f64],
    add_mults: Option<&[i32]>,
    tolerance: f64,
    add: bool,
) -> R<CurveOut> {
    check_structure(degree, periodic, knots, mults)?;
    check_add(add_knots, add_mults)?;
    check_poles_len(degree, periodic, dimension, mults, poles)?;
    let (nb_poles, nb_knots) =
        prepare_insert_core(degree, periodic, knots, mults, add_knots, add_mults, tolerance, add)?
            .ok_or(BSplError::OutOfRange)?;
    if nb_poles < 1 || nb_knots < 1 {
        return Err(BSplError::OutOfRange);
    }
    let mut new_poles = zeros(mul(ix(nb_poles), ix(dimension))?)?;
    let mut new_knots = zeros(count(nb_knots)?)?;
    let mut new_mults = filled(count(nb_knots)?, 0i32)?;
    insert_knots_core(
        degree,
        periodic,
        dimension,
        poles,
        knots,
        mults,
        add_knots,
        add_mults,
        &mut new_poles,
        &mut new_knots,
        &mut new_mults,
        tolerance,
        add,
    )?;
    Ok((new_poles, new_knots, new_mults))
}

// ---------------------------------------------------------------------------------------------
// Knot removal

/// The sizes of the result of `remove_knot` for an `index` in the removable range: the multiplicity of the removed
/// knot and the new numbers of knots and poles.
fn remove_sizes(
    index: i32,
    mult: i32,
    degree: i32,
    periodic: bool,
    mults: &[i32],
    nb_poles: usize,
) -> R<Option<(isize, usize, usize)>> {
    let (first, last) = if periodic {
        (1, len_ix(mults))
    } else {
        (ix(first_u_knot_index(degree, mults)?) + 1, ix(last_u_knot_index(degree, mults)?) - 1)
    };
    if ix(index) < first || ix(index) > last {
        return Ok(None);
    }
    let the_index = if periodic && ix(index) == first { last } else { ix(index) };
    let depth = ix(rd1(mults, the_index)?) - ix(mult);
    if mult < 0 || depth < 1 {
        return Err(BSplError::OutOfRange);
    }
    let new_knots = if mult > 0 { mults.len() } else { mults.len() - 1 };
    let new_poles = nb_poles.checked_sub(count(to_i32(depth)?)?).ok_or(BSplError::OutOfRange)?;
    Ok(Some((depth, new_knots, new_poles)))
}

/// `BSplCLib::RemoveKnot` on flat poles: the new poles, knots and multiplicities, or `None` (the C++ `false`) when
/// the index is outside the removable range or the knot cannot be removed within the tolerance.
pub fn remove_knot(
    index: i32,
    mult: i32,
    degree: i32,
    periodic: bool,
    dimension: i32,
    poles: &[f64],
    knots: &[f64],
    mults: &[i32],
    tolerance: f64,
) -> R<Option<CurveOut>> {
    check_structure(degree, periodic, knots, mults)?;
    check_poles_len(degree, periodic, dimension, mults, poles)?;
    let nb = count(nb_poles(degree, periodic, mults)?)?;
    let Some((depth, nb_knots, nb_new_poles)) = remove_sizes(index, mult, degree, periodic, mults, nb)? else {
        return Ok(None);
    };
    let deg = ix(degree);
    let dim = ix(dimension);
    let the_index = {
        let (first, last) = if periodic {
            (1, len_ix(knots))
        } else {
            (ix(first_u_knot_index(degree, mults)?) + 1, ix(last_u_knot_index(degree, mults)?) - 1)
        };
        if periodic && ix(index) == first { last } else { ix(index) }
    };
    let (first, last) = if periodic {
        (1, len_ix(knots))
    } else {
        (ix(first_u_knot_index(degree, mults)?) + 1, ix(last_u_knot_index(degree, mults)?) - 1)
    };
    let length = deg - ix(mult);

    // build the knots for anti Boor Scheme: the new sequence of knots is obtained from the knots at Index-1 and Index
    let lower = build_knots(degree, to_i32(the_index - 1)?, periodic, knots, Some(mults))?;
    let mut pindex = ix(pole_index(degree, to_i32(the_index - 1)?, periodic, mults)?);
    let upper = build_knots(degree, to_i32(the_index)?, periodic, knots, Some(mults))?;
    let mut local_knots = lower;
    local_knots.extend(upper);

    pindex += ix(mult);

    for i in 0..deg - ix(mult) {
        let v = rd(&local_knots, i + ix(mult))?;
        wr(&mut local_knots, i, v)?;
    }
    for i in deg - ix(mult)..2 * deg {
        let v = rd(&local_knots, 2 * deg + i)?;
        wr(&mut local_knots, i, v)?;
    }

    // build the poles for anti Boor Scheme
    let mut local_poles = zeros(mul(2 * deg + 1, dim)?)?;
    let mut p = 1 + pindex * dim;
    for i in 0..=length + depth {
        let j = dim * boor_index(i, length, depth);
        for k in 0..dim {
            wr(&mut local_poles, j + k, rd1(poles, p + k)?)?;
        }
        p += dim;
        if p > len_ix(poles) {
            p = 1;
        }
    }

    // Anti Boor Scheme
    let u = rd1(knots, the_index)?;
    let result = anti_boor_scheme(u, deg, &local_knots, dim, &mut local_poles, depth, length, tolerance)?;
    if !result {
        return Ok(None);
    }

    // copy the results: poles
    let mut new_poles = zeros(mul(ix_len(nb_new_poles), dim)?)?;
    let mut p = 1isize;
    let mut np = 1isize;
    // unmodified poles before
    copy_poles((pindex + 1) * dim, &mut p, poles, &mut np, &mut new_poles)?;
    // modified
    for i in 1..=length {
        get_pole(i, length, 0, dim, &local_poles, &mut np, &mut new_poles)?;
    }
    p += (length + depth) * dim;
    // unmodified poles after
    if p != 1 {
        let n = len_ix(poles) - p + 1;
        copy_poles(n, &mut p, poles, &mut np, &mut new_poles)?;
    }

    // knots and mults
    let mut new_knots = zeros(nb_knots)?;
    let mut new_mults = filled(nb_knots, 0i32)?;
    if mult > 0 {
        new_knots.copy_from_slice(knots);
        new_mults.copy_from_slice(mults);
        wr1(&mut new_mults, the_index, mult)?;
        if periodic {
            if the_index == first {
                wr1(&mut new_mults, last, mult)?;
            }
            if the_index == last {
                wr1(&mut new_mults, first, mult)?;
            }
        }
    } else if !periodic || (the_index != first && the_index != last) {
        for i in 1..the_index {
            wr1(&mut new_knots, i, rd1(knots, i)?)?;
            wr1(&mut new_mults, i, rd1(mults, i)?)?;
        }
        for i in the_index + 1..=len_ix(knots) {
            wr1(&mut new_knots, i - 1, rd1(knots, i)?)?;
            wr1(&mut new_mults, i - 1, rd1(mults, i)?)?;
        }
    } else {
        // The interesting case of a Periodic curve where the first and last knot is removed.
        for i in first..last - 1 {
            wr1(&mut new_knots, i, rd1(knots, i + 1)?)?;
            wr1(&mut new_mults, i, rd1(mults, i + 1)?)?;
        }
        let value = rd1(&new_knots, first)? + rd1(knots, last)? - rd1(knots, first)?;
        wr1(&mut new_knots, last - 1, value)?;
        let m = rd1(&new_mults, first)?;
        wr1(&mut new_mults, last - 1, m)?;
    }
    Ok(Some((new_poles, new_knots, new_mults)))
}

fn ix_len(n: usize) -> isize {
    n as isize
}

// ---------------------------------------------------------------------------------------------
// Degree elevation

/// `BSplCLib::IncreaseDegreeCountKnots`: the number of knots after the degree elevation.
pub fn increase_degree_count_knots(degree: i32, new_degree: i32, periodic: bool, mults: &[i32]) -> R<i32> {
    if degree < 1 || new_degree < degree || mults.is_empty() {
        return Err(BSplError::OutOfRange);
    }
    let sum: i64 = mults.iter().map(|m| i64::from(*m)).sum();
    if sum < i64::from(degree) + 1 {
        return Err(BSplError::OutOfRange);
    }
    if periodic {
        return to_i32(len_ix(mults));
    }
    let f = ix(first_u_knot_index(degree, mults)?);
    let l = ix(last_u_knot_index(degree, mults)?);
    if f < 1 || l > len_ix(mults) || f > l {
        return Err(BSplError::OutOfRange);
    }
    let deg = ix(degree);
    let step = ix(new_degree) - deg;
    let mut removed = 0isize;

    let mut i = 1isize;
    let mut m = deg + (f - i + 1) * step + 1;
    while m > ix(new_degree) + 1 {
        removed += 1;
        m -= ix(rd1(mults, i)?) + step;
        i += 1;
    }
    if m < ix(new_degree) + 1 {
        removed -= 1;
    }

    i = len_ix(mults);
    m = deg + (i - l + 1) * step + 1;
    while m > ix(new_degree) + 1 {
        removed += 1;
        m -= ix(rd1(mults, i)?) + step;
        i -= 1;
    }
    if m < ix(new_degree) + 1 {
        removed -= 1;
    }

    to_i32(len_ix(mults) - removed)
}

/// What `BSplCLib::IncreaseDegree` has computed before it copies the result: the working curve.
struct Elevated {
    wknots: Vec<f64>,
    wmults: Vec<i32>,
    wpoles: Vec<f64>,
    /// index (1-based) of the first knot of the result in the working curve
    k: isize,
    /// the number of poles (values / dimension) to skip at the start of the working poles
    pf: isize,
}

/// The degree elevation loop of `BSplCLib::IncreaseDegree` and the search of the first knot of the result.
fn elevate(
    degree: i32,
    new_degree: i32,
    periodic: bool,
    dimension: i32,
    poles: &[f64],
    knots: &[f64],
    mults: &[i32],
) -> R<Elevated> {
    let deg = ix(degree);
    let new_deg = ix(new_degree);
    let dim = ix(dimension);

    // create the working curve
    let mut pf = 0isize; // number of null poles added at beginning
    let mut pl = 0isize; // number of null poles added at end

    let mut nbwknots = len_ix(knots);
    let f = ix(first_u_knot_index(degree, mults)?);
    let l = ix(last_u_knot_index(degree, mults)?);

    if periodic {
        // Periodic curves are transformed in non-periodic curves
        nbwknots += f - 1;
        pf = -deg - 1;
        for i in 1..=f {
            pf += ix(rd1(mults, i)?);
        }
        nbwknots += len_ix(mults) - l;
        pl = -deg - 1;
        for i in l..=len_ix(mults) {
            pl += ix(rd1(mults, i)?);
        }
    }

    // copy the knots and multiplicities
    let nw = count(to_i32(nbwknots)?)?;
    let mut wknots = zeros(nw)?;
    let mut wmults = filled(nw, 0i32)?;
    if !periodic {
        wknots.copy_from_slice(knots);
        wmults.copy_from_slice(mults);
    } else {
        // copy the knots for a periodic curve
        let period = rd1(knots, len_ix(knots))? - rd1(knots, 1)?;
        let mut i = 0isize;
        for k in l..len_ix(knots) {
            i += 1;
            wr1(&mut wknots, i, rd1(knots, k)? - period)?;
            wr1(&mut wmults, i, rd1(mults, k)?)?;
        }
        for k in 1..=len_ix(knots) {
            i += 1;
            wr1(&mut wknots, i, rd1(knots, k)?)?;
            wr1(&mut wmults, i, rd1(mults, k)?)?;
        }
        for k in 2..=f {
            i += 1;
            wr1(&mut wknots, i, rd1(knots, k)? + period)?;
            wr1(&mut wmults, i, rd1(mults, k)?)?;
        }
    }

    // set the first and last mults to Degree+1 and add null poles
    pf += deg + 1 - ix(rd1(&wmults, 1)?);
    wr1(&mut wmults, 1, degree + 1)?;
    pl += deg + 1 - ix(rd1(&wmults, nbwknots)?);
    wr1(&mut wmults, nbwknots, degree + 1)?;

    // poles of the working curve
    let mut nbwpoles = 0isize;
    for i in 1..=nbwknots {
        nbwpoles += ix(rd1(&wmults, i)?);
    }
    nbwpoles -= deg + 1;

    // we provide space for degree elevation
    let mut wpoles = zeros(mul(nbwpoles + (nbwknots - 1) * (new_deg - deg), dim)?)?;

    // (the first pf * dimension poles are already zero)
    let mut k = 1isize;
    for i in pf * dim + 1..=(nbwpoles - pl) * dim {
        wr1(&mut wpoles, i, rd1(poles, k)?)?;
        k += 1;
        if k > len_ix(poles) {
            k = 1;
        }
    }

    // Declare the temporary arrays used in degree incrementation
    let nbwp_max = nbwpoles + (nbwknots - 1) * (new_deg - deg);
    let mut tempc1 = zeros(mul(nbwp_max, dim)?)?;
    let mut tempc2 = zeros(mul(nbwp_max, dim)?)?;
    let mut iknots = zeros(nw)?;
    let mut nwpoles = zeros(mul(nbwp_max, dim)?)?;

    // Loop on degree incrementation
    let mut nbwp = nbwpoles;
    let mut cur_deg = deg;
    while cur_deg < new_deg {
        let nbp = nbwp; // current number of poles
        nbwp = nbp + nbwknots - 1; // new number of poles

        // Initialize the portion of nwpoles that will be used this iteration
        for idx in 1..=nbwp * dim {
            wr1(&mut nwpoles, idx, 0.0)?;
        }

        for step in 0..=cur_deg {
            // Compute the bspline of rank step.
            // if not the first time, decrement the multiplicities back
            if step != 0 {
                for i in 1..=nbwknots {
                    let m = rd1(&wmults, i)?;
                    wr1(&mut wmults, i, m - 1)?;
                }
            }

            // Poles are the current poles but the poles congruent to step are duplicated.
            let mut offset = 0isize;
            for i in 0..nbp {
                offset += 1;
                for k in 0..dim {
                    let v = rd1(&wpoles, 1 + i * dim + k)?;
                    wr1(&mut tempc1, (offset - 1) * dim + k + 1, v)?;
                }
                if i % (cur_deg + 1) == step {
                    offset += 1;
                    for k in 0..dim {
                        let v = rd1(&wpoles, 1 + i * dim + k)?;
                        wr1(&mut tempc1, (offset - 1) * dim + k + 1, v)?;
                    }
                }
            }

            // Knots multiplicities are increased for each knot where the sum of multiplicities is congruent to step
            let mut stepmult = step + 1;
            let mut nbknots = 0isize;
            let mut smult = 0isize;
            for k in 1..=nbwknots {
                smult += ix(rd1(&wmults, k)?);
                if smult >= stepmult {
                    // this knot is increased
                    stepmult += cur_deg + 1;
                    let m = rd1(&wmults, k)?;
                    wr1(&mut wmults, k, m + 1)?;
                } else {
                    // this knot is inserted
                    nbknots += 1;
                    wr1(&mut iknots, nbknots, rd1(&wknots, k)?)?;
                }
            }

            // the curve is obtained by inserting the knots to raise the multiplicities
            if nbknots > 0 {
                let aknots = iknots.get(..count(to_i32(nbknots)?)?).ok_or(BSplError::OutOfRange)?;
                let curve = tempc1.get(..count(to_i32(offset * dim)?)?).ok_or(BSplError::OutOfRange)?;
                let snapshot = wmults.clone();
                let mut nknots = zeros(nw)?;
                let ncurve_len = count(to_i32(nbwp * dim)?)?;
                let ncurve = tempc2.get_mut(..ncurve_len).ok_or(BSplError::OutOfRange)?;
                insert_knots_core(
                    to_i32(cur_deg + 1)?,
                    false,
                    dimension,
                    curve,
                    &wknots,
                    &snapshot,
                    aknots,
                    None,
                    ncurve,
                    &mut nknots,
                    &mut wmults,
                    0.0,
                    true,
                )?;

                // add to the average
                for i in 1..=nbwp * dim {
                    let v = rd1(&nwpoles, i)? + rd1(&tempc2, i)?;
                    wr1(&mut nwpoles, i, v)?;
                }
            } else {
                // add to the average
                for i in 1..=nbwp * dim {
                    let v = rd1(&nwpoles, i)? + rd1(&tempc1, i)?;
                    wr1(&mut nwpoles, i, v)?;
                }
            }
        }

        // The result is the average
        for i in 1..=nbwp * dim {
            wr1(&mut wpoles, i, rd1(&nwpoles, i)? / (cur_deg + 1) as f64)?;
        }
        cur_deg += 1;
    }

    // Copy the results: index in new knots of the first knot of the curve
    let firstknot = if periodic { len_ix(mults) - l + 1 } else { f };

    // the new curve starts at index firstknot so we must remove knots until the sum of multiplicities from the
    // first to the start is NewDegree+1; m is the current sum of multiplicities
    let mut m = 0isize;
    for k in 1..=firstknot {
        m += ix(rd1(&wmults, k)?);
    }

    // compute the new first knot (k), pf will be the index of the first pole
    let mut k = 1isize;
    let mut pf = 0isize;
    while m > new_deg + 1 {
        k += 1;
        m -= ix(rd1(&wmults, k)?);
        pf += ix(rd1(&wmults, k)?);
    }
    if m < new_deg + 1 {
        k -= 1;
        let value = rd1(&wmults, k)? + to_i32(m - new_deg - 1)?;
        wr1(&mut wmults, k, value)?;
        pf += m - new_deg - 1;
    }

    // on a periodic curve the knots start with firstknot
    if periodic {
        k = firstknot;
    }
    Ok(Elevated { wknots, wmults, wpoles, k, pf })
}

/// `BSplCLib::IncreaseDegree` on flat poles: the new poles, knots and multiplicities (`increase_degree_count_knots`
/// knots; the number of poles follows from the new multiplicities).
pub fn increase_degree(
    degree: i32,
    new_degree: i32,
    periodic: bool,
    dimension: i32,
    poles: &[f64],
    knots: &[f64],
    mults: &[i32],
) -> R<CurveOut> {
    check_structure(degree, periodic, knots, mults)?;
    check_poles_len(degree, periodic, dimension, mults, poles)?;
    if new_degree < degree || new_degree > 25 {
        return Err(BSplError::OutOfRange);
    }
    let nb_knots = count(increase_degree_count_knots(degree, new_degree, periodic, mults)?)?;
    if !periodic && first_u_knot_index(degree, mults)? >= last_u_knot_index(degree, mults)? {
        return Err(BSplError::OutOfRange);
    }
    let work = elevate(degree, new_degree, periodic, dimension, poles, knots, mults)?;

    // copy knots
    let mut new_knots = zeros(nb_knots)?;
    let mut new_mults = filled(nb_knots, 0i32)?;
    for (i, k) in (1..=ix_len(nb_knots)).zip(work.k..) {
        wr1(&mut new_knots, i, rd1(&work.wknots, k)?)?;
        wr1(&mut new_mults, i, rd1(&work.wmults, k)?)?;
    }

    // the number of poles of the result follows from its multiplicities
    let nb_new_poles = nb_poles(new_degree, periodic, &new_mults)?;
    if nb_new_poles < 1 {
        return Err(BSplError::OutOfRange);
    }
    // copy poles
    let dim = ix(dimension);
    let mut pf = work.pf * dim;
    let mut new_poles = zeros(mul(ix(nb_new_poles), dim)?)?;
    for i in 1..=len_ix(&new_poles) {
        pf += 1;
        wr1(&mut new_poles, i, rd1(&work.wpoles, pf)?)?;
    }
    Ok((new_poles, new_knots, new_mults))
}

// ---------------------------------------------------------------------------------------------
// Trimming

/// `BSplCLib::Trimming(Degree, Periodic, Dimension, Knots, Mults, Poles, U1, U2, NewKnots, NewMults, NewPoles)` on
/// flat poles: the poles (`nb_new_poles` of them), knots and multiplicities (`nb_new_knots` of each) of the part of
/// the curve between `u1` and `u2`. The sizes are those of the output arrays of the C++ (`PrepareTrimming` gives them).
pub fn trimming(
    degree: i32,
    periodic: bool,
    dimension: i32,
    knots: &[f64],
    mults: &[i32],
    poles: &[f64],
    u1: f64,
    u2: f64,
    nb_new_knots: usize,
    nb_new_poles: usize,
) -> R<CurveOut> {
    check_structure(degree, periodic, knots, mults)?;
    check_poles_len(degree, periodic, dimension, mults, poles)?;
    let add_knots = [u1, u2];
    let add_mults = [degree, degree];
    let tolerance = epsilon(u1);
    let (nb_temp_poles, nb_temp_knots) =
        prepare_insert_core(degree, periodic, knots, mults, &add_knots, Some(&add_mults), tolerance, false)?
            .ok_or(BSplError::OutOfRange)?;
    if nb_temp_poles < 1 || nb_temp_knots < 1 {
        return Err(BSplError::OutOfRange);
    }
    let mut temp_poles = zeros(mul(ix(nb_temp_poles), ix(dimension))?)?;
    let mut temp_knots = zeros(count(nb_temp_knots)?)?;
    let mut temp_mults = filled(count(nb_temp_knots)?, 0i32)?;
    insert_knots_core(
        degree,
        periodic,
        dimension,
        poles,
        knots,
        mults,
        &add_knots,
        Some(&add_mults),
        &mut temp_poles,
        &mut temp_knots,
        &mut temp_mults,
        tolerance,
        false,
    )?;
    // find in the temporary poles the index of the pole corresponding to U1
    let (uf, ul) = if periodic { (rd1(&temp_knots, 1)?, rd1(&temp_knots, len_ix(&temp_knots))?) } else { (0.0, 1.0) };
    let (k_index, _) = locate_core(&temp_knots, u1, periodic, 1, to_i32(len_ix(&temp_knots))?, uf, ul)?;
    let p_index = ix(pole_index(degree, k_index, periodic, &temp_mults)?) * ix(dimension);

    let mut new_poles = zeros(nb_new_poles)?;
    for i in 1..=len_ix(&new_poles) {
        wr1(&mut new_poles, i, rd1(&temp_poles, p_index + i)?)?;
    }
    let mut new_knots = zeros(nb_new_knots)?;
    let mut new_mults = filled(nb_new_knots, 0i32)?;
    for i in 1..=len_ix(&new_knots) {
        wr1(&mut new_knots, i, rd1(&temp_knots, ix(k_index) + i - 1)?)?;
        wr1(&mut new_mults, i, rd1(&temp_mults, ix(k_index) + i - 1)?)?;
    }
    let first = rd1(&new_mults, 1)?;
    wr1(&mut new_mults, 1, first.min(degree) + 1)?;
    let n = len_ix(&new_mults);
    let last = rd1(&new_mults, n)?;
    wr1(&mut new_mults, n, last.min(degree) + 1)?;
    Ok((new_poles, new_knots, new_mults))
}

// ---------------------------------------------------------------------------------------------
// Unperiodize

fn check_periodic_mults(degree: i32, mults: &[i32]) -> R<()> {
    let first = *mults.first().ok_or(BSplError::OutOfRange)?;
    let last = *mults.last().ok_or(BSplError::OutOfRange)?;
    if degree < 1 || mults.len() < 2 || first != last || first < 1 || first > degree {
        return Err(BSplError::OutOfRange);
    }
    if mults.iter().any(|m| *m < 1 || *m > degree) {
        return Err(BSplError::OutOfRange);
    }
    Ok(())
}

/// `BSplCLib::PrepareUnperiodize`: the numbers of knots and poles of the unperiodized curve.
pub fn prepare_unperiodize(degree: i32, mults: &[i32]) -> R<(i32, i32)> {
    check_periodic_mults(degree, mults)?;
    let mut nb_knots = len_ix(mults);
    let mut nb_poles = -ix(degree) - 1;
    for i in 1..=len_ix(mults) {
        nb_poles += ix(rd1(mults, i)?);
    }

    // Add knots at the beginning of the curve to raise Multiplicities to Degre + 1
    let mut sigma = ix(rd1(mults, 1)?);
    let mut k = len_ix(mults) - 1;
    while sigma < ix(degree) + 1 {
        sigma += ix(rd1(mults, k)?);
        nb_poles += ix(rd1(mults, k)?);
        k -= 1;
        nb_knots += 1;
    }
    // We must add exactly until Degree + 1 -> Suppress the excedent.
    if sigma > ix(degree) + 1 {
        nb_poles -= sigma - ix(degree) - 1;
    }

    // Add knots at the end of the curve to raise Multiplicities to Degre + 1
    sigma = ix(rd1(mults, len_ix(mults))?);
    k = 2;
    while sigma < ix(degree) + 1 {
        sigma += ix(rd1(mults, k)?);
        nb_poles += ix(rd1(mults, k)?);
        k += 1;
        nb_knots += 1;
    }
    if sigma > ix(degree) + 1 {
        nb_poles -= sigma - ix(degree) - 1;
    }
    Ok((to_i32(nb_knots)?, to_i32(nb_poles)?))
}

/// `BSplCLib::Unperiodize`: the new multiplicities, knots and poles of the unperiodized curve (sizes from
/// `prepare_unperiodize`).
pub fn unperiodize(
    degree: i32,
    dimension: i32,
    mults: &[i32],
    knots: &[f64],
    poles: &[f64],
) -> R<(Vec<i32>, Vec<f64>, Vec<f64>)> {
    check_structure(degree, true, knots, mults)?;
    check_poles_len(degree, true, dimension, mults, poles)?;
    let (nb_knots, nb_poles) = prepare_unperiodize(degree, mults)?;
    let nk = count(nb_knots)?;
    let mut new_mults = filled(nk, 0i32)?;
    let mut new_knots = zeros(nk)?;
    let mut new_poles = zeros(mul(ix(nb_poles), ix(dimension))?)?;

    let nknots = len_ix(knots);
    let deg1 = ix(degree) + 1;
    // number of knots to insert before knot(1) to raise sum of multiplicities to <Degree + 1>
    let mut sigma = ix(rd1(mults, 1)?);
    let mut k = len_ix(mults) - 1;
    let mut index = 0isize;
    while sigma < deg1 {
        sigma += ix(rd1(mults, k)?);
        k -= 1;
        index += 1;
    }

    let period = rd1(knots, nknots)? - rd1(knots, 1)?;

    // set the 'interior' knots
    for k in 1..=nknots {
        wr1(&mut new_knots, k + index, rd1(knots, k)?)?;
        wr1(&mut new_mults, k + index, rd1(mults, k)?)?;
    }

    // set the 'starting' knots
    for k in 1..=index {
        let v = rd1(&new_knots, k + nknots - 1)? - period;
        wr1(&mut new_knots, k, v)?;
        let m = rd1(&new_mults, k + nknots - 1)?;
        wr1(&mut new_mults, k, m)?;
    }
    let first = rd1(&new_mults, 1)?;
    wr1(&mut new_mults, 1, first - to_i32(sigma - deg1)?)?;

    // set the 'ending' knots
    sigma = ix(rd1(&new_mults, index + nknots)?);
    for k in nknots + index + 1..=len_ix(&new_knots) {
        let v = rd1(&new_knots, k - nknots + 1)? + period;
        wr1(&mut new_knots, k, v)?;
        let m = rd1(&new_mults, k - nknots + 1)?;
        wr1(&mut new_mults, k, m)?;
        sigma += ix(m);
    }
    let last_index = len_ix(&new_mults);
    let last = rd1(&new_mults, last_index)?;
    wr1(&mut new_mults, last_index, last - to_i32(sigma - deg1)?)?;

    let np_len = len_ix(poles);
    for k in 1..=len_ix(&new_poles) {
        let v = rd1(poles, (k - 1) % np_len + 1)?;
        wr1(&mut new_poles, k, v)?;
    }
    Ok((new_mults, new_knots, new_poles))
}
