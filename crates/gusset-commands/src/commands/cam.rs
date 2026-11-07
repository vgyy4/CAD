//! CAM commands (ADR 0040): 2.5D pocket and profile toolpaths from sketch regions, and
//! their G-code.

use super::*;
use gusset_cam::{Operation, Setup, Toolpath};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<ToolpathQuery>(), spec::<ExportGcode>()]
}

/// The operation, tool and cutting values shared by both commands (mm, mm/min, rev/min).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJob {
    /// A cam_job.
    #[serde(default)]
    pub job: Option<String>,
    #[serde(default)]
    pub sketch: Option<String>,
    #[serde(default)]
    pub regions: Vec<String>,
    /// pocket(_adaptive), profile_outside, profile_inside, drill, bore
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub tool_diameter: Option<f64>,
    #[serde(default)]
    pub depth: Option<f64>,
    #[serde(default)]
    pub stepdown: Option<f64>,
    /// x diameter.
    #[serde(default)]
    pub stepover: Option<f64>,
    #[serde(default)]
    pub feed: Option<f64>,
    #[serde(default)]
    pub plunge: Option<f64>,
    #[serde(default)]
    pub rpm: Option<f64>,
    #[serde(default)]
    pub top: Option<f64>,
    #[serde(default)]
    pub safe: Option<f64>,
    /// 0 plunges.
    #[serde(default)]
    pub ramp_deg: Option<f64>,
    #[serde(default)]
    pub peck: Option<f64>,
    /// [n, width, height].
    #[serde(default)]
    pub tabs: Option<[f64; 3]>,
    /// Not climb.
    #[serde(default)]
    pub conventional: bool,
}

impl CamJob {
    /// The job with every field given: a stored `cam_job` feature's values (its
    /// expressions evaluated with the document's parameters), or the fields themselves.
    fn resolved(&self, s: &mut Session, ctx: &CmdCtx) -> Result<CamJob, CommandError> {
        let Some(name) = &self.job else {
            for (f, ok) in [
                ("sketch", self.sketch.is_some()),
                ("operation", self.operation.is_some()),
                ("tool_diameter", self.tool_diameter.is_some()),
                ("depth", self.depth.is_some()),
            ] {
                if !ok {
                    return Err(
                        CommandError::args(format!("{f} is missing")).hint("give the job's fields, or job: <cam_job>")
                    );
                }
            }
            return Ok(self.clone());
        };
        if self.sketch.is_some() || self.operation.is_some() || self.tool_diameter.is_some() || self.depth.is_some() {
            return Err(CommandError::args("job takes its fields from the cam_job feature: give job or the fields"));
        }
        let doc = s.document(&ctx.actor)?;
        let Some(FeatureKind::CamJob(d)) = doc.features.iter().find(|f| &f.name == name).map(|f| &f.kind) else {
            return Err(CommandError::not_found(format!("no cam_job '{name}'")));
        };
        let (d, units) = ((**d).clone(), doc.units);
        let params = s.evaluate(&ctx.actor)?.params.clone();
        let v = |e: &gusset_units::expr::Expr, dim: gusset_units::Dim| {
            gusset_eval::expr_value(name, e, dim, &params, units)
                .map_err(|e| CommandError::new("evaluation", e.to_string()))
        };
        let opt = |e: &Option<gusset_units::expr::Expr>, dim| e.as_ref().map(|e| v(e, dim)).transpose();
        use gusset_units::Dim;
        Ok(CamJob {
            job: None,
            sketch: Some(d.sketch.clone()),
            regions: d.regions.clone(),
            operation: Some(d.operation.keyword().to_string()),
            tool_diameter: Some(v(&d.tool_diameter, Dim::LENGTH)?),
            depth: Some(v(&d.depth, Dim::LENGTH)?),
            stepdown: opt(&d.stepdown, Dim::LENGTH)?,
            stepover: opt(&d.stepover, Dim::NONE)?,
            feed: opt(&d.feed, Dim::NONE)?,
            plunge: opt(&d.plunge, Dim::NONE)?,
            rpm: opt(&d.rpm, Dim::NONE)?,
            top: opt(&d.top, Dim::LENGTH)?,
            safe: opt(&d.safe, Dim::LENGTH)?,
            ramp_deg: opt(&d.ramp, Dim::ANGLE)?.map(f64::to_degrees),
            peck: opt(&d.peck, Dim::LENGTH)?,
            tabs: match &d.tabs {
                Some([n, w, h]) => Some([v(n, Dim::NONE)?, v(w, Dim::LENGTH)?, v(h, Dim::LENGTH)?]),
                None => None,
            },
            conventional: d.conventional,
        })
    }

    /// The sketch plane's placement.
    fn frame(&self, s: &mut Session, ctx: &CmdCtx) -> Result<gusset_kernel::Frame, CommandError> {
        self.resolved(s, ctx)?.frame_of(s, ctx)
    }

    /// The toolpath, its setup and warnings about the job.
    fn run(&self, s: &mut Session, ctx: &CmdCtx) -> Result<(Toolpath, Setup, Vec<String>), CommandError> {
        self.resolved(s, ctx)?.run_of(s, ctx)
    }

    fn frame_of(&self, s: &mut Session, ctx: &CmdCtx) -> Result<gusset_kernel::Frame, CommandError> {
        let sketch = self.sketch.clone().unwrap_or_default();
        let doc = s.document(&ctx.actor)?;
        let Some(FeatureKind::Sketch(def)) = doc.features.iter().find(|f| f.name == sketch).map(|f| &f.kind) else {
            return Err(CommandError::not_found(format!("no sketch '{sketch}'")));
        };
        let (def, units) = (def.clone(), doc.units);
        let params = s.evaluate(&ctx.actor)?.params.clone();
        let solved = gusset_eval::solve_sketch_def(&sketch, &def, &params, units)
            .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
        Ok(solved.frame)
    }

    fn run_of(&self, s: &mut Session, ctx: &CmdCtx) -> Result<(Toolpath, Setup, Vec<String>), CommandError> {
        let mut warnings = Vec::new();
        let sketch = self.sketch.clone().unwrap_or_default();
        let (tool, depth) = (self.tool_diameter.unwrap_or(0.0), self.depth.unwrap_or(0.0));
        let op = match self.operation.as_deref().unwrap_or_default() {
            "pocket" => Some(Operation::Pocket { stepover: self.stepover.unwrap_or(0.4) }),
            "pocket_adaptive" => Some(Operation::PocketAdaptive { stepover: self.stepover.unwrap_or(0.25) }),
            "profile_outside" => Some(Operation::Profile { outside: true }),
            "profile_inside" => Some(Operation::Profile { outside: false }),
            "drill" | "bore" => None,
            other => {
                return Err(CommandError::args(format!("unknown operation '{other}'"))
                    .hint("pocket, pocket_adaptive, profile_outside, profile_inside, drill or bore"));
            }
        };
        let top = self.top.unwrap_or(0.0);
        let setup = Setup {
            tool_diameter: tool,
            top,
            depth,
            stepdown: self.stepdown.unwrap_or(tool / 2.0),
            safe: self.safe.unwrap_or(top + 5.0),
            feed: self.feed.unwrap_or(600.0),
            plunge: self.plunge.unwrap_or(200.0),
            rpm: self.rpm.unwrap_or(10_000.0),
            ramp_deg: self.ramp_deg.unwrap_or(3.0),
            tabs: self.tabs.map(|[c, w, h]| gusset_cam::Tabs {
                count: if c.is_finite() && c >= 0.0 { c.min(1000.0) as usize } else { 1000 },
                width: w,
                height: h,
            }),
            climb: !self.conventional,
        };
        let doc = s.document(&ctx.actor)?;
        let Some(FeatureKind::Sketch(def)) = doc.features.iter().find(|f| f.name == sketch).map(|f| &f.kind) else {
            return Err(CommandError::not_found(format!("no sketch '{sketch}'")));
        };
        let def = def.clone();
        let units = doc.units;
        let params = s.evaluate(&ctx.actor)?.params.clone();
        let t = match op {
            Some(op) => {
                let regions = gusset_eval::sketch_regions(&sketch, &def, &params, units, &self.regions)
                    .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
                gusset_cam::toolpath(&regions, op, &setup)
            }
            None => {
                let solved = gusset_eval::solve_sketch_def(&sketch, &def, &params, units)
                    .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
                let holes: Vec<([f64; 2], f64)> = solved
                    .circles
                    .iter()
                    .filter(|(id, _)| self.regions.is_empty() || self.regions.contains(id))
                    // Construction circles mark nothing to drill.
                    .filter(|(id, _)| !def.entities.iter().any(|e| e.id == **id && e.construction))
                    .filter_map(|(id, c)| Some((solved.points.get(c).copied()?, 2.0 * solved.radii.get(id).copied()?)))
                    .collect();
                if self.operation.as_deref() == Some("bore") {
                    // Each circle milled to its own size on a helix.
                    gusset_cam::helical_bore(&holes, &setup)
                } else {
                    // A drill cuts its own size: holes of another size are only spotted
                    // (dogfooding F6-11).
                    let off = holes.iter().filter(|(_, d)| (d - tool).abs() > 1e-3 * tool.max(1.0)).count();
                    if off > 0 {
                        warnings.push(format!(
                            "{off} of {} holes are not {} mm across: a drill cuts its own size (use bore to mill them)",
                            holes.len(),
                            (tool * 1000.0).round() / 1000.0
                        ));
                    }
                    let pts: Vec<[f64; 2]> = holes.iter().map(|(c, _)| *c).collect();
                    gusset_cam::drilling(&pts, self.peck, &setup)
                }
            }
        };
        let t = t.map_err(|e| match e {
            gusset_cam::CamError::Invalid(m) => CommandError::args(m),
            gusset_cam::CamError::NothingToCut(m) => {
                CommandError::new("evaluation", m).hint("use a smaller tool or a larger region")
            }
            gusset_cam::CamError::TooLarge(m) => CommandError::new("too_large", m),
        })?;
        Ok((t, setup, warnings))
    }
}

/// Most points a preview path holds.
const MAX_PATH_POINTS: usize = 200_000;

/// The cutting moves (plunges included, rapids left out) as polylines in world
/// coordinates: the sketch plane's frame, Z along its normal. Arcs are sampled every 10
/// degrees. The flag is set when the path was cut short at `MAX_PATH_POINTS`.
fn world_path(t: &Toolpath, f: &gusset_kernel::Frame) -> (Vec<Vec<[f64; 3]>>, bool) {
    use gusset_cam::Move;
    let nrm = f.normal();
    let w = |p: [f64; 3]| {
        let v = f.to_world([p[0], p[1]]);
        let q = [v[0] + nrm[0] * p[2], v[1] + nrm[1] * p[2], v[2] + nrm[2] * p[2]];
        q.map(|c| (c * 1000.0).round() / 1000.0)
    };
    let mut out: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut cur: Vec<[f64; 3]> = Vec::new();
    let mut at: Option<[f64; 3]> = None;
    let mut count = 0usize;
    for m in &t.moves {
        if count > MAX_PATH_POINTS {
            if cur.len() > 1 {
                out.push(cur);
            }
            return (out, true);
        }
        match *m {
            Move::Rapid(to) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                at = Some(to);
            }
            Move::Line { to, .. } => {
                if cur.is_empty()
                    && let Some(a) = at
                {
                    cur.push(w(a));
                }
                cur.push(w(to));
                count += 1;
                at = Some(to);
            }
            Move::Arc { to, center, ccw } => {
                let a = at.unwrap_or(to);
                if cur.is_empty() {
                    cur.push(w(a));
                }
                let r = libm::hypot(a[0] - center[0], a[1] - center[1]);
                let a0 = libm::atan2(a[1] - center[1], a[0] - center[0]);
                let a1 = libm::atan2(to[1] - center[1], to[0] - center[0]);
                let mut sw = a1 - a0;
                if ccw && sw <= 0.0 {
                    sw += std::f64::consts::TAU;
                }
                if !ccw && sw >= 0.0 {
                    sw -= std::f64::consts::TAU;
                }
                let n = (sw.abs() / 10f64.to_radians()).ceil().clamp(1.0, 36.0) as usize;
                for i in 1..n {
                    let u = i as f64 / n as f64;
                    let t = a0 + sw * u;
                    let z = a[2] + (to[2] - a[2]) * u;
                    cur.push(w([center[0] + r * libm::cos(t), center[1] + r * libm::sin(t), z]));
                }
                cur.push(w(to));
                count += n;
                at = Some(to);
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    (out, false)
}

fn summary(t: &Toolpath, s: &Setup, warnings: &[String]) -> Value {
    let st = t.stats(s);
    let r = |v: f64| (v * 100.0).round() / 100.0;
    let mut out = json!({
        "rings": t.rings,
        "levels_mm": t.levels.iter().map(|z| r(*z)).collect::<Vec<_>>(),
        "moves": t.moves.len(),
        "cut_mm": r(st.cut),
        "plunge_mm": r(st.plunge),
        "rapid_mm": r(st.rapid),
        "minutes": r(st.minutes),
    });
    if !warnings.is_empty()
        && let Some(o) = out.as_object_mut()
    {
        o.insert("warnings".into(), json!(warnings));
    }
    out
}

/// The toolpath of a pocket or profile on sketch regions, summarised.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolpathQuery {
    /// 3D polylines.
    #[serde(default)]
    pub path: bool,
    #[serde(flatten)]
    pub job: CamJob,
}

impl Command for ToolpathQuery {
    const ID: &'static str = "cam.toolpath";
    const TITLE: &'static str = "Toolpath";
    const DESCRIPTION: &'static str = "2.5D pocket/profile/drill toolpath (mm; feed 600, plunge 200, rpm 10000, ramp 3°, stepdown D/2, stepover 0.4, safe top+5): length, time";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (t, setup, warnings) = self.job.run(s, ctx)?;
        let mut out = summary(&t, &setup, &warnings);
        if self.path {
            let frame = self.job.frame(s, ctx)?;
            let (lines, cut_short) = world_path(&t, &frame);
            if let Some(o) = out.as_object_mut() {
                o.insert("path".into(), json!(lines));
                if cut_short {
                    o.insert("path_truncated".into(), json!(true));
                }
            }
        }
        Ok(out)
    }
}

/// Writes the G-code of a pocket or profile to a file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportGcode {
    /// .nc, .gcode or .ngc.
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(flatten)]
    pub job: CamJob,
}

impl Command for ExportGcode {
    const ID: &'static str = "cam.export_gcode";
    const TITLE: &'static str = "Export G-code";
    const DESCRIPTION: &'static str = "Write cam_toolpath's toolpath as metric G-code; check it before running.";
    const CATEGORY: Category = Category::Exchange;
    const FLAGS: Flags = WRITE_FILE;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let p = &self.path;
        let path = crate::paths::export_target(s.project_dir.as_deref(), p, &ctx.actor, &["nc", "gcode", "ngc"])?;
        if path.exists() {
            if !self.overwrite {
                return Err(CommandError::new("confirmation_required", format!("'{p}' already exists"))
                    .hint("pass overwrite: true to replace it"));
            }
            if ctx.is_agent() && s.config.approvals_required && !ctx.approved {
                return Err(CommandError::approval("overwriting an exported file"));
            }
        }
        let (t, setup, warnings) = self.job.run(s, ctx)?;
        let what = self.job.job.clone().unwrap_or_else(|| {
            format!("{} {}", self.job.sketch.as_deref().unwrap_or(""), self.job.operation.as_deref().unwrap_or(""))
        });
        let title = format!("{what} {p}");
        let g = t.gcode(&setup, &title);
        crate::paths::write_file(&path, p, g.as_bytes())?;
        let mut out = summary(&t, &setup, &warnings);
        if let Some(o) = out.as_object_mut() {
            o.insert("written".into(), json!(p));
            o.insert("bytes".into(), json!(g.len()));
        }
        Ok(out)
    }
}
