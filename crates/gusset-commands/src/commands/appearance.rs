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
    /// Only these faces: color colours them, clear drops their colour.
    #[serde(default)]
    pub faces: Option<Vec<String>>,
    /// wood, brushed, carbon, speckle or none.
    #[serde(default)]
    pub texture: Option<String>,
    /// Texture scale in mm.
    #[serde(default)]
    pub texture_size: Option<f64>,
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
            // Any case (dogfooding F14-3); the document writes it in lower case.
            Some(f) => Some(Finish::from_keyword(&f.trim().to_ascii_lowercase()).ok_or_else(|| {
                CommandError::args(format!("unknown finish '{}'", crate::paths::display_name(f)))
                    .hint("finish is plastic, metal, glass or rubber")
            })?),
            None => None,
        };
        let current = s.document(&ctx.actor)?.appearance(&self.body).and_then(|a| a.texture);
        let texture = self.texture_choice(current)?;
        if self.clear && (color.is_some() || finish.is_some() || texture.is_some()) {
            return Err(CommandError::args("clear takes no colour, finish or texture"));
        }
        if !self.clear && color.is_none() && finish.is_none() && texture.is_none() {
            return Err(CommandError::args("give a color, a finish, a texture, or clear: true"));
        }
        let ev = s.evaluate(&ctx.actor)?;
        // Bodies and reference meshes (an imported STL/3MF/OBJ keeps its colour too); an
        // appearance left behind by a body that is gone can still be cleared (review R2).
        let stale = self.clear && s.document(&ctx.actor)?.appearance(&self.body).is_some();
        if !stale && ev.body(&self.body).is_none() && !ev.meshes.iter().any(|m| m.name == self.body) {
            let names: Vec<&str> = ev.bodies.iter().take(20).map(|b| b.name.as_str()).collect();
            return Err(CommandError::not_found(format!("no body '{}'", crate::paths::display_name(&self.body)))
                .hint(format!("bodies: {}", names.join(", "))));
        }
        if let Some(names) = &self.faces {
            return set_face_colors(s, ctx, &ev, &self.body, names, color, finish.is_some() || texture.is_some());
        }
        let body = self.body.clone();
        edit(s, ctx, &format!("Appearance of {body}"), |doc, _| {
            let old = doc.appearances.iter().position(|a| a.body == body);
            if self.clear {
                // Clearing what is not set says so (dogfooding F14-9).
                let Some(i) = old else {
                    return Err(CommandError::args(format!("{body} has no appearance to clear")));
                };
                doc.appearances.remove(i);
                return Ok(json!({ "body": body, "appearance": null }));
            }
            let base = old.and_then(|i| doc.appearances.get(i).cloned());
            let a = Appearance {
                body: body.clone(),
                color: color.or(base.as_ref().map(|b| b.color)).unwrap_or(DEFAULT_COLOR),
                finish: finish.or(base.as_ref().map(|b| b.finish)).unwrap_or_default(),
                // A file's face colours stay; `clear` drops them with the rest.
                faces: base.as_ref().map(|b| b.faces.clone()).unwrap_or_default(),
                texture: match texture {
                    Some(t) => t,
                    None => base.and_then(|b| b.texture),
                },
            };
            let mut shown = json!({ "color": format!("#{:06x}", a.color), "finish": a.finish.keyword() });
            if let Some(t) = a.texture
                && let Some(o) = shown.as_object_mut()
            {
                o.insert("texture".into(), json!(t.kind.keyword()));
                o.insert("texture_size_mm".into(), json!(f64::from(t.size_um) / 1000.0));
            }
            let full = doc.appearances.len() >= gusset_doc::text::MAX_LIST_ITEMS;
            match old.and_then(|i| doc.appearances.get_mut(i)) {
                Some(slot) => *slot = a,
                None if full => {
                    return Err(CommandError::args("the document has as many appearances as it can hold")
                        .hint("clear the appearances of bodies that no longer need one"));
                }
                None => doc.appearances.push(a),
            }
            Ok(json!({ "body": body, "appearance": shown }))
        })
    }
}

impl SetAppearance {
    /// The texture asked for: `Some(None)` takes it off (`none`), `None` leaves it as is.
    /// `current`: the body's texture now; a size alone resizes it, and naming the same kind
    /// again keeps its size (review R6).
    fn texture_choice(
        &self,
        current: Option<gusset_doc::Texture>,
    ) -> Result<Option<Option<gusset_doc::Texture>>, CommandError> {
        let size = match self.texture_size {
            Some(mm) => {
                let um = (mm * 1000.0).round();
                let range = gusset_doc::TEXTURE_SIZE_UM;
                if !mm.is_finite() || !(f64::from(*range.start())..=f64::from(*range.end())).contains(&um) {
                    return Err(CommandError::args("texture_size must be 0.01 to 10000 mm"));
                }
                Some(um as u32)
            }
            None => None,
        };
        let Some(name) = &self.texture else {
            return match (size, current) {
                (Some(size_um), Some(t)) => Ok(Some(Some(gusset_doc::Texture { size_um, ..t }))),
                (Some(_), None) => {
                    Err(CommandError::args("texture_size needs a texture").hint("add texture: wood (or another)"))
                }
                (None, _) => Ok(None),
            };
        };
        let name = name.trim().to_ascii_lowercase();
        if name == "none" {
            return match size {
                Some(_) => Err(CommandError::args("texture none takes no size")),
                None => Ok(Some(None)),
            };
        }
        let kind = gusset_doc::TextureKind::from_keyword(&name).ok_or_else(|| {
            CommandError::args(format!("unknown texture '{}'", crate::paths::display_name(&name)))
                .hint("texture is wood, brushed, carbon, speckle or none")
        })?;
        let keep = current.filter(|t| t.kind == kind).map(|t| t.size_um);
        Ok(Some(Some(gusset_doc::Texture { kind, size_um: size.or(keep).unwrap_or_else(|| kind.default_size_um()) })))
    }
}

/// The viewport's default part colour, for a finish set without a colour.
const DEFAULT_COLOR: u32 = 0x00b4_bcc8;

/// `body.appearance` with `faces`: colours those faces (`color`) or drops their colour
/// (`color` none, i.e. `clear`). The body keeps its own colour and finish.
fn set_face_colors(
    s: &mut Session,
    ctx: &CmdCtx,
    ev: &gusset_eval::Evaluation,
    body: &str,
    names: &[String],
    color: Option<u32>,
    finish: bool,
) -> Result<Value, CommandError> {
    if finish {
        return Err(CommandError::args("faces take a colour, not a finish or texture")
            .hint("set the finish and texture on the whole body"));
    }
    let b = ev.body(body).ok_or_else(|| {
        CommandError::not_found(format!("no body '{}'", crate::paths::display_name(body)))
            .hint("faces of reference meshes cannot be coloured one by one")
    })?;
    if names.is_empty() {
        return Err(CommandError::args("give one or more faces"));
    }
    if names.len() > gusset_doc::text::MAX_LIST_ITEMS {
        return Err(CommandError::args(format!(
            "at most {} faces at once ({} given)",
            gusset_doc::text::MAX_LIST_ITEMS,
            names.len()
        )));
    }
    // A face name as given, for messages: control characters out, cut to 200 (F15-6).
    let shown = |n: &str| n.chars().filter(|c| !c.is_control()).take(200).collect::<String>();
    let own: std::collections::HashSet<String> = b.names.faces.iter().map(ToString::to_string).collect();
    let mut faces = Vec::with_capacity(names.len());
    let mut seen = std::collections::HashSet::new();
    for n in names {
        let f = gusset_doc::text::parse_face_ref(n.trim()).ok().filter(|f| own.contains(&f.to_string())).ok_or_else(
            || {
                let hint = if n.contains("), ") {
                    "one face per list entry; query_body lists its faces"
                } else {
                    "query_body lists its faces"
                };
                CommandError::not_found(format!("{body} has no face '{}'", shown(n))).hint(hint)
            },
        )?;
        if seen.insert(f.to_string()) {
            faces.push(f);
        }
    }
    let body = body.to_string();
    edit(s, ctx, &format!("Face colours of {body}"), |doc, _| {
        let old = doc.appearances.iter().position(|a| a.body == body);
        if color.is_none() && old.is_none() {
            return Err(CommandError::args(format!("{body} has no face colours to clear")));
        }
        let total: usize = doc.appearances.iter().flat_map(|a| &a.faces).map(|g| g.faces.len()).sum();
        if color.is_some() && total + faces.len() > gusset_doc::text::MAX_LIST_ITEMS {
            return Err(CommandError::args("the document has as many face colours as it can hold"));
        }
        if old.is_none() && doc.appearances.len() >= gusset_doc::text::MAX_LIST_ITEMS {
            return Err(CommandError::args("the document has as many appearances as it can hold"));
        }
        let i = match old {
            Some(i) => i,
            None => {
                doc.appearances.push(Appearance {
                    body: body.clone(),
                    color: DEFAULT_COLOR,
                    finish: Finish::default(),
                    faces: Vec::new(),
                    texture: None,
                });
                doc.appearances.len() - 1
            }
        };
        let Some(a) = doc.appearances.get_mut(i) else { return Err(CommandError::args("the appearance went missing")) };
        let before: usize = a.faces.iter().map(|g| g.faces.len()).sum();
        // By name in a set: linear in the faces listed (face colours review #5).
        let named: std::collections::HashSet<String> = faces.iter().map(ToString::to_string).collect();
        for g in &mut a.faces {
            g.faces.retain(|f| !named.contains(&f.to_string()));
        }
        // A face in the body's own colour needs no entry (F15-7).
        let own = color.is_some_and(|c| c == a.color);
        if let Some(c) = color.filter(|_| !own) {
            match a.faces.iter_mut().find(|g| g.color == c) {
                Some(g) => g.faces.extend(faces.iter().cloned()),
                None => a.faces.push(gusset_doc::FaceColor { color: c, faces: faces.clone() }),
            }
        }
        a.faces.retain(|g| !g.faces.is_empty());
        let total: usize = a.faces.iter().map(|g| g.faces.len()).sum();
        let mut out = json!({ "body": body, "faces": faces.len(), "faces_coloured_in_body": total });
        let note = if color.is_none() && total == before {
            Some("none of these faces had a colour of its own")
        } else if own {
            Some("that is the body's colour: the faces now show it without a colour of their own")
        } else {
            None
        };
        if let (Some(n), Some(o)) = (note, out.as_object_mut()) {
            o.insert("note".into(), json!(n));
        }
        Ok(out)
    })
}
