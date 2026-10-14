//! gp_Ax2d
use super::{Dir2d, Direction2d, Pnt2d, Trsf2d, Vec2d, XY};

/// gp_Ax2d: an axis in the plane (location and unit direction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ax2d {
    loc: Pnt2d,
    vdir: Dir2d,
}

impl Default for Ax2d {
    // gp_Ax2d::gp_Ax2d()
    fn default() -> Self {
        Self { loc: Pnt2d::new(0., 0.), vdir: Dir2d::default() }
    }
}

/// Things a 2D axis can be mirrored about: a point or an axis.
pub trait Ax2dMirror {
    /// Mirrors `a` about `self` (gp_Ax2d::Mirror).
    fn mirror_ax2d(&self, a: &mut Ax2d);
}

impl Ax2dMirror for Pnt2d {
    // gp_Ax2d::Mirror(gp_Pnt2d)
    fn mirror_ax2d(&self, a: &mut Ax2d) {
        a.loc.mirror(self);
        a.vdir.reverse();
    }
}

impl Ax2dMirror for Ax2d {
    // gp_Ax2d::Mirror(gp_Ax2d)
    fn mirror_ax2d(&self, a: &mut Ax2d) {
        a.loc.mirror(self);
        a.vdir.mirror(&self.vdir);
    }
}

impl Ax2d {
    // gp_Ax2d::gp_Ax2d(P, V)
    pub fn new(p: Pnt2d, v: Dir2d) -> Self {
        Self { loc: p, vdir: v }
    }
    // gp_Ax2d::gp_Ax2d(P, gp_Dir2d::D)
    pub fn from_point_standard(p: Pnt2d, d: Direction2d) -> Self {
        Self { loc: p, vdir: Dir2d::from_direction(d) }
    }
    // gp_Ax2d::gp_Ax2d(gp_Dir2d::D)
    pub fn from_standard_direction(d: Direction2d) -> Self {
        Self { loc: Pnt2d::new(0., 0.), vdir: Dir2d::from_direction(d) }
    }
    // gp_Ax2d::SetLocation
    pub fn set_location(&mut self, p: Pnt2d) {
        self.loc = p;
    }
    // gp_Ax2d::SetDirection
    pub fn set_direction(&mut self, v: Dir2d) {
        self.vdir = v;
    }
    // gp_Ax2d::Location
    pub fn location(&self) -> &Pnt2d {
        &self.loc
    }
    // gp_Ax2d::Direction
    pub fn direction(&self) -> &Dir2d {
        &self.vdir
    }
    // gp_Ax2d::IsCoaxial
    pub fn is_coaxial(&self, other: &Ax2d, angular_tolerance: f64, linear_tolerance: f64) -> bool {
        let mut xy1 = *self.loc.xy();
        xy1.subtract(other.loc.xy());
        let mut d1 = xy1.crossed(other.vdir.xy());
        if d1 < 0.0 {
            d1 = -d1;
        }
        let mut xy2: XY = *other.loc.xy();
        xy2.subtract(self.loc.xy());
        let mut d2 = xy2.crossed(self.vdir.xy());
        if d2 < 0.0 {
            d2 = -d2;
        }
        self.vdir.is_parallel(&other.vdir, angular_tolerance) && d1 <= linear_tolerance && d2 <= linear_tolerance
    }
    // gp_Ax2d::IsNormal
    pub fn is_normal(&self, other: &Ax2d, angular_tolerance: f64) -> bool {
        self.vdir.is_normal(&other.vdir, angular_tolerance)
    }
    // gp_Ax2d::IsOpposite
    pub fn is_opposite(&self, other: &Ax2d, angular_tolerance: f64) -> bool {
        self.vdir.is_opposite(&other.vdir, angular_tolerance)
    }
    // gp_Ax2d::IsParallel
    pub fn is_parallel(&self, other: &Ax2d, angular_tolerance: f64) -> bool {
        self.vdir.is_parallel(&other.vdir, angular_tolerance)
    }
    // gp_Ax2d::Angle
    pub fn angle(&self, other: &Ax2d) -> f64 {
        self.vdir.angle(&other.vdir)
    }
    // gp_Ax2d::Reverse
    pub fn reverse(&mut self) {
        self.vdir.reverse();
    }
    // gp_Ax2d::Reversed
    pub fn reversed(&self) -> Ax2d {
        let mut a = *self;
        a.reverse();
        a
    }
    // gp_Ax2d::Mirror
    pub fn mirror<M: Ax2dMirror>(&mut self, m: &M) {
        m.mirror_ax2d(self);
    }
    // gp_Ax2d::Mirrored
    pub fn mirrored<M: Ax2dMirror>(&self, m: &M) -> Ax2d {
        let mut a = *self;
        m.mirror_ax2d(&mut a);
        a
    }
    // gp_Ax2d::Rotate
    pub fn rotate(&mut self, p: &Pnt2d, ang: f64) {
        self.loc.rotate(p, ang);
        self.vdir.rotate(ang);
    }
    // gp_Ax2d::Rotated
    pub fn rotated(&self, p: &Pnt2d, ang: f64) -> Ax2d {
        let mut a = *self;
        a.rotate(p, ang);
        a
    }
    // gp_Ax2d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        self.loc.scale(p, s);
        if s < 0.0 {
            self.vdir.reverse();
        }
    }
    // gp_Ax2d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Ax2d {
        let mut a = *self;
        a.scale(p, s);
        a
    }
    // gp_Ax2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        self.loc.transform(t);
        self.vdir.transform(t);
    }
    // gp_Ax2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Ax2d {
        let mut a = *self;
        a.transform(t);
        a
    }
    // gp_Ax2d::Translate(gp_Vec2d)
    pub fn translate(&mut self, v: &Vec2d) {
        self.loc.translate(v);
    }
    // gp_Ax2d::Translated(gp_Vec2d)
    pub fn translated(&self, v: &Vec2d) -> Ax2d {
        let mut a = *self;
        a.loc.translate(v);
        a
    }
    // gp_Ax2d::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
        self.loc.translate_points(p1, p2);
    }
    // gp_Ax2d::Translated(P1, P2)
    pub fn translated_points(&self, p1: &Pnt2d, p2: &Pnt2d) -> Ax2d {
        let mut a = *self;
        a.loc.translate_points(p1, p2);
        a
    }
}
