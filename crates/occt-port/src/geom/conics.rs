//! `Geom_Circle`, `Geom_Ellipse`, `Geom_Hyperbola`, `Geom_Parabola`.
use super::{Conic, Curve, GeomAbsShape, GeomError, Geometry, M_PI, PRECISION_INFINITE};
use super::{ResD1, ResD2, ResD3, Result, conic_reverse, geometry_any, is_infinite};
use crate::elclib;
use crate::gp::{self, Ax1, Ax2, Circ, Elips, Hypr, Parab, Pnt, Trsf};

// `Geom_Conic`: the position accessors; the shared `Reverse`, `Continuity` and `IsCN` are in `curve_common!`.
macro_rules! conic_common {
    ($t:ty) => {
        impl Conic for $t {
            fn pos(&self) -> &Ax2 {
                &self.pos
            }
            fn pos_mut(&mut self) -> &mut Ax2 {
                &mut self.pos
            }
            fn eccentricity(&self) -> Result<f64> {
                self.eccentricity_impl()
            }
        }
    };
}

macro_rules! curve_common {
    () => {
        fn copy_curve(&self) -> Box<dyn Curve> {
            Box::new(self.clone())
        }
        fn reverse(&mut self) {
            conic_reverse(&mut self.pos);
        }
        fn continuity(&self) -> GeomAbsShape {
            GeomAbsShape::Cn
        }
        fn is_cn(&self, _n: i32) -> Result<bool> {
            Ok(true)
        }
    };
}

/// `Geom_Circle`.
#[derive(Debug, Clone)]
pub struct Circle {
    pos: Ax2,
    radius: f64,
}

impl Circle {
    /// `Geom_Circle(gp_Circ)`.
    pub fn from_circ(c: Circ) -> Self {
        Circle { pos: *c.position(), radius: c.radius() }
    }
    /// `Geom_Circle(A2, Radius)`.
    pub fn new(a: Ax2, radius: f64) -> Result<Self> {
        if radius < 0.0 {
            return Err(GeomError::Construction);
        }
        Ok(Circle { pos: a, radius })
    }
    /// `SetCirc`.
    pub fn set_circ(&mut self, c: Circ) {
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
    /// `Circ`.
    pub fn circ(&self) -> Circ {
        Circ::new(self.pos, self.radius).unwrap_or_default()
    }
    /// `Radius`.
    pub fn radius(&self) -> f64 {
        self.radius
    }
    fn eccentricity_impl(&self) -> Result<f64> {
        Ok(0.0)
    }
}

impl Geometry for Circle {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.clone())
    }
}

impl Curve for Circle {
    curve_common!();
    fn reversed_parameter(&self, u: f64) -> f64 {
        2.0 * M_PI - u
    }
    fn first_parameter(&self) -> f64 {
        0.0
    }
    fn last_parameter(&self) -> f64 {
        2.0 * M_PI
    }
    fn is_closed(&self) -> bool {
        true
    }
    fn is_periodic(&self) -> bool {
        true
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        Ok(elclib::circle_value(u, &self.pos, self.radius))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::circle_d1(u, &self.pos, self.radius);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::circle_d2(u, &self.pos, self.radius);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2, d3) = elclib::circle_d3(u, &self.pos, self.radius);
        Ok(ResD3 { point, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec> {
        if n < 1 {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(elclib::circle_dn(u, &self.pos, self.radius, n))
    }
}
conic_common!(Circle);

/// `Geom_Ellipse`.
#[derive(Debug, Clone)]
pub struct Ellipse {
    pos: Ax2,
    major_radius: f64,
    minor_radius: f64,
}

impl Ellipse {
    /// `Geom_Ellipse(gp_Elips)`.
    pub fn from_elips(e: Elips) -> Self {
        Ellipse { pos: *e.position(), major_radius: e.major_radius(), minor_radius: e.minor_radius() }
    }
    /// `Geom_Ellipse(A, MajorRadius, MinorRadius)`.
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self> {
        if major < minor || minor < 0.0 {
            return Err(GeomError::Construction);
        }
        Ok(Ellipse { pos: a, major_radius: major, minor_radius: minor })
    }
    /// `SetElips`.
    pub fn set_elips(&mut self, e: Elips) {
        self.major_radius = e.major_radius();
        self.minor_radius = e.minor_radius();
        self.pos = *e.position();
    }
    /// `SetMajorRadius`.
    pub fn set_major_radius(&mut self, r: f64) -> Result<()> {
        if r < self.minor_radius {
            return Err(GeomError::Construction);
        }
        self.major_radius = r;
        Ok(())
    }
    /// `SetMinorRadius`.
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 || self.major_radius < r {
            return Err(GeomError::Construction);
        }
        self.minor_radius = r;
        Ok(())
    }
    /// `Elips`.
    pub fn elips(&self) -> Elips {
        Elips::new(self.pos, self.major_radius, self.minor_radius).unwrap_or_default()
    }
    /// `Directrix1` (a circle is unchecked in OCCT: an error here).
    pub fn directrix1(&self) -> Result<Ax1> {
        Ok(self.elips().directrix1()?)
    }
    /// `Directrix2`.
    pub fn directrix2(&self) -> Result<Ax1> {
        Ok(self.elips().directrix2()?)
    }
    /// `Focal`.
    pub fn focal(&self) -> f64 {
        2.0 * (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt()
    }
    /// `Focus1`.
    pub fn focus1(&self) -> Pnt {
        let c = (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt();
        let (xp, yp, zp) = self.pos.location().coords();
        let (xd, yd, zd) = self.pos.x_direction().coords();
        Pnt::new(xp + c * xd, yp + c * yd, zp + c * zd)
    }
    /// `Focus2`.
    pub fn focus2(&self) -> Pnt {
        let c = (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt();
        let (xp, yp, zp) = self.pos.location().coords();
        let (xd, yd, zd) = self.pos.x_direction().coords();
        Pnt::new(xp - c * xd, yp - c * yd, zp - c * zd)
    }
    /// `MajorRadius`.
    pub fn major_radius(&self) -> f64 {
        self.major_radius
    }
    /// `MinorRadius`.
    pub fn minor_radius(&self) -> f64 {
        self.minor_radius
    }
    /// `Parameter`.
    pub fn parameter(&self) -> f64 {
        if self.major_radius == 0.0 { 0.0 } else { (self.minor_radius * self.minor_radius) / self.major_radius }
    }
    fn eccentricity_impl(&self) -> Result<f64> {
        if self.major_radius == 0.0 {
            Ok(0.0)
        } else {
            Ok((self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt()
                / self.major_radius)
        }
    }
}

impl Geometry for Ellipse {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.major_radius *= t.scale_factor().abs();
        self.minor_radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.clone())
    }
}

impl Curve for Ellipse {
    curve_common!();
    fn reversed_parameter(&self, u: f64) -> f64 {
        2.0 * M_PI - u
    }
    fn first_parameter(&self) -> f64 {
        0.0
    }
    fn last_parameter(&self) -> f64 {
        2.0 * M_PI
    }
    fn is_closed(&self) -> bool {
        true
    }
    fn is_periodic(&self) -> bool {
        true
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        Ok(elclib::ellipse_value(u, &self.pos, self.major_radius, self.minor_radius))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::ellipse_d1(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::ellipse_d2(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2, d3) = elclib::ellipse_d3(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD3 { point, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec> {
        if n < 1 {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(elclib::ellipse_dn(u, &self.pos, self.major_radius, self.minor_radius, n))
    }
}
conic_common!(Ellipse);

/// `Geom_Hyperbola`.
#[derive(Debug, Clone)]
pub struct Hyperbola {
    pos: Ax2,
    major_radius: f64,
    minor_radius: f64,
}

impl Hyperbola {
    /// `Geom_Hyperbola(gp_Hypr)`.
    pub fn from_hypr(h: Hypr) -> Self {
        Hyperbola { pos: *h.position(), major_radius: h.major_radius(), minor_radius: h.minor_radius() }
    }
    /// `Geom_Hyperbola(A, MajorRadius, MinorRadius)`.
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self> {
        if major < 0.0 || minor < 0.0 {
            return Err(GeomError::Construction);
        }
        Ok(Hyperbola { pos: a, major_radius: major, minor_radius: minor })
    }
    /// `SetHypr`.
    pub fn set_hypr(&mut self, h: Hypr) {
        self.major_radius = h.major_radius();
        self.minor_radius = h.minor_radius();
        self.pos = *h.position();
    }
    /// `SetMajorRadius`.
    pub fn set_major_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(GeomError::Construction);
        }
        self.major_radius = r;
        Ok(())
    }
    /// `SetMinorRadius`.
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(GeomError::Construction);
        }
        self.minor_radius = r;
        Ok(())
    }
    /// `Hypr`.
    pub fn hypr(&self) -> Hypr {
        Hypr::new(self.pos, self.major_radius, self.minor_radius).unwrap_or_default()
    }
    // The `gp_Hypr` functions with a check of the major radius that is compiled out in OCCT 8.0.1.
    fn check_major(&self) -> Result<()> {
        if self.major_radius <= gp::resolution() {
            return Err(GeomError::Domain);
        }
        Ok(())
    }
    /// `Asymptote1`.
    pub fn asymptote1(&self) -> Result<Ax1> {
        self.check_major()?;
        Ok(self.hypr().asymptote1()?)
    }
    /// `Asymptote2`.
    pub fn asymptote2(&self) -> Result<Ax1> {
        self.check_major()?;
        Ok(self.hypr().asymptote2()?)
    }
    /// `ConjugateBranch1`.
    pub fn conjugate_branch1(&self) -> Result<Hypr> {
        Ok(self.hypr().conjugate_branch1()?)
    }
    /// `ConjugateBranch2`.
    pub fn conjugate_branch2(&self) -> Result<Hypr> {
        Ok(self.hypr().conjugate_branch2()?)
    }
    /// `Directrix1`.
    pub fn directrix1(&self) -> Result<Ax1> {
        self.check_major()?;
        Ok(self.hypr().directrix1()?)
    }
    /// `Directrix2`.
    pub fn directrix2(&self) -> Result<Ax1> {
        self.check_major()?;
        Ok(self.hypr().directrix2()?)
    }
    /// `Focal`.
    pub fn focal(&self) -> f64 {
        2.0 * (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt()
    }
    /// `Focus1`.
    pub fn focus1(&self) -> Pnt {
        let c = (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt();
        let (xp, yp, zp) = self.pos.location().coords();
        let (xd, yd, zd) = self.pos.x_direction().coords();
        Pnt::new(xp + c * xd, yp + c * yd, zp + c * zd)
    }
    /// `Focus2`.
    pub fn focus2(&self) -> Pnt {
        let c = (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt();
        let (xp, yp, zp) = self.pos.location().coords();
        let (xd, yd, zd) = self.pos.x_direction().coords();
        Pnt::new(xp - c * xd, yp - c * yd, zp - c * zd)
    }
    /// `MajorRadius`.
    pub fn major_radius(&self) -> f64 {
        self.major_radius
    }
    /// `MinorRadius`.
    pub fn minor_radius(&self) -> f64 {
        self.minor_radius
    }
    /// `OtherBranch`.
    pub fn other_branch(&self) -> Result<Hypr> {
        Ok(self.hypr().other_branch()?)
    }
    /// `Parameter`.
    pub fn parameter(&self) -> Result<f64> {
        self.check_major()?;
        Ok((self.minor_radius * self.minor_radius) / self.major_radius)
    }
    fn eccentricity_impl(&self) -> Result<f64> {
        self.check_major()?;
        Ok((self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt() / self.major_radius)
    }
}

impl Geometry for Hyperbola {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.major_radius *= t.scale_factor().abs();
        self.minor_radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.clone())
    }
}

impl Curve for Hyperbola {
    curve_common!();
    fn reversed_parameter(&self, u: f64) -> f64 {
        -u
    }
    fn first_parameter(&self) -> f64 {
        -PRECISION_INFINITE
    }
    fn last_parameter(&self) -> f64 {
        PRECISION_INFINITE
    }
    fn is_closed(&self) -> bool {
        false
    }
    fn is_periodic(&self) -> bool {
        false
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        Ok(elclib::hyperbola_value(u, &self.pos, self.major_radius, self.minor_radius))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::hyperbola_d1(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::hyperbola_d2(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2, d3) = elclib::hyperbola_d3(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD3 { point, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec> {
        if n < 1 {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(elclib::hyperbola_dn(u, &self.pos, self.major_radius, self.minor_radius, n))
    }
}
conic_common!(Hyperbola);

/// `Geom_Parabola`.
#[derive(Debug, Clone)]
pub struct Parabola {
    pos: Ax2,
    focal_length: f64,
}

impl Parabola {
    /// `Geom_Parabola(gp_Parab)`.
    pub fn from_parab(p: Parab) -> Self {
        Parabola { pos: *p.position(), focal_length: p.focal() }
    }
    /// `Geom_Parabola(A2, Focal)`.
    pub fn new(a: Ax2, focal: f64) -> Result<Self> {
        if focal < 0.0 {
            return Err(GeomError::Construction);
        }
        Ok(Parabola { pos: a, focal_length: focal })
    }
    /// `Geom_Parabola(D, F)`.
    pub fn from_directrix_focus(d: Ax1, f: Pnt) -> Result<Self> {
        let prb = Parab::from_directrix_focus(&d, &f)?;
        Ok(Parabola { pos: *prb.position(), focal_length: prb.focal() })
    }
    /// `SetFocal`.
    pub fn set_focal(&mut self, f: f64) -> Result<()> {
        if f < 0.0 {
            return Err(GeomError::Construction);
        }
        self.focal_length = f;
        Ok(())
    }
    /// `SetParab`.
    pub fn set_parab(&mut self, p: Parab) {
        self.focal_length = p.focal();
        self.pos = *p.position();
    }
    /// `Parab`.
    pub fn parab(&self) -> Parab {
        Parab::new(self.pos, self.focal_length).unwrap_or_default()
    }
    /// `Directrix`.
    pub fn directrix(&self) -> Ax1 {
        self.parab().directrix()
    }
    /// `Focus`.
    pub fn focus(&self) -> Pnt {
        let (xp, yp, zp) = self.pos.location().coords();
        let (xd, yd, zd) = self.pos.x_direction().coords();
        Pnt::new(xp + self.focal_length * xd, yp + self.focal_length * yd, zp + self.focal_length * zd)
    }
    /// `Focal`.
    pub fn focal(&self) -> f64 {
        self.focal_length
    }
    /// `Parameter`.
    pub fn parameter(&self) -> f64 {
        2.0 * self.focal_length
    }
    fn eccentricity_impl(&self) -> Result<f64> {
        Ok(1.0)
    }
}

impl Geometry for Parabola {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.focal_length *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.clone())
    }
}

impl Curve for Parabola {
    curve_common!();
    fn reversed_parameter(&self, u: f64) -> f64 {
        -u
    }
    fn transformed_parameter(&self, u: f64, t: &Trsf) -> f64 {
        if is_infinite(u) {
            return u;
        }
        u * t.scale_factor().abs()
    }
    fn parametric_transformation(&self, t: &Trsf) -> f64 {
        t.scale_factor().abs()
    }
    fn first_parameter(&self) -> f64 {
        -PRECISION_INFINITE
    }
    fn last_parameter(&self) -> f64 {
        PRECISION_INFINITE
    }
    fn is_closed(&self) -> bool {
        false
    }
    fn is_periodic(&self) -> bool {
        false
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        Ok(elclib::parabola_value(u, &self.pos, self.focal_length))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::parabola_d1(u, &self.pos, self.focal_length);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::parabola_d2(u, &self.pos, self.focal_length);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2) = elclib::parabola_d2(u, &self.pos, self.focal_length);
        Ok(ResD3 { point, d1, d2, d3: gp::Vec::new(0.0, 0.0, 0.0) })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec> {
        if n < 1 {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(elclib::parabola_dn(u, &self.pos, self.focal_length, n))
    }
}
conic_common!(Parabola);
