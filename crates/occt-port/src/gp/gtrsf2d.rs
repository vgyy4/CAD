//! gp_GTrsf2d
use super::{ANGULAR, Ax2d, ConstructionError, Mat2d, Trsf2d, TrsfForm, XY};

/// gp_GTrsf2d: a general (affine) 2D transformation: a matrix and a translation. The form is
/// `Other` as soon as the matrix is set directly; the hidden scale factor is only used by the
/// other forms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GTrsf2d {
    matrix: Mat2d,
    loc: XY,
    shape: TrsfForm,
    scale: f64,
}

impl Default for GTrsf2d {
    fn default() -> Self {
        Self::new()
    }
}

impl GTrsf2d {
    // gp_GTrsf2d::gp_GTrsf2d()
    pub fn new() -> Self {
        let mut matrix = Mat2d::new();
        matrix.set_scale(1.0);
        Self { matrix, loc: XY::new(0.0, 0.0), shape: TrsfForm::Identity, scale: 1.0 }
    }
    // gp_GTrsf2d::gp_GTrsf2d(gp_Trsf2d)
    pub fn from_trsf2d(t: &Trsf2d) -> Self {
        Self { matrix: t.matrix, loc: t.loc, shape: t.shape, scale: t.scale }
    }
    // gp_GTrsf2d::gp_GTrsf2d(gp_Mat2d, gp_XY)
    pub fn from_parts(m: Mat2d, v: XY) -> Self {
        Self { matrix: m, loc: v, shape: TrsfForm::Other, scale: 0.0 }
    }
    // gp_GTrsf2d::SetAffinity
    pub fn set_affinity(&mut self, a: &Ax2d, ratio: f64) {
        self.shape = TrsfForm::Other;
        self.scale = 0.0;
        let aa = a.direction().x();
        let b = a.direction().y();
        self.matrix.m = [
            [(1.0 - ratio) * aa * aa + ratio, (1.0 - ratio) * aa * b],
            [(1.0 - ratio) * aa * b, (1.0 - ratio) * b * b + ratio],
        ];
        self.loc = *a.location().xy();
        self.loc.reverse();
        self.loc.multiply_mat(&self.matrix);
        self.loc.add(a.location().xy());
    }
    // gp_GTrsf2d::SetValue
    pub fn set_value(&mut self, row: i32, col: i32, value: f64) -> Result<(), ConstructionError> {
        if !(1..=2).contains(&row) || !(1..=3).contains(&col) {
            return Err(ConstructionError);
        }
        if col == 3 {
            self.loc.set_coord_at(row, value)?;
        } else {
            self.matrix.set_value(row, col, value)?;
        }
        self.shape = TrsfForm::Other;
        Ok(())
    }
    // gp_GTrsf2d::SetTranslationPart
    pub fn set_translation_part(&mut self, coord: &XY) {
        self.loc = *coord;
        match self.shape {
            TrsfForm::CompoundTrsf | TrsfForm::Other | TrsfForm::Translation => {}
            TrsfForm::Identity => self.shape = TrsfForm::Translation,
            _ => self.shape = TrsfForm::CompoundTrsf,
        }
    }
    // gp_GTrsf2d::SetTrsf2d
    pub fn set_trsf2d(&mut self, t: &Trsf2d) {
        self.shape = t.shape;
        self.matrix = t.matrix;
        self.loc = t.loc;
        self.scale = t.scale;
    }
    // gp_GTrsf2d::SetVectorialPart
    pub fn set_vectorial_part(&mut self, m: &Mat2d) {
        self.matrix = *m;
        self.shape = TrsfForm::Other;
        self.scale = 0.0;
    }
    // gp_GTrsf2d::IsNegative
    pub fn is_negative(&self) -> bool {
        self.matrix.determinant() < 0.0
    }
    // gp_GTrsf2d::IsSingular
    pub fn is_singular(&self) -> bool {
        self.matrix.is_singular()
    }
    // gp_GTrsf2d::Form
    pub fn form(&self) -> TrsfForm {
        self.shape
    }
    // gp_GTrsf2d::TranslationPart
    pub fn translation_part(&self) -> &XY {
        &self.loc
    }
    // gp_GTrsf2d::VectorialPart
    pub fn vectorial_part(&self) -> &Mat2d {
        &self.matrix
    }
    // gp_GTrsf2d::Value
    pub fn value(&self, row: i32, col: i32) -> Result<f64, ConstructionError> {
        if !(1..=2).contains(&row) || !(1..=3).contains(&col) {
            return Err(ConstructionError);
        }
        if col == 3 {
            return self.loc.coord(row);
        }
        let v = self.matrix.value(row, col)?;
        if self.shape == TrsfForm::Other {
            return Ok(v);
        }
        Ok(self.scale * v)
    }
    // gp_GTrsf2d::Invert
    pub fn invert(&mut self) -> Result<(), ConstructionError> {
        if self.shape == TrsfForm::Other {
            self.matrix.invert()?;
            self.loc.multiply_mat(&self.matrix);
            self.loc.reverse();
        } else {
            let mut t = self.trsf2d()?;
            t.invert()?;
            self.set_trsf2d(&t);
        }
        Ok(())
    }
    // gp_GTrsf2d::Inverted
    pub fn inverted(&self) -> Result<GTrsf2d, ConstructionError> {
        let mut g = *self;
        g.invert()?;
        Ok(g)
    }
    // gp_GTrsf2d::Multiplied
    pub fn multiplied(&self, t: &GTrsf2d) -> Result<GTrsf2d, ConstructionError> {
        let mut g = *self;
        g.multiply(t)?;
        Ok(g)
    }
    // gp_GTrsf2d::Multiply
    pub fn multiply(&mut self, t: &GTrsf2d) -> Result<(), ConstructionError> {
        if self.shape == TrsfForm::Other || t.shape == TrsfForm::Other {
            self.shape = TrsfForm::Other;
            self.loc.add(&t.loc.multiplied_mat(&self.matrix));
            self.matrix.multiply_mat(&t.matrix);
        } else {
            let mut t1 = self.trsf2d()?;
            let t2 = t.trsf2d()?;
            t1.multiply(&t2);
            self.set_trsf2d(&t1);
        }
        Ok(())
    }
    // gp_GTrsf2d::PreMultiply
    pub fn pre_multiply(&mut self, t: &GTrsf2d) -> Result<(), ConstructionError> {
        if self.shape == TrsfForm::Other || t.shape == TrsfForm::Other {
            self.shape = TrsfForm::Other;
            self.loc.multiply_mat(&t.matrix);
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        } else {
            let mut t1 = self.trsf2d()?;
            let t2 = t.trsf2d()?;
            t1.pre_multiply(&t2);
            self.set_trsf2d(&t1);
        }
        Ok(())
    }
    // gp_GTrsf2d::Power; for a negative exponent the transformation is inverted here and then
    // `Trsf2d::Power(N)` (still negative) inverts it once more, exactly as in OCCT.
    pub fn power(&mut self, n: i32) -> Result<(), ConstructionError> {
        if n == 0 {
            self.scale = 1.0;
            self.shape = TrsfForm::Identity;
            self.matrix.set_identity();
            self.loc = XY::new(0.0, 0.0);
            return Ok(());
        }
        if n == 1 {
            return Ok(());
        }
        if n == -1 {
            return self.invert();
        }
        if n < 0 {
            self.invert()?;
        }
        if self.shape == TrsfForm::Other {
            let mut npower = i64::from(n).abs() - 1;
            let mut temploc = self.loc;
            let mut tempmatrix = self.matrix;
            loop {
                if npower & 1 == 1 {
                    self.loc.add(&temploc.multiplied_mat(&self.matrix));
                    self.matrix.multiply_mat(&tempmatrix);
                }
                if npower == 1 {
                    break;
                }
                let m = temploc.multiplied_mat(&tempmatrix);
                temploc.add(&m);
                tempmatrix.square_in_place();
                npower /= 2;
            }
        } else {
            let mut t = self.trsf2d()?;
            t.power(n)?;
            self.set_trsf2d(&t);
        }
        Ok(())
    }
    // gp_GTrsf2d::Powered
    pub fn powered(&self, n: i32) -> Result<GTrsf2d, ConstructionError> {
        let mut g = *self;
        g.power(n)?;
        Ok(g)
    }
    // gp_GTrsf2d::Transforms(gp_XY)
    pub fn transforms_xy(&self, coord: &mut XY) {
        coord.multiply_mat(&self.matrix);
        if self.shape != TrsfForm::Other && self.scale != 1.0 {
            coord.multiply(self.scale);
        }
        coord.add(&self.loc);
    }
    // gp_GTrsf2d::Transformed
    pub fn transformed(&self, coord: &XY) -> XY {
        let mut c = *coord;
        self.transforms_xy(&mut c);
        c
    }
    // gp_GTrsf2d::Transforms(x, y)
    pub fn transforms_coords(&self, x: f64, y: f64) -> (f64, f64) {
        let mut doublet = XY::new(x, y);
        self.transforms_xy(&mut doublet);
        doublet.coords()
    }
    // gp_GTrsf2d::Trsf2d
    pub fn trsf2d(&self) -> Result<Trsf2d, ConstructionError> {
        let tolerance = ANGULAR;
        let tolerance2 = 2.0 * tolerance;
        if self.shape == TrsfForm::Other {
            return Err(ConstructionError);
        }
        let [[m11, m12], [m21, m22]] = self.matrix.m;
        let mut value = m11 * m11 + m21 * m21;
        if (value - 1.0).abs() > tolerance2 {
            return Err(ConstructionError);
        }
        value = m12 * m12 + m22 * m22;
        if (value - 1.0).abs() > tolerance2 {
            return Err(ConstructionError);
        }
        value = m11 * m12 + m21 * m22;
        if value.abs() > tolerance {
            return Err(ConstructionError);
        }
        Ok(Trsf2d { scale: self.scale, shape: self.shape, matrix: self.matrix, loc: self.loc })
    }
}
