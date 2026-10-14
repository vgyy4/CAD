//! Joints (ADR 0026): place a body by bringing a frame on it onto a frame on another body,
//! a plane or the origin, with the joint's motion in between.

use super::datum::vec3;
use super::*;
use gusset_doc::{JointDef, JointKind};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<JointCommand>()]
}

/// Adds a joint.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JointCommand {
    /// rigid, revolute, slider, cylindrical, pin_slot, planar, ball.
    #[serde(rename = "type")]
    pub kind: String,
    /// The moving body.
    pub body: String,
    /// Bodies carried along.
    #[serde(default)]
    pub with: Vec<String>,
    /// Frame on the body: face, edge(..), plane or origin.
    pub at: String,
    /// Target frame: face, edge(..), plane or origin.
    pub to: String,
    #[serde(default)]
    pub angle: Option<String>,
    #[serde(default)]
    pub distance: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub y: Option<String>,
    #[serde(default)]
    pub angles: Option<[String; 3]>,
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub offset: Option<String>,
    #[serde(default)]
    pub angle_limits: Option<[String; 2]>,
    #[serde(default)]
    pub distance_limits: Option<[String; 2]>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for JointCommand {
    const ID: &'static str = "feature.joint";
    const TITLE: &'static str = "Joint";
    const DESCRIPTION: &'static str = "Place a body by a joint: frame `at` on it goes onto frame `to` (a placed body, plane or origin). Values: revolute angle; slider distance; cylindrical/pin_slot both; planar x, y, angle; ball angles. flip: face against face.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let kind = JointKind::from_keyword(&self.kind).ok_or_else(|| {
            CommandError::args(format!("unknown joint type '{}'", self.kind))
                .hint("use rigid, revolute, slider, cylindrical, pin_slot, planar or ball")
        })?;
        let frame = |what: &str, v: &str| {
            text::parse_frame_ref(v).map_err(|e| {
                CommandError::args(format!("{what}: {e}"))
                    .hint("a face like Lid/cap(start), edge(A/..., B/...), XY, a datum plane or origin")
            })
        };
        let opt = |what: &str, v: &Option<String>| v.as_deref().map(|e| parse_expr(what, e)).transpose();
        let pair = |what: &str, v: &Option<[String; 2]>| {
            v.as_ref().map(|[a, b]| Ok::<_, CommandError>([parse_expr(what, a)?, parse_expr(what, b)?])).transpose()
        };
        let def = JointDef {
            kind,
            body: self.body.clone(),
            with: self.with.clone(),
            at: frame("at", &self.at)?,
            to: frame("to", &self.to)?,
            angle: opt("angle", &self.angle)?,
            distance: opt("distance", &self.distance)?,
            x: opt("x", &self.x)?,
            y: opt("y", &self.y)?,
            angles: self.angles.as_ref().map(|a| vec3("angles", a)).transpose()?,
            flip: self.flip,
            offset: opt("offset", &self.offset)?,
            angle_limits: pair("angle_limits", &self.angle_limits)?,
            distance_limits: pair("distance_limits", &self.distance_limits)?,
        };
        // The same checks as the text form (values the type does not move are refused).
        let probe = Feature::new("Probe", FeatureKind::Joint(Box::new(def.clone())));
        text::parse_feature_object(&text::print_feature_object(&probe))
            .map_err(|e| CommandError::args(e.to_string()))?;
        edit(s, ctx, "Joint", |doc, rb| {
            let name = feature_name(doc, self.name, "Joint")?;
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::Joint(Box::new(def))));
            Ok(json!({ "feature": name }))
        })
    }
}
