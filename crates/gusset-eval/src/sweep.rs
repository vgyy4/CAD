//! Sweep and loft features (ADR 0017).

use super::*;
use gusset_doc::{LoftDef, Orientation, PathRef, SweepDef};
use gusset_kernel::{LoftSpec, SweepOrientation, SweepPath, SweepSpec};

/// Largest sweep twist, in turns (the kernel refuses more as well).
pub(crate) const MAX_TWIST_TURNS: f64 = 100.0;

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
            PathRef::Helix(h) => self.helix_path(s, name, h, &solved, &regions, frame, units, params)?,
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

    /// A helix about the axis, through the profile (ADR 0034): its radius and start direction
    /// are the first region's inside point seen from the axis.
    #[allow(clippy::too_many_arguments)]
    fn helix_path(
        &self,
        s: &State,
        name: &str,
        h: &gusset_doc::HelixDef,
        solved: &sketch::Solved,
        regions: &[String],
        frame: gusset_kernel::Frame,
        units: LengthUnit,
        params: &ParamValues,
    ) -> Result<SweepPath, EvalError> {
        let bad = |m: &str| EvalError::new(name, ErrorCode::InvalidReference, m.to_string());
        let (origin, axis) = match &h.axis {
            AxisRef::X => ([0.0; 3], [1.0, 0.0, 0.0]),
            AxisRef::Y => ([0.0; 3], [0.0, 1.0, 0.0]),
            AxisRef::Z => ([0.0; 3], [0.0, 0.0, 1.0]),
            AxisRef::SketchLine { sketch, entity } => {
                let sv = s.solved.get(sketch).ok_or_else(|| bad(&format!("'{sketch}' is not an evaluated sketch")))?;
                let (o, d) = line_axis(name, sketch, entity, sv)?;
                let f = sv.frame;
                let at = |u: f64, v: f64, w: f64| -> [f64; 3] {
                    [
                        w * f.origin[0] + u * f.x[0] + v * f.y[0],
                        w * f.origin[1] + u * f.x[1] + v * f.y[1],
                        w * f.origin[2] + u * f.x[2] + v * f.y[2],
                    ]
                };
                (at(o[0], o[1], 1.0), at(d[0], d[1], 0.0))
            }
        };
        let pitch = eval_expr(name, &h.pitch, Dim::LENGTH, params, units)?;
        let height = eval_expr(name, &h.height, Dim::LENGTH, params, units)?;
        if !(pitch.is_finite() && pitch > 0.0 && height.is_finite() && height > 0.0) {
            return Err(EvalError::new(name, ErrorCode::Expression, "a helix needs a positive pitch and height")
                .with_hint("e.g. helix(axis: Z, pitch: 1.5 mm, height: 20 mm)"));
        }
        if height / pitch > 2000.0 {
            return Err(EvalError::new(name, ErrorCode::Expression, "the helix has more than 2000 turns"));
        }
        let inside = solved
            .profiles
            .iter()
            .find(|(i, _)| regions.first() == Some(&i.id))
            .map(|(i, _)| i.at)
            .ok_or_else(|| bad("the profile has no region"))?;
        let p = [
            frame.origin[0] + inside[0] * frame.x[0] + inside[1] * frame.y[0],
            frame.origin[1] + inside[0] * frame.x[1] + inside[1] * frame.y[1],
            frame.origin[2] + inside[0] * frame.x[2] + inside[1] * frame.y[2],
        ];
        let n = libm::sqrt(axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]);
        let a = [axis[0] / n, axis[1] / n, axis[2] / n];
        let rel = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
        let along = rel[0] * a[0] + rel[1] * a[1] + rel[2] * a[2];
        let x = [rel[0] - along * a[0], rel[1] - along * a[1], rel[2] - along * a[2]];
        let radius = libm::sqrt(x[0] * x[0] + x[1] * x[1] + x[2] * x[2]);
        if radius < 1e-6 {
            return Err(bad("the profile lies on the helix axis").with_hint("draw the profile beside the axis"));
        }
        // The helix starts level with the profile.
        let origin = [origin[0] + along * a[0], origin[1] + along * a[1], origin[2] + along * a[2]];
        Ok(SweepPath::Helix { origin, axis: a, x, radius, pitch: if h.left { -pitch } else { pitch }, height })
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
