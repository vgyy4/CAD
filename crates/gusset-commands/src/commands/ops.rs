//! Shell, draft, mirror and pattern commands (v0.2). They change an existing body, so the target
//! is resolved to a concrete body name when the feature is created (like add/cut).

use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        spec::<Shell>(),
        spec::<Draft>(),
        spec::<OffsetFace>(),
        spec::<Mirror>(),
        spec::<PatternLinear>(),
        spec::<PatternCircular>(),
    ]
}

/// Renames references to feature `from` inside the new feature kinds.
pub(super) fn rename_refs(kind: &mut FeatureKind, from: &str, to: &str) {
    let body_of = |body: &mut Option<String>| {
        if body.as_deref() == Some(from) {
            *body = Some(to.to_string());
        }
    };
    let sketch_of = |s: &mut String| {
        if s == from {
            *s = to.to_string();
        }
    };
    match kind {
        FeatureKind::Sweep(d) => {
            sketch_of(&mut d.profile.sketch);
            match &mut d.path {
                PathRef::Sketch { sketch, .. } => sketch_of(sketch),
                PathRef::Edges(edges) => edges.iter_mut().for_each(|e| e.rename_feature(from, to)),
            }
            body_of(&mut d.body);
        }
        FeatureKind::Loft(d) => {
            d.profiles.iter_mut().for_each(|p| sketch_of(&mut p.sketch));
            body_of(&mut d.body);
        }
        FeatureKind::Shell(d) => {
            body_of(&mut d.body);
            for f in &mut d.faces {
                f.rename_feature(from, to);
            }
        }
        FeatureKind::Mirror(d) => {
            body_of(&mut d.body);
            d.plane.rename_feature(from, to);
        }
        FeatureKind::OffsetFace(d) => {
            body_of(&mut d.body);
            for f in &mut d.faces {
                f.rename_feature(from, to);
            }
        }
        FeatureKind::Text(d) => {
            body_of(&mut d.body);
            d.plane.rename_feature(from, to);
        }
        FeatureKind::Draft(d) => {
            body_of(&mut d.body);
            for f in &mut d.faces {
                f.rename_feature(from, to);
            }
            d.plane.rename_feature(from, to);
        }
        FeatureKind::PatternLinear(d) => body_of(&mut d.body),
        FeatureKind::PatternCircular(d) => body_of(&mut d.body),
        FeatureKind::Plane(d) => {
            d.base.rename_feature(from, to);
            if let PlaneKind::Angle { axis: PlaneAxis::Edge(e), .. } = &mut d.kind {
                e.rename_feature(from, to);
            }
        }
        FeatureKind::Move(d) => body_of(&mut d.body),
        FeatureKind::Hole(d) => super::hole::rename_refs(d, from, to),
        FeatureKind::Joint(d) => d.rename_feature(from, to),
        FeatureKind::Combine(d) => {
            body_of(&mut d.body);
            for t in &mut d.tools {
                if t == from {
                    *t = to.to_string();
                }
            }
        }
        _ => {}
    }
}

/// Resolves the body to work on now and stores its name in the feature.
pub(super) fn body_for(
    doc: &Document,
    rb: Option<usize>,
    explicit: Option<String>,
) -> Result<Option<String>, CommandError> {
    target_body(doc, rb, BodyOp::Add, explicit)
}

fn parse_faces(faces: &[String]) -> Result<Vec<FaceRef>, CommandError> {
    faces.iter().map(|f| FaceRef::parse(f).map_err(|e| CommandError::args(format!("face '{f}': {e}")))).collect()
}

/// Moves faces along their normals (ADR 0030).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OffsetFace {
    /// Faces to move (face refs from query_body).
    pub faces: Vec<String>,
    /// Along the face normals (`2 mm`); negative moves them in.
    pub distance: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for OffsetFace {
    const ID: &'static str = "feature.offset_face";
    const TITLE: &'static str = "Offset face";
    const DESCRIPTION: &'static str =
        "Push or pull faces along their normals (direct edit); neighbouring faces follow. Needs OCCT.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let distance = parse_expr("distance", &self.distance)?;
        let faces = parse_faces(&self.faces)?;
        if faces.is_empty() {
            return Err(CommandError::args("no faces to move").hint("list the faces to push or pull"));
        }
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        edit(s, ctx, "Offset face", |doc, rb| {
            let name = feature_name(doc, self.name, "OffsetFace")?;
            let body = body_for(doc, rb, self.body)?;
            let def = OffsetFaceDef { body, faces, distance };
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::OffsetFace(def)));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Tapers faces of a body for molding (ADR 0028).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    /// Faces to taper (planar, cylindrical or conical, crossing the plane).
    pub faces: Vec<String>,
    /// Neutral plane: XY/XZ/YZ, planar face or datum plane.
    pub plane: String,
    /// Draft angle (`2 deg`); positive narrows the part along the pull direction.
    pub angle: String,
    /// Reverse the pull direction (plane normal, or into the body for a face).
    #[serde(default)]
    pub flip: bool,
    /// Body to change (default: the latest body).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Draft {
    const ID: &'static str = "feature.draft";
    const TITLE: &'static str = "Draft";
    const DESCRIPTION: &'static str = "Taper faces by an angle about a neutral plane so a molded or cast part releases. Needs the OCCT kernel. Faces keep their names. Returns body_changes.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let angle = parse_expr("angle", &self.angle)?;
        let faces = parse_faces(&self.faces)?;
        if faces.is_empty() {
            return Err(CommandError::args("no faces to draft").hint("list the side faces to taper"));
        }
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        let plane = super::datum::parse_plane_arg(s, ctx, &self.plane)?;
        edit(s, ctx, "Draft", |doc, rb| {
            let name = feature_name(doc, self.name, "Draft")?;
            let body = body_for(doc, rb, self.body)?;
            let def = DraftDef { body, faces, plane, angle, flip: self.flip };
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::Draft(def)));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Hollows a body, opening the given faces.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shell {
    /// Faces to remove (face refs from query_body, e.g. `Extrude1/cap(end)`); empty = closed hollow. Inner walls: `<feature>/inner(<face>)`.
    #[serde(default)]
    pub faces: Vec<String>,
    /// Wall thickness (`2 mm`); positive inward, negative outward.
    pub thickness: String,
    /// Body to hollow (default: the latest body).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Shell {
    const ID: &'static str = "feature.shell";
    const TITLE: &'static str = "Shell";
    const DESCRIPTION: &'static str = "Hollow a body to a wall thickness, removing the selected faces (e.g. the top to make an open box). Needs the OCCT kernel. Fails with a hint when the walls are too thick for the part. Returns body_changes.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let thickness = parse_expr("thickness", &self.thickness)?;
        let faces = parse_faces(&self.faces)?;
        for f in &faces {
            check_face_resolves(s, ctx, f)?;
        }
        edit(s, ctx, "Shell", |doc, rb| {
            let name = feature_name(doc, self.name, "Shell")?;
            let body = body_for(doc, rb, self.body)?;
            insert_feature(
                doc,
                rb,
                Feature::new(name.clone(), FeatureKind::Shell(ShellDef { body, faces, thickness })),
            );
            Ok(json!({ "feature": name }))
        })
    }
}

/// Mirrors a body across a plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mirror {
    /// `XY`, `XZ`, `YZ` (through the origin), a planar face reference such as
    /// `Extrude1/side(Sketch1/l6)` or a datum plane name (feature_plane).
    pub plane: String,
    /// true: unite with the source body; false (default): a new body named after the feature. Faces: `<feature>/mirror(<face>)`.
    #[serde(default)]
    pub merge: bool,
    /// Body to mirror (default: the latest body).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Mirror {
    const ID: &'static str = "feature.mirror";
    const TITLE: &'static str = "Mirror";
    const DESCRIPTION: &'static str = "Mirror a body across XY/XZ/YZ, a planar face or a datum plane, as a new body or united with the source. Uniting needs the OCCT kernel.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let plane = super::datum::parse_plane_arg(s, ctx, &self.plane)?;
        edit(s, ctx, "Mirror", |doc, rb| {
            let name = feature_name(doc, self.name, "Mirror")?;
            let body = body_for(doc, rb, self.body)?;
            insert_feature(
                doc,
                rb,
                Feature::new(name.clone(), FeatureKind::Mirror(MirrorDef { body, plane, merge: self.merge })),
            );
            Ok(json!({ "feature": name }))
        })
    }
}

/// Repeats a body along a direction.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternLinear {
    /// `X`, `Y`, `Z` or a vector such as `[1, 0.5, 0]` (unitless components).
    pub direction: String,
    /// Instances including the original, a whole number >= 2 (may be an expression).
    pub count: String,
    /// Distance between neighbours, e.g. `15 mm` (negative reverses).
    pub spacing: String,
    /// Body to repeat (default: the latest). Faces: `<feature>/instance(k, <face>)`.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for PatternLinear {
    const ID: &'static str = "feature.pattern_linear";
    const TITLE: &'static str = "Linear pattern";
    const DESCRIPTION: &'static str = "Repeat a whole body along a direction (count, spacing) and unite the copies with it. For holes, use `feature_hole` with several `at` points in one call. Uniting needs the OCCT kernel unless the copies stay apart.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let direction =
            text::parse_direction(&self.direction).map_err(|e| CommandError::args(format!("direction: {e}")))?;
        let count = parse_expr("count", &self.count)?;
        let spacing = parse_expr("spacing", &self.spacing)?;
        edit(s, ctx, "Linear pattern", |doc, rb| {
            let name = feature_name(doc, self.name, "Pattern")?;
            let body = body_for(doc, rb, self.body)?;
            insert_feature(
                doc,
                rb,
                Feature::new(
                    name.clone(),
                    FeatureKind::PatternLinear(PatternLinearDef { body, direction, count, spacing }),
                ),
            );
            Ok(json!({ "feature": name }))
        })
    }
}

/// Repeats a body around an axis.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternCircular {
    /// `X`, `Y`, `Z` or a vector like `[0, 1, 0]`.
    pub axis: String,
    /// A point on the axis, three lengths (default: the origin).
    #[serde(default)]
    pub through: Option<[String; 3]>,
    /// Instances including the original, a whole number >= 2 (may be an expression).
    pub count: String,
    /// Total angle, default `360 deg` (full turn: spaced angle/count; less: last at the angle).
    #[serde(default)]
    pub angle: Option<String>,
    /// Body to repeat (default: the latest). Faces: `<feature>/instance(k, <face>)`.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for PatternCircular {
    const ID: &'static str = "feature.pattern_circular";
    const TITLE: &'static str = "Circular pattern";
    const DESCRIPTION: &'static str = "Repeat a whole body around an axis (count, total angle) and unite the copies with it, e.g. a bolt circle. For holes, use `feature_hole` with several `at` points in one call. Uniting needs the OCCT kernel unless the copies stay apart.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let axis = text::parse_direction(&self.axis).map_err(|e| CommandError::args(format!("axis: {e}")))?;
        let count = parse_expr("count", &self.count)?;
        let angle = parse_expr("angle", self.angle.as_deref().unwrap_or("360 deg"))?;
        let through = match &self.through {
            None => None,
            Some([x, y, z]) => Some([parse_expr("through", x)?, parse_expr("through", y)?, parse_expr("through", z)?]),
        };
        edit(s, ctx, "Circular pattern", |doc, rb| {
            let name = feature_name(doc, self.name, "Pattern")?;
            let body = body_for(doc, rb, self.body)?;
            insert_feature(
                doc,
                rb,
                Feature::new(
                    name.clone(),
                    FeatureKind::PatternCircular(PatternCircularDef { body, axis, through, count, angle }),
                ),
            );
            Ok(json!({ "feature": name }))
        })
    }
}
