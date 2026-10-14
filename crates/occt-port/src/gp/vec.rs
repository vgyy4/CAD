//! gp_Vec
use super::{Ax1, Ax2, ConstructionError, Dir, Pnt, Trsf, TrsfForm, XYZ, resolution};
use std::f64::consts::{FRAC_PI_2, PI};

/// gp_Vec
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec(pub(crate) XYZ);

/// Things a vector can be mirrored about: a vector, an axis or a plane (an `Ax2`).
pub trait VecMirror {
    /// Mirrors `v` about `self` (gp_Vec::Mirror).
    fn mirror_vec(&self, v: &mut Vec);
}

fn mirror_terms(v: &mut Vec, dx: f64, dy: f64, dz: f64) {
    let (ox, oy, oz) = v.0.coords();
    let cross_xy = 2.0 * dx * dy;
    let cross_xz = 2.0 * dx * dz;
    let cross_yz = 2.0 * dy * dz;
    let xx = 2.0 * dx * dx - 1.0;
    let yy = 2.0 * dy * dy - 1.0;
    let zz = 2.0 * dz * dz - 1.0;
    v.0.set_x(xx * ox + cross_xy * oy + cross_xz * oz);
    v.0.set_y(cross_xy * ox + yy * oy + cross_yz * oz);
    v.0.set_z(cross_xz * ox + cross_yz * oy + zz * oz);
}

impl VecMirror for Vec {
    // gp_Vec::Mirror(gp_Vec)
    fn mirror_vec(&self, v: &mut Vec) {
        let magnitude = self.0.modulus();
        if magnitude > resolution() {
            let (x, y, z) = self.0.coords();
            mirror_terms(v, x / magnitude, y / magnitude, z / magnitude);
        }
    }
}

impl VecMirror for Ax1 {
    // gp_Vec::Mirror(gp_Ax1)
    fn mirror_vec(&self, v: &mut Vec) {
        let (x, y, z) = self.direction().xyz().coords();
        mirror_terms(v, x, y, z);
    }
}

impl VecMirror for Ax2 {
    // gp_Vec::Mirror(gp_Ax2)
    fn mirror_vec(&self, v: &mut Vec) {
        let z_dir = *self.direction().xyz();
        let mir = z_dir.crossed(&v.0);
        if mir.modulus() <= resolution() {
            v.0.reverse();
        } else {
            let mut new_z = z_dir;
            new_z.cross(&mir);
            Vec(new_z).mirror_vec(v);
        }
    }
}

impl Vec {
    // gp_Vec::gp_Vec(x, y, z)
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self(XYZ::new(x, y, z))
    }
    // gp_Vec::gp_Vec(gp_XYZ)
    pub fn from_xyz(xyz: XYZ) -> Self {
        Self(xyz)
    }
    // gp_Vec::gp_Vec(gp_Dir)
    pub fn from_dir(d: &Dir) -> Self {
        Self(*d.xyz())
    }
    // gp_Vec::gp_Vec(P1, P2)
    pub fn between(p1: &Pnt, p2: &Pnt) -> Self {
        Self(p2.xyz().subtracted(p1.xyz()))
    }
    // gp_Vec::gp_Vec()
    pub fn zero() -> Self {
        Self::new(0., 0., 0.)
    }
    // gp_Vec::X
    pub fn x(&self) -> f64 {
        self.0.x()
    }
    // gp_Vec::Y
    pub fn y(&self) -> f64 {
        self.0.y()
    }
    // gp_Vec::Z
    pub fn z(&self) -> f64 {
        self.0.z()
    }
    // gp_Vec::Coord(x, y, z)
    pub fn coords(&self) -> (f64, f64, f64) {
        self.0.coords()
    }
    // gp_Vec::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        self.0.coord(index)
    }
    // gp_Vec::XYZ
    pub fn xyz(&self) -> &XYZ {
        &self.0
    }
    // gp_Vec::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        self.0.set_coord_at(index, value)
    }
    // gp_Vec::SetCoord(x, y, z)
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) {
        self.0.set_coord(x, y, z);
    }
    // gp_Vec::SetX
    pub fn set_x(&mut self, x: f64) {
        self.0.set_x(x);
    }
    // gp_Vec::SetY
    pub fn set_y(&mut self, y: f64) {
        self.0.set_y(y);
    }
    // gp_Vec::SetZ
    pub fn set_z(&mut self, z: f64) {
        self.0.set_z(z);
    }
    // gp_Vec::SetXYZ
    pub fn set_xyz(&mut self, xyz: XYZ) {
        self.0 = xyz;
    }
    // gp_Vec::IsEqual (a null-magnitude failure of Angle makes the vectors "not equal")
    pub fn is_equal(&self, other: &Vec, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        let magnitude = self.magnitude();
        let other_magnitude = other.magnitude();
        if magnitude <= linear_tolerance || other_magnitude <= linear_tolerance {
            let val = (magnitude - other_magnitude).abs();
            val <= linear_tolerance
        } else {
            let val = (magnitude - other_magnitude).abs();
            val <= linear_tolerance && matches!(self.angle(other), Ok(a) if a <= angular_tolerance)
        }
    }
    // gp_Vec::IsNormal
    pub fn is_normal(&self, other: &Vec, angular_tolerance: f64) -> Result<bool, ConstructionError> {
        let ang = (FRAC_PI_2 - self.angle(other)?).abs();
        Ok(ang <= angular_tolerance)
    }
    // gp_Vec::IsOpposite
    pub fn is_opposite(&self, other: &Vec, angular_tolerance: f64) -> Result<bool, ConstructionError> {
        let ang = PI - self.angle(other)?;
        Ok(ang <= angular_tolerance)
    }
    // gp_Vec::IsParallel
    pub fn is_parallel(&self, other: &Vec, angular_tolerance: f64) -> Result<bool, ConstructionError> {
        let ang = self.angle(other)?;
        Ok(ang <= angular_tolerance || PI - ang <= angular_tolerance)
    }
    // gp_Vec::Angle
    pub fn angle(&self, other: &Vec) -> Result<f64, ConstructionError> {
        if self.0.modulus() <= resolution() || other.0.modulus() <= resolution() {
            return Err(ConstructionError);
        }
        let a = Dir::from_xyz(self.0)?;
        let b = Dir::from_vec(other)?;
        Ok(a.angle(&b))
    }
    // gp_Vec::AngleWithRef
    pub fn angle_with_ref(&self, other: &Vec, vref: &Vec) -> Result<f64, ConstructionError> {
        if self.0.modulus() <= resolution() || vref.0.modulus() <= resolution() || other.0.modulus() <= resolution() {
            return Err(ConstructionError);
        }
        let a = Dir::from_xyz(self.0)?;
        let b = Dir::from_vec(other)?;
        let r = Dir::from_vec(vref)?;
        Ok(a.angle_with_ref(&b, &r))
    }
    // gp_Vec::Magnitude
    pub fn magnitude(&self) -> f64 {
        self.0.modulus()
    }
    // gp_Vec::SquareMagnitude
    pub fn square_magnitude(&self) -> f64 {
        self.0.square_modulus()
    }
    // gp_Vec::Add
    pub fn add(&mut self, other: &Vec) {
        self.0.add(&other.0);
    }
    // gp_Vec::Added
    pub fn added(&self, other: &Vec) -> Vec {
        let mut v = *self;
        v.0.add(&other.0);
        v
    }
    // gp_Vec::Subtract
    pub fn subtract(&mut self, other: &Vec) {
        self.0.subtract(&other.0);
    }
    // gp_Vec::Subtracted
    pub fn subtracted(&self, other: &Vec) -> Vec {
        let mut v = *self;
        v.0.subtract(&other.0);
        v
    }
    // gp_Vec::Multiply
    pub fn multiply(&mut self, scalar: f64) {
        self.0.multiply(scalar);
    }
    // gp_Vec::Multiplied
    pub fn multiplied(&self, scalar: f64) -> Vec {
        let mut v = *self;
        v.0.multiply(scalar);
        v
    }
    // gp_Vec::Divide
    pub fn divide(&mut self, scalar: f64) {
        self.0.divide(scalar);
    }
    // gp_Vec::Divided
    pub fn divided(&self, scalar: f64) -> Vec {
        let mut v = *self;
        v.0.divide(scalar);
        v
    }
    // gp_Vec::Cross
    pub fn cross(&mut self, right: &Vec) {
        self.0.cross(&right.0);
    }
    // gp_Vec::Crossed
    pub fn crossed(&self, right: &Vec) -> Vec {
        let mut v = *self;
        v.0.cross(&right.0);
        v
    }
    // gp_Vec::CrossMagnitude
    pub fn cross_magnitude(&self, right: &Vec) -> f64 {
        self.0.cross_magnitude(&right.0)
    }
    // gp_Vec::CrossSquareMagnitude
    pub fn cross_square_magnitude(&self, right: &Vec) -> f64 {
        self.0.cross_square_magnitude(&right.0)
    }
    // gp_Vec::CrossCross
    pub fn cross_cross(&mut self, v1: &Vec, v2: &Vec) {
        self.0.cross_cross(&v1.0, &v2.0);
    }
    // gp_Vec::CrossCrossed
    pub fn cross_crossed(&self, v1: &Vec, v2: &Vec) -> Vec {
        let mut v = *self;
        v.0.cross_cross(&v1.0, &v2.0);
        v
    }
    // gp_Vec::Dot
    pub fn dot(&self, other: &Vec) -> f64 {
        self.0.dot(&other.0)
    }
    // gp_Vec::DotCross
    pub fn dot_cross(&self, v1: &Vec, v2: &Vec) -> f64 {
        self.0.dot_cross(&v1.0, &v2.0)
    }
    // gp_Vec::Normalize
    pub fn normalize(&mut self) -> Result<(), ConstructionError> {
        let d = self.0.modulus();
        if d <= resolution() {
            return Err(ConstructionError);
        }
        self.0.divide(d);
        Ok(())
    }
    // gp_Vec::Normalized
    pub fn normalized(&self) -> Result<Vec, ConstructionError> {
        let d = self.0.modulus();
        if d <= resolution() {
            return Err(ConstructionError);
        }
        let mut v = *self;
        v.0.divide(d);
        Ok(v)
    }
    // gp_Vec::Reverse
    pub fn reverse(&mut self) {
        self.0.reverse();
    }
    // gp_Vec::Reversed
    pub fn reversed(&self) -> Vec {
        let mut v = *self;
        v.0.reverse();
        v
    }
    // gp_Vec::SetLinearForm(A1, V1, A2, V2, A3, V3, V4)
    #[allow(clippy::too_many_arguments)]
    pub fn set_linear_form_3_plus(&mut self, a1: f64, v1: &Vec, a2: f64, v2: &Vec, a3: f64, v3: &Vec, v4: &Vec) {
        self.0.set_linear_form_3_plus(a1, &v1.0, a2, &v2.0, a3, &v3.0, &v4.0);
    }
    // gp_Vec::SetLinearForm(A1, V1, A2, V2, A3, V3)
    pub fn set_linear_form_3(&mut self, a1: f64, v1: &Vec, a2: f64, v2: &Vec, a3: f64, v3: &Vec) {
        self.0.set_linear_form_3(a1, &v1.0, a2, &v2.0, a3, &v3.0);
    }
    // gp_Vec::SetLinearForm(A1, V1, A2, V2, V3)
    pub fn set_linear_form_2_plus(&mut self, a1: f64, v1: &Vec, a2: f64, v2: &Vec, v3: &Vec) {
        self.0.set_linear_form_2_plus(a1, &v1.0, a2, &v2.0, &v3.0);
    }
    // gp_Vec::SetLinearForm(A1, V1, A2, V2)
    pub fn set_linear_form_2(&mut self, a1: f64, v1: &Vec, a2: f64, v2: &Vec) {
        self.0.set_linear_form_2(a1, &v1.0, a2, &v2.0);
    }
    // gp_Vec::SetLinearForm(A1, V1, V2)
    pub fn set_linear_form_1_plus(&mut self, a1: f64, v1: &Vec, v2: &Vec) {
        self.0.set_linear_form_1_plus(a1, &v1.0, &v2.0);
    }
    // gp_Vec::SetLinearForm(V1, V2)
    pub fn set_linear_form_sum(&mut self, v1: &Vec, v2: &Vec) {
        self.0.set_linear_form_sum(&v1.0, &v2.0);
    }
    // gp_Vec::Mirror
    pub fn mirror<M: VecMirror>(&mut self, m: &M) {
        m.mirror_vec(self);
    }
    // gp_Vec::Mirrored
    pub fn mirrored<M: VecMirror>(&self, m: &M) -> Vec {
        let mut v = *self;
        m.mirror_vec(&mut v);
        v
    }
    // gp_Vec::Rotate
    pub fn rotate(&mut self, a1: &Ax1, ang: f64) {
        let mut t = Trsf::new();
        // The axis direction is a unit vector: SetRotation cannot fail.
        if t.set_rotation(a1, ang).is_ok() {
            self.0.multiply_mat(&t.vectorial_part());
        }
    }
    // gp_Vec::Rotated
    pub fn rotated(&self, a1: &Ax1, ang: f64) -> Vec {
        let mut v = *self;
        v.rotate(a1, ang);
        v
    }
    // gp_Vec::Scale
    pub fn scale(&mut self, s: f64) {
        self.0.multiply(s);
    }
    // gp_Vec::Scaled
    pub fn scaled(&self, s: f64) -> Vec {
        let mut v = *self;
        v.0.multiply(s);
        v
    }
    // gp_Vec::Transform (never fails; `Result` is the documented signature)
    pub fn transform(&mut self, t: &Trsf) -> Result<(), ConstructionError> {
        match t.form() {
            TrsfForm::Identity | TrsfForm::Translation => {}
            TrsfForm::PntMirror => self.0.reverse(),
            TrsfForm::Scale => self.0.multiply(t.scale_factor()),
            _ => self.0.multiply_mat(&t.vectorial_part()),
        }
        Ok(())
    }
    // gp_Vec::Transformed
    pub fn transformed(&self, t: &Trsf) -> Vec {
        let mut v = *self;
        // `transform` never returns an error.
        let _ = v.transform(t);
        v
    }
}
