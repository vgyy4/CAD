//! gp_Quaternion, gp_QuaternionNLerp, gp_QuaternionSLerp and gp_EulerSequence
use super::{ConstructionError, Mat, Vec, resolution};

/// gp_EulerSequence (same discriminants as the C++ enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum EulerSequence {
    EulerAngles,
    YawPitchRoll,
    ExtrinsicXYZ,
    ExtrinsicXZY,
    ExtrinsicYZX,
    ExtrinsicYXZ,
    ExtrinsicZXY,
    ExtrinsicZYX,
    IntrinsicXYZ,
    IntrinsicXZY,
    IntrinsicYZX,
    IntrinsicYXZ,
    IntrinsicZXY,
    IntrinsicZYX,
    ExtrinsicXYX,
    ExtrinsicXZX,
    ExtrinsicYZY,
    ExtrinsicYXY,
    ExtrinsicZYZ,
    ExtrinsicZXZ,
    IntrinsicXYX,
    IntrinsicXZX,
    IntrinsicYZY,
    IntrinsicYXY,
    IntrinsicZXZ,
    IntrinsicZYZ,
}

/// gp_Quaternion
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternion {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

impl Default for Quaternion {
    fn default() -> Self {
        Self::new()
    }
}

/// gp_EulerSequence_Parameters
struct EulerParams {
    i: i32,
    j: i32,
    k: i32,
    is_odd: bool,
    is_two_axes: bool,
    is_extrinsic: bool,
}

impl EulerParams {
    fn new(ax1: i32, is_odd: bool, is_two_axes: bool, is_extrinsic: bool) -> Self {
        Self {
            i: ax1,
            j: 1 + (ax1 + i32::from(is_odd)) % 3,
            k: 1 + (ax1 + i32::from(!is_odd)) % 3,
            is_odd,
            is_two_axes,
            is_extrinsic,
        }
    }
}

// translateEulerSequence
fn translate_euler_sequence(seq: EulerSequence) -> EulerParams {
    use EulerSequence::*;
    const F: bool = false;
    const T: bool = true;
    let p = EulerParams::new;
    match seq {
        ExtrinsicXYZ => p(1, F, F, T),
        ExtrinsicXZY => p(1, T, F, T),
        ExtrinsicYZX => p(2, F, F, T),
        ExtrinsicYXZ => p(2, T, F, T),
        ExtrinsicZXY => p(3, F, F, T),
        ExtrinsicZYX => p(3, T, F, T),
        IntrinsicXYZ => p(3, T, F, F),
        IntrinsicXZY => p(2, F, F, F),
        IntrinsicYZX => p(1, T, F, F),
        IntrinsicYXZ => p(3, F, F, F),
        IntrinsicZXY => p(2, T, F, F),
        IntrinsicZYX => p(1, F, F, F),
        ExtrinsicXYX => p(1, F, T, T),
        ExtrinsicXZX => p(1, T, T, T),
        ExtrinsicYZY => p(2, F, T, T),
        ExtrinsicYXY => p(2, T, T, T),
        ExtrinsicZXZ => p(3, F, T, T),
        ExtrinsicZYZ => p(3, T, T, T),
        IntrinsicXYX => p(1, F, T, F),
        IntrinsicXZX => p(1, T, T, F),
        IntrinsicYZY => p(2, F, T, F),
        IntrinsicYXY => p(2, T, T, F),
        IntrinsicZXZ => p(3, F, T, F),
        IntrinsicZYZ => p(3, T, T, F),
        EulerAngles => p(3, F, T, F),
        YawPitchRoll => p(1, F, F, F),
    }
}

fn put(values: &mut [f64; 4], index: i32, value: f64) {
    if let Some(e) = usize::try_from(index).ok().and_then(|i| values.get_mut(i)) {
        *e = value;
    }
}

fn get(values: &[f64; 4], index: i32) -> f64 {
    usize::try_from(index).ok().and_then(|i| values.get(i)).copied().unwrap_or(0.0)
}

impl Quaternion {
    // gp_Quaternion::gp_Quaternion()
    pub fn new() -> Self {
        Self { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
    }
    // gp_Quaternion::gp_Quaternion(x, y, z, w)
    pub fn from_components(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }
    // gp_Quaternion::gp_Quaternion(VecFrom, VecTo)
    pub fn from_vectors(from: &Vec, to: &Vec) -> Result<Self, ConstructionError> {
        let mut q = Self::new();
        q.set_rotation(from, to)?;
        Ok(q)
    }
    // gp_Quaternion::gp_Quaternion(VecFrom, VecTo, HelpCrossVec)
    pub fn from_vectors_with_help(from: &Vec, to: &Vec, help: &Vec) -> Result<Self, ConstructionError> {
        let mut q = Self::new();
        q.set_rotation_with_help(from, to, help)?;
        Ok(q)
    }
    // gp_Quaternion::gp_Quaternion(Axis, Angle)
    pub fn from_axis_angle(axis: &Vec, angle: f64) -> Result<Self, ConstructionError> {
        let mut q = Self::new();
        q.set_vector_and_angle(axis, angle)?;
        Ok(q)
    }
    // gp_Quaternion::gp_Quaternion(gp_Mat)
    pub fn from_matrix(m: &Mat) -> Self {
        let mut q = Self::new();
        q.set_matrix(m);
        q
    }
    // gp_Quaternion::IsEqual
    pub fn is_equal(&self, other: &Quaternion) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        (self.x - other.x).abs() <= resolution()
            && (self.y - other.y).abs() <= resolution()
            && (self.z - other.z).abs() <= resolution()
            && (self.w - other.w).abs() <= resolution()
    }
    // gp_Quaternion::SetRotation(VecFrom, VecTo)
    pub fn set_rotation(&mut self, from: &Vec, to: &Vec) -> Result<(), ConstructionError> {
        let cross = from.crossed(to);
        self.set_components(cross.x(), cross.y(), cross.z(), from.dot(to));
        self.normalize();
        self.w += 1.0;
        if self.w <= resolution() {
            if (from.z() * from.z()) > (from.x() * from.x()) {
                self.set_components(0.0, from.z(), -from.y(), self.w);
            } else {
                self.set_components(from.y(), -from.x(), 0.0, self.w);
            }
        }
        self.normalize();
        Ok(())
    }
    // gp_Quaternion::SetRotation(VecFrom, VecTo, HelpCrossVec)
    pub fn set_rotation_with_help(&mut self, from: &Vec, to: &Vec, help: &Vec) -> Result<(), ConstructionError> {
        let cross = from.crossed(to);
        self.set_components(cross.x(), cross.y(), cross.z(), from.dot(to));
        self.normalize();
        self.w += 1.0;
        if self.w <= resolution() {
            let axis = from.crossed(help);
            self.set_components(axis.x(), axis.y(), axis.z(), self.w);
        }
        self.normalize();
        Ok(())
    }
    // gp_Quaternion::SetVectorAndAngle
    pub fn set_vector_and_angle(&mut self, axis: &Vec, angle: f64) -> Result<(), ConstructionError> {
        let axis = axis.normalized()?;
        let angle_half = 0.5 * angle;
        let sin_a = libm::sin(angle_half);
        self.set_components(axis.x() * sin_a, axis.y() * sin_a, axis.z() * sin_a, libm::cos(angle_half));
        Ok(())
    }
    // gp_Quaternion::GetVectorAndAngle
    pub fn vector_and_angle(&self) -> (Vec, f64) {
        let vl = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if vl > resolution() {
            let ivl = 1.0 / vl;
            let axis = Vec::new(self.x * ivl, self.y * ivl, self.z * ivl);
            let angle = if self.w < 0.0 { 2.0 * libm::atan2(-vl, -self.w) } else { 2.0 * libm::atan2(vl, self.w) };
            (axis, angle)
        } else {
            (Vec::new(0.0, 0.0, 1.0), 0.0)
        }
    }
    // gp_Quaternion::SetMatrix
    pub fn set_matrix(&mut self, m: &Mat) {
        let e = |r, c| m.at(r, c);
        let tr = e(1, 1) + e(2, 2) + e(3, 3);
        if tr > 0.0 {
            self.set_components(e(3, 2) - e(2, 3), e(1, 3) - e(3, 1), e(2, 1) - e(1, 2), tr + 1.0);
            self.scale(0.5 / self.w.sqrt());
        } else if (e(1, 1) > e(2, 2)) && (e(1, 1) > e(3, 3)) {
            self.set_components(
                1.0 + e(1, 1) - e(2, 2) - e(3, 3),
                e(1, 2) + e(2, 1),
                e(1, 3) + e(3, 1),
                e(3, 2) - e(2, 3),
            );
            self.scale(0.5 / self.x.sqrt());
        } else if e(2, 2) > e(3, 3) {
            self.set_components(
                e(1, 2) + e(2, 1),
                1.0 + e(2, 2) - e(1, 1) - e(3, 3),
                e(2, 3) + e(3, 2),
                e(1, 3) - e(3, 1),
            );
            self.scale(0.5 / self.y.sqrt());
        } else {
            self.set_components(
                e(1, 3) + e(3, 1),
                e(2, 3) + e(3, 2),
                1.0 + e(3, 3) - e(1, 1) - e(2, 2),
                e(2, 1) - e(1, 2),
            );
            self.scale(0.5 / self.z.sqrt());
        }
    }
    // gp_Quaternion::GetMatrix
    pub fn get_matrix(&self) -> Mat {
        let s = 2.0 / self.square_norm();
        let x2 = self.x * s;
        let y2 = self.y * s;
        let z2 = self.z * s;
        let xx = self.x * x2;
        let xy = self.x * y2;
        let xz = self.x * z2;
        let yy = self.y * y2;
        let yz = self.y * z2;
        let zz = self.z * z2;
        let wx = self.w * x2;
        let wy = self.w * y2;
        let wz = self.w * z2;
        Mat::from_values([
            1.0 - (yy + zz),
            xy - wz,
            xz + wy,
            xy + wz,
            1.0 - (xx + zz),
            yz - wx,
            xz - wy,
            yz + wx,
            1.0 - (xx + yy),
        ])
    }
    // gp_Quaternion::SetEulerAngles
    pub fn set_euler_angles(&mut self, order: EulerSequence, alpha: f64, beta: f64, gamma: f64) {
        let o = translate_euler_sequence(order);
        let (mut a, mut b, mut c) = (alpha, beta, gamma);
        if !o.is_extrinsic {
            a = gamma;
            c = alpha;
        }
        if o.is_odd {
            b = -b;
        }
        let ti = 0.5 * a;
        let tj = 0.5 * b;
        let th = 0.5 * c;
        let ci = libm::cos(ti);
        let cj = libm::cos(tj);
        let ch = libm::cos(th);
        let si = libm::sin(ti);
        let sj = libm::sin(tj);
        let sh = libm::sin(th);
        let cc = ci * ch;
        let cs = ci * sh;
        let sc = si * ch;
        let ss = si * sh;
        let mut values = [0.0_f64; 4];
        if o.is_two_axes {
            put(&mut values, o.i, cj * (cs + sc));
            put(&mut values, o.j, sj * (cc + ss));
            put(&mut values, o.k, sj * (cs - sc));
            put(&mut values, 0, cj * (cc - ss));
        } else {
            put(&mut values, o.i, cj * sc - sj * cs);
            put(&mut values, o.j, cj * ss + sj * cc);
            put(&mut values, o.k, cj * cs - sj * sc);
            put(&mut values, 0, cj * cc + sj * ss);
        }
        if o.is_odd {
            let vj = get(&values, o.j);
            put(&mut values, o.j, -vj);
        }
        self.x = get(&values, 1);
        self.y = get(&values, 2);
        self.z = get(&values, 3);
        self.w = get(&values, 0);
    }
    // gp_Quaternion::GetEulerAngles
    pub fn get_euler_angles(&self, order: EulerSequence) -> (f64, f64, f64) {
        let m = self.get_matrix();
        let o = translate_euler_sequence(order);
        let e = |r, c| m.at(r, c);
        let (mut alpha, mut beta, mut gamma);
        if o.is_two_axes {
            let sy = (e(o.i, o.j) * e(o.i, o.j) + e(o.i, o.k) * e(o.i, o.k)).sqrt();
            if sy > 16.0 * f64::EPSILON {
                alpha = libm::atan2(e(o.i, o.j), e(o.i, o.k));
                gamma = libm::atan2(e(o.j, o.i), -e(o.k, o.i));
            } else {
                alpha = libm::atan2(-e(o.j, o.k), e(o.j, o.j));
                gamma = 0.;
            }
            beta = libm::atan2(sy, e(o.i, o.i));
        } else {
            let cy = (e(o.i, o.i) * e(o.i, o.i) + e(o.j, o.i) * e(o.j, o.i)).sqrt();
            if cy > 16.0 * f64::EPSILON {
                alpha = libm::atan2(e(o.k, o.j), e(o.k, o.k));
                gamma = libm::atan2(e(o.j, o.i), e(o.i, o.i));
            } else {
                alpha = libm::atan2(-e(o.j, o.k), e(o.j, o.j));
                gamma = 0.;
            }
            beta = libm::atan2(-e(o.k, o.i), cy);
        }
        if o.is_odd {
            alpha = -alpha;
            beta = -beta;
            gamma = -gamma;
        }
        if !o.is_extrinsic {
            std::mem::swap(&mut alpha, &mut gamma);
        }
        (alpha, beta, gamma)
    }
    // gp_Quaternion::Set(x, y, z, w)
    pub fn set_components(&mut self, x: f64, y: f64, z: f64, w: f64) {
        self.x = x;
        self.y = y;
        self.z = z;
        self.w = w;
    }
    // gp_Quaternion::Set(gp_Quaternion)
    pub fn set_quaternion(&mut self, q: &Quaternion) {
        *self = *q;
    }
    // gp_Quaternion::X
    pub fn x(&self) -> f64 {
        self.x
    }
    // gp_Quaternion::Y
    pub fn y(&self) -> f64 {
        self.y
    }
    // gp_Quaternion::Z
    pub fn z(&self) -> f64 {
        self.z
    }
    // gp_Quaternion::W
    pub fn w(&self) -> f64 {
        self.w
    }
    // gp_Quaternion::SetIdent
    pub fn set_ident(&mut self) {
        *self = Self::new();
    }
    // gp_Quaternion::Reverse
    pub fn reverse(&mut self) {
        self.x = -self.x;
        self.y = -self.y;
        self.z = -self.z;
    }
    // gp_Quaternion::Reversed
    pub fn reversed(&self) -> Quaternion {
        Quaternion::from_components(-self.x, -self.y, -self.z, self.w)
    }
    // gp_Quaternion::Invert
    pub fn invert(&mut self) {
        *self = self.inverted();
    }
    // gp_Quaternion::Inverted
    pub fn inverted(&self) -> Quaternion {
        let an_in = 1.0 / self.square_norm();
        Quaternion::from_components(-self.x * an_in, -self.y * an_in, -self.z * an_in, self.w * an_in)
    }
    // gp_Quaternion::SquareNorm
    pub fn square_norm(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }
    // gp_Quaternion::Norm
    pub fn norm(&self) -> f64 {
        self.square_norm().sqrt()
    }
    // gp_Quaternion::Scale
    pub fn scale(&mut self, s: f64) {
        self.x *= s;
        self.y *= s;
        self.z *= s;
        self.w *= s;
    }
    // gp_Quaternion::Scaled
    pub fn scaled(&self, s: f64) -> Quaternion {
        Quaternion::from_components(self.x * s, self.y * s, self.z * s, self.w * s)
    }
    // gp_Quaternion::StabilizeLength
    pub fn stabilize_length(&mut self) {
        let cs = self.x.abs() + self.y.abs() + self.z.abs() + self.w.abs();
        if cs > 0.0 {
            self.x /= cs;
            self.y /= cs;
            self.z /= cs;
            self.w /= cs;
        } else {
            self.set_ident();
        }
    }
    // gp_Quaternion::Normalize
    pub fn normalize(&mut self) {
        let mut magn = self.norm();
        if magn < resolution() {
            self.stabilize_length();
            magn = self.norm();
        }
        self.scale(1.0 / magn);
    }
    // gp_Quaternion::Normalized
    pub fn normalized(&self) -> Quaternion {
        let mut q = *self;
        q.normalize();
        q
    }
    // gp_Quaternion::Negated
    pub fn negated(&self) -> Quaternion {
        Quaternion::from_components(-self.x, -self.y, -self.z, -self.w)
    }
    // gp_Quaternion::Added
    pub fn added(&self, o: &Quaternion) -> Quaternion {
        Quaternion::from_components(self.x + o.x, self.y + o.y, self.z + o.z, self.w + o.w)
    }
    // gp_Quaternion::Subtracted
    pub fn subtracted(&self, o: &Quaternion) -> Quaternion {
        Quaternion::from_components(self.x - o.x, self.y - o.y, self.z - o.z, self.w - o.w)
    }
    // gp_Quaternion::Multiplied
    pub fn multiplied_quaternion(&self, q: &Quaternion) -> Quaternion {
        Quaternion::from_components(
            self.w * q.x + self.x * q.w + self.y * q.z - self.z * q.y,
            self.w * q.y + self.y * q.w + self.z * q.x - self.x * q.z,
            self.w * q.z + self.z * q.w + self.x * q.y - self.y * q.x,
            self.w * q.w - self.x * q.x - self.y * q.y - self.z * q.z,
        )
    }
    // gp_Quaternion::Add
    pub fn add(&mut self, q: &Quaternion) {
        self.x += q.x;
        self.y += q.y;
        self.z += q.z;
        self.w += q.w;
    }
    // gp_Quaternion::Subtract
    pub fn subtract(&mut self, q: &Quaternion) {
        self.x -= q.x;
        self.y -= q.y;
        self.z -= q.z;
        self.w -= q.w;
    }
    // gp_Quaternion::Multiply(gp_Quaternion)
    pub fn multiply_quaternion(&mut self, q: &Quaternion) {
        *self = self.multiplied_quaternion(q);
    }
    // gp_Quaternion::Dot
    pub fn dot(&self, o: &Quaternion) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }
    // gp_Quaternion::GetRotationAngle
    pub fn rotation_angle(&self) -> f64 {
        let l = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if self.w < 0.0 { 2.0 * libm::atan2(-l, -self.w) } else { 2.0 * libm::atan2(l, self.w) }
    }
    // gp_Quaternion::Multiply(gp_Vec)
    pub fn rotate(&self, v: &Vec) -> Vec {
        let q = Quaternion::from_components(
            v.x() * self.w + v.z() * self.y - v.y() * self.z,
            v.y() * self.w + v.x() * self.z - v.z() * self.x,
            v.z() * self.w + v.y() * self.x - v.x() * self.y,
            v.x() * self.x + v.y() * self.y + v.z() * self.z,
        );
        Vec::new(
            self.w * q.x + self.x * q.w + self.y * q.z - self.z * q.y,
            self.w * q.y + self.y * q.w + self.z * q.x - self.x * q.z,
            self.w * q.z + self.z * q.w + self.x * q.y - self.y * q.x,
        )
        .multiplied(1.0 / self.square_norm())
    }
}

/// gp_QuaternionNLerp: normalized linear interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct QuaternionNLerp {
    q_start: Quaternion,
    q_end: Quaternion,
}

impl QuaternionNLerp {
    // gp_QuaternionNLerp::Interpolate(start, end, t)
    pub fn interpolate_between(q_start: &Quaternion, q_end: &Quaternion, t: f64) -> Quaternion {
        Self::between(q_start, q_end).interpolate(t)
    }
    // gp_QuaternionNLerp::gp_QuaternionNLerp()
    pub fn new() -> Self {
        Self::default()
    }
    // gp_QuaternionNLerp::gp_QuaternionNLerp(start, end)
    pub fn between(q_start: &Quaternion, q_end: &Quaternion) -> Self {
        let mut l = Self::new();
        l.init(q_start, q_end);
        l
    }
    // gp_QuaternionNLerp::Init
    pub fn init(&mut self, q_start: &Quaternion, q_end: &Quaternion) {
        self.init_from_unit(&q_start.normalized(), &q_end.normalized());
    }
    // gp_QuaternionNLerp::InitFromUnit
    pub fn init_from_unit(&mut self, q_start: &Quaternion, q_end: &Quaternion) {
        self.q_start = *q_start;
        self.q_end = *q_end;
        let inner = self.q_start.dot(&self.q_end);
        if inner < 0.0 {
            self.q_end = self.q_end.negated();
        }
        let s = self.q_start;
        self.q_end.subtract(&s);
    }
    // gp_QuaternionNLerp::Interpolate(t, result)
    pub fn interpolate(&self, t: f64) -> Quaternion {
        self.q_start.added(&self.q_end.scaled(t))
    }
}

/// gp_QuaternionSLerp: spherical linear interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct QuaternionSLerp {
    q_start: Quaternion,
    q_end: Quaternion,
    omega: f64,
}

impl QuaternionSLerp {
    // gp_QuaternionSLerp::Interpolate(start, end, t)
    pub fn interpolate_between(q_start: &Quaternion, q_end: &Quaternion, t: f64) -> Quaternion {
        Self::between(q_start, q_end).interpolate(t)
    }
    // gp_QuaternionSLerp::gp_QuaternionSLerp()
    pub fn new() -> Self {
        Self::default()
    }
    // gp_QuaternionSLerp::gp_QuaternionSLerp(start, end)
    pub fn between(q_start: &Quaternion, q_end: &Quaternion) -> Self {
        let mut l = Self::new();
        l.init(q_start, q_end);
        l
    }
    // gp_QuaternionSLerp::Init
    pub fn init(&mut self, q_start: &Quaternion, q_end: &Quaternion) {
        self.init_from_unit(&q_start.normalized(), &q_end.normalized());
    }
    // gp_QuaternionSLerp::InitFromUnit
    pub fn init_from_unit(&mut self, q_start: &Quaternion, q_end: &Quaternion) {
        self.q_start = *q_start;
        self.q_end = *q_end;
        let mut cos_omega = self.q_start.dot(&self.q_end);
        if cos_omega < 0.0 {
            cos_omega = -cos_omega;
            self.q_end = self.q_end.negated();
        }
        if cos_omega > 0.9999 {
            cos_omega = 0.9999;
        }
        self.omega = libm::acos(cos_omega);
        let inv_sin_omega = 1.0 / libm::sin(self.omega);
        self.q_start.scale(inv_sin_omega);
        self.q_end.scale(inv_sin_omega);
    }
    // gp_QuaternionSLerp::Interpolate(t, result)
    pub fn interpolate(&self, t: f64) -> Quaternion {
        self.q_start.scaled(libm::sin((1.0 - t) * self.omega)).added(&self.q_end.scaled(libm::sin(t * self.omega)))
    }
}
