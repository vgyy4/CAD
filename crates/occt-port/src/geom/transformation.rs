//! `Geom_Transformation`.
use super::{GeomError, Result};
use crate::gp::{Ax1, Ax2, Ax3, Pnt, Trsf, TrsfForm};

/// `Geom_Transformation` (not a `Geom_Geometry`).
#[derive(Debug, Clone)]
pub struct Transformation {
    gp_trsf: Trsf,
}

impl Default for Transformation {
    fn default() -> Self {
        Self::new()
    }
}

impl Transformation {
    /// The default constructor (identity).
    pub fn new() -> Self {
        Transformation { gp_trsf: Trsf::new() }
    }
    /// `Geom_Transformation(gp_Trsf)`.
    pub fn from_trsf(t: &Trsf) -> Self {
        Transformation { gp_trsf: *t }
    }
    /// `SetMirror(gp_Pnt)`.
    pub fn set_mirror_point(&mut self, p: &Pnt) {
        self.gp_trsf.set_mirror(p);
    }
    /// `SetMirror(gp_Ax1)`.
    pub fn set_mirror_axis1(&mut self, a: &Ax1) {
        self.gp_trsf.set_mirror(a);
    }
    /// `SetMirror(gp_Ax2)`.
    pub fn set_mirror_axis2(&mut self, a: &Ax2) {
        self.gp_trsf.set_mirror(a);
    }
    /// `SetRotation`.
    pub fn set_rotation(&mut self, a: &Ax1, ang: f64) -> Result<()> {
        self.gp_trsf.set_rotation(a, ang)?;
        Ok(())
    }
    /// `SetScale` (a factor of magnitude `<= gp::Resolution()` is unchecked in OCCT: an error here).
    pub fn set_scale(&mut self, p: &Pnt, s: f64) -> Result<()> {
        self.gp_trsf.set_scale(p, s)?;
        Ok(())
    }
    /// `SetTransformation(FromSystem1, ToSystem2)`.
    pub fn set_transformation_between(&mut self, from: &Ax3, to: &Ax3) {
        self.gp_trsf.set_transformation_between(from, to);
    }
    /// `SetTransformation(ToSystem)`.
    pub fn set_transformation(&mut self, to: &Ax3) {
        self.gp_trsf.set_transformation(to);
    }
    /// `SetTranslation(gp_Vec)`.
    pub fn set_translation(&mut self, v: &crate::gp::Vec) {
        self.gp_trsf.set_translation(v);
    }
    /// `SetTranslation(P1, P2)`.
    pub fn set_translation_points(&mut self, p1: &Pnt, p2: &Pnt) {
        self.gp_trsf.set_translation_points(p1, p2);
    }
    /// `SetTrsf`.
    pub fn set_trsf(&mut self, t: &Trsf) {
        self.gp_trsf = *t;
    }
    /// `IsNegative`.
    pub fn is_negative(&self) -> bool {
        self.gp_trsf.is_negative()
    }
    /// `Form`.
    pub fn form(&self) -> TrsfForm {
        self.gp_trsf.form()
    }
    /// `ScaleFactor`.
    pub fn scale_factor(&self) -> f64 {
        self.gp_trsf.scale_factor()
    }
    /// `Trsf`.
    pub fn trsf(&self) -> Trsf {
        self.gp_trsf
    }
    /// `Value` (a row outside `1..=3` or a column outside `1..=4` is unchecked in OCCT: an error here).
    pub fn value(&self, row: i32, col: i32) -> Result<f64> {
        if !(1..=3).contains(&row) || !(1..=4).contains(&col) {
            return Err(GeomError::OutOfRange);
        }
        Ok(self.gp_trsf.value(row, col)?)
    }
    /// `Invert`.
    pub fn invert(&mut self) -> Result<()> {
        self.gp_trsf.invert()?;
        Ok(())
    }
    /// `Inverted`.
    pub fn inverted(&self) -> Result<Transformation> {
        Ok(Transformation::from_trsf(&self.gp_trsf.inverted()?))
    }
    /// `Multiplied`.
    pub fn multiplied(&self, other: &Transformation) -> Transformation {
        Transformation::from_trsf(&self.gp_trsf.multiplied(&other.trsf()))
    }
    /// `Multiply`.
    pub fn multiply(&mut self, other: &Transformation) {
        self.gp_trsf.multiply(&other.trsf());
    }
    /// `Power`.
    pub fn power(&mut self, n: i32) -> Result<()> {
        self.gp_trsf.power(n)?;
        Ok(())
    }
    /// `Powered`.
    pub fn powered(&self, n: i32) -> Result<Transformation> {
        let mut t = self.gp_trsf;
        t.power(n)?;
        Ok(Transformation::from_trsf(&t))
    }
    /// `PreMultiply`.
    pub fn pre_multiply(&mut self, other: &Transformation) {
        self.gp_trsf.pre_multiply(&other.trsf());
    }
    /// `Transforms`.
    pub fn transforms(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        self.gp_trsf.transforms_coords(x, y, z)
    }
    /// `Copy`.
    pub fn copy(&self) -> Transformation {
        Transformation::from_trsf(&self.gp_trsf)
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
