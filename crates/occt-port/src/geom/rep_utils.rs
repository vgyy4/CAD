//! `GeomEval_RepCurveDesc`, `GeomEval_RepSurfaceDesc` and `GeomEval_RepUtils`: evaluation representations of the Bezier,
//! B-spline and offset curves and of the Bezier, B-spline, extrusion, revolution and offset surfaces.
use super::{Curve, GeomError, ResD1, ResD2, ResD3, Result, SurfResD1, SurfResD2, SurfResD3, Surface, confusion};
use crate::gp;
use crate::gp::Pnt;
use std::sync::Arc;

/// The kind of an evaluation representation (`GeomEval_RepCurveDesc::Base::Kind`).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalRepKind {
    /// `Full`: fully equivalent, no derivative limit, no domain, no map.
    Full = 0,
    /// `DerivBounded`: full domain and identity map, limited to `max_deriv_order`.
    DerivBounded = 1,
    /// `Mapped`: `max_deriv_order`, an optional domain and an affine parameter map.
    Mapped = 2,
}

/// `GeomEval_RepCurveDesc::Base` with its three kinds (`Full`, `DerivBounded`, `Mapped`).
#[derive(Debug, Clone)]
pub struct EvalRepDesc {
    /// The kind.
    pub kind: EvalRepKind,
    /// The curve used for the evaluation (`Representation`).
    pub representation: Option<Arc<dyn Curve>>,
    /// `MaxDerivOrder` (`DerivBounded` and `Mapped`).
    pub max_deriv_order: usize,
    /// `Domain1d { First, Last }` (`Mapped` only; `None` is the whole domain).
    pub domain: Option<(f64, f64)>,
    /// `Map1d::Scale` (`Mapped` only; 1.0 otherwise).
    pub scale: f64,
    /// `Map1d::Offset` (`Mapped` only; 0.0 otherwise).
    pub offset: f64,
}

// Map1d::IsValid
fn map_is_valid(scale: f64) -> bool {
    scale.abs() > confusion()
}

// Map1d::Map
fn map(d: &EvalRepDesc, u: f64) -> f64 {
    d.scale * u + d.offset
}

// Domain1d::Contains
fn domain_contains(first: f64, last: f64, u: f64) -> bool {
    (u >= first - confusion()) && (u <= last + confusion())
}

/// `GeomEval_RepUtils::ValidateCurveDesc`; `owner` is the curve that carries the descriptor.
pub fn validate_curve_desc(desc: &EvalRepDesc, owner: &dyn Curve) -> Result<()> {
    let Some(rep) = desc.representation.as_ref() else {
        return Err(GeomError::Program);
    };
    if desc.kind == EvalRepKind::Mapped && !map_is_valid(desc.scale) {
        return Err(GeomError::Program);
    }
    if std::ptr::addr_eq(Arc::as_ptr(rep), std::ptr::from_ref(owner)) {
        return Err(GeomError::Program);
    }
    Ok(())
}

/// `GeomEval_RepUtils::CloneCurveDesc`: the representation curve is copied.
pub fn clone_curve_desc(desc: &Option<EvalRepDesc>) -> Option<EvalRepDesc> {
    let d = desc.as_ref()?;
    let rep = d.representation.as_ref()?;
    let rep_copy: Arc<dyn Curve> = Arc::from(rep.copy_curve());
    Some(match d.kind {
        EvalRepKind::Full => EvalRepDesc {
            kind: EvalRepKind::Full,
            representation: Some(rep_copy),
            max_deriv_order: 3,
            domain: None,
            scale: 1.0,
            offset: 0.0,
        },
        EvalRepKind::DerivBounded => EvalRepDesc {
            kind: EvalRepKind::DerivBounded,
            representation: Some(rep_copy),
            max_deriv_order: d.max_deriv_order,
            domain: None,
            scale: 1.0,
            offset: 0.0,
        },
        EvalRepKind::Mapped => EvalRepDesc {
            kind: EvalRepKind::Mapped,
            representation: Some(rep_copy),
            max_deriv_order: d.max_deriv_order,
            domain: d.domain,
            scale: d.scale,
            offset: d.offset,
        },
    })
}

// The representation of a descriptor that passed `validate_curve_desc`.
fn rep(d: &EvalRepDesc) -> Result<&Arc<dyn Curve>> {
    d.representation.as_ref().ok_or(GeomError::Program)
}

/// `GeomEval_RepUtils::TryEvalCurveD0`; `None` is the C++ `false`.
pub fn try_eval_curve_d0(desc: &Option<EvalRepDesc>, u: f64) -> Result<Option<Pnt>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full | EvalRepKind::DerivBounded => Ok(Some(rep(d)?.eval_d0(u)?)),
        EvalRepKind::Mapped => try_eval_curve_d0_mapped(d, u),
    }
}

/// `GeomEval_RepUtils::TryEvalCurveD1`.
pub fn try_eval_curve_d1(desc: &Option<EvalRepDesc>, u: f64) -> Result<Option<ResD1>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full => Ok(Some(rep(d)?.eval_d1(u)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < 1 {
                return Ok(None);
            }
            Ok(Some(rep(d)?.eval_d1(u)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < 1 {
                return Ok(None);
            }
            try_eval_curve_d1_mapped(d, u)
        }
    }
}

/// `GeomEval_RepUtils::TryEvalCurveD2`.
pub fn try_eval_curve_d2(desc: &Option<EvalRepDesc>, u: f64) -> Result<Option<ResD2>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full => Ok(Some(rep(d)?.eval_d2(u)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < 2 {
                return Ok(None);
            }
            Ok(Some(rep(d)?.eval_d2(u)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < 2 {
                return Ok(None);
            }
            try_eval_curve_d2_mapped(d, u)
        }
    }
}

/// `GeomEval_RepUtils::TryEvalCurveD3`.
pub fn try_eval_curve_d3(desc: &Option<EvalRepDesc>, u: f64) -> Result<Option<ResD3>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full => Ok(Some(rep(d)?.eval_d3(u)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < 3 {
                return Ok(None);
            }
            Ok(Some(rep(d)?.eval_d3(u)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < 3 {
                return Ok(None);
            }
            try_eval_curve_d3_mapped(d, u)
        }
    }
}

/// `GeomEval_RepUtils::TryEvalCurveDN`.
pub fn try_eval_curve_dn(desc: &Option<EvalRepDesc>, u: f64, n: i32) -> Result<Option<gp::Vec>> {
    let Some(d) = desc else { return Ok(None) };
    if n < 1 {
        return Ok(None);
    }
    match d.kind {
        EvalRepKind::Full => Ok(Some(rep(d)?.eval_dn(u, n)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < n as usize {
                return Ok(None);
            }
            Ok(Some(rep(d)?.eval_dn(u, n)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < n as usize {
                return Ok(None);
            }
            try_eval_curve_dn_mapped(d, u, n)
        }
    }
}

// The mapped parameter, or `None` when it is outside the domain.
fn mapped_param(d: &EvalRepDesc, u: f64) -> Option<f64> {
    let u_rep = map(d, u);
    if let Some((first, last)) = d.domain
        && !domain_contains(first, last, u_rep)
    {
        return None;
    }
    Some(u_rep)
}

/// `GeomEval_RepUtils::TryEvalCurveD0Mapped`.
pub fn try_eval_curve_d0_mapped(d: &EvalRepDesc, u: f64) -> Result<Option<Pnt>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    Ok(Some(rep(d)?.eval_d0(u_rep)?))
}

/// `GeomEval_RepUtils::TryEvalCurveD1Mapped`.
pub fn try_eval_curve_d1_mapped(d: &EvalRepDesc, u: f64) -> Result<Option<ResD1>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    let r = rep(d)?.eval_d1(u_rep)?;
    let scale = d.scale;
    Ok(Some(ResD1 { point: r.point, d1: r.d1.multiplied(scale) }))
}

/// `GeomEval_RepUtils::TryEvalCurveD2Mapped`.
pub fn try_eval_curve_d2_mapped(d: &EvalRepDesc, u: f64) -> Result<Option<ResD2>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    let r = rep(d)?.eval_d2(u_rep)?;
    let scale = d.scale;
    let scale2 = scale * scale;
    Ok(Some(ResD2 { point: r.point, d1: r.d1.multiplied(scale), d2: r.d2.multiplied(scale2) }))
}

/// `GeomEval_RepUtils::TryEvalCurveD3Mapped`.
pub fn try_eval_curve_d3_mapped(d: &EvalRepDesc, u: f64) -> Result<Option<ResD3>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    let r = rep(d)?.eval_d3(u_rep)?;
    let scale = d.scale;
    let scale2 = scale * scale;
    let scale3 = scale2 * scale;
    Ok(Some(ResD3 {
        point: r.point,
        d1: r.d1.multiplied(scale),
        d2: r.d2.multiplied(scale2),
        d3: r.d3.multiplied(scale3),
    }))
}

/// `GeomEval_RepUtils::TryEvalCurveDNMapped`.
pub fn try_eval_curve_dn_mapped(d: &EvalRepDesc, u: f64, n: i32) -> Result<Option<gp::Vec>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    let rep_vec = rep(d)?.eval_dn(u_rep, n)?;
    let scale_n = libm::pow(d.scale, f64::from(n));
    Ok(Some(rep_vec.multiplied(scale_n)))
}

// ---------------------------------------------------------------------------------------------
// Surfaces

/// `GeomEval_RepSurfaceDesc::Base` with its three kinds (`Full`, `DerivBounded`, `Mapped`).
#[derive(Debug, Clone)]
pub struct SurfaceEvalRepDesc {
    /// The kind.
    pub kind: EvalRepKind,
    /// The surface used for the evaluation (`Representation`).
    pub representation: Option<Arc<dyn Surface>>,
    /// `MaxDerivOrder` (`DerivBounded` and `Mapped`).
    pub max_deriv_order: usize,
    /// `Domain2d { UFirst, ULast, VFirst, VLast }` (`Mapped` only; `None` is the whole domain).
    pub domain: Option<(f64, f64, f64, f64)>,
    /// `Map2d::ScaleU`.
    pub scale_u: f64,
    /// `Map2d::OffsetU`.
    pub offset_u: f64,
    /// `Map2d::ScaleV`.
    pub scale_v: f64,
    /// `Map2d::OffsetV`.
    pub offset_v: f64,
    /// `Map2d::SwapUV`.
    pub swap_uv: bool,
}

// Map2d::Map
fn map2d(d: &SurfaceEvalRepDesc, u: f64, v: f64) -> (f64, f64) {
    if d.swap_uv {
        (d.scale_u * v + d.offset_u, d.scale_v * u + d.offset_v)
    } else {
        (d.scale_u * u + d.offset_u, d.scale_v * v + d.offset_v)
    }
}

// Domain2d::Contains
fn domain2d_contains(dom: (f64, f64, f64, f64), u: f64, v: f64) -> bool {
    let (uf, ul, vf, vl) = dom;
    (u >= uf - confusion()) && (u <= ul + confusion()) && (v >= vf - confusion()) && (v <= vl + confusion())
}

/// `GeomEval_RepUtils::ValidateSurfaceDesc`; `owner` is the surface that carries the descriptor.
pub fn validate_surface_desc(desc: &SurfaceEvalRepDesc, owner: &dyn Surface) -> Result<()> {
    let Some(rep) = desc.representation.as_ref() else {
        return Err(GeomError::Program);
    };
    // Map2d::IsValid
    if desc.kind == EvalRepKind::Mapped && !(desc.scale_u.abs() > confusion() && desc.scale_v.abs() > confusion()) {
        return Err(GeomError::Program);
    }
    if std::ptr::addr_eq(Arc::as_ptr(rep), std::ptr::from_ref(owner)) {
        return Err(GeomError::Program);
    }
    Ok(())
}

/// `GeomEval_RepUtils::CloneSurfaceDesc`: the representation surface is copied.
pub fn clone_surface_desc(desc: &Option<SurfaceEvalRepDesc>) -> Option<SurfaceEvalRepDesc> {
    let d = desc.as_ref()?;
    let rep = d.representation.as_ref()?;
    let rep_copy: Arc<dyn Surface> = Arc::from(rep.copy_surface());
    Some(match d.kind {
        EvalRepKind::Full => SurfaceEvalRepDesc {
            kind: EvalRepKind::Full,
            representation: Some(rep_copy),
            max_deriv_order: 3,
            domain: None,
            scale_u: 1.0,
            offset_u: 0.0,
            scale_v: 1.0,
            offset_v: 0.0,
            swap_uv: false,
        },
        EvalRepKind::DerivBounded => SurfaceEvalRepDesc {
            kind: EvalRepKind::DerivBounded,
            representation: Some(rep_copy),
            max_deriv_order: d.max_deriv_order,
            domain: None,
            scale_u: 1.0,
            offset_u: 0.0,
            scale_v: 1.0,
            offset_v: 0.0,
            swap_uv: false,
        },
        EvalRepKind::Mapped => SurfaceEvalRepDesc { representation: Some(rep_copy), ..d.clone() },
    })
}

fn srep(d: &SurfaceEvalRepDesc) -> Result<&Arc<dyn Surface>> {
    d.representation.as_ref().ok_or(GeomError::Program)
}

/// `GeomEval_RepUtils::TryEvalSurfaceD0`; `None` is the C++ `false`.
pub fn try_eval_surface_d0(desc: &Option<SurfaceEvalRepDesc>, u: f64, v: f64) -> Result<Option<Pnt>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full | EvalRepKind::DerivBounded => Ok(Some(srep(d)?.eval_d0(u, v)?)),
        EvalRepKind::Mapped => try_eval_surface_d0_mapped(d, u, v),
    }
}

/// `GeomEval_RepUtils::TryEvalSurfaceD1`.
pub fn try_eval_surface_d1(desc: &Option<SurfaceEvalRepDesc>, u: f64, v: f64) -> Result<Option<SurfResD1>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full => Ok(Some(srep(d)?.eval_d1(u, v)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < 1 {
                return Ok(None);
            }
            Ok(Some(srep(d)?.eval_d1(u, v)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < 1 {
                return Ok(None);
            }
            try_eval_surface_d1_mapped(d, u, v)
        }
    }
}

/// `GeomEval_RepUtils::TryEvalSurfaceD2`.
pub fn try_eval_surface_d2(desc: &Option<SurfaceEvalRepDesc>, u: f64, v: f64) -> Result<Option<SurfResD2>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full => Ok(Some(srep(d)?.eval_d2(u, v)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < 2 {
                return Ok(None);
            }
            Ok(Some(srep(d)?.eval_d2(u, v)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < 2 {
                return Ok(None);
            }
            try_eval_surface_d2_mapped(d, u, v)
        }
    }
}

/// `GeomEval_RepUtils::TryEvalSurfaceD3`.
pub fn try_eval_surface_d3(desc: &Option<SurfaceEvalRepDesc>, u: f64, v: f64) -> Result<Option<SurfResD3>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full => Ok(Some(srep(d)?.eval_d3(u, v)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < 3 {
                return Ok(None);
            }
            Ok(Some(srep(d)?.eval_d3(u, v)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < 3 {
                return Ok(None);
            }
            try_eval_surface_d3_mapped(d, u, v)
        }
    }
}

/// `GeomEval_RepUtils::TryEvalSurfaceDN`.
pub fn try_eval_surface_dn(
    desc: &Option<SurfaceEvalRepDesc>,
    u: f64,
    v: f64,
    nu: i32,
    nv: i32,
) -> Result<Option<gp::Vec>> {
    let total_order = nu + nv;
    let Some(d) = desc else { return Ok(None) };
    if nu < 0 || nv < 0 || total_order < 1 {
        return Ok(None);
    }
    match d.kind {
        EvalRepKind::Full => Ok(Some(srep(d)?.eval_dn(u, v, nu, nv)?)),
        EvalRepKind::DerivBounded => {
            if d.max_deriv_order < total_order as usize {
                return Ok(None);
            }
            Ok(Some(srep(d)?.eval_dn(u, v, nu, nv)?))
        }
        EvalRepKind::Mapped => {
            if d.max_deriv_order < total_order as usize {
                return Ok(None);
            }
            try_eval_surface_dn_mapped(d, u, v, nu, nv)
        }
    }
}

// The mapped parameters, or `None` when they are outside the domain.
fn mapped_params(d: &SurfaceEvalRepDesc, u: f64, v: f64) -> Option<(f64, f64)> {
    let (u_rep, v_rep) = map2d(d, u, v);
    if let Some(dom) = d.domain
        && !domain2d_contains(dom, u_rep, v_rep)
    {
        return None;
    }
    Some((u_rep, v_rep))
}

/// `GeomEval_RepUtils::TryEvalSurfaceD0Mapped`.
pub fn try_eval_surface_d0_mapped(d: &SurfaceEvalRepDesc, u: f64, v: f64) -> Result<Option<Pnt>> {
    let Some((u_rep, v_rep)) = mapped_params(d, u, v) else { return Ok(None) };
    Ok(Some(srep(d)?.eval_d0(u_rep, v_rep)?))
}

/// `GeomEval_RepUtils::TryEvalSurfaceD1Mapped`.
pub fn try_eval_surface_d1_mapped(d: &SurfaceEvalRepDesc, u: f64, v: f64) -> Result<Option<SurfResD1>> {
    let Some((u_rep, v_rep)) = mapped_params(d, u, v) else { return Ok(None) };
    let r = srep(d)?.eval_d1(u_rep, v_rep)?;
    Ok(Some(if d.swap_uv {
        SurfResD1 { point: r.point, d1u: r.d1v.multiplied(d.scale_v), d1v: r.d1u.multiplied(d.scale_u) }
    } else {
        SurfResD1 { point: r.point, d1u: r.d1u.multiplied(d.scale_u), d1v: r.d1v.multiplied(d.scale_v) }
    }))
}

/// `GeomEval_RepUtils::TryEvalSurfaceD2Mapped`.
pub fn try_eval_surface_d2_mapped(d: &SurfaceEvalRepDesc, u: f64, v: f64) -> Result<Option<SurfResD2>> {
    let Some((u_rep, v_rep)) = mapped_params(d, u, v) else { return Ok(None) };
    let r = srep(d)?.eval_d2(u_rep, v_rep)?;
    let su = d.scale_u;
    let sv = d.scale_v;
    let su2 = su * su;
    let sv2 = sv * sv;
    let susv = su * sv;
    Ok(Some(if d.swap_uv {
        SurfResD2 {
            point: r.point,
            d1u: r.d1v.multiplied(sv),
            d1v: r.d1u.multiplied(su),
            d2u: r.d2v.multiplied(sv2),
            d2v: r.d2u.multiplied(su2),
            d2uv: r.d2uv.multiplied(susv),
        }
    } else {
        SurfResD2 {
            point: r.point,
            d1u: r.d1u.multiplied(su),
            d1v: r.d1v.multiplied(sv),
            d2u: r.d2u.multiplied(su2),
            d2v: r.d2v.multiplied(sv2),
            d2uv: r.d2uv.multiplied(susv),
        }
    }))
}

/// `GeomEval_RepUtils::TryEvalSurfaceD3Mapped`.
pub fn try_eval_surface_d3_mapped(d: &SurfaceEvalRepDesc, u: f64, v: f64) -> Result<Option<SurfResD3>> {
    let Some((u_rep, v_rep)) = mapped_params(d, u, v) else { return Ok(None) };
    let r = srep(d)?.eval_d3(u_rep, v_rep)?;
    let su = d.scale_u;
    let sv = d.scale_v;
    Ok(Some(if d.swap_uv {
        SurfResD3 {
            point: r.point,
            d1u: r.d1v.multiplied(sv),
            d1v: r.d1u.multiplied(su),
            d2u: r.d2v.multiplied(sv * sv),
            d2v: r.d2u.multiplied(su * su),
            d2uv: r.d2uv.multiplied(su * sv),
            d3u: r.d3v.multiplied(sv * sv * sv),
            d3v: r.d3u.multiplied(su * su * su),
            d3uuv: r.d3uvv.multiplied(sv * sv * su),
            d3uvv: r.d3uuv.multiplied(su * su * sv),
        }
    } else {
        SurfResD3 {
            point: r.point,
            d1u: r.d1u.multiplied(su),
            d1v: r.d1v.multiplied(sv),
            d2u: r.d2u.multiplied(su * su),
            d2v: r.d2v.multiplied(sv * sv),
            d2uv: r.d2uv.multiplied(su * sv),
            d3u: r.d3u.multiplied(su * su * su),
            d3v: r.d3v.multiplied(sv * sv * sv),
            d3uuv: r.d3uuv.multiplied(su * su * sv),
            d3uvv: r.d3uvv.multiplied(su * sv * sv),
        }
    }))
}

/// `GeomEval_RepUtils::TryEvalSurfaceDNMapped`.
pub fn try_eval_surface_dn_mapped(d: &SurfaceEvalRepDesc, u: f64, v: f64, nu: i32, nv: i32) -> Result<Option<gp::Vec>> {
    let Some((u_rep, v_rep)) = mapped_params(d, u, v) else { return Ok(None) };
    let (nu_rep, nv_rep) = if d.swap_uv { (nv, nu) } else { (nu, nv) };
    let rep_vec = srep(d)?.eval_dn(u_rep, v_rep, nu_rep, nv_rep)?;
    let scale = libm::pow(d.scale_u, f64::from(nu_rep)) * libm::pow(d.scale_v, f64::from(nv_rep));
    Ok(Some(rep_vec.multiplied(scale)))
}
