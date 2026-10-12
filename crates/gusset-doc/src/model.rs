//! The document model (ADR 0004): parameters plus an ordered list of features holding
//! only *inputs*. Derived geometry never lives here.

use crate::refs::{EdgeRef, FaceRef};
use gusset_units::LengthUnit;
use gusset_units::expr::Expr;
use gusset_units::params::Param;

/// Current schema version written by this build (`gusset <N>` header line).
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub units: LengthUnit,
    pub params: Vec<Param>,
    pub features: Vec<Feature>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// Identifier used in references (`Extrude1`). Unique in the document.
    pub name: String,
    /// Free-text label shown in the UI (untrusted user content).
    pub label: Option<String>,
    /// Free-text note; printed as `#` comment lines before the statement.
    pub note: Option<String>,
    pub suppressed: bool,
    pub kind: FeatureKind,
}

impl Feature {
    pub fn new(name: impl Into<String>, kind: FeatureKind) -> Self {
        Feature { name: name.into(), label: None, note: None, suppressed: false, kind }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FeatureKind {
    Sketch(SketchDef),
    Extrude(ExtrudeDef),
    Revolve(RevolveDef),
    Fillet(EdgeBlendDef),
    Chamfer(EdgeBlendDef),
    Import(ImportDef),
    Shell(ShellDef),
    Mirror(MirrorDef),
    PatternLinear(PatternLinearDef),
    PatternCircular(PatternCircularDef),
}

impl FeatureKind {
    pub fn keyword(&self) -> &'static str {
        match self {
            FeatureKind::Sketch(_) => "sketch",
            FeatureKind::Extrude(_) => "extrude",
            FeatureKind::Revolve(_) => "revolve",
            FeatureKind::Fillet(_) => "fillet",
            FeatureKind::Chamfer(_) => "chamfer",
            FeatureKind::Import(_) => "import",
            FeatureKind::Shell(_) => "shell",
            FeatureKind::Mirror(_) => "mirror",
            FeatureKind::PatternLinear(_) => "pattern_linear",
            FeatureKind::PatternCircular(_) => "pattern_circular",
        }
    }

    /// Names of the features this one refers to directly.
    pub fn dependencies(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |n: &str| {
            if !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        };
        match self {
            FeatureKind::Sketch(s) => {
                if let PlaneRef::Face(f) = &s.plane {
                    f.features(&mut add);
                }
            }
            FeatureKind::Extrude(e) => {
                add(&e.profile.sketch);
                if let Some(b) = &e.body {
                    add(b);
                }
            }
            FeatureKind::Revolve(r) => {
                add(&r.profile.sketch);
                if let AxisRef::SketchLine { sketch, .. } = &r.axis {
                    add(sketch);
                }
                if let Some(b) = &r.body {
                    add(b);
                }
            }
            FeatureKind::Fillet(b) | FeatureKind::Chamfer(b) => {
                for e in &b.edges {
                    e.features(&mut add);
                }
            }
            FeatureKind::Import(_) => {}
            FeatureKind::Shell(d) => {
                add_body(&mut add, &d.body);
                for f in &d.faces {
                    f.features(&mut add);
                }
            }
            FeatureKind::Mirror(d) => {
                add_body(&mut add, &d.body);
                if let PlaneRef::Face(f) = &d.plane {
                    f.features(&mut add);
                }
            }
            FeatureKind::PatternLinear(d) => add_body(&mut add, &d.body),
            FeatureKind::PatternCircular(d) => add_body(&mut add, &d.body),
        }
        out
    }

    /// Parameter names referenced by this feature's expressions.
    pub fn parameters(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut take = |e: &Expr| {
            for v in e.variables() {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        };
        match self {
            FeatureKind::Sketch(s) => s.constraints.iter().flat_map(|c| c.value.iter()).for_each(&mut take),
            FeatureKind::Extrude(e) => take(&e.distance),
            FeatureKind::Revolve(r) => take(&r.angle),
            FeatureKind::Fillet(b) | FeatureKind::Chamfer(b) => take(&b.size),
            FeatureKind::Import(_) => {}
            FeatureKind::Shell(d) => take(&d.thickness),
            FeatureKind::Mirror(_) => {}
            FeatureKind::PatternLinear(d) => {
                d.direction.exprs().into_iter().chain([&d.count, &d.spacing]).for_each(&mut take);
            }
            FeatureKind::PatternCircular(d) => {
                let through = d.through.iter().flatten();
                d.axis.exprs().into_iter().chain(through).chain([&d.count, &d.angle]).for_each(&mut take);
            }
        }
        out
    }
}

fn add_body(add: &mut impl FnMut(&str), body: &Option<String>) {
    if let Some(b) = body {
        add(b);
    }
}

// ------------------------------------------------------------------ sketches

#[derive(Debug, Clone, PartialEq, Default)]
pub enum PlaneRef {
    #[default]
    XY,
    XZ,
    YZ,
    /// A planar face of an earlier body.
    Face(FaceRef),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SketchDef {
    pub plane: PlaneRef,
    /// Next entity/constraint number; ids are never reused within a sketch.
    pub next_id: u32,
    pub entities: Vec<SkEntity>,
    pub constraints: Vec<SkConstraint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkEntity {
    /// `p3`, `l4`, `c5`, `a6` — prefix by kind, number from `next_id`.
    pub id: String,
    pub geom: SkGeom,
    pub construction: bool,
}

/// Sketch geometry. Coordinates and radii are solver state in **millimetres**, always,
/// independent of the document's display unit (so text round-trips bit-exactly).
#[derive(Debug, Clone, PartialEq)]
pub enum SkGeom {
    Point {
        x: f64,
        y: f64,
    },
    Line {
        a: String,
        b: String,
    },
    Circle {
        center: String,
        radius: f64,
    },
    /// Counter-clockwise from `start` to `end` around `center`.
    Arc {
        center: String,
        start: String,
        end: String,
    },
}

impl SkGeom {
    pub fn keyword(&self) -> &'static str {
        match self {
            SkGeom::Point { .. } => "point",
            SkGeom::Line { .. } => "line",
            SkGeom::Circle { .. } => "circle",
            SkGeom::Arc { .. } => "arc",
        }
    }
    pub fn id_prefix(&self) -> char {
        match self {
            SkGeom::Point { .. } => 'p',
            SkGeom::Line { .. } => 'l',
            SkGeom::Circle { .. } => 'c',
            SkGeom::Arc { .. } => 'a',
        }
    }
    /// Entity ids this geometry refers to.
    pub fn refs(&self) -> Vec<&str> {
        match self {
            SkGeom::Point { .. } => vec![],
            SkGeom::Line { a, b } => vec![a, b],
            SkGeom::Circle { center, .. } => vec![center],
            SkGeom::Arc { center, start, end } => vec![center, start, end],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkConstraintKind {
    Coincident,
    PointOnLine,
    PointOnCircle,
    Midpoint,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    Equal,
    Distance,
    Angle,
    Radius,
    Diameter,
    Fixed,
    Symmetric,
}

impl SkConstraintKind {
    pub const ALL: [SkConstraintKind; 16] = [
        SkConstraintKind::Coincident,
        SkConstraintKind::PointOnLine,
        SkConstraintKind::PointOnCircle,
        SkConstraintKind::Midpoint,
        SkConstraintKind::Horizontal,
        SkConstraintKind::Vertical,
        SkConstraintKind::Parallel,
        SkConstraintKind::Perpendicular,
        SkConstraintKind::Tangent,
        SkConstraintKind::Equal,
        SkConstraintKind::Distance,
        SkConstraintKind::Angle,
        SkConstraintKind::Radius,
        SkConstraintKind::Diameter,
        SkConstraintKind::Fixed,
        SkConstraintKind::Symmetric,
    ];

    pub fn keyword(self) -> &'static str {
        match self {
            SkConstraintKind::Coincident => "coincident",
            SkConstraintKind::PointOnLine => "on_line",
            SkConstraintKind::PointOnCircle => "on_circle",
            SkConstraintKind::Midpoint => "midpoint",
            SkConstraintKind::Horizontal => "horizontal",
            SkConstraintKind::Vertical => "vertical",
            SkConstraintKind::Parallel => "parallel",
            SkConstraintKind::Perpendicular => "perpendicular",
            SkConstraintKind::Tangent => "tangent",
            SkConstraintKind::Equal => "equal",
            SkConstraintKind::Distance => "distance",
            SkConstraintKind::Angle => "angle",
            SkConstraintKind::Radius => "radius",
            SkConstraintKind::Diameter => "diameter",
            SkConstraintKind::Fixed => "fixed",
            SkConstraintKind::Symmetric => "symmetric",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.keyword() == s)
    }

    /// (number of entity arguments, takes a value expression)
    pub fn arity(self) -> (usize, bool) {
        use SkConstraintKind::*;
        match self {
            Horizontal | Vertical | Fixed => (1, false),
            Radius | Diameter => (1, true),
            Coincident | PointOnLine | PointOnCircle | Midpoint | Parallel | Perpendicular | Tangent | Equal => {
                (2, false)
            }
            Distance | Angle => (2, true),
            Symmetric => (3, false),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkConstraint {
    /// `k7` — shares the sketch's id counter.
    pub id: String,
    pub kind: SkConstraintKind,
    pub entities: Vec<String>,
    /// Dimension value for dimensional constraints.
    pub value: Option<Expr>,
}

// ------------------------------------------------------------------ solid features

/// Which closed regions of a sketch a feature uses.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRef {
    pub sketch: String,
    /// Empty = all regions. Otherwise regions identified by an entity on their outer loop.
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyOp {
    /// Create a new body.
    New,
    /// Union into the target body.
    Add,
    /// Subtract from the target body.
    Cut,
    /// Intersect with the target body.
    Intersect,
}

impl BodyOp {
    pub fn keyword(self) -> &'static str {
        match self {
            BodyOp::New => "new",
            BodyOp::Add => "add",
            BodyOp::Cut => "cut",
            BodyOp::Intersect => "intersect",
        }
    }
    pub fn from_keyword(s: &str) -> Option<Self> {
        [BodyOp::New, BodyOp::Add, BodyOp::Cut, BodyOp::Intersect].into_iter().find(|o| o.keyword() == s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeDef {
    pub profile: ProfileRef,
    pub distance: Expr,
    pub symmetric: bool,
    pub op: BodyOp,
    /// Target body, as the name of the feature that created it; `None` = the most recent body.
    pub body: Option<String>,
    /// Name of the body this feature creates (`op: new` only); `None` = the feature name.
    pub body_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AxisRef {
    X,
    Y,
    Z,
    /// A line entity of a sketch.
    SketchLine {
        sketch: String,
        entity: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RevolveDef {
    pub profile: ProfileRef,
    pub axis: AxisRef,
    pub angle: Expr,
    pub op: BodyOp,
    pub body: Option<String>,
    /// Name of the body this feature creates (`op: new` only); `None` = the feature name.
    pub body_name: Option<String>,
}

/// Fillet (size = radius) or chamfer (size = distance).
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeBlendDef {
    pub edges: Vec<EdgeRef>,
    pub size: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportFormat {
    Step,
    Iges,
    Stl,
    ThreeMf,
    Obj,
    /// Kernel-native B-rep (the import worker's output).
    Brep,
}

impl ImportFormat {
    pub fn keyword(self) -> &'static str {
        match self {
            ImportFormat::Step => "step",
            ImportFormat::Iges => "iges",
            ImportFormat::Stl => "stl",
            ImportFormat::ThreeMf => "3mf",
            ImportFormat::Obj => "obj",
            ImportFormat::Brep => "brep",
        }
    }
    pub fn from_keyword(s: &str) -> Option<Self> {
        [
            ImportFormat::Step,
            ImportFormat::Iges,
            ImportFormat::Stl,
            ImportFormat::ThreeMf,
            ImportFormat::Obj,
            ImportFormat::Brep,
        ]
        .into_iter()
        .find(|f| f.keyword() == s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDef {
    /// BLAKE3 hash (hex) of the stored blob.
    pub blob: String,
    pub format: ImportFormat,
    /// Original file name (untrusted).
    pub file_name: String,
}

// ------------------------------------------------------------------ shell, mirror, patterns

/// Hollows a body: removes `faces` and leaves walls of `thickness` (positive = inward,
/// negative = outward).
#[derive(Debug, Clone, PartialEq)]
pub struct ShellDef {
    /// Target body (named after the feature that created it); `None` = the most recent body.
    pub body: Option<String>,
    /// Faces to open (face references, ADR 0003). May be empty for a fully closed hollow.
    pub faces: Vec<FaceRef>,
    pub thickness: Expr,
}

/// Mirrors a body across a plane.
#[derive(Debug, Clone, PartialEq)]
pub struct MirrorDef {
    /// Source body; `None` = the most recent body.
    pub body: Option<String>,
    /// A base plane through the origin or a planar face reference.
    pub plane: PlaneRef,
    /// `true`: unite the mirror image with the source body. `false`: add it as a new body
    /// named after this feature.
    pub merge: bool,
}

/// A direction or axis direction: a base axis or an explicit vector (components are
/// unitless numbers/expressions; the vector need not be normalized).
#[derive(Debug, Clone, PartialEq)]
pub enum DirRef {
    X,
    Y,
    Z,
    Vector([Expr; 3]),
}

impl DirRef {
    /// Expressions inside the direction.
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            DirRef::Vector(v) => v.iter().collect(),
            _ => Vec::new(),
        }
    }
}

/// Repeats a body along a direction and unites the copies with it.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternLinearDef {
    /// Body to repeat (and to unite the copies into); `None` = the most recent body.
    pub body: Option<String>,
    pub direction: DirRef,
    /// Total number of instances including the original (>= 2); a unitless expression
    /// that must evaluate to a whole number.
    pub count: Expr,
    /// Distance between neighbouring instances (negative reverses the direction).
    pub spacing: Expr,
}

/// Repeats a body around an axis and unites the copies with it.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternCircularDef {
    pub body: Option<String>,
    pub axis: DirRef,
    /// A point on the axis (lengths); `None` = the world origin.
    pub through: Option<[Expr; 3]>,
    /// Total number of instances including the original (>= 2).
    pub count: Expr,
    /// Total angle covered. A full turn (360 deg) spaces instances `angle / count` apart;
    /// a partial angle puts the last instance at `angle` (spacing `angle / (count - 1)`).
    pub angle: Expr,
}
