//! `GeomAdaptor`: adaptors over `Geom` curves and surfaces (`GeomAdaptor_Curve`, `GeomAdaptor_Surface`, the transformed adaptors,
//! the surfaces of revolution and of linear extrusion of an adaptor curve) and `GeomAdaptor::MakeCurve` / `MakeSurface`.
//!
//! Translated from OCCT 8.0.1 (`src/ModelingData/TKG3d/GeomAdaptor`). Errors are [`AdaptorError`](crate::adaptor2d::AdaptorError).
#![allow(clippy::float_cmp)]
mod curve;
mod extrusion;
mod revolution;
mod surface;
mod transformed_curve;
mod transformed_surface;
mod utils;

pub use curve::Curve;
pub use extrusion::SurfaceOfLinearExtrusion;
pub use revolution::SurfaceOfRevolution;
pub use surface::Surface;
pub use transformed_curve::TransformedCurve;
pub use transformed_surface::TransformedSurface;

use crate::adaptor2d::{AdaptorError, GeomAbsCurveType};
use crate::adaptor3d::{Curve3dAdaptor, GeomAbsSurfaceType, SurfaceAdaptor};
use crate::geom::{
    self, Circle, ConicalSurface, CylindricalSurface, Ellipse, Hyperbola, Line, OffsetSurface, Parabola, Plane,
    RectangularTrimmedSurface, SphericalSurface, ToroidalSurface, TrimmedCurve,
};
use std::sync::Arc;

/// The result type of the adaptors.
pub type Result<T> = std::result::Result<T, AdaptorError>;

/// `GeomAdaptor::MakeCurve`.
pub fn make_curve(hc: &dyn Curve3dAdaptor) -> Result<Arc<dyn geom::Curve>> {
    let mut c: Arc<dyn geom::Curve> = match hc.get_type()? {
        GeomAbsCurveType::Line => Arc::new(Line::from_lin(hc.line()?)),
        GeomAbsCurveType::Circle => Arc::new(Circle::from_circ(hc.circle()?)),
        GeomAbsCurveType::Ellipse => Arc::new(Ellipse::from_elips(hc.ellipse()?)),
        GeomAbsCurveType::Parabola => Arc::new(Parabola::from_parab(hc.parabola()?)),
        GeomAbsCurveType::Hyperbola => Arc::new(Hyperbola::from_hypr(hc.hyperbola()?)),
        GeomAbsCurveType::BezierCurve => {
            let b = hc.bezier()?.ok_or(AdaptorError::NullObject)?;
            Arc::new((*b).clone())
        }
        GeomAbsCurveType::BSplineCurve => {
            let b = hc.bspline()?.ok_or(AdaptorError::NullObject)?;
            Arc::new((*b).clone())
        }
        _ => return Err(AdaptorError::Domain),
    };
    if hc.first_parameter()? != c.first_parameter() || hc.last_parameter()? != c.last_parameter() {
        c = Arc::new(TrimmedCurve::new(c, hc.first_parameter()?, hc.last_parameter()?, true, true)?);
    }
    Ok(c)
}

/// `GeomAdaptor::MakeSurface`.
pub fn make_surface(hs: &dyn SurfaceAdaptor, trim: bool) -> Result<Arc<dyn geom::Surface>> {
    let s: Arc<dyn geom::Surface> = match hs.get_type()? {
        GeomAbsSurfaceType::Plane => Arc::new(Plane::from_pln(hs.plane()?)),
        GeomAbsSurfaceType::Cylinder => Arc::new(CylindricalSurface::from_cylinder(hs.cylinder()?)),
        GeomAbsSurfaceType::Cone => Arc::new(ConicalSurface::from_cone(hs.cone()?)),
        GeomAbsSurfaceType::Sphere => Arc::new(SphericalSurface::from_sphere(hs.sphere()?)),
        GeomAbsSurfaceType::Torus => Arc::new(ToroidalSurface::from_torus(hs.torus()?)),
        GeomAbsSurfaceType::BezierSurface => {
            let b = hs.bezier()?.ok_or(AdaptorError::NullObject)?;
            Arc::new((*b).clone())
        }
        GeomAbsSurfaceType::BSplineSurface => {
            let b = hs.bspline()?.ok_or(AdaptorError::NullObject)?;
            Arc::new((*b).clone())
        }
        GeomAbsSurfaceType::SurfaceOfRevolution => {
            let bc = hs.basis_curve()?.ok_or(AdaptorError::NullObject)?;
            let axis = hs.axe_of_revolution()?;
            Arc::new(geom::SurfaceOfRevolution::new(make_curve(bc.as_ref())?, axis)?)
        }
        GeomAbsSurfaceType::SurfaceOfExtrusion => {
            let bc = hs.basis_curve()?.ok_or(AdaptorError::NullObject)?;
            let dir = hs.direction()?;
            Arc::new(geom::SurfaceOfLinearExtrusion::new(make_curve(bc.as_ref())?, dir)?)
        }
        GeomAbsSurfaceType::OffsetSurface => {
            let bs = hs.basis_surface()?.ok_or(AdaptorError::NullObject)?;
            let basis = make_surface(bs.as_ref(), true)?;
            Arc::new(OffsetSurface::new(basis, hs.offset_value()?, false)?)
        }
        GeomAbsSurfaceType::OtherSurface => return Err(AdaptorError::Domain),
    };
    if !trim {
        return Ok(s);
    }
    let (u1, u2, v1, v2) = s.bounds();
    if hs.first_u_parameter()? != u1
        || hs.last_u_parameter()? != u2
        || hs.first_v_parameter()? != v1
        || hs.last_v_parameter()? != v2
    {
        return Ok(Arc::new(RectangularTrimmedSurface::new(
            s,
            hs.first_u_parameter()?,
            hs.last_u_parameter()?,
            hs.first_v_parameter()?,
            hs.last_v_parameter()?,
            true,
            true,
        )?));
    }
    Ok(s)
}
