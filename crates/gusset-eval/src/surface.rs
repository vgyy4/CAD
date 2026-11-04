//! Surface features (ADR 0036): sheets from sketch curves or regions, thicken, stitch.

use super::*;
use gusset_doc::{PathRef, SurfaceDef, SurfaceOp};
use gusset_kernel::{ExtrudeSpec, Loop2, Profile, Region2};

/// Most bodies one stitch may sew.
const MAX_STITCH: usize = 64;

impl Evaluator {
    pub(crate) fn run_surface(
        &self,
        s: &mut State,
        name: &str,
        d: &SurfaceDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        match &d.op {
            SurfaceOp::Extrude { curves, distance, symmetric } => {
                let PathRef::Sketch { sketch, curves } = curves else {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::InvalidReference,
                        "surface_extrude takes sketch curves",
                    ));
                };
                let distance = eval_expr(name, distance, Dim::LENGTH, params, units)?;
                if distance.abs() < 1e-9 {
                    return Err(EvalError::new(name, ErrorCode::Expression, "the distance is zero")
                        .with_hint("enter how far to extrude, for example 20 mm"));
                }
                let sv = s.solved.get(sketch).cloned().ok_or_else(|| {
                    EvalError::new(name, ErrorCode::InvalidReference, format!("'{sketch}' is not an evaluated sketch"))
                })?;
                let chain = sketch::path_chain(name, sketch, &sv, curves)?;
                let profile = Profile {
                    frame: chain.frame,
                    regions: vec![Region2 { outer: Loop2 { edges: chain.edges }, holes: Vec::new() }],
                };
                let res = k
                    .extrude_curves(&profile, &ExtrudeSpec { distance, symmetric: *symmetric })
                    .map_err(|ke| kernel_err(name, "surface extrusion", ke))?;
                let ctx = OpContext::Sweep { feature: name, sketch, regions: &[] };
                self.combine(s, name, &ctx, res, BodyOp::New, (None, d.body_name.as_deref()))
            }
            SurfaceOp::Patch { profile } => {
                let (solved, frame) = sketch_input(name, &profile.sketch, s)?;
                let (kp, regions) = sketch::kernel_profile(name, &profile.sketch, &solved, frame, &profile.regions)?;
                let res = k.patch(&kp).map_err(|ke| kernel_err(name, "patch", ke))?;
                let ctx = OpContext::Sweep { feature: name, sketch: &profile.sketch, regions: &regions };
                self.combine(s, name, &ctx, res, BodyOp::New, (None, d.body_name.as_deref()))
            }
            SurfaceOp::Thicken { body, thickness, both_sides } => {
                let t = eval_expr(name, thickness, Dim::LENGTH, params, units)?;
                if t.abs() < 1e-9 {
                    return Err(EvalError::new(name, ErrorCode::Expression, "the thickness is zero")
                        .with_hint("enter a thickness such as 2 mm (negative thickens the other way)"));
                }
                let (bi, b) = ops::target_body(s, name, body.as_deref())?;
                if b.topology.solids > 0 {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::InvalidReference,
                        format!("'{}' is a solid; thicken takes a sheet", b.name),
                    )
                    .with_hint("use shell to hollow a solid, or thicken a surface body"));
                }
                let res = k.thicken(&b.shape, t, *both_sides).map_err(|ke| {
                    let mut e = kernel_err(name, "thicken", ke);
                    e.hint.get_or_insert_with(|| "try a smaller thickness, or thicken a smoother sheet".into());
                    e
                })?;
                let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
                let inputs = [&b.names];
                let names = name_body(&OpContext::Modify { feature: name, inputs: &inputs }, &res.face_origins, &topo)
                    .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                ops::replace_body(s, bi, &b, res.shape, names, topo);
                Ok(())
            }
            SurfaceOp::Stitch { bodies, tolerance } => {
                let tol = eval_expr(name, tolerance, Dim::LENGTH, params, units)?;
                if !(tol > 0.0 && tol.is_finite()) {
                    return Err(EvalError::new(name, ErrorCode::Expression, "the tolerance must be a positive length"));
                }
                if bodies.len() > MAX_STITCH {
                    return Err(EvalError::new(name, ErrorCode::TooComplex, format!("at most {MAX_STITCH} bodies")));
                }
                if bodies.len() < 2 {
                    return Err(EvalError::new(name, ErrorCode::InvalidReference, "stitch needs at least two bodies")
                        .with_hint("list the sheets to sew, e.g. bodies: [Wall, Lid]"));
                }
                if let Some(dup) =
                    bodies.iter().enumerate().find_map(|(i, b)| bodies.get(..i)?.contains(b).then_some(b))
                {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::InvalidReference,
                        format!("body '{dup}' is listed twice"),
                    ));
                }
                let mut inputs: Vec<Arc<Body>> = Vec::new();
                for t in bodies {
                    let b = s.bodies.iter().find(|b| &b.feature == t).cloned().ok_or_else(|| {
                        EvalError::new(name, ErrorCode::NoTargetBody, format!("body '{t}' does not exist"))
                            .with_hint("name the feature that created the body; it may have been stitched already")
                    })?;
                    inputs.push(b);
                }
                let shapes: Vec<_> = inputs.iter().map(|b| b.shape.clone()).collect();
                let res = k.stitch(&shapes, tol).map_err(|ke| kernel_err(name, "stitch", ke))?;
                let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
                let in_names: Vec<&gusset_naming::BodyNames> = inputs.iter().map(|b| &b.names).collect();
                let names =
                    name_body(&OpContext::Modify { feature: name, inputs: &in_names }, &res.face_origins, &topo)
                        .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                // The inputs are consumed; the stitched body takes the place of the earliest.
                let at = s.bodies.iter().take_while(|b| !bodies.contains(&b.feature)).count();
                s.bodies.retain(|b| !bodies.contains(&b.feature));
                let at = at.min(s.bodies.len());
                s.bodies.insert(
                    at,
                    Arc::new(Body {
                        name: d.body_name.as_deref().unwrap_or(name).to_string(),
                        feature: name.to_string(),
                        shape: res.shape,
                        names,
                        topology: topo,
                        holes: Vec::new(),
                        sheet: None,
                    }),
                );
                Ok(())
            }
        }
    }
}
