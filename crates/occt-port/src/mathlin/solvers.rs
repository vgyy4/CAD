//! `math_Gauss`, `math_Crout`, `math_GaussLeastSquare`, `math_SVD`, `math_Jacobi`, `math_Householder`.
use core::cell::RefCell;

use super::recipes::{self, JacobiResult};
use super::{IntegerVector, MathError, Matrix, Vector};

/// `math_Gauss`: LU decomposition with partial pivoting.
#[derive(Debug, Clone, PartialEq)]
pub struct Gauss {
    lu: Matrix,
    index: IntegerVector,
    d: f64,
    done: bool,
}

impl Gauss {
    /// `math_Gauss(A, MinPivot)`; `MinPivot` is `1.0e-20` in OCCT's default argument.
    /// The matrix must be square (OCCT raises `math_NotSquare`).
    pub fn new(a: &Matrix, min_pivot: f64) -> Result<Self, MathError> {
        if a.row_number() != a.col_number() {
            return Err(MathError::NotSquare);
        }
        let mut lu = Matrix::new(1, a.row_number(), 1, a.col_number())?;
        lu.initialize_from(a)?;

        let (status, lu, index, d) = recipes::lu_decompose(lu, min_pivot)?;
        Ok(Self { lu, index, d, done: status == 0 })
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Solve(B, X)`: returns `X`.
    pub fn solve(&self, b: &Vector) -> Result<Vector, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        recipes::lu_solve(&self.lu, &self.index, b.clone())
    }
    /// `Solve(X)`: solves in place and returns the updated `X`.
    pub fn solve_in_place(&self, x: Vector) -> Result<Vector, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        if x.length() != self.lu.row_number() {
            return Err(MathError::Dimension);
        }
        recipes::lu_solve(&self.lu, &self.index, x)
    }
    /// `Determinant()`. OCCT raises `StdFail_NotDone` if the decomposition failed; as this
    /// signature has no error channel, `NaN` is returned in that case.
    pub fn determinant(&self) -> f64 {
        if !self.done {
            return f64::NAN;
        }
        let mut result = self.d;
        for j in 1..=self.lu.upper_row() {
            result *= self.lu.at(j, j);
        }
        result
    }
    /// `Invert(Inv)`: writes the inverse into `inv` (same shape as the matrix).
    pub fn invert_into(&self, inv: &mut Matrix) -> Result<(), MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        if inv.row_number() != self.lu.row_number() || inv.col_number() != self.lu.col_number() {
            return Err(MathError::Dimension);
        }
        let n = self.lu.upper_row();
        let low_row = inv.lower_row();
        let low_col = inv.lower_col();
        for j in 1..=n {
            let mut column = Vector::filled(1, n, 0.0)?;
            column.put(j, 1.0);
            let column = recipes::lu_solve(&self.lu, &self.index, column)?;
            for i in 1..=self.lu.row_number() {
                inv.set_value(i + low_row - 1, j + low_col - 1, column.at(i))?;
            }
        }
        Ok(())
    }
    /// The inverse as a new matrix with the bounds `1..=n` x `1..=n`.
    pub fn inverse(&self) -> Result<Matrix, MathError> {
        let mut inv = Matrix::new(1, self.lu.row_number(), 1, self.lu.col_number())?;
        self.invert_into(&mut inv)?;
        Ok(inv)
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        if self.done {
            format!("math_Gauss  Status = Done \n Determinant of A = {}\n", self.d)
        } else {
            "math_Gauss  Status = not Done \n".to_string()
        }
    }
}

/// `math_Crout`: LDL^T decomposition of a symmetric matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct Crout {
    inv_a: Matrix,
    det: f64,
    done: bool,
}

impl Crout {
    /// `math_Crout(A, MinPivot)`; `MinPivot` is `1.0e-20` in OCCT's default argument.
    pub fn new(a: &Matrix, min_pivot: f64) -> Result<Self, MathError> {
        let nctl = a.row_number();
        if nctl != a.col_number() {
            return Err(MathError::NotSquare);
        }
        let lowr = a.lower_row();
        let lowc = a.lower_col();
        let mut inv_a = Matrix::new(1, nctl, 1, a.col_number())?;
        let mut l = Matrix::new(1, nctl, 1, nctl)?;
        let mut diag = Vector::new(1, nctl)?;
        let mut det = 1.0;
        for i in 1..=nctl {
            for j in 1..i {
                let mut scale = 0.0;
                for k in 1..j {
                    scale += l.at(i, k) * l.at(j, k) * diag.at(k);
                }
                l.put(i, j, (a.value(i + lowr - 1, j + lowc - 1)? - scale) / diag.at(j));
            }
            let mut scale = 0.0;
            for k in 1..i {
                scale += l.at(i, k) * l.at(i, k) * diag.at(k);
            }
            diag.put(i, a.value(i + lowr - 1, i + lowc - 1)? - scale);
            det *= diag.at(i);
            if diag.at(i).abs() <= min_pivot {
                return Ok(Self { inv_a, det, done: false });
            }
            l.put(i, i, 1.0);
        }

        // Inverse of L.
        l.put(1, 1, 1.0 / l.at(1, 1));
        for i in 2..=nctl {
            for k in 1..i {
                let mut scale = 0.0;
                for j in k..i {
                    scale += l.at(i, j) * l.at(j, k);
                }
                l.put(i, k, -scale / l.at(i, i));
            }
            l.put(i, i, 1.0 / l.at(i, i));
        }

        // Inverse of the matrix.
        for j in 1..=nctl {
            let mut scale = l.at(j, j) * l.at(j, j) / diag.at(j);
            for k in j + 1..=nctl {
                scale += l.at(k, j) * l.at(k, j) / diag.at(k);
            }
            inv_a.put(j, j, scale);
            for i in j + 1..=nctl {
                let mut scale = l.at(i, j) * l.at(i, i) / diag.at(i);
                for k in i + 1..=nctl {
                    scale += l.at(k, j) * l.at(k, i) / diag.at(k);
                }
                inv_a.put(i, j, scale);
            }
        }
        Ok(Self { inv_a, det, done: true })
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Solve(B, X)`: returns `X`, which has the bounds of `B`.
    pub fn solve(&self, b: &Vector) -> Result<Vector, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        let n = self.inv_a.row_number();
        if b.length() != n {
            return Err(MathError::Dimension);
        }
        let mut x = Vector::new(b.lower(), b.upper())?;
        for i in 1..=n {
            x.put(i, self.inv_a.at(i, 1) * b.at(1));
            for j in 2..=i {
                x.put(i, x.at(i) + self.inv_a.at(i, j) * b.at(j));
            }
            for j in i + 1..=n {
                x.put(i, x.at(i) + self.inv_a.at(j, i) * b.at(j));
            }
        }
        Ok(x)
    }
    /// `Determinant()`. OCCT raises `StdFail_NotDone` if the decomposition failed; as this
    /// signature has no error channel, `NaN` is returned in that case.
    pub fn determinant(&self) -> f64 {
        if self.done { self.det } else { f64::NAN }
    }
    /// `Inverse()`.
    pub fn inverse(&self) -> Result<Matrix, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        Ok(self.inv_a.clone())
    }
    /// `Invert(Inv)`: copies the inverse into `inv` (same shape).
    pub fn invert_into(&self, inv: &mut Matrix) -> Result<(), MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        inv.initialize_from(&self.inv_a)
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        if self.done {
            "math_Crout  Status = Done \n".to_string()
        } else {
            "math_Crout  Status = not Done \n".to_string()
        }
    }
}

/// `math_GaussLeastSquare`: least squares through the normal equations.
#[derive(Debug, Clone, PartialEq)]
pub struct GaussLeastSquare {
    lu: Matrix,
    a2: Matrix,
    index: IntegerVector,
    done: bool,
}

impl GaussLeastSquare {
    /// `math_GaussLeastSquare(A, MinPivot)`; `MinPivot` is `1.0e-20` in OCCT's default argument.
    pub fn new(a: &Matrix, min_pivot: f64) -> Result<Self, MathError> {
        let mut lu = Matrix::new(1, a.col_number(), 1, a.col_number())?;
        let mut a2 = Matrix::new(1, a.col_number(), 1, a.row_number())?;
        a2.initialize_from(&a.transposed()?)?;
        lu.multiply_from(&a2, a)?;
        let (status, lu, index, _) = recipes::lu_decompose(lu, min_pivot)?;
        Ok(Self { lu, a2, index, done: status == 0 })
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Solve(B, X)`: returns `X`.
    pub fn solve(&self, b: &Vector) -> Result<Vector, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        if b.length() != self.a2.col_number() {
            return Err(MathError::Dimension);
        }
        let mut x = Vector::new(1, self.a2.row_number())?;
        x.multiply_from_matrix(&self.a2, b)?;
        recipes::lu_solve(&self.lu, &self.index, x)
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        if self.done {
            "math_GaussLeastSquare  Status = Done \n".to_string()
        } else {
            "math_GaussLeastSquare Status = not Done \n".to_string()
        }
    }
}

/// `math_SVD`: singular value decomposition. As in OCCT, `solve` and `pseudo_inverse` zero the
/// singular values below the threshold in the object itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Svd {
    u: Matrix,
    v: Matrix,
    diag: RefCell<Vector>,
    row_a: i32,
    done: bool,
}

impl Svd {
    /// `math_SVD(A)`.
    pub fn new(a: &Matrix) -> Result<Self, MathError> {
        let mut u = Matrix::new(1, a.row_number().max(a.col_number()), 1, a.col_number())?;
        u.set_slice(1, a.row_number(), 1, a.col_number(), a)?;
        let (status, u, diag, v) = recipes::svd_decompose(u)?;
        Ok(Self { u, v, diag: RefCell::new(diag), row_a: a.row_number(), done: status == 0 })
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// Zeroes the singular values below `eps` times the largest one.
    fn threshold(&self, eps: f64) -> Result<(), MathError> {
        let mut diag = self.diag.borrow_mut();
        let wmin = eps * diag.value(diag.max_index())?;
        for i in 1..=diag.upper() {
            if diag.at(i) < wmin {
                diag.put(i, 0.0);
            }
        }
        Ok(())
    }
    /// `Solve(B, X, Eps)`: returns `X` (bounds `1..=ColNumber(A)`); OCCT default `Eps` is `1.0e-6`.
    pub fn solve(&self, b: &Vector, eps: f64) -> Result<Vector, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        if self.row_a != b.length() {
            return Err(MathError::Dimension);
        }
        let mut bb = Vector::filled(1, self.u.row_number(), 0.0)?;
        bb.set_slice(1, b.length(), b)?;
        self.threshold(eps)?;
        recipes::svd_solve(&self.u, &self.diag.borrow(), &self.v, &bb)
    }
    /// `PseudoInverse(Result, Eps)`: returns the `ColNumber(A)` x `RowNumber(A)` pseudo-inverse
    /// (bounds `1..`); OCCT default `Eps` is `1.0e-6`.
    pub fn pseudo_inverse(&self, eps: f64) -> Result<Matrix, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        self.threshold(eps)?;
        let diag = self.diag.borrow();
        let col_a = diag.length();
        let mut result = Matrix::new(1, col_a, 1, self.row_a)?;
        for j in 1..=self.row_a {
            let mut vnorme = Vector::filled(1, self.u.row_number(), 0.0)?;
            vnorme.put(j, 1.0);
            let column = recipes::svd_solve(&self.u, &diag, &self.v, &vnorme)?;
            for i in 1..=col_a {
                result.put(i, j, column.at(i));
            }
        }
        Ok(result)
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        if self.done { "math_SVD Status = Done \n".to_string() } else { "math_SVD Status = not Done \n".to_string() }
    }
}

/// `math_Jacobi`: eigenvalues and eigenvectors of a symmetric matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct Jacobi {
    result: JacobiResult,
    done: bool,
}

impl Jacobi {
    /// `math_Jacobi(A)`; the matrix must be square.
    pub fn new(a: &Matrix) -> Result<Self, MathError> {
        if a.row_number() != a.col_number() {
            return Err(MathError::NotSquare);
        }
        let mut aa = Matrix::new(1, a.row_number(), 1, a.row_number())?;
        aa.initialize_from(a)?;
        let result = recipes::jacobi(aa)?;
        let done = result.status == 0;
        Ok(Self { result, done })
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Values()`.
    pub fn values(&self) -> Result<Vector, MathError> {
        if self.done { Ok(self.result.values.clone()) } else { Err(MathError::NotDone) }
    }
    /// `Value(Num)`.
    pub fn value(&self, num: i32) -> Result<f64, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        self.result.values.value(num)
    }
    /// `Vectors()`: the eigenvectors, one per column.
    pub fn vectors(&self) -> Result<Matrix, MathError> {
        if self.done { Ok(self.result.vectors.clone()) } else { Err(MathError::NotDone) }
    }
    /// `Vector(Num, V)`: returns the eigenvector `Num` (column `Num` of the eigenvector matrix).
    pub fn vector(&self, num: i32) -> Result<Vector, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        self.result.vectors.col(num)
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        if self.done {
            format!("math_Jacobi  Status = Done \n The eigenvalues vector is: {}\n", self.result.values.dump())
        } else {
            "math_Jacobi Status = not Done \n".to_string()
        }
    }
}

/// `math_Householder`: least-squares solution through Householder reflections.
#[derive(Debug, Clone, PartialEq)]
pub struct Householder {
    sol: Matrix,
    done: bool,
}

impl Householder {
    /// `math_Householder(A, B, EPS)` with a vector right-hand side; OCCT default `EPS` is `1.0e-20`.
    pub fn new_vector(a: &Matrix, b: &Vector, eps: f64) -> Result<Self, MathError> {
        let mut b1 = Matrix::new(1, b.length(), 1, 1)?;
        b1.set_col(1, b)?;
        Self::perform(a, &b1, (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col()), eps)
    }
    /// `math_Householder(A, B, EPS)` with a matrix right-hand side.
    pub fn new_matrix(a: &Matrix, b: &Matrix, eps: f64) -> Result<Self, MathError> {
        Self::perform(a, b, (a.lower_row(), a.upper_row(), a.lower_col(), a.upper_col()), eps)
    }
    /// `math_Householder(A, B, lowerArow, upperArow, lowerAcol, upperAcol, EPS)`.
    pub fn new_matrix_with_range(
        a: &Matrix,
        b: &Matrix,
        lower_a_row: i32,
        upper_a_row: i32,
        lower_a_col: i32,
        upper_a_col: i32,
        eps: f64,
    ) -> Result<Self, MathError> {
        Self::perform(a, b, (lower_a_row, upper_a_row, lower_a_col, upper_a_col), eps)
    }
    /// `Perform(A, B, EPS)`, with the `A` window `(lowerArow, upperArow, lowerAcol, upperAcol)`.
    fn perform(a: &Matrix, b: &Matrix, window: (i32, i32, i32, i32), eps: f64) -> Result<Self, MathError> {
        let (mylowerarow, myupperarow, myloweracol, myupperacol) = window;
        let n = myupperacol - myloweracol + 1;
        let l = myupperarow - mylowerarow + 1;
        let m = b.col_number();
        let mut sol = Matrix::new(1, n, 1, m)?;
        let mut q = Matrix::new(1, l, 1, n)?;
        let mut b2 = Matrix::new(1, l, 1, m)?;
        if l != b.row_number() || n > l {
            return Err(MathError::Dimension);
        }
        let lbrow = b.lower_row();
        for i in 1..=l {
            for j in 1..=n {
                q.put(i, j, a.value(i + mylowerarow - 1, j + myloweracol - 1)?);
            }
            for j in 1..=m {
                b2.put(i, j, b.at(i + lbrow - lbrow, j));
            }
        }

        // Treatment of each column of A.
        for i in 1..=n {
            let mut h = 0.0;
            for k in i..=l {
                let qki = q.at(k, i);
                h += qki * qki;
            }
            let f = q.at(i, i);
            let g = if f < 1.0e-15 { h.sqrt() } else { -h.sqrt() };
            if g.abs() <= eps {
                return Ok(Self { sol, done: false });
            }
            h -= f * g;
            let alfaii = g - f;
            for j in i + 1..=n {
                let mut scale = 0.0;
                for k in i..=l {
                    scale += q.at(k, i) * q.at(k, j);
                }
                let cj = (g * q.at(i, j) - scale) / h;
                q.put(i, j, q.at(i, j) - alfaii * cj);
                for k in i + 1..=l {
                    q.put(k, j, q.at(k, j) + cj * q.at(k, i));
                }
            }

            // Modification of B.
            for j in 1..=m {
                let mut scale = q.at(i, i) * b2.at(i, j);
                for k in i + 1..=l {
                    scale += q.at(k, i) * b2.at(k, j);
                }
                let cj = (g * b2.at(i, j) - scale) / h;
                b2.put(i, j, b2.at(i, j) - cj * alfaii);
                for k in i + 1..=l {
                    b2.put(k, j, b2.at(k, j) + cj * q.at(k, i));
                }
            }
            q.put(i, i, g);
        }

        // Back substitution.
        for j in 1..=m {
            sol.put(n, j, b2.at(n, j) / q.at(n, n));
            for i in (1..n).rev() {
                let mut scale = 0.0;
                for k in i + 1..=n {
                    scale += q.at(i, k) * sol.at(k, j);
                }
                sol.put(i, j, (b2.at(i, j) - scale) / q.at(i, i));
            }
        }
        Ok(Self { sol, done: true })
    }
    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Value(sol, Index)`: returns column `Index` of the solution matrix.
    pub fn value(&self, index: i32) -> Result<Vector, MathError> {
        if !self.done {
            return Err(MathError::NotDone);
        }
        if index < 1 || index > self.sol.col_number() {
            return Err(MathError::Range);
        }
        self.sol.col(index)
    }
    /// `AllValues()`: the solution matrix.
    pub fn all_values(&self) -> Result<Matrix, MathError> {
        if self.done { Ok(self.sol.clone()) } else { Err(MathError::NotDone) }
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        if self.done {
            "math_Householder  Status = Done \n".to_string()
        } else {
            "math_Householder Status = not Done \n".to_string()
        }
    }
}
