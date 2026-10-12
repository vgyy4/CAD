//! Datum planes: construction planes that sketches and mirrors can use.

use super::*;
use gusset_doc::{PlaneAxis, PlaneDef, PlaneKind};
use gusset_kernel::{CurveKind, add};

/// Datum plane frames by feature name, in feature order; the list is shared between the
/// cached states that follow each other.
#[derive(Debug, Clone, Default)]
pub(crate) struct Planes(Arc<Vec<(String, Frame)>>);

impl Planes {
    pub(crate) fn get(&self, name: &str) -> Option<&Frame> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, f)| f)
    }

    fn push(&mut self, name: &str, frame: Frame) {
        Arc::make_mut(&mut self.0).push((name.to_string(), frame));
    }

    pub(crate) fn to_vec(&self) -> Vec<(String, Frame)> {
        self.0.as_ref().clone()
    }
}

impl Evaluator {
    pub(crate) fn run_plane(
        &self,
        s: &mut State,
        name: &str,
        d: &PlaneDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let base = plane_frame(name, &d.base, s)?;
        let frame = match &d.kind {
            PlaneKind::Offset(e) => {
                let dist = eval_expr(name, e, Dim::LENGTH, params, units)?;
                Frame { origin: add(base.origin, scale(base.normal(), dist)), ..base }
            }
            PlaneKind::Angle { axis, angle } => {
                let angle = eval_expr(name, angle, Dim::ANGLE, params, units)?;
                let (point, dir) = plane_axis(name, axis, &s.bodies)?;
                rotate_frame(&base, point, dir, angle)
            }
        };
        if !frame.is_valid() {
            return Err(EvalError::new(name, ErrorCode::KernelFailure, "the datum plane is degenerate")
                .with_hint("check the offset or angle: it must be a finite number"));
        }
        s.planes.push(name, frame);
        Ok(())
    }
}

/// A point on the rotation axis and its unit direction.
fn plane_axis(feature: &str, axis: &PlaneAxis, bodies: &[Arc<Body>]) -> Result<([f64; 3], [f64; 3]), EvalError> {
    match axis {
        PlaneAxis::X => Ok(([0.0; 3], [1.0, 0.0, 0.0])),
        PlaneAxis::Y => Ok(([0.0; 3], [0.0, 1.0, 0.0])),
        PlaneAxis::Z => Ok(([0.0; 3], [0.0, 0.0, 1.0])),
        PlaneAxis::Edge(r) => {
            let (bi, edges) = resolve_edges(feature, std::slice::from_ref(r), bodies)?;
            let body = bodies
                .get(bi)
                .ok_or_else(|| EvalError::new(feature, ErrorCode::InvalidReference, "the axis edge has no body"))?;
            let edge = edges.first().and_then(|&i| body.topology.edges.get(i as usize)).ok_or_else(|| {
                EvalError::new(feature, ErrorCode::MissingReference, format!("edge {r} no longer exists"))
                    .with_hint("re-select the axis edge")
            })?;
            let ends = match edge.vertices.as_slice() {
                [a, b] if edge.kind == CurveKind::Line => {
                    body.topology.vertices.get(*a as usize).zip(body.topology.vertices.get(*b as usize))
                }
                _ => None,
            };
            let Some((a, b)) = ends else {
                let mut e =
                    EvalError::new(feature, ErrorCode::InvalidReference, format!("edge {r} is not a straight edge"))
                        .with_hint("use X, Y, Z or a straight edge as the axis");
                e.reference = Some(r.to_string());
                return Err(e);
            };
            let dir = normalize(sub(b.point, a.point)).ok_or_else(|| {
                EvalError::new(feature, ErrorCode::InvalidReference, format!("edge {r} has no length"))
                    .with_hint("use X, Y, Z or a straight edge as the axis")
            })?;
            // The direction of an edge has no meaning of its own, so it is made canonical:
            // towards +X (else +Y, else +Z). Rebuilds then always turn the same way.
            let flip = dir.iter().find(|c| c.abs() > 1e-9).is_some_and(|c| *c < 0.0);
            Ok((a.point, if flip { scale(dir, -1.0) } else { dir }))
        }
    }
}

/// Turns a frame about the line through `point` along the unit vector `axis`.
fn rotate_frame(f: &Frame, point: [f64; 3], axis: [f64; 3], angle: f64) -> Frame {
    let (sin, cos) = angle.sin_cos();
    let rot =
        |v: [f64; 3]| add(add(scale(v, cos), scale(cross(axis, v), sin)), scale(axis, dot(axis, v) * (1.0 - cos)));
    let x = normalize(rot(f.x)).unwrap_or(f.x);
    let y = normalize(rot(f.y)).unwrap_or(f.y);
    Frame { origin: add(point, rot(sub(f.origin, point))), x, y }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_a_frame_about_x() {
        let f = rotate_frame(&Frame::XY, [0.0; 3], [1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_2);
        // The XY plane turned 90 degrees about X has the normal -Y (z -> -y).
        let n = f.normal();
        assert!(n[0].abs() < 1e-12 && (n[1] + 1.0).abs() < 1e-12 && n[2].abs() < 1e-12, "{n:?}");
        assert!(f.is_valid());
        let g = rotate_frame(
            &Frame { origin: [0.0, 0.0, 5.0], ..Frame::XY },
            [0.0; 3],
            [1.0, 0.0, 0.0],
            std::f64::consts::PI,
        );
        assert!((g.origin[2] + 5.0).abs() < 1e-12);
    }
}
