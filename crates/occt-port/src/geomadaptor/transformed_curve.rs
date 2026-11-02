//! `GeomAdaptor_TransformedCurve`: a `GeomAdaptor_Curve` (or an `Adaptor3d_CurveOnSurface`) followed by a `gp_Trsf`.
//!
//! Translated from `GeomAdaptor_TransformedCurve.hxx/.cxx` (OCCT 8.0.1).
use super::Result;
use super::curve::Curve;
use crate::adaptor2d::{AdaptorError, GeomAbsCurveType, GeomAbsShape};
use crate::adaptor3d::{Curve3dAdaptor, CurveOnSurface};
use crate::geom::{BSplineCurve, BezierCurve, Curve as GeomCurve, Geometry, OffsetCurve, ResD1, ResD2, ResD3};
use crate::gp::{Circ, Elips, Hypr, Lin, Parab, Pnt, Trsf, TrsfForm, Vec};
use std::sync::Arc;

fn tv(v: &mut Vec, t: &Trsf) -> Result<()> {
    v.transform(t).map_err(|_| AdaptorError::Construction)
}

/// `GeomAdaptor_TransformedCurve`.
#[derive(Debug)]
pub struct TransformedCurve {
    curve: Curve,
    con_surf: Option<Arc<CurveOnSurface>>,
    trsf: Trsf,
}

impl Default for TransformedCurve {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformedCurve {
    /// Empty adaptor.
    pub fn new() -> Self {
        Self { curve: Curve::new(), con_surf: None, trsf: Trsf::new() }
    }
    /// Constructor from a curve and a transformation.
    pub fn with_curve(c: Arc<dyn GeomCurve>, trsf: Trsf) -> Result<Self> {
        Ok(Self { curve: Curve::with_curve(c)?, con_surf: None, trsf })
    }
    /// Constructor from a curve, a range and a transformation.
    pub fn with_range(c: Arc<dyn GeomCurve>, first: f64, last: f64, trsf: Trsf) -> Result<Self> {
        Ok(Self { curve: Curve::with_range(c, first, last)?, con_surf: None, trsf })
    }
    /// `Load`.
    pub fn load(&mut self, c: Arc<dyn GeomCurve>) -> Result<()> {
        self.curve.load(c)
    }
    /// `Load` with a range.
    pub fn load_range(&mut self, c: Arc<dyn GeomCurve>, first: f64, last: f64) -> Result<()> {
        self.curve.load_range(c, first, last)
    }
    /// `LoadCurveOnSurface`.
    pub fn load_curve_on_surface(&mut self, c: Arc<CurveOnSurface>) {
        self.con_surf = Some(c);
    }
    /// `SetTrsf`.
    pub fn set_trsf(&mut self, t: Trsf) {
        self.trsf = t;
    }
    /// `Trsf`.
    pub fn trsf(&self) -> &Trsf {
        &self.trsf
    }
    /// `Is3DCurve`.
    pub fn is_3d_curve(&self) -> bool {
        self.con_surf.is_none()
    }
    /// `IsCurveOnSurface`.
    pub fn is_curve_on_surface(&self) -> bool {
        self.con_surf.is_some()
    }
    /// `Curve`.
    pub fn curve(&self) -> &Curve {
        &self.curve
    }
    /// `ChangeCurve`.
    pub fn change_curve(&mut self) -> &mut Curve {
        &mut self.curve
    }
    /// `CurveOnSurface` (`None`: the dereferenced null handle of C++).
    pub fn curve_on_surface(&self) -> Option<&Arc<CurveOnSurface>> {
        self.con_surf.as_ref()
    }
    /// `GeomCurve`.
    pub fn geom_curve(&self) -> Option<&Arc<dyn GeomCurve>> {
        self.curve.curve()
    }

    fn inner(&self) -> &dyn Curve3dAdaptor {
        match &self.con_surf {
            None => &self.curve,
            Some(c) => c.as_ref(),
        }
    }

    /// The concrete-type `ShallowCopy`.
    pub fn shallow_copied(&self) -> Result<TransformedCurve> {
        let curve = self.curve.shallow_copied()?;
        let con_surf = match &self.con_surf {
            None => None,
            Some(c) => Some(Arc::new(c.shallow_copied()?)),
        };
        Ok(TransformedCurve { curve, con_surf, trsf: self.trsf })
    }

    /// The concrete-type `Trim`.
    pub fn trimmed(&self, first: f64, last: f64, tol: f64) -> Result<TransformedCurve> {
        let mut copy = TransformedCurve::new();
        match &self.con_surf {
            None => copy.curve.load_range_opt(self.curve.curve().cloned(), first, last)?,
            Some(c) => copy.con_surf = Some(Arc::new(c.trimmed(first, last, tol)?)),
        }
        copy.trsf = self.trsf;
        Ok(copy)
    }

    fn is_identity(&self) -> bool {
        self.trsf.form() == TrsfForm::Identity
    }
}

impl Curve3dAdaptor for TransformedCurve {
    fn shallow_copy(&self) -> Result<Arc<dyn Curve3dAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }
    fn first_parameter(&self) -> Result<f64> {
        self.inner().first_parameter()
    }
    fn last_parameter(&self) -> Result<f64> {
        self.inner().last_parameter()
    }
    fn continuity(&self) -> Result<GeomAbsShape> {
        self.inner().continuity()
    }
    fn nb_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        self.inner().nb_intervals(s)
    }
    fn intervals(&self, t: &mut [f64], s: GeomAbsShape) -> Result<()> {
        self.inner().intervals(t, s)
    }
    fn trim(&self, first: f64, last: f64, tol: f64) -> Result<Arc<dyn Curve3dAdaptor>> {
        Ok(Arc::new(self.trimmed(first, last, tol)?))
    }
    fn is_closed(&self) -> Result<bool> {
        self.inner().is_closed()
    }
    fn is_periodic(&self) -> Result<bool> {
        self.inner().is_periodic()
    }
    fn period(&self) -> Result<f64> {
        self.inner().period()
    }
    fn resolution(&self, r3d: f64) -> Result<f64> {
        self.inner().resolution(r3d)
    }
    fn get_type(&self) -> Result<GeomAbsCurveType> {
        self.inner().get_type()
    }
    fn line(&self) -> Result<Lin> {
        let mut l = self.inner().line()?;
        l.transform(&self.trsf);
        Ok(l)
    }
    fn circle(&self) -> Result<Circ> {
        let mut l = self.inner().circle()?;
        l.transform(&self.trsf);
        Ok(l)
    }
    fn ellipse(&self) -> Result<Elips> {
        let mut l = self.inner().ellipse()?;
        l.transform(&self.trsf);
        Ok(l)
    }
    fn hyperbola(&self) -> Result<Hypr> {
        let mut l = self.inner().hyperbola()?;
        l.transform(&self.trsf);
        Ok(l)
    }
    fn parabola(&self) -> Result<Parab> {
        let mut l = self.inner().parabola()?;
        l.transform(&self.trsf);
        Ok(l)
    }
    fn degree(&self) -> Result<i32> {
        self.inner().degree()
    }
    fn is_rational(&self) -> Result<bool> {
        self.inner().is_rational()
    }
    fn nb_poles(&self) -> Result<i32> {
        self.inner().nb_poles()
    }
    fn nb_knots(&self) -> Result<i32> {
        self.inner().nb_knots()
    }
    fn bezier(&self) -> Result<Option<Arc<BezierCurve>>> {
        let b = self.inner().bezier()?;
        if self.is_identity() {
            return Ok(b);
        }
        let b = b.ok_or(AdaptorError::NullObject)?;
        let mut c = (*b).clone();
        c.transform(&self.trsf);
        Ok(Some(Arc::new(c)))
    }
    fn bspline(&self) -> Result<Option<Arc<BSplineCurve>>> {
        let b = self.inner().bspline()?;
        if self.is_identity() {
            return Ok(b);
        }
        let b = b.ok_or(AdaptorError::NullObject)?;
        let mut c = (*b).clone();
        c.transform(&self.trsf);
        Ok(Some(Arc::new(c)))
    }
    fn offset_curve(&self) -> Result<Option<Arc<OffsetCurve>>> {
        if !self.is_3d_curve() || self.curve.get_type()? != GeomAbsCurveType::OffsetCurve {
            return Err(AdaptorError::NoSuchObject);
        }
        let o = self.curve.offset_curve()?;
        if self.is_identity() {
            return Ok(o);
        }
        let o = o.ok_or(AdaptorError::NullObject)?;
        let mut c = (*o).clone();
        c.transform(&self.trsf);
        Ok(Some(Arc::new(c)))
    }
    fn eval_d0(&self, u: f64) -> Result<Pnt> {
        let mut p = self.inner().d0(u)?;
        p.transform(&self.trsf);
        Ok(p)
    }
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let mut r = self.inner().d1(u).map(|(point, d1)| ResD1 { point, d1 })?;
        r.point.transform(&self.trsf);
        tv(&mut r.d1, &self.trsf)?;
        Ok(r)
    }
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let mut r = self.inner().d2(u).map(|(point, d1, d2)| ResD2 { point, d1, d2 })?;
        r.point.transform(&self.trsf);
        tv(&mut r.d1, &self.trsf)?;
        tv(&mut r.d2, &self.trsf)?;
        Ok(r)
    }
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let mut r = self.inner().d3(u).map(|(point, d1, d2, d3)| ResD3 { point, d1, d2, d3 })?;
        r.point.transform(&self.trsf);
        tv(&mut r.d1, &self.trsf)?;
        tv(&mut r.d2, &self.trsf)?;
        tv(&mut r.d3, &self.trsf)?;
        Ok(r)
    }
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec> {
        let mut v = self.inner().dn(u, n)?;
        tv(&mut v, &self.trsf)?;
        Ok(v)
    }
}
