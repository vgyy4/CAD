//! gp_Lin, gp_Circ, gp_Elips, gp_Hypr, gp_Parab.
use super::macros::scaled_radius;
use super::{Ax1, Ax2, ConstructionError, Dir, Pnt, Trsf, Vec, resolution};
use std::f64::consts::PI;

// `sqrt(major * major - minor * minor)` of the ellipse's Focal/Focus1/Focus2 (same operation order as OCCT).
fn focal_distance_ellipse(major: f64, minor: f64) -> f64 {
    (major * major - minor * minor).sqrt()
}

// `gp_Pnt(P.X() + c * D.X(), P.Y() + c * D.Y(), P.Z() + c * D.Z())`: Focus1 of the conics, Focus of the parabola.
fn point_along(p: &Pnt, c: f64, d: &Dir) -> Pnt {
    Pnt::new(p.x() + c * d.x(), p.y() + c * d.y(), p.z() + c * d.z())
}

// `gp_Pnt(P.X() - c * D.X(), ...)`: Focus2 of the conics, Directrix of the parabola.
fn point_back(p: &Pnt, c: f64, d: &Dir) -> Pnt {
    Pnt::new(p.x() - c * d.x(), p.y() - c * d.y(), p.z() - c * d.z())
}

// ---------------------------------------------------------------------------------------------
// gp_Lin

/// The argument types of `gp_Lin::Distance` / `gp_Lin::SquareDistance`: a point or another line.
pub trait LinDistance {
    /// `line.Distance(self)`
    fn distance_from_lin(&self, line: &Lin) -> f64;
    /// `line.SquareDistance(self)`
    fn square_distance_from_lin(&self, line: &Lin) -> f64;
}

/// gp_Lin (the default is the default `Ax1`: origin, direction +Z)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lin(pub(crate) Ax1);

motion_common!(Lin, LinMirror, mirror_lin, 0);

impl LinDistance for Pnt {
    // gp_Lin::Distance(gp_Pnt)
    fn distance_from_lin(&self, line: &Lin) -> f64 {
        let mut coord = *self.xyz();
        coord.subtract(line.0.location().xyz());
        coord.cross(line.0.direction().xyz());
        coord.modulus()
    }
    // gp_Lin::SquareDistance(gp_Pnt)
    fn square_distance_from_lin(&self, line: &Lin) -> f64 {
        let loc = line.0.location();
        let mut v = Vec::new(self.x() - loc.x(), self.y() - loc.y(), self.z() - loc.z());
        v.cross(&Vec::from_dir(line.0.direction()));
        v.square_magnitude()
    }
}

impl LinDistance for Lin {
    // gp_Lin::Distance(gp_Lin)
    //
    // OCCT raises when the cross product of two almost parallel directions underflows (the squared
    // modulus is zero although the angle is above the resolution); the signature of the contract has no
    // error there, so this case yields NaN.
    fn distance_from_lin(&self, line: &Lin) -> f64 {
        // `self` is the argument ("Other"), `line` the receiver (the C++ `*this`).
        if line.0.is_parallel(&self.0, resolution()) {
            return line.0.location().distance_from_lin(self);
        }
        match line.0.direction().crossed(self.0.direction()) {
            Ok(dir) => Vec::between(line.0.location(), self.0.location()).dot(&Vec::from_dir(&dir)).abs(),
            Err(_) => f64::NAN,
        }
    }
    // gp_Lin::SquareDistance(gp_Lin)
    fn square_distance_from_lin(&self, line: &Lin) -> f64 {
        let d = self.distance_from_lin(line);
        d * d
    }
}

impl Lin {
    // gp_Lin::gp_Lin(Ax1)
    pub fn from_axis(a: Ax1) -> Self {
        Self(a)
    }
    // gp_Lin::gp_Lin(P, V)
    pub fn new(p: Pnt, v: Dir) -> Self {
        Self(Ax1::new(p, v))
    }
    /// Reverse
    pub fn reverse(&mut self) {
        self.0.reverse();
    }
    /// Reversed
    pub fn reversed(&self) -> Lin {
        let mut l = *self;
        l.0.reverse();
        l
    }
    /// SetDirection
    pub fn set_direction(&mut self, v: Dir) {
        self.0.set_direction(v);
    }
    /// SetLocation
    pub fn set_location(&mut self, p: Pnt) {
        self.0.set_location(p);
    }
    /// SetPosition
    pub fn set_position(&mut self, a1: Ax1) {
        self.0 = a1;
    }
    /// Direction
    pub fn direction(&self) -> &Dir {
        self.0.direction()
    }
    /// Location
    pub fn location(&self) -> &Pnt {
        self.0.location()
    }
    /// Position
    pub fn position(&self) -> &Ax1 {
        &self.0
    }
    /// Angle
    pub fn angle(&self, other: &Lin) -> f64 {
        self.0.direction().angle(other.0.direction())
    }
    /// Contains
    pub fn contains(&self, p: &Pnt, linear_tolerance: f64) -> bool {
        self.distance(p) <= linear_tolerance
    }
    /// Distance (a point or a line)
    pub fn distance<D: LinDistance>(&self, d: &D) -> f64 {
        d.distance_from_lin(self)
    }
    /// SquareDistance (a point or a line)
    pub fn square_distance<D: LinDistance>(&self, d: &D) -> f64 {
        d.square_distance_from_lin(self)
    }
    /// Normal
    pub fn normal(&self, p: &Pnt) -> Result<Lin, ConstructionError> {
        let loc = self.0.location();
        let v = Dir::new(p.x() - loc.x(), p.y() - loc.y(), p.z() - loc.z())?;
        let v = self.0.direction().cross_crossed(&v, self.0.direction())?;
        Ok(Lin::new(*p, v))
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.0.scale(p, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Lin {
        let mut l = *self;
        l.0.scale(p, s);
        l
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.0.transform(t);
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Lin {
        let mut l = *self;
        l.0.transform(t);
        l
    }
    /// Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt, p2: &Pnt) {
        self.0.translate_points(p1, p2);
    }
    /// Translated(P1, P2): goes through `gp_Vec(P1, P2)`, unlike the in-place form.
    pub fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> Lin {
        let mut l = *self;
        l.0.translate(&Vec::between(p1, p2));
        l
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Circ

/// gp_Circ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circ {
    pub(crate) a: Ax2,
    pub(crate) r: f64,
}

impl Default for Circ {
    // gp_Circ::gp_Circ()
    fn default() -> Self {
        Self { a: Ax2::default(), r: f64::MAX }
    }
}

motion_common!(Circ, CircMirror, mirror_circ, a);
translate_points_common!(Circ, a);
frame_loc!(Circ, a, Ax2);
frame_set_location!(Circ, a);
frame_xy!(Circ, a);
frame_axis!(Circ, a);

impl Circ {
    // gp_Circ::gp_Circ
    pub fn new(a: Ax2, r: f64) -> Result<Self, ConstructionError> {
        if r < 0.0 { Err(ConstructionError) } else { Ok(Self { a, r }) }
    }
    /// SetRadius
    pub fn set_radius(&mut self, r: f64) -> Result<(), ConstructionError> {
        if r < 0.0 {
            return Err(ConstructionError);
        }
        self.r = r;
        Ok(())
    }
    /// Area
    pub fn area(&self) -> f64 {
        PI * self.r * self.r
    }
    /// Length
    pub fn length(&self) -> f64 {
        2. * PI * self.r
    }
    /// Radius
    pub fn radius(&self) -> f64 {
        self.r
    }
    /// Distance
    pub fn distance(&self, p: &Pnt) -> f64 {
        self.square_distance(p).sqrt()
    }
    /// SquareDistance
    pub fn square_distance(&self, p: &Pnt) -> f64 {
        let v = Vec::between(self.a.location(), p);
        let x = v.dot(&Vec::from_dir(self.a.x_direction()));
        let y = v.dot(&Vec::from_dir(self.a.y_direction()));
        let z = v.dot(&Vec::from_dir(self.a.direction()));
        let t = (x * x + y * y).sqrt() - self.r;
        t * t + z * z
    }
    /// Contains
    pub fn contains(&self, p: &Pnt, linear_tolerance: f64) -> bool {
        self.distance(p) <= linear_tolerance
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.r = scaled_radius(self.r, s);
        self.a.scale(p, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Circ {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.r = scaled_radius(self.r, t.scale_factor());
        self.a.transform(t);
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Circ {
        let mut c = *self;
        c.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Elips

/// gp_Elips
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elips {
    pub(crate) a: Ax2,
    pub(crate) major: f64,
    pub(crate) minor: f64,
}

impl Default for Elips {
    // gp_Elips::gp_Elips()
    fn default() -> Self {
        Self { a: Ax2::default(), major: f64::MAX, minor: resolution() }
    }
}

motion_common!(Elips, ElipsMirror, mirror_elips, a);
translate_points_common!(Elips, a);
frame_loc!(Elips, a, Ax2);
frame_set_location!(Elips, a);
frame_xy!(Elips, a);
frame_axis!(Elips, a);

impl Elips {
    // gp_Elips::gp_Elips
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < minor { Err(ConstructionError) } else { Ok(Self { a, major, minor }) }
    }
    /// SetMajorRadius
    pub fn set_major_radius(&mut self, major: f64) -> Result<(), ConstructionError> {
        if major < self.minor {
            return Err(ConstructionError);
        }
        self.major = major;
        Ok(())
    }
    /// SetMinorRadius
    pub fn set_minor_radius(&mut self, minor: f64) -> Result<(), ConstructionError> {
        if minor < 0.0 || self.major < minor {
            return Err(ConstructionError);
        }
        self.minor = minor;
        Ok(())
    }
    /// Area
    pub fn area(&self) -> f64 {
        PI * self.major * self.minor
    }
    /// Directrix1
    pub fn directrix1(&self) -> Result<Ax1, ConstructionError> {
        let e = self.eccentricity();
        if e <= resolution() {
            return Err(ConstructionError);
        }
        let mut orig = *self.a.x_direction().xyz();
        orig.multiply(self.major / e);
        orig.add(self.a.location().xyz());
        Ok(Ax1::new(Pnt::from_xyz(orig), *self.a.y_direction()))
    }
    /// Directrix2
    pub fn directrix2(&self) -> Result<Ax1, ConstructionError> {
        let e = self.eccentricity();
        if e <= resolution() {
            return Err(ConstructionError);
        }
        let mut orig = *self.a.x_direction().xyz();
        orig.multiply(-self.major / e);
        orig.add(self.a.location().xyz());
        Ok(Ax1::new(Pnt::from_xyz(orig), *self.a.y_direction()))
    }
    /// Eccentricity
    pub fn eccentricity(&self) -> f64 {
        if self.major == 0.0 { 0.0 } else { (self.major * self.major - self.minor * self.minor).sqrt() / self.major }
    }
    /// Focal
    pub fn focal(&self) -> f64 {
        2.0 * focal_distance_ellipse(self.major, self.minor)
    }
    /// Focus1
    pub fn focus1(&self) -> Pnt {
        let c = focal_distance_ellipse(self.major, self.minor);
        point_along(self.a.location(), c, self.a.x_direction())
    }
    /// Focus2
    pub fn focus2(&self) -> Pnt {
        let c = focal_distance_ellipse(self.major, self.minor);
        point_back(self.a.location(), c, self.a.x_direction())
    }
    /// MajorRadius
    pub fn major_radius(&self) -> f64 {
        self.major
    }
    /// MinorRadius
    pub fn minor_radius(&self) -> f64 {
        self.minor
    }
    /// Parameter
    pub fn parameter(&self) -> f64 {
        if self.major == 0.0 { 0.0 } else { (self.minor * self.minor) / self.major }
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.major = scaled_radius(self.major, s);
        self.minor = scaled_radius(self.minor, s);
        self.a.scale(p, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Elips {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.major = scaled_radius(self.major, t.scale_factor());
        self.minor = scaled_radius(self.minor, t.scale_factor());
        self.a.transform(t);
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Elips {
        let mut c = *self;
        c.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Hypr

/// gp_Hypr
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hypr {
    pub(crate) a: Ax2,
    pub(crate) major: f64,
    pub(crate) minor: f64,
}

impl Default for Hypr {
    // gp_Hypr::gp_Hypr()
    fn default() -> Self {
        Self { a: Ax2::default(), major: f64::MAX, minor: -f64::MAX }
    }
}

motion_common!(Hypr, HyprMirror, mirror_hypr, a);
translate_points_common!(Hypr, a);
frame_loc!(Hypr, a, Ax2);
frame_xy!(Hypr, a);
frame_axis!(Hypr, a);

impl Hypr {
    // gp_Hypr::gp_Hypr
    pub fn new(a: Ax2, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if major < 0.0 || minor < 0.0 { Err(ConstructionError) } else { Ok(Self { a, major, minor }) }
    }
    /// SetLocation: OCCT rebuilds the frame with `gp_Ax2(P, Direction, XDirection)`, which renormalizes
    /// the directions. That constructor cannot fail for the (orthonormal) frame of a conic, so no error
    /// is reported.
    pub fn set_location(&mut self, p: Pnt) {
        if let Ok(a) = Ax2::new(p, *self.a.direction(), self.a.x_direction()) {
            self.a = a;
        }
    }
    /// SetMajorRadius
    pub fn set_major_radius(&mut self, major: f64) -> Result<(), ConstructionError> {
        if major < 0.0 {
            return Err(ConstructionError);
        }
        self.major = major;
        Ok(())
    }
    /// SetMinorRadius
    pub fn set_minor_radius(&mut self, minor: f64) -> Result<(), ConstructionError> {
        if minor < 0.0 {
            return Err(ConstructionError);
        }
        self.minor = minor;
        Ok(())
    }
    /// Asymptote1
    pub fn asymptote1(&self) -> Result<Ax1, ConstructionError> {
        if self.major <= resolution() {
            return Err(ConstructionError);
        }
        let mut v1 = Vec::from_dir(self.a.y_direction());
        v1.multiply(self.minor / self.major);
        let mut v = Vec::from_dir(self.a.x_direction());
        v.add(&v1);
        Ok(Ax1::new(*self.a.location(), Dir::from_vec(&v)?))
    }
    /// Asymptote2
    pub fn asymptote2(&self) -> Result<Ax1, ConstructionError> {
        if self.major <= resolution() {
            return Err(ConstructionError);
        }
        let mut v1 = Vec::from_dir(self.a.y_direction());
        v1.multiply(-self.minor / self.major);
        let mut v = Vec::from_dir(self.a.x_direction());
        v.add(&v1);
        Ok(Ax1::new(*self.a.location(), Dir::from_vec(&v)?))
    }
    /// ConjugateBranch1
    pub fn conjugate_branch1(&self) -> Result<Hypr, ConstructionError> {
        let a = Ax2::new(*self.a.location(), *self.a.direction(), self.a.y_direction())?;
        Hypr::new(a, self.minor, self.major)
    }
    /// ConjugateBranch2
    pub fn conjugate_branch2(&self) -> Result<Hypr, ConstructionError> {
        let mut d = *self.a.y_direction();
        d.reverse();
        let a = Ax2::new(*self.a.location(), *self.a.direction(), &d)?;
        Hypr::new(a, self.minor, self.major)
    }
    /// Directrix1
    pub fn directrix1(&self) -> Result<Ax1, ConstructionError> {
        let e = self.eccentricity()?;
        let mut orig = *self.a.x_direction().xyz();
        orig.multiply(self.major / e);
        orig.add(self.a.location().xyz());
        Ok(Ax1::new(Pnt::from_xyz(orig), *self.a.y_direction()))
    }
    /// Directrix2
    pub fn directrix2(&self) -> Result<Ax1, ConstructionError> {
        let e = self.eccentricity()?;
        let mut orig = *self.a.x_direction().xyz();
        orig.multiply(-self.major / e);
        orig.add(self.a.location().xyz());
        Ok(Ax1::new(Pnt::from_xyz(orig), *self.a.y_direction()))
    }
    /// Eccentricity
    pub fn eccentricity(&self) -> Result<f64, ConstructionError> {
        if self.major <= resolution() {
            return Err(ConstructionError);
        }
        Ok((self.major * self.major + self.minor * self.minor).sqrt() / self.major)
    }
    /// Focal
    pub fn focal(&self) -> f64 {
        2.0 * (self.major * self.major + self.minor * self.minor).sqrt()
    }
    /// Focus1
    pub fn focus1(&self) -> Pnt {
        let c = (self.major * self.major + self.minor * self.minor).sqrt();
        point_along(self.a.location(), c, self.a.x_direction())
    }
    /// Focus2
    pub fn focus2(&self) -> Pnt {
        let c = (self.major * self.major + self.minor * self.minor).sqrt();
        point_back(self.a.location(), c, self.a.x_direction())
    }
    /// MajorRadius
    pub fn major_radius(&self) -> f64 {
        self.major
    }
    /// MinorRadius
    pub fn minor_radius(&self) -> f64 {
        self.minor
    }
    /// OtherBranch
    pub fn other_branch(&self) -> Result<Hypr, ConstructionError> {
        let mut d = *self.a.x_direction();
        d.reverse();
        let a = Ax2::new(*self.a.location(), *self.a.direction(), &d)?;
        Hypr::new(a, self.major, self.minor)
    }
    /// Parameter
    pub fn parameter(&self) -> Result<f64, ConstructionError> {
        if self.major <= resolution() {
            return Err(ConstructionError);
        }
        Ok((self.minor * self.minor) / self.major)
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.major = scaled_radius(self.major, s);
        self.minor = scaled_radius(self.minor, s);
        self.a.scale(p, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Hypr {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.major = scaled_radius(self.major, t.scale_factor());
        self.minor = scaled_radius(self.minor, t.scale_factor());
        self.a.transform(t);
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Hypr {
        let mut c = *self;
        c.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Parab

/// gp_Parab
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parab {
    pub(crate) a: Ax2,
    pub(crate) f: f64,
}

impl Default for Parab {
    // gp_Parab::gp_Parab()
    fn default() -> Self {
        Self { a: Ax2::default(), f: f64::MAX }
    }
}

motion_common!(Parab, ParabMirror, mirror_parab, a);
translate_points_common!(Parab, a);
frame_loc!(Parab, a, Ax2);
frame_set_location!(Parab, a);
frame_xy!(Parab, a);
frame_axis!(Parab, a);

impl Parab {
    // gp_Parab::gp_Parab
    pub fn new(a: Ax2, f: f64) -> Result<Self, ConstructionError> {
        if f < 0.0 { Err(ConstructionError) } else { Ok(Self { a, f }) }
    }
    // gp_Parab::gp_Parab(D, F)
    pub fn from_directrix_focus(d: &Ax1, focus: &Pnt) -> Result<Self, ConstructionError> {
        let line = Lin::from_axis(*d);
        let f = line.distance(focus) / 2.;
        let ax = *line.normal(focus)?.position();
        let ay = *line.position();
        let dd = ax.direction();
        let loc = Pnt::new(focus.x() - f * dd.x(), focus.y() - f * dd.y(), focus.z() - f * dd.z());
        let a = Ax2::new(loc, ax.direction().crossed(ay.direction())?, ax.direction())?;
        Ok(Self { a, f })
    }
    /// SetFocal
    pub fn set_focal(&mut self, f: f64) -> Result<(), ConstructionError> {
        if f < 0.0 {
            return Err(ConstructionError);
        }
        self.f = f;
        Ok(())
    }
    /// Directrix
    pub fn directrix(&self) -> Ax1 {
        let p = point_back(self.a.location(), self.f, self.a.x_direction());
        Ax1::new(p, *self.a.y_direction())
    }
    /// Focal
    pub fn focal(&self) -> f64 {
        self.f
    }
    /// Focus
    pub fn focus(&self) -> Pnt {
        point_along(self.a.location(), self.f, self.a.x_direction())
    }
    /// Parameter
    pub fn parameter(&self) -> f64 {
        2.0 * self.f
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.f = scaled_radius(self.f, s);
        self.a.scale(p, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Parab {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.f = scaled_radius(self.f, t.scale_factor());
        self.a.transform(t);
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Parab {
        let mut c = *self;
        c.transform(t);
        c
    }
}
