//! `math_VectorBase<T>` (`math_Vector` = `T = f64`, `math_IntegerVector` = `T = i32`).
use super::{MathError, Matrix};
use crate::gp::XYZ;

/// Item type of a [`VectorBase`] (OCCT's `TheItemType` template parameter: `double` or `int`).
pub trait Element: Copy + PartialEq + core::fmt::Debug + core::fmt::Display {
    /// The value `0`.
    const ZERO: Self;
    /// `static_cast<double>(x)`.
    fn to_f64(self) -> f64;
    /// `static_cast<TheItemType>(x)` (truncating for integers).
    fn from_f64(x: f64) -> Self;
    /// `a + b` (wrapping for integers).
    fn plus(self, o: Self) -> Self;
    /// `a - b` (wrapping for integers).
    fn minus(self, o: Self) -> Self;
    /// `a * b` (wrapping for integers).
    fn times(self, o: Self) -> Self;
    /// `a / b` (truncating for integers).
    fn over(self, o: Self) -> Self;
    /// `-a`.
    fn negated(self) -> Self;
    /// `std::abs(a) <= RealEpsilon()`.
    fn is_tiny(self) -> bool;
}

impl Element for f64 {
    const ZERO: Self = 0.0;
    fn to_f64(self) -> f64 {
        self
    }
    fn from_f64(x: f64) -> Self {
        x
    }
    fn plus(self, o: Self) -> Self {
        self + o
    }
    fn minus(self, o: Self) -> Self {
        self - o
    }
    fn times(self, o: Self) -> Self {
        self * o
    }
    fn over(self, o: Self) -> Self {
        self / o
    }
    fn negated(self) -> Self {
        -self
    }
    fn is_tiny(self) -> bool {
        self.abs() <= f64::EPSILON
    }
}

impl Element for i32 {
    const ZERO: Self = 0;
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(x: f64) -> Self {
        x as i32
    }
    fn plus(self, o: Self) -> Self {
        self.wrapping_add(o)
    }
    fn minus(self, o: Self) -> Self {
        self.wrapping_sub(o)
    }
    fn times(self, o: Self) -> Self {
        self.wrapping_mul(o)
    }
    fn over(self, o: Self) -> Self {
        self.checked_div(o).unwrap_or(0)
    }
    fn negated(self) -> Self {
        self.wrapping_neg()
    }
    fn is_tiny(self) -> bool {
        f64::from(self).abs() <= f64::EPSILON
    }
}

pub(super) fn count(lo: i32, hi: i32) -> Result<usize, MathError> {
    if hi < lo {
        return Err(MathError::Construction);
    }
    usize::try_from(i64::from(hi) - i64::from(lo) + 1).map_err(|_| MathError::Construction)
}

fn same_len(a: usize, b: usize) -> Result<(), MathError> {
    if a == b { Ok(()) } else { Err(MathError::Dimension) }
}

/// A vector with arbitrary integer bounds (`math_VectorBase`).
#[derive(Debug, Clone, PartialEq)]
pub struct VectorBase<T: Element> {
    pub(super) lower: i32,
    pub(super) data: Vec<T>,
}

/// `math_Vector`.
pub type Vector = VectorBase<f64>;
/// `math_IntegerVector`.
pub type IntegerVector = VectorBase<i32>;

impl<T: Element> VectorBase<T> {
    /// `math_VectorBase(lower, upper)`; the items are zero.
    pub fn new(lower: i32, upper: i32) -> Result<Self, MathError> {
        Ok(Self { lower, data: vec![T::ZERO; count(lower, upper)?] })
    }
    /// `math_VectorBase(lower, upper, initialValue)`.
    pub fn filled(lower: i32, upper: i32, value: T) -> Result<Self, MathError> {
        Ok(Self { lower, data: vec![value; count(lower, upper)?] })
    }
    /// `math_VectorBase(const TheItemType* tab, lower, upper)`; the upper bound follows from the slice length.
    pub fn from_slice(lower: i32, values: &[T]) -> Result<Self, MathError> {
        if values.is_empty() {
            return Err(MathError::Construction);
        }
        lower
            .checked_add(i32::try_from(values.len() - 1).map_err(|_| MathError::Construction)?)
            .ok_or(MathError::Construction)?;
        Ok(Self { lower, data: values.to_vec() })
    }
    /// `math_VectorBase(const gp_XY&)`: bounds `1..2`, items converted with `static_cast`.
    pub fn from_xy(x: f64, y: f64) -> Self {
        Self { lower: 1, data: vec![T::from_f64(x), T::from_f64(y)] }
    }
    /// `math_VectorBase(const gp_XYZ&)`: bounds `1..3`, items converted with `static_cast`.
    pub fn from_xyz(xyz: &XYZ) -> Self {
        Self { lower: 1, data: vec![T::from_f64(xyz.x()), T::from_f64(xyz.y()), T::from_f64(xyz.z())] }
    }
    fn offset(&self, i: i32) -> Result<usize, MathError> {
        let n = usize::try_from(i64::from(i) - i64::from(self.lower)).map_err(|_| MathError::Range)?;
        if n >= self.data.len() { Err(MathError::Range) } else { Ok(n) }
    }
    /// `Length()`.
    pub fn length(&self) -> i32 {
        i32::try_from(self.data.len()).unwrap_or(i32::MAX)
    }
    /// `Lower()`.
    pub fn lower(&self) -> i32 {
        self.lower
    }
    /// `Upper()`.
    pub fn upper(&self) -> i32 {
        self.lower.wrapping_add(self.length()).wrapping_sub(1)
    }
    /// `Value(index)` / `operator()`.
    pub fn value(&self, index: i32) -> Result<T, MathError> {
        self.data.get(self.offset(index)?).copied().ok_or(MathError::Range)
    }
    /// `Value(index) = value`.
    pub fn set_value(&mut self, index: i32, value: T) -> Result<(), MathError> {
        let k = self.offset(index)?;
        let slot = self.data.get_mut(k).ok_or(MathError::Range)?;
        *slot = value;
        Ok(())
    }
    /// The items in increasing index order.
    pub fn as_slice(&self) -> &[T] {
        &self.data
    }
    /// 1-based position access used by the translated numerical recipes (zero if out of range).
    pub(super) fn at(&self, pos: i32) -> T {
        usize::try_from(pos - 1).ok().and_then(|k| self.data.get(k)).copied().unwrap_or(T::ZERO)
    }
    /// 1-based position write used by the translated numerical recipes (ignored if out of range).
    pub(super) fn put(&mut self, pos: i32, value: T) {
        if let Some(slot) = usize::try_from(pos - 1).ok().and_then(|k| self.data.get_mut(k)) {
            *slot = value;
        }
    }
    /// `Init(value)`.
    pub fn init(&mut self, value: T) {
        self.data.fill(value);
    }
    /// `Resize(size)`: keeps the lower bound and the overlapping items.
    pub fn resize(&mut self, size: i32) -> Result<(), MathError> {
        if size < 1 {
            return Err(MathError::Construction);
        }
        self.data.resize(usize::try_from(size).map_err(|_| MathError::Construction)?, T::ZERO);
        Ok(())
    }
    fn sum_of_squares(&self) -> f64 {
        let (mut s1, mut s2, mut s3, mut s4) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
        let mut chunks = self.data.chunks_exact(4);
        for c in chunks.by_ref() {
            if let [a, b, c, d] = *c {
                let (a, b, c, d) = (a.to_f64(), b.to_f64(), c.to_f64(), d.to_f64());
                s1 += a * a;
                s2 += b * b;
                s3 += c * c;
                s4 += d * d;
            }
        }
        for a in chunks.remainder() {
            let a = a.to_f64();
            s1 += a * a;
        }
        (s1 + s2) + (s3 + s4)
    }
    /// `Norm()`.
    pub fn norm(&self) -> f64 {
        self.sum_of_squares().sqrt()
    }
    /// `Norm2()`.
    pub fn norm2(&self) -> f64 {
        self.sum_of_squares()
    }
    /// `Max()`: index of the largest item, `0` if none exceeds `-DBL_MAX`.
    pub fn max_index(&self) -> i32 {
        let mut found = 0;
        let mut x = f64::MIN;
        for (k, v) in self.data.iter().enumerate() {
            if v.to_f64() > x {
                x = v.to_f64();
                found = self.lower.wrapping_add(k as i32);
            }
        }
        found
    }
    /// `Min()`: index of the smallest item, `0` if none is below `DBL_MAX`.
    pub fn min_index(&self) -> i32 {
        let mut found = 0;
        let mut x = f64::MAX;
        for (k, v) in self.data.iter().enumerate() {
            if v.to_f64() < x {
                x = v.to_f64();
                found = self.lower.wrapping_add(k as i32);
            }
        }
        found
    }
    /// `Normalize()`: fails (`Standard_NullValue`) when the norm is not larger than `RealEpsilon`.
    pub fn normalize(&mut self) -> Result<(), MathError> {
        let n = self.norm();
        if n <= f64::EPSILON {
            return Err(MathError::Domain);
        }
        for x in &mut self.data {
            *x = T::from_f64(x.to_f64() / n);
        }
        Ok(())
    }
    /// `Normalized()`.
    pub fn normalized(&self) -> Result<Self, MathError> {
        let mut r = self.clone();
        r.normalize()?;
        Ok(r)
    }
    /// `Invert()`, including OCCT's loop bound `(Lower() + Length()) >> 1` (which swaps the middle
    /// pairs back when the lower bound is negative). Where OCCT would raise an out-of-range
    /// exception (it indexes below `Lower()`), the remaining swaps are not done.
    pub fn invert(&mut self) {
        let lower = self.lower;
        let upper = self.upper();
        let last = lower.wrapping_add(self.length()) >> 1;
        let mut index = lower;
        while index <= last {
            let j = upper.wrapping_add(lower).wrapping_sub(index);
            let (Ok(a), Ok(b)) = (self.offset(index), self.offset(j)) else {
                return;
            };
            self.data.swap(a, b);
            index += 1;
        }
    }
    /// `Inverse()`.
    pub fn inverse(&self) -> Self {
        let mut r = self.clone();
        r.invert();
        r
    }
    /// `Opposite()`.
    pub fn opposite(&self) -> Self {
        Self { lower: self.lower, data: self.data.iter().map(|x| x.negated()).collect() }
    }
    /// `Multiply(value)`.
    pub fn multiply(&mut self, value: T) {
        for x in &mut self.data {
            *x = x.times(value);
        }
    }
    /// `Multiplied(value)`.
    pub fn multiplied(&self, value: T) -> Self {
        let mut r = self.clone();
        r.multiply(value);
        r
    }
    /// `TMultiplied(value)`.
    pub fn t_multiplied(&self, value: T) -> Self {
        self.multiplied(value)
    }
    /// `Divide(value)`: fails (`Standard_DivideByZero`) when `|value| <= RealEpsilon`.
    pub fn divide(&mut self, value: T) -> Result<(), MathError> {
        if value.is_tiny() {
            return Err(MathError::Domain);
        }
        for x in &mut self.data {
            *x = x.over(value);
        }
        Ok(())
    }
    /// `Divided(value)`: OCCT multiplies by `1. / value` (converted to the item type).
    pub fn divided(&self, value: T) -> Result<Self, MathError> {
        if value.is_tiny() {
            return Err(MathError::Domain);
        }
        Ok(self.multiplied(T::from_f64(1.0 / value.to_f64())))
    }
    /// `Add(right)`.
    pub fn add(&mut self, right: &Self) -> Result<(), MathError> {
        same_len(self.data.len(), right.data.len())?;
        for (x, y) in self.data.iter_mut().zip(&right.data) {
            *x = x.plus(*y);
        }
        Ok(())
    }
    /// `Added(right)`.
    pub fn added(&self, right: &Self) -> Result<Self, MathError> {
        let mut r = self.clone();
        r.add(right)?;
        Ok(r)
    }
    /// `Subtract(right)`.
    pub fn subtract(&mut self, right: &Self) -> Result<(), MathError> {
        same_len(self.data.len(), right.data.len())?;
        for (x, y) in self.data.iter_mut().zip(&right.data) {
            *x = x.minus(*y);
        }
        Ok(())
    }
    /// `Subtracted(right)`.
    pub fn subtracted(&self, right: &Self) -> Result<Self, MathError> {
        let mut r = self.clone();
        r.subtract(right)?;
        Ok(r)
    }
    /// `Multiplied(vector)`: the dot product.
    pub fn dot(&self, right: &Self) -> Result<T, MathError> {
        same_len(self.data.len(), right.data.len())?;
        let mut r = T::ZERO;
        for (x, y) in self.data.iter().zip(&right.data) {
            r = r.plus(x.times(*y));
        }
        Ok(r)
    }
    /// `Multiply(left, right)` with a scalar: `self = left * right`.
    pub fn scale_from(&mut self, left: T, right: &Self) -> Result<(), MathError> {
        same_len(self.data.len(), right.data.len())?;
        for (z, x) in self.data.iter_mut().zip(&right.data) {
            *z = left.times(*x);
        }
        Ok(())
    }
    /// `Multiply(left, right)` with a scalar: same as [`Self::scale_from`].
    pub fn multiply_from(&mut self, left: T, right: &Self) -> Result<(), MathError> {
        self.scale_from(left, right)
    }
    /// `Set(first, last, v)`: replaces the items `first..=last` by `v`.
    pub fn set_slice(&mut self, first: i32, last: i32, v: &Self) -> Result<(), MathError> {
        if first < self.lower
            || last > self.upper()
            || first > last
            || i64::from(last) - i64::from(first) + 1 != i64::from(v.length())
        {
            return Err(MathError::Range);
        }
        let start = self.offset(first)?;
        for (k, x) in v.data.iter().enumerate() {
            let slot = self.data.get_mut(start + k).ok_or(MathError::Range)?;
            *slot = *x;
        }
        Ok(())
    }
    /// `Slice(first, last)`: a vector with the bounds `min(first, last)..=max(first, last)` that
    /// holds the items of the same indices.
    pub fn slice(&self, first: i32, last: i32) -> Result<Self, MathError> {
        if first < self.lower || first > self.upper() || last < self.lower || last > self.upper() {
            return Err(MathError::Range);
        }
        let (lo, hi) = if last >= first { (first, last) } else { (last, first) };
        let from = self.offset(lo)?;
        let to = self.offset(hi)?;
        let data = self.data.get(from..=to).ok_or(MathError::Range)?.to_vec();
        Ok(Self { lower: lo, data })
    }
    /// `Initialized(other)`: copies the items (the lengths must agree), keeping the bounds.
    pub fn initialized(&mut self, other: &Self) -> Result<(), MathError> {
        same_len(self.data.len(), other.data.len())?;
        self.data.clone_from(&other.data);
        Ok(())
    }
    /// `Add(left, right)`: `self = left + right`.
    pub fn add_from(&mut self, left: &Self, right: &Self) -> Result<(), MathError> {
        same_len(self.data.len(), right.data.len())?;
        same_len(right.data.len(), left.data.len())?;
        for (z, (x, y)) in self.data.iter_mut().zip(left.data.iter().zip(&right.data)) {
            *z = x.plus(*y);
        }
        Ok(())
    }
    /// `Subtract(left, right)`: `self = left - right`.
    pub fn subtract_from(&mut self, left: &Self, right: &Self) -> Result<(), MathError> {
        same_len(self.data.len(), right.data.len())?;
        same_len(right.data.len(), left.data.len())?;
        for (z, (x, y)) in self.data.iter_mut().zip(left.data.iter().zip(&right.data)) {
            *z = x.minus(*y);
        }
        Ok(())
    }
    /// `Dump(stream)`.
    pub fn dump(&self) -> String {
        let mut s = format!("math_Vector of Length = {}\n", self.length());
        for (k, x) in self.data.iter().enumerate() {
            s.push_str(&format!("math_Vector({}) = {}\n", self.lower.wrapping_add(k as i32), x));
        }
        s
    }
}

impl VectorBase<f64> {
    /// `Multiplied(matrix)`: the row vector times the matrix, bounds `LowerCol..=UpperCol`.
    pub fn multiplied_matrix(&self, m: &Matrix) -> Result<Self, MathError> {
        if self.data.len() != m.rows {
            return Err(MathError::Dimension);
        }
        let mut result = Self::new(m.lower_col, m.upper_col())?;
        for (c, slot) in result.data.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (r, x) in self.data.iter().enumerate() {
                acc += x * m.at0(r, c);
            }
            *slot = acc;
        }
        Ok(result)
    }
    /// `Multiply(left, right)`: `self = left (vector) * right (matrix)`.
    pub fn multiply_matrix_from(&mut self, left: &Self, right: &Matrix) -> Result<(), MathError> {
        if self.data.len() != right.cols || left.data.len() != right.rows {
            return Err(MathError::Dimension);
        }
        for (c, slot) in self.data.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (r, x) in left.data.iter().enumerate() {
                acc += x * right.at0(r, c);
            }
            *slot = acc;
        }
        Ok(())
    }
    /// `Multiply(left, right)`: `self = left (matrix) * right (vector)`.
    pub fn multiply_from_matrix(&mut self, left: &Matrix, right: &Self) -> Result<(), MathError> {
        if self.data.len() != left.rows || left.cols != right.data.len() {
            return Err(MathError::Dimension);
        }
        for (r, slot) in self.data.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (c, y) in right.data.iter().enumerate() {
                acc += left.at0(r, c) * y;
            }
            *slot = acc;
        }
        Ok(())
    }
    /// `TMultiply(theLeft, theTRight)`: `self = theTRight * theLeft` for a vector `theLeft` and a
    /// matrix `theTRight` (`result[r] = sum_c theLeft[c] * theTRight(r, c)`).
    pub fn t_multiply_from(&mut self, left: &Self, t_right: &Matrix) -> Result<(), MathError> {
        // result[r] = sum_c left[c] * tRight(r, c)
        if self.data.len() != t_right.rows || left.data.len() != t_right.cols {
            return Err(MathError::Dimension);
        }
        for (r, slot) in self.data.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (c, x) in left.data.iter().enumerate() {
                acc += x * t_right.at0(r, c);
            }
            *slot = acc;
        }
        Ok(())
    }
    /// `TMultiply(theTLeft, theRight)`: `self = theTLeft^T * theRight` with a matrix and a vector.
    pub fn t_multiply_matrix_from(&mut self, t_left: &Matrix, right: &Self) -> Result<(), MathError> {
        // result[c] = sum_r tLeft(r, c) * right[r]
        if self.data.len() != t_left.cols || t_left.rows != right.data.len() {
            return Err(MathError::Dimension);
        }
        for (c, slot) in self.data.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (r, y) in right.data.iter().enumerate() {
                acc += t_left.at0(r, c) * y;
            }
            *slot = acc;
        }
        Ok(())
    }
}
