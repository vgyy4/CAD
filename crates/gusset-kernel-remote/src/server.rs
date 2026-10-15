//! The worker side: a [`Kernel`] served over a byte stream. It owns every shape it made
//! and hands out ids; the client frees them.

use crate::frame::{REQUEST_MAGIC, RESPONSE_MAGIC, read_frame, write_frame};
use crate::proto::{Fault, Reply, Request, VERSION};
use gusset_kernel::KernelErrorKind;
use gusset_kernel::wire::{NoShapes, ShapeRefs, WireError};
use gusset_kernel::{Kernel, KernelError, Shape};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Read, Write};

/// Shapes the worker holds, by id.
#[derive(Debug, Default)]
struct Table {
    shapes: RefCell<HashMap<u64, Shape>>,
    next: RefCell<u64>,
}

impl ShapeRefs for Table {
    /// Every shape in a reply is registered under a fresh id.
    fn id_of(&self, shape: &Shape) -> Result<u64, WireError> {
        let mut next = self.next.borrow_mut();
        *next += 1;
        let id = *next;
        self.shapes.borrow_mut().insert(id, shape.clone());
        Ok(id)
    }

    fn shape(&self, id: u64) -> Result<Shape, WireError> {
        self.shapes.borrow().get(&id).cloned().ok_or_else(|| WireError(format!("unknown shape {id}")))
    }
}

/// Serves one kernel. Not `Sync`: one request at a time, in order.
#[derive(Debug)]
pub struct Server<K: Kernel> {
    kernel: K,
    table: Table,
    status: String,
}

impl<K: Kernel> Server<K> {
    #[must_use]
    pub fn new(kernel: K) -> Self {
        Server { kernel, table: Table::default(), status: String::new() }
    }

    /// What the worker reports when asked for its status: the sandbox it runs in.
    #[must_use]
    pub fn with_status(mut self, status: String) -> Self {
        self.status = status;
        self
    }

    /// Number of shapes currently held (tests: the client frees what it drops).
    #[must_use]
    pub fn held(&self) -> usize {
        self.table.shapes.borrow().len()
    }

    /// Answers one request payload with one reply payload.
    #[must_use]
    pub fn handle(&self, request: &[u8]) -> Vec<u8> {
        let result = Request::decode(request, &self.table).map_err(KernelError::from).and_then(|r| self.execute(r));
        let first = *self.table.next.borrow() + 1;
        Reply::encode(&result, &self.table).unwrap_or_else(|e| {
            // The shapes registered for the reply that could not be sent would never be
            // freed by the client (review N2).
            self.table.shapes.borrow_mut().retain(|id, _| *id < first);
            Reply::encode(&Err(KernelError::from(e)), &NoShapes).unwrap_or_default()
        })
    }

    /// Serves frames from `input` until it ends cleanly.
    ///
    /// # Errors
    /// A broken stream or a malformed frame.
    pub fn serve(&self, input: &mut dyn Read, output: &mut dyn Write) -> io::Result<()> {
        while let Some(frame) = read_frame(input, REQUEST_MAGIC, crate::frame::MAX_FRAME)? {
            write_frame(output, RESPONSE_MAGIC, &self.handle(&frame))?;
        }
        Ok(())
    }

    fn execute(&self, req: Request) -> Result<Reply, KernelError> {
        let k = &self.kernel;
        Ok(match req {
            Request::Ping => Reply::Version(VERSION),
            Request::Status => Reply::Bytes(self.status.clone().into_bytes()),
            Request::Free(ids) => {
                let mut shapes = self.table.shapes.borrow_mut();
                ids.iter().for_each(|id| {
                    shapes.remove(id);
                });
                Reply::Unit
            }
            Request::Extrude(p, spec) => Reply::Op(k.extrude(&p, &spec)?),
            Request::Revolve(p, spec) => Reply::Op(k.revolve(&p, &spec)?),
            Request::Sweep(p, path, spec) => Reply::Op(k.sweep(&p, &path, &spec)?),
            Request::Loft(sections, spec) => Reply::Op(k.loft(&sections, &spec)?),
            Request::Boolean(op, a, b) => Reply::Op(k.boolean(op, &a, &b)?),
            Request::BooleanMany(op, a, tools) => Reply::Op(k.boolean_many(op, &a, &tools)?),
            Request::Fillet(s, edges, r) => Reply::Op(k.fillet(&s, &edges, r)?),
            Request::Chamfer(s, edges, d) => Reply::Op(k.chamfer(&s, &edges, d)?),
            Request::Shell(s, faces, t) => Reply::Op(k.shell(&s, &faces, t)?),
            Request::Draft(s, faces, spec) => Reply::Op(k.draft(&s, &faces, &spec)?),
            Request::Transform(s, t) => Reply::Op(k.transform(&s, &t)?),
            Request::Compound(shapes) => Reply::Op(k.compound(&shapes)?),
            Request::Topology(s) => Reply::Topology(k.topology(&s)?),
            Request::Mass(s) => Reply::Mass(k.mass_properties(&s)?),
            Request::BBox(s) => Reply::BBox(k.bbox(&s)?),
            Request::Tessellate(s, params) => Reply::Mesh(k.tessellate(&s, &params)?),
            Request::ExportStep(bodies) => {
                let refs: Vec<(&Shape, &str)> = bodies.iter().map(|(s, n)| (s, n.as_str())).collect();
                Reply::Bytes(k.export_step(&refs)?)
            }
            Request::ExportIges(bodies) => {
                let refs: Vec<(&Shape, &str)> = bodies.iter().map(|(s, n)| (s, n.as_str())).collect();
                Reply::Bytes(k.export_iges(&refs)?)
            }
            // Untrusted files are parsed by the import worker, never next to the
            // session's shapes (review L4); the requests stay in the protocol, refused.
            Request::ImportStep(_) | Request::ImportIges(_) => {
                return Err(KernelError::new(
                    KernelErrorKind::Unsupported,
                    "files are imported by the sandboxed import worker, not by the kernel worker",
                ));
            }
            Request::WriteNative(s) => Reply::Bytes(k.write_native(&s)?),
            Request::ReadNative(data) => Reply::Shape(k.read_native(&data)?),
            Request::Fault(f) => inject(f)?,
        })
    }
}

#[cfg(feature = "fault-injection")]
fn inject(f: Fault) -> Result<Reply, KernelError> {
    match f {
        Fault::Abort => std::process::abort(),
        Fault::StackOverflow => {
            #[allow(unconditional_recursion)] // the point: overflow the stack
            fn deeper(n: u64) -> u64 {
                let pad = std::hint::black_box([n; 64]);
                deeper(n + 1).wrapping_add(pad.iter().sum::<u64>())
            }
            Ok(Reply::Version(u32::try_from(deeper(0) % 2).unwrap_or(0)))
        }
        Fault::ExhaustMemory => {
            let mut hog: Vec<Vec<u8>> = Vec::new();
            loop {
                hog.push(std::hint::black_box(vec![1u8; 64 << 20]));
            }
        }
        Fault::Sleep(ms) => {
            std::thread::sleep(std::time::Duration::from_millis(u64::from(ms)));
            Ok(Reply::Unit)
        }
        Fault::Exit => std::process::exit(70),
    }
}

#[cfg(not(feature = "fault-injection"))]
fn inject(_: Fault) -> Result<Reply, KernelError> {
    Err(KernelError::new(KernelErrorKind::Unsupported, "fault injection is not built in"))
}
