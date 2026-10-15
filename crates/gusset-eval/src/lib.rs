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

mod budget;
mod component;
mod datum;
mod delta;
mod deps;
pub mod error;
pub mod gear;
mod hole;
mod joint;
mod ops;
mod pattern;
pub mod sketch;
mod sweep;

pub use budget::{DEFAULT_MAX_BODY_FACES, DEFAULT_MAX_BOOLEAN_WORK, DEFAULT_MAX_TOOLS, EvalLimits};
pub use error::{ErrorCode, EvalError};
pub use hole::{DEFAULT_POINT_ANGLE, Dims as HoleDims, HoleInfo, ThreadInfo};
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
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// The holes made by `hole` features, with their cosmetic threads (ADR 0019). A body
    /// made by mirroring, moving a copy or patterning starts without records.
    pub holes: Vec<HoleInfo>,
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
pub trait BlobSource: Sync {
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
    /// Work charged so far ([`budget`]); kept per state so limits do not depend on the cache.
    work: budget::Work,
    /// Joints evaluated so far and the body placements they made (closed loops re-place
    /// earlier bodies, ADR 0026 §1).
    joints: Arc<joint::Joints>,
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

    /// The status pushed last.
    fn newest(&self) -> Option<(&str, &FeatureStatus)> {
        self.0.as_deref().map(|l| (l.name.as_str(), &l.status))
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
    /// The rebuild was cancelled ([`Evaluator::cancel_flag`]): the features from the first
    /// one not finished on are [`ErrorCode::Cancelled`] failures, and nothing of them was
    /// cached. Do not keep or report such an evaluation as the document's state.
    pub cancelled: bool,
    /// The result holds a failure that may not repeat (the kernel worker could not start,
    /// or kept crashing): it is not cached here, and callers should not cache it either.
    pub transient: bool,
    /// The kernel epoch ([`Kernel::epoch`]) the bodies' shapes belong to: an evaluation
    /// whose epoch is no longer the kernel's holds shapes of a restarted kernel worker.
    pub epoch: u64,
    /// Values of each joint as built, in base units (mm, rad), including those the loop
    /// solver chose: `angle`, `distance`, `x`, `y`, `angles`.
    pub joint_values: BTreeMap<String, JointValues>,
}

/// A joint's values as built (base units: mm, rad).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointValues {
    pub angle: f64,
    pub distance: f64,
    pub x: f64,
    pub y: f64,
    pub angles: [f64; 3],
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

/// A cache entry: the state after a document prefix, or what one feature changed
/// (ADR 0024), under keys from separate hash domains.
#[derive(Debug, Clone)]
enum Entry {
    State(Arc<State>),
    Delta(Arc<delta::Delta>),
}

#[derive(Debug)]
struct Cached {
    entry: Entry,
    /// Body/mesh allocations this state references (pointer, estimated bytes).
    refs: Vec<(usize, u64)>,
    /// Last use (for LRU eviction).
    stamp: u64,
}

/// Where a running evaluation is (ADR 0022 §4): the feature being built, its position and
/// the number of features. Readable from any thread through [`Evaluator::progress_cell`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub feature: String,
    /// 1-based position of the feature in the document.
    pub index: usize,
    pub total: usize,
}

/// Shared [`Progress`] of an evaluator: `None` while no evaluation builds a feature.
pub type ProgressCell = Arc<std::sync::Mutex<Option<Progress>>>;

fn set_progress(cell: &ProgressCell, p: Option<Progress>) {
    *cell.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = p;
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
    limits: EvalLimits,
    cancel: Arc<AtomicBool>,
    progress: ProgressCell,
    /// Features built at the same time (ADR 0024 §3); 1 = sequential.
    parallel: usize,
    /// Features built ahead of their turn, over the evaluator's life (tests, diagnostics).
    speculated: usize,
    /// Kernel epoch the cached states belong to.
    epoch: u64,
    /// Features that crashed the kernel, by step key (the feature and everything before
    /// it), oldest first: not run again until something in that chain changes (ADR 0022).
    crashes: VecDeque<([u8; 32], EvalError)>,
    /// Components built in the current top-level evaluation, shared with the evaluators
    /// of nested components (review v0.3 H3).
    components: Arc<component::Builds>,
}

/// Remembered kernel crashes.
const MAX_CRASH_MEMO: usize = 64;
/// Rebuilds restarted after a crash in one evaluation. Each restart memoises one more
/// crashing feature and skips it, so this only bounds a document with many of them.
const MAX_CRASH_RESTARTS: usize = 32;

const CRASH_HINT: &str = "the rest of the model is rebuilt without it; change this feature's inputs \
                          (a smaller size, fewer copies, a simpler profile) and it is tried again";

impl Evaluator {
    pub fn new(kernel: Arc<dyn Kernel>) -> Self {
        let parallel = kernel.parallelism().max(1);
        Evaluator {
            kernel,
            cache: HashMap::new(),
            lru: BTreeMap::new(),
            clock: 0,
            bytes: 0,
            live: HashMap::new(),
            capacity: DEFAULT_CACHE_ENTRIES,
            max_bytes: DEFAULT_CACHE_BYTES,
            limits: EvalLimits::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            progress: ProgressCell::default(),
            parallel,
            speculated: 0,
            epoch: 0,
            crashes: VecDeque::new(),
            components: Arc::default(),
        }
    }

    /// Drops cached states whose shapes died with a kernel worker.
    fn sync_epoch(&mut self) {
        let now = self.kernel.epoch();
        if now != self.epoch {
            self.clear_cache();
            self.epoch = now;
        }
    }

    /// The work limits ([`EvalLimits`]).
    #[must_use]
    pub fn limits(&self) -> EvalLimits {
        self.limits
    }

    /// Changes the work limits. Cached results were decided under the old limits, so the
    /// cache is cleared when they change.
    pub fn set_limits(&mut self, limits: EvalLimits) {
        if limits != self.limits {
            self.limits = limits;
            self.clear_cache();
        }
    }

    /// A flag that stops evaluations when set from any thread (a host sets it for "Stop
    /// rebuild" or when a newer edit makes the running rebuild moot). It is checked between
    /// features and before every charged boolean; a kernel running in a worker process
    /// also stops the operation in progress (ADR 0022 §3), an in-process one finishes it. A cancelled evaluation has [`Evaluation::cancelled`] set and caches nothing it
    /// did after the cancel. The flag stays set (so every evaluation of the command being
    /// cancelled stops) until its owner clears it: `gusset-host` clears it when the next
    /// job starts.
    #[must_use]
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Builds up to `n` independent features at the same time (ADR 0024 §3). Worth it only
    /// with a kernel whose calls run in parallel (a pool of kernel workers); results are
    /// the same as building them one after the other.
    pub fn set_parallel(&mut self, n: usize) {
        self.parallel = n.max(1);
    }

    /// Features built ahead of their turn so far.
    #[must_use]
    pub fn speculated(&self) -> usize {
        self.speculated
    }

    /// Later features that can be built now, next to feature `i`, on the state before `i`:
    /// they depend only on features before `i` and on sketches or planes after it that
    /// themselves depend only on features before `i` (built first on the same thread; they
    /// are cheap), and nothing of them is cached yet. Each is `(feature, sketches/planes)`.
    fn speculation_candidates(
        &self,
        doc: &Document,
        i: usize,
        rollback: Option<usize>,
        graph: &deps::Graph,
        dep_keys: &[[u8; 32]],
    ) -> Vec<(usize, Vec<usize>)> {
        const WINDOW: usize = 64;
        let mut out = Vec::new();
        // Next to a sketch or plane there is nothing to overlap with: this thread would
        // only wait for the others.
        let light =
            doc.features.get(i).is_none_or(|f| matches!(f.kind, FeatureKind::Sketch(_) | FeatureKind::Plane(_)));
        if self.parallel < 2 || light {
            return out;
        }
        let end = doc.features.len().min(i + 1 + WINDOW).min(rollback.unwrap_or(usize::MAX));
        for j in i + 1..end {
            if out.len() + 1 >= self.parallel {
                break;
            }
            let (Some(f), Some(deps), Some(key)) = (doc.features.get(j), graph.deps.get(j), dep_keys.get(j)) else {
                break;
            };
            let cheap = |k: usize| {
                doc.features.get(k).is_some_and(|g| matches!(g.kind, FeatureKind::Sketch(_) | FeatureKind::Plane(_)))
            };
            let mut pre = Vec::new();
            let mut ok = !cheap(j) && !f.suppressed;
            for &d in deps.iter().filter(|&&d| d >= i) {
                let d_ok = d > i && cheap(d) && graph.deps.get(d).is_some_and(|dd| dd.iter().all(|&x| x < i));
                ok &= d_ok;
                pre.push(d);
            }
            if ok && !self.cache.contains_key(key) && !self.crashes.iter().any(|(k, _)| k == key) {
                out.push((j, pre));
            }
        }
        out
    }

    /// The progress of the running evaluation, for display from another thread.
    #[must_use]
    pub fn progress_cell(&self) -> ProgressCell {
        self.progress.clone()
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Charges a boolean feature against the limits ([`budget`]) and checks for cancellation.
    pub(crate) fn charge(
        &self,
        s: &mut State,
        feature: &str,
        what: &str,
        tools: usize,
        faces_in: u64,
    ) -> Result<(), EvalError> {
        if self.is_cancelled() {
            return Err(budget::cancelled(feature));
        }
        s.work.charge(&self.limits, feature, what, tools, faces_in)
    }

    pub fn kernel(&self) -> &Arc<dyn Kernel> {
        &self.kernel
    }

    /// Forgets the features remembered as crashing the kernel, so the next evaluation tries
    /// them again (a crash may have come from memory pressure or an outside kill).
    pub fn forget_crashes(&mut self) {
        self.crashes.clear();
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

    fn lookup_entry(&mut self, key: &[u8; 32]) -> Option<Entry> {
        let stamp = self.tick();
        let c = self.cache.get_mut(key)?;
        self.lru.remove(&c.stamp);
        c.stamp = stamp;
        self.lru.insert(stamp, *key);
        Some(c.entry.clone())
    }

    fn lookup(&mut self, key: &[u8; 32]) -> Option<Arc<State>> {
        match self.lookup_entry(key)? {
            Entry::State(s) => Some(s),
            Entry::Delta(_) => None,
        }
    }

    fn lookup_delta(&mut self, key: &[u8; 32]) -> Option<Arc<delta::Delta>> {
        match self.lookup_entry(key)? {
            Entry::Delta(d) => Some(d),
            Entry::State(_) => None,
        }
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
        self.insert_entry(key, Entry::State(st), refs);
    }

    fn remember_delta(&mut self, key: [u8; 32], d: delta::Delta) {
        let refs = d.refs().collect();
        self.insert_entry(key, Entry::Delta(Arc::new(d)), refs);
    }

    fn insert_entry(&mut self, key: [u8; 32], entry: Entry, refs: Vec<(usize, u64)>) {
        let stamp = self.tick();
        for &(p, b) in &refs {
            self.retain(p, b);
        }
        self.bytes += ENTRY_BYTES;
        if let Some(old) = self.cache.insert(key, Cached { entry, refs, stamp }) {
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
        // The cancel flag also stops a kernel operation in progress where the kernel can
        // (the worker process is killed, ADR 0022 §3); its shapes are then gone too.
        if component::depth() == 0 {
            self.components = Arc::default();
        }
        let cancel = self.cancel.clone();
        let progress = self.progress.clone();
        let e = gusset_kernel::cancel::scope(&cancel, || {
            let mut restarts = 0;
            loop {
                self.kernel.probe();
                self.sync_epoch();
                let (e, crashed) = self.evaluate_pass(doc, rollback, blobs);
                // A crash ended the worker and every shape of this pass: rebuild once more
                // from the document, now skipping the feature that crashed (memoised).
                if !crashed || restarts >= MAX_CRASH_RESTARTS {
                    return e;
                }
                restarts += 1;
            }
        });
        set_progress(&progress, None);
        e
    }

    fn evaluate_pass(&mut self, doc: &Document, rollback: Option<usize>, blobs: &dyn BlobSource) -> (Evaluation, bool) {
        let params = params::evaluate(&doc.params, doc.units);
        let mut key = *blake3::hash(format!("gusset-eval 1 {:?}", doc.units).as_bytes()).as_bytes();
        let mut state = Arc::new(State::default());
        let mut recomputed = 0;
        let mut rolled_back = Vec::new();
        let mut cancelled = false;
        // Set by a kernel failure that may not repeat: nothing after it is cached.
        let mut transient = false;
        // Dependency keys (ADR 0024): a feature's own text and parameters plus the keys of
        // the features it depends on, so what it did can be reused when only features it
        // does not depend on changed.
        let graph = deps::graph(doc);
        let units_key = *blake3::hash(format!("gusset-eval dep 1 {:?}", doc.units).as_bytes()).as_bytes();
        let mut dep_keys: Vec<[u8; 32]> = Vec::with_capacity(doc.features.len());
        for (i, f) in doc.features.iter().enumerate() {
            let deps = graph.deps.get(i).map_or(&[][..], Vec::as_slice);
            let dkey = dep_key(&units_key, f, &params, deps.iter().filter_map(|&d| dep_keys.get(d)));
            dep_keys.push(dkey);
        }
        for (i, f) in doc.features.iter().enumerate() {
            let dkey = dep_keys.get(i).copied().unwrap_or_default();
            if rollback.is_some_and(|r| i >= r) {
                rolled_back.push((f.name.clone(), FeatureStatus::RolledBack));
                continue;
            }
            if cancelled {
                rolled_back.push((f.name.clone(), FeatureStatus::Failed(budget::cancelled(&f.name))));
                continue;
            }
            key = step_key(&key, f, &params);
            if let Some(cached) = self.lookup(&key) {
                state = cached;
                continue;
            }
            let mut s = (*state).clone();
            if let Some(d) = self.lookup_delta(&dkey)
                && d.replayable(&s.work, &self.limits)
            {
                d.replay(&mut s, &f.name);
                let st = Arc::new(s);
                self.remember(key, st.clone());
                state = st;
                continue;
            }
            if let Some((_, err)) = self.crashes.iter().find(|(k, _)| *k == dkey) {
                s.failed.insert(f.name.clone());
                s.statuses.push(f.name.clone(), FeatureStatus::Failed(err.clone()));
                state = Arc::new(s);
                continue;
            }
            let epoch = self.kernel.epoch();
            let mut ahead: Vec<(usize, State)> = Vec::new();
            if !self.is_cancelled() {
                recomputed += 1;
                let (index, total) = (i + 1, doc.features.len());
                set_progress(&self.progress, Some(Progress { feature: f.name.clone(), index, total }));
                let candidates = if transient {
                    Vec::new()
                } else {
                    self.speculation_candidates(doc, i, rollback, &graph, &dep_keys)
                };
                if candidates.is_empty() {
                    self.apply(&mut s, f, doc.units, &params, blobs);
                } else {
                    ahead = self.build_ahead(&mut s, f, &state, &candidates, doc, &params, blobs);
                }
            }
            // What was built ahead is kept as deltas, replayed when its turn comes; not
            // after a crash or a cancel (its shapes may be gone or half made).
            if !self.is_cancelled() && self.kernel.epoch() == epoch {
                for (j, t) in ahead {
                    if let (Some(fj), Some(key)) = (doc.features.get(j), dep_keys.get(j))
                        && let Some(d) = delta::Delta::diff(&state, &t, &fj.name)
                    {
                        self.speculated += 1;
                        self.remember_delta(*key, d);
                    }
                }
            }
            let crash_status = match s.statuses.newest() {
                Some((n, FeatureStatus::Failed(e))) if n == f.name && e.code == ErrorCode::KernelCrashed => {
                    Some(e.clone())
                }
                _ => None,
            };
            if !self.is_cancelled() && self.kernel.epoch() != epoch {
                // Always memoised, whatever error the feature turned the crash into (a
                // fillet reports a too-large radius, review H2): the restart must skip it.
                let e = crash_status.unwrap_or_else(|| {
                    EvalError::new(
                        &f.name,
                        ErrorCode::KernelCrashed,
                        format!("the geometry kernel crashed while building {}", f.name),
                    )
                    .with_hint(CRASH_HINT)
                });
                self.crashes.push_back((dkey, e));
                while self.crashes.len() > MAX_CRASH_MEMO {
                    self.crashes.pop_front();
                }
                let crashed = Evaluation {
                    params,
                    statuses: s.statuses.to_vec(),
                    sketches: BTreeMap::new(),
                    planes: Vec::new(),
                    bodies: Vec::new(),
                    meshes: Vec::new(),
                    recomputed,
                    cancelled: false,
                    transient: true,
                    epoch: self.kernel.epoch(),
                    joint_values: BTreeMap::new(),
                };
                return (crashed, true);
            }
            // A crash without a new worker generation: the worker could not be started
            // (review M2). It may work next time, so neither this nor anything after it
            // is cached.
            transient |= crash_status.is_some();
            if self.is_cancelled() {
                // The feature may have stopped part way: neither keep nor cache its state.
                cancelled = true;
                rolled_back.push((f.name.clone(), FeatureStatus::Failed(budget::cancelled(&f.name))));
                continue;
            }
            let st = Arc::new(s);
            if !transient {
                if let Some(d) = delta::Delta::diff(&state, &st, &f.name) {
                    self.remember_delta(dkey, d);
                }
                self.remember(key, st.clone());
            }
            state = st;
        }
        let sketches = state.solved.to_map().into_iter().map(|(n, s)| (n, s.result.clone())).collect();
        let mut statuses = state.statuses.to_vec();
        statuses.append(&mut rolled_back);
        let e = Evaluation {
            params,
            statuses,
            sketches,
            planes: state.planes.to_vec(),
            bodies: state.bodies.clone(),
            meshes: state.meshes.clone(),
            recomputed,
            cancelled,
            transient,
            epoch: self.epoch,
            joint_values: state
                .joints
                .recs
                .iter()
                .map(|r| {
                    let v = r.vals;
                    (
                        r.name.clone(),
                        JointValues { angle: v.angle, distance: v.distance, x: v.x, y: v.y, angles: v.angles },
                    )
                })
                .collect(),
        };
        (e, false)
    }

    /// Builds feature `f` into `s` on this thread, and the features `ahead` (indices into
    /// `doc`) on the state `before` at the same time, each on its own thread and kernel
    /// worker slot. Returns the states those produced.
    #[allow(clippy::too_many_arguments)]
    fn build_ahead(
        &self,
        s: &mut State,
        f: &Feature,
        before: &State,
        ahead: &[(usize, Vec<usize>)],
        doc: &Document,
        params: &ParamValues,
        blobs: &dyn BlobSource,
    ) -> Vec<(usize, State)> {
        let cancel = self.cancel.clone();
        std::thread::scope(|scope| {
            let handles: Vec<_> = ahead
                .iter()
                .enumerate()
                .filter_map(|(k, (j, pre))| {
                    let j = *j;
                    let fj = doc.features.get(j)?;
                    let cancel = cancel.clone();
                    let spawned =
                        std::thread::Builder::new().name("gusset-eval-ahead".into()).spawn_scoped(scope, move || {
                            gusset_kernel::cancel::scope(&cancel, || {
                                gusset_kernel::slot::scope(k + 1, || {
                                    let mut t = before.clone();
                                    for p in pre.iter().filter_map(|&p| doc.features.get(p)) {
                                        self.apply(&mut t, p, doc.units, params, blobs);
                                    }
                                    self.apply(&mut t, fj, doc.units, params, blobs);
                                    (j, t)
                                })
                            })
                        });
                    spawned.ok()
                })
                .collect();
            gusset_kernel::slot::scope(0, || self.apply(s, f, doc.units, params, blobs));
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        })
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
        let before: Vec<usize> = s.bodies.iter().map(|b| Arc::as_ptr(b) as usize).collect();
        let r = self.run(s, f, units, params, blobs);
        prune_holes(s, &before);
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
                let ctx = OpContext::Sweep { feature: name, sketch: &e.profile.sketch, regions: &regions };
                self.combine(s, name, &ctx, r, e.op, (e.body.as_deref(), e.body_name.as_deref()))
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
                let ctx = OpContext::Sweep { feature: name, sketch: &r.profile.sketch, regions: &regions };
                self.combine(s, name, &ctx, res, r.op, (r.body.as_deref(), r.body_name.as_deref()))
            }
            FeatureKind::Sweep(d) => self.run_sweep(s, name, d, units, params),
            FeatureKind::Loft(d) => self.run_loft(s, name, d),
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
                    holes: body.holes.clone(),
                });
                if let Some(slot) = s.bodies.get_mut(bi) {
                    *slot = nb;
                }
                Ok(())
            }
            FeatureKind::Shell(d) => self.run_shell(s, name, d, units, params),
            FeatureKind::Draft(d) => self.run_draft(s, name, d, units, params),
            FeatureKind::Mirror(d) => self.run_mirror(s, name, d, units, params),
            FeatureKind::PatternLinear(d) => self.run_pattern_linear(s, name, d, units, params),
            FeatureKind::PatternCircular(d) => self.run_pattern_circular(s, name, d, units, params),
            FeatureKind::Plane(d) => self.run_plane(s, name, d, units, params),
            FeatureKind::Move(d) => self.run_move(s, name, d, units, params),
            FeatureKind::Joint(d) => self.run_joint(s, name, d, units, params),
            FeatureKind::Component(d) => self.run_component(s, name, d, params, blobs),
            FeatureKind::Combine(d) => self.run_combine(s, name, d),
            FeatureKind::Hole(d) => self.run_hole(s, name, d, units, params),
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
                    holes: Vec::new(),
                }));
                Ok(())
            }
        }
    }

    /// Adds a swept tool as a new body or combines it with a target body.
    fn combine(
        &self,
        s: &mut State,
        feature: &str,
        ctx: &OpContext<'_>,
        tool: OpResult,
        op: BodyOp,
        (target, new_name): (Option<&str>, Option<&str>),
    ) -> Result<(), EvalError> {
        let k = self.kernel.as_ref();
        let tool_topo = k.topology(&tool.shape).map_err(|ke| kernel_err(feature, "topology", ke))?;
        let tool_names = name_body(ctx, &tool.face_origins, &tool_topo)
            .map_err(|e| EvalError::new(feature, ErrorCode::KernelFailure, e.0))?;
        if op == BodyOp::New {
            s.bodies.push(Arc::new(Body {
                name: new_name.unwrap_or(feature).to_string(),
                feature: feature.to_string(),
                shape: tool.shape,
                names: tool_names,
                topology: tool_topo,
                holes: Vec::new(),
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
        // One tool from a sketch cannot multiply the body, so this is not charged; only stop
        // here when the rebuild was cancelled.
        if self.is_cancelled() {
            return Err(budget::cancelled(feature));
        }
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
            holes: body.holes.clone(),
        });
        if let Some(slot) = s.bodies.get_mut(bi) {
            *slot = nb;
        }
        Ok(())
    }
}

/// Drops the hole records of changed bodies whose wall face no longer exists (a later cut
/// or shell removed the hole; review L1). `before` holds the bodies before the feature.
fn prune_holes(s: &mut State, before: &[usize]) {
    for slot in &mut s.bodies {
        if slot.holes.is_empty() || before.contains(&(Arc::as_ptr(slot) as usize)) {
            continue;
        }
        let live: std::collections::HashSet<(&str, &gusset_doc::FaceKey)> =
            slot.names.faces.iter().map(|f| (f.feature.as_str(), &f.key)).collect();
        let keep = |h: &HoleInfo| live.contains(&(h.wall.feature.as_str(), &h.wall.key));
        if slot.holes.iter().all(&keep) {
            continue;
        }
        let holes = slot.holes.iter().filter(|h| keep(h)).cloned().collect();
        *slot = Arc::new(Body {
            name: slot.name.clone(),
            feature: slot.feature.clone(),
            shape: slot.shape.clone(),
            names: slot.names.clone(),
            topology: slot.topology.clone(),
            holes,
        });
    }
}

/// A feature's dependency key (ADR 0024): its text and parameter values and the keys
/// of the features it depends on.
fn dep_key<'a>(
    units: &[u8; 32],
    f: &Feature,
    params: &ParamValues,
    deps: impl Iterator<Item = &'a [u8; 32]>,
) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&step_key(units, f, params));
    for d in deps {
        h.update(d);
    }
    *h.finalize().as_bytes()
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
    if ke.kind == gusset_kernel::KernelErrorKind::Crashed {
        // The app survived; the operation did not (ADR 0022 §2).
        return EvalError::new(
            feature,
            ErrorCode::KernelCrashed,
            format!("the geometry kernel crashed while building {feature} ({what}): {}", ke.message),
        )
        .with_hint(CRASH_HINT);
    }
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
    if libm::hypot(d[0], d[1]) < 1e-9 {
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
mod tests_budget;
#[cfg(test)]
mod tests_crash;
#[cfg(test)]
mod tests_datum;
#[cfg(test)]
mod tests_deps;
#[cfg(test)]
mod tests_hole;
#[cfg(test)]
mod tests_joint;
#[cfg(test)]
mod tests_ops;
#[cfg(test)]
mod tests_sweep;

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
