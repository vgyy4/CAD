//! gp_Pnt
use super::{Ax1, Ax2, ConstructionError, Trsf, TrsfForm, Vec, XYZ};

/// gp_Pnt
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pnt(pub(crate) XYZ);

/// Things a point can be mirrored about: a point, an axis or a plane (an `Ax2`).
pub trait PntMirror {
    /// Mirrors `p` about `self` (gp_Pnt::Mirror).
    fn mirror_pnt(&self, p: &mut Pnt);
}

impl PntMirror for Pnt {
    // gp_Pnt::Mirror(gp_Pnt)
    fn mirror_pnt(&self, p: &mut Pnt) {
        p.0.reverse();
        let mut xyz = self.0;
        xyz.multiply(2.0);
        p.0.add(&xyz);
    }
}

impl PntMirror for Ax1 {
    // gp_Pnt::Mirror(gp_Ax1)
    fn mirror_pnt(&self, p: &mut Pnt) {
        let mut t = Trsf::new();
        t.set_mirror(self);
        t.transforms_xyz(&mut p.0);
    }
}

impl PntMirror for Ax2 {
    // gp_Pnt::Mirror(gp_Ax2)
    fn mirror_pnt(&self, p: &mut Pnt) {
        let mut t = Trsf::new();
        t.set_mirror(self);
        t.transforms_xyz(&mut p.0);
    }
}

impl Pnt {
    // gp_Pnt::gp_Pnt(x, y, z)
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self(XYZ::new(x, y, z))
    }
    // gp_Pnt::gp_Pnt(gp_XYZ)
    pub fn from_xyz(xyz: XYZ) -> Self {
        Self(xyz)
    }
    // gp_Pnt::gp_Pnt()
    pub fn origin() -> Self {
        Self::new(0., 0., 0.)
    }
    // gp_Pnt::X
    pub fn x(&self) -> f64 {
        self.0.x()
    }
    // gp_Pnt::Y
    pub fn y(&self) -> f64 {
        self.0.y()
    }
    // gp_Pnt::Z
    pub fn z(&self) -> f64 {
        self.0.z()
    }
    // gp_Pnt::Coord(x, y, z)
    pub fn coords(&self) -> (f64, f64, f64) {
        self.0.coords()
    }
    // gp_Pnt::Coord(index)
    pub fn coord(&self, index: i32) -> Result<f64, ConstructionError> {
        self.0.coord(index)
    }
    // gp_Pnt::XYZ
    pub fn xyz(&self) -> &XYZ {
        &self.0
    }
    // gp_Pnt::ChangeCoord
    pub fn change_coord(&mut self) -> &mut XYZ {
        &mut self.0
    }
    // gp_Pnt::SetCoord(index, value)
    pub fn set_coord_at(&mut self, index: i32, value: f64) -> Result<(), ConstructionError> {
        self.0.set_coord_at(index, value)
    }
    // gp_Pnt::SetCoord(x, y, z)
    pub fn set_coord(&mut self, x: f64, y: f64, z: f64) {
        self.0.set_coord(x, y, z);
    }
    // gp_Pnt::SetX
    pub fn set_x(&mut self, x: f64) {
        self.0.set_x(x);
    }
    // gp_Pnt::SetY
    pub fn set_y(&mut self, y: f64) {
        self.0.set_y(y);
    }
    // gp_Pnt::SetZ
    pub fn set_z(&mut self, z: f64) {
        self.0.set_z(z);
    }
    // gp_Pnt::SetXYZ
    pub fn set_xyz(&mut self, xyz: XYZ) {
        self.0 = xyz;
    }
    // gp_Pnt::BaryCenter
    pub fn bary_center(&mut self, alpha: f64, p: &Pnt, beta: f64) {
        let c = self.0;
        self.0.set_linear_form_2(alpha, &c, beta, &p.0);
        self.0.divide(alpha + beta);
    }
    // gp_Pnt::IsEqual
    pub fn is_equal(&self, other: &Pnt, linear_tolerance: f64) -> bool {
        self.distance(other) <= linear_tolerance
    }
    // gp_Pnt::Distance
    pub fn distance(&self, other: &Pnt) -> f64 {
        self.square_distance(other).sqrt()
    }
    // gp_Pnt::SquareDistance
    pub fn square_distance(&self, other: &Pnt) -> f64 {
        let dx = self.0.x() - other.0.x();
        let dy = self.0.y() - other.0.y();
        let dz = self.0.z() - other.0.z();
        dx * dx + dy * dy + dz * dz
    }
    // gp_Pnt::Mirror
    pub fn mirror<M: PntMirror>(&mut self, m: &M) {
        m.mirror_pnt(self);
    }
    // gp_Pnt::Mirrored
    pub fn mirrored<M: PntMirror>(&self, m: &M) -> Pnt {
        let mut p = *self;
        m.mirror_pnt(&mut p);
        p
    }
    // gp_Pnt::Rotate
    pub fn rotate(&mut self, a1: &Ax1, ang: f64) {
        let mut t = Trsf::new();
        // The axis direction is a unit vector: SetRotation cannot fail.
        if t.set_rotation(a1, ang).is_ok() {
            t.transforms_xyz(&mut self.0);
        }
    }
    // gp_Pnt::Rotated
    pub fn rotated(&self, a1: &Ax1, ang: f64) -> Pnt {
        let mut p = *self;
        p.rotate(a1, ang);
        p
    }
    // gp_Pnt::Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        let mut xyz = p.0;
        xyz.multiply(1.0 - s);
        self.0.multiply(s);
        self.0.add(&xyz);
    }
    // gp_Pnt::Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Pnt {
        let mut r = *self;
        r.scale(p, s);
        r
    }
    // gp_Pnt::Transform
    pub fn transform(&mut self, t: &Trsf) {
        match t.form() {
            TrsfForm::Identity => {}
            TrsfForm::Translation => self.0.add(t.translation_part()),
            TrsfForm::Scale => {
                self.0.multiply(t.scale_factor());
                self.0.add(t.translation_part());
            }
            TrsfForm::PntMirror => {
                self.0.reverse();
                self.0.add(t.translation_part());
            }
            _ => t.transforms_xyz(&mut self.0),
        }
    }
    // gp_Pnt::Transformed
    pub fn transformed(&self, t: &Trsf) -> Pnt {
        let mut p = *self;
        p.transform(t);
        p
    }
    // gp_Pnt::Translate(gp_Vec)
    pub fn translate(&mut self, v: &Vec) {
        self.0.add(v.xyz());
    }
    // gp_Pnt::Translated(gp_Vec)
    pub fn translated(&self, v: &Vec) -> Pnt {
        let mut p = *self;
        p.0.add(v.xyz());
        p
    }
    // gp_Pnt::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt, p2: &Pnt) {
        self.0.add(&p2.0);
        self.0.subtract(&p1.0);
    }
    // gp_Pnt::Translated(P1, P2)
    pub fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> Pnt {
        let mut p = *self;
        p.translate_points(p1, p2);
        p
    }
}
