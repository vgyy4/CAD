//! `feature.edit`: changes arguments of an existing feature in place (DESIGN.md §1.5,
//! "edit in context"). The feature's one-line statement (as in the code view) is split into
//! its named arguments, the given ones are replaced, added or removed, and the statement is
//! parsed back; the name, kind and place in the history stay. Sketches are edited with the
//! sketch commands instead.

use super::*;
use gusset_doc::text::{parse_feature_object, statement_args, statement_with_args};
use std::collections::BTreeMap;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![spec::<FeatureEdit>(), spec::<FeatureReplaceReference>(), spec::<FeatureAdd>()]
}

/// Add one feature written as in the code view.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureAdd {
    /// e.g. `cam_job Job1(sketch: Sketch1, operation: pocket, tool_diameter: 6 mm, depth: 3 mm)`
    pub statement: String,
}

impl Command for FeatureAdd {
    const ID: &'static str = "feature.add";
    const TITLE: &'static str = "Add feature from text";
    const DESCRIPTION: &'static str = "Add one feature from its code-view statement (document_get_text shows the grammar), at the rollback point; for kinds without a command, e.g. cam_job.";
    const CATEGORY: Category = Category::Timeline;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let text = self.statement.trim();
        let f = parse_feature_object(text).map_err(|e| CommandError::args(e.to_string()))?;
        // Imports, components and sketches have their own commands (checked contents, ids).
        if matches!(
            f.kind,
            gusset_doc::FeatureKind::Import(_)
                | gusset_doc::FeatureKind::Component(_)
                | gusset_doc::FeatureKind::Sketch(_)
        ) {
            return Err(CommandError::args("imports, components and sketches have their own commands")
                .hint("import_file, assembly_insert_component, sketch_create"));
        }
        let name = f.name.clone();
        // Solids that add to or cut a body warn when they changed nothing, as their commands
        // do (dogfooding F11-9).
        let combine = match &f.kind {
            gusset_doc::FeatureKind::Extrude(d) => Some((d.op, d.body.clone())),
            gusset_doc::FeatureKind::Revolve(d) => Some((d.op, d.body.clone())),
            _ => None,
        };
        let target = std::cell::RefCell::new(None::<String>);
        edit_post(
            s,
            ctx,
            &format!("Add {name}"),
            |doc, rb| {
                let name = feature_name(doc, Some(f.name.clone()), "")?;
                if let Some((op, explicit)) = &combine {
                    *target.borrow_mut() = target_body(doc, rb, *op, explicit.clone())?;
                }
                insert_feature(doc, rb, f);
                Ok(json!({ "feature": name }))
            },
            |_, out, ch| match &combine {
                Some((op, _)) => combine_effect_warnings(&name, *op, target.borrow().as_deref(), &out.new_errors, ch),
                None => Value::Null,
            },
        )
    }
}

/// Replace a face or edge reference in a feature (accepting a rebuild error's suggestion).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureReplaceReference {
    pub name: String,
    /// The reference to replace, as written.
    pub from: String,
    pub to: String,
}

impl Command for FeatureReplaceReference {
    const ID: &'static str = "feature.replace_reference";
    const TITLE: &'static str = "Replace reference";
    const DESCRIPTION: &'static str =
        "Replace a face or edge reference in a feature, e.g. with a rebuild error's suggestion.";
    const CATEGORY: Category = Category::Timeline;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let name = self.name.clone();
        let to = self.to.trim();
        let one_ref = gusset_doc::refs::EdgeRef::parse(to).is_ok() || gusset_doc::refs::FaceRef::parse(to).is_ok();
        if !one_ref || to.contains(['\n', '\r']) {
            return Err(CommandError::args("the new reference must be one face or edge name"));
        }
        edit(s, ctx, &format!("Replace reference in {name}"), |doc, _| {
            let i = doc.feature_index(&name).ok_or_else(|| CommandError::not_found(format!("no feature '{name}'")))?;
            let f = doc.features.get(i).ok_or_else(|| CommandError::not_found(format!("no feature '{name}'")))?;
            let (head, mut args) =
                statement_args(f).ok_or_else(|| CommandError::args(format!("{name} is not a one-line feature")))?;
            let mut replaced = 0;
            for (_, v) in &mut args {
                let (out, n) = replace_token(v, &self.from, &self.to);
                *v = out;
                replaced += n;
            }
            if replaced == 0 {
                return Err(CommandError::not_found(format!("{name} does not refer to '{}'", self.from)));
            }
            let text = statement_with_args(&head, &args);
            let new = parse_feature_object(&text).map_err(|e| CommandError::args(format!("{}: {e}", text.trim())))?;
            if new.name != f.name || std::mem::discriminant(&new.kind) != std::mem::discriminant(&f.kind) {
                return Err(CommandError::args("the name and kind of a feature cannot change"));
            }
            if let Some(slot) = doc.features.get_mut(i) {
                *slot = new;
            }
            Ok(json!({ "feature": name, "replaced": replaced, "statement": text.trim() }))
        })
    }
}

/// Replaces whole occurrences of `from` in an argument value: bounded by the value's ends,
/// brackets, commas or spaces (so `E1/cap(end)` does not match inside a longer name).
fn replace_token(v: &str, from: &str, to: &str) -> (String, usize) {
    let boundary = |c: Option<char>| c.is_none_or(|c| matches!(c, '[' | ']' | ',' | ' '));
    let (mut out, mut rest, mut n) = (String::new(), v, 0);
    while let Some(at) = rest.find(from).filter(|_| !from.is_empty()) {
        // The character before the match: in `rest`, or the last one already written.
        let before = if at > 0 { rest.get(..at).and_then(|p| p.chars().last()) } else { out.chars().last() };
        let after = rest.get(at + from.len()..).and_then(|p| p.chars().next());
        out.push_str(rest.get(..at).unwrap_or(""));
        if boundary(before) && boundary(after) {
            out.push_str(to);
            n += 1;
        } else {
            out.push_str(from);
        }
        rest = rest.get(at + from.len()..).unwrap_or("");
    }
    out.push_str(rest);
    (out, n)
}

/// Change arguments of an existing feature in place.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureEdit {
    pub name: String,
    /// Arguments to set, as in the code view (`{"distance": "15 mm"}`).
    #[serde(default)]
    pub set: BTreeMap<String, String>,
    /// Arguments to remove (back to their defaults).
    #[serde(default)]
    pub remove: Vec<String>,
}

impl Command for FeatureEdit {
    const ID: &'static str = "feature.edit";
    const TITLE: &'static str = "Edit feature";
    const DESCRIPTION: &'static str =
        "Change arguments of an existing feature in place; values as in the code view. Not for sketches.";
    const CATEGORY: Category = Category::Timeline;
    const FLAGS: Flags = EDIT;
    fn run(self, s: &mut Session, ctx: &CmdCtx) -> Result<Value, CommandError> {
        let name = self.name.clone();
        edit(s, ctx, &format!("Edit {name}"), |doc, _| {
            let i = doc.feature_index(&name).ok_or_else(|| CommandError::not_found(format!("no feature '{name}'")))?;
            let f = doc.features.get(i).ok_or_else(|| CommandError::not_found(format!("no feature '{name}'")))?;
            // Imports and components carry checked file contents and pinned sources: only
            // their own commands may change them (review R4).
            if matches!(f.kind, gusset_doc::FeatureKind::Import(_) | gusset_doc::FeatureKind::Component(_)) {
                return Err(CommandError::args(format!("{name} is an import or a component"))
                    .hint("use import_file, assembly_update_component or assembly_configure_component"));
            }
            let (head, mut args) = statement_args(f).ok_or_else(|| {
                CommandError::args(format!("{name} is not a one-line feature"))
                    .hint("edit sketches with the sketch commands (sketch_add_constraint, sketch_move_point, ...)")
            })?;
            for k in &self.remove {
                args.retain(|(key, _)| key != k);
            }
            let ident =
                |k: &str| !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if let Some(k) = self.set.keys().chain(&self.remove).find(|k| !ident(k)) {
                return Err(CommandError::args(format!("'{k}' is not an argument name")));
            }
            for (k, v) in &self.set {
                if v.contains(['\n', '\r']) {
                    return Err(CommandError::args(format!("'{k}' must be one line")));
                }
                match args.iter_mut().find(|(key, _)| key == k) {
                    Some(slot) => slot.1.clone_from(v),
                    None => args.push((k.clone(), v.clone())),
                }
            }
            let text = statement_with_args(&head, &args);
            let new = parse_feature_object(&text).map_err(|e| CommandError::args(format!("{}: {e}", text.trim())))?;
            if new.name != f.name || std::mem::discriminant(&new.kind) != std::mem::discriminant(&f.kind) {
                return Err(CommandError::args("the name and kind of a feature cannot change"));
            }
            if let Some(slot) = doc.features.get_mut(i) {
                *slot = new;
            }
            Ok(json!({ "feature": name, "statement": text.trim() }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::replace_token;

    #[test]
    fn whole_references_are_replaced() {
        assert_eq!(replace_token("[E1/a, E1/ab, E1/a]", "E1/a", "E1/c"), ("[E1/c, E1/ab, E1/c]".into(), 2));
        assert_eq!(replace_token("E1/cap(end)", "E1/cap(end)", "E1/x"), ("E1/x".into(), 1));
        assert_eq!(replace_token("XE1/a", "E1/a", "Z"), ("XE1/a".into(), 0));
        assert_eq!(replace_token("abc", "", "Z"), ("abc".into(), 0));
    }
}
