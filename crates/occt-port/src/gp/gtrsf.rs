//! gp_GTrsf
use super::{Ax1, Ax2, ConstructionError, Mat, Trsf, TrsfForm, XYZ, resolution};

/// gp_GTrsf: a general (possibly non-orthogonal) affine transformation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GTrsf {
    matrix: Mat,
    loc: XYZ,
    shape: TrsfForm,
    scale: f64,
}

impl Default for GTrsf {
    fn default() -> Self {
        Self::new()
    }
}

/// What a `GTrsf` can be set to be an affinity about: an axis or a plane (an `Ax2`).
pub trait Affinity {
    /// gp_GTrsf::SetAffinity
    fn set_affinity_of(&self, t: &mut GTrsf, ratio: f64);
}

impl Affinity for Ax1 {
    // gp_GTrsf::SetAffinity(gp_Ax1, Ratio)
    fn set_affinity_of(&self, t: &mut GTrsf, ratio: f64) {
        t.shape = TrsfForm::Other;
        t.scale = 0.0;
        t.matrix.set_dot(self.direction().xyz());
        t.matrix.multiply_scalar(1.0 - ratio);
        let d = t.matrix.diagonal();
        t.matrix.set_diagonal(d.x() + ratio, d.y() + ratio, d.z() + ratio);
        t.loc = *self.location().xyz();
        t.loc.reverse();
        t.loc.multiply_mat(&t.matrix);
        t.loc.add(self.location().xyz());
    }
}

impl Affinity for Ax2 {
    // gp_GTrsf::SetAffinity(gp_Ax2, Ratio)
    fn set_affinity_of(&self, t: &mut GTrsf, ratio: f64) {
        t.shape = TrsfForm::Other;
        t.scale = 0.0;
        t.matrix.set_dot(self.direction().xyz());
        t.matrix.multiply_scalar(ratio - 1.);
        t.loc = *self.location().xyz();
        t.loc.reverse();
        t.loc.multiply_mat(&t.matrix);
        let d = t.matrix.diagonal();
        t.matrix.set_diagonal(d.x() + 1., d.y() + 1., d.z() + 1.);
    }
}

impl GTrsf {
    // gp_GTrsf::gp_GTrsf()
    pub fn new() -> Self {
        let mut matrix = Mat::new();
        matrix.set_scale(1.0);
        Self { matrix, loc: XYZ::new(0., 0., 0.), shape: TrsfForm::Identity, scale: 1.0 }
    }
    // gp_GTrsf::gp_GTrsf(gp_Trsf)
    pub fn from_trsf(t: &Trsf) -> Self {
        Self { matrix: t.matrix, loc: t.loc, shape: t.shape, scale: t.scale }
    }
    // gp_GTrsf::gp_GTrsf(gp_Mat, gp_XYZ)
    pub fn from_parts(m: Mat, v: XYZ) -> Self {
        Self { matrix: m, loc: v, shape: TrsfForm::Other, scale: 0.0 }
    }
    // gp_GTrsf::SetAffinity
    pub fn set_affinity<A: Affinity>(&mut self, a: &A, ratio: f64) {
        a.set_affinity_of(self, ratio);
    }
    // gp_GTrsf::SetValue
    pub fn set_value(&mut self, row: i32, col: i32, value: f64) -> Result<(), ConstructionError> {
        if !(1..=3).contains(&row) || !(1..=4).contains(&col) {
            return Err(ConstructionError);
        }
        if col == 4 {
            self.loc.set_coord_at(row, value)?;
            if self.shape == TrsfForm::Identity {
                self.shape = TrsfForm::Translation;
            }
        } else {
            if self.shape != TrsfForm::Other && self.scale != 1.0 {
                self.matrix.multiply_scalar(self.scale);
            }
            self.matrix.set_value(row, col, value)?;
            self.shape = TrsfForm::Other;
            self.scale = 0.0;
        }
        Ok(())
    }
    // gp_GTrsf::SetVectorialPart
    pub fn set_vectorial_part(&mut self, m: &Mat) {
        self.matrix = *m;
        self.shape = TrsfForm::Other;
        self.scale = 0.0;
    }
    // gp_GTrsf::SetTranslationPart
    pub fn set_translation_part(&mut self, coord: &XYZ) {
        self.loc = *coord;
        match self.shape {
            TrsfForm::CompoundTrsf | TrsfForm::Other | TrsfForm::Translation => {}
            TrsfForm::Identity => self.shape = TrsfForm::Translation,
            _ => self.shape = TrsfForm::CompoundTrsf,
        }
    }
    // gp_GTrsf::SetTrsf
    pub fn set_trsf(&mut self, t: &Trsf) {
        self.shape = t.shape;
        self.matrix = t.matrix;
        self.loc = t.loc;
        self.scale = t.scale;
    }
    // gp_GTrsf::IsNegative
    pub fn is_negative(&self) -> bool {
        self.matrix.determinant() < 0.0
    }
    // gp_GTrsf::IsSingular
    pub fn is_singular(&self) -> bool {
        self.matrix.is_singular()
    }
    // gp_GTrsf::Form
    pub fn form(&self) -> TrsfForm {
        self.shape
    }
    // gp_GTrsf::SetForm; with a null determinant OCCT raises: the transformation is left unchanged
    pub fn set_form(&mut self) {
        let tol = 1.0e-12;
        let mut m = self.matrix;
        let mut s = m.determinant();
        if s.abs() < resolution() {
            return;
        }
        if s > 0.0 {
            s = libm::pow(s, 1. / 3.);
        } else {
            s = -libm::pow(-s, 1. / 3.);
        }
        m.divide(s);
        let mut tm = m;
        tm.transpose();
        tm.multiply_mat(&m);
        let mut identity = Mat::new();
        identity.set_identity();
        tm.subtract(&identity);
        if self.shape == TrsfForm::Other {
            self.shape = TrsfForm::CompoundTrsf;
        }
        if tm.m.iter().flatten().any(|e| e.abs() > tol) {
            self.shape = TrsfForm::Other;
        }
    }
    // gp_GTrsf::TranslationPart
    pub fn translation_part(&self) -> &XYZ {
        &self.loc
    }
    // gp_GTrsf::VectorialPart
    pub fn vectorial_part(&self) -> &Mat {
        &self.matrix
    }
    // gp_GTrsf::Value
    pub fn value(&self, row: i32, col: i32) -> Result<f64, ConstructionError> {
        if !(1..=3).contains(&row) || !(1..=4).contains(&col) {
            return Err(ConstructionError);
        }
        if col == 4 {
            return self.loc.coord(row);
        }
        let v = self.matrix.value(row, col)?;
        if self.shape == TrsfForm::Other { Ok(v) } else { Ok(self.scale * v) }
    }
    /// The `Trsf` with the same fields, without the "non-orthogonal" check.
    fn trsf_raw(&self) -> Trsf {
        Trsf { scale: self.scale, shape: self.shape, matrix: self.matrix, loc: self.loc }
    }
    // gp_GTrsf::Invert
    pub fn invert(&mut self) -> Result<(), ConstructionError> {
        if self.shape == TrsfForm::Other {
            self.matrix.invert()?;
            self.loc.multiply_mat(&self.matrix);
            self.loc.reverse();
        } else {
            let mut t = self.trsf()?;
            t.invert()?;
            self.set_trsf(&t);
        }
        Ok(())
    }
    // gp_GTrsf::Inverted
    pub fn inverted(&self) -> Result<GTrsf, ConstructionError> {
        let mut t = *self;
        t.invert()?;
        Ok(t)
    }
    // gp_GTrsf::Multiplied
    pub fn multiplied(&self, t: &GTrsf) -> GTrsf {
        let mut r = *self;
        r.multiply(t);
        r
    }
    // gp_GTrsf::Multiply
    pub fn multiply(&mut self, t: &GTrsf) {
        if self.shape == TrsfForm::Other || t.shape == TrsfForm::Other {
            self.shape = TrsfForm::Other;
            self.loc.add(&t.loc.multiplied_mat(&self.matrix));
            self.matrix.multiply_mat(&t.matrix);
        } else {
            let mut t1 = self.trsf_raw();
            let t2 = t.trsf_raw();
            t1.multiply(&t2);
            self.set_trsf(&t1);
        }
    }
    // gp_GTrsf::PreMultiply
    pub fn pre_multiply(&mut self, t: &GTrsf) {
        if self.shape == TrsfForm::Other || t.shape == TrsfForm::Other {
            self.shape = TrsfForm::Other;
            self.loc.multiply_mat(&t.matrix);
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        } else {
            let mut t1 = self.trsf_raw();
            let t2 = t.trsf_raw();
            t1.pre_multiply(&t2);
            self.set_trsf(&t1);
        }
    }
    // gp_GTrsf::Power
    pub fn power(&mut self, n: i32) -> Result<(), ConstructionError> {
        if n == 0 {
            self.scale = 1.;
            self.shape = TrsfForm::Identity;
            self.matrix.set_identity();
            self.loc = XYZ::new(0., 0., 0.);
        } else if n == 1 {
        } else if n == -1 {
            self.invert()?;
        } else if self.shape == TrsfForm::Other {
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
                let tm = tempmatrix;
                tempmatrix.multiply_mat(&tm);
                npower /= 2;
            }
        } else {
            let mut t = self.trsf()?;
            t.power(n)?;
            self.set_trsf(&t);
        }
        Ok(())
    }
    // gp_GTrsf::Powered
    pub fn powered(&self, n: i32) -> Result<GTrsf, ConstructionError> {
        let mut t = *self;
        t.power(n)?;
        Ok(t)
    }
    // gp_GTrsf::Transforms(gp_XYZ)
    pub fn transforms_xyz(&self, coord: &mut XYZ) {
        coord.multiply_mat(&self.matrix);
        if self.shape != TrsfForm::Other && self.scale != 1.0 {
            coord.multiply(self.scale);
        }
        coord.add(&self.loc);
    }
    // gp_GTrsf::Transforms(x, y, z)
    pub fn transforms_coords(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        let mut triplet = XYZ::new(x, y, z);
        self.transforms_xyz(&mut triplet);
        triplet.coords()
    }
    // gp_GTrsf::Trsf
    pub fn trsf(&self) -> Result<Trsf, ConstructionError> {
        if self.shape == TrsfForm::Other {
            return Err(ConstructionError);
        }
        Ok(self.trsf_raw())
    }
    // gp_GTrsf::GetMat4 (zero-based 4x4 layout, row by row)
    pub fn get_mat4(&self) -> [[f64; 4]; 4] {
        if self.shape == TrsfForm::Identity {
            return [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
        }
        let row = |r: i32| -> [f64; 4] {
            [
                self.value(r, 1).unwrap_or(0.0),
                self.value(r, 2).unwrap_or(0.0),
                self.value(r, 3).unwrap_or(0.0),
                self.value(r, 4).unwrap_or(0.0),
            ]
        };
        [row(1), row(2), row(3), [0., 0., 0., 1.]]
    }
    // gp_GTrsf::SetMat4
    pub fn set_mat4(&mut self, m: [[f64; 4]; 4]) {
        self.shape = TrsfForm::Other;
        self.scale = 0.0;
        let [[a, b, c, x], [d, e, f, y], [g, h, i, z], _] = m;
        self.matrix = Mat::from_values([a, b, c, d, e, f, g, h, i]);
        self.loc.set_coord(x, y, z);
    }
}
