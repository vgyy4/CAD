//! `Geom_OffsetCurve` and `Geom_OffsetCurveUtils`.
use super::bspline::BSplineCurve;
use super::rep_utils::{self, EvalRepDesc};
use super::trimmed::{TrimmedCurve, make_unique};
use super::{Curve, GeomAbsShape, GeomError, Geometry, ResD1, ResD2, ResD3, Result};
use super::{angular, computational, geometry_any};
use crate::gp::{self, Dir, Pnt, Trsf, XYZ};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// `Geom_OffsetCurve`.
///
/// `Geom_Curve::IsClosed` evaluates the curve at its end points, so for an offset curve it can raise
/// `Geom_UndefinedValue`; the `bool` of [`Curve::is_closed`] cannot say so. [`OffsetCurve::try_is_closed`] is the
/// faithful form. `is_closed` answers `false` in that case and keeps the exception pending: the next fallible call
/// (`eval_*`, `is_cn`, `period`) returns it.
#[derive(Debug)]
pub struct OffsetCurve {
    basis_curve: Arc<dyn Curve>,
    eval_rep: Option<EvalRepDesc>,
    direction: Dir,
    offset_value: f64,
    basis_curve_continuity: GeomAbsShape,
    pending_error: AtomicBool,
}

impl Clone for OffsetCurve {
    fn clone(&self) -> Self {
        OffsetCurve {
            basis_curve: self.basis_curve.clone(),
            eval_rep: self.eval_rep.clone(),
            direction: self.direction,
            offset_value: self.offset_value,
            basis_curve_continuity: self.basis_curve_continuity,
            pending_error: AtomicBool::new(self.pending_error.load(Ordering::Relaxed)),
        }
    }
}

impl OffsetCurve {
    /// `Geom_OffsetCurve(theCurve, theOffset, theDir, isTheNotCheckC0)`.
    pub fn new(basis: Arc<dyn Curve>, offset: f64, v: Dir, not_check_c0: bool) -> Result<Self> {
        let mut c = OffsetCurve {
            basis_curve: basis.clone(),
            eval_rep: None,
            direction: v,
            offset_value: offset,
            basis_curve_continuity: GeomAbsShape::C0,
            pending_error: AtomicBool::new(false),
        };
        c.set_basis_curve(basis, not_check_c0)?;
        Ok(c)
    }

    /// `HasEvalRepresentation`.
    pub fn has_eval_representation(&self) -> bool {
        self.eval_rep.is_some()
    }

    /// `EvalRepresentation`.
    pub fn eval_representation(&self) -> Option<&EvalRepDesc> {
        self.eval_rep.as_ref()
    }

    /// `SetEvalRepresentation`.
    pub fn set_eval_representation(&mut self, d: Option<EvalRepDesc>) -> Result<()> {
        if let Some(desc) = d.as_ref() {
            rep_utils::validate_curve_desc(desc, self)?;
        }
        self.eval_rep = d;
        Ok(())
    }

    /// `ClearEvalRepresentation`.
    pub fn clear_eval_representation(&mut self) {
        self.eval_rep = None;
    }

    /// `SetBasisCurve`.
    pub fn set_basis_curve(&mut self, c: Arc<dyn Curve>, not_check_c0: bool) -> Result<()> {
        self.eval_rep = None;
        let uf = c.first_parameter();
        let ul = c.last_parameter();
        let mut checking: Arc<dyn Curve> = c;
        let mut is_trimmed = false;
        loop {
            let is_t = checking.as_any().is::<TrimmedCurve>();
            let is_o = checking.as_any().is::<OffsetCurve>();
            if !(is_t || is_o) {
                break;
            }
            if let Some(t) = checking.as_any().downcast_ref::<TrimmedCurve>() {
                let next = t.basis_curve();
                checking = next;
                is_trimmed = true;
            }
            if let Some(o) = checking.as_any().downcast_ref::<OffsetCurve>() {
                let next = o.basis_curve();
                let prev_off = o.offset();
                let v1 = gp::Vec::from_dir(&o.direction());
                let v2 = gp::Vec::from_dir(&self.direction);
                let mut vdir = v1.multiplied(prev_off);
                vdir.add(&v2.multiplied(self.offset_value));
                if self.offset_value >= 0.0 {
                    self.offset_value = vdir.magnitude();
                    self.direction.set_xyz(*vdir.xyz()).map_err(|_| GeomError::Argument)?;
                } else {
                    self.offset_value = -vdir.magnitude();
                    self.direction.set_xyz(*vdir.reversed().xyz()).map_err(|_| GeomError::Argument)?;
                }
                checking = next;
            }
        }
        self.basis_curve_continuity = checking.continuity();
        let mut is_c0 = !not_check_c0 && self.basis_curve_continuity == GeomAbsShape::C0;
        if is_c0 && let Some(bc) = checking.as_any().downcast_ref::<BSplineCurve>() {
            if bc.is_g1(uf, ul, angular())? {
                self.basis_curve_continuity = GeomAbsShape::G1;
                is_c0 = false;
            }
            if is_c0 {
                return Err(GeomError::Construction);
            }
        }
        self.basis_curve =
            if is_trimmed { Arc::new(TrimmedCurve::new(checking, uf, ul, true, true)?) } else { checking };
        Ok(())
    }

    /// `SetDirection`.
    pub fn set_direction(&mut self, v: Dir) {
        self.eval_rep = None;
        self.direction = v;
    }

    /// `Direction`.
    pub fn direction(&self) -> Dir {
        self.direction
    }

    /// `SetOffsetValue`.
    pub fn set_offset_value(&mut self, d: f64) {
        self.eval_rep = None;
        self.offset_value = d;
    }

    /// `BasisCurve`.
    pub fn basis_curve(&self) -> Arc<dyn Curve> {
        self.basis_curve.clone()
    }

    /// `Offset`.
    pub fn offset(&self) -> f64 {
        self.offset_value
    }

    /// `GetBasisCurveContinuity`.
    pub fn get_basis_curve_continuity(&self) -> GeomAbsShape {
        self.basis_curve_continuity
    }

    // The copy constructor
    fn copy_offset(&self) -> OffsetCurve {
        OffsetCurve {
            basis_curve: Arc::from(self.basis_curve.copy_curve()),
            eval_rep: rep_utils::clone_curve_desc(&self.eval_rep),
            direction: self.direction,
            offset_value: self.offset_value,
            basis_curve_continuity: self.basis_curve_continuity,
            pending_error: AtomicBool::new(false),
        }
    }

    /// `IsClosed` with the exception of OCCT: the end points are evaluated, an end point where the basis curve has
    /// no usable first derivative raises `UndefinedValue`.
    pub fn try_is_closed(&self) -> Result<bool> {
        let pf = self.value(self.first_parameter())?;
        let pl = self.value(self.last_parameter())?;
        Ok(pf.square_distance(&pl) <= computational())
    }

    // The exception that `is_closed` could not return.
    fn take_pending_error(&self) -> Result<()> {
        if self.pending_error.swap(false, Ordering::Relaxed) { Err(GeomError::UndefinedValue) } else { Ok(()) }
    }
}

impl Geometry for OffsetCurve {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.eval_rep = None;
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.transform(t);
        }
        let _ = self.direction.transform(t);
        self.offset_value *= t.scale_factor();
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.copy_offset())
    }
}

impl Curve for OffsetCurve {
    fn copy_curve(&self) -> Box<dyn Curve> {
        Box::new(self.copy_offset())
    }
    fn reverse(&mut self) {
        self.eval_rep = None;
        if let Some(b) = make_unique(&mut self.basis_curve) {
            b.reverse();
        }
        self.offset_value = -self.offset_value;
    }
    fn reversed_parameter(&self, u: f64) -> f64 {
        self.basis_curve.reversed_parameter(u)
    }
    fn transformed_parameter(&self, u: f64, t: &Trsf) -> f64 {
        self.basis_curve.transformed_parameter(u, t)
    }
    fn parametric_transformation(&self, t: &Trsf) -> f64 {
        self.basis_curve.parametric_transformation(t)
    }
    fn first_parameter(&self) -> f64 {
        self.basis_curve.first_parameter()
    }
    fn last_parameter(&self) -> f64 {
        self.basis_curve.last_parameter()
    }
    fn is_closed(&self) -> bool {
        match self.try_is_closed() {
            Ok(closed) => closed,
            Err(GeomError::UndefinedValue) => {
                self.pending_error.store(true, Ordering::Relaxed);
                false
            }
            // an evaluation that OCCT does without any check (garbage in, garbage out) is not an exception; at a single
            // parameter both end points are the same value, whatever it is
            Err(_) => self.first_parameter().to_bits() == self.last_parameter().to_bits(),
        }
    }
    fn is_periodic(&self) -> bool {
        self.basis_curve.is_periodic()
    }
    fn period(&self) -> Result<f64> {
        self.take_pending_error()?;
        self.basis_curve.period()
    }
    fn continuity(&self) -> GeomAbsShape {
        match self.basis_curve_continuity {
            GeomAbsShape::C0 => GeomAbsShape::C0,
            GeomAbsShape::C1 => GeomAbsShape::C0,
            GeomAbsShape::C2 => GeomAbsShape::C1,
            GeomAbsShape::C3 => GeomAbsShape::C2,
            GeomAbsShape::Cn => GeomAbsShape::Cn,
            GeomAbsShape::G1 => GeomAbsShape::G1,
            GeomAbsShape::G2 => GeomAbsShape::G2,
        }
    }
    fn is_cn(&self, n: i32) -> Result<bool> {
        self.take_pending_error()?;
        if n < 0 {
            return Err(GeomError::Range);
        }
        self.basis_curve.is_cn(n + 1)
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        self.take_pending_error()?;
        if let Some(r) = rep_utils::try_eval_curve_d0(&self.eval_rep, u)? {
            return Ok(r);
        }
        let basis_d1 = self.basis_curve.eval_d1(u)?;
        let mut value = basis_d1.point;
        if !calculate_d0(&mut value, &basis_d1.d1, self.direction.xyz(), self.offset_value) {
            return Err(GeomError::UndefinedValue);
        }
        Ok(value)
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        self.take_pending_error()?;
        if let Some(r) = rep_utils::try_eval_curve_d1(&self.eval_rep, u)? {
            return Ok(r);
        }
        let basis_d2 = self.basis_curve.eval_d2(u)?;
        let mut value = basis_d2.point;
        let mut d1 = basis_d2.d1;
        if !calculate_d1(&mut value, &mut d1, &basis_d2.d2, self.direction.xyz(), self.offset_value) {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(ResD1 { point: value, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        self.take_pending_error()?;
        if let Some(r) = rep_utils::try_eval_curve_d2(&self.eval_rep, u)? {
            return Ok(r);
        }
        let basis_d3 = self.basis_curve.eval_d3(u)?;
        let mut value = basis_d3.point;
        let (mut d1, mut d2, mut d3) = (basis_d3.d1, basis_d3.d2, basis_d3.d3);
        let mut is_direction_change = false;
        let mut dummy_d4 = gp::Vec::new(0.0, 0.0, 0.0);
        if d1.square_magnitude() <= gp::resolution()
            && !adjust_derivative(
                self.basis_curve.as_ref(),
                3,
                u,
                &mut Derivatives { d1: &mut d1, d2: &mut d2, d3: &mut d3, d4: &mut dummy_d4 },
                &mut is_direction_change,
            )?
        {
            return Err(GeomError::UndefinedDerivative);
        }
        if !calculate_d2(
            &mut value,
            &mut d1,
            &mut d2,
            &d3,
            self.direction.xyz(),
            self.offset_value,
            is_direction_change,
        ) {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(ResD2 { point: value, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        self.take_pending_error()?;
        if let Some(r) = rep_utils::try_eval_curve_d3(&self.eval_rep, u)? {
            return Ok(r);
        }
        let basis_d3 = self.basis_curve.eval_d3(u)?;
        let d4_basis = self.basis_curve.eval_dn(u, 4)?;
        let mut value = basis_d3.point;
        let (mut d1, mut d2, mut d3) = (basis_d3.d1, basis_d3.d2, basis_d3.d3);
        let mut d4 = d4_basis;
        let mut is_direction_change = false;
        if d1.square_magnitude() <= gp::resolution()
            && !adjust_derivative(
                self.basis_curve.as_ref(),
                4,
                u,
                &mut Derivatives { d1: &mut d1, d2: &mut d2, d3: &mut d3, d4: &mut d4 },
                &mut is_direction_change,
            )?
        {
            return Err(GeomError::UndefinedDerivative);
        }
        if !calculate_d3(
            &mut value,
            &mut d1,
            &mut d2,
            &mut d3,
            &d4,
            self.direction.xyz(),
            self.offset_value,
            is_direction_change,
        ) {
            return Err(GeomError::UndefinedDerivative);
        }
        Ok(ResD3 { point: value, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<gp::Vec> {
        self.take_pending_error()?;
        if n < 1 {
            return Err(GeomError::UndefinedDerivative);
        }
        if let Some(r) = rep_utils::try_eval_curve_dn(&self.eval_rep, u, n)? {
            return Ok(r);
        }
        match n {
            1 => Ok(self.eval_d1(u)?.d1),
            2 => Ok(self.eval_d2(u)?.d2),
            3 => Ok(self.eval_d3(u)?.d3),
            _ => self.basis_curve.eval_dn(u, n),
        }
    }
}

// Geom_OffsetCurveUtils::CalculateD0
fn calculate_d0(value: &mut Pnt, d1: &gp::Vec, dir: &XYZ, offset: f64) -> bool {
    let mut ndir = d1.xyz().crossed(dir);
    let r = ndir.modulus();
    if r <= gp::resolution() {
        return false;
    }
    ndir.multiply(offset / r);
    value.change_coord().add(&ndir);
    true
}

// Geom_OffsetCurveUtils::CalculateD1
fn calculate_d1(value: &mut Pnt, d1: &mut gp::Vec, d2: &gp::Vec, dir: &XYZ, offset: f64) -> bool {
    let tol = gp::resolution();
    let mut ndir = d1.xyz().crossed(dir);
    let mut dndir = d2.xyz().crossed(dir);
    let r2 = ndir.square_modulus();
    let r = r2.sqrt();
    let r3 = r * r2;
    let dr = ndir.dot(&dndir);
    if r3 <= tol {
        if r2 <= tol {
            return false;
        }
        dndir.multiply(r);
        dndir.subtract(&ndir.multiplied(dr / r));
        dndir.multiply(offset / r2);
    } else {
        dndir.multiply(offset / r);
        dndir.subtract(&ndir.multiplied(offset * dr / r3));
    }
    ndir.multiply(offset / r);
    value.change_coord().add(&ndir);
    d1.add(&gp::Vec::from_xyz(dndir));
    true
}

// Geom_OffsetCurveUtils::CalculateD2
fn calculate_d2(
    value: &mut Pnt,
    d1: &mut gp::Vec,
    d2: &mut gp::Vec,
    d3: &gp::Vec,
    dir: &XYZ,
    offset: f64,
    is_dir_change: bool,
) -> bool {
    let tol = gp::resolution();
    let mut ndir = d1.xyz().crossed(dir);
    let mut dndir = d2.xyz().crossed(dir);
    let mut d2ndir = d3.xyz().crossed(dir);
    let r2 = ndir.square_modulus();
    let r = r2.sqrt();
    let r3 = r2 * r;
    let mut r4 = r2 * r2;
    let r5 = r3 * r2;
    let dr = ndir.dot(&dndir);
    let d2r = ndir.dot(&d2ndir) + dndir.dot(&dndir);
    if r5 <= tol {
        if r4 <= tol {
            return false;
        }
        r4 = r2 * r2;
        d2ndir.subtract(&dndir.multiplied(2.0 * dr / r2));
        d2ndir.add(&ndir.multiplied(((3.0 * dr * dr) / r4) - (d2r / r2)));
        d2ndir.multiply(offset / r);
        dndir.multiply(r);
        dndir.subtract(&ndir.multiplied(dr / r));
        dndir.multiply(offset / r2);
    } else {
        d2ndir.multiply(offset / r);
        d2ndir.subtract(&dndir.multiplied(2.0 * offset * dr / r3));
        d2ndir.add(&ndir.multiplied(offset * (((3.0 * dr * dr) / r5) - (d2r / r3))));
        dndir.multiply(offset / r);
        dndir.subtract(&ndir.multiplied(offset * dr / r3));
    }
    let _ = r4;
    ndir.multiply(offset / r);
    value.change_coord().add(&ndir);
    d1.add(&gp::Vec::from_xyz(dndir));
    if is_dir_change {
        d2.reverse();
    }
    d2.add(&gp::Vec::from_xyz(d2ndir));
    true
}

// Geom_OffsetCurveUtils::CalculateD3
#[allow(clippy::too_many_arguments)]
fn calculate_d3(
    value: &mut Pnt,
    d1: &mut gp::Vec,
    d2: &mut gp::Vec,
    d3: &mut gp::Vec,
    d4: &gp::Vec,
    dir: &XYZ,
    offset: f64,
    is_dir_change: bool,
) -> bool {
    let tol = gp::resolution();
    let mut ndir = d1.xyz().crossed(dir);
    let mut dndir = d2.xyz().crossed(dir);
    let mut d2ndir = d3.xyz().crossed(dir);
    let mut d3ndir = d4.xyz().crossed(dir);
    let r2 = ndir.square_modulus();
    let r = r2.sqrt();
    let r3 = r2 * r;
    let mut r4 = r2 * r2;
    let r5 = r3 * r2;
    let r6 = r3 * r3;
    let r7 = r5 * r2;
    let dr = ndir.dot(&dndir);
    let d2r = ndir.dot(&d2ndir) + dndir.dot(&dndir);
    let d3r = ndir.dot(&d3ndir) + 3.0 * dndir.dot(&d2ndir);
    if r7 <= tol {
        if r6 <= tol {
            return false;
        }
        d3ndir.subtract(&d2ndir.multiplied(3.0 * dr / r2));
        d3ndir.subtract(&dndir.multiplied(3.0 * ((d2r / r2) + (dr * dr / r4))));
        d3ndir.add(&ndir.multiplied(6.0 * dr * dr / r4 + 6.0 * dr * d2r / r4 - 15.0 * dr * dr * dr / r6 - d3r));
        d3ndir.multiply(offset / r);
        r4 = r2 * r2;
        d2ndir.subtract(&dndir.multiplied(2.0 * dr / r2));
        d2ndir.subtract(&ndir.multiplied((3.0 * dr * dr / r4) - (d2r / r2)));
        d2ndir.multiply(offset / r);
        dndir.multiply(r);
        dndir.subtract(&ndir.multiplied(dr / r));
        dndir.multiply(offset / r2);
    } else {
        d3ndir.divide(r);
        d3ndir.subtract(&d2ndir.multiplied(3.0 * dr / r3));
        d3ndir.subtract(&dndir.multiplied(3.0 * ((d2r / r3) + (dr * dr) / r5)));
        d3ndir.add(&ndir.multiplied(6.0 * dr * dr / r5 + 6.0 * dr * d2r / r5 - 15.0 * dr * dr * dr / r7 - d3r));
        d3ndir.multiply(offset);
        d2ndir.divide(r);
        d2ndir.subtract(&dndir.multiplied(2.0 * dr / r3));
        d2ndir.subtract(&ndir.multiplied((3.0 * dr * dr / r5) - (d2r / r3)));
        d2ndir.multiply(offset);
        dndir.multiply(offset / r);
        dndir.subtract(&ndir.multiplied(offset * dr / r3));
    }
    let _ = r4;
    ndir.multiply(offset / r);
    value.change_coord().add(&ndir);
    d1.add(&gp::Vec::from_xyz(dndir));
    d2.add(&gp::Vec::from_xyz(d2ndir));
    if is_dir_change {
        d3.reverse();
    }
    d3.add(&gp::Vec::from_xyz(d3ndir));
    true
}

// The derivatives that Geom_OffsetCurveUtils::AdjustDerivative replaces.
struct Derivatives<'a> {
    d1: &'a mut gp::Vec,
    d2: &'a mut gp::Vec,
    d3: &'a mut gp::Vec,
    d4: &'a mut gp::Vec,
}

// Geom_OffsetCurveUtils::AdjustDerivative
fn adjust_derivative(
    curve: &dyn Curve,
    max_derivative: i32,
    u: f64,
    ders: &mut Derivatives<'_>,
    is_direction_change: &mut bool,
) -> Result<bool> {
    let tol = gp::resolution();
    let min_step = 1e-7;
    let max_deriv_order = 3;
    *is_direction_change = false;
    let uinfium = curve.first_parameter();
    let usupremum = curve.last_parameter();
    let division_factor = 1.0e-3;
    let du = if usupremum >= f64::MAX || uinfium <= -f64::MAX { 0.0 } else { usupremum - uinfium };
    let delta = (du * division_factor).max(min_step);
    let mut index = 1;
    let mut v;
    loop {
        index += 1;
        v = curve.eval_dn(u, index)?;
        if !(v.square_magnitude() <= tol && index < max_deriv_order) {
            break;
        }
    }
    let uu = if u - uinfium < delta { u + delta } else { u - delta };
    let p1 = curve.eval_d0(u.min(uu))?;
    let p2 = curve.eval_d0(u.max(uu))?;
    let v1 = gp::Vec::between(&p1, &p2);
    *is_direction_change = v.dot(&v1) < 0.0;
    let sign = if *is_direction_change { -1.0 } else { 1.0 };
    *ders.d1 = v.multiplied(sign);
    for i in 1..max_derivative {
        let dn = curve.eval_dn(u, index + i)?;
        let target = match i {
            1 => &mut *ders.d2,
            2 => &mut *ders.d3,
            _ => &mut *ders.d4,
        };
        *target = dn.multiplied(sign);
    }
    Ok(true)
}
