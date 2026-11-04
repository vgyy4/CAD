//! Simulation commands (ADR 0041): linear static analysis on a voxel grid.

use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<StaticQuery>()]
}

/// A force on a face.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaceLoad {
    pub face: String,
    /// N, total: [x, y, z].
    pub force: [f64; 3],
}

/// Linear static analysis of a body.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticQuery {
    /// Default: the latest body.
    #[serde(default)]
    pub body: Option<String>,
    /// Faces held still.
    pub fixed: Vec<String>,
    pub loads: Vec<FaceLoad>,
    /// MPa (default 210000).
    #[serde(default)]
    pub young: Option<f64>,
    /// Default 0.3.
    #[serde(default)]
    pub poisson: Option<f64>,
    /// Voxel edge mm (default longest side/30).
    #[serde(default)]
    pub voxel: Option<f64>,
    /// Stress bands.
    #[serde(default)]
    pub field: bool,
}

/// Colour bands of a stress picture.
const BANDS: usize = 8;
/// Most voxel faces a stress picture holds.
const MAX_FIELD_QUADS: usize = 100_000;

/// The surface quads sorted into `BANDS` equal stress bands from 0 to the largest stress:
/// `[{upto_mpa, quads: [[12 coordinates]]}]`.
fn bands(sol: &gusset_fea::Solution) -> Value {
    let top = sol.surface.iter().map(|q| q.von_mises).fold(0.0f64, f64::max).max(1e-300);
    let mut out: Vec<Vec<Vec<f64>>> = vec![Vec::new(); BANDS];
    for q in sol.surface.iter().take(MAX_FIELD_QUADS) {
        let b = ((q.von_mises / top * BANDS as f64) as usize).min(BANDS - 1);
        if let Some(v) = out.get_mut(b) {
            v.push(q.corners.iter().flatten().map(|c| (c * 1000.0).round() / 1000.0).collect());
        }
    }
    let list: Vec<Value> = out
        .into_iter()
        .enumerate()
        .map(
            |(i, quads)| json!({"upto_mpa": (top * (i + 1) as f64 / BANDS as f64 * 1e3).round() / 1e3, "quads": quads}),
        )
        .collect();
    json!(list)
}

impl Command for StaticQuery {
    const ID: &'static str = "sim.static";
    const TITLE: &'static str = "Static stress";
    const DESCRIPTION: &'static str =
        "Linear static analysis on voxels (mm, N, MPa): max displacement, von Mises stress. An estimate.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let ev = s.evaluate(&ctx.actor)?;
        let body = match &self.body {
            Some(n) => ev.body(n).cloned().ok_or_else(|| CommandError::not_found(format!("no body '{n}'")))?,
            None => ev.bodies.last().cloned().ok_or_else(|| CommandError::not_found("the model has no bodies"))?,
        };
        let face = |f: &str| -> Result<usize, CommandError> {
            let r = FaceRef::parse(f).map_err(|e| CommandError::args(format!("face {f}: {}", e.message)))?;
            body.names
                .face_index(&r)
                .map(|i| i as usize)
                .ok_or_else(|| CommandError::not_found(format!("face {f} is not on '{}'", body.name)))
        };
        let fixed = self.fixed.iter().map(|f| face(f)).collect::<Result<Vec<_>, _>>()?;
        let mut loads = Vec::new();
        for l in &self.loads {
            loads.push(gusset_fea::Load { face: face(&l.face)?, force: l.force });
        }
        let k = s.kernel().clone();
        let mesh = k
            .tessellate(&body.shape, &gusset_kernel::TessellationParams::default())
            .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
        let bb = k.bbox(&body.shape).map_err(|e| CommandError::new("evaluation", e.to_string()))?;
        let longest = bb.size().into_iter().fold(0.0f64, f64::max);
        let voxel = self.voxel.unwrap_or(longest / 30.0);
        let p = gusset_fea::Problem {
            mesh: &mesh,
            voxel,
            fixed,
            loads,
            material: gusset_fea::Material {
                young: self.young.unwrap_or(210_000.0),
                poisson: self.poisson.unwrap_or(0.3),
            },
        };
        let sol = gusset_fea::solve(&p).map_err(|e| match e {
            gusset_fea::FeaError::Invalid(m) => CommandError::args(m),
            gusset_fea::FeaError::TooLarge(m) => CommandError::new("too_large", m),
            gusset_fea::FeaError::NotConverged(m) => CommandError::new("evaluation", m),
        })?;
        let r = |v: f64| (v * 1e6).round() / 1e6;
        let mut out = json!({
            "body": body.name,
            "voxel_mm": r(voxel),
            "elements": sol.elements,
            "nodes": sol.nodes,
            "iterations": sol.iterations,
            "max_displacement_mm": r(sol.max_displacement),
            "max_displacement_at_mm": sol.max_displacement_at.map(r),
            "max_von_mises_mpa": r(sol.max_von_mises),
            "max_von_mises_at_mm": sol.max_von_mises_at.map(r),
            "fixed_nodes": sol.fixed_nodes,
            "loaded_nodes": sol.loaded_nodes,
        });
        if self.field
            && let Some(o) = out.as_object_mut()
        {
            o.insert("stress_bands".into(), bands(&sol));
            if sol.surface.len() > MAX_FIELD_QUADS {
                o.insert("stress_bands_truncated".into(), json!(true));
            }
        }
        Ok(out)
    }
}
