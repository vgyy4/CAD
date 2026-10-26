//! `Geom2d_Direction` and `Geom2d_VectorWithMagnitude`.
use super::{Geom2dError, Geometry2d, Result, Vector2d, geometry_any};
use crate::gp::{self, Dir2d, Pnt2d, Trsf2d, Vec2d};

/// `Geom2d_Direction`.
#[derive(Debug, Clone)]
pub struct Direction {
    gp_vec2d: Vec2d,
}

impl Direction {
    /// `Geom2d_Direction(X, Y)`.
    pub fn new(x: f64, y: f64) -> Result<Self> {
        let d = (x * x + y * y).sqrt();
        if d <= gp::resolution() {
            return Err(Geom2dError::Construction);
        }
        Ok(Direction { gp_vec2d: Vec2d::new(x / d, y / d) })
    }
    /// `Geom2d_Direction(gp_Dir2d)`.
    pub fn from_dir2d(d: Dir2d) -> Self {
        Direction { gp_vec2d: Vec2d::from_dir(&d) }
    }
    /// `SetCoord`.
    pub fn set_coord(&mut self, x: f64, y: f64) -> Result<()> {
        let d = (x * x + y * y).sqrt();
        if d <= gp::resolution() {
            return Err(Geom2dError::Construction);
        }
        self.gp_vec2d = Vec2d::new(x / d, y / d);
        Ok(())
    }
    /// `SetDir2d`.
    pub fn set_dir2d(&mut self, d: Dir2d) {
        self.gp_vec2d = Vec2d::from_dir(&d);
    }
    /// `SetX`.
    pub fn set_x(&mut self, x: f64) -> Result<()> {
        let d = (x * x + self.gp_vec2d.y() * self.gp_vec2d.y()).sqrt();
        if d <= gp::resolution() {
            return Err(Geom2dError::Construction);
        }
        self.gp_vec2d = Vec2d::new(x / d, self.gp_vec2d.y() / d);
        Ok(())
    }
    /// `SetY`.
    pub fn set_y(&mut self, y: f64) -> Result<()> {
        let d = (self.gp_vec2d.x() * self.gp_vec2d.x() + y * y).sqrt();
        if d <= gp::resolution() {
            return Err(Geom2dError::Construction);
        }
        self.gp_vec2d = Vec2d::new(self.gp_vec2d.x() / d, y / d);
        Ok(())
    }
    /// `Dir2d`: the stored vector as a direction (`gp_Dir2d(gp_Vec2d)` normalizes again).
    pub fn dir2d(&self) -> Dir2d {
        Dir2d::from_vec(&self.gp_vec2d).unwrap_or_default()
    }
}

impl Geometry2d for Direction {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        let mut dir = self.dir2d();
        dir.transform(t);
        self.gp_vec2d = Vec2d::from_dir(&dir);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(Direction::from_dir2d(self.dir2d()))
    }
}

impl Vector2d for Direction {
    fn vec2d(&self) -> Vec2d {
        self.gp_vec2d
    }
    fn set_vec2d_raw(&mut self, v: Vec2d) {
        self.gp_vec2d = v;
    }
    fn copy_vector(&self) -> Box<dyn Vector2d> {
        Box::new(Direction::from_dir2d(self.dir2d()))
    }
    fn magnitude(&self) -> f64 {
        1.0
    }
    fn square_magnitude(&self) -> f64 {
        1.0
    }
    fn crossed(&self, other: &dyn Vector2d) -> f64 {
        self.gp_vec2d.crossed(&other.vec2d())
    }
}

/// `Geom2d_VectorWithMagnitude`.
#[derive(Debug, Clone)]
pub struct VectorWithMagnitude {
    gp_vec2d: Vec2d,
}

impl VectorWithMagnitude {
    /// `Geom2d_VectorWithMagnitude(gp_Vec2d)`.
    pub fn from_vec2d(v: Vec2d) -> Self {
        VectorWithMagnitude { gp_vec2d: v }
    }
    /// `Geom2d_VectorWithMagnitude(X, Y)`.
    pub fn new(x: f64, y: f64) -> Self {
        VectorWithMagnitude { gp_vec2d: Vec2d::new(x, y) }
    }
    /// `Geom2d_VectorWithMagnitude(P1, P2)`.
    pub fn from_points(p1: Pnt2d, p2: Pnt2d) -> Self {
        VectorWithMagnitude { gp_vec2d: Vec2d::between(&p1, &p2) }
    }
    /// `SetCoord`.
    pub fn set_coord(&mut self, x: f64, y: f64) {
        self.gp_vec2d = Vec2d::new(x, y);
    }
    /// `SetVec2d`.
    pub fn set_vec2d(&mut self, v: Vec2d) {
        self.gp_vec2d = v;
    }
    /// `SetX`.
    pub fn set_x(&mut self, x: f64) {
        self.gp_vec2d.set_x(x);
    }
    /// `SetY`.
    pub fn set_y(&mut self, y: f64) {
        self.gp_vec2d.set_y(y);
    }
    /// `Add`.
    pub fn add(&mut self, other: &dyn Vector2d) {
        self.gp_vec2d.add(&other.vec2d());
    }
    /// `Added`.
    pub fn added(&self, other: &dyn Vector2d) -> VectorWithMagnitude {
        let mut temp = other.vec2d();
        temp.add(&self.gp_vec2d);
        VectorWithMagnitude::from_vec2d(temp)
    }
    /// `Subtract`.
    pub fn subtract(&mut self, other: &dyn Vector2d) {
        self.gp_vec2d.subtract(&other.vec2d());
    }
    /// `Subtracted`.
    pub fn subtracted(&self, other: &dyn Vector2d) -> VectorWithMagnitude {
        let mut v = self.gp_vec2d;
        v.subtract(&other.vec2d());
        VectorWithMagnitude::from_vec2d(v)
    }
    /// `Multiply`.
    pub fn multiply(&mut self, s: f64) {
        self.gp_vec2d.multiply(s);
    }
    /// `Multiplied`.
    pub fn multiplied(&self, s: f64) -> VectorWithMagnitude {
        let mut v = self.gp_vec2d;
        v.multiply(s);
        VectorWithMagnitude::from_vec2d(v)
    }
    /// `Divide` (a divisor of magnitude `<= gp::Resolution()` is unchecked in OCCT: an error here).
    pub fn divide(&mut self, s: f64) -> Result<()> {
        if s.abs() <= gp::resolution() {
            return Err(Geom2dError::Argument);
        }
        self.gp_vec2d.divide(s);
        Ok(())
    }
    /// `Divided`.
    pub fn divided(&self, s: f64) -> Result<VectorWithMagnitude> {
        if s.abs() <= gp::resolution() {
            return Err(Geom2dError::Argument);
        }
        let mut v = self.gp_vec2d;
        v.divide(s);
        Ok(VectorWithMagnitude::from_vec2d(v))
    }
    /// `Normalize`.
    pub fn normalize(&mut self) -> Result<()> {
        self.gp_vec2d.normalize()?;
        Ok(())
    }
    /// `Normalized`.
    pub fn normalized(&self) -> Result<VectorWithMagnitude> {
        let v = self.gp_vec2d.normalized()?;
        Ok(VectorWithMagnitude::from_vec2d(v))
    }
}

impl Geometry2d for VectorWithMagnitude {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        self.gp_vec2d.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(VectorWithMagnitude::from_vec2d(self.gp_vec2d))
    }
}

impl Vector2d for VectorWithMagnitude {
    fn vec2d(&self) -> Vec2d {
        self.gp_vec2d
    }
    fn set_vec2d_raw(&mut self, v: Vec2d) {
        self.gp_vec2d = v;
    }
    fn copy_vector(&self) -> Box<dyn Vector2d> {
        Box::new(VectorWithMagnitude::from_vec2d(self.gp_vec2d))
    }
    fn magnitude(&self) -> f64 {
        self.gp_vec2d.magnitude()
    }
    fn square_magnitude(&self) -> f64 {
        self.gp_vec2d.square_magnitude()
    }
    fn crossed(&self, other: &dyn Vector2d) -> f64 {
        self.gp_vec2d.crossed(&other.vec2d())
    }
}
