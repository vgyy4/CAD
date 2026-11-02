//! `GeomAdaptor_SurfaceOfLinearExtrusion`: the surface swept by an `Adaptor3d_Curve` along a direction.
//!
//! Translated from `GeomAdaptor_SurfaceOfLinearExtrusion.hxx/.cxx` (OCCT 8.0.1); see `revolution.rs` for the modelling of the
//! `GeomAdaptor_Surface` base class.
use super::Result;
use super::surface::{ExtrusionData, Surface, SurfaceData};
use crate::adaptor2d::{AdaptorError, GeomAbsCurveType, GeomAbsShape};
use crate::adaptor3d::{Curve3dAdaptor, GeomAbsSurfaceType, SurfaceAdaptor};
use crate::geom::{self, SurfResD1, SurfResD2, SurfResD3, angular};
use crate::gp::{Ax1, Ax3, Cone, Cylinder, Dir, Pln, Pnt, Sphere, Torus, Vec};
use std::sync::Arc;

const REAL_LAST: f64 = f64::MAX;
const REAL_FIRST: f64 = -f64::MAX;
const INFINITE: f64 = 2.0e100;

/// `GeomAdaptor_SurfaceOfLinearExtrusion`.
#[derive(Debug)]
pub struct SurfaceOfLinearExtrusion {
    base: Surface,
    basis_curve: Option<Arc<dyn Curve3dAdaptor>>,
    direction: Dir,
    have_dir: bool,
}

impl Default for SurfaceOfLinearExtrusion {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceOfLinearExtrusion {
    /// Empty adaptor.
    pub fn new() -> Self {
        Self { base: Surface::new(), basis_curve: None, direction: Dir::default(), have_dir: false }
    }
    /// Constructor from the basis curve.
    pub fn with_curve(c: Arc<dyn Curve3dAdaptor>) -> Result<Self> {
        let mut s = Self::new();
        s.load_curve(c)?;
        Ok(s)
    }
    /// Constructor from the basis curve and the direction.
    pub fn with_direction(c: Arc<dyn Curve3dAdaptor>, v: Dir) -> Result<Self> {
        let mut s = Self::new();
        s.load_curve(c)?;
        s.load_direction(v)?;
        Ok(s)
    }
    /// `Load(curve)`.
    pub fn load_curve(&mut self, c: Arc<dyn Curve3dAdaptor>) -> Result<()> {
        self.basis_curve = Some(c);
        if self.have_dir {
            self.load_direction(self.direction)?;
        }
        Ok(())
    }
    /// `Load(direction)`.
    pub fn load_direction(&mut self, v: Dir) -> Result<()> {
        self.have_dir = true;
        self.direction = v;
        self.base.surface_type = GeomAbsSurfaceType::SurfaceOfExtrusion;
        if let Some(b) = &self.basis_curve {
            self.base.data =
                SurfaceData::Extrusion(ExtrusionData { basis: b.clone(), direction: *v.xyz(), has_eval_rep: false });
        }
        Ok(())
    }

    fn basis(&self) -> Result<&Arc<dyn Curve3dAdaptor>> {
        self.basis_curve.as_ref().ok_or(AdaptorError::NullObject)
    }

    /// The concrete-type `ShallowCopy`.
    pub fn shallow_copied(&self) -> Result<SurfaceOfLinearExtrusion> {
        let basis_curve = match &self.basis_curve {
            None => None,
            Some(b) => Some(b.shallow_copy()?),
        };
        let mut base = Surface::new();
        base.surface = self.base.surface.clone();
        base.u_first = self.base.u_first;
        base.u_last = self.base.u_last;
        base.v_first = self.base.v_first;
        base.v_last = self.base.v_last;
        base.tol_u = self.base.tol_u;
        base.tol_v = self.base.tol_v;
        base.surface_type = self.base.surface_type;
        if let SurfaceData::Extrusion(d) = &self.base.data {
            base.data = SurfaceData::Extrusion(ExtrusionData {
                basis: d.basis.shallow_copy()?,
                direction: d.direction,
                has_eval_rep: d.has_eval_rep,
            });
        }
        Ok(SurfaceOfLinearExtrusion { base, basis_curve, direction: self.direction, have_dir: self.have_dir })
    }
}

impl SurfaceAdaptor for SurfaceOfLinearExtrusion {
    fn shallow_copy(&self) -> Result<Arc<dyn SurfaceAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }
    fn first_u_parameter(&self) -> Result<f64> {
        self.basis()?.first_parameter()
    }
    fn last_u_parameter(&self) -> Result<f64> {
        self.basis()?.last_parameter()
    }
    fn first_v_parameter(&self) -> Result<f64> {
        Ok(REAL_FIRST)
    }
    fn last_v_parameter(&self) -> Result<f64> {
        Ok(REAL_LAST)
    }
    fn u_continuity(&self) -> Result<GeomAbsShape> {
        self.basis()?.continuity()
    }
    fn v_continuity(&self) -> Result<GeomAbsShape> {
        Ok(GeomAbsShape::Cn)
    }
    fn nb_u_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        self.basis()?.nb_intervals(s)
    }
    fn nb_v_intervals(&self, _s: GeomAbsShape) -> Result<i32> {
        Ok(1)
    }
    fn u_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        self.basis()?.intervals(t, s)
    }
    fn v_intervals(&self, t: &mut [f64], _s: GeomAbsShape) -> Result<()> {
        *t.get_mut(0).ok_or(AdaptorError::OutOfRange)? = REAL_FIRST;
        *t.get_mut(1).ok_or(AdaptorError::OutOfRange)? = REAL_LAST;
        Ok(())
    }
    fn u_trim(&self, _first: f64, _last: f64, _tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        let b = self.basis()?.clone();
        Ok(Arc::new(SurfaceOfLinearExtrusion::with_direction(b, self.direction)?))
    }
    fn v_trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        let hc = self.basis()?.trim(first, last, tol)?;
        Ok(Arc::new(SurfaceOfLinearExtrusion::with_direction(hc, self.direction)?))
    }
    fn is_u_closed(&self) -> Result<bool> {
        self.basis()?.is_closed()
    }
    fn is_v_closed(&self) -> Result<bool> {
        Ok(false)
    }
    fn is_u_periodic(&self) -> Result<bool> {
        self.basis()?.is_periodic()
    }
    fn u_period(&self) -> Result<f64> {
        self.basis()?.period()
    }
    fn is_v_periodic(&self) -> Result<bool> {
        Ok(false)
    }
    fn v_period(&self) -> Result<f64> {
        Err(AdaptorError::Domain)
    }
    fn u_resolution(&self, r3d: f64) -> Result<f64> {
        self.basis()?.resolution(r3d)
    }
    fn v_resolution(&self, r3d: f64) -> Result<f64> {
        Ok(r3d)
    }
    fn get_type(&self) -> Result<GeomAbsSurfaceType> {
        let tol = angular();
        let basis = self.basis()?;
        match basis.get_type()? {
            GeomAbsCurveType::Line => {
                let d = *basis.line()?.direction();
                if !self.direction.is_parallel(&d, tol) {
                    return Ok(GeomAbsSurfaceType::Plane);
                }
            }
            GeomAbsCurveType::Circle => {
                let d = *basis.circle()?.axis().direction();
                if self.direction.is_parallel(&d, tol) {
                    return Ok(GeomAbsSurfaceType::Cylinder);
                } else if self.direction.is_normal(&d, tol) {
                    return Ok(GeomAbsSurfaceType::Plane);
                }
            }
            GeomAbsCurveType::Ellipse => {
                let d = *basis.ellipse()?.axis().direction();
                if self.direction.is_normal(&d, tol) {
                    return Ok(GeomAbsSurfaceType::Plane);
                }
            }
            GeomAbsCurveType::Parabola => {
                let d = *basis.parabola()?.axis().direction();
                if self.direction.is_normal(&d, tol) {
                    return Ok(GeomAbsSurfaceType::Plane);
                }
            }
            GeomAbsCurveType::Hyperbola => {
                let d = *basis.hyperbola()?.axis().direction();
                if self.direction.is_normal(&d, tol) {
                    return Ok(GeomAbsSurfaceType::Plane);
                }
            }
            _ => {}
        }
        Ok(GeomAbsSurfaceType::SurfaceOfExtrusion)
    }
    fn plane(&self) -> Result<Pln> {
        let basis = self.basis()?;
        let mut p = Pnt::default();
        let mut d1u = Vec::zero();
        let mut new_z = Vec::zero();
        let mut ufirst = basis.first_parameter()?;
        let mut ulast = basis.last_parameter()?;
        if ufirst <= -INFINITE && ulast >= INFINITE {
            ufirst = -100.0;
            ulast = 100.0;
        } else if ufirst <= -INFINITE {
            ufirst = ulast - 200.0;
        } else if ulast >= INFINITE {
            ulast = ufirst + 200.0;
        }
        let deltau = (ulast - ufirst) / 20.0;
        let dir = Vec::from_dir(&self.direction);
        for i in 1..=21 {
            let prm = ufirst + f64::from(i - 1) * deltau;
            let (pp, dd) = basis.d1(prm)?;
            p = pp;
            d1u = dd;
            new_z = d1u.normalized()?.crossed(&dir);
            if new_z.magnitude() > 1.0e-12 {
                break;
            }
        }
        let mut ax3 = Ax3::new_unchecked(p, Dir::from_vec_unchecked(&new_z), Dir::from_vec_unchecked(&d1u));
        if self.direction.dot(ax3.y_direction()) < 0.0 {
            ax3.y_reverse();
        }
        Ok(Pln::new(ax3))
    }
    fn cylinder(&self) -> Result<Cylinder> {
        let c = self.basis()?.circle()?;
        let mut ax3 = Ax3::from_ax2(c.position());
        if self.direction.dot(c.axis().direction()) < 0.0 {
            ax3.z_reverse();
        }
        Ok(Cylinder::new_unchecked(ax3, c.radius()))
    }
    fn cone(&self) -> Result<Cone> {
        Err(AdaptorError::NoSuchObject)
    }
    fn sphere(&self) -> Result<Sphere> {
        Err(AdaptorError::NoSuchObject)
    }
    fn torus(&self) -> Result<Torus> {
        Err(AdaptorError::NoSuchObject)
    }
    fn axe_of_revolution(&self) -> Result<Ax1> {
        Err(AdaptorError::NoSuchObject)
    }
    fn u_degree(&self) -> Result<i32> {
        self.basis()?.degree()
    }
    fn nb_u_poles(&self) -> Result<i32> {
        self.basis()?.nb_poles()
    }
    fn is_u_rational(&self) -> Result<bool> {
        Err(AdaptorError::NoSuchObject)
    }
    fn is_v_rational(&self) -> Result<bool> {
        Err(AdaptorError::NoSuchObject)
    }
    fn bezier(&self) -> Result<Option<Arc<geom::BezierSurface>>> {
        Err(AdaptorError::NoSuchObject)
    }
    fn bspline(&self) -> Result<Option<Arc<geom::BSplineSurface>>> {
        Err(AdaptorError::NoSuchObject)
    }
    fn direction(&self) -> Result<Dir> {
        Ok(self.direction)
    }
    fn basis_curve(&self) -> Result<Option<Arc<dyn Curve3dAdaptor>>> {
        Ok(self.basis_curve.clone())
    }
    // inherited from GeomAdaptor_Surface
    fn v_degree(&self) -> Result<i32> {
        self.base.v_degree()
    }
    fn nb_v_poles(&self) -> Result<i32> {
        self.base.nb_v_poles()
    }
    fn nb_u_knots(&self) -> Result<i32> {
        self.base.nb_u_knots()
    }
    fn nb_v_knots(&self) -> Result<i32> {
        self.base.nb_v_knots()
    }
    fn basis_surface(&self) -> Result<Option<Arc<dyn SurfaceAdaptor>>> {
        self.base.basis_surface()
    }
    fn offset_value(&self) -> Result<f64> {
        self.base.offset_value()
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        self.base.eval_d0(u, v)
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        self.base.eval_d1(u, v)
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        self.base.eval_d2(u, v)
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        self.base.eval_d3(u, v)
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<Vec> {
        self.base.eval_dn(u, v, nu, nv)
    }
}
