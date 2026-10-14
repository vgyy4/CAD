//! gp_Ax1
use super::{Ax2, Dir, Direction, Pnt, Trsf, Vec, XYZ};

/// gp_Ax1: a point and a direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ax1 {
    pub(crate) loc: Pnt,
    pub(crate) dir: Dir,
}

impl Default for Ax1 {
    // gp_Ax1::gp_Ax1(): the Z axis
    fn default() -> Self {
        Self { loc: Pnt::new(0., 0., 0.), dir: Dir::from_direction(Direction::Z) }
    }
}

/// Things an axis can be mirrored about: a point, an axis or a plane (an `Ax2`).
pub trait Ax1Mirror {
    /// Mirrors `a` about `self` (gp_Ax1::Mirror).
    fn mirror_ax1(&self, a: &mut Ax1);
}

impl Ax1Mirror for Pnt {
    // gp_Ax1::Mirror(gp_Pnt)
    fn mirror_ax1(&self, a: &mut Ax1) {
        a.loc.mirror(self);
        a.dir.reverse();
    }
}

impl Ax1Mirror for Ax1 {
    // gp_Ax1::Mirror(gp_Ax1)
    fn mirror_ax1(&self, a: &mut Ax1) {
        a.loc.mirror(self);
        a.dir.mirror(&self.dir);
    }
}

impl Ax1Mirror for Ax2 {
    // gp_Ax1::Mirror(gp_Ax2)
    fn mirror_ax1(&self, a: &mut Ax1) {
        a.loc.mirror(self);
        a.dir.mirror(self);
    }
}

impl Ax1 {
    // gp_Ax1::gp_Ax1(P, V)
    pub fn new(p: Pnt, v: Dir) -> Self {
        Self { loc: p, dir: v }
    }
    // gp_Ax1::gp_Ax1(gp_Dir::D)
    pub fn from_direction(d: Direction) -> Self {
        Self { loc: Pnt::new(0., 0., 0.), dir: Dir::from_direction(d) }
    }
    // gp_Ax1::gp_Ax1(P, gp_Dir::D)
    pub fn at(p: Pnt, d: Direction) -> Self {
        Self { loc: p, dir: Dir::from_direction(d) }
    }
    // gp_Ax1::SetDirection
    pub fn set_direction(&mut self, v: Dir) {
        self.dir = v;
    }
    // gp_Ax1::SetLocation
    pub fn set_location(&mut self, p: Pnt) {
        self.loc = p;
    }
    // gp_Ax1::Direction
    pub fn direction(&self) -> &Dir {
        &self.dir
    }
    // gp_Ax1::Location
    pub fn location(&self) -> &Pnt {
        &self.loc
    }
    // gp_Ax1::IsCoaxial
    pub fn is_coaxial(&self, other: &Ax1, angular_tolerance: f64, linear_tolerance: f64) -> bool {
        let mut xyz1: XYZ = *self.loc.xyz();
        xyz1.subtract(other.loc.xyz());
        xyz1.cross(other.dir.xyz());
        let d1 = xyz1.modulus();
        let mut xyz2: XYZ = *other.loc.xyz();
        xyz2.subtract(self.loc.xyz());
        xyz2.cross(self.dir.xyz());
        let d2 = xyz2.modulus();
        self.dir.is_equal(&other.dir, angular_tolerance) && d1 <= linear_tolerance && d2 <= linear_tolerance
    }
    // gp_Ax1::IsNormal
    pub fn is_normal(&self, other: &Ax1, angular_tolerance: f64) -> bool {
        self.dir.is_normal(&other.dir, angular_tolerance)
    }
    // gp_Ax1::IsOpposite
    pub fn is_opposite(&self, other: &Ax1, angular_tolerance: f64) -> bool {
        self.dir.is_opposite(&other.dir, angular_tolerance)
    }
    // gp_Ax1::IsParallel
    pub fn is_parallel(&self, other: &Ax1, angular_tolerance: f64) -> bool {
        self.dir.is_parallel(&other.dir, angular_tolerance)
    }
    // gp_Ax1::Angle
    pub fn angle(&self, other: &Ax1) -> f64 {
        self.dir.angle(&other.dir)
    }
    // gp_Ax1::Reverse
    pub fn reverse(&mut self) {
        self.dir.reverse();
    }
    // gp_Ax1::Reversed
    pub fn reversed(&self) -> Ax1 {
        Ax1 { loc: self.loc, dir: self.dir.reversed() }
    }
    // gp_Ax1::Mirror
    pub fn mirror<M: Ax1Mirror>(&mut self, m: &M) {
        m.mirror_ax1(self);
    }
    // gp_Ax1::Mirrored
    pub fn mirrored<M: Ax1Mirror>(&self, m: &M) -> Ax1 {
        let mut a = *self;
        m.mirror_ax1(&mut a);
        a
    }
    // gp_Ax1::Rotate
    pub fn rotate(&mut self, a1: &Ax1, ang: f64) {
        self.loc.rotate(a1, ang);
        self.dir.rotate(a1, ang);
    }
    // gp_Ax1::Rotated
    pub fn rotated(&self, a1: &Ax1, ang: f64) -> Ax1 {
        let mut a = *self;
        a.rotate(a1, ang);
        a
    }
    // gp_Ax1::Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.loc.scale(p, s);
        if s < 0.0 {
            self.dir.reverse();
        }
    }
    // gp_Ax1::Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Ax1 {
        let mut a = *self;
        a.scale(p, s);
        a
    }
    // gp_Ax1::Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.loc.transform(t);
        self.dir.transform_raw(t);
    }
    // gp_Ax1::Transformed
    pub fn transformed(&self, t: &Trsf) -> Ax1 {
        let mut a = *self;
        a.transform(t);
        a
    }
    // gp_Ax1::Translate(gp_Vec)
    pub fn translate(&mut self, v: &Vec) {
        self.loc.translate(v);
    }
    // gp_Ax1::Translated(gp_Vec)
    pub fn translated(&self, v: &Vec) -> Ax1 {
        let mut a = *self;
        a.loc.translate(v);
        a
    }
    // gp_Ax1::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt, p2: &Pnt) {
        self.loc.translate_points(p1, p2);
    }
    // gp_Ax1::Translated(P1, P2)
    pub fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> Ax1 {
        let mut a = *self;
        a.loc.translate_points(p1, p2);
        a
    }
}
