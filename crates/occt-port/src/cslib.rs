//! Surface normal and polygon classification helpers from OCCT's CSLib package.
use crate::{
    gp::{Dir, Vec as GpVec},
    math1d::{Function, FunctionRoots},
    mathlin::MathError,
    plib,
};
mod class2d;
mod poly;
pub use class2d::{Class2d, Class2dResult};
pub use poly::NormalPolyDef;
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalStatus {
    Singular = 0,
    Defined = 1,
    InfinityOfSolutions = 2,
    D1NuIsNull = 3,
    D1NvIsNull = 4,
    D1NIsNull = 5,
    D1NuNvRatioIsNull = 6,
    D1NvNuRatioIsNull = 7,
    D1NuIsParallelD1Nv = 8,
}
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivativeStatus {
    Done = 0,
    D1uIsNull = 1,
    D1vIsNull = 2,
    D1IsNull = 3,
    D1uD1vRatioIsNull = 4,
    D1vD1uRatioIsNull = 5,
    D1uIsParallelD1v = 6,
}

/// `gp::Resolution()`.
const RESOLUTION: f64 = f64::MIN_POSITIVE;
/// `RealEpsilon()`.
const REAL_EPSILON: f64 = f64::EPSILON;
/// `Precision::Confusion()`.
const CONFUSION: f64 = 1.0e-7;
/// `Precision::PConfusion()` (evaluated in `f64`).
const P_CONFUSION: f64 = 1.0e-7 * 0.01;
/// `THE_PARALLEL_ANGULAR_TOL`.
const PARALLEL_ANGULAR_TOL: f64 = 1e-6;
/// `THE_MAX_ROOT_ITERATIONS`.
const MAX_ROOT_ITERATIONS: i32 = 200;
/// `THE_ROOT_FINDING_TOL`.
const ROOT_FINDING_TOL: f64 = 1e-5;

/// The out-parameters of `CSLib::Normal` with derivatives of any order.
#[derive(Debug, Clone)]
pub struct NormalOfOrder {
    pub status: NormalStatus,
    pub normal: Option<Dir>,
    pub order_u: i32,
    pub order_v: i32,
}

/// `NCollection_Array2<gp_Vec>`.
#[derive(Debug, Clone)]
pub struct VecArray2 {
    row_lower: i32,
    col_lower: i32,
    rows: usize,
    cols: usize,
    values: Vec<GpVec>,
}

impl VecArray2 {
    /// An array filled row by row from `values`.
    pub fn new(
        row_lower: i32,
        col_lower: i32,
        rows: usize,
        cols: usize,
        values: Vec<GpVec>,
    ) -> Result<Self, MathError> {
        if rows == 0 || cols == 0 || rows.checked_mul(cols) != Some(values.len()) {
            return Err(MathError::Dimension);
        }
        Ok(Self { row_lower, col_lower, rows, cols, values })
    }
    pub fn row_lower(&self) -> i32 {
        self.row_lower
    }
    pub fn row_upper(&self) -> i32 {
        self.row_lower.saturating_add(i32::try_from(self.rows).unwrap_or(i32::MAX)).saturating_sub(1)
    }
    pub fn col_lower(&self) -> i32 {
        self.col_lower
    }
    pub fn col_upper(&self) -> i32 {
        self.col_lower.saturating_add(i32::try_from(self.cols).unwrap_or(i32::MAX)).saturating_sub(1)
    }
    /// `Value(row, col)`; `Err(Range)` outside the bounds.
    pub fn value(&self, row: i32, col: i32) -> Result<&GpVec, MathError> {
        let r =
            usize::try_from(row.checked_sub(self.row_lower).ok_or(MathError::Range)?).map_err(|_| MathError::Range)?;
        let c =
            usize::try_from(col.checked_sub(self.col_lower).ok_or(MathError::Range)?).map_err(|_| MathError::Range)?;
        if r >= self.rows || c >= self.cols {
            return Err(MathError::Range);
        }
        self.values.get(r * self.cols + c).ok_or(MathError::Range)
    }
}

/// A square work array indexed `0..=n` in both directions (`NCollection_Array2<T>(0, n, 0, n)`).
struct Grid<T> {
    n: usize,
    values: Vec<T>,
}

impl<T: Copy> Grid<T> {
    fn new(k: i32, init: T) -> Result<Self, MathError> {
        let n = usize::try_from(k).map_err(|_| MathError::Range)?.checked_add(1).ok_or(MathError::Range)?;
        Ok(Self { n, values: vec![init; n.checked_mul(n).ok_or(MathError::Range)?] })
    }
    fn slot(&self, i: i32, j: i32) -> Result<usize, MathError> {
        let r = usize::try_from(i).map_err(|_| MathError::Range)?;
        let c = usize::try_from(j).map_err(|_| MathError::Range)?;
        if r >= self.n || c >= self.n {
            return Err(MathError::Range);
        }
        Ok(r * self.n + c)
    }
    fn get(&self, i: i32, j: i32) -> Result<T, MathError> {
        self.values.get(self.slot(i, j)?).copied().ok_or(MathError::Range)
    }
    fn set(&mut self, i: i32, j: i32, v: T) -> Result<(), MathError> {
        let s = self.slot(i, j)?;
        *self.values.get_mut(s).ok_or(MathError::Range)? = v;
        Ok(())
    }
}

/// `gp_Dir(gp_Vec)`: normalizes; fails for a null vector (unchecked in OCCT).
fn dir_of(v: &GpVec) -> Result<Dir, MathError> {
    Dir::from_vec(v).map_err(|_| MathError::Domain)
}

/// `PLib::Bin`.
fn bin(n: i32, p: i32) -> Result<f64, MathError> {
    plib::bin(n, p).map_err(|_| MathError::Range)
}

/// `CSLib::Normal(D1U, D1V, SinTol, Status, Normal)`.
pub fn normal_sin_tol(d1u: &GpVec, d1v: &GpVec, sin_tol: f64) -> Result<(DerivativeStatus, Option<Dir>), MathError> {
    let d1u_mag = d1u.square_magnitude();
    let d1v_mag = d1v.square_magnitude();
    let d1u_x_d1v = d1u.crossed(d1v);
    if d1u_mag <= RESOLUTION && d1v_mag <= RESOLUTION {
        Ok((DerivativeStatus::D1IsNull, None))
    } else if d1u_mag <= RESOLUTION {
        Ok((DerivativeStatus::D1uIsNull, None))
    } else if d1v_mag <= RESOLUTION {
        Ok((DerivativeStatus::D1vIsNull, None))
    } else {
        let sin2 = d1u_x_d1v.square_magnitude() / (d1u_mag * d1v_mag);
        if sin2 < sin_tol * sin_tol {
            Ok((DerivativeStatus::D1uIsParallelD1v, None))
        } else {
            Ok((DerivativeStatus::Done, Some(dir_of(&d1u_x_d1v)?)))
        }
    }
}

/// `CSLib::Normal(D1U, D1V, D2U, D2V, D2UV, SinTol, Done, Status, Normal)`.
pub fn normal_second_order(
    d1u: &GpVec,
    d1v: &GpVec,
    d2u: &GpVec,
    d2v: &GpVec,
    d2uv: &GpVec,
    sin_tol: f64,
) -> Result<(bool, NormalStatus, Option<Dir>), MathError> {
    let mut d1nu = d2u.crossed(d1v);
    d1nu.add(&d1u.crossed(d2uv));
    let mut d1nv = d2uv.crossed(d1v);
    d1nv.add(&d1u.crossed(d2v));
    let l_d1nu = d1nu.square_magnitude();
    let l_d1nv = d1nv.square_magnitude();
    if l_d1nu <= REAL_EPSILON && l_d1nv <= REAL_EPSILON {
        Ok((false, NormalStatus::D1NIsNull, None))
    } else if l_d1nu < REAL_EPSILON {
        Ok((true, NormalStatus::D1NuIsNull, Some(dir_of(&d1nv)?)))
    } else if l_d1nv < REAL_EPSILON {
        Ok((true, NormalStatus::D1NvIsNull, Some(dir_of(&d1nu)?)))
    } else if (l_d1nv / l_d1nu) <= REAL_EPSILON {
        Ok((false, NormalStatus::D1NvNuRatioIsNull, None))
    } else if (l_d1nu / l_d1nv) <= REAL_EPSILON {
        Ok((false, NormalStatus::D1NuNvRatioIsNull, None))
    } else {
        let d1n_cross = d1nu.crossed(&d1nv);
        let sin2 = d1n_cross.square_magnitude() / (l_d1nu * l_d1nv);
        if sin2 < sin_tol * sin_tol {
            Ok((true, NormalStatus::D1NuIsParallelD1Nv, Some(dir_of(&d1nu)?)))
        } else {
            Ok((false, NormalStatus::InfinityOfSolutions, None))
        }
    }
}

/// `CSLib::Normal(D1U, D1V, MagTol, Status, Normal)`.
pub fn normal_mag_tol(d1u: &GpVec, d1v: &GpVec, mag_tol: f64) -> Result<(NormalStatus, Option<Dir>), MathError> {
    let d1u_mag = d1u.magnitude();
    let d1v_mag = d1v.magnitude();
    let d1u_x_d1v = d1u.crossed(d1v);
    let n_mag = d1u_x_d1v.magnitude();
    if n_mag <= mag_tol || d1u_mag <= mag_tol || d1v_mag <= mag_tol {
        Ok((NormalStatus::Singular, None))
    } else {
        let d1u_dir = dir_of(d1u)?;
        let d1v_dir = dir_of(d1v)?;
        let normal = d1u_dir.crossed(&d1v_dir).map_err(|_| MathError::Domain)?;
        Ok((NormalStatus::Defined, Some(normal)))
    }
}

/// `CSLib::Normal(MaxOrder, DerNUV, MagTol, U, V, Umin, Umax, Vmin, Vmax, ...)`.
#[allow(clippy::too_many_arguments)]
pub fn normal_of_order(
    max_order: i32,
    der_nuv: &VecArray2,
    sin_tol: f64,
    u: f64,
    v: f64,
    u_min: f64,
    u_max: f64,
    v_min: f64,
    v_max: f64,
) -> Result<NormalOfOrder, MathError> {
    if max_order >= 0 {
        // The C++ reads rows and columns `0..=max_order` unchecked.
        der_nuv.value(0, 0)?;
        der_nuv.value(max_order, max_order)?;
    }
    let mut order: i32 = -1;
    let mut found_u_idx: i32 = 0;
    let mut found = false;
    let mut d = GpVec::zero();
    while !found && order < max_order {
        order += 1;
        found_u_idx = order;
        while found_u_idx >= 0 && !found {
            let v_idx = order - found_u_idx;
            d = *der_nuv.value(found_u_idx, v_idx)?;
            let norme = d.magnitude();
            found = norme >= sin_tol;
            found_u_idx -= 1;
        }
    }
    let order_u = found_u_idx + 1;
    let order_v = order - order_u;
    if !found {
        return Ok(NormalOfOrder { status: NormalStatus::Singular, normal: None, order_u, order_v });
    }
    let normalized_d = |d: &GpVec| -> Result<Dir, MathError> {
        let n = d.normalized().map_err(|_| MathError::Domain)?;
        dir_of(&n)
    };
    if order == 0 {
        return Ok(NormalOfOrder { status: NormalStatus::Defined, normal: Some(normalized_d(&d)?), order_u, order_v });
    }
    let vk0 = *der_nuv.value(order_u, order_v)?;
    let mut ratio = vec![0.0f64; usize::try_from(order).map_err(|_| MathError::Range)? + 1];
    let mut ratio_idx: i32 = 0;
    let mut is_defined = false;
    while ratio_idx <= order && !is_defined {
        let der_vec = der_nuv.value(ratio_idx, order - ratio_idx)?;
        let slot = ratio.get_mut(usize::try_from(ratio_idx).map_err(|_| MathError::Range)?).ok_or(MathError::Range)?;
        if der_vec.magnitude() <= sin_tol {
            *slot = 0.0;
        } else if der_vec.is_parallel(&vk0, PARALLEL_ANGULAR_TOL).map_err(|_| MathError::Domain)? {
            let mut magnitude_ratio = der_vec.magnitude() / vk0.magnitude();
            if der_vec.is_opposite(&vk0, PARALLEL_ANGULAR_TOL).map_err(|_| MathError::Domain)? {
                magnitude_ratio = -magnitude_ratio;
            }
            *slot = magnitude_ratio;
        } else {
            is_defined = true;
        }
        ratio_idx += 1;
    }
    if is_defined {
        return Ok(NormalOfOrder { status: NormalStatus::Defined, normal: Some(normalized_d(&d)?), order_u, order_v });
    }
    let mut inf = -std::f64::consts::PI;
    let mut sup = std::f64::consts::PI;
    let is_fu = (u - u_min).abs() < P_CONFUSION;
    let is_lu = (u - u_max).abs() < P_CONFUSION;
    let is_fv = (v - v_min).abs() < P_CONFUSION;
    let is_lv = (v - v_max).abs() < P_CONFUSION;
    let pi = std::f64::consts::PI;
    if is_lu {
        inf = pi / 2.0;
        sup = 3.0 * pi / 2.0;
        if is_lv {
            inf = pi;
        }
        if is_fv {
            sup = pi;
        }
    } else if is_fu {
        sup = pi / 2.0;
        inf = -pi / 2.0;
        if is_lv {
            sup = 0.0;
        }
        if is_fv {
            inf = 0.0;
        }
    } else if is_lv {
        inf = -pi;
        sup = 0.0;
    } else if is_fv {
        inf = 0.0;
        sup = pi;
    }
    let mut changes_sign = false;
    let mut vprec = 0.0f64;
    let mut vsuiv = 0.0f64;
    let mut poly = NormalPolyDef::new(order, 0, &ratio)?;
    let find_roots =
        FunctionRoots::new(&mut poly, inf, sup, MAX_ROOT_ITERATIONS, ROOT_FINDING_TOL, CONFUSION, CONFUSION, 0.0);
    let nb_sol = if find_roots.is_done() { find_roots.nb_solutions()? } else { 0 };
    if find_roots.is_done() && nb_sol > 0 {
        let mut sol = vec![0.0f64; usize::try_from(nb_sol).map_err(|_| MathError::Range)? + 2];
        for root_idx in 1..=nb_sol {
            *sol.get_mut(usize::try_from(root_idx).map_err(|_| MathError::Range)?).ok_or(MathError::Range)? =
                find_roots.value(root_idx)?;
        }
        let last = usize::try_from(nb_sol).map_err(|_| MathError::Range)?;
        sol.get_mut(1..=last)
            .ok_or(MathError::Range)?
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        *sol.first_mut().ok_or(MathError::Range)? = inf;
        *sol.get_mut(last + 1).ok_or(MathError::Range)? = sup;
        let mut first = 0;
        for interval_idx in 0..=last {
            let lo = *sol.get(interval_idx).ok_or(MathError::Range)?;
            let hi = *sol.get(interval_idx + 1).ok_or(MathError::Range)?;
            if (hi - lo).abs() > P_CONFUSION {
                vsuiv = Function::value(&mut poly, (lo + hi) / 2.0).ok_or(MathError::NotDone)?;
                if first == 0 {
                    first = interval_idx;
                    changes_sign = false;
                    vprec = vsuiv;
                } else {
                    changes_sign = changes_sign || (vprec * vsuiv) < 0.0;
                    vprec = vsuiv;
                }
            }
        }
    } else {
        changes_sign = false;
        vsuiv = Function::value(&mut poly, inf).ok_or(MathError::NotDone)?;
    }
    if changes_sign {
        Ok(NormalOfOrder { status: NormalStatus::InfinityOfSolutions, normal: None, order_u, order_v })
    } else {
        let sign = if vsuiv > 0.0 { 1.0 } else { -1.0 };
        let vk0_normalized = vk0.normalized().map_err(|_| MathError::Domain)?;
        let normal = dir_of(&vk0_normalized.multiplied(sign))?;
        Ok(NormalOfOrder { status: NormalStatus::Defined, normal: Some(normal), order_u, order_v })
    }
}

/// `CSLib::DNNUV(Nu, Nv, DerSurf)`.
pub fn dn_nuv(nu: i32, nv: i32, der_surf: &VecArray2) -> Result<GpVec, MathError> {
    let mut result = GpVec::new(0.0, 0.0, 0.0);
    for i in 0..=nu {
        for j in 0..=nv {
            let vg = der_surf.value(i + 1, j)?;
            let vd = der_surf.value(nu - i, nv + 1 - j)?;
            let cross = vg.crossed(vd);
            let bin_coef = bin(nu, i)? * bin(nv, j)?;
            result.add(&cross.multiplied(bin_coef));
        }
    }
    Ok(result)
}

/// `CSLib::DNNUV(Nu, Nv, DerSurf1, DerSurf2)`.
pub fn dn_nuv_two(nu: i32, nv: i32, der_surf1: &VecArray2, der_surf2: &VecArray2) -> Result<GpVec, MathError> {
    let mut result = GpVec::new(0.0, 0.0, 0.0);
    for i in 0..=nu {
        for j in 0..=nv {
            let vg = der_surf1.value(i + 1, j)?;
            let vd = der_surf2.value(nu - i, nv + 1 - j)?;
            let cross = vg.crossed(vd);
            let bin_coef = bin(nu, i)? * bin(nv, j)?;
            result.add(&cross.multiplied(bin_coef));
        }
    }
    Ok(result)
}

/// `CSLib::DNNormal(Nu, Nv, DerNUV, Iduref, Idvref)`.
pub fn dn_normal(nu: i32, nv: i32, der_nuv: &VecArray2, iduref: i32, idvref: i32) -> Result<GpVec, MathError> {
    let kderiv = nu + nv;
    let mut der_vec_nor: Grid<GpVec> = Grid::new(kderiv, GpVec::zero())?;
    let mut tab_scal: Grid<f64> = Grid::new(kderiv, 0.0)?;
    let mut tab_norm: Grid<f64> = Grid::new(kderiv, 0.0)?;
    let mut der_nor = der_nuv.value(iduref, idvref)?.normalized().map_err(|_| MathError::Domain)?;
    der_vec_nor.set(0, 0, der_nor)?;
    let dnorm0 = der_nuv.value(iduref, idvref)?.dot(&der_vec_nor.get(0, 0)?);
    tab_norm.set(0, 0, dnorm0)?;
    tab_scal.set(0, 0, 0.0)?;
    for mderiv in 1..=kderiv {
        for pderiv in 0..=mderiv {
            let qderiv = mderiv - pderiv;
            if pderiv > nu || qderiv > nv {
                continue;
            }
            let mut scal = 0.0f64;
            if pderiv > qderiv {
                for jderiv in 1..=qderiv {
                    scal -= bin(qderiv, jderiv)?
                        * der_vec_nor.get(0, jderiv)?.dot(&der_vec_nor.get(pderiv, qderiv - jderiv)?);
                }
                for jderiv in 0..qderiv {
                    scal -= bin(qderiv, jderiv)?
                        * der_vec_nor.get(pderiv, jderiv)?.dot(&der_vec_nor.get(0, qderiv - jderiv)?);
                }
                for ideriv in 1..pderiv {
                    for jderiv in 0..=qderiv {
                        scal -= bin(pderiv, ideriv)?
                            * bin(qderiv, jderiv)?
                            * der_vec_nor.get(ideriv, jderiv)?.dot(&der_vec_nor.get(pderiv - ideriv, qderiv - jderiv)?);
                    }
                }
            } else {
                for ideriv in 1..=pderiv {
                    scal -= bin(pderiv, ideriv)?
                        * der_vec_nor.get(ideriv, 0)?.dot(&der_vec_nor.get(pderiv - ideriv, qderiv)?);
                }
                for ideriv in 0..pderiv {
                    scal -= bin(pderiv, ideriv)?
                        * der_vec_nor.get(ideriv, qderiv)?.dot(&der_vec_nor.get(pderiv - ideriv, 0)?);
                }
                for ideriv in 0..=pderiv {
                    for jderiv in 1..qderiv {
                        scal -= bin(pderiv, ideriv)?
                            * bin(qderiv, jderiv)?
                            * der_vec_nor.get(ideriv, jderiv)?.dot(&der_vec_nor.get(pderiv - ideriv, qderiv - jderiv)?);
                    }
                }
            }
            tab_scal.set(pderiv, qderiv, scal / 2.0)?;
            let mut dnorm = der_nuv.value(pderiv + iduref, qderiv + idvref)?.dot(&der_vec_nor.get(0, 0)?);
            for jderiv in 0..qderiv {
                dnorm -= bin(qderiv + idvref, jderiv + idvref)?
                    * tab_norm.get(pderiv, jderiv)?
                    * tab_scal.get(0, qderiv - jderiv)?;
            }
            for ideriv in 0..pderiv {
                for jderiv in 0..=qderiv {
                    dnorm -= bin(pderiv + iduref, ideriv + iduref)?
                        * bin(qderiv + idvref, jderiv + idvref)?
                        * tab_norm.get(ideriv, jderiv)?
                        * tab_scal.get(pderiv - ideriv, qderiv - jderiv)?;
                }
            }
            tab_norm.set(pderiv, qderiv, dnorm)?;
            der_nor = *der_nuv.value(pderiv + iduref, qderiv + idvref)?;
            for jderiv in 1..=qderiv {
                let coef =
                    bin(pderiv + iduref, iduref)? * bin(qderiv + idvref, jderiv + idvref)? * tab_norm.get(0, jderiv)?;
                der_nor.subtract(&der_vec_nor.get(pderiv, qderiv - jderiv)?.multiplied(coef));
            }
            for ideriv in 1..=pderiv {
                for jderiv in 0..=qderiv {
                    let coef = bin(pderiv + iduref, ideriv + iduref)?
                        * bin(qderiv + idvref, jderiv + idvref)?
                        * tab_norm.get(ideriv, jderiv)?;
                    der_nor.subtract(&der_vec_nor.get(pderiv - ideriv, qderiv - jderiv)?.multiplied(coef));
                }
            }
            der_nor.divide(bin(pderiv + iduref, iduref)? * bin(qderiv + idvref, idvref)? * tab_norm.get(0, 0)?);
            der_vec_nor.set(pderiv, qderiv, der_nor)?;
        }
    }
    der_vec_nor.get(nu, nv)
}
