//! Simulation commands (ADR 0041): linear static analysis on a voxel grid.

use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<StaticQuery>(), spec::<ModalQuery>()]
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
/// Most voxel faces a stress picture holds (about 2 MB of JSON); beyond it every k-th
/// face is kept, so the picture thins out evenly instead of losing a side.
const MAX_FIELD_QUADS: usize = 20_000;

/// `v` to 6 significant digits (small results stay visible).
fn sig(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return if v.is_finite() { 0.0 } else { f64::NAN };
    }
    let e = libm::floor(libm::log10(v.abs())) as i32;
    let f = libm::pow(10.0, f64::from(5 - e));
    libm::round(v * f) / f
}

/// The surface quads sorted into `BANDS` equal stress bands from 0 to the largest stress:
/// `[{upto_mpa, quads: [[12 coordinates]]}]`.
fn bands(sol: &gusset_fea::Solution) -> Value {
    let top = sol.surface.iter().map(|q| q.von_mises).fold(0.0f64, f64::max).max(1e-300);
    let mut out: Vec<Vec<Vec<f64>>> = vec![Vec::new(); BANDS];
    let step = sol.surface.len().div_ceil(MAX_FIELD_QUADS).max(1);
    for q in sol.surface.iter().step_by(step) {
        let b = ((q.von_mises / top * BANDS as f64) as usize).min(BANDS - 1);
        if let Some(v) = out.get_mut(b) {
            v.push(q.corners.iter().flatten().map(|c| (c * 1000.0).round() / 1000.0).collect());
        }
    }
    let list: Vec<Value> = out
        .into_iter()
        .enumerate()
        .map(|(i, quads)| json!({"upto_mpa": sig(top * (i + 1) as f64 / BANDS as f64), "quads": quads}))
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
        let (body, mesh, voxel, face) = setup(s, ctx, self.body.as_deref(), self.voxel)?;
        let fixed = self.fixed.iter().map(|f| face(f)).collect::<Result<Vec<_>, _>>()?;
        let mut loads = Vec::new();
        for l in &self.loads {
            loads.push(gusset_fea::Load { face: face(&l.face)?, force: l.force });
        }
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
        let sol = gusset_fea::solve(&p).map_err(fea_err)?;
        let r = sig;
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
        if sol.held_load > 1e-9
            && let Some(o) = out.as_object_mut()
        {
            // Load on nodes shared with a held face does nothing.
            o.insert("held_load_share".into(), json!(sig(sol.held_load)));
        }
        if self.field
            && let Some(o) = out.as_object_mut()
        {
            o.insert("stress_bands".into(), bands(&sol));
            if sol.surface.len() > MAX_FIELD_QUADS {
                o.insert("stress_bands_thinned".into(), json!(true));
            }
        }
        Ok(out)
    }
}

fn fea_err(e: gusset_fea::FeaError) -> CommandError {
    match e {
        gusset_fea::FeaError::Invalid(m) => CommandError::args(m),
        gusset_fea::FeaError::TooLarge(m) => CommandError::new("too_large", m),
        gusset_fea::FeaError::NotConverged(m) => CommandError::new("evaluation", m),
    }
}

type FaceIndex = Box<dyn Fn(&str) -> Result<usize, CommandError>>;

/// The body (default the latest), its mesh, the voxel size (default the longest side / 30)
/// and a lookup from face names to the mesh's face indices.
fn setup(
    s: &mut Session,
    ctx: &CmdCtx,
    body: Option<&str>,
    voxel: Option<f64>,
) -> Result<(std::sync::Arc<gusset_eval::Body>, gusset_kernel::Mesh, f64, FaceIndex), CommandError> {
    let ev = s.evaluate(&ctx.actor)?;
    let body = match body {
        Some(n) => ev.body(n).cloned().ok_or_else(|| CommandError::not_found(format!("no body '{n}'")))?,
        None => ev.bodies.last().cloned().ok_or_else(|| CommandError::not_found("the model has no bodies"))?,
    };
    let k = s.kernel().clone();
    let mesh = k
        .tessellate(&body.shape, &gusset_kernel::TessellationParams::default())
        .map_err(|e| CommandError::new("evaluation", e.to_string()))?;
    let bb = k.bbox(&body.shape).map_err(|e| CommandError::new("evaluation", e.to_string()))?;
    let longest = bb.size().into_iter().fold(0.0f64, f64::max);
    let voxel = voxel.unwrap_or(longest / 30.0);
    let b = body.clone();
    let face: FaceIndex = Box::new(move |f: &str| {
        let r = FaceRef::parse(f).map_err(|e| CommandError::args(format!("face {f}: {}", e.message)))?;
        b.names
            .face_index(&r)
            .map(|i| i as usize)
            .ok_or_else(|| CommandError::not_found(format!("face {f} is not on '{}'", b.name)))
    });
    Ok((body, mesh, voxel, face))
}

/// The lowest natural frequencies of a body.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModalQuery {
    #[serde(default)]
    pub body: Option<String>,
    pub fixed: Vec<String>,
    /// Default 3, at most 10.
    #[serde(default)]
    pub modes: Option<usize>,
    /// MPa (default 210000).
    #[serde(default)]
    pub young: Option<f64>,
    #[serde(default)]
    pub poisson: Option<f64>,
    /// kg/m3 (default 7850).
    #[serde(default)]
    pub density: Option<f64>,
    #[serde(default)]
    pub voxel: Option<f64>,
}

impl Command for ModalQuery {
    const ID: &'static str = "sim.modal";
    const TITLE: &'static str = "Natural frequencies";
    const DESCRIPTION: &'static str = "Lowest natural frequencies (Hz) of a body held at its fixed faces.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (body, mesh, voxel, face) = setup(s, ctx, self.body.as_deref(), self.voxel)?;
        let fixed = self.fixed.iter().map(|f| face(f)).collect::<Result<Vec<_>, _>>()?;
        let p = gusset_fea::ModalProblem {
            mesh: &mesh,
            voxel,
            fixed,
            material: gusset_fea::Material {
                young: self.young.unwrap_or(210_000.0),
                poisson: self.poisson.unwrap_or(0.3),
            },
            // kg/m3 to t/mm3.
            density: self.density.unwrap_or(7850.0) * 1e-12,
            modes: self.modes.unwrap_or(3),
        };
        let m = gusset_fea::modes(&p).map_err(fea_err)?;
        let r = sig;
        Ok(json!({
            "body": body.name,
            "voxel_mm": r(voxel),
            "elements": m.elements,
            "iterations": m.iterations,
            "frequencies_hz": m.frequencies.iter().map(|f| r(*f)).collect::<Vec<_>>(),
        }))
    }
}
