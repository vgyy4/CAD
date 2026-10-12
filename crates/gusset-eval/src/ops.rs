//! Shell, mirror and pattern features: operations that rebuild an existing body.

use super::*;
use gusset_doc::{DirRef, MirrorDef, PatternCircularDef, PatternLinearDef, ShellDef};
use gusset_kernel::Transform;
use gusset_naming::BodyNames;

/// Most instances one pattern may create (each costs a boolean operation).
pub(crate) const MAX_INSTANCES: u32 = 500;

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
        let (origin, normal) = mirror_plane(name, &d.plane, &s.bodies)?;
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
            }));
            return Ok(());
        }
        let united = self.unite(name, &body.shape, &body.names, &res.shape, &names)?;
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
        let origin = match &d.through {
            None => [0.0; 3],
            Some([x, y, z]) => {
                let mut p = [0.0; 3];
                for (slot, e) in p.iter_mut().zip([x, y, z]) {
                    *slot = eval_expr(name, e, Dim::LENGTH, params, units)?;
                }
                p
            }
        };
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
        let mut shape = body.shape.clone();
        let mut names = body.names.clone();
        let mut topo = body.topology.clone();
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
            let united = self.unite(name, &shape, &names, &copy.shape, &cnames).map_err(|mut e| {
                e.message = format!("instance {inst_no}: {}", e.message);
                e
            })?;
            (shape, names, topo) = united;
        }
        replace_body(s, bi, &body, shape, names, topo);
        Ok(())
    }

    /// Unites two named shapes and names the result (`Modify` with inputs `[a, b]`).
    fn unite(
        &self,
        feature: &str,
        a: &Shape,
        a_names: &BodyNames,
        b: &Shape,
        b_names: &BodyNames,
    ) -> Result<(Shape, BodyNames, Topology), EvalError> {
        let k = self.kernel.as_ref();
        let res = k.boolean(BoolOp::Union, a, b).map_err(|ke| {
            let mut e = kernel_err(feature, "union", ke);
            e.hint.get_or_insert_with(|| "the copies must overlap or touch cleanly; check the spacing".into());
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(feature, "topology", ke))?;
        let names = name_body(&OpContext::Modify { feature, inputs: &[a_names, b_names] }, &res.face_origins, &topo)
            .map_err(|e| EvalError::new(feature, ErrorCode::KernelFailure, e.0))?;
        Ok((res.shape, names, topo))
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
    let nb = Arc::new(Body { name: old.name.clone(), feature: old.feature.clone(), shape, names, topology });
    if let Some(slot) = s.bodies.get_mut(bi) {
        *slot = nb;
    }
}

/// A face of the same feature as `missing` (preferring the same kind) as a suggestion.
fn closest_face(names: &BodyNames, missing: &FaceRef) -> Option<FaceRef> {
    names
        .faces
        .iter()
        .filter(|f| f.feature == missing.feature)
        .min_by_key(|f| (std::mem::discriminant(&f.key) != std::mem::discriminant(&missing.key), f.to_string()))
        .cloned()
}

fn mirror_plane(feature: &str, plane: &PlaneRef, bodies: &[Arc<Body>]) -> Result<([f64; 3], [f64; 3]), EvalError> {
    match plane {
        PlaneRef::XY => Ok(([0.0; 3], [0.0, 0.0, 1.0])),
        PlaneRef::XZ => Ok(([0.0; 3], [0.0, 1.0, 0.0])),
        PlaneRef::YZ => Ok(([0.0; 3], [1.0, 0.0, 0.0])),
        PlaneRef::Face(r) => {
            for b in bodies.iter().rev() {
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
