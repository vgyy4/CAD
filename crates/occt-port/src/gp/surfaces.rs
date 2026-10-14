//! gp_Pln, gp_Cylinder, gp_Cone, gp_Sphere, gp_Torus.
use super::macros::scaled_radius;
use super::{Ax1, Ax2, Ax3, ConstructionError, Dir, Lin, Pnt, Trsf, Vec, resolution};
use std::f64::consts::PI;

/// `gp_Trsf::Value(row, col)` for valid indices (the coefficient code only asks for those).
fn tv(t: &Trsf, row: i32, col: i32) -> f64 {
    t.value(row, col).unwrap_or(0.0)
}

// ---------------------------------------------------------------------------------------------
// gp_Pln

/// The argument types of `gp_Pln::SignedDistance` / `Distance` / `SquareDistance`: a point, a line
/// or another plane.
pub trait PlnDistance {
    /// `plane.SignedDistance(self)`
    fn signed_distance_from_pln(&self, plane: &Pln) -> f64;
}

/// gp_Pln
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pln {
    pos: Ax3,
}

motion_common!(Pln, PlnMirror, mirror_pln, pos);
translate_points_common!(Pln, pos);
frame_loc!(Pln, pos, Ax3);
frame_set_location!(Pln, pos);
frame_xy!(Pln, pos);
frame_axis!(Pln, pos);

impl PlnDistance for Pnt {
    // gp_Pln::SignedDistance(gp_Pnt)
    fn signed_distance_from_pln(&self, plane: &Pln) -> f64 {
        let loc = plane.pos.location();
        let dir = plane.pos.direction();
        dir.x() * (self.x() - loc.x()) + dir.y() * (self.y() - loc.y()) + dir.z() * (self.z() - loc.z())
    }
}

impl PlnDistance for Lin {
    // gp_Pln::SignedDistance(gp_Lin)
    fn signed_distance_from_pln(&self, plane: &Pln) -> f64 {
        if !plane.pos.direction().is_normal(self.direction(), resolution()) {
            return 0.;
        }
        self.location().signed_distance_from_pln(plane)
    }
}

impl PlnDistance for Pln {
    // gp_Pln::SignedDistance(gp_Pln)
    fn signed_distance_from_pln(&self, plane: &Pln) -> f64 {
        if !plane.pos.direction().is_parallel(self.pos.direction(), resolution()) {
            return 0.;
        }
        self.pos.location().signed_distance_from_pln(plane)
    }
}

impl Pln {
    // gp_Pln::gp_Pln(Ax3)
    pub fn new(a: Ax3) -> Self {
        Self { pos: a }
    }
    // gp_Pln::gp_Pln(P, V)
    pub fn from_point_normal(p: Pnt, v: Dir) -> Self {
        let (a, b, c) = (v.x(), v.y(), v.z());
        let (abs_a, abs_b, abs_c) = (a.abs(), b.abs(), c.abs());
        let x = if abs_b <= abs_a && abs_b <= abs_c {
            if abs_a > abs_c { Dir::new(-c, 0., a) } else { Dir::new(c, 0., -a) }
        } else if abs_a <= abs_b && abs_a <= abs_c {
            if abs_b > abs_c { Dir::new(0., -c, b) } else { Dir::new(0., c, -b) }
        } else if abs_a > abs_b {
            Dir::new(-b, a, 0.)
        } else {
            Dir::new(b, -a, 0.)
        };
        // `v` is a unit direction, so the chosen X direction has a component of at least 1/sqrt(3)
        // and neither constructor can fail.
        let pos = x.and_then(|x| Ax3::new(p, v, x)).unwrap_or_default();
        Self { pos }
    }
    // gp_Pln::gp_Pln(A, B, C, D)
    //
    // OCCT 8.0.1 as built does not check the norm of the normal (it returns a frame full of NaN); the
    // contract is an error when it is zero, which `Dir::new` reports.
    pub fn from_coefficients(a: f64, b: f64, c: f64, d: f64) -> Result<Self, ConstructionError> {
        let (abs_a, abs_b, abs_c) = (a.abs(), b.abs(), c.abs());
        let pos = if abs_b <= abs_a && abs_b <= abs_c {
            if abs_a > abs_c {
                Ax3::new(Pnt::new(-d / a, 0., 0.), Dir::new(a, b, c)?, Dir::new(-c, 0., a)?)?
            } else {
                Ax3::new(Pnt::new(0., 0., -d / c), Dir::new(a, b, c)?, Dir::new(c, 0., -a)?)?
            }
        } else if abs_a <= abs_b && abs_a <= abs_c {
            if abs_b > abs_c {
                Ax3::new(Pnt::new(0., -d / b, 0.), Dir::new(a, b, c)?, Dir::new(0., -c, b)?)?
            } else {
                Ax3::new(Pnt::new(0., 0., -d / c), Dir::new(a, b, c)?, Dir::new(0., c, -b)?)?
            }
        } else if abs_a > abs_b {
            Ax3::new(Pnt::new(-d / a, 0., 0.), Dir::new(a, b, c)?, Dir::new(-b, a, 0.)?)?
        } else {
            Ax3::new(Pnt::new(0., -d / b, 0.), Dir::new(a, b, c)?, Dir::new(b, -a, 0.)?)?
        };
        Ok(Self { pos })
    }
    /// Coefficients
    pub fn coefficients(&self) -> (f64, f64, f64, f64) {
        let dir = self.pos.direction();
        let (a, b, c) = if self.pos.direct() { (dir.x(), dir.y(), dir.z()) } else { (-dir.x(), -dir.y(), -dir.z()) };
        let p = self.pos.location();
        let d = -(a * p.x() + b * p.y() + c * p.z());
        (a, b, c, d)
    }
    /// UReverse
    pub fn u_reverse(&mut self) {
        self.pos.x_reverse();
    }
    /// VReverse
    pub fn v_reverse(&mut self) {
        self.pos.y_reverse();
    }
    /// Direct
    pub fn direct(&self) -> bool {
        self.pos.direct()
    }
    /// SignedDistance (a point, a line or a plane)
    pub fn signed_distance<D: PlnDistance>(&self, d: &D) -> f64 {
        d.signed_distance_from_pln(self)
    }
    /// Distance (a point, a line or a plane)
    pub fn distance<D: PlnDistance>(&self, d: &D) -> f64 {
        self.signed_distance(d).abs()
    }
    /// SquareDistance (a point, a line or a plane)
    pub fn square_distance<D: PlnDistance>(&self, d: &D) -> f64 {
        let d = self.distance(d);
        d * d
    }
    /// Contains(gp_Pnt)
    pub fn contains(&self, p: &Pnt, linear_tolerance: f64) -> bool {
        self.distance(p) <= linear_tolerance
    }
    /// Contains(gp_Lin)
    pub fn contains_line(&self, l: &Lin, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        self.contains(l.location(), linear_tolerance)
            && self.pos.direction().is_normal(l.direction(), angular_tolerance)
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.pos.scale(p, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Pln {
        let mut c = *self;
        c.pos.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.pos.transform(t);
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Pln {
        let mut c = *self;
        c.pos.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Cylinder

/// gp_Cylinder
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    pos: Ax3,
    radius: f64,
}

impl Default for Cylinder {
    // gp_Cylinder::gp_Cylinder()
    fn default() -> Self {
        Self { pos: Ax3::default(), radius: f64::MAX }
    }
}

motion_common!(Cylinder, CylinderMirror, mirror_cylinder, pos);
translate_points_common!(Cylinder, pos);
frame_loc!(Cylinder, pos, Ax3);
frame_set_location!(Cylinder, pos);
frame_xy!(Cylinder, pos);
frame_axis!(Cylinder, pos);

impl Cylinder {
    // gp_Cylinder::gp_Cylinder(A3, Radius)
    pub fn new(a: Ax3, radius: f64) -> Result<Self, ConstructionError> {
        if radius < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: a, radius })
    }
    /// SetRadius
    pub fn set_radius(&mut self, r: f64) -> Result<(), ConstructionError> {
        if r < 0.0 {
            return Err(ConstructionError);
        }
        self.radius = r;
        Ok(())
    }
    /// UReverse
    pub fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    /// VReverse
    pub fn v_reverse(&mut self) {
        self.pos.z_reverse();
    }
    /// Direct
    pub fn direct(&self) -> bool {
        self.pos.direct()
    }
    /// Radius
    pub fn radius(&self) -> f64 {
        self.radius
    }
    /// Coefficients: (A1, A2, A3, B1, B2, B3, C1, C2, C3, D)
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) {
        let mut t = Trsf::new();
        t.set_transformation(&self.pos);
        let (t11, t12, t13, t14) = (tv(&t, 1, 1), tv(&t, 1, 2), tv(&t, 1, 3), tv(&t, 1, 4));
        let (t21, t22, t23, t24) = (tv(&t, 2, 1), tv(&t, 2, 2), tv(&t, 2, 3), tv(&t, 2, 4));
        (
            t11 * t11 + t21 * t21,
            t12 * t12 + t22 * t22,
            t13 * t13 + t23 * t23,
            t11 * t12 + t21 * t22,
            t11 * t13 + t21 * t23,
            t12 * t13 + t22 * t23,
            t11 * t14 + t21 * t24,
            t12 * t14 + t22 * t24,
            t13 * t14 + t23 * t24,
            t14 * t14 + t24 * t24 - self.radius * self.radius,
        )
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.pos.scale(p, s);
        self.radius = scaled_radius(self.radius, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Cylinder {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.pos.transform(t);
        self.radius = scaled_radius(self.radius, t.scale_factor());
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Cylinder {
        let mut c = *self;
        c.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Cone

/// gp_Cone
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone {
    pos: Ax3,
    radius: f64,
    semi_angle: f64,
}

impl Default for Cone {
    // gp_Cone::gp_Cone()
    fn default() -> Self {
        Self { pos: Ax3::default(), radius: f64::MAX, semi_angle: PI * 0.25 }
    }
}

motion_common!(Cone, ConeMirror, mirror_cone, pos);
translate_points_common!(Cone, pos);
frame_loc!(Cone, pos, Ax3);
frame_set_location!(Cone, pos);
frame_xy!(Cone, pos);
frame_axis!(Cone, pos);

impl Cone {
    // gp_Cone::gp_Cone(A3, Ang, Radius)
    pub fn new(a: Ax3, semi_angle: f64, radius: f64) -> Result<Self, ConstructionError> {
        let abs_ang = if semi_angle < 0. { -semi_angle } else { semi_angle };
        if radius < 0. || abs_ang <= resolution() || PI * 0.5 - abs_ang <= resolution() {
            return Err(ConstructionError);
        }
        Ok(Self { pos: a, radius, semi_angle })
    }
    /// SetRadius
    pub fn set_radius(&mut self, r: f64) -> Result<(), ConstructionError> {
        if r < 0.0 {
            return Err(ConstructionError);
        }
        self.radius = r;
        Ok(())
    }
    /// SetSemiAngle
    pub fn set_semi_angle(&mut self, ang: f64) -> Result<(), ConstructionError> {
        if ang.abs() <= resolution() || PI * 0.5 - ang.abs() <= resolution() {
            return Err(ConstructionError);
        }
        self.semi_angle = ang;
        Ok(())
    }
    /// Apex
    pub fn apex(&self) -> Pnt {
        let mut coord = *self.pos.direction().xyz();
        coord.multiply(-self.radius / libm::tan(self.semi_angle));
        coord.add(self.pos.location().xyz());
        Pnt::from_xyz(coord)
    }
    /// UReverse
    pub fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    /// VReverse
    pub fn v_reverse(&mut self) {
        self.pos.z_reverse();
        self.semi_angle = -self.semi_angle;
    }
    /// Direct
    pub fn direct(&self) -> bool {
        self.pos.direct()
    }
    /// RefRadius
    pub fn ref_radius(&self) -> f64 {
        self.radius
    }
    /// SemiAngle
    pub fn semi_angle(&self) -> f64 {
        self.semi_angle
    }
    /// Coefficients: (A1, A2, A3, B1, B2, B3, C1, C2, C3, D)
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) {
        let mut t = Trsf::new();
        t.set_transformation(&self.pos);
        let k_ang = libm::tan(self.semi_angle);
        let (t11, t12, t13, t14) = (tv(&t, 1, 1), tv(&t, 1, 2), tv(&t, 1, 3), tv(&t, 1, 4));
        let (t21, t22, t23, t24) = (tv(&t, 2, 1), tv(&t, 2, 2), tv(&t, 2, 3), tv(&t, 2, 4));
        let t31 = tv(&t, 3, 1) * k_ang;
        let t32 = tv(&t, 3, 2) * k_ang;
        let t33 = tv(&t, 3, 3) * k_ang;
        let t34 = tv(&t, 3, 4) * k_ang;
        let r = self.radius;
        (
            t11 * t11 + t21 * t21 - t31 * t31,
            t12 * t12 + t22 * t22 - t32 * t32,
            t13 * t13 + t23 * t23 - t33 * t33,
            t11 * t12 + t21 * t22 - t31 * t32,
            t11 * t13 + t21 * t23 - t31 * t33,
            t12 * t13 + t22 * t23 - t32 * t33,
            t11 * t14 + t21 * t24 - t31 * (r + t34),
            t12 * t14 + t22 * t24 - t32 * (r + t34),
            t13 * t14 + t23 * t24 - t33 * (r + t34),
            t14 * t14 + t24 * t24 - r * r - t34 * t34 - 2.0 * r * t34,
        )
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.pos.scale(p, s);
        self.radius = scaled_radius(self.radius, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Cone {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.pos.transform(t);
        self.radius = scaled_radius(self.radius, t.scale_factor());
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Cone {
        let mut c = *self;
        c.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Sphere

/// gp_Sphere
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pos: Ax3,
    radius: f64,
}

impl Default for Sphere {
    // gp_Sphere::gp_Sphere()
    fn default() -> Self {
        Self { pos: Ax3::default(), radius: f64::MAX }
    }
}

motion_common!(Sphere, SphereMirror, mirror_sphere, pos);
translate_points_common!(Sphere, pos);
frame_loc!(Sphere, pos, Ax3);
frame_set_location!(Sphere, pos);
frame_xy!(Sphere, pos);

impl Sphere {
    // gp_Sphere::gp_Sphere(A3, Radius)
    pub fn new(a: Ax3, radius: f64) -> Result<Self, ConstructionError> {
        if radius < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: a, radius })
    }
    /// SetRadius
    pub fn set_radius(&mut self, r: f64) -> Result<(), ConstructionError> {
        if r < 0.0 {
            return Err(ConstructionError);
        }
        self.radius = r;
        Ok(())
    }
    /// Area
    pub fn area(&self) -> f64 {
        4.0 * PI * self.radius * self.radius
    }
    /// UReverse
    pub fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    /// VReverse
    pub fn v_reverse(&mut self) {
        self.pos.z_reverse();
    }
    /// Direct
    pub fn direct(&self) -> bool {
        self.pos.direct()
    }
    /// Radius
    pub fn radius(&self) -> f64 {
        self.radius
    }
    /// Volume
    pub fn volume(&self) -> f64 {
        (4.0 * PI * self.radius * self.radius * self.radius) / 3.0
    }
    /// Coefficients: (A1, A2, A3, B1, B2, B3, C1, C2, C3, D)
    pub fn coefficients(&self) -> (f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) {
        let mut t = Trsf::new();
        t.set_transformation(&self.pos);
        let (t11, t12, t13, t14) = (tv(&t, 1, 1), tv(&t, 1, 2), tv(&t, 1, 3), tv(&t, 1, 4));
        let (t21, t22, t23, t24) = (tv(&t, 2, 1), tv(&t, 2, 2), tv(&t, 2, 3), tv(&t, 2, 4));
        let (t31, t32, t33, t34) = (tv(&t, 3, 1), tv(&t, 3, 2), tv(&t, 3, 3), tv(&t, 3, 4));
        (
            t11 * t11 + t21 * t21 + t31 * t31,
            t12 * t12 + t22 * t22 + t32 * t32,
            t13 * t13 + t23 * t23 + t33 * t33,
            t11 * t12 + t21 * t22 + t31 * t32,
            t11 * t13 + t21 * t23 + t31 * t33,
            t12 * t13 + t22 * t23 + t32 * t33,
            t11 * t14 + t21 * t24 + t31 * t34,
            t12 * t14 + t22 * t24 + t32 * t34,
            t13 * t14 + t23 * t24 + t33 * t34,
            t14 * t14 + t24 * t24 + t34 * t34 - self.radius * self.radius,
        )
    }
    /// Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.pos.scale(p, s);
        self.radius = scaled_radius(self.radius, s);
    }
    /// Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Sphere {
        let mut c = *self;
        c.scale(p, s);
        c
    }
    /// Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.pos.transform(t);
        self.radius = scaled_radius(self.radius, t.scale_factor());
    }
    /// Transformed
    pub fn transformed(&self, t: &Trsf) -> Sphere {
        let mut c = *self;
        c.transform(t);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// gp_Torus

/// gp_Torus
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Torus {
    pos: Ax3,
    major: f64,
    minor: f64,
}

impl Default for Torus {
    // gp_Torus::gp_Torus()
    fn default() -> Self {
        Self { pos: Ax3::default(), major: f64::MAX, minor: resolution() }
    }
}

motion_common!(Torus, TorusMirror, mirror_torus, pos);
translate_points_common!(Torus, pos);
frame_loc!(Torus, pos, Ax3);
frame_set_location!(Torus, pos);
frame_xy!(Torus, pos);
frame_axis!(Torus, pos);

impl Torus {
    // gp_Torus::gp_Torus(A3, MajorRadius, MinorRadius)
    pub fn new(a: Ax3, major: f64, minor: f64) -> Result<Self, ConstructionError> {
        if minor < 0.0 || major < 0.0 {
            return Err(ConstructionError);
        }
        Ok(Self { pos: a, major, minor })
    }
    /// SetMajorRadius
    pub fn set_major_radius(&mut self, major: f64) -> Result<(), ConstructionError> {
        if major - self.minor <= resolution() {
            return Err(ConstructionError);
        }
        self.major = major;
        Ok(())
    }
    /// SetMinorRadius
    pub fn set_minor_radius(&mut self, minor: f64) -> Result<(), ConstructionError> {
        if minor < 0.0 || self.major - minor <= resolution() {
            return Err(ConstructionError);
        }
        self.minor = minor;
        Ok(())
    }
    /// Area
    pub fn area(&self) -> f64 {
        4.0 * PI * PI * self.minor * self.major
    }
    /// UReverse
    pub fn u_reverse(&mut self) {
        self.pos.y_reverse();
    }
    /// VReverse
    pub fn v_reverse(&mut self) {
        self.pos.z_reverse();
    }
    /// Direct
    pub fn direct(&self) -> bool {
        self.pos.direct()
    }
    /// MajorRadius
    pub fn major_radius(&self) -> f64 {
        self.major
    }
    /// MinorRadius
    pub fn minor_radius(&self) -> f64 {
        self.minor
    }
    /// Volume
    pub fn volume(&self) -> f64 {
        (PI * self.minor * self.minor) * (2.0 * PI * self.major)
    }
    /// Coefficients (the array overload; the 35 coefficients of the quartic)
    pub fn coefficients(&self) -> [f64; 35] {
        let mut tr = Trsf::new();
        tr.set_transformation(&self.pos);
        let (t11, t12, t13, t14) = (tv(&tr, 1, 1), tv(&tr, 1, 2), tv(&tr, 1, 3), tv(&tr, 1, 4));
        let (t21, t22, t23, t24) = (tv(&tr, 2, 1), tv(&tr, 2, 2), tv(&tr, 2, 3), tv(&tr, 2, 4));
        let (t31, t32, t33, t34) = (tv(&tr, 3, 1), tv(&tr, 3, 2), tv(&tr, 3, 3), tv(&tr, 3, 4));
        let col1sq = t11 * t11 + t21 * t21 + t31 * t31;
        let col2sq = t12 * t12 + t22 * t22 + t32 * t32;
        let col3sq = t13 * t13 + t23 * t23 + t33 * t33;
        let col4sq = t14 * t14 + t24 * t24 + t34 * t34;
        let col1_col2 = t11 * t12 + t21 * t22 + t31 * t32;
        let col1_col3 = t11 * t13 + t21 * t23 + t31 * t33;
        let col2_col3 = t12 * t13 + t22 * t23 + t32 * t33;
        let col1_col4 = t11 * t14 + t21 * t24 + t31 * t34;
        let col2_col4 = t12 * t14 + t22 * t24 + t32 * t34;
        let col3_col4 = t13 * t14 + t23 * t24 + t33 * t34;
        let sum_radius = self.major * self.major + self.minor * self.minor;
        let sub_radius = self.major * self.major - self.minor * self.minor;
        [
            col1sq * col1sq,                                                               // X^4
            col2sq * col2sq,                                                               // Y^4
            col3sq * col3sq,                                                               // Z^4
            4.0 * col1sq * col1_col2,                                                      // X^3*Y
            4.0 * col1sq * col1_col3,                                                      // X^3*Z
            4.0 * col2sq * col1_col2,                                                      // X*Y^3
            4.0 * col2sq * col2_col3,                                                      // Y^3*Z
            4.0 * col3sq * col1_col3,                                                      // X*Z^3
            4.0 * col3sq * col2_col3,                                                      // Y*Z^3
            2.0 * (col1sq * col2sq + 2.0 * col1_col2 * col1_col2),                         // X^2*Y^2
            2.0 * (col1sq * col3sq + 2.0 * col1_col3 * col1_col3),                         // X^2*Z^2
            2.0 * (col2sq * col3sq + 2.0 * col2_col3 * col2_col3),                         // Y^2*Z^2
            4.0 * (col1sq * col2_col3 + 2.0 * col1_col2 * col1_col3),                      // X^2*Y*Z
            4.0 * (col2sq * col1_col3 + 2.0 * col1_col2 * col2_col3),                      // X*Y^2*Z
            4.0 * (col3sq * col1_col2 + 2.0 * col1_col3 * col2_col3),                      // X*Y*Z^2
            4.0 * col1sq * col1_col4,                                                      // X^3
            4.0 * col2sq * col2_col4,                                                      // Y^3
            4.0 * col3sq * col3_col4,                                                      // Z^3
            4.0 * (col1sq * col2_col4 + 2.0 * col1_col4 * col1_col2),                      // X^2*Y
            4.0 * (col1sq * col3_col4 + 2.0 * col1_col4 * col1_col3),                      // X^2*Z
            4.0 * (col2sq * col1_col4 + 2.0 * col2_col4 * col1_col2),                      // X*Y^2
            4.0 * (col2sq * col3_col4 + 2.0 * col2_col4 * col2_col3),                      // Y^2*Z
            4.0 * (col3sq * col1_col4 + 2.0 * col3_col4 * col1_col3),                      // X*Z^2
            4.0 * (col3sq * col2_col4 + 2.0 * col3_col4 * col2_col3),                      // Y*Z^2
            8.0 * (col1_col2 * col3_col4 + col2_col3 * col1_col4 + col2_col4 * col1_col3), // X*Y*Z
            2.0 * (sub_radius * t31 * t31 - sum_radius * (t11 * t11 + t21 * t21)
                + col4sq * col1sq
                + 2.0 * col1_col4 * col1_col4), // X^2
            2.0 * (sub_radius * t32 * t32 - sum_radius * (t12 * t12 + t22 * t22)
                + col4sq * col2sq
                + 2.0 * col2_col4 * col2_col4), // Y^2
            2.0 * (sub_radius * t33 * t33 - sum_radius * (t13 * t13 + t23 * t23)
                + col4sq * col3sq
                + 2.0 * col3_col4 * col3_col4), // Z^2
            4.0 * (sub_radius * t31 * t32 - sum_radius * (t11 * t12 + t21 * t22)
                + col4sq * col1_col2
                + 2.0 * col1_col4 * col2_col4), // X*Y
            4.0 * (sub_radius * t31 * t33 - sum_radius * (t11 * t13 + t21 * t23)
                + col4sq * col1_col3
                + 2.0 * col1_col4 * col3_col4), // X*Z
            4.0 * (sub_radius * t32 * t33 - sum_radius * (t12 * t13 + t22 * t23)
                + col4sq * col2_col3
                + 2.0 * col2_col4 * col3_col4), // Y*Z
            4.0 * (col4sq * col1_col4 + sub_radius * t31 * t34 - sum_radius * (t11 * t14 + t21 * t24)), // X
            4.0 * (col4sq * col2_col4 + sub_radius * t32 * t34 - sum_radius * (t12 * t14 + t22 * t24)), // Y
            4.0 * (col4sq * col3_col4 + sub_radius * t33 * t34 - sum_radius * (t13 * t14 + t23 * t24)), // Z
            2.0 * sub_radius * t34 * t34 - 2.0 * sum_radius * (t14 * t14 + t24 * t24)
                + col4sq * col4sq
                + sub_radius * sub_radius,
        ]
    }
    /// Scale: the in-place form multiplies by the absolute value of the factor.
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.pos.scale(p, s);
        let s = if s < 0. { -s } else { s };
        self.major *= s;
        self.minor *= s;
    }
    /// Scaled: the copy form multiplies by the factor and takes the absolute value only `if radius < 0`.
    pub fn scaled(&self, p: &Pnt, s: f64) -> Torus {
        let mut c = *self;
        c.pos.scale(p, s);
        c.major = scaled_radius(c.major, s);
        c.minor = scaled_radius(c.minor, s);
        c
    }
    /// Transform: the in-place form multiplies by the absolute value of the scale factor.
    pub fn transform(&mut self, t: &Trsf) {
        self.pos.transform(t);
        let s = t.scale_factor();
        let s = if s < 0. { -s } else { s };
        self.minor *= s;
        self.major *= s;
    }
    /// Transformed: the copy form multiplies by the factor and takes the absolute value only `if radius < 0`.
    pub fn transformed(&self, t: &Trsf) -> Torus {
        let mut c = *self;
        c.pos.transform(t);
        c.major = scaled_radius(c.major, t.scale_factor());
        c.minor = scaled_radius(c.minor, t.scale_factor());
        c
    }
}
