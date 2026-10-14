//! OCCT's `math` package, linear-algebra part: bounded vectors and matrices, the Gauss and
//! Gauss-Kronrod tables, LU/Crout/Householder/SVD/Jacobi solvers, the eigenvalue search for
//! tridiagonal matrices and the direct polynomial root finders.
//!
//! The exceptions of OCCT become [`MathError`] values. Note that OCCT's own library is built
//! with `No_Exception`, so some checks that appear in the sources (for example the squareness
//! test of `math_Gauss`) do not fire in the reference binary; where that leaves the behaviour
//! undefined (out-of-range reads), the translation reports an error instead.

mod eigen;
#[path = "gauss_tables.rs"]
mod gauss_tables;
#[path = "kronrod_tables.rs"]
mod kronrod_tables;
mod matrix;
mod poly;
pub mod recipes;
mod solvers;
mod vector;

pub use eigen::{ComputeGaussPointsAndWeights, ComputeKronrodPointsAndWeights, EigenValuesSearcher};
pub use matrix::{DoubleTab, Matrix};
pub use poly::DirectPolynomialRoots;
pub use solvers::{Crout, Gauss, GaussLeastSquare, Householder, Jacobi, Svd};
pub use vector::{Element, IntegerVector, Vector, VectorBase};

/// The error type standing for the exceptions of OCCT's `math` package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathError {
    /// Invalid bounds at construction (`Standard_RangeError` / `Standard_ConstructionError`).
    Construction,
    /// Incompatible dimensions (`Standard_DimensionError`).
    Dimension,
    /// Index out of range (`Standard_OutOfRange`, `Standard_RangeError`).
    Range,
    /// Division by zero or a null norm (`Standard_DivideByZero`, `Standard_NullValue`).
    Domain,
    /// Singular matrix (`math_SingularMatrix`).
    Singular,
    /// Matrix is not square (`math_NotSquare`).
    NotSquare,
    /// The computation did not finish (`StdFail_NotDone`).
    NotDone,
    /// An iteration did not converge.
    NoConvergence,
    /// The polynomial has infinitely many roots (`StdFail_InfiniteSolutions`).
    InfiniteSolutions,
    /// Invalid argument.
    Argument,
}

/// `math_Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStatus {
    /// `math_OK`.
    Ok = 0,
    /// `math_TooManyIterations`.
    TooManyIterations = 1,
    /// `math_FunctionError`.
    FunctionError = 2,
    /// `math_DirectionSearchError`.
    DirectionSearchError = 3,
    /// `math_NotBracketed`.
    NotBracketed = 4,
}

/// `math_Status_UserAborted`.
pub const STATUS_USER_ABORTED: i32 = -1;
/// `math_Status_OK`.
pub const STATUS_OK: i32 = 0;
/// `math_Status_SingularMatrix`.
pub const STATUS_SINGULAR_MATRIX: i32 = 1;
/// `math_Status_ArgumentError`.
pub const STATUS_ARGUMENT_ERROR: i32 = 2;
/// `math_Status_NoConvergence`.
pub const STATUS_NO_CONVERGENCE: i32 = 3;

/// `math::GaussPointsMax()`.
pub fn gauss_points_max() -> i32 {
    61
}

/// `math::KronrodPointsMax()`.
pub fn kronrod_points_max() -> i32 {
    123
}

/// The points and weights of the `n`-point Gauss rule from OCCT's tables, laid out like
/// `math::GaussPoints`/`GaussWeights` (positive half first, then the mirrored half) or, for
/// `ordered`, like `math::OrderedGaussPointsAndWeights` (increasing points).
fn gauss_table(n: i32, lower: i32, ordered: bool) -> Result<(Vector, Vector), MathError> {
    if !(1..=gauss_points_max()).contains(&n) {
        return Err(MathError::Range);
    }
    let mut points = Vector::new(lower, lower + n - 1)?;
    let mut weights = Vector::new(lower, lower + n - 1)?;
    let mut start = 0usize;
    for i in 1..n {
        start += ((i + 1) >> 1) as usize;
    }
    let ind = ((n + 1) >> 1) as usize;
    let table = |k: usize| -> Result<(f64, f64), MathError> {
        let x = *gauss_tables::POINT.get(k).ok_or(MathError::Range)?;
        let w = *gauss_tables::WEIGHT.get(k).ok_or(MathError::Range)?;
        Ok((x, w))
    };
    if ordered {
        for i in 0..(n as usize / 2) {
            let (x, w) = table(start + 1 + i)?;
            points.set_value(lower + i as i32, -x)?;
            points.set_value(lower + n - 1 - i as i32, x)?;
            weights.set_value(lower + i as i32, w)?;
            weights.set_value(lower + n - 1 - i as i32, w)?;
        }
        if n % 2 == 1 {
            let i = n as usize / 2;
            let (x, w) = table(start + 1 + i)?;
            points.set_value(lower + i as i32, x)?;
            weights.set_value(lower + i as i32, w)?;
        }
    } else {
        for i in 0..ind {
            let (x, w) = table(start + 1 + i)?;
            points.set_value(lower + i as i32, x)?;
            weights.set_value(lower + i as i32, w)?;
            if i + ind < n as usize {
                points.set_value(lower + (i + ind) as i32, -x)?;
                weights.set_value(lower + (i + ind) as i32, w)?;
            }
        }
    }
    Ok((points, weights))
}

/// `math::GaussPoints(Index, Points)`, for `1 <= index <= GaussPointsMax()` (OCCT reads past the
/// end of its table beyond that, which is reported as [`MathError::Range`]).
pub fn gauss_points(index: i32, lower: i32) -> Result<Vector, MathError> {
    Ok(gauss_table(index, lower, false)?.0)
}

/// `math::GaussWeights(Index, Weights)`, with the same range as [`gauss_points`].
pub fn gauss_weights(index: i32, lower: i32) -> Result<Vector, MathError> {
    Ok(gauss_table(index, lower, false)?.1)
}

/// Copies computed values into a vector with the bounds `lower..` (`Points = Computer.Points()`).
fn shifted(values: &Vector, lower: i32) -> Result<Vector, MathError> {
    Vector::from_slice(lower, values.as_slice())
}

/// `math::OrderedGaussPointsAndWeights(Index, Points, Weights)`: `None` for the C++ `false`.
pub fn ordered_gauss_points_and_weights(index: i32, lower: i32) -> Result<Option<(Vector, Vector)>, MathError> {
    if index < 1 {
        return Ok(None);
    }
    if index <= gauss_points_max() {
        return Ok(Some(gauss_table(index, lower, true)?));
    }
    let computer = ComputeGaussPointsAndWeights::new(index);
    if !computer.is_done() {
        return Ok(None);
    }
    Ok(Some((shifted(&computer.points(), lower)?, shifted(&computer.weights(), lower)?)))
}

/// `math::KronrodPointsAndWeights(Index, Points, Weights)`: `None` for the C++ `false`.
pub fn kronrod_points_and_weights(index: i32, lower: i32) -> Option<(Vector, Vector)> {
    if index < 3 || index % 2 == 0 {
        return None;
    }
    if index <= kronrod_points_max() {
        let mut start = 0usize;
        let mut i = 3;
        while i < index {
            start += ((i + 1) / 2) as usize;
            i += 2;
        }
        let nb_pts = (i / 2) as usize;
        let mut points = Vector::new(lower, lower + index - 1).ok()?;
        let mut weights = Vector::new(lower, lower + index - 1).ok()?;
        for k in 0..nb_pts {
            let x = *kronrod_tables::POINT.get(start + k)?;
            let w = *kronrod_tables::WEIGHT.get(start + k)?;
            points.set_value(lower + k as i32, -x).ok()?;
            points.set_value(lower + index - 1 - k as i32, x).ok()?;
            weights.set_value(lower + k as i32, w).ok()?;
            weights.set_value(lower + index - 1 - k as i32, w).ok()?;
        }
        points.set_value(lower + nb_pts as i32, *kronrod_tables::POINT.get(start + nb_pts)?).ok()?;
        weights.set_value(lower + nb_pts as i32, *kronrod_tables::WEIGHT.get(start + nb_pts)?).ok()?;
        return Some((points, weights));
    }
    let computer = ComputeKronrodPointsAndWeights::new((index - 1) / 2);
    if !computer.is_done() {
        return None;
    }
    Some((shifted(&computer.points(), lower).ok()?, shifted(&computer.weights(), lower).ok()?))
}
