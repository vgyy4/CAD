//! `Geom_Plane`, `Geom_CylindricalSurface`, `Geom_ConicalSurface`, `Geom_SphericalSurface`, `Geom_ToroidalSurface`.
use super::{Circle, Curve, ElementarySurface, GeomAbsShape, GeomError, Geometry, Line, M_PI, PRECISION_INFINITE};
use super::{Result, SurfResD1, SurfResD2, SurfResD3, Surface, TrimmedCurve, geometry_any, is_infinite};
use crate::elslib;
use crate::gp::{self, Ax2d, Ax3, Cone, Cylinder, Dir, GTrsf2d, Pln, Pnt, Sphere, Torus, Trsf, Trsf2d};
use std::sync::Arc;

fn zero() -> gp::Vec {
    gp::Vec::new(0.0, 0.0, 0.0)
}

// ElSLib D2 / D3 tuples to the result records
fn res_d2(t: elslib::D2Result) -> SurfResD2 {
    SurfResD2 { point: t.0, d1u: t.1, d1v: t.2, d2u: t.3, d2v: t.4, d2uv: t.5 }
}
fn res_d3(t: elslib::D3Result) -> SurfResD3 {
    SurfResD3 {
        point: t.0,
        d1u: t.1,
        d1v: t.2,
        d2u: t.3,
        d2v: t.4,
        d2uv: t.5,
        d3u: t.6,
        d3v: t.7,
        d3uuv: t.8,
        d3uvv: t.9,
    }
}

// `Geom_ElementarySurface`: the position accessors and the shared `UReverse`, `VReverse`, `Continuity`, `IsCNu`, `IsCNv`.
macro_rules! elementary_common {
    ($t:ty) => {
        impl ElementarySurface for $t {
            fn pos(&self) -> &Ax3 {
                &self.pos
            }
            fn pos_mut(&mut self) -> &mut Ax3 {
                &mut self.pos
            }
        }
    };
}

macro_rules! surface_common {
    () => {
        fn copy_surface(&self) -> Box<dyn Surface> {
            Box::new(self.clone())
        }
        fn continuity(&self) -> GeomAbsShape {
            GeomAbsShape::Cn
        }
        fn is_cn_u(&self, _n: i32) -> Result<bool> {
            Ok(true)
        }
        fn is_cn_v(&self, _n: i32) -> Result<bool> {
            Ok(true)
        }
    };
}

macro_rules! geometry_common {
    ($t:ty) => {
        impl Geometry for $t {
            geometry_any!();
            fn copy_geometry(&self) -> Box<dyn Geometry> {
                Box::new(self.clone())
            }
            fn transform(&mut self, t: &Trsf) {
                self.transform_impl(t);
            }
        }
    };
}

// The cylinder and cone `TransformParameters` and `ParametricTransformation`.
fn affinity(t: &Trsf) -> GTrsf2d {
    let mut t2 = GTrsf2d::new();
    let axis = Ax2d::new(gp::origin2d(), gp::dx2d());
    t2.set_affinity(&axis, t.scale_factor().abs());
    t2
}

fn coefficients10(c: (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64)) -> [f64; 10] {
    [c.0, c.1, c.2, c.3, c.4, c.5, c.6, c.7, c.8, c.9]
}

// ---------------------------------------------------------------------------------------------
/// `Geom_Plane`.
#[derive(Debug, Clone)]
pub struct Plane {
    pos: Ax3,
}

impl Plane {
    /// `Geom_Plane(gp_Ax3)`.
    pub fn from_ax3(a: Ax3) -> Self {
        Plane { pos: a }
    }
    /// `Geom_Plane(gp_Pln)`.
    pub fn from_pln(p: Pln) -> Self {
        Plane { pos: *p.position() }
    }
    /// `Geom_Plane(P, V)`.
    pub fn from_point_normal(p: Pnt, v: Dir) -> Self {
        let pl = Pln::from_point_normal(p, v);
        Plane { pos: *pl.position() }
    }
    /// `Geom_Plane(A, B, C, D)`.
    pub fn from_coefficients(a: f64, b: f64, c: f64, d: f64) -> Result<Self> {
        let pl = Pln::from_coefficients(a, b, c, d)?;
        Ok(Plane { pos: *pl.position() })
    }
    /// `SetPln`.
    pub fn set_pln(&mut self, p: Pln) {
        self.pos = *p.position();
    }
    /// `Pln`.
    pub fn pln(&self) -> Pln {
        Pln::new(self.pos)
    }
    /// `Coefficients`.
    pub fn coefficients(&self) -> (f64, f64, f64, f64) {
        Pln::new(self.pos).coefficients()
    }
    fn transform_impl(&mut self, t: &Trsf) {
        self.pos.transform(t);
    }
}
geometry_common!(Plane);
elementary_common!(Plane);

impl Surface for Plane {
    surface_common!();
    fn u_reverse(&mut self) {
        self.pos.x_reverse();
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        -u
    }
    fn v_reverse(&mut self) {
        self.pos.y_reverse();
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        -v
    }
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64) {
        let mut u = u;
        let mut v = v;
        if !is_infinite(u) {
            u *= t.scale_factor().abs();
        }
        if !is_infinite(v) {
            v *= t.scale_factor().abs();
        }
        (u, v)
    }
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d {
        let mut t2 = Trsf2d::new();
        t2.set_scale(&gp::origin2d(), t.scale_factor().abs());
        GTrsf2d::from_trsf2d(&t2)
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (-PRECISION_INFINITE, PRECISION_INFINITE, -PRECISION_INFINITE, PRECISION_INFINITE)
    }
    fn is_u_closed(&self) -> bool {
        false
    }
    fn is_v_closed(&self) -> bool {
        false
    }
    fn is_u_periodic(&self) -> bool {
        false
    }
    fn is_v_periodic(&self) -> bool {
        false
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Line::from_lin(elslib::plane_uiso(&self.pos, u))))
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Line::from_lin(elslib::plane_viso(&self.pos, v))))
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        Ok(elslib::plane_value(u, v, &self.pos))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        let (point, d1u, d1v) = elslib::plane_d1(u, v, &self.pos);
        Ok(SurfResD1 { point, d1u, d1v })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        let (point, d1u, d1v) = elslib::plane_d1(u, v, &self.pos);
        Ok(SurfResD2 { point, d1u, d1v, d2u: zero(), d2v: zero(), d2uv: zero() })
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        let (point, d1u, d1v) = elslib::plane_d1(u, v, &self.pos);
        Ok(SurfResD3 {
            point,
            d1u,
            d1v,
            d2u: zero(),
            d2v: zero(),
            d2uv: zero(),
            d3u: zero(),
            d3v: zero(),
            d3uuv: zero(),
            d3uvv: zero(),
        })
    }
    fn eval_dn(&self, _u: f64, _v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        if nu == 0 && nv == 1 {
            return Ok(gp::Vec::from_dir(self.pos.y_direction()));
        } else if nu == 1 && nv == 0 {
            return Ok(gp::Vec::from_dir(self.pos.x_direction()));
        }
        Ok(zero())
    }
}

// ---------------------------------------------------------------------------------------------
/// `Geom_CylindricalSurface`.
#[derive(Debug, Clone)]
pub struct CylindricalSurface {
    pos: Ax3,
    radius: f64,
}

impl CylindricalSurface {
    /// `Geom_CylindricalSurface(A3, R)`.
    pub fn new(a: Ax3, radius: f64) -> Result<Self> {
        if radius < 0.0 {
            return Err(GeomError::Construction);
        }
        Ok(CylindricalSurface { pos: a, radius })
    }
    /// `Geom_CylindricalSurface(gp_Cylinder)`.
    pub fn from_cylinder(c: Cylinder) -> Self {
        CylindricalSurface { pos: *c.position(), radius: c.radius() }
    }
    /// `SetCylinder`.
    pub fn set_cylinder(&mut self, c: Cylinder) {
        self.radius = c.radius();
        self.pos = *c.position();
    }
    /// `SetRadius`.
    pub fn set_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(GeomError::Construction);
        }
        self.radius = r;
        Ok(())
    }
    /// `Cylinder`.
    pub fn cylinder(&self) -> Cylinder {
        Cylinder::new(self.pos, self.radius).unwrap_or_default()
    }
    /// `Coefficients`.
    pub fn coefficients(&self) -> [f64; 10] {
        coefficients10(self.cylinder().coefficients())
    }
    /// `Radius`.
    pub fn radius(&self) -> f64 {
        self.radius
    }
    fn transform_impl(&mut self, t: &Trsf) {
        self.radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
}
geometry_common!(CylindricalSurface);
elementary_common!(CylindricalSurface);

impl Surface for CylindricalSurface {
    surface_common!();
    fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        2.0 * M_PI - u
    }
    fn v_reverse(&mut self) {
        self.pos.z_reverse();
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        -v
    }
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64) {
        let mut v = v;
        if !is_infinite(v) {
            v *= t.scale_factor().abs();
        }
        (u, v)
    }
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d {
        affinity(t)
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, 2.0 * M_PI, -PRECISION_INFINITE, PRECISION_INFINITE)
    }
    fn is_u_closed(&self) -> bool {
        true
    }
    fn is_v_closed(&self) -> bool {
        false
    }
    fn is_u_periodic(&self) -> bool {
        true
    }
    fn is_v_periodic(&self) -> bool {
        false
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Line::from_lin(elslib::cylinder_uiso(&self.pos, self.radius, u))))
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Circle::from_circ(elslib::cylinder_viso(&self.pos, self.radius, v))))
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        Ok(elslib::cylinder_d0(u, v, &self.pos, self.radius))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        let (point, d1u, d1v) = elslib::cylinder_d1(u, v, &self.pos, self.radius);
        Ok(SurfResD1 { point, d1u, d1v })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        Ok(res_d2(elslib::cylinder_d2(u, v, &self.pos, self.radius)))
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        Ok(res_d3(elslib::cylinder_d3(u, v, &self.pos, self.radius)))
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        if nv > 1 { Ok(zero()) } else { Ok(elslib::cylinder_dn(u, v, &self.pos, self.radius, nu, nv)) }
    }
}

// ---------------------------------------------------------------------------------------------
/// `Geom_ConicalSurface`.
#[derive(Debug, Clone)]
pub struct ConicalSurface {
    pos: Ax3,
    radius: f64,
    semi_angle: f64,
}

impl ConicalSurface {
    /// `Geom_ConicalSurface(A3, Ang, R)`.
    pub fn new(a: Ax3, ang: f64, radius: f64) -> Result<Self> {
        if radius < 0.0 || ang.abs() <= gp::resolution() || ang.abs() >= M_PI / 2.0 - gp::resolution() {
            return Err(GeomError::Construction);
        }
        Ok(ConicalSurface { pos: a, radius, semi_angle: ang })
    }
    /// `Geom_ConicalSurface(gp_Cone)`.
    pub fn from_cone(c: Cone) -> Self {
        ConicalSurface { pos: *c.position(), radius: c.ref_radius(), semi_angle: c.semi_angle() }
    }
    /// `SetCone`.
    pub fn set_cone(&mut self, c: Cone) {
        self.radius = c.ref_radius();
        self.semi_angle = c.semi_angle();
        self.pos = *c.position();
    }
    /// `SetRadius`.
    pub fn set_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(GeomError::Construction);
        }
        self.radius = r;
        Ok(())
    }
    /// `SetSemiAngle`.
    pub fn set_semi_angle(&mut self, ang: f64) -> Result<()> {
        if ang.abs() <= gp::resolution() || ang.abs() >= M_PI / 2.0 - gp::resolution() {
            return Err(GeomError::Construction);
        }
        self.semi_angle = ang;
        Ok(())
    }
    /// `Cone`.
    pub fn cone(&self) -> Cone {
        Cone::new(self.pos, self.semi_angle, self.radius).unwrap_or_default()
    }
    /// `Apex`.
    pub fn apex(&self) -> Pnt {
        let mut coord = *self.pos.direction().xyz();
        coord.multiply(-self.radius / libm::tan(self.semi_angle));
        coord.add(self.pos.location().xyz());
        Pnt::from_xyz(coord)
    }
    /// `Coefficients`.
    pub fn coefficients(&self) -> [f64; 10] {
        // Geom_ConicalSurface::Coefficients (not gp_Cone's: C1..C3 and D differ)
        let mut t = Trsf::new();
        t.set_transformation(&self.pos);
        let tv = |r: i32, c: i32| t.value(r, c).unwrap_or(0.0);
        let k_ang = libm::tan(self.semi_angle);
        let (t11, t12, t13, t14) = (tv(1, 1), tv(1, 2), tv(1, 3), tv(1, 4));
        let (t21, t22, t23, t24) = (tv(2, 1), tv(2, 2), tv(2, 3), tv(2, 4));
        let t31 = tv(3, 1) * k_ang;
        let t32 = tv(3, 2) * k_ang;
        let t33 = tv(3, 3) * k_ang;
        let t34 = tv(3, 4) * k_ang;
        let r = self.radius;
        [
            t11 * t11 + t21 * t21 - t31 * t31,
            t12 * t12 + t22 * t22 - t32 * t32,
            t13 * t13 + t23 * t23 - t33 * t33,
            t11 * t12 + t21 * t22 - t31 * t32,
            t11 * t13 + t21 * t23 - t31 * t33,
            t12 * t13 + t22 * t23 - t32 * t33,
            t11 * t14 + t21 * t24 + r * t31,
            t12 * t14 + t22 * t24 + r * t32,
            t13 * t14 + t23 * t24 + r * t33,
            t14 * t14 + t24 * t24 - r * r - t34 * t34 + 2.0 * r * t34,
        ]
    }
    /// `RefRadius`.
    pub fn ref_radius(&self) -> f64 {
        self.radius
    }
    /// `SemiAngle`.
    pub fn semi_angle(&self) -> f64 {
        self.semi_angle
    }
    fn transform_impl(&mut self, t: &Trsf) {
        self.radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
}
geometry_common!(ConicalSurface);
elementary_common!(ConicalSurface);

impl Surface for ConicalSurface {
    surface_common!();
    fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        2.0 * M_PI - u
    }
    fn v_reverse(&mut self) {
        self.semi_angle = -self.semi_angle;
        self.pos.z_reverse();
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        -v
    }
    fn transform_parameters(&self, u: f64, v: f64, t: &Trsf) -> (f64, f64) {
        let mut v = v;
        if !is_infinite(v) {
            v *= t.scale_factor().abs();
        }
        (u, v)
    }
    fn parametric_transformation(&self, t: &Trsf) -> GTrsf2d {
        affinity(t)
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, 2.0 * M_PI, -PRECISION_INFINITE, PRECISION_INFINITE)
    }
    fn is_u_closed(&self) -> bool {
        true
    }
    fn is_v_closed(&self) -> bool {
        false
    }
    fn is_u_periodic(&self) -> bool {
        true
    }
    fn is_v_periodic(&self) -> bool {
        false
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Line::from_lin(elslib::cone_uiso(&self.pos, self.radius, self.semi_angle, u))))
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Circle::from_circ(elslib::cone_viso(&self.pos, self.radius, self.semi_angle, v))))
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        Ok(elslib::cone_value(u, v, &self.pos, self.radius, self.semi_angle))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        let (point, d1u, d1v) = elslib::cone_d1(u, v, &self.pos, self.radius, self.semi_angle);
        Ok(SurfResD1 { point, d1u, d1v })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        Ok(res_d2(elslib::cone_d2(u, v, &self.pos, self.radius, self.semi_angle)))
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        Ok(res_d3(elslib::cone_d3(u, v, &self.pos, self.radius, self.semi_angle)))
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        if nv > 1 { Ok(zero()) } else { Ok(elslib::cone_dn(u, v, &self.pos, self.radius, self.semi_angle, nu, nv)) }
    }
}

// ---------------------------------------------------------------------------------------------
/// `Geom_SphericalSurface`.
#[derive(Debug, Clone)]
pub struct SphericalSurface {
    pos: Ax3,
    radius: f64,
}

impl SphericalSurface {
    /// `Geom_SphericalSurface(A, R)`.
    pub fn new(a: Ax3, radius: f64) -> Result<Self> {
        if radius < 0.0 {
            return Err(GeomError::Construction);
        }
        Ok(SphericalSurface { pos: a, radius })
    }
    /// `Geom_SphericalSurface(gp_Sphere)`.
    pub fn from_sphere(s: Sphere) -> Self {
        SphericalSurface { pos: *s.position(), radius: s.radius() }
    }
    /// `SetRadius`.
    pub fn set_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(GeomError::Construction);
        }
        self.radius = r;
        Ok(())
    }
    /// `SetSphere`.
    pub fn set_sphere(&mut self, s: Sphere) {
        self.radius = s.radius();
        self.pos = *s.position();
    }
    /// `Sphere`.
    pub fn sphere(&self) -> Sphere {
        Sphere::new(self.pos, self.radius).unwrap_or_default()
    }
    /// `Area`.
    pub fn area(&self) -> f64 {
        4.0 * M_PI * self.radius * self.radius
    }
    /// `Coefficients`.
    pub fn coefficients(&self) -> [f64; 10] {
        coefficients10(self.sphere().coefficients())
    }
    /// `Radius`.
    pub fn radius(&self) -> f64 {
        self.radius
    }
    /// `Volume`.
    pub fn volume(&self) -> f64 {
        (4.0 * M_PI * self.radius * self.radius * self.radius) / 3.0
    }
    fn transform_impl(&mut self, t: &Trsf) {
        self.radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
}
geometry_common!(SphericalSurface);
elementary_common!(SphericalSurface);

impl Surface for SphericalSurface {
    surface_common!();
    fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        2.0 * M_PI - u
    }
    fn v_reverse(&mut self) {
        self.pos.z_reverse();
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        -v
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, M_PI * 2.0, -M_PI / 2.0, M_PI / 2.0)
    }
    fn is_u_closed(&self) -> bool {
        true
    }
    fn is_v_closed(&self) -> bool {
        false
    }
    fn is_u_periodic(&self) -> bool {
        true
    }
    fn is_v_periodic(&self) -> bool {
        false
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        let gc = Circle::from_circ(elslib::sphere_uiso(&self.pos, self.radius, u));
        Ok(Box::new(TrimmedCurve::new(Arc::new(gc), -M_PI / 2.0, M_PI / 2.0, true, true)?))
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Circle::from_circ(elslib::sphere_viso(&self.pos, self.radius, v))))
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        Ok(elslib::sphere_d0(u, v, &self.pos, self.radius))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        let (point, d1u, d1v) = elslib::sphere_d1(u, v, &self.pos, self.radius);
        Ok(SurfResD1 { point, d1u, d1v })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        Ok(res_d2(elslib::sphere_d2(u, v, &self.pos, self.radius)))
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        Ok(res_d3(elslib::sphere_d3(u, v, &self.pos, self.radius)))
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(elslib::sphere_dn(u, v, &self.pos, self.radius, nu, nv))
    }
}

// ---------------------------------------------------------------------------------------------
/// `Geom_ToroidalSurface`.
#[derive(Debug, Clone)]
pub struct ToroidalSurface {
    pos: Ax3,
    major_radius: f64,
    minor_radius: f64,
}

impl ToroidalSurface {
    /// `Geom_ToroidalSurface(A3, MajorRadius, MinorRadius)`.
    pub fn new(a: Ax3, major: f64, minor: f64) -> Result<Self> {
        if minor < 0.0 || major < 0.0 {
            return Err(GeomError::Construction);
        }
        Ok(ToroidalSurface { pos: a, major_radius: major, minor_radius: minor })
    }
    /// `Geom_ToroidalSurface(gp_Torus)`.
    pub fn from_torus(t: Torus) -> Self {
        ToroidalSurface { pos: *t.position(), major_radius: t.major_radius(), minor_radius: t.minor_radius() }
    }
    /// `SetMajorRadius`.
    pub fn set_major_radius(&mut self, r: f64) -> Result<()> {
        if r - self.minor_radius <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        self.major_radius = r;
        Ok(())
    }
    /// `SetMinorRadius`.
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 || self.major_radius - r <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        self.minor_radius = r;
        Ok(())
    }
    /// `SetTorus`.
    pub fn set_torus(&mut self, t: Torus) {
        self.minor_radius = t.minor_radius();
        self.major_radius = t.major_radius();
        self.pos = *t.position();
    }
    /// `Torus`.
    pub fn torus(&self) -> Torus {
        Torus::new(self.pos, self.major_radius, self.minor_radius).unwrap_or_default()
    }
    /// `Area`.
    pub fn area(&self) -> f64 {
        4.0 * M_PI * M_PI * self.minor_radius * self.major_radius
    }
    /// `Coefficients`.
    pub fn coefficients(&self) -> [f64; 35] {
        self.torus().coefficients()
    }
    /// `MajorRadius`.
    pub fn major_radius(&self) -> f64 {
        self.major_radius
    }
    /// `MinorRadius`.
    pub fn minor_radius(&self) -> f64 {
        self.minor_radius
    }
    /// `Volume`.
    pub fn volume(&self) -> f64 {
        (M_PI * self.minor_radius * self.minor_radius) * (2.0 * M_PI * self.major_radius)
    }
    fn transform_impl(&mut self, t: &Trsf) {
        self.major_radius *= t.scale_factor().abs();
        self.minor_radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
}
geometry_common!(ToroidalSurface);
elementary_common!(ToroidalSurface);

impl Surface for ToroidalSurface {
    surface_common!();
    fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        2.0 * M_PI - u
    }
    fn v_reverse(&mut self) {
        self.pos.z_reverse();
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        2.0 * M_PI - v
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, 2.0 * M_PI, 0.0, 2.0 * M_PI)
    }
    fn is_u_closed(&self) -> bool {
        true
    }
    fn is_v_closed(&self) -> bool {
        true
    }
    fn is_u_periodic(&self) -> bool {
        true
    }
    fn is_v_periodic(&self) -> bool {
        true
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Circle::from_circ(elslib::torus_uiso(&self.pos, self.major_radius, self.minor_radius, u))))
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        Ok(Box::new(Circle::from_circ(elslib::torus_viso(&self.pos, self.major_radius, self.minor_radius, v))))
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        Ok(elslib::torus_d0(u, v, &self.pos, self.major_radius, self.minor_radius))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        let (point, d1u, d1v) = elslib::torus_d1(u, v, &self.pos, self.major_radius, self.minor_radius);
        Ok(SurfResD1 { point, d1u, d1v })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        Ok(res_d2(elslib::torus_d2(u, v, &self.pos, self.major_radius, self.minor_radius)))
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        Ok(res_d3(elslib::torus_d3(u, v, &self.pos, self.major_radius, self.minor_radius)))
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(elslib::torus_dn(u, v, &self.pos, self.major_radius, self.minor_radius, nu, nv))
    }
}
