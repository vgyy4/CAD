//! `PLib_JacobiPolynomial`: Jacobi basis polynomials with their Gauss weights, error estimates and the conversion
//! to the canonical basis.
use super::jacobi_coeffs as coeffs;
use super::jacobi_data as data;
use super::{Array2, GeomAbsShape, PLibError, count, ix, mul, need, niv_constr, rd, wr, zeros};
use crate::mathlin::{self, Vector};

/// The possible values of the number of Gauss points.
const NB_GAUSS_POINTS: [i32; 9] = [8, 10, 15, 20, 25, 30, 40, 50, 61];

const INVALID_VALUE: f64 = -999.0;

/// Maximum supported polynomial degree.
const MAX_DEGREE: i32 = 30;

/// `PLib_JacobiPolynomial`: the basis of the Jacobi polynomials for a constraint order and a work degree.
#[derive(Debug, Clone)]
pub struct JacobiPolynomial {
    work_degree: i32,
    niv_constr: i32,
    degree: i32,
}

/// `MaxValuesDB_Cx`.
fn max_values_db(niv_constr: i32) -> &'static [f64] {
    match niv_constr {
        0 => &data::MAX_VALUES_DB_C0,
        1 => &data::MAX_VALUES_DB_C1,
        _ => &data::MAX_VALUES_DB_C2,
    }
}

/// `TransMatrix_Cx` (both halves, the odd one from 496).
fn trans_matrix(niv_constr: i32) -> &'static [f64] {
    match niv_constr {
        0 => &data::TRANS_MATRIX_C0,
        1 => &data::TRANS_MATRIX_C1,
        _ => &data::TRANS_MATRIX_C2,
    }
}

/// The coefficient arrays of `GetJacobiCoefficients(niv_constr, degree)`: `(TNorm, CofA, CofB, Denom)`.
fn jacobi_coefficients(niv_constr: i32, degree: i32) -> Result<[&'static [f64]; 4], PLibError> {
    let (tnorm, cof_a, cof_b, denom): (&[f64], &[f64], &[f64], &[f64]) = match niv_constr {
        0 => (&coeffs::TNORM_C0, &coeffs::COFA_C0, &coeffs::COFB_C0, &coeffs::DENOM_C0),
        1 => (&coeffs::TNORM_C1, &coeffs::COFA_C1, &coeffs::COFB_C1, &coeffs::DENOM_C1),
        _ => (&coeffs::TNORM_C2, &coeffs::COFA_C2, &coeffs::COFB_C2, &coeffs::DENOM_C2),
    };
    let d = count(degree)?;
    let start = d * (d + 1) / 2;
    let range = start..start + d + 1;
    let get = |t: &'static [f64]| t.get(range.clone()).ok_or(PLibError::OutOfRange);
    Ok([get(tnorm)?, get(cof_a)?, get(cof_b)?, get(denom)?])
}

impl JacobiPolynomial {
    /// `PLib_JacobiPolynomial(WorkDegree, ConstraintOrder)`: `Err(Construction)` for a constraint order other than
    /// `C0/C1/C2`, or a work degree outside `2 * (niv + 1) ..= 30 + 2 * (niv + 1)`.
    pub fn new(work_degree: i32, constraint_order: GeomAbsShape) -> Result<Self, PLibError> {
        let niv_constr = niv_constr(constraint_order)?;
        let degree = work_degree.checked_sub(2 * (niv_constr + 1)).ok_or(PLibError::Construction)?;
        if degree < 0 {
            return Err(PLibError::Construction);
        }
        if degree > MAX_DEGREE {
            return Err(PLibError::Construction);
        }
        Ok(Self { work_degree, niv_constr, degree })
    }

    /// `WorkDegree()`.
    pub fn work_degree(&self) -> i32 {
        self.work_degree
    }

    /// `NivConstr()`.
    pub fn niv_constr(&self) -> i32 {
        self.niv_constr
    }

    /// `Points(NbGaussPoints, TabPoints)`: the positive Gauss points, increasing, in `TabPoints(0..=n/2)`
    /// (element 0 is `0.0` for an odd and `-999.0` for an even number of points).
    pub fn points(&self, nb_gauss_points: i32) -> Result<Vec<f64>, PLibError> {
        if !NB_GAUSS_POINTS.contains(&nb_gauss_points) || nb_gauss_points <= self.degree {
            return Err(PLibError::Construction);
        }
        let decreasing = mathlin::gauss_points(nb_gauss_points, 1)?;
        let half = nb_gauss_points / 2;
        let mut tab = zeros(count(half + 1)?)?;
        // TabPoints consist of only positive increasing values
        for i in 1..=half {
            wr(&mut tab, ix(i), decreasing.value(half - i + 1)?)?;
        }
        if nb_gauss_points % 2 == 1 {
            wr(&mut tab, 0, 0.)?;
        } else {
            wr(&mut tab, 0, INVALID_VALUE)?;
        }
        Ok(tab)
    }

    /// `Weights(NbGaussPoints, TabWeights)`: `TabWeights(0..=n/2, 0..=degree)` as an `(n/2 + 1) x (degree + 1)` array.
    pub fn weights(&self, nb_gauss_points: i32) -> Result<Array2<f64>, PLibError> {
        // OCCT then reads the weights of another number of points
        if !NB_GAUSS_POINTS.contains(&nb_gauss_points) || nb_gauss_points <= self.work_degree {
            return Err(PLibError::OutOfRange);
        }
        let rows = count(nb_gauss_points / 2 + 1)?;
        let cols = count(self.degree + 1)?;
        let mut tab = zeros(rows * cols)?;
        let at = |i: i32, j: i32| ix(i) * ix(self.degree + 1) + ix(j);

        let (weights_db, weights_db0): (&[f64], &[f64]) = match self.niv_constr {
            0 => (&data::WEIGHTS_DB_C0, &data::WEIGHTS_DB0_C0),
            1 => (&data::WEIGHTS_DB_C1, &data::WEIGHTS_DB0_C1),
            _ => (&data::WEIGHTS_DB_C2, &data::WEIGHTS_DB0_C2),
        };
        let min_degree = 2 * (self.niv_constr + 1);

        // Calculate offset into the weights database
        let mut db: isize = 0;
        if nb_gauss_points > 8 {
            db += ix(8 * (8 - min_degree) / 2);
        }
        if nb_gauss_points > 10 {
            db += ix(10 * (10 - min_degree) / 2);
        }
        if nb_gauss_points > 15 {
            db += ix(((15 - 1) / 2) * (15 - min_degree));
        }
        if nb_gauss_points > 20 {
            db += ix(20 * (20 - min_degree) / 2);
        }
        if nb_gauss_points > 25 {
            db += ix(((25 - 1) / 2) * (25 - min_degree));
        }
        if nb_gauss_points > 30 {
            db += ix(30 * (30 - min_degree) / 2);
        }
        if nb_gauss_points > 40 {
            db += ix(40 * (40 - min_degree) / 2);
        }
        if nb_gauss_points > 50 {
            db += ix(50 * (50 - min_degree) / 2);
        }

        // Copy TabWeightsDB into TabWeights
        let half_points = nb_gauss_points / 2;
        for j in 0..=self.degree {
            for i in 1..=half_points {
                wr(&mut tab, at(i, j), rd(weights_db, db)?)?;
                db += 1;
            }
        }

        if nb_gauss_points % 2 == 1 {
            // NbGaussPoints is odd - fill row 0 with special values
            let mut db0: isize = 0;
            if nb_gauss_points > 15 {
                db0 += ix((15 - 1 - min_degree) / 2 + 1);
            }
            if nb_gauss_points > 25 {
                db0 += ix((25 - 1 - min_degree) / 2 + 1);
            }
            // Overwrite even columns of row 0 with data from database
            let mut j = 0;
            while j <= self.degree {
                wr(&mut tab, at(0, j), rd(weights_db0, db0)?)?;
                db0 += 1;
                j += 2;
            }
        } else {
            // Fill row 0 with INVALID_VALUE for even NbGaussPoints
            for j in 0..=self.degree {
                wr(&mut tab, at(0, j), INVALID_VALUE)?;
            }
        }
        Array2::new(rows, cols, tab)
    }

    /// `MaxValue(TabMax)`: the first `len` entries of `MaxValuesDB`.
    pub fn max_value(&self, len: i32) -> Result<Vec<f64>, PLibError> {
        let db = max_values_db(self.niv_constr);
        if len < 1 {
            return Err(PLibError::OutOfRange);
        }
        Ok(db.get(..count(len)?).ok_or(PLibError::OutOfRange)?.to_vec())
    }

    /// The checks of the error functions: `dimension >= 1` and `groups` groups of `dimension` coefficients.
    fn check_coeffs(dimension: i32, groups: i32, jac_coeff: &[f64]) -> Result<(), PLibError> {
        if dimension < 1 {
            return Err(PLibError::OutOfRange);
        }
        need(jac_coeff.len(), mul(groups, dimension)?)
    }

    /// `MaxError(Dimension, JacCoeff, NewDegree)`.
    pub fn max_error(&self, dimension: i32, jac_coeff: &[f64], new_degree: i32) -> Result<f64, PLibError> {
        Self::check_coeffs(dimension, self.work_degree + 1, jac_coeff)?;
        let tab_max = self.max_value(self.degree + 2)?;

        let beg_idx = 2 * (self.niv_constr + 1);
        let cut_idx = beg_idx.max(new_degree.saturating_add(1));

        let mut max_err_dim = Vector::filled(1, dimension, 0.)?;
        for dim_idx in 1..=dimension {
            for coeff_idx in cut_idx..=self.work_degree {
                let coeff_value = rd(jac_coeff, ix(coeff_idx) * ix(dimension) + ix(dim_idx) - 1)?;
                let basis_max = rd(&tab_max, ix(coeff_idx - beg_idx))?;
                let v = max_err_dim.value(dim_idx)? + coeff_value.abs() * basis_max;
                max_err_dim.set_value(dim_idx, v)?;
            }
        }
        Ok(max_err_dim.norm())
    }

    /// `ReduceDegree(Dimension, MaxDegree, Tol, JacCoeff, NewDegree, MaxError)`: `(new_degree, max_error)`.
    pub fn reduce_degree(
        &self,
        dimension: i32,
        max_degree: i32,
        tol: f64,
        jac_coeff: &[f64],
    ) -> Result<(i32, f64), PLibError> {
        Self::check_coeffs(dimension, self.work_degree + 1, jac_coeff)?;
        let idx = 2 * (self.niv_constr + 1) - 1;
        let cut_idx = idx + 1;

        let mut max_err_dim = Vector::filled(1, dimension, 0.)?;
        let tab_max = self.max_value(self.degree + 2)?;

        let mut new_degree = idx;
        let mut max_error = 0.;

        // Search for NewDegree from high degree to low
        for i in (cut_idx..=self.work_degree).rev() {
            // Accumulate error contribution for all dimensions
            let i_offset = ix(i) * ix(dimension);
            for idim in 1..=dimension {
                let v = max_err_dim.value(idim)?
                    + rd(jac_coeff, i_offset + ix(idim) - 1)?.abs() * rd(&tab_max, ix(i - cut_idx))?;
                max_err_dim.set_value(idim, v)?;
            }

            let error = max_err_dim.norm();
            if error > tol && i <= max_degree {
                new_degree = i;
                break;
            }
            max_error = error;
        }

        // Fallback: find last non-negligible coefficient
        if new_degree == idx {
            const EPS: f64 = 1.0e-9;
            new_degree = 0;
            for i in (1..=idx).rev() {
                let mut bid = 0.;
                let i_offset = ix(i) * ix(dimension);
                for idim in 1..=dimension {
                    bid += rd(jac_coeff, i_offset + ix(idim) - 1)?.abs();
                }
                if bid > EPS {
                    new_degree = i;
                    break;
                }
            }
        }
        Ok((new_degree, max_error))
    }

    /// `AverageError(Dimension, JacCoeff, NewDegree)`.
    pub fn average_error(&self, dimension: i32, jac_coeff: &[f64], new_degree: i32) -> Result<f64, PLibError> {
        Self::check_coeffs(dimension, self.degree + 1, jac_coeff)?;
        let cut_idx = (2 * (self.niv_constr + 1) + 1).max(new_degree.saturating_add(1));
        let mut average_err = 0.;

        // Compute sum of squares of coefficients beyond NewDegree
        for idim in 1..=dimension {
            for i in cut_idx..=self.degree {
                let c = rd(jac_coeff, ix(i) * ix(dimension) + ix(idim) - 1)?;
                average_err += c * c;
            }
        }
        Ok((average_err / 2.).sqrt())
    }

    /// `ToCoefficients(Dimension, Degree, JacCoeff, Coefficients)`: converts the `degree + 1` groups from the
    /// Jacobi basis to the canonical basis.
    pub fn to_coefficients(&self, dimension: i32, degree: i32, jac_coeff: &[f64]) -> Result<Vec<f64>, PLibError> {
        if !(0..=61).contains(&degree) {
            return Err(PLibError::OutOfRange);
        }
        Self::check_coeffs(dimension, degree + 1, jac_coeff)?;
        const MAX_M: i32 = MAX_DEGREE + 1;
        let half_degree = degree / 2;
        let double_dim = 2 * dimension;
        let mut coefficients = zeros(mul(degree + 1, dimension)?)?;
        let table = trans_matrix(self.niv_constr);
        let mut tr_pointer: isize = 0;

        // Convert even elements of JacCoeff
        for i in 0..=half_degree {
            let i_ptr_idx = i * MAX_M - (i + 1) * i / 2;
            let i_coeff_offset = double_dim * i;
            for idim in 1..=dimension {
                let mut value = 0.;
                for j in i..=half_degree {
                    value += rd(table, tr_pointer + ix(i_ptr_idx + j))? * rd(jac_coeff, ix(double_dim * j + idim - 1))?;
                }
                wr(&mut coefficients, ix(i_coeff_offset + idim - 1), value)?;
            }
        }

        if degree == 0 {
            return Ok(coefficients);
        }

        // Convert odd elements of JacCoeff
        tr_pointer += ix(MAX_M * (MAX_M + 1) / 2);
        let half_degree_minus1 = (degree - 1) / 2;

        for i in 0..=half_degree_minus1 {
            let i_ptr_idx = i * MAX_M - (i + 1) * i / 2;
            let i_base_idx = (2 * i + 1) * dimension;
            let j_base_idx = (2 * i + 1) * dimension;
            for idim in 1..=dimension {
                let mut value = 0.;
                let mut jj = j_base_idx + idim - 1;
                for j in i..=half_degree_minus1 {
                    value += rd(table, tr_pointer + ix(i_ptr_idx + j))? * rd(jac_coeff, ix(jj))?;
                    jj += double_dim;
                }
                wr(&mut coefficients, ix(i_base_idx + idim - 1), value)?;
            }
        }
        Ok(coefficients)
    }

    /// `D0123` (the FORTRAN subroutine MPOJAC): the basis values and the derivatives up to `n_deriv`, each
    /// `degree + 1` values (the arrays above `n_deriv` stay zero).
    fn d0123(&self, n_deriv: i32, u: f64) -> Result<[Vec<f64>; 4], PLibError> {
        let hermit_niv_constr = 2 * (self.niv_constr + 1);
        let n = count(self.degree + 1)?;
        let mut basis_value = zeros(n)?;
        let mut basis_d1 = zeros(n)?;
        let mut basis_d2 = zeros(n)?;
        let mut basis_d3 = zeros(n)?;

        // Get pre-computed coefficients
        let [tnorm, cof_a_tab, cof_b_tab, denom_tab] = jacobi_coefficients(self.niv_constr, self.degree)?;

        //  --- Positionements triviaux -----
        if self.degree == 0 {
            wr(&mut basis_value, 0, 1.)?;
            if n_deriv >= 1 {
                wr(&mut basis_d1, 0, 0.)?;
                if n_deriv >= 2 {
                    wr(&mut basis_d2, 0, 0.)?;
                    if n_deriv == 3 {
                        wr(&mut basis_d3, 0, 0.)?;
                    }
                }
            }
        } else {
            wr(&mut basis_value, 0, 1.)?;
            let aux = f64::from(hermit_niv_constr + 1);
            wr(&mut basis_value, 1, aux * u)?;
            if n_deriv >= 1 {
                wr(&mut basis_d1, 0, 0.)?;
                wr(&mut basis_d1, 1, aux)?;
                if n_deriv >= 2 {
                    wr(&mut basis_d2, 0, 0.)?;
                    wr(&mut basis_d2, 1, 0.)?;
                    if n_deriv == 3 {
                        wr(&mut basis_d3, 0, 0.)?;
                        wr(&mut basis_d3, 1, 0.)?;
                    }
                }
            }
        }

        //  --- Positionement par reccurence
        if self.degree > 1 {
            if n_deriv == 0 {
                for i in 2..=self.degree {
                    let i = ix(i);
                    let v = (rd(cof_a_tab, i)? * u * rd(&basis_value, i - 1)?
                        + rd(cof_b_tab, i)? * rd(&basis_value, i - 2)?)
                        * rd(denom_tab, i)?;
                    wr(&mut basis_value, i, v)?;
                }
            } else {
                for i in 2..=self.degree {
                    let i = ix(i);
                    let cof_a = rd(cof_a_tab, i)?;
                    let cof_b = rd(cof_b_tab, i)?;
                    let denom = rd(denom_tab, i)?;

                    let v = (cof_a * u * rd(&basis_value, i - 1)? + cof_b * rd(&basis_value, i - 2)?) * denom;
                    wr(&mut basis_value, i, v)?;
                    let d1 = (cof_a * (u * rd(&basis_d1, i - 1)? + rd(&basis_value, i - 1)?)
                        + cof_b * rd(&basis_d1, i - 2)?)
                        * denom;
                    wr(&mut basis_d1, i, d1)?;
                    if n_deriv >= 2 {
                        let d2 = (cof_a * (u * rd(&basis_d2, i - 1)? + 2.0 * rd(&basis_d1, i - 1)?)
                            + cof_b * rd(&basis_d2, i - 2)?)
                            * denom;
                        wr(&mut basis_d2, i, d2)?;
                        if n_deriv == 3 {
                            let d3 = (cof_a * (u * rd(&basis_d3, i - 1)? + 3.0 * rd(&basis_d2, i - 1)?)
                                + cof_b * rd(&basis_d3, i - 2)?)
                                * denom;
                            wr(&mut basis_d3, i, d3)?;
                        }
                    }
                }
            }
        }

        // Normalization
        if n_deriv == 0 {
            for i in 0..=ix(self.degree) {
                let v = rd(&basis_value, i)? * rd(tnorm, i)?;
                wr(&mut basis_value, i, v)?;
            }
        } else {
            for i in 0..=ix(self.degree) {
                let norm_value = rd(tnorm, i)?;
                let v = rd(&basis_value, i)? * norm_value;
                wr(&mut basis_value, i, v)?;
                let v = rd(&basis_d1, i)? * norm_value;
                wr(&mut basis_d1, i, v)?;
                if n_deriv >= 2 {
                    let v = rd(&basis_d2, i)? * norm_value;
                    wr(&mut basis_d2, i, v)?;
                    if n_deriv >= 3 {
                        let v = rd(&basis_d3, i)? * norm_value;
                        wr(&mut basis_d3, i, v)?;
                    }
                }
            }
        }
        Ok([basis_value, basis_d1, basis_d2, basis_d3])
    }

    /// `D0(U, BasisValue)`: the `degree + 1` basis values at `u`.
    pub fn d0(&self, u: f64) -> Vec<f64> {
        let [v, _, _, _] = self.d0123(0, u).unwrap_or_default();
        v
    }

    /// `D1(U, BasisValue, BasisD1)`.
    pub fn d1(&self, u: f64) -> (Vec<f64>, Vec<f64>) {
        let [v, d1, _, _] = self.d0123(1, u).unwrap_or_default();
        (v, d1)
    }

    /// `D2(U, BasisValue, BasisD1, BasisD2)`.
    pub fn d2(&self, u: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let [v, d1, d2, _] = self.d0123(2, u).unwrap_or_default();
        (v, d1, d2)
    }

    /// `D3(U, BasisValue, BasisD1, BasisD2, BasisD3)`.
    pub fn d3(&self, u: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let [v, d1, d2, d3] = self.d0123(3, u).unwrap_or_default();
        (v, d1, d2, d3)
    }
}
