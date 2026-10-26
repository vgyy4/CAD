//! `Geom2d_BezierCurve`.
use super::geometry_any;
use super::rep_utils::{self, EvalRepDesc};
use super::{BoundedCurve2d, Curve2d, Geom2dError, GeomAbsShape, Geometry2d, ResD1, ResD2, ResD3, Result};
use crate::bsplclib::{self, curve2d};
use crate::gp::{self, Pnt2d, Trsf2d, Vec2d};
use crate::plib;

/// `Geom2d_BezierCurve`.
#[derive(Debug, Clone)]
pub struct BezierCurve {
    poles: Vec<Pnt2d>,
    weights: Vec<f64>,
    eval_rep: Option<EvalRepDesc>,
    rational: bool,
    closed: bool,
    max_deriv_inv: f64,
    max_deriv_inv_ok: bool,
}

// Rational
fn rational_weights(w: &[f64]) -> bool {
    let n = w.len();
    let mut rat = false;
    let mut i = 1;
    while i < n {
        if let (Some(a), Some(b)) = (w.get(i - 1), w.get(i)) {
            rat = (a - b).abs() > gp::resolution();
        }
        if rat {
            break;
        }
        i += 1;
    }
    rat
}

fn unit_weights(n: usize) -> Vec<f64> {
    vec![1.0; n]
}

impl BezierCurve {
    /// `Geom2d_BezierCurve(Poles)`.
    pub fn new(poles: &[Pnt2d]) -> Result<Self> {
        let nbpoles = poles.len();
        if nbpoles < 2 || nbpoles > (Self::max_degree() as usize + 1) {
            return Err(Geom2dError::Construction);
        }
        let mut c = BezierCurve {
            poles: Vec::new(),
            weights: Vec::new(),
            eval_rep: None,
            rational: false,
            closed: false,
            max_deriv_inv: 0.0,
            max_deriv_inv_ok: false,
        };
        c.init(poles, None);
        Ok(c)
    }

    /// `Geom2d_BezierCurve(Poles, Weights)`.
    pub fn with_weights(poles: &[Pnt2d], weights: &[f64]) -> Result<Self> {
        let nbpoles = poles.len();
        if nbpoles < 2 || nbpoles > (Self::max_degree() as usize + 1) {
            return Err(Geom2dError::Construction);
        }
        if weights.len() != nbpoles {
            return Err(Geom2dError::Construction);
        }
        if weights.iter().any(|w| *w <= gp::resolution()) {
            return Err(Geom2dError::Construction);
        }
        let mut c = BezierCurve {
            poles: Vec::new(),
            weights: Vec::new(),
            eval_rep: None,
            rational: false,
            closed: false,
            max_deriv_inv: 0.0,
            max_deriv_inv_ok: false,
        };
        if rational_weights(weights) {
            c.init(poles, Some(weights));
        } else {
            c.init(poles, None);
        }
        Ok(c)
    }

    // init
    fn init(&mut self, poles: &[Pnt2d], weights: Option<&[f64]>) {
        let nbpoles = poles.len();
        self.closed = match (poles.first(), poles.last()) {
            (Some(a), Some(b)) => a.distance(b) <= gp::resolution(),
            _ => false,
        };
        self.poles = poles.to_vec();
        if let Some(w) = weights {
            self.weights = w.to_vec();
            self.rational = rational_weights(&self.weights);
            if !self.rational {
                self.weights = unit_weights(nbpoles);
            }
        } else {
            self.rational = false;
            self.weights = unit_weights(nbpoles);
        }
        self.max_deriv_inv = 0.0;
        self.max_deriv_inv_ok = false;
        self.eval_rep = None;
    }

    fn weights_opt(&self) -> Option<&[f64]> {
        if self.rational { Some(&self.weights) } else { None }
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

    /// `Increase`.
    pub fn increase(&mut self, degree: i32) -> Result<()> {
        if degree == self.degree() {
            return Ok(());
        }
        if degree < self.degree() || degree > Self::max_degree() {
            return Err(Geom2dError::Construction);
        }
        let knots = self.knots();
        let mults = self.multiplicities();
        let ((npoles, nweights), _, _) =
            curve2d::increase_degree(self.degree(), degree, false, &self.poles, self.weights_opt(), &knots, &mults)?;
        if self.is_rational() {
            self.init(&npoles, nweights.as_deref());
        } else {
            self.init(&npoles, None);
        }
        Ok(())
    }

    /// `InsertPoleAfter`.
    pub fn insert_pole_after(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> {
        let nbpoles = self.nb_poles();
        if nbpoles >= Self::max_degree() || weight <= gp::resolution() {
            return Err(Geom2dError::Construction);
        }
        if index < 0 || index > nbpoles {
            return Err(Geom2dError::OutOfRange);
        }
        let idx = index as usize;
        let mut npoles = Vec::with_capacity(self.poles.len() + 1);
        npoles.extend_from_slice(self.poles.get(..idx).ok_or(Geom2dError::OutOfRange)?);
        npoles.push(p);
        npoles.extend_from_slice(self.poles.get(idx..).ok_or(Geom2dError::OutOfRange)?);
        let rat = self.is_rational() || (weight - 1.0).abs() > gp::resolution();
        if rat {
            let old: Vec<f64> = if self.is_rational() { self.weights.clone() } else { unit_weights(self.poles.len()) };
            let mut nweights = Vec::with_capacity(old.len() + 1);
            nweights.extend_from_slice(old.get(..idx).ok_or(Geom2dError::OutOfRange)?);
            nweights.push(weight);
            nweights.extend_from_slice(old.get(idx..).ok_or(Geom2dError::OutOfRange)?);
            self.init(&npoles, Some(&nweights));
        } else {
            self.init(&npoles, None);
        }
        Ok(())
    }

    /// `InsertPoleBefore`.
    pub fn insert_pole_before(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> {
        self.insert_pole_after(index - 1, p, weight)
    }

    /// `RemovePole`.
    pub fn remove_pole(&mut self, index: i32) -> Result<()> {
        let nbpoles = self.nb_poles();
        if nbpoles <= 2 {
            return Err(Geom2dError::Construction);
        }
        if index < 1 || index > nbpoles {
            return Err(Geom2dError::OutOfRange);
        }
        let k = (index - 1) as usize;
        let mut npoles = self.poles.clone();
        npoles.remove(k);
        if self.is_rational() {
            let mut nweights = self.weights.clone();
            nweights.remove(k);
            self.init(&npoles, Some(&nweights));
        } else {
            self.init(&npoles, None);
        }
        Ok(())
    }

    /// `Segment`.
    pub fn segment(&mut self, u1: f64, u2: f64) {
        let (Ok(p1), Ok(p2)) = (self.eval_d0(u1), self.eval_d0(u2)) else {
            return;
        };
        let closed = p1.distance(&p2).abs() <= gp::resolution();
        let flat = self.knot_sequence();
        let Ok((coeffs, wcoeffs)) =
            curve2d::build_cache(0.0, 1.0, false, self.degree(), &flat, &self.poles, self.weights_opt())
        else {
            return;
        };
        self.closed = closed;
        if self.is_rational() {
            let Ok((c, w)) = plib::trimming_2d(u1, u2, &coeffs, wcoeffs.as_deref()) else { return };
            if let Ok((np, nw)) = plib::coefficients_poles_2d(&c, w.as_deref()) {
                self.poles = np;
                if let Some(nw) = nw {
                    self.weights = nw;
                }
            }
        } else if let Ok((c, _)) = plib::trimming_2d(u1, u2, &coeffs, None)
            && let Ok((np, _)) = plib::coefficients_poles_2d(&c, None)
        {
            self.poles = np;
        }
        self.max_deriv_inv_ok = false;
    }

    /// `SetPole`.
    pub fn set_pole(&mut self, index: i32, p: Pnt2d) -> Result<()> {
        if index < 1 || index > self.nb_poles() {
            return Err(Geom2dError::OutOfRange);
        }
        let k = (index - 1) as usize;
        if let Some(slot) = self.poles.get_mut(k) {
            *slot = p;
        }
        if (index == 1 || index as usize == self.poles.len())
            && let (Some(a), Some(b)) = (self.poles.first(), self.poles.last())
        {
            self.closed = a.distance(b) <= gp::resolution();
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `SetPole(Index, P, Weight)`.
    pub fn set_pole_weight(&mut self, index: i32, p: Pnt2d, weight: f64) -> Result<()> {
        self.set_pole(index, p)?;
        self.set_weight(index, weight)
    }

    /// `SetWeight`.
    pub fn set_weight(&mut self, index: i32, weight: f64) -> Result<()> {
        let nbpoles = self.nb_poles();
        if index < 1 || index > nbpoles {
            return Err(Geom2dError::OutOfRange);
        }
        if weight <= gp::resolution() {
            return Err(Geom2dError::Construction);
        }
        let wasrat = self.is_rational();
        if !wasrat && (weight - 1.0).abs() <= gp::resolution() {
            return Ok(());
        }
        if let Some(slot) = self.weights.get_mut((index - 1) as usize) {
            *slot = weight;
        }
        if wasrat && !rational_weights(&self.weights) {
            self.rational = false;
            self.weights = unit_weights(nbpoles as usize);
        } else {
            self.rational = true;
        }
        self.max_deriv_inv_ok = false;
        Ok(())
    }

    /// `IsRational`.
    pub fn is_rational(&self) -> bool {
        self.rational
    }

    /// `Degree`.
    pub fn degree(&self) -> i32 {
        self.poles.len() as i32 - 1
    }

    /// `NbPoles`.
    pub fn nb_poles(&self) -> i32 {
        self.poles.len() as i32
    }

    /// `Pole`.
    pub fn pole(&self, index: i32) -> Result<Pnt2d> {
        if index < 1 || index > self.nb_poles() {
            return Err(Geom2dError::OutOfRange);
        }
        self.poles.get((index - 1) as usize).copied().ok_or(Geom2dError::OutOfRange)
    }

    /// `Poles`.
    pub fn poles(&self) -> &[Pnt2d] {
        &self.poles
    }

    /// `Weight`.
    pub fn weight(&self, index: i32) -> Result<f64> {
        if index < 1 || index > self.nb_poles() {
            return Err(Geom2dError::OutOfRange);
        }
        if self.is_rational() {
            self.weights.get((index - 1) as usize).copied().ok_or(Geom2dError::OutOfRange)
        } else {
            Ok(1.0)
        }
    }

    /// `Weights` (`None` is `BSplCLib::NoWeights()`).
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights_opt()
    }

    /// `Knots`: `{0, 1}`.
    pub fn knots(&self) -> Vec<f64> {
        vec![0.0, 1.0]
    }

    /// `Multiplicities`: `{n, n}` with `n` the number of poles.
    pub fn multiplicities(&self) -> Vec<i32> {
        let n = self.nb_poles();
        vec![n, n]
    }

    /// `KnotSequence`: `FlatBezierKnots(Degree)`.
    pub fn knot_sequence(&self) -> Vec<f64> {
        let n = self.poles.len();
        let mut k = vec![0.0; n];
        k.extend(std::iter::repeat_n(1.0, n));
        k
    }

    /// `MaxDegree`.
    pub fn max_degree() -> i32 {
        bsplclib::max_degree()
    }

    /// `Resolution`.
    pub fn resolution(&mut self, tolerance_uv: f64) -> f64 {
        if !self.max_deriv_inv_ok {
            let flat = self.knot_sequence();
            if let Ok(v) =
                curve2d::resolution(&self.poles, self.weights_opt(), self.nb_poles(), &flat, self.degree(), 1.0)
            {
                self.max_deriv_inv = v;
            }
            self.max_deriv_inv_ok = true;
        }
        tolerance_uv * self.max_deriv_inv
    }
}

impl Geometry2d for BezierCurve {
    geometry_any!();
    fn transform(&mut self, t: &Trsf2d) {
        for p in &mut self.poles {
            p.transform(t);
        }
        self.eval_rep = None;
        self.max_deriv_inv_ok = false;
    }
    fn copy_geometry(&self) -> Box<dyn Geometry2d> {
        Box::new(self.copy_bezier())
    }
}

impl BezierCurve {
    // The copy constructor
    fn copy_bezier(&self) -> BezierCurve {
        BezierCurve {
            poles: self.poles.clone(),
            weights: if self.rational { self.weights.clone() } else { unit_weights(self.poles.len()) },
            eval_rep: rep_utils::clone_curve_desc(&self.eval_rep),
            rational: self.rational,
            closed: self.closed,
            max_deriv_inv: 0.0,
            max_deriv_inv_ok: false,
        }
    }
}

impl Curve2d for BezierCurve {
    fn copy_curve(&self) -> Box<dyn Curve2d> {
        Box::new(self.copy_bezier())
    }
    fn reverse(&mut self) {
        self.poles.reverse();
        if self.is_rational() {
            self.weights.reverse();
        }
        self.max_deriv_inv_ok = false;
    }
    fn reversed_parameter(&self, u: f64) -> f64 {
        1.0 - u
    }
    fn first_parameter(&self) -> f64 {
        0.0
    }
    fn last_parameter(&self) -> f64 {
        1.0
    }
    fn is_closed(&self) -> bool {
        self.closed
    }
    fn is_periodic(&self) -> bool {
        false
    }
    fn continuity(&self) -> GeomAbsShape {
        GeomAbsShape::Cn
    }
    fn is_cn(&self, _n: i32) -> Result<bool> {
        Ok(true)
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        if let Some(r) = rep_utils::try_eval_curve_d0(&self.eval_rep, u)? {
            return Ok(r);
        }
        let mults = self.multiplicities();
        Ok(curve2d::d0(u, 1, self.degree(), false, &self.poles, self.weights_opt(), &self.knots(), Some(&mults))?)
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        if let Some(r) = rep_utils::try_eval_curve_d1(&self.eval_rep, u)? {
            return Ok(r);
        }
        let mults = self.multiplicities();
        let (point, d1) =
            curve2d::d1(u, 1, self.degree(), false, &self.poles, self.weights_opt(), &self.knots(), Some(&mults))?;
        Ok(ResD1 { point, d1 })
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        if let Some(r) = rep_utils::try_eval_curve_d2(&self.eval_rep, u)? {
            return Ok(r);
        }
        let mults = self.multiplicities();
        let (point, d1, d2) =
            curve2d::d2(u, 1, self.degree(), false, &self.poles, self.weights_opt(), &self.knots(), Some(&mults))?;
        Ok(ResD2 { point, d1, d2 })
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        if let Some(r) = rep_utils::try_eval_curve_d3(&self.eval_rep, u)? {
            return Ok(r);
        }
        let mults = self.multiplicities();
        let (point, d1, d2, d3) =
            curve2d::d3(u, 1, self.degree(), false, &self.poles, self.weights_opt(), &self.knots(), Some(&mults))?;
        Ok(ResD3 { point, d1, d2, d3 })
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        if n < 1 {
            return Err(Geom2dError::UndefinedDerivative);
        }
        if let Some(r) = rep_utils::try_eval_curve_dn(&self.eval_rep, u, n)? {
            return Ok(r);
        }
        let mults = self.multiplicities();
        Ok(curve2d::dn(u, n, 0, self.degree(), false, &self.poles, self.weights_opt(), &self.knots(), Some(&mults))?)
    }
}

impl BoundedCurve2d for BezierCurve {
    fn start_point(&self) -> Pnt2d {
        self.poles.first().copied().unwrap_or_else(Pnt2d::origin)
    }
    fn end_point(&self) -> Pnt2d {
        self.poles.last().copied().unwrap_or_else(Pnt2d::origin)
    }
}
