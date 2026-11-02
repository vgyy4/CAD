# `mathlin_pkg`: Rust API contract

Module `occt_port::mathlin_pkg` translates the header-only package `MathLin` of OCCT 8.0.1 (`cpp/MathLin/`: Gauss, Crout,
SVD, Householder, Jacobi, LeastSquares, EigenSearch). It reuses `mathlin::{Matrix, Vector, IntegerVector, MathError}` and
`mathinteg::Status` (re-exported as `mathlin_pkg::Status`; same discriminants as `MathUtils::Status`). The module
`mathlin` (math_* classes) is a different package and stays unchanged, as does everything else in `src/`.
The numerical-recipes helpers `SVD_Decompose` and `Jacobi` of `math_Recipes` are the ones in `mathlin::recipes`
(re-use them; do not change their APIs).

## Errors

The OCCT build used for the oracle has range checks ON: any out-of-range access to a `math_Vector`/`math_Matrix` throws
`Standard_OutOfRange` (for instance `Gauss::LU` and `Crout`'s solver index rows with column indices, so a matrix whose
row and column lower bounds differ throws). The oracle prints `throws` for such cases. Every function therefore returns
`Result<_, MathError>`: `Err` exactly where C++ throws; the status values `InvalidInput`, `NumericalError`, `Singular` ...
are returned in the result structs as in C++ (`Ok(result)`). Indices are OCCT's absolute indices (`value(i, j)` of
`mathlin::Matrix`), and a faithful port reproduces the throws by using the checked accessors and `?`.

## Result structs (all `Debug + Clone`, field `status: Status`, `Option` = C++ `std::optional`)

```rust
pub struct LuResult { pub status: Status, pub lu: Option<Matrix>, pub pivot: Option<IntegerVector>, pub determinant: Option<f64>, pub sign: i32 } // sign defaults to 1
pub struct LinearResult { pub status: Status, pub solution: Option<Vector>, pub determinant: Option<f64> }
pub struct LinearMultipleResult { pub status: Status, pub solutions: Option<Matrix>, pub determinant: Option<f64> }
pub struct InverseResult { pub status: Status, pub inverse: Option<Matrix>, pub determinant: Option<f64> }
pub struct CroutResult { pub status: Status, pub l: Option<Matrix>, pub d: Option<Vector>, pub inverse: Option<Matrix>, pub determinant: Option<f64> }
pub struct SvdResult { pub status: Status, pub u: Option<Matrix>, pub singular_values: Option<Vector>, pub v: Option<Matrix>, pub rank: i32 }
pub struct QrResult { pub status: Status, pub q: Option<Matrix>, pub r: Option<Matrix>, pub rank: i32 }
pub struct EigenResult { pub status: Status, pub nb_iterations: usize, pub eigen_values: Option<Vector>, pub eigen_vectors: Option<Matrix> } // MathUtils::EigenResult (Jacobi)
pub enum LeastSquaresMethod { NormalEquations = 0, Qr = 1, Svd = 2 }
pub struct LeastSquaresResult { pub status: Status, pub solution: Option<Vector>, pub residual: Option<f64>, pub residual_sq: Option<f64>, pub rank: i32 }
pub struct TridiagonalEigenResult { pub status: Status, pub eigen_values: Option<Vector>, pub eigen_vectors: Option<Matrix>, pub dimension: i32 } // MathLin::EigenResult of EigenSearch
```
`status` defaults to `NotConverged` (C++ default of the structs). Default arguments of the C++ functions are listed
below; Rust has no defaults, the caller passes every argument.

## Functions (C++ name in the comment; defaults in brackets)

```rust
pub fn lu(a: &Matrix, min_pivot: f64) -> Result<LuResult, MathError>;                       // LU [1e-20]
pub fn solve(a: &Matrix, b: &Vector, min_pivot: f64) -> Result<LinearResult, MathError>;    // Solve [1e-20]
pub fn solve_multiple(a: &Matrix, b: &Matrix, min_pivot: f64) -> Result<LinearMultipleResult, MathError>; // SolveMultiple
pub fn determinant(a: &Matrix, min_pivot: f64) -> Result<LinearResult, MathError>;          // Determinant (solution stays None)
pub fn invert(a: &Matrix, min_pivot: f64) -> Result<InverseResult, MathError>;              // Invert
pub fn crout(a: &Matrix, min_pivot: f64) -> Result<CroutResult, MathError>;                 // Crout [1e-20]
pub fn solve_crout(a: &Matrix, b: &Vector, min_pivot: f64) -> Result<LinearResult, MathError>;     // SolveCrout
pub fn invert_crout(a: &Matrix, min_pivot: f64) -> Result<InverseResult, MathError>;        // InvertCrout
pub fn svd(a: &Matrix, tolerance: f64) -> Result<SvdResult, MathError>;                     // SVD [1e-15]
pub fn solve_svd(a: &Matrix, b: &Vector, tolerance: f64) -> Result<LinearResult, MathError>;       // SolveSVD [1e-6]
pub fn pseudo_inverse(a: &Matrix, tolerance: f64) -> Result<InverseResult, MathError>;      // PseudoInverse [1e-6]
pub fn condition_number(a: &Matrix) -> Result<f64, MathError>;                              // ConditionNumber (inf when SVD fails)
pub fn numerical_rank(a: &Matrix, tolerance: f64) -> Result<i32, MathError>;                // NumericalRank [1e-15]
pub fn qr(a: &Matrix, tolerance: f64) -> Result<QrResult, MathError>;                       // QR [1e-20]
pub fn solve_qr(a: &Matrix, b: &Vector, tolerance: f64) -> Result<LinearResult, MathError>; // SolveQR [1e-20]
pub fn solve_qr_multiple(a: &Matrix, b: &Matrix, tolerance: f64) -> Result<LinearMultipleResult, MathError>; // SolveQRMultiple
pub fn jacobi(a: &Matrix, sort_descending: bool) -> Result<EigenResult, MathError>;         // Jacobi [true]
pub fn eigen_values(a: &Matrix, sort_descending: bool) -> Result<EigenResult, MathError>;   // EigenValues [true]
pub fn spectral_decomposition(a: &Matrix) -> Result<EigenResult, MathError>;                // SpectralDecomposition
pub fn matrix_power(a: &Matrix, power: f64) -> Result<Option<Matrix>, MathError>;           // MatrixPower (pow: libm::pow)
pub fn matrix_sqrt(a: &Matrix) -> Result<Option<Matrix>, MathError>;                        // MatrixSqrt
pub fn matrix_inv_sqrt(a: &Matrix) -> Result<Option<Matrix>, MathError>;                    // MatrixInvSqrt
pub fn least_squares(a: &Matrix, b: &Vector, method: LeastSquaresMethod, tolerance: f64) -> Result<LeastSquaresResult, MathError>;       // [Qr, 1e-15]
pub fn weighted_least_squares(a: &Matrix, b: &Vector, w: &Vector, method: LeastSquaresMethod, tolerance: f64) -> Result<LeastSquaresResult, MathError>;
pub fn regularized_least_squares(a: &Matrix, b: &Vector, lambda: f64, tolerance: f64) -> Result<LeastSquaresResult, MathError>;           // [1e-15]
pub fn optimal_regularization(a: &Matrix, b: &Vector, lambda_min: f64, lambda_max: f64, nb_points: i32) -> Result<f64, MathError>;       // [1e-10, 1e2, 20]; log10/pow: libm
pub fn eigen_tridiagonal(diagonal: &Vector, subdiagonal: &Vector, max_iterations: i32) -> Result<TridiagonalEigenResult, MathError>;     // EigenTridiagonal [30]
pub fn get_eigen_vector(result: &TridiagonalEigenResult, index: i32) -> Result<Vector, MathError>; // GetEigenVector
```
Notes: `get_eigen_vector` of a result without eigenvectors returns `math_Vector(1, 1)` whose single item is
uninitialised in C++; the judge only checks its bounds in that case (return a vector `1..=1` holding 0.0).
`LuResult::pivot` of a failed `lu` has uninitialised items in C++; the judge prints it only when `status == Ok`.
`lu` leaves `lu`, `sign` as they are at the failure (partially eliminated copy of `a`), exactly as C++.
A matrix with fewer rows than columns given to `qr`, `solve_qr`, `solve_qr_multiple` gives status `InvalidInput`.

## Judge

`oracle/mathlin_pkg.cpp` is compiled twice (once with `-DPART_EIGEN`, because the two `MathLin::EigenResult` structs
cannot share a translation unit) and the outputs are concatenated into `tests/mathlin_pkg.txt` (see
`build-oracle.sh`). Each line is `family inputs | outputs`; floats are 16-hex-digit f64 bit patterns, integers decimal;
a matrix is `lowerRow upperRow lowerCol upperCol values(row-major)`, a vector `lower upper values`; an optional is a
`0/1` flag followed by its value. All families are compared bit for bit except `matrix_power`, `matrix_sqrt`,
`matrix_inv_sqrt` and `optimal_regularization` (they call `pow`/`log10`: 8 ulps of the largest value on the line).
The oracle was run with randomised stack and heap fill (`SMASH`, `MALLOC_PERTURB_`) and printed identical output.
