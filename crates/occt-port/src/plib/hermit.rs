//! `PLib_HermitJacobi`: the Hermite polynomials of the end constraints followed by the Jacobi polynomials weighted
//! with `(1 - t^2)^(niv + 1)`.
use super::jacobi::JacobiPolynomial;
use super::{
    GeomAbsShape, PLibError, count, eval_polynomial, hermite_coefficients, ix, mul, need,
    no_derivative_eval_polynomial, rd, wr, zeros,
};
use crate::mathlin::Matrix;

/// `PLib_HermitJacobi`: a [`JacobiPolynomial`] with the Hermite basis of its constraint order.
#[derive(Debug, Clone)]
pub struct HermitJacobi {
    jacobi: JacobiPolynomial,
    /// The matrix of `PLib::HermiteCoefficients(-1, 1, niv, niv)`, row by row (`(2 * niv + 2)` square).
    hermite_matrix: Vec<f64>,
}

/// The coefficients of `W(t) = (1 - t^2)^(niv + 1)`.
fn w_coefficients(niv_constr: i32) -> &'static [f64] {
    match niv_constr {
        1 => &[1.0, 0.0, -2.0, 0.0, 1.0],
        2 => &[1.0, 0.0, -3.0, 0.0, 3.0, 0.0, -1.0],
        _ => &[1.0, 0.0, -1.0],
    }
}

impl HermitJacobi {
    /// `PLib_HermitJacobi(WorkDegree, ConstraintOrder)` (the errors of [`JacobiPolynomial::new`]).
    pub fn new(work_degree: i32, constraint_order: GeomAbsShape) -> Result<Self, PLibError> {
        let jacobi = JacobiPolynomial::new(work_degree, constraint_order)?;
        let niv = jacobi.niv_constr();
        let n = 2 * niv + 2;
        let mut matrix = Matrix::new(1, n, 1, n)?;
        hermite_coefficients(-1., 1., niv, niv, &mut matrix)?;
        let mut hermite_matrix = Vec::new();
        for row in 1..=n {
            for col in 1..=n {
                hermite_matrix.push(matrix.value(row, col)?);
            }
        }
        Ok(Self { jacobi, hermite_matrix })
    }

    /// `WorkDegree()`.
    pub fn work_degree(&self) -> i32 {
        self.jacobi.work_degree()
    }

    /// `NivConstr()`.
    pub fn niv_constr(&self) -> i32 {
        self.jacobi.niv_constr()
    }

    /// `MaxError`.
    pub fn max_error(&self, dimension: i32, herm_jac_coeff: &[f64], new_degree: i32) -> Result<f64, PLibError> {
        self.jacobi.max_error(dimension, herm_jac_coeff, new_degree)
    }

    /// `ReduceDegree`: `(new_degree, max_error)`.
    pub fn reduce_degree(
        &self,
        dimension: i32,
        max_degree: i32,
        tol: f64,
        herm_jac_coeff: &[f64],
    ) -> Result<(i32, f64), PLibError> {
        self.jacobi.reduce_degree(dimension, max_degree, tol, herm_jac_coeff)
    }

    /// `AverageError`.
    pub fn average_error(&self, dimension: i32, herm_jac_coeff: &[f64], new_degree: i32) -> Result<f64, PLibError> {
        self.jacobi.average_error(dimension, herm_jac_coeff, new_degree)
    }

    /// `aHermiteMatrix(row + 1, col + 1)` for 0-based `row`, `col`.
    fn hermite(&self, row: i32, col: i32) -> Result<f64, PLibError> {
        let n = ix(2 * self.niv_constr() + 2);
        rd(&self.hermite_matrix, ix(row) * n + ix(col))
    }

    /// The row `row` (0-based) of the Hermite matrix: the coefficients of the Hermite polynomial.
    fn hermite_row(&self, row: i32) -> Result<&[f64], PLibError> {
        let n = count(2 * self.niv_constr() + 2)?;
        let start = count(row)? * n;
        self.hermite_matrix.get(start..start + n).ok_or(PLibError::OutOfRange)
    }

    /// `ToCoefficients`: the first `2 * niv + 2` groups are in the Hermite basis, the rest Jacobi coefficients;
    /// `degree >= 2 * niv + 1`.
    pub fn to_coefficients(&self, dimension: i32, degree: i32, herm_jac_coeff: &[f64]) -> Result<Vec<f64>, PLibError> {
        let niv_constr = self.niv_constr();
        let degree_h = 2 * niv_constr + 1;
        // OCCT writes beyond its auxiliary array for a smaller degree
        if dimension < 1 || degree < degree_h || degree > 61 {
            return Err(PLibError::OutOfRange);
        }
        need(herm_jac_coeff.len(), mul(degree + 1, dimension)?)?;

        let mut aux_coeff = zeros(mul(degree + 1, dimension)?)?;
        for k in 0..=degree_h {
            let kdim = k * dimension;
            for i in 0..=niv_constr {
                let h1 = self.hermite(i, k)?;
                let h2 = self.hermite(i + niv_constr + 1, k)?;
                let i1 = i * dimension;
                let i2 = (i + niv_constr + 1) * dimension;
                for idim in 0..dimension {
                    let a = rd(&aux_coeff, ix(idim + kdim))?;
                    let v = a + (rd(herm_jac_coeff, ix(i1 + idim))? * h1 + rd(herm_jac_coeff, ix(i2 + idim))? * h2);
                    wr(&mut aux_coeff, ix(idim + kdim), v)?;
                }
            }
        }
        let kdim = (degree + 1) * dimension;
        for k in ((degree_h + 1) * dimension)..kdim {
            wr(&mut aux_coeff, ix(k), rd(herm_jac_coeff, ix(k))?)?;
        }

        if degree > degree_h { self.jacobi.to_coefficients(dimension, degree, &aux_coeff) } else { Ok(aux_coeff) }
    }

    /// `D0123` (the FORTRAN subroutine MPOBAS): `work_degree + 1` values per array (the arrays above `n_deriv` stay
    /// zero).
    fn d0123(&self, n_deriv: i32, u: f64) -> Result<[Vec<f64>; 4], PLibError> {
        let niv_constr = self.niv_constr();
        let work_degree = self.work_degree();
        let degree_h = 2 * niv_constr + 1;
        let n = count(work_degree + 1)?;
        let mut basis_value = zeros(n)?;
        let mut basis_d1 = zeros(n)?;
        let mut basis_d2 = zeros(n)?;
        let mut basis_d3 = zeros(n)?;
        let jac_degree = work_degree - degree_h - 1;
        let nd = count(n_deriv)?;

        // Evaluation des polynomes d'hermite: HermitValues(i, d) = hermit_values[i][d]
        let mut hermit_values: Vec<Vec<f64>> = Vec::new();
        for i in 0..=degree_h {
            let row = self.hermite_row(i)?;
            if n_deriv == 0 {
                hermit_values.push(no_derivative_eval_polynomial(u, degree_h, 1, degree_h, row)?);
            } else {
                hermit_values.push(eval_polynomial(u, n_deriv, degree_h, 1, row)?);
            }
        }

        // Evaluation des polynomes de Jaccobi
        let mut jac: [Vec<f64>; 4] = Default::default();
        let mut w_values = zeros(nd + 1)?;
        if jac_degree >= 0 {
            let [j0, j1, j2, j3] = match n_deriv {
                0 => [self.jacobi.d0(u), Vec::new(), Vec::new(), Vec::new()],
                1 => {
                    let (a, b) = self.jacobi.d1(u);
                    [a, b, Vec::new(), Vec::new()]
                }
                2 => {
                    let (a, b, c) = self.jacobi.d2(u);
                    [a, b, c, Vec::new()]
                }
                _ => {
                    let (a, b, c, d) = self.jacobi.d3(u);
                    [a, b, c, d]
                }
            };
            jac = [j0, j1, j2, j3];

            // Evaluation de W(t)
            let w_coeff = w_coefficients(niv_constr);
            if n_deriv == 0 {
                w_values = no_derivative_eval_polynomial(u, degree_h + 1, 1, degree_h + 1, w_coeff)?;
            } else {
                w_values = eval_polynomial(u, n_deriv, degree_h + 1, 1, w_coeff)?;
            }
        }
        let hermit = |i: i32, d: usize| -> Result<f64, PLibError> {
            let row = hermit_values.get(count(i)?).ok_or(PLibError::OutOfRange)?;
            row.get(d).copied().ok_or(PLibError::OutOfRange)
        };
        let jv = |k: usize, j: usize| -> Result<f64, PLibError> {
            jac.get(k).and_then(|a| a.get(j)).copied().ok_or(PLibError::OutOfRange)
        };
        let wv = |d: usize| -> Result<f64, PLibError> { w_values.get(d).copied().ok_or(PLibError::OutOfRange) };

        // Evaluation a l'ordre 0
        for i in 0..=degree_h {
            wr(&mut basis_value, ix(i), hermit(i, 0)?)?;
        }
        let w0 = wv(0)?;
        for (j, i) in ((degree_h + 1)..=work_degree).enumerate() {
            wr(&mut basis_value, ix(i), w0 * jv(0, j)?)?;
        }

        // Evaluation a l'ordre 1
        if n_deriv >= 1 {
            let w1 = wv(1)?;
            for i in 0..=degree_h {
                wr(&mut basis_d1, ix(i), hermit(i, 1)?)?;
            }
            for (j, i) in ((degree_h + 1)..=work_degree).enumerate() {
                wr(&mut basis_d1, ix(i), w0 * jv(1, j)? + w1 * jv(0, j)?)?;
            }
            // Evaluation a l'ordre 2
            if n_deriv >= 2 {
                let w2 = wv(2)?;
                for i in 0..=degree_h {
                    wr(&mut basis_d2, ix(i), hermit(i, 2)?)?;
                }
                for (j, i) in ((degree_h + 1)..=work_degree).enumerate() {
                    wr(&mut basis_d2, ix(i), w0 * jv(2, j)? + 2.0 * w1 * jv(1, j)? + w2 * jv(0, j)?)?;
                }

                // Evaluation a l'ordre 3
                if n_deriv == 3 {
                    let w3 = wv(3)?;
                    for i in 0..=degree_h {
                        wr(&mut basis_d3, ix(i), hermit(i, 3)?)?;
                    }
                    for (j, i) in ((degree_h + 1)..=work_degree).enumerate() {
                        let v = w0 * jv(3, j)? + w3 * jv(0, j)? + 3.0 * (w1 * jv(2, j)? + w2 * jv(1, j)?);
                        wr(&mut basis_d3, ix(i), v)?;
                    }
                }
            }
        }
        Ok([basis_value, basis_d1, basis_d2, basis_d3])
    }

    /// `D0(U, BasisValue)`: the `work_degree + 1` basis values at `u`.
    pub fn d0(&self, u: f64) -> Vec<f64> {
        let [v, _, _, _] = self.d0123(0, u).unwrap_or_default();
        v
    }

    /// `D1`.
    pub fn d1(&self, u: f64) -> (Vec<f64>, Vec<f64>) {
        let [v, d1, _, _] = self.d0123(1, u).unwrap_or_default();
        (v, d1)
    }

    /// `D2`.
    pub fn d2(&self, u: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let [v, d1, d2, _] = self.d0123(2, u).unwrap_or_default();
        (v, d1, d2)
    }

    /// `D3`.
    pub fn d3(&self, u: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let [v, d1, d2, d3] = self.d0123(3, u).unwrap_or_default();
        (v, d1, d2, d3)
    }
}
