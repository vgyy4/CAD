//! gp_Circ2d, gp_Elips2d, gp_Hypr2d, gp_Parab2d
use super::{Ax2d, Ax22d, ConstructionError, Dir2d, Pnt2d, Trsf2d, Vec2d, resolution};
use std::f64::consts::{FRAC_PI_2, PI};

/// `gp_Trsf2d::Value(row, col)` for valid indices (the coefficient code only asks for those).
fn tv(t: &Trsf2d, row: i32, col: i32) -> f64 {
    t.value(row, col).unwrap_or(0.0)
}

/// `Reverse()` shared by the four conics: the Y direction is flipped and the frame rebuilt.
fn reversed_axis(pos: &Ax22d) -> Ax22d {
    let mut temp = *pos.y_direction();
    temp.reverse();
    Ax22d::from_dirs(*pos.location(), *pos.x_direction(), temp)
}

// The members every conic has: the frame accessors, Reverse, the motions that act on the frame
// only, and the mirror trait.
macro_rules! conic2d_common {
    ($t:ident, $trait:ident, $mirror_fn:ident) => {
        /// Things this conic can be mirrored about: a point or an axis.
        pub trait $trait {
            /// Mirrors `c` about `self`.
            fn $mirror_fn(&self, c: &mut $t);
        }

        impl $trait for Pnt2d {
            fn $mirror_fn(&self, c: &mut $t) {
                c.pos.mirror(self);
            }
        }

        impl $trait for Ax2d {
            fn $mirror_fn(&self, c: &mut $t) {
                c.pos.mirror(self);
            }
        }

        impl $t {
            /// SetLocation
            pub fn set_location(&mut self, p: Pnt2d) {
                self.pos.set_location(p);
            }
            /// SetAxis
            pub fn set_axis(&mut self, a: Ax22d) {
                self.pos.set_axis(a);
            }
            /// Location
            pub fn location(&self) -> &Pnt2d {
                self.pos.location()
            }
            /// Axis
            pub fn axis(&self) -> &Ax22d {
                &self.pos
            }
            /// XAxis
            pub fn x_axis(&self) -> Ax2d {
                self.pos.x_axis()
            }
            /// YAxis
            pub fn y_axis(&self) -> Ax2d {
                self.pos.y_axis()
            }
            /// Reverse
            pub fn reverse(&mut self) {
                self.pos.set_axis(reversed_axis(&self.pos));
            }
            /// Reversed
            pub fn reversed(&self) -> $t {
                let mut c = *self;
                c.pos.set_axis(reversed_axis(&self.pos));
                c
            }
            /// IsDirect
            pub fn is_direct(&self) -> bool {
                self.pos.x_direction().crossed(self.pos.y_direction()) >= 0.0
            }
            /// Mirror
            pub fn mirror<M: $trait>(&mut self, m: &M) {
                m.$mirror_fn(self);
            }
            /// Mirrored
            pub fn mirrored<M: $trait>(&self, m: &M) -> $t {
                let mut c = *self;
                m.$mirror_fn(&mut c);
                c
            }
            /// Rotate
            pub fn rotate(&mut self, p: &Pnt2d, ang: f64) {
                self.pos.rotate(p, ang);
            }
            /// Rotated
            pub fn rotated(&self, p: &Pnt2d, ang: f64) -> $t {
                let mut c = *self;
                c.pos.rotate(p, ang);
                c
            }
            /// Translate(gp_Vec2d)
            pub fn translate(&mut self, v: &Vec2d) {
                self.pos.translate(v);
            }
            /// Translated(gp_Vec2d)
            pub fn translated(&self, v: &Vec2d) -> $t {
                let mut c = *self;
                c.pos.translate(v);
                c
            }
            /// Translate(P1, P2)
            pub fn translate_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
                self.pos.translate_points(p1, p2);
            }
            /// Translated(P1, P2)
            pub fn translated_points(&self, p1: &Pnt2d, p2: &Pnt2d) -> $t {
                let mut c = *self;
                c.pos.translate_points(p1, p2);
                c
            }
        }
    };
}

// ---------------------------------------------------------------------------------------------
// gp_Circ2d

/// gp_Circ2d
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circ2d {
    pos: Ax22d,
    radius: f64,
}

impl Default for Circ2d {
    // gp_Circ2d::gp_Circ2d()
    fn default() -> Self {
        Self { pos: Ax22d::default(), radius: f64::MAX }
    }
}

conic2d_common!(Circ2d, Circ2dMirror, mirror_circ2d);

impl Circ2d {
    // gp_Circ2d::gp_Circ2d(XAxis, Radius, Sense)
    pub fn new(x_axis: Ax2d, radius: f64, is_sense: bool) -> Result<Self, ConstructionError> {
        if radius < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: Ax22d::from_ax2d(x_axis, is_sense), radius })
    }
    // gp_Circ2d::gp_Circ2d(Axis, Radius)
    pub fn from_ax22d(axis: Ax22d, radius: f64) -> Result<Self, ConstructionError> {
        if radius < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: axis, radius })
    }
    // gp_Circ2d::SetXAxis
    pub fn set_x_axis(&mut self, a: Ax2d) {
        self.pos.set_x_axis(a);
    }
    // gp_Circ2d::SetYAxis
    pub fn set_y_axis(&mut self, a: Ax2d) {
        self.pos.set_y_axis(a);
    }
    // gp_Circ2d::SetRadius
    pub fn set_radius(&mut self, radius: f64) -> Result<(), ConstructionError> {
        if radius < 0.0 {
            return Err(ConstructionError);
        }
        self.radius = radius;
        Ok(())
    }
    // gp_Circ2d::Area
    pub fn area(&self) -> f64 {
        PI * self.radius * self.radius
    }
    // gp_Circ2d::Coefficients
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64, f64) {
        let xc = self.pos.location().x();
        let yc = self.pos.location().y();
        (1.0, 1.0, 0.0, -xc, -yc, xc * xc + yc * yc - self.radius * self.radius)
    }
    // gp_Circ2d::Contains
    pub fn contains(&self, p: &Pnt2d, linear_tolerance: f64) -> bool {
        self.distance(p) <= linear_tolerance
    }
    // gp_Circ2d::Distance
    pub fn distance(&self, p: &Pnt2d) -> f64 {
        let mut coord = *p.xy();
        coord.subtract(self.pos.location().xy());
        let mut d = self.radius - coord.modulus();
        if d < 0.0 {
            d = -d;
        }
        d
    }
    // gp_Circ2d::SquareDistance
    pub fn square_distance(&self, p: &Pnt2d) -> f64 {
        let mut coord = *p.xy();
        coord.subtract(self.pos.location().xy());
        let d = self.radius - coord.modulus();
        d * d
    }
    // gp_Circ2d::Length
    pub fn length(&self) -> f64 {
        2. * PI * self.radius
    }
    // gp_Circ2d::Radius
    pub fn radius(&self) -> f64 {
        self.radius
    }
    // gp_Circ2d::Position
    pub fn position(&self) -> &Ax22d {
        &self.pos
    }
    // gp_Circ2d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        self.radius *= s;
        if self.radius < 0.0 {
            self.radius = -self.radius;
        }
        self.pos.scale(p, s);
    }
    // gp_Circ2d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Circ2d {
        let mut c = *self;
        c.radius *= s;
        if c.radius < 0.0 {
            c.radius = -c.radius;
        }
        c.pos.scale(p, s);
        c
    }
    // gp_Circ2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        self.radius *= t.scale_factor();
        if self.radius < 0.0 {
            self.radius = -self.radius;
        }
        self.pos.transform(t);
    }
    // gp_Circ2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Circ2d {
        let mut c = *self;
        c.radius *= t.scale_factor();
        if c.radius < 0.0 {
            c.radius = -c.radius;
        }
        c.pos.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Elips2d

/// gp_Elips2d
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elips2d {
    pos: Ax22d,
    major_radius: f64,
    minor_radius: f64,
}

impl Default for Elips2d {
    // gp_Elips2d::gp_Elips2d()
    fn default() -> Self {
        Self { pos: Ax22d::default(), major_radius: f64::MAX, minor_radius: resolution() }
    }
}

conic2d_common!(Elips2d, Elips2dMirror, mirror_elips2d);

impl Elips2d {
    // gp_Elips2d::gp_Elips2d(MajorAxis, MajorRadius, MinorRadius, Sense)
    pub fn new(major_axis: Ax2d, major: f64, minor: f64, is_sense: bool) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < minor {
            return Err(ConstructionError);
        }
        Ok(Self { pos: Ax22d::from_ax2d(major_axis, is_sense), major_radius: major, minor_radius: minor })
    }
    // gp_Elips2d::gp_Elips2d(A, MajorRadius, MinorRadius)
    pub fn from_ax22d(a: Ax22d, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < minor {
            return Err(ConstructionError);
        }
        Ok(Self { pos: a, major_radius: major, minor_radius: minor })
    }
    // gp_Elips2d::SetMajorRadius
    pub fn set_major_radius(&mut self, major: f64) -> Result<(), ConstructionError> {
        if major < self.minor_radius {
            return Err(ConstructionError);
        }
        self.major_radius = major;
        Ok(())
    }
    // gp_Elips2d::SetMinorRadius
    pub fn set_minor_radius(&mut self, minor: f64) -> Result<(), ConstructionError> {
        if minor < 0.0 || self.major_radius < minor {
            return Err(ConstructionError);
        }
        self.minor_radius = minor;
        Ok(())
    }
    // gp_Elips2d::SetXAxis
    pub fn set_x_axis(&mut self, a: Ax2d) {
        self.pos.set_x_axis(a);
    }
    // gp_Elips2d::SetYAxis
    pub fn set_y_axis(&mut self, a: Ax2d) {
        self.pos.set_y_axis(a);
    }
    // gp_Elips2d::Area
    pub fn area(&self) -> f64 {
        PI * self.major_radius * self.minor_radius
    }
    // gp_Elips2d::Coefficients
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64, f64) {
        let d_min = self.minor_radius * self.minor_radius;
        let d_maj = self.major_radius * self.major_radius;
        if d_min <= resolution() && d_maj <= resolution() {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }
        let mut t = Trsf2d::new();
        t.set_transformation(&self.pos.x_axis());
        let t11 = tv(&t, 1, 1);
        let t12 = tv(&t, 1, 2);
        let t13 = tv(&t, 1, 3);
        if d_min <= resolution() {
            (t11 * t11, t12 * t12, t11 * t12, t11 * t13, t12 * t13, t13 * t13 - d_maj)
        } else {
            let t21 = tv(&t, 2, 1);
            let t22 = tv(&t, 2, 2);
            let t23 = tv(&t, 2, 3);
            (
                (t11 * t11 / d_maj) + (t21 * t21 / d_min),
                (t12 * t12 / d_maj) + (t22 * t22 / d_min),
                (t11 * t12 / d_maj) + (t21 * t22 / d_min),
                (t11 * t13 / d_maj) + (t21 * t23 / d_min),
                (t12 * t13 / d_maj) + (t22 * t23 / d_min),
                (t13 * t13 / d_maj) + (t23 * t23 / d_min) - 1.0,
            )
        }
    }
    // gp_Elips2d::Directrix1
    pub fn directrix1(&self) -> Result<Ax2d, ConstructionError> {
        let e = self.eccentricity();
        if e <= resolution() {
            return Err(ConstructionError);
        }
        let mut orig = *self.pos.x_direction().xy();
        orig.multiply(self.major_radius / e);
        orig.add(self.pos.location().xy());
        Ok(Ax2d::new(Pnt2d::from_xy(orig), *self.pos.y_direction()))
    }
    // gp_Elips2d::Directrix2
    pub fn directrix2(&self) -> Result<Ax2d, ConstructionError> {
        let e = self.eccentricity();
        if e <= resolution() {
            return Err(ConstructionError);
        }
        let mut orig = *self.pos.x_direction().xy();
        orig.multiply(-self.major_radius / e);
        orig.add(self.pos.location().xy());
        Ok(Ax2d::new(Pnt2d::from_xy(orig), *self.pos.y_direction()))
    }
    // gp_Elips2d::Eccentricity
    pub fn eccentricity(&self) -> f64 {
        if self.major_radius == 0.0 {
            0.0
        } else {
            (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt() / self.major_radius
        }
    }
    // gp_Elips2d::Focal
    pub fn focal(&self) -> f64 {
        2.0 * (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt()
    }
    // gp_Elips2d::Focus1
    pub fn focus1(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt();
        let pp = self.pos.location();
        let dd = self.pos.x_direction();
        Pnt2d::new(pp.x() + c * dd.x(), pp.y() + c * dd.y())
    }
    // gp_Elips2d::Focus2
    pub fn focus2(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius - self.minor_radius * self.minor_radius).sqrt();
        let pp = self.pos.location();
        let dd = self.pos.x_direction();
        Pnt2d::new(pp.x() - c * dd.x(), pp.y() - c * dd.y())
    }
    // gp_Elips2d::MajorRadius
    pub fn major_radius(&self) -> f64 {
        self.major_radius
    }
    // gp_Elips2d::MinorRadius
    pub fn minor_radius(&self) -> f64 {
        self.minor_radius
    }
    // gp_Elips2d::Parameter
    pub fn parameter(&self) -> f64 {
        if self.major_radius == 0.0 { 0.0 } else { (self.minor_radius * self.minor_radius) / self.major_radius }
    }
    // gp_Elips2d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        self.major_radius *= s;
        if self.major_radius < 0.0 {
            self.major_radius = -self.major_radius;
        }
        self.minor_radius *= s;
        if self.minor_radius < 0.0 {
            self.minor_radius = -self.minor_radius;
        }
        self.pos.scale(p, s);
    }
    // gp_Elips2d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Elips2d {
        let mut e = *self;
        e.major_radius *= s;
        if e.major_radius < 0.0 {
            e.major_radius = -e.major_radius;
        }
        e.minor_radius *= s;
        if e.minor_radius < 0.0 {
            e.minor_radius = -e.minor_radius;
        }
        e.pos.scale(p, s);
        e
    }
    // gp_Elips2d::Transform (takes the absolute value of the scale factor first)
    pub fn transform(&mut self, t: &Trsf2d) {
        let mut t_sca = t.scale_factor();
        if t_sca < 0.0 {
            t_sca = -t_sca;
        }
        self.major_radius *= t_sca;
        self.minor_radius *= t_sca;
        self.pos.transform(t);
    }
    // gp_Elips2d::Transformed (takes the absolute value of the products)
    pub fn transformed(&self, t: &Trsf2d) -> Elips2d {
        let mut e = *self;
        e.major_radius *= t.scale_factor();
        if e.major_radius < 0.0 {
            e.major_radius = -e.major_radius;
        }
        e.minor_radius *= t.scale_factor();
        if e.minor_radius < 0.0 {
            e.minor_radius = -e.minor_radius;
        }
        e.pos.transform(t);
        e
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Hypr2d

/// gp_Hypr2d
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hypr2d {
    pos: Ax22d,
    major_radius: f64,
    minor_radius: f64,
}

impl Default for Hypr2d {
    // gp_Hypr2d::gp_Hypr2d()
    fn default() -> Self {
        Self { pos: Ax22d::default(), major_radius: f64::MAX, minor_radius: f64::MAX }
    }
}

conic2d_common!(Hypr2d, Hypr2dMirror, mirror_hypr2d);

impl Hypr2d {
    // gp_Hypr2d::gp_Hypr2d(MajorAxis, MajorRadius, MinorRadius, Sense)
    pub fn new(major_axis: Ax2d, major: f64, minor: f64, is_sense: bool) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: Ax22d::from_ax2d(major_axis, is_sense), major_radius: major, minor_radius: minor })
    }
    // gp_Hypr2d::gp_Hypr2d(A, MajorRadius, MinorRadius)
    pub fn from_ax22d(a: Ax22d, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: a, major_radius: major, minor_radius: minor })
    }
    // gp_Hypr2d::SetMajorRadius
    pub fn set_major_radius(&mut self, major: f64) -> Result<(), ConstructionError> {
        if major < 0.0 {
            return Err(ConstructionError);
        }
        self.major_radius = major;
        Ok(())
    }
    // gp_Hypr2d::SetMinorRadius
    pub fn set_minor_radius(&mut self, minor: f64) -> Result<(), ConstructionError> {
        if minor < 0.0 {
            return Err(ConstructionError);
        }
        self.minor_radius = minor;
        Ok(())
    }
    // gp_Hypr2d::SetXAxis
    pub fn set_x_axis(&mut self, a: Ax2d) {
        self.pos.set_x_axis(a);
    }
    // gp_Hypr2d::SetYAxis
    pub fn set_y_axis(&mut self, a: Ax2d) {
        self.pos.set_y_axis(a);
    }
    // gp_Hypr2d::Asymptote1
    pub fn asymptote1(&self) -> Result<Ax2d, ConstructionError> {
        if self.major_radius <= resolution() {
            return Err(ConstructionError);
        }
        let mut vdir = *self.pos.x_direction();
        let mut coord1 = *self.pos.y_direction().xy();
        let coord2 = coord1.multiplied(self.minor_radius / self.major_radius);
        coord1.add(&coord2);
        vdir.set_xy(coord1)?;
        Ok(Ax2d::new(*self.pos.location(), vdir))
    }
    // gp_Hypr2d::Asymptote2 (the direction goes through a gp_Vec2d and is normalized on conversion)
    pub fn asymptote2(&self) -> Result<Ax2d, ConstructionError> {
        if self.major_radius <= resolution() {
            return Err(ConstructionError);
        }
        let mut vdir = Vec2d::from_dir(self.pos.x_direction());
        let mut coord1 = *self.pos.y_direction().xy();
        let coord2 = coord1.multiplied(-self.minor_radius / self.major_radius);
        coord1.add(&coord2);
        vdir.set_xy(coord1);
        Ok(Ax2d::new(*self.pos.location(), Dir2d::from_vec(&vdir)?))
    }
    // gp_Hypr2d::Coefficients
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64, f64) {
        let d_min = self.minor_radius * self.minor_radius;
        let d_maj = self.major_radius * self.major_radius;
        if d_min <= resolution() && d_maj <= resolution() {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }
        let mut t = Trsf2d::new();
        t.set_transformation(&self.pos.x_axis());
        let t11 = tv(&t, 1, 1);
        let t12 = tv(&t, 1, 2);
        let t13 = tv(&t, 1, 3);
        if d_min <= resolution() {
            (t11 * t11, t12 * t12, t11 * t12, t11 * t13, t12 * t13, t13 * t13 - d_maj)
        } else {
            let t21 = tv(&t, 2, 1);
            let t22 = tv(&t, 2, 2);
            let t23 = tv(&t, 2, 3);
            (
                (t11 * t11 / d_maj) - (t21 * t21 / d_min),
                (t12 * t12 / d_maj) - (t22 * t22 / d_min),
                (t11 * t12 / d_maj) - (t21 * t22 / d_min),
                (t11 * t13 / d_maj) - (t21 * t23 / d_min),
                (t12 * t13 / d_maj) - (t22 * t23 / d_min),
                (t13 * t13 / d_maj) - (t23 * t23 / d_min) - 1.0,
            )
        }
    }
    /// `gp_Hypr2d(gp_Ax2d(P, V), major, minor, sense)` for radii that are already valid.
    fn from_valid(loc: Pnt2d, v: Dir2d, major: f64, minor: f64, is_sense: bool) -> Hypr2d {
        Hypr2d { pos: Ax22d::from_ax2d(Ax2d::new(loc, v), is_sense), major_radius: major, minor_radius: minor }
    }
    // gp_Hypr2d::ConjugateBranch1
    pub fn conjugate_branch1(&self) -> Hypr2d {
        let v = *self.pos.y_direction();
        let is_sign = self.pos.x_direction().crossed(self.pos.y_direction()) >= 0.0;
        Self::from_valid(*self.pos.location(), v, self.minor_radius, self.major_radius, is_sign)
    }
    // gp_Hypr2d::ConjugateBranch2
    pub fn conjugate_branch2(&self) -> Hypr2d {
        let v = self.pos.y_direction().reversed();
        let is_sign = self.pos.x_direction().crossed(self.pos.y_direction()) >= 0.0;
        Self::from_valid(*self.pos.location(), v, self.minor_radius, self.major_radius, is_sign)
    }
    // gp_Hypr2d::OtherBranch
    pub fn other_branch(&self) -> Hypr2d {
        let is_sign = self.pos.x_direction().crossed(self.pos.y_direction()) >= 0.0;
        Self::from_valid(
            *self.pos.location(),
            self.pos.x_direction().reversed(),
            self.major_radius,
            self.minor_radius,
            is_sign,
        )
    }
    // gp_Hypr2d::Directrix1
    pub fn directrix1(&self) -> Result<Ax2d, ConstructionError> {
        let e = self.eccentricity()?;
        let mut orig = *self.pos.x_direction().xy();
        orig.multiply(self.major_radius / e);
        orig.add(self.pos.location().xy());
        Ok(Ax2d::new(Pnt2d::from_xy(orig), *self.pos.y_direction()))
    }
    // gp_Hypr2d::Directrix2
    pub fn directrix2(&self) -> Result<Ax2d, ConstructionError> {
        let e = self.eccentricity()?;
        let mut orig = *self.pos.x_direction().xy();
        orig.multiply(self.parameter()? / e);
        orig.add(self.focus1().xy());
        Ok(Ax2d::new(Pnt2d::from_xy(orig), *self.pos.y_direction()))
    }
    // gp_Hypr2d::Eccentricity
    pub fn eccentricity(&self) -> Result<f64, ConstructionError> {
        if self.major_radius <= resolution() {
            return Err(ConstructionError);
        }
        Ok((self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt() / self.major_radius)
    }
    // gp_Hypr2d::Focal
    pub fn focal(&self) -> f64 {
        2.0 * (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt()
    }
    // gp_Hypr2d::Focus1
    pub fn focus1(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt();
        Pnt2d::new(
            self.pos.location().x() + c * self.pos.x_direction().x(),
            self.pos.location().y() + c * self.pos.x_direction().y(),
        )
    }
    // gp_Hypr2d::Focus2
    pub fn focus2(&self) -> Pnt2d {
        let c = (self.major_radius * self.major_radius + self.minor_radius * self.minor_radius).sqrt();
        Pnt2d::new(
            self.pos.location().x() - c * self.pos.x_direction().x(),
            self.pos.location().y() - c * self.pos.x_direction().y(),
        )
    }
    // gp_Hypr2d::MajorRadius
    pub fn major_radius(&self) -> f64 {
        self.major_radius
    }
    // gp_Hypr2d::MinorRadius
    pub fn minor_radius(&self) -> f64 {
        self.minor_radius
    }
    // gp_Hypr2d::Parameter
    pub fn parameter(&self) -> Result<f64, ConstructionError> {
        if self.major_radius <= resolution() {
            return Err(ConstructionError);
        }
        Ok((self.minor_radius * self.minor_radius) / self.major_radius)
    }
    // gp_Hypr2d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        self.major_radius *= s;
        if self.major_radius < 0.0 {
            self.major_radius = -self.major_radius;
        }
        self.minor_radius *= s;
        if self.minor_radius < 0.0 {
            self.minor_radius = -self.minor_radius;
        }
        self.pos.scale(p, s);
    }
    // gp_Hypr2d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Hypr2d {
        let mut h = *self;
        h.scale(p, s);
        h
    }
    // gp_Hypr2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        self.major_radius *= t.scale_factor();
        if self.major_radius < 0.0 {
            self.major_radius = -self.major_radius;
        }
        self.minor_radius *= t.scale_factor();
        if self.minor_radius < 0.0 {
            self.minor_radius = -self.minor_radius;
        }
        self.pos.transform(t);
    }
    // gp_Hypr2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Hypr2d {
        let mut h = *self;
        h.transform(t);
        h
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Parab2d

/// gp_Parab2d
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parab2d {
    pos: Ax22d,
    focal_length: f64,
}

impl Default for Parab2d {
    // gp_Parab2d::gp_Parab2d()
    fn default() -> Self {
        Self { pos: Ax22d::default(), focal_length: f64::MAX }
    }
}

conic2d_common!(Parab2d, Parab2dMirror, mirror_parab2d);

impl Parab2d {
    // gp_Parab2d::gp_Parab2d(MirrorAxis, Focal, Sense)
    pub fn new(mirror_axis: Ax2d, focal: f64, is_sense: bool) -> Result<Self, ConstructionError> {
        if focal < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: Ax22d::from_ax2d(mirror_axis, is_sense), focal_length: focal })
    }
    // gp_Parab2d::gp_Parab2d(Axes, Focal)
    pub fn from_ax22d(axes: Ax22d, focal: f64) -> Result<Self, ConstructionError> {
        if focal < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: axes, focal_length: focal })
    }
    // gp_Parab2d::gp_Parab2d(Directrix, Focus, Sense). A focus so close to the directrix that the
    // axis direction underflows (OCCT would raise) falls back to the default direction.
    pub fn from_directrix_focus(directrix: &Ax2d, focus: &Pnt2d, is_sense: bool) -> Parab2d {
        let dir_loc = directrix.location();
        let dir_vec = directrix.direction();
        let f_vec = Vec2d::between(dir_loc, focus);
        let origin = Pnt2d::from_xy(dir_loc.xy().added(&dir_vec.xy().multiplied(f_vec.dot(&Vec2d::from_dir(dir_vec)))));
        let apex = Pnt2d::from_xy(origin.xy().added(focus.xy()).multiplied(0.5));
        let focal_length = 0.5 * origin.distance(focus);
        let x_dir = if focal_length > 0.0 {
            Dir2d::from_xy(focus.xy().subtracted(origin.xy())).unwrap_or_default()
        } else {
            *directrix.rotated(dir_loc, if is_sense { -FRAC_PI_2 } else { FRAC_PI_2 }).direction()
        };
        Parab2d { pos: Ax22d::from_dirs(apex, x_dir, *dir_vec), focal_length }
    }
    // gp_Parab2d::SetFocal
    pub fn set_focal(&mut self, focal: f64) -> Result<(), ConstructionError> {
        if focal < 0.0 {
            return Err(ConstructionError);
        }
        self.focal_length = focal;
        Ok(())
    }
    // gp_Parab2d::SetMirrorAxis
    pub fn set_mirror_axis(&mut self, a: Ax2d) {
        self.pos.set_x_axis(a);
    }
    // gp_Parab2d::Coefficients
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64, f64) {
        let p = 2.0 * self.focal_length;
        let mut t = Trsf2d::new();
        t.set_transformation(&self.pos.x_axis());
        let t11 = tv(&t, 1, 1);
        let t12 = tv(&t, 1, 2);
        let t13 = tv(&t, 1, 3);
        let t21 = tv(&t, 2, 1);
        let t22 = tv(&t, 2, 2);
        let t23 = tv(&t, 2, 3);
        (
            t21 * t21,
            t22 * t22,
            t21 * t22,
            (t21 * t23) - (p * t11),
            (t22 * t23) - (p * t12),
            (t23 * t23) - (2.0 * p * t13),
        )
    }
    // gp_Parab2d::Directrix
    pub fn directrix(&self) -> Ax2d {
        let p = Pnt2d::new(
            self.pos.location().x() - self.focal_length * self.pos.x_direction().x(),
            self.pos.location().y() - self.focal_length * self.pos.x_direction().y(),
        );
        Ax2d::new(p, *self.pos.y_direction())
    }
    // gp_Parab2d::Focal
    pub fn focal(&self) -> f64 {
        self.focal_length
    }
    // gp_Parab2d::Focus
    pub fn focus(&self) -> Pnt2d {
        Pnt2d::new(
            self.pos.location().x() + self.focal_length * self.pos.x_direction().x(),
            self.pos.location().y() + self.focal_length * self.pos.x_direction().y(),
        )
    }
    // gp_Parab2d::MirrorAxis
    pub fn mirror_axis(&self) -> Ax2d {
        self.pos.x_axis()
    }
    // gp_Parab2d::Parameter
    pub fn parameter(&self) -> f64 {
        2.0 * self.focal_length
    }
    // gp_Parab2d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        self.focal_length *= s;
        if self.focal_length < 0.0 {
            self.focal_length = -self.focal_length;
        }
        self.pos.scale(p, s);
    }
    // gp_Parab2d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Parab2d {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    // gp_Parab2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        self.focal_length *= t.scale_factor();
        if self.focal_length < 0.0 {
            self.focal_length = -self.focal_length;
        }
        self.pos.transform(t);
    }
    // gp_Parab2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Parab2d {
        let mut c = *self;
        c.transform(t);
        c
    }
}
