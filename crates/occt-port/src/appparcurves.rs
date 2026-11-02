//! `AppParCurves` (OCCT 8.0.1): multi-point and multi-curve containers of constrained
//! approximations (3D and 2D poles per curve), and their B-spline form, translated from the C++.

use crate::bsplclib::{curve2d, curve3d};
use crate::gp::{Pnt, Pnt2d, Vec as GpVec, Vec2d};

/// Failures of the package: `Standard_OutOfRange` and a null container used as an array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// `Standard_OutOfRange`
    OutOfRange,
    /// A container that was never allocated (C++: a null handle)
    NotComputed,
}

/// Result of the fallible functions of the package.
pub type Result<T> = std::result::Result<T, Error>;

/// `AppParCurves_Constraint`, declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    /// `AppParCurves_NoConstraint`
    NoConstraint,
    /// `AppParCurves_PassPoint`
    PassPoint,
    /// `AppParCurves_TangencyPoint`
    TangencyPoint,
    /// `AppParCurves_CurvaturePoint`
    CurvaturePoint,
}

/// `AppParCurves_ConstraintCouple`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstraintCouple {
    index: i32,
    constraint: Constraint,
}

impl ConstraintCouple {
    /// `AppParCurves_ConstraintCouple()`
    pub fn new_0() -> Self {
        Self { index: -1, constraint: Constraint::NoConstraint }
    }

    /// `AppParCurves_ConstraintCouple(TheIndex, Cons)`
    pub fn new_2(the_index: i32, cons: Constraint) -> Self {
        Self { index: the_index, constraint: cons }
    }

    /// `Index()`
    pub fn index(&self) -> i32 {
        self.index
    }

    /// `Constraint()`
    pub fn constraint(&self) -> Constraint {
        self.constraint
    }

    /// `SetIndex(TheIndex)`
    pub fn set_index(&mut self, the_index: i32) {
        self.index = the_index;
    }

    /// `SetConstraint(Cons)`
    pub fn set_constraint(&mut self, cons: Constraint) {
        self.constraint = cons;
    }
}

/// `AppParCurves_MultiPoint`: the 3D points followed by the 2D points of one pole.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MultiPoint {
    tab_p: Vec<Pnt>,
    tab_p2d: Vec<Pnt2d>,
}

impl MultiPoint {
    /// `AppParCurves_MultiPoint()`
    pub fn new_0() -> Self {
        Self::default()
    }

    /// `AppParCurves_MultiPoint(NbPoles, NbPoles2d)`: default points.
    pub fn new_2_int(nb_points: i32, nb_points2d: i32) -> Self {
        Self {
            tab_p: vec![Pnt::new(0.0, 0.0, 0.0); usize::try_from(nb_points).unwrap_or(0)],
            tab_p2d: vec![Pnt2d::new(0.0, 0.0); usize::try_from(nb_points2d).unwrap_or(0)],
        }
    }

    /// `AppParCurves_MultiPoint(tabP)`
    pub fn new_1_arr_pnt(tab_p: &[Pnt]) -> Self {
        Self { tab_p: tab_p.to_vec(), tab_p2d: Vec::new() }
    }

    /// `AppParCurves_MultiPoint(tabP2d)`
    pub fn new_1_arr_pnt2d(tab_p2d: &[Pnt2d]) -> Self {
        Self { tab_p: Vec::new(), tab_p2d: tab_p2d.to_vec() }
    }

    /// `AppParCurves_MultiPoint(tabP, tabP2d)`
    pub fn new_2_arr_pnt(tab_p: &[Pnt], tab_p2d: &[Pnt2d]) -> Self {
        Self { tab_p: tab_p.to_vec(), tab_p2d: tab_p2d.to_vec() }
    }

    /// `NbPoints()`
    pub fn nb_points(&self) -> i32 {
        self.tab_p.len() as i32
    }

    /// `NbPoints2d()`
    pub fn nb_points2d(&self) -> i32 {
        self.tab_p2d.len() as i32
    }

    /// `Dimension(Index)`: 3 for the 3D points (`Index <= NbPoints`), else 2.
    pub fn dimension(&self, index: i32) -> Result<i32> {
        if index < 0 || index > self.nb_points() + self.nb_points2d() {
            return Err(Error::OutOfRange);
        }
        Ok(if index <= self.nb_points() { 3 } else { 2 })
    }

    /// `SetPoint(Index, Point)`: `1 <= Index <= NbPoints`.
    pub fn set_point(&mut self, index: i32, point: &Pnt) -> Result<()> {
        if index <= 0 || index > self.nb_points() {
            return Err(Error::OutOfRange);
        }
        *self.slot(index - 1)? = *point;
        Ok(())
    }

    /// The 3D point slot `i` (0-based).
    fn slot(&mut self, i: i32) -> Result<&mut Pnt> {
        self.tab_p.get_mut(i as usize).ok_or(Error::OutOfRange)
    }

    /// `Point(Index)`
    pub fn point(&self, index: i32) -> Result<Pnt> {
        if index <= 0 || index > self.nb_points() {
            return Err(Error::OutOfRange);
        }
        self.tab_p.get((index - 1) as usize).copied().ok_or(Error::OutOfRange)
    }

    /// `SetPoint2d(Index, Point)`: `NbPoints < Index <= NbPoints + NbPoints2d`.
    pub fn set_point2d(&mut self, index: i32, point: &Pnt2d) -> Result<()> {
        if index <= self.nb_points() || index > self.nb_points() + self.nb_points2d() {
            return Err(Error::OutOfRange);
        }
        let i = (index - self.nb_points() - 1) as usize;
        let slot = self.tab_p2d.get_mut(i).ok_or(Error::OutOfRange)?;
        *slot = *point;
        Ok(())
    }

    /// `Point2d(Index)`
    pub fn point2d(&self, index: i32) -> Result<Pnt2d> {
        if index <= self.nb_points() || index > self.nb_points() + self.nb_points2d() {
            return Err(Error::OutOfRange);
        }
        self.tab_p2d.get((index - self.nb_points() - 1) as usize).copied().ok_or(Error::OutOfRange)
    }

    /// `Transform(CuIndex, x, dx, y, dy, z, dz)`: a 3D point `P` becomes `(x + P.x dx, ...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn transform(&mut self, cu_index: i32, x: f64, dx: f64, y: f64, dy: f64, z: f64, dz: f64) -> Result<()> {
        if self.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let p = self.point(cu_index)?;
        let new_p = Pnt::new(x + p.x() * dx, y + p.y() * dy, z + p.z() * dz);
        self.set_point(cu_index, &new_p)
    }

    /// `Transform2d(CuIndex, x, dx, y, dy)`
    pub fn transform2d(&mut self, cu_index: i32, x: f64, dx: f64, y: f64, dy: f64) -> Result<()> {
        if self.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let p = self.point2d(cu_index)?;
        let new_p = Pnt2d::new(x + p.x() * dx, y + p.y() * dy);
        self.set_point2d(cu_index, &new_p)
    }
}

/// `AppParCurves_MultiCurve`: one `MultiPoint` per pole; `None` is a null container.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MultiCurve {
    tab_point: Option<Vec<MultiPoint>>,
}

/// The Bezier (`BSplCLib::D*` without knots) knot vector: `[0, 1]`, multiplicities `degree + 1`.
fn bezier_knots(degree: i32) -> ([f64; 2], [i32; 2]) {
    ([0.0, 1.0], [degree + 1, degree + 1])
}

/// Maps a `BSplCLib` error to the package error.
fn bsp_err<T, E>(r: std::result::Result<T, E>) -> Result<T> {
    r.map_err(|_| Error::OutOfRange)
}

impl MultiCurve {
    /// `AppParCurves_MultiCurve()`
    pub fn new_0() -> Self {
        Self { tab_point: None }
    }

    /// `AppParCurves_MultiCurve(NbPol)`
    pub fn new_1_int(nb_pol: i32) -> Self {
        Self { tab_point: Some(vec![MultiPoint::new_0(); usize::try_from(nb_pol).unwrap_or(0)]) }
    }

    /// The pole container (C++ `tabPoint`).
    fn tab(&self) -> Result<&Vec<MultiPoint>> {
        self.tab_point.as_ref().ok_or(Error::NotComputed)
    }

    /// `NbCurves()`: 0 for a null container; the first pole gives the count (an empty
    /// container raises, as `tabPoint->Value(1)` does).
    pub fn nb_curves(&self) -> Result<i32> {
        match self.tab_point.as_ref() {
            None => Ok(0),
            Some(t) => {
                let mp = t.first().ok_or(Error::OutOfRange)?;
                Ok(mp.nb_points() + mp.nb_points2d())
            }
        }
    }

    /// `NbPoles()`
    pub fn nb_poles(&self) -> i32 {
        self.tab_point.as_ref().map_or(0, |t| t.len() as i32)
    }

    /// `Degree()`: number of poles minus one.
    pub fn degree(&self) -> i32 {
        self.nb_poles() - 1
    }

    /// `SetNbPoles(NbPoles)`
    pub fn set_nb_poles(&mut self, nb_poles: i32) {
        self.tab_point = Some(vec![MultiPoint::new_0(); usize::try_from(nb_poles).unwrap_or(0)]);
    }

    /// `SetValue(Index, MPoint)`
    pub fn set_value(&mut self, index: i32, m_point: &MultiPoint) -> Result<()> {
        let tab = self.tab_point.as_mut().ok_or(Error::NotComputed)?;
        if index <= 0 || index as usize > tab.len() {
            return Err(Error::OutOfRange);
        }
        let slot = tab.get_mut((index - 1) as usize).ok_or(Error::OutOfRange)?;
        *slot = m_point.clone();
        Ok(())
    }

    /// `Dimension(Index)` of the curve `Index` (1..NbCurves).
    pub fn dimension(&self, cu_index: i32) -> Result<i32> {
        let tab = self.tab()?;
        let first = tab.first().ok_or(Error::OutOfRange)?;
        let nb = first.nb_points() + first.nb_points2d();
        if cu_index <= 0 || cu_index > nb {
            return Err(Error::OutOfRange);
        }
        first.dimension(cu_index)
    }

    /// `Pole(CuIndex, Nieme)`: the 3D pole `Nieme` of curve `CuIndex`.
    pub fn pole(&self, cu_index: i32, nieme: i32) -> Result<Pnt> {
        if cu_index <= 0 && nieme <= 0 {
            return Err(Error::OutOfRange);
        }
        let tab = self.tab()?;
        let mp = tab
            .get(nieme.checked_sub(1).and_then(|i| usize::try_from(i).ok()).ok_or(Error::OutOfRange)?)
            .ok_or(Error::OutOfRange)?;
        mp.point(cu_index)
    }

    /// `Pole2d(CuIndex, Nieme)`
    pub fn pole2d(&self, cu_index: i32, nieme: i32) -> Result<Pnt2d> {
        if cu_index <= 0 && nieme <= 0 {
            return Err(Error::OutOfRange);
        }
        let tab = self.tab()?;
        let mp = tab
            .get(nieme.checked_sub(1).and_then(|i| usize::try_from(i).ok()).ok_or(Error::OutOfRange)?)
            .ok_or(Error::OutOfRange)?;
        mp.point2d(cu_index)
    }

    /// `Transform(CuIndex, x, dx, y, dy, z, dz)`
    #[allow(clippy::too_many_arguments)]
    pub fn transform(&mut self, cu_index: i32, x: f64, dx: f64, y: f64, dy: f64, z: f64, dz: f64) -> Result<()> {
        if self.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let tab = self.tab_point.as_mut().ok_or(Error::NotComputed)?;
        for mp in tab.iter_mut() {
            mp.transform(cu_index, x, dx, y, dy, z, dz)?;
        }
        Ok(())
    }

    /// `Transform2d(CuIndex, x, dx, y, dy)`
    pub fn transform2d(&mut self, cu_index: i32, x: f64, dx: f64, y: f64, dy: f64) -> Result<()> {
        if self.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let tab = self.tab_point.as_mut().ok_or(Error::NotComputed)?;
        for mp in tab.iter_mut() {
            mp.transform2d(cu_index, x, dx, y, dy)?;
        }
        Ok(())
    }

    /// The 3D poles of curve `cu_index`.
    fn poles3(&self, cu_index: i32) -> Result<Vec<Pnt>> {
        self.tab()?.iter().map(|mp| mp.point(cu_index)).collect()
    }

    /// The 2D poles of curve `cu_index`.
    fn poles2(&self, cu_index: i32) -> Result<Vec<Pnt2d>> {
        self.tab()?.iter().map(|mp| mp.point2d(cu_index)).collect()
    }

    /// `Value(CuIndex, U, Pt)` (3D): the Bezier point.
    pub fn value_3_pnt(&self, cu_index: i32, u: f64) -> Result<Pnt> {
        if self.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let poles = self.poles3(cu_index)?;
        let (k, m) = bezier_knots(poles.len() as i32 - 1);
        bsp_err(curve3d::d0(u, 1, poles.len() as i32 - 1, false, &poles, None, &k, Some(&m)))
    }

    /// `Value(CuIndex, U, Pt)` (2D).
    pub fn value_3_pnt2d(&self, cu_index: i32, u: f64) -> Result<Pnt2d> {
        if self.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let poles = self.poles2(cu_index)?;
        let (k, m) = bezier_knots(poles.len() as i32 - 1);
        bsp_err(curve2d::d0(u, 1, poles.len() as i32 - 1, false, &poles, None, &k, Some(&m)))
    }

    /// `D1(CuIndex, U, Pt, V1)` (3D).
    pub fn d1_pnt(&self, cu_index: i32, u: f64) -> Result<(Pnt, GpVec)> {
        if self.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let poles = self.poles3(cu_index)?;
        let (k, m) = bezier_knots(poles.len() as i32 - 1);
        bsp_err(curve3d::d1(u, 1, poles.len() as i32 - 1, false, &poles, None, &k, Some(&m)))
    }

    /// `D1(CuIndex, U, Pt, V1)` (2D).
    pub fn d1_pnt2d(&self, cu_index: i32, u: f64) -> Result<(Pnt2d, Vec2d)> {
        if self.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let poles = self.poles2(cu_index)?;
        let (k, m) = bezier_knots(poles.len() as i32 - 1);
        bsp_err(curve2d::d1(u, 1, poles.len() as i32 - 1, false, &poles, None, &k, Some(&m)))
    }

    /// `D2(CuIndex, U, Pt, V1, V2)` (3D).
    pub fn d2_pnt(&self, cu_index: i32, u: f64) -> Result<(Pnt, GpVec, GpVec)> {
        if self.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let poles = self.poles3(cu_index)?;
        let (k, m) = bezier_knots(poles.len() as i32 - 1);
        bsp_err(curve3d::d2(u, 1, poles.len() as i32 - 1, false, &poles, None, &k, Some(&m)))
    }

    /// `D2(CuIndex, U, Pt, V1, V2)` (2D).
    pub fn d2_pnt2d(&self, cu_index: i32, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d)> {
        if self.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let poles = self.poles2(cu_index)?;
        let (k, m) = bezier_knots(poles.len() as i32 - 1);
        bsp_err(curve2d::d2(u, 1, poles.len() as i32 - 1, false, &poles, None, &k, Some(&m)))
    }
}

/// `AppParCurves_MultiBSpCurve`: a multi-curve with knots and multiplicities.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MultiBSpCurve {
    base: MultiCurve,
    knots: Vec<f64>,
    mults: Vec<i32>,
    degree: i32,
}

/// `ComputeDegree(mults, nbPoles)`: `sum(mults) - nbPoles - 1`.
fn compute_degree(mults: &[i32], nb_poles: i32) -> i32 {
    mults.iter().sum::<i32>() - nb_poles - 1
}

impl MultiBSpCurve {
    /// `AppParCurves_MultiBSpCurve()`
    pub fn new_0() -> Self {
        Self::default()
    }

    /// `AppParCurves_MultiBSpCurve(NbPol)`
    pub fn new_1(nb_pol: i32) -> Self {
        Self { base: MultiCurve::new_1_int(nb_pol), knots: Vec::new(), mults: Vec::new(), degree: 0 }
    }

    /// `AppParCurves_MultiBSpCurve(SC, Knots, Mults)`
    pub fn new_3_multi_curve(sc: &MultiCurve, knots: &[f64], mults: &[i32]) -> Self {
        let degree = compute_degree(mults, sc.nb_poles());
        Self { base: sc.clone(), knots: knots.to_vec(), mults: mults.to_vec(), degree }
    }

    /// `SetKnots(theKnots)`
    pub fn set_knots(&mut self, knots: &[f64]) {
        self.knots = knots.to_vec();
    }

    /// `SetMultiplicities(theMults)`: the degree follows the new multiplicities.
    pub fn set_multiplicities(&mut self, mults: &[i32]) {
        self.mults = mults.to_vec();
        self.degree = compute_degree(mults, self.base.nb_poles());
    }

    /// `Knots()`
    pub fn knots(&self) -> Vec<f64> {
        self.knots.clone()
    }

    /// `Multiplicities()`
    pub fn multiplicities(&self) -> Vec<i32> {
        self.mults.clone()
    }

    /// `Degree()`
    pub fn degree(&self) -> i32 {
        self.degree
    }

    /// `NbCurves()`
    pub fn nb_curves(&self) -> Result<i32> {
        self.base.nb_curves()
    }

    /// `NbPoles()`
    pub fn nb_poles(&self) -> i32 {
        self.base.nb_poles()
    }

    /// `Value(CuIndex, U, Pt)` (3D, B-spline).
    pub fn value_pnt(&self, cu_index: i32, u: f64) -> Result<Pnt> {
        if self.base.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let poles = self.base.poles3(cu_index)?;
        bsp_err(curve3d::d0(u, 0, self.degree, false, &poles, None, &self.knots, Some(&self.mults)))
    }

    /// `Value(CuIndex, U, Pt)` (2D, B-spline).
    pub fn value_pnt2d(&self, cu_index: i32, u: f64) -> Result<Pnt2d> {
        if self.base.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let poles = self.base.poles2(cu_index)?;
        bsp_err(curve2d::d0(u, 0, self.degree, false, &poles, None, &self.knots, Some(&self.mults)))
    }

    /// `D1(CuIndex, U, Pt, V1)` (3D).
    pub fn d1_pnt(&self, cu_index: i32, u: f64) -> Result<(Pnt, GpVec)> {
        if self.base.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let poles = self.base.poles3(cu_index)?;
        bsp_err(curve3d::d1(u, 0, self.degree, false, &poles, None, &self.knots, Some(&self.mults)))
    }

    /// `D1(CuIndex, U, Pt, V1)` (2D).
    pub fn d1_pnt2d(&self, cu_index: i32, u: f64) -> Result<(Pnt2d, Vec2d)> {
        if self.base.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let poles = self.base.poles2(cu_index)?;
        bsp_err(curve2d::d1(u, 0, self.degree, false, &poles, None, &self.knots, Some(&self.mults)))
    }

    /// `D2(CuIndex, U, Pt, V1, V2)` (3D).
    pub fn d2_pnt(&self, cu_index: i32, u: f64) -> Result<(Pnt, GpVec, GpVec)> {
        if self.base.dimension(cu_index)? != 3 {
            return Err(Error::OutOfRange);
        }
        let poles = self.base.poles3(cu_index)?;
        bsp_err(curve3d::d2(u, 0, self.degree, false, &poles, None, &self.knots, Some(&self.mults)))
    }

    /// `D2(CuIndex, U, Pt, V1, V2)` (2D).
    pub fn d2_pnt2d(&self, cu_index: i32, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d)> {
        if self.base.dimension(cu_index)? != 2 {
            return Err(Error::OutOfRange);
        }
        let poles = self.base.poles2(cu_index)?;
        bsp_err(curve2d::d2(u, 0, self.degree, false, &poles, None, &self.knots, Some(&self.mults)))
    }
}
