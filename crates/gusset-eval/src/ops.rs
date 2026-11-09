//! Shell, draft, mirror and pattern features: operations that rebuild an existing body.

use super::*;
use gusset_doc::{
    CombineDef, DirRef, DraftDef, MirrorDef, MoveDef, MoveFaceDef, MoveRotation, OffsetFaceDef, PatternCircularDef,
    PatternLinearDef, ShellDef,
};
use gusset_kernel::{DraftSpec, FaceOrigin, KernelError, Transform};
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

    /// Moves faces along their normals (ADR 0030).
    pub(crate) fn run_offset_face(
        &self,
        s: &mut State,
        name: &str,
        d: &OffsetFaceDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let distance = eval_expr(name, &d.distance, Dim::LENGTH, params, units)?;
        if distance.abs() < 1e-9 {
            return Err(EvalError::new(name, ErrorCode::Expression, "the offset distance is zero")
                .with_hint("enter how far to move the faces, such as 2 mm (negative moves them in)"));
        }
        let (bi, body) = target_body(s, name, d.body.as_deref())?;
        let faces = resolve_faces(name, &body, &d.faces, "re-select the face to move")?;
        let res = if d.rules {
            k.offset_faces(&body.shape, &faces, distance)
        } else {
            k.offset_faces_only(&body.shape, &faces, distance)
        };
        let res = res.map_err(|ke| {
            let mut e = kernel_err(name, "offset face", ke);
            if e.hint.is_none() {
                e.hint = Some("move the faces less far, or fewer faces at once".into());
            }
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        replace_body(s, bi, &body, res.shape, names, topo);
        Ok(())
    }

    /// Removes faces; the faces around close the gap (ADR 0038).
    pub(crate) fn run_delete_face(
        &self,
        s: &mut State,
        name: &str,
        d: &gusset_doc::DeleteFaceDef,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let (bi, body) = target_body(s, name, d.body.as_deref())?;
        let faces = resolve_faces(name, &body, &d.faces, "re-select the face to delete")?;
        let res = k.delete_faces(&body.shape, &faces).map_err(|ke| {
            let mut e = kernel_err(name, "delete face", ke);
            if e.hint.is_none() {
                e.hint = Some("delete whole features' faces together (all faces of a hole or a fillet)".into());
            }
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        replace_body(s, bi, &body, res.shape, names, topo);
        Ok(())
    }

    /// Moves faces rigidly (ADR 0031); the faces around stretch to follow.
    pub(crate) fn run_move_face(
        &self,
        s: &mut State,
        name: &str,
        d: &MoveFaceDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let mut steps = motion_steps(name, d.translate.as_ref(), d.rotation.as_ref(), params, units)?;
        let (bi, body) = target_body(s, name, d.body.as_deref())?;
        let mut faces = resolve_faces(name, &body, &d.faces, "re-select the face to move")?;
        if let Some(target) = &d.onto {
            steps = onto_steps(name, s, &body, &faces, target)?;
            if let [Transform::Rotate { origin, axis, angle }] = steps.as_slice() {
                return self.tilt_faces(s, name, (bi, &body), &faces, (*origin, *axis, *angle));
            }
        }
        if d.rules {
            let extra = rule_faces(&body.topology, &faces);
            faces.extend(extra);
        }
        // A zero move (a parameter at 0) leaves the body as it is.
        steps.retain(|t| match t {
            Transform::Translate(v) => v.iter().any(|c| c.abs() > 1e-9),
            Transform::Rotate { angle, .. } => angle.abs() > 1e-12,
            Transform::Mirror { .. } => true,
        });
        if steps.is_empty() {
            return Ok(());
        }
        let res = k.move_faces(&body.shape, &faces, &steps).map_err(|ke| {
            let mut e = kernel_err(name, "move face", ke);
            if d.onto.is_some() {
                e.hint = Some(
                    "laying a face onto a plane where a face around it would vanish (flush) is not supported: \
                     extrude up to it and combine instead"
                        .into(),
                );
            } else if e.hint.is_none() {
                e.hint = Some("move the faces less far, or together with the faces around them".into());
            }
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
            .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
        replace_body(s, bi, &body, res.shape, names, topo);
        Ok(())
    }

    /// Turns planar faces about a line in their plane (`move_face onto` a tilted plane):
    /// a draft whose pull lies in the faces, across the hinge, so the faces around are
    /// re-cut against the turned plane instead of carried along rigidly.
    fn tilt_faces(
        &self,
        s: &mut State,
        name: &str,
        (bi, body): (usize, &Body),
        faces: &[u32],
        (origin, axis, angle): ([f64; 3], [f64; 3], f64),
    ) -> Result<(), EvalError> {
        use gusset_kernel::{cross, dot, norm};
        let k = self.kernel.as_ref();
        let first = faces.first().and_then(|f| body.topology.faces.get(*f as usize));
        let n1 = first.map(|f| f.normal).unwrap_or([0.0, 0.0, 1.0]);
        // Where the normal must end: n1 turned by `angle` about `axis`.
        let want = turn(n1, axis, angle);
        let pull = cross(axis, n1);
        let fail = |ke| {
            let mut e = kernel_err(name, "move face", ke);
            e.hint = Some("tilt the face less, or onto a plane that crosses it nearer".into());
            e
        };
        // The sign of a draft depends on which side the material is: try one, check the
        // turned face's normal, else the other.
        let mut last = None;
        for a in [angle, -angle] {
            let res = match k.draft(&body.shape, faces, &DraftSpec { pull, neutral: origin, angle: a }) {
                Ok(r) => r,
                Err(ke) => {
                    last = Some(ke);
                    continue;
                }
            };
            let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
            let turned = res.face_origins.iter().zip(&topo.faces).find_map(|(o, f)| {
                o.iter()
                    .any(|x| matches!(x, FaceOrigin::Input { face, .. } if Some(face) == faces.first()))
                    .then_some(f.normal)
            });
            if turned.is_some_and(|n| norm(cross(n, want)) < 1e-6 && dot(n, want) > 0.0) {
                let names =
                    name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
                        .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                replace_body(s, bi, body, res.shape, names, topo);
                return Ok(());
            }
        }
        Err(match last {
            Some(ke) => fail(ke),
            None => EvalError::new(name, ErrorCode::KernelFailure, "the face could not be turned onto the plane")
                .with_hint("tilt the face less, or onto a plane that crosses it nearer"),
        })
    }

    /// Tapers faces about the neutral plane (ADR 0028). The pull direction is the plane's
    /// normal, or into the body for a face of it; `flip` reverses it.
    pub(crate) fn run_draft(
        &self,
        s: &mut State,
        name: &str,
        d: &DraftDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let angle = eval_expr(name, &d.angle, Dim::ANGLE, params, units)?;
        if angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-9 {
            return Err(EvalError::new(
                name,
                ErrorCode::Expression,
                "the draft angle must be between -90 and 90 degrees",
            )
            .with_hint("molded parts typically use 1 to 3 deg; negative leans the faces outward"));
        }
        let (bi, body) = target_body(s, name, d.body.as_deref())?;
        let faces = resolve_faces(name, &body, &d.faces, "re-select the face to draft")?;
        if faces.is_empty() {
            return Err(EvalError::new(name, ErrorCode::InvalidReference, "no faces to draft")
                .with_hint("select the side faces to taper"));
        }
        // No taper (a parameter at 0, F4-9) leaves the body as it is.
        if angle.abs() <= 1e-9 {
            return Ok(());
        }
        let (neutral, normal) = mirror_plane(name, &d.plane, s)?;
        let into = matches!(d.plane, gusset_doc::PlaneRef::Face(_));
        let pull = if into != d.flip { scale(normal, -1.0) } else { normal };
        let res = k.draft(&body.shape, &faces, &DraftSpec { pull, neutral, angle }).map_err(|mut ke| {
            ke.message = name_face_index(&ke.message, &body);
            let mut e = kernel_err(name, "draft", ke);
            if e.hint.is_none() {
                e.hint = Some("draft side faces that cross the neutral plane, with a smaller angle".into());
            }
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
        let names = name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
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
                sheet: None,
            }));
            return Ok(());
        }
        let faces_in = (body.topology.faces.len() + topo.faces.len()) as u64;
        self.charge(s, name, "the mirror", 1, faces_in)?;
        let united = self.unite(
            name,
            &body.shape,
            &body.names,
            &[Tool { shape: res.shape, names }],
            |_| String::new(),
            d.refine,
        )?;
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
        if !d.features.is_empty() {
            return self.run_feature_pattern(s, name, &d.features, &moves, d.refine);
        }
        self.run_pattern(s, name, d.body.as_deref(), &moves, d.refine)
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
        if !d.features.is_empty() {
            return self.run_feature_pattern(s, name, &d.features, &turns, d.refine);
        }
        self.run_pattern(s, name, d.body.as_deref(), &turns, d.refine)
    }

    /// Repeats features instead of a body: what each one removed from its body (before minus
    /// after) and added to it (after minus before) is copied once per transform, the removed
    /// copies are cut from that body and the added ones united with it.
    fn run_feature_pattern(
        &self,
        s: &mut State,
        name: &str,
        features: &[String],
        transforms: &[Transform],
        refine: bool,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let mut target: Option<String> = None;
        let (mut removed, mut added) = (Vec::new(), Vec::new());
        for f in features {
            let (before, after) = s.effects.get(f).ok_or_else(|| {
                EvalError::new(name, ErrorCode::InvalidReference, format!("{f} changed no body"))
                    .with_hint("a feature pattern repeats features that add to or cut from one body")
            })?;
            let Some(before) = before else {
                return Err(EvalError::new(name, ErrorCode::InvalidReference, format!("{f} made a new body"))
                    .with_hint("repeat that body with a pattern of the body instead"));
            };
            match &target {
                Some(t) if *t != after.feature => {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::InvalidReference,
                        "the features change different bodies",
                    )
                    .with_hint("make one pattern per body"));
                }
                _ => target = Some(after.feature.clone()),
            }
            // The material each feature took away and the material it added.
            for (a, b, into) in [(&before, &after, &mut removed), (&after, &before, &mut added)] {
                // An empty difference (a hole adds nothing) is reported as an error by the
                // kernel: nothing to repeat on that side.
                let res = match k.boolean(BoolOp::Cut, &a.shape, &b.shape) {
                    Ok(r) => r,
                    Err(ke) if ke.message.contains("removed all material") => continue,
                    Err(ke) => return Err(kernel_err(name, "pattern", ke)),
                };
                let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
                if topo.faces.is_empty() {
                    continue;
                }
                let names = name_body(
                    &OpContext::Modify { feature: name, inputs: &[&a.names, &b.names] },
                    &res.face_origins,
                    &topo,
                )
                .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                into.push(Tool { shape: res.shape, names });
            }
        }
        let Some(target) = target else { return Ok(()) };
        let (bi, body) = target_body(s, name, Some(&target))?;
        let copies = |tools: &[Tool]| -> Result<Vec<Tool>, EvalError> {
            let mut out = Vec::new();
            for (i, t) in transforms.iter().enumerate() {
                let inst_no = u32::try_from(i + 1).unwrap_or(u32::MAX);
                for tool in tools {
                    let c = k.transform(&tool.shape, t).map_err(|ke| kernel_err(name, "pattern", ke))?;
                    let topo = k.topology(&c.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
                    let names = name_body(
                        &OpContext::Instance { feature: name, k: inst_no, input: &tool.names },
                        &c.face_origins,
                        &topo,
                    )
                    .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                    out.push(Tool { shape: c.shape, names });
                }
            }
            Ok(out)
        };
        let (cuts, adds) = (copies(&removed)?, copies(&added)?);
        let per = body.topology.faces.len() as u64;
        self.charge(s, name, "the pattern", cuts.len() + adds.len(), per)?;
        let (mut shape, mut names, mut topo) = (body.shape.clone(), body.names.clone(), body.topology.clone());
        if !cuts.is_empty() {
            (shape, names, topo) = self.boolean_tools_refined(
                name,
                BoolOp::Cut,
                (&shape, &names),
                &cuts,
                "the pattern removes the whole body",
                &|i, ke| {
                    let mut e = kernel_err(name, "cut", ke);
                    if let Some(i) = i {
                        e.message = format!("copy {}: {}", i + 1, e.message);
                    }
                    e
                },
                refine,
            )?;
        }
        if !adds.is_empty() {
            (shape, names, topo) = self.unite(name, &shape, &names, &adds, |i| format!("copy {}: ", i + 1), refine)?;
        }
        replace_body(s, bi, &body, shape, names, topo);
        Ok(())
    }

    /// Copies the target body once per transform and unites all copies with it.
    fn run_pattern(
        &self,
        s: &mut State,
        name: &str,
        body: Option<&str>,
        transforms: &[Transform],
        refine: bool,
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
            self.unite(name, &body.shape, &body.names, &copies, |i| format!("instance {}: ", i + 1), refine)?;
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
        refine: bool,
    ) -> Result<(Shape, BodyNames, Topology), EvalError> {
        self.boolean_tools_refined(
            feature,
            BoolOp::Union,
            (a, a_names),
            tools,
            "",
            &|i, ke| {
                let mut e = kernel_err(feature, "union", ke);
                if let Some(i) = i {
                    e.message = format!("{}{}", label(i), e.message);
                }
                e.hint.get_or_insert_with(|| "the copies must overlap or touch cleanly; check the spacing".into());
                e
            },
            refine,
        )
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
        target: (&Shape, &BodyNames),
        tools: &[Tool],
        empty: &str,
        on_error: &dyn Fn(Option<usize>, KernelError) -> EvalError,
    ) -> Result<(Shape, BodyNames, Topology), EvalError> {
        self.boolean_tools_refined(feature, op, target, tools, empty, on_error, true)
    }

    /// [`Self::boolean_tools`] whose result is unified (`refine`) or kept as the boolean
    /// made it (`refine: false`, ADR 0004).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn boolean_tools_refined(
        &self,
        feature: &str,
        op: BoolOp,
        (target, target_names): (&Shape, &BodyNames),
        tools: &[Tool],
        empty: &str,
        on_error: &dyn Fn(Option<usize>, KernelError) -> EvalError,
        refine: bool,
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
        let first = match if refine {
            k.boolean_many(op, target, &shapes)
        } else {
            k.boolean_many_unrefined(op, target, &shapes)
        } {
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
            let res = if refine { k.boolean(op, &cur.0, &t.shape) } else { k.boolean_unrefined(op, &cur.0, &t.shape) }
                .map_err(|ke| on_error(Some(i), ke))?;
            cur = named(res, &[&cur.1, &t.names])?;
        }
        Ok(cur)
    }
}

/// The body a modifying feature works on: the named body, or the most recent one.
pub(crate) fn target_body(s: &State, feature: &str, target: Option<&str>) -> Result<(usize, Arc<Body>), EvalError> {
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

pub(crate) fn rebuilt_body(old: &Body, shape: Shape, names: BodyNames, topology: Topology) -> Arc<Body> {
    Arc::new(Body {
        name: old.name.clone(),
        feature: old.feature.clone(),
        shape,
        names,
        topology,
        holes: old.holes.clone(),
        sheet: old.sheet.clone(),
    })
}

pub(crate) fn replace_body(s: &mut State, bi: usize, old: &Body, shape: Shape, names: BodyNames, topology: Topology) {
    let nb = rebuilt_body(old, shape, names, topology);
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
        let steps = motion_steps(name, d.translate.as_ref(), d.rotation.as_ref(), params, units)?;
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
                sheet: None,
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
            .boolean_tools_refined(
                name,
                bop,
                (&body.shape, &body.names),
                &tool_list,
                "combining the tool bodies left no body",
                &on_error,
                d.refine,
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
                sheet: slot.sheet.clone(),
            };
            b.holes.extend(tool_holes);
            *slot = Arc::new(b);
        }
        s.bodies.retain(|b| !is_tool(b));
        Ok(())
    }
}

/// The kernel names faces by index ("face 2 cannot be drafted"); say which face that is.
fn name_face_index(message: &str, body: &Body) -> String {
    let Some(at) = message.find("face ") else { return message.to_string() };
    let (head, rest) = message.split_at(at + "face ".len());
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let face = digits.parse::<usize>().ok().and_then(|i| body.names.faces.get(i));
    match face {
        Some(f) => format!("{head}{f}{}", rest.get(digits.len()..).unwrap_or("")),
        None => message.to_string(),
    }
}

/// A turn about the axis first, then the translation along the world axes.
fn motion_steps(
    feature: &str,
    translate: Option<&[gusset_units::expr::Expr; 3]>,
    rotation: Option<&MoveRotation>,
    params: &ParamValues,
    units: LengthUnit,
) -> Result<Vec<Transform>, EvalError> {
    let mut steps: Vec<Transform> = Vec::new();
    if let Some(r) = rotation {
        let axis = direction(feature, &r.axis, params, units)?;
        let angle = eval_expr(feature, &r.angle, Dim::ANGLE, params, units)?;
        let origin = length_point(feature, r.through.as_ref(), params, units)?;
        steps.push(Transform::Rotate { origin, axis, angle });
    }
    if translate.is_some() {
        steps.push(Transform::Translate(length_point(feature, translate, params, units)?));
    }
    Ok(steps)
}

/// The motion that lays the (planar) faces `faces` of `body` onto the plane of `target`, a
/// planar face of any body: a translation along the normal when parallel, else a turn
/// about the line where the two planes meet.
fn onto_steps(
    name: &str,
    s: &State,
    body: &Body,
    faces: &[u32],
    target: &gusset_doc::FaceRef,
) -> Result<Vec<Transform>, EvalError> {
    use gusset_kernel::{SurfaceKind, add, cross, dot, norm, scale, sub};
    let bad = |m: String| EvalError::new(name, ErrorCode::InvalidReference, m);
    let plane = |b: &Body, fi: u32| -> Option<([f64; 3], [f64; 3])> {
        let f = b.topology.faces.get(fi as usize)?;
        matches!(f.kind, SurfaceKind::Plane).then_some((f.centroid, f.normal))
    };
    let first = *faces.first().ok_or_else(|| bad("no faces to move".into()))?;
    let (c1, n1) = plane(body, first)
        .ok_or_else(|| bad("onto lays planar faces only".into()).with_hint("pick a flat face to move"))?;
    for f in faces {
        let (c, n) = plane(body, *f).ok_or_else(|| bad("onto lays planar faces only".into()))?;
        if norm(cross(n, n1)) > 1e-9 || dot(sub(c, c1), n1).abs() > 1e-6 {
            return Err(bad("the faces to lay onto a plane must lie in one plane".into()));
        }
    }
    let (c2, n2) =
        s.bodies.iter().find_map(|b| b.names.face_index(target).and_then(|fi| plane(b, fi))).ok_or_else(|| {
            bad(format!("{target} is not a planar face of the model"))
                .with_hint("pick a flat face to lay the faces onto")
        })?;
    let along = cross(n1, n2);
    if norm(along) <= 1e-9 {
        return Ok(vec![Transform::Translate(scale(n2, dot(sub(c2, c1), n2)))]);
    }
    if dot(n1, n2).abs() < 1e-6 {
        return Err(bad(format!("{target} is perpendicular to the faces to move"))
            .with_hint("onto turns a face at most short of a right angle; pick a face it can tilt onto"));
    }
    // A tilted plane: turn the faces about the line where their plane meets the target's,
    // by the smaller angle that makes them parallel (the face keeps its side: its normal
    // ends along n2 or -n2, whichever is nearer).
    let t = if dot(n1, n2) >= 0.0 { n2 } else { scale(n2, -1.0) };
    let u = cross(n1, t);
    let uu = dot(u, u);
    let (d1, d2) = (dot(c1, n1), dot(c2, t));
    // The point of the intersection line nearest the origin.
    let origin = scale(add(scale(cross(t, u), d1), scale(cross(u, n1), d2)), 1.0 / uu);
    let angle = libm::atan2(libm::sqrt(uu), dot(n1, t));
    Ok(vec![Transform::Rotate { origin, axis: scale(u, 1.0 / libm::sqrt(uu)), angle }])
}

/// `v` turned by `angle` (radians) about the unit vector `axis` (Rodrigues).
fn turn(v: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    use gusset_kernel::{add, cross, dot, scale};
    let (s, c) = (libm::sin(angle), libm::cos(angle));
    add(add(scale(v, c), scale(cross(axis, v), s)), scale(axis, dot(axis, v) * (1.0 - c)))
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

/// The indices of `refs` in `body` (each once), or the missing reference with a suggestion.
pub(crate) fn resolve_faces(
    name: &str,
    body: &Body,
    refs: &[gusset_doc::FaceRef],
    hint: &str,
) -> Result<Vec<u32>, EvalError> {
    let mut faces: Vec<u32> = Vec::new();
    for r in refs {
        let Some(fi) = body.names.face_index(r) else {
            let mut e =
                EvalError::new(name, ErrorCode::MissingReference, format!("face {r} no longer exists")).with_hint(hint);
            e.reference = Some(r.to_string());
            e.suggestion = closest_face(&body.names, r).map(|f| f.to_string());
            return Err(e);
        };
        if !faces.contains(&fi) {
            faces.push(fi);
        }
    }
    Ok(faces)
}

/// Faces that move along with `faces` under the live rules of `move_face` (ADR 0031
/// amendment): planar faces in the same plane (same direction and offset) as a moved
/// planar one, and cylinders or cones on the same axis as a moved cylinder or cone.
/// Returns only the extra faces, in face order.
pub fn rule_faces(topo: &gusset_kernel::Topology, faces: &[u32]) -> Vec<u32> {
    use gusset_kernel::{SurfaceKind as K, cross, dot, norm, sub};
    const TOL: f64 = 1e-6;
    let info = |i: u32| topo.faces.get(i as usize);
    let revolved = |k: K| matches!(k, K::Cylinder | K::Cone);
    let mut out = Vec::new();
    for (j, g) in (0u32..).zip(&topo.faces) {
        if faces.contains(&j) {
            continue;
        }
        let hit = faces.iter().filter_map(|i| info(*i)).any(|f| match (f.kind, g.kind) {
            (K::Plane, K::Plane) => {
                norm(sub(f.normal, g.normal)) < TOL
                    && (dot(f.centroid, f.normal) - dot(g.centroid, g.normal)).abs() < TOL
            }
            (a, b) if revolved(a) && revolved(b) => match (f.axis, g.axis) {
                (Some(x), Some(y)) => {
                    norm(cross(x.dir, y.dir)) < TOL && norm(cross(sub(y.origin, x.origin), x.dir)) < TOL
                }
                _ => false,
            },
            _ => false,
        });
        if hit {
            out.push(j);
        }
    }
    out
}
