//! gp_Ax22d
use super::{Ax2d, Dir2d, Pnt2d, Trsf2d, Vec2d};

/// gp_Ax22d: a right- or left-handed frame in the plane (location, X and Y directions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ax22d {
    point: Pnt2d,
    vydir: Dir2d,
    vxdir: Dir2d,
}

impl Default for Ax22d {
    // gp_Ax22d::gp_Ax22d()
    fn default() -> Self {
        Self { point: Pnt2d::default(), vydir: Dir2d::from_direction(super::Direction2d::Y), vxdir: Dir2d::default() }
    }
}

/// Things a 2D frame can be mirrored about: a point or an axis.
pub trait Ax22dMirror {
    /// Mirrors `a` about `self` (gp_Ax22d::Mirror).
    fn mirror_ax22d(&self, a: &mut Ax22d);
}

impl Ax22dMirror for Pnt2d {
    // gp_Ax22d::Mirror(gp_Pnt2d)
    fn mirror_ax22d(&self, a: &mut Ax22d) {
        let mut temp = a.point;
        temp.mirror(self);
        a.point = temp;
        a.vxdir.reverse();
        a.vydir.reverse();
    }
}

impl Ax22dMirror for Ax2d {
    // gp_Ax22d::Mirror(gp_Ax2d)
    fn mirror_ax22d(&self, a: &mut Ax22d) {
        a.vydir.mirror(self);
        a.vxdir.mirror(self);
        let mut temp = a.point;
        temp.mirror(self);
        a.point = temp;
    }
}

/// `gp_Dir2d::SetCoord(x, y)` on a vector derived from a unit direction: it cannot fail.
fn set_unit(d: &mut Dir2d, x: f64, y: f64) {
    if d.set_coord(x, y).is_err() {
        // unreachable for unit input; OCCT would raise Standard_ConstructionError
    }
}

impl Ax22d {
    // gp_Ax22d::gp_Ax22d(P, Vx, Vy)
    pub fn from_dirs(p: Pnt2d, vx: Dir2d, vy: Dir2d) -> Self {
        let mut a = Self { point: p, vydir: vy, vxdir: vx };
        let value = vx.crossed(&vy);
        if value >= 0.0 {
            set_unit(&mut a.vydir, -a.vxdir.y(), a.vxdir.x());
        } else {
            set_unit(&mut a.vydir, a.vxdir.y(), -a.vxdir.x());
        }
        a
    }
    // gp_Ax22d::gp_Ax22d(P, V, isSense)
    pub fn from_dir(p: Pnt2d, v: Dir2d, is_sense: bool) -> Self {
        let mut a = Self { point: p, vxdir: v, ..Self::default() };
        if is_sense {
            set_unit(&mut a.vydir, -v.y(), v.x());
        } else {
            set_unit(&mut a.vydir, v.y(), -v.x());
        }
        a
    }
    // gp_Ax22d::gp_Ax22d(A, isSense)
    pub fn from_ax2d(ax: Ax2d, is_sense: bool) -> Self {
        let mut a = Self { point: *ax.location(), vxdir: *ax.direction(), ..Self::default() };
        if is_sense {
            set_unit(&mut a.vydir, -a.vxdir.y(), a.vxdir.x());
        } else {
            set_unit(&mut a.vydir, a.vxdir.y(), -a.vxdir.x());
        }
        a
    }
    // gp_Ax22d::SetAxis
    pub fn set_axis(&mut self, a1: Ax22d) {
        self.point = a1.point;
        self.vxdir = a1.vxdir;
        self.vydir = a1.vydir;
    }
    // gp_Ax22d::SetXAxis
    pub fn set_x_axis(&mut self, a1: Ax2d) {
        let is_sign = self.vxdir.crossed(&self.vydir) >= 0.0;
        self.point = *a1.location();
        self.vxdir = *a1.direction();
        if is_sign {
            set_unit(&mut self.vydir, -self.vxdir.y(), self.vxdir.x());
        } else {
            set_unit(&mut self.vydir, self.vxdir.y(), -self.vxdir.x());
        }
    }
    // gp_Ax22d::SetYAxis
    pub fn set_y_axis(&mut self, a1: Ax2d) {
        let is_sign = self.vxdir.crossed(&self.vydir) >= 0.0;
        self.point = *a1.location();
        self.vydir = *a1.direction();
        if is_sign {
            set_unit(&mut self.vxdir, self.vydir.y(), -self.vydir.x());
        } else {
            set_unit(&mut self.vxdir, -self.vydir.y(), self.vydir.x());
        }
    }
    // gp_Ax22d::SetLocation
    pub fn set_location(&mut self, p: Pnt2d) {
        self.point = p;
    }
    // gp_Ax22d::SetXDirection
    pub fn set_x_direction(&mut self, vx: Dir2d) {
        let is_sign = self.vxdir.crossed(&self.vydir) >= 0.0;
        self.vxdir = vx;
        if is_sign {
            set_unit(&mut self.vydir, -vx.y(), vx.x());
        } else {
            set_unit(&mut self.vydir, vx.y(), -vx.x());
        }
    }
    // gp_Ax22d::SetYDirection
    pub fn set_y_direction(&mut self, vy: Dir2d) {
        let is_sign = self.vxdir.crossed(&self.vydir) >= 0.0;
        self.vydir = vy;
        if is_sign {
            set_unit(&mut self.vxdir, vy.y(), -vy.x());
        } else {
            set_unit(&mut self.vxdir, -vy.y(), vy.x());
        }
    }
    // gp_Ax22d::XAxis
    pub fn x_axis(&self) -> Ax2d {
        Ax2d::new(self.point, self.vxdir)
    }
    // gp_Ax22d::YAxis
    pub fn y_axis(&self) -> Ax2d {
        Ax2d::new(self.point, self.vydir)
    }
    // gp_Ax22d::Location
    pub fn location(&self) -> &Pnt2d {
        &self.point
    }
    // gp_Ax22d::XDirection
    pub fn x_direction(&self) -> &Dir2d {
        &self.vxdir
    }
    // gp_Ax22d::YDirection
    pub fn y_direction(&self) -> &Dir2d {
        &self.vydir
    }
    // gp_Ax22d::Mirror
    pub fn mirror<M: Ax22dMirror>(&mut self, m: &M) {
        m.mirror_ax22d(self);
    }
    // gp_Ax22d::Mirrored
    pub fn mirrored<M: Ax22dMirror>(&self, m: &M) -> Ax22d {
        let mut a = *self;
        m.mirror_ax22d(&mut a);
        a
    }
    // gp_Ax22d::Rotate
    pub fn rotate(&mut self, p: &Pnt2d, ang: f64) {
        let mut temp = self.point;
        temp.rotate(p, ang);
        self.point = temp;
        self.vxdir.rotate(ang);
        self.vydir.rotate(ang);
    }
    // gp_Ax22d::Rotated
    pub fn rotated(&self, p: &Pnt2d, ang: f64) -> Ax22d {
        let mut a = *self;
        a.rotate(p, ang);
        a
    }
    // gp_Ax22d::Scale
    pub fn scale(&mut self, p: &Pnt2d, s: f64) {
        let mut temp = self.point;
        temp.scale(p, s);
        self.point = temp;
        if s < 0.0 {
            self.vxdir.reverse();
            self.vydir.reverse();
        }
    }
    // gp_Ax22d::Scaled
    pub fn scaled(&self, p: &Pnt2d, s: f64) -> Ax22d {
        let mut a = *self;
        a.scale(p, s);
        a
    }
    // gp_Ax22d::Transform
    pub fn transform(&mut self, t: &Trsf2d) {
        let mut temp = self.point;
        temp.transform(t);
        self.point = temp;
        self.vxdir.transform(t);
        self.vydir.transform(t);
    }
    // gp_Ax22d::Transformed
    pub fn transformed(&self, t: &Trsf2d) -> Ax22d {
        let mut a = *self;
        a.transform(t);
        a
    }
    // gp_Ax22d::Translate(gp_Vec2d)
    pub fn translate(&mut self, v: &Vec2d) {
        self.point.translate(v);
    }
    // gp_Ax22d::Translated(gp_Vec2d)
    pub fn translated(&self, v: &Vec2d) -> Ax22d {
        let mut a = *self;
        a.translate(v);
        a
    }
    // gp_Ax22d::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt2d, p2: &Pnt2d) {
        self.point.translate_points(p1, p2);
    }
    // gp_Ax22d::Translated(P1, P2)
    pub fn translated_points(&self, p1: &Pnt2d, p2: &Pnt2d) -> Ax22d {
        let mut a = *self;
        a.translate_points(p1, p2);
        a
    }
}
