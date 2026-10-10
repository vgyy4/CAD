//! Evaluates a [`Document`] into named solid bodies (ADR 0004 §6).
//!
//! - Deterministic and single-threaded (model-defining work; spec "Determinism").
//! - Incremental: the state after each feature is cached under a key chained from the
//!   previous key, the feature's canonical text and the values of the parameters it
//!   uses. An edit recomputes only from the first affected feature.
//! - A failing feature never panics or aborts the rebuild: it gets a structured
//!   [`EvalError`], and features depending on it are marked blocked.

#![cfg_attr(test, allow(clippy::float_cmp))]
// Feature errors are rare, cold-path values carrying human-readable context; their size
// does not matter for performance.
#![allow(clippy::result_large_err, reason = "cold-path errors with rich context")]

pub mod error;
pub mod sketch;

pub use error::{ErrorCode, EvalError};
pub use sketch::{RegionInfo, SketchResult, SketchStatus};

use gusset_doc::text::print_feature_object;
use gusset_doc::{AxisRef, BodyOp, Document, EdgeRef, FaceRef, Feature, FeatureKind, ImportFormat, PlaneRef};
use gusset_kernel::{
    BoolOp, ExtrudeSpec, Frame, Kernel, KernelError, OpResult, RevolveSpec, Shape, SurfaceKind, Topology, cross, dot,
    normalize, scale, sub,
};
use gusset_naming::{BodyNames, OpContext, Resolution, name_body};
use gusset_units::params::{self, ParamValues, coerce};
use gusset_units::{Dim, LengthUnit, Quantity};
use sketch::Solved;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

/// A solid body produced by evaluation.
#[derive(Debug)]
pub struct Body {
    /// Stable body name: the feature that created it.
    pub name: String,
    pub shape: Shape,
    pub names: BodyNames,
    pub topology: Topology,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FeatureStatus {
    Ok,
    Suppressed,
    /// After the timeline rollback marker; not evaluated.
    RolledBack,
    Failed(EvalError),
    /// Not evaluated because a feature it depends on failed.
    Blocked {
        by: String,
    },
}

/// Supplies imported blobs (kernel-native B-rep bytes) by hash.
pub trait BlobSource {
    fn blob(&self, hash_hex: &str) -> Option<Arc<[u8]>>;
}

impl BlobSource for () {
    fn blob(&self, _: &str) -> Option<Arc<[u8]>> {
        None
    }
}

#[derive(Debug, Clone, Default)]
struct State {
    bodies: Vec<Arc<Body>>,
    solved: BTreeMap<String, Arc<Solved>>,
    failed: BTreeSet<String>,
    statuses: Vec<(String, FeatureStatus)>,
}

/// The outcome of evaluating a document.
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub params: ParamValues,
    pub statuses: Vec<(String, FeatureStatus)>,
    pub sketches: BTreeMap<String, SketchResult>,
    pub bodies: Vec<Arc<Body>>,
    /// How many features were recomputed (the rest came from the cache).
    pub recomputed: usize,
}

impl Evaluation {
    pub fn errors(&self) -> Vec<&EvalError> {
        self.statuses
            .iter()
            .filter_map(|(_, s)| if let FeatureStatus::Failed(e) = s { Some(e) } else { None })
            .collect()
    }

    pub fn status(&self, feature: &str) -> Option<&FeatureStatus> {
        self.statuses.iter().find(|(n, _)| n == feature).map(|(_, s)| s)
    }

    pub fn body(&self, name: &str) -> Option<&Arc<Body>> {
        self.bodies.iter().find(|b| b.name == name)
    }
}

/// Maximum cached feature states (each holds `Arc`s, so sharing keeps this cheap).
pub const DEFAULT_CACHE_ENTRIES: usize = 4096;

/// Incremental evaluator; keep one per open project.
#[derive(Debug)]
pub struct Evaluator {
    kernel: Arc<dyn Kernel>,
    cache: HashMap<[u8; 32], Arc<State>>,
    order: VecDeque<[u8; 32]>,
    capacity: usize,
}

impl Evaluator {
    pub fn new(kernel: Arc<dyn Kernel>) -> Self {
        Evaluator { kernel, cache: HashMap::new(), order: VecDeque::new(), capacity: DEFAULT_CACHE_ENTRIES }
    }

    pub fn kernel(&self) -> &Arc<dyn Kernel> {
        &self.kernel
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    fn remember(&mut self, key: [u8; 32], st: Arc<State>) {
        if self.cache.insert(key, st).is_none() {
            self.order.push_back(key);
            while self.order.len() > self.capacity {
                if let Some(old) = self.order.pop_front() {
                    self.cache.remove(&old);
                }
            }
        }
    }

    /// Evaluates `doc`, stopping before feature index `rollback` (if any).
    pub fn evaluate(&mut self, doc: &Document, rollback: Option<usize>, blobs: &dyn BlobSource) -> Evaluation {
        let params = params::evaluate(&doc.params);
        let mut key = *blake3::hash(format!("gusset-eval 1 {:?}", doc.units).as_bytes()).as_bytes();
        let mut state = Arc::new(State::default());
        let mut recomputed = 0;
        for (i, f) in doc.features.iter().enumerate() {
            if rollback.is_some_and(|r| i >= r) {
                let mut s = (*state).clone();
                s.statuses.push((f.name.clone(), FeatureStatus::RolledBack));
                state = Arc::new(s);
                continue;
            }
            key = step_key(&key, f, &params);
            if let Some(cached) = self.cache.get(&key) {
                state = cached.clone();
                continue;
            }
            recomputed += 1;
            let mut s = (*state).clone();
            self.apply(&mut s, f, doc.units, &params, blobs);
            let st = Arc::new(s);
            self.remember(key, st.clone());
            state = st;
        }
        let sketches = state.solved.iter().map(|(n, s)| (n.clone(), s.result.clone())).collect();
        Evaluation { params, statuses: state.statuses.clone(), sketches, bodies: state.bodies.clone(), recomputed }
    }

    fn apply(&self, s: &mut State, f: &Feature, units: LengthUnit, params: &ParamValues, blobs: &dyn BlobSource) {
        if f.suppressed {
            s.statuses.push((f.name.clone(), FeatureStatus::Suppressed));
            return;
        }
        if let Some(dep) = f.kind.dependencies().into_iter().find(|d| s.failed.contains(d)) {
            s.failed.insert(f.name.clone());
            s.statuses.push((f.name.clone(), FeatureStatus::Blocked { by: dep }));
            return;
        }
        let r = self.run(s, f, units, params, blobs);
        let status = match r {
            Ok(()) => FeatureStatus::Ok,
            Err(e) => {
                s.failed.insert(f.name.clone());
                FeatureStatus::Failed(e)
            }
        };
        s.statuses.push((f.name.clone(), status));
    }

    fn run(
        &self,
        s: &mut State,
        f: &Feature,
        units: LengthUnit,
        params: &ParamValues,
        blobs: &dyn BlobSource,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let name = f.name.as_str();
        match &f.kind {
            FeatureKind::Sketch(def) => {
                let frame = plane_frame(name, &def.plane, &s.bodies)?;
                match sketch::solve_sketch(name, def, params, units, frame) {
                    Ok(solved) => {
                        s.solved.insert(f.name.clone(), Arc::new(solved));
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            FeatureKind::Extrude(e) => {
                let (solved, frame) = sketch_input(name, &e.profile.sketch, s)?;
                let (profile, regions) =
                    sketch::kernel_profile(name, &e.profile.sketch, &solved, frame, &e.profile.regions)?;
                let distance = eval_expr(name, &e.distance, Dim::LENGTH, params, units)?;
                if distance.abs() < 1e-9 {
                    return Err(EvalError::new(name, ErrorCode::Expression, "extrude distance is zero")
                        .with_hint("enter a non-zero distance"));
                }
                let r = k
                    .extrude(&profile, &ExtrudeSpec { distance, symmetric: e.symmetric })
                    .map_err(|ke| kernel_err(name, "extrusion", ke))?;
                self.combine(s, name, &e.profile.sketch, &regions, r, e.op, e.body.as_deref())
            }
            FeatureKind::Revolve(r) => {
                let (solved, frame) = sketch_input(name, &r.profile.sketch, s)?;
                let (profile, regions) =
                    sketch::kernel_profile(name, &r.profile.sketch, &solved, frame, &r.profile.regions)?;
                let angle = eval_expr(name, &r.angle, Dim::ANGLE, params, units)?;
                let (axis_origin, axis_dir) = match &r.axis {
                    AxisRef::X => ([0.0, 0.0], [1.0, 0.0]),
                    AxisRef::Y => ([0.0, 0.0], [0.0, 1.0]),
                    AxisRef::Z => {
                        return Err(EvalError::new(
                            name,
                            ErrorCode::InvalidReference,
                            "the Z axis is normal to the sketch",
                        )
                        .with_hint("revolve around X, Y or a sketch line"));
                    }
                    AxisRef::SketchLine { sketch, entity } => {
                        let sv = s.solved.get(sketch).ok_or_else(|| {
                            EvalError::new(
                                name,
                                ErrorCode::InvalidReference,
                                format!("'{sketch}' is not an evaluated sketch"),
                            )
                        })?;
                        line_axis(name, sketch, entity, sv)?
                    }
                };
                let res = k
                    .revolve(&profile, &RevolveSpec { axis_origin, axis_dir, angle })
                    .map_err(|ke| kernel_err(name, "revolve", ke))?;
                self.combine(s, name, &r.profile.sketch, &regions, res, r.op, r.body.as_deref())
            }
            FeatureKind::Fillet(b) | FeatureKind::Chamfer(b) => {
                let fillet = matches!(f.kind, FeatureKind::Fillet(_));
                let size = eval_expr(name, &b.size, Dim::LENGTH, params, units)?;
                if size <= 0.0 {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::Expression,
                        format!("{} must be positive", if fillet { "radius" } else { "distance" }),
                    ));
                }
                let (bi, edges) = resolve_edges(name, &b.edges, &s.bodies)?;
                let body = s
                    .bodies
                    .get(bi)
                    .cloned()
                    .ok_or_else(|| EvalError::new(name, ErrorCode::NoTargetBody, "no body"))?;
                let res =
                    if fillet { k.fillet(&body.shape, &edges, size) } else { k.chamfer(&body.shape, &edges, size) };
                let res = res.map_err(|ke| blend_error(name, fillet, size, &edges, &body, ke, units))?;
                let topo = k.topology(&res.shape).map_err(|ke| kernel_err(name, "topology", ke))?;
                let names =
                    name_body(&OpContext::Modify { feature: name, inputs: &[&body.names] }, &res.face_origins, &topo)
                        .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                let nb = Arc::new(Body { name: body.name.clone(), shape: res.shape, names, topology: topo });
                if let Some(slot) = s.bodies.get_mut(bi) {
                    *slot = nb;
                }
                Ok(())
            }
            FeatureKind::Import(i) => {
                if !matches!(i.format, ImportFormat::Brep | ImportFormat::Step | ImportFormat::Iges) {
                    return Err(EvalError::new(
                        name,
                        ErrorCode::Unsupported,
                        format!("{} imports are meshes and cannot be used as solids yet", i.format.keyword()),
                    ));
                }
                let bytes = blobs.blob(&i.blob).ok_or_else(|| {
                    EvalError::new(name, ErrorCode::MissingBlob, "the imported data is missing from the project")
                        .with_hint("re-import the file")
                })?;
                let shape = k.read_native(&bytes).map_err(|ke| kernel_err(name, "import", ke))?;
                let topo = k.topology(&shape).map_err(|ke| kernel_err(name, "topology", ke))?;
                let origins: Vec<Vec<gusset_kernel::FaceOrigin>> = vec![Vec::new(); topo.faces.len()];
                let names = name_body(&OpContext::Import { feature: name }, &origins, &topo)
                    .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                s.bodies.push(Arc::new(Body { name: name.to_string(), shape, names, topology: topo }));
                Ok(())
            }
        }
    }

    /// Adds a swept tool as a new body or combines it with a target body.
    #[allow(clippy::too_many_arguments)]
    fn combine(
        &self,
        s: &mut State,
        feature: &str,
        sketch: &str,
        regions: &[String],
        tool: OpResult,
        op: BodyOp,
        target: Option<&str>,
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let tool_topo = k.topology(&tool.shape).map_err(|ke| kernel_err(feature, "topology", ke))?;
        let region_ids: Vec<String> = regions.to_vec();
        let tool_names =
            name_body(&OpContext::Sweep { feature, sketch, regions: &region_ids }, &tool.face_origins, &tool_topo)
                .map_err(|e| EvalError::new(feature, ErrorCode::KernelFailure, e.0))?;
        if op == BodyOp::New {
            s.bodies.push(Arc::new(Body {
                name: feature.to_string(),
                shape: tool.shape,
                names: tool_names,
                topology: tool_topo,
            }));
            return Ok(());
        }
        let bi = match target {
            Some(t) => s.bodies.iter().position(|b| b.name == t).ok_or_else(|| {
                EvalError::new(
                    feature,
                    ErrorCode::NoTargetBody,
                    format!("body '{t}' does not exist (did its feature fail?)"),
                )
            })?,
            None => s.bodies.len().checked_sub(1).ok_or_else(|| {
                EvalError::new(feature, ErrorCode::NoTargetBody, "there is no body to combine with")
                    .with_hint("use op: new for the first body")
            })?,
        };
        let body =
            s.bodies.get(bi).cloned().ok_or_else(|| EvalError::new(feature, ErrorCode::NoTargetBody, "no body"))?;
        let bop = match op {
            BodyOp::Add => BoolOp::Union,
            BodyOp::Cut => BoolOp::Cut,
            BodyOp::Intersect => BoolOp::Intersect,
            BodyOp::New => BoolOp::Union,
        };
        let res = k.boolean(bop, &body.shape, &tool.shape).map_err(|ke| {
            let mut e = kernel_err(feature, "boolean", ke);
            if op == BodyOp::Cut || op == BodyOp::Intersect {
                e.hint.get_or_insert_with(|| "check that the profile overlaps the body".into());
            }
            e
        })?;
        let topo = k.topology(&res.shape).map_err(|ke| kernel_err(feature, "topology", ke))?;
        if topo.faces.is_empty() {
            return Err(EvalError::new(feature, ErrorCode::EmptyResult, "the operation removed the whole body"));
        }
        let names =
            name_body(&OpContext::Modify { feature, inputs: &[&body.names, &tool_names] }, &res.face_origins, &topo)
                .map_err(|e| EvalError::new(feature, ErrorCode::KernelFailure, e.0))?;
        let nb = Arc::new(Body { name: body.name.clone(), shape: res.shape, names, topology: topo });
        if let Some(slot) = s.bodies.get_mut(bi) {
            *slot = nb;
        }
        Ok(())
    }
}

fn step_key(prev: &[u8; 32], f: &Feature, params: &ParamValues) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(prev);
    h.update(print_feature_object(f).as_bytes());
    for p in f.kind.parameters() {
        h.update(p.as_bytes());
        match params.get(&p) {
            Some(Ok(q)) => {
                h.update(&q.value.to_bits().to_le_bytes());
                h.update(format!("{:?}", q.dim).as_bytes());
            }
            Some(Err(e)) => {
                h.update(e.message.as_bytes());
            }
            None => {
                h.update(b"<missing>");
            }
        }
    }
    *h.finalize().as_bytes()
}

fn eval_expr(
    feature: &str,
    e: &gusset_units::expr::Expr,
    dim: Dim,
    params: &ParamValues,
    units: LengthUnit,
) -> Result<f64, EvalError> {
    let scope = |n: &str| params.get(n).and_then(|r| r.as_ref().ok().copied());
    let q: Quantity = gusset_units::expr::eval(e, &scope).map_err(|err| {
        let mut ee = EvalError::new(feature, ErrorCode::Expression, format!("'{e}': {}", err.message));
        ee.hint = err.hint;
        ee
    })?;
    coerce(q, dim, units)
        .map(|q| q.value)
        .map_err(|m| EvalError::new(feature, ErrorCode::Expression, format!("'{e}': {m}")))
}

fn kernel_err(feature: &str, what: &str, ke: KernelError) -> EvalError {
    let mut e = EvalError::new(feature, ErrorCode::KernelFailure, format!("{what} failed: {}", ke.message));
    e.hint = ke.hint;
    e
}

#[allow(clippy::too_many_arguments)]
fn blend_error(
    feature: &str,
    fillet: bool,
    size: f64,
    edges: &[u32],
    body: &Body,
    ke: KernelError,
    units: LengthUnit,
) -> EvalError {
    let what = if fillet { "radius" } else { "distance" };
    let shortest = edges
        .iter()
        .filter_map(|&i| body.topology.edges.get(i as usize).map(|e| (i, e.length)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let fmt = |v: f64| Quantity::mm(v).display(units);
    match shortest {
        Some((i, len)) if size >= len * 0.5 => {
            let edge = body.names.edges.get(i as usize).map(ToString::to_string).unwrap_or_default();
            let mut e = EvalError::new(
                feature,
                ErrorCode::FilletTooLarge,
                format!(
                    "{} {} {} is too large for edge {} (length {})",
                    if fillet { "fillet" } else { "chamfer" },
                    what,
                    fmt(size),
                    edge,
                    fmt(len)
                ),
            )
            .with_hint(format!("use a {what} smaller than {}", fmt(len * 0.5)));
            e.reference = Some(edge);
            e
        }
        _ => kernel_err(feature, if fillet { "fillet" } else { "chamfer" }, ke)
            .with_hint(format!("try a smaller {what} or fewer edges")),
    }
}

/// The solved sketch and its placement frame.
fn sketch_input(feature: &str, sketch: &str, s: &State) -> Result<(Arc<Solved>, Frame), EvalError> {
    let solved = s.solved.get(sketch).cloned().ok_or_else(|| {
        EvalError::new(feature, ErrorCode::InvalidReference, format!("'{sketch}' is not an evaluated sketch"))
    })?;
    let frame = solved.frame;
    Ok((solved, frame))
}

fn plane_frame(feature: &str, plane: &PlaneRef, bodies: &[Arc<Body>]) -> Result<Frame, EvalError> {
    match plane {
        PlaneRef::XY => Ok(Frame::XY),
        PlaneRef::XZ => Ok(Frame::XZ),
        PlaneRef::YZ => Ok(Frame::YZ),
        PlaneRef::Face(r) => face_frame(feature, r, bodies),
    }
}

/// A deterministic frame on a planar face: origin = projection of the world origin onto
/// the plane, X = world X projected (world Y if X is nearly normal).
fn face_frame(feature: &str, r: &FaceRef, bodies: &[Arc<Body>]) -> Result<Frame, EvalError> {
    for b in bodies.iter().rev() {
        if let Some(fi) = b.names.face_index(r) {
            let Some(f) = b.topology.faces.get(fi as usize) else { continue };
            if f.kind != SurfaceKind::Plane {
                return Err(EvalError::new(feature, ErrorCode::InvalidReference, format!("face {r} is not planar"))
                    .with_hint("sketch on a flat face or a base plane"));
            }
            let n = normalize(f.normal).unwrap_or([0.0, 0.0, 1.0]);
            let origin = scale(n, dot(f.centroid, n));
            let project = |v: [f64; 3]| sub(v, scale(n, dot(v, n)));
            let x = normalize(project([1.0, 0.0, 0.0]))
                .filter(|x| dot(*x, [1.0, 0.0, 0.0]).abs() > 1e-3)
                .or_else(|| normalize(project([0.0, 1.0, 0.0])))
                .unwrap_or([1.0, 0.0, 0.0]);
            return Frame::new(origin, x, cross(n, x)).ok_or_else(|| {
                EvalError::new(feature, ErrorCode::KernelFailure, "could not place a sketch on that face")
            });
        }
    }
    let mut e = EvalError::new(feature, ErrorCode::MissingReference, format!("face {r} no longer exists"))
        .with_hint("re-select the face this sketch sits on");
    e.reference = Some(r.to_string());
    Err(e)
}

fn line_axis(feature: &str, sketch: &str, entity: &str, sv: &Solved) -> Result<([f64; 2], [f64; 2]), EvalError> {
    let pts = sv.line_points.get(entity).ok_or_else(|| {
        EvalError::new(feature, ErrorCode::InvalidReference, format!("'{sketch}.{entity}' is not a line"))
    })?;
    let (a, b) = *pts;
    let d = [b[0] - a[0], b[1] - a[1]];
    if d[0].hypot(d[1]) < 1e-9 {
        return Err(EvalError::new(feature, ErrorCode::InvalidReference, "the axis line has zero length"));
    }
    Ok((a, d))
}

/// Resolves every edge reference; all must lie on the same body.
fn resolve_edges(feature: &str, refs: &[EdgeRef], bodies: &[Arc<Body>]) -> Result<(usize, Vec<u32>), EvalError> {
    let mut body_idx: Option<usize> = None;
    let mut out = Vec::new();
    for r in refs {
        let mut found = None;
        let mut best_missing: Option<(String, Option<EdgeRef>)> = None;
        for (bi, b) in bodies.iter().enumerate().rev() {
            match b.names.resolve_edge(r) {
                Resolution::Resolved(i) => {
                    found = Some((bi, i));
                    break;
                }
                Resolution::Ambiguous(c) => {
                    let mut e = EvalError::new(
                        feature,
                        ErrorCode::AmbiguousReference,
                        format!("edge {r} matches {} edges", c.len()),
                    )
                    .with_hint("re-select the edge");
                    e.reference = Some(r.to_string());
                    e.suggestion = c.first().map(ToString::to_string);
                    return Err(e);
                }
                Resolution::Missing { reason, suggestion } => {
                    if best_missing.is_none() || suggestion.is_some() {
                        best_missing = Some((reason, suggestion));
                    }
                }
            }
        }
        let Some((bi, i)) = found else {
            let (reason, sugg) = best_missing.unwrap_or_else(|| ("there are no bodies".into(), None));
            let mut e =
                EvalError::new(feature, ErrorCode::MissingReference, format!("edge {r} no longer exists: {reason}"))
                    .with_hint(if sugg.is_some() {
                        "accept the suggested edge or re-select it"
                    } else {
                        "re-select the edge"
                    });
            e.reference = Some(r.to_string());
            e.suggestion = sugg.map(|s| s.to_string());
            return Err(e);
        };
        if body_idx.is_some_and(|b| b != bi) {
            return Err(EvalError::new(feature, ErrorCode::InvalidReference, "all edges must belong to the same body")
                .with_hint("split this into one feature per body"));
        }
        body_idx = Some(bi);
        if !out.contains(&i) {
            out.push(i);
        }
    }
    body_idx.map(|b| (b, out)).ok_or_else(|| EvalError::new(feature, ErrorCode::InvalidReference, "no edges selected"))
}

#[cfg(test)]
mod tests;
