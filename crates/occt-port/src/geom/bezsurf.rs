//! `Geom_BezierSurface`.
use super::rep_utils::{self, SurfaceEvalRepDesc};
use super::{BezierCurve, Curve, GeomAbsShape, GeomError, Geometry, Result, SurfResD1, SurfResD2, SurfResD3, Surface};
use super::{BoundedSurface, confusion, from_array2, geometry_any, to_array2};
use crate::bsplclib::util::epsilon;
use crate::bsplslib::{self, Surface as BSurface};
use crate::gp::{self, Pnt, Trsf};
use crate::plib::{self, Array2};

/// `Geom_BezierSurface`.
#[derive(Debug, Clone)]
pub struct BezierSurface {
    poles: Vec<Vec<Pnt>>,
    weights: Vec<Vec<f64>>,
    eval_rep: Option<SurfaceEvalRepDesc>,
    u_rational: bool,
    v_rational: bool,
    u_max_deriv_inv: f64,
    v_max_deriv_inv: f64,
    max_deriv_inv_ok: bool,
}

// The static `Rational` of Geom_BezierSurface.cxx: (URational, VRational).
pub(super) fn rational(w: &[Vec<f64>]) -> (bool, bool) {
    let rows = w.len();
    let cols = w.first().map_or(0, Vec::len);
    let at = |i: usize, j: usize| w.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let mut vrat = false;
    let mut j = 0;
    while !vrat && j < cols {
        let mut i = 0;
        while !vrat && i + 1 < rows {
            vrat = (at(i, j) - at(i + 1, j)).abs() > epsilon(at(i, j).abs());
            i += 1;
        }
        j += 1;
    }
    let mut urat = false;
    let mut i = 0;
    while !urat && i < rows {
        let mut j = 0;
        while !urat && j + 1 < cols {
            urat = (at(i, j) - at(i, j + 1)).abs() > epsilon(at(i, j).abs());
            j += 1;
        }
        i += 1;
    }
    (urat, vrat)
}

pub(super) fn unit_weights(rows: usize, cols: usize) -> Vec<Vec<f64>> {
    vec![vec![1.0; cols]; rows]
}

// AddPoleCol / AddRatPoleCol: insert `col` after `after` columns.
fn add_col<T: Clone>(poles: &[Vec<T>], col: &[T], after: usize) -> Result<Vec<Vec<T>>> {
    let mut out = Vec::with_capacity(poles.len());
    for (i, row) in poles.iter().enumerate() {
        let mut r = Vec::with_capacity(row.len() + 1);
        r.extend_from_slice(row.get(..after).ok_or(GeomError::OutOfRange)?);
        r.push(col.get(i).cloned().ok_or(GeomError::OutOfRange)?);
        r.extend_from_slice(row.get(after..).ok_or(GeomError::OutOfRange)?);
        out.push(r);
    }
    Ok(out)
}

// AddPoleRow / AddRatPoleRow: insert `row` after `after` rows.
fn add_row<T: Clone>(poles: &[Vec<T>], row: &[T], after: usize) -> Result<Vec<Vec<T>>> {
    let mut out = Vec::with_capacity(poles.len() + 1);
    out.extend_from_slice(poles.get(..after).ok_or(GeomError::OutOfRange)?);
    out.push(row.to_vec());
    out.extend_from_slice(poles.get(after..).ok_or(GeomError::OutOfRange)?);
    Ok(out)
}

// DeletePoleCol / DeleteRatPoleCol (1-based `index`)
fn delete_col<T: Clone>(poles: &[Vec<T>], index: usize) -> Vec<Vec<T>> {
    poles
        .iter()
        .map(|row| row.iter().enumerate().filter(|(j, _)| *j + 1 != index).map(|(_, v)| v.clone()).collect())
        .collect()
}

// DeletePoleRow / DeleteRatPoleRow (1-based `index`)
fn delete_row<T: Clone>(poles: &[Vec<T>], index: usize) -> Vec<Vec<T>> {
    poles.iter().enumerate().filter(|(i, _)| *i + 1 != index).map(|(_, r)| r.clone()).collect()
}

fn flat_bezier_knots(degree: i32) -> Vec<f64> {
    let n = (degree + 1).max(0) as usize;
    let mut k = vec![0.0; n];
    k.extend(std::iter::repeat_n(1.0, n));
    k
}

impl BezierSurface {
    fn blank() -> Self {
        BezierSurface {
            poles: Vec::new(),
            weights: Vec::new(),
            eval_rep: None,
            u_rational: false,
            v_rational: false,
            u_max_deriv_inv: 0.0,
            v_max_deriv_inv: 0.0,
            max_deriv_inv_ok: false,
        }
    }

    /// `Geom_BezierSurface(SurfacePoles)`.
    pub fn new(poles: &[Vec<Pnt>]) -> Result<Self> {
        let nb_u = poles.len();
        let nb_v = poles.first().map_or(0, Vec::len);
        let max = Self::max_degree() as usize + 1;
        if nb_u < 2 || nb_u > max || nb_v < 2 || nb_v > max || poles.iter().any(|r| r.len() != nb_v) {
            return Err(GeomError::Construction);
        }
        let mut s = BezierSurface::blank();
        s.init(poles, None);
        Ok(s)
    }

    /// `Geom_BezierSurface(SurfacePoles, PoleWeights)`.
    pub fn with_weights(poles: &[Vec<Pnt>], weights: &[Vec<f64>]) -> Result<Self> {
        let nb_u = poles.len();
        let nb_v = poles.first().map_or(0, Vec::len);
        let max = Self::max_degree() as usize + 1;
        if nb_u < 2
            || nb_u > max
            || nb_v < 2
            || nb_v > max
            || poles.iter().any(|r| r.len() != nb_v)
            || weights.len() != nb_u
            || weights.iter().any(|r| r.len() != nb_v)
        {
            return Err(GeomError::Construction);
        }
        if weights.iter().flatten().any(|w| *w <= gp::resolution()) {
            return Err(GeomError::Construction);
        }
        let (urat, vrat) = rational(weights);
        let mut s = BezierSurface::blank();
        s.u_rational = urat;
        s.v_rational = vrat;
        if urat || vrat {
            s.init(poles, Some(weights));
        } else {
            s.init(poles, None);
        }
        Ok(s)
    }

    // init
    fn init(&mut self, poles: &[Vec<Pnt>], weights: Option<&[Vec<f64>]>) {
        let nb_u = poles.len();
        let nb_v = poles.first().map_or(0, Vec::len);
        self.poles = poles.to_vec();
        if let Some(w) = weights {
            self.weights = w.to_vec();
            let (u, v) = rational(&self.weights);
            self.u_rational = u;
            self.v_rational = v;
            if !(u || v) {
                self.weights = unit_weights(nb_u, nb_v);
            }
        } else {
            self.u_rational = false;
            self.v_rational = false;
            self.weights = unit_weights(nb_u, nb_v);
        }
        self.max_deriv_inv_ok = false;
        self.eval_rep = None;
    }

    fn rat(&self) -> bool {
        self.u_rational || self.v_rational
    }

    fn nb_u(&self) -> i32 {
        self.poles.len() as i32
    }

    fn nb_v(&self) -> i32 {
        self.poles.first().map_or(0, Vec::len) as i32
    }

    fn refresh_rational(&mut self) {
        let (u, v) = rational(&self.weights);
        self.u_rational = u;
        self.v_rational = v;
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
        std::mem::swap(&mut self.u_rational, &mut self.v_rational);
        self.max_deriv_inv_ok = false;
    }

    /// `Increase`.
    pub fn increase(&mut self, u_deg: i32, v_deg: i32) -> Result<()> {
        if u_deg < self.u_degree()
            || u_deg > Self::max_degree()
            || v_deg < self.v_degree()
            || v_deg > Self::max_degree()
        {
            return Err(GeomError::Construction);
        }
        let old_u = self.u_degree();
        let old_v = self.v_degree();
        let inc_u = u_deg - old_u;
        let inc_v = v_deg - old_v;
        if inc_u == 0 && inc_v == 0 {
            return Ok(());
        }
        if inc_u > 0 {
            let r = {
                let pa = to_array2(&self.poles)?;
                let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
                bsplslib::increase_degree(
                    true,
                    old_u,
                    u_deg,
                    false,
                    &pa,
                    wa.as_ref(),
                    &[0.0, 1.0],
                    &self.u_multiplicities(),
                )?
            };
            if self.rat()
                && let Some(w) = r.weights.as_ref()
            {
                self.weights = from_array2(w);
            }
            self.poles = from_array2(&r.poles);
        }
        if inc_v > 0 {
            let r = {
                let pa = to_array2(&self.poles)?;
                let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
                bsplslib::increase_degree(
                    false,
                    old_v,
                    v_deg,
                    false,
                    &pa,
                    wa.as_ref(),
                    &[0.0, 1.0],
                    &self.v_multiplicities(),
                )?
            };
            if self.rat()
                && let Some(w) = r.weights.as_ref()
            {
                self.weights = from_array2(w);
            }
            self.poles = from_array2(&r.poles);
        }
        if self.rat() {
            self.refresh_rational();
            if !self.rat() {
                self.reset_unit_weights();
            }
        } else {
            self.reset_unit_weights();
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    fn insert_col_after(&mut self, vindex: i32, cpoles: &[Pnt], cweights: Option<&[f64]>) -> Result<()> {
        if vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        if cpoles.len() != self.poles.len() {
            return Err(GeomError::Construction);
        }
        let after = vindex as usize;
        match cweights {
            None => {
                let npoles = add_col(&self.poles, cpoles, after)?;
                if self.rat() {
                    let cw = vec![1.0; self.poles.len()];
                    self.weights = add_col(&self.weights, &cw, after)?;
                } else {
                    self.poles = npoles.clone();
                    self.reset_unit_weights();
                }
                self.poles = npoles;
                self.max_deriv_inv_ok = false;
            }
            Some(cw) => {
                if cw.len() != cpoles.len() {
                    return Err(GeomError::Construction);
                }
                if cw.iter().any(|w| *w <= gp::resolution()) {
                    return Err(GeomError::Construction);
                }
                let npoles = add_col(&self.poles, cpoles, after)?;
                let nweights = add_col(&self.weights, cw, after)?;
                self.poles = npoles;
                self.weights = nweights;
                self.refresh_rational();
                if !self.rat() {
                    self.reset_unit_weights();
                }
                self.max_deriv_inv_ok = false;
            }
        }
        Ok(())
    }

    fn insert_row_after(&mut self, uindex: i32, cpoles: &[Pnt], cweights: Option<&[f64]>) -> Result<()> {
        if uindex < 1 || uindex > self.nb_u() {
            return Err(GeomError::OutOfRange);
        }
        if cpoles.len() != self.nb_v() as usize {
            return Err(GeomError::Construction);
        }
        let after = uindex as usize;
        match cweights {
            None => {
                let npoles = add_row(&self.poles, cpoles, after)?;
                if self.rat() {
                    let cw = vec![1.0; self.nb_v() as usize];
                    self.weights = add_row(&self.weights, &cw, after)?;
                    self.poles = npoles;
                } else {
                    self.poles = npoles;
                    self.reset_unit_weights();
                }
                self.max_deriv_inv_ok = false;
            }
            Some(cw) => {
                if cw.len() != cpoles.len() {
                    return Err(GeomError::Construction);
                }
                if cw.iter().any(|w| *w <= gp::resolution()) {
                    return Err(GeomError::Construction);
                }
                let npoles = add_row(&self.poles, cpoles, after)?;
                let nweights = add_row(&self.weights, cw, after)?;
                self.poles = npoles;
                self.weights = nweights;
                self.refresh_rational();
                if !self.rat() {
                    self.reset_unit_weights();
                }
                self.max_deriv_inv_ok = false;
            }
        }
        Ok(())
    }

    /// `InsertPoleColAfter(VIndex, CPoles)`.
    pub fn insert_pole_col_after(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()> {
        self.insert_col_after(vindex, poles, None)
    }
    /// `InsertPoleColAfter(VIndex, CPoles, CPoleWeights)`.
    pub fn insert_pole_col_after_weights(&mut self, vindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()> {
        self.insert_col_after(vindex, poles, Some(weights))
    }
    /// `InsertPoleColBefore(VIndex, CPoles)`.
    pub fn insert_pole_col_before(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()> {
        self.insert_col_after(vindex - 1, poles, None)
    }
    /// `InsertPoleColBefore(VIndex, CPoles, CPoleWeights)`.
    pub fn insert_pole_col_before_weights(&mut self, vindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()> {
        self.insert_col_after(vindex - 1, poles, Some(weights))
    }
    /// `InsertPoleRowAfter(UIndex, CPoles)`.
    pub fn insert_pole_row_after(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()> {
        self.insert_row_after(uindex, poles, None)
    }
    /// `InsertPoleRowAfter(UIndex, CPoles, CPoleWeights)`.
    pub fn insert_pole_row_after_weights(&mut self, uindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()> {
        self.insert_row_after(uindex, poles, Some(weights))
    }
    /// `InsertPoleRowBefore(UIndex, CPoles)`.
    pub fn insert_pole_row_before(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()> {
        self.insert_row_after(uindex - 1, poles, None)
    }
    /// `InsertPoleRowBefore(UIndex, CPoles, CPoleWeights)`.
    pub fn insert_pole_row_before_weights(&mut self, uindex: i32, poles: &[Pnt], weights: &[f64]) -> Result<()> {
        self.insert_row_after(uindex - 1, poles, Some(weights))
    }

    /// `RemovePoleCol`.
    pub fn remove_pole_col(&mut self, vindex: i32) -> Result<()> {
        if vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        if self.nb_v() <= 2 {
            return Err(GeomError::Construction);
        }
        let npoles = delete_col(&self.poles, vindex as usize);
        if self.rat() {
            let nweights = delete_col(&self.weights, vindex as usize);
            let (u, v) = rational(&nweights);
            self.u_rational = u;
            self.v_rational = v;
            self.poles = npoles;
            if u || v {
                self.weights = nweights;
            } else {
                self.reset_unit_weights();
            }
        } else {
            self.poles = npoles;
            self.reset_unit_weights();
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `RemovePoleRow`.
    pub fn remove_pole_row(&mut self, uindex: i32) -> Result<()> {
        if uindex < 1 || uindex > self.nb_u() {
            return Err(GeomError::OutOfRange);
        }
        if self.nb_u() <= 2 {
            return Err(GeomError::Construction);
        }
        let npoles = delete_row(&self.poles, uindex as usize);
        if self.rat() {
            let nweights = delete_row(&self.weights, uindex as usize);
            let (u, v) = rational(&nweights);
            self.u_rational = u;
            self.v_rational = v;
            self.poles = npoles;
            if u || v {
                self.weights = nweights;
            } else {
                self.reset_unit_weights();
            }
        } else {
            self.poles = npoles;
            self.reset_unit_weights();
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `Segment`.
    pub fn segment(&mut self, u1: f64, u2: f64, v1: f64, v2: f64) {
        let _ = self.try_segment(u1, u2, v1, v2);
    }

    fn try_segment(&mut self, u1: f64, u2: f64, v1: f64, v2: f64) -> Result<()> {
        let rat = self.rat();
        let udeg = self.u_degree();
        let vdeg = self.v_degree();
        let pa = to_array2(&self.poles)?;
        let wa = if rat { Some(to_array2(&self.weights)?) } else { None };
        let (mut coefs, mut wcoefs) = bsplslib::build_cache(
            0.5,
            0.5,
            0.5,
            0.5,
            false,
            false,
            udeg,
            vdeg,
            0,
            0,
            &flat_bezier_knots(udeg),
            &flat_bezier_knots(vdeg),
            &pa,
            wa.as_ref(),
        )?;
        if udeg <= vdeg {
            let (nu, nv) = ((udeg + 1) as usize, (vdeg + 1) as usize);
            let mut t = Vec::with_capacity(nu * nv);
            for ii in 0..nu {
                for jj in 0..nv {
                    t.push(*coefs.get(jj, ii).ok_or(GeomError::OutOfRange)?);
                }
            }
            coefs = Array2::new(nu, nv, t)?;
            if rat {
                let wc = wcoefs.as_ref().ok_or(GeomError::OutOfRange)?;
                let mut wt = Vec::with_capacity(nu * nv);
                for ii in 0..nu {
                    for jj in 0..nv {
                        wt.push(*wc.get(jj, ii).ok_or(GeomError::OutOfRange)?);
                    }
                }
                wcoefs = Some(Array2::new(nu, nv, wt)?);
            }
        }
        let ufirst = 2.0 * (u1 - 0.5);
        let ulast = 2.0 * (u2 - 0.5);
        let vfirst = 2.0 * (v1 - 0.5);
        let vlast = 2.0 * (v2 - 0.5);
        let (c, w) = plib::u_trimming(ufirst, ulast, &coefs, wcoefs.as_ref())?;
        let (c, w) = plib::v_trimming(vfirst, vlast, &c, w.as_ref())?;
        let (np, nw) = plib::coefficients_poles_surface(&c, w.as_ref())?;
        self.poles = from_array2(&np);
        if rat && let Some(nw) = nw {
            self.weights = from_array2(&nw);
        }
        self.max_deriv_inv_ok = false;
        Ok(())
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
        if let Some(slot) = self.poles.get_mut(i).and_then(|r| r.get_mut(j)) {
            *slot = p;
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetPole(UIndex, VIndex, P, Weight)`.
    pub fn set_pole_weight(&mut self, uindex: i32, vindex: i32, p: Pnt, weight: f64) -> Result<()> {
        if weight <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        let (i, j) = self.check_uv(uindex, vindex)?;
        if let Some(slot) = self.poles.get_mut(i).and_then(|r| r.get_mut(j)) {
            *slot = p;
        }
        self.set_weight(uindex, vindex, weight)
    }

    // The checks of SetPoleCol / SetPoleRow on the array `CPoles` (lower bound 1, upper bound `len`).
    fn array_in_range(len: usize, max: usize) -> bool {
        len >= 1 && len <= max
    }

    /// `SetPoleCol(VIndex, CPoles)`.
    pub fn set_pole_col(&mut self, vindex: i32, poles: &[Pnt]) -> Result<()> {
        if vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        if !Self::array_in_range(poles.len(), self.poles.len()) {
            return Err(GeomError::Construction);
        }
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
        if vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        if !Self::array_in_range(poles.len(), self.poles.len()) || weights.len() != poles.len() {
            return Err(GeomError::Construction);
        }
        for (i, p) in poles.iter().enumerate() {
            if let Some(slot) = self.poles.get_mut(i).and_then(|r| r.get_mut((vindex - 1) as usize)) {
                *slot = *p;
            }
        }
        self.set_weight_col(vindex, weights)
    }

    /// `SetPoleRow(UIndex, CPoles)`.
    pub fn set_pole_row(&mut self, uindex: i32, poles: &[Pnt]) -> Result<()> {
        if uindex < 1 || uindex > self.nb_u() {
            return Err(GeomError::OutOfRange);
        }
        if !Self::array_in_range(poles.len(), self.nb_v() as usize) {
            return Err(GeomError::Construction);
        }
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
        if uindex < 1 || uindex > self.nb_u() {
            return Err(GeomError::OutOfRange);
        }
        if !Self::array_in_range(poles.len(), self.nb_v() as usize) || weights.len() != poles.len() {
            return Err(GeomError::Construction);
        }
        for (j, p) in poles.iter().enumerate() {
            if let Some(slot) = self.poles.get_mut((uindex - 1) as usize).and_then(|r| r.get_mut(j)) {
                *slot = *p;
            }
        }
        self.set_weight_row(uindex, weights)
    }

    /// `SetWeight`.
    pub fn set_weight(&mut self, uindex: i32, vindex: i32, weight: f64) -> Result<()> {
        if weight <= gp::resolution() {
            return Err(GeomError::Construction);
        }
        let (i, j) = self.check_uv(uindex, vindex)?;
        if !self.rat() && (weight - 1.0).abs() <= gp::resolution() {
            return Ok(());
        }
        let cur = self.weights.get(i).and_then(|r| r.get(j)).copied().ok_or(GeomError::OutOfRange)?;
        if (weight - cur).abs() > gp::resolution() {
            if let Some(slot) = self.weights.get_mut(i).and_then(|r| r.get_mut(j)) {
                *slot = weight;
            }
            self.refresh_rational();
            if !self.rat() {
                self.reset_unit_weights();
            }
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetWeightCol`.
    pub fn set_weight_col(&mut self, vindex: i32, weights: &[f64]) -> Result<()> {
        if vindex < 1 || vindex > self.nb_v() {
            return Err(GeomError::OutOfRange);
        }
        if weights.len() != self.poles.len() {
            return Err(GeomError::Construction);
        }
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
        if !Self::array_in_range(weights.len(), self.nb_v() as usize) {
            return Err(GeomError::Construction);
        }
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

    /// `NbUPoles`.
    pub fn nb_u_poles(&self) -> i32 {
        self.nb_u()
    }

    /// `NbVPoles`.
    pub fn nb_v_poles(&self) -> i32 {
        self.nb_v()
    }

    /// `Pole`.
    pub fn pole(&self, uindex: i32, vindex: i32) -> Result<Pnt> {
        let (i, j) = self.check_uv(uindex, vindex)?;
        self.poles.get(i).and_then(|r| r.get(j)).copied().ok_or(GeomError::OutOfRange)
    }

    /// `Poles`.
    pub fn poles(&self) -> &[Vec<Pnt>] {
        &self.poles
    }

    /// `UDegree`.
    pub fn u_degree(&self) -> i32 {
        self.nb_u() - 1
    }

    /// `VDegree`.
    pub fn v_degree(&self) -> i32 {
        self.nb_v() - 1
    }

    /// `Weight`.
    pub fn weight(&self, uindex: i32, vindex: i32) -> Result<f64> {
        let (i, j) = self.check_uv(uindex, vindex)?;
        if self.rat() {
            self.weights.get(i).and_then(|r| r.get(j)).copied().ok_or(GeomError::OutOfRange)
        } else {
            Ok(1.0)
        }
    }

    /// `Weights` (`None` when the surface is not rational).
    pub fn weights(&self) -> Option<&[Vec<f64>]> {
        if self.rat() { Some(&self.weights) } else { None }
    }

    /// `IsURational`.
    pub fn is_u_rational(&self) -> bool {
        self.u_rational
    }

    /// `IsVRational`.
    pub fn is_v_rational(&self) -> bool {
        self.v_rational
    }

    /// `MaxDegree`.
    pub fn max_degree() -> i32 {
        crate::bsplclib::max_degree()
    }

    /// `UKnots`.
    pub fn u_knots(&self) -> Vec<f64> {
        vec![0.0, 1.0]
    }

    /// `VKnots`.
    pub fn v_knots(&self) -> Vec<f64> {
        vec![0.0, 1.0]
    }

    /// `UMultiplicities`.
    pub fn u_multiplicities(&self) -> Vec<i32> {
        vec![self.nb_u(), self.nb_u()]
    }

    /// `VMultiplicities`.
    pub fn v_multiplicities(&self) -> Vec<i32> {
        vec![self.nb_v(), self.nb_v()]
    }

    /// `UKnotSequence`.
    pub fn u_knot_sequence(&self) -> Vec<f64> {
        flat_bezier_knots(self.u_degree())
    }

    /// `VKnotSequence`.
    pub fn v_knot_sequence(&self) -> Vec<f64> {
        flat_bezier_knots(self.v_degree())
    }

    /// `Resolution`: `(UTolerance, VTolerance)`.
    pub fn resolution(&mut self, tolerance_3d: f64) -> (f64, f64) {
        if !self.max_deriv_inv_ok {
            let r = self.with_surface(|s| Ok(bsplslib::resolution(s, 1.0)?));
            if let Ok((u, v)) = r {
                self.u_max_deriv_inv = u;
                self.v_max_deriv_inv = v;
            }
            self.max_deriv_inv_ok = true;
        }
        (tolerance_3d * self.u_max_deriv_inv, tolerance_3d * self.v_max_deriv_inv)
    }

    // Runs `f` on the `BSplSLib` view of the surface (the Bezier knots and multiplicities, `UKnots()` for both).
    fn with_surface<R>(&self, f: impl FnOnce(&BSurface<'_>) -> Result<R>) -> Result<R> {
        let pa = to_array2(&self.poles)?;
        let wa = if self.rat() { Some(to_array2(&self.weights)?) } else { None };
        let um = self.u_multiplicities();
        let vm = self.v_multiplicities();
        let knots = [0.0, 1.0];
        let s = BSurface {
            u_degree: self.u_degree(),
            v_degree: self.v_degree(),
            u_rational: self.u_rational,
            v_rational: self.v_rational,
            u_periodic: false,
            v_periodic: false,
            poles: &pa,
            weights: wa.as_ref(),
            u_knots: &knots,
            v_knots: &knots,
            u_mults: Some(&um),
            v_mults: Some(&vm),
        };
        f(&s)
    }

    // The copy constructor
    fn copy_bezier(&self) -> BezierSurface {
        BezierSurface {
            poles: self.poles.clone(),
            weights: if self.rat() {
                self.weights.clone()
            } else {
                unit_weights(self.poles.len(), self.poles.first().map_or(0, Vec::len))
            },
            eval_rep: rep_utils::clone_surface_desc(&self.eval_rep),
            u_rational: self.u_rational,
            v_rational: self.v_rational,
            u_max_deriv_inv: self.u_max_deriv_inv,
            v_max_deriv_inv: self.v_max_deriv_inv,
            max_deriv_inv_ok: false,
        }
    }
}

impl Geometry for BezierSurface {
    geometry_any!();
    fn transform(&mut self, t: &Trsf) {
        for row in &mut self.poles {
            for p in row {
                p.transform(t);
            }
        }
        self.eval_rep = None;
        self.max_deriv_inv_ok = false;
    }
    fn copy_geometry(&self) -> Box<dyn Geometry> {
        Box::new(self.copy_bezier())
    }
}

impl Surface for BezierSurface {
    fn copy_surface(&self) -> Box<dyn Surface> {
        Box::new(self.copy_bezier())
    }
    fn u_reverse(&mut self) {
        let nu = self.poles.len();
        self.poles.reverse();
        if self.rat() {
            self.weights.reverse();
        }
        let _ = nu;
        self.eval_rep = None;
        self.max_deriv_inv_ok = false;
    }
    fn u_reversed_parameter(&self, u: f64) -> f64 {
        1.0 - u
    }
    fn v_reverse(&mut self) {
        for row in &mut self.poles {
            row.reverse();
        }
        if self.rat() {
            for row in &mut self.weights {
                row.reverse();
            }
        }
        self.eval_rep = None;
        self.max_deriv_inv_ok = false;
    }
    fn v_reversed_parameter(&self, v: f64) -> f64 {
        1.0 - v
    }
    fn bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, 1.0, 0.0, 1.0)
    }
    fn is_u_closed(&self) -> bool {
        let (Some(first), Some(last)) = (self.poles.first(), self.poles.last()) else { return true };
        first.iter().zip(last.iter()).all(|(a, b)| a.distance(b) <= confusion())
    }
    fn is_v_closed(&self) -> bool {
        self.poles.iter().all(|row| match (row.first(), row.last()) {
            (Some(a), Some(b)) => a.distance(b) <= confusion(),
            _ => true,
        })
    }
    fn is_u_periodic(&self) -> bool {
        false
    }
    fn is_v_periodic(&self) -> bool {
        false
    }
    fn u_iso(&self, u: f64) -> Result<Box<dyn Curve>> {
        let pa = to_array2(&self.poles)?;
        let um = self.u_multiplicities();
        if self.rat() {
            let wa = to_array2(&self.weights)?;
            let (p, w) = bsplslib::iso(u, true, &pa, Some(&wa), &[0.0, 1.0], Some(&um), self.u_degree(), false, true)?;
            if self.u_rational {
                Ok(Box::new(BezierCurve::with_weights(&p, &w.unwrap_or_default())?))
            } else {
                Ok(Box::new(BezierCurve::new(&p)?))
            }
        } else {
            let (p, _) = bsplslib::iso(u, true, &pa, None, &[0.0, 1.0], Some(&um), self.u_degree(), false, false)?;
            Ok(Box::new(BezierCurve::new(&p)?))
        }
    }
    fn v_iso(&self, v: f64) -> Result<Box<dyn Curve>> {
        let pa = to_array2(&self.poles)?;
        let vm = self.v_multiplicities();
        if self.rat() {
            let wa = to_array2(&self.weights)?;
            let (p, w) = bsplslib::iso(v, false, &pa, Some(&wa), &[0.0, 1.0], Some(&vm), self.v_degree(), false, true)?;
            if self.v_rational {
                Ok(Box::new(BezierCurve::with_weights(&p, &w.unwrap_or_default())?))
            } else {
                Ok(Box::new(BezierCurve::new(&p)?))
            }
        } else {
            let (p, _) = bsplslib::iso(v, false, &pa, None, &[0.0, 1.0], Some(&vm), self.v_degree(), false, false)?;
            Ok(Box::new(BezierCurve::new(&p)?))
        }
    }
    fn continuity(&self) -> GeomAbsShape {
        GeomAbsShape::Cn
    }
    fn is_cn_u(&self, _n: i32) -> Result<bool> {
        Ok(true)
    }
    fn is_cn_v(&self, _n: i32) -> Result<bool> {
        Ok(true)
    }
    fn eval_d0(&self, u: f64, v: f64) -> Result<Pnt> {
        if let Some(r) = rep_utils::try_eval_surface_d0(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        self.with_surface(|s| Ok(bsplslib::d0(u, v, 1, 1, s)?))
    }
    fn eval_d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        if let Some(r) = rep_utils::try_eval_surface_d1(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        self.with_surface(|s| {
            let (point, d1u, d1v) = bsplslib::d1(u, v, 1, 1, s)?;
            Ok(SurfResD1 { point, d1u, d1v })
        })
    }
    fn eval_d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        if let Some(r) = rep_utils::try_eval_surface_d2(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        self.with_surface(|s| {
            let (point, d1u, d1v, d2u, d2v, d2uv) = bsplslib::d2(u, v, 1, 1, s)?;
            Ok(SurfResD2 { point, d1u, d1v, d2u, d2v, d2uv })
        })
    }
    fn eval_d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        if let Some(r) = rep_utils::try_eval_surface_d3(&self.eval_rep, u, v)? {
            return Ok(r);
        }
        self.with_surface(|s| {
            let r = bsplslib::d3(u, v, 0, 0, s)?;
            Ok(SurfResD3 {
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
            })
        })
    }
    fn eval_dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<gp::Vec> {
        if let Some(r) = rep_utils::try_eval_surface_dn(&self.eval_rep, u, v, nu, nv)? {
            return Ok(r);
        }
        if nu + nv < 1 || nu < 0 || nv < 0 {
            return Err(GeomError::UndefinedDerivative);
        }
        self.with_surface(|s| Ok(bsplslib::dn(u, v, nu, nv, 0, 0, s)?))
    }
}

impl BoundedSurface for BezierSurface {}
