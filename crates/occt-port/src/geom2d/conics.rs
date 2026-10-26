//! `Geom2d_Circle`, `Geom2d_Ellipse`, `Geom2d_Hyperbola`, `Geom2d_Parabola`.
use super::{Conic2d, Curve2d, Geom2dError, GeomAbsShape, Geometry2d, M_PI, PRECISION_INFINITE};
use super::{ResD1, ResD2, ResD3, Result, conic_reverse, geometry_any, is_infinite};
use crate::elclib;
use crate::gp::{self, Ax2d, Ax22d, Circ2d, Elips2d, Hypr2d, Parab2d, Pnt2d, Trsf2d, Vec2d, XY};

// The members every conic shares: the position accessors, the shared `Reverse`, `Continuity` and `IsCN`.
macro_rules! conic_common {
    ($t:ty) => {
        impl Conic2d for $t {
            fn pos(&self) -> &Ax22d {
                &self.pos
            }
            fn pos_mut(&mut self) -> &mut Ax22d {
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
        fn copy_curve(&self) -> Box<dyn Curve2d> {
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

/// `Geom2d_Circle`.
#[derive(Debug, Clone)]
pub struct Circle {
    pos: Ax22d,
    radius: f64,
}

impl Circle {
    /// `Geom2d_Circle(gp_Circ2d)`.
    pub fn from_circ2d(c: Circ2d) -> Self {
        Circle { pos: *c.axis(), radius: c.radius() }
    }
    /// `Geom2d_Circle(gp_Ax2d, Radius, Sense)`.
    pub fn new(a: Ax2d, radius: f64, sense: bool) -> Result<Self> {
        if radius < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Circle { pos: Ax22d::from_ax2d(a, sense), radius })
    }
    /// `Geom2d_Circle(gp_Ax22d, Radius)`.
    pub fn from_ax22d(a: Ax22d, radius: f64) -> Result<Self> {
        if radius < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Circle { pos: a, radius })
    }
    /// `SetCirc2d`.
    pub fn set_circ2d(&mut self, c: Circ2d) {
        self.radius = c.radius();
        self.pos = *c.axis();
    }
    /// `SetRadius`.
    pub fn set_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(Geom2dError::Construction);
        }
        self.radius = r;
        Ok(())
    }
    /// `Circ2d`.
    pub fn circ2d(&self) -> Circ2d {
        Circ2d::from_ax22d(self.pos, self.radius).unwrap_or_default()
    }
    /// `Radius`.
    pub fn radius(&self) -> f64 {
        self.radius
    }
    fn eccentricity_impl(&self) -> Result<f64> {
        Ok(0.0)
    }
}

impl Geometry2d for Circle {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(self.clone())
    }
}

impl Curve2d for Circle {
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
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        Ok(elclib::circle2d_value(u, &self.pos, self.radius))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::circle2d_d1(u, &self.pos, self.radius);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::circle2d_d2(u, &self.pos, self.radius);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2, d3) = elclib::circle2d_d3(u, &self.pos, self.radius);
        Ok(ResD3 { point, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        if n < 1 {
            return Err(Geom2dError::UndefinedDerivative);
        }
        Ok(elclib::circle2d_dn(u, &self.pos, self.radius, n))
    }
}
conic_common!(Circle);

/// `Geom2d_Ellipse`.
#[derive(Debug, Clone)]
pub struct Ellipse {
    pos: Ax22d,
    major_radius: f64,
    minor_radius: f64,
}

impl Ellipse {
    /// `Geom2d_Ellipse(gp_Elips2d)`.
    pub fn from_elips2d(e: Elips2d) -> Self {
        Ellipse { pos: *e.axis(), major_radius: e.major_radius(), minor_radius: e.minor_radius() }
    }
    /// `Geom2d_Ellipse(MajorAxis, MajorRadius, MinorRadius, Sense)`.
    pub fn new(major_axis: Ax2d, major: f64, minor: f64, sense: bool) -> Result<Self> {
        if major < minor || minor < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Ellipse { pos: Ax22d::from_ax2d(major_axis, sense), major_radius: major, minor_radius: minor })
    }
    /// `Geom2d_Ellipse(gp_Ax22d, MajorRadius, MinorRadius)`.
    pub fn from_ax22d(a: Ax22d, major: f64, minor: f64) -> Result<Self> {
        if major < minor || minor < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Ellipse { pos: a, major_radius: major, minor_radius: minor })
    }
    /// `SetElips2d`.
    pub fn set_elips2d(&mut self, e: Elips2d) {
        self.major_radius = e.major_radius();
        self.minor_radius = e.minor_radius();
        self.pos = *e.axis();
    }
    /// `SetMajorRadius`.
    pub fn set_major_radius(&mut self, r: f64) -> Result<()> {
        if r < self.minor_radius {
            return Err(Geom2dError::Construction);
        }
        self.major_radius = r;
        Ok(())
    }
    /// `SetMinorRadius`.
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 || self.major_radius < r {
            return Err(Geom2dError::Construction);
        }
        self.minor_radius = r;
        Ok(())
    }
    /// `Elips2d`.
    pub fn elips2d(&self) -> Elips2d {
        Elips2d::from_ax22d(self.pos, self.major_radius, self.minor_radius).unwrap_or_default()
    }
    /// `Directrix1` (a circle is unchecked in OCCT: an error here).
    pub fn directrix1(&self) -> Result<Ax2d> {
        Ok(self.elips2d().directrix1()?)
    }
    /// `Directrix2`.
    pub fn directrix2(&self) -> Result<Ax2d> {
        Ok(self.elips2d().directrix2()?)
    }
    /// `Focal`.
    pub fn focal(&self) -> f64 {
        2.0 * (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt()
    }
    /// `Focus1`.
    pub fn focus1(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt();
        Pnt2d::new(
            self.pos.location().x() + c * self.pos.x_direction().x(),
            self.pos.location().y() + c * self.pos.x_direction().y(),
        )
    }
    /// `Focus2`.
    pub fn focus2(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt();
        Pnt2d::new(
            self.pos.location().x() - c * self.pos.x_direction().x(),
            self.pos.location().y() - c * self.pos.x_direction().y(),
        )
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

impl Geometry2d for Ellipse {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.major_radius *= t.scale_factor().abs();
        self.minor_radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(self.clone())
    }
}

impl Curve2d for Ellipse {
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
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        Ok(elclib::ellipse2d_value(u, &self.pos, self.major_radius, self.minor_radius))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::ellipse2d_d1(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::ellipse2d_d2(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2, d3) = elclib::ellipse2d_d3(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD3 { point, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        if n < 1 {
            return Err(Geom2dError::UndefinedDerivative);
        }
        Ok(elclib::ellipse2d_dn(u, &self.pos, self.major_radius, self.minor_radius, n))
    }
}
conic_common!(Ellipse);

/// `Geom2d_Hyperbola`.
#[derive(Debug, Clone)]
pub struct Hyperbola {
    pos: Ax22d,
    major_radius: f64,
    minor_radius: f64,
}

impl Hyperbola {
    /// `Geom2d_Hyperbola(gp_Hypr2d)`.
    pub fn from_hypr2d(h: Hypr2d) -> Self {
        Hyperbola { pos: *h.axis(), major_radius: h.major_radius(), minor_radius: h.minor_radius() }
    }
    /// `Geom2d_Hyperbola(A, MajorRadius, MinorRadius, Sense)`.
    pub fn new(major_axis: Ax2d, major: f64, minor: f64, sense: bool) -> Result<Self> {
        if major < 0.0 || minor < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Hyperbola { pos: Ax22d::from_ax2d(major_axis, sense), major_radius: major, minor_radius: minor })
    }
    /// `Geom2d_Hyperbola(gp_Ax22d, MajorRadius, MinorRadius)`.
    pub fn from_ax22d(a: Ax22d, major: f64, minor: f64) -> Result<Self> {
        if major < 0.0 || minor < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Hyperbola { pos: a, major_radius: major, minor_radius: minor })
    }
    /// `SetHypr2d`.
    pub fn set_hypr2d(&mut self, h: Hypr2d) {
        self.major_radius = h.major_radius();
        self.minor_radius = h.minor_radius();
        self.pos = *h.axis();
    }
    /// `SetMajorRadius`.
    pub fn set_major_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(Geom2dError::Construction);
        }
        self.major_radius = r;
        Ok(())
    }
    /// `SetMinorRadius`.
    pub fn set_minor_radius(&mut self, r: f64) -> Result<()> {
        if r < 0.0 {
            return Err(Geom2dError::Construction);
        }
        self.minor_radius = r;
        Ok(())
    }
    /// `Hypr2d`.
    pub fn hypr2d(&self) -> Hypr2d {
        Hypr2d::from_ax22d(self.pos, self.major_radius, self.minor_radius).unwrap_or_default()
    }
    fn check_major(&self) -> Result<()> {
        if self.major_radius <= gp::resolution() {
            return Err(Geom2dError::Domain);
        }
        Ok(())
    }
    /// `Asymptote1`.
    pub fn asymptote1(&self) -> Result<Ax2d> {
        self.check_major()?;
        Ok(self.hypr2d().asymptote1()?)
    }
    /// `Asymptote2`.
    pub fn asymptote2(&self) -> Result<Ax2d> {
        self.check_major()?;
        Ok(self.hypr2d().asymptote2()?)
    }
    /// `ConjugateBranch1`.
    pub fn conjugate_branch1(&self) -> Hypr2d {
        self.hypr2d().conjugate_branch1()
    }
    /// `ConjugateBranch2`.
    pub fn conjugate_branch2(&self) -> Hypr2d {
        self.hypr2d().conjugate_branch2()
    }
    /// `Directrix1`.
    pub fn directrix1(&self) -> Result<Ax2d> {
        self.check_major()?;
        Ok(self.hypr2d().directrix1()?)
    }
    /// `Directrix2`.
    pub fn directrix2(&self) -> Result<Ax2d> {
        self.check_major()?;
        Ok(self.hypr2d().directrix2()?)
    }
    /// `Focal`.
    pub fn focal(&self) -> f64 {
        2.0 * (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt()
    }
    /// `Focus1`.
    pub fn focus1(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt();
        let mut pxy: XY = *self.pos.x_direction().xy();
        pxy.multiply(c);
        pxy.add(self.pos.location().xy());
        Pnt2d::from_xy(pxy)
    }
    /// `Focus2`.
    pub fn focus2(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt();
        let mut pxy: XY = *self.pos.x_direction().xy();
        pxy.multiply(-c);
        pxy.add(self.pos.location().xy());
        Pnt2d::from_xy(pxy)
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
    pub fn other_branch(&self) -> Hypr2d {
        self.hypr2d().other_branch()
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

impl Geometry2d for Hyperbola {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.major_radius *= t.scale_factor().abs();
        self.minor_radius *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(self.clone())
    }
}

impl Curve2d for Hyperbola {
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
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        Ok(elclib::hyperbola2d_value(u, &self.pos, self.major_radius, self.minor_radius))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::hyperbola2d_d1(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::hyperbola2d_d2(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2, d3) = elclib::hyperbola2d_d3(u, &self.pos, self.major_radius, self.minor_radius);
        Ok(ResD3 { point, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        if n < 1 {
            return Err(Geom2dError::UndefinedDerivative);
        }
        Ok(elclib::hyperbola2d_dn(u, &self.pos, self.major_radius, self.minor_radius, n))
    }
}
conic_common!(Hyperbola);

/// `Geom2d_Parabola`.
#[derive(Debug, Clone)]
pub struct Parabola {
    pos: Ax22d,
    focal_length: f64,
}

impl Parabola {
    /// `Geom2d_Parabola(gp_Parab2d)`.
    pub fn from_parab2d(p: Parab2d) -> Self {
        Parabola { pos: *p.axis(), focal_length: p.focal() }
    }
    /// `Geom2d_Parabola(MirrorAxis, Focal, Sense)`.
    pub fn new(mirror_axis: Ax2d, focal: f64, sense: bool) -> Result<Self> {
        if focal < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Parabola { pos: Ax22d::from_ax2d(mirror_axis, sense), focal_length: focal })
    }
    /// `Geom2d_Parabola(gp_Ax22d, Focal)`.
    pub fn from_ax22d(a: Ax22d, focal: f64) -> Result<Self> {
        if focal < 0.0 {
            return Err(Geom2dError::Construction);
        }
        Ok(Parabola { pos: a, focal_length: focal })
    }
    /// `Geom2d_Parabola(D, F)`.
    pub fn from_directrix_focus(d: Ax2d, f: Pnt2d) -> Self {
        let prb = Parab2d::from_directrix_focus(&d, &f, true);
        Parabola { pos: *prb.axis(), focal_length: prb.focal() }
    }
    /// `SetFocal`.
    pub fn set_focal(&mut self, f: f64) -> Result<()> {
        if f < 0.0 {
            return Err(Geom2dError::Construction);
        }
        self.focal_length = f;
        Ok(())
    }
    /// `SetParab2d`.
    pub fn set_parab2d(&mut self, p: Parab2d) {
        self.focal_length = p.focal();
        self.pos = *p.axis();
    }
    /// `Parab2d`.
    pub fn parab2d(&self) -> Parab2d {
        Parab2d::from_ax22d(self.pos, self.focal_length).unwrap_or_default()
    }
    /// `Directrix`.
    pub fn directrix(&self) -> Ax2d {
        self.parab2d().directrix()
    }
    /// `Focus`.
    pub fn focus(&self) -> Pnt2d {
        let mut pxy: XY = *self.pos.x_direction().xy();
        pxy.multiply(self.focal_length);
        pxy.add(self.pos.location().xy());
        Pnt2d::from_xy(pxy)
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

impl Geometry2d for Parabola {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.focal_length *= t.scale_factor().abs();
        self.pos.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(self.clone())
    }
}

impl Curve2d for Parabola {
    curve_common!();
    fn reversed_parameter(&self, u: f64) -> f64 {
        -u
    }
    fn transformed_parameter(&self, u: f64, t: &Trsf2d) -> f64 {
        if is_infinite(u) {
            return u;
        }
        u * t.scale_factor().abs()
    }
    fn parametric_transformation(&self, t: &Trsf2d) -> f64 {
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
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        Ok(elclib::parabola2d_value(u, &self.pos, self.focal_length))
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = elclib::parabola2d_d1(u, &self.pos, self.focal_length);
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = elclib::parabola2d_d2(u, &self.pos, self.focal_length);
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2) = elclib::parabola2d_d2(u, &self.pos, self.focal_length);
        Ok(ResD3 { point, d1, d2, d3: Vec2d::new(0.0, 0.0) })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        if n < 1 {
            return Err(Geom2dError::UndefinedDerivative);
        }
        Ok(elclib::parabola2d_dn(u, &self.pos, self.focal_length, n))
    }
}
conic_common!(Parabola);
