//! gp_Pnt2d
use super::{Ax2d, ConstructionError, Trsf2d, TrsfForm, Vec2d, XY};

/// gp_Pnt2d
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pnt2d(pub(crate) XY);

/// Things a 2D point can be mirrored about: a point or an axis.
pub trait Pnt2dMirror {
    /// Mirrors `p` about `self` (gp_Pnt2d::Mirror).
    fn mirror_pnt2d(&self, p: &mut Pnt2d);
}

impl Pnt2dMirror for Pnt2d {
    // gp_Pnt2d::Mirror(gp_Pnt2d)
    fn mirror_pnt2d(&self, p: &mut Pnt2d) {
        p.0.reverse();
        let mut xy = self.0;
        xy.multiply(2.0);
        p.0.add(&xy);
    }
}

impl Pnt2dMirror for Ax2d {
    // gp_Pnt2d::Mirror(gp_Ax2d)
    fn mirror_pnt2d(&self, p: &mut Pnt2d) {
        let mut t = Trsf2d::new();
        t.set_mirror(self);
        t.transforms_xy(&mut p.0);
    }
}

impl Pnt2d {
    // gp_Pnt2d::gp_Pnt2d(x, y)
    pub fn new(x: f64, y: f64) -> Self {
        Self(XY::new(x, y))
    }
    // gp_Pnt2d::gp_Pnt2d(gp_XY)
    pub fn from_xy(xy: XY) -> Self {
        Self(xy)
    }
    // gp_Pnt2d::gp_Pnt2d()
    pub fn origin() -> Self {
        Self::new(0., 0.)
    }
    // gp_Pnt2d::X
    pub fn x(&self) -> f64 {
        self.0.x()
    }
    // gp_Pnt2d::Y
    pub fn y(&self) -> f64 {
        self.0.y()
    }
    // gp_Pnt2d::Coord(x, y)
    pub fn coords(&self) -> (f64, f64) {
        self.0.coords()
    }
    // gp_Pnt2d::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        self.0.coord(index)
    }
    // gp_Pnt2d::XY
    pub fn xy(&self) -> &XY {
        &self.0
    }
    // gp_Pnt2d::ChangeCoord
    pub fn change_coord(&mut self) -> &mut XY {
        &mut self.0
    }
    // gp_Pnt2d::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        self.0.set_coord_at(index, value)
    }
    // gp_Pnt2d::SetCoord(x, y)
    pub fn set_coord(&mut self, x: f64, y: f64) {
        self.0.set_coord(x, y);
    }
    // gp_Pnt2d::SetX
    pub fn set_x(&mut self, x: f64) {
        self.0.set_x(x);
    }
    // gp_Pnt2d::SetY
    pub fn set_y(&mut self, y: f64) {
        self.0.set_y(y);
    }
    // gp_Pnt2d::SetXY
    pub fn set_xy(&mut self, xy: XY) {
        self.0 = xy;
    }
    // gp_Pnt2d::IsEqual
    pub fn is_equal(&self, other: &Pnt2d, linear_tolerance: f64) -> bool {
        self.distance(other) <= linear_tolerance
    }
    // gp_Pnt2d::Distance
    pub fn distance(&self, other: &Pnt2d) -> f64 {
        let x = self.0.x() - other.0.x();
        let y = self.0.y() - other.0.y();
        (x * x + y * y).sqrt()
    }
    // gp_Pnt2d::SquareDistance
    pub fn square_distance(&self, other: &Pnt2d) -> f64 {
        let x = self.0.x() - other.0.x();
        let y = self.0.y() - other.0.y();
        x * x + y * y
    }
    // gp_Pnt2d::Mirror
    pub fn mirror<M: Pnt2dMirror>(&mut self, m: &M) {
        m.mirror_pnt2d(self);
    }
    // gp_Pnt2d::Mirrored
    pub fn mirrored<M: Pnt2dMirror>(&self, m: &M) -> Pnt2d {
        let mut p = *self;
        m.mirror_pnt2d(&mut p);
        p
    }
    // gp_Pnt2d::Rotate
    pub fn rotate(&mut self, p: &Pnt2d, ang: f64) {
        let mut t = Trsf2d::new();
        t.set_rotation(p, ang);
        t.transforms_xy(&mut self.0);
    }
    // gp_Pnt2d::Rotated
    pub fn rotated(&self, p: &Pnt2d, ang: f64) -> Pnt2d {
        let mut r = *self;
        r.rotate(p, ang);
        r
    }
    // gp_Pnt2d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        let mut xy = p.0;
        xy.multiply(1.0 - s);
        self.0.multiply(s);
        self.0.add(&xy);
    }
    // gp_Pnt2d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Pnt2d {
        let mut r = *self;
        r.scale(p, s);
        r
    }
    // gp_Pnt2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        match t.form() {
            TrsfForm::Identity => {}
            TrsfForm::Translation => self.0.add(t.translation_part()),
            TrsfForm::Scale => {
                self.0.multiply(t.scale_factor());
                self.0.add(t.translation_part());
            }
            TrsfForm::PntMirror => {
                self.0.reverse();
                self.0.add(t.translation_part());
            }
            _ => t.transforms_xy(&mut self.0),
        }
    }
    // gp_Pnt2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Pnt2d {
        let mut p = *self;
        p.transform(t);
        p
    }
    // gp_Pnt2d::Translate(gp_Vec2d)
    pub fn translate(&mut self, v: &Vec2d) {
        self.0.add(v.xy());
    }
    // gp_Pnt2d::Translated(gp_Vec2d)
    pub fn translated(&self, v: &Vec2d) -> Pnt2d {
        let mut p = *self;
        p.0.add(v.xy());
        p
    }
    // gp_Pnt2d::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
        self.0.add(&p2.0);
        self.0.subtract(&p1.0);
    }
    // gp_Pnt2d::Translated(P1, P2)
    pub fn translated_points(&self, p1: &Pnt2d, p2: &Pnt2d) -> Pnt2d {
        let mut p = *self;
        p.translate_points(p1, p2);
        p
    }
}
