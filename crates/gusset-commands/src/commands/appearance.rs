//! `body.appearance`: a body's colour and finish (ADR 0045), stored in the document and
//! used by the viewport and the path-traced renders. Geometry does not change.

use super::*;
use gusset_doc::{Appearance, Finish};

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<SetAppearance>()]
}

/// Sets or clears a body's appearance.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAppearance {
    pub body: String,
    /// "#rrggbb".
    #[serde(default)]
    pub color: Option<String>,
    /// plastic, metal, glass or rubber.
    #[serde(default)]
    pub finish: Option<String>,
    /// Back to the default look.
    #[serde(default)]
    pub clear: bool,
}

impl Command for SetAppearance {
    const ID: &'static str = "body.appearance";
    const TITLE: &'static str = "Appearance";
    const DESCRIPTION: &'static str =
        "Set a body's colour and finish (plastic, metal, glass, rubber) for views and renders.";
    const CATEGORY: Category = Category::Document;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let color = match &self.color {
            Some(c) => Some(gusset_doc::text::parse_color(c.trim()).ok_or_else(|| {
                CommandError::args(format!("'{}' is not a colour", crate::paths::display_name(c)))
                    .hint("write \"#\" and six hex digits, like \"#8a9bb0\"")
            })?),
            None => None,
        };
        let finish = match &self.finish {
            Some(f) => Some(
                Finish::from_keyword(f.trim())
                    .ok_or_else(|| CommandError::args("finish is plastic, metal, glass or rubber"))?,
            ),
            None => None,
        };
        if self.clear && (color.is_some() || finish.is_some()) {
            return Err(CommandError::args("clear takes no colour or finish"));
        }
        if !self.clear && color.is_none() && finish.is_none() {
            return Err(CommandError::args("give a color, a finish, or clear: true"));
        }
        let ev = s.evaluate(&ctx.actor)?;
        if ev.body(&self.body).is_none() {
            let names: Vec<&str> = ev.bodies.iter().take(20).map(|b| b.name.as_str()).collect();
            return Err(CommandError::not_found(format!("no body '{}'", crate::paths::display_name(&self.body)))
                .hint(format!("bodies: {}", names.join(", "))));
        }
        let body = self.body.clone();
        edit(s, ctx, &format!("Appearance of {body}"), |doc, _| {
            let old = doc.appearances.iter().position(|a| a.body == body);
            if self.clear {
                if let Some(i) = old {
                    doc.appearances.remove(i);
                }
                return Ok(json!({ "body": body, "appearance": null }));
            }
            let base = old.and_then(|i| doc.appearances.get(i).cloned());
            let a = Appearance {
                body: body.clone(),
                color: color.or(base.as_ref().map(|b| b.color)).unwrap_or(DEFAULT_COLOR),
                finish: finish.or(base.as_ref().map(|b| b.finish)).unwrap_or_default(),
            };
            let shown = json!({ "color": format!("#{:06x}", a.color), "finish": a.finish.keyword() });
            match old.and_then(|i| doc.appearances.get_mut(i)) {
                Some(slot) => *slot = a,
                None => doc.appearances.push(a),
            }
            Ok(json!({ "body": body, "appearance": shown }))
        })
    }
}

/// The viewport's default part colour, for a finish set without a colour.
const DEFAULT_COLOR: u32 = 0x00b4_bcc8;
