//! The surface modifications of `BSplSLib`: `Iso`, `Reverse`, `IsRational`, `SetPoles`/`GetPoles` and the knot
//! operations (a surface is processed as a curve whose poles are whole rows or columns of the pole array).
#![allow(clippy::float_cmp, clippy::too_many_arguments)]

use crate::bsplclib::BSplError;
use crate::bsplclib::flat::{build_knots, eval_in_place, locate_parameter_opt, pole_index};
use crate::bsplclib::util::{R, epsilon, ix, mul, rd, to_i32, wr, zeros};
use crate::bsplclib::{
    increase_degree as curve_increase_degree, insert_knots as curve_insert_knots, remove_knot as curve_remove_knot,
    unperiodize as curve_unperiodize,
};
use crate::gp::Pnt;
use crate::plib::Array2;

fn array2<T>(rows: usize, cols: usize, data: Vec<T>) -> R<Array2<T>> {
    Ok(Array2::new(rows, cols, data)?)
}

/// The item (`row`, `col`) (1-based).
fn item<T: Copy>(a: &Array2<T>, row: isize, col: isize) -> R<T> {
    let r = usize::try_from(row - 1).map_err(|_| BSplError::OutOfRange)?;
    let c = usize::try_from(col - 1).map_err(|_| BSplError::OutOfRange)?;
    a.get(r, c).copied().ok_or(BSplError::OutOfRange)
}

// ---------------------------------------------------------------------------------------------
// Iso

/// `BSplSLib::Iso`: the poles (and, when `want_weights`, the weights) of the iso-parametric curve of parameter
/// `param` in U (`is_u`) or V. `knots` and `mults` belong to the direction of `param`. The input is rational iff
/// `weights` is `Some`.
pub fn iso(
    param: f64,
    is_u: bool,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
    knots: &[f64],
    mults: Option<&[i32]>,
    degree: i32,
    periodic: bool,
    want_weights: bool,
) -> R<(Vec<Pnt>, Option<Vec<f64>>)> {
    if degree < 1 || (weights.is_some() && !want_weights) {
        return Err(BSplError::OutOfRange);
    }
    let rational = weights.is_some();
    let dim = if rational { 4isize } else { 3 };
    let deg = ix(degree);

    // compute local knots
    let (index, u) = locate_parameter_opt(degree, knots, mults, param, periodic, 0)?;
    let locknots = build_knots(degree, index, periodic, knots, mults)?;
    let mut index = match mults {
        None => ix(index) - (1 + deg),
        Some(m) => ix(pole_index(degree, index, periodic, m)?),
    };

    // copy the local poles
    let (rows, cols) = (len_ix_us(poles.rows()), len_ix_us(poles.cols()));
    let (f1, l1, f2, l2) = if is_u { (1, rows, 1, cols) } else { (1, cols, 1, rows) };
    let mut locpoles = zeros(mul((deg + 1) * (l2 - f2 + 1), dim)?)?;
    let mut pole = 0isize;
    index += f1;
    for _ in 0..=deg {
        for j in f2..=l2 {
            let p = if is_u { item(poles, index, j)? } else { item(poles, j, index)? };
            if rational {
                let w = if is_u { item_opt(weights, index, j)? } else { item_opt(weights, j, index)? };
                wr(&mut locpoles, pole + 3, w)?;
                wr(&mut locpoles, pole, p.x() * w)?;
                wr(&mut locpoles, pole + 1, p.y() * w)?;
                wr(&mut locpoles, pole + 2, p.z() * w)?;
            } else {
                wr(&mut locpoles, pole, p.x())?;
                wr(&mut locpoles, pole + 1, p.y())?;
                wr(&mut locpoles, pole + 2, p.z())?;
            }
            pole += dim;
        }
        index += 1;
        if index > l1 {
            index = f1;
        }
    }

    // compute the iso
    eval_in_place(u, degree, &locknots, to_i32((l2 - f2 + 1) * dim)?, &mut locpoles)?;

    // get the result
    let n = usize::try_from(l2 - f2 + 1).map_err(|_| BSplError::OutOfRange)?;
    let mut cpoles = Vec::new();
    let mut cweights = Vec::new();
    let mut pole = 0isize;
    for _ in 0..n {
        if rational {
            let w = rd(&locpoles, pole + 3)?;
            cweights.push(w);
            cpoles.push(Pnt::new(rd(&locpoles, pole)? / w, rd(&locpoles, pole + 1)? / w, rd(&locpoles, pole + 2)? / w));
        } else {
            cpoles.push(Pnt::new(rd(&locpoles, pole)?, rd(&locpoles, pole + 1)?, rd(&locpoles, pole + 2)?));
        }
        pole += dim;
    }
    // if the input is not rational but weights are wanted
    if !rational && want_weights {
        cweights = vec![1.0; n];
    }
    Ok((cpoles, if want_weights { Some(cweights) } else { None }))
}

fn len_ix_us(n: usize) -> isize {
    n as isize
}

fn item_opt(weights: Option<&Array2<f64>>, row: isize, col: isize) -> R<f64> {
    item(weights.ok_or(BSplError::OutOfRange)?, row, col)
}

// ---------------------------------------------------------------------------------------------
// Reverse

/// The common body of the two `Reverse` overloads.
fn reverse<T: Clone>(a: &Array2<T>, last: i32, u_direction: bool) -> R<Array2<T>> {
    let (rows, cols) = (a.rows(), a.cols());
    let length = if u_direction { rows } else { cols };
    if length == 0 {
        return Err(BSplError::OutOfRange);
    }
    let length_i = i64::try_from(length).map_err(|_| BSplError::OutOfRange)?;
    let l = 1 + (i64::from(last) - 1) % length_i;
    if l < 1 {
        return Err(BSplError::OutOfRange);
    }
    let l = usize::try_from(l).map_err(|_| BSplError::OutOfRange)?;
    let mut data: Vec<T> = a.as_slice().to_vec();
    for i in 1..=rows {
        for j in 1..=cols {
            let source = a.get(i - 1, j - 1).ok_or(BSplError::OutOfRange)?;
            // the position of the item in the reversed array (0-based)
            let (r, c) = if u_direction {
                (if i <= l { l - i } else { l + rows - i }, j - 1)
            } else {
                (i - 1, if j <= l { l - j } else { l + cols - j })
            };
            *data.get_mut(r * cols + c).ok_or(BSplError::OutOfRange)? = source.clone();
        }
    }
    array2(rows, cols, data)
}

/// `BSplSLib::Reverse(Poles, Last, UDirection)`.
pub fn reverse_poles(poles: &Array2<Pnt>, last: i32, u_direction: bool) -> R<Array2<Pnt>> {
    reverse(poles, last, u_direction)
}

/// `BSplSLib::Reverse(Weights, Last, UDirection)`.
pub fn reverse_weights(weights: &Array2<f64>, last: i32, u_direction: bool) -> R<Array2<f64>> {
    reverse(weights, last, u_direction)
}

// ---------------------------------------------------------------------------------------------
// IsRational

/// `BSplSLib::IsRational`, with its odd indexing (it compares neighbours along the first index only and uses
/// `Epsilon(Weights(I1, I2))` when `epsilon <= 0`).
pub fn is_rational(weights: &Array2<f64>, i1: i32, i2: i32, j1: i32, j2: i32, epsilon_value: f64) -> R<bool> {
    let eps = if epsilon_value > 0.0 { epsilon_value } else { epsilon(item(weights, ix(i1), ix(i2))?) };
    let li = len_ix_us(weights.rows());
    let lj = len_ix_us(weights.cols());
    if i1 < i2 && j1 < j2 && (i1 < 1 || j1 < 1) {
        return Err(BSplError::OutOfRange);
    }
    if li == 0 || lj == 0 {
        return Err(BSplError::OutOfRange);
    }
    let mut i = ix(i1) - 1;
    while i < ix(i2) - 1 {
        let mut j = ix(j1) - 1;
        while j < ix(j2) - 1 {
            let a = item(weights, 1 + i % li, 1 + j % lj)?;
            let b = item(weights, 1 + (i + 1) % li, 1 + j % lj)?;
            if (a - b).abs() > eps {
                return Ok(true);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(false)
}

// ---------------------------------------------------------------------------------------------
// SetPoles / GetPoles

fn check_shape<T>(a: &Array2<T>) -> R<()> {
    if a.rows() < 1 || a.cols() < 1 { Err(BSplError::OutOfRange) } else { Ok(()) }
}

/// The order of the items of a flat array: all (`row`, `col`) pairs, rows first for `u_direction`.
fn order(rows: usize, cols: usize, u_direction: bool) -> impl Iterator<Item = (usize, usize)> {
    let count = rows * cols;
    (0..count).map(move |k| if u_direction { (k / cols, k % cols) } else { (k % rows, k / rows) })
}

/// `BSplSLib::SetPoles(Poles, FP, UDirection)`: `x y z` per pole, rows first for `u_direction`, columns first
/// otherwise.
pub fn set_poles(poles: &Array2<Pnt>, u_direction: bool) -> R<Vec<f64>> {
    check_shape(poles)?;
    let mut fp = Vec::with_capacity(3 * poles.rows() * poles.cols());
    for (i, j) in order(poles.rows(), poles.cols(), u_direction) {
        let p = poles.get(i, j).ok_or(BSplError::OutOfRange)?;
        fp.extend([p.x(), p.y(), p.z()]);
    }
    Ok(fp)
}

/// `BSplSLib::SetPoles(Poles, Weights, FP, UDirection)`: `x*w y*w z*w w` per pole.
pub fn set_poles_weighted(poles: &Array2<Pnt>, weights: &Array2<f64>, u_direction: bool) -> R<Vec<f64>> {
    check_shape(poles)?;
    if weights.rows() < poles.rows() || weights.cols() < poles.cols() {
        return Err(BSplError::OutOfRange);
    }
    let mut fp = Vec::with_capacity(4 * poles.rows() * poles.cols());
    for (i, j) in order(poles.rows(), poles.cols(), u_direction) {
        let p = poles.get(i, j).ok_or(BSplError::OutOfRange)?;
        let w = *weights.get(i, j).ok_or(BSplError::OutOfRange)?;
        fp.extend([p.x() * w, p.y() * w, p.z() * w, w]);
    }
    Ok(fp)
}

fn shape(rows: i32, cols: i32) -> R<(usize, usize)> {
    if rows < 1 || cols < 1 {
        return Err(BSplError::OutOfRange);
    }
    Ok((
        usize::try_from(rows).map_err(|_| BSplError::OutOfRange)?,
        usize::try_from(cols).map_err(|_| BSplError::OutOfRange)?,
    ))
}

/// `BSplSLib::GetPoles(FP, Poles, UDirection)` for a `rows` x `cols` array.
pub fn get_poles(fp: &[f64], rows: i32, cols: i32, u_direction: bool) -> R<Array2<Pnt>> {
    let (rows, cols) = shape(rows, cols)?;
    if fp.len() < mul(ix_us(rows * 3), ix_us(cols))? {
        return Err(BSplError::OutOfRange);
    }
    let mut data = vec![Pnt::new(0.0, 0.0, 0.0); rows * cols];
    for (l, (i, j)) in order(rows, cols, u_direction).enumerate() {
        let at = 3 * l as isize;
        *data.get_mut(i * cols + j).ok_or(BSplError::OutOfRange)? =
            Pnt::new(rd(fp, at)?, rd(fp, at + 1)?, rd(fp, at + 2)?);
    }
    array2(rows, cols, data)
}

fn ix_us(n: usize) -> isize {
    n as isize
}

/// `BSplSLib::GetPoles(FP, Poles, Weights, UDirection)`: divides by the weight `FP(l + 3)`.
pub fn get_poles_weighted(fp: &[f64], rows: i32, cols: i32, u_direction: bool) -> R<(Array2<Pnt>, Array2<f64>)> {
    let (rows, cols) = shape(rows, cols)?;
    if fp.len() < mul(ix_us(rows * 4), ix_us(cols))? {
        return Err(BSplError::OutOfRange);
    }
    let mut data = vec![Pnt::new(0.0, 0.0, 0.0); rows * cols];
    let mut weights = vec![0.0; rows * cols];
    for (l, (i, j)) in order(rows, cols, u_direction).enumerate() {
        let at = 4 * l as isize;
        let w = rd(fp, at + 3)?;
        if w == 0.0 {
            return Err(BSplError::OutOfRange);
        }
        *weights.get_mut(i * cols + j).ok_or(BSplError::OutOfRange)? = w;
        *data.get_mut(i * cols + j).ok_or(BSplError::OutOfRange)? =
            Pnt::new(rd(fp, at)? / w, rd(fp, at + 1)? / w, rd(fp, at + 2)? / w);
    }
    Ok((array2(rows, cols, data)?, array2(rows, cols, weights)?))
}

// ---------------------------------------------------------------------------------------------
// Knot operations

/// The result of a knot operation: the new poles (and weights, when the input is rational), knots and
/// multiplicities.
#[derive(Debug)]
pub struct KnotResult {
    /// The new poles.
    pub poles: Array2<Pnt>,
    /// The new weights, `Some` iff the input is rational.
    pub weights: Option<Array2<f64>>,
    /// The new knots.
    pub knots: Vec<f64>,
    /// The new multiplicities.
    pub mults: Vec<i32>,
}

/// The flat poles of a surface processed as a curve, and the number of values of one curve pole.
fn flat_curve_poles(u_direction: bool, poles: &Array2<Pnt>, weights: Option<&Array2<f64>>) -> R<(Vec<f64>, i32)> {
    let dim = if weights.is_some() { 4 } else { 3 };
    let flat = match weights {
        Some(w) => set_poles_weighted(poles, w, u_direction)?,
        None => set_poles(poles, u_direction)?,
    };
    let other = if u_direction { poles.cols() } else { poles.rows() };
    Ok((flat, to_i32(ix_us(other) * dim)?))
}

/// The surface of the flat poles of the result: `flat.len() / curve_dimension` curve poles along the direction.
fn surface_of(
    flat: &[f64],
    curve_dimension: i32,
    old: &Array2<Pnt>,
    rational: bool,
    u_direction: bool,
) -> R<(Array2<Pnt>, Option<Array2<f64>>)> {
    let nb = usize::try_from(curve_dimension).map_err(|_| BSplError::OutOfRange)?;
    if nb == 0 || !flat.len().is_multiple_of(nb) {
        return Err(BSplError::OutOfRange);
    }
    let curve_poles = i32::try_from(flat.len() / nb).map_err(|_| BSplError::OutOfRange)?;
    let (rows, cols) =
        if u_direction { (curve_poles, to_i32(ix_us(old.cols()))?) } else { (to_i32(ix_us(old.rows()))?, curve_poles) };
    if rational {
        let (p, w) = get_poles_weighted(flat, rows, cols, u_direction)?;
        Ok((p, Some(w)))
    } else {
        Ok((get_poles(flat, rows, cols, u_direction)?, None))
    }
}

/// `BSplSLib::InsertKnots`.
pub fn insert_knots(
    u_direction: bool,
    degree: i32,
    periodic: bool,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
    knots: &[f64],
    mults: &[i32],
    add_knots: &[f64],
    add_mults: Option<&[i32]>,
    epsilon: f64,
    add: bool,
) -> R<KnotResult> {
    let (flat, curve_dimension) = flat_curve_poles(u_direction, poles, weights)?;
    let (np, nk, nm) =
        curve_insert_knots(degree, periodic, curve_dimension, &flat, knots, mults, add_knots, add_mults, epsilon, add)?;
    let (poles, weights) = surface_of(&np, curve_dimension, poles, weights.is_some(), u_direction)?;
    Ok(KnotResult { poles, weights, knots: nk, mults: nm })
}

/// `BSplSLib::RemoveKnot`: `None` is the C++ `false`.
pub fn remove_knot(
    u_direction: bool,
    index: i32,
    mult: i32,
    degree: i32,
    periodic: bool,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
    knots: &[f64],
    mults: &[i32],
    tolerance: f64,
) -> R<Option<KnotResult>> {
    let (flat, curve_dimension) = flat_curve_poles(u_direction, poles, weights)?;
    let Some((np, nk, nm)) =
        curve_remove_knot(index, mult, degree, periodic, curve_dimension, &flat, knots, mults, tolerance)?
    else {
        return Ok(None);
    };
    let (poles, weights) = surface_of(&np, curve_dimension, poles, weights.is_some(), u_direction)?;
    Ok(Some(KnotResult { poles, weights, knots: nk, mults: nm }))
}

/// `BSplSLib::IncreaseDegree`.
pub fn increase_degree(
    u_direction: bool,
    degree: i32,
    new_degree: i32,
    periodic: bool,
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
    knots: &[f64],
    mults: &[i32],
) -> R<KnotResult> {
    let (flat, curve_dimension) = flat_curve_poles(u_direction, poles, weights)?;
    let (np, nk, nm) = curve_increase_degree(degree, new_degree, periodic, curve_dimension, &flat, knots, mults)?;
    let (poles, weights) = surface_of(&np, curve_dimension, poles, weights.is_some(), u_direction)?;
    Ok(KnotResult { poles, weights, knots: nk, mults: nm })
}

/// `BSplSLib::Unperiodize`.
pub fn unperiodize(
    u_direction: bool,
    degree: i32,
    mults: &[i32],
    knots: &[f64],
    poles: &Array2<Pnt>,
    weights: Option<&Array2<f64>>,
) -> R<KnotResult> {
    let (flat, curve_dimension) = flat_curve_poles(u_direction, poles, weights)?;
    let (nm, nk, np) = curve_unperiodize(degree, curve_dimension, mults, knots, &flat)?;
    let (poles, weights) = surface_of(&np, curve_dimension, poles, weights.is_some(), u_direction)?;
    Ok(KnotResult { poles, weights, knots: nk, mults: nm })
}
