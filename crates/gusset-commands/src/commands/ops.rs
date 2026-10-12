//! Shell, mirror and pattern commands (v0.2). They change an existing body, so the target
//! is resolved to a concrete body name when the feature is created (like add/cut).

use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<Shell>(), spec::<Mirror>(), spec::<PatternLinear>(), spec::<PatternCircular>()]
}

/// Renames references to feature `from` inside the new feature kinds.
pub(super) fn rename_refs(kind: &mut FeatureKind, from: &str, to: &str) {
    let body_of = |body: &mut Option<String>| {
        if body.as_deref() == Some(from) {
            *body = Some(to.to_string());
        }
    };
    match kind {
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
        FeatureKind::PatternLinear(d) => body_of(&mut d.body),
        FeatureKind::PatternCircular(d) => body_of(&mut d.body),
        FeatureKind::Plane(d) => {
            d.base.rename_feature(from, to);
            if let PlaneKind::Angle { axis: PlaneAxis::Edge(e), .. } = &mut d.kind {
                e.rename_feature(from, to);
            }
        }
        FeatureKind::Move(d) => body_of(&mut d.body),
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

/// Hollows a body, opening the given faces.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shell {
    /// Faces to remove (persistent face references such as `Extrude1/cap(end)`; query_body
    /// lists them). May be empty for a closed hollow. The ring left where a removed face
    /// was keeps that face's name; the new inner walls are named `<feature>/inner(<face>)`.
    #[serde(default)]
    pub faces: Vec<String>,
    /// Wall thickness expression (`2 mm`, `wall`). Positive builds the walls inward from the
    /// existing outside; negative grows them outward.
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
    const DESCRIPTION: &'static str = "Hollow a body to a wall thickness, removing the selected faces (e.g. the top to make an open box). Needs the OCCT kernel. Fails with a hint when the walls are too thick for the part. The result lists body_changes (volume and bounding box before/after).";
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
    /// true: unite the mirror image with the source body (the body keeps its name). false
    /// (default): add it as a new body named after this feature. Mirrored faces are named
    /// `<feature>/mirror(<original face>)`.
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
    /// Number of instances including the original, a whole number >= 2. May be an
    /// expression, e.g. `n_bolts`.
    pub count: String,
    /// Distance between neighbouring instances, e.g. `15 mm` or `pitch` (negative reverses).
    pub spacing: String,
    /// Body to repeat (default: the latest body). The copies are united with it; instance
    /// faces are named `<feature>/instance(k, <original face>)`, k = 1..count-1.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for PatternLinear {
    const ID: &'static str = "feature.pattern_linear";
    const TITLE: &'static str = "Linear pattern";
    const DESCRIPTION: &'static str = "Repeat a whole body along a direction (count, spacing) and unite the copies with it. Uniting needs the OCCT kernel unless the copies stay apart.";
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
    /// Axis direction: `X`, `Y`, `Z` or a vector such as `[0, 1, 0]`.
    pub axis: String,
    /// A point on the axis as three length expressions, e.g. `["10 mm", "0 mm", "0 mm"]`.
    /// Default: the world origin.
    #[serde(default)]
    pub through: Option<[String; 3]>,
    /// Number of instances including the original, a whole number >= 2 (may be an
    /// expression such as `n_bolts`).
    pub count: String,
    /// Total angle, default `360 deg`. A full turn spaces instances angle/count apart; a
    /// smaller angle puts the last instance at that angle.
    #[serde(default)]
    pub angle: Option<String>,
    /// Body to repeat (default: the latest body). Instance faces are named
    /// `<feature>/instance(k, <original face>)`.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for PatternCircular {
    const ID: &'static str = "feature.pattern_circular";
    const TITLE: &'static str = "Circular pattern";
    const DESCRIPTION: &'static str = "Repeat a whole body around an axis (count, total angle) and unite the copies with it, e.g. a bolt circle. Uniting needs the OCCT kernel unless the copies stay apart.";
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
