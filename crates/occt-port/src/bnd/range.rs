use crate::mathlin::MathError;

#[derive(Debug, Clone, PartialEq)]
pub struct BndRange {
    first: f64,
    last: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntersectStatus {
    Out = 0,
    In = 1,
    Boundary = 2,
}

impl BndRange {
    pub fn new() -> Self {
        Self { first: 0.0, last: -1.0 }
    }
    pub fn from_bounds(min: f64, max: f64) -> Result<Self, MathError> {
        if max < min { Err(MathError::Construction) } else { Ok(Self { first: min, last: max }) }
    }
    pub fn common(&mut self, other: &Self) {
        if other.is_void() {
            self.set_void();
            return;
        }
        if self.is_void() {
            return;
        }
        self.first = if self.first < other.first { other.first } else { self.first };
        self.last = if other.last < self.last { other.last } else { self.last };
    }
    pub fn union(&mut self, other: &Self) -> bool {
        if self.is_void() || other.is_void() || self.last < other.first || self.first > other.last {
            return false;
        }
        self.first = if other.first < self.first { other.first } else { self.first };
        self.last = if self.last < other.last { other.last } else { self.last };
        true
    }
    pub fn is_intersected(&self, val: f64, period: f64) -> IntersectStatus {
        if self.is_void() {
            return IntersectStatus::Out;
        }
        let period = period.abs();
        let df = self.first - val;
        let dl = self.last - val;
        if period <= f64::MIN_POSITIVE {
            let delta = df * dl;
            if delta.abs() < f64::MIN_POSITIVE {
                return IntersectStatus::Boundary;
            }
            if delta > 0.0 {
                return IntersectStatus::Out;
            }
            return IntersectStatus::In;
        }
        let val1 = df / period;
        let val2 = dl / period;
        let par1 = val1.floor() as i32;
        let par2 = val2.floor() as i32;
        if par1 != par2 {
            if (val2 - par2 as f64).abs() < f64::MIN_POSITIVE {
                return IntersectStatus::Boundary;
            }
            return IntersectStatus::In;
        }
        if (val1 - par1 as f64).abs() < f64::MIN_POSITIVE { IntersectStatus::Boundary } else { IntersectStatus::Out }
    }
    pub fn split(&self, val: f64, period: f64) -> Vec<Self> {
        let mut result = Vec::new();
        let p = period.abs();
        if self.is_intersected(val, p) != IntersectStatus::In {
            result.push(self.clone());
            return result;
        }
        if p == 0.0 {
            result.push(Self { first: self.first, last: val });
            result.push(Self { first: val, last: self.last });
            return result;
        }
        let mut prev = val + p * ((self.first - val) / p).ceil();
        if prev > self.first {
            result.push(Self { first: self.first, last: prev });
        }
        let mut cur = prev + p;
        while cur <= self.last {
            result.push(Self { first: prev, last: cur });
            prev = cur;
            cur += p;
        }
        if prev < self.last {
            result.push(Self { first: prev, last: self.last });
        }
        result
    }
    pub fn add_parameter(&mut self, p: f64) {
        if self.is_void() {
            self.first = p;
            self.last = p;
            return;
        }
        self.first = if p < self.first { p } else { self.first };
        self.last = if self.last < p { p } else { self.last };
    }
    pub fn add_range(&mut self, other: &Self) {
        if other.is_void() {
            return;
        }
        if self.is_void() {
            *self = other.clone();
            return;
        }
        self.first = if other.first < self.first { other.first } else { self.first };
        self.last = if self.last < other.last { other.last } else { self.last };
    }
    pub fn get_min(&self) -> Option<f64> {
        if self.is_void() { None } else { Some(self.first) }
    }
    pub fn get_max(&self) -> Option<f64> {
        if self.is_void() { None } else { Some(self.last) }
    }
    pub fn get_bounds(&self) -> Option<(f64, f64)> {
        if self.is_void() { None } else { Some((self.first, self.last)) }
    }
    pub fn get(&self) -> Option<(f64, f64)> {
        self.get_bounds()
    }
    pub fn get_intermediate_point(&self, lambda: f64) -> Option<f64> {
        if self.is_void() { None } else { Some(self.first + lambda * (self.last - self.first)) }
    }
    pub fn center(&self) -> Option<f64> {
        if self.is_void() { None } else { Some(0.5 * (self.first + self.last)) }
    }
    pub fn delta(&self) -> f64 {
        self.last - self.first
    }
    pub fn is_void(&self) -> bool {
        self.last < self.first
    }
    pub fn set_void(&mut self) {
        self.last = -1.0;
        self.first = 0.0;
    }
    pub fn enlarge(&mut self, delta: f64) {
        if !self.is_void() {
            self.first -= delta;
            self.last += delta;
        }
    }
    pub fn shifted(&self, val: f64) -> Self {
        if self.is_void() { Self::new() } else { Self { first: self.first + val, last: self.last + val } }
    }
    pub fn shift(&mut self, val: f64) {
        if !self.is_void() {
            self.first += val;
            self.last += val;
        }
    }
    pub fn trim_from(&mut self, lower: f64) {
        if !self.is_void() {
            self.first = if self.first < lower { lower } else { self.first };
        }
    }
    pub fn trim_to(&mut self, upper: f64) {
        if !self.is_void() {
            self.last = if upper < self.last { upper } else { self.last };
        }
    }
    pub fn is_out_value(&self, v: f64) -> bool {
        self.is_void() || v < self.first || v > self.last
    }
    pub fn is_out_range(&self, r: &Self) -> bool {
        self.is_void() || r.is_void() || r.last < self.first || r.first > self.last
    }
    pub fn contains(&self, v: f64) -> bool {
        !self.is_out_value(v)
    }
    pub fn intersects(&self, r: &Self) -> bool {
        !self.is_out_range(r)
    }
    pub fn min(&self) -> Option<f64> {
        self.get_min()
    }
    pub fn max(&self) -> Option<f64> {
        self.get_max()
    }
}
impl Default for BndRange {
    fn default() -> Self {
        Self::new()
    }
}
