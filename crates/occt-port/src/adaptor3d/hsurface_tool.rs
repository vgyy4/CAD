//! `Adaptor3d_HSurfaceTool`: the static methods that forward to an `Adaptor3d_Surface`, with the sample counts and the
//! G1 test of a surface.
//!
//! Translated from `Adaptor3d_HSurfaceTool.hxx/.cxx` (OCCT 8.0.1).
use super::{Curve3dAdaptor, GeomAbsCurveType, GeomAbsShape, GeomAbsSurfaceType, Result, SurfaceAdaptor};
use crate::adaptor2d::AdaptorError;
use crate::geom::{self, SurfResD1, SurfResD2, SurfResD3, Surface};
use crate::gp::{Ax1, Cone, Cylinder, Dir, Pln, Pnt, Sphere, Torus, Vec};
use std::sync::Arc;

type Surf = Arc<dyn SurfaceAdaptor>;

/// `FirstUParameter`.
pub fn first_u_parameter(s: &Surf) -> Result<f64> {
    s.first_u_parameter()
}
/// `FirstVParameter`.
pub fn first_v_parameter(s: &Surf) -> Result<f64> {
    s.first_v_parameter()
}
/// `LastUParameter`.
pub fn last_u_parameter(s: &Surf) -> Result<f64> {
    s.last_u_parameter()
}
/// `LastVParameter`.
pub fn last_v_parameter(s: &Surf) -> Result<f64> {
    s.last_v_parameter()
}
/// `NbUIntervals`.
pub fn nb_u_intervals(s: &Surf, sh: GeomAbsShape) -> Result<i32> {
    s.nb_u_intervals(sh)
}
/// `NbVIntervals`.
pub fn nb_v_intervals(s: &Surf, sh: GeomAbsShape) -> Result<i32> {
    s.nb_v_intervals(sh)
}
/// `UIntervals`.
pub fn u_intervals(s: &Surf, t: &mut [f64], sh: GeomAbsShape) -> Result<()> {
    s.u_intervals(t, sh)
}
/// `VIntervals`.
pub fn v_intervals(s: &Surf, t: &mut [f64], sh: GeomAbsShape) -> Result<()> {
    s.v_intervals(t, sh)
}
/// `UTrim`.
pub fn u_trim(s: &Surf, first: f64, last: f64, tol: f64) -> Result<Surf> {
    s.u_trim(first, last, tol)
}
/// `VTrim`.
pub fn v_trim(s: &Surf, first: f64, last: f64, tol: f64) -> Result<Surf> {
    s.v_trim(first, last, tol)
}
/// `IsUClosed`.
pub fn is_u_closed(s: &Surf) -> Result<bool> {
    s.is_u_closed()
}
/// `IsVClosed`.
pub fn is_v_closed(s: &Surf) -> Result<bool> {
    s.is_v_closed()
}
/// `IsUPeriodic`.
pub fn is_u_periodic(s: &Surf) -> Result<bool> {
    s.is_u_periodic()
}
/// `UPeriod`.
pub fn u_period(s: &Surf) -> Result<f64> {
    s.u_period()
}
/// `IsVPeriodic`.
pub fn is_v_periodic(s: &Surf) -> Result<bool> {
    s.is_v_periodic()
}
/// `VPeriod`.
pub fn v_period(s: &Surf) -> Result<f64> {
    s.v_period()
}
/// `Value`.
pub fn value(s: &Surf, u: f64, v: f64) -> Result<Pnt> {
    s.value(u, v)
}
/// `D0`.
pub fn d0(s: &Surf, u: f64, v: f64) -> Result<Pnt> {
    s.d0(u, v)
}
/// `D1`.
pub fn d1(s: &Surf, u: f64, v: f64) -> Result<SurfResD1> {
    s.d1(u, v)
}
/// `D2`.
pub fn d2(s: &Surf, u: f64, v: f64) -> Result<SurfResD2> {
    s.d2(u, v)
}
/// `D3`.
pub fn d3(s: &Surf, u: f64, v: f64) -> Result<SurfResD3> {
    s.d3(u, v)
}
/// `DN`.
pub fn dn(s: &Surf, u: f64, v: f64, nu: i32, nv: i32) -> Result<Vec> {
    s.dn(u, v, nu, nv)
}
/// `UResolution`.
pub fn u_resolution(s: &Surf, r3d: f64) -> Result<f64> {
    s.u_resolution(r3d)
}
/// `VResolution`.
pub fn v_resolution(s: &Surf, r3d: f64) -> Result<f64> {
    s.v_resolution(r3d)
}
/// `GetType`.
pub fn get_type(s: &Surf) -> Result<GeomAbsSurfaceType> {
    s.get_type()
}
/// `Plane`.
pub fn plane(s: &Surf) -> Result<Pln> {
    s.plane()
}
/// `Cylinder`.
pub fn cylinder(s: &Surf) -> Result<Cylinder> {
    s.cylinder()
}
/// `Cone`.
pub fn cone(s: &Surf) -> Result<Cone> {
    s.cone()
}
/// `Torus`.
pub fn torus(s: &Surf) -> Result<Torus> {
    s.torus()
}
/// `Sphere`.
pub fn sphere(s: &Surf) -> Result<Sphere> {
    s.sphere()
}
/// `Bezier`.
pub fn bezier(s: &Surf) -> Result<Option<Arc<geom::BezierSurface>>> {
    s.bezier()
}
/// `BSpline`.
pub fn bspline(s: &Surf) -> Result<Option<Arc<geom::BSplineSurface>>> {
    s.bspline()
}
/// `AxeOfRevolution`.
pub fn axe_of_revolution(s: &Surf) -> Result<Ax1> {
    s.axe_of_revolution()
}
/// `Direction`.
pub fn direction(s: &Surf) -> Result<Dir> {
    s.direction()
}
/// `BasisCurve`.
pub fn basis_curve(s: &Surf) -> Result<Option<Arc<dyn Curve3dAdaptor>>> {
    s.basis_curve()
}
/// `BasisSurface`.
pub fn basis_surface(s: &Surf) -> Result<Option<Arc<dyn SurfaceAdaptor>>> {
    s.basis_surface()
}
/// `OffsetValue`.
pub fn offset_value(s: &Surf) -> Result<f64> {
    s.offset_value()
}

/// `NbSamplesU(S)`.
pub fn nb_samples_u(s: &Surf) -> Result<i32> {
    Ok(match s.get_type()? {
        GeomAbsSurfaceType::Plane => 2,
        GeomAbsSurfaceType::BezierSurface => 3 + s.nb_u_poles()?,
        GeomAbsSurfaceType::BSplineSurface => {
            let nbs = s.nb_u_knots()? * s.u_degree()?;
            if nbs < 2 { 2 } else { nbs }
        }
        GeomAbsSurfaceType::Torus => 20,
        _ => 10,
    })
}

/// `NbSamplesV(S)`.
pub fn nb_samples_v(s: &Surf) -> Result<i32> {
    Ok(match s.get_type()? {
        GeomAbsSurfaceType::Plane => 2,
        GeomAbsSurfaceType::BezierSurface => 3 + s.nb_v_poles()?,
        GeomAbsSurfaceType::BSplineSurface => {
            let nbs = s.nb_v_knots()? * s.v_degree()?;
            if nbs < 2 { 2 } else { nbs }
        }
        GeomAbsSurfaceType::Cylinder
        | GeomAbsSurfaceType::Cone
        | GeomAbsSurfaceType::Sphere
        | GeomAbsSurfaceType::Torus
        | GeomAbsSurfaceType::SurfaceOfRevolution
        | GeomAbsSurfaceType::SurfaceOfExtrusion => 15,
        _ => 10,
    })
}

/// `NbSamplesU(S, u1, u2)`: the count for the part `[u1, u2]` of the U range.
pub fn nb_samples_u_range(s: &Surf, u1: f64, u2: f64) -> Result<i32> {
    let nbs = nb_samples_u(s)?;
    let mut n = nbs;
    if nbs > 10 {
        let uf = s.first_u_parameter()?;
        let ul = s.last_u_parameter()?;
        n = n.wrapping_mul(((u2 - u1) / (ul - uf)) as i32);
        if n > nbs || n > 50 {
            n = nbs;
        }
        if n < 5 {
            n = 5;
        }
    }
    Ok(n)
}

/// `NbSamplesV(S, v1, v2)`.
pub fn nb_samples_v_range(s: &Surf, v1: f64, v2: f64) -> Result<i32> {
    let nbs = nb_samples_v(s)?;
    let mut n = nbs;
    if nbs > 10 {
        let vf = s.first_v_parameter()?;
        let vl = s.last_v_parameter()?;
        n = n.wrapping_mul(((v2 - v1) / (vl - vf)) as i32);
        if n > nbs || n > 50 {
            n = nbs;
        }
        if n < 5 {
            n = 5;
        }
    }
    Ok(n)
}

/// `IsSurfG1`: whether the surface is G1 along U (`along_u`) or V: a B-spline curve of the surface (the iso curve at the
/// middle of the other direction, or the B-spline basis curve of a revolution / extrusion) is tested with `IsG1`.
pub fn is_surf_g1(surf: &Surf, along_u: bool, ang_tol: f64) -> Result<bool> {
    let u_f = surf.first_u_parameter()?;
    let u_l = surf.last_u_parameter()?;
    let v_f = surf.first_v_parameter()?;
    let v_l = surf.last_v_parameter()?;
    let mut s: Surf = surf.clone();
    let mut c: Option<BasisCurve> = None;
    let mut bc: Option<Arc<geom::BSplineCurve>> = None;
    if s.get_type()? == GeomAbsSurfaceType::OffsetSurface {
        s = s.basis_surface()?.ok_or(AdaptorError::NullObject)?;
    }
    if s.get_type()? == GeomAbsSurfaceType::SurfaceOfRevolution
        || s.get_type()? == GeomAbsSurfaceType::SurfaceOfExtrusion
    {
        c = s.basis_curve()?.map(BasisCurve::Adaptor);
    }
    if let Some(curve) = c.as_mut() {
        if curve.get_type()? == GeomAbsCurveType::OffsetCurve {
            let oc = curve.offset_curve()?.ok_or(AdaptorError::NullObject)?;
            // `new GeomAdaptor_Curve(aOC->BasisCurve())`: only the type and the B-spline of the curve are used
            *curve = BasisCurve::Geom(oc.basis_curve());
        }
        if curve.get_type()? == GeomAbsCurveType::BSplineCurve
            && ((along_u && s.get_type()? == GeomAbsSurfaceType::SurfaceOfExtrusion)
                || (!along_u && s.get_type()? == GeomAbsSurfaceType::SurfaceOfRevolution))
        {
            bc = curve.bspline()?;
        }
    }
    if s.get_type()? == GeomAbsSurfaceType::BSplineSurface {
        let bs = s.bspline()?.ok_or(AdaptorError::NullObject)?;
        let iso = if along_u {
            let iso_par = (v_f + v_l) / 2.0;
            bs.v_iso(iso_par)?
        } else {
            let iso_par = (u_f + u_l) / 2.0;
            bs.u_iso(iso_par)?
        };
        bc = iso.as_any().downcast_ref::<geom::BSplineCurve>().map(|b| Arc::new(b.clone()));
    }
    if let Some(bc) = bc {
        return Ok(if along_u { bc.is_g1(u_f, u_l, ang_tol)? } else { bc.is_g1(v_f, v_l, ang_tol)? });
    }
    Ok(false)
}

// `aC` of `IsSurfG1`: an adaptor handle or the `GeomAdaptor_Curve` made of the basis curve of an offset curve
enum BasisCurve {
    Adaptor(Arc<dyn Curve3dAdaptor>),
    Geom(Arc<dyn geom::Curve>),
}

impl BasisCurve {
    fn get_type(&self) -> Result<GeomAbsCurveType> {
        match self {
            Self::Adaptor(c) => c.get_type(),
            Self::Geom(c) => {
                let any = c.as_any();
                Ok(if any.is::<geom::Line>() {
                    GeomAbsCurveType::Line
                } else if any.is::<geom::Circle>() {
                    GeomAbsCurveType::Circle
                } else if any.is::<geom::Ellipse>() {
                    GeomAbsCurveType::Ellipse
                } else if any.is::<geom::Hyperbola>() {
                    GeomAbsCurveType::Hyperbola
                } else if any.is::<geom::Parabola>() {
                    GeomAbsCurveType::Parabola
                } else if any.is::<geom::BezierCurve>() {
                    GeomAbsCurveType::BezierCurve
                } else if any.is::<geom::BSplineCurve>() {
                    GeomAbsCurveType::BSplineCurve
                } else if any.is::<geom::OffsetCurve>() {
                    GeomAbsCurveType::OffsetCurve
                } else {
                    GeomAbsCurveType::OtherCurve
                })
            }
        }
    }

    fn offset_curve(&self) -> Result<Option<Arc<geom::OffsetCurve>>> {
        match self {
            Self::Adaptor(c) => c.offset_curve(),
            Self::Geom(c) => Ok(c.as_any().downcast_ref::<geom::OffsetCurve>().map(|o| Arc::new(o.clone()))),
        }
    }

    fn bspline(&self) -> Result<Option<Arc<geom::BSplineCurve>>> {
        match self {
            Self::Adaptor(c) => c.bspline(),
            Self::Geom(c) => Ok(c.as_any().downcast_ref::<geom::BSplineCurve>().map(|b| Arc::new(b.clone()))),
        }
    }
}
