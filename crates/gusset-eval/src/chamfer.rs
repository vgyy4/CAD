//! Unequal chamfers (ADR 0004 amendment, chamfer ADR amendment): two distances, or a
//! distance and an angle, measured from a reference face per edge.
//!
//! Reference face rule: the face named for that edge in `faces`, which must be one of the
//! edge's two faces; without one, the adjacent face whose persistent name sorts first
//! (`FaceRef` order: feature name, then key). Names, not kernel indices, decide, so the
//! choice stays the same when the model regenerates and the kernel numbers faces differently.

use super::*;
use gusset_doc::{ChamferSecond, EdgeBlendDef};
use gusset_kernel::ChamferSpec;

/// What the kernel is called with, and the longest leg (for the "too large" message).
pub(crate) struct ChamferPlan {
    pub spec: ChamferSpec,
    /// One reference face index per edge (empty for an equal chamfer).
    pub faces: Vec<u32>,
    pub longest: f64,
}

/// Validates and evaluates a chamfer. `first` is the evaluated `distance` (> 0);
/// `edges` are the resolved edge indices of `body`.
pub(crate) fn plan(
    name: &str,
    b: &EdgeBlendDef,
    first: f64,
    edges: &[u32],
    body: &Body,
    params: &ParamValues,
    units: LengthUnit,
) -> Result<ChamferPlan, EvalError> {
    let expr_err = |msg: String, hint: &str| EvalError::new(name, ErrorCode::Expression, msg).with_hint(hint);
    let (spec, longest) = match &b.second {
        ChamferSecond::Equal => {
            return Ok(ChamferPlan { spec: ChamferSpec::Equal(first), faces: Vec::new(), longest: first });
        }
        ChamferSecond::Distance(e) => {
            let second = eval_expr(name, e, Dim::LENGTH, params, units)?;
            if !second.is_finite() || second <= 0.0 {
                return Err(expr_err("the second distance must be positive".into(), "use a positive distance2"));
            }
            (ChamferSpec::TwoDistances { first, second }, first.max(second))
        }
        ChamferSecond::Angle(e) => {
            let angle = eval_expr(name, e, Dim::ANGLE, params, units)?;
            if !angle.is_finite() || angle <= 0.0 || angle >= std::f64::consts::FRAC_PI_2 {
                return Err(expr_err(
                    format!("the chamfer angle is {:.1} deg", angle.to_degrees()),
                    "use an angle between 0 and 90 deg (45 deg is an equal chamfer)",
                ));
            }
            (ChamferSpec::DistanceAngle { distance: first, angle }, first.max(first * libm::tan(angle)))
        }
    };
    if !b.faces.is_empty() && b.faces.len() != edges.len() {
        return Err(EvalError::new(
            name,
            ErrorCode::InvalidReference,
            format!("{} reference faces for {} edges", b.faces.len(), edges.len()),
        )
        .with_hint("give one reference face per edge, or none"));
    }
    let mut faces = Vec::with_capacity(edges.len());
    for (k, &ei) in edges.iter().enumerate() {
        let adjacent: &[u32] = body.topology.edges.get(ei as usize).map_or(&[], |e| e.faces.as_slice());
        let face = match b.faces.get(k) {
            Some(r) => {
                let Some(fi) = body.names.face_index(r) else {
                    let mut e = EvalError::new(name, ErrorCode::MissingReference, format!("face {r} no longer exists"))
                        .with_hint("re-select the reference face");
                    e.reference = Some(r.to_string());
                    e.suggestion = crate::ops::closest_face(&body.names, r).map(|f| f.to_string());
                    return Err(e);
                };
                if !adjacent.contains(&fi) {
                    let mut e = EvalError::new(
                        name,
                        ErrorCode::InvalidReference,
                        format!("face {r} does not border the edge it is the reference for"),
                    )
                    .with_hint("name one of the two faces that meet at the edge, or leave faces out");
                    e.reference = Some(r.to_string());
                    return Err(e);
                }
                fi
            }
            None => adjacent
                .iter()
                .copied()
                .min_by(|&x, &y| {
                    body.names.faces.get(x as usize).cmp(&body.names.faces.get(y as usize)).then(x.cmp(&y))
                })
                .ok_or_else(|| {
                    EvalError::new(name, ErrorCode::InvalidReference, "the edge has no adjacent face")
                        .with_hint("pick an edge between two faces")
                })?,
        };
        faces.push(face);
    }
    Ok(ChamferPlan { spec, faces, longest })
}
