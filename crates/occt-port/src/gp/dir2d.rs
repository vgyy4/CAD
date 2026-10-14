//! gp_Dir2d
// 0.70710678118655 is OCCT's literal, not an approximation of FRAC_1_SQRT_2 to replace.
#![allow(clippy::approx_constant)]
use super::{Ax2d, ConstructionError, Direction2d, Pnt2d, Trsf2d, TrsfForm, Vec2d, XY, resolution};
use std::f64::consts::PI;

/// gp_Dir2d: a unit vector in the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dir2d(pub(crate) XY);

impl Default for Dir2d {
    // gp_Dir2d::gp_Dir2d()
    fn default() -> Self {
        Self(XY::new(1., 0.))
    }
}

/// Things a 2D direction can be mirrored about: a direction or an axis.
pub trait Dir2dMirror {
    /// Mirrors `d` about `self` (gp_Dir2d::Mirror).
    fn mirror_dir2d(&self, d: &mut Dir2d);
}

fn mirror_about(d: &mut Dir2d, a: f64, b: f64) {
    let x = d.0.x();
    let y = d.0.y();
    let m1 = 2.0 * a * b;
    let xx = ((2.0 * a * a) - 1.0) * x + m1 * y;
    let yy = m1 * x + ((2.0 * b * b) - 1.0) * y;
    d.0.set_coord(xx, yy);
}

impl Dir2dMirror for Dir2d {
    // gp_Dir2d::Mirror(gp_Dir2d)
    fn mirror_dir2d(&self, d: &mut Dir2d) {
        mirror_about(d, self.0.x(), self.0.y());
    }
}

impl Dir2dMirror for Ax2d {
    // gp_Dir2d::Mirror(gp_Ax2d)
    fn mirror_dir2d(&self, d: &mut Dir2d) {
        let xy = self.direction().0;
        mirror_about(d, xy.x(), xy.y());
    }
}

/// The checks shared by `gp_Dir2d::gp_Dir2d(x, y)` and `SetCoord(x, y)`: the stored coordinates.
fn normalized_coords(x: f64, y: f64) -> Result<XY, ConstructionError> {
    let sq_mod = x * x + y * y;
    let res = resolution();
    if sq_mod >= (1.0 - res) && sq_mod <= (1.0 + res) {
        return Ok(XY::new(x, y));
    }
    if sq_mod <= res * res {
        return Err(ConstructionError);
    }
    let d = sq_mod.sqrt();
    Ok(XY::new(x / d, y / d))
}

impl Dir2d {
    // gp_Dir2d::gp_Dir2d(x, y)
    pub fn new(x: f64, y: f64) -> Result<Self, ConstructionError> {
        Ok(Self(normalized_coords(x, y)?))
    }
    // gp_Dir2d::gp_Dir2d(gp_XY)
    pub fn from_xy(xy: XY) -> Result<Self, ConstructionError> {
        Self::new(xy.x(), xy.y())
    }
    // gp_Dir2d::gp_Dir2d(gp_Vec2d)
    pub fn from_vec(v: &Vec2d) -> Result<Self, ConstructionError> {
        Self::from_xy(*v.xy())
    }
    // gp_Dir2d::gp_Dir2d(gp_Dir2d::D)
    pub fn from_direction(d: Direction2d) -> Self {
        let x = match d {
            Direction2d::X => 1.0,
            Direction2d::NegativeX => -1.0,
            _ => 0.0,
        };
        let y = match d {
            Direction2d::Y => 1.0,
            Direction2d::NegativeY => -1.0,
            _ => 0.0,
        };
        Self(XY::new(x, y))
    }
    // gp_Dir2d::SetCoord(x, y)
    pub fn set_coord(&mut self, x: f64, y: f64) -> Result<(), ConstructionError> {
        self.0 = normalized_coords(x, y)?;
        Ok(())
    }
    // gp_Dir2d::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        match index {
            1 => self.set_x(value),
            2 => self.set_y(value),
            _ => Err(ConstructionError),
        }
    }
    // gp_Dir2d::SetX
    pub fn set_x(&mut self, x: f64) -> Result<(), ConstructionError> {
        self.set_coord(x, self.0.y())
    }
    // gp_Dir2d::SetY
    pub fn set_y(&mut self, y: f64) -> Result<(), ConstructionError> {
        self.set_coord(self.0.x(), y)
    }
    // gp_Dir2d::SetXY
    pub fn set_xy(&mut self, xy: XY) -> Result<(), ConstructionError> {
        self.set_coord(xy.x(), xy.y())
    }
    // gp_Dir2d::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        self.0.coord(index)
    }
    // gp_Dir2d::Coord(x, y)
    pub fn coords(&self) -> (f64, f64) {
        self.0.coords()
    }
    // gp_Dir2d::X
    pub fn x(&self) -> f64 {
        self.0.x()
    }
    // gp_Dir2d::Y
    pub fn y(&self) -> f64 {
        self.0.y()
    }
    // gp_Dir2d::XY
    pub fn xy(&self) -> &XY {
        &self.0
    }
    // gp_Dir2d::IsEqual
    pub fn is_equal(&self, other: &Dir2d, angular_tolerance: f64) -> bool {
        let mut ang = self.angle(other);
        if ang < 0.0 {
            ang = -ang;
        }
        ang <= angular_tolerance
    }
    // gp_Dir2d::IsNormal
    pub fn is_normal(&self, other: &Dir2d, angular_tolerance: f64) -> bool {
        let mut ang = self.angle(other);
        if ang < 0.0 {
            ang = -ang;
        }
        ang = PI / 2.0 - ang;
        if ang < 0.0 {
            ang = -ang;
        }
        ang <= angular_tolerance
    }
    // gp_Dir2d::IsOpposite
    pub fn is_opposite(&self, other: &Dir2d, angular_tolerance: f64) -> bool {
        let mut ang = self.angle(other);
        if ang < 0.0 {
            ang = -ang;
        }
        PI - ang <= angular_tolerance
    }
    // gp_Dir2d::IsParallel
    pub fn is_parallel(&self, other: &Dir2d, angular_tolerance: f64) -> bool {
        let mut ang = self.angle(other);
        if ang < 0.0 {
            ang = -ang;
        }
        ang <= angular_tolerance || PI - ang <= angular_tolerance
    }
    // gp_Dir2d::Angle
    pub fn angle(&self, other: &Dir2d) -> f64 {
        let cosinus = self.0.dot(&other.0);
        let sinus = self.0.crossed(&other.0);
        if cosinus > -0.70710678118655 && cosinus < 0.70710678118655 {
            if sinus > 0.0 { libm::acos(cosinus) } else { -libm::acos(cosinus) }
        } else if cosinus > 0.0 {
            libm::asin(sinus)
        } else if sinus > 0.0 {
            PI - libm::asin(sinus)
        } else {
            -PI - libm::asin(sinus)
        }
    }
    // gp_Dir2d::Crossed
    pub fn crossed(&self, right: &Dir2d) -> f64 {
        self.0.crossed(&right.0)
    }
    // gp_Dir2d::Dot
    pub fn dot(&self, other: &Dir2d) -> f64 {
        self.0.dot(&other.0)
    }
    // gp_Dir2d::Reverse
    pub fn reverse(&mut self) {
        self.0.reverse();
    }
    // gp_Dir2d::Reversed
    pub fn reversed(&self) -> Dir2d {
        let mut d = *self;
        d.0.reverse();
        d
    }
    // gp_Dir2d::Mirror
    pub fn mirror<M: Dir2dMirror>(&mut self, m: &M) {
        m.mirror_dir2d(self);
    }
    // gp_Dir2d::Mirrored
    pub fn mirrored<M: Dir2dMirror>(&self, m: &M) -> Dir2d {
        let mut d = *self;
        m.mirror_dir2d(&mut d);
        d
    }
    // gp_Dir2d::Rotate
    pub fn rotate(&mut self, ang: f64) {
        let mut t = Trsf2d::new();
        t.set_rotation(&Pnt2d::new(0.0, 0.0), ang);
        self.0.multiply_mat(t.h_vectorial_part());
    }
    // gp_Dir2d::Rotated
    pub fn rotated(&self, ang: f64) -> Dir2d {
        let mut d = *self;
        d.rotate(ang);
        d
    }
    // gp_Dir2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        match t.form() {
            TrsfForm::Identity | TrsfForm::Translation => {}
            TrsfForm::PntMirror => self.0.reverse(),
            TrsfForm::Scale => {
                if t.scale_factor() < 0.0 {
                    self.0.reverse();
                }
            }
            _ => {
                self.0.multiply_mat(t.h_vectorial_part());
                let d = self.0.modulus();
                self.0.divide(d);
                if t.scale_factor() < 0.0 {
                    self.0.reverse();
                }
            }
        }
    }
    // gp_Dir2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Dir2d {
        let mut d = *self;
        d.transform(t);
        d
    }
}
