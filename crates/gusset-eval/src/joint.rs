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

    pub(crate) fn run_joint(
        &self,
        s: &mut State,
        name: &str,
        d: &JointDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let (moving, _) = self.joint_frame(s, name, &d.at)?;
        let (mut fixed, fixed_body) = self.joint_frame(s, name, &d.to)?;
        if let Some(o) = &d.offset {
            let off = eval_expr(name, o, Dim::LENGTH, params, units)?;
            fixed = fixed.then_after(&Rigid::translation([0.0, 0.0, off]));
        }
        if d.flip {
            fixed = fixed.then_after(&Rigid::rotation([1.0, 0.0, 0.0], std::f64::consts::PI));
        }
        let vals = Self::joint_values(name, d, units, params)?;
        let placed = |b: &str| s.joints.placed.get(b).copied().unwrap_or(Rigid::ID);
        let fixed_place = fixed_body.as_deref().map_or(Rigid::ID, placed);
        // Frames in their bodies' coordinates before any joint moved them, so the loop
        // solver can place every body again from the joint values alone.
        let rec = JointRec {
            name: name.to_string(),
            kind: d.kind,
            moving: d.body.clone(),
            with: d.with.clone(),
            fixed: fixed_body.filter(|b| *b != d.body),
            fixed_local: fixed_place.inverse().then_after(&fixed),
            moving_local: placed(&d.body).inverse().then_after(&moving),
            free: d.free.iter().flat_map(|v| Slot::of(v)).collect(),
            closing: s.joints.recs.iter().any(|r| !r.closing && (r.moving == d.body || r.with.contains(&d.body))),
            vals,
        };
        let closing = rec.closing;
        Arc::make_mut(&mut s.joints).recs.push(rec);
        if !closing {
            vals.check(name)?;
            let t = fixed.then_after(&motion(d.kind, &vals)).then_after(&moving.inverse());
            for b in std::iter::once(&d.body).chain(&d.with) {
                self.move_body(s, name, b, &t)?;
            }
            return Ok(());
        }
        self.close_loops(s, name)
    }

    /// Solves the free values of every joint so far so that the closing joints meet, then
    /// moves the bodies to the solution (ADR 0026 §1, closed loops).
    fn close_loops(&self, s: &mut State, name: &str) -> Result<(), EvalError> {
        let recs = s.joints.recs.clone();
        let unknowns: Vec<(usize, Slot)> =
            recs.iter().enumerate().flat_map(|(i, r)| r.free.iter().map(move |&f| (i, f))).collect();
        let q0: Vec<f64> = unknowns.iter().map(|&(i, f)| recs.get(i).map_or(0.0, |r| f.get(&r.vals))).collect();
        let with_values = |q: &[f64]| {
            let mut rs = recs.clone();
            for (&(i, f), &v) in unknowns.iter().zip(q) {
                if let Some(r) = rs.get_mut(i) {
                    f.set(&mut r.vals, v);
                }
            }
            rs
        };
        let gap = |q: &[f64]| loop_residual(&with_values(q));
        let (q, r) = if unknowns.is_empty() { (q0.clone(), gap(&q0)) } else { least_squares(&q0, gap) };
        let (moved, turned) = r.chunks(6).fold((0.0_f64, 0.0_f64), |(m, t), c| {
            let n = |a: &[f64]| a.iter().map(|x| x * x).sum::<f64>().sqrt();
            (m.max(n(c.get(..3).unwrap_or(&[]))), t.max(n(c.get(3..).unwrap_or(&[]))))
        });
        if moved > LOOP_TOL_MM || turned > LOOP_TOL_RAD {
            let first = recs.iter().find(|r| {
                recs.last().is_some_and(|c| !r.closing && (r.moving == c.moving || r.with.contains(&c.moving)))
            });
            let msg = format!("the joint loop cannot close: {moved:.4} mm and {:.4}° apart", turned.to_degrees());
            let hint = if unknowns.is_empty() {
                format!(
                    "'{}' is already placed by {}: list the values the solver may choose in free, e.g. free: [angle]",
                    recs.last().map_or("", |c| c.moving.as_str()),
                    first.map_or("an earlier joint", |r| r.name.as_str())
                )
            } else {
                "check the frames and lengths: the mechanism cannot be assembled like this (or start from other values)"
                    .into()
            };
            return Err(EvalError::new(name, ErrorCode::InvalidReference, msg).with_hint(hint));
        }
        let solved = with_values(&q);
        for r in &solved {
            r.vals.check(&r.name)?;
        }
        // Move every body from where it is to where the solution puts it.
        let target = placements(&solved);
        for (body, p) in &target {
            let now = s.joints.placed.get(body).copied().unwrap_or(Rigid::ID);
            let delta = p.then_after(&now.inverse());
            self.move_body(s, name, body, &delta)?;
        }
        let js = Arc::make_mut(&mut s.joints);
        js.recs = solved;
        js.placed = target;
        Ok(())
    }

    /// Moves body `body` (its creating feature) rigidly by `t` and records its placement.
    fn move_body(&self, s: &mut State, name: &str, body: &str, t: &Rigid) -> Result<(), EvalError> {
        let (axis, angle) = t.axis_angle();
        let mut steps = Vec::new();
        if angle != 0.0 {
            steps.push(Transform::Rotate { origin: [0.0; 3], axis, angle });
        }
        // The translation after the rotation about the world origin.
        if dot(t.t, t.t) > 0.0 {
            steps.push(Transform::Translate(t.t));
        }
        let now = s.joints.placed.get(body).copied().unwrap_or(Rigid::ID);
        Arc::make_mut(&mut s.joints).placed.insert(body.to_string(), t.then_after(&now));
        if steps.is_empty() {
            return Ok(());
        }
        let k = self.kernel.as_ref();
        let (bi, b) = ops::target_body(s, name, Some(body))?;
        let mut res: Option<gusset_kernel::OpResult> = None;
        for step in &steps {
            let from = res.as_ref().map_or(&b.shape, |r| &r.shape);
            res = Some(k.transform(from, step).map_err(|ke| kernel_err(name, "joint", ke))?);
        }
        let Some(res) = res else { return Ok(()) };
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        // A rigid placement changes where the faces are, not what they are: names stay.
        let names = gusset_naming::name_body(
            &gusset_naming::OpContext::Modify { feature: name, inputs: &[&b.names] },
            &res.face_origins,
            &topo,
        )
        .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        ops::replace_body(s, bi, &b, res.shape, names, topo);
        Ok(())
    }
}

/// Gaps the loop solver accepts.
const LOOP_TOL_MM: f64 = 1e-6;
const LOOP_TOL_RAD: f64 = 1e-8;

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
    fn check(&self, name: &str) -> Result<(), EvalError> {
        for (v, l, what) in
            [(self.angle, self.angle_limits, "angle"), (self.distance, self.distance_limits, "distance")]
        {
            if let Some((lo, hi)) = l
                && (v < lo - 1e-12 || v > hi + 1e-12)
            {
                return Err(EvalError::new(
                    name,
                    ErrorCode::Expression,
                    format!("the {what} is outside the joint's limits"),
                )
                .with_hint(format!("keep it between the limits ({lo} and {hi} in base units: radians or mm)")));
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
/// and the residual where it stopped. Deterministic: same input, same answer.
fn least_squares(q0: &[f64], f: impl Fn(&[f64]) -> Vec<f64>) -> (Vec<f64>, Vec<f64>) {
    let n = q0.len();
    let mut q = q0.to_vec();
    let mut r = f(&q);
    let cost = |r: &[f64]| r.iter().map(|x| x * x).sum::<f64>();
    let mut lambda = 1e-3;
    for _ in 0..200 {
        if cost(&r) < 1e-24 {
            break;
        }
        // J (m × n) by forward differences.
        let mut jac: Vec<Vec<f64>> = Vec::with_capacity(n);
        for k in 0..n {
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
    (q, r)
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

    fn norm_diff(a: V3, b: V3) -> f64 {
        gusset_kernel::norm(sub(a, b))
    }
}
