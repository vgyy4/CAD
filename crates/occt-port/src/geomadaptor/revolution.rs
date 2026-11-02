//! `GeomAdaptor_SurfaceOfRevolution`: the surface of revolution of an `Adaptor3d_Curve` about an axis.
//!
//! Translated from `GeomAdaptor_SurfaceOfRevolution.hxx/.cxx` (OCCT 8.0.1). In C++ the class derives from
//! `GeomAdaptor_Surface` and uses its `EvalD*` through the `RevolutionData` variant; here it holds a base [`Surface`]
//! whose type and data are set by `load_axis`, and forwards the inherited methods to it.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use super::Result;
use super::surface::{RevolutionData, Surface, SurfaceData};
use crate::adaptor2d::{AdaptorError, GeomAbsShape};
use crate::adaptor3d::{Curve3dAdaptor, GeomAbsSurfaceType, SurfaceAdaptor};
use crate::elclib;
use crate::geom::{self, SurfResD1, SurfResD2, SurfResD3, angular, confusion, is_infinite};
use crate::gp::{Ax1, Ax3, Cone, Cylinder, Dir, Lin, Pln, Pnt, Sphere, Torus, Vec};
use std::sync::Arc;

const TWO_PI: f64 = 2.0 * std::f64::consts::PI;

/// `GeomAdaptor_SurfaceOfRevolution`.
#[derive(Debug)]
pub struct SurfaceOfRevolution {
    base: Surface,
    basis_curve: Option<Arc<dyn Curve3dAdaptor>>,
    axis: Ax1,
    have_axis: bool,
    axe_rev: Ax3,
}

impl Default for SurfaceOfRevolution {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceOfRevolution {
    /// Empty adaptor.
    pub fn new() -> Self {
        Self {
            base: Surface::new(),
            basis_curve: None,
            axis: Ax1::default(),
            have_axis: false,
            axe_rev: Ax3::default(),
        }
    }
    /// Constructor from the meridian curve.
    pub fn with_curve(c: Arc<dyn Curve3dAdaptor>) -> Result<Self> {
        let mut s = Self::new();
        s.load_curve(c)?;
        Ok(s)
    }
    /// Constructor from the meridian curve and the axis.
    pub fn with_axis(c: Arc<dyn Curve3dAdaptor>, axis: Ax1) -> Result<Self> {
        let mut s = Self::new();
        s.load_curve(c)?;
        s.load_axis(axis)?;
        Ok(s)
    }

    /// `Load(curve)`.
    pub fn load_curve(&mut self, c: Arc<dyn Curve3dAdaptor>) -> Result<()> {
        self.basis_curve = Some(c);
        if self.have_axis {
            self.load_axis(self.axis)?;
        }
        Ok(())
    }

    fn basis(&self) -> Result<&Arc<dyn Curve3dAdaptor>> {
        self.basis_curve.as_ref().ok_or(AdaptorError::NullObject)
    }

    /// `Axis`.
    pub fn axis(&self) -> &Ax3 {
        &self.axe_rev
    }

    /// `Load(axis)`: evaluates `myAxeRev`.
    pub fn load_axis(&mut self, v: Ax1) -> Result<()> {
        self.have_axis = true;
        self.axis = v;
        self.base.surface_type = GeomAbsSurfaceType::SurfaceOfRevolution;
        let basis = self.basis()?.clone();
        self.base.data =
            SurfaceData::Revolution(RevolutionData { basis: basis.clone(), axis: self.axis, has_eval_rep: false });

        let tol = confusion();
        let mut o = *self.axis.location();
        let mut oz = *self.axis.direction();
        let mut yrev = false;
        if basis.get_type()? == crate::adaptor2d::GeomAbsCurveType::Line && basis.line()?.direction().dot(&oz) < 0.0 {
            yrev = true;
            oz.reverse();
        }
        let p: Pnt;
        let q: Pnt;
        if basis.get_type()? == crate::adaptor2d::GeomAbsCurveType::Circle {
            let c = *basis.circle()?.location();
            p = c;
            q = c;
        } else {
            let first = basis.first_parameter()?;
            p = self.base.eval_d0(0.0, 0.0)?;
            if self.get_type()? == GeomAbsSurfaceType::Cone {
                if Lin::from_axis(self.axis).distance(&p) <= tol {
                    q = elclib::value_lin(1.0, &basis.line()?);
                } else {
                    q = p;
                }
            } else if is_infinite(first) {
                q = p;
            } else {
                q = self.base.eval_d0(0.0, first)?;
            }
        }
        let dz = *self.axis.direction();
        let d = Vec::between(&o, &p).dot(&Vec::from_dir(&dz));
        o.set_xyz(o.xyz().added(&dz.xyz().multiplied(d)));
        let ox: Dir;
        if Lin::from_axis(self.axis).distance(&q) > tol {
            ox = Dir::from_xyz(q.xyz().subtracted(o.xyz()))?;
        } else {
            let first = basis.first_parameter()?;
            let last = basis.last_parameter()?;
            let mut ratio = 1;
            let mut pp;
            loop {
                pp = basis.value(first + (last - first) / f64::from(ratio))?;
                let dist = Lin::from_axis(self.axis).distance(&pp);
                ratio += 1;
                if !(dist < tol && ratio < 100) {
                    break;
                }
            }
            if ratio >= 100 {
                return Err(AdaptorError::Construction);
            }
            let w = Dir::from_xyz(pp.xyz().subtracted(o.xyz()))?;
            ox = oz.crossed(&w)?.crossed(&oz)?;
        }
        self.axe_rev = Ax3::new(o, oz, ox)?;
        if yrev {
            self.axe_rev.y_reverse();
        } else if basis.get_type()? == crate::adaptor2d::GeomAbsCurveType::Circle {
            let dc = *basis.circle()?.axis().direction();
            if ox.crossed(&oz)?.dot(&dc) < 0.0 {
                self.axe_rev.z_reverse();
            }
        }
        Ok(())
    }

    fn radius_at_origin(&self) -> Result<f64> {
        let p = self.base.eval_d0(0.0, 0.0)?;
        Ok(Vec::between(self.axe_rev.location(), &p).dot(&Vec::from_dir(self.axe_rev.x_direction())))
    }

    /// The concrete-type `ShallowCopy`.
    pub fn shallow_copied(&self) -> Result<SurfaceOfRevolution> {
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
        if let SurfaceData::Revolution(d) = &self.base.data {
            base.data = SurfaceData::Revolution(RevolutionData {
                basis: d.basis.shallow_copy()?,
                axis: d.axis,
                has_eval_rep: d.has_eval_rep,
            });
        }
        Ok(SurfaceOfRevolution { base, basis_curve, axis: self.axis, have_axis: self.have_axis, axe_rev: self.axe_rev })
    }
}

impl SurfaceAdaptor for SurfaceOfRevolution {
    fn shallow_copy(&self) -> Result<Arc<dyn SurfaceAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }
    fn axe_of_revolution(&self) -> Result<Ax1> {
        Ok(self.axis)
    }
    fn first_u_parameter(&self) -> Result<f64> {
        Ok(0.0)
    }
    fn last_u_parameter(&self) -> Result<f64> {
        Ok(TWO_PI)
    }
    fn first_v_parameter(&self) -> Result<f64> {
        self.basis()?.first_parameter()
    }
    fn last_v_parameter(&self) -> Result<f64> {
        self.basis()?.last_parameter()
    }
    fn u_continuity(&self) -> Result<GeomAbsShape> {
        Ok(GeomAbsShape::Cn)
    }
    fn v_continuity(&self) -> Result<GeomAbsShape> {
        self.basis()?.continuity()
    }
    fn nb_u_intervals(&self, _s: GeomAbsShape) -> Result<i32> {
        Ok(1)
    }
    fn nb_v_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        self.basis()?.nb_intervals(s)
    }
    fn u_intervals(&self, t: &mut [f64], _s: GeomAbsShape) -> Result<()> {
        *t.get_mut(0).ok_or(AdaptorError::OutOfRange)? = 0.0;
        *t.get_mut(1).ok_or(AdaptorError::OutOfRange)? = TWO_PI;
        Ok(())
    }
    fn v_intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        self.basis()?.intervals(t, s)
    }
    fn u_trim(&self, _first: f64, _last: f64, _tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        let b = self.basis()?.clone();
        Ok(Arc::new(SurfaceOfRevolution::with_axis(b, self.axis)?))
    }
    fn v_trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        let hc = self.basis()?.trim(first, last, tol)?;
        Ok(Arc::new(SurfaceOfRevolution::with_axis(hc, self.axis)?))
    }
    fn is_u_closed(&self) -> Result<bool> {
        Ok(true)
    }
    fn is_v_closed(&self) -> Result<bool> {
        self.basis()?.is_closed()
    }
    fn is_u_periodic(&self) -> Result<bool> {
        Ok(true)
    }
    fn u_period(&self) -> Result<f64> {
        Ok(TWO_PI)
    }
    fn is_v_periodic(&self) -> Result<bool> {
        self.basis()?.is_periodic()
    }
    fn v_period(&self) -> Result<f64> {
        self.basis()?.period()
    }
    fn u_resolution(&self, r3d: f64) -> Result<f64> {
        Ok(r3d * 0.01)
    }
    fn v_resolution(&self, r3d: f64) -> Result<f64> {
        self.basis()?.resolution(r3d)
    }
    fn get_type(&self) -> Result<GeomAbsSurfaceType> {
        use crate::adaptor2d::GeomAbsCurveType as C;
        let tol_conf = confusion();
        let tol_ang = angular();
        let tol_cone_semi_ang = confusion();
        let basis = self.basis()?;
        match basis.get_type()? {
            C::Line => {
                let axe = *basis.line()?.position();
                if self.axis.is_parallel(&axe, tol_ang) {
                    let r = self.radius_at_origin()?;
                    if r > tol_conf {
                        return Ok(GeomAbsSurfaceType::Cylinder);
                    }
                } else if self.axis.is_normal(&axe, tol_ang) {
                    return Ok(GeomAbsSurfaceType::Plane);
                } else {
                    let uf = basis.first_parameter()?;
                    let ul = basis.last_parameter()?;
                    let istrim = !is_infinite(uf) && !is_infinite(ul);
                    if istrim {
                        let pf = basis.value(uf)?;
                        let pl = basis.value(ul)?;
                        let len = pf.distance(&pl);
                        let vlin = Vec::between(&pf, &pl);
                        let vaxe = Vec::from_dir(self.axis.direction());
                        let projlen = vaxe.dot(&vlin).abs();
                        if (len - projlen) <= tol_conf {
                            let r = self.radius_at_origin()?;
                            if r > tol_conf {
                                return Ok(GeomAbsSurfaceType::Cylinder);
                            }
                        } else if projlen <= tol_conf {
                            return Ok(GeomAbsSurfaceType::Plane);
                        }
                    }
                    let v = Vec::between(self.axis.location(), basis.line()?.location());
                    let w = Vec::from_dir(axe.direction());
                    let axis_dir = Vec::from_dir(self.axis.direction());
                    let proj = w.dot(&axis_dir).abs();
                    if v.dot_cross(&axis_dir, &w).abs() <= tol_conf
                        && (proj >= tol_cone_semi_ang && proj <= 1.0 - tol_cone_semi_ang)
                    {
                        return Ok(GeomAbsSurfaceType::Cone);
                    }
                }
            }
            C::Circle => {
                let a_lin = Lin::from_axis(self.axis);
                let c = basis.circle()?;
                let a_lc = *c.location();
                let a_r = c.radius();
                if !c.position().is_coplanar(&self.axis, tol_conf, tol_ang) {
                    return Ok(GeomAbsSurfaceType::SurfaceOfRevolution);
                } else if a_lin.distance(&a_lc) <= tol_conf {
                    return Ok(GeomAbsSurfaceType::Sphere);
                } else {
                    let major = a_lin.distance(&a_lc);
                    if major > a_r {
                        return Ok(GeomAbsSurfaceType::Torus);
                    }
                }
            }
            _ => {}
        }
        Ok(GeomAbsSurfaceType::SurfaceOfRevolution)
    }
    fn plane(&self) -> Result<Pln> {
        let mut axe = self.axe_rev;
        let on_curve = self.base.eval_d0(0.0, 0.0)?;
        let a_dot = on_curve.xyz().subtracted(self.axis.location().xyz()).dot(self.axis.direction().xyz());
        let p = Pnt::from_xyz(self.axis.location().xyz().added(&self.axis.direction().xyz().multiplied(a_dot)));
        axe.set_location(p);
        if axe.x_direction().dot(self.basis()?.line()?.direction()) >= -confusion() {
            axe.x_reverse();
        }
        Ok(Pln::new(axe))
    }
    fn cylinder(&self) -> Result<Cylinder> {
        let r = self.radius_at_origin()?;
        Ok(Cylinder::new_unchecked(self.axe_rev, r))
    }
    fn cone(&self) -> Result<Cone> {
        let axe = self.axe_rev;
        let ldir = *self.basis()?.line()?.direction();
        let mut angle = axe.direction().angle(&ldir);
        let p0 = self.base.eval_d0(0.0, 0.0)?;
        let r = axe.location().distance(&p0);
        if r >= confusion() {
            let o = *axe.location();
            let mut op0 = Vec::between(&o, &p0);
            let mut t = op0.dot(&Vec::from_dir(axe.x_direction()));
            t /= ldir.dot(axe.x_direction());
            op0.add(&Vec::from_dir(&ldir).multiplied(-t));
            if op0.dot(&Vec::from_dir(axe.direction())) > 0.0 {
                angle = -angle;
            }
        }
        Ok(Cone::new_unchecked(axe, angle, r))
    }
    fn sphere(&self) -> Result<Sphere> {
        let c = self.basis()?.circle()?;
        let mut axe = self.axe_rev;
        axe.set_location(*c.location());
        Ok(Sphere::new_unchecked(axe, c.radius()))
    }
    fn torus(&self) -> Result<Torus> {
        let c = self.basis()?.circle()?;
        let major = Lin::from_axis(self.axis).distance(c.location());
        Ok(Torus::new_unchecked(self.axe_rev, major, c.radius()))
    }
    fn v_degree(&self) -> Result<i32> {
        self.basis()?.degree()
    }
    fn nb_v_poles(&self) -> Result<i32> {
        self.basis()?.nb_poles()
    }
    fn nb_v_knots(&self) -> Result<i32> {
        Err(AdaptorError::NoSuchObject)
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
    fn basis_curve(&self) -> Result<Option<Arc<dyn Curve3dAdaptor>>> {
        Ok(self.basis_curve.clone())
    }
    // inherited from GeomAdaptor_Surface
    fn u_degree(&self) -> Result<i32> {
        self.base.u_degree()
    }
    fn nb_u_poles(&self) -> Result<i32> {
        self.base.nb_u_poles()
    }
    fn nb_u_knots(&self) -> Result<i32> {
        self.base.nb_u_knots()
    }
    fn direction(&self) -> Result<Dir> {
        self.base.direction()
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
