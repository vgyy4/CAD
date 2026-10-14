//! `math_Matrix` and `math_DoubleTab`.
use super::vector::count;
use super::{Gauss, MathError, Vector};

/// A matrix with arbitrary integer bounds, stored row by row (`math_Matrix`).
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    pub(super) lower_row: i32,
    pub(super) lower_col: i32,
    pub(super) rows: usize,
    pub(super) cols: usize,
    pub(super) data: Vec<f64>,
}

fn same_shape(a: &Matrix, b: &Matrix) -> Result<(), MathError> {
    if a.rows == b.rows && a.cols == b.cols { Ok(()) } else { Err(MathError::Dimension) }
}

impl Matrix {
    /// `math_Matrix(lowerRow, upperRow, lowerCol, upperCol)`; the items are zero.
    pub fn new(lower_row: i32, upper_row: i32, lower_col: i32, upper_col: i32) -> Result<Self, MathError> {
        let rows = count(lower_row, upper_row)?;
        let cols = count(lower_col, upper_col)?;
        let n = rows.checked_mul(cols).ok_or(MathError::Construction)?;
        Ok(Self { lower_row, lower_col, rows, cols, data: vec![0.0; n] })
    }
    /// `math_Matrix(lowerRow, upperRow, lowerCol, upperCol, initialValue)`.
    pub fn filled(
        lower_row: i32,
        upper_row: i32,
        lower_col: i32,
        upper_col: i32,
        value: f64,
    ) -> Result<Self, MathError> {
        let mut m = Self::new(lower_row, upper_row, lower_col, upper_col)?;
        m.init(value);
        Ok(m)
    }
    /// `math_Matrix(void* tab, ...)` from row-major values.
    pub fn from_slice(
        lower_row: i32,
        lower_col: i32,
        row_count: i32,
        col_count: i32,
        values: &[f64],
    ) -> Result<Self, MathError> {
        if row_count < 1 || col_count < 1 {
            return Err(MathError::Construction);
        }
        let upper_row = lower_row.checked_add(row_count - 1).ok_or(MathError::Construction)?;
        let upper_col = lower_col.checked_add(col_count - 1).ok_or(MathError::Construction)?;
        let mut m = Self::new(lower_row, upper_row, lower_col, upper_col)?;
        if m.data.len() != values.len() {
            return Err(MathError::Dimension);
        }
        m.data.copy_from_slice(values);
        Ok(m)
    }
    fn offset(&self, row: i32, col: i32) -> Result<usize, MathError> {
        let r = usize::try_from(i64::from(row) - i64::from(self.lower_row)).map_err(|_| MathError::Range)?;
        let c = usize::try_from(i64::from(col) - i64::from(self.lower_col)).map_err(|_| MathError::Range)?;
        if r >= self.rows || c >= self.cols {
            return Err(MathError::Range);
        }
        Ok(r * self.cols + c)
    }
    /// Item at the 0-based position (`0` if out of range).
    pub(super) fn at0(&self, r: usize, c: usize) -> f64 {
        if c >= self.cols {
            return 0.0;
        }
        self.data.get(r * self.cols + c).copied().unwrap_or(0.0)
    }
    /// Item at the 1-based position (the numerical recipes' view with lower bounds 1); `0` if out of range.
    pub(super) fn at(&self, i: i32, j: i32) -> f64 {
        match (usize::try_from(i - 1), usize::try_from(j - 1)) {
            (Ok(r), Ok(c)) => self.at0(r, c),
            _ => 0.0,
        }
    }
    /// Writes the item at the 1-based position (ignored if out of range).
    pub(super) fn put(&mut self, i: i32, j: i32, value: f64) {
        let (Ok(r), Ok(c)) = (usize::try_from(i - 1), usize::try_from(j - 1)) else {
            return;
        };
        if c >= self.cols {
            return;
        }
        if let Some(slot) = self.data.get_mut(r * self.cols + c) {
            *slot = value;
        }
    }
    /// `RowNumber()`.
    pub fn row_number(&self) -> i32 {
        i32::try_from(self.rows).unwrap_or(i32::MAX)
    }
    /// `ColNumber()`.
    pub fn col_number(&self) -> i32 {
        i32::try_from(self.cols).unwrap_or(i32::MAX)
    }
    /// `LowerRow()`.
    pub fn lower_row(&self) -> i32 {
        self.lower_row
    }
    /// `UpperRow()`.
    pub fn upper_row(&self) -> i32 {
        self.lower_row.wrapping_add(self.row_number()).wrapping_sub(1)
    }
    /// `LowerCol()`.
    pub fn lower_col(&self) -> i32 {
        self.lower_col
    }
    /// `UpperCol()`.
    pub fn upper_col(&self) -> i32 {
        self.lower_col.wrapping_add(self.col_number()).wrapping_sub(1)
    }
    /// `Value(row, col)`.
    pub fn value(&self, row: i32, col: i32) -> Result<f64, MathError> {
        self.data.get(self.offset(row, col)?).copied().ok_or(MathError::Range)
    }
    /// `Value(row, col) = value`.
    pub fn set_value(&mut self, row: i32, col: i32, value: f64) -> Result<(), MathError> {
        let k = self.offset(row, col)?;
        let slot = self.data.get_mut(k).ok_or(MathError::Range)?;
        *slot = value;
        Ok(())
    }
    /// `Init(value)`.
    pub fn init(&mut self, value: f64) {
        self.data.fill(value);
    }
    /// `Initialized(other)`: copies the items; the shapes must agree.
    pub fn initialize_from(&mut self, other: &Self) -> Result<(), MathError> {
        same_shape(self, other)?;
        self.data.clone_from(&other.data);
        Ok(())
    }
    /// `Initialized(other)`.
    pub fn initialized(&mut self, other: &Self) -> Result<(), MathError> {
        self.initialize_from(other)
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        let mut s = format!("math_Matrix of RowNumber = {} and ColNumber = {}\n", self.rows, self.cols);
        for r in 0..self.rows {
            for c in 0..self.cols {
                s.push_str(&format!(
                    "math_Matrix ( {}, {} ) = {}\n",
                    self.lower_row.wrapping_add(r as i32),
                    self.lower_col.wrapping_add(c as i32),
                    self.at0(r, c)
                ));
            }
        }
        s
    }
    /// `Multiply(value)`.
    pub fn multiply(&mut self, value: f64) {
        for x in &mut self.data {
            *x *= value;
        }
    }
    /// `Multiplied(value)`.
    pub fn multiplied(&self, value: f64) -> Self {
        let mut r = self.clone();
        r.multiply(value);
        r
    }
    /// `TMultiplied(value)`.
    pub fn t_multiplied(&self, value: f64) -> Self {
        self.multiplied(value)
    }
    /// `Divide(value)`: fails (`Standard_DivideByZero`) when `|value| <= RealEpsilon`.
    pub fn divide(&mut self, value: f64) -> Result<(), MathError> {
        if value.abs() <= f64::EPSILON {
            return Err(MathError::Domain);
        }
        for x in &mut self.data {
            *x /= value;
        }
        Ok(())
    }
    /// `Divided(value)`: OCCT multiplies by `1. / value`.
    pub fn divided(&self, value: f64) -> Result<Self, MathError> {
        if value.abs() <= f64::EPSILON {
            return Err(MathError::Domain);
        }
        Ok(self.multiplied(1.0 / value))
    }
    /// `Add(right)`.
    pub fn add(&mut self, right: &Self) -> Result<(), MathError> {
        same_shape(self, right)?;
        for (x, y) in self.data.iter_mut().zip(&right.data) {
            *x += y;
        }
        Ok(())
    }
    /// `Added(right)`.
    pub fn added(&self, right: &Self) -> Result<Self, MathError> {
        let mut r = self.clone();
        r.add(right)?;
        Ok(r)
    }
    /// `Add(left, right)`: `self = left + right`.
    pub fn add_from(&mut self, left: &Self, right: &Self) -> Result<(), MathError> {
        same_shape(self, right)?;
        same_shape(right, left)?;
        for (z, (x, y)) in self.data.iter_mut().zip(left.data.iter().zip(&right.data)) {
            *z = x + y;
        }
        Ok(())
    }
    /// `Subtract(right)`.
    pub fn subtract(&mut self, right: &Self) -> Result<(), MathError> {
        same_shape(self, right)?;
        for (x, y) in self.data.iter_mut().zip(&right.data) {
            *x -= y;
        }
        Ok(())
    }
    /// `Subtracted(right)`.
    pub fn subtracted(&self, right: &Self) -> Result<Self, MathError> {
        let mut r = self.clone();
        r.subtract(right)?;
        Ok(r)
    }
    /// `Subtract(left, right)`: `self = left - right`.
    pub fn subtract_from(&mut self, left: &Self, right: &Self) -> Result<(), MathError> {
        same_shape(self, right)?;
        same_shape(right, left)?;
        for (z, (x, y)) in self.data.iter_mut().zip(left.data.iter().zip(&right.data)) {
            *z = x - y;
        }
        Ok(())
    }
    /// `Opposite()`.
    pub fn opposite(&self) -> Self {
        Self { data: self.data.iter().map(|x| -x).collect(), ..self.clone() }
    }
    /// `Set(i1, i2, j1, j2, m)`: replaces the block `i1..=i2` x `j1..=j2` by `m`.
    pub fn set_slice(
        &mut self,
        first_row: i32,
        last_row: i32,
        first_col: i32,
        last_col: i32,
        m: &Self,
    ) -> Result<(), MathError> {
        if first_row > last_row
            || first_col > last_col
            || i64::from(last_row) - i64::from(first_row) + 1 != i64::from(m.row_number())
            || i64::from(last_col) - i64::from(first_col) + 1 != i64::from(m.col_number())
        {
            return Err(MathError::Dimension);
        }
        for r in 0..m.rows {
            for c in 0..m.cols {
                self.set_value(first_row.wrapping_add(r as i32), first_col.wrapping_add(c as i32), m.at0(r, c))?;
            }
        }
        Ok(())
    }
    /// `SetDiag(value)`.
    pub fn set_diag(&mut self, value: f64) -> Result<(), MathError> {
        if self.rows != self.cols {
            return Err(MathError::NotSquare);
        }
        for i in 0..self.rows {
            let slot = self.data.get_mut(i * self.cols + i).ok_or(MathError::Range)?;
            *slot = value;
        }
        Ok(())
    }
    /// `SetRow(row, v)`.
    pub fn set_row(&mut self, row: i32, v: &Vector) -> Result<(), MathError> {
        if self.cols != v.data.len() {
            return Err(MathError::Dimension);
        }
        for (c, x) in v.data.iter().enumerate() {
            self.set_value(row, self.lower_col.wrapping_add(c as i32), *x)?;
        }
        Ok(())
    }
    /// `SetCol(col, v)`.
    pub fn set_col(&mut self, col: i32, v: &Vector) -> Result<(), MathError> {
        if self.rows != v.data.len() {
            return Err(MathError::Dimension);
        }
        for (r, x) in v.data.iter().enumerate() {
            self.set_value(self.lower_row.wrapping_add(r as i32), col, *x)?;
        }
        Ok(())
    }
    /// `Row(row)`: a vector with the bounds `LowerCol()..=UpperCol()`.
    pub fn row(&self, row: i32) -> Result<Vector, MathError> {
        let mut v = Vector::new(self.lower_col, self.upper_col())?;
        for (c, slot) in v.data.iter_mut().enumerate() {
            *slot = self.value(row, self.lower_col.wrapping_add(c as i32))?;
        }
        Ok(v)
    }
    /// `Col(col)`: a vector with the bounds `LowerRow()..=UpperRow()`.
    pub fn col(&self, col: i32) -> Result<Vector, MathError> {
        let mut v = Vector::new(self.lower_row, self.upper_row())?;
        for (r, slot) in v.data.iter_mut().enumerate() {
            *slot = self.value(self.lower_row.wrapping_add(r as i32), col)?;
        }
        Ok(v)
    }
    /// `SwapRow(row1, row2)`.
    pub fn swap_row(&mut self, row1: i32, row2: i32) -> Result<(), MathError> {
        let v1 = self.row(row1)?;
        let v2 = self.row(row2)?;
        self.set_row(row1, &v2)?;
        self.set_row(row2, &v1)
    }
    /// `SwapCol(col1, col2)`.
    pub fn swap_col(&mut self, col1: i32, col2: i32) -> Result<(), MathError> {
        let v1 = self.col(col1)?;
        let v2 = self.col(col2)?;
        self.set_col(col1, &v2)?;
        self.set_col(col2, &v1)
    }
    /// `Transposed()`: bounds `LowerCol..=UpperCol` x `LowerRow..=UpperRow`.
    pub fn transposed(&self) -> Result<Self, MathError> {
        let mut r = Self::new(self.lower_col, self.upper_col(), self.lower_row, self.upper_row())?;
        for i in 0..self.rows {
            for j in 0..self.cols {
                if let Some(slot) = r.data.get_mut(j * r.cols + i) {
                    *slot = self.at0(i, j);
                }
            }
        }
        Ok(r)
    }
    /// `Transpose()`: square matrices only; the lower row and column bounds are exchanged.
    pub fn transpose(&mut self) -> Result<(), MathError> {
        if self.rows != self.cols {
            return Err(MathError::NotSquare);
        }
        let n = self.rows;
        for i in 0..n {
            for j in i..n {
                self.data.swap(i * n + j, j * n + i);
            }
        }
        core::mem::swap(&mut self.lower_row, &mut self.lower_col);
        Ok(())
    }
    /// `Multiplied(right)`: the product, bounds `LowerRow..=UpperRow` x `right.LowerCol..=right.UpperCol`.
    pub fn multiplied_matrix(&self, right: &Self) -> Result<Self, MathError> {
        if self.cols != right.rows {
            return Err(MathError::Dimension);
        }
        let mut result = Self::new(self.lower_row, self.upper_row(), right.lower_col, right.upper_col())?;
        for i in 0..self.rows {
            for j in 0..self.cols {
                let left_val = self.at0(i, j);
                for jr in 0..right.cols {
                    if let Some(slot) = result.data.get_mut(i * right.cols + jr) {
                        *slot += left_val * right.at0(j, jr);
                    }
                }
            }
        }
        Ok(result)
    }
    /// `Multiply(right)`: `self = self * right`.
    pub fn multiply_matrix(&mut self, right: &Self) -> Result<(), MathError> {
        if self.cols != right.rows || right.cols != self.cols {
            return Err(MathError::Dimension);
        }
        let mut result = self.clone();
        for i in 0..self.rows {
            for j2 in 0..right.cols {
                let mut sum = 0.0;
                for j in 0..self.cols {
                    sum += self.at0(i, j) * right.at0(j, j2);
                }
                if let Some(slot) = result.data.get_mut(i * result.cols + j2) {
                    *slot = sum;
                }
            }
        }
        self.data = result.data;
        Ok(())
    }
    /// `TMultiply(right)`: `self^T * right`, bounds `LowerCol..=UpperCol` x `right.LowerCol..=right.UpperCol`.
    pub fn t_multiply(&self, right: &Self) -> Result<Self, MathError> {
        if self.rows != right.rows {
            return Err(MathError::Dimension);
        }
        let mut result = Self::new(self.lower_col, self.upper_col(), right.lower_col, right.upper_col())?;
        for i in 0..self.cols {
            for j2 in 0..right.cols {
                let mut sum = 0.0;
                for j in 0..self.rows {
                    sum += self.at0(j, i) * right.at0(j, j2);
                }
                if let Some(slot) = result.data.get_mut(i * right.cols + j2) {
                    *slot = sum;
                }
            }
        }
        Ok(result)
    }
    /// `Multiplied(vector)`: `self * right`, bounds `LowerRow..=UpperRow`.
    pub fn multiply_vector(&self, right: &Vector) -> Result<Vector, MathError> {
        if self.cols != right.data.len() {
            return Err(MathError::Dimension);
        }
        let mut result = Vector::new(self.lower_row, self.upper_row())?;
        for (i, slot) in result.data.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (j, y) in right.data.iter().enumerate() {
                acc += self.at0(i, j) * y;
            }
            *slot = acc;
        }
        Ok(result)
    }
    /// `Multiply(left, right)`: `self = left * right`.
    pub fn multiply_from(&mut self, left: &Self, right: &Self) -> Result<(), MathError> {
        if left.cols != right.rows || self.rows != left.rows || self.cols != right.cols {
            return Err(MathError::Dimension);
        }
        self.init(0.0);
        for i in 0..self.rows {
            for k in 0..left.cols {
                let left_val = left.at0(i, k);
                for j in 0..self.cols {
                    if let Some(slot) = self.data.get_mut(i * self.cols + j) {
                        *slot += left_val * right.at0(k, j);
                    }
                }
            }
        }
        Ok(())
    }
    /// `TMultiply(tLeft, right)`: `self = tLeft^T * right`.
    pub fn t_multiply_from(&mut self, t_left: &Self, right: &Self) -> Result<(), MathError> {
        if t_left.rows != right.rows || self.rows != t_left.cols || self.cols != right.cols {
            return Err(MathError::Dimension);
        }
        for i in 0..self.rows {
            for j in 0..self.cols {
                let mut sum = 0.0;
                for k in 0..t_left.rows {
                    sum += t_left.at0(k, i) * right.at0(k, j);
                }
                if let Some(slot) = self.data.get_mut(i * self.cols + j) {
                    *slot = sum;
                }
            }
        }
        Ok(())
    }
    /// `Multiply(left, right)` with two vectors: `self(i, j) = left(i) * right(j)`.
    pub fn outer_product_from(&mut self, left: &Vector, right: &Vector) -> Result<(), MathError> {
        if self.rows != left.data.len() || self.cols != right.data.len() {
            return Err(MathError::Dimension);
        }
        for (i, x) in left.data.iter().enumerate() {
            for (j, y) in right.data.iter().enumerate() {
                if let Some(slot) = self.data.get_mut(i * self.cols + j) {
                    *slot = x * y;
                }
            }
        }
        Ok(())
    }
    /// The outer product as a new matrix with the bounds of `left` x `right`.
    pub fn outer_product(left: &Vector, right: &Vector) -> Result<Self, MathError> {
        let mut m = Self::new(left.lower, left.upper(), right.lower, right.upper())?;
        m.outer_product_from(left, right)?;
        Ok(m)
    }
    /// `Determinant()`: zero when the Gauss decomposition is not done (also for non-square matrices).
    pub fn determinant(&self) -> f64 {
        match Gauss::new(self, 1.0e-20) {
            Ok(g) if g.is_done() => g.determinant(),
            _ => 0.0,
        }
    }
    /// `Invert()`: fails with `NotSquare` or `Singular`.
    pub fn invert(&mut self) -> Result<(), MathError> {
        if self.rows != self.cols {
            return Err(MathError::NotSquare);
        }
        let g = Gauss::new(self, 1.0e-20)?;
        if !g.is_done() {
            return Err(MathError::Singular);
        }
        g.invert_into(self)
    }
    /// `Inverse()`.
    pub fn inverse(&self) -> Result<Self, MathError> {
        let mut r = self.clone();
        r.invert()?;
        Ok(r)
    }
}

/// `math_DoubleTab`: the 2D storage behind `math_Matrix`.
#[derive(Debug, Clone, PartialEq)]
pub struct DoubleTab {
    m: Matrix,
    deletable: bool,
}

impl DoubleTab {
    /// OCCT keeps up to this many items in the object itself (`IsDeletable()` is then false).
    const BUFFER_SIZE: usize = 64;
    /// `math_DoubleTab(lowerRow, upperRow, lowerCol, upperCol)`.
    pub fn new(lower_row: i32, upper_row: i32, lower_col: i32, upper_col: i32) -> Result<Self, MathError> {
        let m = Matrix::new(lower_row, upper_row, lower_col, upper_col)?;
        let deletable = m.data.len() > Self::BUFFER_SIZE;
        Ok(Self { m, deletable })
    }
    /// Like [`Self::new`], with every item set to `value`.
    pub fn filled(
        lower_row: i32,
        upper_row: i32,
        lower_col: i32,
        upper_col: i32,
        value: f64,
    ) -> Result<Self, MathError> {
        let mut t = Self::new(lower_row, upper_row, lower_col, upper_col)?;
        t.init(value);
        Ok(t)
    }
    /// `math_DoubleTab(void* tab, ...)` over caller data (row major): not deletable.
    pub fn from_slice(
        lower_row: i32,
        lower_col: i32,
        row_count: i32,
        col_count: i32,
        values: &[f64],
    ) -> Result<Self, MathError> {
        Ok(Self { m: Matrix::from_slice(lower_row, lower_col, row_count, col_count, values)?, deletable: false })
    }
    /// `Init(value)`.
    pub fn init(&mut self, value: f64) {
        self.m.init(value);
    }
    /// `IsDeletable()`.
    pub fn is_deletable(&self) -> bool {
        self.deletable
    }
    /// `Copy(other)`: copies the items into `other`; the shapes must agree.
    pub fn copy_to(&self, other: &mut Self) -> Result<(), MathError> {
        other.m.initialize_from(&self.m)
    }
    /// `LowerRow()`.
    pub fn lower_row(&self) -> i32 {
        self.m.lower_row()
    }
    /// `UpperRow()`.
    pub fn upper_row(&self) -> i32 {
        self.m.upper_row()
    }
    /// `LowerCol()`.
    pub fn lower_col(&self) -> i32 {
        self.m.lower_col()
    }
    /// `UpperCol()`.
    pub fn upper_col(&self) -> i32 {
        self.m.upper_col()
    }
    /// `NbRows()`.
    pub fn row_number(&self) -> i32 {
        self.m.row_number()
    }
    /// `NbColumns()`.
    pub fn col_number(&self) -> i32 {
        self.m.col_number()
    }
    /// `Value(row, col)`.
    pub fn value(&self, row: i32, col: i32) -> Result<f64, MathError> {
        self.m.value(row, col)
    }
    /// `Value(row, col) = value`.
    pub fn set_value(&mut self, row: i32, col: i32, value: f64) -> Result<(), MathError> {
        self.m.set_value(row, col, value)
    }
    /// `SetLowerRow(lowerRow)`: shifts the row bounds.
    pub fn set_lower_row(&mut self, lower_row: i32) {
        self.m.lower_row = lower_row;
    }
    /// `SetLowerCol(lowerCol)`: shifts the column bounds.
    pub fn set_lower_col(&mut self, lower_col: i32) {
        self.m.lower_col = lower_col;
    }
}
