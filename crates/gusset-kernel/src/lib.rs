//! The kernel-agnostic geometry interface for Gusset (ADR 0002).
//!
//! Everything above this crate (naming, evaluation, document, UI, MCP) talks to geometry
//! only through [`Kernel`] and the plain-data types in [`types`]. Shapes are opaque
//! [`Shape`] handles owned by the kernel implementation.
//!
//! [`mock::MockKernel`] is a small exact pure-Rust kernel (polygonal prisms) used to unit
//! test the layers above without a C++ toolchain.

#![cfg_attr(test, allow(clippy::float_cmp))]

pub mod mock;
pub mod types;

pub use types::*;

use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// Implementation-specific shape data. Kernels downcast through [`ShapeData::as_any`].
pub trait ShapeData: Any + Send + Sync + Debug {
    fn as_any(&self) -> &dyn Any;
    /// Name of the kernel that created the shape; mixing kernels is an error.
    fn kernel(&self) -> &'static str;
}

/// An immutable, cheaply clonable shape handle.
#[derive(Debug, Clone)]
pub struct Shape(pub Arc<dyn ShapeData>);

impl Shape {
    pub fn new<T: ShapeData>(data: T) -> Self {
        Shape(Arc::new(data))
    }

    /// Downcasts to the implementation type, or returns a structured error.
    pub fn downcast<T: ShapeData>(&self) -> KResult<&T> {
        self.0.as_any().downcast_ref::<T>().ok_or_else(|| {
            KernelError::new(
                KernelErrorKind::Internal,
                format!("shape was created by kernel '{}' and cannot be used here", self.0.kernel()),
            )
        })
    }

    /// Identity comparison (same underlying shape object).
    pub fn ptr_eq(&self, other: &Shape) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub type KResult<T> = Result<T, KernelError>;

fn unsupported<T>(what: &str, kernel: &str) -> KResult<T> {
    Err(KernelError::new(KernelErrorKind::Unsupported, format!("{what} is not supported by the {kernel} kernel")))
}

/// The geometry kernel interface.
///
/// Conventions:
/// - All lengths are millimetres, angles radians.
/// - For operations with inputs, [`FaceOrigin::Input`] / [`FaceOrigin::FromEdge`] index
///   inputs in argument order (boolean: 0 = target, 1 = tool; fillet/chamfer: 0 = shape).
/// - Edge and face indices refer to [`Kernel::topology`] of the given shape.
/// - Results are validated; an invalid solid is an [`KernelErrorKind::InvalidResult`] error.
/// - Model-defining operations must be deterministic: same inputs, same output topology
///   order and same geometry on every platform (within documented tolerance).
pub trait Kernel: Send + Sync + Debug {
    fn name(&self) -> &'static str;

    /// Linear sweep of a planar profile along its normal.
    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> KResult<OpResult>;

    /// Rotational sweep of a planar profile around an in-plane axis.
    fn revolve(&self, profile: &Profile, spec: &RevolveSpec) -> KResult<OpResult> {
        let _ = (profile, spec);
        unsupported("revolve", self.name())
    }

    fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> KResult<OpResult> {
        let _ = (op, target, tool);
        unsupported("boolean operations", self.name())
    }

    fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> KResult<OpResult> {
        let _ = (shape, edges, radius);
        unsupported("fillet", self.name())
    }

    fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> KResult<OpResult> {
        let _ = (shape, edges, distance);
        unsupported("chamfer", self.name())
    }

    /// Combines several solids into one multi-body shape without merging them.
    fn compound(&self, shapes: &[Shape]) -> KResult<OpResult>;

    fn topology(&self, shape: &Shape) -> KResult<Topology>;
    fn mass_properties(&self, shape: &Shape) -> KResult<MassProps>;
    fn bbox(&self, shape: &Shape) -> KResult<BBox>;
    fn tessellate(&self, shape: &Shape, params: &TessellationParams) -> KResult<Mesh>;

    /// Writes STEP (the kernel's default schema: AP214 for OCCT) with one named body per entry.
    fn export_step(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
        let _ = bodies;
        unsupported("STEP export", self.name())
    }
    /// Reads STEP. Must only be called inside the sandboxed import worker (ADR 0008).
    fn import_step(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
        let _ = data;
        unsupported("STEP import", self.name())
    }
    fn export_iges(&self, bodies: &[(&Shape, &str)]) -> KResult<Vec<u8>> {
        let _ = bodies;
        unsupported("IGES export", self.name())
    }
    fn import_iges(&self, data: &[u8]) -> KResult<Vec<(Shape, String)>> {
        let _ = data;
        unsupported("IGES import", self.name())
    }
    /// Serializes a shape losslessly in the kernel's native exchange form (e.g. OCCT
    /// BRep). Used to pass imported geometry from the import worker to the app and to
    /// store imported B-rep blobs.
    fn write_native(&self, shape: &Shape) -> KResult<Vec<u8>> {
        let _ = shape;
        unsupported("native shape serialization", self.name())
    }
    fn read_native(&self, data: &[u8]) -> KResult<Shape> {
        let _ = data;
        unsupported("native shape serialization", self.name())
    }
}
