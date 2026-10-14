//! Datum plane, body move/copy and combine commands (v0.2). Bodies are addressed by the
//! name of the feature that created them, stored in the document when the feature is made.

use super::ops::body_for;
use super::*;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<DatumPlane>(), spec::<MoveBody>(), spec::<Combine>()]
}

/// A plane argument: `XY`, `XZ`, `YZ`, a planar face reference or the name of a datum
/// plane. Face and datum references must resolve in the model as it stands.
pub(super) fn parse_plane_arg(s: &mut Session, ctx: &CmdCtx, text: &str) -> Result<PlaneRef, CommandError> {
    let plane = text::parse_plane_ref(text.trim()).map_err(|e| CommandError::args(format!("plane: {e}")))?;
    match &plane {
        PlaneRef::Face(face) => check_face_resolves(s, ctx, face)?,
        PlaneRef::Datum(name) => {
            let ev = s.evaluate(&ctx.actor)?;
            if !ev.planes.iter().any(|(n, _)| n == name) {
                let known: Vec<&str> = ev.planes.iter().map(|(n, _)| n.as_str()).collect();
                let hint = if known.is_empty() {
                    "use XY, XZ, YZ or a face reference, or create a datum plane with feature_plane first".to_string()
                } else {
                    format!("datum planes: {}", known.join(", "))
                };
                return Err(CommandError::not_found(format!("there is no datum plane '{name}'")).hint(hint));
            }
        }
        PlaneRef::XY | PlaneRef::XZ | PlaneRef::YZ => {}
    }
    Ok(plane)
}

pub(super) fn vec3(what: &str, v: &[String; 3]) -> Result<[gusset_units::expr::Expr; 3], CommandError> {
    Ok([parse_expr(what, &v[0])?, parse_expr(what, &v[1])?, parse_expr(what, &v[2])?])
}

/// Creates a construction plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatumPlane {
    /// `XY` (default), `XZ`, `YZ`, a planar face reference or a datum plane name.
    #[serde(default)]
    pub base: Option<String>,
    /// Length along the plane's normal, e.g. `10 mm`. Use this or axis with angle.
    #[serde(default)]
    pub offset: Option<String>,
    /// `X`, `Y`, `Z` (through the origin) or a straight `edge(...)` reference.
    #[serde(default)]
    pub axis: Option<String>,
    /// Expression such as `30 deg`, right-hand rule about the axis.
    #[serde(default)]
    pub angle: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for DatumPlane {
    const ID: &'static str = "feature.plane";
    const TITLE: &'static str = "Datum plane";
    const DESCRIPTION: &'static str = "Construction plane: offset from XY/XZ/YZ, a face or a datum plane along its normal (XY +Z, XZ -Y, YZ +X), or turned about an axis (X/Y/Z or a straight edge). Usable by sketch_create and feature_mirror; returns its frame.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let base = parse_plane_arg(s, ctx, self.base.as_deref().unwrap_or("XY"))?;
        let kind = match (&self.offset, &self.axis, &self.angle) {
            (Some(o), None, None) => PlaneKind::Offset(parse_expr("offset", o)?),
            (None, Some(a), Some(g)) => {
                let axis = match a.trim() {
                    "X" => PlaneAxis::X,
                    "Y" => PlaneAxis::Y,
                    "Z" => PlaneAxis::Z,
                    other => {
                        let edge = EdgeRef::parse(other).map_err(|e| {
                            CommandError::args(format!("axis: {e}")).hint("use X, Y, Z or an edge reference")
                        })?;
                        check_edges_resolve(s, ctx, std::slice::from_ref(&edge))?;
                        PlaneAxis::Edge(edge)
                    }
                };
                PlaneKind::Angle { axis, angle: parse_expr("angle", g)? }
            }
            (None, None, None) => {
                return Err(CommandError::args("give an offset, or an axis and an angle")
                    .hint("for example offset: \"10 mm\" or axis: \"X\", angle: \"30 deg\""));
            }
            _ => {
                return Err(CommandError::args("use either offset, or axis together with angle")
                    .hint("create two datum planes to offset and turn"));
            }
        };
        let created = std::cell::RefCell::new(String::new());
        edit_post(
            s,
            ctx,
            "Datum plane",
            |doc, rb| {
                let name = feature_name(doc, self.name, "Plane")?;
                insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::Plane(PlaneDef { base, kind })));
                *created.borrow_mut() = name.clone();
                Ok(json!({ "feature": name }))
            },
            |_, out, _| match out.evaluation.planes.iter().find(|(n, _)| n == created.borrow().as_str()) {
                Some((_, f)) => json!({ "frame": frame_json(f) }),
                None => Value::Null,
            },
        )
    }
}

/// Moves or copies a body.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveBody {
    /// Three length expressions along world X, Y, Z, e.g. `["10 mm", "0 mm", "h"]`.
    #[serde(default)]
    pub translate: Option<[String; 3]>,
    /// `X`, `Y`, `Z` or a vector like `[0, 0, 1]`; needs angle.
    #[serde(default)]
    pub axis: Option<String>,
    /// Expression such as `45 deg`.
    #[serde(default)]
    pub angle: Option<String>,
    /// A point on the axis, three length expressions (default: the origin).
    #[serde(default)]
    pub through: Option<[String; 3]>,
    /// Keep the original and add the moved copy.
    #[serde(default)]
    pub copy: bool,
    /// Body to move (default: the latest).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for MoveBody {
    const ID: &'static str = "feature.move_body";
    const TITLE: &'static str = "Move body";
    const DESCRIPTION: &'static str = "Rotate (first) and/or translate a body, or with copy: true add the moved copy as a new body named after the feature. A moved body keeps its face names; copy faces are named <feature>/copy(<face>).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let translate = self.translate.as_ref().map(|t| vec3("translate", t)).transpose()?;
        let rotation = match (&self.axis, &self.angle) {
            (Some(a), Some(g)) => Some(MoveRotation {
                axis: text::parse_direction(a).map_err(|e| CommandError::args(format!("axis: {e}")))?,
                angle: parse_expr("angle", g)?,
                through: self.through.as_ref().map(|t| vec3("through", t)).transpose()?,
            }),
            (None, None) if self.through.is_none() => None,
            (Some(_), None) => return Err(CommandError::args("a rotation axis needs an angle")),
            _ => {
                return Err(CommandError::args("angle (and through) need an axis").hint("give axis, for example \"Z\""));
            }
        };
        if translate.is_none() && rotation.is_none() {
            return Err(CommandError::args("the move does nothing").hint("give translate, or axis and angle (or both)"));
        }
        let label = if self.copy { "Copy body" } else { "Move body" };
        edit(s, ctx, label, |doc, rb| {
            let name = feature_name(doc, self.name, if self.copy { "Copy" } else { "Move" })?;
            let body = body_for(doc, rb, self.body)?;
            if self.copy
                && let Some(other) = doc.features.iter().find(|f| created_body_name(f) == Some(name.as_str()))
            {
                return Err(CommandError::args(format!(
                    "a body named '{name}' already exists (feature {})",
                    other.name
                ))
                .hint("choose another name"));
            }
            insert_feature(
                doc,
                rb,
                Feature::new(name.clone(), FeatureKind::Move(MoveDef { body, translate, rotation, copy: self.copy })),
            );
            Ok(json!({ "feature": name, "body": self.copy.then(|| name.clone()) }))
        })
    }
}

/// Merges tool bodies into a target body.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Combine {
    /// Body names to merge into the target; they disappear.
    pub tools: Vec<String>,
    /// `add` (default), `cut` or `intersect`.
    #[serde(default)]
    pub op: Option<String>,
    /// Target body (default: the latest body that is not a tool).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Combine {
    const ID: &'static str = "feature.combine";
    const TITLE: &'static str = "Combine bodies";
    const DESCRIPTION: &'static str = "Unite (add), subtract (cut) or intersect tool bodies with a target body; the tools are consumed. For holes, use `feature_hole` with several `at` points in one call. To pattern one feature: extrude it as its own body (op new), pattern that body, combine it into the base (cut for holes).";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let op = match self.op.as_deref() {
            None | Some("add") => BodyOp::Add,
            Some("cut") => BodyOp::Cut,
            Some("intersect") => BodyOp::Intersect,
            Some(o) => {
                return Err(CommandError::args(format!("unknown op '{o}' (add, cut, intersect)"))
                    .hint("op new does not apply: combine always merges into a target"));
            }
        };
        if self.tools.is_empty() {
            return Err(CommandError::args("tools is empty").hint("list the bodies to merge, e.g. [\"Boss\"]"));
        }
        edit(s, ctx, "Combine bodies", |doc, rb| {
            let at = rb.unwrap_or(doc.features.len()).min(doc.features.len());
            let live = live_body_features(doc, at);
            let names_of = |live: &[&Feature]| -> Vec<String> {
                live.iter().filter_map(|f| created_body_name(f)).map(str::to_string).collect()
            };
            // A tool or target may be named by its body or by the feature that created it.
            let resolve = |what: &str, given: &str| -> Result<String, CommandError> {
                live.iter()
                    .find(|f| f.name == given)
                    .or_else(|| live.iter().find(|f| created_body_name(f) == Some(given)))
                    .map(|f| f.name.clone())
                    .ok_or_else(|| {
                        CommandError::not_found(format!("no {what} body '{given}'"))
                            .hint(format!("bodies: {}", names_of(&live).join(", ")))
                    })
            };
            let mut tools: Vec<String> = Vec::new();
            for t in &self.tools {
                let f = resolve("tool", t)?;
                if !tools.contains(&f) {
                    tools.push(f);
                }
            }
            let body = match self.body.as_deref() {
                Some(b) => resolve("target", b)?,
                None => {
                    live.iter().rev().find(|f| !tools.contains(&f.name)).map(|f| f.name.clone()).ok_or_else(|| {
                        CommandError::args("there is no body besides the tools to combine them into")
                            .hint("name the target with body")
                    })?
                }
            };
            if tools.contains(&body) {
                let target = self.body.as_deref().unwrap_or(&body);
                return Err(CommandError::args(format!("'{target}' is both the target and a tool"))
                    .hint("a body cannot be combined with itself"));
            }
            let name = feature_name(doc, self.name, "Combine")?;
            let target_name = body_display_name(doc, &body);
            insert_feature(
                doc,
                rb,
                Feature::new(
                    name.clone(),
                    FeatureKind::Combine(CombineDef { body: Some(body), tools: tools.clone(), op }),
                ),
            );
            Ok(
                json!({ "feature": name, "target": target_name, "consumed": tools.iter().map(|t| body_display_name(doc, t)).collect::<Vec<_>>() }),
            )
        })
    }
}
