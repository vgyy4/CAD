//! Several kernel workers behind one [`Kernel`] (ADR 0024 §3), so independent features
//! can be built at the same time.
//!
//! An operation runs on the worker that holds its first input shape; input shapes held by
//! another worker are copied there first (`write_native` / `read_native`, the kernel's
//! own lossless format). Operations without input shapes run on the calling thread's
//! worker, chosen with [`with_slot`]; outside it, on the first worker. Each worker has
//! its own lock, so calls on different workers run in parallel.

use crate::RemoteKernel;
use gusset_kernel::{
    BBox, BoolOp, ExtrudeSpec, KResult, Kernel, KernelError, KernelErrorKind, LoftSpec, MassProps, Mesh, OpResult,
    Profile, RevolveSpec, Shape, SweepPath, SweepSpec, TessellationParams, Topology, Transform,
};
use std::sync::Arc;

/// Runs `f` with new shapes going to worker `slot` (modulo the pool size); the same as
/// [`gusset_kernel::slot::scope`].
pub fn with_slot<R>(slot: usize, f: impl FnOnce() -> R) -> R {
    gusset_kernel::slot::scope(slot, f)
}

/// See the module documentation.
pub struct PoolKernel {
    members: Vec<Arc<RemoteKernel>>,
    name: &'static str,
}

impl std::fmt::Debug for PoolKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PoolKernel").field("workers", &self.members.len()).finish_non_exhaustive()
    }
}

fn empty() -> KernelError {
    KernelError::new(KernelErrorKind::Internal, "the kernel pool has no workers")
}

impl PoolKernel {
    /// A pool of `members` (at least one).
    #[must_use]
    pub fn new(name: &'static str, members: Vec<RemoteKernel>) -> PoolKernel {
        PoolKernel { members: members.into_iter().map(Arc::new).collect(), name }
    }

    /// Number of workers.
    #[must_use]
    pub fn workers(&self) -> usize {
        self.members.len()
    }

    /// The workers' generations, in order (each increases when its worker restarts).
    #[must_use]
    pub fn generations(&self) -> Vec<u64> {
        self.members.iter().map(|m| m.generation()).collect()
    }

    fn member(&self, i: usize) -> KResult<&RemoteKernel> {
        self.members.get(i).map(|m| &**m).ok_or_else(empty)
    }

    fn owner(&self, shape: &Shape) -> Option<usize> {
        self.members.iter().position(|m| m.owns(shape))
    }

    fn preferred(&self) -> usize {
        gusset_kernel::slot::current() % self.members.len().max(1)
    }

    /// The worker for an operation on `shapes`, and the shapes as that worker's handles.
    fn place(&self, shapes: &[&Shape]) -> KResult<(&RemoteKernel, Vec<Shape>)> {
        let target = shapes.first().and_then(|s| self.owner(s)).unwrap_or_else(|| self.preferred());
        let k = self.member(target)?;
        let mut local = Vec::with_capacity(shapes.len());
        for s in shapes {
            match self.owner(s) {
                Some(o) if o != target => {
                    let bytes = self.member(o)?.write_native(s)?;
                    local.push(k.read_native(&bytes)?);
                }
                _ => local.push((*s).clone()),
            }
        }
        Ok((k, local))
    }

    fn on_one(&self, shape: &Shape) -> KResult<(&RemoteKernel, Shape)> {
        let (k, mut v) = self.place(&[shape])?;
        Ok((k, v.pop().ok_or_else(empty)?))
    }

    fn here(&self) -> KResult<&RemoteKernel> {
        self.member(self.preferred())
    }
}

impl Kernel for PoolKernel {
    fn name(&self) -> &'static str {
        self.name
    }

    /// Changes whenever any worker restarts (the sum of the generations only grows).
    fn epoch(&self) -> u64 {
        self.members.iter().map(|m| m.generation()).sum()
    }

    fn probe(&self) {
        self.members.iter().for_each(|m| m.probe());
    }

    fn parallelism(&self) -> usize {
        self.members.len().max(1)
    }

    fn diagnostics(&self) -> Option<String> {
        let first = self.members.first()?.status().ok()?;
        Some(format!("{} workers; {first}", self.members.len()))
    }

    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> KResult<OpResult> {
        self.here()?.extrude(profile, spec)
    }

    fn revolve(&self, profile: &Profile, spec: &RevolveSpec) -> KResult<OpResult> {
        self.here()?.revolve(profile, spec)
    }

    fn sweep(&self, profile: &Profile, path: &SweepPath, spec: &SweepSpec) -> KResult<OpResult> {
        self.here()?.sweep(profile, path, spec)
    }

    fn loft(&self, sections: &[Profile], spec: &LoftSpec) -> KResult<OpResult> {
        self.here()?.loft(sections, spec)
    }

    fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> KResult<OpResult> {
        let (k, s) = self.place(&[target, tool])?;
        match s.as_slice() {
            [t, u] => k.boolean(op, t, u),
            _ => Err(empty()),
        }
    }

    fn boolean_many(&self, op: BoolOp, target: &Shape, tools: &[Shape]) -> KResult<OpResult> {
        let all: Vec<&Shape> = std::iter::once(target).chain(tools).collect();
        let (k, s) = self.place(&all)?;
        match s.split_first() {
            Some((t, rest)) => k.boolean_many(op, t, rest),
            None => Err(empty()),
        }
    }

    fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> KResult<OpResult> {
        let (k, s) = self.on_one(shape)?;
        k.fillet(&s, edges, radius)
    }

    fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> KResult<OpResult> {
        let (k, s) = self.on_one(shape)?;
        k.chamfer(&s, edges, distance)
    }

    fn shell(&self, shape: &Shape, remove: &[u32], thickness: f64) -> KResult<OpResult> {
        let (k, s) = self.on_one(shape)?;
        k.shell(&s, remove, thickness)
    }

    fn transform(&self, shape: &Shape, t: &Transform) -> KResult<OpResult> {
        let (k, s) = self.on_one(shape)?;
        k.transform(&s, t)
    }

    fn compound(&self, shapes: &[Shape]) -> KResult<OpResult> {
        let all: Vec<&Shape> = shapes.iter().collect();
        let (k, s) = self.place(&all)?;
        k.compound(&s)
    }

    fn topology(&self, shape: &Shape) -> KResult<Topology> {
        let (k, s) = self.on_one(shape)?;
        k.topology(&s)
    }

    fn mass_properties(&self, shape: &Shape) -> KResult<MassProps> {
        let (k, s) = self.on_one(shape)?;
        k.mass_properties(&s)
    }

    fn bbox(&self, shape: &Shape) -> KResult<BBox> {
        let (k, s) = self.on_one(shape)?;
        k.bbox(&s)
    }

    fn tessellate(&self, shape: &Shape, params: &TessellationParams) -> KResult<Mesh> {
        let (k, s) = self.on_one(shape)?;
        k.tessellate(&s, params)
    }

    fn export_step(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
        let shapes: Vec<&Shape> = bodies.iter().map(|(s, _)| *s).collect();
        let (k, s) = self.place(&shapes)?;
        let named: Vec<(&Shape, &str)> = s.iter().zip(bodies).map(|(s, (_, n))| (s, *n)).collect();
        k.export_step(&named)
    }

    fn import_step(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
        self.here()?.import_step(data)
    }

    fn export_iges(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
        let shapes: Vec<&Shape> = bodies.iter().map(|(s, _)| *s).collect();
        let (k, s) = self.place(&shapes)?;
        let named: Vec<(&Shape, &str)> = s.iter().zip(bodies).map(|(s, (_, n))| (s, *n)).collect();
        k.export_iges(&named)
    }

    fn import_iges(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
        self.here()?.import_iges(data)
    }

    fn write_native(&self, shape: &Shape) -> KResult<Vec<u8>> {
        let (k, s) = self.on_one(shape)?;
        k.write_native(&s)
    }

    fn read_native(&self, data: &[u8]) -> KResult<Shape> {
        self.here()?.read_native(data)
    }
}
