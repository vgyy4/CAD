//! The text feature (ADR 0029): embossed or engraved text on a plane.

use super::*;
use gusset_doc::{TextAlign, TextDef, TextFont};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<Text>()]
}

/// Adds text extruded from a plane.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Text {
    /// One line, ≤200 chars.
    pub text: String,
    /// XY/XZ/YZ, planar face or datum plane.
    pub plane: String,
    /// Baseline start [x, y] (default origin).
    #[serde(default)]
    pub at: Option<[String; 2]>,
    /// Capital-letter height (`5 mm`).
    pub height: String,
    /// Along the plane normal (`1 mm`); negative to cut into a face.
    pub distance: String,
    /// sans (default) or mono.
    #[serde(default)]
    pub font: Option<String>,
    /// left (default), center, right.
    #[serde(default)]
    pub align: Option<String>,
    /// Rotation about `at`.
    #[serde(default)]
    pub angle: Option<String>,
    /// new (default), add, cut, intersect.
    #[serde(default)]
    pub op: Option<String>,
    /// Target body for add/cut/intersect (default: the latest body).
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub body_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Command for Text {
    const ID: &'static str = "feature.text";
    const TITLE: &'static str = "Text";
    const DESCRIPTION: &'static str =
        "Emboss or engrave one line of text on a plane or face (extruded letters: new, add or cut). Needs OCCT.";
    const CATEGORY: Category = Category::Solid;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let chars = self.text.chars().count();
        if chars == 0 || chars > gusset_doc::validate::MAX_TEXT_CHARS {
            return Err(CommandError::args(format!("the text has {chars} characters"))
                .hint(format!("use 1 to {} characters", gusset_doc::validate::MAX_TEXT_CHARS)));
        }
        let height = parse_expr("height", &self.height)?;
        let distance = parse_expr("distance", &self.distance)?;
        let [ax, ay] = self.at.clone().unwrap_or_else(|| ["0 mm".into(), "0 mm".into()]);
        let at = [parse_expr("at", &ax)?, parse_expr("at", &ay)?];
        let angle = self.angle.as_deref().map(|a| parse_expr("angle", a)).transpose()?;
        let font = match self.font.as_deref() {
            None => TextFont::Sans,
            Some(f) => TextFont::from_keyword(f)
                .ok_or_else(|| CommandError::args(format!("unknown font '{f}'")).hint("use sans or mono"))?,
        };
        let align = match self.align.as_deref() {
            None => TextAlign::Left,
            Some(a) => TextAlign::from_keyword(a)
                .ok_or_else(|| CommandError::args(format!("unknown align '{a}'")).hint("use left, center or right"))?,
        };
        let op = op_of(&self.op)?;
        let plane = super::datum::parse_plane_arg(s, ctx, &self.plane)?;
        let seen = std::cell::RefCell::new((String::new(), None::<String>));
        edit_post(
            s,
            ctx,
            "Text",
            |doc, rb| {
                let name = feature_name(doc, self.name, "Text")?;
                let body = target_body(doc, rb, op, self.body)?;
                let body_name = check_new_body_name(doc, op, &name, self.body_name)?;
                *seen.borrow_mut() = (name.clone(), body.as_deref().map(|t| body_display_name(doc, t)));
                let created = (op == BodyOp::New).then(|| body_name.clone().unwrap_or_else(|| name.clone()));
                let def =
                    TextDef { text: self.text, plane, at, height, distance, font, align, angle, op, body, body_name };
                insert_feature(doc, rb, Feature::new(name.clone(), FeatureKind::Text(Box::new(def))));
                Ok(json!({ "feature": name, "body": created }))
            },
            |_, out, ch| {
                let (name, body) = &*seen.borrow();
                combine_effect_warnings(name, op, body.as_deref(), &out.new_errors, ch)
            },
        )
    }
}
