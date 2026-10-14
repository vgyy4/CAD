//! gp_Lin2d
use super::{Ax2d, ConstructionError, Dir2d, Pnt2d, Trsf2d, Vec2d, resolution};

/// gp_Lin2d: a line in the plane.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lin2d {
    pos: Ax2d,
}

/// Things a 2D line can be mirrored about: a point or an axis.
pub trait Lin2dMirror {
    /// Mirrors `l` about `self` (gp_Lin2d::Mirror).
    fn mirror_lin2d(&self, l: &mut Lin2d);
}

impl Lin2dMirror for Pnt2d {
    // gp_Lin2d::Mirror(gp_Pnt2d)
    fn mirror_lin2d(&self, l: &mut Lin2d) {
        l.pos.mirror(self);
    }
}

impl Lin2dMirror for Ax2d {
    // gp_Lin2d::Mirror(gp_Ax2d)
    fn mirror_lin2d(&self, l: &mut Lin2d) {
        l.pos.mirror(self);
    }
}

/// Things the distance from a 2D line can be measured to: a point or another line.
pub trait Lin2dDistance {
    /// gp_Lin2d::Distance
    fn distance_from_lin2d(&self, l: &Lin2d) -> f64;
    /// gp_Lin2d::SquareDistance
    fn square_distance_from_lin2d(&self, l: &Lin2d) -> f64;
}

impl Lin2dDistance for Pnt2d {
    // gp_Lin2d::Distance(gp_Pnt2d)
    fn distance_from_lin2d(&self, l: &Lin2d) -> f64 {
        let mut coord = *self.xy();
        coord.subtract(l.pos.location().xy());
        coord.crossed(l.pos.direction().xy()).abs()
    }
    // gp_Lin2d::SquareDistance(gp_Pnt2d)
    fn square_distance_from_lin2d(&self, l: &Lin2d) -> f64 {
        let mut coord = *self.xy();
        coord.subtract(l.pos.location().xy());
        let d = coord.crossed(l.pos.direction().xy());
        d * d
    }
}

impl Lin2dDistance for Lin2d {
    // gp_Lin2d::Distance(gp_Lin2d)
    fn distance_from_lin2d(&self, l: &Lin2d) -> f64 {
        let mut d = 0.0;
        if l.pos.is_parallel(&self.pos, resolution()) {
            d = self.distance_to_point(l.pos.location());
        }
        d
    }
    // gp_Lin2d::SquareDistance(gp_Lin2d)
    fn square_distance_from_lin2d(&self, l: &Lin2d) -> f64 {
        let mut d = 0.0;
        if l.pos.is_parallel(&self.pos, resolution()) {
            d = self.square_distance_to_point(l.pos.location());
        }
        d
    }
}

impl Lin2d {
    // gp_Lin2d::gp_Lin2d(P, V)
    pub fn new(p: Pnt2d, v: Dir2d) -> Self {
        Self { pos: Ax2d::new(p, v) }
    }
    // gp_Lin2d::gp_Lin2d(A)
    pub fn from_ax2d(a: Ax2d) -> Self {
        Self { pos: a }
    }
    // gp_Lin2d::gp_Lin2d(A, B, C); the line a*x + b*y + c = 0
    pub fn from_coefficients(a: f64, b: f64, c: f64) -> Result<Self, ConstructionError> {
        let norm2 = a * a + b * b;
        if norm2 <= resolution() {
            return Err(ConstructionError);
        }
        let p = Pnt2d::new(-a * c / norm2, -b * c / norm2);
        let v = Dir2d::new(-b, a)?;
        Ok(Self { pos: Ax2d::new(p, v) })
    }
    // gp_Lin2d::Reverse
    pub fn reverse(&mut self) {
        self.pos.reverse();
    }
    // gp_Lin2d::Reversed
    pub fn reversed(&self) -> Lin2d {
        let mut l = *self;
        l.pos.reverse();
        l
    }
    // gp_Lin2d::SetDirection
    pub fn set_direction(&mut self, v: Dir2d) {
        self.pos.set_direction(v);
    }
    // gp_Lin2d::SetLocation
    pub fn set_location(&mut self, p: Pnt2d) {
        self.pos.set_location(p);
    }
    // gp_Lin2d::SetPosition
    pub fn set_position(&mut self, a: Ax2d) {
        self.pos = a;
    }
    // gp_Lin2d::Coefficients
    pub fn coefficients(&self) -> (f64, f64, f64) {
        let a = self.pos.direction().y();
        let b = -self.pos.direction().x();
        let c = -(a * self.pos.location().x() + b * self.pos.location().y());
        (a, b, c)
    }
    // gp_Lin2d::Direction
    pub fn direction(&self) -> &Dir2d {
        self.pos.direction()
    }
    // gp_Lin2d::Location
    pub fn location(&self) -> &Pnt2d {
        self.pos.location()
    }
    // gp_Lin2d::Position
    pub fn position(&self) -> &Ax2d {
        &self.pos
    }
    // gp_Lin2d::Angle
    pub fn angle(&self, other: &Lin2d) -> f64 {
        self.pos.direction().angle(other.pos.direction())
    }
    // gp_Lin2d::Contains
    pub fn contains(&self, p: &Pnt2d, linear_tolerance: f64) -> bool {
        self.distance_to_point(p) <= linear_tolerance
    }
    fn distance_to_point(&self, p: &Pnt2d) -> f64 {
        p.distance_from_lin2d(self)
    }
    fn square_distance_to_point(&self, p: &Pnt2d) -> f64 {
        p.square_distance_from_lin2d(self)
    }
    // gp_Lin2d::Distance(gp_Pnt2d | gp_Lin2d)
    pub fn distance<T: Lin2dDistance>(&self, other: &T) -> f64 {
        other.distance_from_lin2d(self)
    }
    // gp_Lin2d::SquareDistance(gp_Pnt2d | gp_Lin2d)
    pub fn square_distance<T: Lin2dDistance>(&self, other: &T) -> f64 {
        other.square_distance_from_lin2d(self)
    }
    // gp_Lin2d::Normal
    pub fn normal(&self, p: &Pnt2d) -> Lin2d {
        let d = self.pos.direction();
        // The normal of a unit direction is a unit direction: the construction cannot fail.
        let dir = Dir2d::new(-d.y(), d.x()).unwrap_or_default();
        Lin2d { pos: Ax2d::new(*p, dir) }
    }
    // gp_Lin2d::Mirror
    pub fn mirror<M: Lin2dMirror>(&mut self, m: &M) {
        m.mirror_lin2d(self);
    }
    // gp_Lin2d::Mirrored
    pub fn mirrored<M: Lin2dMirror>(&self, m: &M) -> Lin2d {
        let mut l = *self;
        m.mirror_lin2d(&mut l);
        l
    }
    // gp_Lin2d::Rotate
    pub fn rotate(&mut self, p: &Pnt2d, ang: f64) {
        self.pos.rotate(p, ang);
    }
    // gp_Lin2d::Rotated
    pub fn rotated(&self, p: &Pnt2d, ang: f64) -> Lin2d {
        let mut l = *self;
        l.pos.rotate(p, ang);
        l
    }
    // gp_Lin2d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        self.pos.scale(p, s);
    }
    // gp_Lin2d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Lin2d {
        let mut l = *self;
        l.pos.scale(p, s);
        l
    }
    // gp_Lin2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        self.pos.transform(t);
    }
    // gp_Lin2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Lin2d {
        let mut l = *self;
        l.pos.transform(t);
        l
    }
    // gp_Lin2d::Translate(gp_Vec2d)
    pub fn translate(&mut self, v: &Vec2d) {
        self.pos.translate(v);
    }
    // gp_Lin2d::Translated(gp_Vec2d)
    pub fn translated(&self, v: &Vec2d) -> Lin2d {
        let mut l = *self;
        l.pos.translate(v);
        l
    }
    // gp_Lin2d::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
        self.pos.translate_points(p1, p2);
    }
    // gp_Lin2d::Translated(P1, P2): goes through gp_Vec2d(P1, P2) like OCCT
    pub fn translated_points(&self, p1: &Pnt2d, p2: &Pnt2d) -> Lin2d {
        let mut l = *self;
        l.pos.translate(&Vec2d::between(p1, p2));
        l
    }
}
