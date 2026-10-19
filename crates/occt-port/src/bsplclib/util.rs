//! Checked access helpers shared by the translations of the pointer-walking parts of `BSplCLib` and `BSplSLib`.
//!
//! The C++ code reads and writes through raw pointers and `NCollection_Array` operators whose range checks are
//! compiled out of the OCCT libraries; here every access is checked and an access outside the array is
//! [`BSplError::OutOfRange`] (the numbers OCCT returns there are unspecified). Indices are `isize` because the C++
//! `int` arithmetic may go below zero between uses.
use super::BSplError;

/// The result type of the functions of this crate's `BSplCLib` and `BSplSLib`.
pub(crate) type R<T> = Result<T, BSplError>;

/// The most elements any array allocated on behalf of a caller may have (a sanity limit instead of an abort).
const MAX_ELEMENTS: usize = 1 << 24;

pub(crate) fn ix(v: i32) -> isize {
    v as isize
}

/// `isize` index to `i32`.
pub(crate) fn to_i32(v: isize) -> R<i32> {
    i32::try_from(v).map_err(|_| BSplError::OutOfRange)
}

/// A length as `isize`.
pub(crate) fn len_ix<T>(s: &[T]) -> isize {
    s.len() as isize
}

/// `s[i]` (0-based).
pub(crate) fn rd<T: Copy>(s: &[T], i: isize) -> R<T> {
    usize::try_from(i).ok().and_then(|k| s.get(k)).copied().ok_or(BSplError::OutOfRange)
}

/// `s[i] = v` (0-based).
pub(crate) fn wr<T>(s: &mut [T], i: isize, v: T) -> R<()> {
    let slot = usize::try_from(i).ok().and_then(|k| s.get_mut(k)).ok_or(BSplError::OutOfRange)?;
    *slot = v;
    Ok(())
}

/// Element `k` of an OCCT array with lower bound 1.
pub(crate) fn rd1<T: Copy>(s: &[T], k: isize) -> R<T> {
    rd(s, k - 1)
}

/// `s(k) = v` for an OCCT array with lower bound 1.
pub(crate) fn wr1<T>(s: &mut [T], k: isize, v: T) -> R<()> {
    wr(s, k - 1, v)
}

/// `n` copies of `value`, `Err` instead of an abort for an absurd size.
pub(crate) fn filled<T: Clone>(n: usize, value: T) -> R<Vec<T>> {
    if n > MAX_ELEMENTS {
        return Err(BSplError::OutOfRange);
    }
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| BSplError::OutOfRange)?;
    v.resize(n, value);
    Ok(v)
}

/// `n` zeros.
pub(crate) fn zeros(n: usize) -> R<Vec<f64>> {
    filled(n, 0.0)
}

/// A non-negative `i32` count as `usize`.
pub(crate) fn count(n: i32) -> R<usize> {
    usize::try_from(n).map_err(|_| BSplError::OutOfRange)
}

/// `a * b` for non-negative counts.
pub(crate) fn mul(a: isize, b: isize) -> R<usize> {
    if a < 0 || b < 0 {
        return Err(BSplError::OutOfRange);
    }
    usize::try_from(a)
        .ok()
        .and_then(|a| usize::try_from(b).ok().and_then(|b| a.checked_mul(b)))
        .ok_or(BSplError::OutOfRange)
}

/// `Epsilon(x)` of OCCT: the distance from `x` to the next representable double away from zero.
pub(crate) fn epsilon(x: f64) -> f64 {
    if x >= 0.0 { libm::nextafter(x, f64::MAX) - x } else { x - libm::nextafter(x, f64::MIN) }
}
