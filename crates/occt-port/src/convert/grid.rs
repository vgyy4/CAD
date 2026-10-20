//! `Convert_GridPolynomialToPoles`: the poles of the B-spline surface that interpolates a grid of polynomial patches.

use super::{ConvertError, R, at, len_of, put};
use crate::bsplclib;
use crate::bsplslib;
use crate::gp::Pnt;
use crate::plib::{self, Array2};

/// The B-spline surface (poles, knots, multiplicities) equivalent to a grid of polynomial surface patches, computed
/// by interpolation at the Schoenberg points of each direction.
#[derive(Debug, Clone, PartialEq)]
pub struct GridPolynomialToPoles {
    poles: Array2<Pnt>,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    u_mults: Vec<i32>,
    v_mults: Vec<i32>,
    u_degree: i32,
    v_degree: i32,
    done: bool,
}

/// The one-direction tables of `BuildArray`: the multiplicities, the flat knots and the interpolation parameters.
struct Direction {
    mults: Vec<i32>,
    flat_knots: Vec<f64>,
    parameters: Vec<f64>,
}

/// `BuildArray(Degree, Knots, Continuity, FlatKnots, Mults, Parameters)`.
fn build_array(degree: i32, knots: &[f64], continuity: i32) -> R<Direction> {
    let num_curves = i32::try_from(knots.len()).map_err(|_| ConvertError::Argument)? - 1;
    if num_curves < 1 {
        return Err(ConvertError::Argument);
    }
    // Multiplicities
    let multiplicities = degree - continuity;
    if num_curves > 1 && multiplicities < 0 {
        // negative multiplicities corrupt OCCT's memory
        return Err(ConvertError::Argument);
    }
    let mut mults = vec![multiplicities; knots.len()];
    if let Some(m) = mults.first_mut() {
        *m = degree + 1;
    }
    put(&mut mults, i64::from(num_curves) + 1, degree + 1)?;
    // Flat knots
    let num_flat_knots = i64::from(multiplicities) * (i64::from(num_curves) - 1) + 2 * i64::from(degree) + 2;
    let flat_knots = bsplclib::knot_sequence(knots, &mults, degree, false)?;
    if i64::try_from(flat_knots.len()).ok() != Some(num_flat_knots) {
        return Err(ConvertError::Argument);
    }
    // Number of poles and the interpolation parameters
    let num_poles = i32::try_from(num_flat_knots - i64::from(degree) - 1).map_err(|_| ConvertError::Argument)?;
    let parameters = bsplclib::build_schoenberg_points(degree, &flat_knots, num_poles)?;
    Ok(Direction { mults, flat_knots, parameters })
}

/// The index `index` such that the parameter lies in `[true_intervals(index), true_intervals(index + 1)]`, advancing
/// from `index`, and the parameter mapped into the polynomial interval (`polynomial_intervals(1)`, `(2)`).
fn locate_in_patch(
    parameter: f64,
    index: &mut i64,
    nb_knots: usize,
    true_intervals: &[f64],
    polynomial_intervals: &[f64],
) -> R<f64> {
    let last = i64::try_from(nb_knots).map_err(|_| ConvertError::Argument)? - 1;
    while parameter > at(true_intervals, *index + 1)? && *index < last {
        *index += 1;
    }
    let n_value =
        (parameter - at(true_intervals, *index)?) / (at(true_intervals, *index + 1)? - at(true_intervals, *index)?);
    Ok((1.0 - n_value) * at(polynomial_intervals, 1)? + n_value * at(polynomial_intervals, 2)?)
}

impl GridPolynomialToPoles {
    /// `Convert_GridPolynomialToPoles(MaxUDegree, MaxVDegree, NumCoeff, Coefficients, PolynomialUIntervals,
    /// PolynomialVIntervals)`: one patch.
    ///
    /// `Err(Domain)` for `num_coeff.len() != 2`, `coefficients.len() != 3*(max_u_degree+1)*(max_v_degree+1)` and a
    /// degree above the maximum; `Err(Argument)` for an interval slice of fewer than 2 values (OCCT 8.0.1 reads outside
    /// it); `Err(Construction)` for a singular interpolation.
    pub fn single(
        max_u_degree: i32,
        max_v_degree: i32,
        num_coeff: &[i32],
        coefficients: &[f64],
        polynomial_u_intervals: &[f64],
        polynomial_v_intervals: &[f64],
    ) -> R<Self> {
        let [nu, nv] = num_coeff else {
            return Err(ConvertError::Domain);
        };
        let expected = (i64::from(max_u_degree) + 1) * (i64::from(max_v_degree) + 1) * 3;
        if max_u_degree < 0 || max_v_degree < 0 || i64::try_from(coefficients.len()).ok() != Some(expected) {
            return Err(ConvertError::Domain);
        }
        let u_degree = nu - 1;
        let v_degree = nv - 1;
        if u_degree > max_u_degree || v_degree > max_v_degree {
            return Err(ConvertError::Domain);
        }
        if polynomial_u_intervals.len() < 2 || polynomial_v_intervals.len() < 2 {
            return Err(ConvertError::Argument);
        }
        Self::perform(
            u_degree,
            v_degree,
            0,
            0,
            max_u_degree,
            max_v_degree,
            &[[*nu, *nv]],
            coefficients,
            polynomial_u_intervals,
            polynomial_v_intervals,
            polynomial_u_intervals,
            polynomial_v_intervals,
        )
    }

    /// `Convert_GridPolynomialToPoles(NbUSurfaces, NbVSurfaces, UContinuity, VContinuity, MaxUDegree, MaxVDegree,
    /// NumCoeffPerSurface, Coefficients, PolynomialUIntervals, PolynomialVIntervals, TrueUIntervals,
    /// TrueVIntervals)`: a grid of `nb_u_surfaces` by `nb_v_surfaces` patches (patch `p`, u fastest).
    ///
    /// `Err(Domain)` for inconsistent sizes of `num_coeff_per_surface` and `coefficients` or a degree above the
    /// maximum; `Err(Argument)` for the inputs OCCT 8.0.1 does not check (short interval slices, a continuity above
    /// the degree with several surfaces); `Err(Construction)` for a singular interpolation.
    #[allow(clippy::too_many_arguments)]
    pub fn multi(
        nb_u_surfaces: i32,
        nb_v_surfaces: i32,
        u_continuity: i32,
        v_continuity: i32,
        max_u_degree: i32,
        max_v_degree: i32,
        num_coeff_per_surface: &[[i32; 2]],
        coefficients: &[f64],
        polynomial_u_intervals: &[f64],
        polynomial_v_intervals: &[f64],
        true_u_intervals: &[f64],
        true_v_intervals: &[f64],
    ) -> R<Self> {
        if nb_u_surfaces < 1 || nb_v_surfaces < 1 {
            return Err(ConvertError::Argument);
        }
        let real_u_degree = i64::from(max_u_degree).max(2 * i64::from(u_continuity) + 1);
        let real_v_degree = i64::from(max_v_degree).max(2 * i64::from(v_continuity) + 1);
        let nb_surfaces = i64::from(nb_u_surfaces) * i64::from(nb_v_surfaces);
        if i64::try_from(num_coeff_per_surface.len()).ok() != Some(nb_surfaces) {
            return Err(ConvertError::Domain);
        }
        let expected = nb_surfaces
            .checked_mul(3 * (real_u_degree + 1))
            .and_then(|x| x.checked_mul(real_v_degree + 1))
            .ok_or(ConvertError::Argument)?;
        if i64::try_from(coefficients.len()).ok() != Some(expected) {
            return Err(ConvertError::Domain);
        }
        // Compute actual degrees from the supplied coefficient counts.
        let (mut u_degree, mut v_degree) = (0, 0);
        for [nu, nv] in num_coeff_per_surface {
            if *nu > u_degree + 1 {
                u_degree = *nu - 1;
            }
            if *nv > v_degree + 1 {
                v_degree = *nv - 1;
            }
        }
        if i64::from(u_degree) > real_u_degree || i64::from(v_degree) > real_v_degree {
            return Err(ConvertError::Domain);
        }
        if polynomial_u_intervals.len() < 2
            || polynomial_v_intervals.len() < 2
            || i64::try_from(true_u_intervals.len()).ok() != Some(i64::from(nb_u_surfaces) + 1)
            || i64::try_from(true_v_intervals.len()).ok() != Some(i64::from(nb_v_surfaces) + 1)
        {
            return Err(ConvertError::Argument);
        }
        Self::perform(
            u_degree,
            v_degree,
            u_continuity,
            v_continuity,
            i32::try_from(real_u_degree).map_err(|_| ConvertError::Argument)?,
            i32::try_from(real_v_degree).map_err(|_| ConvertError::Argument)?,
            num_coeff_per_surface,
            coefficients,
            polynomial_u_intervals,
            polynomial_v_intervals,
            true_u_intervals,
            true_v_intervals,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn perform(
        u_degree: i32,
        v_degree: i32,
        u_continuity: i32,
        v_continuity: i32,
        max_u_degree: i32,
        max_v_degree: i32,
        num_coeff_per_surface: &[[i32; 2]],
        coefficients: &[f64],
        polynomial_u_intervals: &[f64],
        polynomial_v_intervals: &[f64],
        true_u_intervals: &[f64],
        true_v_intervals: &[f64],
    ) -> R<Self> {
        // (1) Build monodimensional knot/multiplicity/parameter tables.
        let u_knots = true_u_intervals.to_vec();
        let v_knots = true_v_intervals.to_vec();
        let u = build_array(u_degree, &u_knots, u_continuity)?;
        let v = build_array(v_degree, &v_knots, v_continuity)?;

        // (2) Digitalisation
        let dimension = 3 * (v_degree + 1);
        let siz_patch = 3 * i64::from(max_u_degree + 1) * i64::from(max_v_degree + 1);
        let mut patch = vec![0.0; len_of((u_degree + 1) * dimension)?];
        let mut poles = Vec::with_capacity(u.parameters.len() * v.parameters.len());
        let mut patch_indice = 0i64;
        let mut u_index = 1i64;
        for u_parameter in &u.parameters {
            let u_value =
                locate_in_patch(*u_parameter, &mut u_index, u_knots.len(), true_u_intervals, polynomial_u_intervals)?;
            let mut v_index = 1i64;
            for v_parameter in &v.parameters {
                let v_value = locate_in_patch(
                    *v_parameter,
                    &mut v_index,
                    v_knots.len(),
                    true_v_intervals,
                    polynomial_v_intervals,
                )?;
                // (2.1) Extract the active patch coefficients.
                let current =
                    u_index + (i64::try_from(u_knots.len()).map_err(|_| ConvertError::Argument)? - 1) * (v_index - 1);
                if patch_indice != current {
                    patch_indice = current;
                    let [n1, n2] = at(num_coeff_per_surface, patch_indice)?;
                    let mut ll = 1i64;
                    for k1 in 1..=i64::from(n1) {
                        let mut pos = siz_patch * (patch_indice - 1) + 3 * i64::from(max_v_degree + 1) * (k1 - 1) + 1;
                        for _ in 1..=n2 {
                            put(&mut patch, ll, at(coefficients, pos)?)?;
                            put(&mut patch, ll + 1, at(coefficients, pos + 1)?)?;
                            put(&mut patch, ll + 2, at(coefficients, pos + 2)?)?;
                            ll += 3;
                            pos += 3;
                        }
                    }
                }
                // (2.2) Evaluate at (u_value, v_value).
                let [n1, n2] = at(num_coeff_per_surface, patch_indice)?;
                let digit = plib::eval_poly2var(u_value, v_value, 0, 0, n1 - 1, n2 - 1, 3, &patch)?;
                match digit.as_slice() {
                    [x, y, z] => poles.push(Pnt::new(*x, *y, *z)),
                    _ => return Err(ConvertError::OutOfRange),
                }
            }
        }
        let poles = Array2::new(u.parameters.len(), v.parameters.len(), poles)?;

        // (3) Interpolation
        let (inversion_problem, poles) = bsplslib::interpolate(
            u_degree,
            v_degree,
            &u.flat_knots,
            &v.flat_knots,
            &u.parameters,
            &v.parameters,
            &poles,
        )?;
        if inversion_problem != 0 {
            return Err(ConvertError::Construction);
        }
        Ok(Self { poles, u_knots, v_knots, u_mults: u.mults, v_mults: v.mults, u_degree, v_degree, done: true })
    }

    /// Whether the conversion is done (always, after a constructor succeeded).
    pub fn is_done(&self) -> bool {
        self.done
    }

    fn check_done(&self) -> R<()> {
        if self.done { Ok(()) } else { Err(ConvertError::NotDone) }
    }

    /// The number of poles in U; `Err(NotDone)` unless `is_done()`.
    pub fn nb_u_poles(&self) -> R<i32> {
        self.check_done()?;
        i32::try_from(self.poles.rows()).map_err(|_| ConvertError::Argument)
    }

    /// The number of poles in V.
    pub fn nb_v_poles(&self) -> R<i32> {
        self.check_done()?;
        i32::try_from(self.poles.cols()).map_err(|_| ConvertError::Argument)
    }

    /// The poles (rows are the U index).
    pub fn poles(&self) -> R<&Array2<Pnt>> {
        self.check_done()?;
        Ok(&self.poles)
    }

    /// The degree in U.
    pub fn u_degree(&self) -> R<i32> {
        self.check_done()?;
        Ok(self.u_degree)
    }

    /// The degree in V.
    pub fn v_degree(&self) -> R<i32> {
        self.check_done()?;
        Ok(self.v_degree)
    }

    /// The number of knots in U.
    pub fn nb_u_knots(&self) -> R<i32> {
        self.check_done()?;
        i32::try_from(self.u_knots.len()).map_err(|_| ConvertError::Argument)
    }

    /// The number of knots in V.
    pub fn nb_v_knots(&self) -> R<i32> {
        self.check_done()?;
        i32::try_from(self.v_knots.len()).map_err(|_| ConvertError::Argument)
    }

    /// The U knots.
    pub fn u_knots(&self) -> R<&[f64]> {
        self.check_done()?;
        Ok(&self.u_knots)
    }

    /// The V knots.
    pub fn v_knots(&self) -> R<&[f64]> {
        self.check_done()?;
        Ok(&self.v_knots)
    }

    /// The U multiplicities.
    pub fn u_multiplicities(&self) -> R<&[i32]> {
        self.check_done()?;
        Ok(&self.u_mults)
    }

    /// The V multiplicities.
    pub fn v_multiplicities(&self) -> R<&[i32]> {
        self.check_done()?;
        Ok(&self.v_mults)
    }
}
