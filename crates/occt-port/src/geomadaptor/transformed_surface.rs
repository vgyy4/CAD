//! `GeomAdaptor_TransformedSurface`: a `GeomAdaptor_Surface` followed by a `gp_Trsf`.
//!
//! Translated from `GeomAdaptor_TransformedSurface.hxx/.cxx` (OCCT 8.0.1). The `std::optional` transformed adaptor is a
//! `Mutex<Option<Arc<Surface>>>` (a shallow copy shares it).
use super::Result;
use super::surface::Surface;
use crate::adaptor2d::GeomAbsShape;
use crate::adaptor3d::{Curve3dAdaptor, GeomAbsSurfaceType, SurfaceAdaptor};
use crate::geom::{
    self, ConicalSurface, CylindricalSurface, Plane, SphericalSurface, SurfResD1, SurfResD2, SurfResD3,
    Surface as GeomSurface, ToroidalSurface,
};
use crate::gp::{Ax1, Cone, Cylinder, Dir, Pln, Pnt, Sphere, Torus, Trsf, TrsfForm, Vec};
use std::sync::{Arc, Mutex};

/// `GeomAdaptor_TransformedSurface`.
#[derive(Debug)]
pub struct TransformedSurface {
    surf: Surface,
    trsf: Trsf,
    transformed: Mutex<Option<Arc<Surface>>>,
}

impl Default for TransformedSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformedSurface {
    /// Empty adaptor.
    pub fn new() -> Self {
        Self { surf: Surface::new(), trsf: Trsf::new(), transformed: Mutex::new(None) }
    }
    /// Constructor from a surface and a transformation.
    pub fn with_surface(s: Arc<dyn GeomSurface>, trsf: Trsf) -> Result<Self> {
        Ok(Self { surf: Surface::with_surface(s)?, trsf, transformed: Mutex::new(None) })
    }
    /// Constructor from a surface, a range, a transformation and tolerances.
    #[allow(clippy::too_many_arguments)]
    pub fn with_range(
        s: Arc<dyn GeomSurface>,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        trsf: Trsf,
        tol_u: f64,
        tol_v: f64,
    ) -> Result<Self> {
        Ok(Self { surf: Surface::with_range(s, u1, u2, v1, v2, tol_u, tol_v)?, trsf, transformed: Mutex::new(None) })
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Option<Arc<Surface>>> {
        self.transformed.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn invalidate(&mut self) {
        *self.cache() = None;
    }

    /// The concrete-type `ShallowCopy`.
    pub fn shallow_copied(&self) -> Result<TransformedSurface> {
        let surf = self.surf.shallow_copied()?;
        let cache = self.cache().clone();
        Ok(TransformedSurface { surf, trsf: self.trsf, transformed: Mutex::new(cache) })
    }
    /// `Load(S, T)`.
    pub fn load(&mut self, s: Arc<dyn GeomSurface>, trsf: Trsf) -> Result<()> {
        self.surf.load(s)?;
        self.trsf = trsf;
        self.invalidate();
        Ok(())
    }
    /// `Load(S, U1, U2, V1, V2, T, TolU, TolV)`.
    #[allow(clippy::too_many_arguments)]
    pub fn load_range(
        &mut self,
        s: Arc<dyn GeomSurface>,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        trsf: Trsf,
        tol_u: f64,
        tol_v: f64,
    ) -> Result<()> {
        self.surf.load_range(s, u1, u2, v1, v2, tol_u, tol_v)?;
        self.trsf = trsf;
        self.invalidate();
        Ok(())
    }
    /// `SetTrsf`.
    pub fn set_trsf(&mut self, trsf: Trsf) {
        self.trsf = trsf;
        self.invalidate();
    }
    /// `HasTrsf`.
    pub fn has_trsf(&self) -> bool {
        self.trsf.form() != TrsfForm::Identity
    }
    /// `Trsf`.
    pub fn trsf(&self) -> &Trsf {
        &self.trsf
    }
    /// `ToleranceU`.
    pub fn tolerance_u(&self) -> f64 {
        self.surf.tolerance_u()
    }
    /// `ToleranceV`.
    pub fn tolerance_v(&self) -> f64 {
        self.surf.tolerance_v()
    }
    /// `AdaptorSurfaceOriginal`.
    pub fn adaptor_surface_original(&self) -> &Surface {
        &self.surf
    }
    /// `GeomSurfaceOriginal`.
    pub fn geom_surface_original(&self) -> Option<&Arc<dyn GeomSurface>> {
        self.surf.surface()
    }

    fn init_cache(&self) -> Result<Arc<Surface>> {
        if self.trsf.form() == TrsfForm::Identity {
            return Ok(Arc::new(self.surf.shallow_copied()?));
        }
        let Some(orig) = self.surf.surface() else {
            return Ok(Arc::new(Surface::new()));
        };
        let t = &self.trsf;
        let identity = t.form() == TrsfForm::Identity;
        let s: Arc<dyn GeomSurface> = if identity {
            orig.clone()
        } else {
            match self.surf.get_type()? {
                GeomAbsSurfaceType::Plane => Arc::new(Plane::from_pln(self.surf.plane()?.transformed(t))),
                GeomAbsSurfaceType::Cylinder => {
                    Arc::new(CylindricalSurface::from_cylinder(self.surf.cylinder()?.transformed(t)))
                }
                GeomAbsSurfaceType::Cone => Arc::new(ConicalSurface::from_cone(self.surf.cone()?.transformed(t))),
                GeomAbsSurfaceType::Sphere => {
                    Arc::new(SphericalSurface::from_sphere(self.surf.sphere()?.transformed(t)))
                }
                GeomAbsSurfaceType::Torus => Arc::new(ToroidalSurface::from_torus(self.surf.torus()?.transformed(t))),
                _ => {
                    let mut c = orig.copy_surface();
                    c.transform(t);
                    Arc::from(c)
                }
            }
        };
        Ok(Arc::new(Surface::with_range(
            s,
            self.surf.first_u_parameter()?,
            self.surf.last_u_parameter()?,
            self.surf.first_v_parameter()?,
            self.surf.last_v_parameter()?,
            self.surf.tolerance_u(),
            self.surf.tolerance_v(),
        )?))
    }

    /// `AdaptorSurfaceTransformed` (the original adaptor itself for the identity).
    pub fn adaptor_surface_transformed(&self) -> Result<Arc<Surface>> {
        let mut c = self.cache();
        if let Some(a) = c.as_ref() {
            return Ok(a.clone());
        }
        let a = self.init_cache()?;
        *c = Some(a.clone());
        Ok(a)
    }

    /// `GeomSurfaceTransformed`.
    pub fn geom_surface_transformed(&self) -> Result<Option<Arc<dyn GeomSurface>>> {
        if self.trsf.form() == TrsfForm::Identity {
            return Ok(self.surf.surface().cloned());
        }
        Ok(self.adaptor_surface_transformed()?.surface().cloned())
    }

    /// `transformedAdaptor()`: the original adaptor for the identity, else the cached transformed one.
    fn with<R>(&self, f: impl FnOnce(&dyn SurfaceAdaptor) -> Result<R>) -> Result<R> {
        if self.trsf.form() == TrsfForm::Identity {
            f(&self.surf)
        } else {
            f(self.adaptor_surface_transformed()?.as_ref())
        }
    }
}

impl SurfaceAdaptor for TransformedSurface {
    fn shallow_copy(&self) -> Result<Arc<dyn SurfaceAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }
    fn first_u_parameter(&self) -> Result<f64> {
        self.surf.first_u_parameter()
    }
    fn last_u_parameter(&self) -> Result<f64> {
        self.surf.last_u_parameter()
    }
    fn first_v_parameter(&self) -> Result<f64> {
        self.surf.first_v_parameter()
    }
    fn last_v_parameter(&self) -> Result<f64> {
        self.surf.last_v_parameter()
    }
    fn u_continuity(&self) -> Result<GeomAbsShape> {
        self.surf.u_continuity()
    }
    fn v_continuity(&self) -> Result<GeomAbsShape> {
        self.surf.v_continuity()
    }
    fn nb_u_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        self.surf.nb_u_intervals(s)
    }
    fn nb_v_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        self.surf.nb_v_intervals(s)
    }
    fn u_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        self.surf.u_intervals(t, s)
    }
    fn v_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        self.surf.v_intervals(t, s)
    }
    fn u_trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        self.with(|a| a.u_trim(first, last, tol))
    }
    fn v_trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        self.with(|a| a.v_trim(first, last, tol))
    }
    fn is_u_closed(&self) -> Result<bool> {
        self.surf.is_u_closed()
    }
    fn is_v_closed(&self) -> Result<bool> {
        self.surf.is_v_closed()
    }
    fn is_u_periodic(&self) -> Result<bool> {
        self.surf.is_u_periodic()
    }
    fn u_period(&self) -> Result<f64> {
        self.surf.u_period()
    }
    fn is_v_periodic(&self) -> Result<bool> {
        self.surf.is_v_periodic()
    }
    fn v_period(&self) -> Result<f64> {
        self.surf.v_period()
    }
    fn u_resolution(&self, r3d: f64) -> Result<f64> {
        self.surf.u_resolution(r3d)
    }
    fn v_resolution(&self, r3d: f64) -> Result<f64> {
        self.surf.v_resolution(r3d)
    }
    fn get_type(&self) -> Result<GeomAbsSurfaceType> {
        self.surf.get_type()
    }
    fn u_degree(&self) -> Result<i32> {
        self.surf.u_degree()
    }
    fn nb_u_poles(&self) -> Result<i32> {
        self.surf.nb_u_poles()
    }
    fn v_degree(&self) -> Result<i32> {
        self.surf.v_degree()
    }
    fn nb_v_poles(&self) -> Result<i32> {
        self.surf.nb_v_poles()
    }
    fn nb_u_knots(&self) -> Result<i32> {
        self.surf.nb_u_knots()
    }
    fn nb_v_knots(&self) -> Result<i32> {
        self.surf.nb_v_knots()
    }
    fn is_u_rational(&self) -> Result<bool> {
        self.surf.is_u_rational()
    }
    fn is_v_rational(&self) -> Result<bool> {
        self.surf.is_v_rational()
    }
    fn plane(&self) -> Result<Pln> {
        self.with(|a| a.plane())
    }
    fn cylinder(&self) -> Result<Cylinder> {
        self.with(|a| a.cylinder())
    }
    fn cone(&self) -> Result<Cone> {
        self.with(|a| a.cone())
    }
    fn sphere(&self) -> Result<Sphere> {
        self.with(|a| a.sphere())
    }
    fn torus(&self) -> Result<Torus> {
        self.with(|a| a.torus())
    }
    fn bezier(&self) -> Result<Option<Arc<geom::BezierSurface>>> {
        self.with(|a| a.bezier())
    }
    fn bspline(&self) -> Result<Option<Arc<geom::BSplineSurface>>> {
        self.with(|a| a.bspline())
    }
    fn axe_of_revolution(&self) -> Result<Ax1> {
        self.with(|a| a.axe_of_revolution())
    }
    fn direction(&self) -> Result<Dir> {
        self.with(|a| a.direction())
    }
    fn basis_curve(&self) -> Result<Option<Arc<dyn Curve3dAdaptor>>> {
        self.with(|a| a.basis_curve())
    }
    fn basis_surface(&self) -> Result<Option<Arc<dyn SurfaceAdaptor>>> {
        self.with(|a| a.basis_surface())
    }
    fn offset_value(&self) -> Result<f64> {
        self.with(|a| a.offset_value())
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        let p = self.surf.eval_d0(u, v)?;
        if self.trsf.form() == TrsfForm::Identity {
            return Ok(p);
        }
        Ok(p.transformed(&self.trsf))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        let mut r = self.surf.eval_d1(u, v)?;
        if self.trsf.form() == TrsfForm::Identity {
            return Ok(r);
        }
        let t = &self.trsf;
        r.point.transform(t);
        r.d1u = r.d1u.transformed(t);
        r.d1v = r.d1v.transformed(t);
        Ok(r)
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        let mut r = self.surf.eval_d2(u, v)?;
        if self.trsf.form() == TrsfForm::Identity {
            return Ok(r);
        }
        let t = &self.trsf;
        r.point.transform(t);
        r.d1u = r.d1u.transformed(t);
        r.d1v = r.d1v.transformed(t);
        r.d2u = r.d2u.transformed(t);
        r.d2v = r.d2v.transformed(t);
        r.d2uv = r.d2uv.transformed(t);
        Ok(r)
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        let mut r = self.surf.eval_d3(u, v)?;
        if self.trsf.form() == TrsfForm::Identity {
            return Ok(r);
        }
        let t = &self.trsf;
        r.point.transform(t);
        r.d1u = r.d1u.transformed(t);
        r.d1v = r.d1v.transformed(t);
        r.d2u = r.d2u.transformed(t);
        r.d2v = r.d2v.transformed(t);
        r.d2uv = r.d2uv.transformed(t);
        r.d3u = r.d3u.transformed(t);
        r.d3v = r.d3v.transformed(t);
        r.d3uuv = r.d3uuv.transformed(t);
        r.d3uvv = r.d3uvv.transformed(t);
        Ok(r)
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<Vec> {
        let r = self.surf.eval_dn(u, v, nu, nv)?;
        if self.trsf.form() == TrsfForm::Identity {
            return Ok(r);
        }
        Ok(r.transformed(&self.trsf))
    }
}
