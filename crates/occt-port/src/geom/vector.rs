//! `Geom_Direction` and `Geom_VectorWithMagnitude`.
use super::{GeomError, Geometry, Result, Vector, geometry_any};
use crate::gp::{self, Dir, Pnt, Trsf};

/// `Geom_Direction`.
#[derive(Debug, Clone)]
pub struct Direction {
    gp_vec: gp::Vec,
}

impl Direction {
    /// `Geom_Direction(X, Y, Z)` (a vector of length `<= gp::Resolution()` is unchecked in OCCT 8.0.1: an error here).
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self> {
        let d = (x * x + y * y + z * z).sqrt();
        if d <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        Ok(Direction { gp_vec: gp::Vec::new(x / d, y / d, z / d) })
    }
    /// `Geom_Direction(gp_Dir)`.
    pub fn from_dir(d: Dir) -> Self {
        Direction { gp_vec: gp::Vec::from_dir(&d) }
    }
    /// `SetCoord`.
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) -> Result<()> {
        let d = (x * x + y * y + z * z).sqrt();
        if d <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        self.gp_vec = gp::Vec::new(x / d, y / d, z / d);
        Ok(())
    }
    /// `SetDir`.
    pub fn set_dir(&mut self, d: Dir) {
        self.gp_vec = gp::Vec::from_dir(&d);
    }
    /// `SetX`.
    pub fn set_x(&mut self, x: f64) -> Result<()> {
        let (_, y, z) = self.gp_vec.coords();
        let d = (x * x + y * y + z * z).sqrt();
        if d <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        self.gp_vec = gp::Vec::new(x / d, y / d, z / d);
        Ok(())
    }
    /// `SetY`.
    pub fn set_y(&mut self, y: f64) -> Result<()> {
        let (x, _, z) = self.gp_vec.coords();
        let d = (x * x + y * y + z * z).sqrt();
        if d <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        self.gp_vec = gp::Vec::new(x / d, y / d, z / d);
        Ok(())
    }
    /// `SetZ`.
    pub fn set_z(&mut self, z: f64) -> Result<()> {
        let (x, y, _) = self.gp_vec.coords();
        let d = (x * x + y * y + z * z).sqrt();
        if d <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        self.gp_vec = gp::Vec::new(x / d, y / d, z / d);
        Ok(())
    }
    /// `Dir`: the stored vector as a direction (`gp_Dir(gp_Vec)` normalizes again).
    pub fn dir(&self) -> Dir {
        Dir::from_vec(&self.gp_vec).unwrap_or_default()
    }
}

impl Geometry for Direction {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        let mut dir = self.dir();
        if dir.transform(t).is_ok() {
            self.gp_vec = gp::Vec::from_dir(&dir);
        }
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(Direction::from_dir(self.dir()))
    }
}

impl Vector for Direction {
    fn vec(&self) -> gp::Vec {
        self.gp_vec
    }
    fn set_vec_raw(&mut self, v: gp::Vec) {
        self.gp_vec = v;
    }
    fn copy_vector(&self) -> Box<dyn Vector> {
        Box::new(Direction::from_dir(self.dir()))
    }
    fn magnitude(&self) -> f64 {
        1.0
    }
    fn square_magnitude(&self) -> f64 {
        1.0
    }
    fn cross(&mut self, other: &dyn Vector) -> Result<()> {
        let v = Dir::from_vec(&self.gp_vec.crossed(&other.vec()))?;
        self.gp_vec = gp::Vec::from_dir(&v);
        Ok(())
    }
    fn crossed(&self, other: &dyn Vector) -> Result<Box<dyn Vector>> {
        let v = Dir::from_vec(&self.gp_vec.crossed(&other.vec()))?;
        Ok(Box::new(Direction::from_dir(v)))
    }
    fn cross_cross(&mut self, v1: &dyn Vector, v2: &dyn Vector) -> Result<()> {
        let v = Dir::from_vec(&self.gp_vec.cross_crossed(&v1.vec(), &v2.vec()))?;
        self.gp_vec = gp::Vec::from_dir(&v);
        Ok(())
    }
    fn cross_crossed(&self, v1: &dyn Vector, v2: &dyn Vector) -> Result<Box<dyn Vector>> {
        let v = Dir::from_vec(&self.gp_vec.cross_crossed(&v1.vec(), &v2.vec()))?;
        Ok(Box::new(Direction::from_dir(v)))
    }
}

/// `Geom_VectorWithMagnitude`.
#[derive(Debug, Clone)]
pub struct VectorWithMagnitude {
    gp_vec: gp::Vec,
}

impl VectorWithMagnitude {
    /// `Geom_VectorWithMagnitude(gp_Vec)`.
    pub fn from_vec(v: gp::Vec) -> Self {
        VectorWithMagnitude { gp_vec: v }
    }
    /// `Geom_VectorWithMagnitude(X, Y, Z)`.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        VectorWithMagnitude { gp_vec: gp::Vec::new(x, y, z) }
    }
    /// `Geom_VectorWithMagnitude(P1, P2)`.
    pub fn from_points(p1: Pnt, p2: Pnt) -> Self {
        VectorWithMagnitude { gp_vec: gp::Vec::between(&p1, &p2) }
    }
    /// `SetCoord`.
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) {
        self.gp_vec = gp::Vec::new(x, y, z);
    }
    /// `SetVec`.
    pub fn set_vec(&mut self, v: gp::Vec) {
        self.gp_vec = v;
    }
    /// `SetX`.
    pub fn set_x(&mut self, x: f64) {
        self.gp_vec.set_x(x);
    }
    /// `SetY`.
    pub fn set_y(&mut self, y: f64) {
        self.gp_vec.set_y(y);
    }
    /// `SetZ`.
    pub fn set_z(&mut self, z: f64) {
        self.gp_vec.set_z(z);
    }
    /// `Add`.
    pub fn add(&mut self, other: &dyn Vector) {
        self.gp_vec.add(&other.vec());
    }
    /// `Added`.
    pub fn added(&self, other: &dyn Vector) -> VectorWithMagnitude {
        let mut v1 = self.gp_vec;
        v1.add(&other.vec());
        VectorWithMagnitude::from_vec(v1)
    }
    /// `Subtract`.
    pub fn subtract(&mut self, other: &dyn Vector) {
        self.gp_vec.subtract(&other.vec());
    }
    /// `Subtracted`.
    pub fn subtracted(&self, other: &dyn Vector) -> VectorWithMagnitude {
        let mut v = self.gp_vec;
        v.subtract(&other.vec());
        VectorWithMagnitude::from_vec(v)
    }
    /// `Multiply`.
    pub fn multiply(&mut self, s: f64) {
        self.gp_vec.multiply(s);
    }
    /// `Multiplied`.
    pub fn multiplied(&self, s: f64) -> VectorWithMagnitude {
        let mut v = self.gp_vec;
        v.multiply(s);
        VectorWithMagnitude::from_vec(v)
    }
    /// `Divide` (a divisor of magnitude `<= gp::Resolution()` is unchecked in OCCT: an error here).
    pub fn divide(&mut self, s: f64) -> Result<()> {
        if s.abs() <= gp::resolution() {
            return Err(GeomError::Argument);
        }
        self.gp_vec.divide(s);
        Ok(())
    }
    /// `Divided`.
    pub fn divided(&self, s: f64) -> Result<VectorWithMagnitude> {
        if s.abs() <= gp::resolution() {
            return Err(GeomError::Argument);
        }
        let mut v = self.gp_vec;
        v.divide(s);
        Ok(VectorWithMagnitude::from_vec(v))
    }
    /// `Normalize`.
    pub fn normalize(&mut self) -> Result<()> {
        self.gp_vec.normalize()?;
        Ok(())
    }
    /// `Normalized`.
    pub fn normalized(&self) -> Result<VectorWithMagnitude> {
        let mut v = self.gp_vec;
        v.normalize()?;
        Ok(VectorWithMagnitude::from_vec(v))
    }
}

impl Geometry for VectorWithMagnitude {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        let _ = self.gp_vec.transform(t);
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(VectorWithMagnitude::from_vec(self.gp_vec))
    }
}

impl Vector for VectorWithMagnitude {
    fn vec(&self) -> gp::Vec {
        self.gp_vec
    }
    fn set_vec_raw(&mut self, v: gp::Vec) {
        self.gp_vec = v;
    }
    fn copy_vector(&self) -> Box<dyn Vector> {
        Box::new(VectorWithMagnitude::from_vec(self.gp_vec))
    }
    fn magnitude(&self) -> f64 {
        self.gp_vec.magnitude()
    }
    fn square_magnitude(&self) -> f64 {
        self.gp_vec.square_magnitude()
    }
    fn cross(&mut self, other: &dyn Vector) -> Result<()> {
        self.gp_vec.cross(&other.vec());
        Ok(())
    }
    fn crossed(&self, other: &dyn Vector) -> Result<Box<dyn Vector>> {
        let mut v = self.gp_vec;
        v.cross(&other.vec());
        Ok(Box::new(VectorWithMagnitude::from_vec(v)))
    }
    fn cross_cross(&mut self, v1: &dyn Vector, v2: &dyn Vector) -> Result<()> {
        self.gp_vec.cross_cross(&v1.vec(), &v2.vec());
        Ok(())
    }
    fn cross_crossed(&self, v1: &dyn Vector, v2: &dyn Vector) -> Result<Box<dyn Vector>> {
        let mut v = self.gp_vec;
        v.cross_cross(&v1.vec(), &v2.vec());
        Ok(Box::new(VectorWithMagnitude::from_vec(v)))
    }
}
