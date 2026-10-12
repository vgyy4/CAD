//! Plain-data geometry types shared by every kernel implementation.

use std::fmt;

pub type P2 = [f64; 2];
pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
pub fn normalize(a: V3) -> Option<V3> {
    let n = norm(a);
    if n > 1e-12 && n.is_finite() { Some(scale(a, 1.0 / n)) } else { None }
}

/// A right-handed placement: sketch planes, feature frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: V3,
    /// Unit X direction.
    pub x: V3,
    /// Unit Y direction, orthogonal to `x`.
    pub y: V3,
}

impl Frame {
    pub const XY: Frame = Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    pub const XZ: Frame = Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0] };
    pub const YZ: Frame = Frame { origin: [0.0; 3], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };

    /// Builds a frame from an origin and two (not necessarily orthonormal) directions.
    pub fn new(origin: V3, x: V3, y_hint: V3) -> Option<Frame> {
        let x = normalize(x)?;
        let z = normalize(cross(x, y_hint))?;
        let y = cross(z, x);
        Some(Frame { origin, x, y })
    }

    pub fn normal(&self) -> V3 {
        cross(self.x, self.y)
    }

    /// Maps a point in frame coordinates to world coordinates.
    pub fn to_world(&self, p: P2) -> V3 {
        add(self.origin, add(scale(self.x, p[0]), scale(self.y, p[1])))
    }

    pub fn is_valid(&self) -> bool {
        let ok = |v: V3| v.iter().all(|c| c.is_finite());
        ok(self.origin)
            && ok(self.x)
            && ok(self.y)
            && (norm(self.x) - 1.0).abs() < 1e-9
            && (norm(self.y) - 1.0).abs() < 1e-9
            && dot(self.x, self.y).abs() < 1e-9
    }
}

/// A 2D curve segment of a profile, in the profile frame's coordinates (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg2 {
    Line {
        a: P2,
        b: P2,
    },
    /// Arc from `start` to `end` around `center`; `ccw` gives the direction.
    Arc {
        center: P2,
        start: P2,
        end: P2,
        ccw: bool,
    },
    /// A full circle (forms a loop on its own).
    Circle {
        center: P2,
        radius: f64,
    },
    /// One cubic Bézier segment from `p[0]` to `p[3]`. A spline is a chain of these (one
    /// profile edge each, so every segment gets its own lateral face and exact mass
    /// properties: OCCT integrates multi-span B-spline edges too coarsely).
    Bezier {
        p: [P2; 4],
    },
}

/// A profile edge plus the persistent tag of the sketch entity it came from.
/// Kernels propagate tags into [`FaceOrigin::Lateral`], which is how lateral faces get
/// stable names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileEdge {
    pub seg: Seg2,
    pub tag: u64,
}

/// A closed, ordered loop of edges. End of edge *i* is the start of edge *i+1*.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loop2 {
    pub edges: Vec<ProfileEdge>,
}

/// A planar face region: one outer loop and any number of holes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Region2 {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
}

/// A planar profile placed in space.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub frame: Frame,
    pub regions: Vec<Region2>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoolOp {
    Union,
    Cut,
    Intersect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrudeSpec {
    /// Distance along the profile normal (negative extrudes the other way).
    pub distance: f64,
    /// Extrude half the distance to each side.
    pub symmetric: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RevolveSpec {
    /// Axis origin and direction in profile coordinates.
    pub axis_origin: P2,
    pub axis_dir: P2,
    /// Sweep angle in radians, (0, 2π].
    pub angle: f64,
}

/// How a sweep orients its profile along the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SweepOrientation {
    /// The profile keeps its orientation and only slides along the path.
    Fixed,
    /// The profile turns with the path (a corrected Frenet frame: no flips on straight
    /// stretches or at inflection points).
    Frenet,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepSpec {
    pub orientation: SweepOrientation,
    /// Total turn of the profile about the path between its start and end, radians
    /// (spread evenly along the path length). Needs [`SweepOrientation::Frenet`] and an
    /// open path.
    pub twist: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoftSpec {
    /// Ruled surfaces between neighbouring profiles (sharp transitions), else a smooth
    /// surface through all of them.
    pub ruled: bool,
}

/// A connected chain of curves in a plane: the path of a sweep. The edges run from the
/// path's start to its end; each ends where the next begins (a closed path ends where it
/// began). The edge tags are not used.
#[derive(Debug, Clone, PartialEq)]
pub struct Path2 {
    pub frame: Frame,
    pub edges: Vec<ProfileEdge>,
}

/// Which side of a sweep a cap face sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CapEnd {
    Start,
    End,
}

/// Where an output face came from. One face can have several origins (e.g. a face
/// that was modified by a boolean keeps its original origin through `Input`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FaceOrigin {
    /// Swept from the profile edge with this tag.
    Lateral { tag: u64 },
    /// Start or end cap of a sweep; `region` indexes [`Profile::regions`].
    Cap { end: CapEnd, region: u32 },
    /// The face `face` of input shape `input` (unchanged or modified).
    Input { input: u32, face: u32 },
    /// A blend face (fillet/chamfer) generated from edge `edge` of input `input`.
    FromEdge { input: u32, edge: u32 },
    /// A face generated from a vertex (e.g. corner blends).
    FromVertex { input: u32, vertex: u32 },
    /// The inner offset of face `face` of input `input` (the wall's inside after a shell).
    Offset { input: u32, face: u32 },
    /// Swept from the profile edge with this tag along edge `path` of the sweep path
    /// (0-based from the path's start; a path with one edge always reports 0).
    Swept { tag: u64, path: u32 },
    /// Generated by the edge with this tag of loft section `section` (0-based).
    Section { section: u32, tag: u64 },
}

/// A rigid or mirror transform applied to a copy of a shape. Lengths in millimetres,
/// angles in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transform {
    /// Reflection across the plane through `origin` with unit-length-able `normal`.
    Mirror {
        origin: V3,
        normal: V3,
    },
    Translate(V3),
    /// Rotation about the axis through `origin` along `axis` (right-hand rule).
    Rotate {
        origin: V3,
        axis: V3,
        angle: f64,
    },
}

/// Coarse surface classification, used for naming keys and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    BSpline,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurveKind {
    Line,
    Circle,
    Ellipse,
    BSpline,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FaceInfo {
    pub kind: SurfaceKind,
    pub area: f64,
    pub centroid: V3,
    /// Outward normal at (or near) the centroid; for non-planar faces the normal at the
    /// parametric center.
    pub normal: V3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeInfo {
    pub kind: CurveKind,
    pub length: f64,
    pub midpoint: V3,
    /// Indices of the (one or two) adjacent faces, sorted ascending.
    pub faces: Vec<u32>,
    /// Indices of the end vertices (one for closed edges).
    pub vertices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VertexInfo {
    pub point: V3,
}

/// Indexed topology of a shape. Indices are stable for a given shape value and are
/// what [`FaceOrigin`] and the edge/face arguments of operations refer to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Topology {
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
    pub vertices: Vec<VertexInfo>,
    pub solids: u32,
    pub shells: u32,
}

/// Result of a modeling operation.
#[derive(Debug, Clone)]
pub struct OpResult {
    pub shape: crate::Shape,
    /// For every face index of the output topology, its origins (sorted, deduplicated).
    pub face_origins: Vec<Vec<FaceOrigin>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProps {
    pub volume: f64,
    pub area: f64,
    pub centroid: V3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min: V3,
    pub max: V3,
}

impl BBox {
    pub fn size(&self) -> V3 {
        sub(self.max, self.min)
    }
}

/// Triangle mesh for display/export. `face_ranges[i]` is the triangle range of face `i`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub face_ranges: Vec<std::ops::Range<u32>>,
    /// Polyline per topological edge (for drawing edges and edge picking).
    pub edges: Vec<Vec<[f32; 3]>>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TessellationParams {
    /// Max distance between mesh and surface, in mm.
    pub linear_deflection: f64,
    /// Max angle between adjacent facet normals, in radians.
    pub angular_deflection: f64,
}

impl Default for TessellationParams {
    fn default() -> Self {
        TessellationParams { linear_deflection: 0.05, angular_deflection: 0.35 }
    }
}

/// A structured kernel error with a human message and an optional fix hint.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelError {
    pub kind: KernelErrorKind,
    pub message: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelErrorKind {
    InvalidInput,
    OperationFailed,
    InvalidResult,
    Unsupported,
    Cancelled,
    Io,
    Internal,
}

impl KernelError {
    pub fn new(kind: KernelErrorKind, message: impl Into<String>) -> Self {
        KernelError { kind, message: message.into(), hint: None }
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for KernelError {}
