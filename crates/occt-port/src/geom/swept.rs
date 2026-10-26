//! `Geom_SurfaceOfLinearExtrusion` (with `Geom_ExtrusionUtils`) and `Geom_SurfaceOfRevolution` (with
//! `Geom_RevolutionUtils`).
use super::rep_utils::{self, SurfaceEvalRepDesc};
use super::trimmed::make_unique;
use super::{Circle, Curve, GeomAbsShape, GeomError, Geometry, Line, M_PI, PRECISION_INFINITE};
use super::{Result, SurfResD1, SurfResD2, SurfResD3, Surface, SweptSurface, confusion, geometry_any, is_infinite};
use crate::gp::{self, Ax1, Ax2, Ax2d, Dir, GTrsf2d, Pnt, Trsf, XYZ};
use std::sync::Arc;

fn zero() -> gp::Vec {
    gp::Vec::new(0.0, 0.0, 0.0)
}

// ---------------------------------------------------------------------------------------------
// Geom_ExtrusionUtils

// theP.SetXYZ(theCurvePt.XYZ() + theV * theDir)
fn extrusion_point(curve_pt: &Pnt, v: f64, dir: &XYZ) -> Pnt {
    Pnt::from_xyz(curve_pt.xyz().added(&dir.multiplied(v)))
}

/// `Geom_SurfaceOfLinearExtrusion`.
#[derive(Debug, Clone)]
pub struct SurfaceOfLinearExtrusion {
    basis_curve: Arc<dyn Curve>,
    direction: Dir,
    smooth: GeomAbsShape,
    eval_rep: Option<SurfaceEvalRepDesc>,
}

impl SurfaceOfLinearExtrusion {
    /// `Geom_SurfaceOfLinearExtrusion(C, V)`; the basis curve is copied.
    pub fn new(c: Arc<dyn Curve>, v: Dir) -> Result<Self> {
        let smooth = c.continuity();
        Ok(SurfaceOfLinearExtrusion { basis_curve: Arc::from(c.copy_curve()), direction: v, smooth, eval_rep: None })
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
    /// `SetDirection`.
    pub fn set_direction(&mut self, v: Dir) {
        self.eval_rep = None;
        self.direction = v;
    }
    /// `SetBasisCurve`.
    pub fn set_basis_curve(&mut self, c: Arc<dyn Curve>) {
        self.eval_rep = None;
        self.smooth = c.continuity();
        self.basis_curve = Arc::from(c.copy_curve());
    }
}

impl Geometry for SurfaceOfLinearExtrusion {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.eval_rep = None;
        let _ = self.direction.transform(t);
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.transform(t);
        }
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.copy_extrusion())
    }
}

impl SurfaceOfLinearExtrusion {
    // Copy(): new SurfaceOfLinearExtrusion(basisCurve, direction) with the cloned evaluation representation
    fn copy_extrusion(&self) -> SurfaceOfLinearExtrusion {
        SurfaceOfLinearExtrusion {
            basis_curve: Arc::from(self.basis_curve.copy_curve()),
            direction: self.direction,
            smooth: self.basis_curve.continuity(),
            eval_rep: rep_utils::clone_surface_desc(&self.eval_rep),
        }
    }
}

impl Surface for SurfaceOfLinearExtrusion {
    fn copy_surface(&self) -> Box<dyn Surface> {
        Box::new(self.copy_extrusion())
    }
    fn u_reverse(&mut self) {
        self.eval_rep = None;
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.reverse();
        }
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        self.basis_curve.reversed_parameter(u)
    }
    fn v_reverse(&mut self) {
        self.eval_rep = None;
        self.direction.reverse();
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        -v
    }
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64) {
        let u = self.basis_curve.transformed_parameter(u, t);
        let mut v = v;
        if !is_infinite(v) {
            v *= t.scale_factor().abs();
        }
        (u, v)
    }
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d {
        let mut tv = GTrsf2d::new();
        let axis = Ax2d::new(gp::origin2d(), gp::dx2d());
        tv.set_affinity(&axis, t.scale_factor().abs());
        let mut tu = GTrsf2d::new();
        let axis = Ax2d::new(gp::origin2d(), gp::dy2d());
        tu.set_affinity(&axis, self.basis_curve.parametric_transformation(t));
        tu.multiplied(&tv).unwrap_or_else(|_| GTrsf2d::new())
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.basis_curve.first_parameter(), self.basis_curve.last_parameter(), -PRECISION_INFINITE, PRECISION_INFINITE)
    }
    fn is_u_closed(&self) -> bool {
        self.basis_curve.is_closed()
    }
    fn is_v_closed(&self) -> bool {
        false
    }
    fn is_u_periodic(&self) -> bool {
        self.basis_curve.is_periodic()
    }
    fn is_v_periodic(&self) -> bool {
        false
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Line::new(self.basis_curve.value(u)?, self.direction)))
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        let mut vdir = gp::Vec::from_dir(&self.direction);
        vdir.multiply(v);
        let mut c = self.basis_curve.copy_curve();
        c.translate(&vdir);
        Ok(c)
    }
    fn continuity(&self) -> GeomAbsShape {
        self.smooth
    }
    fn is_cn_u(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_curve.is_cn(n)
    }
    fn is_cn_v(&self, n: i32) -> Result<bool> {
        // OCCT does not check N here (the golden file lists n < 0 as unchecked, so it is an error)
        if n < 0 {
            return Err(GeomError::Range);
        }
        Ok(true)
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        if let Some(r) = rep_utils::try_eval_surface_d0(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let basis_d0 = self.basis_curve.eval_d0(u)?;
        Ok(extrusion_point(&basis_d0, v, self.direction.xyz()))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        if let Some(r) = rep_utils::try_eval_surface_d1(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let b = self.basis_curve.eval_d1(u)?;
        Ok(SurfResD1 {
            point: extrusion_point(&b.point, v, self.direction.xyz()),
            d1u: b.d1,
            d1v: gp::Vec::from_xyz(*self.direction.xyz()),
        })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        if let Some(r) = rep_utils::try_eval_surface_d2(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let b = self.basis_curve.eval_d2(u)?;
        Ok(SurfResD2 {
            point: extrusion_point(&b.point, v, self.direction.xyz()),
            d1u: b.d1,
            d1v: gp::Vec::from_xyz(*self.direction.xyz()),
            d2u: b.d2,
            d2v: zero(),
            d2uv: zero(),
        })
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        if let Some(r) = rep_utils::try_eval_surface_d3(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let b = self.basis_curve.eval_d3(u)?;
        Ok(SurfResD3 {
            point: extrusion_point(&b.point, v, self.direction.xyz()),
            d1u: b.d1,
            d1v: gp::Vec::from_xyz(*self.direction.xyz()),
            d2u: b.d2,
            d2v: zero(),
            d2uv: zero(),
            d3u: b.d3,
            d3v: zero(),
            d3uuv: zero(),
            d3uvv: zero(),
        })
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        if let Some(r) = rep_utils::try_eval_surface_dn(&self.eval_rep, u, v, nu, nv)? {
            return Ok(r);
        }
        if nv == 0 {
            // CalculateDN returns the derivative of the curve
            self.basis_curve.eval_dn(u, nu)
        } else if nu == 0 && nv == 1 {
            Ok(gp::Vec::from_xyz(*self.direction.xyz()))
        } else {
            Ok(zero())
        }
    }
}

impl SweptSurface for SurfaceOfLinearExtrusion {
    fn direction(&self) -> Dir {
        self.direction
    }
    fn basis_curve(&self) -> Arc<dyn Curve> {
        self.basis_curve.clone()
    }
}

// ---------------------------------------------------------------------------------------------
// Geom_RevolutionUtils

// aRotation.SetRotation(theAxis, theU)
fn rotation(axis: &Ax1, u: f64) -> Trsf {
    let mut t = Trsf::new();
    // a rotation never fails: the axis direction is a unit direction
    let _ = t.set_rotation(axis, u);
    t
}

fn rotated(v: &gp::Vec, t: &Trsf) -> gp::Vec {
    let mut r = *v;
    let _ = r.transform(t);
    r
}

fn square_confusion() -> f64 {
    confusion() * confusion()
}

// Geom_RevolutionUtils::CalculateD1/D2/D3: the derivatives of the surface before the rotation.
struct RevU {
    d1u: gp::Vec,
}

fn rev_d1u(curve_pt: &Pnt, axis: &Ax1) -> (XYZ, RevU) {
    let cq = curve_pt.xyz().subtracted(axis.location().xyz());
    let mut d1u = gp::Vec::from_xyz(axis.direction().xyz().crossed(&cq));
    if d1u.square_magnitude() < square_confusion() {
        d1u.set_coord(0.0, 0.0, 0.0);
    }
    (cq, RevU { d1u })
}

// Geom_RevolutionUtils::CalculateDN
fn revolution_dn(cur: &gp::Vec, u: f64, axis: &Ax1, der_u: i32) -> gp::Vec {
    let r = rotation(axis, u);
    let result = if der_u == 0 {
        *cur
    } else {
        let dir = axis.direction().xyz();
        if der_u % 4 == 1 {
            gp::Vec::from_xyz(dir.crossed(cur.xyz()))
        } else if der_u % 4 == 2 {
            gp::Vec::from_xyz(dir.multiplied(dir.dot(cur.xyz())).subtracted(cur.xyz()))
        } else if der_u % 4 == 3 {
            gp::Vec::from_xyz(dir.crossed(cur.xyz())).multiplied(-1.0)
        } else {
            gp::Vec::from_xyz(cur.xyz().subtracted(&dir.multiplied(dir.dot(cur.xyz()))))
        }
    };
    rotated(&result, &r)
}

/// `Geom_SurfaceOfRevolution`.
#[derive(Debug, Clone)]
pub struct SurfaceOfRevolution {
    basis_curve: Arc<dyn Curve>,
    direction: Dir,
    smooth: GeomAbsShape,
    loc: Pnt,
    eval_rep: Option<SurfaceEvalRepDesc>,
}

impl SurfaceOfRevolution {
    /// `Geom_SurfaceOfRevolution(C, A1)`; the basis curve is copied.
    pub fn new(c: Arc<dyn Curve>, a: Ax1) -> Result<Self> {
        let smooth = c.continuity();
        Ok(SurfaceOfRevolution {
            basis_curve: Arc::from(c.copy_curve()),
            direction: *a.direction(),
            smooth,
            loc: *a.location(),
            eval_rep: None,
        })
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
    /// `SetAxis`.
    pub fn set_axis(&mut self, a: Ax1) {
        self.eval_rep = None;
        self.direction = *a.direction();
        self.loc = *a.location();
    }
    /// `SetDirection`.
    pub fn set_direction(&mut self, v: Dir) {
        self.eval_rep = None;
        self.direction = v;
    }
    /// `SetBasisCurve`.
    pub fn set_basis_curve(&mut self, c: Arc<dyn Curve>) {
        self.eval_rep = None;
        self.basis_curve = Arc::from(c.copy_curve());
        self.smooth = c.continuity();
    }
    /// `SetLocation`.
    pub fn set_location(&mut self, p: Pnt) {
        self.eval_rep = None;
        self.loc = p;
    }
    /// `Axis`.
    pub fn axis(&self) -> Ax1 {
        Ax1::new(self.loc, self.direction)
    }
    /// `Location`.
    pub fn location(&self) -> Pnt {
        self.loc
    }
    /// `ReferencePlane` (always raises `Standard_NotImplemented`).
    pub fn reference_plane(&self) -> Result<Ax2> {
        Err(GeomError::NotImplemented)
    }
    // Copy(): new Geom_SurfaceOfRevolution(basisCurve, Axis()) with the cloned evaluation representation
    fn copy_revolution(&self) -> SurfaceOfRevolution {
        SurfaceOfRevolution {
            basis_curve: Arc::from(self.basis_curve.copy_curve()),
            direction: self.direction,
            smooth: self.basis_curve.continuity(),
            loc: self.loc,
            eval_rep: rep_utils::clone_surface_desc(&self.eval_rep),
        }
    }
}

impl Geometry for SurfaceOfRevolution {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.eval_rep = None;
        self.loc.transform(t);
        let _ = self.direction.transform(t);
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.transform(t);
        }
        if t.scale_factor() * t.h_vectorial_part().determinant() < 0.0 {
            self.u_reverse();
        }
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.copy_revolution())
    }
}

impl Surface for SurfaceOfRevolution {
    fn copy_surface(&self) -> Box<dyn Surface> {
        Box::new(self.copy_revolution())
    }
    fn u_reverse(&mut self) {
        self.eval_rep = None;
        self.direction.reverse();
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        2.0 * M_PI - u
    }
    fn v_reverse(&mut self) {
        self.eval_rep = None;
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.reverse();
        }
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        self.basis_curve.reversed_parameter(v)
    }
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64) {
        (u, self.basis_curve.transformed_parameter(v, t))
    }
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d {
        let mut t2 = GTrsf2d::new();
        let axis = Ax2d::new(gp::origin2d(), gp::dx2d());
        t2.set_affinity(&axis, self.basis_curve.parametric_transformation(t));
        t2
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, 2.0 * M_PI, self.basis_curve.first_parameter(), self.basis_curve.last_parameter())
    }
    fn is_u_closed(&self) -> bool {
        true
    }
    fn is_v_closed(&self) -> bool {
        self.basis_curve.is_closed()
    }
    fn is_u_periodic(&self) -> bool {
        true
    }
    fn is_v_periodic(&self) -> bool {
        self.basis_curve.is_periodic()
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        let mut c = self.basis_curve.copy_curve();
        let rot_axis = Ax1::new(self.loc, self.direction);
        c.rotate(&rot_axis, u);
        Ok(c)
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        let pc = self.basis_curve.value(v)?;
        let l1 = gp::Lin::new(self.loc, self.direction);
        let rad = l1.distance(&pc);
        let rep;
        if rad > gp::resolution() {
            let mut p = *pc.xyz();
            let mut c = XYZ::zero();
            c.set_linear_form_1_plus(
                p.subtracted(self.loc.xyz()).dot(self.direction.xyz()),
                self.direction.xyz(),
                self.loc.xyz(),
            );
            p = p.subtracted(&c);
            if p.modulus() > gp::resolution() {
                let d = Dir::from_xyz(p.normalized()?)?;
                rep = Ax2::new(Pnt::from_xyz(c), self.direction, &d)?;
            } else {
                rep = Ax2::from_direction(Pnt::from_xyz(c), self.direction);
            }
        } else {
            rep = Ax2::from_direction(pc, self.direction);
        }
        Ok(Box::new(Circle::new(rep, rad)?))
    }
    fn continuity(&self) -> GeomAbsShape {
        self.smooth
    }
    fn is_cn_u(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        Ok(true)
    }
    fn is_cn_v(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_curve.is_cn(n)
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        if let Some(r) = rep_utils::try_eval_surface_d0(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let basis_d0 = self.basis_curve.eval_d0(v)?;
        let axis = self.axis();
        let r = rotation(&axis, u);
        Ok(basis_d0.transformed(&r))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        if let Some(r) = rep_utils::try_eval_surface_d1(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let b = self.basis_curve.eval_d1(v)?;
        let axis = self.axis();
        let (_cq, ru) = rev_d1u(&b.point, &axis);
        let rot = rotation(&axis, u);
        Ok(SurfResD1 { point: b.point.transformed(&rot), d1u: rotated(&ru.d1u, &rot), d1v: rotated(&b.d1, &rot) })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        if let Some(r) = rep_utils::try_eval_surface_d2(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let b = self.basis_curve.eval_d2(v)?;
        let axis = self.axis();
        let (cq, ru) = rev_d1u(&b.point, &axis);
        let dir = axis.direction().xyz();
        let d2u = gp::Vec::from_xyz(dir.multiplied(dir.dot(&cq)).subtracted(&cq));
        let d2uv = gp::Vec::from_xyz(dir.crossed(b.d1.xyz()));
        let rot = rotation(&axis, u);
        Ok(SurfResD2 {
            point: b.point.transformed(&rot),
            d1u: rotated(&ru.d1u, &rot),
            d1v: rotated(&b.d1, &rot),
            d2u: rotated(&d2u, &rot),
            d2v: rotated(&b.d2, &rot),
            d2uv: rotated(&d2uv, &rot),
        })
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        if let Some(r) = rep_utils::try_eval_surface_d3(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let b = self.basis_curve.eval_d3(v)?;
        let axis = self.axis();
        let (cq, ru) = rev_d1u(&b.point, &axis);
        let dir = axis.direction().xyz();
        let d2u = gp::Vec::from_xyz(dir.multiplied(dir.dot(&cq)).subtracted(&cq));
        let d2uv = gp::Vec::from_xyz(dir.crossed(b.d1.xyz()));
        let d3u = ru.d1u.reversed();
        let d3uuv = gp::Vec::from_xyz(dir.multiplied(dir.dot(b.d1.xyz())).subtracted(b.d1.xyz()));
        let d3uvv = gp::Vec::from_xyz(dir.crossed(b.d2.xyz()));
        let rot = rotation(&axis, u);
        Ok(SurfResD3 {
            point: b.point.transformed(&rot),
            d1u: rotated(&ru.d1u, &rot),
            d1v: rotated(&b.d1, &rot),
            d2u: rotated(&d2u, &rot),
            d2v: rotated(&b.d2, &rot),
            d2uv: rotated(&d2uv, &rot),
            d3u: rotated(&d3u, &rot),
            d3v: rotated(&b.d3, &rot),
            d3uuv: rotated(&d3uuv, &rot),
            d3uvv: rotated(&d3uvv, &rot),
        })
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        if let Some(r) = rep_utils::try_eval_surface_dn(&self.eval_rep, u, v, nu, nv)? {
            return Ok(r);
        }
        let cur = if nu == 0 {
            self.basis_curve.eval_dn(v, nv)?
        } else if nv == 0 {
            let d0 = self.basis_curve.eval_d0(v)?;
            gp::Vec::from_xyz(d0.xyz().subtracted(self.loc.xyz()))
        } else {
            self.basis_curve.eval_dn(v, nv)?
        };
        Ok(revolution_dn(&cur, u, &self.axis(), nu))
    }
}

impl SweptSurface for SurfaceOfRevolution {
    fn direction(&self) -> Dir {
        self.direction
    }
    fn basis_curve(&self) -> Arc<dyn Curve> {
        self.basis_curve.clone()
    }
}
