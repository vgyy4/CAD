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

mod datum;
pub mod error;
mod ops;
pub mod sketch;

pub use error::{ErrorCode, EvalError};
pub use sketch::{PartnerKind, RegionInfo, SketchResult, SketchStatus, TrimCut, TrimPlan, trim_plan};

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
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// A solid body produced by evaluation.
#[derive(Debug)]
pub struct Body {
    /// Body name: the creating feature's `body_name`, else the feature's name.
    pub name: String,
    /// The feature that created the body (what `body:` targets refer to).
    pub feature: String,
    pub shape: Shape,
    pub names: BodyNames,
    pub topology: Topology,
}

/// A reference mesh body from an imported STL, 3MF or OBJ file (spec import tier 3).
/// The kernel cannot turn a triangle mesh into a B-rep solid, so it is displayed,
/// measured and exported as a mesh, but features cannot reference or modify it
/// (docs/PROGRESS.md "Deviations from spec").
#[derive(Debug)]
pub struct MeshBody {
    /// The import feature's name.
    pub name: String,
    /// Triangles in millimetres (`edges` empty).
    pub mesh: gusset_kernel::Mesh,
    /// Topology check (watertight, manifold, orientation, signed volume).
    pub validation: gusset_meshio::MeshValidation,
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

/// The result after one feature. Every recomputed feature clones the previous state
/// (states are cached per step), so the parts that grow with the feature count share
/// their contents instead of being copied (review L8).
#[derive(Debug, Clone, Default)]
struct State {
    bodies: Vec<Arc<Body>>,
    meshes: Vec<Arc<MeshBody>>,
    solved: SolvedMap,
    planes: datum::Planes,
    failed: BTreeSet<String>,
    statuses: Statuses,
}

/// Feature statuses as a list whose earlier part is shared, newest first.
#[derive(Debug, Clone, Default)]
struct Statuses(Option<Arc<StatusLink>>);

#[derive(Debug)]
struct StatusLink {
    name: String,
    status: FeatureStatus,
    prev: Option<Arc<StatusLink>>,
}

impl Drop for StatusLink {
    /// Iterative, so dropping a long list cannot overflow the stack.
    fn drop(&mut self) {
        let mut next = self.prev.take();
        while let Some(link) = next {
            match Arc::try_unwrap(link) {
                Ok(mut l) => next = l.prev.take(),
                Err(_) => break,
            }
        }
    }
}

impl Statuses {
    fn push(&mut self, name: String, status: FeatureStatus) {
        self.0 = Some(Arc::new(StatusLink { name, status, prev: self.0.take() }));
    }

    /// In feature order.
    fn to_vec(&self) -> Vec<(String, FeatureStatus)> {
        let mut out = Vec::new();
        let mut cur = self.0.as_deref();
        while let Some(l) = cur {
            out.push((l.name.clone(), l.status.clone()));
            cur = l.prev.as_deref();
        }
        out.reverse();
        out
    }
}

/// Solved sketches by name: a shared base plus a few recent entries, merged into a
/// new base every [`SolvedMap::RECENT`] sketches.
#[derive(Debug, Clone, Default)]
struct SolvedMap {
    base: Arc<BTreeMap<String, Arc<Solved>>>,
    recent: BTreeMap<String, Arc<Solved>>,
}

impl SolvedMap {
    const RECENT: usize = 32;

    fn get(&self, name: &str) -> Option<&Arc<Solved>> {
        self.recent.get(name).or_else(|| self.base.get(name))
    }

    fn insert(&mut self, name: String, solved: Arc<Solved>) {
        self.recent.insert(name, solved);
        if self.recent.len() >= Self::RECENT {
            let mut base = (*self.base).clone();
            base.append(&mut self.recent);
            self.base = Arc::new(base);
        }
    }

    /// Every entry; a name in `recent` replaces the same name in `base`.
    fn to_map(&self) -> BTreeMap<String, Arc<Solved>> {
        let mut all = (*self.base).clone();
        all.extend(self.recent.iter().map(|(k, v)| (k.clone(), v.clone())));
        all
    }
}

/// The outcome of evaluating a document.
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub params: ParamValues,
    pub statuses: Vec<(String, FeatureStatus)>,
    pub sketches: BTreeMap<String, SketchResult>,
    /// Datum planes (`plane` features) in feature order, with their placement.
    pub planes: Vec<(String, Frame)>,
    pub bodies: Vec<Arc<Body>>,
    /// Reference mesh bodies (imported meshes; not solids).
    pub meshes: Vec<Arc<MeshBody>>,
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

/// Maximum cached feature states.
pub const DEFAULT_CACHE_ENTRIES: usize = 4096;
/// Memory budget of the evaluation cache (estimated bytes of the bodies each cached
/// state added; review M10).
pub const DEFAULT_CACHE_BYTES: u64 = 512 << 20;

/// Rough memory of a body: kernel shapes are opaque, so this is an estimate per face
/// and edge (B-rep plus its triangulation) plus the exact size of mesh data.
fn body_bytes(b: &Body) -> u64 {
    let (f, e) = (b.topology.faces.len() as u64, b.topology.edges.len() as u64);
    1024 + f * 4096 + e * 1024
}

fn mesh_bytes(m: &MeshBody) -> u64 {
    let mesh = &m.mesh;
    256 + (mesh.positions.len() + mesh.normals.len()) as u64 * 12 + mesh.indices.len() as u64 * 4
}

/// Fixed overhead charged per cached state.
const ENTRY_BYTES: u64 = 256;

#[derive(Debug)]
struct Cached {
    state: Arc<State>,
    /// Body/mesh allocations this state references (pointer, estimated bytes).
    refs: Vec<(usize, u64)>,
    /// Last use (for LRU eviction).
    stamp: u64,
}

/// Incremental evaluator; keep one per open project.
#[derive(Debug)]
pub struct Evaluator {
    kernel: Arc<dyn Kernel>,
    cache: HashMap<[u8; 32], Cached>,
    /// stamp → key, oldest first.
    lru: BTreeMap<u64, [u8; 32]>,
    clock: u64,
    bytes: u64,
    /// Live body/mesh allocation -> (cached states referencing it, bytes).
    live: HashMap<usize, (usize, u64)>,
    capacity: usize,
    max_bytes: u64,
}

impl Evaluator {
    pub fn new(kernel: Arc<dyn Kernel>) -> Self {
        Evaluator {
            kernel,
            cache: HashMap::new(),
            lru: BTreeMap::new(),
            clock: 0,
            bytes: 0,
            live: HashMap::new(),
            capacity: DEFAULT_CACHE_ENTRIES,
            max_bytes: DEFAULT_CACHE_BYTES,
        }
    }

    pub fn kernel(&self) -> &Arc<dyn Kernel> {
        &self.kernel
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.lru.clear();
        self.live.clear();
        self.bytes = 0;
    }

    /// Changes the cache limits (entries and estimated bytes); evicts at once.
    pub fn set_cache_limits(&mut self, entries: usize, bytes: u64) {
        self.capacity = entries.max(1);
        self.max_bytes = bytes;
        self.evict();
    }

    /// Estimated bytes held by cached states.
    #[must_use]
    pub fn cache_bytes(&self) -> u64 {
        self.bytes
    }

    /// Cached states.
    #[must_use]
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn lookup(&mut self, key: &[u8; 32]) -> Option<Arc<State>> {
        let stamp = self.tick();
        let c = self.cache.get_mut(key)?;
        self.lru.remove(&c.stamp);
        c.stamp = stamp;
        self.lru.insert(stamp, *key);
        Some(c.state.clone())
    }

    /// Charges one reference to a body/mesh allocation; bytes count on 0 -> 1.
    fn retain(&mut self, ptr: usize, bytes: u64) {
        let e = self.live.entry(ptr).or_insert((0, bytes));
        if e.0 == 0 {
            self.bytes += bytes;
        }
        e.0 += 1;
    }

    /// Releases one reference; bytes are freed on 1 -> 0.
    fn release(&mut self, ptr: usize) {
        if let Some(e) = self.live.get_mut(&ptr) {
            e.0 -= 1;
            if e.0 == 0 {
                self.bytes = self.bytes.saturating_sub(e.1);
                self.live.remove(&ptr);
            }
        }
    }

    fn remove_entry(&mut self, c: Cached) {
        self.bytes = self.bytes.saturating_sub(ENTRY_BYTES);
        for (ptr, _) in c.refs {
            self.release(ptr);
        }
    }

    fn remember(&mut self, key: [u8; 32], st: Arc<State>) {
        // Account by unique live allocations: a body shared by many cached states
        // (or kept alive by later states after its own state was evicted) is charged
        // once, until the last state holding it goes (review N2).
        let mut refs: Vec<(usize, u64)> = Vec::new();
        for b in &st.bodies {
            refs.push((Arc::as_ptr(b) as *const () as usize, body_bytes(b)));
        }
        for m in &st.meshes {
            refs.push((Arc::as_ptr(m) as *const () as usize, mesh_bytes(m)));
        }
        let stamp = self.tick();
        for &(p, b) in &refs {
            self.retain(p, b);
        }
        self.bytes += ENTRY_BYTES;
        if let Some(old) = self.cache.insert(key, Cached { state: st, refs, stamp }) {
            self.lru.remove(&old.stamp);
            self.remove_entry(old);
        }
        self.lru.insert(stamp, key);
        self.evict();
    }

    /// Drops least recently used states until both limits hold (the newest state
    /// always stays).
    fn evict(&mut self) {
        while self.cache.len() > 1 && (self.cache.len() > self.capacity || self.bytes > self.max_bytes) {
            let Some((_, key)) = self.lru.pop_first() else { break };
            if let Some(c) = self.cache.remove(&key) {
                self.remove_entry(c);
            }
        }
    }

    /// Evaluates `doc`, stopping before feature index `rollback` (if any).
    pub fn evaluate(&mut self, doc: &Document, rollback: Option<usize>, blobs: &dyn BlobSource) -> Evaluation {
        let params = params::evaluate(&doc.params);
        let mut key = *blake3::hash(format!("gusset-eval 1 {:?}", doc.units).as_bytes()).as_bytes();
        let mut state = Arc::new(State::default());
        let mut recomputed = 0;
        let mut rolled_back = Vec::new();
        for (i, f) in doc.features.iter().enumerate() {
            if rollback.is_some_and(|r| i >= r) {
                rolled_back.push((f.name.clone(), FeatureStatus::RolledBack));
                continue;
            }
            key = step_key(&key, f, &params);
            if let Some(cached) = self.lookup(&key) {
                state = cached;
                continue;
            }
            recomputed += 1;
            let mut s = (*state).clone();
            self.apply(&mut s, f, doc.units, &params, blobs);
            let st = Arc::new(s);
            self.remember(key, st.clone());
            state = st;
        }
        let sketches = state.solved.to_map().into_iter().map(|(n, s)| (n, s.result.clone())).collect();
        let mut statuses = state.statuses.to_vec();
        statuses.append(&mut rolled_back);
        Evaluation {
            params,
            statuses,
            sketches,
            planes: state.planes.to_vec(),
            bodies: state.bodies.clone(),
            meshes: state.meshes.clone(),
            recomputed,
        }
    }

    fn apply(&self, s: &mut State, f: &Feature, units: LengthUnit, params: &ParamValues, blobs: &dyn BlobSource) {
        if f.suppressed {
            s.statuses.push(f.name.clone(), FeatureStatus::Suppressed);
            return;
        }
        if let Some(dep) = f.kind.dependencies().into_iter().find(|d| s.failed.contains(d)) {
            s.failed.insert(f.name.clone());
            s.statuses.push(f.name.clone(), FeatureStatus::Blocked { by: dep });
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
        s.statuses.push(f.name.clone(), status);
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
                let frame = plane_frame(name, &def.plane, s)?;
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
                self.combine(s, name, &e.profile.sketch, &regions, r, e.op, (e.body.as_deref(), e.body_name.as_deref()))
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
                self.combine(
                    s,
                    name,
                    &r.profile.sketch,
                    &regions,
                    res,
                    r.op,
                    (r.body.as_deref(), r.body_name.as_deref()),
                )
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
                let nb = Arc::new(Body {
                    name: body.name.clone(),
                    feature: body.feature.clone(),
                    shape: res.shape,
                    names,
                    topology: topo,
                });
                if let Some(slot) = s.bodies.get_mut(bi) {
                    *slot = nb;
                }
                Ok(())
            }
            FeatureKind::Shell(d) => self.run_shell(s, name, d, units, params),
            FeatureKind::Mirror(d) => self.run_mirror(s, name, d, units, params),
            FeatureKind::PatternLinear(d) => self.run_pattern_linear(s, name, d, units, params),
            FeatureKind::PatternCircular(d) => self.run_pattern_circular(s, name, d, units, params),
            FeatureKind::Plane(d) => self.run_plane(s, name, d, units, params),
            FeatureKind::Move(d) => self.run_move(s, name, d, units, params),
            FeatureKind::Combine(d) => self.run_combine(s, name, d),
            FeatureKind::Import(i) => {
                let bytes = blobs.blob(&i.blob).ok_or_else(|| {
                    EvalError::new(name, ErrorCode::MissingBlob, "the imported data is missing from the project")
                        .with_hint("re-import the file")
                })?;
                let mesh_format = match i.format {
                    ImportFormat::Stl => Some(gusset_meshio::Format::Stl),
                    ImportFormat::ThreeMf => Some(gusset_meshio::Format::ThreeMf),
                    ImportFormat::Obj => Some(gusset_meshio::Format::Obj),
                    ImportFormat::Brep | ImportFormat::Step | ImportFormat::Iges => None,
                };
                if let Some(fmt) = mesh_format {
                    s.meshes.push(Arc::new(read_mesh(name, &bytes, fmt)?));
                    return Ok(());
                }
                let shape = k.read_native(&bytes).map_err(|ke| kernel_err(name, "import", ke))?;
                let topo = k.topology(&shape).map_err(|ke| kernel_err(name, "topology", ke))?;
                let origins: Vec<Vec<gusset_kernel::FaceOrigin>> = vec![Vec::new(); topo.faces.len()];
                let names = name_body(&OpContext::Import { feature: name }, &origins, &topo)
                    .map_err(|e| EvalError::new(name, ErrorCode::KernelFailure, e.0))?;
                s.bodies.push(Arc::new(Body {
                    name: name.to_string(),
                    feature: name.to_string(),
                    shape,
                    names,
                    topology: topo,
                }));
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
        (target, new_name): (Option<&str>, Option<&str>),
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let tool_topo = k.topology(&tool.shape).map_err(|ke| kernel_err(feature, "topology", ke))?;
        let region_ids: Vec<String> = regions.to_vec();
        let tool_names =
            name_body(&OpContext::Sweep { feature, sketch, regions: &region_ids }, &tool.face_origins, &tool_topo)
                .map_err(|e| EvalError::new(feature, ErrorCode::KernelFailure, e.0))?;
        if op == BodyOp::New {
            s.bodies.push(Arc::new(Body {
                name: new_name.unwrap_or(feature).to_string(),
                feature: feature.to_string(),
                shape: tool.shape,
                names: tool_names,
                topology: tool_topo,
            }));
            return Ok(());
        }
        let bi = match target {
            Some(t) => s.bodies.iter().position(|b| b.feature == t).ok_or_else(|| {
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
        let nb = Arc::new(Body {
            name: body.name.clone(),
            feature: body.feature.clone(),
            shape: res.shape,
            names,
            topology: topo,
        });
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

fn plane_frame(feature: &str, plane: &PlaneRef, s: &State) -> Result<Frame, EvalError> {
    match plane {
        PlaneRef::XY => Ok(Frame::XY),
        PlaneRef::XZ => Ok(Frame::XZ),
        PlaneRef::YZ => Ok(Frame::YZ),
        PlaneRef::Face(r) => face_frame(feature, r, &s.bodies),
        PlaneRef::Datum(n) => s.planes.get(n).copied().ok_or_else(|| {
            let mut e =
                EvalError::new(feature, ErrorCode::MissingReference, format!("datum plane '{n}' does not exist"))
                    .with_hint("create it with a plane feature before this one");
            e.reference = Some(n.clone());
            e
        }),
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
#[cfg(test)]
mod tests_datum;
#[cfg(test)]
mod tests_ops;

/// Solves a sketch definition on its own (no 3D placement), for commands that write
/// solved positions back into the document after an edit.
pub fn solve_sketch_def(
    name: &str,
    def: &gusset_doc::SketchDef,
    params: &ParamValues,
    units: LengthUnit,
) -> Result<SketchResult, EvalError> {
    sketch::solve_sketch(name, def, params, units, Frame::XY).map(|s| s.result)
}

/// Parses a stored mesh blob (pure Rust, with gusset-meshio's limits) into one
/// reference mesh body.
fn read_mesh(name: &str, bytes: &[u8], fmt: gusset_meshio::Format) -> Result<MeshBody, EvalError> {
    let imported = gusset_meshio::import_bytes(bytes, Some(fmt), &gusset_meshio::ImportOptions::default())
        .map_err(|e| EvalError::new(name, ErrorCode::Unsupported, format!("the stored mesh cannot be read: {e}")))?;
    let mut mesh = gusset_kernel::Mesh::default();
    let mut validation = gusset_meshio::MeshValidation::default();
    for m in imported.meshes {
        let base = u32::try_from(mesh.positions.len())
            .map_err(|_| EvalError::new(name, ErrorCode::Unsupported, "the mesh is too large"))?;
        let tri0 = u32::try_from(mesh.indices.len() / 3).unwrap_or(u32::MAX);
        mesh.positions.extend_from_slice(&m.mesh.positions);
        mesh.normals.extend_from_slice(&m.mesh.normals);
        mesh.indices.extend(m.mesh.indices.iter().map(|i| i.saturating_add(base)));
        mesh.face_ranges.extend(m.mesh.face_ranges.iter().map(|r| r.start + tri0..r.end + tri0));
        validation.triangles += m.validation.triangles;
        validation.vertices += m.validation.vertices;
        validation.degenerate_triangles += m.validation.degenerate_triangles;
        validation.boundary_edges += m.validation.boundary_edges;
        validation.non_manifold_edges += m.validation.non_manifold_edges;
        validation.inconsistent_edges += m.validation.inconsistent_edges;
        validation.signed_volume += m.validation.signed_volume;
    }
    Ok(MeshBody { name: name.to_string(), mesh, validation })
}
