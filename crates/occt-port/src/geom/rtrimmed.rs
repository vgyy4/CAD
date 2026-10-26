//! `Geom_RectangularTrimmedSurface`.
use super::{BoundedSurface, Curve, GeomAbsShape, GeomError, Geometry, OffsetSurface, Result, SurfResD1, SurfResD2};
use super::{SurfResD3, Surface, TrimmedCurve, geometry_any, pconfusion};
use crate::elclib;
use crate::gp::{self, GTrsf2d, Pnt, Trsf};
use std::sync::Arc;

/// A mutable access to the surface behind a handle (`Arc<dyn Surface>` is immutable: a shared surface is copied
/// first, the aliasing of a C++ handle is not observable).
pub(crate) fn make_unique_surface(a: &mut Arc<dyn Surface>) -> Option<&mut dyn Surface> {
    if Arc::get_mut(a).is_none() {
        *a = Arc::from(a.copy_surface());
    }
    Arc::get_mut(a)
}

/// `Geom_RectangularTrimmedSurface`.
#[derive(Debug, Clone)]
pub struct RectangularTrimmedSurface {
    basis_surf: Arc<dyn Surface>,
    utrim1: f64,
    vtrim1: f64,
    utrim2: f64,
    vtrim2: f64,
    isutrimmed: bool,
    isvtrimmed: bool,
}

impl RectangularTrimmedSurface {
    // The basis of the constructors: a copy of the surface, or of the basis of a trimmed surface; an offset surface
    // is rebuilt over a trimmed copy of its own basis.
    fn make_basis(
        s: &Arc<dyn Surface>,
        trim: &dyn Fn(&Arc<dyn Surface>) -> Result<RectangularTrimmedSurface>,
    ) -> Result<Arc<dyn Surface>> {
        let mut basis: Arc<dyn Surface> = match s.as_any().downcast_ref::<RectangularTrimmedSurface>() {
            Some(t) => Arc::from(t.basis_surf.copy_surface()),
            None => Arc::from(s.copy_surface()),
        };
        if let Some(o) = basis.as_any().downcast_ref::<OffsetSurface>() {
            let offset = o.offset();
            let s2 = trim(&o.basis_surface())?;
            basis = Arc::new(OffsetSurface::new(Arc::new(s2), offset, true)?);
        }
        Ok(basis)
    }

    fn blank(basis: Arc<dyn Surface>) -> Self {
        RectangularTrimmedSurface {
            basis_surf: basis,
            utrim1: 0.0,
            vtrim1: 0.0,
            utrim2: 0.0,
            vtrim2: 0.0,
            isutrimmed: false,
            isvtrimmed: false,
        }
    }

    /// `Geom_RectangularTrimmedSurface(S, U1, U2, V1, V2, USense, VSense)`.
    pub fn new(s: Arc<dyn Surface>, u1: f64, u2: f64, v1: f64, v2: f64, usense: bool, vsense: bool) -> Result<Self> {
        let basis =
            Self::make_basis(&s, &|b| RectangularTrimmedSurface::new(b.clone(), u1, u2, v1, v2, usense, vsense))?;
        let mut r = RectangularTrimmedSurface::blank(basis);
        r.utrim1 = u1;
        r.vtrim1 = v1;
        r.utrim2 = u2;
        r.vtrim2 = v2;
        r.isutrimmed = true;
        r.isvtrimmed = true;
        r.set_trim(u1, u2, v1, v2, usense, vsense)?;
        Ok(r)
    }

    /// `Geom_RectangularTrimmedSurface(S, Param1, Param2, UTrim, Sense)`.
    pub fn new_param(s: Arc<dyn Surface>, param1: f64, param2: f64, utrim: bool, sense: bool) -> Result<Self> {
        let basis =
            Self::make_basis(&s, &|b| RectangularTrimmedSurface::new_param(b.clone(), param1, param2, utrim, sense))?;
        let t = s.as_any().downcast_ref::<RectangularTrimmedSurface>();
        let mut r = RectangularTrimmedSurface::blank(basis);
        if let Some(t) = t {
            if utrim && t.isvtrimmed {
                r.set_trim(param1, param2, t.vtrim1, t.vtrim2, sense, true)?;
                return Ok(r);
            } else if !utrim && t.isutrimmed {
                r.set_trim(t.utrim1, t.utrim2, param1, param2, true, sense)?;
                return Ok(r);
            }
        }
        r.set_trim_param(param1, param2, utrim, sense)?;
        Ok(r)
    }

    /// `SetTrim(U1, U2, V1, V2, USense, VSense)`.
    pub fn set_trim(&mut self, u1: f64, u2: f64, v1: f64, v2: f64, usense: bool, vsense: bool) -> Result<()> {
        self.set_trim8(u1, u2, v1, v2, true, true, usense, vsense)
    }

    /// `SetTrim(Param1, Param2, UTrim, Sense)`.
    pub fn set_trim_param(&mut self, param1: f64, param2: f64, utrim: bool, sense: bool) -> Result<()> {
        let dummy_a = 0.0;
        let dummy_b = 0.0;
        let dummy_sense = true;
        if utrim {
            if self.isvtrimmed {
                self.set_trim8(param1, param2, self.vtrim1, self.vtrim2, true, true, sense, dummy_sense)
            } else {
                self.set_trim8(param1, param2, dummy_a, dummy_b, true, false, sense, dummy_sense)
            }
        } else if self.isutrimmed {
            self.set_trim8(self.utrim1, self.utrim2, param1, param2, true, true, dummy_sense, sense)
        } else {
            self.set_trim8(dummy_a, dummy_b, param1, param2, false, true, dummy_sense, sense)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn set_trim8(
        &mut self,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        utrim: bool,
        vtrim: bool,
        usense: bool,
        vsense: bool,
    ) -> Result<()> {
        let mut usame_sense = true;
        let mut vsame_sense = true;
        let (udeb, ufin, vdeb, vfin) = self.basis_surf.bounds();
        self.isutrimmed = utrim;
        if !utrim {
            self.utrim1 = udeb;
            self.utrim2 = ufin;
        } else {
            if u1 == u2 {
                return Err(GeomError::Construction);
            }
            if self.basis_surf.is_u_periodic() {
                usame_sense = usense;
                self.utrim1 = u1;
                self.utrim2 = u2;
                let (a, b) = elclib::adjust_periodic(
                    udeb,
                    ufin,
                    ((self.utrim2 - self.utrim1).abs() / 2.0).min(pconfusion()),
                    self.utrim1,
                    self.utrim2,
                );
                self.utrim1 = a;
                self.utrim2 = b;
            } else {
                if u1 < u2 {
                    usame_sense = usense;
                    self.utrim1 = u1;
                    self.utrim2 = u2;
                } else {
                    usame_sense = !usense;
                    self.utrim1 = u2;
                    self.utrim2 = u1;
                }
                if (udeb - self.utrim1 > pconfusion()) || (self.utrim2 - ufin > pconfusion()) {
                    return Err(GeomError::Construction);
                }
            }
        }
        self.isvtrimmed = vtrim;
        if !vtrim {
            self.vtrim1 = vdeb;
            self.vtrim2 = vfin;
        } else {
            if v1 == v2 {
                return Err(GeomError::Construction);
            }
            if self.basis_surf.is_v_periodic() {
                vsame_sense = vsense;
                self.vtrim1 = v1;
                self.vtrim2 = v2;
                let (a, b) = elclib::adjust_periodic(
                    vdeb,
                    vfin,
                    ((self.vtrim2 - self.vtrim1).abs() / 2.0).min(pconfusion()),
                    self.vtrim1,
                    self.vtrim2,
                );
                self.vtrim1 = a;
                self.vtrim2 = b;
            } else {
                if v1 < v2 {
                    vsame_sense = vsense;
                    self.vtrim1 = v1;
                    self.vtrim2 = v2;
                } else {
                    vsame_sense = !vsense;
                    self.vtrim1 = v2;
                    self.vtrim2 = v1;
                }
                if (vdeb - self.vtrim1 > pconfusion()) || (self.vtrim2 - vfin > pconfusion()) {
                    return Err(GeomError::Construction);
                }
            }
        }
        if !usame_sense {
            self.u_reverse();
        }
        if !vsame_sense {
            self.v_reverse();
        }
        Ok(())
    }

    /// `BasisSurface`.
    pub fn basis_surface(&self) -> Arc<dyn Surface> {
        self.basis_surf.clone()
    }

    // Copy(): `None` when neither direction is trimmed (a null handle in OCCT)
    fn copy_trimmed(&self) -> Option<RectangularTrimmedSurface> {
        if self.isutrimmed && self.isvtrimmed {
            RectangularTrimmedSurface::new(
                self.basis_surf.clone(),
                self.utrim1,
                self.utrim2,
                self.vtrim1,
                self.vtrim2,
                true,
                true,
            )
            .ok()
        } else if self.isutrimmed {
            RectangularTrimmedSurface::new_param(self.basis_surf.clone(), self.utrim1, self.utrim2, true, true).ok()
        } else if self.isvtrimmed {
            RectangularTrimmedSurface::new_param(self.basis_surf.clone(), self.vtrim1, self.vtrim2, false, true).ok()
        } else {
            None
        }
    }
}

impl Geometry for RectangularTrimmedSurface {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        if let Some(b) = make_unique_surface(&mut self.basis_surf) {
            b.transform(t);
        }
        let (u1, v1) = self.basis_surf.transform_parameters(self.utrim1, self.vtrim1, t);
        self.utrim1 = u1;
        self.vtrim1 = v1;
        let (u2, v2) = self.basis_surf.transform_parameters(self.utrim2, self.vtrim2, t);
        self.utrim2 = u2;
        self.vtrim2 = v2;
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        match self.copy_trimmed() {
            Some(c) => Box::new(c),
            None => Box::new(self.clone()),
        }
    }
}

impl Surface for RectangularTrimmedSurface {
    fn copy_surface(&self) -> Box<dyn Surface> {
        match self.copy_trimmed() {
            Some(c) => Box::new(c),
            None => Box::new(self.clone()),
        }
    }
    fn u_reverse(&mut self) {
        let u1 = self.basis_surf.u_reversed_parameter(self.utrim2);
        let u2 = self.basis_surf.u_reversed_parameter(self.utrim1);
        if let Some(b) = make_unique_surface(&mut self.basis_surf) {
            b.u_reverse();
        }
        let _ = self.set_trim8(u1, u2, self.vtrim1, self.vtrim2, self.isutrimmed, self.isvtrimmed, true, true);
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        self.basis_surf.u_reversed_parameter(u)
    }
    fn v_reverse(&mut self) {
        let v1 = self.basis_surf.v_reversed_parameter(self.vtrim2);
        let v2 = self.basis_surf.v_reversed_parameter(self.vtrim1);
        if let Some(b) = make_unique_surface(&mut self.basis_surf) {
            b.v_reverse();
        }
        let _ = self.set_trim8(self.utrim1, self.utrim2, v1, v2, self.isutrimmed, self.isvtrimmed, true, true);
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        self.basis_surf.v_reversed_parameter(v)
    }
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64) {
        self.basis_surf.transform_parameters(u, v, t)
    }
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d {
        self.basis_surf.parametric_transformation(t)
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.utrim1, self.utrim2, self.vtrim1, self.vtrim2)
    }
    fn is_u_closed(&self) -> bool {
        if !self.isutrimmed {
            return self.basis_surf.is_u_closed();
        }
        if self.basis_surf.is_u_periodic()
            && let Ok(period) = self.basis_surf.u_period()
        {
            let length = self.utrim2 - self.utrim1;
            if length > pconfusion() && libm::remainder(length, period).abs() <= pconfusion() {
                return true;
            }
        }
        false
    }
    fn is_v_closed(&self) -> bool {
        if !self.isvtrimmed {
            return self.basis_surf.is_v_closed();
        }
        if self.basis_surf.is_v_periodic()
            && let Ok(period) = self.basis_surf.v_period()
        {
            let length = self.vtrim2 - self.vtrim1;
            if length > pconfusion() && libm::remainder(length, period).abs() <= pconfusion() {
                return true;
            }
        }
        false
    }
    fn is_u_periodic(&self) -> bool {
        if !self.basis_surf.is_u_periodic() {
            return false;
        }
        if !self.isutrimmed {
            return true;
        }
        let Ok(period) = self.basis_surf.u_period() else { return false };
        let length = self.utrim2 - self.utrim1;
        length > pconfusion() && libm::remainder(length, period).abs() <= pconfusion()
    }
    fn u_period(&self) -> Result<f64> {
        // (the golden file counts a surface that is not periodic in U as unchecked, also when its basis is periodic)
        if !self.is_u_periodic() {
            return Err(GeomError::NoSuchObject);
        }
        self.basis_surf.u_period()
    }
    fn is_v_periodic(&self) -> bool {
        if !self.basis_surf.is_v_periodic() {
            return false;
        }
        if !self.isvtrimmed {
            return true;
        }
        let Ok(period) = self.basis_surf.v_period() else { return false };
        let length = self.vtrim2 - self.vtrim1;
        length > pconfusion() && libm::remainder(length, period).abs() <= pconfusion()
    }
    fn v_period(&self) -> Result<f64> {
        if !self.is_v_periodic() {
            return Err(GeomError::NoSuchObject);
        }
        self.basis_surf.v_period()
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        let c = self.basis_surf.u_iso(u)?;
        if self.isvtrimmed {
            Ok(Box::new(TrimmedCurve::new(Arc::from(c), self.vtrim1, self.vtrim2, true, true)?))
        } else {
            Ok(c)
        }
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        let c = self.basis_surf.v_iso(v)?;
        if self.isutrimmed {
            Ok(Box::new(TrimmedCurve::new(Arc::from(c), self.utrim1, self.utrim2, true, true)?))
        } else {
            Ok(c)
        }
    }
    fn continuity(&self) -> GeomAbsShape {
        self.basis_surf.continuity()
    }
    fn is_cn_u(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_surf.is_cn_u(n)
    }
    fn is_cn_v(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_surf.is_cn_v(n)
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        self.basis_surf.eval_d0(u, v)
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        self.basis_surf.eval_d1(u, v)
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        self.basis_surf.eval_d2(u, v)
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        self.basis_surf.eval_d3(u, v)
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        self.basis_surf.eval_dn(u, v, nu, nv)
    }
}

impl BoundedSurface for RectangularTrimmedSurface {}
