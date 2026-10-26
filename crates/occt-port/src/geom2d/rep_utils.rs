//! `Geom2dEval_RepCurveDesc` and `Geom2dEval_RepUtils`: evaluation representations of Bezier, B-spline and offset curves.
use super::{Curve2d, Geom2dError, ResD1, ResD2, ResD3, Result, confusion};
use crate::gp::{Pnt2d, Vec2d};
use std::sync::Arc;

/// The kind of an evaluation representation (`Geom2dEval_RepCurveDesc::Base::Kind`).
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

/// `Geom2dEval_RepCurveDesc::Base` with its three kinds (`Full`, `DerivBounded`, `Mapped`).
#[derive(Debug, Clone)]
pub struct EvalRepDesc {
    /// The kind.
    pub kind: EvalRepKind,
    /// The curve used for the evaluation (`Representation`).
    pub representation: Option<Arc<dyn Curve2d>>,
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

/// `Geom2dEval_RepUtils::ValidateCurveDesc`; `owner` is the curve that carries the descriptor.
pub fn validate_curve_desc(desc: &EvalRepDesc, owner: &dyn Curve2d) -> Result<()> {
    let Some(rep) = desc.representation.as_ref() else {
        return Err(Geom2dError::Program);
    };
    if desc.kind == EvalRepKind::Mapped && !map_is_valid(desc.scale) {
        return Err(Geom2dError::Program);
    }
    if std::ptr::addr_eq(Arc::as_ptr(rep), std::ptr::from_ref(owner)) {
        return Err(Geom2dError::Program);
    }
    Ok(())
}

/// `Geom2dEval_RepUtils::CloneCurveDesc`: the representation curve is copied.
pub fn clone_curve_desc(desc: &Option<EvalRepDesc>) -> Option<EvalRepDesc> {
    let d = desc.as_ref()?;
    let rep = d.representation.as_ref()?;
    let rep_copy: Arc<dyn Curve2d> = Arc::from(rep.copy_curve());
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
fn rep(d: &EvalRepDesc) -> Result<&Arc<dyn Curve2d>> {
    d.representation.as_ref().ok_or(Geom2dError::Program)
}

/// `Geom2dEval_RepUtils::TryEvalCurveD0`; `None` is the C++ `false`.
pub fn try_eval_curve_d0(desc: &Option<EvalRepDesc>, u: f64) -> Result<Option<Pnt2d>> {
    let Some(d) = desc else { return Ok(None) };
    match d.kind {
        EvalRepKind::Full | EvalRepKind::DerivBounded => Ok(Some(rep(d)?.eval_d0(u)?)),
        EvalRepKind::Mapped => try_eval_curve_d0_mapped(d, u),
    }
}

/// `Geom2dEval_RepUtils::TryEvalCurveD1`.
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

/// `Geom2dEval_RepUtils::TryEvalCurveD2`.
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

/// `Geom2dEval_RepUtils::TryEvalCurveD3`.
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

/// `Geom2dEval_RepUtils::TryEvalCurveDN`.
pub fn try_eval_curve_dn(desc: &Option<EvalRepDesc>, u: f64, n: i32) -> Result<Option<Vec2d>> {
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

/// `Geom2dEval_RepUtils::TryEvalCurveD0Mapped`.
pub fn try_eval_curve_d0_mapped(d: &EvalRepDesc, u: f64) -> Result<Option<Pnt2d>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    Ok(Some(rep(d)?.eval_d0(u_rep)?))
}

/// `Geom2dEval_RepUtils::TryEvalCurveD1Mapped`.
pub fn try_eval_curve_d1_mapped(d: &EvalRepDesc, u: f64) -> Result<Option<ResD1>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    let r = rep(d)?.eval_d1(u_rep)?;
    let scale = d.scale;
    Ok(Some(ResD1 { point: r.point, d1: r.d1.multiplied(scale) }))
}

/// `Geom2dEval_RepUtils::TryEvalCurveD2Mapped`.
pub fn try_eval_curve_d2_mapped(d: &EvalRepDesc, u: f64) -> Result<Option<ResD2>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    let r = rep(d)?.eval_d2(u_rep)?;
    let scale = d.scale;
    let scale2 = scale * scale;
    Ok(Some(ResD2 { point: r.point, d1: r.d1.multiplied(scale), d2: r.d2.multiplied(scale2) }))
}

/// `Geom2dEval_RepUtils::TryEvalCurveD3Mapped`.
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

/// `Geom2dEval_RepUtils::TryEvalCurveDNMapped`.
pub fn try_eval_curve_dn_mapped(d: &EvalRepDesc, u: f64, n: i32) -> Result<Option<Vec2d>> {
    let Some(u_rep) = mapped_param(d, u) else { return Ok(None) };
    let rep_vec = rep(d)?.eval_dn(u_rep, n)?;
    let mut scale_n = 1.0;
    let mut i = 0;
    while i < n {
        scale_n *= d.scale;
        i += 1;
    }
    Ok(Some(rep_vec.multiplied(scale_n)))
}
