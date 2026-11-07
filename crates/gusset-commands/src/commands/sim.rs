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
    /// Refine the peak.
    #[serde(default)]
    pub refine: bool,
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

/// Quads (12 coordinates each) with a value, sorted into `BANDS` equal bands from 0 to the
/// largest value: `[{upto, quads}]`, thinned evenly past `MAX_FIELD_QUADS`.
fn bands(quads: &[([f64; 12], f64)]) -> Value {
    let top = quads.iter().map(|q| q.1).fold(0.0f64, f64::max).max(1e-300);
    let mut out: Vec<Vec<Vec<f64>>> = vec![Vec::new(); BANDS];
    let step = quads.len().div_ceil(MAX_FIELD_QUADS).max(1);
    for (c, v) in quads.iter().step_by(step) {
        let b = ((v / top * BANDS as f64) as usize).min(BANDS - 1);
        if let Some(list) = out.get_mut(b) {
            list.push(c.iter().map(|x| (x * 1000.0).round() / 1000.0).collect());
        }
    }
    let list: Vec<Value> = out
        .into_iter()
        .enumerate()
        .map(|(i, quads)| json!({"upto": sig(top * (i + 1) as f64 / BANDS as f64), "quads": quads}))
        .collect();
    json!(list)
}

fn flat(c: [[f64; 3]; 4]) -> [f64; 12] {
    let mut out = [0.0; 12];
    for (o, v) in out.iter_mut().zip(c.iter().flatten()) {
        *o = *v;
    }
    out
}

impl Command for StaticQuery {
    const ID: &'static str = "sim.static";
    const TITLE: &'static str = "Static stress";
    const DESCRIPTION: &'static str =
        "Linear static analysis on voxels (mm, N, MPa): max displacement, von Mises stress.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (body, mesh, voxel, shortest, face) = setup(s, ctx, self.body.as_deref(), self.voxel)?;
        let fixed = self.fixed.iter().map(|f| face(f)).collect::<Result<Vec<_>, _>>()?;
        // Sheet metal: shell elements on its mid-surface (voxels would need tiny cells).
        if let Some(sheet) = &body.sheet
            && !self.refine
        {
            return self.shell(&body, &mesh, sheet.thickness, fixed, &face);
        }
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
        let (sol, refined) = if self.refine {
            let (s, r) = gusset_fea::solve_refined(&p).map_err(fea_err)?;
            (s, Some(r))
        } else {
            (gusset_fea::solve(&p).map_err(fea_err)?, None)
        };
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
        if let Some(f) = refined
            && let Some(o) = out.as_object_mut()
        {
            // The peak again in a box around it with voxels four times finer.
            o.insert(
                "refined".into(),
                json!({
                    "voxel_mm": sig(f.voxel),
                    "max_von_mises_mpa": sig(f.max_von_mises),
                    "max_von_mises_at_mm": f.max_von_mises_at.map(sig),
                    "box_mm": [f.lo.map(sig), f.hi.map(sig)],
                    "elements": f.elements,
                }),
            );
        }
        let notes = caveats(voxel, shortest, sol.peak_at_fixed);
        if !notes.is_empty()
            && let Some(o) = out.as_object_mut()
        {
            o.insert("caveats".into(), json!(notes));
        }
        if sol.held_load > 1e-9
            && let Some(o) = out.as_object_mut()
        {
            // Load on nodes shared with a held face does nothing.
            o.insert("held_load_share".into(), json!(sig(sol.held_load)));
        }
        if self.field
            && let Some(o) = out.as_object_mut()
        {
            let quads: Vec<([f64; 12], f64)> = sol.surface.iter().map(|q| (flat(q.corners), q.von_mises)).collect();
            o.insert("stress_bands".into(), bands(&quads));
            if sol.surface.len() > MAX_FIELD_QUADS {
                o.insert("stress_bands_thinned".into(), json!(true));
            }
        }
        Ok(out)
    }
}

impl StaticQuery {
    /// The analysis of a sheet-metal body on flat-shell elements over its mid-surface
    /// (element size `voxel`, default the longest side / 40, at least the thickness).
    fn shell(
        &self,
        body: &gusset_eval::Body,
        mesh: &gusset_kernel::Mesh,
        thickness: f64,
        fixed: Vec<usize>,
        face: &FaceIndex,
    ) -> Result<Value, CommandError> {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in &mesh.positions {
            for ((l, h), c) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                *l = l.min(f64::from(*c));
                *h = h.max(f64::from(*c));
            }
        }
        let longest = lo.iter().zip(&hi).map(|(l, h)| h - l).fold(0.0f64, f64::max);
        let h = self.voxel.unwrap_or((longest / 40.0).max(thickness));
        let mid = gusset_fea::skin::mid_surface(mesh, thickness, h).map_err(fea_err)?;
        // Faces are matched within the tessellation's own tolerance.
        let tol = 0.06;
        let mut held = Vec::new();
        for f in &fixed {
            held.extend(gusset_fea::skin::nodes_on_face(mesh, *f, &mid, tol));
        }
        held.sort_unstable();
        held.dedup();
        if held.is_empty() {
            return Err(CommandError::args("the held faces touch no node of the shell model"));
        }
        let mut loads: Vec<(u32, [f64; 3])> = Vec::new();
        let mut loaded = 0;
        for l in &self.loads {
            let on = gusset_fea::skin::nodes_on_face(mesh, face(&l.face)?, &mid, tol);
            if on.is_empty() {
                return Err(CommandError::args(format!("the load on {} touches no node of the shell model", l.face)));
            }
            let share = 1.0 / on.len() as f64;
            loaded += on.len();
            loads.extend(on.into_iter().map(|n| (n, l.force.map(|c| c * share))));
        }
        let nodes = mid.mesh.nodes.clone();
        let quads = mid.mesh.quads.clone();
        let p = gusset_fea::ShellProblem {
            mesh: mid.mesh,
            thickness,
            material: gusset_fea::Material {
                young: self.young.unwrap_or(210_000.0),
                poisson: self.poisson.unwrap_or(0.3),
            },
            fixed: held.clone(),
            loads,
        };
        let sol = gusset_fea::shell_static(&p).map_err(fea_err)?;
        let len = |v: &[f64; 3]| libm::sqrt(v.iter().map(|x| x * x).sum());
        let at_disp = sol
            .displacement
            .iter()
            .zip(&nodes)
            .fold((0.0, [0.0; 3]), |b, (d, p)| if len(d) > b.0 { (len(d), *p) } else { b })
            .1;
        let quad_at = |q: &[u32; 4]| {
            q.iter()
                .filter_map(|i| nodes.get(*i as usize))
                .fold([0.0; 3], |a, p| [a[0] + p[0] / 4.0, a[1] + p[1] / 4.0, a[2] + p[2] / 4.0])
        };
        let vm: Vec<f64> = sol.von_mises_top.iter().zip(&sol.von_mises_bottom).map(|(a, b)| a.max(*b)).collect();
        let at_vm =
            vm.iter().zip(&quads).fold((0.0, [0.0; 3]), |b, (v, q)| if *v > b.0 { (*v, quad_at(q)) } else { b }).1;
        let r = sig;
        let mut out = json!({
            "body": body.name,
            "method": "shell",
            "element_mm": r(h),
            "thickness_mm": r(thickness),
            "elements": quads.len(),
            "nodes": nodes.len(),
            "iterations": sol.iterations,
            "max_displacement_mm": r(sol.max_displacement),
            "max_displacement_at_mm": at_disp.map(r),
            "max_von_mises_mpa": r(sol.max_von_mises),
            "max_von_mises_at_mm": at_vm.map(r),
            "fixed_nodes": held.len(),
            "loaded_nodes": loaded,
        });
        if self.field
            && let Some(o) = out.as_object_mut()
        {
            let field: Vec<([f64; 12], f64)> = quads
                .iter()
                .zip(&vm)
                .map(|(q, v)| {
                    let c = q.map(|i| nodes.get(i as usize).copied().unwrap_or([0.0; 3]));
                    (flat(c), *v)
                })
                .collect();
            o.insert("stress_bands".into(), bands(&field));
            if field.len() > MAX_FIELD_QUADS {
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
) -> Result<(std::sync::Arc<gusset_eval::Body>, gusset_kernel::Mesh, f64, f64, FaceIndex), CommandError> {
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
    let size = bb.size();
    let longest = size.into_iter().fold(0.0f64, f64::max);
    let shortest = size.into_iter().fold(f64::INFINITY, f64::min);
    // At least four voxels across the thinnest side (one through a plate is far too stiff),
    // as long as the grid stays within its cell limit.
    let floor = libm::cbrt(size.iter().product::<f64>() / (gusset_fea::MAX_ELEMENTS as f64 * 7.0));
    let voxel = voxel.unwrap_or_else(|| (longest / 30.0).min(shortest / 4.0).max(floor));
    let b = body.clone();
    let face: FaceIndex = Box::new(move |f: &str| {
        let r = FaceRef::parse(f).map_err(|e| CommandError::args(format!("face {f}: {}", e.message)))?;
        b.names
            .face_index(&r)
            .map(|i| i as usize)
            .ok_or_else(|| CommandError::not_found(format!("face {f} is not on '{}'", b.name)))
    });
    Ok((body, mesh, voxel, shortest, face))
}

/// Notes on how far to trust a result: too few voxels through the thinnest side, a peak at
/// a held edge.
fn caveats(voxel: f64, shortest: f64, peak_at_fixed: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if shortest / voxel < 3.0 {
        out.push("fewer than 3 voxels across the thinnest side: the model is too stiff; give a smaller voxel");
    }
    if peak_at_fixed {
        out.push(
            "the peak stress is at a held face: a sharp held edge has no finite stress; read the stress away from it",
        );
    }
    out
}

/// The lowest natural frequencies of a body.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModalQuery {
    #[serde(default)]
    pub body: Option<String>,
    pub fixed: Vec<String>,
    /// Default 3.
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
    /// Mode to picture.
    #[serde(default)]
    pub shape: Option<usize>,
}

impl Command for ModalQuery {
    const ID: &'static str = "sim.modal";
    const TITLE: &'static str = "Natural frequencies";
    const DESCRIPTION: &'static str = "Lowest natural frequencies (Hz) of a held body.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let (body, mesh, voxel, shortest, face) = setup(s, ctx, self.body.as_deref(), self.voxel)?;
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
            // 1 is the lowest mode.
            shape: self.shape.and_then(|k| k.checked_sub(1)),
        };
        if self.shape.is_some_and(|k| k == 0 || k > p.modes) {
            return Err(CommandError::args("shape names a mode from 1 to modes"));
        }
        let m = gusset_fea::modes(&p).map_err(fea_err)?;
        let r = sig;
        let mut out = json!({
            "body": body.name,
            "voxel_mm": r(voxel),
            "elements": m.elements,
            "iterations": m.iterations,
            "frequencies_hz": m.frequencies.iter().map(|f| r(*f)).collect::<Vec<_>>(),
        });
        let notes = caveats(voxel, shortest, false);
        if !notes.is_empty()
            && let Some(o) = out.as_object_mut()
        {
            o.insert("caveats".into(), json!(notes));
        }
        if !m.shape.is_empty()
            && let Some(o) = out.as_object_mut()
        {
            // Drawn moved by a tenth of the body's longest side at the largest motion,
            // coloured by how far each face moves (relative, 0 to 1).
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for c in m.shape.iter().flat_map(|q| q.corners) {
                for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(c) {
                    *l = l.min(v);
                    *h = h.max(v);
                }
            }
            let amp = 0.1 * lo.iter().zip(&hi).map(|(l, h)| h - l).fold(0.0f64, f64::max);
            let quads: Vec<([f64; 12], f64)> = m
                .shape
                .iter()
                .map(|q| {
                    let mut moved = q.corners;
                    for (c, mv) in moved.iter_mut().zip(&q.motion) {
                        for (x, d) in c.iter_mut().zip(mv) {
                            *x += amp * d;
                        }
                    }
                    let size = q.motion.iter().map(|v| libm::sqrt(v.iter().map(|x| x * x).sum())).sum::<f64>() / 4.0;
                    (flat(moved), size)
                })
                .collect();
            o.insert("shape_bands".into(), bands(&quads));
            // Mode shapes have no physical size: say how this one is drawn and where and
            // which way it moves most (dogfooding F6-14).
            let peak = m
                .shape
                .iter()
                .flat_map(|q| q.corners.iter().zip(&q.motion))
                .map(|(c, v)| (c, v, libm::sqrt(v.iter().map(|x| x * x).sum())))
                .fold(None, |best: Option<(&[f64; 3], &[f64; 3], f64)>, cur| match best {
                    Some(b) if b.2 >= cur.2 => Some(b),
                    _ => Some(cur),
                });
            if let Some((at, v, len)) = peak.filter(|p| p.2 > 0.0) {
                o.insert(
                    "shape_scale".into(),
                    json!({
                        "drawn_mm": r(amp * len),
                        "note": "relative shape: the largest motion is drawn at a tenth of the body's longest side",
                        "largest_at_mm": at.map(r),
                        "direction": v.map(|x| r(x / len)),
                    }),
                );
            }
        }
        Ok(out)
    }
}
