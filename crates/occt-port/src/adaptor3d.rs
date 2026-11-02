//! OCCT's package `Adaptor3d`: the abstract adaptors `Adaptor3d_Curve` and `Adaptor3d_Surface`, the iso curve of a surface
//! `Adaptor3d_IsoCurve`, the curve on a surface `Adaptor3d_CurveOnSurface`, the topological tool `Adaptor3d_TopolTool`, the
//! vertex `Adaptor3d_HVertex`, the surface tool `Adaptor3d_HSurfaceTool` and the function `Adaptor3d_InterFunc`.
//!
//! Translated from `Adaptor3d_*.hxx/.cxx` (OCCT 8.0.1). The C++ virtual classes become the traits [`Curve3dAdaptor`] and
//! [`SurfaceAdaptor`] whose default methods are the base-class bodies (`Standard_NotImplemented`, the `D0 .. DN` wrappers of the
//! `Eval*` methods); adaptor handles are `Arc<dyn ...>`, and a null handle returned by `Bezier()`, `BSpline()`, `BasisCurve()`,
//! ... is `None`. See `docs/api-adaptor3d.md`.
use crate::adaptor2d::AdaptorError;
pub use crate::adaptor2d::{GeomAbsCurveType, Result};
use crate::geom::{self, ResD1, ResD2, ResD3, SurfResD1, SurfResD2, SurfResD3};
use crate::gp::{Ax1, Circ, Cone, Cylinder, Dir, Elips, Hypr, Lin, Parab, Pln, Pnt, Sphere, Torus, Vec};
pub use crate::plib::GeomAbsShape;
use std::sync::Arc;

mod curve_on_surface;
pub mod hsurface_tool;
mod hvertex;
mod inter_func;
mod iso_curve;
mod topol_tool;

pub use curve_on_surface::CurveOnSurface;
pub use hvertex::HVertex;
pub use inter_func::InterFunc;
pub use iso_curve::IsoCurve;
pub use topol_tool::TopolTool;

/// `GeomAbs_SurfaceType`; the discriminants are the C++ enumerator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum GeomAbsSurfaceType {
    /// `GeomAbs_Plane`.
    Plane = 0,
    /// `GeomAbs_Cylinder`.
    Cylinder = 1,
    /// `GeomAbs_Cone`.
    Cone = 2,
    /// `GeomAbs_Sphere`.
    Sphere = 3,
    /// `GeomAbs_Torus`.
    Torus = 4,
    /// `GeomAbs_BezierSurface`.
    BezierSurface = 5,
    /// `GeomAbs_BSplineSurface`.
    BSplineSurface = 6,
    /// `GeomAbs_SurfaceOfRevolution`.
    SurfaceOfRevolution = 7,
    /// `GeomAbs_SurfaceOfExtrusion`.
    SurfaceOfExtrusion = 8,
    /// `GeomAbs_OffsetSurface`.
    OffsetSurface = 9,
    /// `GeomAbs_OtherSurface`.
    OtherSurface = 10,
}

/// `GeomAbs_IsoType`; the discriminants are the C++ enumerator values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum GeomAbsIsoType {
    /// `GeomAbs_IsoU`.
    IsoU = 0,
    /// `GeomAbs_IsoV`.
    IsoV = 1,
    /// `GeomAbs_NoneIso`.
    NoneIso = 2,
}

fn not_implemented<T>() -> Result<T> {
    Err(AdaptorError::NotImplemented)
}

/// `Adaptor3d_Curve`. Every method is const in C++ and so takes `&self`; the default bodies are the base class's
/// (`Standard_NotImplemented`).
pub trait Curve3dAdaptor: std::fmt::Debug + Send + Sync {
    /// `ShallowCopy`.
    fn shallow_copy(&self) -> Result<Arc<dyn Curve3dAdaptor>> {
        not_implemented()
    }
    /// `FirstParameter`.
    fn first_parameter(&self) -> Result<f64> {
        not_implemented()
    }
    /// `LastParameter`.
    fn last_parameter(&self) -> Result<f64> {
        not_implemented()
    }
    /// `Continuity`.
    fn continuity(&self) -> Result<GeomAbsShape> {
        not_implemented()
    }
    /// `NbIntervals`.
    fn nb_intervals(&self, _s: GeomAbsShape) -> Result<i32> {
        not_implemented()
    }
    /// `Intervals`: `t[0]` is `T(T.Lower())`; writing past the end of `t` is `Err(OutOfRange)`.
    fn intervals(&self, _t: &mut [f64], _s: GeomAbsShape) -> Result<()> {
        not_implemented()
    }
    /// `Trim`.
    fn trim(&self, _first: f64, _last: f64, _tol: f64) -> Result<Arc<dyn Curve3dAdaptor>> {
        not_implemented()
    }
    /// `IsClosed`.
    fn is_closed(&self) -> Result<bool> {
        not_implemented()
    }
    /// `IsPeriodic`.
    fn is_periodic(&self) -> Result<bool> {
        not_implemented()
    }
    /// `Period`.
    fn period(&self) -> Result<f64> {
        not_implemented()
    }
    /// `Value`: [`eval_d0`](Self::eval_d0).
    fn value(&self, u: f64) -> Result<Pnt> {
        self.eval_d0(u)
    }
    /// `D0`: [`eval_d0`](Self::eval_d0).
    fn d0(&self, u: f64) -> Result<Pnt> {
        self.eval_d0(u)
    }
    /// `D1`.
    fn d1(&self, u: f64) -> Result<(Pnt, Vec)> {
        let r = self.eval_d1(u)?;
        Ok((r.point, r.d1))
    }
    /// `D2`.
    fn d2(&self, u: f64) -> Result<(Pnt, Vec, Vec)> {
        let r = self.eval_d2(u)?;
        Ok((r.point, r.d1, r.d2))
    }
    /// `D3`.
    fn d3(&self, u: f64) -> Result<(Pnt, Vec, Vec, Vec)> {
        let r = self.eval_d3(u)?;
        Ok((r.point, r.d1, r.d2, r.d3))
    }
    /// `DN`: [`eval_dn`](Self::eval_dn).
    fn dn(&self, u: f64, n: i32) -> Result<Vec> {
        self.eval_dn(u, n)
    }
    /// `Resolution`.
    fn resolution(&self, _r3d: f64) -> Result<f64> {
        not_implemented()
    }
    /// `GetType`.
    fn get_type(&self) -> Result<GeomAbsCurveType> {
        not_implemented()
    }
    /// `Line`.
    fn line(&self) -> Result<Lin> {
        not_implemented()
    }
    /// `Circle`.
    fn circle(&self) -> Result<Circ> {
        not_implemented()
    }
    /// `Ellipse`.
    fn ellipse(&self) -> Result<Elips> {
        not_implemented()
    }
    /// `Hyperbola`.
    fn hyperbola(&self) -> Result<Hypr> {
        not_implemented()
    }
    /// `Parabola`.
    fn parabola(&self) -> Result<Parab> {
        not_implemented()
    }
    /// `Degree`.
    fn degree(&self) -> Result<i32> {
        not_implemented()
    }
    /// `IsRational`.
    fn is_rational(&self) -> Result<bool> {
        not_implemented()
    }
    /// `NbPoles`.
    fn nb_poles(&self) -> Result<i32> {
        not_implemented()
    }
    /// `NbKnots`.
    fn nb_knots(&self) -> Result<i32> {
        not_implemented()
    }
    /// `Bezier` (`None`: a null handle).
    fn bezier(&self) -> Result<Option<Arc<geom::BezierCurve>>> {
        not_implemented()
    }
    /// `BSpline` (`None`: a null handle).
    fn bspline(&self) -> Result<Option<Arc<geom::BSplineCurve>>> {
        not_implemented()
    }
    /// `OffsetCurve` (`None`: a null handle).
    fn offset_curve(&self) -> Result<Option<Arc<geom::OffsetCurve>>> {
        not_implemented()
    }
    /// `EvalD0`.
    fn eval_d0(&self, _u: f64) -> Result<Pnt> {
        not_implemented()
    }
    /// `EvalD1`.
    fn eval_d1(&self, _u: f64) -> Result<ResD1> {
        not_implemented()
    }
    /// `EvalD2`.
    fn eval_d2(&self, _u: f64) -> Result<ResD2> {
        not_implemented()
    }
    /// `EvalD3`.
    fn eval_d3(&self, _u: f64) -> Result<ResD3> {
        not_implemented()
    }
    /// `EvalDN`.
    fn eval_dn(&self, _u: f64, _n: i32) -> Result<Vec> {
        not_implemented()
    }
}

/// `Adaptor3d_Surface`. Every method is const in C++ and so takes `&self`; the default bodies are the base class's.
pub trait SurfaceAdaptor: std::fmt::Debug + Send + Sync {
    /// `ShallowCopy`.
    fn shallow_copy(&self) -> Result<Arc<dyn SurfaceAdaptor>> {
        not_implemented()
    }
    /// `FirstUParameter`.
    fn first_u_parameter(&self) -> Result<f64> {
        not_implemented()
    }
    /// `LastUParameter`.
    fn last_u_parameter(&self) -> Result<f64> {
        not_implemented()
    }
    /// `FirstVParameter`.
    fn first_v_parameter(&self) -> Result<f64> {
        not_implemented()
    }
    /// `LastVParameter`.
    fn last_v_parameter(&self) -> Result<f64> {
        not_implemented()
    }
    /// `UContinuity`.
    fn u_continuity(&self) -> Result<GeomAbsShape> {
        not_implemented()
    }
    /// `VContinuity`.
    fn v_continuity(&self) -> Result<GeomAbsShape> {
        not_implemented()
    }
    /// `NbUIntervals`.
    fn nb_u_intervals(&self, _s: GeomAbsShape) -> Result<i32> {
        not_implemented()
    }
    /// `NbVIntervals`.
    fn nb_v_intervals(&self, _s: GeomAbsShape) -> Result<i32> {
        not_implemented()
    }
    /// `UIntervals`: `t[0]` is `T(T.Lower())`.
    fn u_intervals(&self, _t: &mut [f64], _s: GeomAbsShape) -> Result<()> {
        not_implemented()
    }
    /// `VIntervals`.
    fn v_intervals(&self, _t: &mut [f64], _s: GeomAbsShape) -> Result<()> {
        not_implemented()
    }
    /// `UTrim`.
    fn u_trim(&self, _first: f64, _last: f64, _tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        not_implemented()
    }
    /// `VTrim`.
    fn v_trim(&self, _first: f64, _last: f64, _tol: f64) -> Result<Arc<dyn SurfaceAdaptor>> {
        not_implemented()
    }
    /// `IsUClosed`.
    fn is_u_closed(&self) -> Result<bool> {
        not_implemented()
    }
    /// `IsVClosed`.
    fn is_v_closed(&self) -> Result<bool> {
        not_implemented()
    }
    /// `IsUPeriodic`.
    fn is_u_periodic(&self) -> Result<bool> {
        not_implemented()
    }
    /// `UPeriod`.
    fn u_period(&self) -> Result<f64> {
        not_implemented()
    }
    /// `IsVPeriodic`.
    fn is_v_periodic(&self) -> Result<bool> {
        not_implemented()
    }
    /// `VPeriod`.
    fn v_period(&self) -> Result<f64> {
        not_implemented()
    }
    /// `Value`: [`eval_d0`](Self::eval_d0).
    fn value(&self, u: f64, v: f64) -> Result<Pnt> {
        self.eval_d0(u, v)
    }
    /// `D0`: [`eval_d0`](Self::eval_d0).
    fn d0(&self, u: f64, v: f64) -> Result<Pnt> {
        self.eval_d0(u, v)
    }
    /// `D1`: [`eval_d1`](Self::eval_d1).
    fn d1(&self, u: f64, v: f64) -> Result<SurfResD1> {
        self.eval_d1(u, v)
    }
    /// `D2`: [`eval_d2`](Self::eval_d2).
    fn d2(&self, u: f64, v: f64) -> Result<SurfResD2> {
        self.eval_d2(u, v)
    }
    /// `D3`: [`eval_d3`](Self::eval_d3).
    fn d3(&self, u: f64, v: f64) -> Result<SurfResD3> {
        self.eval_d3(u, v)
    }
    /// `DN`: [`eval_dn`](Self::eval_dn).
    fn dn(&self, u: f64, v: f64, nu: i32, nv: i32) -> Result<Vec> {
        self.eval_dn(u, v, nu, nv)
    }
    /// `UResolution`.
    fn u_resolution(&self, _r3d: f64) -> Result<f64> {
        not_implemented()
    }
    /// `VResolution`.
    fn v_resolution(&self, _r3d: f64) -> Result<f64> {
        not_implemented()
    }
    /// `GetType`.
    fn get_type(&self) -> Result<GeomAbsSurfaceType> {
        not_implemented()
    }
    /// `Plane`.
    fn plane(&self) -> Result<Pln> {
        not_implemented()
    }
    /// `Cylinder`.
    fn cylinder(&self) -> Result<Cylinder> {
        not_implemented()
    }
    /// `Cone`.
    fn cone(&self) -> Result<Cone> {
        not_implemented()
    }
    /// `Sphere`.
    fn sphere(&self) -> Result<Sphere> {
        not_implemented()
    }
    /// `Torus`.
    fn torus(&self) -> Result<Torus> {
        not_implemented()
    }
    /// `UDegree`.
    fn u_degree(&self) -> Result<i32> {
        not_implemented()
    }
    /// `NbUPoles`.
    fn nb_u_poles(&self) -> Result<i32> {
        not_implemented()
    }
    /// `VDegree`.
    fn v_degree(&self) -> Result<i32> {
        not_implemented()
    }
    /// `NbVPoles`.
    fn nb_v_poles(&self) -> Result<i32> {
        not_implemented()
    }
    /// `NbUKnots`.
    fn nb_u_knots(&self) -> Result<i32> {
        not_implemented()
    }
    /// `NbVKnots`.
    fn nb_v_knots(&self) -> Result<i32> {
        not_implemented()
    }
    /// `IsURational`.
    fn is_u_rational(&self) -> Result<bool> {
        not_implemented()
    }
    /// `IsVRational`.
    fn is_v_rational(&self) -> Result<bool> {
        not_implemented()
    }
    /// `Bezier` (`None`: a null handle).
    fn bezier(&self) -> Result<Option<Arc<geom::BezierSurface>>> {
        not_implemented()
    }
    /// `BSpline` (`None`: a null handle).
    fn bspline(&self) -> Result<Option<Arc<geom::BSplineSurface>>> {
        not_implemented()
    }
    /// `AxeOfRevolution`.
    fn axe_of_revolution(&self) -> Result<Ax1> {
        not_implemented()
    }
    /// `Direction`.
    fn direction(&self) -> Result<Dir> {
        not_implemented()
    }
    /// `BasisCurve` (`None`: a null handle).
    fn basis_curve(&self) -> Result<Option<Arc<dyn Curve3dAdaptor>>> {
        not_implemented()
    }
    /// `BasisSurface` (`None`: a null handle).
    fn basis_surface(&self) -> Result<Option<Arc<dyn SurfaceAdaptor>>> {
        not_implemented()
    }
    /// `OffsetValue`.
    fn offset_value(&self) -> Result<f64> {
        not_implemented()
    }
    /// `EvalD0`.
    fn eval_d0(&self, _u: f64, _v: f64) -> Result<Pnt> {
        not_implemented()
    }
    /// `EvalD1`.
    fn eval_d1(&self, _u: f64, _v: f64) -> Result<SurfResD1> {
        not_implemented()
    }
    /// `EvalD2`.
    fn eval_d2(&self, _u: f64, _v: f64) -> Result<SurfResD2> {
        not_implemented()
    }
    /// `EvalD3`.
    fn eval_d3(&self, _u: f64, _v: f64) -> Result<SurfResD3> {
        not_implemented()
    }
    /// `EvalDN`.
    fn eval_dn(&self, _u: f64, _v: f64, _nu: i32, _nv: i32) -> Result<Vec> {
        not_implemented()
    }
}

fn put(t: &mut [f64], k: usize, v: f64) -> Result<()> {
    *t.get_mut(k).ok_or(AdaptorError::OutOfRange)? = v;
    Ok(())
}
