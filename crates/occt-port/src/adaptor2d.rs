//! OCCT's package `Adaptor2d`: the abstract 2D curve adaptor `Adaptor2d_Curve2d`, the line adaptor `Adaptor2d_Line2d`
//! and the offset adaptor `Adaptor2d_OffsetCurve`.
//!
//! Translated from `Adaptor2d_*.hxx/.cxx` (OCCT 8.0.1). The C++ virtual class becomes the trait [`Curve2dAdaptor`]
//! whose default methods are the base-class bodies (`Standard_NotImplemented`, `NbSamples() == 20`, the `Eval*`
//! wrappers); adaptor handles are `Arc<dyn Curve2dAdaptor>`; out-parameters become returned tuples.
// The comparisons `x == y` are part of the OCCT algorithms.
#![allow(clippy::float_cmp)]
use crate::geom2d::{BSplineCurve, BezierCurve, ResD1, ResD2, ResD3, confusion};
use crate::gp::{Ax2d, Circ2d, ConstructionError, Dir2d, Elips2d, Hypr2d, Lin2d, Parab2d, Pnt2d, Vec2d};
pub use crate::plib::GeomAbsShape;
use std::sync::Arc;

/// `GeomAbs_CurveType`; the discriminants are the C++ enumerator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum GeomAbsCurveType {
    /// `GeomAbs_Line`.
    Line = 0,
    /// `GeomAbs_Circle`.
    Circle = 1,
    /// `GeomAbs_Ellipse`.
    Ellipse = 2,
    /// `GeomAbs_Hyperbola`.
    Hyperbola = 3,
    /// `GeomAbs_Parabola`.
    Parabola = 4,
    /// `GeomAbs_BezierCurve`.
    BezierCurve = 5,
    /// `GeomAbs_BSplineCurve`.
    BSplineCurve = 6,
    /// `GeomAbs_OffsetCurve`.
    OffsetCurve = 7,
    /// `GeomAbs_OtherCurve`.
    OtherCurve = 8,
}

/// The OCCT exceptions the adaptors raise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdaptorError {
    /// `Standard_NotImplemented`.
    NotImplemented,
    /// `Standard_NoSuchObject`.
    NoSuchObject,
    /// `Standard_OutOfRange`.
    OutOfRange,
    /// `Standard_TypeMismatch`.
    TypeMismatch,
    /// `Standard_ConstructionError` (and the `gp` exceptions) from the geometry classes.
    Construction,
    /// A method called on an offset adaptor that has no curve (a null handle in C++).
    NullObject,
    /// `Standard_DomainError`.
    Domain,
    /// `Geom2d_UndefinedValue`.
    UndefinedValue,
    /// `Geom2d_UndefinedDerivative`.
    UndefinedDerivative,
    /// Any other error of the `geom2d` classes.
    Geom(crate::geom2d::Geom2dError),
    /// Any error of the `geom` classes (`Adaptor3d` adaptors hand out 3D geometry).
    Geom3d(crate::geom::GeomError),
}
impl From<crate::geom2d::Geom2dError> for AdaptorError {
    fn from(e: crate::geom2d::Geom2dError) -> Self {
        Self::Geom(e)
    }
}
impl From<crate::geom::GeomError> for AdaptorError {
    fn from(e: crate::geom::GeomError) -> Self {
        Self::Geom3d(e)
    }
}
impl From<ConstructionError> for AdaptorError {
    fn from(_: ConstructionError) -> Self {
        Self::Construction
    }
}

/// `Result` of the adaptor methods.
pub type Result<T> = std::result::Result<T, AdaptorError>;

fn shape_from_i32(v: i32) -> GeomAbsShape {
    match v {
        0 => GeomAbsShape::C0,
        1 => GeomAbsShape::G1,
        2 => GeomAbsShape::C1,
        3 => GeomAbsShape::G2,
        4 => GeomAbsShape::C2,
        5 => GeomAbsShape::C3,
        _ => GeomAbsShape::Cn,
    }
}

/// `Adaptor2d_Curve2d`. Every method is const in C++ and so takes `&self`; the default bodies are the base class's.
pub trait Curve2dAdaptor: std::fmt::Debug + Send + Sync {
    /// `ShallowCopy`.
    fn shallow_copy(&self) -> Result<Arc<dyn Curve2dAdaptor>> {
        Err(AdaptorError::NotImplemented)
    }
    /// `FirstParameter`.
    fn first_parameter(&self) -> Result<f64> {
        Err(AdaptorError::NotImplemented)
    }
    /// `LastParameter`.
    fn last_parameter(&self) -> Result<f64> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Continuity`.
    fn continuity(&self) -> Result<GeomAbsShape> {
        Err(AdaptorError::NotImplemented)
    }
    /// `NbIntervals`.
    fn nb_intervals(&self, _s: GeomAbsShape) -> Result<i32> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Intervals`: `t[0]` is `T(T.Lower())`; writing past the end of `t` is `Err(OutOfRange)`. Adaptors that `Resize` the
    /// C++ array (the B-spline ones) replace the vector.
    fn intervals(&self, _t: &mut Vec<f64>, _s: GeomAbsShape) -> Result<()> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Trim`.
    fn trim(&self, _first: f64, _last: f64, _tol: f64) -> Result<Arc<dyn Curve2dAdaptor>> {
        Err(AdaptorError::NotImplemented)
    }
    /// `IsClosed`.
    fn is_closed(&self) -> Result<bool> {
        Err(AdaptorError::NotImplemented)
    }
    /// `IsPeriodic`.
    fn is_periodic(&self) -> Result<bool> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Period`.
    fn period(&self) -> Result<f64> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Value`.
    fn value(&self, _u: f64) -> Result<Pnt2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `D0` (the point).
    fn d0(&self, _u: f64) -> Result<Pnt2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `D1` (point, first derivative).
    fn d1(&self, _u: f64) -> Result<(Pnt2d, Vec2d)> {
        Err(AdaptorError::NotImplemented)
    }
    /// `D2`.
    fn d2(&self, _u: f64) -> Result<(Pnt2d, Vec2d, Vec2d)> {
        Err(AdaptorError::NotImplemented)
    }
    /// `D3`.
    fn d3(&self, _u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        Err(AdaptorError::NotImplemented)
    }
    /// `DN`.
    fn dn(&self, _u: f64, _n: i32) -> Result<Vec2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Resolution`.
    fn resolution(&self, _r3d: f64) -> Result<f64> {
        Err(AdaptorError::NotImplemented)
    }
    /// `GetType`.
    fn get_type(&self) -> Result<GeomAbsCurveType> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Line`.
    fn line(&self) -> Result<Lin2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Circle`.
    fn circle(&self) -> Result<Circ2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Ellipse`.
    fn ellipse(&self) -> Result<Elips2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Hyperbola`.
    fn hyperbola(&self) -> Result<Hypr2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Parabola`.
    fn parabola(&self) -> Result<Parab2d> {
        Err(AdaptorError::NotImplemented)
    }
    /// `Degree`.
    fn degree(&self) -> Result<i32> {
        Err(AdaptorError::NotImplemented)
    }
    /// `IsRational`.
    fn is_rational(&self) -> Result<bool> {
        Err(AdaptorError::NotImplemented)
    }
    /// `NbPoles`.
    fn nb_poles(&self) -> Result<i32> {
        Err(AdaptorError::NotImplemented)
    }
    /// `NbKnots`.
    fn nb_knots(&self) -> Result<i32> {
        Err(AdaptorError::NotImplemented)
    }
    /// `NbSamples` (20 in the base class).
    fn nb_samples(&self) -> Result<i32> {
        Ok(20)
    }
    /// `Bezier` (`None`: a null handle).
    fn bezier(&self) -> Result<Option<Arc<BezierCurve>>> {
        Err(AdaptorError::NotImplemented)
    }
    /// `BSpline` (`None`: a null handle).
    fn bspline(&self) -> Result<Option<Arc<BSplineCurve>>> {
        Err(AdaptorError::NotImplemented)
    }
    /// The 2D curve of a `Geom2dAdaptor_Curve` (what `Geom2dAdaptor::MakeCurve` gets from
    /// `dynamic_cast<const Geom2dAdaptor_Curve*>(&HC)->Curve()`); `None` for every other adaptor.
    fn geom2d_curve(&self) -> Option<Arc<dyn crate::geom2d::Curve2d>> {
        None
    }
    /// Whether this adaptor is a `Geom2dAdaptor_Curve` (`IsKind(STANDARD_TYPE(Geom2dAdaptor_Curve))`; `Geom2dGridEval_Curve`
    /// tests it); `false` for every other adaptor.
    fn is_geom2d_adaptor(&self) -> bool {
        false
    }
    /// `EvalD0`: [`d0`](Self::d0).
    fn eval_d0(&self, u: f64) -> Result<Pnt2d> {
        self.d0(u)
    }
    /// `EvalD1`: [`d1`](Self::d1).
    fn eval_d1(&self, u: f64) -> Result<ResD1> {
        let (point, d1) = self.d1(u)?;
        Ok(ResD1 { point, d1 })
    }
    /// `EvalD2`: [`d2`](Self::d2).
    fn eval_d2(&self, u: f64) -> Result<ResD2> {
        let (point, d1, d2) = self.d2(u)?;
        Ok(ResD2 { point, d1, d2 })
    }
    /// `EvalD3`: [`d3`](Self::d3).
    fn eval_d3(&self, u: f64) -> Result<ResD3> {
        let (point, d1, d2, d3) = self.d3(u)?;
        Ok(ResD3 { point, d1, d2, d3 })
    }
    /// `EvalDN`: [`dn`](Self::dn).
    fn eval_dn(&self, u: f64, n: i32) -> Result<Vec2d> {
        self.dn(u, n)
    }
}

/// Writes `t[k]` (`NCollection_Array1::operator()` of the offset index `k`); a missing item is `Err(OutOfRange)`.
fn put(t: &mut [f64], k: usize, v: f64) -> Result<()> {
    *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = v;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Adaptor2d_Line2d
// ---------------------------------------------------------------------------------------------

/// `Adaptor2d_Line2d`: a line segment given by an axis and a parameter range.
#[derive(Debug, Clone)]
pub struct Line2d {
    u_first: f64,
    u_last: f64,
    ax: Ax2d,
}
impl Default for Line2d {
    fn default() -> Self {
        Self::new()
    }
}
impl Line2d {
    /// `Adaptor2d_Line2d()`: parameters 0, 0 and the default axis.
    pub fn new() -> Self {
        Self { u_first: 0.0, u_last: 0.0, ax: Ax2d::default() }
    }
    /// `Adaptor2d_Line2d(P, D, UFirst, ULast)`.
    pub fn with_point_dir(p: Pnt2d, d: Dir2d, u_first: f64, u_last: f64) -> Self {
        Self { u_first, u_last, ax: Ax2d::new(p, d) }
    }
    /// `Load(L)`: the whole line (`+-Precision::Infinite()`).
    pub fn load(&mut self, l: &Lin2d) {
        self.ax = *l.position();
        self.u_first = -crate::geom2d::PRECISION_INFINITE;
        self.u_last = crate::geom2d::PRECISION_INFINITE;
    }
    /// `Load(L, UFirst, ULast)`.
    pub fn load_range(&mut self, l: &Lin2d, u_first: f64, u_last: f64) {
        self.ax = *l.position();
        self.u_first = u_first;
        self.u_last = u_last;
    }
}
impl Curve2dAdaptor for Line2d {
    fn shallow_copy(&self) -> Result<Arc<dyn Curve2dAdaptor>> {
        Ok(Arc::new(self.clone()))
    }
    fn first_parameter(&self) -> Result<f64> {
        Ok(self.u_first)
    }
    fn last_parameter(&self) -> Result<f64> {
        Ok(self.u_last)
    }
    fn continuity(&self) -> Result<GeomAbsShape> {
        Ok(GeomAbsShape::Cn)
    }
    fn nb_intervals(&self, _s: GeomAbsShape) -> Result<i32> {
        Ok(1)
    }
    fn intervals(&self, t: &mut Vec<f64>, _s: GeomAbsShape) -> Result<()> {
        put(t, 0, self.u_first)?;
        put(t, 1, self.u_last)
    }
    fn trim(&self, first: f64, last: f64, _tol: f64) -> Result<Arc<dyn Curve2dAdaptor>> {
        let mut hl = Line2d::new();
        hl.load_range(&Lin2d::from_ax2d(self.ax), first, last);
        Ok(Arc::new(hl))
    }
    fn is_closed(&self) -> Result<bool> {
        Ok(false)
    }
    fn is_periodic(&self) -> Result<bool> {
        Ok(false)
    }
    fn period(&self) -> Result<f64> {
        Err(AdaptorError::NoSuchObject)
    }
    fn value(&self, u: f64) -> Result<Pnt2d> {
        Ok(crate::elclib::line2d_value(u, &self.ax))
    }
    fn d0(&self, u: f64) -> Result<Pnt2d> {
        Ok(crate::elclib::line2d_value(u, &self.ax))
    }
    fn d1(&self, u: f64) -> Result<(Pnt2d, Vec2d)> {
        Ok(crate::elclib::line2d_d1(u, &self.ax))
    }
    fn d2(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d)> {
        let (p, v1) = crate::elclib::line2d_d1(u, &self.ax);
        Ok((p, v1, Vec2d::new(0.0, 0.0)))
    }
    fn d3(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let (p, v1) = crate::elclib::line2d_d1(u, &self.ax);
        Ok((p, v1, Vec2d::new(0.0, 0.0), Vec2d::new(0.0, 0.0)))
    }
    fn dn(&self, _u: f64, n: i32) -> Result<Vec2d> {
        if n <= 0 {
            return Err(AdaptorError::OutOfRange);
        }
        if n == 1 {
            return Ok(Vec2d::from_dir(self.ax.direction()));
        }
        Ok(Vec2d::new(0.0, 0.0))
    }
    fn resolution(&self, r3d: f64) -> Result<f64> {
        Ok(r3d)
    }
    fn get_type(&self) -> Result<GeomAbsCurveType> {
        Ok(GeomAbsCurveType::Line)
    }
    fn line(&self) -> Result<Lin2d> {
        Ok(Lin2d::from_ax2d(self.ax))
    }
    fn circle(&self) -> Result<Circ2d> {
        Err(AdaptorError::NoSuchObject)
    }
    fn ellipse(&self) -> Result<Elips2d> {
        Err(AdaptorError::NoSuchObject)
    }
    fn hyperbola(&self) -> Result<Hypr2d> {
        Err(AdaptorError::NoSuchObject)
    }
    fn parabola(&self) -> Result<Parab2d> {
        Err(AdaptorError::NoSuchObject)
    }
    fn degree(&self) -> Result<i32> {
        Err(AdaptorError::NoSuchObject)
    }
    fn is_rational(&self) -> Result<bool> {
        Err(AdaptorError::NoSuchObject)
    }
    fn nb_poles(&self) -> Result<i32> {
        Err(AdaptorError::NoSuchObject)
    }
    fn nb_knots(&self) -> Result<i32> {
        Err(AdaptorError::NoSuchObject)
    }
    fn bezier(&self) -> Result<Option<Arc<BezierCurve>>> {
        Err(AdaptorError::NoSuchObject)
    }
    fn bspline(&self) -> Result<Option<Arc<BSplineCurve>>> {
        Err(AdaptorError::NoSuchObject)
    }
}

// ---------------------------------------------------------------------------------------------
// Adaptor2d_OffsetCurve
// ---------------------------------------------------------------------------------------------

/// `Adaptor2d_OffsetCurve`: the offset of an adapted curve by a signed distance along its normal.
#[derive(Debug, Clone)]
pub struct OffsetCurve {
    curve: Option<Arc<dyn Curve2dAdaptor>>,
    offset: f64,
    first: f64,
    last: f64,
}
impl Default for OffsetCurve {
    fn default() -> Self {
        Self::new()
    }
}
impl OffsetCurve {
    /// `Adaptor2d_OffsetCurve()`: no curve, offset 0.
    pub fn new() -> Self {
        Self { curve: None, offset: 0.0, first: 0.0, last: 0.0 }
    }
    /// `Adaptor2d_OffsetCurve(C)`: offset 0 and the parameter range 0, 0 (as C++).
    pub fn with_curve(c: Arc<dyn Curve2dAdaptor>) -> Self {
        Self { curve: Some(c), offset: 0.0, first: 0.0, last: 0.0 }
    }
    /// `Adaptor2d_OffsetCurve(C, Offset)`: the range of the curve.
    pub fn with_offset(c: Arc<dyn Curve2dAdaptor>, offset: f64) -> Result<Self> {
        let first = c.first_parameter()?;
        let last = c.last_parameter()?;
        Ok(Self { curve: Some(c), offset, first, last })
    }
    /// `Adaptor2d_OffsetCurve(C, Offset, WFirst, WLast)`.
    pub fn with_range(c: Arc<dyn Curve2dAdaptor>, offset: f64, w_first: f64, w_last: f64) -> Self {
        Self { curve: Some(c), offset, first: w_first, last: w_last }
    }
    /// `Load(S)`: changes the curve and resets the offset to 0 (the range is kept).
    pub fn load_curve(&mut self, c: Arc<dyn Curve2dAdaptor>) {
        self.curve = Some(c);
        self.offset = 0.0;
    }
    /// `Load(Offset)`: changes the offset and takes the range of the curve.
    pub fn load_offset(&mut self, offset: f64) -> Result<()> {
        self.offset = offset;
        let c = self.cur()?.clone();
        self.first = c.first_parameter()?;
        self.last = c.last_parameter()?;
        Ok(())
    }
    /// `Load(Offset, WFirst, WLast)`.
    pub fn load_range(&mut self, offset: f64, w_first: f64, w_last: f64) {
        self.offset = offset;
        self.first = w_first;
        self.last = w_last;
    }
    /// `ShallowCopy` with the concrete type: the adapted curve is itself shallow-copied.
    pub fn shallow_copied(&self) -> Result<OffsetCurve> {
        let mut copy = OffsetCurve::new();
        if let Some(c) = &self.curve {
            copy.curve = Some(c.shallow_copy()?);
        }
        copy.offset = self.offset;
        copy.first = self.first;
        copy.last = self.last;
        Ok(copy)
    }
    /// `Trim` with the concrete type (`new Adaptor2d_OffsetCurve(*this)` then `Load(myOffset, First, Last)`).
    pub fn trimmed(&self, first: f64, last: f64) -> OffsetCurve {
        let mut ho = self.clone();
        ho.load_range(self.offset, first, last);
        ho
    }
    /// `Curve`.
    pub fn curve(&self) -> Option<&Arc<dyn Curve2dAdaptor>> {
        self.curve.as_ref()
    }
    /// `Offset`.
    pub fn offset(&self) -> f64 {
        self.offset
    }
    fn cur(&self) -> Result<&Arc<dyn Curve2dAdaptor>> {
        self.curve.as_ref().ok_or(AdaptorError::NullObject)
    }
    /// The continuity order asked of the adapted curve for the continuity `s` of the offset curve.
    fn base_shape(s: GeomAbsShape) -> GeomAbsShape {
        if (s as i32) >= (GeomAbsShape::C2 as i32) { GeomAbsShape::Cn } else { shape_from_i32(s as i32 + 2) }
    }
    /// `NbIntervals` and the breaks of the adapted curve (`T(1, nbInter + 1)`): the indices `first`, `last` of the window.
    fn window(&self, sh: GeomAbsShape) -> Result<(i32, Vec<f64>, i32, i32)> {
        let c = self.cur()?;
        let nb_inter = c.nb_intervals(sh)?;
        if nb_inter == 1 {
            return Ok((nb_inter, Vec::new(), 0, 0));
        }
        let mut t = vec![0.0; usize::try_from(nb_inter + 1).map_err(|_| AdaptorError::OutOfRange)?];
        c.intervals(&mut t, sh)?;
        let at = |t: &[f64], j: i32| -> Result<f64> {
            usize::try_from(j - 1).ok().and_then(|k| t.get(k)).copied().ok_or(AdaptorError::OutOfRange)
        };
        let mut first = 1;
        while at(&t, first)? <= self.first {
            first += 1;
        }
        let mut last = nb_inter + 1;
        while at(&t, last)? >= self.last {
            last -= 1;
        }
        Ok((nb_inter, t, first, last))
    }
}
impl Curve2dAdaptor for OffsetCurve {
    fn shallow_copy(&self) -> Result<Arc<dyn Curve2dAdaptor>> {
        Ok(Arc::new(self.shallow_copied()?))
    }
    fn first_parameter(&self) -> Result<f64> {
        Ok(self.first)
    }
    fn last_parameter(&self) -> Result<f64> {
        Ok(self.last)
    }
    fn continuity(&self) -> Result<GeomAbsShape> {
        Ok(match self.cur()?.continuity()? {
            GeomAbsShape::Cn => GeomAbsShape::Cn,
            GeomAbsShape::C3 => GeomAbsShape::C2,
            GeomAbsShape::C2 => GeomAbsShape::G2,
            GeomAbsShape::G2 => GeomAbsShape::C1,
            GeomAbsShape::C1 => GeomAbsShape::G1,
            GeomAbsShape::G1 => GeomAbsShape::C0,
            GeomAbsShape::C0 => return Err(AdaptorError::TypeMismatch),
        })
    }
    fn nb_intervals(&self, s: GeomAbsShape) -> Result<i32> {
        let (nb_inter, _t, first, last) = self.window(Self::base_shape(s))?;
        if nb_inter == 1 {
            return Ok(nb_inter);
        }
        Ok(last - first + 2)
    }
    fn intervals(&self, ti: &mut Vec<f64>, s: GeomAbsShape) -> Result<()> {
        let (nb_inter, t, first, last) = self.window(Self::base_shape(s))?;
        if nb_inter == 1 {
            put(ti, 0, self.first)?;
            return put(ti, 1, self.last);
        }
        for (i, j) in ((first - 1)..=(last + 1)).enumerate() {
            let v = usize::try_from(j - 1).ok().and_then(|k| t.get(k)).copied().ok_or(AdaptorError::OutOfRange)?;
            put(ti, i, v)?;
        }
        put(ti, 0, self.first)?;
        put(ti, usize::try_from(last - first + 2).map_err(|_| AdaptorError::OutOfRange)?, self.last)
    }
    fn trim(&self, first: f64, last: f64, _tol: f64) -> Result<Arc<dyn Curve2dAdaptor>> {
        Ok(Arc::new(self.trimmed(first, last)))
    }
    fn is_closed(&self) -> Result<bool> {
        let c = self.cur()?;
        if self.offset == 0.0 {
            return c.is_closed();
        }
        if c.continuity()? == GeomAbsShape::C0 {
            return Ok(false);
        }
        if c.is_closed()? {
            let (_, d0) = c.d1(c.first_parameter()?)?;
            let (_, d1) = c.d1(c.last_parameter()?)?;
            let angular = crate::geom2d::angular();
            Ok(d0.is_parallel(&d1, angular)? && !d0.is_opposite(&d1, angular)?)
        } else {
            Ok(false)
        }
    }
    fn is_periodic(&self) -> Result<bool> {
        self.cur()?.is_periodic()
    }
    fn period(&self) -> Result<f64> {
        self.cur()?.period()
    }
    fn value(&self, u: f64) -> Result<Pnt2d> {
        let c = self.cur()?;
        if self.offset != 0.0 {
            let (mut p, v) = c.d1(u)?;
            crate::geom2d::offset::calculate_d0(&mut p, &v, self.offset);
            Ok(p)
        } else {
            c.value(u)
        }
    }
    fn d0(&self, u: f64) -> Result<Pnt2d> {
        self.value(u)
    }
    fn d1(&self, u: f64) -> Result<(Pnt2d, Vec2d)> {
        let c = self.cur()?;
        if self.offset != 0.0 {
            let (mut p, mut v, v2) = c.d2(u)?;
            crate::geom2d::offset::calculate_d1(&mut p, &mut v, &v2, self.offset);
            Ok((p, v))
        } else {
            c.d1(u)
        }
    }
    fn d2(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d)> {
        let c = self.cur()?;
        if self.offset != 0.0 {
            let (mut p, mut v1, mut v2, v3) = c.d3(u)?;
            crate::geom2d::offset::calculate_d2(&mut p, &mut v1, &mut v2, &v3, false, self.offset);
            Ok((p, v1, v2))
        } else {
            c.d2(u)
        }
    }
    fn d3(&self, u: f64) -> Result<(Pnt2d, Vec2d, Vec2d, Vec2d)> {
        let c = self.cur()?;
        if self.offset != 0.0 {
            let v4 = c.dn(u, 4)?;
            let (mut p, mut v1, mut v2, mut v3) = c.d3(u)?;
            crate::geom2d::offset::calculate_d3(&mut p, &mut v1, &mut v2, &mut v3, &v4, false, self.offset);
            Ok((p, v1, v2, v3))
        } else {
            c.d3(u)
        }
    }
    fn dn(&self, _u: f64, _n: i32) -> Result<Vec2d> {
        Err(AdaptorError::NotImplemented)
    }
    fn resolution(&self, r3d: f64) -> Result<f64> {
        Ok(confusion() / r3d)
    }
    fn get_type(&self) -> Result<GeomAbsCurveType> {
        let c = self.cur()?;
        if self.offset == 0.0 {
            c.get_type()
        } else {
            Ok(match c.get_type()? {
                GeomAbsCurveType::Line => GeomAbsCurveType::Line,
                GeomAbsCurveType::Circle => GeomAbsCurveType::Circle,
                _ => GeomAbsCurveType::OffsetCurve,
            })
        }
    }
    fn line(&self) -> Result<Lin2d> {
        if self.get_type()? == GeomAbsCurveType::Line {
            let (p, v) = self.d1(0.0)?;
            Ok(Lin2d::new(p, Dir2d::from_vec(&v)?))
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }
    fn circle(&self) -> Result<Circ2d> {
        if self.get_type()? != GeomAbsCurveType::Circle {
            return Err(AdaptorError::NoSuchObject);
        }
        let c = self.cur()?;
        if self.offset == 0.0 {
            return c.circle();
        }
        let c1 = c.circle()?;
        let mut radius = c1.radius();
        let mut axes = *c1.axis();
        let (xd, yd) = (*axes.x_direction(), *axes.y_direction());
        let crossed = xd.x() * yd.y() - xd.y() * yd.x();
        let signe = if crossed > 0.0 { 1.0 } else { -1.0 };
        radius += signe * self.offset;
        if radius > 0.0 {
            Ok(Circ2d::from_ax22d(axes, radius)?)
        } else if radius < 0.0 {
            radius = -radius;
            axes.set_x_direction(axes.x_direction().reversed());
            Ok(Circ2d::from_ax22d(axes, radius)?)
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }
    fn ellipse(&self) -> Result<Elips2d> {
        let c = self.cur()?;
        if c.get_type()? == GeomAbsCurveType::Ellipse && self.offset == 0.0 {
            c.ellipse()
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }
    fn hyperbola(&self) -> Result<Hypr2d> {
        let c = self.cur()?;
        if c.get_type()? == GeomAbsCurveType::Hyperbola && self.offset == 0.0 {
            c.hyperbola()
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }
    fn parabola(&self) -> Result<Parab2d> {
        let c = self.cur()?;
        if c.get_type()? == GeomAbsCurveType::Parabola && self.offset == 0.0 {
            c.parabola()
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }
    fn degree(&self) -> Result<i32> {
        let c = self.cur()?;
        let ty = c.get_type()?;
        if (ty == GeomAbsCurveType::BezierCurve || ty == GeomAbsCurveType::BSplineCurve) && self.offset == 0.0 {
            c.degree()
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }
    fn is_rational(&self) -> Result<bool> {
        if self.offset == 0.0 {
            return self.cur()?.is_rational();
        }
        Ok(false)
    }
    fn nb_poles(&self) -> Result<i32> {
        let c = self.cur()?;
        let ty = c.get_type()?;
        if (ty == GeomAbsCurveType::BezierCurve || ty == GeomAbsCurveType::BSplineCurve) && self.offset == 0.0 {
            c.nb_poles()
        } else {
            Err(AdaptorError::NoSuchObject)
        }
    }
    fn nb_knots(&self) -> Result<i32> {
        if self.offset == 0.0 { self.cur()?.nb_knots() } else { Err(AdaptorError::NoSuchObject) }
    }
    fn bezier(&self) -> Result<Option<Arc<BezierCurve>>> {
        // C++ guards this with `Standard_NoSuchObject_Raise_if(myOffset != 0 || GetType() != BezierCurve)`, which the OCCT
        // build compiles out (the condition is not even evaluated): the call is simply forwarded.
        self.cur()?.bezier()
    }
    fn bspline(&self) -> Result<Option<Arc<BSplineCurve>>> {
        // C++ guards this with `Standard_NoSuchObject_Raise_if(myOffset != 0 || GetType() != BSplineCurve)`, which the OCCT
        // build compiles out (the condition is not even evaluated): the call is simply forwarded.
        self.cur()?.bspline()
    }
    fn nb_samples(&self) -> Result<i32> {
        let c = self.cur()?;
        let mut nbs = 20;
        if c.get_type()? == GeomAbsCurveType::BezierCurve {
            nbs = std::cmp::max(nbs, 3 + c.nb_poles()?);
        } else if c.get_type()? == GeomAbsCurveType::BSplineCurve {
            nbs = std::cmp::max(nbs, c.nb_knots()? * c.degree()?);
        }
        if nbs > 300 {
            nbs = 300;
        }
        Ok(nbs)
    }
}
