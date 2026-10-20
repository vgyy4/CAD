//! `Bnd_Box`: an axis-aligned 3D bounding box, possibly void or open on some sides.
use super::fmt::ostream_double;
use crate::{
    gp::{Dir, Direction, Lin, Pln, Pnt, Trsf, TrsfForm},
    mathlin::MathError,
};

/// Limit used for an open side (`THE_BND_PRECISION_INFINITE`).
const INFINITE: f64 = 1.0e100;

/// `Bnd_Box::Limits`.
#[derive(Debug, Clone, Copy)]
pub struct BoxLimits {
    pub xmin: f64,
    pub xmax: f64,
    pub ymin: f64,
    pub ymax: f64,
    pub zmin: f64,
    pub zmax: f64,
}

/// `Bnd_Box`.
#[derive(Debug, Clone)]
pub struct BndBox {
    xmin: f64,
    xmax: f64,
    ymin: f64,
    ymax: f64,
    zmin: f64,
    zmax: f64,
    gap: f64,
    flags: i32,
}

impl BndBox {
    const VOID_MASK: i32 = 0x01;
    const XMIN_MASK: i32 = 0x02;
    const XMAX_MASK: i32 = 0x04;
    const YMIN_MASK: i32 = 0x08;
    const YMAX_MASK: i32 = 0x10;
    const ZMIN_MASK: i32 = 0x20;
    const ZMAX_MASK: i32 = 0x40;
    const WHOLE_MASK: i32 = 0x7e;

    /// `Bnd_Box()`.
    pub fn new() -> Self {
        Self {
            xmin: f64::MAX,
            xmax: -f64::MAX,
            ymin: f64::MAX,
            ymax: -f64::MAX,
            zmin: f64::MAX,
            zmax: -f64::MAX,
            gap: 0.0,
            flags: Self::VOID_MASK,
        }
    }
    /// `Bnd_Box(theMin, theMax)`.
    pub fn from_corners(min: &Pnt, max: &Pnt) -> Self {
        Self {
            xmin: min.x(),
            xmax: max.x(),
            ymin: min.y(),
            ymax: max.y(),
            zmin: min.z(),
            zmax: max.z(),
            gap: 0.0,
            flags: 0,
        }
    }
    pub fn set_whole(&mut self) {
        self.flags = Self::WHOLE_MASK;
    }
    pub fn set_void(&mut self) {
        *self = Self::new();
    }
    pub fn set_point(&mut self, p: &Pnt) {
        self.set_void();
        self.add_point(p);
    }
    pub fn set_point_direction(&mut self, p: &Pnt, d: &Dir) {
        self.set_void();
        self.add_point_direction(p, d);
    }
    pub fn update(&mut self, xmin: f64, ymin: f64, zmin: f64, xmax: f64, ymax: f64, zmax: f64) {
        if self.is_void() {
            self.xmin = xmin;
            self.ymin = ymin;
            self.zmin = zmin;
            self.xmax = xmax;
            self.ymax = ymax;
            self.zmax = zmax;
            self.flags &= !Self::VOID_MASK;
        } else {
            self.xmin = cmin(self.xmin, xmin);
            self.xmax = cmax(self.xmax, xmax);
            self.ymin = cmin(self.ymin, ymin);
            self.ymax = cmax(self.ymax, ymax);
            self.zmin = cmin(self.zmin, zmin);
            self.zmax = cmax(self.zmax, zmax);
        }
    }
    pub fn update_point(&mut self, x: f64, y: f64, z: f64) {
        if self.is_void() {
            self.xmin = x;
            self.ymin = y;
            self.zmin = z;
            self.xmax = x;
            self.ymax = y;
            self.zmax = z;
            self.flags &= !Self::VOID_MASK;
        } else {
            self.xmin = cmin(self.xmin, x);
            self.xmax = cmax(self.xmax, x);
            self.ymin = cmin(self.ymin, y);
            self.ymax = cmax(self.ymax, y);
            self.zmin = cmin(self.zmin, z);
            self.zmax = cmax(self.zmax, z);
        }
    }
    pub fn gap(&self) -> f64 {
        self.gap
    }
    pub fn set_gap(&mut self, tol: f64) {
        self.gap = tol.abs();
    }
    pub fn enlarge(&mut self, tol: f64) {
        self.gap = cmax(self.gap, tol.abs());
    }
    /// `Get(Xmin, Ymin, Zmin, Xmax, Ymax, Zmax)`; fails for a void box.
    pub fn get(&self) -> Result<[f64; 6], MathError> {
        if self.is_void() {
            return Err(MathError::Construction);
        }
        Ok([self.get_x_min(), self.get_y_min(), self.get_z_min(), self.get_x_max(), self.get_y_max(), self.get_z_max()])
    }
    pub fn limits(&self) -> BoxLimits {
        BoxLimits {
            xmin: self.get_x_min(),
            xmax: self.get_x_max(),
            ymin: self.get_y_min(),
            ymax: self.get_y_max(),
            zmin: self.get_z_min(),
            zmax: self.get_z_max(),
        }
    }
    pub fn get_x_min(&self) -> f64 {
        if self.is_open_xmin() { -INFINITE } else { self.xmin - self.gap }
    }
    pub fn get_x_max(&self) -> f64 {
        if self.is_open_xmax() { INFINITE } else { self.xmax + self.gap }
    }
    pub fn get_y_min(&self) -> f64 {
        if self.is_open_ymin() { -INFINITE } else { self.ymin - self.gap }
    }
    pub fn get_y_max(&self) -> f64 {
        if self.is_open_ymax() { INFINITE } else { self.ymax + self.gap }
    }
    pub fn get_z_min(&self) -> f64 {
        if self.is_open_zmin() { -INFINITE } else { self.zmin - self.gap }
    }
    pub fn get_z_max(&self) -> f64 {
        if self.is_open_zmax() { INFINITE } else { self.zmax + self.gap }
    }
    pub fn corner_min(&self) -> Result<Pnt, MathError> {
        if self.is_void() {
            return Err(MathError::Construction);
        }
        Ok(Pnt::new(
            if self.is_open_xmin() { -INFINITE } else { self.xmin - self.gap },
            if self.is_open_ymin() { -INFINITE } else { self.ymin - self.gap },
            if self.is_open_zmin() { -INFINITE } else { self.zmin - self.gap },
        ))
    }
    pub fn corner_max(&self) -> Result<Pnt, MathError> {
        if self.is_void() {
            return Err(MathError::Construction);
        }
        Ok(Pnt::new(
            if self.is_open_xmax() { INFINITE } else { self.xmax + self.gap },
            if self.is_open_ymax() { INFINITE } else { self.ymax + self.gap },
            if self.is_open_zmax() { INFINITE } else { self.zmax + self.gap },
        ))
    }
    pub fn center(&self) -> Option<Pnt> {
        if self.is_void() {
            return None;
        }
        Some(Pnt::new(
            0.5 * (self.get_x_min() + self.get_x_max()),
            0.5 * (self.get_y_min() + self.get_y_max()),
            0.5 * (self.get_z_min() + self.get_z_max()),
        ))
    }
    pub fn open_xmin(&mut self) {
        self.flags |= Self::XMIN_MASK;
    }
    pub fn open_xmax(&mut self) {
        self.flags |= Self::XMAX_MASK;
    }
    pub fn open_ymin(&mut self) {
        self.flags |= Self::YMIN_MASK;
    }
    pub fn open_ymax(&mut self) {
        self.flags |= Self::YMAX_MASK;
    }
    pub fn open_zmin(&mut self) {
        self.flags |= Self::ZMIN_MASK;
    }
    pub fn open_zmax(&mut self) {
        self.flags |= Self::ZMAX_MASK;
    }
    pub fn is_open(&self) -> bool {
        (self.flags & Self::WHOLE_MASK) != 0
    }
    pub fn is_open_xmin(&self) -> bool {
        (self.flags & Self::XMIN_MASK) != 0
    }
    pub fn is_open_xmax(&self) -> bool {
        (self.flags & Self::XMAX_MASK) != 0
    }
    pub fn is_open_ymin(&self) -> bool {
        (self.flags & Self::YMIN_MASK) != 0
    }
    pub fn is_open_ymax(&self) -> bool {
        (self.flags & Self::YMAX_MASK) != 0
    }
    pub fn is_open_zmin(&self) -> bool {
        (self.flags & Self::ZMIN_MASK) != 0
    }
    pub fn is_open_zmax(&self) -> bool {
        (self.flags & Self::ZMAX_MASK) != 0
    }
    pub fn is_whole(&self) -> bool {
        (self.flags & Self::WHOLE_MASK) == Self::WHOLE_MASK
    }
    pub fn is_void(&self) -> bool {
        (self.flags & Self::VOID_MASK) != 0
    }
    pub fn is_x_thin(&self, tol: f64) -> bool {
        if self.is_whole() {
            return false;
        }
        if self.is_void() {
            return true;
        }
        if self.is_open_xmin() || self.is_open_xmax() {
            return false;
        }
        self.xmax - self.xmin < tol
    }
    pub fn is_y_thin(&self, tol: f64) -> bool {
        if self.is_whole() {
            return false;
        }
        if self.is_void() {
            return true;
        }
        if self.is_open_ymin() || self.is_open_ymax() {
            return false;
        }
        self.ymax - self.ymin < tol
    }
    pub fn is_z_thin(&self, tol: f64) -> bool {
        if self.is_whole() {
            return false;
        }
        if self.is_void() {
            return true;
        }
        if self.is_open_zmin() || self.is_open_zmax() {
            return false;
        }
        self.zmax - self.zmin < tol
    }
    pub fn is_thin(&self, tol: f64) -> bool {
        if self.is_whole() {
            return false;
        }
        if self.is_void() {
            return true;
        }
        if self.is_open_xmin() || self.is_open_xmax() || self.xmax - self.xmin >= tol {
            return false;
        }
        if self.is_open_ymin() || self.is_open_ymax() || self.ymax - self.ymin >= tol {
            return false;
        }
        if self.is_open_zmin() || self.is_open_zmax() || self.zmax - self.zmin >= tol {
            return false;
        }
        true
    }
    pub fn transformed(&self, t: &Trsf) -> Self {
        if self.is_void() {
            return Self::new();
        } else if t.form() == TrsfForm::Identity {
            return self.clone();
        } else if t.form() == TrsfForm::Translation {
            if !self.has_finite_part() {
                return self.clone();
            }
            let delta = t.translation_part();
            let mut new_box = self.clone();
            new_box.xmin += delta.x();
            new_box.xmax += delta.x();
            new_box.ymin += delta.y();
            new_box.ymax += delta.y();
            new_box.zmin += delta.z();
            new_box.zmax += delta.z();
            return new_box;
        }
        let mut new_box = Self::new();
        if self.has_finite_part() {
            let mut corners = [
                Pnt::new(self.xmin, self.ymin, self.zmin),
                Pnt::new(self.xmax, self.ymin, self.zmin),
                Pnt::new(self.xmin, self.ymax, self.zmin),
                Pnt::new(self.xmax, self.ymax, self.zmin),
                Pnt::new(self.xmin, self.ymin, self.zmax),
                Pnt::new(self.xmax, self.ymin, self.zmax),
                Pnt::new(self.xmin, self.ymax, self.zmax),
                Pnt::new(self.xmax, self.ymax, self.zmax),
            ];
            for corner in corners.iter_mut() {
                corner.transform(t);
                new_box.add_point(corner);
            }
        }
        new_box.gap = self.gap;
        if !self.is_open() {
            return new_box;
        }
        let mut dirs: std::vec::Vec<Dir> = std::vec::Vec::with_capacity(6);
        if self.is_open_xmin() {
            dirs.push(Dir::from_direction(Direction::NegativeX));
        }
        if self.is_open_xmax() {
            dirs.push(Dir::from_direction(Direction::X));
        }
        if self.is_open_ymin() {
            dirs.push(Dir::from_direction(Direction::NegativeY));
        }
        if self.is_open_ymax() {
            dirs.push(Dir::from_direction(Direction::Y));
        }
        if self.is_open_zmin() {
            dirs.push(Dir::from_direction(Direction::NegativeZ));
        }
        if self.is_open_zmax() {
            dirs.push(Dir::from_direction(Direction::Z));
        }
        for dir in dirs.iter_mut() {
            // A unit axis direction cannot fail under a `Trsf`; the raw form is `gp_Dir::Transform`.
            dir.transform_raw(t);
            new_box.add_direction(dir);
        }
        new_box
    }
    pub fn add_box(&mut self, other: &Self) {
        if other.is_void() {
            return;
        } else if self.is_void() {
            *self = other.clone();
            return;
        }
        self.gap = cmax(self.gap, other.gap);
        if self.is_whole() {
            return;
        } else if other.is_whole() {
            self.set_whole();
            return;
        }
        if !self.is_open_xmin() {
            if other.is_open_xmin() {
                self.open_xmin();
            } else if self.xmin > other.xmin {
                self.xmin = other.xmin;
            }
        }
        if !self.is_open_xmax() {
            if other.is_open_xmax() {
                self.open_xmax();
            } else if self.xmax < other.xmax {
                self.xmax = other.xmax;
            }
        }
        if !self.is_open_ymin() {
            if other.is_open_ymin() {
                self.open_ymin();
            } else if self.ymin > other.ymin {
                self.ymin = other.ymin;
            }
        }
        if !self.is_open_ymax() {
            if other.is_open_ymax() {
                self.open_ymax();
            } else if self.ymax < other.ymax {
                self.ymax = other.ymax;
            }
        }
        if !self.is_open_zmin() {
            if other.is_open_zmin() {
                self.open_zmin();
            } else if self.zmin > other.zmin {
                self.zmin = other.zmin;
            }
        }
        if !self.is_open_zmax() {
            if other.is_open_zmax() {
                self.open_zmax();
            } else if self.zmax < other.zmax {
                self.zmax = other.zmax;
            }
        }
    }
    pub fn add_point(&mut self, p: &Pnt) {
        self.update_point(p.x(), p.y(), p.z());
    }
    pub fn add_point_direction(&mut self, p: &Pnt, d: &Dir) {
        self.add_point(p);
        self.add_direction(d);
    }
    pub fn add_direction(&mut self, d: &Dir) {
        let (dx, dy, dz) = (d.x(), d.y(), d.z());
        if dx < -f64::EPSILON {
            self.open_xmin();
        } else if dx > f64::EPSILON {
            self.open_xmax();
        }
        if dy < -f64::EPSILON {
            self.open_ymin();
        } else if dy > f64::EPSILON {
            self.open_ymax();
        }
        if dz < -f64::EPSILON {
            self.open_zmin();
        } else if dz > f64::EPSILON {
            self.open_zmax();
        }
    }
    pub fn is_out_point(&self, p: &Pnt) -> bool {
        if self.is_whole() {
            return false;
        } else if self.is_void() {
            return true;
        }
        let (x, y, z) = (p.x(), p.y(), p.z());
        (!self.is_open_xmin() && (x < (self.xmin - self.gap)))
            || (!self.is_open_xmax() && (x > (self.xmax + self.gap)))
            || (!self.is_open_ymin() && (y < (self.ymin - self.gap)))
            || (!self.is_open_ymax() && (y > (self.ymax + self.gap)))
            || (!self.is_open_zmin() && (z < (self.zmin - self.gap)))
            || (!self.is_open_zmax() && (z > (self.zmax + self.gap)))
    }
    pub fn is_out_plane(&self, p: &Pln) -> bool {
        if self.is_whole() {
            return false;
        } else if self.is_void() {
            return true;
        }
        let (a, b, c, d) = p.coefficients();
        let xmin = self.get_x_min();
        let xmax = self.get_x_max();
        let ymin = self.get_y_min();
        let ymax = self.get_y_max();
        let zmin = self.get_z_min();
        let zmax = self.get_z_max();
        let dist = a * xmin + b * ymin + c * zmin + d;
        let plus = dist > 0.0;
        if plus != ((a * xmin + b * ymin + c * zmax + d) > 0.0) {
            return false;
        }
        if plus != ((a * xmin + b * ymax + c * zmin + d) > 0.0) {
            return false;
        }
        if plus != ((a * xmin + b * ymax + c * zmax + d) > 0.0) {
            return false;
        }
        if plus != ((a * xmax + b * ymin + c * zmin + d) > 0.0) {
            return false;
        }
        if plus != ((a * xmax + b * ymin + c * zmax + d) > 0.0) {
            return false;
        }
        if plus != ((a * xmax + b * ymax + c * zmin + d) > 0.0) {
            return false;
        }
        plus == ((a * xmax + b * ymax + c * zmax + d) > 0.0)
    }
    pub fn is_out_line(&self, l: &Lin) -> bool {
        if self.is_whole() {
            return false;
        } else if self.is_void() {
            return true;
        }
        let (mut xmin, mut xmax, mut ymin, mut ymax) = (0.0, 0.0, 0.0, 0.0);
        let (zmin, zmax);
        let (mut parmin, mut parmax, mut par1, mut par2);
        let (x_to_set, y_to_set);
        let (bxmin, bymin, bzmin, bxmax, bymax, bzmax) = match self.get() {
            Ok([a, b, c, d, e, f]) => (a, b, c, d, e, f),
            Err(_) => return true,
        };
        let loc = l.location();
        let dir = l.direction();
        let dir_x = dir.x().abs();
        let dir_y = dir.y().abs();
        let dir_z = dir.z().abs();

        if dir_x > 0.0 {
            par1 = (bxmin - loc.x()) / dir.x();
            par2 = (bxmax - loc.x()) / dir.x();
            parmin = cmin(par1, par2);
            parmax = cmax(par1, par2);
            x_to_set = true;
        } else {
            if loc.x() < bxmin || bxmax < loc.x() {
                return true;
            }
            xmin = loc.x();
            xmax = loc.x();
            parmin = -INFINITE;
            parmax = INFINITE;
            x_to_set = false;
        }

        if dir_y > 0.0 {
            par1 = (bymin - loc.y()) / dir.y();
            par2 = (bymax - loc.y()) / dir.y();
            if parmax < cmin(par1, par2) || parmin > cmax(par1, par2) {
                return true;
            }
            parmin = cmax(parmin, cmin(par1, par2));
            parmax = cmin(parmax, cmax(par1, par2));
            y_to_set = true;
        } else {
            if loc.y() < bymin || bymax < loc.y() {
                return true;
            }
            ymin = loc.y();
            ymax = loc.y();
            y_to_set = false;
        }

        if dir_z > 0.0 {
            par1 = (bzmin - loc.z()) / dir.z();
            par2 = (bzmax - loc.z()) / dir.z();
            if parmax < cmin(par1, par2) || parmin > cmax(par1, par2) {
                return true;
            }
            parmin = cmax(parmin, cmin(par1, par2));
            parmax = cmin(parmax, cmax(par1, par2));
            par1 = loc.z() + parmin * dir.z();
            par2 = loc.z() + parmax * dir.z();
            zmin = cmin(par1, par2);
            zmax = cmax(par1, par2);
        } else {
            if loc.z() < bzmin || bzmax < loc.z() {
                return true;
            }
            zmin = loc.z();
            zmax = loc.z();
        }
        if zmax < bzmin || bzmax < zmin {
            return true;
        }

        if x_to_set {
            par1 = loc.x() + parmin * dir.x();
            par2 = loc.x() + parmax * dir.x();
            xmin = cmin(par1, par2);
            xmax = cmax(par1, par2);
        }
        if xmax < bxmin || bxmax < xmin {
            return true;
        }

        if y_to_set {
            par1 = loc.y() + parmin * dir.y();
            par2 = loc.y() + parmax * dir.y();
            ymin = cmin(par1, par2);
            ymax = cmax(par1, par2);
        }
        if ymax < bymin || bymax < ymin {
            return true;
        }
        false
    }
    pub fn is_out_box(&self, other: &Self) -> bool {
        if self.flags == 0 && other.flags == 0 {
            let delta = other.gap + self.gap;
            if self.xmin - other.xmax > delta {
                return true;
            }
            if other.xmin - self.xmax > delta {
                return true;
            }
            if self.ymin - other.ymax > delta {
                return true;
            }
            if other.ymin - self.ymax > delta {
                return true;
            }
            if self.zmin - other.zmax > delta {
                return true;
            }
            if other.zmin - self.zmax > delta {
                return true;
            }
            return false;
        }
        if self.is_void() || other.is_void() {
            return true;
        }
        if self.is_whole() || other.is_whole() {
            return false;
        }
        let delta = other.gap + self.gap;
        if !self.is_open_xmin() && !other.is_open_xmax() && self.xmin - other.xmax > delta {
            return true;
        }
        if !self.is_open_xmax() && !other.is_open_xmin() && other.xmin - self.xmax > delta {
            return true;
        }
        if !self.is_open_ymin() && !other.is_open_ymax() && self.ymin - other.ymax > delta {
            return true;
        }
        if !self.is_open_ymax() && !other.is_open_ymin() && other.ymin - self.ymax > delta {
            return true;
        }
        if !self.is_open_zmin() && !other.is_open_zmax() && self.zmin - other.zmax > delta {
            return true;
        }
        if !self.is_open_zmax() && !other.is_open_zmin() && other.zmin - self.zmax > delta {
            return true;
        }
        false
    }
    pub fn is_out_box_transformed(&self, other: &Self, t: &Trsf) -> bool {
        self.is_out_box(&other.transformed(t))
    }
    pub fn is_out_transformed(&self, t1: &Trsf, other: &Self, t2: &Trsf) -> bool {
        self.transformed(t1).is_out_box(&other.transformed(t2))
    }
    pub fn contains(&self, p: &Pnt) -> bool {
        !self.is_out_point(p)
    }
    pub fn intersects(&self, other: &Self) -> bool {
        !self.is_out_box(other)
    }
    pub fn is_out_segment(&self, p1: &Pnt, p2: &Pnt, d: &Dir) -> bool {
        if self.is_whole() {
            return false;
        }
        if self.is_void() {
            return true;
        }
        let eps = f64::MIN_POSITIVE;
        let (x0, x1) = (self.get_x_min(), self.get_x_max());
        let (y0, y1) = (self.get_y_min(), self.get_y_max());
        let (z0, z1) = (self.get_z_min(), self.get_z_max());
        let (dx, dy, dz) = (d.x(), d.y(), d.z());
        let (ax, ay, az) = (p1.x(), p1.y(), p1.z());
        let (bx, by, bz) = (p2.x(), p2.y(), p2.z());
        if dx.abs() < eps && dy.abs() < eps {
            return segment_out(x0, y0, x1, y1, ax, ay, bx, by);
        }
        if dx.abs() < eps && dz.abs() < eps {
            return segment_out(x0, z0, x1, z1, ax, az, bx, bz);
        }
        if dy.abs() < eps && dz.abs() < eps {
            return segment_out(y0, z0, y1, z1, ay, az, by, bz);
        }
        if dx.abs() < eps {
            return !(!segment_out(x0, z0, x1, z1, ax, (y0 - ay) * dz / dy + az, bx, (y0 - by) * dz / dy + bz)
                || !segment_out(x0, z0, x1, z1, ax, (y1 - ay) * dz / dy + az, bx, (y1 - by) * dz / dy + bz)
                || !segment_out(x0, y0, x1, y1, ax, (z0 - az) * dy / dz + ay, bx, (z0 - bz) * dy / dz + by)
                || !segment_out(x0, y0, x1, y1, ax, (z1 - az) * dy / dz + ay, bx, (z1 - bz) * dy / dz + by));
        }
        if dy.abs() < eps {
            return !(!segment_out(y0, z0, y1, z1, ay, (x0 - ax) * dz / dx + az, by, (x0 - bx) * dz / dx + bz)
                || !segment_out(y0, z0, y1, z1, ay, (x1 - ax) * dz / dx + az, by, (x1 - bx) * dz / dx + bz)
                || !segment_out(y0, x0, y1, x1, ay, (z0 - az) * dx / dz + ax, by, (z0 - bz) * dx / dz + bx)
                || !segment_out(y0, x0, y1, x1, ay, (z1 - az) * dx / dz + ax, by, (z1 - bz) * dx / dz + bx));
        }
        if dz.abs() < eps {
            return !(!segment_out(z0, x0, z1, x1, az, (y1 - ay) * dx / dy + ax, bz, (y1 - by) * dx / dy + bx)
                || !segment_out(z0, x0, z1, x1, az, (y0 - ay) * dx / dy + ax, bz, (y0 - by) * dx / dy + bx)
                || !segment_out(z0, y0, z1, y1, az, (x1 - ax) * dy / dx + ay, bz, (x1 - bx) * dy / dx + by)
                || !segment_out(z0, y0, z1, y1, az, (x0 - ax) * dy / dx + ay, bz, (x0 - bx) * dy / dx + by));
        }
        !(!segment_out(
            x0,
            z0,
            x1,
            z1,
            (y0 - ay) / dy * dx + ax,
            (y0 - ay) / dy * dz + az,
            (y0 - by) / dy * dx + bx,
            (y0 - by) / dy * dz + bz,
        ) || !segment_out(
            x0,
            z0,
            x1,
            z1,
            (y1 - ay) / dy * dx + ax,
            (y1 - ay) / dy * dz + az,
            (y1 - by) / dy * dx + bx,
            (y1 - by) / dy * dz + bz,
        ) || !segment_out(
            x0,
            y0,
            x1,
            y1,
            (z0 - az) / dz * dx + ax,
            (z0 - az) / dz * dy + ay,
            (z0 - bz) / dz * dx + bx,
            (z0 - bz) / dz * dy + by,
        ) || !segment_out(
            x0,
            y0,
            x1,
            y1,
            (z1 - az) / dz * dx + ax,
            (z1 - az) / dz * dy + ay,
            (z1 - bz) / dz * dx + bx,
            (z1 - bz) / dz * dy + by,
        ) || !segment_out(
            z0,
            y0,
            z1,
            y1,
            (x0 - ax) / dx * dz + az,
            (x0 - ax) / dx * dy + ay,
            (x0 - bx) / dx * dz + bz,
            (x0 - bx) / dx * dy + by,
        ) || !segment_out(
            z0,
            y0,
            z1,
            y1,
            (x1 - ax) / dx * dz + az,
            (x1 - ax) / dx * dy + ay,
            (x1 - bx) / dx * dz + bz,
            (x1 - bx) / dx * dy + by,
        ))
    }
    pub fn distance(&self, other: &Self) -> f64 {
        if self.is_void() || other.is_void() {
            return 0.0;
        }
        let (Ok(b1), Ok(b2)) = (self.get(), other.get()) else {
            return 0.0;
        };
        let [xmin1, ymin1, zmin1, xmax1, ymax1, zmax1] = b1;
        let [xmin2, ymin2, zmin2, xmax2, ymax2, zmax2] = b2;
        let dist_x = distance_in_dimension(xmin1, xmax1, xmin2, xmax2);
        let dist_y = distance_in_dimension(ymin1, ymax1, ymin2, ymax2);
        let dist_z = distance_in_dimension(zmin1, zmax1, zmin2, zmax2);
        (dist_x + dist_y + dist_z).sqrt()
    }
    pub fn dump(&self) -> String {
        let mut out = String::from("Box3D : ");
        if self.is_void() {
            out.push_str("Void");
        } else if self.is_whole() {
            out.push_str("Whole");
        } else {
            let sides = [
                ("Xmin", self.is_open_xmin(), self.xmin),
                ("Xmax", self.is_open_xmax(), self.xmax),
                ("Ymin", self.is_open_ymin(), self.ymin),
                ("Ymax", self.is_open_ymax(), self.ymax),
                ("Zmin", self.is_open_zmin(), self.zmin),
                ("Zmax", self.is_open_zmax(), self.zmax),
            ];
            for (name, open, value) in sides {
                out.push_str("\n ");
                out.push_str(name);
                out.push_str(" : ");
                if open {
                    out.push_str("Infinite");
                } else {
                    out.push_str(&ostream_double(value));
                }
            }
        }
        out.push_str("\n Gap : ");
        out.push_str(&ostream_double(self.gap));
        out.push('\n');
        out
    }
    pub fn square_extent(&self) -> f64 {
        if self.is_void() {
            return 0.0;
        }
        let dx = self.xmax - self.xmin + self.gap + self.gap;
        let dy = self.ymax - self.ymin + self.gap + self.gap;
        let dz = self.zmax - self.zmin + self.gap + self.gap;
        dx * dx + dy * dy + dz * dz
    }
    pub fn finite_part(&self) -> Self {
        if !self.has_finite_part() {
            return Self::new();
        }
        let mut b = Self::new();
        b.update(self.xmin, self.ymin, self.zmin, self.xmax, self.ymax, self.zmax);
        b.set_gap(self.gap);
        b
    }
    pub fn has_finite_part(&self) -> bool {
        !self.is_void() && self.xmax >= self.xmin
    }
}

impl Default for BndBox {
    fn default() -> Self {
        Self::new()
    }
}

/// `Square(a - b)` of the smaller of the two gaps between intervals (`DistMini2Box`).
fn dist_mini_2_box(min1: f64, max1: f64, min2: f64, max2: f64) -> f64 {
    let r1 = (min1 - max2) * (min1 - max2);
    let r2 = (max1 - min2) * (max1 - min2);
    cmin(r1, r2)
}

fn distance_in_dimension(min1: f64, max1: f64, min2: f64, max2: f64) -> f64 {
    if (min1 <= min2 && min2 <= max1) || (min2 <= min1 && min1 <= max2) {
        return 0.0;
    }
    dist_mini_2_box(min1, max1, min2, max2)
}

#[allow(clippy::too_many_arguments)]
fn segment_out(x1: f64, y1: f64, x2: f64, y2: f64, xs1: f64, ys1: f64, xs2: f64, ys2: f64) -> bool {
    let e = f64::MIN_POSITIVE;
    let (xsmin, xsmax) = (cmin(xs1, xs2), cmax(xs1, xs2));
    let (ysmin, ysmax) = (cmin(ys1, ys2), cmax(ys1, ys2));
    if ysmax - ysmin < e
        && (y1 - ys1 < e && ys1 - y2 < e)
        && ((xsmin - x1 < e && x1 - xsmax < e) || (xsmin - x2 < e && x2 - xsmax < e) || (x1 - xs1 < e && xs1 - x2 < e))
    {
        return false;
    }
    if xsmax - xsmin < e
        && (x1 - xs1 < e && xs1 - x2 < e)
        && ((ysmin - y1 < e && y1 - ysmax < e) || (ysmin - y2 < e && y2 - ysmax < e) || (y1 - ys1 < e && ys1 - y2 < e))
    {
        return false;
    }
    if (xs1 < x1 && xs2 < x1) || (xs1 > x2 && xs2 > x2) || (ys1 < y1 && ys2 < y1) || (ys1 > y2 && ys2 > y2) {
        return true;
    }
    if (xs2 - xs1).abs() > e {
        let ya = cmin(x1, x2);
        let yb = cmax(x1, x2);
        let ya = (ya - xs1) * (ys2 - ys1) / (xs2 - xs1) + ys1;
        let yb = (yb - xs1) * (ys2 - ys1) / (xs2 - xs1) + ys1;
        if ya < y1 && yb < y1 || ya > y2 && yb > y2 {
            return true;
        }
    } else if (ys2 - ys1).abs() > e {
        let xa = cmin(y1, y2);
        let xb = cmax(y1, y2);
        let xa = (xa - ys1) * (xs2 - xs1) / (ys2 - ys1) + xs1;
        let xb = (xb - ys1) * (xs2 - xs1) / (ys2 - ys1) + xs1;
        if xa < x1 && xb < x1 || xa > x2 && xb > x2 {
            return true;
        }
    } else {
        return true;
    }
    false
}

/// `std::min(a, b)`.
pub(super) fn cmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}
/// `std::max(a, b)`.
pub(super) fn cmax(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}
