//! The kernel-agnostic geometry interface for Gusset (ADR 0002).
//!
//! Everything above this crate (naming, evaluation, document, UI, MCP) talks to geometry
//! only through [`Kernel`] and the plain-data types in [`types`]. Shapes are opaque
//! [`Shape`] handles owned by the kernel implementation.
//!
//! [`mock::MockKernel`] is a small exact pure-Rust kernel (polygonal prisms) used to unit
//! test the layers above without a C++ toolchain.

#![cfg_attr(test, allow(clippy::float_cmp))]

pub mod cancel;
pub mod mock;
pub mod slot;
pub mod types;
pub mod wire;

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

/// The path of a sweep.
#[derive(Debug, Clone)]
pub enum SweepPath {
    /// A chain of sketch curves.
    Curves(Path2),
    /// Edges of an existing shape (indices into its topology), in any order, forming one chain.
    Edges { shape: Shape, edges: Vec<u32> },
}

fn unsupported<T>(what: &str, kernel: &str) -> KResult<T> {
    Err(KernelError::new(KernelErrorKind::Unsupported, format!("{what} is not supported by the {kernel} kernel")))
}

/// [`Kernel::boolean_many`] as one [`Kernel::boolean`] per tool, with the face history of
/// every step composed so that origins refer to the original target and tools.
pub fn sequential_boolean<K: Kernel + ?Sized>(k: &K, op: BoolOp, target: &Shape, tools: &[Shape]) -> KResult<OpResult> {
    let internal = |what: &str| KernelError::new(KernelErrorKind::Internal, what.to_string());
    let mut cur: Option<OpResult> = None;
    for (i, tool) in tools.iter().enumerate() {
        let tool_no = u32::try_from(i + 1).map_err(|_| internal("too many tools"))?;
        let from = cur.as_ref().map_or(target, |r| &r.shape);
        let step = k.boolean(op, from, tool)?;
        let mut origins = Vec::with_capacity(step.face_origins.len());
        for os in &step.face_origins {
            let mut out: Vec<FaceOrigin> = Vec::new();
            for o in os {
                match (*o, cur.as_ref()) {
                    // A face of the previous result: its own origins.
                    (FaceOrigin::Input { input: 0, face }, Some(prev)) => {
                        let before =
                            prev.face_origins.get(face as usize).ok_or_else(|| internal("bad face history"))?;
                        out.extend(before.iter().copied());
                    }
                    (FaceOrigin::Input { input: 0, face }, None) => out.push(FaceOrigin::Input { input: 0, face }),
                    (FaceOrigin::Input { input: 1, face }, _) => out.push(FaceOrigin::Input { input: tool_no, face }),
                    _ => return Err(internal("a boolean operation reported a face origin it cannot have")),
                }
            }
            out.sort_unstable();
            out.dedup();
            origins.push(out);
        }
        cur = Some(OpResult { shape: step.shape, face_origins: origins });
    }
    match cur {
        Some(r) => Ok(r),
        None => {
            let faces = k.topology(target)?.faces.len();
            let face_origins = (0..faces)
                .map(|f| u32::try_from(f).map(|face| vec![FaceOrigin::Input { input: 0, face }]))
                .collect::<Result<_, _>>()
                .map_err(|_| internal("too many faces"))?;
            Ok(OpResult { shape: target.clone(), face_origins })
        }
    }
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

    /// Changes whenever shapes made earlier became invalid: a kernel running in a worker
    /// process returns its worker generation, which increases with every restart
    /// (ADR 0022). Callers that cache shapes drop them when it changes. In-process
    /// kernels never invalidate their shapes.
    fn epoch(&self) -> u64 {
        0
    }

    /// Notices a kernel worker that ended while idle, so that [`Kernel::epoch`] changes
    /// before the next operation instead of that operation being blamed for the crash.
    /// Callers that cache shapes call it before building. In-process kernels do nothing.
    fn probe(&self) {}

    /// How many operations this kernel can run at the same time (several worker processes,
    /// ADR 0024 §3). Callers may build that many independent features in parallel.
    fn parallelism(&self) -> usize {
        1
    }

    /// A line about where and how the kernel runs (a worker's sandbox report), if it has
    /// one; for `self-test` and Settings.
    fn diagnostics(&self) -> Option<String> {
        None
    }

    /// Linear sweep of a planar profile along its normal.
    fn extrude(&self, profile: &Profile, spec: &ExtrudeSpec) -> KResult<OpResult>;

    /// Rotational sweep of a planar profile around an in-plane axis.
    fn revolve(&self, profile: &Profile, spec: &RevolveSpec) -> KResult<OpResult> {
        let _ = (profile, spec);
        unsupported("revolve", self.name())
    }

    /// Sweeps a planar profile along a path. The path must start on the profile's plane
    /// (the end that does is taken as its start); a closed path may cross it anywhere.
    /// Lateral faces report [`FaceOrigin::Swept`], the caps [`FaceOrigin::Cap`] (a closed
    /// path has none).
    fn sweep(&self, profile: &Profile, path: &SweepPath, spec: &SweepSpec) -> KResult<OpResult> {
        let _ = (profile, path, spec);
        unsupported("sweep", self.name())
    }

    /// Lofts a solid through two or more profiles (one region without holes each), in
    /// order. Lateral faces report [`FaceOrigin::Section`] (possibly from several
    /// sections), the first and last profile give [`FaceOrigin::Cap`] start and end.
    fn loft(&self, sections: &[Profile], spec: &LoftSpec) -> KResult<OpResult> {
        let _ = (sections, spec);
        unsupported("loft", self.name())
    }

    fn boolean(&self, op: BoolOp, target: &Shape, tool: &Shape) -> KResult<OpResult> {
        let _ = (op, target, tool);
        unsupported("boolean operations", self.name())
    }

    /// Applies `op` to `target` with every shape of `tools`, as if one tool after the
    /// other were applied with [`Kernel::boolean`] (review H3: kernels should do it in one
    /// operation). [`FaceOrigin::Input`] index 0 is the target and `i + 1` is `tools[i]`.
    ///
    /// For [`BoolOp::Intersect`] this means the part of the target inside *every* tool.
    /// With no tools, the result is the target itself.
    ///
    /// The default runs [`Kernel::boolean`] once per tool and composes the face histories.
    fn boolean_many(&self, op: BoolOp, target: &Shape, tools: &[Shape]) -> KResult<OpResult> {
        sequential_boolean(self, op, target, tools)
    }

    fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> KResult<OpResult> {
        let _ = (shape, edges, radius);
        unsupported("fillet", self.name())
    }

    fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> KResult<OpResult> {
        let _ = (shape, edges, distance);
        unsupported("chamfer", self.name())
    }

    /// Hollows a solid: removes the faces `remove` (indices into the shape's topology) and
    /// leaves walls of `thickness` mm (> 0 grows inward, < 0 outward). Faces that
    /// remain on the outside report [`FaceOrigin::Input`]; the new inner faces report
    /// [`FaceOrigin::Offset`] (or `FromEdge`/`FromVertex` for joins).
    fn shell(&self, shape: &Shape, remove: &[u32], thickness: f64) -> KResult<OpResult> {
        let _ = (shape, remove, thickness);
        unsupported("shell", self.name())
    }

    /// Tapers the faces `faces` (indices into the shape's topology) about their
    /// intersection with the neutral plane (ADR 0028). Every output face reports
    /// [`FaceOrigin::Input`]: drafted and neighbouring faces are modified, none are added.
    fn draft(&self, shape: &Shape, faces: &[u32], spec: &DraftSpec) -> KResult<OpResult> {
        let _ = (shape, faces, spec);
        unsupported("draft", self.name())
    }

    /// Moves the faces `faces` along their normals by `distance` mm (> 0 outward, adding
    /// material; ADR 0030); the neighbouring faces stretch to follow. Every output face
    /// reports [`FaceOrigin::Input`].
    fn offset_faces(&self, shape: &Shape, faces: &[u32], distance: f64) -> KResult<OpResult> {
        let _ = (shape, faces, distance);
        unsupported("offset face", self.name())
    }

    /// A transformed copy of a shape. Output face *i* is input face *i*
    /// (`face_origins[i] == [Input { input: 0, face: i }]`); a mirror keeps outward
    /// normals outward.
    fn transform(&self, shape: &Shape, t: &Transform) -> KResult<OpResult> {
        let _ = (shape, t);
        unsupported("transform", self.name())
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
