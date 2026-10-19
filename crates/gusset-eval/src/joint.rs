//! Joints (ADR 0026): place a moving body (and the bodies carried with it) so that its
//! joint frame meets the fixed frame, with the joint's motion in between.

use super::*;
use gusset_doc::{FrameRef, JointDef, JointKind};
use gusset_kernel::{Transform, V3, add, cross, dot, normalize, scale, sub};

/// A rigid transform `p -> r · p + t` (rows of `r`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rigid {
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

    fn frame(&self) -> JointFrame {
        JointFrame { origin: self.t, x: col(&self.r, 0), z: col(&self.r, 2) }
    }

    /// The rotation part as a rotation vector (axis × angle).
    fn rotation_vector(&self) -> V3 {
        let (axis, angle) = self.axis_angle();
        scale(axis, angle)
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
    /// The frame `f` in world coordinates, and the body (its creating feature) it lies on.
    fn joint_frame(&self, s: &State, name: &str, f: &FrameRef) -> Result<(Rigid, Option<String>), EvalError> {
        let bad = |what: &str| {
            EvalError::new(name, ErrorCode::InvalidReference, format!("a joint frame cannot be placed on {what}"))
                .with_hint("use a planar or cylindrical face, a circular or straight edge, a plane or the origin")
        };
        let on = |bi: usize| s.bodies.get(bi).map(|b| b.feature.clone());
        let (o, z, body) = match f {
            FrameRef::Origin => return Ok((Rigid::ID, None)),
            FrameRef::Plane(p) => {
                let fr = plane_frame(name, p, s)?;
                return Ok((Rigid::from_frame(fr.origin, fr.x, fr.y, fr.normal()), None));
            }
            FrameRef::Face(r) => {
                let (bi, fi) = find_any_face(name, r, &s.bodies)?;
                let face =
                    s.bodies.get(bi).and_then(|b| b.topology.faces.get(fi as usize)).ok_or_else(|| bad("this face"))?;
                match (face.kind, face.axis) {
                    (gusset_kernel::SurfaceKind::Plane, _) => (face.centroid, face.normal, on(bi)),
                    (gusset_kernel::SurfaceKind::Sphere, Some(a)) => (a.origin, a.dir, on(bi)),
                    (_, Some(a)) => {
                        // The point of the axis nearest the face's centroid.
                        let d = normalize(a.dir).ok_or_else(|| bad("a degenerate axis"))?;
                        (add(a.origin, scale(d, dot(sub(face.centroid, a.origin), d))), d, on(bi))
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
                    (Some(c), _) => (c.center, c.normal, on(bi)),
                    (None, gusset_kernel::CurveKind::Line) => {
                        let p = |i: usize| edge.vertices.get(i).and_then(|&v| body.topology.vertices.get(v as usize));
                        let (Some(a), Some(b)) = (p(0), p(1)) else { return Err(bad("an edge without two ends")) };
                        (edge.midpoint, sub(b.point, a.point), on(bi))
                    }
                    _ => return Err(bad("a curved edge that is not a circle")),
                }
            }
        };
        let (x, y, z) = frame_axes(z).ok_or_else(|| bad("a degenerate face or edge"))?;
        Ok((Rigid::from_frame(o, x, y, z), body))
    }

    /// The joint's values (expressions, 0 when not written) and evaluated limits.
    fn joint_values(name: &str, d: &JointDef, units: LengthUnit, params: &ParamValues) -> Result<Vals, EvalError> {
        let value = |e: &Option<gusset_units::expr::Expr>, dim| {
            e.as_ref().map(|e| eval_expr(name, e, dim, params, units)).transpose().map(|v| v.unwrap_or(0.0))
        };
        let limits = |l: &Option<[gusset_units::expr::Expr; 2]>, dim| {
            l.as_ref()
                .map(|[lo, hi]| {
                    Ok::<_, EvalError>((
                        eval_expr(name, lo, dim, params, units)?,
                        eval_expr(name, hi, dim, params, units)?,
                    ))
                })
                .transpose()
        };
        let mut angles = [0.0; 3];
        if let Some(es) = &d.angles {
            for (slot, e) in angles.iter_mut().zip(es) {
                *slot = eval_expr(name, e, Dim::ANGLE, params, units)?;
            }
        }
        Ok(Vals {
            angle: value(&d.angle, Dim::ANGLE)?,
            distance: value(&d.distance, Dim::LENGTH)?,
            x: value(&d.x, Dim::LENGTH)?,
            y: value(&d.y, Dim::LENGTH)?,
            angles,
            angle_limits: limits(&d.angle_limits, Dim::ANGLE)?,
            distance_limits: limits(&d.distance_limits, Dim::LENGTH)?,
        })
    }

    /// Checks which bodies the two frames lie on (ADR 0026, review M3): the `at` frame on
    /// the moving body or a carried one, the `to` frame on none of them.
    fn check_frame_bodies(
        name: &str,
        d: &JointDef,
        at_body: Option<&str>,
        to_body: Option<&str>,
    ) -> Result<(), EvalError> {
        let moves = |b: &str| b == d.body || d.with.iter().any(|w| w == b);
        if let Some(b) = at_body.filter(|b| !moves(b)) {
            return Err(EvalError::new(
                name,
                ErrorCode::InvalidReference,
                format!(
                    "the 'at' frame {} lies on '{b}', which this joint does not move",
                    gusset_doc::text::print_frame(&d.at)
                ),
            )
            .with_hint(format!("use a frame on '{}' (or a body in with), and put the other body in 'to'", d.body)));
        }
        if let Some(b) = to_body.filter(|b| moves(b)) {
            return Err(EvalError::new(
                name,
                ErrorCode::InvalidReference,
                format!(
                    "the 'to' frame {} lies on '{b}', which this joint moves",
                    gusset_doc::text::print_frame(&d.to)
                ),
            )
            .with_hint("the 'to' frame must lie on a body that stays: another body, a plane or the origin"));
        }
        Ok(())
    }

    pub(crate) fn run_joint(
        &self,
        s: &mut State,
        name: &str,
        d: &JointDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let (moving, at_body) = self.joint_frame(s, name, &d.at)?;
        let (mut fixed, fixed_body) = self.joint_frame(s, name, &d.to)?;
        Self::check_frame_bodies(name, d, at_body.as_deref(), fixed_body.as_deref())?;
        if let Some(o) = &d.offset {
            let off = eval_expr(name, o, Dim::LENGTH, params, units)?;
            fixed = fixed.then_after(&Rigid::translation([0.0, 0.0, off]));
        }
        if d.flip {
            fixed = fixed.then_after(&Rigid::rotation([1.0, 0.0, 0.0], std::f64::consts::PI));
        }
        let vals = Self::joint_values(name, d, units, params)?;
        // Everything is built on the side and installed only when the joint succeeds, so a
        // failing joint leaves no record and moves nothing (review M2).
        let mut js = (*s.joints).clone();
        let is_placed = |js: &Joints, b: &str| {
            js.recs.iter().any(|r| !r.closing && (r.moving == b || r.with.iter().any(|w| w == b)))
        };
        if let Some((_, w)) = d.with.iter().enumerate().find(|(i, w)| d.with.iter().take(*i).any(|o| o == *w)) {
            return Err(EvalError::new(name, ErrorCode::InvalidReference, format!("'{w}' is listed twice in 'with'"))
                .with_hint("list each carried body once (a body name and its feature name are the same body)"));
        }
        let closing = is_placed(&js, &d.body);
        if closing && !d.with.is_empty() {
            return Err(EvalError::new(
                name,
                ErrorCode::InvalidReference,
                format!("a joint that closes a loop cannot carry bodies ('{}' is already placed)", d.body),
            )
            .with_hint("remove 'with'; bodies carried along belong to the joint that placed them first"));
        }
        if !closing && let Some(w) = d.with.iter().find(|w| is_placed(&js, w)) {
            return Err(EvalError::new(
                name,
                ErrorCode::InvalidReference,
                format!("'{w}' in 'with' was already placed by an earlier joint"),
            )
            .with_hint("carry only bodies no joint has placed; give the body its own joint"));
        }
        let placed = |b: &str| js.placed.get(b).copied().unwrap_or(Rigid::ID);
        let fixed_place = fixed_body.as_deref().map_or(Rigid::ID, placed);
        // Frames in their bodies' coordinates before any joint moved them, so the loop
        // solver can place every body again from the joint values alone.
        let rec = JointRec {
            name: name.to_string(),
            kind: d.kind,
            moving: d.body.clone(),
            with: d.with.clone(),
            fixed: fixed_body,
            fixed_local: fixed_place.inverse().then_after(&fixed),
            moving_local: placed(&d.body).inverse().then_after(&moving),
            free: dedup_slots(d.free.iter().flat_map(|v| Slot::of(v))),
            closing,
            vals,
        };
        js.recs.push(rec);
        let mut moved: Vec<(usize, Arc<Body>)> = Vec::new();
        if closing {
            let mut steps = 0u64;
            let r = self.close_loops(s, &mut js, name, units, &mut moved, &mut steps);
            s.work.joint_steps = s.work.joint_steps.saturating_add(steps);
            r?;
        } else {
            vals.check(name, name, units)?;
            let t = fixed.then_after(&motion(d.kind, &vals)).then_after(&moving.inverse());
            for b in std::iter::once(&d.body).chain(&d.with) {
                let now = js.placed.get(b).copied().unwrap_or(Rigid::ID);
                moved.extend(self.moved_body(s, name, b, &t)?);
                js.placed.insert(b.clone(), t.then_after(&now));
            }
        }
        for (bi, nb) in moved {
            if let Some(slot) = s.bodies.get_mut(bi) {
                *slot = nb;
            }
        }
        s.joints = Arc::new(js);
        Ok(())
    }

    /// Solves the free values of the joints connected to the newest (closing) joint so
    /// that the loop's closing joints meet, then works out the bodies' new places (ADR 0026
    /// §1, closed loops). Nothing in `s` changes: the caller installs `js` and `moved`.
    fn close_loops(
        &self,
        s: &State,
        js: &mut Joints,
        name: &str,
        units: LengthUnit,
        moved: &mut Vec<(usize, Arc<Body>)>,
        steps: &mut u64,
    ) -> Result<(), EvalError> {
        let all = js.recs.clone();
        // Only the joints that can change the closing joints' residuals are in the problem.
        let members: Vec<usize> = loop_members(&all).into_iter().collect();
        let sub: Vec<JointRec> = members.iter().filter_map(|&i| all.get(i).cloned()).collect();
        let unknowns: Vec<(usize, Slot)> =
            sub.iter().enumerate().flat_map(|(i, r)| r.free.iter().map(move |&f| (i, f))).collect();
        if unknowns.len() > MAX_UNKNOWNS {
            return Err(EvalError::new(
                name,
                ErrorCode::TooComplex,
                format!("the joint loop has {} free values to solve for (the limit is {MAX_UNKNOWNS})", unknowns.len()),
            )
            .with_hint("list fewer values in free: only the ones the loop needs to close"));
        }
        let q0: Vec<f64> = unknowns.iter().map(|&(i, f)| sub.get(i).map_or(0.0, |r| f.get(&r.vals))).collect();
        let with_values = |q: &[f64]| {
            let mut rs = sub.clone();
            for (&(i, f), &v) in unknowns.iter().zip(q) {
                if let Some(r) = rs.get_mut(i) {
                    f.set(&mut r.vals, v);
                }
            }
            rs
        };
        // Every residual places each joint of the loop once; all loop solving of one rebuild
        // shares `max_joint_steps` of that work (security review v0.3 W1).
        let left = self.limits.max_joint_steps.saturating_sub(s.work.joint_steps);
        let used = std::cell::Cell::new(0u64);
        let per = u64::try_from(sub.len()).unwrap_or(u64::MAX).max(1);
        let gap = |q: &[f64]| {
            used.set(used.get().saturating_add(per));
            loop_residual(&with_values(q))
        };
        let over = || used.get() > left;
        let result = if unknowns.is_empty() {
            Ok((q0.clone(), gap(&q0)))
        } else {
            let (q, r, stopped) = least_squares(&q0, gap, || self.is_cancelled() || over());
            match (stopped, self.is_cancelled()) {
                (true, true) => Err(budget::cancelled(name)),
                (true, false) => Err(EvalError::new(
                    name,
                    ErrorCode::TooComplex,
                    "the joint loops need more solving than one rebuild allows",
                )
                .with_hint("close fewer or shorter loops, or fix values with fewer free entries")),
                _ => Ok((q, r)),
            }
        };
        *steps = used.get();
        let (q, r) = result?;
        let spread = |r: &[f64]| {
            r.chunks(6).fold((0.0_f64, 0.0_f64), |(m, t), c| {
                let n = |a: &[f64]| a.iter().map(|x| x * x).sum::<f64>().sqrt();
                (m.max(n(c.get(..3).unwrap_or(&[]))), t.max(n(c.get(3..).unwrap_or(&[]))))
            })
        };
        let (off_mm, turned) = spread(&r);
        if off_mm > LOOP_TOL_MM || turned > LOOP_TOL_RAD {
            let first = all.iter().find(|r| {
                all.last().is_some_and(|c| !r.closing && (r.moving == c.moving || r.with.contains(&c.moving)))
            });
            let msg = format!("the joint loop cannot close: {off_mm:.4} mm and {:.4}° apart", turned.to_degrees());
            // Would the loop close if the closing joint's own angle were free too? Then
            // that is the fix to name (dogfooding F4-5).
            let own_angle_fixed = all
                .last()
                .is_some_and(|c| c.kind.has_value("angle") && !Slot::of("angle").iter().all(|sl| c.free.contains(sl)));
            let closing_at = members.iter().position(|&i| i + 1 == all.len());
            let freeing_fixes = match closing_at {
                Some(ci) if own_angle_fixed && !unknowns.is_empty() => {
                    let mut u2 = unknowns.clone();
                    u2.push((ci, Slot::Angle));
                    let mut q2 = q.clone();
                    q2.push(sub.get(ci).map_or(0.0, |r| Slot::Angle.get(&r.vals)));
                    let gap2 = |qq: &[f64]| {
                        used.set(used.get().saturating_add(per));
                        let mut rs = sub.clone();
                        for (&(i, f), &v) in u2.iter().zip(qq) {
                            if let Some(r) = rs.get_mut(i) {
                                f.set(&mut r.vals, v);
                            }
                        }
                        loop_residual(&rs)
                    };
                    let (_, r2, stopped) = least_squares(&q2, gap2, || self.is_cancelled() || over());
                    *steps = used.get();
                    let (m2, t2) = spread(&r2);
                    !stopped && m2 <= LOOP_TOL_MM && t2 <= LOOP_TOL_RAD
                }
                _ => false,
            };
            let hint = if freeing_fixes {
                format!(
                    "the pieces meet, but the closing joint's own angle is fixed at its value: add free: [angle] to {name}"
                )
            } else if unknowns.is_empty() {
                format!(
                    "'{}' is already placed by {}: list the values the solver may choose in free, e.g. free: [angle]",
                    all.last().map_or("", |c| c.moving.as_str()),
                    first.map_or("an earlier joint", |r| r.name.as_str())
                )
            } else {
                "check the frames and lengths: the mechanism cannot be assembled like this (or start from other values)"
                    .into()
            };
            return Err(EvalError::new(name, ErrorCode::InvalidReference, msg).with_hint(hint));
        }
        let solved_sub = with_values(&q);
        for r in &solved_sub {
            r.vals.check(name, &r.name, units)?;
        }
        let mut solved = all;
        for (&i, r) in members.iter().zip(solved_sub) {
            if let Some(slot) = solved.get_mut(i) {
                *slot = r;
            }
        }
        // Move every body from where it is to where the solution puts it; bodies whose
        // place changed only by rounding noise are left alone (review L2).
        let target = placements(&solved);
        for (body, p) in &target {
            let now = js.placed.get(body).copied().unwrap_or(Rigid::ID);
            let delta = p.then_after(&now.inverse());
            moved.extend(self.moved_body(s, name, body, &delta)?);
        }
        js.recs = solved;
        js.placed = target;
        Ok(())
    }

    /// Body `body` (its creating feature) moved rigidly by `t`, as its index and new value;
    /// `None` when `t` is rounding noise. Changes nothing.
    fn moved_body(
        &self,
        s: &State,
        name: &str,
        body: &str,
        t: &Rigid,
    ) -> Result<Option<(usize, Arc<Body>)>, EvalError> {
        let (axis, angle) = t.axis_angle();
        let mut steps = Vec::new();
        if angle > PLACE_TOL_RAD {
            steps.push(Transform::Rotate { origin: [0.0; 3], axis, angle });
        }
        // The translation after the rotation about the world origin.
        if dot(t.t, t.t) > PLACE_TOL_MM * PLACE_TOL_MM {
            steps.push(Transform::Translate(t.t));
        }
        if steps.is_empty() {
            return Ok(None);
        }
        let k = self.kernel.as_ref();
        let (bi, b) = ops::target_body(s, name, Some(body))?;
        let mut res: Option<gusset_kernel::OpResult> = None;
        for step in &steps {
            let from = res.as_ref().map_or(&b.shape, |r| &r.shape);
            res = Some(k.transform(from, step).map_err(|ke| kernel_err(name, "joint", ke))?);
        }
        let Some(res) = res else { return Ok(None) };
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        // A rigid placement changes where the faces are, not what they are: names stay.
        let names = gusset_naming::name_body(
            &gusset_naming::OpContext::Modify { feature: name, inputs: &[&b.names] },
            &res.face_origins,
            &topo,
        )
        .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        Ok(Some((bi, ops::rebuilt_body(&b, res.shape, names, topo))))
    }
}

/// Gaps the loop solver accepts.
const LOOP_TOL_MM: f64 = 1e-6;
const LOOP_TOL_RAD: f64 = 1e-8;
/// Placement changes below these are rounding noise: the body is not transformed again.
const PLACE_TOL_MM: f64 = 1e-9;
const PLACE_TOL_RAD: f64 = 1e-12;
/// Most values the loop solver may choose at once (the solve is cubic in this).
const MAX_UNKNOWNS: usize = 60;

fn dedup_slots(it: impl Iterator<Item = Slot>) -> Vec<Slot> {
    let mut out: Vec<Slot> = Vec::new();
    for s in it {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// Indices of the non-closing joints that place `bodies`, directly or through the bodies
/// their fixed frames lie on.
fn placers(recs: &[JointRec], bodies: &[&str]) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    let mut todo: Vec<String> = bodies.iter().map(|b| (*b).to_string()).collect();
    while let Some(b) = todo.pop() {
        let Some(j) = recs.iter().rposition(|r| !r.closing && (r.moving == b || r.with.contains(&b))) else {
            continue;
        };
        if out.insert(j)
            && let Some(f) = recs.get(j).and_then(|r| r.fixed.clone())
        {
            todo.push(f);
        }
    }
    out
}

/// The joints that matter when the newest (closing) joint closes: itself, the joints
/// that placed its two bodies, and the earlier closing joints that share a joint with free
/// values with them (their loops must stay closed).
fn loop_members(recs: &[JointRec]) -> BTreeSet<usize> {
    let mut set = BTreeSet::new();
    let Some(last) = recs.len().checked_sub(1) else { return set };
    let chain = |i: usize| {
        let mut c = recs.get(i).map_or_else(BTreeSet::new, |r| {
            let mut bodies = vec![r.moving.as_str()];
            bodies.extend(r.fixed.as_deref());
            placers(recs, &bodies)
        });
        c.insert(i);
        c
    };
    set.extend(chain(last));
    loop {
        let mut grew = false;
        for (i, r) in recs.iter().enumerate() {
            if !r.closing || set.contains(&i) {
                continue;
            }
            let c = chain(i);
            let shares = c.iter().any(|j| set.contains(j) && recs.get(*j).is_some_and(|r| !r.free.is_empty()));
            if shares {
                set.extend(c);
                grew = true;
            }
        }
        if !grew {
            return set;
        }
    }
}

/// Numeric values of one joint (base units: mm, rad) and its evaluated limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vals {
    pub(crate) angle: f64,
    pub(crate) distance: f64,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) angles: [f64; 3],
    angle_limits: Option<(f64, f64)>,
    distance_limits: Option<(f64, f64)>,
}

impl Vals {
    /// Fails when a value is outside its limits; the error is on `feature` and names the
    /// `joint` (they differ when a closing joint's solution breaks an earlier joint's
    /// limit). Values are shown in the document's unit and in degrees (review N2).
    fn check(&self, feature: &str, joint: &str, units: LengthUnit) -> Result<(), EvalError> {
        for (v, l, what, dim) in [
            (self.angle, self.angle_limits, "angle", Dim::ANGLE),
            (self.distance, self.distance_limits, "distance", Dim::LENGTH),
        ] {
            if let Some((lo, hi)) = l
                && (v < lo - 1e-12 || v > hi + 1e-12)
            {
                let show = |x: f64| gusset_units::Quantity::new(x, dim).display(units);
                return Err(EvalError::new(
                    feature,
                    ErrorCode::Expression,
                    format!("joint {joint}: the {what} {} is outside the joint's limits", show(v)),
                )
                .with_hint(format!("keep it between the limits ({} and {})", show(lo), show(hi))));
            }
        }
        Ok(())
    }
}

/// A value the loop solver may choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    Angle,
    Distance,
    X,
    Y,
    Ball(usize),
}

impl Slot {
    fn of(v: &str) -> Vec<Slot> {
        match v {
            "angle" => vec![Slot::Angle],
            "distance" => vec![Slot::Distance],
            "x" => vec![Slot::X],
            "y" => vec![Slot::Y],
            "angles" => vec![Slot::Ball(0), Slot::Ball(1), Slot::Ball(2)],
            _ => Vec::new(),
        }
    }

    fn get(self, v: &Vals) -> f64 {
        match self {
            Slot::Angle => v.angle,
            Slot::Distance => v.distance,
            Slot::X => v.x,
            Slot::Y => v.y,
            Slot::Ball(i) => v.angles.get(i).copied().unwrap_or(0.0),
        }
    }

    fn set(self, v: &mut Vals, x: f64) {
        match self {
            Slot::Angle => v.angle = x,
            Slot::Distance => v.distance = x,
            Slot::X => v.x = x,
            Slot::Y => v.y = x,
            Slot::Ball(i) => {
                if let Some(a) = v.angles.get_mut(i) {
                    *a = x;
                }
            }
        }
    }
}

/// One evaluated joint, with its frames in the coordinates of the bodies before any
/// joint moved them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JointRec {
    pub(crate) name: String,
    kind: JointKind,
    moving: String,
    with: Vec<String>,
    /// The body the fixed frame lies on (`None`: the world).
    fixed: Option<String>,
    fixed_local: Rigid,
    moving_local: Rigid,
    free: Vec<Slot>,
    /// Its moving body was already placed: it closes a loop and moves nothing itself.
    closing: bool,
    pub(crate) vals: Vals,
}

/// The joints evaluated so far and where they put each body.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Joints {
    pub(crate) recs: Vec<JointRec>,
    placed: BTreeMap<String, Rigid>,
}

impl Joints {
    /// Each joint's fixed frame (after offset and flip) and moving frame in world
    /// coordinates, where the joints placed the bodies (dogfooding F4-3).
    pub(crate) fn world_frames(&self) -> BTreeMap<String, (JointFrame, JointFrame)> {
        let at = |b: Option<&String>| b.and_then(|b| self.placed.get(b)).copied().unwrap_or(Rigid::ID);
        self.recs
            .iter()
            .map(|r| {
                let to = at(r.fixed.as_ref()).then_after(&r.fixed_local);
                let moving = at(Some(&r.moving)).then_after(&r.moving_local);
                (r.name.clone(), (to.frame(), moving.frame()))
            })
            .collect()
    }
}

/// The joint's motion in its frame.
fn motion(kind: JointKind, v: &Vals) -> Rigid {
    let z = [0.0, 0.0, 1.0];
    match kind {
        JointKind::Rigid => Rigid::ID,
        JointKind::Revolute => Rigid::rotation(z, v.angle),
        JointKind::Slider => Rigid::translation([0.0, 0.0, v.distance]),
        JointKind::Cylindrical => Rigid::translation([0.0, 0.0, v.distance]).then_after(&Rigid::rotation(z, v.angle)),
        JointKind::PinSlot => Rigid::translation([v.distance, 0.0, 0.0]).then_after(&Rigid::rotation(z, v.angle)),
        JointKind::Planar => Rigid::translation([v.x, v.y, 0.0]).then_after(&Rigid::rotation(z, v.angle)),
        JointKind::Ball => {
            let [rx, ry, rz] = v.angles;
            Rigid::rotation(z, rz)
                .then_after(&Rigid::rotation([0.0, 1.0, 0.0], ry))
                .then_after(&Rigid::rotation([1.0, 0.0, 0.0], rx))
        }
    }
}

/// Where the (non-closing) joints put every body, replayed from their values.
fn placements(recs: &[JointRec]) -> BTreeMap<String, Rigid> {
    let mut placed: BTreeMap<String, Rigid> = BTreeMap::new();
    for r in recs.iter().filter(|r| !r.closing) {
        let at = |b: &Option<String>, p: &BTreeMap<String, Rigid>| {
            b.as_ref().and_then(|b| p.get(b)).copied().unwrap_or(Rigid::ID)
        };
        let fixed = at(&r.fixed, &placed).then_after(&r.fixed_local);
        let moving = placed.get(&r.moving).copied().unwrap_or(Rigid::ID).then_after(&r.moving_local);
        let t = fixed.then_after(&motion(r.kind, &r.vals)).then_after(&moving.inverse());
        for b in std::iter::once(&r.moving).chain(&r.with) {
            let now = placed.get(b).copied().unwrap_or(Rigid::ID);
            placed.insert(b.clone(), t.then_after(&now));
        }
    }
    placed
}

/// For every closing joint, how far its moving frame is from where the joint wants it:
/// three translation and three rotation components each.
fn loop_residual(recs: &[JointRec]) -> Vec<f64> {
    let placed = placements(recs);
    let at = |b: Option<&String>| b.and_then(|b| placed.get(b)).copied().unwrap_or(Rigid::ID);
    let mut out = Vec::new();
    for r in recs.iter().filter(|r| r.closing) {
        let want = at(r.fixed.as_ref()).then_after(&r.fixed_local).then_after(&motion(r.kind, &r.vals));
        let have = at(Some(&r.moving)).then_after(&r.moving_local);
        let e = want.inverse().then_after(&have);
        out.extend(e.t);
        out.extend(e.rotation_vector());
    }
    out
}

/// Levenberg-Marquardt on `f` from `q0` with a forward-difference Jacobian: the values
/// and the residual where it stopped, and whether `cancelled` stopped it. Deterministic:
/// same input, same answer.
fn least_squares(
    q0: &[f64],
    f: impl Fn(&[f64]) -> Vec<f64>,
    cancelled: impl Fn() -> bool,
) -> (Vec<f64>, Vec<f64>, bool) {
    let n = q0.len();
    let mut q = q0.to_vec();
    let mut r = f(&q);
    let cost = |r: &[f64]| r.iter().map(|x| x * x).sum::<f64>();
    let mut lambda = 1e-3;
    for _ in 0..200 {
        if cancelled() {
            return (q, r, true);
        }
        if cost(&r) < 1e-24 {
            break;
        }
        // J (m × n) by forward differences.
        let mut jac: Vec<Vec<f64>> = Vec::with_capacity(n);
        for k in 0..n {
            if cancelled() {
                return (q, r, true);
            }
            let mut qk = q.clone();
            let h = 1e-7 * q.get(k).map_or(1.0, |v| v.abs().max(1.0));
            if let Some(v) = qk.get_mut(k) {
                *v += h;
            }
            let rk = f(&qk);
            jac.push(rk.iter().zip(&r).map(|(a, b)| (a - b) / h).collect());
        }
        // Normal equations (JᵀJ + λ diag) δ = -Jᵀr.
        let jtj: Vec<Vec<f64>> = (0..n).map(|a| (0..n).map(|b| dot_n(jac.get(a), jac.get(b))).collect()).collect();
        let jtr: Vec<f64> =
            (0..n).map(|a| jac.get(a).map_or(0.0, |c| c.iter().zip(&r).map(|(x, y)| x * y).sum())).collect();
        let mut improved = false;
        for _ in 0..30 {
            if cancelled() {
                return (q, r, true);
            }
            let mut m = jtj.clone();
            for (i, row) in m.iter_mut().enumerate() {
                if let Some(d) = row.get_mut(i) {
                    *d += lambda * (*d).max(1e-12);
                }
            }
            let Some(step) = solve_dense(m, jtr.iter().map(|v| -v).collect()) else {
                lambda *= 10.0;
                continue;
            };
            let qn: Vec<f64> = q.iter().zip(&step).map(|(a, b)| a + b).collect();
            let rn = f(&qn);
            if cost(&rn) < cost(&r) {
                q = qn;
                r = rn;
                lambda = (lambda * 0.1).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    (q, r, false)
}

fn dot_n(a: Option<&Vec<f64>>, b: Option<&Vec<f64>>) -> f64 {
    match (a, b) {
        (Some(a), Some(b)) => a.iter().zip(b).map(|(x, y)| x * y).sum(),
        _ => 0.0,
    }
}

/// Gaussian elimination with partial pivoting (`None` when singular).
fn solve_dense(mut m: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| {
            let v = |k: usize| m.get(k).and_then(|r| r.get(c)).map_or(0.0, |x: &f64| x.abs());
            v(i).total_cmp(&v(j))
        })?;
        m.swap(c, p);
        b.swap(c, p);
        let pivot = *m.get(c)?.get(c)?;
        if pivot.abs() < 1e-300 {
            return None;
        }
        let row = m.get(c)?.clone();
        let bc = *b.get(c)?;
        for r in (c + 1)..n {
            let f = m.get(r)?.get(c)? / pivot;
            if let Some(mr) = m.get_mut(r) {
                for (x, y) in mr.iter_mut().zip(&row) {
                    *x -= f * y;
                }
            }
            if let Some(br) = b.get_mut(r) {
                *br -= f * bc;
            }
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        let row = m.get(c)?;
        let s: f64 = ((c + 1)..n).map(|k| row.get(k).copied().unwrap_or(0.0) * x.get(k).copied().unwrap_or(0.0)).sum();
        *x.get_mut(c)? = (b.get(c)? - s) / row.get(c)?;
    }
    Some(x)
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

    #[test]
    fn the_solver_stops_when_cancelled() {
        // M1: least_squares polls the cancel callback in every iteration.
        use std::cell::Cell;
        let polls = Cell::new(0);
        let f = |q: &[f64]| vec![q.first().copied().unwrap_or(0.0) - 1.0, 0.5 * q.get(1).copied().unwrap_or(0.0)];
        let (_, _, stopped) = least_squares(&[5.0, 5.0], f, || {
            polls.set(polls.get() + 1);
            polls.get() > 2
        });
        assert!(stopped && polls.get() == 3);
        let (q, _, stopped) = least_squares(&[5.0, 5.0], f, || false);
        assert!(!stopped && q.first().is_some_and(|x| (x - 1.0).abs() < 1e-6));
    }

    fn norm_diff(a: V3, b: V3) -> f64 {
        gusset_kernel::norm(sub(a, b))
    }
}
