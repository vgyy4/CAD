//! gp_XY
use super::{ConstructionError, Mat2d, resolution};

/// gp_XY: two coordinates (stored as an array so `change_coord` can hand out `&mut f64`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct XY {
    d: [f64; 2],
}

impl XY {
    // gp_XY::gp_XY(x, y)
    pub fn new(x: f64, y: f64) -> Self {
        Self { d: [x, y] }
    }
    // gp_XY::gp_XY()
    pub fn zero() -> Self {
        Self::new(0., 0.)
    }
    // gp_XY::X
    pub fn x(&self) -> f64 {
        let [x, _] = self.d;
        x
    }
    // gp_XY::Y
    pub fn y(&self) -> f64 {
        let [_, y] = self.d;
        y
    }
    // gp_XY::Coord(x, y)
    pub fn coords(&self) -> (f64, f64) {
        let [x, y] = self.d;
        (x, y)
    }
    // gp_XY::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        let [x, y] = self.d;
        match index {
            1 => Ok(x),
            2 => Ok(y),
            _ => Err(ConstructionError),
        }
    }
    // gp_XY::ChangeCoord
    pub fn change_coord(&mut self, index: i32) -> Result<&mut f64, ConstructionError> {
        let [x, y] = &mut self.d;
        match index {
            1 => Ok(x),
            2 => Ok(y),
            _ => Err(ConstructionError),
        }
    }
    // gp_XY::SetCoord(x, y)
    pub fn set_coord(&mut self, x: f64, y: f64) {
        self.d = [x, y];
    }
    // gp_XY::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        *self.change_coord(index)? = value;
        Ok(())
    }
    // gp_XY::SetX
    pub fn set_x(&mut self, x: f64) {
        let [a, _] = &mut self.d;
        *a = x;
    }
    // gp_XY::SetY
    pub fn set_y(&mut self, y: f64) {
        let [_, a] = &mut self.d;
        *a = y;
    }
    // gp_XY::Modulus
    pub fn modulus(&self) -> f64 {
        self.square_modulus().sqrt()
    }
    // gp_XY::SquareModulus
    pub fn square_modulus(&self) -> f64 {
        self.x() * self.x() + self.y() * self.y()
    }
    // gp_XY::IsEqual
    pub fn is_equal(&self, other: &XY, tolerance: f64) -> bool {
        (self.x() - other.x()).abs() < tolerance && (self.y() - other.y()).abs() < tolerance
    }
    // gp_XY::Add
    pub fn add(&mut self, other: &XY) {
        self.d = [self.x() + other.x(), self.y() + other.y()];
    }
    // gp_XY::Added
    pub fn added(&self, other: &XY) -> XY {
        XY::new(self.x() + other.x(), self.y() + other.y())
    }
    // gp_XY::Crossed
    pub fn crossed(&self, other: &XY) -> f64 {
        self.x() * other.y() - self.y() * other.x()
    }
    // gp_XY::CrossMagnitude
    pub fn cross_magnitude(&self, right: &XY) -> f64 {
        (self.x() * right.y() - self.y() * right.x()).abs()
    }
    // gp_XY::CrossSquareMagnitude
    pub fn cross_square_magnitude(&self, right: &XY) -> f64 {
        let z = self.x() * right.y() - self.y() * right.x();
        z * z
    }
    // gp_XY::Divide (no zero check in OCCT)
    pub fn divide(&mut self, scalar: f64) {
        self.d = [self.x() / scalar, self.y() / scalar];
    }
    // gp_XY::Divided (no zero check in OCCT)
    pub fn divided(&self, scalar: f64) -> XY {
        XY::new(self.x() / scalar, self.y() / scalar)
    }
    // gp_XY::Dot
    pub fn dot(&self, other: &XY) -> f64 {
        self.x() * other.x() + self.y() * other.y()
    }
    // gp_XY::Multiply(double)
    pub fn multiply(&mut self, scalar: f64) {
        self.d = [self.x() * scalar, self.y() * scalar];
    }
    // gp_XY::Multiply(gp_XY)
    pub fn multiply_xy(&mut self, other: &XY) {
        self.d = [self.x() * other.x(), self.y() * other.y()];
    }
    // gp_XY::Multiply(gp_Mat2d)
    pub fn multiply_mat(&mut self, matrix: &Mat2d) {
        let [[a, b], [c, d]] = matrix.m;
        let (x, y) = self.coords();
        self.d = [a * x + b * y, c * x + d * y];
    }
    // gp_XY::Multiplied(double)
    pub fn multiplied(&self, scalar: f64) -> XY {
        XY::new(self.x() * scalar, self.y() * scalar)
    }
    // gp_XY::Multiplied(gp_XY)
    pub fn multiplied_xy(&self, other: &XY) -> XY {
        XY::new(self.x() * other.x(), self.y() * other.y())
    }
    // gp_XY::Multiplied(gp_Mat2d)
    pub fn multiplied_mat(&self, matrix: &Mat2d) -> XY {
        let [[a, b], [c, d]] = matrix.m;
        let (x, y) = self.coords();
        XY::new(a * x + b * y, c * x + d * y)
    }
    // gp_XY::Normalize
    pub fn normalize(&mut self) -> Result<(), ConstructionError> {
        let d = self.modulus();
        if d <= resolution() {
            return Err(ConstructionError);
        }
        self.d = [self.x() / d, self.y() / d];
        Ok(())
    }
    // gp_XY::Normalized
    pub fn normalized(&self) -> Result<XY, ConstructionError> {
        let d = self.modulus();
        if d <= resolution() {
            return Err(ConstructionError);
        }
        Ok(XY::new(self.x() / d, self.y() / d))
    }
    // gp_XY::Reverse
    pub fn reverse(&mut self) {
        self.d = [-self.x(), -self.y()];
    }
    // gp_XY::Reversed
    pub fn reversed(&self) -> XY {
        XY::new(-self.x(), -self.y())
    }
    // gp_XY::SetLinearForm(A1, XY1, A2, XY2)
    pub fn set_linear_form_2(&mut self, a1: f64, x1: &XY, a2: f64, x2: &XY) {
        self.d = [a1 * x1.x() + a2 * x2.x(), a1 * x1.y() + a2 * x2.y()];
    }
    // gp_XY::SetLinearForm(A1, XY1, A2, XY2, XY3)
    pub fn set_linear_form_2_plus(&mut self, a1: f64, x1: &XY, a2: f64, x2: &XY, x3: &XY) {
        self.d = [a1 * x1.x() + a2 * x2.x() + x3.x(), a1 * x1.y() + a2 * x2.y() + x3.y()];
    }
    // gp_XY::SetLinearForm(A1, XY1, XY2)
    pub fn set_linear_form_1_plus(&mut self, a1: f64, x1: &XY, x2: &XY) {
        self.d = [a1 * x1.x() + x2.x(), a1 * x1.y() + x2.y()];
    }
    // gp_XY::SetLinearForm(XY1, XY2)
    pub fn set_linear_form_sum(&mut self, x1: &XY, x2: &XY) {
        self.d = [x1.x() + x2.x(), x1.y() + x2.y()];
    }
    // gp_XY::Subtract
    pub fn subtract(&mut self, other: &XY) {
        self.d = [self.x() - other.x(), self.y() - other.y()];
    }
    // gp_XY::Subtracted
    pub fn subtracted(&self, other: &XY) -> XY {
        XY::new(self.x() - other.x(), self.y() - other.y())
    }
}
