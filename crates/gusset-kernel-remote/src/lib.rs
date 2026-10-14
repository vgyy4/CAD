//! The kernel worker protocol (ADR 0022): a [`gusset_kernel::Kernel`] served over a byte
//! stream by a worker process ([`Server`]), and the app-side [`RemoteKernel`] that
//! implements `Kernel` by talking to it.
//!
//! Geometry then runs in a process the app can lose without dying: a crash, a stack
//! overflow or an allocation failure inside the geometry kernel ends the worker, the call
//! returns [`gusset_kernel::KernelErrorKind::Crashed`], and the next call starts a new
//! worker. This crate knows nothing about processes: the worker binary and the
//! [`Transport`] that starts it live in `gusset-worker`, so everything here is tested in
//! one process with the mock kernel.

mod client;
mod frame;
mod pool;
mod proto;
mod server;

pub use client::{CANCEL_GRACE, Connect, Interrupt, NativeGate, RemoteKernel, RemoteShape, Transport, TransportError};
pub use frame::{MAX_FRAME, MAX_REPLY};
pub use pool::{PoolKernel, with_slot};
pub use proto::{Fault, VERSION};
pub use server::Server;

use std::io::{Read, Write};

/// A [`Transport`] over a pair of byte streams to a worker (its stdin and stdout).
#[derive(Debug)]
pub struct StreamTransport<W: Write + Send + std::fmt::Debug, R: Read + Send + std::fmt::Debug> {
    to_worker: W,
    from_worker: R,
}

impl<W: Write + Send + std::fmt::Debug, R: Read + Send + std::fmt::Debug> StreamTransport<W, R> {
    pub fn new(to_worker: W, from_worker: R) -> Self {
        StreamTransport { to_worker, from_worker }
    }
}

impl<W: Write + Send + std::fmt::Debug, R: Read + Send + std::fmt::Debug> Transport for StreamTransport<W, R> {
    fn call(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError> {
        let died = |e: std::io::Error| TransportError::Died { reason: e.to_string() };
        frame::write_frame(&mut self.to_worker, frame::REQUEST_MAGIC, request).map_err(died)?;
        match frame::read_frame(&mut self.from_worker, frame::RESPONSE_MAGIC, frame::MAX_REPLY) {
            Ok(Some(reply)) => Ok(reply),
            Ok(None) => Err(TransportError::Died { reason: "the worker closed its output".into() }),
            Err(e) => Err(died(e)),
        }
    }
}

#[cfg(test)]
mod tests;
