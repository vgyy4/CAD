//! gp_XYZ
use super::{ConstructionError, Mat, resolution};

/// gp_XYZ: three coordinates (stored as an array so `change_data` can hand out `&mut [f64; 3]`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct XYZ {
    d: [f64; 3],
}

impl XYZ {
    // gp_XYZ::gp_XYZ
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { d: [x, y, z] }
    }
    // gp_XYZ::gp_XYZ()
    pub fn zero() -> Self {
        Self::new(0., 0., 0.)
    }
    // gp_XYZ::X
    pub fn x(&self) -> f64 {
        let [x, _, _] = self.d;
        x
    }
    // gp_XYZ::Y
    pub fn y(&self) -> f64 {
        let [_, y, _] = self.d;
        y
    }
    // gp_XYZ::Z
    pub fn z(&self) -> f64 {
        let [_, _, z] = self.d;
        z
    }
    // gp_XYZ::Coord(x, y, z)
    pub fn coords(&self) -> (f64, f64, f64) {
        let [x, y, z] = self.d;
        (x, y, z)
    }
    // gp_XYZ::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        let [x, y, z] = self.d;
        match index {
            1 => Ok(x),
            2 => Ok(y),
            3 => Ok(z),
            _ => Err(ConstructionError),
        }
    }
    // gp_XYZ::GetData
    pub fn get_data(&self) -> [f64; 3] {
        self.d
    }
    // gp_XYZ::ChangeData
    pub fn change_data(&mut self) -> &mut [f64; 3] {
        &mut self.d
    }
    // gp_XYZ::ChangeCoord
    pub fn change_coord(&mut self, index: i32) -> Result<&mut f64, ConstructionError> {
        let [x, y, z] = &mut self.d;
        match index {
            1 => Ok(x),
            2 => Ok(y),
            3 => Ok(z),
            _ => Err(ConstructionError),
        }
    }
    // gp_XYZ::SetCoord(x, y, z)
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) {
        self.d = [x, y, z];
    }
    // gp_XYZ::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        *self.change_coord(index)? = value;
        Ok(())
    }
    // gp_XYZ::SetX
    pub fn set_x(&mut self, x: f64) {
        let [a, _, _] = &mut self.d;
        *a = x;
    }
    // gp_XYZ::SetY
    pub fn set_y(&mut self, y: f64) {
        let [_, a, _] = &mut self.d;
        *a = y;
    }
    // gp_XYZ::SetZ
    pub fn set_z(&mut self, z: f64) {
        let [_, _, a] = &mut self.d;
        *a = z;
    }
    // gp_XYZ::Modulus
    pub fn modulus(&self) -> f64 {
        (self.x() * self.x() + self.y() * self.y() + self.z() * self.z()).sqrt()
    }
    // gp_XYZ::SquareModulus
    pub fn square_modulus(&self) -> f64 {
        self.x() * self.x() + self.y() * self.y() + self.z() * self.z()
    }
    // gp_XYZ::IsEqual
    pub fn is_equal(&self, other: &XYZ, tolerance: f64) -> bool {
        (self.x() - other.x()).abs() < tolerance
            && (self.y() - other.y()).abs() < tolerance
            && (self.z() - other.z()).abs() < tolerance
    }
    // gp_XYZ::Add
    pub fn add(&mut self, other: &XYZ) {
        self.d = [self.x() + other.x(), self.y() + other.y(), self.z() + other.z()];
    }
    // gp_XYZ::Added
    pub fn added(&self, other: &XYZ) -> XYZ {
        XYZ::new(self.x() + other.x(), self.y() + other.y(), self.z() + other.z())
    }
    // gp_XYZ::Subtract
    pub fn subtract(&mut self, other: &XYZ) {
        self.d = [self.x() - other.x(), self.y() - other.y(), self.z() - other.z()];
    }
    // gp_XYZ::Subtracted
    pub fn subtracted(&self, other: &XYZ) -> XYZ {
        XYZ::new(self.x() - other.x(), self.y() - other.y(), self.z() - other.z())
    }
    // gp_XYZ::Cross
    pub fn cross(&mut self, right: &XYZ) {
        *self = self.crossed(right);
    }
    // gp_XYZ::Crossed
    pub fn crossed(&self, right: &XYZ) -> XYZ {
        XYZ::new(
            self.y() * right.z() - self.z() * right.y(),
            self.z() * right.x() - self.x() * right.z(),
            self.x() * right.y() - self.y() * right.x(),
        )
    }
    // gp_XYZ::CrossSquareMagnitude
    pub fn cross_square_magnitude(&self, right: &XYZ) -> f64 {
        let x = self.y() * right.z() - self.z() * right.y();
        let y = self.z() * right.x() - self.x() * right.z();
        let z = self.x() * right.y() - self.y() * right.x();
        x * x + y * y + z * z
    }
    // gp_XYZ::CrossMagnitude
    pub fn cross_magnitude(&self, right: &XYZ) -> f64 {
        self.cross_square_magnitude(right).sqrt()
    }
    // gp_XYZ::CrossCross
    pub fn cross_cross(&mut self, c1: &XYZ, c2: &XYZ) {
        let cross_x = c1.y() * c2.z() - c1.z() * c2.y();
        let cross_y = c1.z() * c2.x() - c1.x() * c2.z();
        let cross_z = c1.x() * c2.y() - c1.y() * c2.x();
        let rx = self.y() * cross_z - self.z() * cross_y;
        let ry = self.z() * cross_x - self.x() * cross_z;
        let rz = self.x() * cross_y - self.y() * cross_x;
        self.d = [rx, ry, rz];
    }
    // gp_XYZ::CrossCrossed
    pub fn cross_crossed(&self, c1: &XYZ, c2: &XYZ) -> XYZ {
        let mut r = *self;
        r.cross_cross(c1, c2);
        r
    }
    // gp_XYZ::Divide
    pub fn divide(&mut self, scalar: f64) {
        self.d = [self.x() / scalar, self.y() / scalar, self.z() / scalar];
    }
    // gp_XYZ::Divided
    pub fn divided(&self, scalar: f64) -> XYZ {
        XYZ::new(self.x() / scalar, self.y() / scalar, self.z() / scalar)
    }
    // gp_XYZ::Dot
    pub fn dot(&self, other: &XYZ) -> f64 {
        self.x() * other.x() + self.y() * other.y() + self.z() * other.z()
    }
    // gp_XYZ::DotCross
    pub fn dot_cross(&self, c1: &XYZ, c2: &XYZ) -> f64 {
        let x = c1.y() * c2.z() - c1.z() * c2.y();
        let y = c1.z() * c2.x() - c1.x() * c2.z();
        let z = c1.x() * c2.y() - c1.y() * c2.x();
        self.x() * x + self.y() * y + self.z() * z
    }
    // gp_XYZ::Multiply(double)
    pub fn multiply(&mut self, scalar: f64) {
        self.d = [self.x() * scalar, self.y() * scalar, self.z() * scalar];
    }
    // gp_XYZ::Multiply(gp_XYZ)
    pub fn multiply_xyz(&mut self, other: &XYZ) {
        self.d = [self.x() * other.x(), self.y() * other.y(), self.z() * other.z()];
    }
    // gp_XYZ::Multiply(gp_Mat)
    pub fn multiply_mat(&mut self, matrix: &Mat) {
        *self = self.multiplied_mat(matrix);
    }
    // gp_XYZ::Multiplied(double)
    pub fn multiplied(&self, scalar: f64) -> XYZ {
        XYZ::new(self.x() * scalar, self.y() * scalar, self.z() * scalar)
    }
    // gp_XYZ::Multiplied(gp_XYZ)
    pub fn multiplied_xyz(&self, other: &XYZ) -> XYZ {
        XYZ::new(self.x() * other.x(), self.y() * other.y(), self.z() * other.z())
    }
    // gp_XYZ::Multiplied(gp_Mat)
    pub fn multiplied_mat(&self, matrix: &Mat) -> XYZ {
        let [[a, b, c], [d, e, f], [g, h, i]] = matrix.m;
        let (x, y, z) = self.coords();
        XYZ::new(a * x + b * y + c * z, d * x + e * y + f * z, g * x + h * y + i * z)
    }
    // gp_XYZ::Normalize
    pub fn normalize(&mut self) -> Result<(), ConstructionError> {
        let d = self.modulus();
        if d <= resolution() {
            return Err(ConstructionError);
        }
        self.d = [self.x() / d, self.y() / d, self.z() / d];
        Ok(())
    }
    // gp_XYZ::Normalized
    pub fn normalized(&self) -> Result<XYZ, ConstructionError> {
        let d = self.modulus();
        if d <= resolution() {
            return Err(ConstructionError);
        }
        Ok(XYZ::new(self.x() / d, self.y() / d, self.z() / d))
    }
    // gp_XYZ::Reverse
    pub fn reverse(&mut self) {
        self.d = [-self.x(), -self.y(), -self.z()];
    }
    // gp_XYZ::Reversed
    pub fn reversed(&self) -> XYZ {
        XYZ::new(-self.x(), -self.y(), -self.z())
    }
    // gp_XYZ::SetLinearForm(A1, XYZ1, A2, XYZ2, A3, XYZ3, XYZ4)
    #[allow(clippy::too_many_arguments)]
    pub fn set_linear_form_3_plus(&mut self, a1: f64, x1: &XYZ, a2: f64, x2: &XYZ, a3: f64, x3: &XYZ, x4: &XYZ) {
        self.d = [
            a1 * x1.x() + a2 * x2.x() + a3 * x3.x() + x4.x(),
            a1 * x1.y() + a2 * x2.y() + a3 * x3.y() + x4.y(),
            a1 * x1.z() + a2 * x2.z() + a3 * x3.z() + x4.z(),
        ];
    }
    // gp_XYZ::SetLinearForm(A1, XYZ1, A2, XYZ2, A3, XYZ3)
    pub fn set_linear_form_3(&mut self, a1: f64, x1: &XYZ, a2: f64, x2: &XYZ, a3: f64, x3: &XYZ) {
        self.d = [
            a1 * x1.x() + a2 * x2.x() + a3 * x3.x(),
            a1 * x1.y() + a2 * x2.y() + a3 * x3.y(),
            a1 * x1.z() + a2 * x2.z() + a3 * x3.z(),
        ];
    }
    // gp_XYZ::SetLinearForm(A1, XYZ1, A2, XYZ2, XYZ3)
    pub fn set_linear_form_2_plus(&mut self, a1: f64, x1: &XYZ, a2: f64, x2: &XYZ, x3: &XYZ) {
        self.d = [
            a1 * x1.x() + a2 * x2.x() + x3.x(),
            a1 * x1.y() + a2 * x2.y() + x3.y(),
            a1 * x1.z() + a2 * x2.z() + x3.z(),
        ];
    }
    // gp_XYZ::SetLinearForm(A1, XYZ1, A2, XYZ2)
    pub fn set_linear_form_2(&mut self, a1: f64, x1: &XYZ, a2: f64, x2: &XYZ) {
        self.d = [a1 * x1.x() + a2 * x2.x(), a1 * x1.y() + a2 * x2.y(), a1 * x1.z() + a2 * x2.z()];
    }
    // gp_XYZ::SetLinearForm(A1, XYZ1, XYZ2)
    pub fn set_linear_form_1_plus(&mut self, a1: f64, x1: &XYZ, x2: &XYZ) {
        self.d = [a1 * x1.x() + x2.x(), a1 * x1.y() + x2.y(), a1 * x1.z() + x2.z()];
    }
    // gp_XYZ::SetLinearForm(XYZ1, XYZ2)
    pub fn set_linear_form_sum(&mut self, x1: &XYZ, x2: &XYZ) {
        self.d = [x1.x() + x2.x(), x1.y() + x2.y(), x1.z() + x2.z()];
    }
}
