//! Joints (ADR 0026): place a body by bringing a frame on it onto a frame on another body,
//! a plane or the origin, with the joint's motion in between.

use super::datum::{resolve_body, vec3};
use super::*;
use gusset_doc::{JointDef, JointKind};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<JointCommand>(), spec::<QueryJoints>()]
}

/// Adds a joint.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JointCommand {
    /// rigid|revolute|slider|cylindrical|pin_slot|planar|ball
    #[serde(rename = "type")]
    pub kind: String,
    /// The moving body.
    pub body: String,
    /// Bodies carried along.
    #[serde(default)]
    pub with: Vec<String>,
    /// Frame on the body (kinds as for `to`).
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
    pub free: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for JointCommand {
    const ID: &'static str = "feature.joint";
    const TITLE: &'static str = "Joint";
    const DESCRIPTION: &'static str = "Place a body: frame `at` on it onto frame `to`. Values: revolute angle; slider distance; cylindrical/pin_slot both; planar x, y, angle; ball angles. flip: face to face. On a placed body it closes a loop, solving `free` values.";
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
            free: self.free.clone(),
        };
        // The same checks as the text form (values the type does not move are refused).
        let probe = Feature::new("Probe", FeatureKind::Joint(Box::new(def.clone())));
        text::parse_feature_object(&text::print_feature_object(&probe))
            .map_err(|e| CommandError::args(e.to_string()))?;
        edit(s, ctx, "Joint", |doc, rb| {
            // Bodies may be named by body name; the document refers to the creating feature.
            let mut def = def;
            let at = rb.unwrap_or(doc.features.len()).min(doc.features.len());
            let live = live_body_features(doc, at);
            def.body = resolve_body(&live, "moving", &def.body)?;
            for w in &mut def.with {
                *w = resolve_body(&live, "carried", w)?;
            }
            let name = feature_name(doc, self.name, "Joint")?;
            insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::Joint(Box::new(def))));
            Ok(json!({ "feature": name }))
        })
    }
}

/// Lists the joints.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryJoints {}

impl Command for QueryJoints {
    const ID: &'static str = "query.joints";
    const TITLE: &'static str = "Joints";
    const DESCRIPTION: &'static str = "Every joint: type, moving body and bodies carried, frames, values and limits as evaluated, `free` values and `built_values` (as built, including what the loop solver chose), degrees of freedom left, build status and the moved body's centroid. Also the bodies no joint places.";
    const CATEGORY: Category = Category::Inspect;
    const FLAGS: Flags = READ;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let ev = s.evaluate(&ctx.actor)?;
        let doc = s.document(&ctx.actor)?.clone();
        let k = s.kernel().clone();
        let scope = |n: &str| ev.params.get(n).and_then(|r| r.as_ref().ok().copied());
        let value = |e: &gusset_units::expr::Expr| match gusset_units::expr::eval(e, &scope) {
            Ok(q) => json!(q.display(doc.units)),
            Err(err) => json!({ "expr": e.to_string(), "error": err.message }),
        };
        let pair = |p: &Option<[gusset_units::expr::Expr; 2]>| p.as_ref().map(|[a, b]| json!([value(a), value(b)]));
        let mut placed = std::collections::BTreeSet::new();
        let mut joints = Vec::new();
        for f in &doc.features {
            let FeatureKind::Joint(d) = &f.kind else { continue };
            let body = body_display_name(&doc, &d.body);
            let with: Vec<String> = d.with.iter().map(|w| body_display_name(&doc, w)).collect();
            placed.insert(body.clone());
            placed.extend(with.iter().cloned());
            let mut values = serde_json::Map::new();
            let named =
                [("angle", &d.angle), ("distance", &d.distance), ("x", &d.x), ("y", &d.y), ("offset", &d.offset)];
            for (key, e) in named {
                if let Some(e) = e {
                    values.insert(key.into(), value(e));
                }
            }
            if let Some(a) = &d.angles {
                values.insert("angles".into(), json!(a.iter().map(&value).collect::<Vec<_>>()));
            }
            // As built, including the values the loop solver chose (`free`).
            let built = ev.joint_values.get(&f.name).map(|v| {
                let q = |x: f64, dim| json!(gusset_units::Quantity::new(x, dim).display(doc.units));
                let (a, l) = (gusset_units::Dim::ANGLE, gusset_units::Dim::LENGTH);
                let mut m = serde_json::Map::new();
                let used = |key: &str| d.kind.has_value(key);
                for (key, x, dim) in [("angle", v.angle, a), ("distance", v.distance, l), ("x", v.x, l), ("y", v.y, l)]
                {
                    if used(key) {
                        m.insert(key.into(), q(x, dim));
                    }
                }
                if used("angles") {
                    m.insert("angles".into(), json!(v.angles.iter().map(|x| q(*x, a)).collect::<Vec<_>>()));
                }
                Value::Object(m)
            });
            let centroid = ev.body(&body).and_then(|b| k.mass_properties(&b.shape).ok()).map(|m| m.centroid);
            let mut v = json!({
                "name": f.name,
                "type": d.kind.keyword(),
                "body": body,
                "with": with,
                "at": text::print_frame(&d.at),
                "to": text::print_frame(&d.to),
                "flip": d.flip,
                "values": values,
                "free": d.free,
                "built_values": built,
                "angle_limits": pair(&d.angle_limits),
                "distance_limits": pair(&d.distance_limits),
                "dof": d.kind.dof(),
                "suppressed": f.suppressed,
                "status": ev.status(&f.name).map(status_json),
                "body_centroid_mm": centroid,
            });
            snap_zeros(&mut v);
            joints.push(v);
        }
        let free: Vec<&str> = ev.bodies.iter().map(|b| b.name.as_str()).filter(|n| !placed.contains(*n)).collect();
        Ok(json!({ "joints": joints, "unplaced_bodies": free }))
    }
}
