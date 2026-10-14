//! gp_Mat
use super::{ConstructionError, XYZ, resolution};

/// gp_Mat: a 3x3 matrix, row-major. `Mat::new()` is the zero matrix like the C++ default
/// constructor.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mat {
    pub(crate) m: [[f64; 3]; 3],
}

/// 0-based (row, col) of a checked 1-based index pair.
fn cell_index(row: i32, col: i32) -> Result<(usize, usize), ConstructionError> {
    if !(1..=3).contains(&row) || !(1..=3).contains(&col) {
        return Err(ConstructionError);
    }
    match (usize::try_from(row - 1), usize::try_from(col - 1)) {
        (Ok(r), Ok(c)) => Ok((r, c)),
        _ => Err(ConstructionError),
    }
}

fn line_index(i: i32) -> Result<usize, ConstructionError> {
    if !(1..=3).contains(&i) {
        return Err(ConstructionError);
    }
    usize::try_from(i - 1).map_err(|_| ConstructionError)
}

impl Mat {
    // gp_Mat::gp_Mat()
    pub fn new() -> Self {
        Self { m: [[0.0; 3]; 3] }
    }
    // gp_Mat::gp_Mat(A11 .. A33), row-major
    pub fn from_values(v: [f64; 9]) -> Self {
        let [a, b, c, d, e, f, g, h, i] = v;
        Self { m: [[a, b, c], [d, e, f], [g, h, i]] }
    }
    // gp_Mat::gp_Mat(Col1, Col2, Col3)
    pub fn from_columns(c1: XYZ, c2: XYZ, c3: XYZ) -> Self {
        Self { m: [[c1.x(), c2.x(), c3.x()], [c1.y(), c2.y(), c3.y()], [c1.z(), c2.z(), c3.z()]] }
    }
    /// Unchecked 1-based element, 0.0 outside the matrix (internal helper for the Euler code).
    pub(crate) fn at(&self, row: i32, col: i32) -> f64 {
        match cell_index(row, col) {
            Ok((r, c)) => self.m.get(r).and_then(|l| l.get(c)).copied().unwrap_or(0.0),
            Err(_) => 0.0,
        }
    }
    // gp_Mat::SetCol
    pub fn set_col(&mut self, col: i32, value: &XYZ) -> Result<(), ConstructionError> {
        let c = line_index(col)?;
        for (line, v) in self.m.iter_mut().zip([value.x(), value.y(), value.z()]) {
            if let Some(e) = line.get_mut(c) {
                *e = v;
            }
        }
        Ok(())
    }
    // gp_Mat::SetCols
    pub fn set_cols(&mut self, c1: &XYZ, c2: &XYZ, c3: &XYZ) {
        *self = Mat::from_columns(*c1, *c2, *c3);
    }
    // gp_Mat::SetCross
    pub fn set_cross(&mut self, r: &XYZ) {
        let (x, y, z) = r.coords();
        self.m = [[0.0, -z, y], [z, 0.0, -x], [-y, x, 0.0]];
    }
    // gp_Mat::SetDiagonal
    pub fn set_diagonal(&mut self, x1: f64, x2: f64, x3: f64) {
        let [[a00, _, _], [_, a11, _], [_, _, a22]] = &mut self.m;
        *a00 = x1;
        *a11 = x2;
        *a22 = x3;
    }
    // gp_Mat::SetDot
    pub fn set_dot(&mut self, r: &XYZ) {
        let (x, y, z) = r.coords();
        let xy = x * y;
        let xz = x * z;
        let yz = y * z;
        self.m = [[x * x, xy, xz], [xy, y * y, yz], [xz, yz, z * z]];
    }
    // gp_Mat::SetIdentity
    pub fn set_identity(&mut self) {
        self.m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    // gp_Mat::SetRotation
    pub fn set_rotation(&mut self, axis: &XYZ, ang: f64) -> Result<(), ConstructionError> {
        let v = axis.normalized()?;
        let (a, b, c) = v.coords();
        let cos = libm::cos(ang);
        let sin = libm::sin(ang);
        let om_cos = 1.0 - cos;
        let a2 = a * a;
        let b2 = b * b;
        let c2 = c * c;
        let ab = a * b;
        let ac = a * c;
        let bc = b * c;
        self.m = [
            [1.0 + om_cos * (-(b2 + c2)), om_cos * ab - sin * c, om_cos * ac + sin * b],
            [om_cos * ab + sin * c, 1.0 + om_cos * (-(a2 + c2)), om_cos * bc - sin * a],
            [om_cos * ac - sin * b, om_cos * bc + sin * a, 1.0 + om_cos * (-(a2 + b2))],
        ];
        Ok(())
    }
    // gp_Mat::SetRow
    pub fn set_row(&mut self, row: i32, value: &XYZ) -> Result<(), ConstructionError> {
        let r = line_index(row)?;
        if let Some(line) = self.m.get_mut(r) {
            *line = [value.x(), value.y(), value.z()];
        }
        Ok(())
    }
    // gp_Mat::SetRows
    pub fn set_rows(&mut self, r1: &XYZ, r2: &XYZ, r3: &XYZ) {
        self.m = [r1.get_data(), r2.get_data(), r3.get_data()];
    }
    // gp_Mat::SetScale
    pub fn set_scale(&mut self, s: f64) {
        self.m = [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]];
    }
    // gp_Mat::SetValue
    pub fn set_value(&mut self, row: i32, col: i32, value: f64) -> Result<(), ConstructionError> {
        *self.change_value(row, col)? = value;
        Ok(())
    }
    // gp_Mat::Column
    pub fn column(&self, col: i32) -> Result<XYZ, ConstructionError> {
        let c = line_index(col)?;
        let at = |r: usize| self.m.get(r).and_then(|l| l.get(c)).copied().unwrap_or(0.0);
        Ok(XYZ::new(at(0), at(1), at(2)))
    }
    // gp_Mat::Row
    pub fn row(&self, row: i32) -> Result<XYZ, ConstructionError> {
        let r = line_index(row)?;
        let [x, y, z] = self.m.get(r).copied().unwrap_or([0.0; 3]);
        Ok(XYZ::new(x, y, z))
    }
    // gp_Mat::Diagonal
    pub fn diagonal(&self) -> XYZ {
        let [[a, _, _], [_, e, _], [_, _, i]] = self.m;
        XYZ::new(a, e, i)
    }
    // gp_Mat::Value
    pub fn value(&self, row: i32, col: i32) -> Result<f64, ConstructionError> {
        let (r, c) = cell_index(row, col)?;
        self.m.get(r).and_then(|l| l.get(c)).copied().ok_or(ConstructionError)
    }
    // gp_Mat::ChangeValue
    pub fn change_value(&mut self, row: i32, col: i32) -> Result<&mut f64, ConstructionError> {
        let (r, c) = cell_index(row, col)?;
        self.m.get_mut(r).and_then(|l| l.get_mut(c)).ok_or(ConstructionError)
    }
    // gp_Mat::Determinant
    pub fn determinant(&self) -> f64 {
        let [[a00, a01, a02], [a10, a11, a12], [a20, a21, a22]] = self.m;
        a00 * (a11 * a22 - a21 * a12) - a01 * (a10 * a22 - a20 * a12) + a02 * (a10 * a21 - a20 * a11)
    }
    // gp_Mat::IsSingular
    pub fn is_singular(&self) -> bool {
        let det = self.determinant();
        (if det < 0.0 { -det } else { det }) <= resolution()
    }
    fn zip_with(&mut self, other: &Mat, f: impl Fn(f64, f64) -> f64) {
        for (l, o) in self.m.iter_mut().zip(other.m.iter()) {
            for (e, p) in l.iter_mut().zip(o.iter()) {
                *e = f(*e, *p);
            }
        }
    }
    // gp_Mat::Add
    pub fn add(&mut self, other: &Mat) {
        self.zip_with(other, |a, b| a + b);
    }
    // gp_Mat::Added
    pub fn added(&self, other: &Mat) -> Mat {
        let mut r = *self;
        r.add(other);
        r
    }
    // gp_Mat::Subtract
    pub fn subtract(&mut self, other: &Mat) {
        self.zip_with(other, |a, b| a - b);
    }
    // gp_Mat::Subtracted
    pub fn subtracted(&self, other: &Mat) -> Mat {
        let mut r = *self;
        r.subtract(other);
        r
    }
    // gp_Mat::Divide (the zero-divisor check of OCCT is not performed, the result is inf/NaN)
    pub fn divide(&mut self, scalar: f64) {
        let un_sur_scalar = 1.0 / scalar;
        self.multiply_scalar(un_sur_scalar);
    }
    // gp_Mat::Divided
    pub fn divided(&self, scalar: f64) -> Mat {
        let mut r = *self;
        r.divide(scalar);
        r
    }
    // gp_Mat::Multiply(double)
    pub fn multiply_scalar(&mut self, scalar: f64) {
        for l in &mut self.m {
            for e in l.iter_mut() {
                *e *= scalar;
            }
        }
    }
    // gp_Mat::Multiplied(double)
    pub fn multiplied_scalar(&self, scalar: f64) -> Mat {
        let mut r = *self;
        r.multiply_scalar(scalar);
        r
    }
    // gp_Mat::Multiply(gp_Mat)
    pub fn multiply_mat(&mut self, other: &Mat) {
        let [[a00, a01, a02], [a10, a11, a12], [a20, a21, a22]] = self.m;
        let [[o00, o01, o02], [o10, o11, o12], [o20, o21, o22]] = other.m;
        self.m = [
            [a00 * o00 + a01 * o10 + a02 * o20, a00 * o01 + a01 * o11 + a02 * o21, a00 * o02 + a01 * o12 + a02 * o22],
            [a10 * o00 + a11 * o10 + a12 * o20, a10 * o01 + a11 * o11 + a12 * o21, a10 * o02 + a11 * o12 + a12 * o22],
            [a20 * o00 + a21 * o10 + a22 * o20, a20 * o01 + a21 * o11 + a22 * o21, a20 * o02 + a21 * o12 + a22 * o22],
        ];
    }
    // gp_Mat::Multiplied(gp_Mat)
    pub fn multiplied_mat(&self, other: &Mat) -> Mat {
        let mut r = *self;
        r.multiply_mat(other);
        r
    }
    // gp_Mat::PreMultiply
    pub fn pre_multiply(&mut self, other: &Mat) {
        let [[a00, a01, a02], [a10, a11, a12], [a20, a21, a22]] = self.m;
        let [[o00, o01, o02], [o10, o11, o12], [o20, o21, o22]] = other.m;
        self.m = [
            [o00 * a00 + o01 * a10 + o02 * a20, o00 * a01 + o01 * a11 + o02 * a21, o00 * a02 + o01 * a12 + o02 * a22],
            [o10 * a00 + o11 * a10 + o12 * a20, o10 * a01 + o11 * a11 + o12 * a21, o10 * a02 + o11 * a12 + o12 * a22],
            [o20 * a00 + o21 * a10 + o22 * a20, o20 * a01 + o21 * a11 + o22 * a21, o20 * a02 + o21 * a12 + o22 * a22],
        ];
    }
    // gp_Mat::Invert
    pub fn invert(&mut self) -> Result<(), ConstructionError> {
        let [[a00, a01, a02], [a10, a11, a12], [a20, a21, a22]] = self.m;
        let adj00 = a11 * a22 - a12 * a21;
        let adj10 = a12 * a20 - a10 * a22;
        let adj20 = a10 * a21 - a11 * a20;
        let adj01 = a02 * a21 - a01 * a22;
        let adj11 = a00 * a22 - a02 * a20;
        let adj21 = a01 * a20 - a00 * a21;
        let adj02 = a01 * a12 - a02 * a11;
        let adj12 = a02 * a10 - a00 * a12;
        let adj22 = a00 * a11 - a01 * a10;
        let det = a00 * adj00 + a01 * adj10 + a02 * adj20;
        if det.abs() <= resolution() {
            return Err(ConstructionError);
        }
        let inv_det = 1.0 / det;
        self.m = [
            [adj00 * inv_det, adj01 * inv_det, adj02 * inv_det],
            [adj10 * inv_det, adj11 * inv_det, adj12 * inv_det],
            [adj20 * inv_det, adj21 * inv_det, adj22 * inv_det],
        ];
        Ok(())
    }
    // gp_Mat::Inverted
    pub fn inverted(&self) -> Result<Mat, ConstructionError> {
        let mut r = *self;
        r.invert()?;
        Ok(r)
    }
    // gp_Mat::Power
    pub fn power(&mut self, n: i32) -> Result<(), ConstructionError> {
        if n == 0 {
            self.set_identity();
            return Ok(());
        }
        if n == 1 {
            return Ok(());
        }
        if n == -1 {
            return self.invert();
        }
        let is_negative = n < 0;
        if is_negative {
            self.invert()?;
        }
        let mut power = i64::from(n).abs();
        let mut base = *self;
        self.set_identity();
        while power > 0 {
            if power & 1 == 1 {
                self.multiply_mat(&base);
            }
            let b = base;
            base.multiply_mat(&b);
            power >>= 1;
        }
        Ok(())
    }
    // gp_Mat::Powered
    pub fn powered(&self, n: i32) -> Result<Mat, ConstructionError> {
        let mut r = *self;
        r.power(n)?;
        Ok(r)
    }
    // gp_Mat::Transpose
    pub fn transpose(&mut self) {
        let [[a00, a01, a02], [a10, a11, a12], [a20, a21, a22]] = self.m;
        self.m = [[a00, a10, a20], [a01, a11, a21], [a02, a12, a22]];
    }
    // gp_Mat::Transposed
    pub fn transposed(&self) -> Mat {
        let mut r = *self;
        r.transpose();
        r
    }
}
