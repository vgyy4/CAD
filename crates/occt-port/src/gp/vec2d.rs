//! gp_Vec2d
use super::{Ax2d, ConstructionError, Dir2d, Pnt2d, Trsf2d, TrsfForm, XY, resolution};
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};

/// gp_Vec2d
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2d(pub(crate) XY);

/// Things a 2D vector can be mirrored about: a vector or an axis.
pub trait Vec2dMirror {
    /// Mirrors `v` about `self` (gp_Vec2d::Mirror).
    fn mirror_vec2d(&self, v: &mut Vec2d);
}

fn mirror_about(v: &mut Vec2d, dir_x: f64, dir_y: f64) {
    let orig_x = v.0.x();
    let orig_y = v.0.y();
    let cross_term = 2.0 * dir_x * dir_y;
    let xx_term = 2.0 * dir_x * dir_x - 1.0;
    let yy_term = 2.0 * dir_y * dir_y - 1.0;
    v.0.set_x(xx_term * orig_x + cross_term * orig_y);
    v.0.set_y(cross_term * orig_x + yy_term * orig_y);
}

impl Vec2dMirror for Vec2d {
    // gp_Vec2d::Mirror(gp_Vec2d); a vector of (nearly) zero length leaves `v` unchanged
    fn mirror_vec2d(&self, v: &mut Vec2d) {
        let magnitude = self.0.modulus();
        if magnitude > resolution() {
            let norm_dir_x = self.0.x() / magnitude;
            let norm_dir_y = self.0.y() / magnitude;
            mirror_about(v, norm_dir_x, norm_dir_y);
        }
    }
}

impl Vec2dMirror for Ax2d {
    // gp_Vec2d::Mirror(gp_Ax2d)
    fn mirror_vec2d(&self, v: &mut Vec2d) {
        let d = self.direction().xy();
        mirror_about(v, d.x(), d.y());
    }
}

impl Vec2d {
    // gp_Vec2d::gp_Vec2d(x, y)
    pub fn new(x: f64, y: f64) -> Self {
        Self(XY::new(x, y))
    }
    // gp_Vec2d::gp_Vec2d(gp_XY)
    pub fn from_xy(xy: XY) -> Self {
        Self(xy)
    }
    // gp_Vec2d::gp_Vec2d(gp_Dir2d)
    pub fn from_dir(d: &Dir2d) -> Self {
        Self(*d.xy())
    }
    // gp_Vec2d::gp_Vec2d(P1, P2)
    pub fn between(p1: &Pnt2d, p2: &Pnt2d) -> Self {
        Self(p2.xy().subtracted(p1.xy()))
    }
    // gp_Vec2d::gp_Vec2d()
    pub fn zero() -> Self {
        Self::new(0., 0.)
    }
    // gp_Vec2d::X
    pub fn x(&self) -> f64 {
        self.0.x()
    }
    // gp_Vec2d::Y
    pub fn y(&self) -> f64 {
        self.0.y()
    }
    // gp_Vec2d::Coord(x, y)
    pub fn coords(&self) -> (f64, f64) {
        self.0.coords()
    }
    // gp_Vec2d::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        self.0.coord(index)
    }
    // gp_Vec2d::XY
    pub fn xy(&self) -> &XY {
        &self.0
    }
    // gp_Vec2d::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        self.0.set_coord_at(index, value)
    }
    // gp_Vec2d::SetCoord(x, y)
    pub fn set_coord(&mut self, x: f64, y: f64) {
        self.0.set_coord(x, y);
    }
    // gp_Vec2d::SetX
    pub fn set_x(&mut self, x: f64) {
        self.0.set_x(x);
    }
    // gp_Vec2d::SetY
    pub fn set_y(&mut self, y: f64) {
        self.0.set_y(y);
    }
    // gp_Vec2d::SetXY
    pub fn set_xy(&mut self, xy: XY) {
        self.0 = xy;
    }
    // gp_Vec2d::IsEqual
    pub fn is_equal(
        &self,
        other: &Vec2d,
        linear_tolerance: f64,
        angular_tolerance: f64,
    ) -> Result<bool, ConstructionError> {
        let norm = self.magnitude();
        let other_norm = other.magnitude();
        let val = (norm - other_norm).abs();
        let is_equal_length = val <= linear_tolerance;
        if norm > linear_tolerance && other_norm > linear_tolerance {
            let ang = self.angle(other)?.abs();
            return Ok(is_equal_length && ang <= angular_tolerance);
        }
        Ok(is_equal_length)
    }
    // gp_Vec2d::IsNormal; `!(a > b)` is kept as written (it differs from `a <= b` for NaN)
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn is_normal(&self, other: &Vec2d, angular_tolerance: f64) -> Result<bool, ConstructionError> {
        let ang = (FRAC_PI_2 - self.angle(other)?.abs()).abs();
        Ok(!(ang > angular_tolerance))
    }
    // gp_Vec2d::IsOpposite
    pub fn is_opposite(&self, other: &Vec2d, angular_tolerance: f64) -> Result<bool, ConstructionError> {
        let ang = self.angle(other)?.abs();
        Ok(PI - ang <= angular_tolerance)
    }
    // gp_Vec2d::IsParallel
    pub fn is_parallel(&self, other: &Vec2d, angular_tolerance: f64) -> Result<bool, ConstructionError> {
        let ang = self.angle(other)?.abs();
        Ok(ang <= angular_tolerance || PI - ang <= angular_tolerance)
    }
    // gp_Vec2d::Angle
    pub fn angle(&self, other: &Vec2d) -> Result<f64, ConstructionError> {
        let norm = self.magnitude();
        let other_norm = other.magnitude();
        if norm <= resolution() || other_norm <= resolution() {
            return Err(ConstructionError);
        }
        let d = norm * other_norm;
        let cosinus = self.0.dot(&other.0) / d;
        let sinus = self.0.crossed(&other.0) / d;
        let cos_45_deg = FRAC_1_SQRT_2;
        Ok(if cosinus > -cos_45_deg && cosinus < cos_45_deg {
            if sinus > 0.0 { libm::acos(cosinus) } else { -libm::acos(cosinus) }
        } else if cosinus > 0.0 {
            libm::asin(sinus)
        } else if sinus > 0.0 {
            PI - libm::asin(sinus)
        } else {
            -PI - libm::asin(sinus)
        })
    }
    // gp_Vec2d::Magnitude
    pub fn magnitude(&self) -> f64 {
        self.0.modulus()
    }
    // gp_Vec2d::SquareMagnitude
    pub fn square_magnitude(&self) -> f64 {
        self.0.square_modulus()
    }
    // gp_Vec2d::Add
    pub fn add(&mut self, other: &Vec2d) {
        self.0.add(&other.0);
    }
    // gp_Vec2d::Added
    pub fn added(&self, other: &Vec2d) -> Vec2d {
        let mut v = *self;
        v.0.add(&other.0);
        v
    }
    // gp_Vec2d::Crossed
    pub fn crossed(&self, right: &Vec2d) -> f64 {
        self.0.crossed(&right.0)
    }
    // gp_Vec2d::CrossMagnitude
    pub fn cross_magnitude(&self, right: &Vec2d) -> f64 {
        self.0.cross_magnitude(&right.0)
    }
    // gp_Vec2d::CrossSquareMagnitude
    pub fn cross_square_magnitude(&self, right: &Vec2d) -> f64 {
        self.0.cross_square_magnitude(&right.0)
    }
    // gp_Vec2d::Divide (no zero check in OCCT)
    pub fn divide(&mut self, scalar: f64) {
        self.0.divide(scalar);
    }
    // gp_Vec2d::Divided (no zero check in OCCT)
    pub fn divided(&self, scalar: f64) -> Vec2d {
        let mut v = *self;
        v.0.divide(scalar);
        v
    }
    // gp_Vec2d::Dot
    pub fn dot(&self, other: &Vec2d) -> f64 {
        self.0.dot(&other.0)
    }
    // gp_Vec2d::GetNormal
    pub fn get_normal(&self) -> Vec2d {
        Vec2d::new(self.y(), -self.x())
    }
    // gp_Vec2d::Multiply
    pub fn multiply(&mut self, scalar: f64) {
        self.0.multiply(scalar);
    }
    // gp_Vec2d::Multiplied
    pub fn multiplied(&self, scalar: f64) -> Vec2d {
        let mut v = *self;
        v.0.multiply(scalar);
        v
    }
    // gp_Vec2d::Normalize
    pub fn normalize(&mut self) -> Result<(), ConstructionError> {
        let d = self.0.modulus();
        if d <= resolution() {
            return Err(ConstructionError);
        }
        self.0.divide(d);
        Ok(())
    }
    // gp_Vec2d::Normalized
    pub fn normalized(&self) -> Result<Vec2d, ConstructionError> {
        let mut v = *self;
        v.normalize()?;
        Ok(v)
    }
    // gp_Vec2d::Reverse
    pub fn reverse(&mut self) {
        self.0.reverse();
    }
    // gp_Vec2d::Reversed
    pub fn reversed(&self) -> Vec2d {
        let mut v = *self;
        v.0.reverse();
        v
    }
    // gp_Vec2d::Subtract
    pub fn subtract(&mut self, right: &Vec2d) {
        self.0.subtract(&right.0);
    }
    // gp_Vec2d::Subtracted
    pub fn subtracted(&self, right: &Vec2d) -> Vec2d {
        let mut v = *self;
        v.0.subtract(&right.0);
        v
    }
    // gp_Vec2d::SetLinearForm(A1, V1, A2, V2)
    pub fn set_linear_form_2(&mut self, a1: f64, v1: &Vec2d, a2: f64, v2: &Vec2d) {
        self.0.set_linear_form_2(a1, &v1.0, a2, &v2.0);
    }
    // gp_Vec2d::SetLinearForm(A1, V1, A2, V2, V3)
    pub fn set_linear_form_2_plus(&mut self, a1: f64, v1: &Vec2d, a2: f64, v2: &Vec2d, v3: &Vec2d) {
        self.0.set_linear_form_2_plus(a1, &v1.0, a2, &v2.0, &v3.0);
    }
    // gp_Vec2d::SetLinearForm(A1, V1, V2)
    pub fn set_linear_form_1_plus(&mut self, a1: f64, v1: &Vec2d, v2: &Vec2d) {
        self.0.set_linear_form_1_plus(a1, &v1.0, &v2.0);
    }
    // gp_Vec2d::SetLinearForm(V1, V2)
    pub fn set_linear_form_sum(&mut self, v1: &Vec2d, v2: &Vec2d) {
        self.0.set_linear_form_sum(&v1.0, &v2.0);
    }
    // gp_Vec2d::Mirror
    pub fn mirror<M: Vec2dMirror>(&mut self, m: &M) {
        m.mirror_vec2d(self);
    }
    // gp_Vec2d::Mirrored
    pub fn mirrored<M: Vec2dMirror>(&self, m: &M) -> Vec2d {
        let mut v = *self;
        m.mirror_vec2d(&mut v);
        v
    }
    // gp_Vec2d::Rotate
    pub fn rotate(&mut self, ang: f64) {
        let mut t = Trsf2d::new();
        t.set_rotation(&Pnt2d::new(0.0, 0.0), ang);
        self.0.multiply_mat(&t.vectorial_part());
    }
    // gp_Vec2d::Rotated
    pub fn rotated(&self, ang: f64) -> Vec2d {
        let mut v = *self;
        v.rotate(ang);
        v
    }
    // gp_Vec2d::Scale
    pub fn scale(&mut self, s: f64) {
        self.0.multiply(s);
    }
    // gp_Vec2d::Scaled
    pub fn scaled(&self, s: f64) -> Vec2d {
        let mut v = *self;
        v.0.multiply(s);
        v
    }
    // gp_Vec2d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        match t.form() {
            TrsfForm::Identity | TrsfForm::Translation => {}
            TrsfForm::PntMirror => self.0.reverse(),
            TrsfForm::Scale => self.0.multiply(t.scale_factor()),
            _ => self.0.multiply_mat(&t.vectorial_part()),
        }
    }
    // gp_Vec2d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Vec2d {
        let mut v = *self;
        v.transform(t);
        v
    }
}
