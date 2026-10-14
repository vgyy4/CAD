//! Assembly checks (ADR 0026 phase 2): interference between bodies and motion studies
//! that sweep a parameter without changing the document.

use super::*;
use gusset_kernel::{BBox, BoolOp, Kernel};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<QueryInterference>(), spec::<StudyMotion>()]
}

/// Overlap below this volume (mm³) is contact, not interference.
const DEFAULT_MIN_VOLUME: f64 = 1e-6;

fn boxes_overlap(a: &BBox, b: &BBox) -> bool {
    (0..3).all(|k| match (a.min.get(k), a.max.get(k), b.min.get(k), b.max.get(k)) {
        (Some(amin), Some(amax), Some(bmin), Some(bmax)) => amin <= bmax && bmin <= amax,
        _ => false,
    })
}

/// The pairs of `bodies` that share volume: boxes first, then the common volume.
fn interference(k: &dyn Kernel, bodies: &[&gusset_eval::Body], min_volume: f64) -> Result<Vec<Value>, CommandError> {
    let kerr = |e: gusset_kernel::KernelError| CommandError::new("evaluation", e.to_string());
    let boxes = bodies.iter().map(|b| k.bbox(&b.shape).map_err(kerr)).collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (i, (a, ba)) in bodies.iter().zip(&boxes).enumerate() {
        for (b, bb) in bodies.iter().zip(&boxes).skip(i + 1) {
            if !boxes_overlap(ba, bb) {
                continue;
            }
            match k.boolean(BoolOp::Intersect, &a.shape, &b.shape).and_then(|r| k.mass_properties(&r.shape)) {
                Ok(m) if m.volume > min_volume => {
                    out.push(json!({"a": a.name, "b": b.name, "volume_mm3": m.volume, "centroid_mm": m.centroid}));
                }
                Ok(_) => {}
                // An empty common part (bodies that touch or only share a box) is no
                // interference; the kernel reports it as a failed operation that removed all
                // material. Anything else is reported.
                Err(e) if e.message.contains("removed all material") => {}
                Err(e) => out.push(json!({"a": a.name, "b": b.name, "error": e.to_string()})),
            }
        }
    }
    Ok(out)
}

/// The bodies of `ev` named in `filter` (all when empty).
fn select<'a>(ev: &'a gusset_eval::Evaluation, filter: &[String]) -> Result<Vec<&'a gusset_eval::Body>, CommandError> {
    if filter.is_empty() {
        return Ok(ev.bodies.iter().map(|b| &**b).collect());
    }
    filter
        .iter()
        .map(|n| {
            ev.body(n).map(|b| &**b).ok_or_else(|| {
                let names: Vec<&str> = ev.bodies.iter().map(|b| b.name.as_str()).collect();
                CommandError::not_found(format!("no body '{n}'")).hint(format!("bodies: {}", names.join(", ")))
            })
        })
        .collect()
}

/// Checks bodies for overlapping volume.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryInterference {
    /// Bodies to check (default: all).
    #[serde(default)]
    pub bodies: Vec<String>,
    /// Smallest overlap counted, mm³ (default 1e-6).
    #[serde(default)]
    pub min_volume_mm3: Option<f64>,
}

impl Command for QueryInterference {
    const ID: &'static str = "query.interference";
    const TITLE: &'static str = "Interference";
    const DESCRIPTION: &'static str =
        "Pairs of bodies that overlap, with the shared volume and its centroid. Bodies that only touch do not count.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let ev = s.evaluate(&ctx.actor)?;
        let bodies = select(&ev, &self.bodies)?;
        let k = s.kernel().clone();
        let pairs = interference(k.as_ref(), &bodies, self.min_volume_mm3.unwrap_or(DEFAULT_MIN_VOLUME))?;
        let mut v = json!({ "checked": bodies.len(), "interference": pairs });
        snap_zeros(&mut v);
        Ok(v)
    }
}

/// Most steps a motion study takes.
const MAX_STEPS: usize = 100;

/// Sweeps a parameter and records where the bodies go.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StudyMotion {
    /// The parameter to sweep.
    pub parameter: String,
    /// First value (expression).
    pub from: String,
    /// Last value (expression).
    pub to: String,
    /// Positions, from and to included (2..=100, default 11).
    #[serde(default)]
    pub steps: Option<usize>,
    /// Bodies to record (default: all).
    #[serde(default)]
    pub bodies: Vec<String>,
    /// Also check interference at every step.
    #[serde(default)]
    pub interference: bool,
}

impl Command for StudyMotion {
    const ID: &'static str = "study.motion";
    const TITLE: &'static str = "Motion study";
    const DESCRIPTION: &'static str = "Sweep a parameter from `from` to `to` in equal steps without changing the document; at each step: the rebuild errors, the bodies' centroids and (optionally) interference. For joints driven by parameters.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let steps = self.steps.unwrap_or(11);
        if !(2..=MAX_STEPS).contains(&steps) {
            return Err(CommandError::args(format!("steps must be between 2 and {MAX_STEPS}")));
        }
        let (from, to) = (parse_expr("from", &self.from)?, parse_expr("to", &self.to)?);
        let base = s.document(&ctx.actor)?;
        if !base.params.iter().any(|p| p.name == self.parameter) {
            let names: Vec<&str> = base.params.iter().map(|p| p.name.as_str()).collect();
            return Err(CommandError::not_found(format!("no parameter '{}'", self.parameter))
                .hint(format!("parameters: {}", names.join(", "))));
        }
        let branch = s.current_branch(&ctx.actor);
        let rollback = s.history().branch(branch).map_err(CommandError::history)?.rollback;
        let k = s.kernel().clone();
        let mut frames = Vec::with_capacity(steps);
        for i in 0..steps {
            let t = i as f64 / (steps - 1) as f64;
            // from + (to - from) * t keeps the units of the endpoints.
            let expr = parse_expr("value", &format!("({from}) + (({to}) - ({from})) * {t:?}"))?;
            let mut doc = base.clone();
            if let Some(p) = doc.params.iter_mut().find(|p| p.name == self.parameter) {
                p.expr = expr;
            }
            let ev = s.evaluate_doc(&doc, rollback);
            if ev.cancelled {
                return Err(CommandError::new("cancelled", "the motion study was stopped"));
            }
            let value = match ev.params.get(&self.parameter) {
                Some(Ok(q)) => json!(q.display(doc.units)),
                Some(Err(e)) => json!({ "error": e.to_string() }),
                None => Value::Null,
            };
            let bodies = select(&ev, &self.bodies)?;
            let placed: Vec<Value> = bodies
                .iter()
                .map(|b| match k.mass_properties(&b.shape) {
                    Ok(m) => json!({"name": b.name, "centroid_mm": m.centroid}),
                    Err(e) => json!({"name": b.name, "error": e.to_string()}),
                })
                .collect();
            let mut frame = json!({
                "value": value,
                "errors": ev.errors().iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                "bodies": placed,
            });
            if self.interference {
                frame = merge(frame, json!({ "interference": interference(k.as_ref(), &bodies, DEFAULT_MIN_VOLUME)? }));
            }
            snap_zeros(&mut frame);
            frames.push(frame);
        }
        Ok(json!({ "parameter": self.parameter, "steps": frames }))
    }
}
