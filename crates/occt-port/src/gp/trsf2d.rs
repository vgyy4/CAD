//! gp_Trsf2d
use super::{Ax2d, ConstructionError, Mat2d, Pnt2d, Trsf, TrsfForm, Vec2d, XY, resolution};

/// gp_Trsf2d: a 2D transformation (rotation, translation, mirror, scale and their products)
/// with a uniform scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trsf2d {
    pub(crate) scale: f64,
    pub(crate) shape: TrsfForm,
    pub(crate) matrix: Mat2d,
    pub(crate) loc: XY,
}

impl Default for Trsf2d {
    fn default() -> Self {
        Self::new()
    }
}

/// Things a 2D transformation can mirror about: a point or an axis.
pub trait Trsf2dMirror {
    /// Makes `t` the mirror about `self` (gp_Trsf2d::SetMirror).
    fn set_mirror_trsf2d(&self, t: &mut Trsf2d);
}

impl Trsf2dMirror for Pnt2d {
    // gp_Trsf2d::SetMirror(gp_Pnt2d)
    fn set_mirror_trsf2d(&self, t: &mut Trsf2d) {
        t.shape = TrsfForm::PntMirror;
        t.scale = -1.0;
        t.matrix.set_identity();
        t.loc = *self.xy();
        t.loc.multiply(2.0);
    }
}

impl Trsf2dMirror for Ax2d {
    // gp_Trsf2d::SetMirror(gp_Ax2d)
    fn set_mirror_trsf2d(&self, t: &mut Trsf2d) {
        t.shape = TrsfForm::Ax1Mirror;
        t.scale = -1.0;
        let v = self.direction();
        let p = self.location();
        let (vx, vy) = (v.x(), v.y());
        let (x0, y0) = (p.x(), p.y());
        // SetCol(1, ..) and SetCol(2, ..)
        t.matrix.m = [[1.0 - 2.0 * vx * vx, -2.0 * vx * vy], [-2.0 * vx * vy, 1.0 - 2.0 * vy * vy]];
        t.loc.set_coord(-2.0 * ((vx * vx - 1.0) * x0 + (vx * vy * y0)), -2.0 * ((vx * vy * x0) + (vy * vy - 1.0) * y0));
    }
}

fn is_odd(n: i64) -> bool {
    n & 1 == 1
}

fn is_even(n: i32) -> bool {
    n & 1 == 0
}

fn is_translation_scale_pntmirror(f: TrsfForm) -> bool {
    matches!(f, TrsfForm::Scale | TrsfForm::PntMirror)
}

/// CompoundTrsf, Rotation or Ax1Mirror: the forms with a general matrix part.
fn is_matrix_form(f: TrsfForm) -> bool {
    matches!(f, TrsfForm::CompoundTrsf | TrsfForm::Rotation | TrsfForm::Ax1Mirror)
}

impl Trsf2d {
    // gp_Trsf2d::gp_Trsf2d()
    pub fn new() -> Self {
        let mut matrix = Mat2d::new();
        matrix.set_identity();
        Self { scale: 1.0, shape: TrsfForm::Identity, matrix, loc: XY::new(0.0, 0.0) }
    }
    // gp_Trsf2d::gp_Trsf2d(gp_Trsf)
    pub fn from_trsf(t: &Trsf) -> Self {
        let m = t.h_vectorial_part();
        let tp = t.translation_part();
        Self {
            scale: t.scale_factor(),
            shape: t.form(),
            matrix: Mat2d { m: [[m.at(1, 1), m.at(1, 2)], [m.at(2, 1), m.at(2, 2)]] },
            loc: XY::new(tp.x(), tp.y()),
        }
    }
    // gp_Trsf2d::SetMirror
    pub fn set_mirror<M: Trsf2dMirror>(&mut self, m: &M) {
        m.set_mirror_trsf2d(self);
    }
    // gp_Trsf2d::SetRotation
    pub fn set_rotation(&mut self, p: &Pnt2d, ang: f64) {
        self.shape = TrsfForm::Rotation;
        self.scale = 1.0;
        self.loc = *p.xy();
        self.loc.reverse();
        self.matrix.set_rotation(ang);
        self.loc.multiply_mat(&self.matrix);
        self.loc.add(p.xy());
    }
    // gp_Trsf2d::SetScale
    pub fn set_scale(&mut self, p: &Pnt2d, s: f64) {
        self.shape = TrsfForm::Scale;
        self.scale = s;
        self.matrix.set_identity();
        self.loc = *p.xy();
        self.loc.multiply(1.0 - s);
    }
    // gp_Trsf2d::SetTransformation(FromSystem1, ToSystem2)
    pub fn set_transformation_between(&mut self, from_a1: &Ax2d, to_a2: &Ax2d) {
        self.shape = TrsfForm::CompoundTrsf;
        self.scale = 1.0;
        let v1 = *to_a2.direction().xy();
        let v2 = XY::new(-v1.y(), v1.x());
        let _ = self.matrix.set_col(1, &v1);
        let _ = self.matrix.set_col(2, &v2);
        self.loc = *to_a2.location().xy();
        self.matrix.transpose();
        self.loc.multiply_mat(&self.matrix);
        self.loc.reverse();
        let v3 = *from_a1.direction().xy();
        let v4 = XY::new(-v3.y(), v3.x());
        let ma1 = Mat2d::from_columns(v3, v4);
        let mut ma1_loc = *from_a1.location().xy();
        ma1_loc.multiply_mat(&self.matrix);
        self.loc.add(&ma1_loc);
        self.matrix.multiply_mat(&ma1);
    }
    // gp_Trsf2d::SetTransformation(ToSystem)
    pub fn set_transformation(&mut self, a: &Ax2d) {
        self.shape = TrsfForm::CompoundTrsf;
        self.scale = 1.0;
        let v1 = *a.direction().xy();
        let v2 = XY::new(-v1.y(), v1.x());
        let _ = self.matrix.set_col(1, &v1);
        let _ = self.matrix.set_col(2, &v2);
        self.loc = *a.location().xy();
        self.matrix.transpose();
        self.loc.multiply_mat(&self.matrix);
        self.loc.reverse();
    }
    // gp_Trsf2d::SetTranslation(gp_Vec2d)
    pub fn set_translation(&mut self, v: &Vec2d) {
        self.shape = TrsfForm::Translation;
        self.scale = 1.0;
        self.matrix.set_identity();
        self.loc = *v.xy();
    }
    // gp_Trsf2d::SetTranslation(P1, P2)
    pub fn set_translation_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
        self.shape = TrsfForm::Translation;
        self.scale = 1.0;
        self.matrix.set_identity();
        self.loc = p2.xy().subtracted(p1.xy());
    }
    // gp_Trsf2d::SetTranslationPart
    pub fn set_translation_part(&mut self, v: &Vec2d) {
        self.loc = *v.xy();
        if self.loc.x().abs() <= resolution() && self.loc.y().abs() <= resolution() {
            match self.shape {
                TrsfForm::Identity
                | TrsfForm::PntMirror
                | TrsfForm::Scale
                | TrsfForm::Rotation
                | TrsfForm::Ax1Mirror => {}
                TrsfForm::Translation => self.shape = TrsfForm::Identity,
                _ => self.shape = TrsfForm::CompoundTrsf,
            }
        } else {
            match self.shape {
                TrsfForm::Translation | TrsfForm::Scale | TrsfForm::PntMirror => {}
                TrsfForm::Identity => self.shape = TrsfForm::Translation,
                _ => self.shape = TrsfForm::CompoundTrsf,
            }
        }
    }
    // gp_Trsf2d::SetScaleFactor
    pub fn set_scale_factor(&mut self, s: f64) {
        if s == 1.0 {
            let mut x = self.loc.x();
            if x < 0.0 {
                x = -x;
            }
            let mut y = self.loc.y();
            if y < 0.0 {
                y = -y;
            }
            if x <= resolution() && y <= resolution() {
                match self.shape {
                    TrsfForm::Identity | TrsfForm::Rotation => {}
                    TrsfForm::Scale => self.shape = TrsfForm::Identity,
                    TrsfForm::PntMirror => self.shape = TrsfForm::Translation,
                    _ => self.shape = TrsfForm::CompoundTrsf,
                }
            } else {
                match self.shape {
                    TrsfForm::Identity | TrsfForm::Rotation | TrsfForm::Scale => {}
                    TrsfForm::PntMirror => self.shape = TrsfForm::Translation,
                    _ => self.shape = TrsfForm::CompoundTrsf,
                }
            }
        } else if s == -1.0 {
            match self.shape {
                TrsfForm::PntMirror | TrsfForm::Ax1Mirror => {}
                TrsfForm::Identity | TrsfForm::Scale => self.shape = TrsfForm::PntMirror,
                _ => self.shape = TrsfForm::CompoundTrsf,
            }
        } else {
            match self.shape {
                TrsfForm::Scale => {}
                TrsfForm::Identity | TrsfForm::Translation | TrsfForm::PntMirror => self.shape = TrsfForm::Scale,
                _ => self.shape = TrsfForm::CompoundTrsf,
            }
        }
        self.scale = s;
    }
    // gp_Trsf2d::IsNegative
    pub fn is_negative(&self) -> bool {
        self.matrix.determinant() < 0.0
    }
    // gp_Trsf2d::Form
    pub fn form(&self) -> TrsfForm {
        self.shape
    }
    // gp_Trsf2d::ScaleFactor
    pub fn scale_factor(&self) -> f64 {
        self.scale
    }
    // gp_Trsf2d::TranslationPart
    pub fn translation_part(&self) -> &XY {
        &self.loc
    }
    // gp_Trsf2d::VectorialPart
    pub fn vectorial_part(&self) -> Mat2d {
        if self.scale == 1.0 {
            return self.matrix;
        }
        let mut m = self.matrix;
        if self.shape == TrsfForm::Scale || self.shape == TrsfForm::PntMirror {
            let [[a, _], [_, d]] = self.matrix.m;
            m.set_diagonal(a * self.scale, d * self.scale);
        } else {
            m.multiply_scalar(self.scale);
        }
        m
    }
    // gp_Trsf2d::HVectorialPart
    pub fn h_vectorial_part(&self) -> &Mat2d {
        &self.matrix
    }
    // gp_Trsf2d::RotationPart
    pub fn rotation_part(&self) -> f64 {
        let [[a, _], [c, _]] = self.matrix.m;
        libm::atan2(c, a)
    }
    // gp_Trsf2d::Value
    pub fn value(&self, row: i32, col: i32) -> Result<f64, ConstructionError> {
        if !(1..=2).contains(&row) || !(1..=3).contains(&col) {
            return Err(ConstructionError);
        }
        if col < 3 { Ok(self.scale * self.matrix.value(row, col)?) } else { self.loc.coord(row) }
    }
    // gp_Trsf2d::Invert (a zero scale of a form that needs it is unchecked in OCCT 8.0.1 as built,
    // reported as an error)
    pub fn invert(&mut self) -> Result<(), ConstructionError> {
        match self.shape {
            TrsfForm::Identity => {}
            TrsfForm::Translation | TrsfForm::PntMirror => self.loc.reverse(),
            TrsfForm::Scale => {
                if self.scale.abs() <= resolution() {
                    return Err(ConstructionError);
                }
                self.scale = 1.0 / self.scale;
                self.loc.multiply(-self.scale);
            }
            _ => {
                if self.scale.abs() <= resolution() {
                    return Err(ConstructionError);
                }
                self.scale = 1.0 / self.scale;
                self.matrix.transpose();
                self.loc.multiply_mat(&self.matrix);
                self.loc.multiply(-self.scale);
            }
        }
        Ok(())
    }
    // gp_Trsf2d::Inverted
    pub fn inverted(&self) -> Result<Trsf2d, ConstructionError> {
        let mut t = *self;
        t.invert()?;
        Ok(t)
    }
    // gp_Trsf2d::Multiplied
    pub fn multiplied(&self, t: &Trsf2d) -> Trsf2d {
        let mut r = *self;
        r.multiply(t);
        r
    }
    // gp_Trsf2d::Multiply
    pub fn multiply(&mut self, t: &Trsf2d) {
        let (shape, tshape) = (self.shape, t.shape);
        if tshape == TrsfForm::Identity {
        } else if shape == TrsfForm::Identity {
            self.shape = tshape;
            self.scale = t.scale;
            self.loc = t.loc;
            self.matrix = t.matrix;
        } else if shape == TrsfForm::Rotation && tshape == TrsfForm::Rotation {
            if self.loc.x() != 0.0 || self.loc.y() != 0.0 {
                self.loc.add(&t.loc.multiplied_mat(&self.matrix));
            }
            self.matrix.multiply_mat(&t.matrix);
        } else if shape == TrsfForm::Translation && tshape == TrsfForm::Translation {
            self.loc.add(&t.loc);
        } else if shape == TrsfForm::Scale && tshape == TrsfForm::Scale {
            self.loc.add(&t.loc.multiplied(self.scale));
            self.scale *= t.scale;
        } else if shape == TrsfForm::PntMirror && tshape == TrsfForm::PntMirror {
            self.scale = 1.0;
            self.shape = TrsfForm::Translation;
            self.loc.add(&t.loc.reversed());
        } else if shape == TrsfForm::Ax1Mirror && tshape == TrsfForm::Ax1Mirror {
            self.shape = TrsfForm::Rotation;
            let mut tloc = t.loc;
            tloc.multiply_mat(&self.matrix);
            tloc.multiply(self.scale);
            self.scale *= t.scale;
            self.loc.add(&tloc);
            self.matrix.multiply_mat(&t.matrix);
        } else if is_matrix_form(shape) && tshape == TrsfForm::Translation {
            let mut tloc = t.loc;
            tloc.multiply_mat(&self.matrix);
            if self.scale != 1.0 {
                tloc.multiply(self.scale);
            }
            self.loc.add(&tloc);
        } else if is_translation_scale_pntmirror(shape) && tshape == TrsfForm::Translation {
            let mut tloc = t.loc;
            tloc.multiply(self.scale);
            self.loc.add(&tloc);
        } else if shape == TrsfForm::Translation && is_matrix_form(tshape) {
            self.shape = TrsfForm::CompoundTrsf;
            self.scale = t.scale;
            self.loc.add(&t.loc);
            self.matrix = t.matrix;
        } else if shape == TrsfForm::Translation && is_translation_scale_pntmirror(tshape) {
            self.shape = tshape;
            self.loc.add(&t.loc);
            self.scale = t.scale;
        } else if is_translation_scale_pntmirror(shape) && is_translation_scale_pntmirror(tshape) {
            self.shape = TrsfForm::CompoundTrsf;
            let mut tloc = t.loc;
            tloc.multiply(self.scale);
            self.loc.add(&tloc);
            self.scale *= t.scale;
        } else if is_matrix_form(shape) && is_translation_scale_pntmirror(tshape) {
            self.shape = TrsfForm::CompoundTrsf;
            let mut tloc = t.loc;
            tloc.multiply_mat(&self.matrix);
            if self.scale == 1.0 {
                self.scale = t.scale;
            } else {
                tloc.multiply(self.scale);
                self.scale *= t.scale;
            }
            self.loc.add(&tloc);
        } else if is_matrix_form(tshape) && is_translation_scale_pntmirror(shape) {
            self.shape = TrsfForm::CompoundTrsf;
            let mut tloc = t.loc;
            tloc.multiply(self.scale);
            self.scale *= t.scale;
            self.loc.add(&tloc);
            self.matrix = t.matrix;
        } else {
            self.shape = TrsfForm::CompoundTrsf;
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
    // gp_Trsf2d::PreMultiply
    pub fn pre_multiply(&mut self, t: &Trsf2d) {
        let (shape, tshape) = (self.shape, t.shape);
        if tshape == TrsfForm::Identity {
        } else if shape == TrsfForm::Identity {
            self.shape = tshape;
            self.scale = t.scale;
            self.loc = t.loc;
            self.matrix = t.matrix;
        } else if shape == TrsfForm::Rotation && tshape == TrsfForm::Rotation {
            self.loc.multiply_mat(&t.matrix);
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        } else if shape == TrsfForm::Translation && tshape == TrsfForm::Translation {
            self.loc.add(&t.loc);
        } else if shape == TrsfForm::Scale && tshape == TrsfForm::Scale {
            self.loc.multiply(t.scale);
            self.loc.add(&t.loc);
            self.scale *= t.scale;
        } else if shape == TrsfForm::PntMirror && tshape == TrsfForm::PntMirror {
            self.scale = 1.0;
            self.shape = TrsfForm::Translation;
            self.loc.reverse();
            self.loc.add(&t.loc);
        } else if shape == TrsfForm::Ax1Mirror && tshape == TrsfForm::Ax1Mirror {
            self.shape = TrsfForm::Rotation;
            self.loc.multiply_mat(&t.matrix);
            self.loc.multiply(t.scale);
            self.scale *= t.scale;
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        } else if (is_matrix_form(shape) || is_translation_scale_pntmirror(shape)) && tshape == TrsfForm::Translation {
            // OCCT has one branch for (Compound, Rotation, Ax1Mirror) and one for (Scale, PntMirror)
            // here; both only add the translation.
            self.loc.add(&t.loc);
        } else if shape == TrsfForm::Translation && is_matrix_form(tshape) {
            self.shape = TrsfForm::CompoundTrsf;
            self.matrix = t.matrix;
            if t.scale == 1.0 {
                self.loc.multiply_mat(&t.matrix);
            } else {
                self.scale = t.scale;
                self.loc.multiply_mat(&self.matrix);
                self.loc.multiply(self.scale);
            }
            self.loc.add(&t.loc);
        } else if is_translation_scale_pntmirror(tshape) && shape == TrsfForm::Translation {
            self.loc.multiply(t.scale);
            self.loc.add(&t.loc);
            self.scale = t.scale;
            self.shape = tshape;
        } else if (is_translation_scale_pntmirror(shape) || is_matrix_form(shape))
            && is_translation_scale_pntmirror(tshape)
        {
            // OCCT has one branch for (Scale, PntMirror) and one for (Compound, Rotation, Ax1Mirror)
            // here, with the same body.
            self.shape = TrsfForm::CompoundTrsf;
            self.loc.multiply(t.scale);
            self.loc.add(&t.loc);
            self.scale *= t.scale;
        } else if is_matrix_form(tshape) && is_translation_scale_pntmirror(shape) {
            self.shape = TrsfForm::CompoundTrsf;
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
            self.shape = TrsfForm::CompoundTrsf;
            self.loc.multiply_mat(&t.matrix);
            if t.scale != 1.0 {
                self.loc.multiply(t.scale);
                self.scale *= t.scale;
            }
            self.loc.add(&t.loc);
            self.matrix.pre_multiply(&t.matrix);
        }
    }
    // gp_Trsf2d::Power
    pub fn power(&mut self, n: i32) -> Result<(), ConstructionError> {
        if self.shape == TrsfForm::Identity {
            return Ok(());
        }
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
        let mut npower = i64::from(n).abs() - 1;
        match self.shape {
            TrsfForm::Translation => {
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
            }
            TrsfForm::Scale => {
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
            }
            TrsfForm::Rotation => {
                let mut tempmatrix = self.matrix;
                if self.loc.x() == 0.0 && self.loc.y() == 0.0 {
                    loop {
                        if is_odd(npower) {
                            self.matrix.multiply_mat(&tempmatrix);
                        }
                        if npower == 1 {
                            break;
                        }
                        tempmatrix.square_in_place();
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
                        tempmatrix.square_in_place();
                        npower /= 2;
                    }
                }
            }
            TrsfForm::PntMirror | TrsfForm::Ax1Mirror => {
                if is_even(n) {
                    self.shape = TrsfForm::Identity;
                    self.scale = 1.0;
                    self.matrix.set_identity();
                    self.loc.set_coord(0.0, 0.0);
                }
            }
            _ => {
                self.shape = TrsfForm::CompoundTrsf;
                let [[a, _], [_, d]] = self.matrix.m;
                self.matrix.set_diagonal(self.scale * a, self.scale * d);
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
                    tempmatrix.square_in_place();
                    npower /= 2;
                }
            }
        }
        Ok(())
    }
    // gp_Trsf2d::Powered
    pub fn powered(&self, n: i32) -> Result<Trsf2d, ConstructionError> {
        let mut t = *self;
        t.power(n)?;
        Ok(t)
    }
    // gp_Trsf2d::Transforms(gp_XY)
    pub fn transforms_xy(&self, coord: &mut XY) {
        coord.multiply_mat(&self.matrix);
        if self.scale != 1.0 {
            coord.multiply(self.scale);
        }
        coord.add(&self.loc);
    }
    // gp_Trsf2d::Transforms(x, y)
    pub fn transforms_coords(&self, x: f64, y: f64) -> (f64, f64) {
        let mut doublet = XY::new(x, y);
        self.transforms_xy(&mut doublet);
        doublet.coords()
    }
    // gp_Trsf2d::SetValues (the Orthogonalize step included)
    pub fn set_values(&mut self, v: [f64; 6]) -> Result<(), ConstructionError> {
        let [a11, a12, a13, a21, a22, a23] = v;
        let col3 = XY::new(a13, a23);
        let mut m = Mat2d::from_columns(XY::new(a11, a21), XY::new(a12, a22));
        let mut s = m.determinant();
        if s.abs() < resolution() {
            return Err(ConstructionError);
        }
        if s > 0.0 {
            s = s.sqrt();
        } else {
            s = (-s).sqrt();
        }
        m.divide(s);
        self.scale = s;
        self.shape = TrsfForm::CompoundTrsf;
        self.matrix = m;
        self.orthogonalize()?;
        self.loc = col3;
        Ok(())
    }
    // gp_Trsf2d::Orthogonalize
    fn orthogonalize(&mut self) -> Result<(), ConstructionError> {
        let mut tm = self.matrix;
        let mut v1 = tm.column(1)?;
        let mut v2 = tm.column(2)?;
        v1.normalize()?;
        v2.subtract(&v1.multiplied(v2.dot(&v1)));
        v2.normalize()?;
        tm.set_cols(&v1, &v2);
        v1 = tm.row(1)?;
        v2 = tm.row(2)?;
        v1.normalize()?;
        v2.subtract(&v1.multiplied(v2.dot(&v1)));
        v2.normalize()?;
        tm.set_rows(&v1, &v2);
        self.matrix = tm;
        Ok(())
    }
}
