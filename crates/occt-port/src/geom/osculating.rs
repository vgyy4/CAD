//! `Geom_OsculatingSurface`: the osculating surfaces of a B-spline or Bezier surface along the iso-curves that
//! degenerate to a point.
use super::{BSplineSurface, BezierSurface, GeomError, Result, Surface};
use crate::bsplclib;
use crate::bsplslib;
use crate::convert::GridPolynomialToPoles;
use crate::plib::{self, Array2};
use std::sync::Arc;

/// `Geom_OsculatingSurface`.
#[derive(Debug, Clone)]
pub(super) struct OsculatingSurface {
    basis_surf: Arc<dyn Surface>,
    osc1: Vec<BSplineSurface>,
    osc2: Vec<BSplineSurface>,
    kdeg: Vec<i32>,
    along: [bool; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Iso {
    V,
    U,
}

// Geom_OsculatingSurface::isQPunctual
fn is_q_punctual(s: &dyn Surface, param: f64, it: Iso, tol_min: f64, tol_max: f64) -> Result<bool> {
    let (u1, u2, v1, v2) = s.bounds();
    let mut along = true;
    let mut d1_norm_max = 0.0f64;
    if it == Iso::V {
        let step = (u2 - u1) / 10.0;
        let mut t = u1;
        while t <= u2 {
            let r = s.eval_d1(t, param)?;
            d1_norm_max = d1_norm_max.max(r.d1u.magnitude());
            t += step;
        }
    } else {
        let step = (v2 - v1) / 10.0;
        let mut t = v1;
        while t <= v2 {
            let r = s.eval_d1(param, t)?;
            d1_norm_max = d1_norm_max.max(r.d1v.magnitude());
            t += step;
        }
    }
    if d1_norm_max > tol_max || d1_norm_max < tol_min {
        along = false;
    }
    Ok(along)
}

impl OsculatingSurface {
    fn is_along_u(&self) -> bool {
        self.along[0] || self.along[1]
    }

    fn is_along_v(&self) -> bool {
        self.along[2] || self.along[3]
    }

    fn clear_flags(&mut self) {
        self.along = [false; 4];
    }

    /// `Geom_OsculatingSurface(theBS, theTol)`.
    pub(super) fn new(bs: &dyn Surface, tol: f64) -> Result<Self> {
        let basis_surf: Arc<dyn Surface> = Arc::from(bs.copy_surface());
        let mut s =
            OsculatingSurface { basis_surf, osc1: Vec::new(), osc2: Vec::new(), kdeg: Vec::new(), along: [false; 4] };
        s.init(bs, tol)?;
        Ok(s)
    }

    // One chain of `buildOsculatingSurface` calls: (the last `OsculSurf`, `L`, `k`).
    fn chain(
        &self,
        init: &BSplineSurface,
        param: f64,
        mut uknot: i32,
        mut vknot: i32,
        it: Iso,
        tol: f64,
    ) -> Result<(bool, Option<BSplineSurface>, i32)> {
        let mut s = init.clone();
        let mut k = 0;
        let mut l: Option<BSplineSurface> = None;
        let mut is_q_punc = true;
        let mut oscul_surf = true;
        while is_q_punc {
            match self.build(param, uknot, vknot, &s)? {
                None => {
                    oscul_surf = false;
                    break;
                }
                Some(built) => {
                    k += 1;
                    is_q_punc = is_q_punctual(&built, param, it, 0.0, tol)?;
                    uknot = 1;
                    vknot = 1;
                    s = built.clone();
                    l = Some(built);
                }
            }
        }
        Ok((oscul_surf, l, k))
    }

    // Geom_OsculatingSurface::Init
    fn init(&mut self, bs: &dyn Surface, tol: f64) -> Result<()> {
        self.clear_flags();
        self.osc1.clear();
        self.osc2.clear();
        self.kdeg.clear();
        let tol_min = 0.0;
        let is_bspline = bs.as_any().is::<BSplineSurface>();
        let bezier = bs.as_any().downcast_ref::<BezierSurface>();
        if !(is_bspline || bezier.is_some()) {
            self.clear_flags();
            return Ok(());
        }
        let (u1, u2, v1, v2) = bs.bounds();
        self.along[0] = is_q_punctual(bs, v1, Iso::V, tol_min, tol)?;
        self.along[1] = is_q_punctual(bs, v2, Iso::V, tol_min, tol)?;
        self.along[2] = is_q_punctual(bs, u1, Iso::U, tol_min, tol)?;
        self.along[3] = is_q_punctual(bs, u2, Iso::U, tol_min, tol)?;
        if !(self.along[0] || self.along[1] || self.along[2] || self.along[3]) {
            return Ok(());
        }
        let init_surf: BSplineSurface = if let Some(bz) = bezier {
            let uk = [0.0, 1.0];
            let vk = [0.0, 1.0];
            let um = [bz.u_degree() + 1, bz.u_degree() + 1];
            let vm = [bz.v_degree() + 1, bz.v_degree() + 1];
            BSplineSurface::new(bz.poles(), &uk, &vk, &um, &vm, bz.u_degree(), bz.v_degree(), false, false)?
        } else {
            self.basis_surf.as_any().downcast_ref::<BSplineSurface>().cloned().ok_or(GeomError::Construction)?
        };
        if self.is_along_u() && self.is_along_v() {
            self.clear_flags();
        }
        if (self.is_along_u() && init_surf.v_degree() > 1) || (self.is_along_v() && init_surf.u_degree() > 1) {
            if self.along[0] || self.along[1] {
                let mut i = 1;
                while i < init_surf.nb_u_knots() {
                    if self.along[0] {
                        let (ok, l, _k) = self.chain(&init_surf, v1, i, 1, Iso::V, tol)?;
                        if ok {
                            if let Some(l) = l {
                                self.osc1.push(l);
                            }
                        } else {
                            self.clear_flags();
                        }
                        if self.along[1] && ok {
                            let (ok2, l2, k2) =
                                self.chain(&init_surf, v2, i, init_surf.nb_v_knots() - 1, Iso::V, tol)?;
                            if ok2 {
                                if let Some(l2) = l2 {
                                    self.osc2.push(l2);
                                }
                                self.kdeg.push(k2);
                            }
                        }
                    } else {
                        let (ok, l, k) = self.chain(&init_surf, v2, i, init_surf.nb_v_knots() - 1, Iso::V, tol)?;
                        if ok {
                            if let Some(l) = l {
                                self.osc2.push(l);
                            }
                            self.kdeg.push(k);
                        } else {
                            self.clear_flags();
                        }
                    }
                    i += 1;
                }
            }
            if self.along[2] || self.along[3] {
                let mut i = 1;
                while i < init_surf.nb_v_knots() {
                    if self.along[2] {
                        let (ok, l, _k) = self.chain(&init_surf, u1, 1, i, Iso::U, tol)?;
                        if ok {
                            if let Some(l) = l {
                                self.osc1.push(l);
                            }
                        } else {
                            self.clear_flags();
                        }
                        if self.along[3] && ok {
                            let (ok2, l2, k2) =
                                self.chain(&init_surf, u2, init_surf.nb_u_knots() - 1, i, Iso::U, tol)?;
                            if ok2 {
                                if let Some(l2) = l2 {
                                    self.osc2.push(l2);
                                }
                                self.kdeg.push(k2);
                            }
                        }
                    } else {
                        let (ok, l, k) = self.chain(&init_surf, u2, init_surf.nb_u_knots() - 1, i, Iso::U, tol)?;
                        if ok {
                            if let Some(l) = l {
                                self.osc2.push(l);
                            }
                            self.kdeg.push(k);
                        } else {
                            self.clear_flags();
                        }
                    }
                    i += 1;
                }
            }
        } else {
            self.clear_flags();
        }
        Ok(())
    }

    /// `UOsculatingSurface(U, V, T, L)`: `along`, and `t` / `l` are written as the C++ out-parameters.
    #[allow(unused_assignments)]
    pub(super) fn u_osculating_surface(&self, u: f64, v: f64, t: &mut bool, l: &mut Option<BSplineSurface>) -> bool {
        let mut along = false;
        if self.along[0] || self.along[1] {
            let mut nu = 1;
            let mut nv = 1;
            *t = false;
            let nb_vk;
            let mut is_to_skip_second = false;
            if let Some(bs) = self.basis_surf.as_any().downcast_ref::<BSplineSurface>() {
                let nb_uk = bs.nb_u_knots();
                nb_vk = bs.nb_v_knots();
                nu = bsplclib::flat::hunt(bs.u_knots(), u).unwrap_or(0);
                nv = bsplclib::flat::hunt(bs.v_knots(), v).unwrap_or(0);
                if nu < 1 {
                    nu = 1;
                }
                if nu >= nb_uk {
                    nu = nb_uk - 1;
                }
                if nb_vk == 2 && nv == 1 {
                    let vk = bs.v_knots();
                    if let (Some(last), Some(first)) = (vk.last(), vk.first())
                        && last - v > v - first
                    {
                        is_to_skip_second = true;
                    }
                }
            } else {
                nu = 1;
                nv = 1;
                nb_vk = 2;
            }
            if self.along[0]
                && nv == 1
                && let Some(s) = self.osc1.get((nu - 1).max(0) as usize)
            {
                *l = Some(s.clone());
                along = true;
            }
            if self.along[1]
                && nv == nb_vk - 1
                && !is_to_skip_second
                && let Some(s) = self.osc2.get((nu - 1).max(0) as usize)
            {
                if self.kdeg.get((nu - 1).max(0) as usize).copied().unwrap_or(0) % 2 != 0 {
                    *t = true;
                }
                *l = Some(s.clone());
                along = true;
            }
        }
        along
    }

    /// `VOsculatingSurface(U, V, T, L)`.
    #[allow(unused_assignments)]
    pub(super) fn v_osculating_surface(&self, u: f64, v: f64, t: &mut bool, l: &mut Option<BSplineSurface>) -> bool {
        let mut along = false;
        if self.along[2] || self.along[3] {
            let mut nu = 1;
            let mut nv = 1;
            *t = false;
            let nb_uk;
            let mut is_to_skip_second = false;
            if let Some(bs) = self.basis_surf.as_any().downcast_ref::<BSplineSurface>() {
                nb_uk = bs.nb_u_knots();
                let nb_vk = bs.nb_v_knots();
                nu = bsplclib::flat::hunt(bs.u_knots(), u).unwrap_or(0);
                nv = bsplclib::flat::hunt(bs.v_knots(), v).unwrap_or(0);
                if nv < 1 {
                    nv = 1;
                }
                if nv >= nb_vk {
                    nv = nb_vk - 1;
                }
                if nb_uk == 2 && nu == 1 {
                    let uk = bs.u_knots();
                    if let (Some(last), Some(first)) = (uk.last(), uk.first())
                        && last - u > u - first
                    {
                        is_to_skip_second = true;
                    }
                }
            } else {
                nu = 1;
                nv = 1;
                nb_uk = 2;
            }
            if self.along[2]
                && nu == 1
                && let Some(s) = self.osc1.get((nv - 1).max(0) as usize)
            {
                *l = Some(s.clone());
                along = true;
            }
            if self.along[3]
                && nu == nb_uk - 1
                && !is_to_skip_second
                && let Some(s) = self.osc2.get((nv - 1).max(0) as usize)
            {
                if self.kdeg.get((nv - 1).max(0) as usize).copied().unwrap_or(0) % 2 != 0 {
                    *t = true;
                }
                *l = Some(s.clone());
                along = true;
            }
        }
        along
    }

    // Geom_OsculatingSurface::buildOsculatingSurface: `None` is the C++ `false`.
    fn build(&self, the_param: f64, s_uknot: i32, s_vknot: i32, bs: &BSplineSurface) -> Result<Option<BSplineSurface>> {
        let udeg = bs.u_degree();
        let vdeg = bs.v_degree();
        if (self.is_along_u() && vdeg <= 1) || (self.is_along_v() && udeg <= 1) {
            return Ok(None);
        }
        let min_degree = udeg.min(vdeg);
        let max_degree = udeg.max(vdeg);
        let _ = (min_degree, max_degree);
        let mut max_u_degree = udeg;
        let mut max_v_degree = vdeg;
        let poly_u = [0.0, 1.0];
        let poly_v = [0.0, 1.0];
        let true_u = [bs.u_knot(s_uknot)?, bs.u_knot(s_uknot + 1)?];
        let true_v = [bs.v_knot(s_vknot)?, bs.v_knot(s_vknot + 1)?];
        let mut osc_u_num_coeff = 0;
        let mut osc_v_num_coeff = 0;
        if self.is_along_u() {
            osc_u_num_coeff = udeg + 1;
            osc_v_num_coeff = vdeg;
        }
        if self.is_along_v() {
            osc_u_num_coeff = udeg;
            osc_v_num_coeff = vdeg + 1;
        }
        let nbc = osc_u_num_coeff * osc_v_num_coeff * 3;
        if nbc == 0 {
            return Ok(None);
        }
        let mut coefficients: Vec<f64> = Vec::with_capacity(nbc as usize);
        let mut ucacheparameter = bs.u_knot(s_uknot)?;
        let mut vcacheparameter = bs.v_knot(s_vknot)?;
        let vspanlength = bs.v_knot(s_vknot + 1)? - bs.v_knot(s_vknot)?;
        let uspanlength = bs.u_knot(s_uknot + 1)? - bs.u_knot(s_uknot)?;
        let is_v_negative = the_param > vcacheparameter + vspanlength / 2.0;
        let is_u_negative = the_param > ucacheparameter + uspanlength / 2.0;
        if self.is_along_u() && (the_param > vcacheparameter + vspanlength / 2.0) {
            vcacheparameter += vspanlength;
        }
        if self.is_along_v() && (the_param > ucacheparameter + uspanlength / 2.0) {
            ucacheparameter += uspanlength;
        }
        let poles = super::to_array2(bs.poles())?;
        let (cachepoles, _) = bsplslib::build_cache(
            ucacheparameter,
            vcacheparameter,
            uspanlength,
            vspanlength,
            bs.is_u_periodic(),
            bs.is_v_periodic(),
            udeg,
            vdeg,
            0,
            0,
            bs.u_knot_sequence(),
            bs.v_knot_sequence(),
            &poles,
            None,
        )?;
        let cp = |r: i32, c: i32| -> Result<crate::gp::Pnt> {
            cachepoles.get((r - 1) as usize, (c - 1) as usize).copied().ok_or(GeomError::OutOfRange)
        };
        let (ou, ov) = (osc_u_num_coeff as usize, osc_v_num_coeff as usize);
        let mut osc_coeff: Vec<crate::gp::Pnt> = Vec::with_capacity(ou * ov);
        if self.is_along_u() {
            for n in 1..=udeg + 1 {
                for m in 1..=vdeg {
                    osc_coeff.push(if udeg > vdeg { cp(n, m + 1)? } else { cp(m + 1, n)? });
                }
            }
            let mut arr = Array2::new(ou, ov, osc_coeff)?;
            if is_v_negative {
                arr = plib::v_trimming(-1.0, 0.0, &arr, None)?.0;
            }
            for p in arr.as_slice() {
                coefficients.push(p.x());
                coefficients.push(p.y());
                coefficients.push(p.z());
            }
        }
        if self.is_along_v() {
            osc_coeff = Vec::with_capacity(ou * ov);
            for n in 1..=udeg {
                for m in 1..=vdeg + 1 {
                    osc_coeff.push(if udeg > vdeg { cp(n + 1, m)? } else { cp(m, n + 1)? });
                }
            }
            let mut arr = Array2::new(ou, ov, osc_coeff)?;
            if is_u_negative {
                arr = plib::u_trimming(-1.0, 0.0, &arr, None)?.0;
            }
            for p in arr.as_slice() {
                coefficients.push(p.x());
                coefficients.push(p.y());
                coefficients.push(p.z());
            }
        }
        if self.is_along_u() {
            max_v_degree -= 1;
        }
        if self.is_along_v() {
            max_u_degree -= 1;
        }
        let data = GridPolynomialToPoles::multi(
            1,
            1,
            -1,
            -1,
            max_u_degree,
            max_v_degree,
            &[[osc_u_num_coeff, osc_v_num_coeff]],
            &coefficients,
            &poly_u,
            &poly_v,
            &true_u,
            &true_v,
        )
        .map_err(|_| GeomError::Construction)?;
        let cv = |e: crate::convert::ConvertError| {
            let _ = e;
            GeomError::Construction
        };
        let surf = BSplineSurface::new(
            &super::from_array2(data.poles().map_err(cv)?),
            data.u_knots().map_err(cv)?,
            data.v_knots().map_err(cv)?,
            data.u_multiplicities().map_err(cv)?,
            data.v_multiplicities().map_err(cv)?,
            data.u_degree().map_err(cv)?,
            data.v_degree().map_err(cv)?,
            false,
            false,
        )?;
        Ok(Some(surf))
    }
}
