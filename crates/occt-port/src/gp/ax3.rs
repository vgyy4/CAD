//! gp_Ax3
use super::ax2::cross_or;
use super::{
    ANGULAR, Ax1, Ax2, ConstructionError, Dir, Direction, Pnt, Trsf, Vec, cross_standard_dir, perpendicular_x,
    perpendicular_y,
};

/// gp_Ax3: a coordinate system that may be left-handed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ax3 {
    pub(crate) loc: Pnt,
    pub(crate) dir: Dir,
    pub(crate) x: Dir,
    pub(crate) y: Dir,
}

impl Default for Ax3 {
    // gp_Ax3::gp_Ax3()
    fn default() -> Self {
        Self { loc: Pnt::new(0., 0., 0.), dir: Dir::default_z(), x: Dir::default_x(), y: Dir::default_y() }
    }
}

/// Things an `Ax3` can be mirrored about: a point, an axis or a plane (an `Ax2`).
pub trait Ax3Mirror {
    /// Mirrors `a` about `self` (gp_Ax3::Mirror).
    fn mirror_ax3(&self, a: &mut Ax3);
}

impl Ax3Mirror for Pnt {
    // gp_Ax3::Mirror(gp_Pnt)
    fn mirror_ax3(&self, a: &mut Ax3) {
        a.loc.mirror(self);
        a.dir.reverse();
        a.x.reverse();
        a.y.reverse();
    }
}

impl Ax3Mirror for Ax1 {
    // gp_Ax3::Mirror(gp_Ax1)
    fn mirror_ax3(&self, a: &mut Ax3) {
        a.y.mirror(self);
        a.x.mirror(self);
        let mut axis = a.axis();
        axis.mirror(self);
        a.loc = axis.loc;
        a.dir = axis.dir;
    }
}

impl Ax3Mirror for Ax2 {
    // gp_Ax3::Mirror(gp_Ax2)
    fn mirror_ax3(&self, a: &mut Ax3) {
        a.y.mirror(self);
        a.x.mirror(self);
        let mut axis = a.axis();
        axis.mirror(self);
        a.loc = axis.loc;
        a.dir = axis.dir;
    }
}

/// Things an `Ax3` can be tested for coplanarity with: another `Ax3` or an axis.
pub trait Ax3Coplanar {
    /// gp_Ax3::IsCoplanar
    fn coplanar_with_ax3(&self, a: &Ax3, linear_tolerance: f64, angular_tolerance: f64) -> bool;
}

impl Ax3Coplanar for Ax3 {
    fn coplanar_with_ax3(&self, a: &Ax3, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        let vec = Vec::between(&a.loc, &self.loc);
        let mut d1 = Vec::from_dir(&a.dir).dot(&vec);
        if d1 < 0.0 {
            d1 = -d1;
        }
        let mut d2 = Vec::from_dir(&self.dir).dot(&vec);
        if d2 < 0.0 {
            d2 = -d2;
        }
        d1 <= linear_tolerance && d2 <= linear_tolerance && a.axis().is_parallel(&self.axis(), angular_tolerance)
    }
}

impl Ax3Coplanar for Ax1 {
    fn coplanar_with_ax3(&self, a: &Ax3, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        let vec = Vec::between(&a.loc, &self.loc);
        let mut d1 = Vec::from_dir(&a.dir).dot(&vec);
        if d1 < 0.0 {
            d1 = -d1;
        }
        let mut d2 = Vec::from_dir(&self.dir).crossed(&vec).magnitude();
        if d2 < 0.0 {
            d2 = -d2;
        }
        d1 <= linear_tolerance && d2 <= linear_tolerance && a.axis().is_normal(self, angular_tolerance)
    }
}

impl Ax3 {
    // gp_Ax3::gp_Ax3(gp_Ax2)
    pub fn from_ax2(a: &Ax2) -> Self {
        Self { loc: a.loc, dir: a.dir, x: a.x, y: a.y }
    }
    // gp_Ax3::gp_Ax3(P, N, Vx)
    pub fn new(p: Pnt, n: Dir, vx: Dir) -> Result<Self, ConstructionError> {
        let mut x = n;
        x.cross_cross(&vx, &n)?;
        let mut y = n;
        y.cross(&x)?;
        Ok(Self { loc: p, dir: n, x, y })
    }
    // gp_Ax3::gp_Ax3(P, gp_Dir::D N, gp_Dir::D Vx)
    pub fn from_standard_directions(p: Pnt, n: Direction, vx: Direction) -> Self {
        Self {
            loc: p,
            dir: Dir::from_direction(n),
            y: Dir::from_direction(cross_standard_dir(n, vx)),
            x: Dir::from_direction(vx),
        }
    }
    // gp_Ax3::gp_Ax3(P, V)
    pub fn from_direction(p: Pnt, v: Dir) -> Self {
        let (a, b, c) = v.coords();
        let (aabs, babs, cabs) = (a.abs(), b.abs(), c.abs());
        let (dx, dy, dz) = if babs <= aabs && babs <= cabs {
            if aabs > cabs { (-c, 0., a) } else { (c, 0., -a) }
        } else if aabs <= babs && aabs <= cabs {
            if babs > cabs { (0., -c, b) } else { (0., c, -b) }
        } else if aabs > babs {
            (-b, a, 0.)
        } else {
            (b, -a, 0.)
        };
        let x = Dir::new(dx, dy, dz).unwrap_or_default();
        let y = cross_or(&v, &x, Dir::default_y());
        Self { loc: p, dir: v, x, y }
    }
    // gp_Ax3::gp_Ax3(P, gp_Dir::D V)
    pub fn from_point_standard(p: Pnt, v: Direction) -> Self {
        Self {
            loc: p,
            dir: Dir::from_direction(v),
            y: Dir::from_direction(perpendicular_y(v)),
            x: Dir::from_direction(perpendicular_x(v)),
        }
    }
    // gp_Ax3::gp_Ax3(gp_Dir::D V)
    pub fn from_standard_direction(v: Direction) -> Self {
        Self::from_point_standard(Pnt::new(0., 0., 0.), v)
    }
    // gp_Ax3::XReverse
    pub fn x_reverse(&mut self) {
        self.x.reverse();
    }
    // gp_Ax3::YReverse
    pub fn y_reverse(&mut self) {
        self.y.reverse();
    }
    // gp_Ax3::ZReverse
    pub fn z_reverse(&mut self) {
        self.dir.reverse();
    }
    // gp_Ax3::SetAxis
    pub fn set_axis(&mut self, a1: Ax1) -> Result<(), ConstructionError> {
        self.loc = a1.loc;
        self.set_direction(a1.dir)
    }
    // gp_Ax3::SetDirection
    pub fn set_direction(&mut self, v: Dir) -> Result<(), ConstructionError> {
        let dot = v.dot(&self.x);
        if 1.0 - dot.abs() <= ANGULAR {
            if dot > 0.0 {
                self.x = self.y;
                self.y = self.dir;
            } else {
                self.x = self.dir;
            }
            self.dir = v;
        } else {
            let direct = self.direct();
            self.dir = v;
            self.x = v.cross_crossed(&self.x, &v)?;
            if direct {
                self.y = v.crossed(&self.x)?;
            } else {
                self.y = self.x.crossed(&v)?;
            }
        }
        Ok(())
    }
    // gp_Ax3::SetLocation
    pub fn set_location(&mut self, p: Pnt) {
        self.loc = p;
    }
    // gp_Ax3::SetXDirection
    pub fn set_x_direction(&mut self, vx: Dir) -> Result<(), ConstructionError> {
        let dot = vx.dot(&self.dir);
        if 1.0 - dot.abs() <= ANGULAR {
            if dot > 0.0 {
                self.dir = self.x;
                self.y = self.y.reversed();
            } else {
                self.dir = self.x;
            }
            self.x = vx;
        } else {
            let direct = self.direct();
            self.x = self.dir.cross_crossed(&vx, &self.dir)?;
            if direct {
                self.y = self.dir.crossed(&self.x)?;
            } else {
                self.y = self.x.crossed(&self.dir)?;
            }
        }
        Ok(())
    }
    // gp_Ax3::SetYDirection
    pub fn set_y_direction(&mut self, vy: Dir) -> Result<(), ConstructionError> {
        let dot = vy.dot(&self.dir);
        if 1.0 - dot.abs() <= ANGULAR {
            if dot > 0.0 {
                self.dir = self.y;
                self.x = self.x.reversed();
            } else {
                self.dir = self.y;
            }
            self.y = vy;
        } else {
            let is_direct = self.direct();
            self.x = vy.crossed(&self.dir)?;
            self.y = self.dir.crossed(&self.x)?;
            if !is_direct {
                self.x.reverse();
            }
        }
        Ok(())
    }
    // gp_Ax3::Angle
    pub fn angle(&self, other: &Ax3) -> f64 {
        self.axis().angle(&other.axis())
    }
    // gp_Ax3::Axis
    pub fn axis(&self) -> Ax1 {
        Ax1::new(self.loc, self.dir)
    }
    // gp_Ax3::Ax2
    pub fn ax2(&self) -> Ax2 {
        let mut zz = self.dir;
        if !self.direct() {
            zz.reverse();
        }
        Ax2::new(self.loc, zz, &self.x).unwrap_or_default()
    }
    // gp_Ax3::Direction
    pub fn direction(&self) -> &Dir {
        &self.dir
    }
    // gp_Ax3::Location
    pub fn location(&self) -> &Pnt {
        &self.loc
    }
    // gp_Ax3::XDirection
    pub fn x_direction(&self) -> &Dir {
        &self.x
    }
    // gp_Ax3::YDirection
    pub fn y_direction(&self) -> &Dir {
        &self.y
    }
    // gp_Ax3::Direct
    pub fn direct(&self) -> bool {
        matches!(self.x.crossed(&self.y), Ok(c) if c.dot(&self.dir) > 0.0)
    }
    // gp_Ax3::IsCoplanar (with another Ax3 or with an Ax1)
    pub fn is_coplanar<T: Ax3Coplanar>(&self, other: &T, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        other.coplanar_with_ax3(self, linear_tolerance, angular_tolerance)
    }
    // gp_Ax3::Mirror
    pub fn mirror<M: Ax3Mirror>(&mut self, m: &M) {
        m.mirror_ax3(self);
    }
    // gp_Ax3::Mirrored
    pub fn mirrored<M: Ax3Mirror>(&self, m: &M) -> Ax3 {
        let mut a = *self;
        m.mirror_ax3(&mut a);
        a
    }
    // gp_Ax3::Rotate
    pub fn rotate(&mut self, a1: &Ax1, ang: f64) {
        self.loc.rotate(a1, ang);
        self.dir.rotate(a1, ang);
        self.x.rotate(a1, ang);
        self.y.rotate(a1, ang);
    }
    // gp_Ax3::Rotated
    pub fn rotated(&self, a1: &Ax1, ang: f64) -> Ax3 {
        let mut a = *self;
        a.rotate(a1, ang);
        a
    }
    // gp_Ax3::Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.loc.scale(p, s);
        if s < 0.0 {
            self.dir.reverse();
            self.x.reverse();
            self.y.reverse();
        }
    }
    // gp_Ax3::Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Ax3 {
        let mut a = *self;
        a.scale(p, s);
        a
    }
    // gp_Ax3::Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.loc.transform(t);
        self.dir.transform_raw(t);
        self.x.transform_raw(t);
        self.y.transform_raw(t);
    }
    // gp_Ax3::Transformed
    pub fn transformed(&self, t: &Trsf) -> Ax3 {
        let mut a = *self;
        a.transform(t);
        a
    }
    // gp_Ax3::Translate(gp_Vec)
    pub fn translate(&mut self, v: &Vec) {
        self.loc.translate(v);
    }
    // gp_Ax3::Translated(gp_Vec)
    pub fn translated(&self, v: &Vec) -> Ax3 {
        let mut a = *self;
        a.loc.translate(v);
        a
    }
    // gp_Ax3::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt, p2: &Pnt) {
        self.translate(&Vec::between(p1, p2));
    }
    // gp_Ax3::Translated(P1, P2)
    pub fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> Ax3 {
        self.translated(&Vec::between(p1, p2))
    }
}
