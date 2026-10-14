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
    Sweep(SweepDef),
    Loft(LoftDef),
    Fillet(EdgeBlendDef),
    Chamfer(EdgeBlendDef),
    Import(ImportDef),
    Shell(ShellDef),
    Mirror(MirrorDef),
    PatternLinear(PatternLinearDef),
    PatternCircular(PatternCircularDef),
    Plane(PlaneDef),
    Move(MoveDef),
    Combine(CombineDef),
    Hole(HoleDef),
    Joint(Box<JointDef>),
    Component(ComponentDef),
}

impl FeatureKind {
    pub fn keyword(&self) -> &'static str {
        match self {
            FeatureKind::Sketch(_) => "sketch",
            FeatureKind::Extrude(_) => "extrude",
            FeatureKind::Revolve(_) => "revolve",
            FeatureKind::Sweep(_) => "sweep",
            FeatureKind::Loft(_) => "loft",
            FeatureKind::Fillet(_) => "fillet",
            FeatureKind::Chamfer(_) => "chamfer",
            FeatureKind::Import(_) => "import",
            FeatureKind::Shell(_) => "shell",
            FeatureKind::Mirror(_) => "mirror",
            FeatureKind::PatternLinear(_) => "pattern_linear",
            FeatureKind::PatternCircular(_) => "pattern_circular",
            FeatureKind::Plane(_) => "plane",
            FeatureKind::Move(_) => "move",
            FeatureKind::Combine(_) => "combine",
            FeatureKind::Hole(_) => "hole",
            FeatureKind::Joint(_) => "joint",
            FeatureKind::Component(_) => "component",
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
            FeatureKind::Sketch(s) => s.plane.features(&mut add),
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
            FeatureKind::Sweep(d) => {
                add(&d.profile.sketch);
                match &d.path {
                    PathRef::Sketch { sketch, .. } => add(sketch),
                    PathRef::Edges(edges) => edges.iter().for_each(|e| e.features(&mut add)),
                }
                add_body(&mut add, &d.body);
            }
            FeatureKind::Loft(d) => {
                d.profiles.iter().for_each(|p| add(&p.sketch));
                add_body(&mut add, &d.body);
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
                d.plane.features(&mut add);
            }
            FeatureKind::PatternLinear(d) => add_body(&mut add, &d.body),
            FeatureKind::PatternCircular(d) => add_body(&mut add, &d.body),
            FeatureKind::Plane(d) => {
                d.base.features(&mut add);
                if let PlaneKind::Angle { axis: PlaneAxis::Edge(e), .. } = &d.kind {
                    e.features(&mut add);
                }
            }
            FeatureKind::Move(d) => add_body(&mut add, &d.body),
            FeatureKind::Combine(d) => {
                add_body(&mut add, &d.body);
                d.tools.iter().for_each(|t| add(t));
            }
            FeatureKind::Hole(d) => {
                d.face.features(&mut add);
                if let HoleSites::Sketch(s) = &d.sites {
                    add(s);
                }
            }
            FeatureKind::Joint(d) => {
                add(&d.body);
                d.with.iter().for_each(|b| add(b));
                d.at.features(&mut add);
                d.to.features(&mut add);
            }
            FeatureKind::Component(_) => {}
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
            FeatureKind::Sketch(s) => {
                s.constraints.iter().flat_map(|c| c.value.iter()).for_each(&mut take);
                for p in &s.patterns {
                    take(&p.count);
                    match &p.kind {
                        SkPatternKind::Linear { dx, dy } => {
                            take(dx);
                            take(dy);
                        }
                        SkPatternKind::Circular { angle, .. } => take(angle),
                    }
                }
            }
            FeatureKind::Extrude(e) => take(&e.distance),
            FeatureKind::Revolve(r) => take(&r.angle),
            FeatureKind::Sweep(d) => d.twist.iter().for_each(&mut take),
            FeatureKind::Loft(_) => {}
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
            FeatureKind::Plane(d) => match &d.kind {
                PlaneKind::Offset(e) => take(e),
                PlaneKind::Angle { angle, .. } => take(angle),
            },
            FeatureKind::Move(d) => {
                let translate = d.translate.iter().flatten();
                let rot = d
                    .rotation
                    .iter()
                    .flat_map(|r| r.axis.exprs().into_iter().chain(r.through.iter().flatten()).chain([&r.angle]));
                translate.chain(rot).for_each(&mut take);
            }
            FeatureKind::Combine(_) => {}
            FeatureKind::Hole(d) => d.exprs().into_iter().for_each(&mut take),
            FeatureKind::Joint(d) => d.exprs().into_iter().for_each(&mut take),
            FeatureKind::Component(d) => d.set.iter().for_each(|(_, e)| take(e)),
        }
        // A generated gear sketch is checked with its gear's parameters (ADR 0018 §3).
        if let FeatureKind::Sketch(SketchDef { gear: Some(g), .. }) = self {
            for p in gear_inputs(g) {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Mutable expressions referenced by this feature.
    pub fn exprs_mut(&mut self) -> Vec<&mut Expr> {
        let mut out = Vec::new();
        match self {
            FeatureKind::Sketch(s) => {
                out.extend(s.constraints.iter_mut().filter_map(|c| c.value.as_mut()));
                for p in &mut s.patterns {
                    out.push(&mut p.count);
                    match &mut p.kind {
                        SkPatternKind::Linear { dx, dy } => out.extend([dx, dy]),
                        SkPatternKind::Circular { angle, .. } => out.push(angle),
                    }
                }
            }
            FeatureKind::Extrude(e) => out.push(&mut e.distance),
            FeatureKind::Revolve(r) => out.push(&mut r.angle),
            FeatureKind::Sweep(d) => out.extend(d.twist.iter_mut()),
            FeatureKind::Loft(_) => {}
            FeatureKind::Fillet(b) | FeatureKind::Chamfer(b) => out.push(&mut b.size),
            FeatureKind::Import(_) => {}
            FeatureKind::Shell(d) => out.push(&mut d.thickness),
            FeatureKind::Mirror(_) => {}
            FeatureKind::PatternLinear(d) => {
                out.extend(d.direction.exprs_mut());
                out.extend([&mut d.count, &mut d.spacing]);
            }
            FeatureKind::PatternCircular(d) => {
                out.extend(d.axis.exprs_mut());
                if let Some(through) = &mut d.through {
                    out.extend(through.iter_mut());
                }
                out.extend([&mut d.count, &mut d.angle]);
            }
            FeatureKind::Plane(d) => match &mut d.kind {
                PlaneKind::Offset(e) => out.push(e),
                PlaneKind::Angle { angle, .. } => out.push(angle),
            },
            FeatureKind::Move(d) => {
                if let Some(translate) = &mut d.translate {
                    out.extend(translate.iter_mut());
                }
                if let Some(rotation) = &mut d.rotation {
                    out.extend(rotation.axis.exprs_mut());
                    if let Some(through) = &mut rotation.through {
                        out.extend(through.iter_mut());
                    }
                    out.push(&mut rotation.angle);
                }
            }
            FeatureKind::Combine(_) => {}
            FeatureKind::Hole(d) => out.extend(d.exprs_mut()),
            FeatureKind::Joint(d) => out.extend(d.exprs_mut()),
            FeatureKind::Component(d) => out.extend(d.set.iter_mut().map(|(_, e)| e)),
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
    /// A datum plane: the name of an earlier `plane` feature.
    Datum(String),
}

impl PlaneRef {
    /// Calls `f` with every feature name the plane refers to.
    pub fn features(&self, f: &mut dyn FnMut(&str)) {
        match self {
            PlaneRef::Face(r) => r.features(f),
            PlaneRef::Datum(n) => f(n),
            PlaneRef::XY | PlaneRef::XZ | PlaneRef::YZ => {}
        }
    }

    /// Renames every occurrence of feature `from` to `to`.
    pub fn rename_feature(&mut self, from: &str, to: &str) {
        match self {
            PlaneRef::Face(r) => r.rename_feature(from, to),
            PlaneRef::Datum(n) if n == from => *n = to.to_string(),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SketchDef {
    pub plane: PlaneRef,
    /// Next entity/constraint number; ids are never reused within a sketch.
    pub next_id: u32,
    pub entities: Vec<SkEntity>,
    pub constraints: Vec<SkConstraint>,
    /// Patterns of curves: copies derived after solving (ADR 0016 §4).
    pub patterns: Vec<SkPattern>,
    /// Set on the profile sketch `generate.gear` writes: the parameter prefix of the gear
    /// (`gear1` for `gear1_module`, ...). The evaluator then checks the gear's parameters
    /// on every rebuild, as the generator does (ADR 0018 §3); see [`gear_inputs`].
    pub gear: Option<String>,
}

/// The parameters a generated gear sketch with prefix `p` is checked with: the gear's
/// inputs, and `<p>_transverse_module` for the helix angle as the sketch sees it.
pub fn gear_inputs(p: &str) -> [String; 8] {
    ["module", "teeth", "pressure_angle", "shift", "backlash", "fillet", "bore", "transverse_module"]
        .map(|w| format!("{p}_{w}"))
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
    /// Interpolating spline (centripetal Catmull-Rom) through the fit points, which are
    /// ordinary sketch points; `closed` joins the last point back to the first.
    Spline {
        points: Vec<String>,
        closed: bool,
    },
}

/// Most fit points one spline may have.
pub const MAX_SPLINE_POINTS: usize = 1000;

impl SkGeom {
    pub fn keyword(&self) -> &'static str {
        match self {
            SkGeom::Point { .. } => "point",
            SkGeom::Line { .. } => "line",
            SkGeom::Circle { .. } => "circle",
            SkGeom::Arc { .. } => "arc",
            SkGeom::Spline { .. } => "spline",
        }
    }
    pub fn id_prefix(&self) -> char {
        match self {
            SkGeom::Point { .. } => 'p',
            SkGeom::Line { .. } => 'l',
            SkGeom::Circle { .. } => 'c',
            SkGeom::Arc { .. } => 'a',
            SkGeom::Spline { .. } => 's',
        }
    }
    /// Entity ids this geometry refers to.
    pub fn refs(&self) -> Vec<&str> {
        match self {
            SkGeom::Point { .. } => vec![],
            SkGeom::Line { a, b } => vec![a, b],
            SkGeom::Circle { center, .. } => vec![center],
            SkGeom::Arc { center, start, end } => vec![center, start, end],
            SkGeom::Spline { points, .. } => points.iter().map(String::as_str).collect(),
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
    /// Signed horizontal distance between two points: `x(b) − x(a)`.
    HDistance,
    /// Signed vertical distance between two points: `y(b) − y(a)`.
    VDistance,
    /// A point's x coordinate in the sketch (its horizontal distance from the origin).
    AtX,
    /// A point's y coordinate in the sketch.
    AtY,
}

impl SkConstraintKind {
    pub const ALL: [SkConstraintKind; 20] = [
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
        SkConstraintKind::HDistance,
        SkConstraintKind::VDistance,
        SkConstraintKind::AtX,
        SkConstraintKind::AtY,
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
            SkConstraintKind::HDistance => "h_distance",
            SkConstraintKind::VDistance => "v_distance",
            SkConstraintKind::AtX => "at_x",
            SkConstraintKind::AtY => "at_y",
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
            Radius | Diameter | AtX | AtY => (1, true),
            Coincident | PointOnLine | PointOnCircle | Midpoint | Parallel | Perpendicular | Tangent | Equal => {
                (2, false)
            }
            Distance | Angle | HDistance | VDistance => (2, true),
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

/// A pattern of sketch curves: `count` copies (the source included) laid out by `kind`.
/// Copy `j` of entity `l5` is `l5_j` and of point `p3` is `p3_j` (derived, not stored).
#[derive(Debug, Clone, PartialEq)]
pub struct SkPattern {
    /// `g7` — shares the sketch's id counter.
    pub id: String,
    pub kind: SkPatternKind,
    /// Source curves (lines, circles, arcs, splines).
    pub of: Vec<String>,
    /// Number of instances including the original: a whole number from 2 to 500.
    pub count: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkPatternKind {
    /// Copy `j` is shifted by `j * (dx, dy)`.
    Linear { dx: Expr, dy: Expr },
    /// Copy `j` is turned about the point `center` by `j * step`: `angle / count` for a
    /// full turn, `angle / (count - 1)` otherwise.
    Circular { center: String, angle: Expr },
}

impl SkPatternKind {
    pub fn keyword(&self) -> &'static str {
        match self {
            SkPatternKind::Linear { .. } => "pattern_linear",
            SkPatternKind::Circular { .. } => "pattern_circular",
        }
    }
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

/// How a sweep orients its profile along the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Orientation {
    /// The profile keeps its orientation and only slides along the path.
    Fixed,
    /// The profile turns with the path (a corrected Frenet frame).
    #[default]
    Frenet,
}

impl Orientation {
    pub fn keyword(self) -> &'static str {
        match self {
            Orientation::Fixed => "fixed",
            Orientation::Frenet => "frenet",
        }
    }
    pub fn from_keyword(s: &str) -> Option<Self> {
        [Orientation::Fixed, Orientation::Frenet].into_iter().find(|o| o.keyword() == s)
    }
}

/// The path of a sweep.
#[derive(Debug, Clone, PartialEq)]
pub enum PathRef {
    /// Curves of a sketch that form one chain, open or closed. Empty `curves` = every
    /// curve of the sketch that is not construction geometry.
    Sketch { sketch: String, curves: Vec<String> },
    /// Edges of an earlier body (persistent edge references), forming one chain.
    Edges(Vec<EdgeRef>),
}

/// Sweeps closed sketch regions along a path.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepDef {
    pub profile: ProfileRef,
    pub path: PathRef,
    pub orientation: Orientation,
    /// Total turn of the profile about the path from start to end (an angle); `None` = none.
    /// Needs `orientation: frenet` and an open path.
    pub twist: Option<Expr>,
    pub op: BodyOp,
    pub body: Option<String>,
    /// Name of the body this feature creates (`op: new` only); `None` = the feature name.
    pub body_name: Option<String>,
}

/// Lofts a solid through two or more closed profiles, each on its own plane.
#[derive(Debug, Clone, PartialEq)]
pub struct LoftDef {
    /// The profiles in order; each must select exactly one region without holes.
    pub profiles: Vec<ProfileRef>,
    /// Ruled surfaces between neighbouring profiles instead of one smooth surface.
    pub ruled: bool,
    pub op: BodyOp,
    pub body: Option<String>,
    /// Name of the body this feature creates (`op: new` only); `None` = the feature name.
    pub body_name: Option<String>,
}

/// Most profiles one loft may use.
pub const MAX_LOFT_PROFILES: usize = 64;

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

    /// Mutable expressions inside the direction.
    pub fn exprs_mut(&mut self) -> Vec<&mut Expr> {
        match self {
            DirRef::Vector(v) => v.iter_mut().collect(),
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

// ------------------------------------------------------------------ datum planes, move, combine

/// A construction plane. Creates no body; sketches and mirrors can use it as their plane.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneDef {
    /// The plane this one is derived from: a base plane, a planar face or another datum plane.
    pub base: PlaneRef,
    pub kind: PlaneKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaneKind {
    /// Parallel to the base plane, moved along its normal by a length (negative = against it).
    Offset(Expr),
    /// The base plane turned about `axis` by an angle (right-hand rule about the axis
    /// direction).
    Angle { axis: PlaneAxis, angle: Expr },
}

/// The rotation axis of an angled datum plane.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaneAxis {
    /// A world axis through the origin.
    X,
    Y,
    Z,
    /// A straight edge of an earlier body. Its direction points towards +X (else +Y, else +Z).
    Edge(EdgeRef),
}

/// Turns a body about an axis.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveRotation {
    pub axis: DirRef,
    pub angle: Expr,
    /// A point on the axis (lengths); `None` = the world origin.
    pub through: Option<[Expr; 3]>,
}

/// Moves a body: first turns it about an axis, then translates it (world axes). With
/// `copy` the original stays and the moved copy is a new body named after the feature.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveDef {
    /// Body to move (named after the feature that created it); `None` = the most recent body.
    pub body: Option<String>,
    /// Translation along the world X, Y and Z axes (lengths).
    pub translate: Option<[Expr; 3]>,
    pub rotation: Option<MoveRotation>,
    pub copy: bool,
}

/// The kind of a joint (ADR 0026): which motions it leaves free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointKind {
    /// No motion.
    Rigid,
    /// Rotation about Z (`angle`).
    Revolute,
    /// Translation along Z (`distance`).
    Slider,
    /// Rotation about and translation along Z (`angle`, `distance`).
    Cylindrical,
    /// Rotation about Z and translation along X (`angle`, `distance`).
    PinSlot,
    /// Translation in the XY plane and rotation about Z (`x`, `y`, `angle`).
    Planar,
    /// Rotation about X, then Y, then Z (`angles`).
    Ball,
}

impl JointKind {
    pub const ALL: [JointKind; 7] = [
        JointKind::Rigid,
        JointKind::Revolute,
        JointKind::Slider,
        JointKind::Cylindrical,
        JointKind::PinSlot,
        JointKind::Planar,
        JointKind::Ball,
    ];

    /// Degrees of freedom the joint leaves (the values that move it).
    pub fn dof(self) -> usize {
        match self {
            JointKind::Rigid => 0,
            JointKind::Revolute | JointKind::Slider => 1,
            JointKind::Cylindrical | JointKind::PinSlot => 2,
            JointKind::Planar | JointKind::Ball => 3,
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            JointKind::Rigid => "rigid",
            JointKind::Revolute => "revolute",
            JointKind::Slider => "slider",
            JointKind::Cylindrical => "cylindrical",
            JointKind::PinSlot => "pin_slot",
            JointKind::Planar => "planar",
            JointKind::Ball => "ball",
        }
    }

    pub fn from_keyword(s: &str) -> Option<JointKind> {
        JointKind::ALL.into_iter().find(|k| k.keyword() == s)
    }
}

/// Where a joint attaches (a joint origin, ADR 0026 §2).
#[derive(Debug, Clone, PartialEq)]
pub enum FrameRef {
    /// The world origin, Z up.
    Origin,
    /// A plane (world plane, datum plane or planar face): its frame.
    Plane(PlaneRef),
    /// A face: planar (centroid, normal) or revolved (on its axis).
    Face(FaceRef),
    /// An edge: circular (center, normal) or straight (midpoint, direction).
    Edge(EdgeRef),
}

impl FrameRef {
    /// Renames every occurrence of feature `from` to `to`.
    pub fn rename_feature(&mut self, from: &str, to: &str) {
        match self {
            FrameRef::Origin => {}
            FrameRef::Plane(p) => p.rename_feature(from, to),
            FrameRef::Face(f) => f.rename_feature(from, to),
            FrameRef::Edge(e) => e.rename_feature(from, to),
        }
    }

    fn features(&self, add: &mut impl FnMut(&str)) {
        match self {
            FrameRef::Origin => {}
            FrameRef::Plane(p) => p.features(add),
            FrameRef::Face(f) => f.features(add),
            FrameRef::Edge(e) => e.features(add),
        }
    }
}

/// Places a body by a joint (ADR 0026): the frame `at` on the moving body is brought onto
/// A body of another document, pinned to a version (ADR 0026 §7): `document` is the blob
/// holding that document's text as it was when the component was inserted or updated, so
/// the component rebuilds the same until it is updated.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDef {
    /// Where the document came from (a path, untrusted; for updates and display).
    pub source: String,
    /// The source's history node when it was pinned (hex; informational).
    pub version: String,
    /// BLAKE3 hash (hex) of the stored document text.
    pub document: String,
    /// The body of that document (default: its last body).
    pub body: Option<String>,
    /// Parameters of that document set from this one (`set: [teeth = 30, w = 2 * a]`):
    /// evaluated here and given to the component as values.
    pub set: Vec<(String, Expr)>,
}

/// the frame `to` (on a body already in place, a plane or the origin), with the joint's
/// motion in between. The bodies in `with` move along rigidly.
#[derive(Debug, Clone, PartialEq)]
pub struct JointDef {
    pub kind: JointKind,
    /// The moving body (named after the feature that created it).
    pub body: String,
    pub with: Vec<String>,
    pub at: FrameRef,
    pub to: FrameRef,
    /// Rotation about Z (revolute, cylindrical, pin slot, planar).
    pub angle: Option<Expr>,
    /// Translation along Z (slider, cylindrical) or X (pin slot).
    pub distance: Option<Expr>,
    /// Planar translation.
    pub x: Option<Expr>,
    pub y: Option<Expr>,
    /// Ball rotations about X, Y, Z.
    pub angles: Option<[Expr; 3]>,
    /// Reverses the fixed frame's Z axis.
    pub flip: bool,
    /// Moves along the fixed frame's Z axis (length).
    pub offset: Option<Expr>,
    /// Allowed range of `angle` and `distance`.
    pub angle_limits: Option<[Expr; 2]>,
    pub distance_limits: Option<[Expr; 2]>,
}

impl JointDef {
    /// Renames every reference to feature `from`.
    pub fn rename_feature(&mut self, from: &str, to: &str) {
        for b in std::iter::once(&mut self.body).chain(&mut self.with) {
            if b == from {
                *b = to.to_string();
            }
        }
        self.at.rename_feature(from, to);
        self.to.rename_feature(from, to);
    }

    /// Every expression, in a fixed order.
    pub fn exprs(&self) -> Vec<&Expr> {
        let mut out: Vec<&Expr> = Vec::new();
        out.extend(self.angle.iter().chain(&self.distance).chain(&self.x).chain(&self.y));
        out.extend(self.angles.iter().flatten());
        out.extend(self.offset.iter());
        out.extend(self.angle_limits.iter().flatten().chain(self.distance_limits.iter().flatten()));
        out
    }

    pub fn exprs_mut(&mut self) -> Vec<&mut Expr> {
        let mut out: Vec<&mut Expr> = Vec::new();
        out.extend(self.angle.iter_mut().chain(&mut self.distance).chain(&mut self.x).chain(&mut self.y));
        out.extend(self.angles.iter_mut().flatten());
        out.extend(self.offset.iter_mut());
        out.extend(self.angle_limits.iter_mut().flatten().chain(self.distance_limits.iter_mut().flatten()));
        out
    }
}

/// Combines tool bodies into a target body; the tool bodies are consumed.
#[derive(Debug, Clone, PartialEq)]
pub struct CombineDef {
    /// Target body; `None` = the most recent body that is not a tool.
    pub body: Option<String>,
    /// The bodies to combine into the target, as the names of the features that created
    /// them. Not empty, without duplicates, and never the target.
    pub tools: Vec<String>,
    /// `Add` (unite), `Cut` (subtract) or `Intersect`; never `New`.
    pub op: BodyOp,
}

// ------------------------------------------------------------------ holes

/// What kind of hole: how the mouth and the thread are made (ADR 0019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HoleType {
    /// A plain drilled hole.
    #[default]
    Simple,
    /// A cylindrical recess for a screw head (`cbore_diameter`, `cbore_depth`).
    Counterbore,
    /// A conical recess for a flat head (`csink_diameter`, `csink_angle`).
    Countersink,
    /// Modelled at the tap-drill diameter, with a cosmetic thread record (`thread`, `pitch`,
    /// `thread_depth`); no helical geometry.
    Tapped,
}

impl HoleType {
    pub const ALL: [HoleType; 4] = [HoleType::Simple, HoleType::Counterbore, HoleType::Countersink, HoleType::Tapped];

    pub fn keyword(self) -> &'static str {
        match self {
            HoleType::Simple => "simple",
            HoleType::Counterbore => "counterbore",
            HoleType::Countersink => "countersink",
            HoleType::Tapped => "tapped",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.keyword() == s)
    }
}

/// Where the holes of one `hole` feature go.
#[derive(Debug, Clone, PartialEq)]
pub enum HoleSites {
    /// The points of a sketch: every point that is not construction geometry and not an
    /// end or fit point of a line, arc or spline, plus the centre of every circle (its size
    /// is ignored). Pattern copies count. The sketch must be parallel to the face; points
    /// are projected onto the face along its normal. Hole ids are the point ids (`p5`).
    Sketch(String),
    /// Explicit points `(u, v)` in the face's own sketch frame (the frame a sketch placed on
    /// that face gets), as lengths, each with its stored id (`e1`, `e2`, ...; ADR 0019 §3).
    Points(Vec<HolePoint>),
}

/// One explicit hole position. Its id (`e<id>`) is stored with it, so deleting or reordering
/// other points does not rename this hole's faces (review L2). Files written before the ids
/// were stored list bare `[u, v]` pairs, which get `e1`, `e2`, ... in list order.
#[derive(Debug, Clone, PartialEq)]
pub struct HolePoint {
    /// The number in the hole id `e<id>` (at least 1, unique within the feature).
    pub id: u32,
    pub at: [Expr; 2],
}

impl HolePoint {
    /// The hole id, `e<id>`.
    pub fn name(&self) -> String {
        format!("e{}", self.id)
    }
}

/// How deep a hole goes.
#[derive(Debug, Clone, PartialEq)]
pub enum HoleDepth {
    /// To the end of the full-diameter part, measured from the face; the drill point (cone)
    /// comes on top of it.
    Blind(Expr),
    /// All the way through the body, with a flat end.
    Through,
}

/// A counterbore: a cylindrical recess at the mouth.
#[derive(Debug, Clone, PartialEq)]
pub struct CounterboreDef {
    pub diameter: Expr,
    pub depth: Expr,
}

/// A countersink: a conical recess at the mouth.
#[derive(Debug, Clone, PartialEq)]
pub struct CountersinkDef {
    pub diameter: Expr,
    /// The full cone angle (90 deg for ISO 10642 screws).
    pub angle: Expr,
}

/// The cosmetic thread of a tapped hole: recorded, not modelled.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreadDef {
    /// Designation such as `M6` (untrusted text; at most [`MAX_THREAD_DESIGNATION`]
    /// printable characters).
    pub designation: String,
    pub pitch: Expr,
    /// Threaded length from the face; `None` = the whole hole.
    pub depth: Option<Expr>,
}

/// Longest thread designation.
pub const MAX_THREAD_DESIGNATION: usize = 32;
/// Most holes one feature may make (each costs a boolean operation).
pub const MAX_HOLE_SITES: usize = 1000;

/// Holes on a planar face of a body, cut with one boolean per hole (ADR 0019).
#[derive(Debug, Clone, PartialEq)]
pub struct HoleDef {
    /// The planar face the holes start on; they go into the material, against the outward
    /// normal. The face names the body.
    pub face: FaceRef,
    pub sites: HoleSites,
    pub kind: HoleType,
    /// Drilled diameter (the tap-drill diameter for a tapped hole).
    pub diameter: Expr,
    pub depth: HoleDepth,
    /// Full angle of the drill point of a blind hole; `None` = 118 deg.
    pub point_angle: Option<Expr>,
    pub counterbore: Option<CounterboreDef>,
    pub countersink: Option<CountersinkDef>,
    pub thread: Option<ThreadDef>,
}

impl HoleDef {
    /// Every expression of the feature.
    pub fn exprs(&self) -> Vec<&Expr> {
        let mut v: Vec<&Expr> = Vec::new();
        if let HoleSites::Points(p) = &self.sites {
            v.extend(p.iter().flat_map(|h| [&h.at[0], &h.at[1]]));
        }
        v.push(&self.diameter);
        if let HoleDepth::Blind(d) = &self.depth {
            v.push(d);
        }
        v.extend(self.point_angle.iter());
        if let Some(c) = &self.counterbore {
            v.extend([&c.diameter, &c.depth]);
        }
        if let Some(c) = &self.countersink {
            v.extend([&c.diameter, &c.angle]);
        }
        if let Some(t) = &self.thread {
            v.push(&t.pitch);
            v.extend(t.depth.iter());
        }
        v
    }

    /// Mutable expressions of the feature.
    pub fn exprs_mut(&mut self) -> Vec<&mut Expr> {
        let mut v: Vec<&mut Expr> = Vec::new();
        if let HoleSites::Points(p) = &mut self.sites {
            v.extend(p.iter_mut().flat_map(|h| h.at.iter_mut()));
        }
        v.push(&mut self.diameter);
        if let HoleDepth::Blind(d) = &mut self.depth {
            v.push(d);
        }
        v.extend(self.point_angle.iter_mut());
        if let Some(c) = &mut self.counterbore {
            v.extend([&mut c.diameter, &mut c.depth]);
        }
        if let Some(c) = &mut self.countersink {
            v.extend([&mut c.diameter, &mut c.angle]);
        }
        if let Some(t) = &mut self.thread {
            v.push(&mut t.pitch);
            v.extend(t.depth.iter_mut());
        }
        v
    }
}
