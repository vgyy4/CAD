//! CAM commands (ADR 0040): 2.5D pocket and profile toolpaths from sketch regions, and
//! their G-code.

use super::*;
use gusset_cam::{Operation, Setup, Toolpath};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<ToolpathQuery>(), spec::<ExportGcode>()]
}

/// The operation, tool and cutting values shared by both commands (mm, mm/min, rev/min).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJob {
    /// Its plane is z = 0.
    pub sketch: String,
    /// Ids (drill: circles); default: all.
    #[serde(default)]
    pub regions: Vec<String>,
    /// pocket, profile_outside, profile_inside, drill.
    pub operation: String,
    pub tool_diameter: f64,
    pub depth: f64,
    /// Default: half the diameter.
    #[serde(default)]
    pub stepdown: Option<f64>,
    /// Pocket, of the diameter (0.4).
    #[serde(default)]
    pub stepover: Option<f64>,
    /// Default 600.
    #[serde(default)]
    pub feed: Option<f64>,
    /// Default 200.
    #[serde(default)]
    pub plunge: Option<f64>,
    /// 10000; 0: none.
    #[serde(default)]
    pub rpm: Option<f64>,
    /// Stock top (0), rapid height (top+5).
    #[serde(default)]
    pub top: Option<f64>,
    #[serde(default)]
    pub safe: Option<f64>,
    /// Entry ramp, deg (3; 0 plunges).
    #[serde(default)]
    pub ramp_deg: Option<f64>,
    /// Drill peck depth.
    #[serde(default)]
    pub peck: Option<f64>,
}

impl CamJob {
    fn run(&self, s: &mut Session, ctx: &CmdCtx) -> Result<(Toolpath, Setup), CommandError> {
        let op = match self.operation.as_str() {
            "pocket" => Some(Operation::Pocket { stepover: self.stepover.unwrap_or(0.4) }),
            "profile_outside" => Some(Operation::Profile { outside: true }),
            "profile_inside" => Some(Operation::Profile { outside: false }),
            "drill" => None,
            other => {
                return Err(CommandError::args(format!("unknown operation '{other}'"))
                    .hint("pocket, profile_outside, profile_inside or drill"));
            }
        };
        let top = self.top.unwrap_or(0.0);
        let setup = Setup {
            tool_diameter: self.tool_diameter,
            top,
            depth: self.depth,
            stepdown: self.stepdown.unwrap_or(self.tool_diameter / 2.0),
            safe: self.safe.unwrap_or(top + 5.0),
            feed: self.feed.unwrap_or(600.0),
            plunge: self.plunge.unwrap_or(200.0),
            rpm: self.rpm.unwrap_or(10_000.0),
            ramp_deg: self.ramp_deg.unwrap_or(3.0),
        };
        let doc = s.document(&ctx.actor)?;
        let Some(FeatureKind::Sketch(def)) = doc.features.iter().find(|f| f.name == self.sketch).map(|f| &f.kind)
        else {
            return Err(CommandError::not_found(format!("no sketch '{}'", self.sketch)));
        };
        let def = def.clone();
        let units = doc.units;
        let params = s.evaluate(&ctx.actor)?.params.clone();
        let t = match op {
            Some(op) => {
                let regions = gusset_eval::sketch_regions(&self.sketch, &def, &params, units, &self.regions)
                    .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
                gusset_cam::toolpath(&regions, op, &setup)
            }
            None => {
                let solved = gusset_eval::solve_sketch_def(&self.sketch, &def, &params, units)
                    .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
                let pts: Vec<[f64; 2]> = solved
                    .circles
                    .iter()
                    .filter(|(id, _)| self.regions.is_empty() || self.regions.contains(id))
                    .filter_map(|(_, c)| solved.points.get(c).copied())
                    .collect();
                gusset_cam::drilling(&pts, self.peck, &setup)
            }
        };
        let t = t.map_err(|e| match e {
            gusset_cam::CamError::Invalid(m) => CommandError::args(m),
            gusset_cam::CamError::NothingToCut(m) => {
                CommandError::new("evaluation", m).hint("use a smaller tool or a larger region")
            }
            gusset_cam::CamError::TooLarge(m) => CommandError::new("too_large", m),
        })?;
        Ok((t, setup))
    }
}

fn summary(t: &Toolpath, s: &Setup) -> Value {
    let st = t.stats(s);
    let r = |v: f64| (v * 100.0).round() / 100.0;
    json!({
        "rings": t.rings,
        "levels_mm": t.levels.iter().map(|z| r(*z)).collect::<Vec<_>>(),
        "moves": t.moves.len(),
        "cut_mm": r(st.cut),
        "plunge_mm": r(st.plunge),
        "rapid_mm": r(st.rapid),
        "minutes": r(st.minutes),
    })
}

/// The toolpath of a pocket or profile on sketch regions, summarised.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolpathQuery {
    #[serde(flatten)]
    pub job: CamJob,
}

impl Command for ToolpathQuery {
    const ID: &'static str = "cam.toolpath";
    const TITLE: &'static str = "Toolpath";
    const DESCRIPTION: &'static str =
        "2.5D pocket or profile toolpath of sketch regions (flat end mill): rings, depth passes, length, time.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (t, setup) = self.job.run(s, ctx)?;
        Ok(summary(&t, &setup))
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
    const DESCRIPTION: &'static str =
        "Write a 2.5D pocket, profile or drilling of a sketch as metric G-code; check before running.";
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
        let (t, setup) = self.job.run(s, ctx)?;
        let title = format!("{} {} {}", self.job.sketch, self.job.operation, p);
        let g = t.gcode(&setup, &title);
        crate::paths::write_file(&path, p, g.as_bytes())?;
        let mut out = summary(&t, &setup);
        if let Some(o) = out.as_object_mut() {
            o.insert("written".into(), json!(p));
            o.insert("bytes".into(), json!(g.len()));
        }
        Ok(out)
    }
}
