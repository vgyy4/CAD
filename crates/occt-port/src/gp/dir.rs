//! gp_Dir
// 0.70710678118655 is OCCT's literal, not an approximation of FRAC_1_SQRT_2 to replace.
#![allow(clippy::approx_constant)]
use super::{Ax1, Ax2, ConstructionError, Direction, Trsf, TrsfForm, Vec, XYZ, resolution};
use std::f64::consts::PI;

/// gp_Dir: a unit vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dir(pub(crate) XYZ);

impl Default for Dir {
    // gp_Dir::gp_Dir()
    fn default() -> Self {
        Self(XYZ::new(1., 0., 0.))
    }
}

/// Things a direction can be mirrored about: a direction, an axis or a plane (an `Ax2`).
pub trait DirMirror {
    /// Mirrors `d` about `self` (gp_Dir::Mirror).
    fn mirror_dir(&self, d: &mut Dir);
}

fn mirror_about(d: &mut Dir, a: f64, b: f64, c: f64) {
    let (x, y, z) = d.0.coords();
    let m1 = 2.0 * a * b;
    let m2 = 2.0 * a * c;
    let m3 = 2.0 * b * c;
    let xx = ((2.0 * a * a) - 1.0) * x + m1 * y + m2 * z;
    let yy = m1 * x + ((2.0 * b * b) - 1.0) * y + m3 * z;
    let zz = m2 * x + m3 * y + ((2.0 * c * c) - 1.0) * z;
    d.0.set_coord(xx, yy, zz);
}

impl DirMirror for Dir {
    // gp_Dir::Mirror(gp_Dir)
    fn mirror_dir(&self, d: &mut Dir) {
        let (a, b, c) = self.0.coords();
        mirror_about(d, a, b, c);
    }
}

impl DirMirror for Ax1 {
    // gp_Dir::Mirror(gp_Ax1); OCCT 8.0.1 reads `C = XYZ.Y()` here, kept as is.
    fn mirror_dir(&self, d: &mut Dir) {
        let xyz = self.direction().0;
        mirror_about(d, xyz.x(), xyz.y(), xyz.y());
    }
}

impl DirMirror for Ax2 {
    // gp_Dir::Mirror(gp_Ax2)
    fn mirror_dir(&self, d: &mut Dir) {
        self.direction().mirror_dir(d);
        d.reverse();
    }
}

/// The checks of `gp_Dir::gp_Dir(x, y, z)`: the stored coordinates.
fn normalized_coords(x: f64, y: f64, z: f64) -> Result<XYZ, ConstructionError> {
    let sq_mod = x * x + y * y + z * z;
    let res = resolution();
    if ((1.0 - res)..=(1.0 + res)).contains(&sq_mod) {
        return Ok(XYZ::new(x, y, z));
    }
    if sq_mod <= res * res {
        return Err(ConstructionError);
    }
    let d = sq_mod.sqrt();
    Ok(XYZ::new(x / d, y / d, z / d))
}

/// The tail of `gp_Dir::Cross`/`CrossCross`: normalizes `coord` which holds the raw result.
fn normalize_result(coord: &mut XYZ) -> Result<(), ConstructionError> {
    let sq_mod = coord.square_modulus();
    let res = resolution();
    if ((1.0 - res)..=(1.0 + res)).contains(&sq_mod) {
        return Ok(());
    }
    if sq_mod <= res * res {
        return Err(ConstructionError);
    }
    coord.divide(sq_mod.sqrt());
    Ok(())
}

impl Dir {
    // gp_Dir::gp_Dir(x, y, z)
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, ConstructionError> {
        Ok(Self(normalized_coords(x, y, z)?))
    }
    // gp_Dir::gp_Dir(gp_XYZ)
    pub fn from_xyz(xyz: XYZ) -> Result<Self, ConstructionError> {
        Self::new(xyz.x(), xyz.y(), xyz.z())
    }
    // gp_Dir::gp_Dir(gp_Vec)
    pub fn from_vec(v: &Vec) -> Result<Self, ConstructionError> {
        Self::from_xyz(*v.xyz())
    }
    // gp_Dir::gp_Dir(gp_Dir::D)
    pub fn from_direction(d: Direction) -> Self {
        let x = match d {
            Direction::X => 1.0,
            Direction::NegativeX => -1.0,
            _ => 0.0,
        };
        let y = match d {
            Direction::Y => 1.0,
            Direction::NegativeY => -1.0,
            _ => 0.0,
        };
        let z = match d {
            Direction::Z => 1.0,
            Direction::NegativeZ => -1.0,
            _ => 0.0,
        };
        Self(XYZ::new(x, y, z))
    }
    pub fn default_x() -> Self {
        Self::from_direction(Direction::X)
    }
    pub fn default_y() -> Self {
        Self::from_direction(Direction::Y)
    }
    pub fn default_z() -> Self {
        Self::from_direction(Direction::Z)
    }
    // gp_Dir::X
    pub fn x(&self) -> f64 {
        self.0.x()
    }
    // gp_Dir::Y
    pub fn y(&self) -> f64 {
        self.0.y()
    }
    // gp_Dir::Z
    pub fn z(&self) -> f64 {
        self.0.z()
    }
    // gp_Dir::Coord(x, y, z)
    pub fn coords(&self) -> (f64, f64, f64) {
        self.0.coords()
    }
    // gp_Dir::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        self.0.coord(index)
    }
    // gp_Dir::XYZ
    pub fn xyz(&self) -> &XYZ {
        &self.0
    }
    // gp_Dir::SetCoord(x, y, z)
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) -> Result<(), ConstructionError> {
        self.0 = normalized_coords(x, y, z)?;
        Ok(())
    }
    // gp_Dir::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        match index {
            1 => self.set_x(value),
            2 => self.set_y(value),
            3 => self.set_z(value),
            _ => Err(ConstructionError),
        }
    }
    // gp_Dir::SetX
    pub fn set_x(&mut self, x: f64) -> Result<(), ConstructionError> {
        self.set_coord(x, self.0.y(), self.0.z())
    }
    // gp_Dir::SetY
    pub fn set_y(&mut self, y: f64) -> Result<(), ConstructionError> {
        self.set_coord(self.0.x(), y, self.0.z())
    }
    // gp_Dir::SetZ
    pub fn set_z(&mut self, z: f64) -> Result<(), ConstructionError> {
        self.set_coord(self.0.x(), self.0.y(), z)
    }
    // gp_Dir::SetXYZ
    pub fn set_xyz(&mut self, xyz: XYZ) -> Result<(), ConstructionError> {
        self.set_coord(xyz.x(), xyz.y(), xyz.z())
    }
    // gp_Dir::IsEqual
    pub fn is_equal(&self, other: &Dir, angular_tolerance: f64) -> bool {
        self.angle(other) <= angular_tolerance
    }
    // gp_Dir::IsNormal
    pub fn is_normal(&self, other: &Dir, angular_tolerance: f64) -> bool {
        let mut ang = PI / 2.0 - self.angle(other);
        if ang < 0.0 {
            ang = -ang;
        }
        ang <= angular_tolerance
    }
    // gp_Dir::IsOpposite
    pub fn is_opposite(&self, other: &Dir, angular_tolerance: f64) -> bool {
        PI - self.angle(other) <= angular_tolerance
    }
    // gp_Dir::IsParallel
    pub fn is_parallel(&self, other: &Dir, angular_tolerance: f64) -> bool {
        let ang = self.angle(other);
        ang <= angular_tolerance || PI - ang <= angular_tolerance
    }
    // gp_Dir::Angle
    pub fn angle(&self, other: &Dir) -> f64 {
        let cosinus = self.0.dot(&other.0);
        if cosinus > -0.70710678118655 && cosinus < 0.70710678118655 {
            libm::acos(cosinus)
        } else {
            let sinus = self.0.crossed(&other.0).modulus();
            if cosinus < 0.0 { PI - libm::asin(sinus) } else { libm::asin(sinus) }
        }
    }
    // gp_Dir::AngleWithRef
    pub fn angle_with_ref(&self, other: &Dir, vref: &Dir) -> f64 {
        let xyz = self.0.crossed(&other.0);
        let cosinus = self.0.dot(&other.0);
        let sinus = xyz.modulus();
        let ang = if cosinus > -0.70710678118655 && cosinus < 0.70710678118655 {
            libm::acos(cosinus)
        } else if cosinus < 0.0 {
            PI - libm::asin(sinus)
        } else {
            libm::asin(sinus)
        };
        if xyz.dot(&vref.0) >= 0.0 { ang } else { -ang }
    }
    // gp_Dir::Cross
    pub fn cross(&mut self, right: &Dir) -> Result<(), ConstructionError> {
        let mut c = self.0;
        c.cross(&right.0);
        normalize_result(&mut c)?;
        self.0 = c;
        Ok(())
    }
    // gp_Dir::Crossed
    pub fn crossed(&self, right: &Dir) -> Result<Dir, ConstructionError> {
        let mut d = *self;
        d.cross(right)?;
        Ok(d)
    }
    // gp_Dir::CrossCross
    pub fn cross_cross(&mut self, v1: &Dir, v2: &Dir) -> Result<(), ConstructionError> {
        let mut c = self.0;
        c.cross_cross(&v1.0, &v2.0);
        normalize_result(&mut c)?;
        self.0 = c;
        Ok(())
    }
    // gp_Dir::CrossCrossed
    pub fn cross_crossed(&self, v1: &Dir, v2: &Dir) -> Result<Dir, ConstructionError> {
        let mut d = *self;
        d.cross_cross(v1, v2)?;
        Ok(d)
    }
    // gp_Dir::Dot
    pub fn dot(&self, other: &Dir) -> f64 {
        self.0.dot(&other.0)
    }
    // gp_Dir::DotCross
    pub fn dot_cross(&self, v1: &Dir, v2: &Dir) -> f64 {
        self.0.dot(&v1.0.crossed(&v2.0))
    }
    // gp_Dir::Reverse
    pub fn reverse(&mut self) {
        self.0.reverse();
    }
    // gp_Dir::Reversed
    pub fn reversed(&self) -> Dir {
        let mut d = *self;
        d.0.reverse();
        d
    }
    // gp_Dir::Mirror
    pub fn mirror<M: DirMirror>(&mut self, m: &M) {
        m.mirror_dir(self);
    }
    // gp_Dir::Mirrored
    pub fn mirrored<M: DirMirror>(&self, m: &M) -> Dir {
        let mut d = *self;
        m.mirror_dir(&mut d);
        d
    }
    // gp_Dir::Rotate
    pub fn rotate(&mut self, a1: &Ax1, ang: f64) {
        let mut t = Trsf::new();
        // The axis direction is a unit vector: SetRotation cannot fail.
        if t.set_rotation(a1, ang).is_ok() {
            self.0.multiply_mat(t.h_vectorial_part());
        }
    }
    // gp_Dir::Rotated
    pub fn rotated(&self, a1: &Ax1, ang: f64) -> Dir {
        let mut d = *self;
        d.rotate(a1, ang);
        d
    }
    /// gp_Dir::Transform exactly as written in OCCT (a zero length divides by zero).
    pub(crate) fn transform_raw(&mut self, t: &Trsf) {
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
    // gp_Dir::Transform; a zero-length image (divide by zero in OCCT) is an error here.
    pub fn transform(&mut self, t: &Trsf) -> Result<(), ConstructionError> {
        let mut d = *self;
        d.transform_raw(t);
        if !(d.0.x().is_finite() && d.0.y().is_finite() && d.0.z().is_finite()) {
            return Err(ConstructionError);
        }
        *self = d;
        Ok(())
    }
    // gp_Dir::Transformed
    pub fn transformed(&self, t: &Trsf) -> Result<Dir, ConstructionError> {
        let mut d = *self;
        d.transform(t)?;
        Ok(d)
    }
}
