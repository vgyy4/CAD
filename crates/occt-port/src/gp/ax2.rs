//! gp_Ax2
use super::{
    ANGULAR, Ax1, ConstructionError, Dir, Direction, Pnt, Trsf, Vec, cross_standard_dir, perpendicular_x,
    perpendicular_y,
};

/// gp_Ax2: a right-handed coordinate system (location, main direction, X and Y directions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ax2 {
    pub(crate) loc: Pnt,
    pub(crate) dir: Dir,
    pub(crate) x: Dir,
    pub(crate) y: Dir,
}

impl Default for Ax2 {
    // gp_Ax2::gp_Ax2()
    fn default() -> Self {
        Self { loc: Pnt::new(0., 0., 0.), dir: Dir::default_z(), x: Dir::default_x(), y: Dir::default_y() }
    }
}

/// Things an `Ax2` can be mirrored about: a point, an axis or a plane (another `Ax2`).
pub trait Ax2Mirror {
    /// Mirrors `a` about `self` (gp_Ax2::Mirror).
    fn mirror_ax2(&self, a: &mut Ax2);
}

/// `vxdir.Crossed(vydir)` of gp_Ax2/gp_Ax3 where OCCT's call cannot fail for unit directions;
/// `fallback` is kept if it ever would.
pub(crate) fn cross_or(a: &Dir, b: &Dir, fallback: Dir) -> Dir {
    a.crossed(b).unwrap_or(fallback)
}

impl Ax2Mirror for Pnt {
    // gp_Ax2::Mirror(gp_Pnt)
    fn mirror_ax2(&self, a: &mut Ax2) {
        a.loc.mirror(self);
        a.x.reverse();
        a.y.reverse();
    }
}

impl Ax2Mirror for Ax1 {
    // gp_Ax2::Mirror(gp_Ax1)
    fn mirror_ax2(&self, a: &mut Ax2) {
        a.y.mirror(self);
        a.x.mirror(self);
        a.loc.mirror(self);
        a.dir = cross_or(&a.x, &a.y, a.dir);
    }
}

impl Ax2Mirror for Ax2 {
    // gp_Ax2::Mirror(gp_Ax2)
    fn mirror_ax2(&self, a: &mut Ax2) {
        a.y.mirror(self);
        a.x.mirror(self);
        a.loc.mirror(self);
        a.dir = cross_or(&a.x, &a.y, a.dir);
    }
}

/// Things an `Ax2` can be tested for coplanarity with: another `Ax2` or an axis.
pub trait Ax2Coplanar {
    /// gp_Ax2::IsCoplanar
    fn coplanar_with_ax2(&self, a: &Ax2, linear_tolerance: f64, angular_tolerance: f64) -> bool;
}

fn offset_along(a: &Ax2, p: &Pnt) -> f64 {
    let dd = &a.dir;
    let pp = &a.loc;
    let mut d1 = dd.x() * (p.x() - pp.x()) + dd.y() * (p.y() - pp.y()) + dd.z() * (p.z() - pp.z());
    if d1 < 0.0 {
        d1 = -d1;
    }
    d1
}

impl Ax2Coplanar for Ax2 {
    fn coplanar_with_ax2(&self, a: &Ax2, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        // `self` is the argument, `a` the object (see Ax2::is_coplanar).
        let d1 = offset_along(a, &self.loc);
        d1 <= linear_tolerance && a.axis().is_parallel(&self.axis(), angular_tolerance)
    }
}

impl Ax2Coplanar for Ax1 {
    fn coplanar_with_ax2(&self, a: &Ax2, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        let d1 = offset_along(a, &self.loc);
        d1 <= linear_tolerance && a.axis().is_normal(self, angular_tolerance)
    }
}

impl Ax2 {
    // gp_Ax2::gp_Ax2(P, N, Vx)
    pub fn new(p: Pnt, n: Dir, vx: &Dir) -> Result<Self, ConstructionError> {
        let mut x = n;
        x.cross_cross(vx, &n)?;
        let mut y = n;
        y.cross(&x)?;
        Ok(Self { loc: p, dir: n, x, y })
    }
    // gp_Ax2::gp_Ax2(P, gp_Dir::D N, gp_Dir::D Vx)
    pub fn from_standard_directions(p: Pnt, n: Direction, vx: Direction) -> Self {
        Self {
            loc: p,
            dir: Dir::from_direction(n),
            y: Dir::from_direction(cross_standard_dir(n, vx)),
            x: Dir::from_direction(vx),
        }
    }
    // gp_Ax2::gp_Ax2(P, V)
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
        let d = Dir::new(dx, dy, dz).unwrap_or_default();
        let mut r = Self { loc: p, dir: v, x: Dir::default_x(), y: Dir::default_x() };
        // Cannot fail: `d` is perpendicular to the unit direction `v`.
        let _ = r.set_x_direction(d);
        r
    }
    // gp_Ax2::gp_Ax2(P, gp_Dir::D V)
    pub fn from_point_standard(p: Pnt, v: Direction) -> Self {
        Self {
            loc: p,
            dir: Dir::from_direction(v),
            y: Dir::from_direction(perpendicular_y(v)),
            x: Dir::from_direction(perpendicular_x(v)),
        }
    }
    // gp_Ax2::gp_Ax2(gp_Dir::D V)
    pub fn from_standard_direction(v: Direction) -> Self {
        Self::from_point_standard(Pnt::new(0., 0., 0.), v)
    }
    // gp_Ax2::SetAxis
    pub fn set_axis(&mut self, a1: Ax1) -> Result<(), ConstructionError> {
        let a = a1.dir.dot(&self.x);
        if ((a.abs()) - 1.0).abs() <= ANGULAR {
            if a > 0.0 {
                self.x = self.y;
                self.y = self.dir;
                self.loc = a1.loc;
                self.dir = a1.dir;
            } else {
                self.x = self.dir;
                self.loc = a1.loc;
                self.dir = a1.dir;
            }
        } else {
            self.loc = a1.loc;
            self.dir = a1.dir;
            self.x = self.dir.cross_crossed(&self.x, &self.dir)?;
            self.y = self.dir.crossed(&self.x)?;
        }
        Ok(())
    }
    // gp_Ax2::SetDirection
    pub fn set_direction(&mut self, v: Dir) -> Result<(), ConstructionError> {
        let a = v.dot(&self.x);
        if ((a.abs()) - 1.0).abs() <= ANGULAR {
            if a > 0.0 {
                self.x = self.y;
                self.y = self.dir;
                self.dir = v;
            } else {
                self.x = self.dir;
                self.dir = v;
            }
        } else {
            self.dir = v;
            self.x = v.cross_crossed(&self.x, &v)?;
            self.y = v.crossed(&self.x)?;
        }
        Ok(())
    }
    // gp_Ax2::SetLocation
    pub fn set_location(&mut self, p: Pnt) {
        self.loc = p;
    }
    // gp_Ax2::SetXDirection
    pub fn set_x_direction(&mut self, vx: Dir) -> Result<(), ConstructionError> {
        self.x = self.dir.cross_crossed(&vx, &self.dir)?;
        self.y = self.dir.crossed(&self.x)?;
        Ok(())
    }
    // gp_Ax2::SetYDirection
    pub fn set_y_direction(&mut self, vy: Dir) -> Result<(), ConstructionError> {
        self.x = vy.crossed(&self.dir)?;
        self.y = self.dir.crossed(&self.x)?;
        Ok(())
    }
    // gp_Ax2::Angle
    pub fn angle(&self, other: &Ax2) -> f64 {
        self.axis().angle(&other.axis())
    }
    // gp_Ax2::Axis
    pub fn axis(&self) -> Ax1 {
        Ax1::new(self.loc, self.dir)
    }
    // gp_Ax2::Direction
    pub fn direction(&self) -> &Dir {
        &self.dir
    }
    // gp_Ax2::Location
    pub fn location(&self) -> &Pnt {
        &self.loc
    }
    // gp_Ax2::XDirection
    pub fn x_direction(&self) -> &Dir {
        &self.x
    }
    // gp_Ax2::YDirection
    pub fn y_direction(&self) -> &Dir {
        &self.y
    }
    // gp_Ax2::IsCoplanar (with another Ax2 or with an Ax1)
    pub fn is_coplanar<T: Ax2Coplanar>(&self, other: &T, linear_tolerance: f64, angular_tolerance: f64) -> bool {
        other.coplanar_with_ax2(self, linear_tolerance, angular_tolerance)
    }
    // gp_Ax2::Mirror
    pub fn mirror<M: Ax2Mirror>(&mut self, m: &M) {
        m.mirror_ax2(self);
    }
    // gp_Ax2::Mirrored
    pub fn mirrored<M: Ax2Mirror>(&self, m: &M) -> Ax2 {
        let mut a = *self;
        m.mirror_ax2(&mut a);
        a
    }
    // gp_Ax2::Rotate
    pub fn rotate(&mut self, a1: &Ax1, ang: f64) {
        self.loc.rotate(a1, ang);
        self.x.rotate(a1, ang);
        self.y.rotate(a1, ang);
        self.dir = cross_or(&self.x, &self.y, self.dir);
    }
    // gp_Ax2::Rotated
    pub fn rotated(&self, a1: &Ax1, ang: f64) -> Ax2 {
        let mut a = *self;
        a.rotate(a1, ang);
        a
    }
    // gp_Ax2::Scale
    pub fn scale(&mut self, p: &Pnt, s: f64) {
        self.loc.scale(p, s);
        if s < 0.0 {
            self.x.reverse();
            self.y.reverse();
        }
    }
    // gp_Ax2::Scaled
    pub fn scaled(&self, p: &Pnt, s: f64) -> Ax2 {
        let mut a = *self;
        a.scale(p, s);
        a
    }
    // gp_Ax2::Transform
    pub fn transform(&mut self, t: &Trsf) {
        self.loc.transform(t);
        self.x.transform_raw(t);
        self.y.transform_raw(t);
        self.dir = cross_or(&self.x, &self.y, self.dir);
    }
    // gp_Ax2::Transformed
    pub fn transformed(&self, t: &Trsf) -> Ax2 {
        let mut a = *self;
        a.transform(t);
        a
    }
    // gp_Ax2::Translate(gp_Vec)
    pub fn translate(&mut self, v: &Vec) {
        self.loc.translate(v);
    }
    // gp_Ax2::Translated(gp_Vec)
    pub fn translated(&self, v: &Vec) -> Ax2 {
        let mut a = *self;
        a.loc.translate(v);
        a
    }
    // gp_Ax2::Translate(P1, P2)
    pub fn translate_points(&mut self, p1: &Pnt, p2: &Pnt) {
        self.loc.translate_points(p1, p2);
    }
    // gp_Ax2::Translated(P1, P2)
    pub fn translated_points(&self, p1: &Pnt, p2: &Pnt) -> Ax2 {
        let mut a = *self;
        a.loc.translate_points(p1, p2);
        a
    }
}
