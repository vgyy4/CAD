//! The app side: [`RemoteKernel`] implements [`Kernel`] by asking a worker over a
//! [`Transport`]. Shapes are [`RemoteShape`] handles; dropping one frees it in the worker
//! with the next request. When the worker dies, every handle it issued becomes invalid
//! (a new generation starts) and the next call starts a new worker (ADR 0022 §2).

use crate::proto::{Fault, Reply, Request, VERSION};
use gusset_kernel::wire::{ShapeRefs, WireError};
use gusset_kernel::{
    BBox, BoolOp, ExtrudeSpec, KResult, Kernel, KernelError, KernelErrorKind, LoftSpec, MassProps, Mesh, OpResult,
    Profile, RevolveSpec, Shape, ShapeData, SweepPath, SweepSpec, TessellationParams, Topology, Transform,
};
use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

/// How a request reaches the worker and its reply comes back.
pub trait Transport: Send + std::fmt::Debug {
    /// Sends one request payload and waits for its reply payload.
    ///
    /// # Errors
    /// [`TransportError::Died`] when the worker is gone (crashed, killed, or its stream
    /// broke); the client then starts a new one for the next call.
    fn call(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The worker stopped; `reason` says how (signal, exit status, broken stream).
    Died { reason: String },
    /// The call was stopped on request (the worker was stopped with it).
    Cancelled,
}

/// Starts a worker (or connects to one).
pub type Connect = Box<dyn Fn() -> Result<Box<dyn Transport>, String> + Send + Sync>;

/// Shape ids dropped by the app, with the generation they belong to.
type Frees = Mutex<Vec<(u64, u64)>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A shape held by the worker.
#[derive(Debug)]
pub struct RemoteShape {
    id: u64,
    generation: u64,
    frees: Weak<Frees>,
}

impl ShapeData for RemoteShape {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn kernel(&self) -> &'static str {
        "remote"
    }
}

impl Drop for RemoteShape {
    fn drop(&mut self) {
        if let Some(f) = self.frees.upgrade() {
            lock(&f).push((self.generation, self.id));
        }
    }
}

#[derive(Debug)]
struct Link {
    transport: Option<Box<dyn Transport>>,
    generation: u64,
    /// Calls made with the current worker (tests, diagnostics).
    calls: u64,
}

/// A [`Kernel`] whose work happens in a worker process.
pub struct RemoteKernel {
    link: Mutex<Link>,
    connect: Connect,
    frees: Arc<Frees>,
    name: &'static str,
}

impl std::fmt::Debug for RemoteKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteKernel").field("link", &self.link).field("name", &self.name).finish_non_exhaustive()
    }
}

/// Maps shapes to ids for one call, on the client side.
struct Refs<'a> {
    generation: u64,
    frees: &'a Arc<Frees>,
}

impl ShapeRefs for Refs<'_> {
    fn id_of(&self, shape: &Shape) -> Result<u64, WireError> {
        let s = shape.0.as_any().downcast_ref::<RemoteShape>().ok_or_else(|| {
            WireError(format!("a shape from kernel '{}' cannot be sent to the worker", shape.0.kernel()))
        })?;
        if s.generation != self.generation {
            return Err(WireError(STALE.into()));
        }
        Ok(s.id)
    }

    fn shape(&self, id: u64) -> Result<Shape, WireError> {
        Ok(Shape::new(RemoteShape { id, generation: self.generation, frees: Arc::downgrade(self.frees) }))
    }
}

const STALE: &str = "this shape was made by a geometry kernel that has since restarted";

fn crashed(message: String) -> KernelError {
    KernelError::new(KernelErrorKind::Crashed, message)
}

impl RemoteKernel {
    /// A kernel that starts its worker with `connect` on first use and after a crash.
    /// `name` is reported by [`Kernel::name`] (the worker's kernel, e.g. "occt").
    #[must_use]
    pub fn new(name: &'static str, connect: Connect) -> RemoteKernel {
        RemoteKernel {
            link: Mutex::new(Link { transport: None, generation: 1, calls: 0 }),
            connect,
            frees: Arc::new(Mutex::new(Vec::new())),
            name,
        }
    }

    /// The current worker generation (it increases with every restart).
    #[must_use]
    pub fn generation(&self) -> u64 {
        lock(&self.link).generation
    }

    /// Asks the worker to inject a fault (tests; needs the `fault-injection` feature in
    /// the worker).
    ///
    /// # Errors
    /// What the fault produces: usually [`KernelErrorKind::Crashed`].
    pub fn inject_fault(&self, fault: Fault) -> KResult<()> {
        self.call(&Request::Fault(fault)).map(|_| ())
    }

    /// Checks that a worker runs and speaks this protocol version.
    ///
    /// # Errors
    /// The worker could not be started, or speaks another version.
    pub fn ping(&self) -> KResult<()> {
        match self.call(&Request::Ping)? {
            Reply::Version(VERSION) => Ok(()),
            Reply::Version(v) => Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("the geometry kernel worker speaks protocol {v}, the app {VERSION}"),
            )
            .with_hint("reinstall Gusset: the app and its worker come from different versions")),
            _ => Err(unexpected()),
        }
    }

    fn call(&self, req: &Request) -> KResult<Reply> {
        let mut link = lock(&self.link);
        if link.transport.is_none() {
            let t = (self.connect)().map_err(|e| {
                crashed(format!("could not start the geometry kernel: {e}"))
                    .with_hint("reinstall Gusset if this persists (gusset self-test checks the worker)")
            })?;
            link.transport = Some(t);
            link.calls = 0;
        }
        let generation = link.generation;
        let refs = Refs { generation, frees: &self.frees };
        // Frees of this generation ride along first; older ones died with their worker.
        let ids: Vec<u64> = {
            let mut f = lock(&self.frees);
            let mine = f.iter().filter(|(g, _)| *g == generation).map(|(_, id)| *id).collect();
            f.clear();
            mine
        };
        let payload = req.encode(&refs).map_err(|e| {
            if e.0 == STALE { crashed(STALE.into()).with_hint("rebuild the model") } else { KernelError::from(e) }
        })?;
        let send = |link: &mut Link, payload: &[u8]| -> KResult<Vec<u8>> {
            let Some(t) = link.transport.as_mut() else {
                return Err(crashed("the geometry kernel is not running".into()));
            };
            link.calls += 1;
            match t.call(payload) {
                Ok(reply) => Ok(reply),
                Err(e) => {
                    // The worker and every shape it held are gone.
                    link.transport = None;
                    link.generation += 1;
                    Err(match e {
                        TransportError::Cancelled => KernelError::new(KernelErrorKind::Cancelled, "stopped"),
                        TransportError::Died { reason } => crashed(format!("the geometry kernel stopped: {reason}")),
                    })
                }
            }
        };
        if !ids.is_empty() {
            let free = Request::Free(ids).encode(&refs).map_err(KernelError::from)?;
            let reply = send(&mut link, &free)?;
            Reply::decode(&reply, &refs).map_err(KernelError::from)??;
        }
        let reply = send(&mut link, &payload)?;
        Reply::decode(&reply, &refs).map_err(KernelError::from)?
    }

    fn op(&self, req: &Request) -> KResult<OpResult> {
        match self.call(req)? {
            Reply::Op(r) => Ok(r),
            _ => Err(unexpected()),
        }
    }

    fn bytes(&self, req: &Request) -> KResult<Vec<u8>> {
        match self.call(req)? {
            Reply::Bytes(b) => Ok(b),
            _ => Err(unexpected()),
        }
    }

    fn bodies(&self, req: &Request) -> KResult<Vec<(Shape, String)>> {
        match self.call(req)? {
            Reply::Bodies(b) => Ok(b),
            _ => Err(unexpected()),
        }
    }
}

fn unexpected() -> KernelError {
    KernelError::new(KernelErrorKind::Internal, "kernel protocol: unexpected reply")
}

fn named(bodies: &[(&Shape, &str)]) -> Vec<(Shape, String)> {
    bodies.iter().map(|(s, n)| ((*s).clone(), (*n).to_string())).collect()
}

impl Kernel for RemoteKernel {
    fn name(&self) -> &'static str {
        self.name
    }

    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> KResult<OpResult> {
        self.op(&Request::Extrude(profile.clone(), *spec))
    }

    fn revolve(&self, profile: &Profile, spec: &RevolveSpec) -> KResult<OpResult> {
        self.op(&Request::Revolve(profile.clone(), *spec))
    }

    fn sweep(&self, profile: &Profile, path: &SweepPath, spec: &SweepSpec) -> KResult<OpResult> {
        self.op(&Request::Sweep(profile.clone(), path.clone(), *spec))
    }

    fn loft(&self, sections: &[Profile], spec: &LoftSpec) -> KResult<OpResult> {
        self.op(&Request::Loft(sections.to_vec(), *spec))
    }

    fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> KResult<OpResult> {
        self.op(&Request::Boolean(op, target.clone(), tool.clone()))
    }

    fn boolean_many(&self, op: BoolOp, target: &Shape, tools: &[Shape]) -> KResult<OpResult> {
        self.op(&Request::BooleanMany(op, target.clone(), tools.to_vec()))
    }

    fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> KResult<OpResult> {
        self.op(&Request::Fillet(shape.clone(), edges.to_vec(), radius))
    }

    fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> KResult<OpResult> {
        self.op(&Request::Chamfer(shape.clone(), edges.to_vec(), distance))
    }

    fn shell(&self, shape: &Shape, remove: &[u32], thickness: f64) -> KResult<OpResult> {
        self.op(&Request::Shell(shape.clone(), remove.to_vec(), thickness))
    }

    fn transform(&self, shape: &Shape, t: &Transform) -> KResult<OpResult> {
        self.op(&Request::Transform(shape.clone(), *t))
    }

    fn compound(&self, shapes: &[Shape]) -> KResult<OpResult> {
        self.op(&Request::Compound(shapes.to_vec()))
    }

    fn topology(&self, shape: &Shape) -> KResult<Topology> {
        match self.call(&Request::Topology(shape.clone()))? {
            Reply::Topology(t) => Ok(t),
            _ => Err(unexpected()),
        }
    }

    fn mass_properties(&self, shape: &Shape) -> KResult<MassProps> {
        match self.call(&Request::Mass(shape.clone()))? {
            Reply::Mass(m) => Ok(m),
            _ => Err(unexpected()),
        }
    }

    fn bbox(&self, shape: &Shape) -> KResult<BBox> {
        match self.call(&Request::BBox(shape.clone()))? {
            Reply::BBox(b) => Ok(b),
            _ => Err(unexpected()),
        }
    }

    fn tessellate(&self, shape: &Shape, params: &TessellationParams) -> KResult<Mesh> {
        match self.call(&Request::Tessellate(shape.clone(), *params))? {
            Reply::Mesh(m) => Ok(m),
            _ => Err(unexpected()),
        }
    }

    fn export_step(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
        self.bytes(&Request::ExportStep(named(bodies)))
    }

    fn import_step(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
        self.bodies(&Request::ImportStep(data.to_vec()))
    }

    fn export_iges(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
        self.bytes(&Request::ExportIges(named(bodies)))
    }

    fn import_iges(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
        self.bodies(&Request::ImportIges(data.to_vec()))
    }

    fn write_native(&self, shape: &Shape) -> KResult<Vec<u8>> {
        self.bytes(&Request::WriteNative(shape.clone()))
    }

    fn read_native(&self, data: &[u8]) -> KResult<Shape> {
        match self.call(&Request::ReadNative(data.to_vec()))? {
            Reply::Shape(s) => Ok(s),
            _ => Err(unexpected()),
        }
    }
}
