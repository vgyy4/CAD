//! `Geom2d_Transformation`.
use super::{Geom2dError, Result};
use crate::gp::{Ax2d, Pnt2d, Trsf2d, TrsfForm, Vec2d};

/// `Geom2d_Transformation` (not a `Geom2d_Geometry`).
#[derive(Debug, Clone)]
pub struct Transformation {
    gp_trsf2d: Trsf2d,
}

impl Default for Transformation {
    fn default() -> Self {
        Self::new()
    }
}

impl Transformation {
    /// The default constructor (identity).
    pub fn new() -> Self {
        Transformation { gp_trsf2d: Trsf2d::new() }
    }
    /// `Geom2d_Transformation(gp_Trsf2d)`.
    pub fn from_trsf2d(t: &Trsf2d) -> Self {
        Transformation { gp_trsf2d: *t }
    }
    /// `SetMirror(gp_Pnt2d)`.
    pub fn set_mirror_point(&mut self, p: &Pnt2d) {
        self.gp_trsf2d.set_mirror(p);
    }
    /// `SetMirror(gp_Ax2d)`.
    pub fn set_mirror_axis(&mut self, a: &Ax2d) {
        self.gp_trsf2d.set_mirror(a);
    }
    /// `SetRotation`.
    pub fn set_rotation(&mut self, p: &Pnt2d, ang: f64) {
        self.gp_trsf2d.set_rotation(p, ang);
    }
    /// `SetScale`.
    pub fn set_scale(&mut self, p: &Pnt2d, s: f64) {
        self.gp_trsf2d.set_scale(p, s);
    }
    /// `SetTransformation(FromAxis1, ToAxis2)`.
    pub fn set_transformation_between(&mut self, from: &Ax2d, to: &Ax2d) {
        self.gp_trsf2d.set_transformation_between(from, to);
    }
    /// `SetTransformation(ToAxis)`.
    pub fn set_transformation(&mut self, to: &Ax2d) {
        self.gp_trsf2d.set_transformation(to);
    }
    /// `SetTranslation(gp_Vec2d)`.
    pub fn set_translation(&mut self, v: &Vec2d) {
        self.gp_trsf2d.set_translation(v);
    }
    /// `SetTranslation(P1, P2)`.
    pub fn set_translation_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
        self.gp_trsf2d.set_translation_points(p1, p2);
    }
    /// `SetTrsf2d`.
    pub fn set_trsf2d(&mut self, t: &Trsf2d) {
        self.gp_trsf2d = *t;
    }
    /// `IsNegative`.
    pub fn is_negative(&self) -> bool {
        self.gp_trsf2d.is_negative()
    }
    /// `Form`.
    pub fn form(&self) -> TrsfForm {
        self.gp_trsf2d.form()
    }
    /// `ScaleFactor`.
    pub fn scale_factor(&self) -> f64 {
        self.gp_trsf2d.scale_factor()
    }
    /// `Trsf2d`.
    pub fn trsf2d(&self) -> Trsf2d {
        self.gp_trsf2d
    }
    /// `Value`.
    pub fn value(&self, row: i32, col: i32) -> Result<f64> {
        if !(1..=2).contains(&row) || !(1..=3).contains(&col) {
            return Err(Geom2dError::OutOfRange);
        }
        Ok(self.gp_trsf2d.value(row, col)?)
    }
    /// `Invert`.
    pub fn invert(&mut self) -> Result<()> {
        self.gp_trsf2d.invert()?;
        Ok(())
    }
    /// `Inverted`.
    pub fn inverted(&self) -> Result<Transformation> {
        Ok(Transformation::from_trsf2d(&self.gp_trsf2d.inverted()?))
    }
    /// `Multiplied`.
    pub fn multiplied(&self, other: &Transformation) -> Transformation {
        Transformation::from_trsf2d(&self.gp_trsf2d.multiplied(&other.trsf2d()))
    }
    /// `Multiply`.
    pub fn multiply(&mut self, other: &Transformation) {
        self.gp_trsf2d.multiply(&other.trsf2d());
    }
    /// `Power`.
    pub fn power(&mut self, n: i32) -> Result<()> {
        self.gp_trsf2d.power(n)?;
        Ok(())
    }
    /// `Powered`.
    pub fn powered(&self, n: i32) -> Result<Transformation> {
        let mut temp = self.gp_trsf2d;
        temp.power(n)?;
        Ok(Transformation::from_trsf2d(&temp))
    }
    /// `PreMultiply`.
    pub fn pre_multiply(&mut self, other: &Transformation) {
        self.gp_trsf2d.pre_multiply(&other.trsf2d());
    }
    /// `Transforms`.
    pub fn transforms(&self, x: f64, y: f64) -> (f64, f64) {
        self.gp_trsf2d.transforms_coords(x, y)
    }
    /// `Copy`.
    pub fn copy(&self) -> Transformation {
        Transformation::from_trsf2d(&self.gp_trsf2d)
    }
}

impl std::ops::Mul<&Transformation> for &Transformation {
    type Output = Transformation;
    fn mul(self, other: &Transformation) -> Transformation {
        self.multiplied(other)
    }
}

impl std::ops::MulAssign<&Transformation> for Transformation {
    fn mul_assign(&mut self, other: &Transformation) {
        self.multiply(other);
    }
}
