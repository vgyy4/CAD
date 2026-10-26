//! `Geom_BSplineSurface`.
use super::bezsurf::{rational, unit_weights};
use super::rep_utils::{self, SurfaceEvalRepDesc};
use super::{BSplKnotDistribution, BSplineCurve, BoundedSurface, Curve, GeomAbsShape, GeomError, Geometry};
use super::{Result, SurfResD1, SurfResD2, SurfResD3, Surface};
use super::{confusion, from_array2, geometry_any, pconfusion, to_array2};
use crate::bsplclib::util::epsilon;
use crate::bsplclib::{self, curve2d};
use crate::bsplslib::{self, Surface as BSurface};
use crate::gp::{self, Pnt, Trsf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The data of one parametric direction of a B-spline surface.
#[derive(Debug, Clone)]
struct Dir {
    deg: i32,
    periodic: bool,
    rational: bool,
    knots: Vec<f64>,
    flat: Vec<f64>,
    mults: Vec<i32>,
    knot_set: BSplKnotDistribution,
    smooth: GeomAbsShape,
    max_deriv_inv: f64,
}

/// `Geom_BSplineSurface`.
#[derive(Debug, Clone)]
pub struct BSplineSurface {
    poles: Vec<Vec<Pnt>>,
    weights: Vec<Vec<f64>>,
    eval_rep: Option<SurfaceEvalRepDesc>,
    u: Dir,
    v: Dir,
    max_deriv_inv_ok: bool,
    // The exception that `is_u_closed` / `is_v_closed` could not return: the next fallible call returns it.
    pending_error: Pending,
}

#[derive(Debug, Default)]
struct Pending(AtomicBool);

impl Clone for Pending {
    fn clone(&self) -> Self {
        Pending(AtomicBool::new(self.0.load(Ordering::Relaxed)))
    }
}

// `Array.Value(i)` of an OCCT array with lower bound 1 (the range check is compiled out of OCCT: an error here)
fn at<T: Copy>(s: &[T], i: i32) -> Result<T> {
    if i < 1 {
        return Err(GeomError::OutOfRange);
    }
    s.get((i - 1) as usize).copied().ok_or(GeomError::OutOfRange)
}

fn put<T>(s: &mut [T], i: i32, v: T) -> Result<()> {
    if i < 1 {
        return Err(GeomError::OutOfRange);
    }
    *s.get_mut((i - 1) as usize).ok_or(GeomError::OutOfRange)? = v;
    Ok(())
}

// CheckSurfaceData
#[allow(clippy::too_many_arguments)]
fn check_surface_data(
    poles: &[Vec<Pnt>],
    uknots: &[f64],
    vknots: &[f64],
    umults: &[i32],
    vmults: &[i32],
    udeg: i32,
    vdeg: i32,
    uperiodic: bool,
    vperiodic: bool,
) -> Result<()> {
    let max = BSplineSurface::max_degree();
    if udeg < 1 || udeg > max || vdeg < 1 || vdeg > max {
        return Err(GeomError::Construction);
    }
    let nb_u = poles.len();
    let nb_v = poles.first().map_or(0, Vec::len);
    if poles.iter().any(|r| r.len() != nb_v) || nb_u < 2 || nb_v < 2 {
        return Err(GeomError::Construction);
    }
    if uknots.len() != umults.len() || vknots.len() != vmults.len() {
        return Err(GeomError::Construction);
    }
    for pair in uknots.windows(2) {
        if let [a, b] = pair
            && b - a <= epsilon(a.abs())
        {
            return Err(GeomError::Construction);
        }
    }
    for pair in vknots.windows(2) {
        if let [a, b] = pair
            && b - a <= epsilon(a.abs())
        {
            return Err(GeomError::Construction);
        }
    }
    let nu = curve2d_nb_poles(udeg, uperiodic, umults)?;
    if nb_u != nu as usize {
        return Err(GeomError::Construction);
    }
    let nv = curve2d_nb_poles(vdeg, vperiodic, vmults)?;
    if nb_v != nv as usize {
        return Err(GeomError::Construction);
    }
    Ok(())
}

fn curve2d_nb_poles(deg: i32, periodic: bool, mults: &[i32]) -> Result<i32> {
    curve2d_nb(deg, periodic, mults).map_err(|_| GeomError::Construction)
}

fn curve2d_nb(deg: i32, periodic: bool, mults: &[i32]) -> std::result::Result<i32, crate::bsplclib::BSplError> {
    crate::bsplclib::curve3d::nb_poles_of(deg, periodic, mults)
}

impl Dir {
    fn new(deg: i32, periodic: bool, knots: &[f64], mults: &[i32]) -> Self {
        Dir {
            deg,
            periodic,
            rational: false,
            knots: knots.to_vec(),
            flat: Vec::new(),
            mults: mults.to_vec(),
            knot_set: BSplKnotDistribution::NonUniform,
            smooth: GeomAbsShape::C0,
            max_deriv_inv: 0.0,
        }
    }

    // updateUKnots / updateVKnots (the reset of `myMaxDerivInvOk` is done by the caller)
    fn update(&mut self) -> Result<()> {
        let (knot_set, max_knot_mult) = curve2d::knot_analysis(self.deg, self.periodic, &self.knots, &self.mults)?;
        self.knot_set = knot_set;
        if self.knot_set == BSplKnotDistribution::Uniform && !self.periodic {
            self.flat = self.knots.clone();
        } else {
            self.flat = bsplclib::knot_sequence(&self.knots, &self.mults, self.deg, self.periodic)?;
        }
        if max_knot_mult == 0 {
            self.smooth = GeomAbsShape::Cn;
        } else {
            self.smooth = match self.deg - max_knot_mult {
                0 => GeomAbsShape::C0,
                1 => GeomAbsShape::C1,
                2 => GeomAbsShape::C2,
                3 => GeomAbsShape::C3,
                _ => GeomAbsShape::C3,
            };
        }
        Ok(())
    }

    fn nb_knots(&self) -> i32 {
        self.knots.len() as i32
    }

    fn first_knot_index(&self) -> i32 {
        if self.periodic { 1 } else { bsplclib::flat::first_u_knot_index(self.deg, &self.mults).unwrap_or(0) }
    }

    fn last_knot_index(&self) -> i32 {
        if self.periodic {
            self.nb_knots()
        } else {
            bsplclib::flat::last_u_knot_index(self.deg, &self.mults).unwrap_or(0)
        }
    }

    // BSplCLib::LocateParameter(Degree, Knots, Mults, U, Periodic, FromK1, ToK2, KnotIndex, NewU)
    fn locate_range(&self, u: f64, from: i32, to: i32) -> Result<(i32, f64)> {
        let (uf, ul) =
            if self.periodic { (at(&self.knots, 1)?, at(&self.knots, self.nb_knots())?) } else { (0.0, 1.0) };
        Ok(bsplclib::flat::locate_core(&self.knots, u, self.periodic, from, to, uf, ul)?)
    }

    fn first_param(&self) -> Result<f64> {
        at(&self.flat, self.deg + 1)
    }

    fn last_param(&self) -> Result<f64> {
        at(&self.flat, self.flat.len() as i32 - self.deg)
    }

    // The common start of the Local* functions (the span index in the flat knots).
    fn local_index(&self, u: f64, from_k1: i32, to_k2: i32) -> Result<(i32, f64)> {
        if from_k1 == to_k2 || from_k1 < 1 || to_k2 > self.nb_knots() || from_k1 > to_k2 {
            return Err(GeomError::OutOfRange);
        }
        let (index, u) = bsplclib::locate_parameter_range(self.deg, &self.flat, u, self.periodic, from_k1, to_k2)?;
        let index = curve2d::flat_index(self.deg, index, &self.mults, self.periodic)?;
        Ok((index, u))
    }

    fn is_cn(&self, n: i32) -> Result<bool> {
        if n < 0 {
            return Err(GeomError::Range);
        }
        Ok(match self.smooth {
            GeomAbsShape::Cn => true,
            GeomAbsShape::C0 | GeomAbsShape::G1 => n <= 0,
            GeomAbsShape::C1 | GeomAbsShape::G2 => n <= 1,
            GeomAbsShape::C2 => n <= 2,
            GeomAbsShape::C3 => {
                if n <= 3 {
                    true
                } else {
                    n <= self.deg - curve2d::max_knot_mult(&self.mults, 2, self.mults.len() as i32 - 1)?
                }
            }
        })
    }

    // PeriodicNormalization of one parameter
    fn normalize(&self, p: f64) -> Result<f64> {
        let mut p = p;
        if self.periodic {
            let max_val = at(&self.flat, self.flat.len() as i32 - self.deg)?;
            let min_val = at(&self.flat, self.deg + 1)?;
            let eps = epsilon(p).abs();
            let period = max_val - min_val;
            if period <= eps {
                return Err(GeomError::OutOfRange);
            }
            let is_less = min_val - p > 0.0;
            let is_greater = p - max_val > 0.0;
            if is_less || is_greater {
                let d_par = if is_less { max_val - p } else { min_val - p };
                let nb_per = (d_par / period).trunc();
                p += nb_per * period;
            }
        }
        Ok(p)
    }

    // LocateU / LocateV with a normalized parameter; `strict_low_high` is the `LocateV` form of the two outer tests.
    fn locate(&self, new_u: f64, tol: f64, with_repetition: bool, v_form: bool) -> (i32, i32) {
        let the_knots: &[f64] = if with_repetition { &self.flat } else { &self.knots };
        let len = the_knots.len() as i32;
        let (Ok(first), Ok(last)) = (at(the_knots, 1), at(the_knots, len)) else {
            return (0, 0);
        };
        let ptol = tol.abs();
        if (new_u - first).abs() <= ptol {
            (1, 1)
        } else if (new_u - last).abs() <= ptol {
            (len, len)
        } else if if v_form { new_u < first - ptol } else { new_u < first } {
            (0, 1)
        } else if if v_form { new_u > last + ptol } else { new_u > last } {
            (len, len + 1)
        } else {
            let mut i1 = bsplclib::flat::hunt(the_knots, new_u).unwrap_or(1);
            i1 = i1.min(len).max(1);
            while i1 < len && at(the_knots, i1 + 1).map(|k| (k - new_u).abs() <= ptol).unwrap_or(false) {
                i1 += 1;
            }
            if at(the_knots, i1).map(|k| (k - new_u).abs() <= ptol).unwrap_or(false) { (i1, i1) } else { (i1, i1 + 1) }
        }
    }
}

impl BSplineSurface {
    fn blank(u: Dir, v: Dir) -> Self {
        BSplineSurface {
            poles: Vec::new(),
            weights: Vec::new(),
            eval_rep: None,
            u,
            v,
            max_deriv_inv_ok: false,
            pending_error: Pending::default(),
        }
    }

    fn take_pending(&self) -> Result<()> {
        if self.pending_error.0.swap(false, Ordering::Relaxed) { Err(GeomError::Construction) } else { Ok(()) }
    }

    /// `IsUClosed` with the exception OCCT raises when the iso curves cannot be built.
    pub fn try_is_u_closed(&self) -> Result<bool> {
        if self.u.periodic {
            return Ok(true);
        }
        let (u1, u2, _, _) = self.bounds();
        if u1.to_bits() == u2.to_bits() {
            return Ok(true);
        }
        match (self.u_iso_check(u1, true), self.u_iso_check(u2, true)) {
            (Ok(f), Ok(l)) => Ok(f.is_equal(&l, confusion())),
            (Err(GeomError::Construction), _) | (_, Err(GeomError::Construction)) => Err(GeomError::Construction),
            _ => Ok(false),
        }
    }

    /// `IsVClosed` with the exception OCCT raises when the iso curves cannot be built.
    pub fn try_is_v_closed(&self) -> Result<bool> {
        if self.v.periodic {
            return Ok(true);
        }
        let (_, _, v1, v2) = self.bounds();
        if v1.to_bits() == v2.to_bits() {
            return Ok(true);
        }
        match (self.v_iso_check(v1, true), self.v_iso_check(v2, true)) {
            (Ok(f), Ok(l)) => Ok(f.is_equal(&l, confusion())),
            (Err(GeomError::Construction), _) | (_, Err(GeomError::Construction)) => Err(GeomError::Construction),
            _ => Ok(false),
        }
    }

    fn dir(&self, is_u: bool) -> &Dir {
        if is_u { &self.u } else { &self.v }
    }

    #[allow(clippy::too_many_arguments)]
    /// `Geom_BSplineSurface(Poles, UKnots, VKnots, UMults, VMults, UDegree, VDegree, UPeriodic, VPeriodic)`.
    pub fn new(
        poles: &[Vec<Pnt>],
        uknots: &[f64],
        vknots: &[f64],
        umults: &[i32],
        vmults: &[i32],
        udegree: i32,
        vdegree: i32,
        uperiodic: bool,
        vperiodic: bool,
    ) -> Result<Self> {
        check_surface_data(poles, uknots, vknots, umults, vmults, udegree, vdegree, uperiodic, vperiodic)?;
        let mut s = BSplineSurface::blank(
            Dir::new(udegree, uperiodic, uknots, umults),
            Dir::new(vdegree, vperiodic, vknots, vmults),
        );
        s.poles = poles.to_vec();
        s.weights = unit_weights(poles.len(), poles.first().map_or(0, Vec::len));
        s.u.update()?;
        s.v.update()?;
        Ok(s)
    }

    #[allow(clippy::too_many_arguments)]
    /// `Geom_BSplineSurface(Poles, Weights, UKnots, VKnots, UMults, VMults, UDegree, VDegree, UPeriodic, VPeriodic)`.
    pub fn with_weights(
        poles: &[Vec<Pnt>],
        weights: &[Vec<f64>],
        uknots: &[f64],
        vknots: &[f64],
        umults: &[i32],
        vmults: &[i32],
        udegree: i32,
        vdegree: i32,
        uperiodic: bool,
        vperiodic: bool,
    ) -> Result<Self> {
        let nb_u = poles.len();
        let nb_v = poles.first().map_or(0, Vec::len);
        if weights.len() != nb_u || weights.iter().any(|r| r.len() != nb_v) {
            return Err(GeomError::Construction);
        }
        if weights.iter().flatten().any(|w| *w <= gp::resolution()) {
            return Err(GeomError::Construction);
        }
        let (urat, vrat) = rational(weights);
        check_surface_data(poles, uknots, vknots, umults, vmults, udegree, vdegree, uperiodic, vperiodic)?;
        let mut s = BSplineSurface::blank(
            Dir::new(udegree, uperiodic, uknots, umults),
            Dir::new(vdegree, vperiodic, vknots, vmults),
        );
        s.u.rational = urat;
        s.v.rational = vrat;
        s.poles = poles.to_vec();
        s.weights = if urat || vrat { weights.to_vec() } else { unit_weights(nb_u, nb_v) };
        s.u.update()?;
        s.v.update()?;
        Ok(s)
    }

    fn rat(&self) -> bool {
        self.u.rational || self.v.rational
    }

    fn nb_u(&self) -> i32 {
        self.poles.len() as i32
    }

    fn nb_v(&self) -> i32 {
        self.poles.first().map_or(0, Vec::len) as i32
    }

    fn refresh_rational(&mut self) {
        let (u, v) = rational(&self.weights);
        self.u.rational = u;
        self.v.rational = v;
    }

    fn reset_unit_weights(&mut self) {
        self.weights = unit_weights(self.poles.len(), self.poles.first().map_or(0, Vec::len));
    }

    /// `HasEvalRepresentation`.
    pub fn has_eval_representation(&self) -> bool {
        self.eval_rep.is_some()
    }

    /// `EvalRepresentation`.
    pub fn eval_representation(&self) -> Option<&SurfaceEvalRepDesc> {
        self.eval_rep.as_ref()
    }

    /// `SetEvalRepresentation`.
    pub fn set_eval_representation(&mut self, d: Option<SurfaceEvalRepDesc>) -> Result<()> {
        if let Some(desc) = d.as_ref() {
            rep_utils::validate_surface_desc(desc, self)?;
        }
        self.eval_rep = d;
        Ok(())
    }

    /// `ClearEvalRepresentation`.
    pub fn clear_eval_representation(&mut self) {
        self.eval_rep = None;
    }

    /// `ExchangeUV`.
    pub fn exchange_uv(&mut self) {
        self.eval_rep = None;
        let nu = self.poles.len();
        let nv = self.poles.first().map_or(0, Vec::len);
        let mut npoles = Vec::with_capacity(nv);
        let mut nweights = Vec::with_capacity(nv);
        for i in 0..nv {
            let mut prow = Vec::with_capacity(nu);
            let mut wrow = Vec::with_capacity(nu);
            for j in 0..nu {
                if let Some(p) = self.poles.get(j).and_then(|r| r.get(i)) {
                    prow.push(*p);
                }
                if let Some(w) = self.weights.get(j).and_then(|r| r.get(i)) {
                    wrow.push(*w);
                }
            }
            npoles.push(prow);
            nweights.push(wrow);
        }
        self.poles = npoles;
        if self.rat() {
            self.weights = nweights;
        } else {
            self.reset_unit_weights();
        }
        std::mem::swap(&mut self.u, &mut self.v);
        let _ = self.u.update();
        let _ = self.v.update();
        self.max_deriv_inv_ok = false;
    }

    // The `*Dir::update` calls reset `myMaxDerivInvOk`.
    fn update_dir(&mut self, is_u: bool) -> Result<()> {
        self.max_deriv_inv_ok = false;
        if is_u { self.u.update() } else { self.v.update() }
    }

    fn increase_degree_dir(&mut self, is_u: bool, new_deg: i32) -> Result<()> {
        let deg = self.dir(is_u).deg;
        if new_deg == deg {
            return Ok(());
        }
        if new_deg < deg || new_deg > Self::max_degree() {
            return Err(GeomError::Construction);
        }
        self.eval_rep = None;
        let r = {
            let pa = to_array2(&self.poles)?;
            let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
            let d = self.dir(is_u);
            bsplslib::increase_degree(is_u, d.deg, new_deg, d.periodic, &pa, wa.as_ref(), &d.knots, &d.mults)?
        };
        self.poles = from_array2(&r.poles);
        if self.rat() {
            self.weights = r.weights.as_ref().map(from_array2).unwrap_or_default();
        } else {
            self.reset_unit_weights();
        }
        let d = if is_u { &mut self.u } else { &mut self.v };
        d.deg = new_deg;
        d.knots = r.knots;
        d.mults = r.mults;
        self.update_dir(is_u)
    }

    /// `IncreaseDegree`.
    pub fn increase_degree(&mut self, u_degree: i32, v_degree: i32) -> Result<()> {
        self.increase_degree_dir(true, u_degree)?;
        self.increase_degree_dir(false, v_degree)
    }

    fn insert_knots_dir(&mut self, is_u: bool, knots: &[f64], mults: &[i32], tol: f64, add: bool) -> Result<()> {
        self.take_pending()?;
        let prepared = {
            let d = self.dir(is_u);
            bsplclib::prepare_insert_knots(d.deg, d.periodic, &d.knots, &d.mults, knots, Some(mults), tol, add)?
        };
        let Some((nbpoles, _nbknots)) = prepared else {
            return Err(GeomError::Construction);
        };
        let cur = if is_u { self.nb_u() } else { self.nb_v() };
        if nbpoles == cur {
            return Ok(());
        }
        self.eval_rep = None;
        let r = {
            let pa = to_array2(&self.poles)?;
            let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
            let d = self.dir(is_u);
            bsplslib::insert_knots(
                is_u,
                d.deg,
                d.periodic,
                &pa,
                wa.as_ref(),
                &d.knots,
                &d.mults,
                knots,
                Some(mults),
                tol,
                add,
            )?
        };
        self.poles = from_array2(&r.poles);
        if self.rat() {
            self.weights = r.weights.as_ref().map(from_array2).unwrap_or_default();
        } else {
            self.reset_unit_weights();
        }
        let d = if is_u { &mut self.u } else { &mut self.v };
        d.knots = r.knots;
        d.mults = r.mults;
        self.update_dir(is_u)
    }

    /// `InsertUKnots`.
    pub fn insert_u_knots(&mut self, knots: &[f64], mults: &[i32], tol: f64, add: bool) -> Result<()> {
        self.insert_knots_dir(true, knots, mults, tol, add)
    }

    /// `InsertVKnots`.
    pub fn insert_v_knots(&mut self, knots: &[f64], mults: &[i32], tol: f64, add: bool) -> Result<()> {
        self.insert_knots_dir(false, knots, mults, tol, add)
    }

    /// `InsertUKnot`.
    pub fn insert_u_knot(&mut self, u: f64, m: i32, tol: f64, add: bool) -> Result<()> {
        self.insert_knots_dir(true, &[u], &[m], tol, add)
    }

    /// `InsertVKnot`.
    pub fn insert_v_knot(&mut self, v: f64, m: i32, tol: f64, add: bool) -> Result<()> {
        self.insert_knots_dir(false, &[v], &[m], tol, add)
    }

    fn remove_knot_dir(&mut self, is_u: bool, index: i32, m: i32, tol: f64) -> Result<bool> {
        if m < 0 {
            return Ok(true);
        }
        let (i1, i2, periodic) = {
            let d = self.dir(is_u);
            (d.first_knot_index(), d.last_knot_index(), d.periodic)
        };
        if (!periodic && (index <= i1 || index >= i2)) || (periodic && (index < i1 || index > i2)) {
            return Err(GeomError::OutOfRange);
        }
        let step = at(&self.dir(is_u).mults, index)? - m;
        if step <= 0 {
            return Ok(true);
        }
        let removed = {
            let pa = to_array2(&self.poles)?;
            let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
            let d = self.dir(is_u);
            bsplslib::remove_knot(is_u, index, m, d.deg, d.periodic, &pa, wa.as_ref(), &d.knots, &d.mults, tol)?
        };
        let Some(r) = removed else {
            return Ok(false);
        };
        self.eval_rep = None;
        self.poles = from_array2(&r.poles);
        if self.rat() {
            self.weights = r.weights.as_ref().map(from_array2).unwrap_or_default();
        } else {
            self.reset_unit_weights();
        }
        let d = if is_u { &mut self.u } else { &mut self.v };
        d.knots = r.knots;
        d.mults = r.mults;
        self.max_deriv_inv_ok = false;
        self.update_dir(is_u)?;
        Ok(true)
    }

    /// `RemoveUKnot`.
    pub fn remove_u_knot(&mut self, index: i32, m: i32, tolerance: f64) -> Result<bool> {
        self.remove_knot_dir(true, index, m, tolerance)
    }

    /// `RemoveVKnot`.
    pub fn remove_v_knot(&mut self, index: i32, m: i32, tolerance: f64) -> Result<bool> {
        self.remove_knot_dir(false, index, m, tolerance)
    }

    fn increase_mult_dir(&mut self, is_u: bool, index: i32, m: i32) -> Result<()> {
        let k = [at(&self.dir(is_u).knots, index)?];
        let mm = [m - at(&self.dir(is_u).mults, index)?];
        self.insert_knots_dir(is_u, &k, &mm, epsilon(1.0), true)
    }

    fn increase_mult_range_dir(&mut self, is_u: bool, i1: i32, i2: i32, m: i32) -> Result<()> {
        if i1 < 1 || i2 > self.dir(is_u).nb_knots() || i1 > i2 {
            return Err(GeomError::OutOfRange);
        }
        let mut k = Vec::new();
        let mut mm = Vec::new();
        for i in i1..=i2 {
            k.push(at(&self.dir(is_u).knots, i)?);
            mm.push(m - at(&self.dir(is_u).mults, i)?);
        }
        self.insert_knots_dir(is_u, &k, &mm, epsilon(1.0), true)
    }

    fn increment_mult_dir(&mut self, is_u: bool, i1: i32, i2: i32, step: i32) -> Result<()> {
        if i1 < 1 || i2 > self.dir(is_u).nb_knots() || i1 > i2 {
            return Err(GeomError::OutOfRange);
        }
        let mut k = Vec::new();
        let mut mm = Vec::new();
        for i in i1..=i2 {
            k.push(at(&self.dir(is_u).knots, i)?);
            mm.push(step);
        }
        self.insert_knots_dir(is_u, &k, &mm, epsilon(1.0), true)
    }

    /// `IncreaseUMultiplicity(UIndex, M)`.
    pub fn increase_u_multiplicity(&mut self, uindex: i32, m: i32) -> Result<()> {
        self.increase_mult_dir(true, uindex, m)
    }
    /// `IncreaseUMultiplicity(FromI1, ToI2, M)`.
    pub fn increase_u_multiplicity_range(&mut self, from_i1: i32, to_i2: i32, m: i32) -> Result<()> {
        self.increase_mult_range_dir(true, from_i1, to_i2, m)
    }
    /// `IncrementUMultiplicity`.
    pub fn increment_u_multiplicity(&mut self, from_i1: i32, to_i2: i32, step: i32) -> Result<()> {
        self.increment_mult_dir(true, from_i1, to_i2, step)
    }
    /// `IncreaseVMultiplicity(VIndex, M)`.
    pub fn increase_v_multiplicity(&mut self, vindex: i32, m: i32) -> Result<()> {
        self.increase_mult_dir(false, vindex, m)
    }
    /// `IncreaseVMultiplicity(FromI1, ToI2, M)`.
    pub fn increase_v_multiplicity_range(&mut self, from_i1: i32, to_i2: i32, m: i32) -> Result<()> {
        self.increase_mult_range_dir(false, from_i1, to_i2, m)
    }
    /// `IncrementVMultiplicity`.
    pub fn increment_v_multiplicity(&mut self, from_i1: i32, to_i2: i32, step: i32) -> Result<()> {
        self.increment_mult_dir(false, from_i1, to_i2, step)
    }

    // Geom_BSplineSurface::segment
    #[allow(clippy::too_many_arguments)]
    fn segment_impl(
        &mut self,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        eps_u: f64,
        eps_v: f64,
        segment_in_u: bool,
        segment_in_v: bool,
    ) -> Result<()> {
        self.eval_rep = None;
        let mut delta_u = u2 - u1;
        if self.u.periodic {
            let period = at(&self.u.knots, self.u.nb_knots())? - at(&self.u.knots, 1)?;
            if delta_u - period > pconfusion() {
                return Err(GeomError::Domain);
            }
            if delta_u > period {
                delta_u = period;
            }
        }
        let mut delta_v = v2 - v1;
        if self.v.periodic {
            let period = at(&self.v.knots, self.v.nb_knots())? - at(&self.v.knots, 1)?;
            if delta_v - period > pconfusion() {
                return Err(GeomError::Domain);
            }
            if delta_v > period {
                delta_v = period;
            }
        }
        let nk = self.u.nb_knots();
        let (_, new_u1) = self.u.locate_range(u1, 1, nk)?;
        let (_, new_u2) = self.u.locate_range(u2, 1, nk)?;
        if segment_in_u {
            let kn = [new_u1.min(new_u2), new_u1.max(new_u2)];
            let mu = [self.u.deg, self.u.deg];
            self.insert_knots_dir(true, &kn, &mu, eps_u, true)?;
        }
        let nk = self.v.nb_knots();
        let (_, new_v1) = self.v.locate_range(v1, 1, nk)?;
        let (_, new_v2) = self.v.locate_range(v2, 1, nk)?;
        if segment_in_v {
            let kn = [new_v1.min(new_v2), new_v1.max(new_v2)];
            let mu = [self.v.deg, self.v.deg];
            self.insert_knots_dir(false, &kn, &mu, eps_v, true)?;
        }
        if self.u.periodic && segment_in_u {
            // set the origine at NewU1
            let nk = self.u.nb_knots();
            let (mut index, u) = self.u.locate_range(u1, 1, nk)?;
            if (at(&self.u.knots, index + 1)? - u).abs() <= eps_u {
                index += 1;
            }
            self.set_u_origin(index)?;
            self.set_not_periodic_dir(true)?;
        }
        let nk = self.u.nb_knots();
        let (mut index1u, u) = self.u.locate_range(new_u1, 1, nk)?;
        if (at(&self.u.knots, index1u + 1)? - u).abs() <= eps_u {
            index1u += 1;
        }
        let (mut index2u, u) = self.u.locate_range(new_u1 + delta_u, 1, nk)?;
        if (at(&self.u.knots, index2u + 1)? - u).abs() <= eps_u || index2u == index1u {
            index2u += 1;
        }
        let nbuknots = index2u - index1u + 1;
        if nbuknots < 1 {
            return Err(GeomError::OutOfRange);
        }
        let mut nuknots = Vec::new();
        let mut numults = Vec::new();
        for i in index1u..=index2u {
            nuknots.push(at(&self.u.knots, i)?);
            numults.push(at(&self.u.mults, i)?);
        }
        if segment_in_u {
            put(&mut numults, 1, self.u.deg + 1)?;
            put(&mut numults, nbuknots, self.u.deg + 1)?;
        }
        if self.v.periodic && segment_in_v {
            // set the origine at NewV1
            let nk = self.v.nb_knots();
            let (mut index, v) = self.v.locate_range(v1, 1, nk)?;
            if (at(&self.v.knots, index + 1)? - v).abs() <= eps_v {
                index += 1;
            }
            self.set_v_origin(index)?;
            self.set_not_periodic_dir(false)?;
        }
        let nk = self.v.nb_knots();
        let (mut index1v, v) = self.v.locate_range(new_v1, 1, nk)?;
        if (at(&self.v.knots, index1v + 1)? - v).abs() <= eps_v {
            index1v += 1;
        }
        let (mut index2v, v) = self.v.locate_range(new_v1 + delta_v, 1, nk)?;
        if (at(&self.v.knots, index2v + 1)? - v).abs() <= eps_v || index2v == index1v {
            index2v += 1;
        }
        let nbvknots = index2v - index1v + 1;
        if nbvknots < 1 {
            return Err(GeomError::OutOfRange);
        }
        let mut nvknots = Vec::new();
        let mut nvmults = Vec::new();
        for i in index1v..=index2v {
            nvknots.push(at(&self.v.knots, i)?);
            nvmults.push(at(&self.v.mults, i)?);
        }
        if segment_in_v {
            put(&mut nvmults, 1, self.v.deg + 1)?;
            put(&mut nvmults, nbvknots, self.v.deg + 1)?;
        }
        let mut pindex1u = bsplclib::pole_index(self.u.deg, index1u, self.u.periodic, &self.u.mults)?;
        let mut pindex2u = bsplclib::pole_index(self.u.deg, index2u, self.u.periodic, &self.u.mults)?;
        pindex1u += 1;
        pindex2u = (pindex2u + 1).min(self.nb_u());
        let nbupoles = pindex2u - pindex1u + 1;
        let mut pindex1v = bsplclib::pole_index(self.v.deg, index1v, self.v.periodic, &self.v.mults)?;
        let mut pindex2v = bsplclib::pole_index(self.v.deg, index2v, self.v.periodic, &self.v.mults)?;
        pindex1v += 1;
        pindex2v = (pindex2v + 1).min(self.nb_v());
        let nbvpoles = pindex2v - pindex1v + 1;
        if nbupoles < 1 || nbvpoles < 1 {
            return Err(GeomError::OutOfRange);
        }
        // A direction whose end multiplicities leave a single parameter (an unclamped end) cannot give the poles
        // its new multiplicities need: OCCT raises on the surface it then holds (the golden file's `throws`).
        if (segment_in_u && curve2d_nb_poles(self.u.deg, false, &numults)? != nbupoles)
            || (segment_in_v && curve2d_nb_poles(self.v.deg, false, &nvmults)? != nbvpoles)
        {
            return Err(GeomError::OutOfRange);
        }
        let mut npoles = Vec::with_capacity(nbupoles as usize);
        let mut nweights = Vec::with_capacity(nbupoles as usize);
        let rat = self.rat();
        for i in pindex1u..=pindex2u {
            let mut prow = Vec::with_capacity(nbvpoles as usize);
            let mut wrow = Vec::with_capacity(nbvpoles as usize);
            for j in pindex1v..=pindex2v {
                prow.push(self.pole(i, j)?);
                if rat {
                    wrow.push(
                        *self
                            .weights
                            .get((i - 1) as usize)
                            .and_then(|r| r.get((j - 1) as usize))
                            .ok_or(GeomError::OutOfRange)?,
                    );
                }
            }
            npoles.push(prow);
            nweights.push(wrow);
        }
        self.poles = npoles;
        if rat {
            self.weights = nweights;
        } else {
            self.reset_unit_weights();
        }
        self.u.knots = nuknots;
        self.u.mults = numults;
        self.v.knots = nvknots;
        self.v.mults = nvmults;
        self.max_deriv_inv_ok = false;
        self.u.update()?;
        self.v.update()
    }

    /// `Segment`.
    pub fn segment(&mut self, u1: f64, u2: f64, v1: f64, v2: f64, u_tolerance: f64, v_tolerance: f64) -> Result<()> {
        if u2 < u1 || v2 < v1 {
            return Err(GeomError::Domain);
        }
        let max_u = u2.abs().max(u1.abs());
        let eps_u = epsilon(max_u).max(u_tolerance);
        let max_v = v2.abs().max(v1.abs());
        let eps_v = epsilon(max_v).max(v_tolerance);
        self.segment_impl(u1, u2, v1, v2, eps_u, eps_v, true, true)
    }

    /// `CheckAndSegment`.
    pub fn check_and_segment(
        &mut self,
        u1: f64,
        u2: f64,
        v1: f64,
        v2: f64,
        u_tolerance: f64,
        v_tolerance: f64,
    ) -> Result<()> {
        if u2 < u1 || v2 < v1 {
            return Err(GeomError::Domain);
        }
        let max_u = u2.abs().max(u1.abs());
        let eps_u = epsilon(max_u).max(u_tolerance);
        let max_v = v2.abs().max(v1.abs());
        let eps_v = epsilon(max_v).max(v_tolerance);
        let seg_u =
            (u1 - at(&self.u.knots, 1)?).abs() > eps_u || (u2 - at(&self.u.knots, self.u.nb_knots())?).abs() > eps_u;
        let seg_v =
            (v1 - at(&self.v.knots, 1)?).abs() > eps_v || (v2 - at(&self.v.knots, self.v.nb_knots())?).abs() > eps_v;
        self.segment_impl(u1, u2, v1, v2, eps_u, eps_v, seg_u, seg_v)
    }

    fn set_knot_dir(&mut self, is_u: bool, index: i32, k: f64) -> Result<()> {
        self.take_pending()?;
        let d = self.dir(is_u);
        if index < 1 || index > d.nb_knots() {
            return Err(GeomError::OutOfRange);
        }
        let du = epsilon(k).abs();
        if index == 1 {
            if k >= at(&d.knots, 2)? - du {
                return Err(GeomError::Construction);
            }
        } else if index == d.nb_knots() {
            if k <= at(&d.knots, d.nb_knots() - 1)? + du {
                return Err(GeomError::Construction);
            }
        } else if k <= at(&d.knots, index - 1)? + du || k >= at(&d.knots, index + 1)? - du {
            return Err(GeomError::Construction);
        }
        if k != at(&d.knots, index)? {
            self.eval_rep = None;
            let d = if is_u { &mut self.u } else { &mut self.v };
            put(&mut d.knots, index, k)?;
            self.max_deriv_inv_ok = false;
            self.update_dir(is_u)?;
        }
        Ok(())
    }

    fn set_knots_dir(&mut self, is_u: bool, uk: &[f64]) -> Result<()> {
        self.take_pending()?;
        let n = self.dir(is_u).nb_knots();
        let lower = 1;
        let upper = uk.len() as i32;
        if upper < 1 || upper > n {
            return Err(GeomError::OutOfRange);
        }
        let _ = lower;
        if upper < n {
            let nxt = at(&self.dir(is_u).knots, upper + 1)?;
            if (at(uk, upper)? - nxt).abs() <= gp::resolution() {
                return Err(GeomError::Construction);
            }
        }
        let mut k1 = at(uk, 1)?;
        self.eval_rep = None;
        for i in 1..=upper {
            let d = if is_u { &mut self.u } else { &mut self.v };
            put(&mut d.knots, i, at(uk, i)?)?;
            if i != 1 {
                if (at(uk, i)? - k1).abs() <= gp::resolution() {
                    return Err(GeomError::Construction);
                }
                k1 = at(uk, i)?;
            }
        }
        self.max_deriv_inv_ok = false;
        self.update_dir(is_u)
    }

    /// `SetUKnot(UIndex, K)`.
    pub fn set_u_knot(&mut self, uindex: i32, k: f64) -> Result<()> {
        self.set_knot_dir(true, uindex, k)
    }
    /// `SetUKnots`.
    pub fn set_u_knots(&mut self, uk: &[f64]) -> Result<()> {
        self.set_knots_dir(true, uk)
    }
    /// `SetUKnot(UIndex, K, M)`.
    pub fn set_u_knot_mult(&mut self, uindex: i32, k: f64, m: i32) -> Result<()> {
        self.increase_u_multiplicity(uindex, m)?;
        self.set_u_knot(uindex, k)
    }
    /// `SetVKnot(VIndex, K)`.
    pub fn set_v_knot(&mut self, vindex: i32, k: f64) -> Result<()> {
        self.set_knot_dir(false, vindex, k)
    }
    /// `SetVKnots`.
    pub fn set_v_knots(&mut self, vk: &[f64]) -> Result<()> {
        self.set_knots_dir(false, vk)
    }
    /// `SetVKnot(VIndex, K, M)`.
    pub fn set_v_knot_mult(&mut self, vindex: i32, k: f64, m: i32) -> Result<()> {
        self.increase_v_multiplicity(vindex, m)?;
        self.set_v_knot(vindex, k)
    }

    /// `PeriodicNormalization`: the normalized parameters (unchanged when OCCT raises).
    pub fn periodic_normalization(&self, u: f64, v: f64) -> (f64, f64) {
        self.try_periodic_normalization(u, v).unwrap_or((u, v))
    }

    fn try_periodic_normalization(&self, u: f64, v: f64) -> Result<(f64, f64)> {
        let nu = self.u.normalize(u)?;
        let nv = self.v.normalize(v)?;
        Ok((nu, nv))
    }

    fn set_periodic_dir(&mut self, is_u: bool) -> Result<()> {
        self.eval_rep = None;
        let (first, last) = {
            let d = self.dir(is_u);
            (d.first_knot_index(), d.last_knot_index())
        };
        let (cknots, cmults, deg) = {
            let d = self.dir(is_u);
            let mut ck = Vec::new();
            let mut cm = Vec::new();
            for k in first..=last {
                ck.push(at(&d.knots, k)?);
                cm.push(at(&d.mults, k)?);
            }
            let n = cm.len() as i32;
            let m = d.deg.min(at(&cm, 1)?.max(at(&cm, n)?));
            put(&mut cm, 1, m)?;
            put(&mut cm, n, m)?;
            (ck, cm, d.deg)
        };
        let nbp = crate::bsplclib::curve3d::nb_poles_of(deg, true, &cmults)?;
        if nbp < 1 {
            return Err(GeomError::OutOfRange);
        }
        {
            let d = if is_u { &mut self.u } else { &mut self.v };
            d.knots = cknots;
            d.mults = cmults;
        }
        let nbp = nbp as usize;
        let rat = self.rat();
        if is_u {
            self.poles.resize(nbp, vec![Pnt::new(0.0, 0.0, 0.0); self.nb_v() as usize]);
            if rat {
                let cols = self.weights.first().map_or(0, Vec::len);
                self.weights.resize(nbp, vec![0.0; cols]);
            }
        } else {
            for row in &mut self.poles {
                row.resize(nbp, Pnt::new(0.0, 0.0, 0.0));
            }
            if rat {
                for row in &mut self.weights {
                    row.resize(nbp, 0.0);
                }
            }
        }
        if !rat {
            self.reset_unit_weights();
        }
        let d = if is_u { &mut self.u } else { &mut self.v };
        d.periodic = true;
        self.max_deriv_inv_ok = false;
        self.update_dir(is_u)
    }

    /// `SetUPeriodic`.
    pub fn set_u_periodic(&mut self) {
        let _ = self.set_periodic_dir(true);
    }

    /// `SetVPeriodic`.
    pub fn set_v_periodic(&mut self) {
        let _ = self.set_periodic_dir(false);
    }

    fn set_origin_dir(&mut self, is_u: bool, index: i32) -> Result<()> {
        if !self.dir(is_u).periodic {
            return Err(GeomError::NoSuchObject);
        }
        self.eval_rep = None;
        let first = self.dir(is_u).first_knot_index();
        let last = self.dir(is_u).last_knot_index();
        if index < first || index > last {
            return Err(GeomError::Domain);
        }
        let nbknots = self.dir(is_u).nb_knots();
        let mut newknots = vec![0.0; nbknots as usize];
        let mut newmults = vec![0; nbknots as usize];
        let period;
        {
            let d = self.dir(is_u);
            period = at(&d.knots, last)? - at(&d.knots, first)?;
            let mut k = 1;
            for i in index..=last {
                put(&mut newknots, k, at(&d.knots, i)?)?;
                put(&mut newmults, k, at(&d.mults, i)?)?;
                k += 1;
            }
            for i in first + 1..=index {
                put(&mut newknots, k, at(&d.knots, i)? + period)?;
                put(&mut newmults, k, at(&d.mults, i)?)?;
                k += 1;
            }
        }
        let mut pidx = 1;
        for i in first + 1..=index {
            pidx += at(&self.dir(is_u).mults, i)?;
        }
        // the new order of the pole lines along the direction: `pidx ..= last`, then `1 .. pidx`
        let nb = if is_u { self.nb_u() } else { self.nb_v() };
        let order: Vec<usize> = (pidx..=nb).chain(1..pidx).map(|i| (i - 1).max(0) as usize).collect();
        let rat = self.rat();
        if is_u {
            let np: Vec<Vec<Pnt>> = order.iter().filter_map(|&i| self.poles.get(i).cloned()).collect();
            let nw: Vec<Vec<f64>> =
                if rat { order.iter().filter_map(|&i| self.weights.get(i).cloned()).collect() } else { Vec::new() };
            self.poles = np;
            if rat {
                self.weights = nw;
            }
        } else {
            for row in &mut self.poles {
                let nr: Vec<Pnt> = order.iter().filter_map(|&j| row.get(j).copied()).collect();
                *row = nr;
            }
            if rat {
                for row in &mut self.weights {
                    let nr: Vec<f64> = order.iter().filter_map(|&j| row.get(j).copied()).collect();
                    *row = nr;
                }
            }
        }
        if !rat {
            self.reset_unit_weights();
        }
        let d = if is_u { &mut self.u } else { &mut self.v };
        d.knots = newknots;
        d.mults = newmults;
        self.update_dir(is_u)
    }

    /// `SetUOrigin`.
    pub fn set_u_origin(&mut self, index: i32) -> Result<()> {
        self.set_origin_dir(true, index)
    }

    /// `SetVOrigin`.
    pub fn set_v_origin(&mut self, index: i32) -> Result<()> {
        self.set_origin_dir(false, index)
    }

    fn set_not_periodic_dir(&mut self, is_u: bool) -> Result<()> {
        if self.dir(is_u).periodic {
            self.eval_rep = None;
            let r = {
                let pa = to_array2(&self.poles)?;
                let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
                let d = self.dir(is_u);
                bsplslib::unperiodize(is_u, d.deg, &d.mults, &d.knots, &pa, wa.as_ref())?
            };
            self.poles = from_array2(&r.poles);
            if self.rat() {
                self.weights = r.weights.as_ref().map(from_array2).unwrap_or_default();
            } else {
                self.reset_unit_weights();
            }
            let d = if is_u { &mut self.u } else { &mut self.v };
            d.mults = r.mults;
            d.knots = r.knots;
            d.periodic = false;
            self.max_deriv_inv_ok = false;
            self.update_dir(is_u)?;
        }
        Ok(())
    }

    /// `SetUNotPeriodic`.
    pub fn set_u_not_periodic(&mut self) {
        let _ = self.set_not_periodic_dir(true);
    }

    /// `SetVNotPeriodic`.
    pub fn set_v_not_periodic(&mut self) {
        let _ = self.set_not_periodic_dir(false);
    }

    /// `LocateU`.
    pub fn locate_u(&self, u: f64, parametric_tolerance: f64, with_knot_repetition: bool) -> (i32, i32) {
        let vbid = self.v.knots.first().copied().unwrap_or(0.0);
        let new_u = self.try_periodic_normalization(u, vbid).map_or(u, |r| r.0);
        self.u.locate(new_u, parametric_tolerance, with_knot_repetition, false)
    }

    /// `LocateV`.
    pub fn locate_v(&self, v: f64, parametric_tolerance: f64, with_knot_repetition: bool) -> (i32, i32) {
        let ubid = self.u.knots.first().copied().unwrap_or(0.0);
        let new_v = self.try_periodic_normalization(ubid, v).map_or(v, |r| r.1);
        self.v.locate(new_v, parametric_tolerance, with_knot_repetition, true)
    }

    fn check_uv(&self, uindex: i32, vindex: i32) -> Result<(usize, usize)> {
        if uindex < 1 || uindex > self.nb_u() || vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        Ok(((uindex - 1) as usize, (vindex - 1) as usize))
    }

    /// `SetPole(UIndex, VIndex, P)`.
    pub fn set_pole(&mut self, uindex: i32, vindex: i32, p: Pnt) -> Result<()> {
        let (i, j) = self.check_uv(uindex, vindex)?;
        self.eval_rep = None;
        if let Some(slot) = self.poles.get_mut(i).and_then(|r| r.get_mut(j)) {
            *slot = p;
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetPole(UIndex, VIndex, P, Weight)`.
    pub fn set_pole_weight(&mut self, uindex: i32, vindex: i32, p: Pnt, weight: f64) -> Result<()> {
        self.set_weight(uindex, vindex, weight)?;
        self.set_pole(uindex, vindex, p)
    }

    /// `SetPoleCol(VIndex, CPoles)`.
    pub fn set_pole_col(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()> {
        if vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        if poles.is_empty() || poles.len() > self.poles.len() {
            return Err(GeomError::Construction);
        }
        self.eval_rep = None;
        for (i, p) in poles.iter().enumerate() {
            if let Some(slot) = self.poles.get_mut(i).and_then(|r| r.get_mut((vindex - 1) as usize)) {
                *slot = *p;
            }
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetPoleCol(VIndex, CPoles, CPoleWeights)`.
    pub fn set_pole_col_weights(&mut self, vindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()> {
        self.set_pole_col(vindex, poles)?;
        self.set_weight_col(vindex, weights)
    }

    /// `SetPoleRow(UIndex, CPoles)`.
    pub fn set_pole_row(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()> {
        if uindex < 1 || uindex > self.nb_u() {
            return Err(GeomError::OutOfRange);
        }
        if poles.is_empty() || poles.len() > self.nb_v() as usize {
            return Err(GeomError::Construction);
        }
        self.eval_rep = None;
        for (j, p) in poles.iter().enumerate() {
            if let Some(slot) = self.poles.get_mut((uindex - 1) as usize).and_then(|r| r.get_mut(j)) {
                *slot = *p;
            }
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetPoleRow(UIndex, CPoles, CPoleWeights)`.
    pub fn set_pole_row_weights(&mut self, uindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()> {
        self.set_pole_row(uindex, poles)?;
        self.set_weight_row(uindex, weights)
    }

    /// `SetWeight`.
    pub fn set_weight(&mut self, uindex: i32, vindex: i32, weight: f64) -> Result<()> {
        if weight <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        let (i, j) = self.check_uv(uindex, vindex)?;
        self.eval_rep = None;
        if let Some(slot) = self.weights.get_mut(i).and_then(|r| r.get_mut(j)) {
            *slot = weight;
        }
        self.refresh_rational();
        if !self.rat() {
            self.reset_unit_weights();
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetWeightCol`.
    pub fn set_weight_col(&mut self, vindex: i32, weights: &[f64]) -> Result<()> {
        if vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        if weights.is_empty() || weights.len() > self.poles.len() {
            return Err(GeomError::Construction);
        }
        self.eval_rep = None;
        for (i, w) in weights.iter().enumerate() {
            if *w <= gp::resolution() {
                return Err(GeomError::Construction);
            }
            if let Some(slot) = self.weights.get_mut(i).and_then(|r| r.get_mut((vindex - 1) as usize)) {
                *slot = *w;
            }
        }
        self.refresh_rational();
        if !self.rat() {
            self.reset_unit_weights();
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetWeightRow`.
    pub fn set_weight_row(&mut self, uindex: i32, weights: &[f64]) -> Result<()> {
        if uindex < 1 || uindex > self.nb_u() {
            return Err(GeomError::OutOfRange);
        }
        if weights.is_empty() || weights.len() > self.nb_v() as usize {
            return Err(GeomError::Construction);
        }
        self.eval_rep = None;
        for (j, w) in weights.iter().enumerate() {
            if *w <= gp::resolution() {
                return Err(GeomError::Construction);
            }
            if let Some(slot) = self.weights.get_mut((uindex - 1) as usize).and_then(|r| r.get_mut(j)) {
                *slot = *w;
            }
        }
        self.refresh_rational();
        if !self.rat() {
            self.reset_unit_weights();
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    /// `MovePoint`: the first and last modified pole in U and V.
    pub fn move_point(
        &mut self,
        u: f64,
        v: f64,
        p: Pnt,
        uindex1: i32,
        uindex2: i32,
        vindex1: i32,
        vindex2: i32,
    ) -> Result<(i32, i32, i32, i32)> {
        let (nu, nv) = (self.nb_u(), self.nb_v());
        if uindex1 < 1
            || uindex1 > nu
            || uindex2 < 1
            || uindex2 > nu
            || uindex1 > uindex2
            || vindex1 < 1
            || vindex1 > nv
            || vindex2 < 1
            || vindex2 > nv
            || vindex1 > vindex2
        {
            return Err(GeomError::OutOfRange);
        }
        let p0 = self.eval_d0(u, v)?;
        let displ = gp::Vec::between(&p0, &p);
        let rat = self.rat();
        let moved = {
            let pa = to_array2(&self.poles)?;
            let wa = to_array2(&self.weights)?;
            bsplslib::move_point(
                u,
                v,
                &displ,
                uindex1,
                uindex2,
                vindex1,
                vindex2,
                self.u.deg,
                self.v.deg,
                rat,
                &pa,
                &wa,
                &self.u.flat,
                &self.v.flat,
            )?
        };
        if moved.u_first != 0 {
            self.eval_rep = None;
            if let Some(np) = moved.poles.as_ref() {
                self.poles = from_array2(np);
            }
        }
        self.max_deriv_inv_ok = false;
        Ok((moved.u_first, moved.u_last, moved.v_first, moved.v_last))
    }

    /// `IsURational`.
    pub fn is_u_rational(&self) -> bool {
        self.u.rational
    }

    /// `IsVRational`.
    pub fn is_v_rational(&self) -> bool {
        self.v.rational
    }

    /// `FirstUKnotIndex`.
    pub fn first_u_knot_index(&self) -> i32 {
        self.u.first_knot_index()
    }

    /// `FirstVKnotIndex`.
    pub fn first_v_knot_index(&self) -> i32 {
        self.v.first_knot_index()
    }

    /// `LastUKnotIndex`.
    pub fn last_u_knot_index(&self) -> i32 {
        self.u.last_knot_index()
    }

    /// `LastVKnotIndex`.
    pub fn last_v_knot_index(&self) -> i32 {
        self.v.last_knot_index()
    }

    /// `NbUKnots`.
    pub fn nb_u_knots(&self) -> i32 {
        self.u.nb_knots()
    }

    /// `NbUPoles`.
    pub fn nb_u_poles(&self) -> i32 {
        self.nb_u()
    }

    /// `NbVKnots`.
    pub fn nb_v_knots(&self) -> i32 {
        self.v.nb_knots()
    }

    /// `NbVPoles`.
    pub fn nb_v_poles(&self) -> i32 {
        self.nb_v()
    }

    /// `Pole`.
    pub fn pole(&self, uindex: i32, vindex: i32) -> Result<Pnt> {
        self.take_pending()?;
        let (i, j) = self.check_uv(uindex, vindex)?;
        self.poles.get(i).and_then(|r| r.get(j)).copied().ok_or(GeomError::OutOfRange)
    }

    /// `Poles`.
    pub fn poles(&self) -> &[Vec<Pnt>] {
        &self.poles
    }

    /// `UDegree`.
    pub fn u_degree(&self) -> i32 {
        self.u.deg
    }

    /// `VDegree`.
    pub fn v_degree(&self) -> i32 {
        self.v.deg
    }

    /// `UKnot`.
    pub fn u_knot(&self, uindex: i32) -> Result<f64> {
        if uindex < 1 || uindex > self.u.nb_knots() {
            return Err(GeomError::OutOfRange);
        }
        at(&self.u.knots, uindex)
    }

    /// `VKnot`.
    pub fn v_knot(&self, vindex: i32) -> Result<f64> {
        if vindex < 1 || vindex > self.v.nb_knots() {
            return Err(GeomError::OutOfRange);
        }
        at(&self.v.knots, vindex)
    }

    /// `UKnotDistribution`.
    pub fn u_knot_distribution(&self) -> BSplKnotDistribution {
        self.u.knot_set
    }

    /// `VKnotDistribution`.
    pub fn v_knot_distribution(&self) -> BSplKnotDistribution {
        self.v.knot_set
    }

    /// `UKnots`.
    pub fn u_knots(&self) -> &[f64] {
        &self.u.knots
    }

    /// `VKnots`.
    pub fn v_knots(&self) -> &[f64] {
        &self.v.knots
    }

    /// `UKnotSequence`.
    pub fn u_knot_sequence(&self) -> &[f64] {
        &self.u.flat
    }

    /// `VKnotSequence`.
    pub fn v_knot_sequence(&self) -> &[f64] {
        &self.v.flat
    }

    /// `UMultiplicity`.
    pub fn u_multiplicity(&self, uindex: i32) -> Result<i32> {
        if uindex < 1 || uindex > self.u.nb_knots() {
            return Err(GeomError::OutOfRange);
        }
        at(&self.u.mults, uindex)
    }

    /// `VMultiplicity`.
    pub fn v_multiplicity(&self, vindex: i32) -> Result<i32> {
        if vindex < 1 || vindex > self.v.nb_knots() {
            return Err(GeomError::OutOfRange);
        }
        at(&self.v.mults, vindex)
    }

    /// `UMultiplicities`.
    pub fn u_multiplicities(&self) -> &[i32] {
        &self.u.mults
    }

    /// `VMultiplicities`.
    pub fn v_multiplicities(&self) -> &[i32] {
        &self.v.mults
    }

    /// `Weight`.
    pub fn weight(&self, uindex: i32, vindex: i32) -> Result<f64> {
        self.take_pending()?;
        if !self.rat() {
            return Ok(1.0);
        }
        let (i, j) = self.check_uv(uindex, vindex)?;
        self.weights.get(i).and_then(|r| r.get(j)).copied().ok_or(GeomError::OutOfRange)
    }

    /// `Weights` (`None` is `BSplSLib::NoWeights()`).
    pub fn weights(&self) -> Option<&[Vec<f64>]> {
        if self.rat() { Some(&self.weights) } else { None }
    }

    // Runs `f` on the `BSplSLib` view of the surface with the flat knots.
    fn with_flat_surface<R>(&self, f: impl FnOnce(&BSurface<'_>) -> Result<R>) -> Result<R> {
        let pa = to_array2(&self.poles)?;
        let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
        let s = BSurface {
            u_degree: self.u.deg,
            v_degree: self.v.deg,
            u_rational: self.u.rational,
            v_rational: self.v.rational,
            u_periodic: self.u.periodic,
            v_periodic: self.v.periodic,
            poles: &pa,
            weights: wa.as_ref(),
            u_knots: &self.u.flat,
            v_knots: &self.v.flat,
            u_mults: None,
            v_mults: None,
        };
        f(&s)
    }

    /// `LocalD0`.
    pub fn local_d0(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32) -> Result<Pnt> {
        let (ui, vi, nu, nv) = self.local_indices(u, v, from_uk1, to_uk2, from_vk1, to_vk2)?;
        self.with_flat_surface(|s| Ok(bsplslib::d0(nu, nv, ui, vi, s)?))
    }

    // The span indices and located parameters of the Local* functions.
    fn local_indices(
        &self,
        u: f64,
        v: f64,
        from_uk1: i32,
        to_uk2: i32,
        from_vk1: i32,
        to_vk2: i32,
    ) -> Result<(i32, i32, f64, f64)> {
        if from_uk1 == to_uk2 || from_vk1 == to_vk2 {
            return Err(GeomError::Domain);
        }
        let (ui, nu) = self.u.local_index(u, from_uk1, to_uk2)?;
        let (vi, nv) = self.v.local_index(v, from_vk1, to_vk2)?;
        Ok((ui, vi, nu, nv))
    }

    /// `LocalD1`.
    pub fn local_d1(
        &self,
        u: f64,
        v: f64,
        from_uk1: i32,
        to_uk2: i32,
        from_vk1: i32,
        to_vk2: i32,
    ) -> Result<(Pnt, gp::Vec, gp::Vec)> {
        let (ui, vi, nu, nv) = self.local_indices(u, v, from_uk1, to_uk2, from_vk1, to_vk2)?;
        self.with_flat_surface(|s| Ok(bsplslib::d1(nu, nv, ui, vi, s)?))
    }

    /// `LocalD2`.
    pub fn local_d2(
        &self,
        u: f64,
        v: f64,
        from_uk1: i32,
        to_uk2: i32,
        from_vk1: i32,
        to_vk2: i32,
    ) -> Result<(Pnt, gp::Vec, gp::Vec, gp::Vec, gp::Vec, gp::Vec)> {
        let (ui, vi, nu, nv) = self.local_indices(u, v, from_uk1, to_uk2, from_vk1, to_vk2)?;
        self.with_flat_surface(|s| Ok(bsplslib::d2(nu, nv, ui, vi, s)?))
    }

    /// `LocalD3`.
    pub fn local_d3(
        &self,
        u: f64,
        v: f64,
        from_uk1: i32,
        to_uk2: i32,
        from_vk1: i32,
        to_vk2: i32,
    ) -> Result<SurfResD3> {
        let (ui, vi, nu, nv) = self.local_indices(u, v, from_uk1, to_uk2, from_vk1, to_vk2)?;
        self.with_flat_surface(|s| {
            let r = bsplslib::d3(nu, nv, ui, vi, s)?;
            Ok(res_d3(r))
        })
    }

    #[allow(clippy::too_many_arguments)]
    /// `LocalDN`.
    pub fn local_dn(
        &self,
        u: f64,
        v: f64,
        from_uk1: i32,
        to_uk2: i32,
        from_vk1: i32,
        to_vk2: i32,
        nu_order: i32,
        nv_order: i32,
    ) -> Result<gp::Vec> {
        let (ui, vi, nu, nv) = self.local_indices(u, v, from_uk1, to_uk2, from_vk1, to_vk2)?;
        self.with_flat_surface(|s| Ok(bsplslib::dn(nu, nv, nu_order, nv_order, ui, vi, s)?))
    }

    /// `LocalValue`.
    pub fn local_value(&self, u: f64, v: f64, from_uk1: i32, to_uk2: i32, from_vk1: i32, to_vk2: i32) -> Result<Pnt> {
        self.local_d0(u, v, from_uk1, to_uk2, from_vk1, to_vk2)
    }

    fn iso_curve(&self, is_u: bool, p: f64, check_rational: bool) -> Result<BSplineCurve> {
        let pa = to_array2(&self.poles)?;
        let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
        let d = self.dir(is_u);
        let other = self.dir(!is_u);
        let (cp, cw) = bsplslib::iso(p, is_u, &pa, wa.as_ref(), &d.flat, None, d.deg, d.periodic, true)?;
        if self.rat() {
            BSplineCurve::with_weights_checked(
                &cp,
                &cw.unwrap_or_default(),
                &other.knots,
                &other.mults,
                other.deg,
                other.periodic,
                check_rational,
            )
        } else {
            BSplineCurve::new(&cp, &other.knots, &other.mults, other.deg, other.periodic)
        }
    }

    /// `UIso(U, CheckRational)`.
    pub fn u_iso_check(&self, u: f64, check_rational: bool) -> Result<BSplineCurve> {
        self.iso_curve(true, u, check_rational)
    }

    /// `VIso(V, CheckRational)`.
    pub fn v_iso_check(&self, v: f64, check_rational: bool) -> Result<BSplineCurve> {
        self.iso_curve(false, v, check_rational)
    }

    /// `MaxDegree`.
    pub fn max_degree() -> i32 {
        bsplclib::max_degree()
    }

    /// `Resolution`: `(UTolerance, VTolerance)`.
    pub fn resolution(&mut self, tolerance_3d: f64) -> (f64, f64) {
        if !self.max_deriv_inv_ok {
            let r = (|| -> Result<(f64, f64)> {
                let pa = to_array2(&self.poles)?;
                let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
                let s = BSurface {
                    u_degree: self.u.deg,
                    v_degree: self.v.deg,
                    u_rational: self.u.rational,
                    v_rational: self.v.rational,
                    u_periodic: self.u.periodic,
                    v_periodic: self.v.periodic,
                    poles: &pa,
                    weights: wa.as_ref(),
                    u_knots: &self.u.knots,
                    v_knots: &self.v.knots,
                    u_mults: Some(&self.u.mults),
                    v_mults: Some(&self.v.mults),
                };
                Ok(bsplslib::resolution(&s, 1.0)?)
            })();
            if let Ok((u, v)) = r {
                self.u.max_deriv_inv = u;
                self.v.max_deriv_inv = v;
            }
            self.max_deriv_inv_ok = true;
        }
        (tolerance_3d * self.u.max_deriv_inv, tolerance_3d * self.v.max_deriv_inv)
    }

    // The copy constructor
    fn copy_bspline(&self) -> BSplineSurface {
        let mut c = self.clone();
        if !self.rat() {
            c.reset_unit_weights();
        }
        c.eval_rep = rep_utils::clone_surface_desc(&self.eval_rep);
        c.max_deriv_inv_ok = false;
        c
    }

    // The located span and parameter of `EvalD1` / `EvalD2` in one direction.
    fn eval_span(&self, d: &Dir, p: f64) -> Result<(i32, f64)> {
        let (index, new_p) = bsplclib::flat::locate_parameter_opt(d.deg, &d.knots, Some(&d.mults), p, d.periodic, 0)?;
        let index = curve2d::flat_index(d.deg, index, &d.mults, d.periodic)?;
        Ok((index, new_p))
    }
}

fn res_d3(r: bsplslib::D3Result) -> SurfResD3 {
    SurfResD3 {
        point: r.0,
        d1u: r.1,
        d1v: r.2,
        d2u: r.3,
        d2v: r.4,
        d2uv: r.5,
        d3u: r.6,
        d3v: r.7,
        d3uuv: r.8,
        d3uvv: r.9,
    }
}

impl Geometry for BSplineSurface {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        self.eval_rep = None;
        for row in &mut self.poles {
            for p in row {
                p.transform(t);
            }
        }
        self.max_deriv_inv_ok = false;
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.copy_bspline())
    }
}

impl Surface for BSplineSurface {
    fn copy_surface(&self) -> Box<dyn Surface> {
        Box::new(self.copy_bspline())
    }
    fn u_reverse(&mut self) {
        let _ = self.reverse_dir(true);
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        let first = self.u.knots.first().copied().unwrap_or(0.0);
        let last = self.u.knots.last().copied().unwrap_or(0.0);
        first + last - u
    }
    fn v_reverse(&mut self) {
        let _ = self.reverse_dir(false);
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        let first = self.v.knots.first().copied().unwrap_or(0.0);
        let last = self.v.knots.last().copied().unwrap_or(0.0);
        first + last - v
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.u.first_param().unwrap_or(0.0),
            self.u.last_param().unwrap_or(0.0),
            self.v.first_param().unwrap_or(0.0),
            self.v.last_param().unwrap_or(0.0),
        )
    }
    fn is_u_closed(&self) -> bool {
        match self.try_is_u_closed() {
            Ok(c) => c,
            Err(_) => {
                self.pending_error.0.store(true, Ordering::Relaxed);
                false
            }
        }
    }
    fn is_v_closed(&self) -> bool {
        match self.try_is_v_closed() {
            Ok(c) => c,
            Err(_) => {
                self.pending_error.0.store(true, Ordering::Relaxed);
                false
            }
        }
    }
    fn is_u_periodic(&self) -> bool {
        self.u.periodic
    }
    fn is_v_periodic(&self) -> bool {
        self.v.periodic
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        self.take_pending()?;
        Ok(Box::new(self.iso_curve(true, u, true)?))
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        self.take_pending()?;
        Ok(Box::new(self.iso_curve(false, v, true)?))
    }
    fn continuity(&self) -> GeomAbsShape {
        if (self.u.smooth as i32) < (self.v.smooth as i32) { self.u.smooth } else { self.v.smooth }
    }
    fn is_cn_u(&self, n: i32) -> Result<bool> {
        self.u.is_cn(n)
    }
    fn is_cn_v(&self, n: i32) -> Result<bool> {
        self.v.is_cn(n)
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        self.take_pending()?;
        if let Some(r) = rep_utils::try_eval_surface_d0(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let (nu, nv) = self.try_periodic_normalization(u, v)?;
        self.with_flat_surface(|s| Ok(bsplslib::d0(nu, nv, 0, 0, s)?))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        self.take_pending()?;
        if let Some(r) = rep_utils::try_eval_surface_d1(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let _ = self.try_periodic_normalization(u, v)?;
        let (ui, nu) = self.eval_span(&self.u, u)?;
        let (vi, nv) = self.eval_span(&self.v, v)?;
        self.with_flat_surface(|s| {
            let (point, d1u, d1v) = bsplslib::d1(nu, nv, ui, vi, s)?;
            Ok(SurfResD1 { point, d1u, d1v })
        })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        self.take_pending()?;
        if let Some(r) = rep_utils::try_eval_surface_d2(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        let _ = self.try_periodic_normalization(u, v)?;
        let (ui, nu) = self.eval_span(&self.u, u)?;
        let (vi, nv) = self.eval_span(&self.v, v)?;
        self.with_flat_surface(|s| {
            let (point, d1u, d1v, d2u, d2v, d2uv) = bsplslib::d2(nu, nv, ui, vi, s)?;
            Ok(SurfResD2 { point, d1u, d1v, d2u, d2v, d2uv })
        })
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        self.take_pending()?;
        if let Some(r) = rep_utils::try_eval_surface_d3(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        self.with_flat_surface(|s| Ok(res_d3(bsplslib::d3(u, v, 0, 0, s)?)))
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        self.take_pending()?;
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        if let Some(r) = rep_utils::try_eval_surface_dn(&self.eval_rep, u, v, nu, nv)? {
            return Ok(r);
        }
        self.with_flat_surface(|s| Ok(bsplslib::dn(u, v, nu, nv, 0, 0, s)?))
    }
}

impl BSplineSurface {
    // UReverse / VReverse
    fn reverse_dir(&mut self, is_u: bool) -> Result<()> {
        self.eval_rep = None;
        {
            let d = if is_u { &mut self.u } else { &mut self.v };
            curve2d::reverse_mults(&mut d.mults);
            curve2d::reverse_knots(&mut d.knots)?;
        }
        let d = self.dir(is_u);
        let last = if d.periodic {
            d.flat.len() as i32 - d.deg - 1
        } else if is_u {
            self.nb_u()
        } else {
            self.nb_v()
        };
        let pa = to_array2(&self.poles)?;
        self.poles = from_array2(&bsplslib::reverse_poles(&pa, last, is_u)?);
        if self.rat() {
            let wa = to_array2(&self.weights)?;
            self.weights = from_array2(&bsplslib::reverse_weights(&wa, last, is_u)?);
        }
        self.update_dir(is_u)
    }
}

impl BoundedSurface for BSplineSurface {}
