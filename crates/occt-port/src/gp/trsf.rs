//! gp_Trsf and gp_TrsfForm
use super::{Ax1, Ax2, Ax3, ConstructionError, Mat, Pnt, Quaternion, Vec, XYZ, resolution};

/// gp_TrsfForm (same discriminants as the C++ enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TrsfForm {
    Identity,
    Rotation,
    Translation,
    PntMirror,
    Ax1Mirror,
    Ax2Mirror,
    Scale,
    CompoundTrsf,
    Other,
}

/// gp_Trsf: a non-persistent transformation (rotation, translation, mirror, scale and their
/// products) with a uniform scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trsf {
    pub(crate) scale: f64,
    pub(crate) shape: TrsfForm,
    pub(crate) matrix: Mat,
    pub(crate) loc: XYZ,
}

impl Default for Trsf {
    fn default() -> Self {
        Self::new()
    }
}

/// Things a `Trsf` can be set to mirror about: a point, an axis or a plane (an `Ax2`).
pub trait TrsfMirror {
    /// gp_Trsf::SetMirror
    fn set_mirror_of(&self, t: &mut Trsf);
}

impl TrsfMirror for Pnt {
    // gp_Trsf::SetMirror(gp_Pnt)
    fn set_mirror_of(&self, t: &mut Trsf) {
        t.shape = TrsfForm::PntMirror;
        t.scale = -1.0;
        t.loc = *self.xyz();
        t.matrix.set_identity();
        t.loc.multiply(2.0);
    }
}

impl TrsfMirror for Ax1 {
    // gp_Trsf::SetMirror(gp_Ax1)
    fn set_mirror_of(&self, t: &mut Trsf) {
        t.shape = TrsfForm::Ax1Mirror;
        t.scale = 1.0;
        t.loc = *self.location().xyz();
        t.matrix.set_dot(self.direction().xyz());
        t.matrix.multiply_scalar(-2.0);
        let d = t.matrix.diagonal();
        t.matrix.set_diagonal(d.x() + 1.0, d.y() + 1.0, d.z() + 1.0);
        t.loc.multiply_mat(&t.matrix);
        t.loc.add(self.location().xyz());
        t.matrix.multiply_scalar(-1.0);
    }
}

impl TrsfMirror for Ax2 {
    // gp_Trsf::SetMirror(gp_Ax2)
    fn set_mirror_of(&self, t: &mut Trsf) {
        t.shape = TrsfForm::Ax2Mirror;
        t.scale = -1.0;
        t.loc = *self.location().xyz();
        t.matrix.set_dot(self.direction().xyz());
        t.matrix.multiply_scalar(2.0);
        let d = t.matrix.diagonal();
        t.matrix.set_diagonal(d.x() - 1.0, d.y() - 1.0, d.z() - 1.0);
        t.loc.multiply_mat(&t.matrix);
        t.loc.add(self.location().xyz());
    }
}

fn is_odd(n: i64) -> bool {
    n & 1 == 1
}

impl Trsf {
    // gp_Trsf::gp_Trsf()
    pub fn new() -> Self {
        Self {
            scale: 1.0,
            shape: TrsfForm::Identity,
            matrix: Mat::from_values([1., 0., 0., 0., 1., 0., 0., 0., 1.]),
            loc: XYZ::new(0., 0., 0.),
        }
    }
    // gp_Trsf::SetMirror
    pub fn set_mirror<M: TrsfMirror>(&mut self, m: &M) {
        m.set_mirror_of(self);
    }
    // gp_Trsf::SetRotation(gp_Ax1, Ang)
    pub fn set_rotation(&mut self, a1: &Ax1, ang: f64) -> Result<(), ConstructionError> {
        self.shape = TrsfForm::Rotation;
        self.scale = 1.0;
        self.loc = *a1.location().xyz();
        self.matrix.set_rotation(a1.direction().xyz(), ang)?;
        self.loc.reverse();
        self.loc.multiply_mat(&self.matrix);
        self.loc.add(a1.location().xyz());
        Ok(())
    }
    // gp_Trsf::SetRotation(gp_Quaternion)
    pub fn set_rotation_quaternion(&mut self, r: &Quaternion) -> Result<(), ConstructionError> {
        self.shape = TrsfForm::Rotation;
        self.scale = 1.0;
        self.loc.set_coord(0., 0., 0.);
        self.matrix = r.get_matrix();
        Ok(())
    }
    // gp_Trsf::SetRotationPart
    pub fn set_rotation_part(&mut self, r: &Quaternion) {
        let has_rotation = !r.is_equal(&Quaternion::new());
        if has_rotation {
            self.matrix = r.get_matrix();
        } else {
            self.matrix.set_identity();
        }
        match self.shape {
            TrsfForm::Identity => {
                if has_rotation {
                    self.shape = TrsfForm::Rotation;
                }
            }
            TrsfForm::Rotation => {
                if !has_rotation {
                    self.shape = TrsfForm::Identity;
                }
            }
            _ => {
                if has_rotation {
                    self.shape = TrsfForm::CompoundTrsf;
                }
            }
        }
    }
    // gp_Trsf::SetScale
    pub fn set_scale(&mut self, p: &Pnt, s: f64) -> Result<(), ConstructionError> {
        if s.abs() <= resolution() {
            return Err(ConstructionError);
        }
        self.shape = TrsfForm::Scale;
        self.scale = s;
        self.loc = *p.xyz();
        self.matrix.set_identity();
        self.loc.multiply(1.0 - s);
        Ok(())
    }
    // gp_Trsf::SetDisplacement
    pub fn set_displacement(&mut self, from: &Ax3, to: &Ax3) {
        self.shape = TrsfForm::CompoundTrsf;
        self.scale = 1.0;
        // SetCol with the indices 1..3 cannot fail.
        let _ = self.matrix.set_col(1, to.x_direction().xyz());
        let _ = self.matrix.set_col(2, to.y_direction().xyz());
        let _ = self.matrix.set_col(3, to.direction().xyz());
        self.loc = *to.location().xyz();
        let x_dir = from.x_direction().xyz();
        let y_dir = from.y_direction().xyz();
        let z_dir = from.direction().xyz();
        let mut ma1 = Mat::from_columns(*x_dir, *y_dir, *z_dir);
        ma1.transpose();
        let mut ma1_loc = *from.location().xyz();
        ma1_loc.multiply_mat(&ma1);
        ma1_loc.reverse();
        ma1_loc.multiply_mat(&self.matrix);
        self.loc.add(&ma1_loc);
        self.matrix.multiply_mat(&ma1);
    }
    // gp_Trsf::SetTransformation(FromSystem1, ToSystem2)
    pub fn set_transformation_between(&mut self, from: &Ax3, to: &Ax3) {
        self.shape = TrsfForm::CompoundTrsf;
        self.scale = 1.0;
        self.matrix.set_rows(to.x_direction().xyz(), to.y_direction().xyz(), to.direction().xyz());
        self.loc = *to.location().xyz();
        self.loc.multiply_mat(&self.matrix);
        self.loc.reverse();
        let x_dir = from.x_direction().xyz();
        let y_dir = from.y_direction().xyz();
        let z_dir = from.direction().xyz();
        let ma1 = Mat::from_columns(*x_dir, *y_dir, *z_dir);
        let mut ma1_loc = *from.location().xyz();
        ma1_loc.multiply_mat(&self.matrix);
        self.loc.add(&ma1_loc);
        self.matrix.multiply_mat(&ma1);
    }
    // gp_Trsf::SetTransformation(ToSystem)
    pub fn set_transformation(&mut self, to: &Ax3) {
        self.shape = TrsfForm::CompoundTrsf;
        self.scale = 1.0;
        self.matrix.set_rows(to.x_direction().xyz(), to.y_direction().xyz(), to.direction().xyz());
        self.loc = *to.location().xyz();
        self.loc.multiply_mat(&self.matrix);
        self.loc.reverse();
    }
    // gp_Trsf::SetTransformation(R, T)
    pub fn set_transformation_quaternion(&mut self, r: &Quaternion, t: &Vec) {
        self.shape = TrsfForm::CompoundTrsf;
        self.scale = 1.0;
        self.loc = *t.xyz();
        self.matrix = r.get_matrix();
    }
    // gp_Trsf::SetTranslation(gp_Vec)
    pub fn set_translation(&mut self, v: &Vec) {
        self.shape = TrsfForm::Translation;
        self.scale = 1.0;
        self.matrix.set_identity();
        self.loc = *v.xyz();
    }
    // gp_Trsf::SetTranslation(P1, P2)
    pub fn set_translation_points(&mut self, p1: &Pnt, p2: &Pnt) {
        self.shape = TrsfForm::Translation;
        self.scale = 1.0;
        self.matrix.set_identity();
        self.loc = p2.xyz().subtracted(p1.xyz());
    }
    // gp_Trsf::SetTranslationPart
    pub fn set_translation_part(&mut self, v: &Vec) {
        self.loc = *v.xyz();
        let loc_null = self.loc.square_modulus() < resolution();
        match self.shape {
            TrsfForm::Identity => {
                if !loc_null {
                    self.shape = TrsfForm::Translation;
                }
            }
            TrsfForm::Translation => {
                if loc_null {
                    self.shape = TrsfForm::Identity;
                }
            }
            _ => {
                if !loc_null {
                    self.shape = TrsfForm::CompoundTrsf;
                }
            }
        }
    }
    // gp_Trsf::SetScaleFactor
    pub fn set_scale_factor(&mut self, s: f64) -> Result<(), ConstructionError> {
        if s.abs() <= resolution() {
            return Err(ConstructionError);
        }
        self.scale = s;
        let unit = (self.scale - 1.0).abs() <= resolution();
        let munit = (self.scale + 1.0).abs() <= resolution();
        match self.shape {
            TrsfForm::Identity | TrsfForm::Translation => {
                if !unit {
                    self.shape = TrsfForm::Scale;
                }
                if munit {
                    self.shape = TrsfForm::PntMirror;
                }
            }
            TrsfForm::Rotation => {
                if !unit {
                    self.shape = TrsfForm::CompoundTrsf;
                }
            }
            TrsfForm::PntMirror | TrsfForm::Ax1Mirror | TrsfForm::Ax2Mirror => {
                if !munit {
                    self.shape = TrsfForm::Scale;
                }
                if unit {
                    self.shape = TrsfForm::Identity;
                }
            }
            TrsfForm::Scale => {
                if unit {
                    self.shape = TrsfForm::Identity;
                }
                if munit {
                    self.shape = TrsfForm::PntMirror;
                }
            }
            TrsfForm::CompoundTrsf | TrsfForm::Other => {}
        }
        Ok(())
    }
    // gp_Trsf::SetForm
    pub fn set_form(&mut self, form: TrsfForm) {
        self.shape = form;
    }
    // gp_Trsf::SetValues
    pub fn set_values(&mut self, v: [f64; 12]) -> Result<(), ConstructionError> {
        let [a11, a12, a13, a14, a21, a22, a23, a24, a31, a32, a33, a34] = v;
        let col1 = XYZ::new(a11, a21, a31);
        let col2 = XYZ::new(a12, a22, a32);
        let col3 = XYZ::new(a13, a23, a33);
        let col4 = XYZ::new(a14, a24, a34);
        let mut m = Mat::from_columns(col1, col2, col3);
        let mut s = m.determinant();
        if s.abs() < resolution() {
            return Err(ConstructionError);
        }
        if s > 0.0 {
            s = libm::pow(s, 1. / 3.);
        } else {
            s = -libm::pow(-s, 1. / 3.);
        }
        m.divide(s);
        let mut t = *self;
        t.scale = s;
        t.shape = TrsfForm::CompoundTrsf;
        t.matrix = m;
        t.orthogonalize()?;
        t.loc = col4;
        *self = t;
        Ok(())
    }
    // gp_Trsf::IsNegative
    pub fn is_negative(&self) -> bool {
        self.scale < 0.0
    }
    // gp_Trsf::Form
    pub fn form(&self) -> TrsfForm {
        self.shape
    }
    // gp_Trsf::ScaleFactor
    pub fn scale_factor(&self) -> f64 {
        self.scale
    }
    // gp_Trsf::TranslationPart
    pub fn translation_part(&self) -> &XYZ {
        &self.loc
    }
    // gp_Trsf::GetRotation(axis, angle); always true
    pub fn get_rotation(&self) -> (bool, XYZ, f64) {
        let q = self.get_rotation_quaternion();
        let (axis, angle) = q.vector_and_angle();
        (true, *axis.xyz(), angle)
    }
    // gp_Trsf::GetRotation()
    pub fn get_rotation_quaternion(&self) -> Quaternion {
        Quaternion::from_matrix(&self.matrix)
    }
    // gp_Trsf::VectorialPart
    pub fn vectorial_part(&self) -> Mat {
        if self.scale == 1.0 {
            return self.matrix;
        }
        let mut m = self.matrix;
        if self.shape == TrsfForm::Scale || self.shape == TrsfForm::PntMirror {
            let d = m.diagonal();
            m.set_diagonal(self.scale * d.x(), self.scale * d.y(), self.scale * d.z());
        } else {
            m.multiply_scalar(self.scale);
        }
        m
    }
    // gp_Trsf::HVectorialPart
    pub fn h_vectorial_part(&self) -> &Mat {
        &self.matrix
    }
    // gp_Trsf::Value
    pub fn value(&self, row: i32, col: i32) -> Result<f64, ConstructionError> {
        if !(1..=3).contains(&row) || !(1..=4).contains(&col) {
            return Err(ConstructionError);
        }
        if col < 4 { Ok(self.scale * self.matrix.value(row, col)?) } else { self.loc.coord(row) }
    }
    // gp_Trsf::Invert
    pub fn invert(&mut self) -> Result<(), ConstructionError> {
        if self.shape == TrsfForm::Identity {
        } else if self.shape == TrsfForm::Translation || self.shape == TrsfForm::PntMirror {
            self.loc.reverse();
        } else if self.shape == TrsfForm::Scale {
            if self.scale.abs() <= resolution() {
                return Err(ConstructionError);
            }
            self.scale = 1.0 / self.scale;
            self.loc.multiply(-self.scale);
        } else {
            if self.scale.abs() <= resolution() {
                return Err(ConstructionError);
            }
            self.scale = 1.0 / self.scale;
            self.matrix.transpose();
            self.loc.multiply_mat(&self.matrix);
            self.loc.multiply(-self.scale);
        }
        Ok(())
    }
    // gp_Trsf::Inverted
    pub fn inverted(&self) -> Result<Trsf, ConstructionError> {
        let mut t = *self;
        t.invert()?;
        Ok(t)
    }
    // gp_Trsf::Multiplied
    pub fn multiplied(&self, t: &Trsf) -> Trsf {
        let mut r = *self;
        r.multiply(t);
        r
    }
    // gp_Trsf::Multiply
    pub fn multiply(&mut self, t: &Trsf) {
        use TrsfForm::{Ax1Mirror, Ax2Mirror, CompoundTrsf, Identity, PntMirror, Rotation, Scale, Translation};
        let (shape, tshape) = (self.shape, t.shape);
        if tshape == Identity {
        } else if shape == Identity {
            self.shape = t.shape;
            self.scale = t.scale;
            self.loc = t.loc;
            self.matrix = t.matrix;
        } else if shape == Rotation && tshape == Rotation {
            if t.loc.x() != 0.0 || t.loc.y() != 0.0 || t.loc.z() != 0.0 {
                self.loc.add(&t.loc.multiplied_mat(&self.matrix));
            }
            self.matrix.multiply_mat(&t.matrix);
        } else if shape == Translation && tshape == Translation {
            self.loc.add(&t.loc);
        } else if shape == Scale && tshape == Scale {
            self.loc.add(&t.loc.multiplied(self.scale));
            self.scale *= t.scale;
        } else if shape == PntMirror && tshape == PntMirror {
            self.scale = 1.0;
            self.shape = Translation;
            self.loc.add(&t.loc.reversed());
        } else if shape == Ax1Mirror && tshape == Ax1Mirror {
            self.shape = Rotation;
            self.loc.add(&t.loc.multiplied_mat(&self.matrix));
            self.matrix.multiply_mat(&t.matrix);
        } else if (shape == CompoundTrsf || shape == Rotation || shape == Ax1Mirror || shape == Ax2Mirror)
            && tshape == Translation
        {
            let mut tloc = t.loc;
            tloc.multiply_mat(&self.matrix);
            if self.scale != 1.0 {
                tloc.multiply(self.scale);
            }
            self.loc.add(&tloc);
        } else if (shape == Scale || shape == PntMirror) && tshape == Translation {
            let mut tloc = t.loc;
            tloc.multiply(self.scale);
            self.loc.add(&tloc);
        } else if shape == Translation
            && (tshape == CompoundTrsf || tshape == Rotation || tshape == Ax1Mirror || tshape == Ax2Mirror)
        {
            self.shape = CompoundTrsf;
            self.scale = t.scale;
            self.loc.add(&t.loc);
            self.matrix = t.matrix;
        } else if shape == Translation && (tshape == Scale || tshape == PntMirror) {
            self.shape = t.shape;
            self.loc.add(&t.loc);
            self.scale = t.scale;
        } else if (shape == PntMirror || shape == Scale) && (tshape == PntMirror || tshape == Scale) {
            self.shape = CompoundTrsf;
            let mut tloc = t.loc;
            tloc.multiply(self.scale);
            self.loc.add(&tloc);
            self.scale *= t.scale;
        } else if (shape == CompoundTrsf || shape == Rotation || shape == Ax1Mirror || shape == Ax2Mirror)
            && (tshape == Scale || tshape == PntMirror)
        {
            self.shape = CompoundTrsf;
            let mut tloc = t.loc;
            if self.scale == 1.0 {
                self.scale = t.scale;
                tloc.multiply_mat(&self.matrix);
            } else {
                tloc.multiply_mat(&self.matrix);
                tloc.multiply(self.scale);
                self.scale *= t.scale;
            }
            self.loc.add(&tloc);
        } else if (tshape == CompoundTrsf || tshape == Rotation || tshape == Ax1Mirror || tshape == Ax2Mirror)
            && (shape == Scale || shape == PntMirror)
        {
            self.shape = CompoundTrsf;
            let mut tloc = t.loc;
            tloc.multiply(self.scale);
            self.loc.add(&tloc);
            self.scale *= t.scale;
            self.matrix = t.matrix;
        } else {
            self.shape = CompoundTrsf;
            let mut tloc = t.loc;
            tloc.multiply_mat(&self.matrix);
            if self.scale != 1.0 {
                tloc.multiply(self.scale);
                self.scale *= t.scale;
            } else {
                self.scale = t.scale;
            }
            self.loc.add(&tloc);
            self.matrix.multiply_mat(&t.matrix);
        }
    }
    // gp_Trsf::PreMultiply
    pub fn pre_multiply(&mut self, t: &Trsf) {
        use TrsfForm::{Ax1Mirror, Ax2Mirror, CompoundTrsf, Identity, PntMirror, Rotation, Scale, Translation};
        let (shape, tshape) = (self.shape, t.shape);
        if tshape == Identity {
        } else if shape == Identity {
            self.shape = t.shape;
            self.scale = t.scale;
            self.loc = t.loc;
            self.matrix = t.matrix;
        } else if shape == Rotation && tshape == Rotation {
            self.loc.multiply_mat(&t.matrix);
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        } else if shape == Translation && tshape == Translation {
            self.loc.add(&t.loc);
        } else if shape == Scale && tshape == Scale {
            self.loc.multiply(t.scale);
            self.loc.add(&t.loc);
            self.scale *= t.scale;
        } else if shape == PntMirror && tshape == PntMirror {
            self.scale = 1.0;
            self.shape = Translation;
            self.loc.reverse();
            self.loc.add(&t.loc);
        } else if shape == Ax1Mirror && tshape == Ax1Mirror {
            self.shape = Rotation;
            self.loc.multiply_mat(&t.matrix);
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        } else if (shape == CompoundTrsf
            || shape == Rotation
            || shape == Ax1Mirror
            || shape == Ax2Mirror
            || shape == Scale
            || shape == PntMirror)
            && tshape == Translation
        {
            // two C++ branches with the same body
            self.loc.add(&t.loc);
        } else if shape == Translation
            && (tshape == CompoundTrsf || tshape == Rotation || tshape == Ax1Mirror || tshape == Ax2Mirror)
        {
            self.shape = CompoundTrsf;
            self.matrix = t.matrix;
            if t.scale == 1.0 {
                self.loc.multiply_mat(&t.matrix);
            } else {
                self.scale = t.scale;
                self.loc.multiply_mat(&self.matrix);
                self.loc.multiply(self.scale);
            }
            self.loc.add(&t.loc);
        } else if (tshape == Scale || tshape == PntMirror) && shape == Translation {
            self.loc.multiply(t.scale);
            self.loc.add(&t.loc);
            self.scale = t.scale;
            self.shape = t.shape;
        } else if (shape == PntMirror
            || shape == Scale
            || shape == CompoundTrsf
            || shape == Rotation
            || shape == Ax1Mirror
            || shape == Ax2Mirror)
            && (tshape == PntMirror || tshape == Scale)
        {
            // two C++ branches with the same body
            self.shape = CompoundTrsf;
            self.loc.multiply(t.scale);
            self.loc.add(&t.loc);
            self.scale *= t.scale;
        } else if (tshape == CompoundTrsf || tshape == Rotation || tshape == Ax1Mirror || tshape == Ax2Mirror)
            && (shape == Scale || shape == PntMirror)
        {
            self.shape = CompoundTrsf;
            self.matrix = t.matrix;
            if t.scale == 1.0 {
                self.loc.multiply_mat(&t.matrix);
            } else {
                self.loc.multiply_mat(&self.matrix);
                self.loc.multiply(t.scale);
                self.scale *= t.scale;
            }
            self.loc.add(&t.loc);
        } else {
            self.shape = CompoundTrsf;
            self.loc.multiply_mat(&t.matrix);
            if t.scale != 1.0 {
                self.loc.multiply(t.scale);
                self.scale *= t.scale;
            }
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        }
    }
    // gp_Trsf::Power
    pub fn power(&mut self, n: i32) -> Result<(), ConstructionError> {
        use TrsfForm::{Ax1Mirror, Ax2Mirror, Identity, PntMirror, Rotation, Scale, Translation};
        if self.shape == Identity {
            return Ok(());
        }
        if n == 0 {
            self.scale = 1.0;
            self.shape = Identity;
            self.matrix.set_identity();
            self.loc = XYZ::new(0.0, 0.0, 0.0);
        } else if n == 1 {
        } else if n == -1 {
            self.invert()?;
        } else {
            if n < 0 {
                self.invert()?;
            }
            let abs_n = i64::from(n).abs();
            if self.shape == Translation {
                let mut npower = abs_n - 1;
                let mut temploc = self.loc;
                loop {
                    if is_odd(npower) {
                        self.loc.add(&temploc);
                    }
                    if npower == 1 {
                        break;
                    }
                    let t = temploc;
                    temploc.add(&t);
                    npower /= 2;
                }
            } else if self.shape == Scale {
                let mut npower = abs_n - 1;
                let mut temploc = self.loc;
                let mut tempscale = self.scale;
                loop {
                    if is_odd(npower) {
                        self.loc.add(&temploc.multiplied(self.scale));
                        self.scale *= tempscale;
                    }
                    if npower == 1 {
                        break;
                    }
                    let m = temploc.multiplied(tempscale);
                    temploc.add(&m);
                    tempscale *= tempscale;
                    npower /= 2;
                }
            } else if self.shape == Rotation {
                let mut npower = abs_n - 1;
                let mut tempmatrix = self.matrix;
                if self.loc.x() == 0.0 && self.loc.y() == 0.0 && self.loc.z() == 0.0 {
                    loop {
                        if is_odd(npower) {
                            self.matrix.multiply_mat(&tempmatrix);
                        }
                        if npower == 1 {
                            break;
                        }
                        let tm = tempmatrix;
                        tempmatrix.multiply_mat(&tm);
                        npower /= 2;
                    }
                } else {
                    let mut temploc = self.loc;
                    loop {
                        if is_odd(npower) {
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
                }
            } else if self.shape == PntMirror || self.shape == Ax1Mirror || self.shape == Ax2Mirror {
                if n % 2 == 0 {
                    self.shape = Identity;
                    self.scale = 1.0;
                    self.matrix.set_identity();
                    self.loc.set_x(0.0);
                    self.loc.set_y(0.0);
                    self.loc.set_z(0.0);
                }
            } else {
                self.shape = TrsfForm::CompoundTrsf;
                let mut npower = abs_n - 1;
                let mut temploc = self.loc;
                let mut tempscale = self.scale;
                let mut tempmatrix = self.matrix;
                loop {
                    if is_odd(npower) {
                        self.loc.add(&temploc.multiplied_mat(&self.matrix).multiplied(self.scale));
                        self.scale *= tempscale;
                        self.matrix.multiply_mat(&tempmatrix);
                    }
                    if npower == 1 {
                        break;
                    }
                    tempscale *= tempscale;
                    let m = temploc.multiplied_mat(&tempmatrix).multiplied(tempscale);
                    temploc.add(&m);
                    let tm = tempmatrix;
                    tempmatrix.multiply_mat(&tm);
                    npower /= 2;
                }
            }
        }
        Ok(())
    }
    // gp_Trsf::Powered
    pub fn powered(&self, n: i32) -> Result<Trsf, ConstructionError> {
        let mut t = *self;
        t.power(n)?;
        Ok(t)
    }
    // gp_Trsf::Transforms(gp_XYZ)
    pub fn transforms_xyz(&self, coord: &mut XYZ) {
        coord.multiply_mat(&self.matrix);
        if self.scale != 1.0 {
            coord.multiply(self.scale);
        }
        coord.add(&self.loc);
    }
    // gp_Trsf::Transforms(x, y, z)
    pub fn transforms_coords(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        let mut triplet = XYZ::new(x, y, z);
        self.transforms_xyz(&mut triplet);
        triplet.coords()
    }
    // gp_Trsf::GetMat4 (zero-based 4x4 layout, row by row)
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
    // gp_Trsf::Orthogonalize (protected)
    fn orthogonalize(&mut self) -> Result<(), ConstructionError> {
        let mut tm = self.matrix;
        let mut v1 = tm.column(1)?;
        let mut v2 = tm.column(2)?;
        let mut v3 = tm.column(3)?;
        v1.normalize()?;
        v2.subtract(&v1.multiplied(v2.dot(&v1)));
        v2.normalize()?;
        v3.subtract(&v1.multiplied(v3.dot(&v1)).added(&v2.multiplied(v3.dot(&v2))));
        v3.normalize()?;
        tm.set_cols(&v1, &v2, &v3);
        v1 = tm.row(1)?;
        v2 = tm.row(2)?;
        v3 = tm.row(3)?;
        v1.normalize()?;
        v2.subtract(&v1.multiplied(v2.dot(&v1)));
        v2.normalize()?;
        v3.subtract(&v1.multiplied(v3.dot(&v1)).added(&v2.multiplied(v3.dot(&v2))));
        v3.normalize()?;
        tm.set_rows(&v1, &v2, &v3);
        self.matrix = tm;
        Ok(())
    }
}
