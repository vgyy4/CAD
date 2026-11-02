//! OCCT's package `AdvApprox`: the approximation of a function by a B-spline curve (`AdvApprox_ApproxAFunction`) with its
//! one-interval worker `AdvApprox_SimpleApprox`, and the interval cutting tools (`AdvApprox_Cutting`, `_DichoCutting`,
//! `_PrefCutting`, `_PrefAndRec`).
//!
//! Translated from `AdvApprox_*.cxx` (OCCT 8.0.1); see `docs/api-advapprox.md` for the contract. Indices into the arrays of
//! the C++ code, which start at 1 (or 0) there, are 0-based slices here; the comments name the C++ index where it matters.
use crate::bsplclib::{BSplError, curve3d};
use crate::convert::{CompPolynomialToPoles, ConvertError};
use crate::gp::{Pnt, Pnt2d};
use crate::plib::{self, Array2, GeomAbsShape, JacobiPolynomial, PLibError};
use crate::poly::fmt::ostream_g;

/// The errors of the package: the exceptions of OCCT and the checks that OCCT 8.0.1 as built does not make.
#[derive(Debug, Clone, PartialEq)]
pub enum AdvApproxError {
    /// `Standard_ConstructionError`.
    Construction,
    /// `Standard_OutOfRange`, and every access outside an array that the C++ code leaves unchecked.
    OutOfRange,
    /// `Standard_DomainError`.
    Domain,
    /// A null handle that the C++ code dereferences.
    NullObject,
    /// An error of `plib`.
    PLib(PLibError),
    /// An error of `convert`.
    Convert(ConvertError),
    /// An error of `bsplclib`.
    BSpl(BSplError),
}

impl From<PLibError> for AdvApproxError {
    fn from(e: PLibError) -> Self {
        AdvApproxError::PLib(e)
    }
}
impl From<ConvertError> for AdvApproxError {
    fn from(e: ConvertError) -> Self {
        AdvApproxError::Convert(e)
    }
}
impl From<BSplError> for AdvApproxError {
    fn from(e: BSplError) -> Self {
        AdvApproxError::BSpl(e)
    }
}

/// The result type of the package.
pub type Result<T> = std::result::Result<T, AdvApproxError>;

/// `AdvApprox_EvaluatorFunction`: the function to approximate. `result` has `dimension` items; the return value is
/// `*ErrorCode` (0 when everything is fine).
pub trait EvaluatorFunction {
    /// `Evaluate` (and `operator()`): the derivative `derivative_request` of the function at `parameter`.
    fn evaluate(
        &mut self,
        dimension: i32,
        start_end: [f64; 2],
        parameter: f64,
        derivative_request: i32,
        result: &mut [f64],
    ) -> i32;
}

/// `AdvApprox_Cutting`: the way an interval is cut in two.
pub trait Cutting {
    /// `Value(a, b, cuttingvalue)`: (the answer, `cuttingvalue`).
    fn value(&self, a: f64, b: f64) -> (bool, f64);
}

/// `10 * Precision::PConfusion()`, the minimum length of an interval.
fn lgmin() -> f64 {
    10.0 * crate::geom2d::pconfusion()
}

/// `AdvApprox_DichoCutting`: cut in the middle.
#[derive(Debug, Clone, Copy, Default)]
pub struct DichoCutting;

impl Cutting for DichoCutting {
    fn value(&self, a: f64, b: f64) -> (bool, f64) {
        let cutting = (a + b) / 2.0;
        (cutting_len_ok(b - a), cutting)
    }
}
fn cutting_len_ok(d: f64) -> bool {
    d.abs() >= 2.0 * lgmin()
}

/// `AdvApprox_PrefCutting`: cut at the preferred point that is nearest to the middle.
#[derive(Debug, Clone)]
pub struct PrefCutting {
    points: Vec<f64>,
}

impl PrefCutting {
    /// `AdvApprox_PrefCutting(CutPnts)`.
    pub fn new(cut_points: &[f64]) -> Self {
        Self { points: cut_points.to_vec() }
    }
}

impl Cutting for PrefCutting {
    fn value(&self, a: f64, b: f64) -> (bool, f64) {
        let lg = lgmin();
        let mil = (a + b) / 2.0;
        let mut dist = ((a - b) / 2.0).abs();
        let mut cut = mil;
        for p in &self.points {
            if (dist - lg) > (mil - p).abs() {
                cut = *p;
                dist = (mil - p).abs();
            }
        }
        ((cut - a).abs() >= lg && (b - cut).abs() >= lg, cut)
    }
}

/// `AdvApprox_PrefAndRec`: preferred cutting points, then recommended ones.
#[derive(Debug, Clone)]
pub struct PrefAndRec {
    rec: Vec<f64>,
    pref: Vec<f64>,
    weight: f64,
}

impl PrefAndRec {
    /// `AdvApprox_PrefAndRec(RecCut, PrefCut, Weight)`: `Err(Domain)` for `weight <= 1`.
    pub fn new(rec_cut: &[f64], pref_cut: &[f64], weight: f64) -> Result<Self> {
        if weight <= 1.0 {
            return Err(AdvApproxError::Domain);
        }
        Ok(Self { rec: rec_cut.to_vec(), pref: pref_cut.to_vec(), weight })
    }
}

impl Cutting for PrefAndRec {
    fn value(&self, a: f64, b: f64) -> (bool, f64) {
        let lg = lgmin();
        let mil = (a + b) / 2.0;
        let mut cut = mil;
        let mut found = false;
        // search for a preferred cutting point
        let mut dist = ((a * self.weight + b) / (1.0 + self.weight) - mil).abs();
        for p in &self.pref {
            if dist > (mil - p).abs() {
                cut = *p;
                dist = (mil - cut).abs();
                found = true;
            }
        }
        // search for a recommended cutting point
        if !found {
            dist = ((a - b) / 2.0).abs();
            for p in &self.rec {
                if (dist - lg) > (mil - p).abs() {
                    cut = *p;
                    dist = (mil - p).abs();
                }
            }
        }
        ((cut - a).abs() >= lg && (b - cut).abs() >= lg, cut)
    }
}

// ---- helpers -----------------------------------------------------------------------------------------------------------

const MAX_ITEMS: i64 = 1 << 28;

/// The number of items of an array: `Err(OutOfRange)` for a negative or absurd size.
fn count(n: i64) -> Result<usize> {
    if (0..=MAX_ITEMS).contains(&n) {
        usize::try_from(n).map_err(|_| AdvApproxError::OutOfRange)
    } else {
        Err(AdvApproxError::OutOfRange)
    }
}
fn zeros(n: i64) -> Result<Vec<f64>> {
    Ok(vec![0.0; count(n)?])
}
fn at<T: Copy>(v: &[T], i: i64) -> Result<T> {
    usize::try_from(i).ok().and_then(|i| v.get(i)).copied().ok_or(AdvApproxError::OutOfRange)
}
fn put<T>(v: &mut [T], i: i64, x: T) -> Result<()> {
    let slot = usize::try_from(i).ok().and_then(|i| v.get_mut(i)).ok_or(AdvApproxError::OutOfRange)?;
    *slot = x;
    Ok(())
}
fn tail<T>(v: &[T], from: i64) -> Result<&[T]> {
    usize::try_from(from).ok().and_then(|i| v.get(i..)).ok_or(AdvApproxError::OutOfRange)
}
/// `std::max(a, b)`.
fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}
/// `NivConstr` of a continuity: `Err(Construction)` for anything but `C0/C1/C2`.
fn niv_of(c: GeomAbsShape) -> Result<i32> {
    match c {
        GeomAbsShape::C0 => Ok(0),
        GeomAbsShape::C1 => Ok(1),
        GeomAbsShape::C2 => Ok(2),
        _ => Err(AdvApproxError::Construction),
    }
}

// ---- AdvApprox_SimpleApprox --------------------------------------------------------------------------------------------

/// `AdvApprox_SimpleApprox`: a polynomial approximation of the function on one interval, as low a degree as the tolerances
/// and the maximum degree allow. The evaluator is borrowed for the life of the object (C++ keeps a pointer to it).
pub struct SimpleApprox<'a> {
    total_num_ss: i32,
    total_dimension: i32,
    nb_gauss_points: i32,
    work_degree: i32,
    niv_constr: i32,
    jac_pol: JacobiPolynomial,
    tab_points: Vec<f64>,
    tab_weights: Array2<f64>,
    evaluator: &'a mut dyn EvaluatorFunction,
    degree: i32,
    coeff: Vec<f64>,
    first_constr: Array2<f64>,
    last_constr: Array2<f64>,
    som_tab: Vec<f64>,
    dif_tab: Vec<f64>,
    max_error: Vec<f64>,
    average_error: Vec<f64>,
    done: bool,
}

impl std::fmt::Debug for SimpleApprox<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleApprox")
            .field("total_num_ss", &self.total_num_ss)
            .field("total_dimension", &self.total_dimension)
            .field("nb_gauss_points", &self.nb_gauss_points)
            .field("work_degree", &self.work_degree)
            .field("degree", &self.degree)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl<'a> SimpleApprox<'a> {
    /// `AdvApprox_SimpleApprox(TotalDimension, TotalNumSS, Continuity, WorkDegree, NbGaussPoints, JacobiBase, Func)`:
    /// `Err(Construction)` for a continuity other than `C0/C1/C2` or Gauss points that the Jacobi basis does not accept.
    pub fn new(
        total_dimension: i32,
        total_num_ss: i32,
        continuity: GeomAbsShape,
        work_degree: i32,
        nb_gauss_points: i32,
        jacobi: &JacobiPolynomial,
        func: &'a mut dyn EvaluatorFunction,
    ) -> Result<Self> {
        let niv_constr = niv_of(continuity)?;
        // the extraction of the Legendre roots and of the Gauss weights
        let tab_points = jacobi.points(nb_gauss_points)?;
        let tab_weights = jacobi.weights(nb_gauss_points)?;
        let dim = i64::from(total_dimension);
        let coeff = zeros((i64::from(work_degree) + 1) * dim)?;
        let rows = count(dim)?;
        let cols = count(i64::from(niv_constr) + 1)?;
        let first_constr = Array2::new(rows, cols, zeros((rows * cols) as i64)?)?;
        let last_constr = first_constr.clone();
        let half = i64::from(nb_gauss_points / 2 + 1) * dim;
        Ok(Self {
            total_num_ss,
            total_dimension,
            nb_gauss_points,
            work_degree,
            niv_constr,
            jac_pol: jacobi.clone(),
            tab_points,
            tab_weights,
            evaluator: func,
            degree: 0,
            coeff,
            first_constr,
            last_constr,
            som_tab: zeros(half)?,
            dif_tab: zeros(half)?,
            max_error: Vec::new(),
            average_error: Vec::new(),
            done: false,
        })
    }

    fn eval(&mut self, first_last: [f64; 2], param: f64, derive: i32, out: &mut [f64]) -> i32 {
        self.evaluator.evaluate(self.total_dimension, first_last, param, derive, out)
    }

    /// `Perform(LocalDimension, LocalTolerancesArray, First, Last, MaxDegree)`; an evaluation error leaves `is_done()` false.
    pub fn perform(
        &mut self,
        local_dimension: &[i32],
        local_tolerances: &[f64],
        first: f64,
        last: f64,
        max_degree: i32,
    ) -> Result<()> {
        // ======= the computation of Pp(t) = Rr(t) + W(t)*Qq(t) =======
        self.done = false;
        let dim = i64::from(self.total_dimension);
        let nd = count(dim)?;
        let niv = self.niv_constr;

        // ===== the computation of Rr(t) (the first part of Pp) ======
        let degree_r = 2 * niv + 1;
        let degree_q = self.work_degree - 2 * (niv + 1);
        let first_last = [first, last];
        let fact = (last - first) / 2.0;
        let mut result = vec![0.0; nd];
        let ncols = count(i64::from(niv) + 1)?;
        let mut constr = [vec![0.0; nd * ncols], vec![0.0; nd * ncols]];

        for (side, param) in [first, last].into_iter().enumerate() {
            let mut derive = niv;
            while derive >= 0 {
                if self.eval(first_last, param, derive, &mut result) != 0 {
                    return Ok(()); // evaluation error
                }
                if derive >= 1 {
                    for r in &mut result {
                        *r *= fact;
                    }
                }
                if derive == 2 {
                    for r in &mut result {
                        *r *= fact;
                    }
                }
                let m = constr.get_mut(side).ok_or(AdvApproxError::OutOfRange)?;
                for (idim, r) in result.iter().enumerate() {
                    put(m, (idim * ncols) as i64 + i64::from(derive), *r)?;
                }
                derive -= 1;
            }
        }
        let [fc, lc] = constr;
        self.first_constr = Array2::new(nd, ncols, fc)?;
        self.last_constr = Array2::new(nd, ncols, lc)?;

        let hermite = plib::hermite_interpolate(
            self.total_dimension,
            -1.0,
            1.0,
            niv,
            niv,
            &self.first_constr,
            &self.last_constr,
        )?;
        let dst = self.coeff.get_mut(..hermite.len()).ok_or(AdvApproxError::OutOfRange)?;
        dst.copy_from_slice(&hermite);

        // ===== the computation of the coefficients of Qq(t) (the second part of Pp) ======
        let derive = 0;
        let alin = (last - first) / 2.0;
        let blin = (last + first) / 2.0;
        let half = i64::from(self.nb_gauss_points / 2);
        let mut fti = vec![0.0; nd];

        let mut i_idim = dim;
        for i in 1..=half {
            let ti = at(&self.tab_points, i)?;
            let tip = alin * ti + blin;
            if self.eval(first_last, tip, derive, &mut fti) != 0 {
                return Ok(());
            }
            for f in &fti {
                put(&mut self.som_tab, i_idim, *f)?;
                put(&mut self.dif_tab, i_idim, *f)?;
                i_idim += 1;
            }
        }
        i_idim = dim;
        for i in 1..=half {
            let mut ti = at(&self.tab_points, i)?;
            let tin = -alin * ti + blin;
            if self.eval(first_last, tin, derive, &mut fti) != 0 {
                return Ok(());
            }
            let rpti = plib::eval_polynomial(ti, derive, degree_r, self.total_dimension, &self.coeff)?;
            ti = -ti;
            let rmti = plib::eval_polynomial(ti, derive, degree_r, self.total_dimension, &self.coeff)?;
            for (idim, f) in fti.iter().enumerate() {
                let rp = at(&rpti, idim as i64)?;
                let rm = at(&rmti, idim as i64)?;
                let s = at(&self.som_tab, i_idim)? + *f - rp - rm;
                put(&mut self.som_tab, i_idim, s)?;
                let d = at(&self.dif_tab, i_idim)? - *f - rp + rm;
                put(&mut self.dif_tab, i_idim, d)?;
                i_idim += 1;
            }
        }

        // for odd NbGaussPoints - the computation of [ F(0) - R(0) ]
        if self.nb_gauss_points % 2 == 1 {
            let ti = at(&self.tab_points, 0)?;
            let tip = blin;
            if self.eval(first_last, tip, derive, &mut fti) != 0 {
                return Ok(());
            }
            let rpti = plib::eval_polynomial(ti, derive, degree_r, self.total_dimension, &self.coeff)?;
            for (idim, f) in fti.iter().enumerate() {
                let v = *f - at(&rpti, idim as i64)?;
                put(&mut self.som_tab, idim as i64, v)?;
                put(&mut self.dif_tab, idim as i64, v)?;
            }
        }

        // the computation of Qq(t)
        let mut sum = 0.0;
        let w = |t: &Array2<f64>, i: i64, k: i64| -> Result<f64> {
            let (i, k) = (
                usize::try_from(i).map_err(|_| AdvApproxError::OutOfRange)?,
                usize::try_from(k).map_err(|_| AdvApproxError::OutOfRange)?,
            );
            t.get(i, k).copied().ok_or(AdvApproxError::OutOfRange)
        };
        let dq = i64::from(degree_q);
        let dr = i64::from(degree_r);
        let mut k = 0;
        while k <= dq {
            for idim in 1..=dim {
                sum = 0.0;
                for i in 1..=half {
                    sum += w(&self.tab_weights, i, k)? * at(&self.som_tab, i * dim + idim - 1)?;
                }
                put(&mut self.coeff, (k + dr + 1) * dim + idim - 1, sum)?;
            }
            k += 2;
        }
        k = 1;
        while k <= dq {
            for idim in 1..=dim {
                sum = 0.0;
                for i in 1..=half {
                    sum += w(&self.tab_weights, i, k)? * at(&self.dif_tab, i * dim + idim - 1)?;
                }
                put(&mut self.coeff, (k + dr + 1) * dim + idim - 1, sum)?;
            }
            k += 2;
        }
        if self.nb_gauss_points % 2 == 1 {
            for idim in 1..=dim {
                let mut k = 0;
                while k <= dq {
                    // (`sum` is not reset here: the C++ code keeps accumulating)
                    sum += w(&self.tab_weights, 0, k)? * at(&self.som_tab, idim - 1)?;
                    put(&mut self.coeff, (k + dr + 1) * dim + idim - 1, sum)?;
                    k += 2;
                }
            }
        }

        // the computing of NewDegree
        let wd1 = i64::from(self.work_degree) + 1;
        let mut jac_coeff = zeros(dim * wd1)?;
        let nss = i64::from(self.total_num_ss);
        self.max_error = zeros(nss)?;
        self.average_error = zeros(nss)?;
        let mut range_ss = 0;
        let mut range_jac = 0;
        let mut new_degree_max = 0;
        for numss in 1..=nss {
            let d = i64::from(at(local_dimension, numss - 1)?);
            let mut range_coeff = 0;
            let mut range_dim = 0;
            for _ in 0..=self.work_degree {
                for idim in 1..=d {
                    let v = at(&self.coeff, range_coeff + range_ss + idim - 1)?;
                    put(&mut jac_coeff, range_jac + range_dim + idim - 1, v)?;
                }
                range_dim += d;
                range_coeff += dim;
            }
            let (new_degree, _max_err) = self.jac_pol.reduce_degree(
                d as i32,
                max_degree,
                at(local_tolerances, numss - 1)?,
                tail(&jac_coeff, range_jac)?,
            )?;
            if new_degree > new_degree_max {
                new_degree_max = new_degree;
            }
            range_ss += d;
            range_jac += wd1 * d;
        }
        // the computing of MaxError and AverageError
        range_jac = 0;
        for numss in 1..=nss {
            let d = at(local_dimension, numss - 1)?;
            let js = tail(&jac_coeff, range_jac)?;
            let max_err = self.jac_pol.max_error(d, js, new_degree_max)?;
            put(&mut self.max_error, numss - 1, max_err)?;
            let average_err = self.jac_pol.average_error(d, js, new_degree_max)?;
            put(&mut self.average_error, numss - 1, average_err)?;
            range_jac += wd1 * i64::from(d);
        }
        self.degree = new_degree_max;
        self.done = true;
        Ok(())
    }

    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `Degree()`.
    pub fn degree(&self) -> i32 {
        self.degree
    }
    /// `Coefficients()`: the coefficients in the Jacobi base (`(WorkDegree + 1) * TotalDimension` items).
    pub fn coefficients(&self) -> &[f64] {
        &self.coeff
    }
    /// `FirstConstr()`: row `i - 1`, column `k` is the C++ `(i, k)`.
    pub fn first_constr(&self) -> &Array2<f64> {
        &self.first_constr
    }
    /// `LastConstr()`.
    pub fn last_constr(&self) -> &Array2<f64> {
        &self.last_constr
    }
    /// `SomTab()`.
    pub fn som_tab(&self) -> &[f64] {
        &self.som_tab
    }
    /// `DifTab()`.
    pub fn dif_tab(&self) -> &[f64] {
        &self.dif_tab
    }
    /// `MaxError(Index)` (from 1).
    pub fn max_error(&self, index: i32) -> Result<f64> {
        at(&self.max_error, i64::from(index) - 1)
    }
    /// `AverageError(Index)` (from 1).
    pub fn average_error(&self, index: i32) -> Result<f64> {
        at(&self.average_error, i64::from(index) - 1)
    }
    /// `Dump(o)`.
    pub fn dump(&self) -> String {
        let mut s = String::from("Dump of SimpleApprox \n");
        for ii in 1..=self.total_num_ss {
            let e = self.max_error(ii).unwrap_or(0.0);
            s.push_str("Error   ");
            s.push_str(&ostream_g(e, 6));
            s.push('\n');
        }
        s
    }
}

// ---- AdvApprox_ApproxAFunction -----------------------------------------------------------------------------------------

/// The arrays that `AdvApprox_ApproxAFunction::Approximation` fills (the C++ caller passes them in; here they are zero-filled
/// and sized for `max_segments.max(1)` segments).
#[derive(Debug, Clone, PartialEq)]
pub struct ApproximationResult {
    /// `NumCurves`.
    pub num_curves: i32,
    /// `NumCoeffPerCurveArray`.
    pub num_coeff_per_curve: Vec<i32>,
    /// `LocalCoefficientArray` (`segments * total_dimension * num_max_coeffs` items).
    pub coefficients: Vec<f64>,
    /// `IntervalsArray` (`segments + 1` items).
    pub intervals: Vec<f64>,
    /// `ErrorMaxArray` (`segments * total_num_ss` items).
    pub error_max: Vec<f64>,
    /// `AverageErrorArray`.
    pub average_error: Vec<f64>,
    /// `ErrorCode`: 0, or 1 for an input inconsistency or an evaluation error.
    pub error_code: i32,
}

/// `PrepareConvert`: the local continuities of the pieces (`continuity[k]` is the C++ `Continuity(k + 1)`), added to the errors.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn prepare_convert(
    num_curves: i32,
    max_degree: i32,
    continuity_order: i32,
    num_1dss: i32,
    num_2dss: i32,
    num_3dss: i32,
    num_coeff_per_curve: &[i32],
    coefficients: &[f64],
    polynomial_intervals: &[[f64; 2]],
    true_intervals: &[f64],
    local_tolerance: &[f64],
    error_max: &mut [f64],
    continuity: &mut [i32],
) -> Result<()> {
    let dimension = i64::from(num_1dss + 2 * num_2dss + 3 * num_3dss);
    let nb_space = i64::from(num_1dss + num_2dss + num_3dss);
    let real_degree = i64::from((max_degree + 1).max(2 * continuity_order + 2));
    let co = i64::from(continuity_order);
    let mut prec = zeros(nb_space)?;
    let mut suivant = zeros(nb_space)?;

    continuity.fill(0);
    if continuity_order == 0 {
        return Ok(());
    }
    let pi = |row: i64, col: usize| -> Result<f64> {
        let r = polynomial_intervals.get(usize::try_from(row - 1).map_err(|_| AdvApproxError::OutOfRange)?);
        r.and_then(|r| r.get(col)).copied().ok_or(AdvApproxError::OutOfRange)
    };
    for icurve in 1..i64::from(num_curves) {
        // init and positioning at the node
        let mut is_ci = true;
        let coef1 = (icurve - 1) * dimension * real_degree;
        let coef2 = coef1 + dimension * real_degree;
        let deg1 = at(num_coeff_per_curve, icurve - 1)? - 1;
        let res1 = plib::eval_polynomial(
            pi(icurve, 1)?,
            continuity_order,
            deg1,
            dimension as i32,
            tail(coefficients, coef1)?,
        )?;
        let deg2 = at(num_coeff_per_curve, icurve)? - 1;
        let res2 = plib::eval_polynomial(
            pi(icurve + 1, 0)?,
            continuity_order,
            deg2,
            dimension as i32,
            tail(coefficients, coef2)?,
        )?;

        // check in each subspace
        let mut iordre = 1;
        while iordre <= co && is_ci {
            let toler = 1.0e-5;
            let f1_dividend = pi(icurve, 1)? - pi(icurve, 0)?;
            let f2_dividend = pi(icurve + 1, 1)? - pi(icurve + 1, 0)?;
            let f1_divizor = at(true_intervals, icurve)? - at(true_intervals, icurve - 1)?;
            let f2_divizor = at(true_intervals, icurve + 1)? - at(true_intervals, icurve)?;
            let order = iordre as f64;
            let facteur1 = if f1_divizor.abs() < toler { 0.0 } else { libm::pow(f1_dividend / f1_divizor, order) };
            let facteur2 = if f2_divizor.abs() < toler { 0.0 } else { libm::pow(f2_dividend / f2_divizor, order) };
            let normal1 = libm::pow(f1_divizor, order);
            let normal2 = libm::pow(f2_divizor, order);

            let mut idim = 1;
            // Val1[k] is res1[v1 + k]
            let mut v1 = iordre * dimension - 1;
            for ii in 1..=i64::from(num_1dss) {
                if !is_ci {
                    break;
                }
                let eps = at(local_tolerance, idim - 1)? * 0.01;
                let a = at(&res1, v1 + ii)?;
                let b = at(&res2, v1 + ii)?;
                let diff = (a * facteur1 - b * facteur2).abs();
                let moy = (a * facteur1 + b * facteur2).abs();
                // a first check on the relative value
                if diff > moy * 1.0e-9 {
                    put(&mut prec, idim - 1, diff * normal1)?;
                    put(&mut suivant, idim - 1, diff * normal2)?;
                    // and a second check on the upper bound of the generated error
                    if at(&prec, idim - 1)? > eps || at(&suivant, idim - 1)? > eps {
                        is_ci = false;
                    }
                } else {
                    put(&mut prec, idim - 1, 0.0)?;
                    put(&mut suivant, idim - 1, 0.0)?;
                }
                idim += 1;
            }

            v1 += i64::from(num_1dss);
            let mut ii = 1;
            while ii <= i64::from(num_2dss) && is_ci {
                let eps = at(local_tolerance, idim - 1)? * 0.01;
                let (x1, y1) = (at(&res1, v1 + 1)? * facteur1, at(&res1, v1 + 2)? * facteur1);
                let (x2, y2) = (at(&res2, v1 + 1)? * facteur2, at(&res2, v1 + 2)? * facteur2);
                let diff = (x1 - x2).abs() + (y1 - y2).abs();
                let moy = (x1 + x2).abs() + (y1 + y2).abs();
                if diff > moy * 1.0e-9 {
                    put(&mut prec, idim - 1, diff * normal1)?;
                    put(&mut suivant, idim - 1, diff * normal2)?;
                    if at(&prec, idim - 1)? > eps || at(&suivant, idim - 1)? > eps {
                        is_ci = false;
                    }
                } else {
                    put(&mut prec, idim - 1, 0.0)?;
                    put(&mut suivant, idim - 1, 0.0)?;
                }
                ii += 1;
                idim += 1;
                v1 += 2;
            }

            let mut ii = 1;
            while ii <= i64::from(num_3dss) && is_ci {
                let eps = at(local_tolerance, idim - 1)? * 0.01;
                let c1 = [at(&res1, v1 + 1)? * facteur1, at(&res1, v1 + 2)? * facteur1, at(&res1, v1 + 3)? * facteur1];
                let c2 = [at(&res2, v1 + 1)? * facteur2, at(&res2, v1 + 2)? * facteur2, at(&res2, v1 + 3)? * facteur2];
                let diff = (c1[0] - c2[0]).abs() + (c1[1] - c2[1]).abs() + (c1[2] - c2[2]).abs();
                let moy = (c1[0] + c2[0]).abs() + (c1[1] + c2[1]).abs() + (c1[2] + c2[2]).abs();
                if diff > moy * 1.0e-9 {
                    put(&mut prec, idim - 1, diff * normal1)?;
                    put(&mut suivant, idim - 1, diff * normal2)?;
                    if at(&prec, idim - 1)? > eps || at(&suivant, idim - 1)? > eps {
                        is_ci = false;
                    }
                } else {
                    put(&mut prec, idim - 1, 0.0)?;
                    put(&mut suivant, idim - 1, 0.0)?;
                }
                ii += 1;
                idim += 1;
                v1 += 3;
            }
            // if it's good, update everything
            if is_ci {
                put(continuity, icurve, iordre as i32)?;
                let index = (icurve - 1) * nb_space + 1;
                for idim in 1..=nb_space {
                    let ii = index + idim - 1;
                    let e = at(error_max, ii - 1)? + at(&prec, idim - 1)?;
                    put(error_max, ii - 1, e)?;
                    let e = at(error_max, ii - 1 + nb_space)? + at(&suivant, idim - 1)?;
                    put(error_max, ii - 1 + nb_space, e)?;
                }
            }
            iordre += 1;
        }
    }
    Ok(())
}

/// `AdvApprox_ApproxAFunction`: the approximation of a function by a B-spline curve with 1D, 2D and 3D parts.
#[derive(Debug, Clone)]
pub struct ApproxAFunction {
    num_sub_spaces: [i32; 3],
    done: bool,
    has_result: bool,
    one_d_poles: Option<Array2<f64>>,
    two_d_poles: Option<Array2<Pnt2d>>,
    three_d_poles: Option<Array2<Pnt>>,
    knots: Option<Vec<f64>>,
    mults: Option<Vec<i32>>,
    degree: i32,
    one_d_max_error: Option<Vec<f64>>,
    one_d_average_error: Option<Vec<f64>>,
    two_d_max_error: Option<Vec<f64>>,
    two_d_average_error: Option<Vec<f64>>,
    three_d_max_error: Option<Vec<f64>>,
    three_d_average_error: Option<Vec<f64>>,
}

impl ApproxAFunction {
    /// `AdvApprox_ApproxAFunction(Num1DSS, Num2DSS, Num3DSS, OneDTol, TwoDTol, ThreeDTol, First, Last, Continuity, MaxDeg, MaxSeg, Func)`:
    /// the intervals are cut in the middle.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        num_1dss: i32,
        num_2dss: i32,
        num_3dss: i32,
        one_d_tol: &[f64],
        two_d_tol: &[f64],
        three_d_tol: &[f64],
        first: f64,
        last: f64,
        continuity: GeomAbsShape,
        max_deg: i32,
        max_seg: i32,
        func: &mut dyn EvaluatorFunction,
    ) -> Result<Self> {
        Self::with_cutting(
            num_1dss,
            num_2dss,
            num_3dss,
            one_d_tol,
            two_d_tol,
            three_d_tol,
            first,
            last,
            continuity,
            max_deg,
            max_seg,
            func,
            &DichoCutting,
        )
    }

    /// The constructor with a user method of cutting.
    #[allow(clippy::too_many_arguments)]
    pub fn with_cutting(
        num_1dss: i32,
        num_2dss: i32,
        num_3dss: i32,
        one_d_tol: &[f64],
        two_d_tol: &[f64],
        three_d_tol: &[f64],
        first: f64,
        last: f64,
        continuity: GeomAbsShape,
        max_deg: i32,
        max_seg: i32,
        func: &mut dyn EvaluatorFunction,
        cut_tool: &dyn Cutting,
    ) -> Result<Self> {
        let mut s = Self {
            num_sub_spaces: [0; 3],
            done: false,
            has_result: false,
            one_d_poles: None,
            two_d_poles: None,
            three_d_poles: None,
            knots: None,
            mults: None,
            degree: 0,
            one_d_max_error: None,
            one_d_average_error: None,
            two_d_max_error: None,
            two_d_average_error: None,
            three_d_max_error: None,
            three_d_average_error: None,
        };
        s.perform(
            num_1dss,
            num_2dss,
            num_3dss,
            [one_d_tol, two_d_tol, three_d_tol],
            first,
            last,
            continuity,
            max_deg,
            max_seg,
            func,
            cut_tool,
        )?;
        Ok(s)
    }

    /// `Approximation(...)`: the approximation by polynomial pieces; see `ApproximationResult` for the output arrays.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn approximation(
        total_dimension: i32,
        total_num_ss: i32,
        local_dimension: &[i32],
        first: f64,
        last: f64,
        evaluator: &mut dyn EvaluatorFunction,
        cut_tool: &dyn Cutting,
        continuity_order: i32,
        num_max_coeffs: i32,
        max_segments: i32,
        local_tolerances: &[f64],
        code_precis: i32,
    ) -> Result<ApproximationResult> {
        let ms = i64::from(max_segments.max(1));
        let dim = i64::from(total_dimension);
        let nss = i64::from(total_num_ss);
        let mut r = ApproximationResult {
            num_curves: 0,
            num_coeff_per_curve: vec![0; count(ms)?],
            coefficients: zeros(ms * dim * i64::from(num_max_coeffs))?,
            intervals: zeros(ms + 1)?,
            error_max: zeros(ms * nss)?,
            average_error: zeros(ms * nss)?,
            error_code: 0,
        };

        // input validation
        if max_segments < 1 || (last - first).abs() < 1.0e-9 {
            r.error_code = 1;
            return Ok(r);
        }
        // the total dimension must be the sum of the subspace dimensions
        let mut idim = 0;
        for i in 0..nss {
            idim += at(local_dimension, i)?;
        }
        if idim != total_dimension {
            r.error_code = 1;
            return Ok(r);
        }
        let continuity = match continuity_order {
            0 => GeomAbsShape::C0,
            1 => GeomAbsShape::C1,
            2 => GeomAbsShape::C2,
            _ => return Err(AdvApproxError::Construction),
        };

        // choice of number of points and smoothing degree in the orthogonal basis
        let (nb_gauss_points, work_degree) = plib::jacobi_parameters(continuity, num_max_coeffs - 1, code_precis)?;

        // initialization of cutting management
        put(&mut r.intervals, 0, first)?;
        put(&mut r.intervals, 1, last)?;
        let mut nupil = 1;
        r.num_curves = 0;

        // approximation with cutting
        let jacobi = JacobiPolynomial::new(work_degree, continuity)?;
        let max_degree = num_max_coeffs - 1;
        let mut approx = SimpleApprox::new(
            total_dimension,
            total_num_ss,
            continuity,
            work_degree,
            nb_gauss_points,
            &jacobi,
            evaluator,
        )?;
        let mut is_cut = false;
        while nupil - r.num_curves != 0 {
            // when the top of the stack is reached, we are done!
            // compute the approximation curve in the Jacobi basis
            let nc = i64::from(r.num_curves);
            approx.perform(
                local_dimension,
                local_tolerances,
                at(&r.intervals, nc)?,
                at(&r.intervals, nc + 1)?,
                max_degree,
            )?;
            if !approx.is_done() {
                r.error_code = 1;
                return Ok(r);
            }

            // compute the curve degree and max error
            put(&mut r.num_coeff_per_curve, nc, 0)?;

            // the error must be satisfied on all subspaces, otherwise we split
            let mut max_max_err = true;
            for is in 1..=total_num_ss {
                if approx.max_error(is)? > at(local_tolerances, i64::from(is) - 1)? {
                    max_max_err = false;
                    break;
                }
            }

            let mut goto_fin_de_boucle = false;
            if max_max_err {
                r.num_curves += 1;
            } else {
                // otherwise try to split the current interval in 2...
                let (large, tmil) = cut_tool.value(at(&r.intervals, nc)?, at(&r.intervals, nc + 1)?);
                if nupil < max_segments && large {
                    is_cut = true; // now we know it!
                    let ideb = nc + 1;
                    let ilong = i64::from(nupil) - nc - 1;
                    let mut ii = ilong;
                    while ii >= 0 {
                        let v = at(&r.intervals, ideb + ii)?;
                        put(&mut r.intervals, ideb + ii + 1, v)?;
                        ii -= 1;
                    }
                    put(&mut r.intervals, ideb, tmil)?;
                    nupil += 1;
                    // ... and start over
                    goto_fin_de_boucle = true;
                } else {
                    // if the stack is full...
                    r.num_curves += 1;
                }
            }
            if goto_fin_de_boucle {
                continue;
            }
            let ncur = i64::from(r.num_curves);
            for is in 1..=nss {
                let k = (is - 1) + (ncur - 1) * nss;
                put(&mut r.error_max, k, approx.max_error(is as i32)?)?;
                // ...and compute the average error
                put(&mut r.average_error, k, approx.average_error(is as i32)?)?;
            }

            let mut the_deg = approx.degree();
            if is_cut && the_deg < 2 * continuity_order + 1 {
                // to avoid noisy derivatives at the ends, and maintain correct continuity on the resulting BSpline
                the_deg = 2 * continuity_order + 1;
            }
            put(&mut r.num_coeff_per_curve, ncur - 1, the_deg + 1)?;
            let coefficients = jacobi.to_coefficients(total_dimension, the_deg, approx.coefficients())?;
            let f = i64::from(the_deg + 1) * dim;
            let j0 = (ncur - 1) * dim * i64::from(num_max_coeffs);
            for i in 0..f {
                put(&mut r.coefficients, j0 + i, at(&coefficients, i)?)?;
            }
        }
        Ok(r)
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn perform(
        &mut self,
        num_1dss: i32,
        num_2dss: i32,
        num_3dss: i32,
        tols: [&[f64]; 3],
        first: f64,
        last: f64,
        continuity: GeomAbsShape,
        max_degree: i32,
        max_segments: i32,
        func: &mut dyn EvaluatorFunction,
        cut_tool: &dyn Cutting,
    ) -> Result<()> {
        if num_1dss < 0
            || num_2dss < 0
            || num_3dss < 0
            || num_1dss + num_2dss + num_3dss <= 0
            || last < first
            || max_degree < 1
            || max_segments < 0
        {
            return Err(AdvApproxError::Construction);
        }
        let mut max_degree = max_degree;
        if max_degree > 14 {
            max_degree = 14;
        }
        // input
        self.num_sub_spaces = [num_1dss, num_2dss, num_3dss];
        let total_num_ss = num_1dss + num_2dss + num_3dss;
        let total_dimension = num_1dss + 2 * num_2dss + 3 * num_3dss;
        let continuity_order = niv_of(continuity)?;
        let num_max_coeffs = (max_degree + 1).max(2 * continuity_order + 2);
        let max_degree = num_max_coeffs - 1;
        let code_precis = 1;
        // WARNING : the polynomial coefficients are the taylor expansion of the polynomial at 0.0e0 !
        let approx_start_end = [-1.0, 1.0];

        let mut local_dimension = Vec::new();
        let mut local_tolerances = Vec::new();
        for (g, n) in [num_1dss, num_2dss, num_3dss].into_iter().enumerate() {
            for jj in 0..n {
                local_tolerances.push(at(tols.get(g).copied().unwrap_or(&[]), i64::from(jj))?);
                local_dimension.push(g as i32 + 1);
            }
        }

        // output
        let mut r = Self::approximation(
            total_dimension,
            total_num_ss,
            &local_dimension,
            first,
            last,
            func,
            cut_tool,
            continuity_order,
            num_max_coeffs,
            max_segments,
            &local_tolerances,
            code_precis,
        )?;
        let error_code = r.error_code;
        if error_code == 0 || error_code == -1 {
            // everything is OK, or we have a result where one of the max errors is larger than the requested tolerance
            let num_curves = r.num_curves;
            let nc = count(i64::from(num_curves))?;
            let mut tab_continuity = vec![0; nc];
            let mut polynomial_intervals = vec![[0.0; 2]; nc];
            for (ii, pi) in polynomial_intervals.iter_mut().enumerate() {
                // force a minimum degree of 1 (PRO5474)
                let v = r.num_coeff_per_curve.get_mut(ii).ok_or(AdvApproxError::OutOfRange)?;
                *v = (*v).max(2);
                *pi = approx_start_end;
            }

            prepare_convert(
                num_curves,
                max_degree,
                continuity_order,
                num_1dss,
                num_2dss,
                num_3dss,
                &r.num_coeff_per_curve,
                &r.coefficients,
                &polynomial_intervals,
                &r.intervals,
                &local_tolerances,
                &mut r.error_max,
                &mut tab_continuity,
            )?;

            let converter = CompPolynomialToPoles::with_continuities(
                num_curves,
                total_dimension,
                max_degree,
                &tab_continuity,
                &r.num_coeff_per_curve,
                &r.coefficients,
                &polynomial_intervals,
                &r.intervals,
            )?;

            if converter.is_done() {
                let poles = converter.poles();
                self.knots = Some(converter.knots().to_vec());
                self.mults = Some(converter.multiplicities().to_vec());
                self.degree = converter.degree();
                let rows = poles.rows();
                let tp = |ii: usize, col: i64| -> Result<f64> {
                    usize::try_from(col).ok().and_then(|c| poles.get(ii, c)).copied().ok_or(AdvApproxError::OutOfRange)
                };
                let nss = i64::from(total_num_ss);
                let ncurves = i64::from(num_curves);
                // the max / average errors of the subspaces `jj = 1..=n` of a group, whose errors start at `off`
                let errors = |n: i32, off: i64| -> Result<(Vec<f64>, Vec<f64>)> {
                    let mut mx = Vec::new();
                    let mut av = Vec::new();
                    for jj in 1..=i64::from(n) {
                        let mut e = 0.0;
                        for ii in 1..=ncurves {
                            let local_index = (ii - 1) * nss + off;
                            e = cmax(at(&r.error_max, local_index + jj - 1)?, e);
                        }
                        mx.push(e);
                    }
                    for jj in 1..=i64::from(n) {
                        let mut e = 0.0;
                        for ii in 1..=ncurves {
                            let local_index = (ii - 1) * nss + off;
                            e += at(&r.average_error, local_index + jj - 1)?;
                        }
                        e /= f64::from(num_curves);
                        av.push(e);
                    }
                    Ok((mx, av))
                };
                let mut index = 0i64;
                if num_1dss > 0 {
                    let mut data = Vec::new();
                    for ii in 0..rows {
                        for jj in 0..i64::from(num_1dss) {
                            data.push(tp(ii, jj)?);
                        }
                    }
                    self.one_d_poles = Some(Array2::new(rows, count(i64::from(num_1dss))?, data)?);
                    let (mx, av) = errors(num_1dss, 0)?;
                    self.one_d_max_error = Some(mx);
                    self.one_d_average_error = Some(av);
                    index += i64::from(num_1dss);
                }
                let mut dim_index = index; // for the case where there is no 2D
                if num_2dss > 0 {
                    let mut data = Vec::new();
                    for ii in 0..rows {
                        for jj in 1..=i64::from(num_2dss) {
                            let local_index = index + (jj - 1) * 2;
                            data.push(Pnt2d::new(tp(ii, local_index)?, tp(ii, local_index + 1)?));
                        }
                    }
                    self.two_d_poles = Some(Array2::new(rows, count(i64::from(num_2dss))?, data)?);
                    let (mx, av) = errors(num_2dss, index)?;
                    self.two_d_max_error = Some(mx);
                    self.two_d_average_error = Some(av);
                    index += i64::from(num_2dss);
                    // for poles, the offset must be doubled
                    dim_index = index + i64::from(num_2dss);
                }
                if num_3dss > 0 {
                    let mut data = Vec::new();
                    for ii in 0..rows {
                        for jj in 1..=i64::from(num_3dss) {
                            let local_index = dim_index + (jj - 1) * 3;
                            data.push(Pnt::new(
                                tp(ii, local_index)?,
                                tp(ii, local_index + 1)?,
                                tp(ii, local_index + 2)?,
                            ));
                        }
                    }
                    self.three_d_poles = Some(Array2::new(rows, count(i64::from(num_3dss))?, data)?);
                    let (mx, av) = errors(num_3dss, index)?;
                    self.three_d_max_error = Some(mx);
                    self.three_d_average_error = Some(av);
                }
                if error_code == 0 {
                    self.done = true;
                    self.has_result = true;
                } else if error_code == -1 {
                    self.has_result = true;
                }
            }
        }
        Ok(())
    }

    /// `IsDone()`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    /// `HasResult()`.
    pub fn has_result(&self) -> bool {
        self.has_result
    }
    /// `Poles1d()`: row `i - 1`, column `j - 1` is the C++ `(i, j)`; `None` without 1D subspaces or result.
    pub fn poles_1d(&self) -> Option<&Array2<f64>> {
        self.one_d_poles.as_ref()
    }
    /// `Poles2d()`.
    pub fn poles_2d(&self) -> Option<&Array2<Pnt2d>> {
        self.two_d_poles.as_ref()
    }
    /// `Poles()`.
    pub fn poles(&self) -> Option<&Array2<Pnt>> {
        self.three_d_poles.as_ref()
    }
    /// `NbPoles()`.
    pub fn nb_poles(&self) -> Result<i32> {
        if self.done || self.has_result {
            let mults = self.mults.as_deref().ok_or(AdvApproxError::NullObject)?;
            Ok(curve3d::nb_poles_of(self.degree, false, mults)?)
        } else {
            Ok(0)
        }
    }
    /// `Poles1d(Index, P)` for `P` with the bounds `lower..=upper`: the poles of the 1D subspace `index`.
    pub fn poles_1d_at(&self, index: i32, lower: i32, upper: i32) -> Result<Vec<f64>> {
        let p = self.one_d_poles.as_ref().ok_or(AdvApproxError::NullObject)?;
        column(p, index, lower, upper).map(|v| v.into_iter().copied().collect())
    }
    /// `Poles2d(Index, P)`.
    pub fn poles_2d_at(&self, index: i32, lower: i32, upper: i32) -> Result<Vec<Pnt2d>> {
        let p = self.two_d_poles.as_ref().ok_or(AdvApproxError::NullObject)?;
        column(p, index, lower, upper).map(|v| v.into_iter().copied().collect())
    }
    /// `Poles(Index, P)`.
    pub fn poles_at(&self, index: i32, lower: i32, upper: i32) -> Result<Vec<Pnt>> {
        let p = self.three_d_poles.as_ref().ok_or(AdvApproxError::NullObject)?;
        column(p, index, lower, upper).map(|v| v.into_iter().copied().collect())
    }
    /// `Degree()` (0 without a result).
    pub fn degree(&self) -> i32 {
        self.degree
    }
    /// `NbKnots()`.
    pub fn nb_knots(&self) -> Result<i32> {
        let k = self.knots.as_ref().ok_or(AdvApproxError::NullObject)?;
        i32::try_from(k.len()).map_err(|_| AdvApproxError::OutOfRange)
    }
    /// `NumSubSpaces(Dimension)`: `Err(Construction)` outside `1..=3`.
    pub fn num_sub_spaces(&self, dimension: i32) -> Result<i32> {
        if !(1..=3).contains(&dimension) {
            return Err(AdvApproxError::Construction);
        }
        at(&self.num_sub_spaces, i64::from(dimension) - 1)
    }
    /// `Knots()`.
    pub fn knots(&self) -> Option<&[f64]> {
        self.knots.as_deref()
    }
    /// `Multiplicities()`.
    pub fn multiplicities(&self) -> Option<&[i32]> {
        self.mults.as_deref()
    }
    /// `MaxError(Dimension)`: `Err(OutOfRange)` outside `1..=3`, `None` for a null handle.
    pub fn max_error_array(&self, dimension: i32) -> Result<Option<&[f64]>> {
        match dimension {
            1 => Ok(self.one_d_max_error.as_deref()),
            2 => Ok(self.two_d_max_error.as_deref()),
            3 => Ok(self.three_d_max_error.as_deref()),
            _ => Err(AdvApproxError::OutOfRange),
        }
    }
    /// `AverageError(Dimension)`.
    pub fn average_error_array(&self, dimension: i32) -> Result<Option<&[f64]>> {
        match dimension {
            1 => Ok(self.one_d_average_error.as_deref()),
            2 => Ok(self.two_d_average_error.as_deref()),
            3 => Ok(self.three_d_average_error.as_deref()),
            _ => Err(AdvApproxError::OutOfRange),
        }
    }
    /// `MaxError(Dimension, Index)` (from 1).
    pub fn max_error(&self, dimension: i32, index: i32) -> Result<f64> {
        let e = self.max_error_array(dimension)?.ok_or(AdvApproxError::NullObject)?;
        at(e, i64::from(index) - 1)
    }
    /// `AverageError(Dimension, Index)` (from 1).
    pub fn average_error(&self, dimension: i32, index: i32) -> Result<f64> {
        let e = self.average_error_array(dimension)?.ok_or(AdvApproxError::NullObject)?;
        at(e, i64::from(index) - 1)
    }
    /// `Dump(o)`.
    pub fn dump(&self) -> Result<String> {
        let mut s = String::from("Dump of ApproxAFunction\n");
        for (d, title) in [(1, "Error(s) 1d = \n"), (2, "Error(s) 2d = \n"), (3, "Error(s) 3d = \n")] {
            let n = self.num_sub_spaces(d)?;
            if n > 0 {
                s.push_str(title);
                for ii in 1..=n {
                    s.push_str("   ");
                    s.push_str(&ostream_g(self.max_error(d, ii)?, 6));
                    s.push('\n');
                }
            }
        }
        Ok(s)
    }
}

/// The items `(lower..=upper, index)` (1-based, as the C++ `Value(ii, Index)`) of a 2D array.
fn column<T>(p: &Array2<T>, index: i32, lower: i32, upper: i32) -> Result<Vec<&T>> {
    let col = usize::try_from(i64::from(index) - 1).map_err(|_| AdvApproxError::OutOfRange)?;
    let mut v = Vec::new();
    for ii in lower..=upper {
        let row = usize::try_from(i64::from(ii) - 1).map_err(|_| AdvApproxError::OutOfRange)?;
        v.push(p.get(row, col).ok_or(AdvApproxError::OutOfRange)?);
    }
    Ok(v)
}
