//! Maps the shim's safe wrappers (`gusset_occt_sys::safe`) to kernel errors.
//!
//! All `unsafe` code lives in `gusset-occt-sys` (ADR 0008); this crate forbids it.

use gusset_kernel::{KResult, KernelError, KernelErrorKind};
use gusset_occt_sys::safe::{self, ShimError};

pub(crate) use safe::{OpOut, ProfileData, RawShape};

fn err(kind: KernelErrorKind) -> impl Fn(ShimError) -> KernelError {
    move |e| match e {
        ShimError::Failed(m) => KernelError::new(kind, m),
        ShimError::NoShape => KernelError::new(KernelErrorKind::Internal, e.to_string()),
    }
}

pub(crate) fn version() -> String {
    safe::version()
}

pub(crate) fn extrude(p: &ProfileData, distance: f64, symmetric: bool) -> KResult<OpOut> {
    safe::extrude(p, distance, symmetric).map_err(err(KernelErrorKind::OperationFailed))
}

pub(crate) fn revolve(p: &ProfileData, axis_origin: [f64; 2], axis_dir: [f64; 2], angle: f64) -> KResult<OpOut> {
    safe::revolve(p, axis_origin, axis_dir, angle).map_err(err(KernelErrorKind::OperationFailed))
}

pub(crate) fn boolean(op: i32, a: &RawShape, b: &RawShape) -> KResult<OpOut> {
    safe::boolean(op, a, b).map_err(err(KernelErrorKind::OperationFailed))
}

pub(crate) fn blend(s: &RawShape, edges: &[u32], size: f64, fillet: bool) -> KResult<OpOut> {
    safe::blend(s, edges, size, fillet).map_err(err(KernelErrorKind::OperationFailed))
}

pub(crate) fn compound(shapes: &[&RawShape]) -> KResult<OpOut> {
    safe::compound(shapes).map_err(err(KernelErrorKind::OperationFailed))
}

pub(crate) fn topology(s: &RawShape) -> KResult<safe::RawTopology> {
    safe::topology(s).map_err(err(KernelErrorKind::Internal))
}

pub(crate) fn mass(s: &RawShape) -> KResult<Vec<f64>> {
    safe::mass(s).map_err(err(KernelErrorKind::Internal))
}

pub(crate) fn bbox(s: &RawShape) -> KResult<Vec<f64>> {
    safe::bbox(s).map_err(err(KernelErrorKind::Internal))
}

pub(crate) fn tessellate(s: &RawShape, lin: f64, ang: f64) -> KResult<safe::RawMesh> {
    safe::tessellate(s, lin, ang).map_err(err(KernelErrorKind::Internal))
}

pub(crate) fn step_write(shapes: &[&RawShape], names: &[&str]) -> KResult<Vec<u8>> {
    safe::step_write(shapes, names).map_err(err(KernelErrorKind::Io))
}

pub(crate) fn iges_write(shapes: &[&RawShape]) -> KResult<Vec<u8>> {
    safe::iges_write(shapes).map_err(err(KernelErrorKind::Io))
}

pub(crate) fn read_shapes(data: &[u8], iges: bool) -> KResult<Vec<RawShape>> {
    safe::read_shapes(data, iges).map_err(err(KernelErrorKind::Io))
}

pub(crate) fn brep_write(s: &RawShape) -> KResult<Vec<u8>> {
    safe::brep_write(s).map_err(err(KernelErrorKind::Io))
}

pub(crate) fn brep_read(data: &[u8]) -> KResult<RawShape> {
    safe::brep_read(data).map_err(err(KernelErrorKind::Io))
}
