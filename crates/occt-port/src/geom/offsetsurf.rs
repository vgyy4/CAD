//! `Geom_OffsetSurface` with `Geom_OffsetSurfaceUtils`.
use super::osculating::OsculatingSurface;
use super::rep_utils::{self, SurfaceEvalRepDesc};
use super::rtrimmed::make_unique_surface;
use super::{BSplineCurve, BSplineSurface, BezierSurface, Circle, Curve, ElementarySurface, Ellipse};
use super::{ConicalSurface, CylindricalSurface, EvalRepKind, GeomAbsShape, GeomError, Geometry, Plane};
use super::{OffsetCurve, RectangularTrimmedSurface, Result, SphericalSurface, SurfResD1, SurfResD2, SurfResD3};
use super::{Surface, SurfaceOfLinearExtrusion, SurfaceOfRevolution, ToroidalSurface, TrimmedCurve};
use super::{angular, confusion, geometry_any, is_infinite, pconfusion};
use crate::cslib::{self, NormalStatus, VecArray2};
use crate::gp::{self, GTrsf2d, Pnt, Trsf};
use std::sync::Arc;

const D1_MAGNITUDE_TOL: f64 = 1.0e-9;

/// `Geom_OffsetSurface`.
#[derive(Debug, Clone)]
pub struct OffsetSurface {
    basis_surf: Arc<dyn Surface>,
    eval_rep: Option<SurfaceEvalRepDesc>,
    offset_value: f64,
    osc_surf: Option<OsculatingSurface>,
    basis_surf_continuity: GeomAbsShape,
}

// makeFullSurfaceRep
fn make_full_surface_rep(s: Option<Arc<dyn Surface>>) -> Option<SurfaceEvalRepDesc> {
    s.map(|rep| SurfaceEvalRepDesc {
        kind: EvalRepKind::Full,
        representation: Some(rep),
        max_deriv_order: 3,
        domain: None,
        scale_u: 1.0,
        offset_u: 0.0,
        scale_v: 1.0,
        offset_v: 0.0,
        swap_uv: false,
    })
}

impl OffsetSurface {
    /// `Geom_OffsetSurface(theSurf, theOffset, isNotCheckC0)`.
    pub fn new(s: Arc<dyn Surface>, offset: f64, not_check_c0: bool) -> Result<Self> {
        let mut o = OffsetSurface {
            basis_surf: s.clone(),
            eval_rep: None,
            offset_value: offset,
            osc_surf: None,
            basis_surf_continuity: GeomAbsShape::C0,
        };
        o.set_basis_surface(s, not_check_c0)?;
        Ok(o)
    }

    /// `HasEvalRepresentation`.
    pub fn has_eval_representation(&self) -> bool {
        self.eval_rep.is_some()
    }

    /// `EvalRepresentation`.
    pub fn eval_representation(&self) -> Option<&SurfaceEvalRepDesc> {
        self.eval_rep.as_ref()
    }

    /// `SetEvalRepresentation`.
    pub fn set_eval_representation(&mut self, d: Option<SurfaceEvalRepDesc>) -> Result<()> {
        if let Some(desc) = d.as_ref() {
            rep_utils::validate_surface_desc(desc, self)?;
        }
        self.eval_rep = d;
        Ok(())
    }

    /// `ClearEvalRepresentation`.
    pub fn clear_eval_representation(&mut self) {
        self.eval_rep = None;
    }

    /// `SetBasisSurface`.
    pub fn set_basis_surface(&mut self, s: Arc<dyn Surface>, not_check_c0: bool) -> Result<()> {
        self.eval_rep = None;
        let (uf, ul, vf, vl) = s.bounds();
        let mut checking: Arc<dyn Surface> = Arc::from(s.copy_surface());
        let mut is_trimmed = false;
        loop {
            let is_t = checking.as_any().is::<RectangularTrimmedSurface>();
            let is_o = checking.as_any().is::<OffsetSurface>();
            if !(is_t || is_o) {
                break;
            }
            if let Some(t) = checking.as_any().downcast_ref::<RectangularTrimmedSurface>() {
                let next = t.basis_surface();
                checking = next;
                is_trimmed = true;
            }
            if let Some(o) = checking.as_any().downcast_ref::<OffsetSurface>() {
                let next = o.basis_surface();
                self.offset_value += o.offset();
                checking = next;
            }
        }
        self.basis_surf_continuity = checking.continuity();
        let mut is_c0 = !not_check_c0 && self.basis_surf_continuity == GeomAbsShape::C0;
        if is_c0 {
            let mut curve: Option<Arc<dyn Curve>> = None;
            if let Some(rev) = checking.as_any().downcast_ref::<SurfaceOfRevolution>() {
                curve = Some(super::SweptSurface::basis_curve(rev));
            } else if let Some(ext) = checking.as_any().downcast_ref::<SurfaceOfLinearExtrusion>() {
                curve = Some(super::SweptSurface::basis_curve(ext));
            }
            if let Some(mut c) = curve.clone() {
                loop {
                    let is_tc = c.as_any().is::<TrimmedCurve>();
                    let is_oc = c.as_any().is::<OffsetCurve>();
                    if !(is_tc || is_oc) {
                        break;
                    }
                    if let Some(t) = c.as_any().downcast_ref::<TrimmedCurve>() {
                        let next = t.basis_curve();
                        c = next;
                    }
                    if let Some(oc) = c.as_any().downcast_ref::<OffsetCurve>() {
                        let next = oc.basis_curve();
                        c = next;
                    }
                }
                curve = Some(c);
            }
            let u_iso_par = (uf + ul) / 2.0;
            let v_iso_par = (vf + vl) / 2.0;
            let curv1: Arc<dyn Curve> = match curve {
                Some(c) => c,
                None => Arc::from(checking.u_iso(u_iso_par)?),
            };
            let curv2: Arc<dyn Curve> = Arc::from(checking.v_iso(v_iso_par)?);
            let mut is_ug1 = !curv1.as_any().is::<BSplineCurve>();
            let mut is_vg1 = !curv2.as_any().is::<BSplineCurve>();
            if !is_ug1 && let Some(bc) = curv1.as_any().downcast_ref::<BSplineCurve>() {
                is_ug1 = bc.is_g1(vf, vl, angular())?;
            }
            if !is_vg1 && let Some(bc) = curv2.as_any().downcast_ref::<BSplineCurve>() {
                is_vg1 = bc.is_g1(uf, ul, angular())?;
            }
            if is_ug1 && is_vg1 {
                self.basis_surf_continuity = GeomAbsShape::G1;
                is_c0 = false;
            }
            if is_c0 {
                return Err(GeomError::Construction);
            }
        }
        self.basis_surf = if is_trimmed {
            Arc::new(RectangularTrimmedSurface::new(checking.clone(), uf, ul, vf, vl, true, true)?)
        } else {
            checking.clone()
        };
        let surface_rep = self.surface();
        self.set_eval_representation(make_full_surface_rep(surface_rep))?;
        if checking.as_any().is::<BSplineSurface>() || checking.as_any().is::<BezierSurface>() {
            self.osc_surf = Some(OsculatingSurface::new(checking.as_ref(), confusion())?);
        }
        Ok(())
    }

    /// `SetOffsetValue`.
    pub fn set_offset_value(&mut self, d: f64) {
        self.eval_rep = None;
        self.offset_value = d;
        let rep = make_full_surface_rep(self.surface());
        let _ = self.set_eval_representation(rep);
    }

    /// `Offset`.
    pub fn offset(&self) -> f64 {
        self.offset_value
    }

    /// `BasisSurface`.
    pub fn basis_surface(&self) -> Arc<dyn Surface> {
        self.basis_surf.clone()
    }

    /// `GetBasisSurfContinuity`.
    pub fn get_basis_surf_continuity(&self) -> GeomAbsShape {
        self.basis_surf_continuity
    }

    // directRepSurface
    fn direct_rep_surface(&self) -> Option<Arc<dyn Surface>> {
        let d = self.eval_rep.as_ref()?;
        if d.kind != EvalRepKind::Full {
            return None;
        }
        d.representation.clone()
    }

    /// `Surface`: the equivalent analytic surface (`None` is a null handle).
    pub fn surface(&self) -> Option<Arc<dyn Surface>> {
        if self.offset_value == 0.0 {
            return Some(self.basis_surf.clone());
        }
        let tol = confusion();
        let mut result: Option<Arc<dyn Surface>> = None;
        let mut bounds = (0.0, 0.0, 0.0, 0.0);
        let (base, is_trimmed): (Arc<dyn Surface>, bool) =
            if let Some(s) = self.basis_surf.as_any().downcast_ref::<RectangularTrimmedSurface>() {
                bounds = s.bounds();
                (s.basis_surface(), true)
            } else {
                (self.basis_surf.clone(), false)
            };
        let offset = self.offset_value;
        if let Some(p) = base.as_any().downcast_ref::<Plane>() {
            let mut t = gp::Vec::from_dir(p.position().direction());
            t.multiply(offset);
            let mut np = p.clone();
            np.translate(&t);
            result = Some(Arc::new(np));
        } else if let Some(c) = base.as_any().downcast_ref::<CylindricalSurface>() {
            let mut axis = *c.position();
            let sign = if axis.direct() { 1.0 } else { -1.0 };
            let radius = c.radius() + sign * offset;
            if radius >= tol {
                result = CylindricalSurface::new(axis, radius).ok().map(|s| Arc::new(s) as Arc<dyn Surface>);
            } else if radius <= -tol {
                axis.x_reverse();
                result = CylindricalSurface::new(axis, -radius).ok().map(|s| Arc::new(s) as Arc<dyn Surface>);
            }
        } else if let Some(c) = base.as_any().downcast_ref::<ConicalSurface>() {
            let mut axis = *c.position();
            let sign = if axis.direct() { 1.0 } else { -1.0 };
            let alpha = c.semi_angle();
            let cos = libm::cos(alpha);
            let sin = libm::sin(alpha);
            let radius = c.ref_radius() + sign * offset * cos;
            if radius >= 0.0 {
                let mut z = gp::Vec::from_dir(axis.direction());
                z.multiply(-sign * offset * sin);
                axis.translate(&z);
                result = ConicalSurface::new(axis, alpha, radius).ok().map(|s| Arc::new(s) as Arc<dyn Surface>);
            }
        } else if let Some(s) = base.as_any().downcast_ref::<SphericalSurface>() {
            let mut axis = *s.position();
            let sign = if axis.direct() { 1.0 } else { -1.0 };
            let radius = s.radius() + sign * offset;
            if radius >= tol {
                result = SphericalSurface::new(axis, radius).ok().map(|s| Arc::new(s) as Arc<dyn Surface>);
            } else if radius <= -tol {
                axis.x_reverse();
                axis.z_reverse();
                result = SphericalSurface::new(axis, -radius).ok().map(|s| Arc::new(s) as Arc<dyn Surface>);
            }
        } else if let Some(s) = base.as_any().downcast_ref::<ToroidalSurface>() {
            let major = s.major_radius();
            let mut axis = *s.position();
            let sign = if axis.direct() { 1.0 } else { -1.0 };
            let minor = s.minor_radius() + sign * offset;
            if minor >= tol && minor <= major {
                result = ToroidalSurface::new(axis, major, minor).ok().map(|s| Arc::new(s) as Arc<dyn Surface>);
            } else if minor <= -tol && -minor <= major {
                axis.x_reverse();
                result = ToroidalSurface::new(axis, major, -minor).ok().map(|s| Arc::new(s) as Arc<dyn Surface>);
            }
        }
        if is_trimmed && let Some(r) = result.clone() {
            let (u1, u2, v1, v2) = bounds;
            result = RectangularTrimmedSurface::new(r, u1, u2, v1, v2, true, true)
                .ok()
                .map(|s| Arc::new(s) as Arc<dyn Surface>);
        }
        result
    }

    /// `UOsculatingSurface`: `Some((opposite, surface))` when it exists.
    pub fn u_osculating_surface(&self, u: f64, v: f64) -> Option<(bool, BSplineSurface)> {
        let osc = self.osc_surf.as_ref()?;
        let mut t = false;
        let mut l = None;
        if osc.u_osculating_surface(u, v, &mut t, &mut l) { l.map(|s| (t, s)) } else { None }
    }

    /// `VOsculatingSurface`.
    pub fn v_osculating_surface(&self, u: f64, v: f64) -> Option<(bool, BSplineSurface)> {
        let osc = self.osc_surf.as_ref()?;
        let mut t = false;
        let mut l = None;
        if osc.v_osculating_surface(u, v, &mut t, &mut l) { l.map(|s| (t, s)) } else { None }
    }

    // The copy constructor
    fn copy_offset(&self) -> OffsetSurface {
        OffsetSurface {
            basis_surf: Arc::from(self.basis_surf.copy_surface()),
            eval_rep: rep_utils::clone_surface_desc(&self.eval_rep),
            offset_value: self.offset_value,
            osc_surf: self.osc_surf.clone(),
            basis_surf_continuity: self.basis_surf_continuity,
        }
    }
}

impl Geometry for OffsetSurface {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.eval_rep = None;
        if let Some(b) = make_unique_surface(&mut self.basis_surf) {
            b.transform(t);
        }
        self.offset_value *= t.scale_factor();
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.copy_offset())
    }
}

fn is_conic_circle_or_ellipse(c: &Arc<dyn Curve>) -> bool {
    c.as_any().is::<Circle>() || c.as_any().is::<Ellipse>()
}

fn is_elementary(s: &Arc<dyn Surface>) -> bool {
    let a = s.as_any();
    a.is::<Plane>()
        || a.is::<CylindricalSurface>()
        || a.is::<ConicalSurface>()
        || a.is::<SphericalSurface>()
        || a.is::<ToroidalSurface>()
}

impl Surface for OffsetSurface {
    fn copy_surface(&self) -> Box<dyn Surface> {
        Box::new(self.copy_offset())
    }
    fn u_reverse(&mut self) {
        self.eval_rep = None;
        if let Some(b) = make_unique_surface(&mut self.basis_surf) {
            b.u_reverse();
        }
        self.offset_value = -self.offset_value;
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        self.basis_surf.u_reversed_parameter(u)
    }
    fn v_reverse(&mut self) {
        self.eval_rep = None;
        if let Some(b) = make_unique_surface(&mut self.basis_surf) {
            b.v_reverse();
        }
        self.offset_value = -self.offset_value;
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        self.basis_surf.v_reversed_parameter(v)
    }
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64) {
        let (mut u, mut v) = self.basis_surf.transform_parameters(u, v, t);
        if let Some(eq) = self.direct_rep_surface() {
            (u, v) = eq.transform_parameters(u, v, t);
        }
        (u, v)
    }
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d {
        self.basis_surf.parametric_transformation(t)
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        self.basis_surf.bounds()
    }
    fn is_u_closed(&self) -> bool {
        let sbasis = self.basis_surf.clone();
        if let Some(st) = sbasis.as_any().downcast_ref::<RectangularTrimmedSurface>() {
            let s = st.basis_surface();
            if is_elementary(&s) {
                sbasis.is_u_closed()
            } else if let Some(extru) = s.as_any().downcast_ref::<SurfaceOfLinearExtrusion>() {
                let c = super::SweptSurface::basis_curve(extru);
                if is_conic_circle_or_ellipse(&c) { sbasis.is_u_closed() } else { false }
            } else if s.as_any().is::<SurfaceOfRevolution>() {
                sbasis.is_u_closed()
            } else {
                false
            }
        } else if is_elementary(&sbasis) {
            sbasis.is_u_closed()
        } else if let Some(extru) = sbasis.as_any().downcast_ref::<SurfaceOfLinearExtrusion>() {
            let c = super::SweptSurface::basis_curve(extru);
            is_conic_circle_or_ellipse(&c)
        } else {
            sbasis.as_any().is::<SurfaceOfRevolution>()
        }
    }
    fn is_v_closed(&self) -> bool {
        let sbasis = self.basis_surf.clone();
        if let Some(st) = sbasis.as_any().downcast_ref::<RectangularTrimmedSurface>() {
            let s = st.basis_surface();
            if is_elementary(&s) { sbasis.is_v_closed() } else { false }
        } else if is_elementary(&sbasis) {
            sbasis.is_v_closed()
        } else {
            false
        }
    }
    fn is_u_periodic(&self) -> bool {
        self.basis_surf.is_u_periodic()
    }
    fn u_period(&self) -> Result<f64> {
        self.basis_surf.u_period()
    }
    fn is_v_periodic(&self) -> bool {
        self.basis_surf.is_v_periodic()
    }
    fn v_period(&self) -> Result<f64> {
        self.basis_surf.v_period()
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        if let Some(eq) = self.direct_rep_surface() {
            return eq.u_iso(u);
        }
        // The approximation through `AdvApprox` of OCCT is not part of this translation, except for the extrusion.
        if let Some(ext) = self.basis_surf.as_any().downcast_ref::<SurfaceOfLinearExtrusion>() {
            let mut l = ext.u_iso(u)?;
            let r = self.basis_surf.eval_d1(u, 0.0)?;
            let mut dir = r.d1u.crossed(&r.d1v);
            if dir.square_magnitude() < gp::resolution() {
                return Ok(l);
            }
            let _ = dir.normalize();
            dir.multiply(self.offset_value);
            l.translate(&dir);
            return Ok(l);
        }
        Err(GeomError::NotImplemented)
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        if let Some(eq) = self.direct_rep_surface() {
            return eq.v_iso(v);
        }
        Err(GeomError::NotImplemented)
    }
    fn continuity(&self) -> GeomAbsShape {
        match self.basis_surf_continuity {
            GeomAbsShape::C2 => GeomAbsShape::C1,
            GeomAbsShape::C3 => GeomAbsShape::C2,
            GeomAbsShape::Cn => GeomAbsShape::Cn,
            _ => GeomAbsShape::C0,
        }
    }
    fn is_cn_u(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_surf.is_cn_u(n + 1)
    }
    fn is_cn_v(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_surf.is_cn_v(n + 1)
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        if let Some(r) = rep_utils::try_eval_surface_d0(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        match evaluate_d0(u, v, self.basis_surf.as_ref(), self.offset_value, self.osc_surf.as_ref())? {
            Some(p) => Ok(p),
            None => Err(GeomError::UndefinedValue),
        }
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        if let Some(r) = rep_utils::try_eval_surface_d1(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        match evaluate_d1(u, v, self.basis_surf.as_ref(), self.offset_value, self.osc_surf.as_ref())? {
            Some(r) => Ok(r),
            None => Err(GeomError::UndefinedDerivative),
        }
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        if let Some(r) = rep_utils::try_eval_surface_d2(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        match evaluate_d2(u, v, self.basis_surf.as_ref(), self.offset_value, self.osc_surf.as_ref())? {
            Some(r) => Ok(r),
            None => Err(GeomError::UndefinedDerivative),
        }
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        if let Some(r) = rep_utils::try_eval_surface_d3(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        match evaluate_d3(u, v, self.basis_surf.as_ref(), self.offset_value, self.osc_surf.as_ref())? {
            Some(r) => Ok(r),
            None => Err(GeomError::UndefinedDerivative),
        }
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        if let Some(r) = rep_utils::try_eval_surface_dn(&self.eval_rep, u, v, nu, nv)? {
            return Ok(r);
        }
        match evaluate_dn(u, v, nu, nv, self.basis_surf.as_ref(), self.offset_value, self.osc_surf.as_ref())? {
            Some(r) => Ok(r),
            None => Err(GeomError::UndefinedDerivative),
        }
    }
}

impl super::BoundedSurface for OffsetSurface {}

// ---------------------------------------------------------------------------------------------
// Geom_OffsetSurfaceUtils

// `NCollection_Array2<gp_Vec>(0, rows - 1, 0, cols - 1)`, zero initialized.
#[derive(Debug, Clone)]
struct VGrid {
    rows: usize,
    cols: usize,
    values: Vec<gp::Vec>,
}

impl VGrid {
    fn new(rows: i32, cols: i32) -> Result<Self> {
        let r = usize::try_from(rows).map_err(|_| GeomError::OutOfRange)?;
        let c = usize::try_from(cols).map_err(|_| GeomError::OutOfRange)?;
        Ok(VGrid { rows: r, cols: c, values: vec![gp::Vec::new(0.0, 0.0, 0.0); r * c] })
    }
    fn slot(&self, i: i32, j: i32) -> Result<usize> {
        let r = usize::try_from(i).map_err(|_| GeomError::OutOfRange)?;
        let c = usize::try_from(j).map_err(|_| GeomError::OutOfRange)?;
        if r >= self.rows || c >= self.cols {
            return Err(GeomError::OutOfRange);
        }
        Ok(r * self.cols + c)
    }
    fn set(&mut self, i: i32, j: i32, v: gp::Vec) -> Result<()> {
        let s = self.slot(i, j)?;
        *self.values.get_mut(s).ok_or(GeomError::OutOfRange)? = v;
        Ok(())
    }
    fn get(&self, i: i32, j: i32) -> Result<gp::Vec> {
        let s = self.slot(i, j)?;
        self.values.get(s).copied().ok_or(GeomError::OutOfRange)
    }
    fn to_cs(&self) -> Result<VecArray2> {
        Ok(VecArray2::new(0, 0, self.rows, self.cols, self.values.clone())?)
    }
}

#[derive(Default, Clone, Copy)]
struct OscInfo {
    along_u: bool,
    along_v: bool,
    is_opposite: bool,
}

impl OscInfo {
    fn sign(&self) -> f64 {
        if (self.along_u || self.along_v) && self.is_opposite { -1.0 } else { 1.0 }
    }
    fn has_osculating(&self) -> bool {
        self.along_u || self.along_v
    }
}

fn osc_query(osc: Option<&OsculatingSurface>, u: f64, v: f64) -> (OscInfo, Option<BSplineSurface>) {
    let mut info = OscInfo::default();
    let mut surf: Option<BSplineSurface> = None;
    if let Some(o) = osc {
        info.along_u = o.u_osculating_surface(u, v, &mut info.is_opposite, &mut surf);
        info.along_v = o.v_osculating_surface(u, v, &mut info.is_opposite, &mut surf);
    }
    (info, surf)
}

fn is_infinite_coord(v: &gp::Vec) -> bool {
    is_infinite(v.x()) || is_infinite(v.y()) || is_infinite(v.z())
}

// The first derivatives scaled to a magnitude of at most 1.
fn scaled_d1(d1u: &gp::Vec, d1v: &gp::Vec) -> (gp::Vec, gp::Vec) {
    let mut a = *d1u;
    let mut b = *d1v;
    let n2a = a.square_magnitude();
    let n2b = b.square_magnitude();
    if n2a > 1.0 {
        a.divide(n2a.sqrt());
    }
    if n2b > 1.0 {
        b.divide(n2b.sqrt());
    }
    (a, b)
}

// ComputeNormal
fn compute_normal(d1u: &gp::Vec, d1v: &gp::Vec) -> Option<gp::Vec> {
    let (a, b) = scaled_d1(d1u, d1v);
    let mut normal = a.crossed(&b);
    if normal.square_magnitude() <= D1_MAGNITUDE_TOL * D1_MAGNITUDE_TOL {
        return None;
    }
    let _ = normal.normalize();
    Some(normal)
}

// `theValue.SetXYZ(theValue.XYZ() + k * N)`
fn add_scaled(value: &mut Pnt, k: f64, n: &gp::XYZ) {
    *value = Pnt::from_xyz(value.xyz().added(&n.multiplied(k)));
}

// CalculateD0
fn calculate_d0(value: &mut Pnt, d1u: &gp::Vec, d1v: &gp::Vec, offset: f64, sign: f64) -> bool {
    let Some(norm) = compute_normal(d1u, d1v) else { return false };
    add_scaled(value, offset * sign, norm.xyz());
    true
}

// ComputeDerivatives (the oscillating surface is a `Geom_BSplineSurface`)
#[allow(clippy::too_many_arguments)]
fn compute_derivatives(
    max_order_in: i32,
    min_order: i32,
    u: f64,
    v: f64,
    basis: &dyn Surface,
    nu: i32,
    nv: i32,
    along_u: bool,
    along_v: bool,
    osc: Option<&BSplineSurface>,
    der_nuv: &mut VGrid,
    der_surf: &mut VGrid,
) -> Result<bool> {
    let mut max_order = max_order_in;
    if along_u || along_v {
        let osc = osc.ok_or(GeomError::Program)?;
        max_order = 0;
        let mut der_surf_l = VGrid::new(max_order + nu + 2, max_order + nv + 2)?;
        match min_order {
            1 => {
                let a = osc.eval_d1(u, v)?;
                der_surf_l.set(1, 0, a.d1u)?;
                der_surf_l.set(0, 1, a.d1v)?;
            }
            2 => {
                let a = osc.eval_d2(u, v)?;
                der_surf_l.set(1, 0, a.d1u)?;
                der_surf_l.set(0, 1, a.d1v)?;
                der_surf_l.set(1, 1, a.d2uv)?;
                der_surf_l.set(2, 0, a.d2u)?;
                der_surf_l.set(0, 2, a.d2v)?;
            }
            3 => {
                let a = osc.eval_d3(u, v)?;
                der_surf_l.set(1, 0, a.d1u)?;
                der_surf_l.set(0, 1, a.d1v)?;
                der_surf_l.set(1, 1, a.d2uv)?;
                der_surf_l.set(2, 0, a.d2u)?;
                der_surf_l.set(0, 2, a.d2v)?;
                der_surf_l.set(3, 0, a.d3u)?;
                der_surf_l.set(2, 1, a.d3uuv)?;
                der_surf_l.set(1, 2, a.d3uvv)?;
                der_surf_l.set(0, 3, a.d3v)?;
            }
            _ => {}
        }
        if nu <= nv {
            for i in 0..=max_order + 1 + nu {
                for j in i..=max_order + nv + 1 {
                    if i + j > min_order {
                        let osc_dn = osc.eval_dn(u, v, i, j)?;
                        let basis_dn = basis.eval_dn(u, v, i, j)?;
                        der_surf_l.set(i, j, osc_dn)?;
                        der_surf.set(i, j, basis_dn)?;
                        if i != j && j <= nu + 1 {
                            let basis_dnji = basis.eval_dn(u, v, j, i)?;
                            let osc_dnji = osc.eval_dn(u, v, j, i)?;
                            der_surf.set(j, i, basis_dnji)?;
                            der_surf_l.set(j, i, osc_dnji)?;
                        }
                    }
                }
            }
        } else {
            for j in 0..=max_order + 1 + nv {
                for i in j..=max_order + nu + 1 {
                    if i + j > min_order {
                        let osc_dn = osc.eval_dn(u, v, i, j)?;
                        let basis_dn = basis.eval_dn(u, v, i, j)?;
                        der_surf_l.set(i, j, osc_dn)?;
                        der_surf.set(i, j, basis_dn)?;
                        if i != j && i <= nv + 1 {
                            let basis_dnji = basis.eval_dn(u, v, j, i)?;
                            let osc_dnji = osc.eval_dn(u, v, j, i)?;
                            der_surf.set(j, i, basis_dnji)?;
                            der_surf_l.set(j, i, osc_dnji)?;
                        }
                    }
                }
            }
        }
        for i in 0..=max_order + nu {
            for j in 0..=max_order + nv {
                if along_u {
                    let r = cslib::dn_nuv_two(i, j, &der_surf_l.to_cs()?, &der_surf.to_cs()?)?;
                    der_nuv.set(i, j, r)?;
                }
                if along_v {
                    let r = cslib::dn_nuv_two(i, j, &der_surf.to_cs()?, &der_surf_l.to_cs()?)?;
                    der_nuv.set(i, j, r)?;
                }
            }
        }
    } else {
        for i in 0..=max_order + nu + 1 {
            for j in i..=max_order + nv + 1 {
                if i + j > min_order {
                    let dn = basis.eval_dn(u, v, i, j)?;
                    der_surf.set(i, j, dn)?;
                    // `j <= theDerSurf.UpperRow() && i <= theDerSurf.UpperCol()`
                    if i != j && (j as usize) < der_surf.rows && (i as usize) < der_surf.cols {
                        let dnji = basis.eval_dn(u, v, j, i)?;
                        der_surf.set(j, i, dnji)?;
                    }
                }
            }
        }
        for i in 0..=max_order + nu {
            for j in 0..=max_order + nv {
                let r = cslib::dn_nuv(i, j, &der_surf.to_cs()?)?;
                der_nuv.set(i, j, r)?;
            }
        }
    }
    Ok(true)
}

// ReplaceDerivative
#[allow(clippy::too_many_arguments)]
fn replace_derivative(
    u: f64,
    v: f64,
    umin: f64,
    umax: f64,
    vmin: f64,
    vmax: f64,
    du: &mut gp::Vec,
    dv: &mut gp::Vec,
    square_tol: f64,
    basis: &dyn Surface,
) -> Result<bool> {
    let is_replace_du = du.square_magnitude() < square_tol;
    let is_replace_dv = dv.square_magnitude() < square_tol;
    let mut is_replaced = false;
    if is_replace_du != is_replace_dv {
        let mut step;
        if is_replace_dv {
            step = confusion() * du.magnitude();
            if step > umax - umin {
                step = (umax - umin) / 100.0;
            }
        } else {
            step = confusion() * dv.magnitude();
            if step > vmax - vmin {
                step = (vmax - vmin) / 100.0;
            }
        }
        let mut step_sign = -1.0;
        while step_sign <= 1.0 && !is_replaced {
            let mut au = u;
            let mut av = v;
            let mut skip = false;
            if is_replace_dv {
                au = u + step_sign * step;
                if au < umin || au > umax {
                    skip = true;
                }
            } else {
                av = v + step_sign * step;
                if av < vmin || av > vmax {
                    skip = true;
                }
            }
            if !skip {
                let r = basis.eval_d1(au, av)?;
                if is_replace_du && r.d1u.square_magnitude() > square_tol {
                    *du = r.d1u;
                    is_replaced = true;
                }
                if is_replace_dv && r.d1v.square_magnitude() > square_tol {
                    *dv = r.d1v;
                    is_replaced = true;
                }
            }
            step_sign += 2.0;
        }
    }
    Ok(is_replaced)
}

// ShiftPoint
#[allow(clippy::too_many_arguments)]
fn shift_point(
    u_start: f64,
    v_start: f64,
    u: &mut f64,
    v: &mut f64,
    umin: f64,
    umax: f64,
    vmin: f64,
    vmax: f64,
    is_u_periodic: bool,
    is_v_periodic: bool,
    d1u: &gp::Vec,
    d1v: &gp::Vec,
) -> bool {
    let is_u_singular = d1u.square_magnitude() < D1_MAGNITUDE_TOL * D1_MAGNITUDE_TOL;
    let is_v_singular = d1v.square_magnitude() < D1_MAGNITUDE_TOL * D1_MAGNITUDE_TOL;
    let dir_u = if is_u_periodic || (is_u_singular && !is_v_singular) { 0.0 } else { 0.5 * (umin + umax) - u_start };
    let dir_v = if is_v_periodic || (is_v_singular && !is_u_singular) { 0.0 } else { 0.5 * (vmin + vmax) - v_start };
    let dist = (dir_u * dir_u + dir_v * dir_v).sqrt();
    let du = *u - u_start;
    let dv = *v - v_start;
    let mut step = (2.0 * (du * du + dv * dv).sqrt()).max(pconfusion());
    if step >= dist {
        return false;
    }
    step /= dist;
    *u += dir_u * step;
    *v += dir_v * step;
    true
}

fn normal_of(max_order: i32, der_nuv: &VGrid, u: f64, v: f64, b: (f64, f64, f64, f64)) -> Result<cslib::NormalOfOrder> {
    Ok(cslib::normal_of_order(max_order, &der_nuv.to_cs()?, D1_MAGNITUDE_TOL, u, v, b.0, b.1, b.2, b.3)?)
}

fn normal_xyz(n: &Option<crate::gp::Dir>) -> Result<gp::XYZ> {
    n.as_ref().map(|d| *d.xyz()).ok_or(GeomError::Program)
}

// The `if (HasOsculating && !OscSurf.IsNull()) ComputeDerivatives(...osc...) else ComputeDerivatives(...no osc...)`.
#[allow(clippy::too_many_arguments)]
fn derivatives_with(
    max_order: i32,
    min_order: i32,
    u: f64,
    v: f64,
    basis: &dyn Surface,
    nu: i32,
    nv: i32,
    info: &OscInfo,
    osc_surf: &Option<BSplineSurface>,
    der_nuv: &mut VGrid,
    der_surf: &mut VGrid,
) -> Result<bool> {
    if info.has_osculating() && osc_surf.is_some() {
        compute_derivatives(
            max_order,
            min_order,
            u,
            v,
            basis,
            nu,
            nv,
            info.along_u,
            info.along_v,
            osc_surf.as_ref(),
            der_nuv,
            der_surf,
        )
    } else {
        compute_derivatives(max_order, min_order, u, v, basis, nu, nv, false, false, None, der_nuv, der_surf)
    }
}

// EvaluateD0 (with the first derivatives): `None` is the C++ `false`.
fn evaluate_d0(
    u0: f64,
    v0: f64,
    basis: &dyn Surface,
    offset: f64,
    osc: Option<&OsculatingSurface>,
) -> Result<Option<Pnt>> {
    let b1 = basis.eval_d1(u0, v0)?;
    let mut value = b1.point;
    let (u_start, v_start) = (u0, v0);
    let (mut u, mut v) = (u0, v0);
    let bounds = basis.bounds();
    let (umin, umax, vmin, vmax) = bounds;
    let is_u_per = basis.is_u_periodic();
    let is_v_per = basis.is_v_periodic();
    let mut d1u = b1.d1u;
    let mut d1v = b1.d1v;
    let mut first = true;
    loop {
        if !first {
            let r = basis.eval_d1(u, v)?;
            value = r.point;
            d1u = r.d1u;
            d1v = r.d1v;
        }
        first = false;
        if is_infinite_coord(&d1u) || is_infinite_coord(&d1v) {
            return Ok(None);
        }
        if calculate_d0(&mut value, &d1u, &d1v, offset, 1.0) {
            return Ok(Some(value));
        }
        let max_order = 3;
        let (info, osc_surf) = osc_query(osc, u, v);
        let mut der_nuv = VGrid::new(max_order + 1, max_order + 1)?;
        let mut der_surf = VGrid::new(max_order + 2, max_order + 2)?;
        der_surf.set(1, 0, d1u)?;
        der_surf.set(0, 1, d1v)?;
        derivatives_with(max_order, 1, u, v, basis, 0, 0, &info, &osc_surf, &mut der_nuv, &mut der_surf)?;
        let n = normal_of(max_order, &der_nuv, u, v, bounds)?;
        let mut status = n.status;
        let mut normal = n.normal;
        if status == NormalStatus::InfinityOfSolutions {
            let mut new_du = d1u;
            let mut new_dv = d1v;
            if replace_derivative(
                u,
                v,
                umin,
                umax,
                vmin,
                vmax,
                &mut new_du,
                &mut new_dv,
                D1_MAGNITUDE_TOL * D1_MAGNITUDE_TOL,
                basis,
            )? {
                let (s, nm) = cslib::normal_mag_tol(&new_du, &new_dv, D1_MAGNITUDE_TOL)?;
                status = s;
                normal = nm;
            }
        }
        if status == NormalStatus::Defined {
            add_scaled(&mut value, offset * info.sign(), &normal_xyz(&normal)?);
            return Ok(Some(value));
        }
        if !shift_point(u_start, v_start, &mut u, &mut v, umin, umax, vmin, vmax, is_u_per, is_v_per, &d1u, &d1v) {
            return Ok(None);
        }
    }
}

// EvaluateD1
fn evaluate_d1(
    u0: f64,
    v0: f64,
    basis: &dyn Surface,
    offset: f64,
    osc: Option<&OsculatingSurface>,
) -> Result<Option<SurfResD1>> {
    let b2 = basis.eval_d2(u0, v0)?;
    let mut value = b2.point;
    let mut the_d1u = b2.d1u;
    let mut the_d1v = b2.d1v;
    let (u_start, v_start) = (u0, v0);
    let (mut u, mut v) = (u0, v0);
    let bounds = basis.bounds();
    let (umin, umax, vmin, vmax) = bounds;
    let is_u_per = basis.is_u_periodic();
    let is_v_per = basis.is_v_periodic();
    let mut d2u = b2.d2u;
    let mut d2v = b2.d2v;
    let mut d2uv = b2.d2uv;
    let mut first = true;
    loop {
        if !first {
            let r = basis.eval_d2(u, v)?;
            value = r.point;
            the_d1u = r.d1u;
            the_d1v = r.d1v;
            d2u = r.d2u;
            d2v = r.d2v;
            d2uv = r.d2uv;
        }
        first = false;
        if is_infinite_coord(&the_d1u) || is_infinite_coord(&the_d1v) {
            return Ok(None);
        }
        let (a_d1u, a_d1v) = scaled_d1(&the_d1u, &the_d1v);
        let mut is_singular = false;
        let max_order = 3;
        let mut norm = a_d1u.crossed(&a_d1v);
        let mut info = OscInfo::default();
        let mut osc_surf: Option<BSplineSurface> = None;
        if norm.square_magnitude() <= D1_MAGNITUDE_TOL * D1_MAGNITUDE_TOL {
            if osc.is_some() {
                (info, osc_surf) = osc_query(osc, u, v);
            }
            is_singular = true;
        }
        let sign = info.sign();
        if !is_singular {
            let _ = norm.normalize();
            add_scaled(&mut value, offset * sign, norm.xyz());
            let n0 = gp::Vec::from_xyz(*norm.xyz());
            let scale = the_d1u.crossed(&the_d1v).dot(&n0);
            let mut n1u = gp::Vec::new(0.0, 0.0, 0.0);
            n1u.set_x(d2u.y() * the_d1v.z() + the_d1u.y() * d2uv.z() - d2u.z() * the_d1v.y() - the_d1u.z() * d2uv.y());
            n1u.set_y(
                -(d2u.x() * the_d1v.z() + the_d1u.x() * d2uv.z() - d2u.z() * the_d1v.x() - the_d1u.z() * d2uv.x()),
            );
            n1u.set_z(d2u.x() * the_d1v.y() + the_d1u.x() * d2uv.y() - d2u.y() * the_d1v.x() - the_d1u.y() * d2uv.x());
            let scale_u = n1u.dot(&n0);
            n1u.subtract(&n0.multiplied(scale_u));
            n1u.divide(scale);
            let mut n1v = gp::Vec::new(0.0, 0.0, 0.0);
            n1v.set_x(d2uv.y() * the_d1v.z() + d2v.z() * the_d1u.y() - d2uv.z() * the_d1v.y() - d2v.y() * the_d1u.z());
            n1v.set_y(
                -(d2uv.x() * the_d1v.z() + d2v.z() * the_d1u.x() - d2uv.z() * the_d1v.x() - d2v.x() * the_d1u.z()),
            );
            n1v.set_z(d2uv.x() * the_d1v.y() + d2v.y() * the_d1u.x() - d2uv.y() * the_d1v.x() - d2v.x() * the_d1u.y());
            let scale_v = n1v.dot(&n0);
            n1v.subtract(&n0.multiplied(scale_v));
            n1v.divide(scale);
            the_d1u.add(&n1u.multiplied(offset * sign));
            the_d1v.add(&n1v.multiplied(offset * sign));
            return Ok(Some(SurfResD1 { point: value, d1u: the_d1u, d1v: the_d1v }));
        }
        let mut der_nuv = VGrid::new(max_order + 2, max_order + 2)?;
        let mut der_surf = VGrid::new(max_order + 3, max_order + 3)?;
        der_surf.set(1, 0, the_d1u)?;
        der_surf.set(0, 1, the_d1v)?;
        der_surf.set(1, 1, d2uv)?;
        der_surf.set(2, 0, d2u)?;
        der_surf.set(0, 2, d2v)?;
        derivatives_with(max_order, 2, u, v, basis, 1, 1, &info, &osc_surf, &mut der_nuv, &mut der_surf)?;
        let n = normal_of(max_order, &der_nuv, u, v, bounds)?;
        let mut status = n.status;
        let mut normal = n.normal;
        let (mut order_u, mut order_v) = (n.order_u, n.order_v);
        if status == NormalStatus::InfinityOfSolutions {
            let mut new_du = the_d1u;
            let mut new_dv = the_d1v;
            if replace_derivative(
                u,
                v,
                umin,
                umax,
                vmin,
                vmax,
                &mut new_du,
                &mut new_dv,
                D1_MAGNITUDE_TOL * D1_MAGNITUDE_TOL,
                basis,
            )? {
                der_surf.set(1, 0, new_du)?;
                der_surf.set(0, 1, new_dv)?;
                derivatives_with(max_order, 2, u, v, basis, 1, 1, &info, &osc_surf, &mut der_nuv, &mut der_surf)?;
                let n = normal_of(max_order, &der_nuv, u, v, bounds)?;
                status = n.status;
                normal = n.normal;
                order_u = n.order_u;
                order_v = n.order_v;
            }
        }
        if status == NormalStatus::Defined {
            add_scaled(&mut value, offset * sign, &normal_xyz(&normal)?);
            let dnu = cslib::dn_normal(1, 0, &der_nuv.to_cs()?, order_u, order_v)?;
            let dnv = cslib::dn_normal(0, 1, &der_nuv.to_cs()?, order_u, order_v)?;
            let mut r_d1u = dnu;
            let mut r_d1v = dnv;
            r_d1u.multiply(offset * sign);
            r_d1u.add(&der_surf.get(1, 0)?);
            r_d1v.multiply(offset * sign);
            r_d1v.add(&der_surf.get(0, 1)?);
            return Ok(Some(SurfResD1 { point: value, d1u: r_d1u, d1v: r_d1v }));
        }
        if !shift_point(
            u_start, v_start, &mut u, &mut v, umin, umax, vmin, vmax, is_u_per, is_v_per, &the_d1u, &the_d1v,
        ) {
            return Ok(None);
        }
    }
}

// EvaluateD2
fn evaluate_d2(
    u0: f64,
    v0: f64,
    basis: &dyn Surface,
    offset: f64,
    osc: Option<&OsculatingSurface>,
) -> Result<Option<SurfResD2>> {
    let b3 = basis.eval_d3(u0, v0)?;
    let mut value = b3.point;
    let mut the_d1u = b3.d1u;
    let mut the_d1v = b3.d1v;
    let mut the_d2u = b3.d2u;
    let mut the_d2v = b3.d2v;
    let mut the_d2uv = b3.d2uv;
    let (u_start, v_start) = (u0, v0);
    let (mut u, mut v) = (u0, v0);
    let bounds = basis.bounds();
    let (umin, umax, vmin, vmax) = bounds;
    let is_u_per = basis.is_u_periodic();
    let is_v_per = basis.is_v_periodic();
    let mut d3u = b3.d3u;
    let mut d3v = b3.d3v;
    let mut d3uuv = b3.d3uuv;
    let mut d3uvv = b3.d3uvv;
    let mut first = true;
    loop {
        if !first {
            let r = basis.eval_d3(u, v)?;
            value = r.point;
            the_d1u = r.d1u;
            the_d1v = r.d1v;
            the_d2u = r.d2u;
            the_d2v = r.d2v;
            the_d2uv = r.d2uv;
            d3u = r.d3u;
            d3v = r.d3v;
            d3uuv = r.d3uuv;
            d3uvv = r.d3uvv;
        }
        first = false;
        if is_infinite_coord(&the_d1u) || is_infinite_coord(&the_d1v) {
            return Ok(None);
        }
        let (mut nstatus, mut normal) = cslib::normal_mag_tol(&the_d1u, &the_d1v, D1_MAGNITUDE_TOL)?;
        let max_order = if nstatus == NormalStatus::Defined { 0 } else { 3 };
        let mut info = OscInfo::default();
        let mut osc_surf: Option<BSplineSurface> = None;
        if nstatus != NormalStatus::Defined && osc.is_some() {
            (info, osc_surf) = osc_query(osc, u, v);
        }
        let mut der_nuv = VGrid::new(max_order + 3, max_order + 3)?;
        let mut der_surf = VGrid::new(max_order + 4, max_order + 4)?;
        der_surf.set(1, 0, the_d1u)?;
        der_surf.set(0, 1, the_d1v)?;
        der_surf.set(1, 1, the_d2uv)?;
        der_surf.set(2, 0, the_d2u)?;
        der_surf.set(0, 2, the_d2v)?;
        der_surf.set(3, 0, d3u)?;
        der_surf.set(2, 1, d3uuv)?;
        der_surf.set(1, 2, d3uvv)?;
        der_surf.set(0, 3, d3v)?;
        derivatives_with(max_order, 3, u, v, basis, 2, 2, &info, &osc_surf, &mut der_nuv, &mut der_surf)?;
        let n = normal_of(max_order, &der_nuv, u, v, bounds)?;
        nstatus = n.status;
        normal = n.normal.or(normal);
        let (order_u, order_v) = (n.order_u, n.order_v);
        if nstatus == NormalStatus::Defined {
            let sign = offset * info.sign();
            add_scaled(&mut value, sign, &normal_xyz(&normal)?);
            let cs = der_nuv.to_cs()?;
            let r_d1u = der_surf.get(1, 0)?.added(&cslib::dn_normal(1, 0, &cs, order_u, order_v)?.multiplied(sign));
            let r_d1v = der_surf.get(0, 1)?.added(&cslib::dn_normal(0, 1, &cs, order_u, order_v)?.multiplied(sign));
            let dn20 = basis.eval_dn(u, v, 2, 0)?;
            let dn02 = basis.eval_dn(u, v, 0, 2)?;
            let dn11 = basis.eval_dn(u, v, 1, 1)?;
            let mut r_d2u = dn20;
            let mut r_d2v = dn02;
            let mut r_d2uv = dn11;
            r_d2u.add(&cslib::dn_normal(2, 0, &cs, order_u, order_v)?.multiplied(sign));
            r_d2v.add(&cslib::dn_normal(0, 2, &cs, order_u, order_v)?.multiplied(sign));
            r_d2uv.add(&cslib::dn_normal(1, 1, &cs, order_u, order_v)?.multiplied(sign));
            return Ok(Some(SurfResD2 { point: value, d1u: r_d1u, d1v: r_d1v, d2u: r_d2u, d2v: r_d2v, d2uv: r_d2uv }));
        }
        if !shift_point(
            u_start, v_start, &mut u, &mut v, umin, umax, vmin, vmax, is_u_per, is_v_per, &the_d1u, &the_d1v,
        ) {
            return Ok(None);
        }
    }
}

// EvaluateD3
fn evaluate_d3(
    u0: f64,
    v0: f64,
    basis: &dyn Surface,
    offset: f64,
    osc: Option<&OsculatingSurface>,
) -> Result<Option<SurfResD3>> {
    let (u_start, v_start) = (u0, v0);
    let (mut u, mut v) = (u0, v0);
    let bounds = basis.bounds();
    let (umin, umax, vmin, vmax) = bounds;
    let is_u_per = basis.is_u_periodic();
    let is_v_per = basis.is_v_periodic();
    loop {
        let r = basis.eval_d3(u, v)?;
        let mut value = r.point;
        let (the_d1u, the_d1v) = (r.d1u, r.d1v);
        if is_infinite_coord(&the_d1u) || is_infinite_coord(&the_d1v) {
            return Ok(None);
        }
        let (mut nstatus, mut normal) = cslib::normal_mag_tol(&the_d1u, &the_d1v, D1_MAGNITUDE_TOL)?;
        let max_order = if nstatus == NormalStatus::Defined { 0 } else { 3 };
        let mut info = OscInfo::default();
        let mut osc_surf: Option<BSplineSurface> = None;
        if nstatus != NormalStatus::Defined && osc.is_some() {
            (info, osc_surf) = osc_query(osc, u, v);
        }
        let mut der_nuv = VGrid::new(max_order + 4, max_order + 4)?;
        let mut der_surf = VGrid::new(max_order + 5, max_order + 5)?;
        der_surf.set(1, 0, r.d1u)?;
        der_surf.set(0, 1, r.d1v)?;
        der_surf.set(1, 1, r.d2uv)?;
        der_surf.set(2, 0, r.d2u)?;
        der_surf.set(0, 2, r.d2v)?;
        der_surf.set(3, 0, r.d3u)?;
        der_surf.set(2, 1, r.d3uuv)?;
        der_surf.set(1, 2, r.d3uvv)?;
        der_surf.set(0, 3, r.d3v)?;
        derivatives_with(max_order, 3, u, v, basis, 3, 3, &info, &osc_surf, &mut der_nuv, &mut der_surf)?;
        let n = normal_of(max_order, &der_nuv, u, v, bounds)?;
        nstatus = n.status;
        normal = n.normal.or(normal);
        let (order_u, order_v) = (n.order_u, n.order_v);
        if nstatus == NormalStatus::Defined {
            let sign = offset * info.sign();
            add_scaled(&mut value, sign, &normal_xyz(&normal)?);
            let cs = der_nuv.to_cs()?;
            let dnn = |a: i32, b: i32| -> Result<gp::Vec> {
                Ok(cslib::dn_normal(a, b, &cs, order_u, order_v)?.multiplied(sign))
            };
            let o_d1u = der_surf.get(1, 0)?.added(&dnn(1, 0)?);
            let o_d1v = der_surf.get(0, 1)?.added(&dnn(0, 1)?);
            let mut o_d2u = basis.eval_dn(u, v, 2, 0)?;
            let mut o_d2v = basis.eval_dn(u, v, 0, 2)?;
            let mut o_d2uv = basis.eval_dn(u, v, 1, 1)?;
            let mut o_d3u = basis.eval_dn(u, v, 3, 0)?;
            let mut o_d3v = basis.eval_dn(u, v, 0, 3)?;
            let mut o_d3uuv = basis.eval_dn(u, v, 2, 1)?;
            let mut o_d3uvv = basis.eval_dn(u, v, 1, 2)?;
            o_d2u.add(&dnn(2, 0)?);
            o_d2v.add(&dnn(0, 2)?);
            o_d2uv.add(&dnn(1, 1)?);
            o_d3u.add(&dnn(3, 0)?);
            o_d3v.add(&dnn(0, 3)?);
            o_d3uuv.add(&dnn(2, 1)?);
            o_d3uvv.add(&dnn(1, 2)?);
            return Ok(Some(SurfResD3 {
                point: value,
                d1u: o_d1u,
                d1v: o_d1v,
                d2u: o_d2u,
                d2v: o_d2v,
                d2uv: o_d2uv,
                d3u: o_d3u,
                d3v: o_d3v,
                d3uuv: o_d3uuv,
                d3uvv: o_d3uvv,
            }));
        }
        if !shift_point(
            u_start, v_start, &mut u, &mut v, umin, umax, vmin, vmax, is_u_per, is_v_per, &the_d1u, &the_d1v,
        ) {
            return Ok(None);
        }
    }
}

// EvaluateDN
fn evaluate_dn(
    u0: f64,
    v0: f64,
    nu: i32,
    nv: i32,
    basis: &dyn Surface,
    offset: f64,
    osc: Option<&OsculatingSurface>,
) -> Result<Option<gp::Vec>> {
    let b1 = basis.eval_d1(u0, v0)?;
    let (u_start, v_start) = (u0, v0);
    let (mut u, mut v) = (u0, v0);
    let bounds = basis.bounds();
    let (umin, umax, vmin, vmax) = bounds;
    let is_u_per = basis.is_u_periodic();
    let is_v_per = basis.is_v_periodic();
    let mut d1u = b1.d1u;
    let mut d1v = b1.d1v;
    let mut first = true;
    loop {
        if !first {
            let r = basis.eval_d1(u, v)?;
            d1u = r.d1u;
            d1v = r.d1v;
        }
        first = false;
        if is_infinite_coord(&d1u) || is_infinite_coord(&d1v) {
            return Ok(None);
        }
        let (mut nstatus, mut normal) = cslib::normal_mag_tol(&d1u, &d1v, D1_MAGNITUDE_TOL)?;
        let max_order = if nstatus == NormalStatus::Defined { 0 } else { 3 };
        let mut der_nuv = VGrid::new(max_order + nu + 1, max_order + nv + 1)?;
        let mut der_surf = VGrid::new(max_order + nu + 2, max_order + nv + 2)?;
        der_surf.set(1, 0, d1u)?;
        der_surf.set(0, 1, d1v)?;
        let mut info = OscInfo::default();
        let mut osc_surf: Option<BSplineSurface> = None;
        if nstatus != NormalStatus::Defined && osc.is_some() {
            (info, osc_surf) = osc_query(osc, u, v);
        }
        derivatives_with(max_order, 1, u, v, basis, nu, nv, &info, &osc_surf, &mut der_nuv, &mut der_surf)?;
        let n = normal_of(max_order, &der_nuv, u, v, bounds)?;
        nstatus = n.status;
        normal = n.normal.or(normal);
        let _ = &normal;
        if nstatus == NormalStatus::Defined {
            let sign = offset * info.sign();
            let basis_dn = basis.eval_dn(u, v, nu, nv)?;
            let mut result = basis_dn;
            result.add(&cslib::dn_normal(nu, nv, &der_nuv.to_cs()?, n.order_u, n.order_v)?.multiplied(sign));
            return Ok(Some(result));
        }
        if !shift_point(u_start, v_start, &mut u, &mut v, umin, umax, vmin, vmax, is_u_per, is_v_per, &d1u, &d1v) {
            return Ok(None);
        }
    }
}
