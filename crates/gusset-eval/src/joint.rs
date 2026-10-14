//! Joints (ADR 0026): place a moving body (and the bodies carried with it) so that its
//! joint frame meets the fixed frame, with the joint's motion in between.

use super::*;
use gusset_doc::{FrameRef, JointDef, JointKind};
use gusset_kernel::{Transform, V3, add, cross, dot, normalize, scale, sub};

/// A rigid transform `p -> r · p + t` (rows of `r`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rigid {
    r: [V3; 3],
    t: V3,
}

impl Rigid {
    const ID: Rigid = Rigid { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: [0.0; 3] };

    fn rot(&self, p: V3) -> V3 {
        let [a, b, c] = self.r;
        [dot(a, p), dot(b, p), dot(c, p)]
    }

    fn apply(&self, p: V3) -> V3 {
        add(self.rot(p), self.t)
    }

    /// `self ∘ o` (apply `o` first).
    fn then_after(&self, o: &Rigid) -> Rigid {
        let [c0, c1, c2] = [self.rot(col(&o.r, 0)), self.rot(col(&o.r, 1)), self.rot(col(&o.r, 2))];
        Rigid { r: [[c0[0], c1[0], c2[0]], [c0[1], c1[1], c2[1]], [c0[2], c1[2], c2[2]]], t: self.apply(o.t) }
    }

    fn inverse(&self) -> Rigid {
        let rt = [col(&self.r, 0), col(&self.r, 1), col(&self.r, 2)];
        let inv = Rigid { r: rt, t: [0.0; 3] };
        Rigid { r: rt, t: scale(inv.rot(self.t), -1.0) }
    }

    /// The frame (columns x, y, z at `o`) as the transform from frame to world coordinates.
    fn from_frame(o: V3, x: V3, y: V3, z: V3) -> Rigid {
        Rigid { r: [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]], t: o }
    }

    fn rotation(axis: V3, angle: f64) -> Rigid {
        let (s, c) = (libm::sin(angle), libm::cos(angle));
        let [x, y, z] = axis;
        let k = 1.0 - c;
        Rigid {
            r: [
                [c + x * x * k, x * y * k - z * s, x * z * k + y * s],
                [y * x * k + z * s, c + y * y * k, y * z * k - x * s],
                [z * x * k - y * s, z * y * k + x * s, c + z * z * k],
            ],
            t: [0.0; 3],
        }
    }

    fn translation(t: V3) -> Rigid {
        Rigid { t, ..Rigid::ID }
    }

    /// Axis and angle of the rotation part (angle 0 for none).
    fn axis_angle(&self) -> (V3, f64) {
        let [[r00, r01, r02], [r10, r11, r12], [r20, r21, r22]] = self.r;
        // atan2 of sine and cosine is accurate at every angle (acos is not near a half turn).
        let v = [r21 - r12, r02 - r20, r10 - r01];
        let angle = libm::atan2(gusset_kernel::norm(v) * 0.5, (r00 + r11 + r22 - 1.0) * 0.5);
        if angle < 1e-12 {
            return ([0.0, 0.0, 1.0], 0.0);
        }
        if let Some(axis) = normalize(v).filter(|_| angle < std::f64::consts::PI - 1e-6) {
            return (axis, angle);
        }
        // Near a half turn: the axis is the largest column of (R + I) / 2.
        let m = [[r00 + 1.0, r01, r02], [r10, r11 + 1.0, r12], [r20, r21, r22 + 1.0]];
        let best = (0..3).map(|i| col(&m, i)).max_by(|a, b| dot(*a, *a).total_cmp(&dot(*b, *b)));
        (best.and_then(normalize).unwrap_or([0.0, 0.0, 1.0]), angle)
    }
}

/// The body and index of face `r`, of any surface type.
fn find_any_face(feature: &str, r: &gusset_doc::FaceRef, bodies: &[Arc<Body>]) -> Result<(usize, u32), EvalError> {
    for (bi, b) in bodies.iter().enumerate().rev() {
        if let Some(fi) = b.names.face_index(r) {
            return Ok((bi, fi));
        }
    }
    let mut e = EvalError::new(feature, ErrorCode::MissingReference, format!("face {r} no longer exists"))
        .with_hint("re-select the face the joint attaches to");
    e.reference = Some(r.to_string());
    e.suggestion = bodies.last().and_then(|b| ops::closest_face(&b.names, r)).map(|f| f.to_string());
    Err(e)
}

fn col(m: &[V3; 3], i: usize) -> V3 {
    let [a, b, c] = m;
    let pick = |r: &V3| r.get(i).copied().unwrap_or(0.0);
    [pick(a), pick(b), pick(c)]
}

/// X of a joint frame: the world X axis projected onto the plane normal to `z` (world Y
/// when X is along `z`), so frames are deterministic (ADR 0026 §2).
fn frame_axes(z: V3) -> Option<(V3, V3, V3)> {
    let z = normalize(z)?;
    let project = |a: V3| normalize(sub(a, scale(z, dot(a, z))));
    let x = project([1.0, 0.0, 0.0]).filter(|_| dot(z, [1.0, 0.0, 0.0]).abs() < 1.0 - 1e-9);
    let x = x.or_else(|| project([0.0, 1.0, 0.0]))?;
    Some((x, cross(z, x), z))
}

impl Evaluator {
    fn joint_frame(&self, s: &State, name: &str, f: &FrameRef) -> Result<Rigid, EvalError> {
        let bad = |what: &str| {
            EvalError::new(name, ErrorCode::InvalidReference, format!("a joint frame cannot be placed on {what}"))
                .with_hint("use a planar or cylindrical face, a circular or straight edge, a plane or the origin")
        };
        let (o, z) = match f {
            FrameRef::Origin => return Ok(Rigid::ID),
            FrameRef::Plane(p) => {
                let fr = plane_frame(name, p, s)?;
                return Ok(Rigid::from_frame(fr.origin, fr.x, fr.y, fr.normal()));
            }
            FrameRef::Face(r) => {
                let (bi, fi) = find_any_face(name, r, &s.bodies)?;
                let face =
                    s.bodies.get(bi).and_then(|b| b.topology.faces.get(fi as usize)).ok_or_else(|| bad("this face"))?;
                match (face.kind, face.axis) {
                    (gusset_kernel::SurfaceKind::Plane, _) => (face.centroid, face.normal),
                    (gusset_kernel::SurfaceKind::Sphere, Some(a)) => (a.origin, a.dir),
                    (_, Some(a)) => {
                        // The point of the axis nearest the face's centroid.
                        let d = normalize(a.dir).ok_or_else(|| bad("a degenerate axis"))?;
                        (add(a.origin, scale(d, dot(sub(face.centroid, a.origin), d))), d)
                    }
                    _ => return Err(bad("a freeform face")),
                }
            }
            FrameRef::Edge(e) => {
                let (bi, idx) = resolve_edges(name, std::slice::from_ref(e), &s.bodies)?;
                let body = s.bodies.get(bi).ok_or_else(|| bad("this edge"))?;
                let edge =
                    idx.first().and_then(|&i| body.topology.edges.get(i as usize)).ok_or_else(|| bad("this edge"))?;
                match (edge.circle, edge.kind) {
                    (Some(c), _) => (c.center, c.normal),
                    (None, gusset_kernel::CurveKind::Line) => {
                        let p = |i: usize| edge.vertices.get(i).and_then(|&v| body.topology.vertices.get(v as usize));
                        let (Some(a), Some(b)) = (p(0), p(1)) else { return Err(bad("an edge without two ends")) };
                        (edge.midpoint, sub(b.point, a.point))
                    }
                    _ => return Err(bad("a curved edge that is not a circle")),
                }
            }
        };
        let (x, y, z) = frame_axes(z).ok_or_else(|| bad("a degenerate face or edge"))?;
        Ok(Rigid::from_frame(o, x, y, z))
    }

    /// The joint's motion in its frame, after checking the limits.
    fn joint_motion(name: &str, d: &JointDef, units: LengthUnit, params: &ParamValues) -> Result<Rigid, EvalError> {
        let value = |e: &Option<gusset_units::expr::Expr>, dim| {
            e.as_ref().map(|e| eval_expr(name, e, dim, params, units)).transpose().map(|v| v.unwrap_or(0.0))
        };
        let angle = value(&d.angle, Dim::ANGLE)?;
        let distance = value(&d.distance, Dim::LENGTH)?;
        let check = |v: f64, limits: &Option<[gusset_units::expr::Expr; 2]>, dim, what: &str| {
            let Some([lo, hi]) = limits else { return Ok(()) };
            let (lo, hi) = (eval_expr(name, lo, dim, params, units)?, eval_expr(name, hi, dim, params, units)?);
            if v < lo - 1e-12 || v > hi + 1e-12 {
                return Err(EvalError::new(
                    name,
                    ErrorCode::Expression,
                    format!("the {what} is outside the joint's limits"),
                )
                .with_hint(format!("keep it between the limits ({lo} and {hi} in base units: radians or mm)")));
            }
            Ok(())
        };
        check(angle, &d.angle_limits, Dim::ANGLE, "angle")?;
        check(distance, &d.distance_limits, Dim::LENGTH, "distance")?;
        let z = [0.0, 0.0, 1.0];
        Ok(match d.kind {
            JointKind::Rigid => Rigid::ID,
            JointKind::Revolute => Rigid::rotation(z, angle),
            JointKind::Slider => Rigid::translation([0.0, 0.0, distance]),
            JointKind::Cylindrical => Rigid::translation([0.0, 0.0, distance]).then_after(&Rigid::rotation(z, angle)),
            JointKind::PinSlot => Rigid::translation([distance, 0.0, 0.0]).then_after(&Rigid::rotation(z, angle)),
            JointKind::Planar => {
                let (x, y) = (value(&d.x, Dim::LENGTH)?, value(&d.y, Dim::LENGTH)?);
                Rigid::translation([x, y, 0.0]).then_after(&Rigid::rotation(z, angle))
            }
            JointKind::Ball => {
                let mut a = [0.0; 3];
                if let Some(es) = &d.angles {
                    for (slot, e) in a.iter_mut().zip(es) {
                        *slot = eval_expr(name, e, Dim::ANGLE, params, units)?;
                    }
                }
                let [rx, ry, rz] = a;
                Rigid::rotation(z, rz)
                    .then_after(&Rigid::rotation([0.0, 1.0, 0.0], ry))
                    .then_after(&Rigid::rotation([1.0, 0.0, 0.0], rx))
            }
        })
    }

    pub(crate) fn run_joint(
        &self,
        s: &mut State,
        name: &str,
        d: &JointDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let moving = self.joint_frame(s, name, &d.at)?;
        let fixed = self.joint_frame(s, name, &d.to)?;
        let mut fixed = fixed;
        if let Some(o) = &d.offset {
            let off = eval_expr(name, o, Dim::LENGTH, params, units)?;
            fixed = fixed.then_after(&Rigid::translation([0.0, 0.0, off]));
        }
        if d.flip {
            fixed = fixed.then_after(&Rigid::rotation([1.0, 0.0, 0.0], std::f64::consts::PI));
        }
        let motion = Self::joint_motion(name, d, units, params)?;
        let t = fixed.then_after(&motion).then_after(&moving.inverse());
        let (axis, angle) = t.axis_angle();
        let mut steps = Vec::new();
        if angle != 0.0 {
            steps.push(Transform::Rotate { origin: [0.0; 3], axis, angle });
        }
        // The translation after the rotation about the world origin.
        if dot(t.t, t.t) > 0.0 {
            steps.push(Transform::Translate(t.t));
        }
        let k = self.kernel.as_ref();
        for body_name in std::iter::once(&d.body).chain(&d.with) {
            let (bi, body) = ops::target_body(s, name, Some(body_name))?;
            let mut res: Option<gusset_kernel::OpResult> = None;
            for step in &steps {
                let from = res.as_ref().map_or(&body.shape, |r| &r.shape);
                res = Some(k.transform(from, step).map_err(|ke| kernel_err(name, "joint", ke))?);
            }
            let Some(res) = res else { continue };
            let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
            // A rigid placement changes where the faces are, not what they are: names stay.
            let names = gusset_naming::name_body(
                &gusset_naming::OpContext::Modify { feature: name, inputs: &[&body.names] },
                &res.face_origins,
                &topo,
            )
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
            ops::replace_body(s, bi, &body, res.shape, names, topo);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rigid_algebra() {
        let r = Rigid::rotation([0.0, 0.0, 1.0], 0.5).then_after(&Rigid::translation([1.0, 2.0, 3.0]));
        let p = [0.3, -0.7, 2.0];
        let back = r.inverse().apply(r.apply(p));
        assert!(norm_diff(back, p) < 1e-12);
        let (axis, angle) = Rigid::rotation([0.0, 0.6, 0.8], 2.0).axis_angle();
        assert!((angle - 2.0).abs() < 1e-12 && norm_diff(axis, [0.0, 0.6, 0.8]) < 1e-12);
        let (axis, angle) = Rigid::rotation([0.6, 0.0, 0.8], std::f64::consts::PI).axis_angle();
        assert!((angle - std::f64::consts::PI).abs() < 1e-9);
        assert!(norm_diff(axis, [0.6, 0.0, 0.8]) < 1e-9 || norm_diff(axis, [-0.6, 0.0, -0.8]) < 1e-9);
    }

    fn norm_diff(a: V3, b: V3) -> f64 {
        gusset_kernel::norm(sub(a, b))
    }
}
