//! Shell, mirror and pattern features: operations that rebuild an existing body.

use super::*;
use gusset_doc::{CombineDef, DirRef, MirrorDef, MoveDef, PatternCircularDef, PatternLinearDef, ShellDef};
use gusset_kernel::{KernelError, Transform};
use gusset_naming::BodyNames;

/// Most instances one pattern may create (the evaluation's work limits also apply,
/// [`crate::EvalLimits`]).
pub(crate) const MAX_INSTANCES: u32 = 500;

/// A tool for [`Evaluator::boolean_tools`]: its shape, names and face count.
pub(crate) struct Tool {
    pub(crate) shape: Shape,
    pub(crate) names: BodyNames,
}

impl Evaluator {
    pub(crate) fn run_shell(
        &self,
        s: &mut State,
        name: &str,
        d: &ShellDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let thickness = eval_expr(name, &d.thickness, Dim::LENGTH, params, units)?;
        if thickness.abs() < 1e-9 {
            return Err(EvalError::new(name, ErrorCode::Expression, "shell thickness is zero")
                .with_hint("enter a wall thickness such as 2 mm (negative grows the walls outward)"));
        }
        let (bi, body) = target_body(s, name, d.body.as_deref())?;
        let mut faces: Vec<u32> = Vec::new();
        for r in &d.faces {
            let Some(fi) = body.names.face_index(r) else {
                let mut e = EvalError::new(name, ErrorCode::MissingReference, format!("face {r} no longer exists"))
                    .with_hint("re-select the face to open");
                e.reference = Some(r.to_string());
                e.suggestion = closest_face(&body.names, r).map(|f| f.to_string());
                return Err(e);
            };
            if !faces.contains(&fi) {
                faces.push(fi);
            }
        }
        if faces.len() >= body.topology.faces.len() {
            return Err(EvalError::new(name, ErrorCode::InvalidReference, "every face of the body would be removed")
                .with_hint("leave at least one face in place"));
        }
        let res = k.shell(&body.shape, &faces, thickness).map_err(|ke| {
            let mut e = kernel_err(name, "shell", ke);
            if e.hint.is_none() {
                e.hint = Some("try a thinner wall, or open a different face".into());
            }
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Shell { feature: name, input: &body.names }, &res.face_origins, &topo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        replace_body(s, bi, &body, res.shape, names, topo);
        Ok(())
    }

    pub(crate) fn run_mirror(
        &self,
        s: &mut State,
        name: &str,
        d: &MirrorDef,
        _units: LengthUnit,
        _params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let (bi, body) = target_body(s, name, d.body.as_deref())?;
        let (origin, normal) = mirror_plane(name, &d.plane, s)?;
        let res = k
            .transform(&body.shape, &Transform::Mirror { origin, normal })
            .map_err(|ke| kernel_err(name, "mirror", ke))?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Mirror { feature: name, input: &body.names }, &res.face_origins, &topo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        if !d.merge {
            s.bodies.push(Arc::new(Body {
                name: name.to_string(),
                feature: name.to_string(),
                shape: res.shape,
                names,
                topology: topo,
                holes: Vec::new(),
            }));
            return Ok(());
        }
        let faces_in = (body.topology.faces.len() + topo.faces.len()) as u64;
        self.charge(s, name, "the mirror", 1, faces_in)?;
        let united =
            self.unite(name, &body.shape, &body.names, &[Tool { shape: res.shape, names }], |_| String::new())?;
        replace_body(s, bi, &body, united.0, united.1, united.2);
        Ok(())
    }

    pub(crate) fn run_pattern_linear(
        &self,
        s: &mut State,
        name: &str,
        d: &PatternLinearDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let count = instance_count(name, &d.count, params, units)?;
        let dir = direction(name, &d.direction, params, units)?;
        let spacing = eval_expr(name, &d.spacing, Dim::LENGTH, params, units)?;
        if spacing.abs() < 1e-9 {
            return Err(EvalError::new(name, ErrorCode::Expression, "pattern spacing is zero")
                .with_hint("enter a distance between instances such as 15 mm"));
        }
        let moves: Vec<Transform> =
            (1..count).map(|i| Transform::Translate(scale(dir, spacing * f64::from(i)))).collect();
        self.run_pattern(s, name, d.body.as_deref(), &moves)
    }

    pub(crate) fn run_pattern_circular(
        &self,
        s: &mut State,
        name: &str,
        d: &PatternCircularDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let count = instance_count(name, &d.count, params, units)?;
        let axis = direction(name, &d.axis, params, units)?;
        let angle = eval_expr(name, &d.angle, Dim::ANGLE, params, units)?;
        if angle.abs() < 1e-9 {
            return Err(EvalError::new(name, ErrorCode::Expression, "pattern angle is zero")
                .with_hint("enter the total angle, for example 360 deg"));
        }
        let origin = length_point(name, d.through.as_ref(), params, units)?;
        // A full turn spaces instances evenly around the circle; a partial angle spans from
        // the first to the last instance.
        let full = angle.abs() >= std::f64::consts::TAU - 1e-9;
        let step = if full { angle / f64::from(count) } else { angle / f64::from(count - 1) };
        let turns: Vec<Transform> =
            (1..count).map(|i| Transform::Rotate { origin, axis, angle: step * f64::from(i) }).collect();
        self.run_pattern(s, name, d.body.as_deref(), &turns)
    }

    /// Copies the target body once per transform and unites all copies with it.
    fn run_pattern(
        &self,
        s: &mut State,
        name: &str,
        body: Option<&str>,
        transforms: &[Transform],
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let (bi, body) = target_body(s, name, body)?;
        let per_copy = body.topology.faces.len() as u64;
        let copies_in = u64::try_from(transforms.len()).unwrap_or(u64::MAX).saturating_add(1);
        self.charge(s, name, "the pattern", transforms.len(), per_copy.saturating_mul(copies_in))?;
        let mut copies = Vec::with_capacity(transforms.len());
        for (i, t) in transforms.iter().enumerate() {
            let inst_no = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let copy = k.transform(&body.shape, t).map_err(|ke| kernel_err(name, "pattern", ke))?;
            let ctopo = k.topology(&copy.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
            let cnames = name_body(
                &OpContext::Instance { feature: name, k: inst_no, input: &body.names },
                &copy.face_origins,
                &ctopo,
            )
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
            copies.push(Tool { shape: copy.shape, names: cnames });
        }
        let (shape, names, topo) =
            self.unite(name, &body.shape, &body.names, &copies, |i| format!("instance {}: ", i + 1))?;
        replace_body(s, bi, &body, shape, names, topo);
        Ok(())
    }

    /// Unites named shapes with a target and names the result (`Modify` with inputs
    /// `[target, tool_1, ...]`). `label(i)` prefixes the error of tool `i`.
    fn unite(
        &self,
        feature: &str,
        a: &Shape,
        a_names: &BodyNames,
        tools: &[Tool],
        label: impl Fn(usize) -> String,
    ) -> Result<(Shape, BodyNames, Topology), EvalError> {
        self.boolean_tools(feature, BoolOp::Union, (a, a_names), tools, "", &|i, ke| {
            let mut e = kernel_err(feature, "union", ke);
            if let Some(i) = i {
                e.message = format!("{}{}", label(i), e.message);
            }
            e.hint.get_or_insert_with(|| "the copies must overlap or touch cleanly; check the spacing".into());
            e
        })
    }

    /// Applies `op` to a named target with all `tools` in one kernel operation
    /// ([`gusset_kernel::Kernel::boolean_many`], review H3) and names the result (`Modify`
    /// with inputs `[target, tool_1, ...]`).
    ///
    /// If that fails, the tools are applied one at a time, so the error says which tool
    /// fails: `on_error(Some(i), ..)` for tool `i`, `on_error(None, ..)` for the single
    /// operation. A result without faces fails with `EmptyResult` and the message `empty`.
    pub(crate) fn boolean_tools(
        &self,
        feature: &str,
        op: BoolOp,
        (target, target_names): (&Shape, &BodyNames),
        tools: &[Tool],
        empty: &str,
        on_error: &dyn Fn(Option<usize>, KernelError) -> EvalError,
    ) -> Result<(Shape, BodyNames, Topology), EvalError> {
        let k = self.kernel.as_ref();
        let shapes: Vec<Shape> = tools.iter().map(|t| t.shape.clone()).collect();
        let named = |res: OpResult, inputs: &[&BodyNames]| -> Result<(Shape, BodyNames, Topology), EvalError> {
            let topo = k.topology(&res.shape).map_err(|ke| kernel_err(feature, "topology", ke))?;
            if topo.faces.is_empty() {
                return Err(EvalError::new(feature, ErrorCode::EmptyResult, empty.to_string()));
            }
            let names = name_body(&OpContext::Modify { feature, inputs }, &res.face_origins, &topo)
                .map_err(|e| EvalError::new(feature, ErrorCode::KernelFailure, e.0))?;
            Ok((res.shape, names, topo))
        };
        let first = match k.boolean_many(op, target, &shapes) {
            Ok(res) => {
                let mut inputs: Vec<&BodyNames> = Vec::with_capacity(tools.len() + 1);
                inputs.push(target_names);
                inputs.extend(tools.iter().map(|t| &t.names));
                return named(res, &inputs);
            }
            Err(ke) => ke,
        };
        // After a crash every shape is gone: trying the tools one by one would only report
        // the stale target instead of the crash (review L8).
        if tools.len() < 2 || first.kind == gusset_kernel::KernelErrorKind::Crashed {
            return Err(on_error(if tools.is_empty() { None } else { Some(0) }, first));
        }
        // Find the tool that fails (or, if the kernel copes one tool at a time, use that).
        let mut cur: (Shape, BodyNames, Topology) = (target.clone(), target_names.clone(), Topology::default());
        for (i, t) in tools.iter().enumerate() {
            if self.is_cancelled() {
                return Err(budget::cancelled(feature));
            }
            let res = k.boolean(op, &cur.0, &t.shape).map_err(|ke| on_error(Some(i), ke))?;
            cur = named(res, &[&cur.1, &t.names])?;
        }
        Ok(cur)
    }
}

/// The body a modifying feature works on: the named body, or the most recent one.
fn target_body(s: &State, feature: &str, target: Option<&str>) -> Result<(usize, Arc<Body>), EvalError> {
    let bi = match target {
        Some(t) => s.bodies.iter().position(|b| b.feature == t).ok_or_else(|| {
            EvalError::new(
                feature,
                ErrorCode::NoTargetBody,
                format!("body '{t}' does not exist (did its feature fail?)"),
            )
        })?,
        None => s.bodies.len().checked_sub(1).ok_or_else(|| {
            EvalError::new(feature, ErrorCode::NoTargetBody, "there is no body to work on")
                .with_hint("create a body first, for example with extrude")
        })?,
    };
    let body = s.bodies.get(bi).cloned().ok_or_else(|| EvalError::new(feature, ErrorCode::NoTargetBody, "no body"))?;
    Ok((bi, body))
}

fn replace_body(s: &mut State, bi: usize, old: &Body, shape: Shape, names: BodyNames, topology: Topology) {
    let nb = Arc::new(Body {
        name: old.name.clone(),
        feature: old.feature.clone(),
        shape,
        names,
        topology,
        holes: old.holes.clone(),
    });
    if let Some(slot) = s.bodies.get_mut(bi) {
        *slot = nb;
    }
}

/// A face of the same feature as `missing` (preferring the same kind) as a suggestion.
pub(crate) fn closest_face(names: &BodyNames, missing: &FaceRef) -> Option<FaceRef> {
    names
        .faces
        .iter()
        .filter(|f| f.feature == missing.feature)
        .min_by_key(|f| (std::mem::discriminant(&f.key) != std::mem::discriminant(&missing.key), f.to_string()))
        .cloned()
}

fn mirror_plane(feature: &str, plane: &PlaneRef, s: &State) -> Result<([f64; 3], [f64; 3]), EvalError> {
    match plane {
        PlaneRef::XY => Ok(([0.0; 3], [0.0, 0.0, 1.0])),
        PlaneRef::XZ => Ok(([0.0; 3], [0.0, 1.0, 0.0])),
        PlaneRef::YZ => Ok(([0.0; 3], [1.0, 0.0, 0.0])),
        PlaneRef::Datum(_) => plane_frame(feature, plane, s).map(|f| (f.origin, f.normal())),
        PlaneRef::Face(r) => {
            for b in s.bodies.iter().rev() {
                let Some(fi) = b.names.face_index(r) else { continue };
                let Some(f) = b.topology.faces.get(fi as usize) else { continue };
                if f.kind != SurfaceKind::Plane {
                    return Err(EvalError::new(
                        feature,
                        ErrorCode::InvalidReference,
                        format!("face {r} is not planar"),
                    )
                    .with_hint("mirror across a flat face or a base plane"));
                }
                let n = normalize(f.normal).unwrap_or([0.0, 0.0, 1.0]);
                return Ok((f.centroid, n));
            }
            let mut e = EvalError::new(feature, ErrorCode::MissingReference, format!("face {r} no longer exists"))
                .with_hint("re-select the mirror plane");
            e.reference = Some(r.to_string());
            Err(e)
        }
    }
}

/// Evaluates a direction to a unit vector.
fn direction(feature: &str, d: &DirRef, params: &ParamValues, units: LengthUnit) -> Result<[f64; 3], EvalError> {
    let v = match d {
        DirRef::X => [1.0, 0.0, 0.0],
        DirRef::Y => [0.0, 1.0, 0.0],
        DirRef::Z => [0.0, 0.0, 1.0],
        DirRef::Vector([x, y, z]) => [
            eval_expr(feature, x, Dim::NONE, params, units)?,
            eval_expr(feature, y, Dim::NONE, params, units)?,
            eval_expr(feature, z, Dim::NONE, params, units)?,
        ],
    };
    normalize(v).ok_or_else(|| {
        EvalError::new(feature, ErrorCode::Expression, "the direction vector has no length")
            .with_hint("use X, Y, Z or a non-zero vector such as [1, 0, 0]")
    })
}

/// Evaluates a pattern's instance count: a whole number from 2 to [`MAX_INSTANCES`].
fn instance_count(
    feature: &str,
    e: &gusset_units::expr::Expr,
    params: &ParamValues,
    units: LengthUnit,
) -> Result<u32, EvalError> {
    let v = eval_expr(feature, e, Dim::NONE, params, units)?;
    if !v.is_finite() || (v - v.round()).abs() > 1e-6 {
        return Err(EvalError::new(feature, ErrorCode::Expression, format!("'{e}' is {v}, not a whole number"))
            .with_hint("the count of instances must be a whole number"));
    }
    let n = v.round();
    if n < 2.0 || n > f64::from(MAX_INSTANCES) {
        return Err(EvalError::new(
            feature,
            ErrorCode::Expression,
            format!("a pattern needs between 2 and {MAX_INSTANCES} instances, not {n}"),
        )
        .with_hint("the count includes the original"));
    }
    Ok(n as u32)
}

impl Evaluator {
    pub(crate) fn run_move(
        &self,
        s: &mut State,
        name: &str,
        d: &MoveDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let (bi, body) = target_body(s, name, d.body.as_deref())?;
        // A turn about the axis comes first, then the translation along the world axes.
        let mut steps: Vec<Transform> = Vec::new();
        if let Some(r) = &d.rotation {
            let axis = direction(name, &r.axis, params, units)?;
            let angle = eval_expr(name, &r.angle, Dim::ANGLE, params, units)?;
            let origin = length_point(name, r.through.as_ref(), params, units)?;
            steps.push(Transform::Rotate { origin, axis, angle });
        }
        if d.translate.is_some() {
            steps.push(Transform::Translate(length_point(name, d.translate.as_ref(), params, units)?));
        }
        let mut res: Option<OpResult> = None;
        for t in &steps {
            let from = res.as_ref().map_or(&body.shape, |r| &r.shape);
            res = Some(k.transform(from, t).map_err(|ke| kernel_err(name, "move", ke))?);
        }
        // Each step keeps the face order, so the last result's history refers to the
        // original faces.
        let res = res.ok_or_else(|| {
            EvalError::new(name, ErrorCode::Expression, "the move does nothing")
                .with_hint("give a translate, or an axis and an angle")
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        if d.copy {
            let names = name_body(&OpContext::Copy { feature: name, input: &body.names }, &res.face_origins, &topo)
                .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
            s.bodies.push(Arc::new(Body {
                name: name.to_string(),
                feature: name.to_string(),
                shape: res.shape,
                names,
                topology: topo,
                holes: Vec::new(),
            }));
        } else {
            // A rigid move changes where the faces are, not what they are: names stay.
            let names =
                name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
                    .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
            replace_body(s, bi, &body, res.shape, names, topo);
        }
        Ok(())
    }

    /// Combines the tool bodies into the target (in one kernel operation for union and cut)
    /// and removes the tools.
    pub(crate) fn run_combine(&self, s: &mut State, name: &str, d: &CombineDef) -> Result<(), EvalError> {
        let mut tools: Vec<Arc<Body>> = Vec::new();
        for t in &d.tools {
            let b = s.bodies.iter().find(|b| &b.feature == t).cloned().ok_or_else(|| {
                EvalError::new(name, ErrorCode::NoTargetBody, format!("tool body '{t}' does not exist"))
                    .with_hint("name the feature that created the body; it may have been combined already")
            })?;
            tools.push(b);
        }
        let is_tool = |b: &Body| d.tools.contains(&b.feature);
        let bi = match d.body.as_deref() {
            Some(t) => s.bodies.iter().position(|b| b.feature == t).ok_or_else(|| {
                EvalError::new(
                    name,
                    ErrorCode::NoTargetBody,
                    format!("body '{t}' does not exist (did its feature fail?)"),
                )
            })?,
            None => s.bodies.iter().rposition(|b| !is_tool(b)).ok_or_else(|| {
                EvalError::new(name, ErrorCode::NoTargetBody, "there is no body to combine the tools into")
                    .with_hint("name the target body with body:")
            })?,
        };
        let body =
            s.bodies.get(bi).cloned().ok_or_else(|| EvalError::new(name, ErrorCode::NoTargetBody, "no target body"))?;
        if is_tool(&body) {
            return Err(EvalError::new(name, ErrorCode::InvalidReference, "the target is also a tool")
                .with_hint("a body cannot be combined with itself"));
        }
        let (bop, verb) = match d.op {
            BodyOp::Cut => (BoolOp::Cut, "cut"),
            BodyOp::Intersect => (BoolOp::Intersect, "intersect"),
            BodyOp::Add | BodyOp::New => (BoolOp::Union, "union"),
        };
        let faces_in =
            tools.iter().map(|t| t.topology.faces.len() as u64).sum::<u64>() + body.topology.faces.len() as u64;
        self.charge(s, name, "the combination", tools.len(), faces_in)?;
        let tool_list: Vec<Tool> =
            tools.iter().map(|t| Tool { shape: t.shape.clone(), names: t.names.clone() }).collect();
        let on_error = |i: Option<usize>, ke: KernelError| {
            let mut e = kernel_err(name, verb, ke);
            if let Some(t) = i.and_then(|i| tools.get(i)) {
                e.message = format!("{} (tool body '{}')", e.message, t.name);
            }
            if bop != BoolOp::Union {
                e.hint.get_or_insert_with(|| "check that the tool body overlaps the target".into());
            }
            e
        };
        let (shape, names, topo) = self
            .boolean_tools(
                name,
                bop,
                (&body.shape, &body.names),
                &tool_list,
                "combining the tool bodies left no body",
                &on_error,
            )
            .map_err(|e| {
                if e.code != ErrorCode::EmptyResult {
                    return e;
                }
                e.with_hint(if bop == BoolOp::Intersect {
                    "the tool bodies must overlap the target to intersect"
                } else {
                    "the tool bodies remove the whole target"
                })
            })?;
        // Only a union adds the tools' holes; a cut or intersection keeps the target's
        // (review L1). Records whose wall is gone are dropped after every feature.
        let tool_holes: Vec<_> = if bop == BoolOp::Union {
            tools.iter().flat_map(|t| t.holes.iter().cloned()).collect()
        } else {
            Vec::new()
        };
        replace_body(s, bi, &body, shape, names, topo);
        if !tool_holes.is_empty()
            && let Some(slot) = s.bodies.get_mut(bi)
        {
            let mut b = Body {
                name: slot.name.clone(),
                feature: slot.feature.clone(),
                shape: slot.shape.clone(),
                names: slot.names.clone(),
                topology: slot.topology.clone(),
                holes: slot.holes.clone(),
            };
            b.holes.extend(tool_holes);
            *slot = Arc::new(b);
        }
        s.bodies.retain(|b| !is_tool(b));
        Ok(())
    }
}

/// Three length expressions as a point in mm; `None` = the origin.
fn length_point(
    feature: &str,
    p: Option<&[gusset_units::expr::Expr; 3]>,
    params: &ParamValues,
    units: LengthUnit,
) -> Result<[f64; 3], EvalError> {
    let mut out = [0.0; 3];
    if let Some(p) = p {
        for (slot, e) in out.iter_mut().zip(p) {
            *slot = eval_expr(feature, e, Dim::LENGTH, params, units)?;
        }
    }
    Ok(out)
}
