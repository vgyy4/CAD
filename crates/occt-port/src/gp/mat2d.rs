//! gp_Mat2d
use super::{ConstructionError, XY, resolution};

/// gp_Mat2d: a 2x2 matrix, row-major. `Mat2d::new()` is the zero matrix like the C++ default
/// constructor.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mat2d {
    pub(crate) m: [[f64; 2]; 2],
}

/// 0-based (row, col) of a checked 1-based index pair.
fn cell_index(row: i32, col: i32) -> Result<(usize, usize), ConstructionError> {
    if !(1..=2).contains(&row) || !(1..=2).contains(&col) {
        return Err(ConstructionError);
    }
    match (usize::try_from(row - 1), usize::try_from(col - 1)) {
        (Ok(r), Ok(c)) => Ok((r, c)),
        _ => Err(ConstructionError),
    }
}

impl Mat2d {
    // gp_Mat2d::gp_Mat2d()
    pub fn new() -> Self {
        Self { m: [[0.0; 2]; 2] }
    }
    // gp_Mat2d::gp_Mat2d(Col1, Col2)
    pub fn from_columns(c1: XY, c2: XY) -> Self {
        Self { m: [[c1.x(), c2.x()], [c1.y(), c2.y()]] }
    }
    // gp_Mat2d::SetCol (index outside 1..=2: unchecked in OCCT 8.0.1 as built, reported as an error)
    pub fn set_col(&mut self, col: i32, value: &XY) -> Result<(), ConstructionError> {
        let [[a, b], [c, d]] = &mut self.m;
        match col {
            1 => {
                *a = value.x();
                *c = value.y();
            }
            2 => {
                *b = value.x();
                *d = value.y();
            }
            _ => return Err(ConstructionError),
        }
        Ok(())
    }
    // gp_Mat2d::SetCols
    pub fn set_cols(&mut self, c1: &XY, c2: &XY) {
        *self = Mat2d::from_columns(*c1, *c2);
    }
    // gp_Mat2d::SetDiagonal
    pub fn set_diagonal(&mut self, x1: f64, x2: f64) {
        let [[a, _], [_, d]] = &mut self.m;
        *a = x1;
        *d = x2;
    }
    // gp_Mat2d::SetIdentity
    pub fn set_identity(&mut self) {
        self.m = [[1.0, 0.0], [0.0, 1.0]];
    }
    // gp_Mat2d::SetRotation
    pub fn set_rotation(&mut self, ang: f64) {
        let sin_a = libm::sin(ang);
        let cos_a = libm::cos(ang);
        self.m = [[cos_a, -sin_a], [sin_a, cos_a]];
    }
    // gp_Mat2d::SetRow (index outside 1..=2: unchecked in OCCT 8.0.1 as built, reported as an error)
    pub fn set_row(&mut self, row: i32, value: &XY) -> Result<(), ConstructionError> {
        let [r1, r2] = &mut self.m;
        match row {
            1 => *r1 = [value.x(), value.y()],
            2 => *r2 = [value.x(), value.y()],
            _ => return Err(ConstructionError),
        }
        Ok(())
    }
    // gp_Mat2d::SetRows
    pub fn set_rows(&mut self, r1: &XY, r2: &XY) {
        self.m = [[r1.x(), r1.y()], [r2.x(), r2.y()]];
    }
    // gp_Mat2d::SetScale
    pub fn set_scale(&mut self, s: f64) {
        self.m = [[s, 0.0], [0.0, s]];
    }
    // gp_Mat2d::SetValue
    pub fn set_value(&mut self, row: i32, col: i32, value: f64) -> Result<(), ConstructionError> {
        *self.change_value(row, col)? = value;
        Ok(())
    }
    // gp_Mat2d::Column (index outside 1..=2: unchecked in OCCT 8.0.1 as built, reported as an error)
    pub fn column(&self, col: i32) -> Result<XY, ConstructionError> {
        let [[a, b], [c, d]] = self.m;
        match col {
            1 => Ok(XY::new(a, c)),
            2 => Ok(XY::new(b, d)),
            _ => Err(ConstructionError),
        }
    }
    // gp_Mat2d::Row (index outside 1..=2: unchecked in OCCT 8.0.1 as built, reported as an error)
    pub fn row(&self, row: i32) -> Result<XY, ConstructionError> {
        let [[a, b], [c, d]] = self.m;
        match row {
            1 => Ok(XY::new(a, b)),
            2 => Ok(XY::new(c, d)),
            _ => Err(ConstructionError),
        }
    }
    // gp_Mat2d::Diagonal
    pub fn diagonal(&self) -> XY {
        let [[a, _], [_, d]] = self.m;
        XY::new(a, d)
    }
    // gp_Mat2d::Value
    pub fn value(&self, row: i32, col: i32) -> Result<f64, ConstructionError> {
        let (r, c) = cell_index(row, col)?;
        self.m.get(r).and_then(|l| l.get(c)).copied().ok_or(ConstructionError)
    }
    // gp_Mat2d::ChangeValue
    pub fn change_value(&mut self, row: i32, col: i32) -> Result<&mut f64, ConstructionError> {
        let (r, c) = cell_index(row, col)?;
        self.m.get_mut(r).and_then(|l| l.get_mut(c)).ok_or(ConstructionError)
    }
    // gp_Mat2d::Determinant
    pub fn determinant(&self) -> f64 {
        let [[a, b], [c, d]] = self.m;
        a * d - c * b
    }
    // gp_Mat2d::IsSingular
    pub fn is_singular(&self) -> bool {
        let mut det = self.determinant();
        if det < 0.0 {
            det = -det;
        }
        det <= resolution()
    }
    // gp_Mat2d::Add
    pub fn add(&mut self, other: &Mat2d) {
        let [[a, b], [c, d]] = other.m;
        let [[m00, m01], [m10, m11]] = &mut self.m;
        *m00 += a;
        *m01 += b;
        *m10 += c;
        *m11 += d;
    }
    // gp_Mat2d::Added
    pub fn added(&self, other: &Mat2d) -> Mat2d {
        let [[a, b], [c, d]] = self.m;
        let [[oa, ob], [oc, od]] = other.m;
        Mat2d { m: [[a + oa, b + ob], [c + oc, d + od]] }
    }
    // gp_Mat2d::Subtract
    pub fn subtract(&mut self, other: &Mat2d) {
        let [[a, b], [c, d]] = other.m;
        let [[m00, m01], [m10, m11]] = &mut self.m;
        *m00 -= a;
        *m01 -= b;
        *m10 -= c;
        *m11 -= d;
    }
    // gp_Mat2d::Subtracted
    pub fn subtracted(&self, other: &Mat2d) -> Mat2d {
        let [[a, b], [c, d]] = self.m;
        let [[oa, ob], [oc, od]] = other.m;
        Mat2d { m: [[a - oa, b - ob], [c - oc, d - od]] }
    }
    // gp_Mat2d::Divide (no zero check in OCCT: element-wise division)
    pub fn divide(&mut self, scalar: f64) {
        for l in &mut self.m {
            for e in l.iter_mut() {
                *e /= scalar;
            }
        }
    }
    // gp_Mat2d::Divided
    pub fn divided(&self, scalar: f64) -> Mat2d {
        let mut r = *self;
        r.divide(scalar);
        r
    }
    // gp_Mat2d::Multiply(double)
    pub fn multiply_scalar(&mut self, scalar: f64) {
        for l in &mut self.m {
            for e in l.iter_mut() {
                *e *= scalar;
            }
        }
    }
    // gp_Mat2d::Multiplied(double)
    pub fn multiplied_scalar(&self, scalar: f64) -> Mat2d {
        let mut r = *self;
        r.multiply_scalar(scalar);
        r
    }
    // gp_Mat2d::Multiply(gp_Mat2d), operand distinct from `self`
    pub fn multiply_mat(&mut self, other: &Mat2d) {
        let [[m00, m01], [m10, m11]] = self.m;
        let [[o00, o01], [o10, o11]] = other.m;
        let t00 = m00 * o00 + m01 * o10;
        let t10 = m10 * o00 + m11 * o10;
        let n01 = m00 * o01 + m01 * o11;
        let n11 = m10 * o01 + m11 * o11;
        self.m = [[t00, n01], [t10, n11]];
    }
    /// `gp_Mat2d::Multiply(*this)`: OCCT passes the matrix to itself, so `myMat[0][1]` is already
    /// overwritten when the `[1][1]` entry reads `theOther.myMat[0][1]`. Reproduced on purpose.
    pub(crate) fn square_in_place(&mut self) {
        let [[m00, m01], [m10, m11]] = self.m;
        let t00 = m00 * m00 + m01 * m10;
        let t10 = m10 * m00 + m11 * m10;
        let n01 = m00 * m01 + m01 * m11;
        // theOther.myMat[0][1] is now `n01`
        let n11 = m10 * n01 + m11 * m11;
        self.m = [[t00, n01], [t10, n11]];
    }
    // gp_Mat2d::Multiplied(gp_Mat2d)
    pub fn multiplied_mat(&self, other: &Mat2d) -> Mat2d {
        let mut r = *self;
        r.multiply_mat(other);
        r
    }
    // gp_Mat2d::PreMultiply
    pub fn pre_multiply(&mut self, other: &Mat2d) {
        let [[m00, m01], [m10, m11]] = self.m;
        let [[o00, o01], [o10, o11]] = other.m;
        let t00 = o00 * m00 + o01 * m10;
        let n10 = o10 * m00 + o11 * m10;
        let t01 = o00 * m01 + o01 * m11;
        let n11 = o10 * m01 + o11 * m11;
        self.m = [[t00, t01], [n10, n11]];
    }
    // gp_Mat2d::Invert (|det| <= resolution is unchecked in OCCT 8.0.1 as built, reported as an error)
    pub fn invert(&mut self) -> Result<(), ConstructionError> {
        let [[m00, m01], [m10, m11]] = self.m;
        let n00 = m11;
        let n01 = -m01;
        let n10 = -m10;
        let n11 = m00;
        let mut det = n00 * n11 - n01 * n10;
        if det.abs() <= resolution() {
            return Err(ConstructionError);
        }
        det = 1.0 / det;
        self.m = [[n00 * det, n01 * det], [n10 * det, n11 * det]];
        Ok(())
    }
    // gp_Mat2d::Inverted
    pub fn inverted(&self) -> Result<Mat2d, ConstructionError> {
        let mut r = *self;
        r.invert()?;
        Ok(r)
    }
    // gp_Mat2d::Power; the squaring `aTemp.Multiply(aTemp)` aliases its operand like OCCT.
    pub fn power(&mut self, n: i32) -> Result<(), ConstructionError> {
        if n == 1 {
            return Ok(());
        }
        if n == 0 {
            self.set_identity();
            return Ok(());
        }
        if n == -1 {
            return self.invert();
        }
        if n < 0 {
            self.invert()?;
        }
        let mut npower = i64::from(n).abs() - 1;
        let mut temp = *self;
        loop {
            if npower & 1 == 1 {
                self.multiply_mat(&temp);
            }
            if npower == 1 {
                break;
            }
            temp.square_in_place();
            npower /= 2;
        }
        Ok(())
    }
    // gp_Mat2d::Powered
    pub fn powered(&self, n: i32) -> Result<Mat2d, ConstructionError> {
        let mut r = *self;
        r.power(n)?;
        Ok(r)
    }
    // gp_Mat2d::Transpose
    pub fn transpose(&mut self) {
        let [[a, b], [c, d]] = self.m;
        self.m = [[a, c], [b, d]];
    }
    // gp_Mat2d::Transposed
    pub fn transposed(&self) -> Mat2d {
        let mut r = *self;
        r.transpose();
        r
    }
}
