//! `Bnd_OBB`: an oriented bounding box, and the DiTO builder `OBBTool` behind `ReBuild`.
use super::{
    box3d::{BndBox, cmax, cmin},
    bvh::PointBvh,
};
use crate::{
    gp::{Ax3, Dir, Direction, Pnt, XYZ},
    mathlin::MathError,
};

/// `Precision::Confusion()`.
const CONFUSION: f64 = 1.0e-7;
/// `Precision::SquareConfusion()`.
const SQUARE_CONFUSION: f64 = CONFUSION * CONFUSION;
/// `SQRT_3` of `Bnd_OBB.cxx`.
const SQRT_3: f64 = 1.732_050_807_568_877_3;
/// Number of initial axes of the OBB tool.
const NB_INIT_AXES: usize = 7;
/// Number of extremal points of the OBB tool.
const NB_EXTREMAL_POINTS: usize = 2 * NB_INIT_AXES;

/// `sum_k h_k * |a_k . dir|`, accumulated from zero in index order.
fn half_extent(hdims: &[f64; 3], axes: &[XYZ; 3], dir: &XYZ) -> f64 {
    let mut total = 0.0;
    for (h, axis) in hdims.iter().zip(axes) {
        total += h * axis.dot(dir).abs();
    }
    total
}

fn sub(a: &XYZ, b: &XYZ) -> XYZ {
    a.subtracted(b)
}

fn add(a: &XYZ, b: &XYZ) -> XYZ {
    a.added(b)
}

/// `gp_XYZ::Normalized` with the zero-length check compiled out (as in the OCCT libraries).
fn normalized(v: &XYZ) -> XYZ {
    let d = v.modulus();
    XYZ::new(v.x() / d, v.y() / d, v.z() / d)
}

/// `Bnd_OBB`.
#[derive(Debug, Clone)]
pub struct BndObb {
    center: XYZ,
    axes: [XYZ; 3],
    hdims: [f64; 3],
    is_aabox: bool,
}

impl BndObb {
    /// `Bnd_OBB()`.
    pub fn new() -> Self {
        Self { center: XYZ::zero(), axes: [XYZ::zero(); 3], hdims: [-1.0; 3], is_aabox: false }
    }
    /// `Bnd_OBB(center, XDirection, YDirection, ZDirection, HXSize, HYSize, HZSize)`.
    pub fn from_axes(center: &Pnt, x: &Dir, y: &Dir, z: &Dir, hx: f64, hy: f64, hz: f64) -> Self {
        Self { center: *center.xyz(), axes: [*x.xyz(), *y.xyz(), *z.xyz()], hdims: [hx, hy, hz], is_aabox: false }
    }
    /// `Bnd_OBB(const Bnd_Box&)`.
    pub fn from_box(b: &BndBox) -> Self {
        let mut obb = Self::new();
        if b.is_void() {
            return obb;
        }
        obb.is_aabox = true;
        let Ok([x1, y1, z1, x2, y2, z2]) = b.get() else {
            return Self::new();
        };
        obb.axes[0] = XYZ::new(1.0, 0.0, 0.0);
        obb.axes[1] = XYZ::new(0.0, 1.0, 0.0);
        obb.axes[2] = XYZ::new(0.0, 0.0, 1.0);
        obb.hdims = [0.5 * (x2 - x1), 0.5 * (y2 - y1), 0.5 * (z2 - z1)];
        obb.center = XYZ::new(0.5 * (x2 + x1), 0.5 * (y2 + y1), 0.5 * (z2 + z1));
        obb
    }

    /// `ReBuild(theListOfPoints, theListOfTolerances, theIsOptimal)`.
    pub fn rebuild(&mut self, points: &[Pnt], tolerances: Option<&[f64]>, optimal: bool) -> Result<(), MathError> {
        if tolerances.is_some_and(|t| t.len() != points.len()) {
            return Err(MathError::Domain);
        }
        match points {
            [] => return Err(MathError::Construction),
            [p] => {
                self.process_one_point(p);
                if let Some([first, ..]) = tolerances {
                    self.enlarge(*first);
                }
                return Ok(());
            }
            [first, .., last] if points.len() == 2 => {
                let (tol1, tol2) = match tolerances {
                    Some([a, .., b]) => (*a, *b),
                    _ => (0.0, 0.0),
                };
                let p1 = *first.xyz();
                let p2 = *last.xyz();
                let dp = sub(&p2, &p1);
                let dpm = dp.modulus();
                self.is_aabox = false;
                let h = cmax(tol1, tol2);
                self.hdims[1] = h;
                self.hdims[2] = h;
                if dpm < CONFUSION {
                    self.process_one_point(first);
                    self.enlarge(self.hdims[1] + CONFUSION);
                    return Ok(());
                }
                self.hdims[0] = 0.5 * (dpm + tol1 + tol2);
                self.axes[0] = dp.divided(dpm);
                if self.axes[0].x().abs() > self.axes[0].y().abs() {
                    self.axes[1] = XYZ::new(-self.axes[0].z(), 0.0, self.axes[0].x());
                } else {
                    self.axes[1] = XYZ::new(0.0, -self.axes[0].z(), self.axes[0].y());
                }
                self.axes[2] = normalized(&self.axes[0].crossed(&self.axes[1]));
                self.center = add(&p1, &self.axes[0].multiplied(0.5 * (dpm - tol1 + tol2)));
                return Ok(());
            }
            _ => {}
        }
        let mut tool = ObbTool::new(points, tolerances, optimal)?;
        tool.process_di_tetrahedron()?;
        tool.build_box(self)
    }

    fn process_one_point(&mut self, p: &Pnt) {
        self.is_aabox = true;
        self.hdims = [0.0; 3];
        self.axes[0] = XYZ::new(1.0, 0.0, 0.0);
        self.axes[1] = XYZ::new(0.0, 1.0, 0.0);
        self.axes[2] = XYZ::new(0.0, 0.0, 1.0);
        self.center = *p.xyz();
    }

    pub fn set_center(&mut self, c: &Pnt) {
        self.center = *c.xyz();
    }
    pub fn set_x_component(&mut self, d: &Dir, hx: f64) {
        self.axes[0] = *d.xyz();
        self.hdims[0] = hx;
    }
    pub fn set_y_component(&mut self, d: &Dir, hy: f64) {
        self.axes[1] = *d.xyz();
        self.hdims[1] = hy;
    }
    pub fn set_z_component(&mut self, d: &Dir, hz: f64) {
        self.axes[2] = *d.xyz();
        self.hdims[2] = hz;
    }
    /// `Position()`: `gp_Ax3(myCenter, ZDirection(), XDirection())`.
    pub fn position(&self) -> Result<Ax3, MathError> {
        let z = Dir::from_xyz(self.axes[2]).map_err(|_| MathError::Construction)?;
        let x = Dir::from_xyz(self.axes[0]).map_err(|_| MathError::Construction)?;
        Ax3::new(Pnt::from_xyz(self.center), z, x).map_err(|_| MathError::Construction)
    }
    pub fn center(&self) -> &XYZ {
        &self.center
    }
    pub fn x_direction(&self) -> &XYZ {
        &self.axes[0]
    }
    pub fn y_direction(&self) -> &XYZ {
        &self.axes[1]
    }
    pub fn z_direction(&self) -> &XYZ {
        &self.axes[2]
    }
    pub fn x_hsize(&self) -> f64 {
        self.hdims[0]
    }
    pub fn y_hsize(&self) -> f64 {
        self.hdims[1]
    }
    pub fn z_hsize(&self) -> f64 {
        self.hdims[2]
    }
    pub fn half_sizes(&self) -> (f64, f64, f64) {
        (self.hdims[0], self.hdims[1], self.hdims[2])
    }
    pub fn is_void(&self) -> bool {
        (self.hdims[0] < 0.0) || (self.hdims[1] < 0.0) || (self.hdims[2] < 0.0)
    }
    pub fn set_void(&mut self) {
        *self = Self::new();
    }
    pub fn set_aabox(&mut self, flag: bool) {
        self.is_aabox = flag;
    }
    pub fn is_aabox(&self) -> bool {
        self.is_aabox
    }
    pub fn enlarge(&mut self, gap: f64) {
        let g = gap.abs();
        self.hdims[0] += g;
        self.hdims[1] += g;
        self.hdims[2] += g;
    }
    /// `GetVertex`: `None` for a void box.
    pub fn vertices(&self) -> Option<[Pnt; 8]> {
        if self.is_void() {
            return None;
        }
        let h = &self.hdims;
        let a = &self.axes;
        let vertex = |sx: f64, sy: f64, sz: f64| -> Pnt {
            // ((center +- hx * ax) +- hy * ay) +- hz * az, evaluated from the left.
            let step = |acc: XYZ, s: f64, hd: f64, ax: &XYZ| -> XYZ {
                let term = ax.multiplied(hd);
                if s < 0.0 { sub(&acc, &term) } else { add(&acc, &term) }
            };
            let p = step(self.center, sx, h[0], &a[0]);
            let p = step(p, sy, h[1], &a[1]);
            let p = step(p, sz, h[2], &a[2]);
            Pnt::from_xyz(p)
        };
        Some([
            vertex(-1.0, -1.0, -1.0),
            vertex(1.0, -1.0, -1.0),
            vertex(-1.0, 1.0, -1.0),
            vertex(1.0, 1.0, -1.0),
            vertex(-1.0, -1.0, 1.0),
            vertex(1.0, -1.0, 1.0),
            vertex(-1.0, 1.0, 1.0),
            vertex(1.0, 1.0, 1.0),
        ])
    }
    pub fn square_extent(&self) -> f64 {
        4.0 * (self.hdims[0] * self.hdims[0] + self.hdims[1] * self.hdims[1] + self.hdims[2] * self.hdims[2])
    }

    /// `IsOut(const Bnd_OBB&)`.
    pub fn is_out_obb(&self, other: &Self) -> bool {
        if self.is_void() || other.is_void() {
            return true;
        }
        if self.is_aabox && other.is_aabox {
            return (other.center.x() - self.center.x()).abs() > other.hdims[0] + self.hdims[0]
                || (other.center.y() - self.center.y()).abs() > other.hdims[1] + self.hdims[1]
                || (other.center.z() - self.center.z()).abs() > other.hdims[2] + self.hdims[2];
        }
        let d = sub(&other.center, &self.center);
        for (axis, hdim) in self.axes.iter().zip(&self.hdims) {
            let lsegm2 = half_extent(&other.hdims, &other.axes, axis);
            let dist_cc = d.dot(axis).abs();
            if dist_cc > hdim + lsegm2 {
                return true;
            }
        }
        for (axis, hdim) in other.axes.iter().zip(&other.hdims) {
            let lsegm1 = half_extent(&self.hdims, &self.axes, axis);
            let dist_cc = d.dot(axis).abs();
            if dist_cc > lsegm1 + hdim {
                return true;
            }
        }
        let tol_null = f64::EPSILON;
        for axis_i in &self.axes {
            for axis_j in &other.axes {
                let laxe = axis_i.crossed(axis_j);
                let norm = laxe.modulus();
                if norm < tol_null {
                    continue;
                }
                let laxe = laxe.divided(norm);
                let lsegm1 = half_extent(&self.hdims, &self.axes, &laxe);
                let lsegm2 = half_extent(&other.hdims, &other.axes, &laxe);
                let dist_cc = d.dot(&laxe).abs();
                if dist_cc > lsegm1 + lsegm2 {
                    return true;
                }
            }
        }
        false
    }
    /// `IsOut(const gp_Pnt&)`.
    pub fn is_out_point(&self, p: &Pnt) -> bool {
        let rv = sub(p.xyz(), &self.center);
        self.axes[0].dot(&rv).abs() > self.hdims[0]
            || self.axes[1].dot(&rv).abs() > self.hdims[1]
            || self.axes[2].dot(&rv).abs() > self.hdims[2]
    }
    pub fn contains(&self, p: &Pnt) -> bool {
        !self.is_out_point(p)
    }
    pub fn intersects(&self, other: &Self) -> bool {
        !self.is_out_obb(other)
    }
    pub fn is_completely_inside(&self, other: &Self) -> bool {
        if self.is_void() || other.is_void() {
            return false;
        }
        let Some(verts) = other.vertices() else {
            return false;
        };
        for v in &verts {
            if self.is_out_point(v) {
                return false;
            }
        }
        true
    }
    /// `Add(const Bnd_OBB&)`.
    pub fn add_obb(&mut self, other: &Self) {
        if !other.is_void() {
            if self.is_void() {
                self.center = other.center;
                self.axes = other.axes;
                self.hdims = other.hdims;
                self.is_aabox = other.is_aabox;
            } else {
                let (Some(a), Some(b)) = (self.vertices(), other.vertices()) else {
                    return;
                };
                let list: Vec<Pnt> = a.iter().chain(b.iter()).copied().collect();
                // A list of 16 points cannot fail to build.
                let _ = self.rebuild(&list, None, false);
            }
        }
    }
    /// `Add(const gp_Pnt&)`.
    pub fn add_point(&mut self, p: &Pnt) {
        if self.is_void() {
            self.center = *p.xyz();
            self.axes[0] = XYZ::new(1.0, 0.0, 0.0);
            self.axes[1] = XYZ::new(0.0, 1.0, 0.0);
            self.axes[2] = XYZ::new(0.0, 0.0, 1.0);
            self.hdims = [0.0; 3];
            self.is_aabox = true;
        } else {
            let Some(verts) = self.vertices() else {
                return;
            };
            let mut list: Vec<Pnt> = verts.to_vec();
            list.push(*p);
            // A list of 9 points cannot fail to build.
            let _ = self.rebuild(&list, None, false);
        }
    }
}

impl Default for BndObb {
    fn default() -> Self {
        Self::new()
    }
}

/// `OBBTool`: DiTO-14 construction of an oriented box.
struct ObbTool<'a> {
    pnts_list: &'a [Pnt],
    list_of_tolers: Option<&'a [f64]>,
    tri_idx: [i32; 5],
    extremal_points: [XYZ; NB_EXTREMAL_POINTS],
    axes: [XYZ; 3],
    quality_criterion: f64,
    optimal: bool,
    point_box_set: Option<PointBvh>,
    /// `(_ParamMin, _ParamMax)` of every pair of extremal points.
    params: [[(f64, f64); NB_EXTREMAL_POINTS]; NB_EXTREMAL_POINTS],
}

/// `SetMinMax`.
fn set_min_max(prm: &mut [f64], new_param: f64) {
    if let [lo, hi] = prm {
        if new_param < *lo {
            *lo = new_param;
        }
        if new_param > *hi {
            *hi = new_param;
        }
    }
}

/// `OBBTool::ComputeQuality`.
fn compute_quality(prm: &[f64; 6]) -> f64 {
    let dx = prm[1] - prm[0];
    let dy = prm[3] - prm[2];
    let dz = prm[5] - prm[4];
    dx * dy + dy * dz + dx * dz
}

impl<'a> ObbTool<'a> {
    fn new(points: &'a [Pnt], tolerances: Option<&'a [f64]>, optimal: bool) -> Result<Self, MathError> {
        let mut tool = Self {
            pnts_list: points,
            list_of_tolers: tolerances,
            tri_idx: [0; 5],
            extremal_points: [XYZ::zero(); NB_EXTREMAL_POINTS],
            axes: [XYZ::zero(); 3],
            quality_criterion: f64::MAX,
            optimal,
            point_box_set: None,
            params: [[(f64::MAX, -f64::MAX); NB_EXTREMAL_POINTS]; NB_EXTREMAL_POINTS],
        };
        if optimal {
            let xyz: Vec<XYZ> = points.iter().map(|p| *p.xyz()).collect();
            let tols: Vec<f64> = match tolerances {
                Some(t) => t.to_vec(),
                None => vec![CONFUSION; points.len()],
            };
            tool.point_box_set = Some(PointBvh::build(&xyz, &tols)?);
        }
        tool.compute_extreme_points()?;
        Ok(tool)
    }

    fn extremal(&self, i: usize) -> Result<XYZ, MathError> {
        self.extremal_points.get(i).copied().ok_or(MathError::Range)
    }

    fn tri(&self, i: usize) -> Result<usize, MathError> {
        let v = *self.tri_idx.get(i).ok_or(MathError::Range)?;
        usize::try_from(v).map_err(|_| MathError::Range)
    }

    fn compute_extreme_points(&mut self) -> Result<(), MathError> {
        let initial_axes: [XYZ; NB_INIT_AXES] = [
            XYZ::new(1.0, 0.0, 0.0),
            XYZ::new(0.0, 1.0, 0.0),
            XYZ::new(0.0, 0.0, 1.0),
            XYZ::new(1.0, 1.0, 1.0).divided(SQRT_3),
            XYZ::new(1.0, 1.0, -1.0).divided(SQRT_3),
            XYZ::new(1.0, -1.0, 1.0).divided(SQRT_3),
            XYZ::new(1.0, -1.0, -1.0).divided(SQRT_3),
        ];
        let mut prm_ind = 0usize;
        for axis in &initial_axes {
            let min_ind = prm_ind;
            let max_ind = prm_ind + 1;
            prm_ind += 2;
            let (_, _, pmin, pmax) = self.project(axis)?;
            if let Some(p) = pmin {
                *self.extremal_points.get_mut(min_ind).ok_or(MathError::Range)? = p;
            }
            if let Some(p) = pmax {
                *self.extremal_points.get_mut(max_ind).ok_or(MathError::Range)? = p;
            }
        }
        if !self.optimal {
            self.tri_idx = [i32::MAX; 5];
            let mut max_sq_dist = -1.0;
            let mut prm = 0usize;
            while prm < NB_EXTREMAL_POINTS {
                let p1 = Pnt::from_xyz(self.extremal(prm)?);
                let p2 = Pnt::from_xyz(self.extremal(prm + 1)?);
                let sq_dist = p1.square_distance(&p2);
                if sq_dist > max_sq_dist {
                    max_sq_dist = sq_dist;
                    self.tri_idx[0] = i32::try_from(prm).map_err(|_| MathError::Range)?;
                    self.tri_idx[1] = i32::try_from(prm + 1).map_err(|_| MathError::Range)?;
                }
                prm += 2;
            }
            self.fill_to_triangle3()?;
        }
        Ok(())
    }

    fn fill_to_triangle3(&mut self) -> Result<(), MathError> {
        let p0 = self.extremal(self.tri(0)?)?;
        let axis = sub(&self.extremal(self.tri(1)?)?, &p0);
        let mut max_sq_dist = -1.0;
        for i in 0..NB_EXTREMAL_POINTS {
            let ii = i32::try_from(i).map_err(|_| MathError::Range)?;
            if ii == self.tri_idx[0] || ii == self.tri_idx[1] {
                continue;
            }
            let p = self.extremal(i)?;
            let dist_to_axe = axis.cross_square_magnitude(&sub(&p, &p0));
            if dist_to_axe > max_sq_dist {
                self.tri_idx[2] = ii;
                max_sq_dist = dist_to_axe;
            }
        }
        Ok(())
    }

    fn fill_to_triangle5(&mut self, normal: &XYZ, bary_center: &XYZ) -> Result<(), MathError> {
        let mut params = [0.0f64, 0.0f64];
        let mut id3: i32 = -1;
        let mut id4: i32 = -1;
        for pt_idx in 0..NB_EXTREMAL_POINTS {
            let ii = i32::try_from(pt_idx).map_err(|_| MathError::Range)?;
            if ii == self.tri_idx[0] || ii == self.tri_idx[1] || ii == self.tri_idx[2] {
                continue;
            }
            let curr = self.extremal(pt_idx)?;
            let param = normal.dot(&sub(&curr, bary_center));
            if param < params[0] {
                id3 = ii;
                params[0] = param;
            } else if param > params[1] {
                id4 = ii;
                params[1] = param;
            }
        }
        if id3 >= 0 && params[0] < -CONFUSION {
            self.tri_idx[3] = id3;
        }
        if id4 >= 0 && params[1] > CONFUSION {
            self.tri_idx[4] = id4;
        }
        Ok(())
    }

    /// `Project`: `(min, max, point of the min, point of the max)`; the points are `None` when
    /// the C++ leaves the output argument untouched.
    fn project(&self, axis: &XYZ) -> Result<(f64, f64, Option<XYZ>, Option<XYZ>), MathError> {
        if self.optimal {
            let set = self.point_box_set.as_ref().ok_or(MathError::NotDone)?;
            let (min, max, pmin, pmax) = set.extreme_points(axis)?;
            Ok((min, max, Some(pmin), Some(pmax)))
        } else {
            let mut min = f64::MAX;
            let mut max = -f64::MAX;
            let mut pmin = None;
            let mut pmax = None;
            for p in self.pnts_list {
                let point = p.xyz();
                let prm = axis.dot(point);
                if prm < min {
                    min = prm;
                    pmin = Some(*point);
                }
                if prm > max {
                    max = prm;
                    pmax = Some(*point);
                }
            }
            Ok((min, max, pmin, pmax))
        }
    }

    fn find_min_max(&self, axis: &XYZ) -> Result<(f64, f64), MathError> {
        if self.optimal {
            let (min, max, _, _) = self.project(axis)?;
            Ok((min, max))
        } else {
            let mut min = f64::MAX;
            let mut max = -f64::MAX;
            for p in &self.extremal_points {
                let prm = axis.dot(p);
                if prm < min {
                    min = prm;
                }
                if prm > max {
                    max = prm;
                }
            }
            Ok((min, max))
        }
    }

    fn compute_params(&mut self, id1: usize, id2: usize) -> Result<(f64, f64), MathError> {
        let (mut min, mut max) = *self.params.get(id1).and_then(|r| r.get(id2)).ok_or(MathError::Range)?;
        if min > max {
            let axis = normalized(&sub(&self.extremal(id1)?, &self.extremal(id2)?));
            (min, max) = self.find_min_max(&axis)?;
            *self.params.get_mut(id1).and_then(|r| r.get_mut(id2)).ok_or(MathError::Range)? = (min, max);
            *self.params.get_mut(id2).and_then(|r| r.get_mut(id1)).ok_or(MathError::Range)? = (min, max);
        }
        Ok((min, max))
    }

    fn process_triangle(&mut self, idx1: usize, idx2: usize, idx3: usize, is_built_trg: bool) -> Result<(), MathError> {
        let id1 = [idx2, idx3, idx1];
        let id2 = [idx1, idx2, idx3];
        let y_axis = [
            sub(&self.extremal(id1[0])?, &self.extremal(id2[0])?),
            sub(&self.extremal(id1[1])?, &self.extremal(id2[1])?),
            sub(&self.extremal(id1[2])?, &self.extremal(id2[2])?),
        ];
        let z_axis = y_axis[0].crossed(&y_axis[1]);
        let sq_mod = z_axis.square_modulus();
        if sq_mod < SQUARE_CONFUSION {
            return Ok(());
        }
        let z_axis = z_axis.divided(sq_mod.sqrt());
        let mut x_axis = [XYZ::zero(); 3];
        let mut x_axis_valid = [false; 3];
        for (y, (x, valid)) in y_axis.iter().zip(x_axis.iter_mut().zip(x_axis_valid.iter_mut())) {
            let cross = y.crossed(&z_axis);
            let cross_sq_mod = cross.square_modulus();
            if cross_sq_mod < SQUARE_CONFUSION {
                *valid = false;
                continue;
            }
            *x = cross.divided(cross_sq_mod.sqrt());
            *valid = true;
        }
        if is_built_trg {
            self.fill_to_triangle5(&z_axis, &self.extremal(idx1)?)?;
        }
        let mut params = [0.0f64; 6];
        let (z_min, z_max) = self.find_min_max(&z_axis)?;
        params[4] = z_min;
        params[5] = z_max;
        let mut min_idx: Option<usize> = None;
        for (axe_ind, (ax, valid)) in x_axis.iter().zip(x_axis_valid).enumerate() {
            if !valid {
                continue;
            }
            let (x_min, x_max) = self.find_min_max(ax)?;
            params[0] = x_min;
            params[1] = x_max;
            let (y_min, y_max) = self.compute_params(
                *id1.get(axe_ind).ok_or(MathError::Range)?,
                *id2.get(axe_ind).ok_or(MathError::Range)?,
            )?;
            params[2] = y_min;
            params[3] = y_max;
            let area = compute_quality(&params);
            if area < self.quality_criterion {
                self.quality_criterion = area;
                min_idx = Some(axe_ind);
            }
        }
        let Some(min_idx) = min_idx else {
            return Ok(());
        };
        self.axes[0] = *x_axis.get(min_idx).ok_or(MathError::Range)?;
        self.axes[1] = normalized(y_axis.get(min_idx).ok_or(MathError::Range)?);
        self.axes[2] = z_axis;
        Ok(())
    }

    fn process_di_tetrahedron(&mut self) -> Result<(), MathError> {
        if self.optimal {
            for i in 0..NB_EXTREMAL_POINTS - 2 {
                for j in i + 1..NB_EXTREMAL_POINTS - 1 {
                    for k in j + 1..NB_EXTREMAL_POINTS {
                        self.process_triangle(i, j, k, false)?;
                    }
                }
            }
        } else {
            let (t0, t1, t2) = (self.tri(0)?, self.tri(1)?, self.tri(2)?);
            self.process_triangle(t0, t1, t2, true)?;
            let limit = NB_EXTREMAL_POINTS as i32;
            if self.tri_idx[3] < limit {
                let t3 = self.tri(3)?;
                self.process_triangle(t0, t1, t3, false)?;
                self.process_triangle(t1, t2, t3, false)?;
                self.process_triangle(t0, t2, t3, false)?;
            }
            if self.tri_idx[4] < limit {
                let t4 = self.tri(4)?;
                self.process_triangle(t0, t1, t4, false)?;
                self.process_triangle(t1, t2, t4, false)?;
                self.process_triangle(t0, t2, t4, false)?;
            }
        }
        Ok(())
    }

    fn build_box(&self, the_box: &mut BndObb) -> Result<(), MathError> {
        the_box.set_void();
        let is_obb =
            self.axes[0].square_modulus() * self.axes[1].square_modulus() * self.axes[2].square_modulus() > 1.0e-14;
        let dir = |axis: &XYZ, standard: Direction| -> Result<Dir, MathError> {
            if is_obb {
                Dir::from_xyz(*axis).map_err(|_| MathError::Construction)
            } else {
                Ok(Dir::from_direction(standard))
            }
        };
        let x_dir = dir(&self.axes[0], Direction::X)?;
        let y_dir = dir(&self.axes[1], Direction::Y)?;
        let z_dir = dir(&self.axes[2], Direction::Z)?;
        let mut params = [0.0f64; 6];
        let first = self.pnts_list.first().ok_or(MathError::Construction)?;
        let fcurr = first.xyz();
        let v = fcurr.dot(x_dir.xyz());
        params[0] = v;
        params[1] = v;
        let v = fcurr.dot(y_dir.xyz());
        params[2] = v;
        params[3] = v;
        let v = fcurr.dot(z_dir.xyz());
        params[4] = v;
        params[5] = v;
        if let Some(tols) = self.list_of_tolers {
            let tol = *tols.first().ok_or(MathError::Range)?;
            params[0] -= tol;
            params[1] += tol;
            params[2] -= tol;
            params[3] += tol;
            params[4] -= tol;
            params[5] += tol;
        }
        for (i, p) in self.pnts_list.iter().enumerate().skip(1) {
            let curr = p.xyz();
            let dx = curr.dot(x_dir.xyz());
            let dy = curr.dot(y_dir.xyz());
            let dz = curr.dot(z_dir.xyz());
            match self.list_of_tolers {
                None => {
                    set_min_max(params.get_mut(0..2).ok_or(MathError::Range)?, dx);
                    set_min_max(params.get_mut(2..4).ok_or(MathError::Range)?, dy);
                    set_min_max(params.get_mut(4..6).ok_or(MathError::Range)?, dz);
                }
                Some(tols) => {
                    let tol = *tols.get(i).ok_or(MathError::Range)?;
                    params[0] = cmin(params[0], dx - tol);
                    params[1] = cmax(params[1], dx + tol);
                    params[2] = cmin(params[2], dy - tol);
                    params[3] = cmax(params[3], dy + tol);
                    params[4] = cmin(params[4], dz - tol);
                    params[5] = cmax(params[5], dz + tol);
                }
            }
        }
        let hx = 0.5 * (params[1] - params[0]);
        let hy = 0.5 * (params[3] - params[2]);
        let hz = 0.5 * (params[5] - params[4]);
        let sum = add(
            &add(&x_dir.xyz().multiplied(params[1] + params[0]), &y_dir.xyz().multiplied(params[3] + params[2])),
            &z_dir.xyz().multiplied(params[5] + params[4]),
        );
        let center = sum.multiplied(0.5);
        the_box.set_center(&Pnt::from_xyz(center));
        the_box.set_x_component(&x_dir, hx);
        the_box.set_y_component(&y_dir, hy);
        the_box.set_z_component(&z_dir, hz);
        the_box.set_aabox(!is_obb);
        Ok(())
    }
}
