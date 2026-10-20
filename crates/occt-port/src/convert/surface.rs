//! `Convert_ElementarySurfaceToBSplineSurface` with `Convert_CylinderToBSplineSurface`,
//! `Convert_ConeToBSplineSurface`, `Convert_SphereToBSplineSurface` and `Convert_TorusToBSplineSurface`.

use super::{ConvertError, R, at, nb_spans};
use crate::bsplclib::util::epsilon;
use crate::gp::{self, Ax3, Cone, Cylinder, Pnt, Sphere, Torus, Trsf};
use crate::plib::Array2;
use std::f64::consts::PI;

/// The result of every elementary-surface converter: a B-spline surface (degrees, periodicity, poles, weights,
/// knots and multiplicities of each direction).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ElementarySurfaceToBSplineSurface {
    u_degree: i32,
    v_degree: i32,
    u_periodic: bool,
    v_periodic: bool,
    poles: Array2<Pnt>,
    weights: Array2<f64>,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    u_mults: Vec<i32>,
    v_mults: Vec<i32>,
}

/// The oversized pole array of the C++ constructors (`NCollection_Array2<gp_Pnt>(1, MaxNbUPoles, 1, MaxNbVPoles)`).
struct Raw {
    rows: i64,
    cols: i64,
    data: Vec<Pnt>,
}

impl Raw {
    fn new(rows: i64, cols: i64) -> Self {
        Self { rows, cols, data: vec![Pnt::new(0.0, 0.0, 0.0); usize::try_from(rows * cols).unwrap_or(0)] }
    }
    fn slot(&self, i: i64, j: i64) -> R<usize> {
        if i < 1 || j < 1 || i > self.rows || j > self.cols {
            return Err(ConvertError::OutOfRange);
        }
        usize::try_from((i - 1) * self.cols + (j - 1)).map_err(|_| ConvertError::OutOfRange)
    }
    fn set(&mut self, i: i64, j: i64, p: Pnt) -> R<()> {
        let k = self.slot(i, j)?;
        *self.data.get_mut(k).ok_or(ConvertError::OutOfRange)? = p;
        Ok(())
    }
    fn get(&self, i: i64, j: i64) -> R<Pnt> {
        self.data.get(self.slot(i, j)?).copied().ok_or(ConvertError::OutOfRange)
    }
}

/// `ComputePoles` of the four classes: the poles of the surface of revolution whose profile has the points
/// `(x[j], z[j])`, over the angle range `[u1, u2]` (spans of at most 150 degrees).
fn compute_poles(raw: &mut Raw, x: &[f64], z: &[f64], u1: f64, u2: f64) -> R<()> {
    let delta_u = u2 - u1;
    let nb_u_spans = nb_spans(delta_u)?;
    let alfa_u = delta_u / f64::from(nb_u_spans * 2);
    let nb_vp = i64::try_from(x.len()).map_err(|_| ConvertError::Argument)?;
    let mut u_start = u1;
    for j in 0..nb_vp {
        let (xj, zj) = (at(x, j + 1)?, at(z, j + 1)?);
        raw.set(1, j + 1, Pnt::new(xj * libm::cos(u_start), xj * libm::sin(u_start), zj))?;
    }
    for i in 1..=i64::from(nb_u_spans) {
        for j in 0..nb_vp {
            let (xj, zj) = (at(x, j + 1)?, at(z, j + 1)?);
            raw.set(
                2 * i,
                j + 1,
                Pnt::new(
                    xj * libm::cos(u_start + alfa_u) / libm::cos(alfa_u),
                    xj * libm::sin(u_start + alfa_u) / libm::cos(alfa_u),
                    zj,
                ),
            )?;
            raw.set(
                2 * i + 1,
                j + 1,
                Pnt::new(xj * libm::cos(u_start + 2.0 * alfa_u), xj * libm::sin(u_start + 2.0 * alfa_u), zj),
            )?;
        }
        u_start += 2.0 * alfa_u;
    }
    Ok(())
}

/// The profile `(x[j], z[j])` of the sphere between the latitudes `v1` and `v2` (at most `max_poles` points).
fn sphere_profile(r: f64, v1: f64, v2: f64, max_poles: i32) -> R<(Vec<f64>, Vec<f64>)> {
    let delta_v = v2 - v1;
    let nb_v_spans = nb_spans(delta_v)?;
    if nb_v_spans < 1 || 2 * nb_v_spans + 1 > max_poles {
        return Err(ConvertError::Argument);
    }
    let alfa_v = delta_v / f64::from(nb_v_spans * 2);
    let n = usize::try_from(2 * nb_v_spans + 1).map_err(|_| ConvertError::Argument)?;
    let mut x = vec![0.0; n];
    let mut z = vec![0.0; n];
    if let (Some(x0), Some(z0)) = (x.first_mut(), z.first_mut()) {
        *x0 = r * libm::cos(v1);
        *z0 = r * libm::sin(v1);
    }
    let mut v_start = v1;
    for i in 1..=i64::from(nb_v_spans) {
        let cos_alfa = libm::cos(alfa_v);
        super::put(&mut x, 2 * i, r * libm::cos(v_start + alfa_v) / cos_alfa)?;
        super::put(&mut z, 2 * i, r * libm::sin(v_start + alfa_v) / cos_alfa)?;
        super::put(&mut x, 2 * i + 1, r * libm::cos(v_start + 2.0 * alfa_v))?;
        super::put(&mut z, 2 * i + 1, r * libm::sin(v_start + 2.0 * alfa_v))?;
        v_start += 2.0 * alfa_v;
    }
    // the C++ stores the points at x[2i - 1] and x[2i] (0-based), i.e. at 1-based 2i and 2i + 1
    Ok((x, z))
}

/// The profile `(x[j], z[j])` of the torus between the angles `v1` and `v2` (at most `max_poles` points).
fn torus_profile(big_r: f64, r: f64, v1: f64, v2: f64, max_poles: i32) -> R<(Vec<f64>, Vec<f64>)> {
    let delta_v = v2 - v1;
    let nb_v_spans = nb_spans(delta_v)?;
    if nb_v_spans < 1 || 2 * nb_v_spans + 1 > max_poles {
        return Err(ConvertError::Argument);
    }
    let alfa_v = delta_v / f64::from(nb_v_spans * 2);
    let n = usize::try_from(2 * nb_v_spans + 1).map_err(|_| ConvertError::Argument)?;
    let mut x = vec![0.0; n];
    let mut z = vec![0.0; n];
    if let (Some(x0), Some(z0)) = (x.first_mut(), z.first_mut()) {
        *x0 = big_r + r * libm::cos(v1);
        *z0 = r * libm::sin(v1);
    }
    let mut v_start = v1;
    for i in 1..=i64::from(nb_v_spans) {
        let cos_alfa = libm::cos(alfa_v);
        super::put(&mut x, 2 * i, big_r + r * libm::cos(v_start + alfa_v) / cos_alfa)?;
        super::put(&mut z, 2 * i, r * libm::sin(v_start + alfa_v) / cos_alfa)?;
        super::put(&mut x, 2 * i + 1, big_r + r * libm::cos(v_start + 2.0 * alfa_v))?;
        super::put(&mut z, 2 * i + 1, r * libm::sin(v_start + 2.0 * alfa_v))?;
        v_start += 2.0 * alfa_v;
    }
    Ok((x, z))
}

/// The knots `first + (i - 1) * 2 * alfa` with multiplicity 2, the ends with multiplicity 3.
fn arc_knots(first: f64, alfa: f64, nb_knots: i32) -> (Vec<f64>, Vec<i32>) {
    let mut knots = Vec::new();
    let mut mults = Vec::new();
    for i in 1..=nb_knots {
        knots.push(first + f64::from((i - 1) * 2) * alfa);
        mults.push(2);
    }
    if let Some(m) = mults.first_mut() {
        *m += 1;
    }
    if let Some(m) = mults.last_mut() {
        *m += 1;
    }
    (knots, mults)
}

/// The knots `(i - 1) * 2. * M_PI / 3.` of a full period, multiplicity 2.
fn period_knots() -> (Vec<f64>, Vec<i32>) {
    ((1..=4).map(|i| f64::from(i - 1) * 2.0 * PI / 3.0).collect(), vec![2; 4])
}

/// The data of one constructor before `Finalize`.
struct Draft {
    u_degree: i32,
    v_degree: i32,
    u_periodic: bool,
    v_periodic: bool,
    raw: Raw,
    nb_u_poles: i64,
    nb_v_poles: i64,
    u_knots: Vec<f64>,
    u_mults: Vec<i32>,
    v_knots: Vec<f64>,
    v_mults: Vec<i32>,
}

/// The weight `w` of the pole `index` in a direction whose even poles have weight `even` (`W1`, `W2` of the C++).
fn w(index: i64, even: f64) -> f64 {
    if index % 2 == 0 { even } else { 1.0 }
}

impl Draft {
    /// Moves the poles into the frame of the surface (`Trsf.SetTransformation(Position, gp::XOY())`), computes the
    /// weights with `weight(i, j)` and trims the arrays to their real sizes (`Finalize`).
    fn finish(self, position: &Ax3, weight: &dyn Fn(i64, i64) -> f64) -> R<ElementarySurfaceToBSplineSurface> {
        let mut trsf = Trsf::new();
        trsf.set_transformation_between(position, &Ax3::from_ax2(&gp::xoy()));
        let rows = usize::try_from(self.nb_u_poles).map_err(|_| ConvertError::Argument)?;
        let cols = usize::try_from(self.nb_v_poles).map_err(|_| ConvertError::Argument)?;
        let mut poles = Vec::with_capacity(rows * cols);
        let mut weights = Vec::with_capacity(rows * cols);
        for i in 1..=self.nb_u_poles {
            for j in 1..=self.nb_v_poles {
                weights.push(weight(i, j));
                let mut p = self.raw.get(i, j)?;
                p.transform(&trsf);
                poles.push(p);
            }
        }
        Ok(ElementarySurfaceToBSplineSurface {
            u_degree: self.u_degree,
            v_degree: self.v_degree,
            u_periodic: self.u_periodic,
            v_periodic: self.v_periodic,
            poles: Array2::new(rows, cols, poles)?,
            weights: Array2::new(rows, cols, weights)?,
            u_knots: self.u_knots,
            v_knots: self.v_knots,
            u_mults: self.u_mults,
            v_mults: self.v_mults,
        })
    }
}

/// `abs(V2 - V1) <= abs(Epsilon(V1))`: the unchecked `DomainError` of the cylinder and the cone.
fn degenerate_v(v1: f64, v2: f64) -> bool {
    (v2 - v1).abs() <= epsilon(v1).abs()
}

/// A cylinder or a cone: `x[j]` and `z[j]` of the two profile points.
struct Ruled {
    x: [f64; 2],
    z: [f64; 2],
}

impl Ruled {
    fn cylinder(r: f64, v1: f64, v2: f64) -> Self {
        Self { x: [r, r], z: [v1, v2] }
    }
    fn cone(r: f64, a: f64, v1: f64, v2: f64) -> Self {
        Self { x: [r + v1 * libm::sin(a), r + v2 * libm::sin(a)], z: [v1 * libm::cos(a), v2 * libm::cos(a)] }
    }

    /// The trimmed constructor (`U1`, `U2`, `V1`, `V2`).
    fn trimmed(&self, position: &Ax3, u1: f64, u2: f64, v1: f64, v2: f64) -> R<ElementarySurfaceToBSplineSurface> {
        let delta_u = u2 - u1;
        if degenerate_v(v1, v2) || !(0.0..=2.0 * PI).contains(&delta_u) {
            return Err(ConvertError::Argument);
        }
        let nb_u_spans = nb_spans(delta_u)?;
        let alfa_u = delta_u / f64::from(nb_u_spans * 2);
        let nb_u_poles = 2 * nb_u_spans + 1;
        let nb_u_knots = nb_u_spans + 1;
        let mut raw = Raw::new(9, 2);
        compute_poles(&mut raw, &self.x, &self.z, u1, u2)?;
        let (u_knots, u_mults) = arc_knots(u1, alfa_u, nb_u_knots);
        let draft = Draft {
            u_degree: 2,
            v_degree: 1,
            u_periodic: false,
            v_periodic: false,
            raw,
            nb_u_poles: i64::from(nb_u_poles),
            nb_v_poles: 2,
            u_knots,
            u_mults,
            v_knots: vec![v1, v2],
            v_mults: vec![2, 2],
        };
        draft.finish(position, &|i, _| w(i, libm::cos(alfa_u)))
    }

    /// The constructor of the full surface (`V1`, `V2`), periodic in U.
    fn full(&self, position: &Ax3, v1: f64, v2: f64) -> R<ElementarySurfaceToBSplineSurface> {
        if degenerate_v(v1, v2) {
            return Err(ConvertError::Argument);
        }
        let mut raw = Raw::new(9, 2);
        compute_poles(&mut raw, &self.x, &self.z, 0.0, 2.0 * PI)?;
        let (u_knots, u_mults) = period_knots();
        let draft = Draft {
            u_degree: 2,
            v_degree: 1,
            u_periodic: true,
            v_periodic: false,
            raw,
            nb_u_poles: 6,
            nb_v_poles: 2,
            u_knots,
            u_mults,
            v_knots: vec![v1, v2],
            v_mults: vec![2, 2],
        };
        draft.finish(position, &|i, _| w(i, 0.5))
    }
}

impl ElementarySurfaceToBSplineSurface {
    /// `Convert_CylinderToBSplineSurface(Cyl, U1, U2, V1, V2)`. `Err(Argument)` where OCCT 8.0.1 has its check
    /// compiled out: `abs(v2 - v1) <= abs(Epsilon(v1))`, `u2 - u1 > 2*PI` or `u2 - u1 < 0`.
    pub fn cylinder_trimmed(c: &Cylinder, u1: f64, u2: f64, v1: f64, v2: f64) -> R<Self> {
        Ruled::cylinder(c.radius(), v1, v2).trimmed(c.position(), u1, u2, v1, v2)
    }

    /// `Convert_CylinderToBSplineSurface(Cyl, V1, V2)`: periodic in U. `Err(Argument)` for
    /// `abs(v2 - v1) <= abs(Epsilon(v1))`.
    pub fn cylinder(c: &Cylinder, v1: f64, v2: f64) -> R<Self> {
        Ruled::cylinder(c.radius(), v1, v2).full(c.position(), v1, v2)
    }

    /// `Convert_ConeToBSplineSurface(C, U1, U2, V1, V2)`; errors as `cylinder_trimmed`.
    pub fn cone_trimmed(c: &Cone, u1: f64, u2: f64, v1: f64, v2: f64) -> R<Self> {
        Ruled::cone(c.ref_radius(), c.semi_angle(), v1, v2).trimmed(c.position(), u1, u2, v1, v2)
    }

    /// `Convert_ConeToBSplineSurface(C, V1, V2)`: periodic in U; errors as `cylinder`.
    pub fn cone(c: &Cone, v1: f64, v2: f64) -> R<Self> {
        Ruled::cone(c.ref_radius(), c.semi_angle(), v1, v2).full(c.position(), v1, v2)
    }

    /// `Convert_SphereToBSplineSurface(Sph, U1, U2, V1, V2)`. `Err(Argument)` where OCCT 8.0.1 has its check compiled
    /// out: `u2 - u1 > 2*PI` or `< 0`, `v1 < -PI/2`, `v2 > PI/2`, or no V span.
    pub fn sphere_trimmed(s: &Sphere, u1: f64, u2: f64, v1: f64, v2: f64) -> R<Self> {
        let delta_u = u2 - u1;
        if !(0.0..=2.0 * PI).contains(&delta_u) || v1 < -PI / 2.0 || v2 > PI / 2.0 {
            return Err(ConvertError::Argument);
        }
        let nb_u_spans = nb_spans(delta_u)?;
        let alfa_u = delta_u / f64::from(nb_u_spans * 2);
        let delta_v = v2 - v1;
        let nb_v_spans = nb_spans(delta_v)?;
        let alfa_v = delta_v / f64::from(nb_v_spans * 2);
        let r = s.radius();
        let (x, z) = sphere_profile(r, v1, v2, 5)?;
        let mut raw = Raw::new(7, 5);
        compute_poles(&mut raw, &x, &z, u1, u2)?;
        let (u_knots, u_mults) = arc_knots(u1, alfa_u, nb_u_spans + 1);
        let (v_knots, v_mults) = arc_knots(v1, alfa_v, nb_v_spans + 1);
        let draft = Draft {
            u_degree: 2,
            v_degree: 2,
            u_periodic: false,
            v_periodic: false,
            raw,
            nb_u_poles: i64::from(2 * nb_u_spans + 1),
            nb_v_poles: i64::from(2 * nb_v_spans + 1),
            u_knots,
            u_mults,
            v_knots,
            v_mults,
        };
        draft.finish(s.position(), &|i, j| w(i, libm::cos(alfa_u)) * w(j, libm::cos(alfa_v)))
    }

    /// `Convert_SphereToBSplineSurface(Sph, Param1, Param2, UTrim)`: with `u_trim` the U range is
    /// `[param1, param2]` (V full, U not periodic), otherwise U is full and periodic and `[param1, param2]` is the
    /// V range. `Err(Argument)` for `param2 - param1 > 2*PI` or `< 0`, and for a V range of more than two spans.
    pub fn sphere_param(s: &Sphere, param1: f64, param2: f64, u_trim: bool) -> R<Self> {
        let delta = param2 - param1;
        if !(0.0..=2.0 * PI).contains(&delta) {
            return Err(ConvertError::Argument);
        }
        let r = s.radius();
        let mut raw = Raw::new(7, 5);
        if !u_trim {
            let (x, z) = sphere_profile(r, param1, param2, 5)?;
            compute_poles(&mut raw, &x, &z, 0.0, 2.0 * PI)?;
            let delta_v = param2 - param1;
            let nb_v_spans = nb_spans(delta_v)?;
            let alfa_v = delta_v / f64::from(nb_v_spans * 2);
            let (u_knots, u_mults) = period_knots();
            let (v_knots, v_mults) = arc_knots(param1, alfa_v, nb_v_spans + 1);
            let cos_v = libm::cos(alfa_v);
            let draft = Draft {
                u_degree: 2,
                v_degree: 2,
                u_periodic: true,
                v_periodic: false,
                raw,
                nb_u_poles: 6,
                nb_v_poles: i64::from(2 * nb_v_spans + 1),
                u_knots,
                u_mults,
                v_knots,
                v_mults,
            };
            draft.finish(s.position(), &|i, j| w(i, 0.5) * w(j, cos_v))
        } else {
            let (x, z) = sphere_profile(r, -PI / 2.0, PI / 2.0, 5)?;
            compute_poles(&mut raw, &x, &z, param1, param2)?;
            let delta_u = param2 - param1;
            let nb_u_spans = nb_spans(delta_u)?;
            let alfa_u = delta_u / f64::from(nb_u_spans * 2);
            let (u_knots, u_mults) = arc_knots(param1, alfa_u, nb_u_spans + 1);
            let cos_u = libm::cos(alfa_u);
            let draft = Draft {
                u_degree: 2,
                v_degree: 2,
                u_periodic: false,
                v_periodic: false,
                raw,
                nb_u_poles: i64::from(2 * nb_u_spans + 1),
                nb_v_poles: 5,
                u_knots,
                u_mults,
                v_knots: vec![-PI / 2.0, 0.0, PI / 2.0],
                v_mults: vec![3, 2, 3],
            };
            draft.finish(s.position(), &|i, j| w(i, cos_u) * w(j, 0.5))
        }
    }

    /// `Convert_SphereToBSplineSurface(Sph)`: the whole sphere, periodic in U.
    pub fn sphere(s: &Sphere) -> Self {
        Self::sphere_full(s).unwrap_or_default()
    }

    fn sphere_full(s: &Sphere) -> R<Self> {
        let r = s.radius();
        let (x, z) = sphere_profile(r, -PI / 2.0, PI / 2.0, 5)?;
        let mut raw = Raw::new(7, 5);
        compute_poles(&mut raw, &x, &z, 0.0, 2.0 * PI)?;
        let draft = Draft {
            u_degree: 2,
            v_degree: 2,
            u_periodic: true,
            v_periodic: false,
            raw,
            nb_u_poles: 6,
            nb_v_poles: 5,
            u_knots: vec![0.0, 2.0 * PI / 3.0, 4.0 * PI / 3.0, 2.0 * PI],
            u_mults: vec![2; 4],
            v_knots: vec![-PI / 2.0, 0.0, PI / 2.0],
            v_mults: vec![3, 2, 3],
        };
        draft.finish(s.position(), &|i, j| w(i, 0.5) * w(j, f64::sqrt(2.0) / 2.0))
    }

    /// `Convert_TorusToBSplineSurface(T, U1, U2, V1, V2)`. `Err(Argument)` where OCCT 8.0.1 has its check compiled
    /// out: `u2 - u1` or `v2 - v1` outside `[0, 2*PI]`.
    pub fn torus_trimmed(t: &Torus, u1: f64, u2: f64, v1: f64, v2: f64) -> R<Self> {
        let delta_u = u2 - u1;
        let delta_v = v2 - v1;
        if !(0.0..=2.0 * PI).contains(&delta_u) || !(0.0..=2.0 * PI).contains(&delta_v) {
            return Err(ConvertError::Argument);
        }
        let nb_u_spans = nb_spans(delta_u)?;
        let nb_v_spans = nb_spans(delta_v)?;
        let alfa_u = delta_u / f64::from(nb_u_spans * 2);
        let alfa_v = delta_v / f64::from(nb_v_spans * 2);
        let (x, z) = torus_profile(t.major_radius(), t.minor_radius(), v1, v2, 7)?;
        let mut raw = Raw::new(7, 7);
        compute_poles(&mut raw, &x, &z, u1, u2)?;
        let (u_knots, u_mults) = arc_knots(u1, alfa_u, nb_u_spans + 1);
        let (v_knots, v_mults) = arc_knots(v1, alfa_v, nb_v_spans + 1);
        let draft = Draft {
            u_degree: 2,
            v_degree: 2,
            u_periodic: false,
            v_periodic: false,
            raw,
            nb_u_poles: i64::from(2 * nb_u_spans + 1),
            nb_v_poles: i64::from(2 * nb_v_spans + 1),
            u_knots,
            u_mults,
            v_knots,
            v_mults,
        };
        draft.finish(t.position(), &|i, j| w(i, libm::cos(alfa_u)) * w(j, libm::cos(alfa_v)))
    }

    /// `Convert_TorusToBSplineSurface(T, Param1, Param2, UTrim)`: with `u_trim` the U range is `[param1, param2]`
    /// (V full and periodic), otherwise U is full and periodic and `[param1, param2]` is the V range. `Err(Argument)`
    /// for `param2 - param1` outside `[0, 2*PI]`.
    pub fn torus_param(t: &Torus, param1: f64, param2: f64, u_trim: bool) -> R<Self> {
        let delta = param2 - param1;
        if !(0.0..=2.0 * PI).contains(&delta) {
            return Err(ConvertError::Argument);
        }
        let big_r = t.major_radius();
        let r = t.minor_radius();
        let mut raw = Raw::new(7, 7);
        if !u_trim {
            let (x, z) = torus_profile(big_r, r, param1, param2, 7)?;
            compute_poles(&mut raw, &x, &z, 0.0, 2.0 * PI)?;
            let delta_v = param2 - param1;
            let nb_v_spans = nb_spans(delta_v)?;
            let alfa_v = delta_v / f64::from(nb_v_spans * 2);
            let (u_knots, u_mults) = period_knots();
            let (v_knots, v_mults) = arc_knots(param1, alfa_v, nb_v_spans + 1);
            let cos_v = libm::cos(alfa_v);
            let draft = Draft {
                u_degree: 2,
                v_degree: 2,
                u_periodic: true,
                v_periodic: u_trim,
                raw,
                nb_u_poles: 6,
                nb_v_poles: i64::from(2 * nb_v_spans + 1),
                u_knots,
                u_mults,
                v_knots,
                v_mults,
            };
            draft.finish(t.position(), &|i, j| w(i, 0.5) * w(j, cos_v))
        } else {
            let (x, z) = torus_profile(big_r, r, 0.0, 2.0 * PI, 7)?;
            compute_poles(&mut raw, &x, &z, param1, param2)?;
            let delta_u = param2 - param1;
            let nb_u_spans = nb_spans(delta_u)?;
            let alfa_u = delta_u / f64::from(nb_u_spans * 2);
            let (v_knots, v_mults) = period_knots();
            let (u_knots, u_mults) = arc_knots(param1, alfa_u, nb_u_spans + 1);
            let cos_u = libm::cos(alfa_u);
            let draft = Draft {
                u_degree: 2,
                v_degree: 2,
                u_periodic: false,
                v_periodic: u_trim,
                raw,
                nb_u_poles: i64::from(2 * nb_u_spans + 1),
                nb_v_poles: 6,
                u_knots,
                u_mults,
                v_knots,
                v_mults,
            };
            draft.finish(t.position(), &|i, j| w(i, cos_u) * w(j, 0.5))
        }
    }

    /// `Convert_TorusToBSplineSurface(T)`: the whole torus, periodic in both directions.
    pub fn torus(t: &Torus) -> Self {
        Self::torus_full(t).unwrap_or_default()
    }

    fn torus_full(t: &Torus) -> R<Self> {
        let (x, z) = torus_profile(t.major_radius(), t.minor_radius(), 0.0, 2.0 * PI, 7)?;
        let mut raw = Raw::new(7, 7);
        compute_poles(&mut raw, &x, &z, 0.0, 2.0 * PI)?;
        let (u_knots, u_mults) = period_knots();
        let draft = Draft {
            u_degree: 2,
            v_degree: 2,
            u_periodic: true,
            v_periodic: true,
            raw,
            nb_u_poles: 6,
            nb_v_poles: 6,
            u_knots: u_knots.clone(),
            u_mults: u_mults.clone(),
            v_knots: u_knots,
            v_mults: u_mults,
        };
        draft.finish(t.position(), &|i, j| w(i, 0.5) * w(j, 0.5))
    }

    /// The degree in U.
    pub fn u_degree(&self) -> i32 {
        self.u_degree
    }

    /// The degree in V.
    pub fn v_degree(&self) -> i32 {
        self.v_degree
    }

    /// The number of poles in U.
    pub fn nb_u_poles(&self) -> i32 {
        i32::try_from(self.poles.rows()).unwrap_or(i32::MAX)
    }

    /// The number of poles in V.
    pub fn nb_v_poles(&self) -> i32 {
        i32::try_from(self.poles.cols()).unwrap_or(i32::MAX)
    }

    /// The number of knots in U.
    pub fn nb_u_knots(&self) -> i32 {
        i32::try_from(self.u_knots.len()).unwrap_or(i32::MAX)
    }

    /// The number of knots in V.
    pub fn nb_v_knots(&self) -> i32 {
        i32::try_from(self.v_knots.len()).unwrap_or(i32::MAX)
    }

    /// Whether the surface is periodic in U.
    pub fn is_u_periodic(&self) -> bool {
        self.u_periodic
    }

    /// Whether the surface is periodic in V.
    pub fn is_v_periodic(&self) -> bool {
        self.v_periodic
    }

    /// The poles (rows are the U index).
    pub fn poles(&self) -> &Array2<Pnt> {
        &self.poles
    }

    /// The weights (same shape as the poles).
    pub fn weights(&self) -> &Array2<f64> {
        &self.weights
    }

    /// The U knots.
    pub fn u_knots(&self) -> &[f64] {
        &self.u_knots
    }

    /// The V knots.
    pub fn v_knots(&self) -> &[f64] {
        &self.v_knots
    }

    /// The U multiplicities.
    pub fn u_multiplicities(&self) -> &[i32] {
        &self.u_mults
    }

    /// The V multiplicities.
    pub fn v_multiplicities(&self) -> &[i32] {
        &self.v_mults
    }

    fn grid_index(&self, u_index: i32, v_index: i32) -> R<usize> {
        let (u, v) = (usize::try_from(u_index), usize::try_from(v_index));
        match (u, v) {
            (Ok(u), Ok(v)) if u >= 1 && v >= 1 && u <= self.poles.rows() && v <= self.poles.cols() => {
                Ok((u - 1) * self.poles.cols() + (v - 1))
            }
            _ => Err(ConvertError::OutOfRange),
        }
    }

    /// `Pole(UIndex, VIndex)`, 1-based; `Err(OutOfRange)` outside the ranges.
    pub fn pole(&self, u_index: i32, v_index: i32) -> R<Pnt> {
        let k = self.grid_index(u_index, v_index)?;
        self.poles.as_slice().get(k).copied().ok_or(ConvertError::OutOfRange)
    }

    /// `Weight(UIndex, VIndex)`, 1-based; `Err(OutOfRange)` outside the ranges.
    pub fn weight(&self, u_index: i32, v_index: i32) -> R<f64> {
        let k = self.grid_index(u_index, v_index)?;
        self.weights.as_slice().get(k).copied().ok_or(ConvertError::OutOfRange)
    }

    /// `UKnot(UIndex)`, 1-based.
    pub fn u_knot(&self, index: i32) -> R<f64> {
        at(&self.u_knots, i64::from(index))
    }

    /// `VKnot(VIndex)`, 1-based.
    pub fn v_knot(&self, index: i32) -> R<f64> {
        at(&self.v_knots, i64::from(index))
    }

    /// `UMultiplicity(UIndex)`, 1-based.
    pub fn u_multiplicity(&self, index: i32) -> R<i32> {
        at(&self.u_mults, i64::from(index))
    }

    /// `VMultiplicity(VIndex)`, 1-based.
    pub fn v_multiplicity(&self, index: i32) -> R<i32> {
        at(&self.v_mults, i64::from(index))
    }
}
