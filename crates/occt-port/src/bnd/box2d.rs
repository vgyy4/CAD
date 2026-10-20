//! `Bnd_Box2d`: an axis-aligned 2D bounding box, possibly void or open on some sides.
use super::{
    box3d::{cmax, cmin},
    fmt::ostream_double,
};
use crate::{
    gp::{Dir2d, Direction2d, Lin2d, Pnt2d, Trsf2d, TrsfForm, XY},
    mathlin::MathError,
};

/// Limit used for an open side (`THE_BND_PRECISION_INFINITE`).
const INFINITE: f64 = 1.0e100;

/// `Bnd_Box2d::Limits`.
#[derive(Debug, Clone, Copy)]
pub struct BoxLimits2d {
    pub xmin: f64,
    pub xmax: f64,
    pub ymin: f64,
    pub ymax: f64,
}

/// `Bnd_Box2d`.
#[derive(Debug, Clone)]
pub struct BndBox2d {
    xmin: f64,
    xmax: f64,
    ymin: f64,
    ymax: f64,
    gap: f64,
    flags: i32,
}

impl BndBox2d {
    const VOID_MASK: i32 = 0x01;
    const XMIN_MASK: i32 = 0x02;
    const XMAX_MASK: i32 = 0x04;
    const YMIN_MASK: i32 = 0x08;
    const YMAX_MASK: i32 = 0x10;
    const WHOLE_MASK: i32 = 0x1e;

    /// `Bnd_Box2d()`.
    pub fn new() -> Self {
        Self { xmin: f64::MAX, xmax: -f64::MAX, ymin: f64::MAX, ymax: -f64::MAX, gap: 0.0, flags: Self::VOID_MASK }
    }
    pub fn set_whole(&mut self) {
        self.flags = Self::WHOLE_MASK;
    }
    pub fn set_void(&mut self) {
        *self = Self::new();
    }
    /// `Set(thePnt)`: the limits are not reset.
    pub fn set_point(&mut self, p: &Pnt2d) {
        self.flags = Self::VOID_MASK;
        self.gap = 0.0;
        self.add_point(p);
    }
    pub fn set_point_direction(&mut self, p: &Pnt2d, d: &Dir2d) {
        self.flags = Self::VOID_MASK;
        self.gap = 0.0;
        self.add_point_direction(p, d);
    }
    pub fn update(&mut self, xmin: f64, ymin: f64, xmax: f64, ymax: f64) {
        if self.flags & Self::VOID_MASK != 0 {
            self.xmin = xmin;
            self.ymin = ymin;
            self.xmax = xmax;
            self.ymax = ymax;
            self.flags &= !Self::VOID_MASK;
        } else {
            if self.flags & Self::XMIN_MASK == 0 {
                self.xmin = cmin(self.xmin, xmin);
            }
            if self.flags & Self::XMAX_MASK == 0 {
                self.xmax = cmax(self.xmax, xmax);
            }
            if self.flags & Self::YMIN_MASK == 0 {
                self.ymin = cmin(self.ymin, ymin);
            }
            if self.flags & Self::YMAX_MASK == 0 {
                self.ymax = cmax(self.ymax, ymax);
            }
        }
    }
    pub fn update_point(&mut self, x: f64, y: f64) {
        if self.flags & Self::VOID_MASK != 0 {
            self.xmin = x;
            self.ymin = y;
            self.xmax = x;
            self.ymax = y;
            self.flags &= !Self::VOID_MASK;
        } else {
            if self.flags & Self::XMIN_MASK == 0 {
                self.xmin = cmin(self.xmin, x);
            }
            if self.flags & Self::XMAX_MASK == 0 {
                self.xmax = cmax(self.xmax, x);
            }
            if self.flags & Self::YMIN_MASK == 0 {
                self.ymin = cmin(self.ymin, y);
            }
            if self.flags & Self::YMAX_MASK == 0 {
                self.ymax = cmax(self.ymax, y);
            }
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
    /// `Get(Xmin, Ymin, Xmax, Ymax)`; fails for a void box.
    pub fn get(&self) -> Result<[f64; 4], MathError> {
        if self.flags & Self::VOID_MASK != 0 {
            return Err(MathError::Construction);
        }
        Ok([self.get_x_min(), self.get_y_min(), self.get_x_max(), self.get_y_max()])
    }
    pub fn limits(&self) -> BoxLimits2d {
        BoxLimits2d { xmin: self.get_x_min(), xmax: self.get_x_max(), ymin: self.get_y_min(), ymax: self.get_y_max() }
    }
    pub fn get_x_min(&self) -> f64 {
        if self.flags & Self::XMIN_MASK != 0 { -INFINITE } else { self.xmin - self.gap }
    }
    pub fn get_x_max(&self) -> f64 {
        if self.flags & Self::XMAX_MASK != 0 { INFINITE } else { self.xmax + self.gap }
    }
    pub fn get_y_min(&self) -> f64 {
        if self.flags & Self::YMIN_MASK != 0 { -INFINITE } else { self.ymin - self.gap }
    }
    pub fn get_y_max(&self) -> f64 {
        if self.flags & Self::YMAX_MASK != 0 { INFINITE } else { self.ymax + self.gap }
    }
    pub fn center(&self) -> Option<Pnt2d> {
        if self.is_void() {
            return None;
        }
        Some(Pnt2d::new(0.5 * (self.get_x_min() + self.get_x_max()), 0.5 * (self.get_y_min() + self.get_y_max())))
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
    pub fn is_whole(&self) -> bool {
        (self.flags & Self::WHOLE_MASK) == Self::WHOLE_MASK
    }
    pub fn is_void(&self) -> bool {
        (self.flags & Self::VOID_MASK) != 0
    }
    pub fn transformed(&self, t: &Trsf2d) -> Self {
        let form = t.form();
        let mut new_box = self.clone();
        if self.is_void() {
            return new_box;
        }
        if form == TrsfForm::Identity {
        } else if form == TrsfForm::Translation {
            let (dx, dy) = t.translation_part().coords();
            if self.flags & Self::XMIN_MASK == 0 {
                new_box.xmin += dx;
            }
            if self.flags & Self::XMAX_MASK == 0 {
                new_box.xmax += dx;
            }
            if self.flags & Self::YMIN_MASK == 0 {
                new_box.ymin += dy;
            }
            if self.flags & Self::YMAX_MASK == 0 {
                new_box.ymax += dy;
            }
        } else {
            let mut vertex = [true; 4];
            let mut dirs: std::vec::Vec<Dir2d> = std::vec::Vec::with_capacity(4);
            if self.flags & Self::XMIN_MASK != 0 {
                dirs.push(Dir2d::from_direction(Direction2d::NegativeX));
                vertex[0] = false;
                vertex[2] = false;
            }
            if self.flags & Self::XMAX_MASK != 0 {
                dirs.push(Dir2d::from_direction(Direction2d::X));
                vertex[1] = false;
                vertex[3] = false;
            }
            if self.flags & Self::YMIN_MASK != 0 {
                dirs.push(Dir2d::from_direction(Direction2d::NegativeY));
                vertex[0] = false;
                vertex[1] = false;
            }
            if self.flags & Self::YMAX_MASK != 0 {
                dirs.push(Dir2d::from_direction(Direction2d::Y));
                vertex[2] = false;
                vertex[3] = false;
            }
            new_box.set_void();
            for dir in dirs.iter_mut() {
                dir.transform(t);
                new_box.add_direction(dir);
            }
            let mut points = [
                Pnt2d::new(self.xmin, self.ymin),
                Pnt2d::new(self.xmax, self.ymin),
                Pnt2d::new(self.xmin, self.ymax),
                Pnt2d::new(self.xmax, self.ymax),
            ];
            for (point, keep) in points.iter_mut().zip(vertex) {
                if keep {
                    point.transform(t);
                    new_box.add_point(point);
                }
            }
            new_box.gap = self.gap;
        }
        new_box
    }
    pub fn add_box(&mut self, other: &Self) {
        if self.is_whole() || other.is_void() {
            return;
        }
        if other.is_whole() {
            self.set_whole();
        } else if self.is_void() {
            *self = other.clone();
        } else {
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
            self.gap = cmax(self.gap, other.gap);
        }
    }
    pub fn add_point(&mut self, p: &Pnt2d) {
        self.update_point(p.x(), p.y());
    }
    pub fn add_point_direction(&mut self, p: &Pnt2d, d: &Dir2d) {
        self.add_point(p);
        self.add_direction(d);
    }
    pub fn add_direction(&mut self, d: &Dir2d) {
        let (dx, dy) = (d.x(), d.y());
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
    }
    pub fn is_out_point(&self, p: &Pnt2d) -> bool {
        if self.is_whole() {
            false
        } else if self.is_void() {
            true
        } else {
            let (x, y) = (p.x(), p.y());
            (self.flags & Self::XMIN_MASK == 0 && (x < (self.xmin - self.gap)))
                || (self.flags & Self::XMAX_MASK == 0 && (x > (self.xmax + self.gap)))
                || (self.flags & Self::YMIN_MASK == 0 && (y < (self.ymin - self.gap)))
                || (self.flags & Self::YMAX_MASK == 0 && (y > (self.ymax + self.gap)))
        }
    }
    pub fn is_out_line(&self, l: &Lin2d) -> bool {
        if self.is_whole() {
            return false;
        }
        if self.is_void() {
            return true;
        }
        let Ok([xmin, ymin, xmax, ymax]) = self.get() else {
            return true;
        };
        let center = XY::new((xmin + xmax) / 2.0, (ymin + ymax) / 2.0);
        let heigh = XY::new((xmax - center.x()).abs(), (ymax - center.y()).abs());
        let dir = l.direction();
        let prod = [dir.xy().crossed(&center.subtracted(l.location().xy())), dir.x() * heigh.y(), dir.y() * heigh.x()];
        prod[0].abs() > (prod[1].abs() + prod[2].abs())
    }
    pub fn is_out_segment(&self, p0: &Pnt2d, p1: &Pnt2d) -> bool {
        if self.is_whole() {
            return false;
        }
        if self.is_void() {
            return true;
        }
        let mut status = true;
        let Ok([xmin, ymin, xmax, ymax]) = self.get() else {
            return true;
        };
        let seg_delta = p1.xy().subtracted(p0.xy());
        let center = XY::new((xmin + xmax) / 2.0, (ymin + ymax) / 2.0);
        let heigh = XY::new((xmax - center.x()).abs(), (ymax - center.y()).abs());
        let prod =
            [seg_delta.crossed(&center.subtracted(p0.xy())), seg_delta.x() * heigh.y(), seg_delta.y() * heigh.x()];
        if prod[0].abs() <= (prod[1].abs() + prod[2].abs()) {
            let hseg = XY::new(0.5 * seg_delta.x(), 0.5 * seg_delta.y());
            let hseg_abs = XY::new(hseg.x().abs(), hseg.y().abs());
            let rel = p0.xy().added(&hseg).subtracted(&center);
            let limit = heigh.added(&hseg_abs);
            status = (rel.x().abs() > limit.x()) || (rel.y().abs() > limit.y());
        }
        status
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
            return false;
        }
        if self.is_void() || other.is_void() {
            return true;
        }
        if self.is_whole() || other.is_whole() {
            return false;
        }
        let Ok([oxmin, oymin, oxmax, oymax]) = other.get() else {
            return true;
        };
        if self.flags & Self::XMIN_MASK == 0 && (oxmax < (self.xmin - self.gap)) {
            return true;
        }
        if self.flags & Self::XMAX_MASK == 0 && (oxmin > (self.xmax + self.gap)) {
            return true;
        }
        if self.flags & Self::YMIN_MASK == 0 && (oymax < (self.ymin - self.gap)) {
            return true;
        }
        if self.flags & Self::YMAX_MASK == 0 && (oymin > (self.ymax + self.gap)) {
            return true;
        }
        false
    }
    pub fn is_out_box_transformed(&self, other: &Self, t: &Trsf2d) -> bool {
        self.is_out_box(&other.transformed(t))
    }
    pub fn is_out_transformed(&self, t1: &Trsf2d, other: &Self, t2: &Trsf2d) -> bool {
        self.transformed(t1).is_out_box(&other.transformed(t2))
    }
    pub fn contains(&self, p: &Pnt2d) -> bool {
        !self.is_out_point(p)
    }
    pub fn intersects(&self, other: &Self) -> bool {
        !self.is_out_box(other)
    }
    pub fn distance(&self, other: &Self) -> f64 {
        if self.is_void() || other.is_void() {
            return 0.0;
        }
        let (Ok([xmin1, ymin1, xmax1, ymax1]), Ok([xmin2, ymin2, xmax2, ymax2])) = (self.get(), other.get()) else {
            return 0.0;
        };
        let dist_axis = |min1: f64, max1: f64, min2: f64, max2: f64| -> f64 {
            if min1 > max2 {
                let d = min1 - max2;
                return d * d;
            }
            if min2 > max1 {
                let d = min2 - max1;
                return d * d;
            }
            0.0
        };
        let dx = dist_axis(xmin1, xmax1, xmin2, xmax2);
        let dy = dist_axis(ymin1, ymax1, ymin2, ymax2);
        (dx + dy).sqrt()
    }
    pub fn dump(&self) -> String {
        let mut out = String::from("Box2d : ");
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
        dx * dx + dy * dy
    }
}

impl Default for BndBox2d {
    fn default() -> Self {
        Self::new()
    }
}
