//! Sweep and loft features (ADR 0017).

use super::*;
use gusset_doc::{LoftDef, Orientation, PathRef, SweepDef};
use gusset_kernel::{LoftSpec, SweepOrientation, SweepPath, SweepSpec};

/// Largest sweep twist, in turns (the kernel refuses more as well).
pub const MAX_TWIST_TURNS: f64 = 100.0;

impl Evaluator {
    pub(crate) fn run_sweep(
        &self,
        s: &mut State,
        name: &str,
        d: &SweepDef,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let (solved, frame) = sketch_input(name, &d.profile.sketch, s)?;
        let (profile, regions) = sketch::kernel_profile(name, &d.profile.sketch, &solved, frame, &d.profile.regions)?;
        let twist = match &d.twist {
            Some(e) => eval_expr(name, e, Dim::ANGLE, params, units)?,
            None => 0.0,
        };
        // Review H2: absurd twists reached the kernel (and overflowed its sampling).
        if !twist.is_finite() || twist.abs() > MAX_TWIST_TURNS * std::f64::consts::TAU {
            return Err(EvalError::new(name, ErrorCode::Expression, "the twist is too large")
                .with_hint(format!("use at most {MAX_TWIST_TURNS} turns ({} deg)", MAX_TWIST_TURNS * 360.0)));
        }
        if twist != 0.0 && d.orientation == Orientation::Fixed {
            return Err(EvalError::new(name, ErrorCode::Expression, "a twist needs the profile to follow the path")
                .with_hint("set orientation: frenet, or remove the twist"));
        }
        let path = match &d.path {
            PathRef::Sketch { sketch, curves } => {
                let sv = s.solved.get(sketch).ok_or_else(|| {
                    EvalError::new(name, ErrorCode::InvalidReference, format!("'{sketch}' is not an evaluated sketch"))
                })?;
                SweepPath::Curves(sketch::path_chain(name, sketch, sv, curves)?)
            }
            PathRef::Edges(refs) => {
                let (bi, edges) = resolve_edges(name, refs, &s.bodies)?;
                let body = s
                    .bodies
                    .get(bi)
                    .ok_or_else(|| EvalError::new(name, ErrorCode::NoTargetBody, "the path edges have no body"))?;
                SweepPath::Edges { shape: body.shape.clone(), edges }
            }
        };
        let orientation = match d.orientation {
            Orientation::Fixed => SweepOrientation::Fixed,
            Orientation::Frenet => SweepOrientation::Frenet,
        };
        let res =
            k.sweep(&profile, &path, &SweepSpec { orientation, twist }).map_err(|ke| kernel_err(name, "sweep", ke))?;
        let ctx = OpContext::Sweep { feature: name, sketch: &d.profile.sketch, regions: &regions };
        self.combine(s, name, &ctx, res, d.op, (d.body.as_deref(), d.body_name.as_deref()))
    }

    pub(crate) fn run_loft(&self, s: &mut State, name: &str, d: &LoftDef) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        if d.profiles.len() < 2 {
            return Err(EvalError::new(name, ErrorCode::InvalidReference, "a loft needs at least two profiles")
                .with_hint("list the profile sketches in order: profiles: [Sketch1, Sketch2]"));
        }
        let mut sections = Vec::with_capacity(d.profiles.len());
        let mut sketches = Vec::with_capacity(d.profiles.len());
        for p in &d.profiles {
            let (solved, frame) = sketch_input(name, &p.sketch, s)?;
            let (profile, regions) = sketch::kernel_profile(name, &p.sketch, &solved, frame, &p.regions)?;
            if regions.len() != 1 {
                return Err(EvalError::new(
                    name,
                    ErrorCode::InvalidReference,
                    format!("{} has {} regions in the loft; each profile needs exactly one", p.sketch, regions.len()),
                )
                .with_hint(format!(
                    "pick one, e.g. {}.regions({})",
                    p.sketch,
                    regions.first().map_or("l1", String::as_str)
                )));
            }
            if profile.regions.iter().any(|r| !r.holes.is_empty()) {
                return Err(EvalError::new(
                    name,
                    ErrorCode::InvalidReference,
                    format!("the profile in {} has holes, which a loft cannot follow", p.sketch),
                )
                .with_hint("loft the outline, then cut the holes with an extrude"));
            }
            sections.push(profile);
            sketches.push(p.sketch.clone());
        }
        let res = k.loft(&sections, &LoftSpec { ruled: d.ruled }).map_err(|ke| kernel_err(name, "loft", ke))?;
        let ctx = OpContext::Loft { feature: name, sketches: &sketches };
        self.combine(s, name, &ctx, res, d.op, (d.body.as_deref(), d.body_name.as_deref()))
    }
}
