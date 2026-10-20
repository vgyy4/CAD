//! Translation of OCCT 8.0.1's package `Convert` (`TKMath`): the conic and elementary-surface converters to B-splines,
//! the Bezier-sequence converter, and the polynomial-to-poles converters. The API is described in
//! docs/api-convert.md; every OCCT array has lower bound 1, so "element `k`" of the C++ is `slice[k - 1]` here.
//!
//! Operations are done in the order of the C++ source (no fused multiply-add, no reassociation).

mod bezier;
mod conic;
mod cos_sin;
mod grid;
mod polynomial;
mod surface;

pub use bezier::{CompBezierCurves2dToBSplineCurve2d, CompBezierCurvesToBSplineCurve};
pub use conic::ConicToBSplineCurve;
pub use cos_sin::{CosAndSin, build_polynomial_cos_and_sin};
pub use grid::GridPolynomialToPoles;
pub use polynomial::CompPolynomialToPoles;
pub use surface::ElementarySurfaceToBSplineSurface;

use crate::bsplclib::BSplError;
use crate::gp;
use crate::mathlin::MathError;
use crate::plib::PLibError;

/// The error type of the converters, standing for the exceptions of OCCT and for the checks that OCCT 8.0.1 as built
/// does not make (an input for which the numbers OCCT returns are unspecified).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// `Standard_ConstructionError` raised by OCCT.
    Construction,
    /// `Standard_DomainError` raised by OCCT.
    Domain,
    /// `Standard_OutOfRange` (and an access outside an array).
    OutOfRange,
    /// `StdFail_NotDone`.
    NotDone,
    /// An input that OCCT 8.0.1 as built does not check and that is unspecified there, or a wrong shape of a slice.
    Argument,
    /// An error of `gp`.
    Gp(gp::ConstructionError),
    /// An error of `mathlin`.
    Math(MathError),
    /// An error of `plib`.
    PLib(PLibError),
    /// An error of `bsplclib` / `bsplslib`.
    BSpl(BSplError),
}

impl From<MathError> for ConvertError {
    fn from(e: MathError) -> Self {
        ConvertError::Math(e)
    }
}

impl From<gp::ConstructionError> for ConvertError {
    fn from(e: gp::ConstructionError) -> Self {
        ConvertError::Gp(e)
    }
}

impl From<PLibError> for ConvertError {
    fn from(e: PLibError) -> Self {
        ConvertError::PLib(e)
    }
}

impl From<BSplError> for ConvertError {
    fn from(e: BSplError) -> Self {
        ConvertError::BSpl(e)
    }
}

/// `Convert_ParameterisationType`, in the order of the C++ enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum ParameterisationType {
    /// `Convert_TgtThetaOver2`.
    TgtThetaOver2,
    /// `Convert_TgtThetaOver2_1`.
    TgtThetaOver2_1,
    /// `Convert_TgtThetaOver2_2`.
    TgtThetaOver2_2,
    /// `Convert_TgtThetaOver2_3`.
    TgtThetaOver2_3,
    /// `Convert_TgtThetaOver2_4`.
    TgtThetaOver2_4,
    /// `Convert_QuasiAngular`.
    QuasiAngular,
    /// `Convert_RationalC1`.
    RationalC1,
    /// `Convert_Polynomial`.
    Polynomial,
}

pub(crate) type R<T> = Result<T, ConvertError>;

/// `Precision::PConfusion()`.
pub(crate) const PCONFUSION: f64 = 1.0e-7 / 100.0;

/// The most spans / elements any converter allocates (a sanity limit instead of an abort for an absurd parameter range).
pub(crate) const MAX_SPANS: i32 = 1 << 20;

/// `std::max(a, b)`.
pub(crate) fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `std::min(a, b)`.
pub(crate) fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Element `k` (1-based) of an OCCT array.
pub(crate) fn at<T: Copy>(s: &[T], k: i64) -> R<T> {
    k.checked_sub(1)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| s.get(i))
        .copied()
        .ok_or(ConvertError::OutOfRange)
}

/// `s(k) = v` for an OCCT array with lower bound 1.
pub(crate) fn put<T>(s: &mut [T], k: i64, v: T) -> R<()> {
    let slot = k
        .checked_sub(1)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| s.get_mut(i))
        .ok_or(ConvertError::OutOfRange)?;
    *slot = v;
    Ok(())
}

/// A non-negative `i32` count as `usize`.
pub(crate) fn len_of(n: i32) -> R<usize> {
    usize::try_from(n).map_err(|_| ConvertError::Argument)
}

/// `(int)std::trunc(1.2 * delta / M_PI) + 1`, the number of spans of an arc of `delta` radians (maximum opening 150
/// degrees). The conversion of a non-finite or absurd value to `int` is undefined in C++: `Err(Argument)` here.
pub(crate) fn nb_spans(delta: f64) -> R<i32> {
    let t = (1.2 * delta / std::f64::consts::PI).trunc();
    if !t.is_finite() || t.abs() > f64::from(MAX_SPANS) {
        return Err(ConvertError::Argument);
    }
    Ok(t as i32 + 1)
}
