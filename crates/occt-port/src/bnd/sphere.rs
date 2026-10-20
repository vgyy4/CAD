use crate::gp::XYZ;

#[derive(Debug, Clone)]
pub struct BndSphere {
    center: XYZ,
    radius: f64,
    valid: bool,
    u: i32,
    v: i32,
}
impl BndSphere {
    pub fn new() -> Self {
        Self { center: XYZ::zero(), radius: 0.0, valid: false, u: 0, v: 0 }
    }
    pub fn with_center(center: &XYZ, radius: f64, u: i32, v: i32) -> Self {
        Self { center: *center, radius, valid: false, u, v }
    }
    pub fn u(&self) -> i32 {
        self.u
    }
    pub fn v(&self) -> i32 {
        self.v
    }
    pub fn is_valid(&self) -> bool {
        self.valid
    }
    pub fn set_valid(&mut self, valid: bool) {
        self.valid = valid;
    }
    pub fn center(&self) -> &XYZ {
        &self.center
    }
    pub fn radius(&self) -> f64 {
        self.radius
    }
    pub fn square_distances(&self, p: &XYZ) -> (f64, f64) {
        let max = p.subtracted(&self.center).square_modulus();
        let rsq = self.radius * self.radius;
        (if max < rsq { 0.0 } else { max - rsq }, max + rsq)
    }
    pub fn distances(&self, p: &XYZ) -> (f64, f64) {
        let max = p.subtracted(&self.center).modulus();
        (if max - self.radius < 0.0 { 0.0 } else { max - self.radius }, max + self.radius)
    }
    pub fn project(&self, node: &XYZ) -> (bool, XYZ, f64, bool) {
        (true, self.center, node.subtracted(&self.center).modulus(), true)
    }
    pub fn distance(&self, node: &XYZ) -> f64 {
        node.subtracted(&self.center).modulus()
    }
    pub fn square_distance(&self, node: &XYZ) -> f64 {
        node.subtracted(&self.center).square_modulus()
    }
    pub fn add(&mut self, other: &Self) {
        if self.radius < 0.0 {
            *self = other.clone();
            return;
        }
        let dist = self.center.subtracted(&other.center).modulus();
        if self.radius + dist <= other.radius {
            *self = other.clone();
            return;
        }
        if other.radius + dist <= self.radius {
            return;
        }
        let rad = (dist + self.radius + other.radius) * 0.5;
        let param = (rad - self.radius) / dist;
        self.center = self.center.multiplied(1.0 - param).added(&other.center.multiplied(param));
        self.radius = rad;
        self.valid = false;
    }
    pub fn is_out_sphere(&self, other: &Self) -> bool {
        self.center.subtracted(&other.center).square_modulus()
            > (self.radius + other.radius) * (self.radius + other.radius)
    }
    pub fn is_out_point(&self, p: &XYZ, max_dist: f64) -> (bool, f64) {
        let (min, max) = self.distances(p);
        if min > max_dist {
            return (true, max_dist);
        }
        (false, if self.valid && max < max_dist { max } else { max_dist })
    }
    pub fn square_extent(&self) -> f64 {
        4.0 * self.radius * self.radius
    }
}
impl Default for BndSphere {
    fn default() -> Self {
        Self::new()
    }
}
