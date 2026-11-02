//! `GProp_PEquation`: the equation of the set of points (a point, a line, a plane or a box of space).

use super::{Error, PGProps, Type};
use crate::gp::{Ax3, Dir, Lin, Pln, Pnt, Vec as GpVec};
use crate::mathinteg::Status;
use crate::mathlin::Matrix;
use crate::mathlin_pkg;

/// Slot of `myAxes[0]` in [`PEquation::doubles`].
const AXES_SLOT: i64 = 15;
/// Slot of `myExtents[0]` in [`PEquation::doubles`].
const EXTENTS_SLOT: i64 = 24;

/// `GProp_PEquation`: the description of a set of points by its barycentre, principal axes and extents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PEquation {
    ty: Type,
    g: Pnt,
    v1: GpVec,
    v2: GpVec,
    v3: GpVec,
    barycentre: Pnt,
    axes: [GpVec; 3],
    extents: [f64; 3],
}

/// Column `c` (1-based) of an eigen vector matrix as a vector.
fn column(m: &Matrix, c: i32) -> GpVec {
    let at = |r: i32| m.value(r, c).unwrap_or(0.0);
    GpVec::new(at(1), at(2), at(3))
}

impl PEquation {
    /// `GProp_PEquation::GProp_PEquation(thePnts, theTol)`.
    pub fn new(pnts: &[Pnt], tol: f64) -> Self {
        let props = PGProps::new_1_arr_pnt(pnts);
        let barycentre = props.centre_of_mass();
        let mut this = Self {
            ty: Type::None,
            g: barycentre,
            v1: GpVec::zero(),
            v2: GpVec::zero(),
            v3: GpVec::zero(),
            barycentre,
            axes: [GpVec::zero(); 3],
            extents: [0.0; 3],
        };
        let anertia = props.matrix_of_inertia();
        let mut a_mat = match Matrix::new(1, 3, 1, 3) {
            Ok(m) => m,
            Err(_) => return this,
        };
        for i in 1..=3 {
            for j in 1..=3 {
                if a_mat.set_value(i, j, anertia.at(i, j)).is_err() {
                    return this;
                }
            }
        }
        let eigen = match mathlin_pkg::jacobi(&a_mat, true) {
            Ok(e) if e.status == Status::Ok => e,
            _ => {
                this.axes = [GpVec::new(1.0, 0.0, 0.0), GpVec::new(0.0, 1.0, 0.0), GpVec::new(0.0, 0.0, 1.0)];
                this.extents = [0.0; 3];
                return this;
            }
        };
        let vecs = match eigen.eigen_vectors {
            Some(v) => v,
            None => return this,
        };
        let axes = [column(&vecs, 1), column(&vecs, 2), column(&vecs, 3)];
        this.axes = axes;
        let (xg, yg, zg) = (this.g.x(), this.g.y(), this.g.z());
        let mut max = [f64::MIN; 3];
        let mut min = [f64::MAX; 3];
        for p in pnts {
            let dx = p.x() - xg;
            let dy = p.y() - yg;
            let dz = p.z() - zg;
            for ((axis, mx), mn) in axes.iter().zip(max.iter_mut()).zip(min.iter_mut()) {
                let d = dx * axis.x() + dy * axis.y() + dz * axis.z();
                if d > *mx {
                    *mx = d;
                }
                if d < *mn {
                    *mn = d;
                }
            }
        }
        let [max1, max2, max3] = max;
        let [min1, min2, min3] = min;
        let extents = [max1 - min1, max2 - min2, max3 - min3];
        this.extents = extents;
        let mut dimension = 3;
        let mut dim_code = 0;
        if extents[0].abs() <= tol {
            dimension -= 1;
            dim_code = 1;
        }
        if extents[1].abs() <= tol {
            dimension -= 1;
            dim_code = 2 * (dim_code + 1);
        }
        if extents[2].abs() <= tol {
            dimension -= 1;
            dim_code = 3 * (dim_code + 1);
        }
        let [a0, a1, a2] = axes;
        match dimension {
            0 => this.ty = Type::Point,
            1 => {
                this.ty = Type::Line;
                this.v1 = if dim_code == 4 {
                    a2
                } else if dim_code == 6 {
                    a1
                } else {
                    a0
                };
            }
            2 => {
                this.ty = Type::Plane;
                this.v1 = if dim_code == 1 {
                    a0
                } else if dim_code == 2 {
                    a1
                } else {
                    a2
                };
            }
            _ => {
                this.ty = Type::Space;
                let mut g = this.g.xyz().added(&a0.xyz().multiplied(min1));
                g = g.added(&a1.xyz().multiplied(min2));
                g = g.added(&a2.xyz().multiplied(min3));
                this.g.set_xyz(g);
                this.v1 = a0.multiplied(extents[0]);
                this.v2 = a1.multiplied(extents[1]);
                this.v3 = a2.multiplied(extents[2]);
            }
        }
        this
    }

    /// `GProp_PEquation::GetType`.
    pub fn get_type(&self) -> Type {
        self.ty
    }

    /// `GProp_PEquation::IsPlanar`.
    pub fn is_planar(&self) -> bool {
        self.ty == Type::Plane
    }

    /// `GProp_PEquation::IsLinear`.
    pub fn is_linear(&self) -> bool {
        self.ty == Type::Line
    }

    /// `GProp_PEquation::IsPoint`.
    pub fn is_point(&self) -> bool {
        self.ty == Type::Point
    }

    /// `GProp_PEquation::IsSpace`.
    pub fn is_space(&self) -> bool {
        self.ty == Type::Space
    }

    /// `GProp_PEquation::Plane`: the plane through the barycentre with the normal `v1`.
    pub fn plane(&self) -> Result<Pln, Error> {
        if !self.is_planar() {
            return Err(Error);
        }
        let dir = Dir::from_vec(&self.v1).map_err(|_| Error)?;
        Ok(Pln::new(Ax3::from_direction(self.g, dir)))
    }

    /// `GProp_PEquation::Line`: the line through the barycentre along `v1`.
    pub fn line(&self) -> Result<Lin, Error> {
        if !self.is_linear() {
            return Err(Error);
        }
        let dir = Dir::from_vec(&self.v1).map_err(|_| Error)?;
        Ok(Lin::new(self.g, dir))
    }

    /// `GProp_PEquation::Point`.
    pub fn point(&self) -> Result<Pnt, Error> {
        if !self.is_point() {
            return Err(Error);
        }
        Ok(self.g)
    }

    /// `GProp_PEquation::Box`: the origin and the three edge vectors of a box of space.
    pub fn box_(&self) -> Result<(Pnt, GpVec, GpVec, GpVec), Error> {
        if !self.is_space() {
            return Err(Error);
        }
        Ok((self.g, self.v1, self.v2, self.v3))
    }

    /// `GProp_PEquation::Barycentre`.
    pub fn barycentre(&self) -> Pnt {
        self.barycentre
    }

    /// `GProp_PEquation::PrincipalAxis(theIndex)`: the 1-based principal axis, `myAxes[theIndex - 1]`.
    ///
    /// The C++ accessor does not check the index. Outside `1..=3` it reads the neighbouring members of the object
    /// (see [`PEquation::doubles`]), and this translation does the same.
    pub fn principal_axis(&self, index: i32) -> GpVec {
        let base = AXES_SLOT + 3 * (i64::from(index) - 1);
        GpVec::new(self.slot(base), self.slot(base + 1), self.slot(base + 2))
    }

    /// `GProp_PEquation::Extent(theIndex)`: `myExtents[theIndex - 1]`, unchecked like [`PEquation::principal_axis`].
    pub fn extent(&self, index: i32) -> f64 {
        self.slot(EXTENTS_SLOT + i64::from(index) - 1)
    }

    /// The double members of the C++ object in declaration order (`myG`, `myV1`, `myV2`, `myV3`, `myBarycentre`,
    /// `myAxes[3]`, `myExtents[3]`); the `myType` word is not a double and is not counted.
    fn doubles(&self) -> Vec<f64> {
        let p = |q: &Pnt| {
            let (x, y, z) = q.coords();
            [x, y, z]
        };
        let v = |w: &GpVec| {
            let (x, y, z) = w.coords();
            [x, y, z]
        };
        let mut out = Vec::with_capacity(27);
        out.extend(p(&self.g));
        out.extend(v(&self.v1));
        out.extend(v(&self.v2));
        out.extend(v(&self.v3));
        out.extend(p(&self.barycentre));
        for a in &self.axes {
            out.extend(v(a));
        }
        out.extend(self.extents);
        out
    }

    /// The double at `slot` of [`PEquation::doubles`]; zero outside the object.
    fn slot(&self, slot: i64) -> f64 {
        usize::try_from(slot).ok().and_then(|i| self.doubles().get(i).copied()).unwrap_or(0.0)
    }
}
